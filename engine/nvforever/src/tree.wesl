// SpeedTree trees (tree.rs): vertex shader for the bark (TREE_BARK) and the leaf cards
// (TREE_LEAVES), used for the main pass and the prepass (depth, shadows) so both see the
// same motion.
//
// Leaf cards: each card's four vertices sit at the leaf's position; UV_1 holds the
// corner's offset in the card's plane (local units, x right, y up). The card is spread
// in the plane of the view, so it always faces the camera (and, in the shadow pass, the
// light).
//
// Distant tree billboards (TREE_BILLBOARD, treelod.rs): every vertex of a card sits at
// the tree's foot; UV_1 holds the corner's offset (x right, y up, game units). The card
// stands upright and turns about the vertical towards the camera (the view position, so
// each tree faces it on its own). The wind uniform is zero for them.
//
// Wind: the whole tree rocks along the wind, more the higher a point is; leaf cards also
// rustle (flutter and tilt), each with its own phase. Speeds come from the TREE record's
// rock and rustle speeds and strength from the weather's wind speed; amplitudes and the
// mapping to frequencies are chosen by eye (speedtree.md §6).
//
// The fragment shader (leaf cards and billboards, main pass) is Bevy's standard one with
// the sun's shadow taken outside the crown on leaf cards (NVF_CROWN_SHADOW) and
// soft cut-out edges where the view is multisampled (SOFT_EDGES; nvf_assets
// nif_material.rs `soft_edges`).

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
@if(!PREPASS_PIPELINE) import bevy_pbr::render::pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing};
@if(!PREPASS_PIPELINE && VISIBILITY_RANGE_DITHER) import bevy_pbr::render::pbr_functions::visibility_range_dither;
@if(PREPASS_PIPELINE && MOTION_VECTOR_PREPASS) import bevy_pbr::prepass::bindings::previous_view_uniforms;
// The prepass view bind group holds the globals at binding 1 (the main pass at 11).
@if(PREPASS_PIPELINE) @group(0) @binding(1) var<uniform> globals: Globals;

