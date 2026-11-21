//! Modern Feel (`nvf_play::modern_feel`), two parts on the actors other than the player
//! (docs/design/modern-feel-npc.md). NVForever's own design, not in the original: every
//! constant below is an engineering choice tuned by eye [design].
//!
//! - **Grounded footsteps** (`ModernFeel::grounded_footsteps`):
//!   - *Planted turns.* A standing person's lower body keeps its own heading while its
//!     root turns: the skeleton below the root is turned back by the difference (the
//!     lag), so the feet stay where they stand, and the spine turns the upper body
//!     round to face the way the actor faces (at most [`UPPER_MAX`]). Once the lag
//!     passes [`START_LAG`], or the actor stops turning ([`SETTLE_TIME`]) with more than
//!     [`SETTLE_LAG`] left, the feet step round: the lower body catches up at a speed
//!     by the lag and the turn clip plays at the rate for that speed
//!     ([`turn_clip_rate`]), through `Locomotion::feel_turn`. Small turns (a gunman
//!     following its target) no longer spin the whole body on the idle, and a turn's
//!     steps go with its rotation.
//!   - *Feet along the way they go.* A person stepping off its facing (a fighter
//!     strafing on a diagonal) plays the nearest of its four direction clips; the lower
//!     body is turned by what is left over (at most 45°), the spine turns back, so the
//!     legs walk the way the body travels and do not slide sideways.
//!   - *Strides that match the ground speed in a walk/run fade*
//!     (`fnv_actor::locomotion::Blend::stride_match`): both clips advance at the cycle
//!     rate that covers the ground at the actor's speed. Outside fades the rate already
//!     matched (`fnv_actor::locomotion::choose`).
//! - **NPC aim** (`ModernFeel::npc_aim`): a fighting gunman (`npc_combat::Fighters::
//!   aim_at`) with its gun drawn and not reloading or blocking has its spine bent, over
//!   the animation's aim (the aim clips' up and down forms keep doing the coarse
//!   pitch), so the barrel (the weapon model's +X, from the `Muzzle`) points at where
//!   its shots go: a yaw and a pitch, solved in three passes from the muzzle and barrel
//!   the bent pose gives, limited ([`AIM_YAW_MAX`], [`AIM_PITCH_MAX`]), eased in and out
//!   and followed smoothly ([`AIM_EASE`]).
//!
//! Both write the same bones (the root's child above the pelvis, and the `Bip01 Spine…`
//! chain), once, after `locomotion::drive` and the turns of conversations and scripts,
//! before head tracking and foot IK (which then build on this pose). What they wrote is
//! remembered so a bone no clip posed since is not turned twice
//! (`head_look::animated_rotation`); switched off, every bone eases back to the
//! animation and is given back.
//!
//! Log, starting `grounded footsteps:` and `npc aim:`: every [`REPORT_SECONDS`] and at
//! the end, the frames worked on, the steps, the foot slip against the ground speed
//! (before: the old blend; after: stride-matched), and the barrel's error from its
//! target in degrees before (the animation alone) and after the bend.

use crate::actors::ActorWorld;
use crate::attack_anims::CombatPose;
use crate::furniture::OnFurniture;
use crate::limbs::ActorRig;
use crate::locomotion::{heading_of_rotation, Dying, Locomotion};
use crate::npc_combat::Fighters;
use crate::weapons::{Muzzle, WeaponHold};
use bevy::math::Affine3A;
use bevy::prelude::*;
use fnv_actor::locomotion::{Move, STAND_SPEED};
use fnv_actor::weapon::Stage;
use fnv_sim::FormId;
use nvf_play::modern_feel::ModernFeel;
use nvf_play::space;
use std::collections::{HashMap, HashSet};
use std::f32::consts::{PI, TAU};

const fn deg(d: f32) -> f32 {
    d * PI / 180.0
}

