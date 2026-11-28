// Grass (grass.rs): vertex shader for the main pass and the prepass (depth), so both see
// the same motion, and the main pass's fragment shader.
//
// A grass mesh holds every plant of one type in a quarter of a cell. UV_1 holds, per
// vertex, the sway weight (the model's vertex alpha: 0 at the foot, more towards the
// tips) and the height above the plant's foot (game units).
//
// Wind: a vertex moves along the wind by its sway weight × the wind's magnitude, in waves
// that travel across the ground, so neighbouring plants move nearly together and distant
// ones do not. The wave speed comes from the grass type's wave period and the magnitude
// from the weather's wind speed (grass.md §5).
//
// Distance: beyond the grass distance the plants sink into the ground, each vertex by its
// own height (so the plant flattens) and a little more, over the fade range. The game
// fades its grass out over that range; how it does so is not known (grass.md §5).
//
// Edges (fragment, GRASS_COVERAGE: the view is multisampled): the texture's alpha is cut
// at the model's alpha-test threshold, as in the depth prepass, but texels below the
// threshold are not dropped: they cover a share of the pixel's samples, alpha / threshold
// (alpha to coverage). Strands end softly instead of in hard steps, and thin strands
// whose alpha the smaller mip levels have averaged down stay visible at a distance. The
// share is 1 wherever the prepass wrote depth, so nothing behind is wrongly hidden.
// Without multisampling the alpha is tested as before.
//
// Light (fragment): lit with the ground's normal (grass.rs), but with the ambient light
// and the sun weighted by `light` (grass.md §5).
//
// Burning (nvf_play grass_fire, docs/design/grass-fire.md): the burn map holds a byte per
// 64-unit patch of the ground, wrapping every 512 patches: 0 untouched, up to 0.5 while
// the patch burns, 0.5 to 1 once it has burnt. A plant turns black over the first half
// (its vertex colour), glows like embers while it burns (fragment), and over the second
// half sinks into the ground the way distant grass does.

import bevy_pbr::{
    render::mesh_functions,
    render::mesh_view_bindings::view,
    render::view_transformations::position_world_to_clip,
};
import bevy_render::globals::Globals;
@if(PREPASS_PIPELINE) import bevy_pbr::prepass::io::{Vertex, VertexOutput};
@if(!PREPASS_PIPELINE) import bevy_pbr::render::forward_io::{Vertex, VertexOutput, FragmentOutput};
@if(!PREPASS_PIPELINE) import bevy_pbr::render::mesh_view_bindings::globals;
@if(!PREPASS_PIPELINE) import bevy_pbr::render::pbr_fragment::pbr_input_from_standard_material;
@if(!PREPASS_PIPELINE) import bevy_pbr::render::pbr_functions::{nvf_apply_pbr_lighting, main_pass_post_lighting_processing};
@if(!PREPASS_PIPELINE && VISIBILITY_RANGE_DITHER) import bevy_pbr::render::pbr_functions::visibility_range_dither;
@if(PREPASS_PIPELINE && MOTION_VECTOR_PREPASS) import bevy_pbr::prepass::bindings::previous_view_uniforms;
// The prepass view bind group holds the globals at binding 1 (the main pass at 11).
@if(PREPASS_PIPELINE) @group(0) @binding(1) var<uniform> globals: Globals;

