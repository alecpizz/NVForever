//! Terrain: one mesh per LAND quadrant for the base texture, plus one alpha-blended copy
//! per additional texture layer whose vertex alpha is the layer's opacity.
//!
//! This multi-pass layering is an interim technique with Bevy's standard material; a
//! single-pass splatting shader replaces it once the layer rules are confirmed against
//! in-game screenshots (docs/design/layer3.md). Facts used: docs/spec-notes/world.md §3.
//! Hypotheses here, each to be checked against screenshots:
//! - VTXT positions index the 17 × 17 quadrant grid row by row, rows south → north;
//! - layers draw in ascending `layer` order over the quadrant's base texture;
//! - the normal map (TXST TX01) is a tangent-space map in the frame of the texture
//!   coordinates: red towards +u (east), green towards +v (north), [`TANGENT_SIGN`];
//!   its alpha (a specular mask on NIF shapes) and the LTEX specular exponent are unused;
//! - each grid square is split along its south-west → north-east diagonal;
//! - a quadrant without BTXT, or a layer with a null texture, uses the INI default
//!   ([`DEFAULT_DIFFUSE`]); drawing such quadrants untextured left hard-edged patches.
//!
//! Measured [observed, world.md §3 "Texture repeat"]: textures repeat every 256 world
//! units ([`TerrainSettings::tile`]) with u = x / 256 and v = y / 256, v growing northwards.
//!
//! # The surface: stored or smoothed
//!
//! The meshes are built over the cell's landscape surface (`fnv_world::land_surface`): the
//! LAND grid as stored, or smoothed (#2149, on by default, `nvf_play::smooth_terrain`),
//! each grid square then cut into `subdiv` × `subdiv` squares (4 by default) on a smooth
//! detail patch (thresholds 2:6, bound 8 by default), the same surface the collision is
//! built on. Smoothed, the normals are the
//! surface's own; the vertex colours and the layers' opacities are interpolated on the
//! stored triangles, so the paint lies where it did [design, #2149]. Stored, the meshes are
//! what they always were (VNML normals, the record's colours and opacities).
//!
//! # One draw for the base and its first layers
//!
//! Each blended layer is a second lighting of the ground under it: at Goodsprings a
//! ground triangle carries 2.2 painted layers on average, so the ground was lit 3.2 times
//! [observed 2026-11-10: 50 cells, 102,400 base triangles, 224,778 painted layer
//! triangles; docs/design/frame-rate-audit.md §9]. Where the GPU has bindless textures
//! ([`TerrainMaterials::folds`]), a quadrant's first [`folded_layers`] layers are folded
//! into its base instead: one opaque mesh ([`folded_mesh`]) that carries the layers'
//! opacities at each vertex, with a [`TerrainMaterial`] holding their textures, whose
//! shader (terrain.wesl) blends the surface colour and the normal in the layers' order
//! and lights the result once. That is [`FOLDED_LAYERS`] layers, every layer of every
//! quadrant seen (up to 6), when the texture repeat divides the cell: the texture
//! coordinate is then the vertex's place in the cell over the repeat, which the shader
//! works out itself, and its slot carries two more opacities. Otherwise it is
//! [`FOLDED_LAYERS_WITH_UV`], and a quadrant's later layers keep their blended draws over
//! it, in the same order as before (at Goodsprings the fifth and sixth layers are 15 % of
//! the painted triangles and cost 0.18 ms a frame at 2560 x 1440 [observed 2026-11-10]).
//!
//! What differs in the picture: the layers' *surfaces* are blended and then lit, where
//! their *lit colours* were blended. The two agree where a layer is fully opaque or
//! absent; in between, the bumps of two normal maps are mixed before the light falls on
//! them [engineering choice: the interim technique above was to be replaced by this].
//! Without bindless textures (a material would need eight more textures than such a GPU
//! can bind beside the standard ones) every layer keeps its blended draw.
//!
//! # Height blending
//!
//! [design, #2550; docs/design/graphics-enhanced.md §3.12] The settings switch Terrain
//! height blending (`Graphics::terrain_height_blend`, off by default and in both presets)
//! gives the folded material's pipeline `TERRAIN_HEIGHT_BLEND` ([`TerrainLayersKey`]):
//! terrain.wesl then turns the ordered blend into a weight for the base and each folded
//! layer and competes within a depth of the highest share-plus-height score. Height is
//! raw decoded texel brightness less a global or local reference, so a layer comes
//! in through its bright stones and clumps first (terrain_weights.wesl, mirrored on
//! the CPU in the tests). A share of 0 stays 0 and a sole contributor keeps 1: fully painted
//! and unpainted ground looks as without it. The switch flips the materials in place
//! ([`sync_height_blend`]); new materials take the current setting. Start-up test aid
//! `NVF_PLAY_TEST_HEIGHT_BLEND=depth:gain[:source]` configures it without enabling it:
//! defaults `0.10:8:global` (the user's choice from in-game comparisons, 2026-11-15); `local` subtracts a coarser mip at the same UV. Equal heights
//! sharpen unequal shares too; colour and normals use the same resulting weights.
//!
//! Only the folded draw is weighted: a quadrant's later layers that keep their blended
//! draws (past [`folded_layers`]), and every layer where the GPU has no bindless textures,
//! blend as before. Their coverage over the folded result is unchanged, but height does not
//! compete across that draw boundary.
//!
//! # Heights from a diffuse's alpha
//!
//! [design, #3486; docs/spec-notes/pbr-materials.md "Heights from a ground pack's diffuse
//! alpha"] The ground packs made for the original's terrain parallax shaders keep a height map
//! in the landscape diffuse's alpha (DXT5). While Terrain parallax is on, a folded layer or base whose
//! diffuse the authored index knows no set for takes that alpha as its height map
//! (nvf_assets `TextureCache::alpha_height`: the alpha's blocks copied to a BC4 texture), at
//! one depth for every pack. From there it is a height map like any other: same slot, same
//! march, same blend. The switch gives them to the materials and takes them away in place
//! ([`sync_alpha_heights`]); their images then go with the texture cache's usual grace.
//!
//! # Anti-tiling
//!
//! [design, #2551; docs/design/graphics-enhanced.md §3.13] The settings switch Terrain
//! anti-tiling (`Graphics::terrain_anti_tiling`, off by default and in both presets, not an
//! effect of the Enhanced effects row) gives the folded material's pipeline
//! `TERRAIN_ANTI_TILING` ([`TerrainLayersKey`], nvf_assets `terrain_sampling::anti_tiling_defs`):
//! every texture of the quadrant (the base's and the folded layers', diffuse and normal map)
//! is read at up to three offset and rotated places (Hex3) by a hash of a lattice
//! four repeats on a side laid over the world, and the reads are blended, so the 256-unit
//! repeat no longer shows as a grid (nvf_assets terrain_sampling.wesl and terrain_pbr.wesl).
//! The lattice hangs on world coordinates, so neighbouring quadrants and cells read the
//! same place the same way. Reads take explicit gradients from the plain coordinate
//! (scaled by the view's mip bias), so the hash borders cause no mip jumps. The layers are
//! walked last to first and a texture is read only where its share can be above 0; with
//! height blending on too, the heights come from the same reads. The switch flips the
//! materials in place ([`sync_anti_tiling`]); new materials take the current setting. The
//! texture repeat travels in the material ([`TerrainLayers::texture_repeat`]), also where
//! the mesh carries the coordinate (a repeat that does not divide the cell).
//!
//! The landscape's own NIF materials get the same reads ([`TerrainCtx::base_material`],
//! [`TerrainCtx::layer_material`]: their texture repeat and the switch in
//! `NifSurface::terrain_repeat` and `anti_tiling`; nvf_assets nif_material.rs "Landscape
//! anti-tiling"): a quadrant's base without folded layers, the layers past
//! [`folded_layers`] drawn blended over the folded result, and every layer where the GPU has
//! no bindless textures. Their order, blending and depth bias are as before, and the same
//! world lattice lines them up with the folded draw under them.
//!
//! Not anti-tiled: the depth prepass and the shadows (same depth either way), the normal
//! prepass's normals (screen-space passes that read them keep the plain reads), and the
//! distant LOD terrain's baked textures. Height blending stays within the folded draw.
//!
//! # Parallax
//!
//! [design, #3322; docs/spec-notes/pbr-materials.md "Terrain layers: parallax"] The settings
//! switch Terrain parallax (`Graphics::terrain_parallax`, off by default and in both presets;
//! it needs the run's Authored materials state) gives a folded material that holds at least
//! one height map the pipeline `TERRAIN_PARALLAX` ([`TerrainLayersKey`]). The height maps are
//! authored sets' `_p`, found by each folded layer's diffuse path as a NIF
//! shape's diffuse finds its set (nvf_assets `TextureCache::authored_height`), and loaded
//! with the material whenever the run's Authored materials state is on, so the switch flips
//! the materials in place ([`sync_parallax`]) as height blending does; only materials that
//! hold a height map are edited, noted for the frame ([`ParallaxEdits`]), and the mesh
//! terrain blend does not recapture for those edits alone when what its captures are made
//! from compares equal (the bake copy drops the switch). terrain.wesl marches once over the
//! blended depth, each layer's ordered share of the painted opacity
//! x its own set's depth x (1 - its height), on one step grid for every quadrant
//! ([`PARALLAX_SCALE`]), then reads every texture at the one shifted coordinate, the layers
//! blended by the march's own shares; the march fades out
//! between [`PARALLAX_FADE`]'s distances, well inside the loaded cells. The base is marched as
//! a layer is, by its share and its own height map (#3846); a quadrant without painted layers
//! whose base has one (or may have, [`TerrainCtx::base_may_march`]) is drawn by the folded
//! material with no layers for it, so nothing jumps at its edge. A layer or base without a
//! height map adds no depth (flat at the surface), so the shift falls to nothing where those
//! with height maps fade out, and a layer moves as far on both sides of a quadrant edge
//! whatever else either quadrant holds. Never with anti-tiling
//! (the key leaves parallax off while it is on): its offset and turned reads take their
//! place from the world, and the march would read other texels than the ones drawn. Only the
//! folded draw's main pass moves its texture coordinates: the depth prepass, the shadows,
//! the collision, the blended layer draws past [`folded_layers`], the separate landscape
//! materials (GPUs without bindless textures), the surface bake of the mesh terrain blend
//! and the distant LOD keep the unshifted surface. A material without a height map keeps the
//! old pipeline, so without a pack the switch changes nothing.

use crate::world::WorldData;
use fnv_esm::{LoadOrder, Schema as EsmSchema};
use bevy::asset::{load_internal_asset, uuid_handle, RenderAssetUsages};
use bevy::ecs::system::SystemParam;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use fnv_world::land::{land_texture, load_land, Land, LandParts, Layer, DEFAULT_DIFFUSE, DEFAULT_NORMAL, QUAD_SIDE, SIDE};
use fnv_world::land_surface::{block_of, load_heights, LandIds, QuadrantDraw, Surface, SurfaceSpec};
use nvf_assets::model::ModelCtx;
use nvf_assets::nif_material::sort_first;
use nvf_assets::{nif_material, NifMaterial};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

#[derive(Resource)]
pub struct TerrainSettings {
    /// World units per texture repeat (256 in the game [observed, see module docs]).
    pub tile: f32,
}

#[derive(Resource, Default)]
pub struct TerrainCache {
    /// LTEX FormID → diffuse texture path.
    textures: HashMap<u32, Option<String>>,
    /// Diffuse texture path → normal map path (the LTEX's TXST TX01).
    normals: HashMap<String, String>,
    base: HashMap<Option<String>, Handle<NifMaterial>>,
    layer: HashMap<(String, usize), Handle<NifMaterial>>,
    /// Base texture and folded layer textures → the material of a folded quadrant.
    folded: HashMap<(Option<String>, Vec<String>, bool), Handle<TerrainMaterial>>,
    /// Quadrants drawn with their first layers folded in, and the layers folded.
    pub folded_quadrants: usize,
    pub folded_layers: usize,
    /// Layers drawn blended over a folded quadrant (past [`folded_layers`]), so far.
    pub overflow_layers: usize,
    /// Quadrants without painted layers drawn by the folded material (#3846), so far.
    pub marched_bases: usize,
    /// Folded materials made with at least one layer's authored height map (module notes,
    /// "Parallax"; not the base's), so far.
    pub parallax_materials: usize,
    /// Folded materials made with at least one height from a diffuse's alpha, so far.
    pub alpha_height_materials: usize,
    alpha_height_textures: HashSet<String>,
    /// What [`TerrainCtx::base_may_march`] found, by diffuse path.
    base_marches: HashMap<String, bool>,
    pub quadrants_without_base: usize,
    pub layers: usize,
    /// Cells drawn on the smoothed surface and on the stored one (#2149).
    pub smoothed_cells: usize,
    pub stored_cells: usize,
    /// 4 × 4 near's quadrant levels (#2676, `crate::terrain_lod`); `None` unless the run
    /// chose it (made when its first cell spawns).
    pub lod: Option<crate::terrain_lod::TerrainLod>,
}

impl TerrainCache {
    /// The landscape materials held, for the stress census.
    pub fn share(&self, materials: &Assets<NifMaterial>) -> nvf_assets::release::CacheShare {
        let mut s = nvf_assets::release::CacheShare::default();
        for h in self.base.values().chain(self.layer.values()) {
            s.material(materials, h, 1);
        }
        s
    }
}

/// The layers of a quadrant folded into its base's draw (module notes, "One draw for the
/// base and its first layers") when the shader works the texture coordinate out, and when
/// the mesh has to carry it.
pub const FOLDED_LAYERS: usize = 6;
pub const FOLDED_LAYERS_WITH_UV: usize = 4;

/// The base's place after the layers' in [`TerrainLayers::height_bits`] and
/// [`TerrainLayers::alpha_heights`], and its index in terrain.wesl's `layer_height` (#3846).
pub const BASE_HEIGHT: usize = FOLDED_LAYERS;

/// The layers' bits of those, without the base's.
const LAYER_BITS: u32 = (1 << FOLDED_LAYERS) - 1;

/// How many layers a quadrant folds with textures repeating every `tile` units: all
/// [`FOLDED_LAYERS`] when a cell holds a whole number of repeats (16 at the game's 256),
/// since the texture coordinate of a vertex is then its place in the cell over `tile`,
/// but for whole repeats.
pub fn folded_layers(tile: f32) -> usize {
    if tile > 0.0 && (fnv_world::CELL_SIZE / tile).fract() == 0.0 {
        FOLDED_LAYERS
    } else {
        FOLDED_LAYERS_WITH_UV
    }
}

/// The landscape material of a quadrant whose first layers are folded into the base:
/// the base's StandardMaterial with the layers' textures (terrain.wesl).
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainLayers>;

const SHADER: &str = "embedded://nvforever/terrain.wesl";
const SURFACE_BAKE_SHADER: Handle<Shader> = uuid_handle!("6da97f8c-732c-4697-b345-9a8ac21c2661");

/// The folded layers' textures, in the layers' order, and how many there are. The values
/// and texture indices sit in bindless tables beside StandardMaterial's, as NifSurface's
/// do (nvf_assets nif_material.rs "Bindless"), so quadrants still draw in one batch.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
#[data(50, TerrainLayersUniform, binding_array(101))]
#[bindless(limit(512), index_table(range(50..71), binding(100)))]
#[bind_group_data(TerrainLayersKey)]
pub struct TerrainLayers {
    /// [design] Dedicated unlit surface capture of a folded base, terrain_surface.rs
    /// (every other landscape draw is captured with its `SurfaceCaptureMaterial`).
    pub surface_bake: bool,
    /// Layers held (at most [`FOLDED_LAYERS`]).
    pub count: u32,
    /// Height blending (module notes, "Height blending"): selects the pipeline
    /// ([`TerrainLayersKey`]).
    pub height_blend: bool,
    /// Anti-tiling (module notes, "Anti-tiling"): selects the pipeline
    /// ([`TerrainLayersKey`]) when [`texture_repeat`](Self::texture_repeat) can place it.
    pub anti_tiling: bool,
    /// World units per texture repeat, always (anti-tiling's coordinate), also where the
    /// mesh carries the texture coordinate and [`tile`](Self::tile) is 0.
    pub texture_repeat: f32,
    /// World units per texture repeat when the shader works the texture coordinate out
    /// (the mesh's first texture coordinate then holds the fifth and sixth opacities);
    /// 0 when the mesh carries it.
    pub tile: f32,
    /// Parallax (module notes, "Parallax"): selects the pipeline ([`TerrainLayersKey`]) when
    /// the material holds a height map ([`TerrainLayers::holds_heights`]).
    pub parallax: bool,
    /// Each layer's depth in texture-coordinate units, its height map's set's (0 for a layer
    /// without one) [design, #3322: kept per layer, so a layer moves as far wherever it lies].
    pub parallax_depths: [f32; FOLDED_LAYERS],
    /// The base's depth (0 without a height map, #3846).
    pub base_depth: f32,
    /// Bit k set when layer k's height map, bit [`BASE_HEIGHT`] when the base's, is its
    /// diffuse's alpha (module notes, "Heights from
    /// a diffuse's alpha"); none while the switch is off ([`sync_alpha_heights`]).
    pub alpha_heights: u32,
    #[texture(51)]
    pub diffuse0: Option<Handle<Image>>,
    #[texture(52)]
    pub normal0: Option<Handle<Image>>,
    #[texture(53)]
    pub diffuse1: Option<Handle<Image>>,
    #[texture(54)]
    pub normal1: Option<Handle<Image>>,
    #[texture(55)]
    pub diffuse2: Option<Handle<Image>>,
    #[texture(56)]
    pub normal2: Option<Handle<Image>>,
    #[texture(57)]
    pub diffuse3: Option<Handle<Image>>,
    #[texture(58)]
    pub normal3: Option<Handle<Image>>,
    #[texture(59)]
    pub diffuse4: Option<Handle<Image>>,
    #[texture(60)]
    pub normal4: Option<Handle<Image>>,
    #[texture(61)]
    pub diffuse5: Option<Handle<Image>>,
    #[texture(62)]
    pub normal5: Option<Handle<Image>>,
    /// The grass fires' burn map (`crate::grass::GrassBurn`): burnt ground is drawn
    /// scorched (terrain.wesl "Scorch"). `None`: no fires, nothing scorched.
    #[texture(63)]
    pub burn: Option<Handle<Image>>,
    /// The height maps of each folded layer's diffuse (an authored set's `_p`, module notes
    /// "Parallax", or the diffuse's alpha, "Heights from a diffuse's alpha"), in the layers'
    /// order, and the base's after them (#3846); `None` where there is none (flat for the
    /// march) and always while the run's Authored materials state is off.
    #[texture(64)]
    pub height0: Option<Handle<Image>>,
    #[texture(65)]
    pub height1: Option<Handle<Image>>,
    #[texture(66)]
    pub height2: Option<Handle<Image>>,
    #[texture(67)]
    pub height3: Option<Handle<Image>>,
    #[texture(68)]
    pub height4: Option<Handle<Image>>,
    #[texture(69)]
    pub height5: Option<Handle<Image>>,
    #[texture(70)]
    pub height_base: Option<Handle<Image>>,
}

impl TerrainLayers {
    /// The layers' height maps in the layers' order, to be changed.
    fn heights_mut(&mut self) -> [&mut Option<Handle<Image>>; FOLDED_LAYERS] {
        [&mut self.height0, &mut self.height1, &mut self.height2, &mut self.height3, &mut self.height4, &mut self.height5]
    }

    /// Layer `k` (the base for [`BASE_HEIGHT`]) takes the alpha height `height`
    /// ([`Self::alpha_heights`]), or loses it.
    fn set_alpha_height(&mut self, k: usize, height: Option<(Handle<Image>, f32)>) {
        let (image, depth) = height.map_or((None, 0.0), |(h, d)| (Some(h), d));
        self.alpha_heights = self.alpha_heights & !(1 << k) | u32::from(image.is_some()) << k;
        if k == BASE_HEIGHT {
            (self.height_base, self.base_depth) = (image, depth);
        } else {
            self.parallax_depths[k] = depth;
            *self.heights_mut()[k] = image;
        }
    }

    /// Whether layer `k` (the base for [`BASE_HEIGHT`]) has a height map, drawn or not.
    fn has_height(&self, k: usize) -> bool {
        if k == BASE_HEIGHT { self.height_base.is_some() } else { self.heights()[k].is_some() }
    }

    /// The layers' height maps in the layers' order.
    pub fn heights(&self) -> [&Option<Handle<Image>>; FOLDED_LAYERS] {
        [&self.height0, &self.height1, &self.height2, &self.height3, &self.height4, &self.height5]
    }

    /// Bit k set when layer k has a height map, among the layers drawn (the uniform's layer
    /// count), and bit [`BASE_HEIGHT`] when the base has one.
    pub fn height_bits(&self) -> u32 {
        let most = if self.tile > 0.0 { FOLDED_LAYERS } else { FOLDED_LAYERS_WITH_UV };
        let held = self.count.min(most as u32) as usize;
        let base = u32::from(self.height_base.is_some()) << BASE_HEIGHT;
        self.heights().iter().take(held).enumerate().filter(|(_, h)| h.is_some()).fold(base, |b, (k, _)| b | 1 << k)
    }

    /// Each layer's depth as the march takes it: its own where it is drawn and has a height
    /// map, else 0.
    pub fn layer_depths(&self) -> [f32; FOLDED_LAYERS] {
        let bits = self.height_bits();
        std::array::from_fn(|k| if bits & 1 << k != 0 { self.parallax_depths[k].clamp(0.0, PARALLAX_SCALE) } else { 0.0 })
    }

    /// The base's depth as the march takes it: 0 without a height map.
    pub fn marched_base_depth(&self) -> f32 {
        if self.height_base.is_some() { self.base_depth.clamp(0.0, PARALLAX_SCALE) } else { 0.0 }
    }

    /// The march's scale: [`PARALLAX_SCALE`] when the material holds a height map, else 0.
    pub fn parallax_scale(&self) -> f32 {
        if self.holds_heights() { PARALLAX_SCALE } else { 0.0 }
    }

    /// Whether the material holds a height map the march can read (and a depth to march).
    pub fn holds_heights(&self) -> bool {
        self.marched_base_depth() > 0.0 || self.layer_depths().iter().any(|&d| d > 0.0)
    }
}

/// What terrain.wesl reads of a [`TerrainLayers`]: `counts` x the layer count, y a bit for
/// each layer that has a normal map (bits 0 to 5) and, from bit [`HEIGHT_BITS_SHIFT`], a bit
/// for each layer that has a height map ([`TerrainLayers::height_bits`]), z 1 when it has
/// the burn map, w the feature bits ([`FEATURE_HEIGHT_BLEND`], [`FEATURE_ANTI_TILING`],
/// [`FEATURE_PARALLAX`]); `tile` x [`TerrainLayers::tile`], y
/// [`TerrainLayers::texture_repeat`]; `height` the start-up (depth, gain, source ID: global 0
/// / local 1, reserved) [design, #2550]; `parallax` the march's scale
/// ([`TerrainLayers::parallax_scale`]), its most layers ([`PARALLAX_LAYERS`]) and the
/// distances its fade starts and ends at ([`PARALLAX_FADE`]), `depths_low` and
/// `depths_high` each layer's own depth ([`TerrainLayers::layer_depths`]: layers 0 to 3,
/// then 4 and 5) and the base's in `depths_high.z` [design, #3322, #3846].
#[derive(Clone, Default, ShaderType)]
pub struct TerrainLayersUniform {
    pub counts: UVec4,
    pub tile: Vec4,
    pub height: Vec4,
    pub parallax: Vec4,
    pub depths_low: Vec4,
    pub depths_high: Vec4,
}

/// Where the height-map bits start in `counts.y` ([`TerrainLayersUniform`]).
pub const HEIGHT_BITS_SHIFT: u32 = 8;

/// The most layers of the terrain parallax march's linear search, on a surface seen edge-on
/// (one straight down), before the relief search ([`PARALLAX_RELIEF_STEPS`]) [design, #3322
/// round 2: Bevy's default count. At 8, at the ground pack's depth 0.08 and a view 20 degrees
/// above the ground, a layer's step (about 0.039 texture repeats) was longer than the pack's
/// ridges are wide (8/256), so rays stepped over ridges and the search found the wrong wall;
/// at 16 a step is 0.020 there, and no ridge of that width is stepped over down to about 12 degrees
/// (the test `the_march_meets_the_ground_packs_ridges_where_the_ray_does`). Every step reads
/// the height maps of each layer with a share there, so its frame cost is still to measure].
pub const PARALLAX_LAYERS: f32 = 16.0;

/// The terrain march's grazing clamp: the step's divisor is the view's steepness (the view
/// vector's normal part) held at least at this, as on NIF shapes (vendored
/// parallax_mapping.wesl "NVForever grazing clamp", #2979), so the full-depth shift stays
/// under about 6.7 x the depth and a view along the ground gives finite coordinates
/// [design, #3322 round 2; terrain.wesl's `PARALLAX_GRAZING`, the tests read it].
#[cfg(test)]
pub const PARALLAX_GRAZING: f32 = 0.15;

/// The relief search's halvings after the linear search (Policarpo, Oliveira and Comba,
/// "Real-Time Relief Mapping on Arbitrary Polygonal Surfaces", I3D 2005; Bevy's relief
/// method), as NIF shapes take them (nvf-assets authored.rs `PARALLAX_METHOD`, relief:4)
/// [design, #3322 round 2; terrain.wesl's `PARALLAX_RELIEF_STEPS`].
#[cfg(test)]
pub const PARALLAX_RELIEF_STEPS: u32 = 4;

/// The terrain parallax march's scale, in texture-coordinate units: the deepest a set's
/// height map may be (the scalar range's end), the same for every folded material, so every
/// quadrant marches on one step grid and a layer meets the same steps on both sides of a
/// quadrant edge, whatever else either quadrant holds [design, #3322 review: a grid from
/// each material's deepest layer stepped a shallow layer four times as coarsely beside a
/// deep one]. Shallower layers take fewer of the [`PARALLAX_LAYERS`] steps (the relief
/// search refines between them).
pub const PARALLAX_SCALE: f32 = nvf_assets::texture::TextureCache::AUTHORED_HEIGHT_MAX_DEPTH;

/// The distances from the camera (world units) where the terrain parallax starts to fade and
/// where it is gone [design, #3322: within the camera's own cell and its neighbours' near
/// half, well inside the loaded cells; the texture repeats every 256 units].
pub const PARALLAX_FADE: (f32, f32) = (1024.0, 2048.0);

/// Start-up comparison parameters [design, #2550], independent of the on/off key.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HeightBlendParams {
    depth: f32,
    gain: f32,
    local: bool,
}

impl Default for HeightBlendParams {
    fn default() -> Self {
        Self { depth: 0.10, gain: 8.0, local: false }
    }
}

impl HeightBlendParams {
    fn uniform(self) -> Vec4 {
        Vec4::new(self.depth, self.gain, if self.local { 1.0 } else { 0.0 }, 0.0)
    }
}

/// The smallest transition depth accepted (and the shader's floor) [design, #2550].
const MIN_DEPTH: f32 = 0.001;

/// Pure parser; invalid input falls back as a whole at the environment boundary.
fn parse_height_blend(value: &str) -> Result<HeightBlendParams, &'static str> {
    let fields: Vec<_> = value.split(':').collect();
    if !(2..=3).contains(&fields.len()) {
        return Err("expected depth:gain[:global|local]");
    }
    let depth: f32 = fields[0].parse().map_err(|_| "invalid depth")?;
    let gain: f32 = fields[1].parse().map_err(|_| "invalid gain")?;
    if !depth.is_finite() || depth < MIN_DEPTH {
        return Err("depth must be finite and at least 0.001");
    }
    if !gain.is_finite() || gain < 0.0 {
        return Err("gain must be finite and non-negative");
    }
    let local = match fields.get(2).copied().unwrap_or("global") {
        "global" => false,
        "local" => true,
        _ => return Err("source must be global or local"),
    };
    Ok(HeightBlendParams { depth, gain, local })
}

fn height_blend_params() -> &'static HeightBlendParams {
    static PARAMS: OnceLock<HeightBlendParams> = OnceLock::new();
    PARAMS.get_or_init(|| match std::env::var("NVF_PLAY_TEST_HEIGHT_BLEND") {
        Ok(value) => parse_height_blend(&value).unwrap_or_else(|why| {
            warn!("terrain: invalid NVF_PLAY_TEST_HEIGHT_BLEND ({why}); using 0.10:8:global");
            HeightBlendParams::default()
        }),
        Err(std::env::VarError::NotPresent) => HeightBlendParams::default(),
        Err(std::env::VarError::NotUnicode(_)) => {
            warn!("terrain: non-Unicode NVF_PLAY_TEST_HEIGHT_BLEND; using 0.10:8:global");
            HeightBlendParams::default()
        }
    })
}

fn log_height_blend_params() {
    let p = height_blend_params();
    info!("terrain: height blend parameters algorithm=depth depth={} gain={} source={} height_limit=0.25 (start-up configuration; graphics switch enables)", p.depth, p.gain, if p.local { "local" } else { "global" });
}

/// Bit of `counts.w` set while the material blends by height (the pipeline's
/// `TERRAIN_HEIGHT_BLEND` decides what is drawn; the bit records the material's state).
pub const FEATURE_HEIGHT_BLEND: u32 = 1;

/// Bit of `counts.w` set while the material is anti-tiled (as [`FEATURE_HEIGHT_BLEND`]).
pub const FEATURE_ANTI_TILING: u32 = 2;

/// Bit of `counts.w` set while the material's pipeline marches its height maps (as
/// [`FEATURE_HEIGHT_BLEND`]).
pub const FEATURE_PARALLAX: u32 = 4;

impl From<&TerrainLayers> for TerrainLayersUniform {
    fn from(l: &TerrainLayers) -> Self {
        let normals = [&l.normal0, &l.normal1, &l.normal2, &l.normal3, &l.normal4, &l.normal5];
        let bits = normals.iter().enumerate().filter(|(_, n)| n.is_some()).fold(0u32, |b, (i, _)| b | 1 << i);
        let most = if l.tile > 0.0 { FOLDED_LAYERS } else { FOLDED_LAYERS_WITH_UV };
        let key = TerrainLayersKey::from(l);
        let features = if l.height_blend { FEATURE_HEIGHT_BLEND } else { 0 } | if key.anti_tiling { FEATURE_ANTI_TILING } else { 0 } | if key.parallax { FEATURE_PARALLAX } else { 0 };
        let repeat = if nvf_assets::terrain_sampling::repeat_valid(l.texture_repeat) { l.texture_repeat } else { 0.0 };
        let bits = bits | l.height_bits() << HEIGHT_BITS_SHIFT;
        let parallax = Vec4::new(l.parallax_scale(), PARALLAX_LAYERS, PARALLAX_FADE.0, PARALLAX_FADE.1);
        let d = l.layer_depths();
        TerrainLayersUniform {
            counts: UVec4::new(l.count.min(most as u32), bits, u32::from(l.burn.is_some()), features),
            tile: Vec4::new(l.tile.max(0.0), repeat, 0.0, 0.0),
            height: height_blend_params().uniform(),
            parallax,
            depths_low: Vec4::new(d[0], d[1], d[2], d[3]),
            depths_high: Vec4::new(d[4], d[5], l.marched_base_depth(), 0.0),
        }
    }
}

/// The part of [`TerrainLayers`] that selects the pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TerrainLayersKey {
    pub height_blend: bool,
    /// Anti-tiling asked for and a texture repeat to place it with.
    pub anti_tiling: bool,
    /// [design] The mesh terrain blend's unlit bake copy of a folded base (#2661,
    /// terrain_surface.rs `bake_copy`): `SURFACE_BAKE`, with the two switches above as the
    /// near draw has them, so the colour cache holds what the near draw composes.
    pub surface_bake: bool,
    /// Parallax asked for and a height map to march (module notes, "Parallax"); never on a
    /// bake copy, whose colour stays the unshifted surface's, and never with anti-tiling,
    /// whose offset and turned reads the march does not share [design, #3322 review].
    pub parallax: bool,
}

impl From<&TerrainLayers> for TerrainLayersKey {
    fn from(l: &TerrainLayers) -> Self {
        let anti_tiling = l.anti_tiling && nvf_assets::terrain_sampling::repeat_valid(l.texture_repeat);
        TerrainLayersKey { height_blend: l.height_blend, anti_tiling, surface_bake: l.surface_bake, parallax: l.parallax && !anti_tiling && !l.surface_bake && l.holds_heights() }
    }
}

/// terrain.wesl's switch for height blending, in the main pass only (the prepass and the
/// shadows draw the same depth either way).
fn height_blend_def(descriptor: &mut RenderPipelineDescriptor, key: TerrainLayersKey) {
    use bevy::shader::ShaderDefVal;
    if !key.height_blend || descriptor.vertex.shader_defs.contains(&ShaderDefVal::from("PREPASS_PIPELINE")) {
        return;
    }
    if let Some(fragment) = descriptor.fragment.as_mut() {
        fragment.shader_defs.push("TERRAIN_HEIGHT_BLEND".into());
    }
}

/// terrain.wesl's switch for parallax, in the main pass only: the prepass and the shadows
/// draw the unshifted surface (module notes, "Parallax").
fn parallax_def(descriptor: &mut RenderPipelineDescriptor, key: TerrainLayersKey) {
    use bevy::shader::ShaderDefVal;
    if !key.parallax || key.surface_bake || key.anti_tiling || descriptor.vertex.shader_defs.contains(&ShaderDefVal::from("PREPASS_PIPELINE")) {
        return;
    }
    if let Some(fragment) = descriptor.fragment.as_mut() {
        fragment.shader_defs.push("TERRAIN_PARALLAX".into());
    }
}

impl MaterialExtension for TerrainLayers {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.surface_bake {
            if let Some(fragment) = descriptor.fragment.as_mut() {
                fragment.shader_defs.push("SURFACE_BAKE".into());
            }
        }
        // Land fades in and out with its cell (fade.rs), and is opaque.
        nvf_assets::fade_prepass(descriptor, key.mesh_key);
        nvf_assets::depth_equal(descriptor, key.mesh_key);
        height_blend_def(descriptor, key.bind_group_data);
        parallax_def(descriptor, key.bind_group_data);
        if key.bind_group_data.anti_tiling {
            nvf_assets::terrain_sampling::anti_tiling_defs(descriptor, false);
        }
        if nvf_assets::authored::physical_pbr() && !descriptor.vertex.shader_defs.contains(&bevy::shader::ShaderDefVal::from("PREPASS_PIPELINE")) {
            if let Some(fragment) = descriptor.fragment.as_mut() {
                fragment.shader_defs.push("NVF_PHYSICAL_PBR".into());
            }
        }
        Ok(())
    }
}