/// The turn speed the turn clips play at rate 1 (they carry no rotation of their own,
/// actors.md §8.9; 120°/s was what they were played at) [design].
pub const TURN_CLIP_SPEED: f32 = deg(120.0);
/// The turn clips' rates are kept within this [design].
pub const TURN_CLIP_RATE: (f32, f32) = (0.7, 1.8);
/// The lag past which the feet step round [design].
pub const START_LAG: f32 = deg(25.0);
/// With the root still for this long, a lag over [`SETTLE_LAG`] is stepped out [design].
pub const SETTLE_TIME: f32 = 0.35;
pub const SETTLE_LAG: f32 = deg(8.0);
/// The root counts as still below this turn rate.
const STILL_RATE: f32 = deg(20.0);
/// The most the lower body lags: beyond it, it is dragged round [design].
pub const MAX_LAG: f32 = deg(60.0);
/// A step ends within this of the root's heading.
const STEP_DONE: f32 = deg(1.5);
/// The feet step round at the lag × this per second, within these speeds [design].
const STEP_GAIN: f32 = 4.0;
const STEP_SPEED: (f32, f32) = (deg(100.0), deg(270.0));
/// The spine turns the upper body back by at most this [design].
pub const UPPER_MAX: f32 = deg(50.0);
/// How fast the lower body follows the way it travels, per second (exponential).
const MOVE_EASE: f32 = 10.0;
/// The lower body turned to its travel by at most this.
const TRAVEL_MAX: f32 = deg(50.0);
/// How fast a lag is let go of when grounded turns stop, per second.
const RELEASE_EASE: f32 = 8.0;
/// The aim's bend: limits, how fast it follows, how fast it eases in and out.
pub const AIM_YAW_MAX: f32 = deg(40.0);
pub const AIM_PITCH_MAX: f32 = deg(40.0);
pub const AIM_EASE: f32 = 14.0;
const AIM_FADE: f32 = 5.0;
/// The target must lie within this of the root's facing for the bend [design].
pub const AIM_FRONT: f32 = deg(60.0);
const AIM_PASSES: usize = 3;
/// Only actors this near the camera, game units [design: the head tracking's reach].
pub const ACTIVE_RADIUS: f32 = 3000.0;
/// Summary every this many seconds.
pub const REPORT_SECONDS: f32 = 10.0;
/// The walk and run rates counted as natural (for the log).
const NATURAL_RATE: (f32, f32) = (0.75, 1.35);

/// The turn clip's rate for a turn at `turn` radians per second (either way).
pub fn turn_clip_rate(turn: f32) -> f32 {
    (turn.abs() / TURN_CLIP_SPEED).clamp(TURN_CLIP_RATE.0, TURN_CLIP_RATE.1)
}

/// `a` wrapped into (-π, π].
pub fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// The compass direction (radians, clockwise from forward) a move's clip travels.
fn clip_direction(mv: Move) -> Option<f32> {
    match mv {
        Move::Forward | Move::FastForward => Some(0.0),
        Move::Right | Move::FastRight => Some(PI / 2.0),
        Move::Left | Move::FastLeft => Some(-PI / 2.0),
        Move::Backward | Move::FastBackward => Some(PI),
        _ => None,
    }
}

/// The bones bent on an actor.
#[derive(Debug, Clone)]
struct FeelRig {
    /// The child of the root the whole skeleton hangs from (`Bip01`).
    top: usize,
    /// The spine bones, from the pelvis up (at most three).
    spine: Vec<usize>,
    /// A person (planted turns are for people).
    person: bool,
}

fn feel_rig(rig: &ActorRig) -> Option<FeelRig> {
    let skel = &rig.skel;
    let find = |n: &str| skel.names.iter().position(|x| x.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(n))).filter(|&i| i < rig.nodes.len());
    let mut spine = Vec::new();
    for n in ["Bip01 Spine", "Bip01 Spine1", "Bip01 Spine2"] {
        let Some(i) = find(n) else { break };
        if let Some(&last) = spine.last() {
            if skel.parents[i] != Some(last) {
                break;
            }
        }
        spine.push(i);
    }
    let first = *spine.first()?;
    let mut top = first;
    let mut guard = 0;
    while let Some(p) = skel.parents[top] {
        if p == 0 {
            break;
        }
        top = p;
        guard += 1;
        if guard > skel.parents.len() {
            return None;
        }
    }
    (top != 0 && top != first).then_some(FeelRig { top, spine, person: find("Bip01 L Thigh").is_some() && find("Bip01 R Thigh").is_some() })
}

