//! Connectivity checks between two games, a small version of ICE (RFC 8445) as #3795 section
//! 7.2 describes: pair every local socket with every remote candidate of its family, probe the
//! pairs round-robin from both sides at once (hole punching), and let the guest name the best
//! pair that answered.
//!
//! Sans-IO. The agent never sees a packet: it asks for probes by check id, and the caller
//! sends them (as Noise handshake messages, which is what makes an answer trustworthy:
//! this agent believes whatever the caller tells it), and tells it what came back and from
//! where.
//!
//! The host and the guest each run an [`Agent`]. Both probe. The guest picks the pair
//! ([`Action::Selected`]) and the caller tells the host which, over that pair; the host calls
//! [`Agent::on_peer_selected`].
//!
//! The birthday punch (docs/spec-notes/nat-traversal.md, #4263): when this side's NAT is hard
//! and the other's easy, the agent asks for extra sockets ([`Action::OpenSockets`]) and
//! probes from each; when this side is easy and the other hard, it sprays the other side's
//! public IP across the port range. The caller says what its own NAT is
//! ([`Agent::set_local_nat`]).

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use crate::candidate::{canonical, pair_class, Candidate, Kind, RELAYED_CLASS};
use crate::report::Nat;

/// Which side of the join this agent is on. The guest nominates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Host,
    Guest,
}

/// Timings [design: #3795 section 7.2, except `settle`, this crate's own].
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// One probe per this, round-robin over the pairs still unanswered (20 ms).
    pub pace: Duration,
    /// No valid pair by then: give up (10 s).
    pub give_up: Duration,
    /// After the first valid pair, how long the guest waits for a better class to answer
    /// before it picks (200 ms). It picks at once when no better class is left to try.
    pub settle: Duration,
    /// The same wait when the best pair that answered goes through a hosted relay (2 s): a
    /// relay answers in one round trip, while punching a direct hole can take a few.
    pub relay_settle: Duration,
    /// A keepalive on the chosen pair this often while nothing else is sent (1 s): many NATs
    /// forget a UDP mapping after about 30 s.
    pub keepalive: Duration,
    /// Nothing heard on the chosen pair for this long: [`Action::PathLost`] (4 s, `net.rs`'s
    /// `LOST_AFTER`).
    pub lost_after: Duration,
    /// Port prediction against a symmetric NAT (#3795 section 7.2, phase 5): when the other
    /// side's STUN servers saw two different ports on one address, also probe this many ports
    /// above the highest, where its NAT's next mappings are likely to land. 0 turns it off,
    /// the default: it looks like a port scan to some firewalls, so it waits for a setting.
    pub predict: u16,
    /// The birthday punch (module notes): how many extra sockets the hard side asks for (256).
    /// 0 turns the punch off on both sides.
    pub birthday_sockets: u16,
    /// With no pair but a hosted relay's answered by then, the punch starts (1 s).
    pub birthday_after: Duration,
    /// The easy side's spray: one probe per this, to the next port of the sweep (4 ms).
    pub spray_pace: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pace: Duration::from_millis(20),
            give_up: Duration::from_secs(10),
            settle: Duration::from_millis(200),
            relay_settle: Duration::from_secs(2),
            keepalive: Duration::from_secs(1),
            lost_after: Duration::from_secs(4),
            predict: 0,
            birthday_sockets: 256,
            birthday_after: Duration::from_secs(1),
            spray_pace: Duration::from_millis(4),
        }
    }
}

pub type PairId = usize;

/// The `pair` of a probe that belongs to no pair yet: the birthday punch's.
pub const NO_PAIR: PairId = usize::MAX;

/// The spray waits this long after the punch starts, so the hard side's mappings exist first.
const SPRAY_DELAY: Duration = Duration::from_millis(300);
/// The sweep's ports: 1024..=65535, in steps of a prime that divides none of their count, so
/// every port comes once before any repeats.
const SWEEP_FIRST: u32 = 1024;
const SWEEP_LEN: u32 = 65536 - SWEEP_FIRST;
const SWEEP_STEP: u32 = 4099;
/// At most this many sprayed probes per poll, should the caller poll late.
const SPRAY_BURST: u32 = 16;

