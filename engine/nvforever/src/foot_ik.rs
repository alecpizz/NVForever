//! Feet on the ground (docs/spec-notes/actors.md §13): on a slope, a step or uneven
//! ground each foot of a standing or walking biped is put on the ground under it. An
//! NVForever design feature after the owner's screenshots of the game (2026-10-17): the
//! original's method and numbers are not known, so the constants below are design
//! choices [design].
//!
//! - **Who.** Every drawn actor whose skeleton has the biped's leg bones (`Bip01 L/R
//!   Thigh`, `Calf`, `Foot`, each the child of the one before, and `Bip01 NonAccum` or
//!   `Bip01 Pelvis` above both thighs): NPCs, the player's third-person body, and the
//!   creatures built on such a skeleton. Not while dead, hidden, on furniture, in the air
//!   or swimming (their character in the physics world says so), nor further than
//!   [`ACTIVE_RADIUS`] from the camera: there the feet ease back to the animation.
//! - **The ground.** With `--play`, a ray down through each ankle in the collision world
//!   (terrain and static models; not characters, actor hit volumes nor the layers a
//!   character walks through), which gives the height and the slope, the height raised
//!   to a model's drawn floor where its collision lies under it
//!   (`PhysState::drawn_over`, floating-actors.md §7); without it the drawn landscape's
//!   heights.
//! - **How.** After the frame's animation has posed the skeleton and before transforms are
//!   propagated, on top of the animated pose: the clips stand on level ground at the
//!   actor's root, so each foot is moved up or down by the ground's height under it
//!   against the root. The body is lowered as far as the lower foot must go
//!   ([`MAX_DROP`] at most), since a straight leg cannot reach further; then each leg is
//!   bent by a two-bone solve (thigh and calf, the knee kept in the plane the animation
//!   bends it in) so its ankle reaches its own ground, the higher foot's knee bending the
//!   more. Each foot then turns to lie on the slope under it ([`MAX_TILT`] at most),
//!   fully while it is planted and not at all once the animation lifts it
//!   [`LIFTED`] above where it stands. Every change eases in and out ([`EASE_RATE`]).
//!
//! Log, starting `foot ik:`: one line per skeleton saying which bones are moved; at the
//! end of a run how many actors stood on uneven ground, and the largest drop and raise.

use crate::furniture::{OnFurniture, PlayerUse};
use crate::head_look::animated_rotation;
use crate::limbs::ActorRig;
use crate::locomotion::Dying;
use crate::player_body::PlayerBody;
use crate::ragdoll::Corpse;
use crate::world::WorldData;
use bevy::math::Affine3A;
use bevy::prelude::*;
use fnv_phys::{layers, Filter, Key};
use fnv_sim::FormId;
use nvf_play::combat::is_dead;
use nvf_play::phys::PhysState;
use nvf_play::script::ScriptState;
use nvf_play::space;
use std::collections::{HashMap, HashSet};

/// Feet are placed only on actors this near the camera, game units (about 36 m), the
/// head tracking's reach [design].
pub const ACTIVE_RADIUS: f32 = 2500.0;
/// The body is lowered at most this far for a foot below the root, skeleton units: about
/// half a shin, past which a bent stance looks wrong [design].
pub const MAX_DROP: f32 = 28.0;
/// A foot is raised at most this far above where the animation puts it [design].
pub const MAX_RAISE: f32 = 40.0;
/// A foot turns at most this far to lie on a slope, radians (about 35°) [design].
pub const MAX_TILT: f32 = 0.61;
/// A foot lies on the slope while the animation holds its ankle within `PLANTED` of its
/// height in the bind pose and no longer once it is [`LIFTED`] above it, units [design:
/// a walk lifts the ankle about 15 units, floating-actors.md].
pub const PLANTED: f32 = 3.0;
pub const LIFTED: f32 = 14.0;
/// How fast the changes follow the ground, per second (exponential ease) [design].
pub const EASE_RATE: f32 = 12.0;
/// The ground is looked for from this far above the actor's root to this far below it,
/// game units [design: just above `MAX_RAISE`, below `MAX_DROP`]. Ground found higher
/// than `MAX_RAISE` above the root is not stood on: the foot is inside something (a rock,
/// a crate), and the animation is kept for it.
const PROBE_UP: f32 = MAX_RAISE + 5.0;
const PROBE_DOWN: f32 = 60.0;
/// Foot locking (the player's body with Motion Matching on, docs/design/motion-matching.md
/// §11): a foot the animation holds within `LOCK_LIFT` of its bind height, moving slower
/// than `LOCK_SPEED` over the ground, is held where it came down; let go when it lifts or
/// when the animation has left it `LOCK_DRIFT` behind; held and let go along an ease of
/// `LOCK_RATE` per second [design].
pub const LOCK_LIFT: f32 = 4.0;
pub const LOCK_SPEED: f32 = 250.0;
pub const LOCK_DRIFT: f32 = 16.0;
pub const LOCK_RATE: f32 = 30.0;
/// Below this the state counts as at rest and the bones are given back.
const REST: f32 = 0.05;

