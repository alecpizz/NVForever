// HDR output's encode (hdr.rs): a finished frame (1.0 = paper white, the main view's
// highlights above it) into scRGB (1.0 = 80 nits) for the display.
//
// Below 1, each channel is read as an SDR display would show it: through the sRGB curve
// and back with a 2.2 power (`gamma22`), as SDR games are made on gamma 2.2 screens and a
// straight sRGB decode lifts the shadows grey in HDR [design]; then the black level (> 0
// sinks the darkest shades to black, < 0 lifts them). Above 1 values pass as they are (both
// curves meet at 1). Then paper white, and nothing above the peak.
//
// While an HDR row of the settings page is chosen, a calibration pattern is drawn in `rect`
// (hdr.rs `Calibration`), in absolute nits, over whatever the frame has there.

struct EncodeParams {
    // Paper white and the peak in scRGB units (80 nits), the black level, gamma 2.2 (1).
    look: vec4<f32>,
    // The pattern (0 none, 1 peak, 2 shadows, 3 paper white, 4 highlights), the peak in
    // paper whites, the knee, the highlights' strength.
    pattern: vec4<f32>,
    // The pattern's box in the window's parts: left, top, right, bottom.
    rect: vec4<f32>,
};

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

fn srgb_decode(v: f32) -> f32 {
    if v <= 0.04045 {
        return v / 12.92;
    }
    return pow((v + 0.055) / 1.055, 2.4);
}

fn shape(v: f32) -> f32 {
    if v >= 1.0 {
        return v;
    }
    var x = max(v, 0.0);
    if params.look.w > 0.5 {
        x = pow(srgb_encode(x), 2.2);
    }
    let f = params.look.z;
    return max(x - f, 0.0) / (1.0 - f);
}

// An SDR grey (linear, 1 = paper white) as the frame's pixels are encoded.
fn grey(v: f32) -> vec3<f32> {
    return vec3<f32>(min(shape(v) * params.look.x, params.look.y));
}

// The grade's highlight gain for an SDR luminance (imagespace.wesl, its last step).
fn gain(l: f32) -> f32 {
    let knee = params.pattern.z;
    let t = clamp((l - knee) / max(1.0 - knee, 1e-3), 0.0, 1.0);
    return 1.0 + (params.pattern.y - 1.0) * t * t * params.pattern.w;
}

// The pattern at `p` (0..1 across the box, y down); `px` is the box's size in pixels.
fn pattern(p: vec2<f32>, px: vec2<f32>) -> vec3<f32> {
    // A frame of mid grey round the box, two pixels wide.
    let edge = 2.0 / px;
    if p.x < edge.x || p.y < edge.y || p.x > 1.0 - edge.x || p.y > 1.0 - edge.y {
        return grey(0.2);
    }
    let mode = i32(params.pattern.x + 0.5);
    if mode == 1 {
        // Peak: the box at the peak setting, a plus at 10,000 nits. Above the display's
        // brightest both clip alike and the plus is gone.
        let c = (p - vec2<f32>(0.5)) * px;
        let arm = min(px.x, px.y) * 0.3;
        let width = min(px.x, px.y) * 0.06;
        if (abs(c.x) < width && abs(c.y) < arm) || (abs(c.y) < width && abs(c.x) < arm) {
            return vec3<f32>(125.0);
        }
        return vec3<f32>(params.look.y);
    }
    if mode == 2 {
        // Shadows: eight patches at sRGB codes 1 to 15 on true black, encoded as the frame
        // is; the darkest should just vanish.
        let codes = array<f32, 8>(1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 11.0, 15.0);
        let cell = p.x * 8.0;
        let i = i32(floor(cell));
        let inside = fract(cell) > 0.15 && fract(cell) < 0.85 && p.y > 0.3 && p.y < 0.7;
        if inside && i >= 0 && i < 8 {
            return grey(srgb_decode(codes[i] / 255.0));
        }
        return vec3<f32>(0.0);
    }
    if mode == 3 {
        // Paper white: a ramp from black to white above, paper white below with a mid grey
        // patch (the menu's own white is paper white too).
        if p.y < 0.5 {
            return grey(srgb_decode(floor(p.x * 16.0) / 15.0));
        }
        if abs(p.x - 0.5) < 0.12 && abs(p.y - 0.75) < 0.12 {
            return grey(0.18);
        }
        return grey(1.0);
    }
    if mode == 4 {
        // Highlights: the SDR ramp above, the same with the grade's highlight gain below, a
        // mark where the knee is.
        let l = srgb_decode(p.x);
        if abs(l - params.pattern.z) < 0.004 && p.y > 0.45 && p.y < 0.55 {
            return vec3<f32>(params.look.x, 0.0, 0.0);
        }
        if p.y < 0.48 {
            return grey(l);
        }
        if p.y < 0.52 {
            return vec3<f32>(0.0);
        }
        return vec3<f32>(min(shape(l * gain(l)) * params.look.x, params.look.y));
    }
    return vec3<f32>(0.0);
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    let r = params.rect;
    if params.pattern.x > 0.5 && in.uv.x >= r.x && in.uv.x <= r.z && in.uv.y >= r.y && in.uv.y <= r.w {
        let size = vec2<f32>(textureDimensions(frame)) * (r.zw - r.xy);
        return vec4<f32>(pattern((in.uv - r.xy) / (r.zw - r.xy), size), 1.0);
    }
    let c = textureSampleLevel(frame, nearest, in.uv, 0.0);
    let shaped = vec3<f32>(shape(c.r), shape(c.g), shape(c.b));
    return vec4<f32>(min(shaped * params.look.x, vec3<f32>(params.look.y)), 1.0);
}
