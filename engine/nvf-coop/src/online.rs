//! Joining by an online code (docs/design/coop-networking.md §5-§7, phase 3): the matchmaking
//! service (`coop.nv-forever.com`, [`crate::signal_io`]) introduces host and guest, STUN tells
//! each its public address, and both probe every pair of addresses at once (hole punching,
//! `nvf_nat::ice`). When a pair answers, the guest's Noise handshake ([`crate::noise`]) goes
//! there, and from then on the game's own link ([`crate::net`]) runs as on a LAN. No game
//! traffic goes through the service.
//!
//! [`Online`] is driven from the network thread's tick ([`crate::net::Session::tick`]): it
//! takes the service's frames, STUN answers and probes, and gives packets to send from the
//! game's own sockets (STUN asks and probes must leave from them, so the NAT's mappings are
//! the game's).
//!
//! **Probes** are packets of their own, under the co-op magic and version:
//! `0xE5` probe and `0xE6` answer, each with the guest's number in the lobby (u32) and a
//! check id (u64). They are not authenticated: a forged answer at most sends the guest's
//! handshake to a wrong address, and the guest takes a session only from the host key the
//! service gave it ([`Online::host_key`]). A game answers probes only for a guest it is
//! checking with, so it reflects nothing for strangers.
//!
//! What is not here yet (phase 4): router port mapping for every player (PCP, NAT-PMP,
//! UPnP), a relay through another guest, join approval, and the reasons on the failure
//! screens beyond one line.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::Instant;

use nvf_nat::candidate::{is_lan, Candidate, Kind};
use nvf_nat::hosted::{self, SessionId};
use nvf_nat::ice::{Action, Agent, Config, PairId, Role, NO_PAIR};
use nvf_nat::report::{self, Connect};
use nvf_nat::relay::{self, Answer, GiveUp, Helper, Planner, Policy, Refusal};
use nvf_nat::stun::{self, Gather, Mapping};
use nvf_signal::client::{GuestClient, GuestEvent, HostClient, HostEvent};
use nvf_signal::code::Code;
use nvf_signal::wire::{key_hex, CreateLobby, PeerNo, WIRE_VERSION};

use crate::portmap::{MapEvent, PortMap};
use crate::signal_io::{self, FromIo, SignalIo};

pub const KIND_PROBE: u8 = 0xE5;
pub const KIND_ANSWER: u8 = 0xE6;
/// The peer relay's checks, between a player that cannot reach the host and a guest that
/// may relay for it (docs/design/coop-networking.md §8): a probe's `peer` is its sender's
/// number, and the answer repeats it.
pub const KIND_RELAY_PROBE: u8 = 0xE7;
pub const KIND_RELAY_ANSWER: u8 = 0xE8;
/// A join that failed with no STUN answer at all (§10): what the player is told.
pub const UDP_BLOCKED: &str = "Your network seems to block the kind of traffic co-op needs (UDP), as school and work networks often do. Try another network, or Tailscale and join by address.";

/// Turns relaying for other players off (`0`): the "Relay for other players" setting.
pub const RELAY_ENV: &str = "NVF_COOP_RELAY";
/// Turns join approval off (`0`): the "Ask before players join" setting. On by default for
/// an online code (docs/design/coop-networking.md §15.2, the recommended default).
pub const ASK_ENV: &str = "NVF_COOP_ASK";
/// A player asking to join that the host has not answered by then is turned away: time to
/// open the Co-op Lobby from a corner message [observed 2026-11-25: 60 s was too short in a
/// screenshot run; the guest's own wait was shorter still].
pub const ASK_FOR: std::time::Duration = std::time::Duration::from_secs(120);

/// Test aid: a guest ignores the host's candidates (`1`), so its direct checks fail and
/// the peer relay is tried, as for a player whose network lets nothing in.
pub const NO_DIRECT_ENV: &str = "NVF_COOP_TEST_NO_DIRECT";

fn no_direct() -> bool {
    std::env::var(NO_DIRECT_ENV).is_ok_and(|v| v.trim() == "1")
}

/// Test aid: the plain checks are off (`1`), on either side, so only the birthday punch can
/// find a path: a check of a pair (not [`NO_PAIR`]) from the game's own socket to a
/// candidate the service passed on is never sent. The punch's spread probes and spray, the
/// checks to peer-reflexive addresses learnt, and everything from its extra sockets still go.
pub const BIRTHDAY_ONLY_ENV: &str = "NVF_COOP_TEST_BIRTHDAY_ONLY";

fn birthday_only() -> bool {
    std::env::var(BIRTHDAY_ONLY_ENV).is_ok_and(|v| v.trim() == "1")
}

/// Whether [`BIRTHDAY_ONLY_ENV`] drops this probe: a plain check, from the game's own
/// socket, to a candidate the service passed on.
fn plain_check(pair: PairId, from_main: bool, to: SocketAddr, passed_on: &[Candidate]) -> bool {
    pair != NO_PAIR && from_main && passed_on.iter().any(|c| c.addr == to)
}

/// What the game hears from [`Online`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineEvent {
    /// The host's lobby code, short form (`K7Q-M2X`), and the secret that keeps it across a
    /// restart.
    Code { code: String, secret: String },
    /// The guest's path to the host: its Hello goes there.
    HostAt(SocketAddr),
    /// What went wrong, for the player.
    Failed(String),
    /// A line for the log.
    Note(String),
    /// The guest's path to the host goes through another guest (the peer relay): its
    /// Hello goes there, from now on, even when it had a host.
    HostVia(SocketAddr),
    /// Relay for player `peer` (another guest that cannot reach the host).
    RelayFor(PeerNo),
    /// Relayed player `peer` is at `addr`.
    RelayAt { peer: PeerNo, addr: SocketAddr },
    /// Stop relaying for player `peer`.
    RelayEnd(PeerNo),
    /// Open a session on the hosted relay at `relay` for one pair, as `side`
    /// ([`crate::sockets::Transport::hosted_open`]).
    HostedOpen { relay: SocketAddr, session: SessionId, side: hosted::Side },
    /// Player `peer` asks to join the host's lobby: let it in or not ([`Online::admit`]).
    JoinAsk { peer: PeerNo, name: String },
    /// The ask of player `peer` is over (answered, timed out, or it gave up).
    JoinAskOver(PeerNo),
    /// A guest: the host is asked to let this game in (join approval); waits for it.
    AwaitingHost,
    /// A guest: the host let this game in.
    LetIn,
}

/// A player asking to join, waiting for the host's answer.
struct Asking {
    name: String,
    key: [u8; 32],
    candidates: Vec<Candidate>,
    since: Instant,
}

/// The peer relay's state on a guest: as the player that cannot reach the host (C: the
/// planner and its checks with each helper) and as a helper (A: the players it relays).
#[derive(Default)]
struct Relays {
    /// The guests the service says could relay.
    helpers: Vec<PeerNo>,
    planner: Option<Planner>,
    /// A helper that accepted: its candidates, for the checks.
    offered: HashMap<PeerNo, Vec<Candidate>>,
    /// C's checks with each helper that accepted, and when they started.
    reaching: HashMap<PeerNo, (Agent, Instant)>,
    /// Where each helper was reached.
    reached: HashMap<PeerNo, SocketAddr>,
    /// A's players relayed: the checks with each, and where it was last heard.
    serving: HashMap<PeerNo, (Agent, Option<SocketAddr>)>,
    /// C's direct checks failed with no helper known yet: since when it waits for the
    /// service's `helpers` ([`HELPERS_WAIT`]) before the planner starts.
    waiting: Option<Instant>,
}

/// How long a guest with no direct path waits for the service's `helpers` before it asks
/// nobody and gives up [design: a list sent a moment after the checks' end still counts].
const HELPERS_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// What a host sends the service when it makes its lobby.
#[derive(Clone, Debug, Default)]
pub struct HostInfo {
    pub name: String,
    pub load_order_hash: String,
    /// The lobby browser's listing (docs/design/coop-lobby-browser.md); None: private, no
    /// password.
    pub listing: Option<nvf_signal::wire::Listing>,
    /// The lobby's password, empty for none: its hash goes to the service once the code is
    /// known (the code salts it).
    pub password: String,
}

struct Peer {
    key: [u8; 32],
    remotes: Vec<Candidate>,
    agent: Option<Agent>,
}

