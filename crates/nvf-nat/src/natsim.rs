//! An in-process NAT simulator for tests (#3795 section 12): LANs behind the usual NAT kinds,
//! two public STUN servers that answer Binding requests, and packet routing between them, all
//! on a queue with no sockets and no clock. Run whole connections through it with the real
//! [`crate::stun::Gather`] and [`crate::ice::Agent`].
//!
//! The NAT kinds, by how they map and filter (the classic names, RFC 3489's, with RFC 4787's
//! terms) [design: a model, not a measurement of any router]:
//!
//! | Kind | Public port | Lets in |
//! |---|---|---|
//! | `Open` | none needed: the machine has a public address | anything |
//! | `FullCone` | one per socket (endpoint-independent) | anything to that port |
//! | `Restricted` | one per socket | from an IP the socket has sent to |
//! | `PortRestricted` | one per socket | from an IP and port the socket has sent to |
//! | `Symmetric` | a new one per destination, the next free port | only from that destination |
//! | `SymmetricRandom` | a new one per destination, a random port (a carrier NAT) | only from that destination |
//! | `Blocked` | no UDP out or in (the LAN itself still works) | nothing |
//!
//! No hairpinning: a packet to the LAN's own public address is dropped, as on many routers.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::stun;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NatKind {
    Open,
    FullCone,
    Restricted,
    PortRestricted,
    Symmetric,
    SymmetricRandom,
    Blocked,
}

impl NatKind {
    pub const ALL: [NatKind; 7] = [
        NatKind::Open,
        NatKind::FullCone,
        NatKind::Restricted,
        NatKind::PortRestricted,
        NatKind::Symmetric,
        NatKind::SymmetricRandom,
        NatKind::Blocked,
    ];
}

pub type LanId = usize;

/// A datagram as the receiver sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub from: SocketAddr,
    pub to: SocketAddr,
    pub data: Vec<u8>,
}

struct Binding {
    internal: SocketAddr,
    sent_to: HashSet<SocketAddr>,
}

struct Lan {
    kind: NatKind,
    public: Ipv4Addr,
    hosts: u8,
    next_port: u16,
    /// `SymmetricRandom`'s port source (a xorshift; any fixed seed will do in tests).
    rng: u64,
    /// (internal, destination for symmetric) -> public port.
    map: HashMap<(SocketAddr, Option<SocketAddr>), u16>,
    bindings: HashMap<u16, Binding>,
}

/// The simulated internet. See the module notes.
pub struct Sim {
    lans: Vec<Lan>,
    endpoints: HashMap<SocketAddr, LanId>,
    stun: Vec<SocketAddr>,
    queue: Vec<Packet>,
}

impl Default for Sim {
    fn default() -> Self {
        Self::new()
    }
}

impl Sim {
    /// A world with two STUN servers, at 198.51.100.1:3478 and 198.51.100.2:3478 (a
    /// documentation range, RFC 5737).
    pub fn new() -> Self {
        Sim {
            lans: Vec::new(),
            endpoints: HashMap::new(),
            stun: vec![SocketAddr::from(([198, 51, 100, 1], 3478)), SocketAddr::from(([198, 51, 100, 2], 3478))],
            queue: Vec::new(),
        }
    }

    pub fn stun_servers(&self) -> &[SocketAddr] {
        &self.stun
    }

    /// A LAN behind a NAT of `kind`, with public address 203.0.113.(n+1).
    pub fn add_lan(&mut self, kind: NatKind) -> LanId {
        let n = self.lans.len();
        self.lans.push(Lan {
            kind,
            public: Ipv4Addr::new(203, 0, 113, n as u8 + 1),
            hosts: 0,
            next_port: 40000,
            rng: 0x9E37_79B9_7F4A_7C15 ^ (n as u64 + 1),
            map: HashMap::new(),
            bindings: HashMap::new(),
        });
        n
    }

