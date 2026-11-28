//! Water surfaces: one plane per "has water" cell (world.rs) drawn with a material built
//! from the water type's WATR record (fnv_world::sky::water). Which record fields mean
//! what is documented (xEdit names); how the engine combines them is a hypothesis
//! (docs/spec-notes/world.md §6), checked against in-game screenshots:
//!
//! - Seen from above, the water body takes its colour by how far below the surface the
//!   opaque surface behind it lies (from the camera's depth prepass; the above fog's
//!   near and far planes are depths below the surface [documented: GECK wiki
//!   WaterType]), shallow to deep colour from the near to the far depth (-80 / 850 for
//!   Lake Mead's NVCleanWater), and its opacity by the length of the view ray inside the
//!   water over the same ramp, opaque beyond the far plane. Placed water (PWAT) only
//!   reaches the above fog's amount (the deep plane's transparency [documented: same
//!   page]); cell and distant water are not capped, as the cap left the lake bed showing
//!   far out in Lake Mead (NVCleanWater, amount 0.75) [design, until a far-out Lake Mead
//!   comparison is made]. So the bed shows at the waterline and fades out as it drops,
//!   and a pool shallower than the far depth keeps its shallow colour, opaque at a
//!   grazing look (the Ultra-Luxe fountain, #1787; before that the colour too followed
//!   the ray, which made small pools the deep colour). Compared with the original:
//!   fogging by the distance from the camera turned the water opaque right at the shore
//!   when seen from a few metres up; the opacity by the depth alone showed the
//!   fountain's floor (world.md §6).
//! - Reflective types reflect the world (reflection.rs: a mirrored camera's image of
//!   the scene above the water nearest the camera; the weather's sky gradient for water
//!   at other heights), others their reflection colour, weighted by
//!   `reflectivity × mix(1, Schlick fresnel, fresnel amount)`; the sun adds a highlight
//!   with the type's shininess as exponent.
//! - The noise texture holds three height layers, one per channel (R, G, B: fine to
//!   coarse) [observed: uncorrelated, tileable channels in WastelandWaterPotomac.dds];
//!   layer i scrolls with its wind direction and speed and is scaled by its amplitude.
//!
//! - Refraction: the water is drawn after the opaque pass with a copy of it (Bevy's view
//!   transmission texture) and composites itself over that copy, sampled with an offset
//!   along the wave normal's screen-space tilt, growing over the first 200 units of
//!   water [tuned by eye]; where the offset sample lands on something in front of the
//!   water, the unshifted one is used.
//! - Under water, the camera's fog becomes the water's (underwater.rs).
//!
//! - Ripples from rain and from things moving through the water (ripples.rs) add to
//!   the wave normal.
//!
//! - Shore fade: seen from above, the whole surface (reflection included) fades in over
//!   the type's depth falloff, from its start to its end in units of water the view ray
//!   crosses (`fnv_world::sky::Water::edge_falloff`), so the reflecting surface meets
//!   the land without a seam ("used for edges of the water, to blend it smoothly with
//!   the land" [documented: GECK wiki WaterType]; the units are a hypothesis).
//!
//! - A script's RemapWaterType (Fallout 3's; `fnv_game::Game::water_type`) draws one
//!   water type as another: [`WaterRemaps`] holds the game's remaps and
//!   [`water_material`] builds the remapped type's material. Water already drawn keeps
//!   its material until its cell is drawn again [design].
//!
//! - Placed water (PWAT) is drawn with its own water type; one whose PWAT does not set
//!   "Reflects" (bit 0) reflects nothing, only its water body shows
//!   (`fnv_world::sky::placeable_water_reflects`; the Ultra-Luxe fountain, #1780).
//!
//! Not done: the ANAM opacity (the GECK wiki names it without a meaning), distortion,
//! light radius and brightness.

use crate::lod::LodState;
use crate::reflection::{Reflection, WATER_LAYER};
use crate::river_flow::{surface_flow, FlowDirection};
use crate::ripples::Ripples;
use crate::sky::SkyState;
use crate::world::{Streamer, WorldData, WorldRoot};
use bevy::asset::{embedded_asset, load_internal_asset, uuid_handle};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, BufferUsages, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::{Shader, ShaderRef};
use fnv_world::land::{load_land, LandParts, SIDE};
use fnv_world::sky::{srgb_to_linear, ColorType, Water};
use fnv_world::CELL_SIZE;
use nvf_assets::model::ModelCtx;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

/// Height of the noise texture's full range in game units: the record's "noise normals
/// scale" [hypothesis] times this factor [tuned by eye against in-game screenshots].
const WAVE_HEIGHT_FACTOR: f32 = 2.0;

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        const CAUSTICS: Handle<Shader> = uuid_handle!("dcc81bbe-64d4-40df-a75d-ce8d410c5e16");
        load_internal_asset!(app, CAUSTICS, "caustics.wesl", |s, _| Shader::from_wesl(s, "embedded://nvforever/caustics.wesl"));
        embedded_asset!(app, "water.wesl");
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default())
            .init_resource::<WaterCache>()
            .init_resource::<DistantWater>()
            .init_resource::<WaterRemaps>()
            .add_plugins(crate::underwater::UnderwaterFogPlugin)
            .add_systems(PostUpdate, update_shot_rings.run_if(resource_exists::<nvf_play::combat::ShotVisuals>));
        add_update_systems(app);
    }
}