struct GrassParams {
    // xyz: wind direction (world, horizontal, unit); w: how far a vertex with sway
    // weight 1 moves (game units).
    wind: vec4<f32>,
    // x: wave angular speed (rad/s); y: distance at which the grass starts to go; z:
    // distance over which it goes; w: unused.
    motion: vec4<f32>,
    // x: factor on the ambient light; y: factor on every other light (the sun).
    light: vec4<f32>,
    previous_wind: vec4<f32>,
    previous_motion: vec4<f32>,
    change_frame: vec4<u32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> grass: GrassParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var burn_map: texture_2d<f32>;

// The burn map's patch size (game units) and size (patches); nvf_play grass_fire PATCH, MAP.
const BURN_PATCH: f32 = 64.0;
const BURN_MAP: i32 = 512;
// How dark burnt grass gets (a factor on its colour) [chosen].
const CHARRED: f32 = 0.05;

// A vertex's plant's foot (Bevy world space): the vertex colour's alpha holds the way
// there, game x and y packed as (x + 1024) × 2048 + (y + 1024) (grass.rs `pack_foot`).
fn foot_of(world: vec3<f32>, packed: f32) -> vec3<f32> {
    let qx = floor(packed / 2048.0);
    let qy = packed - qx * 2048.0;
    // Game (x, y) is Bevy (x, -z).
    return world + vec3<f32>(qx - 1024.0, 0.0, -(qy - 1024.0));
}

// The burn map's value at a point (Bevy world space): game (x, y) is Bevy (x, -z)
// (nvforever world.rs `bevy_to_game`).
fn burn_at(world: vec3<f32>) -> f32 {
    // (`patch` is a reserved word in WGSL.)
    let square = vec2<i32>(floor(vec2<f32>(world.x, -world.z) / BURN_PATCH));
    let t = ((square % BURN_MAP) + BURN_MAP) % BURN_MAP;
    return textureLoad(burn_map, t, 0).r;
}

// How far below the ground a plant's foot ends up once it is gone (game units).
const SINK: f32 = 24.0;
// Wave length of the wind's waves along and across the wind (game units) [chosen].
const WAVE_ALONG: f32 = 900.0;
const WAVE_ACROSS: f32 = 2300.0;

// Evaluate the identical deformation at either frame's model, time and camera.
@if(VERTEX_UVS_B)
fn deform(base: vec3<f32>, uv_b: vec2<f32>, wind: vec4<f32>, motion: vec4<f32>, time: f32, camera: vec3<f32>, burn: f32) -> vec3<f32> {
    var world = base;
    let along = wind.xyz;
    let across = vec3<f32>(-along.z, 0.0, along.x);
    let phase = 6.2831853 * (dot(world, along) / WAVE_ALONG + dot(world, across) / WAVE_ACROSS);
    let t = time * motion.x;
    let swing = 0.5 + 0.35 * sin(t - phase) + 0.15 * sin(2.31 * t - 1.7 * phase);
    world += along * (wind.w * uv_b.x * swing);
    let far = clamp((distance(camera, world) - motion.y) / motion.z, 0.0, 1.0);
    let gone = max(far, saturate(burn * 2.0 - 1.0));
    world.y -= gone * (uv_b.y + SINK);
    return world;
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0)).xyz;

    // At the plant's foot, so the whole plant chars and sinks as one.
    var foot = world;
    @if(VERTEX_COLORS) foot = foot_of(world, vertex.color.a);
    let burn = burn_at(foot);
    @if(VERTEX_UVS_B) world = deform(world, vertex.uv_b, grass.wind, grass.motion, globals.time, view.world_position.xyz, burn);
    @if(VERTEX_UVS_B) out.uv_b = vertex.uv_b;

    out.world_position = vec4<f32>(world, 1.0);
    out.position = position_world_to_clip(world);
    @if(VERTEX_UVS_A) out.uv = vertex.uv;

    @if(PREPASS_PIPELINE && UNCLIPPED_DEPTH_ORTHO_EMULATION) out.unclipped_depth = out.position.z;
    @if(PREPASS_PIPELINE && UNCLIPPED_DEPTH_ORTHO_EMULATION) out.position.z = min(out.position.z, 1.0);
    @if(PREPASS_PIPELINE && NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS) out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    @if(PREPASS_PIPELINE && NORMAL_PREPASS_OR_DEFERRED_PREPASS && !VERTEX_NORMALS) out.world_normal = view.world_from_view[2].xyz;
    @if(PREPASS_PIPELINE && MOTION_VECTOR_PREPASS) {
        let previous_model = mesh_functions::get_previous_world_from_local(vertex.instance_index);
        var previous_world = mesh_functions::mesh_position_local_to_world(previous_model, vec4(vertex.position, 1.0)).xyz;
        @if(VERTEX_UVS_B) {
            // The view transform is rigid: its transposed rotation and translation
            // recover the previous camera position without a matrix inverse.
            let v = previous_view_uniforms.view_from_world;
            let rotation = transpose(mat3x3(v[0].xyz, v[1].xyz, v[2].xyz));
            let camera = -(rotation * v[3].xyz);
            let changed = globals.frame_count == grass.change_frame.x;
            let wind = select(grass.wind, grass.previous_wind, changed);
            let motion = select(grass.motion, grass.previous_motion, changed);
            let previous_time_unwrapped = globals.time - globals.delta_time;
            let previous_time = select(previous_time_unwrapped, previous_time_unwrapped + 3600.0, previous_time_unwrapped < 0.0);
            previous_world = deform(previous_world, vertex.uv_b, wind, motion, previous_time, camera, burn);
        }
        out.previous_world_position = vec4(previous_world, 1.0);
    }
    @if(!PREPASS_PIPELINE && VERTEX_NORMALS) {
        var wn = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
        // Canopy shading (Physical lighting): the tuft's normal leans with the wind's push.
        @if(!PREPASS_PIPELINE && NVF_CANOPY && VERTEX_UVS_B) {
            let bend = saturate(vertex.uv_b.x * grass.wind.w / 60.0) * 0.25;
            wn = normalize(wn + grass.wind.xyz * bend);
        }
        out.world_normal = wn;
    }

    // Opaque: the alpha held the way to the foot.
    @if(VERTEX_COLORS) {
        var vc = vertex.color.rgb * mix(1.0, CHARRED, saturate(burn * 2.0));
        // Canopy shading: each plant's brightness and hue varied a little by its foot
        // [design, tuned by eye on #3828's branch].
        @if(!PREPASS_PIPELINE && NVF_CANOPY) {
            let h1 = fract(sin(dot(foot.xz, vec2<f32>(12.9898, 78.233))) * 43758.5453);
            let h2 = fract(sin(dot(foot.xz, vec2<f32>(39.3468, 11.135))) * 24634.6345);
            let luma_var = mix(0.86, 1.14, h1);
            let hue_var = mix(vec3<f32>(1.06, 1.01, 0.90), vec3<f32>(0.93, 1.02, 0.96), h2);
            vc *= hue_var * luma_var;
        }
        out.color = vec4<f32>(vc, 1.0);
    }
    @if(VERTEX_OUTPUT_INSTANCE_INDEX) out.instance_index = vertex.instance_index;
    @if(VISIBILITY_RANGE_DITHER) out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index, world_from_local[3]);
    return out;
}