    /// A socket on a machine on `lan`, at 192.168.(lan+1).(n+2):`port`, or on the public
    /// address for an `Open` LAN (one machine only).
    pub fn add_socket(&mut self, lan: LanId, port: u16) -> SocketAddr {
        let l = &mut self.lans[lan];
        let ip = if l.kind == NatKind::Open {
            assert_eq!(l.hosts, 0, "an open LAN holds one machine");
            l.public
        } else {
            Ipv4Addr::new(192, 168, lan as u8 + 1, l.hosts + 2)
        };
        l.hosts += 1;
        let addr = SocketAddr::new(IpAddr::V4(ip), port);
        self.endpoints.insert(addr, lan);
        addr
    }

    /// Another socket on the machine `beside` is on, at `port` (0: any free port).
    pub fn add_port(&mut self, beside: SocketAddr, port: u16) -> SocketAddr {
        let lan = self.endpoints[&beside];
        let mut addr = SocketAddr::new(beside.ip(), port);
        if port == 0 {
            addr.set_port(50000);
            while self.endpoints.contains_key(&addr) {
                addr.set_port(addr.port() + 1);
            }
        }
        self.endpoints.insert(addr, lan);
        addr
    }

    /// Sends a datagram from one of the sockets. It arrives on the next [`Sim::deliver`], or
    /// is dropped where a real network would drop it.
    pub fn send(&mut self, from: SocketAddr, to: SocketAddr, data: Vec<u8>) {
        let Some(&src_lan) = self.endpoints.get(&from) else { return };
        // Same LAN: straight there.
        if self.endpoints.get(&to) == Some(&src_lan) {
            self.queue.push(Packet { from, to, data });
            return;
        }
        if crate::candidate::is_lan(to.ip()) {
            return; // someone else's private address
        }
        let Some(ext) = self.outbound(src_lan, from, to) else { return };
        if self.stun.contains(&to) {
            if let Ok(msg) = stun::decode(&data) {
                if msg.kind == stun::BINDING_REQUEST {
                    let answer = stun::binding_success(msg.tx, ext);
                    self.inbound(to, ext, answer);
                }
            }
            return;
        }
        if self.lans[src_lan].public == to.ip() && self.lans[src_lan].kind != NatKind::Open {
            return; // no hairpinning
        }
        self.inbound(ext, to, data);
    }

    /// The source address a packet leaves the LAN with.
    fn outbound(&mut self, lan: LanId, from: SocketAddr, to: SocketAddr) -> Option<SocketAddr> {
        let l = &mut self.lans[lan];
        let key = match l.kind {
            NatKind::Open => return Some(from),
            NatKind::Blocked => return None,
            NatKind::Symmetric | NatKind::SymmetricRandom => (from, Some(to)),
            _ => (from, None),
        };
        let port = match l.map.get(&key) {
            Some(&p) => p,
            None => {
                let p = if l.kind == NatKind::SymmetricRandom {
                    loop {
                        l.rng ^= l.rng << 13;
                        l.rng ^= l.rng >> 7;
                        l.rng ^= l.rng << 17;
                        let p = 1024 + (l.rng % (65536 - 1024)) as u16;
                        if !l.bindings.contains_key(&p) {
                            break p;
                        }
                    }
                } else {
                    l.next_port += 1;
                    l.next_port - 1
                };
                l.map.insert(key, p);
                l.bindings.insert(p, Binding { internal: from, sent_to: HashSet::new() });
                p
            }
        };
        l.bindings.get_mut(&port).unwrap().sent_to.insert(to);
        Some(SocketAddr::new(IpAddr::V4(l.public), port))
    }

    /// A packet from the internet (`from`, already public) arriving at public address `to`.
    fn inbound(&mut self, from: SocketAddr, to: SocketAddr, data: Vec<u8>) {
        let Some(lan) = self.lans.iter().position(|l| IpAddr::V4(l.public) == to.ip()) else { return };
        let l = &self.lans[lan];
        let internal = match l.kind {
            NatKind::Open => {
                if !self.endpoints.contains_key(&to) {
                    return;
                }
                to
            }
            NatKind::Blocked => return,
            kind => {
                let Some(b) = l.bindings.get(&to.port()) else { return };
                let allowed = match kind {
                    NatKind::FullCone => true,
                    NatKind::Restricted => b.sent_to.iter().any(|s| s.ip() == from.ip()),
                    _ => b.sent_to.contains(&from),
                };
                if !allowed {
                    return;
                }
                b.internal
            }
        };
        self.queue.push(Packet { from, to: internal, data });
    }

