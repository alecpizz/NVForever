//! Every co-op packet encrypted and authenticated (docs/design/coop-networking.md §4, phase
//! 2), with the Noise protocol's `XX` handshake (`Noise_XX_25519_ChaChaPoly_BLAKE2s`, the
//! `snow` crate). Nothing here does I/O: [`Secure`] takes the packets that came in and gives
//! the packets to send, each with the address it goes to, and [`crate::net`] routes them.
//!
//! **Identity.** Each game has a key pair ([`Identity`], kept in its home by the game). Its
//! public key is the player to the host: a guest coming back (another address, a restarted
//! game) is known by it, where the plaintext session token was before.
//!
//! **Packets.** The co-op magic and protocol version (as [`crate::net`]'s packets), then a
//! kind byte from a range no game message uses:
//!
//! | Kind | Fields after the kind byte |
//! |---|---|
//! | `0xE1` handshake 1 (guest) | sender index (u32), Noise message 1 (its payload: the lobby key, u32) |
//! | `0xE2` handshake 2 (host) | sender index, receiver index, Noise message 2 |
//! | `0xE3` handshake 3 (guest) | sender index, receiver index, Noise message 3 |
//! | `0xE0` data | receiver index, nonce (u64), ciphertext and its 16-byte tag |
//! | `0xE4` unknown session (host) | nothing: a data packet came for a session the host does not have (it started again) |
//!
//! All little-endian. An index names one side's session to the other, so a data packet finds
//! its session whatever address it came from: a guest whose address changed (a Wi-Fi blip, a
//! router that moved its port) keeps its session, and the packet's address becomes the
//! session's ("roaming"). That only happens after the packet decrypted and passed the
//! replay check, so a copy sent again from elsewhere cannot move it.
//!
//! **A host started again** knows none of its guests' sessions: it answers their data with
//! `0xE4`, and a guest that has not heard its host on the session for [`QUIET`] starts a
//! new handshake at once instead of waiting to find the host lost. The hint is not
//! authenticated; at worst a forged one makes a quiet session start again.
//!
//! **Replay.** Each session takes a nonce once, within the newest [`WINDOW`] (2048). Older or
//! repeated packets are dropped, as UDP may drop any.
//!
//! **Lost handshake packets.** A guest sends message 1 again (the same bytes) until message
//! 2 comes; the host answers a repeated message 1 with its message 2 again. A guest's
//! message 3 goes again before each data packet until the host's first packet on the
//! session arrives, so a lost message 3 never strands a session only one side has.
//!
//! What is not here yet: pinning the host's key (the matchmaking service hands it over in
//! phase 3; LAN codes carry its hash), and a cookie against handshake floods (a cap on
//! handshakes waiting is all).

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use snow::params::NoiseParams;
use snow::{Builder, HandshakeState, StatelessTransportState};

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
/// Bound into every handshake: a game of another protocol cannot complete one.
const PROLOGUE: &[u8] = b"NVForever co-op";

pub const KIND_DATA: u8 = 0xE0;
pub const KIND_HS1: u8 = 0xE1;
pub const KIND_HS2: u8 = 0xE2;
pub const KIND_HS3: u8 = 0xE3;
pub const KIND_RESET: u8 = 0xE4;
/// A guest takes the host's unknown-session hint only when it has not heard the host on
/// the session for this long.
pub const QUIET: Duration = Duration::from_secs(1);

/// The tag each data packet carries.
pub const TAG: usize = 16;
/// The bytes a data packet adds to the message it carries: header, kind, index, nonce, tag.
pub const OVERHEAD: usize = 4 + 1 + 1 + 4 + 8 + TAG;
/// Nonces a session takes out of order.
pub const WINDOW: u64 = 2048;
/// Handshakes the host keeps waiting for message 3; past this the oldest goes.
const PENDING_MAX: usize = 64;
/// A handshake not finished in this long is dropped.
const PENDING_FOR: Duration = Duration::from_secs(10);
/// A session that heard nothing for this long is dropped (longer than `net::GRACE`).
const IDLE_FOR: Duration = Duration::from_secs(300);
const MAX_PACKET: usize = 2048;

fn params() -> NoiseParams {
    PATTERN.parse().expect("the Noise pattern parses")
}

/// A game's key pair.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    private: [u8; 32],
    public: [u8; 32],
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Identity({})", self.fingerprint())
    }
}

