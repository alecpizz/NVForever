//! The matchmaking service's wire format (#3795 section 6), as JSON. The prose version, for the
//! Worker, is `docs/design/coop-signal-wire.md`; keep the two the same.
//!
//! REST: `POST /v1/lobbies` ([`CreateLobby`] → [`LobbyCreated`]), `GET /v1/lobbies/{code}`
//! ([`LobbyInfo`]), and for the lobby browser (wire 2, `docs/design/coop-lobby-browser.md`)
//! `GET /v1/browse` ([`BrowseQuery`] → [`BrowseAnswer`]) and `POST /v1/lobbies/{code}/report`. WebSocket: `GET /v1/lobbies/{code}/ws`, then text frames of [`ToService`]
//! and [`FromService`], each one JSON object with a `"type"`.

use std::net::SocketAddr;

use nvf_nat::candidate::{Candidate, Kind};
use serde::{Deserialize, Serialize};

/// The version of this wire format, separate from the game protocol. Sent in [`CreateLobby`]
/// and in the WebSocket's first message, so the service can turn old clients away. Version 2
/// adds the lobby browser (listings, passwords, browse, reports).
pub const WIRE_VERSION: u32 = 2;

/// The oldest wire version the service still takes: a version-1 game keeps working, and its
/// lobbies stay private.
pub const MIN_WIRE_VERSION: u32 = 1;

/// Whether the service takes wire version `v`.
pub fn wire_supported(v: u32) -> bool {
    (MIN_WIRE_VERSION..=WIRE_VERSION).contains(&v)
}

/// A lobby password as the wire carries it: lowercase hex of SHA-256 over
/// `"lobby-password v1\0"`, the compact code, `"\0"` and the password's UTF-8
/// (`docs/design/coop-lobby-browser.md` section 4) [design]. The code salts it, so one password
/// hashes differently in each lobby; the service never sees the password itself.
pub fn password_hash(code: &crate::code::Code, password: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"lobby-password v1\0");
    h.update(code.compact().as_bytes());
    h.update(b"\0");
    h.update(password.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The first 8 bytes of a host key, as 16 hex digits: what favourites keep, so a favourite host
/// is found again under a new code.
pub fn key_fingerprint(key: &str) -> String {
    key.chars().take(16).collect()
}

/// A host's listing in the lobby browser (wire 2). Texts are cut to their lengths by the
/// service: title 40 characters, place 32.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    /// Listed in the browser. Private (false) is the default (owner decision 2026-11-25).
    #[serde(default)]
    pub public: bool,
    /// Empty: the service shows "<host name>'s game".
    #[serde(default)]
    pub title: String,
    /// The host's location as the game names it.
    #[serde(default)]
    pub place: String,
    #[serde(default)]
    pub friendly_fire: bool,
    /// How many plugins are loaded (not their names).
    #[serde(default)]
    pub plugins: u32,
    /// A password will be set: joins are declined until the host's first `password`.
    #[serde(default)]
    pub locked: bool,
}

/// A player's public key (Curve25519, #3795 section 4) as 64 lowercase hex digits.
pub type KeyHex = String;

/// Whether `s` is 64 lowercase hex digits.
pub fn is_key(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A key as [`KeyHex`].
pub fn key_hex(key: &[u8; 32]) -> KeyHex {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

/// A candidate on the wire: `{"kind": "host" | "mapped" | "srflx" | "prflx" | "relay", "addr": "ip:port"}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCandidate {
    pub kind: String,
    pub addr: String,
}

impl From<Candidate> for WireCandidate {
    fn from(c: Candidate) -> Self {
        let kind = match c.kind {
            Kind::Host => "host",
            Kind::Mapped => "mapped",
            Kind::ServerReflexive => "srflx",
            Kind::PeerReflexive => "prflx",
            Kind::Relayed => "relay",
        };
        WireCandidate { kind: kind.into(), addr: c.addr.to_string() }
    }
}

impl WireCandidate {
    /// The candidate, or `None` for an unknown kind or a bad address (skipped, not fatal: a
    /// newer service may add kinds) [design].
    pub fn parse(&self) -> Option<Candidate> {
        let kind = match self.kind.as_str() {
            "host" => Kind::Host,
            "mapped" => Kind::Mapped,
            "srflx" => Kind::ServerReflexive,
            "prflx" => Kind::PeerReflexive,
            "relay" => Kind::Relayed,
            _ => return None,
        };
        let addr: SocketAddr = self.addr.parse().ok()?;
        Some(Candidate::new(kind, addr))
    }
}

pub fn to_wire(cands: &[Candidate]) -> Vec<WireCandidate> {
    cands.iter().map(|&c| c.into()).collect()
}

pub fn from_wire(cands: &[WireCandidate]) -> Vec<Candidate> {
    cands.iter().filter_map(WireCandidate::parse).collect()
}

/// `POST /v1/lobbies`, the host's request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateLobby {
    pub wire: u32,
    /// The game protocol (`net.rs`'s version): a guest must match it.
    pub protocol: u32,
    /// The game's build, e.g. "0.8.1".
    pub build: String,
    /// A hash of the load order, so a guest is told before connecting that it differs.
    pub load_order_hash: String,
    pub max_players: u8,
    pub host_key: KeyHex,
    /// The host's character name, shown to guests.
    pub name: String,
    /// Wire 2: the lobby browser's listing. None: private, no password (wire 1's lobbies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing: Option<Listing>,
}

