//! [design] Enabled-only interface; ordinary NifSurface keeps main's interface.
//!
//! The blend type has the same bindings as the ordinary [`NifSurface`]: uniform 50 (a
//! longer uniform), texture 51, sampler 52 and texture 53. Where the
//! ordinary type binds its holes texture, that texture's sampler and the FaceGen offset,
//! the blend type binds the landscape colour atlas, its sampler and the height atlas.
//! Blend clones are made only from materials without holes or FaceGen
//! (nvforever `mesh_terrain_blend.rs::eligible_material`), so neither ordinary use is
//! needed, and a blend pipeline binds no more textures in any stage than the ordinary
//! NIF pipeline it replaces (the view and mesh groups are the same mesh pipeline's).
//!
//! [design] Round 10a, landscape detail: with bindless textures the blend type's index
//! table (binding 100) is longer, 50..[`PALETTE_END`] instead of the ordinary 50..57 (whose 54-56, the window
//! environment map's, the blend type has not: a shape that reflects is never given it). After the four
//! ordinary entries come [`PALETTE_IMAGES`] texture entries and [`PALETTE_SAMPLERS`]
//! sampler entries: the landscape layer textures (diffuse and normal maps) and samplers a
//! publication's detail quadrants sample ([`TerrainPalette`]). Their slots are local to
//! the material's slab, so they are registered through this type's own allocator and
//! resolved through the material's own table row, never copied from the landscape's
//! materials. Unused entries hold the fallback image and sampler. Without bindless the
//! type's layout is exactly [`NifSurfaceBlendColour`]'s (uniform 50, textures 51 and 53,
//! sampler 52): [`NifSurfaceBlend`]'s `bind_group_layout_entries` is that type's, and the
//! palette is bound only in bindless mode, where it adds no binding, only table entries.
use super::nif_material::*;
use bevy::prelude::*;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_resource::{AsBindGroup, AsBindGroupError, BindGroupBuilder, BindGroupLayout, BindGroupLayoutEntry, BindlessDescriptor, BindlessIndex, BindlessIndexTableDescriptor, BindlessResourceType, BindlessSlabResourceLimit, SamplerBindingType, ShaderType, RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureViewDimension, UnpreparedBindingResource};
use bevy::render::renderer::RenderDevice;
use bevy::ecs::system::SystemParamItem;
use bevy::shader::{ShaderRef, ShaderDefVal};
// [design] Shared CPU packing and shader specialization dimensions. The colour atlas's
// resolution is not one of them: the shader reads it from the bound texture.
/// LAND grid squares per cell side: the quadrants and the landscape detail's 17 by 17
/// vertices per quadrant are on this grid, whatever the surface's subdivision.
pub const GRID: usize = fnv_world::land::SIDE - 1;
/// [design] #2149/#2661: the height atlas's layout for a run whose landscape surface has
/// `sub` squares per LAND grid square and side (1 as stored, 2 or 4 smoothed;
/// `fnv_world::land_surface::Surface::sub`, fixed for the run by nvf_play
/// `smooth_terrain::spec`). Each of the 3 by 3 cell tiles holds one pair of triangle planes
/// per surface square ([`HeightLayout::grid`] a side, 6 texels per square: two triangles,
/// three coefficients each) and one metadata row below them (residency, colour mask, detail
/// mask), so the atlas holds the very triangles the ground is drawn and collided as
/// (`grid_triangles`). With `sub` 1 it is the stored layout (576 by 99 texels of heights).
/// The publication that packs an atlas records its layout, and every blend material binding
/// that atlas carries it into its shader definitions ([`blend_shader_defs`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
pub struct HeightLayout { sub: u32 }
impl Default for HeightLayout {
    fn default() -> Self { Self::STORED }
}
impl HeightLayout {
    /// The stored LAND triangles (no smoothing).
    pub const STORED: Self = Self { sub: 1 };
    /// The layout of a surface with `sub` (1 to 4) squares per LAND square and side.
    pub const fn new(sub: usize) -> Self {
        assert!(matches!(sub, 1..=4), "a surface has 1 to 4 squares per LAND square and side");
        Self { sub: sub as u32 }
    }
    pub const fn sub(self) -> usize { self.sub as usize }
    /// Surface squares per cell side.
    pub const fn grid(self) -> usize { GRID * self.sub() }
    /// Surface vertices per cell side (`Surface::side`): a cell's heights are `side * side`.
    pub const fn side(self) -> usize { self.grid() + 1 }
    pub const fn tile_width(self) -> usize { self.grid() * 6 }
    /// A tile's rows: one per surface square, then the metadata row.
    pub const fn tile_height(self) -> usize { self.grid() + 1 }
    /// The atlas's width and its rows of heights and metadata (3 by 3 tiles); a publication
    /// with landscape detail appends [`HeightLayout::detail_rows`] rows below them.
    pub const fn width(self) -> usize { 3 * self.tile_width() }
    pub const fn rows(self) -> usize { 3 * self.tile_height() }
    pub const fn detail_rows(self) -> usize { DETAIL_WORDS.div_ceil(self.width()) }
    /// The atlas's size in bytes (RGBA8, one 32-bit value per texel), with or without the
    /// detail rows.
    pub const fn bytes(self, detail: bool) -> usize {
        self.width() * (self.rows() + if detail { self.detail_rows() } else { 0 }) * 4
    }
    /// Game units between neighbouring surface vertices.
    pub fn spacing(self) -> f32 { fnv_world::land::SPACING / self.sub as f32 }
}
/// [design] Round 10a's landscape detail, packed after the height rows as 32-bit words,
/// one per RGBA8 texel (read with `textureLoad`, as the heights are): first a table of
/// [`DETAIL_QUADRANT_WORDS`] words for each of the 36 quadrants (slot-major, quadrant
/// minor), then [`DETAIL_VERTEX_WORDS`] words for each of each quadrant's 17 by 17 LAND
/// vertices. Word `w` sits at texel `(w % width, rows + w / width)` of the layout
/// ([`HeightLayout::width`], [`HeightLayout::rows`]), so the table follows the heights
/// at any width; the words themselves do not depend on the layout.
pub const DETAIL_QUADRANTS: usize = 36;
pub const DETAIL_QUADRANT_WORDS: usize = 32;
pub const QUAD_VERTICES: usize = fnv_world::land::QUAD_SIDE * fnv_world::land::QUAD_SIDE;
/// [design] A vertex record: 22 16-bit values in 11 words, in this order: linear red,
/// green, blue (unorm16), the vertex normal x, y, z and the tangent x, y, z (game space,
/// snorm16), then one opacity per overlay (unorm16), the last value padding.
pub const DETAIL_VERTEX_WORDS: usize = 11;
pub const DETAIL_WORDS: usize = DETAIL_QUADRANTS * DETAIL_QUADRANT_WORDS + DETAIL_QUADRANTS * QUAD_VERTICES * DETAIL_VERTEX_WORDS;
/// [design] NVForever's cap (main session's round-10 decision): a quadrant's base and at
/// most 12 overlays, the folded ones and then the separately drawn overflow ones, in
/// their drawing order. 12 opacities fit three vec4s; a quadrant with more overlays keeps
/// the colour cache.
pub const DETAIL_MAX_OVERLAYS: usize = 12;
/// [design] Palette entries per material: up to 384 distinct layer images and 16 distinct
/// sampler descriptions per publication, so one material's table stays well inside the
/// 512-resource slab with its own textures, the two atlases and its data.
pub const PALETTE_IMAGES: usize = 384;
pub const PALETTE_SAMPLERS: usize = 16;
pub const PALETTE_ENTRIES: usize = PALETTE_IMAGES + PALETTE_SAMPLERS;
/// The first bindless index after the ordinary four, and the end of the blend type's index
/// table (the attribute on [`NifSurfaceBlendColour`] spells the number out; a test checks).
pub const PALETTE_FIRST_INDEX: u32 = 54;
pub const PALETTE_END: u32 = PALETTE_FIRST_INDEX + PALETTE_ENTRIES as u32;
/// [design] Initial tuning (round 10 plan): layer detail in full to 2048 game units from
/// the camera, mixed towards the colour cache over 2048-4096, colour alone beyond.
pub const DETAIL_NEAR: f32 = 2048.0;
pub const DETAIL_FAR: f32 = 4096.0;
/// [design] Colour texels per cell side the aid accepts. The default (and the settings
/// row's) is the smallest since round 10b: near the camera the landscape's own layers serve
/// the blend, and the colour cache, now with its per-cell mip chain, is the fallback beyond
/// them (NVForever's choice, round-10 plan). The aid's explicit 1024 and 2048 stay.
pub const RESOLUTIONS: [u32; 3] = [512, 1024, 2048];
pub const DEFAULT_RESOLUTION: u32 = 512;
/// [design] NVForever's contact constants (initial tuning choices, not taken from another
/// renderer): surfaces turned at least 35 degrees from the ground get the full contact
/// weight, those within 15 degrees none; 2 units below the ground count as penetrating.
/// Millionths, so the shader (via [`blend_shader_defs`]) and [`contact_weight`] share
/// the same numbers.
pub const CONTACT_COS_FULL_E6: u32 = 819_152; // cos 35 degrees
pub const CONTACT_COS_NONE_E6: u32 = 965_926; // cos 15 degrees
pub const PENETRATION_E6: u32 = 2_000_000;
/// [design] Hybrid v2's band-width limits (NVForever's round-6 tuning choices): a surface
/// parallel to the ground gets the flat width, ramping smoothly to the shallow width (the
/// band) at 5 degrees (the flat width is not held over 0-5 degrees), those turned 5 to 35
/// degrees the band, and those turned 75 degrees or more the steep width; the steep width
/// ramps in between 35 and 75 degrees ([`CONTACT_COS_FULL_E6`] is cos 35).
pub const V2_COS_PARALLEL_E6: u32 = 996_195; // cos 5 degrees
pub const V2_COS_STEEP_E6: u32 = 258_819; // cos 75 degrees
/// [design] The steepness fade (#2661 review; NVForever's choice): everything the blend
/// paints is placed by world xy, so on a near-vertical face one row of texels stretches down
/// the face (the streaked road lips of the user's play test). The contact weight is kept
/// whole on faces up to 60 degrees from horizontal and is 0 from 80 degrees, smooth between
/// ([`steepness_fade`]); the cosines of those angles against game up.
pub const STEEP_COS_FULL_E6: u32 = 500_000; // cos 60 degrees
pub const STEEP_COS_NONE_E6: u32 = 173_648; // cos 80 degrees
/// The landscape's own material values (nvforever `terrain.rs::surface` uses these), the
/// targets of the full shading blend.
pub const LAND_PERCEPTUAL_ROUGHNESS: f32 = 0.95;
pub const LAND_REFLECTANCE: f32 = 0.2;
pub fn micro(value: u32) -> f32 { value as f32 / 1e6 }
fn to_micro(value: f32) -> u32 { (value * 1e6).round() as u32 }

