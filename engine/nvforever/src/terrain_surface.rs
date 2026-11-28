//! [design] The mesh terrain blend's bounded 3x3 unlit colour atlas (#2661), using
//! terrain.rs's prepared meshes and materials; nothing is captured, baked or allocated
//! while the blend is off (`mesh_terrain_blend.rs`: the settings row or the test aid;
//! [`SurfaceBakePlugin`] says what runs then).
//! Heights cover every prepared cell; colour validity is per quadrant.
//! A quadrant's colour is its whole near-terrain stack: the base draw (folded or not)
//! and then every blended layer draw over it, as flat unlit copies with the near draws'
//! textures, alpha and draw order (terrain.rs `spawn_terrain_with`). A folded base (only
//! where the GPU has bindless textures, `TerrainMaterials::folds`) is copied with its
//! [`TerrainMaterial`] (terrain.wesl `SURFACE_BAKE`); every other base and every layer
//! with the single-texture [`SurfaceCaptureMaterial`] (terrain_capture.wesl), whose
//! material group binds one texture. The bake publishes only after every copy, opaque
//! and blended, has drawn in one frame. The atlas is 512 (default), 1024 or 2048 texels
//! per cell side (the settings row: 512 since round 10b; the test aid chooses), checked
//! against the device's texture limit.
//! Round 10a: with the detail capability a publication also packs, after the heights,
//! each admitted quadrant's near-draw description (its layer images in a per-publication
//! palette, prepared vertex attributes and opacities: [`QuadrantDetail`], [`DetailPalette`]),
//! which the blend shader samples directly near the camera (graphics-enhanced.md §3.14).
//! Round 10b: the colour atlas has a mip chain down to one texel per cell
//! ([`colour_levels`]). The bake camera draws into a single-level bake target; the render
//! world's assembly (`assemble_atlas`) then copies the retained cells at every level from
//! the previous publication and the baked cells' level 0 from the bake target, and filters
//! each baked cell's lower levels from its own texels only (terrain_surface_mips.wgsl),
//! before the receipt.

use crate::terrain::TerrainMaterial;
use crate::world::{bevy_to_game, game_to_bevy, Streamer, WorldData};
use bevy::asset::RenderAssetUsages;
use bevy::camera::{visibility::RenderLayers, Hdr, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::ImageSampler;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems, sync_world::RenderEntity};
use bevy::render::render_phase::{CachedRenderPipelinePhaseItem, DrawFunctions, RenderCommand, RenderCommandResult, RenderCommandState, PhaseItem, TrackedRenderPass};
use bevy::core_pipeline::core_3d::{Opaque3d, Transparent3d};
use bevy::ecs::{query::ROQueryItem, system::SystemParamItem};
use std::{collections::{HashMap, HashSet}, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}};
use bevy::render::render_resource::{AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat, TextureUsages};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialGate, MaterialPipeline, MaterialPipelineKey};
use bevy::shader::ShaderRef;

pub use nvf_assets::nif_surface_blend::{GRID, HeightLayout, DEFAULT_RESOLUTION, RESOLUTIONS};
/// Texels across the 3x3-cell colour atlas.
pub fn atlas_width(resolution: u32) -> u32 { 3 * resolution }
/// [design] A resolution the aid accepts and the device can hold as one atlas texture.
pub fn resolution_supported(resolution: u32, max_texture_dimension_2d: u32) -> bool {
    RESOLUTIONS.contains(&resolution) && atlas_width(resolution) <= max_texture_dimension_2d
}
/// [design] #2149/#2661: the height atlas's rows, the heights' and, with landscape detail,
/// the detail words' after them.
pub fn height_atlas_rows(layout: HeightLayout, detail: bool) -> usize {
    if detail { layout.rows() + layout.detail_rows() } else { layout.rows() }
}
/// [design] #2149/#2661: a height atlas of `layout` the device can hold as one texture.
pub fn height_atlas_supported(layout: HeightLayout, detail: bool, max_texture_dimension_2d: u32) -> bool {
    layout.width().max(height_atlas_rows(layout, detail)) <= max_texture_dimension_2d as usize
}
/// [design] Round 10b: the colour atlas's mip levels, from `resolution` texels per cell side
/// down to one texel per cell. Every level is a 3 by 3 grid of whole cells (the resolutions
/// are powers of two), and no level below one texel per cell exists, so no texel of any
/// level covers two cells.
pub fn colour_levels(resolution: u32) -> u32 { resolution.ilog2() + 1 }
/// Colour atlas storage in bytes (RGBA16F, every level), CPU copies, the bake's single-level
/// target and render intermediates aside.
pub fn colour_bytes(resolution: u32) -> u64 {
    (0..colour_levels(resolution)).map(|level| u64::from(atlas_width(resolution) >> level).pow(2) * 8).sum()
}
/// `bytes` in MiB, rounded to the nearest (the mip chain is a few bytes short of 4/3 of level 0).
pub fn mib(bytes: u64) -> u64 { (bytes + (1 << 19)) >> 20 }
pub const BAKE_LAYER: usize = 16;
const INVALID_HEIGHT: f32 = -f32::MAX;

/// [design] How a quadrant's colour is captured: its base draw, folded (bindless) or not
/// (fallback, non-bindless), with `layers` blended layer draws over it; or not at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePath { Folded { layers: usize }, Fallback { layers: usize }, HeightOnly(&'static str) }
impl CapturePath {
    fn label(self) -> String {
        let with = |name: &str, layers: usize| if layers == 0 { name.to_string() } else { format!("{name}+{layers}-layers") };
        match self {
            Self::Folded { layers } => with("folded", layers), Self::Fallback { layers } => with("fallback", layers),
            Self::HeightOnly(reason) => format!("height-only:{reason}"),
        }
    }
    fn colour_valid(self) -> bool { !matches!(self, Self::HeightOnly(_)) }
}

pub const CAPTURE_SHADER: &str = "embedded://nvforever/terrain_capture.wesl";

/// [design] The unlit capture of one landscape draw that is not a folded base: a fallback
/// or non-bindless base, or a blended layer (terrain_capture.wesl). One diffuse texture,
/// one sampler, one uniform: its material group fits a GPU without bindless textures,
/// where [`TerrainMaterial`]'s (six standard and the layers' textures) does not.
/// Registered in every run behind the blend's gate ([`SurfaceBakePlugin`]), drawn only
/// while the blend bakes; no prepass, no shadows.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[bind_group_data(SurfaceCaptureKey)]
pub struct SurfaceCaptureMaterial {
    #[uniform(0)]
    pub values: SurfaceCaptureUniform,
    /// The source's diffuse texture; `None` samples white (the fallback image), and the
    /// uniform's flag ignores it then.
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Option<Handle<Image>>,
    /// A layer copy: alpha blended over the copies before it, without depth writes.
    pub layer: bool,
    /// The source material's depth bias: sort order plus rasteriser bias (`sort_first`).
    pub depth_bias: f32,
}

/// [design] terrain_capture.wesl `SurfaceCapture`.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType, Reflect)]
pub struct SurfaceCaptureUniform {
    /// Linear material colour; alpha the material's opacity.
    pub colour: Vec4,
    /// The texture coordinate transform's x axis (xy) and y axis (zw).
    pub uv_axes: Vec4,
    /// xy the transform's translation; z 1 with a diffuse texture; w 1 for a layer.
    pub uv_offset_flags: Vec4,
    /// [design] #2551: x the world units per texture repeat of an anti-tiled source
    /// (`NifSurface::terrain_repeat`), 0 when the source reads its texture plainly; y, z, w
    /// unused.
    pub anti_tiling: Vec4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceCaptureKey { layer: bool, raster_bias: i32, anti_tiling: bool }
impl From<&SurfaceCaptureMaterial> for SurfaceCaptureKey {
    fn from(m: &SurfaceCaptureMaterial) -> Self {
        Self { layer: m.layer, raster_bias: nvf_assets::nif_material::raster_bias(m.depth_bias as i32), anti_tiling: m.values.anti_tiling.x > 0.0 }
    }
}

impl SurfaceCaptureMaterial {
    /// [design] The capture of a near draw with the landscape NIF material `source`: its
    /// StandardMaterial's values ([`Self::from_standard`]) and, with terrain anti-tiling on
    /// that material (#2551, `NifSurfaceKey::anti_tiling`), the same anti-tiled read of its
    /// diffuse texture at the same world lattice (terrain_capture.wesl `TERRAIN_ANTI_TILING`,
    /// nvf_assets terrain_pbr.wesl `terrain_pbr_standard`).
    pub fn from_source(source: &nvf_assets::NifMaterial, layer: bool) -> Self {
        let mut capture = Self::from_standard(&source.base, layer);
        if nvf_assets::nif_material::NifSurfaceKey::from(&source.extension).anti_tiling {
            capture.values.anti_tiling.x = source.extension.terrain_repeat;
        }
        capture
    }

    /// The capture of a near draw with `source` (terrain.rs `surface`, `layer_material`),
    /// read plainly.
    pub fn from_standard(source: &StandardMaterial, layer: bool) -> Self {
        let colour = LinearRgba::from(source.base_color);
        let t = source.uv_transform;
        Self {
            values: SurfaceCaptureUniform {
                colour: Vec4::new(colour.red, colour.green, colour.blue, colour.alpha),
                uv_axes: Vec4::new(t.matrix2.x_axis.x, t.matrix2.x_axis.y, t.matrix2.y_axis.x, t.matrix2.y_axis.y),
                uv_offset_flags: Vec4::new(t.translation.x, t.translation.y, f32::from(u8::from(source.base_color_texture.is_some())), f32::from(u8::from(layer))),
                anti_tiling: Vec4::ZERO,
            },
            diffuse: source.base_color_texture.clone(),
            layer,
            depth_bias: source.depth_bias,
        }
    }
}

impl Material for SurfaceCaptureMaterial {
    fn fragment_shader() -> ShaderRef { CAPTURE_SHADER.into() }
    fn alpha_mode(&self) -> AlphaMode { if self.layer { AlphaMode::Blend } else { AlphaMode::Opaque } }
    /// Orders layer copies as the near layers are ordered (the transparent sort).
    fn depth_bias(&self) -> f32 { self.depth_bias }
    fn enable_prepass() -> bool { false }
    fn enable_shadows() -> bool { false }
    fn specialize(_: &MaterialPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, key: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        // [design] The rasteriser gets only its own part of the bias, as NifSurface's
        // specialize gives the near layer (`raster_bias`).
        if let Some(depth_stencil) = descriptor.depth_stencil.as_mut() {
            depth_stencil.bias.constant = key.bind_group_data.raster_bias;
        }
        // [design] #2551: an anti-tiled source's capture reads its diffuse texture anti-tiled.
        if key.bind_group_data.anti_tiling {
            if let Some(fragment) = descriptor.fragment.as_mut() {
                fragment.shader_defs.push("TERRAIN_ANTI_TILING".into());
            }
        }
        Ok(())
    }
}

/// [design] One bake copy's material: a folded base with its own material, or the
/// single-texture capture of any other draw.
#[derive(Clone, Debug)]
pub enum CaptureDraw { Folded(Box<TerrainMaterial>), Single(SurfaceCaptureMaterial) }

/// [design] A folded base's bake copy: the unlit surface bake (terrain.wesl `SURFACE_BAKE`
/// returns the base colour only), no scorch, no fog. Round 10a review (second round):
/// without the normal maps, which that colour never reads, so a removed normal map (a
/// detail dependency, which masks and rebakes the cell) cannot keep the copy from being
/// prepared and the replacement's receipt from ever arriving. #3322: without the height maps
/// and parallax for the same reason; the bake keeps the unshifted surface.
fn bake_copy(material: &mut TerrainMaterial) {
    let e = &mut material.extension;
    e.surface_bake = true;
    e.burn = None;
    for normal in [&mut e.normal0, &mut e.normal1, &mut e.normal2, &mut e.normal3, &mut e.normal4, &mut e.normal5] { *normal = None; }
    for height in [&mut e.height0, &mut e.height1, &mut e.height2, &mut e.height3, &mut e.height4, &mut e.height5] { *height = None; }
    e.parallax = false;
    e.parallax_depths = [0.0; crate::terrain::FOLDED_LAYERS];
    e.alpha_heights = 0;
    e.height_base = None;
    e.base_depth = 0.0;
    material.base.fog_enabled = false;
    material.base.normal_map_texture = None;
}

/// [design] The bake root's transform: game space to Bevy's, moved to the publication's
/// south-west cell (`origin`), under which each slot `s` lies at `s` x 4096 game units.
pub fn bake_root(origin: (i32, i32)) -> Transform {
    game_to_bevy() * Transform::from_xyz(origin.0 as f32 * 4096.0, origin.1 as f32 * 4096.0, 0.0)
}

/// [design] A new reconstruction gets a new input revision, including stitched inputs.
pub fn capture_revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub enum CaptureSource { Folded(Handle<TerrainMaterial>), Single(Handle<nvf_assets::NifMaterial>, bool) }

/// [design] Round 10a: what one quadrant's near draw is made of, for direct sampling of
/// its layer textures at a mesh fragment (nif_surface_bindings.wesl `mesh_terrain_detail`):
/// the base draw's source material (a folded `TerrainMaterial` or the single base
/// material), the separately drawn overflow layers' materials in drawing order, and the
/// prepared vertex attributes of the very meshes drawn (terrain.rs `terrain_draws`):
/// linear vertex colour, normalized vertex normal and tangent (game space), and one
/// opacity grid per drawn overlay (the folded ones, then the overflow ones).
#[derive(Clone, Debug)]
pub struct QuadrantDetail {
    pub base: DetailBase,
    pub overflow: Vec<Handle<nvf_assets::NifMaterial>>,
    /// The base draw's texture coordinate is the place in the cell over the repeat (a
    /// six-layer folded draw), not the place in the world.
    pub cell_local: bool,
    /// World units per texture repeat the meshes were built for.
    pub tile: f32,
    pub tint: Vec<[f32; 3]>,
    pub normal: Vec<[f32; 3]>,
    pub tangent: Vec<[f32; 3]>,
    /// The tangents' handedness (terrain.rs `TANGENT_SIGN`, from the mesh drawn).
    pub tangent_sign: f32,
    pub opacity: Vec<Vec<f32>>,
    /// The textures and values read from the source materials, or why they cannot serve
    /// (a source material no longer matching the prepared layers, or an overflow layer the
    /// detail does not reproduce): the quadrant then keeps the colour cache with that
    /// reason, and a later material edit reads them again (`refresh_captures`).
    pub resolved: Result<ResolvedDetail, &'static str>,
}

#[derive(Clone, Debug)]
pub enum DetailBase { Folded(Handle<TerrainMaterial>), Single(Handle<nvf_assets::NifMaterial>) }

/// [design] The near draw's surface inputs: the base's diffuse and normal map, colour and
/// texture-coordinate transform (the base `StandardMaterial`, terrain.rs `surface`), and
/// each drawn overlay's diffuse and normal map, folded ones first.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedDetail {
    pub base_diffuse: Option<Handle<Image>>,
    pub base_normal: Option<Handle<Image>>,
    pub base_rgb: Vec3,
    pub uv: bevy::math::Affine2,
    pub overlays: Vec<(Handle<Image>, Option<Handle<Image>>)>,
    pub folded: usize,
    /// Normal maps are read with green pointing down the image (`flip_normal_map_y`).
    pub flip_y: bool,
    /// Height blending and anti-tiling as these materials have them now (#2550, #2551).
    pub features: nvf_assets::nif_surface_blend::DetailFeatures,
}

impl ResolvedDetail {
    /// From the base draw's material and the overflow layers' materials.
    /// `unresolved-materials` when an overlay has no diffuse texture (the near draw would
    /// not have drawn it), an overflow layer reads its normal map the other way up from the
    /// base, or the overlays do not match the `layers` opacity grids prepared.
    /// `overflow-material` (round 10a review) when an overflow layer's own material has a
    /// colour other than opaque white, or a texture-coordinate transform other than the
    /// base's: the detail composes overflow layers with white at full alpha through the
    /// base's transform (nif_surface_bindings.wesl `mesh_terrain_detail`), while the near
    /// draw uses each layer's own values. terrain.rs `layer_material` makes them white with
    /// the default transform, so only a later material edit (or a base transform edit)
    /// reaches this.
    ///
    /// [design] #2550/#2551: the features are read from the same materials, as their
    /// pipelines select them: a folded base's `TerrainLayersKey` (height blending, with its
    /// uniform's depth, gain and source; anti-tiling), a single base's and each overflow
    /// layer's `NifSurfaceKey::anti_tiling` (`base_surface`, the overflow materials'
    /// extensions). Every anti-tiled draw of the quadrant must share one texture repeat,
    /// since the detail reads them all at one lattice place (`anti-tiling-repeat`
    /// otherwise; terrain.rs gives them all `TerrainSettings::tile`).
    pub fn from_materials(base: &StandardMaterial, base_surface: Option<&nvf_assets::nif_material::NifSurface>, folded: Option<&crate::terrain::TerrainLayers>, overflow: &[&nvf_assets::NifMaterial], layers: usize) -> Result<Self, &'static str> {
        const UNRESOLVED: &str = "unresolved-materials";
        let mut overlays = Vec::new();
        let mut features = nvf_assets::nif_surface_blend::DetailFeatures::default();
        let mut repeats = Vec::new();
        if let Some(e) = folded {
            let pairs = [(&e.diffuse0, &e.normal0), (&e.diffuse1, &e.normal1), (&e.diffuse2, &e.normal2), (&e.diffuse3, &e.normal3), (&e.diffuse4, &e.normal4), (&e.diffuse5, &e.normal5)];
            for (d, n) in pairs.into_iter().take(e.count as usize) { overlays.push((d.clone().ok_or(UNRESOLVED)?, n.clone())); }
            let key = crate::terrain::TerrainLayersKey::from(e);
            features.height_blend = key.height_blend.then(|| crate::terrain::TerrainLayersUniform::from(e).height);
            if key.anti_tiling { features.anti_tiling = true; repeats.push(e.texture_repeat); }
        } else if let Some(s) = base_surface.filter(|s| nvf_assets::nif_material::NifSurfaceKey::from(*s).anti_tiling) {
            features.anti_tiling = true;
            repeats.push(s.terrain_repeat);
        }
        let folded = overlays.len();
        for (k, material) in overflow.iter().enumerate() {
            let m = &material.base;
            if m.flip_normal_map_y != base.flip_normal_map_y { return Err(UNRESOLVED); }
            if LinearRgba::from(m.base_color) != LinearRgba::WHITE || m.uv_transform != base.uv_transform { return Err("overflow-material"); }
            overlays.push((m.base_color_texture.clone().ok_or(UNRESOLVED)?, m.normal_map_texture.clone()));
            if nvf_assets::nif_material::NifSurfaceKey::from(&material.extension).anti_tiling {
                features.overflow_anti_tiling |= 1u32.checked_shl((folded + k) as u32).unwrap_or(0);
                repeats.push(material.extension.terrain_repeat);
            }
        }
        if overlays.len() != layers { return Err(UNRESOLVED); }
        if let Some(&repeat) = repeats.first() {
            if repeats.iter().any(|r| *r != repeat) { return Err("anti-tiling-repeat"); }
            features.repeat = repeat;
        }
        let c = LinearRgba::from(base.base_color);
        Ok(Self { base_diffuse: base.base_color_texture.clone(), base_normal: base.normal_map_texture.clone(), base_rgb: Vec3::new(c.red, c.green, c.blue), uv: base.uv_transform, overlays, folded, flip_y: base.flip_normal_map_y, features })
    }
    fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.base_diffuse.iter().chain(&self.base_normal).chain(self.overlays.iter().flat_map(|(d, n)| std::iter::once(d).chain(n)))
    }
}

/// [design] A quadrant's detail capture, or why it keeps the colour cache.
#[derive(Clone, Debug)]
pub enum DetailCapture { Layers(Box<QuadrantDetail>), Colour(&'static str) }
impl Default for DetailCapture { fn default() -> Self { Self::Colour("not-captured") } }

#[derive(Component)]
pub struct SurfaceCell {
    pub world: u32,
    pub grid: (i32, i32),
    /// #2149/#2661: the drawn surface's own heights (`Surface::heights`, `side()` squared,
    /// row-major, rows south to north); published only when that is the run's surface
    /// ([`SurfaceCache::layout`]).
    pub heights: Vec<f32>,
    pub quadrants: Vec<(Mesh, CaptureDraw)>,
    pub paths: [CapturePath; 4],
    pub content_revision: u64,
    pub sources: Vec<CaptureSource>,
    /// Diffuse dependencies recorded with the captured draw stack, never mesh hashes; with
    /// round 10a also every image a quadrant's detail reads (normal maps included).
    pub dependencies: HashSet<AssetId<Image>>,
    /// Round 10a: each quadrant's detail capture.
    pub detail: [DetailCapture; 4],
}

impl SurfaceCell {
    /// A quadrant's draws in their drawing order: its base first, then each blended layer.
    /// Each becomes a flat unlit copy; a layer copy keeps its alpha blending.
    pub fn capture(&mut self, quadrant: u8, draws: Vec<(Mesh, CaptureDraw)>, path: CapturePath) {
        for (mut flat, draw) in draws {
            if let Some(VertexAttributeValues::Float32x3(p)) = flat.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
                for v in p { v[2] = 0.0; }
            }
            let draw = match draw {
                CaptureDraw::Folded(mut material) => { bake_copy(&mut material); CaptureDraw::Folded(material) }
                single => single,
            };
            self.quadrants.push((flat, draw));
        }
        self.paths[quadrant as usize] = path;
        self.refresh_dependencies();
    }
    fn image_dependencies(&self) -> &HashSet<AssetId<Image>> { &self.dependencies }
    /// [design] The colour images the bake copies bind (each draw's diffuse; a folded copy's
    /// base diffuse and layer diffuses, its normal maps being stripped, [`bake_copy`]).
    fn colour_images(&self) -> impl Iterator<Item = AssetId<Image>> + '_ {
        self.quadrants.iter().flat_map(|(_, draw)| -> Vec<AssetId<Image>> {
            match draw {
                CaptureDraw::Single(m) => m.diffuse.iter().map(Handle::id).collect(),
                CaptureDraw::Folded(m) => {
                    let e = &m.extension;
                    [&m.base.base_color_texture, &e.diffuse0, &e.diffuse1, &e.diffuse2, &e.diffuse3, &e.diffuse4, &e.diffuse5].into_iter().flatten().map(Handle::id).collect()
                }
            }
        })
    }
    fn refresh_dependencies(&mut self) {
        let mut images: HashSet<_> = self.colour_images().collect();
        // [design] Round 10a: the detail's images too (normal maps, and through their
        // assets the samplers), so their edits and removals invalidate the cell.
        for d in &self.detail {
            if let DetailCapture::Layers(d) = d { images.extend(d.resolved.iter().flat_map(ResolvedDetail::images).map(Handle::id)); }
        }
        self.dependencies = images;
    }
    fn colour_mask(&self) -> u32 {
        self.paths.iter().enumerate().fold(0, |mask, (q, path)| mask | (u32::from(path.colour_valid()) << q))
    }
}

#[derive(Clone, Default)]
pub struct PublishedSurface {
    pub colour: Handle<Image>,
    pub height: Handle<Image>,
    pub origin: (i32, i32),
    pub cells: usize,
    pub resolution: u32,
    pub revision: u64,
    /// Enabled-only mask in this publication's original coordinates.
    pub coverage: u32,
    pub(crate) keys: Vec<CellKey>,
    /// #2149/#2661: how its height atlas is packed (every clone's shader follows it).
    pub layout: HeightLayout,
    /// Round 10a: the layer images and samplers its detail quadrants sample (empty without
    /// landscape detail), bound by every enabled clone of this publication.
    pub palette: nvf_assets::nif_surface_blend::TerrainPalette,
    /// The quadrants whose layer detail this publication supplies (grid, quadrant).
    pub detail: Vec<((i32, i32), u8)>,
    /// Which palette entries each admitted quadrant reads, for [`Self::live_palette`].
    pub(crate) palette_use: PaletteUse,
}

/// One admitted quadrant's cell, palette images read and the palette samplers it samples.
type QuadrantReads = ((i32, i32), Vec<u16>, Vec<u16>);
/// [design] Round 10a review: the publication's whole palette and which entries each
/// admitted quadrant reads (its images, the samplers it actually samples), with each
/// palette image's own sampler description as a palette sampler number (`None`: not one
/// of the palette's).
#[derive(Clone, Debug, Default)]
pub(crate) struct PaletteUse {
    full: nvf_assets::nif_surface_blend::TerrainPalette,
    quadrants: Vec<QuadrantReads>,
    own_sampler: Vec<Option<u16>>,
}