/// On an actor's root: what the two parts keep of it.
#[derive(Component, Default)]
pub struct NpcFeel {
    rig: Option<FeelRig>,
    npc: bool,
    /// The lower body's heading against the root's (radians, clockwise), shown now.
    lag: f32,
    stepping: bool,
    still: f32,
    last_root: Option<f32>,
    /// The aim's bend as applied (yaw counter-clockwise, pitch up), and its weight.
    aim: (f32, f32),
    aim_w: f32,
    /// The weapon node (skeleton index) the muzzle hangs from.
    weapon: Option<usize>,
    /// What was written over the top bone and the spine bones, with the animation's.
    written_top: Option<(Transform, Transform)>,
    written_spine: [Option<(Quat, Quat)>; 3],
    logged_aim: bool,
}

impl NpcFeel {
    fn holds_bones(&self) -> bool {
        self.written_top.is_some() || self.written_spine.iter().any(Option::is_some)
    }
}

#[derive(Resource, Default)]
pub struct NpcFeelStats {
    // Grounded footsteps.
    planted_frames: u64,
    step_frames: u64,
    steps: u64,
    dragged_frames: u64,
    travel_frames: u64,
    largest_lag: f32,
    turners: HashSet<u32>,
    moving_frames: u64,
    clipless_frames: u64,
    natural_frames: u64,
    fade_frames: u64,
    slip_before: f64,
    slip_after: f64,
    fade_slip_before: f64,
    fade_slip_after: f64,
    // NPC aim.
    aim_frames: u64,
    err_before: f64,
    err_after: f64,
    worst_before: f32,
    worst_after: f32,
    under_2: u64,
    limited: u64,
    aimers: HashSet<u32>,
    total: std::time::Duration,
    frames: u64,
    since_report: f32,
}

pub struct NpcFeelPlugin;

impl Plugin for NpcFeelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NpcFeelStats>()
            .add_systems(
                Update,
                (attach, bend)
                    .chain()
                    .after(crate::locomotion::drive)
                    .after(crate::weapons::reparent)
                    .after(crate::dialogue_look::turn_speaker)
                    .after(crate::script_look::turn_bodies)
                    .after(crate::speech_look::face)
                    .after(nvf_play::player::BodySet)
                    .before(crate::head_look::track)
                    .before(crate::foot_ik::place),
            )
            .add_systems(Last, report_at_exit);
    }
}

fn attach(mut commands: Commands, world: Option<Res<ActorWorld>>, new: Query<(Entity, &ActorRig), (With<Locomotion>, Without<NpcFeel>)>) {
    for (e, rig) in &new {
        let npc = world.as_ref().is_some_and(|w| matches!(w.actors.sigs.get(&FormId(rig.base)), Some(b"NPC_") | Some(b"LVLN")));
        commands.entity(e).insert(NpcFeel { rig: feel_rig(rig), npc, ..default() });
    }
}

type Bender = (
    Entity,
    &'static ActorRig,
    &'static mut Locomotion,
    &'static mut NpcFeel,
    Has<Dying>,
    Has<OnFurniture>,
    Option<&'static WeaponHold>,
    Option<&'static CombatPose>,
    &'static InheritedVisibility,
);

fn affine(t: &Transform) -> Affine3A {
    Affine3A::from_scale_rotation_translation(t.scale, t.rotation, t.translation)
}

/// Yaw (counter-clockwise about z) and elevation of a direction.
fn yaw_elev(v: Vec3) -> (f32, f32) {
    (v.y.atan2(v.x), v.z.atan2(v.x.hypot(v.y)))
}

