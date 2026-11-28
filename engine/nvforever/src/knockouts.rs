//! Knocked-out actors on screen (docs/spec-notes/unconscious.md §4): an actor the game
//! knocks out (`fnv_game::unconscious`) goes limp as a corpse does, lies there, and when
//! the game says it is up plays its get-up clip and walks on.
//!
//! - **Falling**: as a fresh death in `deaths.rs` — [`Dying`] (which every system that
//!   moves, aims or turns an actor already leaves alone) and, when its skeleton has one,
//!   its ragdoll ([`Corpse`]) dropping it where it stands; without a ragdoll the death
//!   pose, or the body laid on its back. [`KnockedOut`] marks it as not dead.
//! - **Lying**: the side it lies on, from the pose — face up when its front (left upper
//!   arm → right upper arm, crossed with pelvis → head) points up — is told to the game
//!   (IsFacingUp).
//! - **Getting up**: the ragdoll leaves the physics world, the actor stands where it was
//!   again and plays the get-up clip of its side ([`crate::locomotion::ClipSet::getup`])
//!   once, as a [`Dying`] clip from its first frame. The frame after, once the clip's
//!   first pose is on the bones, the actor is turned and moved so that the clip's pelvis
//!   and pelvis → head line lie where the ragdoll's did. When the clip is over the actor
//!   is itself again.
//! - **Knockdowns** (PushActorAway, explosions; unconscious.md §5) fall the same way,
//!   the ragdoll thrown with the knockdown's velocity (`Game::take_shove`); a body
//!   already lying that is pushed again is shoved as a hit shoves a corpse.
//! - Killed while down: it stays a corpse. Killed while getting up (a second blast): it
//!   falls again from where it is, as a fresh death does (#2218).

use crate::limbs::ActorRig;
use crate::locomotion::{Dying, Flinch, Locomotion};
use crate::ragdoll::{Corpse, Ragdolls};
use bevy::prelude::*;
use nvf_assets::data::GameData;
use nvf_play::phys::PhysState;
use nvf_play::script::ScriptState;
use nvf_play::RefVisual;
use std::sync::Arc;

pub struct KnockoutsPlugin;

impl Plugin for KnockoutsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KnockoutShows>().add_systems(Update, show_knockouts.after(nvf_play::PlaySet::Activation)).add_systems(Last, report);
    }
}

/// Counts for the exit report.
#[derive(Resource, Default)]
struct KnockoutShows {
    fell: usize,
    ragdolls: usize,
    got_up: usize,
    clips: usize,
}

/// A knocked-out actor on screen (on the actor's root, next to its [`ActorRig`]).
#[derive(Component)]
pub struct KnockedOut {
    /// Its transform when it fell (laid on its back without a ragdoll or a death pose).
    stood: Transform,
    /// The side it lies on, once seen: face up.
    face_up: Option<bool>,
    phase: Phase,
}

enum Phase {
    Down,
    /// Its get-up clip starts this frame: the ragdoll's pelvis and head (world space).
    Align { pelvis: Vec3, head: Vec3, length: f32 },
    /// The clip plays until then (`Time::elapsed_secs`); the actor stands where it was
    /// put to start it (`at`) and must not be moved by anything until the end.
    Rising { until: f32, at: Vec3 },
}

/// What [`show_knockouts`] reads of an actor.
type Shown = (
    Entity,
    &'static RefVisual,
    &'static ActorRig,
    Option<&'static Locomotion>,
    &'static mut Transform,
    &'static GlobalTransform,
    Option<&'static mut KnockedOut>,
    Has<Dying>,
    Has<Corpse>,
    Option<&'static crate::outfits::Dressed>,
);

/// A bone's world position, by name.
pub(crate) fn bone(rig: &ActorRig, globals: &Query<&GlobalTransform>, name: &str) -> Option<Vec3> {
    let &i = rig.skel.by_name.get(name)?;
    globals.get(*rig.nodes.get(i)?).ok().map(|g| g.translation())
}

/// The pelvis and the head (world space).
pub(crate) fn pelvis_head(rig: &ActorRig, globals: &Query<&GlobalTransform>) -> Option<(Vec3, Vec3)> {
    Some((bone(rig, globals, "Bip01 Pelvis")?, bone(rig, globals, "Bip01 Head")?))
}

/// Whether the body lies face up: its front, (left upper arm − right upper arm) ×
/// (head − pelvis) in game space (z up), points up. `None` for a skeleton without those
/// bones.
pub(crate) fn face_up(rig: &ActorRig, globals: &Query<&GlobalTransform>) -> Option<bool> {
    let g = |n: &str| bone(rig, globals, n).map(|p| Vec3::from(nvf_play::space::to_game(p)));
    let (l, r) = (g("Bip01 L UpperArm")?, g("Bip01 R UpperArm")?);
    let (p, h) = (g("Bip01 Pelvis")?, g("Bip01 Head")?);
    let front = (l - r).cross(h - p);
    Some(front.z > 0.0)
}