/// The answer to [`CreateLobby`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobbyCreated {
    /// The code, compact form (6 characters).
    pub code: String,
    /// Proves the WebSocket that connects as host is this host. Never shown to players.
    pub host_secret: String,
    /// STUN servers to ask, `host:port`.
    pub stun: Vec<String>,
    /// Unix seconds after which an unclaimed lobby (no host WebSocket) is gone.
    pub expires: u64,
}

/// `GET /v1/lobbies/{code}`: what a guest checks before it connects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobbyInfo {
    pub protocol: u32,
    pub build: String,
    pub load_order_hash: String,
    pub players: u8,
    pub max_players: u8,
    pub full: bool,
    /// Wire 2: the lobby asks for a password; the game asks the player before joining.
    #[serde(default)]
    pub locked: bool,
}

/// `GET /v1/browse`'s query (wire 2). Every field narrows the list; `None`/false: no filter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrowseQuery {
    /// Only lobbies of this game protocol.
    pub protocol: Option<u32>,
    /// Only lobbies with this load-order hash.
    pub load_order: Option<String>,
    /// Search in title, host name and place, case-insensitive, at most 32 characters.
    pub q: Option<String>,
    /// Hide lobbies with only the host (`empty=0`).
    pub hide_empty: bool,
    /// Hide full lobbies (`full=0`).
    pub hide_full: bool,
    /// The next page, from the last answer's `cursor`.
    pub cursor: Option<String>,
}

fn query_escape(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn query_unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl BrowseQuery {
    /// The query string, with its `?` (empty when there is no filter).
    pub fn to_query(&self) -> String {
        let mut parts = Vec::new();
        if let Some(p) = self.protocol {
            parts.push(format!("protocol={p}"));
        }
        if let Some(l) = &self.load_order {
            parts.push(format!("load_order={}", query_escape(l)));
        }
        if let Some(q) = &self.q {
            parts.push(format!("q={}", query_escape(q)));
        }
        if self.hide_empty {
            parts.push("empty=0".into());
        }
        if self.hide_full {
            parts.push("full=0".into());
        }
        if let Some(c) = &self.cursor {
            parts.push(format!("cursor={}", query_escape(c)));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }

    /// The query from a query string (with or without its `?`); unknown keys are skipped.
    pub fn parse(query: &str) -> BrowseQuery {
        let mut q = BrowseQuery::default();
        for pair in query.trim_start_matches('?').split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = query_unescape(v);
            match k {
                "protocol" => q.protocol = v.parse().ok(),
                "load_order" if !v.is_empty() => q.load_order = Some(v),
                "q" if !v.is_empty() => q.q = Some(v.chars().take(32).collect()),
                "empty" => q.hide_empty = v == "0",
                "full" => q.hide_full = v == "0",
                "cursor" if !v.is_empty() => q.cursor = Some(v),
                _ => {}
            }
        }
        q
    }
}

/// One public lobby in the browser (wire 2). No addresses: `km` is the distance between the
/// data centres' view of the host and of the browsing player, rounded to 250 km, or None when
/// the service cannot tell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseRow {
    pub code: String,
    pub title: String,
    pub host_name: String,
    /// [`key_fingerprint`] of the host's key, for favourites.
    pub host_fp: String,
    pub place: String,
    pub players: u8,
    pub max_players: u8,
    pub locked: bool,
    pub friendly_fire: bool,
    pub plugins: u32,
    /// "continent/country" (e.g. "OC/AU"), or empty.
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub km: Option<u32>,
    /// Seconds since the lobby was made.
    pub age: u64,
    pub protocol: u32,
    pub build: String,
    pub load_order_hash: String,
}

