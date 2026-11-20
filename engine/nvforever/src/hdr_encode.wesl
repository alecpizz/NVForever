// HDR output's encode (hdr.rs): the finished frame (the views and the UI, 1.0 = paper
// white, the main view's highlights above it) into scRGB (1.0 = 80 nits) for the display.
//
// Below 1, each channel is read as an SDR display would show it: through the sRGB curve
// and back with a 2.2 power (`gamma22`), as SDR games are made on gamma 2.2 screens and a
// straight sRGB decode lifts the shadows grey in HDR [design]; then the black floor (> 0
// crushes, < 0 lifts). Above 1 values pass as they are (both curves meet at 1).

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct EncodeParams {
    // Paper white and the peak in scRGB units (80 nits).
    paper_scale: f32,
    peak_scale: f32,
    black_floor: f32,
    gamma22: f32,
}

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: EncodeParams;

fn srgb_encode(v: f32) -> f32 {
    if v <= 0.0031308 {
        return v * 12.92;
    }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}

fn shape(v: f32) -> f32 {
    if v >= 1.0 {
        return v;
    }
    var x = max(v, 0.0);
    if params.gamma22 > 0.5 {
        x = pow(srgb_encode(x), 2.2);
    }
    let f = params.black_floor;
    return max(x - f, 0.0) / (1.0 - f);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let c = textureLoad(frame, vec2<i32>(in.position.xy), 0);
    let shaped = vec3(shape(c.r), shape(c.g), shape(c.b));
    return vec4(min(shaped * params.paper_scale, vec3(params.peak_scale)), 1.0);
}