impl PublishedSurface {
    /// [design] Round 10a review: the palette its clones bind while `coverage` (this
    /// publication's coordinates) is enabled. Entries only masked cells' quadrants read
    /// become `None` (the fallback; their numbers stay reserved, so the height atlas's
    /// numbers keep their meaning), and a sampler still read takes as its representative
    /// an image a covered quadrant reads with that description. So a masked cell's images
    /// (e.g. one removed from `Assets<Image>`, which masks every cell depending on it, see
    /// `update_cache`) are never needed again: rebuilding a retained clone for a coverage
    /// update cannot wait for an image that is gone (Bevy drops the old prepared material
    /// before preparing the new one).
    ///
    /// [design] Round 10a review (second round): a sampler counts as read only by the
    /// quadrants that sample it ([`DetailPalette::usage`]; a textureless quadrant samples
    /// none), and a sampler without a live representative image binds the fallback, never
    /// the publication's original representative (which may be the removed image).
    pub fn live_palette(&self, coverage: u32) -> nvf_assets::nif_surface_blend::TerrainPalette {
        let u = &self.palette_use;
        let (mut images, mut samplers) = (vec![false; u.full.images.len()], vec![false; u.full.samplers.len()]);
        for (grid, read, sampled) in &u.quadrants {
            let slot = (grid.0 - self.origin.0, grid.1 - self.origin.1);
            if !(0..3).contains(&slot.0) || !(0..3).contains(&slot.1) || coverage & (1 << (slot.1 * 3 + slot.0)) == 0 { continue; }
            for &i in read { if let Some(b) = images.get_mut(i as usize) { *b = true; } }
            for &j in sampled { if let Some(b) = samplers.get_mut(j as usize) { *b = true; } }
        }
        let representative = |j: usize| u.own_sampler.iter().zip(&images).position(|(own, &live)| live && *own == Some(j as u16)).and_then(|i| u.full.images[i].clone());
        nvf_assets::nif_surface_blend::TerrainPalette {
            images: u.full.images.iter().zip(&images).map(|(h, &live)| h.clone().filter(|_| live)).collect(),
            samplers: samplers.iter().enumerate().map(|(j, &live)| if live { representative(j) } else { None }).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CellKey { grid: (i32, i32), entity: Entity, revision: u64, changed: u32, images: Vec<(AssetId<Image>, u64)> }

#[derive(Clone, Debug, PartialEq, Eq)]
struct Request { epoch: u64, origin: (i32, i32), cells: Vec<CellKey> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Receipt { epoch: u64, request: u64, destination: AssetId<Image>, source_revision: u64 }

/// A retained cell's level-0 rectangle corners in the previous and the new atlas.
#[derive(Clone)]
struct CellCopy { source: (u32, u32), destination: (u32, u32) }

/// [design] Round 10b: what the render world's assembly writes into a replacement's fresh
/// atlas (`destination`) once the bake camera has drawn every copy in one frame: the
/// retained `cells` copied from the previous publication (`source`) at every level, the
/// `baked` cells' level 0 (corners of their level-0 rectangles) copied from the
/// single-level bake target (`bake`), then those cells' lower levels filtered from their own
/// texels. A copy-only replacement has no bake target and nothing baked.
#[derive(Clone)]
struct Transfers {
    source: Option<Handle<Image>>,
    destination: Handle<Image>,
    cells: Vec<CellCopy>,
    bake: Option<Handle<Image>>,
    baked: Vec<(u32, u32)>,
    resolution: u32,
}

fn colour_rectangle(grid: (i32, i32), origin: (i32, i32), resolution: u32) -> (u32, u32) {
    ((grid.0 - origin.0) as u32 * resolution, (2 - (grid.1 - origin.1)) as u32 * resolution)
}
fn retained_mask(publication: &PublishedSurface, current: &[CellKey]) -> u32 {
    publication.keys.iter().filter(|k| current.contains(k)).fold(0, |mask, k| {
        mask | (1 << ((k.grid.1 - publication.origin.1) * 3 + k.grid.0 - publication.origin.0))
    })
}

struct BakeJob {
    root: Entity,
    publication: PublishedSurface,
    report: String,
    receipt: Receipt,
    copied: usize,
    baked: usize,
}

#[derive(Resource)]
pub struct SurfaceCache {
    pub generation: u64,
    signature: Option<Request>,
    context: Option<(u32, bool, u32, usize)>,
    selected_centre: Option<(i32, i32)>,
    captured_inputs: HashMap<Entity, u64>,
    stitched_dirty: HashMap<Entity, (i32, i32)>,
    epoch: u64,
    revision: u64,
    current: Vec<CellKey>,
    image_revisions: HashMap<AssetId<Image>, u64>,
    /// Round 10a review (second round): dependencies removed from the image assets and not
    /// added back, so a cell whose bake copies would bind one is planned without it.
    removed_images: HashSet<AssetId<Image>>,
    /// Round 10a review (third round): detail images the last planned replacement found
    /// not in the image assets (its quadrants refused, `image-not-loaded`). With
    /// `removed_images`, the images whose return advances their dependency revision.
    awaited_images: HashSet<AssetId<Image>>,
    image_events: bevy::ecs::message::MessageCursor<AssetEvent<Image>>,
    /// Cancelled render extraction gets three frames to retire before another allocation.
    retirement: u8,
    job: Option<BakeJob>,
    ready: Option<Receipt>,
    submitted: Arc<Mutex<Option<Receipt>>>,
    pub published: Option<PublishedSurface>,
    /// Colour texels per cell side (the blend's resolution).
    pub resolution: u32,
    /// The device cannot hold the requested atlas ([`check_resolution`]): nothing bakes.
    pub rejected: bool,
    /// Round 10a: publications carry landscape detail (the blend material is bindless on
    /// this device, `mesh_terrain_blend.rs::detail_capability`); otherwise colour only.
    pub detail: bool,
    /// #2149/#2661: the height atlas's layout, the run's landscape surface's
    /// ([`SurfaceCache::start`]); a cell whose heights are not that surface's is refused.
    pub layout: HeightLayout,
    /// Round 10a review: frames in a row the front's materials have not settled.
    unsettled_frames: u32,
}
/// [design] Round 10a review: the longest the replacement guard waits for the front's
/// materials to settle (frames in a row) before it starts the replacement anyway. Far
/// above every settlement a working publication needs, including the 120-frame
/// readiness delay aid and the crossing aid's held readiness (seconds at most).
const SETTLE_FRAMES: u32 = 1800;
impl Default for SurfaceCache {
    fn default() -> Self { Self::with_resolution(DEFAULT_RESOLUTION) }
}
impl SurfaceCache {
    fn ensure_context(&mut self, commands: &mut Commands, world: &WorldData, streamer: &Streamer) {
        let context = (world.world, streamer.interior.is_some(), self.resolution, std::ptr::from_ref(world.lo) as usize);
        if self.context == Some(context) { return; }
        let retire = self.job.is_some() || self.published.is_some() || self.retirement > 0;
        self.release(commands);
        self.retirement = if retire { 3 } else { 0 };
        self.context = Some(context);
    }
    pub fn coverage_for(&self, publication: &PublishedSurface) -> u32 {
        if self.context.is_none() { publication.coverage } else { retained_mask(publication, &self.current) }
    }
    pub fn with_resolution(resolution: u32) -> Self {
        Self { generation: 0, signature: None, context: None, selected_centre: None, captured_inputs: default(), stitched_dirty: default(), epoch: 0, revision: 0, current: vec![], image_revisions: default(), removed_images: default(), awaited_images: default(), image_events: default(), retirement: 0, job: None, ready: None, submitted: default(), published: None, resolution, rejected: false, detail: false, layout: HeightLayout::STORED, unsettled_frames: 0 }
    }

    /// [design] Select independently of the immutable front, including while a job waits.
    /// The inclusive 256-unit margin suppresses boundary jitter on each axis separately.
    fn selected_origin(&mut self, point: Vec2) -> (i32, i32) {
        let cell = fnv_world::cell_of(point.x, point.y);
        let centre = self.selected_centre.get_or_insert(cell);
        for (coordinate, position, next) in [(&mut centre.0, point.x, cell.0), (&mut centre.1, point.y, cell.1)] {
            if position < *coordinate as f32 * 4096.0 - 256.0 || position > (*coordinate as f32 + 1.0) * 4096.0 + 256.0 {
                *coordinate = next;
            }
        }
        (centre.0 - 1, centre.1 - 1)
    }

    fn track_stitched_inputs(&mut self, captures: impl Iterator<Item = CapturedInput>) {
        let captures: Vec<_> = captures.collect();
        let loaded: HashMap<_, _> = captures.iter().map(|&(grid, entity, _)| (entity, grid)).collect();
        self.captured_inputs.retain(|e, _| loaded.contains_key(e));
        self.stitched_dirty.retain(|e, grid| loaded.get(e) == Some(grid));
        let affected = stitched_dependents(&mut self.captured_inputs, captures.into_iter());
        self.stitched_dirty.extend(loaded.into_iter().filter(|(_, grid)| affected.contains(grid)));
    }

    fn refreshed_capture(&mut self, entity: Entity, old: Option<&SurfaceCell>, mut surface: SurfaceCell) -> SurfaceCell {
        if let Some(old) = old { surface.content_revision = old.content_revision; }
        self.stitched_dirty.remove(&entity);
        surface
    }

    /// [design] The blend turned on: the atlas at `resolution` (its level 0; round 10b: the
    /// line also gives the whole mip chain's size and level count), checked against the
    /// device's 2D texture limit (`None` without a render device: not checked, not
    /// logged). An unsupported request is rejected in the log and bakes nothing, so no
    /// material samples it. Nothing is allocated here; the first bake allocates.
    /// #2149/#2661: `layout` is the height atlas's, from the run's landscape surface (fixed
    /// for the run before the first cell; nvf_play `smooth_terrain::spec`). The height atlas
    /// is checked against the same limit ([`height_atlas_supported`]: 2304 texels wide at
    /// 4 x 4) and refused the same way.
    pub fn start(&mut self, resolution: u32, max_texture_dimension_2d: Option<u32>, detail: bool, layout: HeightLayout) {
        self.resolution = resolution;
        self.rejected = false;
        self.detail = detail;
        self.layout = layout;
        let Some(max) = max_texture_dimension_2d else { return };
        let width = atlas_width(resolution);
        if resolution_supported(resolution, max) {
            info!("mesh terrain blend: colour atlas resolution={resolution} atlas={width}x{width} rgba16f colour-mib={} levels={} max-texture-dimension-2d={max}", mib(colour_bytes(resolution)), colour_levels(resolution));
        } else {
            warn!("mesh terrain blend: rejected resolution={resolution}: atlas {width} texels wide exceeds max-texture-dimension-2d={max}; no colour cache is baked");
            self.rejected = true;
        }
        if !height_atlas_supported(layout, detail, max) {
            warn!("mesh terrain blend: rejected height atlas subdiv={} atlas={}x{}: exceeds max-texture-dimension-2d={max}; no colour cache is baked", layout.sub(), layout.width(), height_atlas_rows(layout, detail));
            self.rejected = true;
        }
    }

    /// [design] The blend turned off: the pending bake's copies and camera despawned, the
    /// published atlas dropped (its images go with their last handle, the enabled clones'
    /// included), and a new generation, so a receipt of the old bake can never publish.
    /// The generation counter keeps counting across switches.
    pub fn release(&mut self, commands: &mut Commands) {
        if let Some(job) = self.job.take() { commands.entity(job.root).despawn(); }
        self.generation += 1;
        self.epoch += 1;
        self.revision += 1;
        self.context = None;
        self.selected_centre = None;
        self.captured_inputs.clear();
        self.stitched_dirty.clear();
        self.current.clear();
        self.retirement = 3;
        self.signature = None;
        self.ready = None;
        self.published = None;
        self.unsettled_frames = 0;
        *self.submitted.lock().unwrap() = None;
    }
}

// [design] One production height representation: the coefficients of both terrain
// triangles, encoded losslessly. WGSL only selects the triangle and evaluates its plane.
// #2149/#2661: the triangles are the drawn surface's (`layout`'s squares, `HeightLayout`),
// the slopes per surface square.
pub fn height_planes(a: f32, east: f32, north: f32, ne: f32) -> [[f32; 3]; 2] {
    [[a, east - a, ne - east], [a, ne - north, north - a]]
}
pub fn height_index(layout: HeightLayout, slot: (i32, i32), square: (usize, usize), triangle: usize, coefficient: usize) -> usize {
    let x = slot.0 as usize * layout.tile_width() + square.0 * 6 + triangle * 3 + coefficient;
    let y = slot.1 as usize * layout.tile_height() + square.1;
    (y * layout.width() + x) * 4
}
/// The byte index of texel `texel` of `slot`'s metadata row, the row after its squares
/// (0 the residency mask, 1 the colour mask, 2 the detail mask).
fn metadata_index(layout: HeightLayout, slot: (i32, i32), texel: usize) -> usize {
    ((slot.1 as usize * layout.tile_height() + layout.grid()) * layout.width() + slot.0 as usize * layout.tile_width() + texel) * 4
}
fn put_metadata(layout: HeightLayout, data: &mut [u8], slot: (i32, i32), texel: usize, value: u32) {
    let at = metadata_index(layout, slot, texel);
    data[at..at + 4].copy_from_slice(&(value as f32).to_le_bytes());
}
fn metadata(layout: HeightLayout, data: &[u8], slot: (i32, i32), texel: usize) -> u32 {
    let at = metadata_index(layout, slot, texel);
    f32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as u32
}
pub fn residency_mask(slot: (i32, i32), resident: &HashSet<(i32, i32)>) -> u32 {
    let mut mask = 0;
    for y in -1..=1 {
        for x in -1..=1 {
            if resident.contains(&(slot.0 + x, slot.1 + y)) { mask |= 1 << ((y + 1) * 3 + x + 1); }
        }
    }
    mask
}
fn encode_residency(layout: HeightLayout, data: &mut [u8], slot: (i32, i32), resident: &HashSet<(i32, i32)>) {
    put_metadata(layout, data, slot, 0, residency_mask(slot, resident));
}
fn encode_colour_mask(layout: HeightLayout, data: &mut [u8], slot: (i32, i32), mask: u32) {
    put_metadata(layout, data, slot, 1, mask);
}

#[derive(Debug, PartialEq, Eq)]
pub enum LookupStatus { OutOfRange, InvalidHeight, RejectedFootprint, InvalidColourQuadrant, Success }

fn colour_mask(layout: HeightLayout, data: &[u8], slot: (i32, i32)) -> u32 {
    metadata(layout, data, slot, 1)
}
fn colour_valid(layout: HeightLayout, data: &[u8], slot: (i32, i32), quadrant: u32) -> bool {
    colour_mask(layout, data, slot) & (1 << quadrant) != 0
}
/// [design] #2149/#2661, the shader's `terrain_square` of the ground: the surface square of
/// `layout` under `local` (LAND squares from the cell's south-west corner, the shader's
/// `local`), the fraction within it and the triangle (1 above the square's south-west to
/// north-east diagonal).
pub fn ground_square(layout: HeightLayout, local: Vec2) -> (UVec2, Vec2, usize) {
    let fine = local * layout.sub() as f32;
    let square = fine.floor().as_uvec2().min(UVec2::splat(layout.grid() as u32 - 1));
    let f = fine - square.as_vec2();
    (square, f, usize::from(f.y > f.x))
}
/// [design] The shader's ground under game-space `point` (`mesh_terrain_lookup`): the
/// fraction within the ground square and its triangle's plane coefficients `a`, `dx`, `dy`;
/// `None` outside the atlas or without heights there.
fn ground_plane(layout: HeightLayout, data: &[u8], origin: (i32, i32), point: Vec2) -> Option<(Vec2, [f32; 3])> {
    let cell = fnv_world::cell_of(point.x, point.y);
    let slot = (cell.0 - origin.0, cell.1 - origin.1);
    if !(0..3).contains(&slot.0) || !(0..3).contains(&slot.1) { return None; }
    let local = point / fnv_world::land::SPACING - Vec2::new(cell.0 as f32, cell.1 as f32) * GRID as f32;
    let (square, f, triangle) = ground_square(layout, local);
    let coefficient = |c| {
        let at = height_index(layout, slot, (square.x as usize, square.y as usize), triangle, c);
        f32::from_le_bytes(data[at..at + 4].try_into().unwrap())
    };
    (coefficient(0) > -1e30).then(|| (f, [coefficient(0), coefficient(1), coefficient(2)]))
}
/// [design] The ground height the shader measures against at `point` (`a + f.x dx + f.y
/// dy` of [`ground_plane`]); `None` without heights. The tests' twin of the shader's read.
#[cfg(test)]
pub fn probe_height(layout: HeightLayout, data: &[u8], origin: (i32, i32), point: Vec2) -> Option<f32> {
    ground_plane(layout, data, origin, point).map(|(f, [a, dx, dy])| a + f.x * dx + f.y * dy)
}
// [design] CPU publication probe, with the same precedence/footprint as the shader.
// This classifies uploaded bytes; it does not assert that GPU bindings are correct.
pub fn lookup_status(layout: HeightLayout, data: &[u8], origin: (i32, i32), point: Vec2, resolution: u32) -> LookupStatus {
    lookup_covered_status(layout, data, origin, point, resolution, 0x1ff)
}
fn covered(slot: (i32, i32), coverage: u32) -> bool {
    (0..3).contains(&slot.0) && (0..3).contains(&slot.1) && coverage & (1 << (slot.1 * 3 + slot.0)) != 0
}
/// The lookup of a fragment whose colour read takes level 0 alone (a level of detail of 0
/// or less: a close or face-on view), as [`lookup_level_status`].
pub fn lookup_covered_status(layout: HeightLayout, data: &[u8], origin: (i32, i32), point: Vec2, resolution: u32, coverage: u32) -> LookupStatus {
    lookup_level_status(layout, data, origin, point, resolution, coverage, 0.0)
}
/// [design] Round 10b: the lookup of a fragment whose colour read has level of detail `lod`
/// (the shader's `terrain_colour_lod`). The read takes the levels around it
/// ([`colour_read_levels`]), each the bilinear footprint at the fragment's position across
/// valid neighbouring cells ([`colour_read_taps`], round 10b review), so the colour footprint
/// is valid when every own-cell tap either level with a share reads lies in a captured
/// quadrant; neighbouring taps that are not covered and captured get no weight. The cell's
/// own live coverage is part of the height check before it.
pub fn lookup_level_status(layout: HeightLayout, data: &[u8], origin: (i32, i32), point: Vec2, resolution: u32, coverage: u32, lod: f32) -> LookupStatus {
    let cell = fnv_world::cell_of(point.x, point.y);
    let slot = (cell.0 - origin.0, cell.1 - origin.1);
    if !(0..3).contains(&slot.0) || !(0..3).contains(&slot.1) { return LookupStatus::OutOfRange; }
    let local = point / 4096.0 - Vec2::new(cell.0 as f32, cell.1 as f32);
    let square = (local * layout.grid() as f32).floor().as_uvec2().min(UVec2::splat(layout.grid() as u32 - 1));
    let at = height_index(layout, slot, (square.x as usize, square.y as usize), 0, 0);
    if !covered(slot, coverage) || f32::from_le_bytes(data[at..at + 4].try_into().unwrap()) < -1e30 { return LookupStatus::InvalidHeight; }
    let quadrant = u32::from(local.x >= 0.5) + 2 * u32::from(local.y >= 0.5);
    if !colour_valid(layout, data, slot, quadrant) { return LookupStatus::InvalidColourQuadrant; }
    let atlas = (point / 4096.0 - Vec2::new(origin.0 as f32, origin.1 as f32)) / 3.0;
    let uv = Vec2::new(atlas.x, 1.0 - atlas.y);
    let (low, high, share) = colour_read_levels(resolution, lod);
    let valid = colour_read_taps(layout, data, coverage, slot, uv, resolution, low).is_some()
        && (share <= 0.0 || high == low || colour_read_taps(layout, data, coverage, slot, uv, resolution, high).is_some());
    if valid { LookupStatus::Success } else { LookupStatus::RejectedFootprint }
}
/// [design] Round 10b: the levels a colour read at level of detail `lod` takes, as the
/// shader's `terrain_colour_mips`: `lod` clamped to the atlas's levels, the level below it,
/// the one above (the last level stays the last) and the upper level's share.
pub fn colour_read_levels(resolution: u32, lod: f32) -> (u32, u32, f32) {
    let last = colour_levels(resolution) - 1;
    let lod = lod.clamp(0.0, last as f32);
    let low = lod.floor() as u32;
    (low, (low + 1).min(last), lod - low as f32)
}
/// [design] Round 10b review, as the shader's `terrain_colour_taps`: at a level with `size`
/// texels per cell side, the bilinear footprint of the atlas coordinate `uv`: its first
/// (north-west) tap in that level's atlas texels (the atlas's rows run north to south, the
/// bake camera's reversed Y) and the fraction towards the next column and row. Not clamped:
/// it crosses cell edges into the neighbour's adjacent texels.
pub fn colour_read_footprint(uv: Vec2, size: u32) -> (IVec2, Vec2) {
    let p = uv * 3.0 * size as f32 - Vec2::splat(0.5);
    let first = p.floor();
    (first.as_ivec2(), p - first)
}
/// The cell slot of the atlas texel `tap` at a level with `size` texels per cell side;
/// `None` outside the atlas.
fn colour_tap_slot(tap: IVec2, size: u32) -> Option<(i32, i32)> {
    let size = size as i32;
    if tap.cmplt(IVec2::ZERO).any() || tap.cmpge(IVec2::splat(3 * size)).any() { return None; }
    Some((tap.x / size, 2 - tap.y / size))
}
/// [design] Round 10b review, as the shader's `terrain_colour_tap_valid`: whether the texel
/// `tap` can be read: inside the atlas, its cell live-covered and the texel in a captured
/// quadrant of that cell. A texel of a level with two or more texels per cell side covers
/// level-0 texels of one quadrant only (each level halves whole quadrants), the quadrant its
/// own position names; the last level's one texel covers all four, so it needs all four.
pub fn colour_tap_valid(layout: HeightLayout, data: &[u8], coverage: u32, tap: IVec2, size: u32) -> bool {
    let Some(slot) = colour_tap_slot(tap, size) else { return false };
    if !covered(slot, coverage) { return false; }
    let mask = colour_mask(layout, data, slot);
    if size <= 1 { return mask & 0xf == 0xf; }
    let t = tap - IVec2::new(slot.0, 2 - slot.1) * size as i32;
    let half = size as i32 / 2;
    mask & (1 << (u32::from(t.x >= half) + 2 * u32::from(t.y < half))) != 0
}
/// One level's bilinear read: each tap's atlas texel and its weight, in the order (0, 0),
/// (1, 0), (0, 1), (1, 1) from the first tap.
pub type ColourTaps = [(IVec2, f32); 4];
/// [design] Round 10b review, as the shader's `terrain_colour_taps` and
/// `terrain_colour_level`: the read of cell `slot`'s fragment at `level`. Each level is
/// filtered per cell, but the 3 by 3 atlas puts neighbouring cells' texels side by side, so
/// the read is the plain bilinear footprint ([`colour_read_footprint`]), crossing a cell edge
/// into the neighbour's adjacent texels. A tap that cannot be read ([`colour_tap_valid`])
/// gets weight 0 and the others are renormalized; where a whole neighbouring column or row
/// is missing that is the read clamped to the fragment's own cell, and at the last level
/// (texels at the cells' centres) it interpolates between the centres of valid neighbours.
/// The weights depend on the position and the taps' validity only, not on which cell the
/// fragment is in, so both sides of an edge between valid cells read the same value. `None`
/// (the footprint rejected) when no tap is the own cell's or one of the own cell's taps
/// lies in an uncaptured quadrant.
pub fn colour_read_taps(layout: HeightLayout, data: &[u8], coverage: u32, slot: (i32, i32), uv: Vec2, resolution: u32, level: u32) -> Option<ColourTaps> {
    let size = resolution >> level;
    let (first, f) = colour_read_footprint(uv, size);
    let mut taps = [(IVec2::ZERO, 0.0); 4];
    let mut own = false;
    let along = |share: f32, next: bool| if next { share } else { 1.0 - share };
    for (i, tap) in taps.iter_mut().enumerate() {
        let offset = IVec2::new(i as i32 % 2, i as i32 / 2);
        let texel = first + offset;
        let valid = colour_tap_valid(layout, data, coverage, texel, size);
        if colour_tap_slot(texel, size) == Some(slot) {
            if !valid { return None; }
            own = true;
        }
        let weight = along(f.x, offset.x == 1) * along(f.y, offset.y == 1);
        *tap = (texel, if valid { weight } else { 0.0 });
    }
    let total: f32 = taps.iter().map(|t| t.1).sum();
    if !own || total <= 0.0 { return None; }
    for tap in &mut taps { tap.1 /= total; }
    Some(taps)
}

/// [design] Round 10a's end of the publication line: whether the publication supplies
/// layer detail, the palette's use, the admitted quadrants' count and each quadrant that
/// keeps the colour cache with its reason (a publication can be mixed). Without the
/// capability: ` detail=colour detail-reason=capability`.
fn detail_log(detail: Option<&DetailPalette>) -> String {
    let Some(d) = detail else { return " detail=colour detail-reason=capability".into() };
    let refused: Vec<_> = d.refused.iter().map(|((g, q), r)| format!("({}, {}):q{q}:{r}", g.0, g.1)).collect();
    format!(" detail={} palette-images={} palette-samplers={} detail-quadrants={} colour-quadrants=[{}]",
        if d.admitted.is_empty() { "colour" } else { "layers" }, d.palette.images.len(), d.palette.samplers.len(), d.admitted.len(), refused.join(";"))
}
fn publication_log(generation: u64, origin: (i32, i32), resident: &[String], excluded: &[String], probe: &str, assets: (AssetId<Image>, AssetId<Image>), resolution: u32) -> String {
    format!("mesh terrain blend: publication generation={generation} origin=({}, {}) resident=[{}] excluded=[{}] {probe} colour={:?} height={:?} resolution={resolution}", origin.0, origin.1, resident.join(";"), excluded.join(";"), assets.0, assets.1)
}
/// [design] The ground triangle's game-space normal under `point`, from the uploaded
/// coefficients (the `normal` view's colour there is 0.5 + 0.5 n); `None` without heights.
pub fn probe_normal(layout: HeightLayout, data: &[u8], origin: (i32, i32), point: Vec2) -> Option<Vec3> {
    ground_plane(layout, data, origin, point).map(|(_, [_, dx, dy])| nvf_assets::nif_surface_blend::terrain_game_normal(layout, dx, dy))
}
fn publication_probe(layout: HeightLayout, data: &[u8], origin: (i32, i32), point: Vec2, resolution: u32) -> String {
    let cell = fnv_world::cell_of(point.x, point.y);
    let slot = (cell.0 - origin.0, cell.1 - origin.1);
    let local = point / fnv_world::land::SPACING - Vec2::new(cell.0 as f32, cell.1 as f32) * GRID as f32;
    // The ground's square and triangle (surface squares, `layout.grid()` a side).
    let (square, _, triangle) = ground_square(layout, local);
    let at = height_index(layout, slot, (square.x as usize, square.y as usize), triangle, 0);
    let normal = probe_normal(layout, data, origin, point).map_or("none".to_string(), |n| format!("({:.3}, {:.3}, {:.3})", n.x + 0.0, n.y + 0.0, n.z + 0.0));
    // [design] Round 10a: the probe cell's detail mask (the colour classification above is
    // the colour cache's; the layers are checked separately, as in the shader).
    let detail = if (0..3).contains(&slot.0) && (0..3).contains(&slot.1) { detail_mask(layout, data, slot) } else { 0 };
    format!("probe-cell={cell:?} probe-slot={slot:?} probe-game-xy={point:?} probe-square={square:?} probe-triangle={triangle} probe-height-bytes={:?} probe-status={:?} probe-normal={normal} probe-detail-mask={detail:#x}", &data[at..at + 4], lookup_status(layout, data, origin, point, resolution))
}
/// [design] #2149/#2661: a cell's surface `heights` (`layout.side()` squared, row-major,
/// rows south to north: `Surface::heights`) as both triangles' planes of each surface square
/// (the diagonal `grid_triangles` draws), in `slot`'s tile: one pass over the squares,
/// each tile row's planes written in place.
pub fn encode_height_cell(layout: HeightLayout, data: &mut [u8], slot: (i32, i32), heights: &[f32]) {
    let (grid, side) = (layout.grid(), layout.side());
    assert_eq!(heights.len(), side * side, "a cell's heights are its surface's ({side} x {side})");
    for y in 0..grid {
        let at = height_index(layout, slot, (0, y), 0, 0);
        let (south, north) = (&heights[y * side..(y + 1) * side], &heights[(y + 1) * side..(y + 2) * side]);
        for (x, out) in data[at..at + grid * 24].as_chunks_mut::<24>().0.iter_mut().enumerate() {
            let planes = height_planes(south[x], south[x + 1], north[x], north[x + 1]);
            for (texel, value) in out.as_chunks_mut::<4>().0.iter_mut().zip(planes.as_flattened()) {
                texel.copy_from_slice(&value.to_le_bytes());
            }
        }
    }
}
fn encode_surface_cell(layout: HeightLayout, data: &mut [u8], slot: (i32, i32), cell: &SurfaceCell, resident: &HashSet<(i32, i32)>) {
    encode_height_cell(layout, data, slot, &cell.heights);
    encode_residency(layout, data, slot, resident);
    encode_colour_mask(layout, data, slot, cell.colour_mask());
    encode_detail_mask(layout, data, slot, 0);
}
#[cfg(test)]
fn empty_height_atlas() -> Vec<u8> { empty_height_atlas_rows(HeightLayout::STORED, HeightLayout::STORED.rows()) }
/// [design] The height atlas of `layout` with `rows` rows: the heights' and, with landscape
/// detail, the detail words' after them (zero: no quadrant entry is valid).
fn empty_height_atlas_rows(layout: HeightLayout, rows: usize) -> Vec<u8> {
    let mut heights = vec![0; layout.width() * rows * 4];
    for h in heights[..layout.width() * layout.rows() * 4].as_chunks_mut::<4>().0 { h.copy_from_slice(&INVALID_HEIGHT.to_le_bytes()); }
    heights
}

// [design] Round 10a: a cell's four-bit mask of the quadrants whose layer detail this
// publication supplies, beside the residency and colour masks (texel 2 of the metadata row).
fn encode_detail_mask(layout: HeightLayout, data: &mut [u8], slot: (i32, i32), mask: u32) {
    put_metadata(layout, data, slot, 2, mask);
}
pub fn detail_mask(layout: HeightLayout, data: &[u8], slot: (i32, i32)) -> u32 {
    metadata(layout, data, slot, 2)
}
/// Detail word `word` sits after the layout's height rows (nif_surface_blend.rs notes).
fn put_word(layout: HeightLayout, data: &mut [u8], word: usize, value: u32) {
    let at = (layout.rows() * layout.width() + word) * 4;
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
#[cfg(test)]
pub fn detail_word(layout: HeightLayout, data: &[u8], word: usize) -> u32 {
    let at = (layout.rows() * layout.width() + word) * 4;
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
/// The detail table's entry number of quadrant `q` of `slot` (slot-major, as the shader).
pub fn detail_entry(slot: (i32, i32), q: u8) -> usize { (slot.1 as usize * 3 + slot.0 as usize) * 4 + q as usize }
/// [design] One admitted quadrant's table entry and vertex records.
pub(crate) fn encode_detail(layout: HeightLayout, data: &mut [u8], entry: usize, quadrant: &nvf_assets::nif_surface_blend::DetailQuadrant, detail: &QuadrantDetail) {
    use nvf_assets::nif_surface_blend::{detail_quadrant_word, detail_vertex_word, detail_vertex_words, QUAD_VERTICES};
    for (i, w) in quadrant.words().into_iter().enumerate() { put_word(layout, data, detail_quadrant_word(entry) + i, w); }
    let mut opacity = Vec::with_capacity(detail.opacity.len());
    for v in 0..QUAD_VERTICES {
        opacity.clear();
        opacity.extend(detail.opacity.iter().map(|grid| grid[v]));
        let words = detail_vertex_words(detail.tint[v], detail.normal[v], detail.tangent[v], &opacity);
        for (i, w) in words.into_iter().enumerate() { put_word(layout, data, detail_vertex_word(entry, v) + i, w); }
    }
}

/// [design] StandardMaterial's two-component normal map rule (vendored bevy_pbr
/// `pbr_material.rs`, `as_bind_group_shader_type`): the image's format is a two-channel one.
fn two_component(image: &Image) -> bool {
    matches!(image.texture_descriptor.format, TextureFormat::Rg8Unorm | TextureFormat::Rg16Unorm | TextureFormat::Bc5RgUnorm | TextureFormat::EacRg11Unorm
        | TextureFormat::Astc { channel: wgpu::AstcChannel::Unorm, .. })
}

/// [design] Round 10a review: whether the detail's samplers are the ones the near draw
/// reads with. The detail reads every diffuse and overlay normal map with one sampler, the
/// base diffuse image's (the first overlay's without a base texture), and the base normal
/// map with its own. The near draw reads the folded overlays with the base's diffuse
/// sampler too (terrain.wesl), but a textureless base binds Bevy's fallback image and so
/// its fallback sampler, which need not match any layer image's (with anisotropic
/// filtering off the configured fallback is 1x while texture.rs gives landscape textures
/// their own description), and each separately drawn overflow layer reads its diffuse and
/// normal map with those images' own samplers. So a quadrant whose folded overlays sit on
/// a textureless base, or whose overflow layer has a diffuse or normal image with a
/// description other than the detail's diffuse one, is not reproduced (`sampler-mismatch`,
/// the colour cache). The palette format stays as it is (no per-layer samplers). Every
/// image is loaded (checked first).
fn samplers_reproduced(r: &ResolvedDetail, images: &Assets<Image>) -> bool {
    if r.base_diffuse.is_none() && r.folded > 0 { return false; }
    let sampler = |h: &Handle<Image>| images.get(h).map(|i| &i.sampler);
    let Some(diffuse) = r.base_diffuse.as_ref().or(r.overlays.first().map(|(d, _)| d)).and_then(sampler) else { return true };
    r.overlays.iter().skip(r.folded).all(|(d, n)| sampler(d) == Some(diffuse) && n.as_ref().is_none_or(|n| sampler(n) == Some(diffuse)))
}

/// [design] Round 10a: one publication's palette ([`PALETTE_IMAGES`] images and
/// [`PALETTE_SAMPLERS`] sampler descriptions at most) and the quadrants it admits, in a
/// deterministic order: quadrants the previous publication admitted for unchanged cells
/// first, then the rest by grid and quadrant. A quadrant is admitted whole or keeps the
/// colour cache, with the reason. Samplers are shared by description: a palette sampler
/// stands for every image whose `ImageSampler` equals its representative's (texture.rs
/// gives landscape textures one description).
/// A quadrant of a cell: its grid and quadrant number.
pub type QuadrantAt = ((i32, i32), u8);
#[derive(Default)]
pub struct DetailPalette {
    pub palette: nvf_assets::nif_surface_blend::TerrainPalette,
    sampler_keys: Vec<bevy::image::ImageSampler>,
    pub admitted: Vec<(QuadrantAt, nvf_assets::nif_surface_blend::DetailQuadrant)>,
    pub refused: Vec<(QuadrantAt, &'static str)>,
}
use nvf_assets::nif_surface_blend::{PALETTE_IMAGES, PALETTE_SAMPLERS};
impl DetailPalette {
    pub fn assemble<'a>(cells: impl IntoIterator<Item = &'a SurfaceCell>, images: &Assets<Image>, retained: &[((i32, i32), u8)]) -> Self {
        let mut order: Vec<(&SurfaceCell, u8)> = cells.into_iter().flat_map(|c| (0..4u8).map(move |q| (c, q))).collect();
        order.sort_by_key(|(c, q)| (!retained.contains(&(c.grid, *q)), c.grid, *q));
        let mut out = Self::default();
        for (cell, q) in order {
            match &cell.detail[q as usize] {
                DetailCapture::Colour(reason) => out.refused.push(((cell.grid, q), reason)),
                DetailCapture::Layers(d) => match out.admit(d, images) {
                    Ok(quadrant) => out.admitted.push(((cell.grid, q), quadrant)),
                    Err(reason) => out.refused.push(((cell.grid, q), reason)),
                },
            }
        }
        out
    }
    /// [design] Round 10a review: which entries each admitted quadrant reads and each
    /// image's own sampler description, for [`PublishedSurface::live_palette`]. A quadrant
    /// samples its diffuse sampler only with a base diffuse or an overlay, its normal
    /// sampler only with a base normal map (nif_surface_bindings.wesl
    /// `mesh_terrain_detail`); a textureless quadrant samples none, so the zero its table
    /// words hold for both keeps no sampler alive.
    pub(crate) fn usage(&self, images: &Assets<Image>) -> PaletteUse {
        let quadrants = self.admitted.iter().map(|((grid, _), q)| {
            let read = q.base_diffuse.iter().chain(&q.base_normal).copied().chain(q.overlays.iter().flat_map(|(d, n, _)| std::iter::once(*d).chain(*n))).collect();
            let diffuse = (q.base_diffuse.is_some() || !q.overlays.is_empty()).then_some(q.diffuse_sampler);
            let sampled = diffuse.into_iter().chain(q.base_normal.map(|_| q.normal_sampler)).collect();
            (*grid, read, sampled)
        }).collect();
        let own_sampler = self.palette.images.iter().map(|h| {
            let image = images.get(h.as_ref()?)?;
            self.sampler_keys.iter().position(|k| *k == image.sampler).map(|j| j as u16)
        }).collect();
        PaletteUse { full: self.palette.clone(), quadrants, own_sampler }
    }
    fn image(&self, h: &Handle<Image>, tentative: &mut Vec<Handle<Image>>) -> u16 {
        let at = self.palette.images.iter().map(Option::as_ref).chain(tentative.iter().map(Some)).position(|i| i == Some(h)).unwrap_or_else(|| { tentative.push(h.clone()); self.palette.images.len() + tentative.len() - 1 });
        at as u16
    }
    fn sampler(&self, image: &Image, h: &Handle<Image>, tentative: &mut Vec<(Handle<Image>, bevy::image::ImageSampler)>) -> u16 {
        let at = self.sampler_keys.iter().chain(tentative.iter().map(|(_, k)| k)).position(|k| *k == image.sampler)
            .unwrap_or_else(|| { tentative.push((h.clone(), image.sampler.clone())); self.sampler_keys.len() + tentative.len() - 1 });
        at as u16
    }
    fn admit(&mut self, d: &QuadrantDetail, images: &Assets<Image>) -> Result<nvf_assets::nif_surface_blend::DetailQuadrant, &'static str> {
        let r = d.resolved.as_ref().map_err(|reason| *reason)?;
        let loaded = |h: &Handle<Image>| images.get(h).ok_or("image-not-loaded");
        for h in r.images() { loaded(h)?; }
        if !samplers_reproduced(r, images) { return Err("sampler-mismatch"); }
        let (mut tentative, mut samplers) = (Vec::new(), Vec::new());
        let base_diffuse = r.base_diffuse.as_ref().map(|h| self.image(h, &mut tentative));
        let base_normal = r.base_normal.as_ref().map(|h| self.image(h, &mut tentative));
        let mut overlays = Vec::with_capacity(r.overlays.len());
        for (diffuse, normal) in &r.overlays {
            let two = normal.as_ref().map(|n| loaded(n).map(two_component)).transpose()?.unwrap_or(false);
            overlays.push((self.image(diffuse, &mut tentative), normal.as_ref().map(|n| self.image(n, &mut tentative)), two));
        }
        // The base diffuse sampler reads every diffuse and overlay normal map (terrain.wesl);
        // without a base texture the first overlay's stands in.
        let diffuse_rep = r.base_diffuse.as_ref().or(r.overlays.first().map(|(d, _)| d));
        let diffuse_sampler = match diffuse_rep { Some(h) => self.sampler(loaded(h)?, h, &mut samplers), None => 0 };
        let normal_sampler = match &r.base_normal { Some(h) => self.sampler(loaded(h)?, h, &mut samplers), None => diffuse_sampler };
        if self.palette.images.len() + tentative.len() > PALETTE_IMAGES { return Err("palette-images-full"); }
        if self.sampler_keys.len() + samplers.len() > PALETTE_SAMPLERS { return Err("palette-samplers-full"); }
        self.palette.images.extend(tentative.into_iter().map(Some));
        for (h, key) in samplers { self.palette.samplers.push(Some(h)); self.sampler_keys.push(key); }
        let base_two_component = r.base_normal.as_ref().map(|n| loaded(n).map(two_component)).transpose()?.unwrap_or(false);
        let t = r.uv;
        Ok(nvf_assets::nif_surface_blend::DetailQuadrant {
            cell_local: d.cell_local, base_diffuse, base_normal, diffuse_sampler, normal_sampler, overlays, folded: r.folded,
            base_two_component, flip_y: r.flip_y, base_rgb: r.base_rgb,
            uv: [t.matrix2.x_axis.x, t.matrix2.x_axis.y, t.matrix2.y_axis.x, t.matrix2.y_axis.y, t.translation.x, t.translation.y],
            tile: d.tile, tangent_sign: d.tangent_sign, features: r.features,
        })
    }
}

// [design] Receipts follow DrawMaterial's successful direct draw, not merely readiness
// or a fence. Every frame starts empty, so incomplete frames cannot accumulate to success.
#[derive(Component, Clone)]
struct BakeView {
    receipt: Receipt,
    height: Handle<Image>,
    resources_ready: Arc<AtomicBool>,
    transfers: Transfers,
    expected: HashSet<Entity>,
    complete: Arc<AtomicBool>,
    drawn: Arc<Mutex<HashSet<Entity>>>,
    submitted: Arc<Mutex<Option<Receipt>>>,
}

/// [design] Registered in every run, so the settings row can switch the blend while the
/// game runs, but gated: while the blend's [`MaterialGate`] is closed (never turned on, or
/// off and its materials gone, mesh_terrain_blend.rs `close_gate`) the capture material's
/// systems and the bake view extraction do not run, the capture shader is not loaded and
/// no allocator exists; the receipt and span systems run only while a bake view is in the
/// render world. Bevy's draw functions are wrapped when the first bake view appears and
/// put back once the gate closes.
pub struct SurfaceBakePlugin {
    pub gate: MaterialGate,
}
impl Plugin for SurfaceBakePlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "terrain_capture.wesl");
        app.add_plugins(MaterialPlugin::<SurfaceCaptureMaterial>::gated(self.gate.clone()));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            let (extract, closed) = (self.gate.clone(), self.gate.clone());
            render.add_systems(ExtractSchedule, extract_bake_views.run_if(move || extract.is_open()))
                .add_systems(Render, install_bake_receipts.run_if(first_bake_view).in_set(RenderSystems::Prepare))
                .add_systems(Render, uninstall_bake_receipts
                    .run_if(move |installed: Option<Res<BakeReceiptsInstalled>>| installed.is_some() && !closed.is_open())
                    .in_set(RenderSystems::Prepare))
                .add_systems(Render, reset_receipts.run_if(any_bake_view).in_set(RenderSystems::Prepare))
                .add_systems(Render, (assemble_atlas, submit_receipts).chain().run_if(any_bake_view).in_set(RenderSystems::Cleanup))
                .add_systems(bevy::core_pipeline::Core3d, (
                    bake_span_begin.before(bevy::core_pipeline::Core3dSystems::Prepass),
                    bake_span_end.after(bevy::core_pipeline::Core3dSystems::PostProcess),
                ).run_if(any_bake_view));
        }
    }
}

/// The bake cameras' views into the render world (what `ExtractComponentPlugin` does, but
/// gated). A view leaves with its camera: the bake camera is only ever despawned.
fn extract_bake_views(mut commands: Commands, views: Extract<Query<(RenderEntity, &BakeView)>>) {
    for (entity, view) in &views {
        commands.entity(entity).insert(view.clone());
    }
}

/// A bake view is in the render world (the receipt and span systems run only then).
fn any_bake_view(views: Query<(), With<BakeView>>) -> bool {
    !views.is_empty()
}

/// The render world's note that the receipts wrap the draw functions now.
#[derive(Resource)]
struct BakeReceiptsInstalled;

/// [design] A bake view is in the render world and the receipts are not installed yet:
/// the blend bakes for the first time since the gate opened (a run that never turns the
/// blend on keeps Bevy's own draw functions).
fn first_bake_view(views: Query<(), With<BakeView>>, installed: Option<Res<BakeReceiptsInstalled>>) -> bool {
    installed.is_none() && !views.is_empty()
}

/// Both phases' receipts, once (before the bake's passes are recorded; `DrawFunctions`
/// prepares each function when its phase renders), and round 10b's mip pipeline.
fn install_bake_receipts(world: &mut World) {
    set_draw_material::<Opaque3d, (bevy::pbr::DrawMaterial, BakeReceipt)>(world);
    set_draw_material::<Transparent3d, (bevy::pbr::DrawMaterial, BakeReceipt)>(world);
    let mips = MipPipeline::new(world.resource::<bevy::render::renderer::RenderDevice>().wgpu_device());
    world.insert_resource(mips);
    world.insert_resource(BakeReceiptsInstalled);
}

/// [design] The gate closed (no bake view, no blend or capture material left): both
/// phases get a new `DrawMaterial` command of Bevy's own, what bevy_pbr's
/// `add_render_command::<P, DrawMaterial>` installs, so an idle run draws as main does;
/// round 10b's mip pipeline is dropped.
fn uninstall_bake_receipts(world: &mut World) {
    set_draw_material::<Opaque3d, bevy::pbr::DrawMaterial>(world);
    set_draw_material::<Transparent3d, bevy::pbr::DrawMaterial>(world);
    world.remove_resource::<MipPipeline>();
    world.remove_resource::<BakeReceiptsInstalled>();
}

// [design] Base copies draw in Opaque3d, layer copies (alpha blended) in Transparent3d;
// while installed, both phases' DrawMaterial carries the receipt. The function keeps
// DrawMaterial's id, so prepared materials need no change either way.
fn set_draw_material<P: CachedRenderPipelinePhaseItem, C>(world: &mut World)
where
    C: RenderCommand<P> + Send + Sync + 'static,
    C::Param: bevy::ecs::system::ReadOnlySystemParam,
{
    let command = RenderCommandState::<P, C>::new(world);
    let functions = world.resource::<DrawFunctions<P>>();
    let mut functions = functions.write();
    let id = functions.id::<bevy::pbr::DrawMaterial>();
    functions.draw_functions[id.0 as usize] = Box::new(command);
}

/// The GPU time span's name (Tracy's GPU zone and the `--log-frame-times` list, graphics.rs).
pub const BAKE_SPAN: &str = "nvf_mesh_terrain_bake";
/// What the bake's span systems read: the render context, the view being drawn and the
/// bake views.
type BakeSpanParams = (bevy::render::renderer::RenderContext<'static, 'static>, Option<Res<'static, bevy::render::renderer::CurrentView>>, Query<'static, 'static, (), With<BakeView>>);

/// [design] A GPU time span around a bake view's whole draw ([`BAKE_SPAN`]; recorded only
/// with the render diagnostics on: `--log-frame-times`, a Tracy build). The span opens
/// before the view's first pass and closes after its last, so the bake's own passes nest
/// in it. Both ends are exclusive systems: the executor runs those on its scope's thread,
/// and the recorder pairs a span's end with the newest span still open on the same thread
/// (bevy_render `diagnostic/internal.rs`). Each end writes its timestamp into a command
/// encoder of its own, finished and queued at that system's place in the render order
/// (`RenderContext::add_command_buffer`), never into the render context's encoder: with
/// `NVF_PLAY_TEST_PROFILE_ENCODERS=1` asking for that encoder opens an automatic
/// `enc:<system>` span first, which the begin would nest inside and the context's drop
/// would close in its place (#2661 round 8). So neither system opens an `enc:` span, every
/// pass system in between opens and closes its own, and the end closes the bake span with
/// encoder profiling on or off. Any other view, or no recorder: nothing.
fn bake_span_begin(world: &mut World, state: &mut bevy::ecs::system::SystemState<BakeSpanParams>) {
    bake_span(world, state, true);
}
fn bake_span_end(world: &mut World, state: &mut bevy::ecs::system::SystemState<BakeSpanParams>) {
    bake_span(world, state, false);
}
fn bake_span(world: &mut World, state: &mut bevy::ecs::system::SystemState<BakeSpanParams>, begin: bool) {
    use bevy::render::diagnostic::RecordDiagnostics;
    {
        let Ok((mut ctx, view, bakes)) = state.get_mut(world) else { return };
        if !view.is_some_and(|v| bakes.contains(v.0)) { return; }
        let Some(recorder) = ctx.diagnostic_recorder() else { return };
        let mut encoder = ctx.render_device().create_command_encoder(&bevy::render::render_resource::CommandEncoderDescriptor { label: Some(BAKE_SPAN) });
        if begin {
            recorder.begin_time_span(&mut encoder, BAKE_SPAN.into());
        } else {
            recorder.end_time_span(&mut encoder);
        }
        ctx.add_command_buffer(encoder.finish());
    }
    state.apply(world);
}
fn reset_receipts(views: Query<&BakeView>, mut cameras: ResMut<bevy::render::camera::SortedCameras>) {
    // Freeze successful generations before another view pass can clear their target.
    cameras.0.retain(|camera| !views.get(camera.entity).is_ok_and(|view| view.complete.load(Ordering::Acquire)));
    for view in &views { view.drawn.lock().unwrap().clear(); }
}
/// [design] Round 10b: the compute pipeline that filters a baked cell's lower colour levels
/// (terrain_surface_mips.wgsl). Created when the first bake view reaches the render world
/// (`install_bake_receipts`) and dropped when the gate closes (`uninstall_bake_receipts`),
/// so a run that never turns the blend on never compiles it.
#[derive(Resource)]
struct MipPipeline { layout: wgpu::BindGroupLayout, pipeline: wgpu::ComputePipeline }

/// The mip filter's source (terrain_surface_mips.wgsl).
pub const MIP_SHADER: &str = include_str!("terrain_surface_mips.wgsl");
/// [design] terrain_surface_mips.wgsl's workgroup side and its uniform's cell capacity.
pub const MIP_WORKGROUP: u32 = 8;
pub const MIP_CELLS: usize = 9;

/// [design] The mip filter's uniform: each cell's atlas column and row (top row 0, the bake's
/// reversed Y) in a `vec4<u32>`, then the cell count: terrain_surface_mips.wgsl `Cells`.
pub fn mip_cells(baked: &[(u32, u32)], resolution: u32) -> [u32; MIP_CELLS * 4 + 4] {
    let mut words = [0; MIP_CELLS * 4 + 4];
    let cells = &baked[..baked.len().min(MIP_CELLS)];
    for (i, &(x, y)) in cells.iter().enumerate() { words[i * 4] = x / resolution; words[i * 4 + 1] = y / resolution; }
    words[MIP_CELLS * 4] = cells.len() as u32;
    words
}
/// [design] One mip pass's workgroups: the level's texels per cell side in x and y, one
/// layer of workgroups per baked cell in z.
pub fn mip_workgroups(resolution: u32, level: u32, cells: u32) -> (u32, u32, u32) {
    let side = (resolution >> level).div_ceil(MIP_WORKGROUP);
    (side, side, cells)
}

impl MipPipeline {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mesh terrain colour mips"), source: wgpu::ShaderSource::Wgsl(MIP_SHADER.into()) });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh terrain colour mips"),
            entries: &[
                entry(0, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(1, wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: TextureFormat::Rgba16Float, view_dimension: wgpu::TextureViewDimension::D2 }),
                entry(2, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("mesh terrain colour mips"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("mesh terrain colour mips"), layout: Some(&pipeline_layout), module: &module,
            entry_point: Some("downsample"), compilation_options: default(), cache: None,
        });
        Self { layout, pipeline }
    }

    /// [design] Records, after `encoder`'s copies, one compute pass per level below 0: each
    /// writes level `k` of the `baked` cells (corners of their level-0 rectangles) as the
    /// linear mean of the 2 by 2 texels of level `k - 1` under each texel, every block inside
    /// its own cell (cells are whole at every level), so no cell's levels read another's.
    fn record(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, atlas: &wgpu::Texture, baked: &[(u32, u32)], resolution: u32) {
        use wgpu::util::DeviceExt;
        let words = mip_cells(baked, resolution);
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let cells = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("mesh terrain colour mip cells"), contents: &bytes, usage: wgpu::BufferUsages::UNIFORM });
        let level_view = |level| atlas.create_view(&wgpu::TextureViewDescriptor { label: Some("mesh terrain colour level"), base_mip_level: level, mip_level_count: Some(1), ..default() });
        for level in 1..colour_levels(resolution) {
            let (source, destination) = (level_view(level - 1), level_view(level));
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mesh terrain colour mips"), layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&destination) },
                    wgpu::BindGroupEntry { binding: 2, resource: cells.as_entire_binding() },
                ],
            });
            let (x, y, z) = mip_workgroups(resolution, level, words[MIP_CELLS * 4]);
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("mesh terrain colour mips"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(x, y, z);
        }
    }
}

