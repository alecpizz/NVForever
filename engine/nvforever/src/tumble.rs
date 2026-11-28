//! The weather's wind on loose bodies (docs/spec-notes/wind.md, issue #1401): the
//! tumbleweeds.
//!
//! - **Which bodies.** Those whose model's rigid body has Body Flags bit 0, "respond to
//!   wind" [documented: nif.xml bhkRigidBody]; of the base archive's 12,941 collision
//!   objects three set it: the tumbleweed (`clutter\tumbleweednv.nif`), the tumbleweed
//!   pile effect and Caesar's throne [observed: `fnvtool collide objects`, its `wind`
//!   column, 2026-11-06]. Chosen by that flag, never by a name (`fnv_phys`
//!   `PhysWorld::wind_refs`); only dynamic bodies are blown, and none indoors.
//! - **How.** Such a body starts awake when its cell loads. For its first [`GUST`] seconds
//!   awake its horizontal velocity is drawn towards the wind's, by at most [`WIND_ACCEL`]
//!   a second, so it rolls downwind and climbs what it can; then the gust is over and it
//!   is slowed by [`SETTLE`] a second until it rests and falls asleep. A body asleep is
//!   not woken by the wind; woken by something else (a shot, a push) it has a gust again
//!   [hypothesis fitted to o27_tumble, 2026-11-06:
//!   in the original the tumbleweeds near the player rolled at 100–200 units a second
//!   towards about 330° (north-north-west) after a load, came to rest within 5–17 s and
//!   stayed there for the next minute, in a dust storm too; one held on a slope kept
//!   rocking, more in the storm].
//! - **The wind.** Its direction is the trees' and the grass's (`crate::tree::wind_vector`),
//!   its speed [`ROLL_SPEED`] in any weather with wind, faded in and out across a change
//!   to or from a still one; none indoors.
//!
//! Log: `wind summary: …` at exit (bodies that respond to the wind, the most blown in one
//! frame, the longest way one went and where to, the most time a frame took).

use bevy::prelude::*;
use nvf_play::phys::PhysState;
use std::collections::HashMap;

/// The speed the wind rolls a tumbleweed at, game units a second, whatever the weather's
/// WTHR DATA wind speed, as long as it is not 0 [observed: o27_tumble, NVWastelandClear's
/// 50, the fastest rolled at 150–210 units a second; o32_tumble, HVDustStorm's 127 forced
/// before the cells loaded, the one that rolled went 654 units in 6 s at a median 126 and
/// a 90th percentile of 182, towards the same 319° (observer check 94, 2026-11-25, issue
/// #1401): no faster; hypothesis: the same in every weather with wind, 200 from the fastest
/// under 50]. Before, 4 units a second per unit of wind speed rolled them at 508 under 127.
pub const ROLL_SPEED: f32 = 200.0;

/// The most the wind changes a body's horizontal speed, game units a second squared: on
/// a slope of more than about 25° (gravity's share 9.8 m/s² × sin 25° ≈ 290 units/s²)
/// a tumbleweed stops [hypothesis: one of o27_tumble climbed about 85 units and stopped].
pub const WIND_ACCEL: f32 = 300.0;

/// How long the wind drives a body after it wakes, seconds [hypothesis: with
/// [`SETTLE`], a tumbleweed rolling at 200 units a second goes about 1,000 units, as the
/// farthest of o27_tumble did (1,012 along its path) before resting].
pub const GUST: f32 = 3.0;

/// How fast a body slows once its gust is over, game units a second squared
/// [hypothesis: the original's slowed from about 200 units a second to rest in 2–5 s].
pub const SETTLE: f32 = 50.0;

/// The wind in game space: (x, y) in game units a second.
pub fn wind_velocity(sky: &crate::sky::SkyState) -> [f32; 2] {
    let w = crate::tree::wind_vector(sky);
    // Bevy (x, z) is game (x, −y); w.w is the wind speed ÷ 255, 0 in a still weather.
    [w.x * roll_speed(w.w * 255.0), -w.z * roll_speed(w.w * 255.0)]
}

