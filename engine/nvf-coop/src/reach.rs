//! How friends reach a host's game with no server in between: the host's router is
//! asked to open the lobby's port (UPnP IGD) and says the host's internet address; when it
//! will not, the address is asked of a public STUN server, and the host is told to open
//! the port by hand (or to use Tailscale or the like, whose address `NVF_COOP_ADDRESS`
//! can put in the code).
//!
//! Everything here blocks (a router search takes up to a few seconds): call it from a
//! thread of its own.

use crate::code::Lobby;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::time::Duration;

/// Overrides the internet address put in the code (a Tailscale or VPN address, or a
/// forwarded port's public address when the router cannot be asked).
pub const ADDRESS_ENV: &str = "NVF_COOP_ADDRESS";
const STUN: &str = "stun.l.google.com:19302";
const SEARCH: Duration = Duration::from_secs(4);

/// How the lobby can be reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reach {
    /// The code's address: the internet one when it is known, else this network's.
    pub lobby: Lobby,
    /// This PC's address on its network (friends there find the host by broadcast).
    pub local: Ipv4Addr,
    /// The router opened the port; friends outside the host's network can come in.
    pub opened: bool,
    /// What the host should know (why friends outside may not get in), if anything.
    pub note: Option<String>,
}

/// Find the host's addresses and open `port` on its router.
pub fn open(port: u16) -> Reach {
    let local = local_ip().unwrap_or(Ipv4Addr::LOCALHOST);
    let reach = |addr: Ipv4Addr, opened, note| Reach { lobby: Lobby::ip(addr, port), local, opened, note };
    if let Some(public) = std::env::var(ADDRESS_ENV).ok().and_then(|a| a.trim().parse::<Ipv4Addr>().ok()) {
        return reach(public, true, None);
    }
    if !crate::prefs::port_mapping() {
        let why = if crate::prefs::connection() == crate::prefs::Connection::LanOnly { "Co-op connection is LAN only" } else { "the router port setting is off" };
        return reach(local, false, Some(format!("Only players on your network can join ({why}).")));
    }
    match open_router(local, port) {
        Ok(public) if carrier_nat(public) => reach(
            local,
            false,
            Some("Your internet provider shares your address (carrier NAT): only friends on your network, or on Tailscale, can join.".into()),
        ),
        Ok(public) => reach(public, true, None),
        Err(why) => {
            let note = format!("Your router did not open the port ({why}): friends outside your network need UDP port {port} forwarded to this PC, or Tailscale.");
            reach(stun().unwrap_or(local), false, Some(note))
        }
    }
}

/// Close the port [`open`] opened (best effort).
pub fn close(port: u16) {
    if let Ok(gw) = igd_next::search_gateway(options()) {
        let _ = gw.remove_port(igd_next::PortMappingProtocol::UDP, port);
    }
}

fn options() -> igd_next::SearchOptions {
    let mut o = igd_next::SearchOptions::default();
    o.timeout = Some(SEARCH);
    o
}

fn open_router(local: Ipv4Addr, port: u16) -> Result<Ipv4Addr, String> {
    let gw = igd_next::search_gateway(options()).map_err(|_| "no UPnP router found".to_string())?;
    let to = SocketAddr::V4(SocketAddrV4::new(local, port));
    // Lease 0: until removed (close) or the router restarts.
    gw.add_port(igd_next::PortMappingProtocol::UDP, port, to, 0, "NVForever co-op")
        .or_else(|_| gw.add_port(igd_next::PortMappingProtocol::UDP, port, to, 7200, "NVForever co-op"))
        .map_err(|e| format!("the router refused: {e}"))?;
    match gw.get_external_ip() {
        Ok(IpAddr::V4(ip)) => Ok(ip),
        Ok(IpAddr::V6(_)) | Err(_) => stun().ok_or_else(|| "the router did not say its address".to_string()),
    }
}

/// An address the router reports that is not on the internet: the provider's own NAT
/// (100.64.0.0/10) or a private network above this one.
fn carrier_nat(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    (a == 100 && (64..128).contains(&b)) || ip.is_private() || ip.is_unspecified()
}

/// This PC's address on its network: the one a packet to the internet would leave from
/// (`connect` on UDP only picks the route; nothing is sent).
pub fn local_ip() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// This PC's internet address as a STUN server sees it (RFC 5389 Binding request; the
/// XOR-MAPPED-ADDRESS or MAPPED-ADDRESS of the answer).
pub fn stun() -> Option<Ipv4Addr> {
    let server = STUN.to_socket_addrs().ok()?.find(|a| a.is_ipv4())?;
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    const COOKIE: u32 = 0x2112_A442;
    let id: [u8; 12] = crate::net::new_token().to_le_bytes().iter().chain(&[7, 3, 5, 9]).copied().collect::<Vec<_>>().try_into().ok()?;
    let mut req = vec![0x00, 0x01, 0x00, 0x00];
    req.extend_from_slice(&COOKIE.to_be_bytes());
    req.extend_from_slice(&id);
    let mut buf = [0u8; 512];
    for _ in 0..2 {
        s.send_to(&req, server).ok()?;
        let Ok((n, _)) = s.recv_from(&mut buf) else { continue };
        let b = &buf[..n];
        if n < 20 || b[0..2] != [0x01, 0x01] || b[8..20] != id {
            continue;
        }
        let mut at = 20;
        while at + 4 <= n {
            let kind = u16::from_be_bytes([b[at], b[at + 1]]);
            let len = u16::from_be_bytes([b[at + 2], b[at + 3]]) as usize;
            let v = b.get(at + 4..at + 4 + len)?;
            if (kind == 0x0020 || kind == 0x0001) && len >= 8 && v[1] == 0x01 {
                let raw = u32::from_be_bytes([v[4], v[5], v[6], v[7]]);
                return Some(Ipv4Addr::from(if kind == 0x0020 { raw ^ COOKIE } else { raw }));
            }
            at += 4 + len.div_ceil(4) * 4;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carrier_nat_addresses() {
        assert!(carrier_nat("100.72.1.2".parse().unwrap()));
        assert!(carrier_nat("10.0.0.1".parse().unwrap()));
        assert!(!carrier_nat("100.128.0.1".parse().unwrap()));
        assert!(!carrier_nat("203.0.113.7".parse().unwrap()));
    }
}