/// What the agent asks the caller to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Send a probe carrying `check_id` from the socket bound at `from` to `to`. `pair` is
    /// [`NO_PAIR`] for the birthday punch's probes. `from` may be an extra socket.
    SendProbe { pair: PairId, from: SocketAddr, to: SocketAddr, check_id: u64 },
    /// The birthday punch: open `count` more UDP sockets (any port, IPv4), listen on them as
    /// on the main one, answer probes from the socket they came in on, and pass their
    /// addresses to [`Agent::add_extra_sockets`].
    OpenSockets { count: u16 },
    /// Close the extra sockets, except `keep` (the chosen path's, when it is one of them: the
    /// game's traffic goes over it from now on).
    CloseSockets { keep: Option<SocketAddr> },
    /// This pair is the path. On the guest: tell the host (over it). On the host: the guest
    /// named it.
    Selected { pair: PairId, local: SocketAddr, remote: SocketAddr },
    /// Send a keepalive on the chosen path, unless game traffic went out on it just now.
    Keepalive { local: SocketAddr, remote: SocketAddr },
    /// Nothing answered: no direct path. The caller may try a relay (#3795 section 8).
    Failed,
    /// The chosen path went quiet. The caller may gather again and restart (section 7.3).
    PathLost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Waiting,
    InProgress,
    Succeeded { rtt: Duration },
}

#[derive(Clone, Debug)]
struct Pair {
    local: SocketAddr,
    remote: Candidate,
    class: u8,
    state: State,
    /// Outstanding checks: (check id, sent at). The last few only.
    checks: VecDeque<(u64, Instant)>,
}

const CHECKS_KEPT: usize = 16;

/// One side's connectivity checks. See the module notes.
pub struct Agent {
    role: Role,
    cfg: Config,
    locals: Vec<SocketAddr>,
    remotes: Vec<Candidate>,
    pairs: Vec<Pair>,
    next_check: u64,
    started: Option<Instant>,
    last_probe: Option<Instant>,
    cursor: usize,
    triggered: VecDeque<PairId>,
    first_valid: Option<Instant>,
    selected: Option<PairId>,
    failed: bool,
    last_keepalive: Option<Instant>,
    last_heard: Option<Instant>,
    lost_reported: bool,
    /// The addresses port prediction has already run for.
    predicted: Vec<IpAddr>,
    /// This side's NAT, from its own STUN gather.
    local_nat: Nat,
    /// The birthday punch's extra sockets, once the caller opened them.
    extras: Vec<SocketAddr>,
    /// [`Action::OpenSockets`] went out.
    extras_asked: bool,
    /// How many of `extras` have sent their probe.
    extras_probed: usize,
    /// How many ports the spray has probed.
    sprayed: u32,
    /// The birthday punch's probes still unanswered: check id -> sent at.
    punch_checks: HashMap<u64, Instant>,
    out: Vec<Action>,
}

/// What the birthday punch has this side do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Punch {
    /// Hard here, easy there: probe the other side's address from many sockets.
    Spread(SocketAddr),
    /// Easy here, hard there: probe the other side's IP across the port range.
    Spray(IpAddr),
}

impl Agent {
    /// `locals`: the addresses the caller's sockets are bound at, one per family (the base of
    /// every local candidate). The caller sends the other side every candidate it gathered
    /// (host, mapped, server reflexive); the agent only needs the sockets.
    pub fn new(role: Role, locals: &[SocketAddr], cfg: Config) -> Self {
        Agent {
            role,
            cfg,
            locals: locals.iter().map(|&l| canonical(l)).collect(),
            remotes: Vec::new(),
            pairs: Vec::new(),
            next_check: 1,
            started: None,
            last_probe: None,
            cursor: 0,
            triggered: VecDeque::new(),
            first_valid: None,
            selected: None,
            failed: false,
            last_keepalive: None,
            last_heard: None,
            lost_reported: false,
            predicted: Vec::new(),
            local_nat: Nat::Unknown,
            extras: Vec::new(),
            extras_asked: false,
            extras_probed: 0,
            sprayed: 0,
            punch_checks: HashMap::new(),
            out: Vec::new(),
        }
    }

    pub fn role(&self) -> Role {
        self.role
    }

    /// This side's NAT, as its own STUN gather saw it ([`Nat::from_gather`]). Unknown until
    /// called; the birthday punch needs it.
    pub fn set_local_nat(&mut self, nat: Nat) {
        self.local_nat = nat;
    }

    /// The extra sockets [`Action::OpenSockets`] asked for, as the caller bound them.
    pub fn add_extra_sockets(&mut self, addrs: &[SocketAddr]) {
        for &a in addrs {
            let a = canonical(a);
            if !self.extras.contains(&a) && !self.locals.contains(&a) {
                self.extras.push(a);
            }
        }
    }

    /// What the birthday punch did, for the caller's log once the checks end: `None` when it
    /// never ran on this side.
    pub fn punch_summary(&self) -> Option<String> {
        match self.punch()? {
            Punch::Spray(ip) if self.sprayed > 0 => Some(format!("birthday punch: sprayed {} ports of {ip}", self.sprayed)),
            Punch::Spread(to) if self.extras_asked => {
                Some(format!("birthday punch: {} extra sockets probed {to}", self.extras_probed))
            }
            _ => None,
        }
    }