enum Side {
    Host {
        client: Option<HostClient>,
        peers: HashMap<PeerNo, Peer>,
        /// Players asking to join (join approval).
        asking: HashMap<PeerNo, Asking>,
        secret: Option<String>,
        /// The lobby's code, compact, once the service said it.
        code: Option<String>,
    },
    Guest {
        name: String,
        client: Option<GuestClient>,
        me: Option<PeerNo>,
        host_key: Option<[u8; 32]>,
        remotes: Vec<Candidate>,
        agent: Option<Box<Agent>>,
        load_order_hash: String,
        connected: bool,
    },
}

/// A game's side of the matchmaking service and the checks it starts.
pub struct Online {
    service: String,
    io: Option<SignalIo>,
    side: Side,
    /// This game's own addresses, each with its socket's port: the host candidates, and the
    /// agents' local sockets.
    locals: Vec<SocketAddr>,
    gather: Option<Gather>,
    gathered: bool,
    found: Vec<Candidate>,
    events: Vec<OnlineEvent>,
    /// The socket closed while hosting: when to open it again.
    reopen_at: Option<Instant>,
    /// This player's router asked to open the IPv4 socket's port ([`crate::portmap`]).
    portmap: Option<PortMap>,
    /// STUN asked and no server answered: this network likely blocks UDP (§10).
    no_stun: bool,
    /// This network's NAT as STUN found it, for the `co-op connect:` line.
    nat: report::Nat,
    /// This game's key (from the first tick), for the hosted relay's session ids.
    my_key: Option<[u8; 32]>,
    /// When the checks began (the service's `go`), for the `co-op connect:` line.
    go_at: Option<Instant>,
    relays: Relays,
    /// Keys the host banned ([`Online::ban`]).
    banned: Vec<[u8; 32]>,
    /// A failure was told already: the socket's close after it says nothing more.
    failed: bool,
    /// The host's lobby password (sent hashed once the code is known), and the listing last
    /// sent (to send only a change).
    host_password: String,
    listing_sent: Option<(bool, String, String, bool)>,
    /// The guest's password hash, for its `join`.
    join_hash: Option<String>,
    /// The birthday punch's asks of the sockets ([`Online::take_socket_asks`]), and the probes
    /// to send from its sockets (their ports) ([`Online::take_sends_from`]).
    socket_asks: Vec<SocketAsk>,
    sends_from: Vec<(u16, SocketAddr, Vec<u8>)>,
    /// [`BIRTHDAY_ONLY_ENV`].
    birthday_only: bool,
}

/// The guest's own checks' owner of birthday sockets (a host's are its guests' numbers).
pub const GUEST_SOCKETS: PeerNo = PeerNo::MAX;

/// What the birthday punch asks of the sockets (docs/spec-notes/nat-traversal.md, "Birthday
/// punch"): `owner` is the checks' (a host's guest number, or [`GUEST_SOCKETS`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketAsk {
    /// Open `count` more IPv4 sockets; [`Online::extras_opened`] takes their ports.
    Open { owner: PeerNo, count: u16 },
    /// Close them, but the one on this port, from now on the way to this address.
    Close { owner: PeerNo, keep: Option<(u16, SocketAddr)> },
}

impl Online {
    /// A host making its lobby, for the sockets at `locals`.
    pub fn host(service: &str, info: HostInfo, key: &[u8; 32], locals: Vec<SocketAddr>) -> Self {
        let body = CreateLobby {
            wire: WIRE_VERSION,
            protocol: crate::net::VERSION as u32,
            build: env!("CARGO_PKG_VERSION").into(),
            load_order_hash: info.load_order_hash.clone(),
            max_players: (crate::net::MAX_GUESTS + 1) as u8,
            host_key: key_hex(key),
            name: info.name.clone(),
            listing: info.listing.clone(),
        };
        let mut o = Self::new(service, Some(SignalIo::create(service, body)), Side::Host { client: None, peers: HashMap::new(), asking: HashMap::new(), secret: None, code: None }, locals);
        o.host_password = info.password;
        o
    }

    /// A host taking its lobby back after a restart: the same code and secret.
    pub fn host_again(service: &str, code: &str, secret: &str, locals: Vec<SocketAddr>) -> Self {
        let mut o = Self::new(service, Some(SignalIo::resume(service, code)), Side::Host { client: None, peers: HashMap::new(), asking: HashMap::new(), secret: Some(secret.into()), code: Some(code.replace('-', "")) }, locals);
        o.start_stun(&default_stun(), Instant::now());
        o
    }

    /// A guest joining `code`, with the lobby's password (empty: none).
    pub fn guest(service: &str, code: Code, name: &str, load_order_hash: &str, locals: Vec<SocketAddr>, password: &str) -> Self {
        let hash = (!password.is_empty()).then(|| nvf_signal::wire::password_hash(&code, password));
        let side = Side::Guest { name: name.into(), client: None, me: None, host_key: None, remotes: Vec::new(), agent: None, load_order_hash: load_order_hash.into(), connected: false };
        let mut o = Self::new(service, Some(SignalIo::lookup(service, code)), side, locals);
        o.join_hash = hash;
        o.start_stun(&default_stun(), Instant::now());
        o
    }

    fn new(service: &str, io: Option<SignalIo>, side: Side, locals: Vec<SocketAddr>) -> Self {
        let mut found: Vec<Candidate> = locals.iter().map(|a| Candidate::new(Kind::Host, *a)).collect();
        // A relay this player hosts or uses: the last-resort path, for each peer a session of
        // its own on it.
        if let Some(r) = crate::prefs::hosted_relay() {
            found.push(Candidate::new(Kind::Relayed, r));
        }
        let portmap = locals.iter().find(|a| a.is_ipv4()).and_then(|a| PortMap::start(*a, Instant::now()));
        // The checks' local sockets: one per family (every address is the same socket).
        let locals: Vec<SocketAddr> = [locals.iter().find(|a| a.is_ipv4()), locals.iter().find(|a| a.is_ipv6())].into_iter().flatten().copied().collect();
        let birthday_only = birthday_only();
        if birthday_only {
            log::info!("co-op: online: test aid: plain checks off ({BIRTHDAY_ONLY_ENV}); only the birthday punch can find a path");
        }
        Self { service: service.into(), io, side, locals, gather: None, gathered: false, found, events: Vec::new(), reopen_at: None, portmap, no_stun: false, nat: report::Nat::Unknown, my_key: None, go_at: None, relays: Relays::default(), banned: Vec::new(), failed: false, host_password: String::new(), listing_sent: None, join_hash: None, socket_asks: Vec::new(), sends_from: Vec::new(), birthday_only }
    }

    /// The host key the service gave a guest: the only key its Noise session may have.
    pub fn host_key(&self) -> Option<[u8; 32]> {
        match &self.side {
            Side::Guest { host_key, .. } => *host_key,
            Side::Host { .. } => None,
        }
    }

    pub fn take_events(&mut self) -> Vec<OnlineEvent> {
        std::mem::take(&mut self.events)
    }

    /// The birthday punch's asks of the sockets, oldest first.
    pub fn take_socket_asks(&mut self) -> Vec<SocketAsk> {
        std::mem::take(&mut self.socket_asks)
    }

    /// Probes to send from the birthday punch's sockets: (its port, to, bytes).
    pub fn take_sends_from(&mut self) -> Vec<(u16, SocketAddr, Vec<u8>)> {
        std::mem::take(&mut self.sends_from)
    }

    /// The sockets `owner`'s checks asked for are open on these ports.
    pub fn extras_opened(&mut self, owner: PeerNo, ports: &[u16]) {
        let Some(ip) = self.locals.iter().find(|a| a.is_ipv4()).map(SocketAddr::ip) else { return };
        let addrs: Vec<SocketAddr> = ports.iter().map(|&p| SocketAddr::new(ip, p)).collect();
        let agent = match &mut self.side {
            Side::Host { peers, .. } => peers.get_mut(&owner).and_then(|p| p.agent.as_mut()),
            Side::Guest { agent, .. } if owner == GUEST_SOCKETS => agent.as_deref_mut(),
            Side::Guest { .. } => None,
        };
        if let Some(a) = agent {
            a.add_extra_sockets(&addrs);
            self.events.push(OnlineEvent::Note(format!("birthday punch: {} more sockets open for the checks", addrs.len())));
        }
    }