    /// Every datagram that has arrived since the last call.
    pub fn deliver(&mut self) -> Vec<Packet> {
        std::mem::take(&mut self.queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::{Candidate, Kind};
    use crate::ice::{Action, Agent, Config, Role};
    use crate::report::Nat;
    use crate::stun::Gather;
    use std::time::{Duration, Instant};

    const PORT: u16 = 47960;
    const TICK: Duration = Duration::from_millis(10);

    /// Gathers each socket's candidates through the sim's STUN servers.
    fn gather(sim: &mut Sim, sockets: &[SocketAddr], t0: Instant) -> Vec<Vec<Candidate>> {
        gather_nat(sim, sockets, t0).into_iter().map(|(c, _)| c).collect()
    }

    /// [`gather`], with the NAT each socket's gather saw.
    fn gather_nat(sim: &mut Sim, sockets: &[SocketAddr], t0: Instant) -> Vec<(Vec<Candidate>, Nat)> {
        let servers: Vec<_> = sim.stun_servers().to_vec();
        let mut gathers: Vec<Gather> = sockets
            .iter()
            .enumerate()
            .map(|(i, _)| Gather::new(&[(servers[0], [i as u8; 12]), (servers[1], [i as u8 + 100; 12])], t0))
            .collect();
        let mut now = t0;
        while !gathers.iter().all(|g| g.done(now)) {
            for (g, &s) in gathers.iter_mut().zip(sockets) {
                for (to, bytes) in g.poll(now) {
                    sim.send(s, to, bytes);
                }
            }
            for p in sim.deliver() {
                if let Some(i) = sockets.iter().position(|&s| s == p.to) {
                    gathers[i].on_packet(p.from, &p.data);
                }
            }
            now += TICK;
        }
        gathers
            .iter()
            .zip(sockets)
            .map(|(g, &s)| {
                let mut c = vec![Candidate::new(Kind::Host, s)];
                for a in g.result().reflexive() {
                    if a != s {
                        c.push(Candidate::new(Kind::ServerReflexive, a));
                    }
                }
                (c, Nat::from_gather(&g.result()))
            })
            .collect()
    }

    /// Runs a host and a guest through the checks. Returns both chosen paths, or None.
    fn connect(sim: &mut Sim, host: SocketAddr, guest: SocketAddr) -> Option<(SocketAddr, SocketAddr)> {
        connect_with(sim, host, guest, &[])
    }

    /// [`connect`], with `extra` added to the host's candidates.
    fn connect_with(sim: &mut Sim, host: SocketAddr, guest: SocketAddr, extra: &[Candidate]) -> Option<(SocketAddr, SocketAddr)> {
        connect_cfg(sim, host, guest, extra, Config::default())
    }

    fn connect_cfg(sim: &mut Sim, host: SocketAddr, guest: SocketAddr, extra: &[Candidate], cfg: Config) -> Option<(SocketAddr, SocketAddr)> {
        connect_full(sim, host, guest, extra, cfg).map(|r| r.paths)
    }

    /// What [`connect_full`] saw.
    struct Run {
        /// (the host's remote, the guest's remote).
        paths: (SocketAddr, SocketAddr),
        /// (host, guest): the chosen local socket.
        locals: (SocketAddr, SocketAddr),
        /// Extra sockets each side opened, and how many were still open at the end.
        opened: [usize; 2],
        open_at_end: [usize; 2],
        /// Probes each side sent.
        probes: [usize; 2],
        /// From start to both sides choosing.
        took: Duration,
    }

    fn connect_full(sim: &mut Sim, host: SocketAddr, guest: SocketAddr, extra: &[Candidate], cfg: Config) -> Option<Run> {
        let t0 = Instant::now();
        let gathered = gather_nat(sim, &[host, guest], t0);
        let nats = [gathered[0].1, gathered[1].1];
        let mut cands: Vec<Vec<Candidate>> = gathered.into_iter().map(|(c, _)| c).collect();
        cands[0].extend_from_slice(extra);
        let mut agents = [Agent::new(Role::Host, &[host], cfg), Agent::new(Role::Guest, &[guest], cfg)];
        agents[0].set_local_nat(nats[0]);
        agents[1].set_local_nat(nats[1]);
        for c in &cands[1] {
            agents[0].add_remote(*c);
        }
        for c in &cands[0] {
            agents[1].add_remote(*c);
        }
        let sockets = [host, guest];
        // Every socket a side listens on: its own, then any extra ones.
        let mut owned: [Vec<SocketAddr>; 2] = [vec![host], vec![guest]];
        let mut opened = [0, 0];
        let mut probes = [0, 0];
        let start = t0 + Duration::from_secs(5);
        agents.iter_mut().for_each(|a| a.start(start));
        let mut now = start;
        let mut done = [false, false];
        while now < start + Duration::from_secs(12) && done != [true, true] {
            for (i, a) in agents.iter_mut().enumerate() {
                for act in a.poll(now) {
                    match act {
                        Action::SendProbe { from, to, check_id, .. } => {
                            assert!(owned[i].contains(&from), "side {i} probes from {from}, a socket it does not have");
                            probes[i] += 1;
                            let mut d = vec![b'P'];
                            d.extend_from_slice(&check_id.to_be_bytes());
                            sim.send(from, to, d);
                        }
                        Action::Selected { local, remote, .. } => {
                            done[i] = true;
                            if a.role() == Role::Guest {
                                sim.send(local, remote, vec![b'N']);
                            }
                        }
                        Action::OpenSockets { count } => {
                            let new: Vec<SocketAddr> = (0..count).map(|_| sim.add_port(sockets[i], 0)).collect();
                            opened[i] += new.len();
                            owned[i].extend_from_slice(&new);
                            a.add_extra_sockets(&new);
                        }
                        Action::CloseSockets { keep } => owned[i].retain(|&s| s == sockets[i] || Some(s) == keep),
                        Action::Failed => return None,
                        Action::Keepalive { .. } | Action::PathLost => {}
                    }
                }
            }
            for p in sim.deliver() {
                let Some(i) = owned.iter().position(|o| o.contains(&p.to)) else { continue };
                match p.data[0] {
                    b'P' => {
                        agents[i].on_probe_received(p.to, p.from, now);
                        let mut d = p.data.clone();
                        d[0] = b'R';
                        sim.send(p.to, p.from, d);
                    }
                    b'R' => {
                        let id = u64::from_be_bytes(p.data[1..9].try_into().unwrap());
                        agents[i].on_probe_result(id, p.to, p.from, now);
                    }
                    b'N' => agents[i].on_peer_selected(p.to, p.from, now),
                    _ => {}
                }
            }
            now += TICK;
        }
        if done != [true, true] {
            return None;
        }
        let (h, g) = (agents[0].selected()?, agents[1].selected()?);
        Some(Run {
            paths: (h.1, g.1),
            locals: (h.0, g.0),
            opened,
            open_at_end: [owned[0].len() - 1, owned[1].len() - 1],
            probes,
            took: now - start,
        })
    }

    /// Which kinds can reach each other with STUN, hole punching and the birthday punch
    /// [design: the textbook result for these models. Port-restricted against symmetric
    /// connects only through the birthday punch (#4263); symmetric against symmetric needs a
    /// relay].
    fn expected(a: NatKind, b: NatKind) -> bool {
        use NatKind::*;
        let sym = |k| matches!(k, Symmetric | SymmetricRandom);
        !(a == Blocked || b == Blocked || (sym(a) && sym(b)))
    }

    #[test]
    fn matrix() {
        let mut wrong = Vec::new();
        for host_kind in NatKind::ALL {
            for guest_kind in NatKind::ALL {
                let mut sim = Sim::new();
                let (hl, gl) = (sim.add_lan(host_kind), sim.add_lan(guest_kind));
                let (h, g) = (sim.add_socket(hl, PORT), sim.add_socket(gl, PORT));
                let got = connect(&mut sim, h, g).is_some();
                if got != expected(host_kind, guest_kind) {
                    wrong.push((host_kind, guest_kind, got));
                }
            }
        }
        assert!(wrong.is_empty(), "(host, guest, connected) not as expected: {wrong:?}");
    }

    #[test]
    fn same_lan_connects_behind_any_nat() {
        for kind in [NatKind::Symmetric, NatKind::Blocked, NatKind::PortRestricted] {
            let mut sim = Sim::new();
            let lan = sim.add_lan(kind);
            let (h, g) = (sim.add_socket(lan, PORT), sim.add_socket(lan, PORT));
            let (to_guest, to_host) = connect(&mut sim, h, g).unwrap_or_else(|| panic!("{kind:?}"));
            assert_eq!((to_guest, to_host), (g, h), "{kind:?}: the LAN path, not the internet");
        }
    }

    #[test]
    fn symmetric_guest_found_as_peer_reflexive() {
        let mut sim = Sim::new();
        let (hl, gl) = (sim.add_lan(NatKind::FullCone), sim.add_lan(NatKind::Symmetric));
        let (h, g) = (sim.add_socket(hl, PORT), sim.add_socket(gl, PORT));
        let (to_guest, _) = connect(&mut sim, h, g).unwrap();
        // The guest's NAT gave the host's address a port of its own, which neither STUN
        // server saw.
        assert_eq!(to_guest.ip(), IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2)));
        assert!(to_guest.port() >= 40002, "{to_guest}");
    }

