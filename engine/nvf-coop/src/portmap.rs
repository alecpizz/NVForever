//! Asking this player's router to open the game's IPv4 port for an online lobby
//! (docs/design/coop-networking.md §7.1 and phase 4): every player, host or guest, so the
//! other side has a "mapped" candidate that needs no hole punched on this side.
//!
//! PCP, then NAT-PMP ([`nvf_nat::portmap::Mapper`], sent from the game's own socket, which
//! is the one mapped), then UPnP IGD (`igd-next`, blocking, on a thread of its own) when the
//! router answers neither. A mapping is renewed from half its lifetime and removed on
//! leaving. `NVF_COOP_PORTMAP=0` turns all of it off.
//!
//! A router that hands out an address that is not on the internet (carrier NAT,
//! 100.64.0.0/10, or a private network above this one) gives no candidate: nobody outside
//! could use it.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use nvf_nat::portmap::{Failure, Mapper, Protocol, SERVER_PORT};

/// Turns router port mapping off (`0`).
pub const ENV: &str = "NVF_COOP_PORTMAP";
/// A UPnP lease (seconds), renewed from half of it. Some routers refuse lease 0 (forever),
/// and one that outlives a crashed game should not stay open long.
const UPNP_LEASE: u32 = 7200;
const UPNP_SEARCH: Duration = Duration::from_secs(3);
const UPNP_NAME: &str = "NVForever co-op";

/// What [`PortMap`] has to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapEvent {
    /// The router opened the port at this public address: a mapped candidate.
    Mapped(SocketAddr),
    /// A line for the log.
    Note(String),
}

struct Upnp {
    gateway: igd_next::Gateway,
    external_port: u16,
}

enum State {
    /// PCP or NAT-PMP under way, or a mapping it made (kept for renewing and releasing).
    Asking(Mapper),
    /// The UPnP thread is searching.
    Searching(Receiver<Result<(Upnp, Ipv4Addr), String>>),
    /// UPnP opened the port; renew at the instant.
    Upnp(Upnp, Instant),
    /// Nothing more to do.
    Idle,
}

/// One socket's port on this player's router.
pub struct PortMap {
    gateway: Ipv4Addr,
    client: Ipv4Addr,
    port: u16,
    nonce: [u8; 12],
    state: State,
    /// The address last given as a candidate, so a renewal at the same one says nothing.
    told: Option<SocketAddr>,
    events: Vec<MapEvent>,
}

impl PortMap {
    /// Start asking for `local`'s port (an IPv4 LAN address of this PC with the game
    /// socket's port), when mapping is on and the router's address is known.
    pub fn start(local: SocketAddr, now: Instant) -> Option<Self> {
        if !crate::prefs::port_mapping() {
            return None;
        }
        let IpAddr::V4(client) = local.ip() else { return None };
        let gateway = default_gateway()?;
        Some(Self::with_gateway(gateway, client, local.port(), now))
    }

    /// As [`PortMap::start`], with the router at `gateway`.
    pub fn with_gateway(gateway: Ipv4Addr, client: Ipv4Addr, port: u16, now: Instant) -> Self {
        let nonce = nonce();
        let mapper = Mapper::new(IpAddr::V4(gateway), IpAddr::V4(client), port, nonce, now);
        PortMap { gateway, client, port, nonce, state: State::Asking(mapper), told: None, events: Vec::new() }
    }

    pub fn take_events(&mut self) -> Vec<MapEvent> {
        std::mem::take(&mut self.events)
    }