impl Identity {
    /// A new key pair, from the system's random source.
    pub fn generate() -> Self {
        let kp = Builder::new(params()).generate_keypair().expect("the system has a random source");
        Self { private: kp.private.try_into().expect("a 32-byte key"), public: kp.public.try_into().expect("a 32-byte key") }
    }

    pub fn public(&self) -> [u8; 32] {
        self.public
    }

    /// The first 8 bytes of the public key in hex, for logs.
    pub fn fingerprint(&self) -> String {
        hex(&self.public[..8])
    }

    /// As kept in a file: the private key then the public key, in hex, space between.
    pub fn to_text(&self) -> String {
        format!("{} {}", hex(&self.private), hex(&self.public))
    }

    /// What [`Identity::to_text`] wrote; None for anything else.
    pub fn from_text(text: &str) -> Option<Self> {
        let mut parts = text.split_whitespace();
        let private = unhex32(parts.next()?)?;
        let public = unhex32(parts.next()?)?;
        Some(Self { private, public })
    }
}

/// The first 8 bytes of a public key in hex, for logs.
pub fn fingerprint(key: &[u8; 32]) -> String {
    hex(&key[..8])
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
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

/// The nonces a session took, within the newest [`WINDOW`]: bit `k` is nonce `top - 1 - k`.
#[derive(Clone, Default)]
struct Replay {
    top: u64,
    bits: [u64; (WINDOW / 64) as usize],
}

impl Replay {
    /// Whether `n` is new, and if so take it.
    fn accept(&mut self, n: u64) -> bool {
        if n >= self.top {
            let shift = n - self.top + 1;
            self.shift(shift);
            self.top = n + 1;
            self.bits[0] |= 1;
            return true;
        }
        let back = self.top - 1 - n;
        if back >= WINDOW {
            return false;
        }
        let (w, b) = ((back / 64) as usize, back % 64);
        if self.bits[w] & (1 << b) != 0 {
            return false;
        }
        self.bits[w] |= 1 << b;
        true
    }

    /// Old bit `k` becomes bit `k + by`.
    fn shift(&mut self, by: u64) {
        if by >= WINDOW {
            self.bits = Default::default();
            return;
        }
        let (words, b) = ((by / 64) as usize, (by % 64) as u32);
        let n = self.bits.len();
        for i in (0..n).rev() {
            let src = i.checked_sub(words);
            let hi = src.map_or(0, |s| self.bits[s] << b);
            let lo = match (src.and_then(|s| s.checked_sub(1)), b) {
                (Some(s), b) if b > 0 => self.bits[s] >> (64 - b),
                _ => 0,
            };
            self.bits[i] = hi | lo;
        }
    }
}

struct Session {
    addr: SocketAddr,
    remote_index: u32,
    transport: StatelessTransportState,
    send_nonce: u64,
    replay: Replay,
    remote_key: [u8; 32],
    last_heard: Instant,
    /// A guest's message 3, sent again before each data packet until the host is heard on
    /// this session.
    unconfirmed: Option<Vec<u8>>,
}

/// A guest's handshake waiting for message 2: message 1 (sent again as it is), and the
/// address it went to (a broadcast address, perhaps: message 2 comes from the host's own).
struct Initiating {
    hs: HandshakeState,
    msg1: Vec<u8>,
    target: SocketAddr,
    started: Instant,
}

/// A host's handshake waiting for message 3.
struct Responding {
    hs: HandshakeState,
    addr: SocketAddr,
    remote_index: u32,
    msg2: Vec<u8>,
    started: Instant,
}

/// What a packet that came in was.
#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    /// A game message, decrypted, from `from` (the session's address now).
    Message { bytes: Vec<u8>, from: SocketAddr },
    /// Part of a handshake: these packets go out (each to its address), and when a session
    /// was made, with whom.
    Handshake { send: Vec<(SocketAddr, Vec<u8>)>, made: Option<SocketAddr> },
    /// A data packet for a session this side does not have, from `from` (a host answers
    /// it with [`Secure::unknown_session`]).
    UnknownSession { from: SocketAddr },
    /// The host's unknown-session hint, from `from`.
    ResetHint { from: SocketAddr },
    /// Not ours, not authentic, a repeat or out of date: dropped.
    Dropped,
}

