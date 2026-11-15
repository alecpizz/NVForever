//! Actors going through a door that leads elsewhere fade out at it and fade in where it
//! leads, as the player's view does (owner's playtest list 5 item 3: "NPCs going
//! through a door should fade out/in and play the door sound, like the player").
//!
//! - **Out**: the drawn body of an actor that used such a door (`residents.rs`, the AI's
//!   `AiEvent::Door`) stands at the door, facing it (`fnv_nav::Walker::face_door`), for
//!   [`FADE_SECONDS`] while it fades out; then it is gone. It does not walk on: carried
//!   along its old heading it slid past the frame of a door it came at from the side
//!   (#1472). It is no longer the actor's body meanwhile (it has
//!   neither `RefVisual` nor `ActorRig`), so nothing else takes it for the actor.
//! - **In**: the actor's body drawn at the other side (when that place is drawn) fades
//!   in over the same time.
//! - **Sound**: the door's open sound (DOOR `SNAM`, as the player's load doors,
//!   `nvf_play::world_sounds`) at the door it went in by and at the one it came out of,
//!   wherever the listener is drawn (`residents.rs` sends `nvf_play::world_sounds::DoorPassed`).
//!
//! The fade is the streamed cells' dither (`fade.rs`): each mesh with a NIF material
//! gets the crossfading visibility range of its step. Bevy picks the dithering pipeline
//! only when a mesh or its material changes, so a mesh given its first range has its
//! material marked changed. Meshes keep the fully drawn range afterwards (fade.rs: one
//! whose range went away would vanish). Meshes of other materials do not fade.
//! [design: how the game fades actors at doors, and for how long, is not documented.]

use crate::fade;
use crate::limbs::ActorRig;
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use nvf_assets::NifMaterial;
use std::collections::HashSet;

/// How long a fade out or in lasts, seconds [design].
pub const FADE_SECONDS: f32 = 0.75;

/// A body fading at a door.
#[derive(Component, Debug, Clone, Copy)]
pub struct DoorFade {
    /// When it started (`Time::elapsed_secs`).
    pub since: f32,
    /// Fading out (despawned at the end) rather than in.
    pub out: bool,
    /// Seamless doors: the door is opened onto its other side (`door_portal.rs`), and the
    /// body walks on into the doorway at this velocity (game units a second, bodies being
    /// placed in game space) once it shows, cut off by the doorway's view, instead of
    /// fading.
    pub walk: Option<Vec3>,
}

/// Seamless doors: a body going through an opened door waits this long for the doorway to
/// show (`door_portal::SHOW_AFTER_SECS` and a little), then walks this long into it
/// [engineering choice].
pub const WALK_WAIT_SECONDS: f32 = 0.45;
pub const WALK_SECONDS: f32 = 1.2;

/// The references whose next drawn body fades in (they came through a door), and the
/// counts for the summary.
#[derive(Resource, Default)]
pub struct DoorFades {
    pub fade_in: HashSet<u32>,
    pub outs: usize,
    pub ins: usize,
}

/// Give every NIF mesh under `root` the dither step `k` (0 hidden … `fade::STEPS` drawn).
fn apply(commands: &mut Commands, root: Entity, k: u8, children: &Query<&Children>, meshes: &mut Query<(&mut MeshMaterial3d<NifMaterial>, Has<VisibilityRange>)>) {
    for e in children.iter_descendants(root) {
        let Ok((mut material, ranged)) = meshes.get_mut(e) else { continue };
        commands.entity(e).insert(fade::drawn(k));
        if !ranged {
            // Its pipeline is chosen again, with the dither (module notes).
            material.set_changed();
        }
    }
}

/// Fade the bodies at doors (after `residents::drive`, whose spawns are in by then).
pub fn fade(
    mut commands: Commands,
    time: Res<Time>,
    mut fades: ResMut<DoorFades>,
    new_roots: Query<(Entity, &ActorRig), Added<ActorRig>>,
    mut bodies: Query<(Entity, &DoorFade, &mut Transform)>,
    children: Query<&Children>,
    mut meshes: Query<(&mut MeshMaterial3d<NifMaterial>, Has<VisibilityRange>)>,
) {
    let now = time.elapsed_secs();
    // A body drawn for one that came through a door starts hidden, this frame.
    for (e, rig) in &new_roots {
        if fades.fade_in.remove(&rig.reference) {
            commands.entity(e).insert(DoorFade { since: now, out: false, walk: None });
            apply(&mut commands, e, 0, &children, &mut meshes);
            fades.ins += 1;
        }
    }
    let dt = time.delta_secs();
    for (e, f, mut t) in &mut bodies {
        if let Some(v) = f.walk {
            // Into the opened doorway, then gone (not dithered: the doorway's view hides it).
            let since = now - f.since;
            if since >= WALK_WAIT_SECONDS + WALK_SECONDS {
                commands.entity(e).despawn();
            } else if since >= WALK_WAIT_SECONDS {
                t.translation += v * dt;
            }
            continue;
        }
        let share = ((now - f.since) / FADE_SECONDS).clamp(0.0, 1.0);
        if f.out {
            if share >= 1.0 {
                commands.entity(e).despawn();
                continue;
            }
            apply(&mut commands, e, fade::step(1.0 - share), &children, &mut meshes);
        } else {
            apply(&mut commands, e, fade::step(share), &children, &mut meshes);
            if share >= 1.0 {
                commands.entity(e).remove::<DoorFade>();
            }
        }
    }
}

/// The counts when the app exits.
pub fn report(mut exit: MessageReader<AppExit>, fades: Res<DoorFades>) {
    if exit.read().next().is_some() {
        info!("door fade summary: {} actors faded out at a door, {} faded in at the other side", fades.outs, fades.ins);
    }
}