/// [design] Round 10b: copies one cell's rectangle at `level` (corners given at level 0)
/// from one texture to another.
fn copy_cell(encoder: &mut wgpu::CommandEncoder, from: (&wgpu::Texture, (u32, u32)), to: (&wgpu::Texture, (u32, u32)), level: u32, resolution: u32) {
    let mut src = from.0.as_image_copy();
    src.mip_level = level; src.origin.x = from.1 .0 >> level; src.origin.y = from.1 .1 >> level;
    let mut dst = to.0.as_image_copy();
    dst.mip_level = level; dst.origin.x = to.1 .0 >> level; dst.origin.y = to.1 .1 >> level;
    let side = resolution >> level;
    encoder.copy_texture_to_texture(src, dst, Extent3d { width: side, height: side, depth_or_array_layers: 1 });
}

/// [design: bevy_render/src/lib.rs] Cleanup follows render_system, including the final
/// camera output and queue submission. This separate submission is therefore AFTER all
/// camera writes (the bake target's draws of this frame included), never before a
/// clear/upscale can erase them. Round 10b: in one submission, the retained cells at every
/// level from the previous publication, the baked cells' level 0 from the bake target and
/// then their lower levels ([`MipPipeline::record`]); only after it is the view complete,
/// so the receipt ([`submit_receipts`]) covers the copies and the mip passes. A view whose
/// bake draws did not all draw in this frame, or that needs the mip pipeline before it
/// exists, waits for a later frame (the bake camera draws again until complete).
fn assemble_atlas(
    views: Query<&BakeView>,
    images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
    device: Res<bevy::render::renderer::RenderDevice>,
    queue: Res<bevy::render::renderer::RenderQueue>,
    mips: Option<Res<MipPipeline>>,
) {
    for view in &views {
        let plan = &view.transfers;
        let available = images.get(&view.height).is_some() && images.get(view.receipt.destination).is_some()
            && images.get(&plan.destination).is_some()
            && plan.source.as_ref().is_none_or(|s| images.get(s).is_some())
            && plan.bake.as_ref().is_none_or(|b| images.get(b).is_some());
        view.resources_ready.store(available, Ordering::Release);
        if !available || view.complete.load(Ordering::Acquire) || !draws_ready(&view.expected, &view.drawn.lock().unwrap()) { continue; }
        let Some(destination) = images.get(&plan.destination) else { continue };
        let filter = match (&mips, plan.baked.is_empty()) { (_, true) => None, (Some(m), false) => Some(m), (None, false) => continue };
        let mut encoder = device.create_command_encoder(&bevy::render::render_resource::CommandEncoderDescriptor { label: Some("mesh terrain atlas assembly") });
        if let Some(source) = plan.source.as_ref().and_then(|s| images.get(s)) {
            for cell in &plan.cells { for level in 0..colour_levels(plan.resolution) {
                copy_cell(&mut encoder, (&source.texture, cell.source), (&destination.texture, cell.destination), level, plan.resolution);
            }}
        }
        if let Some(bake) = plan.bake.as_ref().and_then(|b| images.get(b)) {
            for &corner in &plan.baked { copy_cell(&mut encoder, (&bake.texture, corner), (&destination.texture, corner), 0, plan.resolution); }
        }
        if let Some(filter) = filter { filter.record(device.wgpu_device(), &mut encoder, &destination.texture, &plan.baked, plan.resolution); }
        queue.submit([encoder.finish()]);
        view.complete.store(true, Ordering::Release);
    }
}
/// [design] Every planned bake draw, and nothing else, drew in this frame (the drawn set is
/// emptied every frame, so partial frames cannot add up). A copy-only view plans none.
fn draws_ready(expected: &HashSet<Entity>, drawn: &HashSet<Entity>) -> bool { expected == drawn }
/// [design] The receipt, once the assembly's submission (copies and mip passes) is queued
/// and the view's GPU images exist.
fn submit_receipts(views: Query<&BakeView>) {
    for view in &views {
        if view.resources_ready.load(Ordering::Acquire) && view.complete.load(Ordering::Acquire) {
            *view.submitted.lock().unwrap() = Some(view.receipt);
        }
    }
}
struct BakeReceipt;
impl<P: PhaseItem> RenderCommand<P> for BakeReceipt {
    type Param = ();
    type ViewQuery = Option<&'static BakeView>;
    type ItemQuery = ();
    fn render<'w>(item: &P, view: ROQueryItem<'w, '_, Self::ViewQuery>, _: Option<()>, _: SystemParamItem<'w, '_, Self::Param>, _: &mut TrackedRenderPass<'w>) -> RenderCommandResult {
        if let Some(view) = view {
            // NoAutomaticBatching + NoIndirectDrawing: one successful indexed draw
            // represents this exact quadrant, with no indirect culling ambiguity.
            view.drawn.lock().unwrap().insert(item.main_entity().id());
        }
        RenderCommandResult::Success
    }
}

/// [design] Cells drawn while the blend was off have no [`SurfaceCell`]: each loaded
/// exterior cell of the 3x3 region that lacks one gets its capture from its landscape
/// prepared again (`terrain.rs::capture_surface`, the same quadrant stacks and cached
/// materials as a cell drawn with the blend on), once per cell entity while the blend
/// stays on (`BlendState::tried`, emptied when it turns off). Cells loaded while it is on
/// are captured as they are drawn.
#[allow(clippy::too_many_arguments)]
pub fn backfill_cells(
    mut commands: Commands,
    world: Res<WorldData>,
    streamer: Res<Streamer>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    captured: Query<&SurfaceCell>,
    mut cache: ResMut<SurfaceCache>,
    mut terrain: crate::terrain::TerrainMaterials,
    mut models: nvf_assets::model::ModelCtx,
    mut state: ResMut<crate::mesh_terrain_blend::BlendState>,
) {
    cache.ensure_context(&mut commands, &world, &streamer);
    if streamer.interior.is_some() { return; }
    cache.track_stitched_inputs(streamer.loaded.iter().filter_map(|(grid, entity)| {
        captured.get(*entity).ok().filter(|c| c.world == world.world).map(|c| (*grid, *entity, c.content_revision))
    }));
    let origin = camera.single().ok().map(|camera| cache.selected_origin(bevy_to_game(camera.translation()).truncate()));
    // Refresh loaded affected captures even outside the requested region. Failed work
    // stays scoped to this entity incarnation and is excluded from copying/publication.
    for (&(x, y), &cell) in &streamer.loaded {
        let existing = captured.get(cell).ok();
        if existing.is_some() && !cache.stitched_dirty.contains_key(&cell) { continue; }
        if existing.is_none() && !origin.is_some_and(|o| (o.0..o.0 + 3).contains(&x) && (o.1..o.1 + 3).contains(&y)) { continue; }
        if existing.is_none() && !state.tried.insert(cell) { continue; }
        let Some(land) = world.grid_cell((x, y)).and_then(|c| c.land) else { continue };
        let tile = terrain.settings.tile;
        let Some(prep) = crate::terrain::prepare_terrain(world.lo, world.schema, land, (x, y), tile, world.land_ids((x, y)), nvf_play::smooth_terrain::spec(), crate::terrain_lod::first_draw()) else { continue };
        let folds = terrain.folds();
        if let Some(surface) = crate::terrain::capture_surface(&world, prep, &mut crate::terrain::TerrainCtx { tm: &mut terrain, assets: &mut models }, folds) {
            commands.entity(cell).try_insert(cache.refreshed_capture(cell, existing, surface));
        }
    }
}

/// [design] Epoch changes clear publications before conversion, even with no camera.
pub fn invalidate_context(mut commands: Commands, mut cache: ResMut<SurfaceCache>, world: Res<WorldData>, streamer: Res<Streamer>) {
    cache.ensure_context(&mut commands, &world, &streamer);
}

type CapturedInput = ((i32, i32), Entity, u64);
fn stitched_dependents(inputs: &mut HashMap<Entity, u64>, captures: impl Iterator<Item = CapturedInput>) -> HashSet<(i32, i32)> {
    let mut stitched = HashSet::new();
    for (grid, entity, revision) in captures {
        // First residency does not change the LAND records the neighbours already
        // stitched from WorldData. Only reconstruction of a known input dirties them.
        if inputs.insert(entity, revision).is_some_and(|previous| previous != revision) {
            for offset in [(-1, 0), (1, 0), (0, -1), (0, 1)] { stitched.insert((grid.0 + offset.0, grid.1 + offset.1)); }
        }
    }
    stitched
}

/// [design] Capture sources are retained separately from their unlit clones. Edits to
/// source materials refresh those clones and dependencies before a new request is keyed.
/// Diffuse image modifications are versioned per dependency in update_cache.
pub fn refresh_captures(
    mut cells: Query<&mut SurfaceCell>,
    folded: Res<Assets<TerrainMaterial>>,
    ordinary: Res<Assets<nvf_assets::NifMaterial>>,
    folded_events: Option<Res<Messages<AssetEvent<TerrainMaterial>>>>,
    ordinary_events: Option<Res<Messages<AssetEvent<nvf_assets::NifMaterial>>>>,
    parallax_edits: Option<Res<crate::terrain::ParallaxEdits>>,
    mut cursors: Local<CaptureEvents>,
) {
    // Each changed folded material, and whether something other than this frame's parallax
    // flips changed it (a removal, or more Modified events than the flips made).
    let mut modified: HashMap<AssetId<TerrainMaterial>, u32> = HashMap::new();
    let mut removed: HashSet<AssetId<TerrainMaterial>> = HashSet::new();
    if let Some(events) = folded_events.as_ref() {
        for event in cursors.folded.read(events) {
            match event {
                AssetEvent::Modified { id } => *modified.entry(*id).or_default() += 1,
                AssetEvent::Removed { id } => { removed.insert(*id); }
                _ => {}
            }
        }
    }
    let flips = |id: &AssetId<TerrainMaterial>| parallax_edits.as_deref().and_then(|p| p.0.get(id)).copied().unwrap_or(0);
    let generic: HashSet<AssetId<TerrainMaterial>> = modified.iter().filter(|(id, n)| **n > flips(id)).map(|(id, _)| *id).chain(removed.iter().copied()).collect();
    let folded_changed: HashSet<AssetId<TerrainMaterial>> = modified.keys().copied().chain(removed.iter().copied()).collect();
    let ordinary_changed: HashSet<_> = ordinary_events.as_ref().map(|e| cursors.ordinary.read(e).filter_map(modified_asset).collect()).unwrap_or_default();
    for mut cell in &mut cells {
        // Component edits/replacement also refresh the derived dependency list. Preserve
        // the producer's change tick; this bookkeeping is not another content edit.
        if cell.is_changed() { cell.bypass_change_detection().refresh_dependencies(); }
        let dirty = cell.sources.iter().any(|s| match s { CaptureSource::Folded(h) => folded_changed.contains(&h.id()), CaptureSource::Single(h, _) => ordinary_changed.contains(&h.id()) });
        if !dirty { continue; }
        // #3322 review: the terrain parallax switch's own edits (and nothing else this frame)
        // leave the cell untouched when what it is made from compares equal (the bake copy
        // drops the switch, the detail never reads it); any other edit remakes it as before.
        let parallax_only = cell.sources.iter().all(|s| match s {
            CaptureSource::Folded(h) => !generic.contains(&h.id()),
            CaptureSource::Single(h, _) => !ordinary_changed.contains(&h.id()),
        });
        if parallax_only && captured_inputs_unchanged(&cell, &folded, &ordinary) { continue; }
        let sources = cell.sources.clone();
        let mut missing = false;
        for (source, (_, draw)) in sources.iter().zip(&mut cell.quadrants) {
            *draw = match source {
                CaptureSource::Folded(h) => {
                    let Some(m) = folded.get(h) else { missing = true; continue };
                    let mut m = m.clone();
                    bake_copy(&mut m);
                    CaptureDraw::Folded(Box::new(m))
                }
                CaptureSource::Single(h, layer) => {
                    let Some(m) = ordinary.get(h) else { missing = true; continue };
                    CaptureDraw::Single(SurfaceCaptureMaterial::from_source(m, *layer))
                }
            };
        }
        if missing { cell.paths = [CapturePath::HeightOnly("missing-source-material"); 4]; cell.quadrants.clear(); cell.sources.clear(); }
        // [design] Round 10a: the detail's textures and values come from the same source
        // materials; read them again. A source gone sends the quadrant to the colour cache
        // for good; an edit the detail cannot follow (layers no longer matching, or, round
        // 10a review, an overflow layer's colour, alpha or texture transform, or the base's
        // transform, `overflow-material`) keeps the capture with the reason, so the
        // publication refuses it (`DetailPalette::admit`) until an edit makes it
        // reproducible again.
        for d in &mut cell.detail {
            let DetailCapture::Layers(detail) = &mut *d else { continue };
            if missing { *d = DetailCapture::Colour("missing-source-material"); continue; }
            detail.resolved = resolve_detail(&detail.base, &detail.overflow, detail.opacity.len(), &folded, &ordinary);
        }
        cell.refresh_dependencies();
        // This is a source edit, not a LAND reconstruction; Changed catches it without
        // propagating a false stitched-LAND reconstruction to its neighbours.
    }
}
/// [design] A quadrant's detail resolved against its source materials as they are now
/// (capture, terrain.rs `DetailParts::capture`, and material refresh): the reasons of
/// [`ResolvedDetail::from_materials`], or `unresolved-materials` when a source material
/// is not in its assets.
pub fn resolve_detail(base: &DetailBase, overflow: &[Handle<nvf_assets::NifMaterial>], layers: usize, folded: &Assets<TerrainMaterial>, ordinary: &Assets<nvf_assets::NifMaterial>) -> Result<ResolvedDetail, &'static str> {
    const UNRESOLVED: &str = "unresolved-materials";
    let overflow: Vec<&nvf_assets::NifMaterial> = overflow.iter().map(|h| ordinary.get(h)).collect::<Option<_>>().ok_or(UNRESOLVED)?;
    match base {
        DetailBase::Folded(h) => { let m = folded.get(h).ok_or(UNRESOLVED)?; ResolvedDetail::from_materials(&m.base, None, Some(&m.extension), &overflow, layers) }
        DetailBase::Single(h) => { let m = ordinary.get(h).ok_or(UNRESOLVED)?; ResolvedDetail::from_materials(&m.base, Some(&m.extension), None, &overflow, layers) }
    }
}
/// Whether a cell's capture would come out the same from its source materials as they are
/// now: every bake copy as it is kept (compared whole, by its debug form: every field, every
/// texture handle) and every detail's resolution [design, #3322 review: compared inputs, not
/// remembered edits, so no edit is ever taken for another].
fn captured_inputs_unchanged(cell: &SurfaceCell, folded: &Assets<TerrainMaterial>, ordinary: &Assets<nvf_assets::NifMaterial>) -> bool {
    let draws = cell.sources.iter().zip(&cell.quadrants).all(|(source, (_, kept))| {
        let fresh = match source {
            CaptureSource::Folded(h) => folded.get(h).map(|m| {
                let mut m = m.clone();
                bake_copy(&mut m);
                CaptureDraw::Folded(Box::new(m))
            }),
            CaptureSource::Single(h, layer) => ordinary.get(h).map(|m| CaptureDraw::Single(SurfaceCaptureMaterial::from_source(m, *layer))),
        };
        fresh.is_some_and(|f| format!("{f:?}") == format!("{kept:?}"))
    });
    draws && cell.detail.iter().all(|d| match d {
        DetailCapture::Layers(detail) => resolve_detail(&detail.base, &detail.overflow, detail.opacity.len(), folded, ordinary) == detail.resolved,
        _ => true,
    })
}

fn modified_asset<A: Asset>(event: &AssetEvent<A>) -> Option<AssetId<A>> {
    match event { AssetEvent::Modified { id } | AssetEvent::Removed { id } => Some(*id), _ => None }
}
#[derive(Default)]
pub struct CaptureEvents {
    folded: bevy::ecs::message::MessageCursor<AssetEvent<TerrainMaterial>>,
    ordinary: bevy::ecs::message::MessageCursor<AssetEvent<nvf_assets::NifMaterial>>,
}

