//! The player's own body in `--play` (docs/design/player-body.md), as two rigs:
//!
//! - **Third person**: the player's NPC_ (FormID 7) assembled like a placed NPC
//!   (`actors.rs`) on `characters\_male\skeleton.nif`, wearing what the simulation has
//!   equipped, standing at the character's feet and facing the view's heading. Seen in
//!   third person and while the camera circles the player.
//! - **First person**: the game's separate arms rig, `characters\_1stperson\skeleton.nif`,
//!   with the arms and hands only (`fnv_actor::first_person`: hands' `…1st.nif`
//!   variants, the `Arms` shapes of the upper body and of upper-body armour) and the
//!   weapon's first-person model (WEAP WNAM). Its `Bip01 Looking` node turns with the
//!   view's pitch, so arms and weapon follow the view, and the camera sits at its
//!   `Camera1st` node (118 units up, 78 crouched) [hypothesis: the node names; observed:
//!   the nodes, and the first-person sneak clips lower `Bip01 NonAccum` by 40;
//!   animation.md §9.5].
//!
//! Both play layered KF clips (`nvf_assets::anim_blend`) from their own folder, chosen by
//! `fnv_actor::player_clips`: an idle, movement in four directions walking or running,
//! sneaking, swimming, jumping and landing, turning on the spot, and the weapon drawn,
//! held (aimed down the sights, or blocking) and holstered, with the weapon moving
//! between the hand and the holster bone at the clips' `Attach`/`Detach` keys. Attacks
//! and reloads (combat step C4, `nvf_play::combat`): the body tells combat when the weapon
//! is out and how long the attack and reload clips run (`WeaponAnim`), and plays the clip
//! once each time combat counts an attack or a reload up (a round-by-round reload: the
//! opening clip, then the reload clip again for each round).
//!
//! The weapon model keeps its node tree under the `Weapon` node, and the clips move its
//! parts by name as they move bones: the slide, hammer and trigger of a pistol in its
//! attack clip, the magazine in its reload clip, a minigun's barrel in its spin clip
//! (animation.md §9.8).
//!
//! Runs in `nvf_play::player::BodySet`: after the character moved, before the camera is
//! placed. The eye (`Player::body_eye`) is the first-person rig's camera node. Without
//! actors (`--no-actors`) there is no body and the eye stays at the fixed eye height;
//! without a first-person skeleton the third-person body's head gives the eye and its
//! head is hidden in first person.
//!
//! **The body in first person** ([`PlayerBody::first_person_body`], `--first-person-body`
//! and the settings page; off as the original, which draws the arms rig alone): the
//! third-person body is drawn in first person too, without its head, standing so that
//! its eyes are under the camera, and upright whatever the pitch. While the arms rig's
//! arms are up the body's own arms and hands are hidden, and its weapon while it is in
//! the hand. On furniture the seated body is drawn without its head, the view turns no
//! further from the way the body faces than a neck does ([`seated_heading`]) and the eyes
//! go round the neck with it. While an idle drives the first-person view (the opening's
//! wake-up) the body is not drawn: the arms rig plays the idle alone.
//!
//! **The shadow** [engineering choice; the original casts no sun shadows from the world]:
//! the third-person body casts the player's shadow in every view, head, arms and weapon
//! included, and the arms rig and its weapon cast none on the world. What of the body the
//! camera is not to see is drawn on [`SHADOW_LAYER`], which the sun casts from and no
//! camera sees, instead of being hidden (a hidden entity casts no shadow). The arms rig
//! is on [`ARMS_LAYER`], which the sun does not cast from. What the camera sees of the
//! body is on [`BODY_LAYER`], which the sun casts from and the flashlight does not. The unseen body stands where
//! the arms rig is and would shade it, so the arms rig takes its sun shadows from a light
//! of its own ([`ArmsShade`], `NifSurface::shade_light`) that casts from the world's
//! layer and the arms', but only what lies by the eye toward the sun ([`near_shade_casters`]), and
//! from the sun's own map what lies beyond the unseen body (`nif_material.wesl`
//! `shadow_past_body`): buildings, hills, what is seen of the body and the arms
//! themselves shade the arms, as before the unseen body cast anything.
//!
//! **Lying on a bed** ([`lying_view`]; owner requests 2026-10-10): in first person the
//! view is the lying body's, whether or not the body is drawn. It keeps its place in the
//! body's own frame while the body lies down and gets up, so the picture leans with the
//! head and ends on its side; the mouse turns the head in that frame, as far as a neck
//! does. In third person the camera's pivot is lifted clear of the bed.

use crate::actors::{part_uses, ActorWorld};
use crate::idles::{IdleSlot, PlayerIdles};
use crate::sky::{Sun, ARMS_LAYER, BODY_LAYER, SHADOW_LAYER};
use crate::world::WorldRoot;
use crate::view_sway::{SwayInput, SwayPose, ViewSway};
use crate::weapon_lower::{LowerInput, WeaponLower};
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::math::Affine3A;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use fnv_actor::first_person::{self, is_first_person_shape, shape_shows, Show};
use fnv_actor::player_clips::{
    aim_clips, aim_variant, aim_weights, attack_clips, attack_stem, block_clips, blocks, direction_weights, equip_clips, holster_clips, idle_clips, jump_clips,
    limping_idle_clips, limping_move_clips, limping_turn_clips, move_clips, reload_clips, reload_open_clips, toddler_idle_clips, toddler_move_clips,
    toddler_turn_clips, turn_clips, Aim, ClipKeys, Dir, HandEvent, Stance, Transition, WeaponHands, WeaponPhase, UNARMED,
};
use fnv_actor::weapon::AnimType;
use fnv_actor::Appearance;
use fnv_sim::FormId;
use nvf_assets::actor::{rigid_local, spawn_actor, ActorCtx, ActorParts, PartUse, PreparedSkeleton};
use nvf_assets::anim_blend::{Animator, ExtraNode, Mode, DEFAULT_FADE};
use nvf_assets::material_anim::StoredLook;
use nvf_assets::anim_play::Clip;
use nvf_assets::model::ModelCtx;
use nvf_assets::nif_material::NifMaterial;
use nvf_play::combat::WeaponAnim;
use nvf_play::control::{BodyView, PlayerControl};
use nvf_play::phys::PhysState;
use nvf_play::player::{MoveIntent, Player, MAX_PITCH};
use nvf_play::sim::SimState;
use nvf_play::PlayWorld;
use std::collections::HashMap;
use std::sync::Arc;

/// Layer roles ([`Animator`] tags).
const TAG_IDLE: u32 = 0;
const TAG_MOVE: u32 = 10; // + 0 along, + 1 sideways
const TAG_TURN: u32 = 20;
const TAG_JUMP: u32 = 30;
const TAG_LAND: u32 = 31;
const TAG_AIM: u32 = 40; // + 0 level, + 1 up, + 2 down
const TAG_GRIP: u32 = 45;
const TAG_HOLSTER: u32 = 50;
const TAG_EQUIP: u32 = 60;
const TAG_ATTACK: u32 = 70; // + 0 level, + 1 up, + 2 down
const TAG_RELOAD: u32 = 75;
const TAG_FURNITURE: u32 = 80;
/// The Pip-Boy coming up (`pipboy_arm.rs`).
const TAG_PIPBOY: u32 = 90;
/// The Pip-Boy clip fades in over its first 8 frames [observed: its `Blend: 8` key;
/// hypothesis: frames at 30 a second, as for idles] and out over its last 8 going down
/// [design].
const PIPBOY_BLEND: f32 = 8.0 / 30.0;

/// The field of view the first-person arms are drawn with while the Pip-Boy is up,
/// degrees, as the game's field-of-view setting counts them (across a 4:3 picture)
/// [documented: `Fallout_default.ini` `fPipboy1stPersonFOV=47.0`, and the owner's
/// `Fallout.ini` the same].
pub const PIPBOY_FOV: f32 = 47.0;

/// The field of view the first-person arms and weapon are drawn with otherwise, degrees
/// as [`PIPBOY_FOV`] [documented: `Fallout_default.ini` `fDefault1stPersonFOV=55`,
/// docs/design/layer3.md; hypothesis: that it is the view of the arms' own camera, so
/// they are drawn bigger than through the world's 75: our 9mm pistol was drawn smaller
/// than the game's, with less of its detail and a coarser mip (owner's `Weapon Quality`
/// screenshots, playtest list 5 item 18)].
pub const FIRST_PERSON_FOV: f32 = 55.0;

/// How much of the Pip-Boy clip shows at `t` seconds into it: up over its first
/// [`PIPBOY_BLEND`], down over its last (smoothstep).
pub fn pipboy_ease(t: f32) -> f32 {
    let e = (t.min(nvf_play::pipboy::CLIP_SECONDS - t) / PIPBOY_BLEND).clamp(0.0, 1.0);
    e * e * (3.0 - 2.0 * e)
}

/// Where the Pip-Boy clip is shown at `t` seconds into it with movement inertia: eased into
/// its hold at the `Hit` key and out of it, so the arm settles there and sets off from rest
/// (a cubic with the clip's own speed at the far ends, none at the hold); without, the clip
/// stopped dead at the hold with the arm still moving about 28 units/s, and set off at that
/// speed [design: the owner's tenth playtest, 2026-11-14: "PipBoy jitters a lot also"; the
/// motion trace]. The time spent is the clip's own.
pub fn pipboy_clip_time(t: f32) -> f32 {
    use nvf_play::pipboy::{CLIP_SECONDS, HOLD_SECONDS};
    let out = |x: f32| x + x * x - x * x * x;
    if t <= HOLD_SECONDS {
        HOLD_SECONDS * out((t / HOLD_SECONDS).clamp(0.0, 1.0))
    } else {
        let down = CLIP_SECONDS - HOLD_SECONDS;
        HOLD_SECONDS + down * (1.0 - out((1.0 - (t - HOLD_SECONDS) / down).clamp(0.0, 1.0)))
    }
}

/// Where the Pip-Boy's own clock `t` (the original clip's seconds: up until
/// `HOLD_SECONDS`, held there, down until `CLIP_SECONDS`) falls in the clip the load order
/// actually supplies: up maps onto the clip's start..`Hit`, down onto `Hit`..its end.
/// Identity for the original clip. Nuclear Sunset's Optimization Patch ships a 0.067 s
/// `pipboy.kf` (`Hit` at 0.033); held at 0.333 s it sat past its end, in the pose with the
/// arm put away, so the Pip-Boy was drawn below the view [observed 2026-11-21 in FadedLight's
/// Nuclear Sunset fork, #4492].
pub fn pipboy_clip_at(clip: &Clip, t: f32) -> f32 {
    let hit = clip.text_keys.iter().find(|(_, k)| k.trim().eq_ignore_ascii_case("hit")).map(|(at, _)| *at);
    pipboy_time_in(clip.start, clip.stop, hit, t)
}

/// [`pipboy_clip_at`] for a clip from `start` to `stop` with its `Hit` key at `hit`.
fn pipboy_time_in(start: f32, stop: f32, hit: Option<f32>, t: f32) -> f32 {
    use nvf_play::pipboy::{CLIP_SECONDS, HOLD_SECONDS};
    let stop = stop.max(start);
    let hit = hit.filter(|h| (start..=stop).contains(h)).unwrap_or(start + (stop - start) * HOLD_SECONDS / CLIP_SECONDS);
    if t <= HOLD_SECONDS {
        start + (hit - start) * (t / HOLD_SECONDS).clamp(0.0, 1.0)
    } else {
        hit + (stop - hit) * ((t - HOLD_SECONDS) / (CLIP_SECONDS - HOLD_SECONDS)).clamp(0.0, 1.0)
    }
}

/// How much bigger the arms are drawn so that they look as through a view of `arms_fov`
/// (the game's degrees across 4:3) inside the view's own vertical field of view
/// `view_vfov` (radians): the ratio of the two views' half-widths.
pub fn arms_magnification(view_vfov: f32, arms_fov: f32) -> f32 {
    let arms_vfov_half = ((arms_fov.to_radians() / 2.0).tan() * 3.0 / 4.0).atan();
    (view_vfov / 2.0).tan() / arms_vfov_half.tan()
}
/// Idles (`idles.rs`): one role per idle started, so an idle keeps its layer while the
/// next one fades in over it.
const TAG_SPECIAL: u32 = 1000;

/// Biped slots of headgear hidden in first person when the third-person body has to stand
/// in for the first-person rig: Head, Hair, Headband, Hat, Eye Glasses, Nose Ring,
/// Earrings, Mask, Mouth Object [documented: xEdit biped flag names, bits 0, 1, 9–14,
/// 16; engineering choice: which ones block the view].
const HEAD_SLOTS: u32 = 1 | 1 << 1 | 0x7E << 8 | 1 << 16;

/// Time in the air before the jump loop plays, and the shortest fall that plays a
/// landing, seconds [engineering choice: steps and slopes leave the ground briefly].
const AIRBORNE_LOOP: f32 = 0.15;
const AIRBORNE_LAND: f32 = 0.35;
/// The jump loop (falling) starts only with the ground farther below the feet than this,
/// or out of the controller's reach, game units: running down a slope or sliding over
/// rocks leaves the ground for a few steps without a fall [engineering choice: the
/// controller's step height, the drop it walks down without leaving the ground; the
/// original's rule is not documented (physics.md §6.4.2)].
const FALL_GAP: f32 = 24.0;
/// A landing counts (its log line, its clip, the view's dip) only after the body fell
/// faster than this in the flight, units/s: on a car wreck's roof a player was logged
/// "landed at 0 units/s" after time off the ground with no fall at all (player report
/// R-BRQX496D, #3611), and the landing clip and dip that followed bobbed the view
/// [engineering choice: real landings in that report were 144 to 442 units/s].
const LAND_MIN_FALL: f32 = 30.0;
/// Turning on the spot faster than this (radians per second) plays a turn clip
/// [engineering choice].
const TURN_RATE: f32 = 1.0;
/// Without a first-person rig: share of the view's pitch the neck takes when nothing else
/// bends the body, so the eye clears the chest looking down [engineering choice].
const NECK_PITCH: f32 = 0.6;
/// Blend-in of an attack clip, seconds [engineering choice: a shot should show at once].
const ATTACK_FADE: f32 = 0.03;
/// The body in first person: how far behind the camera its eyes stand, in the skeleton's
/// units, so the open neck stays out of the picture looking down [engineering choice].
const OWN_BODY_BACK: f32 = 4.0;
/// The body in first person on furniture: how far the view turns to either side of the
/// way the body faces, radians [owner decision 2026-10-10: "limited to a real person's
/// head movement"; engineering choice: 80 degrees, about what a neck turns].
const NECK_TURN: f32 = 80.0 * std::f32::consts::PI / 180.0;
/// The same with the player's Turn Further While Seated tweak: 115 degrees [design: docs/design/tweaks.md].
const NECK_TURN_WIDE: f32 = 115.0 * std::f32::consts::PI / 180.0;
/// How fast a view turned further than that when the body sits down comes round to it,
/// radians per second [engineering choice].
const NECK_RETURN_RATE: f32 = 4.0;
/// And how far down it tilts, radians [engineering choice: 75 degrees, head and eyes;
/// steeper, the view is down the body's own collar].
const NECK_DOWN: f32 = 75.0 * std::f32::consts::PI / 180.0;
/// Lying on a bed: how far the third-person camera's pivot is lifted above the head, in
/// game units, so the camera's way round the body is clear of the mattress and the bed's
/// frame [engineering choice].
const LYING_PIVOT_UP: f32 = 40.0;
/// Lying on a bed: how far below level the first-person view looks at most, radians
/// [engineering choice: 10 degrees; the head is on the pillow, and lower is the inside of
/// the mattress].
const LYING_DOWN: f32 = 10.0 * std::f32::consts::PI / 180.0;
/// An attack clip with a `Loop` key goes round its repeated part while the shots come no
/// further apart than this, seconds [engineering choice: the clips that have the key are
/// the `attackspin` ones, of weapons that fire many times a second; animation.md §9.8].
const LOOP_HOLD: f32 = 0.25;

/// The share of the sun's light the arms rig's shade light carries. The materials need
/// some to work the sun's part of the light out (`nif_material.wesl`), and the world gets
/// that much more sun with it [engineering choice: a thousandth, a quarter of one step of
/// an 8-bit picture at full brightness].
const ARMS_SHADE_SHARE: f32 = 1.0 / 1024.0;

/// How far from the camera the [`ArmsShade`] light's cascade reaches (game units): the
/// arms rig and its weapon lie within about 100 units of the eye [engineering choice].
const ARMS_SHADE_REACH: f32 = 256.0;

/// The light the arms rig's sun shadows are read from (module docs, "The shadow"): a
/// child of the sun, so it points the same way, with the sun's colour and
/// [`ARMS_SHADE_SHARE`] of its strength. It casts from the world's layer and the arms',
/// and not from the sun's shadow-only layer. Its one shadow cascade reaches only
/// [`ARMS_SHADE_REACH`] from the camera, around the arms: a cascade takes every caster
/// between it and the sun, so what shades the arms still does, while the pass draws only
/// those (before 2026-10-12 it was the sun's first cascade, about 190 casters at
/// Goodsprings against about 30 then; the arms' shadows are finer for it). Of those it
/// draws only the ones by the eye toward the sun ([`near_shade_casters`]). It turns with the
/// sun, so its cascade is snapped to a grid anchored near the camera as the sun's are
/// (shadow_anchor.rs).
#[derive(Component)]
pub(crate) struct ArmsShade;

/// How far toward the sun from the eye the [`ArmsShade`] light's casters are looked for,
/// and how far from that line they may lie (game units): the arms rig and its weapon reach
/// about 60 units from the eye, and past the plane through the eye and 24 units more the
/// sun's own map takes over (`nif_material.wesl` `shadow_past_body`) [engineering
/// choice].
const ARMS_SHADE_LINE: f32 = 96.0;
const ARMS_SHADE_NEAR: f32 = 64.0;

/// Keep in the [`ArmsShade`] light's cascades only the casters whose bounds come within
/// [`ARMS_SHADE_NEAR`] of the line from the view's camera [`ARMS_SHADE_LINE`] toward the
/// sun (PostUpdate, after Bevy's light visibility). Bevy keeps every caster between a
/// cascade and the light, so at a low sun the column toward the sun took in hills,
/// buildings and distant land far off, each drawn into a map as large as the sun's: up to
/// 3.6 ms a frame at Novac at 07:00 for 36-38 casters [observed 2026-11-02, #1374]. The
/// land is left out too: a cell's land box spans its hills, so it holds the eye wherever
/// the player stands, and the land filling the whole map still cost about 0.9 ms there
/// [observed 2026-11-02, 5 casters left]; a slope rising toward the sun shades the arms
/// from past the body, through the sun's map. What
/// shades the arms from further off is read from the sun's own map beyond the unseen body
/// instead, so this light is left with what the body's clearance hides there: the arms on
/// themselves, a roof or a door frame right at the head. What it still costs is the pass
/// itself, whatever it draws: about 0.75 ms at Novac at 07:00 with 4096² maps for one
/// caster as for three, against 0.25 ms for none [observed 2026-11-02, #1374].
#[allow(clippy::type_complexity)]
pub(crate) fn near_shade_casters(
    mut shade: Query<(&mut bevy::camera::visibility::CascadesVisibleEntities, &GlobalTransform), With<ArmsShade>>,
    views: Query<&GlobalTransform, With<Camera>>,
    casters: Query<(&bevy::camera::primitives::Aabb, &GlobalTransform, Has<crate::terrain::LandMesh>)>,
) {
    for (mut visible, light) in &mut shade {
        // A directional light shines along its forward axis.
        let to_sun = light.back().as_vec3();
        for (view, cascades) in visible.entities.iter_mut() {
            let Ok(eye) = views.get(*view) else { continue };
            let eye = eye.translation();
            for cascade in cascades.iter_mut() {
                cascade.entities.retain(|e| match casters.get(*e) {
                    Ok((aabb, at, land)) => !land && near_line(aabb, at, eye, to_sun),
                    Err(_) => true,
                });
            }
        }
    }
}

/// Whether the bounds `aabb` at `at` come within [`ARMS_SHADE_NEAR`] of the line from
/// `eye` [`ARMS_SHADE_LINE`] along `to_sun`: the nearest point of the box, as placed, to
/// points every 16 units along the line.
fn near_line(aabb: &bevy::camera::primitives::Aabb, at: &GlobalTransform, eye: Vec3, to_sun: Vec3) -> bool {
    let place = at.affine();
    let back = place.inverse();
    let (min, max) = (Vec3::from(aabb.min()), Vec3::from(aabb.max()));
    let steps = (ARMS_SHADE_LINE / 16.0) as usize;
    (0..=steps).any(|i| {
        let p = eye + to_sun * (i as f32 * 16.0);
        let nearest = back.transform_point3(p).clamp(min, max);
        place.transform_point3(nearest).distance(p) <= ARMS_SHADE_NEAR
    })
}

/// A copy of material `m` whose sun shadows are the [`ArmsShade`] light's and whose depth
/// is squeezed in front of the world, so no wall, door or actor the camera stands against
/// hides the arms or the weapon (`NifSurface::first_person`; `m` itself is shared with
/// other actors and weapons).
fn arms_material(materials: &mut Assets<NifMaterial>, m: &Handle<NifMaterial>) -> Handle<NifMaterial> {
    let Some(mut copy) = materials.get(m).cloned() else { return m.clone() };
    copy.extension.shade_light = true;
    copy.extension.first_person = true;
    materials.add(copy)
}

/// Test aid `NVF_PLAY_TEST_NO_ARMS_SHADE=1`: no [`ArmsShade`] light, for measuring what it
/// costs (#1374; docs/design/frame-rate-audit.md §1).
fn no_arms_shade() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var("NVF_PLAY_TEST_NO_ARMS_SHADE").is_ok_and(|v| v == "1"))
}

/// Keep the [`ArmsShade`] light: there while the arms rig is drawn and the sun casts
/// shadows, so that nothing pays for it otherwise.
///
/// The light is despawned when not wanted, never hidden. A hidden directional light
/// keeps its shadow views in Bevy 0.19.1's render world, and when it is shown again
/// Bevy gives it a new set without despawning the old (`add_light_view_entities`
/// replaces the light's `DirectionalLightViewEntities`, which runs no removal hook).
/// The old views keep their place among the views, so the light's shadow phase was
/// batched once per set every frame, and the doubled batch sets upset the shadow
/// passes' shared indirect draw buffers: the sun's cascades lost or gained whole
/// batch sets from frame to frame, hillsides flickering between lit and shadowed in
/// first person, more often the more times the arms had been hidden and shown
/// [observed 2026-10-26: owner's captures; gone with GPU culling off or without this
/// light; a probe in the batching saw this light's 12 batch sets prepared as 24].
#[allow(clippy::type_complexity)]
fn shade_light(
    mut commands: Commands,
    state: Res<PlayerBody>,
    performance: Res<crate::shadow_rate::PerformanceShadows>,
    sun: Query<(Entity, &DirectionalLight), (With<Sun>, Without<ArmsShade>)>,
    mut shade: Query<(Entity, &mut DirectionalLight), With<ArmsShade>>,
) {
    // A sun casting no shadows has no shade light either.
    let Ok((sun, sun_light)) = sun.single() else { return };
    let arms_drawn = state.body.as_ref().and_then(|b| b.first.as_ref()).is_some_and(|f| f.shown == Some(true));
    // Nor with Sun shadows on Performance (shadow_rate.rs): one shadow view less.
    let on = arms_drawn && sun_light.shadow_maps_enabled && !no_arms_shade() && !performance.0;
    let illuminance = sun_light.illuminance * ARMS_SHADE_SHARE;
    if let Ok((e, mut light)) = shade.single_mut() {
        if !on {
            debug!("arms shade light off");
            commands.entity(e).despawn();
        } else if light.color != sun_light.color || light.illuminance != illuminance {
            light.color = sun_light.color;
            light.illuminance = illuminance;
        }
    } else if on {
        let cascades = CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 5.0,
            first_cascade_far_bound: ARMS_SHADE_REACH,
            maximum_distance: ARMS_SHADE_REACH,
            overlap_proportion: 0.0,
        }
        .build();
        debug!("arms shade light on");
        commands.spawn((
            Name::new("Arms shade light"),
            ArmsShade,
            DirectionalLight { color: sun_light.color, illuminance, shadow_maps_enabled: true, ..default() },
            cascades,
            RenderLayers::from_layers(&[0, ARMS_LAYER, BODY_LAYER]),
            Transform::IDENTITY,
            ChildOf(sun),
        ));
    }
}

