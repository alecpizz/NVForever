//! What is seen of the player's shots (docs/spec-notes/combat-rules.md §15): the PROJ's
//! muzzle flash at the weapon's muzzle and the PROJ's model on every projectile in
//! flight. Combat (`nvf_play::combat`) says what was fired ([`ShotFired`]) and where
//! each projectile is drawn ([`ShotVisuals`]); the models and the light come from here.
//!
//! - **Muzzle flash.** A PROJ with the Muzzle Flash flag has a flash model (`NAM1`), a
//!   duration and a light (LIGH) [documented: xEdit PROJ, GECK wiki Projectile]. The
//!   model is spawned under the weapon model's `ProjectileNode` in the rig in view
//!   (`PlayerBody::muzzle_entity`), so it sits at the muzzle, points the way the node
//!   does and moves with the gun [hypothesis: that the game attaches it there, from the
//!   node's name, its place and its y axis lying along the barrel as the flash models'
//!   length does; animation.md §9.9]. It stays for the PROJ's duration and no longer
//!   (0.04 s for the bullet PROJs), but always for the frame it is made in, so a flash
//!   shorter than a frame is still seen once; the light with it. At the first-person
//!   weapon's muzzle it is drawn in front of the world as the weapon is
//!   ([`first_person_model`], docs/design/first-person-depth.md). Another actor's flash
//!   goes under its drawn weapon's muzzle marker (`weapons::Muzzle`, the same node of
//!   its weapon model, at rest) the same way; without one (a weapon model with no
//!   `ProjectileNode`) it stands where the shot left, its +y turned along the shot
//!   [design]. Creatures fire no guns yet (`npc_combat`: their attacks are blows).
//! - **Projectiles.** The PROJ's model is drawn at each [`ShotVisuals`] entry with its
//!   +y along the flight [observed: the models are built along +y; hypothesis: that it
//!   is their forward axis]. A beam's model is stretched along y from the muzzle to its
//!   end by the place of its `BeamEnd` node [hypothesis: the node's meaning, from its
//!   name and its place at the far end of the beam's geometry].
//! - **Billboards.** A `NiBillboardNode` of these models turns to the camera each
//!   frame (`crate::world::face_camera`, with placed models' ones; the turn per mode in
//!   docs/spec-notes/billboard-nodes.md).
//! - **Add-on nodes.** A `BSValueNode` of these models becomes an emitter of its ADDN's
//!   particle effect for as long as the model is there (`crate::particles`): the sparks
//!   and smoke of a flash, a missile's trail, the flamer's flame.
//! - **Impacts.** A shot that lands ([`ImpactFx`]) shows its IPCT's model at the point
//!   for the IPCT's effect duration, its +z along the IPCT's orientation: the surface's
//!   normal, the projectile's direction or its reflection [documented: the IPCT fields;
//!   hypothesis: that +z is the axis turned, from the impact models' add-on nodes
//!   standing along +z].
//! - **Shell casings.** [`CasingFx`]: the WEAP's casing model, falling and bouncing; a
//!   bounce from at least [`CASING_KNOCK`] units/s plays the casing's PHY sound
//!   ([`casing_sound`]) where it lands, up to [`CASING_SOUNDS`] times a casing [design].
//!   A casing is drawn only: it has no collider, and its path is traced against the
//!   still world alone ([`casing_filter`]), so it never lands on the player or anyone
//!   else and nothing (the camera, movement, shots) ever meets it (the owner,
//!   2026-10-22).
//! - **Explosions.** [`ExplosionFx`]: the EXPL's model at the point, emitting
//!   its own particle systems (flash, sparks, smoke, shrapnel: `crate::particles::
//!   ModelEmitter`) for as long as its sequence runs (else [`EXPLOSION_SECS`]), and the
//!   EXPL's light (LIGH) fading out over [`EXPLOSION_LIGHT_SECS`] [design: the LIGH's
//!   time is -1, no duration of its own]. The model stands upright when the EXPL has
//!   "Always Uses World Orientation", else its +z along the normal of the ground under
//!   the blast ([`explosion_rotation`]) [documented: the xEdit flag name; hypothesis:
//!   that the other orientation is the surface's, the audit's reading, and that the
//!   ground below stands for the surface hit]. The dirt thrown up is the impact of the EXPL's
//!   data set on the ground below (`nvf_play::combat`), shown as any impact. Seen from
//!   within the EXPL's image-space radius, its image space modifier plays on the screen
//!   (`crate::imagespace::ScreenModifiers::play`).
//! - **The models' own animation** (docs/spec-notes/explosion-effects.md): a flash,
//!   impact, explosion or projectile model plays its effect clip from when it appears
//!   (`nvf_assets::model::Rig::effect`): its `SpecialIdle_AreaEffect` sequence and the
//!   controllers no sequence drives, so fireballs and dust splashes grow, move, fade and
//!   show and hide as their files say ([`spawn_effect`]), and the energy beams scroll
//!   their textures along and across themselves (combat-rules.md §15).
//! - **Beam fade.** A beam is drawn at [`ShotVisual::strength`]: the alpha of every
//!   shape of its model (and with it all it adds, the beams being additive) falls from
//!   full to nothing over the PROJ's Fade Duration (`nvf_play::combat::beam_strength`),
//!   unless the model fades itself with an alpha controller (the green laser's, the
//!   pulse gun's) [hypothesis, combat-rules.md §15]. A beam model with no controllers
//!   at all (`DLC03ScatterBeam`'s) has no rig and is not faded.
//! - **Heat haze.** The refracting shapes of these models (the shells of the fireballs:
//!   the Incinerator's, the missile's, the big fire explosions') bend what lies behind
//!   them instead of being drawn (`nvf_assets::nif_material`, "Refraction"); their
//!   `BSRefractionStrengthController` keys play with the effect clip.
//! - **Impact decals.** The decal an impact leaves is drawn by `crate::decals`.

use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::prelude::*;
use fnv_phys::{layers, Filter};
use nvf_assets::anim_play::MaterialTarget;
use nvf_assets::material_anim::RigMaterials;
use nvf_assets::model::{Billboard, ModelCtx, PreparedModel};
use nvf_assets::nif_material::NifMaterial;
use nvf_play::combat::{CasingFx, Combat, ExplosionFx, ExplosionLook, ImpactFx, ShotFired, ShotVisual, ShotVisuals, WATER_SURFACE_EXPLOSION};
use nvf_play::phys::PhysState;
use fnv_sim::FormId;
use nvf_play::sfx::{PlaySound, Sfx};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::light_shadows::{ShadowSource, SourceKind};
use crate::player_body::PlayerBody;
use crate::weapons::{HeldWeapon, Muzzle};
use crate::world::{WorldData, WorldRoot};