#[allow(clippy::too_many_arguments)]
fn bend(
    time: Res<Time>,
    feel: Option<Res<ModernFeel>>,
    fighters: Option<Res<Fighters>>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    mut actors: Query<Bender>,
    muzzles: Query<(Entity, &Muzzle, &ChildOf)>,
    parents: Query<&ChildOf, Without<Muzzle>>,
    globals: Query<&GlobalTransform>,
    mut transforms: Query<&mut Transform>,
    mut stats: ResMut<NpcFeelStats>,
    mut muzzle_of: Local<HashMap<u32, (Entity, Entity)>>,
    mut aid_off: Local<Option<bool>>,
) {
    let t0 = std::time::Instant::now();
    let dt = time.delta_secs();
    // Test aid `NVF_PLAY_TEST_FEEL_NPC_OFF=1`: both parts off for the run (the original's
    // look, for side-by-side screenshots); the log still measures the animation's aim.
    let off = *aid_off.get_or_insert_with(|| std::env::var_os("NVF_PLAY_TEST_FEEL_NPC_OFF").is_some_and(|v| v == "1"));
    let (turns_on, aim_on) = feel.as_deref().filter(|_| !off).map_or((false, false), |f| (f.grounded_footsteps(), f.npc_aim()));
    let measure_aim = aim_on || (off && feel.is_some());
    let cam = camera.iter().next().map(|g| g.translation());
    muzzle_of.clear();
    if measure_aim {
        for (e, m, p) in &muzzles {
            muzzle_of.insert(m.reference, (e, p.parent()));
        }
    }
    for (e, rig, mut lo, mut st, dying, seated, hold, pose, vis) in &mut actors {
        let st = &mut *st;
        let Some(fr) = st.rig.clone() else {
            lo.feel_turn = None;
            continue;
        };
        let Ok(root_t) = transforms.get(e).copied() else { continue };
        let parent = parents.get(e).ok().and_then(|p| globals.get(p.parent()).ok()).map_or(Affine3A::IDENTITY, |g| g.affine());
        let to_world = parent * root_t.compute_affine();
        let near = cam.is_none_or(|c| c.distance(Vec3::from(to_world.translation)) <= ACTIVE_RADIUS);
        let alive = vis.get() && !dying;
        let root_h = heading_of_rotation(root_t.rotation);

        // ---- grounded footsteps: the lower body's lag behind the root ----------------
        let turning = turns_on && alive && near && fr.person && st.npc && !seated && !lo.hold && !lo.in_air && !lo.swimming;
        lo.blend.stride_match = turns_on;
        if turns_on && near && alive {
            slip_stats(&lo, &mut stats);
        }
        if turning {
            let speed = lo.velocity[0].hypot(lo.velocity[1]);
            let moving = speed >= STAND_SPEED || lo.blend.cur.mv.is_moving();
            let root_rate = match st.last_root {
                Some(h) if dt > 0.0 => wrap(root_h - h) / dt,
                _ => 0.0,
            };
            // The root turned this frame: the lower body stays, so its lag grows by it.
            let mut lag = wrap(st.lag - root_rate * dt);
            if moving {
                let travel = lo.velocity[0].atan2(lo.velocity[1]);
                let want = match clip_direction(lo.blend.cur.mv) {
                    Some(d) if speed >= STAND_SPEED => wrap(travel - d).clamp(-TRAVEL_MAX, TRAVEL_MAX),
                    _ => 0.0,
                };
                if want.abs() > deg(2.0) {
                    stats.travel_frames += 1;
                }
                lag += (want - lag) * (1.0 - (-MOVE_EASE * dt).exp());
                st.stepping = false;
                st.still = 0.0;
                lo.feel_turn = None;
            } else {
                st.still = if root_rate.abs() < STILL_RATE { st.still + dt } else { 0.0 };
                if lag.abs() > MAX_LAG {
                    lag = MAX_LAG.copysign(lag);
                    stats.dragged_frames += 1;
                }
                if !st.stepping && (lag.abs() > START_LAG || (st.still > SETTLE_TIME && lag.abs() > SETTLE_LAG)) {
                    st.stepping = true;
                    stats.steps += 1;
                    stats.turners.insert(rig.reference);
                }
                if st.stepping {
                    let speed = (lag.abs() * STEP_GAIN).clamp(STEP_SPEED.0, STEP_SPEED.1);
                    lag -= lag.signum() * lag.abs().min(speed * dt);
                    if lag.abs() < STEP_DONE {
                        st.stepping = false;
                    }
                    stats.step_frames += 1;
                } else {
                    stats.planted_frames += 1;
                }
                // The lower body's own turn rate (heading, clockwise: the root's heading
                // and the lag, against last frame's); `Locomotion::feel_turn` is
                // counter-clockwise.
                let body_rate = match st.last_root {
                    Some(h) if dt > 0.0 => wrap(root_h + lag - h - st.lag) / dt,
                    _ => 0.0,
                };
                lo.feel_turn = Some(if st.stepping { -body_rate } else { 0.0 });
            }
            stats.largest_lag = stats.largest_lag.max(lag.abs());
            st.lag = lag;
        } else {
            lo.feel_turn = None;
            st.stepping = false;
            st.still = 0.0;
            st.lag *= (-RELEASE_EASE * dt).exp();
            if st.lag.abs() < 1e-3 || !alive {
                st.lag = 0.0;
            }
        }
        st.last_root = Some(root_h);

        // ---- npc aim: who aims, and at what ---------------------------------------------
        let target = fighters.as_deref().and_then(|f| f.aim_at(rig.reference));
        let drawn = hold.is_none_or(|h| h.anim.stage == Stage::Drawn);
        let posed = pose.is_some_and(|p| p.fighting && !p.blocking && p.attack.as_ref().is_none_or(|a| !a.reload && !a.melee));
        let muzzle = muzzle_of.get(&rig.reference).copied();
        // Only a target in front (within `AIM_FRONT` of the way the root faces): one behind
        // or to the side is turned to first, by the AI.
        let in_front = target.is_some_and(|t| {
            let a = to_world.inverse().transform_point3(space::to_bevy(t));
            a.x.atan2(a.y).abs() <= AIM_FRONT
        });
        let would_aim = alive && near && drawn && posed && in_front && muzzle.is_some();
        // With the test aid the pipeline runs to measure, and bends nothing.
        let aiming = measure_aim && would_aim;
        st.aim_w += ((if aiming { 1.0 } else { 0.0 }) - st.aim_w) * (1.0 - (-AIM_FADE * dt).exp());
        if !aiming && st.aim_w < 0.01 {
            st.aim_w = 0.0;
        }

        if !alive {
            // Dead or hidden: the death pose or the ragdoll owns the bones now.
            (st.lag, st.aim, st.aim_w, st.stepping) = (0.0, (0.0, 0.0), 0.0, false);
            st.written_top = None;
            st.written_spine = [None; 3];
            continue;
        }
        let twist = st.lag.clamp(-UPPER_MAX, UPPER_MAX);
        let idle = st.lag.abs() < 1e-4 && st.aim_w == 0.0 && st.aim.0.abs() < 1e-4 && st.aim.1.abs() < 1e-4;
        if idle && !st.holds_bones() {
            continue;
        }
        // The animated pose of the bones written.
        let Ok(top_now) = transforms.get(rig.nodes[fr.top]).copied() else { continue };
        let top_anim = match st.written_top {
            Some((base, w)) if top_now.translation.abs_diff_eq(w.translation, 1e-4) && top_now.rotation.abs_diff_eq(w.rotation, 1e-6) => base,
            _ => top_now,
        };
        let n = fr.spine.len();
        let mut spine_anim = [Quat::IDENTITY; 3];
        for (k, &i) in fr.spine.iter().enumerate() {
            let Ok(t) = transforms.get(rig.nodes[i]) else { continue };
            spine_anim[k] = crate::head_look::animated_rotation(t.rotation, st.written_spine[k]);
        }
        if idle {
            // Back on the animation: give the bones back.
            if let Ok(mut t) = transforms.get_mut(rig.nodes[fr.top]) {
                *t = top_anim;
            }
            for (k, &i) in fr.spine.iter().enumerate() {
                if let Ok(mut t) = transforms.get_mut(rig.nodes[i]) {
                    t.rotation = spine_anim[k];
                }
            }
            st.written_top = None;
            st.written_spine = [None; 3];
            st.aim = (0.0, 0.0);
            continue;
        }

        // The top bone turned back by the lag (about the root's z, through its origin).
        let back = Quat::from_rotation_z(-st.lag);
        let top_new = Transform { translation: back * top_anim.translation, rotation: (back * top_anim.rotation).normalize(), scale: top_anim.scale };
        // Actor-space rotations: the spine's parent and the spine bones, as animated
        // under the turned top bone.
        let local_of = |i: usize| -> Option<Transform> {
            if i == fr.top {
                return Some(top_new);
            }
            if let Some(k) = fr.spine.iter().position(|&s| s == i) {
                let t = transforms.get(rig.nodes[i]).ok()?;
                return Some(Transform { rotation: spine_anim[k], ..*t });
            }
            transforms.get(rig.nodes[i]).ok().copied()
        };
        let actor_space = |mut i: usize| -> Option<Affine3A> {
            let mut m = Affine3A::IDENTITY;
            let mut guard = 0;
            while i != 0 {
                m = affine(&local_of(i)?) * m;
                i = rig.skel.parents[i]?;
                guard += 1;
                if guard > rig.skel.parents.len() {
                    return None;
                }
            }
            Some(m)
        };
        let Some(spine_parent) = rig.skel.parents[fr.spine[0]].and_then(actor_space) else { continue };
        let wp = spine_parent.to_scale_rotation_translation().1;
        let mut w = [Quat::IDENTITY; 3];
        let mut acc = wp;
        for k in 0..n {
            acc *= spine_anim[k];
            w[k] = acc;
        }
        // New local rotations of the spine for extra actor-space turns `x` (cumulative).
        let locals = |x: &[Quat; 3]| -> [Quat; 3] {
            let mut out = [Quat::IDENTITY; 3];
            let mut above = wp;
            for k in 0..n {
                let now = x[k] * w[k];
                out[k] = (above.inverse() * now).normalize();
                above = now;
            }
            out
        };
        let share = |k: usize| (k + 1) as f32 / n as f32;
        let twist_x = |k: usize| Quat::from_rotation_z(twist * share(k));

        // The aim: the barrel's muzzle and axis in actor space for a bend.
        let mut aim_x = [Quat::IDENTITY; 3];
        if let (Some(target), Some((m_e, w_e))) = (target.filter(|_| st.aim_w > 0.0), muzzle) {
            if st.weapon.is_none_or(|i| rig.nodes.get(i) != Some(&w_e)) {
                st.weapon = rig.nodes.iter().position(|&x| x == w_e);
            }
            let weapon = st.weapon;
            let hand = hold.map(|h| h.parent).or_else(|| weapon.and_then(|i| rig.skel.parents[i]));
            let path_ok = hand.is_some_and(|h| {
                let mut i = h;
                let mut guard = 0;
                loop {
                    if i == *fr.spine.last().expect("a spine") {
                        break true;
                    }
                    match rig.skel.parents.get(i).copied().flatten() {
                        Some(p) if guard <= rig.skel.parents.len() => {
                            i = p;
                            guard += 1;
                        }
                        _ => break false,
                    }
                }
            });
            let muzzle_local = transforms.get(m_e).ok().copied();
            if let (true, Some(weapon), Some(hand), Some(muzzle_local)) = (path_ok, weapon, hand, muzzle_local) {
                let tgt = to_world.inverse().transform_point3(space::to_bevy(target));
                // The path from the spine's top down to the weapon node.
                let mut below = vec![weapon];
                let mut i = hand;
                while i != *fr.spine.last().expect("a spine") {
                    below.push(i);
                    i = rig.skel.parents[i].expect("checked");
                }
                below.reverse();
                let below_m = below.iter().try_fold(Affine3A::IDENTITY, |m, &i| local_of(i).map(|t| m * affine(&t)));
                let spine_pos: Vec<Transform> = fr.spine.iter().filter_map(|&i| local_of(i)).collect();
                if let (Some(below_m), true) = (below_m, spine_pos.len() == n) {
                    let weapon_tail = below_m * affine(&muzzle_local);
                    // The spine's parent placed, then the bent spine, then the rest.
                    let eval = |x: &[Quat; 3]| -> (Vec3, Vec3) {
                        let l = locals(x);
                        let mut m = spine_parent;
                        for k in 0..n {
                            m *= Affine3A::from_scale_rotation_translation(spine_pos[k].scale, l[k], spine_pos[k].translation);
                        }
                        let wm = m * below_m;
                        let mm = m * weapon_tail;
                        (Vec3::from(mm.translation), wm.transform_vector3(Vec3::X).normalize_or_zero())
                    };
                    let twisted: [Quat; 3] = std::array::from_fn(|k| if k < n { twist_x(k) } else { Quat::IDENTITY });
                    let unbent = eval(&twisted);
                    // Pitch turns about the level axis across the way to the target.
                    let fwd = (tgt - unbent.0).with_z(0.0).normalize_or(Vec3::Y);
                    let axis = fwd.cross(Vec3::Z).normalize_or(Vec3::X);
                    let make = |yaw: f32, pitch: f32| -> [Quat; 3] {
                        std::array::from_fn(|k| if k < n { Quat::from_axis_angle(axis, pitch * share(k)) * Quat::from_rotation_z(yaw * share(k)) * twist_x(k) } else { Quat::IDENTITY })
                    };
                    let err = |(m, b): (Vec3, Vec3)| b.angle_between((tgt - m).normalize_or(b)).to_degrees();
                    let before = err(unbent);
                    // Solve from where the bend is now.
                    let (mut yaw, mut pitch) = if st.aim_w > 0.05 && !off { (st.aim.0 / st.aim_w, st.aim.1 / st.aim_w) } else { (0.0, 0.0) };
                    for _ in 0..if off { 0 } else { AIM_PASSES } {
                        let (m, b) = eval(&make(yaw, pitch));
                        let d = (tgt - m).normalize_or(b);
                        let (by, be) = yaw_elev(b);
                        let (dy, de) = yaw_elev(d);
                        yaw = (yaw + wrap(dy - by)).clamp(-AIM_YAW_MAX, AIM_YAW_MAX);
                        pitch = (pitch + (de - be)).clamp(-AIM_PITCH_MAX, AIM_PITCH_MAX);
                    }
                    let k = 1.0 - (-AIM_EASE * dt).exp();
                    st.aim.0 += (yaw * st.aim_w - st.aim.0) * k;
                    st.aim.1 += (pitch * st.aim_w - st.aim.1) * k;
                    aim_x = make(st.aim.0, st.aim.1);
                    let limited = yaw.abs() >= AIM_YAW_MAX - 1e-4 || pitch.abs() >= AIM_PITCH_MAX - 1e-4;
                    stats.limited += u64::from(aiming && limited && st.aim_w > 0.9);
                    // Counted once fully faded in (the fade's own frames are not aim).
                    if aiming && st.aim_w > 0.9 {
                        let after = err(eval(&aim_x));
                        stats.aim_frames += 1;
                        stats.err_before += before as f64;
                        stats.err_after += after as f64;
                        stats.worst_before = stats.worst_before.max(before);
                        stats.worst_after = stats.worst_after.max(after);
                        stats.under_2 += u64::from(after < 2.0);
                        stats.aimers.insert(rig.reference);
                        if !st.logged_aim && st.aim_w > 0.9 {
                            st.logged_aim = true;
                            info!(
                                "npc aim: {:08X} bends {} spine bones: barrel {before:.1}° off its target before, {after:.1}° after (yaw {:.1}°, pitch {:.1}°)",
                                rig.reference,
                                n,
                                st.aim.0.to_degrees(),
                                st.aim.1.to_degrees()
                            );
                        }
                    }
                }
            }
        } else {
            // Easing out (no target any more): the bend falls with the weight.
            let k = 1.0 - (-AIM_EASE * dt).exp();
            st.aim.0 -= st.aim.0 * k;
            st.aim.1 -= st.aim.1 * k;
        }
        let x: [Quat; 3] = if aim_x.iter().all(|q| *q == Quat::IDENTITY) { std::array::from_fn(|k| if k < n { twist_x(k) } else { Quat::IDENTITY }) } else { aim_x };
        let new_locals = locals(&x);

        // Write.
        if let Ok(mut t) = transforms.get_mut(rig.nodes[fr.top]) {
            *t = top_new;
        }
        st.written_top = Some((top_anim, top_new));
        for (k, &i) in fr.spine.iter().enumerate() {
            if let Ok(mut t) = transforms.get_mut(rig.nodes[i]) {
                t.rotation = new_locals[k];
            }
            st.written_spine[k] = Some((spine_anim[k], new_locals[k]));
        }
    }
    stats.total += t0.elapsed();
    stats.frames += 1;
    stats.since_report += dt;
    if stats.since_report >= REPORT_SECONDS {
        stats.since_report = 0.0;
        report(&stats);
    }
}