const SIDES: [&str; 2] = ["L", "R"];

#[derive(Resource)]
pub struct FootIkSettings {
    /// Feet follow the ground (`--no-foot-ik`; the settings page switches it).
    pub enabled: bool,
}

/// An actor's leg bones (skeleton node indices).
#[derive(Debug, Clone, Copy)]
struct Legs {
    /// The node lowered to lower the body.
    body: usize,
    /// Per side: thigh, calf, foot.
    legs: [[usize; 3]; 2],
    /// The ankles' height above the root in the bind pose.
    ankle_bind: [f32; 2],
}

/// The feet of one actor (on its root entity).
#[derive(Component, Default)]
pub struct FootIk {
    /// `None` when the skeleton has no legs to place.
    legs: Option<Legs>,
    /// As eased: how far the body is lowered (≤ 0), each foot raised above that (≥ 0),
    /// and each foot's turn onto the slope (actor space).
    drop: f32,
    raise: [f32; 2],
    tilt: [Quat; 2],
    /// The animated translation of the body node and what was written over it.
    written_body: Option<(Vec3, Vec3)>,
    /// Per side, per bone (thigh, calf, foot): animated rotation and what was written.
    written: [[Option<(Quat, Quat)>; 3]; 2],
    /// Foot locking, per side: where the foot is held (world), whether it is held now, how
    /// fully (eased 0-1), and the animated ankle's world place last frame.
    lock_at: [Option<Vec3>; 2],
    locked: [bool; 2],
    lock_w: [f32; 2],
    last_ankle: [Option<Vec3>; 2],
    /// The ankle's target as shown last frame (world; the skate test aid).
    shown_last: [Option<Vec3>; 2],
}

impl FootIk {
    fn at_rest(&self) -> bool {
        self.lock_w.iter().all(|w| *w < REST) && self.drop.abs() < REST && self.raise.iter().all(|r| r.abs() < REST) && self.tilt.iter().all(|t| t.angle_between(Quat::IDENTITY) < 0.002)
    }
}

#[derive(Resource, Default)]
pub struct FootIkStats {
    pub rigged: usize,
    /// Actors (references) whose feet were moved noticeably (over a unit) at some time.
    pub placed: HashSet<u32>,
    pub largest_drop: f32,
    pub largest_raise: f32,
    /// Who had the largest raise.
    pub largest_raise_by: u32,
    pub largest_tilt: f32,
    /// Feet looked for, and found ground under.
    pub probes: u64,
    /// Actor-frames worked on, and those left out: hidden or dead, on furniture, not on
    /// the ground (in the air or swimming), too far.
    pub active: u64,
    pub out: [u64; 4],
    pub found: u64,
    /// Of those, the ground too high above the root to stand on.
    pub inside: u64,
    pub total: std::time::Duration,
    pub frames: u32,
    described: HashSet<String>,
    /// The test aid `NVF_PLAY_TEST_FOOT_SKATE`: the player's planted feet's speeds over the
    /// ground, as animated and as shown, since the last report, and seconds since it.
    skate: (Vec<f32>, Vec<f32>, f32),
    pub locks: u64,
    /// With `NVF_PLAY_TEST_FOOT_SKATE=<file>.csv`: one line a frame and foot.
    skate_csv: Option<std::io::BufWriter<std::fs::File>>,
    skate_clock: f32,
}

pub struct FootIkPlugin {
    pub enabled: bool,
}

impl Plugin for FootIkPlugin {
    fn build(&self, app: &mut App) {
        info!("foot ik: {} (--no-foot-ik)", if self.enabled { "on" } else { "off" });
        // After the frame's animation (and the player's body, in BodySet) posed the
        // skeletons; transforms are propagated in PostUpdate.
        app.insert_resource(FootIkSettings { enabled: self.enabled })
            .init_resource::<FootIkStats>()
            .add_systems(Update, (attach, place).chain().after(crate::locomotion::drive).after(nvf_play::player::BodySet))
            .add_systems(Last, report);
    }
}

// ---- the solve (pure) -------------------------------------------------------------------

