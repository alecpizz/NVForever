// Landscape anti-tiling's surface set-up, shared by the folded terrain draw (nvforever
// terrain.wesl) and the terrain's own NifMaterial draws (nif_material.wesl), both only with
// TERRAIN_ANTI_TILING, in the main pass (nvf-assets terrain_sampling.rs; nvforever
// terrain.rs "Anti-tiling") [design, #2551]. The reads themselves are terrain_sampling.wesl's.
//
// Bevy's pbr_input_from_standard_material reads the base texture and normal map once at the
// plain coordinate; anti-tiling reads them at the taps instead, so instead of calling it and
// overwriting its result (wasted reads, and the SSAO multibounce taken from the plain
// colour) this starts from pbr_input_from_vertex_output and fills in what the landscape's
// StandardMaterial holds (nvforever terrain.rs `TerrainCtx::surface`): its colour, diffuse
// texture, normal map and scalar values. No other texture (emissive, metallic-roughness,
// occlusion, parallax, transmission, clearcoat, anisotropy, specular) and no lightmap is
// read: the landscape's materials have none (the contract test in terrain_sampling.rs).
// A future change to that material's contract has to change this file too.

import bevy_pbr::{
    render::forward_io::VertexOutput,
    render::pbr_fragment::pbr_input_from_vertex_output,
    render::pbr_types::{PbrInput, StandardMaterial, STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT, STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT, STANDARD_MATERIAL_FLAGS_TWO_COMPONENT_NORMAL_MAP, STANDARD_MATERIAL_FLAGS_FLIP_NORMAL_MAP_Y, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_BLEND},
    render::pbr_functions::{winding_corrected_front_facing, calculate_tbn_mikktspace},
    render::pbr_lighting::perceptualRoughnessToRoughness,
    render::mesh_view_bindings::view,
    render::mesh_bindings::mesh,
};
@if(SCREEN_SPACE_AMBIENT_OCCLUSION) import bevy_pbr::render::mesh_view_bindings::screen_space_ambient_occlusion_texture;
@if(SCREEN_SPACE_AMBIENT_OCCLUSION) import bevy_pbr::ssao::utils::ssao_multibounce;
@if(BINDLESS) import bevy_render::bindless::{bindless_textures_2d, bindless_samplers_filtering};
@if(BINDLESS) import bevy_pbr::render::pbr_bindings::{material_indices, material_array};
@if(!BINDLESS) import bevy_pbr::render::pbr_bindings;
import nvf_assets::terrain_sampling::{TerrainFootprint, TerrainTaps, terrain_hex3, terrain_sample_diffuse, terrain_sample_normal, terrain_normal_tap, terrain_unit_normal, terrain_world_normal, terrain_world_uv};

// The fragment's footprint: its texture coordinate from where it is (u = game x / repeat,
// v = game y / repeat, as the meshes carry it) through the material's coordinate transform,
// and that coordinate's change across a view pixel, scaled by the view's mip bias (set by
// the upscalers; a gradient read takes no bias of its own). Call it where every fragment of
// the quad still runs (before any discard or branch): screen derivatives.
fn terrain_footprint(world: vec4<f32>, repeat: f32, uv_transform: mat3x3<f32>) -> TerrainFootprint {
    let uv = (uv_transform * vec3(terrain_world_uv(world.xyz, repeat), 1.0)).xy;
    let mip_scale = exp2(view.mip_bias);
    return TerrainFootprint(uv, dpdx(uv) * mip_scale, dpdy(uv) * mip_scale);
}

// Hex3 reads, shared by every texture the fragment reads [design, #2551].
fn terrain_taps(uv: vec2<f32>) -> TerrainTaps {
    return terrain_hex3(uv);
}

