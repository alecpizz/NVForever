//! The volumetric medium's light in the air (LootJunkieAzza/NVForever#3032, stage 3): the
//! plugin, the per-frame camera state, its extraction, the froxel volume and the grid pass.
//! An engineering choice of NVForever's own for the enhanced look: an addition of NVForever's
//! own; not a statement about the original's frames, and never used to explain a difference
//! from them [design]. Off unless the Volumetric light row or `NVF_PLAY_DEBUG_MEDIUM_LIGHT` asks for it
//! ([`AirWanted`]).
//!
//! **What runs** [design]:
//! - Main world, PostUpdate: [`follow_air`] keeps an [`AirCamera`] on the main camera
//!   (`FlyCam`) while [`AirWanted`] is true, holding the grid, the tune, the camera's
//!   effective fog, the direction to the sun or moon light and whether the camera is
//!   outdoors and above water; it opens and closes the render side's [`AirGate`].
//! - Render world, extract: [`extract_air`] copies the component to the camera's view.
//! - Render world, PrepareResources: [`drain_air`] takes the volume off every render entity
//!   that is no longer a drawable view (no [`AirCamera`], or no `ExtractedView`: Bevy takes
//!   that off an inactive camera) and keeps the render side running while any render entity
//!   still holds either component, so switching the light off after a camera went inactive
//!   still drains it; then [`prepare_air`] makes the volume (one `Rgba16Float` 3D texture,
//!   `nx x ny x nz`, storage and sampled), the uniform ([`AirGpuUniform`]) and asks for the
//!   pipelines, for views whose projection and fog it can read (else their volume goes and
//!   one "not drawn" line says why).
//! - Render world, PrepareResourcesFlush, after sun_visibility.rs `prepare_sun_visibility`:
//!   [`prepare_air_volume`] takes the surfaces' cloud shadow of this frame (`CloudVolumeFrame`:
//!   the source, the light volume's window and strength, its two levels) for the grid's K, with
//!   the light volume's variant of the grid ([`AIR_VOLUME_DEF`], bindings 8-11, asked for only
//!   where the surfaces may read the volume) and its mean pass.
//! - `Core3d`, after `Core3dSystems::MainPass` and before `Core3dSystems::EarlyPostProcess`,
//!   chained: [`air_grid_pass`] (compute, `medium_air_grid.wesl`, span `nvf_medium_air`), with
//!   `FILTER` on (the default) [`air_filter_pass`] (compute, `medium_air_filter.wesl`, span
//!   `nvf_medium_air_filter`: a 3 x 3 tent in x and y within each slice into a second volume,
//!   the one the composite then reads), then medium_air_compose.rs
//!   `air_compose_pass` (span `nvf_medium_air_compose`).
//!
//! **The grid** [documented: Wronski 2014, "Volumetric Fog", SIGGRAPH 2014 Advances, for
//! froxel grids; Bauer 2019, "Creating the Atmospheric World of Red Dead Redemption 2",
//! SIGGRAPH 2019 Advances, slides 42-48, for the 160x88x64 grid marched front to back per
//! column]: one compute invocation per column (8 x 8 x 1 workgroups), or on a discrete NVIDIA
//! card four (8 x 8 x 4), each walking a quarter of its slices front to back, their sums joined
//! in order through workgroup memory after a barrier (medium_air_grid.wesl, "The chunks";
//! [`GridWalk`]), storing the sums to the slice's far boundary
//! (r = N = sum of dF x V, g = M = sum of dF, b the slice's mean V; each visibility sample carries
//! its own depth stratum's fog step). The geometry and the fog share are medium_air.rs's (pure functions and their tests); the CPU
//! mirror of the loop, its chunks and their join is this file's test `column_mirror_at`.
//!
//! **Off means off** [design]: with [`AirWanted`] false nothing is allocated, no pipeline is
//! asked for and nothing is logged; the render systems stop at their run condition (the gate
//! closed and nothing held). When it turns off the render side releases the volume and the
//! uniform in the next frame, then closes too.
//!
//! The pipeline gate (pipeline_gate.rs) counts every pipeline in Bevy's cache, these two
//! included, so a loading screen waits for them like any other once they are asked for.

use crate::fly::FlyCam;
use crate::medium_air::{self, AirFog, AirGrid, AirTune, AirView, AirWalkAid, NormedPhase};
use crate::medium_air_demand::{AirDemandPlugin, AirWanted};
use crate::sun_visibility::{CloudSourceInUse, CloudVolumeFrame, SunVisibilityUniform, MASK_CLOUD_VOLUME, MASK_CLOUD_VOLUME_ONLY};
use crate::volumetric_clouds_lightvol::LightVolumeWindow;
use bevy::camera::MainPassResolutionOverride;
use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::pbr::{DistanceFog, FogFalloff, GpuLights, LightMeta, ShadowSamplers, ViewLightsUniformOffset, ViewShadowBindings};
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{sampler, storage_buffer_sized, texture_2d, texture_2d_array, texture_3d, texture_storage_3d, uniform_buffer};
use bevy::render::render_resource::{
    AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer, BufferDescriptor, BufferUsages, CachedComputePipelineId, CachedPipelineState, ComputePassDescriptor,
    ComputePipeline, ComputePipelineDescriptor,
    Extent3d, FilterMode, PipelineCache, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, StorageTextureAccess, Texture, TextureDescriptor, TextureDimension, TextureFormat,
    TextureFormatFeatureFlags, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor, UniformBuffer,
};
use bevy::render::renderer::{RenderAdapter, RenderAdapterInfo, RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::sync_world::RenderEntity;
use bevy::render::texture::GpuImage;
use bevy::render::view::ExtractedView;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::shader::ShaderCacheError;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

/// The volume's format: four half floats (N = sum of dF x V, M = sum of dF, the slice's own
/// V, 1) [the phase taken out].
pub const AIR_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// Bytes per texel of [`AIR_FORMAT`].
const AIR_TEXEL_BYTES: u64 = 8;
/// The compute workgroups' side in columns (8 x 8), as medium_air_grid.wesl and
/// medium_air_filter.wesl declare it.
const WORKGROUP: u32 = 8;
/// The grid's chunks: the invocations that share a column's slices, the workgroup's z (the
/// dispatch is one group deep), as medium_air_grid.wesl's `AIR_CHUNKS`.
const CHUNKS: u32 = 4;
/// The chunked workgroup: its invocations, and the bytes of workgroup memory (one vec2 of f32
/// an invocation).
const GRID_INVOCATIONS: u32 = WORKGROUP * WORKGROUP * CHUNKS;
const GRID_SHARED_BYTES: u32 = GRID_INVOCATIONS * 8;

/// NVIDIA's PCI vendor id, as wgpu's `AdapterInfo::vendor` gives it.
const NVIDIA: u32 = 0x10DE;

/// How the grid pass walks a column's slices: a variant of the shader each, chosen by the
/// adapter ([`GridWalk::choose`]) and the grid's slices and specialised (the kept slices are an
/// array whose size the pipeline fixes, the smallest that holds the grid's) [design].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum GridWalk {
    /// One invocation a column (8 x 8 x 1): every adapter the chunks are not measured faster on.
    #[default]
    Serial,
    /// Four chunks a column of at most 16 slices: grids of up to 64 slices, the default's 64.
    Chunks,
    /// Four chunks of at most 32: the asked grids of more, up to 128.
    LongChunks,
}

impl GridWalk {
    #[cfg(test)]
    pub(crate) const ALL: [GridWalk; 3] = [GridWalk::Serial, GridWalk::Chunks, GridWalk::LongChunks];

    /// The walk for a grid of `nz` slices, and why: the start aid's; else the chunks on a
    /// discrete NVIDIA adapter alone (about twice as fast on an RTX 4080 SUPER, up to 1.75 times
    /// as dear on an AMD 780M [observed: docs/design/volumetric-medium.md, "Cost and memory"];
    /// no discrete AMD or Intel card is measured); never where their workgroup does not fit
    /// (`fits`, [`workgroup_fits`]). `adapter`: its vendor and type, when known.
    fn choose(aid: AirWalkAid, adapter: Option<(u32, wgpu::DeviceType)>, fits: bool, nz: u32) -> (GridWalk, &'static str) {
        match (aid, adapter) {
            (AirWalkAid::Serial, _) => (GridWalk::Serial, "the debug aid"),
            (AirWalkAid::Chunks, _) if !fits => (GridWalk::Serial, "the debug aid asks for chunks, their workgroup is over the adapter's limits"),
            (AirWalkAid::Chunks, _) => (GridWalk::chunked(nz), "the debug aid"),
            (AirWalkAid::Auto, Some((NVIDIA, wgpu::DeviceType::DiscreteGpu))) if !fits => (GridWalk::Serial, "the chunks' workgroup is over the adapter's limits"),
            (AirWalkAid::Auto, Some((NVIDIA, wgpu::DeviceType::DiscreteGpu))) => (GridWalk::chunked(nz), "a discrete NVIDIA adapter"),
            (AirWalkAid::Auto, Some(_)) => (GridWalk::Serial, "not a discrete NVIDIA adapter"),
            (AirWalkAid::Auto, None) => (GridWalk::Serial, "the adapter is not known"),
        }
    }

    /// The chunks for a grid of `nz` slices.
    fn chunked(nz: u32) -> GridWalk {
        if nz <= CHUNKS * GridWalk::Chunks.chunk_slices() { GridWalk::Chunks } else { GridWalk::LongChunks }
    }

    /// The shader defs.
    pub(crate) fn defs(self) -> &'static [&'static str] {
        match self {
            GridWalk::Serial => &[],
            GridWalk::Chunks => &["AIR_CHUNKED"],
            GridWalk::LongChunks => &["AIR_CHUNKED", "AIR_LONG_CHUNKS"],
        }
    }

    /// The shader's `AIR_CHUNK_SLICES`: the most slices a chunk walks (every grid's slices fit
    /// in its walk's chunks).
    const fn chunk_slices(self) -> u32 {
        match self {
            GridWalk::Serial => medium_air::AIR_SLICES.1,
            GridWalk::Chunks => 16,
            GridWalk::LongChunks => 32,
        }
    }

    /// The walk in the log.
    fn name(self) -> &'static str {
        match self {
            GridWalk::Serial => "one walk a column",
            GridWalk::Chunks => "four chunks of at most 16 slices a column",
            GridWalk::LongChunks => "four chunks of at most 32 slices a column",
        }
    }
}

const _: () = assert!(medium_air::AIR_SLICES.1 <= CHUNKS * GridWalk::LongChunks.chunk_slices());
const _: () = assert!(medium_air::AIR_GRID[2] <= CHUNKS * GridWalk::Chunks.chunk_slices());

/// The start aids, read once [design: medium_air.rs's parsers].
#[derive(Clone, Copy)]
struct AirAids {
    tune: AirTune,
    grid: Option<[u32; 3]>,
    walk: AirWalkAid,
}

static AIDS: LazyLock<AirAids> = LazyLock::new(|| {
    let var = crate::dev_aids::var;
    let tune = AirTune::start(
        medium_air::debug_strength(var(medium_air::ENV_STRENGTH).as_deref()),
        medium_air::debug_view(var(medium_air::ENV_VIEW).as_deref()),
        medium_air::debug_occluders(var(medium_air::ENV_OCCLUDERS).as_deref()),
        medium_air::debug_depth(var(medium_air::ENV_DEPTH).as_deref()),
    );
    AirAids { tune, grid: medium_air::debug_grid(var(medium_air::ENV_GRID).as_deref()), walk: medium_air::debug_walk(var(medium_air::ENV_WALK).as_deref()) }
});

/// The start aids; a build without the developer aids (dev_aids.rs) has their defaults, without a look
/// at the static.
#[inline]
fn aids() -> AirAids {
    if crate::dev_aids::ON { *AIDS } else { AirAids { tune: AirTune::default(), grid: None, walk: AirWalkAid::Auto } }
}

/// A view's name in the log, by its shader code (medium_air.rs `AirView::code`, so a view
/// added there needs no arm here to compile; an unknown code reads "other").
pub fn view_name(view: AirView) -> &'static str {
    match view.code() {
        0 => "none",
        1 => "visibility",
        2 => "slice",
        3 => "share",
        4 => "delta",
        5 => "off",
        6 => "lit",
        _ => "other",
    }
}

/// The render side's gate, shared by both worlds: its systems run only while it is open or
/// while they still hold something to release.
#[derive(Resource, Clone, Default)]
pub struct AirGate(Arc<AtomicBool>);

impl AirGate {
    pub fn is_open(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn set_open(&self, open: bool) {
        if self.is_open() != open {
            self.0.store(open, Ordering::Release);
        }
    }
}

/// What the render world needs of the main camera each frame (main world on the `FlyCam`
/// while the light is wanted; copied onto its view in the render world).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct AirCamera {
    /// The grid for the main pass's aspect and the aids.
    pub grid: AirGrid,
    /// The look's constants (the start aids; the tune file).
    pub tune: AirTune,
    /// The camera's effective fog: near, far, power, strength (the colour's alpha).
    pub fog: AirFog,
    /// The fog colour, linear rgb.
    pub fog_colour: Vec3,
    /// The fog blends in display values (fog.rs `bi.x > 0.5`, outdoors and with `--fog`).
    pub display_blend: bool,
    /// The direction to the sun or moon light in world space (the `sky::Sun` light's
    /// transform, its night reversal included); zero without one.
    pub to_light: Vec3,
    /// A sun light exists and the camera is outdoors and above water.
    pub has_light: bool,
    /// Outdoors (no interior cell lighting) and not under water.
    pub outdoors: bool,
    /// The scaled sunlit share s this frame (medium_air.rs `scaled_share`, set by
    /// medium_air_light.rs; 0 until it ran).
    pub share: f32,
    /// The light's gate passed this frame (medium_air.rs `gate`, set by medium_air_light.rs).
    pub draws: bool,
}

/// The camera's effective fog as the world shaders get it (fog.rs `falloff`): the game's curve
/// (`Atmospheric`: extinction near, far, power; inscattering x the display-value blend), Bevy's
/// linear fallback (power 1, linear blend), anything else a fog of strength 0 (not usable).
/// Strength from the colour's alpha, the colour's linear rgb [verified: fog.rs, sky.rs].
pub fn camera_air_fog(fog: Option<&DistanceFog>) -> (AirFog, Vec3, bool) {
    let none = AirFog { near: 0.0, far: 0.0, power: 1.0, strength: 0.0 };
    let Some(fog) = fog else { return (none, Vec3::ZERO, false) };
    let colour = fog.color.to_linear();
    let rgb = Vec3::new(colour.red, colour.green, colour.blue);
    let strength = colour.alpha;
    match fog.falloff {
        FogFalloff::Atmospheric { extinction, inscattering } => (AirFog { near: extinction.x, far: extinction.y, power: extinction.z, strength }, rgb, inscattering.x > 0.5),
        FogFalloff::Linear { start, end } => (AirFog { near: start, far: end, power: 1.0, strength }, rgb, false),
        _ => (none, rgb, false),
    }
}

/// The uniform both passes read, medium_air_grid.wesl's and medium_air_compose.wesl's
/// `AirUniform` field for field.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct AirGpuUniform {
    /// The camera's world-from-view transform (unjittered).
    pub world_from_view: Mat4,
    /// x, y: the projection's x and y scale; z: its near plane; w: the sky's distance.
    pub proj: Vec4,
    /// medium_air.rs `AirUniformData`'s groups.
    pub grid: Vec4,
    pub curve: Vec4,
    pub fog: Vec4,
    pub phase: Vec4,
    pub phase2: Vec4,
    /// xyz: the direction to the light (world); w: 0 when no light is to be drawn, else
    /// 2 - FLOOR (1 at the default FLOOR 1; medium_air.rs `light_flag`).
    pub light: Vec4,
    /// rgb: the fog colour (linear); w: 1 for the display-value blend.
    pub fog_colour: Vec4,
    /// x: the occluders' code; y: the visibility samples per froxel (the tune's `SAMPLES`,
    /// default [`medium_air::AIR_SAMPLES_DEFAULT`]; the shader holds it to 1..8); z: the sky weight
    /// (SKY); w: SKYPHASE.
    pub misc: Vec4,
    /// The main pass's viewport: origin and size in pixels.
    pub viewport: Vec4,
    /// The terrain map's block ([`terrain_block`]): world xz to uv (scale xy, offset zw) ...
    pub terrain_uv: Vec4,
    /// ... x valid for the current light, y the dot of its light with the current one, z the
    /// texel size, w the side ...
    pub terrain: Vec4,
    /// ... and the light it was computed for (xyz, towards the light).
    pub terrain_light: Vec4,
    /// The Cloud shadows image's block ([`cloud_block`]), as sun_visibility.rs packs it for
    /// surfaces (K, or its fallback where the light volume has no data): world xz to uv (scale xy, offset zw) ...
    pub cloud_uv: Vec4,
    /// ... the plane's height, the strength (held to 0..1), the light elevation's sines where
    /// the shadow starts and where it is whole ...
    pub cloud: Vec4,
    /// ... and x: 1 while the image is published and on the GPU (else K is 1); y: the mean of K
    /// over the pattern's repeat for the current light ([`cloud_mean_k`], 1 when not in use), the
    /// reference's; z: the clouds' own mean transmittance ([`flat_deck`], 1 with none), the deck's
    /// share's; w free. The mean pass copies the light volume's y and z over them on the GPU while the
    /// grid reads the volume ([`CLOUD_MEAN_OFFSET`], [`CLOUD_MEAN_COPY`]).
    pub cloud_mode: Vec4,
    /// The composite's (medium_air.rs `relative_lit`; the grid shader does not declare it):
    /// x LIFT, y the REF override (-1 for auto, the clouds' mean `cloud_mode.y`), z SHADE, w 1 while
    /// the tune's SHARE is set (medium_air.rs `deck_share` is then 1), else 0.
    pub rays2: Vec4,
}

/// The view's side of the uniform.
#[derive(Clone, Copy, Debug)]
pub struct AirViewInputs {
    pub world_from_view: Mat4,
    /// Bevy's reverse infinite perspective (`ExtractedView::clip_from_view`, unjittered).
    pub clip_from_view: Mat4,
    /// The viewport's origin in the target (pixels).
    pub origin: UVec2,
    /// The main pass's size (`MainPassResolutionOverride`, else the viewport's).
    pub render: UVec2,
}

/// Why a view's inputs cannot be drawn, or none [design]: both shaders take Bevy's
/// reverse infinite perspective (`ExtractedView::clip_from_view`'s documented form: clip w =
/// -view z, no z scale, the near term in w_axis.z) with x and y scales above 0 (they divide
/// NDC by them) and a near term above 0 (view depth = near / depth), all finite; and finite
/// fog numbers and colour (a NaN there would fill the volume or the picture with NaN).
pub fn unsupported_inputs(clip: &Mat4, fog: AirFog, fog_colour: Vec3) -> Option<&'static str> {
    if !clip.is_finite() {
        return Some("the projection is not finite");
    }
    if clip.z_axis.w != -1.0 || clip.w_axis.w != 0.0 || clip.z_axis.z != 0.0 || clip.z_axis.x != 0.0 || clip.z_axis.y != 0.0 {
        return Some("the projection is not a centred reverse infinite perspective");
    }
    // Finite from here on, so `<= 0` is "not above 0".
    if clip.x_axis.x <= 0.0 || clip.y_axis.y <= 0.0 {
        return Some("the projection's x or y scale is not above 0");
    }
    if clip.w_axis.z <= 0.0 {
        return Some("the projection's near term is not above 0");
    }
    if !(fog.near.is_finite() && fog.far.is_finite() && fog.power.is_finite() && fog.strength.is_finite()) {
        return Some("the fog's numbers are not finite");
    }
    if !fog_colour.is_finite() {
        return Some("the fog colour is not finite");
    }
    None
}

/// The uniform for a camera and a view (the sunlit share and the gate from
/// medium_air_light.rs).
pub fn air_uniform(cam: &AirCamera, phase: &NormedPhase, view: &AirViewInputs) -> AirGpuUniform {
    let packed = medium_air::pack(&cam.grid, cam.fog, phase, &cam.tune, cam.share);
    let p = view.clip_from_view;
    AirGpuUniform {
        world_from_view: view.world_from_view,
        proj: Vec4::new(p.x_axis.x, p.y_axis.y, p.w_axis.z, crate::sky::SKY_DEPTH_FROM),
        grid: Vec4::from_array(packed.grid),
        curve: Vec4::from_array(packed.curve),
        fog: Vec4::from_array(packed.fog),
        phase: Vec4::from_array(packed.phase),
        phase2: Vec4::from_array(packed.phase2),
        light: cam.to_light.extend(medium_air::light_flag(cam.has_light && cam.draws, cam.tune.floor)),
        fog_colour: cam.fog_colour.extend(if cam.display_blend { 1.0 } else { 0.0 }),
        misc: Vec4::new(cam.tune.occluders.code() as f32, cam.tune.samples.clamp(1, 8) as f32, cam.tune.sky_weight, cam.tune.sky_phase),
        viewport: Vec4::new(view.origin.x as f32, view.origin.y as f32, view.render.x.max(1) as f32, view.render.y.max(1) as f32),
        // Filled by [`AirGpuUniform::with_terrain`].
        terrain_uv: Vec4::ZERO,
        terrain: Vec4::ZERO,
        terrain_light: Vec4::ZERO,
        // Filled by [`AirGpuUniform::with_clouds`].
        cloud_uv: Vec4::ZERO,
        cloud: Vec4::ZERO,
        cloud_mode: Vec4::ZERO,
        rays2: Vec4::new(cam.tune.lift, cam.tune.reference.unwrap_or(-1.0), cam.tune.shade, if cam.tune.share.is_some() { 1.0 } else { 0.0 }),
    }
}

impl AirGpuUniform {
    /// The uniform with the terrain map's block.
    pub fn with_terrain(self, block: AirTerrainBlock) -> AirGpuUniform {
        AirGpuUniform { terrain_uv: block.uv, terrain: block.terrain, terrain_light: block.light, ..self }
    }

    /// The uniform with the Cloud shadows image's block.
    pub fn with_clouds(self, block: AirCloudBlock) -> AirGpuUniform {
        AirGpuUniform { cloud_uv: block.uv, cloud: block.cloud, cloud_mode: block.mode, ..self }
    }
}

/// The least dot between the light the terrain map was computed for and the current light for
/// the map to count (about 2.6 degrees): at a sun/moon switch, or while a new map is computed
/// after the light moved on, the map describes another light and T is 1 [design].
pub const TERRAIN_SAME_LIGHT: f32 = 0.999;

/// The terrain map's publication as the grid pass can use it: the extracted
/// `terrain_shadows::TerrainShadows` fields and whether its image is on the GPU in the map's
/// format and size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirTerrainPublication {
    pub uv_scale: Vec2,
    pub uv_offset: Vec2,
    pub texel: f32,
    pub side: u32,
    pub light_dir: Vec3,
    pub on_gpu: bool,
}

/// The terrain map's block of the uniform.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AirTerrainBlock {
    pub uv: Vec4,
    pub terrain: Vec4,
    pub light: Vec4,
}

/// The Cloud shadows image's block of the uniform (K, the flat clouds' half).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AirCloudBlock {
    pub uv: Vec4,
    pub cloud: Vec4,
    pub mode: Vec4,
}

/// The block for the cloud shadow the surfaces read (`sun_visibility::SunVisibility::cloud`,
/// published by cloud_shadows.rs while the Cloud shadows row is on, or by the sun visibility's
/// test producer) and whether its image is on the GPU: packed as
/// `sun_visibility::SunVisibilityUniform::pack` packs `cloud_uv` and `cloud` (the strength
/// held to 0..1, the fade's sines [`crate::sun_visibility::CLOUD_FADE_FROM`] and
/// [`crate::sun_visibility::CLOUD_FADE_TO`]); valid with the image on the GPU and finite
/// fields. Not valid: zeros (K is 1). The caller fills the mode's y and z.
pub fn cloud_block(cloud: Option<&crate::sun_visibility::CloudVisibility>, on_gpu: bool) -> AirCloudBlock {
    let Some(c) = cloud else { return AirCloudBlock::default() };
    let finite = c.uv_scale.is_finite() && c.uv_offset.is_finite() && c.plane_height.is_finite() && c.strength.is_finite();
    if !on_gpu || !finite {
        return AirCloudBlock::default();
    }
    AirCloudBlock {
        uv: Vec4::new(c.uv_scale.x, c.uv_scale.y, c.uv_offset.x, c.uv_offset.y),
        cloud: Vec4::new(c.plane_height, c.strength.clamp(0.0, 1.0), crate::sun_visibility::CLOUD_FADE_FROM, crate::sun_visibility::CLOUD_FADE_TO),
        mode: Vec4::new(1.0, 0.0, 0.0, 0.0),
    }
}

/// The mean of the Cloud shadows image's texel values (`R8Unorm`, 0..1), over its whole repeat:
/// the mean of what the repeating bilinear sampler reads over one period equals the texels'
/// mean (each texel's bilinear weights over a period sum to one texel's share). `None` for no
/// data or another format.
pub fn texel_mean(image: &Image) -> Option<f32> {
    if image.texture_descriptor.format != TextureFormat::R8Unorm {
        return None;
    }
    let data = image.data.as_deref().filter(|d| !d.is_empty())?;
    let sum: u64 = data.iter().map(|&b| u64::from(b)).sum();
    Some(sum as f32 / (data.len() as f32 * 255.0))
}

/// The stand-in for the image's mean while its CPU copy cannot be read (never expected: the
/// producer keeps one, `RenderAssetUsages::default()`): half the texels' range [design].
pub const CLOUD_MEAN_STAND_IN: f32 = 0.5;

/// The mean of K over the cloud pattern's whole repeat for a camera `height` up, as
/// medium_air_grid.wesl `air_cloud_weather` reads it with light elevation sine `up`: K = 1 - s (1 - v)
/// x fade is affine in the texel value v (strength s held to 0..1 keeps 1 - s (1 - v) in
/// 0..1, so the clamp never acts), so its mean is 1 - s x fade x (1 - mean v), the fade
/// smoothstep(z, w, up) of the block's sines and 0 at or below z. 1 at or above the plane, where the
/// image shades nothing (the mean pass's own check, so a front arriving steps nothing), and when the
/// block is not valid; `mean_value` none takes [`CLOUD_MEAN_STAND_IN`].
pub fn cloud_mean_k(block: &AirCloudBlock, mean_value: Option<f32>, up: f32, height: f32) -> f32 {
    if block.mode.x < 0.5 || height >= block.cloud.x {
        return 1.0;
    }
    let (from, to) = (block.cloud.z, block.cloud.w);
    if up <= from {
        return 1.0;
    }
    let t = ((up - from) / (to - from)).clamp(0.0, 1.0);
    let fade = t * t * (3.0 - 2.0 * t);
    let v = mean_value.unwrap_or(CLOUD_MEAN_STAND_IN).clamp(0.0, 1.0);
    (1.0 - block.cloud.y * fade * (1.0 - v)).clamp(0.0, 1.0)
}

/// The deck's transmittance without the light volume (`cloud_mode.z`), for a camera `height` up: the flat
/// layers' own (cloud_shadows.rs `CloudShadowWork::deck`: absolute, where the image is relative to
/// its own clear percentile), 1 with none (Cloud shadows off, indoors, no layer read yet) or no
/// number, and 1 from their plane up, as [`cloud_mean_k`] (the plane comes with it, whether or not the
/// image is on the GPU yet). A 0 is an opaque deck.
pub fn flat_deck(deck: Option<crate::cloud_shadows::FlatDeck>, height: f32) -> f32 {
    deck.filter(|d| d.transmittance.is_finite() && height < d.plane_height).map_or(1.0, |d| d.transmittance.clamp(0.0, 1.0))
}

/// The Cloud shadows image's texel mean, kept in the main world for the image the surfaces read
/// (`sun_visibility::SunVisibility::cloud`), worked out again when that image is replaced or
/// its data changes (each composition writes the CPU copy: an asset event).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct AirCloudMean {
    pub image: Option<AssetId<Image>>,
    pub mean: Option<f32>,
}