    #[test]
    fn stun_sees_public_address() {
        let mut sim = Sim::new();
        let lan = sim.add_lan(NatKind::PortRestricted);
        let s = sim.add_socket(lan, PORT);
        let c = gather(&mut sim, &[s], Instant::now());
        assert_eq!(c[0][1], Candidate::new(Kind::ServerReflexive, "203.0.113.1:40000".parse().unwrap()));
        assert_eq!(c[0].len(), 2, "one public port for both servers");
    }

    /// Both ends symmetric, no direct path: through a relay a player hosts (#3795 section 9),
    /// with the real agent. The relay's address is one more remote candidate on both sides;
    /// what goes to it is wrapped and what comes from it unwrapped, by session.
    #[test]
    fn symmetric_pair_connects_through_a_hosted_relay() {
        use crate::hosted::{self, Client, Received, Server, ServerConfig, Side};
        let mut sim = Sim::new();
        let (hl, gl, rl) = (sim.add_lan(NatKind::Symmetric), sim.add_lan(NatKind::Symmetric), sim.add_lan(NatKind::Open));
        let (host, guest) = (sim.add_socket(hl, PORT), sim.add_socket(gl, PORT));
        let relay = sim.add_socket(rl, 47961);
        let t0 = Instant::now();
        let mut cands = gather(&mut sim, &[host, guest], t0);
        let session = hosted::session_id(&[1; 32], &[2; 32]);
        let mut server = Server::new(ServerConfig::default());
        let mut clients = [Client::new(relay, session, Side::Host), Client::new(relay, session, Side::Guest)];
        // The host has `[Coop] Relay=` set and offers it; the guest takes it from the candidates.
        cands[0].push(Candidate::new(Kind::Relayed, relay));
        let mut agents = [Agent::new(Role::Host, &[host], Config::default()), Agent::new(Role::Guest, &[guest], Config::default())];
        cands[1].iter().for_each(|c| agents[0].add_remote(*c));
        cands[0].iter().for_each(|c| agents[1].add_remote(*c));
        agents[0].add_remote(Candidate::new(Kind::Relayed, relay));
        let sockets = [host, guest];
        let start = t0 + Duration::from_secs(5);
        agents.iter_mut().for_each(|a| a.start(start));
        let mut now = start;
        let mut chosen: [Option<(SocketAddr, SocketAddr)>; 2] = [None, None];
        let send = |sim: &mut Sim, clients: &[Client; 2], i: usize, from: SocketAddr, to: SocketAddr, d: Vec<u8>| {
            if to == relay {
                sim.send(from, to, clients[i].wrap(&d));
            } else {
                sim.send(from, to, d);
            }
        };
        while now < start + Duration::from_secs(12) && chosen.iter().any(Option::is_none) {
            for i in 0..2 {
                if let Some(b) = clients[i].poll(now) {
                    sim.send(sockets[i], relay, b);
                }
                for act in agents[i].poll(now) {
                    match act {
                        Action::SendProbe { from, to, check_id, .. } => {
                            let mut d = vec![b'P'];
                            d.extend_from_slice(&check_id.to_be_bytes());
                            send(&mut sim, &clients, i, from, to, d);
                        }
                        Action::Selected { local, remote, .. } => {
                            chosen[i] = Some((local, remote));
                            if i == 1 {
                                send(&mut sim, &clients, i, local, remote, vec![b'N']);
                            }
                        }
                        Action::Failed => panic!("side {i} found no path"),
                        Action::Keepalive { .. } | Action::PathLost => {}
                        Action::OpenSockets { .. } | Action::CloseSockets { .. } => panic!("no birthday punch: no NAT kinds given"),
                    }
                }
            }
            for p in sim.deliver() {
                if p.to == relay {
                    for (to, d) in server.on_datagram(p.from, &p.data, now) {
                        sim.send(relay, to, d);
                    }
                    continue;
                }
                let Some(i) = sockets.iter().position(|&s| s == p.to) else { continue };
                let data = if p.from == relay {
                    match clients[i].on_datagram(&p.data) {
                        Received::Data(d) => d.to_vec(),
                        _ => continue,
                    }
                } else {
                    p.data.clone()
                };
                match data[0] {
                    b'P' => {
                        agents[i].on_probe_received(p.to, p.from, now);
                        let mut d = data.clone();
                        d[0] = b'R';
                        send(&mut sim, &clients, i, p.to, p.from, d);
                    }
                    b'R' => {
                        let id = u64::from_be_bytes(data[1..9].try_into().unwrap());
                        agents[i].on_probe_result(id, p.to, p.from, now);
                    }
                    b'N' => agents[i].on_peer_selected(p.to, p.from, now),
                    _ => {}
                }
            }
            now += TICK;
        }
        assert_eq!(chosen, [Some((host, relay)), Some((guest, relay))]);
        assert!(clients.iter().all(Client::bound));
        assert_eq!(agents[1].selected_candidate().unwrap().kind, Kind::Relayed);
        assert!(server.stats().forwarded >= 3, "{:?}", server.stats());
        // The relay answered within a round trip, but the guest gave a direct pair
        // `relay_settle` to answer first.
        assert!(now >= start + Config::default().relay_settle);
    }

