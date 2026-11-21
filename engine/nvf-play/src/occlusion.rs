//! Sound occlusion (Modern Feel, `ModernFeel::sound_occlusion`, docs/design/modern-feel-audio.md)
//! [design: NVForever's own, not the original's; every constant an engineering choice
//! tuned by ear]. A sound played at a place with a wall, a floor or a hill between it and
//! the listener is muffled and quieter.
//!
//! - **Which sounds**: those playing at a place with a [`SoundAt`]: `crate::sfx`'s placed
//!   one-shots (shots, impacts, footsteps, creatures, doors), `crate::ambient`'s placed
//!   loops, `crate::radio`'s radios. Not the sounds at the listener (the player's own,
//!   interface, background loops) nor voices: the conversation with the player is played
//!   unplaced, so the one you talk to is never muffled.
//! - **The test**: a ray from the listener (the player's eye) to the sound through the
//!   physics world, counting the different colliders it crosses (terrain, statics,
//!   animated statics, doors; not actors, moving bodies, clutter, trees, glass or
//!   triggers), hits within [`END_SLACK`] of either end not counted (the sound's own
//!   object). Each blocker adds one to the occlusion, at most [`MOST`]. Sounds nearer than
//!   [`NEAR`] are not tested.
//! - **When**: a one-shot as it starts (its first sample already muffled); loops and long
//!   sounds again every [`RECHECK`], eased toward the new value over [`SMOOTH`]. At most
//!   [`NEW_RAYS`] starts and [`RECHECK_RAYS`] re-checks cast a ray in a frame; a start
//!   past the budget, or near one tested in the last [`CACHE_LIFE`] from about the same
//!   place, takes that cached answer ([`CELL`]-unit squares).
//! - **The sound** ([`Muffle`], run in the placed sound's decoder): an occlusion `o`
//!   lowers it [`DB_PER`] dB per blocker (at most [`DB_MOST`]) and low-passes it toward
//!   [`ONE_HZ`] at one blocker and [`MOST_HZ`] at two or more ([`muffle`]).
//!
//! Switched off, every sound eases back to clear and nothing is tested. Cost: at most
//! `NEW_RAYS + RECHECK_RAYS` rays a frame (each a BVH walk) and, per placed sound, one
//! one-pole filter while occluded.

use crate::modern_feel::ModernFeel;
use crate::phys::PhysState;
use crate::player::Player;
use crate::sfx::{Pan, SoundAt};
use bevy::prelude::*;
use fnv_phys::layers::{self, LayerSet};
use fnv_phys::{Filter, Owner, Queries};
use std::collections::HashMap;

/// Sounds nearer than this (units) are never occluded.
pub const NEAR: f32 = 160.0;
/// Hits this near either end of the ray (units) are not counted.
pub const END_SLACK: f32 = 24.0;
/// The most blockers counted.
pub const MOST: f32 = 3.0;
/// Decibels per blocker, and the most taken off.
pub const DB_PER: f32 = -6.5;
pub const DB_MOST: f32 = -16.0;
/// The low-pass at one blocker and at two or more, Hz.
pub const ONE_HZ: f32 = 1_000.0;
pub const MOST_HZ: f32 = 600.0;
/// Seconds between re-checks of a playing sound, and the easing toward a new value.
pub const RECHECK: f64 = 0.25;
pub const SMOOTH: f32 = 0.15;
/// Rays a frame, for starting sounds and for re-checks.
pub const NEW_RAYS: usize = 12;
pub const RECHECK_RAYS: usize = 8;
/// A cached answer's life, seconds, and the size of its squares, units.
pub const CACHE_LIFE: f64 = 0.5;
pub const CELL: f32 = 96.0;

/// The low-pass (Hz) and gain an occlusion `o` gives (module notes).
pub fn muffle(o: f32) -> (f32, f32) {
    let o = o.clamp(0.0, MOST);
    let hz = if o <= 1.0 { 20_000.0 * (ONE_HZ / 20_000.0).powf(o) } else { ONE_HZ * (MOST_HZ / ONE_HZ).powf((o - 1.0).min(1.0)) };
    let db = (DB_PER * o).max(DB_MOST);
    (hz, 10f32.powf(db / 20.0))
}

/// The occlusion filter of one placed sound, in its decoder: eases to the occlusion the
/// main thread sets (the first frame at it), low-passes and lowers by it. Clear (0), the
/// sound passes untouched.
#[derive(Clone, Debug)]
pub struct Muffle {
    now: Option<f32>,
    step: f32,
    rate: f32,
    a: f32,
    gain: f32,
    y: f32,
    count: u32,
    /// The occlusion `a` and `gain` were worked out for.
    made_for: f32,
}