/// A shell casing in the air or on the ground: its velocity and spin (game space) and
/// how long it has left.
#[derive(Component)]
struct Casing {
    velocity: Vec3,
    spin: Vec3,
    left: f32,
    resting: bool,
    /// Its knock (a PHYCasing SOUN) and how many times it has sounded.
    sound: Option<FormId>,
    knocks: u8,
}

/// The speed at which a casing's bounce is heard, units/s, and how many bounces of one
/// casing are [design].
pub const CASING_KNOCK: f32 = 60.0;
pub const CASING_SOUNDS: u8 = 3;

/// The PHY sound (SOUN editor ID) of a shell casing by its model's name [hypothesis: by
/// the names; FalloutNV.esm has `PHYCasingPistol`, `PHYCasingRifle` and
/// `PHYCasingShotgun` (files `fx\phy\weapon\casing_<kind>\`) and no record links them to
/// a weapon or a casing model, audio.md §14].
pub fn casing_sound(model: &str) -> &'static str {
    let m = model.to_ascii_lowercase();
    if m.contains("shotgun") {
        "PHYCasingShotgun"
    } else if ["45cal", "357", "44magnum", "10mm", "9mm", "pistol", "eccell"].iter().any(|k| m.contains(k)) {
        "PHYCasingPistol"
    } else {
        "PHYCasingRifle"
    }
}

/// The share of its speed a casing keeps when it bounces, and the speed below which it
/// stays down, units per second [design].
const CASING_BOUNCE: f32 = 0.35;
const CASING_REST: f32 = 25.0;

/// What a casing falls onto: terrain and the still collision of references, never an
/// actor (capsules, hit volumes, ragdolls), a moving body or a layer the character
/// passes through. A casing thrown from the weapon starts inside the shooter's hit
/// volume, and came to rest on it in the air where the player had stood.
pub fn casing_filter() -> Filter {
    Filter { no_actors: true, no_dynamic: true, exclude_layers: layers::character_ignores(), ..Default::default() }
}

/// A muzzle flash or an impact effect and how long it has left.
#[derive(Component)]
struct Flash {
    left: f32,
}

/// A muzzle flash's light: its full brightness and the flash's length, lit along
/// [`flash_light_share`] as the flash ages.
#[derive(Component)]
struct FlashLight {
    full: f32,
    life: f32,
    age: f32,
}

/// The share of its full brightness a muzzle flash's light has `age` seconds into a
/// flash of `life` seconds: up over the first [`FLASH_LIGHT_RISE`] of it and down over the
/// rest, both eased [design, #3785: the light was on at full strength for the flash's
/// time and off at once; ED-E's laser (MuzzleLaserRed300, a red light of radius 300 for
/// 0.14 s) flashed the night scene once a second, hard on and off, which the player saw as
/// a strobe. The LIGH's Flicker flag is not played].
fn flash_light_share(age: f32, life: f32) -> f32 {
    if life <= 0.0 {
        return 0.0;
    }
    let x = (age / life).clamp(0.0, 1.0);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    if x < FLASH_LIGHT_RISE {
        smooth(x / FLASH_LIGHT_RISE)
    } else {
        1.0 - smooth((x - FLASH_LIGHT_RISE) / (1.0 - FLASH_LIGHT_RISE))
    }
}

/// The share of a muzzle flash's time its light takes to come up ([`flash_light_share`]).
const FLASH_LIGHT_RISE: f32 = 0.2;

/// How long an explosion's model stays and emits when it has no sequence, and how long
/// its light takes to fade, seconds [design: about the grenade explosion's sequence,
/// 5.03 s, and its flash's emitter, on for 0.33 s]. A model with a sequence stays for its
/// length [hypothesis: the game removes the effect when its sequence has run].
pub const EXPLOSION_SECS: f32 = 5.0;
pub const EXPLOSION_LIGHT_SECS: f32 = 0.6;

/// An explosion's light: its full intensity and how long it has been fading.
#[derive(Component)]
struct FadingLight {
    full: f32,
    age: f32,
}

/// The systems that put shot effects in the world; particles are emitted after them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShotFxSet;

#[derive(Resource, Default)]
struct ShotFx {
    /// The model drawn for each projectile in flight (`ShotVisual::id`); `None`: it has
    /// none to draw. With the y its model's `BeamEnd` node is at, if it has one, and how
    /// far the model reaches behind its origin.
    flying: HashMap<u64, Option<(Entity, Option<f32>, f32)>>,
    /// Model paths that were reported.
    reported: HashSet<String>,
    /// PROJs whose flying model's heading was reported ([`draw_flying`]).
    headed: HashSet<FormId>,
    flashes: u32,
    lights: u32,
    /// Other actors' flashes: under their weapon's muzzle marker, and where the shot
    /// left (no marker).
    actor_flashes_muzzle: u32,
    actor_flashes_at_shot: u32,
    /// Weapons drawn on other actors whose flash and projectile models were loaded.
    actor_weapons_loaded: HashSet<fnv_sim::FormId>,
    projectiles: u32,
    no_model: u32,
    casings: u32,
    casing_knocks: u32,
    impacts: u32,
    /// Impacts not shown on an actor flagged No Blood Spray.
    no_spray: u32,
    explosions: u32,
    /// Explosions turned to the ground under them (no "Always Uses World Orientation").
    explosions_aligned: u32,
    /// Explosions in or by water whose own model was not drawn, and the water surfaces
    /// thrown up (`WATER_SURFACE_EXPLOSION`).
    explosions_in_water: u32,
    water_surfaces: u32,
    /// Effect models spawned playing their own clip, and of those, explosions whose
    /// image space modifier went on the screen.
    animated: u32,
    screen_effects: u32,
    /// Beams that played a clip of their own, and those drawn fading.
    beams_animated: u32,
    beams_faded: u32,
    /// Flash models as drawn at the first-person weapon's muzzle ([`first_person_model`]),
    /// by model path, and the player's flashes drawn so.
    first_person_models: HashMap<String, Arc<PreparedModel>>,
    first_person_flashes: u32,
}