/// A clip choice, for the clip caches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ClipKey {
    /// Posture; limping (a leg crippled, standing); toddler (`SetPCToddler`).
    Idle(Stance, bool, bool),
    /// Posture, female, running, direction, the weapon drawn; limping; toddler.
    Move(Stance, bool, bool, Dir, Option<&'static str>, bool, bool),
    Aim(Stance, &'static str, Aim, bool),
    Block(Stance, &'static str),
    Holster(Stance, &'static str),
    Equip(Stance, &'static str, bool),
    Jump(Stance, Option<&'static str>, Option<Dir>, bool),
    /// Posture, the weapon drawn, to the left; limping; toddler.
    Turn(Stance, Option<&'static str>, bool, bool, bool),
    /// Weapon prefix, attack clip stem (`attackright`), down the sights.
    Attack(Stance, &'static str, &'static str, bool),
    /// Weapon prefix, WEAP reload animation (`ReloadL`).
    Reload(Stance, &'static str, &'static str),
    /// The opening clip of a round-by-round reload (`ReloadX` → `1hpreloadxstart.kf`).
    ReloadOpen(Stance, &'static str, &'static str),
    /// The Pip-Boy coming up (first person); a woman's.
    PipBoy(bool),
    /// Weapon prefix and hand grip number: the grip clip (`1lmhandgrip1.kf`,
    /// `2hahandgrip1_plasmarifle.kf`).
    Grip(&'static str, u8),
}

impl ClipKey {
    /// A crippled leg's choice (standing).
    fn limping(self) -> bool {
        matches!(self, ClipKey::Idle(Stance::Stand, true, false) | ClipKey::Move(Stance::Stand, .., true, false) | ClipKey::Turn(Stance::Stand, .., true, false))
    }

    fn candidates(self) -> Vec<String> {
        match self {
            ClipKey::Idle(Stance::Stand, _, true) => toddler_idle_clips(Stance::Stand),
            ClipKey::Idle(Stance::Stand, true, false) => limping_idle_clips(),
            ClipKey::Idle(s, _, _) => idle_clips(s),
            ClipKey::Move(Stance::Stand, female, fast, d, w, _, true) => toddler_move_clips(Stance::Stand, female, fast, d, w),
            ClipKey::Move(Stance::Stand, female, fast, d, w, true, false) => limping_move_clips(female, fast, d, w),
            ClipKey::Move(s, female, fast, d, w, _, _) => move_clips(s, female, fast, d, w),
            ClipKey::Aim(s, p, a, sights) => aim_clips(s, p, a, sights),
            ClipKey::Block(s, p) => block_clips(s, p),
            ClipKey::Holster(s, p) => holster_clips(s, p),
            ClipKey::Equip(s, p, e) => equip_clips(s, p, e),
            ClipKey::Jump(s, w, d, land) => jump_clips(s, w, d, land),
            ClipKey::Turn(Stance::Stand, w, left, _, true) => toddler_turn_clips(Stance::Stand, w, left),
            ClipKey::Turn(Stance::Stand, w, left, true, false) => limping_turn_clips(w, left),
            ClipKey::Turn(s, w, left, _, _) => turn_clips(s, w, left),
            ClipKey::Attack(s, p, a, sights) => attack_clips(s, p, a, sights),
            ClipKey::Reload(s, p, r) => reload_clips(s, p, r),
            ClipKey::ReloadOpen(s, p, r) => reload_open_clips(s, p, r),
            // [observed: `_1stperson\locomotion\female\pipboyfemale.kf` beside the
            // male one]
            ClipKey::Grip(p, n) => vec![format!("{p}handgrip{n}.kf")],
            ClipKey::PipBoy(female) => {
                let own = if female { vec!["locomotion\\female\\pipboyfemale.kf".to_string()] } else { Vec::new() };
                own.into_iter().chain(["locomotion\\male\\pipboy.kf".to_string()]).collect()
            }
        }
    }
}

/// A loaded clip with what the body reads from it.
struct ClipInfo {
    path: String,
    clip: Arc<Clip>,
    keys: ClipKeys,
    /// Ground speed of its accumulation root (movement clips).
    speed: Option<f32>,
    /// Its track on the `Weapon` node.
    weapon_track: Option<usize>,
    /// How its motion goes along its time ([`ClipMotion`]), worked out when first asked.
    motion: std::sync::OnceLock<ClipMotion>,
}

/// How a clip's motion is spread over its time, for playing it by how far its motion has
/// got instead of by its clock: a draw whose arm comes up in its first fifth (the fists')
/// then follows the settling curve as a whole and not only its slow tail [design: the
/// owner's eleventh playtest, 2026-11-15: "some equip animations are not affected - check
/// that every animation for guns/melee weapons is affected, they can use the same curve";
/// the trace `movement-inertia-trace-weapons`: the fists came up at 980 units/s in 0.1 s].
#[derive(Debug, Clone, PartialEq)]
struct ClipMotion {
    /// The share of the motion done (0–1, rising) at each of [`MOTION_STEPS`] + 1 even
    /// steps of the clip's time; the steps that jump are left out of it.
    done: Vec<f32>,
    /// Where the clip's own pose jumps (shares of its time): a step its keys take at once
    /// (the straight razor's put-away swings the arm 34 units in one key at 97 %), eased
    /// across when played (`Animator::smooth_jump`).
    jumps: Vec<f32>,
}

/// Steps a clip's motion is measured in.
const MOTION_STEPS: usize = 120;
/// A turn of a bone counts as this many units of motion per radian (about an arm's reach
/// from the shoulder to the hand), a move as itself.
const MOTION_REACH: f32 = 25.0;
/// A turn of a hand counts as this many units of motion per radian: about how far the held
/// weapon's muzzle or blade swings with it [hypothesis].
const MOTION_HAND_REACH: f32 = 12.0;
/// A step (or two in a row) moving this many times as much as any of the steps two to
/// four away, and at least the share after it of the clip's motion, is a jump of its pose:
/// a key the clip takes at once, not a quick swing (which builds up over several steps).
const MOTION_JUMP_RATIO: f32 = 6.0;
const MOTION_JUMP_SHARE: f32 = 0.03;

impl ClipMotion {
    /// The motion of `clip` on skeleton `skel`: how far the hands go, as seen from the eye
    /// ([`Self::hand_steps`]), or, on a skeleton without hands, its bones' own motion
    /// ([`Self::bone_steps`]).
    fn of(clip: &Clip, skel: &PreparedSkeleton) -> ClipMotion {
        let step = Self::hand_steps(clip, skel).unwrap_or_else(|| Self::bone_steps(clip));
        Self::from_steps(step)
    }

    /// The motion of each step of `clip`'s time: both hands' moves and turns (a turn by
    /// [`MOTION_HAND_REACH`] units a radian) in the space of the first-person camera node
    /// (`Camera1st`; the skeleton's root without one), the clip's tracks posed through the
    /// skeleton's hierarchy with its bind pose where the clip has no track. `None` without
    /// a hand bone. What is seen moves, not what the bones do on their own: summed by bone,
    /// a turn of the upper arm (that swings the hand over a long arm) counted as little as
    /// one of the wrist, so the razor's draw raced through its swing (two pushes of 390 and
    /// 360 units/s where the original makes one) and the frag mine's ended with a flick (the
    /// traces `movement-inertia-trace-weapons` and `-rest` against their `-original`s,
    /// 2026-11-16).
    fn hand_steps(clip: &Clip, skel: &PreparedSkeleton) -> Option<Vec<f32>> {
        let find = |name: &str| {
            skel.by_name.get(name).copied().or_else(|| skel.names.iter().position(|n| n.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name))))
        };
        let hands: Vec<usize> = ["Bip01 R Hand", "Bip01 L Hand"].into_iter().filter_map(find).collect();
        if hands.is_empty() {
            return None;
        }
        let camera = find("Camera1st");
        let mut track = vec![None; skel.parents.len()];
        for (name, k) in &clip.tracks {
            if let Some(i) = find(name).filter(|&i| i < track.len()) {
                track[i] = Some(k);
            }
        }
        let (start, stop) = (clip.start, clip.stop.max(clip.start));
        // The clip's local transform of node `n` at `t`, the bind pose where it has none.
        let local = |n: usize, t: f32| -> Affine3A {
            let mut l = skel.locals.get(n).copied().unwrap_or_default();
            if let Some(k) = track[n] {
                if let Some(p) = k.translation.sample(t) {
                    l.translation = Vec3::from_array(p);
                }
                if let Some([w, x, y, z]) = k.rotation_at(t) {
                    l.rotation = Quat::from_xyzw(x, y, z, w);
                }
                if let Some([s]) = k.scale.sample(t) {
                    l.scale = Vec3::splat(s);
                }
            }
            affine(l)
        };
        // Node `n` in the root's space (the root's own transform left out, as the animator
        // poses it).
        let model = |n: usize, t: f32| -> Affine3A {
            let (mut m, mut cur, mut depth) = (Affine3A::IDENTITY, Some(n), 0);
            while let Some(i) = cur.filter(|&i| i > 0 && depth < 256) {
                m = local(i, t) * m;
                cur = skel.parents.get(i).copied().flatten();
                depth += 1;
            }
            m
        };
        let pose = |t: f32| -> Vec<(Vec3, Quat)> {
            let eye = camera.map_or(Affine3A::IDENTITY, |c| model(c, t)).inverse();
            hands
                .iter()
                .map(|&h| {
                    let (_, r, p) = (eye * model(h, t)).to_scale_rotation_translation();
                    (p, r)
                })
                .collect()
        };
        let mut last = pose(start);
        let mut step = Vec::with_capacity(MOTION_STEPS);
        for i in 1..=MOTION_STEPS {
            let now = pose(start + (stop - start) * i as f32 / MOTION_STEPS as f32);
            let moved: f32 = now.iter().zip(&last).map(|((p, q), (p0, q0))| p.distance(*p0) + MOTION_HAND_REACH * q.angle_between(*q0)).sum();
            step.push(if moved.is_finite() { moved } else { 0.0 });
            last = now;
        }
        Some(step)
    }

    /// The motion of each step of `clip`'s time as its bones' own moves and turns (a turn
    /// by [`MOTION_REACH`] units a radian), summed: the arms only, the fingers' motion being
    /// small and many, and the weapon's node (the flamer swinging off the back while the
    /// hands hold still) made the playback all but stop with the hands (the trace
    /// `movement-inertia-trace-weapons`, 2026-11-15).
    fn bone_steps(clip: &Clip) -> Vec<f32> {
        let (start, stop) = (clip.start, clip.stop.max(clip.start));
        let arms = |n: &str| {
            let n = n.to_ascii_lowercase();
            !n.contains("finger") && n != "weapon"
        };
        let tracks: Vec<_> = clip.tracks.iter().filter(|(n, _)| arms(n)).map(|(_, t)| t).collect();
        let pose = |t: f32| -> Vec<(Option<Vec3>, Option<Quat>)> {
            tracks.iter().map(|k| (k.translation.sample(t).map(Vec3::from_array), k.rotation_at(t).map(|[w, x, y, z]| Quat::from_xyzw(x, y, z, w)))).collect()
        };
        let mut last = pose(start);
        let mut step = Vec::with_capacity(MOTION_STEPS);
        for i in 1..=MOTION_STEPS {
            let now = pose(start + (stop - start) * i as f32 / MOTION_STEPS as f32);
            let moved: f32 = now
                .iter()
                .zip(&last)
                .map(|((p, q), (p0, q0))| {
                    let along = p.zip(*p0).map_or(0.0, |(a, b)| a.distance(b));
                    let turned = q.zip(*q0).map_or(0.0, |(a, b)| a.angle_between(b));
                    along + MOTION_REACH * turned
                })
                .sum();
            step.push(moved);
            last = now;
        }
        step
    }

    /// The motion done along the clip's time from each step's motion, its jumps left out.
    fn from_steps(mut step: Vec<f32>) -> ClipMotion {
        let total: f32 = step.iter().sum();
        let jumped: Vec<usize> = (0..step.len())
            .filter(|&i| {
                let around = (2..=4).flat_map(|d| [i.checked_sub(d), Some(i + d)]).flatten().filter_map(|k| step.get(k)).fold(0.0f32, |a, m| a.max(*m));
                total > 1e-3 && step[i] > MOTION_JUMP_SHARE * total && step[i] > MOTION_JUMP_RATIO * around
            })
            .collect();
        let jumps = jumped.iter().map(|&i| (i as f32 + 0.5) / MOTION_STEPS as f32).collect();
        for &i in &jumped {
            step[i] = 0.0;
        }
        let kept: f32 = step.iter().sum();
        let mut done = Vec::with_capacity(MOTION_STEPS + 1);
        done.push(0.0);
        let mut sum = 0.0;
        for (i, m) in step.iter().enumerate() {
            sum += m;
            // A clip that does not move (or only jumps) goes by its clock.
            done.push(if kept > 1e-3 { sum / kept } else { (i + 1) as f32 / MOTION_STEPS as f32 });
        }
        ClipMotion { done, jumps }
    }

    /// The share of its time (0–1) at which `share` of the motion is done.
    fn time_for(&self, share: f32) -> f32 {
        let share = share.clamp(0.0, 1.0);
        let i = self.done.partition_point(|d| *d < share).clamp(1, MOTION_STEPS);
        let (a, b) = (self.done[i - 1], self.done[i]);
        let within = if b > a { (share - a) / (b - a) } else { 0.0 };
        (i as f32 - 1.0 + within) / MOTION_STEPS as f32
    }

    /// The share of the motion done at `time` (a share of the clip's time).
    fn done_at(&self, time: f32) -> f32 {
        let x = time.clamp(0.0, 1.0) * MOTION_STEPS as f32;
        let i = (x.floor() as usize).min(MOTION_STEPS - 1);
        self.done[i] + (self.done[i + 1] - self.done[i]) * (x - i as f32)
    }

    /// Whether a jump of the pose lies between the times `from` and `to` (shares).
    fn jumps_between(&self, from: f32, to: f32) -> bool {
        let (a, b) = if from <= to { (from, to) } else { (to, from) };
        self.jumps.iter().any(|j| *j > a && *j <= b)
    }
}

impl ClipInfo {
    fn new(path: String, clip: Arc<Clip>, speed: Option<f32>) -> ClipInfo {
        let keys = ClipKeys::parse(&clip.text_keys);
        let weapon_track = clip.tracks.iter().position(|(n, _)| n.eq_ignore_ascii_case("Weapon"));
        ClipInfo { path, clip, keys, speed, weapon_track, motion: std::sync::OnceLock::new() }
    }

    /// Its [`ClipMotion`] on skeleton `skel` (the rig's that plays it).
    fn motion(&self, skel: &PreparedSkeleton) -> &ClipMotion {
        self.motion.get_or_init(|| {
            let m = ClipMotion::of(&self.clip, skel);
            debug!("player rig: {} played by its motion, jumps of its pose at {:?}", self.path, m.jumps);
            m
        })
    }

    fn fade(&self) -> f32 {
        self.keys.blend_seconds(DEFAULT_FADE)
    }

    /// Where the clip leaves the `Weapon` node at its end (`bind`: the node's bind pose,
    /// for the parts the track does not key).
    fn weapon_end(&self, bind: Transform) -> Option<Transform> {
        let (_, t) = &self.clip.tracks[self.weapon_track?];
        let time = self.clip.stop;
        Some(Transform {
            translation: t.translation.sample(time).map_or(bind.translation, Vec3::from_array),
            rotation: t.rotation_at(time).map_or(bind.rotation, |[w, x, y, z]| Quat::from_xyzw(x, y, z, w)),
            scale: Vec3::splat(t.scale.sample(time).map_or(bind.scale.x, |[s]| s)),
        })
    }
}

/// Which view a rig is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Third,
    First,
}

/// One shape of the weapon model.
struct WeaponPart {
    entity: Entity,
    /// The model nodes above it whose names begin `##`, each with whether the clips
    /// last showed it: clips show and hide such nodes (the minigun's gun while it is
    /// holstered) [observed: animation.md §9.7].
    tags: Vec<(String, bool)>,
    /// Whether it hangs from a bone of its own (the minigun's backpack) rather than
    /// from the `Weapon` node.
    on_bone: bool,
    /// Never shown: a mine's lit light while the mine is a held weapon.
    never: bool,
    /// The visibility last written to the entity.
    shown: Option<bool>,
}

/// kNVSE replacement files by a clip file's name, in lower case (`Rig::overrides`).
type Overrides = Arc<HashMap<String, Vec<String>>>;

/// One skeleton with its parts, clips and weapon.
struct Rig {
    view: View,
    root: Entity,
    /// First person: the nodes above the root that draw the rig bigger about the eye
    /// ([`place_lens`]).
    lens: Option<[Entity; 2]>,
    /// The skeleton's folder (`meshes\characters\_male\`), and its path.
    folder: String,
    skeleton: String,
    animator: Animator,
    clips: HashMap<ClipKey, Option<Arc<ClipInfo>>>,
    /// The up and down forms of clips, by path (failures too).
    variants: HashMap<String, Option<Arc<ClipInfo>>>,
    /// kNVSE's replacement files for this view ([`apply_anim_overrides`]): a clip's file
    /// name, in lower case, to the replacement's path under meshes.
    overrides: Overrides,
    /// The rig's current main body clip path (under meshes) and clip metadata, updated
    /// each frame in [`pose_rig`] for `anim_state.rs` (`GetPlayingAnimSequencePaths`,
    /// `GetAnimationAttributes`, `GetPlayingAnimBySequenceType`) [design].
    active_clip: Option<(String, Arc<Clip>)>,
    /// Third person: the head bone and the eye in its space.
    head: Option<usize>,
    eye_local: Vec3,
    /// First person: the node that turns with the pitch, and the camera node.
    looking: Option<usize>,
    camera: Option<usize>,
    /// First person: the turn that nodes a script put above the camera node add to it
    /// this frame, in the world ([`script_offset`]); the view takes it.
    script_turn: Option<Quat>,
    /// Where [`pose_rig`] placed the rig's root this frame, before the first-person lens
    /// ([`place_lens`]) takes it under itself: the rig's place for the scripts' world
    /// positions (`nif_mirror`, [`PlayerBody::posed_root`]).
    posed_root: Option<Transform>,
    /// First person: what the camera was placed from this frame: the script nodes' offset
    /// above the camera node ([`script_offset`]), the rig's root and the camera node in
    /// the animator's model space; [`follow_script_nodes`] moves the view by what the
    /// scripts changed after.
    script_used: Option<(Affine3A, Affine3A, Affine3A)>,
    /// First person: the node above the arms that the sway turns and moves
    /// (`view_sway.rs`).
    translate: Option<usize>,
    weapon_node: Option<usize>,
    /// The skeleton node the weapon node should hang from, and the one it was last
    /// attached to.
    weapon_parent: Option<usize>,
    /// Where the holster clip holds the `Weapon` node under the holster bone, from the
    /// last pose with the weapon holstered: the place it keeps on furniture
    /// ([`pose_furniture`]).
    holstered_weapon: Option<Transform>,
    /// The furniture clip [`pose_furniture`] last logged the weapon's place for.
    furniture_logged: Option<u32>,
    attached: Option<usize>,
    weapon_model: Option<Entity>,
    /// The weapon model's particle systems, when it has any: the holder carrying their
    /// clip-driven emitter ([`crate::particles::ModelEmitter::held`]) and the systems'
    /// names (the dynamite's fuse sparks, [`drive_weapon_emitters`]).
    weapon_emitter: Option<(Entity, Vec<String>)>,
    /// The weapon model's shapes.
    weapon_parts: Vec<WeaponPart>,
    /// The weapon is in the hand with no draw, attack or reload clip playing, and the
    /// parts' pose then was logged ([`pose_rig`]).
    idle_logged: bool,
    /// Down the sights: the most the spine was turned back to the sights clip's facing
    /// since the sights went up ([`keep_sights_facing`]; logged as they come down).
    sights_turned: Option<f32>,
    /// The way from the `Weapon` node down to the weapon model's `ProjectileNode`, when
    /// it has one: each node's place in the animator's extra nodes (a named node; the
    /// clips may move it) and its transform in the model [hypothesis: the node marks
    /// the muzzle; animation.md §9.9].
    muzzle: Option<Vec<(Option<usize>, Transform)>>,
    /// That node's entity (muzzle flashes are spawned under it; `shot_fx.rs`).
    muzzle_entity: Option<Entity>,
    /// The way down to the weapon model's `ShellCasingNode`, as `muzzle`, and its entity.
    casing: Option<Vec<(Option<usize>, Transform)>>,
    casing_entity: Option<Entity>,
    /// The way down to the weapon model's `##SightingNode`, as `muzzle`
    /// (`iron_sights.rs`).
    sighting: Option<Vec<(Option<usize>, Transform)>>,
    /// The way down to a second sighting node, `##SightingNode2`, which the player's
    /// Toggle Sighting Node key switches to (docs/design/tweaks.md).
    sighting2: Option<Vec<(Option<usize>, Transform)>>,
    /// The weapon model's scope eyepiece, when `scope_lens` knows the model (3D scopes),
    /// and the entity the lens is drawn on (`scope_lens::LensMount`, under the model).
    scope_eye: Option<crate::scope_lens::Eyepiece>,
    /// While a 3D scope is up: the eye goes behind the eyepiece instead of on the
    /// sighting node (`line_up_sights`).
    use_lens: bool,
    /// Down the iron sights: how far past the eye along the line of sight the sighting
    /// node goes (`iron_sights::eye_relief`), measured while the gun is only held (not
    /// during an attack's or a reload's clip, whose hand moves: a lever worked) and kept
    /// for the weapon model.
    sight_relief: Option<f32>,
    /// Third person: the entities drawn for the body's parts.
    drawn: Vec<Drawn>,
    /// The forms of the appearance's parts with at least one shape spawned (a part
    /// whose model did not load has none), in the parts' order.
    spawned_forms: Vec<FormId>,
    /// First person: whether the rig is shown (last applied).
    shown: Option<bool>,
    /// Third person: what of the rig the camera sees (last applied).
    seen: Option<Seen>,
    /// The furniture clip it plays (`BodyClip::serial`), while it plays one.
    furniture: Option<u32>,
    /// Seconds since the last attack started, and the repeated part of its clip, if it
    /// has one: from and to, seconds into the clip.
    since_attack: f32,
    attack_loop: Option<(f32, f32)>,
    /// The attack clip a wind-up started, until its shot leaves: the shot plays on in
    /// it rather than starting it again.
    wound_up: Option<Arc<Clip>>,
    /// The wound-up clip's `Hit` key, from its start (where a held throw waits).
    wound_hit: Option<f32>,
    /// The attack clip last started and its `Eject` key, seconds into the clip, when it
    /// has one; and whether its layer passed the key in the last pose.
    attack_eject: Option<(Arc<Clip>, f32)>,
    ejected: bool,
    /// How far its melee attack clip's root travelled in the last pose (x, y in the
    /// body's axes, unscaled), while one whose root travels plays: the player is carried
    /// that far (`nvf_play::player::Lunge`, docs/spec-notes/actors.md §8.13).
    carry: Option<Vec2>,
}

/// The node of a weapon model that shots are drawn from, by either of its names
/// (animation.md §9.9).
const MUZZLE_NODES: [&str; 2] = ["ProjectileNode", "##ProjectileNode"];

fn affine(t: Transform) -> Affine3A {
    Affine3A::from_scale_rotation_translation(t.scale, t.rotation, t.translation)
}

/// What nodes a script put above skeleton node `c` (JIP's InsertNode, moved by
/// SetNifBlockRotation and SetNifBlockTranslation, `crate::nif_mirror`) add to its place
/// in the rig, in model space; `None` without such nodes. The bones take the animator's
/// pose, the inserted nodes their own transforms, through the drawn entities up to the
/// skeleton's root. In the original the first-person camera hangs from the skeleton
/// [hypothesis: B42 True Leaning leans the view by nodes it inserts above `Camera1st`
/// and `Bip01 Looking`; observed: mods_reference ln_B42Leaning.txt].
fn script_offset(a: &Animator, c: usize, parents: &Query<&ChildOf>, transforms: &Query<&mut Transform>) -> Option<Affine3A> {
    let root = *a.nodes.first()?;
    let mut e = *a.nodes.get(c)?;
    let (mut drawn, mut plain, mut inserted) = (Affine3A::IDENTITY, Affine3A::IDENTITY, false);
    for _ in 0..256 {
        if e == root {
            return inserted.then(|| drawn * plain.inverse());
        }
        match a.nodes.iter().position(|&n| n == e) {
            Some(i) => {
                let l = affine(a.locals[i]);
                drawn = l * drawn;
                plain = l * plain;
            }
            None => {
                drawn = transforms.get(e).map_or(Affine3A::IDENTITY, |t| affine(*t)) * drawn;
                inserted = true;
            }
        }
        e = parents.get(e).ok()?.parent();
    }
    None
}

/// The script nodes above the first-person camera node as the mirror set them this frame
/// (`nif_mirror`, after the scripts ran): the view and the arms' lens move by what they
/// changed since the body was posed ([`drive_body`], before the scripts), so the view is
/// drawn with the same values as the arms. Without it the view kept the previous frame's
/// while a lean eased in or out, and shook against the arms [observed: the reporter's
/// playtest of B42 True Leaning, 2026-11-20].
fn follow_script_nodes(state: Res<PlayerBody>, parents: Query<&ChildOf>, mut transforms: Query<&mut Transform>, cams: Query<Entity, With<nvf_play::PlayerCamera>>) {
    let Some(f) = state.body.as_ref().and_then(|b| b.first.as_ref()).filter(|_| state.script_view) else { return };
    let (Some((used, root_m, plain)), Some(c)) = (f.script_used, f.camera) else { return };
    let Some(now) = script_offset(&f.animator, c, &parents, &transforms) else { return };
    if now.abs_diff_eq(used, 1e-4) {
        return;
    }
    let moved = Vec3::from((root_m * now * plain).translation - (root_m * used * plain).translation);
    let root = root_m.to_scale_rotation_translation().1;
    let turn = |o: Affine3A| root * o.to_scale_rotation_translation().1 * root.inverse();
    let turned = turn(now) * turn(used).inverse();
    if let Some(mut t) = cams.single().ok().and_then(|cam| transforms.get_mut(cam).ok()) {
        t.translation += nvf_play::space::to_bevy(moved.to_array());
        t.rotation = nvf_play::space::view_rotation(turned) * t.rotation;
    }
    // The lens stands at the eye ([`place_lens`]); the rig's root under it keeps its pose.
    let (Some([outer, _]), Some(posed)) = (f.lens, f.posed_root) else { return };
    let Ok(at) = transforms.get(outer).copied() else { return };
    let at = Transform { translation: at.translation + moved, ..at };
    if let Ok(mut t) = transforms.get_mut(outer) {
        *t = at;
    }
    if let Ok(mut t) = transforms.get_mut(f.root) {
        *t = Transform::from_matrix(at.to_matrix().inverse() * posed.to_matrix());
    }
}

/// An entity drawn for a part of the third-person body.
struct Drawn {
    entity: Entity,
    /// Of the head (head, hair, headgear): not seen when the body stands in for the
    /// first-person rig.
    head: bool,
    /// One the first-person rig has its own of (arms, hands, gloves, the Pip-Boy): not
    /// seen while that rig's arms are up in front of the body.
    arm: bool,
}

/// Where a layer's time is after going round the repeated part of its clip, `start` to
/// `end` seconds into it: unchanged before `end`.
fn loop_time(time: f32, start: f32, end: f32) -> f32 {
    if time < end || start >= end {
        return time;
    }
    start + (time - end).rem_euclid(end - start)
}

impl Rig {
    /// Whether the rig plays the down-the-sights clips (`…aimis…`, `…attack…is`) while
    /// Block is held. The third-person body always does; the arms rig only with a weapon
    /// model that has a sighting node (`iron_sights::SIGHTING_NODES`). Without one the
    /// original keeps the hip pose and only narrows the view [observed: the owner's
    /// screenshots, 2026-10-24, Laser Rifle in first person at the hip and aimed: same
    /// pose, the view zoomed; the owner: "base game has no aim animation it literally
    /// just zooms the screen in", "third person is still animated"].
    fn sights_clips(&self, aiming: bool) -> bool {
        aiming && (self.view != View::First || self.sighting.is_some())
    }

    /// Where the weapon's muzzle is in the last pose with the rig at `root` (game
    /// space): under the node the `Weapon` node hangs from, down the weapon model to its
    /// `ProjectileNode`, each node where the clips have it.
    fn muzzle_at(&self, root: Transform) -> Option<Vec3> {
        Some(self.weapon_node_at(root, self.muzzle.as_ref()?)?.translation.into())
    }

    /// Where the weapon's muzzle is and the barrel's forward unit vector (game space) in
    /// the last pose with the rig at `root`: `ProjectileNode` position and weapon model
    /// `+X` axis (the barrel axis used by `line_up_sights`).
    fn weapon_aim_at(&self, root: Transform) -> Option<([f32; 3], [f32; 3])> {
        let muzzle = self.muzzle_at(root)?;
        let model = self.weapon_node_at(root, &[])?;
        let dir = model.transform_vector3(Vec3::X).normalize_or_zero();
        (dir.length_squared() > 0.5).then_some((muzzle.to_array(), dir.to_array()))
    }

    /// Where a node of the weapon model is in the last pose with the rig at `root`
    /// (game space), given the way down to it from the `Weapon` node.
    fn weapon_node_at(&self, root: Transform, path: &[(Option<usize>, Transform)]) -> Option<Affine3A> {
        let a = &self.animator;
        let mut m = affine(root) * a.model_space(self.weapon_parent?) * affine(a.locals[self.weapon_node?]);
        for (extra, bind) in path {
            m *= affine(extra.and_then(|k| a.extra_local(k)).unwrap_or(*bind));
        }
        Some(m)
    }

    /// True iron sights (`iron_sights.rs`): turn and move the arms' node `t` (`Bip01
    /// Translate`) in the last pose so the weapon's `##SightingNode` lies a share `share`
    /// of the way onto the camera node, the barrel's axis (the weapon model's +X
    /// [observed: every `ProjectileNode` lies ahead along it]) along `view` (model
    /// space). Nothing when the weapon has no such node or does not hang below `t`.
    ///
    /// Nodes scripts put above the camera node and above `t` (`script_offset`: a lean's,
    /// ISControl's `##trm`) are drawn with: the eye, the view and the gun are taken where
    /// they are drawn, so the sights meet the eye the view is drawn from [hypothesis:
    /// B42 True Leaning and ISControl move them while aiming; observed: their
    /// `ln_B42Leaning.txt`, `ln_ISControl.txt`].
    fn line_up_sights(&mut self, t: usize, share: f32, view: Vec3, (parents, transforms): (&Query<&ChildOf>, &Query<&mut Transform>)) {
        let (Some(camera), Some(hand)) = (self.camera, self.weapon_parent) else { return };
        let a = &self.animator;
        let mut up = Some(hand);
        while let Some(n) = up.filter(|&n| n != t) {
            up = a.skel.parents[n];
        }
        if up.is_none() {
            return;
        }
        // A 3D scope: the point behind the eyepiece (`scope_lens`); else the sighting node.
        let second = nvf_play::tweaks::SECOND_SIGHT.load(std::sync::atomic::Ordering::Relaxed) && self.sighting2.is_some();
        let sighting = if second { self.sighting2.as_deref() } else { self.sighting.as_deref() };
        let (sight, barrel, iron) = match (self.use_lens.then_some(self.scope_eye).flatten(), sighting) {
            (Some(e), _) => {
                let Some(model) = self.weapon_node_at(Transform::IDENTITY, &[]) else { return };
                (model.transform_point3(crate::scope_lens::eye_point(&e)), model.transform_vector3(Vec3::X), false)
            }
            (None, Some(path)) => {
                let (Some(sight), Some(model)) = (self.weapon_node_at(Transform::IDENTITY, path), self.weapon_node_at(Transform::IDENTITY, &path[..path.len() - 1])) else { return };
                (sight.translation.into(), model.transform_vector3(Vec3::X), true)
            }
            (None, None) => return,
        };
        let a = &self.animator;
        let eye: Vec3 = a.model_space(camera).translation.into();
        // Iron sights: the gun pushed on along the line of sight so the gripping hand is
        // not at the eye (`iron_sights::eye_relief`); a 3D scope keeps its own eye point.
        let busy = a.layers.iter().any(|l| ((TAG_ATTACK..TAG_ATTACK + 3).contains(&l.tag) || l.tag == TAG_RELOAD) && l.ease > 0.0);
        if iron && (!busy || self.sight_relief.is_none()) {
            let grip = a.skel.by_name.get("Bip01 R Hand").map(|&h| Vec3::from(a.model_space(h).translation));
            let relief = grip.map_or(0.0, |g| crate::iron_sights::eye_relief(sight, barrel, g));
            if self.sight_relief.is_none_or(|r| (r - relief).abs() > 0.5) {
                debug!("true iron sights: eye relief {relief:.1} units");
            }
            self.sight_relief = Some(relief);
        }
        let relief = if iron { self.sight_relief.unwrap_or(0.0) } else { 0.0 };
        let a = &self.animator;
        let at_camera = script_offset(a, camera, parents, transforms).unwrap_or(Affine3A::IDENTITY);
        let above = script_offset(a, t, parents, transforms).unwrap_or(Affine3A::IDENTITY);
        let (eye, view) = (at_camera.transform_point3(eye), at_camera.transform_vector3(view));
        let (sight, barrel) = (above.transform_point3(sight), above.transform_vector3(barrel));
        let g = crate::iron_sights::correction(eye + view.normalize_or_zero() * relief, view, sight, barrel, share);
        let parent = above * a.skel.parents[t].map_or(Affine3A::IDENTITY, |p| a.model_space(p));
        let moved = parent.inverse() * g * parent * affine(self.animator.locals[t]);
        self.animator.locals[t] = Transform::from_matrix(Mat4::from(moved));
    }

    /// Where the weapon's `ShellCasingNode` is and the way a casing leaves it: the
    /// node's own +z [hypothesis: animation.md §9.9].
    fn casing_at(&self, root: Transform) -> Option<(Vec3, Vec3)> {
        let m = self.weapon_node_at(root, self.casing.as_ref()?)?;
        Some((m.translation.into(), m.transform_vector3(Vec3::Z).normalize_or_zero()))
    }

    /// The first existing file of a clip choice in this rig's folder (cached, failures
    /// too).
    fn clip(&mut self, ctx: &mut ActorCtx, key: ClipKey) -> Option<Arc<ClipInfo>> {
        if let Some(c) = self.clips.get(&key) {
            return c.clone();
        }
        // A kNVSE replacement of a candidate's file comes first [documented:
        // SetActorAnimationPath, "the anim group that is overridden is specified in the
        // KF file"; hypothesis: a replacement file is named as the clip it replaces, as
        // in the mods' folders (`mtfastforward.kf`)].
        // A replacement whose path ends as the candidate does (`locomotion\female\
        // mtforward.kf`) beats one that only shares the file's name, so a folder with
        // `male` and `female` files gives each body its own; among equals the later file
        // (the stack's top) wins.
        let replaced = key.candidates().into_iter().find_map(|c| {
            let c = c.to_ascii_lowercase();
            let name = c.rsplit('\\').next().unwrap_or(&c);
            let found = self.overrides.get(name)?;
            let tail = format!("\\{c}");
            let other_sex = if c.contains("female\\") { "\\male\\" } else { "\\female\\" };
            let pick = found.iter().rev().find(|p| p.to_ascii_lowercase().ends_with(&tail)).or_else(|| found.iter().rev().find(|p| !p.to_ascii_lowercase().contains(other_sex)));
            pick.map(|p| format!("meshes\\{p}"))
        });
        let mut path = replaced.filter(|p| ctx.models.data.vfs.exists(p)).or_else(|| key.candidates().into_iter().map(|c| format!("{}{c}", self.folder)).find(|p| ctx.models.data.vfs.exists(p)));
        // A grip clip may carry the weapon's name after its number
        // (`2hrhandgrip3_combatshotgun.kf`): the folder's one for that prefix and number.
        if let (None, ClipKey::Grip(p, n)) = (&path, key) {
            let stem = format!("{}{p}handgrip{n}_", self.folder);
            path = ctx.models.data.vfs.paths(|x| x.starts_with(&stem) && x.ends_with(".kf") && !x[self.folder.len()..].contains('\\')).into_iter().min();
        }
        let found = path.and_then(|path| {
            let clip = ctx.clip(&path)?;
            Some(Arc::new(ClipInfo::new(path, clip.clone(), clip.root_speed())))
        });
        match &found {
            Some(c) if key.limping() => info!("player {:?} rig: limping {key:?} plays {}", self.view, c.path),
            Some(c) if matches!(key, ClipKey::Grip(..)) => info!("player {:?} rig: hand grip {}", self.view, c.path),
            Some(c) if matches!(key, ClipKey::Move(..) | ClipKey::PipBoy(_)) => info!("player {:?} rig: {key:?} plays {}", self.view, c.path),
            // Without it the arm never comes up (#1330): said where a player's log shows it.
            None if matches!(key, ClipKey::PipBoy(_)) => warn!("player {:?} rig: {key:?}: none of {:?} in {} could be read; the Pip-Boy cannot come up", self.view, key.candidates(), self.folder),
            Some(c) => debug!("player {:?} rig: {key:?} plays {} ({:.3} s, shot at {:?}, repeats to {:?})", self.view, c.path, c.clip.length(), c.keys.hit, c.keys.loop_end),
            None => debug!("player {:?} rig: {key:?}: none of {:?} in {}", self.view, key.candidates(), self.folder),
        }
        self.clips.insert(key, found.clone());
        found
    }

    /// The up or down form of clip `c` (`2hrattackleftup.kf` beside `2hrattackleft.kf`),
    /// if its folder has one (cached, failures too).
    fn variant(&mut self, ctx: &mut ActorCtx, c: &ClipInfo, aim: Aim) -> Option<Arc<ClipInfo>> {
        let path = aim_variant(&c.path, aim);
        if let Some(v) = self.variants.get(&path) {
            return v.clone();
        }
        let found = if ctx.models.data.vfs.exists(&path) { ctx.clip(&path) } else { None }.map(|clip| Arc::new(ClipInfo::new(path.clone(), clip, None)));
        debug!("player {:?} rig: {aim:?} form of {}: {}", self.view, c.path, if found.is_some() { &path } else { "none" });
        self.variants.insert(path, found.clone());
        found
    }

    /// The draw and holster timings of weapon `prefix` for this rig.
    fn transitions(&mut self, ctx: &mut ActorCtx, stance: Stance, prefix: &'static str) -> (Transition, Transition) {
        let draw = self.clip(ctx, ClipKey::Equip(stance, prefix, true));
        let away = self.clip(ctx, ClipKey::Equip(stance, prefix, false));
        (
            draw.map_or(Transition::new(0.0, None), |c| Transition::new(c.clip.length(), c.keys.attach)),
            away.map_or(Transition::new(0.0, None), |c| Transition::new(c.clip.length(), c.keys.detach)),
        )
    }
}

struct Body {
    worn: Vec<FormId>,
    /// The look chosen in the character creator the body was built with (`None`: the
    /// player record's).
    look: Option<fnv_actor::Look>,
    /// AgeRace's and SexChange's change the body was built with (`ActorWorld::change`,
    /// `crate::appearance`).
    change: fnv_actor::Change,
    female: bool,
    scale: f32,
    third: Rig,
    first: Option<Rig>,
    /// The armour addons (ARMO and WEAP BIPL members, by Editor ID) the first-person rig
    /// wears and drew a shape of, for the log.
    first_addons: Vec<String>,
}

/// State kept across body rebuilds (an outfit change rebuilds the body).
struct Motion {
    weapon: Option<FormId>,
    /// The weapon's mod flags: its model is the one for them (`Weapon::model_for`,
    /// docs/spec-notes/weapon-mods.md §2).
    weapon_mods: u8,
    /// The weapon's models as last drawn, third and first person: a script that sets them
    /// (`crate::weapon_models`) draws the weapon again.
    weapon_models: [Option<String>; 2],
    prefix: &'static str,
    /// The weapon's DNAM Hand Grip ([`Frame::grip`]).
    grip: Option<u8>,
    hands: WeaponHands,
    events: Vec<HandEvent>,
    speed: f32,
    last_feet: Option<[f32; 3]>,
    airborne: f32,
    /// The fastest the character fell in this flight, and at the last landing (units/s):
    /// a harder landing stands up slower (`nvf_play::inertia::MovementInertia::land_stretch`).
    fall_speed: f32,
    landed_speed: f32,
    /// Playing the jump loop: in the air long enough, with the ground far below
    /// ([`FALL_GAP`]); kept until the character is on the ground again.
    falling: bool,
    /// Falls (jump loops started) and landings played, for the summary at exit.
    falls: u32,
    landings: u32,
    last_heading: Option<f32>,
    turn_rate: f32,
    /// The view an idle gave the first-person camera last frame (`Player::body_view`).
    idle_view: Option<Quat>,
    /// Seconds until the line saying where the character stands after an idle let it
    /// go.
    after_idle: Option<f32>,
    /// Once-a-second status (debug log).
    log_elapsed: f32,
    /// On furniture with the body in first person: the view has come within the neck's
    /// reach of the way the body faces, and is held there.
    seat_settled: bool,
    /// The same: the way the body faced last frame, to turn the view with it.
    seat_facing: Option<f32>,
    /// Lying on a bed in first person: where the view looks in the body's own frame.
    lying: Option<LyingView>,
    /// The attack, reload, reload round and spin-up counts last acted on (`WeaponAnim`).
    attacks_seen: u32,
    reloads_seen: u32,
    reload_rounds_seen: u32,
    jams_seen: u32,
    spin_ups_seen: u32,
    wind_ups_seen: u32,
    /// Interned clip names (attack stems, reload names) for the clip keys.
    names: HashMap<String, &'static str>,
    /// Ground velocity in the view's frame (x right, y ahead), smoothed.
    velocity: Vec2,
    /// The first-person arms' sway (`view_sway.rs`).
    sway: ViewSway,
    /// The camera's bob: first person's, and the third-person pivot's smoothing
    /// (`head_bob.rs`).
    head: crate::head_bob::HeadBob,
    /// The last times (seconds of the game's clock) the sway moved the first-person arms and
    /// the head moved the view off the clips' pose (the exit summary, a smoke check).
    moved_at: [Option<f32>; 2],
    /// The first-person weapon lowered against a wall or a friend (`weapon_lower.rs`).
    lower: WeaponLower,
    /// The first-person sprint pose's fade (`sprint_view.rs`).
    sprint: crate::sprint_view::SprintView,
    /// The true iron sights were last reported on the eye (a smoke check).
    sights_told: bool,
    /// The weapon was last reported lowered (a smoke check).
    lower_told: bool,
    /// How far the gun is on its way up to the eye (0–1, not eased; `iron_sights::step`).
    sights_share: f32,
    /// The first-person view's way into a crouch and out of one (0 standing, 1 crouched): a
    /// timer over the clips' cross-over (`Frame::stance_fade`).
    crouch: nvf_play::ease::EaseOut,
    /// The true iron sights' move along the settling curve, with movement inertia, and how
    /// far the gun is moved by it this frame (0–1).
    sights: nvf_play::ease::Settled,
    sights_eased: f32,
    /// The true iron sights could run last frame (the sway's raise follows their share).
    sights_ran: bool,
}

impl Motion {
    fn intern(&mut self, s: String) -> &'static str {
        if let Some(v) = self.names.get(&s) {
            return v;
        }
        let v: &'static str = Box::leak(s.clone().into_boxed_str());
        self.names.insert(s, v);
        v
    }
}

impl Default for Motion {
    fn default() -> Self {
        Motion {
            weapon: None,
            weapon_mods: 0,
            weapon_models: [None, None],
            prefix: UNARMED,
            grip: None,
            hands: WeaponHands::default(),
            events: Vec::new(),
            speed: 0.0,
            last_feet: None,
            airborne: 0.0,
            fall_speed: 0.0,
            landed_speed: 0.0,
            falling: false,
            falls: 0,
            landings: 0,
            last_heading: None,
            turn_rate: 0.0,
            idle_view: None,
            after_idle: None,
            log_elapsed: 0.0,
            seat_settled: false,
            seat_facing: None,
            lying: None,
            attacks_seen: 0,
            reloads_seen: 0,
            reload_rounds_seen: 0,
            jams_seen: 0,
            spin_ups_seen: 0,
            wind_ups_seen: 0,
            names: HashMap::new(),
            velocity: Vec2::ZERO,
            sway: ViewSway::default(),
            head: crate::head_bob::HeadBob::default(),
            moved_at: [None; 2],
            lower: WeaponLower::default(),
            sprint: crate::sprint_view::SprintView::default(),
            sights_told: false,
            lower_told: false,
            sights_share: 0.0,
            crouch: nvf_play::ease::EaseOut::default(),
            sights: nvf_play::ease::Settled::default(),
            sights_eased: 0.0,
            sights_ran: false,
        }
    }
}

#[derive(Resource, Default)]
pub struct PlayerBody {
    /// Draw the body in first person too (`--first-person-body`, the settings page).
    pub first_person_body: bool,
    /// Weapon Inertia: the first-person arms and weapon lag, bob, dip, breathe and recoil,
    /// heavier weapons more (`view_sway.rs`; off with `--no-weapon-sway`, the settings page).
    pub weapon_sway: bool,
    /// Camera Motion: the first-person view bobs, breathes, gets out of breath and jolts when
    /// hit (`head_bob.rs`; off with `--no-head-bob`, the settings page).
    pub head_bob: bool,
    /// Lower the first-person weapon against a wall or a friendly actor close ahead
    /// (`weapon_lower.rs`; off with `--no-weapon-lower`, the settings page).
    pub weapon_lower: bool,
    /// Line the gun's sights up with the eye while aiming (`iron_sights.rs`; off with
    /// `--no-true-iron-sights`, the settings page).
    pub true_iron_sights: bool,
    /// A minigame's close camera wants the player's own body out of its view: the slot
    /// machine's camera in on the reels (`slots_view.rs`), where the seated body's head
    /// and arms came between it and the reels [observed 2026-11-11, test run]. The
    /// body still casts its shadow.
    pub out_of_view: bool,
    body: Option<Body>,
    /// The muzzle node of the weapon in hand in the rig in view, this frame.
    muzzle_entity: Option<Entity>,
    /// Its `ShellCasingNode`, likewise.
    casing_entity: Option<Entity>,
    /// The third-person head's eye this frame, game space, while the view is from
    /// outside; `None` in first person or without a body.
    head_eye: Option<[f32; 3]>,
    /// The weapon muzzle position and barrel forward unit vector (game space) in the rig
    /// in view while a weapon is drawn in hand; `None` when holstered or unarmed.
    weapon_aim: Option<([f32; 3], [f32; 3])>,
    motion: Motion,
    /// Why the body could not be built (reported once).
    failed: Option<String>,
    /// This frame's first-person view takes the turn of the script nodes above the camera
    /// node (`Rig::script_turn`), so [`follow_script_nodes`] keeps it with them.
    script_view: bool,
    /// Set by `ensure_body` when `UpdatePlayer3D` rebuilds the player rigs so
    /// `apply_anim_overrides` forces an immediate override/clip refresh on the same frame
    /// [design].
    force_anim_refresh: bool,
}

impl PlayerBody {
    /// [design: show_rigs] Add-ons follow the part at their attachment, with the body
    /// as the fallback for an attachment outside the known skeleton.
    pub(crate) fn effect_part(&self, attachment: Entity) -> EffectPart {
        let Some(rig) = self.body.as_ref().map(|b| &b.third) else { return EffectPart::Body };
        if let Some(d) = rig.drawn.iter().find(|d| d.entity == attachment) {
            return if d.head { EffectPart::Head } else if d.arm { EffectPart::Arms } else { EffectPart::Body };
        }
        rig.animator.nodes.iter().position(|e| *e == attachment).map_or(EffectPart::Body, |i| effect_bone_part(&rig.animator.skel, i))
    }

    /// [design: show_rigs] The camera's current decision, including the headless body.
    pub(crate) fn sees_effect(&self, part: EffectPart) -> bool {
        self.body.as_ref().and_then(|b| b.third.seen).is_some_and(|s| s.effect_part(part))
    }

    /// The landing clip after a jump or fall is playing (GetAnimAction 12, `anim_state`).
    pub fn landing(&self) -> bool {
        self.body.as_ref().is_some_and(|b| b.third.animator.layers.iter().any(|l| l.tag == TAG_LAND && !l.finished()))
    }

    /// The rigs built, root entity and animator (the body's, then the arms rig's): their
    /// clips drive the materials of their shapes too (`actor_materials.rs`).
    pub fn animators(&self) -> Vec<(Entity, &Animator)> {
        let Some(b) = self.body.as_ref() else { return Vec::new() };
        std::iter::once((b.third.root, &b.third.animator)).chain(b.first.as_ref().map(|f| (f.root, &f.animator))).collect()
    }

    /// Whether the first-person rig is drawn (`pipboy_arm.rs`).
    pub fn first_person_drawn(&self) -> bool {
        self.body.as_ref().and_then(|b| b.first.as_ref()).is_some_and(|f| f.shown == Some(true))
    }

    /// The active main body clip (path under meshes and decoded clip) for the player's
    /// selected view, for `anim_state.rs` and `Game::set_sequence_info` [design].
    pub fn active_clip(&self, first_person: bool) -> Option<(&str, &Arc<Clip>)> {
        let b = self.body.as_ref()?;
        let rig = if first_person { b.first.as_ref().unwrap_or(&b.third) } else { &b.third };
        rig.active_clip.as_ref().map(|(p, c)| (p.as_str(), c))
    }

    /// The real arms rig available for hidden draws [design: pipboy_arm.rs].
    pub fn first_person_root(&self) -> Option<Entity> {
        self.body.as_ref()?.first.as_ref().map(|f| f.root)
    }
    /// A tried-and-missing native raise clip cannot support direct manipulation.
    /// Unknown still uses the ordinary rig.clip load/female-to-male fallback.
    #[cfg(any(target_os = "android", test))]
    pub fn pipboy_clip_missing(&self) -> bool {
        self.body.as_ref().and_then(|body| body.first.as_ref()
            .and_then(|rig|rig.clips.get(&ClipKey::PipBoy(body.female))))
            .is_some_and(Option::is_none)
    }

    /// The root entities of the rigs built (the body's, then the arms rig's).
    pub fn roots(&self) -> Vec<Entity> {
        let Some(b) = self.body.as_ref() else { return Vec::new() };
        std::iter::once(b.third.root).chain(b.first.as_ref().map(|f| f.root)).collect()
    }

    /// Whether the body drawn is a woman's; `None` without a body.
    pub fn female(&self) -> Option<bool> {
        self.body.as_ref().map(|b| b.female)
    }

    /// The third-person head's eye this frame, game space, while the view is from
    /// outside (the flashlight follows it, `flashlight.rs`); `None` in first person or
    /// without a body.
    pub fn head_eye(&self) -> Option<[f32; 3]> {
        self.head_eye
    }

    /// Muzzle position and barrel forward unit vector (game space) of the weapon in hand
    /// in the rig in view, for weapon-attached flashlight alignment (`flashlight.rs`).
    pub fn weapon_aim(&self) -> Option<([f32; 3], [f32; 3])> {
        self.weapon_aim
    }

    #[cfg(test)]
    pub(crate) fn set_weapon_aim_for_test(&mut self, aim: Option<([f32; 3], [f32; 3])>) {
        self.weapon_aim = aim;
    }

    /// The entity of the muzzle node (`ProjectileNode`) of the weapon in hand, in the
    /// rig in view; `None` without a body, a weapon in hand or such a node.
    pub fn muzzle_entity(&self) -> Option<Entity> {
        self.muzzle_entity
    }

    /// The entity of the `ShellCasingNode` of the weapon in hand, in the rig in view, as
    /// [`Self::muzzle_entity`]: where a casing is seen to leave (`shot_fx.rs`).
    pub fn casing_entity(&self) -> Option<Entity> {
        self.casing_entity
    }

    /// The armour the drawn body wears (none before it is built).
    pub fn worn(&self) -> &[FormId] {
        self.body.as_ref().map_or(&[], |b| &b.worn)
    }

    /// The third-person rig as an actor's rig of the player's reference, for its ragdoll
    /// (`death_cam.rs`): its root entity and the rig.
    pub fn third_rig(&self) -> Option<(Entity, crate::limbs::ActorRig)> {
        self.script_rig(false)
    }

    /// Where a player rig's root was posed this frame, before the first-person lens
    /// (`Rig::posed_root`): the rig's place in the world, as the scripts' world
    /// positions take it (`nif_mirror`).
    pub fn posed_root(&self, first_person: bool) -> Option<Transform> {
        let body = self.body.as_ref()?;
        if first_person { body.first.as_ref()?.posed_root } else { body.third.posed_root }
    }

    /// Each built player rig separately, including a hidden rig, for script model
    /// selection. Its names and entities must stay with the same model (#2387).
    pub fn script_rig(&self, first_person: bool) -> Option<(Entity, crate::limbs::ActorRig)> {
        let body = self.body.as_ref()?;
        let r = if first_person { body.first.as_ref()? } else { &body.third };
        let rig = crate::limbs::ActorRig {
            reference: FormId::PLAYER_REF.0,
            base: FormId::PLAYER_BASE.0,
            body: FormId::PLAYER_BASE.0,
            skeleton: r.skeleton.clone(),
            skel: r.animator.skel.clone(),
            nodes: r.animator.nodes.clone(),
            base_scale: 1.0,
        };
        Some((r.root, rig))
    }

    /// Authored CPU-only rigs for the real scene-mirror pipeline controls. This
    /// bypasses asset loading, not script_rig/root/name/publication or edit dispatch.
    #[cfg(test)]
    pub(crate) fn synthetic_script_models(
        third: (Entity, crate::limbs::ActorRig),
        first: Option<(Entity, crate::limbs::ActorRig)>,
    ) -> Self {
        fn rig((root, actor): (Entity, crate::limbs::ActorRig), view: View) -> Rig {
            Rig {
                view, root, lens: None, folder: String::new(), skeleton: actor.skeleton,
                animator: Animator::new(actor.skel, actor.nodes, None),
                clips: HashMap::new(), variants: HashMap::new(), overrides: Arc::default(), active_clip: None, head: None,
                eye_local: Vec3::ZERO, looking: None, camera: None, script_turn: None, posed_root: None, script_used: None, translate: None,
                weapon_node: None, weapon_parent: None, holstered_weapon: None, furniture_logged: None, attached: None,
                weapon_model: None, weapon_emitter: None, weapon_parts: Vec::new(),
                idle_logged: false, sights_turned: None, muzzle: None, muzzle_entity: None,
                casing: None, casing_entity: None, sighting: None, scope_eye: None, sight_relief: None,
                use_lens: false, drawn: Vec::new(), spawned_forms: Vec::new(), shown: Some(false), seen: None,
                furniture: None, since_attack: f32::INFINITY, attack_loop: None,
                wound_up: None, wound_hit: None, sighting2: None, attack_eject: None, ejected: false, carry: None,
            }
        }
        Self {
            body: Some(Body {
                worn: Vec::new(), look: None, change: fnv_actor::Change::default(),
                female: false, scale: 1.0, third: rig(third, View::Third),
                first: first.map(|r| rig(r, View::First)),
                first_addons: Vec::new(),
            }),
            ..Self::default()
        }
    }

    /// Build the actual player rigs through the owning resolver/asset path for
    /// dialogue-provider controls; no ready state or private worn vector is seeded.
    #[cfg(test)]
    pub(crate) fn dialogue_fixture_body(
        commands: &mut Commands, parent: Entity, world: &ActorWorld, ctx: &mut ActorCtx,
        worn: &[FormId], look: Option<&fnv_actor::Look>,
    ) -> Result<Self, String> {
        Ok(Self { body: Some(build(commands,parent,world,ctx,worn,look)?), ..Self::default() })
    }

    /// The clips heard (`anim_sounds.rs`): the animator of the rig in view (the
    /// first-person rig while it is shown, else the body), whether that is the
    /// first-person rig, and the armour worn.
    pub fn heard(&self) -> Option<(&Animator, bool, &[FormId])> {
        let b = self.body.as_ref()?;
        match b.first.as_ref().filter(|f| f.shown == Some(true)) {
            Some(f) => Some((&f.animator, true, &b.worn)),
            None => Some((&b.third.animator, false, &b.worn)),
        }
    }
}

/// Seconds the first-person arms take to ease onto a looping clip again after its seam
/// (95%) [design: the owner's sixth playtest, 2026-11-14, the idle's
/// hitch every 2.5 s].
const SEAM_SECONDS: f32 = 0.3;
/// With movement inertia, the share of a draw's or a holster's time (along its settling
/// curve) after which its clip hands the arms over to what follows (`pose_rig`).
const EQUIP_HANDOVER_FROM: f32 = 0.35;

/// How far a draw's or a holster's clip has handed the arms over (0–1) at `x`, the share of
/// its time: from [`EQUIP_HANDOVER_FROM`] on, by a curve that starts with no speed and no
/// pull, is quickest a little before the middle and tapers off over its last part, coming
/// to rest with no speed, pull or jerk left (its speed goes as `u³(1 − u)⁴`: the integral
/// `70u⁴ − 224u⁵ + 280u⁶ − 160u⁷ + 35u⁸`) [design: the owner, 2026-11-16: "make the last
/// 25% of the equip and holster animations smoother without changing their timing"]. With
/// `6u⁵ − 15u⁴ + 10u³` the hand-over came into the last quarter at nine tenths of its top
/// speed with 29 % still to go and braked all of it there (the 10mm pistol's put-away, 105
/// units/s to none in 0.16 s); now 15 % is left, braked over the whole quarter.
fn handover_share(x: f32) -> f32 {
    let u = ((x - EQUIP_HANDOVER_FROM) / (1.0 - EQUIP_HANDOVER_FROM)).clamp(0.0, 1.0);
    if u < 0.5 {
        u * u * u * u * (70.0 + u * (-224.0 + u * (280.0 + u * (-160.0 + u * 35.0))))
    } else {
        // The same from its end, where the expanded form loses its last digits in f32 (it
        // stepped back by a thousandth): what is left is `v⁵(v³ + 8uv² + 28u²v + 56u³)`.
        let v = 1.0 - u;
        1.0 - v * v * v * v * v * (v * v * v + 8.0 * u * v * v + 28.0 * u * u * v + 56.0 * u * u * u)
    }
}

/// The share of a draw's or a holster's time from which [`equip_curve`] leaves the settling
/// curve for its own end.
const EQUIP_TAIL_FROM: f32 = 0.75;

/// How far a draw or a holster has got (0–1) at `x`, the share of its time, with movement
/// inertia: the settling curve, its last quarter a quintic that carries on from it (place,
/// speed and pull alike) and comes to rest with no speed and no pull left [design: the owner,
/// 2026-11-16: "make the last 25% of the equip and holster animations smoother without
/// changing their timing"]. The settling curve stops with the arms still braking (its pull
/// at the end is 0.8 of a draw a time², the speed dropping to nothing at an angle); this end
/// eases out of the braking too. Same time, still only rising.
fn equip_curve(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= EQUIP_TAIL_FROM {
        return nvf_play::ease::settle(x);
    }
    let h = 1.0 - EQUIP_TAIL_FROM;
    let (p0, v, a) = nvf_assets::anim_blend::settle_slopes(EQUIP_TAIL_FROM);
    // The quintic in s (0–1 over the tail) with p(0), p'(0), p''(0) the curve's and p(1) = 1,
    // p'(1) = p''(1) = 0.
    let (v0, a0, d) = (v * h, a * h * h, 1.0 - p0);
    let c3 = 10.0 * d - 6.0 * v0 - 1.5 * a0;
    let c4 = -15.0 * d + 8.0 * v0 + 1.5 * a0;
    let c5 = 6.0 * d - 3.0 * v0 - 0.5 * a0;
    let s = (x - EQUIP_TAIL_FROM) / h;
    p0 + s * (v0 + s * (0.5 * a0 + s * (c3 + s * (c4 + s * c5))))
}

/// The `x` at which [`equip_curve`] is `y` (by halving; it only rises).
fn equip_curve_inv(y: f32) -> f32 {
    let y = y.clamp(0.0, 1.0);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if equip_curve(mid) < y {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Test aid `NVF_PLAY_TEST_MOTION_TRACE=<file>`: one CSV line a frame of the first-person
/// motion, for finding hitches and two-step moves by numbers: time, frame time, view
/// heading and pitch, the eye (game space), the right hand from the camera node in the
/// arms rig's model space and its turn from the camera node's (scaled axis; the clips'
/// turn (yaw, pitch, roll), the true iron sights' share, aiming, falling, sneaking, the
/// ground speed and a weapon out.
#[derive(Default)]
struct MotionTrace {
    file: Option<std::io::BufWriter<std::fs::File>>,
    /// Beside it (`<file>.layers`): the first-person rig's layers each frame (tag, shown
    /// fade, weight, time / length, rate; `<` leaving), the hands' phase and time and
    /// `seam` where the pose eased across a jump, for telling which clip a hitch in the
    /// trace comes from.
    layers: Option<std::io::BufWriter<std::fs::File>>,
    tried: bool,
}

impl MotionTrace {
    fn wanted(&mut self) -> bool {
        if !self.tried {
            self.tried = true;
            if let Ok(path) = std::env::var("NVF_PLAY_TEST_MOTION_TRACE") {
                match std::fs::File::create(&path) {
                    Ok(f) => {
                        let mut w = std::io::BufWriter::new(f);
                        let _ = std::io::Write::write_all(&mut w, b"t,dt,heading,pitch,eye_x,eye_y,eye_z,hand_x,hand_y,hand_z,hand_rx,hand_ry,hand_rz,sway_x,sway_y,sway_z,sway_yaw,sway_pitch,sway_roll,sights,aiming,falling,sneaking,speed,drawn,feet_z,airborne,lhand_x,lhand_y,lhand_z,lens,rgrip_x,rgrip_y,rgrip_z,rgrip_turn,lgrip_x,lgrip_y,lgrip_z\n");
                        info!("motion trace: writing {path}");
                        self.file = Some(w);
                        self.layers = std::fs::File::create(format!("{path}.layers")).ok().map(std::io::BufWriter::new);
                    }
                    Err(e) => warn!("NVF_PLAY_TEST_MOTION_TRACE: {path}: {e}"),
                }
            }
        }
        self.file.is_some()
    }

    #[allow(clippy::too_many_arguments)]
    fn write(&mut self, t: f64, dt: f32, player: &Player, eye: Option<Vec3>, hand: Option<(Vec3, Vec3)>, sway: SwayPose, flags: (f32, bool, bool, bool, f32, bool), airborne: f32, left: Option<Vec3>, lens: f32, grips: Option<(Vec3, f32, Vec3)>) {
        let Some(w) = self.file.as_mut() else { return };
        let e = eye.unwrap_or(Vec3::NAN);
        let (h, r) = hand.unwrap_or((Vec3::NAN, Vec3::NAN));
        let l = left.unwrap_or(Vec3::NAN);
        let (rg, rt, lg) = grips.unwrap_or((Vec3::NAN, f32::NAN, Vec3::NAN));
        let (yaw, pitch, roll) = sway.rotation.to_euler(EulerRot::ZXY);
        let (sights, aiming, falling, sneaking, speed, drawn) = flags;
        let b = |v: bool| u8::from(v);
        let line = format!(
            "{t:.5},{dt:.5},{:.5},{:.5},{:.3},{:.3},{:.3},{:.4},{:.4},{:.4},{:.6},{:.6},{:.6},{:.4},{:.4},{:.4},{yaw:.5},{pitch:.5},{roll:.5},{sights:.4},{},{},{},{speed:.1},{},{:.3},{airborne:.3},{:.4},{:.4},{:.4},{lens:.5},{:.3},{:.3},{:.3},{rt:.2},{:.3},{:.3},{:.3}\n",
            player.heading, player.pitch, e.x, e.y, e.z, h.x, h.y, h.z, r.x, r.y, r.z, sway.offset.x, sway.offset.y, sway.offset.z, b(aiming), b(falling), b(sneaking), b(drawn), player.feet[2], l.x, l.y, l.z, rg.x, rg.y, rg.z, lg.x, lg.y, lg.z
        );
        let _ = std::io::Write::write_all(w, line.as_bytes());
    }

    /// One line of the layers' file (`layers`).
    fn write_layers(&mut self, t: f64, hands: &WeaponHands, animator: Option<&Animator>) {
        let Some(w) = self.layers.as_mut() else { return };
        let mut line = format!("{t:.5} {:?} {:.4}{}", hands.phase, hands.time, if animator.is_some_and(|a| a.seamed) { " seam" } else { "" });
        for l in animator.map(|a| a.layers.as_slice()).unwrap_or_default() {
            let len = l.clip.length().max(1e-6);
            line += &format!(" | {}{} e{:.4} w{:.3} x{:.4} r{:.3}", if l.is_leaving() { "<" } else { "" }, l.tag, l.shown_ease(), l.weight, l.time / len, l.rate);
        }
        line.push('\n');
        let _ = std::io::Write::write_all(w, line.as_bytes());
    }
}

/// Blows the player took since the body was last posed, as one punch (view frame: x
/// right, y ahead, z up): the arms and the camera's place jolt and settle on their springs
/// (`view_sway.rs`, `head_bob.rs`) [design: the owner, 2026-11-14, "damage screenshake";
/// the camera's rotation is never shaken].
#[derive(Resource, Default)]
struct PlayerBlows {
    punch: Vec3,
    count: u32,
}

/// A blow of `share` (the part of the player's health it took) as a punch: down and back,
/// a little aside by turns.
fn blow_punch(share: f32, n: u32) -> Vec3 {
    let strength = 0.6 + 1.4 * (share.max(0.0) * 4.0).min(1.0);
    let side = if n.is_multiple_of(2) { 1.0 } else { -1.0 } * (0.3 + 0.2 * ((n as f32 * 7.31).sin()).abs());
    Vec3::new(side, -0.4, -1.0) * strength
}

fn collect_blows(mut hits: MessageReader<nvf_play::combat::ActorHit>, mut blows: ResMut<PlayerBlows>) {
    for h in hits.read().filter(|h| h.form_id == FormId::PLAYER_REF.0 && h.share > 0.0) {
        blows.count = blows.count.wrapping_add(1);
        let p = blow_punch(h.share, blows.count);
        blows.punch += p;
    }
}

#[path = "player_sequence_pose_aid.rs"]
mod sequence_pose_aid;

pub struct PlayerBodyPlugin {
    /// `--first-person-body`.
    pub first_person_body: bool,
    /// Not `--no-weapon-sway`.
    pub weapon_sway: bool,
    /// Not `--no-head-bob`.
    pub head_bob: bool,
    /// Not `--no-weapon-lower`.
    pub weapon_lower: bool,
    /// Not `--no-true-iron-sights`.
    pub true_iron_sights: bool,
}

/// The Tweaks tab's key turns true iron sights on and off [design: docs/design/tweaks.md].
fn toggle_iron_sights(input: nvf_play::bindings::ControlInput, mut body: ResMut<PlayerBody>) {
    if !input.suppressed() && input.just_pressed(nvf_play::bindings::Control::ToggleIronSights) {
        body.true_iron_sights = !body.true_iron_sights;
        info!("player body: true iron sights {} (key)", if body.true_iron_sights { "on" } else { "off" });
    }
}

impl Plugin for PlayerBodyPlugin {
    fn build(&self, app: &mut App) {
        // Furniture (`furniture.rs`) says what the body plays before it is posed.
        app.insert_resource(PlayerBody { first_person_body: self.first_person_body, weapon_sway: self.weapon_sway, head_bob: self.head_bob, weapon_lower: self.weapon_lower, true_iron_sights: self.true_iron_sights, ..default() })
            .init_resource::<crate::furniture::BodyOverride>()
            .init_resource::<crate::furniture::PlayerUse>()
            .init_resource::<crate::weapon_lower::Ahead>()
            .init_resource::<WarmPose>()
            .init_resource::<PlayerBlows>()
            .add_systems(Update, collect_blows.in_set(nvf_play::player::BodySet).before(drive_body))
            .init_resource::<crate::script_sequences::PlayerSequences>()
            .add_systems(Update, (restore_warm_pose, crate::furniture::drive_player, ensure_body, crate::weapon_lower::sense, apply_anim_overrides, crate::script_sequences::prepare, drive_body, drive_weapon_emitters, shade_light, draw_warm_pose).chain().in_set(nvf_play::player::BodySet))
            .add_systems(Update, toggle_iron_sights)
            .add_systems(PostUpdate, follow_script_nodes.after(crate::nif_mirror::MirrorSystems).before(bevy::transform::TransformSystems::Propagate))
            .add_systems(PostUpdate, near_shade_casters.after(bevy::light::SimulationLightSystems::CheckLightVisibility))
            .add_systems(PostUpdate, sequence_pose_aid::observe.after(bevy::transform::TransformSystems::Propagate))
            .add_systems(Last, report);
    }
}

/// [design: this file] Only ECS transforms/visibility are borrowed for a render frame.
#[derive(Resource, Default)]
struct WarmPose {
    transforms: Vec<(Entity, Transform)>,
    visibility: Option<(Entity, Visibility)>,
}

/// [design: this file] Marks only culling overrides inserted by the warm-up.
#[derive(Component)]
struct WarmUnculled;

fn restore_warm_pose(
    mut commands: Commands,
    mut pip: ResMut<nvf_play::pipboy::PipBoy>,
    mut saved: ResMut<WarmPose>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    unculled: Query<Entity, With<WarmUnculled>>,
) {
    for (e, before) in saved.transforms.drain(..) {
        if let Ok(mut t) = transforms.get_mut(e) {
            *t = before;
        }
    }
    if let Some((e, before)) = saved.visibility.take() {
        if let Ok(mut v) = visibility.get_mut(e) {
            *v = before;
        }
    }
    // [design: #2862] Keep WarmUnculled/NoFrustumCulling across consecutive warm frames.
    // Removing via Commands here while draw_warm_pose checks Has<NoFrustumCulling> later
    // in the same chain leaves the deferred removal pending, which stripped culling off
    // every alternating warm frame while the rig stood behind the camera.
    if pip.warm_clip().is_none() {
        for e in &unculled {
            commands.entity(e).remove::<(NoFrustumCulling, WarmUnculled)>();
        }
    }
    pip.pose_hidden = false;
}

/// [design: this file] Pose after the normal body has supplied the eye, view and weapon
/// state. Restore animator locals/layers immediately; restore ECS transforms before the
/// next body tick, even if the body then returns early (death, furniture, a missing rig).
#[allow(clippy::too_many_arguments)]
fn draw_warm_pose(
    mut commands: Commands,
    mut pip: ResMut<nvf_play::pipboy::PipBoy>,
    mut state: ResMut<PlayerBody>,
    mut saved: ResMut<WarmPose>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    children: Query<&Children>,
    mut meshes: Query<(Has<NoFrustumCulling>, &mut Mesh3d)>,
    unculled: Query<Entity, With<WarmUnculled>>,
    cameras: Query<(&GlobalTransform, &Projection), With<nvf_play::PlayerCamera>>,
    sizing: Option<Res<nvf_play::pipboy::touch_presentation::PipBoySize>>,
) {
    let clear_unculled = |commands: &mut Commands| {
        for e in &unculled {
            commands.entity(e).remove::<(NoFrustumCulling, WarmUnculled)>();
        }
    };
    let Some(t) = pip.warm_clip() else {
        clear_unculled(&mut commands);
        return;
    };
    let Ok((cam, projection)) = cameras.single() else {
        clear_unculled(&mut commands);
        return;
    };
    // [design: world.rs::setup_root] The rig's parent is in game space, the camera is not.
    let eye = crate::world::bevy_to_game(cam.translation());
    let forward = crate::world::bevy_to_game(cam.forward().as_vec3());
    let Some(body) = state.body.as_mut() else {
        clear_unculled(&mut commands);
        return;
    };
    let female = body.female;
    let Some(rig) = body.first.as_mut() else {
        clear_unculled(&mut commands);
        return;
    };
    let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
    let Some(clip) = rig.clip(&mut ctx, ClipKey::PipBoy(female)) else {
        clear_unculled(&mut commands);
        return;
    };
    // [design: this file] Capture descendants, including extra weapon nodes and the lens.
    let outer = rig.lens.map_or(rig.root, |l| l[0]);
    for e in std::iter::once(outer).chain(children.iter_descendants(outer)) {
        if let Ok(before) = transforms.get(e) {
            saved.transforms.push((e, *before));
        }
        if let Ok((unculled, mut mesh)) = meshes.get_mut(e) {
            // [design: #1803] Rebuild retained colour bins after borrowing visibility
            // and swapping materials, including retries delayed by an upload.
            mesh.set_changed();
            if !unculled {
                commands.entity(e).insert((NoFrustumCulling, WarmUnculled));
            }
        }
    }
    if let Ok(mut v) = visibility.get_mut(rig.root) {
        saved.visibility = Some((rig.root, *v));
        *v = Visibility::Inherited;
    }
    let a = &mut rig.animator;
    let layers = a.layers.clone();
    let locals = a.locals.clone();
    a.begin();
    let i = a.want(&clip.clip, TAG_PIPBOY, 1.0, Mode::Once, 0.0);
    a.layers[i].time = pipboy_clip_at(&clip.clip, t);
    a.layers[i].ease = 1.0;
    a.end();
    a.pose_unsmoothed();
    a.apply(&mut transforms);
    a.layers = layers;
    a.locals = locals;
    // [design: this file] Covered draws use the normal lens. The backstop also places
    // the rig behind the camera, so FOV/aspect cannot expose any of the down pose.
    let covered = pip.warming.as_ref().is_some_and(|w| w.covered);
    if !covered {
        if let Ok(mut root) = transforms.get_mut(outer) {
            root.translation = eye - forward * 10_000.0;
        }
    } else if let (Some([_, inner]), Projection::Perspective(p)) = (rig.lens, projection) {
        if let Ok(mut lens) = transforms.get_mut(inner) {
            let k = arms_magnification(p.fov, sizing.as_deref().map_or(PIPBOY_FOV, |s| s.device_fov(PIPBOY_FOV)));
            lens.scale = Vec3::new(k, 1.0, k);
        }
    }
    pip.warming.as_mut().expect("warm pose").drawn = true;
    pip.pose_hidden = true;
}

/// The body's falls at exit (smoke tests read it).
fn report(mut exit: MessageReader<AppExit>, body: Res<PlayerBody>) {
    if exit.read().next().is_some() {
        let m = &body.motion;
        info!("player body summary: {} falls (the jump loop), {} landings", m.falls, m.landings);
        info!(
            "player body summary: weapon sway {}, most turn lag {:.2} degrees sideways and {:.2} up or down, deepest bob {:.2} units, deepest jump or landing dip {:.2} units, weapon weight {}",
            if body.weapon_sway { "on" } else { "off" },
            m.sway.peak_lag.x.to_degrees(),
            m.sway.peak_lag.y.to_degrees(),
            m.sway.peak_dip,
            m.sway.peak_lift_dip.max(0.0) + 0.0,
            if body.weapon_sway { "on" } else { "off" }
        );
        let (head, pivot) = m.head.pivot_rms();
        info!("player body summary: head bob {}, deepest first-person dip {:.2} units; third-person pivot over {} frames moved {pivot:.3} units a frame (rms) against the head's {head:.3}, pivot/head {:.2}", if body.head_bob { "on" } else { "off" }, m.head.peak_dip, m.head.frames, pivot / head.max(1e-3));
        info!("player body summary: head inertia, deepest take-off or landing dip {:.2} units; breathing camera {}, highest rise {:.2} units", m.head.peak_land.max(0.0) + 0.0, if body.head_bob { "on" } else { "off" }, m.head.peak_breath.max(0.0) + 0.0);
        info!("player body summary: most gun recoil back {:.2} units, most out of breath {:.2}", m.sway.peak_recoil.max(0.0) + 0.0, m.head.peak_exertion);
        info!(
            "player body summary: off the clips' pose, the arms last moved by Weapon Inertia {}, the view by the head's springs (Camera Motion, Movement Inertia's landing dip) {}",
            m.moved_at[0].map_or("never".to_string(), |t| format!("at {t:.1} s")),
            m.moved_at[1].map_or("never".to_string(), |t| format!("at {t:.1} s"))
        );
        info!("player body summary: weapon lowering {}, lowered {} times, most {:.2}", if body.weapon_lower { "on" } else { "off" }, m.lower.times, m.lower.peak);
    }
}

/// The player's equipped armour and weapon in the simulation, and the weapon's mods.
fn equipped(sim: &SimState, world: &ActorWorld) -> (Vec<FormId>, Option<FormId>, u8) {
    let Some(inv) = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()) else { return (Vec::new(), None, 0) };
    let eq = inv.equipped();
    let mut worn: Vec<FormId> = eq.iter().copied().filter(|id| world.actors.armors.contains_key(id)).collect();
    let weapon = eq.iter().copied().find(|id| world.actors.weapons.contains_key(id));
    // A weapon with "Hide Backpack" goes with the armour: while it is equipped the body is
    // dressed without the pieces in the Backpack slot (`Resolver::resolve_wearing`).
    // So does one with armour addons of its own (WEAP BIPL: a hand-to-hand weapon's hand
    // model, `Resolver::resolve_wearing`), for the third-person body and, when the weapon
    // has no model of its own, the first-person arms ([`build`]).
    worn.extend(weapon.filter(|w| world.actors.weapons.get(w).is_some_and(|x| x.hide_backpack || !x.addons.is_empty())));
    (worn, weapon, weapon.map_or(0, |w| inv.equipped_mods(w)))
}

/// The centre of the eye parts in the head bone's space, from their meshes [observed:
/// the race's eye parts hang from `Bip01 Head` by `Prn`, actors.md §1.2].
fn eye_centre(skel: &PreparedSkeleton, head: usize, parts: &[&nvf_assets::actor::PreparedPart]) -> Option<Vec3> {
    let mut sum = Vec3::ZERO;
    let mut n = 0.0;
    for part in parts {
        let Some(&b) = skel.by_name.get(part.attach_bone()) else { continue };
        for s in &part.shapes {
            let Some(VertexAttributeValues::Float32x3(pos)) = s.cpu.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
            if pos.is_empty() {
                continue;
            }
            let c = pos.iter().fold(Vec3::ZERO, |a, p| a + Vec3::from_array(*p)) / pos.len() as f32;
            // Into the attachment bone's space, then the head's (normally the same bone).
            let local = rigid_local(skel, b, &part.root, &s.transform).transform_point3(c);
            let world = skel.bind_world[b].transform_point3(local);
            sum += skel.bind_world[head].inverse().transform_point3(world);
            n += 1.0;
        }
    }
    (n > 0.0).then(|| sum / n)
}

/// A point in front of the head when the eyes cannot be measured: 9 units forward and 7
/// up from the head bone at the bind pose [engineering choice].
fn default_eye(skel: &PreparedSkeleton, head: usize) -> Vec3 {
    let bind = skel.bind_world[head];
    let world = bind.translation + Vec3A::new(0.0, 9.0, 7.0);
    bind.inverse().transform_point3(world.into())
}

#[allow(clippy::too_many_arguments)]
fn ensure_body(
    mut commands: Commands,
    world: Option<Res<ActorWorld>>,
    sim: Res<SimState>,
    mut state: ResMut<PlayerBody>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
    root: Query<Entity, With<WorldRoot>>,
    (death, knock): (Res<crate::death_cam::DeathCam>, Res<crate::player_knockdown::PlayerKnockdown>),
    (mut script, real, mut last_look): (Option<ResMut<nvf_play::script::ScriptState>>, Res<Time<Real>>, Local<f32>),
) {
    let (Some(world), Ok(world_root)) = (world, root.single()) else { return };
    let (update_3d, equip_mask) = match script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        Some(rt) => (rt.game.take_update_player_3d(), rt.game.take_player_equipped_model_reload()),
        None => (false, 0),
    };
    // The dead or knocked-down player's body lies as its ragdoll left it: it is not
    // rebuilt.
    if state.failed.is_some() || death.down() || knock.ragdolled() {
        return;
    }
    let (worn, weapon, weapon_mods) = equipped(&sim, &world);
    // The look chosen in the character creator (docs/spec-notes/character-creator.md §5).
    let look = script.as_deref().and_then(|s| s.rt.as_ref()).and_then(|rt| rt.game.player.look.as_ref()).map(nvf_play::creator::actor_look);
    let state = &mut *state;
    let force_body = update_3d || (equip_mask & !fnv_game::nvse_gaps::EQUIP_SLOT_WEAPON_BIT) != 0;
    let force_weapon = update_3d || (equip_mask & fnv_game::nvse_gaps::EQUIP_SLOT_WEAPON_BIT) != 0;
    if update_3d {
        state.force_anim_refresh = true;
    }
    let mut rebuilt = false;
    // A new look rebuilds the body, at most every [`LOOK_REBUILD`] seconds while a
    // slider is dragged [design]. Explicit UpdatePlayer3D / non-weapon ReloadEquippedModels
    // bypasses the slider throttle so scripted 3D/equipment reloads apply immediately.
    // AgeRace and SexChange before the creator gave a look (`crate::appearance`).
    let change = world.change(FormId::PLAYER_REF.0);
    let look_changed = state.body.as_ref().is_some_and(|b| b.look != look || b.change != change);
    let look_due = look_changed && real.elapsed_secs() - *last_look >= LOOK_REBUILD;
    if state.body.as_ref().is_none_or(|b| b.worn != worn) || look_due || force_body {
        if look_due {
            *last_look = real.elapsed_secs();
            debug!("player body: the look changed; rebuilt");
        } else if update_3d {
            debug!("player body: rebuilt for UpdatePlayer3D");
        } else if force_body {
            debug!("player body: rebuilt for ReloadEquippedModels ({equip_mask:#x})");
        }
        let old = state.body.take();
        if let Some(old) = &old {
            commands.entity(old.third.root).despawn();
            if let Some(f) = &old.first {
                commands.entity(f.lens.map_or(f.root, |[outer, _]| outer)).despawn();
            }
        }
        let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
        match build(&mut commands, world_root, &world, &mut ctx, &worn, look.as_ref()) {
            Ok(mut b) => {
                // The new body goes on with the old one's clips and pose: no T-pose while
                // its clips would fade in [owner 2026-10-17: "a few frames where the model
                // is t posing"].
                if let Some(old) = &old {
                    b.third.animator.continue_from(&old.third.animator);
                    info!("player body: rebuilt; it goes on with the old body's {} clips and pose", b.third.animator.layers.len());
                    if let (Some(n), Some(o)) = (b.first.as_mut(), old.first.as_ref()) {
                        n.animator.continue_from(&o.animator);
                    }
                }
                info!(
                    "player body: third-person rig {} nodes (eye {:.1?} in the head's space), first-person rig {}, wearing {:?} with the addons {:?}",
                    b.third.animator.nodes.len(),
                    b.third.eye_local,
                    b.first.as_ref().map_or("missing".to_string(), |f| format!("{} nodes from {}", f.animator.nodes.len(), f.folder)),
                    worn.iter().map(|w| w.to_string()).collect::<Vec<_>>(),
                    // The armour addons of their biped model lists the first-person rig
                    // wears (`fnv_actor::first_person::worn_addons`; a held fist weapon's
                    // hand model among them, #3226).
                    b.first_addons
                );
                state.body = Some(b);
                rebuilt = true;
            }
            Err(e) => {
                warn!("player body: {e}; playing without one");
                state.failed = Some(e);
                return;
            }
        }
    }
    let Some(body) = state.body.as_mut() else { return };
    let m = &mut state.motion;
    let w = weapon.and_then(|id| world.actors.weapons.get(&id));
    let drawn = [false, true].map(|first| w.and_then(|w| w.model_for(first, weapon_mods)));
    if !rebuilt && !force_weapon && m.weapon == weapon && m.weapon_mods == weapon_mods && m.weapon_models.iter().zip(drawn).all(|(a, b)| a.as_deref() == b) {
        return;
    }
    m.weapon = weapon;
    m.weapon_mods = weapon_mods;
    m.weapon_models = drawn.map(|d| d.map(str::to_string));
    m.prefix = w.and_then(|w| w.anim_type.as_deref()).and_then(AnimType::from_name).map_or(UNARMED, AnimType::prefix);
    m.grip = w.and_then(|w| w.grip);
    info!("player weapon: {}, animations {}", weapon.map_or("none (fists)".to_string(), |w| w.to_string()), m.prefix);
    if weapon.is_some() {
        info!("player weapon: mods {weapon_mods} on, models {:?} and {:?} (first person)", w.and_then(|w| w.model_for(false, weapon_mods)), w.and_then(|w| w.model_for(true, weapon_mods)));
    }
    for rig in std::iter::once(&mut body.third).chain(body.first.as_mut()) {
        if let Some(e) = rig.weapon_model.take() {
            commands.entity(e).despawn();
        }
        rig.animator.set_extra(Vec::new());
        rig.weapon_emitter = None;
        rig.muzzle = None;
        rig.muzzle_entity = None;
        rig.casing = None;
        rig.casing_entity = None;
        rig.sighting = None;
        rig.sighting2 = None;
        rig.scope_eye = None;
        rig.sight_relief = None;
        for p in rig.weapon_parts.drain(..).filter(|p| p.on_bone) {
            commands.entity(p.entity).despawn();
        }
        let model = w.and_then(|w| w.model_for(rig.view == View::First, weapon_mods));
        let (Some(path), Some(node)) = (model.map(fnv_actor::resolve::mesh_path), rig.weapon_node) else { continue };
        // In the arms rig's hands the weapon wears its `1stperson` textures where it has
        // them (`nvf_assets::texture::first_person_texture`).
        // With the weapon's alternate textures for the view (texture-swaps.md §4).
        let swaps = weapon.map_or(&[][..], |id| world.actors.weapon_alt_textures(id, rig.view == View::First, weapon_mods));
        let pm = models.model_swapped(&path, rig.view == View::First, swaps);
        match pm {
            Some(pm) => {
                let holder = commands.spawn((Name::new(format!("Player weapon {path}")), Transform::IDENTITY, Visibility::Inherited)).id();
                commands.entity(rig.animator.nodes[node]).add_child(holder);
                // The model's own particle systems emit from the holder as the rig's clips
                // switch them on: the dynamite's fuse sparks while the throw lights it
                // (docs/spec-notes/particles.md §7.6).
                if pm.particles {
                    let names: Vec<String> = models.particles(&path).map(|pp| pp.systems.iter().map(|d| d.name.clone()).collect()).unwrap_or_default();
                    if !names.is_empty() {
                        info!("player weapon model ({:?} person): particle systems {names:?}, emitting as the clips say", rig.view);
                        commands.entity(holder).insert(crate::particles::ModelEmitter::held(path.clone()));
                        rig.weapon_emitter = Some((holder, names));
                    }
                }
                // A node of the model whose `UPB` string names a bone hangs its shapes
                // from that bone instead [observed: the minigun's `Backpack` node says
                // `Bip01 Spine2`; hypothesis: that is its meaning; animation.md §9.7].
                let bones = models.node_strings(&path, "UPB");
                let skel = rig.animator.skel.clone();
                let in_hand = m.hands.in_hand;
                // A mine's lit light shows only on a live mine, never in the hand.
                let mine = matches!(m.prefix, "1lm" | "1md");
                // The model's nodes keep their tree and their names, so the clips can
                // move its parts [observed: the clips' tracks on `##Slide`, `##Clip`,
                // `##MinigunBarrel` name nodes of the weapon models; animation.md §9.8].
                let local = |a: &Affine3A| Transform::from_matrix(Mat4::from(*a));
                let part_nodes: Vec<Entity> = pm.nodes.iter().map(|n| commands.spawn((local(&n.local), Visibility::Inherited)).id()).collect();
                for (n, e) in pm.nodes.iter().zip(&part_nodes) {
                    commands.entity(n.parent.map_or(holder, |p| part_nodes[p])).add_child(*e);
                }
                let mut extra: Vec<ExtraNode> = Vec::new();
                // Each node's place among the extra nodes, if it has a name.
                let extra_of: Vec<Option<usize>> = pm
                    .nodes
                    .iter()
                    .zip(&part_nodes)
                    .map(|(n, e)| {
                        let name = n.name.clone()?;
                        extra.push(ExtraNode { name, entity: *e, bind: local(&n.local) });
                        Some(extra.len() - 1)
                    })
                    .collect();
                let muzzle = nvf_assets::model::node_path(&pm.nodes, &MUZZLE_NODES);
                rig.muzzle_entity = muzzle.as_ref().and_then(|path| path.last()).map(|&i| part_nodes[i]);
                rig.muzzle = muzzle.map(|path| path.into_iter().map(|i| (extra_of[i], local(&pm.nodes[i].local))).collect());
                let casing = nvf_assets::model::node_path(&pm.nodes, &["ShellCasingNode", "##ShellCasingNode"]);
                rig.casing_entity = casing.as_ref().and_then(|path| path.last()).map(|&i| part_nodes[i]);
                rig.casing = casing.map(|path| path.into_iter().map(|i| (extra_of[i], local(&pm.nodes[i].local))).collect());
                rig.sighting = nvf_assets::model::node_path(&pm.nodes, &crate::iron_sights::SIGHTING_NODES).map(|path| path.into_iter().map(|i| (extra_of[i], local(&pm.nodes[i].local))).collect());
                rig.sighting2 = nvf_assets::model::node_path(&pm.nodes, &["##SightingNode2"]).map(|path| path.into_iter().map(|i| (extra_of[i], local(&pm.nodes[i].local))).collect());
                // A new weapon starts on its first sight (the Toggle Sighting Node key).
                nvf_play::tweaks::SECOND_SIGHT.store(false, std::sync::atomic::Ordering::Relaxed);
                if rig.view == View::First {
                    // 3D scopes: the lens is drawn on an entity at the eyepiece, its disc
                    // (+Z of a Bevy circle) facing back up the barrel (−X), its up the
                    // model's +Y, its right +Z.
                    rig.scope_eye = crate::scope_lens::eyepiece(&path);
                    if let Some(e) = rig.scope_eye {
                        let turn = Quat::from_mat3(&Mat3::from_cols(Vec3::Z, Vec3::Y, Vec3::NEG_X));
                        let mount = commands.spawn((Name::new("Scope lens mount"), crate::scope_lens::LensMount { radius: e.radius }, Transform::from_translation(e.centre).with_rotation(turn), Visibility::Inherited)).id();
                        commands.entity(holder).add_child(mount);
                        info!("scope lens: {path} eyepiece at ({:.2}, {:.2}, {:.2}), radius {:.2}", e.centre.x, e.centre.y, e.centre.z, e.radius);
                    }
                    info!("player weapon model (First person): {}", if rig.sighting.is_some() { "a sighting node: aimed with the down-the-sights clips" } else { "no sighting node: aimed with the hip clips, the view zoomed" });
                }
                for s in &pm.shapes {
                    let bone = s.ancestors.iter().find_map(|a| skel.by_name.get(bones.get(a)?.trim()).copied());
                    if bone.is_some() && rig.view == View::First {
                        // On the body, behind the first-person view.
                        continue;
                    }
                    let tags: Vec<(String, bool)> =
                        s.ancestors.iter().chain(s.name.as_ref()).filter(|n| n.starts_with("##")).map(|n| (n.clone(), true)).collect();
                    let never = mine && nvf_play::mines::lit_part(s.name.as_deref(), &s.ancestors);
                    let shown = !never && (tags.is_empty() || in_hand);
                    let visibility = if shown { Visibility::Inherited } else { Visibility::Hidden };
                    // On a bone of its own the shape keeps its place in the whole model;
                    // else it sits under its node, which the clips may move.
                    let (parent, place) = match bone {
                        Some(b) => (rig.animator.nodes[b], local(&s.transform)),
                        None => (s.node.map_or(holder, |n| part_nodes[n]), local(&s.own)),
                    };
                    // The shadow on the world is the third-person body's, weapon
                    // included: the arms rig's weapon casts none there, and takes none
                    // from that body.
                    let e = match rig.view {
                        View::Third => commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(s.material.clone()), place, visibility)).id(),
                        View::First => {
                            let material = arms_material(&mut models.materials, &s.material);
                            commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(material), place, visibility, RenderLayers::layer(ARMS_LAYER))).id()
                        }
                    };
                    commands.entity(parent).add_child(e);
                    // Named, so the clips can drive its glow (`##Bulbs:0` in `1hpaim.kf`;
                    // actor_materials.rs).
                    if let Some(name) = &s.name {
                        commands.entity(e).insert((nvf_assets::actor::ShapeName(name.to_ascii_lowercase()), StoredLook(s.look)));
                    }
                    if let (None, Some(name)) = (bone, &s.name) {
                        extra.push(ExtraNode { name: name.clone(), entity: e, bind: place });
                    }
                    rig.weapon_parts.push(WeaponPart { entity: e, tags, on_bone: bone.is_some(), never, shown: Some(shown) });
                }
                rig.weapon_model = Some(holder);
                rig.seen = None;
                info!(
                    "player weapon model ({:?} person): {path}, {} nodes, {} named parts for the clips, {}",
                    rig.view,
                    pm.nodes.len(),
                    extra.len(),
                    match &rig.muzzle {
                        Some(p) => format!("{} {} node(s) down: shots are drawn from it", MUZZLE_NODES[0], p.len()),
                        None => format!("no {}: shots are drawn from the eye", MUZZLE_NODES[0]),
                    }
                );
                rig.animator.set_extra(extra);
            }
            None => warn!("player weapon {path}: model not loaded"),
        }
    }
}

