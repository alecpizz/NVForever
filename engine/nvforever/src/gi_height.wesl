// The height field's pass (gi.rs `gi_height_from_depth`, graphics-remaster.md §2 "Height
// field"): the top-down camera's depth prepass (reverse z, orthographic: 1 at the camera,
// 0 at the far plane) as world heights. The uniform is gi.wesl's (the same struct).

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct Gi {
    // Last frame's view from this frame's view (camera motion).
    prev_view_from_view: mat4x4<f32>,
    // x, y: the projection's x and y scale; z: near plane; w: frame number.
    proj: vec4<f32>,
    // x, y: the picture's size (pixels); z, w: the half-resolution size.
    size: vec4<f32>,
    // x: radius (game units); y: thickness (game units); z: bounce strength; w: the
    // distance where the effect has faded out.
    params: vec4<f32>,
    // x: occlusion strength; y: the share of the new frame in `temporal`; z: the most
    // light a history texel gives; w: 1 when last frame's textures hold last frame.
    params2: vec4<f32>,
    // x: a debug view (1 distance, 2 openness, 3 bounced light), y: its scale; z: the
    // interior fill light (linear, after exposure; 0 outdoors); w: the glow field's
    // strength (gi.rs GLOW).
    debug: vec4<f32>,
    // World from view (rotation): the probe's light by world normal.
    world_from_view: mat4x4<f32>,
    // The light probe: x its strength on surfaces.
    probe: vec4<f32>,
    // The camera's world position.
    cam_world: vec4<f32>,
    // The height field: x, y its centre (world x, z), z its span, w 1 when valid.
    hf: vec4<f32>,
    // x the top-down camera's height, y its far plane, z bounce strength, w occlusion.
    hf2: vec4<f32>,
}

@group(0) @binding(0) var height_depth: texture_depth_2d;
@group(0) @binding(1) var<uniform> gi: Gi;

@fragment
fn height_from_depth(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let d = textureLoad(height_depth, vec2<i32>(floor(in.position.xy)), 0);
    return vec4(gi.hf2.x - (1.0 - d) * gi.hf2.y, 0.0, 0.0, 0.0);
}
