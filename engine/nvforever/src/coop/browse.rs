//! The lobby browser's game side (docs/design/coop-lobby-browser.md): the Browse tab's
//! request on a thread, its rows as the lobby screen shows them (an estimated ping, a reason
//! when this game cannot join), reports, the favourite hosts, and the host's listing.
//!
//! Log: `co-op: the lobby browser lists <n> public lobbies (<total> in all)`, `co-op:
//! reported public lobby <code>`, `co-op: the report of lobby <code> failed: <why>`.

use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;

use bevy::prelude::*;
use fnv_util::words::{tr, trf};
use nvf_play::lobby::{Browse, BrowseQuery, HostOptions, PublicLobby};
use nvf_signal::wire::{self, BrowseAnswer, Listing};

/// The favourite hosts' key fingerprints, one a line, in the engine home.
const FAVOURITES_FILE: &str = "coop-favourites.txt";
/// The host's place is looked up this often (seconds), and the listing sent when it changed.
pub(super) const LISTING_EVERY: f32 = 5.0;
/// The Browse tab asks the service at most this often (seconds): its limit
/// [documented: docs/design/coop-signal-wire.md, Lobby browser].
pub(super) const BROWSE_EVERY: f32 = 2.0;

/// The Browse tab's request, running.
pub(super) type BrowseJob = Mutex<Receiver<Result<BrowseAnswer, String>>>;

/// Ask the service for the public lobbies, on a thread. `None`: there is no service (LAN
/// only, or turned off).
pub(super) fn start(q: &BrowseQuery) -> Option<BrowseJob> {
    let service = nvf_coop::signal_io::service()?;
    let query = wire::BrowseQuery {
        protocol: (!q.all_versions).then_some(nvf_coop::net::VERSION as u32),
        load_order: None,
        q: Some(q.search.clone()).filter(|s| !s.is_empty()),
        hide_empty: false,
        hide_full: false,
        cursor: None,
    };
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("coop-browse".into())
        .spawn(move || {
            let _ = tx.send(nvf_coop::signal_io::browse(&service, &query));
        })
        .ok()?;
    Some(Mutex::new(rx))
}

/// The answer, once it came.
pub(super) fn poll(job: &BrowseJob, load_order: &str, favourites: &[String]) -> Option<Browse> {
    let r = job.lock().ok()?.try_recv().ok()?;
    Some(match r {
        Ok(a) => {
            info!("co-op: the lobby browser lists {} public lobbies ({} in all)", a.lobbies.len(), a.total);
            Browse::Ready(rows(&a, load_order, favourites))
        }
        Err(why) => {
            warn!("co-op: the lobby browser: {why}");
            Browse::Failed(why)
        }
    })
}

/// A report of a public lobby, running.
pub(super) type ReportJob = Mutex<Receiver<Result<(), String>>>;

/// Report lobby `code` to the service (docs/design/coop-lobby-browser.md §6: enough reports
/// from enough addresses hide it from browse), on a thread. `None`: there is no service.
pub(super) fn report(code: &str) -> Option<ReportJob> {
    let service = nvf_coop::signal_io::service()?;
    let code = code.to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("coop-report".into())
        .spawn(move || {
            let r = nvf_coop::signal_io::report(&service, &code);
            match &r {
                Ok(()) => info!("co-op: reported public lobby {code}"),
                Err(why) => warn!("co-op: the report of lobby {code} failed: {why}"),
            }
            let _ = tx.send(r);
        })
        .ok()?;
    Some(Mutex::new(rx))
}

/// The report's answer, once it came.
pub(super) fn report_done(job: &ReportJob) -> Option<Result<(), String>> {
    job.lock().ok()?.try_recv().ok()
}

/// The service's rows as the Browse tab shows them.
pub(super) fn rows(a: &BrowseAnswer, load_order: &str, favourites: &[String]) -> Vec<PublicLobby> {
    a.lobbies
        .iter()
        .map(|r| {
            let cannot = if r.protocol != nvf_coop::net::VERSION as u32 {
                Some(trf("The host runs another version of NVForever ({build}): both games need the same one.", &[("build", &r.build)]))
            } else if !r.load_order_hash.is_empty() && !load_order.is_empty() && r.load_order_hash != load_order {
                Some(tr("The host runs a different load order: both games need the same plugins in the same order.").to_string())
            } else {
                None
            };
            let code = r.code.clone();
            PublicLobby {
                code: if code.len() == 6 { format!("{}-{}", &code[..3], &code[3..]) } else { code },
                title: r.title.clone(),
                host_name: r.host_name.clone(),
                place: r.place.clone(),
                players: r.players,
                max_players: r.max_players,
                locked: r.locked,
                friendly_fire: r.friendly_fire,
                region: r.region.clone(),
                ping_ms: r.km.map(estimated_ping),
                host_fp: r.host_fp.clone(),
                favourite: favourites.contains(&r.host_fp),
                cannot,
            }
        })
        .collect()
}

/// A round trip estimated from the distance (docs/design/coop-lobby-browser.md §5): about 1 ms
/// per 100 km, plus 10 [design: light in fibre covers about 200 km a millisecond one way, and
/// routes run longer than great circles].
pub(super) fn estimated_ping(km: u32) -> u32 {
    km / 100 + 10
}

