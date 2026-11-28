// Landscape with its first texture layers folded into the base's draw (terrain.rs "One
// draw for the base and its first layers"): the fragment shader of `TerrainMaterial`.
//
// The mesh carries the opacity of up to six layers a vertex (terrain.rs `folded_mesh`):
// the vertex colour's alpha, the second texture coordinate's two numbers, the tangent's
// fourth number, whose size past 1 is the opacity while its sign stays the tangent's
// handedness, and, when the material gives a texture repeat, the first texture
// coordinate's two numbers; the coordinate itself is then the fragment's place in its
// mesh over the repeat, which is the mesh's coordinate but for whole repeats. The surface
// colour and the normal are the base's, blended towards each
// layer's in the layers' order by its opacity, as the layers' own blended draws blended
// their lit colours over the base; then the surface is lit once. A layer texture's alpha
// has no part in it: the original draws the layers opaque whatever it is
// (docs/spec-notes/world.md section 3 "Landscape layers", #3789).
// What follows the lighting is nif_material.wesl's for an opaque lit shape with no emitted
// light: Bevy's ambient specular taken out again, then fog (and tonemapping in a view
// without HDR).
//
// TERRAIN_HEIGHT_BLEND (the Terrain height blending switch, terrain.rs "Height blending",
// [design, #2550]): the same textures, read the same way, but the ordered blend becomes a
// weight for the base and each layer (terrain_weights.wesl), competing by share plus
// height within a depth of the highest score. Height compares raw decoded colour to a
// global (coarsest mip) or local (coarser mip at the same UV) reference, so a layer
// comes in through its stones and clumps first. Without it the path below is the one
// there always was, operation for operation.
//
// TERRAIN_ANTI_TILING (the Terrain anti-tiling switch, terrain.rs "Anti-tiling", [design,
// #2551]): every texture (the base's and the layers', diffuse and normal map) is read at a
// few offset and rotated places (Hex3) hashed from where the fragment lies in the world
// and blended (nvf_assets terrain_sampling.wesl), so its repeat does not show as a grid.
// The surface is set up by nvf_assets terrain_pbr.wesl instead of Bevy's
// pbr_input_from_standard_material (which would read the base once more at the plain
// place). The layers are walked last to first and a texture is read only where its share
// can be above 0; with TERRAIN_HEIGHT_BLEND as well the heights come from those same reads.
// Without it both paths above are as they were.
//
// TERRAIN_PARALLAX (the Terrain parallax switch, terrain.rs "Parallax", [design, #3322];
// only for a material that holds a height map, and never with TERRAIN_ANTI_TILING,
// whose taps the march does not share): before the surface is set up, one march over the
// blended depth, each layer's share (ordered shares of the painted opacity) x its own
// set's depth x (1 - its height) on one fixed step grid, the base likewise (#3846), one
// without a height map adding no depth, moves the texture coordinate; then every texture (the
// base's and the layers',
// diffuse and normal map) is read at the one moved coordinate with the plain coordinate's
// gradients (one unturned tap through the anti-tiling helpers, so filtering does not follow
// the shift's jumps), and the layers are blended with the weights the march used: the
// ordered shares of opacity, or with height blending its weights worked out at the plain
// coordinate. Faded out with distance (the material's values), none
// on a surface seen head-on. Only the main pass; without it every path above is as it was.

import bevy_pbr::{
    render::forward_io::{VertexOutput, FragmentOutput},
    render::pbr_fragment::pbr_input_from_standard_material,
    render::pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing, calculate_F0, calculate_tbn_mikktspace, apply_normal_mapping},
    render::pbr_lighting::{EnvBRDFApprox, F_AB},
    render::mesh_view_bindings::{lights, view},
    render::mesh_bindings::mesh,
    render::mesh_functions::get_local_from_world,
    render::pbr_types::StandardMaterial,
};
@if(SURFACE_BAKE) import nvforever::terrain_surface::surface_bake;
@if(VISIBILITY_RANGE_DITHER) import bevy_pbr::render::pbr_functions::visibility_range_dither;
@if(BINDLESS) import bevy_render::bindless::{bindless_textures_2d, bindless_samplers_filtering};
@if(BINDLESS) import bevy_pbr::render::pbr_bindings::{material_indices, material_array};
@if(!BINDLESS) import bevy_pbr::render::pbr_bindings;
@if(TERRAIN_HEIGHT_BLEND) import bevy_pbr::render::pbr_types::STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT;
@if(TERRAIN_HEIGHT_BLEND) import nvforever::terrain_weights::{terrain_height, terrain_ordered_shares, terrain_height_weights};
@if(TERRAIN_PARALLAX && !TERRAIN_HEIGHT_BLEND) import nvforever::terrain_weights::terrain_ordered_shares;
@if(TERRAIN_PARALLAX) import bevy_pbr::render::pbr_functions::calculate_view;
@if(TERRAIN_PARALLAX && !TERRAIN_ANTI_TILING) import nvf_assets::terrain_sampling::{TerrainFootprint, TerrainTap, TerrainTaps, terrain_sample_diffuse, terrain_sample_normal, terrain_world_normal};
@if(TERRAIN_PARALLAX && TERRAIN_ANTI_TILING) import nvf_assets::terrain_sampling::TerrainTap;
@if(TERRAIN_PARALLAX && !TERRAIN_ANTI_TILING) import nvf_assets::terrain_pbr::{terrain_pbr_begin, terrain_pbr_occlusion, terrain_base_diffuse, terrain_base_normal, terrain_two_component, terrain_flip_y, terrain_back_face};
@if(TERRAIN_PARALLAX && !TERRAIN_ANTI_TILING && BINDLESS) import nvf_assets::terrain_pbr::{terrain_bindless_diffuse, terrain_bindless_normal};
@if(TERRAIN_ANTI_TILING) import nvf_assets::terrain_sampling::{TerrainFootprint, TerrainTaps, terrain_sample_diffuse, terrain_sample_normal, terrain_world_normal};
@if(TERRAIN_ANTI_TILING) import nvf_assets::terrain_pbr::{terrain_footprint, terrain_taps, terrain_pbr_begin, terrain_pbr_occlusion, terrain_base_diffuse, terrain_base_normal, terrain_base_textured, terrain_two_component, terrain_flip_y, terrain_back_face};
@if(TERRAIN_ANTI_TILING && BINDLESS) import nvf_assets::terrain_pbr::{terrain_bindless_diffuse, terrain_bindless_normal};

struct TerrainLayers {
    // x: how many layers the material holds (0 to 6); y: bit i set when layer i has a
    // normal map (without one its normal is the surface's own), bit 8 + i when layer i
    // has a height map, and bit 14 when the base has one; z: 1 with the burn map;
    // w: feature bits, bit 0 height blending, bit 1 anti-tiling, bit 2 parallax (the
    // pipeline's TERRAIN_HEIGHT_BLEND, TERRAIN_ANTI_TILING and TERRAIN_PARALLAX decide; this
    // records the material's state).
    counts: vec4<u32>,
    // x: world units per texture repeat when the texture coordinate is worked out here and
    // the mesh's holds the fifth and sixth opacities; 0 when the mesh's is the coordinate.
    // y: world units per texture repeat, always (anti-tiling's coordinate).
    tile: vec4<f32>,
    // Start-up comparison parameters: depth, gain, source (global 0 / local 1), reserved.
    height: vec4<f32>,
    // Parallax: the march's scale (terrain.rs PARALLAX_SCALE, the same for every material
    // with a height map, so every quadrant marches on one step grid; 0 without one), its
    // most steps, the distances its fade starts and ends at (terrain.rs PARALLAX_FADE).
    parallax: vec4<f32>,
    // Each layer's own depth (0 without a height map): layers 0 to 3, then 4 and 5, and the
    // base's in depths_high.z.
    depths_low: vec4<f32>,
    depths_high: vec4<f32>,
}

