//! The internet transport under the session (docs/design/coop-networking.md §3-§9): one
//! [`seam::Transport`] that the session ([`crate::net`]) sends its messages through and
//! takes them from, by address, as it would plain UDP. Under it:
//!
//! - the game's sockets ([`Sockets`]: IPv4 and IPv6, the hosted relay's sessions, the
//!   birthday punch's sockets);
//! - Noise ([`crate::noise::Secure`]): every message sealed for one peer, a key per player;
//!   nothing reaches the session that did not open on a session, and the host's admission
//!   ([`KeyAdmission`]) takes the key the sender's session proved;
//! - for an online code, the matchmaking service and the checks that find a path
//!   ([`crate::online::Online`]: STUN, hole punching, the router's port mapping, the hosted
//!   and peer relays). Their packets share the game's sockets and never reach the session.
//!
//! **A guest's host by an online code** is [`ONLINE_HOST`] to the session: a stand-in
//! address that is never sent to. What the session sends there goes to wherever the checks
//! found the host (directly, through the hosted relay, or through another guest), and
//! what the host's session sends comes as from there, so a path that changes (a relay
//! taken after the direct path was lost) is not seen above.
//!
//! **The peer relay** (§8): a guest that relays for another player passes that player's
//! packets to the host as they are (sealed for the host, which this guest cannot open),
//! inside a frame on its own session with the host ([`RELAY_TAG`], a sub-channel byte, the
//! packet); the host opens them as from a stand-in address of their own ([`relay_addr`]),
//! and its answers go back the same way.
//!
//! The game keeps a [`Control`] to the stack it handed the session: the service's events
//! (the code, a player asking to join, failures, notes for the log), its answers to join
//! asks, and the key of the player at each address (for Kick and Ban, and the lobby list's
//! paths).

use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nvf_signal::wire::PeerNo;

use crate::noise::{Identity, Opened, Secure};
use crate::online::{HostInfo, Online, OnlineEvent, SocketAsk};
use crate::seam::{self, Admit, Admission};
use crate::sockets::{self, Sockets};

/// The session's address for a guest's host by an online code (module notes): never a
/// real host (the broadcast network's last address but one, port 1), never sent to.
pub const ONLINE_HOST: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 254)), 1);

/// The first bytes of a peer-relay frame's plaintext (module notes); a game message starts
/// with `net::MAGIC`.
pub const RELAY_TAG: [u8; 4] = *b"NVRF";
/// Messages kept for the host while a guest's handshake is under way (the newest).
const WAITING_MAX: usize = 16;
/// A guest's handshake message 1 goes again at most this often to one address.
const HANDSHAKE_EVERY: Duration = Duration::from_millis(250);
/// The host's unknown-session hint goes at most this often to one address.
const HINT_EVERY: Duration = Duration::from_millis(500);
/// The service, the checks and the relays' Binds are looked at at most this often.
const PUMP_EVERY: Duration = Duration::from_millis(5);
/// Other players a guest relays over the network at most (`nvf_nat::relay::Policy`'s
/// default; docs/design/coop-networking.md §8).
pub const PEER_RELAYS: usize = 2;
/// A relayed player's sub-channels start here (below: kept for split screen's games behind
/// a guest, which this part does not have).
const PEER_SUB: u8 = 0x80;
/// A relayed player silent this long is dropped, here and in the checks' count.
const PEER_IDLE: Duration = Duration::from_secs(60);
/// Addresses a relayed player is known at (the checks' and the one it moved to).
const PEER_ADDRS: usize = 4;
/// The stand-in addresses of games relayed through a guest, on the host: an IPv6 prefix
/// no network uses ([documented: RFC 3849, 2001:db8::/32 is for documentation only]).
const RELAY_PREFIX: u64 = 0x2001_0db8_4e56_5246;

/// The stand-in address the host knows the `i`th relayed game by.
pub fn relay_addr(i: usize) -> SocketAddr {
    SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::from((u128::from(RELAY_PREFIX) << 64) | i as u128), 0, 0, 0))
}