/// A game's Noise sessions with its peers.
pub struct Secure {
    me: Identity,
    /// A host's lobby key (a guest's message 1 for another lobby is ignored); a guest's,
    /// carried in its message 1.
    lobby: Option<u32>,
    sessions: HashMap<u32, Session>,
    by_addr: HashMap<SocketAddr, u32>,
    initiating: HashMap<u32, Initiating>,
    initiating_to: HashMap<SocketAddr, u32>,
    responding: HashMap<u32, Responding>,
    next_index: u32,
}

impl Secure {
    pub fn new(me: Identity) -> Self {
        // Indices start anywhere, so a restarted game's do not repeat its last run's.
        let start = crate::net::new_token() as u32;
        Self { me, lobby: None, sessions: HashMap::new(), by_addr: HashMap::new(), initiating: HashMap::new(), initiating_to: HashMap::new(), responding: HashMap::new(), next_index: start }
    }

    pub fn identity(&self) -> &Identity {
        &self.me
    }

    /// A new identity: every session and handshake is dropped (they were the old key's).
    pub fn set_identity(&mut self, me: Identity) {
        *self = Self { lobby: self.lobby, ..Self::new(me) };
    }

    pub fn set_lobby(&mut self, lobby: Option<u32>) {
        self.lobby = lobby;
    }

    fn index(&mut self) -> u32 {
        loop {
            self.next_index = self.next_index.wrapping_add(1);
            let i = self.next_index;
            if !self.sessions.contains_key(&i) && !self.initiating.contains_key(&i) && !self.responding.contains_key(&i) {
                return i;
            }
        }
    }