#[allow(clippy::too_many_arguments)]
pub fn update_cache(
    mut commands: Commands,
    mut cache: ResMut<SurfaceCache>,
    world: Res<WorldData>,
    streamer: Res<Streamer>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    cells: Query<(Entity, Ref<SurfaceCell>)>,
    events: Option<Res<Messages<AssetEvent<Image>>>>,
    readiness: Option<Res<crate::mesh_terrain_blend::BlendReadiness>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    mut captures: ResMut<Assets<SurfaceCaptureMaterial>>,
) {
    // Also done before material conversion in the production schedule. Keep the guard
    // here so direct callers cannot accept a receipt without context validation.
    cache.ensure_context(&mut commands, &world, &streamer);
    if cache.rejected || streamer.interior.is_some() { return; }
    let mut dependencies = HashSet::new();
    for (_, cell) in &cells { dependencies.extend(cell.image_dependencies().iter().copied()); }
    cache.image_revisions.retain(|id, _| dependencies.contains(id));
    cache.removed_images.retain(|id| dependencies.contains(id));
    cache.awaited_images.retain(|id| dependencies.contains(id));
    if let Some(events) = events {
        let changed: Vec<_> = cache.image_events.read(&events).filter(|e| match e {
            AssetEvent::Added { id } | AssetEvent::Modified { id } | AssetEvent::Removed { id } | AssetEvent::LoadedWithDependencies { id } => dependencies.contains(id),
            _ => false,
        }).copied().collect();
        for event in changed {
            match event {
                AssetEvent::Modified { id } => { *cache.image_revisions.entry(id).or_default() += 1; cache.removed_images.remove(&id); cache.awaited_images.remove(&id); }
                AssetEvent::Removed { id } => { *cache.image_revisions.entry(id).or_default() += 1; cache.removed_images.insert(id); }
                // [design] Round 10a review (third round): an image that returns (added back
                // after a removal) or arrives late (a replacement refused its quadrant,
                // `image-not-loaded`) advances its revision, so the resident cell's request
                // changes and the detail is rebuilt. Only the first of its Added /
                // LoadedWithDependencies notifications does: it leaves both sets, so the
                // second (in this frame or a later one) changes nothing.
                AssetEvent::Added { id } | AssetEvent::LoadedWithDependencies { id } => {
                    let returned = cache.removed_images.remove(&id);
                    if cache.awaited_images.remove(&id) | returned { *cache.image_revisions.entry(id).or_default() += 1; }
                }
                _ => {}
            }
        }
    }
    // [design] Round 10a review (second round): a cell whose bake copies bind a colour image
    // that was removed (and not added back) is planned without that cell, as if it were not
    // resident (`missing-colour-image`): its copy could never be prepared, so a replacement
    // waiting for its draw would never receive a receipt. An image not loaded yet is still
    // waited for, as before. The cell returns (and bakes) when the image is added back.
    let colour_missing: HashSet<Entity> = cells.iter().filter(|(_, c)| c.colour_images().any(|id| cache.removed_images.contains(&id) && !images.contains(id))).map(|(e, _)| e).collect();
    let resolution = cache.resolution;
    let width = atlas_width(resolution);
    let layout = cache.layout;
    let Ok(camera) = camera.single() else { return };
    let eye = bevy_to_game(camera.translation());
    let origin = cache.selected_origin(eye.truncate());
    let mut resident: Vec<_> = cells.iter().filter(|(e, c)| {
        c.world == world.world && streamer.loaded.get(&c.grid) == Some(e)
            && !cache.stitched_dirty.contains_key(e)
            && !colour_missing.contains(e)
            && (origin.0..origin.0 + 3).contains(&c.grid.0)
            && (origin.1..origin.1 + 3).contains(&c.grid.1)
            && c.heights.len() == layout.side() * layout.side()
    }).collect();
    resident.sort_by_key(|(_, c)| c.grid);
    let keys: Vec<_> = resident.iter().map(|(entity, c)| {
        let mut images: Vec<_> = c.image_dependencies().iter().copied().map(|id| (id, cache.image_revisions.get(&id).copied().unwrap_or(0))).collect();
        images.sort_by_key(|(id, _)| *id);
        CellKey { grid: c.grid, entity: *entity, revision: c.content_revision, changed: c.last_changed().get(), images }
    }).collect();
    cache.current.clone_from(&keys);
    // Retain immutable pixels at their OLD origin, masking departing/changed cells.
    if let Some(front) = &cache.published {
        let coverage = retained_mask(front, &keys);
        if coverage != front.coverage {
            cache.revision += 1;
            let revision = cache.revision;
            let front = cache.published.as_mut().unwrap();
            front.coverage = coverage;
            front.cells = coverage.count_ones() as usize;
            front.revision = revision;
            // [design] Round 10a review: new clones of the front bind only what covered
            // quadrants read (a removed image masks its cells in this same update, through
            // its dependency revision above), so they can always be prepared.
            front.palette = front.live_palette(coverage);
        }
    }
    // [design] Round 10a review: frames in a row the front's materials have not settled.
    // The replacement guard below waits for settlement, but never for longer than
    // `SETTLE_FRAMES`, so a candidate that can never be prepared cannot hold every later
    // publication back.
    let unsettled = cache.published.as_ref().is_some_and(|p| readiness.as_ref().is_some_and(|r| !r.settled(p.revision)));
    cache.unsettled_frames = if unsettled { cache.unsettled_frames.saturating_add(1) } else { 0 };
    let signature = Request { epoch: cache.epoch, origin, cells: keys };
    if cache.signature.as_ref() != Some(&signature) {
        if let Some(old) = cache.job.take() {
            commands.entity(old.root).despawn();
            cache.retirement = 3;
        }
        cache.signature = None;
        cache.ready = None;
        *cache.submitted.lock().unwrap() = None;
    }
    // Consume only exact receipts for the current request and source publication.
    let publish = cache.ready.take();
    let received = cache.submitted.lock().unwrap().take();
    cache.ready = received;
    if cache.signature.as_ref() == Some(&signature) {
        if cache.job.as_ref().is_some_and(|j| Some(j.receipt) == publish
            && j.receipt.epoch == cache.epoch && j.receipt.request == cache.generation
            && j.receipt.destination == j.publication.colour.id()
            && j.receipt.source_revision == cache.published.as_ref().map_or(0, |p| p.revision)) {
            let job = cache.job.take().unwrap();
            commands.entity(job.root).despawn();
            cache.revision += 1;
            let mut publication = job.publication;
            publication.revision = cache.revision;
            info!("{}", job.report);
            cache.published = Some(publication);
        }
        return;
    }
    if cache.retirement > 0 { cache.retirement -= 1; return; }
    if resident.is_empty() { cache.signature = Some(signature); return; }
    // A pure removal changes only enabled coverage; no colour allocation or transfer.
    if cache.published.as_ref().is_some_and(|p| signature.cells.iter().all(|k| p.keys.contains(k))) {
        cache.signature = Some(signature);
        return;
    }
    // One fresh back allocation only after the previous publication's materials have
    // switched. Combined with cancellation retirement this bounds rapid replanning. A wait
    // past `SETTLE_FRAMES` starts the replacement anyway, with a line (at most one
    // allocation per `SETTLE_FRAMES` frames that way).
    if unsettled {
        if cache.unsettled_frames <= SETTLE_FRAMES { return; }
        warn!("mesh terrain blend: settlement wait expired revision={} frames={}; the replacement starts", cache.published.as_ref().map_or(0, |p| p.revision), cache.unsettled_frames);
        cache.unsettled_frames = 0;
    }
    cache.generation += 1;
    cache.signature = Some(signature.clone());
    let copied: HashSet<_> = cache.published.as_ref().map(|p| resident.iter().filter(|(_, c)| c.colour_mask() != 0 && signature.cells.iter().any(|k| k.grid == c.grid && p.keys.contains(k))).map(|(_, c)| c.grid).collect()).unwrap_or_default();

    // [design] Round 10b: the published atlas has every level and is only ever written by
    // the render world's assembly (copies and the mip filter's storage writes), never drawn
    // into; it has no CPU copy (the GPU texture starts zeroed).
    let mut colour = Image::new_uninit(
        Extent3d { width, height: width, depth_or_array_layers: 1 },
        TextureDimension::D2, TextureFormat::Rgba16Float,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    colour.texture_descriptor.mip_level_count = colour_levels(resolution);
    colour.texture_descriptor.usage |= TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC | TextureUsages::COPY_DST;
    colour.sampler = ImageSampler::linear();
    let colour = images.add(colour);
    // [design] Round 10a: with landscape detail the height atlas carries the detail rows.
    let rows = height_atlas_rows(layout, cache.detail);
    let mut heights = empty_height_atlas_rows(layout, rows);
    let mut expected = HashSet::new();
    // [design] #2551: the bake's copies and camera sit at the cells' true world places (the
    // slots' offsets from the publication's origin, under a root moved to that origin), so
    // a draw that hangs its reads on world coordinates (terrain anti-tiling's lattice,
    // nvf_assets terrain_sampling.wesl) reads the same texels in the bake as near the camera.
    let root = commands.spawn((Name::new("Mesh terrain surface bake"), bake_root(origin), Visibility::Inherited)).id();
    let slots = resident.iter().map(|(_, c)| (c.grid.0 - origin.0, c.grid.1 - origin.1)).collect();
    for (_, cell) in &resident {
        let slot = (cell.grid.0 - origin.0, cell.grid.1 - origin.1);
        encode_surface_cell(layout, &mut heights, slot, cell, &slots);
        if copied.contains(&cell.grid) { continue; }
        for (mesh, draw) in &cell.quadrants {
            let mut entity = commands.spawn((
                bevy::render::batching::NoAutomaticBatching, Mesh3d(meshes.add(mesh.clone())),
                Transform::from_xyz(slot.0 as f32 * 4096.0, slot.1 as f32 * 4096.0, 0.0),
                RenderLayers::layer(BAKE_LAYER), bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver, ChildOf(root),
            ));
            match draw {
                CaptureDraw::Folded(material) => entity.insert(MeshMaterial3d(materials.add(material.as_ref().clone()))),
                CaptureDraw::Single(material) => entity.insert(MeshMaterial3d(captures.add(material.clone()))),
            };
            expected.insert(entity.id());
        }
    }
    // [design] Round 10a: the palette and the admitted quadrants' detail, packed after the
    // heights; detail masks join the metadata rows. Every replacement repacks its own
    // palette numbers (the height atlas is rebuilt for it), so no numbers of another
    // publication's palette survive.
    let detail = cache.detail.then(|| {
        let retained: Vec<_> = cache.published.as_ref().map(|p| p.detail.iter().copied().filter(|(grid, _)| signature.cells.iter().any(|k| k.grid == *grid && p.keys.contains(k))).collect()).unwrap_or_default();
        let assembled = DetailPalette::assemble(resident.iter().map(|(_, c)| &**c), &images, &retained);
        for (((grid, q), quadrant), cell) in assembled.admitted.iter().filter_map(|a| resident.iter().find(|(_, c)| c.grid == a.0 .0).map(|(_, c)| (a, c))) {
            let DetailCapture::Layers(d) = &cell.detail[*q as usize] else { continue };
            let slot = (grid.0 - origin.0, grid.1 - origin.1);
            encode_detail(layout, &mut heights, detail_entry(slot, *q), quadrant, d);
        }
        for (_, cell) in &resident {
            let slot = (cell.grid.0 - origin.0, cell.grid.1 - origin.1);
            let mask = assembled.admitted.iter().filter(|((g, _), _)| *g == cell.grid).fold(0, |m, ((_, q), _)| m | 1 << q);
            encode_detail_mask(layout, &mut heights, slot, mask);
        }
        assembled
    });
    // [design] Round 10a review (third round): the detail images this replacement plans
    // without (not in the image assets), whose arrival must rebuild it (see the events above).
    cache.awaited_images = if cache.detail {
        resident.iter().flat_map(|(_, c)| c.detail.iter()).filter_map(|d| match d { DetailCapture::Layers(d) => d.resolved.as_ref().ok(), DetailCapture::Colour(_) => None })
            .flat_map(ResolvedDetail::images).map(Handle::id).filter(|id| !images.contains(*id)).collect()
    } else { HashSet::new() };
    let resident_report: Vec<_> = resident.iter().map(|(_, cell)| format!("({}, {}):q=[{}] colour-mask={:#x}", cell.grid.0, cell.grid.1, cell.paths.map(CapturePath::label).join(","), cell.colour_mask())).collect();
    let mut excluded = Vec::new();
    for y in origin.1..origin.1 + 3 { for x in origin.0..origin.0 + 3 {
        if !resident.iter().any(|(_, c)| c.grid == (x, y)) {
            let reason = if !streamer.loaded.contains_key(&(x, y)) { "not-loaded" }
                else if cells.iter().any(|(e, c)| c.grid == (x, y) && c.world == world.world && colour_missing.contains(&e)) { "missing-colour-image" }
                else if cells.iter().any(|(_, c)| c.grid == (x, y) && c.world == world.world) { "invalid-prepared-heights" }
                else { "no-prepared-LAND" };
            excluded.push(format!("({x}, {y}):{reason}"));
        }
    }}
    // Eye XY is inside the centre slot, regardless of eye height. This probes the
    // uploaded coefficients beneath that point, not an unobserved mesh fragment.
    let probe = publication_probe(layout, &heights, origin, eye.truncate(), resolution);
    let height = images.add(Image::new(
        Extent3d { width: layout.width() as u32, height: rows as u32, depth_or_array_layers: 1 },
        TextureDimension::D2, heights, TextureFormat::Rgba8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    ));
    let source_revision = cache.published.as_ref().map_or(0, |p| p.revision);
    let receipt = Receipt { epoch: cache.epoch, request: cache.generation, destination: colour.id(), source_revision };
    let baked_cells: Vec<_> = resident.iter().filter(|(_, c)| !copied.contains(&c.grid) && !c.quadrants.is_empty()).map(|(_, c)| colour_rectangle(c.grid, origin, resolution)).collect();
    let baked = baked_cells.len();
    info!("mesh terrain blend: assembly epoch={} request={} copied={} baked={} retained-origin={:?} destination-origin={origin:?} levels={}", cache.epoch, cache.generation, copied.len(), baked, cache.published.as_ref().map(|p| p.origin), colour_levels(resolution));
    if expected.is_empty() && copied.is_empty() {
        cache.ready = Some(receipt);
    } else {
        // [design] Round 10b: the bake camera draws into a single-level target of its own (a
        // render attachment has one level; the published atlas's view has them all), held
        // only by this job: the assembly copies the baked cells' level 0 from it.
        let bake = (!expected.is_empty()).then(|| {
            let mut target = Image::new_uninit(Extent3d { width, height: width, depth_or_array_layers: 1 }, TextureDimension::D2, TextureFormat::Rgba16Float, RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD);
            target.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
            images.add(target)
        });
        let transfers = Transfers {
            source: cache.published.as_ref().filter(|_| !copied.is_empty()).map(|p| p.colour.clone()),
            destination: colour.clone(), resolution,
            cells: cache.published.as_ref().map(|p| copied.iter().map(|&grid| CellCopy { source: colour_rectangle(grid, p.origin, resolution), destination: colour_rectangle(grid, origin, resolution) }).collect()).unwrap_or_default(),
            bake: bake.clone(), baked: baked_cells,
        };
        // A copy-only replacement draws nothing: its camera stays inactive (no target, no
        // passes) and only carries the view to the render world's assembly.
        let (is_active, target) = match bake { Some(bake) => (true, RenderTarget::Image(bake.into())), None => (false, RenderTarget::None { size: UVec2::ONE }) };
        commands.spawn((
            Camera3d::default(), bevy::render::view::NoIndirectDrawing,
            BakeView { receipt, height: height.clone(), resources_ready: default(), transfers, expected, complete: default(), drawn: default(), submitted: cache.submitted.clone() },
            Camera { order: -10, is_active, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
            // [design] Unlit surface-bake target: no dither, which Bevy warns of beside
            // `Tonemapping::None` every frame it sees the camera (#2757).
            target, Hdr, Tonemapping::None, DebandDither::Disabled, Msaa::Off,
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed { width: 12288.0, height: 12288.0 },
                near: 0.0, far: 200.0, ..OrthographicProjection::default_3d()
            }),
            Transform::from_xyz(6144.0, 6144.0, 100.0).looking_at(Vec3::new(6144.0, 6144.0, 0.0), Vec3::Y),
            RenderLayers::layer(BAKE_LAYER), ChildOf(root),
        ));
    }
    let report = publication_log(cache.generation, origin, &resident_report, &excluded, &probe, (colour.id(), height.id()), resolution) + &detail_log(detail.as_ref());
    let palette_use = detail.as_ref().map(|d| d.usage(&images)).unwrap_or_default();
    let (palette, detail) = detail.map(|d| (d.palette, d.admitted.into_iter().map(|(at, _)| at).collect())).unwrap_or_default();
    cache.job = Some(BakeJob {
        root, receipt, copied: copied.len(), baked,
        publication: PublishedSurface { colour, height, origin, cells: resident.len(), resolution, revision: 0,
            coverage: signature.cells.iter().fold(0, |m, k| m | (1 << ((k.grid.1-origin.1)*3+k.grid.0-origin.0))), keys: signature.cells,
            layout, palette, detail, palette_use },
        report,
    });
}

/// [design] Test aid only. Counts every main frame after an initially prepared retained
/// witness exists. It checks coherent CPU bytes and prepared material acknowledgements;
/// screenshots remain necessary to judge the actual rendered contact.
#[derive(Resource)]
struct ContinuityProbe {
    /// The crossing sequence the probe drives (`NVF_PLAY_TEST_MESH_BLEND_CROSSING`).
    crossing: Option<Crossing>,
    start: f64, duration: f64, done: bool, armed: bool,
    witnesses: Vec<(Entity, CellKey, Vec2)>,
    camera: Option<(i32, i32)>, colour: Option<AssetId<Image>>, request: u64,
    selected: Option<(i32, i32)>, direction: Option<(i32, i32)>, previous_pending: bool, recentres: u64, pending_reversals: u64,
    frames: u64, transitions: u64, pending: u64, retained: u64, pending_retained: u64, unwitnessed_pending: u64, replacements: u64,
    unpublished: u64, missing_enabled: u64, missing_prepared: u64, failures: u64,
    copied: usize, baked: usize,
}
pub fn install_continuity_probe(app: &mut App) {
    let Ok(value) = std::env::var("NVF_PLAY_TEST_MESH_BLEND_CONTINUITY") else { return };
    let Some((start, duration)) = value.split_once(':').and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?))) else {
        warn!("mesh terrain blend: continuity rejected aid; expected start-seconds:duration-seconds"); return;
    };
    if !start.is_finite() || !duration.is_finite() || start < 0.0 || !(1.0..=120.0).contains(&duration) { return; }
    let crossing = match std::env::var("NVF_PLAY_TEST_MESH_BLEND_CROSSING") {
        Err(_) => None,
        Ok(v) => match Crossing::parse(&v) {
            Some(c) => Some(c),
            None => { warn!("mesh terrain blend: continuity rejected crossing aid; expected y:z:delay/x[,delay/x...]"); return; }
        },
    };
    app.insert_resource(ContinuityProbe {
        crossing,
        start, duration, done: false, armed: false, witnesses: vec![], camera: None, colour: None, request: 0,
        selected: None, direction: None, previous_pending: false, recentres: 0, pending_reversals: 0,
        frames: 0, transitions: 0, pending: 0, retained: 0, pending_retained: 0, unwitnessed_pending: 0, replacements: 0,
        unpublished: 0, missing_enabled: 0, missing_prepared: 0, failures: 0, copied: 0, baked: 0,
    }).add_systems(PostUpdate, (continuity_probe, continuity_crossing).chain().after(crate::mesh_terrain_blend::BlendConversion).after(update_cache).run_if(crate::mesh_terrain_blend::active).run_if(resource_exists::<nvf_play::pause::PlayClock>));
}

/// [design] Test aid only (round 10a, the round-9b review's two notes):
/// `NVF_PLAY_TEST_MESH_BLEND_CROSSING=<y>:<z>:<delay>/<x>[,<delay>/<x>...]` (with the
/// continuity probe) puts the player's feet at game `(x, y, z)` step by step, on the
/// probe's own world clock and only after it armed: a step is due `delay` world seconds
/// after the previous step (after arming for the first), and every step after the first
/// also waits until the probe has seen, in a frame after the previous step, the selected
/// centre in the previous target's cell (its recentre) and pending work in the current
/// frame. So each reversal lands on a distinct later frame while a replacement is pending,
/// never in the same frame as the step before it. A step still waiting 10 world seconds
/// after it was due goes anyway and counts as a timeout. The staging go-to before arming
/// stays with `NVF_PLAY_TEST_GOTO`.
///
/// Round 10a review (P3): the replacement a step starts could settle before a later step
/// with a long delay came due, and that step would then wait for pending work that never
/// came, up to its timeout. So, until the next step is issued, the materials first seen
/// after a step are held unacknowledged ([`Crossing::hold`], mesh_terrain_blend.rs
/// `acknowledgeable`): that step's replacement stays pending until the driver has issued
/// the step after it, whatever its delay; retained clones, seen before, are never held, and
/// nothing is held after the last step or the probe's summary.
struct Crossing {
    y: f32, z: f32,
    steps: Vec<(f64, f32)>,
    next: usize,
    /// World seconds of the previous step (of arming, before the first).
    last_at: f64,
    last_frame: u64,
    last_target: Option<(i32, i32)>,
    timeouts: u64,
}
impl Crossing {
    const TIMEOUT: f64 = 10.0;
    fn parse(value: &str) -> Option<Self> {
        let mut parts = value.splitn(3, ':');
        let (y, z) = (parts.next()?.parse::<f32>().ok()?, parts.next()?.parse::<f32>().ok()?);
        let steps: Option<Vec<_>> = parts.next()?.split(',').map(|s| {
            let (delay, x) = s.split_once('/')?;
            let (delay, x) = (delay.trim().parse::<f64>().ok()?, x.trim().parse::<f32>().ok()?);
            (delay.is_finite() && delay >= 0.0 && x.is_finite()).then_some((delay, x))
        }).collect();
        let steps = steps.filter(|s| !s.is_empty())?;
        (y.is_finite() && z.is_finite()).then_some(Self { y, z, steps, next: 0, last_at: 0.0, last_frame: 0, last_target: None, timeouts: 0 })
    }
    /// Whether step `next` goes now: due, and (after the first) the previous step's
    /// recentre seen in a later probed frame with work pending now, or timed out. `None`:
    /// not yet; `Some(gated)`: go, `gated` false for a timeout.
    fn ready(&self, now: f64, frame: u64, selected: Option<(i32, i32)>, pending: bool) -> Option<bool> {
        let &(delay, _) = self.steps.get(self.next)?;
        let due = self.last_at + delay;
        if now < due { return None; }
        let seen = self.last_target.is_none_or(|target| frame > self.last_frame && selected == Some(target) && pending);
        if seen { Some(true) } else if now > due + Self::TIMEOUT { Some(false) } else { None }
    }
    /// The readiness hold after the steps issued so far (mesh_terrain_blend.rs
    /// `acknowledgeable`): the last step's number while another step follows, so the
    /// materials its replacement makes stay pending until that next step is issued; 0
    /// after the last step, so the sequence settles.
    fn hold(&self) -> u64 {
        if self.next < self.steps.len() { self.next as u64 } else { 0 }
    }
}