@if(!PREPASS_PIPELINE) @fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    @if(!PREPASS_PIPELINE && VISIBILITY_RANGE_DITHER) visibility_range_dither(in.position, in.visibility_range_dither);
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let cutoff = max(pbr_input.material.alpha_cutoff, 0.01);
    let alpha = pbr_input.material.base_color.a;
    @if(!PREPASS_PIPELINE && GRASS_COVERAGE) let coverage = saturate(alpha / cutoff);
    @if(!PREPASS_PIPELINE && GRASS_COVERAGE) if coverage < 0.03 {
        discard;
    }
    @if(!PREPASS_PIPELINE && !GRASS_COVERAGE) let coverage = 1.0;
    // PREPASS_COVERAGE (nvf_assets `grass_edges`): the depth prepass wrote the blade on the
    // samples it covers and the depth test is for equality: nothing to test here.
    @if(!PREPASS_PIPELINE && !GRASS_COVERAGE && !PREPASS_COVERAGE) if alpha < cutoff {
        discard;
    }
    pbr_input.material.base_color.a = 1.0;
    // Canopy shading: the dense root darker (occlusion and colour), the tips open.
    @if(!PREPASS_PIPELINE && NVF_CANOPY) {
        var tip = 0.5;
        @if(VERTEX_UVS_B) tip = saturate(max(in.uv_b.x, in.uv_b.y / max(grass.motion.w, 6.0)));
        let root_ao = mix(0.32, 1.0, pow(tip, 0.65));
        pbr_input.diffuse_occlusion *= vec3<f32>(root_ao);
        let root_grounding = mix(0.68, 1.0, smoothstep(0.0, 0.28, tip));
        pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * root_grounding, pbr_input.material.base_color.a);
    }
    var out: FragmentOutput;
    // Grass weighs the ambient light and the sun differently from other surfaces
    // (grass.md §5): the two parts of the display-value lighting sum (nvforever lights.rs
    // DISPLAY_HELPERS) are scaled, each by its factor's display value, so that either part
    // alone is scaled by the factor as light. Under Physical lighting the sun keeps its whole
    // light and the canopy's root shading grounds the tuft instead [design, #3828's branch].
    @if(NVF_PHYSICAL_PBR) let grass_weights = vec2<f32>(1.0, 1.0);
    @if(!NVF_PHYSICAL_PBR) let grass_weights = pow(max(grass.light.xy, vec2(0.0)), vec2(1.0 / 2.2));
    var lit = nvf_apply_pbr_lighting(pbr_input, grass_weights).rgb;
    // Embers while the patch burns, flickering, brighter at the foot [chosen].
    let burn = burn_at(in.world_position.xyz);
    if burn > 0.004 && burn < 0.5 {
        let p = in.world_position.xyz;
        let flicker = 0.65 + 0.35 * sin(globals.time * 9.0 + p.x * 0.11 + p.z * 0.07);
        var low = 1.0;
        @if(VERTEX_UVS_B) low = 1.0 - saturate(in.uv_b.y / 60.0) * 0.6;
        lit += vec3<f32>(1.0, 0.32, 0.06) * (0.9 * flicker * low);
    }
    out.color = vec4(lit, 1.0);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    out.color.a = coverage;
    return out;
}
