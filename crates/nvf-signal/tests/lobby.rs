//! The lobby protocol end to end: clients and the stand-in service, and a whole join through
//! simulated NATs (#3795 section 12, "Simulated NAT" and "Service").

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use nvf_nat::candidate::{Candidate, Kind};
use nvf_nat::ice::{Action, Agent, Config as IceConfig, Role};
use nvf_nat::natsim::{NatKind, Sim};
use nvf_nat::stun::Gather;
use nvf_signal::client::{GuestClient, GuestEvent, HostClient, HostEvent};
use nvf_signal::standin::{Config, ConnId, StandIn};
use nvf_signal::wire::{key_hex, CreateLobby, LobbyCreated, LobbyInfo, WIRE_VERSION};

const PORT: u16 = 47960;

fn key(n: u8) -> String {
    key_hex(&[n; 32])
}

fn create(si: &mut StandIn, max: u8, now: Instant) -> LobbyCreated {
    let req = CreateLobby {
        wire: WIRE_VERSION,
        protocol: 138,
        build: "0.8.1".into(),
        load_order_hash: "abc123".into(),
        max_players: max,
        host_key: key(1),
        name: "Courier".into(),
        listing: None,
    };
    let (status, body) = si.create(&serde_json::to_string(&req).unwrap(), now, 1_000);
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).unwrap()
}

/// Frames waiting for each socket, and the sockets the service closed.
#[derive(Default)]
struct Wires {
    inbox: HashMap<ConnId, Vec<String>>,
    closed: Vec<ConnId>,
}

impl Wires {
    fn send(&mut self, si: &mut StandIn, conn: ConnId, frames: Vec<String>, now: Instant) {
        for f in frames {
            for o in si.message(conn, &f, now) {
                self.inbox.entry(o.conn).or_default().push(o.text);
                if o.close {
                    self.closed.push(o.conn);
                }
            }
        }
    }
    fn take(&mut self, conn: ConnId) -> Vec<String> {
        self.inbox.remove(&conn).unwrap_or_default()
    }
}

fn host_events(h: &mut HostClient, w: &mut Wires, conn: ConnId) -> Vec<HostEvent> {
    w.take(conn).iter().flat_map(|f| h.on_text(f).unwrap()).collect()
}

fn guest_events(g: &mut GuestClient, w: &mut Wires, conn: ConnId) -> Vec<GuestEvent> {
    w.take(conn).iter().flat_map(|f| g.on_text(f).unwrap()).collect()
}

#[test]
fn info_and_gates() {
    let t0 = Instant::now();
    let mut si = StandIn::new(Config { min_protocol: 138, ..Config::default() });
    let lobby = create(&mut si, 6, t0);
    assert_eq!(lobby.code.len(), 6);
    assert_eq!(lobby.expires, 1_120);
    let (status, body) = si.info(&nvf_signal::code::Code::parse(&lobby.code).unwrap().to_string());
    assert_eq!(status, 200);
    let info: LobbyInfo = serde_json::from_str(&body).unwrap();
    assert_eq!((info.protocol, info.players, info.full, info.load_order_hash.as_str()), (138, 1, false, "abc123"));
    assert_eq!(si.info("ZZZ-ZZZ").0, 404);
    let old = r#"{"wire":1,"protocol":137,"build":"0.7","load_order_hash":"x","max_players":6,"host_key":"KEY","name":"n"}"#;
    assert_eq!(si.create(&old.replace("KEY", &key(2)), t0, 0).0, 403);
    assert_eq!(si.create("{}", t0, 0).0, 400);
}

#[test]
fn host_secret_checked() {
    let t0 = Instant::now();
    let mut si = StandIn::new(Config::default());
    let lobby = create(&mut si, 6, t0);
    let conn = si.open(&lobby.code).unwrap();
    let mut w = Wires::default();
    let mut h = HostClient::new("wrong");
    w.send(&mut si, conn, h.take_outgoing(), t0);
    let ev = host_events(&mut h, &mut w, conn);
    assert!(matches!(&ev[0], HostEvent::Error { error, .. } if error == "bad_secret"), "{ev:?}");
    assert_eq!(w.closed, vec![conn]);
}