impl Muffle {
    pub fn new(rate: u32) -> Muffle {
        let rate = rate.max(1) as f32;
        // A whole blocker in 60 ms.
        Muffle { now: None, step: 1.0 / (rate * 0.06), rate, a: 0.0, gain: 1.0, y: 0.0, count: 0, made_for: -1.0 }
    }

    pub fn run(&mut self, x: f32, target: f32) -> f32 {
        let target = if target.is_finite() { target.clamp(0.0, MOST) } else { 0.0 };
        let now = match self.now {
            None => target,
            Some(n) => n + (target - n).clamp(-self.step, self.step),
        };
        self.now = Some(now);
        if now <= 0.0 {
            self.y = x;
            return x;
        }
        // The coefficients again every 32 frames while it moves, at once after a jump.
        let off = (self.made_for - now).abs();
        if off > 0.25 || (off > 0.0 && self.count == 0) {
            let (hz, gain) = muffle(now);
            self.a = (-std::f32::consts::TAU * hz.min(self.rate * 0.45) / self.rate).exp();
            self.gain = gain;
            self.made_for = now;
        }
        self.count = (self.count + 1) % 32;
        self.y += (1.0 - self.a) * (x - self.y);
        self.y * self.gain
    }
}

/// A placed sound being followed.
struct Track {
    target: f32,
    now: f32,
    next: f64,
    pan: Pan,
    seen: u64,
}

/// The sounds followed and the counts for the exit line.
#[derive(Resource, Default)]
pub struct Occlusion {
    tracked: HashMap<Entity, Track>,
    cache: HashMap<([i32; 3], [i32; 3]), (f32, f64)>,
    frame: u64,
    on: Option<bool>,
    pub rays: u64,
    pub cached: u64,
    pub muffled: u64,
    pub most_rays: usize,
    logged: f64,
}

fn cell(p: [f32; 3]) -> [i32; 3] {
    p.map(|v| (v / CELL).floor() as i32)
}

/// The layers that do not block a sound (everything but statics, animated statics,
/// ground and unidentified).
fn see_through() -> LayerSet {
    let keep = [layers::UNIDENTIFIED, layers::STATIC, layers::ANIM_STATIC, layers::GROUND];
    let all: Vec<u8> = (0..=42u8).filter(|l| !keep.contains(l)).collect();
    LayerSet::from_layers(&all)
}

/// The blockers between `eye` and `at` (module notes).
fn blockers(q: &Queries<'_>, filter: &Filter, eye: [f32; 3], at: [f32; 3]) -> f32 {
    let d = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if !len.is_finite() || len < NEAR {
        return 0.0;
    }
    let hits = q.ray_all(eye, d, len - END_SLACK, filter);
    let mut owners: Vec<Owner> = Vec::new();
    for h in hits {
        if h.distance > END_SLACK && !owners.contains(&h.owner) {
            owners.push(h.owner);
            if owners.len() as f32 >= MOST {
                break;
            }
        }
    }
    owners.len() as f32
}

/// What one frame's checks share.
struct Pass<'a, 'q> {
    q: &'a Queries<'q>,
    filter: &'a Filter,
    eye: [f32; 3],
    eye_cell: [i32; 3],
    now: f64,
    ease: f32,
    frame: u64,
    new_rays: usize,
    re_rays: usize,
    logged: bool,
}

