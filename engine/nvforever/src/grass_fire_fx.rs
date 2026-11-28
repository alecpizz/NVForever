//! Grass fires on screen (`nvf_play::grass_fire`, docs/design/grass-fire.md): the burn
//! map copied to the grass materials' texture when it changes (`crate::grass::GrassBurn`),
//! the cells that unloaded forgotten so their grass grows back whole, the weather's wind
//! handed to the fires, and the flames, smoke and light of each burning patch.
//!
//! Each patch that catches gets [design, owner 2026-11-14: "a bit sparse", "starts small
//! and gets larger"]:
//!
//! - two small fires (`mps\mpsfiresmall01.nif`, the particle fire effects add on as an
//!   add-on node) at random spots in it, and on about a third of patches a larger one
//!   (`mps\mpsfiremed01.nif`);
//! - each starts at [`START_SIZE`] of its size and grows to it over [`GROW_SECS`] (its
//!   spray from the holder's scale, and `ModelEmitter::size` dividing out that scale so
//!   born particles' radius stays linear in the growth share, #3673), and dies
//!   down over [`SHRINK_SECS`] when the patch burns out;
//! - on about one patch in six, an orange point light that flickers and grows and dies
//!   with the flames, at most [`MAX_LIGHTS`];
//! - when it burns out, on half the patches, smoke (`mps\mpssmokesmall01.nif`) that
//!   smoulders for [`SMOULDER_SECS`].
//!
//! At most [`MAX_EFFECTS`] flame and smoke models at once; past that a patch gets none
//! (the grass still glows and chars).
//!
//! Log: `grass fire fx: …` once per model when it is loaded (or not), and at exit
//! `grass fire fx summary: …`.

use crate::grass::GrassBurn;
use crate::particles::ModelEmitter;
use crate::world::{Streamer, WorldRoot};
use bevy::prelude::*;
use nvf_assets::model::{ModelCtx, PreparedModel};
use nvf_play::grass_fire::{centre, FireEvent, GrassFires, Key};
use std::collections::HashMap;
use std::sync::Arc;

pub const SMALL_FIRE: &str = r"mps\mpsfiresmall01.nif";
pub const MEDIUM_FIRE: &str = r"mps\mpsfiremed01.nif";
pub const SMOKE: &str = r"mps\mpssmokesmall01.nif";
/// The most flame and smoke models at once [design: frame cost].
pub const MAX_EFFECTS: usize = 420;
/// The most fire lights at once [design: frame cost].
pub const MAX_LIGHTS: usize = 16;
/// A flame's size when its patch catches, as a share of its full size [chosen].
pub const START_SIZE: f32 = 0.15;
/// Seconds a flame takes to grow to its full size [chosen].
pub const GROW_SECS: f32 = 2.0;
/// Seconds a flame takes to die down once its patch burnt out [chosen].
pub const SHRINK_SECS: f32 = 1.2;
/// Seconds smoke rises from a burnt-out patch [chosen].
pub const SMOULDER_SECS: f32 = 5.0;
/// A fire light's brightness (a LIGH fade) and reach [chosen].
const LIGHT_FADE: f32 = 1.4;
const LIGHT_RANGE: f32 = 420.0;

#[derive(Resource, Default)]
struct FlameFx {
    /// Each burning patch's ground height and its flames (and light).
    patches: HashMap<Key, (f32, Vec<Entity>)>,
    /// Models by path, once tried (`None` inside: it did not load).
    models: HashMap<&'static str, Option<Arc<PreparedModel>>>,
    /// Flame and smoke models alive, lights alive.
    effects: usize,
    lights: usize,
    spawned: u32,
    smoke: u32,
    lit: u32,
    skipped: u32,
    uploads: u32,
    cells_forgotten: u32,
}

/// A flame: it grows, burns, and dies down once `out` is set.
#[derive(Component)]
struct Flame {
    age: f32,
    /// Seconds since its patch burnt out.
    out: Option<f32>,
    /// Its full particle size and spray scale.
    size: f32,
    scale: f32,
}

/// Smoke over a burnt-out patch.
#[derive(Component)]
struct Smoulder {
    age: f32,
    size: f32,
}

/// A fire light: it grows and dies with its patch's flames, and flickers.
#[derive(Component)]
struct FireLight {
    age: f32,
    out: Option<f32>,
    full: f32,
    phase: f32,
}

pub struct GrassFireFxPlugin;

impl Plugin for GrassFireFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlameFx>()
            .add_systems(Update, (sync, animate).chain().after(nvf_play::PlaySet::Activation).after(crate::world::stream).run_if(resource_exists::<GrassFires>))
            .add_systems(Last, report);
    }
}

