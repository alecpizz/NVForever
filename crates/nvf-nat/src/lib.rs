//! NAT traversal for co-op, the "Getting through NATs" part of the co-op networking design
//! (issue #3795, sections 7.1 and 7.2). Formats and rules are written up in plain language in
//! `docs/spec-notes/nat-traversal.md`.
//!
//! Everything here is sans-IO: no sockets, no threads, no clock of its own and no async
//! runtime. The caller owns the UDP sockets (`nvf-coop`'s transport), passes in what arrives
//! and the time, and sends what these state machines ask for. That keeps the logic testable
//! on a fake clock, and `natsim` runs whole connections through simulated NATs in `cargo test`.
//!
//! - [`stun`]: STUN Binding requests and responses (RFC 8489), and [`stun::Gather`], which asks
//!   two servers for this socket's public address and tells whether the NAT keeps one mapping
//!   for every destination.
//! - [`portmap`]: NAT-PMP (RFC 6886) and PCP (RFC 6887) MAP requests, and [`portmap::Mapper`],
//!   which asks the router for a port (PCP first, NAT-PMP when the router only speaks that).
//!   UPnP IGD stays with the `igd-next` crate in the caller.
//! - [`relay`]: when a guest cannot reach the host, which other guest relays for it (section 8).
//! - [`hosted`]: a relay a player hosts (`nvf-relay`, section 9): its wire format, the relay's
//!   state machine and the game's side of a session.
//! - [`report`]: the `co-op connect:` log line, one per join (section 12, "Field").
//! - [`candidate`]: the addresses a game may be reached at, and how they rank.
//! - [`ice`]: [`ice::Agent`], the connectivity checks (hole punching) between two games, which
//!   picks the path both use.
//! - `natsim` (feature `natsim`, and in this crate's tests): a userspace router with the usual
//!   NAT kinds, for tests.

pub mod candidate;
pub mod hosted;
pub mod ice;
pub mod portmap;
pub mod relay;
pub mod report;
pub mod stun;

#[cfg(any(test, feature = "natsim"))]
pub mod natsim;