    /// The extra sockets still open, as far as the agent knows.
    pub fn extra_sockets(&self) -> &[SocketAddr] {
        &self.extras
    }

    /// The other side's public IPv4 address when its NAT is hard: two or more
    /// server-reflexive ports on one address.
    fn remote_hard(&self) -> Option<IpAddr> {
        let srflx = || self.remotes.iter().filter(|r| r.kind == Kind::ServerReflexive && r.addr.is_ipv4());
        srflx().map(|r| r.addr.ip()).find(|&ip| srflx().filter(|r| r.addr.ip() == ip).count() >= 2)
    }

    /// The other side's one server-reflexive IPv4 address, when it has exactly one (an easy NAT).
    fn remote_easy(&self) -> Option<SocketAddr> {
        let mut srflx = self.remotes.iter().filter(|r| r.kind == Kind::ServerReflexive && r.addr.is_ipv4());
        match (srflx.next(), srflx.next()) {
            (Some(one), None) => Some(one.addr),
            _ => None,
        }
    }

    fn punch(&self) -> Option<Punch> {
        if self.cfg.birthday_sockets == 0 || !self.locals.iter().any(SocketAddr::is_ipv4) {
            return None;
        }
        match self.local_nat {
            Nat::Hard => self.remote_easy().map(Punch::Spread),
            Nat::Easy => self.remote_hard().map(Punch::Spray),
            _ => None,
        }
    }

    /// A candidate of the other side's, as the service passes it on. Pairs it with every local
    /// socket of the same family. Duplicates are ignored.
    pub fn add_remote(&mut self, c: Candidate) {
        self.remote(Candidate::new(c.kind, c.addr));
    }

    /// Adds the candidate and its pairs, if the address is new.
    fn remote(&mut self, c: Candidate) {
        if self.remotes.iter().any(|r| r.addr == c.addr) {
            return;
        }
        self.remotes.push(c);
        for i in 0..self.locals.len() {
            let local = self.locals[i];
            if local.is_ipv4() == c.addr.is_ipv4() {
                self.pairs.push(Pair { local, remote: c, class: pair_class(c), state: State::Waiting, checks: VecDeque::new() });
            }
        }
        if c.kind == Kind::ServerReflexive {
            self.predict(c.addr.ip());
        }
    }

    /// Port prediction (see [`Config::predict`]): two server-reflexive ports on one address
    /// mean a NAT that maps each destination anew, usually to the next free port. The ports
    /// just above the highest seen are added as candidates, once per address.
    fn predict(&mut self, ip: IpAddr) {
        if self.cfg.predict == 0 || self.predicted.contains(&ip) {
            return;
        }
        let ports: Vec<u16> =
            self.remotes.iter().filter(|r| r.kind == Kind::ServerReflexive && r.addr.ip() == ip).map(|r| r.addr.port()).collect();
        let (Some(&lo), Some(&hi)) = (ports.iter().min(), ports.iter().max()) else { return };
        if lo == hi {
            return;
        }
        self.predicted.push(ip);
        for d in 1..=self.cfg.predict {
            if let Some(port) = hi.checked_add(d) {
                self.remote(Candidate::new(Kind::ServerReflexive, SocketAddr::new(ip, port)));
            }
        }
    }

    fn find(&self, local: SocketAddr, remote: SocketAddr) -> Option<PairId> {
        self.pairs.iter().position(|p| p.local == local && p.remote.addr == remote)
    }

    /// The pair (local, remote), learning `remote` as peer reflexive when it is new.
    fn pair_for(&mut self, local: SocketAddr, remote: SocketAddr) -> Option<PairId> {
        let (local, remote) = (canonical(local), canonical(remote));
        if self.extras.contains(&local) {
            // An extra socket pairs only with the addresses that reached it.
            self.remote(Candidate::new(Kind::PeerReflexive, remote));
            if let Some(id) = self.find(local, remote) {
                return Some(id);
            }
            let c = *self.remotes.iter().find(|r| r.addr == remote)?;
            self.pairs.push(Pair { local, remote: c, class: pair_class(c), state: State::Waiting, checks: VecDeque::new() });
            return Some(self.pairs.len() - 1);
        }
        if !self.locals.contains(&local) {
            return None;
        }
        // A known address always has its pairs already (one per local socket of its family).
        self.remote(Candidate::new(Kind::PeerReflexive, remote));
        self.find(local, remote)
    }

    /// Both sides call this at the service's "go", so they punch at the same moment.
    pub fn start(&mut self, now: Instant) {
        if self.started.is_none() {
            self.started = Some(now);
        }
    }