// The material's slot in the bindless tables.
fn terrain_slot(instance_index: u32) -> u32 {
    return mesh[instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
}

// A bindless texture `t` read with sampler `s` at every kept tap (terrain_sample_diffuse's
// rule). The table is indexed here, where it is read, so the index keeps its non-uniform
// marking.
@if(BINDLESS) fn terrain_bindless_diffuse(t: u32, s: u32, taps: TerrainTaps, fp: TerrainFootprint) -> vec4<f32> {
    var ts = taps;
    var sum = vec4(0.0);
    for (var j = 0u; j < ts.count; j += 1u) {
        let tap = ts.taps[j];
        if tap.weight > 0.0 {
            sum += tap.weight * textureSampleGrad(bindless_textures_2d[t], bindless_samplers_filtering[s], tap.uv, tap.rotation * fp.dx, tap.rotation * fp.dy);
        }
    }
    return sum;
}

// A bindless normal map (terrain_sample_normal's rule).
@if(BINDLESS) fn terrain_bindless_normal(t: u32, s: u32, taps: TerrainTaps, fp: TerrainFootprint, two_component: bool, flip_y: bool) -> vec3<f32> {
    var ts = taps;
    var sum = vec3(0.0);
    for (var j = 0u; j < ts.count; j += 1u) {
        let tap = ts.taps[j];
        if tap.weight > 0.0 {
            let encoded = textureSampleGrad(bindless_textures_2d[t], bindless_samplers_filtering[s], tap.uv, tap.rotation * fp.dx, tap.rotation * fp.dy).rgb;
            sum += tap.weight * terrain_normal_tap(encoded, two_component, flip_y, tap.rotation);
        }
    }
    return terrain_unit_normal(sum);
}

// Whether the StandardMaterial has a diffuse texture (without one its raw colour is white).
fn terrain_base_textured(flags: u32) -> bool {
    return (flags & STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u;
}

// Whether normal maps under these StandardMaterial flags hold only x and y, and whether
// their green points down the image.
fn terrain_two_component(flags: u32) -> bool {
    return (flags & STANDARD_MATERIAL_FLAGS_TWO_COMPONENT_NORMAL_MAP) != 0u;
}
fn terrain_flip_y(flags: u32) -> bool {
    return (flags & STANDARD_MATERIAL_FLAGS_FLIP_NORMAL_MAP_Y) != 0u;
}

// The StandardMaterial's diffuse texture, untinted (linear colour, its alpha), anti-tiled;
// white without one. Read with the material's base-colour sampler, as Bevy reads it.
fn terrain_base_diffuse(instance_index: u32, flags: u32, taps: TerrainTaps, fp: TerrainFootprint) -> vec4<f32> {
    if !terrain_base_textured(flags) {
        return vec4(1.0);
    }
    @if(BINDLESS) {
        let i = material_indices[terrain_slot(instance_index)];
        return terrain_bindless_diffuse(i.base_color_texture, i.base_color_sampler, taps, fp);
    }
    @if(!BINDLESS) {
        return terrain_sample_diffuse(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler, taps, fp);
    }
}

// The StandardMaterial's normal map, anti-tiled, as a tangent-space normal (decoded by the
// material's flags, turned back per tap, blended, unit length). Read with the material's
// normal-map sampler, as Bevy reads it. Only where the pipeline has STANDARD_MATERIAL_NORMAL_MAP.
fn terrain_base_normal(instance_index: u32, flags: u32, taps: TerrainTaps, fp: TerrainFootprint) -> vec3<f32> {
    @if(BINDLESS) {
        let i = material_indices[terrain_slot(instance_index)];
        return terrain_bindless_normal(i.normal_map_texture, i.normal_map_sampler, taps, fp, terrain_two_component(flags), terrain_flip_y(flags));
    }
    @if(!BINDLESS) {
        return terrain_sample_normal(pbr_bindings::normal_map_texture, pbr_bindings::normal_map_sampler, taps, fp, terrain_two_component(flags), terrain_flip_y(flags));
    }
}

// The surface before any texture is read: Bevy's set-up from the vertex (mesh flags, view
// vector, position, world normal; N the prepass normal with LOAD_PREPASS_NORMALS, else the
// world normal), the StandardMaterial's values copied whole (flags, roughness, reflectance,
// alpha mode and cutoff, uv transform, the rest), and the colour vertex colour x material
// colour, once; the caller multiplies its texture in and sets N.
fn terrain_pbr_begin(in: VertexOutput, is_front: bool, standard: StandardMaterial) -> PbrInput {
    let double_sided = (standard.flags & STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT) != 0u;
    var p = pbr_input_from_vertex_output(in, is_front, double_sided);
    // White without vertex colours (pbr_input_new's).
    let vertex_colour = p.material.base_color;
    p.material = standard;
    p.material.base_color = vertex_colour * standard.base_color;
    p.clearcoat_N = normalize(p.world_normal);
    return p;
}

// The StandardMaterial of the draw (either binding).
fn terrain_standard_material(instance_index: u32) -> StandardMaterial {
    @if(BINDLESS) return material_array[material_indices[terrain_slot(instance_index)].material];
    @if(!BINDLESS) return pbr_bindings::material;
}

// Whether the back of a double-sided surface is in view (its tangent-space normals are
// turned round, as in Bevy's apply_normal_mapping).
fn terrain_back_face(p: PbrInput, is_front: bool) -> bool {
    let double_sided = (p.material.flags & STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT) != 0u;
    return double_sided && !winding_corrected_front_facing(p.flags, is_front);
}

// Diffuse and specular occlusion once the surface colour is final: Bevy's SSAO multibounce
// on that colour and its specular occlusion (Lagarde and Rousiers 2014), with the
// view-vector term taken from `n_pre`, the normal before any normal map (as Bevy takes it);
// none without SSAO (the landscape's materials have no occlusion texture).
fn terrain_pbr_occlusion(p_in: PbrInput, n_pre: vec3<f32>) -> PbrInput {
    var p = p_in;
    var diffuse_occlusion = vec3(1.0);
    var specular_occlusion = 1.0;
    @if(SCREEN_SPACE_AMBIENT_OCCLUSION) {
        let ssao = textureLoad(screen_space_ambient_occlusion_texture, vec2<i32>(p.frag_coord.xy - view.viewport.xy), 0i).r;
        diffuse_occlusion = min(diffuse_occlusion, ssao_multibounce(ssao, p.material.base_color.rgb));
        let roughness = perceptualRoughnessToRoughness(p.material.perceptual_roughness);
        let NdotV = max(dot(n_pre, p.V), 0.0001);
        specular_occlusion = saturate(pow(NdotV + ssao, exp2(-16.0 * roughness - 1.0)) - 1.0 + ssao);
    }
    p.diffuse_occlusion = diffuse_occlusion;
    p.specular_occlusion = specular_occlusion;
    return p;
}

// A landscape draw of its own (nif_material.wesl with TERRAIN_SEPARATE: a quadrant's base
// that has no folded layers, a layer drawn blended over it, every layer where the GPU has
// no bindless textures): the surface as Bevy's pbr_input_from_standard_material would set
// it up for the landscape's material, with the diffuse texture and normal map read
// anti-tiled. Colour: vertex colour x material colour x texture; alpha: a layer's opacity
// (the vertex colour's alpha) x the material's, never the texture's (#3789). A blended layer
// whose coverage is exactly 0 adds nothing: it reads nothing. The caller's alpha_discard
// makes an opaque base opaque.
fn terrain_pbr_standard(in: VertexOutput, is_front: bool, fp: TerrainFootprint) -> PbrInput {
    let standard = terrain_standard_material(in.instance_index);
    var p = terrain_pbr_begin(in, is_front, standard);
    let n_pre = p.N;
    p.N = normalize(p.world_normal);
    let blended = (standard.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS) == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_BLEND;
    if blended && p.material.base_color.a == 0.0 {
        return terrain_pbr_occlusion(p, n_pre);
    }
    let taps = terrain_taps(fp.uv);
    p.material.base_color = vec4(p.material.base_color.rgb * terrain_base_diffuse(in.instance_index, standard.flags, taps, fp).rgb, p.material.base_color.a);
    @if(VERTEX_TANGENTS && STANDARD_MATERIAL_NORMAL_MAP) {
        let tbn = calculate_tbn_mikktspace(p.world_normal, in.world_tangent);
        p.N = terrain_world_normal(tbn, terrain_base_normal(in.instance_index, standard.flags, taps, fp), terrain_back_face(p, is_front));
    }
    return terrain_pbr_occlusion(p, n_pre);
}
