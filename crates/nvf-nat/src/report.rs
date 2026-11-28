//! The `co-op connect:` log line (#3795 section 12, "Field"): one per join, with the kind of
//! path chosen, both sides' NAT mapping and the time to connect. Player reports gather it only
//! from players who opted in; the matchmaking service never sees it.
//!
//! The line is stable data: `key=value` words after the prefix, in this order, so a report
//! query can split it. A new key goes on the end.

use std::fmt;
use std::time::Duration;

use crate::candidate::{is_global_v6, is_lan, Candidate, Kind};
use crate::stun::{Gathered, Mapping};

/// The kind of path a join ended up on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// A LAN address (same network).
    Lan,
    /// A global IPv6 address, no NAT in the way.
    Ipv6,
    /// A port the router opened (UPnP, NAT-PMP, PCP), or a machine with a public address.
    Open,
    /// A STUN-seen or peer-learnt address: a punched hole.
    Punched,
    /// Through a relay a player hosts (`nvf-relay`).
    HostedRelay,
    /// Through another guest (section 8).
    PeerRelay,
    /// No path: the checks failed and no relay helped.
    None,
}

impl Path {
    /// The path a chosen remote candidate stands for.
    pub fn of(c: Candidate) -> Path {
        let ip = c.addr.ip();
        match c.kind {
            Kind::Relayed => Path::HostedRelay,
            Kind::Mapped => Path::Open,
            Kind::ServerReflexive | Kind::PeerReflexive => Path::Punched,
            Kind::Host => match ip {
                std::net::IpAddr::V6(v6) if is_global_v6(v6) => Path::Ipv6,
                _ if is_lan(ip) => Path::Lan,
                _ => Path::Open,
            },
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Path::Lan => "lan",
            Path::Ipv6 => "ipv6",
            Path::Open => "open",
            Path::Punched => "punched",
            Path::HostedRelay => "hosted-relay",
            Path::PeerRelay => "peer-relay",
            Path::None => "none",
        }
    }
}

/// A side's NAT, as its STUN gather saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nat {
    /// One public port for every destination.
    Easy,
    /// A new public port per destination (symmetric).
    Hard,
    /// One STUN answer only, or the other side's was not passed on.
    Unknown,
    /// No STUN answer at all: UDP is blocked on the way out (or both servers are down).
    NoUdp,
}

impl Nat {
    pub fn from_gather(g: &Gathered) -> Nat {
        if g.answers.is_empty() {
            return Nat::NoUdp;
        }
        match g.mapping {
            Mapping::EndpointIndependent => Nat::Easy,
            Mapping::EndpointDependent => Nat::Hard,
            Mapping::Unknown => Nat::Unknown,
        }
    }

    /// The other side's NAT, from the candidates the service passed on: two or more
    /// server-reflexive ports on one address is a hard NAT (as `ice`'s birthday punch reads
    /// it), exactly one an easy one; none says nothing.
    pub fn from_candidates(cands: &[Candidate]) -> Nat {
        let srflx: Vec<_> = cands.iter().filter(|c| c.kind == Kind::ServerReflexive).map(|c| c.addr).collect();
        if srflx.iter().any(|a| srflx.iter().filter(|b| b.ip() == a.ip() && b.port() != a.port()).count() >= 1) {
            Nat::Hard
        } else if srflx.is_empty() {
            Nat::Unknown
        } else {
            Nat::Easy
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Nat::Easy => "easy",
            Nat::Hard => "hard",
            Nat::Unknown => "unknown",
            Nat::NoUdp => "no-udp",
        }
    }
}

/// One join's outcome. `Display` writes the log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connect {
    /// True on the host's side.
    pub host: bool,
    pub path: Path,
    pub nat: Nat,
    pub peer_nat: Nat,
    /// From the service's "go" (or the first probe) to the chosen path, or to giving up.
    pub elapsed: Duration,
}

impl fmt::Display for Connect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "co-op connect: side={} path={} nat={} peer_nat={} ms={}",
            if self.host { "host" } else { "guest" },
            self.path.word(),
            self.nat.word(),
            self.peer_nat.word(),
            self.elapsed.as_millis()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn c(kind: Kind, a: &str) -> Candidate {
        Candidate::new(kind, a.parse().unwrap())
    }

    #[test]
    fn paths() {
        assert_eq!(Path::of(c(Kind::Host, "192.168.1.2:1")), Path::Lan);
        assert_eq!(Path::of(c(Kind::Host, "[2001:db8::1]:1")), Path::Ipv6);
        assert_eq!(Path::of(c(Kind::Host, "203.0.113.4:1")), Path::Open);
        assert_eq!(Path::of(c(Kind::Mapped, "203.0.113.4:1")), Path::Open);
        assert_eq!(Path::of(c(Kind::ServerReflexive, "203.0.113.4:1")), Path::Punched);
        assert_eq!(Path::of(c(Kind::PeerReflexive, "203.0.113.4:2")), Path::Punched);
        assert_eq!(Path::of(c(Kind::Relayed, "198.51.100.9:47961")), Path::HostedRelay);
    }

    #[test]
    fn nat_from_gather() {
        let s: SocketAddr = "198.51.100.1:3478".parse().unwrap();
        let seen: SocketAddr = "203.0.113.1:40000".parse().unwrap();
        let g = |answers: Vec<(SocketAddr, SocketAddr)>, mapping| Gathered { answers, mapping };
        assert_eq!(Nat::from_gather(&g(vec![], Mapping::Unknown)), Nat::NoUdp);
        assert_eq!(Nat::from_gather(&g(vec![(s, seen)], Mapping::Unknown)), Nat::Unknown);
        assert_eq!(Nat::from_gather(&g(vec![(s, seen), (s, seen)], Mapping::EndpointIndependent)), Nat::Easy);
        assert_eq!(Nat::from_gather(&g(vec![(s, seen), (s, seen)], Mapping::EndpointDependent)), Nat::Hard);
    }

    #[test]
    fn nat_from_candidates() {
        let host = c(Kind::Host, "192.168.1.2:47960");
        assert_eq!(Nat::from_candidates(&[host]), Nat::Unknown);
        assert_eq!(Nat::from_candidates(&[host, c(Kind::ServerReflexive, "203.0.113.4:47960")]), Nat::Easy);
        let hard = [host, c(Kind::ServerReflexive, "203.0.113.4:1797"), c(Kind::ServerReflexive, "203.0.113.4:55757")];
        assert_eq!(Nat::from_candidates(&hard), Nat::Hard);
        // Two addresses, one port each (IPv4 and IPv6, say): easy.
        assert_eq!(Nat::from_candidates(&[c(Kind::ServerReflexive, "203.0.113.4:1"), c(Kind::ServerReflexive, "[2001:db8::1]:1")]), Nat::Easy);
    }

    #[test]
    fn line() {
        let l = Connect { host: false, path: Path::Punched, nat: Nat::Easy, peer_nat: Nat::Hard, elapsed: Duration::from_millis(830) };
        assert_eq!(l.to_string(), "co-op connect: side=guest path=punched nat=easy peer_nat=hard ms=830");
        let l = Connect { host: true, path: Path::None, nat: Nat::NoUdp, peer_nat: Nat::Unknown, elapsed: Duration::from_secs(10) };
        assert_eq!(l.to_string(), "co-op connect: side=host path=none nat=no-udp peer_nat=unknown ms=10000");
    }
}