    /// The other side's probe arrived on the socket at `local`, from `from` (the caller
    /// answers it). Learns `from` as a peer-reflexive candidate when it is new, and checks
    /// that pair next: its hole is open now (RFC 8445's triggered check).
    pub fn on_probe_received(&mut self, local: SocketAddr, from: SocketAddr, now: Instant) {
        self.last_heard = Some(now);
        if self.selected.is_some() {
            return;
        }
        if let Some(id) = self.pair_for(local, from) {
            if !matches!(self.pairs[id].state, State::Succeeded { .. }) && !self.triggered.contains(&id) {
                self.triggered.push_back(id);
            }
        }
    }

    /// The answer to probe `check_id` arrived on the socket at `local`, from `from`. When
    /// `from` is not where the probe went, the answer came through a NAT that picked a new
    /// port: `from` is learnt as peer reflexive and that pair is the valid one. Returns false
    /// for an unknown check id.
    pub fn on_probe_result(&mut self, check_id: u64, local: SocketAddr, from: SocketAddr, now: Instant) -> bool {
        if let Some(sent) = self.punch_checks.remove(&check_id) {
            self.last_heard = Some(now);
            if self.selected.is_none() {
                if let Some(v) = self.pair_for(local, from) {
                    self.succeed(v, now.saturating_duration_since(sent), now);
                }
            }
            return true;
        }
        let Some((id, sent)) = self.pairs.iter().enumerate().find_map(|(i, p)| {
            p.checks.iter().find(|(c, _)| *c == check_id).map(|&(_, sent)| (i, sent))
        }) else {
            return false;
        };
        self.last_heard = Some(now);
        if self.selected.is_some() {
            return true;
        }
        let from = canonical(from);
        let valid = if self.pairs[id].remote.addr == from && self.pairs[id].local == canonical(local) {
            Some(id)
        } else {
            self.pair_for(local, from)
        };
        if let Some(v) = valid {
            self.succeed(v, now.saturating_duration_since(sent), now);
        }
        true
    }

    fn succeed(&mut self, id: PairId, rtt: Duration, now: Instant) {
        let p = &mut self.pairs[id];
        p.state = match p.state {
            State::Succeeded { rtt: old } => State::Succeeded { rtt: old.min(rtt) },
            _ => State::Succeeded { rtt },
        };
        self.first_valid.get_or_insert(now);
    }

    /// The guest named the pair its message came over: the socket at `local`, from `from`.
    pub fn on_peer_selected(&mut self, local: SocketAddr, from: SocketAddr, now: Instant) {
        self.last_heard = Some(now);
        if self.role != Role::Host || self.selected.is_some() {
            return;
        }
        if let Some(id) = self.pair_for(local, from) {
            self.select(id, now);
        }
    }

    /// Any authenticated packet from the other side on the chosen path: it is alive.
    pub fn on_traffic(&mut self, now: Instant) {
        self.last_heard = Some(now);
        self.lost_reported = false;
    }

    /// Game traffic went out on the chosen path: no keepalive is needed for a while.
    pub fn on_sent(&mut self, now: Instant) {
        self.last_keepalive = Some(now);
    }

    /// The chosen path as (local socket, remote address).
    pub fn selected(&self) -> Option<(SocketAddr, SocketAddr)> {
        self.selected.map(|id| (self.pairs[id].local, self.pairs[id].remote.addr))
    }