pub(crate) fn add_update_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            sync_remaps.before(crate::world::stream),
            // After streaming and distant water so newly created materials get the
            // current sky, sun, and water-effects params on their first frame.
            update_water_sky.after(crate::sky::update_sky).after(crate::world::stream).after(distant_water),
            // After the LOD chose its chunks this frame (the water covers them).
            distant_water.after(crate::world::stream).after(crate::lod::update_lod),
        ),
    );
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct WaterParams {
    pub shallow: Vec4,
    pub deep: Vec4,
    pub reflection: Vec4,
    pub sky_upper: Vec4,
    pub sky_horizon: Vec4,
    pub above_fog: Vec4,
    pub under_fog: Vec4,
    pub surface: Vec4,
    pub layers: [Vec4; 3],
    /// Bevy y of the plane the reflection texture shows, and 1 when it is in use.
    pub reflection_plane: Vec4,
    /// Ripple grid: game x, y of its corner, units per cell, height scale (0: none).
    pub ripple: Vec4,
    /// Shore fade: the depth falloff's start and end in units of water crossed, and 1
    /// when the type has one (`fnv_world::sky::Water::edge_falloff`).
    pub edge: Vec4,
    /// Visibility probe: Bevy y of the plane, whether to count it, and the two 16-bit
    /// halves of the probe generation (`crate::reflection`, #1060).
    pub seen_plane: Vec4,
    /// Direction toward the outdoor sun (Bevy axes) and daylight strength.
    pub water_sun: Vec4,
    shot_rings: [ShotRingUniform; MAX_SHOT_RINGS],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
struct ShotRingUniform {
    /// Game x/y, surface height, and wrapped shader start time plus one. Empty slots are zero.
    point_time: Vec4,
    /// Unit projectile direction projected onto the surface, or the axis of a blast's fallout.
    travel: Vec2,
    receiver: u32,
    /// 0 for a bullet, 1 for a heavy projectile crossing, 2 for a blast.
    kind: u32,
    /// Horizontal spread and vertical strength of a blast, relative to the authored column.
    blast_scale: Vec2,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct WaterMaterial {
    #[uniform(0)]
    pub params: WaterParams,
    /// Original WATR layers and their CPU-baked river counterparts.
    pub wind_layers: [Vec4; 3],
    pub current_layers: [Vec4; 3],
    #[texture(1)]
    #[sampler(2)]
    pub noise: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub reflection: Option<Handle<Image>>,
    /// R32Float ripple heights (not filterable: the shader interpolates).
    #[texture(5, sample_type = "float", filterable = false)]
    pub ripples: Option<Handle<Image>>,
    /// Shared visibility packet: an atomic fragment count followed by the probe
    /// generation, source frame and source time written by `crate::reflection`.
    /// It requires storage buffers in the fragment stage (not available in WebGL2).
    #[storage(6, visibility(fragment))]
    pub seen: Handle<ShaderBuffer>,
}

impl Material for WaterMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://nvforever/water.wesl".into()
    }

    // Opaque, drawn after the opaque pass with a copy of it (the view transmission
    // texture), which the shader refracts and composites the water over.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }

    fn reads_view_transmission_texture(&self) -> bool {
        true
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Visible from below as well.
        descriptor.primitive.cull_mode = None;
        // Not in the depth prepass: the ambient occlusion's texels under the water are the
        // bed's (and within the lighting's distance tolerance of shallow water), so it does
        // not read them (ssao_lighting.rs).
        nvf_assets::ambient_occlusion_opt_out(descriptor);
        Ok(())
    }
}

/// One material per water type FormID, whether it reflects (placed water whose PWAT
/// does not: [`water_material`]) and whether it is placed water (its opacity capped at
/// the above fog's amount), and the shared cell-sized plane.
#[derive(Resource, Default)]
pub struct WaterCache {
    materials: HashMap<(u32, bool, bool, FlowDirection), Handle<WaterMaterial>>,
    plane: Option<Handle<Mesh>>,
    /// The water-pixel counter ([`WaterMaterial::seen`]), made with the first material.
    seen: Option<Handle<ShaderBuffer>>,
}

// Keep the newest crossings under automatic fire. The shader stops at the first empty slot.
const MAX_SHOT_RINGS: usize = 8;
const SHOT_RING_LIFETIME: f64 = 1.2;
const BLAST_RING_LIFETIME: f64 = 3.4;
const BLAST_RING: u32 = 2;

/// Horizontal spread and strength of the surface wave [design].
/// A deeper blast spreads and weakens as it reaches the surface. An airburst
/// loses coupling as its height above the water grows.
fn blast_surface_scale(blast: nvf_play::combat::SurfaceBlast) -> Vec2 {
    // A conventional frag's 900-unit EXPL radius and a missile's 1000 both
    // stay in the scaling range (docs/spec-notes/explosion-effects.md).
    let size = (blast.radius.max(0.0) / 600.0).sqrt().clamp(0.55, 1.8);
    let depth = (blast.depth / blast.reach.max(1.0)).clamp(-1.0, 1.0);
    let width = size * (1.0 + 0.15 * depth.max(0.0));
    let height = size * (1.0 - 0.56 * depth.max(0.0) - 0.75 * (-depth).max(0.0));
    Vec2::new(width, height)
}

#[derive(Default)]
struct ShotRings {
    // Ordered by birth; the first value is its expiry on the unwrapped clock.
    live: VecDeque<(f64, ShotRingUniform)>,
    space: Option<(u32, Option<u32>)>,
}

impl ShotRings {
    fn push(&mut self, entry: nvf_play::combat::ShotWaterEntry, now: f64, shader_time: f32) {
        use nvf_play::combat::WaterImpact;
        let (kind, lifetime, travel, blast_scale) = match entry.kind {
            WaterImpact::Blast(blast) => {
                let axis = Vec2::from_angle((entry.point[0] * 0.013 + entry.point[1] * 0.027 + shader_time).rem_euclid(std::f32::consts::TAU));
                (BLAST_RING, BLAST_RING_LIFETIME, axis, blast_surface_scale(blast))
            }
            WaterImpact::HeavyEntry => (1, SHOT_RING_LIFETIME, Vec3::from_array(entry.direction).truncate(), Vec2::ZERO),
            WaterImpact::Bullet => (0, SHOT_RING_LIFETIME, Vec3::from_array(entry.direction).truncate(), Vec2::ZERO),
        };
        if self.live.len() == MAX_SHOT_RINGS {
            // A burst of pistol fire must not cut off a grenade's receding wave.
            if let Some(index) = self.live.iter().position(|(_, ring)| ring.kind != BLAST_RING) {
                self.live.remove(index);
            } else if kind == BLAST_RING {
                self.live.pop_front();
            } else {
                return;
            }
        }
        self.live.push_back((now + lifetime, ShotRingUniform {
            point_time: Vec3::from_array(entry.point).extend(shader_time + 1.0),
            travel,
            receiver: entry.receiver,
            kind,
            blast_scale,
        }));
    }

    fn expire(&mut self, now: f64) {
        self.live.retain(|(expires, _)| *expires > now);
    }
}