/// The block for a publication (none: not valid) and the current light `to_light`: valid only
/// with the image on the GPU, a side and a texel above 0, finite transforms, and the map's
/// light within [`TERRAIN_SAME_LIGHT`] of the current one (a zero light never is).
pub fn terrain_block(publication: Option<&AirTerrainPublication>, to_light: Vec3) -> AirTerrainBlock {
    let Some(p) = publication else { return AirTerrainBlock::default() };
    let same = p.light_dir.normalize_or_zero().dot(to_light.normalize_or_zero());
    let finite = p.uv_scale.is_finite() && p.uv_offset.is_finite() && p.texel.is_finite();
    let valid = p.on_gpu && p.side > 0 && p.texel > 0.0 && finite && same >= TERRAIN_SAME_LIGHT;
    AirTerrainBlock {
        uv: Vec4::new(p.uv_scale.x, p.uv_scale.y, p.uv_offset.x, p.uv_offset.y),
        terrain: Vec4::new(if valid { 1.0 } else { 0.0 }, same, p.texel, p.side as f32),
        light: p.light_dir.extend(0.0),
    }
}

/// The shader def of the grid's light-volume variant (medium_air_grid.wesl).
pub const AIR_VOLUME_DEF: &str = "AIR_LIGHT_VOLUME";

/// Where `cloud_mode.y` lies in the uniform's buffer (bytes): the sixteenth vec4 after the matrix,
/// its second number.
pub const CLOUD_MEAN_OFFSET: u64 = 64 + 15 * 16 + 4;
/// The mean pass's storage buffer (one vec4: the mean of K, the deck's transmittance, 0, 0).
const CLOUD_MEAN_BYTES: u64 = 16;
/// How much of it is copied over the uniform at [`CLOUD_MEAN_OFFSET`]: `cloud_mode`'s y and z; never
/// its x, which the grid reads.
pub const CLOUD_MEAN_COPY: u64 = 8;

/// medium_air_grid.wesl `AirLightVolume` (binding 8), field for field.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct AirLightVolume {
    /// x the strength, y and z the low-sun band, w 1 for the volume alone.
    pub cloud: Vec4,
    /// The surfaces' window.
    pub window: LightVolumeWindow,
}

/// The light volume as the surfaces' uniform has it this frame (the same strength, band, window and
/// presences), or none while they read the weather's layers. Their debug views (the reader's share
/// or depth shown in place of K) are theirs alone: the air reads the volume alone then.
pub fn air_light_volume(u: &SunVisibilityUniform) -> Option<AirLightVolume> {
    (u.mode.x & MASK_CLOUD_VOLUME != 0).then(|| AirLightVolume { cloud: Vec4::new(u.volume.x, u.volume.y, u.volume.z, if u.mode.x & MASK_CLOUD_VOLUME_ONLY != 0 { 1.0 } else { 0.0 }), window: u.light_volume })
}

/// The light-volume variant's pipelines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumePipelines {
    /// Not asked for, or still compiling.
    Waiting,
    Ready,
    Failed,
    /// The adapter has no storage buffer in the compute stage (the mean pass's).
    NoStorage,
}

/// Where the air's K comes from in a frame, for the log and the pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirCloudSource {
    /// The surfaces' source, word for word.
    Surfaces(CloudSourceInUse),
    /// The surfaces read the light volume, the air the weather's layers: its pipelines say why.
    Held(VolumePipelines),
}

impl AirCloudSource {
    /// Whether the grid's light-volume variant runs.
    pub fn reads_volume(self) -> bool {
        matches!(self, Self::Surfaces(CloudSourceInUse::Volume { .. } | CloudSourceInUse::Debug { .. }))
    }

    /// The log line's words.
    pub fn line(self) -> String {
        match self {
            Self::Surfaces(CloudSourceInUse::Debug { near, far, .. }) => CloudSourceInUse::Volume { near, far, only: true }.line(),
            Self::Surfaces(s) => s.line(),
            Self::Held(VolumePipelines::Failed) => "the weather's layers (the light volume's pipelines failed)".to_string(),
            Self::Held(VolumePipelines::NoStorage) => "the weather's layers (this adapter has no storage buffer in compute)".to_string(),
            Self::Held(_) => "the weather's layers (the light volume's pipelines are not ready)".to_string(),
        }
    }
}

/// The air's source: the surfaces' (so the gate, sun_visibility.rs `AutoGate`, the aid and every
/// fallback are the ground's), the weather's layers while the volume's own pipelines cannot run.
pub fn air_cloud_source(surfaces: CloudSourceInUse, pipelines: VolumePipelines) -> AirCloudSource {
    let source = AirCloudSource::Surfaces(surfaces);
    if source.reads_volume() && pipelines != VolumePipelines::Ready { AirCloudSource::Held(pipelines) } else { source }
}

pub struct MediumAirPlugin;

impl Plugin for MediumAirPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "medium_air_grid.wesl");
        bevy::asset::embedded_asset!(app, "medium_air_compose.wesl");
        bevy::asset::embedded_asset!(app, "medium_air_filter.wesl");
        let gate = add_main_world(app);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .insert_resource(gate)
            .init_resource::<AirState>()
            .init_resource::<crate::medium_air_compose::ComposePipelines>()
            .add_systems(ExtractSchedule, extract_air.run_if(air_render_in_use))
            .add_systems(Render, (drain_air::<AirViewGpu>, prepare_air).chain().in_set(RenderSystems::PrepareResources).run_if(air_render_in_use))
            .add_systems(
                Render,
                prepare_air_volume.in_set(RenderSystems::PrepareResourcesFlush).after(crate::sun_visibility::CloudShadowsDecided).run_if(air_render_in_use),
            )
            .add_systems(
                Core3d,
                (air_grid_pass, air_filter_pass, crate::medium_air_compose::air_compose_pass)
                    .chain()
                    .after(Core3dSystems::MainPass)
                    .before(Core3dSystems::EarlyPostProcess)
                    .run_if(air_render_in_use),
            );
    }
}

/// The main world's half: the demand, the gate and [`follow_air`]. Returns the gate for the
/// render world.
fn add_main_world(app: &mut App) -> AirGate {
    let gate = AirGate::default();
    app.add_plugins(AirDemandPlugin).insert_resource(gate.clone()).init_resource::<AirCloudMean>().add_systems(
        PostUpdate,
        (
            // After underwater.rs too: this frame's fog and whether the camera is under water.
            follow_air
                .run_if(air_in_use)
                .after(bevy::camera::CameraUpdateSystems)
                .after(bevy::transform::TransformSystems::Propagate)
                .after(crate::underwater::underwater_fog),
            follow_cloud_mean.run_if(|w: Option<Res<AirWanted>>| w.is_some_and(|w| w.0)).run_if(resource_exists::<Assets<Image>>),
            forget_cloud_mean.run_if(|w: Option<Res<AirWanted>>, images: Option<Res<Assets<Image>>>| !(w.is_some_and(|w| w.0) && images.is_some())),
        ),
    );
    gate
}

/// Whenever [`follow_cloud_mean`] does not run (the light not wanted, or no image assets): the
/// cached mean is forgotten, so the first frame it runs again works it out from the image's
/// data. Across the gap the image's texels may have been rewritten under the same handle (a
/// weather change with Cloud shadows on) and the modification's event expired unread, so the
/// image id alone cannot tell the cache is stale.
fn forget_cloud_mean(mut mean: ResMut<AirCloudMean>) {
    if *mean != AirCloudMean::default() {
        *mean = AirCloudMean::default();
    }
}

/// While wanted: [`AirCloudMean`] for the image the surfaces read, from its CPU copy, when the
/// image changes or its data does.
fn follow_cloud_mean(
    visibility: Option<Res<crate::sun_visibility::SunVisibility>>,
    images: Res<Assets<Image>>,
    mut events: MessageReader<AssetEvent<Image>>,
    mut mean: ResMut<AirCloudMean>,
) {
    let id = visibility.as_ref().and_then(|v| v.cloud.as_ref()).map(|c| c.image.id());
    let touched = events
        .read()
        .filter(|e| match e {
            AssetEvent::Added { id: i } | AssetEvent::Modified { id: i } | AssetEvent::LoadedWithDependencies { id: i } => Some(*i) == id,
            _ => false,
        })
        .count()
        > 0;
    if id != mean.image || touched {
        let next = AirCloudMean { image: id, mean: id.and_then(|i| images.get(i)).and_then(texel_mean) };
        if *mean != next {
            *mean = next;
        }
    }
}

/// Whether [`follow_air`] has anything to do: the light wanted, or a camera still marked, or
/// the gate still open.
fn air_in_use(wanted: Option<Res<AirWanted>>, gate: Res<AirGate>, marked: Query<(), With<AirCamera>>) -> bool {
    wanted.is_some_and(|w| w.0) || gate.is_open() || !marked.is_empty()
}

/// Each frame while wanted: the main camera's [`AirCamera`] from the projection, the camera's
/// fog, the sun light and the sky; taken off every camera once it is not wanted.
#[allow(clippy::type_complexity)]
pub(crate) fn follow_air(
    mut commands: Commands,
    (wanted, gate): (Option<Res<AirWanted>>, Res<AirGate>),
    (sky, submerged): (Option<Res<crate::sky::SkyState>>, Option<Res<crate::underwater::CameraSubmerged>>),
    suns: Query<&GlobalTransform, With<crate::sky::Sun>>,
    cams: Query<(Entity, &Projection, Option<&DistanceFog>, Option<&AirCamera>), With<FlyCam>>,
    marked: Query<Entity, With<AirCamera>>,
    mut told_projection: Local<bool>,
) {
    let on = wanted.is_some_and(|w| w.0);
    gate.set_open(on);
    // Only a perspective main camera (both shaders take a perspective's rays and depth).
    let main = cams.iter().next().and_then(|(e, projection, fog, was)| match projection {
        Projection::Perspective(p) => Some((e, p.aspect_ratio, fog, was)),
        _ => None,
    });
    for e in &marked {
        if !on || main.is_none_or(|m| m.0 != e) {
            commands.entity(e).try_remove::<AirCamera>();
        }
    }
    if !on {
        *told_projection = false;
        return;
    }
    if main.is_none() && !cams.is_empty() && !*told_projection {
        *told_projection = true;
        warn!("volumetric medium: light in the air not drawn: the main camera's projection is not a perspective");
    }
    // The main pass's render aspect: the perspective's own (the upscalers keep it).
    let Some((entity, aspect, fog, was)) = main else { return };
    let aids = aids();
    let grid = AirGrid::for_view(aspect, aids.grid, aids.tune.depth);
    let (air_fog, fog_colour, display_blend) = camera_air_fog(fog);
    let outdoors = sky.as_ref().is_some_and(|s| s.interior.is_none()) && !submerged.is_some_and(|s| s.0);
    let sun = suns.iter().next();
    let to_light = sun.map_or(Vec3::ZERO, |t| t.back().as_vec3());
    // The share and the gate are medium_air_light.rs's (it runs after this): kept as it left them.
    let (share, draws) = was.map_or((0.0, false), |w| (w.share, w.draws));
    let next = AirCamera { grid, tune: aids.tune, fog: air_fog, fog_colour, display_blend, to_light, has_light: sun.is_some() && outdoors, outdoors, share, draws };
    if was != Some(&next) {
        commands.entity(entity).try_insert(next);
    }
}

/// The render side runs while its gate is open, and once more after it closed while it still
/// holds a volume or a view to release.
fn air_render_in_use(gate: Res<AirGate>, state: Res<AirState>) -> bool {
    gate.is_open() || state.held
}

/// The main camera's [`AirCamera`] onto its view; taken off every other render entity.
fn extract_air(
    mut commands: Commands,
    cams: Extract<Query<(RenderEntity, &AirCamera)>>,
    marked: Query<Entity, With<AirCamera>>,
    visibility: Extract<Option<Res<crate::sun_visibility::SunVisibility>>>,
    cloud_mean: Extract<Option<Res<AirCloudMean>>>,
    layers: Extract<Option<Res<crate::cloud_shadows::CloudShadowWork>>>,
    mut state: ResMut<AirState>,
) {
    let deck = layers.as_ref().and_then(|w| w.deck()).filter(|_| !cams.is_empty());
    if state.cloud_deck != deck {
        state.cloud_deck = deck;
    }
    // The cloud shadow the surfaces read (sun_visibility.rs's main-world resource): kept while wanted, dropped once nothing is held.
    let cloud = visibility.as_ref().and_then(|v| v.cloud.clone()).filter(|_| !cams.is_empty());
    // Its texel mean, only for that same image.
    let mean = cloud_mean.as_ref().filter(|m| m.image.is_some() && m.image == cloud.as_ref().map(|c| c.image.id())).and_then(|m| m.mean);
    if state.cloud != cloud {
        state.cloud = cloud;
    }
    if state.cloud_mean != mean {
        state.cloud_mean = mean;
    }
    mark_views(&mut commands, &mut state, cams.iter().next().map(|(e, cam)| (e, *cam)), &marked);
}

/// The render world's marks: `kept`'s render entity gets its [`AirCamera`], every other
/// render entity loses its own (whether or not it is a view this frame: an inactive camera's
/// render entity has no `ExtractedView` and is unmarked here like any other). Queuing a mark
/// latches [`AirState::held`] at once, so the render side runs again to drain it even if the
/// main world closes the gate before the pipelined preparation looks.
fn mark_views(commands: &mut Commands, state: &mut AirState, kept: Option<(Entity, AirCamera)>, marked: &Query<Entity, With<AirCamera>>) {
    if let Some((e, cam)) = kept {
        commands.entity(e).try_insert(cam);
        state.held = true;
    }
    for e in marked {
        if kept.is_none_or(|(k, _)| k != e) {
            commands.entity(e).try_remove::<AirCamera>();
        }
    }
}

/// The render world's state: the grid pipeline, the phase's norm, the adapter's answer and
/// what was logged.
#[derive(Resource, Default)]
pub(crate) struct AirState {
    /// A render entity holds an [`AirCamera`] or a view's GPU side ([`drain_air`]): the
    /// systems run until nothing is held.
    pub(crate) held: bool,
    grid_layout: Option<BindGroupLayoutDescriptor>,
    /// The grid's pipelines, one a [`GridWalk`], asked for when their walk is first needed (a
    /// tune file's GRID can change it), then kept.
    grid_pipelines: [Option<CachedComputePipelineId>; 3],
    /// This frame's walk ([`prepare_air`]), and the one last logged.
    walk: GridWalk,
    told_walk: Option<GridWalk>,
    /// The filter pass's layout and pipeline (asked for when `FILTER` is first on).
    filter_layout: Option<BindGroupLayoutDescriptor>,
    filter_pipeline: Option<CachedComputePipelineId>,
    /// The phase with its norm, made again when the tune's phase changes.
    normed: Option<NormedPhase>,
    /// Whether the adapter can store and sample the volume (the format, once per activation)
    /// and hold the grid (every frame: a tune file can change GRID).
    support: VolumeSupport,
    /// The grid pass wrote the volume this frame (reset in [`prepare_air`]).
    pub(crate) drawn: bool,
    told_drawn: bool,
    pub(crate) told_composed: bool,
    told: Vec<String>,
    /// The terrain map's 1 x 1 stand-in, bound while no valid map is (made on first need).
    terrain_fallback: Option<(Texture, TextureView)>,
    /// Whether the terrain map was in use when last logged.
    told_terrain: Option<bool>,
    /// The cloud shadow the surfaces read, extracted while wanted, and its image's texel mean.
    cloud: Option<crate::sun_visibility::CloudVisibility>,
    cloud_mean: Option<f32>,
    /// The flat layers' own transmittance and plane ([`flat_deck`]), extracted while wanted.
    cloud_deck: Option<crate::cloud_shadows::FlatDeck>,
    /// The cloud image's 1 x 1 stand-in and the sampler it and the image are read with
    /// (repeating, linear, as the surfaces' plumbing samples it); made on first need.
    cloud_fallback: Option<(Texture, TextureView)>,
    cloud_sampler: Option<Sampler>,
    /// Why the cloud shadow was not in use when last logged ("" in use).
    told_cloud: Option<String>,
    /// The light volume's side of the grid.
    volume: AirVolumeState,
}

/// The light volume's side of the grid: its variant's layout and pipelines (asked for once the
/// surfaces may read the volume, then kept), its uniform, the mean's buffer, and this frame's
/// levels while the grid reads it.
#[derive(Default)]
struct AirVolumeState {
    layout: Option<BindGroupLayoutDescriptor>,
    /// The grid's pipelines, one a [`GridWalk`] as needed, and the mean's.
    grids: [Option<CachedComputePipelineId>; 3],
    mean_pipeline: Option<CachedComputePipelineId>,
    uniform: Option<UniformBuffer<AirLightVolume>>,
    mean: Option<Buffer>,
    levels: Option<[TextureView; 2]>,
    /// This frame's composite reads the clouds' mean ([`wants_cloud_mean`]): the mean pass
    /// runs, and its pipeline has to be ready.
    mean_wanted: bool,
    /// The source last named in the log.
    told: Option<AirCloudSource>,
    told_failed: bool,
    /// The grid's draw from the light volume was logged.
    told_drawn: bool,
}

/// Whether a view's composite reads the clouds' mean transmittance (`cloud_mode.y`,
/// medium_air_compose.wesl `air_reference` and `air_deck_share`): a view that shows the light, with
/// no fixed `REF` or no fixed `SHARE`. The diagnostic views and the `off` baseline never ask for it.
pub fn wants_cloud_mean(tune: &AirTune) -> bool {
    crate::medium_air_compose::shows_light(tune.view) && (tune.reference.is_none() || tune.share.is_none())
}

/// What the grid pass needs to read the light volume this frame.
struct VolumePass<'c> {
    grid: &'c ComputePipeline,
    /// The mean pass, while it is wanted.
    mean: Option<&'c ComputePipeline>,
    layout: BindGroupLayoutDescriptor,
    uniform: Buffer,
    mean_buffer: Buffer,
    levels: [TextureView; 2],
}

impl AirVolumeState {
    /// The state of the pipelines this frame needs (the grid's, and the mean's while wanted) in the
    /// cache; a failure is logged once.
    fn pipelines(&mut self, cache: &PipelineCache, walk: GridWalk) -> VolumePipelines {
        let (Some(grid), Some(mean)) = (self.grids[walk as usize], self.mean_pipeline) else { return VolumePipelines::Waiting };
        let mut all = VolumePipelines::Ready;
        for id in std::iter::once(grid).chain(self.mean_wanted.then_some(mean)) {
            match cache.get_compute_pipeline_state(id) {
                CachedPipelineState::Ok(_) => {}
                // The cache's waiting states, as in `air_grid_pass`.
                CachedPipelineState::Err(ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable) => all = VolumePipelines::Waiting,
                CachedPipelineState::Err(e) => {
                    if !self.told_failed {
                        self.told_failed = true;
                        warn!("volumetric medium: light in the air: a pipeline of the light volume failed, the weather's layers are read: {e}");
                    }
                    return VolumePipelines::Failed;
                }
                _ => all = VolumePipelines::Waiting,
            }
        }
        all
    }

    /// The pass's side while the grid reads the light volume this frame.
    fn pass<'c>(&self, cache: &'c PipelineCache, walk: GridWalk) -> Option<VolumePass<'c>> {
        let levels = self.levels.clone()?;
        let grid = self.grids[walk as usize]?;
        let mean = match self.mean_wanted {
            true => Some(cache.get_compute_pipeline(self.mean_pipeline?)?),
            false => None,
        };
        Some(VolumePass {
            grid: cache.get_compute_pipeline(grid)?,
            mean,
            layout: self.layout.clone()?,
            uniform: self.uniform.as_ref()?.buffer()?.clone(),
            mean_buffer: self.mean.clone()?,
            levels,
        })
    }

    /// Nothing held: the buffers, the levels and what was logged go; the pipelines stay in the
    /// cache and their ids with them.
    fn release(&mut self) {
        self.uniform = None;
        self.mean = None;
        self.levels = None;
        self.told = None;
        self.told_failed = false;
        self.told_drawn = false;
    }
}

impl AirState {
    /// `volumetric medium: light in the air not drawn: <reason>`, once per reason per activation.
    pub(crate) fn tell(&mut self, reason: &str) {
        if !self.told.iter().any(|r| r == reason) {
            self.told.push(reason.to_string());
            warn!("volumetric medium: light in the air not drawn: {reason}");
        }
    }

    /// Nothing held: forget the activation's state (the next one logs again).
    fn release(&mut self) {
        self.held = false;
        self.normed = None;
        self.support = VolumeSupport::default();
        self.drawn = false;
        self.told_drawn = false;
        self.told_walk = None;
        self.told_composed = false;
        self.told.clear();
        self.terrain_fallback = None;
        self.told_terrain = None;
        self.cloud = None;
        self.cloud_mean = None;
        self.cloud_deck = None;
        self.cloud_fallback = None;
        self.cloud_sampler = None;
        self.told_cloud = None;
        self.volume.release();
    }
}

/// A view's GPU side of the light: the volume, its uniform and its composite pipeline.
#[derive(Component)]
pub(crate) struct AirViewGpu {
    pub(crate) grid: AirGrid,
    /// The grid volume and, with `FILTER` on, the filtered one (kept so their views stay
    /// valid; both dropped with the component, so the drain releases them together).
    _volumes: (Texture, Option<Texture>),
    /// The grid volume's view: storage for the grid pass, sampled by the filter pass.
    pub(crate) grid_view: TextureView,
    /// The filtered volume's view while `FILTER` is on (storage for the filter pass); none
    /// with it off (the filter pass then does not run).
    pub(crate) filter_target: Option<TextureView>,
    /// The view the composite samples: the filtered volume's with `FILTER` on, else the grid
    /// volume's (the composite's binding is unchanged).
    pub(crate) volume_view: TextureView,
    pub(crate) uniform: UniformBuffer<AirGpuUniform>,
    pub(crate) view: AirView,
    /// The composite's pipeline for the debug views, by target format and depth sample type.
    pub(crate) compose: Option<ComposeId>,
    /// The terrain map's view while it is valid for the current light ([`terrain_block`]);
    /// else the stand-in is bound.
    pub(crate) terrain_view: Option<TextureView>,
    /// The Cloud shadows image's view while published and on the GPU; else the stand-in.
    pub(crate) cloud_view: Option<TextureView>,
}

/// A composite pipeline with what it was made for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ComposeId {
    pub(crate) id: bevy::render::render_resource::CachedRenderPipelineId,
    pub(crate) format: TextureFormat,
    pub(crate) multisampled: bool,
}

/// Whether the adapter can do what the volume needs, whatever the grid: `rgba16float` with
/// storage and sampled usage and filterable (all core in WebGPU, so a formality) and one
/// storage texture in the compute stage [documented: WebGPU, the texel format capability
/// table; wgpu `Limits`].
pub(crate) fn format_supported(adapter: &RenderAdapter, limits: &bevy::render::render_resource::WgpuLimits) -> Result<(), String> {
    let features = adapter.get_texture_format_features(AIR_FORMAT);
    volume_format(features.allowed_usages, features.flags.contains(TextureFormatFeatureFlags::FILTERABLE), limits.max_storage_textures_per_shader_stage)
}

/// The volume's usages.
pub(crate) const AIR_USAGES: TextureUsages = TextureUsages::STORAGE_BINDING.union(TextureUsages::TEXTURE_BINDING);

/// [`format_supported`] of what the device says: the format's `usages`, whether it filters, the
/// storage textures of a stage.
pub(crate) fn volume_format(usages: TextureUsages, filterable: bool, storage_textures: u32) -> Result<(), String> {
    if !usages.contains(AIR_USAGES) {
        return Err("the adapter cannot store and sample rgba16float".into());
    }
    if !filterable {
        return Err("the adapter cannot filter rgba16float".into());
    }
    if storage_textures < 1 {
        return Err("the adapter has no storage textures in compute".into());
    }
    Ok(())
}

/// Whether the chunks' 8 x 8 x 4 workgroup and its shared memory fit the adapter: 256
/// invocations and 2,048 bytes. WebGPU's defaults are 256 and 16,384, but Bevy asks for the
/// adapter's own limits and Vulkan 1.0 requires no more than 128 invocations, so an adapter it
/// does not fit walks each column in one whatever it is ([`GridWalk::Serial`], 64 invocations)
/// [documented: WebGPU, "Limits"; Vulkan 1.0, "Required Limits", maxComputeWorkGroupInvocations].
fn workgroup_fits(limits: &bevy::render::render_resource::WgpuLimits) -> bool {
    limits.max_compute_invocations_per_workgroup >= GRID_INVOCATIONS
        && limits.max_compute_workgroup_size_x >= WORKGROUP
        && limits.max_compute_workgroup_size_y >= WORKGROUP
        && limits.max_compute_workgroup_size_z >= CHUNKS
        && limits.max_compute_workgroup_storage_size >= GRID_SHARED_BYTES
}

/// Whether a 3D texture of the grid's size fits the adapter's `max_texture_dimension_3d`
/// [documented: wgpu `Limits`].
pub fn grid_fits(max_texture_dimension_3d: u32, grid: &AirGrid) -> Result<(), String> {
    let side = grid.nx.max(grid.ny).max(grid.nz);
    if side > max_texture_dimension_3d {
        return Err(format!("the grid's side {side} is over the adapter's 3D limit {max_texture_dimension_3d}"));
    }
    Ok(())
}

/// The adapter's answer for the volume: the format's support, asked once per activation and
/// kept; the grid's size, checked again on every call (it depends on the grid, which a tune
/// file can change both ways: a grid over the limit is refused, one under it afterwards is
/// drawn) [design].
#[derive(Clone, Debug, Default)]
pub struct VolumeSupport {
    format: Option<Result<(), String>>,
}

impl VolumeSupport {
    /// The answer for `grid`: the format's (asked through `format` on the first call only),
    /// then [`grid_fits`].
    pub fn check(&mut self, format: impl FnOnce() -> Result<(), String>, max_texture_dimension_3d: u32, grid: &AirGrid) -> Result<(), String> {
        self.format.get_or_insert_with(format).clone()?;
        grid_fits(max_texture_dimension_3d, grid)
    }
}

/// [`drain_air`]'s query: each holder, whether it is marked, holds `G`, is a view this frame.
type HolderData<G> = (Entity, Has<AirCamera>, Has<G>, Has<ExtractedView>);
/// [`drain_air`]'s filter: the render entities holding an [`AirCamera`] or `G`.
type HolderFilter<G> = Or<(With<AirCamera>, With<G>)>;

/// The render entities that hold an [`AirCamera`] or a view's GPU side `G` ([`AirViewGpu`]; a
/// stand-in in the tests), whether each is a view this frame, before [`prepare_air`]: the GPU
/// side goes from every entity that is not a marked view (unmarked, or no `ExtractedView`:
/// Bevy takes that off an inactive camera's render entity and leaves ours), and the render
/// side stays running ([`AirState::held`]) while any entity still holds either component, so
/// [`extract_air`] runs to take the marks away after the light is switched off even when the
/// camera had gone inactive first. With nothing held the activation's state is released.
fn drain_air<G: Component>(mut commands: Commands, mut state: ResMut<AirState>, holders: Query<HolderData<G>, HolderFilter<G>>) {
    let mut held = false;
    for (entity, marked, gpu, view) in &holders {
        held = true;
        if gpu && !(marked && view) {
            commands.entity(entity).try_remove::<G>();
        }
    }
    if held {
        state.held = true;
    } else if state.held {
        state.release();
    }
}

/// medium_air_grid.wesl's bindings 0-7: the uniform, the volume (write-only storage), Bevy's
/// lights (dynamic offset), the directional shadow maps (depth 2D array), their comparison
/// sampler [bevy_pbr light.rs `LightMeta`, `ViewShadowBindings`, `ShadowSamplers`],
/// the terrain shadow map (unfilterable, textureLoad), the Cloud shadows image (filterable)
/// and its repeating linear sampler.
fn grid_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "medium_air_grid_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<AirGpuUniform>(false),
                texture_storage_3d(AIR_FORMAT, StorageTextureAccess::WriteOnly),
                uniform_buffer::<GpuLights>(true),
                texture_2d_array(TextureSampleType::Depth),
                sampler(SamplerBindingType::Comparison),
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    )
}

/// The light-volume variant's bindings: [`grid_layout`]'s 0-7, the light volume's uniform, its
/// near and far levels (R16Float arrays, filterable) and the mean's storage buffer. In the compute
/// stage that is 5 sampled textures, 2 samplers, 3 uniform buffers, 1 storage buffer and 1 storage
/// texture, inside WebGPU's default limits [documented: WebGPU, "Limits": 16, 16, 12, 8 and 4].
fn grid_volume_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "medium_air_grid_volume_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<AirGpuUniform>(false),
                texture_storage_3d(AIR_FORMAT, StorageTextureAccess::WriteOnly),
                uniform_buffer::<GpuLights>(true),
                texture_2d_array(TextureSampleType::Depth),
                sampler(SamplerBindingType::Comparison),
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<AirLightVolume>(false),
                texture_2d_array(TextureSampleType::Float { filterable: true }),
                texture_2d_array(TextureSampleType::Float { filterable: true }),
                storage_buffer_sized(false, NonZeroU64::new(CLOUD_MEAN_BYTES)),
            ),
        ),
    )
}