/// [design] The blend's shader definitions for a height atlas packed with `layout`
/// (#2149/#2661: the surface's subdivision, `MESH_TERRAIN_SUB`, and the tile and atlas
/// dimensions that follow from it; `MESH_TERRAIN_SUBDIVIDED` only where it is above 1, so
/// the stored layout's lookup is today's).
pub fn blend_shader_defs(layout: HeightLayout) -> Vec<ShaderDefVal> {
    vec![
        ShaderDefVal::UInt("MESH_TERRAIN_GRID".into(), GRID as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_SUB".into(), layout.sub() as u32),
        ShaderDefVal::Bool("MESH_TERRAIN_SUBDIVIDED".into(), layout.sub() > 1),
        ShaderDefVal::UInt("MESH_TERRAIN_HEIGHT_TILE_WIDTH".into(), layout.tile_width() as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_HEIGHT_TILE_HEIGHT".into(), layout.tile_height() as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_CELL_SIZE".into(), fnv_world::CELL_SIZE as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_SPACING".into(), fnv_world::land::SPACING as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_COS_FULL_E6".into(), CONTACT_COS_FULL_E6),
        ShaderDefVal::UInt("MESH_TERRAIN_COS_NONE_E6".into(), CONTACT_COS_NONE_E6),
        ShaderDefVal::UInt("MESH_TERRAIN_PENETRATION_E6".into(), PENETRATION_E6),
        ShaderDefVal::UInt("MESH_TERRAIN_V2_COS_PARALLEL_E6".into(), V2_COS_PARALLEL_E6),
        ShaderDefVal::UInt("MESH_TERRAIN_V2_COS_STEEP_E6".into(), V2_COS_STEEP_E6),
        ShaderDefVal::UInt("MESH_TERRAIN_STEEP_COS_FULL_E6".into(), STEEP_COS_FULL_E6),
        ShaderDefVal::UInt("MESH_TERRAIN_STEEP_COS_NONE_E6".into(), STEEP_COS_NONE_E6),
        ShaderDefVal::UInt("MESH_TERRAIN_ROUGHNESS_E6".into(), to_micro(LAND_PERCEPTUAL_ROUGHNESS)),
        ShaderDefVal::UInt("MESH_TERRAIN_REFLECTANCE_E6".into(), to_micro(LAND_REFLECTANCE)),
        // Round 10a's landscape detail (the module notes; nvforever terrain_surface.rs packs it).
        ShaderDefVal::UInt("MESH_TERRAIN_HEIGHT_WIDTH".into(), layout.width() as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_DETAIL_ROW".into(), layout.rows() as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_DETAIL_QUADRANT_WORDS".into(), DETAIL_QUADRANT_WORDS as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_DETAIL_VERTEX_WORDS".into(), DETAIL_VERTEX_WORDS as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_DETAIL_QUADRANTS".into(), DETAIL_QUADRANTS as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_QUAD_VERTICES".into(), QUAD_VERTICES as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_DETAIL_MAX_OVERLAYS".into(), DETAIL_MAX_OVERLAYS as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_PALETTE_IMAGES".into(), PALETTE_IMAGES as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_PALETTE_ENTRIES".into(), PALETTE_ENTRIES as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_DETAIL_NEAR".into(), DETAIL_NEAR as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_DETAIL_FAR".into(), DETAIL_FAR as u32),
        ShaderDefVal::UInt("MESH_TERRAIN_FLAG_TWO_COMPONENT".into(), bevy::pbr::StandardMaterialFlags::TWO_COMPONENT_NORMAL_MAP.bits()),
        ShaderDefVal::UInt("MESH_TERRAIN_FLAG_FLIP_Y".into(), bevy::pbr::StandardMaterialFlags::FLIP_NORMAL_MAP_Y.bits()),
    ]
}

/// [design] One quadrant's entry in the detail table (module notes; nvforever
/// terrain_surface.rs packs it from the near draw's materials and meshes, nif_surface_bindings.wesl
/// `mesh_terrain_detail` reads it). Palette numbers are this publication's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DetailQuadrant {
    /// Word 0 bit 1: the texture coordinate is the place in the cell over the repeat (a
    /// six-layer folded draw), not the place in the world.
    pub cell_local: bool,
    /// Palette numbers of the base's diffuse and normal map (`None`: the base draws with
    /// its material colour only, or without a normal map).
    pub base_diffuse: Option<u16>,
    pub base_normal: Option<u16>,
    /// Palette sampler numbers: the base diffuse sampler (every diffuse and overlay normal
    /// read) and the base normal map's.
    pub diffuse_sampler: u16,
    pub normal_sampler: u16,
    /// The overlays in drawing order: diffuse and optional normal palette numbers, and
    /// whether the normal map is a two-component one. The first `folded` are folded into
    /// the base draw (their colour takes the base material's colour and their normal maps
    /// its decoding, terrain.wesl); the rest are separately drawn overflow layers.
    pub overlays: Vec<(u16, Option<u16>, bool)>,
    pub folded: usize,
    /// The base normal map is a two-component one (StandardMaterial's rule).
    pub base_two_component: bool,
    /// Every normal map is read with green pointing down the image (terrain.rs `surface`).
    pub flip_y: bool,
    /// The base material's linear colour, texture-coordinate transform (x axis, y axis,
    /// translation) and the texture repeat in game units.
    pub base_rgb: Vec3,
    pub uv: [f32; 6],
    pub tile: f32,
    /// The vertex tangents' handedness (word 27; nvforever terrain.rs `TANGENT_SIGN`).
    pub tangent_sign: f32,
    /// Height blending and anti-tiling as the near draws have them (words 0, 28-31).
    pub features: DetailFeatures,
}

/// [design] The near draws' height blending (#2550) and anti-tiling (#2551) for a
/// quadrant's detail, read from the very materials drawn (nvforever terrain_surface.rs
/// `ResolvedDetail::from_materials`), so the detail follows the settings rows exactly when
/// the near draw does (the materials are flipped in place, nvforever terrain.rs
/// `sync_height_blend`, `sync_anti_tiling`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DetailFeatures {
    /// The folded base draw blends its folded overlays by height: its material's uniform
    /// values (nvforever terrain.rs `TerrainLayersUniform::height`: depth, gain, source 0
    /// global or 1 local, reserved). `None` without height blending or without a folded draw.
    pub height_blend: Option<Vec4>,
    /// The base draw (and so its folded overlays) reads its textures anti-tiled.
    pub anti_tiling: bool,
    /// Bit `k`: overlay `k`, a separately drawn overflow layer, reads anti-tiled.
    pub overflow_anti_tiling: u32,
    /// World units per texture repeat of every anti-tiled read (`TerrainLayers::
    /// texture_repeat`, `NifSurface::terrain_repeat`); 0 when nothing is anti-tiled.
    pub repeat: f32,
}

impl DetailQuadrant {
    /// The entry's words (the shader's `mesh_terrain_detail`).
    pub fn words(&self) -> [u32; DETAIL_QUADRANT_WORDS] {
        let mut w = [0u32; DETAIL_QUADRANT_WORDS];
        let total = self.overlays.len().min(DETAIL_MAX_OVERLAYS);
        w[0] = 1 | u32::from(self.cell_local) << 1 | u32::from(self.base_diffuse.is_some()) << 2 | u32::from(self.base_normal.is_some()) << 3
            | (self.folded.min(total) as u32) << 8 | (total as u32) << 12;
        w[1] = u32::from(self.base_diffuse.unwrap_or(0)) | u32::from(self.base_normal.unwrap_or(0)) << 16;
        w[2] = u32::from(self.diffuse_sampler) | u32::from(self.normal_sampler) << 16;
        let mut decode = u32::from(self.base_two_component) | u32::from(self.flip_y) << 1;
        for (k, &(diffuse, normal, two)) in self.overlays.iter().take(total).enumerate() {
            w[3] |= u32::from(normal.is_some()) << k;
            // Folded overlays decode with the base's flags (terrain.wesl), overflow ones
            // with their own material's.
            let two = if k < self.folded { self.base_two_component } else { two };
            decode |= (u32::from(two) | u32::from(self.flip_y) << 1) << (2 + 2 * k);
            w[15 + k] = u32::from(diffuse) | u32::from(normal.unwrap_or(0)) << 16;
        }
        w[4] = decode;
        for (i, v) in [self.base_rgb.x, self.base_rgb.y, self.base_rgb.z].into_iter().chain(self.uv).chain([self.tile]).enumerate() {
            w[5 + i] = v.to_bits();
        }
        w[27] = self.tangent_sign.to_bits();
        // [design] #2550/#2551: word 0 bit 4 height blending (folded overlays only), bit 5
        // the base draw anti-tiled, bit 6 the local height reference; word 28 the overflow
        // overlays read anti-tiled; words 29, 30 depth and gain; word 31 the anti-tiled
        // reads' repeat (nif_surface_bindings.wesl `mesh_terrain_detail`).
        let f = &self.features;
        let height = f.height_blend.filter(|_| self.folded.min(total) > 0);
        w[0] |= u32::from(height.is_some()) << 4 | u32::from(f.anti_tiling) << 5 | u32::from(height.is_some_and(|h| h.z > 0.5)) << 6;
        let below = |n: usize| (1u32 << n) - 1;
        w[28] = f.overflow_anti_tiling & below(total) & !below(self.folded.min(total));
        let h = height.unwrap_or(Vec4::ZERO);
        w[29] = h.x.to_bits();
        w[30] = h.y.to_bits();
        w[31] = f.repeat.to_bits();
        w
    }
}