/// A two-bone solve: with the hip at `h`, the knee at `k` and the ankle at `a`, the
/// rotations that bring the ankle to `t` (or as near as the leg reaches): first the calf
/// turned about the knee (`.1`), then the whole leg about the hip (`.0`), both in the
/// space the points are given in. The knee stays in the plane it bends in; a straight leg
/// bends its knee towards `forward`.
pub fn two_bone(h: Vec3, k: Vec3, a: Vec3, t: Vec3, forward: Vec3) -> (Quat, Quat) {
    let (u, v) = (k - h, a - k);
    let (l1, l2) = (u.length(), v.length());
    // A broken skeleton (NaN or huge bone lengths) or target leaves the leg as it is:
    // `clamp` panics on NaN or crossed bounds.
    let (lo, hi) = ((l1 - l2).abs() + 1e-3, l1 + l2 - 1e-3);
    let reach = (t - h).length();
    if !(l1 >= 1e-3 && l2 >= 1e-3 && lo <= hi && hi.is_finite() && reach.is_finite() && forward.is_finite()) {
        return (Quat::IDENTITY, Quat::IDENTITY);
    }
    let d = reach.clamp(lo, hi);
    // The knee's inner angle now and wanted (law of cosines).
    let now = (-u).angle_between(v);
    let want = ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2)).clamp(-1.0, 1.0).acos();
    let mut axis = u.cross(v);
    if axis.length_squared() < 1e-6 * l1 * l2 {
        // A straight leg: bending turns the calf back, the knee forward.
        axis = forward.cross(v);
    }
    let calf = if axis.length_squared() > 1e-12 { Quat::from_axis_angle(axis.normalize(), now - want) } else { Quat::IDENTITY };
    let a2 = k + calf * v;
    let (from, to) = ((a2 - h).normalize_or_zero(), (t - h).normalize_or_zero());
    let leg = if from == Vec3::ZERO || to == Vec3::ZERO { Quat::IDENTITY } else { Quat::from_rotation_arc(from, to) };
    (leg, calf)
}

/// The turn that lays a foot on ground with normal `n` (actor space, z up), limited to
/// [`MAX_TILT`] and given `weight` (0–1) of it.
pub fn tilt_onto(n: Vec3, weight: f32) -> Quat {
    let n = n.normalize_or_zero();
    if n.z <= 0.0 || weight.is_nan() || weight <= 0.0 {
        return Quat::IDENTITY;
    }
    let full = Quat::from_rotation_arc(Vec3::Z, n);
    let angle = full.angle_between(Quat::IDENTITY);
    let limited = if angle > MAX_TILT { Quat::IDENTITY.slerp(full, MAX_TILT / angle) } else { full };
    Quat::IDENTITY.slerp(limited, weight.clamp(0.0, 1.0))
}

/// How fully a foot lies on the slope, from how high the animation holds its ankle above
/// its bind height.
pub fn planted(lift: f32) -> f32 {
    1.0 - ((lift - PLANTED) / (LIFTED - PLANTED)).clamp(0.0, 1.0)
}

/// How far the body is lowered and each foot raised for ground `h` under each foot (game
/// units against the root, positive up).
pub fn drop_and_raise(h: [f32; 2]) -> (f32, [f32; 2]) {
    let h = h.map(|h| h.clamp(-MAX_DROP, MAX_RAISE));
    let drop = h[0].min(h[1]).min(0.0);
    (drop, h.map(|h| (h - drop).clamp(0.0, MAX_RAISE)))
}

/// The local translation the animation gave a node that now has `now` (as
/// [`animated_rotation`]).
pub fn animated_translation(now: Vec3, written: Option<(Vec3, Vec3)>) -> Vec3 {
    match written {
        Some((base, w)) if now.abs_diff_eq(w, 1e-4) => base,
        _ => now,
    }
}

// ---- the actors -------------------------------------------------------------------------