/// Which relayed game `addr` stands in for, if it is one ([`relay_addr`]).
pub fn relay_index(addr: SocketAddr) -> Option<usize> {
    match addr {
        SocketAddr::V6(a) if (u128::from(*a.ip()) >> 64) as u64 == RELAY_PREFIX => usize::try_from(u128::from(*a.ip()) & u128::from(u64::MAX)).ok(),
        _ => None,
    }
}

/// The token a player's key stands for in its Hello: the key's first 8 bytes (never 0).
/// A guest sends it; the host's [`KeyAdmission`] lets in only the token of the key the
/// Hello's session proved, so no one takes another player's slot.
pub fn token_of(key: &[u8; 32]) -> u64 {
    u64::from_le_bytes(key[..8].try_into().expect("8 bytes")).max(1)
}

/// How a host's guest reaches it, for the lobby list (docs/design/coop-networking.md §10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// On the host's own network.
    Lan,
    /// Over the internet, straight.
    Direct,
    /// Through another guest (the peer relay).
    Relayed,
    /// Through a relay a player hosts (`nvf-relay`).
    HostedRelay,
}

impl Path {
    /// What the lobby list says.
    pub fn word(self) -> &'static str {
        match self {
            Path::Lan => "local network",
            Path::Direct => "direct",
            Path::Relayed => "through another player",
            Path::HostedRelay => "through a relay",
        }
    }

    /// The path of a guest at `addr` (as the host's session knows it).
    pub fn of(addr: SocketAddr) -> Self {
        if relay_index(addr).is_some() {
            Path::Relayed
        } else if sockets::is_hosted_standin(addr) {
            Path::HostedRelay
        } else if nvf_nat::candidate::is_lan(addr.ip()) {
            Path::Lan
        } else {
            Path::Direct
        }
    }
}

/// What the game asks of the stack, taken on its next pump.
enum Ask {
    Admit(PeerNo, bool),
    Ban([u8; 32]),
    Listing { public: bool, title: String, place: String, friendly_fire: bool },
}

#[derive(Default)]
struct Shared {
    /// The key of the peer at each address (as the session knows it), once its session is
    /// made.
    keys: HashMap<SocketAddr, [u8; 32]>,
    banned: Vec<[u8; 32]>,
    events: Vec<OnlineEvent>,
    asks: Vec<Ask>,
}

/// Bounds what the game has not read yet.
const EVENTS_MAX: usize = 256;