/// [design] A vertex record's words (module notes): linear colour, game-space normal and
/// tangent, and the overlays' opacities (at most [`DETAIL_MAX_OVERLAYS`]).
pub fn detail_vertex_words(rgb: [f32; 3], normal: [f32; 3], tangent: [f32; 3], opacity: &[f32]) -> [u32; DETAIL_VERTEX_WORDS] {
    let unorm = |x: f32| (x.clamp(0.0, 1.0) * 65535.0).round() as u32;
    let snorm = |x: f32| u32::from((x.clamp(-1.0, 1.0) * 32767.0).round() as i16 as u16);
    let mut halves = [0u32; 2 * DETAIL_VERTEX_WORDS];
    for (i, &c) in rgb.iter().enumerate() { halves[i] = unorm(c); }
    for (i, &n) in normal.iter().chain(&tangent).enumerate() { halves[3 + i] = snorm(n); }
    for (k, &o) in opacity.iter().take(DETAIL_MAX_OVERLAYS).enumerate() { halves[9 + k] = unorm(o); }
    let mut w = [0u32; DETAIL_VERTEX_WORDS];
    for (i, pair) in halves.as_chunks::<2>().0.iter().enumerate() { w[i] = pair[0] | pair[1] << 16; }
    w
}

/// The word index of quadrant `entry`'s table entry and of vertex `v`'s record.
pub fn detail_quadrant_word(entry: usize) -> usize { entry * DETAIL_QUADRANT_WORDS }
pub fn detail_vertex_word(entry: usize, v: usize) -> usize {
    DETAIL_QUADRANTS * DETAIL_QUADRANT_WORDS + (entry * QUAD_VERTICES + v) * DETAIL_VERTEX_WORDS
}
/// The 16-bit value `h` of a vertex record, as the shader decodes it: unorm (colour,
/// opacities) or snorm (normal, tangent).
pub fn detail_unorm(words: &[u32], h: usize) -> f32 { ((words[h / 2] >> (16 * (h % 2))) & 0xffff) as f32 / 65535.0 }
pub fn detail_snorm(words: &[u32], h: usize) -> f32 { (((words[h / 2] >> (16 * (h % 2))) & 0xffff) as u16 as i16 as f32 / 32767.0).max(-1.0) }

/// [design] How the contact weight is formed (`mesh_terrain_weight` in
/// nif_surface_bindings.wesl, [`contact_weight`] here). `Hybrid` is hybrid v2 (round 6,
/// an angle-dependent band width); `HybridV1` keeps round 5's hybrid for comparisons.
/// The shader numbers stay stable; the legacy mode is appended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactShape { Height, Penetration, Hybrid, HybridV1 }
impl ContactShape {
    pub fn name(self) -> &'static str {
        match self { Self::Height => "height", Self::Penetration => "penetration", Self::Hybrid => "hybrid", Self::HybridV1 => "hybrid-v1" }
    }
    pub fn number(self) -> f32 {
        match self { Self::Height => 0.0, Self::Penetration => 1.0, Self::Hybrid => 2.0, Self::HybridV1 => 3.0 }
    }
    /// The weight formula's version the logs name: 2 for hybrid v2, 1 for the formulas
    /// round 5 introduced and round 6 left unchanged.
    pub fn weight_version(self) -> u32 {
        if self == Self::Hybrid { 2 } else { 1 }
    }
}

/// [design] Hybrid v2's band widths in game units: `flat` for surfaces parallel to the
/// ground, `band` (the aid's band, the shallow width) for shoulders turned 5-35 degrees,
/// `steep` for faces turned 75 degrees or more. Omitted widths are `band / 16` and
/// `6 band` (0.5 and 48 at the default band of 8); NVForever's own choices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactWidths { pub flat: f32, pub band: f32, pub steep: f32 }
impl ContactWidths {
    pub const FLAT_SHARE: f32 = 1.0 / 16.0;
    pub const STEEP_FACTOR: f32 = 6.0;
    pub fn from_band(band: f32) -> Self {
        Self { flat: band * Self::FLAT_SHARE, band, steep: band * Self::STEEP_FACTOR }
    }
    /// Finite, positive and ordered `flat <= band <= steep`.
    pub fn valid(&self) -> bool {
        [self.flat, self.band, self.steep].iter().all(|w| w.is_finite() && *w > 0.0)
            && self.flat <= self.band && self.band <= self.steep
    }
}
/// [design] What the weight blends: the colour alone, or colour, normal and material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shading { Albedo, Full }
impl Shading {
    pub fn name(self) -> &'static str {
        match self { Self::Albedo => "albedo", Self::Full => "full" }
    }
    pub fn number(self) -> f32 {
        match self { Self::Albedo => 0.0, Self::Full => 1.0 }
    }
}

/// WGSL's `smoothstep` for `low < high`.
pub fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
/// [design] Hybrid v2's band width for the cosine `cos_angle` between the unperturbed mesh
/// normal and the ground triangle's normal: `abs` treats opposite parallel normals alike;
/// `shallow` rises from 0 at the parallel to 1 at 5 degrees, `steep` from 0 at 35 to 1 at
/// 75 degrees, so the width is continuous in the angle with no on/off gate.
pub fn v2_band(widths: ContactWidths, cos_angle: f32) -> f32 {
    let c = cos_angle.abs().clamp(0.0, 1.0);
    let shallow = 1.0 - smoothstep(micro(V2_COS_PARALLEL_E6), 1.0, c);
    let steep = 1.0 - smoothstep(micro(V2_COS_STEEP_E6), micro(CONTACT_COS_FULL_E6), c);
    widths.flat + (widths.band - widths.flat) * shallow + (widths.steep - widths.band) * steep
}

/// [design] The CPU statement of the shader's contact weight, before the steepness fade
/// ([`steepness_fade`]) and the residency fade, which multiply it:
/// `d` is the height above the ground (negative below it), `cos_angle` the cosine between
/// the unperturbed mesh normal and the ground triangle's normal. Height, penetration and
/// hybrid v1 use only `widths.band`; hybrid v2 also the flat and steep widths.
pub fn contact_weight(shape: ContactShape, widths: ContactWidths, d: f32, cos_angle: f32) -> f32 {
    let height = 1.0 - smoothstep(0.0, widths.band, d.max(0.0));
    let penetration = 1.0 - smoothstep(-micro(PENETRATION_E6), 0.0, d);
    match shape {
        ContactShape::Height => height,
        ContactShape::Penetration => penetration,
        ContactShape::HybridV1 => {
            let turned = 1.0 - smoothstep(micro(CONTACT_COS_FULL_E6), micro(CONTACT_COS_NONE_E6), cos_angle.clamp(-1.0, 1.0));
            height * turned.max(penetration)
        }
        ContactShape::Hybrid => {
            let band = v2_band(widths, cos_angle);
            (1.0 - smoothstep(0.0, band, d.max(0.0))).max(penetration)
        }
    }
}
/// [design] The cosine between a mesh face's geometric normal and up (Bevy world space, up
/// +Y), as the shader's `mesh_terrain_face_up` takes it: the normal is the cross product
/// of the world position's screen derivatives `dx`, `dy`, so a lip whose smoothed vertex
/// normals lean towards up still reads as steep; abs treats back faces as front faces.
/// Where `dx` and `dy` are (nearly) parallel or zero the interpolated vertex normal
/// `mesh_normal` stands in, and without one either the result is 1 (no fade).
pub fn face_up(dx: Vec3, dy: Vec3, mesh_normal: Vec3) -> f32 {
    let face = dx.cross(dy);
    let face_length = face.length();
    if face_length > 1e-6 * dx.length() * dy.length() { return (face.y.abs() / face_length).min(1.0); }
    let n_length = mesh_normal.length();
    if n_length > 1e-6 { return (mesh_normal.y.abs() / n_length).min(1.0); }
    1.0
}
/// [design] The steepness fade every contact shape's weight is multiplied by
/// ([`STEEP_COS_FULL_E6`]): 1 for `up` at or above cos 60 degrees, 0 at or below cos 80,
/// smooth between.
pub fn steepness_fade(up: f32) -> f32 {
    smoothstep(micro(STEEP_COS_NONE_E6), micro(STEEP_COS_FULL_E6), up.clamp(0.0, 1.0))
}
/// [design] The geometric normal of a ground triangle in game space (x east, y north, z
/// up) from its plane's per-square slopes (`terrain_surface.rs::height_planes`), the
/// squares of `layout` (the shader's `terrain_game_normal`, over `HEIGHT_SPACING`).
pub fn terrain_game_normal(layout: HeightLayout, dx: f32, dy: f32) -> Vec3 {
    let spacing = layout.spacing();
    Vec3::new(-dx / spacing, -dy / spacing, 1.0).normalize()
}
/// Game-space direction to Bevy world space (nvforever `world.rs::game_to_bevy`).
pub fn game_to_world_normal(n: Vec3) -> Vec3 { Vec3::new(n.x, n.z, -n.y) }
/// [design] The shading normal moved towards the ground's by `w`; `w <= 0` keeps the mesh
/// normal exactly, and a mix that cancels out falls back to the ground's normal.
pub fn blend_normal(mesh: Vec3, terrain: Vec3, w: f32) -> Vec3 {
    if w <= 0.0 { return mesh; }
    let mixed = mesh.lerp(terrain, w.min(1.0));
    if mixed.length_squared() < 1e-12 { terrain } else { mixed.normalize() }
}