    /// The router's PCP/NAT-PMP address: packets from it go to [`PortMap::on_packet`].
    pub fn gateway(&self) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(self.gateway), SERVER_PORT)
    }

    /// Once a tick: the packets for the router, to send from the mapped socket.
    pub fn tick(&mut self, now: Instant) -> Vec<(SocketAddr, Vec<u8>)> {
        let mut send = Vec::new();
        let to = self.gateway();
        match &mut self.state {
            State::Asking(m) => {
                if let Some(b) = m.poll(now) {
                    send.push((to, b));
                }
                match m.outcome() {
                    Some(Ok(mapping)) if now >= mapping.renew_at() => {
                        // Renew: the same nonce asks for the same mapping (RFC 6887 §11.2.1)
                        // [documented].
                        self.state = State::Asking(Mapper::new(IpAddr::V4(self.gateway), IpAddr::V4(self.client), self.port, self.nonce, now));
                    }
                    Some(Err(why)) => {
                        let said = match why {
                            Failure::NoAnswer => "no PCP or NAT-PMP answer".to_string(),
                            Failure::Refused { protocol, result } => format!("{protocol:?} refused (result {result})"),
                        };
                        self.events.push(MapEvent::Note(format!("router port mapping: {said}; trying UPnP")));
                        self.state = State::Searching(upnp_thread(self.client, self.port));
                    }
                    _ => {}
                }
            }
            State::Searching(rx) => match rx.try_recv() {
                Ok(Ok((u, ip))) => {
                    let at = SocketAddr::new(IpAddr::V4(ip), u.external_port);
                    self.tell(at, "UPnP");
                    self.state = State::Upnp(u, now + Duration::from_secs(UPNP_LEASE as u64 / 2));
                }
                Ok(Err(why)) => {
                    self.events.push(MapEvent::Note(format!("router port mapping: UPnP {why}; none on this router")));
                    self.state = State::Idle;
                }
                Err(TryRecvError::Disconnected) => self.state = State::Idle,
                Err(TryRecvError::Empty) => {}
            },
            State::Upnp(u, renew) if now >= *renew => {
                *renew = now + Duration::from_secs(UPNP_LEASE as u64 / 2);
                let (gw, port, client) = (u.gateway.clone(), u.external_port, self.client);
                let local = SocketAddr::V4(SocketAddrV4::new(client, self.port));
                std::thread::Builder::new()
                    .name("coop-upnp".into())
                    .spawn(move || {
                        let _ = gw.add_port(igd_next::PortMappingProtocol::UDP, port, local, UPNP_LEASE, UPNP_NAME);
                    })
                    .ok();
            }
            State::Upnp(..) | State::Idle => {}
        }
        send
    }

    /// A datagram from `from`: whether it was the router's answer (and so not the game's).
    pub fn on_packet(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) -> bool {
        if from.port() != SERVER_PORT {
            return false;
        }
        let State::Asking(m) = &mut self.state else { return false };
        if !m.on_packet(from, bytes, now) {
            return false;
        }
        if let Some(Ok(mapping)) = m.outcome() {
            let how = match mapping.protocol {
                Protocol::Pcp => "PCP",
                Protocol::NatPmp => "NAT-PMP",
                Protocol::Upnp => "UPnP",
            };
            self.tell(mapping.external, how);
        }
        true
    }

    fn tell(&mut self, at: SocketAddr, how: &str) {
        if self.told == Some(at) {
            return;
        }
        if !reachable(at.ip()) {
            self.events.push(MapEvent::Note(format!("router port mapping ({how}): {at} is not on the internet (carrier NAT?); not offered")));
            return;
        }
        self.told = Some(at);
        self.events.push(MapEvent::Note(format!("router port mapping ({how}): port {} open at {at}", self.port)));
        self.events.push(MapEvent::Mapped(at));
    }

    /// Leaving: the PCP/NAT-PMP packet that removes the mapping (to send from the mapped
    /// socket), and a UPnP mapping removed (one request to the router, on this thread: the
    /// game may be about to exit).
    pub fn release(&mut self) -> Option<(SocketAddr, Vec<u8>)> {
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Asking(m) => m.release().map(|b| (self.gateway(), b)),
            State::Upnp(u, _) => {
                let _ = u.gateway.remove_port(igd_next::PortMappingProtocol::UDP, u.external_port);
                None
            }
            State::Searching(_) | State::Idle => None,
        }
    }
}

/// An address someone on the internet could send to: not private, loopback, link-local,
/// unspecified or carrier NAT (100.64.0.0/10).
fn reachable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(v6) => nvf_nat::candidate::is_global_v6(v6),
    }
}

/// The default route's IPv4 gateway: this PC's router.
fn default_gateway() -> Option<Ipv4Addr> {
    netdev::get_default_gateway().ok()?.ipv4.into_iter().find(|ip| !ip.is_unspecified())
}

/// UPnP on a thread: the router found, `port` opened (the same external port, else any),
/// and its public address.
fn upnp_thread(client: Ipv4Addr, port: u16) -> Receiver<Result<(Upnp, Ipv4Addr), String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("coop-upnp".into())
        .spawn(move || {
            let _ = tx.send(upnp(client, port));
        })
        .ok();
    rx
}

fn upnp(client: Ipv4Addr, port: u16) -> Result<(Upnp, Ipv4Addr), String> {
    let mut o = igd_next::SearchOptions::default();
    o.timeout = Some(UPNP_SEARCH);
    let gateway = igd_next::search_gateway(o).map_err(|_| "found no router".to_string())?;
    let local = SocketAddr::V4(SocketAddrV4::new(client, port));
    let proto = igd_next::PortMappingProtocol::UDP;
    let external_port = match gateway.add_port(proto, port, local, UPNP_LEASE, UPNP_NAME) {
        Ok(()) => port,
        Err(_) => gateway.add_any_port(proto, local, UPNP_LEASE, UPNP_NAME).map_err(|e| format!("was refused ({e})"))?,
    };
    let ip = match gateway.get_external_ip() {
        Ok(IpAddr::V4(ip)) => ip,
        _ => {
            let _ = gateway.remove_port(proto, external_port);
            return Err("did not say the router's address".into());
        }
    };
    Ok((Upnp { gateway, external_port }, ip))
}