/// The foot slip of a moving actor against its ground speed: the old blend's and the
/// stride-matched one's (for the log).
fn slip_stats(lo: &Locomotion, stats: &mut NpcFeelStats) {
    let actual = lo.velocity[0].hypot(lo.velocity[1]);
    if actual < STAND_SPEED {
        return;
    }
    stats.moving_frames += 1;
    let b = &lo.blend;
    let Some(ci) = lo.set.info(b.cur.mv).filter(|_| b.cur.mv.is_moving()) else {
        // Moving on a standing clip (an attack or a fade from the idle): not a stride.
        stats.moving_frames -= 1;
        stats.clipless_frames += 1;
        return;
    };
    if (NATURAL_RATE.0..=NATURAL_RATE.1).contains(&b.cur.rate) {
        stats.natural_frames += 1;
    }
    let after = b.cur.rate * ci.speed;
    let prev_set = lo.prev_set.as_deref().unwrap_or(&lo.set);
    let before = match b.prev.and_then(|p| Some((p, prev_set.info(p.mv)?))) {
        Some((p, pi)) if p.mv.is_moving() && ci.duration > 0.0 && pi.duration > 0.0 => {
            let w = b.weight();
            let (pc, cc) = (p.rate / pi.duration, b.cur.rate / ci.duration);
            let c = pc + (cc - pc) * w;
            stats.fade_frames += 1;
            let drawn = c * (pi.speed * pi.duration + (ci.speed * ci.duration - pi.speed * pi.duration) * w);
            let after_fade = fnv_actor::locomotion::stride_matched_cycles(Some(pi), Some(ci), b.cur.rate, w).map_or(drawn, |m| m * (pi.speed * pi.duration + (ci.speed * ci.duration - pi.speed * pi.duration) * w));
            stats.fade_slip_before += ((drawn - actual).abs() / actual) as f64;
            stats.fade_slip_after += ((after_fade - actual).abs() / actual) as f64;
            stats.slip_after += ((after_fade - actual).abs() / actual) as f64;
            stats.slip_before += ((drawn - actual).abs() / actual) as f64;
            return;
        }
        _ => after,
    };
    stats.slip_before += ((before - actual).abs() / actual) as f64;
    stats.slip_after += ((after - actual).abs() / actual) as f64;
}