pub type NifBlendMaterial = ExtendedMaterial<StandardMaterial, NifSurfaceBlend>;

// [design] The source remains accessible to gameplay material producers even while
// the renderer uses the separate blend type. Fade/range state stays on the entity.
#[derive(Component)]
pub struct BlendShape {
    pub original: Handle<NifMaterial>,
    pub enabled: Option<Handle<NifBlendMaterial>>,
}
impl BlendShape {
    pub fn new(original: Handle<NifMaterial>) -> Self { Self { original, enabled: None } }
}
pub fn source_material<'a>(ordinary: Option<&'a MeshMaterial3d<NifMaterial>>, blend: Option<&'a BlendShape>) -> Option<&'a Handle<NifMaterial>> {
    ordinary.map(|m| &m.0).or_else(|| blend.map(|b| &b.original))
}
/// [design] The blend type's colour interface: every binding the type has without bindless
/// (module notes). Its index table reaches [`PALETTE_END`] (454) so [`NifSurfaceBlend`] can
/// put the landscape palette in the same table row in bindless mode.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[data(50, NifSurfaceBlendUniform, binding_array(101))]
#[bindless(limit(512), index_table(range(50..454), binding(100)))]
#[bind_group_data(NifSurfaceBlendKey)]
pub struct NifSurfaceBlendColour {
    pub falloff: Vec4,
    pub alpha_cut: f32,
    pub add_whole: f32,
    pub refraction: f32,
    pub fire_period: f32,
    pub emissive_rule: Vec4,
    pub refract: bool,
    pub decal: bool,
    pub hard_edges: bool,
    pub shade_light: bool,
    pub depth_write: bool,
    pub first_person: bool,
    pub add_linear: bool,
    pub pipboy_screen: bool,
    pub pipboy_a: Vec4,
    pub pipboy_b: Vec4,
    /// The source's landscape anti-tiling values (nif_material.rs `NifSurface::
    /// terrain_repeat`, `anti_tiling`), as the ordinary type's uniform and key carry them.
    pub terrain_repeat: f32,
    pub anti_tiling: bool,
    /// The source's authored metal gain (nif_material.rs `NifSurface::metal_gain`, #2981), in
    /// the ordinary uniform's place (`terrain.y`).
    pub metal_gain: f32,
    pub blended_cutout: Option<f32>,
    pub holes: Vec4,
    pub holes_edge: Vec4,
    pub soft: Vec4,
    /// Band, view, contact shape, shading (the aid's numbers, nvforever
    /// `mesh_terrain_blend.rs::BlendConfig::controls`).
    pub mesh_terrain: Vec4,
    /// xy immutable publication origin, z enabled coverage (nine bits), w reserved.
    pub mesh_terrain_origin: Vec4,
    /// Hybrid v2's flat and steep widths in x and y (game units); z, w unused
    /// (`BlendConfig::widths`). Enabled-only: the ordinary uniform is unchanged.
    pub mesh_terrain_widths: Vec4,
    /// In the ordinary type's holes texture and sampler slots (module notes).
    #[texture(51)]
    #[sampler(52)]
    pub mesh_terrain_colour: Option<Handle<Image>>,
    /// In the ordinary type's FaceGen offset slot (module notes).
    #[texture(53)]
    pub mesh_terrain_height: Option<Handle<Image>>,
    /// How that height atlas is packed (the publication's [`HeightLayout`]); no binding,
    /// it reaches the shader through the key ([`NifSurfaceBlendKey`], [`blend_shader_defs`]).
    pub mesh_terrain_layout: HeightLayout,
}

#[derive(Clone, Default, ShaderType)]
pub struct NifSurfaceBlendUniform {
    pub falloff: Vec4,
    pub alpha_cut: f32,
    pub add_whole: f32,
    pub refraction: f32,
    pub fire_period: f32,
    pub emissive_rule: Vec4,
    pub holes: Vec4,
    pub holes_edge: Vec4,
    pub soft: Vec4,
    pub pipboy_a: Vec4,
    pub pipboy_b: Vec4,
    /// The ordinary uniform's `terrain` (nif_material.rs `NifSurfaceUniform`: x the repeat,
    /// y the metal gain), in its place.
    pub terrain: Vec4,
    pub mesh_terrain: Vec4,
    /// xy immutable publication origin, z enabled coverage (nine bits), w reserved.
    pub mesh_terrain_origin: Vec4,
    pub mesh_terrain_widths: Vec4,
}

impl From<&NifSurfaceBlendColour> for NifSurfaceBlendUniform {
    fn from(s: &NifSurfaceBlendColour) -> Self {
        NifSurfaceBlendUniform { falloff: s.falloff, alpha_cut: s.alpha_cut, add_whole: s.add_whole, refraction: s.refraction * REFRACTION_SCREEN, fire_period: s.fire_period, emissive_rule: s.emissive_rule, holes: s.holes, holes_edge: s.holes_edge, soft: s.soft, pipboy_a: s.pipboy_a, pipboy_b: s.pipboy_b, terrain: Vec4::new(if crate::terrain_sampling::repeat_valid(s.terrain_repeat) { s.terrain_repeat } else { 0.0 }, s.metal_gain, 0.0, 0.0), mesh_terrain: s.mesh_terrain, mesh_terrain_origin: s.mesh_terrain_origin, mesh_terrain_widths: s.mesh_terrain_widths }
    }
}


/// The ordinary type's key for the source's variant, and the height atlas's layout
/// (#2149/#2661: fixed for a run, so one run specializes for one layout).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NifSurfaceBlendKey(pub NifSurfaceKey, pub HeightLayout);
impl From<&NifSurfaceBlend> for NifSurfaceBlendKey {
    fn from(s: &NifSurfaceBlend) -> Self { Self::from(&s.colour) }
}
impl From<&NifSurfaceBlendColour> for NifSurfaceBlendKey {
    fn from(s: &NifSurfaceBlendColour) -> Self {
        Self(NifSurfaceKey {
            decal: s.decal,
            refract: s.refract,
            hard_edges: s.hard_edges,
            shade_light: s.shade_light,
            // No FaceGen: slot 53 holds the height atlas (module notes).
            facegen: false,
            depth_write: s.depth_write,
            first_person: s.first_person,
            add_linear: s.add_linear,
            pipboy_screen: s.pipboy_screen,
            anti_tiling: s.anti_tiling && crate::terrain_sampling::repeat_valid(s.terrain_repeat),
            // An object blended into the landscape, not a landscape layer: bevy_pbr's rule
            // decides its ambient occlusion.
            terrain_layer: false,
            // A soft distance, as in the ordinary type's key (NVF_SOFT_FADE, #2868).
            soft: s.soft.x > 0.0,
            // HDR output, as in the ordinary type's key (a live switch prepares both again).
            hdr: crate::nif_material::hdr_output(),
        }, s.mesh_terrain_layout)
    }
}
/// [design] Round 10a: the layer images and sampler representatives of one publication's
/// detail quadrants (nvforever terrain_surface.rs `DetailPalette`), in palette order. A
/// sampler entry names an image whose GPU sampler stands for every palette image with an
/// equal sampler description. Empty without landscape detail.
///
/// [design] Round 10a review: an entry is `None` when no quadrant still covered by the
/// clones reads it (nvforever terrain_surface.rs `PublishedSurface::live_palette`, after a
/// cell's coverage was masked, e.g. because one of its images was removed). It binds the
/// fallback image or sampler, and its palette number stays reserved, so the numbers in
/// the publication's height atlas keep meaning the same entries.
#[derive(Clone, Debug, Default, PartialEq, Reflect)]
pub struct TerrainPalette {
    pub images: Vec<Option<Handle<Image>>>,
    pub samplers: Vec<Option<Handle<Image>>>,
}

/// [design] The enabled blend extension: the colour interface ([`NifSurfaceBlendColour`],
/// its fields reached through `Deref`) and, in bindless mode only, the landscape palette in
/// the same index table row (module notes).
#[derive(Asset, Reflect, Debug, Clone)]
pub struct NifSurfaceBlend {
    pub colour: NifSurfaceBlendColour,
    pub palette: TerrainPalette,
}
impl std::ops::Deref for NifSurfaceBlend {
    type Target = NifSurfaceBlendColour;
    fn deref(&self) -> &NifSurfaceBlendColour { &self.colour }
}
impl std::ops::DerefMut for NifSurfaceBlend {
    fn deref_mut(&mut self) -> &mut NifSurfaceBlendColour { &mut self.colour }
}

/// The palette's resource types after the colour interface's (bindless index 54 on).
pub fn palette_resource(index: usize) -> BindlessResourceType {
    if index < PALETTE_IMAGES { BindlessResourceType::Texture2d } else { BindlessResourceType::SamplerFiltering }
}

impl AsBindGroup for NifSurfaceBlend {
    type Data = <NifSurfaceBlendColour as AsBindGroup>::Data;
    type Param = <NifSurfaceBlendColour as AsBindGroup>::Param;

    fn bindless_slot_count() -> Option<BindlessSlabResourceLimit> { NifSurfaceBlendColour::bindless_slot_count() }
    // [design] The colour interface's own rule, so the type stays bindless exactly where
    // StandardMaterial and the ordinary NifSurface are (ExtendedMaterial combines them).
    fn bindless_supported(render_device: &RenderDevice) -> bool { NifSurfaceBlendColour::bindless_supported(render_device) }
    fn label() -> &'static str { "NifSurfaceBlend" }
    fn bind_group_data(&self) -> Self::Data { self.colour.bind_group_data() }