fn lock(s: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// The game's handle on a [`Stack`] it gave the session (module notes).
#[derive(Clone)]
pub struct Control(Arc<Mutex<Shared>>);

impl Control {
    /// The service's events since the last call, oldest first.
    pub fn events(&self) -> Vec<OnlineEvent> {
        std::mem::take(&mut lock(&self.0).events)
    }

    /// The host's answer to player `peer` asking to join ([`OnlineEvent::JoinAsk`]).
    pub fn admit(&self, peer: PeerNo, yes: bool) {
        lock(&self.0).asks.push(Ask::Admit(peer, yes));
    }

    /// The host's listing in the lobby browser.
    pub fn set_listing(&self, public: bool, title: &str, place: &str, friendly_fire: bool) {
        lock(&self.0).asks.push(Ask::Listing { public, title: title.into(), place: place.into(), friendly_fire });
    }

    /// The key of the player at `addr`, once its session is made.
    pub fn key_at(&self, addr: SocketAddr) -> Option<[u8; 32]> {
        lock(&self.0).keys.get(&addr).copied()
    }

    /// Whether `key` is banned in this lobby.
    pub fn is_banned(&self, key: &[u8; 32]) -> bool {
        lock(&self.0).banned.contains(key)
    }
}

/// The host's admission by key (module notes, [`token_of`]); on a guest, every answer that
/// reached the session came on a session it made itself with its host (pinned to the
/// service's host key for an online code), so it takes them.
pub struct KeyAdmission {
    shared: Arc<Mutex<Shared>>,
}

impl Admission for KeyAdmission {
    fn hello(&self) -> u64 {
        1
    }

    /// Only [`Admission::admits_from`] lets anyone in: the key decides, not the proof.
    fn admits(&self, _proof: u64) -> bool {
        false
    }

    fn answer(&self, _token: u64, _id: Option<u8>) -> u64 {
        1
    }

    fn answered(&self, _token: u64, _id: Option<u8>, _proof: u64) -> bool {
        true
    }

    fn admits_from(&self, _proof: u64, token: u64, from: SocketAddr) -> Admit {
        let s = lock(&self.shared);
        match s.keys.get(&from) {
            Some(k) if s.banned.contains(k) => Admit::Banned,
            Some(k) if token_of(k) == token => Admit::Yes,
            _ => Admit::No,
        }
    }

    fn ban(&mut self, at: SocketAddr) -> bool {
        let mut s = lock(&self.shared);
        let Some(key) = s.keys.get(&at).copied() else { return false };
        if !s.banned.contains(&key) {
            s.banned.push(key);
            s.asks.push(Ask::Ban(key));
        }
        true
    }
}

/// A player this guest relays for (the peer relay's helper side).
struct PeerRelay {
    peer: PeerNo,
    allowed: VecDeque<SocketAddr>,
    addr: Option<SocketAddr>,
    heard: Instant,
}

/// The internet transport (module notes); its state, in a cell because the session sends
/// through `&self`.
pub struct Stack(std::cell::RefCell<Inner>);

struct Inner {
    socks: Sockets,
    secure: Secure,
    host: bool,
    online: Option<Online>,
    shared: Arc<Mutex<Shared>>,
    inbox: VecDeque<(Vec<u8>, SocketAddr)>,
    last_pump: Option<Instant>,
    // A guest's:
    /// Messages to the host while there is no session yet.
    waiting: VecDeque<Vec<u8>>,
    /// When message 1 last went to each address.
    hs_sent: HashMap<SocketAddr, Instant>,
    /// The address its session with the host is at.
    host_path: Option<SocketAddr>,
    /// Where the checks found the host (an online code).
    online_path: Option<SocketAddr>,
    host_heard: Option<Instant>,
    host_lost: bool,
    helping: Vec<Option<PeerRelay>>,
    // A host's:
    hinted: HashMap<SocketAddr, Instant>,
    /// The games relayed through its guests: (the guest's address, the sub-channel).
    relayed: Vec<(SocketAddr, u8)>,
}

impl Inner {
    fn new(socks: Sockets, me: Identity, host: bool, lobby: Option<u32>) -> (Self, Control) {
        let mut secure = Secure::new(me);
        secure.set_lobby(lobby);
        let shared = Arc::new(Mutex::new(Shared::default()));
        let s = Self {
            socks,
            secure,
            host,
            online: None,
            shared: shared.clone(),
            inbox: VecDeque::new(),
            last_pump: None,
            waiting: VecDeque::new(),
            hs_sent: HashMap::new(),
            host_path: None,
            online_path: None,
            host_heard: None,
            host_lost: false,
            helping: Vec::new(),
            hinted: HashMap::new(),
            relayed: Vec::new(),
        };
        (s, Control(shared))
    }

    /// This PC's addresses at this stack's ports, for the other side's checks.
    fn local_candidates(&self) -> Vec<SocketAddr> {
        let v4 = self.socks.local_addr().ok().filter(SocketAddr::is_ipv4).map(|a| a.port());
        let v6 = self.socks.local_addr_v6().map(|a| a.port());
        crate::online::local_addrs(v4, v6)
    }

    fn event(&self, e: OnlineEvent) {
        let mut s = lock(&self.shared);
        if s.events.len() < EVENTS_MAX {
            s.events.push(e);
        }
    }

    /// A packet of this layer to `to`: a game relayed through a guest gets it in a frame
    /// on that guest's session, anyone else straight.
    fn route(&mut self, bytes: &[u8], to: SocketAddr) {
        match relay_index(to) {
            Some(i) => {
                let Some(&(via, sub)) = self.relayed.get(i) else { return };
                let mut frame = Vec::with_capacity(bytes.len() + 5);
                frame.extend_from_slice(&RELAY_TAG);
                frame.push(sub);
                frame.extend_from_slice(bytes);
                self.seal_to(&frame, via);
            }
            None => self.socks.send(bytes, to),
        }
    }

    /// `plain` sealed on the session at `to` and sent; false without a session.
    fn seal_to(&mut self, plain: &[u8], to: SocketAddr) -> bool {
        let Some(packets) = self.secure.seal(to, plain) else { return false };
        for p in packets {
            self.route(&p, to);
        }
        true
    }

    /// A guest: where what the session sends to `to` goes.
    fn guest_target(&self, to: SocketAddr) -> Option<SocketAddr> {
        if self.online.is_some() || to == ONLINE_HOST {
            return self.host_path.or(self.online_path);
        }
        Some(self.host_path.unwrap_or(to))
    }

    fn guest_send(&mut self, bytes: &[u8], to: SocketAddr, now: Instant) {
        let Some(target) = self.guest_target(to) else {
            self.wait(bytes);
            return;
        };
        if self.secure.has_session(target) {
            self.seal_to(bytes, target);
            return;
        }
        self.wait(bytes);
        if self.hs_sent.get(&target).is_none_or(|t| now.saturating_duration_since(*t) >= HANDSHAKE_EVERY) {
            self.hs_sent.insert(target, now);
            let m1 = self.secure.initiate(target, now);
            self.socks.send(&m1, target);
        }
    }

    fn wait(&mut self, bytes: &[u8]) {
        if self.waiting.len() == WAITING_MAX {
            self.waiting.pop_front();
        }
        self.waiting.push_back(bytes.to_vec());
    }

    /// The service, the checks, the relays and what the game asked, once a pump.
    fn pump(&mut self, now: Instant) {
        self.socks.hosted_poll(now);
        self.secure.prune(now);
        let asks = std::mem::take(&mut lock(&self.shared).asks);
        for a in asks {
            let Some(o) = self.online.as_mut() else { continue };
            match a {
                Ask::Admit(peer, yes) => o.admit(peer, yes),
                Ask::Ban(key) => o.ban(key),
                Ask::Listing { public, title, place, friendly_fire } => o.set_listing(public, &title, &place, friendly_fire),
            }
        }
        // A guest whose host went quiet: a new session, and with an online code new checks.
        if !self.host && !self.host_lost && self.host_heard.is_some_and(|t| now.saturating_duration_since(t) >= crate::net::LOST_AFTER) {
            self.host_lost = true;
            if let Some(h) = self.host_path.take() {
                self.secure.forget(h);
            }
            if let Some(o) = &mut self.online {
                o.path_lost();
            }
        }
        // Relayed players gone quiet.
        for i in 0..self.helping.len() {
            if let Some(r) = self.helping[i].take_if(|r| now.saturating_duration_since(r.heard) >= PEER_IDLE) {
                if let Some(o) = &mut self.online {
                    o.relay_ended(r.peer);
                }
                self.event(OnlineEvent::RelayEnd(r.peer));
            }
        }
        let key = self.secure.identity().public();
        let Some(o) = self.online.as_mut() else { return };
        let send = o.tick(now, &key);
        for (to, b) in send {
            self.socks.send(&b, to);
        }
        // The birthday punch's sockets (docs/spec-notes/nat-traversal.md).
        for ask in o.take_socket_asks() {
            match ask {
                SocketAsk::Open { owner, count } => {
                    let ports = self.socks.open_extras(owner, count);
                    o.extras_opened(owner, &ports);
                }
                SocketAsk::Close { owner, keep } => self.socks.close_extras(owner, keep),
            }
        }
        for (port, to, b) in o.take_sends_from() {
            self.socks.send_from(&b, Some(port), to);
        }
        for e in o.take_events() {
            match &e {
                OnlineEvent::HostAt(at) if self.online_path.is_none() => self.online_path = Some(*at),
                // Through a relay: the handshake goes to the helper, from now on.
                OnlineEvent::HostVia(at) => {
                    if let Some(h) = self.host_path.take() {
                        self.secure.forget(h);
                    }
                    self.online_path = Some(*at);
                }
                OnlineEvent::HostedOpen { relay, session, side } => {
                    self.socks.hosted_open(*relay, *session, *side);
                }
                OnlineEvent::RelayFor(peer) => self.relay_for(*peer, now),
                OnlineEvent::RelayAt { peer, addr } => self.relay_at(*peer, *addr),
                OnlineEvent::RelayEnd(peer) => {
                    for r in &mut self.helping {
                        if r.as_ref().is_some_and(|r| r.peer == *peer) {
                            *r = None;
                        }
                    }
                }
                _ => {}
            }
            self.event(e);
        }
    }

    /// A guest relays for `peer` (module notes), with [`PEER_RELAYS`] at most.
    fn relay_for(&mut self, peer: PeerNo, now: Instant) {
        if self.host || self.helping.iter().flatten().any(|r| r.peer == peer) {
            return;
        }
        let slot = match self.helping.iter().position(Option::is_none) {
            Some(i) => i,
            None if self.helping.len() < PEER_RELAYS => {
                self.helping.push(None);
                self.helping.len() - 1
            }
            None => return,
        };
        self.helping[slot] = Some(PeerRelay { peer, allowed: VecDeque::new(), addr: None, heard: now });
    }

    fn relay_at(&mut self, peer: PeerNo, addr: SocketAddr) {
        let addr = sockets::canonical(addr);
        if let Some(r) = self.helping.iter_mut().flatten().find(|r| r.peer == peer) {
            if !r.allowed.contains(&addr) {
                if r.allowed.len() == PEER_ADDRS {
                    r.allowed.pop_front();
                }
                r.allowed.push_back(addr);
            }
            r.addr.get_or_insert(addr);
        }
    }

    /// Everything waiting on the sockets, sorted: the service's and the checks' packets to
    /// [`Online`], a relayed player's on to the host, the rest opened.
    fn drain(&mut self, now: Instant) {
        let mut got = Vec::new();
        self.socks.recv_via(|b, from, via| got.push((b.to_vec(), from, via)));
        for (bytes, from, via) in got {
            if let Some(o) = &mut self.online {
                if nvf_nat::stun::is_stun(&bytes) {
                    o.on_stun(from, &bytes);
                    continue;
                }
                if crate::online::is_probe(&bytes) {
                    // Answered from the socket it came in on.
                    if let Some((to, b)) = o.on_probe(&bytes, from, via, now) {
                        self.socks.send_from(&b, via, to);
                    }
                    continue;
                }
                if o.on_router(from, &bytes, now) {
                    continue;
                }
            }
            // The game's packet on a birthday punch socket: that socket is the way back.
            if let Some(port) = via {
                self.socks.pin(from, port);
            }
            if !self.host {
                if let Some(i) = self.helping.iter().position(|r| r.as_ref().is_some_and(|r| r.allowed.contains(&from))) {
                    self.relay_up(i, &bytes, from, now);
                    continue;
                }
            }
            self.open(&bytes, from, now);
        }
    }

    /// A guest: a packet of a player it relays, to the host in a frame (module notes).
    fn relay_up(&mut self, i: usize, bytes: &[u8], from: SocketAddr, now: Instant) {
        if let Some(r) = &mut self.helping[i] {
            r.addr = Some(from);
            r.heard = now;
        }
        let Some(h) = self.host_path.filter(|_| !self.host_lost) else { return };
        let mut frame = Vec::with_capacity(bytes.len() + 5);
        frame.extend_from_slice(&RELAY_TAG);
        frame.push(PEER_SUB + i as u8);
        frame.extend_from_slice(bytes);
        self.seal_to(&frame, h);
    }

    /// A packet from `from` (a relayed game: its stand-in address), opened on its session.
    fn open(&mut self, bytes: &[u8], from: SocketAddr, now: Instant) {
        match self.secure.open(bytes, from, now) {
            Opened::Message { bytes, from } => self.message(bytes, from, now),
            Opened::Handshake { send, made } => {
                // A guest takes no handshake it did not start (it hosts nothing).
                for (to, p) in send {
                    self.route(&p, to);
                }
                let Some(at) = made else { return };
                let Some(key) = self.secure.remote_key(at) else { return };
                if !self.host {
                    // With an online code, the host is the key the service gave: no other.
                    let pinned = self.online.as_ref().and_then(Online::host_key);
                    if pinned.is_some_and(|want| want != key) {
                        self.secure.forget(at);
                        self.event(OnlineEvent::Note(format!("{at} answered with key {}, not the host's: ignored", crate::noise::fingerprint(&key))));
                        return;
                    }
                    self.host_path = Some(at);
                    self.host_heard = Some(now);
                    self.host_lost = false;
                    for m in std::mem::take(&mut self.waiting) {
                        self.seal_to(&m, at);
                    }
                }
                lock(&self.shared).keys.insert(at, key);
                if let Some(o) = &mut self.online {
                    o.session_made(&key);
                }
            }
            Opened::UnknownSession { from } => {
                if self.host && self.hinted.get(&from).is_none_or(|t| now.saturating_duration_since(*t) >= HINT_EVERY) {
                    self.hinted.insert(from, now);
                    self.route(&Secure::unknown_session(), from);
                }
            }
            Opened::ResetHint { from } => {
                // The host started again: a new session at once, rather than waiting to
                // find it lost.
                if !self.host && self.host_path == Some(from) && !self.secure.heard_within(from, now, crate::noise::QUIET) {
                    self.secure.forget(from);
                    self.host_path = None;
                }
            }
            Opened::Dropped => {}
        }
    }

    fn message(&mut self, bytes: Vec<u8>, from: SocketAddr, now: Instant) {
        if let Some(rest) = bytes.strip_prefix(&RELAY_TAG) {
            let Some((&sub, inner)) = rest.split_first() else { return };
            if self.host {
                // Never a relay in a relay, and only from a guest this host has a session
                // with.
                if relay_index(from).is_some() {
                    return;
                }
                let i = match self.relayed.iter().position(|r| *r == (from, sub)) {
                    Some(i) => i,
                    None => {
                        self.relayed.push((from, sub));
                        self.relayed.len() - 1
                    }
                };
                self.open(inner, relay_addr(i), now);
            } else if Some(from) == self.host_path && sub >= PEER_SUB {
                if let Some(Some(PeerRelay { addr: Some(to), .. })) = self.helping.get((sub - PEER_SUB) as usize) {
                    let to = *to;
                    self.socks.send(inner, to);
                }
            }
            return;
        }
        if self.host {
            // A session that roamed to a new address keeps its key there.
            let mut s = lock(&self.shared);
            if let (std::collections::hash_map::Entry::Vacant(e), Some(k)) = (s.keys.entry(from), self.secure.remote_key(from)) {
                e.insert(k);
            }
            drop(s);
            self.inbox.push_back((bytes, from));
            return;
        }
        if self.host_path != Some(from) {
            // The host's session moved (roaming): what goes to the host follows.
            self.host_path = Some(from);
        }
        self.host_heard = Some(now);
        self.host_lost = false;
        let shown = if self.online.is_some() { ONLINE_HOST } else { from };
        self.inbox.push_back((bytes, shown));
    }
}

impl Stack {
    /// A host's stack on `socks`, with this game's key; `lobby` is its address code's key
    /// (a guest's message 1 for another lobby is ignored), None for any.
    pub fn host(socks: Sockets, me: Identity, lobby: Option<u32>) -> (Self, Control) {
        let (i, c) = Inner::new(socks, me, true, lobby);
        (Self(i.into()), c)
    }

    /// A guest's stack on `socks`; `lobby` is the address code's key (0 for none).
    pub fn guest(socks: Sockets, me: Identity, lobby: u32) -> (Self, Control) {
        let (i, c) = Inner::new(socks, me, false, Some(lobby));
        (Self(i.into()), c)
    }

    /// A guest's stack for joining by online `code`, with the lobby's password (empty:
    /// none): the session joins [`ONLINE_HOST`].
    pub fn join_online(service: &str, code: nvf_signal::code::Code, name: &str, load_order_hash: &str, password: &str, me: Identity) -> io::Result<(Self, Control)> {
        let (mut s, c) = Inner::new(Sockets::guest_anywhere()?, me, false, Some(0));
        s.online = Some(Online::guest(service, code, name, load_order_hash, s.local_candidates(), password));
        Ok((Self(s.into()), c))
    }

    /// A host's lobby gets an online code from the matchmaking `service`
    /// ([`OnlineEvent::Code`] says it).
    pub fn go_online(&mut self, service: &str, info: HostInfo) {
        let s = self.0.get_mut();
        if s.host {
            let key = s.secure.identity().public();
            s.online = Some(Online::host(service, info, &key, s.local_candidates()));
        }
    }

    /// A host's lobby after a load's restart takes its online `code` back from the service,
    /// proving it with the lobby's `secret` (the service keeps a lobby a while after its
    /// host goes quiet, `nvf_coop::online`): the guests' code stays good.
    pub fn go_online_again(&mut self, service: &str, code: &str, secret: &str) {
        let s = self.0.get_mut();
        if s.host {
            s.online = Some(Online::host_again(service, code, secret, s.local_candidates()));
        }
    }

    /// The admission that goes with this stack (the host's by key).
    pub fn admission(&self) -> KeyAdmission {
        KeyAdmission { shared: self.0.borrow().shared.clone() }
    }

    /// This game's key pair.
    pub fn identity(&self) -> Identity {
        self.0.borrow().secure.identity().clone()
    }

    /// The address shown for this game ([`Sockets::local_addr`]).
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.borrow().socks.local_addr()
    }

    /// The IPv6 socket's address, when there is one.
    pub fn local_addr_v6(&self) -> Option<SocketAddr> {
        self.0.borrow().socks.local_addr_v6()
    }

}