#[allow(clippy::too_many_arguments)]
fn show_knockouts(
    time: Res<Time>,
    mut commands: Commands,
    mut script: Option<ResMut<ScriptState>>,
    data: Option<Res<GameData>>,
    mut phys: Option<ResMut<PhysState>>,
    mut ragdolls: ResMut<Ragdolls>,
    mut shows: ResMut<KnockoutShows>,
    mut actors: Query<Shown>,
    globals: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
    mut residents: Option<ResMut<crate::residents::Residents>>,
    mut pushes: MessageWriter<nvf_play::combat::ActorPush>,
    actor_world: Option<Res<crate::actors::ActorWorld>>,
) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let now = time.elapsed_secs();
    for (e, r, rig, lo, mut t, gt, ko, dying, corpse, dressed) in &mut actors {
        let id = fnv_sim::FormId(r.0);
        // Lying knocked out; a knockout already getting up waits for its clip here.
        let down = rt.game.knockout(id).is_some_and(|k| k.lying());
        let Some(mut ko) = ko else {
            // Knocked out now (a corpse already shown is not).
            if !down || dying || rt.game.is_dead(id) {
                continue;
            }
            let templates = crate::ragdoll::worn_templates(actor_world.as_deref(), dressed.map_or(&[][..], |d| &d.worn));
            let mut body = data.as_deref().and_then(|d| ragdolls.corpse(d, rig, &templates));
            if let Some(v) = rt.game.take_shove(id) {
                if let Some(b) = body.as_mut() {
                    b.velocity = v;
                }
                info!("knockout: {:08X} thrown at ({:.0}, {:.0}, {:.0}) units/s", r.0, v[0], v[1], v[2]);
            }
            let falls = body.is_some() && phys.as_deref().is_some_and(|p| p.ready_at(nvf_play::space::to_game(gt.translation())));
            let clip = lo.filter(|lo| !falls && !lo.set.death.is_empty()).map(|lo| lo.set.death[(r.0 as usize) % lo.set.death.len()].clone());
            let stood = *t;
            if clip.is_none() && !falls {
                t.rotate_local_x(-std::f32::consts::FRAC_PI_2);
            }
            info!("knockout: {:08X} falls ({})", r.0, if falls { "ragdoll" } else if clip.is_some() { "death pose" } else { "laid down" });
            shows.fell += 1;
            shows.ragdolls += usize::from(falls);
            let mut ec = commands.entity(e);
            ec.insert((Dying { clip, t: 0.0 }, KnockedOut { stood, face_up: None, phase: Phase::Down }));
            ec.remove::<Flinch>();
            if let Some(mut c) = body {
                // Not falling, it lies without its ragdoll already (`ragdoll::unbuilt`).
                c.laid_out = !falls;
                ec.insert(c);
            }
            continue;
        };
        if rt.game.is_dead(id) {
            // Killed while down: a corpse from here on.
            let mut ec = commands.entity(e);
            ec.remove::<KnockedOut>();
            if !matches!(ko.phase, Phase::Down) {
                // Killed while getting up: its ragdoll has left the world and the get-up
                // clip holds the bones, which would play on to standing (#2218). It
                // falls again from where it is, as a fresh death in `deaths.rs` does.
                let templates = crate::ragdoll::worn_templates(actor_world.as_deref(), dressed.map_or(&[][..], |d| &d.worn));
                let body = data.as_deref().and_then(|d| ragdolls.corpse(d, rig, &templates));
                let falls = body.is_some() && phys.as_deref().is_some_and(|p| p.ready_at(nvf_play::space::to_game(gt.translation())));
                let clip = lo.filter(|lo| !falls && !lo.set.death.is_empty()).map(|lo| lo.set.death[(r.0 as usize) % lo.set.death.len()].clone());
                if clip.is_none() && !falls {
                    t.rotate_local_x(-std::f32::consts::FRAC_PI_2);
                }
                info!("knockout: {:08X} killed while getting up: falls ({})", r.0, if falls { "ragdoll" } else if clip.is_some() { "death pose" } else { "laid down" });
                ec.insert(Dying { clip, t: 0.0 });
                if let Some(mut c) = body {
                    c.laid_out = !falls;
                    ec.insert(c);
                }
            }
            continue;
        }
        match ko.phase {
            Phase::Down if down => {
                // Pushed again while it lies: shove the body.
                if let Some(dv) = rt.game.take_shove(id) {
                    if let Some((pelvis, _)) = pelvis_head(rig, &globals) {
                        pushes.write(nvf_play::combat::ActorPush { form_id: r.0, point: nvf_play::space::to_game(pelvis), dv });
                    }
                }
                // The side it lies on, for IsFacingUp.
                if let Some(up) = face_up(rig, &globals) {
                    if ko.face_up != Some(up) {
                        ko.face_up = Some(up);
                        rt.game.set_knocked_side(id, up);
                    }
                }
            }
            Phase::Down => {
                let lying = pelvis_head(rig, &globals);
                if corpse {
                    if let Some(p) = phys.as_deref_mut() {
                        p.world.remove_ragdoll(r.0);
                    }
                    ragdolls.rest.remove(&r.0);
                    commands.entity(e).remove::<Corpse>();
                }
                *t = ko.stood;
                let side = usize::from(ko.face_up == Some(false));
                let clip: Option<Arc<_>> = lo.and_then(|lo| lo.set.getup[side].clone().or_else(|| lo.set.getup[1 - side].clone()));
                shows.got_up += 1;
                match (clip, lying) {
                    (Some(clip), Some((pelvis, head))) => {
                        let length = clip.clip.length();
                        info!("knockout: {:08X} gets up face {} ({:.1} s clip)", r.0, if side == 0 { "up" } else { "down" }, length);
                        shows.clips += 1;
                        commands.entity(e).insert(Dying { clip: Some(clip), t: 0.0 });
                        ko.phase = Phase::Align { pelvis, head, length };
                    }
                    _ => {
                        info!("knockout: {:08X} stands up (no get-up clip)", r.0);
                        rt.game.finish_getting_up(id);
                        commands.entity(e).remove::<(Dying, KnockedOut)>();
                    }
                }
            }
            Phase::Align { pelvis, head, length } => {
                // The clip's first pose is on the bones: lay it over where the body lay.
                if let Some((p, h)) = pelvis_head(rig, &globals) {
                    let flat = |v: Vec3| Vec2::new(v.x, v.z);
                    let (want, got) = (flat(head - pelvis), flat(h - p));
                    if want.length() > 1.0 && got.length() > 1.0 {
                        // Bevy y is up; a turn about it by `angle` takes `got` onto `want`.
                        // Worked out in world space about the actor's own origin, then
                        // brought into its parent's space (the world root: game space, z
                        // up), where its transform lives.
                        let angle = got.angle_to(want);
                        let turn = Quat::from_rotation_y(-angle);
                        let origin = gt.translation();
                        let moved = origin + turn * (p - origin);
                        let shift = Vec3::new(pelvis.x - moved.x, 0.0, pelvis.z - moved.z);
                        let to_local = parents.get(e).ok().and_then(|c| globals.get(c.parent()).ok()).map_or(bevy::math::Affine3A::IDENTITY, |g| g.affine().inverse());
                        let axis = to_local.transform_vector3(Vec3::Y).normalize_or(Vec3::Z);
                        t.rotation = (Quat::from_axis_angle(axis, -angle) * t.rotation).normalize();
                        t.translation += to_local.transform_vector3(shift);
                        info!("knockout: {:08X} laid over its ragdoll: turned {:.0}°, moved {:.0} units", r.0, angle.to_degrees(), shift.length());
                        // A resident is drawn where its walker is, or its physics character
                        // when it has one: the walker comes along, and the character is made
                        // again where it stands when it next walks.
                        if let Some(res) = residents.as_deref_mut() {
                            res.put_walker(id, t.translation.to_array(), crate::nav::tr_heading(&t));
                        }
                        if let Some(p) = phys.as_deref_mut() {
                            p.remove_npc(r.0);
                        }
                    }
                }
                ko.phase = Phase::Rising { until: now + length, at: t.translation };
            }
            Phase::Rising { until, at } => {
                if now >= until {
                    // How far anything moved it along the ground during the clip (0: it
                    // stayed put); its transform is in game space (z up).
                    let moved = (t.translation - at).truncate().length();
                    info!("knockout: {:08X} is up: it may move again (moved {:.1} units while getting up)", r.0, moved);
                    rt.game.finish_getting_up(id);
                    commands.entity(e).remove::<(Dying, KnockedOut)>();
                }
            }
        }
    }
}

fn report(mut exit: MessageReader<AppExit>, shows: Res<KnockoutShows>) {
    if exit.read().last().is_none() || shows.fell == 0 {
        return;
    }
    info!("knockouts shown: {} fell ({} as ragdolls), {} got up ({} with a get-up clip)", shows.fell, shows.ragdolls, shows.got_up, shows.clips);
}