    fn build_bind_group(
        &self,
        layout: &BindGroupLayout,
        render_device: &RenderDevice,
        param: &mut SystemParamItem<'_, '_, Self::Param>,
        force_no_bindless: bool,
        output: &mut BindGroupBuilder,
    ) -> Result<(), AsBindGroupError> {
        self.colour.build_bind_group(layout, render_device, param, force_no_bindless, output)?;
        // Without bindless: the colour interface's bindings only (module notes).
        if force_no_bindless || !Self::bindless_supported(render_device) { return Ok(()); }
        let (images, fallback, _) = param;
        for k in 0..PALETTE_IMAGES {
            let view = match self.palette.images.get(k).and_then(Option::as_ref) {
                Some(h) => images.get(h).ok_or(AsBindGroupError::RetryNextUpdate)?.texture_view.clone(),
                None => fallback.d2.texture_view.clone(),
            };
            output.binding_resources.push((PALETTE_FIRST_INDEX + k as u32, UnpreparedBindingResource::TextureView(TextureViewDimension::D2, view)));
        }
        for j in 0..PALETTE_SAMPLERS {
            let sampler = match self.palette.samplers.get(j).and_then(Option::as_ref) {
                Some(h) => images.get(h).ok_or(AsBindGroupError::RetryNextUpdate)?.sampler.clone(),
                None => fallback.d2.sampler.clone(),
            };
            output.binding_resources.push((PALETTE_FIRST_INDEX + (PALETTE_IMAGES + j) as u32, UnpreparedBindingResource::Sampler(SamplerBindingType::Filtering, sampler)));
        }
        Ok(())
    }

    // [design] Exactly the colour interface's layout, with or without bindless: the
    // palette lives in the colour interface's longer index table and its texture and
    // sampler binding arrays (module notes).
    fn bind_group_layout_entries(render_device: &RenderDevice, force_no_bindless: bool) -> Vec<BindGroupLayoutEntry> {
        NifSurfaceBlendColour::bind_group_layout_entries(render_device, force_no_bindless)
    }

    fn bindless_descriptor() -> Option<BindlessDescriptor> {
        let mut descriptor = NifSurfaceBlendColour::bindless_descriptor()?;
        let mut resources = descriptor.resources.to_vec();
        resources.resize(PALETTE_FIRST_INDEX as usize, BindlessResourceType::None);
        resources.extend((0..PALETTE_ENTRIES).map(palette_resource));
        descriptor.resources = resources.into();
        debug_assert!(descriptor.index_tables.iter().any(|t| t.indices == (BindlessIndex(50)..BindlessIndex(PALETTE_END))));
        Some(descriptor)
    }
}

/// The palette's part of the index table ([`PALETTE_FIRST_INDEX`]..[`PALETTE_END`]) lies in
/// the colour interface's table: no table of its own.
pub fn palette_index_table(descriptor: &BindlessDescriptor) -> Option<&BindlessIndexTableDescriptor> {
    descriptor.index_tables.iter().find(|t| t.indices.start.0 <= PALETTE_FIRST_INDEX && t.indices.end.0 >= PALETTE_END)
}

impl NifSurfaceBlend {
    /// `source` has neither holes nor FaceGen (eligibility); `controls` are band, view,
    /// contact shape and shading, `widths` hybrid v2's flat and steep widths; `height` the
    /// height atlas and how it is packed.
    pub fn from_source(source: &NifSurface, controls: Vec4, widths: Vec4, origin: Vec4, colour: Handle<Image>, height: (Handle<Image>, HeightLayout), palette: TerrainPalette) -> Self {
        Self { colour: NifSurfaceBlendColour::from_source(source, controls, widths, origin, colour, height), palette }
    }
}

impl NifSurfaceBlendColour {
    /// See [`NifSurfaceBlend::from_source`].
    pub fn from_source(source: &NifSurface, controls: Vec4, widths: Vec4, origin: Vec4, colour: Handle<Image>, height: (Handle<Image>, HeightLayout)) -> Self {
        debug_assert!(source.holes_texture.is_none() && source.facegen_offset.is_none(), "blend clones are made only without holes or FaceGen");
        Self {
            falloff: source.falloff,
            alpha_cut: source.alpha_cut,
            add_whole: source.add_whole,
            refraction: source.refraction,
            fire_period: source.fire_period,
            emissive_rule: source.emissive_rule,
            refract: source.refract,
            decal: source.decal,
            hard_edges: source.hard_edges,
            shade_light: source.shade_light,
            depth_write: source.depth_write,
            first_person: source.first_person,
            add_linear: source.add_linear,
            pipboy_screen: source.pipboy_screen,
            pipboy_a: source.pipboy_a,
            pipboy_b: source.pipboy_b,
            terrain_repeat: source.terrain_repeat,
            anti_tiling: source.anti_tiling,
            metal_gain: source.metal_gain,
            blended_cutout: source.blended_cutout,
            holes: source.holes,
            holes_edge: source.holes_edge,
            soft: source.soft,
            mesh_terrain: controls,
            mesh_terrain_origin: origin,
            mesh_terrain_widths: widths,
            mesh_terrain_colour: Some(colour),
            mesh_terrain_height: Some(height.0),
            mesh_terrain_layout: height.1,
        }
    }
}
impl MaterialExtension for NifSurfaceBlend {
    fn fragment_shader() -> ShaderRef { NifSurface::fragment_shader() }
    fn deferred_fragment_shader() -> ShaderRef { NifSurface::deferred_fragment_shader() }
    fn specialize(pipeline: &MaterialExtensionPipeline, descriptor: &mut RenderPipelineDescriptor, layout: &MeshVertexBufferLayoutRef, key: MaterialExtensionKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        NifSurface::specialize(pipeline, descriptor, layout, MaterialExtensionKey::<NifSurface> { mesh_key: key.mesh_key, bind_group_data: key.bind_group_data.0 })?;
        blend_interface(descriptor, key.bind_group_data.1);
        // The landscape takes no bounce light, and this surface is partly the landscape's.
        rtgi_volume_opt_out(descriptor);
        Ok(())
    }
}

