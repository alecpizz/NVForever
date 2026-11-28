//! The host's and the guest's side of the lobby WebSocket, sans-IO. The caller opens the
//! socket (`tungstenite`, blocking, on the network thread), sends [`HostClient::take_outgoing`]
//! / [`GuestClient::take_outgoing`] as text frames, and hands every text frame it gets to
//! `on_text`. The events say what to do: add candidates to the `nvf_nat::ice::Agent`, start it
//! on `Go`, and so on.

use nvf_nat::candidate::Candidate;

use crate::wire::{from_wire, to_wire, FromService, KeyHex, PeerNo, ToService, WIRE_VERSION};

/// A text frame that is not a message of this wire format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadMessage(pub String);

fn parse(text: &str) -> Result<FromService, BadMessage> {
    serde_json::from_str(text).map_err(|e| BadMessage(e.to_string()))
}

fn text(msg: &ToService) -> String {
    serde_json::to_string(msg).expect("wire messages always serialise")
}

/// What the host learns from the service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEvent {
    /// The service took the host's socket: this code is live.
    Hosting { code: String, peers: Vec<PeerNo> },
    /// A guest asks to join: show the Accept / Decline prompt (or accept at once when join
    /// approval is off), then [`HostClient::accept`] or [`HostClient::decline`].
    JoinRequest { peer: PeerNo, name: String, key: KeyHex, candidates: Vec<Candidate> },
    /// More of a guest's candidates: add them to that guest's agent.
    Candidates { peer: PeerNo, candidates: Vec<Candidate> },
    /// Start (or restart) that guest's checks now.
    Go { peer: PeerNo },
    /// That guest lost its path: gather again, make a new agent; a `Go` follows.
    Reconnect { peer: PeerNo },
    /// That guest's path is up (its report).
    Connected { peer: PeerNo, path: String },
    Left { peer: PeerNo },
    Closed { reason: String },
    Error { error: String, message: String },
}

/// The host's lobby socket.
pub struct HostClient {
    secret: String,
    local: Vec<Candidate>,
    accepted: Vec<PeerNo>,
    out: Vec<String>,
}

impl HostClient {
    /// `host_secret` from [`crate::wire::LobbyCreated`]. Queues the first message.
    pub fn new(host_secret: &str) -> Self {
        let mut c = HostClient { secret: host_secret.to_owned(), local: Vec::new(), accepted: Vec::new(), out: Vec::new() };
        c.hello();
        c
    }

    /// Queues the first message again, after the socket dropped and was reopened (within the
    /// service's grace, the lobby and its guests are still there).
    pub fn hello(&mut self) {
        self.out.push(text(&ToService::Host { wire: WIRE_VERSION, host_secret: self.secret.clone() }));
    }

    pub fn on_text(&mut self, frame: &str) -> Result<Vec<HostEvent>, BadMessage> {
        let ev = match parse(frame)? {
            FromService::Hosting { code, peers } => HostEvent::Hosting { code, peers },
            FromService::Join { peer, name, key, candidates } => {
                HostEvent::JoinRequest { peer, name, key, candidates: from_wire(&candidates) }
            }
            FromService::Candidates { peer: Some(peer), candidates } => {
                HostEvent::Candidates { peer, candidates: from_wire(&candidates) }
            }
            FromService::Go { peer: Some(peer) } => HostEvent::Go { peer },
            FromService::Reconnect { peer } => HostEvent::Reconnect { peer },
            FromService::Connected { peer, path } => HostEvent::Connected { peer, path },
            FromService::Left { peer } => {
                self.accepted.retain(|&p| p != peer);
                HostEvent::Left { peer }
            }
            FromService::Closed { reason } => HostEvent::Closed { reason },
            FromService::Error { error, message } => HostEvent::Error { error, message },
            other => return Err(BadMessage(format!("not for a host: {other:?}"))),
        };
        Ok(vec![ev])
    }