/// terrain_weights.wesl, imported by terrain.wesl as `nvforever::terrain_weights`.
const WEIGHTS_SHADER: Handle<bevy::shader::Shader> = bevy::asset::uuid_handle!("8ec9dd26-246b-49a1-91f0-1368b1a56604");

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "terrain.wesl");
        // [design] The bake's import module is loaded whether or not the mesh terrain blend
        // is on (the settings row switches it while the game runs, #2661); only a bake
        // copy's pipeline (`SURFACE_BAKE`) ever imports it.
        load_internal_asset!(app, SURFACE_BAKE_SHADER, "terrain_surface.wesl", |s, _| Shader::from_wesl(s, "embedded://nvforever/terrain_surface.wesl"));
        bevy::asset::load_internal_asset!(app, WEIGHTS_SHADER, "terrain_weights.wesl", |s, _| bevy::shader::Shader::from_wesl(s, "embedded://nvforever/terrain_weights.wesl"));
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default()).add_systems(Startup, log_height_blend_params);
        add_switch_systems(app);
    }
}

/// The terrain switches' systems: each flips the folded materials in place, all before the
/// asset events are sent ([`bevy::asset::AssetEventSystems`]), so a frame's edits reach the
/// mesh terrain blend's capture refresh (after the events) that same frame.
fn add_switch_systems(app: &mut App) {
    app.init_resource::<ParallaxEdits>()
        .init_resource::<AlphaHeightSync>()
        .add_systems(PostUpdate, (sync_height_blend, sync_anti_tiling, (sync_parallax, alpha_heights_system()).chain()).before(bevy::asset::AssetEventSystems))
        .add_systems(Last, |mut edits: ResMut<ParallaxEdits>| edits.0.clear());
}

/// The folded materials [`sync_parallax`] edited this frame, one count per edit. The mesh
/// terrain blend's capture refresh passes over a material's Modified events only when they
/// are no more than these and the capture's inputs compare equal (terrain_surface.rs
/// `refresh_captures`); any other edit, even one that leaves the inputs as they were,
/// remakes the capture. Emptied every frame (`Last`): the edits are sent the frame they are
/// made (the switches run before the asset events) and read that frame [design, #3322
/// review].
#[derive(Resource, Default, Debug)]
pub struct ParallaxEdits(pub HashMap<AssetId<TerrainMaterial>, u32>);

/// Whether the settings ask for height blending (module notes); false without the
/// `Graphics` resource.
fn height_blend_wanted(graphics: Option<&crate::graphics::Graphics>) -> bool {
    graphics.is_some_and(|g| g.terrain_height_blend)
}

/// Height blending switched on or off: every folded material flipped in place (the
/// pipeline follows its key), logged with how many there are. New materials take the
/// setting when they are made ([`TerrainCtx::folded_material`]).
pub fn sync_height_blend(graphics: Option<Res<crate::graphics::Graphics>>, mut materials: ResMut<Assets<TerrainMaterial>>, mut last: Local<Option<bool>>) {
    let want = height_blend_wanted(graphics.as_deref());
    if *last == Some(want) {
        return;
    }
    let stale: Vec<AssetId<TerrainMaterial>> = materials.iter().filter(|(_, m)| m.extension.height_blend != want).map(|(id, _)| id).collect();
    for id in &stale {
        if let Some(mut m) = materials.get_mut(*id) {
            m.extension.height_blend = want;
        }
    }
    if last.is_some() || want {
        info!(
            "terrain: height blending {} ({} folded quadrant materials, {} changed; layers drawn separately keep the ordered blend)",
            if want { "on" } else { "off" },
            materials.len(),
            stale.len()
        );
    }
    *last = Some(want);
}

/// Whether the settings ask for parallax (module notes, "Parallax"); false without the
/// `Graphics` resource.
fn parallax_wanted(graphics: Option<&crate::graphics::Graphics>) -> bool {
    graphics.is_some_and(|g| g.terrain_parallax)
}

/// Parallax switched on or off: the folded materials that hold a height map flipped in
/// place (the pipeline follows their key), logged with how many there are, how many hold a
/// height map and how many changed. A material without one is not touched: its key cannot
/// change (and an edit would make the mesh terrain blend recapture it for nothing); the
/// switch it was made with stays recorded, unread. New materials take the setting when
/// they are made ([`TerrainCtx::folded_material`]).
pub fn sync_parallax(graphics: Option<Res<crate::graphics::Graphics>>, mut materials: ResMut<Assets<TerrainMaterial>>, mut edits: Option<ResMut<ParallaxEdits>>, mut last: Local<Option<bool>>) {
    let want = parallax_wanted(graphics.as_deref());
    if *last == Some(want) {
        return;
    }
    let stale: Vec<AssetId<TerrainMaterial>> = materials.iter().filter(|(_, m)| m.extension.parallax != want && m.extension.holds_heights()).map(|(id, _)| id).collect();
    for id in &stale {
        if let Some(mut m) = materials.get_mut(*id) {
            m.extension.parallax = want;
            if let Some(edits) = edits.as_deref_mut() {
                *edits.0.entry(*id).or_default() += 1;
            }
        }
    }
    if last.is_some() || want {
        let heights = materials.iter().filter(|(_, m)| m.extension.holds_heights()).count();
        info!(
            "terrain: parallax {} ({} folded quadrant materials, {heights} with height maps, {} changed; anti-tiled and separately drawn layers stay unshifted)",
            if want { "on" } else { "off" },
            materials.len(),
            stale.len()
        );
    }
    *last = Some(want);
}

/// A layer's height map and its depth.
type HeightMap = (Handle<Image>, f32);

/// Where [`sync_alpha_heights`] is: the switch it last followed and the folded materials (their
/// base's and layers' diffuse paths) still to be asked about.
#[derive(Resource, Default)]
pub struct AlphaHeightSync {
    last: Option<bool>,
    pending: Vec<(Option<String>, Vec<String>, Handle<TerrainMaterial>)>,
    changed: usize,
    textures: HashSet<String>,
}

/// The main thread's time a frame [`sync_alpha_heights`] may start another layer in (one
/// texture, whole, may go over it) [engineering policy].
const ALPHA_HEIGHTS_FRAME: std::time::Duration = std::time::Duration::from_millis(3);

/// Whether [`sync_alpha_heights`] has anything to do.
fn alpha_heights_due(sync: Res<AlphaHeightSync>, graphics: Option<Res<crate::graphics::Graphics>>) -> bool {
    sync.last != Some(parallax_wanted(graphics.as_deref())) || !sync.pending.is_empty()
}

/// [`sync_alpha_heights`] where a world's resources are (not in the tests of the other
/// switches) and it has something to do.
fn alpha_heights_system() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    sync_alpha_heights
        .run_if(resource_exists::<TerrainCache>)
        .run_if(resource_exists::<TerrainSettings>)
        .run_if(resource_exists::<nvf_assets::GameData>)
        .run_if(resource_exists::<nvf_assets::TextureCache>)
        .run_if(resource_exists::<nvf_assets::ModelCache>)
        .run_if(alpha_heights_due)
}

/// Parallax switched on or off, for the heights read from diffuse alphas (module notes,
/// "Heights from a diffuse's alpha"): on, each folded material's layers without a height map
/// ask for their diffuse's, a few a frame ([`ALPHA_HEIGHTS_FRAME`]: each new one reads its
/// file), so the relief comes in over some frames; off, the materials let go of them at once.
/// A material given its first height is flipped with it (after [`sync_parallax`] has been);
/// the edits are noted in [`ParallaxEdits`] as that one's are (the mesh terrain blend's
/// captures hold no height maps). New materials ask when they are made
/// ([`TerrainCtx::folded_material`]).
pub fn sync_alpha_heights(mut sync: ResMut<AlphaHeightSync>, mut tm: TerrainMaterials, mut assets: ModelCtx, mut edits: Option<ResMut<ParallaxEdits>>) {
    let want = parallax_wanted(tm.graphics.as_deref());
    let sync = &mut *sync;
    if sync.last != Some(want) {
        sync.last = Some(want);
        sync.pending = tm.cache.folded.iter().map(|((base, layers, _), h)| (base.clone(), layers.clone(), h.clone())).collect();
        sync.changed = 0;
        sync.textures.clear();
        if sync.pending.is_empty() {
            return;
        }
    }
    let start = std::time::Instant::now();
    while let Some((base, layers, handle)) = sync.pending.pop() {
        let Some(e) = tm.folded.get(&handle).map(|m| &m.extension) else { continue };
        let (bits, held) = (e.alpha_heights, std::array::from_fn::<bool, { BASE_HEIGHT + 1 }, _>(|k| e.has_height(k)));
        let mut set: Vec<(usize, Option<HeightMap>)> = Vec::new();
        let paths = layers.iter().enumerate().take(FOLDED_LAYERS).chain(base.iter().map(|b| (BASE_HEIGHT, b)));
        for (k, layer) in paths {
            if want && !held[k] {
                let found = TerrainCtx { tm: &mut tm, assets: &mut assets }.alpha_height(layer);
                if let Some(height) = found {
                    sync.textures.insert(layer.clone());
                    set.push((k, Some(height)));
                }
            } else if !want && bits & 1 << k != 0 {
                set.push((k, None));
            }
        }
        if !set.is_empty() {
            if let Some(mut m) = tm.folded.get_mut(&handle) {
                for (k, height) in set {
                    m.extension.set_alpha_height(k, height);
                }
                m.extension.parallax = want;
                sync.changed += 1;
                if let Some(edits) = edits.as_deref_mut() {
                    *edits.0.entry(handle.id()).or_default() += 1;
                }
            }
        }
        if want && start.elapsed() >= ALPHA_HEIGHTS_FRAME {
            break;
        }
    }
    if sync.pending.is_empty() && sync.changed > 0 {
        info!("terrain: heights from diffuse alphas {}: {} folded quadrant materials changed, {} textures", if want { "on" } else { "off" }, sync.changed, sync.textures.len());
    }
}

/// The diffuse paths the ground pack test aid writes height-only sets for (nvf_assets
/// `seed_ground_pack`, #3322): every landscape texture the load order `plugins` names (each
/// LTEX's winning definition, its texture set's TX00, as [`TerrainCtx::texture_of`] reads it
/// through `land_texture`) and the default ground texture (a layer without a texture draws
/// it), so every texture a folded layer can draw has one. The LTEX records are read from a
/// load order of the plugins' own, made for this and dropped again (the aid runs before the
/// game's own is built).
pub fn ground_pack_diffuses(plugins: &[fnv_esm::Plugin], schema_dir: &std::path::Path) -> Result<Vec<String>, String> {
    let schema = EsmSchema::load_dir(schema_dir).map_err(|e| format!("{}: {e}", schema_dir.display()))?;
    let lo = LoadOrder::new(plugins).map_err(|e| e.to_string())?;
    let ltex = lo.index.iter().filter(|(_, defs)| defs.last().is_some_and(|d| &lo.records[d.plugin as usize][d.ordinal as usize].header.sig == b"LTEX")).map(|(&id, _)| id);
    Ok(ground_pack_list(ltex.map(|id| land_texture(&lo, &schema, id).and_then(|t| t.diffuse))))
}

/// [`ground_pack_diffuses`]' list from the LTEX records' diffuses (`None`: no texture set or
/// no TX00): the default ground texture added, sorted, without repeats.
fn ground_pack_list(diffuses: impl Iterator<Item = Option<String>>) -> Vec<String> {
    let mut list: Vec<String> = diffuses.flatten().chain([DEFAULT_DIFFUSE.to_string()]).collect();
    list.sort();
    list.dedup();
    list
}

/// What the folded draw's march moves at a point of the landscape, for the terrain parallax
/// readback aid (`crate::terrain_parallax_ab`, #3322): the quadrant's layers as the cell
/// draws them (its own, stitched to its neighbours', [`stitch_layers`]), the first
/// [`folded_layers`] folded into the quadrant's draw and those past them drawn on their own
/// over it (unshifted, module notes "Parallax"), and the folded material the cell made for
/// them ([`TerrainCache`]), with the height maps it holds. A CPU estimate from the painted
/// opacities [design].
#[derive(Debug, Clone, PartialEq)]
pub struct GroundCover {
    pub grid: (i32, i32),
    /// 0 south-west, 1 south-east, 2 north-west, 3 north-east.
    pub quadrant: u8,
    /// Layers folded into the quadrant's draw, those of them with a height map, and the
    /// layers drawn on their own over the folded draw.
    pub folded: usize,
    pub with_heights: usize,
    pub over: usize,
    /// Whether the quadrant has a folded material (none: drawn by the separate materials,
    /// which the march never reaches).
    pub material: bool,
    /// Whether the base has a height map (#3846).
    pub base_height: bool,
    /// The share of the folded blend at the point that has a height map, the layers' and the
    /// base's (the ordered shares, terrain_weights.wesl `terrain_ordered_shares`).
    pub height_share: f32,
    /// The height-mapped layers' part of it.
    pub layer_share: f32,
    /// How much of the folded draw the layers drawn over it hide at the point.
    pub over_cover: f32,
}

impl GroundCover {
    /// How much of what shows at the point the march moves: the height-mapped share, less
    /// what the separately drawn layers hide; 0 without a folded material.
    pub fn shifted(&self) -> f32 {
        if self.material { self.height_share * (1.0 - self.over_cover) } else { 0.0 }
    }

    /// [`Self::shifted`] of the layers' part alone.
    pub fn layers_shifted(&self) -> f32 {
        if self.material { self.layer_share * (1.0 - self.over_cover) } else { 0.0 }
    }

    pub fn describe(&self) -> String {
        format!(
            "cell ({}, {}) quadrant {}: {} folded layers ({} with height maps{}{}), {} drawn over them; shifted share {:.2} (height-mapped {:.2}, hidden by the layers over them {:.2})",
            self.grid.0,
            self.grid.1,
            self.quadrant,
            self.folded,
            self.with_heights,
            if self.base_height { ", the base too" } else { "" },
            if self.material { "" } else { "; no folded material" },
            self.over,
            self.shifted(),
            self.height_share,
            self.over_cover
        )
    }
}

/// One quadrant's layers as [`GroundCover`] reads them.
#[derive(Debug, Clone, Default)]
struct QuadCover {
    /// The opacity grids of the quadrant's drawn layers in drawing order ([`alpha_grid`]).
    grids: Vec<Vec<f32>>,
    /// How many of them are folded.
    folded: usize,
    /// The folded material's height-map bits, when there is one.
    bits: Option<u32>,
}

/// A cell's [`GroundCover`]s, prepared once (the layers loaded and stitched, the materials
/// looked up), for asking many points.
#[derive(Debug, Clone)]
pub struct CellCover {
    grid: (i32, i32),
    quads: [QuadCover; 4],
}

impl CellCover {
    /// `None` without landscape at grid square `grid` (or outside the worldspace).
    pub fn new(world: &WorldData, cache: &TerrainCache, materials: &Assets<TerrainMaterial>, tile: f32, grid: (i32, i32)) -> Option<CellCover> {
        let lands = world.land_ids(grid);
        let parts = LandParts { heights: false, normals: false, colors: false, layers: true };
        let land = load_land(world.lo, world.schema, lands[1][1]?, parts)?;
        let around = NEIGHBOURS.map(|(dx, dy)| lands[(dy + 1) as usize][(dx + 1) as usize].and_then(|id| load_land(world.lo, world.schema, id, parts)).map(|l| l.layers));
        let drawn = stitch_layers(&land.layers, &around);
        let most = folded_layers(tile);
        let shader_tile = if most == FOLDED_LAYERS { tile } else { 0.0 };
        // The texture a layer draws, as `TerrainCtx::texture_of` resolved it (`None`: not
        // resolved, or an LTEX without a diffuse, which keeps a quadrant from folding).
        let name = |ltex: Option<u32>| -> Option<String> {
            match ltex {
                None => Some(DEFAULT_DIFFUSE.to_string()),
                Some(id) => cache.textures.get(&id).cloned().flatten(),
            }
        };
        let quads = std::array::from_fn(|q| {
            let q = q as u8;
            let layers: Vec<&Layer> = quadrant_layers(&drawn, q).into_iter().map(|(_, l)| l).collect();
            let folded = layers.len().min(most);
            let base = drawn.iter().rev().find(|l| l.base && l.quadrant == q).and_then(|b| b.texture);
            let first: Option<Vec<String>> = layers.iter().take(most).map(|l| name(l.texture)).collect();
            let bits = first
                .and_then(|f| { let tiled = shader_tile > 0.0 && !f.is_empty(); cache.folded.get(&(name(base), f, tiled)) })
                .and_then(|h| materials.get(h))
                .map(|m| m.extension.height_bits());
            // The folded layers, then those drawn on their own over them: only the ones the
            // cell made a material for (`quadrant_draws` leaves out a layer whose texture
            // set has no diffuse or whose diffuse does not load), by their rank.
            let over = layers.iter().enumerate().skip(folded).filter(|(rank, l)| name(l.texture).is_some_and(|t| cache.layer.contains_key(&(t, *rank)))).map(|(_, l)| *l);
            QuadCover { grids: layers.iter().take(folded).copied().chain(over).map(alpha_grid).collect(), folded, bits }
        });
        Some(CellCover { grid, quads })
    }

    /// At cell-local `(x, y)` (game units from the cell's south-west corner).
    pub fn at(&self, x: f32, y: f32) -> GroundCover {
        let half = fnv_world::CELL_SIZE / 2.0;
        let (qx, qy) = (usize::from(x >= half), usize::from(y >= half));
        let q = qy * 2 + qx;
        let spacing = fnv_world::land::SPACING;
        let (u, v) = ((x - qx as f32 * half) / spacing, (y - qy as f32 * half) / spacing);
        let quad = &self.quads[q];
        let (layer_share, base_share, over_cover) = cover_shares(&quad.grids, quad.folded, quad.bits.unwrap_or(0), u, v);
        GroundCover {
            grid: self.grid,
            quadrant: q as u8,
            folded: quad.folded,
            with_heights: quad.bits.map_or(0, |b| (b & LAYER_BITS).count_ones() as usize),
            base_height: quad.bits.is_some_and(|b| b >> BASE_HEIGHT != 0),
            over: quad.grids.len() - quad.folded,
            material: quad.bits.is_some(),
            height_share: layer_share + base_share,
            layer_share,
            over_cover,
        }
    }
}

/// [`GroundCover`] at a point of the worldspace (game units); `None` without landscape there.
pub fn ground_cover(world: &WorldData, cache: &TerrainCache, materials: &Assets<TerrainMaterial>, tile: f32, x: f32, y: f32) -> Option<GroundCover> {
    let size = fnv_world::CELL_SIZE;
    let grid = ((x / size).floor() as i32, (y / size).floor() as i32);
    Some(CellCover::new(world, cache, materials, tile, grid)?.at(x - grid.0 as f32 * size, y - grid.1 as f32 * size))
}

/// An opacity grid ([`alpha_grid`]) at quadrant grid coordinates `(u, v)` (vertex steps from
/// the quadrant's south-west corner), on the stored triangles the meshes interpolate it on
/// (each grid square split along its south-west to north-east diagonal, [`stored_weights`]'s
/// rule, which [`refine_quad`] gives the drawn vertices; a point on the last row or column on
/// the square before it).
fn opacity_at(g: &[f32], u: f32, v: f32) -> f32 {
    let max = (QUAD_SIDE - 1) as f32;
    let (u, v) = (u.clamp(0.0, max), v.clamp(0.0, max));
    let (c, r) = ((u.floor() as usize).min(QUAD_SIDE - 2), (v.floor() as usize).min(QUAD_SIDE - 2));
    let (fx, fy) = (u - c as f32, v - r as f32);
    let a = |r: usize, c: usize| g[r * QUAD_SIDE + c];
    let (sw, se, nw, ne) = (a(r, c), a(r, c + 1), a(r + 1, c), a(r + 1, c + 1));
    if fx >= fy {
        sw * (1.0 - fx) + se * (fx - fy) + ne * fy
    } else {
        sw * (1.0 - fy) + nw * (fy - fx) + ne * fx
    }
}

/// For a quadrant's layers' opacity `grids` in drawing order, the first `folded` folded with
/// height-map bits `bits`: the height-mapped layers' ordered share of the folded blend at
/// `(u, v)`, the base's when it has a height map (else 0), and how much the layers past the
/// folded ones hide of it there.
fn cover_shares(grids: &[Vec<f32>], folded: usize, bits: u32, u: f32, v: f32) -> (f32, f32, f32) {
    let mut remaining = 1.0f32;
    let mut height_share = 0.0f32;
    for k in (0..folded.min(grids.len())).rev() {
        let paint = opacity_at(&grids[k], u, v);
        if bits & (1 << k) != 0 {
            height_share += paint * remaining;
        }
        remaining *= 1.0 - paint;
    }
    let through: f32 = grids.iter().skip(folded).map(|g| 1.0 - opacity_at(g, u, v)).product();
    (height_share, if bits >> BASE_HEIGHT & 1 != 0 { remaining } else { 0.0 }, 1.0 - through)
}

/// Whether the settings ask for anti-tiling (module notes); false without the `Graphics`
/// resource.
fn anti_tiling_wanted(graphics: Option<&crate::graphics::Graphics>) -> bool {
    graphics.is_some_and(|g| g.terrain_anti_tiling)
}

/// Anti-tiling switched on or off: every folded material and every landscape NIF material
/// (a texture repeat set, [`terrain_nif_material`]) flipped in place (the pipelines follow
/// their keys), logged with how many of each there are and how many changed. New materials
/// take the setting when they are made ([`TerrainCtx::folded_material`],
/// [`TerrainCtx::base_material`], [`TerrainCtx::layer_material`]). A texture repeat that
/// cannot place the reads (not above 0) keeps the ordinary reads, said when it is switched
/// on.
pub fn sync_anti_tiling(
    graphics: Option<Res<crate::graphics::Graphics>>,
    settings: Option<Res<TerrainSettings>>,
    mut folded: ResMut<Assets<TerrainMaterial>>,
    mut separate: Option<ResMut<Assets<NifMaterial>>>,
    mut last: Local<Option<bool>>,
) {
    let want = anti_tiling_wanted(graphics.as_deref());
    if *last == Some(want) {
        return;
    }
    let stale: Vec<AssetId<TerrainMaterial>> = folded.iter().filter(|(_, m)| m.extension.anti_tiling != want).map(|(id, _)| id).collect();
    for id in &stale {
        if let Some(mut m) = folded.get_mut(*id) {
            m.extension.anti_tiling = want;
        }
    }
    let (mut landscape, mut stale_separate) = (0usize, 0usize);
    if let Some(separate) = separate.as_deref_mut() {
        let ids: Vec<(AssetId<NifMaterial>, bool)> = separate.iter().filter(|(_, m)| m.extension.terrain_repeat != 0.0).map(|(id, m)| (id, m.extension.anti_tiling != want)).collect();
        landscape = ids.len();
        for (id, _) in ids.iter().filter(|(_, stale)| *stale) {
            if let Some(mut m) = separate.get_mut(*id) {
                m.extension.anti_tiling = want;
                stale_separate += 1;
            }
        }
    }
    if want {
        let repeat = settings.as_deref().map_or(0.0, |s| s.tile);
        if !nvf_assets::terrain_sampling::repeat_valid(repeat) {
            warn!("terrain: anti-tiling needs a texture repeat above 0 (it is {repeat}); the textures keep their ordinary reads");
        }
    }
    if last.is_some() || want {
        info!("terrain: anti-tiling {} ({} folded quadrant materials, {} changed) and {landscape} separate landscape materials ({stale_separate} changed)", if want { "on" } else { "off" }, folded.len(), stale.len());
    }
    *last = Some(want);
}

/// Terrain settings and caches; GPU assets come from the shared [`ModelCtx`].
#[derive(SystemParam)]
pub struct TerrainMaterials<'w> {
    pub settings: Res<'w, TerrainSettings>,
    pub cache: ResMut<'w, TerrainCache>,
    pub folded: ResMut<'w, Assets<TerrainMaterial>>,
    device: Option<Res<'w, bevy::render::renderer::RenderDevice>>,
    burn: Option<Res<'w, crate::grass::GrassBurn>>,
    graphics: Option<Res<'w, crate::graphics::Graphics>>,
}

impl TerrainMaterials<'_> {
    /// Whether quadrants fold their first layers into the base (module notes): the GPU
    /// keeps materials' textures in bindless tables. Test aid `NVF_PLAY_TEST_TERRAIN_LAYERS=blended`:
    /// every layer keeps its blended draw, for comparing the two.
    pub fn folds(&self) -> bool {
        static BLENDED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *BLENDED.get_or_init(|| std::env::var("NVF_PLAY_TEST_TERRAIN_LAYERS").is_ok_and(|v| v == "blended")) {
            return false;
        }
        static ONCE: std::sync::Once = std::sync::Once::new();
        let folds = self.device.as_ref().is_some_and(|d| bevy::render::material_bind_groups::material_uses_bindless_resources::<TerrainMaterial>(d));
        ONCE.call_once(|| info!("terrain: the first {} texture layers of a quadrant are {} (render device {})", folded_layers(self.settings.tile), if folds { "folded into its base's draw" } else { "blended over it, as every layer" }, if self.device.is_some() { "known" } else { "not known" }));
        folds
    }
}

/// Terrain state plus the shared asset context, for one spawn.
pub struct TerrainCtx<'a, 'w1, 'w2> {
    pub tm: &'a mut TerrainMaterials<'w1>,
    pub assets: &'a mut ModelCtx<'w2>,
}

impl TerrainCtx<'_, '_, '_> {
    /// Diffuse path of an LTEX; `None` (no BTXT, or a null layer texture) is the default.
    fn texture_of(&mut self, world: &WorldData, ltex: Option<u32>) -> Option<String> {
        let cache = &mut *self.tm.cache;
        let Some(id) = ltex else {
            cache.normals.entry(DEFAULT_DIFFUSE.to_string()).or_insert_with(|| DEFAULT_NORMAL.to_string());
            return Some(DEFAULT_DIFFUSE.to_string());
        };
        if let Some(diffuse) = cache.textures.get(&id) {
            return diffuse.clone();
        }
        let tex = land_texture(world.lo, world.schema, id);
        let diffuse = tex.as_ref().and_then(|t| t.diffuse.clone());
        if let (Some(d), Some(n)) = (&diffuse, tex.and_then(|t| t.normal)) {
            cache.normals.entry(d.clone()).or_insert(n);
        }
        cache.textures.insert(id, diffuse.clone());
        diffuse
    }

    fn image(&mut self, path: &str, srgb: bool) -> Option<Handle<Image>> {
        let a = &mut *self.assets;
        a.textures.get(path, srgb, &a.data, &mut a.images, a.formats.as_deref())
    }

    fn normal(&mut self, path: &str) -> Option<Handle<Image>> {
        let a = &mut *self.assets;
        a.textures.normal(path, &a.data, &mut a.images, a.formats.as_deref()).map(|(image, _)| image)
    }

    /// The authored height map of a landscape texture and its set's depth (module notes,
    /// "Parallax"): `None` without one, or while the run's Authored materials state is off.
    fn height(&mut self, diffuse: &str) -> Option<(Handle<Image>, f32)> {
        let a = &mut *self.assets;
        a.textures.authored_height(diffuse, &a.data, &mut a.images, a.formats.as_deref())
    }

    /// The height in a landscape texture's alpha (module notes, "Heights from a diffuse's
    /// alpha"), while the switch is on: `None` otherwise, where the authored index knows a set
    /// for the texture, and where its alpha is none.
    fn alpha_height(&mut self, diffuse: &str) -> Option<(Handle<Image>, f32)> {
        if !parallax_wanted(self.tm.graphics.as_deref()) {
            return None;
        }
        let a = &mut *self.assets;
        a.textures.alpha_height(diffuse, &a.data, &mut a.images, a.formats.as_deref())
    }

    /// The lit surface of a landscape texture: diffuse and normal map. `None` for the
    /// diffuse path (an LTEX without a texture set) gives a plain grey.
    fn surface(&mut self, tex: Option<&str>) -> StandardMaterial {
        let image = tex.and_then(|t| self.image(t, true));
        let normal = tex.and_then(|t| self.tm.cache.normals.get(t).cloned()).and_then(|n| self.normal(&n));
        surface_material(image, normal)
    }

    /// A landscape NIF material: `base` with the texture repeat and the anti-tiling switch
    /// (module notes, "Anti-tiling").
    fn terrain_nif(&self, base: StandardMaterial) -> NifMaterial {
        terrain_nif_material(base, self.tm.settings.tile, anti_tiling_wanted(self.tm.graphics.as_deref()))
    }

    fn base_material(&mut self, tex: Option<String>) -> Handle<NifMaterial> {
        if let Some(h) = self.tm.cache.base.get(&tex) {
            return h.clone();
        }
        let m = self.surface(tex.as_deref());
        let m = self.terrain_nif(m);
        let h = self.assets.materials.add(m);
        self.tm.cache.base.insert(tex, h.clone());
        h
    }

    /// Whether a quadrant of the base `tex` alone, without painted layers, is drawn by the
    /// folded material with no layers, so it can march over the base's height (#3846): the base
    /// has an authored height map, or no set and a diffuse of the layout whose alpha may be one
    /// (BC3, whatever the switch and the alpha, so the draws do not change with them). The
    /// game's own ground (DXT1, no sets) never: it keeps the separate landscape material. Told
    /// once a texture and run.
    fn base_may_march(&mut self, tex: Option<&str>) -> bool {
        let Some(tex) = tex.filter(|_| nvf_assets::authored::enabled()) else { return false };
        if let Some(&known) = self.tm.cache.base_marches.get(tex) {
            return known;
        }
        let a = &mut *self.assets;
        let key = nvf_assets::texture::resolved_path(tex, |p| a.data.vfs.exists(p));
        let may = if a.data.pbr_index().lookup(&key).is_some() {
            self.height(tex).is_some()
        } else {
            // The alpha before the picture, as `folded_material` asks.
            use bevy::render::render_resource::TextureFormat::{Bc3RgbaUnorm, Bc3RgbaUnormSrgb};
            self.alpha_height(tex).is_some() || self.image(tex, true).and_then(|h| self.assets.images.get(&h)).is_some_and(|i| matches!(i.texture_descriptor.format, Bc3RgbaUnorm | Bc3RgbaUnormSrgb))
        };
        self.tm.cache.base_marches.insert(tex.to_string(), may);
        may
    }

    /// The material of a quadrant with `layers` (diffuse paths, in order; none: the base alone,
    /// [`Self::base_may_march`]) folded into the
    /// base `tex`. `None` when a layer's diffuse texture is not found: the quadrant then
    /// keeps its blended layers, which leave such a layer out.
    fn folded_material(&mut self, tex: Option<String>, layers: &[String], tile: f32) -> Option<Handle<TerrainMaterial>> {
        let key = (tex.clone(), layers.to_vec(), tile > 0.0);
        if let Some(h) = self.tm.cache.folded.get(&key) {
            return Some(h.clone());
        }
        // Asked for before the pictures: a file read ahead is still there to be looked at.
        let alphas: Vec<Option<HeightMap>> = layers.iter().take(FOLDED_LAYERS).map(|l| self.alpha_height(l)).collect();
        let base_alpha = tex.as_deref().and_then(|t| self.alpha_height(t));
        let mut textures = Vec::with_capacity(layers.len());
        for l in layers {
            let diffuse = self.image(l, true)?;
            let normal = self.tm.cache.normals.get(l).cloned().and_then(|n| self.normal(&n));
            textures.push((diffuse, normal));
        }
        let mut textures = textures.into_iter();
        let mut next = || textures.next().map_or((None, None), |(d, n)| (Some(d), n));
        let ((diffuse0, normal0), (diffuse1, normal1), (diffuse2, normal2), (diffuse3, normal3), (diffuse4, normal4), (diffuse5, normal5)) = (next(), next(), next(), next(), next(), next());
        // The layers' authored height maps in their order (module notes, "Parallax"); none
        // while the run's Authored materials state is off. The base's likewise (#3846). One
        // without an authored set may have a height in its diffuse's alpha ("Heights
        // from a diffuse's alpha").
        let mut alpha_heights = 0u32;
        let layer_heights: Vec<Option<(Handle<Image>, f32)>> = layers
            .iter()
            .enumerate()
            .map(|(k, l)| {
                // Never both: an alpha is none where the index knows a set.
                self.height(l).or_else(|| {
                    let height = alphas.get(k).cloned().flatten();
                    alpha_heights |= u32::from(height.is_some()) << k;
                    height
                })
            })
            .collect();
        let (heights, parallax_depths) = layer_height_slots(layer_heights);
        let base_height = tex.as_deref().and_then(|t| self.height(t)).or_else(|| {
            alpha_heights |= u32::from(base_alpha.is_some()) << BASE_HEIGHT;
            base_alpha
        });
        let (height_base, base_depth) = base_height.map_or((None, 0.0), |(h, d)| (Some(h), d));
        let [height0, height1, height2, height3, height4, height5] = heights;
        let burn = self.tm.burn.as_deref().map(|b| b.0.clone());
        let height_blend = height_blend_wanted(self.tm.graphics.as_deref());
        let anti_tiling = anti_tiling_wanted(self.tm.graphics.as_deref());
        let texture_repeat = self.tm.settings.tile;
        let parallax = parallax_wanted(self.tm.graphics.as_deref());
        let extension = TerrainLayers {
            count: layers.len() as u32, tile, height_blend, anti_tiling, texture_repeat, parallax, parallax_depths, base_depth, alpha_heights,
            diffuse0, normal0, diffuse1, normal1, diffuse2, normal2, diffuse3, normal3, diffuse4, normal4, diffuse5, normal5, burn,
            height0, height1, height2, height3, height4, height5, height_base, ..default()
        };
        if alpha_heights != 0 {
            let cache = &mut *self.tm.cache;
            cache.alpha_height_materials += 1;
            cache.alpha_height_textures.extend(layers.iter().enumerate().take(FOLDED_LAYERS).chain(tex.iter().map(|t| (BASE_HEIGHT, t))).filter(|(k, _)| alpha_heights & 1 << k != 0).map(|(_, l)| l.clone()));
            let n = cache.alpha_height_materials;
            if n == 1 {
                info!("terrain: first folded material with heights from diffuse alphas: {} of {} layers{} (depth {})", (alpha_heights & LAYER_BITS).count_ones(), layers.len(), if alpha_heights >> BASE_HEIGHT != 0 { " and the base" } else { "" }, nvf_assets::ALPHA_HEIGHT_DEPTH);
            } else if n.is_power_of_two() {
                info!("terrain: {n} folded materials made with heights from {} textures' diffuse alphas", cache.alpha_height_textures.len());
            }
        }
        if extension.height_bits() & !extension.alpha_heights & LAYER_BITS != 0 {
            self.tm.cache.parallax_materials += 1;
            if self.tm.cache.parallax_materials == 1 {
                info!("terrain: first folded material with authored height maps: {} of {} layers (depths {:?}, marched on scale {PARALLAX_SCALE})", (extension.height_bits() & !extension.alpha_heights & LAYER_BITS).count_ones(), layers.len(), extension.layer_depths());
            }
        }
        let base = self.surface(tex.as_deref());
        let h = self.tm.folded.add(TerrainMaterial { base, extension });
        self.tm.cache.folded.insert(key, h.clone());
        Some(h)
    }

    fn layer_material(&mut self, tex: String, rank: usize) -> Option<Handle<NifMaterial>> {
        if let Some(h) = self.tm.cache.layer.get(&(tex.clone(), rank)) {
            return Some(h.clone());
        }
        let surface = self.surface(Some(&tex));
        surface.base_color_texture.as_ref()?;
        let m = StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            // Draw over the base and earlier layers (also orders the blending), and before
            // every other blended mesh: one without depth (the corn rows of
            // `scol\scolmaizerow*.nif`) was painted over by the layers of the ground
            // behind it and looked see-through [observed].
            depth_bias: sort_first(2.0 * (rank as f32 + 1.0)),
            ..surface
        };
        let m = self.terrain_nif(m);
        let h = self.assets.materials.add(m);
        self.tm.cache.layer.insert((tex, rank), h.clone());
        Some(h)
    }
}