fn nonce() -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&crate::net::new_token().to_le_bytes());
    n[8..].copy_from_slice(&(crate::net::new_token() as u32).to_le_bytes());
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PCP MAP response (RFC 6887 §7.2, §11.1): result 0, `lifetime`, the request's nonce
    /// and port, the external address.
    fn pcp_answer(request: &[u8], lifetime: u32, external: SocketAddrV4) -> Vec<u8> {
        let mut b = vec![0u8; 60];
        b[0] = 2;
        b[1] = 0x81;
        b[4..8].copy_from_slice(&lifetime.to_be_bytes());
        b[24..36].copy_from_slice(&request[24..36]);
        b[36] = 17;
        b[40..42].copy_from_slice(&request[40..42]);
        b[42..44].copy_from_slice(&external.port().to_be_bytes());
        b[44..60].copy_from_slice(&external.ip().to_ipv6_mapped().octets());
        b
    }

    /// Against this PC's own router (run by hand: `--ignored --nocapture`): asks for a
    /// socket's port for up to 10 s, prints what happened, then removes the mapping.
    #[test]
    #[ignore]
    fn live_router() {
        let sock = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        sock.set_nonblocking(true).unwrap();
        let ip = crate::reach::local_ip().expect("a LAN address");
        let local = SocketAddr::new(IpAddr::V4(ip), sock.local_addr().unwrap().port());
        println!("gateway {:?}, local {local}", default_gateway());
        let mut pm = PortMap::start(local, Instant::now()).expect("a gateway");
        let end = Instant::now() + Duration::from_secs(10);
        let mut buf = [0u8; 1500];
        let mut mapped = false;
        while Instant::now() < end && !mapped {
            for (to, b) in pm.tick(Instant::now()) {
                sock.send_to(&b, to).unwrap();
            }
            while let Ok((n, from)) = sock.recv_from(&mut buf) {
                pm.on_packet(from, &buf[..n], Instant::now());
            }
            for e in pm.take_events() {
                println!("{e:?}");
                mapped |= matches!(e, MapEvent::Mapped(_));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Some((to, b)) = pm.release() {
            sock.send_to(&b, to).unwrap();
        }
        println!("mapped: {mapped}");
    }

    const GW: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);

    #[test]
    fn pcp_mapping_is_offered_then_renewed() {
        let now = Instant::now();
        let mut pm = PortMap::with_gateway(GW, Ipv4Addr::new(192, 168, 1, 20), 47960, now);
        let send = pm.tick(now);
        assert_eq!(send.len(), 1);
        assert_eq!(send[0].0, SocketAddr::from((GW, 5351)));
        let at = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 9), 40000);
        // Someone else's packet at the PCP port is not the router's answer.
        assert!(!pm.on_packet(SocketAddr::from((Ipv4Addr::new(192, 168, 1, 7), 5351)), &pcp_answer(&send[0].1, 600, at), now));
        assert!(pm.on_packet(pm.gateway(), &pcp_answer(&send[0].1, 600, at), now));
        assert!(pm.take_events().contains(&MapEvent::Mapped(at.into())));
        // Half the lifetime on, the same nonce asks again; the same answer says nothing new.
        let later = now + Duration::from_secs(300);
        pm.tick(later);
        let again = pm.tick(later);
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].1[24..36], send[0].1[24..36], "a renewal keeps the nonce");
        assert!(pm.on_packet(pm.gateway(), &pcp_answer(&again[0].1, 600, at), later));
        assert!(!pm.take_events().iter().any(|e| matches!(e, MapEvent::Mapped(_))));
        assert!(pm.release().is_some(), "leaving removes the mapping");
    }

    #[test]
    fn a_carrier_nat_address_is_not_offered() {
        let now = Instant::now();
        let mut pm = PortMap::with_gateway(GW, Ipv4Addr::new(192, 168, 1, 20), 47960, now);
        let send = pm.tick(now);
        assert!(pm.on_packet(pm.gateway(), &pcp_answer(&send[0].1, 600, SocketAddrV4::new(Ipv4Addr::new(100, 70, 1, 2), 47960)), now));
        assert!(!pm.take_events().iter().any(|e| matches!(e, MapEvent::Mapped(_))));
    }

    #[test]
    fn game_packets_are_not_the_routers() {
        let now = Instant::now();
        let mut pm = PortMap::with_gateway(GW, Ipv4Addr::new(192, 168, 1, 20), 47960, now);
        pm.tick(now);
        assert!(!pm.on_packet(SocketAddr::from((GW, 47960)), &[0xE0; 40], now));
    }
}
