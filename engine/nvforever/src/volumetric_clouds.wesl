// Volumetric clouds (volumetric_clouds.rs; docs/design/graphics-enhanced.md §3.23, a
// prototype): the fragment of the sky proxy (the dome's own mesh, drawn depth-Equal on it).
// The slab is no longer marched here: the volumetric medium's trace pass (medium.rs,
// medium_trace.wesl) marched it before the main pass into the far buffer, and this fragment
// reads that buffer where its own view ray lies (medium_upsample.wesl) and writes it
// premultiplied over the sky dome, the stars and the moon. The colour is the weather's linear
// colours, as sky.wesl's are, mapped through the shared tonemapper input (nvf_assets tone_input.wesl) as the sky's
// are, once, here. An engineering choice, not the original
// game's.

import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};
import nvforever::medium_upsample::{medium_screen_uv, medium_far};
import nvf_assets::tone_input::to_tonemap_input_each;

// volumetric_clouds.rs CloudComposeUniform: x the view's tonemapper curve (lights.rs
// `ToneCurve::code`: 1 the colour goes in as it is, for display values clipped or, under
// Physical lighting, linear light; 0 the Tony inverse); y 1 to hold (nothing added: the proxy is drawn so that its pipeline is
// made, while the flat layers still show); z, w unused.
struct CloudCompose {
    flags: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> compose: CloudCompose;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var far_light: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var far_sampler: sampler;

fn cloud_radiance(colour: vec3<f32>) -> vec3<f32> {
    return to_tonemap_input_each(colour, compose.flags.x);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if compose.flags.y > 0.5 {
        return vec4<f32>(0.0);
    }
    let uv = medium_screen_uv(view.unjittered_clip_from_world, in.world_position.xyz);
    var c = medium_far(far_light, far_sampler, uv);
    // Convert to the framebuffer's lighting space before compositing.
    if c.a > 1e-4 {
        c = vec4<f32>(cloud_radiance(c.rgb / c.a) * c.a, c.a);
    }
    return c;
}
