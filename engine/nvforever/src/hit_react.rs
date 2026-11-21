//! Hit reactions (Modern Feel, `nvf_play::modern_feel::ModernFeel::hit_reactions`;
//! docs/design/modern-feel-hit.md): a living actor that a shot or a blow strikes has the
//! struck bone, and with less of it the bones above it, pushed on a damped spring laid over
//! whatever its animation does; the spring settles in about 0.4 s and hits stack. A head
//! hit snaps the head back, a leg hit buckles the knee, a torso hit bends and twists the
//! spine (the turn comes from where the hit lands about each bone). A killing hit hands the
//! posed skeleton, reaction and all, to the ragdoll (`ragdoll.rs` starts from the bones'
//! global transforms after this writes them) and adds a shove at the struck body part
//! scaled by the weapon's damage, so a shotgun throws a body and a pistol drops it.
//!
//! All of it is NVForever's own [design; every constant an engineering choice tuned by
//! eye], none of it in the original, which plays a whole reaction clip or nothing (that
//! clip still plays: `deaths.rs`).
//!
//! The struck bone is found from the hit itself, not from bone names: the hit's body part
//! (`ActorHit::location`, the GetHitLocation numbering) picks the kind of limb, the actor's
//! body part chains (`limbs::LimbRig`, from its body part data, creatures' too) give the
//! candidate bones, and the bone whose segment passes nearest the hit point
//! (`ActorPush::point`) is struck. Bones that carry both legs (the pelvis and above it) are
//! never turned, so the feet stay where the animation and foot IK put them.
//!
//! Layering: [`apply`] runs in `PostUpdate` before transform propagation, after every
//! `Update` system (the animation, foot IK, head tracking, aim) posed the skeleton; it
//! multiplies each sprung bone's local rotation by the spring's turn and remembers what it
//! wrote. [`restore`] puts the animated rotation back in `PreUpdate`, before anything of
//! the next frame looks, where the bone still holds what was written (bones the
//! animation does not drive would otherwise collect the turn frame after frame, and foot
//! IK would take it for the animation).

use crate::limbs::{ActorRig, LimbRig};
use crate::ragdoll::Corpse;
use bevy::prelude::*;
use fnv_combat::data::Limb;
use nvf_play::combat::{is_dead, ActorHit, ActorPush, Combat};
use nvf_play::modern_feel::ModernFeel;
use nvf_play::script::ScriptState;
use nvf_play::space;
use std::time::{Duration, Instant};

/// The bones above the struck one that turn with it, and their shares of its push.
const FALLOFF: [f32; 3] = [1.0, 0.55, 0.3];

/// The spring's damping ratio: a little under critical, so a hit overshoots once and
/// settles in about 0.4 s (4 / (ζ ω₀) for the torso's ω₀) [design].
const DAMPING_RATIO: f32 = 0.55;

/// The most a bone is turned, radians (stacked hits saturate here) [design].
const MAX_ANGLE: f32 = 0.6;

/// The most one hit sets a bone turning, radians a second [design].
const MAX_SPIN: f32 = 16.0;

/// The shortest lever a hit has about a bone, game units: a hit right on the joint turns it
/// as if this far from it [design].
const MIN_LEVER: f32 = 9.0;

/// The knee's buckle on a leg hit: the thigh's turn, radians a second, per unit a second
/// of the hit's push (the shin turns twice that the other way) [design].
const BUCKLE: f32 = 0.035;

/// A killing hit's extra shove on the struck body part, game units a second per point of
/// the weapon's damage a shot, within [`KILL_PUSH_MIN`] and [`KILL_PUSH_MAX`]; a share of it
/// [`KILL_LIFT`] upwards [design: a weapon of base damage 20 adds 180; the Caravan
/// shotgun's killing pellet (50, observed in the smoke run) 450; the cap 900 at 100].
const KILL_PUSH_PER_DAMAGE: f32 = 9.0;
const KILL_PUSH_MIN: f32 = 60.0;
const KILL_PUSH_MAX: f32 = 900.0;
const KILL_LIFT: f32 = 0.3;

