//! The matchmaking service in-process: the same wire format as the Worker
//! (`docs/design/coop-signal-wire.md`), for tests, so CI needs no Node (#3795 section 12), and
//! as the reference a self-hoster can wrap in any HTTP and WebSocket server.
//!
//! Sans-IO like the rest: the caller maps HTTP calls to [`StandIn::create`] and
//! [`StandIn::info`], each WebSocket to a [`ConnId`] from [`StandIn::open`], frames to
//! [`StandIn::message`], a dropped socket to [`StandIn::close`], and calls [`StandIn::tick`]
//! now and then. Every call returns the frames to send ([`Out`]).
//!
//! Rate limits (the Worker's 10 code lookups a minute per IP) are left to whatever serves it
//! [design].

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use crate::code::Code;
use crate::wire::{
    is_key, key_fingerprint, wire_supported, BrowseAnswer, BrowseQuery, BrowseRow, CreateLobby, ErrorBody, FromService,
    Listing, LobbyCreated, LobbyInfo, PeerNo, Report, ToService,
};

pub type ConnId = u64;

/// A frame to send on a socket; `close` means close that socket after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Out {
    pub conn: ConnId,
    pub text: String,
    pub close: bool,
}

#[derive(Clone, Debug)]
pub struct Config {
    /// The oldest game protocol let in (403 below it).
    pub min_protocol: u32,
    /// STUN servers handed to hosts.
    pub stun: Vec<String>,
    /// How long a lobby outlives its host's socket, and how long an unclaimed lobby lives
    /// (2 minutes, #3795 section 5).
    pub grace: Duration,
    /// Seeds the code and secret generator. For tests any number; a real deployment must seed
    /// it from the operating system's random source, since the secrets guard lobbies.
    pub seed: u64,
    /// Where codes and secrets come from instead, when set: a real deployment's operating
    /// system random source (`nvf-signal-server`). The seeded generator is splitmix64, whose
    /// outputs give its state away, so a host that saw its own code and secret could work out
    /// the next lobby's secret: fine for tests, not for a service.
    pub random: Option<fn() -> u64>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            min_protocol: 0,
            stun: vec!["stun.cloudflare.com:3478".into(), "stun.l.google.com:19302".into()],
            grace: Duration::from_secs(120),
            seed: 0x6e76_666f_7265_7672,
            random: None,
        }
    }
}

struct Guest {
    conn: ConnId,
    accepted: bool,
    /// Its last `connected` path was direct or LAN: it may relay for others.
    helper: bool,
}

struct Lobby {
    secret: String,
    req: CreateLobby,
    host: Option<ConnId>,
    host_gone: Option<Instant>,
    guests: BTreeMap<PeerNo, Guest>,
    next_peer: PeerNo,
    /// The browser listing (wire 2), texts already cut and cleaned.
    listing: Listing,
    /// The password's hash; with `listing.locked` and none yet, every join is declined.
    password: Option<String>,
    created: Instant,
    /// Who reported it (an IP on the service; whatever the caller passes here).
    reports: std::collections::BTreeSet<String>,
}

impl Lobby {
    fn locked(&self) -> bool {
        self.listing.locked || self.password.is_some()
    }
    fn players(&self) -> u8 {
        1 + self.guests.values().filter(|g| g.accepted).count() as u8
    }
}

/// Page size of `GET /v1/browse`.
pub const BROWSE_PAGE: usize = 100;
/// Reports from this many reporters hide a lobby from browse (owner decision 2026-11-25).
pub const REPORTS_TO_HIDE: usize = 5;

/// A listing text cut to `max` characters, control characters dropped, and lowered when it is
/// all capitals (`docs/design/coop-lobby-browser.md` section 6). The word filter is the
/// Worker's; the stand-in has none.
pub fn clean_text(s: &str, max: usize) -> String {
    let t: String = s.chars().filter(|c| !c.is_control()).take(max).collect();
    let t = t.trim().to_string();
    let letters = t.chars().filter(|c| c.is_alphabetic()).count();
    if letters >= 2 && t.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) {
        t.to_lowercase()
    } else {
        t
    }
}

