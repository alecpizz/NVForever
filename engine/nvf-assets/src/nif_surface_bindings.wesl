// The NifSurface extension's values and textures for one fragment, bindless or not
// (nif_material.rs "Bindless"; the Rust side: `NifSurface`, `NifSurfaceUniform`). Used by
// nif_material.wesl and nif_prepass.wesl.


import bevy_pbr::render::mesh_bindings::mesh;
@if(BINDLESS) import bevy_render::bindless::{bindless_textures_2d, bindless_textures_cube, bindless_samplers_filtering};
@if(BINDLESS) import bevy_pbr::render::pbr_bindings::{material_indices, material_array};
@if(BINDLESS) import bevy_pbr::render::pbr_types::StandardMaterial;
@if(!BINDLESS) import bevy_pbr::render::pbr_bindings;
@if(!BINDLESS) import bevy_pbr::render::pbr_types::StandardMaterial;
// [design] Round 10a's landscape detail uses Bevy's own TBN and normal-map helpers, as the
// near landscape's draws do (terrain.wesl, StandardMaterial).
@if(MESH_TERRAIN_BLEND && BINDLESS) import bevy_pbr::render::pbr_functions::{calculate_tbn_mikktspace, apply_normal_mapping};
// [design] With the near landscape's height blending (#2550) or anti-tiling (#2551) on, the
// detail composes with the very functions terrain.wesl uses: nvforever's terrain_weights.wesl
// (loaded by nvforever terrain.rs, the crate whose blend pipelines set MESH_TERRAIN_BLEND)
// and this crate's terrain_sampling.wesl (graphics-enhanced.md §3.14 "Height blending and
// anti-tiling").
@if(MESH_TERRAIN_BLEND && BINDLESS) import nvforever::terrain_weights::{terrain_height, terrain_ordered_shares, terrain_height_weights};
@if(MESH_TERRAIN_BLEND && BINDLESS) import nvf_assets::terrain_sampling::{TerrainFootprint, TerrainTaps, terrain_hex3, terrain_normal_tap, terrain_unit_normal, terrain_world_normal};

struct NifSurface {
    // x start angle (cosine), y stop angle (cosine), z start opacity, w stop opacity;
    // (1, 0, 1, 1) when the shape has no falloff.
    falloff: vec4<f32>,
    // Fragments with a lower alpha are not drawn (the alpha test of a shape that is
    // drawn blended); 0 on every other shape.
    alpha_cut: f32,
    // 1: an additive shape with source factor ONE, which adds its colour whole (only the
    // view-angle falloff fades it); 0 otherwise.
    add_whole: f32,
    // REFRACT: the screen shift (UV) per unit of the view-space normal (the strength ×
    // nif_material.rs REFRACTION_SCREEN); 0 otherwise.
    refraction: f32,
    // REFRACT: seconds for the normal map to scroll once up its texture (fire
    // refraction); 0: it does not move.
    fire_period: f32,
    // x 1 skips the image-space emissive scale; y holds each channel on the soft path
    // (0: no hold); z, w unused [hypothesis, nif.md §6 "Unlit shapes that are pictures,
    // and cut-out signs"]. Same fields in the same order as NifSurfaceUniform.
    emissive_rule: vec4<f32>,
    // Holes: x the value (alpha units, negative: no holes), y the edge band's width.
    holes: vec4<f32>,
    // The edge band's colour (linear).
    holes_edge: vec4<f32>,
    // Soft intersections: x the fade distance (game units), 0 for none.
    soft: vec4<f32>,
    // PIPBOY_SCREEN (nif_material.rs NifSurface::pipboy_a): x the roll up (share of the
    // canvas's height), y the wash (gain 1 - y on display values), z display value added,
    // w the canvas's share of the texture's height.
    pipboy_a: vec4<f32>,
    // PIPBOY_SCREEN (NifSurface::pipboy_b): x the softening's reach (view pixels), y the
    // noise's reach (display values), z the canvas's share of the texture's width; w unused.
    pipboy_b: vec4<f32>,
    // Landscape anti-tiling (nif_material.rs NifSurface::terrain_repeat): x world units per
    // texture repeat of a landscape material, 0 on every other shape; y an authored set's
    // metal gain (NifSurface::metal_gain, #2981; 0 on every other material), read by
    // nif_material.wesl's lift only; z, w unused.
    terrain: vec4<f32>,
    // Environment map (nif_material.rs NifSurface::env): x the scale (0: none), y the mask
    // (0 none, 1 the mask's red, 2 the mask's alpha: the game's normal map bound as the
    // mask, a debug switch's variant), z and w the fade distances
    // (both 0: no fade). After `terrain`, as in NifSurfaceUniform; the blend type has none
    // (nif_surface_blend.rs: its uniform goes on with its own rows here).
    @if(!NIF_SURFACE_BLEND) env: vec4<f32>,
    // Environment map (NifSurface::env_how): x the cube orientation (0 #2669's lookup),
    // y 1 scales by received light, 2 by the scene light, z 1 for a window shape, w 0 without an
    // environment map, 1 read at the shading normal, 2 at the interpolated one. The blend type has none either.
    @if(!NIF_SURFACE_BLEND) env_how: vec4<f32>,
    // [design] The enabled blend type (nif_surface_blend.rs): band, view, contact shape,
    // shading; the cache's southwest cell in origin.xy; hybrid v2's flat and steep band
    // widths in widths.xy (z, w unused).
    @if(NIF_SURFACE_BLEND) mesh_terrain: vec4<f32>,
    @if(NIF_SURFACE_BLEND) mesh_terrain_origin: vec4<f32>,
    @if(NIF_SURFACE_BLEND) mesh_terrain_widths: vec4<f32>,
}