/// Combat places each crossing at the waterline when the shot reaches it. The material
/// only needs its start time: expanding the ring costs no per-frame asset changes.
#[allow(clippy::too_many_arguments)]
fn update_shot_rings(
    time: Res<Time>,
    graphics: Res<crate::graphics::Graphics>,
    world: Res<WorldData>,
    streamer: Res<Streamer>,
    mut shots: ResMut<nvf_play::combat::ShotVisuals>,
    mut rings: Local<ShotRings>,
    cache: Res<WaterCache>,
    mut materials: ResMut<Assets<WaterMaterial>>,
) {
    if rings.live.is_empty() && shots.water_entries.is_empty() {
        return;
    }
    let space = (world.world, streamer.interior);
    if !graphics.water_effects || rings.space != Some(space) {
        rings.live.clear();
        rings.space = Some(space);
    }
    let now = time.elapsed_secs_f64();
    rings.expire(now);
    if !shots.water_entries.is_empty() {
        for entry in shots.water_entries.drain(..) {
            if graphics.water_effects {
                rings.push(entry, now, time.elapsed_secs_wrapped());
                let point = entry.point;
                let name = match entry.kind {
                    nvf_play::combat::WaterImpact::Bullet => "shot ring",
                    nvf_play::combat::WaterImpact::HeavyEntry => "heavy entry ring",
                    nvf_play::combat::WaterImpact::Blast(_) => "blast wave",
                };
                debug!("water: {name} at ({:.0}, {:.0}, {:.0})", point[0], point[1], point[2]);
            }
        }
    }
    let mut packed = [ShotRingUniform::default(); MAX_SHOT_RINGS];
    for (slot, (_, entry)) in packed.iter_mut().zip(&rings.live) {
        *slot = *entry;
    }
    // This also seeds materials streamed in while a ring is still travelling.
    for h in cache.materials.values() {
        if materials.get(h).is_some_and(|m| m.params.shot_rings != packed) {
            if let Some(mut m) = materials.get_mut(h) {
                m.params.shot_rings = packed;
            }
        }
    }
}

impl WaterCache {
    /// Water materials made (the stress census).
    pub fn len(&self) -> usize {
        self.materials.len()
    }

    /// Tell every water material which plane the reflection texture shows.
    pub fn set_reflection_plane(&self, materials: &mut Assets<WaterMaterial>, plane: Vec4) {
        for h in self.materials.values() {
            if materials.get(h).is_some_and(|m| m.params.reflection_plane != plane) {
                if let Some(mut m) = materials.get_mut(h) {
                    m.params.reflection_plane = plane;
                }
            }
        }
    }

    /// Tell every water material which water's drawn pixels to count ([`WaterParams::seen_plane`]).
    pub fn set_seen_plane(&self, materials: &mut Assets<WaterMaterial>, plane: Vec4) {
        for h in self.materials.values() {
            if materials.get(h).is_some_and(|m| m.params.seen_plane != plane) {
                if let Some(mut m) = materials.get_mut(h) {
                    m.params.seen_plane = plane;
                }
            }
        }
    }

    /// The water-pixel counter, once a water material exists.
    pub fn seen_counter(&self) -> Option<&Handle<ShaderBuffer>> {
        self.seen.as_ref()
    }

    /// Tell every water material where the ripple grid lies.
    pub fn set_ripples(&self, materials: &mut Assets<WaterMaterial>, ripple: Vec4) {
        for h in self.materials.values() {
            if materials.get(h).is_some_and(|m| m.params.ripple != ripple) {
                if let Some(mut m) = materials.get_mut(h) {
                    m.params.ripple = ripple;
                }
            }
        }
    }
}

/// The game's RemapWaterType remaps (old water type → new), copied each frame they change.
#[derive(Resource, Default, PartialEq)]
pub struct WaterRemaps(HashMap<u32, u32>);

impl WaterRemaps {
    pub fn resolve(&self, wid: u32) -> u32 {
        self.0.get(&wid).copied().unwrap_or(wid)
    }
}

fn sync_remaps(script: Option<Res<nvf_play::script::ScriptState>>, mut remaps: ResMut<WaterRemaps>) {
    let Some(g) = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game) else { return };
    let now = WaterRemaps(g.forms.water_remaps.iter().map(|(a, b)| (a.0, b.0)).collect());
    if *remaps != now {
        info!("water: {} water type(s) remapped (RemapWaterType)", now.0.len());
        *remaps = now;
    }
}

/// Water materials and the plane mesh, for spawning water.
#[derive(SystemParam)]
pub struct WaterCtx<'w> {
    remaps: Option<Res<'w, WaterRemaps>>,
    materials: ResMut<'w, Assets<WaterMaterial>>,
    cache: ResMut<'w, WaterCache>,
    reflection: Option<Res<'w, Reflection>>,
    ripples: Option<Res<'w, Ripples>>,
    buffers: ResMut<'w, Assets<ShaderBuffer>>,
}

impl WaterCtx<'_> {
    /// A cell-sized plane (game space, normal +z) centred on the origin.
    pub fn plane(&mut self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.cache
            .plane
            .get_or_insert_with(|| meshes.add(Mesh::from(Plane3d::new(Vec3::Z, Vec2::splat(CELL_SIZE / 2.0)))))
            .clone()
    }
}

/// The material for water type `wid` (built on first use). `reflects` false (a placed
/// water whose PWAT does not set "Reflects", `fnv_world::sky::placeable_water_reflects`)
/// draws the type without any reflection: no world, sky or reflection colour and no sun
/// highlight, only the water body [hypothesis, world.md §6 "Placed water"]. `placed`
/// (placed water, PWAT) caps the body's opacity seen from above at the above fog's
/// amount (`above_fog.w` = 1); cell and distant water stay opaque beyond the far plane,
/// as the cap left the lake bed showing far out in Lake Mead [design, until a far-out
/// Lake Mead comparison is made, world.md §6].
pub fn water_material(world: &WorldData, wid: u32, reflects: bool, placed: bool, exterior_position: Option<Vec3>, models: &mut ModelCtx, water: &mut WaterCtx) -> Option<Handle<WaterMaterial>> {
    let wid = water.remaps.as_ref().map_or(wid, |r| r.resolve(wid));
    let direction = exterior_position.map_or(FlowDirection::STILL, |pos| {
        surface_flow(world.index.worldspaces[&world.world].edid.as_deref(), pos.truncate(), pos.z)
    });
    let key = (wid, reflects, placed, direction);
    if let Some(m) = water.cache.materials.get(&key) {
        return Some(m.clone());
    }
    let w = fnv_world::sky::water(world.lo, world.schema, wid)?;
    // Linear: the channels are heights, not colours.
    let noise = w.noise_texture.as_deref().and_then(|t| {
        let a = &mut *models;
        a.textures.get(t, false, &a.data, &mut a.images, a.formats.as_deref())
    });
    let reflection = water.reflection.as_ref().map(|r| r.image.clone());
    let ripples = water.ripples.as_ref().map(|r| r.image.clone());
    let buffers = &mut water.buffers;
    let seen = water
        .cache
        .seen
        .get_or_insert_with(|| {
            let mut b = ShaderBuffer::from(vec![0u32; 4]);
            b.buffer_usage |= BufferUsages::COPY_SRC | BufferUsages::COPY_DST;
            buffers.add(b)
        })
        .clone();
    let mut params = water_params(&w);
    if !reflects {
        params.surface.x = 0.0;
    }
    if placed {
        params.above_fog.w = 1.0;
    }
    let wind_layers = params.layers;
    let current_layers = current_layers(wind_layers, direction.velocity());
    let m = water.materials.add(WaterMaterial { params, wind_layers, current_layers, noise, reflection, ripples, seen });
    water.cache.materials.insert(key, m.clone());
    Some(m)
}