/// The least time between two body rebuilds for a changed look, seconds.
const LOOK_REBUILD: f32 = 0.1;

fn build(commands: &mut Commands, parent: Entity, world: &ActorWorld, ctx: &mut ActorCtx, worn: &[FormId], look: Option<&fnv_actor::Look>) -> Result<Body, String> {
    let a = world.resolve_as(FormId::PLAYER_BASE.0, FormId::PLAYER_REF.0, Some(worn), look)?;
    // The armour pieces drawn (an addon in the Backpack slot is left out while a weapon
    // with Hide Backpack is held).
    let mut drawn: Vec<String> = Vec::new();
    for p in a.parts.iter().filter(|p| p.role == "armor") {
        let name = world.actors.armors.get(&p.form).and_then(|x| x.edid.clone()).unwrap_or_else(|| p.form.to_string());
        if !drawn.contains(&name) {
            drawn.push(name);
        }
    }
    info!("player body: armour drawn {drawn:?}");
    let scale = a.race_height as f32;
    let third = build_rig(commands, parent, world, ctx, &a, View::Third, None)?;
    let vfs = ctx.models.data.vfs.clone();
    // A held weapon's own addons (WEAP BIPL, `equipped`) are worn by the first-person
    // arms only when the weapon has no model of its own for that view: the Bladed
    // Gauntlet's or the knuckles' hand model is all there is of them (#3226). The power
    // fist's metal hand stays the third-person body's: its rigid model is drawn over the
    // arms' bare hand (`first_person::wears_weapon_addons`).
    let first_worn = first_person::first_person_worn(&world.actors, worn);
    let first_a = if first_worn.len() == worn.len() { None } else { Some(world.resolve_as(FormId::PLAYER_BASE.0, FormId::PLAYER_REF.0, Some(&first_worn), look)?) };
    let (fa, shows) = first_person::appearance(&world.actors, first_a.as_ref().unwrap_or(&a), |p| vfs.exists(p));
    let first = match build_rig(commands, parent, world, ctx, &fa, View::First, Some(&shows)) {
        Ok(r) => Some(r),
        Err(e) => {
            warn!("player body: no first-person rig ({e}); the third-person body's head gives the view");
            None
        }
    };
    // Of those, the ones the built rig drew a shape of (none without a rig).
    let first_addons = first.as_ref().map_or_else(Vec::new, |r| {
        first_person::worn_addons(&world.actors, &first_worn, &fa)
            .into_iter()
            .filter(|x| r.spawned_forms.contains(x))
            .map(|x| world.actors.armors.get(&x).and_then(|x| x.edid.clone()).unwrap_or_else(|| x.to_string()))
            .collect()
    });
    Ok(Body { worn: worn.to_vec(), look: look.cloned(), change: world.change(FormId::PLAYER_REF.0), female: a.female, scale, third, first, first_addons })
}