pub struct ShotFxPlugin;

impl Plugin for ShotFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShotFx>()
            // The casings' knocks are sounds: before sfx plays them.
            .configure_sets(Update, ShotFxSet.before(nvf_play::sfx::SfxSet))
            .add_systems(Update, (preload, flash, draw_flying, casings, impacts, explosions).chain().in_set(ShotFxSet).after(nvf_play::PlaySet::Activation).run_if(resource_exists::<Combat>))
            .add_systems(Last, report);
    }
}

fn local(a: &Affine3A) -> Transform {
    Transform::from_matrix(Mat4::from(*a))
}

/// Spawn `pm` under `parent` at `place` as its node tree, so that its billboards can
/// turn; returns the entity above it.
fn spawn_model(commands: &mut Commands, parent: Entity, pm: &PreparedModel, place: Transform) -> Entity {
    let holder = commands.spawn((place, Visibility::Inherited, ChildOf(parent))).id();
    let nodes: Vec<Entity> = pm
        .nodes
        .iter()
        .map(|n| {
            let t = local(&n.local);
            let mut e = commands.spawn((t, Visibility::Inherited));
            if let Some(mode) = n.billboard {
                e.insert(Billboard { mode, bind: t.rotation });
            }
            if let Some(index) = n.addon {
                e.insert(crate::particles::AddonEmitter::new(index));
            }
            e.id()
        })
        .collect();
    for (n, e) in pm.nodes.iter().zip(&nodes) {
        commands.entity(n.parent.map_or(holder, |p| nodes[p])).add_child(*e);
    }
    for s in &pm.shapes {
        let e = commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(s.material.clone()), local(&s.own), Visibility::Inherited, NotShadowCaster)).id();
        commands.entity(s.node.map_or(holder, |n| nodes[n])).add_child(e);
    }
    holder
}

/// `pm` with each material replaced by a copy drawn as a first-person shape
/// (`NifSurface::first_person`, nvf_assets nif_material.rs "First-person depth"): the
/// flash at the first-person weapon's muzzle, which the squeezed weapon would hide
/// otherwise. A material shared by several shapes gets one copy.
fn first_person_model(pm: &PreparedModel, materials: &mut Assets<NifMaterial>) -> PreparedModel {
    let mut copies: HashMap<AssetId<NifMaterial>, Handle<NifMaterial>> = HashMap::new();
    let mut first_person = |m: &mut Handle<NifMaterial>| {
        let copy = copies.entry(m.id()).or_insert_with(|| match materials.get(&*m).cloned() {
            Some(mut c) => {
                c.extension.first_person = true;
                materials.add(c)
            }
            None => m.clone(),
        });
        *m = copy.clone();
    };
    let mut out = pm.clone();
    for s in &mut out.shapes {
        first_person(&mut s.material);
    }
    for s in out.rig.iter_mut().flat_map(|r| r.shapes.iter_mut()) {
        first_person(&mut s.material);
    }
    out
}

/// Spawn an effect model under `parent` at `place`: its rig playing its effect clip when
/// something in the file moves (`Rig::effect`), else its node tree ([`spawn_model`]).
/// Its billboards turn and its add-on nodes emit either way. Returns the entity above it
/// and the length of its clip in seconds (0: none). `beam`: a beam, which fades with its
/// [`ShotVisual::strength`] ([`Fade`]).
fn spawn_effect(commands: &mut Commands, fx: &mut ShotFx, parent: Entity, pm: &PreparedModel, place: Transform, source: &str, beam: bool) -> (Entity, f32) {
    let Some((rig, clip)) = pm.rig.as_ref().and_then(|r| Some((r, r.effect.as_ref()?))) else {
        return (spawn_model(commands, parent, pm, place), 0.0);
    };
    let (holder, root, drawn) = spawn_rig(commands, parent, pm, rig, clip, place, source);
    if beam {
        // Every shape gets a material of its own to fade, not only those the clip drives
        // (the laser beam's white core is not).
        let fades_itself = clip.materials.iter().any(|t| t.target == MaterialTarget::Alpha);
        if !fades_itself {
            let slots = rig.shapes.iter().zip(&drawn).map(|(s, e)| s.slot(*e)).collect();
            commands.entity(root).insert(RigMaterials { slots, idle: Arc::new(clip.materials.clone()), dark: false, fade: 1.0 });
        }
        commands.entity(holder).insert(Fade { root, own: fades_itself });
        fx.beams_animated += 1;
    }
    fx.animated += 1;
    (holder, clip.length() / clip.frequency.max(1e-6))
}

/// Spawn `rig` under a holder at `place` playing `clip`, with the billboards, add-on
/// nodes and visibility tracks of [`spawn_effect`]; returns the holder, the rig's root
/// and its drawn shapes.
fn spawn_rig(commands: &mut Commands, parent: Entity, pm: &PreparedModel, rig: &nvf_assets::model::Rig, clip: &nvf_assets::anim_play::Clip, place: Transform, source: &str) -> (Entity, Entity, Vec<Entity>) {
    // Under a holder at the place: a track of the clip may drive the file's root node.
    let holder = commands.spawn((place, Visibility::Inherited, ChildOf(parent))).id();
    let (root, drawn, by_name) = rig.spawn_playing(commands, holder, Transform::IDENTITY, source, 0.0, Some(clip));
    for e in &drawn {
        commands.entity(*e).insert(NotShadowCaster);
    }
    for n in &pm.nodes {
        // Its billboards come with the rig (`Rig::spawn_playing`).
        let Some(&e) = n.name.as_ref().and_then(|k| by_name.get(k)) else { continue };
        if let Some(index) = n.addon {
            commands.entity(e).insert(crate::particles::AddonEmitter::new(index));
        }
    }
    (holder, root, drawn)
}

/// Spawn a model another effect puts on something (an effect shader's add-on models,
/// `crate::effect_shaders`) under `parent` at `place`, as [`spawn_effect`] does: its rig
/// playing its effect clip, its billboards turning, its add-on nodes emitting. Returns
/// the entity above it and the rig root whose [`RigMaterials::fade`] fades it (`None`:
/// no rig, or its clip drives its own alpha).
pub(crate) fn spawn_attached(commands: &mut Commands, parent: Entity, pm: &PreparedModel, place: Transform, source: &str) -> (Entity, Option<Entity>) {
    let Some((rig, clip)) = pm.rig.as_ref().and_then(|r| Some((r, r.effect.as_ref()?))) else {
        return (spawn_model(commands, parent, pm, place), None);
    };
    let (holder, root, drawn) = spawn_rig(commands, parent, pm, rig, clip, place, source);
    if clip.materials.iter().any(|t| t.target == MaterialTarget::Alpha) {
        return (holder, None);
    }
    let slots = rig.shapes.iter().zip(&drawn).map(|(s, e)| s.slot(*e)).collect();
    commands.entity(root).insert(RigMaterials { slots, idle: Arc::new(clip.materials.clone()), dark: false, fade: 1.0 });
    (holder, Some(root))
}