    fn builder(&self) -> Builder<'_> {
        Builder::new(params()).prologue(PROLOGUE).expect("a prologue").local_private_key(&self.me.private).expect("a 32-byte key")
    }

    /// Whether there is a session with `addr`.
    pub fn has_session(&self, addr: SocketAddr) -> bool {
        self.by_addr.contains_key(&addr)
    }

    /// The public key of the peer at `addr`, once a session with it was made.
    pub fn remote_key(&self, addr: SocketAddr) -> Option<[u8; 32]> {
        self.by_addr.get(&addr).and_then(|i| self.sessions.get(i)).map(|s| s.remote_key)
    }

    /// The addresses there are sessions with.
    pub fn session_addrs(&self) -> Vec<SocketAddr> {
        self.by_addr.keys().copied().collect()
    }

    /// The host's answer to a data packet for a session it does not have.
    pub fn unknown_session() -> Vec<u8> {
        header(KIND_RESET)
    }

    /// Whether the peer at `addr` was heard on its session within `within` of `now`.
    pub fn heard_within(&self, addr: SocketAddr, now: Instant, within: Duration) -> bool {
        self.by_addr.get(&addr).and_then(|i| self.sessions.get(i)).is_some_and(|s| now.saturating_duration_since(s.last_heard) < within)
    }

    /// Drop the session with `addr` (a guest that lost its host starts a new one).
    pub fn forget(&mut self, addr: SocketAddr) {
        if let Some(i) = self.by_addr.remove(&addr) {
            self.sessions.remove(&i);
        }
        if let Some(i) = self.initiating_to.remove(&addr) {
            self.initiating.remove(&i);
        }
    }

    /// A guest: message 1 to `target` (the one already started there, sent again as it is).
    pub fn initiate(&mut self, target: SocketAddr, now: Instant) -> Vec<u8> {
        if let Some(p) = self.initiating_to.get(&target).and_then(|i| self.initiating.get(i)) {
            if now.duration_since(p.started) < PENDING_FOR {
                return p.msg1.clone();
            }
        }
        if let Some(old) = self.initiating_to.remove(&target) {
            self.initiating.remove(&old);
        }
        let index = self.index();
        let mut hs = self.builder().build_initiator().expect("an XX initiator");
        let mut buf = [0u8; MAX_PACKET];
        let payload = self.lobby.unwrap_or(0).to_le_bytes();
        let n = hs.write_message(&payload, &mut buf).expect("message 1 fits");
        let mut msg1 = header(KIND_HS1);
        msg1.extend_from_slice(&index.to_le_bytes());
        msg1.extend_from_slice(&buf[..n]);
        self.initiating.insert(index, Initiating { hs, msg1: msg1.clone(), target, started: now });
        self.initiating_to.insert(target, index);
        msg1
    }

    /// `plain` sealed for the peer at `addr`: the packets to send there (a guest's message
    /// 3 first while the host has not been heard on the session). None without a session.
    pub fn seal(&mut self, addr: SocketAddr, plain: &[u8]) -> Option<Vec<Vec<u8>>> {
        let index = *self.by_addr.get(&addr)?;
        let s = self.sessions.get_mut(&index)?;
        let nonce = s.send_nonce;
        s.send_nonce += 1;
        let mut out = header(KIND_DATA);
        out.extend_from_slice(&s.remote_index.to_le_bytes());
        out.extend_from_slice(&nonce.to_le_bytes());
        let start = out.len();
        out.resize(start + plain.len() + TAG, 0);
        let n = s.transport.write_message(nonce, plain, &mut out[start..]).ok()?;
        out.truncate(start + n);
        let mut packets = Vec::with_capacity(2);
        if let Some(m3) = &s.unconfirmed {
            packets.push(m3.clone());
        }
        packets.push(out);
        Some(packets)
    }

    /// A packet that came in from `from`.
    pub fn open(&mut self, bytes: &[u8], from: SocketAddr, now: Instant) -> Opened {
        let Some(kind) = kind_of(bytes) else { return Opened::Dropped };
        let body = &bytes[6..];
        match kind {
            KIND_DATA => self.data(body, from, now),
            KIND_HS1 => self.hs1(body, from, now),
            KIND_HS2 => self.hs2(body, from, now),
            KIND_HS3 => self.hs3(body, from, now),
            KIND_RESET => Opened::ResetHint { from },
            _ => Opened::Dropped,
        }
    }

    fn data(&mut self, body: &[u8], from: SocketAddr, now: Instant) -> Opened {
        if body.len() < 12 + TAG {
            return Opened::Dropped;
        }
        let index = u32::from_le_bytes(body[..4].try_into().unwrap());
        let nonce = u64::from_le_bytes(body[4..12].try_into().unwrap());
        let Some(s) = self.sessions.get_mut(&index) else { return Opened::UnknownSession { from } };
        let mut plain = vec![0u8; body.len() - 12];
        let Ok(n) = s.transport.read_message(nonce, &body[12..], &mut plain) else { return Opened::Dropped };
        if !s.replay.accept(nonce) {
            return Opened::Dropped;
        }
        plain.truncate(n);
        s.last_heard = now;
        s.unconfirmed = None;
        if s.addr != from {
            // Roaming: the session's peer is at `from` now.
            let old = std::mem::replace(&mut s.addr, from);
            if self.by_addr.get(&old) == Some(&index) {
                self.by_addr.remove(&old);
            }
            if let Some(other) = self.by_addr.insert(from, index) {
                if other != index {
                    self.sessions.remove(&other);
                }
            }
        }
        Opened::Message { bytes: plain, from }
    }

    /// A host: a guest's message 1.
    fn hs1(&mut self, body: &[u8], from: SocketAddr, now: Instant) -> Opened {
        if body.len() < 4 {
            return Opened::Dropped;
        }
        let remote_index = u32::from_le_bytes(body[..4].try_into().unwrap());
        // The same message 1 again (its message 2 was lost): the same answer.
        if let Some(r) = self.responding.values().find(|r| r.addr == from && r.remote_index == remote_index) {
            return Opened::Handshake { send: vec![(from, r.msg2.clone())], made: None };
        }
        let mut hs = self.builder().build_responder().expect("an XX responder");
        let mut payload = [0u8; 64];
        let Ok(n) = hs.read_message(&body[4..], &mut payload) else { return Opened::Dropped };
        let lobby = if n >= 4 { u32::from_le_bytes(payload[..4].try_into().unwrap()) } else { 0 };
        // Another lobby's guest asking around its network.
        if lobby != 0 && self.lobby.is_some_and(|k| k != lobby) {
            return Opened::Dropped;
        }
        let index = self.index();
        let mut buf = [0u8; MAX_PACKET];
        let Ok(n) = hs.write_message(&[], &mut buf) else { return Opened::Dropped };
        let mut msg2 = header(KIND_HS2);
        msg2.extend_from_slice(&index.to_le_bytes());
        msg2.extend_from_slice(&remote_index.to_le_bytes());
        msg2.extend_from_slice(&buf[..n]);
        if self.responding.len() >= PENDING_MAX {
            if let Some(oldest) = self.responding.iter().min_by_key(|(_, r)| r.started).map(|(i, _)| *i) {
                self.responding.remove(&oldest);
            }
        }
        self.responding.insert(index, Responding { hs, addr: from, remote_index, msg2: msg2.clone(), started: now });
        Opened::Handshake { send: vec![(from, msg2)], made: None }
    }

    /// A guest: the host's message 2. The session is made, and message 3 goes back.
    fn hs2(&mut self, body: &[u8], from: SocketAddr, now: Instant) -> Opened {
        if body.len() < 8 {
            return Opened::Dropped;
        }
        let remote_index = u32::from_le_bytes(body[..4].try_into().unwrap());
        let index = u32::from_le_bytes(body[4..8].try_into().unwrap());
        let Some(mut p) = self.initiating.remove(&index) else { return Opened::Dropped };
        let mut payload = [0u8; 64];
        if p.hs.read_message(&body[8..], &mut payload).is_err() {
            // Not the host's answer after all: keep waiting for one that is.
            self.initiating.insert(index, p);
            return Opened::Dropped;
        }
        self.initiating_to.remove(&p.target);
        let mut buf = [0u8; MAX_PACKET];
        let Ok(n) = p.hs.write_message(&[], &mut buf) else { return Opened::Dropped };
        let mut msg3 = header(KIND_HS3);
        msg3.extend_from_slice(&index.to_le_bytes());
        msg3.extend_from_slice(&remote_index.to_le_bytes());
        msg3.extend_from_slice(&buf[..n]);
        let Some(remote_key) = p.hs.get_remote_static().and_then(|k| <[u8; 32]>::try_from(k).ok()) else { return Opened::Dropped };
        let Ok(transport) = p.hs.into_stateless_transport_mode() else { return Opened::Dropped };
        self.install(index, Session { addr: from, remote_index, transport, send_nonce: 0, replay: Replay::default(), remote_key, last_heard: now, unconfirmed: Some(msg3.clone()) });
        Opened::Handshake { send: vec![(from, msg3)], made: Some(from) }
    }

    /// A host: a guest's message 3. The session is made.
    fn hs3(&mut self, body: &[u8], from: SocketAddr, now: Instant) -> Opened {
        if body.len() < 8 {
            return Opened::Dropped;
        }
        let remote_index = u32::from_le_bytes(body[..4].try_into().unwrap());
        let index = u32::from_le_bytes(body[4..8].try_into().unwrap());
        let Some(mut r) = self.responding.remove(&index) else { return Opened::Dropped };
        if r.remote_index != remote_index {
            self.responding.insert(index, r);
            return Opened::Dropped;
        }
        let mut payload = [0u8; 64];
        if r.hs.read_message(&body[8..], &mut payload).is_err() {
            self.responding.insert(index, r);
            return Opened::Dropped;
        }
        let Some(remote_key) = r.hs.get_remote_static().and_then(|k| <[u8; 32]>::try_from(k).ok()) else { return Opened::Dropped };
        let Ok(transport) = r.hs.into_stateless_transport_mode() else { return Opened::Dropped };
        let _ = r.addr;
        self.install(index, Session { addr: from, remote_index, transport, send_nonce: 0, replay: Replay::default(), remote_key, last_heard: now, unconfirmed: None });
        Opened::Handshake { send: Vec::new(), made: Some(from) }
    }

    /// A new session at its address, in place of any older one there.
    fn install(&mut self, index: u32, s: Session) {
        if let Some(old) = self.by_addr.insert(s.addr, index) {
            self.sessions.remove(&old);
        }
        self.sessions.insert(index, s);
    }

    /// Drop handshakes not finished and sessions not heard from in a long while.
    pub fn prune(&mut self, now: Instant) {
        self.initiating.retain(|_, p| now.duration_since(p.started) < PENDING_FOR);
        let live: Vec<u32> = self.initiating.keys().copied().collect();
        self.initiating_to.retain(|_, i| live.contains(i));
        self.responding.retain(|_, r| now.duration_since(r.started) < PENDING_FOR);
        let stale: Vec<u32> = self.sessions.iter().filter(|(_, s)| now.duration_since(s.last_heard) >= IDLE_FOR).map(|(i, _)| *i).collect();
        for i in stale {
            if let Some(s) = self.sessions.remove(&i) {
                if self.by_addr.get(&s.addr) == Some(&i) {
                    self.by_addr.remove(&s.addr);
                }
            }
        }
    }
}