// The extension's index table (indices 50-56 of NifSurface's bindless attribute; 54-56
// are the window environment map's cube, sampler and mask, the ordinary type's only). The
// enabled blend type (nif_surface_blend.rs module notes) has the same table, with its
// landscape colour atlas at 51, that atlas's sampler at 52 and its height atlas at 53,
// and after them round 10a's landscape palette (indices 54-453: 384 textures, then 16
// samplers; nif_surface_blend.rs PALETTE_ENTRIES, which a test keeps equal to 400). The
// member is in the blend type's prepass too, so both read the same table row size.
@if(BINDLESS) struct NifSurfaceIndices {
    surface: u32,          // 50
    holes_texture: u32,    // 51
    holes_sampler: u32,    // 52
    facegen_offset: u32,   // 53
    @if(!NIF_SURFACE_BLEND) env_cube: u32,     // 54
    @if(!NIF_SURFACE_BLEND) env_sampler: u32,  // 55
    @if(!NIF_SURFACE_BLEND) env_mask: u32,     // 56
    @if(NIF_SURFACE_BLEND) palette: array<u32, 400>, // 54-453
}
@if(BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(100) var<storage> nif_indices: array<NifSurfaceIndices>;
@if(BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(101) var<storage> nif_surfaces: array<NifSurface>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(50) var<uniform> nif_surface_values: NifSurface;
@if(!BINDLESS && !NIF_SURFACE_BLEND) @group(constants::MATERIAL_BIND_GROUP) @binding(51) var holes_texture: texture_2d<f32>;
@if(!BINDLESS && !NIF_SURFACE_BLEND) @group(constants::MATERIAL_BIND_GROUP) @binding(52) var holes_sampler: sampler;
@if(!BINDLESS && !NIF_SURFACE_BLEND) @group(constants::MATERIAL_BIND_GROUP) @binding(53) var facegen_offset: texture_2d<f32>;
@if(!BINDLESS && !NIF_SURFACE_BLEND) @group(constants::MATERIAL_BIND_GROUP) @binding(54) var env_cube: texture_cube<f32>;
@if(!BINDLESS && !NIF_SURFACE_BLEND) @group(constants::MATERIAL_BIND_GROUP) @binding(55) var env_sampler: sampler;
@if(!BINDLESS && !NIF_SURFACE_BLEND) @group(constants::MATERIAL_BIND_GROUP) @binding(56) var env_mask: texture_2d<f32>;
@if(!BINDLESS && NIF_SURFACE_BLEND) @group(constants::MATERIAL_BIND_GROUP) @binding(51) var mesh_terrain_colour: texture_2d<f32>;
@if(!BINDLESS && NIF_SURFACE_BLEND) @group(constants::MATERIAL_BIND_GROUP) @binding(52) var mesh_terrain_sampler: sampler;
@if(!BINDLESS && NIF_SURFACE_BLEND) @group(constants::MATERIAL_BIND_GROUP) @binding(53) var mesh_terrain_height: texture_2d<f32>;

// [design] Values come from nif_surface_blend.rs::blend_shader_defs, shared with packing
// and with that file's CPU statements of the weight (`contact_weight`) and normal.
@if(MESH_TERRAIN_BLEND) const GRID: i32 = i32(constants::MESH_TERRAIN_GRID);
@if(MESH_TERRAIN_BLEND) const HEIGHT_TILE: vec2<i32> = vec2(i32(constants::MESH_TERRAIN_HEIGHT_TILE_WIDTH), i32(constants::MESH_TERRAIN_HEIGHT_TILE_HEIGHT));
@if(MESH_TERRAIN_BLEND) const CELL_SIZE: f32 = f32(constants::MESH_TERRAIN_CELL_SIZE);
@if(MESH_TERRAIN_BLEND) const SPACING: f32 = f32(constants::MESH_TERRAIN_SPACING);
// [design] #2149/#2661: the height atlas holds the drawn surface, SUB squares per LAND grid
// square and side (nif_surface_blend.rs HeightLayout): HEIGHT_GRID squares per cell side,
// HEIGHT_SPACING game units apart, the metadata row after them. GRID stays the LAND grid
// (quadrants, landscape detail).
@if(MESH_TERRAIN_BLEND) const SUB: i32 = i32(constants::MESH_TERRAIN_SUB);
@if(MESH_TERRAIN_BLEND) const HEIGHT_GRID: i32 = GRID * SUB;
@if(MESH_TERRAIN_BLEND) const HEIGHT_SPACING: f32 = SPACING / f32(SUB);
@if(MESH_TERRAIN_BLEND) const COS_FULL: f32 = f32(constants::MESH_TERRAIN_COS_FULL_E6) / 1e6;
@if(MESH_TERRAIN_BLEND) const COS_NONE: f32 = f32(constants::MESH_TERRAIN_COS_NONE_E6) / 1e6;
@if(MESH_TERRAIN_BLEND) const PENETRATION: f32 = f32(constants::MESH_TERRAIN_PENETRATION_E6) / 1e6;
@if(MESH_TERRAIN_BLEND) const V2_COS_PARALLEL: f32 = f32(constants::MESH_TERRAIN_V2_COS_PARALLEL_E6) / 1e6;
@if(MESH_TERRAIN_BLEND) const V2_COS_STEEP: f32 = f32(constants::MESH_TERRAIN_V2_COS_STEEP_E6) / 1e6;
// [design] The steepness fade's limits (nif_surface_blend.rs STEEP_COS_FULL_E6,
// STEEP_COS_NONE_E6): the cosine of the face's angle from horizontal at which the contact
// weight is still whole (60 degrees) and from which it is 0 (80 degrees).
@if(MESH_TERRAIN_BLEND) const STEEP_COS_FULL: f32 = f32(constants::MESH_TERRAIN_STEEP_COS_FULL_E6) / 1e6;
@if(MESH_TERRAIN_BLEND) const STEEP_COS_NONE: f32 = f32(constants::MESH_TERRAIN_STEEP_COS_NONE_E6) / 1e6;
@if(MESH_TERRAIN_BLEND) const LAND_ROUGHNESS: f32 = f32(constants::MESH_TERRAIN_ROUGHNESS_E6) / 1e6;
@if(MESH_TERRAIN_BLEND) const LAND_REFLECTANCE: f32 = f32(constants::MESH_TERRAIN_REFLECTANCE_E6) / 1e6;
// [design] Round 10a's landscape detail layout and distances (nif_surface_blend.rs).
@if(MESH_TERRAIN_BLEND) const HEIGHT_WIDTH: u32 = u32(constants::MESH_TERRAIN_HEIGHT_WIDTH);
@if(MESH_TERRAIN_BLEND) const DETAIL_ROW: u32 = u32(constants::MESH_TERRAIN_DETAIL_ROW);
@if(MESH_TERRAIN_BLEND) const DETAIL_QUADRANT_WORDS: u32 = u32(constants::MESH_TERRAIN_DETAIL_QUADRANT_WORDS);
@if(MESH_TERRAIN_BLEND) const DETAIL_VERTEX_WORDS: u32 = u32(constants::MESH_TERRAIN_DETAIL_VERTEX_WORDS);
@if(MESH_TERRAIN_BLEND) const DETAIL_QUADRANTS: u32 = u32(constants::MESH_TERRAIN_DETAIL_QUADRANTS);
@if(MESH_TERRAIN_BLEND) const QUAD_VERTICES: u32 = u32(constants::MESH_TERRAIN_QUAD_VERTICES);
@if(MESH_TERRAIN_BLEND) const DETAIL_MAX_OVERLAYS: u32 = u32(constants::MESH_TERRAIN_DETAIL_MAX_OVERLAYS);
@if(MESH_TERRAIN_BLEND) const PALETTE_IMAGES: u32 = u32(constants::MESH_TERRAIN_PALETTE_IMAGES);
@if(MESH_TERRAIN_BLEND) const DETAIL_NEAR: f32 = f32(constants::MESH_TERRAIN_DETAIL_NEAR);
@if(MESH_TERRAIN_BLEND) const DETAIL_FAR: f32 = f32(constants::MESH_TERRAIN_DETAIL_FAR);
@if(MESH_TERRAIN_BLEND) const FLAG_TWO_COMPONENT: u32 = u32(constants::MESH_TERRAIN_FLAG_TWO_COMPONENT);
@if(MESH_TERRAIN_BLEND) const FLAG_FLIP_Y: u32 = u32(constants::MESH_TERRAIN_FLAG_FLIP_Y);

// [design] Lookup classifications, in diagnostic precedence order.
@if(MESH_TERRAIN_BLEND) const LOOKUP_OUT_OF_RANGE: u32 = 0u;
@if(MESH_TERRAIN_BLEND) const LOOKUP_INVALID_HEIGHT: u32 = 1u;
@if(MESH_TERRAIN_BLEND) const LOOKUP_INVALID_COLOUR: u32 = 2u;
@if(MESH_TERRAIN_BLEND) const LOOKUP_REJECTED_FOOTPRINT: u32 = 3u;
@if(MESH_TERRAIN_BLEND) const LOOKUP_SUCCESS: u32 = 4u;
// [design] #2661 review: in the surface view only, the height test alone gave a contact
// weight of exactly 0 and the lookup stopped before the colour and detail checks
// (`mesh_terrain_lookup`). The diagnostic views never get it, so it has no colour there.
@if(MESH_TERRAIN_BLEND) const LOOKUP_NO_CONTACT: u32 = 5u;

// [design] One fragment's landscape lookup: its classification, contact weight (zero
// unless it succeeded), the cached colour (sampled only with a positive weight), and the
// ground triangle's geometric normal in game space (x east, y north, z up) and Bevy world
// space, valid from LOOKUP_INVALID_COLOUR on.
@if(MESH_TERRAIN_BLEND) struct MeshTerrainLookup {
    status: u32,
    weight: f32,
    colour: vec3<f32>,
    game_normal: vec3<f32>,
    normal: vec3<f32>,
    // [design] Round 10a: the landscape layers' share of the target (the distance mix d
    // where both sources serve the fragment, 1 where only the layers do, the weight then
    // faded by d; 0 for the colour cache alone), and their colour and mapped normal (Bevy
    // world space), evaluated only with a positive weight and share.
    detail: f32,
    detail_colour: vec3<f32>,
    detail_normal: vec3<f32>,
}

// [design] Round 10a: what the lookup needs of the fragment besides its position: the
// camera's distance (game units), the derivatives of its game xy across the view's x and
// y (taken by the caller in uniform control flow, before any lookup branch), the view's
// mip bias, and whether full shading reads the mapped normal (0 albedo, 1 full). #2661
// review: `up`, the cosine between the mesh face's geometric normal and game up
// (`mesh_terrain_face_up`, from the same derivatives), for the steepness fade.
@if(MESH_TERRAIN_BLEND) struct MeshTerrainInputs {
    distance: f32,
    ddx: vec2<f32>,
    ddy: vec2<f32>,
    bias: f32,
    full: u32,
    up: f32,
}

// [design] Encoded coefficients preserve their f32 bits without requiring filterable R32F.
@if(MESH_TERRAIN_BLEND) fn terrain_height_bits(instance_index: u32, p: vec2<i32>) -> u32 {
    @if(BINDLESS) let v = textureLoad(bindless_textures_2d[nif_indices[material_slot(instance_index)].facegen_offset], p, 0);
    @if(!BINDLESS) let v = textureLoad(mesh_terrain_height, p, 0);
    let b = vec4<u32>(round(v * 255.0));
    return b.x | (b.y << 8u) | (b.z << 16u) | (b.w << 24u);
}
@if(MESH_TERRAIN_BLEND) fn terrain_height_texel(instance_index: u32, p: vec2<i32>) -> f32 {
    return bitcast<f32>(terrain_height_bits(instance_index, p));
}

// [design] Round 10b: one texel of a level of the colour atlas (the read's bilinear taps,
// weighted by the caller: no implicit derivatives inside the lookup's branches).
@if(MESH_TERRAIN_BLEND) fn terrain_colour_load(instance_index: u32, p: vec2<i32>, level: i32) -> vec3<f32> {
    @if(BINDLESS) return textureLoad(bindless_textures_2d[nif_indices[material_slot(instance_index)].holes_texture], p, level).rgb;
    @if(!BINDLESS) return textureLoad(mesh_terrain_colour, p, level).rgb;
}

// [design] Round 10a: this publication supplies the quadrant's layer detail (texel 2 of the
// cell's metadata row, terrain_surface.rs `encode_detail_mask`), intersected with the live
// coverage as the colour is. Without bindless there is no palette: never.
@if(MESH_TERRAIN_BLEND) fn terrain_detail_valid(instance_index: u32, slot: vec2<i32>, quadrant: u32) -> bool {
    @if(!BINDLESS) return false;
    @if(BINDLESS) {
        if !terrain_covered(instance_index, slot) { return false; }
        let mask = u32(terrain_height_texel(instance_index, slot * HEIGHT_TILE + vec2(2, HEIGHT_GRID)));
        return (mask & (1u << quadrant)) != 0u;
    }
}

// [design] Round 10a: word `w` of the detail rows after the heights (nif_surface_blend.rs
// module notes), and a 16-bit value of it.
@if(MESH_TERRAIN_BLEND && BINDLESS) fn terrain_detail_word(instance_index: u32, w: u32) -> u32 {
    return terrain_height_bits(instance_index, vec2<i32>(i32(w % HEIGHT_WIDTH), i32(DETAIL_ROW + w / HEIGHT_WIDTH)));
}
@if(MESH_TERRAIN_BLEND && BINDLESS) fn detail_unorm(h: u32) -> f32 { return f32(h & 0xffffu) / 65535.0; }
@if(MESH_TERRAIN_BLEND && BINDLESS) fn detail_snorm(h: u32) -> f32 { return max(f32(bitcast<i32>(h << 16u) >> 16u) / 32767.0, -1.0); }

// [design] The fragment's ground triangle's three vertex records and barycentric weights.
@if(MESH_TERRAIN_BLEND && BINDLESS) struct TerrainTriangle {
    records: vec3<u32>,
    weights: vec3<f32>,
}

// [design] The interpolated linear vertex colour, normal and tangent (game space; the
// normal and tangent not renormalized, as the mesh's interpolated ones are not).
@if(MESH_TERRAIN_BLEND && BINDLESS) struct TerrainVertexMix {
    tint: vec3<f32>,
    normal: vec3<f32>,
    tangent: vec3<f32>,
}

@if(MESH_TERRAIN_BLEND && BINDLESS) fn detail_attributes(instance_index: u32, t: TerrainTriangle) -> TerrainVertexMix {
    var m = TerrainVertexMix(vec3(0.0), vec3(0.0), vec3(0.0));
    for (var i = 0u; i < 3u; i++) {
        let r = t.records[i];
        let w0 = terrain_detail_word(instance_index, r);
        let w1 = terrain_detail_word(instance_index, r + 1u);
        let w2 = terrain_detail_word(instance_index, r + 2u);
        let w3 = terrain_detail_word(instance_index, r + 3u);
        let w4 = terrain_detail_word(instance_index, r + 4u);
        m.tint += t.weights[i] * vec3(detail_unorm(w0), detail_unorm(w0 >> 16u), detail_unorm(w1));
        m.normal += t.weights[i] * vec3(detail_snorm(w1 >> 16u), detail_snorm(w2), detail_snorm(w2 >> 16u));
        m.tangent += t.weights[i] * vec3(detail_snorm(w3), detail_snorm(w3 >> 16u), detail_snorm(w4));
    }
    return m;
}

// Overlay `k`'s opacity, interpolated (value 9 + k of each record).
@if(MESH_TERRAIN_BLEND && BINDLESS) fn detail_opacity(instance_index: u32, t: TerrainTriangle, k: u32) -> f32 {
    let h = 9u + k;
    var o = 0.0;
    for (var i = 0u; i < 3u; i++) {
        o += t.weights[i] * detail_unorm(terrain_detail_word(instance_index, t.records[i] + h / 2u) >> (16u * (h % 2u)));
    }
    return o;
}

// The StandardMaterial flags a normal map is decoded with: two bits per entry of word 4
// (entry 0 the base, entry k + 1 overlay k), two-component and green flipped.
@if(MESH_TERRAIN_BLEND && BINDLESS) fn detail_decode(decode: u32, entry: u32) -> u32 {
    let bits = decode >> (2u * entry);
    return select(0u, FLAG_TWO_COMPONENT, (bits & 1u) != 0u) | select(0u, FLAG_FLIP_Y, (bits & 2u) != 0u);
}

// A palette texture read with a palette sampler through this material's own table row
// (its slab's slots), with explicit gradients (mip selection as an implicit read would
// make it, without derivatives in this divergent branch).
@if(MESH_TERRAIN_BLEND && BINDLESS) fn detail_sample(instance_index: u32, image: u32, palette_sampler: u32, uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec4<f32> {
    let slot = material_slot(instance_index);
    let image_slot = nif_indices[slot].palette[image & 0xffffu];
    let sampler_slot = nif_indices[slot].palette[PALETTE_IMAGES + (palette_sampler & 0xffffu)];
    return textureSampleGrad(bindless_textures_2d[image_slot], bindless_samplers_filtering[sampler_slot], uv, ddx, ddy);
}

@if(MESH_TERRAIN_BLEND && BINDLESS) struct TerrainDetail {
    colour: vec3<f32>,
    normal: vec3<f32>,
}

// [design] #2551: a palette texture read anti-tiled, at every kept tap of `taps` with the
// footprint's gradients turned with the tap, blended by the taps' weights: the rule of
// nvf_assets terrain_sampling.wesl `terrain_sample_diffuse` and terrain_pbr.wesl
// `terrain_bindless_diffuse`, with the bindless table indexed here, where it is read
// (through this material's palette row, as `detail_sample`).
@if(MESH_TERRAIN_BLEND && BINDLESS) fn detail_sample_taps(instance_index: u32, image: u32, palette_sampler: u32, taps: TerrainTaps, fp: TerrainFootprint) -> vec4<f32> {
    let slot = material_slot(instance_index);
    let image_slot = nif_indices[slot].palette[image & 0xffffu];
    let sampler_slot = nif_indices[slot].palette[PALETTE_IMAGES + (palette_sampler & 0xffffu)];
    var ts = taps;
    var sum = vec4(0.0);
    for (var j = 0u; j < ts.count; j += 1u) {
        let tap = ts.taps[j];
        if tap.weight > 0.0 {
            sum += tap.weight * textureSampleGrad(bindless_textures_2d[image_slot], bindless_samplers_filtering[sampler_slot], tap.uv, tap.rotation * fp.dx, tap.rotation * fp.dy);
        }
    }
    return sum;
}

// [design] #2551: a palette normal map read anti-tiled (terrain_pbr.wesl
// `terrain_bindless_normal`'s rule: each tap decoded by `decode`'s StandardMaterial flags and
// turned back, blended, unit length), in the frame `tbn` (terrain_sampling.wesl
// `terrain_world_normal`; the landscape is never seen from behind).
@if(MESH_TERRAIN_BLEND && BINDLESS) fn detail_normal_taps(instance_index: u32, image: u32, palette_sampler: u32, decode: u32, tbn: mat3x3<f32>, taps: TerrainTaps, fp: TerrainFootprint) -> vec3<f32> {
    let slot = material_slot(instance_index);
    let image_slot = nif_indices[slot].palette[image & 0xffffu];
    let sampler_slot = nif_indices[slot].palette[PALETTE_IMAGES + (palette_sampler & 0xffffu)];
    let two_component = (decode & FLAG_TWO_COMPONENT) != 0u;
    let flip_y = (decode & FLAG_FLIP_Y) != 0u;
    var ts = taps;
    var sum = vec3(0.0);
    for (var j = 0u; j < ts.count; j += 1u) {
        let tap = ts.taps[j];
        if tap.weight > 0.0 {
            let encoded = textureSampleGrad(bindless_textures_2d[image_slot], bindless_samplers_filtering[sampler_slot], tap.uv, tap.rotation * fp.dx, tap.rotation * fp.dy).rgb;
            sum += tap.weight * terrain_normal_tap(encoded, two_component, flip_y, tap.rotation);
        }
    }
    return terrain_world_normal(tbn, terrain_unit_normal(sum), false);
}

// [design] #2550: a palette texture's height reference, placed as terrain.wesl's
// `height_reference_at` places it (global: the coarsest level, at the middle; local: two
// levels past the read's own level at the same place, never finer than level 4, the
// level from the coordinate's change across a view pixel `dx`, `dy` without the mip bias,
// scaled by the texture's size, plus the view's mip bias), read with the palette sampler at
// that explicit level.
@if(MESH_TERRAIN_BLEND && BINDLESS) fn detail_height_reference(instance_index: u32, image: u32, palette_sampler: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, bias: f32, local: bool) -> vec3<f32> {
    let slot = material_slot(instance_index);
    let image_slot = nif_indices[slot].palette[image & 0xffffu];
    let sampler_slot = nif_indices[slot].palette[PALETTE_IMAGES + (palette_sampler & 0xffffu)];
    let last_mip = f32(textureNumLevels(bindless_textures_2d[image_slot]) - 1u);
    var lod = last_mip;
    var coord = vec2(0.5);
    if local {
        let fsize = vec2<f32>(textureDimensions(bindless_textures_2d[image_slot], 0));
        let gx = dx * fsize;
        let gy = dy * fsize;
        let current_lod = 0.5 * log2(max(max(dot(gx, gx), dot(gy, gy)), 1e-12)) + bias;
        lod = min(last_mip, max(4.0, current_lod + 2.0));
        coord = uv;
    }
    return textureSampleLevel(bindless_textures_2d[image_slot], bindless_samplers_filtering[sampler_slot], coord, lod).rgb;
}

// [design] Round 10a: the near landscape's surface inputs at the fragment's game xy,
// composed as its draw composes them (terrain.wesl; terrain.rs `quadrant_mesh`, `folded_mesh`):
// the base diffuse x interpolated linear vertex colour x base material colour, then each
// overlay in drawing order mixed in by saturate(opacity) (its diffuse's alpha has no part,
// #3789; ordered opacities, not normalized weights), its colour the texel x vertex colour x the base
// material colour for folded overlays (terrain.wesl) and x white for the separately drawn
// overflow ones (terrain.rs `layer_material`: a layer drawn has a texture, so white). The
// normal (full shading only) starts as the base's mapped normal (or the plain interpolated
// one) and mixes towards each overlay's mapped normal (or the plain one) by the same shares,
// normalized once. Overflow layers are NVForever's approximation (graphics-enhanced.md
// §3.14): the near draw blends their lit result, this composes albedo and normal and lights
// once. `place` is the fragment's game xy, `cell` its cell.
//
// [design] #2550/#2551 (graphics-enhanced.md §3.14 "Height blending and anti-tiling"): with
// either switch on the near base draw (word 0 bits 4 and 5, nif_surface_blend.rs
// `DetailQuadrant::words`), the base draw is composed as terrain.wesl's TERRAIN_HEIGHT_BLEND
// and TERRAIN_ANTI_TILING paths compose it, with the same shared functions: the base and
// folded overlays get weights (terrain_weights.wesl `terrain_ordered_shares`, or the
// anti-tiled walk last to first that reads only where a share can be above 0), heights
// from the same raw texels against their references compete within the depth
// (`terrain_height`, `terrain_height_weights`; anti-tiled, the global reference always),
// colour and normal take the same weights, and the normal is made unit length once. A single
// base with anti-tiling reads as terrain_pbr.wesl `terrain_pbr_standard`. Anti-tiled reads
// share one footprint (the place in the world over the repeat, then the base's transform,
// as terrain_pbr.wesl `terrain_footprint`) and one set of Hex3 taps (`terrain_hex3`). The
// overflow layers then mix in as above, each read anti-tiled where its own material is
// (word 28); height blending never reaches them, as it does not in the near draw.
@if(MESH_TERRAIN_BLEND && BINDLESS) fn mesh_terrain_detail(instance_index: u32, entry: u32, t: TerrainTriangle, place: vec2<f32>, cell: vec2<i32>, inputs: MeshTerrainInputs) -> TerrainDetail {
    let q = entry * DETAIL_QUADRANT_WORDS;
    let flags = terrain_detail_word(instance_index, q);
    let total = min((flags >> 12u) & 15u, DETAIL_MAX_OVERLAYS);
    let folded = min((flags >> 8u) & 15u, total);
    let images = terrain_detail_word(instance_index, q + 1u);
    let samplers = terrain_detail_word(instance_index, q + 2u);
    let normals = terrain_detail_word(instance_index, q + 3u);
    let decode = terrain_detail_word(instance_index, q + 4u);
    let base_rgb = vec3(bitcast<f32>(terrain_detail_word(instance_index, q + 5u)), bitcast<f32>(terrain_detail_word(instance_index, q + 6u)), bitcast<f32>(terrain_detail_word(instance_index, q + 7u)));
    let axes = mat2x2<f32>(
        bitcast<f32>(terrain_detail_word(instance_index, q + 8u)), bitcast<f32>(terrain_detail_word(instance_index, q + 9u)),
        bitcast<f32>(terrain_detail_word(instance_index, q + 10u)), bitcast<f32>(terrain_detail_word(instance_index, q + 11u)));
    let offset = vec2(bitcast<f32>(terrain_detail_word(instance_index, q + 12u)), bitcast<f32>(terrain_detail_word(instance_index, q + 13u)));
    let tile = bitcast<f32>(terrain_detail_word(instance_index, q + 14u));
    let handedness = bitcast<f32>(terrain_detail_word(instance_index, q + 27u));
    // #2550/#2551: the near draws' switches as their materials have them.
    let height_blend = (flags & 16u) != 0u;
    let anti = (flags & 32u) != 0u;
    let overflow_anti = terrain_detail_word(instance_index, q + 28u);
    let weighted = height_blend || anti;
    // The near draw's texture coordinate: the place in the world over the repeat, or in
    // the cell for a six-layer folded draw (terrain.wesl), then the base's transform. The
    // gradients follow the same linear map; the view's mip bias scales them, as
    // textureSampleBias would.
    let local = select(place, place - vec2<f32>(cell) * CELL_SIZE, (flags & 2u) != 0u);
    let uv = axes * (local / tile) + offset;
    let scale = exp2(inputs.bias) / tile;
    let ddx = axes * inputs.ddx * scale;
    let ddy = axes * inputs.ddy * scale;
    // The anti-tiled reads' footprint and taps, only where something reads anti-tiled.
    var fp = TerrainFootprint(uv, ddx, ddy);
    var taps: TerrainTaps;
    if anti || overflow_anti != 0u {
        let repeat = bitcast<f32>(terrain_detail_word(instance_index, q + 31u));
        let at_scale = exp2(inputs.bias) / repeat;
        fp = TerrainFootprint(axes * (place / repeat) + offset, axes * inputs.ddx * at_scale, axes * inputs.ddy * at_scale);
        taps = terrain_hex3(fp.uv);
    }
    let diffuse_sampler = samplers & 0xffffu;
    let normal_sampler = samplers >> 16u;
    let a = detail_attributes(instance_index, t);
    // Bevy world space: game (x, y, z) is Bevy (x, z, -y), for the normal and the tangent.
    let world_n = game_to_world_normal(a.normal);
    let tbn = calculate_tbn_mikktspace(world_n, vec4(game_to_world_normal(a.tangent), handedness));
    let plain = normalize(world_n);
    let full = inputs.full != 0u;
    var colour = a.tint * base_rgb;
    var normal = plain;
    if !weighted {
        if (flags & 4u) != 0u { colour *= detail_sample(instance_index, images, diffuse_sampler, uv, ddx, ddy).rgb; }
        if full && (flags & 8u) != 0u {
            normal = apply_normal_mapping(detail_decode(decode, 0u), tbn, false, true, detail_sample(instance_index, images >> 16u, normal_sampler, uv, ddx, ddy).rgb);
        }
    } else {
        // The base draw's weights: base at 0, folded overlay k at k + 1 (terrain.wesl).
        let n = min(folded, 6u);
        var raw = array<vec4<f32>, 7>(vec4(1.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0));
        var w = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        if anti {
            var remaining = 1.0;
            for (var i = i32(n) - 1; i >= 0; i -= 1) {
                if remaining == 0.0 {
                    break;
                }
                let k = u32(i);
                let o = saturate(detail_opacity(instance_index, t, k));
                if o == 0.0 {
                    continue;
                }
                raw[k + 1u] = detail_sample_taps(instance_index, terrain_detail_word(instance_index, q + 15u + k), diffuse_sampler, taps, fp);
                w[k + 1u] = o * remaining;
                remaining *= 1.0 - o;
            }
            w[0] = remaining;
        } else {
            // [design] #2661 review: an overlay at opacity exactly 0 is not read. Its paint
            // stays 0, so terrain_ordered_shares gives it the share 0
            // and leaves every other share as it was; a height is taken only where the share
            // is not 0, terrain_height_weights keeps a zero share zero, and colour and
            // normal read a raw texel only where its weight is not 0, so the unread texel is
            // never used.
            var paint = array<f32, 6>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            for (var k = 0u; k < n; k += 1u) {
                let o = saturate(detail_opacity(instance_index, t, k));
                if o == 0.0 {
                    continue;
                }
                raw[k + 1u] = detail_sample(instance_index, terrain_detail_word(instance_index, q + 15u + k), diffuse_sampler, uv, ddx, ddy);
                paint[k] = o;
            }
            w = terrain_ordered_shares(paint, n);
        }
        // The base's raw diffuse (white without a texture), anti-tiled only where it shows.
        if (flags & 4u) != 0u {
            if !anti {
                raw[0] = detail_sample(instance_index, images, diffuse_sampler, uv, ddx, ddy);
            } else if w[0] != 0.0 {
                raw[0] = detail_sample_taps(instance_index, images, diffuse_sampler, taps, fp);
            }
        }
        var f = w;
        if height_blend {
            // Depth competition from the same raw texels. Anti-tiled, the global reference
            // always (terrain.wesl: a local one would need the same offset taps).
            let depth = bitcast<f32>(terrain_detail_word(instance_index, q + 29u));
            let gain = bitcast<f32>(terrain_detail_word(instance_index, q + 30u));
            let local_reference = (flags & 64u) != 0u && !anti;
            let height_dx = axes * inputs.ddx / tile;
            let height_dy = axes * inputs.ddy / tile;
            var height = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            if w[0] != 0.0 && (flags & 4u) != 0u {
                height[0] = terrain_height(raw[0].rgb, detail_height_reference(instance_index, images, diffuse_sampler, uv, height_dx, height_dy, inputs.bias, local_reference), gain);
            }
            for (var k = 0u; k < n; k += 1u) {
                if w[k + 1u] != 0.0 {
                    height[k + 1u] = terrain_height(raw[k + 1u].rgb, detail_height_reference(instance_index, terrain_detail_word(instance_index, q + 15u + k), diffuse_sampler, uv, height_dx, height_dy, inputs.bias, local_reference), gain);
                }
            }
            f = terrain_height_weights(w, height, depth);
        }
        // Colour and normal with the same weights; a normal map read only where its texture
        // contributes, then the sum made unit length once.
        var colour_sum = vec3(0.0);
        var normal_sum = vec3(0.0);
        if f[0] != 0.0 {
            colour_sum += f[0] * raw[0].rgb * a.tint * base_rgb;
            var base_n = plain;
            if full && (flags & 8u) != 0u {
                if anti {
                    base_n = detail_normal_taps(instance_index, images >> 16u, normal_sampler, detail_decode(decode, 0u), tbn, taps, fp);
                } else {
                    base_n = apply_normal_mapping(detail_decode(decode, 0u), tbn, false, true, detail_sample(instance_index, images >> 16u, normal_sampler, uv, ddx, ddy).rgb);
                }
            }
            normal_sum += f[0] * base_n;
        }
        for (var k = 0u; k < n; k += 1u) {
            if f[k + 1u] != 0.0 {
                colour_sum += f[k + 1u] * raw[k + 1u].rgb * a.tint * base_rgb;
                var layer_n = plain;
                if full && ((normals >> k) & 1u) != 0u {
                    let pair = terrain_detail_word(instance_index, q + 15u + k);
                    if anti {
                        layer_n = detail_normal_taps(instance_index, pair >> 16u, diffuse_sampler, detail_decode(decode, k + 1u), tbn, taps, fp);
                    } else {
                        layer_n = apply_normal_mapping(detail_decode(decode, k + 1u), tbn, false, true, detail_sample(instance_index, pair >> 16u, diffuse_sampler, uv, ddx, ddy).rgb);
                    }
                }
                normal_sum += f[k + 1u] * layer_n;
            }
        }
        colour = colour_sum;
        normal = select(plain, normalize(normal_sum), dot(normal_sum, normal_sum) > 1e-12);
    }
    // The overlays not composed above, in drawing order: every overlay without either switch
    // on the base draw, else the overflow ones. #2661 review: the opacity is known before the
    // diffuse read and is the share; an overlay whose opacity is exactly 0 is
    // skipped (no diffuse read, no normal-map read): mixing by 0 leaves colour and normal
    // exactly as they were.
    for (var k = select(0u, folded, weighted); k < total; k++) {
        let o = saturate(detail_opacity(instance_index, t, k));
        if o == 0.0 {
            continue;
        }
        let pair = terrain_detail_word(instance_index, q + 15u + k);
        let anti_k = ((overflow_anti >> k) & 1u) != 0u;
        var texel: vec4<f32>;
        if anti_k {
            texel = detail_sample_taps(instance_index, pair, diffuse_sampler, taps, fp);
        } else {
            texel = detail_sample(instance_index, pair, diffuse_sampler, uv, ddx, ddy);
        }
        let share = o;
        colour = mix(colour, texel.rgb * a.tint * select(vec3(1.0), base_rgb, k < folded), share);
        if full {
            var layer_n = plain;
            if ((normals >> k) & 1u) != 0u {
                if anti_k {
                    layer_n = detail_normal_taps(instance_index, pair >> 16u, diffuse_sampler, detail_decode(decode, k + 1u), tbn, taps, fp);
                } else {
                    layer_n = apply_normal_mapping(detail_decode(decode, k + 1u), tbn, false, true, detail_sample(instance_index, pair >> 16u, diffuse_sampler, uv, ddx, ddy).rgb);
                }
            }
            normal = mix(normal, layer_n, share);
        }
    }
    return TerrainDetail(colour, normalize(normal));
}

// [design] Round 10a: the fragment's ground triangle in its quadrant's 17 by 17 grid, as the
// height lookup picks it (`square`, the fraction `f` within it, `triangle` 1 above the
// south-west to north-east diagonal), and the barycentric weights of its corners: below
// the diagonal (1 - u) SW + (u - v) SE + v NE, above it (1 - v) SW + u NE + (v - u) NW.
@if(MESH_TERRAIN_BLEND && BINDLESS) fn terrain_triangle(entry: u32, quadrant: u32, square: vec2<i32>, f: vec2<f32>, triangle: i32) -> TerrainTriangle {
    let half_grid = GRID / 2;
    let local = square - vec2(i32(quadrant % 2u), i32(quadrant / 2u)) * half_grid;
    let side = u32(half_grid + 1);
    let sw = u32(local.y) * side + u32(local.x);
    var vertices = vec3(sw, sw + 1u, sw + side + 1u);
    var weights = vec3(1.0 - f.x, f.x - f.y, f.y);
    if triangle == 1 {
        vertices = vec3(sw, sw + side + 1u, sw + side);
        weights = vec3(1.0 - f.y, f.x, f.y - f.x);
    }
    let start = DETAIL_QUADRANTS * DETAIL_QUADRANT_WORDS + entry * QUAD_VERTICES * DETAIL_VERTEX_WORDS;
    return TerrainTriangle(vec3(start) + vertices * DETAIL_VERTEX_WORDS, weights);
}

// [design] Colour texels per cell side: the bound atlas is three cells wide
// (terrain_surface.rs, 512, 1024 or 2048 per cell).
@if(MESH_TERRAIN_BLEND) fn terrain_resolution(instance_index: u32) -> i32 {
    @if(BINDLESS) let size = textureDimensions(bindless_textures_2d[nif_indices[material_slot(instance_index)].holes_texture], 0);
    @if(!BINDLESS) let size = textureDimensions(mesh_terrain_colour, 0);
    return i32(size.x) / 3;
}

// [design] Enabled-only coverage at the retained publication's OLD origin. Baked
// height/quadrant validity is immutable; every lookup intersects it with live coverage.
@if(MESH_TERRAIN_BLEND) fn terrain_covered(instance_index: u32, slot: vec2<i32>) -> bool {
    if any(slot < vec2(0)) || any(slot >= vec2(3)) { return false; }
    let coverage = u32(nif_surface(instance_index).mesh_terrain_origin.z);
    return (coverage & (1u << u32(slot.y * 3 + slot.x))) != 0u;
}

@if(MESH_TERRAIN_BLEND) fn terrain_cell_resident(instance_index: u32, slot: vec2<i32>) -> bool {
    if any(slot < vec2(0)) || any(slot >= vec2(3)) { return false; }
    return terrain_covered(instance_index, slot) && terrain_height_texel(instance_index, slot * HEIGHT_TILE) > -1e30;
}

@if(MESH_TERRAIN_BLEND) fn terrain_colour_mask(instance_index: u32, slot: vec2<i32>) -> u32 {
    return u32(terrain_height_texel(instance_index, slot * HEIGHT_TILE + vec2(1, HEIGHT_GRID)));
}
@if(MESH_TERRAIN_BLEND) fn terrain_colour_valid(instance_index: u32, slot: vec2<i32>, quadrant: u32) -> bool {
    if !terrain_covered(instance_index, slot) { return false; }
    return (terrain_colour_mask(instance_index, slot) & (1u << quadrant)) != 0u;
}

// [design] Round 10b: the colour atlas has a mip chain per cell (terrain_surface.rs
// `colour_levels`): level k holds `resolution >> k` texels per cell side, down to one, each
// level a 3 by 3 grid of whole cells filtered from that cell's own texels only (the read,
// not the filter, crosses cell edges: `terrain_colour_taps`). The last level is log2 of the
// texels per cell side.
@if(MESH_TERRAIN_BLEND) fn terrain_last_level(resolution: i32) -> i32 {
    return i32(firstTrailingBit(u32(max(resolution, 1))));
}

// [design] Round 10b: the colour read's level of detail, as an implicit read would choose it:
// log2 of the larger of the fragment's two screen-axis footprints in level-0 texels (the
// game-xy derivatives, taken by the caller in uniform control flow before any lookup
// branch, times the atlas's texels per game unit), plus the view's mip bias, clamped to the
// atlas's levels.
@if(MESH_TERRAIN_BLEND) fn terrain_colour_lod(resolution: i32, inputs: MeshTerrainInputs) -> f32 {
    let footprint = max(length(inputs.ddx), length(inputs.ddy)) * f32(resolution) / CELL_SIZE;
    return clamp(log2(max(footprint, 1e-6)) + inputs.bias, 0.0, f32(terrain_last_level(resolution)));
}

// [design] Round 10b (terrain_surface.rs `colour_read_levels`): the level below the level of
// detail, the one above it (the last stays the last) and the upper one's share.
@if(MESH_TERRAIN_BLEND) struct TerrainColourLevels {
    low: i32,
    high: i32,
    share: f32,
}
@if(MESH_TERRAIN_BLEND) fn terrain_colour_levels(resolution: i32, lod: f32) -> TerrainColourLevels {
    let low = i32(floor(lod));
    return TerrainColourLevels(low, min(low + 1, terrain_last_level(resolution)), lod - f32(low));
}

// [design] Round 10b review (terrain_surface.rs `colour_tap_valid`): whether the texel `tap`
// (atlas texels of a level with `size` texels per cell side; the atlas's rows run north to
// south, the bake camera's reversed Y) can be read: inside the atlas, its cell live-covered
// and the texel in a captured quadrant of that cell. A texel of a level with two or more
// texels per cell side covers level-0 texels of one quadrant only, the quadrant its own
// position names; the last level's single texel covers all four, so it needs all four.
@if(MESH_TERRAIN_BLEND) fn terrain_colour_tap_valid(instance_index: u32, tap: vec2<i32>, size: i32) -> bool {
    if any(tap < vec2(0)) || any(tap >= vec2(3 * size)) { return false; }
    let cell = tap / size;
    let slot = vec2(cell.x, 2 - cell.y);
    if !terrain_covered(instance_index, slot) { return false; }
    let mask = terrain_colour_mask(instance_index, slot);
    if size <= 1 { return (mask & 15u) == 15u; }
    let t = tap - cell * size;
    let middle = size / 2;
    let quadrant = select(0u, 1u, t.x >= middle) + select(0u, 2u, t.y < middle);
    return (mask & (1u << quadrant)) != 0u;
}

// [design] Round 10b review (terrain_surface.rs `colour_read_taps`): one level's bilinear
// read, its first (north-west) tap and the four taps' weights in the order (0, 0), (1, 0),
// (0, 1), (1, 1). Each level is filtered per cell, but the 3 by 3 atlas puts neighbouring
// cells' texels side by side, so the read is the plain bilinear footprint at the fragment's
// position, crossing a cell edge into the neighbour's adjacent texels (a texel grid as
// regular across the edge as inside a cell: a smooth colour stays continuous there). A
// tap that cannot be read (outside the atlas, an uncovered cell, an uncaptured quadrant)
// gets no weight and the others are renormalized; where a whole neighbouring column or row
// is missing that is the read clamped to the fragment's own cell. The weights depend on
// the position and the taps' validity only, never on which cell the fragment is in, so
// fragments on both sides of an edge between two valid cells read the same value. `valid`:
// the footprint holds a tap of the fragment's own cell and every such tap can be read
// (an uncaptured quadrant of its own cell rejects the read, as before).
@if(MESH_TERRAIN_BLEND) struct TerrainColourTaps {
    first: vec2<i32>,
    weights: vec4<f32>,
    valid: bool,
}
@if(MESH_TERRAIN_BLEND) fn terrain_colour_taps(instance_index: u32, slot: vec2<i32>, uv: vec2<f32>, size: i32) -> TerrainColourTaps {
    let p = uv * f32(3 * size) - vec2(0.5);
    let first = vec2<i32>(floor(p));
    let f = p - floor(p);
    let own = vec2(slot.x, 2 - slot.y);
    var weights = vec4((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
    var has_own = false;
    var own_valid = true;
    for (var i = 0; i < 4; i++) {
        let tap = first + vec2(i % 2, i / 2);
        let valid = terrain_colour_tap_valid(instance_index, tap, size);
        if all(tap >= vec2(0)) && all(tap / size == own) {
            has_own = true;
            own_valid = own_valid && valid;
        }
        if !valid { weights[i] = 0.0; }
    }
    let total = weights.x + weights.y + weights.z + weights.w;
    return TerrainColourTaps(first, weights / max(total, 1e-6), has_own && own_valid && total > 0.0);
}

// [design] Round 10b review: the colour at one level, the readable taps' renormalized
// bilinear mix (the last level's texels sit at the cells' centres, so there it interpolates
// between the centres of valid neighbouring cells).
@if(MESH_TERRAIN_BLEND) fn terrain_colour_level(instance_index: u32, slot: vec2<i32>, uv: vec2<f32>, resolution: i32, level: i32) -> vec3<f32> {
    let t = terrain_colour_taps(instance_index, slot, uv, resolution >> u32(level));
    var colour = vec3(0.0);
    for (var i = 0; i < 4; i++) {
        if t.weights[i] > 0.0 { colour += t.weights[i] * terrain_colour_load(instance_index, t.first + vec2(i % 2, i / 2), level); }
    }
    return colour;
}

// [design] Round 10b (terrain_surface.rs `colour_read_taps`): the level's read is valid.
@if(MESH_TERRAIN_BLEND) fn terrain_colour_level_valid(instance_index: u32, slot: vec2<i32>, uv: vec2<f32>, resolution: i32, level: i32) -> bool {
    return terrain_colour_taps(instance_index, slot, uv, resolution >> u32(level)).valid;
}

// [design] Round 10b: the colour footprint is valid when both levels that contribute (the
// upper only with a share) are; the cell's own live coverage is part of the height check
// before it, and every neighbouring tap either level weights is covered and captured.
@if(MESH_TERRAIN_BLEND) fn terrain_colour_footprint(instance_index: u32, slot: vec2<i32>, uv: vec2<f32>, resolution: i32, l: TerrainColourLevels) -> bool {
    if !terrain_colour_level_valid(instance_index, slot, uv, resolution, l.low) { return false; }
    return l.share <= 0.0 || l.high == l.low || terrain_colour_level_valid(instance_index, slot, uv, resolution, l.high);
}

// [design] Round 10b: the cached colour, the two levels' reads mixed by the upper's share.
@if(MESH_TERRAIN_BLEND) fn terrain_colour_mips(instance_index: u32, slot: vec2<i32>, uv: vec2<f32>, resolution: i32, l: TerrainColourLevels) -> vec3<f32> {
    let low = terrain_colour_level(instance_index, slot, uv, resolution, l.low);
    if l.share <= 0.0 || l.high == l.low { return low; }
    return mix(low, terrain_colour_level(instance_index, slot, uv, resolution, l.high), l.share);
}

// [design] nif_surface_blend.rs::terrain_game_normal and game_to_world_normal: the slopes
// are per surface square (HEIGHT_SPACING).
@if(MESH_TERRAIN_BLEND) fn terrain_game_normal(dx: f32, dy: f32) -> vec3<f32> {
    return normalize(vec3(-dx / HEIGHT_SPACING, -dy / HEIGHT_SPACING, 1.0));
}
// [design] #2149/#2661: the square of a `grid`-squares-a-side cell grid under `local`
// (squares from the cell's south-west corner), the fraction within it and the triangle (1
// above the south-west to north-east diagonal), as fnv_world land_surface.rs
// `grid_height_at` splits a surface's squares.
@if(MESH_TERRAIN_BLEND) struct TerrainSquare {
    square: vec2<i32>,
    f: vec2<f32>,
    triangle: i32,
}
@if(MESH_TERRAIN_BLEND) fn terrain_square(local: vec2<f32>, grid: i32) -> TerrainSquare {
    let square = min(vec2<i32>(floor(local)), vec2(grid - 1));
    let f = local - vec2<f32>(square);
    return TerrainSquare(square, f, select(0, 1, f.y > f.x));
}
@if(MESH_TERRAIN_BLEND) fn game_to_world_normal(n: vec3<f32>) -> vec3<f32> {
    return vec3(n.x, n.z, -n.y);
}

// [design] nif_surface_blend.rs::v2_band: widths are (flat, band, steep); the flat width
// only parallel to the ground, ramping smoothly to the band at 5 degrees (not held over
// 0-5 degrees), the band from 5 to 35 degrees, ramping to steep at 75 degrees and beyond;
// abs treats opposite parallel normals alike.
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_band(widths: vec3<f32>, cos_angle: f32) -> f32 {
    let c = clamp(abs(cos_angle), 0.0, 1.0);
    let shallow = 1.0 - smoothstep(V2_COS_PARALLEL, 1.0, c);
    let steep = 1.0 - smoothstep(V2_COS_STEEP, COS_FULL, c);
    return widths.x + (widths.y - widths.x) * shallow + (widths.z - widths.y) * steep;
}

// [design] nif_surface_blend.rs::contact_weight: shape 0 height band, 1 penetration,
// 2 hybrid v2 (the larger of the angle-widened height band and the penetration term),
// 3 hybrid v1 (round 5: the band times the larger of the orientation and penetration
// terms). Widths are (flat, band, steep); only hybrid v2 reads flat and steep.
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_weight(shape: u32, widths: vec3<f32>, d: f32, cos_angle: f32) -> f32 {
    let height = 1.0 - smoothstep(0.0, widths.y, max(d, 0.0));
    let penetration = 1.0 - smoothstep(-PENETRATION, 0.0, d);
    if shape == 0u { return height; }
    if shape == 1u { return penetration; }
    if shape == 3u {
        let turned = 1.0 - smoothstep(COS_FULL, COS_NONE, clamp(cos_angle, -1.0, 1.0));
        return height * max(turned, penetration);
    }
    let band = mesh_terrain_band(widths, cos_angle);
    return max(1.0 - smoothstep(0.0, band, max(d, 0.0)), penetration);
}

// [design] nif_surface_blend.rs::face_up (#2661 review): everything the blend paints is
// placed by world xy, a top-down projection that stretches one row of texels down a
// near-vertical face. The face's steepness comes from its geometric normal, the cross
// product of the world position's screen derivatives `dx`, `dy` (taken by the caller in
// uniform control flow, before any discard), not from the interpolated vertex normal,
// which smoothing tilts towards up on a road's lip. Bevy world space: up is +y; abs treats
// back faces as front faces. Where the derivatives are (nearly) parallel or zero (a face
// seen edge-on) the cross product says nothing and the interpolated vertex normal stands
// in; without one either, 1 (no fade).
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_face_up(dx: vec3<f32>, dy: vec3<f32>, mesh_normal: vec3<f32>) -> f32 {
    let face = cross(dx, dy);
    let face_length = length(face);
    if face_length > 1e-6 * length(dx) * length(dy) { return min(abs(face.y) / face_length, 1.0); }
    let n_length = length(mesh_normal);
    if n_length > 1e-6 { return min(abs(mesh_normal.y) / n_length, 1.0); }
    return 1.0;
}

// [design] nif_surface_blend.rs::steepness_fade: 1 up to STEEP_COS_FULL (60 degrees from
// horizontal), 0 from STEEP_COS_NONE (80 degrees), smooth between; it multiplies every
// contact shape's weight.
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_steepness(up: f32) -> f32 {
    return smoothstep(STEEP_COS_NONE, STEEP_COS_FULL, clamp(up, 0.0, 1.0));
}

// [design] nif_surface_blend.rs::blend_normal: exact at w <= 0, the ground normal where
// the mix cancels out.
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_blend_normal(mesh: vec3<f32>, terrain: vec3<f32>, w: f32) -> vec3<f32> {
    if w <= 0.0 { return mesh; }
    let mixed = mix(mesh, terrain, min(w, 1.0));
    if dot(mixed, mixed) < 1e-12 { return terrain; }
    return normalize(mixed);
}

// [design] Coverage, alpha, vertices and depth stay with nif_material.wesl. `mesh_normal`
// is the interpolated vertex normal before normal mapping (pbr_input.world_normal), so
// the weight does not follow the normal map's texture. Round 10a: two sources can serve
// a fragment with valid heights, each checked on its own: the colour cache (its quadrant
// and, round 10b, its whole footprint at both contributing mip levels) and the landscape's layers (the quadrant's detail, the
// fragment nearer than DETAIL_FAR); a failed colour footprint no longer fails a fragment
// the layers serve. `status` is LOOKUP_SUCCESS when either serves it. #2661 review: the
// contact weight is faded by the face's steepness (`mesh_terrain_steepness` of
// `inputs.up`), for every contact shape and so in the weight view too.
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_lookup(instance_index: u32, world: vec3<f32>, mesh_normal: vec3<f32>, inputs: MeshTerrainInputs) -> MeshTerrainLookup {
    let s = nif_surface(instance_index);
    var r = MeshTerrainLookup(LOOKUP_OUT_OF_RANGE, 0.0, vec3(0.0), vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 0.0), 0.0, vec3(0.0), vec3(0.0, 1.0, 0.0));
    let game = vec3(world.x, -world.z, world.y);
    let cell = vec2<i32>(floor(game.xy / CELL_SIZE));
    let slot = cell - vec2<i32>(s.mesh_terrain_origin.xy);
    if any(slot < vec2(0)) || any(slot >= vec2(3)) { return r; }
    let local = game.xy / SPACING - vec2<f32>(cell) * f32(GRID);
    let square = min(vec2<i32>(floor(local)), vec2(GRID - 1));
    let f = local - vec2<f32>(square);
    let triangle = select(0, 1, f.y > f.x);
    // [design] #2149/#2661: the ground's own square, SUB per LAND square and side: the drawn
    // surface's triangle (terrain_surface.rs `encode_height_cell`); with SUB 1 the LAND
    // square above. The LAND square stays the quadrant's and the detail's.
    @if(MESH_TERRAIN_SUBDIVIDED) let ground = terrain_square(local * f32(SUB), HEIGHT_GRID);
    @if(!MESH_TERRAIN_SUBDIVIDED) let ground = TerrainSquare(square, f, triangle);
    let base = slot * HEIGHT_TILE + vec2(ground.square.x * 6 + ground.triangle * 3, ground.square.y);
    // Coefficients are built by terrain_surface.rs::height_planes from the real mesh diagonal.
    let a = terrain_height_texel(instance_index, base);
    let dx = terrain_height_texel(instance_index, base + vec2(1, 0));
    let dy = terrain_height_texel(instance_index, base + vec2(2, 0));
    if !terrain_covered(instance_index, slot) || a < -1e30 { r.status = LOOKUP_INVALID_HEIGHT; return r; }
    r.game_normal = terrain_game_normal(dx, dy);
    r.normal = game_to_world_normal(r.game_normal);
    let height = a + ground.f.x * dx + ground.f.y * dy;
    // [design] #2661 review: the contact weight needs only the three height loads above, the
    // ground triangle's normal and the face's steepness; the residency fade and the distance
    // mix below only multiply it by values in [0, 1]. Where it is exactly 0 the final weight
    // is 0 and nif_material.wesl's surface view leaves the fragment as it is, so the lookup
    // stops here, before the colour and detail masks, the neighbour residency, the colour
    // level and footprint work. A diagnostic view (`mesh_terrain_view` != 0: weight, normal,
    // selection, status) never stops here, so its classification and precedence are exactly
    // the full lookup's.
    let n_length = length(mesh_normal);
    let cos_angle = select(dot(mesh_normal / max(n_length, 1e-6), r.normal), 1.0, n_length < 1e-6);
    let widths = vec3(s.mesh_terrain_widths.x, s.mesh_terrain.x, s.mesh_terrain_widths.y);
    let contact = mesh_terrain_weight(u32(s.mesh_terrain.z), widths, game.z - height, cos_angle) * mesh_terrain_steepness(inputs.up);
    if contact == 0.0 && u32(s.mesh_terrain.y) == 0u { r.status = LOOKUP_NO_CONTACT; return r; }
    let quadrant = select(0u, 1u, local.x >= f32(GRID) * 0.5) + select(0u, 2u, local.y >= f32(GRID) * 0.5);
    let colour_quadrant = terrain_colour_valid(instance_index, slot, quadrant);
    let detail_valid = inputs.distance < DETAIL_FAR && terrain_detail_valid(instance_index, slot, quadrant);
    if !colour_quadrant && !detail_valid { r.status = LOOKUP_INVALID_COLOUR; return r; }
    // [design] Fade over one LAND grid interval next to missing slots (the region's edge).
    // Round 10b review: the colour read gives a missing neighbour's taps no weight (the read
    // clamped to the cell there); the fade keeps the region's edge soft as before.
    let within = game.xy - vec2<f32>(cell) * CELL_SIZE;
    var border = CELL_SIZE;
    if !terrain_cell_resident(instance_index, slot + vec2(-1, 0)) { border = min(border, within.x); }
    if !terrain_cell_resident(instance_index, slot + vec2(1, 0)) { border = min(border, CELL_SIZE - within.x); }
    if !terrain_cell_resident(instance_index, slot + vec2(0, -1)) { border = min(border, within.y); }
    if !terrain_cell_resident(instance_index, slot + vec2(0, 1)) { border = min(border, CELL_SIZE - within.y); }
    // Round 10b: the colour read takes two levels of the per-cell mip chains at the level of
    // detail the derivatives give, each bilinear across valid neighbouring cells (review);
    // every own-cell tap either level with a share reads must lie in a captured quadrant,
    // and neighbouring taps that do not get no weight. The atlas Y axis is reversed by the
    // bake camera.
    //
    // [design] #2661 review: with valid detail nearer than DETAIL_NEAR the colour cache
    // cannot matter, so its footprint is not checked. There the distance mix `near` below is
    // exactly 1 (smoothstep of a distance under its lower edge is 0), so with or without a
    // valid colour footprint the status is LOOKUP_SUCCESS (detail serves), `r.detail` is 1
    // and the weight is the same (times 1 on the detail-only branch); and at `r.detail == 1`
    // the cache is not read (below). No view shows `colour_valid` otherwise.
    let detail_alone = detail_valid && inputs.distance < DETAIL_NEAR;
    var colour_valid = false;
    var resolution = 0;
    var uv = vec2(0.0);
    var levels = TerrainColourLevels(0, 0, 0.0);
    if colour_quadrant && !detail_alone {
        resolution = terrain_resolution(instance_index);
        let atlas = (game.xy / CELL_SIZE - s.mesh_terrain_origin.xy) / 3.0;
        uv = vec2(atlas.x, 1.0 - atlas.y);
        levels = terrain_colour_levels(resolution, terrain_colour_lod(resolution, inputs));
        colour_valid = terrain_colour_footprint(instance_index, slot, uv, resolution, levels);
    }
    if !colour_valid && !detail_valid { r.status = LOOKUP_REJECTED_FOOTPRINT; return r; }
    r.status = LOOKUP_SUCCESS;
    var weight = contact * smoothstep(0.0, SPACING, border);
    // [design] Round 10a's distance mix d (1 to DETAIL_NEAR, 0 from DETAIL_FAR): with both
    // sources the layers' share of the target; with the layers alone a fade of the weight,
    // so the blend never mixes towards an invalid colour texel.
    let near = 1.0 - smoothstep(DETAIL_NEAR, DETAIL_FAR, inputs.distance);
    if detail_valid && colour_valid { r.detail = near; } else if detail_valid { r.detail = 1.0; weight *= near; }
    r.weight = weight;
    if r.weight > 0.0 {
        // [design] #2661 review: at `r.detail == 1` the target colour is the layers' alone
        // (`mesh_terrain_target_colour`: mix by 1), and no diagnostic view reads `r.colour`,
        // so the cache is read only where it has a share.
        if colour_valid && r.detail < 1.0 { r.colour = terrain_colour_mips(instance_index, slot, uv, resolution, levels); }
        @if(BINDLESS) if r.detail > 0.0 {
            let entry = u32(slot.y * 3 + slot.x) * 4u + quadrant;
            let d = mesh_terrain_detail(instance_index, entry, terrain_triangle(entry, quadrant, square, f, triangle), game.xy, cell, inputs);
            r.detail_colour = d.colour;
            r.detail_normal = d.normal;
        }
    }
    return r;
}

// [design] Round 10a: the target colour and normal of a successful lookup: the colour
// cache's colour and the ground triangle's geometric normal, mixed towards the layers' by
// their share (the normal renormalized; where the mix cancels the layers' stands in).
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_target_colour(t: MeshTerrainLookup) -> vec3<f32> {
    return mix(t.colour, t.detail_colour, t.detail);
}
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_target_normal(t: MeshTerrainLookup) -> vec3<f32> {
    if t.detail <= 0.0 { return t.normal; }
    let mixed = mix(t.normal, t.detail_normal, t.detail);
    if dot(mixed, mixed) < 1e-12 { return t.detail_normal; }
    return normalize(mixed);
}

// [design] The aid's view: 0 surface, 1 weight, 2 normal, 3 selection, 4 status.
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_view(instance_index: u32) -> u32 {
    return u32(nif_surface(instance_index).mesh_terrain.y);
}
// [design] 0 albedo, 1 full (colour, normal, roughness, reflectance, metallic, emissive).
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_shading(instance_index: u32) -> u32 {
    return u32(nif_surface(instance_index).mesh_terrain.w);
}
// [design] The weight, normal and status views are drawn unlit, as they are, so their
// colours read without the scene's light (nif_material.wesl).
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_unlit_view(view: u32) -> bool {
    return view == 1u || view == 2u || view == 4u;
}

// [design] The diagnostic colour of a view other than surface; `colour` is the mesh's own
// (unlit) colour. Status precedence: magenta out of range, red invalid height, orange
// invalid colour quadrant, cyan rejected footprint, green success with brightness
// 0.2 + 0.8 weight. Weight: blue (0) to red (1), the mesh's colour where the lookup fails.
// Normal: the ground triangle's game-space normal as 0.5 + 0.5 n (flat ground (0.5,
// 0.5, 1)), black without a valid height. Selection: a green tint (drawn lit).
@if(MESH_TERRAIN_BLEND) fn mesh_terrain_diagnostic(view: u32, t: MeshTerrainLookup, colour: vec3<f32>) -> vec3<f32> {
    if view == 3u { return mix(colour, vec3(0.1, 0.8, 0.25), 0.6); }
    if view == 2u {
        if t.status < LOOKUP_INVALID_COLOUR { return vec3(0.0); }
        return t.game_normal * 0.5 + vec3(0.5);
    }
    if view == 1u {
        if t.status != LOOKUP_SUCCESS { return colour; }
        return vec3(t.weight, 0.0, 1.0 - t.weight);
    }
    if t.status == LOOKUP_OUT_OF_RANGE { return vec3(1.0, 0.0, 1.0); }
    if t.status == LOOKUP_INVALID_HEIGHT { return vec3(1.0, 0.0, 0.0); }
    if t.status == LOOKUP_INVALID_COLOUR { return vec3(1.0, 0.35, 0.0); }
    if t.status == LOOKUP_REJECTED_FOOTPRINT { return vec3(0.0, 1.0, 1.0); }
    return vec3(0.0, 0.2 + 0.8 * t.weight, 0.0);
}

// The material's slot in the bindless tables (unused without bindless).
fn material_slot(instance_index: u32) -> u32 {
    return mesh[instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
}

fn nif_surface(instance_index: u32) -> NifSurface {
    @if(BINDLESS) return nif_surfaces[nif_indices[material_slot(instance_index)].surface];
    @if(!BINDLESS) return nif_surface_values;
}

// The standard part of the material.
fn standard_material(instance_index: u32) -> StandardMaterial {
    @if(BINDLESS) return material_array[material_indices[material_slot(instance_index)].material];
    @if(!BINDLESS) return pbr_bindings::material;
}

// The holes texture's alpha at `uv` (0-1).
fn holes_alpha(instance_index: u32, uv: vec2<f32>) -> f32 {
    @if(BINDLESS) let i = nif_indices[material_slot(instance_index)];
    @if(BINDLESS) return textureSampleLevel(bindless_textures_2d[i.holes_texture], bindless_samplers_filtering[i.holes_sampler], uv, 0.0).a;
    @if(!BINDLESS && !NIF_SURFACE_BLEND) return textureSampleLevel(holes_texture, holes_sampler, uv, 0.0).a;
    // [design] The blend type has no holes (nif_surface_blend.rs module notes): its holes
    // value is always none, so the callers' holes test never reads this.
    @if(!BINDLESS && NIF_SURFACE_BLEND) return 0.0;
}

// The environment cube in the direction `dir` (cube space), at its top level.
@if(!NIF_SURFACE_BLEND) fn env_cube_texel(instance_index: u32, dir: vec3<f32>) -> vec3<f32> {
    @if(BINDLESS) let i = nif_indices[material_slot(instance_index)];
    @if(BINDLESS) return textureSampleLevel(bindless_textures_cube[i.env_cube], bindless_samplers_filtering[i.env_sampler], dir, 0.0).rgb;
    @if(!BINDLESS) return textureSampleLevel(env_cube, env_sampler, dir, 0.0).rgb;
}

// The environment mask's red channel at `uv`, at its top level.
@if(!NIF_SURFACE_BLEND) fn env_mask_texel(instance_index: u32, uv: vec2<f32>) -> f32 {
    @if(BINDLESS) let i = nif_indices[material_slot(instance_index)];
    @if(BINDLESS) return textureSampleLevel(bindless_textures_2d[i.env_mask], bindless_samplers_filtering[i.env_sampler], uv, 0.0).r;
    @if(!BINDLESS) return textureSampleLevel(env_mask, env_sampler, uv, 0.0).r;
}

// The environment mask's alpha at `uv`, at its top level: in a debug switch's variant
// (env_map.rs NVF_PLAY_DEBUG_ENV_NO_MASK=normal-alpha) the mask slot holds the game's own
// normal map (nif_spawn.rs `shape_material_inner`), so an authored set that replaces the
// drawn normal map does not change what is read here.
@if(!NIF_SURFACE_BLEND) fn env_mask_alpha(instance_index: u32, uv: vec2<f32>) -> f32 {
    @if(BINDLESS) let i = nif_indices[material_slot(instance_index)];
    @if(BINDLESS) return textureSampleLevel(bindless_textures_2d[i.env_mask], bindless_samplers_filtering[i.env_sampler], uv, 0.0).a;
    @if(!BINDLESS) return textureSampleLevel(env_mask, env_sampler, uv, 0.0).a;
}

// The size of the FaceGen offset image.
fn facegen_size(instance_index: u32) -> vec2<u32> {
    @if(BINDLESS) return textureDimensions(bindless_textures_2d[nif_indices[material_slot(instance_index)].facegen_offset]);
    @if(!BINDLESS && !NIF_SURFACE_BLEND) return textureDimensions(facegen_offset);
    // [design] The blend type never has FACEGEN_TINT (its key's facegen is false).
    @if(!BINDLESS && NIF_SURFACE_BLEND) return vec2(1u);
}

// One texel of the FaceGen offset image.
fn facegen_texel(instance_index: u32, p: vec2<i32>) -> vec3<f32> {
    @if(BINDLESS) return textureLoad(bindless_textures_2d[nif_indices[material_slot(instance_index)].facegen_offset], p, 0).rgb;
    @if(!BINDLESS && !NIF_SURFACE_BLEND) return textureLoad(facegen_offset, p, 0).rgb;
    @if(!BINDLESS && NIF_SURFACE_BLEND) return vec3(0.0);
}

// The base colour texture sampled at `uv` with the view's mip bias `bias`.
fn base_color_texel(instance_index: u32, uv: vec2<f32>, bias: f32) -> vec4<f32> {
    @if(BINDLESS) let i = material_indices[material_slot(instance_index)];
    @if(BINDLESS) return textureSampleBias(bindless_textures_2d[i.base_color_texture], bindless_samplers_filtering[i.base_color_sampler], uv, bias);
    @if(!BINDLESS) return textureSampleBias(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler, uv, bias);
}

// The base colour texture sampled at `uv` at mip level `level`, with no implicit
// derivatives (the Pip-Boy screen's taps, nif_material.wesl `pipboy_picture`).
fn base_color_texel_level(instance_index: u32, uv: vec2<f32>, level: f32) -> vec4<f32> {
    @if(BINDLESS) let i = material_indices[material_slot(instance_index)];
    @if(BINDLESS) return textureSampleLevel(bindless_textures_2d[i.base_color_texture], bindless_samplers_filtering[i.base_color_sampler], uv, level);
    @if(!BINDLESS) return textureSampleLevel(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler, uv, level);
}

// The size of the base colour texture (the Pip-Boy screen's taps, nif_material.wesl
// `pipboy_picture`).
fn base_color_size(instance_index: u32) -> vec2<u32> {
    @if(BINDLESS) return textureDimensions(bindless_textures_2d[material_indices[material_slot(instance_index)].base_color_texture]);
    @if(!BINDLESS) return textureDimensions(pbr_bindings::base_color_texture);
}

// The occlusion texture's red channel sampled at `uv` with the view's mip bias `bias` (the
// authored materials' occlusion view, nif_material.wesl "NVF_PBR_VIEW"); the caller checks
// the material has one.
fn occlusion_texel(instance_index: u32, uv: vec2<f32>, bias: f32) -> f32 {
    @if(BINDLESS) let i = material_indices[material_slot(instance_index)];
    @if(BINDLESS) return textureSampleBias(bindless_textures_2d[i.occlusion_texture], bindless_samplers_filtering[i.occlusion_sampler], uv, bias).r;
    @if(!BINDLESS) return textureSampleBias(pbr_bindings::occlusion_texture, pbr_bindings::occlusion_sampler, uv, bias).r;
}

// The depth map's red channel (the authored height map as stored: 1 high, 0 low, #2981)
// sampled at `uv` with the view's mip bias `bias` (the authored materials' height view,
// nif_material.wesl "NVF_PBR_VIEW"); the caller checks the material has one.
fn height_texel(instance_index: u32, uv: vec2<f32>, bias: f32) -> f32 {
    @if(BINDLESS) let i = material_indices[material_slot(instance_index)];
    @if(BINDLESS) return textureSampleBias(bindless_textures_2d[i.depth_map_texture], bindless_samplers_filtering[i.depth_map_sampler], uv, bias).r;
    @if(!BINDLESS) return textureSampleBias(pbr_bindings::depth_map_texture, pbr_bindings::depth_map_sampler, uv, bias).r;
}

// The normal map sampled at `uv`.
fn normal_map_texel(instance_index: u32, uv: vec2<f32>) -> vec4<f32> {
    @if(BINDLESS) let i = material_indices[material_slot(instance_index)];
    @if(BINDLESS) return textureSample(bindless_textures_2d[i.normal_map_texture], bindless_samplers_filtering[i.normal_map_sampler], uv);
    @if(!BINDLESS) return textureSample(pbr_bindings::normal_map_texture, pbr_bindings::normal_map_sampler, uv);
}