/// Spawn a model that moves by itself, as a part of a creature (Mister Handy's jet,
/// `nvf_assets::actor::PreparedPart::plays_itself`, #2228), under `parent` at `place`: its
/// rig playing its idle clip, which holds the controllers no sequence drives, each looping
/// on its own cycle as a placed object's do (`nvf_assets::model` "free" tracks), with
/// its billboards turning and its add-on nodes emitting; else its node tree
/// ([`spawn_model`]). Returns the entity above it and the entities that carry meshes.
pub(crate) fn spawn_playing_itself(commands: &mut Commands, parent: Entity, pm: &PreparedModel, place: Transform, source: &str) -> (Entity, Vec<Entity>) {
    let Some((rig, clip)) = pm.rig.as_ref().and_then(|r| Some((r, r.idle.as_ref()?))) else {
        let holder = spawn_model(commands, parent, pm, place);
        return (holder, Vec::new());
    };
    let (holder, _, drawn) = spawn_rig(commands, parent, pm, rig, clip, place, source);
    (holder, drawn)
}

/// On a beam's holder: the root of its rig, whose materials fade with the beam's
/// strength, unless `own`: its model fades itself (an alpha track of its clip).
#[derive(Component)]
struct Fade {
    root: Entity,
    own: bool,
}

/// Draw a beam at its strength (see the module notes); the beams that faded are counted
/// once each.
fn fade_beam(fx: &mut ShotFx, m: &mut RigMaterials, v: &ShotVisual) {
    if m.fade >= 1.0 && v.strength < 1.0 {
        fx.beams_faded += 1;
    }
    if m.fade != v.strength {
        m.fade = v.strength;
    }
}

fn load(models: &mut ModelCtx, fx: &mut ShotFx, what: &str, model: &str) -> Option<Arc<PreparedModel>> {
    let path = fnv_actor::resolve::mesh_path(model);
    let pm = models.model(&path);
    if fx.reported.insert(path.clone()) {
        match &pm {
            Some(pm) => info!(
                "shot fx: {what} {path}: {} shapes, {} nodes, {} billboards, {} refracting{}",
                pm.shapes.len(),
                pm.nodes.len(),
                pm.nodes.iter().filter(|n| n.billboard.is_some()).count(),
                pm.shapes.iter().filter(|s| models.materials.get(&s.material).is_some_and(|m| m.extension.refract)).count(),
                match pm.rig.as_ref().and_then(|r| r.effect.as_ref()) {
                    Some(c) => format!(
                        "; plays {} ({:.2} s: {} node track(s), {} visibility, {} material)",
                        c.name,
                        c.length(),
                        c.tracks.len(),
                        c.visibility.len(),
                        c.materials.len()
                    ),
                    None => String::new(),
                }
            ),
            None => warn!("shot fx: {what} {path}: model not loaded"),
        }
    }
    pm
}

/// Load the flash and projectile models of the weapon in use before its first shot: a
/// model first asked for in the frame it is drawn in can miss that frame, which is the
/// whole of a 0.04 s flash. The same for each weapon drawn on another actor, once per
/// weapon: its default ammo's (a creature's body weapon too, `body_weapons.rs`).
fn preload(
    combat: Res<Combat>,
    mut models: ModelCtx,
    mut fx: ResMut<ShotFx>,
    mut last: Local<Option<fnv_sim::FormId>>,
    drawn: Query<&HeldWeapon, Added<HeldWeapon>>,
    body: Query<&crate::body_weapons::BodyWeapon, Changed<crate::body_weapons::BodyWeapon>>,
) {
    let fx = &mut *fx;
    let mut models_of = |fx: &mut ShotFx, id: fnv_sim::FormId| {
        let Some(p) = combat.forms.projectiles.get(&id) else { return };
        if let Some(m) = p.muzzle_flash_model.as_deref().filter(|_| p.has("Muzzle Flash")) {
            load(&mut models, fx, "muzzle flash", m);
        }
        if let Some(m) = p.model.as_deref() {
            load(&mut models, fx, "projectile", m);
        }
    };
    for w in drawn.iter().map(|h| h.weapon).chain(body.iter().map(|b| b.weapon)) {
        if fx.actor_weapons_loaded.insert(w) {
            if let Some(p) = combat.forms.chain(w, None).and_then(|c| c.projectile) {
                models_of(fx, p);
            }
        }
    }
    let now = combat.projectile();
    if *last == now {
        return;
    }
    *last = now;
    if let Some(p) = now {
        models_of(fx, p);
    }
}

