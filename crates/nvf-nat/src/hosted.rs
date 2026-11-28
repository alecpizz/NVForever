//! A relay a player hosts: `nvf-relay` (#3795 section 9, option 2). It is for lobbies where no
//! direct path and no other player can help (both ends behind symmetric NATs, say), run by a
//! player on a VPS or a home server with one UDP port open, and never by the project. The wire
//! format and its rules are in `docs/spec-notes/nat-traversal.md`, "Hosted relay".
//!
//! The relay knows nothing of Noise or the game: two clients name the same session, each from
//! its side, and whatever one sends the relay hands to the other. The game's Noise session
//! stays end to end, so the relay sees only ciphertext and cannot forge a packet.
//!
//! Sans-IO, as the rest of the crate: [`Server`] is the relay's state machine (the binary is
//! `crates/nvf-relay`), [`Client`] the game's side of one session.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// The first four bytes of every relay datagram.
pub const MAGIC: [u8; 4] = *b"NVRL";
pub const VERSION: u8 = 2;
/// Magic, version, op, session, side.
pub const HEADER: usize = 4 + 1 + 1 + 16 + 1;
/// Length of the keyed-hash return-check cookie carried in [`Op::Challenge`] and echoed in
/// [`Op::Bind`] (#3908) [design].
pub const COOKIE_LEN: usize = 16;
/// The largest payload the relay forwards: a game datagram (at most 1200 bytes) fits.
pub const MAX_PAYLOAD: usize = 1400;

pub type SessionId = [u8; 16];

/// The session two players meet in: the first 16 bytes of SHA-256 over a label, the host's
/// Noise public key and the guest's [design]. Both sides know both keys from the matchmaking
/// service, so nothing new crosses the wire. Anyone who knows both keys could claim a side
/// first; that only blocks the relay for that pair (Noise keeps the content safe).
pub fn session_id(host_key: &[u8; 32], guest_key: &[u8; 32]) -> SessionId {
    let mut h = Sha256::new();
    h.update(b"nvf-relay session v1\0");
    h.update(host_key);
    h.update(guest_key);
    let d = h.finalize();
    let mut s = [0; 16];
    s.copy_from_slice(&d[..16]);
    s
}

/// Which end of a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Host,
    Guest,
}

impl Side {
    fn byte(self) -> u8 {
        match self {
            Side::Host => 0,
            Side::Guest => 1,
        }
    }
    fn from_byte(b: u8) -> Option<Side> {
        match b {
            0 => Some(Side::Host),
            1 => Some(Side::Guest),
            _ => None,
        }
    }
    fn index(self) -> usize {
        self.byte() as usize
    }
    pub fn other(self) -> Side {
        match self {
            Side::Host => Side::Guest,
            Side::Guest => Side::Host,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Client to relay: claim (or keep) this side of the session for the sending address.
    /// Body is empty on an initial request or a keepalive refresh from the active holder, or
    /// the [`COOKIE_LEN`]-byte cookie from [`Op::Challenge`] to prove reachability (#3908).
    Bind,
    /// Relay to client: the claim holds. Body: the refresh interval in seconds (u16, BE).
    Bound,
    /// Relay to client: refused. Body: one [`Refusal`] byte.
    Refused,
    /// Either way: a payload. To the relay, from a proven active holder of the sender's side;
    /// from the relay, the side it came from.
    Data,
    /// Relay to client: return-check challenge for an unproven address. Body: a
    /// [`COOKIE_LEN`]-byte keyed-hash cookie to echo in [`Op::Bind`] (#3908).
    Challenge,
}

impl Op {
    fn byte(self) -> u8 {
        match self {
            Op::Bind => 1,
            Op::Bound => 2,
            Op::Refused => 3,
            Op::Data => 4,
            Op::Challenge => 5,
        }
    }
    fn from_byte(b: u8) -> Option<Op> {
        Some(match b {
            1 => Op::Bind,
            2 => Op::Bound,
            3 => Op::Refused,
            4 => Op::Data,
            5 => Op::Challenge,
            _ => return None,
        })
    }
}

/// Why the relay said no.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// It holds as many sessions as it is set to.
    Full,
    /// Another address holds this side and has been heard from lately.
    Taken,
    /// An unknown reason byte (a newer relay).
    Other(u8),
}

impl Refusal {
    fn byte(self) -> u8 {
        match self {
            Refusal::Full => 1,
            Refusal::Taken => 2,
            Refusal::Other(b) => b,
        }
    }
    fn from_byte(b: u8) -> Refusal {
        match b {
            1 => Refusal::Full,
            2 => Refusal::Taken,
            b => Refusal::Other(b),
        }
    }
}

/// One decoded datagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    pub op: Op,
    pub session: SessionId,
    pub side: Side,
    pub body: &'a [u8],
}