    fn start_stun(&mut self, servers: &[SocketAddr], now: Instant) {
        if self.gather.is_some() || servers.is_empty() {
            return;
        }
        let list: Vec<(SocketAddr, stun::TxId)> = servers.iter().map(|s| (*s, txid())).collect();
        self.gather = Some(Gather::new(&list, now).with_software(concat!("NVForever ", env!("CARGO_PKG_VERSION"))));
    }

    /// A candidate this game found: kept, and given to the service.
    fn found(&mut self, c: Candidate) {
        if self.found.contains(&c) {
            return;
        }
        self.found.push(c);
        match &mut self.side {
            Side::Host { client: Some(cl), .. } => cl.add_local(c),
            Side::Guest { client: Some(cl), .. } => cl.add_local(c),
            _ => {}
        }
    }

    /// The service's socket, STUN and the checks, once a tick: the packets to send from the
    /// game's sockets.
    pub fn tick(&mut self, now: Instant, my_key: &[u8; 32]) -> Vec<(SocketAddr, Vec<u8>)> {
        let mut send = Vec::new();
        self.my_key = Some(*my_key);
        if self.reopen_at.is_some_and(|t| now >= t) {
            self.reopen_at = None;
            if let Side::Host { secret: Some(_), code: Some(code), .. } = &self.side {
                self.io = Some(SignalIo::resume(&self.service, code));
            }
        }
        let came = self.io.as_ref().map(SignalIo::take).unwrap_or_default();
        for m in came {
            self.take_io(m, now, my_key);
        }
        if let Some(g) = &mut self.gather {
            send.extend(g.poll(now));
            if !self.gathered && g.done(now) {
                self.gathered = true;
                let r = g.result();
                let mapping = match r.mapping {
                    Mapping::EndpointIndependent => "the same public port for every server (hole punching works)",
                    Mapping::EndpointDependent => "a new public port per server (a hard NAT: joins may fail)",
                    Mapping::Unknown => "unknown (fewer than two servers answered)",
                };
                self.events.push(OnlineEvent::Note(format!("STUN: public address {:?}; mapping {mapping}", r.reflexive())));
                self.no_stun = r.reflexive().is_empty();
                self.nat = report::Nat::from_gather(&r);
                // The checks running now learn this side's NAT (the birthday punch needs it).
                match &mut self.side {
                    Side::Host { peers, .. } => {
                        for a in peers.values_mut().filter_map(|p| p.agent.as_mut()) {
                            a.set_local_nat(self.nat);
                        }
                    }
                    Side::Guest { agent: Some(a), .. } => a.set_local_nat(self.nat),
                    Side::Guest { .. } => {}
                }
                if self.no_stun {
                    self.events.push(OnlineEvent::Note("no STUN server answered: this network may block UDP".into()));
                }
                for a in r.reflexive() {
                    self.found(Candidate::new(Kind::ServerReflexive, a));
                }
            }
        }
        if let Some(pm) = &mut self.portmap {
            send.extend(pm.tick(now));
            for e in pm.take_events() {
                match e {
                    MapEvent::Mapped(at) => self.found(Candidate::new(Kind::Mapped, at)),
                    MapEvent::Note(n) => self.events.push(OnlineEvent::Note(n)),
                }
            }
        }
        self.ask_timeouts(now);
        send.extend(self.poll_agents(now));
        send.extend(self.poll_relays(now));
        self.flush();
        send
    }

    /// A datagram from `from` that may be the router's answer about the port: whether it was.
    pub fn on_router(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) -> bool {
        self.portmap.as_mut().is_some_and(|pm| pm.on_packet(from, bytes, now))
    }

    fn flush(&mut self) {
        let out = match &mut self.side {
            Side::Host { client: Some(c), .. } => c.take_outgoing(),
            Side::Guest { client: Some(c), .. } => c.take_outgoing(),
            _ => Vec::new(),
        };
        if let Some(io) = &self.io {
            for t in out {
                io.send(t);
            }
        }
    }

    fn take_io(&mut self, m: FromIo, now: Instant, my_key: &[u8; 32]) {
        match m {
            FromIo::Created { code, host_secret, stun } => {
                if let Side::Host { secret, .. } = &mut self.side {
                    *secret = Some(host_secret);
                }
                let servers = if stun.is_empty() { default_stun() } else { stun };
                self.start_stun(&servers, now);
                self.events.push(OnlineEvent::Note(format!("the service made lobby {code}")));
            }
            FromIo::LookedUp(info) => {
                if let Side::Guest { load_order_hash, .. } = &self.side {
                    if info.protocol != crate::net::VERSION as u32 {
                        return self.fail(format!("The host runs another version of NVForever ({}): update both games to the same one.", info.build));
                    }
                    if !info.load_order_hash.is_empty() && !load_order_hash.is_empty() && info.load_order_hash != *load_order_hash {
                        return self.fail("The host runs a different load order: both games need the same plugins in the same order.".into());
                    }
                    if info.full {
                        return self.fail("The host's lobby is full.".into());
                    }
                    if info.locked && self.join_hash.is_none() {
                        self.fail("This lobby has a password: type it on the Join screen.".into());
                    }
                }
            }
            FromIo::Open => {
                let found = self.found.clone();
                match &mut self.side {
                    Side::Host { client, secret: Some(s), .. } => match client {
                        // Reopened after a drop: the same lobby.
                        Some(c) => c.hello(),
                        None => {
                            let mut c = HostClient::new(s);
                            for f in &found {
                                c.add_local(*f);
                            }
                            *client = Some(c);
                        }
                    },
                    Side::Guest { name, client, .. } => {
                        *client = Some(GuestClient::joining(name, &key_hex(my_key), &found, self.join_hash.as_deref()));
                    }
                    Side::Host { secret: None, .. } => {}
                }
            }
            FromIo::Text(t) => {
                self.on_text(&t, now);
                // A refusal the service then closes the socket on (a wrong password, a lobby
                // that is full): its reason is the one the player sees, not the close.
                if self.events.iter().any(|e| matches!(e, OnlineEvent::Failed(_))) {
                    self.failed = true;
                }
            }
            FromIo::Failed(why) => {
                if matches!(self.side, Side::Host { code: Some(_), .. }) {
                    // The lobby is still there for 120 s: try again soon.
                    self.reopen_at = Some(now + std::time::Duration::from_secs(3));
                    self.events.push(OnlineEvent::Note(format!("the service: {why}; trying again")));
                } else {
                    self.fail(why);
                }
            }
            FromIo::Closed if self.failed => self.io = None,
            FromIo::Closed => {
                self.io = None;
                match &self.side {
                    Side::Host { code: Some(_), .. } => self.reopen_at = Some(now + std::time::Duration::from_secs(2)),
                    // Once on the host's link the guest needs the service only to come back.
                    Side::Guest { connected: true, .. } => {}
                    _ => self.fail("The matchmaking service closed the connection.".into()),
                }
            }
        }
    }

    fn fail(&mut self, why: String) {
        self.events.push(OnlineEvent::Failed(why));
        self.failed = true;
        self.io = None;
    }