/// Muzzle flashes: one for each shot fired, gone after their time.
#[allow(clippy::too_many_arguments)]
fn flash(
    mut commands: Commands,
    time: Res<Time>,
    mut fired: MessageReader<ShotFired>,
    combat: Res<Combat>,
    body: Option<Res<PlayerBody>>,
    world: Option<Res<WorldData>>,
    mut models: ModelCtx,
    mut fx: ResMut<ShotFx>,
    mut flashes: Query<(Entity, &mut Flash)>,
    mut flash_lights: Query<(&mut FlashLight, &mut PointLight)>,
    root: Query<Entity, With<WorldRoot>>,
    muzzles: Query<(Entity, &Muzzle, &GlobalTransform)>,
    mut told: Local<HashSet<fnv_sim::FormId>>,
    script: Option<Res<nvf_play::script::ScriptState>>,
) {
    let dt = time.delta_secs();
    for (e, mut f) in &mut flashes {
        f.left -= dt;
        if f.left <= 0.0 {
            commands.entity(e).try_despawn();
        }
    }
    for (mut f, mut l) in &mut flash_lights {
        f.age += dt;
        l.intensity = f.full * flash_light_share(f.age, f.life);
    }
    for shot in fired.read() {
        let Some(p) = combat.forms.projectiles.get(&shot.projectile) else { continue };
        let (true, Some(model)) = (p.has("Muzzle Flash"), p.muzzle_flash_model.as_deref()) else { continue };
        // The player's at the held weapon's muzzle (no body, no `ProjectileNode`: no
        // flash); another actor's at its weapon's muzzle marker, else where its shot left
        // (game space, under the world root), +y along the shot (module notes).
        let player = shot.shooter == fnv_sim::FormId::PLAYER_REF;
        let marker = (!player).then(|| muzzles.iter().find(|(_, m, _)| m.reference == shot.shooter.0)).flatten();
        let place = if player {
            body.as_deref().and_then(PlayerBody::muzzle_entity).map(|m| (m, Transform::IDENTITY))
        } else if let Some((m, _, _)) = marker {
            Some((m, Transform::IDENTITY))
        } else {
            let along = |d: [f32; 3]| Quat::from_rotation_arc(Vec3::Y, Vec3::from_array(d).normalize_or(Vec3::Y));
            shot.at.zip(root.single().ok()).map(|(at, r)| (r, Transform::from_translation(Vec3::from_array(at)).with_rotation(shot.dir.map_or(Quat::IDENTITY, along))))
        };
        let Some((parent, place)) = place else { continue };
        let Some(mut pm) = load(&mut models, &mut fx, "muzzle flash", model) else { continue };
        // At the first-person weapon's muzzle it is drawn in front of the world as the
        // weapon is (`NifSurface::first_person`), else the barrel hid it.
        if player && body.as_deref().is_some_and(PlayerBody::first_person_drawn) {
            pm = match fx.first_person_models.get(model) {
                Some(m) => m.clone(),
                None => {
                    let m = Arc::new(first_person_model(&pm, &mut models.materials));
                    fx.first_person_models.insert(model.to_string(), m.clone());
                    m
                }
            };
            fx.first_person_flashes += 1;
        }
        let (holder, _) = spawn_effect(&mut commands, &mut fx, parent, &pm, place, model, false);
        // Made after this frame's flashes were aged: it is drawn this frame whatever
        // its duration, and goes in the first frame its time is up.
        commands.entity(holder).insert((Flash { left: p.muzzle_flash_duration }, crate::particles::ModelEmitter::new(fnv_actor::resolve::mesh_path(model))));
        fx.flashes += 1;
        match (player, marker) {
            (true, _) => {}
            (false, Some(_)) => fx.actor_flashes_muzzle += 1,
            (false, None) => fx.actor_flashes_at_shot += 1,
        }
        // Once an actor: how near the flash is to where the shot left and how far its
        // length (the marker's +y, game space as the world root's turn alone) is turned
        // from the shot (the marker as last frame left it).
        if let (Some((_, _, g)), Some(at), Some(dir), true) = (marker, shot.at, shot.dir, told.insert(shot.shooter)) {
            let off = crate::world::bevy_to_game(g.translation()).distance(Vec3::from_array(at));
            let angle = crate::world::bevy_to_game(g.affine().matrix3.y_axis.into()).normalize_or_zero().angle_between(Vec3::from_array(dir).normalize_or(Vec3::Y)).to_degrees();
            info!("shot fx: {} muzzle flash at its weapon's muzzle, {off:.1} units from where the shot left, its length {angle:.0} degrees off the shot", shot.shooter);
        }
        // The flash's light: the LIGH's colour, brightness and radius; none while a
        // script's DisableMuzzleFlashLights turns this shooter's off (`fnv_game::nvse_jg10`).
        let lit = script.as_deref().and_then(|s| s.rt.as_ref()).is_none_or(|rt| rt.game.muzzle_light_on(shot.shooter));
        let light = p.muzzle_flash_light.filter(|_| lit).zip(world.as_deref()).and_then(|(id, w)| fnv_world::lighting::light_data(w.lo, w.schema, id.0));
        if let Some(l) = light.filter(|l| l.radius > 0.0) {
            let color = crate::lights::display_color(l.color.map(|b| b as f32 / 255.0));
            let intensity = crate::lights::lumens(l.fade.max(0.0), crate::sky::light_unit());
            // It casts lamp shadows when the ALC shadows row is on (`crate::light_shadows`);
            // the flash's own shapes are `NotShadowCaster` (`spawn_effect`).
            commands.entity(holder).with_child((
                PointLight { color, intensity: 0.0, range: l.radius, shadow_maps_enabled: false, ..default() },
                FlashLight { full: intensity, life: p.muzzle_flash_duration, age: 0.0 },
                Transform::IDENTITY,
                ShadowSource(SourceKind::MuzzleFlash),
            ));
            fx.lights += 1;
        }
    }
}