fn model(fx: &mut FlameFx, models: &mut ModelCtx, path: &'static str) -> Option<Arc<PreparedModel>> {
    fx.models
        .entry(path)
        .or_insert_with(|| {
            let full = fnv_actor::resolve::mesh_path(path);
            let pm = models.model(&full);
            match &pm {
                Some(pm) => info!("grass fire fx: model {full}: {} shapes, {} nodes", pm.shapes.len(), pm.nodes.len()),
                None => warn!("grass fire fx: model {full} not loaded; fires show without it"),
            }
            pm
        })
        .clone()
}

/// Spawn one of the effect models at `place`; `None` past the cap or without the model.
fn spawn(commands: &mut Commands, fx: &mut FlameFx, models: &mut ModelCtx, root: Entity, path: &'static str, place: Transform, size: f32) -> Option<Entity> {
    if fx.effects >= MAX_EFFECTS {
        fx.skipped += 1;
        return None;
    }
    let pm = model(fx, models, path)?;
    let full = fnv_actor::resolve::mesh_path(path);
    let (holder, _) = crate::shot_fx::spawn_playing_itself(commands, root, &pm, place, &full);
    let mut emitter = ModelEmitter::new(full);
    emitter.size = size;
    emitter.scale_radius_with_holder = matches!(path, SMALL_FIRE | MEDIUM_FIRE);
    commands.entity(holder).insert(emitter);
    fx.effects += 1;
    Some(holder)
}