/// The answer to [`BrowseQuery`]: at most 100 rows a page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseAnswer {
    pub lobbies: Vec<BrowseRow>,
    #[serde(default)]
    pub cursor: Option<String>,
    pub total: u32,
}

/// `POST /v1/lobbies/{code}/report`'s body; `{}` is fine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    #[serde(default)]
    pub reason: String,
}

/// An error body, for any non-2xx answer: `{"error": "not_found", "message": "…"}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
    pub message: String,
}

/// A guest's number in a lobby, given by the service. The host is not numbered.
pub type PeerNo = u32;

/// WebSocket messages from a game to the service.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToService {
    /// The host's first message (again after a reconnect within the grace).
    Host { wire: u32, host_secret: String },
    /// A guest's first message. `password`: [`password_hash`] of what the player typed, for a
    /// locked lobby (wire 2).
    Join {
        wire: u32,
        name: String,
        key: KeyHex,
        candidates: Vec<WireCandidate>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<String>,
    },
    /// Host (wire 2): the listing changed (place, title, visibility). `public: false` takes the
    /// lobby off the browser at once.
    Listing { public: bool, title: String, place: String, friendly_fire: bool },
    /// Host (wire 2): the lobby's password, as [`password_hash`]; empty removes it.
    Password { hash: String },
    /// Host: let this guest in, with the host's candidates so far.
    Accept { peer: PeerNo, candidates: Vec<WireCandidate> },
    /// Host: turn this guest away.
    Decline { peer: PeerNo, reason: String },
    /// More candidates as they are found (trickle). The host names the guest; a guest's go to
    /// the host.
    Candidates {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peer: Option<PeerNo>,
        candidates: Vec<WireCandidate>,
    },
    /// The checks found a path ("direct", "lan", "relay"): for the host's lobby screen only.
    Connected {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peer: Option<PeerNo>,
        path: String,
    },
    /// Guest: the path was lost; gather and check again.
    Reconnect,
    /// Guest C, which cannot reach the host: ask guest `helper` to relay, with C's candidates
    /// (#3795 section 8). Goes guest to guest; the host is not involved.
    RelayAsk { helper: PeerNo, candidates: Vec<WireCandidate> },
    /// Guest A's answer to a relay request from `for`. With `accept`, A's candidates, for C's
    /// checks against A; otherwise `reason` ("off", "busy", "no_direct_path").
    RelayAnswer {
        #[serde(rename = "for")]
        for_peer: PeerNo,
        accept: bool,
        #[serde(default)]
        reason: String,
        #[serde(default)]
        candidates: Vec<WireCandidate>,
    },
    /// Leaving. From the host, it closes the lobby.
    Bye,
}