/// The projectiles in flight: each one's PROJ model where combat says it is drawn.
#[allow(clippy::too_many_arguments)]
fn draw_flying(
    mut commands: Commands,
    visuals: Res<ShotVisuals>,
    combat: Res<Combat>,
    mut fx: ResMut<ShotFx>,
    mut models: ModelCtx,
    root: Query<Entity, With<WorldRoot>>,
    mut transforms: Query<&mut Transform>,
    fading: Query<&Fade>,
    mut materials: Query<&mut RigMaterials>,
) {
    let Ok(world_root) = root.single() else { return };
    let fx = &mut *fx;
    for v in &visuals.flying {
        let dir = Vec3::from_array(v.dir);
        let known = fx.flying.get(&v.id).copied();
        let beam_end = known.flatten().and_then(|k| k.1);
        // A model is not shown until it has flown its own length behind it: a tracer's
        // streak (220 units behind the bullet) would otherwise run back through the gun
        // and the shooter in its first frame [design].
        let out = |tail: f32| if v.beam.is_some() || v.travelled >= tail { Visibility::Inherited } else { Visibility::Hidden };
        let place = |beam_end: Option<f32>| Transform {
            translation: Vec3::from_array(v.pos),
            rotation: flight_rotation(dir),
            scale: match (v.beam, beam_end) {
                (Some(len), Some(end)) if end > 0.0 => Vec3::new(1.0, len / end, 1.0),
                _ => Vec3::ONE,
            },
        };
        match known {
            Some(Some((e, _, tail))) => {
                if let Ok(mut t) = transforms.get_mut(e) {
                    *t = place(beam_end);
                }
                commands.entity(e).try_insert(out(tail));
                if let Some(mut m) = fading.get(e).ok().filter(|f| !f.own).and_then(|f| materials.get_mut(f.root).ok()) {
                    fade_beam(fx, &mut m, v);
                }
            }
            Some(None) => {}
            None => {
                let p = combat.forms.projectiles.get(&v.projectile);
                let model = p.and_then(|p| p.model.as_deref());
                let pm = model.and_then(|m| load(&mut models, fx, "projectile", m));
                let spawned = pm.map(|pm| {
                    let end = pm.nodes.iter().find(|n| n.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case("BeamEnd"))).map(|n| n.local.translation.y);
                    debug!(
                        "shot fx: {} drawn at ({:.1}, {:.1}, {:.1}){}",
                        p.and_then(|p| p.edid.as_deref()).unwrap_or("?"),
                        v.pos[0],
                        v.pos[1],
                        v.pos[2],
                        v.beam.map_or(String::new(), |l| format!(", a beam {l:.0} units long"))
                    );
                    let tail = pm.bounds.map_or(0.0, |(lo, _)| (-lo.y).max(0.0));
                    let placed = place(end);
                    // Once a PROJ: the way its model points against the shot's (smoke
                    // `missile-shot-one-model`, #2470).
                    if fx.headed.insert(v.projectile) {
                        let off = (placed.rotation * Vec3::Y).angle_between(dir.normalize_or(Vec3::Y)).to_degrees();
                        info!(
                            "shot fx: projectile {} {} drawn along its flight: its +y {off:.1} degrees off the shot's direction, its long axis {}",
                            p.and_then(|p| p.edid.as_deref()).unwrap_or("?"),
                            fnv_actor::resolve::mesh_path(model.unwrap_or_default()),
                            long_axis(pm.bounds)
                        );
                    }
                    let (e, _) = spawn_effect(&mut commands, fx, world_root, &pm, placed, model.unwrap_or_default(), v.beam.is_some());
                    // Its own particle systems too (the flamer's flame strips, the laser's
                    // sparks; particles.md §6.2).
                    commands.entity(e).insert((out(tail), crate::particles::ModelEmitter::new(fnv_actor::resolve::mesh_path(model.unwrap_or_default()))));
                    (e, end, tail)
                });
                match spawned {
                    Some(_) => fx.projectiles += 1,
                    None => fx.no_model += 1,
                }
                fx.flying.insert(v.id, spawned);
            }
        }
    }
    fx.flying.retain(|id, drawn| {
        let keep = visuals.flying.iter().any(|v| v.id == *id);
        if let (false, Some((e, _, _))) = (keep, drawn) {
            commands.entity(*e).try_despawn();
        }
        keep
    });
}

/// How a projectile's model is turned for its flight (game space): its +y along `dir`
/// [observed: of the models decoded, the missile's, the mini nuke's, the 10mm bullet's,
/// the 25 mm grenade's, the orbital nuke's and the Red Glare's are longest along y (the
/// 40 mm rounds are squat), and the missile's and the orbital nuke's exhaust flames lie
/// behind them at -y; `fnvtool nif decode`, #2470].
fn flight_rotation(dir: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::Y, dir.normalize_or(Vec3::Y))
}

/// The axis along which a model's bounds reach farthest (`x`, `y` or `z`), for the log.
fn long_axis(bounds: Option<(Vec3, Vec3)>) -> &'static str {
    let Some((lo, hi)) = bounds else { return "unknown" };
    let e = hi - lo;
    if e.y >= e.x && e.y >= e.z {
        "y"
    } else if e.x >= e.z {
        "x"
    } else {
        "z"
    }
}

/// Shell casings: the WEAP's casing model thrown from the `ShellCasingNode`, falling
/// under the world's gravity, bouncing off what it meets and gone after its time.
#[allow(clippy::too_many_arguments)]
fn casings(
    mut commands: Commands,
    time: Res<Time>,
    mut thrown: MessageReader<CasingFx>,
    sfx: Option<Res<Sfx>>,
    mut phys: ResMut<PhysState>,
    mut models: ModelCtx,
    mut fx: ResMut<ShotFx>,
    root: Query<(Entity, &GlobalTransform), With<WorldRoot>>,
    mut live: Query<(Entity, &mut Casing, &mut Transform)>,
    mut knocks: MessageWriter<PlaySound>,
    body: Option<Res<PlayerBody>>,
    nodes: Query<&GlobalTransform>,
) {
    let dt = time.delta_secs();
    let gravity = phys.world.params.gravity;
    let world_only = casing_filter();
    for (e, mut c, mut t) in &mut live {
        c.left -= dt;
        if c.left <= 0.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        if c.resting {
            continue;
        }
        c.velocity.z -= gravity * dt;
        let step = c.velocity * dt;
        let len = step.length();
        let hit = (len > 1e-4).then(|| phys.ray(t.translation.to_array(), step.to_array(), len, world_only.clone())).flatten();
        match hit {
            Some(h) => {
                let n = Vec3::from_array(h.normal);
                t.translation = Vec3::from_array(h.point) + n * 0.5;
                if let (Some(sound), true) = (c.sound, c.velocity.length() >= CASING_KNOCK && c.knocks < CASING_SOUNDS) {
                    c.knocks += 1;
                    fx.casing_knocks += 1;
                    knocks.write(PlaySound { sound, at: Some(h.point) });
                }
                c.velocity = (c.velocity - 2.0 * c.velocity.dot(n) * n) * CASING_BOUNCE;
                c.spin *= CASING_BOUNCE;
                c.resting = c.velocity.length() < CASING_REST;
            }
            None => t.translation += step,
        }
        if !c.resting {
            t.rotation = Quat::from_scaled_axis(c.spin * dt) * t.rotation;
        }
    }
    let (Some(sfx), Ok((world_root, root_at))) = (sfx, root.single()) else {
        thrown.clear();
        return;
    };
    // The held weapon's `ShellCasingNode` as drawn (last frame's place), under the world
    // root: in first person the arms are drawn through a narrower view than the world's
    // (`player_body::place_lens`), so the node as posed is not where the weapon is seen.
    let seen = body.as_deref().and_then(PlayerBody::casing_entity).and_then(|e| nodes.get(e).ok()).map(|g| root_at.affine().inverse() * g.affine());
    for c in thrown.read() {
        let Some(model) = sfx.forms.weapons.get(&c.weapon).and_then(|w| w.shell_casing_model.as_deref()) else { continue };
        let Some(pm) = load(&mut models, &mut fx, "shell casing", model) else { continue };
        let (point, velocity) = match seen {
            Some(m) => (m.translation.into(), m.transform_vector3(Vec3::Z).normalize_or_zero() * Vec3::from_array(c.velocity).length()),
            None => (Vec3::from_array(c.point), Vec3::from_array(c.velocity)),
        };
        // It tumbles about an axis across its flight [design].
        let spin = velocity.cross(Vec3::Z).normalize_or(Vec3::X) * 12.0;
        let holder = spawn_model(&mut commands, world_root, &pm, Transform::from_translation(point));
        let sound = sfx.forms.sounds.iter().find(|(_, s)| s.edid.as_deref() == Some(casing_sound(model))).map(|(&id, _)| id);
        commands.entity(holder).insert(Casing { velocity, spin, left: c.life.max(0.5), resting: false, sound, knocks: 0 });
        fx.casings += 1;
    }
}