    /// Lets a guest in, with every candidate found so far.
    pub fn accept(&mut self, peer: PeerNo) {
        if !self.accepted.contains(&peer) {
            self.accepted.push(peer);
        }
        self.out.push(text(&ToService::Accept { peer, candidates: to_wire(&self.local) }));
    }

    pub fn decline(&mut self, peer: PeerNo, reason: &str) {
        self.out.push(text(&ToService::Decline { peer, reason: reason.to_owned() }));
    }

    /// A candidate the host just found (STUN, a router port). Sent to every accepted guest at
    /// once, and kept for guests accepted later.
    pub fn add_local(&mut self, c: Candidate) {
        if self.local.contains(&c) {
            return;
        }
        self.local.push(c);
        if !self.accepted.is_empty() {
            self.out.push(text(&ToService::Candidates { peer: None, candidates: to_wire(&[c]) }));
        }
    }

    /// Candidates gathered again for one guest after its `Reconnect`.
    pub fn send_candidates_to(&mut self, peer: PeerNo, cands: &[Candidate]) {
        self.out.push(text(&ToService::Candidates { peer: Some(peer), candidates: to_wire(cands) }));
    }

    /// Closes the lobby.
    /// Wire 2: the browser listing changed (visibility, title, the host's place). The service
    /// passes it on at most every 5 s, so send it whenever it changes.
    pub fn listing(&mut self, public: bool, title: &str, place: &str, friendly_fire: bool) {
        self.out.push(text(&ToService::Listing { public, title: title.to_owned(), place: place.to_owned(), friendly_fire }));
    }

    /// Wire 2: the lobby's password as [`crate::wire::password_hash`]; empty removes it.
    pub fn password(&mut self, hash: &str) {
        self.out.push(text(&ToService::Password { hash: hash.to_owned() }));
    }

    pub fn bye(&mut self) {
        self.out.push(text(&ToService::Bye));
    }

    pub fn take_outgoing(&mut self) -> Vec<String> {
        std::mem::take(&mut self.out)
    }
}

/// What a guest learns from the service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuestEvent {
    /// The host has been asked.
    Waiting { peer: PeerNo },
    /// In: the host's key (for the Noise handshake) and candidates.
    Accepted { host_key: KeyHex, host_name: String, candidates: Vec<Candidate> },
    Declined { reason: String },
    Candidates { candidates: Vec<Candidate> },
    /// Start (or restart) the checks now.
    Go,
    /// The other guests that could relay (they have a direct or LAN path to the host).
    Helpers { peers: Vec<PeerNo> },
    /// Guest `from` cannot reach the host and asks this game to relay. Answer with
    /// [`GuestClient::relay_answer`] (`nvf_nat::relay::Policy` decides); on accepting, run checks
    /// against `candidates`.
    RelayAsk { from: PeerNo, candidates: Vec<Candidate> },
    /// A helper's answer. Accepted: run checks against its `candidates`.
    RelayAnswer { helper: PeerNo, accept: bool, reason: String, candidates: Vec<Candidate> },
    Closed { reason: String },
    Error { error: String, message: String },
}

/// A guest's lobby socket.
pub struct GuestClient {
    accepted: bool,
    held: Vec<Candidate>,
    sent: Vec<Candidate>,
    out: Vec<String>,
}

impl GuestClient {
    /// Queues the join with the candidates found so far (often just the LAN ones).
    pub fn new(name: &str, key: &KeyHex, candidates: &[Candidate]) -> Self {
        Self::joining(name, key, candidates, None)
    }

    /// [`GuestClient::new`] for a locked lobby: `password_hash` is
    /// [`crate::wire::password_hash`] of what the player typed. A wrong one comes back as
    /// [`GuestEvent::Declined`] with reason `bad_password`.
    pub fn joining(name: &str, key: &KeyHex, candidates: &[Candidate], password_hash: Option<&str>) -> Self {
        let join = ToService::Join {
            wire: WIRE_VERSION,
            name: name.to_owned(),
            key: key.clone(),
            candidates: to_wire(candidates),
            password: password_hash.map(str::to_owned),
        };
        GuestClient { accepted: false, held: Vec::new(), sent: candidates.to_vec(), out: vec![text(&join)] }
    }

