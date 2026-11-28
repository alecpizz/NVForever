//! The player's co-op connection settings (the settings page's Co-op rows,
//! docs/design/coop-networking.md §10), set by the game and read where the network code
//! decides. An environment variable set to `0` (or `off`) still turns a feature off for a
//! run whatever the setting says (tests and the smoke runner use them).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;

/// How this game connects to other players: the Co-op connection row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    /// Online codes, router port mapping, and a relay through another player when needed.
    Automatic,
    /// As Automatic, but never through another player: a join that needs a relay fails.
    DirectOnly,
    /// Players on this network only: no matchmaking service, no router port, no relay.
    LanOnly,
}

impl Connection {
    pub const ALL: [Connection; 3] = [Connection::Automatic, Connection::DirectOnly, Connection::LanOnly];

    /// The row's choice, as the page shows it and the INI keeps it.
    pub fn name(self) -> &'static str {
        match self {
            Connection::Automatic => "Automatic",
            Connection::DirectOnly => "Direct only",
            Connection::LanOnly => "LAN only",
        }
    }

    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name().eq_ignore_ascii_case(name.trim()))
    }
}

static CONNECTION: AtomicU8 = AtomicU8::new(0);
static PORT_MAPPING: AtomicBool = AtomicBool::new(true);
static RELAY_FOR_OTHERS: AtomicBool = AtomicBool::new(true);
static ASK_BEFORE_JOIN: AtomicBool = AtomicBool::new(true);
static HOSTED_RELAY: Mutex<Option<SocketAddr>> = Mutex::new(None);

/// The relay this player hosts or uses (`[Coop] Relay=host:port` in NVForever.ini,
/// docs/coop-self-hosting.md): offered to the other side as a last-resort path.
pub fn hosted_relay() -> Option<SocketAddr> {
    *HOSTED_RELAY.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_hosted_relay(relay: Option<SocketAddr>) {
    *HOSTED_RELAY.lock().unwrap_or_else(|e| e.into_inner()) = relay;
}

fn env_off(key: &str) -> bool {
    std::env::var(key).is_ok_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off"))
}

pub fn connection() -> Connection {
    Connection::ALL[CONNECTION.load(Ordering::Relaxed) as usize % Connection::ALL.len()]
}

pub fn set_connection(c: Connection) {
    CONNECTION.store(Connection::ALL.iter().position(|x| *x == c).unwrap_or(0) as u8, Ordering::Relaxed);
}

/// Ask the router to open the game's port (UPnP, NAT-PMP, PCP): the setting, not LAN only,
/// and not turned off by `NVF_COOP_PORTMAP=0`.
pub fn port_mapping() -> bool {
    PORT_MAPPING.load(Ordering::Relaxed) && connection() != Connection::LanOnly && !env_off(crate::portmap::ENV)
}

pub fn set_port_mapping(on: bool) {
    PORT_MAPPING.store(on, Ordering::Relaxed);
}

/// Relay for another player that cannot reach the host: the setting, and not
/// `NVF_COOP_RELAY=0`.
pub fn relay_for_others() -> bool {
    RELAY_FOR_OTHERS.load(Ordering::Relaxed) && !env_off(crate::online::RELAY_ENV)
}

pub fn set_relay_for_others(on: bool) {
    RELAY_FOR_OTHERS.store(on, Ordering::Relaxed);
}

/// Ask the host before a player joins by online code: the setting, and not
/// `NVF_COOP_ASK=0`.
pub fn ask_before_join() -> bool {
    ASK_BEFORE_JOIN.load(Ordering::Relaxed) && !env_off(crate::online::ASK_ENV)
}

pub fn set_ask_before_join(on: bool) {
    ASK_BEFORE_JOIN.store(on, Ordering::Relaxed);
}

/// Join a host through another player when no direct path is found.
pub fn relay_through_others() -> bool {
    connection() == Connection::Automatic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_names_read_back() {
        for c in Connection::ALL {
            assert_eq!(Connection::by_name(c.name()), Some(c));
        }
        assert_eq!(Connection::by_name("lan ONLY"), Some(Connection::LanOnly));
        assert_eq!(Connection::by_name("Relay"), None);
    }
}
