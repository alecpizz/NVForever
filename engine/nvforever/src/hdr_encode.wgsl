// HDR output's encode (hdr.rs): a finished frame (1.0 = paper white, the main view's
// highlights above it) into scRGB (1.0 = 80 nits) for the display.
//
// Below 1, each channel is read as an SDR display would show it: through the sRGB curve
// and back with a 2.2 power (`gamma22`), as SDR games are made on gamma 2.2 screens and a
// straight sRGB decode lifts the shadows grey in HDR [design]; then the black level (> 0
// sinks the darkest shades to black, < 0 lifts them). Above 1 values pass as they are (both
// curves meet at 1). Then paper white, and nothing above the peak.

struct EncodeParams {
    paper_scale: f32,
    peak_scale: f32,
    black_floor: f32,
    gamma22: f32,
}

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var nearest: sampler;
@group(0) @binding(2) var<uniform> params: EncodeParams;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return Out(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv);
}

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
fn fragment(in: Out) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(frame, nearest, in.uv, 0.0);
    let shaped = vec3<f32>(shape(c.r), shape(c.g), shape(c.b));
    return vec4<f32>(min(shaped * params.paper_scale, vec3<f32>(params.peak_scale)), 1.0);
}
