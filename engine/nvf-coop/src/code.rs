//! Lobby codes: where the host is, written for a person to read out.
//!
//! These are the address codes: the host's own address (its internet one when it is known,
//! else its local network's; [`Host::Ip`]), for a host the matchmaking service cannot
//! reach and for LAN play. Online codes (6 characters) are the service's
//! (`nvf_signal::code`, docs/design/coop-networking.md §5). An address is 4 bytes, and a
//! check byte against typos follows: 5 bytes in Crockford's base32 make 8 characters, two
//! groups of four
//! (`K7QM-2X9D`). A host not on [`DEFAULT_PORT`] adds the port: 7 bytes, 12 characters
//! (`K7QM-2X9D-8RTC`). Friends on the host's own network need nothing more: a guest also
//! asks its network for the code's host ([`Lobby::candidates`]), which answers only to
//! its own code ([`Lobby::key`]). Reading forgives case, spaces, dashes and the
//! look-alikes O (0), I and L (1).

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// The port a code without one means.
pub const DEFAULT_PORT: u16 = 47_960;
/// Characters in a code on the default port, and with its own port.
const SHORT: usize = 8;
const LONG: usize = 12;

/// The sizes of home network a guest broadcasts to, as prefix lengths: /24 most often; a
/// mesh router's is often /22 (eero: 192.168.4.0–192.168.7.255, where a host at
/// 192.168.4.26 never hears a guest at 192.168.5.x broadcast to 192.168.5.255).
pub const HOME_NETWORKS: [u32; 6] = [24, 23, 22, 21, 20, 16];

/// Who the host is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    /// Where the host can be reached: its internet address, or its local network's.
    Ip(Ipv4Addr),
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Host::Ip(a) => write!(f, "{a}"),
        }
    }
}

/// What a code says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lobby {
    pub host: Host,
    /// The host's UDP port.
    pub port: u16,
}

impl Lobby {
    pub fn ip(addr: Ipv4Addr, port: u16) -> Self {
        Lobby { host: Host::Ip(addr), port }
    }

    /// The 4 bytes a code carries.
    fn body(&self) -> [u8; 4] {
        match self.host {
            Host::Ip(a) => a.octets(),
        }
    }

    /// Where a guest tries the host, in turn: the code's address, this PC (a host in
    /// another window), and the guest's own network by broadcast (`local` is the guest's
    /// address there). Its network's size is not known here, so the broadcast goes to the
    /// broadcast address of each common size ([`HOME_NETWORKS`]): the network's own
    /// reaches the host, the others reach nobody. These go before the all-ones one, which
    /// Windows sends out of a single adapter only.
    pub fn candidates(&self, local: Option<Ipv4Addr>) -> Vec<SocketAddr> {
        let mut out: Vec<SocketAddr> = Vec::new();
        let Host::Ip(a) = self.host;
        let mut ips = vec![a, Ipv4Addr::LOCALHOST];
        if let Some(l) = local.filter(|l| l.is_private()) {
            for prefix in HOME_NETWORKS {
                ips.push(Ipv4Addr::from(u32::from(l) | (u32::MAX >> prefix)));
            }
        }
        ips.push(Ipv4Addr::BROADCAST);
        for ip in ips {
            let a = SocketAddr::V4(SocketAddrV4::new(ip, self.port));
            if !out.contains(&a) {
                out.push(a);
            }
        }
        out
    }

    /// The number a guest's Hello carries so that only this code's host answers it (never
    /// 0, which is a guest that came by address, not by code).
    pub fn key(&self) -> u32 {
        let mut b = self.body().to_vec();
        b.extend_from_slice(&self.port.to_be_bytes());
        fnv(&b).max(1)
    }

