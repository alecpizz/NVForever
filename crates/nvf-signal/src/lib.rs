//! Co-op matchmaking, the game's side (#3795 sections 5 and 6): how two games that only share a
//! lobby code learn each other's keys and addresses. The service only introduces players; no
//! game traffic goes through it.
//!
//! - [`code`]: lobby codes (`K7Q-M2X`) and the other join forms (LAN, a typed address).
//! - [`wire`]: the REST bodies and WebSocket messages, as JSON. `docs/design/coop-signal-wire.md`
//!   is the same contract in prose, for the Worker in its own repository.
//! - [`client`]: the host's and the guest's state machines, sans-IO: the caller owns the HTTP
//!   and WebSocket connections and passes the text in and out.
//! - [`standin`]: the service itself, in-process, for tests (CI needs no Node) and for anyone
//!   who wants to run one without Cloudflare.

pub mod client;
pub mod code;
pub mod standin;
pub mod wire;

/// The matchmaking service, unless `NVF_COOP_SIGNAL` or `[Coop] SignalUrl` names another
/// [design: #3795 section 6].
pub const DEFAULT_SERVICE: &str = "https://coop.nv-forever.com";