pub fn encode(op: Op, session: &SessionId, side: Side, body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(HEADER + body.len());
    v.extend_from_slice(&MAGIC);
    v.push(VERSION);
    v.push(op.byte());
    v.extend_from_slice(session);
    v.push(side.byte());
    v.extend_from_slice(body);
    v
}

/// A relay datagram, or None for anything else (another protocol, another version, a bad op
/// or side).
pub fn decode(bytes: &[u8]) -> Option<Frame<'_>> {
    if bytes.len() < HEADER || bytes[..4] != MAGIC || bytes[4] != VERSION {
        return None;
    }
    let op = Op::from_byte(bytes[5])?;
    let session: SessionId = bytes[6..22].try_into().ok()?;
    let side = Side::from_byte(bytes[22])?;
    Some(Frame { op, session, side, body: &bytes[HEADER..] })
}

/// The session a relay datagram belongs to, to hand it to the right [`Client`] when one
/// socket talks to the relay for several peers.
pub fn peek_session(bytes: &[u8]) -> Option<SessionId> {
    decode(bytes).map(|f| f.session)
}

/// The relay's limits [design].
#[derive(Clone, Copy, Debug)]
pub struct ServerConfig {
    /// Sessions held at once (256).
    pub max_sessions: usize,
    /// A side not heard from for this long is let go, and another address may claim it
    /// (30 s, a few missed refreshes).
    pub idle: Duration,
    /// What a client is told to refresh at (10 s).
    pub refresh: Duration,
    /// Bytes a second each side may send, payloads counted (256 KiB/s: well above a game's
    /// 15 KB/s, section 1, and the join snapshot's 110 KB/s peak).
    pub rate: u32,
    /// How often the relay rotates the secret used to key return-check cookies (60 s; the
    /// previous secret stays valid across one rotation boundary so in-flight cookies still
    /// succeed, #3908).
    pub cookie_rotate: Duration,
    /// Random source for 256-bit cookie secrets. A real relay (`nvf-relay`) passes the
    /// operating system's random source; `None` derives deterministic secrets via SHA-256
    /// chaining for sans-IO unit tests only.
    pub random_secret: Option<fn() -> [u8; 32]>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            max_sessions: 256,
            idle: Duration::from_secs(30),
            refresh: Duration::from_secs(10),
            rate: 256 * 1024,
            cookie_rotate: Duration::from_secs(60),
            random_secret: None,
        }
    }
}

/// What the relay has done, for its status line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub forwarded: u64,
    pub forwarded_bytes: u64,
    /// Not ours, from no holder, to a side nobody holds, or over the rate.
    pub dropped: u64,
    pub refused: u64,
}

struct Holder {
    addr: SocketAddr,
    last: Instant,
    tokens: f64,
    filled: Instant,
}

#[derive(Default)]
struct Session {
    sides: [Option<Holder>; 2],
}