/// Water planes beyond the loaded cells, over the cells of the distant terrain chunks
/// chosen now ([`LodState::chosen_chunks`]), so the water ends where the terrain does:
/// one per cell whose water surface lies above the lowest point of its landscape (or that
/// has no landscape). Not for a cell or worldspace flagged No LOD Water. Without them a
/// lake ends at the loaded cells and its LOD bed shows.
#[derive(Resource, Default)]
pub struct DistantWater {
    /// Per cell: its visible water surface (height, water type), or `None`.
    surfaces: HashMap<(i32, i32), Option<(f32, u32)>>,
    /// Per cell with a visible surface: the part of it where the water stands above the
    /// land, `[x0, y0, x1, y1]` in game units ([`DistantWater::wet_area`]).
    wet: HashMap<(i32, i32), [f32; 4]>,
    planes: HashMap<(i32, i32), Entity>,
    /// The worldspace's cells that can have distant water ([`water_candidates`]), found
    /// once per worldspace (its FormID).
    candidates: Option<(u32, BTreeSet<(i32, i32)>)>,
    /// The candidates under the chosen terrain chunks, loaded or not.
    want: Vec<(i32, i32)>,
    /// Worldspace, LOD choice ([`LodState::choice`]) and whether distant water is on, that
    /// `want` was found for.
    want_for: Option<WantKey>,
    /// `want_for` the planes were placed for.
    built_for: Option<WantKey>,
    /// Cells of `want` skipped by the read budget ([`SURFACE_READS_PER_FRAME`]), in `want`'s
    /// order: the only ones the frames after go through while neither the choice nor the
    /// loaded cells change (#3879: the whole list again was 16,000 cells a frame in Novac).
    unread: Vec<(i32, i32)>,
    /// The loaded cells the planes were placed for, sorted ([`loaded_changed`]), and a
    /// buffer for this frame's.
    loaded_seen: Vec<(i32, i32)>,
    loaded_now: Vec<(i32, i32)>,
}

type WantKey = (u32, u64, bool);

impl DistantWater {
    /// The visible water surface of cell `g` (height, water type), computed on first use.
    /// Whether [`DistantWater::surface`] of cell `g` is already known (no landscape read).
    pub fn surface_known(&self, g: (i32, i32)) -> bool {
        self.surfaces.contains_key(&g)
    }

    pub fn surface(&mut self, world: &WorldData, g: (i32, i32)) -> Option<(f32, u32)> {
        if let Some(s) = self.surfaces.get(&g) {
            return *s;
        }
        let v = visible_surface(world, g);
        if let Some((_, _, wet)) = v {
            self.wet.insert(g, wet);
        }
        let s = v.map(|(h, w, _)| (h, w));
        self.surfaces.insert(g, s);
        s
    }

    /// The part of cell `g` where its visible water stands above the land, `[x0, y0, x1,
    /// y1]` in game units: the box round the landscape's vertices under the surface,
    /// widened by one vertex step (the shore lies between a vertex under it and one
    /// above), or the whole cell without a landscape. `None` without a visible surface.
    pub fn wet_area(&mut self, world: &WorldData, g: (i32, i32)) -> Option<[f32; 4]> {
        self.surface(world, g)?;
        self.wet.get(&g).copied()
    }

    /// The cells that have a distant water plane now.
    pub fn plane_cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.planes.keys().copied()
    }

    /// Forget every surface and remove every plane (the worldspace changed).
    pub fn clear(&mut self, commands: &mut Commands) {
        for (_, e) in self.planes.drain() {
            commands.entity(e).despawn();
        }
        self.surfaces.clear();
        self.wet.clear();
        self.candidates = None;
        self.want.clear();
        self.want_for = None;
        self.built_for = None;
        self.loaded_seen.clear();
        self.loaded_now.clear();
    }

    /// The drawn worldspace's [`water_candidates`], found on first use.
    fn candidates(&mut self, world: &WorldData) -> &BTreeSet<(i32, i32)> {
        if self.candidates.as_ref().is_none_or(|(w, _)| *w != world.world) {
            let set = world.index.worldspaces.get(&world.world).map_or_else(BTreeSet::new, |ws| water_candidates(ws, &world.index.cells));
            self.candidates = Some((world.world, set));
        }
        &self.candidates.as_ref().expect("set above").1
    }
}

/// Cell `c`'s water surface (height, water type) as distant water would draw it in
/// worldspace `ws`, before the landscape is looked at. "No LOD Water" on the cell (CELL
/// DATA bit 3) or the worldspace (WRLD DATA bit 4): no distant water for it [documented:
/// xEdit flag names]. Zion Canyon sets it on its cells (7 of them had a plane at the 5000
/// default over a dip), and five worldspaces (Big MT, the Higgs Village and Lucky 38
/// worlds among them) on the whole world [observed].
fn lod_water(c: &fnv_world::Cell, ws: &fnv_world::Worldspace) -> Option<(f32, u32)> {
    if !c.has_water() || c.no_lod_water() || ws.data_flags & 0x10 != 0 {
        return None;
    }
    Some((c.water_surface(ws.default_water_height)?, c.water_type.or(ws.water)?))
}

/// The grid cells of worldspace `ws` that can have a distant water plane ([`lod_water`]
/// gives them a surface); whether the water stands above the land is found later, only
/// for the cells under the chosen terrain chunks ([`DistantWater::surface`]).
pub fn water_candidates(ws: &fnv_world::Worldspace, cells: &HashMap<u32, fnv_world::Cell>) -> BTreeSet<(i32, i32)> {
    ws.grid.iter().filter(|(_, id)| cells.get(id).is_some_and(|c| lod_water(c, ws).is_some())).map(|(&g, _)| g).collect()
}

/// The cells of `candidates` under the terrain chunks `chunks` (a level-`l` chunk at
/// `(x, y)` covers cells `x..x + l` by `y..y + l`). The chunks do not overlap
/// ([`crate::lod::select_chunks`]), so no cell comes twice; one range of the set per chunk
/// column, so cells without water cost nothing.
pub fn cells_under(chunks: &[crate::lod::Chunk], candidates: &BTreeSet<(i32, i32)>) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for &(l, x, y) in chunks {
        for cx in x..x + l {
            out.extend(candidates.range((cx, y)..(cx, y + l)).copied());
        }
    }
    out
}