/// The volume for a grid.
fn make_volume(device: &RenderDevice, grid: &AirGrid, label: &'static str) -> (Texture, TextureView) {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d { width: grid.nx, height: grid.ny, depth_or_array_layers: grid.nz },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D3,
        format: AIR_FORMAT,
        usage: AIR_USAGES,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    (texture, view)
}

/// The terrain map's format (terrain_shadows.rs `OUTPUT_FORMAT`).
const TERRAIN_FORMAT: TextureFormat = TextureFormat::Rgba32Float;

/// The extracted terrain map as the air reads it, and its view when its image is on the GPU
/// in the map's format and size.
fn terrain_publication(t: Option<&crate::terrain_shadows::TerrainShadows>, images: &RenderAssets<GpuImage>) -> (Option<AirTerrainPublication>, Option<TextureView>) {
    let Some(t) = t else { return (None, None) };
    let image = t.image.as_ref().and_then(|h| images.get(h.id())).filter(|g| {
        let d = &g.texture_descriptor;
        d.format == TERRAIN_FORMAT && d.size.width == t.side && d.size.height == t.side
    });
    let publication = AirTerrainPublication { uv_scale: t.uv_scale, uv_offset: t.uv_offset, texel: t.texel, side: t.side, light_dir: t.light_dir, on_gpu: image.is_some() };
    (Some(publication), image.map(|g| g.texture_view.clone()))
}

/// The terrain map's stand-in: 1 x 1, never read (the block says not valid).
fn make_terrain_fallback(device: &RenderDevice) -> (Texture, TextureView) {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("medium_air_terrain_stand_in"),
        size: Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TERRAIN_FORMAT,
        usage: TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    (texture, view)
}

/// The cloud image's stand-in: 1 x 1, never read (the block says not valid).
fn make_cloud_fallback(device: &RenderDevice) -> (Texture, TextureView) {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("medium_air_cloud_stand_in"),
        size: Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::R8Unorm,
        usage: TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    (texture, view)
}

/// The cloud image's sampler: repeating and linear on both axes, as sun_visibility.rs's
/// plumbing samples it for surfaces.
fn make_cloud_sampler(device: &RenderDevice) -> Sampler {
    device.create_sampler(&SamplerDescriptor {
        label: Some("medium_air_cloud_sampler"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    })
}

/// Whether a texture format can be bound as the cloud image (filterable float).
fn cloud_format_ok(format: TextureFormat) -> bool {
    matches!(format.sample_type(None, None), Some(TextureSampleType::Float { filterable: true }))
}

/// Each frame while on, after [`drain_air`]: for each marked view whose projection and fog
/// can be read ([`unsupported_inputs`]) and whose grid the adapter can hold
/// ([`VolumeSupport`]), the pipelines asked for, the volume made (again when the grid
/// changed) and the uniform written; a view that cannot be drawn loses its GPU side and one
/// "not drawn" line says why.
#[allow(clippy::type_complexity)]
fn prepare_air(
    mut commands: Commands,
    (mut state, mut compose): (ResMut<AirState>, ResMut<crate::medium_air_compose::ComposePipelines>),
    (cache, assets, fullscreen): (Res<PipelineCache>, Res<AssetServer>, Res<bevy::core_pipeline::FullscreenShader>),
    (device, queue, adapter, info): (Res<RenderDevice>, Res<RenderQueue>, Res<RenderAdapter>, Option<Res<RenderAdapterInfo>>),
    (terrain, images): (Option<Res<crate::terrain_shadows::TerrainShadows>>, Res<RenderAssets<GpuImage>>),
    mut views: Query<(Entity, &ExtractedView, &AirCamera, &Msaa, Option<&MainPassResolutionOverride>, Option<&mut AirViewGpu>)>,
) {
    state.drawn = false;
    let limits = device.limits();
    let fits = workgroup_fits(&limits);
    let kind = info.map(|i| (i.vendor, i.device_type));
    for (entity, view, cam, msaa, resolution, gpu) in &mut views {
        let grid = cam.grid;
        let refused = match unsupported_inputs(&view.clip_from_view, cam.fog, cam.fog_colour) {
            Some(reason) => Err(reason.to_string()),
            None => {
                state.support.check(|| format_supported(&adapter, &limits), limits.max_texture_dimension_3d, &grid)
            }
        };
        if let Err(reason) = refused {
            state.tell(&reason);
            if gpu.is_some() {
                commands.entity(entity).try_remove::<AirViewGpu>();
            }
            continue;
        }
        let (walk, why) = GridWalk::choose(aids().walk, kind, fits, grid.nz);
        state.walk = walk;
        if state.told_walk != Some(walk) {
            state.told_walk = Some(walk);
            info!("volumetric medium: light in the air: grid walk {} ({why})", walk.name());
        }
        if state.grid_pipelines[walk as usize].is_none() {
            let layout = grid_layout();
            state.grid_pipelines[walk as usize] = Some(cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some("medium_air_grid_pipeline".into()),
                layout: vec![layout.clone()],
                shader: assets.load("embedded://nvforever/medium_air_grid.wesl"),
                shader_defs: walk.defs().iter().map(|d| (*d).into()).collect(),
                entry_point: Some("air_grid".into()),
                zero_initialize_workgroup_memory: false,
                ..default()
            }));
            state.grid_layout = Some(layout);
        }
        if cam.tune.filter && state.filter_pipeline.is_none() {
            let layout = filter_layout();
            state.filter_pipeline = Some(cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some("medium_air_filter_pipeline".into()),
                layout: vec![layout.clone()],
                shader: assets.load("embedded://nvforever/medium_air_filter.wesl"),
                entry_point: Some("air_filter".into()),
                zero_initialize_workgroup_memory: false,
                ..default()
            }));
            state.filter_layout = Some(layout);
        }
        let multisampled = msaa.samples() > 1;
        let wanted_compose = crate::medium_air_compose::draws(cam.tune.view);
        let compose_id = wanted_compose.then(|| ComposeId {
            id: compose.pipeline(&cache, &assets, &fullscreen, view.target_format, multisampled),
            format: view.target_format,
            multisampled,
        });
        let phase = cam.tune.phase();
        if state.normed.is_none_or(|n| n.phase != phase) {
            state.normed = Some(phase.normed());
        }
        let Some(normed) = state.normed else { continue };
        let render = resolution.map_or(view.viewport.zw(), |r| r.0).max(UVec2::ONE);
        let inputs = AirViewInputs { world_from_view: view.world_from_view.to_matrix(), clip_from_view: view.clip_from_view, origin: view.viewport.xy(), render };
        // The terrain map: its block, and its view only while valid for the current light.
        let (publication, map_view) = terrain_publication(terrain.as_deref(), &images);
        let block = terrain_block(publication.as_ref(), cam.to_light);
        let in_use = block.terrain.x > 0.5;
        let terrain_view = map_view.filter(|_| in_use);
        if terrain_view.is_none() {
            state.terrain_fallback.get_or_insert_with(|| make_terrain_fallback(&device));
        }
        if state.told_terrain != Some(in_use) {
            state.told_terrain = Some(in_use);
            match (in_use, publication) {
                (true, _) => info!("volumetric medium: light in the air: terrain map in use ({} texels of {:.0} units)", block.terrain.w, block.terrain.z),
                (false, None) => info!("volumetric medium: light in the air: terrain map not in use (none published)"),
                (false, Some(p)) => info!(
                    "volumetric medium: light in the air: terrain map not in use (on the GPU {}, side {}, texel {:.0}, its light against the current {:.4})",
                    p.on_gpu, p.side, p.texel, block.terrain.y
                ),
            }
        }
        // The Cloud shadows image: its block, and its view while on the GPU.
        let cloud_image = state.cloud.as_ref().and_then(|c| images.get(c.image.id())).filter(|g| cloud_format_ok(g.texture_descriptor.format));
        let cloud_view = cloud_image.map(|g| g.texture_view.clone());
        let mut clouds = cloud_block(state.cloud.as_ref(), cloud_view.is_some());
        // The mean of K over the pattern's repeat for the current light (cloud_mode.y; 1 when
        // not in use, or from the plane up).
        clouds.mode.y = cloud_mean_k(&clouds, state.cloud_mean, cam.to_light.y, inputs.world_from_view.w_axis.y);
        // The clouds' own transmittance (cloud_mode.z), always a number: 1 with none.
        clouds.mode.z = flat_deck(state.cloud_deck, inputs.world_from_view.w_axis.y);
        if cloud_view.is_none() {
            state.cloud_fallback.get_or_insert_with(|| make_cloud_fallback(&device));
        }
        state.cloud_sampler.get_or_insert_with(|| make_cloud_sampler(&device));
        let why = match (state.cloud.as_ref(), clouds.mode.x > 0.5) {
            // In use: the line again when the mean moves by a hundredth.
            (_, true) => format!("{:.2}", clouds.mode.y),
            (None, _) => "nothing published: the Cloud shadows row is off, indoors, or no textured cloud layer".to_string(),
            (Some(_), _) if cloud_view.is_none() => "the image is not on the GPU".to_string(),
            (Some(_), _) => "the published fields are not finite".to_string(),
        };
        if state.told_cloud.as_ref() != Some(&why) {
            if clouds.mode.x > 0.5 {
                let source = if state.cloud_mean.is_some() { "from the image" } else { "a stand-in: the image's data not read" };
                info!(
                    "volumetric medium: light in the air: cloud shadow in use (plane {:.0} units up, strength {:.2}, mean transmittance {:.2}, {source})",
                    clouds.cloud.x, clouds.cloud.y, clouds.mode.y
                );
            } else {
                info!("volumetric medium: light in the air: cloud shadow not in use ({why})");
            }
            state.told_cloud = Some(why);
        }
        let uniform = air_uniform(cam, &normed, &inputs).with_terrain(block).with_clouds(clouds);
        match gpu {
            Some(mut gpu) if gpu.grid == grid && gpu.filter_target.is_some() == cam.tune.filter => {
                gpu.uniform.set(uniform);
                gpu.uniform.write_buffer(&device, &queue);
                gpu.view = cam.tune.view;
                gpu.compose = compose_id;
                gpu.terrain_view = terrain_view;
                gpu.cloud_view = cloud_view;
            }
            _ => {
                let (volume, grid_view) = make_volume(&device, &grid, "medium_air_volume");
                // With FILTER on a second volume of the same size, the one the composite reads.
                let (filtered, filter_target) = if cam.tune.filter {
                    let (texture, view) = make_volume(&device, &grid, "medium_air_volume_filtered");
                    (Some(texture), Some(view))
                } else {
                    (None, None)
                };
                let volume_view = filter_target.clone().unwrap_or_else(|| grid_view.clone());
                let volumes = 1 + u64::from(cam.tune.filter);
                let mib = (grid.bytes(AIR_TEXEL_BYTES) * volumes) as f64 / (1024.0 * 1024.0);
                info!(
                    "volumetric medium: light in the air: grid {}x{}x{} to {:.0} units ({mib:.1} MiB in {volumes} volume{}, filter {}), view {}",
                    grid.nx,
                    grid.ny,
                    grid.nz,
                    grid.depth,
                    if volumes == 1 { "" } else { "s" },
                    if cam.tune.filter { "3x3" } else { "off" },
                    view_name(cam.tune.view)
                );
                let mut buffer = UniformBuffer::from(uniform);
                buffer.set_label(Some("medium_air_uniform"));
                buffer.write_buffer(&device, &queue);
                commands.entity(entity).try_insert(AirViewGpu { grid, _volumes: (volume, filtered), grid_view, filter_target, volume_view, uniform: buffer, view: cam.tune.view, compose: compose_id, terrain_view, cloud_view });
            }
        }
        state.held = true;
    }
}

/// A view [`prepare_air`] prepared this frame (its volume made, the device able to hold it).
type PreparedView = (With<ExtractedView>, With<AirViewGpu>);

/// Each frame while on with a prepared view, after the surfaces' cloud shadow is decided (sun_visibility.rs
/// `prepare_sun_visibility`) and before the bind groups: the light volume for the grid's K, with the
/// surfaces' own window, strength and levels, in the frames whose surfaces read it
/// ([`air_cloud_source`]). The variant's two pipelines (the grid's for this frame's [`GridWalk`],
/// and the mean's) are asked for as soon as the volume may become
/// the source (`CloudVolumeFrame::expected`: never while the Volumetric clouds row is off; with it on and
/// Cloud shadows the aids that force the volume ask for them whatever is traced), so they are
/// usually there when the first front fades in; until the ones this frame needs are, the flat variant
/// reads the weather's image. One log line whenever the source in use changes.
fn prepare_air_volume(
    mut state: ResMut<AirState>,
    frame: Option<Res<CloudVolumeFrame>>,
    cams: Query<&AirCamera, PreparedView>,
    (cache, assets): (Res<PipelineCache>, Res<AssetServer>),
    (device, queue): (Res<RenderDevice>, Res<RenderQueue>),
) {
    state.volume.levels = None;
    if cams.is_empty() {
        return;
    }
    let Some(frame) = frame else { return };
    let walk = state.walk;
    let volume = &mut state.volume;
    volume.mean_wanted = cams.iter().any(|c| wants_cloud_mean(&c.tune));
    let storage = device.limits().max_storage_buffers_per_shader_stage >= 1;
    if frame.expected && storage && (volume.grids[walk as usize].is_none() || volume.mean_pipeline.is_none()) {
        let layout = grid_volume_layout();
        let ask = |label: &'static str, entry: &'static str, defs: &[&'static str]| {
            cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some(label.into()),
                layout: vec![layout.clone()],
                shader: assets.load("embedded://nvforever/medium_air_grid.wesl"),
                shader_defs: std::iter::once(&AIR_VOLUME_DEF).chain(defs).map(|d| (*d).into()).collect(),
                entry_point: Some(entry.into()),
                zero_initialize_workgroup_memory: false,
                ..default()
            })
        };
        volume.grids[walk as usize].get_or_insert_with(|| ask("medium_air_grid_volume_pipeline", "air_grid", walk.defs()));
        volume.mean_pipeline.get_or_insert_with(|| ask("medium_air_cloud_mean_pipeline", "air_cloud_mean", &[]));
        volume.layout = Some(layout);
    }
    let pipelines = if storage { volume.pipelines(&cache, walk) } else { VolumePipelines::NoStorage };
    let source = air_cloud_source(frame.source, pipelines);
    if volume.told != Some(source) {
        if source != AirCloudSource::Surfaces(CloudSourceInUse::Off) || volume.told.is_some() {
            info!("volumetric medium: light in the air: cloud shadow source {}", source.line());
        }
        volume.told = Some(source);
    }
    let (true, Some(block), Some(levels)) = (source.reads_volume(), air_light_volume(&frame.uniform), frame.levels.clone()) else { return };
    let uniform = volume.uniform.get_or_insert_with(|| {
        let mut buffer = UniformBuffer::from(block);
        buffer.set_label(Some("medium_air_light_volume"));
        buffer
    });
    uniform.set(block);
    uniform.write_buffer(&device, &queue);
    volume.mean.get_or_insert_with(|| {
        device.create_buffer(&BufferDescriptor { label: Some("medium_air_cloud_mean"), size: CLOUD_MEAN_BYTES, usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC, mapped_at_creation: false })
    });
    volume.levels = Some(levels);
}

/// Whether the grid pass (and so the filter) runs for a view this frame: the views
/// that show the light (`none`, `delta`, `rays`) and the `off` baseline (everything running
/// but the correction, for paired pictures) only while the light draws (`light_on`: the gate
/// open and the display-value fog); the diagnostic views the composite shows whatever the gate
/// (`visibility`, `lit`, which read the volume, and `slice`, `share`, which do not but are
/// drawn only in a frame the grid was drawn) always.
pub fn grid_dispatched(view: AirView, light_on: bool) -> bool {
    match view {
        AirView::None | AirView::Delta | AirView::Rays | AirView::Off => light_on,
        AirView::Visibility | AirView::Lit | AirView::Slice | AirView::Share => true,
    }
}

/// The grid pass: one dispatch over the columns into the view's volume. Not drawn (and the
/// composite then skipped) while the pipeline compiles or Bevy's light bindings are missing. While
/// the surfaces read the clouds' light volume ([`prepare_air_volume`]) its variant runs, after (while
/// the composite reads the clouds' mean, [`wants_cloud_mean`]) one dispatch of the mean pass
/// whose result is copied over the uniform's `cloud_mode.yz`: the reference visibility is then the
/// mean of the K that is read, and the deck's share of the clouds it is read from. The first such draw of an activation is logged.
#[allow(clippy::type_complexity)]
pub(crate) fn air_grid_pass(
    view: ViewQuery<(&AirViewGpu, Option<&ViewShadowBindings>, Option<&ViewLightsUniformOffset>)>,
    mut state: ResMut<AirState>,
    (light_meta, samplers): (Res<LightMeta>, Option<Res<ShadowSamplers>>),
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (gpu, shadows, offset) = view.into_inner();
    // Gated off (night without a moon, indoors, an unusable fog) in a view that shows the light: no grid and
    // no filter; the volumes stay allocated, and `drawn` stays false so the composite (which
    // skips such a frame anyway) never reads a volume not written this frame.
    if !grid_dispatched(gpu.view, crate::medium_air_compose::light_on(gpu.uniform.get())) {
        return;
    }
    let walk = state.walk;
    let volume = state.volume.pass(&cache, walk);
    let (Some(id), Some(layout)) = (state.grid_pipelines[walk as usize], state.grid_layout.clone()) else { return };
    let pipeline = match cache.get_compute_pipeline_state(id) {
        // The light volume's variant is ready: the flat one's state does not matter.
        _ if volume.is_some() => volume.as_ref().map(|v| v.grid),
        CachedPipelineState::Ok(_) => cache.get_compute_pipeline(id),
        // The cache's waiting states (a shader or an import not loaded yet): it queues the
        // pipeline again by itself (bevy_render pipeline_cache.rs, "Retry"), so not ready yet.
        CachedPipelineState::Err(ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable) => return,
        CachedPipelineState::Err(e) => {
            let reason = format!("the grid pipeline failed: {e}");
            state.tell(&reason);
            return;
        }
        _ => return,
    };
    let Some(pipeline) = pipeline else { return };
    let (Some(shadows), Some(offset), Some(samplers)) = (shadows, offset, samplers) else {
        state.tell("the view has no light or shadow bindings");
        return;
    };
    let (Some(uniform), Some(lights)) = (gpu.uniform.binding(), light_meta.view_gpu_lights.binding()) else {
        state.tell("a uniform buffer is missing");
        return;
    };
    let Some(terrain) = gpu.terrain_view.clone().or_else(|| state.terrain_fallback.as_ref().map(|(_, v)| v.clone())) else {
        state.tell("no terrain map or stand-in to bind");
        return;
    };
    let cloud = gpu.cloud_view.clone().or_else(|| state.cloud_fallback.as_ref().map(|(_, v)| v.clone()));
    let (Some(cloud), Some(cloud_sampler)) = (cloud, state.cloud_sampler.clone()) else {
        state.tell("no cloud image or stand-in to bind");
        return;
    };
    let depths = &shadows.directional_light_depth_texture_view;
    let comparison = &samplers.directional_light_comparison_sampler;
    let group = match volume.as_ref() {
        None => device.create_bind_group(
            "medium_air_grid_bind_group",
            &cache.get_bind_group_layout(&layout),
            &BindGroupEntries::sequential((uniform, &gpu.grid_view, lights, depths, comparison, &terrain, &cloud, &cloud_sampler)),
        ),
        Some(v) => device.create_bind_group(
            "medium_air_grid_volume_bind_group",
            &cache.get_bind_group_layout(&v.layout),
            &BindGroupEntries::sequential((
                uniform,
                &gpu.grid_view,
                lights,
                depths,
                comparison,
                &terrain,
                &cloud,
                &cloud_sampler,
                v.uniform.as_entire_binding(),
                &v.levels[0],
                &v.levels[1],
                v.mean_buffer.as_entire_binding(),
            )),
        ),
    };
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut mean_copied = false;
    if let (Some(v), Some(mean), Some(target)) = (volume.as_ref(), volume.as_ref().and_then(|v| v.mean), gpu.uniform.buffer()) {
        {
            let mut pass = ctx.command_encoder().begin_compute_pass(&ComputePassDescriptor { label: Some("medium_air_cloud_mean"), timestamp_writes: None });
            let span = diagnostics.time_span(&mut pass, "nvf_medium_air_cloud_mean");
            pass.set_pipeline(mean);
            pass.set_bind_group(0, &group, &[offset.offset]);
            pass.dispatch_workgroups(1, 1, 1);
            span.end(&mut pass);
        }
        ctx.command_encoder().copy_buffer_to_buffer(&v.mean_buffer, 0, target, CLOUD_MEAN_OFFSET, CLOUD_MEAN_COPY);
        mean_copied = true;
    }
    {
        let mut pass = ctx.command_encoder().begin_compute_pass(&ComputePassDescriptor { label: Some("medium_air_grid"), timestamp_writes: None });
        let span = diagnostics.time_span(&mut pass, "nvf_medium_air");
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[offset.offset]);
        // One group deep: its four layers of invocations are the columns' chunks of slices.
        pass.dispatch_workgroups(gpu.grid.nx.div_ceil(WORKGROUP), gpu.grid.ny.div_ceil(WORKGROUP), 1);
        span.end(&mut pass);
    }
    state.drawn = true;
    if !state.told_drawn {
        state.told_drawn = true;
        info!("volumetric medium: light in the air: grid pass drawn");
    }
    if volume.is_some() && !state.volume.told_drawn {
        state.volume.told_drawn = true;
        info!(
            "volumetric medium: light in the air: grid drawn from the clouds' light volume ({})",
            if mean_copied { "its mean transmittance copied into the reference" } else { "no mean: nothing reads it" }
        );
    }
}

/// medium_air_filter.wesl's bindings 0-1: the grid volume (unfilterable float, read with
/// textureLoad) and the filtered volume (write-only storage).
fn filter_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "medium_air_filter_layout",
        &BindGroupLayoutEntries::sequential(ShaderStages::COMPUTE, (texture_3d(TextureSampleType::Float { filterable: false }), texture_storage_3d(AIR_FORMAT, StorageTextureAccess::WriteOnly))),
    )
}