/// Spawn one rig of appearance `a` under `parent` (its transform set each frame).
/// `shows` (first person): per part of `a`, how it shows.
fn build_rig(commands: &mut Commands, parent: Entity, world: &ActorWorld, ctx: &mut ActorCtx, a: &Appearance, view: View, shows: Option<&[Show]>) -> Result<Rig, String> {
    let skel = ctx.skeleton(&a.skeleton).ok_or_else(|| format!("skeleton {} not loaded", a.skeleton))?;
    let mut stats = crate::actors::ActorStats::default();
    let mut uses = part_uses(a, world, ctx, &mut stats);
    if stats.parts_missing > 0 {
        warn!("player {view:?} rig: {} parts not loaded", stats.parts_missing);
    }
    if let Some(shows) = shows {
        // Upper-body models show their `Arms` shapes and the left forearm sleeve only
        // (`PipBoyOn…` or `PipBoyOff…`, whichever the player wears). A model with no
        // `Arms…` shape draws the arm partitions of its skins instead
        // (`first_person::arms_by_partition`, #2457).
        for (i, u) in &mut uses {
            if shows[*i] == Show::Arms {
                let mut cut: Vec<String> = Vec::new();
                if first_person::arms_by_partition(u.part.shapes.iter().filter_map(|s| s.name.as_deref())) {
                    u.meshes.resize(u.part.shapes.len(), None);
                    for (si, s) in u.part.shapes.iter().enumerate() {
                        let Some(name) = s.name.as_deref() else { continue };
                        if s.cap || !first_person::partition_arm_shape(name, s.partitions.iter().map(|p| p.body_part)) {
                            continue;
                        }
                        let mesh = nvf_assets::actor::dismember_mesh(&s.cpu, &s.partitions, |p| !p.cap && first_person::is_arm_body_part(p.body_part));
                        u.meshes[si] = Some(ctx.models.meshes.add(mesh));
                        cut.push(name.to_string());
                    }
                    let p = &a.parts[*i];
                    info!("player First rig part {} {}: no Arms shape, the arm partitions of {cut:?} are drawn", p.form, p.model);
                }
                let hide = u.part.shapes.iter().filter_map(|s| s.name.clone().filter(|n| !is_first_person_shape(n) && !cut.contains(n)));
                u.hidden_shapes.extend(hide);
            }
        }
    }
    for (i, u) in &uses {
        let p = &a.parts[*i];
        let drawn: Vec<&str> = u.part.shapes.iter().filter_map(|s| s.name.as_deref()).filter(|n| !u.hidden_shapes.iter().any(|h| h == n)).collect();
        debug!("player {view:?} rig part {} {} {}: shapes {drawn:?}", p.role, p.form, p.model);
    }
    let head = skel.by_name.get("Bip01 Head").copied();
    let eye_local = match view {
        View::Third => {
            let eyes: Vec<_> = uses.iter().filter(|(i, _)| a.parts[*i].role.ends_with("_eye")).map(|(_, u)| &*u.part).collect();
            head.and_then(|h| eye_centre(&skel, h, &eyes).or_else(|| Some(default_eye(&skel, h)))).unwrap_or(Vec3::new(0.0, 0.0, 120.0))
        }
        View::First => Vec3::ZERO,
    };
    let head_part = |i: usize| {
        let p = &a.parts[i];
        p.role.starts_with("head") || p.role == "hair" || world.actors.armors.get(&p.form).is_some_and(|ar| ar.slots & HEAD_SLOTS != 0)
    };
    let hide: Vec<bool> = uses.iter().map(|(i, _)| head_part(*i)).collect();
    // Third person: per part, how the first-person rig shows it.
    let arms: Vec<Option<Show>> = uses.iter().map(|(i, _)| (view == View::Third).then(|| first_person::show(&world.actors, &a.parts[*i])).flatten()).collect();
    let part_index: Vec<usize> = uses.iter().map(|(i, _)| *i).collect();
    let mut uses: Vec<PartUse> = uses.into_iter().map(|(_, u)| u).collect();
    if view == View::First {
        // The arms take no shadow from the unseen body they are in the middle of.
        for u in &mut uses {
            u.materials = (u.part.shapes.iter().enumerate()).map(|(i, s)| arms_material(&mut ctx.models.materials, u.materials.get(i).unwrap_or(&s.material))).collect();
        }
    }
    // The first-person rig hangs under two nodes that can draw it bigger about the eye
    // ([`place_lens`]).
    let lens = (view == View::First).then(|| {
        let outer = commands.spawn((Name::new("Arms lens"), Transform::IDENTITY, Visibility::Inherited, ChildOf(parent))).id();
        let inner = commands.spawn((Transform::IDENTITY, Visibility::Inherited, ChildOf(outer))).id();
        [outer, inner]
    });
    let parent = lens.map_or(parent, |[_, inner]| inner);
    let spawned = spawn_actor(commands, parent, Transform::from_scale(Vec3::splat(a.race_height as f32)), &skel, &uses);
    commands.entity(spawned.root).insert(Name::new(format!("Player body ({view:?} person)")));
    let drawn = (spawned.drawn.iter().zip(&spawned.part_of).zip(&spawned.shape_of))
        .map(|((&entity, &p), &s)| Drawn { entity, head: hide[p], arm: shape_shows(arms[p], uses[p].part.shapes[s].name.as_deref()) })
        .collect();
    let mut spawned_forms: Vec<FormId> = Vec::new();
    for &p in &spawned.part_of {
        let form = a.parts[part_index[p]].form;
        if !spawned_forms.contains(&form) {
            spawned_forms.push(form);
        }
    }
    if view == View::First {
        // The arms rig casts no shadow on the world: the third-person body casts the
        // player's.
        for e in &spawned.drawn {
            commands.entity(*e).insert((RenderLayers::layer(ARMS_LAYER), crate::pipboy_ready::PipBoyShape));
        }
        for ((&e, &p), &s) in spawned.drawn.iter().zip(&spawned.part_of).zip(&spawned.shape_of) {
            let shape = &uses[p].part.shapes[s];
            if let Some(part) = shape.name.as_deref().and_then(crate::pipboy_arm::part) {
                // The CPU copy: the pointer is followed onto the posed shape
                // (`pipboy_arm::follow_pointer`).
                commands.entity(e).insert((part, crate::pipboy_arm::PartMesh(shape.cpu.clone())));
            }
        }
    }
    let folder = a.skeleton.rfind('\\').map_or(String::new(), |i| a.skeleton[..=i].to_string());
    let node = |n: &str| skel.by_name.get(n).copied();
    let weapon_node = node("Weapon");
    let weapon_parent = weapon_node.and_then(|w| skel.parents[w]);
    let (looking, camera, translate) = match view {
        View::First => (node("Bip01 Looking"), node("Camera1st"), node("Bip01 Translate")),
        View::Third => (None, None, None),
    };
    if view == View::First && camera.is_none() {
        return Err(format!("{} has no Camera1st node", a.skeleton));
    }
    let animator = Animator::new(skel.clone(), spawned.nodes, Some("Bip01"));
    Ok(Rig {
        view,
        root: spawned.root,
        lens,
        folder,
        skeleton: a.skeleton.clone(),
        animator,
        clips: HashMap::new(),
        variants: HashMap::new(),
        overrides: Arc::default(),
        active_clip: None,
        head,
        eye_local,
        looking,
        camera,
        script_turn: None,
        posed_root: None,
        script_used: None,
        translate,
        weapon_node,
        weapon_parent,
        holstered_weapon: None,
        furniture_logged: None,
        attached: weapon_parent,
        weapon_model: None,
        weapon_emitter: None,
        weapon_parts: Vec::new(),
        idle_logged: false,
        sights_turned: None,
        muzzle: None,
        muzzle_entity: None,
        casing: None,
        casing_entity: None,
        sighting: None,
        sighting2: None,
        scope_eye: None,
        use_lens: false,
        sight_relief: None,
        drawn,
        spawned_forms,
        shown: None,
        seen: None,
        furniture: None,
        since_attack: f32::INFINITY,
        attack_loop: None,
        wound_up: None,
        wound_hit: None,
        attack_eject: None,
        ejected: false,
        carry: None,
    })
}

/// What both rigs play this frame.
struct Frame {
    /// Smooth weapon animations are on. In the first-person rig ([`pose_rig`]): every
    /// clip's fade follows the settling curve (`anim_blend::Layer::smooth`: quick first,
    /// slow at the end, never straight); and the turning-on-the-spot clips are left out
    /// (they started and stopped with every turn of the view, restarting under an aim: "the
    /// animation resets and kinda skips", the seventh playtest; the arms' turn is the sway's).
    inertia: bool,
    /// The real arm-gesture producer owns clip_time, including settle/cancel.
    pipboy_manual: bool,
    /// Eased crouch is on: in the first-person rig a crouch is kept through a jump (no
    /// standing jump or landing clip while sneaking).
    crouch_jump: bool,
    /// With smooth weapon animations: the aim and block clips cross over this long (seconds)
    /// along the settling curve, the same time and curve as the true iron sights' move to
    /// the eye and the zoom, so going down the sights is one motion [design: the owner's
    /// sixth and seventh playtests, 2026-11-14].
    aim_fade: Option<f32>,
    /// While the first-person view goes into a crouch or out of one: the standing and
    /// sneaking clips (idle, movement, aim) cross over this long (seconds), so the view
    /// and the arms' hold change together, slowly (`nvf_play::inertia::MovementInertia::crouch_seconds`).
    stance_fade: Option<f32>,
    /// With smooth weapon animations: the first-person idle and movement clips cross over
    /// this long (seconds), so stopping and starting ease the arms over.
    move_fade: Option<f32>,
    /// With smooth weapon animations, while a weapon is drawn or put away: how far through its
    /// clip the draw or holster is (0–1, along the settling curve: about the clip's own
    /// speed at first, very slow at the end).
    equip_at: Option<f32>,
    /// With smooth weapon animations: how many times its own length the first-person
    /// landing clip takes (`nvf_play::inertia::LAND_STRETCH`, longer for a hard landing
    /// with Landing weight).
    land_stretch: Option<f32>,
    /// With smooth weapon animations: how many times their base times the hands take to
    /// draw, put away, aim and come back from a reload or an attack (the weapon's handling,
    /// `nvf_play::inertia::MovementInertia::hands`).
    hands_pace: Option<f32>,
    dt: f32,
    stance: Stance,
    dirs: Option<[(Dir, f32); 2]>,
    fast: bool,
    female: bool,
    /// A leg is crippled: standing, the body limps (`locomotion\hurt\` clips).
    limping: bool,
    /// `SetPCToddler 1`: standing, the body plays the toddler walk animation set.
    toddler: bool,
    /// The weapon prefix, and whether the weapon is out.
    prefix: &'static str,
    /// The weapon's DNAM Hand Grip (1 to 6), `None` for the default.
    grip: Option<u8>,
    drawn: Option<&'static str>,
    hands: WeaponHands,
    airborne: f32,
    landing: bool,
    turn_rate: f32,
    pitch: f32,
    /// The third-person body is drawn in first person: it stays upright whatever the
    /// pitch, as the arms rig alone follows the view.
    upright: bool,
    /// Block held.
    aiming: bool,
    speed: f32,
    /// The attack and reload clips of the weapon, and whether each starts this frame.
    attack: Option<&'static str>,
    reload: Option<&'static str>,
    /// The weapon's jam animation (`JamA`), and whether a shot jammed it since the last
    /// frame (combat, the owner's playtest list 5 item 9).
    jam: Option<&'static str>,
    jam_now: bool,
    attack_now: bool,
    reload_now: bool,
    /// A round-by-round reload starts on a round: the reload clip plays from its start.
    reload_round_now: bool,
    /// How fast the reload clips play (`nvf_play::combat::WeaponAnim::reload_rate`).
    reload_rate: f32,
    /// How fast the attack and the draw and holster clips play
    /// (`nvf_play::combat::WeaponAnim::attack_rate`, `equip_rate`).
    attack_rate: f32,
    equip_rate: f32,
    /// The weapon starts to spin up this frame: its attack clip plays from the start.
    spin_now: bool,
    /// A pull starts a shot that leaves at the clip's `Hit` key (a throw's release): the
    /// attack clip plays from the start now, and the shot does not restart it
    /// (`nvf_play::combat::WeaponAnim::wind_ups`).
    wind_now: bool,
    /// The throw is held at its release (`nvf_play::combat::WeaponAnim::throw_held`): the
    /// wound-up clip waits at its `Hit` key.
    throw_held: bool,
}

/// Where a rig stands in the frame and what it plays beside its own clips.
struct Posed<'a> {
    root: Transform,
    /// The pitch the first-person rig turns by.
    look_pitch: f32,
    /// The idles the rig plays (`idles.rs`).
    special: &'a IdleSlot<Arc<Clip>>,
    sequences: &'a [crate::script_sequences::Sample],
    /// Script-layer blends can use the device-menu clock while the world is held.
    sequence_dt: f32,
    /// Menu real time for first-person seam smoothing while the Pip-Boy arm clip runs
    /// under a held virtual clock (`--menu-pause original`) [design: #2862].
    pipboy_dt: f32,
    /// The Pip-Boy shows: seconds into its clip (first person).
    pipboy: Option<f32>,
    /// The sway of the arms (first person, `view_sway.rs`).
    sway: SwayPose,
    /// True iron sights (first person, `iron_sights.rs`): how far in (0–1), and the
    /// view's direction in the rig's model space.
    sights: Option<(f32, Vec3)>,
}

/// The turn and offset (in the hand's frame, game units) a Land Mine weapon is given in
/// the first-person hand over what its clips say [observed: fitted to the game's frames of
/// the frag mine, docs/spec-notes/animation.md §9.11].
fn land_mine_grip() -> (Quat, Vec3) {
    (Quat::from_rotation_x(240f32.to_radians()), Vec3::new(4.4, -3.9, 4.5))
}

// Shared by ordinary and direct arm poses: the real Animator samples the clip
// at this absolute time; it never advances an owned finger pose on its own.
fn set_pipboy_layer(a: &mut Animator, clip: &Arc<Clip>, time: f32) {
    let i=a.want(clip,TAG_PIPBOY,1.0,Mode::Once,0.0);
    (a.layers[i].rate,a.layers[i].time)=(0.0,time);
}
fn ease_pipboy_layer(a:&mut Animator,time:f32) {
    for layer in a.layers.iter_mut().filter(|layer|layer.tag==TAG_PIPBOY) {
        (layer.weight,layer.ease)=(1.0,pipboy_ease(time));
    }
}