/// See the module notes.
#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    mut gf: ResMut<GrassFires>,
    mut fx: ResMut<FlameFx>,
    streamer: Res<Streamer>,
    burn: Res<GrassBurn>,
    sky: Option<Res<crate::sky::SkyState>>,
    mut models: ModelCtx,
    root: Query<Entity, With<WorldRoot>>,
    mut flames: Query<&mut Flame>,
    mut lights: Query<&mut FireLight>,
) {
    let fx = &mut *fx;
    let fires = &mut gf.fires;
    // The weather's wind: game (x, y) is Bevy (x, -z) (`crate::tree::wind_vector`).
    if let Some(sky) = sky.as_deref() {
        let w = crate::tree::wind_vector(sky);
        fires.wind = [w.x * w.w, -w.z * w.w];
    }
    // Cells no longer loaded (nor being spawned): forgotten, so they grow back.
    let gone: Vec<(i32, i32)> = fires.cells().filter(|c| streamer.interior.is_some() || !(streamer.loaded.contains_key(c) || streamer.spawning.contains_key(c))).collect();
    for c in gone {
        fires.forget_cell(c);
        fx.cells_forgotten += 1;
    }
    if fires.dirty {
        if let Some(mut image) = models.images.get_mut(&burn.0) {
            image.data = Some(fires.map.clone());
            fires.dirty = false;
            fx.uploads += 1;
        }
    }
    let events = std::mem::take(&mut fires.events);
    let Ok(world_root) = root.single() else { return };
    for e in events {
        match e {
            FireEvent::Lit { key, z } => {
                let c = centre(key);
                let r = hash(key);
                let bits = |shift: u32, n: u32| ((r >> shift) & ((1 << n) - 1)) as f32 / ((1u32 << n) - 1) as f32;
                let mut made = Vec::new();
                // Two small fires at random spots in the patch, sized at random [chosen].
                for (i, shift) in [(0u32, 0u32), (1, 12)] {
                    let at = Vec3::new(c[0] + (bits(shift, 4) - 0.5) * 48.0, c[1] + (bits(shift + 4, 4) - 0.5) * 48.0, z);
                    let size = 0.9 + 0.5 * bits(shift + 8, 4);
                    let place = Transform::from_translation(at).with_rotation(Quat::from_rotation_z(i as f32 * 2.1 + bits(24, 4) * std::f32::consts::TAU)).with_scale(Vec3::splat(START_SIZE));
                    // `particles::emit` multiplies these flames' born radius by the holder's scale
                    // (`START_SIZE`) as well as `ModelEmitter::size` (#3673), so passing
                    // `size` keeps the initial radius factor at `size * START_SIZE`.
                    if let Some(h) = spawn(&mut commands, fx, &mut models, world_root, SMALL_FIRE, place, size) {
                        commands.entity(h).insert(Flame { age: 0.0, out: None, size, scale: 1.0 });
                        made.push(h);
                    }
                }
                // A larger fire on about a third of the patches.
                if r.is_multiple_of(3) {
                    let size = 0.8 + 0.3 * bits(28, 4);
                    let place = Transform::from_translation(Vec3::new(c[0], c[1], z)).with_scale(Vec3::splat(START_SIZE));
                    if let Some(h) = spawn(&mut commands, fx, &mut models, world_root, MEDIUM_FIRE, place, size) {
                        commands.entity(h).insert(Flame { age: 0.0, out: None, size, scale: 1.0 });
                        made.push(h);
                    }
                }
                // A light on about one patch in six, above the flames.
                if (r >> 20).is_multiple_of(6) && fx.lights < MAX_LIGHTS {
                    let full = crate::lights::lumens(LIGHT_FADE, crate::sky::light_unit());
                    let light = commands
                        .spawn((
                            PointLight { color: crate::lights::display_color([1.0, 0.45, 0.12]), intensity: 0.0, range: LIGHT_RANGE, shadow_maps_enabled: false, ..default() },
                            FireLight { age: 0.0, out: None, full, phase: bits(4, 8) * std::f32::consts::TAU },
                            Transform::from_translation(Vec3::new(c[0], c[1], z + 48.0)),
                            ChildOf(world_root),
                            // ALC shadows (`crate::light_shadows`); the flames and smoke
                            // cast none (`crate::shot_fx::spawn_playing_itself`).
                            crate::light_shadows::ShadowSource(crate::light_shadows::SourceKind::GrassFire),
                        ))
                        .id();
                    fx.lights += 1;
                    fx.lit += 1;
                    made.push(light);
                }
                fx.spawned += made.len() as u32;
                fx.patches.insert(key, (z, made));
            }
            FireEvent::Out { key } => {
                let Some((z, made)) = fx.patches.remove(&key) else { continue };
                for e in made {
                    if let Ok(mut f) = flames.get_mut(e) {
                        f.out.get_or_insert(0.0);
                    } else if let Ok(mut l) = lights.get_mut(e) {
                        l.out.get_or_insert(0.0);
                    }
                }
                // Smoke on half the burnt-out patches.
                let r = hash(key);
                if (r >> 16).is_multiple_of(2) {
                    let c = centre(key);
                    let place = Transform::from_translation(Vec3::new(c[0], c[1], z));
                    if let Some(h) = spawn(&mut commands, fx, &mut models, world_root, SMOKE, place, 1.0) {
                        commands.entity(h).insert(Smoulder { age: 0.0, size: 1.0 });
                        fx.smoke += 1;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Flames grow, burn and die down; smoke rises and thins; lights flicker.
#[allow(clippy::type_complexity)]
fn animate(
    mut commands: Commands,
    time: Res<Time>,
    mut fx: ResMut<FlameFx>,
    mut flames: Query<(Entity, &mut Flame, &mut Transform, &mut ModelEmitter), (Without<Smoulder>, Without<FireLight>)>,
    mut smoke: Query<(Entity, &mut Smoulder, &mut ModelEmitter), Without<Flame>>,
    mut lights: Query<(Entity, &mut FireLight, &mut PointLight)>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    for (e, mut f, mut t, mut em) in &mut flames {
        f.age += dt;
        let out = f.out.map(|o| o + dt);
        f.out = out;
        let share = match out {
            Some(out) if out >= SHRINK_SECS => {
                commands.entity(e).try_despawn();
                fx.effects = fx.effects.saturating_sub(1);
                continue;
            }
            // From the size it had when its patch burnt out.
            Some(out) => grown(f.age - out) * (1.0 - out / SHRINK_SECS),
            None => grown(f.age),
        };
        let (holder_scale, emitter_size) = flame_scales(f.size, f.scale, share);
        em.size = emitter_size;
        t.scale = Vec3::splat(holder_scale);
    }
    for (e, mut s, mut em) in &mut smoke {
        s.age += dt;
        if s.age >= SMOULDER_SECS {
            commands.entity(e).try_despawn();
            fx.effects = fx.effects.saturating_sub(1);
            continue;
        }
        // Thinning over its last 1.5 s.
        em.size = s.size * ((SMOULDER_SECS - s.age) / 1.5).clamp(0.0, 1.0);
    }
    for (e, mut l, mut light) in &mut lights {
        l.age += dt;
        let share = match l.out.as_mut() {
            Some(out) => {
                *out += dt;
                if *out >= SHRINK_SECS {
                    commands.entity(e).try_despawn();
                    fx.lights = fx.lights.saturating_sub(1);
                    continue;
                }
                1.0 - *out / SHRINK_SECS
            }
            None => grown(l.age),
        };
        let flicker = 0.8 + 0.12 * (now * 11.0 + l.phase).sin() + 0.08 * (now * 23.0 + 2.0 * l.phase).sin();
        light.intensity = l.full * share * flicker;
    }
}

/// Holder transform scale (spray) and `ModelEmitter::size` for a grass-fire flame of full
/// particle factor `size` and full spray scale `scale` at growth `share`.
///
/// These emitters opt in to holder scaling. `emitter_size` divides out the holder scale so
/// the effective born radius factor stays linear (`size * share`) without quadratic
/// double-scaling [design].
pub(crate) fn flame_scales(size: f32, scale: f32, share: f32) -> (f32, f32) {
    let holder = scale * (0.35 + 0.65 * share);
    let emitter_size = if holder > 1e-6 { (size * share) / holder } else { 0.0 };
    (holder, emitter_size)
}

/// How far grown a flame is `age` seconds after it caught: [`START_SIZE`] to 1 over
/// [`GROW_SECS`], fast at first [chosen].
fn grown(age: f32) -> f32 {
    let k = (age / GROW_SECS).clamp(0.0, 1.0);
    let ease = 1.0 - (1.0 - k) * (1.0 - k);
    START_SIZE + (1.0 - START_SIZE) * ease
}

/// A patch's fixed random bits.
fn hash(k: Key) -> u32 {
    let mut h = (k.0 as u32).wrapping_mul(0x9E37_79B1) ^ (k.1 as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0xC2B2_AE3D);
    h ^ (h >> 13)
}

fn report(mut exit: MessageReader<AppExit>, fx: Res<FlameFx>) {
    if exit.read().next().is_some() {
        info!(
            "grass fire fx summary: {} flames drawn ({} past the cap of {MAX_EFFECTS} not drawn), {} smoke, {} lights, {} burn map uploads, {} cells forgotten (their grass grows back)",
            fx.spawned, fx.skipped, fx.smoke, fx.lit, fx.uploads, fx.cells_forgotten
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn flames_start_small_and_grow() {
        assert_eq!(grown(0.0), START_SIZE);
        assert!(grown(GROW_SECS * 0.5) > 0.6);
        assert_eq!(grown(GROW_SECS), 1.0);
        assert_eq!(grown(100.0), 1.0);
    }

    /// [design, #3673] Grass-fire flames opt in to holder scaling and keep their born radius linear in
    /// `size * share` across spawn, growth, full burn, shrink, and zero-scale guard.
    #[test]
    fn flame_effective_radius_stays_linear_without_double_scaling() {
        for size in [0.8_f32, 1.15, 1.4] {
            for scale in [0.5_f32, 1.0, 1.5] {
                for share in [0.0_f32, START_SIZE, 0.5, 1.0] {
                    let (holder, em_size) = flame_scales(size, scale, share);
                    assert!((holder - scale * (0.35 + 0.65 * share)).abs() < 1e-6);
                    assert!((holder * em_size - size * share).abs() < 1e-5, "size={size} scale={scale} share={share}");
                }
            }
            // Zero-scale guard produces finite zero, never NaN or Inf.
            let (holder, em_size) = flame_scales(size, 0.0, 0.5);
            assert_eq!((holder, em_size), (0.0, 0.0));
        }

        // Real `animate` ECS system check across spawn -> growth -> full -> shrink.
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(FlameFx { effects: 1, ..default() });
        let full_size = 1.2_f32;
        let mut em = ModelEmitter::new(SMALL_FIRE);
        em.size = full_size;
        let e = world
            .spawn((
                Flame { age: 0.0, out: None, size: full_size, scale: 1.0 },
                Transform::IDENTITY.with_scale(Vec3::splat(START_SIZE)),
                em,
            ))
            .id();
        // Spawn state before first animate tick: effective radius factor == full_size * START_SIZE.
        let spawn_effective = world.get::<Transform>(e).unwrap().scale.x * world.get::<ModelEmitter>(e).unwrap().size;
        assert!((spawn_effective - full_size * START_SIZE).abs() < 1e-5);

        // Mid-growth (0.5 s) and full burn (2.5 s).
        for (dt, expected_age) in [(0.5_f32, 0.5_f32), (2.0_f32, 2.5_f32)] {
            world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(dt));
            world.run_system_once(animate).unwrap();
            let t = world.get::<Transform>(e).unwrap();
            let em = world.get::<ModelEmitter>(e).unwrap();
            let share = grown(expected_age);
            assert!((t.scale.x - (0.35 + 0.65 * share)).abs() < 1e-5, "spray scale preserved");
            assert!((t.scale.x * em.size - full_size * share).abs() < 1e-5, "effective born radius stays linear");
        }

        // Shrinking after patch burns out (0.6 s of 1.2 s shrink).
        world.get_mut::<Flame>(e).unwrap().out = Some(0.0);
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.6));
        world.run_system_once(animate).unwrap();
        let t = world.get::<Transform>(e).unwrap();
        let em = world.get::<ModelEmitter>(e).unwrap();
        let share = grown(2.5) * (1.0 - 0.6 / SHRINK_SECS);
        assert!((t.scale.x * em.size - full_size * share).abs() < 1e-5, "shrink radius stays linear");
    }
}