/// The roll speed for the weather's wind speed `wind` (DATA units, faded across a change):
/// [`ROLL_SPEED`] for any wind of 1 or more, eased in below that so a fade from a still
/// weather does not start it at once [design].
fn roll_speed(wind: f32) -> f32 {
    ROLL_SPEED * wind.clamp(0.0, 1.0)
}

#[derive(Resource, Default)]
struct WindStats {
    /// Where each blown body was first seen (game units).
    first: HashMap<u32, [f32; 3]>,
    /// The bodies blown in the frame that blew most.
    most: usize,
    /// The longest a frame's blowing took, µs.
    most_us: u128,
    /// Seconds each awake body has been driven since it woke.
    driven: HashMap<u32, f32>,
}

fn blow(time: Res<Time>, sky: Option<Res<crate::sky::SkyState>>, phys: Option<ResMut<PhysState>>, paused: Option<Res<nvf_play::pause::Paused>>, mut stats: ResMut<WindStats>) {
    let (Some(sky), Some(mut phys)) = (sky, phys) else { return };
    if paused.is_some_and(|p| p.0) {
        return;
    }
    let t = std::time::Instant::now();
    let wind = wind_velocity(&sky);
    // No wind (indoors, a still weather): nothing blown, nothing held back either.
    if wind == [0.0, 0.0] {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let mut n = 0;
    for (id, at) in phys.world.wind_refs() {
        stats.first.entry(id).or_insert(at);
        if phys.world.ref_bodies(id).iter().all(|b| b.sleeping) {
            stats.driven.remove(&id);
            continue;
        }
        let driven = stats.driven.entry(id).or_insert(0.0);
        *driven += dt;
        n += if *driven <= GUST { phys.world.blow(id, wind, WIND_ACCEL, dt) } else { phys.world.blow(id, [0.0, 0.0], SETTLE, dt) };
    }
    stats.most = stats.most.max(n);
    stats.most_us = stats.most_us.max(t.elapsed().as_micros());
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<WindStats>, phys: Option<Res<PhysState>>) {
    if exit.read().next().is_none() {
        return;
    }
    let now: HashMap<u32, [f32; 3]> = phys.map(|p| p.world.wind_refs().into_iter().collect()).unwrap_or_default();
    let far = stats
        .first
        .iter()
        .filter_map(|(id, a)| now.get(id).map(|b| (*id, (b[0] - a[0]).hypot(b[1] - a[1]), (b[0] - a[0]).atan2(b[1] - a[1]).to_degrees().rem_euclid(360.0))))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    let far = far.map_or("none".into(), |(id, d, h)| format!("{id:08X} {d:.0} units heading {h:.0}° (clockwise from north)"));
    info!(
        "wind summary: {} bodies respond to the wind, {} blown at most in a frame; the farthest went {far}; {} µs a frame at most",
        stats.first.len(),
        stats.most,
        stats.most_us
    );
}

pub struct TumblePlugin;

impl Plugin for TumblePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WindStats>().add_systems(Update, blow.after(crate::sky::update_sky)).add_systems(Last, report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wind_rolls_them_alike_in_a_breeze_and_a_storm() {
        // NVWastelandClear's 50 and HVDustStorm's 127 (observer check 94).
        assert_eq!(roll_speed(50.0), 200.0);
        assert_eq!(roll_speed(127.0), 200.0);
        assert_eq!(roll_speed(0.0), 0.0);
        assert!(roll_speed(0.5) < ROLL_SPEED);
        // A slope of 25° holds a body against the pull: gravity's share exceeds it.
        let g = 9.8 * 69.99;
        assert!(g * 25f32.to_radians().sin() < WIND_ACCEL * 1.0 + 1.0);
        assert!(g * 30f32.to_radians().sin() > WIND_ACCEL);
    }
}