/// The listing a new lobby is made with: None for a private lobby with no password.
pub(super) fn listing_for(o: &HostOptions, name: &str, place: &str, friendly_fire: bool, plugins: u32) -> Option<Listing> {
    let locked = !o.password.is_empty();
    (o.public || locked).then(|| Listing { public: o.public, title: title(o, name), place: place.to_string(), friendly_fire, plugins, locked })
}

/// Whether the host's lobby has a listing to keep current (listed, or locked).
pub(super) fn listed(o: &HostOptions) -> bool {
    o.public || !o.password.is_empty()
}

/// The browser's title: the host's own, else "<name>'s game".
pub(super) fn title(o: &HostOptions, name: &str) -> String {
    let t = o.title.trim();
    if t.is_empty() {
        format!("{name}'s game")
    } else {
        t.to_string()
    }
}

fn favourites_path() -> Option<std::path::PathBuf> {
    fnv_install::EngineHome::default_root().map(|h| h.join(FAVOURITES_FILE))
}

/// The favourite hosts kept in the engine home.
pub(super) fn load_favourites() -> Vec<String> {
    let Some(p) = favourites_path() else { return Vec::new() };
    std::fs::read_to_string(p).map(|t| parse_favourites(&t)).unwrap_or_default()
}

/// A favourites file's fingerprints: 16 hex digits a line, anything else left out.
fn parse_favourites(t: &str) -> Vec<String> {
    t.lines().map(str::trim).filter(|l| l.len() == 16 && l.chars().all(|c| c.is_ascii_hexdigit())).map(str::to_string).collect()
}

/// Add or remove a favourite host, and keep the list.
pub(super) fn set_favourite(list: &mut Vec<String>, host_fp: &str, on: bool) {
    list.retain(|f| f != host_fp);
    if on {
        list.push(host_fp.to_string());
    }
    if let Some(p) = favourites_path() {
        let text: String = list.iter().map(|f| format!("{f}\n")).collect();
        if let Err(e) = std::fs::write(&p, text) {
            warn!("co-op: cannot write {}: {e}", p.display());
        }
    }
}

/// The host's place in the game as the listing names it: the cell's or the worldspace's
/// name, as a save names it (`nvf_play::console::place_name`).
pub(super) fn place_now(world: &World) -> Option<String> {
    let d = &world.get_resource::<nvf_play::PlayWorld>()?.0;
    let sim = world.get_resource::<nvf_play::sim::SimState>()?;
    let loc = world.get_resource::<nvf_play::Location>().copied();
    let cell = loc.and_then(|l| l.interior).or_else(|| sim.world.peek(fnv_sim::FormId::PLAYER_REF).and_then(|s| s.cell).map(|c| c.0))?;
    Some(nvf_play::console::place_name(d, cell))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvf_signal::wire::BrowseRow;

    fn row(code: &str, protocol: u32, hash: &str, km: Option<u32>) -> BrowseRow {
        BrowseRow { code: code.into(), title: "t".into(), host_name: "h".into(), host_fp: "0123456789abcdef".into(), place: "Goodsprings".into(), players: 2, max_players: 6, locked: false, friendly_fire: false, plugins: 3, region: "OC/AU".into(), km, age: 5, protocol, build: "0.8.1".into(), load_order_hash: hash.into() }
    }

    #[test]
    fn rows_estimate_ping_and_say_what_cannot_join() {
        let v = nvf_coop::net::VERSION as u32;
        let a = BrowseAnswer { lobbies: vec![row("K7QM2X", v, "aa", Some(1500)), row("OLDOLD", v - 1, "aa", None), row("MODDED", v, "bb", Some(250))], cursor: None, total: 3 };
        let r = rows(&a, "aa", &["0123456789abcdef".to_string()]);
        assert_eq!(r[0].code, "K7Q-M2X");
        assert_eq!(r[0].ping_ms, Some(25));
        assert!(r[0].cannot.is_none() && r[0].favourite);
        assert!(r[1].cannot.as_deref().is_some_and(|c| c.contains("another version")) && r[1].ping_ms.is_none());
        assert!(r[2].cannot.as_deref().is_some_and(|c| c.contains("load order")));
    }

    #[test]
    fn a_listing_only_for_public_or_locked_lobbies() {
        let private = HostOptions::default();
        assert!(listing_for(&private, "Veronica", "Goodsprings", false, 3).is_none() && !listed(&private));
        let public = HostOptions { public: true, ..Default::default() };
        let l = listing_for(&public, "Veronica", "Goodsprings", true, 3).unwrap();
        assert_eq!((l.title.as_str(), l.locked, l.public), ("Veronica's game", false, true));
        let locked = HostOptions { password: "x".into(), ..Default::default() };
        let l = listing_for(&locked, "V", "", false, 0).unwrap();
        assert!(l.locked && !l.public && listed(&locked), "a private lobby with a password is made locked, not listed");
    }

    #[test]
    fn the_favourites_file_keeps_fingerprints_only() {
        assert_eq!(parse_favourites("0123456789abcdef\n  FEDCBA9876543210 \nnot one\n0123\n"), vec!["0123456789abcdef", "FEDCBA9876543210"]);
    }
}