/// WebSocket messages from the service to a game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromService {
    /// To the host: you are hosting this code. `peers` lists guests still in the lobby (after a
    /// reconnect within the grace).
    Hosting { code: String, peers: Vec<PeerNo> },
    /// To a guest: the host has been asked; wait.
    Waiting { peer: PeerNo },
    /// To the host: a guest asks to join.
    Join { peer: PeerNo, name: String, key: KeyHex, candidates: Vec<WireCandidate> },
    /// To a guest: the host accepted.
    Accepted { host_key: KeyHex, host_name: String, candidates: Vec<WireCandidate> },
    /// To a guest: the host declined.
    Declined { reason: String },
    /// The other side's new candidates. To the host, `peer` says whose.
    Candidates {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peer: Option<PeerNo>,
        candidates: Vec<WireCandidate>,
    },
    /// Start the checks now, sent to both sides at once (#3795 section 7.2).
    Go {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peer: Option<PeerNo>,
    },
    /// To the host: this guest lost its path and is checking again (a `go` follows).
    Reconnect { peer: PeerNo },
    /// To the host: this guest's path is up.
    Connected { peer: PeerNo, path: String },
    /// To the host: this guest left or dropped.
    Left { peer: PeerNo },
    /// To every accepted guest, when it changes: the other guests with a direct or LAN path
    /// to the host (from their `connected`), who could relay.
    Helpers { peers: Vec<PeerNo> },
    /// To guest A: guest `from` asks A to relay for it.
    RelayAsk { from: PeerNo, candidates: Vec<WireCandidate> },
    /// To guest C: `helper`'s answer.
    RelayAnswer {
        helper: PeerNo,
        accept: bool,
        #[serde(default)]
        reason: String,
        #[serde(default)]
        candidates: Vec<WireCandidate>,
    },
    /// The lobby is over ("host_left", "expired"); the socket closes after this.
    Closed { reason: String },
    /// A message was refused ("bad_secret", "bad_key", "full", "wire_version", "bad_message",
    /// "unknown_peer", "rate_limited"). After the first message's error the socket closes.
    Error { error: String, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_as_documented() {
        // The examples in docs/design/coop-signal-wire.md.
        let j = serde_json::to_string(&ToService::Accept {
            peer: 1,
            candidates: vec![WireCandidate { kind: "srflx".into(), addr: "203.0.113.5:47960".into() }],
        })
        .unwrap();
        assert_eq!(j, r#"{"type":"accept","peer":1,"candidates":[{"kind":"srflx","addr":"203.0.113.5:47960"}]}"#);
        assert_eq!(serde_json::to_string(&ToService::Reconnect).unwrap(), r#"{"type":"reconnect"}"#);
        assert_eq!(serde_json::to_string(&FromService::Go { peer: None }).unwrap(), r#"{"type":"go"}"#);
        let back: FromService = serde_json::from_str(r#"{"type":"go","peer":3}"#).unwrap();
        assert_eq!(back, FromService::Go { peer: Some(3) });
        let c: ToService = serde_json::from_str(r#"{"type":"candidates","candidates":[]}"#).unwrap();
        assert_eq!(c, ToService::Candidates { peer: None, candidates: vec![] });
    }

    #[test]
    fn candidates_round_trip() {
        let cands = vec![
            Candidate::new(Kind::Host, "192.168.1.2:47960".parse().unwrap()),
            Candidate::new(Kind::Host, "[2001:db8::2]:47960".parse().unwrap()),
            Candidate::new(Kind::ServerReflexive, "203.0.113.5:40000".parse().unwrap()),
            Candidate::new(Kind::Relayed, "198.51.100.9:47961".parse().unwrap()),
        ];
        assert_eq!(from_wire(&to_wire(&cands)), cands);
        let odd = [WireCandidate { kind: "turn".into(), addr: "1.2.3.4:5".into() }, WireCandidate { kind: "host".into(), addr: "nope".into() }];
        assert!(from_wire(&odd).is_empty());
    }

    #[test]
    fn keys() {
        let k = key_hex(&[0xab; 32]);
        assert!(is_key(&k));
        assert!(!is_key(&k.to_uppercase()));
        assert!(!is_key(&k[1..]));
    }

    #[test]
    fn password_hash_vector() {
        // Python: hashlib.sha256(b"lobby-password v1\x00K7QM2X\x00hunter2").hexdigest()
        let code = crate::code::Code::parse("k7q-m2x").unwrap();
        assert_eq!(password_hash(&code, "hunter2"), "49ce5c210b7ef838407a97bfdc3908a71bcde231f8eb62f935691da069e60ad2");
        assert_eq!(key_fingerprint(&"ab".repeat(32)), "abababababababab");
    }

    #[test]
    fn browse_query_round_trip() {
        let q = BrowseQuery {
            protocol: Some(139),
            load_order: Some("abc".into()),
            q: Some("the strip & co".into()),
            hide_empty: true,
            hide_full: false,
            cursor: Some("100".into()),
        };
        let s = q.to_query();
        assert_eq!(s, "?protocol=139&load_order=abc&q=the%20strip%20%26%20co&empty=0&cursor=100");
        assert_eq!(BrowseQuery::parse(&s), q);
        assert_eq!(BrowseQuery::default().to_query(), "");
        assert_eq!(BrowseQuery::parse("q=a+b&full=0&junk=1").q.as_deref(), Some("a b"));
        assert!(wire_supported(1) && wire_supported(2) && !wire_supported(3) && !wire_supported(0));
    }
}