impl Pass<'_, '_> {
    /// Follow one placed sound: tested as it starts, again every [`RECHECK`], eased.
    fn visit(&mut self, occ: &mut Occlusion, e: Entity, at: [f32; 3], pan: &Pan) {
        let key = (cell(at), self.eye_cell);
        match occ.tracked.get_mut(&e) {
            None => {
                let o = match occ.cache.get(&key) {
                    Some(&(o, _)) => {
                        occ.cached += 1;
                        o
                    }
                    None if self.new_rays < NEW_RAYS => {
                        self.new_rays += 1;
                        let o = blockers(self.q, self.filter, self.eye, at);
                        occ.cache.insert(key, (o, self.now));
                        o
                    }
                    None => 0.0,
                };
                pan.set_occlusion(o);
                occ.muffled += (o > 0.0) as u64;
                if o > 0.0 && !self.logged && self.now - occ.logged > 0.5 {
                    self.logged = true;
                    occ.logged = self.now;
                    let eye = self.eye;
                    let d = ((at[0] - eye[0]).powi(2) + (at[1] - eye[1]).powi(2) + (at[2] - eye[2]).powi(2)).sqrt();
                    let (hz, g) = muffle(o);
                    info!("occlusion: a sound {d:.0} units off behind {o:.0} blockers: low-pass {hz:.0} Hz, {:.1} dB", 20.0 * g.log10());
                }
                occ.tracked.insert(e, Track { target: o, now: o, next: self.now + RECHECK, pan: pan.clone(), seen: self.frame });
            }
            Some(t) => {
                t.seen = self.frame;
                if t.next <= self.now && self.re_rays < RECHECK_RAYS {
                    self.re_rays += 1;
                    t.next = self.now + RECHECK;
                    t.target = blockers(self.q, self.filter, self.eye, at);
                    occ.cache.insert(key, (t.target, self.now));
                }
                let before = t.now;
                t.now += (t.target - t.now) * self.ease;
                if (t.now - t.target).abs() < 0.01 {
                    t.now = t.target;
                }
                if t.now != before {
                    t.pan.set_occlusion(t.now);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn occlude(
    mut occ: ResMut<Occlusion>,
    feel: Option<Res<ModernFeel>>,
    phys: Option<ResMut<PhysState>>,
    player: Option<Res<Player>>,
    sounds: Query<(Entity, &SoundAt)>,
    ambient: Option<Res<crate::ambient::Ambient>>,
    time: Res<Time<Real>>,
    mut filter: Local<Option<Filter>>,
) {
    let on = feel.is_some_and(|f| f.sound_occlusion());
    let occ = &mut *occ;
    if occ.on != Some(on) {
        occ.on = Some(on);
        info!("occlusion: sound occlusion {}", if on { "on" } else { "off" });
        if !on {
            // Every sound eases back to clear (its decoder's easing).
            for t in occ.tracked.values() {
                t.pan.set_occlusion(0.0);
            }
            occ.tracked.clear();
            occ.cache.clear();
        }
    }
    let (true, Some(mut phys), Some(player)) = (on, phys, player) else { return };
    let filter = filter.get_or_insert_with(|| Filter { no_actors: true, no_dynamic: true, exclude_layers: see_through(), ..Default::default() });
    occ.frame += 1;
    let now = time.elapsed_secs_f64();
    occ.cache.retain(|_, (_, t)| now - *t < CACHE_LIFE);
    let eye = player.eye();
    let q = phys.world.queries();
    let mut pass = Pass { q: &q, filter, eye, eye_cell: cell(eye), now, ease: 1.0 - (-time.delta_secs() / SMOOTH).exp(), frame: occ.frame, new_rays: 0, re_rays: 0, logged: false };
    for (e, s) in &sounds {
        pass.visit(occ, e, s.at, &s.pan);
    }
    for (e, pan, at) in ambient.iter().flat_map(|a| a.placed()) {
        pass.visit(occ, e, at, pan);
    }
    let rays = pass.new_rays + pass.re_rays;
    occ.rays += rays as u64;
    occ.most_rays = occ.most_rays.max(rays);
    let frame = occ.frame;
    occ.tracked.retain(|_, t| t.seen == frame);
}

fn report(mut exit: MessageReader<AppExit>, occ: Res<Occlusion>) {
    if exit.read().last().is_some() && occ.rays > 0 {
        info!("occlusion summary: {} rays cast (at most {} in a frame), {} answers from the cache, {} sounds started muffled", occ.rays, occ.most_rays, occ.cached, occ.muffled);
    }
}

/// Added by [`crate::sfx::SfxPlugin`].
pub(crate) fn build(app: &mut App) {
    app.init_resource::<Occlusion>().add_systems(Update, occlude.after(crate::sfx::SfxStep::Play).in_set(crate::sfx::SfxSet)).add_systems(Last, report);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_blockers_muffle_more() {
        let (h0, g0) = muffle(0.0);
        let (h1, g1) = muffle(1.0);
        let (h3, g3) = muffle(3.0);
        assert!(h0 > 19_000.0 && (g0 - 1.0).abs() < 1e-6);
        assert!(h1 < h0 && g1 < g0 && h3 < h1 && g3 < g1);
        assert!((20.0 * g3.log10() - DB_MOST).abs() < 0.01);
    }

    #[test]
    fn clear_passes_and_occluded_is_quieter() {
        let mut m = Muffle::new(22050);
        assert_eq!(m.run(0.5, 0.0), 0.5);
        let mut m = Muffle::new(22050);
        let mut last = 0.0;
        for _ in 0..2000 {
            last = m.run(1.0, 2.0);
        }
        assert!(last < 0.5 && last > 0.0);
    }
}
