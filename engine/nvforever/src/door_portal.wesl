// Seamless doors' portal (door_portal.rs): the doorway shows what the portal camera drew
// of the place behind the door, at the same screen position. Linear HDR in, the main
// view's tonemapping and grade on top, as the scene around it.

import bevy_pbr::{
    render::forward_io::VertexOutput,
    render::mesh_view_bindings::view,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var portal_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var portal_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = (in.position.xy - view.main_pass_viewport.xy) / view.main_pass_viewport.zw;
    let c = textureSampleLevel(portal_texture, portal_sampler, uv, 0.0);
    return vec4(c.rgb, 1.0);
}
