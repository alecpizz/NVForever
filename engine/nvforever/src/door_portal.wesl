// Seamless doors' portal (door_portal.rs): the doorway shows what the portal camera drew
// of the place behind the door, at the same screen position. Linear HDR in, the main
// view's tonemapping and grade on top, as the scene around it.

import bevy_pbr::{
    render::forward_io::VertexOutput,
    render::mesh_view_bindings::view,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var portal_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var portal_sampler: sampler;
// x = 1: flat magenta (the probe test aid's mask of the doorway).
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> portal_debug: vec4<f32>;
// xyz: into the doorway (world), w: how far back the view's depth is set, so the door
// swinging open is drawn in front of it and what lies deeper behind the door is not.
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<uniform> portal_recess: vec4<f32>;

struct PortalOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fragment(in: VertexOutput) -> PortalOut {
    var out: PortalOut;
    let uv = (in.position.xy - view.main_pass_viewport.xy) / view.main_pass_viewport.zw;
    let back = in.world_position.xyz + portal_recess.xyz * portal_recess.w;
    let clip = view.clip_from_world * vec4(back, 1.0);
    out.depth = clamp(clip.z / clip.w, 0.0, in.position.z);
    if portal_debug.x > 0.5 {
        // Marked (alpha 0.25, imagespace.wesl SCREEN_MARK): left ungraded, so the mask
        // stays magenta whatever the place's grade.
        out.color = vec4(1000.0, 0.0, 1000.0, 0.25);
        return out;
    }
    let c = textureSampleLevel(portal_texture, portal_sampler, uv, 0.0);
    out.color = vec4(c.rgb, 1.0);
    return out;
}