/// The filter pass: with `FILTER` on and the grid drawn this frame, one
/// dispatch over every texel (8 x 8 columns per group, one group layer per slice) from the
/// grid volume into the filtered one the composite reads. While its pipeline compiles, or if
/// it failed, the frame counts as not drawn so the composite never reads a filtered volume
/// that was not written this frame.
pub(crate) fn air_filter_pass(view: ViewQuery<&AirViewGpu>, mut state: ResMut<AirState>, cache: Res<PipelineCache>, device: Res<RenderDevice>, mut ctx: RenderContext) {
    let gpu = view.into_inner();
    let Some(target) = gpu.filter_target.as_ref() else { return };
    if !state.drawn {
        return;
    }
    let (Some(id), Some(layout)) = (state.filter_pipeline, state.filter_layout.clone()) else {
        state.drawn = false;
        return;
    };
    let pipeline = match cache.get_compute_pipeline_state(id) {
        CachedPipelineState::Ok(_) => cache.get_compute_pipeline(id),
        CachedPipelineState::Err(ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable) => None,
        CachedPipelineState::Err(e) => {
            let reason = format!("the filter pipeline failed: {e}");
            state.tell(&reason);
            None
        }
        _ => None,
    };
    let Some(pipeline) = pipeline else {
        state.drawn = false;
        return;
    };
    let layout = cache.get_bind_group_layout(&layout);
    let group = device.create_bind_group("medium_air_filter_bind_group", &layout, &BindGroupEntries::sequential((&gpu.grid_view, target)));
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    {
        let mut pass = ctx.command_encoder().begin_compute_pass(&ComputePassDescriptor { label: Some("medium_air_filter"), timestamp_writes: None });
        let span = diagnostics.time_span(&mut pass, "nvf_medium_air_filter");
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(gpu.grid.nx.div_ceil(WORKGROUP), gpu.grid.ny.div_ceil(WORKGROUP), gpu.grid.nz);
        span.end(&mut pass);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::medium_air::AirOccluders;

    const GRID: &str = include_str!("medium_air_grid.wesl");
    const COMPOSE: &str = include_str!("medium_air_compose.wesl");

    fn vendored(path: &str) -> String {
        let full = format!("{}/../vendor/bevy_pbr/src/render/{path}", env!("CARGO_MANIFEST_DIR"));
        crate::lights::unix_lines(&std::fs::read_to_string(full).expect("vendored bevy_pbr shader"))
    }

    /// The declaration starting at `start` up to its closing brace (and a `;` after it).
    pub(crate) fn block<'a>(source: &'a str, start: &str) -> &'a str {
        let a = source.find(start).unwrap_or_else(|| panic!("{start}"));
        let mut b = a + source[a..].find("\n}").unwrap_or_else(|| panic!("end of {start}")) + 2;
        if source[b..].starts_with(';') {
            b += 1;
        }
        &source[a..b]
    }

    /// A shader with its WESL parts resolved as plain WGSL: `prelude` first (stand-ins for the
    /// imports), import lines dropped, one side of each single-line `@if(NAME)` / `@if(!NAME)`.
    pub(crate) fn plain_wgsl(source: &str, defs: &[&str], prelude: &str) -> String {
        let mut out = String::from(prelude);
        for line in source.replace("\r\n", "\n").lines() {
            let mut line = line;
            if let Some(rest) = line.strip_prefix("@if(") {
                let (cond, rest) = rest.split_once(") ").expect("an @if(NAME) line");
                let (negated, name) = cond.strip_prefix('!').map_or((false, cond), |n| (true, n));
                assert!(name.chars().all(|c| c.is_ascii_uppercase() || c == '_'), "a WESL condition this test does not resolve: {line}");
                if defs.contains(&name) == negated {
                    continue;
                }
                line = rest;
            }
            if line.starts_with("import ") {
                continue;
            }
            assert!(!line.contains("@if"), "a WESL condition this test does not resolve: {line}");
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    pub(crate) fn validate(source: &str, entry: &str, stage: wgpu::naga::ShaderStage, what: &str) {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{what}: {}", e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{what}: {e:?}"));
        let found = module.entry_points.iter().find(|e| e.name == entry).unwrap_or_else(|| panic!("{what}: entry {entry}"));
        assert_eq!(found.stage, stage, "{what}");
    }

    /// Bevy's types the grid shader imports, cut from the vendored mesh_view_types.wesl with
    /// its constants filled in (light.rs: 10 directional lights, 4 cascades, 8 rect lights).
    fn bevy_stand_ins() -> String {
        let types = vendored("mesh_view_types.wesl");
        let mut out = String::new();
        for start in ["struct DirectionalCascade {", "struct DirectionalLight {", "struct RectLight {", "struct Lights {"] {
            out.push_str(block(&types, start));
            out.push('\n');
        }
        for name in ["DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT", "DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT"] {
            let line = types.lines().find(|l| l.starts_with(&format!("const {name}:"))).expect(name);
            out.push_str(line);
            out.push('\n');
        }
        let out = out.replace("constants::MAX_CASCADES_PER_LIGHT", "4").replace("u32(constants::MAX_DIRECTIONAL_LIGHTS)", "10u").replace("u32(constants::MAX_RECT_LIGHTS)", "8u");
        assert!(!out.contains("constants::"), "a constant this test does not fill in");
        out
    }

    /// The grid shader of one variant as plain WGSL text: [`AIR_VOLUME_DEF`]'s and the walk's
    /// conditions resolved and, in the light volume's variant, the shared reader's text in its
    /// import's place.
    fn grid_variant(volume: bool, walk: GridWalk) -> String {
        const IMPORT: &str = "import nvforever::light_volume_reader::{LightVolumeWindow, lvr_optical_depth};";
        let source = GRID.replace("\r\n", "\n");
        let has = |def: &str| walk.defs().contains(&def);
        let known = [(AIR_VOLUME_DEF, volume), ("AIR_CHUNKED", has("AIR_CHUNKED")), ("AIR_LONG_CHUNKS", has("AIR_LONG_CHUNKS"))];
        let resolved = crate::sun_visibility::tests::resolve_known(&source, &known);
        assert!(!resolved.contains("@if("), "volume {volume}, {walk:?}: every condition resolved");
        assert_eq!(resolved.lines().filter(|l| *l == IMPORT).count(), usize::from(volume), "volume {volume}: the reader's import");
        let reader = include_str!("light_volume_reader.wesl").replace("\r\n", "\n");
        let linked = resolved.lines().map(|l| if l == IMPORT { reader.clone() } else { l.to_string() }).collect::<Vec<_>>().join("\n");
        plain_wgsl(&linked, &[], &bevy_stand_ins())
    }

    #[test]
    fn the_grid_shader_is_valid_wgsl() {
        let compute = wgpu::naga::ShaderStage::Compute;
        for walk in GridWalk::ALL {
            for volume in [false, true] {
                let (text, what) = (grid_variant(volume, walk), format!("grid, light volume {volume}, {walk:?}"));
                validate(&text, "air_grid", compute, &what);
                let module = wgpu::naga::front::wgsl::parse_str(&text).expect("validated");
                assert_eq!(module.entry_points.iter().find(|e| e.name == "air_grid").expect("the entry").workgroup_size, [WORKGROUP, WORKGROUP, chunks_of(walk)], "{what}");
            }
            validate(&grid_variant(true, walk), "air_cloud_mean", compute, "the mean, light volume");
        }
    }

    /// Without the light volume's def the grid is the flat path alone (the Volumetric clouds row off:
    /// the layout's bindings 0-7, no reader, no mean entry, K the image's); with it the variant adds
    /// the uniform, the two levels and the mean's buffer after them.
    #[test]
    fn the_flat_variant_names_nothing_of_the_light_volume() {
        use crate::sun_visibility::tests::code;
        let (flat, volume) = (code(&grid_variant(false, GridWalk::Chunks)), code(&grid_variant(true, GridWalk::Chunks)));
        for s in ["@binding(8)", "@binding(9)", "@binding(10)", "@binding(11)", "AirLightVolume", "LightVolumeWindow", "fn lvr_optical_depth(", "air_mean", "fn air_cloud_mean("] {
            assert!(!flat.contains(s), "flat: {s}");
            assert!(volume.contains(s), "light volume: {s}");
        }
        for text in [&flat, &volume] {
            assert_eq!(text.matches("fn air_clouds(").count(), 1);
            assert_eq!(text.matches("fn air_cloud_weather(").count(), 1);
            assert_eq!(text.matches("textureSampleLevel(cloud_map").count(), 1, "the image is read in one place");
        }
        assert!(block(&flat, "fn air_clouds(").ends_with("{\n    return air_cloud_weather(P, L);\n}"), "flat: K is the image's");
        for binding in [
            "@if(AIR_LIGHT_VOLUME) @group(0) @binding(8) var<uniform> air_volume: AirLightVolume;",
            "@if(AIR_LIGHT_VOLUME) @group(0) @binding(9) var light_near: texture_2d_array<f32>;",
            "@if(AIR_LIGHT_VOLUME) @group(0) @binding(10) var light_far: texture_2d_array<f32>;",
            "@if(AIR_LIGHT_VOLUME) @group(0) @binding(11) var<storage, read_write> air_mean: vec4<f32>;",
        ] {
            assert!(GRID.contains(binding), "{binding}");
        }
        assert_eq!(AIR_VOLUME_DEF, "AIR_LIGHT_VOLUME");
        assert_eq!(AirLightVolume::min_size().get(), 16 + LightVolumeWindow::min_size().get());
        // The layouts: the flat one's entries come first in the variant's, unchanged.
        let (a, b) = (grid_layout().entries, grid_volume_layout().entries);
        assert_eq!((a.len(), b.len()), (8, 12));
        assert_eq!(a[..], b[..8]);
        assert!(b.iter().enumerate().all(|(i, e)| e.binding == i as u32));
    }

    /// The air reads the light volume through the surfaces' shared reader, not a copy: no function
    /// of the reader (`lvr_`) is declared in the grid's own text, the one read passes the window as
    /// it came, and what follows the read is the surfaces' text (sun_visibility.wesl
    /// `nvf_cloud_transmittance`) with the names changed.
    #[test]
    fn the_air_reads_the_light_volume_as_the_surfaces_do() {
        use crate::sun_visibility::tests::{block_after, code, code_lines};
        let grid = GRID.replace("\r\n", "\n");
        assert!(!code(&grid).contains("fn lvr_") && !code(&grid).contains("struct LightVolumeWindow"), "no copy of the reader");
        assert_eq!(code(&grid).matches("lvr_optical_depth(").count(), 2, "one read per sample, one per point of the mean");
        let air = block_after(&grid, "@if(AIR_LIGHT_VOLUME)\nfn air_clouds(P: vec3<f32>, L: vec3<f32>) -> f32");
        assert!(air.contains("let r = lvr_optical_depth(light_near, light_far, cloud_sampler, air_volume.window, P, L);"));
        assert!(!air.contains("for ") && !air.contains("loop"), "no march");
        let surfaces = include_str!("sun_visibility.wesl").replace("\r\n", "\n");
        let theirs = block_after(&surfaces, "@if(NVF_CLOUD_VISIBILITY && NVF_CLOUD_VOLUME)\nfn nvf_cloud_transmittance(P: vec3<f32>, L: vec3<f32>) -> f32");
        let tail = |text: &str| -> Vec<String> {
            let lines = code_lines(text);
            let at = lines.iter().position(|l| l.trim() == "var other = 1.0;").expect("the fallback");
            lines[at..].iter().map(|l| l.to_string()).collect()
        };
        let renamed: Vec<String> = tail(theirs).iter().map(|l| l.replace("nvf_cloud_weather", "air_cloud_weather").replace("nvf_sun.volume", "air_volume.cloud")).collect();
        assert_eq!(tail(air), renamed, "the surfaces' mix, word for word");
        assert!(theirs.contains("if up <= nvf_sun.volume.y {") && air.contains("if up <= air_volume.cloud.y {"), "the same band's foot");
        assert!(theirs.contains("lvr_optical_depth(nvf_light_near, nvf_light_far, nvf_sun_visibility_sampler, nvf_sun.light_volume, P, L)"));
        // The grid hands the same P and L as for the terrain: the world position and the light.
        assert!(grid.contains("k = air_clouds(world, air.light.xyz);"));
        // The mean takes the same volume K (the read and the two lines after it are `air_clouds`'),
        // on the far level's node spacing, and never the image.
        let part = block_after(&grid, "@if(AIR_LIGHT_VOLUME)\nfn air_volume_part(P: vec3<f32>, L: vec3<f32>) -> vec3<f32>");
        for start in ["let r = ", "let k = ", "let faded = ", "if up <= "] {
            let line = |text: &str| text.lines().find(|l| l.trim().starts_with(start)).map(|l| l.trim().to_string()).expect(start);
            assert_eq!(line(part), line(air), "{start}");
        }
        assert!(part.contains("return vec3<f32>(r.y * faded, 1.0 - r.y, r.y * exp(-r.x));"));
        let mean = block_after(&grid, "fn air_cloud_mean(");
        assert!(mean.contains("let other = select(air.cloud_mode.y, 1.0, air_volume.cloud.w > 0.5 || origin.y >= air.cloud.x);"));
        assert!(mean.contains("sum += air_volume_part(vec3<f32>(origin.x + at.x, origin.y, origin.z + at.y), air.light.xyz);"));
        assert!(mean.contains("let mean = total / f32(AIR_MEAN_SIDE * AIR_MEAN_SIDE);\n        let mixed = clamp(mean.x + mean.y * other, AIR_MEAN_LEAST, 1.0);"));
        assert!(mean.contains("air_mean = vec4<f32>(select(other, mixed, mean.y < 1.0), deck, 0.0, 0.0);"));
        assert!(mean.contains("var deck = air.cloud_mode.z;\n        if mean.y < 1.0 {\n            let covered = 1.0 - mean.y;"));
        assert!(mean.contains("let above = pow(clamp(mean.z / covered, AIR_DECK_THIN, 1.0), clamp(air.light.y, AIR_DECK_UP, 1.0));\n            deck = covered * above + mean.y * air.cloud_mode.z;"));
        assert!(grid.contains(&format!("const AIR_DECK_UP: f32 = {:?};", medium_air::AIR_DECK_UP)));
        assert!(grid.contains("const AIR_DECK_THIN: f32 = 1e-4;") && medium_air::AIR_DECK_THIN == 1e-4);
        assert!(grid.contains("const AIR_MEAN_LEAST: f32 = 1e-6;"));
        assert!(!code(mean).contains("air_clouds(") && !code(mean).contains("air_cloud_weather("));
        assert!(grid.contains(&format!("const AIR_MEAN_STEP: f32 = {:?};", crate::volumetric_clouds_lightvol::LV_FAR_ACROSS)));
    }

    /// CPU mirror of medium_air_grid.wesl `air_clouds` in the light volume's variant, after the
    /// reader's result `r`; `weather` the image's K.
    fn volume_mirror(v: &AirLightVolume, up: f32, r: Vec2, weather: &dyn Fn() -> f32) -> f32 {
        let only = v.cloud.w > 0.5;
        if up <= v.cloud.y {
            return 1.0;
        }
        let other = if r.y < 1.0 && !only { weather() } else { 1.0 };
        if r.y <= 0.0 {
            return other;
        }
        let k = (1.0 - v.cloud.x * (1.0 - (-r.x).exp())).clamp(0.0, 1.0);
        let t = ((up - v.cloud.y) / (v.cloud.z - v.cloud.y)).clamp(0.0, 1.0);
        let faded = 1.0 + (k - 1.0) * (t * t * (3.0 - 2.0 * t));
        other + (faded - other) * r.y
    }

    /// A window about `cam` with the default levels, the near one's corner off the camera by odd
    /// shares of a cell (the snapped, moving origin and the drift).
    fn test_window(cam: Vec3, presence: [f32; 2]) -> LightVolumeWindow {
        use crate::volumetric_clouds_lightvol::{lvr_window, LvReaderLevel, LV_ACROSS, LV_FAR_ACROSS, LV_GUARD, LV_LEVEL_UP};
        let level = |across: f32, at: Vec2| Some(LvReaderLevel { set: 0, window: Vec4::new(at.x, at.y, across, LV_LEVEL_UP), nodes: 128, layers: 41 });
        lvr_window([level(LV_ACROSS, Vec2::new(63.3, 64.1)), level(LV_FAR_ACROSS, Vec2::new(62.7, 63.9))], (cam, 21_024.0, 6_000_000.0, LV_GUARD), presence)
    }

    /// A froxel's sample and a surface at the same world place have the same K: the sample's world
    /// position (camera translation + its world ray x view depth, with a turned view) goes to the
    /// volume's nodes by the surfaces' own window, whatever camera the window was measured from
    /// (a kept window of an earlier frame included), through both levels, the guard bands, the
    /// presences, the weather's fallback and the low-sun band.
    #[test]
    fn a_froxel_and_a_surface_at_one_place_have_one_cloud_shadow() {
        use crate::sun_visibility as sv;
        use crate::volumetric_clouds_lightvol::lvr_optical_depth;
        let eye = Vec3::new(-71_000.0, 9_300.0, 22_500.0);
        let view = Mat4::from_rotation_translation(Quat::from_euler(EulerRot::YXZ, 0.9, -0.15, 0.0), eye);
        let (sx, sy) = (1.1f32, 1.9f32);
        // Depths that vary over the nodes and the layers.
        let near = |n: IVec3| ((n.x * 7 + n.z * 3 + n.y) % 11) as f32 * 0.17;
        let far = |n: IVec3| ((n.x * 5 + n.z * 2 + n.y * 3) % 13) as f32 * 0.11;
        let packed = sv::SunVisibilityUniform::pack(&sv::SunVisibility { cloud_enabled: true, ..default() });
        let (mut whole, mut part, mut none, mut nodes) = (0, 0, 0, 0);
        for (window_cam, presence) in [(eye, [1.0, 1.0]), (eye + Vec3::new(40.0, -3.0, 25.0), [1.0, 1.0]), (eye, [0.4, 0.0]), (eye, [0.0, 0.7])] {
            let window = test_window(window_cam, presence);
            for l in [Vec3::new(0.4, 0.8, -0.3).normalize(), Vec3::new(0.95, 0.2, 0.1).normalize(), Vec3::new(0.998, 0.06, 0.0).normalize()] {
                for only in [false, true] {
                    let used = sv::CloudSourceInUse::Volume { near: presence[0] > 0.0, far: presence[1] > 0.0, only };
                    let u = sv::with_cloud_volume(packed, used, 0.6, window);
                    let block = air_light_volume(&u).expect("the volume in use");
                    assert_eq!((block.window, block.cloud.x), (window, 0.6));
                    for (i, j, z) in [(3, 5, 400.0f32), (80, 44, 3_000.0), (159, 87, 20_000.0), (40, 2, 48_000.0)] {
                        let ndc = Vec2::new((i as f32 + 0.5) / 160.0 * 2.0 - 1.0, 1.0 - (j as f32 + 0.5) / 88.0 * 2.0);
                        let ray = view.transform_vector3(Vec3::new(ndc.x / sx, ndc.y / sy, -1.0));
                        let p = view.w_axis.truncate() + ray * z;
                        let r = lvr_optical_depth(&block.window, &near, &far, p, l);
                        let k = volume_mirror(&block, l.y, r, &|| 0.8);
                        let ground = sv::cloud_transmittance(&u, l.y, lvr_optical_depth(&u.light_volume, &near, &far, p, l), &|| 0.8);
                        assert_eq!(k, ground, "at {p} toward {l}");
                        match r.y {
                            c if c >= 1.0 => whole += 1,
                            c if c > 0.0 => part += 1,
                            _ => none += 1,
                        }
                        // The node coordinate: a point under the floor moved along the light to it,
                        // measured from the window's camera.
                        // The bend (6,000,000) moves that place by less than a hundredth of a cell here.
                        if z <= 3_000.0 && presence == [1.0, 1.0] && l.y > 0.5 {
                            let rise = (21_024.0 - p.y) / l.y;
                            let node = Vec2::new(window.near.x, window.near.y) + (Vec2::new(p.x, p.z) + Vec2::new(l.x, l.z) * rise - Vec2::new(window_cam.x, window_cam.z)) / window.near.z;
                            let edge = node.min(Vec2::splat(127.0) - node).min_element();
                            if edge > 8.0 {
                                let at = crate::volumetric_clouds_lightvol::lv_trilinear(Vec3::new(node.x, 0.0, node.y), 41, &near);
                                nodes += 1;
                                assert!((at - r.x).abs() < 0.02, "the near level's floor at node {node}: {at} against {}", r.x);
                            }
                        }
                    }
                }
            }
        }
        assert!(whole > 0 && part > 0 && nodes >= 4, "the fixture covers: {whole} whole, {part} in part, {none} none");
        // The surfaces' debug views are theirs alone: the air reads the volume alone.
        let debug = sv::with_cloud_volume(packed, sv::CloudSourceInUse::Debug { near: true, far: true, view: sv::CloudSource::Share }, 0.6, test_window(eye, [1.0, 1.0]));
        assert_eq!(air_light_volume(&debug).expect("the volume").cloud.w, 1.0);
        // The weather's layers: no block, the flat variant.
        assert_eq!(air_light_volume(&packed), None);
    }

    /// The air's gate is the surfaces' decision word for word; only the volume's own pipelines can
    /// hold it back, to the weather's layers.
    #[test]
    fn the_airs_cloud_source_is_the_surfaces() {
        use crate::sun_visibility::{AutoGate, CloudSource, WeatherWhy};
        let gate = AutoGate { row: true, sky_shown: true, traced: true };
        let decide = |on: bool, asked: CloudSource, gate: AutoGate| CloudSourceInUse::decide(on, asked, [true; 2], gate);
        let volume = decide(true, CloudSource::Auto, gate);
        assert!(air_cloud_source(volume, VolumePipelines::Ready).reads_volume());
        assert!(air_cloud_source(volume, VolumePipelines::Ready).line().starts_with("the light volume (near and far levels)"));
        for (surfaces, why) in [
            (decide(true, CloudSource::Weather, gate), WeatherWhy::Asked),
            (decide(true, CloudSource::Auto, AutoGate { row: false, ..gate }), WeatherWhy::RowOff),
            (decide(true, CloudSource::Auto, AutoGate { sky_shown: false, ..gate }), WeatherWhy::SkyNotShown),
            (decide(true, CloudSource::Auto, AutoGate { traced: false, ..gate }), WeatherWhy::SkyNotShown),
            (CloudSourceInUse::decide(true, CloudSource::Auto, [false; 2], gate), WeatherWhy::NoFront),
            (volume.without_room(), WeatherWhy::NoRoom),
        ] {
            assert_eq!(surfaces, CloudSourceInUse::Weather { why });
            let source = air_cloud_source(surfaces, VolumePipelines::Ready);
            assert_eq!(source, AirCloudSource::Surfaces(surfaces));
            assert!(!source.reads_volume(), "{why:?}");
            assert!(source.line().starts_with("the weather's layers ("), "{why:?}");
        }
        assert!(!air_cloud_source(decide(false, CloudSource::Volume, gate), VolumePipelines::Ready).reads_volume(), "Cloud shadows off");
        for held in [VolumePipelines::Waiting, VolumePipelines::Failed, VolumePipelines::NoStorage] {
            let source = air_cloud_source(volume, held);
            assert_eq!(source, AirCloudSource::Held(held));
            assert!(!source.reads_volume() && source.line().starts_with("the weather's layers ("));
            // Nothing to hold back: the surfaces' word stands.
            assert_eq!(air_cloud_source(CloudSourceInUse::Off, held), AirCloudSource::Surfaces(CloudSourceInUse::Off));
        }
        let debug = air_cloud_source(CloudSourceInUse::Debug { near: true, far: false, view: CloudSource::Depth }, VolumePipelines::Ready);
        assert!(debug.reads_volume() && debug.line().starts_with("the light volume alone (near level"));
    }

    /// CPU mirror of medium_air_grid.wesl `air_cloud_mean` about `eye` for the light from `l`, with the
    /// image's block `flat` (its mean, its plane) and the levels' nodes from `near` and `far`; summed as
    /// the shader does, 64 groups of 4 x 4 points and then the groups.
    fn mean_mirror(v: &AirLightVolume, flat: &AirCloudBlock, eye: Vec3, l: Vec3, near: &dyn Fn(IVec3) -> f32, far: &dyn Fn(IVec3) -> f32) -> f32 {
        mean_mirror_all(v, flat, eye, l, near, far).x
    }

    /// The same with the deck's transmittance (the two numbers copied over `cloud_mode.yz`) and the
    /// covered share.
    fn mean_mirror_all(v: &AirLightVolume, flat: &AirCloudBlock, eye: Vec3, l: Vec3, near: &dyn Fn(IVec3) -> f32, far: &dyn Fn(IVec3) -> f32) -> Vec3 {
        let other = if v.cloud.w > 0.5 || eye.y >= flat.cloud.x { 1.0 } else { flat.mode.y };
        let part = |p: Vec3| {
            if l.y <= v.cloud.y {
                return Vec3::Y;
            }
            let r = crate::volumetric_clouds_lightvol::lvr_optical_depth(&v.window, near, far, p, l);
            if r.y <= 0.0 {
                return Vec3::Y;
            }
            let k = (1.0 - v.cloud.x * (1.0 - (-r.x).exp())).clamp(0.0, 1.0);
            let t = ((l.y - v.cloud.y) / (v.cloud.z - v.cloud.y)).clamp(0.0, 1.0);
            let fade = t * t * (3.0 - 2.0 * t);
            Vec3::new(r.y * ((1.0 - fade) + k * fade), 1.0 - r.y, r.y * (-r.x).exp())
        };
        let points = mean_points(eye);
        let mut total = Vec3::ZERO;
        for group in 0..64usize {
            let mut sum = Vec3::ZERO;
            for n in 0..16usize {
                sum += part(points[(group / 8 * 4 + n / 4) * 32 + group % 8 * 4 + n % 4]);
            }
            total += sum;
        }
        let mean = total / 1024.0;
        if mean.y < 1.0 {
            let covered = 1.0 - mean.y;
            let deck = medium_air::deck_transmittance(mean.z / covered, covered, flat.mode.z, l.y);
            Vec3::new((mean.x + mean.y * other).clamp(1e-6, 1.0), deck, covered)
        } else {
            Vec3::new(other, flat.mode.z, 0.0)
        }
    }

    /// The mean pass's 32 x 32 points.
    fn mean_points(eye: Vec3) -> Vec<Vec3> {
        (0..1024).map(|n| eye + Vec3::new(((n % 32) as f32 + 0.5 - 16.0) * 4_000.0, 0.0, ((n / 32) as f32 + 0.5 - 16.0) * 4_000.0)).collect()
    }

    /// The reference does not change before the volume contributes. With the
    /// checker (2,000-unit cells, whole-pattern mean of K 0.5) the mean pass's lattice, 4,000 apart,
    /// can meet clear cells alone (a lattice mean of the image is 1); the mirror is 0.5 exactly at
    /// zero coverage, moves continuously with the levels' presence, and is the volume's own mean,
    /// free of the image, under full coverage.
    #[test]
    fn the_reference_starts_at_the_images_exact_mean() {
        use crate::sun_visibility as sv;
        let cloud = sv::checker_cloud(Handle::default());
        let image = sv::checker_image();
        let up = Vec3::Y;
        let eye = Vec3::new(1_000.0, 500.0, 1_000.0);
        let mut flat = cloud_block(Some(&cloud), true);
        flat.mode.y = cloud_mean_k(&flat, texel_mean(&image), up.y, eye.y);
        assert_eq!(flat.mode.y, 0.5, "the checker's mean K");
        // The lattice aliases the checker: every point in a clear cell, or every one in an opaque one.
        let u = AirGpuUniform::default().with_clouds(flat);
        let cell = |uv: Vec2| {
            let c = (uv * 8.0).floor();
            if (c.x + c.y).rem_euclid(2.0) < 1.0 { 1.0 } else { 0.0 }
        };
        let sampled: f32 = mean_points(eye).iter().map(|p| clouds_mirror(&u, cell, *p, up)).sum::<f32>() / 1024.0;
        assert!(sampled == 1.0 || sampled == 0.0, "the lattice sees one colour: {sampled}");
        let packed = sv::SunVisibilityUniform::pack(&sv::SunVisibility { cloud: Some(cloud), ..default() });
        let depth = |_: IVec3| 0.7f32;
        let at = |presence: [f32; 2], only: bool| {
            let window = test_window(eye, presence);
            let v = air_light_volume(&sv::with_cloud_volume(packed, sv::CloudSourceInUse::Volume { near: true, far: true, only }, 0.6, window)).expect("the volume");
            mean_mirror(&v, &flat, eye, up, &depth, &depth)
        };
        assert_eq!(at([0.0, 0.0], false), 0.5, "no coverage: the image's mean, exactly");
        let full = 1.0 - 0.6 * (1.0 - (-0.7f32).exp());
        assert!((at([1.0, 1.0], false) - full).abs() < 1e-5, "full coverage: the volume's mean, not the image's error");
        let mut before = 0.5;
        for n in 1..=100 {
            let now = at([n as f32 / 100.0; 2], false);
            assert!((now - before).abs() < 0.01 && now >= before, "presence {n} %: {before} then {now}");
            before = now;
        }
        // The volume alone: nothing under it, K and its mean are 1 where it has no data.
        assert_eq!(at([0.0, 0.0], true), 1.0);
        assert!((at([1.0, 1.0], true) - full).abs() < 1e-5);
        // A light under the band's foot: the CPU's mean is 1 there and stays.
        let low = Vec3::new((1.0f32 - 0.01 * 0.01).sqrt(), 0.01, 0.0);
        let v = air_light_volume(&sv::with_cloud_volume(packed, sv::CloudSourceInUse::Volume { near: true, far: true, only: false }, 0.6, test_window(eye, [1.0, 1.0]))).expect("the volume");
        let mut at_low = flat;
        at_low.mode.y = cloud_mean_k(&flat, Some(0.5), low.y, eye.y);
        assert_eq!(mean_mirror(&v, &at_low, eye, low, &depth, &depth), 1.0);
    }

    /// The fixture of the two tests below: the light volume at `strength` with the levels' `presence`
    /// about `eye`, and the image's block (the checker's) with the mean `weather`.
    fn mean_fixture(strength: f32, presence: [f32; 2], eye: Vec3, weather: f32) -> (AirLightVolume, AirCloudBlock) {
        use crate::sun_visibility as sv;
        let cloud = sv::checker_cloud(Handle::default());
        let mut flat = cloud_block(Some(&cloud), true);
        flat.mode.y = weather;
        let packed = sv::SunVisibilityUniform::pack(&sv::SunVisibility { cloud: Some(cloud), ..default() });
        let used = sv::CloudSourceInUse::Volume { near: presence[0] > 0.0, far: presence[1] > 0.0, only: false };
        (air_light_volume(&sv::with_cloud_volume(packed, used, strength, test_window(eye, presence))).expect("the volume"), flat)
    }

    /// A dark mean is not lost against a bright fallback. Full coverage, the image's
    /// mean 0.9, strength 1, depth 15 everywhere: the mean is above 0, so the composite's reference is
    /// its least (0.25), not the 1 it takes for no mean; so too where K itself rounds to 0. No coverage
    /// stays the image's mean, bit for bit.
    #[test]
    fn a_dark_mean_is_not_lost_against_a_bright_fallback() {
        let eye = Vec3::new(1_000.0, 500.0, 1_000.0);
        let (full, flat) = mean_fixture(1.0, [1.0, 1.0], eye, 0.9);
        for (depth, most) in [(15.0f32, 1e-6f32), (40.0, 1e-6), (6.0, 0.0026)] {
            let field = move |_: IVec3| depth;
            let mean = mean_mirror(&full, &flat, eye, Vec3::Y, &field, &field);
            assert!(mean > 0.0 && mean <= most, "depth {depth}: {mean}");
            assert_eq!(medium_air::reference_visibility(None, mean), 0.25, "depth {depth}");
        }
        for weather in [0.9f32, 0.123_456_7, 0.0] {
            let (none, flat) = mean_fixture(1.0, [0.0, 0.0], eye, weather);
            assert_eq!(mean_mirror(&none, &flat, eye, Vec3::Y, &|_| 15.0, &|_| 15.0), weather, "no coverage");
        }
    }

    /// From the weather plane up the image shades nothing (`air_cloud_weather` is 1),
    /// so the fallback's mean there is 1: clear volume air above it keeps the reference 1 at every
    /// coverage, where the image's mean 0.5 gave 0.75 at half.
    #[test]
    fn above_the_weather_plane_the_fallback_is_clear() {
        let plane = crate::sun_visibility::CHECKER_PLANE;
        for (height, clear) in [(plane - 1.0, false), (plane, true), (plane + 15_000.0, true)] {
            let eye = Vec3::new(1_000.0, height, 1_000.0);
            for presence in [0.0f32, 0.25, 0.5, 1.0] {
                let (v, flat) = mean_fixture(0.6, [0.0, presence], eye, 0.5);
                assert_eq!(flat.cloud.x, plane);
                // The grid's own lookup at the same height.
                let u = AirGpuUniform::default().with_clouds(flat);
                assert_eq!(clouds_mirror(&u, |_| 0.0, eye, Vec3::Y) == 1.0, clear);
                let mean = mean_mirror(&v, &flat, eye, Vec3::Y, &|_| 0.0, &|_| 0.0);
                let want = if clear { 1.0 } else { 0.5 + 0.5 * presence };
                assert!((mean - want).abs() < 1e-5, "height {height}, presence {presence}: {mean}");
            }
        }
    }

    /// The handoff from the image to the volume at zero coverage steps nothing, with the
    /// camera below, at and above the weather plane: the flat path's uniform (the CPU's mean at the
    /// camera's height) and the mean pass's result are the same number, bit for bit.
    #[test]
    fn the_handoff_at_zero_coverage_steps_nothing_at_any_height() {
        let plane = crate::sun_visibility::CHECKER_PLANE;
        for (height, want) in [(500.0, 0.5f32), (plane - 1.0, 0.5), (plane, 1.0), (plane + 1.0, 1.0), (plane + 15_000.0, 1.0)] {
            let eye = Vec3::new(1_000.0, height, 1_000.0);
            let (v, mut flat) = mean_fixture(0.6, [0.0, 0.0], eye, 0.0);
            flat.mode.y = cloud_mean_k(&flat, texel_mean(&crate::sun_visibility::checker_image()), 1.0, height);
            assert_eq!(flat.mode.y, want, "the flat path at {height}");
            assert_eq!(mean_mirror(&v, &flat, eye, Vec3::Y, &|_| 0.7, &|_| 0.7), flat.mode.y, "the handoff at {height}");
        }
        // The shader's check is of the same number as the CPU's (the view's translation, the block's plane).
        assert!(GRID.contains("let origin = air.world_from_view[3].xyz;") && GRID.contains("origin.y >= air.cloud.x"));
    }

    /// The deck's transmittance steps nothing at the handoff from the flat layers to the volume, in
    /// itself or in the share, whatever the two strengths (the image's is 1 here): with no coverage the
    /// mean pass answers the CPU's numbers bit for bit, a thousandth of coverage moves the deck by at
    /// most a thousandth, and between them it goes from the one to the other.
    #[test]
    fn the_decks_handoff_steps_nothing() {
        use crate::medium_air::deck_share;
        let eye = Vec3::new(1_000.0, 500.0, 1_000.0);
        let depth = |_: IVec3| 0.3f32;
        for strength in [0.3f32, 0.6, 1.0] {
            for t_flat in [0.0f32, 0.35, 0.7, 1.0] {
                let at = |presence: f32| {
                    let (v, mut flat) = mean_fixture(strength, [0.0, presence], eye, 0.7);
                    flat.mode.z = t_flat;
                    mean_mirror_all(&v, &flat, eye, Vec3::Y, &depth, &depth)
                };
                assert_eq!(at(0.0), Vec3::new(0.7, t_flat, 0.0));
                let sliver = at(0.001);
                assert!((sliver.y - t_flat).abs() < 1.1e-3 && (sliver.x - 0.7).abs() < 1.1e-3, "{strength} {t_flat}: {sliver}");
                // The smoothstep's slope: 0.75 x 1.5 / 0.45 = 2.5.
                assert!((deck_share(sliver.y, false) - deck_share(t_flat, false)).abs() < 3e-3);
                let mut before = at(0.0).y;
                for n in 1..=50 {
                    let now = at(n as f32 / 50.0).y;
                    assert!((now - before).abs() < 0.021, "{strength} {t_flat} {n}: {before} then {now}");
                    before = now;
                }
                assert!((before - (-0.3f32).exp()).abs() < 1e-4, "the volume's own, free of both strengths: {before}");
            }
        }
    }

    /// One deck through the day: a uniform optical depth of 2 straight up, its ways 1 / up as long, is
    /// a closed deck (D at its least) at every light height, at any strength, with the flat layers
    /// describing the same deck where the light is so low that the ways leave the volume's window; a
    /// thin sheet keeps its vertical transmittance while the window holds the ways.
    #[test]
    fn one_deck_is_one_share_at_every_light_height() {
        use crate::medium_air::{deck_share, AIR_DECK_LEAST};
        let eye = Vec3::new(1_000.0, 500.0, 1_000.0);
        for strength in [0.3f32, 0.6, 1.0] {
            let at = |tau: f32, up: f32| {
                let (v, mut flat) = mean_fixture(strength, [1.0, 1.0], eye, 0.9);
                flat.mode.z = (-tau).exp();
                let slanted = move |_: IVec3| tau / up;
                mean_mirror_all(&v, &flat, eye, Vec3::new((1.0 - up * up).sqrt(), up, 0.0), &slanted, &slanted)
            };
            for up in [0.01f32, 0.03, 0.06, 0.1, 0.2, 0.306, 0.525, 0.9, 1.0] {
                let deck = at(2.0, up).y;
                assert!((deck_share(deck, false) - AIR_DECK_LEAST).abs() < 1e-5, "{strength} {up}: {deck}");
            }
            // (Between the band's foot and the least height the power is held, and part of the ways
            // are in the window: 0.06 would read thinner clouds as thicker, 0.82 for 0.86.)
            for up in [0.01f32, 0.03, 0.1, 0.2, 0.306, 0.525, 0.9, 1.0] {
                let thin = at(0.15, up).y;
                assert!((thin - (-0.15f32).exp()).abs() < 1e-3, "{strength} {up}: {thin}");
            }
            assert!((at(2.0, 0.525).z - 1.0).abs() < 1e-5, "whole coverage at 17:30's height");
        }
    }

    /// The flat layers' number: theirs, a 0 as 0; 1 with none, no number, or from their plane up, with
    /// no image needed for any of it.
    #[test]
    fn the_flat_deck_is_always_a_number() {
        let deck = |transmittance: f32| Some(crate::cloud_shadows::FlatDeck { transmittance, plane_height: 9_000.0 });
        assert_eq!(flat_deck(deck(0.0), 100.0), 0.0);
        assert_eq!(flat_deck(deck(0.35), 8_999.0), 0.35);
        assert_eq!(flat_deck(deck(0.35), 9_000.0), 1.0);
        assert_eq!(flat_deck(None, 100.0), 1.0);
        for bad in [f32::NAN, f32::INFINITY] {
            assert_eq!(flat_deck(deck(bad), 100.0), 1.0);
        }
        assert_eq!(flat_deck(deck(1.5), 100.0), 1.0);
    }

    /// The mean pass runs (and has to be ready) only where the composite reads the clouds' mean:
    /// the light's views, unless both `REF` and `SHARE` are fixed, as the shader takes them.
    #[test]
    fn the_mean_is_wanted_for_the_automatic_reference_and_the_deck() {
        for view in [AirView::None, AirView::Visibility, AirView::Slice, AirView::Share, AirView::Delta, AirView::Off, AirView::Lit, AirView::Rays] {
            let shows = matches!(view, AirView::None | AirView::Delta | AirView::Rays);
            for (reference, share, wanted) in [(None, None, true), (Some(0.6), None, true), (None, Some(0.4), true), (Some(0.6), Some(0.4), false)] {
                let tune = AirTune { view, reference, share, ..AirTune::default() };
                assert_eq!(wants_cloud_mean(&tune), shows && wanted, "{view:?} REF {reference:?} SHARE {share:?}");
            }
        }
        // The composite reads the reference and the deck's share in the light's branch alone, and the
        // uniform's fixed REF first.
        let compose = COMPOSE.replace("\r\n", "\n");
        assert_eq!(compose.matches("air_reference()").count(), 2, "the function and its one call");
        let light = compose.find("if code == VIEW_NONE || code == VIEW_DELTA || code == VIEW_RAYS {").expect("the light's branch");
        let call = compose.find("px.v_ref = air_reference();").expect("the call");
        let deck = compose.find("px.scale = air.phase2.z * air_deck_share();").expect("the deck's share's call");
        let views = compose.find("// The debug views read sample 0.").expect("the diagnostic views");
        assert!(light < call && call < views && light < deck && deck < views);
        assert_eq!((compose.matches("air.cloud_mode.y").count(), compose.matches("air.cloud_mode.z").count(), compose.matches("air.cloud_mode.w").count()), (1, 1, 0));
        assert!(compose.contains("if air.rays2.y > 0.0 {\n        return clamp(air.rays2.y, 0.05, 1.0);"));
    }

    /// The mean pass's result lands on `cloud_mode.yz` and on nothing else (its x, whether the image
    /// is in use, the grid reads): the byte offsets of those in the uniform as it is written to the GPU.
    #[test]
    fn the_mean_is_copied_over_the_reference_visibility() {
        use bevy::render::render_resource::encase;
        let bytes = |u: &AirGpuUniform| {
            let mut buffer = encase::UniformBuffer::new(Vec::<u8>::new());
            buffer.write(u).expect("the uniform's bytes");
            buffer.into_inner()
        };
        let plain = bytes(&AirGpuUniform::default());
        let marked = bytes(&AirGpuUniform { cloud_mode: Vec4::new(0.0, 0.625, 0.375, 0.0), ..default() });
        let at = CLOUD_MEAN_OFFSET as usize;
        // What the copy takes from the mean pass's buffer, the vec4's first two numbers.
        let copied: Vec<u8> = [0.625f32, 0.375].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(copied.len() as u64, CLOUD_MEAN_COPY);
        let mut patched = plain;
        patched[at..at + copied.len()].copy_from_slice(&copied);
        assert_eq!(patched, marked);
        assert!(CLOUD_MEAN_OFFSET.is_multiple_of(4) && CLOUD_MEAN_COPY.is_multiple_of(4) && CLOUD_MEAN_COPY <= CLOUD_MEAN_BYTES);
        assert!(COMPOSE.contains("let m = air.cloud_mode.y;"), "the composite's reference");
        assert!(COMPOSE.contains("clamp((air.cloud_mode.z - AIR_DECK_FROM)"), "the deck's share");
    }

    /// The uniform's fields, in order, as both shaders declare `AirUniform`, and its size.
    #[test]
    fn both_shaders_declare_the_uniform_as_the_cpu_side_has_it() {
        // 16 vec4s both shaders know, and rays2 at the end, the composite's alone.
        assert_eq!(AirGpuUniform::min_size().get(), 64 + 17 * 16);
        let expected = [
            "world_from_view: mat4x4<f32>,",
            "proj: vec4<f32>,",
            "grid: vec4<f32>,",
            "curve: vec4<f32>,",
            "fog: vec4<f32>,",
            "phase: vec4<f32>,",
            "phase2: vec4<f32>,",
            "light: vec4<f32>,",
            "fog_colour: vec4<f32>,",
            "misc: vec4<f32>,",
            "viewport: vec4<f32>,",
            "terrain_uv: vec4<f32>,",
            "terrain: vec4<f32>,",
            "terrain_light: vec4<f32>,",
            "cloud_uv: vec4<f32>,",
            "cloud: vec4<f32>,",
            "cloud_mode: vec4<f32>,",
        ];
        let fields = |source: &str| -> Vec<String> {
            let source = source.replace("\r\n", "\n");
            block(&source, "struct AirUniform {").lines().skip(1).map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//") && *l != "}").map(String::from).collect()
        };
        assert_eq!(fields(GRID), expected);
        // The composite declares every field and then rays2, the last (its own; the grid shader's
        // shorter struct reads the same buffer).
        let compose = fields(COMPOSE);
        let mut with_rays: Vec<&str> = expected.to_vec();
        with_rays.push("rays2: vec4<f32>,");
        assert_eq!(compose, with_rays);
        // The grid layout's order (grid_layout).
        for binding in [
            "@group(0) @binding(0) var<uniform> air: AirUniform;",
            "@group(0) @binding(1) var volume: texture_storage_3d<rgba16float, write>;",
            "@group(0) @binding(2) var<uniform> lights: Lights;",
            "@group(0) @binding(3) var shadow_maps: texture_depth_2d_array;",
            "@group(0) @binding(4) var shadow_sampler: sampler_comparison;",
            "@group(0) @binding(5) var terrain_map: texture_2d<f32>;",
            "@group(0) @binding(6) var cloud_map: texture_2d<f32>;",
            "@group(0) @binding(7) var cloud_sampler: sampler;",
        ] {
            assert!(GRID.contains(binding), "{binding}");
        }
        let entry = "@compute @workgroup_size(8, 8, AIR_CHUNKS)\nfn air_grid(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>, @builtin(local_invocation_index) li: u32) {";
        assert!(GRID.replace("\r\n", "\n").contains(entry) && (WORKGROUP, CHUNKS) == (8, 4));
        // The import the shader takes Bevy's lights from (the module path bevy_pbr loads it at).
        assert!(GRID.contains("import bevy_pbr::render::mesh_view_types::{Lights, DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT, DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT};"));
        // The loop starts from a share of 0 (medium_air.rs column_sum) and keeps dF >= 0.
        assert!(GRID.contains("    var before = 0.0;\n") || GRID.contains("    var before = 0.0;\r\n"));
        assert!(GRID.contains("let df = max(after - before, 0.0);"));
    }

    /// CPU mirror of medium_air_grid.wesl `air_cascade_index` and `air_cascades`' blend (Bevy's
    /// rule, shadows.wesl `get_cascade_index` and `fetch_directional_shadow`): the cascade for
    /// view depth `z` and, within the overlap share of its far end, the next cascade with its
    /// mix weight; `None` beyond the last cascade (lit).
    fn cascade_choice(bounds: &[f32], overlap: f32, z: f32) -> Option<(usize, Option<(usize, f32)>)> {
        let i = bounds.iter().position(|&far| z < far)?;
        let next = (i + 1 < bounds.len()).then(|| (1.0 - overlap) * bounds[i]).filter(|&near| z >= near).map(|near| (i + 1, (z - near) / (bounds[i] - near)));
        Some((i, next))
    }

    /// The cascade choice at the Quality layout's day bounds (700/2,000/5,000/8,000) with Bevy's default overlap of 0.2.
    #[test]
    fn the_cascade_is_chosen_as_bevy_chooses_it() {
        let bounds = [700.0, 2_000.0, 5_000.0, 8_000.0];
        assert_eq!(cascade_choice(&bounds, 0.2, 0.0), Some((0, None)));
        assert_eq!(cascade_choice(&bounds, 0.2, 559.0), Some((0, None)));
        let (i, next) = cascade_choice(&bounds, 0.2, 630.0).expect("inside");
        assert_eq!(i, 0);
        let (j, w) = next.expect("in the overlap");
        assert!(j == 1 && (w - 0.5).abs() < 1e-5, "{w}");
        // At a far bound the next cascade is chosen alone (its own overlap starts later).
        assert_eq!(cascade_choice(&bounds, 0.2, 700.0), Some((1, None)));
        assert_eq!(cascade_choice(&bounds, 0.2, 7_999.0), Some((3, None)), "the last cascade blends with nothing");
        assert_eq!(cascade_choice(&bounds, 0.2, 8_000.0), None, "lit beyond the last cascade");
        assert_eq!(cascade_choice(&[], 0.2, 10.0), None, "no cascades: lit");
        assert_eq!(cascade_choice(&bounds, 0.0, 699.0), Some((0, None)), "no overlap: no blend");
        // The shader's text as the mirror has it.
        let grid = GRID.replace("\r\n", "\n");
        assert!(grid.contains("if z < lights.directional_lights[light].cascades[i].far_bound {"));
        assert!(grid.contains("let next_near = (1.0 - lights.directional_lights[light].cascades_overlap_proportion) * far;"));
        assert!(grid.contains("c = mix(c, air_cascade_lit(light, i + 1u, world), (z - next_near) / (far - next_near));"));
        assert!(grid.contains("let offset = world + lights.directional_lights[light].shadow_depth_bias * lights.directional_lights[light].direction_to_light;"));
        assert!(grid.contains("let uv = ndc.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);"));
        // Never index 0 or a direction match: the flagged light with shadows on.
        assert!(grid.contains("let wanted = DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT | DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT;"));
    }

    /// The terrain block: valid only with the image on the GPU, a side and a texel, and the
    /// map's light within TERRAIN_SAME_LIGHT of the current one.
    #[test]
    fn the_terrain_map_counts_only_for_its_own_light() {
        let light = Vec3::new(0.3, 0.5, -0.8).normalize();
        let publication = AirTerrainPublication { uv_scale: Vec2::new(1.0 / 65_536.0, -1.0 / 65_536.0), uv_offset: Vec2::new(0.5, 0.5), texel: 256.0, side: 256, light_dir: light, on_gpu: true };
        let block = terrain_block(Some(&publication), light);
        assert_eq!(block.terrain.x, 1.0);
        assert_eq!(block.uv, Vec4::new(1.0 / 65_536.0, -1.0 / 65_536.0, 0.5, 0.5));
        assert_eq!((block.terrain.z, block.terrain.w), (256.0, 256.0));
        assert_eq!(block.light, light.extend(0.0));
        assert_eq!(terrain_block(None, light).terrain.x, 0.0);
        assert_eq!(terrain_block(Some(&AirTerrainPublication { on_gpu: false, ..publication }), light).terrain.x, 0.0);
        assert_eq!(terrain_block(Some(&AirTerrainPublication { side: 0, ..publication }), light).terrain.x, 0.0);
        assert_eq!(terrain_block(Some(&AirTerrainPublication { texel: 0.0, ..publication }), light).terrain.x, 0.0);
        assert_eq!(terrain_block(Some(&AirTerrainPublication { uv_scale: Vec2::NAN, ..publication }), light).terrain.x, 0.0);
        // The moon's light (the sun's reversed) and a light 5 degrees on: another map's.
        assert_eq!(terrain_block(Some(&publication), -light).terrain.x, 0.0);
        let turned = Quat::from_rotation_y(5f32.to_radians()) * light;
        assert_eq!(terrain_block(Some(&publication), turned).terrain.x, 0.0);
        let nudged = Quat::from_rotation_y(1f32.to_radians()) * light;
        assert_eq!(terrain_block(Some(&publication), nudged).terrain.x, 1.0, "a degree on: still this map");
        assert_eq!(terrain_block(Some(&publication), Vec3::ZERO).terrain.x, 0.0, "no light: T 1");
        let u = AirGpuUniform::default().with_terrain(block);
        assert_eq!((u.terrain_uv, u.terrain, u.terrain_light), (block.uv, block.terrain, block.light));
    }

    /// CPU mirror of medium_air_grid.wesl `air_terrain_lit`.
    fn terrain_lit_mirror(s: Vec4, y: f32, extra: f32, texel: f32) -> f32 {
        let band = (s.y * 0.01).max(0.25 * texel) + extra;
        let t = ((y - s.x) / band + 1.0).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// CPU mirror of medium_air_grid.wesl `air_terrain`, line for line.
    fn terrain_mirror(u: &AirGpuUniform, load: impl Fn(IVec2) -> Vec4, p: Vec3, l: Vec3) -> f32 {
        if u.terrain.x < 0.5 {
            return 1.0;
        }
        let (texel, side) = (u.terrain.z, u.terrain.w);
        let x = (Vec2::new(p.x, p.z) * Vec2::new(u.terrain_uv.x, u.terrain_uv.y) + Vec2::new(u.terrain_uv.z, u.terrain_uv.w)) * side;
        let inside = x.x.min(side - x.x).min(x.y.min(side - x.y));
        let e = (inside / 4.0).clamp(0.0, 1.0);
        let edge = e * e * (3.0 - 2.0 * e);
        if edge <= 0.0 {
            return 1.0;
        }
        let c = x - Vec2::splat(0.5);
        let base = c.floor();
        let f = c - base;
        let last = IVec2::splat(side as i32 - 1);
        let i0 = base.as_ivec2().clamp(IVec2::ZERO, last);
        let i1 = (base.as_ivec2() + IVec2::ONE).clamp(IVec2::ZERO, last);
        let (t00, t10, t01, t11) = (load(i0), load(IVec2::new(i1.x, i0.y)), load(IVec2::new(i0.x, i1.y)), load(i1));
        let (w00, w10, w01, w11) = ((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
        let relief = w00 * t00.z + w10 * t10.z + w01 * t01.z + w11 * t11.z;
        let y = p.y + 16.0 + 2.0 * relief;
        let tan_e = (l.y / Vec2::new(l.x, l.z).length().max(1.0e-4)).clamp(0.0, 1.0);
        let extra = 0.5 * texel * tan_e;
        let lit = w00 * terrain_lit_mirror(t00, y, extra, texel) + w10 * terrain_lit_mirror(t10, y, extra, texel) + w01 * terrain_lit_mirror(t01, y, extra, texel) + w11 * terrain_lit_mirror(t11, y, extra, texel);
        1.0 + (lit - 1.0) * edge
    }

    /// The air reads the terrain map as the surfaces do: the shader's constants are
    /// sun_visibility's, and the mirror of the air's receiver equals sun_visibility.rs
    /// `terrain_visibility` (the surface receiver's own mirror) on a map produced as the
    /// surface tests produce theirs (terrain_shadows.rs `march_params`, `produce_output`): a
    /// ridge 3,000 units high across a flat map, the sun 10 degrees up in the south.
    #[test]
    fn the_air_reads_the_terrain_map_as_the_surfaces_do() {
        use crate::sun_visibility as sv;
        let grid = GRID.replace("\r\n", "\n");
        for (name, value) in [
            ("PENUMBRA", sv::TERRAIN_PENUMBRA),
            ("SOFT_MIN", sv::TERRAIN_SOFT_MIN),
            ("SOFT_TEXELS", sv::TERRAIN_SOFT_TEXELS),
            ("TAN_MAX", sv::TERRAIN_TAN_MAX),
            ("BIAS_RELIEF", sv::TERRAIN_BIAS_RELIEF),
            ("BIAS_MIN", sv::TERRAIN_BIAS_MIN),
            ("EDGE_TEXELS", sv::TERRAIN_EDGE_TEXELS),
        ] {
            assert!(grid.contains(&format!("const AIR_TERRAIN_{name}: f32 = {value:?};")), "AIR_TERRAIN_{name} = {value:?}");
        }
        const TEXEL: f32 = 256.0;
        let side = 32usize;
        let heights: Vec<f32> = (0..side * side).map(|i| if i / side == 12 { 3_000.0 } else { 0.0 }).collect();
        let valid = vec![1u8; side * side];
        let elevation = 10f32.to_radians();
        // Game axes (x east, y north, z up), towards the light: the sun in the south.
        let toward_game = Vec3::new(0.0, -elevation.cos(), elevation.sin());
        let params = crate::terrain_shadows::march_params(toward_game, TEXEL, side as u32, 0.0, 3_000.0).expect("a light above the horizon");
        let out = crate::terrain_shadows::produce_output(&heights, &valid, &params);
        let (uv_scale, uv_offset) = crate::terrain_shadows::TerrainShadows::uv_transform(Vec2::ZERO, TEXEL, side as u32);
        let bevy = |g: Vec3| Vec3::new(g.x, g.z, -g.y);
        let l = bevy(toward_game).normalize();
        let publication = AirTerrainPublication { uv_scale, uv_offset, texel: TEXEL, side: side as u32, light_dir: l, on_gpu: true };
        let u = AirGpuUniform::default().with_terrain(terrain_block(Some(&publication), l));
        assert_eq!(u.terrain.x, 1.0);
        let surface = sv::SunVisibilityUniform { terrain_uv: u.terrain_uv, terrain: Vec4::new(0.0, 0.0, TEXEL, side as f32), mode: UVec4::new(sv::MASK_TERRAIN, 0, 0, 0), ..default() };
        let load = |c: IVec2| {
            assert!(c.x >= 0 && c.y >= 0 && (c.x as usize) < side && (c.y as usize) < side, "a load outside the map: {c}");
            out[c.y as usize * side + c.x as usize]
        };
        let (mut dark, mut lit) = (0, 0);
        for gx in (0..40).map(|i| i as f32 * 0.83 - 2.0) {
            for gy in (0..40).map(|i| i as f32 * 0.81 - 2.0) {
                for z in [0.0f32, 400.0, 1_500.0, 2_900.0, 3_500.0] {
                    let p = bevy(Vec3::new(gx * TEXEL, gy * TEXEL, z));
                    let air = terrain_mirror(&u, load, p, l);
                    let surf = sv::terrain_visibility(&surface, load, p, l);
                    assert!((air - surf).abs() < 1e-6, "T {air} against the surfaces' {surf} at texel ({gx}, {gy}), height {z}");
                    dark += usize::from(air < 0.1);
                    lit += usize::from(air > 0.9);
                }
            }
        }
        assert!(dark > 500 && lit > 500, "the fixture shades and lights: {dark} dark, {lit} lit");
        // Not valid: 1 before any read.
        let off = AirGpuUniform::default();
        assert_eq!(terrain_mirror(&off, |_| panic!("no read"), Vec3::ZERO, l), 1.0);
    }

    /// CPU mirror of medium_air_grid.wesl `air_cloud_weather`, line for line, with `sample` giving
    /// the image's R at a uv (the repeating linear sampler's part).
    fn clouds_mirror(u: &AirGpuUniform, sample: impl Fn(Vec2) -> f32, p: Vec3, l: Vec3) -> f32 {
        if u.cloud_mode.x < 0.5 {
            return 1.0;
        }
        let height = u.cloud.x;
        let up = l.y;
        if up <= u.cloud.z || p.y >= height {
            return 1.0;
        }
        let hit = Vec2::new(p.x, p.z) + Vec2::new(l.x, l.z) * ((height - p.y) / up);
        let uv = hit * Vec2::new(u.cloud_uv.x, u.cloud_uv.y) + Vec2::new(u.cloud_uv.z, u.cloud_uv.w);
        let k = (1.0 - u.cloud.y * (1.0 - sample(uv))).clamp(0.0, 1.0);
        let t = ((up - u.cloud.z) / (u.cloud.w - u.cloud.z)).clamp(0.0, 1.0);
        let fade = t * t * (3.0 - 2.0 * t);
        1.0 + (k - 1.0) * fade
    }

    /// The air reads the Cloud shadows image as the surfaces do: the block packs exactly as
    /// sun_visibility.rs packs the surfaces' uniform (its own test's values: strength 1.5 held
    /// to 1), the mirror's uv is cloud_shadows.rs `projected_uv` (the shader contract's
    /// reference, on that test's values), and the shader's lines are sun_visibility.wesl's.
    #[test]
    fn the_air_reads_the_cloud_shadow_as_the_surfaces_do() {
        use crate::sun_visibility as sv;
        let cloud = sv::CloudVisibility { image: Handle::default(), uv_scale: Vec2::new(0.5, 0.25), uv_offset: Vec2::new(3.0, 4.0), plane_height: 9_000.0, strength: 1.5 };
        let packed = cloud_block(Some(&cloud), true);
        let surface = sv::SunVisibilityUniform::pack(&sv::SunVisibility { cloud: Some(cloud.clone()), ..default() });
        assert_eq!((packed.uv, packed.cloud, packed.mode.x), (surface.cloud_uv, surface.cloud, 1.0));
        // Not published, not on the GPU, not finite: K is 1 before any read.
        assert_eq!(cloud_block(None, true), AirCloudBlock::default());
        assert_eq!(cloud_block(Some(&cloud), false), AirCloudBlock::default());
        assert_eq!(cloud_block(Some(&sv::CloudVisibility { plane_height: f32::NAN, ..cloud.clone() }), true), AirCloudBlock::default());
        let off = AirGpuUniform::default().with_clouds(AirCloudBlock::default());
        assert_eq!(clouds_mirror(&off, |_| panic!("no read"), Vec3::ZERO, Vec3::Y), 1.0);
        // cloud_shadows.rs `cloud_projection_accounts_for_height`'s values.
        let (scale, offset, plane) = (Vec2::splat(1.0 / crate::cloud_shadows::REPEAT_UNITS), Vec2::new(0.25, 0.5), 20_000.0);
        let to_light = Vec3::new(0.5, 0.8, -0.2).normalize();
        let shadow = sv::CloudVisibility { image: Handle::default(), uv_scale: scale, uv_offset: offset, plane_height: plane, strength: crate::cloud_shadows::STRENGTH };
        let u = AirGpuUniform::default().with_clouds(cloud_block(Some(&shadow), true));
        for p in [Vec3::new(1_000.0, 0.0, -3_000.0), Vec3::new(-7_000.0, 1_500.0, 12_000.0), Vec3::new(30.0, 19_000.0, 40.0)] {
            let expected = crate::cloud_shadows::projected_uv(p, to_light, plane, scale, offset).expect("below the plane");
            let seen = std::cell::Cell::new(None);
            let k = clouds_mirror(&u, |uv| { seen.set(Some(uv)); 0.25 }, p, to_light);
            let uv = seen.get().expect("the image read");
            assert!((uv - expected).length() < 1e-5, "{uv} against {expected}");
            // Light high enough for the whole shadow: K = 1 - strength x (1 - texel).
            assert!((k - (1.0 - crate::cloud_shadows::STRENGTH * 0.75)).abs() < 1e-6, "{k}");
        }
        // At or above the plane, or the light at the horizon: lit without a read, as the
        // reference has none.
        for (p, l) in [(Vec3::new(10.0, plane, 20.0), to_light), (Vec3::Y * 25_000.0, to_light), (Vec3::new(1.0, 0.0, 2.0), Vec3::X)] {
            assert!(crate::cloud_shadows::projected_uv(p, l, plane, scale, offset).is_none());
            assert_eq!(clouds_mirror(&u, |_| panic!("no read"), p, l), 1.0);
        }
        // The fade between the sines: half way, half the shadow (smoothstep at 0.5).
        let mid = (sv::CLOUD_FADE_FROM + sv::CLOUD_FADE_TO) * 0.5;
        let low = Vec3::new((1.0 - mid * mid).sqrt(), mid, 0.0);
        let k = clouds_mirror(&u, |_| 0.0, Vec3::ZERO, low);
        assert!((k - (1.0 - 0.5 * crate::cloud_shadows::STRENGTH)).abs() < 1e-5, "{k}");
        // The shader's lines are the surfaces' (sun_visibility.wesl), bar the names.
        let grid = GRID.replace("\r\n", "\n");
        let body = block(&grid, "fn air_cloud_weather(");
        for line in [
            "if up <= air.cloud.z || P.y >= height {",
            "let hit = P.xz + L.xz * ((height - P.y) / up);",
            "let uv = hit * air.cloud_uv.xy + air.cloud_uv.zw;",
            "let transmittance = textureSampleLevel(cloud_map, cloud_sampler, uv, 0.0).r;",
            "let k = saturate(1.0 - air.cloud.y * (1.0 - transmittance));",
            "return mix(1.0, k, smoothstep(air.cloud.z, air.cloud.w, up));",
        ] {
            assert!(body.contains(line), "{line}");
        }
        let surfaces = include_str!("sun_visibility.wesl").replace("\r\n", "\n");
        // The weather layers' receiver (since #3032 stage 2i M2 `nvf_cloud_weather`, behind the
        // dispatcher `nvf_cloud_transmittance` that may read the light volume instead, as the air's
        // `air_clouds` does: `the_air_reads_the_light_volume_as_the_surfaces_do`).
        let theirs = block(&surfaces, "fn nvf_cloud_weather(P: vec3<f32>, L: vec3<f32>) -> f32 {");
        for line in [
            "if up <= nvf_sun.cloud.z || P.y >= height {",
            "let hit = P.xz + L.xz * ((height - P.y) / up);",
            "let uv = hit * nvf_sun.cloud_uv.xy + nvf_sun.cloud_uv.zw;",
            "let k = saturate(1.0 - nvf_sun.cloud.y * (1.0 - transmittance));",
            "return mix(1.0, k, smoothstep(nvf_sun.cloud.z, nvf_sun.cloud.w, up));",
        ] {
            assert!(theirs.contains(line), "the surfaces' receiver changed: {line}");
        }
        // The occluders' codes: all multiplies K in, 4 is the clouds alone.
        assert!(grid.contains("if code == 0u || code == 4u {") && grid.contains("return k * min(c, t);"));
    }

    /// CPU mirror of medium_air_grid.wesl `air_ign` (the shader's literals rounded to the f32
    /// each one parses to: 52.9829189 is 52.982918 in f32, the others are exact as written).
    fn ign_mirror(c: Vec2) -> f32 {
        let f = |x: f32| x - x.floor();
        f(52.982_918 * f(c.dot(Vec2::new(0.067_110_56, 0.005_837_15))))
    }

    /// CPU mirror of medium_air_grid.wesl `air_sample_depth`.
    fn sample_depth_mirror(s: u32, count: u32, o: f32) -> f32 {
        let centred = count % 2 == 1 && s == count - 1;
        let sign = if s & 1 == 1 { -1.0 } else { 1.0 };
        let d = if centred { 0.0 } else { (o - 0.5) * sign };
        (s as f32 + 0.5 + d) / count as f32
    }

    /// The R2 sequence's two steps, 1 / g and 1 / g^2 for the plastic number g (Roberts 2018),
    /// as the shader's `AIR_R2` literals (0.7548776662, 0.5698402910) parse to f32.
    const R2_MIRROR: Vec2 = Vec2::new(0.754_877_7, 0.569_840_3);

    /// CPU mirror of medium_air_grid.wesl `air_sample_lateral`.
    fn sample_lateral_mirror(s: u32, count: u32, shift: Vec2) -> Vec2 {
        if count <= 1 {
            return Vec2::ZERO;
        }
        let r = shift + R2_MIRROR * (s as f32 + 0.5);
        r - r.floor() - Vec2::splat(0.5)
    }

    /// CPU mirror of medium_air_grid.wesl `air_stratum_end` (WGSL `mix(a, b, t)` is
    /// `a x (1 - t) + b x t`).
    fn stratum_end_mirror(s: u32, count: u32, z0: f32, z1: f32) -> f32 {
        if s + 1 >= count {
            return z1;
        }
        let t = (s + 1) as f32 / count as f32;
        z0 * (1.0 - t) + z1 * t
    }

    /// CPU mirror of medium_air_grid.wesl `air_sample_within`.
    fn sample_within_mirror(s: u32, count: u32, o: f32) -> f32 {
        let centred = count % 2 == 1 && s == count - 1;
        let sign = if s & 1 == 1 { -1.0 } else { 1.0 };
        0.5 + if centred { 0.0 } else { (o - 0.5) * sign }
    }

    /// WGSL `mix(a, b, t)`: `a x (1 - t) + b x t`.
    fn mix_mirror(a: f32, b: f32, t: f32) -> f32 {
        a * (1.0 - t) + b * t
    }

    /// One walk over some of a column's slices, as [`walk_mirror`] leaves it.
    struct Walk {
        /// Per slice the walk's own (N, M, b): the shader's `kept`.
        kept: Vec<[f32; 3]>,
        /// The walk's own (N, M) at its end: the shader's entry of `air_chunk_sums`.
        sums: Vec2,
        /// The visibility reads.
        reads: u32,
        /// The largest share so far at each slice's start.
        befores: Vec<f32>,
    }

    /// CPU mirror of medium_air_grid.wesl `air_grid` up to its barrier for the slices [k0, k1) of
    /// one column, line for line, with the column's depth dither `o0`: per slice k the walk's
    /// own (r N, g M, b mean V of the samples read). `visibility(k, s, z)` is sample s of slice
    /// k's V at view depth z (the shader's `air_visibility` at that sample's point), called only
    /// for a stratum the fog share changes in, with z inside the part of the stratum where it
    /// changes; the fog share is medium_air.rs `fog_share` (the shader's `air_fog_share`). Over
    /// 0..nz it is the serial variant's walk.
    fn walk_mirror(grid: &AirGrid, count: u32, ray_len: f32, fog: AirFog, o0: f32, (k0, k1): (u32, u32), visibility: impl Fn(u32, u32, f32) -> f32) -> Walk {
        let curve = grid.curve();
        let (mut n, mut m, mut before, mut z0) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        if k0 > 0 {
            z0 = curve.depth_at(k0 as f32);
            before = medium_air::fog_share(z0 * ray_len, fog).max(0.0);
        }
        let mut befores = Vec::new();
        let fog_lo = fog.near / ray_len;
        let fog_hi = (fog.near + (fog.far - fog.near).max(1.0)) / ray_len;
        let mut reads = 0u32;
        let kept = (k0..k1)
            .map(|k| {
                befores.push(before);
                let z1 = curve.depth_at(k as f32 + 1.0);
                let (mut v_sum, mut read, mut za) = (0.0f32, 0u32, z0);
                for s in 0..count {
                    let zb = stratum_end_mirror(s, count, z0, z1);
                    let after = medium_air::fog_share(zb * ray_len, fog);
                    let df = (after - before).max(0.0);
                    before = before.max(after);
                    if df > 0.0 {
                        let (lo, hi) = (za.max(fog_lo), zb.min(fog_hi));
                        let mut z = mix_mirror(z0, z1, sample_depth_mirror(s, count, o0));
                        if (lo > za || hi < zb) && lo < hi {
                            z = mix_mirror(lo, hi, sample_within_mirror(s, count, o0));
                        }
                        let v = visibility(k, s, z);
                        n += df * v;
                        v_sum += v;
                        read += 1;
                        reads += 1;
                    }
                    m += df;
                    za = zb;
                }
                z0 = z1;
                [n, m, if read > 0 { v_sum / read.max(1) as f32 } else { 1.0 }]
            })
            .collect();
        Walk { kept, sums: Vec2::new(n, m), reads, befores }
    }

    /// The shader's `AIR_CHUNKS`.
    fn chunks_of(way: GridWalk) -> u32 {
        if way == GridWalk::Serial { 1 } else { CHUNKS }
    }

    /// CPU mirror of medium_air_grid.wesl `air_grid`'s chunks: each one's slices [k0, k1).
    fn chunks_mirror(way: GridWalk, nz: u32) -> Vec<(u32, u32)> {
        let per = nz.div_ceil(chunks_of(way)).min(way.chunk_slices());
        (0..chunks_of(way)).map(|z| (z * per, (z * per + per).min(nz))).collect()
    }

    /// The chunks' walks ([`walk_mirror`]) of one column, as they stand at the barrier.
    fn walks_mirror(way: GridWalk, grid: &AirGrid, count: u32, ray_len: f32, fog: AirFog, o0: f32, visibility: impl Fn(u32, u32, f32) -> f32) -> Vec<Walk> {
        chunks_mirror(way, grid.nz).into_iter().map(|slices| walk_mirror(grid, count, ray_len, fog, o0, slices, &visibility)).collect()
    }

    /// CPU mirror of medium_air_grid.wesl `air_grid` after the barrier: per slice the stored
    /// (r N, g M, b mean V of the samples read). (The serial variant stores as it walks: its one
    /// walk with nothing added.)
    fn join_mirror(walks: &[Walk]) -> Vec<[f32; 3]> {
        let mut column = Vec::new();
        for (z, walk) in walks.iter().enumerate() {
            let mut carry = Vec2::ZERO;
            for earlier in &walks[..z] {
                carry += earlier.sums;
            }
            column.extend(walk.kept.iter().map(|own| [carry.x + own[0], carry.y + own[1], own[2]]));
        }
        column
    }

    /// CPU mirror of medium_air_grid.wesl `air_grid` for one column, by the chunks its grid takes:
    /// the stored texels ([`join_mirror`]) and the count of visibility reads.
    fn column_mirror_at(grid: &AirGrid, count: u32, ray_len: f32, fog: AirFog, o0: f32, visibility: impl Fn(u32, u32, f32) -> f32) -> (Vec<[f32; 3]>, u32) {
        let walks = walks_mirror(GridWalk::chunked(grid.nz), grid, count, ray_len, fog, o0, visibility);
        (join_mirror(&walks), walks.iter().map(|w| w.reads).sum())
    }

    /// A column as [`column_reference`] has it: per slice (N, M, b), the reads, and the largest
    /// share before each slice.
    struct Reference {
        column: Vec<[f32; 3]>,
        reads: u32,
        befores: Vec<f32>,
    }

    /// The column's sums as the header of medium_air_grid.wesl defines them, written apart from
    /// the mirrors (it calls none of them): the grid's strata as one list of edges, the fog's
    /// share at each, the largest share up to each, the steps over it, the taps, and then the
    /// sums added in one go from the camera.
    fn column_reference(grid: &AirGrid, count: u32, ray_len: f32, fog: AirFog, o0: f32, visibility: impl Fn(u32, u32, f32) -> f32) -> Reference {
        let curve = grid.curve();
        let strata = (grid.nz * count) as usize;
        let slice = |k: u32| if k == 0 { 0.0 } else { curve.depth_at(k as f32) };
        let edges: Vec<f32> = (0..=strata as u32)
            .map(|i| {
                let (k, s) = (i / count, i % count);
                let part = s as f32 / count as f32;
                if s > 0 { slice(k) * (1.0 - part) + slice(k + 1) * part } else { slice(k) }
            })
            .collect();
        let mut peaks = vec![0.0f32];
        let mut steps = Vec::new();
        for (i, edge) in edges.iter().enumerate().skip(1) {
            let share = medium_air::fog_share(edge * ray_len, fog);
            let peak = peaks[i - 1];
            steps.push(if share > peak { share - peak } else { 0.0 });
            peaks.push(if share > peak { share } else { peak });
        }
        // The fog's ramp along the ray, in view depth.
        let ramp = (fog.near / ray_len, (fog.near + (fog.far - fog.near).max(1.0)) / ray_len);
        let tap = |i: usize| {
            let (k, s) = (i as u32 / count, i as u32 % count);
            let off = match s {
                s if s + 1 == count && count % 2 == 1 => 0.0,
                s if s & 1 == 0 => o0 - 0.5,
                _ => 0.5 - o0,
            };
            let (near, far) = (edges[i], edges[i + 1]);
            let (lo, hi) = (if ramp.0 > near { ramp.0 } else { near }, if ramp.1 < far { ramp.1 } else { far });
            if (lo, hi) != (near, far) && lo < hi {
                return lo * (1.0 - (0.5 + off)) + hi * (0.5 + off);
            }
            let at = (s as f32 + 0.5 + off) / count as f32;
            slice(k) * (1.0 - at) + slice(k + 1) * at
        };
        let seen: Vec<Option<f32>> = (0..strata).map(|i| (steps[i] > 0.0).then(|| visibility(i as u32 / count, i as u32 % count, tap(i)))).collect();
        let (mut n, mut m) = (0.0f32, 0.0f32);
        let mut column = Vec::new();
        for k in 0..grid.nz as usize {
            let range = k * count as usize..(k + 1) * count as usize;
            for (step, v) in steps[range.clone()].iter().zip(&seen[range.clone()]) {
                n += v.map_or(0.0, |v| step * v);
                m += step;
            }
            let read: Vec<f32> = seen[range].iter().flatten().copied().collect();
            let mean = if read.is_empty() { 1.0 } else { read.iter().sum::<f32>() / read.len() as f32 };
            column.push([n, m, mean]);
        }
        Reference { column, reads: seen.iter().flatten().count() as u32, befores: (0..grid.nz).map(|k| peaks[(k * count) as usize]).collect() }
    }

    /// [`column_mirror_at`] with the dither at its middle and V by slice and sample.
    fn column_mirror(grid: &AirGrid, count: u32, ray_len: f32, fog: AirFog, visibility: impl Fn(u32, u32) -> f32) -> Vec<[f32; 3]> {
        column_mirror_at(grid, count, ray_len, fog, 0.5, |k, s, _| visibility(k, s)).0
    }

    /// The fogs the loop's tests run on: the base game's front-loaded curve, a fog starting
    /// behind the camera (a share at the camera itself), a narrow modded ramp, a linear one
    /// and a power below 1 with a weak strength.
    fn loop_fogs() -> [AirFog; 5] {
        [
            AirFog { near: 0.0, far: 200_000.0, power: 0.6, strength: 0.8 },
            AirFog { near: -10.0, far: 200_000.0, power: 0.6, strength: 1.0 },
            AirFog { near: 3_000.0, far: 3_400.0, power: 2.0, strength: 1.0 },
            AirFog { near: 500.0, far: 20_000.0, power: 1.0, strength: 0.5 },
            AirFog { near: 0.0, far: 9_000.0, power: 0.3, strength: 0.05 },
        ]
    }

    /// The volume holds the phase-free N = sum of
    /// dF x V. With every visibility sample 1, N and M take the same f32 additions in the same
    /// order (`n += df * 1`, `m += df`), so N == M exactly for every slice of every column,
    /// whatever the phase block holds (the shader reads none of it: checked on its text).
    #[test]
    fn with_every_sample_lit_n_equals_m_exactly() {
        let grids = [AirGrid::for_view(16.0 / 9.0, None, Some(12_000.0)), AirGrid::for_view(16.0 / 9.0, None, Some(48_000.0)), AirGrid { nx: 16, ny: 8, nz: 8, depth: 400.0, first: 50.0 }];
        for grid in &grids {
            for fog in loop_fogs() {
                for count in 1..=8u32 {
                    // Ray lengths from the centre ray (1) to a wide view's corner.
                    for ray_len in [1.0f32, 1.17, 1.6] {
                        let column = column_mirror(grid, count, ray_len, fog, |_, _| 1.0);
                        for (k, [n, m, v]) in column.iter().enumerate() {
                            assert_eq!(n.to_bits(), m.to_bits(), "slice {k}, {count} samples, ray {ray_len}, {fog:?}: N {n} M {m}");
                            assert_eq!(*v, 1.0);
                        }
                    }
                }
            }
        }
        // The shader has no phase: no phase function, no read of the phase block, and the sums
        // as the mirror takes them.
        let grid = GRID.replace("\r\n", "\n");
        assert!(!grid.contains("fn air_phase") && !grid.contains("fn air_lobe"));
        assert!(!grid.contains("air.phase"), "the grid shader reads the phase block");
        let body = block(&grid, "fn air_grid(");
        for line in [
            "let after = air_fog_share(zb * ray_len);",
            "if (lo > za || hi < zb) && lo < hi {",
            "z = mix(lo, hi, air_sample_within(s, count, o0));",
            "let df = max(after - before, 0.0);",
            "before = max(before, after);",
            "n += df * v;",
            "m += df;",
            "v_sum += v;",
            "let v_mean = select(1.0, v_sum / f32(max(read, 1u)), read > 0u);",
            "kept[k - k0] = vec3<f32>(n, m, v_mean);",
            "air_chunk_sums[li] = vec2<f32>(n, m);",
            "textureStore(volume, vec3<u32>(id.x, id.y, k), vec4<f32>(carry + own.xy, own.z, 1.0));",
            "let fog_hi = (air.fog.x + max(air.fog.y - air.fog.x, 1.0)) / ray_len;",
        ] {
            assert!(body.contains(line), "{line}");
        }
        assert!(grid.contains("return select(mix(z0, z1, f32(s + 1u) / f32(count)), z1, s + 1u >= count);"));
    }

    /// A fixed hash of three numbers to [0, 1).
    fn hash01(a: u32, b: u32, c: u32) -> f32 {
        let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
        h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
        h = (h ^ (h >> 12)).wrapping_mul(0x297A_2D39);
        ((h ^ (h >> 15)) >> 8) as f32 / 16_777_216.0
    }

    /// A fog the light is drawn with, by its number.
    fn random_fog(i: u32) -> AirFog {
        let near = (hash01(i, 1, 0) - 0.2) * 5_000.0;
        AirFog { near, far: near + 10.0f32.powf(1.0 + 4.0 * hash01(i, 2, 0)), power: 0.1 + 2.0 * hash01(i, 3, 0), strength: 0.05 + 0.95 * hash01(i, 4, 0) }
    }

    /// The chunks of every grid the game can make (8 to 128 slices) and of the smaller ones, in
    /// every walk that grid can take, share the slices out: in order, each slice once, none past the last, none over what a
    /// chunk keeps; the shader's text as [`walk_mirror`], [`chunks_mirror`] and the join have it.
    #[test]
    fn the_chunks_cover_every_slice_once() {
        for nz in 1..=medium_air::AIR_SLICES.1 {
            for way in [GridWalk::chunked(nz), GridWalk::LongChunks, GridWalk::Serial] {
                let mut next = 0;
                for (k0, k1) in chunks_mirror(way, nz) {
                    for k in k0..k1 {
                        assert_eq!(k, next, "{way:?}, {nz} slices");
                        assert!(k - k0 < way.chunk_slices(), "{way:?}, {nz} slices: kept[{}]", k - k0);
                        next += 1;
                    }
                }
                assert_eq!(next, nz, "{way:?}");
            }
        }
        assert_eq!(chunks_mirror(GridWalk::Chunks, 64), [(0, 16), (16, 32), (32, 48), (48, 64)]);
        assert_eq!(chunks_mirror(GridWalk::LongChunks, 128), [(0, 32), (32, 64), (64, 96), (96, 128)]);
        assert_eq!(chunks_mirror(GridWalk::Serial, 128), [(0, 128)]);
        // Not a multiple of 4: the last chunk shorter, or (k1 <= k0) empty.
        assert_eq!(chunks_mirror(GridWalk::Chunks, 10), [(0, 3), (3, 6), (6, 9), (9, 10)]);
        assert_eq!(chunks_mirror(GridWalk::Chunks, 9), [(0, 3), (3, 6), (6, 9), (9, 9)]);
        assert_eq!(chunks_mirror(GridWalk::Chunks, 1), [(0, 1), (1, 1), (2, 1), (3, 1)]);
        assert_eq!((GRID_INVOCATIONS, GRID_SHARED_BYTES), (256, 2_048));
        // The variants' constants; the body without its comments and blank lines.
        let body_of = |way: GridWalk| {
            let grid = crate::sun_visibility::tests::code(&grid_variant(false, way));
            for (line, there) in [
                (format!("const AIR_CHUNKS: u32 = {}u;", chunks_of(way)), true),
                (format!("const AIR_CHUNK_SLICES: u32 = {}u;", way.chunk_slices()), true),
                (format!("var<workgroup> air_chunk_sums: array<vec2<f32>, {GRID_INVOCATIONS}>;"), way != GridWalk::Serial),
            ] {
                assert_eq!(grid.lines().filter(|l| l.trim_end() == line).count(), usize::from(there), "{way:?}: {line}");
            }
            block(&grid, "fn air_grid(").lines().filter(|l| !l.trim().is_empty()).map(|l| format!("{}\n", l.trim_end())).collect::<String>()
        };
        let (body, serial) = (body_of(GridWalk::Chunks), body_of(GridWalk::Serial));
        assert_eq!(body_of(GridWalk::LongChunks), body, "the array's size is all that differs");
        assert!(serial.contains("        textureStore(volume, vec3<u32>(id.x, id.y, k), vec4<f32>(n, m, v_mean, 1.0));\n    }\n}"));
        assert!(!serial.contains("kept") && !serial.contains("carry"));
        // One walk for all: what the serial variant does up to its store, the chunks do.
        let walk = |text: &str| text[text.find("    for (var k = k0;").expect("the walk")..text.find("        let v_mean").expect("b")].to_string();
        assert_eq!(walk(&serial), walk(&body));
        for text in [
            "    let inside = id.x < nx && id.y < ny;\n    let per = min((nz + AIR_CHUNKS - 1u) / AIR_CHUNKS, AIR_CHUNK_SLICES);\n    let k0 = lid.z * per;\n    let k1 = select(k0, min(k0 + per, nz), inside);\n",
            "    var n = 0.0;\n    var m = 0.0;\n    var before = 0.0;\n    var z0 = 0.0;\n    if k0 > 0u {\n        z0 = air_depth_at(f32(k0));\n        before = max(air_fog_share(z0 * ray_len), 0.0);\n    }\n",
            "    var kept: array<vec3<f32>, AIR_CHUNK_SLICES>;\n",
            "    for (var k = k0; k < k1; k += 1u) {\n        let z1 = air_depth_at(f32(k) + 1.0);\n",
            "        kept[k - k0] = vec3<f32>(n, m, v_mean);\n    }\n    air_chunk_sums[li] = vec2<f32>(n, m);\n    workgroupBarrier();\n",
            "    var carry = vec2<f32>(0.0, 0.0);\n    for (var j = 0u; j < lid.z; j += 1u) {\n        carry += air_chunk_sums[lid.x + 8u * lid.y + 64u * j];\n    }\n",
            "    for (var k = k0; k < k1; k += 1u) {\n        let own = kept[k - k0];\n        textureStore(volume, vec3<u32>(id.x, id.y, k), vec4<f32>(carry + own.xy, own.z, 1.0));\n    }\n",
        ] {
            assert_eq!(body.matches(text).count(), 1, "{text}");
        }
    }

    /// The join needs every invocation at the barrier: the grid's entry has exactly one, in the
    /// function's own block (not in a branch or a loop), and no return at all; the chunks' sums
    /// are written before it and read after it. The serial variant has neither.
    #[test]
    fn every_invocation_of_the_grid_reaches_its_one_barrier() {
        for volume in [false, true] {
            let serial = crate::sun_visibility::tests::code(&grid_variant(volume, GridWalk::Serial));
            assert!(!block(&serial, "fn air_grid(").contains("Barrier(") && !serial.contains("air_chunk_sums"), "volume {volume}");
        }
        for (volume, way) in [(false, GridWalk::Chunks), (true, GridWalk::Chunks), (false, GridWalk::LongChunks), (true, GridWalk::LongChunks)] {
            let text = crate::sun_visibility::tests::code(&grid_variant(volume, way));
            let body = block(&text, "fn air_grid(");
            assert_eq!(body.matches("Barrier(").count(), 1, "volume {volume}");
            assert!(body.lines().any(|l| l.trim_end() == "    workgroupBarrier();"), "volume {volume}: in the function's own block");
            assert!(!body.contains("return"), "volume {volume}");
            let (before, after) = body.split_once("workgroupBarrier();").expect("the barrier");
            assert!(before.contains("\n    air_chunk_sums[li] = ") && before.matches("air_chunk_sums").count() == 1, "volume {volume}");
            assert!(!before.contains("textureStore(") && after.matches("textureStore(").count() == 1, "volume {volume}");
            assert!(!after.contains("air_chunk_sums[li]") && after.matches("air_chunk_sums").count() == 1, "volume {volume}");
        }
    }

    /// The walks' sums (the chunks a grid takes, the serial variant's one, and the long chunks at
    /// every size) against [`column_reference`], over every slice count, several depths, rays and
    /// sample counts, the loop's fogs, fogs of one narrow ramp or none (strata without a step)
    /// and random ones, with V all lit, all dark, random or a shadow's band. Equal up to the
    /// f32 grouping of the additions: each of the at most nz x count + 4 of either way is off by
    /// at most 2^-24 of the sum so far, at most the column's M. The first chunk (so the serial
    /// variant's column) is the reference to the bit; so are b and the reads wherever the share
    /// before each chunk is (it is, unless `powf` rounds two neighbouring places out of order).
    #[test]
    fn the_walks_sums_are_the_columns() {
        let mut fogs: Vec<AirFog> = loop_fogs().to_vec();
        fogs.extend([
            AirFog { near: 0.0, far: 200_000.0, power: 0.6, strength: 0.0 },
            AirFog { near: 190.0, far: 198.0, power: 1.0, strength: 1.0 },
            AirFog { near: 1_000.0, far: 500.0, power: 0.6, strength: 1.0 },
            AirFog { near: 0.0, far: 100.0, power: 0.0, strength: 0.7 },
            AirFog { near: 60_000.0, far: 90_000.0, power: 1.0, strength: 1.0 },
        ]);
        fogs.extend((0..6).map(random_fog));
        let (mut apart, mut columns) = (0u32, 0u32);
        for nz in 1..=medium_air::AIR_SLICES.1 {
            let depth = [400.0f32, 3_300.0, 12_000.0, 48_000.0][nz as usize % 4];
            let grid = AirGrid { nx: 16, ny: 8, nz, depth, first: medium_air::AIR_FIRST };
            for (f, fog) in fogs.iter().copied().enumerate() {
                for count in [1u32, 3, 4, 8] {
                    let ray_len = [1.0f32, 1.17, 1.6][(nz as usize + f) % 3];
                    let o0 = hash01(nz, f as u32, count);
                    for kind in 0..4u32 {
                        let v = |k: u32, s: u32, z: f32| match kind {
                            0 => 1.0f32,
                            1 => 0.0,
                            2 => hash01(k, s, nz),
                            _ => f32::from(!(0.1 * depth..0.6 * depth).contains(&z)),
                        };
                        let whole = column_reference(&grid, count, ray_len, fog, o0, v);
                        for way in [GridWalk::chunked(nz), GridWalk::LongChunks, GridWalk::Serial] {
                            let case = format!("{way:?}, {nz} slices to {depth}, {count} samples, ray {ray_len}, V {kind}, {fog:?}");
                            let walks = walks_mirror(way, &grid, count, ray_len, fog, o0, v);
                            let (joined, reads) = (join_mirror(&walks), walks.iter().map(|w| w.reads).sum::<u32>());
                            assert_eq!((joined.len(), whole.column.len()), (nz as usize, nz as usize), "{case}");
                            let mut same = true;
                            let mut k0 = 0;
                            for walk in walks.iter().filter(|w| !w.kept.is_empty()) {
                                let (a, b) = (walk.befores[0], whole.befores[k0]);
                                assert!((a - b).abs() <= 4.0 * f32::EPSILON * a.max(b), "{case}: the share before slice {k0}: {a} for {b}");
                                same &= a == b;
                                k0 += walk.kept.len();
                            }
                            columns += 1;
                            apart += u32::from(!same);
                            let first = walks[0].kept.len();
                            let end = whole.column[nz as usize - 1][1].max(joined[nz as usize - 1][1]);
                            let bound = (2 * nz * count + 64) as f32 * (0.5 * f32::EPSILON) * end;
                            for (k, (j, w)) in joined.iter().zip(&whole.column).enumerate() {
                                assert!((j[0] - w[0]).abs() <= bound && (j[1] - w[1]).abs() <= bound, "{case}: slice {k}: {j:?} for {w:?}, bound {bound}");
                                assert!(j[0] <= j[1], "{case}: slice {k}: N over M: {j:?}");
                                if k > 0 {
                                    assert!(j[0] >= joined[k - 1][0] && j[1] >= joined[k - 1][1], "{case}: slice {k}: a sum falls");
                                }
                                if k < first {
                                    assert_eq!(j.map(f32::to_bits), w.map(f32::to_bits), "{case}: slice {k} of the first chunk: {j:?} for {w:?}");
                                }
                                if same {
                                    assert_eq!(j[2].to_bits(), w[2].to_bits(), "{case}: slice {k}: b");
                                }
                                match kind {
                                    0 => assert!(j[0].to_bits() == j[1].to_bits() && j[2] == 1.0, "{case}: slice {k}: lit, {j:?}"),
                                    1 => assert_eq!(j[0], 0.0, "{case}: slice {k}: dark"),
                                    _ => {}
                                }
                            }
                            if same {
                                assert_eq!(reads, whole.reads, "{case}");
                            }
                            if fog.strength == 0.0 {
                                assert!(reads == 0 && joined.iter().all(|j| *j == [0.0, 0.0, 1.0]), "{case}");
                            }
                        }
                    }
                }
            }
        }
        assert!(apart * 100 <= columns, "{apart} of {columns} columns with another share before a chunk");
        // Shares that fall with distance or are negative (no light is drawn with such a fog, but
        // the diagnostic views run the grid): one step or none, in one walk and in the chunks.
        for fog in [
            AirFog { near: -10.0, far: 200_000.0, power: -0.5, strength: 0.8 },
            AirFog { near: 3_000.0, far: 9_000.0, power: -0.5, strength: 0.8 },
            AirFog { near: 0.0, far: 9_000.0, power: 0.6, strength: -0.5 },
            AirFog { near: 0.0, far: 9_000.0, power: -0.5, strength: -0.5 },
        ] {
            for nz in [1u32, 2, 3, 5, 8, 31, 64, 127, 128] {
                let grid = AirGrid { nx: 16, ny: 8, nz, depth: 48_000.0, first: medium_air::AIR_FIRST };
                let v = |k: u32, s: u32, _: f32| hash01(k, s, nz);
                let whole = column_reference(&grid, 4, 1.17, fog, 0.3, v);
                let (joined, _) = column_mirror_at(&grid, 4, 1.17, fog, 0.3, v);
                let bound = (8 * nz + 64) as f32 * (0.5 * f32::EPSILON) * whole.column[nz as usize - 1][1];
                for (k, (j, w)) in joined.iter().zip(&whole.column).enumerate() {
                    assert!((j[0] - w[0]).abs() <= bound && (j[1] - w[1]).abs() <= bound, "{nz} slices, {fog:?}: slice {k}: {j:?} for {w:?}");
                }
            }
        }
    }

    /// Where every sum is a whole number of 2^-13 no addition rounds, so the chunks' column is
    /// the reference to the bit, for every slice count: linear slices 16 units deep, a linear
    /// fog ramp 4,096 units wide (from the camera, or from inside a slice: strata without a step
    /// first), 1, 2, 4 or 8 strata (steps of 2^-8 to 2^-11) and V in quarters.
    #[test]
    fn the_chunks_are_the_column_to_the_bit_where_nothing_rounds() {
        for nz in 1..=medium_air::AIR_SLICES.1 {
            let grid = AirGrid { nx: 16, ny: 8, nz, depth: 16.0 * nz as f32, first: 16.0 };
            assert_eq!((grid.ratio(), grid.curve().depth_at(nz as f32)), (1.0, 16.0 * nz as f32));
            for near in [0.0f32, 40.0, 100.0] {
                let fog = AirFog { near, far: near + 4_096.0, power: 1.0, strength: 1.0 };
                for count in [1u32, 2, 4, 8] {
                    let v = |k: u32, s: u32, _: f32| (hash01(k, s, count) * 5.0).floor() / 4.0;
                    let whole = column_reference(&grid, count, 1.0, fog, 0.4, v);
                    let (joined, reads) = column_mirror_at(&grid, count, 1.0, fog, 0.4, v);
                    assert_eq!((joined.len(), reads), (nz as usize, whole.reads));
                    for (k, (j, w)) in joined.iter().zip(&whole.column).enumerate() {
                        assert_eq!(j.map(f32::to_bits), w.map(f32::to_bits), "{nz} slices, {count} samples, near {near}, slice {k}: {j:?} for {w:?}");
                    }
                }
            }
        }
    }

    /// The chunks' sums are added front to back. By hand: 1, then twice 2^-24 (half the last
    /// place of 1: each rounds to even, away), is 1 in order and 1 + 2^-23 the other way. And
    /// in columns: with four slices and one sample each chunk holds one addend, so joined in
    /// order the additions are the reference's own, to the bit; in many of these columns the
    /// other order gives another sum.
    #[test]
    fn the_chunks_are_joined_in_order() {
        let half = 0.5 * f32::EPSILON;
        let one = |sum: f32| Walk { kept: vec![[sum, sum, 1.0]], sums: Vec2::splat(sum), reads: 0, befores: vec![0.0] };
        let joined = join_mirror(&[one(1.0), one(half), one(half), one(0.0)]);
        assert_eq!(joined[3], [1.0, 1.0, 1.0]);
        assert_eq!((half + half) + 1.0, 1.0 + f32::EPSILON, "the other order");
        let grid = AirGrid { nx: 16, ny: 8, nz: 4, depth: 12_000.0, first: medium_air::AIR_FIRST };
        let mut ordered = 0;
        for i in 0..2_000u32 {
            let fog = AirFog { near: -10.0 - 3_000.0 * hash01(i, 7, 1), ..random_fog(i + 100) };
            let fog = AirFog { far: fog.far.max(30_000.0), ..fog };
            let v = |k: u32, s: u32, _: f32| hash01(k, s, i);
            let whole = column_reference(&grid, 1, 1.17, fog, 0.4, v);
            let walks = walks_mirror(GridWalk::Chunks, &grid, 1, 1.17, fog, 0.4, v);
            assert!(walks.iter().all(|w| w.kept.len() == 1));
            if walks.iter().zip(&whole.befores).any(|(w, b)| w.befores[0] != *b) {
                continue;
            }
            for (k, (j, w)) in join_mirror(&walks).iter().zip(&whole.column).enumerate() {
                assert_eq!(j.map(f32::to_bits), w.map(f32::to_bits), "{fog:?}: slice {k}: {j:?} for {w:?}");
            }
            let s: Vec<Vec2> = walks.iter().map(|w| w.sums).collect();
            ordered += u32::from(((s[2] + s[1]) + s[0]) + s[3] != ((s[0] + s[1]) + s[2]) + s[3]);
        }
        assert!(ordered >= 40, "{ordered} columns whose order matters");
    }

    /// The walk: the chunks on a discrete NVIDIA adapter their workgroup fits (WebGPU's default
    /// limits do) and where the aid asks for them, the smallest kept array that holds the grid's
    /// slices; every other vendor and type, an unknown adapter, and one under any of the limits
    /// take the serial walk, whatever the grid, and are not refused.
    #[test]
    fn the_walk_follows_the_adapter_and_the_slices() {
        use wgpu::DeviceType::{Cpu, DiscreteGpu, IntegratedGpu, Other, VirtualGpu};
        use bevy::render::render_resource::WgpuLimits;
        let limits = WgpuLimits::default();
        assert!(workgroup_fits(&limits));
        for under in [
            WgpuLimits { max_compute_invocations_per_workgroup: 128, ..limits.clone() },
            WgpuLimits { max_compute_workgroup_size_x: 4, ..limits.clone() },
            WgpuLimits { max_compute_workgroup_size_y: 4, ..limits.clone() },
            WgpuLimits { max_compute_workgroup_size_z: 2, ..limits.clone() },
            WgpuLimits { max_compute_workgroup_storage_size: 1_024, ..limits.clone() },
        ] {
            assert!(!workgroup_fits(&under));
        }
        let walk = |aid, adapter, fits, nz| GridWalk::choose(aid, adapter, fits, nz).0;
        // NVIDIA, AMD, Intel, none.
        let vendors = [0x10DE_u32, 0x1002, 0x8086, 0];
        let types = [DiscreteGpu, IntegratedGpu, VirtualGpu, Cpu, Other];
        let adapters: Vec<_> = vendors.into_iter().flat_map(|v| types.map(|t| Some((v, t)))).chain([None]).collect();
        for nz in medium_air::AIR_SLICES.0..=medium_air::AIR_SLICES.1 {
            let chunks = if nz <= 64 { GridWalk::Chunks } else { GridWalk::LongChunks };
            for adapter in &adapters {
                for fits in [true, false] {
                    let nvidia = *adapter == Some((0x10DE, DiscreteGpu));
                    let case = format!("{adapter:?}, fits {fits}, {nz} slices");
                    assert_eq!(walk(AirWalkAid::Auto, *adapter, fits, nz), if nvidia && fits { chunks } else { GridWalk::Serial }, "{case}");
                    assert_eq!(walk(AirWalkAid::Serial, *adapter, fits, nz), GridWalk::Serial, "{case}");
                    assert_eq!(walk(AirWalkAid::Chunks, *adapter, fits, nz), if fits { chunks } else { GridWalk::Serial }, "{case}");
                }
            }
        }
        assert_eq!(GridWalk::chunked(AirGrid::for_view(16.0 / 9.0, None, None).nz), GridWalk::Chunks);
        let why = |aid, adapter, fits| GridWalk::choose(aid, adapter, fits, 64).1;
        assert_eq!(why(AirWalkAid::Auto, Some((0x10DE, DiscreteGpu)), true), "a discrete NVIDIA adapter");
        assert!(why(AirWalkAid::Auto, Some((0x10DE, DiscreteGpu)), false).contains("limits"));
        assert_eq!(why(AirWalkAid::Auto, Some((0x1002, IntegratedGpu)), true), "not a discrete NVIDIA adapter");
        assert_eq!(why(AirWalkAid::Auto, None, true), "the adapter is not known");
        assert!(why(AirWalkAid::Chunks, None, false).contains("limits") && why(AirWalkAid::Serial, None, true).contains("aid"));
        // The aid.
        for (value, aid) in [
            (None, AirWalkAid::Auto),
            (Some("auto"), AirWalkAid::Auto),
            (Some("serial"), AirWalkAid::Serial),
            (Some(" chunks "), AirWalkAid::Chunks),
            (Some("both"), AirWalkAid::Auto),
        ] {
            assert_eq!(medium_air::debug_walk(value), aid, "{value:?}");
        }
        assert_eq!(GridWalk::ALL.map(|w| w as usize), [0, 1, 2], "the pipelines' places");
    }

    /// Each sample carries its own stratum's fog step. With V = 1 the strata's steps telescope
    /// to the slice's: M at slice k's far boundary is F(z(k + 1) x ray_len) (the share there,
    /// for these fogs, which do not fall with distance), as the old one step per slice gave it,
    /// up to f32 rounding: over 64 slices of 8 strata, 512 subtractions and 512 additions of
    /// values at most 1, each off by at most half an ulp of 1 (6e-8), 6.1e-5 at worst: 1e-4.
    /// Slice 0's first stratum starts from a share of 0, not F(0): with a fog starting behind
    /// the camera M at slice 0 is the whole F(z(1) x ray_len).
    #[test]
    fn the_strata_steps_add_up_to_the_slices_step() {
        let grid = AirGrid::for_view(16.0 / 9.0, None, Some(48_000.0));
        let curve = grid.curve();
        for fog in loop_fogs() {
            for count in 1..=8u32 {
                for ray_len in [1.0f32, 1.4] {
                    let column = column_mirror(&grid, count, ray_len, fog, |_, _| 1.0);
                    for (k, [_, m, _]) in column.iter().enumerate() {
                        let share = medium_air::fog_share(curve.depth_at(k as f32 + 1.0) * ray_len, fog);
                        assert!((m - share).abs() < 1e-4, "slice {k}, {count} samples, {fog:?}: M {m} against F {share}");
                    }
                }
            }
        }
        let behind = AirFog { near: -10.0, far: 200_000.0, power: 0.6, strength: 1.0 };
        assert!(medium_air::fog_share(0.0, behind) > 0.002, "the fixture has a share at the camera");
        let column = column_mirror(&grid, 4, 1.0, behind, |_, _| 1.0);
        assert!((column[0][1] - medium_air::fog_share(curve.depth_at(1.0), behind)).abs() < 1e-6, "slice 0 owns the camera's share");
    }

    /// A narrow modded fog ramp wholly inside the last stratum of a slice counts only that
    /// stratum's V. Linear slices 50 units deep (400 units over 8 slices with a first slice of
    /// 50: `AirGrid::ratio` is 1), four strata of 12.5; slice 3 spans 150..200 and its last
    /// stratum 187.5..200 (the mirror's mix gives 187.5 exactly: 150 x 0.25 + 200 x 0.75). The
    /// fog rises from 0 at 190 to 1 at 198 along the centre ray, so slice 3's last stratum gets
    /// the whole step 1 and the others 0: N = 1 x 0.25 exactly, M = 1, and only that stratum's
    /// sample is read, so the slice's b channel is 0.25 too. The old uniform mean would have
    /// stored N = (3 x 0.9 + 0.25) / 4 = 0.7375 for the same column.
    #[test]
    fn a_ramp_inside_one_stratum_counts_only_that_samples_visibility() {
        let grid = AirGrid { nx: 16, ny: 8, nz: 8, depth: 400.0, first: 50.0 };
        assert_eq!(grid.ratio(), 1.0);
        assert_eq!((grid.curve().depth_at(3.0), grid.curve().depth_at(4.0)), (150.0, 200.0));
        assert_eq!(stratum_end_mirror(2, 4, 150.0, 200.0), 187.5);
        let fog = AirFog { near: 190.0, far: 198.0, power: 1.0, strength: 1.0 };
        let column = column_mirror(&grid, 4, 1.0, fog, |_, s| if s == 3 { 0.25 } else { 0.9 });
        for [n, m, _] in &column[..3] {
            assert_eq!((*n, *m), (0.0, 0.0), "no fog before the ramp");
        }
        let [n, m, v] = column[3];
        assert_eq!((n, m), (0.25, 1.0));
        assert_eq!(v, 0.25, "b: the mean of the samples read");
        for [n, m, _] in &column[4..] {
            assert_eq!((*n, *m), (0.25, 1.0), "nothing more after the ramp: the sums carried");
        }
    }

    /// A modded fog ramp from 514 to 515 (one unit) inside a stratum spanning 500 to
    /// 531.25 (linear slices 125 deep: 1,000 units over 8 slices with a first slice of 125,
    /// four strata of 31.25), with a shadow from 513.5 to 515.5 and clear air elsewhere. The
    /// tap unclamped sits at 515.625 (500 + 31.25 x 0.5, dither 0.5), in the clear air; it is
    /// now clamped into the ramp, 514.5, reads the shadow, and the step is all shadowed: N = 0,
    /// M = 1. One read for the whole column: every other stratum has no fog change.
    #[test]
    fn the_tap_sits_inside_a_ramp_narrower_than_its_stratum() {
        let grid = AirGrid { nx: 16, ny: 8, nz: 8, depth: 1_000.0, first: 125.0 };
        assert_eq!(grid.ratio(), 1.0);
        assert_eq!((grid.curve().depth_at(4.0), stratum_end_mirror(0, 4, 500.0, 625.0)), (500.0, 531.25));
        assert_eq!(mix_mirror(500.0, 625.0, sample_depth_mirror(0, 4, 0.5)), 515.625, "the unclamped tap: clear air");
        let fog = AirFog { near: 514.0, far: 515.0, power: 1.0, strength: 1.0 };
        let shadow = |z: f32| if (513.5..=515.5).contains(&z) { 0.0 } else { 1.0 };
        let seen = std::cell::Cell::new(0.0f32);
        let (column, reads) = column_mirror_at(&grid, 4, 1.0, fog, 0.5, |_, _, z| {
            seen.set(z);
            shadow(z)
        });
        assert_eq!(seen.get(), 514.5);
        assert_eq!(reads, 1);
        assert_eq!((column[4][0], column[4][1]), (0.0, 1.0), "the step is read in the shadow");
        assert_eq!((column[7][0], column[7][1]), (0.0, 1.0));
    }

    /// A stratum the fog's ramp covers whole is not clamped: its tap is where it was, to the
    /// bit, for every count and dither (the base game's curve, near 0 and far 200,000, covers
    /// the whole grid along every ray: no read is skipped, 64 x S per column).
    #[test]
    fn an_unclamped_stratum_keeps_its_tap_to_the_bit() {
        let grid = AirGrid::for_view(16.0 / 9.0, None, Some(48_000.0));
        let curve = grid.curve();
        let fog = AirFog { near: 0.0, far: 200_000.0, power: 0.6, strength: 0.8 };
        for count in 1..=8u32 {
            for o0 in [0.0f32, 0.31, 0.5, 0.999_9] {
                for ray_len in [1.0f32, 1.6] {
                    let (_, reads) = column_mirror_at(&grid, count, ray_len, fog, o0, |k, s, z| {
                        let (z0, z1) = (curve.depth_at(k as f32), curve.depth_at(k as f32 + 1.0));
                        assert_eq!(z.to_bits(), mix_mirror(z0, z1, sample_depth_mirror(s, count, o0)).to_bits(), "slice {k} sample {s}");
                        1.0
                    });
                    assert_eq!(reads, grid.nz * count, "every stratum read");
                }
            }
        }
    }

    /// A stratum with no fog change does no visibility read (a counted decision in the
    /// mirror): no fog at all reads nothing; a short ramp (3,000 to 3,400 units at the 12,000
    /// grid, slices about 150 deep there) reads only the strata it crosses (about 11 of 256),
    /// and b is 1 where a slice read nothing.
    #[test]
    fn a_stratum_without_fog_change_reads_nothing() {
        let grid = AirGrid::for_view(16.0 / 9.0, None, Some(12_000.0));
        let none = AirFog { near: 0.0, far: 200_000.0, power: 0.6, strength: 0.0 };
        let (column, reads) = column_mirror_at(&grid, 4, 1.0, none, 0.5, |_, _, _| panic!("a read with no fog change"));
        assert_eq!(reads, 0);
        assert!(column.iter().all(|[n, m, v]| *n == 0.0 && *m == 0.0 && *v == 1.0));
        let short = AirFog { near: 3_000.0, far: 3_400.0, power: 1.0, strength: 1.0 };
        let (column, reads) = column_mirror_at(&grid, 4, 1.0, short, 0.5, |_, _, z| {
            assert!((2_999.0..=3_401.0).contains(&z), "a read outside the ramp: {z}");
            0.5
        });
        assert!(reads > 0 && reads < 24, "{reads} reads");
        let last = column.last().expect("slices");
        assert!((last[1] - 1.0).abs() < 1e-5 && (last[0] - 0.5).abs() < 1e-5, "{last:?}");
        assert_eq!(column[0][2], 1.0, "b with nothing read");
    }

    /// The samples' positions: each inside its own stratum of the slice, their mean the slice's
    /// middle exactly (so a V linear in depth averages to the slice's mean), one sample the
    /// slice's middle with no lateral offset (the pinned legacy case), the dither in [0, 1).
    #[test]
    fn the_visibility_samples_are_stratified_and_centred() {
        for count in 1..=8u32 {
            for o in [0.0f32, 0.13, 0.5, 0.77, 0.999_9] {
                let fractions: Vec<f32> = (0..count).map(|s| sample_depth_mirror(s, count, o)).collect();
                for (s, f) in fractions.iter().enumerate() {
                    let (lo, hi) = (s as f32 / count as f32, (s as f32 + 1.0) / count as f32);
                    assert!(*f >= lo && *f < hi + 1e-6, "count {count} o {o}: sample {s} at {f} outside [{lo}, {hi})");
                }
                let mean = fractions.iter().sum::<f32>() / count as f32;
                assert!((mean - 0.5).abs() < 1e-6, "count {count} o {o}: mean {mean}");
                // A V linear in depth over a slice [z0, z1]: the samples' mean is its value at the
                // middle, which is its mean over the slice.
                let (z0, z1) = (1_000.0f32, 1_466.0f32);
                let v = |z: f32| (z - z0) / (z1 - z0) * 0.8 + 0.1;
                let sampled = fractions.iter().map(|f| v(z0 + (z1 - z0) * f)).sum::<f32>() / count as f32;
                assert!((sampled - v((z0 + z1) * 0.5)).abs() < 1e-5, "count {count} o {o}: {sampled}");
            }
        }
        // One sample: the slice's middle, no lateral offset, whatever the dither.
        for o in [0.0f32, 0.3, 0.999] {
            assert_eq!(sample_depth_mirror(0, 1, o), 0.5);
            assert_eq!(sample_lateral_mirror(0, 1, Vec2::splat(o)), Vec2::ZERO);
        }
        for i in 0..200u32 {
            let o = ign_mirror(Vec2::new((i % 160) as f32, (i * 7 % 88) as f32));
            assert!((0.0..1.0).contains(&o), "{o}");
        }
        // The shader's text as the mirrors have it.
        let grid = GRID.replace("\r\n", "\n");
        for line in [
            "return fract(52.9829189 * fract(dot(c, vec2<f32>(0.06711056, 0.00583715))));",
            "let d = select((o - 0.5) * sign, 0.0, centred);",
            "return (f32(s) + 0.5 + d) / f32(count);",
            "const AIR_R2: vec2<f32> = vec2<f32>(0.7548776662, 0.5698402910);",
            "return clamp(u32(air.misc.y + 0.5), 1u, AIR_MAX_SAMPLES);",
            "z = mix(z0, z1, air_sample_depth(s, count, o0));",
            "if (code == 0u || code == 1u) && z < reach {",
        ] {
            assert!(grid.contains(line), "{line}");
        }
        assert!((1..=8).contains(&medium_air::AIR_SAMPLES_DEFAULT), "the default is inside the shader's range");
    }

    /// Every lateral offset stays inside its own column on each axis (|x|, |y| <= 0.5 column
    /// widths) for every sample count 1..8 and a sweep of the column's two shift values,
    /// including both ends of [0, 1) and the shifts the shader gives the screen's corner
    /// columns; so column (0, 0)'s samples never leave the frustum (the turned pattern put
    /// sample 0 about 0.625 widths out there). Hand bound: fract is in [0, 1), so
    /// fract - 0.5 is in [-0.5, 0.5).
    #[test]
    fn the_lateral_samples_stay_inside_their_column() {
        let mut shifts: Vec<Vec2> = Vec::new();
        let sweep = [0.0f32, 0.001, 0.125, 0.245_122_3, 0.430_159_7, 0.5, 0.625, 0.75, 0.9, 0.999, 0.999_999_9];
        for &a in &sweep {
            for &b in &sweep {
                shifts.push(Vec2::new(a, b));
            }
        }
        // The shader's own shifts (o1, o2) for the corner columns of a 160 x 88 grid and a band.
        for (x, y) in [(0.0f32, 0.0f32), (159.0, 0.0), (0.0, 87.0), (159.0, 87.0)].into_iter().chain((0..300).map(|i| ((i % 160) as f32, (i * 13 % 88) as f32))) {
            let c = Vec2::new(x, y);
            shifts.push(Vec2::new(ign_mirror(c + Vec2::new(5.588_238, 0.0)), ign_mirror(c + Vec2::new(0.0, 5.588_238))));
        }
        for count in 1..=8u32 {
            for shift in &shifts {
                for s in 0..count {
                    let l = sample_lateral_mirror(s, count, *shift);
                    assert!(l.x.abs() <= 0.5 && l.y.abs() <= 0.5, "count {count}, shift {shift}, sample {s}: {l}");
                }
            }
        }
        // Column (0, 0) of a 160 x 88 grid: every sample's NDC inside [-1, 1] on both axes.
        let grid = AirGrid::for_view(16.0 / 9.0, None, None);
        let shift = Vec2::new(ign_mirror(Vec2::new(5.588_238, 0.0)), ign_mirror(Vec2::new(0.0, 5.588_238)));
        for count in 1..=8u32 {
            for s in 0..count {
                let l = sample_lateral_mirror(s, count, shift);
                let ndc = medium_air::froxel_ndc(&grid, 0.5 + l.x, 0.5 + l.y);
                assert!(ndc.x >= -1.0 && ndc.y <= 1.0, "count {count}, sample {s}: {ndc}");
            }
        }
        let grid_text = GRID.replace("\r\n", "\n");
        assert!(grid_text.contains("return fract(shift + AIR_R2 * (f32(s) + 0.5)) - vec2<f32>(0.5, 0.5);"));
        assert!(grid_text.contains("let shift = vec2<f32>(o1, o2);") && grid_text.contains("let lateral = air_sample_lateral(s, count, shift);"));
    }

    /// The mean of K over the pattern's repeat: equal to a brute-force mean of the shader's
    /// mirror over one period of a small synthetic image read through a repeating bilinear
    /// sampler, for an overhead light and one in the fade; 1 when not in use or too low.
    #[test]
    fn the_clouds_mean_transmittance_is_the_mirrors_mean_over_a_repeat() {
        use crate::sun_visibility as sv;
        let side = 4usize;
        let bytes: Vec<u8> = vec![255, 255, 40, 255, 0, 255, 255, 128, 255, 200, 255, 255, 255, 255, 90, 10];
        let image = Image::new(Extent3d { width: side as u32, height: side as u32, depth_or_array_layers: 1 }, TextureDimension::D2, bytes.clone(), TextureFormat::R8Unorm, bevy::asset::RenderAssetUsages::default());
        let mean_value = texel_mean(&image).expect("an R8Unorm image");
        assert!((mean_value - bytes.iter().map(|&b| f32::from(b)).sum::<f32>() / (16.0 * 255.0)).abs() < 1e-6);
        // The repeating bilinear sampler (texel centres at (i + 0.5) / side).
        let sample = |uv: Vec2| {
            let t = uv * side as f32 - Vec2::splat(0.5);
            let (b, f) = (t.floor(), t - t.floor());
            let at = |x: f32, y: f32| f32::from(bytes[(y.rem_euclid(side as f32) as usize) * side + x.rem_euclid(side as f32) as usize]) / 255.0;
            let top = at(b.x, b.y) * (1.0 - f.x) + at(b.x + 1.0, b.y) * f.x;
            let bottom = at(b.x, b.y + 1.0) * (1.0 - f.x) + at(b.x + 1.0, b.y + 1.0) * f.x;
            top * (1.0 - f.y) + bottom * f.y
        };
        let period = 8_000.0f32;
        let cloud = sv::CloudVisibility { image: Handle::default(), uv_scale: Vec2::splat(1.0 / period), uv_offset: Vec2::new(0.3, 0.7), plane_height: 20_000.0, strength: 0.6 };
        let block = cloud_block(Some(&cloud), true);
        let u = AirGpuUniform::default().with_clouds(block);
        let mid = (sv::CLOUD_FADE_FROM + sv::CLOUD_FADE_TO) * 0.5;
        for up in [1.0f32, mid] {
            let l = Vec3::new((1.0 - up * up).sqrt(), up, 0.0);
            // 64 x 64 points over one period: 16 per texel along each axis, evenly spaced.
            let n = 64;
            let mut sum = 0.0f64;
            for j in 0..n {
                for i in 0..n {
                    let p = Vec3::new((i as f32 + 0.25) / n as f32 * period, 100.0, (j as f32 + 0.25) / n as f32 * period);
                    sum += f64::from(clouds_mirror(&u, sample, p, l));
                }
            }
            let brute = (sum / (n * n) as f64) as f32;
            let mean = cloud_mean_k(&block, Some(mean_value), up, 100.0);
            assert!((brute - mean).abs() < 1e-4, "up {up}: brute {brute} against {mean}");
            assert!(mean < 1.0);
        }
        assert_eq!(cloud_mean_k(&AirCloudBlock::default(), Some(mean_value), 1.0, 100.0), 1.0, "not in use");
        assert_eq!(cloud_mean_k(&block, Some(mean_value), sv::CLOUD_FADE_FROM, 100.0), 1.0, "the light too low");
        assert_eq!(cloud_mean_k(&block, Some(1.0), 1.0, 100.0), 1.0, "a clear image");
        assert!((cloud_mean_k(&block, None, 1.0, 100.0) - (1.0 - 0.6 * (1.0 - CLOUD_MEAN_STAND_IN))).abs() < 1e-6, "the stand-in");
        let other = Image::new(Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2, vec![0; 4], TextureFormat::Rgba8Unorm, bevy::asset::RenderAssetUsages::default());
        assert_eq!(texel_mean(&other), None);
    }

    #[test]
    fn the_uniform_takes_the_projection_the_fog_and_the_viewport() {
        let cam = AirCamera {
            grid: AirGrid::for_view(16.0 / 9.0, None, None),
            tune: AirTune::start(1.0, AirView::Slice, AirOccluders::Terrain, None),
            fog: AirFog { near: 0.0, far: 200_000.0, power: 0.6, strength: 0.8 },
            fog_colour: Vec3::new(0.5, 0.4, 0.3),
            display_blend: true,
            to_light: Vec3::Y,
            has_light: true,
            outdoors: true,
            share: 0.5,
            draws: true,
        };
        // Bevy's reverse infinite perspective (ExtractedView's documented form): fov 1 rad,
        // 16:9, near 5.
        let f = 1.0 / 0.5f32.tan();
        let clip = Mat4::from_cols(Vec4::new(f * 9.0 / 16.0, 0.0, 0.0, 0.0), Vec4::new(0.0, f, 0.0, 0.0), Vec4::new(0.0, 0.0, 0.0, -1.0), Vec4::new(0.0, 0.0, 5.0, 0.0));
        let inputs = AirViewInputs { world_from_view: Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0)), clip_from_view: clip, origin: UVec2::new(4, 6), render: UVec2::new(1280, 720) };
        let normed = cam.tune.phase().normed();
        let u = air_uniform(&cam, &normed, &inputs);
        let py = 1.0 / 0.5f32.tan();
        assert!((u.proj.y - py).abs() < 1e-5 && (u.proj.x - py * 9.0 / 16.0).abs() < 1e-5);
        assert_eq!(u.proj.z, 5.0);
        assert_eq!(u.proj.w, crate::sky::SKY_DEPTH_FROM);
        assert_eq!(u.grid, Vec4::new(160.0, 88.0, 64.0, medium_air::AIR_DEPTH));
        assert_eq!(u.fog, Vec4::new(0.0, 200_000.0, 0.6, 0.8));
        assert_eq!(u.phase2.w, AirView::Slice.code() as f32);
        assert_eq!(u.light, Vec4::new(0.0, 1.0, 0.0, 1.0));
        assert_eq!(u.fog_colour, Vec4::new(0.5, 0.4, 0.3, 1.0));
        assert_eq!(u.misc, Vec4::new(AirOccluders::Terrain.code() as f32, medium_air::AIR_SAMPLES_DEFAULT as f32, cam.tune.sky_weight, cam.tune.sky_phase));
        assert_eq!(u.viewport, Vec4::new(4.0, 6.0, 1280.0, 720.0));
        // rays2: LIFT, the REF override (-1 for auto), SHADE, whether SHARE is set.
        assert_eq!(u.rays2, Vec4::new(cam.tune.lift, -1.0, cam.tune.shade, 0.0));
        let set = AirCamera { tune: AirTune { share: Some(0.0), reference: Some(0.6), ..cam.tune }, ..cam };
        assert_eq!(air_uniform(&set, &normed, &inputs).rays2, Vec4::new(cam.tune.lift, 0.6, cam.tune.shade, 1.0));
        assert_eq!(u.world_from_view.w_axis.truncate(), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn the_effective_fog_follows_the_falloff() {
        let game = DistanceFog { color: Color::linear_rgba(0.2, 0.3, 0.4, 0.5), falloff: FogFalloff::Atmospheric { extinction: Vec3::new(10.0, 9_000.0, 0.7), inscattering: Vec3::new(1.0, 1.0, 0.0) }, ..default() };
        let (fog, colour, display) = camera_air_fog(Some(&game));
        assert_eq!(fog, AirFog { near: 10.0, far: 9_000.0, power: 0.7, strength: 0.5 });
        assert!((colour - Vec3::new(0.2, 0.3, 0.4)).abs().max_element() < 1e-6 && display);
        let indoor = DistanceFog { falloff: FogFalloff::Atmospheric { extinction: Vec3::new(0.0, 3_000.0, 1.0), inscattering: Vec3::ZERO }, ..game };
        assert!(!camera_air_fog(Some(&indoor)).2);
        let linear = DistanceFog { falloff: FogFalloff::Linear { start: 8_000.0, end: 20_000.0 }, ..game };
        assert_eq!(camera_air_fog(Some(&linear)), (AirFog { near: 8_000.0, far: 20_000.0, power: 1.0, strength: 0.5 }, camera_air_fog(Some(&game)).1, false));
        let other = DistanceFog { falloff: FogFalloff::Exponential { density: 0.1 }, ..game };
        assert!(!camera_air_fog(Some(&other)).0.usable());
        assert!(!camera_air_fog(None).0.usable());
    }

    /// Off means off: no component, the gate closed; on puts both there; off again takes the
    /// component away and closes the gate.
    #[test]
    fn off_marks_nothing_and_closes_the_gate() {
        let mut app = App::new();
        let gate = add_main_world(&mut app);
        let cam = app.world_mut().spawn((FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, Projection::default(), GlobalTransform::default())).id();
        app.update();
        assert!(app.world().get::<AirCamera>(cam).is_none() && !gate.is_open());
        app.world_mut().insert_resource(AirWanted(true));
        app.update();
        assert!(app.world().get::<AirCamera>(cam).is_some() && gate.is_open());
        app.world_mut().insert_resource(AirWanted(false));
        app.update();
        assert!(app.world().get::<AirCamera>(cam).is_none() && !gate.is_open());
    }

    /// The camera selector takes only a perspective `FlyCam`: an orthographic one loses the
    /// mark it had and gets none while the light is wanted.
    #[test]
    fn a_non_perspective_main_camera_is_not_marked() {
        let mut app = App::new();
        let gate = add_main_world(&mut app);
        let cam = app.world_mut().spawn((FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, Projection::default(), GlobalTransform::default())).id();
        // The first update runs the demand's Startup `read_aid`, which sets AirWanted from the
        // aid (off here): the light is asked for after it, as a played run's settings would.
        app.update();
        app.world_mut().insert_resource(AirWanted(true));
        app.update();
        assert!(app.world().get::<AirCamera>(cam).is_some() && gate.is_open());
        *app.world_mut().get_mut::<Projection>(cam).expect("a projection") = Projection::Orthographic(OrthographicProjection::default_3d());
        app.update();
        assert!(app.world().get::<AirCamera>(cam).is_none(), "an orthographic main camera is not marked");
        app.update();
        assert!(app.world().get::<AirCamera>(cam).is_none());
    }

    /// A stand-in for a view's GPU side ([`AirViewGpu`] needs a device).
    #[derive(Component)]
    struct HeldGpu;

    /// What [`extract_air`] reads from the main world in the drain test: the camera's render
    /// entity and its mark.
    #[derive(Resource, Default)]
    struct MainMark(Option<(Entity, AirCamera)>);

    /// [`extract_air`]'s marking ([`mark_views`]) with the main world's mark handed over.
    fn extract_stand_in(mut commands: Commands, mut state: ResMut<AirState>, main: Res<MainMark>, marked: Query<Entity, With<AirCamera>>) {
        mark_views(&mut commands, &mut state, main.0, &marked);
    }

    /// [`prepare_air`]'s part that matters here: a marked view gets its GPU side.
    /// The marked views still without the stand-in GPU side.
    type StandInViews = (With<ExtractedView>, With<AirCamera>, Without<HeldGpu>);

    fn prepare_stand_in(mut commands: Commands, views: Query<Entity, StandInViews>) {
        for e in &views {
            commands.entity(e).insert(HeldGpu);
        }
    }

    /// A view in the render world, as Bevy extracts an active camera (only its presence counts).
    fn extracted_view() -> ExtractedView {
        ExtractedView {
            retained_view_entity: bevy::render::view::RetainedViewEntity::new(bevy::render::sync_world::MainEntity::from(Entity::PLACEHOLDER), None, 0),
            clip_from_view: Mat4::IDENTITY,
            world_from_view: GlobalTransform::default(),
            clip_from_world: None,
            target_format: TextureFormat::Rgba16Float,
            viewport: UVec4::ZERO,
            color_grading: default(),
            invert_culling: false,
        }
    }

    /// One frame of both worlds: the main world's update, the mark handed to the render world
    /// (the extraction), then the render side's PrepareResources ([`drain_air`] and the
    /// stand-in for [`prepare_air`]), each under the real run condition [`air_render_in_use`].
    fn frame(app: &mut App, cam: Entity, render: &mut World, view: Entity, schedules: &mut (Schedule, Schedule)) {
        app.update();
        let mark = app.world().get::<AirCamera>(cam).copied();
        render.resource_mut::<MainMark>().0 = mark.map(|m| (view, m));
        schedules.0.run(render);
        schedules.1.run(render);
    }

    /// A camera made inactive (Bevy takes `ExtractedView` off its
    /// render entity and leaves the air's components there), then the light switched off:
    /// the render side keeps running until no render entity holds a mark or a GPU side, and
    /// afterwards nothing is held, the state is released and the render side stops.
    #[test]
    fn an_inactive_camera_then_the_light_off_holds_nothing() {
        use bevy::ecs::schedule::SingleThreadedExecutor;
        let mut app = App::new();
        let gate = add_main_world(&mut app);
        let cam = app.world_mut().spawn((FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, Projection::default(), GlobalTransform::default())).id();
        let mut render = World::new();
        render.insert_resource(gate.clone());
        render.init_resource::<AirState>();
        render.init_resource::<MainMark>();
        let view = render.spawn(extracted_view()).id();
        let mut extract = Schedule::default();
        extract.set_executor(SingleThreadedExecutor::new());
        extract.add_systems(extract_stand_in.run_if(air_render_in_use));
        let mut prepare = Schedule::default();
        prepare.set_executor(SingleThreadedExecutor::new());
        prepare.add_systems((drain_air::<HeldGpu>, prepare_stand_in).chain().run_if(air_render_in_use));
        let mut schedules = (extract, prepare);
        let holds = |render: &World| (render.get::<AirCamera>(view).is_some(), render.get::<HeldGpu>(view).is_some(), render.resource::<AirState>().held);

        // On (after the first update, whose Startup `read_aid` sets AirWanted from the aid):
        // the view is marked, gets its GPU side, the state holds it.
        app.update();
        app.world_mut().insert_resource(AirWanted(true));
        frame(&mut app, cam, &mut render, view, &mut schedules);
        frame(&mut app, cam, &mut render, view, &mut schedules);
        assert_eq!(holds(&render), (true, true, true));

        // The camera made inactive: its view goes, the mark stays (the main camera keeps it);
        // the GPU side is dropped and the render side keeps running for the mark.
        render.entity_mut(view).remove::<ExtractedView>();
        frame(&mut app, cam, &mut render, view, &mut schedules);
        assert_eq!(holds(&render), (true, false, true), "an inactive view loses its volume, its mark keeps the render side on");
        frame(&mut app, cam, &mut render, view, &mut schedules);
        assert_eq!(holds(&render), (true, false, true));

        // The light switched off: the gate closes; the render side still runs, takes the mark
        // away and then holds nothing.
        app.world_mut().insert_resource(AirWanted(false));
        frame(&mut app, cam, &mut render, view, &mut schedules);
        assert!(!gate.is_open());
        assert_eq!(holds(&render), (false, false, false), "nothing held after the light went off");
        for _ in 0..3 {
            frame(&mut app, cam, &mut render, view, &mut schedules);
            assert_eq!(holds(&render), (false, false, false));
        }
        assert!(app.world().get::<AirCamera>(cam).is_none());

        // A GPU side left on an unmarked entity (the orphan case) is drained too, and the light
        // on again marks and fills the view once it is active again.
        render.entity_mut(view).insert((extracted_view(), HeldGpu));
        render.resource_mut::<AirState>().held = true;
        frame(&mut app, cam, &mut render, view, &mut schedules);
        frame(&mut app, cam, &mut render, view, &mut schedules);
        assert_eq!(holds(&render), (false, false, false), "an orphaned GPU side is drained");
        app.world_mut().insert_resource(AirWanted(true));
        frame(&mut app, cam, &mut render, view, &mut schedules);
        frame(&mut app, cam, &mut render, view, &mut schedules);
        assert_eq!(holds(&render), (true, true, true));
    }

    /// The format's answer is asked once and kept; the grid's size is checked on every call,
    /// both ways (a 256-texel 3D limit: 160 columns drawn, 320 refused, 160 drawn again;
    /// starting at 320 and tuning down to 160 recovers too); a refused format refuses every
    /// grid.
    #[test]
    fn the_volume_support_caches_the_format_only() {
        let at = |columns: u32| AirGrid { nx: columns, ..AirGrid::for_view(16.0 / 9.0, None, None) };
        let asked = std::cell::Cell::new(0);
        let format = || {
            asked.set(asked.get() + 1);
            Ok(())
        };
        let mut support = VolumeSupport::default();
        assert!(support.check(format, 256, &at(160)).is_ok());
        let refused = support.check(format, 256, &at(320)).expect_err("over the limit");
        assert!(refused.contains("320") && refused.contains("256"), "{refused}");
        assert!(support.check(format, 256, &at(160)).is_ok(), "tuned down again: drawn");
        assert_eq!(asked.get(), 1, "the format asked once");
        let mut support = VolumeSupport::default();
        assert!(support.check(format, 256, &at(320)).is_err());
        assert!(support.check(format, 256, &at(160)).is_ok(), "starting over the limit and tuning down recovers");
        // The slices count too (the largest side).
        assert!(grid_fits(64, &AirGrid { nx: 64, ny: 64, nz: 65, depth: 12_000.0, first: 50.0 }).is_err());
        assert!(grid_fits(2_048, &at(320)).is_ok());
        let mut support = VolumeSupport::default();
        assert!(support.check(|| Err("no rgba16float".to_string()), 2_048, &at(160)).is_err());
        assert_eq!(support.check(|| panic!("asked again"), 2_048, &at(160)), Err("no rgba16float".to_string()), "a refused format stays refused");
    }

    /// The inputs both shaders can read: Bevy's reverse infinite perspective with positive
    /// scales and near term, finite fog numbers and colour; anything else is refused with its
    /// reason.
    #[test]
    fn unsupported_projections_and_fogs_are_refused() {
        let f = 1.0 / 0.5f32.tan();
        let perspective = Mat4::from_cols(Vec4::new(f * 9.0 / 16.0, 0.0, 0.0, 0.0), Vec4::new(0.0, f, 0.0, 0.0), Vec4::new(0.0, 0.0, 0.0, -1.0), Vec4::new(0.0, 0.0, 5.0, 0.0));
        let fog = AirFog { near: 0.0, far: 200_000.0, power: 0.6, strength: 0.8 };
        let colour = Vec3::new(0.5, 0.4, 0.3);
        assert_eq!(unsupported_inputs(&perspective, fog, colour), None);
        // Bevy's own perspective (PerspectiveProjection, no oblique near plane) passes.
        let bevy_perspective = bevy::camera::CameraProjection::get_clip_from_view(&PerspectiveProjection { near: 5.0, aspect_ratio: 16.0 / 9.0, ..default() });
        assert_eq!(unsupported_inputs(&bevy_perspective, fog, colour), None);
        // An orthographic projection (w_axis.w 1, Bevy's documented test).
        let ortho = bevy::camera::CameraProjection::get_clip_from_view(&OrthographicProjection::default_3d());
        assert!(unsupported_inputs(&ortho, fog, colour).is_some_and(|r| r.contains("perspective")));
        let with = |change: &dyn Fn(&mut Mat4)| {
            let mut m = perspective;
            change(&mut m);
            unsupported_inputs(&m, fog, colour)
        };
        assert!(with(&|m| m.x_axis.x = 0.0).is_some_and(|r| r.contains("scale")));
        assert!(with(&|m| m.y_axis.y = -1.0).is_some_and(|r| r.contains("scale")));
        assert!(with(&|m| m.w_axis.z = 0.0).is_some_and(|r| r.contains("near")));
        assert!(with(&|m| m.w_axis.z = -5.0).is_some_and(|r| r.contains("near")));
        assert!(with(&|m| m.x_axis.x = f32::NAN).is_some_and(|r| r.contains("finite")));
        assert!(with(&|m| m.y_axis.y = f32::INFINITY).is_some_and(|r| r.contains("finite")));
        assert!(with(&|m| m.z_axis.z = 0.1).is_some_and(|r| r.contains("perspective")), "a finite far plane: view depth is not near / depth");
        assert!(with(&|m| m.z_axis.x = 0.01).is_some_and(|r| r.contains("centred")), "an off-centre projection");
        assert!(with(&|m| m.z_axis.y = -0.01).is_some_and(|r| r.contains("centred")));
        for bad in [AirFog { near: f32::NAN, ..fog }, AirFog { far: f32::INFINITY, ..fog }, AirFog { power: f32::NAN, ..fog }, AirFog { strength: f32::NEG_INFINITY, ..fog }] {
            assert!(unsupported_inputs(&perspective, bad, colour).is_some_and(|r| r.contains("fog")), "{bad:?}");
        }
        assert!(unsupported_inputs(&perspective, fog, Vec3::new(0.5, f32::NAN, 0.3)).is_some_and(|r| r.contains("colour")));
        // No fog at all (strength 0, as Bevy's other falloffs give it) is finite: drawn, its share 0.
        let (none, none_colour, _) = camera_air_fog(None);
        assert_eq!(unsupported_inputs(&perspective, none, none_colour), None);
    }

    const FILTER: &str = include_str!("medium_air_filter.wesl");

    #[test]
    fn the_filter_shader_is_valid_wgsl() {
        validate(&plain_wgsl(FILTER, &[], ""), "air_filter", wgpu::naga::ShaderStage::Compute, "filter");
        let filter = FILTER.replace("\r\n", "\n");
        for line in [
            "@group(0) @binding(0) var source: texture_3d<f32>;",
            "@group(0) @binding(1) var filtered: texture_storage_3d<rgba16float, write>;",
            "@compute @workgroup_size(8, 8, 1)",
            "let w = f32((2 - abs(dx)) * (2 - abs(dy)));",
            "if p.x >= 0 && p.y >= 0 && p.x <= last.x && p.y <= last.y {",
            "textureStore(filtered, id, vec4<f32>(sum / weight, 1.0));",
        ] {
            assert!(filter.contains(line), "{line}");
        }
    }

    /// CPU mirror of medium_air_filter.wesl `air_filter` at texel (x, y) of an `nx` x `ny`
    /// slice, in the shader's order (dy outer, dx inner, f32 sums).
    fn filter_mirror(field: &dyn Fn(i32, i32) -> Vec3, nx: i32, ny: i32, x: i32, y: i32) -> Vec3 {
        let (mut sum, mut weight) = (Vec3::ZERO, 0.0f32);
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                let (px, py) = (x + dx, y + dy);
                if px >= 0 && py >= 0 && px < nx && py < ny {
                    let w = ((2 - dx.abs()) * (2 - dy.abs())) as f32;
                    sum += w * field(px, py);
                    weight += w;
                }
            }
        }
        sum / weight
    }

    /// The filter's weights at a texel, renormalised over the neighbours that exist.
    fn filter_weights(nx: i32, ny: i32, x: i32, y: i32) -> (f32, f32) {
        let raw: f32 = (-1..=1i32).flat_map(|dy| (-1..=1i32).map(move |dx| (dx, dy))).filter(|(dx, dy)| x + dx >= 0 && y + dy >= 0 && x + dx < nx && y + dy < ny).map(|(dx, dy)| ((2 - dx.abs()) * (2 - dy.abs())) as f32).sum();
        let normed: f32 = (-1..=1i32).flat_map(|dy| (-1..=1i32).map(move |dx| (dx, dy))).filter(|(dx, dy)| x + dx >= 0 && y + dy >= 0 && x + dx < nx && y + dy < ny).map(|(dx, dy)| ((2 - dx.abs()) * (2 - dy.abs())) as f32 / raw).sum();
        (raw, normed)
    }

    /// The tent's weights: 16 at interior texels, 12 on an edge, 9 in a corner (the products of
    /// each axis's 4 or 3), each set summing to 1 once renormalised; a constant field comes out
    /// unchanged, exactly for values with few mantissa bits (every partial sum of 1, 2 and 4
    /// times the value is then exact; any other value within an ulp or two of f32, under the
    /// half float the volume stores anyway).
    #[test]
    fn the_filter_keeps_a_constant_and_its_weights_sum_to_one() {
        let (nx, ny) = (160, 88);
        for ((x, y), raw) in [((50, 40), 16.0), ((0, 40), 12.0), ((159, 40), 12.0), ((50, 0), 12.0), ((50, 87), 12.0), ((0, 0), 9.0), ((159, 87), 9.0), ((0, 87), 9.0)] {
            let (r, normed) = filter_weights(nx, ny, x, y);
            assert_eq!(r, raw, "({x}, {y})");
            assert!((normed - 1.0).abs() < 1e-6, "({x}, {y}): {normed}");
            for c in [Vec3::new(0.375, 0.8125, 1.0), Vec3::ZERO, Vec3::new(0.5, 0.25, 0.125)] {
                assert_eq!(filter_mirror(&|_, _| c, nx, ny, x, y), c, "({x}, {y})");
            }
            let c = Vec3::new(0.1, 0.7, 0.33);
            assert!((filter_mirror(&|_, _| c, nx, ny, x, y) - c).abs().max_element() < 3e-7, "({x}, {y})");
        }
    }

    /// M stays monotone in z: a field whose every column grows with the slice gives filtered
    /// slices that grow too (the same non-negative weights in every slice, and f32 addition,
    /// multiplication and division by positive numbers never reverse an order), and N <= M
    /// survives.
    #[test]
    fn the_filter_keeps_m_monotone_in_z() {
        let (nx, ny) = (24, 16);
        let m = |x: i32, y: i32, k: i32| (k as f32 * (0.01 + ((x * 7 + y * 3) % 11) as f32 * 0.003)).min(0.9) + ((x + y) % 2) as f32 * 0.001 * k as f32;
        for (x, y) in [(0, 0), (5, 7), (23, 15), (12, 0), (0, 9)] {
            let mut last = -1.0f32;
            for k in 0..64 {
                let field = |px: i32, py: i32| {
                    let mm = m(px, py, k);
                    Vec3::new(mm * 0.6, mm, 1.0)
                };
                let f = filter_mirror(&field, nx, ny, x, y);
                assert!(f.y >= last, "({x}, {y}) slice {k}: {} after {last}", f.y);
                assert!(f.x <= f.y, "N <= M");
                last = f.y;
            }
        }
    }

    /// The dither's two extreme values in a checkerboard (the worst column-to-column
    /// alternation) become their mean at interior texels: the centre's 4 and the corners' 4 x 1
    /// carry one value, the edges' 4 x 2 the other, 8 / 16 each (the tent's response at the
    /// grid's Nyquist frequency is 0; a box would leave (5 a + 4 b) / 9).
    #[test]
    fn the_filter_flattens_a_checkerboard_at_interior_texels() {
        let (nx, ny) = (32, 16);
        for (a, b) in [(0.0f32, 1.0f32), (0.1, 0.9)] {
            let board = |x: i32, y: i32| Vec3::splat(if (x + y) % 2 == 0 { a } else { b });
            for (x, y) in [(1, 1), (2, 1), (10, 7), (30, 14)] {
                let f = filter_mirror(&board, nx, ny, x, y);
                assert!((f - Vec3::splat(0.5 * (a + b))).abs().max_element() < 1e-6, "({x}, {y}): {f}");
            }
        }
    }

    /// `FILTER`: on by default, `0` off and `1` on again in the tune file, a change the poller
    /// sees and logs.
    #[test]
    fn the_filter_key_switches_the_pass() {
        let base = AirTune::default();
        assert!(base.filter);
        let off = AirTune::parse("FILTER=0", &base);
        assert!(!off.filter);
        assert!(AirTune::parse("FILTER=1", &off).filter);
        assert!(!AirTune::parse("FILTER=nonsense", &off).filter, "an unreadable value keeps the base");
        assert!(medium_air::tune_changed(&base, &off));
        assert_eq!(medium_air::tune_diff(&base, &off), "FILTER=0");
    }

    /// The grid (and the filter) run while the light draws in every view the composite draws,
    /// and while it is gated off only in the diagnostic views the composite shows without it;
    /// the composite's own list of views it draws and of those that need the light agrees.
    #[test]
    fn the_grid_is_not_dispatched_while_the_light_is_gated_off() {
        use crate::medium_air_compose::{draws, shows_light};
        let all = [AirView::None, AirView::Visibility, AirView::Slice, AirView::Share, AirView::Delta, AirView::Off, AirView::Lit, AirView::Rays];
        for view in all {
            assert!(grid_dispatched(view, true) || !draws(view), "{view:?}: drawn with the light on");
            if shows_light(view) {
                assert!(!grid_dispatched(view, false), "{view:?}: no grid while gated off");
            } else if draws(view) {
                assert!(grid_dispatched(view, false), "{view:?}: a diagnostic view keeps its grid");
            }
        }
        assert!(!grid_dispatched(AirView::None, false) && grid_dispatched(AirView::None, true));
        assert!(!grid_dispatched(AirView::Off, false) && grid_dispatched(AirView::Off, true));
        assert!(grid_dispatched(AirView::Visibility, false) && grid_dispatched(AirView::Lit, false));
    }

    /// The light wanted for one extraction, then off before the render side's
    /// preparation looks (the main world closed the shared gate in between): the mark queued
    /// by that extraction latched `held`, so the render side runs on, drains it, and nothing
    /// is held afterwards.
    #[test]
    fn a_mark_queued_just_before_the_gate_closes_is_drained() {
        use bevy::ecs::schedule::SingleThreadedExecutor;
        let mut app = App::new();
        let gate = add_main_world(&mut app);
        let cam = app.world_mut().spawn((FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, Projection::default(), GlobalTransform::default())).id();
        let mut render = World::new();
        render.insert_resource(gate.clone());
        render.init_resource::<AirState>();
        render.init_resource::<MainMark>();
        let view = render.spawn(extracted_view()).id();
        let mut extract = Schedule::default();
        extract.set_executor(SingleThreadedExecutor::new());
        extract.add_systems(extract_stand_in.run_if(air_render_in_use));
        let mut prepare = Schedule::default();
        prepare.set_executor(SingleThreadedExecutor::new());
        prepare.add_systems((drain_air::<HeldGpu>, prepare_stand_in).chain().run_if(air_render_in_use));
        let mut schedules = (extract, prepare);
        let holds = |render: &World| (render.get::<AirCamera>(view).is_some(), render.get::<HeldGpu>(view).is_some(), render.resource::<AirState>().held);

        // The first update runs Startup's read_aid; then the light is asked for.
        app.update();
        app.world_mut().insert_resource(AirWanted(true));
        app.update();
        assert!(gate.is_open());
        // One extraction with the light wanted: the mark is queued (applied at its end).
        let mark = app.world().get::<AirCamera>(cam).copied();
        render.resource_mut::<MainMark>().0 = mark.map(|m| (view, m));
        schedules.0.run(&mut render);
        assert!(render.get::<AirCamera>(view).is_some() && render.resource::<AirState>().held, "queuing the mark latches held");
        // The main world switches the light off before the preparation runs.
        app.world_mut().insert_resource(AirWanted(false));
        app.update();
        assert!(!gate.is_open());
        schedules.1.run(&mut render);
        for _ in 0..3 {
            frame(&mut app, cam, &mut render, view, &mut schedules);
        }
        assert_eq!(holds(&render), (false, false, false), "nothing stranded");
        assert!(render.resource::<AirState>().cloud.is_none());
    }
    /// The cloud image's texels rewritten under the same handle while the light was
    /// off, with no modification event reaching the system (the asset events are never sent
    /// here, as an expired one is never read): on again, the mean is the new texels', not the
    /// cached one of the earlier weather.
    #[test]
    fn the_cloud_mean_is_worked_out_again_after_the_light_was_off() {
        use crate::sun_visibility as sv;
        let mut app = App::new();
        add_main_world(&mut app);
        app.init_resource::<Assets<Image>>().add_message::<AssetEvent<Image>>();
        let texels = |v: u8| Image::new(Extent3d { width: 2, height: 2, depth_or_array_layers: 1 }, TextureDimension::D2, vec![v; 4], TextureFormat::R8Unorm, bevy::asset::RenderAssetUsages::default());
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(texels(51));
        app.insert_resource(sv::SunVisibility { cloud: Some(sv::CloudVisibility { image: image.clone(), uv_scale: Vec2::splat(1e-4), uv_offset: Vec2::ZERO, plane_height: 20_000.0, strength: 0.6 }), ..default() });
        // The first update runs Startup's read_aid; then the light is asked for.
        app.update();
        app.insert_resource(AirWanted(true));
        app.update();
        let mean = |app: &App| app.world().resource::<AirCloudMean>().mean.expect("a mean");
        assert!((mean(&app) - 0.2).abs() < 1e-6, "{}", mean(&app));
        app.insert_resource(AirWanted(false));
        app.update();
        app.update();
        // Another weather's texels under the same handle; no event delivered.
        app.world_mut().resource_mut::<Assets<Image>>().get_mut(&image).expect("the image").data = Some(vec![204; 4]);
        app.update();
        app.insert_resource(AirWanted(true));
        app.update();
        assert!((mean(&app) - 0.8).abs() < 1e-6, "the new texels' mean: {}", mean(&app));
        assert_eq!(app.world().resource::<AirCloudMean>().image, Some(image.id()));
    }
}