/// Pose `rig` for the frame, apply the weapon events, place it at its root; returns the
/// eye (game space): the camera node in first person, between the eyes in third. With
/// it, in first person while an idle drives the camera node, the node's rotation (game
/// space): the view the animation gives.
///
/// Once an idle covers the rig (it has faded in, or fades in over the idle before it)
/// the rig's other clips are not played: a special idle takes the place of what the body
/// would do by itself [hypothesis: the clips of the opening share track priorities with
/// the standing idle, so both cannot play; idles.md §4].
fn pose_rig(rig: &mut Rig, f: &Frame, events: &[HandEvent], posed: Posed, ctx: &mut ActorCtx, (transforms, parents): (&mut Query<&mut Transform>, &Query<&ChildOf>)) -> Option<(Vec3, Option<Quat>)> {
    let Posed { root, look_pitch, special, sequences, sequence_dt, pipboy_dt, pipboy, sway, sights } = posed;
    rig.carry = None;
    let pipboy = pipboy.filter(|_| rig.view == View::First).and_then(|t| Some((rig.clip(ctx, ClipKey::PipBoy(f.female))?, t)));
    let draw = rig.clip(ctx, ClipKey::Equip(f.stance, f.prefix, true));
    let put_away = rig.clip(ctx, ClipKey::Equip(f.stance, f.prefix, false));
    let holster = rig.clip(ctx, ClipKey::Holster(f.stance, f.prefix));
    let skel = rig.animator.skel.clone();
    let bone = |name: &str| skel.by_name.get(name).copied();
    for ev in events {
        match ev {
            // At the Equip Speed perks' rate (Quick Draw), as the hands time it.
            // With movement inertia in first person the clip shows at once and the arms ease
            // across the step to its first pose (`Animator::smooth_jump`); its share is then
            // the hand-over's (`EQUIP_HANDOVER_FROM`).
            HandEvent::StartDraw => {
                if let Some(c) = &draw {
                    let warped = f.inertia && rig.view == View::First && f.equip_at.is_some();
                    let l = rig.animator.restart(&c.clip, TAG_EQUIP, 1.0, Mode::Once, if warped { 0.0 } else { c.fade() });
                    rig.animator.layers[l].rate = f.equip_rate;
                    if warped {
                        rig.animator.smooth_jump();
                    }
                }
            }
            HandEvent::StartHolster => {
                if let Some(c) = &put_away {
                    let warped = f.inertia && rig.view == View::First && f.equip_at.is_some();
                    let l = rig.animator.restart(&c.clip, TAG_EQUIP, 1.0, Mode::Once, if warped { 0.0 } else { c.fade() });
                    rig.animator.layers[l].rate = f.equip_rate;
                    if warped {
                        rig.animator.smooth_jump();
                    }
                }
            }
            // The holstered weapon's clip goes at once; with movement inertia in first person
            // the arms ease across what it held (the flamer's draw: a 7 to 86 units/s step at the
            // hand-over, the trace `movement-inertia-trace-weapons`, 2026-11-15).
            HandEvent::ToHand => {
                rig.animator.cut(TAG_HOLSTER);
                if f.inertia && rig.view == View::First && f.equip_at.is_some() {
                    rig.animator.smooth_jump();
                }
            }
            HandEvent::ToHolster | HandEvent::Drawn | HandEvent::Holstered => {}
        }
    }
    // In the hand, the `Weapon` node rests where the draw clip leaves it: the aim and
    // movement clips of some types (`2hh`, `1hp`) do not drive it, and the draw, attack
    // and reload clips that do agree on the place [observed: animation.md §9.7;
    // hypothesis: the game leaves the node where the last clip put it].
    if let Some(n) = rig.weapon_node {
        let rest = draw.as_ref().filter(|_| f.hands.in_hand).and_then(|c| c.weapon_end(skel.locals[n]));
        rig.animator.set_rest(n, rest);
    }
    // So do the weapon's parts the draw clip moves: the 9mm pistol's hammer stays cocked
    // while held, as its draw clip leaves it; the aim clip does not key it [observed:
    // animation.md §9.8, the game's 9mm cocked at idle in the owner's screenshot].
    rig.animator.set_extra_rest(draw.as_ref().filter(|_| f.hands.in_hand).map(|c| &c.clip));
    // In the hand, the weapon hangs from the bone the draw clip's `prn:` names (`Bip01 R
    // ForeTwist` for fists, `Bip01 R Hand` for a rifle), taken from the draw clip of the
    // weapon held now: a weapon equipped while another is out (a script's EquipItem after
    // the fists were drawn) hangs from its own draw clip's bone, not the last one's
    // (before 2026-10-20 the bone was taken only when a draw put the weapon in the hand,
    // and a varmint rifle given after drawing the fists hung from the forearm, below the
    // first-person view).
    if f.hands.in_hand {
        rig.weapon_parent = draw.as_ref().and_then(|c| c.keys.prn.as_ref()).and_then(|(_, b)| bone(b)).or_else(|| bone("Bip01 R Hand"));
        rig.holstered_weapon = None;
    } else {
        // The holster clip's `prn:` names the holster bone [observed: `Bip01 Pelvis` for
        // pistols, `Bip01 Spine2` for rifles, `Bip01 Translate` in first person]; the
        // unequip clip's says the same.
        let from_keys = holster.as_ref().and_then(|c| c.keys.prn.as_ref()).or_else(|| put_away.as_ref().and_then(|c| c.keys.prn.as_ref()));
        if let Some(b) = from_keys.and_then(|(_, b)| bone(b)) {
            rig.weapon_parent = Some(b);
        }
        rig.holstered_weapon = rig.weapon_node.and_then(|n| holster.as_ref()?.weapon_end(skel.locals[n]));
    }

    // The frame's clips.
    let mut want: Vec<(Arc<ClipInfo>, u32, f32, Mode)> = Vec::with_capacity(8);
    if let Some(c) = rig.clip(ctx, ClipKey::Idle(f.stance, f.limping, f.toddler)) {
        want.push((c, TAG_IDLE, 1.0, Mode::Loop));
    }
    let mut expected_speed = 0.0;
    let main_dir = f.dirs.map(|w| if w[0].1 >= w[1].1 { w[0].0 } else { w[1].0 });
    // A crouched jump in first person keeps the crouch: no jump clip, and the crouched walk
    // plays on through the air. The weapons' jump clips stand the body up, and so does the
    // unarmed sneaking one (the eye 118 units over the feet instead of 78), which drew the
    // view up mid-jump and back down after it; and dropping the walk for the idle after
    // 0.15 s in the air lifted the eye 9 units on a hop off a slope [design: the owner's
    // fifth playtest, 2026-11-14: "jumping from crouch … looks like the character stands and
    // then jumps"; tenth: "sometimes still"; the trace `movement-inertia-trace-crouch-jump`].
    let first_inertia = f.inertia && rig.view == View::First;
    let keep_crouch = f.crouch_jump && rig.view == View::First && f.stance == Stance::Sneak;
    if f.airborne >= AIRBORNE_LOOP && !keep_crouch {
        if let Some(c) = rig.clip(ctx, ClipKey::Jump(f.stance, f.drawn, main_dir, false)) {
            want.push((c, TAG_JUMP, 1.0, Mode::Loop));
        }
    } else if let Some(w) = f.dirs {
        for (i, (dir, weight)) in w.into_iter().enumerate() {
            if weight <= 0.0 {
                continue;
            }
            if let Some(c) = rig.clip(ctx, ClipKey::Move(f.stance, f.female, f.fast, dir, f.drawn, f.limping, f.toddler)) {
                expected_speed += weight * c.speed.unwrap_or(0.0);
                want.push((c, TAG_MOVE + i as u32, weight, Mode::Synced));
            }
        }
    } else if f.turn_rate.abs() > TURN_RATE && !first_inertia && !nvf_play::tweaks::NO_TURN_CLIP.load(std::sync::atomic::Ordering::Relaxed) {
        // (The player's No Turning Animation tweak plays none, docs/design/tweaks.md.)
        // Heading grows clockwise: a negative rate turns left.
        if let Some(c) = rig.clip(ctx, ClipKey::Turn(f.stance, f.drawn, f.turn_rate < 0.0, f.limping, f.toddler)) {
            want.push((c, TAG_TURN, 1.0, Mode::Loop));
        }
    }
    // The player's No Landing Animation tweak plays none (docs/design/tweaks.md).
    if f.landing && !keep_crouch && !nvf_play::tweaks::NO_LAND_CLIP.load(std::sync::atomic::Ordering::Relaxed) {
        if let Some(c) = rig.clip(ctx, ClipKey::Jump(f.stance, f.drawn, main_dir, true)) {
            rig.animator.restart(&c.clip, TAG_LAND, 1.0, Mode::Once, c.fade());
        }
    }
    // First person pitches the whole arms rig instead of bending the body.
    let sights_clips = rig.sights_clips(f.aiming);
    let aim_w = match rig.view {
        View::Third if !f.upright => aim_weights(f.pitch),
        _ => [(Aim::Level, 1.0), (Aim::Up, 0.0), (Aim::Down, 0.0)],
    };
    // With movement inertia the first-person hold is asked for through the draw too, under
    // the draw clip (which outranks it): when the draw ends the hold is already there.
    // Asked for only after it, it was still fading in as the draw clip faded out, and the
    // arms-down idle showed through: the gun went down and came up again (seventh playtest).
    // Putting the weapon away the arms go to the arms-down pose under the clip instead (below).
    let holding = if first_inertia { f.hands.phase == WeaponPhase::Drawing || (f.hands.in_hand && f.hands.phase != WeaponPhase::Holstering) } else { f.hands.in_hand && f.hands.phase != WeaponPhase::Drawing };
    if holding {
        if f.aiming && blocks(f.prefix) {
            if let Some(c) = rig.clip(ctx, ClipKey::Block(f.stance, f.prefix)) {
                want.push((c, TAG_AIM, 1.0, Mode::Loop));
            }
        } else {
            for (i, (aim, w)) in aim_w.into_iter().enumerate() {
                // The level clip plays at any weight: the up and down clips of rifles
                // and two-handed melee weapons drive the spine only, so the arms are the
                // level clip's alone and both hands stay on the weapon [observed:
                // animation.md §9.6].
                if w <= 0.0 && aim != Aim::Level {
                    continue;
                }
                if let Some(c) = rig.clip(ctx, ClipKey::Aim(f.stance, f.prefix, aim, sights_clips)) {
                    want.push((c, TAG_AIM + i as u32, w, Mode::Loop));
                }
            }
            // The weapon's hand grip clip over the aim: WEAP DNAM Hand Grip, "a hand pose
            // animation for 3rd person" [documented: GECK wiki Weapons], so the body rig
            // only (the arms rig's folder has the same files: whether the game plays them
            // in first person is not known). A Land Mine weapon with the default grip is
            // held with its type's grip clip in both views: it puts the left hand out of
            // view and the right thumb over the mine's rim [observed: the game's
            // first-person frames of the frag mine (owner 2026-10-23), which show no left
            // hand; `1lmhandgrip1.kf` is the type's only grip clip; hypothesis: the game
            // plays it for the weapon's default grip].
            let grip = match (f.grip, rig.view) {
                (Some(n), View::Third) => Some(n),
                _ if f.prefix == "1lm" => Some(1),
                _ => None,
            };
            if let Some(c) = grip.and_then(|n| rig.clip(ctx, ClipKey::Grip(f.prefix, n))) {
                want.push((c, TAG_GRIP, 1.0, Mode::Loop));
            }
        }
    }
    // With movement inertia in first person the arms-down pose is under the put-away clip all
    // through it, so the clip hands over to it as it ends (below).
    if !f.hands.in_hand || (first_inertia && f.hands.phase == WeaponPhase::Holstering) {
        if let Some(c) = &holster {
            want.push((c.clone(), TAG_HOLSTER, 1.0, Mode::Once));
        }
    }
    if f.hands.in_hand && (f.attack_now || f.spin_now || f.wind_now) {
        if let Some(c) = f.attack.and_then(|a| rig.clip(ctx, ClipKey::Attack(f.stance, f.prefix, a, sights_clips))) {
            // A clip with a `Loop` key plays on through the shots that follow while it
            // runs (a minigun's spin) instead of starting again at each, going round from
            // its `Fire` key to its `Loop` key [documented: GECK wiki NiTextKeyExtraData,
            // combat.md §4.2; observed: the barrel turns at one speed between the two,
            // animation.md §9.8]. A spin-up starts it from the beginning, so the first
            // shot, which combat holds back until then, meets the `Fire` key.
            rig.attack_loop = c.keys.hit.zip(c.keys.loop_end).filter(|(from, to)| from < to).map(|(from, to)| (from - c.clip.start, to - c.clip.start));
            rig.attack_eject = c.keys.eject.map(|e| (c.clip.clone(), e - c.clip.start));
            let playing = rig.animator.layers.iter().any(|l| l.tag == TAG_ATTACK && Arc::ptr_eq(&l.clip, &c.clip) && !l.finished());
            // The shot of a wind-up that started this clip plays on in it.
            let wound = f.attack_now && !f.wind_now && rig.wound_up.take().is_some_and(|w| Arc::ptr_eq(&w, &c.clip)) && playing;
            let going = !f.spin_now && !f.wind_now && (wound || (rig.attack_loop.is_some() && playing));
            if f.wind_now {
                rig.wound_up = Some(c.clip.clone());
                rig.wound_hit = c.keys.hit.map(|h| h - c.clip.start);
                info!("player {:?} rig: {} starts at the pull; the shot leaves at its Hit key", rig.view, c.path);
            }
            if !going {
                // At the attack's rate: the weapon's multipliers and the Modify Attack Speed
                // perks, as combat times the shot. With movement inertia in first person the
                // clip starts at once and the arms ease across the jump back to its start
                // (`Animator::smooth_jump`): its kick shows whole, with no snap between shots
                // [design: the owner's eighth playtest, 2026-11-14, "the jump is not instant to
                // the next recoil point"].
                let l = rig.animator.restart(&c.clip, TAG_ATTACK, aim_w[0].1, Mode::Once, if first_inertia { 0.0 } else { ATTACK_FADE });
                if first_inertia {
                    rig.animator.smooth_jump();
                }
                rig.animator.layers[l].rate = f.attack_rate;
                debug!("player {:?} rig: {} moves the weapon's {:?}", rig.view, c.path, rig.animator.extra_driven(&c.clip));
                // The attack's own up and down forms keep the body bent with the pitch while
                // it plays, as the aim's do [observed: `2hrattackleftup.kf`, §9.6].
                for (i, (aim, w)) in aim_w.into_iter().enumerate().skip(1) {
                    let tag = TAG_ATTACK + i as u32;
                    rig.animator.cut(tag);
                    if let Some(v) = (rig.view == View::Third).then(|| rig.variant(ctx, &c, aim)).flatten() {
                        let l = rig.animator.restart(&v.clip, tag, w, Mode::Once, ATTACK_FADE);
                        rig.animator.layers[l].rate = f.attack_rate;
                    }
                }
            }
            // A spin-up is not a shot: the clip runs on to its `Fire` key by itself.
            if f.attack_now {
                rig.since_attack = 0.0;
            }
        }
    }
    // A reload: its clip, or the opening clip of one that loads round by round; then the
    // reload clip from its start for each round, running on past its `Hit` key to close
    // the weapon after the last (`fnv_combat::attack`).
    if f.hands.in_hand && (f.reload_now || f.reload_round_now) {
        let open = f.reload_now && !f.reload_round_now;
        let key = |r| if open { ClipKey::ReloadOpen(f.stance, f.prefix, r) } else { ClipKey::Reload(f.stance, f.prefix, r) };
        let found = f.reload.and_then(|r| rig.clip(ctx, key(r)).or_else(|| rig.clip(ctx, ClipKey::Reload(f.stance, f.prefix, r))));
        if let Some(c) = found {
            // A round follows straight on from the opening clip or the last round: no
            // fade back through the held pose [hypothesis: the clips are made to chain,
            // the opening's end and the round's `Hit` close to its start pose].
            let fade = if f.reload_round_now || first_inertia { 0.0 } else { c.fade() };
            let layer = rig.animator.restart(&c.clip, TAG_RELOAD, 1.0, Mode::Once, fade);
            // With movement inertia in first person the arms ease into the reload as across a
            // loop's seam: the clip plays whole from its start, the step to it dies away.
            if first_inertia {
                rig.animator.smooth_jump();
            }
            // At the player's reload rate, as the cycle times it (Agility, Rapid Reload).
            rig.animator.layers[layer].rate = f.reload_rate;
            info!("player {:?} rig: {} moves the weapon's {:?} (reload at x{:.2})", rig.view, c.path, rig.animator.extra_driven(&c.clip), f.reload_rate);
        }
    }
    // A jam: the weapon's jam clip (`1hpjama.kf`), played once on the reload layer as a
    // reload is [observed: the clips sit beside the reload clips in both rigs' folders].
    if f.hands.in_hand && f.jam_now {
        if let Some(c) = f.jam.and_then(|j| rig.clip(ctx, ClipKey::Reload(f.stance, f.prefix, j))) {
            rig.animator.restart(&c.clip, TAG_RELOAD, 1.0, Mode::Once, if first_inertia { 0.0 } else { c.fade() });
            if first_inertia {
                rig.animator.smooth_jump();
            }
            debug!("player {:?} rig: jam clip {}", rig.view, c.path);
        }
    }
    if matches!(f.hands.phase, WeaponPhase::Drawing | WeaponPhase::Holstering) {
        let c = if f.hands.phase == WeaponPhase::Drawing { &draw } else { &put_away };
        if let Some(c) = c {
            want.push((c.clone(), TAG_EQUIP, 1.0, Mode::Once));
        }
    }
    // Record the rig's main body clip (movement/jump/turn, else standing idle) for
    // `anim_state.rs` (`GetPlayingAnimSequencePaths`, `GetAnimationAttributes`).
    let main_clip = want
        .iter()
        .filter(|(_, tag, _, _)| *tag == TAG_JUMP || (*tag >= TAG_MOVE && *tag < TAG_MOVE + 4) || *tag == TAG_TURN)
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .or_else(|| want.iter().find(|(_, tag, _, _)| *tag == TAG_IDLE));
    rig.active_clip = main_clip.map(|(c, ..)| (c.path.clone(), c.clip.clone()));
    let a = &mut rig.animator;
    a.begin();
    let covered = special.covers();
    let is_aim = |tag: u32| (TAG_AIM..TAG_AIM + 3).contains(&tag);
    let is_move = |tag: u32| tag == TAG_IDLE || (TAG_MOVE..TAG_MOVE + 4).contains(&tag);
    // An aim clip coming in over another aim clip (the hold to the sights, to a block and
    // back) crosses over the aim's time. One coming in with none there (after a draw)
    // keeps its own, short time: over the aim's, slower than the draw clip leaves, the
    // arms-down idle showed through and the gun went down and came up again (the seventh
    // playtest: "starts pulling out the gun then quickly retracts it").
    let aim_crossing = a.layers.iter().any(|l| is_aim(l.tag) && l.ease > 0.0);
    // The fades with movement inertia, first person: a crouch's, the aim's, a start's or
    // stop's, else the clip's own.
    let crouch_fade = |tag: u32| f.stance_fade.filter(|_| rig.view == View::First && (is_aim(tag) || is_move(tag)));
    let fade_for = |tag: u32| -> Option<f32> {
        if let Some(t) = crouch_fade(tag) {
            return Some(t);
        }
        if !first_inertia {
            return f.aim_fade.filter(|_| is_aim(tag) && aim_crossing);
        }
        match (f.aim_fade, f.move_fade) {
            (Some(t), _) if is_aim(tag) && aim_crossing => Some(t),
            (_, Some(t)) if is_move(tag) => Some(t),
            _ => None,
        }
    };
    for (c, tag, w, mode) in want.iter().filter(|_| !covered) {
        let fade = if *tag == TAG_HOLSTER { 0.0 } else { fade_for(*tag).unwrap_or_else(|| c.fade()) };
        let l = a.want(&c.clip, *tag, *w, *mode, fade);
        if *tag == TAG_EQUIP {
            a.layers[l].rate = f.equip_rate;
        }
    }
    let aims_in = a.layers.iter().any(|l| is_aim(l.tag));
    // The layers leaving take the same times, and with movement inertia every first-person
    // fade follows the settling curve.
    for l in a.layers.iter_mut() {
        if let Some(t) = fade_for(l.tag) {
            l.fade_out = t;
        }
        // A draw's or a holster's clip keeps a straight fade: the hand-over sets it (below).
        if (first_inertia && l.tag != TAG_EQUIP) || (f.aim_fade.is_some() && is_aim(l.tag)) || crouch_fade(l.tag).is_some() {
            l.smooth = true;
        } else if !first_inertia && l.tag < TAG_SPECIAL {
            // None of the rules above holds now (Animation Smoothing, Weapon Handling's aim or
            // the eased crouch switched off, or the crouch done): the rig's own layers take
            // the clips' own straight fade again. Kept, a layer that stays (the idle, a drawn
            // weapon's hold) faded the switched-off way for the rest of the session: the hold
            // crossed over to the sights along the settling curve over the aim's 0.6 s. A layer
            // already leaving keeps its way out, but one left to the draw's hand-over (which
            // no longer runs) goes at the clips' pace instead of never.
            if !l.is_leaving() {
                l.straighten();
                l.fade_out = DEFAULT_FADE;
            } else if !l.fade_out.is_finite() {
                l.fade_out = DEFAULT_FADE;
            }
        }
        // A draw's or a holster's clip lets go slowly: its last pose is not the pose that
        // follows (the arms down after a holster), and over its own short time the
        // hand-over was a second movement.
        if let (true, TAG_EQUIP, Some(t)) = (first_inertia, l.tag, f.move_fade) {
            // A draw's clip with the hold coming in: it goes as the hold comes (below).
            l.fade_out = if aims_in && draw.as_ref().is_some_and(|c| Arc::ptr_eq(&c.clip, &l.clip)) { f32::INFINITY } else { t };
        }
        // A reload, a jam or an attack ending: the arms go back to the hold slowly, along
        // the settling curve, the heavier the weapon the slower (`Frame::hands_pace`).
        if let (true, Some(pace)) = (first_inertia, f.hands_pace) {
            if l.tag == TAG_RELOAD {
                l.fade_out = nvf_play::inertia::RELOAD_OUT_SECONDS * pace;
            } else if (TAG_ATTACK..TAG_ATTACK + 3).contains(&l.tag) {
                l.fade_out = nvf_play::inertia::ATTACK_OUT_SECONDS * pace;
            }
        }
    }
    // A draw or a holster with smooth weapon animations: the clip is played by its place,
    // its motion along the settling curve (`ClipMotion`: `at` is the share of the motion
    // done), and a jump of the clip's own pose is eased across.
    if let Some(at) = f.equip_at {
        let c = if f.hands.phase == WeaponPhase::Drawing { &draw } else { &put_away };
        let mut jumped = false;
        let skel = a.skel.clone();
        for l in a.layers.iter_mut().filter(|l| l.tag == TAG_EQUIP) {
            let len = l.clip.length();
            let x = match c.as_ref().filter(|c| Arc::ptr_eq(&c.clip, &l.clip)) {
                Some(c) => {
                    let x = c.motion(&skel).time_for(at);
                    jumped |= len > 0.0 && c.motion(&skel).jumps_between(l.time / len, x);
                    x
                }
                None => at.clamp(0.0, 1.0),
            };
            (l.time, l.rate) = (x * len, 0.0);
        }
        if jumped {
            a.smooth_jump();
        }
    } else {
        // Played by its rate again (Smooth weapon animations switched off part-way): the
        // clip goes on from where it is instead of holding the place it was put at.
        for l in a.layers.iter_mut().filter(|l| l.tag == TAG_EQUIP && l.rate == 0.0) {
            l.rate = f.equip_rate;
        }
    }
    // Clips that play once to their end once started: landing, attack, reload.
    for (tag, w) in [(TAG_LAND, 1.0), (TAG_ATTACK, aim_w[0].1), (TAG_ATTACK + 1, aim_w[1].1), (TAG_ATTACK + 2, aim_w[2].1), (TAG_RELOAD, 1.0)] {
        if let Some(c) = a.layers.iter().find(|l| l.tag == tag && !l.finished()).map(|l| l.clip.clone()).filter(|_| !covered) {
            a.want(&c, tag, w, Mode::Once, 0.0);
        }
    }
    // A landing with movement inertia: the clip is played by its place along
    // `land_curve` over a longer time, so the view dips at the clip's own pace from the
    // landing frame on and stands up slowly to a soft end (at the clip's own pace the
    // standing up took 0.4 s).
    if let (View::First, Some(stretch)) = (rig.view, f.land_stretch) {
        for l in a.layers.iter_mut().filter(|l| l.tag == TAG_LAND && !l.finished()) {
            let length = l.clip.length().max(1e-3);
            let u = nvf_play::inertia::land_curve_inv((l.time / length).clamp(0.0, 1.0), stretch) + f.dt.max(0.0) / (length * stretch.max(1.0));
            (l.time, l.rate) = (nvf_play::inertia::land_curve(u, stretch) * length, 0.0);
        }
    } else {
        // Played by its clock again (Movement Inertia switched off part-way through a
        // landing): held at the place it was put at, the view stayed dipped.
        for l in a.layers.iter_mut().filter(|l| l.tag == TAG_LAND && l.rate == 0.0) {
            l.rate = 1.0;
        }
    }
    // The idles: each at the time its playback is at. One that has ended holds its last
    // pose until it is gone or the next idle takes over from it (a collection's next
    // idle starts a frame later), so the view it gives does not jump in between; the
    // rig's other clips are back under it meanwhile.
    let mut view_driven = false;
    for (p, w) in special.shown() {
        let newest = special.cur.as_ref().is_some_and(|c| c.serial == p.serial);
        let i = a.want(&p.clip, TAG_SPECIAL + p.serial % 1000, if newest { 1.0 } else { w }, Mode::Once, p.play.keys.blend_in);
        let l = &mut a.layers[i];
        (l.rate, l.time) = (0.0, p.play.time());
        if !newest {
            // The one before: its share shrinks as the new one fades in.
            l.weight = w;
        }
        view_driven |= rig.camera.is_some() && p.clip.tracks.iter().any(|(n, _)| n == crate::idles::CAMERA_NODE);
    }
    for sequence in sequences {
        let tag = 0x4000_0000 | sequence.request.serial;
        if sequence.request.stopped.is_some() {
            if let Some(layer) = a.layers.iter_mut().find(|l| l.tag == tag) {
                layer.fade_out = sequence.request.ease_out;
            }
            continue;
        }
        let mode = if sequence.clip.cycle_type != 2 { Mode::Loop } else { Mode::Once };
        let i = a.want(&sequence.clip, tag, sequence.weight, mode, sequence.fade_in);
        let layer = &mut a.layers[i];
        layer.rate = 0.0;
        layer.time = sequence.time;
        layer.weight = sequence.weight;
        layer.target = sequence.weight;
        layer.weight_as_blend = true;
        layer.additive = sequence.additive.clone();
        layer.sequence_priority = sequence.request.priority;
        layer.fade_out = sequence.request.ease_out;
    }
    // The Pip-Boy: its clip at the time the Pip-Boy is at, over the rest (its tracks'
    // priority is above the others' [observed: 98]); it does not turn the view.
    if let Some((c, t)) = &pipboy {
        set_pipboy_layer(a, &c.clip, pipboy_clip_at(&c.clip, if f.inertia && !f.pipboy_manual { pipboy_clip_time(*t) } else { *t }));
        // Direct manipulation owns the sampled pose and its original blend law.
        // Ordinary automatic raises retain movement-inertia smoothing.
        if f.pipboy_manual {
            for layer in a.layers.iter_mut().filter(|l| l.tag == TAG_PIPBOY) {
                layer.smooth = false;
            }
        }
    }
    a.end();
    // Movement clips play at the speed the character actually moves [hypothesis: the
    // game matches the clip's root motion to the ground speed].
    let rate = if expected_speed > 1.0 { (f.speed / expected_speed).clamp(0.25, 2.5) } else { 1.0 };
    let attack_time = |a: &Animator| {
        let (clip, _) = rig.attack_eject.as_ref()?;
        a.layers.iter().find(|l| l.tag == TAG_ATTACK && Arc::ptr_eq(&l.clip, clip)).map(|l| l.time)
    };
    let before = attack_time(a);
    // A melee attack clip whose root travels, and where it was (its travel carries the
    // player, actors.md §8.13; shots and reloads do not carry [design]).
    let carried = if matches!(f.prefix, "h2h" | "1hm" | "2hm") { a.layers.iter().rev().find(|l| l.tag == TAG_ATTACK && !l.finished()).map(|l| (l.clip.clone(), l.time)) } else { None };
    let carried = carried.filter(|(c, _)| c.root_reach() > nvf_assets::anim_play::CARRY_MIN);
    a.advance(f.dt, rate);
    if rig.view == View::First {
        let seq_extra = (sequence_dt - f.dt).max(0.0);
        a.advance_pose_blends(seq_extra, |layer| layer.tag & 0x4000_0000 != 0);
        a.advance_seam_clock((pipboy_dt - f.dt - seq_extra).max(0.0));
    }
    // A draw or a holster with movement inertia: over the last part of its time the clip
    // hands the arms over to what follows (the hold, the arms down) along the settling
    // curve while it still slows to its end, so the two are one motion. Played to its end
    // first and then faded out, its last pose (3 units off the hold for a 10mm pistol) made
    // a second movement after the arms had nearly stopped [design: the owner's ninth
    // playtest, 2026-11-14: "they slow down but then the animation speeds up"; the trace].
    // The hand-over sets off with no speed and no sudden pull (`handover_share`): along the
    // settling curve, whose start is sharp, the arms of the fists' and the 10mm pistol's
    // holster, nearly stopped, set off again at 350 units/s within two frames (the trace
    // `movement-inertia-trace-weapons`, 2026-11-15).
    // A draw hands over no faster than the hold it hands over to comes in: the hold fades
    // in under the draw at its own pace and shares a priority with it on the spine and
    // the arms, so a draw that let go first left the two together short of the whole
    // (98.7 %) and the arms-down idle under them showed through: the arms sagged towards
    // it and came back, a kick and a second movement at the 10mm pistol's draw's end (the
    // trace `movement-inertia-trace-weapons` and its layers' file; the owner, 2026-11-15,
    // "pistol drawing animation is not smooth at all, it hitches multiple times").
    // After the draw, its clip leaving keeps covering what the hold has not yet taken too
    // (dropping its last 2 % in one frame was a second kick).
    if first_inertia {
        let over = f.equip_at.map(|at| handover_share(equip_curve_inv(at)));
        // The aim clips together (the hold, or the hold crossing over to the sights or a block:
        // their shares then add up to about the whole, so an aim taken up right after a draw
        // does not bring the draw clip back).
        let aims: Vec<f32> = a.layers.iter().filter(|l| is_aim(l.tag)).map(|l| l.shown_ease()).collect();
        let hold = (!aims.is_empty()).then(|| aims.iter().sum::<f32>().min(1.0));
        for l in a.layers.iter_mut().filter(|l| l.tag == TAG_EQUIP) {
            let a_draw = draw.as_ref().is_some_and(|c| Arc::ptr_eq(&c.clip, &l.clip));
            let leaving = l.is_leaving();
            let own = match over {
                Some(o) if !leaving => 1.0 - o,
                _ => l.ease,
            };
            let keep = match hold {
                // Drawing: its own hand-over, or what the hold has not yet taken.
                Some(h) if a_draw && !leaving => own.max(1.0 - h),
                // Drawn and leaving (it does not fade by itself then): what the hold has not
                // yet taken, never more than it had; gone once the hold is whole.
                Some(h) if a_draw => own.min(1.0 - h),
                _ => own,
            };
            (l.ease, l.smooth) = (keep.clamp(0.0, 1.0), false);
        }
    }
    // A throw held at its release waits at the wound-up clip's `Hit` key.
    if let (true, Some(clip), Some(hit)) = (f.throw_held, rig.wound_up.as_ref(), rig.wound_hit) {
        for l in a.layers.iter_mut().filter(|l| (TAG_ATTACK..TAG_ATTACK + 3).contains(&l.tag) && Arc::ptr_eq(&l.clip, clip)) {
            l.time = l.time.min(hit);
        }
    }
    rig.carry = carried.and_then(|(c, from)| Some(c.root_travel(from, a.layers.iter().find(|l| l.tag == TAG_ATTACK && Arc::ptr_eq(&l.clip, &c))?.time)));
    // The attack clip passing its `Eject` key: the casing leaves now (combat throws it).
    rig.ejected = match (before, attack_time(a), &rig.attack_eject) {
        (Some(from), Some(to), Some((_, key))) => from < *key && to >= *key,
        _ => false,
    };
    // While the shots keep coming, an attack clip with a repeated part goes round it;
    // after the last it plays out from where it is.
    rig.since_attack += f.dt;
    if let Some((from, to)) = rig.attack_loop.filter(|_| rig.since_attack <= LOOP_HOLD) {
        for l in a.layers.iter_mut().filter(|l| (TAG_ATTACK..TAG_ATTACK + 3).contains(&l.tag)) {
            l.time = loop_time(l.time, from, to);
        }
    }
    if let Some((_, t)) = pipboy {
        ease_pipboy_layer(a, t);
    }
    a.pose();
    // A Land Mine weapon sits turned in the first-person hand: the clips leave the frag
    // mine on its edge, underside to the view, where the game shows its top tilted back
    // with the handle in the hand [observed: the game's first-person frames (owner
    // 2026-10-23); our pose of `1lmaim.kf` matches NifSkope's; the turn and offset are
    // fitted to those frames, the game's own rule is unknown; animation.md §9.11].
    if rig.view == View::First && f.prefix == "1lm" {
        if let Some(w) = rig.weapon_node {
            let (turn, offset) = land_mine_grip();
            a.locals[w].rotation = turn * a.locals[w].rotation;
            a.locals[w].translation = turn * a.locals[w].translation + offset;
        }
    }
    let turned = (rig.view == View::Third && f.aiming && f.hands.in_hand && !blocks(f.prefix)).then(|| bone("Bip01 Spine").map_or(0.0, |s| keep_sights_facing(a, s)));
    match (turned, rig.sights_turned) {
        (Some(t), m) => rig.sights_turned = Some(m.unwrap_or(0.0).max(t)),
        (None, Some(m)) => {
            info!("player sights: upper body turned back to the sights clip's facing by up to {m:.1}° while down the sights");
            rig.sights_turned = None;
        }
        (None, None) => {}
    }
    // Each time the weapon comes to rest in the hand, the parts it holds away from their
    // place in the model (the 9mm pistol's cocked hammer; smoke `weapon-parts-rest`).
    let busy = |t: u32| t == TAG_EQUIP || t == TAG_RELOAD || (TAG_ATTACK..TAG_ATTACK + 3).contains(&t);
    let idle = f.hands.in_hand && rig.weapon_model.is_some() && !a.layers.iter().any(|l| busy(l.tag) && l.ease > 0.0);
    if idle && !rig.idle_logged {
        let parts: Vec<String> = a.extra_moved().filter(|p| p.1 > 0.01 || p.2 > 0.1).map(|(n, d, turn)| format!("{n} {d:.1} units {turn:.0}°")).collect();
        info!("player {:?} rig: weapon at rest in the hand, parts away from their place: {}", rig.view, if parts.is_empty() { "none".to_string() } else { parts.join(", ") });
    }
    rig.idle_logged = idle;
    if let Some(l) = rig.looking {
        // The arms rig turns about `Bip01 Looking` with the view's pitch (about the
        // actor's right axis, x) [hypothesis: the node's role, from its name and place].
        let bind = a.skel.locals[l];
        a.locals[l].rotation = Quat::from_rotation_x(look_pitch) * bind.rotation;
    }
    if let (Some((share, view)), Some(t)) = (sights.filter(|(s, _)| *s > 0.0), rig.translate) {
        rig.line_up_sights(t, share, view, (parents, &*transforms));
    }
    let a = &mut rig.animator;
    if let Some(t) = rig.translate.filter(|_| sway != SwayPose::NONE) {
        // The sway turns the arms about the eye (`Bip01 Looking`'s origin, which
        // `Bip01 Translate` hangs from) and moves them in the view's frame; the camera
        // node beside them stays put.
        let posed = a.locals[t];
        a.locals[t].rotation = sway.rotation * posed.rotation;
        a.locals[t].translation = sway.rotation * posed.translation + sway.offset;
    }
    a.apply(transforms);
    if let Ok(mut t) = transforms.get_mut(rig.root) {
        *t = root;
    }
    rig.posed_root = Some(root);
    let root_m = Affine3A::from_scale_rotation_translation(root.scale, root.rotation, root.translation);
    match rig.view {
        View::First => {
            let c = rig.camera?;
            // With the nodes a script put above the camera: their move and turn too.
            let offset = script_offset(a, c, parents, transforms);
            let m = root_m * offset.unwrap_or(Affine3A::IDENTITY) * a.model_space(c);
            rig.script_turn = offset.map(|o| root.rotation * o.to_scale_rotation_translation().1 * root.rotation.inverse());
            rig.script_used = offset.map(|o| (o, root_m, a.model_space(c)));
            Some((m.translation.into(), view_driven.then(|| m.to_scale_rotation_translation().1)))
        }
        View::Third => rig.head.map(|h| ((root_m * a.model_space(h)).transform_point3(rig.eye_local), None)),
    }
}

/// Down the sights in third person the body stays square to the view, whatever way the
/// legs step. The sights clips (`2hraimis.kf`, `1hpaimis.kf`) take the spine, arms and
/// head (priority 50) but leave `Bip01 NonAccum`, the body's node that the pelvis and
/// `Bip01 Spine` both hang from, to the movement clips (26 under 30). The unarmed
/// `male\mtleft.kf` and `mtfastleft.kf`, which a rifle (no `2hrleft.kf`) strafes left
/// with, turn that node a further 90° (yaw 177.5° against 87.5–90° in every other move
/// and in the weapons' own side-steps), so strafing left swung the body and the rifle a
/// quarter turn off the crosshair [observed: `kf_pose_compare` and `fnvtool actor
/// locomotion --samples`, animation.md §9.6; the owner's playtest 2026-10-23].
///
/// The aim clips' own rotation of the nodes above the spine is taken in place of the
/// blend's, faded with the aim clips; the movement clip keeps their translation (the
/// bob) and the legs below the pelvis. In the original, strafing left down the sights
/// the hips stay square to the camera and the legs step as in a backpedal [observed:
/// the owner's capture 2026-10-23], which is that [hypothesis: the game's rule is not
/// known]. Turning only the spine instead wrung the skin at the waist a quarter turn.
/// Standing still the aim clip holds those nodes itself and nothing changes. Returns
/// the largest turn, degrees.
fn keep_sights_facing(a: &mut Animator, spine: usize) -> f32 {
    let aim = |l: &nvf_assets::anim_blend::Layer| (TAG_AIM..TAG_AIM + 3).contains(&l.tag);
    let ease = a.layers.iter().filter(|l| aim(l)).map(|l| l.ease).fold(0.0f32, f32::max).min(1.0);
    let mut most = 0.0f32;
    let mut cur = a.skel.parents.get(spine).copied().flatten();
    while let Some(n) = cur.filter(|&n| n > 0 && ease > 0.0) {
        if let Some(own) = a.local_from(n, aim) {
            let posed = a.locals[n].rotation;
            let r = posed.slerp(own.rotation, ease).normalize();
            most = most.max(posed.angle_between(r).to_degrees());
            a.locals[n].rotation = r;
        }
        cur = a.skel.parents[n];
    }
    most
}

/// Draw the first-person rig `k` times as big across the view about the eye (along the
/// view it keeps its depth): what a view of a narrower field of view gives, without a
/// second camera [design]. The rig's root keeps the place [`pose_rig`] gave it, under
/// the lens nodes: the outer node stands at the eye looking along `forward`, the inner
/// one scales across that look.
fn place_lens(rig: &Rig, eye: Vec3, forward: Vec3, k: f32, transforms: &mut Query<&mut Transform>) {
    let Some([outer, inner]) = rig.lens else { return };
    let Ok(root) = transforms.get(rig.root).copied() else { return };
    let at = Transform { translation: eye, rotation: Quat::from_rotation_arc(Vec3::Y, forward.normalize_or(Vec3::Y)), scale: Vec3::ONE };
    let local = Transform::from_matrix(at.to_matrix().inverse() * root.to_matrix());
    for (e, t) in [(outer, at), (inner, Transform::from_scale(Vec3::new(k, 1.0, k))), (rig.root, local)] {
        if let Ok(mut m) = transforms.get_mut(e) {
            *m = t;
        }
    }
}

/// Pose the third-person rig with a furniture clip alone and place it at `root`
/// (`crate::furniture`); returns the eye. The clips of one use play in different frames,
/// so a new clip replaces the last at once instead of fading in.
fn pose_furniture(rig: &mut Rig, o: &crate::furniture::BodyClip, special: &IdleSlot<Arc<Clip>>, dt: f32, root: Transform, transforms: &mut Query<&mut Transform>) -> Option<Vec3> {
    let a = &mut rig.animator;
    let mode = if o.looped { Mode::Loop } else { Mode::Once };
    if rig.furniture != Some(o.serial) {
        a.layers.retain(|l| l.tag >= TAG_SPECIAL);
        a.restart(&o.clip, TAG_FURNITURE, 1.0, mode, 0.0);
        rig.furniture = Some(o.serial);
    }
    // The holstered weapon keeps its place on the holster bone: the seat clips have no
    // `Weapon` track (`dynamicidle_chairsit.kf`), and the holster clip, which does, also
    // keys `Bip01 Spine2` and `Bip01 NonAccum`, so only its `Weapon` place is kept. At the
    // node's bind transform (its place under the right hand) a rifle on the back stood up
    // through the seated body's head [observed in NVForever: player report R-DNBYBM6D,
    // #3612; NPCs keep the stance's `Weapon` track on furniture, `weapons.rs`].
    if let Some(n) = rig.weapon_node {
        a.set_rest(n, rig.holstered_weapon);
        if rig.furniture_logged != Some(o.serial) {
            rig.furniture_logged = Some(o.serial);
            let on = rig.weapon_parent.and_then(|b| a.skel.names.get(b).cloned().flatten()).unwrap_or_default();
            let place = if rig.holstered_weapon.is_some() { "at the holster clip's place" } else { "at its bind transform" };
            info!("player body: on furniture, the weapon hangs from {on} {place}");
        }
    }
    a.begin();
    a.want(&o.clip, TAG_FURNITURE, 1.0, mode, 0.0);
    // Over it the idle the body plays there (a fidget on furniture, `idles.rs`), as
    // `pose_rig` lays them.
    for (p, w) in special.shown() {
        let newest = special.cur.as_ref().is_some_and(|c| c.serial == p.serial);
        let i = a.want(&p.clip, TAG_SPECIAL + p.serial % 1000, if newest { 1.0 } else { w }, Mode::Once, p.play.keys.blend_in);
        let l = &mut a.layers[i];
        (l.rate, l.time) = (0.0, p.play.time());
        if !newest {
            l.weight = w;
        }
    }
    a.end();
    a.advance(dt, 1.0);
    a.pose();
    a.apply(transforms);
    if let Ok(mut t) = transforms.get_mut(rig.root) {
        *t = root;
    }
    let root_m = Affine3A::from_scale_rotation_translation(root.scale, root.rotation, root.translation);
    rig.head.map(|h| (root_m * a.model_space(h)).transform_point3(rig.eye_local))
}

/// The view's heading on furniture with the body in first person: within [`NECK_TURN`] of
/// `facing` (the body's heading) it is the player's; the mouse cannot take it further
/// once it has been there (`settled`); a view further round when the body sits down
/// comes to the nearer limit at [`NECK_RETURN_RATE`]. Also whether it is within reach.
fn seated_heading(heading: f32, facing: f32, settled: &mut bool, dt: f32) -> (f32, bool) {
    use std::f32::consts::{PI, TAU};
    let turned = (heading - facing + PI).rem_euclid(TAU) - PI;
    let wide = nvf_play::tweaks::SEATED_WIDE.load(std::sync::atomic::Ordering::Relaxed);
    let over = turned.abs() - if wide { NECK_TURN_WIDE } else { NECK_TURN };
    if over <= 0.0 {
        *settled = true;
        return (heading, true);
    }
    let back = if *settled { over } else { over.min(NECK_RETURN_RATE * dt) };
    let within = back >= over;
    *settled |= within;
    ((facing + turned.signum() * (turned.abs() - back)).rem_euclid(TAU), within)
}