/// Integration step at most, seconds (the spring is stiff; steps are cut to this) [design].
const STEP: f32 = 1.0 / 240.0;

/// How each kind of limb reacts: a gain on the push's turn, the spring's natural frequency
/// (radians a second), and how many bones above the struck one turn too.
fn style(limb: Limb) -> (f32, f32, usize) {
    match limb {
        Limb::Head => (1.5, 26.0, 2),
        Limb::Torso => (1.3, 17.0, 2),
        Limb::Arm => (1.2, 20.0, 2),
        Limb::Leg => (0.8, 16.0, 1),
        Limb::Other => (1.0, 18.0, 2),
    }
}

/// The limb a GetHitLocation number names (`fnv_combat::data::BodyPart::limb`).
fn limb_of(location: i32) -> Limb {
    match location {
        0 => Limb::Torso,
        1 | 2 | 13 => Limb::Head,
        3..=6 => Limb::Arm,
        7..=12 => Limb::Leg,
        _ => Limb::Other,
    }
}

#[derive(Clone, Copy, Debug)]
struct Spring {
    node: usize,
    /// The turn (a rotation vector in the parent bone's frame) and its rate.
    theta: Vec3,
    omega: Vec3,
    /// Natural frequency, radians a second.
    w0: f32,
}

/// An actor's sprung bones and what [`apply`] wrote over them.
#[derive(Component, Default)]
pub struct HitReact {
    springs: Vec<Spring>,
    /// (node, the rotation found, the rotation written).
    written: Vec<(usize, Quat, Quat)>,
}

impl HitReact {
    fn kick(&mut self, node: usize, omega: Vec3, w0: f32) {
        match self.springs.iter_mut().find(|s| s.node == node) {
            Some(s) => {
                s.omega += omega;
                s.w0 = s.w0.max(w0);
            }
            None => self.springs.push(Spring { node, theta: Vec3::ZERO, omega, w0 }),
        }
    }
}

/// Counts for the exit report.
#[derive(Resource, Default)]
struct HitReactStats {
    reactions: u64,
    kills: u64,
    actors: std::collections::HashSet<u32>,
    unplaced: u64,
    time: Duration,
    frames: u64,
    busy_frames: u64,
}

/// A killing hit's extra shove, written after the frame's pushes were read.
#[derive(Resource, Default)]
struct KillPushes(Vec<ActorPush>);

pub struct HitReactPlugin;

impl Plugin for HitReactPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HitReactStats>()
            .init_resource::<KillPushes>()
            .add_systems(PreUpdate, restore)
            .add_systems(Update, (collect, send_kill_pushes).chain().after(nvf_play::PlaySet::Activation).after(nvf_play::knockouts::TestHitSet))
            .add_systems(PostUpdate, apply.before(bevy::transform::TransformSystems::Propagate))
            .add_systems(Last, report);
    }
}

fn on(feel: Option<&ModernFeel>) -> bool {
    feel.is_some_and(ModernFeel::hit_reactions)
}

/// Put back the rotations the last frame's reaction turned, where they still hold what was
/// written; a corpse whose ragdoll moves its bones keeps them.
fn restore(mut actors: Query<(&ActorRig, &mut HitReact, Option<&Corpse>)>, mut transforms: Query<&mut Transform>) {
    for (rig, mut react, corpse) in &mut actors {
        if react.written.is_empty() {
            continue;
        }
        let written = std::mem::take(&mut react.written);
        if corpse.is_some_and(|c| c.live) {
            react.springs.clear();
            continue;
        }
        for (node, base, w) in written {
            if let Some(Ok(mut t)) = rig.nodes.get(node).map(|&n| transforms.get_mut(n)) {
                if t.rotation.abs_diff_eq(w, 1e-5) {
                    t.rotation = base;
                }
            }
        }
    }
}