fn header(kind: u8) -> Vec<u8> {
    let mut b = Vec::with_capacity(96);
    b.extend_from_slice(&crate::net::MAGIC);
    b.push(crate::net::VERSION);
    b.push(kind);
    b
}

/// The kind byte of a packet of this layer (magic and version checked), else None.
pub fn kind_of(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < 6 || bytes[..4] != crate::net::MAGIC || bytes[4] != crate::net::VERSION {
        return None;
    }
    matches!(bytes[5], KIND_DATA | KIND_HS1 | KIND_HS2 | KIND_HS3 | KIND_RESET).then_some(bytes[5])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(p: u16) -> SocketAddr {
        SocketAddr::from(([10, 0, 0, 1], p))
    }

    /// A guest and a host, handshaken: (guest, host, the host's address, the guest's).
    fn pair() -> (Secure, Secure, SocketAddr, SocketAddr) {
        let now = Instant::now();
        let (h_at, g_at) = (addr(47960), addr(50000));
        let mut g = Secure::new(Identity::generate());
        let mut h = Secure::new(Identity::generate());
        let m1 = g.initiate(h_at, now);
        let Opened::Handshake { send, made: None } = h.open(&m1, g_at, now) else { panic!("message 2") };
        let Opened::Handshake { send: s3, made: Some(m) } = g.open(&send[0].1, h_at, now) else { panic!("message 3") };
        assert_eq!(m, h_at);
        let Opened::Handshake { made: Some(m), .. } = h.open(&s3[0].1, g_at, now) else { panic!("host session") };
        assert_eq!(m, g_at);
        (g, h, h_at, g_at)
    }

    #[test]
    fn identities_round_trip_as_text() {
        let id = Identity::generate();
        assert_eq!(Identity::from_text(&id.to_text()), Some(id.clone()));
        assert_eq!(Identity::from_text("nonsense"), None);
        assert_ne!(Identity::generate().public(), id.public());
    }

    #[test]
    fn a_handshake_makes_sessions_both_ways_and_each_knows_the_others_key() {
        let (mut g, mut h, h_at, g_at) = pair();
        assert_eq!(h.remote_key(g_at), Some(g.identity().public()));
        assert_eq!(g.remote_key(h_at), Some(h.identity().public()));
        let now = Instant::now();
        let packets = g.seal(h_at, b"hello").unwrap();
        assert_eq!(packets.len(), 2, "message 3 again, until the host is heard");
        assert!(matches!(h.open(&packets[0], g_at, now), Opened::Dropped), "a repeated message 3 is ignored");
        assert_eq!(h.open(&packets[1], g_at, now), Opened::Message { bytes: b"hello".to_vec(), from: g_at });
        let back = h.seal(g_at, b"welcome").unwrap();
        assert_eq!(g.open(&back[0], h_at, now), Opened::Message { bytes: b"welcome".to_vec(), from: h_at });
        assert_eq!(g.seal(h_at, b"x").unwrap().len(), 1, "the host was heard: no more message 3");
    }

    #[test]
    fn forged_and_replayed_packets_are_dropped() {
        let (mut g, mut h, h_at, g_at) = pair();
        let now = Instant::now();
        let p = g.seal(h_at, b"state").unwrap().pop().unwrap();
        assert!(matches!(h.open(&p, g_at, now), Opened::Message { .. }));
        assert_eq!(h.open(&p, g_at, now), Opened::Dropped, "the same packet again");
        let mut forged = g.seal(h_at, b"state").unwrap().pop().unwrap();
        let last = forged.len() - 1;
        forged[last] ^= 1;
        assert_eq!(h.open(&forged, g_at, now), Opened::Dropped, "a flipped bit");
        // A stranger's key: its packets never open.
        let (mut g2, _h2, h2_at, _) = pair();
        let stranger = g2.seal(h2_at, b"state").unwrap().pop().unwrap();
        assert!(matches!(h.open(&stranger, g_at, now), Opened::Dropped | Opened::UnknownSession { .. }));
        // A plaintext game message is not this layer's.
        assert_eq!(h.open(b"NVCO\x8a\x01junk", g_at, now), Opened::Dropped);
    }

    #[test]
    fn packets_out_of_order_open_once_each() {
        let (mut g, mut h, h_at, g_at) = pair();
        let now = Instant::now();
        let ps: Vec<Vec<u8>> = (0..5).map(|i| g.seal(h_at, &[i]).unwrap().pop().unwrap()).collect();
        for i in [3, 0, 4, 1, 2] {
            assert_eq!(h.open(&ps[i], g_at, now), Opened::Message { bytes: vec![i as u8], from: g_at });
        }
        for p in &ps {
            assert_eq!(h.open(p, g_at, now), Opened::Dropped);
        }
    }

    #[test]
    fn replay_window_edges() {
        let mut r = Replay::default();
        assert!(r.accept(0));
        assert!(r.accept(5000));
        assert!(!r.accept(0), "far behind the window");
        assert!(r.accept(5000 - 2047), "the oldest the window holds");
        assert!(!r.accept(5000 - 2048));
        assert!(!r.accept(5000));
        assert!(r.accept(4999));
        assert!(r.accept(5070), "a shift across a word boundary");
        assert!(!r.accept(4999), "kept after the shift");
        assert!(r.accept(4998));
    }

    #[test]
    fn a_guest_at_a_new_address_keeps_its_session() {
        let (mut g, mut h, h_at, g_at) = pair();
        let now = Instant::now();
        let moved = addr(50001);
        let p = g.seal(h_at, b"from a new port").unwrap().pop().unwrap();
        assert_eq!(h.open(&p, moved, now), Opened::Message { bytes: b"from a new port".to_vec(), from: moved });
        assert!(h.has_session(moved) && !h.has_session(g_at));
        assert_eq!(h.remote_key(moved), Some(g.identity().public()));
        // A copy sent again from the old address cannot move it back.
        assert_eq!(h.open(&p, g_at, now), Opened::Dropped);
        assert!(h.has_session(moved));
    }

    #[test]
    fn lost_handshake_messages_are_sent_again() {
        let now = Instant::now();
        let (h_at, g_at) = (addr(47960), addr(50000));
        let mut g = Secure::new(Identity::generate());
        let mut h = Secure::new(Identity::generate());
        let m1 = g.initiate(h_at, now);
        assert_eq!(g.initiate(h_at, now), m1, "the same message 1 until answered");
        let Opened::Handshake { send: a, .. } = h.open(&m1, g_at, now) else { panic!() };
        let Opened::Handshake { send: b, .. } = h.open(&m1, g_at, now) else { panic!() };
        assert_eq!(a, b, "the same message 2 for the same message 1");
        // Message 3 lost: the guest's first data packet carries it again.
        let Opened::Handshake { made: Some(_), .. } = g.open(&a[0].1, h_at, now) else { panic!() };
        let packets = g.seal(h_at, b"hello").unwrap();
        assert!(matches!(h.open(&packets[0], g_at, now), Opened::Handshake { made: Some(_), .. }));
        assert_eq!(h.open(&packets[1], g_at, now), Opened::Message { bytes: b"hello".to_vec(), from: g_at });
    }

    #[test]
    fn a_message_1_for_another_lobby_is_ignored() {
        let now = Instant::now();
        let mut g = Secure::new(Identity::generate());
        g.set_lobby(Some(7));
        let mut h = Secure::new(Identity::generate());
        h.set_lobby(Some(9));
        let m1 = g.initiate(addr(1), now);
        assert_eq!(h.open(&m1, addr(2), now), Opened::Dropped);
        h.set_lobby(Some(7));
        assert!(matches!(h.open(&m1, addr(2), now), Opened::Handshake { .. }));
    }

    #[test]
    fn a_new_handshake_from_the_same_guest_replaces_its_session() {
        let (mut g, mut h, h_at, g_at) = pair();
        let now = Instant::now();
        let key = g.identity().public();
        g.forget(h_at);
        let m1 = g.initiate(h_at, now);
        let Opened::Handshake { send, .. } = h.open(&m1, g_at, now) else { panic!() };
        let Opened::Handshake { send: s3, .. } = g.open(&send[0].1, h_at, now) else { panic!() };
        h.open(&s3[0].1, g_at, now);
        assert_eq!(h.remote_key(g_at), Some(key), "the same player");
        assert_eq!(h.sessions.len(), 1, "the old session went");
        let p = g.seal(h_at, b"again").unwrap().pop().unwrap();
        assert!(matches!(h.open(&p, g_at, now), Opened::Message { .. }));
    }

    #[test]
    fn stale_handshakes_and_sessions_are_pruned() {
        let (_g, mut h, _, g_at) = pair();
        let later = Instant::now() + IDLE_FOR + Duration::from_secs(1);
        h.prune(later);
        assert!(!h.has_session(g_at));
    }
}