    fn on_text(&mut self, t: &str, now: Instant) {
        let locals = self.locals.clone();
        let nat = self.nat;
        match &mut self.side {
            Side::Host { client: Some(c), peers, asking, secret, code: kept, .. } => {
                let Ok(evs) = c.on_text(t) else { return };
                for e in evs {
                    match e {
                        HostEvent::Hosting { code, .. } => {
                            if !self.host_password.is_empty() {
                                if let Some(lobby) = Code::parse(&code) {
                                    c.password(&nvf_signal::wire::password_hash(&lobby, &self.host_password));
                                }
                            }
                            *kept = Some(code.clone());
                            let short = format!("{}-{}", &code[..3.min(code.len())], &code[3.min(code.len())..]);
                            self.events.push(OnlineEvent::Code { code: short, secret: secret.clone().unwrap_or_default() });
                        }
                        HostEvent::JoinRequest { peer, name, key, candidates } => {
                            // Two players of one name (the default "Courier"): the ask names
                            // the key's first characters too, as its fingerprint starts.
                            let shown = format!("{name} ({})", key.get(..4).unwrap_or("?"));
                            let Some(key) = unhex32(&key) else {
                                c.decline(peer, "bad_key");
                                continue;
                            };
                            if self.banned.contains(&key) {
                                c.decline(peer, "banned");
                                self.events.push(OnlineEvent::Note(format!("{name} (guest {peer}) asks to join: banned, turned away")));
                                continue;
                            }
                            // Join approval: the host is asked, unless it turned that off. The
                            // game's own link still turns a full lobby away.
                            if !crate::prefs::ask_before_join() {
                                c.accept(peer);
                                self.events.push(OnlineEvent::Note(format!("{name} (guest {peer}) asks to join: let in")));
                                let remotes = hosted_pair(&mut self.events, self.my_key, Some(key), hosted::Side::Host, candidates);
                                peers.insert(peer, Peer { key, remotes, agent: None });
                            } else {
                                self.events.push(OnlineEvent::Note(format!("{name} (guest {peer}) asks to join: asking the host")));
                                self.events.push(OnlineEvent::JoinAsk { peer, name: shown });
                                asking.insert(peer, Asking { name, key, candidates, since: now });
                            }
                        }
                        HostEvent::Candidates { peer, candidates } => {
                            log::debug!("co-op: guest {peer}'s candidates: {candidates:?}");
                            if let Some(p) = peers.get_mut(&peer) {
                                let candidates = hosted_pair(&mut self.events, self.my_key, Some(p.key), hosted::Side::Host, candidates);
                                for c in candidates {
                                    if !p.remotes.contains(&c) {
                                        p.remotes.push(c);
                                    }
                                    if let Some(a) = &mut p.agent {
                                        a.add_remote(c);
                                    }
                                }
                            }
                        }
                        HostEvent::Go { peer } | HostEvent::Reconnect { peer } => {
                            if let Some(p) = peers.get_mut(&peer) {
                                let mut a = Agent::new(Role::Host, &locals, Config::default());
                                a.set_local_nat(nat);
                                for c in &p.remotes {
                                    a.add_remote(*c);
                                }
                                if matches!(e, HostEvent::Go { .. }) {
                                    a.start(now);
                                }
                                p.agent = Some(a);
                            }
                        }
                        HostEvent::Connected { peer, path } => self.events.push(OnlineEvent::Note(format!("guest {peer}'s path: {path}"))),
                        HostEvent::Left { peer } => {
                            peers.remove(&peer);
                            if asking.remove(&peer).is_some() {
                                self.events.push(OnlineEvent::JoinAskOver(peer));
                            }
                        }
                        HostEvent::Closed { reason } => self.events.push(OnlineEvent::Failed(format!("The matchmaking service closed the lobby ({reason})."))),
                        HostEvent::Error { error, message } if error == "too_many_public" => {
                            self.events.push(OnlineEvent::Note(format!("not listed: too many public lobbies from this network ({message}); the lobby stays private")));
                        }
                        HostEvent::Error { error, message } => self.events.push(OnlineEvent::Note(format!("the service: {error}: {message}"))),
                    }
                }
            }
            Side::Guest { client: Some(c), me, host_key, remotes, agent, connected, .. } => {
                let Ok(evs) = c.on_text(t) else { return };
                let relays = &mut self.relays;
                for e in evs {
                    match e {
                        GuestEvent::Waiting { peer } => {
                            *me = Some(peer);
                            self.events.push(OnlineEvent::Note(format!("waiting for the host to let this game in (guest {peer})")));
                            self.events.push(OnlineEvent::AwaitingHost);
                        }
                        GuestEvent::Accepted { host_key: k, host_name, candidates } => {
                            self.events.push(OnlineEvent::LetIn);
                            *host_key = unhex32(&k);
                            self.events.push(OnlineEvent::Note(format!("{host_name}'s game let this one in")));
                            let candidates = hosted_pair(&mut self.events, *host_key, self.my_key, hosted::Side::Guest, candidates);
                            for c in candidates.into_iter().filter(|c| !no_direct() || is_hosted(c)) {
                                if !remotes.contains(&c) {
                                    remotes.push(c);
                                }
                            }
                        }
                        GuestEvent::Candidates { candidates } => {
                            let candidates = hosted_pair(&mut self.events, *host_key, self.my_key, hosted::Side::Guest, candidates);
                            for c in candidates.into_iter().filter(|c| !no_direct() || is_hosted(c)) {
                                if !remotes.contains(&c) {
                                    remotes.push(c);
                                }
                                if let Some(a) = agent {
                                    a.add_remote(c);
                                }
                            }
                        }
                        GuestEvent::Go => {
                            self.go_at = Some(now);
                            let mut a = Agent::new(Role::Guest, &locals, Config::default());
                            a.set_local_nat(nat);
                            for c in remotes.iter() {
                                a.add_remote(*c);
                            }
                            a.start(now);
                            *agent = Some(Box::new(a));
                        }
                        GuestEvent::Declined { reason } => {
                            let why = match reason.as_str() {
                                "declined" => "The host turned your request to join away.".to_string(),
                                "no_answer" => "The host did not answer your request to join: try again, and tell them to look at their Co-op Lobby.".to_string(),
                                "banned" => "The host banned you from this lobby.".to_string(),
                                "bad_password" => "Wrong password for this lobby.".to_string(),
                                _ => format!("The host's game turned this one away ({reason})."),
                            };
                            self.events.push(OnlineEvent::Failed(why));
                        }
                        GuestEvent::Closed { reason } => {
                            self.events.push(OnlineEvent::Failed(format!("The lobby closed ({reason}).")));
                        }
                        GuestEvent::Error { error, message } => {
                            let why = match error.as_str() {
                                "full" => "The host's lobby is full.".to_string(),
                                "host_away" => "The host's game is reconnecting: try again in a moment.".to_string(),
                                "rate_limited" => "Too many wrong passwords: wait a minute, then try again.".to_string(),
                                _ => format!("The matchmaking service refused ({error}: {message})."),
                            };
                            self.events.push(OnlineEvent::Failed(why));
                        }
                        GuestEvent::Helpers { peers } => {
                            relays.helpers = peers.into_iter().filter(|p| Some(*p) != *me).collect();
                            if let Some(pl) = &mut relays.planner {
                                pl.set_helpers(&helpers(&relays.helpers), now);
                            }
                        }
                        GuestEvent::RelayAsk { from, candidates } => {
                            // A: another player asks this game to relay for it. Only with a
                            // direct path of its own (not relayed itself).
                            let direct = *connected && relays.planner.as_ref().is_none_or(|p| p.current().is_none());
                            let policy = Policy { allow: crate::prefs::relay_for_others(), ..Policy::default() };
                            let answer = if relays.serving.contains_key(&from) { Answer::Accept } else { policy.answer(relays.serving.len(), direct) };
                            match answer {
                                Answer::Accept => {
                                    c.relay_answer(from, true, "", &self.found);
                                    // No birthday punch for a relay's checks.
                                    let mut a = Agent::new(Role::Host, &locals, Config { birthday_sockets: 0, ..Config::default() });
                                    for cand in candidates {
                                        a.add_remote(cand);
                                    }
                                    a.start(now);
                                    relays.serving.insert(from, (a, None));
                                    self.events.push(OnlineEvent::Note(format!("guest {from} cannot reach the host: relaying for it")));
                                    self.events.push(OnlineEvent::RelayFor(from));
                                }
                                Answer::Refuse(why) => {
                                    c.relay_answer(from, false, why.as_wire(), &[]);
                                    self.events.push(OnlineEvent::Note(format!("guest {from} asked this game to relay: no ({})", why.as_wire())));
                                }
                            }
                        }
                        GuestEvent::RelayAnswer { helper, accept, reason, candidates } => {
                            if accept {
                                relays.offered.insert(helper, candidates);
                            }
                            if let Some(pl) = &mut relays.planner {
                                pl.on_answer(helper, if accept { Answer::Accept } else { Answer::Refuse(Refusal::from_wire(&reason)) });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn poll_agents(&mut self, now: Instant) -> Vec<(SocketAddr, Vec<u8>)> {
        let mut send = Vec::new();
        // A probe from one of the birthday punch's sockets goes from it (its port); from the
        // game's own socket otherwise.
        let main: Vec<SocketAddr> = self.locals.clone();
        let sends_from = &mut self.sends_from;
        let asks = &mut self.socket_asks;
        let birthday_only = self.birthday_only;
        let mut out = |pair: PairId, from: SocketAddr, to: SocketAddr, passed_on: &[Candidate], bytes: Vec<u8>, send: &mut Vec<(SocketAddr, Vec<u8>)>| {
            let from_main = !from.is_ipv4() || main.contains(&from);
            if birthday_only && plain_check(pair, from_main, to, passed_on) {
                return;
            }
            if !from_main {
                sends_from.push((from.port(), to, bytes));
            } else {
                send.push((to, bytes));
            }
        };
        match &mut self.side {
            Side::Host { peers, .. } => {
                for (&peer, p) in peers.iter_mut() {
                    let Some(a) = &mut p.agent else { continue };
                    let mut failed = false;
                    for act in a.poll(now) {
                        match act {
                            Action::SendProbe { pair, from, to, check_id } => out(pair, from, to, &p.remotes, probe(KIND_PROBE, peer, check_id), &mut send),
                            Action::OpenSockets { count } => asks.push(SocketAsk::Open { owner: peer, count }),
                            // The host keeps the socket the guest's handshake comes in on
                            // (`Session` pins it); the rest close when the session is made.
                            Action::CloseSockets { .. } => asks.push(SocketAsk::Close { owner: peer, keep: None }),
                            // The guest names the path, and its handshake comes over it; a host
                            // agent that finds none stops, and says so.
                            Action::Failed => failed = true,
                            _ => {}
                        }
                    }
                    if failed {
                        if let Some(line) = p.agent.take().and_then(|a| a.punch_summary()) {
                            self.events.push(OnlineEvent::Note(format!("guest {peer}: {line}")));
                        }
                        let kinds: Vec<String> = p.remotes.iter().map(|c| format!("{:?}", c.kind)).collect();
                        self.events.push(OnlineEvent::Note(format!("guest {peer}: the checks found no direct path ({} candidates: {}); it may come through another player", p.remotes.len(), kinds.join(", "))));
                    }
                }
            }
            Side::Guest { me: Some(me), agent, client, connected, remotes, .. } => {
                let mut done = false;
                if let Some(a) = agent {
                    let mut chosen = None;
                    for act in a.poll(now) {
                        match act {
                            Action::SendProbe { pair, from, to, check_id } => out(pair, from, to, remotes, probe(KIND_PROBE, *me, check_id), &mut send),
                            Action::OpenSockets { count } => asks.push(SocketAsk::Open { owner: GUEST_SOCKETS, count }),
                            // After `Selected`: a kept socket is the way to the host chosen.
                            Action::CloseSockets { keep } => {
                                let keep = keep.zip(chosen).map(|(l, r): (SocketAddr, SocketAddr)| (l.port(), r));
                                if let Some((port, _)) = keep {
                                    self.events.push(OnlineEvent::Note(format!("birthday punch: the path goes from the extra socket on port {port}")));
                                }
                                asks.push(SocketAsk::Close { owner: GUEST_SOCKETS, keep });
                            }
                            Action::Selected { remote, .. } => {
                                chosen = Some(remote);
                                let hosted_path = crate::sockets::is_hosted_standin(remote);
                                // The service passes any word of up to 16 characters on to the host; only "direct" and
                                // "lan" make a guest a helper [documented: docs/design/coop-signal-wire.md].
                                let path = if hosted_path { "hosted-relay" } else if is_lan(remote.ip()) { "lan" } else { "direct" };
                                let how = a.selected_candidate().map_or(report::Path::Open, report::Path::of);
                                log::info!("{}", Connect { host: false, path: how, nat: self.nat, peer_nat: report::Nat::from_candidates(remotes), elapsed: self.go_at.map_or(std::time::Duration::ZERO, |t| now.saturating_duration_since(t)) });
                                if let Some(c) = client {
                                    c.connected(path);
                                }
                                *connected = true;
                                self.events.push(OnlineEvent::Note(format!("path to the host: {remote} ({path})")));
                                self.events.push(OnlineEvent::HostAt(remote));
                                done = true;
                            }
                            Action::Failed if !crate::prefs::relay_through_others() => {
                                let why = if self.no_stun { UDP_BLOCKED } else { "Couldn't reach the host from your network, and Co-op connection is set to Direct only (no relay through another player)." };
                                self.events.push(OnlineEvent::Failed(why.into()));
                                done = true;
                            }
                            Action::Failed => {
                                // No direct path: ask the other guests to relay (§8).
                                self.events.push(OnlineEvent::Note("no direct path to the host: asking the other players to relay".into()));
                                self.relays.waiting = Some(now);
                                done = true;
                            }
                            Action::Keepalive { .. } | Action::PathLost => {}
                        }
                    }
                }
                if done {
                    if let Some(line) = agent.take().and_then(|a| a.punch_summary()) {
                        self.events.push(OnlineEvent::Note(line));
                    }
                }
            }
            Side::Guest { .. } => {}
        }
        send
    }

    /// The host's answer to player `peer` asking to join.
    pub fn admit(&mut self, peer: PeerNo, yes: bool) {
        if let Side::Host { client: Some(c), peers, asking, .. } = &mut self.side {
            let Some(a) = asking.remove(&peer) else { return };
            if yes {
                c.accept(peer);
                let remotes = hosted_pair(&mut self.events, self.my_key, Some(a.key), hosted::Side::Host, a.candidates);
                peers.insert(peer, Peer { key: a.key, remotes, agent: None });
                self.events.push(OnlineEvent::Note(format!("{} (guest {peer}): let in", a.name)));
            } else {
                c.decline(peer, "declined");
                self.events.push(OnlineEvent::Note(format!("{} (guest {peer}): turned away", a.name)));
            }
            self.events.push(OnlineEvent::JoinAskOver(peer));
        }
        self.flush();
    }

    /// Asks the host has not answered in [`ASK_FOR`]: turned away.
    fn ask_timeouts(&mut self, now: Instant) {
        if let Side::Host { client: Some(c), asking, .. } = &mut self.side {
            let late: Vec<PeerNo> = asking.iter().filter(|(_, a)| now.saturating_duration_since(a.since) >= ASK_FOR).map(|(p, _)| *p).collect();
            for p in late {
                if let Some(a) = asking.remove(&p) {
                    c.decline(p, "no_answer");
                    self.events.push(OnlineEvent::Note(format!("{} (guest {p}): no answer from the host; turned away", a.name)));
                    self.events.push(OnlineEvent::JoinAskOver(p));
                }
            }
        }
    }

    /// The peer relay, once a tick: the planner's steps (C) and the checks with helpers or
    /// relayed players (both sides).
    fn poll_relays(&mut self, now: Instant) -> Vec<(SocketAddr, Vec<u8>)> {
        let mut send = Vec::new();
        let Side::Guest { me: Some(me), client, connected, remotes, .. } = &mut self.side else { return send };
        let peer_nat = report::Nat::from_candidates(remotes);
        let me = *me;
        let no_stun = self.no_stun;
        let r = &mut self.relays;
        // The planner starts once a helper is known, or after the wait with none.
        if let Some(since) = r.waiting {
            if !r.helpers.is_empty() || now.saturating_duration_since(since) >= HELPERS_WAIT {
                r.waiting = None;
                let mut pl = Planner::new(relay::Config::default());
                pl.set_helpers(&helpers(&r.helpers), now);
                pl.start(now);
                r.planner = Some(pl);
            }
        }
        // C: the checks with each helper that accepted.
        let mut done = Vec::new();
        for (&helper, (a, since)) in r.reaching.iter_mut() {
            for act in a.poll(now) {
                match act {
                    Action::SendProbe { to, check_id, .. } => send.push((to, probe(KIND_RELAY_PROBE, me, check_id))),
                    Action::Selected { remote, .. } => {
                        r.reached.insert(helper, remote);
                        done.push((helper, Some(now.saturating_duration_since(*since))));
                    }
                    Action::Failed => done.push((helper, None)),
                    Action::Keepalive { .. } | Action::PathLost | Action::OpenSockets { .. } | Action::CloseSockets { .. } => {}
                }
            }
        }
        for (helper, rtt) in done {
            if rtt.is_none() {
                r.reaching.remove(&helper);
            }
            if let Some(pl) = &mut r.planner {
                pl.on_reach(helper, rtt);
            }
        }
        if let Some(pl) = &mut r.planner {
            for act in pl.poll(now) {
                match act {
                    relay::Action::Ask { helper } => {
                        if let Some(c) = client {
                            c.relay_ask(helper, &self.found);
                        }
                        self.events.push(OnlineEvent::Note(format!("asking guest {helper} to relay")));
                    }
                    relay::Action::TryReach { helper } => {
                        let mut a = Agent::new(Role::Guest, &self.locals, Config { birthday_sockets: 0, ..Config::default() });
                        for c in r.offered.get(&helper).into_iter().flatten() {
                            a.add_remote(*c);
                        }
                        a.start(now);
                        r.reaching.insert(helper, (a, now));
                    }
                    relay::Action::Chosen { helper } => {
                        if let Some(&at) = r.reached.get(&helper) {
                            if let Some(c) = client {
                                c.connected("relay");
                            }
                            *connected = true;
                            self.events.push(OnlineEvent::Note(format!("path to the host: through guest {helper} at {at} (relayed)")));
                            log::info!("{}", Connect { host: false, path: report::Path::PeerRelay, nat: self.nat, peer_nat, elapsed: self.go_at.map_or(std::time::Duration::ZERO, |t| now.saturating_duration_since(t)) });
                            self.events.push(OnlineEvent::HostVia(at));
                        }
                    }
                    relay::Action::Lost { helper } => {
                        r.reaching.remove(&helper);
                        r.reached.remove(&helper);
                        *connected = false;
                        self.events.push(OnlineEvent::Note(format!("the relay through guest {helper} stopped working")));
                    }
                    relay::Action::GiveUp(_) if no_stun => {
                        log::info!("{}", Connect { host: false, path: report::Path::None, nat: self.nat, peer_nat, elapsed: self.go_at.map_or(std::time::Duration::ZERO, |t| now.saturating_duration_since(t)) });
                        self.events.push(OnlineEvent::Failed(UDP_BLOCKED.into()));
                    }
                    relay::Action::GiveUp(why) => {
                        log::info!("{}", Connect { host: false, path: report::Path::None, nat: self.nat, peer_nat, elapsed: self.go_at.map_or(std::time::Duration::ZERO, |t| now.saturating_duration_since(t)) });
                        let text = match why {
                            GiveUp::NoHelpers => "Couldn't reach the host from your network, and no other player can relay for you. Ask the host to turn on UPnP on their router or forward UDP port 47960, or join over Tailscale by address.",
                            GiveUp::AllRefused => "Couldn't reach the host from your network, and the other players can't relay for you (their setting is off, or they relay for others).",
                            GiveUp::NoneReachable => "Couldn't reach the host from your network, nor any player who offered to relay.",
                            GiveUp::RelaysFailed => "The players relaying for you stopped answering.",
                        };
                        self.events.push(OnlineEvent::Failed(text.into()));
                    }
                }
            }
        }
        // A: the checks with each player relayed (the probes open this side's hole too).
        for (a, _) in r.serving.values_mut() {
            for act in a.poll(now) {
                if let Action::SendProbe { to, check_id, .. } = act {
                    send.push((to, probe(KIND_RELAY_PROBE, me, check_id)));
                }
            }
        }
        send
    }

    /// The host's listing now (the browser's title, the host's place in the game, friendly
    /// fire), sent when it changed; `public: false` takes the lobby off the list.
    pub fn set_listing(&mut self, public: bool, title: &str, place: &str, friendly_fire: bool) {
        let now = (public, title.to_string(), place.to_string(), friendly_fire);
        if self.listing_sent.as_ref() == Some(&now) {
            return;
        }
        if let Side::Host { client: Some(c), .. } = &mut self.side {
            c.listing(public, title, place, friendly_fire);
            self.listing_sent = Some(now);
            self.flush();
        }
    }

    /// The host banned `key`: its later asks to join are turned away at once.
    pub fn ban(&mut self, key: [u8; 32]) {
        if !self.banned.contains(&key) {
            self.banned.push(key);
        }
    }

    /// This game stopped relaying for `peer` (it went quiet): its checks end.
    pub fn relay_ended(&mut self, peer: PeerNo) {
        self.relays.serving.remove(&peer);
    }

    /// A relay check's probe or answer from `from` (C and A both): the answer to send back.
    fn on_relay_probe(&mut self, kind: u8, peer: PeerNo, check_id: u64, from: SocketAddr, now: Instant) -> Option<(SocketAddr, Vec<u8>)> {
        let local = self.local_for(from);
        let r = &mut self.relays;
        if kind == KIND_RELAY_PROBE {
            // `peer` sent it: a helper (C's side) or a player relayed (A's side).
            if let Some((a, _)) = r.reaching.get_mut(&peer) {
                a.on_probe_received(local, from, now);
            } else if let Some((a, at)) = r.serving.get_mut(&peer) {
                a.on_probe_received(local, from, now);
                let from = crate::sockets::canonical(from);
                if *at != Some(from) {
                    *at = Some(from);
                    self.events.push(OnlineEvent::RelayAt { peer, addr: from });
                }
            } else {
                return None;
            }
            return Some((from, probe(KIND_RELAY_ANSWER, peer, check_id)));
        }
        // An answer to this game's probe: find the check.
        for (a, _) in r.reaching.values_mut() {
            if a.on_probe_result(check_id, local, from, now) {
                return None;
            }
        }
        for (&p, (a, at)) in r.serving.iter_mut() {
            if a.on_probe_result(check_id, local, from, now) {
                let from = crate::sockets::canonical(from);
                if *at != Some(from) {
                    *at = Some(from);
                    self.events.push(OnlineEvent::RelayAt { peer: p, addr: from });
                }
                return None;
            }
        }
        None
    }

    /// A guest whose host went quiet: ask the service to start the checks again (or, when
    /// relayed, the planner tries the next helper).
    pub fn path_lost(&mut self) {
        if let Some(h) = self.relays.planner.as_ref().and_then(Planner::current) {
            if let Some(pl) = &mut self.relays.planner {
                pl.on_relay_lost(h, Instant::now());
            }
            return;
        }
        if let Side::Guest { client: Some(c), connected, .. } = &mut self.side {
            if std::mem::take(connected) {
                c.reconnect();
                self.flush();
            }
        }
    }

    /// A Noise session was made with `addr`, whose key is `key`: the host stops probing for
    /// that guest.
    pub fn session_made(&mut self, key: &[u8; 32]) {
        match &mut self.side {
            Side::Host { peers, .. } => {
                for (&peer, p) in peers.iter_mut().filter(|(_, p)| p.key == *key) {
                    let Some(a) = p.agent.take() else { continue };
                    // The host's checks end here (it never selects): what its punch did.
                    if let Some(line) = a.punch_summary() {
                        self.events.push(OnlineEvent::Note(format!("guest {peer}: {line}")));
                    }
                    if !a.extra_sockets().is_empty() {
                        self.socket_asks.push(SocketAsk::Close { owner: peer, keep: None });
                    }
                }
            }
            Side::Guest { host_key: Some(k), .. } if k == key => {
                // Through a relay: the first round trip to the host came back.
                if let Some(pl) = &mut self.relays.planner {
                    if let Some(h) = pl.current() {
                        pl.on_confirmed(h);
                    }
                }
            }
            Side::Guest { .. } => {}
        }
    }

    /// A STUN answer that came to the game's socket.
    pub fn on_stun(&mut self, from: SocketAddr, bytes: &[u8]) {
        if let Some(g) = &mut self.gather {
            g.on_packet(from, bytes);
        }
    }

    /// A probe or an answer from `from`, that came in on the birthday punch's socket on port
    /// `via` (None: the game's own): the answer to send back (from that same socket), if any.
    pub fn on_probe(&mut self, bytes: &[u8], from: SocketAddr, via: Option<u16>, now: Instant) -> Option<(SocketAddr, Vec<u8>)> {
        let (kind, peer, check_id) = parse_probe(bytes)?;
        if matches!(kind, KIND_RELAY_PROBE | KIND_RELAY_ANSWER) {
            return self.on_relay_probe(kind, peer, check_id, from, now);
        }
        // Test aid: nothing of the host's checks gets through, as on a network that lets
        // nothing in (but a hosted relay's, which is not a direct path).
        if no_direct() && !crate::sockets::is_hosted_standin(from) {
            return None;
        }
        let local = match via {
            Some(port) => SocketAddr::new(self.local_for(from).ip(), port),
            None => self.local_for(from),
        };
        let agent = match &mut self.side {
            Side::Host { peers, .. } => peers.get_mut(&peer).and_then(|p| p.agent.as_mut()),
            Side::Guest { me: Some(me), agent, .. } if *me == peer => agent.as_deref_mut(),
            Side::Guest { .. } => None,
        };
        // Probes for a check this game is not running are not answered.
        let agent = agent?;
        match kind {
            KIND_PROBE => {
                agent.on_probe_received(local, from, now);
                Some((from, probe(KIND_ANSWER, peer, check_id)))
            }
            _ => {
                agent.on_probe_result(check_id, local, from, now);
                None
            }
        }
    }

    fn local_for(&self, from: SocketAddr) -> SocketAddr {
        self.locals.iter().find(|l| l.is_ipv4() == from.is_ipv4()).copied().unwrap_or(from)
    }

    /// Leaving: the service is told (a host's lobby ends) and the router's port is closed:
    /// the packet that closes it, to send from the game's IPv4 socket.
    pub fn bye(&mut self) -> Option<(SocketAddr, Vec<u8>)> {
        match &mut self.side {
            Side::Host { client: Some(c), .. } => c.bye(),
            Side::Guest { client: Some(c), .. } => c.bye(),
            _ => {}
        }
        self.flush();
        self.portmap.take().and_then(|mut pm| pm.release())
    }
}

/// A candidate on a hosted relay ([`hosted_pair`]'s stand-in).
fn is_hosted(c: &Candidate) -> bool {
    c.kind == Kind::Relayed
}

/// The other side's candidates as this game uses them, for the pair of `host_key` and
/// `guest_key` (this game is `side`): a hosted relay's address (theirs, or the one this game
/// hosts itself) opens a session for the pair on it ([`OnlineEvent::HostedOpen`]) and stands
/// in as that session's address ([`crate::sockets::hosted_standin`]). Before both keys are
/// known, relay candidates are dropped.
fn hosted_pair(events: &mut Vec<OnlineEvent>, host_key: Option<[u8; 32]>, guest_key: Option<[u8; 32]>, side: hosted::Side, candidates: Vec<Candidate>) -> Vec<Candidate> {
    let session = host_key.zip(guest_key).map(|(h, g)| hosted::session_id(&h, &g));
    let mine = crate::prefs::hosted_relay();
    let mut out = Vec::new();
    let mut open = |relay: SocketAddr, out: &mut Vec<Candidate>| {
        if let Some(s) = session {
            events.push(OnlineEvent::HostedOpen { relay, session: s, side });
            let c = Candidate::new(Kind::Relayed, crate::sockets::hosted_standin(&s));
            if !out.contains(&c) {
                out.push(c);
            }
        }
    };
    for c in candidates {
        if c.kind == Kind::Relayed {
            open(c.addr, &mut out);
        } else {
            out.push(c);
        }
    }
    if let Some(r) = mine {
        open(r, &mut out);
    }
    out
}

/// The service's helpers, round trips unknown.
fn helpers(ids: &[PeerNo]) -> Vec<Helper> {
    ids.iter().map(|&id| Helper { id, host_rtt: None }).collect()
}

fn probe(kind: u8, peer: PeerNo, check_id: u64) -> Vec<u8> {
    let mut b = Vec::with_capacity(18);
    b.extend_from_slice(&crate::net::MAGIC);
    b.push(crate::net::VERSION);
    b.push(kind);
    b.extend_from_slice(&peer.to_le_bytes());
    b.extend_from_slice(&check_id.to_le_bytes());
    b
}

/// A probe or an answer: kind, the guest's number, the check id.
pub fn parse_probe(b: &[u8]) -> Option<(u8, PeerNo, u64)> {
    if b.len() != 18 || b[..4] != crate::net::MAGIC || b[4] != crate::net::VERSION || !matches!(b[5], KIND_PROBE | KIND_ANSWER | KIND_RELAY_PROBE | KIND_RELAY_ANSWER) {
        return None;
    }
    Some((b[5], u32::from_le_bytes(b[6..10].try_into().ok()?), u64::from_le_bytes(b[10..18].try_into().ok()?)))
}

pub fn is_probe(b: &[u8]) -> bool {
    parse_probe(b).is_some()
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn txid() -> stun::TxId {
    let a = crate::net::new_token().to_le_bytes();
    let b = crate::net::new_token().to_le_bytes();
    let mut t = [0u8; 12];
    t[..8].copy_from_slice(&a);
    t[8..].copy_from_slice(&b[..4]);
    t
}

fn default_stun() -> Vec<SocketAddr> {
    signal_io::resolve_stun(&signal_io::DEFAULT_STUN.map(String::from))
}

/// This PC's addresses the other side may reach directly, with these ports: first the IPv4
/// and global IPv6 address it would send to the internet from (asked of the system;
/// nothing is sent), then every other private IPv4 and global IPv6 address of its network
/// adapters that are up. With a VPN up, the first is the tunnel's, and the home network's
/// own address (where a player on the same network reaches it) is only among the others
/// [observed 2026-11-24: a desktop on NordVPN offered only 10.5.0.2 and a laptop on its
/// Wi-Fi could not reach it].
pub fn local_addrs(v4_port: Option<u16>, v6_port: Option<u16>) -> Vec<SocketAddr> {
    let mut out = route_addrs(v4_port, v6_port);
    for iface in netdev::get_interfaces().into_iter().filter(|i| i.is_up() && !i.is_loopback()) {
        let v4 = iface.ipv4.iter().map(|n| n.addr()).filter(|ip| ip.is_private()).filter_map(|ip| v4_port.map(|p| SocketAddr::new(IpAddr::V4(ip), p)));
        let v6 = iface.ipv6.iter().map(|n| n.addr()).filter(|ip| nvf_nat::candidate::is_global_v6(*ip)).filter_map(|ip| v6_port.map(|p| SocketAddr::new(IpAddr::V6(ip), p)));
        for a in v4.chain(v6) {
            if !out.contains(&a) && out.len() < LOCAL_ADDRS {
                out.push(a);
            }
        }
    }
    out
}

/// At most this many of this PC's own addresses are offered.
const LOCAL_ADDRS: usize = 6;

/// The addresses the system would send to the internet from.
fn route_addrs(v4_port: Option<u16>, v6_port: Option<u16>) -> Vec<SocketAddr> {
    let mut out = Vec::new();
    if let Some(p) = v4_port {
        let ip = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).and_then(|s| s.connect(("192.0.2.1", 9)).and(s.local_addr())).ok().map(|a| a.ip());
        if let Some(ip @ IpAddr::V4(v4)) = ip {
            if !v4.is_unspecified() && !v4.is_loopback() {
                out.push(SocketAddr::new(ip, p));
            }
        }
    }
    if let Some(p) = v6_port {
        let ip = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0)).and_then(|s| s.connect(("2001:db8::1", 9)).and(s.local_addr())).ok().map(|a| a.ip());
        if let Some(ip @ IpAddr::V6(v6)) = ip {
            if nvf_nat::candidate::is_global_v6(v6) {
                out.push(SocketAddr::new(ip, p));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    /// The birthday-only aid drops just the plain checks to candidates passed on.
    #[test]
    fn birthday_only_drops_plain_checks_only() {
        let passed: SocketAddr = "203.0.113.7:47960".parse().unwrap();
        let learnt: SocketAddr = "203.0.113.7:51000".parse().unwrap();
        let cands = [Candidate::new(Kind::ServerReflexive, passed)];
        assert!(super::plain_check(3, true, passed, &cands));
        assert!(!super::plain_check(super::NO_PAIR, true, passed, &cands), "spread probes and spray go");
        assert!(!super::plain_check(3, false, passed, &cands), "from an extra socket goes");
        assert!(!super::plain_check(3, true, learnt, &cands), "to a peer-reflexive address learnt goes");
    }

    use super::*;
    use std::time::Duration;

    #[test]
    fn probes_round_trip_and_nothing_else_is_one() {
        let p = probe(KIND_PROBE, 7, 0xDEAD_BEEF);
        assert_eq!(parse_probe(&p), Some((KIND_PROBE, 7, 0xDEAD_BEEF)));
        assert_eq!(parse_probe(&probe(KIND_ANSWER, 1, 2)), Some((KIND_ANSWER, 1, 2)));
        assert!(!is_probe(&p[..17]));
        assert!(!is_probe(b"NVCO\x8a\xe0................"));
        assert!(!stun::is_stun(&p), "a probe is never taken for STUN");
    }

    #[test]
    fn a_probe_for_no_check_is_not_answered() {
        let mut o = Online::new("http://127.0.0.1:1", None, Side::Host { client: None, peers: HashMap::new(), asking: HashMap::new(), secret: None, code: None }, vec!["192.168.1.2:47960".parse().unwrap()]);
        assert_eq!(o.on_probe(&probe(KIND_PROBE, 3, 9), "203.0.113.9:5000".parse().unwrap(), None, Instant::now()), None);
    }

    fn guest_at(me: PeerNo, at: &str, connected: bool) -> Online {
        let client = GuestClient::new("G", &"cd".repeat(32), &[]);
        let side = Side::Guest { name: "G".into(), client: Some(client), me: Some(me), host_key: Some([7; 32]), remotes: Vec::new(), agent: None, load_order_hash: String::new(), connected };
        Online::new("http://127.0.0.1:1", None, side, vec![at.parse().unwrap()])
    }

    /// A wrong password: the decline is what the player sees, not the socket's close after it.
    #[test]
    fn a_decline_is_not_overwritten_by_the_close() {
        let mut o = guest_at(3, "10.0.0.3:50003", false);
        o.take_io(FromIo::Text(r#"{"type":"declined","reason":"bad_password"}"#.into()), Instant::now(), &[3; 32]);
        o.take_io(FromIo::Closed, Instant::now(), &[3; 32]);
        let ev = o.take_events();
        assert_eq!(ev.iter().filter(|e| matches!(e, OnlineEvent::Failed(_))).count(), 1, "{ev:?}");
        assert!(ev.contains(&OnlineEvent::Failed("Wrong password for this lobby.".into())));
    }

    /// No STUN server answers and the join fails: the player is told UDP looks blocked.
    #[test]
    fn a_join_with_no_stun_answer_says_udp_is_blocked() {
        let mut o = guest_at(3, "10.0.0.3:50003", false);
        let mut now = Instant::now();
        o.start_stun(&["192.0.2.1:3478".parse().unwrap()], now);
        o.on_text(r#"{"type":"go"}"#, now);
        let mut ev = Vec::new();
        for _ in 0..1500 {
            now += Duration::from_millis(20);
            o.tick(now, &[3; 32]);
            ev.extend(o.take_events());
            if ev.iter().any(|e| matches!(e, OnlineEvent::Failed(_))) {
                break;
            }
        }
        assert!(ev.contains(&OnlineEvent::Failed(UDP_BLOCKED.into())), "{ev:?}");
    }

    /// The peer relay's setup (docs/design/coop-networking.md §8), sans service and sockets:
    /// C's direct checks fail, it asks helper A (from the service's `helpers`), A accepts,
    /// their relay checks meet, C's path to the host is A's address and A relays for C at
    /// C's; the host's key over that path confirms it.
    #[test]
    fn a_guest_with_no_path_is_relayed_by_another() {
        let (a_at, c_at): (SocketAddr, SocketAddr) = ("192.168.1.2:50002".parse().unwrap(), "10.0.0.3:50003".parse().unwrap());
        let mut a = guest_at(2, "192.168.1.2:50002", true);
        let mut c = guest_at(3, "10.0.0.3:50003", false);
        let mut now = Instant::now();
        c.on_text(r#"{"type":"helpers","peers":[2,3]}"#, now);
        c.on_text(r#"{"type":"go"}"#, now);
        let (mut a_ev, mut c_ev) = (Vec::new(), Vec::new());
        // No host candidates: the direct checks give up, and the planner asks A.
        for _ in 0..600 {
            now += Duration::from_millis(20);
            c.tick(now, &[3; 32]);
            c_ev.extend(c.take_events());
            if c_ev.iter().any(|e| matches!(e, OnlineEvent::Note(n) if n == "asking guest 2 to relay")) {
                break;
            }
        }
        assert!(c_ev.iter().any(|e| matches!(e, OnlineEvent::Note(n) if n == "asking guest 2 to relay")), "{c_ev:?}");
        assert!(!c_ev.iter().any(|e| matches!(e, OnlineEvent::Failed(_))), "not failed yet: {c_ev:?}");
        a.on_text(r#"{"type":"relay_ask","from":3,"candidates":[{"kind":"host","addr":"10.0.0.3:50003"}]}"#, now);
        a_ev.extend(a.take_events());
        assert!(a_ev.contains(&OnlineEvent::RelayFor(3)), "{a_ev:?}");
        c.on_text(r#"{"type":"relay_answer","helper":2,"accept":true,"candidates":[{"kind":"host","addr":"192.168.1.2:50002"}]}"#, now);
        for _ in 0..400 {
            now += Duration::from_millis(20);
            let (to_a, to_c) = (c.tick(now, &[3; 32]), a.tick(now, &[2; 32]));
            for (to, b) in to_a.into_iter().filter(|(to, _)| *to == a_at) {
                let _ = to;
                if let Some((back, ans)) = a.on_probe(&b, c_at, None, now) {
                    assert_eq!(back, c_at);
                    c.on_probe(&ans, a_at, None, now);
                }
            }
            for (_, b) in to_c.into_iter().filter(|(to, _)| *to == c_at) {
                if let Some((_, ans)) = c.on_probe(&b, a_at, None, now) {
                    a.on_probe(&ans, c_at, None, now);
                }
            }
            a_ev.extend(a.take_events());
            c_ev.extend(c.take_events());
            if c_ev.contains(&OnlineEvent::HostVia(a_at)) {
                break;
            }
        }
        assert!(c_ev.contains(&OnlineEvent::HostVia(a_at)), "{c_ev:?}");
        assert!(a_ev.contains(&OnlineEvent::RelayAt { peer: 3, addr: c_at }), "{a_ev:?}");
        // The host's key, through A: confirmed, and the relay holds past the confirm time.
        c.session_made(&[7; 32]);
        now += Duration::from_secs(5);
        c.tick(now, &[3; 32]);
        assert!(!c.take_events().iter().any(|e| matches!(e, OnlineEvent::Failed(_) | OnlineEvent::HostVia(_))));
        // A refuses a third player when its setting is off.
        std::env::set_var(RELAY_ENV, "0");
        a.on_text(r#"{"type":"relay_ask","from":4,"candidates":[]}"#, now);
        std::env::remove_var(RELAY_ENV);
        assert!(!a.take_events().contains(&OnlineEvent::RelayFor(4)));
    }

    #[test]
    fn a_host_checks_with_a_guest_it_let_in() {
        let mut o = Online::new("http://127.0.0.1:1", None, Side::Host { client: Some(HostClient::new("s")), peers: HashMap::new(), asking: HashMap::new(), secret: Some("s".into()), code: None }, vec!["192.168.1.2:47960".parse().unwrap()]);
        let now = Instant::now();
        let key = "ab".repeat(32);
        // Join approval: the host is asked first, and lets this one in.
        o.on_text(&format!(r#"{{"type":"join","peer":2,"name":"Late","key":"{key}","candidates":[]}}"#), now);
        assert!(o.take_events().contains(&OnlineEvent::JoinAsk { peer: 2, name: "Late (abab)".into() }));
        o.tick(now + ASK_FOR + Duration::from_secs(1), &[0; 32]);
        assert!(o.take_events().contains(&OnlineEvent::JoinAskOver(2)), "unanswered: turned away");
        o.on_text(&format!(r#"{{"type":"join","peer":1,"name":"G","key":"{key}","candidates":[{{"kind":"host","addr":"192.168.1.3:50000"}}]}}"#), now);
        assert!(o.take_events().contains(&OnlineEvent::JoinAsk { peer: 1, name: "G (abab)".into() }));
        o.admit(1, true);
        assert!(o.take_events().contains(&OnlineEvent::JoinAskOver(1)));
        o.on_text(r#"{"type":"go","peer":1}"#, now);
        let sent = o.tick(now, &[0; 32]);
        assert!(sent.iter().any(|(to, b)| to.to_string() == "192.168.1.3:50000" && parse_probe(b).is_some_and(|(k, p, _)| k == KIND_PROBE && p == 1)), "{sent:?}");
        let from: SocketAddr = "192.168.1.3:50000".parse().unwrap();
        let answer = o.on_probe(&probe(KIND_PROBE, 1, 42), from, None, now);
        assert_eq!(answer, Some((from, probe(KIND_ANSWER, 1, 42))));
        o.session_made(&unhex32(&key).unwrap());
        assert_eq!(o.on_probe(&probe(KIND_PROBE, 1, 43), from, None, now), None, "checks over once the guest's session is made");
    }
}
