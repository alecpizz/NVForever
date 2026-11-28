//! Co-op for NVForever: a host and up to five guests, each playing their own game and
//! seeing the others' players (docs/design/coop-networking.md). The protocol layer:
//! joining and hosting a lobby, every player's state reaching the others, and the host's
//! world reaching its guests (the host runs the world: its NPCs, hits, doors, the clock).
//!
//! `net` is the session (the messages and who talks to whom), `rel` the reliable ordered
//! stream inside it, `repl` the kinds of item that stream carries, `guard` the host's
//! checks on everything a guest sends, `world` the host's checks on the world's messages,
//! `progress` shared progress on the reliable stream (quest rewards, stage offers, kill
//! credit, the host's quest, level and `story`, with the host's checks on what a guest
//! sends of it), and `seam` what carries the packets ([`seam::Transport`]) and who may
//! join.
//!
//! Under the seam, the internet transport: `stack` (the transport the session is given),
//! `sockets` (IPv4 and IPv6), `noise` (every packet sealed, a key per player), `online`
//! and `signal_io` (online codes through the matchmaking service, and the checks that find
//! a path between two games), `portmap` and `reach` (the router asked to let friends in),
//! `code` (address codes for LAN play) and `prefs` (the player's Co-op settings).

pub mod code;
pub mod guard;
pub mod net;
pub mod noise;
pub mod online;
pub mod portmap;
pub mod prefs;
pub mod progress;
pub mod reach;
pub mod rel;
pub mod repl;
pub mod seam;
pub mod signal_io;
pub mod sockets;
pub mod stack;
pub mod story;
pub mod world;