impl seam::Transport for Stack {
    fn send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        let mut s = self.0.borrow_mut();
        let now = Instant::now();
        if s.host {
            s.seal_to(bytes, to);
        } else {
            s.guest_send(bytes, to, now);
        }
        Ok(bytes.len())
    }

    fn recv_from(&mut self, buf: &mut [u8]) -> io::Result<Option<(usize, SocketAddr)>> {
        let s = self.0.get_mut();
        if s.inbox.is_empty() {
            let now = Instant::now();
            if s.last_pump.is_none_or(|t| now.saturating_duration_since(t) >= PUMP_EVERY) {
                s.last_pump = Some(now);
                s.pump(now);
            }
            s.drain(now);
        }
        Ok(s.inbox.pop_front().map(|(b, from)| {
            let n = b.len().min(buf.len());
            buf[..n].copy_from_slice(&b[..n]);
            (n, from)
        }))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.borrow().socks.local_addr()
    }
}

impl Drop for Stack {
    /// Leaving: the service is told, and the router's port closed.
    fn drop(&mut self) {
        let s = self.0.get_mut();
        if let Some((to, b)) = s.online.as_mut().and_then(Online::bye) {
            s.socks.send(&b, to);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{Event, Session};
    use crate::sockets::Scope;

    fn host_session() -> (Session, Control, SocketAddr) {
        let socks = Sockets::host(0, Scope::Loopback).unwrap();
        let (stack, ctl) = Stack::host(socks, Identity::generate(), None);
        let at = stack.local_addr().unwrap();
        let adm = stack.admission();
        (Session::host_on(Box::new(stack), Box::new(adm)), ctl, at)
    }

    fn guest_session(at: SocketAddr, me: &Identity, token: u64) -> Session {
        let socks = Sockets::guest(&[at]).unwrap();
        let (stack, _) = Stack::guest(socks, me.clone(), 0);
        let adm = stack.admission();
        Session::join_on(Box::new(stack), Box::new(adm), &[at], 0, "Guest", token)
    }

    /// Ticks both until `done` holds of the guest's events (all of them so far), or 3 s.
    fn run(host: &mut Session, guest: &mut Session, mut done: impl FnMut(&[Event]) -> bool) -> (Vec<Event>, Vec<Event>) {
        let (mut he, mut ge) = (Vec::new(), Vec::new());
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            let now = Instant::now();
            host.tick(now);
            guest.tick(now);
            he.extend(host.take_events());
            ge.extend(guest.take_events());
            if done(&ge) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        (he, ge)
    }

    #[test]
    fn a_guest_joins_over_noise_and_the_host_knows_its_key() {
        let (mut host, ctl, at) = host_session();
        let me = Identity::generate();
        let mut guest = guest_session(at, &me, token_of(&me.public()));
        let (he, ge) = run(&mut host, &mut guest, |e| e.contains(&Event::Welcomed(1)));
        assert!(ge.contains(&Event::Welcomed(1)), "welcomed: {ge:?}");
        assert!(he.iter().any(|e| matches!(e, Event::Joined { id: 1, .. })));
        let g_at = guest.local_addr().unwrap();
        let g_at = SocketAddr::from((Ipv4Addr::LOCALHOST, g_at.port()));
        assert_eq!(ctl.key_at(g_at), Some(me.public()));
    }

    #[test]
    fn a_token_not_of_the_guests_key_is_turned_away() {
        let (mut host, _ctl, at) = host_session();
        let me = Identity::generate();
        let mut guest = guest_session(at, &me, token_of(&Identity::generate().public()));
        let (he, ge) = run(&mut host, &mut guest, |e| !e.is_empty());
        assert!(!ge.iter().any(|e| matches!(e, Event::Welcomed(_))), "{ge:?}");
        assert!(!he.iter().any(|e| matches!(e, Event::Joined { .. })));
    }

    #[test]
    fn a_banned_key_is_kicked_and_told_so_when_it_comes_back() {
        let (mut host, ctl, at) = host_session();
        let me = Identity::generate();
        let mut guest = guest_session(at, &me, token_of(&me.public()));
        run(&mut host, &mut guest, |e| e.contains(&Event::Welcomed(1)));
        assert!(host.kick(1, true));
        let (_, ge) = run(&mut host, &mut guest, |e| e.iter().any(|e| matches!(e, Event::Kicked { .. })));
        assert!(ge.contains(&Event::Kicked { banned: true }), "{ge:?}");
        assert!(ctl.is_banned(&me.public()));
        // The same player started again (a new address): told it is banned.
        let mut again = guest_session(at, &me, token_of(&me.public()));
        let (he, ge) = run(&mut host, &mut again, |e| e.iter().any(|e| matches!(e, Event::Kicked { .. })));
        assert!(ge.contains(&Event::Kicked { banned: true }), "{ge:?}");
        assert!(!he.iter().any(|e| matches!(e, Event::Joined { .. })));
        // Another player still gets in.
        let other = Identity::generate();
        let mut third = guest_session(at, &other, token_of(&other.public()));
        let (_, ge) = run(&mut host, &mut third, |e| e.iter().any(|e| matches!(e, Event::Welcomed(_))));
        assert!(ge.iter().any(|e| matches!(e, Event::Welcomed(_))), "{ge:?}");
    }

    #[test]
    fn nothing_unsealed_reaches_the_session() {
        let (stack, _ctl) = Stack::host(Sockets::host(0, Scope::Loopback).unwrap(), Identity::generate(), None);
        let at = stack.local_addr().unwrap();
        let mut stack = stack;
        let raw = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        // A plaintext Hello of the game's protocol: never opened, never passed up.
        let mut hello = crate::net::MAGIC.to_vec();
        hello.extend_from_slice(&[crate::net::VERSION, 1]);
        hello.extend_from_slice(&[0; 24]);
        raw.send_to(&hello, at).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let mut buf = [0u8; 64];
        for _ in 0..5 {
            assert_eq!(seam::Transport::recv_from(&mut stack, &mut buf).unwrap(), None);
        }
    }

    #[test]
    fn relay_addresses_name_their_place_and_paths_read_right() {
        for i in [0, 1, 7, 255] {
            assert_eq!(relay_index(relay_addr(i)), Some(i));
        }
        assert_eq!(relay_index("127.0.0.1:47960".parse().unwrap()), None);
        assert_eq!(Path::of(relay_addr(0)), Path::Relayed);
        assert_eq!(Path::of("192.168.1.5:47960".parse().unwrap()), Path::Lan);
        assert_eq!(Path::of("203.0.113.9:47960".parse().unwrap()), Path::Direct);
        assert_eq!(Path::of(sockets::hosted_standin(&[7; 16])), Path::HostedRelay);
        assert_ne!(token_of(&[0; 32]), 0);
    }
}