/// Whether the loaded cells differ from `seen` (the set the planes were last placed for,
/// sorted): `now` is refilled from `loaded` and sorted; when they differ it becomes `seen`.
/// An exact comparison, so a cell unloading in the same frame another finishes loading is
/// seen too (the count alone stays the same) [design, #3055 review]; at most the loaded
/// grid's 25 to 81 cells, in buffers kept between frames.
pub fn loaded_changed(seen: &mut Vec<(i32, i32)>, now: &mut Vec<(i32, i32)>, loaded: impl Iterator<Item = (i32, i32)>) -> bool {
    now.clear();
    now.extend(loaded);
    now.sort_unstable();
    if now == seen {
        return false;
    }
    std::mem::swap(seen, now);
    true
}

fn visible_surface(world: &WorldData, g: (i32, i32)) -> Option<(f32, u32, [f32; 4])> {
    let c = world.grid_cell(g)?;
    let ws = &world.index.worldspaces[&world.world];
    let (height, wid) = lod_water(c, ws)?;
    let (x0, y0) = (g.0 as f32 * CELL_SIZE, g.1 as f32 * CELL_SIZE);
    let mut wet = [x0, y0, x0 + CELL_SIZE, y0 + CELL_SIZE];
    if let Some(land) = c.land {
        let heights = load_land(world.lo, world.schema, land, LandParts::HEIGHTS)?.heights?;
        // A smoothed landscape (#2149) can dip up to its bound below the stored vertices, so a
        // vertex that close above the water may have wet ground beside it [design, #2149].
        let slack = nvf_play::smooth_terrain::spec().bound();
        let [i0, j0, i1, j1] = wet_vertices(&heights, height + slack)?;
        let step = CELL_SIZE / (SIDE - 1) as f32;
        wet = [x0 + i0 as f32 * step, y0 + j0 as f32 * step, x0 + i1 as f32 * step, y0 + j1 as f32 * step];
    }
    Some((height, wid, wet))
}

/// The box `[col0, row0, col1, row1]` round the vertices of a `SIDE` × `SIDE` height grid
/// (row-major, rows south to north) that lie under `surface`, one vertex wider on every
/// side and kept inside the grid; `None` when none lies under it.
fn wet_vertices(heights: &[f32], surface: f32) -> Option<[usize; 4]> {
    let mut b: Option<[usize; 4]> = None;
    for (k, &h) in heights.iter().enumerate() {
        if h >= surface {
            continue;
        }
        let (col, row) = (k % SIDE, k / SIDE);
        b = Some(match b {
            None => [col, row, col, row],
            Some([c0, r0, c1, r1]) => [c0.min(col), r0.min(row), c1.max(col), r1.max(row)],
        });
    }
    b.map(|[c0, r0, c1, r1]| [c0.saturating_sub(1), r0.saturating_sub(1), (c1 + 1).min(SIDE - 1), (r1 + 1).min(SIDE - 1)])
}

/// What [`walk_with_reads`]' visit did with a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walked {
    /// Handled without a surface read.
    Done,
    /// Handled with one read of the budget.
    Read,
    /// It needs a read and none was left: kept for a later frame.
    Later,
}

/// Visits `cells` in order, `visit(cell, may_read)` told whether a surface read is still
/// within the frame's `reads`; returns the cells it put off ([`Walked::Later`]), in order,
/// for the frames after to visit instead of the whole list (#3879).
pub fn walk_with_reads(cells: &[(i32, i32)], reads: usize, mut visit: impl FnMut((i32, i32), bool) -> Walked) -> Vec<(i32, i32)> {
    let mut used = 0;
    let mut later = Vec::new();
    for &c in cells {
        match visit(c, used < reads) {
            Walked::Done => {}
            Walked::Read => used += 1,
            Walked::Later => later.push(c),
        }
    }
    later
}

/// The distant water's landscape reads in one frame ([`distant_water`]).
const SURFACE_READS_PER_FRAME: usize = 12;

#[allow(clippy::too_many_arguments)]
pub fn distant_water(
    mut commands: Commands,
    world: Res<WorldData>,
    streamer: Res<Streamer>,
    lod: Res<LodState>,
    root: Query<Entity, With<WorldRoot>>,
    mut dw: ResMut<DistantWater>,
    mut models: ModelCtx,
    mut water: WaterCtx,
    pace: Res<crate::stream_pace::StreamPace>,
) {
    let Ok(root) = root.single() else { return };
    let on = lod.enabled && streamer.interior.is_none();
    let want_key = (world.world, lod.choice(), on);
    let dwm = &mut *dw;
    let loaded = loaded_changed(&mut dwm.loaded_seen, &mut dwm.loaded_now, streamer.loaded.keys().copied());
    // A full pass when the choice or the loaded cells changed; otherwise only the cells
    // the read budget left for later, if any (#3879).
    let full = dw.built_for != Some(want_key) || loaded;
    if !full && dw.unread.is_empty() {
        return;
    }
    dw.built_for = Some(want_key);
    let _spending = pace.spending();
    // Above 60 fps a frame gets the same share of itself (stream_pace.rs).
    let reads_a_frame = pace.count(SURFACE_READS_PER_FRAME);
    // The cells with water under the terrain chunks chosen now (none while the LOD is off
    // or nothing is chosen), redone only when the choice changes.
    if dw.want_for != Some(want_key) {
        dw.want_for = Some(want_key);
        // No chosen chunk (both reaches 0): no water index is built either.
        dw.want = if on && !lod.chosen_chunks().is_empty() { cells_under(lod.chosen_chunks(), dw.candidates(&world)) } else { Vec::new() };
        let want: HashSet<(i32, i32)> = dw.want.iter().copied().collect();
        let gone: Vec<(i32, i32)> = dw.planes.keys().filter(|c| !want.contains(c)).copied().collect();
        for c in gone {
            if let Some(e) = dw.planes.remove(&c) {
                commands.entity(e).despawn();
            }
        }
    }
    // Loaded cells have their own water: their planes go, and come back when they unload.
    // The full pass goes through `want`; a later frame through what it left unread.
    let cells = if full { std::mem::take(&mut dw.want) } else { std::mem::take(&mut dw.unread) };
    let unread = walk_with_reads(&cells, reads_a_frame, |c, may_read| {
        if streamer.loaded.contains_key(&c) {
            if let Some(e) = dw.planes.remove(&c) {
                commands.entity(e).despawn();
            }
            return Walked::Done;
        }
        if dw.planes.contains_key(&c) {
            return Walked::Done;
        }
        // [design] #2956 follow-up (#3070): a cell's surface not known yet reads and
        // inflates its landscape record; a new chunk choice can uncover many at once (5-11 ms
        // of one frame [observed 2026-11-19, `profile hitch`]). At most
        // `SURFACE_READS_PER_FRAME` are read a frame, the rest the frames after.
        let read = !dw.surface_known(c);
        if read && !may_read {
            return Walked::Later;
        }
        let walked = if read { Walked::Read } else { Walked::Done };
        let Some((height, wid)) = dw.surface(&world, c) else { return walked };
        let Some(mat) = water_material(&world, wid, true, false, Some(cell_water_center(c, height)), &mut models, &mut water) else { return walked };
        let mesh = water.plane(&mut models.meshes);
        let s = CELL_SIZE;
        let e = commands
            .spawn((
                Name::new(format!("Distant water {},{}", c.0, c.1)),
                Mesh3d(mesh),
                MeshMaterial3d(mat),
                Transform::from_xyz(c.0 as f32 * s + s / 2.0, c.1 as f32 * s + s / 2.0, height),
                RenderLayers::layer(WATER_LAYER),
            ))
            .id();
        commands.entity(root).add_child(e);
        dw.planes.insert(c, e);
        walked
    });
    if full {
        dw.want = cells;
    }
    dw.unread = unread;
}