/// [design] The crossing steps (see [`Crossing`]), after the probe in the same frame, so
/// they read the frame's probed recentre and pending state.
fn continuity_crossing(
    mut probe: ResMut<ContinuityProbe>, time: nvf_play::pause::AidTime, cache: Res<SurfaceCache>,
    phys: Option<ResMut<nvf_play::phys::PhysState>>, player: Option<ResMut<nvf_play::player::Player>>,
    readiness: Res<crate::mesh_terrain_blend::BlendReadiness>,
) {
    let Some(now) = time.world_secs().map(f64::from) else { return };
    // The probe's summary ends the sequence: nothing is held after it.
    if probe.done { readiness.hold(0); return; }
    if !probe.armed { return; }
    let (frame, pending) = (probe.frames, probe.previous_pending);
    let Some(c) = probe.crossing.as_mut() else { return };
    let Some(gated) = c.ready(now, frame, cache.selected_centre, pending) else { return };
    let (Some(mut phys), Some(mut player)) = (phys, player) else { return };
    if phys.player.is_none() { return; }
    let x = c.steps[c.next].1;
    let at = [x, c.y, c.z];
    phys.place_player(at, 60.0, 1000.0);
    player.feet = phys.player.as_ref().map_or(at, |ch| ch.feet);
    c.timeouts += u64::from(!gated);
    c.next += 1;
    (c.last_at, c.last_frame, c.last_target) = (now, frame, Some(fnv_world::cell_of(x, c.y)));
    readiness.hold(c.hold());
    info!("mesh terrain blend: continuity crossing step={} x={x:.0} y={:.0} frame={frame} world-secs={now:.2} gated={gated} pending={pending} timeouts={}", c.next, c.y, c.timeouts);
    if c.next == c.steps.len() { info!("mesh terrain blend: continuity crossing done steps={} timeouts={}", c.next, c.timeouts); }
}
type ProbeShapes<'w, 's> = Query<'w, 's, (Entity, &'static nvf_assets::nif_surface_blend::BlendShape, &'static GlobalTransform, Option<&'static MeshMaterial3d<nvf_assets::nif_surface_blend::NifBlendMaterial>>)>;
#[allow(clippy::too_many_arguments)]
fn continuity_probe(
    mut probe: ResMut<ContinuityProbe>, time: nvf_play::pause::AidTime, cache: Res<SurfaceCache>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>, shapes: ProbeShapes,
    images: Res<Assets<Image>>, blends: Res<Assets<nvf_assets::nif_surface_blend::NifBlendMaterial>>,
    ready: Res<crate::mesh_terrain_blend::BlendReadiness>,
) {
    let Some(now) = time.world_secs().map(f64::from) else { return };
    if probe.done || now < probe.start { return; }
    if now >= probe.start + probe.duration || probe.frames >= 20000 {
        probe.done = true;
        let settled = cache.job.is_none() && cache.published.as_ref().is_some_and(|p| ready.settled(p.revision));
        info!("mesh terrain blend: continuity summary frames={} crossings={} pending-frames={} retained-lookups={} pending-retained-lookups={} unwitnessed-pending-frames={} replacements={} published-false={} missing-enabled={} missing-prepared={} failures={} copied={} baked={} settled={settled} recentres={} pending-reversals={}", probe.frames, probe.transitions, probe.pending, probe.retained, probe.pending_retained, probe.unwitnessed_pending, probe.replacements, probe.unpublished, probe.missing_enabled, probe.missing_prepared, probe.failures, probe.copied, probe.baked, probe.recentres, probe.pending_reversals);
        return;
    }
    let camera_cell = camera.single().ok().map(|c| { let p = bevy_to_game(c.translation()); fnv_world::cell_of(p.x, p.y) });
    if !probe.armed {
        let (Some(front), Some(centre)) = (&cache.published, camera_cell) else { return };
        for (entity, shape, transform, drawn) in &shapes {
            let Some(enabled) = &shape.enabled else { continue };
            if !ready.usable(enabled.id()) || drawn.is_none() { continue; }
            let point = bevy_to_game(transform.translation()).truncate();
            let grid = fnv_world::cell_of(point.x, point.y);
            if grid != centre { continue; }
            let Some(key) = cache.current.iter().find(|k| k.grid == grid) else { continue };
            let Some(data) = images.get(&front.height).and_then(|i| i.data.as_ref()) else { continue };
            if lookup_covered_status(front.layout, data, front.origin, point, front.resolution, front.coverage) != LookupStatus::Success { continue; }
            probe.witnesses.push((entity, key.clone(), point));
            if probe.witnesses.len() == 32 { break; }
        }
        if probe.witnesses.is_empty() { return; }
        probe.armed = true; probe.camera = camera_cell; probe.colour = Some(front.colour.id()); probe.selected = cache.selected_centre;
        // The crossing sequence's clock starts at arming (the world clock, as the probe's).
        if let Some(c) = probe.crossing.as_mut() { c.last_at = now; }
        info!("mesh terrain blend: continuity armed witnesses={}", probe.witnesses.len());
    }
    probe.frames += 1;
    let transition = matches!((camera_cell, probe.camera), (Some(a), Some(b)) if a != b);
    probe.transitions += u64::from(transition);
    probe.camera = camera_cell;
    let colour = cache.published.as_ref().map(|p| p.colour.id());
    if colour.is_some() && colour != probe.colour { probe.replacements += 1; probe.colour = colour; }
    let pending = cache.job.is_some() || cache.retirement > 0 || cache.published.as_ref().is_some_and(|p| !ready.settled(p.revision));
    let movement = probe.selected.zip(cache.selected_centre).filter(|(previous, selected)| previous != selected);
    if let Some((previous, selected)) = movement {
        let direction = ((selected.0 - previous.0).signum(), (selected.1 - previous.1).signum());
        let reversal = probe.direction.is_some_and(|d| d.0 * direction.0 + d.1 * direction.1 < 0);
        probe.pending_reversals += u64::from(reversal && probe.previous_pending);
        probe.recentres += 1;
        probe.direction = Some(direction);
    }
    probe.selected = cache.selected_centre;
    probe.previous_pending = pending;
    probe.pending += u64::from(pending);
    probe.unpublished += u64::from(cache.published.is_none());
    if cache.generation != probe.request {
        probe.request = cache.generation;
        if let Some(job) = &cache.job {
            probe.copied += job.copied; probe.baked += job.baked;
        }
    }
    let mut retained = 0;
    let mut missing_enabled = 0;
    let mut missing_prepared = 0;
    let mut failures = u64::from(camera_cell.is_none());
    for (entity, key, point) in &probe.witnesses {
        if !cache.current.contains(key) { continue; }
        retained += 1;
        let Ok((_, shape, _, drawn)) = shapes.get(*entity) else { missing_enabled += 1; continue };
        let Some(enabled) = shape.enabled.as_ref().filter(|h| drawn.is_some_and(|d| &d.0 == *h)) else { missing_enabled += 1; continue };
        if !ready.usable(enabled.id()) { missing_prepared += 1; }
        let Some(material) = blends.get(enabled) else { failures += 1; continue };
        let e = &material.extension;
        let Some(data) = e.mesh_terrain_height.as_ref().and_then(|h| images.get(h)).and_then(|i| i.data.as_ref()) else { failures += 1; continue };
        let origin = (e.mesh_terrain_origin.x as i32, e.mesh_terrain_origin.y as i32);
        let resolution = e.mesh_terrain_colour.as_ref().and_then(|h| images.get(h)).map_or(0, |i| i.texture_descriptor.size.width / 3);
        if resolution == 0 || lookup_covered_status(e.mesh_terrain_layout, data, origin, *point, resolution, e.mesh_terrain_origin.z as u32) != LookupStatus::Success { failures += 1; }
    }
    probe.retained += retained;
    if pending { probe.pending_retained += retained; probe.unwitnessed_pending += u64::from(retained == 0); }
    probe.missing_enabled += missing_enabled;
    probe.missing_prepared += missing_prepared;
    probe.failures += failures;
    info!("mesh terrain blend: continuity frame={} camera-cell={camera_cell:?} transition={transition} pending={pending} published={} retained={retained} missing-enabled={missing_enabled} missing-prepared={missing_prepared} failures={failures} copied={} baked={}", probe.frames, cache.published.is_some(), probe.copied, probe.baked);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use fnv_world::land::SIDE;
    /// The stored layout (smoothing off), as every test before #2149/#2661 packed.
    const STORED: HeightLayout = HeightLayout::STORED;
    fn surface(paths: [CapturePath; 4]) -> SurfaceCell {
        SurfaceCell { world: 0, grid: (-17, 0), heights: vec![42.5; SIDE * SIDE], quadrants: vec![], paths, content_revision: 0, sources: vec![], dependencies: default(), detail: default() }
    }
    /// [`surface`] at grid (0, 0) without heights (the caller gives them).
    fn surface_cell_with(paths: [CapturePath; 4]) -> SurfaceCell {
        SurfaceCell { grid: (0, 0), heights: vec![], ..surface(paths) }
    }
    /// The probe's classification, the same at every accepted resolution.
    fn status(data: &[u8], origin: (i32, i32), point: Vec2) -> LookupStatus {
        let [a, b, c] = RESOLUTIONS.map(|r| lookup_status(STORED, data, origin, point, r));
        assert!(a == b && b == c, "{point:?}: {a:?} {b:?} {c:?}");
        a
    }
    #[test]
    fn footprint_boundary_scales_with_resolution_in_negative_cells_too() {
        // q0 of slot (0, 0) has no colour. A point 3 units east of the quadrant edge: at 8
        // units per texel its bilinear footprint still reaches the invalid quadrant, at 4
        // and 2 units it does not. The same holds with every coordinate shifted to
        // negative cells.
        let mut data = empty_height_atlas();
        let cell = surface([CapturePath::Folded { layers: 0 }; 4]);
        let slots = HashSet::from([(0, 0), (1, 0), (0, 1), (1, 1)]);
        for &slot in &slots { encode_surface_cell(STORED, &mut data, slot, &cell, &slots); }
        encode_colour_mask(STORED, &mut data, (0, 0), 0b1110);
        for origin in [(0, 0), (-3, -2), (-18, -1)] {
            let shift = Vec2::new(origin.0 as f32, origin.1 as f32) * 4096.0;
            let point = shift + Vec2::new(2051.0, 1024.0);
            assert_eq!(lookup_status(STORED, &data, origin, point, 512), LookupStatus::RejectedFootprint, "{origin:?}");
            assert_eq!(lookup_status(STORED, &data, origin, point, 1024), LookupStatus::Success, "{origin:?}");
            assert_eq!(lookup_status(STORED, &data, origin, point, 2048), LookupStatus::Success, "{origin:?}");
            // One texel from the cell edge at each resolution, towards the resident east cell.
            for resolution in RESOLUTIONS {
                let texel = 4096.0 / resolution as f32;
                assert_eq!(lookup_status(STORED, &data, origin, shift + Vec2::new(4096.0 - 0.25 * texel, 1024.0), resolution), LookupStatus::Success, "{origin:?} {resolution}");
                assert_eq!(lookup_status(STORED, &data, origin, shift + Vec2::new(0.25 * texel, 3072.0), resolution), LookupStatus::Success, "{origin:?} {resolution}: taps clamp at the atlas's west edge (the shader's border fade covers it)");
            }
            // The missing quadrant itself, at every resolution.
            assert_eq!(status(&data, origin, shift + Vec2::splat(1024.0)), LookupStatus::InvalidColourQuadrant);
        }
    }
    #[test]
    fn resolutions_are_the_three_accepted_and_checked_against_the_device_limit() {
        assert_eq!(RESOLUTIONS, [512, 1024, 2048]);
        // Round 10b: the default is 512 colour texels per cell; the aid's 1024 and 2048 stay.
        assert_eq!(DEFAULT_RESOLUTION, 512);
        assert_eq!(SurfaceCache::default().resolution, 512);
        assert_eq!(RESOLUTIONS.map(atlas_width), [1536, 3072, 6144]);
        // Every level down to one texel per cell: a 3 by 3 atlas last.
        assert_eq!(RESOLUTIONS.map(colour_levels), [10, 11, 12]);
        assert!(RESOLUTIONS.iter().all(|&r| atlas_width(r) >> (colour_levels(r) - 1) == 3));
        assert_eq!(RESOLUTIONS.map(|r| (u64::from(atlas_width(r)).pow(2) * 8) >> 20), [18, 72, 288], "MiB of level 0 (the bake target's size)");
        assert_eq!(RESOLUTIONS.map(|r| mib(colour_bytes(r))), [24, 96, 384], "MiB of the RGBA16F atlas with its mip chain");
        assert_eq!(2 * mib(colour_bytes(DEFAULT_RESOLUTION)), 48, "the default front and back, against 144 MiB at 1024 without mips before round 10b");
        assert!(resolution_supported(2048, 8192) && resolution_supported(2048, 6144));
        assert!(!resolution_supported(2048, 6143) && !resolution_supported(1024, 2048));
        assert!(resolution_supported(512, 2048));
        assert!(!resolution_supported(768, 16384), "only the listed resolutions");
    }
    #[test]
    fn unsupported_quadrants_and_height_only_cells_keep_all_prepared_heights() {
        let mut data = empty_height_atlas();
        let slots = HashSet::from([(0, 0), (1, 0)]);
        let partial = surface([CapturePath::Folded { layers: 0 }, CapturePath::HeightOnly("missing-base-mesh"), CapturePath::Fallback { layers: 2 }, CapturePath::Folded { layers: 1 }]);
        let height_only = surface([CapturePath::HeightOnly("missing-base-mesh"); 4]);
        encode_surface_cell(STORED, &mut data, (0, 0), &partial, &slots);
        encode_surface_cell(STORED, &mut data, (1, 0), &height_only, &slots);
        assert_eq!(partial.colour_mask(), 0b1101);
        assert_eq!(height_only.colour_mask(), 0);
        for slot in [(0, 0), (1, 0)] { for y in 0..GRID { for x in 0..GRID { for triangle in 0..2 {
            let at = height_index(STORED, slot, (x, y), triangle, 0);
            assert_eq!(f32::from_le_bytes(data[at..at + 4].try_into().unwrap()), 42.5);
        }}}}
        assert_eq!(status(&data, (-18, -1), Vec2::new(-18.0 * 4096.0 + 1024.0, -3072.0)), LookupStatus::Success);
        assert_eq!(status(&data, (-18, -1), Vec2::new(-18.0 * 4096.0 + 3072.0, -3072.0)), LookupStatus::InvalidColourQuadrant);
        assert_eq!(status(&data, (-18, -1), Vec2::new(-17.0 * 4096.0 + 1024.0, -3072.0)), LookupStatus::InvalidColourQuadrant);
    }
    #[test]
    fn cpu_publication_probe_classifies_all_five_shader_outcomes() {
        let mut data = empty_height_atlas();
        let cell = surface([CapturePath::Folded { layers: 0 }; 4]);
        let slots = HashSet::from([(0, 0), (1, 0), (0, 1)]);
        for &slot in &slots { encode_surface_cell(STORED, &mut data, slot, &cell, &slots); }
        assert_eq!(status(&data, (0, 0), Vec2::new(-1.0, 100.0)), LookupStatus::OutOfRange);
        assert_eq!(status(&data, (0, 0), Vec2::splat(6144.0)), LookupStatus::InvalidHeight);
        // Round 10b review: the read crosses into valid neighbours, and the missing diagonal
        // cell (1, 1) (no colour captured) only loses its tap's weight: the fragment keeps the
        // colour of (0, 0), (1, 0) and (0, 1), the same value their fragments read beside it
        // (before round 10b the whole read was rejected; round 10b clamped it to the cell).
        assert_eq!(status(&data, (0, 0), Vec2::splat(4095.9)), LookupStatus::Success, "the missing diagonal's tap gets no weight");
        assert_eq!(status(&data, (0, 0), Vec2::splat(1024.0)), LookupStatus::Success);
        encode_colour_mask(STORED, &mut data, (0, 0), 0b1110);
        assert_eq!(status(&data, (0, 0), Vec2::splat(1024.0)), LookupStatus::InvalidColourQuadrant);
        assert_eq!(status(&data, (0, 0), Vec2::new(2048.1, 1024.0)), LookupStatus::RejectedFootprint, "one bilinear tap crosses into the invalid neighbour quadrant");
        assert_eq!(status(&data, (0, 0), Vec2::new(3072.0, 1024.0)), LookupStatus::Success, "supported neighbour quadrant is retained");
    }
    #[test]
    fn publication_line_names_paths_exclusions_probe_and_asset_ids() {
        let id = Handle::<Image>::default().id();
        let resident = vec![format!("(-17, 0):q=[{}] colour-mask=0x7", [CapturePath::Folded { layers: 0 }, CapturePath::Folded { layers: 2 }, CapturePath::Fallback { layers: 3 }, CapturePath::HeightOnly("missing-base-mesh")].map(CapturePath::label).join(","))];
        let line = publication_log(7, (-18, -1), &resident, &["(-18, -1):not-loaded".into()], "probe-slot=(1, 1)", (id, id), 1024);
        assert_eq!(line, format!("mesh terrain blend: publication generation=7 origin=(-18, -1) resident=[(-17, 0):q=[folded,folded+2-layers,fallback+3-layers,height-only:missing-base-mesh] colour-mask=0x7] excluded=[(-18, -1):not-loaded] probe-slot=(1, 1) colour={id:?} height={id:?} resolution=1024"));
        let mut data = empty_height_atlas();
        let cell = surface([CapturePath::Folded { layers: 0 }; 4]);
        encode_surface_cell(STORED, &mut data, (1, 1), &cell, &HashSet::from([(1, 1)]));
        let probe = publication_probe(STORED, &data, (-18, -1), Vec2::new(-17.0 * 4096.0 + 1024.0, 1024.0), 1024);
        assert!(probe.contains("probe-cell=(-17, 0) probe-slot=(1, 1)"));
        assert!(probe.contains(&format!("probe-height-bytes={:?} probe-status=Success probe-normal=(0.000, 0.000, 1.000)", 42.5f32.to_le_bytes())), "{probe}");
        assert!(publication_probe(STORED, &empty_height_atlas(), (-18, -1), Vec2::new(-17.0 * 4096.0 + 1024.0, 1024.0), 512).contains("probe-status=InvalidHeight probe-normal=none"));
    }
    /// [design] The probe's normal comes from the uploaded coefficients of the triangle
    /// under the point, both triangles of a square, in game space; Bevy world space is
    /// `game_to_bevy`'s rotation of it.
    #[test]
    fn probe_normal_reads_both_triangles_and_converts_like_the_world_root() {
        let spacing = fnv_world::land::SPACING;
        // Heights rising 1 per unit east and 0.5 per unit north, plus a kink so the two
        // triangles of each square differ: the north-east corner of every square is raised.
        let h: Vec<f32> = (0..SIDE * SIDE).map(|i| (i % SIDE) as f32 * spacing + (i / SIDE) as f32 * spacing * 0.5 + if (i % SIDE) % 2 == 1 && (i / SIDE) % 2 == 1 { 64.0 } else { 0.0 }).collect();
        let mut data = empty_height_atlas();
        encode_height_cell(STORED, &mut data, (1, 1), &h);
        let origin = (-18, -3);
        let square = Vec2::new(-17.0 * 4096.0, -2.0 * 4096.0);
        for (f, triangle) in [(Vec2::new(0.7, 0.2), 0), (Vec2::new(0.2, 0.7), 1)] {
            let n = probe_normal(STORED, &data, origin, square + f * spacing).unwrap();
            let (a, east, north, ne) = (h[0], h[1], h[SIDE], h[SIDE + 1]);
            let corners = if triangle == 0 {
                [Vec3::new(0.0, 0.0, a), Vec3::new(spacing, 0.0, east), Vec3::new(spacing, spacing, ne)]
            } else {
                [Vec3::new(0.0, 0.0, a), Vec3::new(spacing, spacing, ne), Vec3::new(0.0, spacing, north)]
            };
            assert!(n.dot(corners[1] - corners[0]).abs() < 1e-3 && n.dot(corners[2] - corners[0]).abs() < 1e-3, "triangle {triangle}: {n:?}");
            let world = nvf_assets::nif_surface_blend::game_to_world_normal(n);
            assert!((world - crate::world::game_to_bevy().rotation * n).length() < 1e-5);
            assert!(world.y > 0.0, "up in Bevy space");
        }
        assert_ne!(probe_normal(STORED, &data, origin, square + Vec2::new(0.7, 0.2) * spacing), probe_normal(STORED, &data, origin, square + Vec2::new(0.2, 0.7) * spacing));
        assert_eq!(probe_normal(STORED, &data, origin, square - Vec2::splat(4096.0 * 2.0)), None, "outside the atlas");
        assert_eq!(probe_normal(STORED, &data, (-17, -2), square), None, "slot without heights");
    }
    #[test]
    fn production_render_layer_reservations_are_distinct() {
        let layers = [
            ("world", 0), ("water", crate::reflection::WATER_LAYER),
            ("shadow", crate::sky::SHADOW_LAYER), ("arms", crate::sky::ARMS_LAYER),
            ("body", crate::sky::BODY_LAYER), ("lock", crate::lockpick_view::LOCK_LAYER),
            ("terminal", crate::terminal_view::MONITOR_LAYER), ("blackjack", crate::blackjack_view::TABLE_LAYER),
            ("roulette", crate::roulette_view::ROULETTE_LAYER), ("poker", crate::poker_view::POKER_LAYER),
            ("decal", crate::decals::DECAL_LAYER), ("scope", crate::scope_view::SCOPE_LAYER),
            ("vigor", nvf_play::chargen::vigor::VIGOR_LAYER), ("creator", crate::creator_view::STUDIO_LAYER),
            ("vats", crate::vats_highlight::MASK_LAYER), ("bake", BAKE_LAYER),
            ("overlay_pre_is", crate::camera_overlay::OVERLAY_PRE_IS_LAYER),
            ("overlay_pre_ui", crate::camera_overlay::OVERLAY_PRE_UI_LAYER),
            ("culled", crate::rooms::CULL_LAYER),
            ("volumetric clouds", crate::volumetric_clouds::VOLUME_LAYER),
            ("flat clouds beside the volume", crate::volumetric_clouds::FLAT_CLOUD_LAYER),
        ];
        for (i, a) in layers.iter().enumerate() {
            for b in &layers[i + 1..] { assert_ne!(a.1, b.1, "{} and {}", a.0, b.0); }
        }
        // RenderLayers::layer literal calls in particles.rs/effect_shaders.rs are test
        // fixtures, not further production reservations. Multi-layer views combine these.
    }
    #[test]
    fn l_shaped_residency_includes_cardinals_but_rejects_the_diagonal_tap() {
        let resident = HashSet::from([(0, 0), (1, 0), (0, 1)]);
        let mask = residency_mask((0, 0), &resident);
        for bit in [4, 5, 7] { assert_ne!(mask & (1 << bit), 0); }
        assert_eq!(mask & (1 << 8), 0, "northeast bilinear tap must be rejected");
        let mut full = resident.clone(); full.insert((1, 1));
        assert_ne!(residency_mask((0, 0), &full) & (1 << 8), 0);
        let negative = resident.iter().map(|&(x, y)| (x - 2, y - 1)).collect();
        assert_eq!(mask, residency_mask((-2, -1), &negative));
    }
    #[test]
    fn cache_origin_uses_negative_floor_and_covers_all_slots() {
        assert_eq!(SurfaceCache::default().selected_origin(Vec2::new(-0.01, -0.01)), (-2, -2));
        assert_eq!(SurfaceCache::default().selected_origin(Vec2::new(-8192.0, -4096.0)), (-3, -2));
        assert_eq!(SurfaceCache::default().selected_origin(Vec2::ZERO), (-1, -1));
        let origin = SurfaceCache::default().selected_origin(Vec2::new(-8192.0, -4096.0));
        for y in 0..3 { for x in 0..3 {
            let cell = (origin.0 + x, origin.1 + y);
            let point = Vec2::new(cell.0 as f32 * 4096.0 + 64.0, cell.1 as f32 * 4096.0 + 64.0);
            assert_eq!(fnv_world::cell_of(point.x, point.y), cell);
        }}
    }
    #[test]
    fn all_atlas_slots_and_plane_bytes_round_trip() {
        let h: Vec<_> = (0..SIDE * SIDE).map(|i| (i as f32 * 0.125) - 123.75).collect();
        let mut data = vec![0; STORED.width() * STORED.rows() * 4];
        for y in 0..3 { for x in 0..3 {
            encode_height_cell(STORED, &mut data, (x, y), &h);
        }}
        for y in 0..3 { for x in 0..3 { for sy in 0..GRID { for sx in 0..GRID {
            let i = sy * SIDE + sx;
            let planes = height_planes(h[i], h[i + 1], h[i + SIDE], h[i + SIDE + 1]);
            for (triangle, plane) in planes.iter().enumerate() { for (c, value) in plane.iter().enumerate() {
                let at = height_index(STORED, (x, y), (sx, sy), triangle, c);
                let bytes: [u8; 4] = data[at..at + 4].try_into().unwrap();
                let loaded = bytes.map(|b| ((b as f32 / 255.0) * 255.0).round() as u8);
                assert_eq!(f32::from_le_bytes(loaded).to_bits(), value.to_bits());
            }}
        }}}}
    }
    #[test]
    fn receipt_systems_reset_partial_frames_and_freeze_submitted_view() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.init_resource::<bevy::render::camera::SortedCameras>();
        let a = app.world_mut().spawn_empty().id();
        let b = app.world_mut().spawn_empty().id();
        let drawn = Arc::new(Mutex::new(HashSet::new()));
        let submitted = Arc::new(Mutex::new(None));
        let expected = HashSet::from([a, b]);
        let transfers = Transfers { source: None, destination: default(), cells: vec![], bake: Some(default()), baked: vec![(0, 0)], resolution: 512 };
        let complete = Arc::new(AtomicBool::new(false));
        let camera = app.world_mut().spawn(BakeView {
            receipt: Receipt { epoch: 1, request: 7, destination: Handle::<Image>::default().id(), source_revision: 0 }, height: default(), resources_ready: Arc::new(AtomicBool::new(true)), transfers, expected: expected.clone(), complete: complete.clone(),
            drawn: drawn.clone(), submitted: submitted.clone(),
        }).id();
        app.world_mut().resource_mut::<bevy::render::camera::SortedCameras>().0.push(bevy::render::camera::SortedCamera {
            entity: camera, order: 0, target: None, output_mode: default(),
        });
        // Round 10b: the assembly (copies and mip passes) runs only in a frame where every
        // planned draw drew (`draws_ready`); the drawn set restarts every frame.
        drawn.lock().unwrap().insert(a);
        assert!(!draws_ready(&expected, &drawn.lock().unwrap()));
        app.world_mut().run_system_once(submit_receipts).unwrap();
        assert_eq!(*submitted.lock().unwrap(), None);
        app.world_mut().run_system_once(reset_receipts).unwrap();
        assert!(drawn.lock().unwrap().is_empty());
        drawn.lock().unwrap().insert(b);
        assert!(!draws_ready(&expected, &drawn.lock().unwrap()), "partial frames must not accumulate");
        drawn.lock().unwrap().insert(a);
        assert!(draws_ready(&expected, &drawn.lock().unwrap()));
        app.world_mut().run_system_once(submit_receipts).unwrap();
        assert_eq!(*submitted.lock().unwrap(), None, "all draws alone are no receipt: the assembly's submission must be queued first");
        // What `assemble_atlas` does after queueing its submission.
        complete.store(true, Ordering::Release);
        app.world_mut().run_system_once(submit_receipts).unwrap();
        assert_eq!(submitted.lock().unwrap().as_ref().map(|r| r.request), Some(7));
        app.world_mut().run_system_once(reset_receipts).unwrap();
        assert!(app.world().resource::<bevy::render::camera::SortedCameras>().0.is_empty(), "a later pass must not clear the completed bake target");
    }
    /// [design] Production path regression: `spawn_terrain_with` captures every quadrant's
    /// whole draw stack (folded or fallback base plus the blended layers drawn over it) and
    /// `update_cache` publishes the cell's complete heights with all four colour quadrants
    /// valid, waiting for one receipt per near draw. A return to whole-cell or
    /// per-layered-quadrant exclusion fails here. Synthetic land and a synthetic BC1
    /// texture; no game data.
    #[test]
    fn spawned_layered_quadrants_publish_whole_heights_full_colour_and_every_draw() {
        use bevy::ecs::system::RunSystemOnce;
        use fnv_world::land::{Land, Layer, QUAD_SIDE};
        let grid = (-17, 0);
        let heights: Vec<f32> = (0..SIDE * SIDE).map(|i| ((i * 13 + i / SIDE * 7) % 59) as f32 * 2.5 - 40.0).collect();
        // Layers paint only the quadrant's middle vertex, so stitching adds none across
        // the quadrant edges: q0 has 7 layers (6 fold, 1 stays blended where folding is
        // on), q1 2, q2 none, q3 no base record and no layers.
        let centre = ((QUAD_SIDE / 2) * QUAD_SIDE + QUAD_SIDE / 2) as u16;
        let mut layers = Vec::new();
        for (q, count) in [(0u8, 7i16), (1, 2), (2, 0)] {
            layers.push(Layer { quadrant: q, texture: None, layer: -1, base: true, alpha: vec![] });
            for layer in 0..count {
                layers.push(Layer { quadrant: q, texture: None, layer, base: false, alpha: vec![(centre, 0.75)] });
            }
        }
        let land = Land { heights: Some(heights.clone()), layers, ..default() };
        for (folds, expected) in [
            (true, [CapturePath::Folded { layers: 1 }, CapturePath::Folded { layers: 0 }, CapturePath::Fallback { layers: 0 }, CapturePath::Fallback { layers: 0 }]),
            (false, [CapturePath::Fallback { layers: 7 }, CapturePath::Fallback { layers: 2 }, CapturePath::Fallback { layers: 0 }, CapturePath::Fallback { layers: 0 }]),
        ] {
            let (mut app, _files) = crate::outfits::tests::asset_fixture();
            app.insert_resource(crate::outfits::tests::empty_world_data())
                .insert_resource(crate::terrain::TerrainSettings { tile: 256.0 })
                .init_resource::<crate::terrain::TerrainCache>()
                .init_resource::<Assets<TerrainMaterial>>()
                .init_resource::<Assets<SurfaceCaptureMaterial>>()
                .init_resource::<SurfaceCache>();
            app.world_mut().resource_mut::<nvf_assets::TextureCache>()
                .offer(fnv_vfs::texture_path(fnv_world::land::DEFAULT_DIFFUSE), synthetic_dds(), 0.0);
            let cell = app.world_mut().spawn_empty().id();
            let prep_land = land.clone();
            app.world_mut().run_system_once(move |mut commands: Commands, world: Res<WorldData>, mut terrain: crate::terrain::TerrainMaterials, mut models: nvf_assets::model::ModelCtx| {
                let prep = crate::terrain::prepare_land(prep_land.clone(), &[None, None, None, None], grid, 256.0);
                crate::terrain::spawn_terrain_with(&mut commands, cell, &world, prep, &mut crate::terrain::TerrainCtx { tm: &mut terrain, assets: &mut models }, folds, true);
            }).unwrap();
            let near_draws = app.world_mut().query::<(&ChildOf, &crate::terrain::LandMesh)>().iter(app.world()).filter(|(parent, _)| parent.parent() == cell).count();
            // Round 7: the capture of a cell drawn while the blend was off (the settings row
            // turned on later) is the same stack, and draws nothing near again.
            let again = land.clone();
            let captured = app.world_mut().run_system_once(move |world: Res<WorldData>, mut terrain: crate::terrain::TerrainMaterials, mut models: nvf_assets::model::ModelCtx| {
                let prep = crate::terrain::prepare_land(again.clone(), &[None, None, None, None], grid, 256.0);
                crate::terrain::capture_surface(&world, prep, &mut crate::terrain::TerrainCtx { tm: &mut terrain, assets: &mut models }, folds)
            }).unwrap().expect("heights give a capture");
            let land_meshes = app.world_mut().query::<&crate::terrain::LandMesh>().iter(app.world()).count();
            assert_eq!(land_meshes, near_draws, "folds={folds}: the capture alone spawns no near draw");
            let surface = app.world().get::<SurfaceCell>(cell).expect("capture inserts the cell's surface");
            assert_eq!((captured.paths, captured.heights.len(), captured.quadrants.len()), (surface.paths, surface.heights.len(), surface.quadrants.len()), "folds={folds}");
            let kind = |d: &CaptureDraw| match d { CaptureDraw::Folded(m) => (true, false, m.base.depth_bias.to_bits()), CaptureDraw::Single(m) => (false, m.layer, m.depth_bias.to_bits()) };
            assert!(captured.quadrants.iter().zip(&surface.quadrants).all(|((_, a), (_, b))| kind(a) == kind(b)), "folds={folds}: the same copies in the same order");
            assert_eq!(surface.paths, expected, "folds={folds}");
            assert_eq!(surface.colour_mask(), 0xf, "folds={folds}: every quadrant's colour is captured");
            assert_eq!(surface.quadrants.len(), near_draws, "folds={folds}: one bake copy per near draw");
            let blended_layers: usize = expected.iter().map(|p| match p { CapturePath::Folded { layers } | CapturePath::Fallback { layers } => *layers, CapturePath::HeightOnly(_) => 0 }).sum();
            // Folded bases keep their TerrainMaterial (unlit bake variant); every other base
            // and every layer is a single-texture capture, so a device without bindless
            // (folds=false) never meets TerrainMaterial's 19-texture layout (round-4 P2).
            let folded: Vec<_> = surface.quadrants.iter().filter_map(|(_, d)| match d { CaptureDraw::Folded(m) => Some(m), CaptureDraw::Single(_) => None }).collect();
            let single: Vec<_> = surface.quadrants.iter().filter_map(|(_, d)| match d { CaptureDraw::Single(m) => Some(m), CaptureDraw::Folded(_) => None }).collect();
            let folded_paths = expected.iter().filter(|p| matches!(p, CapturePath::Folded { .. })).count();
            assert_eq!(folded.len(), folded_paths, "folds={folds}: one folded copy per folded base, none without folding");
            assert!(folded.iter().all(|m| m.extension.surface_bake && m.extension.burn.is_none() && !m.base.fog_enabled), "folded copies use the unlit bake variant");
            assert_eq!(single.iter().filter(|m| m.layer).count(), blended_layers, "folds={folds}");
            assert!(single.iter().filter(|m| m.layer).all(|m| m.alpha_mode() == AlphaMode::Blend && m.values.uv_offset_flags.w == 1.0), "layer copies keep the near layers' blending");
            assert!(single.iter().filter(|m| !m.layer).all(|m| m.alpha_mode() == AlphaMode::Opaque && m.depth_bias == 0.0), "base copies stay opaque");
            assert!(single.iter().all(|m| m.diffuse.is_some() && m.values.uv_offset_flags.z == 1.0), "the synthetic default texture is bound");
            // Layer copies keep the near layers' sort order and the rasteriser part of their bias.
            // In drawing order, each quadrant's layer copies follow its base with rising bias.
            let mut previous: Option<f32> = None;
            for (_, draw) in &surface.quadrants {
                match draw {
                    CaptureDraw::Single(m) if m.layer => {
                        assert!(previous.is_some_and(|p| m.depth_bias > p), "folds={folds}: a layer copy follows its base or a lower layer");
                        previous = Some(m.depth_bias);
                    }
                    _ => previous = Some(f32::NEG_INFINITY),
                }
            }
            let biases: Vec<_> = single.iter().filter(|m| m.layer).map(|m| (m.depth_bias, SurfaceCaptureKey::from(*m).raster_bias)).collect();
            assert!(biases.iter().all(|&(sort, raster)| nvf_assets::nif_material::sorts_first(sort) && raster == nvf_assets::nif_material::raster_bias(sort as i32)), "{biases:?}");
            assert!(surface.quadrants.iter().all(|(mesh, _)| match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
                Some(VertexAttributeValues::Float32x3(p)) => p.iter().all(|v| v[2] == 0.0),
                _ => false,
            }), "copies are flat");

            // The publication: the production cache update around a camera in the cell.
            app.insert_resource(Streamer {
                radius: 1, loaded: [(grid, cell)].into(), fades: default(), fade_secs: 0.0, settled: true,
                arrival: None, per_frame: 1, spawning: default(), dying: default(), ahead: default(),
                hide: default(), interior: None, light_scale: 1.0, managed: default(), manage_actors: false,
                stats: default(), buffer: default(), centre: None, waiting: default(), model_overrides: default(), coop_areas: Vec::new(), travels: 0,
            });
            let eye = Vec3::new(grid.0 as f32 * 4096.0 + 2048.0, grid.1 as f32 * 4096.0 + 2048.0, 600.0);
            app.world_mut().spawn((Camera3d::default(), crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, GlobalTransform::from_translation(Vec3::new(eye.x, eye.z, -eye.y))));
            // Round 10a: with the detail capability, folded or not.
            app.world_mut().resource_mut::<SurfaceCache>().detail = true;
            app.world_mut().run_system_once(update_cache).unwrap();
            let cache = app.world().resource::<SurfaceCache>();
            let job = cache.job.as_ref().expect("a bake is pending for the resident cell");
            assert_eq!((job.publication.origin, job.publication.cells), ((grid.0 - 1, grid.1 - 1), 1));
            assert!(job.report.contains("colour-mask=0xf"), "{}", job.report);
            // Every quadrant's detail is admitted (q0: six folded and one overflow layer with
            // folding, seven overflow layers without; q3 without a base record draws the
            // default texture), sharing one image and one sampler description.
            assert!(job.report.ends_with(" detail=layers palette-images=1 palette-samplers=1 detail-quadrants=4 colour-quadrants=[]"), "folds={folds}: {}", job.report);
            assert_eq!((job.publication.detail.len(), job.publication.palette.images.len()), (4, 1));
            {
                let image = app.world().resource::<Assets<Image>>().get(&job.publication.height).unwrap();
                assert_eq!(image.texture_descriptor.size.height as usize, STORED.rows() + STORED.detail_rows(), "the detail rows follow the heights");
                let data = image.data.as_ref().unwrap();
                assert_eq!(detail_mask(STORED, data, (1, 1)), 0xf);
                assert_eq!(detail_mask(STORED, data, (0, 0)), 0, "a slot without a cell");
                let q0 = detail_word(STORED, data, nvf_assets::nif_surface_blend::detail_quadrant_word(detail_entry((1, 1), 0)));
                assert_eq!(((q0 >> 8) & 15, (q0 >> 12) & 15), (if folds { 6 } else { 0 }, 7), "folds={folds}: folded and overflow overlays");
                assert!(job.report.contains("probe-detail-mask=0xf"), "{}", job.report);
            }
            let height = app.world().resource::<Assets<Image>>().get(&job.publication.height).unwrap().data.clone().unwrap();
            let origin = job.publication.origin;
            for y in 0..GRID { for x in 0..GRID { for triangle in 0..2 {
                let at = height_index(STORED, (1, 1), (x, y), triangle, 0);
                assert_eq!(f32::from_le_bytes(height[at..at + 4].try_into().unwrap()), heights[y * SIDE + x], "square ({x}, {y})");
            }}}
            for q in 0..4 {
                let point = Vec2::new(grid.0 as f32 * 4096.0 + 1024.0 + 2048.0 * (q % 2) as f32, grid.1 as f32 * 4096.0 + 1024.0 + 2048.0 * (q / 2) as f32);
                assert_eq!(status(&height, origin, point), LookupStatus::Success, "folds={folds} quadrant {q}");
            }
            let expected_draws = app.world_mut().query::<&BakeView>().single(app.world()).unwrap().expected.len();
            assert_eq!(expected_draws, near_draws, "folds={folds}: the receipt waits for every copy, blended layers included");
            let terrain_copies = app.world_mut().query_filtered::<(), (With<RenderLayers>, With<MeshMaterial3d<TerrainMaterial>>)>().iter(app.world()).count();
            let single_copies = app.world_mut().query_filtered::<(), (With<RenderLayers>, With<MeshMaterial3d<SurfaceCaptureMaterial>>)>().iter(app.world()).count();
            assert_eq!((terrain_copies, single_copies), (folded_paths, near_draws - folded_paths), "folds={folds}: bake entities by material type");
            let colour = app.world().resource::<Assets<Image>>().get(&app.world().resource::<SurfaceCache>().job.as_ref().unwrap().publication.colour).unwrap();
            assert_eq!(colour.texture_descriptor.size.width, atlas_width(DEFAULT_RESOLUTION), "the default 512-texel cells (round 10b)");
            // Round 10b: the published atlas has its whole mip chain and is never a render
            // target; the bake camera draws into a single-level target of the same size.
            assert_eq!(colour.texture_descriptor.mip_level_count, colour_levels(DEFAULT_RESOLUTION));
            assert!(colour.data.is_none(), "no CPU copy of the atlas");
            assert!(colour.texture_descriptor.usage.contains(TextureUsages::STORAGE_BINDING | TextureUsages::COPY_DST | TextureUsages::COPY_SRC | TextureUsages::TEXTURE_BINDING));
            assert!(!colour.texture_descriptor.usage.contains(TextureUsages::RENDER_ATTACHMENT));
            let view = app.world_mut().query::<&BakeView>().single(app.world()).unwrap();
            let bake = app.world().resource::<Assets<Image>>().get(view.transfers.bake.as_ref().expect("the bake target")).unwrap();
            assert_eq!((bake.texture_descriptor.size.width, bake.texture_descriptor.mip_level_count), (atlas_width(DEFAULT_RESOLUTION), 1));
            assert!(bake.texture_descriptor.usage.contains(TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC));
            assert_eq!(view.transfers.baked, vec![colour_rectangle(grid, origin, DEFAULT_RESOLUTION)], "the baked cell's level-0 corner, its levels filtered after the copy");
            assert!(view.transfers.cells.is_empty() && view.transfers.source.is_none());
        }
    }
    /// A detail capture whose base reads `base` and `normal` and whose overlays read the
    /// given images (opacities and vertices are irrelevant to the palette).
    fn layers(base: Handle<Image>, normal: Handle<Image>, overlays: Vec<(Handle<Image>, Option<Handle<Image>>)>) -> DetailCapture {
        let n = nvf_assets::nif_surface_blend::QUAD_VERTICES;
        DetailCapture::Layers(Box::new(QuadrantDetail {
            base: DetailBase::Single(Handle::default()), overflow: vec![], cell_local: false, tile: 256.0,
            tint: vec![[1.0; 3]; n], normal: vec![[0.0, 0.0, 1.0]; n], tangent: vec![[1.0, 0.0, 0.0]; n], tangent_sign: -1.0,
            opacity: vec![vec![0.5; n]; overlays.len()],
            resolved: Ok(ResolvedDetail { base_diffuse: Some(base), base_normal: Some(normal), base_rgb: Vec3::ONE, uv: bevy::math::Affine2::IDENTITY, overlays, folded: 0, flip_y: true, features: default() }),
        }))
    }
    fn sampled(images: &mut Assets<Image>, anisotropy: u16) -> Handle<Image> {
        let mut image = Image::default();
        image.sampler = ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor { anisotropy_clamp: anisotropy, ..bevy::image::ImageSamplerDescriptor::linear() });
        images.add(image)
    }

    /// [design] Round 10a: the palette admits whole quadrants in a deterministic order
    /// (previous admissions of unchanged cells first, then grid and quadrant), refuses a
    /// quadrant whose images or sampler descriptions would overflow it, with the reason,
    /// shares images and sampler descriptions between quadrants, and the cell's
    /// dependencies include its normal maps.
    #[test]
    fn palette_admits_whole_quadrants_in_order_and_refuses_overflow() {
        use nvf_assets::nif_surface_blend::{PALETTE_IMAGES, PALETTE_SAMPLERS};
        let mut images = Assets::<Image>::default();
        // Sixteen quadrants of 26 distinct images each (a base, its normal map and twelve
        // overlays with normal maps): 14 fit 384 (364), the 15th and 16th do not.
        let mut cells = Vec::new();
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let mut cell = surface([CapturePath::Folded { layers: 12 }; 4]);
            cell.grid = (x, y);
            for detail in &mut cell.detail {
                let overlays = (0..12).map(|_| (sampled(&mut images, 8), Some(sampled(&mut images, 8)))).collect();
                *detail = layers(sampled(&mut images, 8), sampled(&mut images, 8), overlays);
            }
            cell.refresh_dependencies();
            cells.push(cell);
        }
        assert_eq!(cells[0].dependencies.len(), 4 * 26, "every detail image is a dependency, normal maps included");
        let p = DetailPalette::assemble(&cells, &images, &[]);
        assert_eq!((p.admitted.len(), p.palette.images.len(), p.palette.samplers.len()), (14, 364, 1));
        assert!(p.palette.images.len() <= PALETTE_IMAGES);
        assert_eq!(p.refused, vec![(((1, 1), 2), "palette-images-full"), (((1, 1), 3), "palette-images-full")]);
        assert_eq!(p.admitted.first().map(|a| a.0), Some(((0, 0), 0)), "grid order");
        // A retained admission comes first, so another quadrant is refused instead.
        let retained = [((1, 1), 3)];
        let r = DetailPalette::assemble(&cells, &images, &retained);
        assert_eq!(r.admitted.first().map(|a| a.0), Some(((1, 1), 3)));
        assert_eq!(r.refused.iter().map(|(at, _)| *at).collect::<Vec<_>>(), vec![((1, 1), 1), ((1, 1), 2)]);
        // Shared images and descriptions: two quadrants reading the same textures add one set.
        let (base, normal, overlay) = (sampled(&mut images, 8), sampled(&mut images, 8), sampled(&mut images, 8));
        let mut shared = surface([CapturePath::Folded { layers: 1 }; 4]);
        for detail in &mut shared.detail { *detail = layers(base.clone(), normal.clone(), vec![(overlay.clone(), None)]); }
        let s = DetailPalette::assemble([&shared], &images, &[]);
        assert_eq!((s.admitted.len(), s.palette.images.len(), s.palette.samplers.len()), (4, 3, 1));
        let words = s.admitted[3].1.words();
        assert_eq!((words[1], words[2], words[15]), (1 << 16, 0, 2), "palette numbers: base 0, its normal 1, overlay 2; sampler 0 for both");
        // Sampler descriptions: two per quadrant (diffuse and normal) fit sixteen in eight
        // quadrants; the ninth is refused.
        let mut many = Vec::new();
        for x in 0..3 {
            let mut cell = surface([CapturePath::Folded { layers: 0 }; 4]);
            cell.grid = (x, 0);
            for q in 0..4 {
                let k = (x * 4 + q) as u16;
                cell.detail[q as usize] = layers(sampled(&mut images, 2 * k + 1), sampled(&mut images, 2 * k + 2), vec![]);
            }
            many.push(cell);
        }
        let m = DetailPalette::assemble(&many, &images, &[]);
        assert_eq!((m.admitted.len(), m.palette.samplers.len()), (8, PALETTE_SAMPLERS));
        assert!(m.refused.iter().all(|(_, r)| *r == "palette-samplers-full") && m.refused.len() == 4);
        // A colour-only capture keeps its reason; a missing image refuses the quadrant.
        let mut colour = surface([CapturePath::Folded { layers: 0 }; 4]);
        colour.detail[1] = DetailCapture::Colour("overlays-over-cap");
        colour.detail[2] = layers(Handle::default(), base.clone(), vec![]);
        let c = DetailPalette::assemble([&colour], &images, &[]);
        assert!(c.refused.contains(&(((-17, 0), 1), "overlays-over-cap")) && c.refused.contains(&(((-17, 0), 2), "image-not-loaded")));
        assert!(c.refused.contains(&(((-17, 0), 0), "not-captured")));
    }

    /// [design] Round 10a: without the detail capability (or with the aid forcing colour)
    /// a publication's height atlas has no detail rows, no palette and no detail masks.
    #[test]
    fn colour_only_publications_carry_no_detail() {
        let (mut app, _files) = cache_fixture();
        let cache = app.world().resource::<SurfaceCache>();
        assert!(!cache.detail);
        let published = cache.published.clone().unwrap();
        assert!(published.palette.images.is_empty() && published.detail.is_empty());
        let image = app.world_mut().resource_mut::<Assets<Image>>().get(&published.height).unwrap().clone();
        assert_eq!(image.texture_descriptor.size.height as usize, STORED.rows());
        let data = image.data.unwrap();
        for y in 0..3 { for x in 0..3 { assert_eq!(detail_mask(STORED, &data, (x, y)), 0); } }
    }

    /// [design] The crossing aid's steps: on the probe's clock after arming, each later step
    /// only after the previous recentre was seen in a later frame with work pending, so two
    /// steps never share a frame; a timeout after 10 world seconds.
    #[test]
    fn crossing_steps_wait_for_the_seen_recentre_and_pending_work() {
        assert!(Crossing::parse("4800:8600").is_none() && Crossing::parse("4800:8600:").is_none() && Crossing::parse("4800:8600:x/1").is_none());
        assert!(Crossing::parse("4800:8600:-1/5").is_none(), "no negative delay");
        let mut c = Crossing::parse("4800:8600:4/-65152,0.3/-65920").unwrap();
        assert_eq!((c.y, c.z, c.steps.clone()), (4800.0, 8600.0, vec![(4.0, -65152.0), (0.3, -65920.0)]));
        c.last_at = 10.0; // armed at 10 s
        assert_eq!(c.ready(13.9, 5, None, false), None, "the first step waits for its delay after arming");
        assert_eq!(c.ready(14.0, 5, None, false), Some(true), "and nothing else");
        (c.next, c.last_at, c.last_frame, c.last_target) = (1, 14.0, 5, Some(fnv_world::cell_of(-65152.0, 4800.0)));
        let east = Some((-16, 1));
        assert_eq!(c.last_target, east);
        assert_eq!(c.ready(14.5, 5, east, true), None, "never in the frame of the previous step");
        assert_eq!(c.ready(14.5, 6, Some((-17, 1)), true), None, "the recentre not yet seen");
        assert_eq!(c.ready(14.5, 6, east, false), None, "no pending work");
        assert_eq!(c.ready(14.2, 6, east, true), None, "the delay");
        assert_eq!(c.ready(14.5, 6, east, true), Some(true));
        assert_eq!(c.ready(24.4, 9, None, false), Some(false), "a timeout 10 s after it was due");
        c.next = 2;
        assert_eq!(c.ready(100.0, 50, east, true), None, "done");
    }

    /// [design] Round 10a review (P3): a delayed step after the previous step's work was
    /// prepared. The materials first seen after step 1 are held unacknowledged until step 2
    /// is issued, so its replacement is still pending when the 3-second step comes due and
    /// the step goes gated, without a timeout; then they are released, and nothing is held
    /// after the last step. Materials seen before a step (retained clones) are never held.
    #[test]
    fn delayed_steps_find_the_previous_work_still_pending_after_it_prepared() {
        use crate::mesh_terrain_blend::acknowledgeable;
        let mut c = Crossing::parse("4800:8600:4/-65152,3/-65920,0.3/-65152").unwrap();
        c.last_at = 10.0;
        assert_eq!(c.hold(), 0, "nothing held before the first step");
        assert_eq!(c.ready(14.0, 5, None, false), Some(true));
        (c.next, c.last_at, c.last_frame, c.last_target) = (1, 14.0, 5, Some(fnv_world::cell_of(-65152.0, 4800.0)));
        let hold = c.hold();
        assert_eq!(hold, 1);
        // The replacement's clone, first seen after step 1, prepared and bound 200 frames
        // later (past the 120-frame delay aid), is still held; a clone seen before is not.
        let (delay, candidate, retained) = (120, hold, 0);
        assert!(!acknowledgeable(200, delay, candidate, hold), "held: its work stays pending");
        assert!(acknowledgeable(200, delay, retained, hold), "a retained clone is never held");
        assert!(!acknowledgeable(100, delay, retained, 0), "the delay aid still applies");
        // So when the delayed step comes due (3 s later, long after the preparation) the
        // probe still sees pending work and the step goes gated.
        let east = c.last_target;
        let pending = !acknowledgeable(200, delay, candidate, hold);
        assert_eq!(c.ready(16.9, 400, east, pending), None, "its delay");
        assert_eq!(c.ready(17.0, 400, east, pending), Some(true), "gated, no timeout");
        (c.next, c.last_at, c.last_frame, c.last_target) = (2, 17.0, 400, Some(fnv_world::cell_of(-65920.0, 4800.0)));
        assert_eq!(c.hold(), 2);
        assert!(acknowledgeable(201, delay, candidate, c.hold()), "released once the next step is issued");
        c.next = 3;
        assert_eq!(c.hold(), 0, "nothing held after the last step");
        assert!(acknowledgeable(1, 0, 2, c.hold()));
    }

    /// [design] Round 10a review (P2): a quadrant whose near draw reads with a sampler the
    /// detail does not reproduce keeps the colour cache (`sampler-mismatch`): folded
    /// overlays on a textureless base (Bevy's fallback sampler), and overflow layers whose
    /// diffuse or normal image's description differs from the detail's diffuse sampler.
    #[test]
    fn samplers_the_detail_does_not_reproduce_keep_the_colour_cache() {
        let mut images = Assets::<Image>::default();
        let (base, normal) = (sampled(&mut images, 8), sampled(&mut images, 8));
        let capture = |base: Option<Handle<Image>>, overlays: Vec<(Handle<Image>, Option<Handle<Image>>)>, folded: usize| {
            let DetailCapture::Layers(mut d) = layers(Handle::default(), normal.clone(), overlays) else { unreachable!() };
            let r = d.resolved.as_mut().unwrap();
            (r.base_diffuse, r.folded) = (base, folded);
            DetailCapture::Layers(d)
        };
        let mut cell = surface([CapturePath::Folded { layers: 1 }; 4]);
        cell.detail[0] = capture(None, vec![(sampled(&mut images, 8), None)], 1);
        cell.detail[1] = capture(Some(base.clone()), vec![(sampled(&mut images, 1), None)], 0);
        cell.detail[2] = capture(Some(base.clone()), vec![(sampled(&mut images, 8), Some(sampled(&mut images, 16)))], 0);
        cell.detail[3] = capture(Some(base.clone()), vec![(sampled(&mut images, 8), Some(sampled(&mut images, 8)))], 0);
        let p = DetailPalette::assemble([&cell], &images, &[]);
        assert_eq!(p.refused, vec![(((-17, 0), 0), "sampler-mismatch"), (((-17, 0), 1), "sampler-mismatch"), (((-17, 0), 2), "sampler-mismatch")],
            "a textureless base under folded overlays; an overflow diffuse, an overflow normal map with another description");
        assert_eq!(p.admitted.iter().map(|a| a.0).collect::<Vec<_>>(), vec![((-17, 0), 3)], "overflow images with the base diffuse's description");
        // Folded overlays read with the base's sampler in both draws, whatever their own
        // description; overflow layers alone over a textureless base read with the first
        // overlay's, so every later one must share it.
        let mut other = surface([CapturePath::Folded { layers: 2 }; 4]);
        other.grid = (-16, 0);
        other.detail[0] = capture(Some(base.clone()), vec![(sampled(&mut images, 1), Some(sampled(&mut images, 2)))], 1);
        other.detail[1] = capture(None, vec![(sampled(&mut images, 4), None), (sampled(&mut images, 4), None)], 0);
        other.detail[2] = capture(None, vec![(sampled(&mut images, 4), None), (sampled(&mut images, 2), None)], 0);
        let o = DetailPalette::assemble([&other], &images, &[]);
        assert_eq!(o.admitted.iter().map(|a| a.0).collect::<Vec<_>>(), vec![((-16, 0), 0), ((-16, 0), 1)]);
        assert!(o.refused.contains(&(((-16, 0), 2), "sampler-mismatch")) && o.refused.contains(&(((-16, 0), 3), "not-captured")));
    }

    /// [design] Round 10a review, second round (P2): a textureless admitted quadrant
    /// samples no sampler (its table words still name sampler 0 for both). When the cell
    /// whose image represents sampler 0 is masked (that image removed from the image
    /// assets), the surviving textureless quadrant keeps nothing alive: the live palette
    /// binds the fallback for the sampler, never the removed representative.
    #[test]
    fn a_textureless_survivor_never_restores_a_removed_sampler_representative() {
        let mut images = Assets::<Image>::default();
        let (base, normal) = (sampled(&mut images, 8), sampled(&mut images, 8));
        let mut textured = surface([CapturePath::Fallback { layers: 0 }; 4]);
        textured.grid = (0, 0);
        textured.detail[0] = layers(base.clone(), normal.clone(), vec![]);
        let mut bare = surface([CapturePath::Fallback { layers: 0 }; 4]);
        bare.grid = (1, 0);
        let DetailCapture::Layers(mut d) = layers(Handle::default(), Handle::default(), vec![]) else { unreachable!() };
        let r = d.resolved.as_mut().unwrap();
        (r.base_diffuse, r.base_normal) = (None, None);
        bare.detail[0] = DetailCapture::Layers(d);
        let p = DetailPalette::assemble([&textured, &bare], &images, &[]);
        assert_eq!(p.admitted.iter().map(|a| a.0).collect::<Vec<_>>(), vec![((0, 0), 0), ((1, 0), 0)], "the textureless quadrant is admitted");
        assert_eq!((p.admitted[1].1.diffuse_sampler, p.admitted[1].1.normal_sampler), (0, 0), "its table words name sampler 0 for both");
        assert_eq!((p.palette.images.clone(), p.palette.samplers.clone()), (vec![Some(base.clone()), Some(normal.clone())], vec![Some(base.clone())]));
        let published = PublishedSurface { origin: (-1, -1), palette: p.palette.clone(), palette_use: p.usage(&images), ..default() };
        // (0, 0) and (1, 0) in slots (1, 1) and (2, 1) of origin (-1, -1).
        let (textured_slot, bare_slot) = (1 << 4, 1 << 5);
        assert_eq!(published.live_palette(textured_slot | bare_slot), p.palette, "both covered: the whole palette");
        images.remove(&base);
        let live = published.live_palette(bare_slot);
        assert_eq!(live.images, vec![None, None], "the masked cell's images on the fallback");
        assert_eq!(live.samplers, vec![None], "sampler 0 has no live representative: the fallback, not the removed image");
        assert_eq!(published.live_palette(textured_slot).samplers, vec![Some(base.clone())], "a covered quadrant that samples it keeps its representative");
    }

    /// The images a pending bake's expected draws bind that are not in the image assets
    /// (a draw that binds one could never be prepared, so its receipt would never come).
    fn unpreparable_bake_images(app: &mut App) -> Vec<Handle<Image>> {
        let expected = app.world_mut().query::<&BakeView>().single(app.world()).unwrap().expected.clone();
        let world = app.world();
        let images = world.resource::<Assets<Image>>();
        let mut missing = Vec::new();
        for e in expected {
            let bound: Vec<Handle<Image>> = if let Some(m) = world.get::<MeshMaterial3d<TerrainMaterial>>(e) {
                let m = world.resource::<Assets<TerrainMaterial>>().get(&m.0).unwrap();
                let x = &m.extension;
                [&m.base.base_color_texture, &m.base.normal_map_texture, &x.diffuse0, &x.normal0, &x.diffuse1, &x.normal1, &x.diffuse2, &x.normal2,
                    &x.diffuse3, &x.normal3, &x.diffuse4, &x.normal4, &x.diffuse5, &x.normal5, &x.burn].into_iter().flatten().cloned().collect()
            } else {
                let m = world.get::<MeshMaterial3d<SurfaceCaptureMaterial>>(e).unwrap();
                world.resource::<Assets<SurfaceCaptureMaterial>>().get(&m.0).unwrap().diffuse.iter().cloned().collect()
            };
            missing.extend(bound.into_iter().filter(|h| !images.contains(h)));
        }
        missing
    }

    /// [design] Round 10a review, second round (P2): a folded capture (base diffuse and
    /// normal map, one folded layer with its normal map) whose layer normal map is removed
    /// from the image assets. The removal masks the cell and rebakes it; its folded bake
    /// copy binds no normal map, so every draw the replacement waits for can be prepared.
    /// Then a colour image of the copy is removed: the cell is planned out (no draw waits
    /// on it, the front's coverage masks it) until the image is added back, when it bakes
    /// again. No receipt is injected for a draw that could not be prepared.
    #[test]
    fn removed_images_never_leave_the_replacement_waiting_on_an_unpreparable_draw() {
        let (mut app, _files, _) = fixture(false, true);
        let (base, base_normal, diffuse, normal) = { let mut i = app.world_mut().resource_mut::<Assets<Image>>(); (sampled(&mut i, 8), sampled(&mut i, 8), sampled(&mut i, 8), sampled(&mut i, 8)) };
        let entity = app.world().resource::<Streamer>().loaded[&(0, 0)];
        {
            let mut cell = app.world_mut().get_mut::<SurfaceCell>(entity).unwrap();
            cell.quadrants.clear();
            let material = TerrainMaterial {
                base: StandardMaterial { base_color_texture: Some(base.clone()), normal_map_texture: Some(base_normal.clone()), ..default() },
                extension: crate::terrain::TerrainLayers { count: 1, diffuse0: Some(diffuse.clone()), normal0: Some(normal.clone()), ..default() },
            };
            let DetailCapture::Layers(mut d) = layers(base.clone(), base_normal.clone(), vec![(diffuse.clone(), Some(normal.clone()))]) else { unreachable!() };
            d.resolved.as_mut().unwrap().folded = 1;
            cell.detail[0] = DetailCapture::Layers(d);
            let mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            cell.capture(0, vec![(mesh, CaptureDraw::Folded(Box::new(material)))], CapturePath::Folded { layers: 0 });
            let CaptureDraw::Folded(copy) = &cell.quadrants[0].1 else { unreachable!() };
            assert!(copy.extension.surface_bake && copy.base.normal_map_texture.is_none() && copy.extension.normal0.is_none(), "the bake copy binds no normal map");
            assert_eq!(copy.extension.diffuse0.as_ref(), Some(&diffuse));
            assert!(cell.dependencies.contains(&normal.id()), "the normal map is still a (detail) dependency");
        }
        let folded_copies = |app: &mut App| {
            let expected = app.world_mut().query::<&BakeView>().single(app.world()).unwrap().expected.clone();
            expected.iter().filter(|e| app.world().get::<MeshMaterial3d<TerrainMaterial>>(**e).is_some()).count()
        };
        tick(&mut app);
        assert!(job_colour(&app).is_some() && work_sets(&mut app).1.contains(&(0, 0)) && folded_copies(&mut app) == 1);
        assert_eq!(unpreparable_bake_images(&mut app), Vec::<Handle<Image>>::new(), "every draw can be prepared");
        finish(&mut app);
        assert!(app.world().resource::<SurfaceCache>().published.as_ref().unwrap().detail.contains(&((0, 0), 0)));
        // The layer normal map removed (Assets::remove ignores strong handles) and its event.
        app.world_mut().resource_mut::<Assets<Image>>().remove(&normal);
        app.world_mut().resource_mut::<Messages<AssetEvent<Image>>>().write(AssetEvent::Removed { id: normal.id() });
        tick(&mut app);
        assert!(job_colour(&app).is_some(), "the removal rebakes the cell");
        assert!(work_sets(&mut app).1.contains(&(0, 0)) && folded_copies(&mut app) == 1, "its folded copy is drawn again");
        assert_eq!(unpreparable_bake_images(&mut app), Vec::<Handle<Image>>::new(), "the removed normal map is not bound by the folded copy");
        assert!(!app.world().resource::<SurfaceCache>().job.as_ref().unwrap().publication.detail.contains(&((0, 0), 0)), "its detail is refused (image-not-loaded)");
        // A colour image of the folded copy removed: the cell is planned out. The pending
        // job (which would bind it) is cancelled; the rest is unchanged, so the front's
        // coverage masks the cell and no draw waits.
        app.world_mut().resource_mut::<Assets<Image>>().remove(&diffuse);
        app.world_mut().resource_mut::<Messages<AssetEvent<Image>>>().write(AssetEvent::Removed { id: diffuse.id() });
        for _ in 0..5 {
            tick(&mut app);
            if job_colour(&app).is_some() { assert_eq!(unpreparable_bake_images(&mut app), Vec::<Handle<Image>>::new(), "a job never waits on the removed colour image"); }
        }
        let front = app.world().resource::<SurfaceCache>().published.clone().unwrap();
        assert_eq!((front.coverage >> 4) & 1, 0, "(0, 0) masked");
        assert!(job_colour(&app).is_none(), "a pure removal: nothing to bake");
        // Added back: the cell returns and bakes, with a copy that can be prepared.
        assert!(app.world_mut().resource_mut::<Assets<Image>>().insert(diffuse.id(), Image::default()).is_ok());
        app.world_mut().resource_mut::<Messages<AssetEvent<Image>>>().write(AssetEvent::Added { id: diffuse.id() });
        tick(&mut app);
        assert!(job_colour(&app).is_some() && work_sets(&mut app).1.contains(&(0, 0)), "the cell bakes again");
        assert_eq!(unpreparable_bake_images(&mut app), Vec::<Handle<Image>>::new());
    }

    /// Cell (0, 0)'s quadrant 0 as a folded capture with detail: base diffuse and normal map,
    /// one folded layer with its diffuse and normal map. Returns `[base normal, layer normal]`.
    fn folded_detail_cell(app: &mut App) -> [Handle<Image>; 2] {
        let (base, base_normal, diffuse, normal) = { let mut i = app.world_mut().resource_mut::<Assets<Image>>(); (sampled(&mut i, 8), sampled(&mut i, 8), sampled(&mut i, 8), sampled(&mut i, 8)) };
        let entity = app.world().resource::<Streamer>().loaded[&(0, 0)];
        let mut cell = app.world_mut().get_mut::<SurfaceCell>(entity).unwrap();
        cell.quadrants.clear();
        let material = TerrainMaterial {
            base: StandardMaterial { base_color_texture: Some(base.clone()), normal_map_texture: Some(base_normal.clone()), ..default() },
            extension: crate::terrain::TerrainLayers { count: 1, diffuse0: Some(diffuse.clone()), normal0: Some(normal.clone()), ..default() },
        };
        let DetailCapture::Layers(mut d) = layers(base, base_normal.clone(), vec![(diffuse, Some(normal.clone()))]) else { unreachable!() };
        d.resolved.as_mut().unwrap().folded = 1;
        cell.detail[0] = DetailCapture::Layers(d);
        let mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        cell.capture(0, vec![(mesh, CaptureDraw::Folded(Box::new(material)))], CapturePath::Folded { layers: 0 });
        [base_normal, normal]
    }
    fn job_detail(app: &App) -> bool { app.world().resource::<SurfaceCache>().job.as_ref().unwrap().publication.detail.contains(&((0, 0), 0)) }
    fn front_detail(app: &App) -> bool { app.world().resource::<SurfaceCache>().published.as_ref().unwrap().detail.contains(&((0, 0), 0)) }
    fn image_event(app: &mut App, event: AssetEvent<Image>) { app.world_mut().resource_mut::<Messages<AssetEvent<Image>>>().write(event); }

    /// [design] Round 10a review, third round (P2): a base or layer normal map removed from
    /// the image assets rebakes the cell without its detail (`image-not-loaded`), and that
    /// replacement publishes. Nothing else changes, so the cell stays resident and its request
    /// is unchanged until the normal map is added back under the same id: then the return
    /// rebuilds the detail, once for its Added and LoadedWithDependencies notifications
    /// (a later second notification does not cancel the rebuilt replacement).
    #[test]
    fn a_normal_map_added_back_restores_the_detail_once() {
        for which in 0..2 {
            let (mut app, _files, _) = fixture(false, true);
            let normal = folded_detail_cell(&mut app)[which].clone();
            tick(&mut app);
            finish(&mut app);
            assert!(front_detail(&app), "case {which}: the first publication carries the detail");
            let saved = app.world().resource::<Assets<Image>>().get(&normal).unwrap().clone();
            app.world_mut().resource_mut::<Assets<Image>>().remove(&normal);
            image_event(&mut app, AssetEvent::Removed { id: normal.id() });
            tick(&mut app);
            assert!(job_colour(&app).is_some() && !job_detail(&app), "case {which}: the removal rebakes the cell without its detail");
            finish(&mut app);
            assert!(!front_detail(&app), "case {which}: the replacement published on the colour cache");
            for _ in 0..3 { tick(&mut app); }
            assert!(job_colour(&app).is_none(), "case {which}: nothing else invalidates the request");
            let generation = app.world().resource::<SurfaceCache>().generation;
            assert!(app.world_mut().resource_mut::<Assets<Image>>().insert(normal.id(), saved).is_ok());
            image_event(&mut app, AssetEvent::Added { id: normal.id() });
            image_event(&mut app, AssetEvent::LoadedWithDependencies { id: normal.id() });
            tick(&mut app);
            assert!(job_colour(&app).is_some() && job_detail(&app), "case {which}: the return rebuilds the detail");
            assert_eq!(app.world().resource::<SurfaceCache>().generation, generation + 1, "case {which}: one replacement for both notifications");
            // A repeated notification a frame later: `finish` asserts the job survives it.
            image_event(&mut app, AssetEvent::LoadedWithDependencies { id: normal.id() });
            finish(&mut app);
            assert!(front_detail(&app), "case {which}: the detail is restored");
            for _ in 0..3 { tick(&mut app); }
            assert!(job_colour(&app).is_none());
            assert_eq!(app.world().resource::<SurfaceCache>().generation, generation + 1, "case {which}: no redundant bake");
        }
    }

    /// [design] Round 10a review, third round (P2): a normal map not loaded when the first
    /// replacement is planned (refused, `image-not-loaded`) ends up with detail when it
    /// arrives: the base normal map after the colour-only publication, the layer normal map
    /// while that replacement is still pending (cancelled and replanned). Its Added and,
    /// a frame later, LoadedWithDependencies notifications cause one replacement.
    #[test]
    fn a_normal_map_loaded_late_ends_up_with_detail() {
        for which in 0..2 {
            let pending = which == 1;
            let (mut app, _files, _) = fixture(false, true);
            let normal = folded_detail_cell(&mut app)[which].clone();
            // Not loaded yet: not in the image assets, and no event so far.
            let saved = app.world_mut().resource_mut::<Assets<Image>>().remove(&normal).unwrap();
            tick(&mut app);
            assert!(job_colour(&app).is_some() && !job_detail(&app), "case {which}: refused while not loaded");
            assert!(app.world().resource::<SurfaceCache>().awaited_images.contains(&normal.id()));
            if !pending {
                finish(&mut app);
                assert!(!front_detail(&app));
            }
            let generation = app.world().resource::<SurfaceCache>().generation;
            assert!(app.world_mut().resource_mut::<Assets<Image>>().insert(normal.id(), saved).is_ok());
            image_event(&mut app, AssetEvent::Added { id: normal.id() });
            tick(&mut app);
            image_event(&mut app, AssetEvent::LoadedWithDependencies { id: normal.id() });
            // A cancelled pending job retires for three frames before the replan.
            for _ in 0..4 { if app.world().resource::<SurfaceCache>().generation > generation { break; } tick(&mut app); }
            assert!(job_colour(&app).is_some() && job_detail(&app), "case {which}: the arrival rebuilds the detail");
            finish(&mut app);
            assert!(front_detail(&app), "case {which}: the late normal map ends up with detail");
            for _ in 0..3 { tick(&mut app); }
            assert!(job_colour(&app).is_none());
            assert_eq!(app.world().resource::<SurfaceCache>().generation, generation + 1, "case {which}: one replacement for both notifications");
        }
    }

    /// [design] Round 10a review (P2): an overflow layer's colour, alpha or texture
    /// transform, or the base's transform alone, edited to what the detail does not
    /// reproduce keeps the quadrant on the colour cache (`overflow-material`) after the
    /// material refresh; an edit back (or the same transform on both) serves layer detail
    /// again.
    #[test]
    fn overflow_material_edits_keep_the_colour_cache_until_reproducible_again() {
        use bevy::ecs::system::RunSystemOnce;
        use bevy::math::Affine2;
        use nvf_assets::NifMaterial;
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        app.init_resource::<Assets<TerrainMaterial>>().init_resource::<Messages<AssetEvent<NifMaterial>>>();
        let (diffuse, normal, layer_diffuse) = { let mut i = app.world_mut().resource_mut::<Assets<Image>>(); (sampled(&mut i, 8), sampled(&mut i, 8), sampled(&mut i, 8)) };
        let (base, layer) = {
            let mut m = app.world_mut().resource_mut::<Assets<NifMaterial>>();
            let base = m.add(NifMaterial { base: StandardMaterial { base_color_texture: Some(diffuse), normal_map_texture: Some(normal), ..default() }, extension: default() });
            let layer = m.add(NifMaterial { base: StandardMaterial { base_color_texture: Some(layer_diffuse), alpha_mode: AlphaMode::Blend, ..default() }, extension: default() });
            (base, layer)
        };
        let n = nvf_assets::nif_surface_blend::QUAD_VERTICES;
        let resolved = resolve_detail(&DetailBase::Single(base.clone()), std::slice::from_ref(&layer), 1, app.world().resource::<Assets<TerrainMaterial>>(), app.world().resource::<Assets<NifMaterial>>());
        assert!(resolved.is_ok(), "the near draw's own materials: white, opaque, the default transform");
        let mut cell = surface([CapturePath::Fallback { layers: 1 }; 4]);
        cell.sources = vec![CaptureSource::Single(base.clone(), false), CaptureSource::Single(layer.clone(), true)];
        cell.detail[0] = DetailCapture::Layers(Box::new(QuadrantDetail {
            base: DetailBase::Single(base.clone()), overflow: vec![layer.clone()], cell_local: false, tile: 256.0,
            tint: vec![[1.0; 3]; n], normal: vec![[0.0, 0.0, 1.0]; n], tangent: vec![[1.0, 0.0, 0.0]; n], tangent_sign: -1.0,
            opacity: vec![vec![0.5; n]], resolved,
        }));
        let entity = app.world_mut().spawn(cell).id();
        type Edit<'a> = &'a dyn Fn(&mut StandardMaterial, &mut StandardMaterial);
        let refresh = |app: &mut App, edit: Edit| -> Option<&'static str> {
            {
                let mut m = app.world_mut().resource_mut::<Assets<NifMaterial>>();
                let (mut b, mut l) = (m.get(&base).unwrap().base.clone(), m.get(&layer).unwrap().base.clone());
                edit(&mut b, &mut l);
                m.get_mut(&base).unwrap().base = b;
                m.get_mut(&layer).unwrap().base = l;
            }
            for id in [base.id(), layer.id()] { app.world_mut().resource_mut::<Messages<AssetEvent<NifMaterial>>>().write(AssetEvent::Modified { id }); }
            app.world_mut().run_system_once(refresh_captures).unwrap();
            let p = DetailPalette::assemble([app.world().get::<SurfaceCell>(entity).unwrap()], app.world().resource::<Assets<Image>>(), &[]);
            let reason = p.refused.iter().find(|(at, _)| *at == ((-17, 0), 0)).map(|(_, r)| *r);
            assert_eq!(reason.is_none(), p.admitted.iter().any(|(at, _)| *at == ((-17, 0), 0)));
            reason
        };
        let defaults = |b: &mut StandardMaterial, l: &mut StandardMaterial| {
            (b.uv_transform, l.uv_transform, l.base_color) = (Affine2::IDENTITY, Affine2::IDENTITY, Color::WHITE);
        };
        let doubled = Affine2::from_scale(Vec2::splat(2.0));
        let edits: [(&str, Edit); 4] = [
            ("a tinted overflow layer", &|_, l| l.base_color = Color::srgb(1.0, 0.5, 0.5)),
            ("a partly transparent overflow layer", &|_, l| l.base_color = Color::WHITE.with_alpha(0.5)),
            ("an overflow layer's own transform", &|_, l| l.uv_transform = doubled),
            ("the base's transform alone", &|b, _| b.uv_transform = doubled),
        ];
        for (what, edit) in edits {
            assert_eq!(refresh(&mut app, &|b, l| { defaults(b, l); edit(b, l); }), Some("overflow-material"), "{what}");
            assert_eq!(refresh(&mut app, &defaults), None, "{what}: back to layer detail once reverted");
        }
        assert_eq!(refresh(&mut app, &|b, l| { b.uv_transform = doubled; l.uv_transform = doubled; }), None, "the same transform on the base and the overflow layer");
    }

    /// [design] Round 10a review (P2): the replacement guard waits for the front's
    /// materials to settle, but a front that never settles (a candidate that can never be
    /// prepared) holds the replacement back for at most `SETTLE_FRAMES` frames.
    #[test]
    fn settlement_guard_waits_but_never_forever() {
        let (mut app, _files) = cache_fixture();
        // Settled at revision 0, never the front's.
        app.insert_resource(crate::mesh_terrain_blend::BlendReadiness::default());
        let entity = app.world().resource::<Streamer>().loaded[&(0, 0)];
        app.world_mut().get_mut::<SurfaceCell>(entity).unwrap().heights[0] = 123.0;
        for _ in 0..SETTLE_FRAMES {
            tick(&mut app);
            assert!(app.world().resource::<SurfaceCache>().job.is_none(), "waiting for settlement");
        }
        tick(&mut app);
        assert!(app.world().resource::<SurfaceCache>().job.is_some(), "the replacement starts after the wait expires");
        assert_eq!(app.world().resource::<SurfaceCache>().unsettled_frames, 0);
    }

    #[test]
    fn rejected_resolution_bakes_and_publishes_nothing() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.insert_resource(crate::outfits::tests::empty_world_data())
            .init_resource::<Assets<Image>>().init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<TerrainMaterial>>().init_resource::<Assets<SurfaceCaptureMaterial>>()
            .insert_resource(SurfaceCache { rejected: true, ..SurfaceCache::with_resolution(2048) });
        let cell = app.world_mut().spawn(SurfaceCell { world: 0, grid: (0, 0), heights: vec![0.0; SIDE * SIDE], quadrants: vec![], paths: [CapturePath::Fallback { layers: 0 }; 4], content_revision: 0, sources: vec![], dependencies: default(), detail: default() }).id();
        app.insert_resource(Streamer {
            radius: 1, loaded: [((0, 0), cell)].into(), fades: default(), fade_secs: 0.0, settled: true,
            arrival: None, per_frame: 1, spawning: default(), dying: default(), ahead: default(),
            hide: default(), interior: None, light_scale: 1.0, managed: default(), manage_actors: false,
            stats: default(), buffer: default(), centre: None, waiting: default(), model_overrides: default(), coop_areas: Vec::new(), travels: 0,
        });
        app.world_mut().spawn((Camera3d::default(), crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, GlobalTransform::from_translation(Vec3::new(2048.0, 600.0, -2048.0))));
        for _ in 0..3 { app.world_mut().run_system_once(update_cache).unwrap(); }
        let cache = app.world().resource::<SurfaceCache>();
        assert!(cache.job.is_none() && cache.published.is_none() && cache.generation == 1);
        assert!(app.world().resource::<Assets<Image>>().is_empty());
    }
    fn cache_fixture() -> (App, impl Sized) { cache_fixture_with_tracking(false) }
    fn cache_fixture_with_tracking(tracking: bool) -> (App, impl Sized) {
        let (app, files, _) = fixture(tracking, false);
        (app, files)
    }
    /// [design] Round 10a review: [`cache_fixture`] with landscape detail, published:
    /// quadrant 0 of cells (0, 0) and (1, 0) reads its own base and normal map, all four
    /// images with one sampler description. Returns them as (0, 0)'s base and normal map,
    /// then (1, 0)'s; the palette holds them in that order, its one sampler represented
    /// by (0, 0)'s base.
    pub(crate) fn detail_fixture() -> (App, impl Sized, [Handle<Image>; 4]) {
        let (app, files, images) = fixture(false, true);
        (app, files, images.try_into().unwrap())
    }
    fn fixture(tracking: bool, detail: bool) -> (App, impl Sized, Vec<Handle<Image>>) {
        let (mut app, files) = crate::outfits::tests::asset_fixture();
        let mut detail_images = Vec::new();
        app.insert_resource(crate::outfits::tests::empty_world_data())
            .insert_resource(SurfaceCache { detail, ..SurfaceCache::with_resolution(512) }).init_resource::<Assets<TerrainMaterial>>()
            .init_resource::<Assets<SurfaceCaptureMaterial>>()
            .init_resource::<Messages<AssetEvent<Image>>>();
        if tracking {
            app.add_plugins((bevy::app::TaskPoolPlugin::default(), bevy::asset::AssetPlugin::default()))
                .init_asset::<Image>().init_asset::<Mesh>().init_asset::<TerrainMaterial>().init_asset::<SurfaceCaptureMaterial>();
        }
        app.insert_resource(Streamer {
            radius: 2, loaded: default(), fades: default(), fade_secs: 0.0, settled: true,
            arrival: None, per_frame: 1, spawning: default(), dying: default(), ahead: default(),
            hide: default(), interior: None, light_scale: 1.0, managed: default(), manage_actors: false,
            stats: default(), buffer: default(), centre: None, waiting: default(), model_overrides: default(), coop_areas: Vec::new(), travels: 0,
        });
        for y in -3..=3 { for x in -3..=3 {
            let diffuse = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
            let mut cell = surface([CapturePath::Fallback { layers: 0 }; 4]);
            cell.grid = (x, y);
            cell.quadrants.push((Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::default()),
                CaptureDraw::Single(SurfaceCaptureMaterial::from_standard(&StandardMaterial { base_color_texture: Some(diffuse), ..default() }, false))));
            if detail && y == 0 && (0..2).contains(&x) {
                let mut images = app.world_mut().resource_mut::<Assets<Image>>();
                let (base, normal) = (sampled(&mut images, 8), sampled(&mut images, 8));
                detail_images.extend([base.clone(), normal.clone()]);
                cell.detail[0] = layers(base, normal, vec![]);
            }
            cell.refresh_dependencies();
            let entity = app.world_mut().spawn(cell).id();
            app.world_mut().resource_mut::<Streamer>().loaded.insert((x, y), entity);
        }}
        app.world_mut().spawn((Camera3d::default(), crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, GlobalTransform::from_translation(Vec3::new(1024.0, 600.0, -1024.0))));
        tick(&mut app);
        finish(&mut app);
        (app, files, detail_images)
    }
    pub(crate) fn tick(app: &mut App) {
        use bevy::ecs::system::RunSystemOnce;
        app.world_mut().run_system_once(update_cache).unwrap();
    }
    /// The pending replacement's colour atlas, if one is pending.
    pub(crate) fn job_colour(app: &App) -> Option<Handle<Image>> {
        app.world().resource::<SurfaceCache>().job.as_ref().map(|j| j.publication.colour.clone())
    }
    pub(crate) fn finish(app: &mut App) {
        let cache = app.world_mut().resource_mut::<SurfaceCache>();
        *cache.submitted.lock().unwrap() = Some(cache.job.as_ref().unwrap().receipt);
        tick(app);
        assert!(app.world().resource::<SurfaceCache>().job.is_some(), "extra main-frame publication delay");
        tick(app);
        assert!(app.world().resource::<SurfaceCache>().job.is_none());
    }
    fn move_camera(app: &mut App, cell: (i32, i32)) {
        move_camera_point(app, Vec2::new(cell.0 as f32 * 4096.0 + 1024.0, cell.1 as f32 * 4096.0 + 1024.0));
    }
    fn move_camera_point(app: &mut App, point: Vec2) {
        let mut q = app.world_mut().query_filtered::<&mut GlobalTransform, With<crate::fly::FlyCam>>();
        *q.single_mut(app.world_mut()).unwrap() = GlobalTransform::from_translation(Vec3::new(point.x, 600.0, -point.y));
    }
    fn work_counts(app: &mut App) -> (usize, usize) {
        let view = app.world_mut().query::<&BakeView>().single(app.world()).unwrap();
        (view.transfers.cells.len(), view.expected.len())
    }
    type CellSet = HashSet<(i32, i32)>;
    fn work_sets(app: &mut App) -> (CellSet, CellSet) {
        let origin = app.world().resource::<SurfaceCache>().job.as_ref().unwrap().publication.origin;
        let view = app.world_mut().query::<&BakeView>().single(app.world()).unwrap();
        let resolution = app.world().resource::<SurfaceCache>().resolution;
        let copied = view.transfers.cells.iter().map(|c| (origin.0 + (c.destination.0 / resolution) as i32, origin.1 + 2 - (c.destination.1 / resolution) as i32)).collect();
        let baked = view.expected.iter().map(|entity| {
            let t = app.world().get::<Transform>(*entity).unwrap().translation;
            (origin.0 + (t.x / 4096.0) as i32, origin.1 + (t.y / 4096.0) as i32)
        }).collect();
        (copied, baked)
    }
    fn region(centre: (i32, i32)) -> CellSet {
        (-1..=1).flat_map(|y| (-1..=1).map(move |x| (centre.0 + x, centre.1 + y))).collect()
    }
    #[test]
    fn axial_diagonal_negative_and_backtracking_copy_exact_sets_keep_old_origin() {
        for (destination, expected) in [((1, 0), (6, 3)), ((1, 1), (4, 5)), ((-1, -1), (4, 5)), ((-1, 0), (6, 3))] {
            let (mut app, _files) = cache_fixture();
            let front = app.world().resource::<SurfaceCache>().published.clone().unwrap();
            move_camera(&mut app, destination);
            tick(&mut app);
            assert_eq!(work_counts(&mut app), expected);
            let old_region = region((0, 0));
            let new_region = region(destination);
            assert_eq!(work_sets(&mut app), (old_region.intersection(&new_region).copied().collect(), new_region.difference(&old_region).copied().collect()));
            for _ in 0..5 {
                tick(&mut app);
                let cache = app.world().resource::<SurfaceCache>();
                let retained = cache.published.as_ref().unwrap();
                assert_eq!((retained.colour.id(), retained.origin), (front.colour.id(), front.origin));
                let height = app.world().resource::<Assets<Image>>().get(&retained.height).unwrap().data.as_ref().unwrap();
                assert_eq!(lookup_covered_status(retained.layout, height, retained.origin, Vec2::splat(1024.0), 1024, retained.coverage), LookupStatus::Success);
            }
            finish(&mut app);
            assert_ne!(app.world().resource::<SurfaceCache>().published.as_ref().unwrap().colour.id(), front.colour.id());
            move_camera(&mut app, (0, 0)); tick(&mut app);
            assert_eq!(work_counts(&mut app), expected);
            finish(&mut app);
        }
    }
    #[test]
    fn selected_centre_hysteresis_is_inclusive_per_axis_and_handles_teleports() {
        let mut cache = SurfaceCache::default();
        let x = -17.0 * 4096.0 + 1024.0;
        assert_eq!(cache.selected_origin(Vec2::new(x, -8300.0)), (-18, -4));
        for y in [-8193.0, -8191.0, -7936.0, -8192.0, -7936.0] {
            assert_eq!(cache.selected_origin(Vec2::new(x, y)), (-18, -4));
        }
        assert_eq!(cache.selected_origin(Vec2::new(x, -7935.5)), (-18, -3));
        for y in [-8191.0, -8193.0, -8448.0, -8192.0, -8448.0] {
            assert_eq!(cache.selected_origin(Vec2::new(x, y)), (-18, -3));
        }
        assert_eq!(cache.selected_origin(Vec2::new(x, -8448.5)), (-18, -4));
        assert_eq!(cache.selected_origin(Vec2::new(-16.0 * 4096.0 + 256.0, -8191.0)), (-18, -4));
        assert_eq!(cache.selected_origin(Vec2::new(-16.0 * 4096.0 + 256.5, -8191.0)), (-17, -4));
        assert_eq!(cache.selected_origin(Vec2::new(42000.0, -42000.0)), (9, -12));
    }
    #[test]
    fn edge_jitter_does_no_camera_work_but_input_and_residency_still_invalidate() {
        let (mut app, _files) = cache_fixture();
        let generation = app.world().resource::<SurfaceCache>().generation;
        for y in [4095.0, 4097.0, 4352.0, 4090.0, 4100.0] {
            move_camera_point(&mut app, Vec2::new(1024.0, y)); tick(&mut app);
            let cache = app.world().resource::<SurfaceCache>();
            assert_eq!(cache.generation, generation);
            assert!(cache.job.is_none());
            assert_eq!(cache.published.as_ref().unwrap().origin, (-1, -1));
        }
        let cell = app.world().resource::<Streamer>().loaded[&(0, 0)];
        app.world_mut().get_mut::<SurfaceCell>(cell).unwrap().heights[0] += 1.0;
        tick(&mut app);
        assert_eq!(work_counts(&mut app), (8, 1));
        finish(&mut app);
        app.world_mut().resource_mut::<Streamer>().loaded.remove(&(1, 1));
        tick(&mut app);
        assert_eq!(app.world().resource::<SurfaceCache>().published.as_ref().unwrap().coverage.count_ones(), 8);
        move_camera_point(&mut app, Vec2::new(1024.0, 4352.5)); tick(&mut app);
        let cache = app.world().resource::<SurfaceCache>();
        assert_eq!(cache.job.as_ref().unwrap().publication.origin, (-1, 0));
        assert_eq!(cache.published.as_ref().unwrap().origin, (-1, -1), "front stays at its committed origin");
    }
    #[test]
    fn repeated_pending_reversals_retire_assets_over_real_app_updates() {
        let (mut app, _files) = cache_fixture_with_tracking(true);
        app.add_systems(Update, update_cache);
        for _ in 0..6 { app.update(); }
        let baseline = app.world().resource::<Assets<Image>>().len();
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
        assert_eq!(app.world().resource::<Assets<SurfaceCaptureMaterial>>().len(), 0);
        for turn in 0..12 {
            move_camera(&mut app, (if turn % 2 == 0 { 1 } else { -1 }, 0));
            app.update();
            assert!(app.world().resource::<Assets<Image>>().len() <= baseline + 4);
            assert!(app.world().resource::<Assets<Mesh>>().len() <= 18);
            assert!(app.world().resource::<Assets<SurfaceCaptureMaterial>>().len() <= 18);
            let cancelled = app.world().resource::<SurfaceCache>().job.as_ref().unwrap().receipt;
            move_camera(&mut app, (0, 0)); app.update();
            assert!(app.world().resource::<Assets<Image>>().len() <= baseline + 4);
            assert!(app.world().resource::<Assets<Mesh>>().len() <= 18);
            assert!(app.world().resource::<Assets<SurfaceCaptureMaterial>>().len() <= 18);
            *app.world().resource::<SurfaceCache>().submitted.lock().unwrap() = Some(cancelled);
            for _ in 0..6 {
                app.update();
                assert!(app.world().resource::<Assets<Image>>().len() <= baseline + 4, "at most three live atlas pairs including retirement");
                assert!(app.world().resource::<Assets<Mesh>>().len() <= 18);
                assert!(app.world().resource::<Assets<SurfaceCaptureMaterial>>().len() <= 18);
            }
            assert!(app.world().resource::<SurfaceCache>().job.is_none());
            assert_eq!(app.world().resource::<Assets<Image>>().len(), baseline, "cancelled images retired");
            assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0, "cancelled draw meshes retired");
            assert_eq!(app.world().resource::<Assets<SurfaceCaptureMaterial>>().len(), 0, "cancelled draw materials retired");
        }
        // Finish a genuine replacement, then ensure the old front also retires.
        move_camera(&mut app, (1, 0)); app.update();
        let receipt = app.world().resource::<SurfaceCache>().job.as_ref().unwrap().receipt;
        *app.world().resource::<SurfaceCache>().submitted.lock().unwrap() = Some(receipt);
        for _ in 0..8 { app.update(); }
        let cache = app.world().resource::<SurfaceCache>();
        assert!(cache.job.is_none());
        assert_eq!(cache.published.as_ref().unwrap().origin, (0, -1));
        assert_eq!(app.world().resource::<Assets<Image>>().len(), baseline);
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
        assert_eq!(app.world().resource::<Assets<SurfaceCaptureMaterial>>().len(), 0);
    }
    #[test]
    fn rapid_crossings_reject_cancelled_receipts() {
        let (mut app, _files) = cache_fixture();
        let colour = app.world().resource::<SurfaceCache>().published.as_ref().unwrap().colour.id();
        move_camera(&mut app, (1, 0)); tick(&mut app);
        let stale = app.world().resource::<SurfaceCache>().job.as_ref().unwrap().receipt;
        move_camera(&mut app, (1, 1)); tick(&mut app);
        assert!(app.world().resource::<SurfaceCache>().job.is_none());
        *app.world().resource::<SurfaceCache>().submitted.lock().unwrap() = Some(stale);
        for _ in 0..4 {
            tick(&mut app);
            assert_eq!(app.world().resource::<SurfaceCache>().published.as_ref().unwrap().colour.id(), colour);
        }
        let receipt = app.world().resource::<SurfaceCache>().job.as_ref().unwrap().receipt;
        assert_ne!(receipt, stale);
        // Stale and out-of-order receipts never satisfy the new job.
        *app.world().resource::<SurfaceCache>().submitted.lock().unwrap() = Some(stale);
        tick(&mut app); tick(&mut app);
        assert_eq!(app.world().resource::<SurfaceCache>().job.as_ref().unwrap().receipt, receipt);
        finish(&mut app);
    }
    #[test]
    fn unchanged_frames_and_residency_removal_do_no_colour_work() {
        let (mut app, _files) = cache_fixture();
        let before = app.world().resource::<Assets<Image>>().len();
        let generation = app.world().resource::<SurfaceCache>().generation;
        for _ in 0..4 { tick(&mut app); }
        app.world_mut().resource_mut::<Streamer>().loaded.remove(&(1, 1));
        tick(&mut app);
        let cache = app.world().resource::<SurfaceCache>();
        assert_eq!(cache.generation, generation);
        assert!(cache.job.is_none());
        assert_eq!(cache.published.as_ref().unwrap().coverage.count_ones(), 8);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), before);
        // A surviving SurfaceCell of the removed entity must not restore residency.
        tick(&mut app);
        assert_eq!(app.world().resource::<SurfaceCache>().published.as_ref().unwrap().cells, 8);
    }
    #[test]
    fn capture_and_dependent_image_changes_bake_only_one_cell() {
        for image_change in [false, true] {
            let (mut app, _files) = cache_fixture();
            let entity = app.world().resource::<Streamer>().loaded[&(0, 0)];
            if image_change {
                let dependency = *app.world().get::<SurfaceCell>(entity).unwrap().image_dependencies().iter().next().unwrap();
                app.world_mut().resource_mut::<Messages<AssetEvent<Image>>>().write(AssetEvent::Modified { id: dependency });
            } else {
                app.world_mut().get_mut::<SurfaceCell>(entity).unwrap().heights[0] = 123.0;
            }
            tick(&mut app);
            assert_eq!(work_counts(&mut app), (8, 1));
            let mut unchanged = region((0, 0)); unchanged.remove(&(0, 0));
            assert_eq!(work_sets(&mut app), (unchanged, HashSet::from([(0, 0)])));
            assert_eq!(app.world().resource::<SurfaceCache>().published.as_ref().unwrap().coverage.count_ones(), 8);
            finish(&mut app);
        }
    }
    #[test]
    fn coverage_drops_unloaded_diagonal_taps_and_rejects_invalid_quadrants() {
        let mut data = empty_height_atlas();
        let cell = surface([CapturePath::Fallback { layers: 0 }; 4]);
        let slots = HashSet::from([(0, 0), (1, 0), (0, 1), (1, 1)]);
        for &slot in &slots { encode_surface_cell(STORED, &mut data, slot, &cell, &slots); }
        let full = 0x1ff;
        assert_eq!(lookup_covered_status(STORED, &data, (-2, -2), Vec2::splat(-4096.1), 1024, full), LookupStatus::Success);
        let without_diagonal = full & !(1 << 4);
        // Round 10b review: a masked (no longer covered) diagonal cell's tap gets no weight and
        // the other three are renormalized, so the read stays valid and the same on both sides
        // of the fragment's edges with its two covered neighbours (before round 10b it was
        // rejected); the cell's own coverage still rejects (below).
        assert_eq!(lookup_covered_status(STORED, &data, (-2, -2), Vec2::splat(-4096.1), 1024, without_diagonal), LookupStatus::Success);
        let uv = Vec2::new((4096.0 - 0.1) / 12288.0, 1.0 - (4096.0 - 0.1) / 12288.0);
        let taps = colour_read_taps(STORED, &data, without_diagonal, (0, 0), uv, 1024, 0).unwrap();
        assert_eq!(taps.map(|t| colour_tap_slot(t.0, 1024)), [Some((0, 1)), Some((1, 1)), Some((0, 0)), Some((1, 0))], "north-west first: rows run north to south");
        assert_eq!(taps[1].1, 0.0, "the masked diagonal");
        assert!(taps.iter().all(|t| t.1 >= 0.0) && (taps.iter().map(|t| t.1).sum::<f32>() - 1.0).abs() < 1e-6);
        assert_eq!(lookup_covered_status(STORED, &data, (-2, -2), Vec2::splat(-4096.1), 1024, full & !1), LookupStatus::InvalidHeight, "the cell's own coverage");
        assert_eq!(lookup_covered_status(STORED, &data, (-2, -2), Vec2::splat(-3072.0), 1024, without_diagonal), LookupStatus::InvalidHeight);
        encode_colour_mask(STORED, &mut data, (0, 0), 0b0111);
        assert_eq!(lookup_covered_status(STORED, &data, (-2, -2), Vec2::splat(-4096.1), 1024, full), LookupStatus::InvalidColourQuadrant);
    }
    /// [design] Round 10b: the colour read's levels and footprint. Every level's bilinear
    /// footprint holds a tap of the fragment's own cell (round 10b review: it may cross into a
    /// neighbour); a coarser level whose own-cell texels cover an uncaptured quadrant rejects
    /// the read only when it contributes (the level below it alone at an integer level of
    /// detail), and the last level (one texel per cell) needs all four.
    #[test]
    fn colour_mips_validate_every_contributing_level() {
        assert_eq!(colour_read_levels(512, -3.0), (0, 1, 0.0), "magnified: level 0 alone");
        assert_eq!(colour_read_levels(512, 2.25), (2, 3, 0.25));
        assert_eq!(colour_read_levels(512, 9.0), (9, 9, 0.0), "the last level stays the last");
        assert_eq!(colour_read_levels(512, 40.0), (9, 9, 0.0));
        assert_eq!(colour_read_levels(2048, 10.5), (10, 11, 0.5));
        // Every level's footprint has a tap of the fragment's cell, with at least a quarter of
        // the weight (the tap holding the position is the nearest), at the cell's corners and
        // the atlas's edges too.
        for resolution in RESOLUTIONS { for level in 0..colour_levels(resolution) {
            let size = resolution >> level;
            for slot in [(0, 0), (2, 2), (1, 0)] { for local in [Vec2::ZERO, Vec2::splat(4095.999), Vec2::new(0.001, 4095.0)] {
                let uv = Vec2::new((slot.0 as f32 * 4096.0 + local.x) / 12288.0, 1.0 - (slot.1 as f32 * 4096.0 + local.y) / 12288.0);
                let (first, f) = colour_read_footprint(uv, size);
                let weight = (0..4).map(|i| IVec2::new(i % 2, i / 2)).filter(|&o| colour_tap_slot(first + o, size) == Some(slot))
                    .map(|o| (if o.x == 1 { f.x } else { 1.0 - f.x }) * (if o.y == 1 { f.y } else { 1.0 - f.y })).fold(-1.0, f32::max);
                assert!(weight >= 0.25 - 1e-3, "{resolution} level {level} {slot:?} {local:?}: {weight}");
            }}
        }}
        let mut data = empty_height_atlas();
        let cell = surface([CapturePath::Folded { layers: 0 }; 4]);
        let slots = HashSet::from([(0, 0), (1, 0), (0, 1), (1, 1)]);
        for &slot in &slots { encode_surface_cell(STORED, &mut data, slot, &cell, &slots); }
        encode_colour_mask(STORED, &mut data, (0, 0), 0b1110);
        let origin = (-18, -1);
        let shift = Vec2::new(origin.0 as f32, origin.1 as f32) * 4096.0;
        let status = |point: Vec2, lod: f32| lookup_level_status(STORED, &data, origin, shift + point, 512, 0x1ff, lod);
        // 20 units east of q0 (uncaptured), in q1 at 512 texels per cell: levels 0-2 (texels
        // of 8 to 32 units) keep both taps in q1; level 3's west tap (64-unit texels) is q0's.
        let near_edge = Vec2::new(2048.0 + 20.0, 1024.0);
        assert_eq!(status(near_edge, 0.0), LookupStatus::Success);
        assert_eq!(status(near_edge, 2.0), LookupStatus::Success, "level 2 alone");
        assert_eq!(status(near_edge, 2.5), LookupStatus::RejectedFootprint, "level 3 contributes");
        // In q3, 512 units from both quadrant edges: levels to 7 (4 texels per cell side) read
        // q3 and q1 (both captured); level 8 (two texels per side, quadrant-sized) reads the
        // west column, q0's texel among them; the last level's one texel covers all four.
        let quarter = Vec2::new(2560.0, 2560.0);
        assert_eq!(status(quarter, 7.0), LookupStatus::Success);
        assert_eq!(status(quarter, 7.5), LookupStatus::RejectedFootprint, "level 8 contributes");
        assert_eq!(status(quarter, 9.0), LookupStatus::RejectedFootprint, "the last level alone");
        // At the centre of q3 level 8's footprint is the east column (q3 and q1, at its texel
        // centres) and the east neighbour's west column, with no weight on q0's texel. Round
        // 10b's clamp inside the cell also took q0's texel there (with a 1/64 weight) and
        // rejected; the exact footprint reads no uncaptured texel, so the read serves.
        let far = Vec2::new(3072.0, 3072.0);
        assert_eq!(status(far, 7.5), LookupStatus::Success, "level 8 reads q3, q1 and the neighbour");
        assert_eq!(status(far, 9.0), LookupStatus::RejectedFootprint, "the last level alone");
        // All four captured: every level serves; the uncaptured west neighbour (0, 0) at the last
        // level and the atlas's south edge only lose their taps' weight.
        assert_eq!(lookup_level_status(STORED, &data, origin, shift + Vec2::new(4096.0 + 1024.0, 1024.0), 512, 0x1ff, 9.0), LookupStatus::Success);
        let uv = Vec2::new(1.25 / 3.0, 1.0 - 0.25 / 3.0);
        let taps = colour_read_taps(STORED, &data, 0x1ff, (1, 0), uv, 512, 9).unwrap();
        assert_eq!(taps.map(|t| (colour_tap_slot(t.0, 1), t.1 > 0.0)), [(Some((0, 0)), false), (Some((1, 0)), true), (None, false), (None, false)]);
        // The cell's own live coverage gates every level.
        assert_eq!(lookup_level_status(STORED, &data, origin, shift + Vec2::new(4096.0 + 1024.0, 1024.0), 512, 0x1ff & !(1 << 1), 0.0), LookupStatus::InvalidHeight);
    }
    /// [design] Round 10b review: the CPU statement of the shader's colour read
    /// (`terrain_colour_mips` over `terrain_colour_level`) at `point` (game units from the
    /// atlas's south-west corner, origin (0, 0)) for a test colour `ramp` of game position.
    /// A linear colour's mean over a texel's block of level-0 texels (the per-cell mip
    /// filter) is its value at the block's centre, so each level's texel holds `ramp` at
    /// its own centre.
    fn ramp_read(data: &[u8], coverage: u32, point: Vec2, resolution: u32, lod: f32, ramp: impl Fn(Vec2) -> f32) -> Option<f32> {
        let slot = fnv_world::cell_of(point.x, point.y);
        let uv = Vec2::new(point.x / 12288.0, 1.0 - point.y / 12288.0);
        let (low, high, share) = colour_read_levels(resolution, lod);
        let level = |level: u32| colour_read_taps(STORED, data, coverage, slot, uv, resolution, level).map(|taps| {
            let size = (resolution >> level) as f32;
            taps.iter().map(|&(t, w)| w * ramp(Vec2::new(t.x as f32 + 0.5, 3.0 * size - t.y as f32 - 0.5) * 4096.0 / size)).sum::<f32>()
        });
        let low_value = level(low)?;
        if share <= 0.0 || high == low { return Some(low_value); }
        Some(low_value + (level(high)? - low_value) * share)
    }
    /// [design] Round 10b review: a smooth colour ramp read across an internal cell edge is
    /// continuous at every level of detail, fractional ones and the last level included,
    /// when both cells are valid (round 10b's clamp into each cell jumped there by a texel's
    /// worth of the ramp: 0.02 at level 0 of 2048 for the x slope below, a whole cell's,
    /// about 41, at the last level). A linear colour read bilinearly across valid cells is
    /// the colour itself. A missing diagonal's tap is dropped by the fragments on both
    /// sides alike, so their shared edge stays continuous; beside a missing cardinal
    /// neighbour the read is the one clamped to the cell.
    #[test]
    fn colour_reads_are_continuous_across_valid_cell_edges() {
        let mut data = empty_height_atlas();
        let cell = surface([CapturePath::Folded { layers: 0 }; 4]);
        let slots: HashSet<_> = (0..3).flat_map(|y| (0..3).map(move |x| (x, y))).collect();
        for &slot in &slots { encode_surface_cell(STORED, &mut data, slot, &cell, &slots); }
        let ramp = |p: Vec2| 0.01 * p.x + 0.004 * p.y;
        let across = |coverage: u32, edge: Vec2, normal: Vec2, resolution: u32, lod: f32| {
            let before = ramp_read(&data, coverage, edge - normal * 0.01, resolution, lod, ramp).expect("valid before the edge");
            let after = ramp_read(&data, coverage, edge + normal * 0.01, resolution, lod, ramp).expect("valid after the edge");
            assert!((before - after).abs() < 1e-3, "{resolution} lod {lod} {edge:?} coverage {coverage:#x}: {before} against {after}");
            before
        };
        for resolution in RESOLUTIONS {
            let last = (colour_levels(resolution) - 1) as f32;
            for lod in [0.0, 0.5, 2.25, 5.5, last - 1.25, last - 1.0, last - 0.5, last] {
                // The edge between (0, 1) and (1, 1) at (0, 1)'s middle row, and the edge between
                // (1, 1) and (1, 2) at (1, 1)'s middle column: every tap inside the atlas.
                for (edge, normal) in [(Vec2::new(4096.0, 6144.0), Vec2::X), (Vec2::new(6144.0, 8192.0), Vec2::Y)] {
                    let value = across(0x1ff, edge, normal, resolution, lod);
                    assert!((value - ramp(edge)).abs() < 1e-3, "{resolution} lod {lod} {edge:?}: {value} against {}", ramp(edge));
                }
            }
            // (1, 1) uncovered: at the corner it shares with (0, 0), (1, 0) and (0, 1), the
            // fragments on both sides of (0, 0)'s edges read the same value, the diagonal's tap
            // dropped (the read differs from the full one: the tap was in the footprint).
            let without_diagonal = 0x1ff & !(1 << 4);
            for (lod, inset) in [(0.0, 0.5), (last - 1.0, 100.0), (last - 0.5, 100.0), (last, 100.0)] {
                let corner = Vec2::splat(4096.0);
                let value = across(without_diagonal, corner - Vec2::Y * inset, Vec2::X, resolution, lod);
                across(without_diagonal, corner - Vec2::X * inset, Vec2::Y, resolution, lod);
                let full = ramp_read(&data, 0x1ff, corner - Vec2::Y * inset - Vec2::X * 0.01, resolution, lod, ramp).unwrap();
                assert!((value - full).abs() > 1e-3, "{resolution} lod {lod}: the diagonal's tap had a weight");
            }
            // (1, 0) uncovered: (0, 0)'s read at its east edge is the one clamped to its last
            // column (texel centres half a texel inside the edge), at level 0 and the last level.
            let without_east = 0x1ff & !(1 << 1);
            let edge = Vec2::new(4096.0 - 0.01, 2048.0);
            let clamped = ramp_read(&data, without_east, edge, resolution, 0.0, ramp).unwrap();
            assert!((clamped - ramp(Vec2::new(4096.0 - 2048.0 / resolution as f32, 2048.0))).abs() < 1e-3, "{resolution}: {clamped}");
            let clamped = ramp_read(&data, without_east, edge, resolution, last, ramp).unwrap();
            assert!((clamped - ramp(Vec2::splat(2048.0))).abs() < 1e-3, "{resolution}: the cell's own centre, {clamped}");
        }
    }
    /// [design] Round 10b: the mip filter's uniform and workgroups: each baked cell's atlas
    /// column and row, its count, and one workgroup layer per cell covering that level's
    /// texels per cell side. The filter's WGSL is validated in mesh_blend_shader_tests.rs.
    #[test]
    fn mip_dispatch_covers_each_baked_cell_and_no_more() {
        let words = mip_cells(&[colour_rectangle((-17, 0), (-18, -1), 512), colour_rectangle((-16, 1), (-18, -1), 512)], 512);
        assert_eq!(&words[..8], &[1, 1, 0, 0, 2, 0, 0, 0]);
        assert_eq!(words[MIP_CELLS * 4], 2);
        assert!(words[8..MIP_CELLS * 4].iter().all(|&w| w == 0));
        assert_eq!(mip_workgroups(512, 1, 2), (32, 32, 2), "256 texels per cell side at level 1");
        assert_eq!(mip_workgroups(512, 7, 1), (1, 1, 1), "4 texels: one partial workgroup");
        assert_eq!(mip_workgroups(512, 9, 9), (1, 1, 9), "the last level's one texel per cell");
        assert!(MIP_SHADER.contains(&format!("@workgroup_size({MIP_WORKGROUP}, {MIP_WORKGROUP}, 1)")) && MIP_SHADER.contains(&format!("array<vec4<u32>, {MIP_CELLS}>")));
    }
    /// [design] Round 10b: a replacement that only copies (the entering cells have heights but
    /// no colour capture, so nothing of theirs is drawn) gets no bake target and an inactive
    /// camera: the assembly copies every level of the retained cells and needs no mip pass.
    /// A baking one draws into its bake target with an active camera.
    #[test]
    fn copy_only_replacements_draw_nothing_and_copy_every_level() {
        let (mut app, _files) = cache_fixture();
        for y in -1..=1 {
            let entity = app.world().resource::<Streamer>().loaded[&(2, y)];
            let mut cell = app.world_mut().get_mut::<SurfaceCell>(entity).unwrap();
            cell.quadrants.clear();
            cell.paths = [CapturePath::HeightOnly("missing-base-mesh"); 4];
        }
        move_camera(&mut app, (1, 0));
        tick(&mut app);
        let colour = job_colour(&app).expect("a replacement");
        let mut cameras = app.world_mut().query::<(&BakeView, &Camera, &RenderTarget)>();
        let (view, camera, target) = cameras.single(app.world()).unwrap();
        assert_eq!((view.transfers.cells.len(), view.expected.len()), (6, 0));
        assert!(view.transfers.bake.is_none() && view.transfers.baked.is_empty() && view.transfers.source.is_some());
        assert_eq!(view.transfers.destination.id(), colour.id());
        assert!(!camera.is_active && matches!(target, RenderTarget::None { .. }));
        finish(&mut app);
        let (mut app, _files) = cache_fixture();
        move_camera(&mut app, (1, 0));
        tick(&mut app);
        let mut cameras = app.world_mut().query::<(&BakeView, &Camera, &RenderTarget)>();
        let (view, camera, target) = cameras.single(app.world()).unwrap();
        assert!(camera.is_active);
        assert_eq!(target.as_image().map(Handle::id), view.transfers.bake.as_ref().map(Handle::id), "the camera draws into the bake target, never the published atlas");
        assert_eq!(view.transfers.baked.len(), 3);
    }
    #[test]
    fn epoch_invalidates_before_receipts_even_without_camera_and_off_on() {
        let (mut app, _files) = cache_fixture();
        move_camera(&mut app, (1, 0)); tick(&mut app);
        let stale = app.world().resource::<SurfaceCache>().job.as_ref().unwrap().receipt;
        *app.world().resource::<SurfaceCache>().submitted.lock().unwrap() = Some(stale);
        let camera = app.world_mut().query_filtered::<Entity, With<crate::fly::FlyCam>>().single(app.world()).unwrap();
        app.world_mut().despawn(camera);
        app.world_mut().resource_mut::<WorldData>().world = 42;
        tick(&mut app);
        let cache = app.world().resource::<SurfaceCache>();
        assert!(cache.published.is_none() && cache.job.is_none() && cache.ready.is_none());
        assert!(cache.selected_centre.is_none() && cache.captured_inputs.is_empty() && cache.stitched_dirty.is_empty());
        assert_ne!(cache.epoch, stale.epoch);
        app.world_mut().resource_mut::<WorldData>().world = 0;
        tick(&mut app);
        assert!(app.world().resource::<SurfaceCache>().published.is_none());
    }
    #[test]
    fn interior_and_incompatible_resolution_cancel_pending_work_without_a_camera() {
        for interior in [false, true] {
            let (mut app, _files) = cache_fixture();
            move_camera(&mut app, (1, 0)); tick(&mut app);
            let stale = app.world().resource::<SurfaceCache>().job.as_ref().unwrap().receipt;
            *app.world().resource::<SurfaceCache>().submitted.lock().unwrap() = Some(stale);
            let camera = app.world_mut().query_filtered::<Entity, With<crate::fly::FlyCam>>().single(app.world()).unwrap();
            app.world_mut().despawn(camera);
            if interior { app.world_mut().resource_mut::<Streamer>().interior = Some(42); }
            else { app.world_mut().resource_mut::<SurfaceCache>().resolution = 2048; }
            tick(&mut app);
            let cache = app.world().resource::<SurfaceCache>();
            assert!(cache.published.is_none() && cache.job.is_none() && cache.ready.is_none());
            assert_ne!(cache.epoch, stale.epoch);
        }
    }

    #[test]
    fn copy_only_receipt_waits_for_transfer_resources_and_completion() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        let shared = Arc::new(Mutex::new(None));
        let view = BakeView {
            receipt: Receipt { epoch: 3, request: 4, destination: Handle::<Image>::default().id(), source_revision: 2 },
            height: default(), resources_ready: default(),
            expected: default(), drawn: default(), complete: default(), submitted: shared.clone(),
            transfers: Transfers { source: Some(default()), destination: default(), cells: vec![CellCopy { source: (0, 0), destination: (1024, 2048) }], bake: None, baked: vec![], resolution: 1024 },
        };
        let completed = view.complete.clone();
        let resources = view.resources_ready.clone();
        app.world_mut().spawn(view);
        app.world_mut().run_system_once(submit_receipts).unwrap();
        assert!(shared.lock().unwrap().is_none(), "no receipt when resources/copies are still missing");
        completed.store(true, Ordering::Release);
        app.world_mut().run_system_once(submit_receipts).unwrap();
        assert!(shared.lock().unwrap().is_none(), "a completed copy still requires height and colour GPU resources");
        resources.store(true, Ordering::Release);
        app.world_mut().run_system_once(submit_receipts).unwrap();
        assert_eq!(shared.lock().unwrap().as_ref().map(|r| (r.epoch, r.request)), Some((3, 4)));
        assert_eq!(colour_rectangle((-2, -3), (-3, -3), 1024), (1024, 2048));
        assert_eq!(colour_rectangle((-3, -1), (-3, -3), 1024), (0, 0));
    }
    #[test]
    fn stitched_reconstruction_outside_region_stays_dirty_until_refreshed_after_entry() {
        use bevy::ecs::system::RunSystemOnce;
        let (mut app, _files) = cache_fixture();
        app.insert_resource(crate::terrain::TerrainSettings { tile: 256.0 })
            .init_resource::<crate::terrain::TerrainCache>().init_resource::<crate::mesh_terrain_blend::BlendState>();
        // No LAND records in this fixture: backfill must retain failed refresh work.
        app.add_systems(Update, backfill_cells);
        app.update();
        let source = app.world().resource::<Streamer>().loaded[&(1, 0)];
        let excluded = app.world().resource::<Streamer>().loaded[&(2, 0)];
        let old_revision = app.world().get::<SurfaceCell>(excluded).unwrap().content_revision;
        app.world_mut().get_mut::<SurfaceCell>(source).unwrap().content_revision += 1;
        app.update();
        assert_eq!(app.world().resource::<SurfaceCache>().stitched_dirty.get(&excluded), Some(&(2, 0)));
        move_camera(&mut app, (1, 0)); app.update(); tick(&mut app);
        assert!(app.world().resource::<SurfaceCache>().current.iter().all(|k| k.entity != excluded), "entry cannot copy or bake the stale captured neighbour");
        assert!(app.world().resource::<SurfaceCache>().stitched_dirty.contains_key(&excluded));
        // A successful retry uses the same capture/install path as backfill. It keeps
        // the primary revision, while the replacement component tick invalidates pixels.
        app.world_mut().run_system_once(move |mut commands: Commands, world: Res<WorldData>, mut terrain: crate::terrain::TerrainMaterials, mut models: nvf_assets::model::ModelCtx, mut cache: ResMut<SurfaceCache>, cells: Query<&SurfaceCell>| {
            let old = cells.get(excluded).unwrap();
            let land = fnv_world::land::Land { heights: Some(old.heights.clone()), ..default() };
            let prep = crate::terrain::prepare_land(land, &[None, None, None, None], (2, 0), 256.0);
            let capture = crate::terrain::capture_surface(&world, prep, &mut crate::terrain::TerrainCtx { tm: &mut terrain, assets: &mut models }, false).unwrap();
            let capture = cache.refreshed_capture(excluded, Some(old), capture);
            assert_eq!(capture.content_revision, old_revision);
            commands.entity(excluded).insert(capture);
        }).unwrap();
        for _ in 0..5 { app.update(); tick(&mut app); }
        let cache = app.world().resource::<SurfaceCache>();
        assert!(!cache.stitched_dirty.contains_key(&excluded));
        let fresh = cache.current.iter().find(|k| k.entity == excluded).unwrap();
        assert!(cache.job.as_ref().unwrap().publication.keys.contains(fresh));
        assert!(!work_sets(&mut app).0.contains(&(2, 0)), "the refreshed entering neighbour is never copied from stale pixels");
    }
    #[test]
    fn stitched_pending_is_scoped_to_context_and_entity_incarnation() {
        use bevy::ecs::system::RunSystemOnce;
        let (mut app, _files) = cache_fixture();
        let a = app.world().resource::<Streamer>().loaded[&(1, 0)];
        let b = app.world().resource::<Streamer>().loaded[&(2, 0)];
        {
            let mut cache = app.world_mut().resource_mut::<SurfaceCache>();
            cache.track_stitched_inputs([((1, 0), a, 1), ((2, 0), b, 2)].into_iter());
            cache.track_stitched_inputs([((1, 0), a, 3), ((2, 0), b, 2)].into_iter());
            assert!(cache.stitched_dirty.contains_key(&b));
        }
        let replacement = app.world_mut().spawn_empty().id();
        {
            let mut cache = app.world_mut().resource_mut::<SurfaceCache>();
            cache.track_stitched_inputs([((1, 0), a, 3), ((2, 0), replacement, 4)].into_iter());
            assert!(cache.stitched_dirty.is_empty(), "old incarnation cannot dirty the new capture");
            cache.track_stitched_inputs([((1, 0), a, 5), ((2, 0), replacement, 4)].into_iter());
            assert!(cache.stitched_dirty.contains_key(&replacement));
        }
        app.world_mut().run_system_once(|mut commands: Commands, mut cache: ResMut<SurfaceCache>| cache.release(&mut commands)).unwrap();
        let cache = app.world().resource::<SurfaceCache>();
        assert!(cache.selected_centre.is_none() && cache.captured_inputs.is_empty() && cache.stitched_dirty.is_empty());
    }
    #[test]
    fn stitched_inputs_ignore_first_residency_and_dirty_cardinals_on_reconstruction_only() {
        let mut world = World::new();
        let a = world.spawn_empty().id();
        let b = world.spawn_empty().id();
        let mut inputs = HashMap::new();
        let capture = ((-17, -2), a, 1);
        assert!(stitched_dependents(&mut inputs, [capture].into_iter()).is_empty());
        // An entering neighbour already supplied its LAND layers through WorldData.
        let entering = ((-16, -2), b, 2);
        assert!(stitched_dependents(&mut inputs, [capture, entering].into_iter()).is_empty());
        assert_eq!(stitched_dependents(&mut inputs, [((-17, -2), a, 3), entering].into_iter()), HashSet::from([(-18, -2), (-16, -2), (-17, -3), (-17, -1)]));
        assert!(stitched_dependents(&mut inputs, [((-17, -2), a, 3), entering].into_iter()).is_empty(), "stitched refresh keeps the primary revision and cannot propagate recursively");
    }
    #[test]
    fn source_material_edits_refresh_cloned_capture_inputs_and_dependencies() {
        use bevy::ecs::system::RunSystemOnce;
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        app.init_resource::<Assets<TerrainMaterial>>().init_resource::<Messages<AssetEvent<nvf_assets::NifMaterial>>>();
        let source = app.world_mut().resource_mut::<Assets<nvf_assets::NifMaterial>>().add(nvf_assets::NifMaterial::default());
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
        let mut cell = surface([CapturePath::Fallback { layers: 0 }; 4]);
        cell.sources.push(CaptureSource::Single(source.clone(), false));
        cell.quadrants.push((Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::default()), CaptureDraw::Single(SurfaceCaptureMaterial::from_standard(&StandardMaterial::default(), false))));
        let entity = app.world_mut().spawn(cell).id();
        app.world_mut().resource_mut::<Assets<nvf_assets::NifMaterial>>().get_mut(&source).unwrap().base.base_color_texture = Some(image.clone());
        app.world_mut().resource_mut::<Messages<AssetEvent<nvf_assets::NifMaterial>>>().write(AssetEvent::Modified { id: source.id() });
        app.world_mut().run_system_once(refresh_captures).unwrap();
        let cell = app.world().get::<SurfaceCell>(entity).unwrap();
        assert_eq!(cell.image_dependencies(), &HashSet::from([image.id()]));
        let CaptureDraw::Single(draw) = &cell.quadrants[0].1 else { panic!() };
        assert_eq!(draw.diffuse, Some(image));
    }
    fn synthetic_dds() -> Vec<u8> {
        // A 4x4 BC1 DDS header and one block, as the texture-swap test's.
        let mut dds = vec![0u8; 136];
        dds[..4].copy_from_slice(b"DDS ");
        for (at, value) in [(4, 124u32), (8, 0x1007), (12, 4), (16, 4), (28, 1), (76, 32), (80, 4), (108, 0x1000)] { dds[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
        dds[84..88].copy_from_slice(b"DXT1");
        dds
    }
    #[test]
    fn receipt_requires_all_draws_in_one_frame() {
        let mut world = World::new();
        let a = world.spawn_empty().id();
        let b = world.spawn_empty().id();
        let expected = HashSet::from([a, b]);
        assert!(!draws_ready(&expected, &HashSet::new()));
        assert!(!draws_ready(&expected, &HashSet::from([a])));
        assert!(!draws_ready(&expected, &HashSet::from([b])));
        assert!(draws_ready(&expected, &expected));
        // Round 10b: a copy-only view plans no draw (its camera is inactive), so it is ready
        // at once; a view with neither copies nor draws is never spawned (`update_cache`
        // publishes such a request without one).
        assert!(draws_ready(&HashSet::new(), &HashSet::new()));
        let extra = world.spawn_empty().id();
        assert!(!draws_ready(&expected, &HashSet::from([a, b, extra])), "unplanned draws can contaminate the bake target");
    }

    /// A hilly height block written for these tests (no game data): heights in steps of 8
    /// units (as stored LAND heights come), continuous across the nine cells, so the
    /// neighbours stitch.
    fn hilly_block() -> [[Vec<f32>; 3]; 3] {
        let hilly = |dx: i32, dy: i32| -> Vec<f32> {
            (0..SIDE * SIDE).map(|i| {
                let (c, r) = ((i % SIDE) as f32 + dx as f32 * GRID as f32, (i / SIDE) as f32 + dy as f32 * GRID as f32);
                ((90.0 * (c * 0.37).sin() * (r * 0.23).cos() + 25.0 * ((c + 2.0 * r) * 0.71).sin() + 40.0) / 8.0).round() * 8.0
            }).collect()
        };
        std::array::from_fn(|r| std::array::from_fn(|c| hilly(c as i32 - 1, r as i32 - 1)))
    }
    /// The surface of the block's centre cell: stored, or smoothed with `subdiv`.
    fn block_surface(block: &[[Vec<f32>; 3]; 3], subdiv: Option<usize>) -> fnv_world::land_surface::Surface {
        use fnv_world::land_surface::{SmoothParams, Surface};
        let b: fnv_world::land_surface::Block = std::array::from_fn(|r| std::array::from_fn(|c| Some(block[r][c].as_slice())));
        match subdiv {
            None => Surface::stored(&block[1][1]),
            Some(subdiv) => Surface::smoothed(&b, SmoothParams { subdiv, ..SmoothParams::DEFAULT }).unwrap(),
        }
    }
    /// The geometric normal of the surface's triangle under cell-local `p`, taken from its
    /// vertices with `grid_height_at`'s split (below the diagonal when `u >= v`).
    fn surface_triangle_normal(s: &fnv_world::land_surface::Surface, p: Vec2) -> Vec3 {
        let (sp, side, h) = (s.spacing(), s.side(), s.heights());
        let (fx, fy) = (p.x / sp, p.y / sp);
        let (c, r) = ((fx as usize).min(side - 2), (fy as usize).min(side - 2));
        let (u, v) = (fx - c as f32, fy - r as f32);
        let at = |r: usize, c: usize| Vec3::new(c as f32 * sp, r as f32 * sp, h[r * side + c]);
        let (sw, se, nw, ne) = (at(r, c), at(r, c + 1), at(r + 1, c), at(r + 1, c + 1));
        let n = if u >= v { (se - sw).cross(ne - sw) } else { (ne - sw).cross(nw - sw) };
        n.normalize()
    }
    /// Cell-local sample points of a surface with `sub` squares per LAND square: a scatter
    /// inside squares, LAND vertices, surface vertices, both sides of squares' diagonals,
    /// and the cell's edges (the south and west edges, and just inside the north and east
    /// ones, whose points belong to the next cell).
    fn sample_points(sub: usize) -> Vec<Vec2> {
        let sp = fnv_world::land::SPACING / sub as f32;
        let grid = GRID * sub;
        let mut points = Vec::new();
        let mut s = 0x2661u32;
        let mut next = || { s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); (s >> 8) as f32 / (1u32 << 24) as f32 * 4095.0 };
        for _ in 0..400 { points.push(Vec2::new(next(), next())); }
        for j in (0..GRID).step_by(3) { for i in (0..GRID).step_by(5) { points.push(Vec2::new(i as f32, j as f32) * fnv_world::land::SPACING); } }
        for j in (0..grid).step_by(7) { for i in (0..grid).step_by(5) { points.push(Vec2::new(i as f32, j as f32) * sp); } }
        for (i, j) in [(0, 0), (5, 9), (grid - 1, 2), (17, grid - 1), (grid / 2, grid / 2 + 1)] {
            let centre = Vec2::new(i as f32 + 0.5, j as f32 + 0.5) * sp;
            for side in [-1.0, 1.0] { points.push(centre + Vec2::new(0.2, -0.2) * sp * side); }
            points.push(centre);
        }
        for k in (0..=4000).step_by(173) {
            let t = k as f32;
            points.extend([Vec2::new(0.0, t), Vec2::new(t, 0.0), Vec2::new(4095.99, t), Vec2::new(t, 4095.99)]);
        }
        points
    }

    /// [design] #2149/#2661: the height the shader reads (its CPU twin, `probe_height`) at
    /// every sample point of a resident cell is `Surface::height_at` there, and the normal
    /// it derives (`probe_normal`) the surface triangle's, for the 4 x 4 and 2 x 2 smoothed
    /// surfaces and the stored one, each packed with its own layout.
    #[test]
    fn the_atlas_holds_the_drawn_surface_at_every_subdivision() {
        let block = hilly_block();
        // Cell (0, 0) at slot (1, 1): its cell-local places are its game places, so the
        // twin's and the surface's fractions are computed from the same numbers.
        let origin = (-1, -1);
        for subdiv in [Some(4), Some(2), None] {
            let surface = block_surface(&block, subdiv);
            let layout = HeightLayout::new(surface.sub());
            assert_eq!(surface.side(), layout.side());
            let mut data = empty_height_atlas_rows(layout, layout.rows());
            assert_eq!(data.len(), layout.bytes(false));
            encode_height_cell(layout, &mut data, (1, 1), surface.heights());
            for p in sample_points(surface.sub()) {
                let want = surface.height_at(p.x, p.y).unwrap();
                let got = probe_height(layout, &data, origin, p).unwrap_or_else(|| panic!("{subdiv:?} {p:?}: no heights"));
                assert!((got - want).abs() <= 1e-3, "{subdiv:?} {p:?}: {got} != {want}");
                let n = probe_normal(layout, &data, origin, p).unwrap();
                assert!((n - surface_triangle_normal(&surface, p)).abs().max_element() <= 1e-4, "{subdiv:?} {p:?}: normal {n:?}");
            }
            // The smoothed surface differs from its LAND triangles, so the stored layout's
            // planes would not do (the gap this layout closes).
            if subdiv.is_some() {
                assert!(sample_points(surface.sub()).iter().any(|p| (surface.height_at(p.x, p.y).unwrap() - surface.stored_height_at(p.x, p.y).unwrap()).abs() > 1.0), "{subdiv:?}");
            }
            // Outside the encoded cell: no heights.
            assert_eq!(probe_height(layout, &data, origin, Vec2::new(-10.0, 10.0)), None);
            assert_eq!(probe_normal(layout, &data, origin, Vec2::new(5000.0, 10.0)), None);
        }
    }

    /// [design] #2149/#2661: at the stored layout the packing is the one before the layout
    /// existed (LAND squares, the planes of the stored heights).
    #[test]
    fn stored_layout_packs_the_land_squares_as_before() {
        let block = hilly_block();
        let h = &block[1][1];
        let mut data = empty_height_atlas();
        encode_height_cell(STORED, &mut data, (2, 0), h);
        for (x, y) in [(0, 0), (31, 0), (0, 31), (31, 31), (12, 7)] {
            let i = y * SIDE + x;
            let planes = height_planes(h[i], h[i + 1], h[i + SIDE], h[i + SIDE + 1]);
            for (triangle, plane) in planes.iter().enumerate() { for (c, value) in plane.iter().enumerate() {
                // Texel (2 x 192 + 6 x + 3 t + c, y) of the 576-wide atlas.
                let at = (y * 576 + 2 * 192 + x * 6 + triangle * 3 + c) * 4;
                assert_eq!(f32::from_le_bytes(data[at..at + 4].try_into().unwrap()), *value, "({x}, {y}) {triangle} {c}");
            }}
        }
        // The metadata row is row 32 of the tile.
        encode_colour_mask(STORED, &mut data, (2, 0), 0b1010);
        let at = (32 * 576 + 2 * 192 + 1) * 4;
        assert_eq!(f32::from_le_bytes(data[at..at + 4].try_into().unwrap()), 10.0);
    }

    /// [design] #2149/#2661: at the 4 x 4 surface's layout the metadata row follows the 128
    /// rows of squares and the detail words follow the 387 rows of heights; a table entry
    /// and a vertex record read back as written, and no write reaches another tile's
    /// heights.
    #[test]
    fn metadata_and_detail_words_round_trip_at_the_wider_atlas() {
        use nvf_assets::nif_surface_blend::{detail_quadrant_word, detail_unorm, detail_snorm, detail_vertex_word, detail_vertex_words, DetailQuadrant, DETAIL_QUADRANT_WORDS, DETAIL_VERTEX_WORDS, QUAD_VERTICES};
        let layout = HeightLayout::new(4);
        let mut data = empty_height_atlas_rows(layout, layout.rows() + layout.detail_rows());
        assert_eq!(data.len(), layout.bytes(true));
        assert_eq!(data.len(), 2304 * 438 * 4, "4 x 4: 2304 by 387 texels of heights, 51 detail rows");
        let block = hilly_block();
        let surface = block_surface(&block, Some(4));
        let slots: HashSet<_> = [(0, 0), (1, 1), (2, 2)].into();
        let cell = SurfaceCell { heights: surface.heights().to_vec(), ..surface_cell_with([CapturePath::Fallback { layers: 0 }; 4]) };
        for &slot in &slots { encode_surface_cell(layout, &mut data, slot, &cell, &slots); }
        encode_colour_mask(layout, &mut data, (1, 1), 0b0101);
        encode_detail_mask(layout, &mut data, (1, 1), 0b0011);
        assert_eq!((colour_mask(layout, &data, (1, 1)), detail_mask(layout, &data, (1, 1)), colour_mask(layout, &data, (2, 2))), (0b0101, 0b0011, 0xf));
        assert_eq!(metadata(layout, &data, (1, 1), 0), residency_mask((1, 1), &slots));
        // Texel 1 of slot (1, 1)'s metadata row: row 129 + 128, column 768 + 1.
        let at = ((129 + 128) * 2304 + 768 + 1) * 4;
        assert_eq!(f32::from_le_bytes(data[at..at + 4].try_into().unwrap()), 5.0);
        // The heights around the metadata are the surface's (the last square row, and slot
        // (1, 1)'s first square above slot (1, 0)'s metadata row).
        let origin = (0, 0);
        for p in [Vec2::new(4096.0 + 1.0, 4096.0 + 4095.0), Vec2::new(4096.0 + 2000.0, 4096.0 + 0.5), Vec2::new(4096.0 * 2.0 + 4095.5, 4096.0 * 2.0 + 4095.5)] {
            let local = p - Vec2::new((p.x / 4096.0).floor(), (p.y / 4096.0).floor()) * 4096.0;
            assert!((probe_height(layout, &data, origin, p).unwrap() - surface.height_at(local.x, local.y).unwrap()).abs() <= 1e-3, "{p:?}");
        }
        assert_eq!(lookup_status(layout, &data, origin, Vec2::new(4096.0 + 100.0, 4096.0 + 100.0), 512), LookupStatus::Success);
        assert_eq!(lookup_status(layout, &data, origin, Vec2::new(4096.0 + 3000.0, 4096.0 + 100.0), 512), LookupStatus::InvalidColourQuadrant);
        assert_eq!(lookup_status(layout, &data, origin, Vec2::new(4096.0 + 100.0, 100.0), 512), LookupStatus::InvalidHeight, "slot (1, 0) holds no cell");
        // The detail words after the heights: the last quadrant's entry and last vertex.
        let entry = detail_entry((2, 2), 3);
        let quadrant = DetailQuadrant { base_diffuse: Some(3), base_normal: Some(4), diffuse_sampler: 1, normal_sampler: 2, overlays: vec![(5, Some(6), true)], folded: 1, tile: 256.0, tangent_sign: -1.0, ..default() };
        let n = QUAD_VERTICES;
        let tint = vec![[0.25, 0.5, 1.0]; n];
        let normal = vec![[0.6, -0.8, 0.0]; n];
        let tangent = vec![[0.0, 0.6, -0.8]; n];
        let detail = QuadrantDetail {
            base: DetailBase::Single(Handle::default()), overflow: vec![], cell_local: false, tile: 256.0,
            tint, normal, tangent, tangent_sign: -1.0, opacity: vec![(0..n).map(|v| v as f32 / (n - 1) as f32).collect()],
            resolved: Err("not-resolved"),
        };
        let before = data.clone();
        encode_detail(layout, &mut data, entry, &quadrant, &detail);
        assert_eq!(data[..layout.rows() * layout.width() * 4], before[..layout.rows() * layout.width() * 4], "the detail words never reach the heights");
        let words: Vec<u32> = (0..DETAIL_QUADRANT_WORDS).map(|i| detail_word(layout, &data, detail_quadrant_word(entry) + i)).collect();
        assert_eq!(words, quadrant.words());
        let last = n - 1;
        let record: Vec<u32> = (0..DETAIL_VERTEX_WORDS).map(|j| detail_word(layout, &data, detail_vertex_word(entry, last) + j)).collect();
        assert_eq!(record, detail_vertex_words([0.25, 0.5, 1.0], [0.6, -0.8, 0.0], [0.0, 0.6, -0.8], &[1.0]));
        assert!((detail_unorm(&record, 1) - 0.5).abs() <= 1.0 / 65535.0 && (detail_snorm(&record, 4) + 0.8).abs() <= 1.0 / 32767.0 && detail_unorm(&record, 9) == 1.0, "the last vertex's green, normal y and opacity");
        // The last word lies inside the atlas: the detail rows are enough at this width.
        assert!((layout.rows() * layout.width() + detail_vertex_word(entry, last) + DETAIL_VERTEX_WORDS) * 4 <= data.len());
    }

    /// [design] #2149/#2661: a publication at the 4 x 4 surface's layout packs the drawn
    /// surface's heights (its probe reads `Surface::height_at`), names the layout for its
    /// clones, and refuses a cell whose heights are not that surface's
    /// (`invalid-prepared-heights`, as heights of no surface are).
    #[test]
    fn a_publication_packs_the_run_surface_and_refuses_other_heights() {
        use bevy::ecs::system::RunSystemOnce;
        let layout = HeightLayout::new(4);
        let block = hilly_block();
        let surface = block_surface(&block, Some(4));
        let mut app = App::new();
        app.insert_resource(crate::outfits::tests::empty_world_data())
            .init_resource::<Assets<Image>>().init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<TerrainMaterial>>().init_resource::<Assets<SurfaceCaptureMaterial>>()
            .insert_resource(SurfaceCache { layout, ..SurfaceCache::with_resolution(512) });
        let fine = app.world_mut().spawn(SurfaceCell { grid: (0, 0), heights: surface.heights().to_vec(), ..surface_cell_with([CapturePath::Fallback { layers: 0 }; 4]) }).id();
        let stored = app.world_mut().spawn(SurfaceCell { grid: (1, 0), heights: block[1][1].clone(), ..surface_cell_with([CapturePath::Fallback { layers: 0 }; 4]) }).id();
        app.insert_resource(Streamer {
            radius: 1, loaded: [((0, 0), fine), ((1, 0), stored)].into(), fades: default(), fade_secs: 0.0, settled: true,
            arrival: None, per_frame: 1, spawning: default(), dying: default(), ahead: default(),
            hide: default(), interior: None, light_scale: 1.0, managed: default(), manage_actors: false,
            stats: default(), buffer: default(), centre: None, waiting: default(), model_overrides: default(), coop_areas: Vec::new(), travels: 0,
        });
        app.world_mut().spawn((Camera3d::default(), crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, GlobalTransform::from_translation(Vec3::new(2048.0, 600.0, -2048.0))));
        app.world_mut().run_system_once(update_cache).unwrap();
        let cache = app.world().resource::<SurfaceCache>();
        let job = cache.job.as_ref().expect("a publication is pending");
        assert_eq!((job.publication.layout, job.publication.cells, job.publication.origin), (layout, 1, (-1, -1)));
        assert!(job.report.contains("excluded=[") && job.report.contains("(1, 0):invalid-prepared-heights"), "{}", job.report);
        let image = app.world().resource::<Assets<Image>>().get(&job.publication.height).unwrap();
        assert_eq!((image.texture_descriptor.size.width as usize, image.texture_descriptor.size.height as usize), (layout.width(), layout.rows()));
        let data = image.data.as_ref().unwrap();
        for p in sample_points(4) {
            assert!((probe_height(layout, data, (-1, -1), p).unwrap() - surface.height_at(p.x, p.y).unwrap()).abs() <= 1e-3, "{p:?}");
        }
        assert_eq!(lookup_status(layout, data, (-1, -1), Vec2::splat(1024.0), 512), LookupStatus::Success);
    }

    /// [design] #2149/#2661: the height atlas is checked against the device's 2D texture
    /// limit as the colour atlas is, and refused the same way (nothing bakes).
    #[test]
    fn start_refuses_a_height_atlas_the_device_cannot_hold() {
        let fine = HeightLayout::new(4);
        assert_eq!((height_atlas_rows(fine, false), height_atlas_rows(fine, true)), (387, 438));
        for (layout, max, rejected) in [(STORED, 2048, false), (fine, 2048, true), (fine, 2304, false), (HeightLayout::new(2), 2048, false), (fine, 4096, false)] {
            let mut cache = SurfaceCache::with_resolution(512);
            cache.start(512, Some(max), true, layout);
            assert_eq!((cache.rejected, cache.layout), (rejected, layout), "{layout:?} {max}");
        }
        // Without a render device nothing is checked.
        let mut cache = SurfaceCache::with_resolution(512);
        cache.start(512, None, true, fine);
        assert!(!cache.rejected);
        assert!(height_atlas_supported(STORED, true, 576) && !height_atlas_supported(STORED, true, 575));
    }

    /// [design] #2149/#2661: the production capture of a smoothed cell (`capture_surface`
    /// over a prep on a 4 × 4 surface) gives the surface's own heights, and its bake copies
    /// at the LAND grid's resolution (terrain.rs `land_resolution`); the detail still reads
    /// 17 × 17 vertices.
    #[test]
    fn a_smoothed_capture_carries_the_surface_heights_and_land_resolution_copies() {
        use bevy::ecs::system::RunSystemOnce;
        use fnv_world::land::{Land, Layer, QUAD_SIDE};
        use fnv_world::land_surface::{QuadrantDraw, SmoothParams, Surface};
        let grid = (-17, 0);
        let block = hilly_block();
        let b: fnv_world::land_surface::Block = std::array::from_fn(|r| std::array::from_fn(|c| Some(block[r][c].as_slice())));
        let surface = Surface::smoothed(&b, SmoothParams::DEFAULT).unwrap();
        assert_eq!(surface.sub(), 4);
        let centre = ((QUAD_SIDE / 2) * QUAD_SIDE + QUAD_SIDE / 2) as u16;
        let mut layers = Vec::new();
        for q in 0..4u8 {
            layers.push(Layer { quadrant: q, texture: None, layer: -1, base: true, alpha: vec![] });
            layers.push(Layer { quadrant: q, texture: None, layer: 0, base: false, alpha: vec![(centre, 0.75)] });
        }
        let land = Land { heights: Some(block[1][1].clone()), layers, ..default() };
        for folds in [true, false] {
            let (mut app, _files) = crate::outfits::tests::asset_fixture();
            app.insert_resource(crate::outfits::tests::empty_world_data())
                .insert_resource(crate::terrain::TerrainSettings { tile: 256.0 })
                .init_resource::<crate::terrain::TerrainCache>()
                .init_resource::<Assets<TerrainMaterial>>()
                .init_resource::<Assets<SurfaceCaptureMaterial>>()
                .init_resource::<SurfaceCache>();
            app.world_mut().resource_mut::<nvf_assets::TextureCache>()
                .offer(fnv_vfs::texture_path(fnv_world::land::DEFAULT_DIFFUSE), synthetic_dds(), 0.0);
            let (prep_land, prep_surface) = (land.clone(), surface.clone());
            let captured = app.world_mut().run_system_once(move |world: Res<WorldData>, mut terrain: crate::terrain::TerrainMaterials, mut models: nvf_assets::model::ModelCtx| {
                let prep = crate::terrain::prepare_land_on(prep_land.clone(), prep_surface.clone(), &[None, None, None, None], grid, 256.0, QuadrantDraw::FULL);
                crate::terrain::capture_surface(&world, prep, &mut crate::terrain::TerrainCtx { tm: &mut terrain, assets: &mut models }, folds)
            }).unwrap().expect("heights give a capture");
            assert_eq!(captured.heights, surface.heights(), "folds={folds}: the surface's own heights");
            assert_eq!(captured.heights.len(), HeightLayout::new(4).side().pow(2));
            assert_eq!(captured.colour_mask(), 0xf, "folds={folds}");
            assert!(!captured.quadrants.is_empty());
            for (mesh, _) in &captured.quadrants {
                assert_eq!(mesh.count_vertices(), QUAD_SIDE * QUAD_SIDE, "folds={folds}: a bake copy at the LAND grid's resolution");
            }
            for d in &captured.detail {
                let DetailCapture::Layers(d) = d else { panic!("folds={folds}: {d:?}") };
                assert_eq!((d.tint.len(), d.normal.len()), (QUAD_SIDE * QUAD_SIDE, QUAD_SIDE * QUAD_SIDE));
            }
        }
    }

    /// [design] #2757: the bake camera renders an unlit RGBA16F atlas with `Tonemapping::None`
    /// and must attach `DebandDither::Disabled` in both active-bake and copy-only configurations
    /// so Bevy does not emit a per-frame warning.
    #[test]
    fn bake_camera_disables_deband_dither_beside_tonemapping_none() {
        let (mut app, _files) = cache_fixture();
        move_camera(&mut app, (1, 0));
        tick(&mut app);
        let mut cameras = app.world_mut().query_filtered::<(&Tonemapping, &DebandDither, &Msaa), With<BakeView>>();
        let (tonemapping, dither, msaa) = cameras.single(app.world()).unwrap();
        assert_eq!((*tonemapping, *dither, *msaa), (Tonemapping::None, DebandDither::Disabled, Msaa::Off));

        let (mut app, _files) = cache_fixture();
        for y in -1..=1 {
            let entity = app.world().resource::<Streamer>().loaded[&(2, y)];
            let mut cell = app.world_mut().get_mut::<SurfaceCell>(entity).unwrap();
            cell.quadrants.clear();
            cell.paths = [CapturePath::HeightOnly("missing-base-mesh"); 4];
        }
        move_camera(&mut app, (1, 0));
        tick(&mut app);
        let mut cameras = app.world_mut().query_filtered::<(&Tonemapping, &DebandDither, &Msaa), With<BakeView>>();
        let (tonemapping, dither, msaa) = cameras.single(app.world()).unwrap();
        assert_eq!((*tonemapping, *dither, *msaa), (Tonemapping::None, DebandDither::Disabled, Msaa::Off));
    }
}