/// Impact effects: the IPCT's model where a shot landed, for the IPCT's duration.
fn impacts(
    mut commands: Commands,
    mut landed: MessageReader<ImpactFx>,
    sfx: Option<Res<Sfx>>,
    mut models: ModelCtx,
    mut fx: ResMut<ShotFx>,
    root: Query<Entity, With<WorldRoot>>,
) {
    let (Some(sfx), Ok(world_root)) = (sfx, root.single()) else {
        landed.clear();
        return;
    };
    for hit in landed.read() {
        // A blood splatter on the world behind a shot actor is a decal only (`decals`).
        if hit.splatter {
            continue;
        }
        // An actor flagged No Blood Spray shows none (docs/spec-notes/actor-flags.md §2).
        if hit.no_spray {
            fx.no_spray += 1;
            continue;
        }
        let Some(i) = sfx.forms.impacts.get(&hit.impact) else { continue };
        let Some((model, pm)) = i.model.as_deref().and_then(|m| Some((m, load(&mut models, &mut fx, "impact", m)?))) else { continue };
        let (normal, dir) = (Vec3::from_array(hit.normal).normalize_or(Vec3::Z), Vec3::from_array(hit.dir).normalize_or(Vec3::NEG_Z));
        let along = match i.orientation.as_deref() {
            Some("Projectile Vector") => dir,
            Some("Projectile Reflection") => (dir - 2.0 * dir.dot(normal) * normal).normalize_or(normal),
            _ => normal,
        };
        let place = Transform { translation: Vec3::from_array(hit.point), rotation: Quat::from_rotation_arc(Vec3::Z, along), scale: Vec3::ONE };
        let (holder, _) = spawn_effect(&mut commands, &mut fx, world_root, &pm, place, model, false);
        // Its own particle systems, if any (the debris of an explosion's dirt impact).
        commands.entity(holder).insert((Flash { left: i.effect_duration }, crate::particles::ModelEmitter::new(fnv_actor::resolve::mesh_path(model))));
        fx.impacts += 1;
    }
}

/// Explosions: the EXPL's model playing its sequence and emitting its particle systems,
/// its light fading, and its image space modifier for an eye within its radius.
#[allow(clippy::too_many_arguments)]
fn explosions(
    mut commands: Commands,
    time: Res<Time>,
    mut boom: MessageReader<ExplosionFx>,
    (sfx, combat, mut screen): (Option<Res<Sfx>>, Res<Combat>, Option<ResMut<crate::imagespace::ScreenModifiers>>),
    tweaks: Option<Res<nvf_play::tweaks::TweakSettings>>,
    world: Option<Res<WorldData>>,
    mut models: ModelCtx,
    mut fx: ResMut<ShotFx>,
    root: Query<(Entity, &Transform), With<WorldRoot>>,
    camera: Query<&GlobalTransform, With<nvf_play::PlayerCamera>>,
    mut lights: Query<(Entity, &mut FadingLight, &mut PointLight)>,
) {
    let dt = time.delta_secs();
    for (e, mut f, mut l) in &mut lights {
        f.age += dt;
        let k = 1.0 - f.age / EXPLOSION_LIGHT_SECS;
        if k <= 0.0 {
            commands.entity(e).try_despawn();
        } else {
            l.intensity = f.full * k;
        }
    }
    let (Some(sfx), Ok((world_root, root_place))) = (sfx, root.single()) else {
        boom.clear();
        return;
    };
    // The eye in game space.
    let eye = camera.single().ok().map(|c| root_place.compute_affine().inverse().transform_point3(c.translation()));
    for b in boom.read() {
        let Some(x) = sfx.forms.explosions.get(&b.explosion) else { continue };
        // One that went off in or by water shows only the water thrown up (its own
        // model and light are not drawn; its screen effect still plays).
        let model = match b.look {
            ExplosionLook::Own => x.model.as_deref(),
            ExplosionLook::Hidden => None,
            ExplosionLook::WaterSurface => Some(WATER_SURFACE_EXPLOSION),
        };
        match b.look {
            ExplosionLook::Hidden => fx.explosions_in_water += 1,
            ExplosionLook::WaterSurface => fx.water_surfaces += 1,
            ExplosionLook::Own => {}
        }
        let world_up = b.look == ExplosionLook::WaterSurface || combat.forms.explosions.get(&b.explosion).is_some_and(|s| s.flags.iter().any(|f| f == WORLD_ORIENTATION));
        let place = Transform::from_translation(Vec3::from_array(b.point)).with_rotation(explosion_rotation(world_up, b.normal));
        if !world_up && b.normal.is_some() && b.look == ExplosionLook::Own {
            fx.explosions_aligned += 1;
        }
        let (holder, length) = match model.and_then(|m| load(&mut models, &mut fx, "explosion", m).map(|pm| (m, pm))) {
            Some((m, pm)) => {
                let (h, length) = spawn_effect(&mut commands, &mut fx, world_root, &pm, place, m, false);
                commands.entity(h).insert(crate::particles::ModelEmitter::new(fnv_actor::resolve::mesh_path(m)));
                (h, length)
            }
            None => (commands.spawn((place, Visibility::Inherited, ChildOf(world_root))).id(), 0.0),
        };
        commands.entity(holder).insert(Flash { left: if length > 0.0 { length } else { EXPLOSION_SECS } });
        if b.look == ExplosionLook::Own {
            fx.explosions += 1;
        }
        // Its image space modifier, seen from within its image-space radius ("IS Radius:
        // Radius of the area that will see the ImageSpace effect" [documented: GECK wiki
        // Explosion]), at full strength [hypothesis: the IMAD's keys have no distance in
        // them; nothing says the effect weakens towards the edge].
        let stats = combat.forms.explosions.get(&b.explosion);
        if let (Some(s), Some(eye), Some(screen)) = (stats, eye, screen.as_deref_mut()) {
            // The player's tweak leaves out the blur of a blast in the face [design: docs/design/tweaks.md].
        let blur_off = tweaks.as_ref().is_some_and(|t| t.no_explosion_blur);
        if let Some(imod) = s.image_space_modifier.filter(|_| !blur_off && eye.distance(Vec3::from_array(b.point)) <= s.is_radius) {
                screen.play(imod.0, time.elapsed_secs());
                fx.screen_effects += 1;
            }
        }
        let light = x.light.filter(|_| b.look == ExplosionLook::Own).zip(world.as_deref()).and_then(|(id, w)| fnv_world::lighting::light_data(w.lo, w.schema, id.0));
        if let Some(l) = light.filter(|l| l.radius > 0.0) {
            let color = crate::lights::display_color(l.color.map(|b| b as f32 / 255.0));
            let intensity = crate::lights::lumens(l.fade.max(0.0), crate::sky::light_unit());
            // Above the point, so the ground it went off on is lit [design].
            commands.spawn((
                PointLight { color, intensity, range: l.radius, shadow_maps_enabled: false, ..default() },
                FadingLight { full: intensity, age: 0.0 },
                Transform::from_translation(Vec3::from_array(b.point) + Vec3::Z * 32.0),
                ChildOf(world_root),
                // ALC shadows (`crate::light_shadows`); its model's shapes cast none.
                ShadowSource(SourceKind::Explosion),
            ));
        }
    }
}