/// The height-map slots of a folded material (each layer's in the layers' order, up to
/// [`FOLDED_LAYERS`]; `None` for a layer without one) and each layer's depth, its set's (0
/// without a height map) [design, #3322].
fn layer_height_slots(layers: Vec<Option<(Handle<Image>, f32)>>) -> ([Option<Handle<Image>>; FOLDED_LAYERS], [f32; FOLDED_LAYERS]) {
    let mut slots: [Option<Handle<Image>>; FOLDED_LAYERS] = Default::default();
    let mut depths = [0.0f32; FOLDED_LAYERS];
    for ((slot, depth), height) in slots.iter_mut().zip(&mut depths).zip(layers) {
        if let Some((image, d)) = height {
            *slot = Some(image);
            *depth = d;
        }
    }
    (slots, depths)
}

/// The landscape's StandardMaterial for a diffuse texture and normal map ([`TerrainCtx::surface`]).
/// Anti-tiling's surface set-up (nvf_assets terrain_pbr.wesl) reads exactly what this holds:
/// the colour, the diffuse texture, the normal map and scalar values (the contract test).
fn surface_material(image: Option<Handle<Image>>, normal: Option<Handle<Image>>) -> StandardMaterial {
    StandardMaterial {
        base_color: if image.is_some() { Color::WHITE } else { Color::srgb(0.45, 0.42, 0.38) },
        base_color_texture: image,
        normal_map_texture: normal,
        // Green points down the image, as on NIF shapes (docs/spec-notes/nif.md §6
        // "Normal maps").
        flip_normal_map_y: true,
        // Also the targets of the mesh terrain blend's full shading (#2661).
        perceptual_roughness: nvf_assets::nif_surface_blend::LAND_PERCEPTUAL_ROUGHNESS,
        reflectance: nvf_assets::nif_surface_blend::LAND_REFLECTANCE,
        ..default()
    }
}

/// A landscape NIF material (a quadrant's base or a layer drawn on its own): `base` with
/// the texture repeat `repeat` (which marks it as the landscape's for [`sync_anti_tiling`])
/// and the anti-tiling switch.
fn terrain_nif_material(base: StandardMaterial, repeat: f32, anti_tiling: bool) -> NifMaterial {
    let mut m = nif_material(base);
    m.extension.terrain_repeat = repeat;
    m.extension.anti_tiling = anti_tiling;
    m
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Sign (w) of the terrain's vertex tangents: the one Bevy's tangent generator gives a mesh
/// whose texture coordinates grow with x and y (checked in the tests), so the normal maps
/// are read in the same frame as on NIF shapes. With `flip_normal_map_y`, red tilts the
/// normal towards +u (east) and green towards +v (north).
const TANGENT_SIGN: f32 = -1.0;

/// Vertices along a quadrant's side on a surface of `sub` squares per grid square.
fn quad_side(sub: usize) -> usize {
    (QUAD_SIDE - 1) * sub + 1
}

/// The vertices of a grid with `side` vertices a side (the LAND's 33 or a quadrant's 17)
/// and their weights at its point (`fr`, `fc`) refined `sub` times: the stored triangle
/// (south-west → north-east split) the point lies in. A grid vertex is itself, weight 1.
fn stored_weights(fr: usize, fc: usize, sub: usize, side: usize) -> [(usize, f32); 3] {
    let (r, c) = (fr / sub, fc / sub);
    if fr.is_multiple_of(sub) && fc.is_multiple_of(sub) {
        let i = r * side + c;
        return [(i, 1.0), (i, 0.0), (i, 0.0)];
    }
    let (fy, fx) = ((fr % sub) as f32 / sub as f32, (fc % sub) as f32 / sub as f32);
    // A point on the last row or column lies on the square before it.
    let (r, fy) = if r == side - 1 { (r - 1, 1.0) } else { (r, fy) };
    let (c, fx) = if c == side - 1 { (c - 1, 1.0) } else { (c, fx) };
    let (sw, se, nw, ne) = (r * side + c, r * side + c + 1, (r + 1) * side + c, (r + 1) * side + c + 1);
    if fx >= fy {
        [(sw, 1.0 - fx), (se, fx - fy), (ne, fy)]
    } else {
        [(sw, 1.0 - fy), (nw, fy - fx), (ne, fx)]
    }
}

/// A quadrant's 17 × 17 grid (a layer's opacities) on the quadrant's vertices of a surface
/// of `sub` squares per grid square, interpolated on the stored triangles.
fn refine_quad(grid: &[f32], sub: usize) -> Vec<f32> {
    if sub == 1 {
        return grid.to_vec();
    }
    let qs = quad_side(sub);
    (0..qs * qs).map(|v| stored_weights(v / qs, v % qs, sub, QUAD_SIDE).iter().map(|&(k, w)| w * grid[k]).sum()).collect()
}

/// Mesh of one quadrant in cell-local game space, on `surface` (17 × 17 vertices as
/// stored, 65 × 65 with the default 4 × 4 smoothing), drawn in full; `alpha`: a layer's
/// opacities on the same vertices ([`refine_quad`]).
#[cfg(test)]
fn quadrant_mesh(land: &Land, surface: &Surface, q: u8, alpha: Option<&[f32]>, cell_origin: Vec2, tile: f32) -> Option<Mesh> {
    quadrant_mesh_drawn(land, surface, q, QuadrantDraw::FULL, alpha, cell_origin, tile)
}

/// Squares per LAND grid square and side a quadrant of `surface` is drawn with as `draw`
/// says (#2676): the surface's own, or half of them drawn with step 2.
fn drawn_sub(surface: &Surface, draw: QuadrantDraw) -> usize {
    surface.sub() / surface.draw_step(draw)
}

/// [`quadrant_mesh`] drawn as `draw` says (`fnv_world::land_surface::QuadrantDraw`, #2676):
/// every surface vertex or every other one, with its coarse sides on the straight
/// segments of a coarser neighbour. `alpha` is on the drawn vertices ([`refine_quad`] by
/// [`drawn_sub`]). The colours and opacities are the stored triangles' at each drawn
/// vertex either way (a coarse side's moved vertices keep theirs: the stored triangles are
/// linear along a quadrant's side, so they are what the neighbour interpolates there).
fn quadrant_mesh_drawn(land: &Land, surface: &Surface, q: u8, draw: QuadrantDraw, alpha: Option<&[f32]>, cell_origin: Vec2, tile: f32) -> Option<Mesh> {
    let (sub, side) = (surface.sub(), surface.side());
    let grid = surface.drawn_quadrant(q, draw);
    let qs = grid.side;
    let n = qs * qs;
    let (mut pos, mut nor, mut uv, mut col) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    let mut tan = Vec::with_capacity(n);
    // Smoothed: the vertex colours in linear light at the LAND vertices, interpolated below.
    let linear: Option<Vec<[f32; 3]>> = if sub > 1 { land.colors.as_ref().map(|v| v.iter().map(|c| c.map(srgb_to_linear)).collect()) } else { None };
    for r in 0..qs {
        for c in 0..qs {
            let (gr, gc) = grid.surface_vertex(r, c);
            let i = gr * side + gc;
            let v = r * qs + c;
            let p = grid.positions[v];
            let (x, y) = (p[0], p[1]);
            pos.push(p);
            let normal = match &grid.normals {
                Some(nm) => Vec3::from_array(nm[v]).normalize_or(Vec3::Z),
                // As stored the surface is the LAND grid: vertex i is LAND vertex i.
                None => land
                    .normals
                    .as_ref()
                    .map(|v| Vec3::new(v[i][0] as f32, v[i][1] as f32, v[i][2] as f32).normalize_or(Vec3::Z))
                    .unwrap_or(Vec3::Z),
            };
            nor.push(normal.to_array());
            // East (the direction of +u) laid into the surface.
            let east = (Vec3::X - normal * normal.x).normalize_or(Vec3::X);
            tan.push([east.x, east.y, east.z, TANGENT_SIGN]);
            let (wx, wy) = (cell_origin.x + x, cell_origin.y + y);
            uv.push([wx / tile, wy / tile]);
            let rgb = match &linear {
                Some(lin) => stored_weights(gr, gc, sub, SIDE).iter().fold([0.0; 3], |acc, &(k, w)| [acc[0] + w * lin[k][0], acc[1] + w * lin[k][1], acc[2] + w * lin[k][2]]),
                None => land.colors.as_ref().map_or([1.0; 3], |v| v[i].map(srgb_to_linear)),
            };
            let a = alpha.map_or(1.0, |a| a[v]);
            col.push([rgb[0], rgb[1], rgb[2], a]);
        }
    }
    let mut idx = Vec::with_capacity((qs - 1) * (qs - 1) * 6);
    // A layer keeps only the triangles it paints: one whose three corners have opacity 0
    // blends to exactly what lies under it (its alpha is 0 across the triangle), so it
    // is left out [engineering choice: the same picture; most layers paint patches, and
    // drawing them over whole quadrants cost about a millisecond of blended terrain a
    // frame outdoors at 1440p, docs/design/frame-rate-audit.md, terrain texture layers,
    // #1886].
    let paints = |t: [u32; 3]| alpha.is_none_or(|a| t.iter().any(|&v| a[v as usize] > 0.0));
    for r in 0..qs - 1 {
        for c in 0..qs - 1 {
            let i = (r * qs + c) as u32;
            let (e, nn, ne) = (i + 1, i + qs as u32, i + qs as u32 + 1);
            // Counter-clockwise seen from above (+z), as the collision's triangles
            // (`Surface::triangles`).
            for t in [[i, e, ne], [i, ne, nn]] {
                if paints(t) {
                    idx.extend_from_slice(&t);
                }
            }
        }
    }
    if idx.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tan);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
    Some(mesh)
}

/// The neighbouring cells' grid squares, in the order [`stitch_layers`] takes their
/// layers: north, east, south, west.
pub const NEIGHBOURS: [(i32, i32); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];
const NORTH: usize = 0;
const EAST: usize = 1;
const SOUTH: usize = 2;
const WEST: usize = 3;

/// The positions along one side of a quadrant's 17 × 17 grid, west → east or south →
/// north (rows run south → north, world.md §3).
fn edge_position(side: usize, k: usize) -> usize {
    let last = QUAD_SIDE - 1;
    match side {
        NORTH => last * QUAD_SIDE + k,
        SOUTH => k,
        EAST => k * QUAD_SIDE + last,
        _ => k * QUAD_SIDE,
    }
}

/// The quadrant across `side` of quadrant `q` (0 south-west, 1 south-east, 2 north-west,
/// 3 north-east): in the same cell (`None`) or in the neighbouring cell on that side.
fn across(q: u8, side: usize) -> (Option<usize>, u8) {
    match side {
        EAST if q.is_multiple_of(2) => (None, q + 1),
        EAST => (Some(EAST), q - 1),
        WEST if q % 2 == 1 => (None, q - 1),
        WEST => (Some(WEST), q + 1),
        NORTH if q < 2 => (None, q + 2),
        NORTH => (Some(NORTH), q - 2),
        SOUTH if q >= 2 => (None, q - 2),
        _ => (Some(SOUTH), q + 2),
    }
}

/// The base mesh of quadrant `q` with the opacities of its first layers (`alphas`, at most
/// [`folded_layers`], in the layers' order, on the mesh's vertices: [`refine_quad`]) at each vertex, where terrain.wesl reads them:
/// the first in the vertex colour's alpha, the second and third in the second texture
/// coordinate, the fourth in the size of the tangent's last number past 1 (its sign stays
/// [`TANGENT_SIGN`]), and, where [`folded_layers`] is [`FOLDED_LAYERS`], the fifth and
/// sixth in the first texture coordinate, which the shader then works out.
#[cfg(test)]
fn folded_mesh(land: &Land, surface: &Surface, q: u8, alphas: &[&[f32]], cell_origin: Vec2, tile: f32) -> Option<Mesh> {
    folded_mesh_drawn(land, surface, q, QuadrantDraw::FULL, alphas, cell_origin, tile)
}

/// [`folded_mesh`] drawn as `draw` says ([`quadrant_mesh_drawn`]).
fn folded_mesh_drawn(land: &Land, surface: &Surface, q: u8, draw: QuadrantDraw, alphas: &[&[f32]], cell_origin: Vec2, tile: f32) -> Option<Mesh> {
    use bevy::mesh::VertexAttributeValues;
    let mut mesh = quadrant_mesh_drawn(land, surface, q, draw, None, cell_origin, tile)?;
    let at = |k: usize, v: usize| alphas.get(k).map_or(0.0, |a| a[v]);
    let n = quad_side(drawn_sub(surface, draw)).pow(2);
    if let Some(VertexAttributeValues::Float32x4(col)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        for (v, c) in col.iter_mut().enumerate() {
            c[3] = at(0, v);
        }
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, (0..n).map(|v| [at(1, v), at(2, v)]).collect::<Vec<_>>());
    if let Some(VertexAttributeValues::Float32x4(tan)) = mesh.attribute_mut(Mesh::ATTRIBUTE_TANGENT) {
        for (v, t) in tan.iter_mut().enumerate() {
            t[3] = TANGENT_SIGN * (1.0 + at(3, v));
        }
    }
    if folded_layers(tile) == FOLDED_LAYERS {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, (0..n).map(|v| [at(4, v), at(5, v)]).collect::<Vec<_>>());
    }
    Some(mesh)
}

/// A layer's opacity over the 17 × 17 grid.
fn alpha_grid(l: &Layer) -> Vec<f32> {
    let mut alpha = vec![0.0f32; QUAD_SIDE * QUAD_SIDE];
    for &(p, o) in &l.alpha {
        if let Some(a) = alpha.get_mut(p as usize) {
            *a = o.clamp(0.0, 1.0);
        }
    }
    alpha
}

/// The opacity a cell's (or a neighbour's) layers give texture `tex` in quadrant `q`
/// (several layers of one texture: the most), `None` when no layer paints it there.
fn texture_grid(layers: &[Layer], q: u8, tex: Option<u32>) -> Option<Vec<f32>> {
    layers.iter().filter(|l| !l.base && l.quadrant == q && l.texture == tex).map(alpha_grid).reduce(|a, b| a.iter().zip(&b).map(|(x, y)| x.max(*y)).collect())
}

/// The texture layers a cell is drawn with: its own, stitched to the quadrants across
/// each quadrant edge, inside the cell and in the neighbouring cells (`neighbours`, by
/// side: [`NEIGHBOURS`]; `None` where there is no landscape or it is not read).
///
/// Each quadrant's layers come from its own record, so along a shared edge the two sides
/// can disagree and the ground changes texture along a straight line: at save 36 (cell
/// (−14, −17), issue #2089) the south cell paints LTEX 000357BA at up to 1.00 along its
/// north edge and the north cell has no layer of it [observed: `fnvtool world land
/// --full`; world.md §3 counts the disagreeing edges]. The rule [design; what the original
/// does at such edges is not known]: at every vertex of a shared edge a texture layer's
/// opacity is the larger of the two sides'; a quadrant with no layer of a texture its
/// neighbour paints on the edge gets one, drawn over its own layers in the neighbour's
/// order, painted on the edge only (it fades out across the next grid square). A texture
/// that is the quadrant's own base is not added (the base shows it), and bases are left
/// as they are. Both cells take the larger value from the same records, so they agree.
/// Corners shared only diagonally are not looked at.
pub fn stitch_layers(own: &[Layer], neighbours: &[Option<Vec<Layer>>; 4]) -> Vec<Layer> {
    let mut out: Vec<Layer> = own.to_vec();
    // Stitched grids of `out`'s non-base layers, by index; added layers after them.
    let mut grids: HashMap<usize, Vec<f32>> = HashMap::new();
    for q in 0..4u8 {
        let base_tex = own.iter().rev().find(|l| l.base && l.quadrant == q).and_then(|b| b.texture);
        let top = own.iter().filter(|l| l.quadrant == q).map(|l| l.layer).max().unwrap_or(-1);
        let mut added = 0i16;
        for side in [NORTH, EAST, SOUTH, WEST] {
            let (cell, q2) = across(q, side);
            let Some(theirs) = (match cell {
                None => Some(own),
                Some(c) => neighbours[c].as_deref(),
            }) else {
                continue;
            };
            let opposite = (side + 2) % 4;
            let mut their_layers: Vec<&Layer> = theirs.iter().filter(|l| !l.base && l.quadrant == q2).collect();
            their_layers.sort_by_key(|l| l.layer);
            for l in their_layers {
                let tex = l.texture;
                if tex == base_tex {
                    continue;
                }
                let their_grid = alpha_grid(l);
                let edge: Vec<(usize, f32)> = (0..QUAD_SIDE).map(|k| (edge_position(side, k), their_grid[edge_position(opposite, k)])).filter(|e| e.1 > 0.0).collect();
                if edge.is_empty() {
                    continue;
                }
                let i = match out.iter().position(|o| !o.base && o.quadrant == q && o.texture == tex) {
                    Some(i) => i,
                    None => {
                        added += 1;
                        out.push(Layer { quadrant: q, texture: tex, layer: top + added, base: false, alpha: Vec::new() });
                        out.len() - 1
                    }
                };
                let g = grids.entry(i).or_insert_with(|| texture_grid(own, q, tex).unwrap_or_else(|| vec![0.0; QUAD_SIDE * QUAD_SIDE]));
                for (p, a) in edge {
                    g[p] = g[p].max(a);
                }
            }
        }
    }
    for (i, g) in grids {
        out[i].alpha = g.iter().enumerate().filter(|(_, a)| **a > 0.0).map(|(p, a)| (p as u16, *a)).collect();
    }
    out
}

/// A quadrant's layers over its base, in drawing order, with their index in `layers`.
fn quadrant_layers(layers: &[Layer], q: u8) -> Vec<(usize, &Layer)> {
    let mut layers: Vec<_> = layers.iter().enumerate().filter(|(_, l)| !l.base && l.quadrant == q).collect();
    layers.sort_by_key(|(_, l)| l.layer);
    layers
}

/// One quadrant's meshes as drawn ([`build_quadrant`]): its base, the blended mesh of each
/// drawn layer (by its index in the cell's drawn layers) and its base with its first
/// [`folded_layers`] layers folded in ([`folded_mesh`]; none for a quadrant without layers).
#[derive(Default)]
pub struct QuadMeshes {
    base: Option<Mesh>,
    layers: HashMap<usize, Mesh>,
    folded: Option<Mesh>,
}

/// Build quadrant `q`'s meshes over `surface` as `draw` says (on a worker thread: when a
/// cell is read ahead, or when 4 × 4 near redraws a quadrant at another level,
/// `crate::terrain_lod`).
pub fn build_quadrant(land: &Land, surface: &Surface, layers: &[Layer], q: u8, draw: QuadrantDraw, origin: Vec2, tile: f32) -> QuadMeshes {
    let mut out = QuadMeshes { base: quadrant_mesh_drawn(land, surface, q, draw, None, origin, tile), ..default() };
    let quad = quadrant_layers(layers, q);
    let sub = drawn_sub(surface, draw);
    let alphas: Vec<Vec<f32>> = quad.iter().map(|(_, l)| refine_quad(&alpha_grid(l), sub)).collect();
    for ((i, _), alpha) in quad.iter().zip(&alphas) {
        if let Some(mesh) = quadrant_mesh_drawn(land, surface, q, draw, Some(alpha), origin, tile) {
            out.layers.insert(*i, mesh);
        }
    }
    if !alphas.is_empty() {
        let first: Vec<&[f32]> = alphas.iter().take(folded_layers(tile)).map(Vec::as_slice).collect();
        out.folded = folded_mesh_drawn(land, surface, q, draw, &first, origin, tile);
    }
    out
}

/// A cell's landscape, read and its quadrant meshes built (on a worker thread when it is
/// read ahead, world.rs).
pub struct TerrainPrep {
    grid: (i32, i32),
    /// The cell's landscape as the data has it (grass and footsteps read its layers).
    pub land: Land,
    /// The surface the meshes are built on, stored or smoothed (the collision's too).
    pub surface: Surface,
    /// The layers drawn: the land's, stitched to the neighbours' ([`stitch_layers`]).
    layers: Vec<Layer>,
    /// Each quadrant's meshes, and how they are drawn.
    quads: [QuadMeshes; 4],
    draw: QuadrantDraw,
    /// The cell's south-west corner (game space).
    origin: Vec2,
    /// The texture repeat the meshes were built for.
    tile: f32,
}

/// Read LAND `land_id` of grid square `grid` and build its meshes (`tile`: world units per
/// texture repeat, [`TerrainSettings::tile`]); `lands`: the LAND of the grid squares around
/// it (`fnv_world::land_surface::LandIds`), whose texture layers its own are stitched to
/// ([`NEIGHBOURS`]) and, when `spec` (`nvf_play::smooth_terrain::spec`, taken when the job
/// is made) smooths, whose heights the smoothed surface reads along its edges; every
/// quadrant drawn as `draw` says (in full, or 2 × 2 at first under 4 × 4 near:
/// `crate::terrain_lod::first_draw`). `None`: no landscape or no heights.
#[allow(clippy::too_many_arguments)]
pub fn prepare_terrain(lo: &LoadOrder, schema: &EsmSchema, land_id: u32, grid: (i32, i32), tile: f32, lands: LandIds, spec: SurfaceSpec, draw: QuadrantDraw) -> Option<TerrainPrep> {
    let land = load_land(lo, schema, land_id, LandParts::ALL)?;
    let centre = land.heights.as_ref()?;
    let surface = match spec {
        SurfaceSpec::Smoothed(params) => {
            let mut ids = lands;
            ids[1][1] = None;
            let mut around = load_heights(lo, schema, &ids, true);
            around[1][1] = Some(centre.clone());
            Surface::smoothed(&block_of(&around), params)?
        }
        SurfaceSpec::Stored => Surface::stored(centre),
    };
    let parts = LandParts { heights: false, normals: false, colors: false, layers: true };
    let around = NEIGHBOURS.map(|(dx, dy)| lands[(dy + 1) as usize][(dx + 1) as usize].and_then(|id| load_land(lo, schema, id, parts)).map(|l| l.layers));
    Some(prepare_land_on(land, surface, &around, grid, tile, draw))
}

/// [`prepare_terrain`] once the records are read: `land` with its `surface`, the
/// neighbours' layers by side ([`NEIGHBOURS`]), every quadrant drawn as `draw` says.
pub(crate) fn prepare_land_on(land: Land, surface: Surface, around: &[Option<Vec<Layer>>; 4], grid: (i32, i32), tile: f32, draw: QuadrantDraw) -> TerrainPrep {
    let layers = stitch_layers(&land.layers, around);
    let origin = Vec2::new(grid.0 as f32 * fnv_world::CELL_SIZE, grid.1 as f32 * fnv_world::CELL_SIZE);
    let quads = std::array::from_fn(|q| build_quadrant(&land, &surface, &layers, q as u8, draw, origin, tile));
    TerrainPrep { grid, land, surface, layers, quads, draw, origin, tile }
}

/// [`prepare_land_on`] on the stored triangles, drawn in full (`land` has heights).
#[cfg(test)]
pub(crate) fn prepare_land(land: Land, around: &[Option<Vec<Layer>>; 4], grid: (i32, i32), tile: f32) -> TerrainPrep {
    let surface = Surface::stored(land.heights.as_ref().expect("a land with heights"));
    prepare_land_on(land, surface, around, grid, tile, QuadrantDraw::FULL)
}

/// A mesh of a cell's land (its base or a texture layer over it).
#[derive(Component)]
pub struct LandMesh;

/// The quadrant (0 south-west, 1 south-east, 2 north-west, 3 north-east) a [`LandMesh`]
/// draws, so 4 × 4 near can replace one quadrant's meshes (`crate::terrain_lod`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LandQuadrant(pub u8);

/// What a cell's quadrants are redrawn from (`crate::terrain_lod`, 4 × 4 near): its
/// landscape, surface, drawn layers, origin and texture repeat, as the cell was spawned
/// with them.
pub struct TerrainSource {
    pub land: Land,
    pub surface: Surface,
    pub layers: Vec<Layer>,
    pub origin: Vec2,
    pub tile: f32,
}

/// Spawn a cell's terrain from its prepared landscape ([`prepare_terrain`]) in one go;
/// returns the landscape and the surface it is drawn on (for the grass). Under 4 × 4 near
/// the cell's quadrants are handed to `crate::terrain_lod` to be redrawn by distance. Cell
/// streaming spawns the same through [`TerrainSpawn`], a quadrant at a time.
pub fn spawn_terrain(commands: &mut Commands, cell: Entity, world: &WorldData, prep: TerrainPrep, tm: &mut TerrainCtx) -> (Land, Surface) {
    let mut spawn = TerrainSpawn::begin(world, prep, tm);
    while !spawn.step(commands, cell, world, tm) {}
    spawn.finish(commands, cell, tm)
}

/// [`spawn_terrain`] with the fold decision and the mesh terrain blend's capture given
/// (`capture`: the blend is on, `mesh_terrain_blend.rs::recording`). With it, the cell
/// also gets a [`crate::terrain_surface::SurfaceCell`]: every quadrant's draws in their
/// drawing order, the base (folded or not) and then each blended layer drawn over it, for
/// the bake's flat unlit copies [design]. The near draws are the same either way.
#[cfg(test)]
pub(crate) fn spawn_terrain_with(commands: &mut Commands, cell: Entity, world: &WorldData, prep: TerrainPrep, tm: &mut TerrainCtx, folds: bool, capture: bool) -> (Land, Surface) {
    let mut spawn = TerrainSpawn::begin_with(world, prep, tm, folds, capture, true);
    while !spawn.step(commands, cell, world, tm) {}
    spawn.finish(commands, cell, tm)
}

/// A cell's terrain drawn only, for the full-detail ring (full_detail.rs, #2701): as
/// [`spawn_terrain`] draws it, but it adds nothing to the cache's counts (the smoothed and
/// stored cells of the terrain summary, the layers past a folded group), captures no
/// surface for the mesh terrain blend and is not redrawn by distance under 4 × 4 near
/// (`crate::terrain_lod` knows the loaded cells only) [design].
pub(crate) fn spawn_terrain_draw_only(commands: &mut Commands, cell: Entity, world: &WorldData, prep: TerrainPrep, tm: &mut TerrainCtx) -> (Land, Surface) {
    let folds = tm.tm.folds();
    let mut spawn = TerrainSpawn::begin_with(world, prep, tm, folds, false, false);
    while !spawn.step(commands, cell, world, tm) {}
    spawn.finish(commands, cell, tm)
}

/// [design] A cell's terrain being spawned a quadrant at a time (docs/design/
/// streaming-hitches.md §3.4): [`spawn_terrain`] in steps, so cell streaming can stop
/// between two quadrants when its frame's budget is spent (world.rs `CellStart`). The cell
/// stays hidden until all of it is in, and what is spawned is the same either way.
pub struct TerrainSpawn {
    grid: (i32, i32),
    land: Land,
    surface: Surface,
    drawn: Vec<Layer>,
    /// The quadrants not spawned yet, in order.
    quads: std::vec::IntoIter<(usize, QuadMeshes)>,
    draw: QuadrantDraw,
    origin: Vec2,
    tile: f32,
    folds: bool,
    /// A loaded cell's terrain (counted, handed to `crate::terrain_lod`), not a draw-only
    /// one ([`spawn_terrain_draw_only`]).
    counted: bool,
    captured: Option<crate::terrain_surface::SurfaceCell>,
    overflow: usize,
    entities: [Vec<Entity>; 4],
}

impl TerrainSpawn {
    /// A loaded cell's terrain: the run's fold decision, and the capture while the mesh
    /// terrain blend records.
    pub fn begin(world: &WorldData, prep: TerrainPrep, tm: &mut TerrainCtx) -> Self {
        let folds = tm.tm.folds();
        Self::begin_with(world, prep, tm, folds, crate::mesh_terrain_blend::recording(), true)
    }

    fn begin_with(world: &WorldData, prep: TerrainPrep, tm: &mut TerrainCtx, folds: bool, capture: bool, counted: bool) -> Self {
        let TerrainPrep { grid, land, surface, layers: drawn, quads, draw, origin, tile } = prep;
        if counted {
            if surface.is_smoothed() {
                tm.tm.cache.smoothed_cells += 1;
            } else {
                tm.tm.cache.stored_cells += 1;
            }
        }
        let captured = capture.then(|| surface_cell(world, grid, &surface));
        let quads: Vec<(usize, QuadMeshes)> = quads.into_iter().enumerate().collect();
        Self { grid, land, surface, drawn, quads: quads.into_iter(), draw, origin, tile, folds, counted, captured, overflow: 0, entities: Default::default() }
    }

    /// The landscape and the cell's grid square (for the grass, world.rs).
    pub fn grid(&self) -> (i32, i32) { self.grid }

    /// Spawn the next quadrant; `true` when none is left.
    pub fn step(&mut self, commands: &mut Commands, cell: Entity, world: &WorldData, tm: &mut TerrainCtx) -> bool {
        let Some((q, meshes)) = self.quads.next() else { return true };
        let near = NearDraw { commands: &mut *commands, cell, visibility: Visibility::Inherited, count: self.counted };
        let spawned = quadrant_draws(Some(near), world, &self.drawn, q as u8, meshes, self.tile, tm, self.folds, self.captured.as_mut());
        self.overflow += spawned.overflow;
        self.entities[q] = spawned.entities;
        self.quads.len() == 0
    }

    /// Every quadrant is in ([`TerrainSpawn::step`] said so): the capture, the 4 × 4 near
    /// hand-over and the landscape and surface for the grass.
    pub fn finish(self, commands: &mut Commands, cell: Entity, tm: &mut TerrainCtx) -> (Land, Surface) {
        let Self { land, surface, drawn, draw, origin, tile, counted, captured, overflow, entities, quads, .. } = self;
        debug_assert_eq!(quads.len(), 0, "finished with quadrants left");
        // The first cell with layers past a folded group says how many it drew blended over
        // them (once a run; the smoke check that such layer draws exist at all).
        if overflow > 0 && counted {
            let most = folded_layers(tile);
            let first = tm.tm.cache.overflow_layers == 0;
            tm.tm.cache.overflow_layers += overflow;
            if first {
                info!("terrain: {overflow} layers drawn blended over folded quadrants (past the first {most} of a quadrant), in the first cell that has them");
            }
        }
        if let Some(captured) = captured {
            commands.entity(cell).insert(captured);
        }
        // 4 x 4 near (the run's choice, fixed before the first cell): the cell's quadrants are
        // redrawn by distance from what they were built from.
        if let Some(rule) = nvf_play::smooth_terrain::distance_rule().filter(|_| counted) {
            let source = TerrainSource { land: land.clone(), surface: surface.clone(), layers: drawn, origin, tile };
            tm.tm.cache.lod.get_or_insert_with(|| crate::terrain_lod::TerrainLod::new(rule)).add_cell(cell, std::sync::Arc::new(source), draw, entities);
        }
        (land, surface)
    }
}

/// [design] The capture alone of a cell drawn while the mesh terrain blend was off
/// (#2661: the settings row turns it on while the game runs): the same quadrant stacks
/// as [`spawn_terrain_with`] gives, from the same prepared landscape and the same cached
/// materials, without spawning a near draw again. Always `Some`: a prepared landscape has
/// heights.
pub(crate) fn capture_surface(world: &WorldData, prep: TerrainPrep, tm: &mut TerrainCtx, folds: bool) -> Option<crate::terrain_surface::SurfaceCell> {
    let TerrainPrep { grid, surface, layers: drawn, quads, tile, .. } = prep;
    let mut captured = surface_cell(world, grid, &surface);
    for (q, meshes) in quads.into_iter().enumerate() {
        quadrant_draws(None, world, &drawn, q as u8, meshes, tile, tm, folds, Some(&mut captured));
    }
    Some(captured)
}

/// An empty capture of the cell at `grid`, before its quadrants' draws are added. [design]
/// #2149/#2661: the ground heights the mesh terrain blend measures against are the drawn
/// `surface`'s own (`side() * side()`, row-major: the stored LAND heights as stored, every
/// vertex of the smoothed surface where it smooths); the blend's height atlas holds that
/// surface's triangles (terrain_surface.rs `encode_height_cell`).
fn surface_cell(world: &WorldData, grid: (i32, i32), surface: &Surface) -> crate::terrain_surface::SurfaceCell {
    use crate::terrain_surface::{CapturePath, SurfaceCell};
    SurfaceCell {
        world: world.world, grid, heights: surface.heights().to_vec(), quadrants: Vec::new(),
        paths: [CapturePath::HeightOnly("missing-base-mesh"); 4],
        content_revision: crate::terrain_surface::capture_revision(), sources: vec![], dependencies: default(),
        detail: default(),
    }
}

