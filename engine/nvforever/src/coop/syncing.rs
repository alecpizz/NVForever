//! The syncing screen [design].
//!
//! A guest that has just joined, or has come back by loading a save after going down,
//! sees a black screen with the Swift Learner picture and "Syncing..."
//! (`nvf_play::coop_downed::SyncingScreen`) until its game holds the host's world as it is
//! now, so it never sees what its own save had there (an NPC the host's story had
//! disabled still standing, NPCs where its save left them). Not on ordinary loads.
//!
//! - **When.** The first time this run is welcomed by a host, when this run was not
//!   started by a load (`--handoff`), or when the load that started it set
//!   [`super::SYNC_ENV`]: a guest's join with a save from the main menu, and the
//!   progress side's reloads after going down.
//! - **The hand-shake.** Once its loading screen is down and it has stood in its place
//!   for [`ASK_AFTER`] (its place has gone to the host with its state), the guest asks
//!   the host (`Effect::Sync`, done false). The host sends that guest its whole area
//!   again at a sweep and answers on the same ordered stream after those changes
//!   (`replication`), so the answer means every enabled, disabled, moved and dead state
//!   of the area is applied here.
//! - **Then.** The screen holds until the host's whole presence list (the enabled flags
//!   all over its world) has come too, and, with the host in this player's place, until
//!   the host's NPCs have been placed as the host has them and this game's own actors
//!   the host lacks are hidden ([`HOLD_WITH_HOST`] after the host is settled here);
//!   elsewhere [`HOLD_AWAY`] after the answer, for the NPC poses to be drawn.
//! - **Never stuck.** It comes down after [`GIVE_UP`] whatever has come, and at once if
//!   the host is lost or the lobby ends (log lines say which).
//!
//! Log: `co-op: syncing: ...`.

use super::{at_main_menu, place_of, Coop, HIDE_AFTER, HOST_SETTLE};
use bevy::prelude::*;
use nvf_coop::repl::{self, Effect};
use nvf_play::coop_downed::SyncingScreen;
use nvf_play::loading::Loading;
use nvf_play::Location;

/// The guest asks this long (seconds) after its loading screen came down.
const ASK_AFTER: f32 = 0.5;
/// Asked again this often while no answer has come (a lost stream start).
const ASK_AGAIN: f32 = 5.0;
/// With the host in this place: held this long after the host has been settled here
/// (its NPCs placed as the host has them after [`HOST_SETTLE`], this game's own actors
/// the host lacks hidden after [`HIDE_AFTER`] more), with a little room.
const HOLD_WITH_HOST: f32 = HOST_SETTLE + HIDE_AFTER + 0.3;
/// The host elsewhere: held this long after the answer.
const HOLD_AWAY: f32 = 1.0;
/// Down after this long (seconds since this run was welcomed) whatever has come.
const GIVE_UP: f32 = 30.0;

/// Where the syncing screen is.
#[derive(Default)]
pub(super) struct Sync {
    /// Decided when this run is first welcomed: whether it syncs.
    decided: bool,
    up: bool,
    since: f32,
    /// Since when the loading screen has been down (None: up, or not yet seen).
    settled: Option<f32>,
    /// The serial asked, and when.
    asked: Option<(u32, f32)>,
    /// When this game's digest was asked to go before the ask.
    digest: Option<f32>,
    /// When the answer came.
    answered: Option<f32>,
}

/// Whether the screen may come down (pure, for the tests): the answer came, the whole
/// presence list too, and the hold after them is over.
fn ready(now: f32, answered: Option<f32>, presence: bool, host_here_since: Option<f32>) -> bool {
    let Some(at) = answered else { return false };
    presence
        && match host_here_since {
            Some(since) => now - since >= HOLD_WITH_HOST,
            None => now - at >= HOLD_AWAY,
        }
}

/// Whether this run syncs when first welcomed (pure): a run not started by a load, or one
/// a load marked.
fn due(marked: bool, handoff_run: bool) -> bool {
    marked || !handoff_run
}