/// The bone a hit at `point` (Bevy space) struck: of the body part chains of the hit's
/// limb (all chains when none is of it), the node whose segment to the next node of its
/// chain (or its skull tip, or itself at the end) passes nearest the point; never one that
/// carries the legs.
fn struck_bone(rig: &ActorRig, limbs: &LimbRig, limb: Limb, point: Vec3, carried: &[usize], globals: &Query<&GlobalTransform>) -> Option<(usize, Limb)> {
    let index = |e: Entity| rig.nodes.iter().position(|&n| n == e);
    let of_limb = limbs.chains.iter().any(|c| c.1 == limb);
    let mut best: Option<(f32, usize, Limb)> = None;
    for (_, kind, chain, tip) in limbs.chains.iter().filter(|c| !of_limb || c.1 == limb) {
        for (i, &e) in chain.iter().enumerate() {
            let Ok(g) = globals.get(e) else { continue };
            let a = g.translation();
            let b = match chain.get(i + 1) {
                Some(&next) => globals.get(next).map_or(a, GlobalTransform::translation),
                None => tip.map_or(a, |t| g.transform_point(t)),
            };
            let ab = b - a;
            let s = if ab.length_squared() > 1e-6 { ((point - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
            let d = point.distance_squared(a + ab * s);
            if best.is_none_or(|(bd, _, _)| d < bd) {
                if let Some(n) = index(e).filter(|n| !carried.contains(n)) {
                    best = Some((d, n, *kind));
                }
            }
        }
    }
    best.map(|(_, n, k)| (n, k))
}

/// The nodes that carry both legs (or every leg chain): the pelvis, the skeleton's root and
/// what is between; with no leg chains, the root and its child.
fn carried(rig: &ActorRig, limbs: &LimbRig) -> Vec<usize> {
    let parents = &rig.skel.parents;
    let mut out = vec![0];
    let mut legs = limbs.chains.iter().filter(|c| c.1 == Limb::Leg).filter_map(|c| c.2.first()).filter_map(|&e| rig.nodes.iter().position(|&n| n == e)).peekable();
    if legs.peek().is_none() {
        out.extend(parents.iter().position(|p| *p == Some(0)));
        return out;
    }
    for leg in legs {
        let mut n = parents.get(leg).copied().flatten();
        while let Some(p) = n {
            if !out.contains(&p) {
                out.push(p);
            }
            n = parents.get(p).copied().flatten();
        }
    }
    out
}

/// The rotation part of a global transform.
fn rot(g: &GlobalTransform) -> Quat {
    g.to_scale_rotation_translation().1
}

#[allow(clippy::too_many_arguments)]
fn collect(
    feel: Option<Res<ModernFeel>>,
    combat: Option<Res<Combat>>,
    script: Option<Res<ScriptState>>,
    mut hits: MessageReader<ActorHit>,
    mut pushes: MessageReader<ActorPush>,
    mut kills: ResMut<KillPushes>,
    mut stats: ResMut<HitReactStats>,
    mut commands: Commands,
    actors: Query<(Entity, &ActorRig, Option<&LimbRig>)>,
    mut reacts: Query<&mut HitReact>,
    globals: Query<&GlobalTransform>,
) {
    let mut hits: Vec<Option<ActorHit>> = hits.read().map(|h| Some(*h)).collect();
    let pushes: Vec<ActorPush> = pushes.read().copied().collect();
    if !on(feel.as_deref()) || pushes.is_empty() {
        return;
    }
    let t0 = Instant::now();
    // Actors struck for the first time: their reaction is put on them at the end.
    let mut fresh: std::collections::HashMap<Entity, HitReact> = std::collections::HashMap::new();
    for p in pushes {
        // The hit that made this push (the combat systems write the two together).
        let Some(hit) = hits.iter_mut().find(|h| h.is_some_and(|h| h.form_id == p.form_id)).and_then(Option::take) else { continue };
        if !hit.died && is_dead(script.as_deref(), p.form_id) {
            continue;
        }
        let Some((e, rig, limbs)) = actors.iter().find(|(_, r, _)| r.reference == p.form_id) else { continue };
        let Some(limbs) = limbs else {
            stats.unplaced += 1;
            continue;
        };
        let dv = Vec3::from(p.dv);
        let speed = dv.length();
        if speed < 1e-3 {
            continue;
        }
        // Bevy space: the push's way and where it landed.
        let dir = space::to_bevy(p.dv).normalize();
        let point = space::to_bevy(p.point);
        let carried = carried(rig, limbs);
        let Some((struck, limb)) = struck_bone(rig, limbs, limb_of(hit.location), point, &carried, &globals) else {
            stats.unplaced += 1;
            continue;
        };
        let (gain, w0, above) = style(limb);
        // The hit's strength: its push, more for a hit that took more of the actor's health.
        let strength = speed * (0.6 + 2.5 * hit.share).clamp(0.6, 1.6);
        let parents = &rig.skel.parents;
        // The struck bone and those above it, short of what carries the legs.
        let mut chain = Vec::with_capacity(FALLOFF.len());
        let mut n = Some(struck);
        while let Some(i) = n {
            if chain.len() > above {
                break;
            }
            if carried.contains(&i) {
                break;
            }
            chain.push(i);
            n = parents.get(i).copied().flatten();
        }
        let mut kicks: Vec<(usize, Vec3)> = Vec::new();
        for (k, &i) in chain.iter().enumerate() {
            let Ok(g) = globals.get(rig.nodes[i]) else { continue };
            let r = point - g.translation();
            let spin = gain * FALLOFF[k] * r.cross(dir * strength) / r.length_squared().max(MIN_LEVER * MIN_LEVER);
            kicks.push((i, spin.clamp_length_max(MAX_SPIN)));
        }
        // A leg hit buckles its knee: the thigh forward, the shin back.
        if limb == Limb::Leg {
            if let Some((thigh, calf, n)) = knee(rig, limbs, struck, &globals) {
                let b = BUCKLE * strength;
                kicks.push((thigh, -n * b));
                kicks.push((calf, n * (2.0 * b)));
            }
        }
        if kicks.is_empty() {
            continue;
        }
        let peak = kicks.iter().map(|k| k.1.length()).fold(0.0, f32::max);
        // Into each bone's parent frame (the spring's).
        let mut react = reacts.get_mut(e).ok();
        let react: &mut HitReact = match react.as_deref_mut() {
            Some(r) => r,
            None => fresh.entry(e).or_default(),
        };
        for (i, spin) in kicks {
            let parent = parents.get(i).copied().flatten().and_then(|pi| globals.get(rig.nodes[pi]).ok()).map_or(Quat::IDENTITY, rot);
            react.kick(i, parent.inverse() * spin, w0);
        }
        stats.reactions += 1;
        stats.actors.insert(p.form_id);
        let name = rig.skel.names.get(struck).cloned().flatten().unwrap_or_else(|| format!("#{struck}"));
        info!("hit reaction: {:08X} bone {name} strength {strength:.0}, {} bones, fastest {peak:.1} rad/s{}", p.form_id, chain.len(), if hit.died { " (killing hit)" } else { "" });
        if hit.died {
            let weapon = hit.weapon.and_then(|w| combat.as_deref().and_then(|c| c.forms.weapons.get(&w)));
            let damage = weapon.map_or(0.0, |w| w.base_damage as f32);
            let extra = (KILL_PUSH_PER_DAMAGE * damage).clamp(KILL_PUSH_MIN, KILL_PUSH_MAX);
            let way = dv / speed;
            let kick = [way[0] * extra, way[1] * extra, way[2] * extra + KILL_LIFT * extra];
            kills.0.push(ActorPush { form_id: p.form_id, point: p.point, dv: kick });
            stats.kills += 1;
            info!(
                "hit reaction: {:08X} killed by {}, ragdoll shoved {extra:.0} units/s more at bone {name}",
                p.form_id,
                weapon.and_then(|w| w.edid.clone()).unwrap_or_else(|| hit.weapon.map_or("no weapon".into(), |w| w.to_string()))
            );
        }
    }
    for (e, r) in fresh {
        commands.entity(e).insert(r);
    }
    stats.time += t0.elapsed();
}

/// The struck leg's thigh and shin and the knee's hinge (Bevy space, the way that bends it
/// further): the top of the leg chains walking up from the struck bone, its child on a leg
/// chain, and that one's.
fn knee(rig: &ActorRig, limbs: &LimbRig, struck: usize, globals: &Query<&GlobalTransform>) -> Option<(usize, usize, Vec3)> {
    let leg: Vec<usize> = limbs.chains.iter().filter(|c| c.1 == Limb::Leg).flat_map(|c| c.2.iter()).filter_map(|&e| rig.nodes.iter().position(|&n| n == e)).collect();
    let parents = &rig.skel.parents;
    let mut thigh = struck;
    while let Some(p) = parents.get(thigh).copied().flatten().filter(|p| leg.contains(p)) {
        thigh = p;
    }
    let child = |of: usize| leg.iter().copied().find(|&n| parents.get(n).copied().flatten() == Some(of));
    let calf = child(thigh)?;
    let foot = child(calf)?;
    let at = |i: usize| globals.get(rig.nodes[i]).ok().map(GlobalTransform::translation);
    let (h, k, a) = (at(thigh)?, at(calf)?, at(foot)?);
    let n = (k - h).cross(a - k);
    let n = if n.length() > 1e-3 * (k - h).length() * (a - k).length() {
        n.normalize()
    } else {
        // A straight leg: the actor's right (the skeleton's +X), bending the knee forward.
        -(rot(globals.get(rig.nodes[0]).ok()?) * Vec3::X).normalize()
    };
    Some((thigh, calf, n))
}

fn send_kill_pushes(mut kills: ResMut<KillPushes>, mut out: MessageWriter<ActorPush>) {
    for p in kills.0.drain(..) {
        out.write(p);
    }
}

/// Step the springs and lay their turns over the frame's pose.
fn apply(
    time: Res<Time>,
    feel: Option<Res<ModernFeel>>,
    mut stats: ResMut<HitReactStats>,
    mut actors: Query<(&ActorRig, &mut HitReact, Option<&Corpse>)>,
    mut transforms: Query<&mut Transform>,
) {
    let t0 = Instant::now();
    stats.frames += 1;
    let enabled = on(feel.as_deref());
    let dt = time.delta_secs();
    let mut busy = false;
    for (rig, mut react, corpse) in &mut actors {
        if react.springs.is_empty() {
            continue;
        }
        if !enabled || corpse.is_some_and(|c| c.live) {
            react.springs.clear();
            continue;
        }
        busy = true;
        let steps = ((dt / STEP).ceil() as usize).clamp(if dt > 0.0 { 1 } else { 0 }, 16);
        let h = if steps > 0 { dt / steps as f32 } else { 0.0 };
        let react = &mut *react;
        for s in &mut react.springs {
            let (k, c) = (s.w0 * s.w0, 2.0 * DAMPING_RATIO * s.w0);
            for _ in 0..steps {
                s.omega += (-k * s.theta - c * s.omega) * h;
                s.theta = (s.theta + s.omega * h).clamp_length_max(MAX_ANGLE);
            }
        }
        react.springs.retain(|s| s.theta.length() > 0.002 || s.omega.length() > 0.02);
        for s in &react.springs {
            let Some(Ok(mut t)) = rig.nodes.get(s.node).map(|&n| transforms.get_mut(n)) else { continue };
            let base = t.rotation;
            let w = Quat::from_scaled_axis(s.theta) * base;
            t.rotation = w;
            react.written.push((s.node, base, w));
        }
    }
    if busy {
        stats.busy_frames += 1;
        stats.time += t0.elapsed();
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<HitReactStats>) {
    if exit.read().next().is_none() {
        return;
    }
    let per = if stats.busy_frames > 0 { stats.time.as_secs_f64() * 1e3 / stats.busy_frames as f64 } else { 0.0 };
    info!(
        "hit reactions: {} reactions on {} actors, {} killing shoves, {} hits not placed on a bone; {:.4} ms a frame while any reacts ({} of {} frames)",
        stats.reactions,
        stats.actors.len(),
        stats.kills,
        stats.unplaced,
        per,
        stats.busy_frames,
        stats.frames
    );
}