fn find(rig: &ActorRig, name: &str) -> Option<usize> {
    rig.skel.names.iter().position(|n| n.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

fn legs_of(rig: &ActorRig) -> Result<Legs, String> {
    let skel = &rig.skel;
    let mut legs = [[0; 3]; 2];
    for (s, side) in SIDES.iter().enumerate() {
        for (b, bone) in ["Thigh", "Calf", "Foot"].iter().enumerate() {
            let name = format!("Bip01 {side} {bone}");
            legs[s][b] = find(rig, &name).ok_or_else(|| format!("no {name}"))?;
            if b > 0 && skel.parents[legs[s][b]] != Some(legs[s][b - 1]) {
                return Err(format!("{name} does not hang from the {}", ["thigh", "calf"][b - 1]));
            }
        }
    }
    let above = |n: usize, mut i: usize| {
        while let Some(p) = skel.parents[i] {
            if p == n {
                return true;
            }
            i = p;
        }
        false
    };
    let body = ["Bip01 NonAccum", "Bip01 Pelvis"]
        .iter()
        .filter_map(|n| find(rig, n))
        .find(|&b| b != 0 && legs.iter().all(|l| above(b, l[0])))
        .ok_or("no Bip01 NonAccum or Bip01 Pelvis above both thighs")?;
    let ankle_bind = legs.map(|l| skel.bind_world[l[2]].translation.z);
    // A four-legged skeleton names its hind legs as a biped's (the dog's, the brahmin's)
    // and stands on its "hands": placing the hind feet alone would tilt it.
    let hip = skel.bind_world[legs[0][0]].translation.z;
    if let Some(hand) = find(rig, "Bip01 L Hand").map(|h| skel.bind_world[h].translation.z) {
        if hand < hip * 0.5 {
            return Err(format!("four-legged: the hands stand {hand:.0} units up, the hips {hip:.0}"));
        }
    }
    Ok(Legs { body, legs, ankle_bind })
}

/// Give every actor root a state, matching its skeleton once.
fn attach(mut commands: Commands, mut stats: ResMut<FootIkStats>, new: Query<(Entity, &ActorRig), Without<FootIk>>, placed: Query<(), With<FootIk>>, body: Option<Res<PlayerBody>>) {
    // The player's body root carries no `ActorRig`: given its state once, like the rest
    // (a fresh state every frame left its feet one ease step from the animation, the
    // step changing with the frame time: the legs twitched on a slope).
    let player = body.as_deref().and_then(PlayerBody::third_rig);
    let all = new.iter().map(|(e, r)| (e, r.clone())).chain(player.filter(|(e, _)| !new.contains(*e) && !placed.contains(*e)));
    for (e, rig) in all {
        let Ok(mut ec) = commands.get_entity(e) else { continue };
        let legs = match legs_of(&rig) {
            Ok(l) => {
                stats.rigged += 1;
                if stats.described.insert(rig.skeleton.clone()) {
                    info!("foot ik: {}: lowers {} and bends both legs", rig.skeleton, rig.skel.names[l.body].as_deref().unwrap_or("?"));
                }
                Some(l)
            }
            Err(why) => {
                if stats.described.insert(rig.skeleton.clone()) {
                    debug!("foot ik: {}: no feet placed ({why})", rig.skeleton);
                }
                None
            }
        };
        ec.try_insert(FootIk { legs, tilt: [Quat::IDENTITY; 2], ..default() });
    }
}

/// The ground under game point `p`: height and normal (game space).
type Ground = Option<(f32, Vec3)>;

/// The drawn landscape surface of exterior cells (smoothed or stored), without `--play`.
#[derive(Default)]
struct LandCache(HashMap<(i32, i32), Option<fnv_world::land_surface::Surface>>);

impl LandCache {
    fn height(&mut self, world: &WorldData, x: f32, y: f32) -> Option<f32> {
        let g = fnv_world::cell_of(x, y);
        let surface = self.0.entry(g).or_insert_with(|| world.land_surface(g)).as_ref()?;
        let (ox, oy) = (g.0 as f32 * fnv_world::CELL_SIZE, g.1 as f32 * fnv_world::CELL_SIZE);
        Some(surface.height_at_clamped(x - ox, y - oy))
    }

    fn ground(&mut self, world: &WorldData, p: Vec3) -> Ground {
        const D: f32 = 8.0;
        let z = self.height(world, p.x, p.y)?;
        let dx = self.height(world, p.x + D, p.y)? - self.height(world, p.x - D, p.y)?;
        let dy = self.height(world, p.x, p.y + D)? - self.height(world, p.x, p.y - D)?;
        Some((z, Vec3::new(-dx / (2.0 * D), -dy / (2.0 * D), 1.0).normalize()))
    }
}

/// Who an actor is, for the checks before its feet are placed.
type Actor = (Entity, &'static ActorRig, &'static ChildOf, &'static InheritedVisibility, Has<Dying>, Has<Corpse>, Has<OnFurniture>);

#[allow(clippy::too_many_arguments)]
fn place(
    time: Res<Time>,
    settings: Res<FootIkSettings>,
    script: Option<Res<ScriptState>>,
    body: Option<Res<PlayerBody>>,
    player_use: Option<Res<PlayerUse>>,
    mut phys: Option<ResMut<PhysState>>,
    world: Option<Res<WorldData>>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    actors: Query<Actor>,
    parents: Query<&ChildOf>,
    globals: Query<&GlobalTransform>,
    mut states: Query<&mut FootIk>,
    mut transforms: Query<&mut Transform>,
    mut stats: ResMut<FootIkStats>,
    (mut land, inertia): (Local<LandCache>, Option<Res<nvf_play::inertia::MovementInertia>>),
) {
    let dt = time.delta_secs();
    let lock_on = inertia.as_deref().is_some_and(|i| i.matches_motion()) && std::env::var_os("NVF_MM_NO_LOCK").is_none();
    let skate_aid = std::env::var("NVF_PLAY_TEST_FOOT_SKATE").ok();
    let skating = skate_aid.is_some();
    if let Some(path) = skate_aid.filter(|p| p.ends_with(".csv") && stats.skate_clock == 0.0 && stats.skate_csv.is_none()) {
        use std::io::Write;
        match std::fs::File::create(&path) {
            Ok(f) => {
                let mut w = std::io::BufWriter::new(f);
                let _ = writeln!(w, "time,dt,side,lift,animated_speed,shown_speed,locked,lock_weight,root_x,root_y");
                info!("foot skate: writing {path}");
                stats.skate_csv = Some(w);
            }
            Err(e) => warn!("NVF_PLAY_TEST_FOOT_SKATE: {path}: {e}"),
        }
    }
    stats.skate_clock += dt;
    let t0 = std::time::Instant::now();
    let ease = 1.0 - (-EASE_RATE * time.delta_secs()).exp();
    let cam = camera.iter().next().map(|g| g.translation());
    if let Some(p) = phys.as_deref_mut() {
        p.world.update_queries();
    }

    // The player's body first, then the NPCs: (root, rig, may stand, the character's key,
    // the player's: its feet locked and measured).
    let mut todo: Vec<(Entity, ActorRig, bool, Option<Key>, bool)> = Vec::new();
    if let Some((root, rig)) = body.as_deref().and_then(PlayerBody::third_rig) {
        let ch = phys.as_deref().and_then(|p| p.player.as_ref());
        let alive = !is_dead(script.as_deref(), FormId::PLAYER_REF.0);
        let stands = alive && ch.is_none_or(|c| c.grounded && !c.swimming) && !player_use.as_deref().is_some_and(PlayerUse::in_use);
        todo.push((root, rig, stands, ch.map(|c| c.key()), true));
    }
    for (e, rig, _, vis, dying, corpse, seated) in &actors {
        let ch = phys.as_deref().and_then(|p| p.npcs.get(&rig.reference)).map(|b| &b.ch);
        let alive = vis.get() && !dying && !corpse && !is_dead(script.as_deref(), rig.reference);
        let grounded = ch.is_none_or(|c| c.grounded && !c.swimming);
        let stands = alive && !seated && grounded;
        if let Some(k) = [!alive, seated, !grounded].iter().position(|&b| b) {
            stats.out[k] += 1;
        }
        todo.push((e, rig.clone(), stands, ch.map(|c| c.key()), false));
    }

    for (root, rig, stands, key, player) in todo {
        let Ok(mut st) = states.get_mut(root) else { continue };
        let st = &mut *st;
        let Some(legs) = st.legs else { continue };
        let Ok(root_t) = transforms.get(root).copied() else { continue };
        let parent = parents.get(root).ok().and_then(|p| globals.get(p.parent()).ok()).map_or(Affine3A::IDENTITY, |g| g.affine());
        // Actor space (the root's own frame, z up, skeleton units) to Bevy world.
        let to_world = parent * root_t.compute_affine();
        let near = cam.is_none_or(|c| c.distance(to_world.translation.into()) <= ACTIVE_RADIUS);
        let locking = player && lock_on && stands && near;
        let placing = settings.enabled && stands && near;
        let active = placing || locking;
        if stands && !near {
            stats.out[3] += 1;
        }
        if active {
            stats.active += 1;
        }
        if !active && st.at_rest() && st.written_body.is_none() && st.written.iter().flatten().all(Option::is_none) {
            continue;
        }

        // The animated pose: local transforms with what was written last frame taken out.
        let local = |i: usize, st: &FootIk| -> Option<Transform> {
            let mut t = *transforms.get(rig.nodes[i]).ok()?;
            if i == legs.body {
                t.translation = animated_translation(t.translation, st.written_body);
            }
            for (s, leg) in legs.legs.iter().enumerate() {
                if let Some(b) = leg.iter().position(|&n| n == i) {
                    t.rotation = animated_rotation(t.rotation, st.written[s][b]);
                }
            }
            Some(t)
        };
        let actor_space = |i: usize, st: &FootIk| -> Option<Affine3A> {
            let mut m = Affine3A::IDENTITY;
            let mut n = i;
            while n != 0 {
                m = local(n, st)?.compute_affine() * m;
                n = rig.skel.parents[n]?;
            }
            Some(m)
        };
        let Some(body_parent) = rig.skel.parents[legs.body].and_then(|p| actor_space(p, st)) else { continue };
        let Some(body_local) = local(legs.body, st) else { continue };
        // Per side and bone: the actor-space transform and the local rotation, animated.
        let mut joints = [[Affine3A::IDENTITY; 3]; 2];
        let mut animated = [[Quat::IDENTITY; 3]; 2];
        let mut ok = true;
        for s in 0..2 {
            for b in 0..3 {
                match (actor_space(legs.legs[s][b], st), local(legs.legs[s][b], st)) {
                    (Some(m), Some(l)) => (joints[s][b], animated[s][b]) = (m, l.rotation),
                    _ => ok = false,
                }
            }
        }
        if !ok {
            continue;
        }

        // The ground under each ankle, against the root.
        let mut want_h = [0.0; 2];
        let mut want_tilt = [Quat::IDENTITY; 2];
        if placing {
            let from_world = to_world.inverse();
            let root_game = space::to_game(to_world.translation.into());
            for s in 0..2 {
                let ankle = Vec3::from(joints[s][2].translation);
                let g = space::to_game(to_world.transform_point3(ankle));
                let ground: Ground = if let Some(p) = phys.as_deref_mut() {
                    let filter = Filter { exclude: key.into_iter().collect(), exclude_layers: layers::character_ignores().union(layers::actor_volumes()), ..Default::default() };
                    // On a model's drawn floor, not its collision when that lies lower
                    // (`PhysState::drawn_over`, floating-actors.md §7).
                    let hit = p.world.queries().ray([g[0], g[1], root_game[2] + PROBE_UP], [0.0, 0.0, -1.0], PROBE_UP + PROBE_DOWN, &filter);
                    hit.map(|hit| (p.drawn_over(&hit), Vec3::from(hit.normal)))
                } else {
                    world.as_deref().and_then(|w| land.ground(w, Vec3::from(g)))
                };
                stats.probes += 1;
                let Some((z, n)) = ground else { continue };
                stats.found += 1;
                let at = from_world.transform_point3(space::to_bevy([g[0], g[1], z]));
                if at.z > MAX_RAISE {
                    stats.inside += 1;
                    continue;
                }
                want_h[s] = at.z;
                let n_actor = from_world.transform_vector3(space::to_bevy(n.to_array())).normalize_or_zero();
                want_tilt[s] = tilt_onto(n_actor, planted(ankle.z - legs.ankle_bind[s]));
            }
        }
        let (want_drop, want_raise) = drop_and_raise(want_h);
        st.drop += (want_drop - st.drop) * ease;
        for s in 0..2 {
            st.raise[s] += (want_raise[s] - st.raise[s]) * ease;
            st.tilt[s] = st.tilt[s].slerp(want_tilt[s], ease).normalize();
        }

        if !active && st.at_rest() {
            // Back on the animation: give back what no clip poses.
            if let (Some((base, _)), Ok(mut t)) = (st.written_body, transforms.get_mut(rig.nodes[legs.body])) {
                t.translation = base;
            }
            for s in 0..2 {
                for b in 0..3 {
                    if let (Some((base, _)), Ok(mut t)) = (st.written[s][b], transforms.get_mut(rig.nodes[legs.legs[s][b]])) {
                        t.rotation = base;
                    }
                }
            }
            *st = FootIk { legs: Some(legs), tilt: [Quat::IDENTITY; 2], ..default() };
            continue;
        }

        // Foot locking: each animated ankle's place over the ground, whether it is down,
        // and where it is held (actor space).
        let from_world = to_world.inverse();
        let lock_ease = 1.0 - (-LOCK_RATE * dt).exp();
        let mut held = [None; 2];
        let mut down = [false; 2];
        let (mut lifts, mut animated_speed) = ([0.0f32; 2], [0.0f32; 2]);
        for s in 0..2 {
            let ankle = Vec3::from(joints[s][2].translation);
            let world = to_world.transform_point3(ankle);
            let moving = st.last_ankle[s].filter(|_| dt > 0.0).map_or(0.0, |l| {
                let d = from_world.transform_vector3(world - l);
                Vec2::new(d.x, d.y).length() / dt
            });
            st.last_ankle[s] = Some(world);
            down[s] = ankle.z - legs.ankle_bind[s] < LOCK_LIFT;
            if skating && player && down[s] && dt > 0.0 {
                stats.skate.0.push(moving);
            }
            lifts[s] = ankle.z - legs.ankle_bind[s];
            animated_speed[s] = moving;
            if !locking {
                st.locked[s] = false;
            } else if down[s] && moving < LOCK_SPEED {
                if !st.locked[s] {
                    st.lock_at[s] = Some(world);
                    st.locked[s] = true;
                    stats.locks += 1;
                }
            } else {
                st.locked[s] = false;
            }
            if let (true, Some(at)) = (st.locked[s], st.lock_at[s]) {
                let d = from_world.transform_point3(at) - ankle;
                if Vec2::new(d.x, d.y).length() > LOCK_DRIFT {
                    st.locked[s] = false;
                }
            }
            st.lock_w[s] += (f32::from(u8::from(st.locked[s])) - st.lock_w[s]) * lock_ease;
            if st.lock_w[s] < REST && !st.locked[s] {
                st.lock_w[s] = 0.0;
            }
            held[s] = st.lock_at[s].map(|at| (from_world.transform_point3(at), st.lock_w[s]));
        }

        // Lower the body.
        let lower = Vec3::Z * st.drop;
        let body_t = body_local.translation + body_parent.matrix3.inverse() * lower;
        if let Ok(mut t) = transforms.get_mut(rig.nodes[legs.body]) {
            t.translation = body_t;
            st.written_body = Some((body_local.translation, body_t));
        }
        // Bend each leg and lay its foot on the slope.
        for s in 0..2 {
            let [thigh, calf, foot] = joints[s];
            let rot = |m: &Affine3A| m.to_scale_rotation_translation().1;
            let (h, k, a) = (Vec3::from(thigh.translation) + lower, Vec3::from(calf.translation) + lower, Vec3::from(foot.translation) + lower);
            let mut target = a + Vec3::Z * st.raise[s];
            // Held where it came down: over the ground the held place, its height the
            // animation's and the ground's.
            if let Some((at, w)) = held[s].filter(|(_, w)| *w > 0.0) {
                let at = at + lower;
                target.x += (at.x - target.x) * w;
                target.y += (at.y - target.y) * w;
            }
            if skating && player {
                let shown = to_world.transform_point3(target);
                let shown_speed = st.shown_last[s].filter(|_| dt > 0.0).map(|prev| {
                    let d = from_world.transform_vector3(shown - prev);
                    Vec2::new(d.x, d.y).length() / dt
                });
                if let (Some(v), true) = (shown_speed, down[s]) {
                    stats.skate.1.push(v);
                }
                st.shown_last[s] = Some(shown);
                let clock = stats.skate_clock;
                if let Some(w) = stats.skate_csv.as_mut() {
                    use std::io::Write;
                    let r = to_world.translation;
                    let _ = writeln!(
                        w,
                        "{:.4},{:.4},{},{:.2},{:.1},{:.1},{},{:.3},{:.1},{:.1}",
                        clock, dt, SIDES[s], lifts[s], animated_speed[s], shown_speed.unwrap_or(0.0), u8::from(st.locked[s]), st.lock_w[s], r.x, r.z
                    );
                }
            }
            let (leg_turn, calf_turn) = two_bone(h, k, a, target, Vec3::Y);
            let thigh_r = (leg_turn * rot(&thigh)).normalize();
            let calf_r = (leg_turn * calf_turn * rot(&calf)).normalize();
            let foot_r = (st.tilt[s] * rot(&foot)).normalize();
            // The thigh's parent as animated (the body's lowering does not turn it).
            let thigh_parent = rot(&thigh) * animated[s][0].inverse();
            let new = [(thigh_parent.inverse() * thigh_r).normalize(), (thigh_r.inverse() * calf_r).normalize(), (calf_r.inverse() * foot_r).normalize()];
            for b in 0..3 {
                if let Ok(mut t) = transforms.get_mut(rig.nodes[legs.legs[s][b]]) {
                    t.rotation = new[b];
                    st.written[s][b] = Some((animated[s][b], new[b]));
                }
            }
        }

        let raise = st.raise[0].max(st.raise[1]);
        let tilt = st.tilt.iter().map(|t| t.angle_between(Quat::IDENTITY)).fold(0.0, f32::max);
        if -st.drop > 1.0 || raise > 1.0 {
            stats.placed.insert(rig.reference);
        }
        stats.largest_drop = stats.largest_drop.max(-st.drop);
        if raise > stats.largest_raise {
            (stats.largest_raise, stats.largest_raise_by) = (raise, rig.reference);
        }
        stats.largest_tilt = stats.largest_tilt.max(tilt);
    }
    stats.total += t0.elapsed();
    stats.frames += 1;
    if skating {
        stats.skate.2 += dt;
        if stats.skate.2 >= 10.0 {
            stats.skate.2 = 0.0;
            let summary = |v: &mut Vec<f32>| {
                v.sort_by(f32::total_cmp);
                let at = |q: f32| v.get(((v.len().max(1) - 1) as f32 * q) as usize).copied().unwrap_or(0.0);
                let mean = if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 };
                let out = format!("mean {mean:.0}, median {:.0}, 90th percentile {:.0}", at(0.5), at(0.9));
                v.clear();
                out
            };
            let (a, b) = (summary(&mut stats.skate.0), summary(&mut stats.skate.1));
            info!("foot skate: planted feet over the ground, units/s: animated {a}; shown {b}; {} locks so far", stats.locks);
        }
    }
}

/// The run's foot placement when the app exits (smoke tests read it).
fn report(mut exit: MessageReader<AppExit>, stats: Res<FootIkStats>) {
    if exit.read().next().is_none() {
        return;
    }
    let player = stats.placed.contains(&FormId::PLAYER_REF.0);
    info!(
        "foot ik: {} actors with legs, {} actors stood on uneven ground ({}the player), largest body drop {:.1} units, foot raise {:.1} units ({:08X}), foot tilt {:.0}°; ground found under {} of {} feet looked at ({} too high to stand on); {} actor-frames placed, left out {} hidden or dead, {} seated, {} off the ground, {} far; {:.3} ms a frame",
        stats.rigged,
        stats.placed.len(),
        if player { "" } else { "not " },
        stats.largest_drop,
        stats.largest_raise,
        stats.largest_raise_by,
        stats.largest_tilt.to_degrees(),
        stats.found,
        stats.probes,
        stats.inside,
        stats.active,
        stats.out[0],
        stats.out[1],
        stats.out[2],
        stats.out[3],
        if stats.frames > 0 { stats.total.as_secs_f64() * 1000.0 / stats.frames as f64 } else { 0.0 }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-3
    }

    /// A leg standing straight down from a hip 60 up: knee at 32, ankle at 6.
    fn leg() -> (Vec3, Vec3, Vec3) {
        (Vec3::new(0.0, 0.0, 60.0), Vec3::new(0.0, 0.5, 32.0), Vec3::new(0.0, 0.0, 6.0))
    }

    fn solve(t: Vec3) -> (Vec3, Vec3) {
        let (h, k, a) = leg();
        let (legq, calfq) = two_bone(h, k, a, t, Vec3::Y);
        let k2 = h + legq * (k - h);
        let a2 = k2 + legq * calfq * (a - k);
        (k2, a2)
    }

    #[test]
    fn broken_legs_and_targets_are_left_alone() {
        let (h, k, a) = leg();
        let id = (Quat::IDENTITY, Quat::IDENTITY);
        assert_eq!(two_bone(h, Vec3::NAN, a, a, Vec3::Y), id);
        assert_eq!(two_bone(h, k, a, Vec3::new(0.0, f32::INFINITY, 0.0), Vec3::Y), id);
        // Bones so unequal that rounding crosses the clamp bounds.
        assert_eq!(two_bone(Vec3::ZERO, Vec3::new(0.0, 0.0, 1e9), Vec3::new(0.0, 0.0, 1e9 + 1e-3), Vec3::ONE, Vec3::Y), id);
        assert_eq!(tilt_onto(Vec3::new(0.1, 0.0, 1.0), f32::NAN), Quat::IDENTITY);
    }

    #[test]
    fn the_ankle_reaches_a_raised_target() {
        let (h, k, a) = leg();
        let t = a + Vec3::Z * 15.0;
        let (k2, a2) = solve(t);
        assert!(close(a2, t), "{a2} vs {t}");
        // Bone lengths kept, the knee forward of the hip-ankle line.
        assert!(((k2 - h).length() - (k - h).length()).abs() < 1e-3);
        assert!(((a2 - k2).length() - (a - k).length()).abs() < 1e-3);
        assert!(k2.y > k.y, "the knee bends forward");
    }

    #[test]
    fn a_target_out_of_reach_gives_a_straight_leg_towards_it() {
        let (h, _, a) = leg();
        let (_, a2) = solve(a - Vec3::Z * 50.0);
        assert!(a2.z < a.z && a2.z > a.z - 5.0);
        assert!(a2.x.abs() < 1e-3);
        let _ = h;
    }

    #[test]
    fn a_straight_leg_bends_its_knee_forward() {
        let h = Vec3::new(0.0, 0.0, 60.0);
        let k = Vec3::new(0.0, 0.0, 33.0);
        let a = Vec3::new(0.0, 0.0, 6.0);
        let t = a + Vec3::Z * 10.0;
        let (legq, calfq) = two_bone(h, k, a, t, Vec3::Y);
        let k2 = h + legq * (k - h);
        let a2 = k2 + legq * calfq * (a - k);
        assert!(close(a2, t), "{a2}");
        assert!(k2.y > 0.5, "{k2}");
    }

    #[test]
    fn the_lower_foot_lowers_the_body() {
        let (drop, raise) = drop_and_raise([8.0, -6.0]);
        assert_eq!(drop, -6.0);
        assert_eq!(raise, [14.0, 0.0]);
        let (drop, raise) = drop_and_raise([3.0, 2.0]);
        assert_eq!(drop, 0.0);
        assert_eq!(raise, [3.0, 2.0]);
        let (drop, _) = drop_and_raise([-100.0, 0.0]);
        assert_eq!(drop, -MAX_DROP);
    }

    #[test]
    fn a_foot_tilts_onto_the_slope_within_its_limit() {
        let slope = |deg: f32| Vec3::new(0.0, -deg.to_radians().sin(), deg.to_radians().cos());
        let t = tilt_onto(slope(20.0), 1.0);
        assert!(close(t * Vec3::Z, slope(20.0)));
        let t = tilt_onto(slope(60.0), 1.0);
        assert!((t.angle_between(Quat::IDENTITY) - MAX_TILT).abs() < 1e-3);
        assert_eq!(tilt_onto(slope(20.0), 0.0), Quat::IDENTITY);
        assert_eq!(planted(0.0), 1.0);
        assert_eq!(planted(LIFTED + 1.0), 0.0);
    }

    #[test]
    fn a_written_translation_is_not_taken_for_the_animation() {
        let (base, w) = (Vec3::new(0.0, 0.0, 65.0), Vec3::new(0.0, 0.0, 60.0));
        assert_eq!(animated_translation(w, Some((base, w))), base);
        assert_eq!(animated_translation(Vec3::Z * 66.0, Some((base, w))), Vec3::Z * 66.0);
    }
}