#[allow(clippy::too_many_arguments)]
pub(super) fn syncing(
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    loading: Option<Res<Loading>>,
    loc: Option<Res<Location>>,
    main_menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    mut screen: ResMut<SyncingScreen>,
    mut st: Local<Sync>,
    mut marked: Local<Option<bool>>,
) {
    let now = time.elapsed_secs();
    // Read once, and taken out of the environment so a later ordinary load does not hold it.
    let marked = *marked.get_or_insert_with(|| {
        let on = std::env::var_os(super::SYNC_ENV).is_some();
        std::env::remove_var(super::SYNC_ENV);
        on
    });
    let coop = &mut *coop;
    let welcomed = coop.is_guest() && coop.link.as_ref().and_then(|l| l.id()).is_some();
    if !st.decided {
        if !welcomed {
            return;
        }
        st.decided = true;
        let handoff_run = std::env::args().any(|a| a == "--handoff");
        if !due(marked, handoff_run) {
            info!("co-op: syncing: this run came from an ordinary load: no syncing screen");
            return;
        }
        info!("co-op: syncing: {}: the syncing screen holds until this game has the host's world", if marked { "a join or a load after going down" } else { "a first join" });
        st.up = true;
        st.since = now;
    }
    if !st.up {
        screen.0 = false;
        return;
    }
    let down = |st: &mut Sync, screen: &mut SyncingScreen, why: &str| {
        info!("co-op: syncing: {why}: the syncing screen comes down after {:.1} s", now - st.since);
        st.up = false;
        screen.0 = false;
    };
    if !welcomed || coop.host_lost || at_main_menu(main_menu.as_deref()) {
        down(&mut st, &mut screen, "the host is gone or the lobby ended");
        return;
    }
    screen.0 = true;
    if now - st.since >= GIVE_UP {
        let what = match (st.answered, coop.presence.complete) {
            (None, _) => "no answer from the host",
            (Some(_), false) => "the host's presence list not whole",
            _ => "the host's NPCs not settled",
        };
        down(&mut st, &mut screen, &format!("gave up ({what})"));
        return;
    }
    if loading.as_deref().is_some_and(|l| l.active()) || loc.is_none() {
        st.settled = None;
        return;
    }
    let settled = *st.settled.get_or_insert(now);
    // The answer to what was asked.
    if let (Some((serial, _)), Some(done)) = (st.asked, coop.repl.sync_done) {
        if done == serial && st.answered.is_none() {
            st.answered = Some(now);
            info!("co-op: syncing: the host's answer came {:.1} s after the ask", now - st.asked.map_or(now, |(_, at)| at));
        }
    }
    // A joining guest asks once it stands beside the host (`rejoin`): its own save's
    // place is not where it plays.
    if coop.join_trip {
        st.settled = None;
        return;
    }
    if st.answered.is_none() && now - settled >= ASK_AFTER && st.asked.is_none_or(|(_, at)| now - at >= ASK_AGAIN) {
        // This game's digest first (the host sends back what its own save changed here),
        // then, on a later frame, the ask behind it on the same stream.
        if st.digest.is_none_or(|at| st.asked.is_some_and(|(_, asked)| asked >= at)) {
            coop.repl.digest_now();
            st.digest = Some(now);
            return;
        }
        if st.digest.is_some_and(|at| at >= now) {
            return;
        }
        let serial = st.asked.map_or(1, |(s, _)| s + 1);
        if let Some(link) = coop.link.as_ref() {
            for item in repl::pack_effects(&[Effect::Sync { serial, done: false }], 1000) {
                // A full stream refuses it; the stream starts again and resends the whole state.
                let _ = link.send_reliable(None, item);
            }
            info!("co-op: syncing: asked the host for its world here ({serial})");
            st.asked = Some((serial, now));
        }
    }
    let here = loc.as_deref().map(place_of);
    let host_here_since = coop.host_place.filter(|(p, _)| Some(*p) == here).map(|(_, since)| since);
    if ready(now, st.answered, coop.presence.complete, host_here_since) {
        down(&mut st, &mut screen, &format!("synced ({})", if host_here_since.is_some() { "the host's NPCs placed here" } else { "the host elsewhere" }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_first_joins_and_marked_loads_sync() {
        assert!(due(false, false), "a run started by hand that joins");
        assert!(due(true, true), "a join with a save, or a load after going down");
        assert!(!due(false, true), "an ordinary load");
    }

    #[test]
    fn the_screen_waits_for_the_answer_the_list_and_the_hold() {
        assert!(!ready(100.0, None, true, Some(0.0)), "no answer");
        assert!(!ready(100.0, Some(99.0), false, Some(0.0)), "the list not whole");
        assert!(!ready(10.0, Some(9.0), true, Some(10.0 - HOLD_WITH_HOST + 0.1)), "the host's NPCs not settled");
        assert!(ready(10.0, Some(9.0), true, Some(10.0 - HOLD_WITH_HOST)));
        assert!(!ready(10.0, Some(9.5), true, None), "away: the poses' hold");
        assert!(ready(10.0, Some(9.0), true, None));
    }
}