fn blend_interface(descriptor: &mut RenderPipelineDescriptor, layout: HeightLayout) {
    // The separate layout is also declared in the otherwise identical NIF prepass.
    descriptor.vertex.shader_defs.push("NIF_SURFACE_BLEND".into());
    let prepass = descriptor.vertex.shader_defs.contains(&ShaderDefVal::from("PREPASS_PIPELINE"));
    if let Some(fragment) = descriptor.fragment.as_mut() {
        fragment.shader_defs.push("NIF_SURFACE_BLEND".into());
        if !prepass {
            fragment.shader_defs.push("MESH_TERRAIN_BLEND".into());
            fragment.shader_defs.extend(blend_shader_defs(layout));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::render_resource::{BindlessDescriptor, BindlessResourceType as R, FragmentState, MultisampleState};
    #[test]
    fn enabled_interface_preserves_descriptors_and_keeps_blend_out_of_prepass() {
        for samples in [1, 4, 8] { for prepass in [false, true] {
            let mut descriptor = RenderPipelineDescriptor {
                fragment: Some(FragmentState::default()),
                multisample: MultisampleState { count: samples, ..default() },
                ..default()
            };
            if prepass { descriptor.vertex.shader_defs.push("PREPASS_PIPELINE".into()); }
            let original = descriptor.clone();
            blend_interface(&mut descriptor, HeightLayout::new(4));
            assert_eq!(descriptor.depth_stencil, original.depth_stencil);
            assert_eq!(descriptor.multisample, original.multisample);
            assert_eq!(descriptor.primitive, original.primitive);
            assert_eq!(descriptor.vertex.shader, original.vertex.shader);
            assert_eq!(descriptor.fragment.as_ref().unwrap().shader, original.fragment.as_ref().unwrap().shader);
            assert_eq!(descriptor.fragment.as_ref().unwrap().targets, original.fragment.as_ref().unwrap().targets);
            assert_eq!(descriptor.fragment.as_ref().unwrap().shader_defs.contains(&"MESH_TERRAIN_BLEND".into()), !prepass);
        }}
        assert_eq!(NifSurfaceBlendUniform::min_size().get(), 192, "the ordinary rows up to #2551's terrain row (144 bytes), then three vec4s");
        assert_eq!(crate::nif_material::NifSurfaceUniform::min_size().get(), 176, "the ordinary uniform: the same 144 bytes, then #2462's env row and #2841's env_how row, which the blend type has not");
    }

    fn textures(descriptor: &BindlessDescriptor) -> usize {
        descriptor.resources.iter().filter(|r| matches!(r, R::Texture1d | R::Texture2d | R::Texture2dArray | R::Texture3d | R::TextureCube | R::TextureCubeArray)).count()
    }
    /// [design] Without bindless every resource of these tables is its own binding, so the
    /// texture entries are the material group's sampled textures in the fragment stage.
    /// The blend pipeline's other groups (view, mesh) are the same mesh pipeline's as the
    /// ordinary NIF pipeline's, so equal material groups mean equal per-stage totals: the
    /// blend can never exceed a 16-texture stage limit the ordinary shape fits.
    #[test]
    fn blend_layout_binds_exactly_the_ordinary_slots_and_texture_count() {
        let ordinary = NifSurface::bindless_descriptor().unwrap();
        // [design] Without bindless every binding is the colour interface's (the blend
        // type's `bind_group_layout_entries` is that type's, checked below): the same slot
        // types and texture count as the ordinary NIF type.
        let colour = NifSurfaceBlendColour::bindless_descriptor().unwrap();
        // The ordinary type has two more since #2462 (the window environment map's cube and
        // mask at 54 and 56, which the blend type has not), so the blend binds fewer.
        assert_eq!((textures(&colour), textures(&ordinary)), (2, 4));
        assert_eq!(&colour.resources[50..], &ordinary.resources[50..54], "same slot types: 50 data, 51 texture, 52 sampler, 53 texture");
        assert_eq!((colour.buffers[0].binding_number.0, colour.buffers[0].bindless_index.0, colour.buffers[0].size), (101, 50, Some(192)));
        let source = include_str!("nif_surface_blend.rs");
        let fields = source.split("pub struct NifSurfaceBlendColour {").nth(1).unwrap().split("\n}").next().unwrap();
        assert_eq!(fields.matches("#[texture(").count(), 2);
        assert!(fields.contains("#[texture(51)]") && fields.contains("#[sampler(52)]") && fields.contains("#[texture(53)]"));
        assert!(!fields.contains("#[texture(54)]") && !fields.contains("holes_texture") && !fields.contains("facegen_offset"));
        let layout = source.split("fn bind_group_layout_entries(render_device: &RenderDevice, force_no_bindless: bool) -> Vec<BindGroupLayoutEntry> {").nth(1).unwrap().split("\n    }").next().unwrap();
        assert_eq!(layout.trim(), "NifSurfaceBlendColour::bind_group_layout_entries(render_device, force_no_bindless)", "no binding of its own, bindless or not");
        let build = source.split("fn build_bind_group(").nth(1).unwrap();
        assert!(build.find("if force_no_bindless || !Self::bindless_supported(render_device) { return Ok(()); }").unwrap() < build.find("PALETTE_FIRST_INDEX").unwrap(), "the palette only in bindless mode");
        // [design] Bindless (intentional change in round 10a): the same binding (100) for one
        // longer index table, 50..454, whose entries after the ordinary four are the palette's
        // 384 textures and 16 samplers; the ordinary type's table is unchanged.
        let blend = NifSurfaceBlend::bindless_descriptor().unwrap();
        assert_eq!(&blend.resources[50..54], &ordinary.resources[50..54]);
        assert_eq!(blend.resources.len(), PALETTE_END as usize);
        assert!(blend.resources[54..54 + PALETTE_IMAGES].iter().all(|r| *r == R::Texture2d));
        assert!(blend.resources[54 + PALETTE_IMAGES..].iter().all(|r| *r == R::SamplerFiltering));
        assert_eq!(blend.index_tables.len(), 1, "no second table and no second storage binding");
        let (b, o) = (&blend.index_tables[0], &ordinary.index_tables[0]);
        assert_eq!((b.indices.start.0, b.indices.end.0, b.binding_number.0), (50, PALETTE_END, 100));
        assert_eq!((o.indices.start.0, o.indices.end.0, o.binding_number.0), (50, 57, 100), "the ordinary NifSurface table is main's");
        assert_eq!(PALETTE_END, 454, "the attribute on NifSurfaceBlendColour spells it out");
        assert!(palette_index_table(&blend).is_some_and(|t| t.binding_number.0 == 100));
        assert_eq!(blend.buffers.len(), colour.buffers.len(), "no buffer of its own");
        // One material's resources stay inside the 512-resource slab: the palette, the
        // standard material's textures and samplers, the two atlases and the data.
        let standard = StandardMaterial::bindless_descriptor().unwrap().resources.iter().filter(|r| **r != R::None).count();
        assert!(PALETTE_ENTRIES + standard + 4 < 512, "{standard}");
    }

    #[test]
    fn detail_layout_fits_the_height_atlas_and_round_trips() {
        // The stored layout (smoothing off) is the one before #2149: 576 by 99 texels of
        // heights and metadata, 201 detail rows.
        let stored = HeightLayout::STORED;
        assert_eq!((stored.width(), stored.rows()), (576, 99));
        assert_eq!(DETAIL_WORDS, 36 * 32 + 36 * 289 * 11);
        assert_eq!(stored.detail_rows(), 201);
        assert_eq!((stored.bytes(false), stored.bytes(true)), (576 * 99 * 4, 576 * 300 * 4));
        // #2149/#2661: the smoothed surfaces' layouts, one plane pair per surface square.
        for (sub, width, rows, detail_rows) in [(2, 1152, 195, 101), (4, 2304, 387, 51)] {
            let l = HeightLayout::new(sub);
            assert_eq!((l.sub(), l.grid(), l.side(), l.tile_width(), l.tile_height()), (sub, 32 * sub, 32 * sub + 1, 192 * sub, 32 * sub + 1));
            assert_eq!((l.width(), l.rows(), l.detail_rows()), (width, rows, detail_rows), "{sub}");
            assert_eq!(l.side(), (fnv_world::land::SIDE - 1) * sub + 1, "Surface::side");
            assert_eq!(l.spacing(), 128.0 / sub as f32);
        }
        assert_eq!(HeightLayout::new(1), HeightLayout::STORED);
        assert_eq!(HeightLayout::default(), HeightLayout::STORED);
        assert_eq!((HeightLayout::new(2).bytes(true), HeightLayout::new(4).bytes(false), HeightLayout::new(4).bytes(true)), (1152 * 296 * 4, 2304 * 387 * 4, 2304 * 438 * 4));
        // The detail table fits after the heights at every width.
        for l in [stored, HeightLayout::new(2), HeightLayout::new(4)] {
            assert!(detail_vertex_word(DETAIL_QUADRANTS - 1, QUAD_VERTICES - 1) + DETAIL_VERTEX_WORDS <= l.detail_rows() * l.width());
            assert!((l.detail_rows() - 1) * l.width() < DETAIL_WORDS, "no whole row to spare");
        }
        assert_eq!(detail_vertex_word(0, 0), detail_quadrant_word(DETAIL_QUADRANTS));
        // Twelve opacities, colour, normal and tangent in 22 halves; the last is padding.
        let opacity: Vec<f32> = (0..12).map(|k| k as f32 / 11.0).collect();
        let w = detail_vertex_words([0.25, 1.0, 0.0], [0.6, -0.8, 0.0], [-1.0, 0.0, 0.5], &opacity);
        let close = |a: f32, b: f32, e: f32| (a - b).abs() <= e;
        for (h, v) in [(0, 0.25), (1, 1.0), (2, 0.0)] { assert!(close(detail_unorm(&w, h), v, 1.0 / 65535.0)); }
        for (h, v) in [(3, 0.6), (4, -0.8), (5, 0.0), (6, -1.0), (7, 0.0), (8, 0.5)] { assert!(close(detail_snorm(&w, h), v, 1.0 / 32767.0), "{h}"); }
        for (k, o) in opacity.iter().enumerate() { assert!(close(detail_unorm(&w, 9 + k), *o, 1.0 / 65535.0), "opacity {k}"); }
        assert_eq!((detail_unorm(&w, 9), detail_unorm(&w, 20)), (0.0, 1.0), "0 and 1 are exact");
        assert_eq!(w[10] >> 16, 0, "padding");
        // Out-of-range inputs clamp; more than twelve opacities never spill.
        let clamped = detail_vertex_words([2.0, -1.0, 0.5], [3.0, -3.0, 0.0], [0.0; 3], &[1.5; 14]);
        assert_eq!((detail_unorm(&clamped, 0), detail_unorm(&clamped, 1), detail_snorm(&clamped, 3), detail_snorm(&clamped, 4)), (1.0, 0.0, 1.0, -1.0));
        assert_eq!(clamped[10] >> 16, 0);
    }

    #[test]
    fn detail_quadrant_words_name_folded_and_overflow_overlays() {
        let q = DetailQuadrant {
            cell_local: true, base_diffuse: Some(3), base_normal: Some(4), diffuse_sampler: 1, normal_sampler: 2,
            overlays: vec![(5, Some(6), true), (7, None, false), (8, Some(9), true)], folded: 2,
            base_two_component: false, flip_y: true, base_rgb: Vec3::new(0.5, 0.25, 1.0),
            uv: [1.0, 0.0, 0.0, 1.0, 0.25, -0.5], tile: 256.0, tangent_sign: -1.0, features: DetailFeatures::default(),
        };
        let w = q.words();
        assert_eq!(w[0], 1 | 2 | 4 | 8 | 2 << 8 | 3 << 12);
        assert_eq!(&w[28..], &[0, 0, 0, 0], "neither feature: words 28-31 zero");
        assert_eq!((w[1], w[2], w[3]), (3 | 4 << 16, 1 | 2 << 16, 0b101));
        // Base: flip only. Folded overlays 0 and 1 decode as the base (two-component off even
        // where their own map is one); overflow overlay 2 with its own two-component map.
        assert_eq!(w[4], 0b10 | 0b10 << 2 | 0b10 << 4 | 0b11 << 6);
        assert_eq!((f32::from_bits(w[5]), f32::from_bits(w[7]), f32::from_bits(w[12]), f32::from_bits(w[13]), f32::from_bits(w[14])), (0.5, 1.0, 0.25, -0.5, 256.0));
        assert_eq!((w[15], w[16], w[17], w[18]), (5 | 6 << 16, 7, 8 | 9 << 16, 0));
        assert_eq!(f32::from_bits(w[27]), -1.0, "the tangents' handedness");
        // The cap: thirteen overlays keep twelve (the packer refuses such a quadrant first).
        let many = DetailQuadrant { overlays: vec![(1, None, false); 13], folded: 6, ..q.clone() };
        assert_eq!((many.words()[0] >> 12) & 15, 12);
        // #2550/#2551: the features in word 0 (bits 4-6) and words 28-31; the overflow mask
        // keeps overflow overlays only (overlay 2 here), height blending needs folded ones.
        let features = DetailFeatures { height_blend: Some(Vec4::new(0.1, 8.0, 1.0, 0.0)), anti_tiling: true, overflow_anti_tiling: 0b111, repeat: 256.0 };
        let w = DetailQuadrant { features, ..q.clone() }.words();
        assert_eq!(w[0], 1 | 2 | 4 | 8 | 2 << 8 | 3 << 12 | 1 << 4 | 1 << 5 | 1 << 6);
        assert_eq!((w[28], f32::from_bits(w[29]), f32::from_bits(w[30]), f32::from_bits(w[31])), (0b100, 0.1, 8.0, 256.0));
        let global = DetailFeatures { height_blend: Some(Vec4::new(0.1, 8.0, 0.0, 0.0)), ..features };
        assert_eq!((DetailQuadrant { features: global, ..q.clone() }.words()[0] >> 4) & 7, 0b011, "global reference: bit 6 clear");
        let unfolded = DetailQuadrant { features, folded: 0, ..q.clone() }.words();
        assert_eq!(((unfolded[0] >> 4) & 7, unfolded[28], unfolded[29]), (0b010, 0b111, 0), "no folded overlays: no height blending, every overlay overflow");
        let anti_only = DetailQuadrant { features: DetailFeatures { height_blend: None, ..features }, ..q }.words();
        assert_eq!(((anti_only[0] >> 4) & 7, anti_only[29], anti_only[30]), (0b010, 0, 0));
    }

    #[test]
    fn contact_weight_follows_band_orientation_and_penetration() {
        // Round 5's shapes, unchanged in round 6 (hybrid v1 is round 5's hybrid).
        let cos = |deg: f32| deg.to_radians().cos();
        let (parallel, sloped, upright) = (1.0, cos(25.0), 0.0);
        // Heights -2, 0, 4, 8, 12 with the default band of 8.
        let heights = [-2.0, 0.0, 4.0, 8.0, 12.0];
        let w = ContactWidths::from_band(8.0);
        let height = heights.map(|d| contact_weight(ContactShape::Height, w, d, parallel));
        assert_eq!(height, [1.0, 1.0, 0.5, 0.0, 0.0]);
        let penetration = heights.map(|d| contact_weight(ContactShape::Penetration, w, d, upright));
        assert_eq!(penetration, [1.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(contact_weight(ContactShape::Penetration, w, -1.0, upright), 0.5, "continuous between -2 and 0");
        // The flat and steep widths do not reach the old shapes.
        let odd = ContactWidths { flat: 0.01, band: 8.0, steep: 500.0 };
        for shape in [ContactShape::Height, ContactShape::Penetration, ContactShape::HybridV1] {
            for d in heights { for c in [parallel, sloped, upright] {
                assert_eq!(contact_weight(shape, odd, d, c), contact_weight(shape, w, d, c));
            }}
        }
        let v1 = ContactShape::HybridV1;
        // Hybrid v1: a parallel top above ground gets nothing, coplanar included.
        assert_eq!(heights.map(|d| contact_weight(v1, w, d, parallel)), [1.0, 0.0, 0.0, 0.0, 0.0]);
        // An upright face gets the full band above ground, and full weight below.
        assert_eq!(heights.map(|d| contact_weight(v1, w, d, upright)), [1.0, 1.0, 0.5, 0.0, 0.0]);
        // 25 degrees lies between the 15 and 35 degree limits: part weight.
        let part = contact_weight(v1, w, 0.0, sloped);
        assert!(part > 0.0 && part < 1.0, "{part}");
        assert!((contact_weight(v1, w, 0.0, cos(35.0)) - 1.0).abs() < 1e-6);
        assert!(contact_weight(v1, w, 0.0, cos(15.0)) < 1e-6);
        // A road top 10-15 units above the ground gets nothing at the default band.
        for d in [10.0, 15.0] { assert_eq!(contact_weight(v1, w, d, upright), 0.0); }
        // Out-of-range cosines are clamped; a buried parallel surface still saturates.
        assert_eq!(contact_weight(v1, w, -3.0, 2.0), 1.0);
        assert_eq!((ContactShape::HybridV1.number(), ContactShape::HybridV1.name()), (3.0, "hybrid-v1"));
        assert_eq!([ContactShape::Height, ContactShape::Penetration, ContactShape::Hybrid].map(|s| s.number()), [0.0, 1.0, 2.0], "numbers stay stable");
        assert_eq!([ContactShape::Hybrid, ContactShape::HybridV1, ContactShape::Height].map(|s| s.weight_version()), [2, 1, 1]);
    }

    #[test]
    fn hybrid_v2_band_has_flat_shallow_and_steep_plateaus() {
        let cos = |deg: f32| deg.to_radians().cos();
        let w = ContactWidths::from_band(8.0);
        assert_eq!(w, ContactWidths { flat: 0.5, band: 8.0, steep: 48.0 }, "the default widths");
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        // Parallel (either facing) gets the flat width; 5-35 degrees the band; 75-90 the steep.
        assert!(close(v2_band(w, 1.0), 0.5) && close(v2_band(w, -1.0), 0.5));
        for deg in [5.0, 10.0, 20.0, 35.0] { assert!(close(v2_band(w, cos(deg)), 8.0), "{deg}: {}", v2_band(w, cos(deg))); }
        for deg in [75.0, 80.0, 90.0, 105.0] { assert!(close(v2_band(w, cos(deg)), 48.0), "{deg}"); }
        // Ramps in between, monotonic in the angle.
        let mut last = 0.0;
        for tenth in 0..=900 {
            let b = v2_band(w, cos(tenth as f32 / 10.0));
            assert!(b >= last - 1e-4, "{tenth}: {b} < {last}");
            assert!((0.5 - 1e-4..=48.0 + 1e-4).contains(&b));
            last = b;
        }
        let mid = v2_band(w, cos(55.0));
        assert!(mid > 8.0 && mid < 48.0, "{mid}");
        // Explicit widths change only the flat and steep plateaus.
        let wide = ContactWidths { flat: 0.5, band: 8.0, steep: 64.0 };
        assert!(close(v2_band(wide, cos(20.0)), 8.0) && close(v2_band(wide, 0.0), 64.0));
    }

    #[test]
    fn hybrid_v2_weight_is_continuous_saturates_below_and_leaves_tops_alone() {
        let cos = |deg: f32| deg.to_radians().cos();
        let w = ContactWidths::from_band(8.0);
        let v2 = |d: f32, c: f32| contact_weight(ContactShape::Hybrid, w, d, c);
        // Below the ground every orientation saturates (H = 1); penetration is redundant there.
        for deg in [0.0, 3.0, 20.0, 60.0, 90.0] { for d in [-5.0, -2.0, -0.5, 0.0] {
            assert_eq!(v2(d, cos(deg)), 1.0, "{deg} {d}");
        }}
        // A parallel asphalt top 10-15 units above the ground (and anything over 0.5) gets none;
        // so does a top tilted by a few degrees.
        for d in [0.5, 1.0, 10.0, 12.5, 15.0] { assert_eq!(v2(d, 1.0), 0.0, "{d}"); }
        for d in [10.0, 12.5, 15.0] { for deg in [1.0, 3.0] {
            assert_eq!(v2(d, cos(deg)), 0.0, "{deg} {d}");
        }}
        // Shoulders: round 5's successful 8-unit band; rock faces: 48 units.
        assert_eq!(v2(4.0, cos(20.0)), contact_weight(ContactShape::Height, w, 4.0, 1.0));
        assert!((v2(24.0, cos(90.0)) - 0.5).abs() < 1e-6 && v2(48.0, cos(90.0)) == 0.0);
        // Continuous in height and angle: small steps change the weight by a little.
        for deg10 in 0..900 { for d10 in -30..600 {
            let (deg, d) = (deg10 as f32 / 10.0, d10 as f32 / 10.0);
            let here = v2(d, cos(deg));
            assert!((0.0..=1.0).contains(&here));
            assert!((v2(d + 0.1, cos(deg)) - here).abs() < 0.35, "height jump at {deg} {d}");
            assert!((v2(d, cos(deg + 0.1)) - here).abs() < 0.35, "angle jump at {deg} {d}");
        }}
        // The degenerate-normal fallback (cosine 1) is the flat width.
        assert_eq!(v2(1.0, 1.0), 0.0);
    }

    #[test]
    fn steepness_fade_keeps_flat_faces_and_drops_vertical_ones() {
        let cos = |deg: f32| deg.to_radians().cos();
        // Faces up to 60 degrees from horizontal keep the weight whole, from 80 none.
        for deg in [0.0, 10.0, 30.0, 45.0, 59.9] { assert_eq!(steepness_fade(cos(deg)), 1.0, "{deg}"); }
        for deg in [80.1, 85.0, 89.0, 90.0] { assert_eq!(steepness_fade(cos(deg)), 0.0, "{deg}"); }
        assert_eq!(steepness_fade(micro(STEEP_COS_FULL_E6)), 1.0);
        assert_eq!(steepness_fade(micro(STEEP_COS_NONE_E6)), 0.0);
        let mid = steepness_fade(cos(70.0));
        assert!(mid > 0.0 && mid < 1.0, "{mid}");
        // Monotonic: never rising as the face turns from flat to vertical; out-of-range
        // cosines clamp.
        let mut last = 1.0;
        for tenth in 0..=900 {
            let f = steepness_fade(cos(tenth as f32 / 10.0));
            assert!(f <= last + 1e-6 && (0.0..=1.0).contains(&f), "{tenth}: {f} > {last}");
            last = f;
        }
        assert_eq!((steepness_fade(1.5), steepness_fade(-0.5)), (1.0, 0.0));
        // Every contact shape's weight times the fade: a flat face keeps its weight, a
        // vertical face gets none, at every height in the band.
        let w = ContactWidths::from_band(8.0);
        for shape in [ContactShape::Height, ContactShape::Penetration, ContactShape::Hybrid, ContactShape::HybridV1] {
            for d in [-2.0, 0.0, 4.0] {
                let flat = contact_weight(shape, w, d, 1.0);
                assert_eq!(flat * steepness_fade(1.0), flat, "{shape:?} {d}");
                assert_eq!(contact_weight(shape, w, d, 0.0) * steepness_fade(0.0), 0.0, "{shape:?} {d}");
            }
        }
    }

    #[test]
    fn face_up_reads_the_geometric_face_not_the_smoothed_normal() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
        // A level face (Bevy x and z across the screen): straight up, either winding.
        assert!(close(face_up(Vec3::X, Vec3::Z, Vec3::Y), 1.0) && close(face_up(Vec3::Z, Vec3::X, Vec3::NEG_Y), 1.0));
        // A vertical face whose smoothed vertex normal leans up (a road's lip): steep anyway.
        let lip_normal = Vec3::new(0.0, 0.8, 0.6);
        assert!(close(face_up(Vec3::X, Vec3::Y, lip_normal), 0.0));
        assert_eq!(steepness_fade(face_up(Vec3::X, Vec3::Y, lip_normal)), 0.0);
        // 45 degrees, footprints of any size: the derivatives' lengths cancel.
        for scale in [1e-3, 1.0, 100.0] {
            assert!(close(face_up(Vec3::X * scale, Vec3::new(0.0, 1.0, 1.0) * scale, Vec3::Y), 45f32.to_radians().cos()), "{scale}");
        }
        // Parallel or zero derivatives (a face seen edge-on): the vertex normal stands in,
        // and without one, no fade.
        assert!(close(face_up(Vec3::X, Vec3::X * 2.0, lip_normal), 0.8));
        assert!(close(face_up(Vec3::ZERO, Vec3::ZERO, Vec3::new(0.0, -3.0, 4.0)), 0.6));
        assert_eq!(face_up(Vec3::ZERO, Vec3::Z, Vec3::ZERO), 1.0);
    }

    #[test]
    fn contact_widths_validate_order_and_finiteness() {
        assert!(ContactWidths::from_band(8.0).valid());
        assert!(ContactWidths { flat: 8.0, band: 8.0, steep: 8.0 }.valid(), "equal widths are ordered");
        for bad in [
            ContactWidths { flat: 0.0, band: 8.0, steep: 48.0 }, ContactWidths { flat: -1.0, band: 8.0, steep: 48.0 },
            ContactWidths { flat: 9.0, band: 8.0, steep: 48.0 }, ContactWidths { flat: 0.5, band: 8.0, steep: 4.0 },
            ContactWidths { flat: f32::NAN, band: 8.0, steep: 48.0 }, ContactWidths { flat: 0.5, band: 8.0, steep: f32::INFINITY },
            ContactWidths::from_band(f32::MAX),
        ] { assert!(!bad.valid(), "{bad:?}"); }
    }

    #[test]
    fn terrain_normal_matches_plane_slopes_and_converts_to_world_space() {
        let spacing = fnv_world::land::SPACING;
        let stored = HeightLayout::STORED;
        assert_eq!(terrain_game_normal(stored, 0.0, 0.0), Vec3::Z);
        // A square rising 128 units eastwards (a 1:1 slope in x only).
        let n = terrain_game_normal(stored, spacing, 0.0);
        assert!((n - Vec3::new(-1.0, 0.0, 1.0).normalize()).length() < 1e-6);
        // Perpendicular to both triangles' edges for arbitrary heights (height_planes'
        // coefficients: [a, east-a, ne-east] and [a, ne-north, north-a]).
        let (a, east, north, ne) = (10.0, 40.0, -5.0, 22.0);
        for (dx, dy, corners) in [
            (east - a, ne - east, [Vec3::new(0.0, 0.0, a), Vec3::new(spacing, 0.0, east), Vec3::new(spacing, spacing, ne)]),
            (ne - north, north - a, [Vec3::new(0.0, 0.0, a), Vec3::new(spacing, spacing, ne), Vec3::new(0.0, spacing, north)]),
        ] {
            let n = terrain_game_normal(stored, dx, dy);
            assert!(n.dot(corners[1] - corners[0]).abs() < 1e-3 && n.dot(corners[2] - corners[0]).abs() < 1e-3);
            assert!(n.z > 0.0);
            // #2149: the same rise over a surface square a quarter as wide is four times as steep.
            let fine = terrain_game_normal(HeightLayout::new(4), dx, dy);
            let corners = corners.map(|c| Vec3::new(c.x / 4.0, c.y / 4.0, c.z));
            assert!(fine.dot(corners[1] - corners[0]).abs() < 1e-3 && fine.dot(corners[2] - corners[0]).abs() < 1e-3);
        }
        // Game (x east, y north, z up) -> Bevy (x, z, -y): up is +Y, north is -Z.
        assert_eq!(game_to_world_normal(Vec3::Z), Vec3::Y);
        assert_eq!(game_to_world_normal(Vec3::Y), Vec3::NEG_Z);
        assert_eq!(game_to_world_normal(Vec3::X), Vec3::X);
    }

    #[test]
    fn blended_normal_is_exact_at_zero_and_safe_when_opposed() {
        let mesh = Vec3::new(0.3, 0.4, 0.866).normalize();
        assert_eq!(blend_normal(mesh, Vec3::Y, 0.0), mesh, "w = 0 keeps the input exactly");
        assert_eq!(blend_normal(mesh, Vec3::Y, -0.5), mesh);
        assert!((blend_normal(mesh, Vec3::Y, 1.0) - Vec3::Y).length() < 1e-6);
        let half = blend_normal(Vec3::X, Vec3::Y, 0.5);
        assert!((half.length() - 1.0).abs() < 1e-6 && (half - Vec3::new(1.0, 1.0, 0.0).normalize()).length() < 1e-6);
        // Opposed normals cancel at w = 0.5: the ground normal stands in.
        assert_eq!(blend_normal(Vec3::NEG_Y, Vec3::Y, 0.5), Vec3::Y);
        assert_eq!(blend_normal(Vec3::ZERO, Vec3::Y, 0.5).length(), 1.0);
    }

    #[test]
    fn shared_constants_reach_the_shader_definitions() {
        let defs = blend_shader_defs(HeightLayout::STORED);
        let uint = |name: &str| defs.iter().find_map(|d| match d { ShaderDefVal::UInt(n, v) if n == name => Some(*v), _ => None }).unwrap();
        // #2149/#2661: the layout's dimensions; the stored layout's are those before it.
        let bool_def = |defs: &[ShaderDefVal], name: &str| defs.iter().find_map(|d| match d { ShaderDefVal::Bool(n, v) if n == name => Some(*v), _ => None }).unwrap();
        assert_eq!(["MESH_TERRAIN_GRID", "MESH_TERRAIN_SUB", "MESH_TERRAIN_HEIGHT_TILE_WIDTH", "MESH_TERRAIN_HEIGHT_TILE_HEIGHT", "MESH_TERRAIN_HEIGHT_WIDTH", "MESH_TERRAIN_DETAIL_ROW"].map(uint), [32, 1, 192, 33, 576, 99]);
        assert!(!bool_def(&defs, "MESH_TERRAIN_SUBDIVIDED"));
        let fine = blend_shader_defs(HeightLayout::new(4));
        let fine_uint = |name: &str| fine.iter().find_map(|d| match d { ShaderDefVal::UInt(n, v) if n == name => Some(*v), _ => None }).unwrap();
        assert_eq!(["MESH_TERRAIN_GRID", "MESH_TERRAIN_SUB", "MESH_TERRAIN_HEIGHT_TILE_WIDTH", "MESH_TERRAIN_HEIGHT_TILE_HEIGHT", "MESH_TERRAIN_HEIGHT_WIDTH", "MESH_TERRAIN_DETAIL_ROW"].map(fine_uint), [32, 4, 768, 129, 2304, 387]);
        assert!(bool_def(&fine, "MESH_TERRAIN_SUBDIVIDED"));
        // Everything else is the same for every layout.
        let rest = |defs: &[ShaderDefVal]| defs.iter().filter(|d| !matches!(d, ShaderDefVal::UInt(n, _) | ShaderDefVal::Bool(n, _) if n.starts_with("MESH_TERRAIN_HEIGHT") || n == "MESH_TERRAIN_SUB" || n == "MESH_TERRAIN_SUBDIVIDED" || n == "MESH_TERRAIN_DETAIL_ROW")).cloned().collect::<Vec<_>>();
        assert_eq!(rest(&defs), rest(&fine));
        assert_eq!(uint("MESH_TERRAIN_ROUGHNESS_E6"), 950_000);
        assert_eq!(uint("MESH_TERRAIN_REFLECTANCE_E6"), 200_000);
        assert_eq!(uint("MESH_TERRAIN_COS_FULL_E6"), CONTACT_COS_FULL_E6);
        assert!((micro(CONTACT_COS_FULL_E6) - 35f32.to_radians().cos()).abs() < 1e-6);
        assert!((micro(CONTACT_COS_NONE_E6) - 15f32.to_radians().cos()).abs() < 1e-6);
        assert_eq!(uint("MESH_TERRAIN_V2_COS_PARALLEL_E6"), V2_COS_PARALLEL_E6);
        assert_eq!(uint("MESH_TERRAIN_V2_COS_STEEP_E6"), V2_COS_STEEP_E6);
        assert!((micro(V2_COS_PARALLEL_E6) - 5f32.to_radians().cos()).abs() < 1e-6);
        assert!((micro(V2_COS_STEEP_E6) - 75f32.to_radians().cos()).abs() < 1e-6);
        assert_eq!(uint("MESH_TERRAIN_STEEP_COS_FULL_E6"), STEEP_COS_FULL_E6);
        assert_eq!(uint("MESH_TERRAIN_STEEP_COS_NONE_E6"), STEEP_COS_NONE_E6);
        assert!((micro(STEEP_COS_FULL_E6) - 60f32.to_radians().cos()).abs() < 1e-6);
        assert!((micro(STEEP_COS_NONE_E6) - 80f32.to_radians().cos()).abs() < 1e-6);
        assert!(!defs.iter().any(|d| matches!(d, ShaderDefVal::UInt(n, _) if n.contains("RESOLUTION"))), "the shader reads the atlas size from the texture");
    }
}