    pub fn on_text(&mut self, frame: &str) -> Result<Vec<GuestEvent>, BadMessage> {
        let ev = match parse(frame)? {
            FromService::Waiting { peer } => GuestEvent::Waiting { peer },
            FromService::Accepted { host_key, host_name, candidates } => {
                self.accepted = true;
                if !self.held.is_empty() {
                    let held = std::mem::take(&mut self.held);
                    self.out.push(text(&ToService::Candidates { peer: None, candidates: to_wire(&held) }));
                }
                GuestEvent::Accepted { host_key, host_name, candidates: from_wire(&candidates) }
            }
            FromService::Declined { reason } => GuestEvent::Declined { reason },
            FromService::Candidates { candidates, .. } => GuestEvent::Candidates { candidates: from_wire(&candidates) },
            FromService::Go { .. } => GuestEvent::Go,
            FromService::Helpers { peers } => GuestEvent::Helpers { peers },
            FromService::RelayAsk { from, candidates } => GuestEvent::RelayAsk { from, candidates: from_wire(&candidates) },
            FromService::RelayAnswer { helper, accept, reason, candidates } => {
                GuestEvent::RelayAnswer { helper, accept, reason, candidates: from_wire(&candidates) }
            }
            FromService::Closed { reason } => GuestEvent::Closed { reason },
            FromService::Error { error, message } => GuestEvent::Error { error, message },
            other => return Err(BadMessage(format!("not for a guest: {other:?}"))),
        };
        Ok(vec![ev])
    }

    /// A candidate the guest just found. Held until the host accepts (the service passes on a
    /// guest's candidates only after that) [design].
    pub fn add_local(&mut self, c: Candidate) {
        if self.sent.contains(&c) || self.held.contains(&c) {
            return;
        }
        if self.accepted {
            self.sent.push(c);
            self.out.push(text(&ToService::Candidates { peer: None, candidates: to_wire(&[c]) }));
        } else {
            self.held.push(c);
        }
    }

    /// The path is up: tells the host's lobby screen which kind.
    pub fn connected(&mut self, path: &str) {
        self.out.push(text(&ToService::Connected { peer: None, path: path.to_owned() }));
    }

    /// The path was lost: ask for a new round of checks. Gather again and send the new
    /// candidates with [`GuestClient::resend_candidates`].
    pub fn reconnect(&mut self) {
        self.out.push(text(&ToService::Reconnect));
    }

    /// Asks another guest to relay for this one, sending the candidates for its checks.
    pub fn relay_ask(&mut self, helper: PeerNo, cands: &[Candidate]) {
        self.out.push(text(&ToService::RelayAsk { helper, candidates: to_wire(cands) }));
    }

    /// Answers a relay request: accepting sends this game's candidates; refusing sends a
    /// reason ("off", "busy", "no_direct_path").
    pub fn relay_answer(&mut self, for_peer: PeerNo, accept: bool, reason: &str, cands: &[Candidate]) {
        let candidates = if accept { to_wire(cands) } else { Vec::new() };
        self.out.push(text(&ToService::RelayAnswer { for_peer, accept, reason: reason.to_owned(), candidates }));
    }

    pub fn resend_candidates(&mut self, cands: &[Candidate]) {
        self.sent = cands.to_vec();
        self.out.push(text(&ToService::Candidates { peer: None, candidates: to_wire(cands) }));
    }

    pub fn bye(&mut self) {
        self.out.push(text(&ToService::Bye));
    }

    pub fn take_outgoing(&mut self) -> Vec<String> {
        std::mem::take(&mut self.out)
    }
}