fn linear(c: [u8; 3]) -> Vec3 {
    Vec3::from_array(c.map(srgb_to_linear))
}

/// Shader parameters for a water type (sky colours are filled in by [`update_water_sky`]).
pub fn water_params(w: &Water) -> WaterParams {
    let layer = |l: &fnv_world::sky::NoiseLayer| {
        // Layer UV scale in game units per texture repeat; 0 (as in the masters) falls
        // back to the record's noise normals UV scale [hypothesis].
        let scale = if l.uv_scale > 0.0 { l.uv_scale } else { w.noise_normals_uv_scale.max(1.0) };
        // Wind direction in compass degrees, 0 = north (+y) [hypothesis]; speed in texture
        // repeats per second [hypothesis].
        let (sin, cos) = l.wind_direction.to_radians().sin_cos();
        Vec4::new(sin * l.wind_speed, cos * l.wind_speed, 1.0 / scale, l.amplitude)
    };
    WaterParams {
        shallow: linear(w.shallow).extend(1.0),
        deep: linear(w.deep).extend(1.0),
        reflection: linear(w.reflection).extend(if w.reflective { 1.0 } else { 0.0 }),
        sky_upper: Vec4::ONE,
        sky_horizon: Vec4::ONE,
        above_fog: Vec3::from_array(w.above_fog).extend(0.0),
        under_fog: Vec3::from_array(w.under_fog).extend(0.0),
        surface: Vec4::new(w.reflectivity, w.fresnel, w.shininess, w.noise_normals_scale * WAVE_HEIGHT_FACTOR),
        layers: [layer(&w.noise_layers[0]), layer(&w.noise_layers[1]), layer(&w.noise_layers[2])],
        reflection_plane: Vec4::ZERO,
        ripple: Vec4::ZERO,
        edge: w.edge_falloff().map_or(Vec4::ZERO, |(a, b)| Vec4::new(a, b, 1.0, 0.0)),
        seen_plane: Vec4::ZERO,
        water_sun: Vec4::ZERO,
        shot_rings: [ShotRingUniform::default(); MAX_SHOT_RINGS],
    }
}

/// The share of the current each noise layer travels at [design, by eye].
const LAYER_PACE: [f32; 3] = [1.0, 0.75, 1.3];

/// Bake advection once per material [design]. UVs add velocity over time, so
/// moving a crest downstream requires subtracting the physical current. Bound
/// each layer's wind to 40% of its current in UV units; opposing wind therefore
/// cannot reverse any layer. Scale and amplitude remain exactly as authored.
fn current_layers(wind_layers: [Vec4; 3], current: Vec2) -> [Vec4; 3] {
    let speed = current.length();
    if speed == 0.0 {
        return wind_layers;
    }
    let mut i = 0;
    wind_layers.map(|layer| {
        // Each layer rides the current at its own pace, so the surface changes shape
        // as it travels instead of sliding past as one rigid sheet.
        let current = current * LAYER_PACE[i];
        i += 1;
        let speed = current.length();
        let wind = layer.xy();
        let share = (0.4 * speed * layer.z / wind.length().max(1e-6)).min(1.0);
        let velocity = wind * share - current * layer.z;
        Vec4::new(velocity.x, velocity.y, layer.z, layer.w)
    })
}

pub fn cell_water_center((x, y): (i32, i32), height: f32) -> Vec3 {
    Vec3::new((x as f32 + 0.5) * CELL_SIZE, (y as f32 + 0.5) * CELL_SIZE, height)
}

/// The sky uses a directional light for the moon too. Only the sun can concentrate
/// daylight on a submerged receiver, and dark weather supplies less direct light.
pub fn caustic_sun(sky: &SkyState) -> Vec4 {
    if sky.interior.is_some() {
        return Vec4::ZERO;
    }
    let sun = sky.sun_dir();
    if sun.y <= 0.0 {
        return Vec4::ZERO;
    }
    let light = Vec3::from_array(sky.color(ColorType::Sunlight)).max_element().clamp(0.0, 1.0);
    sun.extend(light)
}

/// How far the caustics' sun must move (direction components, daylight) before the water
/// materials are written again: an explorer's running clock moved it every frame, and
/// each write prepares every water material anew.
const SUN_STEP: f32 = 2.0e-3;

/// Keep the reflected sky in step with the sky dome (weather colours at the current hour;
/// the fog colour indoors).
pub fn update_water_sky(sky: Res<SkyState>, graphics: Option<Res<crate::graphics::Graphics>>, cache: Res<WaterCache>, mut materials: ResMut<Assets<WaterMaterial>>) {
    let (upper, horizon) = match sky.interior {
        Some(l) => {
            let f = linear(l.fog_color);
            (f, f)
        }
        // Blended across a weather fade, like the dome.
        None => (Vec3::from_array(sky.color(ColorType::SkyUpper)), Vec3::from_array(sky.color(ColorType::Horizon))),
    };
    let (upper, horizon) = (upper.extend(1.0), horizon.extend(1.0));
    let effects = graphics.is_some_and(|g| g.water_effects);
    let sun = if effects { caustic_sun(&sky) } else { Vec4::ZERO };
    for h in cache.materials.values() {
        let stale = materials.get(h).is_some_and(|m| {
            let layers = if effects { m.current_layers } else { m.wind_layers };
            m.params.sky_upper != upper || m.params.sky_horizon != horizon || !m.params.water_sun.abs_diff_eq(sun, SUN_STEP) || (m.params.water_sun == Vec4::ZERO) != (sun == Vec4::ZERO)
                || m.params.layers != layers || m.params.under_fog.w != f32::from(effects)
        });
        if stale {
            if let Some(mut m) = materials.get_mut(h) {
                m.params.sky_upper = upper;
                m.params.sky_horizon = horizon;
                m.params.water_sun = sun;
                m.params.under_fog.w = f32::from(effects);
                m.params.layers = if effects { m.current_layers } else { m.wind_layers };
            }
        }
    }
}