#[test]
fn accept_decline_full_and_held_candidates() {
    let t0 = Instant::now();
    let mut si = StandIn::new(Config::default());
    let lobby = create(&mut si, 3, t0);
    let mut w = Wires::default();
    let hc = si.open(&lobby.code).unwrap();
    let mut h = HostClient::new(&lobby.host_secret);
    w.send(&mut si, hc, h.take_outgoing(), t0);
    assert!(matches!(&host_events(&mut h, &mut w, hc)[0], HostEvent::Hosting { code, .. } if *code == lobby.code));
    let hcand = Candidate::new(Kind::ServerReflexive, "203.0.113.1:40000".parse().unwrap());
    h.add_local(hcand);
    assert!(h.take_outgoing().is_empty(), "no guest to tell yet");

    // Guest A joins; a candidate found before the host answers is held.
    let ac = si.open(&lobby.code).unwrap();
    let a_lan = Candidate::new(Kind::Host, "192.168.2.2:47960".parse().unwrap());
    let mut a = GuestClient::new("A", &key(10), &[a_lan]);
    w.send(&mut si, ac, a.take_outgoing(), t0);
    assert_eq!(guest_events(&mut a, &mut w, ac), vec![GuestEvent::Waiting { peer: 1 }]);
    let ev = host_events(&mut h, &mut w, hc);
    assert_eq!(ev, vec![HostEvent::JoinRequest { peer: 1, name: "A".into(), key: key(10), candidates: vec![a_lan] }]);
    let a_srflx = Candidate::new(Kind::ServerReflexive, "203.0.113.2:40000".parse().unwrap());
    a.add_local(a_srflx);
    assert!(a.take_outgoing().is_empty(), "held until accepted");

    h.accept(1);
    w.send(&mut si, hc, h.take_outgoing(), t0);
    assert_eq!(host_events(&mut h, &mut w, hc), vec![HostEvent::Go { peer: 1 }]);
    let ev = guest_events(&mut a, &mut w, ac);
    assert_eq!(
        ev,
        vec![
            GuestEvent::Accepted { host_key: key(1), host_name: "Courier".into(), candidates: vec![hcand] },
            GuestEvent::Go,
            GuestEvent::Helpers { peers: vec![] }
        ]
    );
    // The held candidate goes now, and reaches the host.
    w.send(&mut si, ac, a.take_outgoing(), t0);
    assert_eq!(host_events(&mut h, &mut w, hc), vec![HostEvent::Candidates { peer: 1, candidates: vec![a_srflx] }]);
    // A host candidate found later reaches the accepted guest.
    let hmapped = Candidate::new(Kind::Mapped, "203.0.113.1:47960".parse().unwrap());
    h.add_local(hmapped);
    w.send(&mut si, hc, h.take_outgoing(), t0);
    assert_eq!(guest_events(&mut a, &mut w, ac), vec![GuestEvent::Candidates { candidates: vec![hmapped] }]);

    // Guest B is declined.
    let bc = si.open(&lobby.code).unwrap();
    let mut b = GuestClient::new("B", &key(11), &[]);
    w.send(&mut si, bc, b.take_outgoing(), t0);
    guest_events(&mut b, &mut w, bc);
    host_events(&mut h, &mut w, hc);
    h.decline(2, "Not now");
    w.send(&mut si, hc, h.take_outgoing(), t0);
    assert_eq!(guest_events(&mut b, &mut w, bc), vec![GuestEvent::Declined { reason: "Not now".into() }]);
    assert!(w.closed.contains(&bc));

    // C fills the lobby (host, A, C = 3); D finds it full.
    let cc = si.open(&lobby.code).unwrap();
    let mut c = GuestClient::new("C", &key(12), &[]);
    w.send(&mut si, cc, c.take_outgoing(), t0);
    assert_eq!(guest_events(&mut c, &mut w, cc), vec![GuestEvent::Waiting { peer: 3 }]);
    let info: LobbyInfo = serde_json::from_str(&si.info(&lobby.code).1).unwrap();
    assert!(info.full);
    let dc = si.open(&lobby.code).unwrap();
    let mut d = GuestClient::new("D", &key(13), &[]);
    w.send(&mut si, dc, d.take_outgoing(), t0);
    assert!(matches!(&guest_events(&mut d, &mut w, dc)[0], GuestEvent::Error { error, .. } if error == "full"));

    // A reports its path, then leaves.
    a.connected("direct");
    w.send(&mut si, ac, a.take_outgoing(), t0);
    let ev = host_events(&mut h, &mut w, hc);
    assert!(matches!(&ev[0], HostEvent::JoinRequest { peer: 3, .. }), "C's request: {ev:?}");
    assert_eq!(ev[1..], [HostEvent::Connected { peer: 1, path: "direct".into() }]);
    a.bye();
    w.send(&mut si, ac, a.take_outgoing(), t0);
    let ev = host_events(&mut h, &mut w, hc);
    assert!(ev.contains(&HostEvent::Left { peer: 1 }), "{ev:?}");
    assert!(w.closed.contains(&ac));

    // The host leaves: C is told and the code is free.
    h.bye();
    w.send(&mut si, hc, h.take_outgoing(), t0);
    assert_eq!(guest_events(&mut c, &mut w, cc), vec![GuestEvent::Closed { reason: "host_left".into() }]);
    assert_eq!(si.lobbies(), 0);
    assert_eq!(si.info(&lobby.code).0, 404);
}

#[test]
fn host_reconnects_within_grace_or_lobby_expires() {
    let t0 = Instant::now();
    let mut si = StandIn::new(Config::default());
    let lobby = create(&mut si, 6, t0);
    let mut w = Wires::default();
    let hc = si.open(&lobby.code).unwrap();
    let mut h = HostClient::new(&lobby.host_secret);
    w.send(&mut si, hc, h.take_outgoing(), t0);
    host_events(&mut h, &mut w, hc);
    let gc = si.open(&lobby.code).unwrap();
    let mut g = GuestClient::new("G", &key(5), &[]);
    w.send(&mut si, gc, g.take_outgoing(), t0);
    guest_events(&mut g, &mut w, gc);
    host_events(&mut h, &mut w, hc);

    // The host's socket drops; a new one within the grace finds its guest still there.
    si.close(hc, t0 + Duration::from_secs(1));
    assert!(si.tick(t0 + Duration::from_secs(100)).is_empty());
    let hc2 = si.open(&lobby.code).unwrap();
    h.hello();
    w.send(&mut si, hc2, h.take_outgoing(), t0 + Duration::from_secs(100));
    assert_eq!(host_events(&mut h, &mut w, hc2), vec![HostEvent::Hosting { code: lobby.code.clone(), peers: vec![1] }]);

    // It drops again and stays away: the lobby ends after 2 minutes.
    si.close(hc2, t0 + Duration::from_secs(200));
    assert!(si.tick(t0 + Duration::from_secs(319)).is_empty());
    let outs = si.tick(t0 + Duration::from_secs(320));
    assert!(outs.iter().any(|o| o.conn == gc && o.close && o.text.contains("expired")), "{outs:?}");
    assert_eq!(si.lobbies(), 0);

    // An unclaimed lobby (no host socket ever) expires too.
    let unclaimed = create(&mut si, 6, t0 + Duration::from_secs(400));
    assert_eq!(si.tick(t0 + Duration::from_secs(520)).len(), 0);
    assert_eq!(si.info(&unclaimed.code).0, 404);
}

