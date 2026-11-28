//! Test aids for the online path in one game, so a smoke run needs no second game and no
//! internet (docs/design/coop-networking.md §12, docs/design/coop-lobby-browser.md §9):
//!
//! - `NVF_COOP_SIGNAL=local`: the matchmaking service runs inside this game
//!   (`nvf_signal_server`, the stand-in with the Worker's rules) on a loopback port, and the
//!   game uses it as if `NVF_COOP_SIGNAL=http://127.0.0.1:<port>` had been given.
//! - `NVF_COOP_TEST_GUEST=<name>`: once the host's lobby has its online code, a guest with no
//!   window joins it from this process (with the host's password, if any) through the
//!   service, its checks and its encrypted session, as a second game would, with a key of
//!   its own.
//!
//! Log: `co-op: test aid NVF_COOP_SIGNAL=local: the matchmaking service runs in this game at
//! <url>`, `co-op test guest: <name> welcomed as player <n>`, `co-op test guest: <name>
//! failed to join: <why>`.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use nvf_coop::net::Session;
use nvf_coop::noise::Identity;
use nvf_coop::online::OnlineEvent;
use nvf_coop::stack::{self, Stack};

/// The test guest's name; unset: none.
const GUEST_ENV: &str = "NVF_COOP_TEST_GUEST";
/// How long the test guest tries to get in; once in, it stays for the rest of the run.
const GUEST_TRIES_FOR: Duration = Duration::from_secs(60);

/// `NVF_COOP_SIGNAL=local`: start the service here and point the game at it. Called before
/// anything reads the service.
pub(super) fn start_local_service() {
    let local = std::env::var(nvf_coop::signal_io::SERVICE_ENV).is_ok_and(|v| v.trim().eq_ignore_ascii_case("local"));
    if !local {
        return;
    }
    // No STUN servers: loopback needs none, and a smoke run does not reach the internet.
    let cfg = nvf_signal_server::deployment_config(nvf_coop::net::VERSION as u32, Vec::new());
    match nvf_signal_server::Server::start(std::net::SocketAddr::from(([127, 0, 0, 1], 0)), cfg) {
        Ok(server) => {
            let url = format!("http://{}", server.addr);
            info!("co-op: test aid {}=local: the matchmaking service runs in this game at {url}", nvf_coop::signal_io::SERVICE_ENV);
            std::env::set_var(nvf_coop::signal_io::SERVICE_ENV, &url);
            // It serves for the rest of the run (dropping it stops it).
            std::mem::forget(server);
        }
        Err(e) => warn!("co-op: test aid {}=local: cannot listen on loopback: {e}", nvf_coop::signal_io::SERVICE_ENV),
    }
}

/// `NVF_COOP_TEST_GUEST`: the host's lobby has `code`; a guest joins it on a thread.
pub(super) fn join_as_test_guest(code: &str, password: &str, load_order: &str) {
    let Some(name) = std::env::var(GUEST_ENV).ok().filter(|n| !n.trim().is_empty()) else { return };
    let Some(service) = nvf_coop::signal_io::service() else {
        warn!("co-op test guest: no matchmaking service");
        return;
    };
    let Some(code) = nvf_signal::code::Code::parse(code) else { return };
    let (password, load_order) = (password.to_string(), load_order.to_string());
    let spawned = std::thread::Builder::new().name("coop-test-guest".into()).spawn(move || {
        let me = Identity::generate();
        let token = stack::token_of(&me.public());
        let (st, control) = match Stack::join_online(&service, code, &name, &load_order, &password, me) {
            Ok(s) => s,
            Err(e) => {
                warn!("co-op test guest: cannot open a socket: {e}");
                return;
            }
        };
        let admission = st.admission();
        let mut guest = Session::join_on(Box::new(st), Box::new(admission), &[stack::ONLINE_HOST], 0, &name, token);
        let start = Instant::now();
        let mut welcomed: Option<Instant> = None;
        loop {
            let now = Instant::now();
            guest.tick(now);
            guest.take_events();
            for e in control.events() {
                if let OnlineEvent::Failed(why) = e {
                    match welcomed {
                        // The host's lobby closing at the end of the run.
                        Some(_) => info!("co-op test guest: {name} is out: {why}"),
                        None => warn!("co-op test guest: {name} failed to join: {why}"),
                    }
                    return;
                }
            }
            match (welcomed, guest.id()) {
                (None, Some(id)) => {
                    info!("co-op test guest: {name} welcomed as player {}", id + 1);
                    welcomed = Some(now);
                }
                (None, None) if now - start >= GUEST_TRIES_FOR => {
                    warn!("co-op test guest: {name} was not let in within {} s", GUEST_TRIES_FOR.as_secs());
                    return;
                }
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    if let Err(e) = spawned {
        warn!("co-op test guest: cannot start its thread: {e}");
    }
}