fn report(s: &NpcFeelStats) {
    let pct = |a: f64, n: u64| if n > 0 { 100.0 * a / n as f64 } else { 0.0 };
    info!(
        "grounded footsteps: {} standing frames planted, {} stepping ({} steps by {} actors, lag up to {:.0}°, {} frames dragged); {} frames feet turned to their travel; moving {} frames on a walk or run clip ({} on a standing one), {:.0}% at a natural rate, foot slip {:.1}% before, {:.1}% after (in {} walk/run fades: {:.1}% before, {:.1}% after)",
        s.planted_frames,
        s.step_frames,
        s.steps,
        s.turners.len(),
        s.largest_lag.to_degrees(),
        s.dragged_frames,
        s.travel_frames,
        s.moving_frames,
        s.clipless_frames,
        pct(s.natural_frames as f64, s.moving_frames),
        pct(s.slip_before, s.moving_frames),
        pct(s.slip_after, s.moving_frames),
        s.fade_frames,
        pct(s.fade_slip_before, s.fade_frames),
        pct(s.fade_slip_after, s.fade_frames),
    );
    if s.aim_frames > 0 {
        info!(
            "npc aim: {} frames by {} gunmen, barrel off its target {:.1}° on average before (worst {:.1}°), {:.1}° after (worst {:.1}°), {:.0}% of frames within 2°, {} at the bend's limit",
            s.aim_frames,
            s.aimers.len(),
            s.err_before / s.aim_frames as f64,
            s.worst_before,
            s.err_after / s.aim_frames as f64,
            s.worst_after,
            pct(s.under_2 as f64, s.aim_frames),
            s.limited
        );
    }
    if s.frames > 0 {
        info!("npc feel: {:.1} µs per frame", s.total.as_secs_f64() * 1e6 / s.frames as f64);
    }
}

fn report_at_exit(mut exit: MessageReader<AppExit>, stats: Res<NpcFeelStats>) {
    if exit.read().next().is_some() {
        report(&stats);
    }
}