/// EXPL DATA flag 1 [documented: schema combat.json, xEdit EXPL].
pub const WORLD_ORIENTATION: &str = "Always Uses World Orientation";

/// How an explosion's model is turned (game space): upright when it always uses the
/// world's orientation or no ground was found, else its +z along the ground's normal
/// (the shortest turn from up).
pub fn explosion_rotation(world_up: bool, normal: Option<[f32; 3]>) -> Quat {
    match normal.map(Vec3::from_array).filter(|n| !world_up && n.length_squared() > 1e-6) {
        Some(n) => Quat::from_rotation_arc(Vec3::Z, n.normalize()),
        None => Quat::IDENTITY,
    }
}

fn report(mut exit: MessageReader<AppExit>, fx: Res<ShotFx>) {
    if exit.read().last().is_some() && fx.flashes + fx.projectiles + fx.no_model > 0 {
        info!(
            "shot fx: {} muzzle flashes ({} with a light), {} projectile models drawn, {} projectiles without a model, {} shell casings, {} impact effects, {} explosions, {} casing knocks heard",
            fx.flashes, fx.lights, fx.projectiles, fx.no_model, fx.casings, fx.impacts, fx.explosions, fx.casing_knocks
        );
        info!("shot fx: {} effect models played their own animation, {} explosion screen effects", fx.animated, fx.screen_effects);
        if fx.explosions > 0 {
            info!("shot fx: {} explosions turned to the ground under them, {} upright", fx.explosions_aligned, fx.explosions - fx.explosions_aligned);
        }
        if fx.explosions_in_water + fx.water_surfaces > 0 {
            info!("shot fx: {} explosions in water drawn as {} water surfaces thrown up", fx.explosions_in_water, fx.water_surfaces);
        }
        if fx.no_spray > 0 {
            info!("shot fx: {} impacts on actors flagged No Blood Spray not shown", fx.no_spray);
        }
        if fx.actor_flashes_muzzle + fx.actor_flashes_at_shot > 0 {
            info!("shot fx: other actors' muzzle flashes: {} at their weapon's muzzle, {} where the shot left (no muzzle node)", fx.actor_flashes_muzzle, fx.actor_flashes_at_shot);
        }
        if fx.first_person_flashes > 0 {
            info!("shot fx: {} muzzle flashes drawn in front of the world with the first-person weapon", fx.first_person_flashes);
        }
        if fx.beams_animated > 0 {
            info!("shot fx: {} beams played their own animation, {} faded out over their fade duration", fx.beams_animated, fx.beams_faded);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_muzzle_flash_light_comes_up_and_goes_down_without_a_jump() {
        // ED-E's laser: 0.14 s (#3785). Dark at the start and the end, full at the top.
        let life = 0.14;
        assert_eq!(flash_light_share(0.0, life), 0.0);
        assert!((flash_light_share(life * FLASH_LIGHT_RISE, life) - 1.0).abs() < 1e-5);
        assert!(flash_light_share(life, life) < 1e-5);
        // No step between frames at 60 frames a second larger than a fair share of it.
        let mut last = 0.0;
        for i in 1..=9 {
            let s = flash_light_share(i as f32 / 60.0, life);
            assert!((s - last).abs() <= 0.75, "frame {i}: {last} -> {s}");
            last = s;
        }
        assert_eq!(flash_light_share(0.05, 0.0), 0.0);
    }

    #[test]
    fn a_flying_model_points_its_plus_y_along_the_shot() {
        // Level, up, down, straight back and a slant: the missile's nose (+y) goes the
        // way the shot goes, never across it (#2470).
        for d in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::NEG_Z, Vec3::NEG_Y, Vec3::new(-3.0, 4.0, 1.5)] {
            let along = flight_rotation(d) * Vec3::Y;
            assert!(along.angle_between(d.normalize()) < 1e-3, "{d}: {along}");
        }
        // No direction: as built.
        assert!((flight_rotation(Vec3::ZERO) * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-6));
    }

    #[test]
    fn the_long_axis_of_a_model() {
        // MLProjectile.NIF's bounds, flame included (`fnvtool nif decode`).
        assert_eq!(long_axis(Some((Vec3::new(-5.7, -55.7, -5.9), Vec3::new(5.7, 12.7, 5.9)))), "y");
        assert_eq!(long_axis(Some((Vec3::new(-9.0, -1.0, -1.0), Vec3::new(9.0, 1.0, 1.0)))), "x");
        assert_eq!(long_axis(Some((Vec3::new(-1.0, -1.0, -4.0), Vec3::new(1.0, 1.0, 4.0)))), "z");
        assert_eq!(long_axis(None), "unknown");
    }
}