struct TreeWind {
    // xyz: wind direction (world, horizontal, unit); w: strength 0-1.
    wind: vec4<f32>,
    // x: unused; y: rock angular speed; z: rustle angular speed (rad/s); w: tree
    // height in local units.
    motion: vec4<f32>,
    previous_wind: vec4<f32>,
    change_frame: vec4<u32>,
    // Leaf cards: the crown sphere (local units: centre xyz, radius w) for the sun's
    // shadow (NVF_CROWN_SHADOW); zero elsewhere.
    crown: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> tree_wind: TreeWind;

// How far the top of the tree moves along the wind, as a fraction of its height, at
// full strength [tuned by eye].
const ROCK: f32 = 0.25;
// Leaf flutter along the wind (fraction of the card size) and tilt (radians) at full
// strength [tuned by eye].
const FLUTTER: f32 = 0.08;
const TILT: f32 = 0.35;

// World-space offset of a point `local` (game space, z up) of the tree placed by
// `world_from_local`.
fn rock(world_from_local: mat4x4<f32>, local: vec3<f32>, wind: vec4<f32>, time: f32) -> vec3<f32> {
    let origin = world_from_local[3].xyz;
    let scale = length(world_from_local[0].xyz);
    let height = max(tree_wind.motion.w, 1e-4);
    let h = clamp(local.z / height, 0.0, 1.5);
    // Each tree has its own phase, from where it stands.
    let phase = dot(origin.xz, vec2<f32>(0.0131, 0.0173));
    // Seconds since start-up (wraps after an hour: one jump in the motion per hour).
    let t = time * tree_wind.motion.y;
    // Leaning with the wind, plus two unrelated swings.
    let swing = 0.5 + 0.35 * sin(t + phase) + 0.15 * sin(2.31 * t + 1.7 * phase);
    return wind.xyz * (wind.w * ROCK * h * h * height * scale * swing);
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let base = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0)).xyz;
    var world = base + rock(world_from_local, vertex.position, tree_wind.wind, globals.time);

    // Bevy space: y is up. Right = up x (towards the camera, flattened).
    @if(TREE_BILLBOARD && VERTEX_UVS_B) let to_cam = view.world_position.xyz - base;
    @if(TREE_BILLBOARD && VERTEX_UVS_B) var flat = vec2<f32>(to_cam.x, to_cam.z);
    @if(TREE_BILLBOARD && VERTEX_UVS_B) var right = vec3<f32>(1.0, 0.0, 0.0);
    @if(TREE_BILLBOARD && VERTEX_UVS_B) if (dot(flat, flat) > 1e-6) {
        flat = normalize(flat);
        right = vec3<f32>(flat.y, 0.0, -flat.x);
    }
    @if(TREE_BILLBOARD && VERTEX_UVS_B) world += right * vertex.uv_b.x + vec3<f32>(0.0, vertex.uv_b.y, 0.0);
    @if(TREE_BILLBOARD && VERTEX_UVS_B) out.uv_b = vertex.uv_b;

    // Rustle: each card (all four corners share its position) gets its own phase.
    @if(TREE_LEAVES && VERTEX_UVS_B) let s = length(world_from_local[0].xyz);
    @if(TREE_LEAVES && VERTEX_UVS_B) let phase = dot(vertex.position, vec3<f32>(311.7, 173.3, 237.1));
    @if(TREE_LEAVES && VERTEX_UVS_B) let r = sin(globals.time * tree_wind.motion.z + phase);
    @if(TREE_LEAVES && VERTEX_UVS_B) let a = tree_wind.wind.w * TILT * r;
    @if(TREE_LEAVES && VERTEX_UVS_B) let c = cos(a);
    @if(TREE_LEAVES && VERTEX_UVS_B) let sn = sin(a);
    @if(TREE_LEAVES && VERTEX_UVS_B) let corner = vec2<f32>(c * vertex.uv_b.x - sn * vertex.uv_b.y, sn * vertex.uv_b.x + c * vertex.uv_b.y);
    @if(TREE_LEAVES && VERTEX_UVS_B) let size = length(vertex.uv_b);
    @if(TREE_LEAVES && VERTEX_UVS_B) let right = view.world_from_view[0].xyz;
    @if(TREE_LEAVES && VERTEX_UVS_B) let up = view.world_from_view[1].xyz;
    @if(TREE_LEAVES && VERTEX_UVS_B) world += (right * corner.x + up * corner.y) * s;
    @if(TREE_LEAVES && VERTEX_UVS_B) world += tree_wind.wind.xyz * (tree_wind.wind.w * FLUTTER * size * s * r);
    @if(TREE_LEAVES && VERTEX_UVS_B) out.uv_b = vertex.uv_b;

    out.world_position = vec4<f32>(world, 1.0);
    out.position = position_world_to_clip(world);
    @if(VERTEX_UVS_A) out.uv = vertex.uv;
    @if(TREE_BARK && VERTEX_UVS_B) out.uv_b = vertex.uv_b;

    @if(PREPASS_PIPELINE && UNCLIPPED_DEPTH_ORTHO_EMULATION) out.unclipped_depth = out.position.z;
    @if(PREPASS_PIPELINE && UNCLIPPED_DEPTH_ORTHO_EMULATION) out.position.z = min(out.position.z, 1.0);
    @if(PREPASS_PIPELINE && NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS) out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    @if(PREPASS_PIPELINE && NORMAL_PREPASS_OR_DEFERRED_PREPASS && !VERTEX_NORMALS) out.world_normal = view.world_from_view[2].xyz;
    @if(PREPASS_PIPELINE && MOTION_VECTOR_PREPASS) {
        let previous_model = mesh_functions::get_previous_world_from_local(vertex.instance_index);
        let previous_base = mesh_functions::mesh_position_local_to_world(previous_model, vec4(vertex.position, 1.0)).xyz;
        let previous_time_unwrapped = globals.time - globals.delta_time;
        let previous_time = select(previous_time_unwrapped, previous_time_unwrapped + 3600.0, previous_time_unwrapped < 0.0);
        let wind = select(tree_wind.wind, tree_wind.previous_wind, globals.frame_count == tree_wind.change_frame.x);
        var previous_world = previous_base + rock(previous_model, vertex.position, wind, previous_time);
        @if((TREE_BILLBOARD || TREE_LEAVES) && VERTEX_UVS_B) {
            let v = previous_view_uniforms.view_from_world;
            let rotation = transpose(mat3x3(v[0].xyz, v[1].xyz, v[2].xyz));
            @if(TREE_BILLBOARD) {
                let camera = -(rotation * v[3].xyz);
                let to_camera = camera - previous_base;
                var previous_flat = vec2(to_camera.x, to_camera.z);
                var previous_right = vec3(1.0, 0.0, 0.0);
                if dot(previous_flat, previous_flat) > 1e-6 {
                    previous_flat = normalize(previous_flat);
                    previous_right = vec3(previous_flat.y, 0.0, -previous_flat.x);
                }
                previous_world += previous_right * vertex.uv_b.x + vec3(0.0, vertex.uv_b.y, 0.0);
            }
            @if(TREE_LEAVES) {
                let scale = length(previous_model[0].xyz);
                let previous_phase = dot(vertex.position, vec3(311.7, 173.3, 237.1));
                let rustle = sin(previous_time * tree_wind.motion.z + previous_phase);
                let angle = wind.w * TILT * rustle;
                let previous_c = cos(angle);
                let previous_sn = sin(angle);
                let previous_corner = vec2(previous_c * vertex.uv_b.x - previous_sn * vertex.uv_b.y, previous_sn * vertex.uv_b.x + previous_c * vertex.uv_b.y);
                previous_world += (rotation[0] * previous_corner.x + rotation[1] * previous_corner.y) * scale;
                previous_world += wind.xyz * (wind.w * FLUTTER * length(vertex.uv_b) * scale * rustle);
            }
        }
        out.previous_world_position = vec4(previous_world, 1.0);
    }
    @if(!PREPASS_PIPELINE && VERTEX_NORMALS) {
        var wn = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
        // Canopy shading (Physical lighting): a leaf card's normal bent out toward its
        // corners, so a card reads as part of a rounded clump [design, #3828's branch].
        @if(!PREPASS_PIPELINE && TREE_LEAVES && VERTEX_UVS_B && NVF_CANOPY) {
            wn = normalize(wn + (right * corner.x + up * corner.y) * (0.28 / max(size, 1.0)));
        }
        out.world_normal = wn;
    }

    @if(VERTEX_COLORS) out.color = vertex.color;
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
    // The share of the pixel's samples the leaf covers: 1 at the cutoff and above, falling
    // to 0 within a pixel outside it.
    @if(!PREPASS_PIPELINE && SOFT_EDGES) let edge_alpha = pbr_input.material.base_color.a;
    @if(!PREPASS_PIPELINE && SOFT_EDGES) let edge = saturate((edge_alpha - pbr_input.material.alpha_cutoff) / max(fwidth(edge_alpha), 0.0001) + 1.0);
    @if(!PREPASS_PIPELINE && SOFT_EDGES) if edge <= 0.0 {
        discard;
    }
    @if(!PREPASS_PIPELINE && SOFT_EDGES) pbr_input.material.base_color.a = 1.0;
    // PREPASS_COVERAGE (nvf_assets `soft_edges`): the depth prepass wrote the card on the
    // samples it covers and the depth test is for equality: nothing to test here.
    @if(!PREPASS_PIPELINE && PREPASS_COVERAGE) pbr_input.material.base_color.a = 1.0;
    @if(!PREPASS_PIPELINE && !SOFT_EDGES && !PREPASS_COVERAGE) pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    // Leaf cards receive no shadows (tree.rs `spawn`); the sun's comes from where its ray
    // leaves the crown (vendored bevy_pbr pbr_functions.wesl, "crown shadow"), the crown
    // rocking with the tree.
    @if(!PREPASS_PIPELINE && NVF_CROWN_SHADOW) {
        let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
        let crown = tree_wind.crown.xyz;
        let centre = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(crown, 1.0)).xyz
            + rock(world_from_local, crown, tree_wind.wind, globals.time);
        pbr_input.nvf_crown = vec4<f32>(centre, tree_wind.crown.w * length(world_from_local[0].xyz));
    }
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    @if(!PREPASS_PIPELINE && SOFT_EDGES) out.color.a = edge;
    return out;
}
