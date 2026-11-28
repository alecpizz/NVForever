//! Candidates: the addresses a game may be reached at (#3795 section 7.1), and the order in
//! which pairs of them are tried (section 7.2).

use std::net::{IpAddr, Ipv6Addr, SocketAddr};

/// Where a candidate address came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// An address of the machine itself: a LAN IPv4 address, a global IPv6 address, or a
    /// public address when the machine has one.
    Host,
    /// A port the router opened on request (UPnP IGD, NAT-PMP or PCP).
    Mapped,
    /// The public address a STUN server saw ("server reflexive").
    ServerReflexive,
    /// An address the other side's packets turned out to come from, learnt during the checks
    /// ("peer reflexive"). A symmetric NAT shows up this way.
    PeerReflexive,
    /// A relay a player hosts (`nvf-relay`, [`crate::hosted`]): packets to this address go
    /// wrapped, and the relay forwards them to the other side. Last resort.
    Relayed,
}

/// One address the other side may try.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Candidate {
    pub kind: Kind,
    pub addr: SocketAddr,
}

impl Candidate {
    /// A candidate, with an IPv4-mapped IPv6 address taken as the IPv4 address it maps.
    pub fn new(kind: Kind, addr: SocketAddr) -> Self {
        Candidate { kind, addr: canonical(addr) }
    }
}

/// An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) as plain IPv4; other addresses as they are.
pub fn canonical(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V6(v6) => match v6.ip().to_ipv4_mapped() {
            Some(v4) => SocketAddr::new(IpAddr::V4(v4), v6.port()),
            None => addr,
        },
        SocketAddr::V4(_) => addr,
    }
}

/// A private, link-local or loopback address: one only reachable on the same network.
/// IPv4: 10/8, 172.16/12, 192.168/16 (RFC 1918), 169.254/16, 127/8. IPv6: fc00::/7 (unique
/// local), fe80::/10 (link-local), ::1. The shared address space 100.64/10 (carrier NAT) is
/// not counted: two homes behind one carrier NAT are not on one LAN.
pub fn is_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// A global unicast IPv6 address (2000::/3), the kind that may be reachable with no NAT.
pub fn is_global_v6(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xe000) == 0x2000
}

/// The class of a pair through a hosted relay: the worst.
pub const RELAYED_CLASS: u8 = 4;

/// The rank of a pair, lower first (#3795 section 7.2): IPv6 direct, then LAN, then a mapped
/// port, then a STUN or peer-learnt address. A public host address (a machine with no NAT)
/// ranks with the mapped ports: like them, it needs nothing punched on its side. A hosted
/// relay comes last of all.
pub fn pair_class(remote: Candidate) -> u8 {
    let ip = remote.addr.ip();
    match remote.kind {
        Kind::Host => match ip {
            IpAddr::V6(v6) if is_global_v6(v6) => 0,
            _ if is_lan(ip) => 1,
            _ => 2,
        },
        Kind::Mapped => 2,
        Kind::ServerReflexive | Kind::PeerReflexive => 3,
        Kind::Relayed => RELAYED_CLASS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sa(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn mapped_v6_becomes_v4() {
        assert_eq!(canonical(sa("[::ffff:192.0.2.7]:5")), sa("192.0.2.7:5"));
        assert_eq!(canonical(sa("[2001:db8::1]:5")), sa("[2001:db8::1]:5"));
        assert_eq!(Candidate::new(Kind::Host, sa("[::ffff:10.0.0.2]:9")).addr, sa("10.0.0.2:9"));
    }

    #[test]
    fn lan_ranges() {
        for lan in ["10.1.2.3", "172.16.0.1", "172.31.255.1", "192.168.1.1", "169.254.3.3", "127.0.0.1", "fd00::1", "fe80::1", "::1"] {
            assert!(is_lan(lan.parse().unwrap()), "{lan}");
        }
        for wan in ["8.8.8.8", "172.32.0.1", "100.64.0.1", "203.0.113.9", "2001:db8::1"] {
            assert!(!is_lan(wan.parse().unwrap()), "{wan}");
        }
    }

    #[test]
    fn class_order() {
        let v6 = pair_class(Candidate::new(Kind::Host, sa("[2001:db8::1]:1")));
        let lan = pair_class(Candidate::new(Kind::Host, sa("192.168.0.2:1")));
        let mapped = pair_class(Candidate::new(Kind::Mapped, sa("203.0.113.1:1")));
        let srflx = pair_class(Candidate::new(Kind::ServerReflexive, sa("203.0.113.1:2")));
        let relayed = pair_class(Candidate::new(Kind::Relayed, sa("198.51.100.9:47961")));
        assert!(v6 < lan && lan < mapped && mapped < srflx && srflx < relayed);
        assert_eq!(pair_class(Candidate::new(Kind::Host, sa("[fd00::1]:1"))), lan);
    }
}
