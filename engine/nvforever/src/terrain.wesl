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
// layer's in the layers' order by opacity x the layer texture's alpha, as the layers'
// own blended draws blended their lit colours over the base; then the surface is lit once.
// What follows the lighting is nif_material.wesl's for an opaque lit shape with no emitted
// light: Bevy's ambient specular taken out again, then fog (and tonemapping in a view
// without HDR).

// PREPASS_PIPELINE: the deferred G-buffer pass (Solari, nvforever solari.rs): the same
// blended surface, written to the G-buffer instead of lit here.
@if(!PREPASS_PIPELINE) import bevy_pbr::{
    render::forward_io::{VertexOutput, FragmentOutput},
    render::pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing, calculate_F0},
    render::pbr_lighting::{EnvBRDFApprox, F_AB},
    render::mesh_view_bindings::lights,
};
@if(PREPASS_PIPELINE) import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
import bevy_pbr::{
    render::pbr_fragment::pbr_input_from_standard_material,
    render::pbr_functions::{calculate_tbn_mikktspace, apply_normal_mapping},
    render::mesh_view_bindings::view,
    render::mesh_bindings::mesh,
    render::mesh_functions::get_local_from_world,
    render::pbr_types::StandardMaterial,
};
@if(VISIBILITY_RANGE_DITHER) import bevy_pbr::render::pbr_functions::visibility_range_dither;
@if(BINDLESS) import bevy_render::bindless::{bindless_textures_2d, bindless_samplers_filtering};
@if(BINDLESS) import bevy_pbr::render::pbr_bindings::{material_indices, material_array};
@if(!BINDLESS) import bevy_pbr::render::pbr_bindings;

struct TerrainLayers {
    // x: how many layers the material holds (0 to 6); y: bit i set when layer i has a
    // normal map (without one its normal is the surface's own).
    counts: vec4<u32>,
    // x: world units per texture repeat when the texture coordinate is worked out here and
    // the mesh's holds the fifth and sixth opacities; 0 when the mesh's is the coordinate.
    tile: vec4<f32>,
}

// The extension's index table (indices 50-62 of TerrainLayers' bindless attribute).
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

// NVForever remaster (graphics-remaster.md §4 "Detail"): procedural micro-relief on the
// normal near the camera, where the 2010 textures run out of resolution: the gradient of
// two octaves of world-space value noise (cells of 9 and 3.5 game units), tilting the
// normal by up to about 0.3, faded out between 150 and 700 units [engineering choice].
// The same code is in terrain.wesl and nif_material.wesl. Integer hashing keeps it exact
// at the Mojave's coordinates (tens of thousands of units).
fn nvf_hash3(c: vec3<i32>) -> f32 {
    var h = (u32(c.x) * 0x8da6b343u) ^ (u32(c.y) * 0xd8163841u) ^ (u32(c.z) * 0xcb1ab31fu);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffu) / 65535.0;
}
fn nvf_noise3(p: vec3<f32>) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(nvf_hash3(i), nvf_hash3(i + vec3(1, 0, 0)), u.x), mix(nvf_hash3(i + vec3(0, 1, 0)), nvf_hash3(i + vec3(1, 1, 0)), u.x), u.y);
    let b = mix(mix(nvf_hash3(i + vec3(0, 0, 1)), nvf_hash3(i + vec3(1, 0, 1)), u.x), mix(nvf_hash3(i + vec3(0, 1, 1)), nvf_hash3(i + vec3(1, 1, 1)), u.x), u.y);
    return mix(a, b, u.z);
}
fn nvf_relief(p: vec3<f32>) -> f32 {
    return nvf_noise3(p / 9.0) * 0.65 + nvf_noise3(p / 3.5) * 0.35;
}
// The same relief as a detail map on the colour: darker in its hollows, lighter on its
// crests, by up to `amount`, near the camera only.
fn nvf_detail_colour(c: vec3<f32>, p: vec3<f32>, distance: f32, amount: f32) -> vec3<f32> {
    let fade = 1.0 - smoothstep(150.0, 700.0, distance);
    if fade <= 0.0 {
        return c;
    }
    return c * (1.0 + amount * fade * (nvf_relief(p) - 0.5) * 2.0);
}
fn nvf_detail_normal(n: vec3<f32>, p: vec3<f32>, distance: f32, strength: f32) -> vec3<f32> {
    let fade = 1.0 - smoothstep(150.0, 700.0, distance);
    if fade <= 0.0 {
        return n;
    }
    let e = 0.6;
    let h = nvf_relief(p);
    let g = vec3(nvf_relief(p + vec3(e, 0.0, 0.0)) - h, nvf_relief(p + vec3(0.0, e, 0.0)) - h, nvf_relief(p + vec3(0.0, 0.0, e)) - h) / e;
    let tangential = g - dot(g, n) * n;
    return normalize(n - tangential * (strength * fade));
}

@fragment
fn fragment(
    vertex: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var in = vertex;
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

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let counts = layer_counts(in.instance_index);
    @if(VERTEX_UVS_A) {
        let standard = standard_material(in.instance_index);
        let uv = (standard.uv_transform * vec3(in.uv, 1.0)).xy;
        var colour = pbr_input.material.base_color.rgb;
        var normal = pbr_input.N;
        let plain = normalize(pbr_input.world_normal);
        @if(VERTEX_TANGENTS) let tbn = calculate_tbn_mikktspace(in.world_normal, in.world_tangent);
        for (var k = 0u; k < min(counts.x, 6u); k += 1u) {
            let texel = layer_diffuse(k, in.instance_index, uv);
            let share = saturate(opacity[k]) * texel.a;
            var layer_n = plain;
            @if(VERTEX_TANGENTS) if (counts.y & (1u << k)) != 0u {
                layer_n = apply_normal_mapping(standard.flags, tbn, false, is_front, layer_normal(k, in.instance_index, uv));
            }
            // A normal that went bad (no tangent in some passes): the surface's own.
            if any(layer_n != layer_n) || dot(layer_n, layer_n) < 1e-6 {
                layer_n = plain;
            }
            colour = mix(colour, texel.rgb * tint * standard.base_color.rgb, share);
            normal = mix(normal, layer_n, share);
        }
        pbr_input.material.base_color = vec4(colour, 1.0);
        pbr_input.N = select(plain, normalize(normal), dot(normal, normal) > 1e-6);
    }
    pbr_input.material.base_color.a = 1.0;
    @if(SCREEN_INDIRECT) {
        let nvf_d = length(view.world_position - pbr_input.world_position.xyz);
        pbr_input.N = nvf_detail_normal(pbr_input.N, pbr_input.world_position.xyz, nvf_d, 6.0);
        pbr_input.material.base_color = vec4(nvf_detail_colour(pbr_input.material.base_color.rgb, pbr_input.world_position.xyz, nvf_d, 0.14), 1.0);
    }

    @if(PREPASS_PIPELINE) {
        pbr_input.material.reflectance = min(pbr_input.material.reflectance, vec3(0.3));
        pbr_input.material.perceptual_roughness = max(pbr_input.material.perceptual_roughness, 0.8);
        return deferred_output(in, pbr_input);
    }
    @if(!PREPASS_PIPELINE) {
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Bevy's ambient specular out again, as in nif_material.wesl (the game's ambient light
    // is a flat colour; highlights come from lights).
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
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
    }
}