fn gather(sim: &mut Sim, socket: SocketAddr, now: &mut Instant, seed: u8) -> Vec<Candidate> {
    let servers = sim.stun_servers().to_vec();
    let mut g = Gather::new(&[(servers[0], [seed; 12]), (servers[1], [seed + 1; 12])], *now);
    while !g.done(*now) {
        for (to, b) in g.poll(*now) {
            sim.send(socket, to, b);
        }
        for p in sim.deliver() {
            g.on_packet(p.from, &p.data);
        }
        *now += Duration::from_millis(10);
    }
    let mut c = vec![Candidate::new(Kind::Host, socket)];
    c.extend(g.result().reflexive().into_iter().map(|a| Candidate::new(Kind::ServerReflexive, a)));
    c
}

/// A whole join: the host creates a lobby, a guest behind a symmetric NAT joins by code, the
/// service passes keys and candidates, both start at `go`, and the checks find the path.
#[test]
fn join_by_code_through_nats() {
    let mut now = Instant::now();
    let mut sim = Sim::new();
    let (hl, gl) = (sim.add_lan(NatKind::Restricted), sim.add_lan(NatKind::Symmetric));
    let (hs, gs) = (sim.add_socket(hl, PORT), sim.add_socket(gl, PORT));

    let mut si = StandIn::new(Config::default());
    let lobby = create(&mut si, 6, now);
    let mut w = Wires::default();
    let hc = si.open(&lobby.code).unwrap();
    let mut host = HostClient::new(&lobby.host_secret);
    w.send(&mut si, hc, host.take_outgoing(), now);
    host_events(&mut host, &mut w, hc);
    for c in gather(&mut sim, hs, &mut now, 1) {
        host.add_local(c);
    }

    // The guest types the code as a player would.
    let typed = nvf_signal::code::Code::parse(&lobby.code).unwrap().to_string().to_lowercase();
    assert_eq!(si.info(&typed).0, 200);
    let gcands = gather(&mut sim, gs, &mut now, 3);
    let gc = si.open(&typed).unwrap();
    let mut guest = GuestClient::new("Six", &key(6), &gcands[..1]);
    w.send(&mut si, gc, guest.take_outgoing(), now);
    for c in &gcands[1..] {
        guest.add_local(*c);
    }

    let mut agents = [Agent::new(Role::Host, &[hs], IceConfig::default()), Agent::new(Role::Guest, &[gs], IceConfig::default())];
    let sockets = [hs, gs];
    let mut selected = [false, false];
    let mut host_heard_connected = false;
    let end = now + Duration::from_secs(12);
    while now < end && !(selected == [true, true] && host_heard_connected) {
        // Lobby traffic.
        for ev in host_events(&mut host, &mut w, hc) {
            match ev {
                HostEvent::JoinRequest { peer, key: k, candidates, .. } => {
                    assert_eq!(k, key(6));
                    candidates.into_iter().for_each(|c| agents[0].add_remote(c));
                    host.accept(peer);
                }
                HostEvent::Candidates { candidates, .. } => candidates.into_iter().for_each(|c| agents[0].add_remote(c)),
                HostEvent::Go { .. } => agents[0].start(now),
                HostEvent::Connected { path, .. } => {
                    assert_eq!(path, "direct");
                    host_heard_connected = true;
                }
                other => panic!("host: {other:?}"),
            }
        }
        w.send(&mut si, hc, host.take_outgoing(), now);
        for ev in guest_events(&mut guest, &mut w, gc) {
            match ev {
                GuestEvent::Waiting { .. } => {}
                GuestEvent::Accepted { host_key, candidates, .. } => {
                    assert_eq!(host_key, key(1));
                    candidates.into_iter().for_each(|c| agents[1].add_remote(c));
                }
                GuestEvent::Candidates { candidates } => candidates.into_iter().for_each(|c| agents[1].add_remote(c)),
                GuestEvent::Go => agents[1].start(now),
                GuestEvent::Helpers { .. } => {}
                other => panic!("guest: {other:?}"),
            }
        }
        w.send(&mut si, gc, guest.take_outgoing(), now);

        // Game traffic: probes, straight between the players through their NATs.
        for (i, a) in agents.iter_mut().enumerate() {
            for act in a.poll(now) {
                match act {
                    Action::SendProbe { from, to, check_id, .. } => {
                        let mut d = vec![b'P'];
                        d.extend_from_slice(&check_id.to_be_bytes());
                        sim.send(from, to, d);
                    }
                    Action::Selected { local, remote, .. } => {
                        selected[i] = true;
                        if i == 1 {
                            sim.send(local, remote, vec![b'N']);
                            guest.connected("direct");
                        }
                    }
                    Action::Failed => panic!("no path"),
                    _ => {}
                }
            }
        }
        for p in sim.deliver() {
            let Some(i) = sockets.iter().position(|&s| s == p.to) else { continue };
            match p.data[0] {
                b'P' => {
                    agents[i].on_probe_received(p.to, p.from, now);
                    let mut d = p.data.clone();
                    d[0] = b'R';
                    sim.send(p.to, p.from, d);
                }
                b'R' => {
                    agents[i].on_probe_result(u64::from_be_bytes(p.data[1..9].try_into().unwrap()), p.to, p.from, now);
                }
                b'N' => agents[i].on_peer_selected(p.to, p.from, now),
                _ => {}
            }
        }
        now += Duration::from_millis(10);
    }
    assert_eq!(selected, [true, true]);
    assert!(host_heard_connected);
    // The host talks to the guest's per-destination port, which only the checks could find.
    let (_, to_guest) = agents[0].selected().unwrap();
    assert_eq!(agents[0].selected_candidate().unwrap().kind, Kind::PeerReflexive);
    assert_eq!(to_guest.ip(), "203.0.113.2".parse::<std::net::IpAddr>().unwrap());
}