/// [design] Round 10a: the vertex attributes of a base draw's mesh that the mesh terrain
/// blend's landscape detail interpolates (linear vertex colour, normal, tangent direction;
/// game space) and the tangents' handedness, read from the very mesh drawn
/// ([`quadrant_mesh_drawn`] or [`folded_mesh_drawn`], whose tangent's fourth number keeps
/// the sign of [`TANGENT_SIGN`]) at its LAND vertices. The detail holds a quadrant's
/// 17 × 17 LAND vertices: of a mesh drawn finer (#2149, #2676; a full, half or coarse-sided
/// draw) it takes the vertices that lie on them, every `stride`-th of each row and column.
/// [design] #2149/#2661, a known approximation: as stored, and for the tint and the
/// opacities always (they are linear on the LAND triangles, [`stored_weights`],
/// [`refine_quad`]), the detail interpolated over a LAND triangle is what the drawn mesh
/// has; with smoothing the normal and tangent are the surface's at the LAND vertices,
/// interpolated linearly over the LAND triangle, while the drawn mesh has the surface's
/// normal (and its tangent) at every finer vertex, so the detail's shading follows the
/// smoothed ground's curvature only from one LAND vertex to the next. `None` for a mesh
/// that is no such grid.
pub(crate) struct DetailVertices { pub tint: Vec<[f32; 3]>, pub normal: Vec<[f32; 3]>, pub tangent: Vec<[f32; 3]>, pub sign: f32 }
pub(crate) fn detail_vertices(mesh: &Mesh) -> Option<DetailVertices> {
    use bevy::mesh::VertexAttributeValues as V;
    let Some(V::Float32x3(normal)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { return None };
    let Some(V::Float32x4(tangent)) = mesh.attribute(Mesh::ATTRIBUTE_TANGENT) else { return None };
    let Some(V::Float32x4(colour)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else { return None };
    let sign = tangent.first()?[3].signum();
    let qs = normal.len().isqrt();
    if qs < QUAD_SIDE || qs * qs != normal.len() || !(qs - 1).is_multiple_of(QUAD_SIDE - 1) || tangent.len() != normal.len() || colour.len() != normal.len() {
        return None;
    }
    let stride = (qs - 1) / (QUAD_SIDE - 1);
    let at = |v: usize| (v / QUAD_SIDE) * stride * qs + (v % QUAD_SIDE) * stride;
    let land = 0..QUAD_SIDE * QUAD_SIDE;
    Some(DetailVertices {
        tint: land.clone().map(|v| { let c = colour[at(v)]; [c[0], c[1], c[2]] }).collect(),
        normal: land.clone().map(|v| normal[at(v)]).collect(),
        tangent: land.map(|v| { let t = tangent[at(v)]; [t[0], t[1], t[2]] }).collect(),
        sign,
    })
}

/// [design] #2149/#2661: the mesh terrain blend's bake copy of a drawn quadrant mesh at the
/// LAND grid's resolution. Of a mesh drawn finer than the LAND grid (a row-major `qs × qs`
/// grid, `qs - 1` a multiple of 16: [`quadrant_mesh_drawn`], [`folded_mesh_drawn`], drawn
/// full, half or with coarse sides), the 17 × 17 mesh of its LAND vertices: every attribute
/// taken at those vertices (the stride [`detail_vertices`] takes), and each LAND triangle
/// (split south-west → north-east, as a mesh drawn as stored has it) wherever the drawn mesh
/// has a triangle inside it. The bake draws its copies flat and unlit, and everything it
/// reads is linear on the LAND triangles (the vertex colour and every opacity, packed
/// channels included: [`stored_weights`], [`refine_quad`]; the texture coordinate: the
/// world place over the repeat) or not read for the colour at all (the normals, the
/// tangents' direction, the heights, which the copy flattens), so the copy bakes the same
/// picture (up to float rounding in the interpolation) from about a fifteenth of the vertices at
/// the default 4 × 4. A layer's mesh keeps a LAND triangle exactly where one of its corners
/// paints, as a mesh drawn as stored does ([`quadrant_mesh_drawn`]): its opacity is linear there, so a finer
/// triangle inside it with three unpainted corners would be degenerate. A mesh at the LAND
/// grid's resolution (smoothing off), or one that is no such grid, comes back unchanged.
pub(crate) fn land_resolution(mesh: &Mesh) -> Mesh {
    use bevy::mesh::VertexAttributeValues as V;
    let n = mesh.count_vertices();
    let qs = n.isqrt();
    if qs <= QUAD_SIDE || qs * qs != n || !(qs - 1).is_multiple_of(QUAD_SIDE - 1) {
        return mesh.clone();
    }
    let Some(Indices::U32(drawn)) = mesh.indices() else { return mesh.clone() };
    let stride = (qs - 1) / (QUAD_SIDE - 1);
    let at = |v: usize| (v / QUAD_SIDE) * stride * qs + (v % QUAD_SIDE) * stride;
    let land = 0..QUAD_SIDE * QUAD_SIDE;
    let mut out = Mesh::new(mesh.primitive_topology(), mesh.asset_usage);
    for (attribute, values) in mesh.attributes() {
        let picked = match values {
            V::Float32x2(a) => V::Float32x2(land.clone().map(|v| a[at(v)]).collect()),
            V::Float32x3(a) => V::Float32x3(land.clone().map(|v| a[at(v)]).collect()),
            V::Float32x4(a) => V::Float32x4(land.clone().map(|v| a[at(v)]).collect()),
            _ => return mesh.clone(),
        };
        out.insert_attribute(*attribute, picked);
    }
    // The LAND triangle each drawn triangle lies in, from three times its centroid (whole
    // numbers): its LAND square, and whether it lies above the square's diagonal.
    let squares = QUAD_SIDE - 1;
    let s3 = 3 * stride;
    let mut kept = vec![false; squares * squares * 2];
    for t in drawn.as_chunks::<3>().0 {
        let (r, c) = t.iter().fold((0, 0), |(r, c), &v| (r + v as usize / qs, c + v as usize % qs));
        let (sr, sc) = ((r / s3).min(squares - 1), (c / s3).min(squares - 1));
        kept[(sr * squares + sc) * 2 + usize::from(r - sr * s3 > c - sc * s3)] = true;
    }
    let mut idx = Vec::with_capacity(squares * squares * 6);
    for sr in 0..squares {
        for sc in 0..squares {
            let i = (sr * QUAD_SIDE + sc) as u32;
            let (e, nn, ne) = (i + 1, i + QUAD_SIDE as u32, i + QUAD_SIDE as u32 + 1);
            // Counter-clockwise from above, as [`quadrant_mesh_drawn`]'s.
            for (k, t) in [[i, e, ne], [i, ne, nn]].into_iter().enumerate() {
                if kept[(sr * squares + sc) * 2 + k] {
                    idx.extend_from_slice(&t);
                }
            }
        }
    }
    out.insert_indices(Indices::U32(idx));
    out
}

/// [design] Round 10a: one quadrant's detail parts while its draws are made: the base
/// draw's source and vertices, whether its coordinate is cell-local, the drawn overlays'
/// opacity grids and the overflow layers' materials, in drawing order.
struct DetailParts {
    base: crate::terrain_surface::DetailBase,
    vertices: Option<DetailVertices>,
    cell_local: bool,
    opacity: Vec<Vec<f32>>,
    overflow: Vec<Handle<NifMaterial>>,
}

impl DetailParts {
    /// The capture, resolved against the materials the near draw uses; the colour cache
    /// with the reason when the vertices are missing or the overlays exceed the cap.
    /// [design] Round 10a review (second round): materials the detail cannot reproduce
    /// (`ResolvedDetail::from_materials`'s reasons, e.g. an overflow layer already tinted
    /// when the blend is turned on or a cell is backfilled) keep the capture with its
    /// `Err(reason)`, as `refresh_captures` does, so the publication refuses the quadrant
    /// with that reason and an edit back restores its detail.
    fn capture(self, folded: &Assets<TerrainMaterial>, ordinary: &Assets<NifMaterial>, tile: f32) -> crate::terrain_surface::DetailCapture {
        use crate::terrain_surface::{DetailCapture, QuadrantDetail};
        let Some(DetailVertices { tint, normal, tangent, sign: tangent_sign }) = self.vertices else { return DetailCapture::Colour("missing-vertex-attributes") };
        if self.opacity.len() > nvf_assets::nif_surface_blend::DETAIL_MAX_OVERLAYS { return DetailCapture::Colour("overlays-over-cap"); }
        let resolved = crate::terrain_surface::resolve_detail(&self.base, &self.overflow, self.opacity.len(), folded, ordinary);
        DetailCapture::Layers(Box::new(QuadrantDetail { base: self.base, overflow: self.overflow, cell_local: self.cell_local, tile, tint, normal, tangent, tangent_sign, opacity: self.opacity, resolved }))
    }
}

/// What [`spawn_quadrant`] spawned: the entities and the layers drawn blended over a
/// folded base.
pub struct SpawnedQuadrant {
    pub entities: Vec<Entity>,
    /// Their meshes (for the swap's readiness check, `crate::terrain_lod`).
    pub meshes: Vec<AssetId<Mesh>>,
    pub overflow: usize,
}

/// Spawn quadrant `q`'s meshes under `cell` with `visibility`; `count`: add them to the
/// cache's counts (a cell's first spawn; not a redraw at another level).
#[allow(clippy::too_many_arguments)]
pub fn spawn_quadrant(commands: &mut Commands, cell: Entity, world: &WorldData, drawn: &[Layer], q: u8, meshes: QuadMeshes, tile: f32, tm: &mut TerrainCtx, visibility: Visibility, count: bool) -> SpawnedQuadrant {
    let folds = tm.tm.folds();
    quadrant_draws(Some(NearDraw { commands, cell, visibility, count }), world, drawn, q, meshes, tile, tm, folds, None)
}

/// Where [`quadrant_draws`] spawns a quadrant's near draws: under `cell` with
/// `visibility`; `count`: add them to the cache's counts.
struct NearDraw<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    cell: Entity,
    visibility: Visibility,
    count: bool,
}

impl NearDraw<'_, '_, '_> {
    fn spawn(&mut self, tm: &mut TerrainCtx, out: &mut SpawnedQuadrant, q: u8, mesh: Mesh, mat: MeshMaterialAny) {
        let mesh = tm.assets.meshes.add(mesh);
        let id = mesh.id();
        let land = (Mesh3d(mesh), Transform::IDENTITY, self.visibility, LandMesh, LandQuadrant(q));
        let e = match mat {
            MeshMaterialAny::Folded(m) => self.commands.spawn((land, MeshMaterial3d(m))).id(),
            MeshMaterialAny::Nif(m) => self.commands.spawn((land, MeshMaterial3d(m))).id(),
        };
        self.commands.entity(self.cell).add_child(e);
        out.entities.push(e);
        out.meshes.push(id);
    }
}

/// Quadrant `q`'s draws from its built `meshes`: spawned at `near` when given
/// ([`spawn_quadrant`]) and, with `capture` (the mesh terrain blend, #2661), added to the
/// cell's [`crate::terrain_surface::SurfaceCell`] in their drawing order: the base (folded
/// or not), then each blended layer drawn over it, with the quadrant's detail parts
/// [design]. The cache's counts move only with a counting `near`.
#[allow(clippy::too_many_arguments)]
fn quadrant_draws(mut near: Option<NearDraw>, world: &WorldData, drawn: &[Layer], q: u8, meshes: QuadMeshes, tile: f32, tm: &mut TerrainCtx, folds: bool, mut capture: Option<&mut crate::terrain_surface::SurfaceCell>) -> SpawnedQuadrant {
    use crate::terrain_surface::{CaptureDraw, CapturePath, CaptureSource, DetailBase, DetailCapture, SurfaceCaptureMaterial};
    let QuadMeshes { base: mut base_mesh, layers: mut layer_meshes, folded } = meshes;
    let count = near.as_ref().is_some_and(|n| n.count);
    let most = folded_layers(tile);
    // The repeat for the shader to work the texture coordinate out with, 0 when the mesh
    // carries the coordinate.
    let shader_tile = if most == FOLDED_LAYERS { tile } else { 0.0 };
    let mut out = SpawnedQuadrant { entities: Vec::new(), meshes: Vec::new(), overflow: 0 };
    let base = drawn.iter().rev().find(|l| l.base && l.quadrant == q);
    if base.is_none() && count {
        tm.tm.cache.quadrants_without_base += 1;
    }
    let tex = tm.texture_of(world, base.and_then(|b| b.texture));
    let layers = quadrant_layers(drawn, q);
    // [design] The bake's copies of this quadrant's draws, in drawing order; `None`
    // while no base is drawn (the quadrant's colour then stays invalid).
    let mut stack: Option<(bool, Vec<(Mesh, CaptureDraw)>)> = None;
    // [design] Round 10a: the same draws' detail parts (only while capturing).
    let mut detail: Option<DetailParts> = None;
    // The first layers folded into the base's draw (module notes), when each of them
    // has a texture; otherwise the base alone and every layer blended over it.
    let mut blended_from = 0;
    let folded_mat = match folded.filter(|_| folds) {
        Some(mesh) => {
            let first: Option<Vec<String>> = layers.iter().take(most).map(|(_, l)| tm.texture_of(world, l.texture)).collect();
            first.and_then(|first: Vec<String>| Some((mesh, tm.folded_material(tex.clone(), &first, shader_tile)?, first.len())))
        }
        // The base alone (#3846): its own mesh, which carries the texture coordinate.
        None if folds && layers.is_empty() && base_mesh.is_some() && tm.base_may_march(tex.as_deref()) => tm.folded_material(tex.clone(), &[], 0.0).and_then(|mat| Some((base_mesh.take()?, mat, 0))),
        None => None,
    };
    if let Some((mesh, mat, n)) = folded_mat {
        if let Some(surface) = capture.as_deref_mut() {
            surface.sources.push(CaptureSource::Folded(mat.clone()));
            // [design] #2149/#2661: the bake's copy at the LAND grid's resolution
            // ([`land_resolution`]); the detail reads the drawn mesh itself.
            stack = Some((true, vec![(land_resolution(&mesh), CaptureDraw::Folded(Box::new(tm.tm.folded.get(&mat).unwrap().clone())))]));
            detail = Some(DetailParts {
                base: DetailBase::Folded(mat.clone()), vertices: detail_vertices(&mesh), cell_local: n > 0 && shader_tile > 0.0,
                opacity: layers.iter().take(n).map(|(_, l)| alpha_grid(l)).collect(), overflow: Vec::new(),
            });
        }
        if let Some(near) = near.as_mut() {
            near.spawn(tm, &mut out, q, mesh, MeshMaterialAny::Folded(mat));
        }
        if count && n > 0 {
            tm.tm.cache.folded_quadrants += 1;
            tm.tm.cache.folded_layers += n;
        }
        if count && n == 0 {
            tm.tm.cache.marched_bases += 1;
            if tm.tm.cache.marched_bases == 1 {
                info!("terrain: first quadrant without painted layers drawn by the folded material (its base has or may have a height)");
            }
        }
        blended_from = n;
    } else {
        let mat = tm.base_material(tex);
        if let Some(mesh) = base_mesh {
            if let Some(surface) = capture.as_deref_mut() {
                surface.sources.push(CaptureSource::Single(mat.clone(), false));
                // [design] Not folded: the single-texture capture, never TerrainMaterial
                // (whose layout a GPU without bindless textures cannot bind).
                let copy = SurfaceCaptureMaterial::from_source(tm.assets.materials.get(&mat).unwrap(), false);
                stack = Some((false, vec![(land_resolution(&mesh), CaptureDraw::Single(copy))]));
                detail = Some(DetailParts { base: DetailBase::Single(mat.clone()), vertices: detail_vertices(&mesh), cell_local: false, opacity: Vec::new(), overflow: Vec::new() });
            }
            if let Some(near) = near.as_mut() {
                near.spawn(tm, &mut out, q, mesh, MeshMaterialAny::Nif(mat));
            }
        }
    }
    for (rank, (i, l)) in layers.into_iter().enumerate().skip(blended_from) {
        let Some(tex) = tm.texture_of(world, l.texture) else { continue };
        let Some(mat) = tm.layer_material(tex, rank) else { continue };
        if let Some(mesh) = layer_meshes.remove(&i) {
            // [design] The near layer's colour, texture, blending and depth bias (so its
            // draw order), as a single-texture layer capture.
            if let Some((_, draws)) = &mut stack {
                if let Some(surface) = capture.as_deref_mut() { surface.sources.push(CaptureSource::Single(mat.clone(), true)); }
                let copy = SurfaceCaptureMaterial::from_source(tm.assets.materials.get(&mat).unwrap(), true);
                draws.push((land_resolution(&mesh), CaptureDraw::Single(copy)));
                // [design] Round 10a: an overflow layer joins the detail after the folded
                // ones, in drawing order, with the opacity grid its mesh was built from.
                if let Some(d) = &mut detail {
                    d.opacity.push(alpha_grid(l));
                    d.overflow.push(mat.clone());
                }
            }
            if let Some(near) = near.as_mut() {
                near.spawn(tm, &mut out, q, mesh, MeshMaterialAny::Nif(mat));
            }
            if count {
                tm.tm.cache.layers += 1;
            }
            if blended_from > 0 {
                out.overflow += 1;
            }
        }
    }
    if let (Some(surface), Some((folded, draws))) = (capture, stack) {
        let layers = draws.len() - 1;
        surface.detail[q as usize] = detail.map_or(DetailCapture::Colour("no-base-draw"), |d| d.capture(&tm.tm.folded, &tm.assets.materials, tile));
        surface.capture(q, draws, if folded { CapturePath::Folded { layers } } else { CapturePath::Fallback { layers } });
    }
    out
}

/// A land mesh's material: a folded quadrant's or a NIF one.
enum MeshMaterialAny {
    Folded(Handle<TerrainMaterial>),
    Nif(Handle<NifMaterial>),
}

