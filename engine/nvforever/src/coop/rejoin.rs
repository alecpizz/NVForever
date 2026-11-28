//! Beside the host on joining, and back beside it after everyone was down [design].
//!
//! **Joining.** Any save joins any save, and only the character carries over: a guest
//! loads into the host's world at the host's location (a guest whose save was in another
//! worldspace or interior woke up there otherwise). So each time this
//! run is welcomed by a host, the guest is taken beside the host once, under the syncing
//! screen (`super::syncing` holds its ask until the trip is done), as soon as its own game
//! is kept and the host has stood in its place for [`HOST_SETTLED`]. A guest already
//! within [`ALREADY_NEAR`] of the host in its place stays where it is. Its own game stays
//! where it left it (`nvf_play::save::SoloBase`): a save made while visiting loads back
//! there.
//!
//! A run started with [`super::REJOIN_ENV`] set (a guest's reload after every player was
//! down; what sets it is the progress side's) joins the host's again. A guest's save is
//! its own game where it left it (`nvf_play::save::SoloBase`), and free roam moves a guest
//! to the host only when the host's script moves the host (`super::leash`), so the guest
//! would wake up at its own save's spot, far from the party. That run takes the guest
//! beside the host once: as soon
//! as this game is kept as the guest's own, the host is heard, and the host has stood in
//! its place for [`HOST_SETTLED`] seconds (its own reload's loading screen is over).
//!
//! Log: `co-op: joined: beside the host at …`, `co-op: joined: already beside the host`,
//! `co-op: back after everyone was down: beside the host at …`.

use super::{at_main_menu, place_of, scene, Coop};
use bevy::prelude::*;
use nvf_coop::net::Place;
use nvf_play::loading::{Loading, TravelRequest};
use nvf_play::player::Player;
use nvf_play::{Location, Travel};

/// A joining guest this near the host (units across, in its place) is not moved
/// [design: well within sight; the leash's step-out puts it beside the host].
const ALREADY_NEAR: f32 = 1000.0;

/// The host must have stood in its place this long (seconds) before a guest is brought
/// to it: the host's own reload puts it there behind a loading screen.
const HOST_SETTLED: f32 = 2.0;

/// Whether the trip beside the host may go now (pure, for the tests).
fn ready(guest: bool, kept: bool, host_lost: bool, loading: bool, host_here_since: Option<f32>, now: f32) -> bool {
    guest && kept && !host_lost && !loading && host_here_since.is_some_and(|t| now - t >= HOST_SETTLED)
}

/// Whether a guest at `mine` (its place and feet) is beside the host already (pure).
fn already_near(mine: Option<(Place, [f32; 3])>, host: Place, host_feet: [f32; 3]) -> bool {
    mine.is_some_and(|(p, f)| p == host && (f[0] - host_feet[0]).hypot(f[1] - host_feet[1]) <= ALREADY_NEAR)
}

/// What the joining trip keeps: whether this link was taken beside the host, and whether
/// this run follows an all-down reload (read once).
#[derive(Default)]
pub(super) struct Trip {
    rejoin: Option<bool>,
    done: bool,
}

/// A guest, each time this run is welcomed by a host: beside the host, once (module
/// notes).
#[allow(clippy::too_many_arguments)]
pub(super) fn rejoin_host(
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    loading: Option<Res<Loading>>,
    main_menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    solo: Option<Res<nvf_play::save::SoloBase>>,
    (loc, player): (Option<Res<Location>>, Option<Res<Player>>),
    mut travel: MessageWriter<TravelRequest>,
    mut trip: Local<Trip>,
) {
    // Read once, and taken out of the environment so a later load of this run does not
    // say so again.
    let rejoin = *trip.rejoin.get_or_insert_with(|| {
        let on = std::env::var_os(super::REJOIN_ENV).is_some();
        std::env::remove_var(super::REJOIN_ENV);
        if on {
            info!("co-op: this run follows an all-down reload: the guest goes beside the host once it is heard");
        }
        on
    });
    // Each new link (a join, or a join again after leaving) makes one trip.
    let welcomed = coop.is_guest() && coop.link.as_ref().and_then(|l| l.id()).is_some();
    if !welcomed {
        trip.done = false;
        coop.join_trip = false;
        return;
    }
    if trip.done {
        return;
    }
    coop.join_trip = true;
    if at_main_menu(main_menu.as_deref()) {
        return;
    }
    let now = time.elapsed_secs();
    let since = coop.host_place.map(|(_, t)| t);
    if !ready(coop.is_guest(), solo.is_some(), coop.host_lost, loading.is_some_and(|l| l.active()), since, now) {
        return;
    }
    let Some(host) = coop.host else { return };
    trip.done = true;
    coop.join_trip = false;
    let mine = loc.as_deref().map(place_of).zip(player.as_deref().map(|p| p.feet));
    if !rejoin && already_near(mine, host.place, host.feet) {
        info!("co-op: joined: already beside the host at {:?}: not moved", host.place);
        return;
    }
    // Each guest its own place beside the host (`scene::beside_slot`).
    let feet = scene::beside_slot(host.feet, host.yaw, coop.link.as_ref().and_then(|l| l.id()).unwrap_or(1));
    let (world, interior) = match host.place {
        Place::Interior(c) => (None, Some(c)),
        Place::Exterior(w) => (Some(w), None),
    };
    let from = mine.map_or_else(|| "nowhere yet".to_string(), |(p, f)| format!("{p:?} ({:.0}, {:.0}, {:.0})", f[0], f[1], f[2]));
    if rejoin {
        info!("co-op: back after everyone was down: beside the host at {:?} {feet:?} (from {from})", host.place);
    } else {
        info!("co-op: joined: beside the host at {:?} {feet:?} (from its own save's place, {from})", host.place);
    }
    travel.write(TravelRequest(Travel { world, interior, feet, heading_deg: host.yaw.to_degrees(), pitch_deg: None }));
    coop.last_leash = Some(now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goes_once_the_host_has_settled() {
        // Not before the guest's game is kept, the host is heard and has settled.
        assert!(!ready(true, false, false, false, Some(0.0), 10.0));
        assert!(!ready(true, true, false, false, None, 10.0));
        assert!(!ready(true, true, false, false, Some(9.0), 10.0));
        assert!(ready(true, true, false, false, Some(8.0), 10.0));
        // Not while this game loads, nor with a lost host, nor on the host.
        assert!(!ready(true, true, false, true, Some(0.0), 10.0));
        assert!(!ready(true, true, true, false, Some(0.0), 10.0));
        assert!(!ready(false, true, false, false, Some(0.0), 10.0));
    }

    #[test]
    fn a_guest_elsewhere_is_taken_to_the_host() {
        let strip = Place::Exterior(0x0013_B308);
        let host = [100.0, 200.0, 0.0];
        // Another worldspace, or an interior: taken.
        assert!(!already_near(Some((Place::Exterior(0x3C), host)), strip, host));
        assert!(!already_near(Some((Place::Interior(0x00AB_C10A), host)), strip, host));
        // Same worldspace, far: taken; near: stays.
        assert!(!already_near(Some((strip, [5000.0, 200.0, 0.0])), strip, host));
        assert!(already_near(Some((strip, [400.0, 600.0, 0.0])), strip, host));
        // Its place not known yet: taken.
        assert!(!already_near(None, strip, host));
    }
}