/// Constant-time byte comparison for return-check cookies.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Keyed-hash (HMAC-SHA256, RFC 2104, truncated to [`COOKIE_LEN`] bytes) over the sender's
/// address, session id and side [design: #3908].
fn cookie_with(secret: &[u8; 32], addr: SocketAddr, session: &SessionId, side: Side) -> [u8; COOKIE_LEN] {
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..32 {
        ipad[i] ^= secret[i];
        opad[i] ^= secret[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(b"nvf-relay cookie v2\0");
    match addr {
        SocketAddr::V4(v4) => {
            inner.update([4u8]);
            inner.update(v4.ip().octets());
            inner.update(v4.port().to_be_bytes());
        }
        SocketAddr::V6(v6) => {
            inner.update([6u8]);
            inner.update(v6.ip().octets());
            inner.update(v6.port().to_be_bytes());
        }
    }
    inner.update(session);
    inner.update([side.byte()]);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    let digest = outer.finalize();
    let mut out = [0u8; COOKIE_LEN];
    out.copy_from_slice(&digest[..COOKIE_LEN]);
    out
}

/// The relay. Feed it every datagram; send what it returns.
pub struct Server {
    cfg: ServerConfig,
    sessions: HashMap<SessionId, Session>,
    stats: Stats,
    secret: [u8; 32],
    prev_secret: Option<[u8; 32]>,
    secret_at: Option<Instant>,
    secret_seq: u64,
}

impl Server {
    pub fn new(cfg: ServerConfig) -> Self {
        let mut s = Server {
            cfg,
            sessions: HashMap::new(),
            stats: Stats::default(),
            secret: [0u8; 32],
            prev_secret: None,
            secret_at: None,
            secret_seq: 0,
        };
        s.secret = s.next_secret();
        s
    }

    fn next_secret(&mut self) -> [u8; 32] {
        if let Some(f) = self.cfg.random_secret {
            return f();
        }
        let seq = self.secret_seq;
        self.secret_seq = self.secret_seq.wrapping_add(1);
        let mut h = Sha256::new();
        h.update(b"nvf-relay secret v2\0");
        h.update(seq.to_be_bytes());
        h.finalize().into()
    }

    fn maybe_rotate(&mut self, now: Instant) {
        if self.cfg.cookie_rotate.is_zero() {
            return;
        }
        let Some(at) = self.secret_at else {
            self.secret_at = Some(now);
            return;
        };
        let elapsed = now.saturating_duration_since(at);
        if elapsed >= self.cfg.cookie_rotate.saturating_mul(2) {
            self.prev_secret = None;
            self.secret = self.next_secret();
            self.secret_at = Some(now);
        } else if elapsed >= self.cfg.cookie_rotate {
            self.prev_secret = Some(self.secret);
            self.secret = self.next_secret();
            self.secret_at = Some(now);
        }
    }

    fn verify_cookie(&self, body: &[u8], from: SocketAddr, session: &SessionId, side: Side) -> bool {
        if body.len() != COOKIE_LEN {
            return false;
        }
        if ct_eq(body, &cookie_with(&self.secret, from, session, side)) {
            return true;
        }
        self.prev_secret.is_some_and(|p| ct_eq(body, &cookie_with(&p, from, session, side)))
    }

    pub fn sessions(&self) -> usize {
        self.sessions.len()
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Lets go of sides not heard from within `idle`, and sessions with no side left. Call
    /// now and then (each second is plenty); claims also check.
    pub fn expire(&mut self, now: Instant) {
        self.maybe_rotate(now);
        let idle = self.cfg.idle;
        self.sessions.retain(|_, s| {
            for h in s.sides.iter_mut() {
                if h.as_ref().is_some_and(|h| now.saturating_duration_since(h.last) >= idle) {
                    *h = None;
                }
            }
            s.sides.iter().any(Option::is_some)
        });
    }

    /// Claims `side` of `session` for `from`, or keeps the claim fresh. Callers must only
    /// invoke this after `from` has proven reachability via a return-check cookie (#3908).
    fn claim(&mut self, session: SessionId, side: Side, from: SocketAddr, now: Instant) -> Result<(), Refusal> {
        if !self.sessions.contains_key(&session) {
            if self.sessions.len() >= self.cfg.max_sessions {
                self.expire(now);
                if self.sessions.len() >= self.cfg.max_sessions {
                    return Err(Refusal::Full);
                }
            }
            self.sessions.insert(session, Session::default());
        }
        let rate = self.cfg.rate as f64;
        let idle = self.cfg.idle;
        let slot = &mut self.sessions.get_mut(&session).expect("inserted above").sides[side.index()];
        match slot {
            Some(h) if h.addr == from => h.last = now,
            Some(h) if now.saturating_duration_since(h.last) < idle => return Err(Refusal::Taken),
            _ => *slot = Some(Holder { addr: from, last: now, tokens: rate, filled: now }),
        }
        Ok(())
    }

    /// One datagram from `from`. Returns what to send, and where.
    pub fn on_datagram(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) -> Vec<(SocketAddr, Vec<u8>)> {
        let Some(f) = decode(bytes) else {
            self.stats.dropped += 1;
            return Vec::new();
        };
        let refuse = |stats: &mut Stats, r: Refusal| {
            stats.refused += 1;
            vec![(from, encode(Op::Refused, &f.session, f.side, &[r.byte()]))]
        };
        let refresh_secs = self.cfg.refresh.as_secs().min(u16::MAX as u64) as u16;
        let bound = || vec![(from, encode(Op::Bound, &f.session, f.side, &refresh_secs.to_be_bytes()))];
        match f.op {
            Op::Bind => {
                self.maybe_rotate(now);
                let idle = self.cfg.idle;
                // An already-proven active holder refreshes in place with no extra round trip.
                if let Some(h) = self
                    .sessions
                    .get_mut(&f.session)
                    .and_then(|s| s.sides[f.side.index()].as_mut())
                    .filter(|h| h.addr == from && now.saturating_duration_since(h.last) < idle)
                {
                    h.last = now;
                    return bound();
                }
                // Any unproven address (new claim, moved address after silence, or expired
                // holder) must echo a valid cookie before touching session state (#3908).
                if !self.verify_cookie(f.body, from, &f.session, f.side) {
                    let cookie = cookie_with(&self.secret, from, &f.session, f.side);
                    return vec![(from, encode(Op::Challenge, &f.session, f.side, &cookie))];
                }
                match self.claim(f.session, f.side, from, now) {
                    Ok(()) => bound(),
                    Err(r) => refuse(&mut self.stats, r),
                }
            }
            Op::Data => {
                if f.body.len() > MAX_PAYLOAD {
                    self.stats.dropped += 1;
                    return Vec::new();
                }
                let idle = self.cfg.idle;
                let rate = self.cfg.rate as f64;
                // Data from an unproven or expired address is dropped; it never claims a side.
                let Some(s) = self.sessions.get_mut(&f.session) else {
                    self.stats.dropped += 1;
                    return Vec::new();
                };
                let Some(me) = s.sides[f.side.index()]
                    .as_mut()
                    .filter(|h| h.addr == from && now.saturating_duration_since(h.last) < idle)
                else {
                    self.stats.dropped += 1;
                    return Vec::new();
                };
                me.last = now;
                me.tokens = (me.tokens + now.saturating_duration_since(me.filled).as_secs_f64() * rate).min(rate);
                me.filled = now;
                let cost = f.body.len() as f64;
                if me.tokens < cost {
                    self.stats.dropped += 1;
                    return Vec::new();
                }
                me.tokens -= cost;
                let Some(to) = s.sides[f.side.other().index()]
                    .as_ref()
                    .filter(|h| now.saturating_duration_since(h.last) < idle)
                    .map(|h| h.addr)
                else {
                    self.stats.dropped += 1;
                    return Vec::new();
                };
                self.stats.forwarded += 1;
                self.stats.forwarded_bytes += f.body.len() as u64;
                vec![(to, encode(Op::Data, &f.session, f.side, f.body))]
            }
            // Only the relay sends these.
            Op::Bound | Op::Refused | Op::Challenge => {
                self.stats.dropped += 1;
                Vec::new()
            }
        }
    }
}

/// What a datagram from the relay was, to a [`Client`].
#[derive(Debug, PartialEq, Eq)]
pub enum Received<'a> {
    /// The other side's payload: hand it on as if it came from the relay's address.
    Data(&'a [u8]),
    /// A Challenge, Bound or Refused, taken in.
    Control,
    /// Not this session's, or not a relay datagram.
    NotMine,
}

/// Before the first Bound, a Bind is resent this often.
const BIND_RETRY: Duration = Duration::from_secs(1);

/// The game's side of one session on a hosted relay.
pub struct Client {
    relay: SocketAddr,
    session: SessionId,
    side: Side,
    refresh: Duration,
    last_bind: Option<Instant>,
    cookie: Option<[u8; COOKIE_LEN]>,
    bound: bool,
    refused: Option<Refusal>,
}

impl Client {
    pub fn new(relay: SocketAddr, session: SessionId, side: Side) -> Self {
        Client {
            relay,
            session,
            side,
            refresh: Duration::from_secs(10),
            last_bind: None,
            cookie: None,
            bound: false,
            refused: None,
        }
    }

    pub fn relay(&self) -> SocketAddr {
        self.relay
    }

    pub fn session(&self) -> SessionId {
        self.session
    }

    /// The relay confirmed this side is ours.
    pub fn bound(&self) -> bool {
        self.bound
    }

    /// The relay's last refusal, cleared by the next Bound.
    pub fn refused(&self) -> Option<Refusal> {
        self.refused
    }

    /// A Bind to send to the relay, when one is due: at once, immediately upon receiving a
    /// fresh [`Op::Challenge`] cookie, each second while unbound, then at the refresh
    /// interval the relay gave.
    pub fn poll(&mut self, now: Instant) -> Option<Vec<u8>> {
        let every = if self.bound { self.refresh } else { BIND_RETRY };
        if self.last_bind.is_some_and(|t| now.saturating_duration_since(t) < every) {
            return None;
        }
        self.last_bind = Some(now);
        let body: &[u8] = self.cookie.as_ref().map_or(&[], |c| c.as_slice());
        Some(encode(Op::Bind, &self.session, self.side, body))
    }

    /// A payload for the other side, as the datagram to send to the relay.
    pub fn wrap(&self, payload: &[u8]) -> Vec<u8> {
        encode(Op::Data, &self.session, self.side, payload)
    }

    /// A datagram that came from the relay's address.
    pub fn on_datagram<'a>(&mut self, bytes: &'a [u8]) -> Received<'a> {
        let Some(f) = decode(bytes) else { return Received::NotMine };
        if f.session != self.session {
            return Received::NotMine;
        }
        match f.op {
            Op::Data if f.side == self.side.other() => Received::Data(f.body),
            Op::Challenge if f.side == self.side => {
                let Ok(cookie) = <[u8; COOKIE_LEN]>::try_from(f.body) else {
                    return Received::NotMine;
                };
                self.bound = false;
                if self.cookie != Some(cookie) {
                    self.cookie = Some(cookie);
                    // Reply at once on the next `poll` so binding costs only one extra RTT.
                    self.last_bind = None;
                }
                Received::Control
            }
            Op::Bound if f.side == self.side => {
                self.bound = true;
                self.refused = None;
                self.cookie = None;
                if let Some(b) = f.body.get(..2) {
                    let secs = u16::from_be_bytes([b[0], b[1]]).max(1);
                    self.refresh = Duration::from_secs(secs as u64);
                }
                Received::Control
            }
            Op::Refused if f.side == self.side => {
                self.bound = false;
                self.cookie = None;
                self.refused = Some(Refusal::from_byte(f.body.first().copied().unwrap_or(0)));
                Received::Control
            }
            _ => Received::NotMine,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sa(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    const S: SessionId = [7; 16];

    /// Complete the two-step return-check bind for `from` at `t`, returning the final reply.
    fn bind_with_check(s: &mut Server, from: SocketAddr, session: &SessionId, side: Side, t: Instant) -> Vec<(SocketAddr, Vec<u8>)> {
        let ch = s.on_datagram(from, &encode(Op::Bind, session, side, &[]), t);
        assert_eq!(ch.len(), 1);
        assert_eq!(ch[0].0, from);
        let f = decode(&ch[0].1).unwrap();
        assert_eq!(f.op, Op::Challenge);
        assert_eq!(f.body.len(), COOKIE_LEN);
        s.on_datagram(from, &encode(Op::Bind, session, side, f.body), t)
    }

    #[test]
    fn frames_round_trip_and_reject_junk() {
        let b = encode(Op::Data, &S, Side::Guest, b"hi");
        assert_eq!(b.len(), HEADER + 2);
        assert_eq!(decode(&b), Some(Frame { op: Op::Data, session: S, side: Side::Guest, body: b"hi" }));
        assert_eq!(peek_session(&b), Some(S));
        let ch = encode(Op::Challenge, &S, Side::Host, &[0xAB; COOKIE_LEN]);
        assert_eq!(decode(&ch), Some(Frame { op: Op::Challenge, session: S, side: Side::Host, body: &[0xAB; COOKIE_LEN] }));
        let mut bad = b.clone();
        bad[4] = 1;
        assert_eq!(decode(&bad), None, "older wire version rejected");
        let mut bad = b.clone();
        bad[5] = 9;
        assert_eq!(decode(&bad), None, "unknown op");
        let mut bad = b.clone();
        bad[22] = 2;
        assert_eq!(decode(&bad), None, "unknown side");
        assert_eq!(decode(&b[..HEADER - 1]), None);
        assert_eq!(decode(b"NVCO\x8a............................"), None);
    }

    #[test]
    fn session_ids_differ_by_key_and_order() {
        let (a, b) = ([1; 32], [2; 32]);
        assert_eq!(session_id(&a, &b), session_id(&a, &b));
        assert_ne!(session_id(&a, &b), session_id(&b, &a));
        assert_ne!(session_id(&a, &b), session_id(&a, &[3; 32]));
    }

    #[test]
    fn forwards_between_the_two_sides() {
        let t = Instant::now();
        let mut s = Server::new(ServerConfig::default());
        let (h, g) = (sa("203.0.113.1:40000"), sa("203.0.113.2:40000"));
        let out = bind_with_check(&mut s, h, &S, Side::Host, t);
        assert_eq!(out, vec![(h, encode(Op::Bound, &S, Side::Host, &10u16.to_be_bytes()))]);
        // Nobody holds the guest side yet: dropped.
        assert!(s.on_datagram(h, &encode(Op::Data, &S, Side::Host, b"x"), t).is_empty());
        // Data from an unproven guest is dropped and does not claim the side (#3908).
        assert!(s.on_datagram(g, &encode(Op::Data, &S, Side::Guest, b"unproven"), t).is_empty());
        // Once the guest completes the return check, Data flows both ways.
        let out = bind_with_check(&mut s, g, &S, Side::Guest, t);
        assert_eq!(out, vec![(g, encode(Op::Bound, &S, Side::Guest, &10u16.to_be_bytes()))]);
        let out = s.on_datagram(g, &encode(Op::Data, &S, Side::Guest, b"hello"), t);
        assert_eq!(out, vec![(h, encode(Op::Data, &S, Side::Guest, b"hello"))]);
        let out = s.on_datagram(h, &encode(Op::Data, &S, Side::Host, b"back"), t);
        assert_eq!(out, vec![(g, encode(Op::Data, &S, Side::Host, b"back"))]);
        assert_eq!(s.stats().forwarded, 2);
        assert_eq!(s.stats().dropped, 2);
        // Too big, or a relay-only op from a client: dropped.
        assert!(s.on_datagram(g, &encode(Op::Data, &S, Side::Guest, &[0; MAX_PAYLOAD + 1]), t).is_empty());
        assert!(s.on_datagram(g, &encode(Op::Bound, &S, Side::Guest, &[]), t).is_empty());
        assert!(s.on_datagram(g, &encode(Op::Challenge, &S, Side::Guest, &[0; COOKIE_LEN]), t).is_empty());
        assert!(s.on_datagram(g, b"not a relay frame at all..", t).is_empty());
    }

    #[test]
    fn spoofed_claim_never_forwards_data() {
        let t = Instant::now();
        let mut s = Server::new(ServerConfig::default());
        let (victim, attacker) = (sa("198.51.100.77:5555"), sa("203.0.113.9:40000"));
        // Spoofed Bind claiming the victim's address gets only a Challenge and creates no session.
        let ch = s.on_datagram(victim, &encode(Op::Bind, &S, Side::Guest, &[]), t);
        assert_eq!(ch.len(), 1);
        assert_eq!(ch[0].0, victim);
        assert_eq!(decode(&ch[0].1).unwrap().op, Op::Challenge);
        assert_eq!(s.sessions(), 0);
        // Spoofed Data claiming the victim's address is dropped with no reply and no session.
        assert!(s.on_datagram(victim, &encode(Op::Data, &S, Side::Guest, b"spoof"), t).is_empty());
        assert_eq!(s.sessions(), 0);
        // Even after the attacker proves the Host side, Data never reflects to the victim.
        let out = bind_with_check(&mut s, attacker, &S, Side::Host, t);
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Bound);
        assert!(s.on_datagram(attacker, &encode(Op::Data, &S, Side::Host, b"flood"), t).is_empty());
        assert_eq!(s.stats().forwarded, 0);
    }

    #[test]
    fn forged_mismatched_and_stale_cookies_are_rejected() {
        let t = Instant::now();
        let cfg = ServerConfig { cookie_rotate: Duration::from_secs(60), ..ServerConfig::default() };
        let mut s = Server::new(cfg);
        let (a1, a2) = (sa("203.0.113.1:40000"), sa("[2001:db8::2]:40000"));

        // Obtain genuine cookies at t before any rotation.
        let ch = s.on_datagram(a1, &encode(Op::Bind, &S, Side::Host, &[]), t);
        let cookie = decode(&ch[0].1).unwrap().body.to_vec();
        assert_eq!(cookie.len(), COOKIE_LEN);
        let ch2 = s.on_datagram(a2, &encode(Op::Bind, &S, Side::Guest, &[]), t);
        let stale_cookie = decode(&ch2[0].1).unwrap().body.to_vec();

        // Forged (flipped bit) and truncated cookies fail and get a fresh Challenge.
        let mut forged = cookie.clone();
        forged[0] ^= 0x01;
        let out = s.on_datagram(a1, &encode(Op::Bind, &S, Side::Host, &forged), t);
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Challenge);
        let out = s.on_datagram(a1, &encode(Op::Bind, &S, Side::Host, &cookie[..COOKIE_LEN - 1]), t);
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Challenge);

        // A cookie bound to (a1, S, Host) is rejected for another address, side, or session.
        let out = s.on_datagram(a2, &encode(Op::Bind, &S, Side::Host, &cookie), t);
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Challenge);
        let out = s.on_datagram(a1, &encode(Op::Bind, &S, Side::Guest, &cookie), t);
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Challenge);
        let out = s.on_datagram(a1, &encode(Op::Bind, &[9; 16], Side::Host, &cookie), t);
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Challenge);
        assert_eq!(s.sessions(), 0);

        // Across one rotation boundary, the previous secret keeps in-flight cookies valid.
        let t1 = t + cfg.cookie_rotate;
        let out = s.on_datagram(a1, &encode(Op::Bind, &S, Side::Host, &cookie), t1);
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Bound);

        // After two rotation windows, a cookie from t is stale and rejected with a fresh Challenge.
        let t2 = t + cfg.cookie_rotate * 2;
        let out = s.on_datagram(a2, &encode(Op::Bind, &S, Side::Guest, &stale_cookie), t2);
        let f = decode(&out[0].1).unwrap();
        assert_eq!(f.op, Op::Challenge);
        assert_ne!(f.body, stale_cookie.as_slice());
        // Echoing the fresh cookie succeeds.
        let out = s.on_datagram(a2, &encode(Op::Bind, &S, Side::Guest, f.body), t2);
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Bound);

        // Even with no traffic during the intermediate window, jumping >= 2 rotations invalidates it.
        let mut s2 = Server::new(cfg);
        let ch3 = s2.on_datagram(a1, &encode(Op::Bind, &S, Side::Host, &[]), t);
        let old = decode(&ch3[0].1).unwrap().body.to_vec();
        let out = s2.on_datagram(a1, &encode(Op::Bind, &S, Side::Host, &old), t + cfg.cookie_rotate * 2);
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Challenge);
    }

    #[test]
    fn a_side_moves_only_after_silence_and_rechecks_return() {
        let t = Instant::now();
        let cfg = ServerConfig::default();
        let mut s = Server::new(cfg);
        let (h, g1, g2) = (sa("203.0.113.1:40000"), sa("203.0.113.2:40000"), sa("203.0.113.2:40001"));
        assert_eq!(decode(&bind_with_check(&mut s, h, &S, Side::Host, t)[0].1).unwrap().op, Op::Bound);
        assert_eq!(decode(&bind_with_check(&mut s, g1, &S, Side::Guest, t)[0].1).unwrap().op, Op::Bound);
        // While g1 is active, g2 must still pass the cookie check and then gets Refused(Taken).
        let out = bind_with_check(&mut s, g2, &S, Side::Guest, t + Duration::from_secs(5));
        assert_eq!(out, vec![(g2, encode(Op::Refused, &S, Side::Guest, &[2]))]);
        // The active holder refreshes without a second challenge.
        let out = s.on_datagram(g1, &encode(Op::Bind, &S, Side::Guest, &[]), t + Duration::from_secs(20));
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Bound);
        let out = bind_with_check(&mut s, g2, &S, Side::Guest, t + Duration::from_secs(40));
        assert_eq!(out[0].1[HEADER], 2);
        // Silent past `idle`: g2 cannot claim by Data or unchallenged Bind.
        let after_idle = t + Duration::from_secs(20) + cfg.idle;
        // Keep the host side fresh so we can check forwarding.
        let _ = s.on_datagram(h, &encode(Op::Bind, &S, Side::Host, &[]), t + Duration::from_secs(25));
        let _ = s.on_datagram(h, &encode(Op::Bind, &S, Side::Host, &[]), after_idle);
        assert!(s.on_datagram(g2, &encode(Op::Data, &S, Side::Guest, b"early"), after_idle).is_empty());
        let ch = s.on_datagram(g2, &encode(Op::Bind, &S, Side::Guest, &[]), after_idle);
        assert_eq!(decode(&ch[0].1).unwrap().op, Op::Challenge);
        // Host Data still does not forward to g2 before g2 echoes the cookie.
        assert!(s.on_datagram(h, &encode(Op::Data, &S, Side::Host, b"ping"), after_idle).is_empty());
        // Echoing the cookie claims the side and resumes forwarding to g2.
        let cookie = decode(&ch[0].1).unwrap().body.to_vec();
        let out = s.on_datagram(g2, &encode(Op::Bind, &S, Side::Guest, &cookie), after_idle);
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Bound);
        let out = s.on_datagram(h, &encode(Op::Data, &S, Side::Host, b"pong"), after_idle);
        assert_eq!(out, vec![(g2, encode(Op::Data, &S, Side::Host, b"pong"))]);
        assert_eq!(s.stats().refused, 2);
    }

    #[test]
    fn expires_and_caps_sessions() {
        let t = Instant::now();
        let mut s = Server::new(ServerConfig { max_sessions: 2, ..ServerConfig::default() });
        let a = sa("203.0.113.1:1");
        for i in 0..2u8 {
            let out = bind_with_check(&mut s, a, &[i; 16], Side::Host, t);
            assert_eq!(decode(&out[0].1).unwrap().op, Op::Bound);
        }
        let out = bind_with_check(&mut s, a, &[9; 16], Side::Host, t + Duration::from_secs(1));
        assert_eq!(out[0].1[HEADER], 1, "full");
        // The first two go quiet; a claim past `idle` clears them and gets in.
        let out = bind_with_check(&mut s, a, &[9; 16], Side::Host, t + Duration::from_secs(31));
        assert_eq!(decode(&out[0].1).unwrap().op, Op::Bound);
        assert_eq!(s.sessions(), 1);
        s.expire(t + Duration::from_secs(62));
        assert_eq!(s.sessions(), 0);
    }

    #[test]
    fn caps_each_sides_rate() {
        let t = Instant::now();
        let mut s = Server::new(ServerConfig { rate: 3000, ..ServerConfig::default() });
        let (h, g) = (sa("203.0.113.1:1"), sa("203.0.113.2:1"));
        assert_eq!(decode(&bind_with_check(&mut s, h, &S, Side::Host, t)[0].1).unwrap().op, Op::Bound);
        assert_eq!(decode(&bind_with_check(&mut s, g, &S, Side::Guest, t)[0].1).unwrap().op, Op::Bound);
        let sent = (0..5).filter(|_| !s.on_datagram(g, &encode(Op::Data, &S, Side::Guest, &[0; 1000]), t).is_empty()).count();
        assert_eq!(sent, 3, "the bucket starts full: 3000 bytes");
        // Half a second refills 1500 bytes: one more.
        let later = t + Duration::from_millis(500);
        let sent = (0..3).filter(|_| !s.on_datagram(g, &encode(Op::Data, &S, Side::Guest, &[0; 1000]), later).is_empty()).count();
        assert_eq!(sent, 1);
        // The host's own bucket is untouched.
        assert!(!s.on_datagram(h, &encode(Op::Data, &S, Side::Host, &[0; 1000]), later).is_empty());
    }

    #[test]
    fn client_binds_refreshes_and_unwraps() {
        let t = Instant::now();
        let relay = sa("198.51.100.9:47961");
        let mut c = Client::new(relay, S, Side::Guest);
        assert_eq!(c.poll(t), Some(encode(Op::Bind, &S, Side::Guest, &[])));
        assert_eq!(c.poll(t + Duration::from_millis(500)), None);
        assert!(c.poll(t + Duration::from_secs(1)).is_some(), "retried while unbound");
        // A Challenge schedules an immediate cookie-bearing Bind without waiting BIND_RETRY.
        let cookie = [0x42u8; COOKIE_LEN];
        assert_eq!(c.on_datagram(&encode(Op::Challenge, &S, Side::Guest, &cookie)), Received::Control);
        assert!(!c.bound());
        assert_eq!(c.poll(t + Duration::from_secs(1)), Some(encode(Op::Bind, &S, Side::Guest, &cookie)));
        // Duplicate Challenge with the same cookie does not spam immediate retries.
        assert_eq!(c.on_datagram(&encode(Op::Challenge, &S, Side::Guest, &cookie)), Received::Control);
        assert_eq!(c.poll(t + Duration::from_millis(1200)), None);
        // Malformed Challenge body is ignored.
        assert_eq!(c.on_datagram(&encode(Op::Challenge, &S, Side::Guest, &[1, 2, 3])), Received::NotMine);
        assert_eq!(c.on_datagram(&encode(Op::Bound, &S, Side::Guest, &5u16.to_be_bytes())), Received::Control);
        assert!(c.bound());
        assert_eq!(c.poll(t + Duration::from_secs(5)), None);
        assert_eq!(c.poll(t + Duration::from_secs(6)), Some(encode(Op::Bind, &S, Side::Guest, &[])), "the relay's 5 s refresh");
        assert_eq!(c.on_datagram(&encode(Op::Data, &S, Side::Host, b"hi")), Received::Data(b"hi"));
        assert_eq!(c.on_datagram(&encode(Op::Data, &S, Side::Guest, b"echo")), Received::NotMine);
        assert_eq!(c.on_datagram(&encode(Op::Data, &[1; 16], Side::Host, b"hi")), Received::NotMine);
        assert_eq!(c.on_datagram(&encode(Op::Refused, &S, Side::Guest, &[2])), Received::Control);
        assert_eq!((c.bound(), c.refused()), (false, Some(Refusal::Taken)));
        assert_eq!(decode(&c.wrap(b"p")).unwrap(), Frame { op: Op::Data, session: S, side: Side::Guest, body: b"p" });
    }
}