#[cfg(test)]
use crate::decals::bc5_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    use fnv_world::land::SPACING;

    /// The land's own grid as stored (smoothing off).
    fn stored(land: &Land) -> Surface {
        Surface::stored(land.heights.as_ref().unwrap())
    }

    /// Hills with ridges across the split diagonal, coloured by height.
    fn hilly_land() -> Land {
        let heights: Vec<f32> = (0..SIDE * SIDE)
            .map(|i| {
                let (r, c) = ((i / SIDE) as f32, (i % SIDE) as f32);
                ((200.0 * (c * 0.4).sin() * (r * 0.3).cos() + 60.0 * ((c - r) * 0.9).sin()) / 8.0).round() * 8.0
            })
            .collect();
        let colors = (0..SIDE * SIDE).map(|i| [(i % 251) as u8, (i % 7 * 30) as u8, 200]).collect();
        Land { heights: Some(heights), normals: Some(vec![[0, 0, 127]; SIDE * SIDE]), colors: Some(colors), ..Default::default() }
    }

    fn smoothed_with(land: &Land, p: fnv_world::land_surface::SmoothParams) -> Surface {
        let mut block: fnv_world::land_surface::Block = [[None; 3]; 3];
        block[1][1] = land.heights.as_deref();
        Surface::smoothed(&block, p).unwrap()
    }

    /// Each mode at each subdivision (the bound does not change the meshes' layout).
    fn variants() -> Vec<fnv_world::land_surface::SmoothParams> {
        use fnv_world::land_surface::{SmoothMode, SmoothParams, SUBDIVS};
        let modes = [SmoothMode::Interpolate, SmoothMode::Fit, SmoothMode::DETAIL];
        modes.iter().flat_map(|&mode| SUBDIVS.map(|subdiv| SmoothParams { mode, subdiv, max_deviation: 12.0 })).collect()
    }

    fn attr3(mesh: &Mesh, a: bevy::mesh::MeshVertexAttribute) -> Vec<[f32; 3]> {
        let Some(VertexAttributeValues::Float32x3(v)) = mesh.attribute(a) else { panic!() };
        v.clone()
    }

    /// Smoothed, each quadrant mesh lies exactly on the surface the collision is built on
    /// (its vertices are the surface's), with the surface's normals and subdiv² times the
    /// squares; the LAND vertices keep their colours even when their heights move.
    #[test]
    fn a_smoothed_quadrant_is_drawn_on_the_collision_surface() {
        let land = hilly_land();
        for p in variants() {
            let s = smoothed_with(&land, p);
            let (sub, qs) = (s.sub(), quad_side(s.sub()));
            for q in 0..4u8 {
                let mesh = quadrant_mesh(&land, &s, q, None, Vec2::ZERO, 256.0).unwrap();
                let pos = attr3(&mesh, Mesh::ATTRIBUTE_POSITION);
                let nor = attr3(&mesh, Mesh::ATTRIBUTE_NORMAL);
                assert_eq!((pos.len(), triangles(&mesh).len()), (qs * qs, (qs - 1) * (qs - 1) * 2), "{p:?}");
                let (qx, qy) = ((q % 2) as usize * (qs - 1), (q / 2) as usize * (qs - 1));
                for (v, pt) in pos.iter().enumerate() {
                    let i = (qy + v / qs) * s.side() + qx + v % qs;
                    assert_eq!(*pt, s.vertex(i), "{p:?} quadrant {q} vertex {v}");
                    let n = Vec3::from_array(nor[v]);
                    assert!(n.dot(Vec3::from_array(s.normals().unwrap()[i])) > 0.9999);
                }
                // The triangles are the collision's, by vertex position: every one of them.
                let all: std::collections::HashSet<[u32; 3]> = s.triangles().into_iter().collect();
                for tri in triangles(&mesh) {
                    let global = tri.map(|v| ((qy + v as usize / qs) * s.side() + qx + v as usize % qs) as u32);
                    assert!(all.contains(&global), "{p:?} {global:?}");
                }
            }
            let mesh = quadrant_mesh(&land, &s, 0, None, Vec2::ZERO, 256.0).unwrap();
            let Some(VertexAttributeValues::Float32x4(col)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!() };
            let colors = land.colors.as_ref().unwrap();
            // LAND vertex (1, 1) is mesh vertex (sub, sub): its colour exactly; the point half
            // way along the diagonal to (2, 2) the mean of the two (the stored triangles'
            // paint), whether or not the fit moved the vertex's height.
            let at = |r: usize, c: usize| colors[r * SIDE + c].map(srgb_to_linear);
            assert_eq!(col[sub * qs + sub][..3], at(1, 1), "{p:?}");
            let half = sub + sub / 2;
            let mid = col[half * qs + half];
            for ((m, a), b) in mid.iter().zip(at(1, 1)).zip(at(2, 2)) {
                assert!((m - (a + b) / 2.0).abs() < 1e-6, "{p:?}");
            }
        }
    }

    #[test]
    fn layer_opacities_are_interpolated_on_the_stored_triangles() {
        let mut alpha = vec![0.0; QUAD_SIDE * QUAD_SIDE];
        alpha[5 * QUAD_SIDE + 7] = 0.8;
        let fine = refine_quad(&alpha, 2);
        let qs = quad_side(2);
        assert_eq!(fine.len(), qs * qs);
        assert_eq!(fine[10 * qs + 14], 0.8);
        // Half way to each neighbour along the rows, the columns and the diagonal.
        for v in [10 * qs + 15, 10 * qs + 13, 11 * qs + 14, 9 * qs + 14, 11 * qs + 15, 9 * qs + 13] {
            assert!((fine[v] - 0.4).abs() < 1e-6, "{v}: {}", fine[v]);
        }
        // Not across the other diagonal: the stored triangles never join those corners.
        assert_eq!((fine[11 * qs + 13], fine[9 * qs + 15]), (0.0, 0.0));
        assert_eq!(refine_quad(&alpha, 1), alpha);
        // By 4: the vertex at (20, 28), a quarter and half of the way to its neighbours.
        let (fine4, qs4) = (refine_quad(&alpha, 4), quad_side(4));
        assert_eq!((fine4.len(), fine4[20 * qs4 + 28]), (qs4 * qs4, 0.8));
        for (v, want) in [(20 * qs4 + 29, 0.6), (20 * qs4 + 30, 0.4), (22 * qs4 + 28, 0.4), (22 * qs4 + 30, 0.4), (22 * qs4 + 26, 0.0)] {
            assert!((fine4[v] - want).abs() < 1e-6, "{v}: {}", fine4[v]);
        }
        // A smoothed layer keeps only the triangles it paints: six stored triangles,
        // each split into subdiv² triangles. The opacity grid must match the surface.
        let land = hilly_land();
        for (subdiv, alpha, count) in [(2, &fine, 24), (4, &fine4, 96)] {
            let s = smoothed_with(&land, fnv_world::land_surface::SmoothParams { subdiv, ..Default::default() });
            let layer = quadrant_mesh(&land, &s, 0, Some(alpha), Vec2::ZERO, 256.0).unwrap();
            assert_eq!(attr3(&layer, Mesh::ATTRIBUTE_POSITION).len(), quad_side(subdiv).pow(2));
            assert_eq!(triangles(&layer).len(), count);
        }
    }

    /// Off: the stored surface draws exactly what was drawn before smoothing came.
    #[test]
    fn stored_quadrants_are_the_land_grid() {
        let land = hilly_land();
        let mesh = quadrant_mesh(&land, &stored(&land), 3, None, Vec2::ZERO, 256.0).unwrap();
        let pos = attr3(&mesh, Mesh::ATTRIBUTE_POSITION);
        let h = land.heights.as_ref().unwrap();
        assert_eq!(pos.len(), QUAD_SIDE * QUAD_SIDE);
        for (v, p) in pos.iter().enumerate() {
            let (gr, gc) = (16 + v / QUAD_SIDE, 16 + v % QUAD_SIDE);
            assert_eq!(*p, [gc as f32 * SPACING, gr as f32 * SPACING, h[gr * SIDE + gc]]);
        }
        let Some(VertexAttributeValues::Float32x4(col)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!() };
        assert_eq!(col[0][..3], land.colors.as_ref().unwrap()[16 * SIDE + 16].map(srgb_to_linear));
        assert!(attr3(&mesh, Mesh::ATTRIBUTE_NORMAL).iter().all(|n| *n == [0.0, 0.0, 1.0]));
    }


    #[test]
    fn surface_height_planes_match_real_mesh_triangles() {
        let h: Vec<_> = (0..SIDE * SIDE).map(|i| ((i * 17 + i / SIDE * 11) % 73) as f32 * 3.25 - 90.0).collect();
        let land = Land { heights: Some(h.clone()), ..default() };
        for q in 0..4 {
            let mesh = quadrant_mesh(&land, &stored(&land), q, None, Vec2::new(-8192.0, -4096.0), 256.0).unwrap();
            let VertexAttributeValues::Float32x3(vertices) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() else { panic!() };
            let triangles = triangles(&mesh);
            for (n, triangle) in triangles.iter().enumerate() {
                let v = triangle.map(|i| Vec3::from_array(vertices[i as usize]));
                let square = n / 2;
                let x = (q as usize % 2) * 16 + square % 16;
                let y = (q as usize / 2) * 16 + square / 16;
                let i = y * SIDE + x;
                let planes = crate::terrain_surface::height_planes(h[i], h[i + 1], h[i + SIDE], h[i + SIDE + 1]);
                for bary in [[0.2, 0.3, 0.5], [0.8, 0.1, 0.1], [0.0, 0.5, 0.5], if n % 2 == 0 { [0.5, 0.0, 0.5] } else { [0.5, 0.5, 0.0] }] {
                    let p = v[0] * bary[0] + v[1] * bary[1] + v[2] * bary[2];
                    let f = p.truncate() / SPACING - Vec2::new(x as f32, y as f32);
                    let plane = planes[n % 2];
                    let z = plane[0] + f.x * plane[1] + f.y * plane[2];
                    assert!((z - p.z).abs() < 0.001, "quadrant={q} triangle={n}");
                    if (f.y - f.x).abs() > 0.0001 {
                        assert_eq!(usize::from(f.y > f.x), n % 2, "actual mesh diagonal");
                    } else {
                        for plane in planes {
                            assert!((plane[0] + f.x * plane[1] + f.y * plane[2] - p.z).abs() < 0.001, "shared diagonal");
                        }
                    }
                }
            }
        }
    }

    fn sloped_land() -> Land {
        // A slope rising to the east and, less, to the north.
        let heights = (0..SIDE * SIDE).map(|i| (i % SIDE) as f32 * 40.0 + (i / SIDE) as f32 * 15.0).collect();
        let n = Vec3::new(-40.0 / SPACING, -15.0 / SPACING, 1.0).normalize() * 127.0;
        Land { heights: Some(heights), normals: Some(vec![[n.x as i8, n.y as i8, n.z as i8]; SIDE * SIDE]), ..Default::default() }
    }

    #[test]
    fn texture_repeat_and_direction() {
        // Quadrant 3 (north-east) of cell (-17, 0): u and v grow with world x and y.
        let origin = Vec2::new(-17.0 * 4096.0, 0.0);
        let mesh = quadrant_mesh(&sloped_land(), &stored(&sloped_land()), 3, None, origin, 256.0).unwrap();
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!() };
        assert_eq!(uv[0], [(origin.x + 2048.0) / 256.0, 2048.0 / 256.0]);
        // One repeat every two grid squares, eastwards and northwards.
        assert_eq!(uv[2][0] - uv[0][0], 1.0);
        assert_eq!(uv[2 * QUAD_SIDE][1] - uv[0][1], 1.0);
    }

    #[test]
    fn tangents_match_the_generator() {
        let mesh = quadrant_mesh(&sloped_land(), &stored(&sloped_land()), 0, None, Vec2::ZERO, 256.0).unwrap();
        let Some(VertexAttributeValues::Float32x4(ours)) = mesh.attribute(Mesh::ATTRIBUTE_TANGENT).cloned() else { panic!() };
        let mut generated = mesh.clone();
        generated.generate_tangents().unwrap();
        let Some(VertexAttributeValues::Float32x4(theirs)) = generated.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
        for ((a, b), n) in ours.iter().zip(theirs).zip(normals) {
            let (ta, tb) = (Vec3::new(a[0], a[1], a[2]), Vec3::new(b[0], b[1], b[2]));
            assert!(ta.dot(tb) > 0.99, "{a:?} {b:?}");
            assert_eq!(a[3], b[3]);
            assert!(ta.dot(Vec3::from_array(*n)).abs() < 1e-3 && (ta.length() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn a_folded_mesh_carries_its_layers_opacities_and_keeps_the_base() {
        let land = sloped_land();
        let n = QUAD_SIDE * QUAD_SIDE;
        let v = 5 * QUAD_SIDE + 7;
        let layer = |o: f32| {
            let mut a = vec![0.0; n];
            a[v] = o;
            a
        };
        let alphas = [layer(0.2), layer(0.4), layer(0.6), layer(0.8)];
        let refs: Vec<&[f32]> = alphas.iter().map(Vec::as_slice).collect();
        // A repeat that does not divide the cell: four layers, the mesh keeps its texture
        // coordinate.
        assert_eq!((folded_layers(256.0), folded_layers(300.0), folded_layers(0.0)), (FOLDED_LAYERS, FOLDED_LAYERS_WITH_UV, FOLDED_LAYERS_WITH_UV));
        let base = quadrant_mesh(&land, &stored(&land), 0, None, Vec2::ZERO, 300.0).unwrap();
        let folded = folded_mesh(&land, &stored(&land), 0, &refs, Vec2::ZERO, 300.0).unwrap();
        // The base's own geometry: every triangle, the same positions, normals and texture
        // coordinates, so the depth prepass and the shadows see the same ground.
        assert_eq!(triangles(&folded), triangles(&base));
        for a in [Mesh::ATTRIBUTE_POSITION, Mesh::ATTRIBUTE_NORMAL, Mesh::ATTRIBUTE_UV_0] {
            assert_eq!(folded.attribute(a).unwrap().get_bytes(), base.attribute(a).unwrap().get_bytes());
        }
        let Some(VertexAttributeValues::Float32x4(col)) = folded.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!() };
        let Some(VertexAttributeValues::Float32x2(uv_b)) = folded.attribute(Mesh::ATTRIBUTE_UV_1) else { panic!() };
        let Some(VertexAttributeValues::Float32x4(tan)) = folded.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        let Some(VertexAttributeValues::Float32x4(base_tan)) = base.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        // As terrain.wesl reads them back.
        let read = |i: usize| [col[i][3], uv_b[i][0], uv_b[i][1], tan[i][3].abs() - 1.0];
        let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        assert!(close(read(v), [0.2, 0.4, 0.6, 0.8]), "{:?}", read(v));
        assert!(close(read(0), [0.0; 4]), "{:?}", read(0));
        // The tangent's direction and handedness are the base's.
        for (t, b) in tan.iter().zip(base_tan) {
            assert_eq!(t[..3], b[..3]);
            assert_eq!(t[3].signum(), b[3].signum());
        }
        // The game's repeat: six layers, the fifth and sixth in the first texture coordinate,
        // which is the vertex's place in the cell over the repeat but for whole repeats (the
        // cell's origin is a whole number of them away), so the shader can work it out.
        let six = [layer(0.1), layer(0.2), layer(0.3), layer(0.4), layer(0.5), layer(0.6)];
        let six_refs: Vec<&[f32]> = six.iter().map(Vec::as_slice).collect();
        let origin = Vec2::new(-17.0 * 4096.0, 3.0 * 4096.0);
        let all = folded_mesh(&land, &stored(&land), 0, &six_refs, origin, 256.0).unwrap();
        let Some(VertexAttributeValues::Float32x2(uv)) = all.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!() };
        assert!(close([uv[v][0], uv[v][1], 0.0, 0.0], [0.5, 0.6, 0.0, 0.0]) && uv[0] == [0.0, 0.0]);
        let with_uv = quadrant_mesh(&land, &stored(&land), 0, None, origin, 256.0).unwrap();
        let Some(VertexAttributeValues::Float32x2(real)) = with_uv.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!() };
        let Some(VertexAttributeValues::Float32x3(pos)) = with_uv.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        for (uv, p) in real.iter().zip(pos) {
            let worked_out = [p[0] / 256.0, p[1] / 256.0];
            assert_eq!(((uv[0] - worked_out[0]).fract(), (uv[1] - worked_out[1]).fract()), (0.0, 0.0), "{uv:?} {p:?}");
        }
        // Fewer layers: the others' opacities are 0.
        let two = folded_mesh(&land, &stored(&land), 0, &refs[..2], Vec2::ZERO, 300.0).unwrap();
        let Some(VertexAttributeValues::Float32x2(uv_b)) = two.attribute(Mesh::ATTRIBUTE_UV_1) else { panic!() };
        let Some(VertexAttributeValues::Float32x4(tan)) = two.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        assert_eq!((uv_b[v], tan[v][3]), ([0.4, 0.0], TANGENT_SIGN));
    }

    #[test]
    fn the_shader_is_told_how_many_layers_and_which_have_normal_maps() {
        let image = Handle::<Image>::default();
        let layers = TerrainLayers { count: 3, diffuse0: Some(image.clone()), normal0: Some(image.clone()), diffuse1: Some(image.clone()), diffuse2: Some(image.clone()), normal2: Some(image.clone()), ..default() };
        let uniform = TerrainLayersUniform::from(&layers);
        assert_eq!((uniform.counts, uniform.tile.x), (UVec4::new(3, 0b101, 0, 0), 0.0));
        // No more layers than the mesh can carry: four with its own texture coordinate, six
        // when the shader works it out.
        assert_eq!(TerrainLayersUniform::from(&TerrainLayers { count: 9, ..default() }).counts.x, FOLDED_LAYERS_WITH_UV as u32);
        let six = TerrainLayersUniform::from(&TerrainLayers { count: 9, tile: 256.0, normal5: Some(image), ..default() });
        assert_eq!((six.counts, six.tile.x), (UVec4::new(FOLDED_LAYERS as u32, 1 << 5, 0, 0), 256.0));
    }

    // CPU mirror of terrain_weights.wesl, line for line (its constants are checked against
    // the file below).
    const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
    // A fixture gain for the height function's own tests (the default is 8, tested above).
    const HEIGHT_GAIN: f32 = 4.0;
    const HEIGHT_LIMIT: f32 = 0.25;
    const DEPTH: f32 = 0.10;

    fn luma(c: [f32; 3]) -> f32 {
        c[0] * LUMA[0] + c[1] * LUMA[1] + c[2] * LUMA[2]
    }

    fn terrain_height(texel: [f32; 3], reference: [f32; 3], gain: f32) -> f32 {
        (gain * (luma(texel) - luma(reference))).clamp(-HEIGHT_LIMIT, HEIGHT_LIMIT)
    }

    fn ordered_shares(a: [f32; 6], n: usize) -> [f32; 7] {
        let mut w = [0.0; 7];
        let mut remaining = 1.0;
        for i in (0..n.min(6)).rev() {
            let s = a[i].clamp(0.0, 1.0);
            w[i + 1] = s * remaining;
            remaining *= 1.0 - s;
        }
        w[0] = remaining;
        w
    }

    fn height_weights(w: [f32; 7], h: [f32; 7], depth: f32) -> [f32; 7] {
        let mut p = w.map(|x| x.max(0.0));
        let total: f32 = p.iter().sum();
        if total <= 0.0 {
            return [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        }
        let mut score = [0.0f32; 7];
        let mut highest = 0.0f32;
        for i in 0..7 {
            p[i] /= total;
            score[i] = p[i] + h[i].clamp(-p[i], p[i]);
            highest = highest.max(score[i]);
        }
        let d = depth.max(MIN_DEPTH);
        let q: [f32; 7] = std::array::from_fn(|i| p[i] * (d - (highest - score[i])).max(0.0));
        let sum: f32 = q.iter().sum();
        q.map(|x| x / sum)
    }

    fn close7(a: [f32; 7], b: [f32; 7], tol: f32) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
    }

    #[test]
    fn the_weights_shader_is_valid_wgsl_and_matches_the_mirror() {
        use wgpu::naga;
        let source = include_str!("terrain_weights.wesl");
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
        for line in [
            "const TERRAIN_LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);",
            "const TERRAIN_HEIGHT_LIMIT: f32 = 0.25;",
        ] {
            assert!(source.contains(line), "terrain_weights.wesl must declare `{line}` (the tests' mirror)");
        }
        // terrain.wesl takes those functions by name, only with the switch on.
        let terrain = include_str!("terrain.wesl");
        assert!(terrain.contains("@if(TERRAIN_HEIGHT_BLEND) import nvforever::terrain_weights::{terrain_height, terrain_ordered_shares, terrain_height_weights};"));
        for f in ["fn terrain_height(", "fn terrain_ordered_shares(", "fn terrain_height_weights("] {
            assert!(source.contains(f), "{f}");
        }
    }

    #[test]
    fn height_reference_sampling_is_valid_wgsl() {
        use wgpu::naga;
        // Validate the actual shared reference sampler, including its texture/sampler
        // arguments and explicit LOD. Runtime smoke covers the active binding form.
        // Line endings as checked out (CRLF on Windows) normalised before cutting the function out.
        let terrain = include_str!("terrain.wesl").replace("\r\n", "\n");
        // From the plain-value helper through the non-bindless reader that calls it; the WESL
        // conditions in front of each `fn` are left out of the cut.
        let start = terrain.find("fn height_reference_at(").unwrap();
        let reader = terrain.find("fn height_reference(").unwrap();
        let end = reader + terrain[reader..].find("\n}\n").unwrap() + 2;
        let source = format!("struct View {{ mip_bias: f32 }} @group(0) @binding(0) var<uniform> view: View;\n{}", terrain[start..end].replace("@if(TERRAIN_HEIGHT_BLEND) ", ""));
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    }

    #[test]
    fn ordered_shares_give_what_the_mix_chain_gives() {
        // Synthetic colours and opacities (no texture's values), the chain of terrain.wesl's
        // ordinary path against the weighted sum, within rounding.
        let mut seed = 0x2550u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        for case in 0..400 {
            let n = case % 7;
            let mut a = [0.0f32; 6];
            for x in a.iter_mut().take(n) {
                // Zeros, ones, tiny shares and the rest.
                *x = match (next() * 5.0) as u32 {
                    0 => 0.0,
                    1 => 1.0,
                    2 => 1e-4,
                    _ => next(),
                };
            }
            let colours: Vec<[f32; 3]> = (0..7).map(|_| [next(), next(), next()]).collect();
            let mut chain = colours[0];
            for (k, layer) in colours[1..=n].iter().enumerate() {
                for (x, l) in chain.iter_mut().zip(layer) {
                    *x = *x * (1.0 - a[k]) + l * a[k];
                }
            }
            let w = ordered_shares(a, n);
            assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5, "{w:?}");
            assert!(w.iter().all(|x| *x >= 0.0) && w[n + 1..].iter().all(|x| *x == 0.0), "{w:?}");
            for (c, want) in chain.iter().enumerate() {
                let sum: f32 = w.iter().zip(&colours).map(|(w, colour)| w * colour[c]).sum();
                assert!((sum - want).abs() < 1e-5, "case {case}: {sum} vs {want}");
            }
            // A sufficiently large depth converges to the ordered shares, even though
            // equal heights at the starting depth now sharpen unequal shares.
            assert!(close7(height_weights(w, [0.1; 7], 1e6), w, 2e-6));
        }
    }

    #[test]
    fn height_weights_keep_what_the_data_paints() {
        let heights = [0.2, -0.25, 0.25, 0.0, -0.1, 0.15, 0.05];
        for (paint, expected) in [
            ([0.0; 6], [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            ([1.0; 6], [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]),
            ([0.5, 0.7, 1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]),
        ] {
            assert_eq!(height_weights(ordered_shares(paint, 6), heights, DEPTH), expected);
        }
        let w = ordered_shares([0.4, 0.0, 1e-4, 0.6, 0.0, 0.3], 6);
        let f = height_weights(w, heights, DEPTH);
        for (fi, wi) in f.iter().zip(w) {
            assert!(*fi >= 0.0 && fi.is_finite(), "{f:?}");
            if wi == 0.0 {
                assert_eq!(*fi, 0.0);
            }
        }
        assert!((f.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert_eq!(height_weights([0.0; 7], heights, DEPTH), [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let half = [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0];
        let f = height_weights(half, [-0.05, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0], DEPTH);
        assert!(close7(f, [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0], 1e-6), "{f:?}");
        assert!(close7(height_weights(half, [0.0; 7], DEPTH), half, 1e-6));
        let unequal = [0.6, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(height_weights(unequal, [0.0; 7], DEPTH), [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        // Inactive heights have no vote, even at their extremes.
        assert_eq!(height_weights(half, [0.0, 0.0, 0.25, -0.25, 0.25, -0.25, 0.25], DEPTH), height_weights(half, [0.0; 7], DEPTH));
    }

    #[test]
    fn depth_weights_are_continuous_at_share_and_height_boundaries() {
        let epsilon = 1e-6;
        // Appearance/disappearance, equal shares, a score crossing the depth cutoff,
        // and a score crossing the share cap. No equal-height gate or special cases.
        for h in [[0.0; 7], [-0.05, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0], [0.25, -0.25, 0.0, 0.0, 0.0, 0.0, 0.0]] {
            for a in [0.0f32, 1e-4, 0.25, 0.45, 0.5, 0.55, 0.75, 1.0] {
                let weights = |a: f32| height_weights(ordered_shares([a, 0.0, 0.0, 0.0, 0.0, 0.0], 1), h, DEPTH);
                assert!(close7(weights((a - epsilon).max(0.0)), weights((a + epsilon).min(1.0)), 1e-4), "a={a} h={h:?}");
            }
        }
        let half = [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0];
        for h in [-0.1, 0.0, 0.1] {
            let weights = |h| height_weights(half, [0.0, h, 0.0, 0.0, 0.0, 0.0, 0.0], DEPTH);
            assert!(close7(weights(h - epsilon), weights(h + epsilon), 1e-4));
        }
    }

    #[test]
    fn large_depth_converges_to_ordered_shares() {
        let w = ordered_shares([0.4, 0.0, 1e-4, 0.6, 0.0, 0.3], 6);
        let h = [0.2, -0.25, 0.25, 0.0, -0.1, 0.15, 0.05];
        let error = |d| height_weights(w, h, d).iter().zip(w).map(|(a, b)| (a - b).abs()).sum::<f32>();
        assert!(error(100.0) < error(1.0));
        assert!(error(1e6) < 2e-6);
    }

    #[test]
    fn height_blend_parameters_parse_without_changing_the_environment() {
        assert_eq!(HeightBlendParams::default().gain, 8.0);
        assert_eq!(HeightBlendParams::default().depth, DEPTH);
        assert_eq!(parse_height_blend("0.10:8"), Ok(HeightBlendParams::default()));
        assert_eq!(parse_height_blend("0.10:8:global"), Ok(HeightBlendParams::default()));
        assert_ne!(parse_height_blend("0.10:4"), Ok(HeightBlendParams::default()));
        let local = parse_height_blend("0.05:8:local").unwrap();
        assert_eq!(local.uniform(), Vec4::new(0.05, 8.0, 1.0, 0.0));
        assert_eq!(parse_height_blend("1:0").unwrap().uniform(), Vec4::new(1.0, 0.0, 0.0, 0.0));
        assert_eq!(parse_height_blend("0.001:4").unwrap().depth, MIN_DEPTH);
        for invalid in ["", "0.1", "0.1:4:", "0.1:4:GLOBAL", "0.1:4:local:extra", "0:4", "0.0009:4", "0.00000001:8:local", "-1:4", "NaN:4", "inf:4", "0.1:NaN", "0.1:inf", "0.1:-1", ":4", "0.1:", "abc:4"] {
            assert!(parse_height_blend(invalid).is_err(), "{invalid}");
        }
        // The additional uniform does not enter TerrainLayersKey or enable the effect.
        // Six four-number fields since #3322 (parallax and each layer's depth).
        assert_eq!(TerrainLayersUniform::min_size().get(), 96);
        let off = TerrainLayers::default();
        assert!(!TerrainLayersKey::from(&off).height_blend);
        assert_eq!(TerrainLayersUniform::from(&off).height, height_blend_params().uniform());
    }

    #[test]
    fn a_texel_height_is_its_brightness_over_its_textures_mean() {
        assert_eq!(terrain_height([0.5; 3], [0.5; 3], HEIGHT_GAIN), 0.0);
        assert!((terrain_height([0.55; 3], [0.5; 3], HEIGHT_GAIN) - 0.2).abs() < 1e-6);
        assert_eq!(terrain_height([0.6; 3], [0.5; 3], 0.0), 0.0);
        assert!((terrain_height([0.55; 3], [0.5; 3], 1.0) - 0.05).abs() < 1e-6);
        assert_eq!(terrain_height([0.55; 3], [0.5; 3], 8.0), HEIGHT_LIMIT);
        // A bright texture does not stand higher for being bright: only the texel against
        // its own texture's mean counts.
        assert_eq!(terrain_height([0.9; 3], [0.9; 3], HEIGHT_GAIN), terrain_height([0.1; 3], [0.1; 3], HEIGHT_GAIN));
        assert_eq!(terrain_height([1.0; 3], [0.0; 3], HEIGHT_GAIN), HEIGHT_LIMIT);
        assert_eq!(terrain_height([0.0; 3], [1.0; 3], HEIGHT_GAIN), -HEIGHT_LIMIT);
        // Green counts most, blue least.
        assert!(terrain_height([0.0, 0.1, 0.0], [0.0; 3], HEIGHT_GAIN) > terrain_height([0.1, 0.0, 0.0], [0.0; 3], HEIGHT_GAIN));
    }

    #[test]
    fn height_blending_selects_the_pipeline_and_the_feature_bit() {
        let off = TerrainLayers { count: 2, ..default() };
        let on = TerrainLayers { count: 2, height_blend: true, ..default() };
        assert_eq!((TerrainLayersKey::from(&off), TerrainLayersKey::from(&on)), (TerrainLayersKey { height_blend: false, anti_tiling: false, surface_bake: false, parallax: false }, TerrainLayersKey { height_blend: true, anti_tiling: false, surface_bake: false, parallax: false }));
        assert_eq!((TerrainLayersUniform::from(&off).counts.w, TerrainLayersUniform::from(&on).counts.w), (0, FEATURE_HEIGHT_BLEND));
        assert_eq!(TerrainLayersUniform::from(&on).counts.truncate(), TerrainLayersUniform::from(&off).counts.truncate());
        let main = || RenderPipelineDescriptor { fragment: Some(bevy::render::render_resource::FragmentState { shader_defs: vec!["VERTEX_UVS_A".into()], ..default() }), ..default() };
        let has = |d: &RenderPipelineDescriptor| d.fragment.as_ref().unwrap().shader_defs.contains(&"TERRAIN_HEIGHT_BLEND".into());
        let mut d = main();
        height_blend_def(&mut d, TerrainLayersKey { height_blend: true, anti_tiling: false, surface_bake: false, parallax: false });
        assert!(has(&d));
        let mut d = main();
        height_blend_def(&mut d, TerrainLayersKey { height_blend: false, anti_tiling: false, surface_bake: false, parallax: false });
        assert!(!has(&d), "off: the pipeline there always was");
        let mut prepass = main();
        prepass.vertex.shader_defs.push("PREPASS_PIPELINE".into());
        height_blend_def(&mut prepass, TerrainLayersKey { height_blend: true, anti_tiling: false, surface_bake: false, parallax: false });
        assert!(!has(&prepass), "the prepass draws the same depth either way");
    }

    #[test]
    fn parallax_needs_a_height_map_and_selects_the_main_pass_only() {
        // #3322: the switch alone changes nothing; a held layer's height map and a depth do.
        let image = Handle::<Image>::default();
        let plain = TerrainLayers { count: 2, tile: 256.0, normal1: Some(image.clone()), ..default() };
        let asked = TerrainLayers { parallax: true, ..plain.clone() };
        assert!(!TerrainLayersKey::from(&asked).parallax, "no height map: the pipeline there always was");
        assert_eq!(TerrainLayersUniform::from(&asked).counts, TerrainLayersUniform::from(&plain).counts);
        let mapped = TerrainLayers { height1: Some(image.clone()), parallax_depths: [0.0, 0.04, 0.0, 0.0, 0.0, 0.0], ..asked.clone() };
        assert!(mapped.holds_heights() && TerrainLayersKey::from(&mapped).parallax);
        let u = TerrainLayersUniform::from(&mapped);
        assert_eq!(u.counts.y, 0b10 | 0b10 << HEIGHT_BITS_SHIFT, "layer 1's normal map and height map, each in its bits");
        assert_eq!(u.counts.w, FEATURE_PARALLAX);
        assert_eq!(u.parallax, Vec4::new(PARALLAX_SCALE, PARALLAX_LAYERS, PARALLAX_FADE.0, PARALLAX_FADE.1));
        assert_eq!((u.depths_low, u.depths_high), (Vec4::new(0.0, 0.04, 0.0, 0.0), Vec4::ZERO));
        assert_eq!(TerrainLayersUniform::from(&mapped).counts.truncate(), TerrainLayersUniform::from(&TerrainLayers { parallax: false, ..mapped.clone() }).counts.truncate(), "the switch moves only the feature bit");
        // Off with height maps held: the old pipeline, the height bits still recorded.
        let off = TerrainLayers { parallax: false, ..mapped.clone() };
        assert!(!TerrainLayersKey::from(&off).parallax && TerrainLayersUniform::from(&off).counts.w == 0);
        // A height map past the layers drawn, or a depth of 0: nothing to march.
        let past = TerrainLayers { count: 1, height3: Some(image.clone()), parallax_depths: [0.04; 6], parallax: true, tile: 256.0, ..default() };
        assert_eq!((past.height_bits(), past.holds_heights(), TerrainLayersKey::from(&past).parallax), (0, false, false));
        let flat = TerrainLayers { parallax_depths: [0.0; 6], ..mapped.clone() };
        assert!(!TerrainLayersKey::from(&flat).parallax);
        // The mesh terrain blend's bake copy keeps the unshifted surface.
        assert!(!TerrainLayersKey::from(&TerrainLayers { surface_bake: true, ..mapped.clone() }).parallax);
        // #3322 review: never with anti-tiling, whose taps the march does not share; a repeat
        // that cannot place anti-tiling leaves parallax on.
        let anti = TerrainLayers { anti_tiling: true, texture_repeat: 256.0, ..mapped.clone() };
        assert_eq!(TerrainLayersKey::from(&anti), TerrainLayersKey { height_blend: false, anti_tiling: true, surface_bake: false, parallax: false });
        assert_eq!(TerrainLayersUniform::from(&anti).counts.w, FEATURE_ANTI_TILING);
        assert!(TerrainLayersKey::from(&TerrainLayers { anti_tiling: true, texture_repeat: 0.0, ..mapped.clone() }).parallax);
        // The pipeline: the main pass only, and only with the key.
        let key = |parallax: bool, surface_bake: bool| TerrainLayersKey { height_blend: false, anti_tiling: false, surface_bake, parallax };
        let main = || RenderPipelineDescriptor { fragment: Some(bevy::render::render_resource::FragmentState { shader_defs: vec!["VERTEX_UVS_A".into()], ..default() }), ..default() };
        let has = |d: &RenderPipelineDescriptor| d.fragment.as_ref().unwrap().shader_defs.contains(&"TERRAIN_PARALLAX".into());
        let mut d = main();
        parallax_def(&mut d, key(true, false));
        assert!(has(&d));
        let mut d = main();
        parallax_def(&mut d, key(false, false));
        assert!(!has(&d), "off: the pipeline there always was");
        let mut d = main();
        parallax_def(&mut d, key(true, true));
        assert!(!has(&d), "never on a bake copy");
        let mut d = main();
        parallax_def(&mut d, TerrainLayersKey { anti_tiling: true, ..key(true, false) });
        assert!(!has(&d), "never with anti-tiling");
        let mut prepass = main();
        prepass.vertex.shader_defs.push("PREPASS_PIPELINE".into());
        parallax_def(&mut prepass, key(true, false));
        assert!(!has(&prepass), "the prepass and the shadows draw the unshifted surface");
        // Height blending's switch does not bring parallax in, nor the other way round.
        let mut d = main();
        height_blend_def(&mut d, key(true, false));
        assert!(!has(&d) && d.fragment.as_ref().unwrap().shader_defs.len() == 1);
    }

    /// #3322 review: terrain.wesl's `TerrainLayers` and `TerrainIndices` are the Rust side's
    /// uniform and bindless index table, field for field.
    #[test]
    fn the_shader_structs_match_the_rust_layouts() {
        let terrain = include_str!("terrain.wesl").replace("\r\n", "\n");
        let fields = |name: &str| -> Vec<(String, String)> {
            let body = terrain.split(&format!("struct {name} {{\n")).nth(1).unwrap_or_else(|| panic!("struct {name}")).split("\n}").next().unwrap();
            body.lines()
                .map(|l| l.split("//").next().unwrap().trim())
                .filter(|l| !l.is_empty())
                .map(|l| {
                    let (n, t) = l.trim_end_matches(',').split_once(':').unwrap();
                    (n.trim().to_string(), t.trim().to_string())
                })
                .collect()
        };
        let layers = fields("TerrainLayers");
        let names: Vec<&str> = layers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["counts", "tile", "height", "parallax", "depths_low", "depths_high"], "TerrainLayersUniform's fields, in order");
        assert!(layers[0].1 == "vec4<u32>" && layers[1..].iter().all(|(_, t)| t == "vec4<f32>"), "{layers:?}");
        assert_eq!(TerrainLayersUniform::min_size().get(), 16 * layers.len() as u64);
        // The index table: one u32 per index of range(50..71) (the data, then every texture).
        let indices = fields("TerrainIndices");
        assert_eq!(indices.len(), 71 - 50);
        assert!(indices.iter().all(|(_, t)| t == "u32"));
        let heights: Vec<&str> = indices.iter().map(|(n, _)| n.as_str()).filter(|n| n.starts_with("height")).collect();
        assert_eq!(heights, ["height0", "height1", "height2", "height3", "height4", "height5", "height_base"], "bindings 64 to 70, last");
        assert!(terrain.contains("    height0: u32,   // 64") && terrain.contains("    height5: u32,   // 69"));
        assert!(terrain.contains("    height_base: u32, // 70"));
        assert!(include_str!("terrain.rs").contains("index_table(range(50..71), binding(100))"));
    }

    #[test]
    fn height_maps_take_their_layers_slots_and_keep_their_own_depths() {
        // #3322: the layers' order; a layer without a height map keeps its slot empty and
        // depth 0; each layer keeps its own set's depth, the deepest scales the march.
        let a: Handle<Image> = uuid_handle!("5a3e2b5c-3322-4c1e-8f00-000000000001");
        let b: Handle<Image> = uuid_handle!("5a3e2b5c-3322-4c1e-8f00-000000000002");
        let (slots, depths) = layer_height_slots(vec![None, Some((a.clone(), 0.02)), None, Some((b.clone(), 0.05))]);
        assert_eq!(slots, [None, Some(a.clone()), None, Some(b.clone()), None, None]);
        assert_eq!(depths, [0.0, 0.02, 0.0, 0.05, 0.0, 0.0]);
        let (slots, depths) = layer_height_slots(vec![None, None]);
        assert!(slots.iter().all(Option::is_none) && depths == [0.0; 6], "no height map: nothing to march");
        let [height0, height1, height2, height3, height4, height5] = slots_of(&a, &b);
        let l = TerrainLayers { count: 4, tile: 256.0, parallax_depths: depths_of(), height0, height1, height2, height3, height4, height5, ..default() };
        assert_eq!(l.height_bits(), 0b1010, "bit k for layer k");
        assert_eq!((l.layer_depths(), l.parallax_scale()), ([0.0, 0.02, 0.0, 0.05, 0.0, 0.0], PARALLAX_SCALE));
        let u = TerrainLayersUniform::from(&l);
        assert_eq!((u.parallax.x, u.depths_low, u.depths_high), (PARALLAX_SCALE, Vec4::new(0.0, 0.02, 0.0, 0.05), Vec4::ZERO));
        // The base's (#3846): its own bit and depth, clamped as a layer's, alone enough to march.
        let based = TerrainLayers { height_base: Some(a.clone()), base_depth: 0.04, ..l.clone() };
        assert_eq!((based.height_bits(), TerrainLayersUniform::from(&based).depths_high, TerrainLayersUniform::from(&based).counts.y >> HEIGHT_BITS_SHIFT), (0b100_1010, Vec4::new(0.0, 0.0, 0.04, 0.0), 0b100_1010));
        let alone = TerrainLayers { height_base: Some(a.clone()), base_depth: 9.0, parallax: true, texture_repeat: 256.0, ..default() };
        assert_eq!((alone.height_bits(), alone.marched_base_depth(), alone.holds_heights(), TerrainLayersKey::from(&alone).parallax), (1 << BASE_HEIGHT, PARALLAX_SCALE, true, true));
        assert!(!TerrainLayers { base_depth: 0.04, ..default() }.holds_heights(), "a depth without a map is none");
        // Only the layers drawn count: with three, the fourth's map and depth are left out.
        let three = TerrainLayers { count: 3, ..l.clone() };
        assert_eq!((three.height_bits(), three.layer_depths()), (0b10, [0.0, 0.02, 0.0, 0.0, 0.0, 0.0]));
        // Without the shader-worked coordinate four layers are drawn, so a fifth's map is not.
        let carried = TerrainLayers { count: 6, tile: 0.0, height4: Some(a.clone()), parallax_depths: [0.05; 6], ..default() };
        assert_eq!((carried.height_bits(), carried.holds_heights()), (0, false));

        fn slots_of(a: &Handle<Image>, b: &Handle<Image>) -> [Option<Handle<Image>>; 6] {
            layer_height_slots(vec![None, Some((a.clone(), 0.02)), None, Some((b.clone(), 0.05))]).0
        }
        fn depths_of() -> [f32; 6] {
            [0.0, 0.02, 0.0, 0.05, 0.0, 0.0]
        }
    }

    /// A CPU statement of terrain.wesl's march (`parallax_shift` over `parallax_depth_at`:
    /// the vendored parallax_mapping.wesl's linear search, grazing clamp and relief search)
    /// on the march's scale `scale`, for a depth function already on that scale.
    fn parallax_march(depth_at: &dyn Fn(Vec2) -> f32, vt: Vec3, scale: f32) -> Vec2 {
        parallax_march_with(depth_at, vt, scale, PARALLAX_LAYERS, true)
    }

    /// The march with `layers` and either the relief search (`relief`, terrain.wesl now) or
    /// the round-1 shader's occlusion refinement and unclamped divisor (kept here only to
    /// show what the round-1 pictures showed).
    fn parallax_march_with(depth_at: &dyn Fn(Vec2) -> f32, vt: Vec3, scale: f32, layers: f32, relief: bool) -> Vec2 {
        let steepness = if relief { vt.z.abs() } else { vt.z.abs().max(1e-3) };
        let layer_count = layers + (1.0 - layers) * steepness;
        let layer_depth = 1.0 / layer_count;
        let divisor = if relief { steepness.max(PARALLAX_GRAZING) } else { steepness };
        let mut delta = scale * layer_depth * Vec2::new(vt.x, -vt.y) / divisor;
        let (mut shift, mut current, mut depth) = (Vec2::ZERO, 0.0, depth_at(Vec2::ZERO));
        if relief && depth <= 0.0 {
            return Vec2::ZERO;
        }
        let mut i = 0;
        while depth > current && i <= layer_count as i32 {
            current += layer_depth;
            shift += delta;
            depth = depth_at(shift);
            i += 1;
        }
        if relief {
            delta *= 0.5;
            let mut step_depth = 0.5 * layer_depth;
            shift -= delta;
            current -= step_depth;
            for _ in 0..PARALLAX_RELIEF_STEPS {
                depth = depth_at(shift);
                delta *= 0.5;
                step_depth *= 0.5;
                if depth > current {
                    shift += delta;
                    current += step_depth;
                } else {
                    shift -= delta;
                    current -= step_depth;
                }
            }
            return shift;
        }
        let previous = shift - delta;
        let next_depth = depth - current;
        let previous_depth = depth_at(previous) - current + layer_depth;
        let gap = next_depth - previous_depth;
        let weight = if gap.abs() > 1e-6 { next_depth / gap } else { 0.0 };
        shift.lerp(previous, weight)
    }

    /// A view `elevation` degrees above flat ground, looking along +u (east): the vector
    /// towards the surface in the tangent frame.
    fn view_at(elevation: f32) -> Vec3 {
        let e = elevation.to_radians();
        Vec3::new(e.cos(), 0.0, -e.sin())
    }

    /// The ground pack's generated height map (nvf-assets pbr_field/ground.rs: 256 texels a
    /// repeat, a ridge 8 texels wide every 64, high on the ridges and low elsewhere) as the
    /// march's depth for one fully painted layer at the scale's depth: 0 on a ridge, 1 off.
    fn ground_pack_depth(p: Vec2) -> f32 {
        let on = |c: f32| (c * 256.0).rem_euclid(64.0) < 8.0;
        if on(p.x) || on(p.y) { 0.0 } else { 1.0 }
    }

    #[test]
    fn the_march_moves_a_sunk_layer_by_its_depth_over_the_views_slope() {
        // #3322 round 2: the shift is in texture-coordinate units (one texture repeat, 256
        // world units on the landscape), so a layer sunk by the full 0.08 seen 20 degrees above
        // the ground moves 0.08 / tan(20 degrees) = 0.2198 of a repeat away from the camera
        // (+u here), about 56 texels of the ground pack's 256 and 225 of a 1024 texture; the
        // relief search ends within a thirty-second of a layer's step of it.
        let floor = |_: Vec2| 1.0;
        let shift = parallax_march(&floor, view_at(20.0), PARALLAX_SCALE);
        let want = PARALLAX_SCALE / 20f32.to_radians().tan();
        assert!((shift.x - want).abs() < 2e-3 && shift.y == 0.0, "{shift} against {want}");
        assert!((shift.x * 256.0 - 56.3).abs() < 0.6, "{} texels of 256", shift.x * 256.0);
        // The frame's signs: towards +B (Bevy's bitangent, -v on the landscape) moves -v.
        let sideways = parallax_march(&floor, Vec3::new(0.0, 20f32.to_radians().cos(), -20f32.to_radians().sin()), PARALLAX_SCALE);
        assert!(sideways.x == 0.0 && (sideways.y + want).abs() < 2e-3, "{sideways}");
        // At the surface already (a ridge's top): nothing, not a step backwards.
        assert_eq!(parallax_march(&|_| 0.0, view_at(20.0), PARALLAX_SCALE), Vec2::ZERO);
    }

    #[test]
    fn the_march_is_bounded_at_grazing_views() {
        // #3322 round 2: the step's divisor stops at PARALLAX_GRAZING, as on NIF shapes, so a
        // view along the ground neither throws the samples away (unclamped, a view 3 degrees
        // above it moved a sunk layer 1.5 repeats) nor gives non-finite coordinates.
        let floor = |_: Vec2| 1.0;
        let limit = PARALLAX_SCALE / PARALLAX_GRAZING;
        for elevation in [8.0, 3.0, 0.5, 0.0] {
            let shift = parallax_march(&floor, view_at(elevation), PARALLAX_SCALE);
            assert!(shift.is_finite() && shift.length() <= limit * 1.001, "{elevation} degrees: {shift}");
        }
        let unclamped = parallax_march_with(&floor, view_at(3.0), PARALLAX_SCALE, 8.0, false);
        assert!(unclamped.length() > 1.0, "round 1 at 3 degrees: {unclamped}");
        let shader = include_str!("terrain.wesl").replace("\r\n", "\n");
        assert!(shader.contains(&format!("const PARALLAX_GRAZING: f32 = {PARALLAX_GRAZING};")));
        assert!(shader.contains(&format!("const PARALLAX_RELIEF_STEPS: u32 = {PARALLAX_RELIEF_STEPS}u;")));
    }

    /// Where along +u from `u0` the ray (seen `elevation` degrees above the ground) first goes
    /// below the ground pack's surface, found by small steps: the shift a perfect march would
    /// return, `None` when the ray stays above it within the full depth.
    fn first_crossing(u0: f32, elevation: f32) -> Option<f32> {
        let e = elevation.to_radians();
        let full = PARALLAX_SCALE * e.cos() / e.sin().max(PARALLAX_GRAZING);
        (0..=4000).map(|k| k as f32 / 4000.0 * full).find(|&s| ground_pack_depth(Vec2::new(u0 + s, 0.5 / 256.0 + 16.0 / 256.0)) <= s / full + 1e-6)
    }

    #[test]
    fn the_march_meets_the_ground_packs_ridges_where_the_ray_does() {
        // #3322 round 2, the cause of the round-1 pictures' doubled, stair-stepped bands:
        // over the ground pack's sharp ridges the occlusion refinement put each pixel's hit
        // between two layers' guesses, so a ridge's wall showed once per layer, and at 8
        // layers a step (0.039 of a repeat at 20 degrees) was longer than a ridge is wide
        // (0.031), so rays stepped over ridges. Now each pixel ends where its ray first meets
        // the surface, within a small part of a texel, along a scanline of start points
        // across two ridge periods (v kept between ridges, so only the ridges across +u count).
        let v = 0.5 / 256.0 + 16.0 / 256.0;
        let misses = |elevation: f32, march: &dyn Fn(Vec2) -> Vec2| {
            (0..512)
                .filter(|&k| {
                    let u0 = k as f32 / 512.0 * 0.5;
                    let got = march(Vec2::new(u0, v)).x;
                    first_crossing(u0, elevation).is_none_or(|want| (got - want).abs() > 1.5 / 256.0)
                })
                .count()
        };
        for elevation in [45.0, 30.0, 20.0, 15.0, 12.0] {
            let now = misses(elevation, &|p| parallax_march(&|s| ground_pack_depth(p + s), view_at(elevation), PARALLAX_SCALE));
            assert_eq!(now, 0, "{elevation} degrees: pixels whose hit is not where the ray meets the surface");
        }
        let round_one = misses(20.0, &|p| parallax_march_with(&|s| ground_pack_depth(p + s), view_at(20.0), PARALLAX_SCALE, 8.0, false));
        assert!(round_one > 50, "the round-1 march misplaced {round_one} of 512 at 20 degrees");
    }

    /// The blended depth on the march's scale, as `parallax_depth_at` adds it from a
    /// material's uniform: each layer's share x its depth over the scale x (1 - height), and the
    /// base's after them (#3846).
    fn parallax_depth_from(u: &TerrainLayersUniform, shares: [f32; 7], heights: [f32; 7]) -> f32 {
        let depths = [u.depths_low.x, u.depths_low.y, u.depths_low.z, u.depths_low.w, u.depths_high.x, u.depths_high.y, u.depths_high.z];
        (0..7).filter(|&k| shares[k] > 0.0 && depths[k] > 0.0).map(|k| shares[k] * depths[k] / u.parallax.x * (1.0 - heights[k])).sum()
    }

    /// #3846: a base marches by its own share and depth as a layer of the same height map would.
    #[test]
    fn a_base_moves_as_a_layer_of_the_same_map_would() {
        let image = Handle::<Image>::default();
        let layer = TerrainLayersUniform::from(&TerrainLayers { count: 1, tile: 256.0, parallax: true, height0: Some(image.clone()), parallax_depths: [0.03, 0.0, 0.0, 0.0, 0.0, 0.0], ..default() });
        let base = TerrainLayersUniform::from(&TerrainLayers { parallax: true, height_base: Some(image), base_depth: 0.03, ..default() });
        assert_eq!((layer.parallax.x, base.parallax.x), (PARALLAX_SCALE, PARALLAX_SCALE));
        let ridges = |p: Vec2| 0.5 + 0.5 * (p.x * 900.0).sin().powi(15);
        let tilted = Vec3::new(0.6, 0.0, -0.8);
        for share in [1.0, 0.4] {
            let a = parallax_march(&|p| parallax_depth_from(&layer, [share, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [ridges(p); 7]), tilted, layer.parallax.x);
            let b = parallax_march(&|p| parallax_depth_from(&base, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, share], [ridges(p); 7]), tilted, base.parallax.x);
            assert_eq!(a, b, "share {share}");
            assert_ne!(b, Vec2::ZERO);
        }
        let terrain = include_str!("terrain.wesl").replace("\r\n", "\n");
        assert!(terrain.contains("    if w[0] > 0.0 && scale[6] > 0.0 {\n        depth += w[0] * scale[6] * (1.0 - layer_height(6u, instance_index, uv, footprint));\n    }\n"));
        assert!(terrain.contains("    var relief = w[0] > 0.0 && scale[6] > 0.0;\n"));
    }

    #[test]
    fn a_layer_moves_as_far_whatever_else_its_quadrant_holds() {
        // #3322 review: a 0.02 layer beside a quadrant edge, its neighbour quadrant also
        // holding a 0.08 layer that is not painted there (share 0): the same steps and the
        // same shift on both sides, over a varying height field (narrow ridges a coarser grid
        // would step over), as each layer keeps its own depth on the one fixed scale.
        let image = Handle::<Image>::default();
        let alone = TerrainLayers { count: 1, tile: 256.0, parallax: true, height0: Some(image.clone()), parallax_depths: [0.02, 0.0, 0.0, 0.0, 0.0, 0.0], ..default() };
        let beside = TerrainLayers { count: 2, height1: Some(image.clone()), parallax_depths: [0.02, 0.08, 0.0, 0.0, 0.0, 0.0], ..alone.clone() };
        let (ua, ub) = (TerrainLayersUniform::from(&alone), TerrainLayersUniform::from(&beside));
        assert_eq!((ua.parallax.x, ub.parallax.x), (PARALLAX_SCALE, PARALLAX_SCALE), "one step grid for both quadrants");
        assert_eq!(PARALLAX_SCALE, 0.08, "the scalar range's end (fnv_pbr::PARALLAX_DEPTH_RANGE)");
        let tilted = Vec3::new(0.6, 0.0, -0.8);
        // Narrow ridges along U in the 0.02 layer's height map; the 0.08 layer high and flat.
        let ridges = |p: Vec2| [0.5 + 0.5 * (p.x * 900.0).sin().powi(15), 0.9, 0.0, 0.0, 0.0, 0.0, 0.0];
        for share in [1.0, 0.6, 0.25] {
            let shares = [share, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            let a = parallax_march(&|p| parallax_depth_from(&ua, shares, ridges(p)), tilted, ua.parallax.x);
            let b = parallax_march(&|p| parallax_depth_from(&ub, shares, ridges(p)), tilted, ub.parallax.x);
            assert!(a.length() > 0.0 && a == b, "share {share}: {a} and {b}");
        }
        // A set deeper than the scale is held to it.
        let deep = TerrainLayers { parallax_depths: [0.2, 0.0, 0.0, 0.0, 0.0, 0.0], ..alone.clone() };
        assert_eq!(deep.layer_depths()[0], PARALLAX_SCALE);
    }

    #[test]
    fn the_ground_pack_covers_every_named_texture_and_the_default_once() {
        let list = ground_pack_list([Some(r"Landscape\Dirt02.dds".to_string()), None, Some(r"Landscape\Dirt02.dds".to_string()), Some(r"landscape\rocks01.dds".to_string())].into_iter());
        let mut want = vec![DEFAULT_DIFFUSE.to_string(), r"Landscape\Dirt02.dds".to_string(), r"landscape\rocks01.dds".to_string()];
        want.sort();
        assert_eq!(list, want, "each named texture once, a record without one left out, the default added");
        assert_eq!(ground_pack_list(std::iter::empty()), [DEFAULT_DIFFUSE.to_string()], "a load order without LTEX still covers the default ground");
    }

    #[test]
    fn the_ground_cover_counts_the_height_mapped_folded_share_and_what_hides_it() {
        let cells = (QUAD_SIDE * QUAD_SIDE) as u16;
        let layer = |o: Option<f32>| alpha_grid(&Layer { quadrant: 0, texture: Some(1), layer: 0, base: false, alpha: o.map_or_else(Vec::new, |o| (0..cells).map(|p| (p, o)).collect()) });
        let (full, half, none) = (|_: i16, _: u32| layer(Some(1.0)), |_: i16, _: u32| layer(Some(0.5)), |_: i16, _: u32| layer(None));
        // Two folded layers, both with height maps, the top one painted fully: all of it.
        assert_eq!(cover_shares(&[half(0, 1), full(1, 2)], 2, 0b11, 3.0, 4.0), (1.0, 0.0, 0.0));
        // Only the lower one has a height map: the top one hides it.
        assert_eq!(cover_shares(&[half(0, 1), full(1, 2)], 2, 0b01, 3.0, 4.0), (0.0, 0.0, 0.0));
        // A half-painted top layer without a height map over a full one with: half of it.
        assert_eq!(cover_shares(&[full(0, 1), half(1, 2)], 2, 0b01, 3.0, 4.0), (0.5, 0.0, 0.0));
        // A layer past the folded ones (drawn on its own, unshifted) painted fully hides it all.
        assert_eq!(cover_shares(&[full(0, 1), full(1, 2)], 1, 0b1, 3.0, 4.0), (1.0, 0.0, 1.0));
        // Unpainted layers and the base alone: nothing to move.
        assert_eq!(cover_shares(&[none(0, 1)], 1, 0b1, 3.0, 4.0), (0.0, 0.0, 0.0));
        // The base's part, where it has a height map (#3846): what the layers leave.
        assert_eq!(cover_shares(&[half(0, 1)], 1, 0b1 | 1 << BASE_HEIGHT, 3.0, 4.0), (0.5, 0.5, 0.0));
        assert_eq!(cover_shares(&[], 0, 1 << BASE_HEIGHT, 3.0, 4.0), (0.0, 1.0, 0.0));
        assert_eq!(cover_shares(&[], 0, 0, 3.0, 4.0), (0.0, 0.0, 0.0));
        // Between vertices the opacity is interpolated.
        let ramp = alpha_grid(&Layer { quadrant: 0, texture: Some(1), layer: 0, base: false, alpha: vec![(0, 0.0), (1, 1.0), (QUAD_SIDE as u16, 0.0), (QUAD_SIDE as u16 + 1, 1.0)] });
        assert_eq!(opacity_at(&ramp, 0.25, 0.5), 0.25);
        // On the stored triangles, not bilinear: one corner painted (north-west) is seen only
        // in the north-west triangle of its square.
        let corner = alpha_grid(&Layer { quadrant: 0, texture: Some(1), layer: 0, base: false, alpha: vec![(QUAD_SIDE as u16, 1.0)] });
        assert_eq!((opacity_at(&corner, 0.75, 0.25), opacity_at(&corner, 0.25, 0.75)), (0.0, 0.5), "bilinear would give 0.1875 for both");
        for (r, c) in [(0usize, 0usize), (3, 7), (15, 15), (16, 16)] {
            let w = stored_weights(r * 4 + 1, c * 4 + 3, 4, QUAD_SIDE);
            let want: f32 = w.iter().map(|&(k, w)| w * corner[k]).sum();
            assert!((opacity_at(&corner, c as f32 + 0.75, r as f32 + 0.25) - want).abs() < 1e-6, "the meshes' rule at ({r}, {c})");
        }
        let c = GroundCover { grid: (1, 2), quadrant: 3, folded: 6, with_heights: 6, over: 2, material: true, base_height: false, height_share: 0.8, layer_share: 0.8, over_cover: 0.25 };
        assert!((c.shifted() - 0.6).abs() < 1e-6);
        // The base's part counts for what moves, not for the layers' (#3846).
        let based = GroundCover { base_height: true, height_share: 1.0, ..c.clone() };
        assert!((based.shifted() - 0.75).abs() < 1e-6 && (based.layers_shifted() - 0.6).abs() < 1e-6);
        assert!(based.describe().contains("6 folded layers (6 with height maps, the base too), 2 drawn over them; shifted share 0.75"), "{}", based.describe());
        assert_eq!(GroundCover { material: false, ..c.clone() }.shifted(), 0.0, "no folded material: the march never runs");
        assert!(c.describe().contains("6 folded layers (6 with height maps), 2 drawn over them; shifted share 0.60"), "{}", c.describe());
    }

    #[test]
    fn the_parallax_switch_flips_only_the_materials_with_height_maps() {
        use crate::graphics::{Graphics, Preset};
        let mut app = App::new();
        app.insert_resource(Graphics::preset(Preset::Vanilla));
        app.insert_resource(Assets::<TerrainMaterial>::default());
        app.add_systems(Update, sync_parallax);
        let image = Handle::<Image>::default();
        let mapped = TerrainMaterial { base: StandardMaterial::default(), extension: TerrainLayers { count: 1, tile: 256.0, height0: Some(image), parallax_depths: [0.04; 6], ..default() } };
        let plain = TerrainMaterial { base: StandardMaterial::default(), extension: TerrainLayers { count: 1, ..default() } };
        let a = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(mapped);
        let b = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(plain);
        app.update();
        let ext = |app: &App, h: &Handle<TerrainMaterial>| app.world().resource::<Assets<TerrainMaterial>>().get(h).unwrap().extension.clone();
        assert!(!TerrainLayersKey::from(&ext(&app, &a)).parallax && !TerrainLayersKey::from(&ext(&app, &b)).parallax, "off by default");
        assert!(!Graphics::preset(Preset::Enhanced).terrain_parallax, "and in the enhanced preset");
        app.world_mut().resource_mut::<Graphics>().terrain_parallax = true;
        app.update();
        assert!(TerrainLayersKey::from(&ext(&app, &a)).parallax, "a material with a height map takes the march");
        assert!(!ext(&app, &b).parallax, "one without is not touched (its key cannot change)");
        app.world_mut().resource_mut::<Graphics>().terrain_parallax = false;
        app.update();
        assert!(!TerrainLayersKey::from(&ext(&app, &a)).parallax);
        assert!(!parallax_wanted(None));
    }

    /// #3486: a layer's alpha height takes the layer's height slot and depth and says so, and
    /// goes again, the authored ones beside it untouched; the bake copy holds neither.
    #[test]
    fn an_alpha_height_fills_a_layers_slot_and_goes_with_the_switch() {
        let mut images = Assets::<Image>::default();
        let (authored, alpha) = (images.add(Image::default()), images.add(Image::default()));
        let mut l = TerrainLayers { count: 3, tile: 256.0, texture_repeat: 256.0, height1: Some(authored.clone()), parallax: true, ..default() };
        l.parallax_depths[1] = 0.08;
        l.set_alpha_height(2, Some((alpha.clone(), nvf_assets::ALPHA_HEIGHT_DEPTH)));
        assert_eq!((l.height_bits(), l.alpha_heights, l.parallax_depths[1], l.parallax_depths[2]), (0b110, 0b100, 0.08, nvf_assets::ALPHA_HEIGHT_DEPTH));
        assert_eq!((l.height1.clone(), l.height2.clone()), (Some(authored.clone()), Some(alpha)));
        assert!(TerrainLayersKey::from(&l).parallax);
        l.set_alpha_height(2, None);
        assert_eq!((l.height_bits(), l.alpha_heights, l.parallax_depths[2], l.height2.is_none()), (0b010, 0, 0.0, true));
        assert_eq!(l.height1, Some(authored));
        // Alone it is enough for the march, and nothing is left without it.
        let mut only = TerrainLayers { count: 1, tile: 256.0, texture_repeat: 256.0, parallax: true, ..default() };
        only.set_alpha_height(0, Some((images.add(Image::default()), nvf_assets::ALPHA_HEIGHT_DEPTH)));
        assert!(TerrainLayersKey::from(&only).parallax);
        only.set_alpha_height(0, None);
        assert!(!TerrainLayersKey::from(&only).parallax && !only.holds_heights());
    }

    /// #3486, the switch over materials in place: on, a layer without a height takes its
    /// diffuse's alpha (an authored one beside it untouched) and the material marches; off, it
    /// lets go; a layer whose alpha does not vary stays flat; the edits are all noted.
    #[test]
    fn the_parallax_switch_gives_and_takes_the_alpha_heights_in_place() {
        use crate::graphics::Graphics;
        let dir = std::env::temp_dir().join(format!("nvf-alpha-heights-{}", std::process::id()));
        let land = dir.join("textures").join("landscape");
        std::fs::create_dir_all(&land).unwrap();
        // A 8x4 DXT5 of two blocks whose alphas are 200 and 200 - `by`.
        let dds = |by: u8| {
            let mut b = vec![0u8; 128];
            b[..4].copy_from_slice(b"DDS ");
            for (at, v) in [(4, 124u32), (8, 0x1007), (12, 4), (16, 8), (76, 32), (80, 4), (108, 0x1000)] {
                b[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
            b[84..88].copy_from_slice(b"DXT5");
            for a in [200, 200 - by] {
                b.extend_from_slice(&[a, a, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
            }
            b
        };
        std::fs::write(land.join("deep.dds"), dds(100)).unwrap();
        std::fs::write(land.join("flat.dds"), dds(0)).unwrap();
        // A DXT1, and a DXT5 with an authored set that has no height.
        let mut plain = dds(0);
        plain[84..88].copy_from_slice(b"DXT1");
        plain.truncate(128 + 16);
        std::fs::write(land.join("plain.dds"), plain).unwrap();
        std::fs::write(land.join("coloured.dds"), dds(100)).unwrap();
        let pbr = dir.join("textures").join("pbr").join("landscape");
        std::fs::create_dir_all(&pbr).unwrap();
        std::fs::write(pbr.join("coloured_orm.dds"), dds(0)).unwrap();
        let mut app = App::new();
        app.init_resource::<Assets<TerrainMaterial>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<NifMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
            .init_resource::<nvf_assets::TextureCache>()
            .init_resource::<nvf_assets::ModelCache>()
            .init_resource::<TerrainCache>()
            .insert_resource(TerrainSettings { tile: 256.0 })
            .init_resource::<AlphaHeightSync>()
            .init_resource::<ParallaxEdits>()
            .insert_resource(nvf_assets::GameData::new(std::sync::Arc::new(fnv_vfs::Vfs::open(&dir, &[]).unwrap()), std::sync::Arc::new(fnv_nif::Schema::default())))
            .insert_resource(Graphics { terrain_parallax: false, ..Graphics::preset(crate::graphics::Preset::Vanilla) });
        app.add_systems(Update, (sync_parallax, alpha_heights_system()).chain());
        let authored = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
        let mut e = TerrainLayers { count: 3, tile: 256.0, texture_repeat: 256.0, height0: Some(authored.clone()), ..default() };
        e.parallax_depths[0] = 0.08;
        let both = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(TerrainMaterial { base: default(), extension: e });
        let alone = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(TerrainMaterial { base: default(), extension: TerrainLayers { count: 2, tile: 256.0, texture_repeat: 256.0, ..default() } });
        // A quadrant of its base alone (#3846).
        let base = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(TerrainMaterial { base: default(), extension: TerrainLayers { texture_repeat: 256.0, ..default() } });
        let path = |p: &str| format!("landscape\\{p}.dds");
        {
            let mut cache = app.world_mut().resource_mut::<TerrainCache>();
            cache.folded.insert((None, vec![path("set"), path("deep"), path("flat")], true), both.clone());
            cache.folded.insert((None, vec![path("flat"), path("deep")], true), alone.clone());
            cache.folded.insert((Some(path("deep")), Vec::new(), false), base.clone());
        }
        let state = |app: &App, h: &Handle<TerrainMaterial>| {
            let e = &app.world().resource::<Assets<TerrainMaterial>>().get(h).unwrap().extension;
            (e.height_bits(), e.alpha_heights, TerrainLayersKey::from(e).parallax)
        };
        let set = |app: &mut App, on: bool| {
            app.world_mut().resource_mut::<Graphics>().terrain_parallax = on;
            app.world_mut().resource_mut::<ParallaxEdits>().0.clear();
            for _ in 0..8 {
                app.update();
            }
        };
        set(&mut app, false);
        assert_eq!((state(&app, &both), state(&app, &alone)), ((0b001, 0, false), (0, 0, false)), "off: nothing read");
        for _ in 0..2 {
            set(&mut app, true);
            assert_eq!((state(&app, &both), state(&app, &alone)), ((0b011, 0b010, true), (0b010, 0b010, true)));
            assert_eq!(state(&app, &base), (1 << BASE_HEIGHT, 1 << BASE_HEIGHT, true), "the base's alpha");
            assert_eq!((app.world().resource::<ParallaxEdits>().0[&both.id()], app.world().resource::<ParallaxEdits>().0[&alone.id()]), (2, 1), "each edit noted: the flip and the height, the height alone");
            assert_eq!(app.world().resource::<Assets<TerrainMaterial>>().get(&both).unwrap().extension.height0, Some(authored.clone()));
            set(&mut app, false);
            assert_eq!((state(&app, &both), state(&app, &alone)), ((0b001, 0, false), (0, 0, false)), "and gone again");
            assert_eq!(state(&app, &base), (0, 0, false));
        }
        // Which bases alone take the folded material (#3846): a BC3 diffuse without a set does,
        // varying alpha or not; a DXT1 one does not, nor a BC3 one whose set has no height, nor
        // none. Whatever the switch.
        use bevy::ecs::system::RunSystemOnce;
        for on in [true, false] {
            set(&mut app, on);
            app.world_mut().resource_mut::<TerrainCache>().base_marches.clear();
            let found = app
                .world_mut()
                .run_system_once(|mut tm: TerrainMaterials, mut assets: ModelCtx| {
                    let mut ctx = TerrainCtx { tm: &mut tm, assets: &mut assets };
                    ["deep", "flat", "plain", "coloured", "missing"].map(|p| ctx.base_may_march(Some(&format!("landscape\\{p}.dds"))))
                })
                .unwrap();
            assert_eq!(found, [true, true, false, false, false], "switch {on}");
        }
        assert_eq!(app.world().resource::<TerrainCache>().base_marches.len(), 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parallax_moves_every_read_by_one_shift_and_none_head_on() {
        // The shader's text: the coordinate moved once before the surface set-up (never with
        // anti-tiling, whose place is the world's); the march's step proportional to the view
        // vector's tangent part (none head-on), along the view's own ray; each layer's depth
        // its own over the common scale, a missing map and the base adding none; the weights
        // worked out once at the plain place and shared by the march and the drawn blend; the
        // drawn reads at the moved place with the plain place's gradients.
        let terrain = include_str!("terrain.wesl").replace("\r\n", "\n");
        let shift = terrain.find("        in.uv += parallax_shift(").expect("the coordinate moved");
        let setup = terrain.find("    @if(TERRAIN_PARALLAX && !TERRAIN_ANTI_TILING) var pbr_input = terrain_pbr_begin(in, is_front, standard_material(in.instance_index));").expect("parallax's own set-up");
        assert!(shift < setup, "before the surface set-up reads the base");
        assert!(terrain.contains("    @if(!TERRAIN_ANTI_TILING && !TERRAIN_PARALLAX) var pbr_input = pbr_input_from_standard_material(in, is_front);"), "Bevy's set-up without it");
        assert!(!terrain.contains("fp.uv +="), "the anti-tiled place is not moved: the two never meet");
        assert!(terrain.contains("    @if(TERRAIN_ANTI_TILING) let fp = terrain_footprint("));
        assert!(terrain.contains("    var delta = values.x * fade * layer_depth * Vt.xy * vec2(1.0, -1.0) / max(steepness, PARALLAX_GRAZING);"));
        assert!(terrain.contains("    for (var i = 0u; i < PARALLAX_RELIEF_STEPS; i += 1u) {"), "the relief search after the layers");
        assert!(terrain.contains("    let V = calculate_view(world, view.clip_from_view[3].w == 1.0);"), "the view's own ray (orthographic views too)");
        assert!(terrain.contains("            depth += w[k + 1u] * scale[k] * (1.0 - layer_height(k, instance_index, uv, footprint));"));
        assert!(terrain.contains("    let scale = parallax_scales(instance_index, values.x);"));
        // One set of weights, worked out at the plain place before the shift.
        for line in [
            "            parallax_paint[k] = saturate(opacity[k]);",
            "                    let parallax_texel = layer_diffuse_antitiled(k, in.instance_index, parallax_plain_taps, parallax_plain_fp);",
            "        parallax_w = terrain_ordered_shares(parallax_paint, parallax_n);",
            "            parallax_w = terrain_height_weights(parallax_w, parallax_height, parallax_params.x);",
        ] {
            assert!(terrain.find(line).unwrap_or_else(|| panic!("{line}")) < shift, "{line}: before the shift");
        }
        assert!(terrain.contains("        in.uv += parallax_shift(in.instance_index, in.uv, in.world_position, calculate_tbn_mikktspace(in.world_normal, in.world_tangent), parallax_w, parallax_n, parallax_footprint);"));
        assert!(terrain.contains("                @if(TERRAIN_PARALLAX) let share = saturate(opacity[k]);"), "the ordinary blend by the march's shares");
        assert!(terrain.contains("            @if(TERRAIN_PARALLAX) f = parallax_w;"), "the height blend by the march's weights");
        // The drawn reads: at the moved place with the plain place's gradients, base included.
        assert!(terrain.contains("            parallax_ddx = parallax_dx * exp2(view.mip_bias);"));
        assert!(terrain.contains("    @if(TERRAIN_PARALLAX && VERTEX_UVS_A) let parallax_fp = TerrainFootprint((standard_material(in.instance_index).uv_transform * vec3(in.uv, 1.0)).xy, parallax_ddx, parallax_ddy);"));
        assert!(terrain.contains("        pbr_input.material.base_color = pbr_input.material.base_color * terrain_base_diffuse(in.instance_index, parallax_flags, parallax_taps, parallax_fp);"));
        assert_eq!(terrain.matches("@if(TERRAIN_PARALLAX) let texel = layer_diffuse_antitiled(k, in.instance_index, parallax_taps, parallax_fp);").count(), 2, "both blend paths");
        assert!(terrain.contains("    let tap = TerrainTap(uv, mat2x2<f32>(1.0, 0.0, 0.0, 1.0), 1.0);"), "one unturned tap");
        assert!(terrain.contains("@if(TERRAIN_PARALLAX && !TERRAIN_HEIGHT_BLEND) import nvforever::terrain_weights::terrain_ordered_shares;"));
        // The march itself: none head-on, none over flat ground, some when tilted.
        let ridged = |p: Vec2| 0.6 * (0.5 - 0.5 * (p.x * 40.0).sin());
        let head_on = Vec3::new(0.0, 0.0, -1.0);
        let tilted = Vec3::new(0.6, 0.0, -0.8);
        assert_eq!(parallax_march(&ridged, head_on, 0.04), Vec2::ZERO, "none head-on");
        assert_eq!(parallax_march(&|_| 0.0, tilted, 0.04), Vec2::ZERO, "flat ground (every map missing) adds no depth: none");
        assert!(parallax_march(&ridged, tilted, 0.04).length() > 0.0, "some when tilted");
    }

    /// A world with the mesh terrain blend's capture refresh as it runs in the game (after
    /// the asset events, only while `BlendOn` says so, as mesh_terrain_blend.rs runs it while
    /// active), the terrain switches before the events, and one cell captured from a folded
    /// material that holds a height map. Returns the app, the cell and the material.
    fn capture_world() -> (App, Entity, Handle<TerrainMaterial>) {
        use bevy::asset::{AssetApp, AssetPlugin};
        use crate::graphics::{Graphics, Preset};
        use crate::terrain_surface::{CaptureDraw, CapturePath, CaptureSource, SurfaceCell};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>().init_asset::<NifMaterial>().init_asset::<TerrainMaterial>()
            .insert_resource(Graphics::preset(Preset::Vanilla)).insert_resource(TerrainSettings { tile: 256.0 }).insert_resource(BlendOn(true))
            .add_systems(PostUpdate, crate::terrain_surface::refresh_captures.after(bevy::asset::AssetEventSystems).run_if(|on: Res<BlendOn>| on.0));
        add_switch_systems(&mut app);
        let diffuse = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
        let source = TerrainMaterial { base: surface_material(Some(diffuse.clone()), None), extension: TerrainLayers { count: 1, diffuse0: Some(diffuse.clone()), height0: Some(diffuse.clone()), parallax_depths: [0.04; 6], texture_repeat: 256.0, tile: 256.0, ..default() } };
        let folded = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(source.clone());
        let mut cell = SurfaceCell {
            world: 0, grid: (0, 0), heights: vec![0.0; SIDE * SIDE], quadrants: vec![], paths: [CapturePath::HeightOnly("not-captured"); 4], content_revision: 0,
            sources: vec![CaptureSource::Folded(folded.clone())], dependencies: default(), detail: default(),
        };
        cell.capture(0, vec![(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()), CaptureDraw::Folded(Box::new(source)))], CapturePath::Folded { layers: 1 });
        let entity = app.world_mut().spawn(cell).id();
        // Settle: the material's Added event and the switches' first pass.
        app.update();
        app.update();
        (app, entity, folded)
    }

    #[derive(Resource)]
    struct BlendOn(bool);

    /// The tick the cell was last changed at: a remade capture moves it.
    fn cell_changed(app: &App, entity: Entity) -> bevy::ecs::change_detection::Tick {
        app.world().entity(entity).get_change_ticks::<crate::terrain_surface::SurfaceCell>().unwrap().changed
    }

    fn kept_height_blend(app: &App, entity: Entity) -> bool {
        match &app.world().get::<crate::terrain_surface::SurfaceCell>(entity).unwrap().quadrants[0].1 {
            crate::terrain_surface::CaptureDraw::Folded(m) => m.extension.height_blend,
            _ => panic!("the folded copy"),
        }
    }

    /// #3322 review: the capture refresh compares what a capture is made from, so the
    /// parallax switch's edits (dropped by the bake copy) leave it alone, and a real edit
    /// beside one in the same frame still remakes it.
    #[test]
    fn the_capture_refresh_compares_its_inputs() {
        use crate::graphics::Graphics;
        let (mut app, entity, folded) = capture_world();
        let before = cell_changed(&app, entity);
        app.world_mut().resource_mut::<Graphics>().terrain_parallax = true;
        app.update();
        app.update();
        assert!(app.world().resource::<Assets<TerrainMaterial>>().get(&folded).unwrap().extension.parallax, "the material was flipped");
        assert_eq!(cell_changed(&app, entity), before, "parallax alone: the capture untouched");
        {
            let mut g = app.world_mut().resource_mut::<Graphics>();
            g.terrain_height_blend = true;
            g.terrain_parallax = false;
        }
        app.update();
        app.update();
        assert!(kept_height_blend(&app, entity), "height blending beside a parallax edit remade the capture");
        assert_ne!(cell_changed(&app, entity), before);
    }

    /// #3322 review: an edit the parallax switch did not make remakes the capture even when
    /// it leaves every input as it was (a generic Modified is never second-guessed).
    #[test]
    fn a_generic_edit_remakes_the_capture_even_with_equal_inputs() {
        let (mut app, entity, folded) = capture_world();
        let before = cell_changed(&app, entity);
        {
            let mut materials = app.world_mut().resource_mut::<Assets<TerrainMaterial>>();
            let mut m = materials.get_mut(&folded).unwrap();
            // Touched mutably, nothing changed: a generic Modified with equal inputs.
            std::hint::black_box(&mut *m);
        }
        app.update();
        app.update();
        assert_ne!(cell_changed(&app, entity), before, "remade");
    }

    /// #3322 review: parallax flipped while the mesh terrain blend is off (its refresh not
    /// running), for longer than any event lives, then the blend and height blending turned
    /// on together: the height-blending edit remakes the capture (no remembered edit stands in
    /// its way).
    #[test]
    fn an_edit_after_a_blend_off_interval_still_remakes_the_capture() {
        use crate::graphics::Graphics;
        let (mut app, entity, _) = capture_world();
        app.world_mut().resource_mut::<BlendOn>().0 = false;
        app.world_mut().resource_mut::<Graphics>().terrain_parallax = true;
        for _ in 0..5 {
            app.update();
        }
        app.world_mut().resource_mut::<BlendOn>().0 = true;
        app.world_mut().resource_mut::<Graphics>().terrain_height_blend = true;
        app.update();
        app.update();
        assert!(kept_height_blend(&app, entity), "the height-blending edit reached the capture");
    }

    #[test]
    fn the_switch_flips_every_folded_material_in_place() {
        use crate::graphics::{Graphics, Preset};
        let mut app = App::new();
        app.insert_resource(Graphics::preset(Preset::Vanilla));
        app.insert_resource(Assets::<TerrainMaterial>::default());
        app.add_systems(Update, sync_height_blend);
        let material = || TerrainMaterial { base: StandardMaterial::default(), extension: TerrainLayers { count: 1, ..default() } };
        let a = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(material());
        app.update();
        let blends = |app: &App, h: &Handle<TerrainMaterial>| app.world().resource::<Assets<TerrainMaterial>>().get(h).unwrap().extension.height_blend;
        assert!(!blends(&app, &a), "off by default");
        assert!(!Graphics::preset(Preset::Enhanced).terrain_height_blend, "and in the enhanced preset");
        app.world_mut().resource_mut::<Graphics>().terrain_height_blend = true;
        let b = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(material());
        app.update();
        assert!(blends(&app, &a) && blends(&app, &b));
        app.world_mut().resource_mut::<Graphics>().terrain_height_blend = false;
        app.update();
        assert!(!blends(&app, &a) && !blends(&app, &b));
        // Without the settings resource (a tool's world): off.
        assert!(!height_blend_wanted(None));
    }

    #[test]
    fn the_sampling_shader_is_valid_wgsl_and_reads_inside_branches() {
        use wgpu::naga;
        let validate = |source: &str| {
            let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
        };
        let helper = nvf_assets::terrain_sampling::SAMPLING_SOURCE;
        validate(helper);
        // A fragment that reads a texture through the helpers inside a branch that differs
        // between the fragments of a quad (explicit gradients allow it), with derivatives
        // taken before it, as terrain.wesl and terrain_pbr.wesl do.
        let harness = format!(
            "{helper}\n\
             @group(0) @binding(0) var tex: texture_2d<f32>;\n\
             @group(0) @binding(1) var samp: sampler;\n\
             @fragment fn harness(@location(0) world: vec3<f32>, @location(1) o: f32) -> @location(0) vec4<f32> {{\n\
                 let uv = terrain_world_uv(world, 256.0);\n\
                 let fp = TerrainFootprint(uv, dpdx(uv) * 2.0, dpdy(uv) * 2.0);\n\
                 var c = vec4<f32>(0.0);\n\
                 if o > 0.5 {{\n\
                     c = terrain_sample_diffuse(tex, samp, terrain_hex3(fp.uv), fp);\n\
                     let n = terrain_sample_normal(tex, samp, terrain_hex3(fp.uv), fp, o > 0.75, true);\n\
                     let tbn = mat3x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0));\n\
                     c += vec4<f32>(terrain_world_normal(tbn, n, o > 0.9), 0.0);\n\
                 }}\n\
                 return c;\n\
             }}\n"
        );
        validate(&harness);
        // terrain.wesl takes the helpers and the surface set-up by name, only with the switch.
        let terrain = include_str!("terrain.wesl");
        assert!(terrain.contains("@if(TERRAIN_ANTI_TILING) import nvf_assets::terrain_sampling::{"));
        assert!(terrain.contains("@if(TERRAIN_ANTI_TILING) import nvf_assets::terrain_pbr::{"));
    }

    #[test]
    fn with_anti_tiling_off_both_old_paths_stay_as_they_were() {
        // The two #2550 paths are fenced off from the anti-tiled one, not edited: their
        // conditions gain `!TERRAIN_ANTI_TILING`, and Bevy's set-up is kept for them.
        let terrain = include_str!("terrain.wesl");
        assert!(terrain.contains("        @if(!TERRAIN_HEIGHT_BLEND && !TERRAIN_ANTI_TILING) {"));
        assert!(terrain.contains("        @if(TERRAIN_HEIGHT_BLEND && !TERRAIN_ANTI_TILING) {"));
        assert!(terrain.contains("    @if(!TERRAIN_ANTI_TILING && !TERRAIN_PARALLAX) var pbr_input = pbr_input_from_standard_material(in, is_front);"));
        assert!(terrain.contains("    @if(TERRAIN_ANTI_TILING) var pbr_input = terrain_pbr_begin(in, is_front, standard_material(in.instance_index));"));
        // The anti-tiled walk is the ordered blend's (mirrored in `antitiled_walk` below)
        // and the heights come from its reads.
        for line in [
            "                if remaining == 0.0 {",
            "                if o == 0.0 {",
            "                w[k + 1u] = o * remaining;",
            "                remaining *= 1.0 - o;",
            "            if w[0] != 0.0 {",
            "                    height[0] = terrain_height(raw[0].rgb, base_reference(in.instance_index, vec2(0.5), vec2(0.0), vec2(0.0), 0.0), anti_params.y);",
            "                        height[k + 1u] = terrain_height(raw[k + 1u].rgb, layer_reference(k, in.instance_index, vec2(0.5), vec2(0.0), vec2(0.0), 0.0), anti_params.y);",
        ] {
            assert!(terrain.contains(line), "terrain.wesl: `{line}`");
        }
    }

    /// CPU mirror of terrain.wesl's anti-tiled walk over the layers: the ordered shares
    /// (base at 0, layer k at k + 1) and which diffuse textures were read.
    fn antitiled_walk(opacity: [f32; 6], n: usize) -> ([f32; 7], [bool; 7]) {
        let mut w = [0.0f32; 7];
        let mut read = [false; 7];
        let mut remaining = 1.0f32;
        for k in (0..n.min(6)).rev() {
            if remaining == 0.0 {
                break;
            }
            let o = opacity[k].clamp(0.0, 1.0);
            if o == 0.0 {
                continue;
            }
            read[k + 1] = true;
            w[k + 1] = o * remaining;
            remaining *= 1.0 - o;
        }
        w[0] = remaining;
        read[0] = w[0] != 0.0;
        (w, read)
    }

    #[test]
    fn the_antitiled_walk_gives_the_ordered_shares_and_reads_only_what_can_show() {
        // Synthetic opacities: the same shares as the ordered blend, exactly.
        let mut seed = 0x2551u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        for case in 0..600 {
            let n = case % 7;
            let mut opacity = [0.0f32; 6];
            for o in &mut opacity {
                *o = match (next() * 5.0) as u32 {
                    0 => 0.0,
                    1 => 1.0,
                    2 => 1e-4,
                    _ => next(),
                };
            }
            let paint = opacity.map(|o| o.clamp(0.0, 1.0));
            let (w, read) = antitiled_walk(opacity, n);
            assert_eq!(w, ordered_shares(paint, n), "case {case}");
            for k in 0..n {
                // Opacity 0: never read. A share above 0: always read.
                if opacity[k] == 0.0 {
                    assert!(!read[k + 1], "case {case}: layer {k}");
                }
                if w[k + 1] != 0.0 {
                    assert!(read[k + 1], "case {case}: layer {k}");
                }
            }
            assert_eq!(read[0], w[0] != 0.0);
            // With height blending, a texture not read has a share of 0 and keeps it.
            for depth in [MIN_DEPTH, DEPTH, 0.5, 1e6] {
                let f = height_weights(w, [0.2, -0.25, 0.25, 0.0, -0.1, 0.15, 0.05], depth);
                assert!(f.iter().all(|x| x.is_finite() && *x >= 0.0));
                assert!((f.iter().sum::<f32>() - 1.0).abs() < 1e-5);
                for i in 0..7 {
                    assert!(read[i] || f[i] == 0.0, "case {case}, depth {depth}: {i} unread but weighted");
                }
            }
        }
        // A fully covering later layer: everything under it (the base too) is not read.
        let (w, read) = antitiled_walk([0.6, 0.3, 1.0, 0.0, 0.0, 0.0], 3);
        assert_eq!(w, [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        assert_eq!(read, [false, false, false, true, false, false, false]);
        // A tiny painted share survives (exact zeros skip, no epsilon).
        let (w, read) = antitiled_walk([1e-4, 0.0, 0.0, 0.0, 0.0, 0.0], 1);
        assert!(read[1] && w[1] > 0.0 && w[1] < 1e-3);
        // Unpainted ground reads the base alone.
        assert_eq!(antitiled_walk([0.0; 6], 6).1, [true, false, false, false, false, false, false]);
    }

    #[test]
    fn anti_tiling_selects_the_pipeline_the_feature_bit_and_the_repeat() {
        let off = TerrainLayers { count: 2, texture_repeat: 256.0, ..default() };
        let on = TerrainLayers { count: 2, texture_repeat: 256.0, anti_tiling: true, ..default() };
        assert_eq!(TerrainLayersKey::from(&off), TerrainLayersKey { height_blend: false, anti_tiling: false, surface_bake: false, parallax: false });
        assert_eq!(TerrainLayersKey::from(&on), TerrainLayersKey { height_blend: false, anti_tiling: true, surface_bake: false, parallax: false });
        assert_eq!((TerrainLayersUniform::from(&off).counts.w, TerrainLayersUniform::from(&on).counts.w), (0, FEATURE_ANTI_TILING));
        let both = TerrainLayers { height_blend: true, ..on.clone() };
        assert_eq!(TerrainLayersUniform::from(&both).counts.w, FEATURE_HEIGHT_BLEND | FEATURE_ANTI_TILING);
        assert_eq!(TerrainLayersKey::from(&both), TerrainLayersKey { height_blend: true, anti_tiling: true, surface_bake: false, parallax: false });
        // The repeat travels in tile.y, also where the mesh carries the coordinate (tile 0),
        // and the folding (tile.x, the layer count) is as before.
        let carried = TerrainLayersUniform::from(&TerrainLayers { count: 9, texture_repeat: 300.0, anti_tiling: true, ..default() });
        assert_eq!((carried.tile.x, carried.tile.y, carried.counts.x), (0.0, 300.0, FOLDED_LAYERS_WITH_UV as u32));
        let worked_out = TerrainLayersUniform::from(&TerrainLayers { count: 9, tile: 256.0, texture_repeat: 256.0, ..default() });
        assert_eq!((worked_out.tile.x, worked_out.tile.y, worked_out.counts.x), (256.0, 256.0, FOLDED_LAYERS as u32));
        // A repeat that cannot place the reads keeps the ordinary pipeline.
        for bad in [0.0, -256.0, f32::NAN] {
            let l = TerrainLayers { count: 2, texture_repeat: bad, anti_tiling: true, ..default() };
            assert!(!TerrainLayersKey::from(&l).anti_tiling, "{bad}");
            assert_eq!((TerrainLayersUniform::from(&l).counts.w, TerrainLayersUniform::from(&l).tile.y), (0, 0.0), "{bad}");
        }
        // The definitions go to the main pass only (nvf_assets terrain_sampling's test has
        // the rest); height blending's are untouched.
        let main = || RenderPipelineDescriptor { fragment: Some(bevy::render::render_resource::FragmentState { shader_defs: vec!["VERTEX_UVS_A".into()], ..default() }), ..default() };
        let mut d = main();
        height_blend_def(&mut d, TerrainLayersKey { height_blend: false, anti_tiling: true, surface_bake: false, parallax: false });
        assert_eq!(d.fragment.as_ref().unwrap().shader_defs.len(), 1, "height blending's switch alone decides TERRAIN_HEIGHT_BLEND");
    }

    #[test]
    fn the_anti_tiling_switch_flips_every_folded_material_in_place() {
        use crate::graphics::{Graphics, Preset};
        let mut app = App::new();
        app.insert_resource(Graphics::preset(Preset::Vanilla));
        app.insert_resource(TerrainSettings { tile: 256.0 });
        app.insert_resource(Assets::<TerrainMaterial>::default());
        app.add_systems(Update, (sync_height_blend, sync_anti_tiling));
        let material = || TerrainMaterial { base: StandardMaterial::default(), extension: TerrainLayers { count: 1, texture_repeat: 256.0, ..default() } };
        let a = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(material());
        app.update();
        let get = |app: &App, h: &Handle<TerrainMaterial>| {
            let e = &app.world().resource::<Assets<TerrainMaterial>>().get(h).unwrap().extension;
            (e.anti_tiling, e.height_blend)
        };
        assert_eq!(get(&app, &a), (false, false), "off by default");
        assert!(!Graphics::preset(Preset::Enhanced).terrain_anti_tiling, "and in the enhanced preset");
        app.world_mut().resource_mut::<Graphics>().terrain_anti_tiling = true;
        let b = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(material());
        app.update();
        assert_eq!((get(&app, &a), get(&app, &b)), ((true, false), (true, false)), "height blending left alone");
        app.world_mut().resource_mut::<Graphics>().terrain_height_blend = true;
        app.update();
        assert_eq!(get(&app, &a), (true, true), "the two switches together");
        app.world_mut().resource_mut::<Graphics>().terrain_anti_tiling = false;
        app.update();
        assert_eq!((get(&app, &a), get(&app, &b)), ((false, true), (false, true)));
        assert!(!anti_tiling_wanted(None));
    }

    #[test]
    fn the_switch_reaches_the_landscapes_own_nif_materials_and_no_other() {
        use crate::graphics::{Graphics, Preset};
        let mut app = App::new();
        app.insert_resource(Graphics::preset(Preset::Vanilla));
        app.insert_resource(TerrainSettings { tile: 300.0 });
        app.insert_resource(Assets::<TerrainMaterial>::default());
        app.insert_resource(Assets::<NifMaterial>::default());
        app.add_systems(Update, sync_anti_tiling);
        let surface = || surface_material(None, None);
        let (base, layer, other) = {
            let mut nif = app.world_mut().resource_mut::<Assets<NifMaterial>>();
            let base = nif.add(terrain_nif_material(surface(), 300.0, false));
            let layer = nif.add(terrain_nif_material(StandardMaterial { alpha_mode: AlphaMode::Blend, depth_bias: sort_first(4.0), ..surface() }, 300.0, false));
            (base, layer, nif.add(nif_material(StandardMaterial::default())))
        };
        let on = |app: &App, h: &Handle<NifMaterial>| app.world().resource::<Assets<NifMaterial>>().get(h).unwrap().extension.anti_tiling;
        app.update();
        assert!(!on(&app, &base) && !on(&app, &layer) && !on(&app, &other));
        app.world_mut().resource_mut::<Graphics>().terrain_anti_tiling = true;
        app.update();
        assert!(on(&app, &base) && on(&app, &layer), "the landscape's materials flipped in place");
        assert!(!on(&app, &other), "a shape's material is left alone (no texture repeat)");
        // The layer keeps its blending and draw order; the key selects the separate path.
        let nif = app.world().resource::<Assets<NifMaterial>>();
        let l = nif.get(&layer).unwrap();
        assert!(matches!(l.base.alpha_mode, AlphaMode::Blend) && l.base.depth_bias == sort_first(4.0));
        assert!(nvf_assets::nif_material::NifSurfaceKey::from(&l.extension).anti_tiling);
        assert!(!nvf_assets::nif_material::NifSurfaceKey::from(&nif.get(&other).unwrap().extension).anti_tiling);
        assert_eq!(terrain_nif_material(surface(), 256.0, true).extension.terrain_repeat, 256.0);
        app.world_mut().resource_mut::<Graphics>().terrain_anti_tiling = false;
        app.update();
        assert!(!on(&app, &base) && !on(&app, &layer));
    }

    #[test]
    fn the_landscape_materials_hold_only_what_the_anti_tiled_set_up_reads() {
        // nvf_assets terrain_pbr.wesl's contract: colour, diffuse texture, normal map and
        // scalar values; nothing it would leave out.
        let image = Handle::<Image>::default();
        for m in [surface_material(Some(image.clone()), Some(image.clone())), surface_material(None, None)] {
            assert!(m.emissive_texture.is_none() && m.metallic_roughness_texture.is_none() && m.occlusion_texture.is_none() && m.depth_map.is_none());
            // (The transmission, clearcoat and anisotropy textures are not built into this
            // Bevy; their strengths are checked below.)
            assert!(m.specular_texture.is_none() && m.specular_tint_texture.is_none());
            assert_eq!((m.specular_transmission, m.diffuse_transmission, m.thickness, m.clearcoat, m.anisotropy_strength), (0.0, 0.0, 0.0, 0.0, 0.0));
            assert!(!m.unlit && !m.double_sided && m.flip_normal_map_y && matches!(m.alpha_mode, AlphaMode::Opaque));
            assert_eq!(m.uv_transform, bevy::math::Affine2::IDENTITY, "a transform would need the normal frame turned too");
            assert_eq!(m.emissive, LinearRgba::BLACK);
        }
    }

    fn triangles(mesh: &Mesh) -> Vec<[u32; 3]> {
        let Some(Indices::U32(idx)) = mesh.indices() else { panic!() };
        idx.chunks(3).map(|t| [t[0], t[1], t[2]]).collect()
    }

    /// The LAND vertex (row, column) of quadrant `q`'s detail vertex `v`.
    fn land_vertex_of(q: u8, v: usize) -> (usize, usize) {
        let half = QUAD_SIDE - 1;
        ((q as usize / 2) * half + v / QUAD_SIDE, (q as usize % 2) * half + v % QUAD_SIDE)
    }

    /// The draws a 4 × 4 surface can be drawn with (#2676): full, half, and full with two
    /// coarse sides, with the LAND stride of each.
    fn fine_draws() -> [(usize, QuadrantDraw, usize); 4] {
        let coarse = QuadrantDraw { step: 1, coarse: [true, false, true, false] };
        [(4, QuadrantDraw::FULL, 4), (4, QuadrantDraw::HALF, 2), (4, coarse, 4), (2, QuadrantDraw::FULL, 2)]
    }

    /// [design] #2149/#2661: the detail reads a drawn mesh's LAND vertices, in order, from a
    /// full 65 × 65 draw, a half 33 × 33 one and a full one with coarse sides: the LAND
    /// vertices' linear colour and the surface's normal there, with the tangents' sign.
    #[test]
    fn detail_vertices_are_the_land_vertices_of_every_drawn_level() {
        let land = hilly_land();
        for (subdiv, draw, stride) in fine_draws() {
            let s = smoothed_with(&land, fnv_world::land_surface::SmoothParams { subdiv, ..Default::default() });
            let normals = s.normals().unwrap();
            for q in 0..4u8 {
                let mesh = quadrant_mesh_drawn(&land, &s, q, draw, None, Vec2::ZERO, 256.0).unwrap();
                let qs = (QUAD_SIDE - 1) * stride + 1;
                assert_eq!(mesh.count_vertices(), qs * qs, "{subdiv} {draw:?}");
                let d = detail_vertices(&mesh).expect("a drawn grid");
                let pos = attr3(&mesh, Mesh::ATTRIBUTE_POSITION);
                assert_eq!((d.tint.len(), d.normal.len(), d.tangent.len(), d.sign), (QUAD_SIDE * QUAD_SIDE, QUAD_SIDE * QUAD_SIDE, QUAD_SIDE * QUAD_SIDE, TANGENT_SIGN));
                for v in 0..QUAD_SIDE * QUAD_SIDE {
                    let (r, c) = land_vertex_of(q, v);
                    let colour = land.colors.as_ref().unwrap()[r * SIDE + c].map(srgb_to_linear);
                    assert_eq!(d.tint[v], colour, "{subdiv} {draw:?} q{q} v{v}: tint");
                    let n = Vec3::from_array(normals[s.land_vertex(r, c)]).normalize_or(Vec3::Z);
                    assert!((Vec3::from_array(d.normal[v]) - n).abs().max_element() < 1e-6, "{subdiv} {draw:?} q{q} v{v}: normal");
                    let east = (Vec3::X - n * n.x).normalize_or(Vec3::X);
                    assert!((Vec3::from_array(d.tangent[v]) - east).abs().max_element() < 1e-6, "{subdiv} {draw:?} q{q} v{v}: tangent");
                    // The vertex read is the LAND vertex's (a coarse side moves only the
                    // vertices between them).
                    let i = (v / QUAD_SIDE) * stride * qs + (v % QUAD_SIDE) * stride;
                    assert_eq!((pos[i][0], pos[i][1], pos[i][2]), (c as f32 * SPACING, r as f32 * SPACING, s.heights()[s.land_vertex(r, c)]), "{subdiv} {draw:?} q{q} v{v}");
                }
            }
        }
    }

    /// [design] #2149/#2661: the bake's copies at the LAND grid's resolution. Drawn as
    /// stored they are the mesh itself; drawn finer (full, half, coarse-sided), the 17 × 17
    /// LAND vertices with every attribute's value there, and the triangles a mesh drawn as
    /// stored has (a layer's: those it paints).
    #[test]
    fn land_resolution_copies_keep_the_land_vertices() {
        use bevy::mesh::VertexAttributeValues as V;
        let land = hilly_land();
        let mut alpha = vec![0.0; QUAD_SIDE * QUAD_SIDE];
        alpha[5 * QUAD_SIDE + 7] = 0.4;
        alpha[12 * QUAD_SIDE + 16] = 1.0;
        alpha[13 * QUAD_SIDE + 16] = 0.25;
        let mut second = vec![0.0; QUAD_SIDE * QUAD_SIDE];
        second[QUAD_SIDE * QUAD_SIDE - 1] = 0.6;
        second[3 * QUAD_SIDE + 3] = 0.8;
        let tile = 256.0;
        let stored_surface = stored(&land);
        for q in 0..4u8 {
            // Stride 1: unchanged, byte for byte (smoothing off).
            let base = quadrant_mesh(&land, &stored_surface, q, None, Vec2::ZERO, tile).unwrap();
            let layer = quadrant_mesh(&land, &stored_surface, q, Some(&alpha), Vec2::ZERO, tile).unwrap();
            let folded = folded_mesh(&land, &stored_surface, q, &[&alpha, &second, &alpha, &second, &alpha, &second], Vec2::ZERO, tile).unwrap();
            for mesh in [&base, &layer, &folded] { assert_eq!(&land_resolution(mesh), mesh, "q{q}"); }
        }
        for (subdiv, draw, stride) in fine_draws() {
            let s = smoothed_with(&land, fnv_world::land_surface::SmoothParams { subdiv, ..Default::default() });
            let sub = drawn_sub(&s, draw);
            let (fine, fine_second) = (refine_quad(&alpha, sub), refine_quad(&second, sub));
            let qs = (QUAD_SIDE - 1) * stride + 1;
            let at = |v: usize| (v / QUAD_SIDE) * stride * qs + (v % QUAD_SIDE) * stride;
            for q in 0..4u8 {
                let base = quadrant_mesh_drawn(&land, &s, q, draw, None, Vec2::ZERO, tile).unwrap();
                let layer = quadrant_mesh_drawn(&land, &s, q, draw, Some(&fine), Vec2::ZERO, tile).unwrap();
                let folded = folded_mesh_drawn(&land, &s, q, draw, &[&fine, &fine_second, &fine, &fine_second, &fine, &fine_second], Vec2::ZERO, tile).unwrap();
                let as_stored = [
                    quadrant_mesh(&land, &stored_surface, q, None, Vec2::ZERO, tile).unwrap(),
                    quadrant_mesh(&land, &stored_surface, q, Some(&alpha), Vec2::ZERO, tile).unwrap(),
                    folded_mesh(&land, &stored_surface, q, &[&alpha, &second, &alpha, &second, &alpha, &second], Vec2::ZERO, tile).unwrap(),
                ];
                for (mesh, stored_mesh) in [&base, &layer, &folded].into_iter().zip(&as_stored) {
                    assert_eq!(mesh.count_vertices(), qs * qs);
                    let copy = land_resolution(mesh);
                    assert_eq!(copy.count_vertices(), QUAD_SIDE * QUAD_SIDE);
                    assert_eq!(copy.attributes().count(), mesh.attributes().count(), "every attribute kept");
                    for (attribute, values) in mesh.attributes() {
                        let picked = copy.attribute(*attribute).expect("the attribute");
                        let ok = match (values, picked) {
                            (V::Float32x2(a), V::Float32x2(b)) => (0..b.len()).all(|v| b[v] == a[at(v)]),
                            (V::Float32x3(a), V::Float32x3(b)) => (0..b.len()).all(|v| b[v] == a[at(v)]),
                            (V::Float32x4(a), V::Float32x4(b)) => (0..b.len()).all(|v| b[v] == a[at(v)]),
                            _ => false,
                        };
                        assert!(ok, "{subdiv} {draw:?} q{q}: {attribute:?} at the LAND vertices");
                    }
                    // The triangles of the LAND grid a mesh drawn as stored has: valid
                    // indices, the same split, a layer's painted ones only.
                    let tris = triangles(&copy);
                    assert!(tris.iter().flatten().all(|&i| (i as usize) < QUAD_SIDE * QUAD_SIDE));
                    assert_eq!(tris, triangles(stored_mesh), "{subdiv} {draw:?} q{q}");
                    // Its colours and opacities are the stored mesh's: the bake reads the same values.
                    let (Some(V::Float32x4(a)), Some(V::Float32x4(b))) = (copy.attribute(Mesh::ATTRIBUTE_COLOR), stored_mesh.attribute(Mesh::ATTRIBUTE_COLOR)) else { panic!() };
                    assert!(a.iter().zip(b).all(|(x, y)| (Vec4::from_array(*x) - Vec4::from_array(*y)).abs().max_element() < 1e-6), "{subdiv} {draw:?} q{q}: colours");
                    // So are the other channels the bake reads: both texture coordinates (a
                    // folded draw's packed opacities among them) and the tangent's fourth
                    // number, against the mesh built as stored, not the copy's own rule.
                    for uv in [Mesh::ATTRIBUTE_UV_0, Mesh::ATTRIBUTE_UV_1] {
                        match (copy.attribute(uv), stored_mesh.attribute(uv)) {
                            (Some(V::Float32x2(a)), Some(V::Float32x2(b))) => assert!(a.iter().zip(b).all(|(x, y)| (Vec2::from_array(*x) - Vec2::from_array(*y)).abs().max_element() < 1e-4), "{subdiv} {draw:?} q{q}: {uv:?}"),
                            (None, None) => {}
                            _ => panic!("{subdiv} {draw:?} q{q}: {uv:?} differs in kind"),
                        }
                    }
                    let (Some(V::Float32x4(a)), Some(V::Float32x4(b))) = (copy.attribute(Mesh::ATTRIBUTE_TANGENT), stored_mesh.attribute(Mesh::ATTRIBUTE_TANGENT)) else { panic!() };
                    assert!(a.iter().zip(b).all(|(x, y)| (x[3] - y[3]).abs() < 1e-6), "{subdiv} {draw:?} q{q}: the tangent's fourth number");
                }
            }
        }
    }

    #[test]
    fn a_layer_keeps_only_the_triangles_it_paints() {
        let land = sloped_land();
        // The base keeps every triangle.
        let base = quadrant_mesh(&land, &stored(&land), 0, None, Vec2::ZERO, 256.0).unwrap();
        assert_eq!(triangles(&base).len(), (QUAD_SIDE - 1) * (QUAD_SIDE - 1) * 2);
        // Opacity at one inner vertex: the six triangles around it.
        let mut alpha = vec![0.0; QUAD_SIDE * QUAD_SIDE];
        let v = 5 * QUAD_SIDE + 7;
        alpha[v] = 0.4;
        let layer = quadrant_mesh(&land, &stored(&land), 0, Some(&alpha), Vec2::ZERO, 256.0).unwrap();
        let tris = triangles(&layer);
        assert_eq!(tris.len(), 6);
        assert!(tris.iter().all(|t| t.contains(&(v as u32))));
        // No opacity anywhere: no mesh at all.
        assert!(quadrant_mesh(&land, &stored(&land), 0, Some(&vec![0.0; QUAD_SIDE * QUAD_SIDE]), Vec2::ZERO, 256.0).is_none());
    }

    fn base(q: u8, tex: u32) -> Layer {
        Layer { quadrant: q, texture: Some(tex), layer: -1, base: true, alpha: Vec::new() }
    }

    fn layer(q: u8, tex: u32, n: i16, alpha: &[(usize, f32)]) -> Layer {
        Layer { quadrant: q, texture: Some(tex), layer: n, base: false, alpha: alpha.iter().map(|&(p, a)| (p as u16, a)).collect() }
    }

    fn opacity(layers: &[Layer], q: u8, tex: u32, p: usize) -> Option<f32> {
        layers.iter().find(|l| !l.base && l.quadrant == q && l.texture == Some(tex)).map(|l| alpha_grid(l)[p])
    }

    const L: usize = QUAD_SIDE - 1;

    #[test]
    fn a_texture_painted_up_to_a_cell_edge_carries_over_it() {
        // Save 36 (issue #2089): the south cell's quadrant 2 paints texture 0xA along its
        // north edge; the north cell's quadrant 0 has no layer of it.
        let south = vec![base(2, 0xB), layer(2, 0xA, 0, &[(L * QUAD_SIDE + 12, 1.0), (L * QUAD_SIDE + 13, 0.5), ((L - 1) * QUAD_SIDE + 12, 0.8)])];
        let north = vec![base(0, 0xB), layer(0, 0xC, 0, &[(5 * QUAD_SIDE + 3, 0.4)])];
        let stitched = stitch_layers(&north, &[None, None, Some(south.clone()), None]);
        // A layer of 0xA over the north cell's own, painted on its south edge only.
        let added = stitched.iter().find(|l| l.texture == Some(0xA)).unwrap();
        assert_eq!((added.quadrant, added.layer, added.base), (0, 1, false));
        assert_eq!(opacity(&stitched, 0, 0xA, 12), Some(1.0));
        assert_eq!(opacity(&stitched, 0, 0xA, 13), Some(0.5));
        assert_eq!(added.alpha.len(), 2);
        // Its own layer and base are as they were.
        assert_eq!(&stitched[..2], &north[..]);
        // The south cell keeps what it paints: nothing of 0xC reaches its edge.
        assert_eq!(stitch_layers(&south, &[Some(north), None, None, None]), south);
    }

    #[test]
    fn edge_opacities_take_the_larger_side_inside_a_cell_too() {
        // Quadrant 0's east edge meets quadrant 1's west edge: 0xA disagrees on both
        // sides' rows 3 and 5; a texture that is quadrant 1's base is not added there.
        let own = vec![
            base(0, 0xB),
            base(1, 0xD),
            layer(0, 0xA, 0, &[(3 * QUAD_SIDE + L, 0.2), (5 * QUAD_SIDE + L, 0.9)]),
            layer(1, 0xA, 0, &[(3 * QUAD_SIDE, 0.6), (5 * QUAD_SIDE, 0.1)]),
            layer(0, 0xD, 1, &[(7 * QUAD_SIDE + L, 1.0)]),
        ];
        let stitched = stitch_layers(&own, &[None, None, None, None]);
        assert_eq!(opacity(&stitched, 0, 0xA, 3 * QUAD_SIDE + L), Some(0.6));
        assert_eq!(opacity(&stitched, 0, 0xA, 5 * QUAD_SIDE + L), Some(0.9));
        assert_eq!(opacity(&stitched, 1, 0xA, 3 * QUAD_SIDE), Some(0.6));
        assert_eq!(opacity(&stitched, 1, 0xA, 5 * QUAD_SIDE), Some(0.9));
        assert_eq!(opacity(&stitched, 1, 0xD, 7 * QUAD_SIDE), None);
        assert_eq!(stitched.len(), own.len());
        // Edges that agree are left alone.
        let agreed = stitch_layers(&stitched, &[None, None, None, None]);
        assert_eq!(agreed, stitched);
    }

    /// A 4x4 BC1 DDS header and one block (terrain_surface.rs tests' synthetic texture).
    fn synthetic_dds() -> Vec<u8> {
        let mut dds = vec![0u8; 136];
        dds[..4].copy_from_slice(b"DDS ");
        for (at, value) in [(4, 124u32), (8, 0x1007), (12, 4), (16, 4), (28, 1), (76, 32), (80, 4), (108, 0x1000)] { dds[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
        dds[84..88].copy_from_slice(b"DXT1");
        dds
    }

    /// The shader's reconstruction (nif_surface_bindings.wesl `terrain_triangle`,
    /// `detail_attributes`, `detail_opacity`) from the packed words: the corner records of
    /// the square's triangle and their barycentric weights.
    fn shader_mix(layout: crate::terrain_surface::HeightLayout, data: &[u8], entry: usize, q: u8, square: (usize, usize), f: Vec2, overlays: usize) -> (Vec3, Vec3, Vec3, Vec<f32>) {
        use nvf_assets::nif_surface_blend::{detail_snorm, detail_unorm, detail_vertex_word, DETAIL_VERTEX_WORDS};
        let half = QUAD_SIDE - 1;
        let local = (square.0 - half * (q as usize % 2), square.1 - half * (q as usize / 2));
        let sw = local.1 * QUAD_SIDE + local.0;
        let (vertices, weights) = if f.y > f.x {
            ([sw, sw + QUAD_SIDE + 1, sw + QUAD_SIDE], [1.0 - f.y, f.x, f.y - f.x])
        } else {
            ([sw, sw + 1, sw + QUAD_SIDE + 1], [1.0 - f.x, f.x - f.y, f.y])
        };
        let (mut tint, mut normal, mut tangent, mut opacity) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, vec![0.0; overlays]);
        for (v, w) in vertices.into_iter().zip(weights) {
            let words: Vec<u32> = (0..DETAIL_VERTEX_WORDS).map(|j| crate::terrain_surface::detail_word(layout, data, detail_vertex_word(entry, v) + j)).collect();
            tint += w * Vec3::new(detail_unorm(&words, 0), detail_unorm(&words, 1), detail_unorm(&words, 2));
            normal += w * Vec3::new(detail_snorm(&words, 3), detail_snorm(&words, 4), detail_snorm(&words, 5));
            tangent += w * Vec3::new(detail_snorm(&words, 6), detail_snorm(&words, 7), detail_snorm(&words, 8));
            for (k, o) in opacity.iter_mut().enumerate() { *o += w * detail_unorm(&words, 9 + k); }
        }
        (tint, normal, tangent, opacity)
    }

    /// Barycentric interpolation over the actual mesh triangle containing `p` (cell-local).
    fn mesh_mix(mesh: &Mesh, p: Vec2) -> Option<([u32; 3], [f32; 3])> {
        let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        triangles(mesh).into_iter().find_map(|t| {
            let [a, b, c] = t.map(|i| Vec2::new(pos[i as usize][0], pos[i as usize][1]));
            let d = (b - a).perp_dot(c - a);
            let wb = (p - a).perp_dot(c - a) / d;
            let wc = (b - a).perp_dot(p - a) / d;
            let wa = 1.0 - wb - wc;
            (wa >= -1e-5 && wb >= -1e-5 && wc >= -1e-5).then_some((t, [wa, wb, wc]))
        })
    }

    /// One quadrant's inputs at a fragment for the detail mirrors below: raw texels (linear
    /// rgb, alpha), the interpolated opacities, the textures' global and local height
    /// references, the vertex tint and the base material's colour. Synthetic values only.
    /// A folded overlay's raw texel, opacity and (global, local) height references.
    type FoldedInput = ([f32; 4], f32, ([f32; 3], [f32; 3]));

    struct DetailCase {
        base: Option<[f32; 4]>,
        base_reference: ([f32; 3], [f32; 3]),
        folded: Vec<FoldedInput>,
        overflow: Vec<([f32; 4], f32)>,
        tint: [f32; 3],
        base_rgb: [f32; 3],
    }

    fn mul3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
    }

    fn rgb(t: [f32; 4]) -> [f32; 3] {
        [t[0], t[1], t[2]]
    }

    /// The separately drawn overflow layers over the base draw's colour: each mixed in by
    /// saturate(opacity), white-tinted (graphics-enhanced.md §3.14: the
    /// detail's albedo approximation of their lit blend, the same for both mirrors).
    fn overflow_mix(mut colour: [f32; 3], c: &DetailCase, from: usize) -> [f32; 3] {
        for (texel, o) in &c.overflow[from..] {
            let share = o.clamp(0.0, 1.0);
            let layer = mul3(rgb(*texel), c.tint);
            for (x, l) in colour.iter_mut().zip(layer) {
                *x = *x * (1.0 - share) + l * share;
            }
        }
        colour
    }

    /// CPU mirror of nif_surface_bindings.wesl `mesh_terrain_detail`'s colour, branch for
    /// branch, driven by the quadrant's table words (nvf_assets `DetailQuadrant::words`).
    fn detail_mirror(w: &[u32], c: &DetailCase) -> [f32; 3] {
        let total = (((w[0] >> 12) & 15) as usize).min(12);
        let folded = (((w[0] >> 8) & 15) as usize).min(total);
        let (height_blend, anti) = (w[0] & 16 != 0, w[0] & 32 != 0);
        let weighted = height_blend || anti;
        let tinted = mul3(c.tint, c.base_rgb);
        let overlay = |k: usize| -> ([f32; 4], f32, bool) {
            if k < folded { (c.folded[k].0, c.folded[k].1, true) } else { (c.overflow[k - folded].0, c.overflow[k - folded].1, false) }
        };
        let mut colour = tinted;
        if !weighted {
            if w[0] & 4 != 0 {
                colour = mul3(colour, rgb(c.base.unwrap()));
            }
        } else {
            let n = folded.min(6);
            let mut raw = [[0.0f32; 4]; 7];
            raw[0] = [1.0; 4];
            let mut wt = [0.0f32; 7];
            if anti {
                let mut remaining = 1.0f32;
                for (k, l) in c.folded.iter().enumerate().take(n).rev() {
                    if remaining == 0.0 {
                        break;
                    }
                    let o = l.1.clamp(0.0, 1.0);
                    if o == 0.0 {
                        continue;
                    }
                    raw[k + 1] = l.0;
                    wt[k + 1] = o * remaining;
                    remaining *= 1.0 - o;
                }
                wt[0] = remaining;
            } else {
                // #2661 review: an overlay at opacity 0 is not read (its raw texel stays 0).
                let mut paint = [0.0f32; 6];
                for (k, l) in c.folded.iter().enumerate().take(n) {
                    let o = l.1.clamp(0.0, 1.0);
                    if o == 0.0 {
                        continue;
                    }
                    raw[k + 1] = l.0;
                    paint[k] = o;
                }
                wt = ordered_shares(paint, n);
            }
            if w[0] & 4 != 0 && (!anti || wt[0] != 0.0) {
                raw[0] = c.base.unwrap();
            }
            let mut f = wt;
            if height_blend {
                let (depth, gain) = (f32::from_bits(w[29]), f32::from_bits(w[30]));
                let local = w[0] & 64 != 0 && !anti;
                let pick = |r: ([f32; 3], [f32; 3])| if local { r.1 } else { r.0 };
                let mut h = [0.0f32; 7];
                if wt[0] != 0.0 && w[0] & 4 != 0 {
                    h[0] = terrain_height(rgb(raw[0]), pick(c.base_reference), gain);
                }
                for (k, l) in c.folded.iter().enumerate().take(n) {
                    if wt[k + 1] != 0.0 {
                        h[k + 1] = terrain_height(rgb(raw[k + 1]), pick(l.2), gain);
                    }
                }
                f = height_weights(wt, h, depth);
            }
            colour = [0.0; 3];
            for (fi, r) in f.iter().zip(&raw).take(n + 1) {
                if *fi != 0.0 {
                    for (x, v) in colour.iter_mut().zip(mul3(rgb(*r), tinted)) {
                        *x += fi * v;
                    }
                }
            }
        }
        for k in if weighted { folded } else { 0 }..total {
            // #2661 review: an opacity of exactly 0 skips the overlay.
            let (texel, o, is_folded) = overlay(k);
            let share = o.clamp(0.0, 1.0);
            if share == 0.0 {
                continue;
            }
            let layer = mul3(mul3(rgb(texel), c.tint), if is_folded { c.base_rgb } else { [1.0; 3] });
            for (x, l) in colour.iter_mut().zip(layer) {
                *x = *x * (1.0 - share) + l * share;
            }
        }
        colour
    }

    /// The near draw's colour as this module's mirrors of terrain.wesl state it: the
    /// ordinary mix chain, the height-blended path (`ordered_shares`, heights against the
    /// chosen reference, `height_weights`) or the anti-tiled walk (`antitiled_walk`, the
    /// global reference always), then the overflow layers ([`overflow_mix`]).
    fn near_mirror(c: &DetailCase, height: Option<(f32, f32, bool)>, anti: bool) -> [f32; 3] {
        let n = c.folded.len();
        let tinted = mul3(c.tint, c.base_rgb);
        let base = c.base.unwrap_or([1.0; 4]);
        let base_colour = mul3(tinted, rgb(base));
        let layer = |l: &FoldedInput| mul3(rgb(l.0), tinted);
        let colour = match (height, anti) {
            (None, false) => {
                let mut colour = base_colour;
                for l in &c.folded {
                    let share = l.1.clamp(0.0, 1.0);
                    for (x, v) in colour.iter_mut().zip(layer(l)) {
                        *x = *x * (1.0 - share) + v * share;
                    }
                }
                colour
            }
            _ => {
                let w = if anti {
                    let opacity: [f32; 6] = std::array::from_fn(|k| c.folded.get(k).map_or(0.0, |l| l.1));
                    antitiled_walk(opacity, n).0
                } else {
                    let paint: [f32; 6] = std::array::from_fn(|k| c.folded.get(k).map_or(0.0, |l| l.1.clamp(0.0, 1.0)));
                    ordered_shares(paint, n)
                };
                let f = match height {
                    None => w,
                    Some((depth, gain, local)) => {
                        let pick = |r: ([f32; 3], [f32; 3])| if local && !anti { r.1 } else { r.0 };
                        let mut h = [0.0f32; 7];
                        if w[0] > 0.0 && c.base.is_some() {
                            h[0] = terrain_height(rgb(base), pick(c.base_reference), gain);
                        }
                        for (k, l) in c.folded.iter().enumerate() {
                            if w[k + 1] > 0.0 {
                                h[k + 1] = terrain_height(rgb(l.0), pick(l.2), gain);
                            }
                        }
                        height_weights(w, h, depth)
                    }
                };
                let mut colour = base_colour.map(|x| x * f[0]);
                for (fk, l) in f[1..].iter().zip(&c.folded) {
                    for (x, v) in colour.iter_mut().zip(layer(l)) {
                        *x += fk * v;
                    }
                }
                colour
            }
        };
        overflow_mix(colour, c, 0)
    }

    /// [design] #2661 with #2550 and #2551: the mesh terrain blend's detail composes the
    /// landscape's colour as the near draw does, with neither switch, either or both, and
    /// both height references: the detail's mirror ([`detail_mirror`], driven by the table
    /// words the publication packs from `DetailFeatures`) against this module's mirrors of
    /// terrain.wesl ([`near_mirror`]). The shader's lines are kept to the mirror by
    /// mesh_blend_shader_tests.rs. Synthetic texels, opacities and references only. The
    /// texels' alpha has no part in either (#3789).
    #[test]
    fn the_mesh_blend_detail_composes_as_the_near_draw_with_and_without_the_switches() {
        use nvf_assets::nif_surface_blend::{DetailFeatures, DetailQuadrant};
        let mut seed = 0x2661u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let rgb3 = |next: &mut dyn FnMut() -> f32| [next(), next(), next()];
        let share = |next: &mut dyn FnMut() -> f32| match (next() * 5.0) as u32 { 0 => 0.0, 1 => 1.0, 2 => 1e-4, _ => next() };
        let texel = |next: &mut dyn FnMut() -> f32| [next(), next(), next(), match (next() * 4.0) as u32 { 0 => 0.0, 1 => 1.0, _ => next() }];
        let mut checked = [0usize; 4];
        for case in 0..800 {
            let n = case % (FOLDED_LAYERS + 1);
            let m = (case / 7) % 3;
            let c = DetailCase {
                base: (case % 5 != 0).then(|| { let t = texel(&mut next); [t[0], t[1], t[2], 1.0] }),
                base_reference: (rgb3(&mut next), rgb3(&mut next)),
                folded: (0..n).map(|_| (texel(&mut next), share(&mut next), (rgb3(&mut next), rgb3(&mut next)))).collect(),
                overflow: (0..m).map(|_| (texel(&mut next), share(&mut next))).collect(),
                tint: rgb3(&mut next),
                base_rgb: rgb3(&mut next),
            };
            for (switches, (height, anti)) in [(None, false), (Some((DEPTH, 8.0, false)), false), (Some((0.05, 4.0, true)), false), (None, true), (Some((DEPTH, 8.0, true)), true)].into_iter().enumerate().map(|(i, s)| (i.min(3), s)) {
                let features = DetailFeatures {
                    height_blend: height.map(|(depth, gain, local)| Vec4::new(depth, gain, if local { 1.0 } else { 0.0 }, 0.0)),
                    anti_tiling: anti,
                    overflow_anti_tiling: if anti { u32::MAX } else { 0 },
                    repeat: if anti { 256.0 } else { 0.0 },
                };
                let quadrant = DetailQuadrant {
                    base_diffuse: c.base.map(|_| 0), overlays: vec![(1, None, false); n + m], folded: n, features,
                    base_rgb: Vec3::from_array(c.base_rgb), tile: 256.0, ..default()
                };
                let words = quadrant.words();
                let detail = detail_mirror(&words, &c);
                // Height blending needs folded overlays (the words drop it without them).
                let near = near_mirror(&c, height.filter(|_| n > 0), anti);
                for (d, e) in detail.iter().zip(near) {
                    assert!((d - e).abs() < 2e-5, "case {case} switches {switches}: detail {detail:?} near {near:?}");
                }
                // #3789: whatever the texels' alpha.
                let clear = DetailCase {
                    folded: c.folded.iter().map(|l| ([l.0[0], l.0[1], l.0[2], 0.0], l.1, l.2)).collect(),
                    overflow: c.overflow.iter().map(|l| ([l.0[0], l.0[1], l.0[2], 0.0], l.1)).collect(),
                    ..c
                };
                assert_eq!((detail_mirror(&words, &clear), near_mirror(&clear, height.filter(|_| n > 0), anti)), (detail, near), "case {case}");
                checked[switches] += 1;
            }
        }
        assert!(checked.iter().all(|&k| k >= 800), "{checked:?}");
        // The anti-tiled path reads the global reference even with a local source: a
        // texel equal to its global reference but not its local one has no height there.
        let c = DetailCase {
            base: Some([0.5, 0.5, 0.5, 1.0]), base_reference: ([0.5; 3], [0.9; 3]),
            folded: vec![([0.9, 0.9, 0.9, 1.0], 0.5, ([0.9; 3], [0.1; 3]))], overflow: vec![], tint: [1.0; 3], base_rgb: [1.0; 3],
        };
        let words = |anti: bool| DetailQuadrant {
            base_diffuse: Some(0), overlays: vec![(1, None, false)], folded: 1, base_rgb: Vec3::ONE, tile: 256.0,
            features: DetailFeatures { height_blend: Some(Vec4::new(DEPTH, 8.0, 1.0, 0.0)), anti_tiling: anti, overflow_anti_tiling: 0, repeat: 256.0 }, ..default()
        }.words();
        let even = [0.7; 3];
        assert_ne!(detail_mirror(&words(false), &c), even, "local: the layer stands out against its local reference");
        let anti = detail_mirror(&words(true), &c);
        assert!(anti.iter().all(|x| (x - 0.7).abs() < 1e-6), "anti-tiled, global: both flat, an even mix ({anti:?})");
    }

    /// [design] #2661 with #2550/#2551: the detail takes the switches from the very
    /// materials drawn (terrain_surface.rs `ResolvedDetail::from_materials`): a folded base's
    /// key and uniform, a single base's and each overflow layer's NifSurface key, and one
    /// shared texture repeat for every anti-tiled read.
    #[test]
    fn the_detail_reads_the_switches_from_the_materials_drawn() {
        use crate::terrain_surface::ResolvedDetail;
        use nvf_assets::nif_surface_blend::DetailFeatures;
        let image = Handle::<Image>::default();
        let base = surface_material(Some(image.clone()), None);
        let layer = |repeat: f32, on: bool| terrain_nif_material(StandardMaterial { alpha_mode: AlphaMode::Blend, depth_bias: sort_first(4.0), ..surface_material(Some(image.clone()), None) }, repeat, on);
        let folded = |height_blend: bool, anti_tiling: bool| TerrainLayers { count: 1, diffuse0: Some(image.clone()), height_blend, anti_tiling, texture_repeat: 256.0, tile: 256.0, ..default() };
        let off = ResolvedDetail::from_materials(&base, None, Some(&folded(false, false)), &[&layer(256.0, false)], 2).unwrap();
        assert_eq!(off.features, DetailFeatures::default(), "neither switch");
        let on = folded(true, true);
        let both = ResolvedDetail::from_materials(&base, None, Some(&on), &[&layer(256.0, true)], 2).unwrap();
        assert_eq!(both.features, DetailFeatures { height_blend: Some(TerrainLayersUniform::from(&on).height), anti_tiling: true, overflow_anti_tiling: 0b10, repeat: 256.0 });
        assert_eq!(both.features.height_blend, Some(height_blend_params().uniform()), "the start-up parameters the near draw's uniform carries");
        let height_only = ResolvedDetail::from_materials(&base, None, Some(&folded(true, false)), &[&layer(256.0, false)], 2).unwrap();
        assert_eq!((height_only.features.anti_tiling, height_only.features.overflow_anti_tiling, height_only.features.repeat), (false, 0, 0.0));
        assert!(height_only.features.height_blend.is_some());
        // A single base: anti-tiling from its own key; height blending never.
        let single = terrain_nif_material(base.clone(), 256.0, true);
        let r = ResolvedDetail::from_materials(&single.base, Some(&single.extension), None, &[&layer(256.0, true)], 1).unwrap();
        assert_eq!(r.features, DetailFeatures { height_blend: None, anti_tiling: true, overflow_anti_tiling: 0b1, repeat: 256.0 });
        // Anti-tiled reads at different repeats cannot share the detail's one footprint.
        assert_eq!(ResolvedDetail::from_materials(&base, None, Some(&on), &[&layer(300.0, true)], 2).err(), Some("anti-tiling-repeat"));
        // A repeat that cannot place the reads keeps the plain reads, as the near draw does.
        let plain = ResolvedDetail::from_materials(&single.base, Some(&terrain_nif_material(base.clone(), 0.0, true).extension), None, &[], 0).unwrap();
        assert_eq!(plain.features, DetailFeatures::default());
    }

    /// [design] #2661 with #2550/#2551: the settings rows switch the detail and the colour
    /// cache while the game runs. The rows flip the source materials in place
    /// (`sync_height_blend`, `sync_anti_tiling`); their Modified events (written here as the
    /// asset system writes them for an in-place edit) make terrain_surface.rs
    /// `refresh_captures` read the detail's switches again and remake the bake copies, a
    /// folded copy with both pipeline switches beside `surface_bake`, a single-texture copy
    /// of an anti-tiled layer reading anti-tiled; and back.
    #[test]
    fn the_switches_reach_the_detail_and_the_bake_copies_while_the_game_runs() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::graphics::{Graphics, Preset};
        use crate::terrain_surface::{CaptureDraw, CapturePath, CaptureSource, DetailBase, DetailCapture, SurfaceCaptureKey, SurfaceCaptureMaterial, SurfaceCell};
        let mut app = App::new();
        app.insert_resource(Graphics::preset(Preset::Vanilla)).insert_resource(TerrainSettings { tile: 256.0 })
            .init_resource::<Assets<Image>>().init_resource::<Assets<NifMaterial>>().init_resource::<Assets<TerrainMaterial>>()
            .init_resource::<Messages<AssetEvent<NifMaterial>>>().init_resource::<Messages<AssetEvent<TerrainMaterial>>>()
            .add_systems(Update, (sync_height_blend, sync_anti_tiling));
        let (diffuse, layer_diffuse) = { let mut i = app.world_mut().resource_mut::<Assets<Image>>(); (i.add(Image::default()), i.add(Image::default())) };
        let folded_material = TerrainMaterial { base: surface_material(Some(diffuse.clone()), None), extension: TerrainLayers { count: 1, diffuse0: Some(diffuse.clone()), texture_repeat: 256.0, tile: 256.0, ..default() } };
        let folded = app.world_mut().resource_mut::<Assets<TerrainMaterial>>().add(folded_material.clone());
        let layer_material = terrain_nif_material(StandardMaterial { alpha_mode: AlphaMode::Blend, ..surface_material(Some(layer_diffuse.clone()), None) }, 256.0, false);
        let layer = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(layer_material.clone());
        let n = nvf_assets::nif_surface_blend::QUAD_VERTICES;
        let parts = DetailParts {
            base: DetailBase::Folded(folded.clone()), cell_local: true, opacity: vec![vec![0.5; n]; 2], overflow: vec![layer.clone()],
            vertices: Some(DetailVertices { tint: vec![[1.0; 3]; n], normal: vec![[0.0, 0.0, 1.0]; n], tangent: vec![[1.0, 0.0, 0.0]; n], sign: TANGENT_SIGN }),
        };
        let capture = parts.capture(app.world().resource::<Assets<TerrainMaterial>>(), app.world().resource::<Assets<NifMaterial>>(), 256.0);
        let mut detail: [DetailCapture; 4] = default();
        detail[0] = capture;
        let mut cell = SurfaceCell {
            world: 0, grid: (0, 0), heights: vec![0.0; SIDE * SIDE], quadrants: vec![], paths: [CapturePath::HeightOnly("not-captured"); 4], content_revision: 0,
            sources: vec![CaptureSource::Folded(folded.clone()), CaptureSource::Single(layer.clone(), true)], dependencies: default(), detail,
        };
        let mesh = || Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        cell.capture(0, vec![(mesh(), CaptureDraw::Folded(Box::new(folded_material))), (mesh(), CaptureDraw::Single(SurfaceCaptureMaterial::from_source(&layer_material, true)))], CapturePath::Folded { layers: 1 });
        let entity = app.world_mut().spawn(cell).id();
        let state = |app: &App| {
            let cell = app.world().get::<SurfaceCell>(entity).unwrap();
            let DetailCapture::Layers(d) = &cell.detail[0] else { panic!("the detail is kept") };
            let features = d.resolved.as_ref().unwrap().features;
            let CaptureDraw::Folded(copy) = &cell.quadrants[0].1 else { panic!("the folded copy first") };
            let CaptureDraw::Single(single) = &cell.quadrants[1].1 else { panic!("then the layer's") };
            (features, TerrainLayersKey::from(&copy.extension), SurfaceCaptureKey::from(single) == SurfaceCaptureKey::from(&SurfaceCaptureMaterial::from_standard(&layer_material.base, true)), single.values.anti_tiling.x)
        };
        let switch = |app: &mut App, on: bool| {
            { let mut g = app.world_mut().resource_mut::<Graphics>(); g.terrain_height_blend = on; g.terrain_anti_tiling = on; }
            app.update();
            app.world_mut().resource_mut::<Messages<AssetEvent<TerrainMaterial>>>().write(AssetEvent::Modified { id: folded.id() });
            app.world_mut().resource_mut::<Messages<AssetEvent<NifMaterial>>>().write(AssetEvent::Modified { id: layer.id() });
            app.world_mut().run_system_once(crate::terrain_surface::refresh_captures).unwrap();
        };
        let (features, key, plain_layer, repeat) = state(&app);
        assert_eq!((features, key, plain_layer, repeat), (default(), TerrainLayersKey { height_blend: false, anti_tiling: false, surface_bake: true, parallax: false }, true, 0.0), "both off at the start");
        switch(&mut app, true);
        let (features, key, plain_layer, repeat) = state(&app);
        assert_eq!(features, nvf_assets::nif_surface_blend::DetailFeatures { height_blend: Some(height_blend_params().uniform()), anti_tiling: true, overflow_anti_tiling: 0b10, repeat: 256.0 });
        assert_eq!(key, TerrainLayersKey { height_blend: true, anti_tiling: true, surface_bake: true, parallax: false }, "the folded copy bakes with the near draw's pipeline switches");
        assert!(!plain_layer && repeat == 256.0, "the layer's copy reads anti-tiled at the near draw's repeat");
        switch(&mut app, false);
        let (features, key, plain_layer, repeat) = state(&app);
        assert_eq!((features, key, plain_layer, repeat), (default(), TerrainLayersKey { height_blend: false, anti_tiling: false, surface_bake: true, parallax: false }, true, 0.0), "and back");
    }

    /// [design] Round 10a review, second round (P3): a quadrant captured (blend turned on,
    /// or a cell backfilled) while its overflow layer's material is tinted keeps the
    /// capture with its reason (`overflow-material`), so the publication refuses it, and
    /// reverting the edit (the material refresh) restores its detail.
    #[test]
    fn an_initially_rejected_overflow_capture_recovers_when_the_edit_is_reverted() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::terrain_surface::{CaptureSource, CapturePath, DetailBase, DetailCapture, DetailPalette, SurfaceCell};
        let mut app = App::new();
        app.init_resource::<Assets<Image>>().init_resource::<Assets<NifMaterial>>().init_resource::<Assets<TerrainMaterial>>()
            .init_resource::<Messages<AssetEvent<NifMaterial>>>();
        let (diffuse, layer_diffuse) = { let mut i = app.world_mut().resource_mut::<Assets<Image>>(); (i.add(Image::default()), i.add(Image::default())) };
        let (base, layer) = {
            let mut m = app.world_mut().resource_mut::<Assets<NifMaterial>>();
            let base = m.add(NifMaterial { base: StandardMaterial { base_color_texture: Some(diffuse), ..default() }, extension: default() });
            let layer = m.add(NifMaterial { base: StandardMaterial { base_color_texture: Some(layer_diffuse), base_color: Color::srgb(1.0, 0.5, 0.5), alpha_mode: AlphaMode::Blend, ..default() }, extension: default() });
            (base, layer)
        };
        let n = nvf_assets::nif_surface_blend::QUAD_VERTICES;
        let parts = DetailParts {
            base: DetailBase::Single(base.clone()), cell_local: false, opacity: vec![vec![0.5; n]], overflow: vec![layer.clone()],
            vertices: Some(DetailVertices { tint: vec![[1.0; 3]; n], normal: vec![[0.0, 0.0, 1.0]; n], tangent: vec![[1.0, 0.0, 0.0]; n], sign: TANGENT_SIGN }),
        };
        let capture = parts.capture(app.world().resource::<Assets<TerrainMaterial>>(), app.world().resource::<Assets<NifMaterial>>(), 256.0);
        let DetailCapture::Layers(d) = &capture else { panic!("the capture is kept with its reason") };
        assert_eq!(d.resolved.as_ref().err(), Some(&"overflow-material"));
        let mut detail: [DetailCapture; 4] = default();
        detail[0] = capture;
        let entity = app.world_mut().spawn(SurfaceCell {
            world: 0, grid: (0, 0), heights: vec![0.0; SIDE * SIDE], quadrants: vec![], paths: [CapturePath::Fallback { layers: 1 }; 4], content_revision: 0,
            sources: vec![CaptureSource::Single(base.clone(), false), CaptureSource::Single(layer.clone(), true)], dependencies: default(), detail,
        }).id();
        let admitted = |app: &App| {
            let p = DetailPalette::assemble([app.world().get::<SurfaceCell>(entity).unwrap()], app.world().resource::<Assets<Image>>(), &[]);
            (p.admitted.iter().any(|(at, _)| *at == ((0, 0), 0)), p.refused.iter().find(|(at, _)| *at == ((0, 0), 0)).map(|(_, r)| *r))
        };
        assert_eq!(admitted(&app), (false, Some("overflow-material")), "refused with the capture's reason");
        // The tint reverted under the same handle: the material refresh restores the detail.
        app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&layer).unwrap().base.base_color = Color::WHITE;
        app.world_mut().resource_mut::<Messages<AssetEvent<NifMaterial>>>().write(AssetEvent::Modified { id: layer.id() });
        app.world_mut().run_system_once(crate::terrain_surface::refresh_captures).unwrap();
        assert_eq!(admitted(&app), (true, None), "layer detail once the edit is reverted");
    }

    /// [design] Round 10a: a quadrant's detail, packed and read back as the shader reads it,
    /// equals barycentric interpolation of the meshes actually drawn: the base draw's vertex
    /// colour, normal and tangent, the folded layers' opacities in their folded mesh's
    /// channels and the overflow layers' in their own meshes' vertex alpha. Both triangles,
    /// vertices, diagonals and quadrant edges, a negative cell, a stitched neighbour layer,
    /// six-layer folding and no folding, a quadrant without a base record, the 12-overlay
    /// cap (12 kept, 13 refused).
    #[test]
    fn detail_capture_reconstructs_the_drawn_triangles() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::terrain_surface::{DetailCapture, DetailPalette, SurfaceCell};
        let grid = (-17, 0);
        let heights: Vec<f32> = (0..SIDE * SIDE).map(|i| ((i * 13 + i / SIDE * 7) % 59) as f32 * 2.5 - 40.0).collect();
        let normals: Vec<[i8; 3]> = (0..SIDE * SIDE).map(|i| [((i * 7) % 41) as i8 - 20, ((i * 11) % 37) as i8 - 18, 100]).collect();
        let colors: Vec<[u8; 3]> = (0..SIDE * SIDE).map(|i| [(i * 5 % 256) as u8, (i * 9 % 256) as u8, (255 - i % 256) as u8]).collect();
        let interior = |k: usize, n: usize| -> Vec<(u16, f32)> {
            (0..n).map(|j| {
                let (r, c) = (2 + (k * 3 + j * 5) % 13, 2 + (k * 7 + j * 3) % 13);
                ((r * QUAD_SIDE + c) as u16, 0.05 + ((k * 13 + j * 29) % 19) as f32 / 20.0)
            }).collect()
        };
        let mut layers = Vec::new();
        for (q, count) in [(0u8, 7usize), (1, 13), (2, 12), (3, 2)] {
            if q != 3 { layers.push(Layer { quadrant: q, texture: Some(0xB0 + u32::from(q)), layer: -1, base: true, alpha: vec![] }); }
            for k in 0..count {
                let mut alpha = interior(k, 6);
                // q0's first layer also paints its south edge (the cell edge) and a vertex.
                if q == 0 && k == 0 { alpha.extend([(3u16, 0.9), (0, 1.0)]); }
                layers.push(Layer { quadrant: q, texture: Some(0x100 + u32::from(q) * 0x20 + k as u32), layer: k as i16, base: false, alpha });
            }
        }
        // The south neighbour paints texture 0x99 along its north edge, under our q0.
        let south = vec![Layer { quadrant: 2, texture: Some(0x99), layer: 0, base: false, alpha: (0..6).map(|k| (((QUAD_SIDE - 1) * QUAD_SIDE + k * 2) as u16, 0.3 + 0.1 * k as f32)).collect() }];
        let around = [None, None, Some(south), None];
        let land = Land { heights: Some(heights), normals: Some(normals), colors: Some(colors), layers, ..default() };
        for folds in [true, false] {
            let (mut app, _files) = crate::outfits::tests::asset_fixture();
            app.insert_resource(crate::outfits::tests::empty_world_data())
                .insert_resource(TerrainSettings { tile: 256.0 })
                .init_resource::<TerrainCache>()
                .init_resource::<Assets<TerrainMaterial>>()
                .init_resource::<Assets<crate::terrain_surface::SurfaceCaptureMaterial>>();
            app.world_mut().resource_mut::<nvf_assets::TextureCache>().offer(fnv_vfs::texture_path(DEFAULT_DIFFUSE), synthetic_dds(), 0.0);
            // Every LTEX of the fixture draws the synthetic default texture.
            let ids: Vec<u32> = land.layers.iter().filter_map(|l| l.texture).chain([0x99]).collect();
            app.world_mut().resource_mut::<TerrainCache>().textures.extend(ids.into_iter().map(|id| (id, Some(DEFAULT_DIFFUSE.to_string()))));
            let cell = app.world_mut().spawn_empty().id();
            let (prep_land, prep_around) = (land.clone(), around.clone());
            app.world_mut().run_system_once(move |mut commands: Commands, world: Res<WorldData>, mut terrain: TerrainMaterials, mut models: ModelCtx| {
                let prep = prepare_land(prep_land.clone(), &prep_around, grid, 256.0);
                spawn_terrain_with(&mut commands, cell, &world, prep, &mut TerrainCtx { tm: &mut terrain, assets: &mut models }, folds, true);
            }).unwrap();
            let reference = prepare_land(land.clone(), &around, grid, 256.0);
            let surface = app.world().get::<SurfaceCell>(cell).expect("a capture");
            let images = app.world().resource::<Assets<Image>>();
            let detail = |q: usize| match &surface.detail[q] { DetailCapture::Layers(d) => d.as_ref(), DetailCapture::Colour(r) => panic!("folds={folds} q{q}: {r}") };
            // The cap: 13 overlays keep the colour cache, 12 are composed.
            assert!(matches!(surface.detail[1], DetailCapture::Colour("overlays-over-cap")), "folds={folds}");
            assert_eq!(detail(2).opacity.len(), 12);
            let q0 = quadrant_layers(&reference.layers, 0);
            assert_eq!(q0.len(), 8, "seven own layers and the stitched one");
            assert!(q0.last().is_some_and(|(_, l)| l.texture == Some(0x99)), "the stitched layer is drawn last");
            let palette = DetailPalette::assemble([surface], images, &[]);
            assert_eq!(palette.admitted.len(), 3, "folds={folds}: q0, q2, q3");
            assert_eq!((palette.palette.images.len(), palette.palette.samplers.len()), (1, 1), "one shared texture, one sampler description");
            let slot = (1, 1);
            // #2149/#2661: packed after the stored layout's heights and after the 4 x 4
            // surface's (a wider atlas): the same words, read back alike.
            use crate::terrain_surface::HeightLayout;
            let (stored, fine_layout) = (HeightLayout::STORED, HeightLayout::new(4));
            let [data, fine] = [stored, fine_layout].map(|layout| {
                let mut data = vec![0u8; layout.bytes(true)];
                for ((_, q), quadrant) in &palette.admitted {
                    crate::terrain_surface::encode_detail(layout, &mut data, crate::terrain_surface::detail_entry(slot, *q), quadrant, detail(*q as usize));
                }
                data
            });
            for q in [0u8, 2, 3] {
                let d = detail(q as usize);
                let r = d.resolved.as_ref().unwrap();
                let drawn = quadrant_layers(&reference.layers, q);
                let folded_count = if folds { drawn.len().min(FOLDED_LAYERS) } else { 0 };
                assert_eq!((r.folded, d.opacity.len(), d.cell_local, d.tangent_sign), (folded_count, drawn.len(), folds, TANGENT_SIGN), "folds={folds} q{q}");
                let words: Vec<u32> = (0..nvf_assets::nif_surface_blend::DETAIL_QUADRANT_WORDS).map(|i| crate::terrain_surface::detail_word(stored, &data, nvf_assets::nif_surface_blend::detail_quadrant_word(crate::terrain_surface::detail_entry(slot, q)) + i)).collect();
                let fine_words: Vec<u32> = (0..nvf_assets::nif_surface_blend::DETAIL_QUADRANT_WORDS).map(|i| crate::terrain_surface::detail_word(fine_layout, &fine, nvf_assets::nif_surface_blend::detail_quadrant_word(crate::terrain_surface::detail_entry(slot, q)) + i)).collect();
                assert_eq!(fine_words, words, "folds={folds} q{q}: the table entry at the 4 x 4 surface's width");
                assert_eq!(((words[0] >> 8) & 15, (words[0] >> 12) & 15, words[0] & 2 != 0), (folded_count as u32, drawn.len() as u32, folds), "folds={folds} q{q}: table counts");
                let quad = &reference.quads[q as usize];
                let base = if folds { quad.folded.as_ref().unwrap() } else { quad.base.as_ref().unwrap() };
                let attr = |name, i: u32| -> Vec3 { match base.attribute(name) {
                    Some(VertexAttributeValues::Float32x3(v)) => Vec3::from_array(v[i as usize]),
                    Some(VertexAttributeValues::Float32x4(v)) => Vec3::new(v[i as usize][0], v[i as usize][1], v[i as usize][2]),
                    _ => panic!(),
                }};
                // The reference opacity of overlay k at vertex i: a folded channel of the base
                // draw's mesh, or the overflow layer mesh's vertex alpha.
                let opacity = |k: usize, i: u32| -> f32 {
                    let i = i as usize;
                    if k < folded_count {
                        let f32x2 = |a| match base.attribute(a) { Some(VertexAttributeValues::Float32x2(v)) => v[i], _ => panic!() };
                        let Some(VertexAttributeValues::Float32x4(col)) = base.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!() };
                        let Some(VertexAttributeValues::Float32x4(tan)) = base.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
                        [col[i][3], f32x2(Mesh::ATTRIBUTE_UV_1)[0], f32x2(Mesh::ATTRIBUTE_UV_1)[1], tan[i][3].abs() - 1.0, f32x2(Mesh::ATTRIBUTE_UV_0)[0], f32x2(Mesh::ATTRIBUTE_UV_0)[1]][k]
                    } else {
                        let Some(VertexAttributeValues::Float32x4(col)) = quad.layers[&drawn[k].0].attribute(Mesh::ATTRIBUTE_COLOR) else { panic!() };
                        col[i][3]
                    }
                };
                let half = QUAD_SIDE - 1;
                let (qx, qy) = (half * (q as usize % 2), half * (q as usize / 2));
                for local in [(0usize, 0usize), (15, 15), (7, 3), (0, 15), (15, 0), (4, 11)] {
                    for f in [Vec2::new(0.25, 0.75), Vec2::new(0.75, 0.25), Vec2::splat(0.5), Vec2::ZERO, Vec2::new(0.999, 0.0), Vec2::new(0.0, 0.999), Vec2::new(0.6, 0.6)] {
                        let square = (qx + local.0, qy + local.1);
                        let p = (Vec2::new(square.0 as f32, square.1 as f32) + f) * SPACING;
                        let (corners, weights) = mesh_mix(base, p).expect("inside a drawn triangle");
                        let expect = |name| corners.iter().zip(weights).map(|(&i, w)| attr(name, i) * w).sum::<Vec3>();
                        let (tint, normal, tangent, opacities) = shader_mix(stored, &data, crate::terrain_surface::detail_entry(slot, q), q, square, f, d.opacity.len());
                        assert_eq!(shader_mix(fine_layout, &fine, crate::terrain_surface::detail_entry(slot, q), q, square, f, d.opacity.len()), (tint, normal, tangent, opacities.clone()), "folds={folds} q{q}: the records at the 4 x 4 surface's width");
                        let close = |a: Vec3, b: Vec3| (a - b).abs().max_element() < 2e-4;
                        assert!(close(tint, expect(Mesh::ATTRIBUTE_COLOR)), "folds={folds} q{q} {square:?} {f:?}: tint {tint:?}");
                        assert!(close(normal, expect(Mesh::ATTRIBUTE_NORMAL)), "folds={folds} q{q} {square:?} {f:?}: normal");
                        assert!(close(tangent, expect(Mesh::ATTRIBUTE_TANGENT)), "folds={folds} q{q} {square:?} {f:?}: tangent");
                        for (k, o) in opacities.iter().enumerate() {
                            let want: f32 = corners.iter().zip(weights).map(|(&i, w)| opacity(k, i) * w).sum();
                            assert!((o - want).abs() < 2e-4, "folds={folds} q{q} {square:?} {f:?}: overlay {k} {o} != {want}");
                        }
                    }
                }
            }
            // Interpolated normals vary across a triangle (the fixture's LAND normals differ
            // per vertex), so a tangent recomputed from the interpolated normal would not be
            // the interpolated tangent the shader reads.
            let d = detail(0);
            assert!(d.normal[0] != d.normal[1] && d.tangent[0] != d.tangent[1]);
        }
    }

    #[test]
    fn quadrants_across_each_side() {
        assert_eq!(across(0, EAST), (None, 1));
        assert_eq!(across(1, EAST), (Some(EAST), 0));
        assert_eq!(across(0, WEST), (Some(WEST), 1));
        assert_eq!(across(2, SOUTH), (None, 0));
        assert_eq!(across(0, SOUTH), (Some(SOUTH), 2));
        assert_eq!(across(3, NORTH), (Some(NORTH), 1));
        assert_eq!(across(1, NORTH), (None, 3));
        // A side's positions meet the opposite side's of the quadrant across.
        assert_eq!(edge_position(NORTH, 4), L * QUAD_SIDE + 4);
        assert_eq!(edge_position(SOUTH, 4), 4);
        assert_eq!(edge_position(EAST, 4), 4 * QUAD_SIDE + L);
        assert_eq!(edge_position(WEST, 4), 4 * QUAD_SIDE);
    }
    #[test]
    fn bc5_terrain_base_blended_and_all_folded_slots_use_normal_role() {
        use bevy::ecs::system::SystemState;
        let (_inputs, mut world) = bc5_fixture::inputs();
        world.insert_resource(TerrainSettings { tile: 256.0 });
        world.init_resource::<TerrainCache>();
        world.init_resource::<Assets<TerrainMaterial>>();
        let mut state = SystemState::<(ModelCtx, TerrainMaterials)>::new(&mut world);
        let (mut assets, mut tm) = state.get_mut(&mut world).unwrap();
        for (normal, size) in [("signed.dds", UVec2::splat(4)), ("partial.dds", UVec2::new(3, 5)), ("unsigned.dds", UVec2::splat(4))] {
            // Independent fresh material ownership for each normal; one texture cache.
            *tm.cache = TerrainCache::default();
            tm.cache.normals.insert("old.dds".into(), normal.into());
            let linear = assets.textures.get(normal, false, &assets.data, &mut assets.images, None).unwrap();
            let mut ctx = TerrainCtx { tm: &mut tm, assets: &mut assets };
            let base = ctx.base_material(Some("old.dds".into()));
            let blended = ctx.layer_material("old.dds".into(), 1).unwrap();
            let layers = vec!["old.dds".to_string(); FOLDED_LAYERS];
            let folded = ctx.folded_material(Some("old.dds".into()), &layers, 256.0).unwrap();
            let material = ctx.assets.materials.get(&base).unwrap();
            let normal_h = material.base.normal_map_texture.as_ref().unwrap().clone();
            bc5_fixture::normal(&ctx.assets.images, &normal_h, size);
            assert_ne!(normal_h, linear);
            assert!(material.base.flip_normal_map_y);
            assert_eq!(material.base.reflectance, 0.2);
            let material = ctx.assets.materials.get(&blended).unwrap();
            assert_eq!(material.base.normal_map_texture, Some(normal_h.clone()));
            assert_eq!(material.base.alpha_mode, AlphaMode::Blend);
            let material = ctx.tm.folded.get(&folded).unwrap();
            assert_eq!(material.base.normal_map_texture, Some(normal_h.clone()));
            let ext = &material.extension;
            assert_eq!(ext.count, FOLDED_LAYERS as u32);
            for slot in [&ext.normal0, &ext.normal1, &ext.normal2, &ext.normal3, &ext.normal4, &ext.normal5] {
                assert_eq!(slot.as_ref(), Some(&normal_h));
            }
            assert_eq!(ctx.base_material(Some("old.dds".into())), base);
            assert_eq!(ctx.layer_material("old.dds".into(), 1), Some(blended));
            assert_eq!(ctx.folded_material(Some("old.dds".into()), &layers, 256.0), Some(folded));
        }
        let mut ctx = TerrainCtx { tm: &mut tm, assets: &mut assets };
        ctx.tm.cache.normals.insert("old.dds".into(), "old.dds".into());
        let old = ctx.surface(Some("old.dds"));
        assert!(old.normal_map_texture.is_some());
        let image = ctx.assets.images.get(old.normal_map_texture.as_ref().unwrap()).unwrap();
        assert_eq!(image.texture_descriptor.format, bevy::render::render_resource::TextureFormat::Bc3RgbaUnorm);
        assert_eq!(image.data.as_deref(), Some([255u8; 16].as_slice()));
        assert_eq!(old.perceptual_roughness, 0.95);
        ctx.tm.cache.normals.insert("old.dds".into(), "missing.dds".into());
        let missing = ctx.surface(Some("old.dds"));
        assert!(missing.normal_map_texture.is_none());
        assert!(missing.base_color_texture.is_some());
        assert!(ctx.surface(None).normal_map_texture.is_none());
    }

    #[test]
    fn bc5_folded_normals_are_admitted_independently_of_the_base_normal() {
        use bevy::ecs::system::SystemState;
        let (_inputs, mut world) = bc5_fixture::inputs();
        world.insert_resource(TerrainSettings { tile: 256.0 });
        world.init_resource::<TerrainCache>();
        world.init_resource::<Assets<TerrainMaterial>>();
        let mut state = SystemState::<(ModelCtx, TerrainMaterials)>::new(&mut world);
        let (mut assets, mut tm) = state.get_mut(&mut world).unwrap();
        let layers: Vec<String> = ["old.dds", "OLD.dds", "Old.dds", "oLd.dds", "olD.dds", "OLD.DDS"].into_iter().map(str::to_string).collect();
        let normals = ["signed.dds", "partial.dds", "unsigned.dds", "signed.dds", "partial.dds", "unsigned.dds"];
        for (layer, normal) in layers.iter().zip(normals) { tm.cache.normals.insert(layer.clone(), normal.into()); }
        let mut ctx = TerrainCtx { tm: &mut tm, assets: &mut assets };
        for count in [1, FOLDED_LAYERS] {
            let h = ctx.folded_material(None, &layers[..count], 256.0).unwrap();
            let material = ctx.tm.folded.get(&h).unwrap();
            assert!(material.base.normal_map_texture.is_none(), "base normal cannot stand in for a folded slot");
            let ext = &material.extension;
            assert_eq!(ext.count, count as u32);
            let slots = [&ext.normal0, &ext.normal1, &ext.normal2, &ext.normal3, &ext.normal4, &ext.normal5];
            for (i, slot) in slots.into_iter().enumerate() {
                if i < count {
                    bc5_fixture::normal(&ctx.assets.images, slot.as_ref().unwrap(), if i % 3 == 1 { UVec2::new(3, 5) } else { UVec2::splat(4) });
                    if i >= 3 { assert_eq!(slot, slots[i - 3]); }
                } else { assert!(slot.is_none()); }
            }
            assert_eq!(ctx.folded_material(None, &layers[..count], 256.0), Some(h));
        }
    }

}