    /// With a direct path to be had, a relay offered alongside is not chosen.
    #[test]
    fn direct_beats_a_hosted_relay() {
        let mut sim = Sim::new();
        let (hl, gl) = (sim.add_lan(NatKind::FullCone), sim.add_lan(NatKind::Symmetric));
        let (host, guest) = (sim.add_socket(hl, PORT), sim.add_socket(gl, PORT));
        let relay: SocketAddr = "198.51.100.9:47961".parse().unwrap();
        let (h, g) = connect_with(&mut sim, host, guest, &[Candidate::new(Kind::Relayed, relay)]).unwrap();
        assert!(h != relay && g != relay, "{h} {g}");
    }

    /// Port prediction (phase 5, off by default) reaches a symmetric NAT from a
    /// port-restricted one, both ways round; symmetric against symmetric still fails.
    #[test]
    fn port_prediction_reaches_a_symmetric_nat() {
        use NatKind::*;
        // Prediction alone: the birthday punch off.
        let cfg = Config { predict: 8, birthday_sockets: 0, ..Config::default() };
        for (h, g, want) in [
            (Symmetric, PortRestricted, true),
            (PortRestricted, Symmetric, true),
            (Symmetric, Restricted, true),
            (FullCone, Symmetric, true),
            (Symmetric, Symmetric, false),
        ] {
            let mut sim = Sim::new();
            let (hl, gl) = (sim.add_lan(h), sim.add_lan(g));
            let (hs, gs) = (sim.add_socket(hl, PORT), sim.add_socket(gl, PORT));
            assert_eq!(connect_cfg(&mut sim, hs, gs, &[], cfg).is_some(), want, "{h:?} host, {g:?} guest");
        }
        // Off (the default), and the birthday punch off too: no path.
        let mut sim = Sim::new();
        let (hl, gl) = (sim.add_lan(Symmetric), sim.add_lan(PortRestricted));
        let (hs, gs) = (sim.add_socket(hl, PORT), sim.add_socket(gl, PORT));
        assert!(connect_cfg(&mut sim, hs, gs, &[], Config { birthday_sockets: 0, ..Config::default() }).is_none());
    }