/// Section 8: guest C, with no path to the host, finds a helper and asks it through the
/// service, guest to guest.
#[test]
fn relay_exchange_between_guests() {
    let t0 = Instant::now();
    let mut si = StandIn::new(Config::default());
    let lobby = create(&mut si, 6, t0);
    let mut w = Wires::default();
    let hc = si.open(&lobby.code).unwrap();
    let mut h = HostClient::new(&lobby.host_secret);
    w.send(&mut si, hc, h.take_outgoing(), t0);
    host_events(&mut h, &mut w, hc);

    let join = |w: &mut Wires, si: &mut StandIn, h: &mut HostClient, name: &str, n: u8| {
        let c = si.open(&lobby.code).unwrap();
        let mut g = GuestClient::new(name, &key(n), &[]);
        w.send(si, c, g.take_outgoing(), t0);
        let peer = match guest_events(&mut g, w, c)[0] {
            GuestEvent::Waiting { peer } => peer,
            ref e => panic!("{e:?}"),
        };
        host_events(h, w, hc);
        h.accept(peer);
        w.send(si, hc, h.take_outgoing(), t0);
        host_events(h, w, hc);
        guest_events(&mut g, w, c);
        (c, g, peer)
    };
    let (ac, mut a, ap) = join(&mut w, &mut si, &mut h, "A", 10);
    let (bc, mut b, bp) = join(&mut w, &mut si, &mut h, "B", 11);
    let (cc, mut c, cp) = join(&mut w, &mut si, &mut h, "C", 12);

    // Asking before anyone is a helper still routes (C may know A from earlier).
    c.relay_ask(99, &[]);
    w.send(&mut si, cc, c.take_outgoing(), t0);
    assert!(matches!(&guest_events(&mut c, &mut w, cc)[0], GuestEvent::Error { error, .. } if error == "unknown_peer"));

    // A and B report direct paths: everyone hears who could relay.
    a.connected("direct");
    w.send(&mut si, ac, a.take_outgoing(), t0);
    b.connected("lan");
    w.send(&mut si, bc, b.take_outgoing(), t0);
    let last_helpers = |ev: Vec<GuestEvent>| {
        ev.into_iter()
            .rev()
            .find_map(|e| match e {
                GuestEvent::Helpers { peers } => Some(peers),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(last_helpers(guest_events(&mut c, &mut w, cc)), vec![ap, bp]);
    assert_eq!(last_helpers(guest_events(&mut a, &mut w, ac)), vec![bp], "not itself");
    host_events(&mut h, &mut w, hc);
    guest_events(&mut b, &mut w, bc);

    // C asks A, which refuses; then B, which accepts with its candidates.
    let c_cand = Candidate::new(Kind::ServerReflexive, "198.51.100.7:50000".parse().unwrap());
    c.relay_ask(ap, &[c_cand]);
    w.send(&mut si, cc, c.take_outgoing(), t0);
    assert_eq!(guest_events(&mut a, &mut w, ac), vec![GuestEvent::RelayAsk { from: cp, candidates: vec![c_cand] }]);
    assert!(host_events(&mut h, &mut w, hc).is_empty(), "the host is not involved");
    a.relay_answer(cp, false, "busy", &[]);
    w.send(&mut si, ac, a.take_outgoing(), t0);
    assert_eq!(
        guest_events(&mut c, &mut w, cc),
        vec![GuestEvent::RelayAnswer { helper: ap, accept: false, reason: "busy".into(), candidates: vec![] }]
    );
    c.relay_ask(bp, &[c_cand]);
    w.send(&mut si, cc, c.take_outgoing(), t0);
    guest_events(&mut b, &mut w, bc);
    let b_cand = Candidate::new(Kind::ServerReflexive, "203.0.113.9:47960".parse().unwrap());
    b.relay_answer(cp, true, "", &[b_cand]);
    w.send(&mut si, bc, b.take_outgoing(), t0);
    assert_eq!(
        guest_events(&mut c, &mut w, cc),
        vec![GuestEvent::RelayAnswer { helper: bp, accept: true, reason: String::new(), candidates: vec![b_cand] }]
    );

    // B loses its path: it is no longer offered.
    b.reconnect();
    w.send(&mut si, bc, b.take_outgoing(), t0);
    assert_eq!(last_helpers(guest_events(&mut c, &mut w, cc)), vec![ap]);

    // D is accepted after the set last changed and still learns it (found live, lobby Q5Y-C1C).
    let dc = si.open(&lobby.code).unwrap();
    let mut d = GuestClient::new("D", &key(13), &[]);
    w.send(&mut si, dc, d.take_outgoing(), t0);
    let dp = match guest_events(&mut d, &mut w, dc)[0] {
        GuestEvent::Waiting { peer } => peer,
        ref e => panic!("{e:?}"),
    };
    host_events(&mut h, &mut w, hc);
    h.accept(dp);
    w.send(&mut si, hc, h.take_outgoing(), t0);
    host_events(&mut h, &mut w, hc);
    assert_eq!(last_helpers(guest_events(&mut d, &mut w, dc)), vec![ap]);
    // A reconnect that changes no helper still tells the reconnecting guest the set.
    c.reconnect();
    w.send(&mut si, cc, c.take_outgoing(), t0);
    assert_eq!(last_helpers(guest_events(&mut c, &mut w, cc)), vec![ap]);
    assert!(guest_events(&mut d, &mut w, dc).is_empty(), "the others hear nothing new");
    host_events(&mut h, &mut w, hc);

    // A leaves: the list empties.
    a.bye();
    w.send(&mut si, ac, a.take_outgoing(), t0);
    assert_eq!(last_helpers(guest_events(&mut c, &mut w, cc)), Vec::<u32>::new());
}

/// The lobby browser (wire 2, docs/design/coop-lobby-browser.md): listings, passwords, browse
/// and reports in the stand-in.
#[test]
fn browser_listings_passwords_and_reports() {
    use nvf_signal::code::Code;
    use nvf_signal::wire::{password_hash, BrowseAnswer, Listing};
    let t0 = Instant::now();
    let mut si = StandIn::new(Config::default());
    let mut w = Wires::default();
    let make = |si: &mut StandIn, name: &str, listing: Option<Listing>| -> LobbyCreated {
        let req = CreateLobby {
            wire: WIRE_VERSION,
            protocol: 139,
            build: "0.8.1".into(),
            load_order_hash: "abc123".into(),
            max_players: 4,
            host_key: key(1),
            name: name.into(),
            listing,
        };
        let (status, body) = si.create(&serde_json::to_string(&req).unwrap(), t0, 1_000);
        assert_eq!(status, 200, "{body}");
        serde_json::from_str(&body).unwrap()
    };
    let browse = |si: &StandIn, q: &str| -> BrowseAnswer { serde_json::from_str(&si.browse(q, t0 + Duration::from_secs(9)).1).unwrap() };

    let public = make(&mut si, "Courier", Some(Listing { public: true, title: "STRIP RUN".into(), place: "The Strip".into(), plugins: 14, ..Listing::default() }));
    let private = make(&mut si, "Six", None);
    let locked = make(&mut si, "Boone", Some(Listing { public: true, locked: true, ..Listing::default() }));
    // A lobby is listed only while its host's socket is open.
    assert_eq!(browse(&si, "").total, 0);
    let mut hosts = HashMap::new();
    for l in [&public, &private, &locked] {
        let c = si.open(&l.code).unwrap();
        let mut h = HostClient::new(&l.host_secret);
        w.send(&mut si, c, h.take_outgoing(), t0);
        host_events(&mut h, &mut w, c);
        hosts.insert(l.code.clone(), (c, h));
    }
    let all = browse(&si, "");
    assert_eq!(all.total, 2, "the private lobby is not listed");
    let row = all.lobbies.iter().find(|r| r.code == public.code).unwrap();
    assert_eq!((row.title.as_str(), row.place.as_str(), row.plugins, row.players), ("strip run", "The Strip", 14, 1), "capitals lowered");
    assert_eq!((row.host_fp.as_str(), row.km, row.age), (&key(1)[..16], None, 9));
    let lrow = all.lobbies.iter().find(|r| r.code == locked.code).unwrap();
    assert_eq!((lrow.title.as_str(), lrow.locked), ("Boone's game", true));
    assert_eq!(browse(&si, "?q=strip").total, 1);
    assert_eq!(browse(&si, "?q=BOONE").total, 1);
    assert_eq!(browse(&si, "?protocol=138").total, 0);
    assert_eq!(browse(&si, "?load_order=other").total, 0);
    assert_eq!(browse(&si, "?empty=0").total, 0, "both have only their host");

    // Locked, no password yet: every join is declined.
    let try_join = |si: &mut StandIn, w: &mut Wires, code: &str, pw: Option<&str>| -> Vec<GuestEvent> {
        let c = si.open(code).unwrap();
        let mut g = GuestClient::joining("Guest", &key(9), &[], pw);
        w.send(si, c, g.take_outgoing(), t0);
        guest_events(&mut g, w, c)
    };
    let declined = vec![GuestEvent::Declined { reason: "bad_password".into() }];
    assert_eq!(try_join(&mut si, &mut w, &locked.code, None), declined);
    let code = Code::parse(&locked.code).unwrap();
    let (lc, lh) = hosts.get_mut(&locked.code).unwrap();
    lh.password(&password_hash(&code, "hunter2"));
    let lc = *lc;
    w.send(&mut si, lc, lh.take_outgoing(), t0);
    assert!(serde_json::from_str::<LobbyInfo>(&si.info(&locked.code).1).unwrap().locked);
    assert_eq!(try_join(&mut si, &mut w, &locked.code, Some(&password_hash(&code, "wrong"))), declined);
    // The same password hashed for another lobby's code does not open this one.
    let other = Code::parse(&public.code).unwrap();
    assert_eq!(try_join(&mut si, &mut w, &locked.code, Some(&password_hash(&other, "hunter2"))), declined);
    assert!(matches!(try_join(&mut si, &mut w, &locked.code, Some(&password_hash(&code, "hunter2")))[..], [GuestEvent::Waiting { .. }]));
    // A private lobby may have a password too; an empty hash removes it.
    let (pc, ph) = hosts.get_mut(&private.code).unwrap();
    ph.password(&password_hash(&Code::parse(&private.code).unwrap(), "x"));
    let pc = *pc;
    w.send(&mut si, pc, ph.take_outgoing(), t0);
    assert!(serde_json::from_str::<LobbyInfo>(&si.info(&private.code).1).unwrap().locked);
    assert_eq!(try_join(&mut si, &mut w, &private.code, None), declined);
    let (_, ph) = hosts.get_mut(&private.code).unwrap();
    ph.password("");
    w.send(&mut si, pc, ph.take_outgoing(), t0);
    assert!(!serde_json::from_str::<LobbyInfo>(&si.info(&private.code).1).unwrap().locked);
    assert!(matches!(try_join(&mut si, &mut w, &private.code, None)[..], [GuestEvent::Waiting { .. }]));

    // The host's listing message: a new place, then private (off the list at once).
    let (pubc, pubh) = hosts.get_mut(&public.code).unwrap();
    pubh.listing(true, "Strip run", "Goodsprings", true);
    let pubc = *pubc;
    w.send(&mut si, pubc, pubh.take_outgoing(), t0);
    let row = browse(&si, "?q=goodsprings").lobbies.remove(0);
    assert_eq!((row.place.as_str(), row.friendly_fire, row.plugins), ("Goodsprings", true, 14));
    let (_, pubh) = hosts.get_mut(&public.code).unwrap();
    pubh.listing(false, "Strip run", "Goodsprings", true);
    w.send(&mut si, pubc, pubh.take_outgoing(), t0);
    assert_eq!(browse(&si, "").total, 1);
    let (_, pubh) = hosts.get_mut(&public.code).unwrap();
    pubh.listing(true, "Strip run", "Goodsprings", true);
    w.send(&mut si, pubc, pubh.take_outgoing(), t0);

    // Reports: once per reporter; five hide the lobby from browse, not from its code.
    for ip in ["a", "a", "b", "c", "d"] {
        assert_eq!(si.report(&public.code, ip, "{}").0, 200);
    }
    assert_eq!(browse(&si, "").total, 2, "four reporters so far");
    assert_eq!(si.report(&public.code, "e", r#"{"reason":"spam"}"#).0, 200);
    assert_eq!(browse(&si, "").total, 1);
    assert_eq!(si.info(&public.code).0, 200);
    assert_eq!(si.report("ZZZ-ZZZ", "a", "{}").0, 404);
    assert_eq!(si.report(&public.code, "a", "not json").0, 400);
}

/// A wire-1 game (before the browser) still hosts and joins; its lobby is private.
#[test]
fn wire_one_still_works() {
    let t0 = Instant::now();
    let mut si = StandIn::new(Config::default());
    let old = format!(r#"{{"wire":1,"protocol":138,"build":"0.8","load_order_hash":"x","max_players":2,"host_key":"{}","name":"n"}}"#, key(1));
    let (status, body) = si.create(&old, t0, 0);
    assert_eq!(status, 200, "{body}");
    let lobby: LobbyCreated = serde_json::from_str(&body).unwrap();
    let hc = si.open(&lobby.code).unwrap();
    let mut w = Wires::default();
    w.send(&mut si, hc, vec![format!(r#"{{"type":"host","wire":1,"host_secret":"{}"}}"#, lobby.host_secret)], t0);
    assert!(w.take(hc)[0].contains("hosting"));
    let gc = si.open(&lobby.code).unwrap();
    w.send(&mut si, gc, vec![format!(r#"{{"type":"join","wire":1,"name":"g","key":"{}","candidates":[]}}"#, key(2))], t0);
    assert!(w.take(gc)[0].contains("waiting"));
    assert_eq!(si.browse("", t0).1, r#"{"lobbies":[],"cursor":null,"total":0}"#);
}

/// Section 8 end to end, in one process: the host and guest C are both behind symmetric NATs,
/// so C's direct checks fail. Guest A, behind a full-cone NAT, reaches the host and becomes a
/// helper. C's planner asks A through the service, A's policy accepts, their checks find a
/// path, and a packet from C reaches the host through A and comes back. That confirms the
/// relay, and C reports `relay` to the host. Before this the pieces were only tested apart,
/// and a gap between them (a late guest never hearing `helpers`) was first found live.
#[test]
fn relay_through_a_helper_across_nats() {
    use nvf_nat::relay::{self, Answer, Helper, Planner, Policy, Refusal};
    const H: usize = 0;
    const A: usize = 1;
    const C: usize = 2;

    let mut now = Instant::now();
    let mut sim = Sim::new();
    let kinds = [NatKind::Symmetric, NatKind::FullCone, NatKind::Symmetric];
    let socks: Vec<SocketAddr> = kinds
        .iter()
        .map(|&k| {
            let lan = sim.add_lan(k);
            sim.add_socket(lan, PORT)
        })
        .collect();
    let cands: Vec<Vec<Candidate>> = (0..3).map(|i| gather(&mut sim, socks[i], &mut now, 1 + 2 * i as u8)).collect();

    let mut si = StandIn::new(Config::default());
    let lobby = create(&mut si, 6, now);
    let mut w = Wires::default();
    let hc = si.open(&lobby.code).unwrap();
    let mut host = HostClient::new(&lobby.host_secret);
    w.send(&mut si, hc, host.take_outgoing(), now);
    host_events(&mut host, &mut w, hc);
    cands[H].iter().for_each(|&c| host.add_local(c));

    // A joins first, then C, so their peer numbers (1, 2) are their indexes; guests[i - 1].
    // C joins only once A is a helper: the case first found live (lobby Q5Y-C1C).
    let mut conns = [hc, si.open(&lobby.code).unwrap(), 0];
    let mut guests = [GuestClient::new("A", &key(11), &cands[A]), GuestClient::new("C", &key(12), &cands[C])];
    w.send(&mut si, conns[A], guests[0].take_outgoing(), now);
    let mut joined = vec![A];

    // One agent per (player, other player); every packet carries its sender's index.
    let mut agents: HashMap<(usize, usize), Agent> = HashMap::new();
    let mut planner = Planner::new(relay::Config::default());
    let policy = Policy::default();
    let (mut a_direct, mut c_direct_failed, mut chosen, mut confirmed) = (false, false, None, false);
    let mut host_heard: Vec<(u32, String)> = Vec::new();
    let mut try_reach_at = now;

    let end = now + Duration::from_secs(40);
    while now < end && !host_heard.contains(&(C as u32, "relay".into())) {
        // Only after the service has passed A's `connected` on: the helper set is settled.
        if joined.len() == 1 && host_heard.contains(&(A as u32, "direct".into())) {
            conns[C] = si.open(&lobby.code).unwrap();
            w.send(&mut si, conns[C], guests[C - 1].take_outgoing(), now);
            joined.push(C);
        }

        // Lobby traffic, host side.
        for ev in host_events(&mut host, &mut w, hc) {
            match ev {
                HostEvent::JoinRequest { peer, candidates, .. } => {
                    let mut ag = Agent::new(Role::Host, &[socks[H]], IceConfig::default());
                    candidates.into_iter().for_each(|c| ag.add_remote(c));
                    agents.insert((H, peer as usize), ag);
                    host.accept(peer);
                }
                HostEvent::Candidates { peer, candidates } => {
                    let ag = agents.get_mut(&(H, peer as usize)).unwrap();
                    candidates.into_iter().for_each(|c| ag.add_remote(c));
                }
                HostEvent::Go { peer } => agents.get_mut(&(H, peer as usize)).unwrap().start(now),
                HostEvent::Connected { peer, path } => host_heard.push((peer, path)),
                other => panic!("host: {other:?}"),
            }
        }
        w.send(&mut si, hc, host.take_outgoing(), now);

        // Lobby traffic, guest side.
        for &i in &joined {
            let g = &mut guests[i - 1];
            for ev in guest_events(g, &mut w, conns[i]) {
                match ev {
                    GuestEvent::Waiting { peer } => assert_eq!(peer as usize, i),
                    GuestEvent::Accepted { candidates, .. } => {
                        let mut ag = Agent::new(Role::Guest, &[socks[i]], IceConfig::default());
                        candidates.into_iter().for_each(|c| ag.add_remote(c));
                        agents.insert((i, H), ag);
                    }
                    GuestEvent::Candidates { candidates } => {
                        let ag = agents.get_mut(&(i, H)).unwrap();
                        candidates.into_iter().for_each(|c| ag.add_remote(c));
                    }
                    GuestEvent::Go => agents.get_mut(&(i, H)).unwrap().start(now),
                    GuestEvent::Helpers { peers } if i == C => {
                        let hs: Vec<Helper> = peers.iter().map(|&id| Helper { id, host_rtt: None }).collect();
                        planner.set_helpers(&hs, now);
                        if c_direct_failed {
                            planner.start(now);
                        }
                    }
                    GuestEvent::Helpers { .. } => {}
                    GuestEvent::RelayAsk { from, candidates } => {
                        assert_eq!((i, from as usize), (A, C));
                        match policy.answer(0, a_direct) {
                            Answer::Accept => {
                                let mut ag = Agent::new(Role::Host, &[socks[A]], IceConfig::default());
                                candidates.into_iter().for_each(|c| ag.add_remote(c));
                                ag.start(now);
                                agents.insert((A, C), ag);
                                g.relay_answer(from, true, "", &cands[A]);
                            }
                            Answer::Refuse(r) => g.relay_answer(from, false, r.as_wire(), &[]),
                        }
                    }
                    GuestEvent::RelayAnswer { helper, accept, reason, candidates } => {
                        assert_eq!((i, helper as usize), (C, A));
                        if accept {
                            let mut ag = Agent::new(Role::Guest, &[socks[C]], IceConfig::default());
                            candidates.into_iter().for_each(|c| ag.add_remote(c));
                            agents.insert((C, A), ag);
                            planner.on_answer(helper, Answer::Accept);
                        } else {
                            planner.on_answer(helper, Answer::Refuse(Refusal::from_wire(&reason)));
                        }
                    }
                    other => panic!("guest {i}: {other:?}"),
                }
            }
        }

        // C's relay choice.
        for act in planner.poll(now) {
            match act {
                relay::Action::Ask { helper } => guests[C - 1].relay_ask(helper, &cands[C]),
                relay::Action::TryReach { helper } => {
                    try_reach_at = now;
                    agents.get_mut(&(C, helper as usize)).unwrap().start(now);
                }
                relay::Action::Chosen { helper } => {
                    chosen = Some(helper);
                    // The first packet for the host goes to the helper, which forwards it.
                    let (local, remote) = agents[&(C, A)].selected().unwrap();
                    sim.send(local, remote, vec![b'F', C as u8]);
                }
                other => panic!("planner: {other:?}"),
            }
        }
        for &i in &joined {
            w.send(&mut si, conns[i], guests[i - 1].take_outgoing(), now);
        }

        // Game traffic.
        let mut keys: Vec<(usize, usize)> = agents.keys().copied().collect();
        keys.sort();
        for (me, other) in keys {
            let ag = agents.get_mut(&(me, other)).unwrap();
            let guest_role = ag.role() == Role::Guest;
            for act in ag.poll(now) {
                match act {
                    Action::SendProbe { from, to, check_id, .. } => {
                        let mut d = vec![b'P', me as u8];
                        d.extend_from_slice(&check_id.to_be_bytes());
                        sim.send(from, to, d);
                    }
                    Action::Selected { local, remote, .. } => {
                        if guest_role {
                            sim.send(local, remote, vec![b'N', me as u8]);
                        }
                        match (me, other) {
                            (A, H) => {
                                a_direct = true;
                                guests[A - 1].connected("direct");
                            }
                            (C, H) => panic!("C reached the host directly through two symmetric NATs"),
                            (C, A) => planner.on_reach(A as u32, Some(now - try_reach_at)),
                            _ => {}
                        }
                    }
                    Action::Keepalive { local, remote } => sim.send(local, remote, vec![b'K', me as u8]),
                    Action::Failed => match (me, other) {
                        (C, H) => {
                            c_direct_failed = true;
                            planner.start(now);
                        }
                        (H, C) => {}
                        (C, A) => planner.on_reach(A as u32, None),
                        pair => panic!("no path for {pair:?}"),
                    },
                    Action::PathLost => panic!("path lost for {:?}", (me, other)),
                    // No NAT kinds given: no birthday punch.
                    Action::OpenSockets { .. } | Action::CloseSockets { .. } => {}
                }
            }
        }
        for p in sim.deliver() {
            let Some(me) = socks.iter().position(|&s| s == p.to) else { continue };
            let from = p.data[1] as usize;
            match p.data[0] {
                b'P' => {
                    if let Some(ag) = agents.get_mut(&(me, from)) {
                        ag.on_probe_received(p.to, p.from, now);
                        let mut d = p.data.clone();
                        d[0] = b'R';
                        d[1] = me as u8;
                        sim.send(p.to, p.from, d);
                    }
                }
                b'R' => {
                    let id = u64::from_be_bytes(p.data[2..10].try_into().unwrap());
                    agents.get_mut(&(me, from)).unwrap().on_probe_result(id, p.to, p.from, now);
                }
                b'N' => agents.get_mut(&(me, from)).unwrap().on_peer_selected(p.to, p.from, now),
                b'K' => agents.get_mut(&(me, from)).unwrap().on_traffic(now),
                // The relay: A forwards C's packet to the host, and the host's answer back.
                b'F' if me == A => {
                    let (local, remote) = agents[&(A, H)].selected().expect("A has its own path");
                    sim.send(local, remote, vec![b'F', A as u8]);
                }
                b'F' if me == H => {
                    let (local, remote) = agents[&(H, A)].selected().unwrap();
                    sim.send(local, remote, vec![b'B', H as u8]);
                }
                b'B' if me == A => {
                    let (local, remote) = agents[&(A, C)].selected().unwrap();
                    sim.send(local, remote, vec![b'B', A as u8]);
                }
                b'B' if me == C => {
                    planner.on_confirmed(chosen.unwrap());
                    confirmed = true;
                    guests[C - 1].connected("relay");
                }
                other => panic!("packet {other} at {me}"),
            }
        }
        now += Duration::from_millis(10);
    }
    assert!(c_direct_failed, "C's direct checks should fail");
    assert!(a_direct, "A should reach the host");
    assert_eq!(chosen, Some(A as u32));
    assert_eq!(planner.current(), Some(A as u32));
    assert!(confirmed, "a round trip C -> A -> host -> A -> C");
    assert!(host_heard.contains(&(A as u32, "direct".into())), "{host_heard:?}");
    assert!(host_heard.contains(&(C as u32, "relay".into())), "{host_heard:?}");
}