/// The compass heading bone `b` of the posed rig faces when the rig's root faces
/// `heading`: the root's, turned by the bone's turn about the vertical since the bind
/// pose (the clips of getting on furniture turn the body inside the rig).
fn bone_heading(rig: &Rig, b: usize, heading: f32) -> f32 {
    let a = &rig.animator;
    let forward = a.model_space(b).matrix3 * a.skel.bind_world[b].matrix3.inverse() * Vec3A::Y;
    heading + forward.x.atan2(forward.y)
}

/// How bone `b` of the posed rig is turned in game space since the bind pose, the rig's
/// root facing `heading`: it takes +X to the bone's right, +Y to the way it faces and +Z
/// to its up (the bind pose stands upright facing the rig's +Y).
fn bone_turn(rig: &Rig, b: usize, heading: f32) -> Quat {
    let a = &rig.animator;
    let turned = Mat3::from(a.model_space(b).matrix3 * a.skel.bind_world[b].matrix3.inverse());
    (Quat::from_rotation_z(-heading) * Quat::from_mat3(&turned)).normalize()
}

/// The way a view looks and the top of its picture, for a turn clockwise from +Y towards
/// +X and a tilt up from the XY plane: in game space for a compass heading and a pitch,
/// in a body's own frame ([`bone_turn`]) for its head's turn and tilt.
fn view_axes(turn: f32, tilt: f32) -> (Vec3, Vec3) {
    let (st, ct) = turn.sin_cos();
    let (sp, cp) = tilt.sin_cos();
    (Vec3::new(st * cp, ct * cp, sp), Vec3::new(-st * sp, -ct * sp, cp))
}

/// The compass heading and pitch of the level view nearest a view that looks along
/// `forward` with `up` the top of its picture (game space). Looking straight up or down
/// the top of the picture says which way the view is turned; `heading` is kept when
/// neither does.
fn view_angles(forward: Vec3, up: Vec3, heading: f32) -> (f32, f32) {
    let pitch = forward.z.clamp(-1.0, 1.0).asin().clamp(-MAX_PITCH, MAX_PITCH);
    let (sp, cp) = pitch.sin_cos();
    let flat = forward.truncate() * cp - up.truncate() * sp;
    let heading = if flat.length_squared() > 1e-6 { flat.x.atan2(flat.y).rem_euclid(std::f32::consts::TAU) } else { heading };
    (heading, pitch)
}

/// The first-person view of a body lying on a bed, in the body's own frame: the head's
/// turn to the body's right and its tilt towards the top of the body's head, radians.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LyingView {
    turn: f32,
    tilt: f32,
    /// The heading and pitch given to the player for it.
    wrote: (f32, f32),
}

/// One frame of the view of a body lying on a bed (or lying down on it, or getting up)
/// [engineering choice throughout; the original's player never lies down]:
///
/// - The view keeps its place in the body's frame (`frame`, [`bone_turn`] of the pelvis)
///   from frame to frame, so it lies back with the body and the picture leans with it:
///   on its back the body looks at the ceiling, the top of the picture towards the top
///   of its head; with the head turned on the pillow the room is on its side [owner
///   request 2026-10-10: the camera sideways when lying].
/// - The mouse (`look`: the heading and pitch asked for since the last frame) turns and
///   tilts the head in that frame. A heading and pitch set by anything else (the first
///   frame, a test aid) are taken as they are, in game space.
/// - The head turns as far as a seated one ([`NECK_TURN`], [`NECK_DOWN`];
///   [`seated_heading`] for a view that starts further round): beyond that the view is
///   in the body.
/// - The further the body has lain back, the less the view looks below level, down to
///   [`LYING_DOWN`]: the head is on the pillow and below it is the mattress. A body on
///   its side turns its head up to the ceiling but not down into the pillow.
///
/// Returns the view, the way it looks and the top of its picture (game space), and
/// whether it is within the neck's reach.
fn lying_view(last: Option<LyingView>, frame: Quat, heading: f32, pitch: f32, look: [f32; 2], settled: &mut bool, dt: f32) -> (LyingView, [Vec3; 2], bool) {
    use std::f32::consts::{PI, TAU};
    let near = |a: f32, b: f32| ((a - b + PI).rem_euclid(TAU) - PI).abs() < 1e-3;
    let (turn, tilt) = match last {
        Some(v) if near(heading, v.wrote.0 + look[0]) && near(pitch, (v.wrote.1 + look[1]).clamp(-MAX_PITCH, MAX_PITCH)) => (v.turn + look[0], v.tilt + look[1]),
        _ => {
            let d = frame.inverse() * view_axes(heading, pitch).0;
            (d.x.atan2(d.y), d.z.clamp(-1.0, 1.0).asin())
        }
    };
    let (turn, within) = seated_heading(turn, 0.0, settled, dt);
    let mut turn = (turn + PI).rem_euclid(TAU) - PI;
    let mut tilt = tilt.clamp(-NECK_DOWN, MAX_PITCH);
    let lain = (1.0 - (frame * Vec3::Z).z).clamp(0.0, 1.0);
    let lowest = -MAX_PITCH + (MAX_PITCH - LYING_DOWN) * lain;
    let ahead = frame * view_axes(turn, tilt).0;
    if ahead.z < lowest.sin() {
        // Up to the lowest it may look, the same way round.
        let d = frame.inverse() * (ahead.truncate().try_normalize().unwrap_or(Vec2::Y) * lowest.cos()).extend(lowest.sin());
        (turn, tilt) = (d.x.atan2(d.y), d.z.clamp(-1.0, 1.0).asin());
    }
    let (forward, up) = view_axes(turn, tilt);
    let (forward, up) = (frame * forward, frame * up);
    (LyingView { turn, tilt, wrote: view_angles(forward, up, heading) }, [forward, up], within)
}

/// What of the third-person rig the camera sees. All of it casts the shadow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    /// Any of it.
    body: bool,
    /// Its head, its arms and hands, the weapon on its `Weapon` node.
    head: bool,
    arms: bool,
    weapon: bool,
}

/// [design: show_rigs] The independently hidden parts of the third-person body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EffectPart {
    Body,
    Head,
    Arms,
}

/// [design: build_rig] Descendants follow their head or arm branch. Unclassified
/// bones follow the body; the names select branches, not individual finger bones.
fn effect_bone_part(skel: &PreparedSkeleton, mut bone: usize) -> EffectPart {
    for _ in 0..skel.parents.len() {
        match skel.names.get(bone).and_then(|n| n.as_deref()) {
            Some("Bip01 Head") => return EffectPart::Head,
            Some("Bip01 L UpperArm" | "Bip01 R UpperArm") => return EffectPart::Arms,
            _ => {}
        }
        let Some(parent) = skel.parents.get(bone).copied().flatten() else { break };
        bone = parent;
    }
    EffectPart::Body
}

impl Seen {
    fn effect_part(self, part: EffectPart) -> bool {
        self.body && match part {
            EffectPart::Body => true,
            EffectPart::Head => self.head,
            EffectPart::Arms => self.arms,
        }
    }

    /// Seen with the body in first person: the arms rig's arms stand in for the body's
    /// own while they are up, with the weapon out (in hand, drawing or putting away) or
    /// the Pip-Boy raised, so the body's arms and hands are hidden then and one pair of
    /// hands is drawn (#1523) [hypothesis, from reading this file: the Pip-Boy clip plays
    /// on the arms rig only, and the body's hands were still seen beside it]. The body's
    /// weapon is hidden while the arms rig holds it.
    fn upright(in_hand: bool, weapon_moving: bool, pipboy_up: bool) -> Self {
        let arms_up = in_hand || weapon_moving || pipboy_up;
        Seen { body: true, head: false, arms: !arms_up, weapon: !in_hand }
    }

    /// The render layer of a drawn entity: the seen body's when the camera sees it, else
    /// the one only the sun's shadows are cast from. Neither is the world's, so the
    /// flashlight casts no shadow from the player (`flashlight.rs`).
    fn layer(on: bool) -> RenderLayers {
        RenderLayers::layer(if on { BODY_LAYER } else { SHADOW_LAYER })
    }

    fn part(self, d: &Drawn) -> bool {
        self.body && (self.head || !d.head) && (self.arms || !d.arm)
    }

    /// A shape of the weapon model: the ones on a bone of their own (the minigun's
    /// backpack) are seen with the body.
    fn weapon_part(self, p: &WeaponPart) -> bool {
        self.body && (self.weapon || p.on_bone)
    }
}

/// Show the camera `seen` of the third-person rig, and the first-person rig or not.
///
/// `hidden` (photo mode's Hide Player): neither is seen, whatever the view.
fn show_rigs(body: &mut Body, commands: &mut Commands, visibility: &mut Query<&mut Visibility>, seen: Seen, first: bool, hidden: bool) {
    let (seen, first) = if hidden { (Seen { body: false, head: false, arms: false, weapon: false }, false) } else { (seen, first) };
    let third = &mut body.third;
    if third.seen != Some(seen) {
        for d in &third.drawn {
            commands.entity(d.entity).insert(Seen::layer(seen.part(d)));
        }
        for p in &third.weapon_parts {
            commands.entity(p.entity).insert(Seen::layer(seen.weapon_part(p)));
        }
        third.seen = Some(seen);
        debug!("player body: the camera sees {seen:?} of the body");
    }
    if let Some(f) = body.first.as_mut() {
        let want = if first { Visibility::Inherited } else { Visibility::Hidden };
        if let Ok(mut v) = visibility.get_mut(f.root) {
            if *v != want {
                *v = want;
            }
        }
        if f.shown != Some(first) {
            debug!("player body: first-person rig {}", if first { "shown" } else { "hidden" });
            f.shown = Some(first);
        }
    }
}

/// Switch the held weapon model's particle systems on and off as the seen rig's clips
/// say (`Animator::emitter`): the dynamite's `##PCloud01` sparks at the fuse from the
/// throw clip's lighting to its release (docs/spec-notes/particles.md §7.6). A system
/// the clips hide (`NiVisController`) does not emit, nor does the rig the camera does not
/// see: the third-person body stands behind the arms in first person, drawn for its
/// shadow only.
/// kNVSE's SetActorAnimationPath and AnimGroupOverride on the player
/// (docs/design/knvse-anim.md): each frame the game says which replacement files apply
/// now, conditions run (`Game::anim_overrides_now`); when that changes, each rig takes
/// the new files and forgets the clips it chose, so the next pick finds them.
fn apply_anim_overrides(mut script: Option<ResMut<nvf_play::script::ScriptState>>, sim: Option<ResMut<nvf_play::sim::SimState>>, mut state: ResMut<PlayerBody>, mut current: Local<[(Vec<String>, Overrides); 2]>) {
    let (Some(rt), Some(mut sim)) = (script.as_deref_mut().and_then(|s| s.rt.as_mut()), sim) else { return };
    let forced = rt.game.take_player_anim_refresh() || std::mem::take(&mut state.force_anim_refresh);
    let empty = rt.game.anim_paths.actor.is_empty() && rt.game.anim_paths.weapon.is_empty() && rt.game.anim_folders.active.is_empty() && current.iter().all(|c| c.0.is_empty());
    if empty && !forced {
        return;
    }
    let now = nvf_play::script::with_world(rt, &mut sim, |rt| {
        if forced {
            rt.game.refresh_anim_folders_for(FormId::PLAYER_REF);
        }
        [rt.game.anim_overrides_now(FormId::PLAYER_REF, false), rt.game.anim_overrides_now(FormId::PLAYER_REF, true)]
    });
    for (i, paths) in now.into_iter().enumerate() {
        if forced || current[i].0 != paths {
            // By file name, in stack order (a later file wins, `Rig::clip`).
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            for p in &paths {
                map.entry(p.rsplit('\\').next().unwrap_or(p).to_ascii_lowercase()).or_default().push(p.clone());
            }
            info!("player {} rig: {} kNVSE replacement file(s){}", if i == 1 { "First" } else { "Third" }, paths.len(), if paths.is_empty() { String::new() } else { format!(": {}", paths.join(", ")) });
            current[i] = (paths, Arc::new(map));
        }
    }
    let Some(body) = state.body.as_mut() else { return };
    let rigs = [Some(&mut body.third), body.first.as_mut()];
    for (i, rig) in rigs.into_iter().enumerate() {
        let Some(rig) = rig else { continue };
        if forced || !Arc::ptr_eq(&rig.overrides, &current[i].1) {
            rig.overrides = current[i].1.clone();
            rig.clips.clear();
            rig.variants.clear();
        }
    }
}