    /// The case of #4263: an easy NAT that filters by address and port (home broadband)
    /// against a carrier NAT with random ports (mobile data), both ways round. The birthday
    /// punch finds the path; the hard side keeps one extra socket, the chosen one.
    #[test]
    fn birthday_punch_reaches_a_carrier_nat() {
        use NatKind::*;
        for (h, g) in [(PortRestricted, SymmetricRandom), (SymmetricRandom, PortRestricted), (PortRestricted, Symmetric)] {
            let mut sim = Sim::new();
            let (hl, gl) = (sim.add_lan(h), sim.add_lan(g));
            let (hs, gs) = (sim.add_socket(hl, PORT), sim.add_socket(gl, PORT));
            let r = connect_full(&mut sim, hs, gs, &[], Config::default()).unwrap_or_else(|| panic!("{h:?} host, {g:?} guest"));
            let hard = if h == PortRestricted { 1 } else { 0 };
            let (easy_local, hard_local) = if hard == 1 { (r.locals.0, r.locals.1) } else { (r.locals.1, r.locals.0) };
            assert_eq!(easy_local, [hs, gs][1 - hard], "{h:?}/{g:?}: the easy side uses its own socket");
            assert_ne!(hard_local, [hs, gs][hard], "{h:?}/{g:?}: the hard side's path is an extra socket");
            assert_eq!(r.opened[hard], 256, "{h:?}/{g:?}");
            assert_eq!(r.opened[1 - hard], 0, "{h:?}/{g:?}: the easy side opens none");
            assert_eq!(r.open_at_end, [usize::from(hard == 0), usize::from(hard == 1)], "{h:?}/{g:?}: all but the chosen one closed");
            assert!(r.took < Duration::from_secs(10), "{h:?}/{g:?}: {:?}", r.took);
            // 1.3 s before the spray starts, then 250 a second: well under 10 s of spraying.
            assert!(r.probes[1 - hard] < 2400, "{h:?}/{g:?}: {} probes", r.probes[1 - hard]);
        }
    }