#[cfg(test)]
mod wet_tests {
    use super::*;

    #[test]
    fn shot_rings_keep_the_latest_entries_and_expire_across_the_shader_clock_wrap() {
        let mut rings = ShotRings::default();
        for i in 0..=MAX_SHOT_RINGS {
            let now = 3599.0 + i as f64 * 0.1;
            let entry = nvf_play::combat::ShotWaterEntry { point: [i as f32, 1.0, 2600.0], direction: [0.6, 0.0, -0.8], receiver: 0xff01_0000 + i as u32, kind: nvf_play::combat::WaterImpact::Bullet };
            rings.push(entry, now, now as f32);
        }
        assert_eq!(rings.live.len(), MAX_SHOT_RINGS);
        let first = rings.live.front().unwrap().1;
        assert_eq!(first.point_time.xyz(), Vec3::new(1.0, 1.0, 2600.0));
        assert_eq!(first.receiver, 0xff01_0001);
        assert_eq!(first.travel, Vec2::new(0.6, 0.0));
        rings.expire(3600.41);
        assert_eq!(rings.live.front().unwrap().1.point_time.x, 3.0);
        rings.expire(3601.1);
        assert!(rings.live.is_empty());
    }

    #[test]
    fn submerged_blast_is_wider_and_lower_and_airburst_loses_lift() {
        use nvf_play::combat::SurfaceBlast;
        let surface = SurfaceBlast { radius: 600.0, depth: 0.0, reach: 250.0 };
        let on = blast_surface_scale(surface);
        let below = blast_surface_scale(SurfaceBlast { depth: 180.0, ..surface });
        let above = blast_surface_scale(SurfaceBlast { depth: -180.0, ..surface });
        assert_eq!(on, Vec2::ONE);
        assert!(below.x > on.x && below.y < on.y);
        assert!(above.x == on.x && above.y < below.y);
        assert!(blast_surface_scale(SurfaceBlast { radius: 800.0, ..surface }).x > on.x);
    }

    #[test]
    fn blast_wave_survives_following_gunfire_and_uses_its_own_lifetime() {
        use nvf_play::combat::{ShotWaterEntry, SurfaceBlast, WaterImpact};
        let mut rings = ShotRings::default();
        let blast = SurfaceBlast { radius: 200.0, depth: 80.0, reach: 250.0 };
        let entry = ShotWaterEntry { point: [12.0, 34.0, 90.0], direction: [0.0; 3], receiver: 44, kind: WaterImpact::Blast(blast) };
        rings.push(entry, 0.0, 0.0);
        for _ in 0..MAX_SHOT_RINGS {
            rings.push(ShotWaterEntry { kind: WaterImpact::HeavyEntry, ..entry }, 0.1, 0.1);
        }
        assert_eq!(rings.live.len(), MAX_SHOT_RINGS);
        rings.expire(1.4);
        assert_eq!(rings.live.len(), 1);
        let wave = rings.live[0].1;
        assert_eq!(wave.kind, BLAST_RING);
        assert_eq!(wave.receiver, 44);
        assert_eq!(wave.blast_scale, blast_surface_scale(blast));
        rings.expire(BLAST_RING_LIFETIME);
        assert!(rings.live.is_empty());
    }

    #[test]
    fn baked_current_keeps_each_layer_downstream_against_strong_wind() {
        let wind = [
            Vec4::new(0.0, 900.0, 0.01, 0.7),
            Vec4::new(500.0, 500.0, 0.002, 1.2),
            Vec4::new(-300.0, -900.0, 0.0005, 0.3),
        ];
        let current = Vec2::new(0.0, 40.0);
        for ((original, baked), pace) in wind.into_iter().zip(current_layers(wind, current)).zip(LAYER_PACE) {
            assert_eq!(baked.z, original.z);
            assert_eq!(baked.w, original.w);
            let current = current * pace;
            let physical_velocity = -baked.xy() / baked.z;
            assert!(physical_velocity.dot(current.normalize()) >= 0.6 * current.length() - 0.001);
            let wind_contribution = baked.xy() + current * baked.z;
            assert!(wind_contribution.length() <= 0.4 * current.length() * baked.z + 1e-6);
        }
    }

    #[test]
    fn still_current_preserves_original_watr_layers_exactly() {
        let wind = [Vec4::new(1.5, -2.5, 0.001, 0.7), Vec4::ZERO, Vec4::ONE];
        assert_eq!(current_layers(wind, Vec2::ZERO), wind);
        let calm = [Vec4::new(0.0, 0.0, 0.01, 0.7); 3];
        for (layer, pace) in current_layers(calm, Vec2::new(0.0, 40.0)).into_iter().zip(LAYER_PACE) {
            assert!(layer.abs_diff_eq(Vec4::new(0.0, -0.4 * pace, 0.01, 0.7), 1e-6));
        }
    }

    #[test]
    fn wet_box_is_the_dip_one_vertex_wider() {
        let mut h = vec![100.0; SIDE * SIDE];
        // A pond under 50 at columns 10-12, rows 20-21.
        for row in 20..=21 {
            for col in 10..=12 {
                h[row * SIDE + col] = 0.0;
            }
        }
        assert_eq!(wet_vertices(&h, 50.0), Some([9, 19, 13, 22]));
        assert_eq!(wet_vertices(&h, -1.0), None);
        // At the grid's edge it stays inside the grid.
        h[0] = 0.0;
        assert_eq!(wet_vertices(&h, 50.0), Some([0, 0, 13, 22]));
    }
}

#[cfg(test)]
mod distant_tests {
    use super::*;
    use crate::lod::{select_chunks, LodSettings};

    /// A swapped loaded cell is a change though the count stays; the order does not matter;
    /// the same set again is no change.
    #[test]
    fn a_swapped_loaded_cell_is_a_change() {
        let (mut seen, mut now) = (Vec::new(), Vec::new());
        assert!(!loaded_changed(&mut seen, &mut now, std::iter::empty()), "nothing loaded, nothing seen");
        assert!(loaded_changed(&mut seen, &mut now, [(1, 0), (0, 0), (0, 1)].into_iter()));
        assert_eq!(seen, vec![(0, 0), (0, 1), (1, 0)]);
        assert!(!loaded_changed(&mut seen, &mut now, [(0, 1), (1, 0), (0, 0)].into_iter()), "same cells, another order");
        assert!(loaded_changed(&mut seen, &mut now, [(0, 0), (1, 0), (-1, 1)].into_iter()), "one swapped, same count");
        assert!(loaded_changed(&mut seen, &mut now, [(-49, 3), (-49, 7)].into_iter()));
        assert!(loaded_changed(&mut seen, &mut now, [(-47, 3), (-47, 7)].into_iter()), "two swapped for two");
    }