fn drive_weapon_emitters(state: Res<PlayerBody>, mut emitters: Query<&mut crate::particles::ModelEmitter>) {
    let Some(body) = state.body.as_ref() else { return };
    let first_seen = body.first.as_ref().is_some_and(|f| f.shown == Some(true));
    let third_seen = body.third.seen.is_some_and(|s| s.body && s.weapon);
    for (rig, seen) in std::iter::once((&body.third, third_seen)).chain(body.first.as_ref().map(|f| (f, first_seen))) {
        let Some((holder, names)) = &rig.weapon_emitter else { continue };
        let Ok(mut em) = emitters.get_mut(*holder) else { continue };
        let mut on: HashMap<String, Option<f32>> = HashMap::new();
        for name in names.iter().filter(|_| seen) {
            if rig.animator.visible(name) == Some(false) {
                continue;
            }
            if let Some((true, rate)) = rig.animator.emitter(name) {
                on.insert(name.clone(), rate);
            }
        }
        let was = em.driven.as_ref().map_or(0, HashMap::len);
        if was != on.len() {
            // Smoke tests read it (`thrown-dynamite`): the fuse lit in the hand.
            info!("player {:?} rig weapon particles: {}", rig.view, if on.is_empty() { "off".to_string() } else { format!("{:?} on", on.keys().collect::<Vec<_>>()) });
        }
        if em.driven.as_ref() != Some(&on) {
            em.driven = Some(on);
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn drive_body(
    mut commands: Commands,
    (time, play_clock, paused, turbo, sim, mut told_scale, sequences, mut trace, stance_motion): (Res<Time>, Option<Res<nvf_play::pause::PlayClock>>, Option<Res<nvf_play::pause::Paused>>, Option<Res<nvf_play::script::PlayerTimeScale>>, Option<Res<nvf_play::sim::SimState>>, Local<Option<f32>>, Res<crate::script_sequences::PlayerSequences>, Local<MotionTrace>, Option<Res<nvf_play::stance::StanceMotion>>),
    d: Res<PlayWorld>,
    intent: Res<MoveIntent>,
    control: Res<PlayerControl>,
    mut phys: ResMut<PhysState>,
    mut player: ResMut<Player>,
    mut view: ResMut<BodyView>,
    mut state: ResMut<PlayerBody>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut anim: ResMut<WeaponAnim>,
    (furniture, mut idles, mut seat, pip, cams, first_fov, limbs, (death, knock), (zoom, combat, mut true_sights, scope, mut lens3d, lens_sway), (ahead, mut lunge, s1st, vats, inertia, mut blows, photo)): (
        Res<crate::furniture::BodyOverride>,
        ResMut<PlayerIdles>,
        ResMut<nvf_play::furniture::PlayerSeat>,
        Res<nvf_play::pipboy::PipBoy>,
        Query<&Projection, With<nvf_play::PlayerCamera>>,
        (Option<Res<nvf_play::free_cam::FirstPersonFov>>, Option<Res<crate::Args>>, Option<Res<nvf_play::pipboy::touch_presentation::PipBoySize>>),
        Option<Res<nvf_play::combat::PlayerLimbs>>,
        (Res<crate::death_cam::DeathCam>, Res<crate::player_knockdown::PlayerKnockdown>),
        (Option<Res<nvf_play::aim_zoom::AimZoom>>, Option<Res<nvf_play::combat::Combat>>, Option<ResMut<nvf_play::aim_zoom::TrueSights>>, Option<Res<nvf_play::aim_zoom::Scope>>, Option<ResMut<nvf_play::aim_zoom::Scope3d>>, Option<Res<nvf_play::aim_zoom::LensSway>>),
        (Res<crate::weapon_lower::Ahead>, ResMut<nvf_play::player::Lunge>, Option<Res<nvf_play::console::ShowFirstPerson>>, Option<Res<nvf_play::vats_mode::VatsMode>>, Option<Res<nvf_play::inertia::MovementInertia>>, Option<ResMut<PlayerBlows>>, Option<Res<nvf_play::photo_mode::PhotoMode>>),
    ),
    (mut driven, parents, mut toddler): (ResMut<nvf_play::player::ViewDriven>, Query<&ChildOf>, Option<ResMut<nvf_play::player::PlayerToddler>>),
) {
    let state = &mut *state;
    // Photo mode's Hide Player: no rig is drawn (the shadow stays).
    let hidden = photo.as_deref().is_some_and(|p| p.hides_player());
    player.body_view = None;
    // Set again below while the body's attack clip carries the player.
    let was_carrying = std::mem::replace(&mut lunge.carrying, false);
    // Set again below when the sights come up; not on the frames that leave early.
    if let Some(t) = true_sights.as_deref_mut() {
        t.0 = false;
    }
    anim.drawing = false;
    anim.holstering = false;
    state.head_eye = None;
    let own_body = state.first_person_body;
    let seat_look = std::mem::take(&mut seat.look);
    let Some(body) = state.body.as_mut() else {
        player.body_eye = None;
        view.pivot = None;
        // No body to time the weapon: it is ready as soon as it is wanted out.
        anim.ready = control.weapon_out;
        anim.muzzle = None;
        anim.casing = None;
        state.muzzle_entity = None;
        state.casing_entity = None;
        state.weapon_aim = None;
        return;
    };
    if death.down() || knock.ragdolled() {
        // Dead or knocked down: the clips stop and the ragdoll has the body
        // (`death_cam.rs`, `player_knockdown.rs`). The whole body is drawn, but for its
        // head while the view is inside it (a knockdown's view is outside).
        anim.ready = false;
        anim.muzzle = None;
        anim.casing = None;
        state.muzzle_entity = None;
        state.casing_entity = None;
        state.weapon_aim = None;
        show_rigs(body, &mut commands, &mut visibility, Seen { body: true, head: death.third_person() || knock.ragdolled(), arms: true, weapon: true }, false, hidden);
        return;
    }
    // Turbo slows the world's clock, not the player's clips (magic-archetypes.md §2).
    let dt = time.delta_secs() * turbo.as_deref().map_or(1.0, |t| t.0);
    let m = &mut state.motion;
    let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };

    // What the character is doing.
    let (grounded, swimming, ground_gap) = phys.player.as_ref().map_or((true, false, None), |c| (c.grounded, c.swimming, c.ground_gap));
    // The body's vertical velocity in the air, for the weapon's jump and landing.
    let vertical = phys.player.as_ref().filter(|c| !c.grounded && !c.swimming).map_or(0.0, |c| c.vertical_speed);
    let stance = if swimming {
        Stance::Swim
    } else if control.sneak {
        Stance::Sneak
    } else {
        Stance::Stand
    };
    // The movement clips: the keys', or with movement inertia the way the body glides
    // once the keys are let go or brakes against them (`nvf_play::inertia`).
    let (forward, right, fast) = if d.auto_walk > 0.0 {
        (1.0, 0.0, true)
    } else {
        let (f, r, walk) = inertia.as_deref().map_or((intent.forward, intent.right, intent.walk), |i| i.clip_intent(intent.forward, intent.right, intent.walk, player.heading));
        // A sprint from a walk (Always Run off) runs: the run clips from half a sprint on
        // (`nvf_play::sprint`).
        let sprint_runs = stance_motion.as_deref().is_some_and(|s| s.sprint_share >= nvf_play::sprint::RUN_GAIT_FROM);
        (f, r, !walk || sprint_runs)
    };
    let feet = player.feet;
    if dt > 0.0 {
        let step = m.last_feet.map_or(Vec2::ZERO, |l| Vec2::new(feet[0] - l[0], feet[1] - l[1]) / dt);
        let moved = step.length();
        m.speed += (moved - m.speed) * (dt / 0.15).min(1.0);
        // In the view's frame: the heading is clockwise from +y.
        let (sin, cos) = player.heading.sin_cos();
        let local = Vec2::new(step.x * cos - step.y * sin, step.x * sin + step.y * cos);
        m.velocity += (local - m.velocity) * (dt / 0.1).min(1.0);
        if let Some(h) = m.last_heading {
            let dh = (player.heading - h + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            m.turn_rate += (dh / dt - m.turn_rate) * (dt / 0.1).min(1.0);
        }
    }
    m.last_feet = Some(feet);
    m.last_heading = Some(player.heading);
    // The body and the view are drawn lifted over a step or a bump climbed in one physics
    // step, so they climb it smoothly; the feet stay where the character is
    // (`nvf_play::phys::step_lift`, owner's playtest list 7 item 9).
    let feet = [feet[0], feet[1], feet[2] + phys.drawn_lift_of(FormId::PLAYER_REF.0)];
    let airborne_before = m.airborne;
    m.airborne = if grounded || swimming { 0.0 } else { m.airborne + dt };
    let was_falling = m.falling;
    m.falling = m.airborne > 0.0 && (m.falling || (m.airborne >= AIRBORNE_LOOP && ground_gap.is_none_or(|g| g > FALL_GAP)));
    let landing = was_falling && airborne_before >= AIRBORNE_LAND && m.airborne == 0.0 && m.fall_speed > LAND_MIN_FALL;
    if was_falling && airborne_before >= AIRBORNE_LAND && m.airborne == 0.0 && !landing {
        info!("player view: down again after {airborne_before:.2}s off the ground without a fall ({:.0} units/s): no landing", m.fall_speed);
    }
    if m.airborne > 0.0 {
        m.fall_speed = m.fall_speed.max(-vertical);
    } else {
        if landing {
            m.landed_speed = m.fall_speed;
            info!("player view: landed at {:.0} units/s", m.landed_speed);
        }
        m.fall_speed = 0.0;
    }
    m.falls += u32::from(m.falling && !was_falling);
    m.landings += u32::from(landing);
    if m.falling != was_falling {
        let gap = ground_gap.map_or("out of reach".to_string(), |g| format!("{g:.1} units"));
        debug!("player body: {} after {:.2}s in the air (ground {gap} below)", if m.falling { "falling" } else { "down again" }, airborne_before.max(m.airborne));
    }

    let script_scale = sim.as_deref().and_then(|s| s.script_scale(FormId::PLAYER_REF.0)).unwrap_or(1.0).max(nvf_play::sim::MIN_DRAWN_SCALE);
    let third_view = control.third_person_view();
    if let Some(o) = furniture.0.as_ref() {
        // On furniture: the third-person body plays the furniture's clip where the
        // furniture has it. In first person the view is that body's eye and the body is
        // not drawn (seen from inside it gets in the way of looking round) [design],
        // unless the body is wanted in first person: then all but its head is, and the
        // view turns only as far as a neck does, so it never looks into the body.
        let root = Transform { translation: Vec3::from_array(o.pos), rotation: Quat::from_rotation_z(-o.heading), scale: Vec3::splat(body.scale * script_scale) };
        let mut eye = pose_furniture(&mut body.third, o, &idles.third, dt, root, &mut transforms);
        let pelvis = body.third.animator.skel.by_name.get("Bip01 Pelvis").copied();
        // A body on a bed: the frame it lies in is its pelvis's.
        let lying = o.lying.then(|| pelvis.map_or(root.rotation, |b| bone_turn(&body.third, b, o.heading)));
        let own_body = if let (Some(frame), false) = (lying, third_view) {
            // Lying down: the view is the lying body's, drawn or not ([`lying_view`]).
            let third = &body.third;
            let (v, [forward, up], within) = lying_view(m.lying, frame, player.heading, player.pitch, seat_look, &mut m.seat_settled, dt);
            (player.heading, player.pitch) = v.wrote;
            m.lying = Some(v);
            m.seat_facing = None;
            // The camera leans with the head: its right, the way it looks and its top.
            player.body_view = Some(Quat::from_mat3(&Mat3::from_cols(forward.cross(up), forward, up)));
            // With the body drawn the eyes go round the neck with the view, about the
            // body's own length, as a head turned on a pillow: the open neck stays
            // behind the view.
            if let (true, Some(e), Some(h)) = (own_body, eye.as_mut(), third.head) {
                let root_m = Affine3A::from_scale_rotation_translation(root.scale, root.rotation, root.translation);
                let neck = Vec3::from((root_m * third.animator.model_space(h)).translation);
                let faces = frame.inverse() * (bone_turn(third, h, o.heading) * Vec3::Y);
                *e = neck + Quat::from_axis_angle(frame * Vec3::Z, faces.x.atan2(faces.y) - v.turn) * (*e - neck);
            }
            own_body && within
        } else if own_body && !third_view {
            use std::f32::consts::{PI, TAU};
            let third = &body.third;
            m.lying = None;
            // The way the body faces is its pelvis's: the clips of getting on and off
            // turn it inside the rig. The view turns with it (owner report 2026-10-10:
            // sitting down, the camera "didn't move in line with the rest of the body"),
            // and within a neck's reach of it is the player's.
            let facing = pelvis.map_or(o.heading, |b| bone_heading(third, b, o.heading)).rem_euclid(TAU);
            if let Some(last) = m.seat_facing.replace(facing) {
                player.heading = (player.heading + (facing - last + PI).rem_euclid(TAU) - PI).rem_euclid(TAU);
            }
            let (heading, within) = seated_heading(player.heading, facing, &mut m.seat_settled, dt);
            player.heading = heading;
            player.pitch = player.pitch.max(-NECK_DOWN);
            // The eyes go round the neck with the view, as a turned head's do: the open
            // neck stays behind the view whichever way it looks.
            if let (Some(e), Some(h)) = (eye.as_mut(), third.head) {
                let root_m = Affine3A::from_scale_rotation_translation(root.scale, root.rotation, root.translation);
                let neck = Vec3::from((root_m * third.animator.model_space(h)).translation);
                *e = neck + Quat::from_rotation_z(bone_heading(third, h, o.heading) - heading) * (*e - neck);
            }
            within
        } else {
            m.seat_settled = false;
            m.seat_facing = None;
            m.lying = None;
            false
        };
        player.body_eye = eye.map(|e| e.to_array());
        state.head_eye = eye.filter(|_| third_view).map(|e| e.to_array());
        // Third person: the pivot is above the body; the further the body has lain
        // back, the higher, clear of the bed.
        let lift = lying.map_or(0.0, |frame| LYING_PIVOT_UP * (1.0 - (frame * Vec3::Z).z).clamp(0.0, 1.0));
        view.pivot = eye.map(|e| [o.pos[0], o.pos[1], e.z + 5.0 + lift]);
        m.head.pivot_reset();
        anim.ready = false;
        anim.muzzle = None;
        anim.casing = None;
        state.muzzle_entity = None;
        state.casing_entity = None;
        state.weapon_aim = None;
        show_rigs(body, &mut commands, &mut visibility, Seen { body: (third_view || own_body) && !state.out_of_view, head: third_view, arms: true, weapon: true }, false, hidden);
        return;
    }
    if body.third.furniture.take().is_some() {
        body.third.animator.cut(TAG_FURNITURE);
    }
    m.seat_settled = false;
    m.seat_facing = None;
    m.lying = None;
    // The weapon: draw and holster, timed by the clips of the rig in view.
    let prefix = m.prefix;
    let timing_rig = match body.first.as_mut() {
        Some(f) if !third_view => f,
        _ => &mut body.third,
    };
    let (draw_t, away_t) = timing_rig.transitions(&mut ctx, stance, prefix);
    // The Equip Speed perks (Quick Draw): the draw and holster take 1 ÷ the rate as long;
    // times the player's Agility (the original's), or with Weapon Handling the weapon's
    // weight and the attributes by its class (`nvf_play::inertia::MovementInertia::equip_rate`).
    let equip_pace = inertia.as_deref().map_or(1.0, |i| i.equip_rate());
    let eq = if anim.equip_rate > 0.0 { anim.equip_rate } else { 1.0 } * equip_pace;
    // With movement inertia the clip is played along its curve (`equip_curve`, `Frame::equip_at`):
    // the weapon changes hands when the curve reaches the clip's own moment for it.
    let warped = inertia.as_deref().is_some_and(|i| i.smooth_hands());
    // Played by its motion (`ClipMotion`): the moment is where the curve reaches the motion
    // done by then.
    let (draw_c, away_c) = (timing_rig.clip(&mut ctx, ClipKey::Equip(stance, prefix, true)), timing_rig.clip(&mut ctx, ClipKey::Equip(stance, prefix, false)));
    let skel = timing_rig.animator.skel.clone();
    let quick = |t: fnv_actor::player_clips::Transition, c: Option<&Arc<ClipInfo>>| {
        let length = t.length / eq;
        let moved = |x: f32| c.map_or(x, |c| c.motion(&skel).done_at(x));
        let switch_at = if warped && t.length > 0.0 { length * equip_curve_inv(moved(t.switch_at / t.length)) } else { t.switch_at / eq };
        fnv_actor::player_clips::Transition { length, switch_at }
    };
    let (draw_t, away_t) = (quick(draw_t, draw_c.as_ref()), quick(away_t, away_c.as_ref()));
    let want_out = control.weapon_out && stance != Stance::Swim;
    let mut events = std::mem::take(&mut m.events);
    m.hands.update(want_out, dt, draw_t, away_t, &mut events);
    let equip_at = match m.hands.phase {
        WeaponPhase::Drawing if warped && draw_t.length > 0.0 => Some(equip_curve(m.hands.time / draw_t.length)),
        WeaponPhase::Holstering if warped && away_t.length > 0.0 => Some(equip_curve(m.hands.time / away_t.length)),
        _ => None,
    };
    // The first-person view's way into a crouch and out of one (`Frame::stance_fade`): a
    // timer over the cross-over's time, for the clips' fades while it runs and for the log.
    let crouching = stance == Stance::Sneak;
    let crouch_seconds = inertia.as_deref().filter(|i| i.eased_crouch() && stance != Stance::Swim).map(|i| i.crouch_seconds(crouching));
    let stance_fade = match crouch_seconds {
        Some(t) => {
            let goal = f32::from(u8::from(crouching));
            let was = m.crouch.value;
            m.crouch.retarget(goal, t, 1.0);
            m.crouch.step(dt);
            if m.crouch.value == goal && was != goal {
                info!("player view: {} (eased crouch)", if crouching { "crouched" } else { "standing" });
            }
            (m.crouch.value != goal || was != goal).then_some(t)
        }
        None => {
            m.crouch = nvf_play::ease::EaseOut::at(f32::from(u8::from(crouching)));
            None
        }
    };

    // Attacks and reloads (combat): the clips of the weapon, their timing in the rig in
    // view, and whether combat started one since the last frame.
    // A melee swing plays the clip combat chose (a combo's next group, a power attack's
    // direction; `fnv_combat::melee`), else the weapon's attack animation.
    let attack = match anim.swing.clone() {
        Some(stem) => m.intern(stem),
        None => m.intern(attack_stem(anim.attack.as_deref())),
    };
    let reload = anim.reload.clone().map(|r| m.intern(r));
    let attack_clip = timing_rig.clip(&mut ctx, ClipKey::Attack(stance, prefix, attack, timing_rig.sights_clips(control.aiming)));
    let reload_clip = reload.and_then(|r| timing_rig.clip(&mut ctx, ClipKey::Reload(stance, prefix, r)));
    // A reload keeps a lowered weapon up for its length (`weapon_lower.rs`).
    let reload_len = reload_clip.as_ref().map_or(0.0, |c| c.clip.length() / if anim.reload_rate > 0.0 { anim.reload_rate } else { 1.0 });
    let reload_open = reload.and_then(|r| timing_rig.clip(&mut ctx, ClipKey::ReloadOpen(stance, prefix, r)));
    let jam = anim.jam.clone().map(|j| m.intern(j));
    let jam_len = jam.and_then(|j| timing_rig.clip(&mut ctx, ClipKey::Reload(stance, prefix, j))).map(|c| c.clip.length()).filter(|l| *l > 0.0);
    anim.timing = nvf_play::combat::clip_timing(
        attack_clip.as_ref().map(|c| (c.clip.length(), c.keys.hit, c.keys.hit.zip(c.keys.loop_end).is_some_and(|(fire, back)| fire < back))),
        reload_clip.as_ref().map(|c| (c.clip.length(), c.keys.hit)),
        reload_open.as_ref().map(|c| c.clip.length()),
        attack_clip.as_ref().and_then(|c| c.keys.next_attack.as_ref()).map(|(t, v)| (*t, v.as_str())),
        attack_clip.as_ref().and_then(|c| c.keys.attach),
    );
    anim.timing.jam_len = jam_len;
    anim.ejects_on_key = attack_clip.as_ref().is_some_and(|c| c.keys.eject.is_some());
    anim.timed = Some(attack.to_string());
    anim.ready = m.hands.phase == WeaponPhase::Drawn && m.hands.in_hand;
    anim.drawing = m.hands.phase == WeaponPhase::Drawing;
    anim.holstering = m.hands.phase == WeaponPhase::Holstering;
    let attack_now = anim.attacks != m.attacks_seen;
    let reload_now = anim.reloads != m.reloads_seen;
    let reload_round_now = anim.reload_rounds != m.reload_rounds_seen;
    let spin_now = anim.spin_ups != m.spin_ups_seen;
    let wind_now = anim.wind_ups != m.wind_ups_seen;
    m.wind_ups_seen = anim.wind_ups;
    m.attacks_seen = anim.attacks;
    m.reloads_seen = anim.reloads;
    m.reload_rounds_seen = anim.reload_rounds;
    let jam_now = anim.jams != m.jams_seen;
    m.jams_seen = anim.jams;
    m.spin_ups_seen = anim.spin_ups;

    // While an idle turns the first-person view, its rig keeps the heading the idle
    // started at and the camera node alone places the view: mouse look and movement are
    // ignored (`nvf_play::player::ViewDriven`) [design: the owner's wish, playtest of
    // 2026-10-11; the opening's scripts leave looking on, so the original may let the
    // view turn; hypothesis that the body does not swing round with the view].
    if !idles.holds_camera() {
        if let Some(h) = idles.held.take() {
            seat.hold = None;
            // The character stands where it stood before the idles held it, clear of what
            // it collides with: let go where it was held (the opening's marker, against
            // the bed), it was pushed up onto the bed frame [design: owner report
            // 2026-10-16; idles.md §4]. The stand-up ends with the view over the marker,
            // so the view steps aside by that much (22 units in the opening).
            let hit = phys.place_player(h.free, 60.0, 200.0);
            let stood = phys.player.as_ref().map_or(h.free, |c| c.feet);
            info!(
                "player view: idle over; the character stands at ({:.0}, {:.0}, {:.0}), {:.0} units from where it was held{}",
                stood[0],
                stood[1],
                stood[2],
                (stood[0] - h.feet[0]).hypot(stood[1] - h.feet[1]),
                if hit.is_some() { "" } else { " (no ground under it)" }
            );
            player.feet = stood;
            m.after_idle = Some(1.0);
        }
    }
    // The game put the player somewhere else while held (a script's move landing after
    // the idle began): the scene plays there.
    if let Some(h) = idles.held.and_then(|h| h.moved(player.placed, player.heading, player.pitch)) {
        info!("player view: the game moved the player while an idle holds them: held at ({:.0}, {:.0}, {:.0}) now", h.feet[0], h.feet[1], h.feet[2]);
        idles.held = Some(h);
    }
    // A second later: where the collision has left the character (a smoke check).
    if let Some(t) = m.after_idle.as_mut() {
        *t -= dt;
        if *t <= 0.0 {
            m.after_idle = None;
            let at = player.feet;
            info!("player view: a second after the idle the character stands at ({:.0}, {:.0}, {:.0})", at[0], at[1], at[2]);
        }
    }
    if driven.0 != idles.held.is_some() {
        driven.0 = idles.held.is_some();
        info!("player view: {}", if driven.0 { "driven by an idle's camera node; mouse look and movement ignored" } else { "back to the player" });
        if own_body {
            info!("player body: {} in first person", if driven.0 { "not drawn while the idle drives the view" } else { "drawn again" });
        }
    }
    // The switches of Advanced movement (`nvf_play::inertia::MovementInertia`, the main
    // switch over all; the sway's, the head bob's and the breath's are `PlayerBody`'s).
    let smooth_with = inertia.as_deref().filter(|i| i.smooth_hands());
    let smooth = smooth_with.is_some();
    let advanced = inertia.as_deref().is_none_or(|i| i.advanced);
    let is_toddler = toddler.as_deref().is_some_and(|t| t.active);
    if let Some(t) = toddler.as_deref_mut() {
        t.clip_walk_speed = if t.active {
            let key = ClipKey::Move(Stance::Stand, body.female, false, Dir::Forward, None, false, true);
            body.third
                .clip(&mut ctx, key)
                .and_then(|c| c.speed)
                .or_else(|| body.first.as_mut().and_then(|f| f.clip(&mut ctx, key)).and_then(|c| c.speed))
        } else {
            None
        };
    }
    let frame = Frame {
        inertia: smooth,
        pipboy_manual: pip.manual_pose(),
        crouch_jump: inertia.as_deref().is_some_and(|i| i.eased_crouch()),
        aim_fade: inertia.as_deref().filter(|i| i.smooth_hands() || i.handles()).map(|i| i.aim_seconds(control.aiming)),
        stance_fade,
        move_fade: smooth.then_some(nvf_play::inertia::MOVE_FADE_SECONDS),
        land_stretch: inertia.as_deref().filter(|i| i.landing_weight()).map(|i| i.land_stretch(m.landed_speed)),
        hands_pace: smooth_with.map(|i| i.hands()),
        equip_at,
        dt,
        stance,
        dirs: direction_weights(forward, right),
        fast,
        female: body.female,
        limping: limbs.as_deref().is_some_and(|l| l.limping()),
        toddler: is_toddler,
        prefix,
        grip: m.grip,
        drawn: (m.hands.phase == WeaponPhase::Drawn).then_some(prefix),
        hands: m.hands,
        airborne: if m.falling { m.airborne } else { 0.0 },
        landing,
        turn_rate: m.turn_rate,
        pitch: player.pitch,
        // Not while an idle drives the view (the opening's wake-up): the arms rig plays
        // the idle and the body would stand where the view lies [design: owner report
        // 2026-10-16, "the players own body standing over themselves"].
        upright: own_body && !third_view && body.first.is_some() && idles.held.is_none(),
        aiming: control.aiming,
        speed: m.speed,
        attack: Some(attack),
        reload,
        jam,
        jam_now,
        attack_now,
        reload_now,
        reload_round_now,
        reload_rate: if anim.reload_rate > 0.0 { anim.reload_rate } else { 1.0 },
        attack_rate: if anim.attack_rate > 0.0 { anim.attack_rate } else { 1.0 },
        equip_rate: eq,
        spin_now,
        wind_now,
        throw_held: anim.throw_held,
    };
    // The body stands on the drawn ground under the feet (`nvf_play::phys::drawn_sink`);
    // its eye counts from the feet as before: without the sink, and without the share of
    // the lift that easing the body onto changes of the drawn ground put there (else a
    // crack in a broken road's drawn floor moved the view at once and eased it back,
    // feedback 2026-11-12; `step_lift::StepLift::ground_at`).
    let sink = Vec3::Z * phys.drawn_sink_of(FormId::PLAYER_REF.0);
    let ground_lift = phys.drawn_ground_lift_of(FormId::PLAYER_REF.0);
    let eye_sink = sink - Vec3::Z * ground_lift;
    // A script's SetScale on the player sizes both rigs, so the view (the first-person
    // camera node) comes down with the body: Fallout 3's baby stage, CG01 stage 0,
    // `player.SetScale 0.4`, stage 100 `1` [observed: TTW's Fallout3.esm, fnvtool script
    // decompile; owner's TTW run 2026-11-02, the baby was drawn full size].
    if *told_scale != Some(script_scale) {
        if told_scale.is_some() || script_scale != 1.0 {
            info!("player body: drawn at scale {script_scale:.2} (SetScale), race height {:.2}", body.scale);
        }
        *told_scale = Some(script_scale);
    }
    // Sprint (`sprint_view.rs`): its share, how out of breath it left the player, and in
    // third person the body's lean about its feet.
    let (sprint_share, winded) = stance_motion.as_deref().map_or((0.0, 0.0), |s| (s.sprint_share, s.winded));
    let lean = if third_view { crate::sprint_view::lean(sprint_share) } else { Quat::IDENTITY };
    let root = Transform { translation: Vec3::from_array(feet) - sink, rotation: Quat::from_rotation_z(-player.heading) * lean, scale: Vec3::splat(body.scale * script_scale) };
    let third = Posed { root, look_pitch: player.pitch, special: &idles.third, sequences: &sequences.third, sequence_dt: dt, pipboy_dt: 0.0, pipboy: None, sway: SwayPose::NONE, sights: None };
    let third_eye = pose_rig(&mut body.third, &frame, &events, third, &mut ctx, (&mut transforms, &parents)).map(|(e, _)| e + eye_sink);
    // The third-person body's melee attack clip carries the player by its root's travel
    // (actors.md §8.13), turned by the heading and scaled with the body.
    if let Some(d) = body.third.carry.filter(|_| dt > 0.0) {
        let (s, c) = player.heading.sin_cos();
        let k = body.scale * script_scale;
        lunge.owed[0] += (d.x * c + d.y * s) * k;
        lunge.owed[1] += (-d.x * s + d.y * c) * k;
        if !was_carrying {
            info!("player: the body's attack clip carries the player");
        }
        lunge.carrying = true;
    }
    let (held, pitch, mut at) = idles.held.map_or((player.heading, player.pitch, [feet[0], feet[1], feet[2] - ground_lift]), |h| (h.heading, player.pitch - h.pitch, h.feet));
    if let Some(h) = idles.held {
        // The character stays where the scene plays (and collides with nothing there).
        seat.hold = Some(h.feet);
    }
    let pipboy = (pip.on_arm() && pip.warming.is_none()).then_some(pip.clip_time);
    // A heavier weapon in the hand sways slower and further, with its switch on.
    let gun_weight = combat.as_deref().filter(|_| m.hands.in_hand).and_then(|c| c.forms.weapons.get(&c.player_weapon()?)).map_or(0.0, |w| w.weight);
    let weight = gun_weight;
    // A shot of a gun or launcher this frame: its recoil on the gun's springs, stronger for
    // a heavier gun (`view_sway.rs`); thrown and melee weapons have none.
    let soft_recoil = inertia.as_deref().is_none_or(|i| i.soft_recoil());
    let kick = if attack_now && matches!(prefix, "1hp" | "2hr" | "2ha" | "2hh" | "2hl") { 0.7 + 0.6 * nvf_play::ease::smoothstep(2.0, 20.0, gun_weight) } else { 0.0 };
    // The blows taken since last frame, as one punch (already in the view's frame).
    let punch = blows.as_deref_mut().map_or(Vec3::ZERO, |b| std::mem::take(&mut b.punch));
    let punch = if advanced && state.head_bob { punch } else { Vec3::ZERO };
    let hard_landings = inertia.as_deref().is_some_and(|i| i.landing_weight());
    // The arms' field of view: the console's `fov` can ask for another first-person field
    // of view; otherwise the settings page's (`--viewmodel-fov`). The sway, tuned at the
    // original's 55°, is scaled against the arms' size for it, so it looks the same size at
    // any viewmodel field of view (`view_sway::rescale`).
    let viewmodel_fov = {
        let (asked_fov, args, _) = &first_fov;
        asked_fov.as_ref().and_then(|f| f.0).or_else(|| args.as_ref().map(|a| a.viewmodel_fov)).unwrap_or(FIRST_PERSON_FOV)
    };
    let sway_share = arms_magnification(1.0, FIRST_PERSON_FOV) / arms_magnification(1.0, viewmodel_fov).max(1e-3);
    // The arms sway in first person by themselves: not under the Pip-Boy or an idle.
    let sway = body.first.as_ref().map_or(SwayPose::NONE, |f| {
        m.sway.update(SwayInput {
            dt,
            heading: player.heading,
            pitch: player.pitch,
            velocity: m.velocity,
            vertical,
            phase: f.animator.phase,
            sights: control.aiming,
            sneaking: stance == Stance::Sneak,
            airborne: m.falling,
            heft: crate::view_sway::heft(weight),
            breathing: true,
            hard_landings,
            // How far the gun is up to aim or block: the true iron sights' own share
            // while they ran last frame, else the aim or block clip asked for.
            raised: if m.sights_ran { m.sights_eased } else { f32::from(u8::from(control.aiming && m.hands.in_hand)) },
            // How far the weapon is drawn: with movement inertia the draw's and the holster's
            // own progress, so their trail moves with the clip; a step at the draw's end
            // dipped the gun 1.4 units after the clip had settled (ninth playtest's trace).
            drawn: match (equip_at, m.hands.phase) {
                (Some(at), WeaponPhase::Drawing) => at,
                (Some(at), WeaponPhase::Holstering) => 1.0 - at,
                _ => f32::from(u8::from(m.hands.in_hand && m.hands.phase == WeaponPhase::Drawn)),
            },
            kick,
            punch,
            exertion: if advanced && state.head_bob { m.head.exertion() } else { 0.0 },
            reloading: f32::from(u8::from(f.animator.layers.iter().any(|l| l.tag == TAG_RELOAD && !l.finished() && !l.is_leaving()))),
            on: advanced && state.weapon_sway && !third_view && idles.held.is_none() && !idles.first.covers() && pipboy.is_none(),
            sprint: sprint_share,
        })
    });
    let sway = crate::view_sway::rescale(sway, sway_share);
    let mut moved = [sway != SwayPose::NONE, false];
    // The head bob in first person moves the whole arms rig, camera node, arms and
    // weapon together (`head_bob.rs`).
    if let Some(f) = body.first.as_ref() {
        let o = m.head.first_person(crate::head_bob::BobInput {
            dt,
            speed: m.speed,
            phase: f.animator.phase,
            heading: player.heading,
            sights: control.aiming,
            sneaking: stance == Stance::Sneak,
            airborne: m.falling,
            velocity: m.velocity,
            vertical,
            out_of_breath: advanced && state.head_bob,
            hard_landings,
            // The take-off and landing dip is Movement Inertia's, the rest Camera Motion's.
            landing: hard_landings && !third_view && idles.held.is_none() && !idles.first.covers() && pipboy.is_none(),
            breathing: advanced && state.head_bob && !third_view && idles.held.is_none() && !idles.first.covers() && pipboy.is_none(),
            // The view's nudge back from a shot is Advanced Recoil's.
            kick: if soft_recoil { kick } else { 0.0 },
            punch,
            on: advanced && state.head_bob && !third_view && idles.held.is_none() && !idles.first.covers() && pipboy.is_none(),
            // Sprint's bob and its out of breath, Camera Motion's (`sprint_view.rs`).
            sprint: sprint_share,
            winded: if advanced && state.head_bob { winded } else { 0.0 },
        });
        at = [at[0] + o.x, at[1] + o.y, at[2] + o.z];
        moved[1] = o != Vec3::ZERO;
    }
    for (at, moved) in m.moved_at.iter_mut().zip(moved) {
        if moved {
            *at = Some(time.elapsed_secs());
        }
    }
    // Lowered against a wall or a friend ahead (`weapon_lower.rs`): a drawn weapon, not
    // fists; aiming, the trigger, an attack or a reload bring it up.
    let lowered = m.lower.update(LowerInput {
        dt,
        // The switch is for the automatic lowering; by hand works either way.
        wall: ahead.wall.filter(|_| state.weapon_lower),
        friendly: ahead.friendly && state.weapon_lower,
        by_hand: control.lowered,
        up_for: if reload_now {
            reload_len
        } else if control.aiming || control.trigger || attack_now {
            crate::weapon_lower::HOLD_UP_SECONDS
        } else {
            0.0
        },
        on: body.first.is_some() && !third_view && idles.held.is_none() && !idles.first.covers() && pipboy.is_none() && m.hands.phase == WeaponPhase::Drawn && prefix != UNARMED,
    });
    if m.lower.down != m.lower_told {
        m.lower_told = m.lower.down;
        info!("weapon lowering: {}", if m.lower.down { if control.lowered { "lowered by hand" } else if ahead.friendly { "lowered for a friendly actor ahead" } else { "lowered against a surface ahead" } } else { "raised" });
    }
    let sway = crate::weapon_lower::then(sway, crate::weapon_lower::pose(lowered, crate::weapon_lower::weight(prefix)));
    // Sprinting lowers the gun further (`sprint_view.rs`), faded where the arms are not the
    // player's.
    let sprint_pose = m.sprint.pose_share(sprint_share, idles.held.is_none() && !idles.first.covers() && pipboy.is_none(), dt);
    let sprint_weight = if prefix == UNARMED { crate::sprint_view::FISTS } else { crate::weapon_lower::weight(prefix) };
    let sway = crate::weapon_lower::then(sway, crate::sprint_view::pose(sprint_pose, sprint_weight));
    // True iron sights: brought up at their own pace while the view zooms in, for the
    // guns whose model has a sighting node, while the player looks through the arms rig
    // by themself.
    // A 3D scope brings the gun's eyepiece to the eye the same way (`scope_lens`), on its
    // own switch, for scoped guns whose model the table knows.
    let has_lens = body.first.as_ref().is_some_and(|f| f.scope_eye.is_some());
    if let Some(l) = lens3d.as_deref_mut() {
        if l.lens != has_lens {
            l.lens = has_lens;
        }
    }
    let lens_aim = lens3d.as_deref().is_some_and(|l| l.on) && has_lens && combat.as_deref().is_some_and(|c| c.player_weapon_scoped());
    if let Some(f) = body.first.as_mut() {
        f.use_lens = lens_aim;
    }
    let sights_can = !third_view && idles.held.is_none() && !idles.first.covers() && pipboy.is_none() && combat.as_deref().is_some_and(|c| c.sight_fov().is_some()) && (lens_aim || (state.true_iron_sights && body.first.as_ref().is_some_and(|f| f.sighting.is_some())));
    // With movement inertia the gun's way up to the eye takes the aim clips' cross-over time
    // (`Frame::aim_fade`).
    let aimed = zoom.as_deref().is_some_and(|z| z.aimed);
    // `sights_share`: the way's progress (0 down, 1 on the eye); `sights_eased`: how far the
    // gun is moved. With movement inertia along the settling curve over the aim's times
    // (the aim clips' cross-over and the zoom run on the same), else straight and eased.
    (m.sights_share, m.sights_eased) = match (sights_can, smooth_with) {
        (false, _) => {
            m.sights.reset();
            (0.0, 0.0)
        }
        (true, None) => {
            // Weapon Handling's pace, else the sights' own; the settling curve kept where
            // the gun is, so Animation Smoothing switched on part-way goes on from there.
            let s = match inertia.as_deref().filter(|i| i.handles()) {
                Some(i) => crate::iron_sights::step_over(m.sights_share, aimed, dt, i.aim_seconds(true), i.aim_seconds(false)),
                None => crate::iron_sights::step(m.sights_share, aimed, dt),
            };
            let eased = crate::iron_sights::ease(s);
            m.sights.place(eased, aimed);
            (s, eased)
        }
        (true, Some(i)) => {
            let v = m.sights.step(aimed, dt, i.aim_seconds(true), i.aim_seconds(false));
            (if m.sights.is_up() { 1.0 } else if m.sights.is_down() { 0.0 } else { v.clamp(1e-4, 1.0 - 1e-4) }, v)
        }
    };
    m.sights_ran = sights_can;
    let sights_in = (m.sights_share > 0.0).then_some(m.sights_eased);
    // The view's direction in the arms rig's own frame (turned by the heading only); a 3D
    // scope's sway turns the gun off it (`scope_view.rs`).
    let gun_dir = match lens_sway.as_deref().filter(|_| lens_aim) {
        Some(s) => s.look_dir(player.heading, player.pitch),
        None => nvf_play::space::look_dir(player.heading, player.pitch),
    };
    let sights = sights_in.map(|s| (s, Quat::from_rotation_z(held) * Vec3::from_array(gun_dir)));
    // The crosshair goes while the gun is on its way to the eye.
    let lined = sights.is_some();
    if let Some(t) = true_sights.as_deref_mut() {
        t.0 = lined;
    }
    if (m.sights_share >= 1.0) != m.sights_told {
        m.sights_told = !m.sights_told;
        info!("true iron sights: {}", if m.sights_told { "the sights are on the eye" } else { "off the eye" });
    }
    let pipboy_dt = if pipboy.is_some() && !paused.as_deref().is_some_and(|p| p.0) {
        dt.max(play_clock.as_deref().and_then(nvf_play::pause::PlayClock::frame_secs).unwrap_or(0.0))
    } else {
        0.0
    };
    let first = Posed { root: Transform { translation: Vec3::from_array(at), rotation: Quat::from_rotation_z(-held), ..root }, look_pitch: pitch, special: &idles.first, sequences: &sequences.first, sequence_dt: dt.max(sequences.first_menu_dt), pipboy_dt, pipboy, sway, sights };
    // With movement inertia the arms' looping clips have their seams smoothed (the idle's
    // hand turned back in one frame at each loop; `nvf_assets::anim_blend` `Seams`).
    if let Some(f) = body.first.as_mut() {
        f.animator.smooth_seams(smooth.then_some(SEAM_SECONDS));
    }
    let first = body.first.as_mut().and_then(|f| pose_rig(f, &frame, &events, first, &mut ctx, (&mut transforms, &parents)));
    let first_eye = first.map(|(e, _)| e);
    // The arms and the weapon are drawn as through the first-person view, narrower than
    // the world's, and narrower still while the Pip-Boy shows.
    let mut lens_k = 1.0;
    if let (Some(f), Some(eye)) = (body.first.as_ref(), first_eye) {
        let vfov = cams.single().ok().and_then(|p| match p {
            Projection::Perspective(p) => Some(p.fov),
            _ => None,
        });
        let pip_size = &first_fov.2;
        let first_fov = viewmodel_fov;
        let k = match (pip.on_arm() && pip.warming.is_none(), vfov) {
            (true, Some(v)) => {
                let normal = arms_magnification(v, first_fov);
                normal + (arms_magnification(v, pip_size.as_deref().map_or(PIPBOY_FOV, |s| s.device_fov(PIPBOY_FOV))) - normal) * pipboy_ease(pip.clip_time)
            }
            (false, Some(v)) => arms_magnification(v, first_fov),
            _ => 1.0,
        };
        let forward = Vec3::from_array(nvf_play::space::look_dir(player.heading, player.pitch));
        place_lens(f, eye, forward, k, &mut transforms);
        lens_k = k;
    }
    match first.and_then(|(_, v)| v) {
        Some(v) => {
            let v = Quat::from_rotation_z(held - player.heading) * v;
            player.body_view = Some(v);
            m.idle_view = Some(v);
        }
        // The idle is over: the view goes on from where it left it.
        None => {
            if let Some(v) = m.idle_view.take() {
                let fwd = v * Vec3::Y;
                player.heading = fwd.x.atan2(fwd.y);
                player.pitch = fwd.z.clamp(-1.0, 1.0).asin();
            }
        }
    }
    // Sprint's side sway (`sprint_view.rs`, Camera Motion): a small roll of the first-person
    // view about its own line, so the crosshair's point and the aim stay.
    let stride = body.first.as_ref().map_or(0.0, |f| f.animator.phase);
    let roll_on = advanced && state.head_bob && !third_view && idles.held.is_none() && !idles.first.covers() && pipboy.is_none();
    let roll = m.sprint.roll(stride, sprint_share, roll_on, dt);
    if roll != 0.0 && player.body_view.is_none() && !third_view {
        player.body_view = Some(crate::sprint_view::rolled_view(player.heading, player.pitch, roll));
    }
    // A script's nodes above the camera node turn the first-person view with them (a
    // lean's roll, `script_offset`); an idle's view has them already.
    let script_turn = body.first.as_ref().and_then(|f| f.script_turn).filter(|_| !third_view && first.and_then(|(_, v)| v).is_none());
    if let Some(turn) = script_turn {
        let view = player.body_view.unwrap_or_else(|| crate::sprint_view::rolled_view(player.heading, player.pitch, 0.0));
        player.body_view = Some(turn * view);
    }
    state.script_view = script_turn.is_some();
    m.events = events;

    // The eye: the first-person camera node; without that rig, the head's eye, swung
    // forward about the neck by the pitch while no weapon bends the body.
    let eye = first_eye.or_else(|| {
        let mut e = third_eye?;
        if let (false, Some(h)) = (m.hands.in_hand, body.third.head) {
            let head = Affine3A::from_scale_rotation_translation(root.scale, root.rotation, root.translation) * body.third.animator.model_space(h);
            let right = Vec3::new(player.heading.cos(), -player.heading.sin(), 0.0);
            let neck = Vec3::from(head.translation) + eye_sink;
            e = neck + Quat::from_axis_angle(right, NECK_PITCH * player.pitch) * (e - neck);
        }
        Some(e)
    });
    player.body_eye = eye.map(|e| e.to_array());
    // Test aid: one line a frame of the first-person motion (`MotionTrace`).
    if trace.wanted() {
        let left = body.first.as_ref().and_then(|f| {
            let bone = |n: &str| f.animator.skel.by_name.get(n).copied();
            Some(Vec3::from(f.animator.model_space(bone("Bip01 L Hand")?).translation) - Vec3::from(f.animator.model_space(bone("Camera1st")?).translation))
        });
        let hand = body.first.as_ref().and_then(|f| {
            let bone = |n: &str| f.animator.skel.by_name.get(n).copied();
            let (h, c) = (f.animator.model_space(bone("Bip01 R Hand")?), f.animator.model_space(bone("Camera1st")?));
            let rot = |a: Affine3A| a.to_scale_rotation_translation().1;
            let turn = rot(c).inverse() * rot(h);
            Some((Vec3::from(h.translation) - Vec3::from(c.translation), turn.to_scaled_axis()))
        });
        // Each hand in the weapon's frame (the `Weapon` node, model space): where it holds
        // the gun, to see a hand leave its grip; the right hand with its turn there.
        let grips = body.first.as_ref().filter(|_| m.hands.in_hand).and_then(|f| {
            let gun = f.weapon_node_at(Transform::IDENTITY, &[])?.inverse();
            let bone = |n: &str| f.animator.skel.by_name.get(n).map(|&b| gun * f.animator.model_space(b));
            let (r, l) = (bone("Bip01 R Hand")?, bone("Bip01 L Hand")?);
            Some((Vec3::from(r.translation), r.to_scale_rotation_translation().1.to_scaled_axis().length().to_degrees(), Vec3::from(l.translation)))
        });
        trace.write(time.elapsed_secs_f64(), dt, &player, eye, hand, sway, (m.sights_share, control.aiming, m.falling, stance == Stance::Sneak, m.speed, m.hands.phase == WeaponPhase::Drawn), m.airborne, left, lens_k, grips);
        trace.write_layers(time.elapsed_secs_f64(), &m.hands, body.first.as_ref().map(|f| &f.animator));
    }
    state.head_eye = third_eye.filter(|_| third_view).map(|e| e.to_array());
    // The third-person pivot: the head's height above the feet without the stride's
    // rise and fall (`head_bob.rs`).
    view.pivot = third_eye.or(eye).map(|e| {
        // From the feet without the drawn ground's share of the lift, as the eye.
        let base = feet[2] - ground_lift;
        let up = e.z - base;
        let up = if third_view { m.head.pivot_height(up, dt) } else { up };
        [feet[0], feet[1], base + up + 5.0]
    });
    if !third_view {
        m.head.pivot_reset();
    }

    // The body in first person stands where its eyes are under the camera (and a little
    // behind), feet on the ground: the view is the arms rig's, the body is moved to it.
    if let (true, Some(cam), Some(head)) = (frame.upright, first_eye, third_eye) {
        let back = root.rotation * Vec3::new(0.0, OWN_BODY_BACK * root.scale.x, 0.0);
        if let Ok(mut t) = transforms.get_mut(body.third.root) {
            t.translation += Vec3::new(cam.x - head.x - back.x, cam.y - head.y - back.y, 0.0);
        }
    }

    // The weapon nodes' parents, and the weapon parts the clips show or hide (a part
    // no playing clip speaks for stays as it was).
    for rig in std::iter::once(&mut body.third).chain(body.first.as_mut()) {
        for p in &mut rig.weapon_parts {
            for (tag, on) in &mut p.tags {
                if let Some(v) = rig.animator.visible(tag) {
                    *on = v;
                }
            }
            let shown = !p.never && p.tags.iter().all(|(_, on)| *on);
            if p.shown != Some(shown) {
                if let Ok(mut v) = visibility.get_mut(p.entity) {
                    *v = if shown { Visibility::Inherited } else { Visibility::Hidden };
                    p.shown = Some(shown);
                }
            }
        }
        if rig.attached != rig.weapon_parent {
            if let (Some(w), Some(p)) = (rig.weapon_node, rig.weapon_parent) {
                commands.entity(rig.animator.nodes[w]).insert(ChildOf(rig.animator.nodes[p]));
                // Smoke tests read it (`varmint-after-fists`): the weapon drawn hangs from
                // its own draw clip's bone.
                info!(
                    "player {:?} rig weapon {}: now on {:?}",
                    rig.view,
                    m.weapon.filter(|_| rig.weapon_model.is_some()).map_or("none".to_string(), |w| w.to_string()),
                    rig.animator.skel.names[p]
                );
                rig.attached = Some(p);
            }
        }
    }
    // Which rig is seen; without a first-person rig the third-person head is hidden
    // from inside. With the body in first person, the arms rig's arms and weapon stand
    // in for the body's own while they are up (weapon out, drawing or putting away, or
    // the Pip-Boy raised: `Seen::upright`).
    let has_first = body.first.is_some();
    // The muzzle of the weapon in hand, for combat (shots are drawn from it;
    // `WeaponAnim::muzzle`): in the first-person rig while that view is on (its weapon
    // is the one seen in hand, with or without the body below it), else the body's.
    let seen = match body.first.as_ref() {
        Some(f) if !third_view => f,
        _ => &body.third,
    };
    anim.muzzle = if m.hands.in_hand { seen.muzzle_at(root).map(|p| p.to_array()) } else { None };
    state.weapon_aim = if m.hands.in_hand { seen.weapon_aim_at(root) } else { None };
    state.muzzle_entity = seen.muzzle_entity.filter(|_| m.hands.in_hand);
    state.casing_entity = seen.casing_entity.filter(|_| m.hands.in_hand);
    anim.casing = if m.hands.in_hand { seen.casing_at(root).map(|(p, d)| (p.to_array(), d.to_array())) } else { None };
    if seen.ejected && m.hands.in_hand {
        anim.ejects = anim.ejects.wrapping_add(1);
    }
    let seen = if frame.upright {
        Seen::upright(m.hands.in_hand, m.hands.phase != WeaponPhase::Holstered, pipboy.is_some())
    } else {
        Seen { body: third_view || !has_first, head: third_view || has_first, arms: true, weapon: true }
    };
    // Through a scope the arms rig is not drawn (docs/spec-notes/weapon-scopes.md §2),
    // unless the scope is shown in its lens (3D scopes).
    let scoped = scope.as_deref().is_some_and(|s| s.on && !s.lens_3d);
    // The console's Show1stPerson shows it in third person too, at the eye.
    let s1st = s1st.is_some_and(|s| s.0);
    // Nor while the classic VATS style chooses targets: the view is on the target alone
    // [observed: the owner's screenshots of the original's VATS, 2026-11-10, no arms or
    // weapon in the picture].
    let targeting = vats.is_some_and(|v| v.selected().is_some());
    let seen = Seen { body: seen.body && !state.out_of_view, ..seen };
    show_rigs(body, &mut commands, &mut visibility, seen, (!third_view || s1st) && !scoped && !targeting, hidden);

    m.log_elapsed += dt;
    if m.log_elapsed >= 1.0 {
        m.log_elapsed = 0.0;
        let layers = |r: &Rig| {
            let playing = r.animator.layers.iter().filter(|l| l.ease > 0.01).map(|l| format!("{}:{}×{:.2}", l.tag, l.clip.name, l.weight)).collect::<Vec<_>>().join(" ");
            // The weapon parts the clips hold away from their place in the model.
            let parts: Vec<String> = r.animator.extra_moved().filter(|p| p.1 > 0.01 || p.2 > 0.1).map(|(n, d, turn)| format!("{n} {d:.1} units {turn:.0}°")).collect();
            if parts.is_empty() {
                playing
            } else {
                format!("{playing}; weapon parts {}", parts.join(", "))
            }
        };
        debug!(
            "player body: {stance:?} speed {:.0} weapon {:?} {} in hand {} airborne {:.2}s view {} eye z {:.1} (feet {:.1}); third [{}]; first [{}]",
            m.speed,
            m.hands.phase,
            m.prefix,
            m.hands.in_hand,
            m.airborne,
            if third_view { "third" } else { "first" },
            player.body_eye.map_or(f32::NAN, |e| e[2]),
            feet[2],
            layers(&body.third),
            body.first.as_ref().map_or(String::new(), layers),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hand_over_sets_off_softly_and_ends_at_rest() {
        assert_eq!(handover_share(0.0), 0.0);
        assert_eq!(handover_share(EQUIP_HANDOVER_FROM), 0.0);
        assert_eq!(handover_share(1.0), 1.0);
        let d = 1e-3;
        let speed = |x: f32| (handover_share(x + d) - handover_share(x)) / d;
        assert!(speed(EQUIP_HANDOVER_FROM) < 1e-3 && speed(1.0 - d) < 1e-2, "no speed at either end");
        // Its pull grows from nothing: a tenth of the way in it moves at under a fifth of its
        // top speed (the settling curve's start was at about three fifths).
        let top = speed(0.5 * (1.0 + EQUIP_HANDOVER_FROM));
        assert!(speed(EQUIP_HANDOVER_FROM + 0.1 * (1.0 - EQUIP_HANDOVER_FROM)) < 0.2 * top);
        // Its last quarter only tapers off: under a fifth of the way left, entered at under
        // two thirds of its top speed (the owner, 2026-11-16: "make the last 25% smoother").
        let top = (0..100).map(|i| speed(EQUIP_HANDOVER_FROM + i as f32 * 0.0064)).fold(0.0f32, f32::max);
        assert!(1.0 - handover_share(EQUIP_TAIL_FROM) < 0.2, "{} left", 1.0 - handover_share(EQUIP_TAIL_FROM));
        assert!(speed(EQUIP_TAIL_FROM) < 0.67 * top, "{} against {top}", speed(EQUIP_TAIL_FROM));
        // (Steps of a hundredth: finer ones are lost to f32 rounding of the polynomial.)
        let coarse = |x: f32| (handover_share(x + 0.01) - handover_share(x)) / 0.01;
        assert!((1..=24).all(|i| coarse(EQUIP_TAIL_FROM + i as f32 * 0.01) <= coarse(EQUIP_TAIL_FROM + (i - 1) as f32 * 0.01)), "it only slows over the last quarter");
    }

    #[test]
    fn a_draws_last_quarter_eases_to_rest_from_the_settling_curve() {
        // The settling curve up to the last quarter, the same ends.
        for x in [0.0, 0.2, 0.5, EQUIP_TAIL_FROM] {
            assert_eq!(equip_curve(x), nvf_play::ease::settle(x), "at {x}");
        }
        assert!((equip_curve(1.0) - 1.0).abs() < 1e-6);
        // Place, speed and pull carry on across the join.
        let (_, v, a) = nvf_assets::anim_blend::settle_slopes(EQUIP_TAIL_FROM);
        let d = 1e-3;
        let (x0, x1, x2) = (EQUIP_TAIL_FROM, EQUIP_TAIL_FROM + d, EQUIP_TAIL_FROM + 2.0 * d);
        let speed = (equip_curve(x1) - equip_curve(x0)) / d;
        let pull = (equip_curve(x2) - 2.0 * equip_curve(x1) + equip_curve(x0)) / (d * d);
        assert!((speed - v).abs() < 0.01 && (pull - a).abs() < 0.2, "speed {speed} against {v}, pull {pull} against {a}");
        // It only rises, and arrives with no speed and no pull, where the settling curve
        // still brakes.
        let n = 1000;
        assert!((1..=n).all(|i| equip_curve(i as f32 / n as f32) >= equip_curve((i - 1) as f32 / n as f32)));
        let (_, end_speed, end_pull) = nvf_assets::anim_blend::settle_slopes(1.0);
        assert!(end_speed.abs() < 1e-4 && end_pull < -0.5, "the settling curve brakes to its end: {end_pull}");
        let h = 0.01;
        let tail_speed = (1.0 - equip_curve(1.0 - h)) / h;
        let settle_speed = (1.0 - nvf_play::ease::settle(1.0 - h)) / h;
        assert!(tail_speed < 0.5 * settle_speed, "its last hundredth goes slower: {tail_speed} against {settle_speed}");
        // The inverse finds the time again.
        for x in [0.1, 0.5, 0.8, 0.95] {
            assert!((equip_curve_inv(equip_curve(x)) - x).abs() < 1e-3, "at {x}");
        }
    }

    #[test]
    fn a_clip_is_played_by_its_motion_and_its_jumps_are_found() {
        // All the motion in the first fifth of the time (the fists' draw).
        let done: Vec<f32> = (0..=MOTION_STEPS).map(|i| (i as f32 / (0.2 * MOTION_STEPS as f32)).min(1.0)).collect();
        let m = ClipMotion { done, jumps: vec![0.97] };
        assert!((m.time_for(0.5) - 0.1).abs() < 1e-3, "half the motion at a tenth of the time: {}", m.time_for(0.5));
        assert!((m.time_for(1.0) - 0.2).abs() < 1e-2 && m.time_for(0.0) == 0.0);
        for share in [0.1, 0.3, 0.77, 0.95] {
            assert!((m.done_at(m.time_for(share)) - share).abs() < 1e-3, "{share}");
        }
        assert!(m.jumps_between(0.9, 0.98) && m.jumps_between(0.98, 0.9) && !m.jumps_between(0.5, 0.96) && !m.jumps_between(0.97, 0.99));
        // A still clip goes by its clock.
        let still = ClipMotion { done: (0..=MOTION_STEPS).map(|i| i as f32 / MOTION_STEPS as f32).collect(), jumps: Vec::new() };
        assert!((still.time_for(0.4) - 0.4).abs() < 1e-4);
    }

    #[test]
    fn the_pip_boy_clip_settles_into_its_hold_and_sets_off_from_rest() {
        use nvf_play::pipboy::{CLIP_SECONDS, HOLD_SECONDS};
        let speed = |t: f32| (pipboy_clip_time(t + 1e-3) - pipboy_clip_time(t - 1e-3)) / 2e-3;
        assert_eq!(pipboy_clip_time(0.0), 0.0);
        assert!((pipboy_clip_time(HOLD_SECONDS) - HOLD_SECONDS).abs() < 1e-6);
        assert!((pipboy_clip_time(CLIP_SECONDS) - CLIP_SECONDS).abs() < 1e-5);
        assert!((speed(1e-3) - 1.0).abs() < 0.02, "starts at the clip's own speed: {}", speed(1e-3));
        assert!(speed(HOLD_SECONDS - 1e-3).abs() < 0.03 && speed(HOLD_SECONDS + 1e-3).abs() < 0.03, "at rest at the hold");
        assert!((speed(CLIP_SECONDS - 2e-3) - 1.0).abs() < 0.03, "ends at the clip's own speed");
        let mut last = 0.0;
        for i in 1..=100 {
            let t = pipboy_clip_time(CLIP_SECONDS * i as f32 / 100.0);
            assert!(t >= last, "never backwards");
            last = t;
        }
    }

    #[test]
    fn a_short_pip_boy_clip_is_held_at_its_own_hit_key() {
        use nvf_play::pipboy::{CLIP_SECONDS, HOLD_SECONDS};
        // The original clip: its own times.
        for t in [0.0, 0.1, HOLD_SECONDS, 0.5, CLIP_SECONDS] {
            assert!((pipboy_time_in(0.0, CLIP_SECONDS, Some(HOLD_SECONDS), t) - t).abs() < 1e-5);
        }
        // Nuclear Sunset's 0.067 s clip, Hit at 0.033: held there, not past its end.
        let (hit, stop) = (1.0 / 30.0, 2.0 / 30.0);
        assert!((pipboy_time_in(0.0, stop, Some(hit), HOLD_SECONDS) - hit).abs() < 1e-6);
        assert!((pipboy_time_in(0.0, stop, Some(hit), CLIP_SECONDS) - stop).abs() < 1e-6);
        assert!((pipboy_time_in(0.0, stop, Some(hit), HOLD_SECONDS / 2.0) - hit / 2.0).abs() < 1e-6);
        // No Hit key (or one outside the clip): the original's share of the clip.
        assert!((pipboy_time_in(0.0, stop, None, HOLD_SECONDS) - stop * HOLD_SECONDS / CLIP_SECONDS).abs() < 1e-6);
        assert!((pipboy_time_in(0.0, stop, Some(5.0), HOLD_SECONDS) - stop * HOLD_SECONDS / CLIP_SECONDS).abs() < 1e-6);
    }

    #[test]
    fn hidden_pose_cleanup_restores_transforms_visibility_and_only_its_own_culling() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let original = Transform::from_xyz(1.0, 2.0, 3.0);
        let rig = world.spawn((Transform::from_xyz(9.0, 9.0, 9.0), Visibility::Inherited)).id();
        let borrowed = world.spawn((NoFrustumCulling, WarmUnculled)).id();
        let existing = world.spawn(NoFrustumCulling).id();
        world.insert_resource(WarmPose { transforms: vec![(rig, original)], visibility: Some((rig, Visibility::Hidden)) });
        let mut pip = nvf_play::pipboy::PipBoy::default();
        pip.pose_hidden = true;
        world.insert_resource(pip);
        world.run_system_once(restore_warm_pose).unwrap();
        assert_eq!(*world.get::<Transform>(rig).unwrap(), original);
        assert_eq!(*world.get::<Visibility>(rig).unwrap(), Visibility::Hidden);
        assert!(world.get::<NoFrustumCulling>(borrowed).is_none());
        assert!(world.get::<WarmUnculled>(borrowed).is_none());
        assert!(world.get::<NoFrustumCulling>(existing).is_some());
        assert!(world.resource::<WarmPose>().transforms.is_empty());
        assert!(!world.resource::<nvf_play::pipboy::PipBoy>().pose_hidden);
    }

    #[test]
    fn warm_pose_keeps_no_frustum_culling_across_consecutive_frames_and_cleans_up_on_release_or_abort() {
        use fnv_nif::anim::{Channel, TransformTrack};
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        app.init_resource::<WarmPose>()
            .add_systems(Update, (restore_warm_pose, draw_warm_pose).chain());

        let world = app.world_mut();
        let cam_entity = world
            .spawn((
                GlobalTransform::IDENTITY,
                Projection::Perspective(PerspectiveProjection::default()),
                nvf_play::PlayerCamera,
            ))
            .id();
        let outer = world.spawn((Transform::from_xyz(1.0, 2.0, 3.0), Visibility::Inherited)).id();
        let inner = world.spawn((Transform::IDENTITY, Visibility::Inherited, ChildOf(outer))).id();
        let root = world.spawn((Transform::IDENTITY, Visibility::Hidden, ChildOf(inner))).id();
        let bone = world.spawn((Transform::from_xyz(10.0, 0.0, 0.0), Visibility::Inherited, ChildOf(root))).id();
        let warm_mesh = world.spawn((Mesh3d::default(), Transform::IDENTITY, ChildOf(root))).id();
        let pre_unculled = world.spawn((Mesh3d::default(), NoFrustumCulling, Transform::IDENTITY, ChildOf(root))).id();
        let third_root = world.spawn((Transform::IDENTITY, Visibility::Inherited)).id();

        let skel = Arc::new(PreparedSkeleton {
            names: vec![Some("Root".into()), Some("Arm".into())],
            parents: vec![None, Some(0)],
            locals: vec![Transform::IDENTITY, Transform::from_xyz(10.0, 0.0, 0.0)],
            bind_world: vec![Affine3A::IDENTITY, Affine3A::from_translation(Vec3::new(10.0, 0.0, 0.0))],
            by_name: [("Root".into(), 0), ("Arm".into(), 1)].into_iter().collect(),
            ..Default::default()
        });
        let make_clip = |x: f32, priority: i32| {
            Arc::new(Clip {
                name: "warm".into(),
                start: 0.0,
                stop: 1.0,
                frequency: 1.0,
                cycle_type: 2,
                tracks: vec![(
                    "Arm".into(),
                    Arc::new(TransformTrack {
                        translation: Channel::Static(Some([x, 0.0, 0.0])),
                        rotation: Channel::Static(None),
                        euler: None,
                        scale: Channel::Static(None),
                        per_cycle: None,
                    }),
                )],
                priorities: vec![priority],
                skipped: 0,
                accum_root: None,
                text_keys: vec![],
                visibility: vec![],
                emitters: vec![],
                materials: vec![],
                free: vec![],
                free_visibility: vec![],
                lights: vec![],
            })
        };
        let idle = make_clip(10.0, 10);
        let pip_clip = Arc::new(ClipInfo::new("pipboy".into(), make_clip(42.0, 98), None));
        let mut animator = Animator::new(skel.clone(), vec![root, bone], None);
        animator.smooth_seams(Some(SEAM_SECONDS));
        animator.begin();
        animator.want(&idle, TAG_IDLE, 1.0, Mode::Loop, 0.0);
        animator.end();
        animator.advance(0.1, 1.0);
        animator.pose();
        // World paused: last_dt becomes 0.0 before hidden warming runs.
        animator.advance(0.0, 1.0);

        let third_rig = Rig {
            view: View::Third, root: third_root, lens: None, folder: String::new(), skeleton: String::new(),
            animator: Animator::new(skel, vec![third_root, bone], None),
            clips: HashMap::new(), variants: HashMap::new(), overrides: Arc::default(), active_clip: None, head: None,
            eye_local: Vec3::ZERO, looking: None, camera: None, script_turn: None, posed_root: None,
            script_used: None, translate: None, weapon_node: None, weapon_parent: None, holstered_weapon: None, furniture_logged: None, attached: None,
            weapon_model: None, weapon_emitter: None, weapon_parts: Vec::new(), idle_logged: false,
            sights_turned: None, muzzle: None, muzzle_entity: None, casing: None, casing_entity: None,
            sighting: None, scope_eye: None, use_lens: false, sight_relief: None, drawn: Vec::new(),
            spawned_forms: Vec::new(), shown: Some(false), seen: None, furniture: None,
            since_attack: f32::INFINITY, attack_loop: None, wound_up: None, wound_hit: None,
            sighting2: None, attack_eject: None, ejected: false, carry: None,
        };
        let mut first_rig = Rig {
            view: View::First, root, lens: Some([outer, inner]), folder: String::new(), skeleton: String::new(),
            animator,
            clips: [(ClipKey::PipBoy(false), Some(pip_clip))].into_iter().collect(),
            variants: HashMap::new(), overrides: Arc::default(), active_clip: None, head: None,
            eye_local: Vec3::ZERO, looking: None, camera: None, script_turn: None, posed_root: None,
            script_used: None, translate: None, weapon_node: None, weapon_parent: None, holstered_weapon: None, furniture_logged: None, attached: None,
            weapon_model: None, weapon_emitter: None, weapon_parts: Vec::new(), idle_logged: false,
            sights_turned: None, muzzle: None, muzzle_entity: None, casing: None, casing_entity: None,
            sighting: None, scope_eye: None, use_lens: false, sight_relief: None, drawn: Vec::new(),
            spawned_forms: Vec::new(), shown: Some(false), seen: None, furniture: None,
            since_attack: f32::INFINITY, attack_loop: None, wound_up: None, wound_hit: None,
            sighting2: None, attack_eject: None, ejected: false, carry: None,
        };
        first_rig.clips.insert(ClipKey::Idle(Stance::Stand, false, false), Some(Arc::new(ClipInfo::new("idle".into(), idle, None))));

        let pb = PlayerBody {
            body: Some(Body {
                worn: Vec::new(), look: None, change: fnv_actor::Change::default(),
                female: false, scale: 1.0, third: third_rig, first: Some(first_rig), first_addons: Vec::new(),
            }),
            ..Default::default()
        };
        world.insert_resource(pb);
        let mut pip = nvf_play::pipboy::PipBoy::default();
        pip.phase = nvf_play::pipboy::Phase::Raising;
        pip.warming = Some(nvf_play::pipboy::Warming {
            since: std::time::Instant::now(),
            frames: 0, asked_at: 0, covered: false, device: true,
            saved_frame: nvf_play::pipboy::Frame::Device, saved_page: None, drawn: false,
        });
        world.insert_resource(pip);

        // Consecutive cold-warm frames: NoFrustumCulling must NEVER flicker off on even/odd frames,
        // and the hidden warm pose must apply the Pip-Boy clip (x = 42.0) despite last_dt == 0.0.
        for step in 0..4 {
            app.update();
            assert!(app.world().get::<NoFrustumCulling>(warm_mesh).is_some(), "warm frame {step} lost NoFrustumCulling");
            assert!(app.world().get::<WarmUnculled>(warm_mesh).is_some(), "warm frame {step} lost WarmUnculled");
            assert!(app.world().get::<NoFrustumCulling>(pre_unculled).is_some(), "pre-existing NoFrustumCulling preserved");
            assert!(app.world().get::<WarmUnculled>(pre_unculled).is_none(), "pre-existing NoFrustumCulling not tagged WarmUnculled");
            assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);
            let posed_x = app.world().get::<Transform>(bone).unwrap().translation.x;
            assert!((posed_x - 42.0).abs() < 1e-4, "warm frame {step} did not pose clip unsmoothed: x={posed_x}");
        }

        // Animator seam history must still hold the pre-warm pose (10.0) when evaluated at dt == 0.
        {
            let mut pb = app.world_mut().resource_mut::<PlayerBody>();
            let first = pb.body.as_mut().unwrap().first.as_mut().unwrap();
            first.animator.pose();
            assert!((first.animator.locals[1].translation.x - 10.0).abs() < 1e-4, "warm pose corrupted seam history");
        }

        // Abort mid-warm (camera lost): draw_warm_pose cleans up WarmUnculled immediately.
        app.world_mut().despawn(cam_entity);
        app.update();
        assert!(app.world().get::<NoFrustumCulling>(warm_mesh).is_none(), "abort cleans up borrowed NoFrustumCulling");
        assert!(app.world().get::<WarmUnculled>(warm_mesh).is_none());
        assert!(app.world().get::<NoFrustumCulling>(pre_unculled).is_some(), "abort keeps pre-existing NoFrustumCulling");
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
        assert_eq!(app.world().get::<Transform>(outer).unwrap().translation, Vec3::new(1.0, 2.0, 3.0));
        assert!(!app.world().resource::<nvf_play::pipboy::PipBoy>().pose_hidden);
    }

    #[test]
    fn show_rigs_restores_desynced_first_person_visibility() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let third_root = world.spawn(Visibility::Inherited).id();
        let first_root = world.spawn(Visibility::Hidden).id();
        let skel = Arc::new(PreparedSkeleton::default());
        let make_rig = |root: Entity, view: View, shown: Option<bool>| Rig {
            view, root, lens: None, folder: String::new(), skeleton: String::new(),
            animator: Animator::new(skel.clone(), vec![root], None),
            clips: HashMap::new(), variants: HashMap::new(), overrides: Arc::default(), active_clip: None, head: None,
            eye_local: Vec3::ZERO, looking: None, camera: None, script_turn: None, posed_root: None,
            script_used: None, translate: None, weapon_node: None, weapon_parent: None, holstered_weapon: None, furniture_logged: None, attached: None,
            weapon_model: None, weapon_emitter: None, weapon_parts: Vec::new(), idle_logged: false,
            sights_turned: None, muzzle: None, muzzle_entity: None, casing: None, casing_entity: None,
            sighting: None, scope_eye: None, use_lens: false, sight_relief: None, drawn: Vec::new(),
            spawned_forms: Vec::new(), shown, seen: None, furniture: None,
            since_attack: f32::INFINITY, attack_loop: None, wound_up: None, wound_hit: None,
            sighting2: None, attack_eject: None, ejected: false, carry: None,
        };
        let pb = PlayerBody {
            body: Some(Body {
                worn: Vec::new(), look: None, change: fnv_actor::Change::default(),
                female: false, scale: 1.0,
                third: make_rig(third_root, View::Third, None),
                // Cached as shown == Some(true), yet ECS Visibility was externally hidden.
                first: Some(make_rig(first_root, View::First, Some(true))),
                first_addons: Vec::new(),
            }),
            ..Default::default()
        };
        world.insert_resource(pb);
        let seen = Seen { body: false, head: true, arms: true, weapon: true };
        world.run_system_once(move |mut pb: ResMut<PlayerBody>, mut commands: Commands, mut vis: Query<&mut Visibility>| {
            show_rigs(pb.body.as_mut().unwrap(), &mut commands, &mut vis, seen, true, false);
        }).unwrap();
        assert_eq!(*world.get::<Visibility>(first_root).unwrap(), Visibility::Inherited);
        assert!(world.resource::<PlayerBody>().first_person_drawn());
        // Hiding hides cleanly; re-showing shows again.
        world.run_system_once(move |mut pb: ResMut<PlayerBody>, mut commands: Commands, mut vis: Query<&mut Visibility>| {
            show_rigs(pb.body.as_mut().unwrap(), &mut commands, &mut vis, seen, false, false);
        }).unwrap();
        assert_eq!(*world.get::<Visibility>(first_root).unwrap(), Visibility::Hidden);
        assert!(!world.resource::<PlayerBody>().first_person_drawn());
    }

    #[test]
    fn the_raised_pip_boy_hides_the_bodys_arms_in_first_person() {
        // Weapon away, Pip-Boy down: the body's own arms are seen.
        assert!(Seen::upright(false, false, false).arms);
        // Weapon away, Pip-Boy up: the arms rig's raised arm stands in for them (#1523).
        assert_eq!(Seen::upright(false, false, true), Seen { body: true, head: false, arms: false, weapon: true });
        // Weapon out or moving, as before.
        assert_eq!(Seen::upright(true, true, false), Seen { body: true, head: false, arms: false, weapon: false });
        assert!(!Seen::upright(false, true, false).arms);
    }

    #[test]
    fn effect_bones_follow_their_branch_and_unknown_bones_follow_the_body() {
        let skel = PreparedSkeleton {
            names: ["Bip01", "Bip01 Head", "eye", "Bip01 L UpperArm", "finger", "Bip01 R UpperArm", "hand", "extra"].map(|n| Some(n.to_string())).to_vec(),
            parents: vec![None, Some(0), Some(1), Some(0), Some(3), Some(0), Some(5), None],
            ..default()
        };
        for (bone, part) in [(0, EffectPart::Body), (1, EffectPart::Head), (2, EffectPart::Head), (3, EffectPart::Arms), (4, EffectPart::Arms), (5, EffectPart::Arms), (6, EffectPart::Arms), (7, EffectPart::Body), (99, EffectPart::Body)] {
            assert_eq!(effect_bone_part(&skel, bone), part);
        }
    }

    #[test]
    fn effect_addons_follow_first_person_headless_and_third_person_parts() {
        let parts = [EffectPart::Body, EffectPart::Head, EffectPart::Arms];
        let first = Seen { body: false, head: true, arms: true, weapon: true };
        assert_eq!(parts.map(|p| first.effect_part(p)), [false, false, false]);
        let headless = Seen { body: true, head: false, arms: false, weapon: false };
        assert_eq!(parts.map(|p| headless.effect_part(p)), [true, false, false]);
        let third = Seen { body: true, head: true, arms: true, weapon: true };
        assert_eq!(parts.map(|p| third.effect_part(p)), [true, true, true]);
        assert!(!PlayerBody::default().sees_effect(EffectPart::Body));
    }

    #[test]
    fn shade_casters_near_the_line_to_the_sun() {
        use bevy::camera::primitives::Aabb;
        let eye = Vec3::new(100.0, 120.0, 0.0);
        let up = Vec3::Y;
        let low = Vec3::new(1.0, 0.25, 0.0).normalize();
        let small = Aabb::from_min_max(Vec3::splat(-10.0), Vec3::splat(10.0));
        // Land 120 units below the eye, 4,096 units square: never near the line.
        let land = Aabb::from_min_max(Vec3::new(-2048.0, -5.0, -2048.0), Vec3::new(2048.0, 5.0, 2048.0));
        let ground = GlobalTransform::from_translation(Vec3::new(100.0, 0.0, 0.0));
        assert!(!near_line(&land, &ground, eye, up));
        assert!(!near_line(&land, &ground, eye, low));
        // A roof 80 units over the head with the sun overhead; not with a low sun off to the side.
        let roof = GlobalTransform::from_translation(eye + up * 80.0);
        assert!(near_line(&small, &roof, eye, up));
        assert!(!near_line(&small, &roof, eye, Vec3::new(0.0, 0.0, 1.0)));
        // A box beside the eye (the arms), and one 1,000 units off.
        assert!(near_line(&small, &GlobalTransform::from_translation(eye + Vec3::X * 50.0), eye, up));
        assert!(!near_line(&small, &GlobalTransform::from_translation(eye + Vec3::X * 1000.0), eye, low));
        // Scale counts: the far box scaled 100 times reaches the line.
        assert!(near_line(&small, &GlobalTransform::from(Transform::from_translation(eye + Vec3::X * 1000.0).with_scale(Vec3::splat(100.0))), eye, low));
    }

    #[test]
    fn head_slots() {
        // Head, Hair, Headband, Hat, Eye Glasses, Nose Ring, Earrings, Mask, Mouth Object.
        for b in [0, 1, 9, 10, 11, 12, 13, 14, 16] {
            assert!(HEAD_SLOTS & 1 << b != 0, "bit {b}");
        }
        // Upper body, hands, weapon, Pip-Boy, backpack, necklace, choker.
        for b in [2, 3, 4, 5, 6, 7, 8, 15] {
            assert!(HEAD_SLOTS & 1 << b == 0, "bit {b}");
        }
    }

    #[test]
    fn a_seated_view_turns_as_far_as_a_neck() {
        let deg = |d: f32| d.to_radians().rem_euclid(std::f32::consts::TAU);
        let close = |a: f32, b: f32| ((a - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI).abs() < 1e-4;
        // Within reach the view is the player's, either side, across north too.
        let mut settled = false;
        for h in [350.0, 10.0, 80.0, 290.0] {
            assert_eq!(seated_heading(deg(h), deg(10.0), &mut settled, 0.016), (deg(h), true), "{h}");
        }
        assert!(settled);
        // Once there, the mouse takes it no further than the limit.
        let (h, within) = seated_heading(deg(150.0), deg(10.0), &mut settled, 0.016);
        assert!(close(h, deg(90.0)) && within, "{h}");
        let (h, within) = seated_heading(deg(200.0), deg(10.0), &mut settled, 0.016);
        assert!(close(h, deg(290.0)) && within, "{h}");
        // Sitting down facing the chair: the view comes round at the return rate, and
        // is out of reach until it has.
        let mut settled = false;
        let (h, within) = seated_heading(deg(170.0), deg(10.0), &mut settled, 0.1);
        assert!(close(h, deg(170.0) - 0.1 * NECK_RETURN_RATE) && !within && !settled, "{h}");
        let mut h = deg(170.0);
        let mut frames = 0;
        while !seated_heading(h, deg(10.0), &mut settled, 0.016).1 {
            h = seated_heading(h, deg(10.0), &mut settled, 0.016).0;
            frames += 1;
            assert!(frames < 100);
        }
        assert!(settled && frames > 10);
    }

    #[test]
    fn a_view_lies_down_with_its_body() {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        let close = |a: Vec3, b: Vec3| (a - b).length() < 1e-3;
        // Standing, facing east: the view is the player's, level.
        let standing = Quat::from_rotation_z(-FRAC_PI_2);
        let mut settled = false;
        let (v, [forward, up], within) = lying_view(None, standing, FRAC_PI_2 + 0.3, -0.2, [0.0; 2], &mut settled, 0.016);
        assert!((v.turn - 0.3).abs() < 1e-4 && (v.tilt + 0.2).abs() < 1e-4 && within, "{v:?}");
        assert!(close(forward, view_axes(FRAC_PI_2 + 0.3, -0.2).0) && close(up, view_axes(FRAC_PI_2 + 0.3, -0.2).1));
        assert!((v.wrote.0 - (FRAC_PI_2 + 0.3)).abs() < 1e-4 && (v.wrote.1 + 0.2).abs() < 1e-4, "{v:?}");

        // The body lies back, its head to the west: a view straight ahead looks at the
        // ceiling, the top of the picture towards the top of the head. The level view
        // nearest to it looks east, the way the feet point, as far up as a view goes.
        let on_back = standing * Quat::from_rotation_x(FRAC_PI_2);
        assert!(close(on_back * Vec3::Y, Vec3::Z) && close(on_back * Vec3::Z, -Vec3::X));
        let ahead = LyingView { turn: 0.0, tilt: 0.0, wrote: (FRAC_PI_2, 0.0) };
        let (v, [forward, up], _) = lying_view(Some(ahead), on_back, FRAC_PI_2, 0.0, [0.0; 2], &mut settled, 0.016);
        assert!(close(forward, Vec3::Z) && close(up, -Vec3::X), "{forward} {up}");
        assert!((v.wrote.0 - FRAC_PI_2).abs() < 1e-3 && v.wrote.1 == MAX_PITCH, "{v:?}");
        // The mouse turns the head in the body's frame, also where a level view is
        // already as far up as it goes: to the body's right is south here.
        let (heading, pitch) = look_after(v, [0.5, 0.25]);
        let (v, [forward, _], _) = lying_view(Some(v), on_back, heading, pitch, [0.5, 0.25], &mut settled, 0.016);
        assert!((v.turn - 0.5).abs() < 1e-4 && (v.tilt - 0.25).abs() < 1e-4, "{v:?}");
        assert!(forward.y < -0.3 && forward.z > 0.5 && forward.x < -0.1, "{forward}");
        // No further than a neck turns, and not down into the body.
        let (heading, pitch) = look_after(v, [2.0, -3.0]);
        let (v, _, within) = lying_view(Some(v), on_back, heading, pitch, [2.0, -3.0], &mut settled, 0.016);
        assert!((v.turn - NECK_TURN).abs() < 1e-4 && v.tilt >= -NECK_DOWN - 1e-4 && within, "{v:?}");
        // A heading and pitch set from outside are taken in game space: north and a
        // little up is to the body's left.
        let (v, [forward, _], _) = lying_view(Some(v), on_back, 0.0, 0.3, [0.0; 2], &mut settled, 0.016);
        assert!(close(forward, view_axes(0.0, 0.3).0) && v.turn < -1.0, "{v:?} {forward}");

        // On its right side (its right is down): the head turns up to the ceiling but
        // not into the pillow.
        let on_side = on_back * Quat::from_rotation_z(-FRAC_PI_2);
        assert!(close(on_side * Vec3::X, -Vec3::Z), "{}", on_side * Vec3::X);
        let mut settled = true;
        for (turn, tilt) in [(1.2, 0.0), (0.8, -0.9), (0.6, 0.7), (-1.3, 0.2), (0.0, 0.0)] {
            let last = LyingView { turn: 0.0, tilt: 0.0, wrote: (0.0, 0.0) };
            let (v, [forward, up], _) = lying_view(Some(last), on_side, turn, tilt, [turn, tilt], &mut settled, 0.016);
            assert!(forward.z >= -LYING_DOWN.sin() - 1e-3, "{turn} {tilt}: {forward}");
            assert!((forward.length() - 1.0).abs() < 1e-3 && forward.dot(up).abs() < 1e-3);
            // Looking up is left alone.
            assert!(turn > 0.0 || ((v.turn - turn).abs() < 1e-4 && (v.tilt - tilt).abs() < 1e-4), "{turn} {tilt}: {v:?}");
            assert!((0.0..TAU).contains(&v.wrote.0) && v.wrote.1.abs() <= MAX_PITCH && v.turn.abs() <= PI);
        }
    }

    /// The heading and pitch the mouse leaves the player with after `look`.
    fn look_after(v: LyingView, look: [f32; 2]) -> (f32, f32) {
        ((v.wrote.0 + look[0]).rem_euclid(std::f32::consts::TAU), (v.wrote.1 + look[1]).clamp(-MAX_PITCH, MAX_PITCH))
    }

    #[test]
    fn a_level_view_is_found_for_a_leaning_one() {
        use std::f32::consts::FRAC_PI_2;
        // A level view is its own.
        for (h, p) in [(0.3, 0.2), (4.0, -1.0), (2.0, 1.3)] {
            let (forward, up) = view_axes(h, p);
            let (heading, pitch) = view_angles(forward, up, 0.0);
            assert!((heading - h).abs() < 1e-4 && (pitch - p).abs() < 1e-4, "{h} {p}");
        }
        // Straight up, the top of the picture to the west: the view is turned east.
        let (heading, pitch) = view_angles(Vec3::Z, -Vec3::X, 0.0);
        assert!((heading - FRAC_PI_2).abs() < 1e-3 && pitch == MAX_PITCH);
        // Level, the picture on its side: the heading is the view's own.
        let (heading, pitch) = view_angles(Vec3::X, Vec3::Y, 0.0);
        assert!((heading - FRAC_PI_2).abs() < 1e-4 && pitch.abs() < 1e-4);
    }

    #[test]
    fn default_eye_is_in_front_of_and_above_the_head() {
        let skel = PreparedSkeleton { bind_world: vec![Affine3A::from_translation(Vec3::new(0.0, 0.0, 110.0))], ..Default::default() };
        assert_eq!(default_eye(&skel, 0), Vec3::new(0.0, 9.0, 7.0));
    }

    #[test]
    fn an_attack_clip_goes_round_its_repeated_part() {
        // 2hhattackspin.kf: repeated from `Fire` at 0.967 s to `Loop` at 1.633 s.
        let (start, end) = (0.967, 1.633);
        assert_eq!(loop_time(0.5, start, end), 0.5);
        assert_eq!(loop_time(1.6, start, end), 1.6);
        assert!((loop_time(1.633, start, end) - start).abs() < 1e-5);
        assert!((loop_time(1.733, start, end) - (start + 0.1)).abs() < 1e-5);
        // A long frame, or a shot during the way out: back inside the repeated part.
        let t = loop_time(2.4, start, end);
        assert!((start..end).contains(&t), "{t}");
        // No repeated part to go round.
        assert_eq!(loop_time(3.0, 2.5, 2.5), 3.0);
    }

    fn player_refresh_fixture() -> (App, crate::outfits::tests::TestFiles) {
        let (mut app, files) = crate::outfits::tests::asset_fixture();
        {
            let mut world = app.world_mut().resource_mut::<ActorWorld>();
            let mut npc = world.actors.npcs[&crate::outfits::tests::OTHER].clone();
            npc.id = FormId::PLAYER_BASE;
            world.actors.npcs.insert(FormId::PLAYER_BASE, npc);
        }
        let forms: &'static fnv_sim::FormStore = Box::leak(Box::new(crate::outfits::tests::actor_forms(vec![])));
        let refs: &'static fnv_sim::RefTable = Box::leak(Box::default());
        let mut sim = fnv_sim::World::new(forms, refs, Default::default());
        sim.get(FormId::PLAYER_REF).unwrap().inventory = Some(crate::outfits::tests::equipped());
        app.insert_resource(SimState { forms, refs, world: sim })
            .insert_resource(nvf_play::script::ScriptState::over_plugins(&[]))
            .init_resource::<PlayerBody>()
            .init_resource::<crate::death_cam::DeathCam>()
            .init_resource::<crate::player_knockdown::PlayerKnockdown>()
            .init_resource::<Time<Real>>();
        app.world_mut().spawn(WorldRoot);
        app.add_systems(Update, (ensure_body, apply_anim_overrides).chain());
        // Initial frame builds the player body and weapon model.
        app.update();
        assert!(app.world().resource::<PlayerBody>().body.is_some(), "player body built");
        assert!(
            app.world().resource::<PlayerBody>().body.as_ref().unwrap().third.weapon_model.is_some(),
            "player weapon model built"
        );
        (app, files)
    }

    #[test]
    fn update_player_3d_rebuilds_body_and_weapon_bypassing_look_throttle_and_clears_anim_cache() {
        use fnv_game::game::RunCtx;
        use fnv_script::vm::XVal;
        let (mut app, _files) = player_refresh_fixture();
        let before_root = app.world().resource::<PlayerBody>().body.as_ref().unwrap().third.root;
        let before_weapon = app.world().resource::<PlayerBody>().body.as_ref().unwrap().third.weapon_model.unwrap();

        // Real script call at elapsed Real time 0.0 (within LOOK_REBUILD cooldown).
        assert_eq!(
            app.world_mut()
                .resource_mut::<nvf_play::script::ScriptState>()
                .rt
                .as_mut()
                .unwrap()
                .game
                .run_query(
                    "Begin Function {}\nSetFunctionValue (UpdatePlayer3D)\nEnd",
                    RunCtx::default(),
                ),
            Ok(XVal::Num(0.0))
        );
        assert!(app.world().resource::<nvf_play::script::ScriptState>().rt.as_ref().unwrap().game.update_player_3d_pending());

        app.update();
        let body = app.world().resource::<PlayerBody>().body.as_ref().unwrap();
        let after_root = body.third.root;
        let after_weapon = body.third.weapon_model.unwrap();
        assert_ne!(after_root, before_root, "UpdatePlayer3D rebuilds player rig immediately");
        assert_ne!(after_weapon, before_weapon, "UpdatePlayer3D respawns equipped weapon model");
        assert!(app.world().get_entity(before_root).is_err(), "old rig despawned");
        assert!(!app.world().resource::<nvf_play::script::ScriptState>().rt.as_ref().unwrap().game.update_player_3d_pending());
        assert!(!app.world().resource::<PlayerBody>().force_anim_refresh, "anim refresh flag consumed by apply_anim_overrides");

        // Next frame: strictly one-shot, no second rebuild.
        app.update();
        let body2 = app.world().resource::<PlayerBody>().body.as_ref().unwrap();
        assert_eq!(body2.third.root, after_root);
        assert_eq!(body2.third.weapon_model, Some(after_weapon));
    }

    #[test]
    fn reload_equipped_models_weapon_slot_respawns_player_weapon_without_rebuilding_body() {
        use fnv_game::game::RunCtx;
        use fnv_script::vm::XVal;
        let (mut app, _files) = player_refresh_fixture();
        let before_root = app.world().resource::<PlayerBody>().body.as_ref().unwrap().third.root;
        let before_weapon = app.world().resource::<PlayerBody>().body.as_ref().unwrap().third.weapon_model.unwrap();

        assert_eq!(
            app.world_mut()
                .resource_mut::<nvf_play::script::ScriptState>()
                .rt
                .as_mut()
                .unwrap()
                .game
                .run_query(
                    "Begin Function {}\nSetFunctionValue (ReloadEquippedModels 5)\nEnd",
                    RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() },
                ),
            Ok(XVal::Num(0.0))
        );

        app.update();
        let body = app.world().resource::<PlayerBody>().body.as_ref().unwrap();
        assert_eq!(body.third.root, before_root, "slot 5 reload keeps body skeleton intact");
        let after_weapon = body.third.weapon_model.unwrap();
        assert_ne!(after_weapon, before_weapon, "slot 5 reload respawns weapon model");
        assert!(app.world().get_entity(before_weapon).is_err(), "old weapon model despawned");

        // Next frame: strictly one-shot.
        app.update();
        assert_eq!(app.world().resource::<PlayerBody>().body.as_ref().unwrap().third.weapon_model, Some(after_weapon));
    }

    #[test]
    fn reload_equipped_models_armor_slot_rebuilds_player_body_and_weapon_one_shot() {
        use fnv_game::game::RunCtx;
        use fnv_script::vm::XVal;
        let (mut app, _files) = player_refresh_fixture();
        let before_root = app.world().resource::<PlayerBody>().body.as_ref().unwrap().third.root;
        let before_weapon = app.world().resource::<PlayerBody>().body.as_ref().unwrap().third.weapon_model.unwrap();

        assert_eq!(
            app.world_mut()
                .resource_mut::<nvf_play::script::ScriptState>()
                .rt
                .as_mut()
                .unwrap()
                .game
                .run_query(
                    "Begin Function {}\nSetFunctionValue (ReloadEquippedModels 2)\nEnd",
                    RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() },
                ),
            Ok(XVal::Num(0.0))
        );

        app.update();
        let body = app.world().resource::<PlayerBody>().body.as_ref().unwrap();
        let after_root = body.third.root;
        let after_weapon = body.third.weapon_model.unwrap();
        assert_ne!(after_root, before_root, "armour slot reload rebuilds player body");
        assert_ne!(after_weapon, before_weapon, "rebuilt body respawns weapon model");

        // Next frame: strictly one-shot.
        app.update();
        assert_eq!(app.world().resource::<PlayerBody>().body.as_ref().unwrap().third.root, after_root);
    }

    #[test]
    fn refresh_anim_data_clears_player_rig_clips_and_variants_one_shot() {
        use fnv_game::game::RunCtx;
        use fnv_script::vm::XVal;
        let (mut app, _files) = player_refresh_fixture();
        // Seed cached clip & variant entries on the third-person rig.
        {
            let mut pb = app.world_mut().resource_mut::<PlayerBody>();
            let rig = &mut pb.body.as_mut().unwrap().third;
            rig.clips.insert(ClipKey::Idle(Stance::Stand, false, false), None);
            rig.variants.insert("test_up.kf".into(), None);
        }

        // Steady frame without RefreshAnimData leaves cached clips intact.
        app.update();
        {
            let pb = app.world().resource::<PlayerBody>();
            let rig = &pb.body.as_ref().unwrap().third;
            assert_eq!(rig.clips.len(), 1);
            assert_eq!(rig.variants.len(), 1);
        }

        // Script calls RefreshAnimData on PlayerREF.
        assert_eq!(
            app.world_mut()
                .resource_mut::<nvf_play::script::ScriptState>()
                .rt
                .as_mut()
                .unwrap()
                .game
                .run_query(
                    "Begin Function {}\nSetFunctionValue (RefreshAnimData)\nEnd",
                    RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() },
                ),
            Ok(XVal::Num(0.0))
        );

        // Next frame: apply_anim_overrides consumes the one-shot flag and clears caches.
        app.update();
        {
            let pb = app.world().resource::<PlayerBody>();
            let rig = &pb.body.as_ref().unwrap().third;
            assert!(rig.clips.is_empty(), "RefreshAnimData clears cached player clips");
            assert!(rig.variants.is_empty(), "RefreshAnimData clears cached player variants");
        }
        assert!(
            !app.world()
                .resource::<nvf_play::script::ScriptState>()
                .rt
                .as_ref()
                .unwrap()
                .game
                .anim_refresh_pending(FormId::PLAYER_REF)
        );

        // Re-populate cache and run another frame: must NOT clear again (one-shot).
        {
            let mut pb = app.world_mut().resource_mut::<PlayerBody>();
            let rig = &mut pb.body.as_mut().unwrap().third;
            rig.clips.insert(ClipKey::Idle(Stance::Stand, false, false), None);
        }
        app.update();
        assert_eq!(app.world().resource::<PlayerBody>().body.as_ref().unwrap().third.clips.len(), 1);
    }
}

#[cfg(test)]
#[path = "movement_camera_tests.rs"]
mod movement_camera_tests;

#[cfg(test)]
mod sequence_consumer_tests;

#[cfg(test)]
#[path="player_body/arm_gesture_tests.rs"]
mod arm_gesture_tests;
