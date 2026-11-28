//! Lobby codes and the other join forms (#3795 section 5).
//!
//! An online code is 6 characters of Crockford base32 (30 bits), shown as `K7Q-M2X`. The
//! service makes it at random; it holds no address. Reading forgives what Crockford's alphabet
//! forgives (case, `I`/`L` for 1, `O` for 0) and ignores spaces and dashes [documented:
//! Douglas Crockford, "Base32", the decode rules].

use std::fmt;
use std::net::SocketAddr;

/// Crockford's alphabet: digits and letters without I, L, O and U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The characters in an online code.
pub const LEN: usize = 6;

/// An online lobby code: 30 random bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Code(u32);

impl Code {
    /// The low 30 bits of `random` (the caller's random source).
    pub fn from_random(random: u32) -> Self {
        Code(random & ((1 << 30) - 1))
    }

    /// Reads a code as a player typed it. `None` unless it has exactly 6 code characters.
    pub fn parse(text: &str) -> Option<Self> {
        let mut v = 0u32;
        let mut n = 0;
        for ch in text.chars() {
            if ch == '-' || ch.is_whitespace() {
                continue;
            }
            v = (v << 5) | digit(ch)?;
            n += 1;
            if n > LEN {
                return None;
            }
        }
        (n == LEN).then_some(Code(v))
    }

    /// The six characters, no dash (the form in URLs).
    pub fn compact(&self) -> String {
        (0..LEN).rev().map(|i| ALPHABET[((self.0 >> (5 * i)) & 31) as usize] as char).collect()
    }
}

/// `K7Q-M2X`.
impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.compact();
        write!(f, "{}-{}", &s[..3], &s[3..])
    }
}

fn digit(ch: char) -> Option<u32> {
    let up = match ch.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        c => c,
    };
    ALPHABET.iter().position(|&a| a as char == up).map(|p| p as u32)
}

/// What a player typed in the join box, told apart by its shape (#3795 section 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinForm {
    /// Ask the matchmaking service.
    Online(Code),
    /// A LAN code (`L-…`): `nvf-coop` reads its body; this crate only recognises it.
    Lan(String),
    /// A typed address, for VPN and forwarded-port players.
    Direct(SocketAddr),
}

impl JoinForm {
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim();
        if let Ok(addr) = t.parse::<SocketAddr>() {
            return Some(JoinForm::Direct(addr));
        }
        if let Some(rest) = t.strip_prefix("L-").or_else(|| t.strip_prefix("l-")) {
            return (!rest.is_empty()).then(|| JoinForm::Lan(rest.to_owned()));
        }
        Code::parse(t).map(JoinForm::Online)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_display() {
        for r in [0u32, 1, 0x3fff_ffff, 0x1234_5678, 0xffff_ffff] {
            let c = Code::from_random(r);
            assert_eq!(Code::parse(&c.to_string()), Some(c));
            assert_eq!(Code::parse(&c.compact()), Some(c));
        }
        assert_eq!(Code::from_random(0).to_string(), "000-000");
        assert_eq!(Code::from_random(0x3fff_ffff).to_string(), "ZZZ-ZZZ");
    }

    #[test]
    fn forgiving_reading() {
        let c = Code::parse("K7Q-M2X").unwrap();
        assert_eq!(Code::parse("k7q m2x"), Some(c));
        assert_eq!(Code::parse("OIL-000"), Code::parse("011-000"));
        assert_eq!(Code::parse("K7Q-M2"), None, "too short");
        assert_eq!(Code::parse("K7Q-M2XX"), None, "too long");
        assert_eq!(Code::parse("K7Q-M2U"), None, "U is not in the alphabet");
    }

    #[test]
    fn join_forms() {
        assert_eq!(JoinForm::parse("203.0.113.5:47960"), Some(JoinForm::Direct("203.0.113.5:47960".parse().unwrap())));
        assert_eq!(JoinForm::parse("[2001:db8::5]:47960"), Some(JoinForm::Direct("[2001:db8::5]:47960".parse().unwrap())));
        assert_eq!(JoinForm::parse(" L-K7QM-2X9D "), Some(JoinForm::Lan("K7QM-2X9D".into())));
        assert_eq!(JoinForm::parse("K7Q-M2X"), Some(JoinForm::Online(Code::parse("K7QM2X").unwrap())));
        assert_eq!(JoinForm::parse("hello"), None);
    }
}