    /// #3879: with 12 reads a frame, 40 cells not read yet take four frames, and each frame
    /// after the first visits only the cells put off, not the whole list again; cells
    /// handled without a read (known, loaded, with a plane) never use the budget.
    #[test]
    fn a_frame_after_the_budget_visits_only_the_cells_put_off() {
        let cells: Vec<(i32, i32)> = (0..50).map(|i| (i, 0)).collect();
        // Every fifth cell's surface is known already; the rest need a read.
        let mut known: std::collections::HashSet<(i32, i32)> = cells.iter().copied().filter(|c| c.0 % 5 == 0).collect();
        let mut visits = Vec::new();
        let mut walk = |list: &[(i32, i32)], known: &mut std::collections::HashSet<(i32, i32)>| {
            let mut visited = 0;
            let later = walk_with_reads(list, 12, |c, may_read| {
                visited += 1;
                if known.contains(&c) {
                    return Walked::Done;
                }
                if !may_read {
                    return Walked::Later;
                }
                known.insert(c);
                Walked::Read
            });
            visits.push(visited);
            later
        };
        let mut left = walk(&cells, &mut known);
        assert_eq!(left.len(), 40 - 12, "the known 10 and 12 read; the rest put off");
        assert_eq!(left, cells.iter().copied().filter(|c| c.0 % 5 != 0).skip(12).collect::<Vec<_>>(), "in the list's order");
        let mut frames = 1;
        while !left.is_empty() {
            left = walk(&left.clone(), &mut known);
            frames += 1;
        }
        assert_eq!(frames, 4, "40 reads at 12 a frame");
        assert_eq!(visits, vec![50, 28, 16, 4], "each later frame visits only what was put off");
        assert_eq!(known.len(), 50);
    }

    /// A chunk's cells are `x..x + l` by `y..y + l`: the range per column takes no
    /// neighbour.
    #[test]
    fn cells_under_a_chunk_are_its_footprint() {
        let candidates: BTreeSet<(i32, i32)> = [(0, 0), (3, 3), (4, 0), (0, 4), (-1, 2), (2, -1)].into_iter().collect();
        assert_eq!(cells_under(&[(4, 0, 0)], &candidates), vec![(0, 0), (3, 3)]);
        assert!(cells_under(&[], &candidates).is_empty());
    }

    /// From the hill above Goodsprings (#3055, WastelandNV's grid from (-64, -64)) the
    /// camera's cell is (-16, 1) (-65,347 / 4,096 = -15.95, 5,770 / 4,096 = 1.41); the old
    /// square of 16 × 4 + 3 = 67 cells round it ended at x = 51, while the chosen level-32
    /// chunk (32, 32, 32) (232,983 away, lod.rs `level_32_from_the_goodsprings_hill`)
    /// covers x and y 32..=63, so lakes at (60, 40) and (63, 63) get their planes. From
    /// (0, 0) the old square held (63, 63), but (32, 32, 32) is nearer than its split
    /// distance (√2 × 131,072 ≈ 185,364 < 196,608) and its quarter (16, 48, 48) starts
    /// √2 × 196,608 ≈ 278,046 away, past the 250,000 reach: no chunk covers (63, 63), so
    /// no plane. (60, 40) lies in (16, 48, 32), √(196,608² + 131,072²) ≈ 236,294 away:
    /// chosen.
    #[test]
    fn water_cells_follow_the_chosen_chunks() {
        let s = LodSettings::default();
        let candidates: BTreeSet<(i32, i32)> = [(60, 40), (63, 63), (-16, 1), (2000, 2000)].into_iter().collect();
        let sorted = |mut v: Vec<(i32, i32)>| {
            v.sort_unstable();
            v
        };
        let hill = select_chunks(Vec2::new(-65_347.0, 5_770.0), &s, (-64, -64));
        assert_eq!(sorted(cells_under(&hill, &candidates)), vec![(-16, 1), (60, 40), (63, 63)]);
        let p = Vec2::ZERO;
        let centre = select_chunks(p, &s, (0, 0));
        assert!(centre.contains(&(16, 48, 32)), "{centre:?}");
        assert!(!centre.iter().any(|&(l, x, y)| (x..x + l).contains(&63) && (y..y + l).contains(&63)), "{centre:?}");
        assert_eq!(sorted(cells_under(&centre, &candidates)), vec![(-16, 1), (60, 40)]);
        // Nothing chosen (both distances 0, or the LOD off): no water cell at all.
        let none = select_chunks(p, &LodSettings { far: 0.0, terrain_far: 0.0, ..s }, (0, 0));
        assert!(none.is_empty());
        assert!(cells_under(&none, &candidates).is_empty());
    }

    fn cell(id: u32, g: (i32, i32), data_flags: u8, water_height: Option<f32>, water_type: Option<u32>) -> fnv_world::Cell {
        fnv_world::Cell { form_id: id, grid: Some(g), data_flags, water_height, water_type, ..Default::default() }
    }

    /// Only cells with the "has water" flag, a surface height (their own or the world's
    /// default), a water type (theirs or the world's) and no No LOD Water flag are
    /// candidates; none in a world flagged No LOD Water.
    #[test]
    fn candidates_are_the_cells_with_lod_water() {
        let cells: HashMap<u32, fnv_world::Cell> = [
            cell(1, (0, 0), 2, Some(100.0), Some(0x18)),
            cell(2, (1, 0), 0, Some(100.0), Some(0x18)),
            cell(3, (2, 0), 2 | 8, Some(100.0), Some(0x18)),
            cell(4, (3, 0), 2, Some(3.4e38), None),
        ]
        .into_iter()
        .map(|c| (c.form_id, c))
        .collect();
        // Grid square (4, 0) names a cell that is not in the index.
        let grid = [((0, 0), 1), ((1, 0), 2), ((2, 0), 3), ((3, 0), 4), ((4, 0), 5)].into_iter().collect();
        let ws = fnv_world::Worldspace { default_water_height: Some(0.0), water: Some(0x18), grid, ..Default::default() };
        assert_eq!(water_candidates(&ws, &cells).into_iter().collect::<Vec<_>>(), vec![(0, 0), (3, 0)]);
        // Without the world's default height and water type, cell 4 has neither.
        let bare = fnv_world::Worldspace { default_water_height: None, water: None, ..ws.clone() };
        assert_eq!(water_candidates(&bare, &cells).into_iter().collect::<Vec<_>>(), vec![(0, 0)]);
        let no_lod_water = fnv_world::Worldspace { data_flags: 0x10, ..ws };
        assert!(water_candidates(&no_lod_water, &cells).is_empty());
    }
}

#[cfg(test)]
#[path = "water_lifecycle_tests.rs"]
mod lifecycle_tests;