    /// Many seeds: the punch should almost always land inside the checks' 10 s (the odds in
    /// docs/spec-notes/nat-traversal.md put a miss at about 1 in 5,000).
    #[test]
    fn birthday_punch_odds() {
        let mut took = Vec::new();
        for seed in 0..40u8 {
            let mut sim = Sim::new();
            // Shift the carrier NAT's port source by adding LANs first.
            for _ in 0..seed % 7 {
                sim.add_lan(NatKind::FullCone);
            }
            let (hl, gl) = (sim.add_lan(NatKind::PortRestricted), sim.add_lan(NatKind::SymmetricRandom));
            sim.lans[gl].rng ^= u64::from(seed).wrapping_mul(0x2545_F491_4F6C_DD1D);
            let (hs, gs) = (sim.add_socket(hl, PORT), sim.add_socket(gl, PORT));
            let r = connect_full(&mut sim, hs, gs, &[], Config::default()).unwrap_or_else(|| panic!("seed {seed}"));
            took.push(r.took);
        }
        took.sort();
        let median = took[took.len() / 2];
        assert!(median < Duration::from_secs(4), "median {median:?}, all {took:?}");
    }

    /// Both sides hard: no punch can help, and the hard sides' extra sockets are all closed.
    #[test]
    fn birthday_punch_not_for_two_hard_nats() {
        let mut sim = Sim::new();
        let (hl, gl) = (sim.add_lan(NatKind::SymmetricRandom), sim.add_lan(NatKind::SymmetricRandom));
        let (hs, gs) = (sim.add_socket(hl, PORT), sim.add_socket(gl, PORT));
        assert!(connect(&mut sim, hs, gs).is_none());
    }
}