// The extension's index table (indices 50-70 of TerrainLayers' bindless attribute).
@if(BINDLESS) struct TerrainIndices {
    layers: u32,    // 50
    diffuse0: u32,  // 51
    normal0: u32,   // 52
    diffuse1: u32,  // 53
    normal1: u32,   // 54
    diffuse2: u32,  // 55
    normal2: u32,   // 56
    diffuse3: u32,  // 57
    normal3: u32,   // 58
    diffuse4: u32,  // 59
    normal4: u32,   // 60
    diffuse5: u32,  // 61
    normal5: u32,   // 62
    burn: u32,      // 63
    height0: u32,   // 64
    height1: u32,   // 65
    height2: u32,   // 66
    height3: u32,   // 67
    height4: u32,   // 68
    height5: u32,   // 69
    height_base: u32, // 70
}
@if(BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(100) var<storage> terrain_indices: array<TerrainIndices>;
@if(BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(101) var<storage> terrain_layers: array<TerrainLayers>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(50) var<uniform> terrain_layer_values: TerrainLayers;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(51) var diffuse0: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(52) var normal0: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(53) var diffuse1: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(54) var normal1: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(55) var diffuse2: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(56) var normal2: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(57) var diffuse3: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(58) var normal3: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(59) var diffuse4: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(60) var normal4: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(61) var diffuse5: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(62) var normal5: texture_2d<f32>;
@if(!BINDLESS) @group(constants::MATERIAL_BIND_GROUP) @binding(63) var burn_map: texture_2d<f32>;
@if(!BINDLESS && TERRAIN_PARALLAX) @group(constants::MATERIAL_BIND_GROUP) @binding(64) var height0: texture_2d<f32>;
@if(!BINDLESS && TERRAIN_PARALLAX) @group(constants::MATERIAL_BIND_GROUP) @binding(65) var height1: texture_2d<f32>;
@if(!BINDLESS && TERRAIN_PARALLAX) @group(constants::MATERIAL_BIND_GROUP) @binding(66) var height2: texture_2d<f32>;
@if(!BINDLESS && TERRAIN_PARALLAX) @group(constants::MATERIAL_BIND_GROUP) @binding(67) var height3: texture_2d<f32>;
@if(!BINDLESS && TERRAIN_PARALLAX) @group(constants::MATERIAL_BIND_GROUP) @binding(68) var height4: texture_2d<f32>;
@if(!BINDLESS && TERRAIN_PARALLAX) @group(constants::MATERIAL_BIND_GROUP) @binding(69) var height5: texture_2d<f32>;
@if(!BINDLESS && TERRAIN_PARALLAX) @group(constants::MATERIAL_BIND_GROUP) @binding(70) var height_base: texture_2d<f32>;

// Scorch (nvf_play grass_fire, docs/design/grass-fire.md): the ground under grass that
// burns turns black and stays so until its cell unloads. The burn map holds a byte per
// 64-unit patch, wrapping every 512: above 0 the patch has caught, and by 0.5 it is fully
// charred. Read between the patches' middles (so the mark has no square edges) and
// broken up with noise at its edge [chosen].
const BURN_PATCH: f32 = 64.0;
const BURN_MAP: i32 = 512;
// How dark scorched ground gets (a factor on its colour) [chosen].
const SCORCHED: f32 = 0.16;

fn burn_texel(instance_index: u32, p: vec2<i32>) -> f32 {
    let t = ((p % BURN_MAP) + BURN_MAP) % BURN_MAP;
    @if(BINDLESS) return textureLoad(bindless_textures_2d[terrain_indices[material_slot(instance_index)].burn], t, 0).r;
    @if(!BINDLESS) return textureLoad(burn_map, t, 0).r;
}

// How scorched the ground is at a point (Bevy world space; game (x, y) is Bevy (x, -z)).
fn scorch(instance_index: u32, world: vec3<f32>) -> f32 {
    let game = vec2<f32>(world.x, -world.z);
    let p = game / BURN_PATCH - 0.5;
    let base = vec2<i32>(floor(p));
    let f = fract(p);
    let a = saturate(burn_texel(instance_index, base) * 2.0);
    let b = saturate(burn_texel(instance_index, base + vec2(1, 0)) * 2.0);
    let c = saturate(burn_texel(instance_index, base + vec2(0, 1)) * 2.0);
    let d = saturate(burn_texel(instance_index, base + vec2(1, 1)) * 2.0);
    let s = mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
    if s <= 0.0 {
        return 0.0;
    }
    // Value noise about 20 units across, so the edge is ragged.
    let q = game / 20.0;
    let i = floor(q);
    let g = fract(q);
    let h = g * g * (3.0 - 2.0 * g);
    let n = mix(mix(hash2(i), hash2(i + vec2(1.0, 0.0)), h.x), mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), h.x), h.y);
    return smoothstep(0.2, 0.7, s + (n - 0.5) * 0.45);
}

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// The material's slot in the bindless tables (unused without bindless).
fn material_slot(instance_index: u32) -> u32 {
    return mesh[instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
}

fn layer_counts(instance_index: u32) -> vec4<u32> {
    @if(BINDLESS) return terrain_layers[terrain_indices[material_slot(instance_index)].layers].counts;
    @if(!BINDLESS) return terrain_layer_values.counts;
}

fn layer_tile(instance_index: u32) -> f32 {
    @if(BINDLESS) return terrain_layers[terrain_indices[material_slot(instance_index)].layers].tile.x;
    @if(!BINDLESS) return terrain_layer_values.tile.x;
}

// The texture repeat in world units, whether or not the mesh carries the coordinate.
@if(TERRAIN_ANTI_TILING) fn layer_repeat(instance_index: u32) -> f32 {
    @if(BINDLESS) return terrain_layers[terrain_indices[material_slot(instance_index)].layers].tile.y;
    @if(!BINDLESS) return terrain_layer_values.tile.y;
}

fn standard_material(instance_index: u32) -> StandardMaterial {
    @if(BINDLESS) return material_array[material_indices[material_slot(instance_index)].material];
    @if(!BINDLESS) return pbr_bindings::material;
}

// Layer `k`'s diffuse texture at `uv`, read with the base texture's sampler.
fn layer_diffuse(k: u32, instance_index: u32, uv: vec2<f32>) -> vec4<f32> {
    @if(BINDLESS) {
        let slot = material_slot(instance_index);
        let i = terrain_indices[slot];
        var t = i.diffuse0;
        if k == 1u { t = i.diffuse1; }
        if k == 2u { t = i.diffuse2; }
        if k == 3u { t = i.diffuse3; }
        if k == 4u { t = i.diffuse4; }
        if k == 5u { t = i.diffuse5; }
        return textureSampleBias(bindless_textures_2d[t], bindless_samplers_filtering[material_indices[slot].base_color_sampler], uv, view.mip_bias);
    }
    @if(!BINDLESS) {
        if k == 1u { return textureSampleBias(diffuse1, pbr_bindings::base_color_sampler, uv, view.mip_bias); }
        if k == 2u { return textureSampleBias(diffuse2, pbr_bindings::base_color_sampler, uv, view.mip_bias); }
        if k == 3u { return textureSampleBias(diffuse3, pbr_bindings::base_color_sampler, uv, view.mip_bias); }
        if k == 4u { return textureSampleBias(diffuse4, pbr_bindings::base_color_sampler, uv, view.mip_bias); }
        if k == 5u { return textureSampleBias(diffuse5, pbr_bindings::base_color_sampler, uv, view.mip_bias); }
        return textureSampleBias(diffuse0, pbr_bindings::base_color_sampler, uv, view.mip_bias);
    }
}

// Layer `k`'s normal map at `uv`.
fn layer_normal(k: u32, instance_index: u32, uv: vec2<f32>) -> vec3<f32> {
    @if(BINDLESS) {
        let slot = material_slot(instance_index);
        let i = terrain_indices[slot];
        var t = i.normal0;
        if k == 1u { t = i.normal1; }
        if k == 2u { t = i.normal2; }
        if k == 3u { t = i.normal3; }
        if k == 4u { t = i.normal4; }
        if k == 5u { t = i.normal5; }
        return textureSampleBias(bindless_textures_2d[t], bindless_samplers_filtering[material_indices[slot].base_color_sampler], uv, view.mip_bias).rgb;
    }
    @if(!BINDLESS) {
        if k == 1u { return textureSampleBias(normal1, pbr_bindings::base_color_sampler, uv, view.mip_bias).rgb; }
        if k == 2u { return textureSampleBias(normal2, pbr_bindings::base_color_sampler, uv, view.mip_bias).rgb; }
        if k == 3u { return textureSampleBias(normal3, pbr_bindings::base_color_sampler, uv, view.mip_bias).rgb; }
        if k == 4u { return textureSampleBias(normal4, pbr_bindings::base_color_sampler, uv, view.mip_bias).rgb; }
        if k == 5u { return textureSampleBias(normal5, pbr_bindings::base_color_sampler, uv, view.mip_bias).rgb; }
        return textureSampleBias(normal0, pbr_bindings::base_color_sampler, uv, view.mip_bias).rgb;
    }
}

// Layer `k`'s diffuse texture at `uv` with explicit gradients (`dx`, `dy`: the coordinate's
// screen derivatives scaled by the view's mip bias, [`layer_grads`]): the same mip level as
// `layer_diffuse`, and a read that may sit in a branch some fragments of a quad skip (#2868).
fn layer_diffuse_grad(k: u32, instance_index: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    @if(BINDLESS) {
        let slot = material_slot(instance_index);
        let i = terrain_indices[slot];
        var t = i.diffuse0;
        if k == 1u { t = i.diffuse1; }
        if k == 2u { t = i.diffuse2; }
        if k == 3u { t = i.diffuse3; }
        if k == 4u { t = i.diffuse4; }
        if k == 5u { t = i.diffuse5; }
        return textureSampleGrad(bindless_textures_2d[t], bindless_samplers_filtering[material_indices[slot].base_color_sampler], uv, dx, dy);
    }
    @if(!BINDLESS) {
        if k == 1u { return textureSampleGrad(diffuse1, pbr_bindings::base_color_sampler, uv, dx, dy); }
        if k == 2u { return textureSampleGrad(diffuse2, pbr_bindings::base_color_sampler, uv, dx, dy); }
        if k == 3u { return textureSampleGrad(diffuse3, pbr_bindings::base_color_sampler, uv, dx, dy); }
        if k == 4u { return textureSampleGrad(diffuse4, pbr_bindings::base_color_sampler, uv, dx, dy); }
        if k == 5u { return textureSampleGrad(diffuse5, pbr_bindings::base_color_sampler, uv, dx, dy); }
        return textureSampleGrad(diffuse0, pbr_bindings::base_color_sampler, uv, dx, dy);
    }
}

// Layer `k`'s normal map at `uv` with explicit gradients, as [`layer_diffuse_grad`].
fn layer_normal_grad(k: u32, instance_index: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec3<f32> {
    @if(BINDLESS) {
        let slot = material_slot(instance_index);
        let i = terrain_indices[slot];
        var t = i.normal0;
        if k == 1u { t = i.normal1; }
        if k == 2u { t = i.normal2; }
        if k == 3u { t = i.normal3; }
        if k == 4u { t = i.normal4; }
        if k == 5u { t = i.normal5; }
        return textureSampleGrad(bindless_textures_2d[t], bindless_samplers_filtering[material_indices[slot].base_color_sampler], uv, dx, dy).rgb;
    }
    @if(!BINDLESS) {
        if k == 1u { return textureSampleGrad(normal1, pbr_bindings::base_color_sampler, uv, dx, dy).rgb; }
        if k == 2u { return textureSampleGrad(normal2, pbr_bindings::base_color_sampler, uv, dx, dy).rgb; }
        if k == 3u { return textureSampleGrad(normal3, pbr_bindings::base_color_sampler, uv, dx, dy).rgb; }
        if k == 4u { return textureSampleGrad(normal4, pbr_bindings::base_color_sampler, uv, dx, dy).rgb; }
        if k == 5u { return textureSampleGrad(normal5, pbr_bindings::base_color_sampler, uv, dx, dy).rgb; }
        return textureSampleGrad(normal0, pbr_bindings::base_color_sampler, uv, dx, dy).rgb;
    }
}

// Height blending's reads beside the ordinary ones. The base's diffuse once more, as
// pbr_input_from_standard_material reads it (same texture, sampler, coordinate and bias:
// texels already in cache), for its colour before the tint, which the height is taken from.
@if(TERRAIN_HEIGHT_BLEND) fn base_diffuse(instance_index: u32, uv: vec2<f32>) -> vec4<f32> {
    @if(BINDLESS) {
        let slot = material_slot(instance_index);
        return textureSampleBias(bindless_textures_2d[material_indices[slot].base_color_texture], bindless_samplers_filtering[material_indices[slot].base_color_sampler], uv, view.mip_bias);
    }
    @if(!BINDLESS) {
        return textureSampleBias(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler, uv, view.mip_bias);
    }
}

// Only the on variant reads the appended parameters [design, #2550].
@if(TERRAIN_HEIGHT_BLEND) fn height_params(instance_index: u32) -> vec4<f32> {
    @if(BINDLESS) return terrain_layers[terrain_indices[material_slot(instance_index)].layers].height;
    @if(!BINDLESS) return terrain_layer_values.height;
}

// Global: the existing coarsest-mip reference. Local: a coarse spatial reference at
// the same UV, two levels beyond the estimated current LOD, never finer than mip 4.
// Gradients are captured before any divergent control flow in fragment(), and scaled
// by each texture's own dimensions here; explicit-level reads are safe in share branches.
// Where and at which level the reference is read: plain values only, so the bindless
// callers index their texture array themselves (a texture taken out of a binding array
// is never passed to a function: the index's non-uniformity is only known where the
// array is indexed, and drivers have crashed on such arguments) [design, #2550].
@if(TERRAIN_HEIGHT_BLEND) fn height_reference_at(levels: u32, size: vec2<u32>, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, source: f32) -> vec3<f32> {
    let last_mip = f32(levels - 1u);
    var lod = last_mip;
    var coord = vec2(0.5);
    if source > 0.5 {
        let fsize = vec2<f32>(size);
        let gx = dx * fsize;
        let gy = dy * fsize;
        let current_lod = 0.5 * log2(max(max(dot(gx, gx), dot(gy, gy)), 1e-12)) + view.mip_bias;
        lod = min(last_mip, max(4.0, current_lod + 2.0));
        coord = uv;
    }
    return vec3(coord, lod);
}

// The non-bindless reference read (plain texture bindings may be passed).
@if(TERRAIN_HEIGHT_BLEND) fn height_reference(t: texture_2d<f32>, s: sampler, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, source: f32) -> vec3<f32> {
    let at = height_reference_at(textureNumLevels(t), textureDimensions(t, 0), uv, dx, dy, source);
    return textureSampleLevel(t, s, at.xy, at.z).rgb;
}

@if(TERRAIN_HEIGHT_BLEND) fn base_reference(instance_index: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, source: f32) -> vec3<f32> {
    @if(BINDLESS) {
        let slot = material_slot(instance_index);
        let t = material_indices[slot].base_color_texture;
        let at = height_reference_at(textureNumLevels(bindless_textures_2d[t]), textureDimensions(bindless_textures_2d[t], 0), uv, dx, dy, source);
        return textureSampleLevel(bindless_textures_2d[t], bindless_samplers_filtering[material_indices[slot].base_color_sampler], at.xy, at.z).rgb;
    }
    @if(!BINDLESS) {
        return height_reference(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler, uv, dx, dy, source);
    }
}

@if(TERRAIN_HEIGHT_BLEND) fn layer_reference(k: u32, instance_index: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, source: f32) -> vec3<f32> {
    @if(BINDLESS) {
        let slot = material_slot(instance_index);
        let i = terrain_indices[slot];
        var t = i.diffuse0;
        if k == 1u { t = i.diffuse1; }
        if k == 2u { t = i.diffuse2; }
        if k == 3u { t = i.diffuse3; }
        if k == 4u { t = i.diffuse4; }
        if k == 5u { t = i.diffuse5; }
        let at = height_reference_at(textureNumLevels(bindless_textures_2d[t]), textureDimensions(bindless_textures_2d[t], 0), uv, dx, dy, source);
        return textureSampleLevel(bindless_textures_2d[t], bindless_samplers_filtering[material_indices[slot].base_color_sampler], at.xy, at.z).rgb;
    }
    @if(!BINDLESS) {
        if k == 1u { return height_reference(diffuse1, pbr_bindings::base_color_sampler, uv, dx, dy, source); }
        if k == 2u { return height_reference(diffuse2, pbr_bindings::base_color_sampler, uv, dx, dy, source); }
        if k == 3u { return height_reference(diffuse3, pbr_bindings::base_color_sampler, uv, dx, dy, source); }
        if k == 4u { return height_reference(diffuse4, pbr_bindings::base_color_sampler, uv, dx, dy, source); }
        if k == 5u { return height_reference(diffuse5, pbr_bindings::base_color_sampler, uv, dx, dy, source); }
        return height_reference(diffuse0, pbr_bindings::base_color_sampler, uv, dx, dy, source);
    }
}

// Parallax's reads and march [design, #3322]. Explicit levels only (the march loops and
// branches per fragment): the level from the footprint taken at the top of fragment(),
// log2 of texture-coordinate units per pixel, and each texture's own size.
@if(TERRAIN_PARALLAX) fn parallax_values(instance_index: u32) -> vec4<f32> {
    @if(BINDLESS) return terrain_layers[terrain_indices[material_slot(instance_index)].layers].parallax;
    @if(!BINDLESS) return terrain_layer_values.parallax;
}

// Each layer's depth over the march's scale, and the base's at index 6 (0 without a height
// map), so a
// layer moves the coordinate as far as its own set's depth asks; with the scale the same
// everywhere, the same layer is marched on the same steps on both sides of a quadrant edge.
@if(TERRAIN_PARALLAX) fn parallax_scales(instance_index: u32, depth: f32) -> array<f32, 7> {
    @if(BINDLESS) let l = terrain_layers[terrain_indices[material_slot(instance_index)].layers];
    @if(!BINDLESS) let l = terrain_layer_values;
    let s = 1.0 / max(depth, 1e-6);
    return array<f32, 7>(l.depths_low.x * s, l.depths_low.y * s, l.depths_low.z * s, l.depths_low.w * s, l.depths_high.x * s, l.depths_high.y * s, l.depths_high.z * s);
}

@if(TERRAIN_PARALLAX) fn parallax_level(size: vec2<u32>, footprint: f32) -> f32 {
    return max(0.0, footprint + log2(f32(max(max(size.x, size.y), 1u))));
}

// The non-bindless read (plain texture bindings may be passed).
@if(TERRAIN_PARALLAX) fn parallax_read(t: texture_2d<f32>, s: sampler, uv: vec2<f32>, footprint: f32) -> vec4<f32> {
    return textureSampleLevel(t, s, uv, parallax_level(textureDimensions(t, 0), footprint));
}

// Layer `k`'s height map (the base's for 6) at `uv`, read with the base texture's sampler. The caller reads
// only those its bits name.
@if(TERRAIN_PARALLAX) fn layer_height(k: u32, instance_index: u32, uv: vec2<f32>, footprint: f32) -> f32 {
    @if(BINDLESS) {
        let slot = material_slot(instance_index);
        let i = terrain_indices[slot];
        var h = i.height0;
        if k == 1u { h = i.height1; }
        if k == 2u { h = i.height2; }
        if k == 3u { h = i.height3; }
        if k == 4u { h = i.height4; }
        if k == 5u { h = i.height5; }
        if k == 6u { h = i.height_base; }
        let level = parallax_level(textureDimensions(bindless_textures_2d[h], 0), footprint);
        return textureSampleLevel(bindless_textures_2d[h], bindless_samplers_filtering[material_indices[slot].base_color_sampler], uv, level).r;
    }
    @if(!BINDLESS) {
        if k == 1u { return parallax_read(height1, pbr_bindings::base_color_sampler, uv, footprint).r; }
        if k == 2u { return parallax_read(height2, pbr_bindings::base_color_sampler, uv, footprint).r; }
        if k == 3u { return parallax_read(height3, pbr_bindings::base_color_sampler, uv, footprint).r; }
        if k == 4u { return parallax_read(height4, pbr_bindings::base_color_sampler, uv, footprint).r; }
        if k == 5u { return parallax_read(height5, pbr_bindings::base_color_sampler, uv, footprint).r; }
        if k == 6u { return parallax_read(height_base, pbr_bindings::base_color_sampler, uv, footprint).r; }
        return parallax_read(height0, pbr_bindings::base_color_sampler, uv, footprint).r;
    }
}

// The march's depth of the blended surface at `uv`, on the march's scale: each of the `n`
// layers' share (index k + 1) and the base's (index 0, its scale at 6, #3846) x its scale x
// (1 - its height map's height). One without a height map (scale 0) adds none: flat at the
// surface, so the shift falls to nothing as those with height maps fade out, with no jump at
// their edges (terrain.rs "Parallax").
@if(TERRAIN_PARALLAX) fn parallax_depth_at(instance_index: u32, uv: vec2<f32>, w: array<f32, 7>, n: u32, scale: array<f32, 7>, footprint: f32) -> f32 {
    var depth = 0.0;
    if w[0] > 0.0 && scale[6] > 0.0 {
        depth += w[0] * scale[6] * (1.0 - layer_height(6u, instance_index, uv, footprint));
    }
    for (var k = 0u; k < n; k += 1u) {
        if w[k + 1u] > 0.0 && scale[k] > 0.0 {
            depth += w[k + 1u] * scale[k] * (1.0 - layer_height(k, instance_index, uv, footprint));
        }
    }
    return depth;
}

// The march's grazing clamp and relief search (terrain.rs PARALLAX_GRAZING,
// PARALLAX_RELIEF_STEPS, whose tests read these lines): the NIF shapes' march's values
// (vendored parallax_mapping.wesl "NVForever grazing clamp", nvf-assets authored.rs
// PARALLAX_METHOD relief:4) [design, #3322 round 2].
const PARALLAX_GRAZING: f32 = 0.15;
const PARALLAX_RELIEF_STEPS: u32 = 4u;

// How far the texture coordinate `uv` moves: steep parallax (a linear search through
// layers) with the relief refinement, a binary search between the last two layers
// (Policarpo, Oliveira and Comba, "Real-Time Relief Mapping on Arbitrary Polygonal
// Surfaces", I3D 2005; Bevy's relief method in the vendored parallax_mapping.wesl
// `parallaxed_uv`, as on NIF shapes: the same view vector, frame and signs, and its grazing
// clamp), over the blended depth. The occlusion refinement first written here (one linear
// interpolation between the last two layers, Tatarchuk's parallax occlusion mapping, I3D
// 2006) placed a sharp height map's ridge between two layers' guesses, once per layer,
// instead of at its wall: the stair-stepped, doubled bands of the round-1 pictures. None where the march has
// nothing to move (no depth, faded out, nothing with both a share and a height map, the
// plain place already at the surface) and none head-on (the view vector's tangent part is
// 0). `w`: the base's and the layers' weights worked out at the plain coordinate
// (fragment()), which the drawn blend uses too. The ray is the view's own (Bevy's
// `calculate_view`: one direction in an orthographic view, as the minimap's); the fade goes
// by the distance from the camera.
@if(TERRAIN_PARALLAX) fn parallax_shift(instance_index: u32, uv: vec2<f32>, world: vec4<f32>, tbn: mat3x3<f32>, w: array<f32, 7>, n: u32, footprint: f32) -> vec2<f32> {
    let values = parallax_values(instance_index);
    let fade = 1.0 - smoothstep(values.z, values.w, length(view.world_position.xyz - world.xyz));
    if values.x <= 0.0 || fade <= 0.0 {
        return vec2(0.0);
    }
    let scale = parallax_scales(instance_index, values.x);
    var relief = w[0] > 0.0 && scale[6] > 0.0;
    for (var k = 0u; k < n; k += 1u) {
        if w[k + 1u] > 0.0 && scale[k] > 0.0 {
            relief = true;
        }
    }
    if !relief {
        return vec2(0.0);
    }
    let V = calculate_view(world, view.clip_from_view[3].w == 1.0);
    // Towards the surface, in the tangent frame.
    let Vt = -vec3(dot(V, tbn[0]), dot(V, tbn[1]), dot(V, tbn[2]));
    let steepness = abs(Vt.z);
    // More layers the shallower the view (the layer count mixes by the unclamped steepness);
    // the step's divisor stops at the grazing clamp, so a view along the ground neither
    // throws the samples several texture repeats away nor gives non-finite coordinates (the
    // full-depth shift stays under 1 / 0.15, about 6.7 x the depth).
    let layer_count = mix(values.y, 1.0, steepness);
    let layer_depth = 1.0 / layer_count;
    var delta = values.x * fade * layer_depth * Vt.xy * vec2(1.0, -1.0) / max(steepness, PARALLAX_GRAZING);
    var shift = vec2(0.0);
    var current = 0.0;
    var depth = parallax_depth_at(instance_index, uv, w, n, scale, footprint);
    if depth <= 0.0 {
        return vec2(0.0);
    }
    for (var i = 0; depth > current && i <= i32(layer_count); i += 1) {
        current += layer_depth;
        shift += delta;
        depth = parallax_depth_at(instance_index, uv + shift, w, n, scale, footprint);
    }
    // Relief: halve the last layer's step PARALLAX_RELIEF_STEPS times, moving forward while
    // the ray is still above the surface and back once below it, so the coordinate ends
    // within a thirty-second of a step of where the ray meets the surface (a ridge's wall
    // or its top), not between two layers' guesses.
    delta *= 0.5;
    var step_depth = 0.5 * layer_depth;
    shift -= delta;
    current -= step_depth;
    for (var i = 0u; i < PARALLAX_RELIEF_STEPS; i += 1u) {
        depth = parallax_depth_at(instance_index, uv + shift, w, n, scale, footprint);
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

// One unturned read at `uv` through the anti-tiling helpers (one tap of weight 1): with a
// footprint of the plain coordinate's gradients, a plain read at a moved place whose
// filtering does not follow the shift's jumps between neighbouring pixels [design, #3322
// review].
@if(TERRAIN_PARALLAX) fn parallax_tap(uv: vec2<f32>) -> TerrainTaps {
    let tap = TerrainTap(uv, mat2x2<f32>(1.0, 0.0, 0.0, 1.0), 1.0);
    return TerrainTaps(array<TerrainTap, 3>(tap, tap, tap), 1u);
}

// Anti-tiling's reads of layer `k`: its diffuse texture with the base texture's sampler
// (as `layer_diffuse`), its normal map decoded by the base material's flags (as the
// ordinary path's apply_normal_mapping does), at every kept tap (terrain_sampling.wesl).
@if(TERRAIN_ANTI_TILING || TERRAIN_PARALLAX) fn layer_diffuse_antitiled(k: u32, instance_index: u32, taps: TerrainTaps, fp: TerrainFootprint) -> vec4<f32> {
    @if(BINDLESS) {
        let slot = material_slot(instance_index);
        let i = terrain_indices[slot];
        var t = i.diffuse0;
        if k == 1u { t = i.diffuse1; }
        if k == 2u { t = i.diffuse2; }
        if k == 3u { t = i.diffuse3; }
        if k == 4u { t = i.diffuse4; }
        if k == 5u { t = i.diffuse5; }
        return terrain_bindless_diffuse(t, material_indices[slot].base_color_sampler, taps, fp);
    }
    @if(!BINDLESS) {
        if k == 1u { return terrain_sample_diffuse(diffuse1, pbr_bindings::base_color_sampler, taps, fp); }
        if k == 2u { return terrain_sample_diffuse(diffuse2, pbr_bindings::base_color_sampler, taps, fp); }
        if k == 3u { return terrain_sample_diffuse(diffuse3, pbr_bindings::base_color_sampler, taps, fp); }
        if k == 4u { return terrain_sample_diffuse(diffuse4, pbr_bindings::base_color_sampler, taps, fp); }
        if k == 5u { return terrain_sample_diffuse(diffuse5, pbr_bindings::base_color_sampler, taps, fp); }
        return terrain_sample_diffuse(diffuse0, pbr_bindings::base_color_sampler, taps, fp);
    }
}

@if(TERRAIN_ANTI_TILING || TERRAIN_PARALLAX) fn layer_normal_antitiled(k: u32, instance_index: u32, taps: TerrainTaps, fp: TerrainFootprint, two_component: bool, flip_y: bool) -> vec3<f32> {
    @if(BINDLESS) {
        let slot = material_slot(instance_index);
        let i = terrain_indices[slot];
        var t = i.normal0;
        if k == 1u { t = i.normal1; }
        if k == 2u { t = i.normal2; }
        if k == 3u { t = i.normal3; }
        if k == 4u { t = i.normal4; }
        if k == 5u { t = i.normal5; }
        return terrain_bindless_normal(t, material_indices[slot].base_color_sampler, taps, fp, two_component, flip_y);
    }
    @if(!BINDLESS) {
        if k == 1u { return terrain_sample_normal(normal1, pbr_bindings::base_color_sampler, taps, fp, two_component, flip_y); }
        if k == 2u { return terrain_sample_normal(normal2, pbr_bindings::base_color_sampler, taps, fp, two_component, flip_y); }
        if k == 3u { return terrain_sample_normal(normal3, pbr_bindings::base_color_sampler, taps, fp, two_component, flip_y); }
        if k == 4u { return terrain_sample_normal(normal4, pbr_bindings::base_color_sampler, taps, fp, two_component, flip_y); }
        if k == 5u { return terrain_sample_normal(normal5, pbr_bindings::base_color_sampler, taps, fp, two_component, flip_y); }
        return terrain_sample_normal(normal0, pbr_bindings::base_color_sampler, taps, fp, two_component, flip_y);
    }
}

@fragment
fn fragment(
    vertex: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var in = vertex;
    // Anti-tiling's footprint first, while every fragment of the quad runs (screen
    // derivatives; nothing has been discarded or branched on yet).
    @if(TERRAIN_ANTI_TILING) let fp = terrain_footprint(in.world_position, layer_repeat(in.instance_index), standard_material(in.instance_index).uv_transform);
    // Parallax's footprint, likewise before any branch: log2 of texture-coordinate units per
    // pixel at the plain coordinate (the one worked out below), with the view's mip bias.
    // Its gradients, scaled by the mip bias as anti-tiling's (a gradient read takes no bias of
    // its own), are every later read's.
    @if(TERRAIN_PARALLAX) var parallax_footprint = 0.0;
    @if(TERRAIN_PARALLAX) var parallax_ddx = vec2(0.0);
    @if(TERRAIN_PARALLAX) var parallax_ddy = vec2(0.0);
    @if(TERRAIN_PARALLAX) {
        @if(VERTEX_UVS_A) {
            let parallax_tile = layer_tile(in.instance_index);
            let parallax_local_uv = (get_local_from_world(in.instance_index) * in.world_position).xy / select(1.0, parallax_tile, parallax_tile > 0.0);
            let parallax_raw_uv = select(in.uv, parallax_local_uv, parallax_tile > 0.0);
            let parallax_dx = dpdx(parallax_raw_uv);
            let parallax_dy = dpdy(parallax_raw_uv);
            parallax_footprint = 0.5 * log2(max(max(dot(parallax_dx, parallax_dx), dot(parallax_dy, parallax_dy)), 1e-12)) + view.mip_bias;
            parallax_ddx = parallax_dx * exp2(view.mip_bias);
            parallax_ddy = parallax_dy * exp2(view.mip_bias);
        }
    }
    @if(TERRAIN_HEIGHT_BLEND && !TERRAIN_ANTI_TILING) var height_dx = vec2(0.0);
    @if(TERRAIN_HEIGHT_BLEND && !TERRAIN_ANTI_TILING) var height_dy = vec2(0.0);
    @if(TERRAIN_HEIGHT_BLEND && !TERRAIN_ANTI_TILING) {
        @if(VERTEX_UVS_A) {
            // Before dither/standard-material discard or any per-fragment branch. Recreate
            // the same UV used below without a branch on the material's tile value.
            // Names of their own: an `@if` block's braces do not make a scope once the WESL
            // conditions are resolved, so `tile` and `uv` below would be defined twice.
            let height_tile = layer_tile(in.instance_index);
            let height_local_uv = (get_local_from_world(in.instance_index) * in.world_position).xy / select(1.0, height_tile, height_tile > 0.0);
            let height_raw_uv = select(in.uv, height_local_uv, height_tile > 0.0);
            let height_uv = (standard_material(in.instance_index).uv_transform * vec3(height_raw_uv, 1.0)).xy;
            height_dx = dpdx(height_uv);
            height_dy = dpdy(height_uv);
        }
    }
    @if(VISIBILITY_RANGE_DITHER) visibility_range_dither(in.position, in.visibility_range_dither);
    // The layers' opacities, taken out of the attributes that carried them, which then
    // hold what the standard material expects: an opaque vertex colour and a tangent of
    // handedness +-1.
    var opacity = array<f32, 6>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    var tint = vec3(1.0);
    @if(VERTEX_COLORS) opacity[0] = in.color.a;
    @if(VERTEX_COLORS) tint = in.color.rgb;
    @if(VERTEX_COLORS) in.color = vec4(in.color.rgb, 1.0);
    @if(VERTEX_UVS_B) opacity[1] = in.uv_b.x;
    @if(VERTEX_UVS_B) opacity[2] = in.uv_b.y;
    @if(VERTEX_TANGENTS) opacity[3] = abs(in.world_tangent.w) - 1.0;
    @if(VERTEX_TANGENTS) in.world_tangent = vec4(in.world_tangent.xyz, sign(in.world_tangent.w));
    // The texture coordinate worked out from the fragment's place in its mesh.
    @if(VERTEX_UVS_A) let tile = layer_tile(in.instance_index);
    @if(VERTEX_UVS_A) if tile > 0.0 {
        opacity[4] = in.uv.x;
        opacity[5] = in.uv.y;
        in.uv = (get_local_from_world(in.instance_index) * in.world_position).xy / tile;
    }
    // Parallax: the coordinate moved before the surface is set up, so the base's read and
    // every later read take the moved place (the landscape's texture transform is the
    // identity, terrain.rs tests). Never with anti-tiling (terrain.rs
    // `TerrainLayersKey`), whose reads take their place from the world, not from `in.uv`.
    // The weights are worked out once at the plain place (the painted opacities; with height
    // blending its weights from the texels there, read with the drawn reads' own gradients),
    // and both the march and the drawn blend use them: a layer the height blend leaves out
    // does not move what is drawn, and with no shift the blend is the one without parallax
    // [design, #3322 review].
    @if(TERRAIN_PARALLAX) var parallax_w = array<f32, 7>(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    @if(TERRAIN_PARALLAX && VERTEX_UVS_A && VERTEX_TANGENTS) {
        let parallax_n = min(layer_counts(in.instance_index).x, 6u);
        var parallax_paint = array<f32, 6>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for (var k = 0u; k < parallax_n; k += 1u) {
            parallax_paint[k] = saturate(opacity[k]);
        }
        parallax_w = terrain_ordered_shares(parallax_paint, parallax_n);
        @if(TERRAIN_HEIGHT_BLEND && !TERRAIN_ANTI_TILING) {
            // Height blending's weights as its drawn path works them out (the same heights and
            // references), from the plain place's texels.
            let parallax_standard = standard_material(in.instance_index);
            let parallax_plain_uv = (parallax_standard.uv_transform * vec3(in.uv, 1.0)).xy;
            let parallax_plain_fp = TerrainFootprint(parallax_plain_uv, parallax_ddx, parallax_ddy);
            let parallax_plain_taps = parallax_tap(parallax_plain_uv);
            let parallax_params = height_params(in.instance_index);
            var parallax_height = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            if parallax_w[0] > 0.0 && (parallax_standard.flags & STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
                let parallax_base = terrain_base_diffuse(in.instance_index, parallax_standard.flags, parallax_plain_taps, parallax_plain_fp);
                parallax_height[0] = terrain_height(parallax_base.rgb, base_reference(in.instance_index, parallax_plain_uv, height_dx, height_dy, parallax_params.z), parallax_params.y);
            }
            for (var k = 0u; k < parallax_n; k += 1u) {
                if parallax_w[k + 1u] > 0.0 {
                    let parallax_texel = layer_diffuse_antitiled(k, in.instance_index, parallax_plain_taps, parallax_plain_fp);
                    parallax_height[k + 1u] = terrain_height(parallax_texel.rgb, layer_reference(k, in.instance_index, parallax_plain_uv, height_dx, height_dy, parallax_params.z), parallax_params.y);
                }
            }
            parallax_w = terrain_height_weights(parallax_w, parallax_height, parallax_params.x);
        }
        in.uv += parallax_shift(in.instance_index, in.uv, in.world_position, calculate_tbn_mikktspace(in.world_normal, in.world_tangent), parallax_w, parallax_n, parallax_footprint);
    }

    @if(!TERRAIN_ANTI_TILING && !TERRAIN_PARALLAX) var pbr_input = pbr_input_from_standard_material(in, is_front);
    @if(TERRAIN_ANTI_TILING) var pbr_input = terrain_pbr_begin(in, is_front, standard_material(in.instance_index));
    // Parallax: the surface set up as anti-tiling sets it up (terrain_pbr.wesl: no read at the
    // plain place), the base's diffuse and normal map read at the moved place with the plain
    // place's gradients, the occlusion from the base's colour (below).
    @if(TERRAIN_PARALLAX && !TERRAIN_ANTI_TILING) var pbr_input = terrain_pbr_begin(in, is_front, standard_material(in.instance_index));
    @if(TERRAIN_PARALLAX && !TERRAIN_ANTI_TILING) let parallax_n_pre = pbr_input.N;
    @if(TERRAIN_PARALLAX && VERTEX_UVS_A) let parallax_fp = TerrainFootprint((standard_material(in.instance_index).uv_transform * vec3(in.uv, 1.0)).xy, parallax_ddx, parallax_ddy);
    @if(TERRAIN_PARALLAX && VERTEX_UVS_A) let parallax_taps = parallax_tap(parallax_fp.uv);
    @if(TERRAIN_PARALLAX && !TERRAIN_ANTI_TILING && VERTEX_UVS_A) {
        let parallax_flags = standard_material(in.instance_index).flags;
        pbr_input.material.base_color = pbr_input.material.base_color * terrain_base_diffuse(in.instance_index, parallax_flags, parallax_taps, parallax_fp);
        @if(VERTEX_TANGENTS && STANDARD_MATERIAL_NORMAL_MAP) {
            pbr_input.N = terrain_world_normal(calculate_tbn_mikktspace(in.world_normal, in.world_tangent), terrain_base_normal(in.instance_index, parallax_flags, parallax_taps, parallax_fp), terrain_back_face(pbr_input, is_front));
        }
    }
    // The occlusion from the base's colour and the normal before any map, as Bevy's set-up
    // takes it on the path without parallax (before the layers are folded in), so a pixel
    // with no shift is shaded the same [design, #3322 review].
    @if(TERRAIN_PARALLAX && !TERRAIN_ANTI_TILING) pbr_input = terrain_pbr_occlusion(pbr_input, parallax_n_pre);
    let counts = layer_counts(in.instance_index);
    @if(VERTEX_UVS_A) {
        let standard = standard_material(in.instance_index);
        let uv = (standard.uv_transform * vec3(in.uv, 1.0)).xy;
        var colour = pbr_input.material.base_color.rgb;
        var normal = pbr_input.N;
        let plain = normalize(pbr_input.world_normal);
        @if(VERTEX_TANGENTS) let tbn = calculate_tbn_mikktspace(in.world_normal, in.world_tangent);
        @if(!TERRAIN_HEIGHT_BLEND && !TERRAIN_ANTI_TILING) {
            // The coordinate's screen derivatives, taken once before the loop (#2868), with the
            // view's mip bias folded in as a scale (a gradient read takes no bias of its own):
            // the mip level textureSampleBias(.., view.mip_bias) picks.
            @if(!TERRAIN_PARALLAX) let bias_scale = exp2(view.mip_bias);
            @if(!TERRAIN_PARALLAX) let uv_dx = dpdx(uv) * bias_scale;
            @if(!TERRAIN_PARALLAX) let uv_dy = dpdy(uv) * bias_scale;
            for (var k = 0u; k < min(counts.x, 6u); k += 1u) {
                @if(!TERRAIN_PARALLAX) {
                    // A layer of opacity 0 here has a share of 0, which leaves the colour and
                    // the normal as they are: its two reads are skipped (#2868: on an integrated
                    // GPU they were most of the ground's cost, every held layer on every pixel).
                    let o = saturate(opacity[k]);
                    if o > 0.0 {
                        let texel = layer_diffuse_grad(k, in.instance_index, uv, uv_dx, uv_dy);
                        let share = o;
                        var layer_n = plain;
                        @if(VERTEX_TANGENTS) if (counts.y & (1u << k)) != 0u {
                            layer_n = apply_normal_mapping(standard.flags, tbn, false, is_front, layer_normal_grad(k, in.instance_index, uv, uv_dx, uv_dy));
                        }
                        colour = mix(colour, texel.rgb * tint * standard.base_color.rgb, share);
                        normal = mix(normal, layer_n, share);
                    }
                }
                @if(TERRAIN_PARALLAX) let texel = layer_diffuse_antitiled(k, in.instance_index, parallax_taps, parallax_fp);
                @if(TERRAIN_PARALLAX) let share = saturate(opacity[k]);
                @if(TERRAIN_PARALLAX) var layer_n = plain;
                @if(TERRAIN_PARALLAX && VERTEX_TANGENTS) if (counts.y & (1u << k)) != 0u {
                    layer_n = terrain_world_normal(tbn, layer_normal_antitiled(k, in.instance_index, parallax_taps, parallax_fp, terrain_two_component(standard.flags), terrain_flip_y(standard.flags)), false);
                }
                @if(TERRAIN_PARALLAX) colour = mix(colour, texel.rgb * tint * standard.base_color.rgb, share);
                @if(TERRAIN_PARALLAX) normal = mix(normal, layer_n, share);
            }
            pbr_input.material.base_color = vec4(colour, 1.0);
            pbr_input.N = normalize(normal);
        }
        @if(TERRAIN_HEIGHT_BLEND && !TERRAIN_ANTI_TILING) {
            let params = height_params(in.instance_index);
            // The same reads as above, in the same places (every held layer's diffuse,
            // the normal maps it has), kept for the weights; the base's diffuse once more.
            let base_raw = base_diffuse(in.instance_index, uv);
            let n = min(counts.x, 6u);
            var texels = array<vec4<f32>, 6>(vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0));
            var paint = array<f32, 6>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            var layer_n = array<vec3<f32>, 6>(plain, plain, plain, plain, plain, plain);
            for (var k = 0u; k < n; k += 1u) {
                @if(!TERRAIN_PARALLAX) let texel = layer_diffuse(k, in.instance_index, uv);
                @if(TERRAIN_PARALLAX) let texel = layer_diffuse_antitiled(k, in.instance_index, parallax_taps, parallax_fp);
                texels[k] = texel;
                paint[k] = saturate(opacity[k]);
                @if(VERTEX_TANGENTS) if (counts.y & (1u << k)) != 0u {
                    @if(!TERRAIN_PARALLAX) layer_n[k] = apply_normal_mapping(standard.flags, tbn, false, is_front, layer_normal(k, in.instance_index, uv));
                    @if(TERRAIN_PARALLAX) layer_n[k] = terrain_world_normal(tbn, layer_normal_antitiled(k, in.instance_index, parallax_taps, parallax_fp, terrain_two_component(standard.flags), terrain_flip_y(standard.flags)), false);
                }
            }
            var w = terrain_ordered_shares(paint, n);
            // One reference read per contributing texture, global or local. Heights
            // use raw decoded colour before tint. A base without a texture is flat.
            var height = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            if w[0] > 0.0 && (standard.flags & STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
                height[0] = terrain_height(base_raw.rgb, base_reference(in.instance_index, uv, height_dx, height_dy, params.z), params.y);
            }
            for (var k = 0u; k < n; k += 1u) {
                if w[k + 1u] > 0.0 {
                    height[k + 1u] = terrain_height(texels[k].rgb, layer_reference(k, in.instance_index, uv, height_dx, height_dy, params.z), params.y);
                }
            }
            var f = terrain_height_weights(w, height, params.x);
            // Parallax: the weights the march used, worked out at the plain place.
            @if(TERRAIN_PARALLAX) f = parallax_w;
            // Colour and normal with the same weights; the normal made unit length once.
            colour = f[0] * colour;
            normal = f[0] * normal;
            for (var k = 0u; k < n; k += 1u) {
                colour += f[k + 1u] * (texels[k].rgb * tint * standard.base_color.rgb);
                normal += f[k + 1u] * layer_n[k];
            }
            pbr_input.material.base_color = vec4(colour, 1.0);
            pbr_input.N = select(plain, normalize(normal), dot(normal, normal) > 1e-12);
        }
        @if(TERRAIN_ANTI_TILING) {
            // One set of taps for every texture of the fragment. The layers walked last to
            // first: a layer's ordered share is its painted share x what the later layers
            // leave (terrain_weights.wesl `terrain_ordered_shares`, the same products), so a
            // layer of opacity 0, and everything under a fully covering layer (the base
            // too), is not read.
            let taps = terrain_taps(fp.uv);
            let n = min(counts.x, 6u);
            let flags = standard.flags;
            let tinted = tint * standard.base_color.rgb;
            var raw = array<vec4<f32>, 7>(vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0));
            var w = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            var remaining = 1.0;
            for (var i = i32(n) - 1; i >= 0; i -= 1) {
                if remaining == 0.0 {
                    break;
                }
                let k = u32(i);
                let o = saturate(opacity[k]);
                if o == 0.0 {
                    continue;
                }
                raw[k + 1u] = layer_diffuse_antitiled(k, in.instance_index, taps, fp);
                w[k + 1u] = o * remaining;
                remaining *= 1.0 - o;
            }
            w[0] = remaining;
            if w[0] != 0.0 {
                raw[0] = terrain_base_diffuse(in.instance_index, flags, taps, fp);
            }
            // Final weights: ordered shares, or depth competition using heights from
            // those same raw texels (one global coarsest-mip reference per contributor).
            // Exact zeros skip; tiny shares count [design, #2551].
            var f = w;
            @if(TERRAIN_HEIGHT_BLEND) {
                // Anti-tiling always uses the offset-independent global reference, even
                // when source=local: a local reference would need the same offset taps.
                let anti_params = height_params(in.instance_index);
                var height = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
                if w[0] != 0.0 && terrain_base_textured(flags) {
                    height[0] = terrain_height(raw[0].rgb, base_reference(in.instance_index, vec2(0.5), vec2(0.0), vec2(0.0), 0.0), anti_params.y);
                }
                for (var k = 0u; k < n; k += 1u) {
                    if w[k + 1u] != 0.0 {
                        height[k + 1u] = terrain_height(raw[k + 1u].rgb, layer_reference(k, in.instance_index, vec2(0.5), vec2(0.0), vec2(0.0), 0.0), anti_params.y);
                    }
                }
                f = terrain_height_weights(w, height, anti_params.x);
            }
            // Colour and normal with the same weights; a normal map read only where its
            // texture contributes. Layer normals in the frame the ordinary path uses.
            var colour_sum = vec3(0.0);
            var normal_sum = vec3(0.0);
            if f[0] != 0.0 {
                colour_sum += f[0] * raw[0].rgb * tinted;
                var base_n = plain;
                @if(VERTEX_TANGENTS && STANDARD_MATERIAL_NORMAL_MAP) {
                    base_n = terrain_world_normal(tbn, terrain_base_normal(in.instance_index, flags, taps, fp), terrain_back_face(pbr_input, is_front));
                }
                normal_sum += f[0] * base_n;
            }
            for (var k = 0u; k < n; k += 1u) {
                if f[k + 1u] != 0.0 {
                    colour_sum += f[k + 1u] * raw[k + 1u].rgb * tinted;
                    var layer_n = plain;
                    @if(VERTEX_TANGENTS) if (counts.y & (1u << k)) != 0u {
                        layer_n = terrain_world_normal(tbn, layer_normal_antitiled(k, in.instance_index, taps, fp, terrain_two_component(flags), terrain_flip_y(flags)), false);
                    }
                    normal_sum += f[k + 1u] * layer_n;
                }
            }
            let n_pre = pbr_input.N;
            pbr_input.material.base_color = vec4(colour_sum, 1.0);
            pbr_input.N = select(plain, normalize(normal_sum), dot(normal_sum, normal_sum) > 1e-12);
            pbr_input = terrain_pbr_occlusion(pbr_input, n_pre);
        }
    }
    // [design] Surface bake of a folded base (terrain_surface.rs): its unlit colour.
    @if(SURFACE_BAKE) return surface_bake(pbr_input.material.base_color.rgb);
    @if(!SURFACE_BAKE) {
    if counts.z != 0u {
        let s = scorch(in.instance_index, in.world_position.xyz);
        pbr_input.material.base_color = vec4(pbr_input.material.base_color.rgb * mix(1.0, SCORCHED, s), 1.0);
    }
    pbr_input.material.base_color.a = 1.0;

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Bevy's ambient specular out again in display-value mode, as in nif_material.wesl
    // (the game's ambient light is a flat colour; highlights come from lights).
    @if(!NVF_PHYSICAL_PBR) {
        let m = pbr_input.material;
        let F0 = calculate_F0(m.base_color.rgb, m.metallic, m.reflectance);
        let NdotV = max(dot(pbr_input.N, pbr_input.V), 0.0001);
        let spec = EnvBRDFApprox(F0, F_AB(m.perceptual_roughness, NdotV));
        let spec_occlusion = saturate(dot(F0, vec3(50.0 * 0.33)));
        var ambient_spec = spec * spec_occlusion * lights.ambient_color.rgb * pbr_input.diffuse_occlusion;
        if lights.ambient_color.a < 0.0 {
            ambient_spec *= pow(max(lights.ambient_color.rgb * view.exposure, vec3(1e-6)), vec3(1.2));
        }
        out.color = vec4(max(out.color.rgb - view.exposure * ambient_spec, vec3(0.0)), out.color.a);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
    }
}