fn clean_listing(l: &Listing) -> Listing {
    Listing { title: clean_text(&l.title, 40), place: clean_text(&l.place, 32), ..l.clone() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Unbound,
    Host,
    Guest(PeerNo),
}

struct Conn {
    code: String,
    role: Role,
}

pub struct StandIn {
    cfg: Config,
    rng: u64,
    lobbies: HashMap<String, Lobby>,
    conns: HashMap<ConnId, Conn>,
    next_conn: ConnId,
}

fn json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("wire types serialise")
}

fn error_body(error: &str, message: &str) -> String {
    json(&ErrorBody { error: error.into(), message: message.into() })
}

impl StandIn {
    pub fn new(cfg: Config) -> Self {
        let rng = cfg.seed;
        StandIn { cfg, rng, lobbies: HashMap::new(), conns: HashMap::new(), next_conn: 1 }
    }

    /// splitmix64.
    fn random(&mut self) -> u64 {
        if let Some(f) = self.cfg.random {
            return f();
        }
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// `POST /v1/lobbies`: (status, body). `unix_now` fills `expires`.
    pub fn create(&mut self, body: &str, now: Instant, unix_now: u64) -> (u16, String) {
        let Ok(req) = serde_json::from_str::<CreateLobby>(body) else {
            return (400, error_body("bad_request", "not a lobby request"));
        };
        if !wire_supported(req.wire) {
            return (400, error_body("wire_version", "this service speaks wire versions 1 and 2"));
        }
        if req.protocol < self.cfg.min_protocol {
            return (403, error_body("protocol", "this game version can no longer host; update NVForever"));
        }
        if !is_key(&req.host_key) || !(2..=6).contains(&req.max_players) {
            return (400, error_body("bad_request", "bad host key or player count"));
        }
        let code = loop {
            let c = Code::from_random(self.random() as u32).compact();
            if !self.lobbies.contains_key(&c) {
                break c;
            }
        };
        let secret = format!("{:016x}{:016x}", self.random(), self.random());
        let created = LobbyCreated {
            code: code.clone(),
            host_secret: secret.clone(),
            stun: self.cfg.stun.clone(),
            expires: unix_now + self.cfg.grace.as_secs(),
        };
        let listing = req.listing.as_ref().map(clean_listing).unwrap_or_default();
        self.lobbies.insert(
            code,
            Lobby {
                secret,
                req,
                host: None,
                host_gone: Some(now),
                guests: BTreeMap::new(),
                next_peer: 1,
                listing,
                password: None,
                created: now,
                reports: Default::default(),
            },
        );
        (200, json(&created))
    }

    /// `GET /v1/lobbies/{code}`: (status, body). The code in any form a player may type.
    pub fn info(&self, code: &str) -> (u16, String) {
        let Some(l) = Code::parse(code).and_then(|c| self.lobbies.get(&c.compact())) else {
            return (404, error_body("not_found", "no lobby with that code"));
        };
        let players = 1 + l.guests.len() as u8;
        let info = LobbyInfo {
            protocol: l.req.protocol,
            build: l.req.build.clone(),
            load_order_hash: l.req.load_order_hash.clone(),
            players,
            max_players: l.req.max_players,
            full: players >= l.req.max_players,
            locked: l.locked(),
        };
        (200, json(&info))
    }

    /// `GET /v1/lobbies/{code}/ws`: a new socket, or (status, body) when there is no lobby.
    pub fn open(&mut self, code: &str) -> Result<ConnId, (u16, String)> {
        let Some(c) = Code::parse(code).map(|c| c.compact()).filter(|c| self.lobbies.contains_key(c)) else {
            return Err((404, error_body("not_found", "no lobby with that code")));
        };
        let id = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(id, Conn { code: c, role: Role::Unbound });
        Ok(id)
    }

    fn send(out: &mut Vec<Out>, conn: ConnId, msg: &FromService) {
        out.push(Out { conn, text: json(msg), close: false });
    }

    fn fail(&mut self, out: &mut Vec<Out>, conn: ConnId, error: &str, message: &str, close: bool) {
        out.push(Out { conn, text: json(&FromService::Error { error: error.into(), message: message.into() }), close });
        if close {
            self.conns.remove(&conn);
        }
    }

    /// A text frame from socket `conn`.
    pub fn message(&mut self, conn: ConnId, text: &str, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        let Some(c) = self.conns.get(&conn) else { return out };
        let (code, role) = (c.code.clone(), c.role);
        let Ok(msg) = serde_json::from_str::<ToService>(text) else {
            self.fail(&mut out, conn, "bad_message", "not a message of wire version 1", role == Role::Unbound);
            return out;
        };
        if !self.lobbies.contains_key(&code) {
            out.push(Out { conn, text: json(&FromService::Closed { reason: "expired".into() }), close: true });
            self.conns.remove(&conn);
            return out;
        }
        match (role, msg) {
            (Role::Unbound, ToService::Host { wire, host_secret }) => {
                if !wire_supported(wire) {
                    self.fail(&mut out, conn, "wire_version", "this service speaks wire versions 1 and 2", true);
                    return out;
                }
                let lobby = self.lobbies.get_mut(&code).unwrap();
                if lobby.secret != host_secret {
                    self.fail(&mut out, conn, "bad_secret", "not this lobby's host", true);
                    return out;
                }
                // A second host socket (a reconnect racing the old one's close) replaces the first.
                if let Some(old) = lobby.host.replace(conn) {
                    out.push(Out { conn: old, text: json(&FromService::Closed { reason: "replaced".into() }), close: true });
                    self.conns.remove(&old);
                }
                lobby.host_gone = None;
                let peers = lobby.guests.keys().copied().collect();
                self.conns.get_mut(&conn).unwrap().role = Role::Host;
                Self::send(&mut out, conn, &FromService::Hosting { code, peers });
            }
            (Role::Unbound, ToService::Join { wire, name, key, candidates, password }) => {
                if !wire_supported(wire) {
                    self.fail(&mut out, conn, "wire_version", "this service speaks wire versions 1 and 2", true);
                    return out;
                }
                if !is_key(&key) {
                    self.fail(&mut out, conn, "bad_key", "a key is 64 lowercase hex digits", true);
                    return out;
                }
                let lobby = self.lobbies.get_mut(&code).unwrap();
                // A locked lobby with no password yet is never open by accident.
                let wrong = match &lobby.password {
                    Some(h) => password.as_deref() != Some(h.as_str()),
                    None => lobby.listing.locked,
                };
                if wrong {
                    out.push(Out { conn, text: json(&FromService::Declined { reason: "bad_password".into() }), close: true });
                    self.conns.remove(&conn);
                    return out;
                }
                let Some(host) = lobby.host else {
                    self.fail(&mut out, conn, "host_away", "the host is reconnecting; try again shortly", true);
                    return out;
                };
                if 1 + lobby.guests.len() >= lobby.req.max_players as usize {
                    self.fail(&mut out, conn, "full", "the lobby is full", true);
                    return out;
                }
                let peer = lobby.next_peer;
                lobby.next_peer += 1;
                lobby.guests.insert(peer, Guest { conn, accepted: false, helper: false });
                self.conns.get_mut(&conn).unwrap().role = Role::Guest(peer);
                Self::send(&mut out, conn, &FromService::Waiting { peer });
                Self::send(&mut out, host, &FromService::Join { peer, name, key, candidates });
            }
            (Role::Host, ToService::Accept { peer, candidates }) => {
                let lobby = self.lobbies.get_mut(&code).unwrap();
                let (host_key, host_name) = (lobby.req.host_key.clone(), lobby.req.name.clone());
                match lobby.guests.get_mut(&peer) {
                    Some(g) if !g.accepted => {
                        g.accepted = true;
                        let gc = g.conn;
                        Self::send(&mut out, gc, &FromService::Accepted { host_key, host_name, candidates });
                        // Both sides start their checks together (#3795 section 7.2).
                        Self::send(&mut out, conn, &FromService::Go { peer: Some(peer) });
                        Self::send(&mut out, gc, &FromService::Go { peer: None });
                        // The helper set may have changed before this guest was in.
                        out.extend(Self::helpers_to(lobby, peer));
                    }
                    Some(_) => {}
                    None => self.fail(&mut out, conn, "unknown_peer", "no such guest", false),
                }
            }
            (Role::Host, ToService::Decline { peer, reason }) => {
                let lobby = self.lobbies.get_mut(&code).unwrap();
                match lobby.guests.remove(&peer) {
                    Some(g) => {
                        out.push(Out { conn: g.conn, text: json(&FromService::Declined { reason }), close: true });
                        self.conns.remove(&g.conn);
                    }
                    None => self.fail(&mut out, conn, "unknown_peer", "no such guest", false),
                }
            }
            (Role::Host, ToService::Candidates { peer, candidates }) => {
                let lobby = &self.lobbies[&code];
                let to: Vec<ConnId> = match peer {
                    Some(p) => lobby.guests.get(&p).filter(|g| g.accepted).map(|g| g.conn).into_iter().collect(),
                    None => lobby.guests.values().filter(|g| g.accepted).map(|g| g.conn).collect(),
                };
                for gc in to {
                    Self::send(&mut out, gc, &FromService::Candidates { peer: None, candidates: candidates.clone() });
                }
            }
            (
                Role::Guest(peer),
                msg @ (ToService::Candidates { .. }
                | ToService::Connected { .. }
                | ToService::Reconnect
                | ToService::RelayAsk { .. }
                | ToService::RelayAnswer { .. }),
            ) => {
                let lobby = &self.lobbies[&code];
                if !lobby.guests.get(&peer).is_some_and(|g| g.accepted) {
                    self.fail(&mut out, conn, "not_accepted", "wait for the host", false);
                    return out;
                }
                // Guest to guest (#3795 section 8): both must be accepted, and not the same.
                let other = |p: PeerNo| lobby.guests.get(&p).filter(|g| g.accepted && p != peer).map(|g| g.conn);
                match msg {
                    ToService::RelayAsk { helper, candidates } => match other(helper) {
                        Some(to) => Self::send(&mut out, to, &FromService::RelayAsk { from: peer, candidates }),
                        None => self.fail(&mut out, conn, "unknown_peer", "no such guest", false),
                    },
                    ToService::RelayAnswer { for_peer, accept, reason, candidates } => match other(for_peer) {
                        Some(to) => Self::send(&mut out, to, &FromService::RelayAnswer { helper: peer, accept, reason, candidates }),
                        None => self.fail(&mut out, conn, "unknown_peer", "no such guest", false),
                    },
                    msg => {
                        let Some(host) = lobby.host else {
                            self.fail(&mut out, conn, "host_away", "the host is reconnecting", false);
                            return out;
                        };
                        match msg {
                            ToService::Candidates { candidates, .. } => {
                                Self::send(&mut out, host, &FromService::Candidates { peer: Some(peer), candidates })
                            }
                            ToService::Connected { path, .. } => {
                                let helper = path == "direct" || path == "lan";
                                Self::send(&mut out, host, &FromService::Connected { peer, path });
                                out.extend(self.set_helper(&code, peer, helper));
                            }
                            _ => {
                                Self::send(&mut out, host, &FromService::Reconnect { peer });
                                Self::send(&mut out, host, &FromService::Go { peer: Some(peer) });
                                Self::send(&mut out, conn, &FromService::Go { peer: None });
                                // A guest that lost its path may need a relay: it always gets the set.
                                let told = self.set_helper(&code, peer, false);
                                if told.is_empty() {
                                    out.extend(Self::helpers_to(&self.lobbies[&code], peer));
                                } else {
                                    out.extend(told);
                                }
                            }
                        }
                    }
                }
            }
            (Role::Host, ToService::Connected { .. }) => {}
            (Role::Host, ToService::Listing { public, title, place, friendly_fire }) => {
                let lobby = self.lobbies.get_mut(&code).unwrap();
                lobby.listing = clean_listing(&Listing { public, title, place, friendly_fire, ..lobby.listing.clone() });
            }
            (Role::Host, ToService::Password { hash }) => {
                let lobby = self.lobbies.get_mut(&code).unwrap();
                if hash.is_empty() {
                    lobby.password = None;
                    lobby.listing.locked = false;
                } else {
                    lobby.password = Some(hash.chars().take(128).collect());
                }
            }
            (Role::Host, ToService::Bye) => out.extend(self.end_lobby(&code, "host_left")),
            (Role::Guest(_), ToService::Bye) => {
                out.extend(self.close(conn, now));
                out.push(Out { conn, text: json(&FromService::Closed { reason: "bye".into() }), close: true });
            }
            _ => self.fail(&mut out, conn, "bad_message", "not expected now", role == Role::Unbound),
        }
        out
    }

    /// Marks whether a guest may relay; when the set changes, tells every accepted guest the
    /// others that may.
    fn set_helper(&mut self, code: &str, peer: PeerNo, helper: bool) -> Vec<Out> {
        let Some(lobby) = self.lobbies.get_mut(code) else { return Vec::new() };
        match lobby.guests.get_mut(&peer) {
            Some(g) if g.helper != helper => g.helper = helper,
            Some(_) => return Vec::new(),
            None => {}
        }
        Self::helpers_out(lobby)
    }

    fn helpers_out(lobby: &Lobby) -> Vec<Out> {
        lobby.guests.keys().flat_map(|&p| Self::helpers_to(lobby, p)).collect()
    }

    /// Tells accepted guest `peer` the others that may relay (nothing if it is not accepted).
    fn helpers_to(lobby: &Lobby, peer: PeerNo) -> Vec<Out> {
        let mut out = Vec::new();
        if let Some(g) = lobby.guests.get(&peer).filter(|g| g.accepted) {
            let peers = lobby.guests.iter().filter(|&(&q, h)| q != peer && h.accepted && h.helper).map(|(&q, _)| q).collect();
            Self::send(&mut out, g.conn, &FromService::Helpers { peers });
        }
        out
    }

    fn end_lobby(&mut self, code: &str, reason: &str) -> Vec<Out> {
        let mut out = Vec::new();
        let Some(lobby) = self.lobbies.remove(code) else { return out };
        let closed = json(&FromService::Closed { reason: reason.into() });
        for c in lobby.host.into_iter().chain(lobby.guests.values().map(|g| g.conn)) {
            out.push(Out { conn: c, text: closed.clone(), close: true });
            self.conns.remove(&c);
        }
        self.conns.retain(|_, c| c.code != code);
        out
    }

    /// Socket `conn` closed (or the guest said bye). A host's lobby waits out the grace; a
    /// guest is gone.
    pub fn close(&mut self, conn: ConnId, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        let Some(c) = self.conns.remove(&conn) else { return out };
        let Some(lobby) = self.lobbies.get_mut(&c.code) else { return out };
        match c.role {
            Role::Host if lobby.host == Some(conn) => {
                lobby.host = None;
                lobby.host_gone = Some(now);
            }
            Role::Guest(peer) if lobby.guests.get(&peer).is_some_and(|g| g.conn == conn) => {
                let was_helper = lobby.guests.remove(&peer).is_some_and(|g| g.helper);
                if let Some(host) = lobby.host {
                    Self::send(&mut out, host, &FromService::Left { peer });
                }
                if was_helper {
                    out.extend(Self::helpers_out(lobby));
                }
            }
            _ => {}
        }
        out
    }

    /// Ends lobbies whose host has been gone (or never came) past the grace.
    pub fn tick(&mut self, now: Instant) -> Vec<Out> {
        let grace = self.cfg.grace;
        let expired: Vec<String> = self
            .lobbies
            .iter()
            .filter(|(_, l)| l.host_gone.is_some_and(|t| now.saturating_duration_since(t) >= grace))
            .map(|(c, _)| c.clone())
            .collect();
        expired.iter().flat_map(|c| self.end_lobby(c, "expired")).collect()
    }

    /// `GET /v1/browse` with its query string (wire 2): (status, body). Lists public lobbies
    /// whose host is connected and that fewer than [`REPORTS_TO_HIDE`] have reported, newest
    /// first, [`BROWSE_PAGE`] a page. `km` and `region` stay empty: the stand-in has no
    /// data-centre locations.
    pub fn browse(&self, query: &str, now: Instant) -> (u16, String) {
        let q = BrowseQuery::parse(query);
        let needle = q.q.as_deref().map(str::to_lowercase);
        let mut rows: Vec<BrowseRow> = self
            .lobbies
            .iter()
            .filter(|(_, l)| l.listing.public && l.host.is_some() && l.reports.len() < REPORTS_TO_HIDE)
            .map(|(code, l)| BrowseRow {
                code: code.clone(),
                title: if l.listing.title.is_empty() { format!("{}'s game", l.req.name) } else { l.listing.title.clone() },
                host_name: clean_text(&l.req.name, 24),
                host_fp: key_fingerprint(&l.req.host_key),
                place: l.listing.place.clone(),
                players: l.players(),
                max_players: l.req.max_players,
                locked: l.locked(),
                friendly_fire: l.listing.friendly_fire,
                plugins: l.listing.plugins,
                region: String::new(),
                km: None,
                age: now.saturating_duration_since(l.created).as_secs(),
                protocol: l.req.protocol,
                build: l.req.build.clone(),
                load_order_hash: l.req.load_order_hash.clone(),
            })
            .filter(|r| q.protocol.is_none_or(|p| r.protocol == p))
            .filter(|r| q.load_order.as_ref().is_none_or(|h| &r.load_order_hash == h))
            .filter(|r| !(q.hide_empty && r.players <= 1))
            .filter(|r| !(q.hide_full && r.players >= r.max_players))
            .filter(|r| {
                needle.as_ref().is_none_or(|n| {
                    [&r.title, &r.host_name, &r.place].iter().any(|t| t.to_lowercase().contains(n.as_str()))
                })
            })
            .collect();
        rows.sort_by(|a, b| a.age.cmp(&b.age).then_with(|| a.code.cmp(&b.code)));
        let total = rows.len() as u32;
        let start: usize = q.cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(0).min(rows.len());
        let end = (start + BROWSE_PAGE).min(rows.len());
        let cursor = (end < rows.len()).then(|| end.to_string());
        let lobbies = rows.drain(start..end).collect();
        (200, json(&BrowseAnswer { lobbies, cursor, total }))
    }

    /// `POST /v1/lobbies/{code}/report` from `reporter` (the service passes the IP): (status,
    /// body). Each reporter counts once; at [`REPORTS_TO_HIDE`] the lobby leaves browse (still
    /// joinable by code). Kept only while the lobby lives.
    pub fn report(&mut self, code: &str, reporter: &str, body: &str) -> (u16, String) {
        if !body.trim().is_empty() && serde_json::from_str::<Report>(body).is_err() {
            return (400, error_body("bad_request", "a report is {} or {\"reason\": \"...\"}"));
        }
        let Some(l) = Code::parse(code).and_then(|c| self.lobbies.get_mut(&c.compact())) else {
            return (404, error_body("not_found", "no lobby with that code"));
        };
        l.reports.insert(reporter.chars().take(64).collect());
        (200, "{\"ok\":true}".to_string())
    }

    /// Lobbies alive, for tests and a self-hoster's status page.
    pub fn lobbies(&self) -> usize {
        self.lobbies.len()
    }
}