    pub fn encode(&self) -> String {
        let mut b = self.body().to_vec();
        if self.port != DEFAULT_PORT {
            b.extend_from_slice(&self.port.to_be_bytes());
        }
        let sum = check(&b);
        b.push(sum);
        // 5 bits to a character, the last character's spare bits zero.
        let mut chars = String::new();
        let (mut acc, mut bits) = (0u32, 0);
        for byte in b {
            acc = acc << 8 | byte as u32;
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                chars.push(ALPHABET[(acc >> bits & 31) as usize] as char);
            }
        }
        if bits > 0 {
            chars.push(ALPHABET[(acc << (5 - bits) & 31) as usize] as char);
        }
        let groups: Vec<&str> = (0..chars.len()).step_by(4).map(|i| &chars[i..(i + 4).min(chars.len())]).collect();
        groups.join("-")
    }

    /// Read a code as typed; None for one that is mistyped (or not a code).
    pub fn decode(text: &str) -> Option<Lobby> {
        let mut values = Vec::with_capacity(LONG);
        for c in text.chars().filter(|c| !matches!(c, '-' | ' ')) {
            let c = match c.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            values.push(ALPHABET.iter().position(|&a| a as char == c)? as u32);
        }
        let bytes = match values.len() {
            SHORT => 5,
            LONG => 7,
            _ => return None,
        };
        let mut b = Vec::with_capacity(bytes);
        let (mut acc, mut bits) = (0u32, 0);
        for v in values {
            acc = acc << 5 | v;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                b.push((acc >> bits & 0xFF) as u8);
            }
        }
        b.truncate(bytes);
        if b.len() != bytes {
            return None;
        }
        let (body, sum) = b.split_at(bytes - 1);
        let four = [body[0], body[1], body[2], body[3]];
        if sum[0] != check(body) {
            return None;
        }
        let host = Host::Ip(Ipv4Addr::from(four));
        let port = if bytes == 7 { u16::from_be_bytes([body[4], body[5]]) } else { DEFAULT_PORT };
        let Host::Ip(a) = host;
        let real = !a.is_unspecified();
        (port != 0 && real).then_some(Lobby { host, port })
    }
}

fn fnv(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811C_9DC5u32, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// A check byte that changes with any one character (an FNV-1a hash folded to a byte).
fn check(bytes: &[u8]) -> u8 {
    let h = fnv(bytes);
    (h ^ h >> 8 ^ h >> 16 ^ h >> 24) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lobby() -> Lobby {
        Lobby::ip(Ipv4Addr::new(203, 0, 113, 7), DEFAULT_PORT)
    }

    #[test]
    fn round_trips_and_reads_forgivingly() {
        let code = lobby().encode();
        assert_eq!(code.len(), SHORT + 1, "{code}");
        assert_eq!(code.matches('-').count(), 1);
        assert_eq!(Lobby::decode(&code), Some(lobby()));
        let typed = code.to_lowercase().replace('-', " ").replace('0', "o").replace('1', "l");
        assert_eq!(Lobby::decode(&typed), Some(lobby()), "{typed}");
        let other = Lobby { port: 50_000, ..lobby() };
        let long = other.encode();
        assert_eq!(long.len(), LONG + 2, "{long}");
        assert_eq!(Lobby::decode(&long), Some(other));
    }

    #[test]
    fn a_typo_is_caught() {
        for l in [lobby(), Lobby { port: 50_000, ..lobby() }] {
            let code = l.encode();
            let n = code.chars().filter(|c| *c != '-').count();
            let mut caught = 0;
            for (i, c) in code.char_indices().filter(|(_, c)| *c != '-') {
                let other = if c == 'A' { 'B' } else { 'A' };
                let typo = format!("{}{other}{}", &code[..i], &code[i + 1..]);
                caught += (Lobby::decode(&typo) != Some(l)) as usize;
            }
            // The last character's spare bits carry nothing, so a change there can still read.
            assert!(caught >= n - 1, "{caught} of {n} typos caught in {code}");
        }
        assert_eq!(Lobby::decode("hello"), None);
        assert_eq!(Lobby::decode(&lobby().encode()[..7]), None);
    }

    #[test]
    fn candidates_try_the_code_then_this_pc_then_the_network() {
        let c = lobby().candidates(Some(Ipv4Addr::new(192, 168, 1, 20)));
        let ips: Vec<String> = c.iter().map(|a| a.ip().to_string()).collect();
        assert_eq!(ips, ["203.0.113.7", "127.0.0.1", "192.168.1.255", "192.168.1.255", "192.168.3.255", "192.168.7.255", "192.168.15.255", "192.168.255.255", "255.255.255.255"][..].iter().fold(Vec::<&str>::new(), |mut v, ip| {
            if !v.contains(ip) {
                v.push(ip);
            }
            v
        }));
        // A mesh router's /22: a guest at 192.168.5.40 reaches a host at 192.168.4.26.
        let mesh = lobby().candidates(Some(Ipv4Addr::new(192, 168, 5, 40)));
        assert!(mesh.iter().any(|a| a.ip().to_string() == "192.168.7.255"));
        assert!(c.iter().all(|a| a.port() == DEFAULT_PORT));
        assert_eq!(lobby().candidates(None).len(), 3);
    }

    #[test]
    fn keys_tell_lobbies_apart() {
        let other = Lobby::ip(Ipv4Addr::new(203, 0, 113, 8), DEFAULT_PORT);
        assert_ne!(lobby().key(), other.key());
        assert_ne!(lobby().key(), 0);
    }
}