    /// The chosen pair's candidate, to log what kind of path it is.
    pub fn selected_candidate(&self) -> Option<Candidate> {
        self.selected.map(|id| self.pairs[id].remote)
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    fn select(&mut self, id: PairId, now: Instant) {
        self.selected = Some(id);
        self.last_keepalive = Some(now);
        self.last_heard = Some(now);
        let p = &self.pairs[id];
        let local = p.local;
        self.out.push(Action::Selected { pair: id, local, remote: p.remote.addr });
        self.punch_checks.clear();
        if self.extras_asked {
            let keep = self.extras.contains(&local).then_some(local);
            self.out.push(Action::CloseSockets { keep });
            self.extras.retain(|&e| Some(e) == keep);
        }
    }

    fn fail(&mut self) {
        self.failed = true;
        self.out.push(Action::Failed);
        self.punch_checks.clear();
        if self.extras_asked {
            self.out.push(Action::CloseSockets { keep: None });
            self.extras.clear();
        }
    }

    /// A probe of the birthday punch's, from `from` to `to`.
    fn punch_probe(&mut self, from: SocketAddr, to: SocketAddr, now: Instant) {
        let check_id = self.next_check;
        self.next_check += 1;
        self.punch_checks.insert(check_id, now);
        self.out.push(Action::SendProbe { pair: NO_PAIR, from, to, check_id });
    }

    /// The birthday punch's step this poll (module notes).
    fn birthday(&mut self, started: Instant, now: Instant) {
        let since = now.saturating_duration_since(started);
        if since < self.cfg.birthday_after || self.best_valid().is_some_and(|(_, class)| class < RELAYED_CLASS) {
            return;
        }
        match self.punch() {
            Some(Punch::Spread(to)) => {
                if !self.extras_asked {
                    self.extras_asked = true;
                    self.out.push(Action::OpenSockets { count: self.cfg.birthday_sockets });
                }
                while self.extras_probed < self.extras.len() {
                    let from = self.extras[self.extras_probed];
                    self.extras_probed += 1;
                    self.punch_probe(from, to, now);
                }
            }
            Some(Punch::Spray(ip)) => {
                let Some(since) = since.checked_sub(self.cfg.birthday_after + SPRAY_DELAY) else { return };
                let due = (since.as_micros() / self.cfg.spray_pace.as_micros().max(1)) as u32 + 1;
                let Some(&from) = self.locals.iter().find(|l| l.is_ipv4()) else { return };
                // Where the sweep starts: from the address, so two hosts do not walk in step.
                let start = match ip {
                    IpAddr::V4(v4) => u32::from(v4) % SWEEP_LEN,
                    IpAddr::V6(_) => 0,
                };
                for _ in 0..due.saturating_sub(self.sprayed).min(SPRAY_BURST) {
                    let port = SWEEP_FIRST + (start + (self.sprayed % SWEEP_LEN) * SWEEP_STEP % SWEEP_LEN) % SWEEP_LEN;
                    self.sprayed += 1;
                    self.punch_probe(from, SocketAddr::new(ip, port as u16), now);
                }
            }
            None => {}
        }
    }

    fn best_valid(&self) -> Option<(PairId, u8)> {
        self.pairs
            .iter()
            .enumerate()
            .filter_map(|(i, p)| match p.state {
                State::Succeeded { rtt } => Some((i, p.class, rtt)),
                _ => None,
            })
            .min_by_key(|&(_, class, rtt)| (class, rtt))
            .map(|(i, class, _)| (i, class))
    }

    /// What to do now. Call every tick (10 ms is fine) and after each `on_*` call.
    pub fn poll(&mut self, now: Instant) -> Vec<Action> {
        let Some(started) = self.started else { return std::mem::take(&mut self.out) };
        if self.failed {
            return std::mem::take(&mut self.out);
        }
        if let Some(id) = self.selected {
            let p = &self.pairs[id];
            let (local, remote) = (p.local, p.remote.addr);
            if self.last_keepalive.is_none_or(|t| now.saturating_duration_since(t) >= self.cfg.keepalive) {
                self.last_keepalive = Some(now);
                self.out.push(Action::Keepalive { local, remote });
            }
            if !self.lost_reported && self.last_heard.is_some_and(|t| now.saturating_duration_since(t) >= self.cfg.lost_after) {
                self.lost_reported = true;
                self.out.push(Action::PathLost);
            }
            return std::mem::take(&mut self.out);
        }

        let timed_out = now.saturating_duration_since(started) >= self.cfg.give_up;
        let best = self.best_valid();
        if self.role == Role::Guest {
            if let Some((id, class)) = best {
                let better_left =
                    self.pairs.iter().any(|p| p.class < class && !matches!(p.state, State::Succeeded { .. }));
                let wait = if class >= RELAYED_CLASS { self.cfg.relay_settle } else { self.cfg.settle };
                let settled = self.first_valid.is_some_and(|t| now.saturating_duration_since(t) >= wait);
                if !better_left || settled || timed_out {
                    self.select(id, now);
                    return std::mem::take(&mut self.out);
                }
            }
        }
        if timed_out && best.is_none() {
            self.fail();
            return std::mem::take(&mut self.out);
        }
        self.birthday(started, now);

        if self.last_probe.is_none_or(|t| now.saturating_duration_since(t) >= self.cfg.pace) {
            if let Some(id) = self.next_pair() {
                self.last_probe = Some(now);
                let check_id = self.next_check;
                self.next_check += 1;
                let p = &mut self.pairs[id];
                if p.checks.len() == CHECKS_KEPT {
                    p.checks.pop_front();
                }
                p.checks.push_back((check_id, now));
                if p.state == State::Waiting {
                    p.state = State::InProgress;
                }
                self.out.push(Action::SendProbe { pair: id, from: p.local, to: p.remote.addr, check_id });
            }
        }
        std::mem::take(&mut self.out)
    }

    /// Triggered checks first, then round-robin over the pairs not yet answered.
    fn next_pair(&mut self) -> Option<PairId> {
        while let Some(id) = self.triggered.pop_front() {
            if !matches!(self.pairs[id].state, State::Succeeded { .. }) {
                return Some(id);
            }
        }
        let n = self.pairs.len();
        for step in 0..n {
            let id = (self.cursor + step) % n;
            if !matches!(self.pairs[id].state, State::Succeeded { .. }) {
                self.cursor = id + 1;
                return Some(id);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sa(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    fn probes(actions: &[Action]) -> Vec<(SocketAddr, u64)> {
        actions
            .iter()
            .filter_map(|a| match *a {
                Action::SendProbe { to, check_id, .. } => Some((to, check_id)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn pairs_by_family_and_round_robin_pace() {
        let t0 = Instant::now();
        let mut a = Agent::new(Role::Guest, &[sa("192.168.1.2:47960"), sa("[2001:db8::2]:47960")], Config::default());
        a.add_remote(Candidate::new(Kind::Host, sa("192.168.1.3:47960")));
        a.add_remote(Candidate::new(Kind::ServerReflexive, sa("203.0.113.3:5000")));
        a.add_remote(Candidate::new(Kind::Host, sa("[2001:db8::3]:47960")));
        a.add_remote(Candidate::new(Kind::Host, sa("192.168.1.3:47960")));
        assert_eq!(a.pairs.len(), 3, "v4 x 2 + v6 x 1, duplicate dropped");
        assert!(a.poll(t0).is_empty(), "nothing before start");
        a.start(t0);
        let mut sent = Vec::new();
        for ms in (0..200).step_by(10) {
            sent.extend(probes(&a.poll(t0 + Duration::from_millis(ms))));
        }
        assert_eq!(sent.len(), 10, "one probe per 20 ms");
        let targets: std::collections::HashSet<_> = sent.iter().take(3).map(|s| s.0).collect();
        assert_eq!(targets.len(), 3, "each pair once per round");
    }

    #[test]
    fn guest_picks_best_class_after_settle() {
        let t0 = Instant::now();
        let local = sa("192.168.1.2:47960");
        let mut a = Agent::new(Role::Guest, &[local], Config::default());
        let lan = sa("192.168.1.3:47960");
        let wan = sa("203.0.113.3:5000");
        a.add_remote(Candidate::new(Kind::ServerReflexive, wan));
        a.add_remote(Candidate::new(Kind::Host, lan));
        a.start(t0);
        let mut ids = std::collections::HashMap::new();
        for ms in (0..60).step_by(10) {
            for (to, id) in probes(&a.poll(t0 + Duration::from_millis(ms))) {
                ids.insert(to, id);
            }
        }
        // The srflx pair answers first; the LAN pair is still open, so the guest waits.
        assert!(a.on_probe_result(ids[&wan], local, wan, t0 + Duration::from_millis(60)));
        assert!(!a.poll(t0 + Duration::from_millis(70)).iter().any(|x| matches!(x, Action::Selected { .. })));
        // The LAN pair answers: best possible class, picked at once.
        assert!(a.on_probe_result(ids[&lan], local, lan, t0 + Duration::from_millis(80)));
        let acts = a.poll(t0 + Duration::from_millis(80));
        assert!(acts.contains(&Action::Selected { pair: 1, local, remote: lan }), "{acts:?}");
        assert_eq!(a.selected(), Some((local, lan)));
        assert!(probes(&a.poll(t0 + Duration::from_millis(200))).is_empty(), "probing stops");
    }

    #[test]
    fn guest_settles_for_worse_class() {
        let t0 = Instant::now();
        let local = sa("192.168.1.2:47960");
        let mut a = Agent::new(Role::Guest, &[local], Config::default());
        let wan = sa("203.0.113.3:5000");
        a.add_remote(Candidate::new(Kind::Host, sa("10.9.9.9:47960")));
        a.add_remote(Candidate::new(Kind::ServerReflexive, wan));
        a.start(t0);
        let mut id = 0;
        for ms in (0..60).step_by(10) {
            for (to, c) in probes(&a.poll(t0 + Duration::from_millis(ms))) {
                if to == wan {
                    id = c;
                }
            }
        }
        a.on_probe_result(id, local, wan, t0 + Duration::from_millis(60));
        assert!(!a.poll(t0 + Duration::from_millis(250)).iter().any(|x| matches!(x, Action::Selected { .. })));
        assert!(a.poll(t0 + Duration::from_millis(260)).iter().any(|x| matches!(x, Action::Selected { .. })));
    }

    #[test]
    fn answer_from_new_port_is_peer_reflexive() {
        let t0 = Instant::now();
        let local = sa("192.168.1.2:47960");
        let mut a = Agent::new(Role::Guest, &[local], Config::default());
        a.add_remote(Candidate::new(Kind::ServerReflexive, sa("203.0.113.3:5000")));
        a.start(t0);
        let (_, id) = probes(&a.poll(t0))[0];
        let symmetric = sa("203.0.113.3:6123");
        assert!(a.on_probe_result(id, local, symmetric, t0 + Duration::from_millis(30)));
        assert!(!a.on_probe_result(999, local, symmetric, t0), "unknown check");
        let acts = a.poll(t0 + Duration::from_millis(30));
        assert!(acts.iter().any(|x| matches!(x, Action::Selected { remote, .. } if *remote == symmetric)), "{acts:?}");
        assert_eq!(a.selected_candidate().unwrap().kind, Kind::PeerReflexive);
    }

    #[test]
    fn incoming_probe_triggers_check_first() {
        let t0 = Instant::now();
        let local = sa("192.168.1.2:47960");
        let mut a = Agent::new(Role::Host, &[local], Config::default());
        for p in 5000..5010 {
            a.add_remote(Candidate::new(Kind::ServerReflexive, SocketAddr::new("203.0.113.3".parse().unwrap(), p)));
        }
        a.start(t0);
        a.poll(t0);
        let from = sa("203.0.113.3:7000");
        a.on_probe_received(local, from, t0 + Duration::from_millis(5));
        let p = probes(&a.poll(t0 + Duration::from_millis(20)));
        assert_eq!(p[0].0, from);
    }

    #[test]
    fn host_follows_guest_then_keepalive_and_lost() {
        let t0 = Instant::now();
        let local = sa("192.168.1.2:47960");
        let guest = sa("203.0.113.3:5000");
        let mut a = Agent::new(Role::Host, &[local], Config::default());
        a.add_remote(Candidate::new(Kind::ServerReflexive, guest));
        a.start(t0);
        a.on_peer_selected(local, guest, t0 + Duration::from_millis(100));
        let acts = a.poll(t0 + Duration::from_millis(100));
        assert!(acts.contains(&Action::Selected { pair: 0, local, remote: guest }));
        assert!(a.poll(t0 + Duration::from_millis(1099)).is_empty());
        assert_eq!(a.poll(t0 + Duration::from_millis(1100)), vec![Action::Keepalive { local, remote: guest }]);
        a.on_sent(t0 + Duration::from_millis(2000));
        assert!(a.poll(t0 + Duration::from_millis(2500)).is_empty(), "game traffic stands in for keepalives");
        let acts = a.poll(t0 + Duration::from_millis(4100));
        assert!(acts.contains(&Action::PathLost));
        assert!(!a.poll(t0 + Duration::from_millis(4200)).contains(&Action::PathLost), "reported once");
    }

    /// Easy here, hard there: after 1.3 s, one sprayed probe per 4 ms to the other side's IP,
    /// every port once, none from a pair.
    #[test]
    fn easy_side_sprays_the_hard_sides_ip() {
        let t0 = Instant::now();
        let mut a = Agent::new(Role::Host, &[sa("192.168.1.2:47960")], Config::default());
        a.set_local_nat(Nat::Easy);
        a.add_remote(Candidate::new(Kind::ServerReflexive, sa("198.51.100.7:63744")));
        a.add_remote(Candidate::new(Kind::ServerReflexive, sa("198.51.100.7:41216")));
        a.start(t0);
        let mut sprayed = Vec::new();
        let mut first = None;
        for ms in (0..3300).step_by(10) {
            for act in a.poll(t0 + Duration::from_millis(ms)) {
                assert!(!matches!(act, Action::OpenSockets { .. }), "the easy side opens no sockets");
                if let Action::SendProbe { pair: NO_PAIR, to, .. } = act {
                    first.get_or_insert(ms);
                    assert_eq!(to.ip(), sa("198.51.100.7:1").ip());
                    sprayed.push(to.port());
                }
            }
        }
        assert_eq!(first, Some(1300), "1 s, then the 300 ms for the other side's sockets");
        assert!((495..=505).contains(&sprayed.len()), "2 s at 250 a second: {}", sprayed.len());
        assert_eq!(a.punch_summary(), Some(format!("birthday punch: sprayed {} ports of 198.51.100.7", sprayed.len())));
        let distinct: std::collections::HashSet<_> = sprayed.iter().collect();
        assert_eq!(distinct.len(), sprayed.len(), "no port twice");
        assert!(sprayed.iter().all(|&p| p >= 1024));
    }

    /// Hard here, easy there: ask for the sockets once, probe once from each, and on the
    /// choice close all but the chosen one.
    #[test]
    fn hard_side_spreads_and_keeps_the_chosen_socket() {
        let t0 = Instant::now();
        let main = sa("192.168.1.2:47960");
        let easy = sa("203.0.113.3:47960");
        let mut a = Agent::new(Role::Guest, &[main], Config::default());
        a.set_local_nat(Nat::Hard);
        a.add_remote(Candidate::new(Kind::Host, sa("192.168.7.3:47960")));
        a.add_remote(Candidate::new(Kind::ServerReflexive, easy));
        a.start(t0);
        assert!(!a.poll(t0 + Duration::from_millis(990)).iter().any(|x| matches!(x, Action::OpenSockets { .. })));
        let acts = a.poll(t0 + Duration::from_secs(1));
        assert!(acts.contains(&Action::OpenSockets { count: 256 }), "{acts:?}");
        assert!(!a.poll(t0 + Duration::from_millis(1010)).iter().any(|x| matches!(x, Action::OpenSockets { .. })), "asked once");
        let extras: Vec<SocketAddr> = (0..256u16).map(|i| SocketAddr::new(main.ip(), 50000 + i)).collect();
        a.add_extra_sockets(&extras);
        let punch: Vec<(SocketAddr, SocketAddr)> = a
            .poll(t0 + Duration::from_millis(1020))
            .into_iter()
            .filter_map(|x| match x {
                Action::SendProbe { pair: NO_PAIR, from, to, .. } => Some((from, to)),
                _ => None,
            })
            .collect();
        assert_eq!(punch.len(), 256);
        assert!(punch.iter().all(|&(_, to)| to == easy));
        assert!(a.poll(t0 + Duration::from_millis(1030)).iter().all(|x| !matches!(x, Action::SendProbe { pair: NO_PAIR, .. })), "once each");
        // A sprayed probe lands on extra socket 77 from the easy side's address; its triggered
        // check goes out from that socket and is answered.
        let hit = extras[77];
        a.on_probe_received(hit, easy, t0 + Duration::from_millis(1500));
        let check = a
            .poll(t0 + Duration::from_millis(1500))
            .into_iter()
            .find_map(|x| match x {
                Action::SendProbe { from, to, check_id, .. } if from == hit && to == easy => Some(check_id),
                _ => None,
            })
            .expect("a triggered check from the socket that was hit");
        assert!(a.on_probe_result(check, hit, easy, t0 + Duration::from_millis(1540)));
        // The LAN candidate has not answered, so the guest waits `settle` for it first.
        let acts = a.poll(t0 + Duration::from_millis(1750));
        assert!(acts.contains(&Action::CloseSockets { keep: Some(hit) }), "{acts:?}");
        assert_eq!(a.selected(), Some((hit, easy)));
        assert_eq!(a.extra_sockets(), &[hit]);
        assert_eq!(a.punch_summary().as_deref(), Some("birthday punch: 256 extra sockets probed 203.0.113.3:47960"));
    }

    /// No NAT kind given, both sides easy, or the punch turned off: nothing new happens.
    #[test]
    fn no_punch_without_the_pairing() {
        let t0 = Instant::now();
        for (nat, srflx, sockets) in [
            (Nat::Unknown, vec!["198.51.100.7:1000", "198.51.100.7:2000"], 256),
            (Nat::Easy, vec!["198.51.100.7:1000"], 256),
            (Nat::Hard, vec!["198.51.100.7:1000", "198.51.100.7:2000"], 256),
            (Nat::Easy, vec!["198.51.100.7:1000", "198.51.100.7:2000"], 0),
        ] {
            let mut a = Agent::new(Role::Guest, &[sa("192.168.1.2:47960")], Config { birthday_sockets: sockets, ..Config::default() });
            a.set_local_nat(nat);
            for s in &srflx {
                a.add_remote(Candidate::new(Kind::ServerReflexive, sa(s)));
            }
            a.start(t0);
            for ms in (0..12000).step_by(10) {
                for act in a.poll(t0 + Duration::from_millis(ms)) {
                    let new = matches!(act, Action::OpenSockets { .. } | Action::CloseSockets { .. } | Action::SendProbe { pair: NO_PAIR, .. });
                    assert!(!new, "{nat:?} {srflx:?} {sockets}: {act:?}");
                }
            }
            assert_eq!(a.punch_summary(), None, "{nat:?} {srflx:?} {sockets}");
        }
    }

    #[test]
    fn gives_up_after_ten_seconds() {
        let t0 = Instant::now();
        let mut a = Agent::new(Role::Guest, &[sa("192.168.1.2:47960")], Config::default());
        a.add_remote(Candidate::new(Kind::ServerReflexive, sa("203.0.113.3:5000")));
        a.start(t0);
        assert!(!a.poll(t0 + Duration::from_millis(9990)).contains(&Action::Failed));
        assert!(a.poll(t0 + Duration::from_secs(10)).contains(&Action::Failed));
        assert!(a.failed());
    }
}
