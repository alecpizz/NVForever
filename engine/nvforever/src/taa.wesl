// Temporal anti-aliasing of the enhanced graphics (taa.rs; docs/design/graphics-enhanced.md
// §3.6): this frame (jittered) blended into the history of the frames before, found where
// the camera's move put each pixel's point (from the depth prepass; nothing here knows
// what moves by itself). The history is clipped to the colours around the pixel this
// frame (variance clipping in YCoCg), sampled with a Catmull-Rom filter so it does not
// blur over the frames. Writes the picture and the next history. Engineering choices, not
// the original game's.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct Taa {
    // This frame's clip space (unjittered) to the last frame's.
    reproject: mat4x4<f32>,
    // x: near plane, y: distance below which points move with the camera, z: 1 when the
    // history starts over, w: unused.
    params: vec4<f32>,
    // x: share of this frame standing still, y: moving fast, z: "fast" in pixels a frame.
    blend: vec4<f32>,
}

@group(0) @binding(0) var current: texture_2d<f32>;
@group(0) @binding(1) var history: texture_2d<f32>;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var linear_sampler: sampler;
@group(0) @binding(4) var<uniform> taa: Taa;
// The motion-vector prepass, when the camera has it (Modern Feel's object motion blur,
// object_blur.rs): the history is then found by each pixel's own motion, so what moves by
// itself (actors, plants in the wind) keeps its smoothing and leaves no trail.
@if(MOTION_VECTORS) @group(0) @binding(5) var motion: texture_2d<f32>;

struct Out {
    @location(0) picture: vec4<f32>,
    @location(1) history: vec4<f32>,
}

fn rgb_to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3(0.25 * c.r + 0.5 * c.g + 0.25 * c.b, 0.5 * c.r - 0.5 * c.b, -0.25 * c.r + 0.5 * c.g - 0.25 * c.b);
}

fn ycocg_to_rgb(c: vec3<f32>) -> vec3<f32> {
    return vec3(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z);
}

fn history_at(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(history, linear_sampler, uv, 0.0).rgb;
}

// The history at `uv` through a Catmull-Rom filter from five bilinear taps (the corner
// taps' weights are small and left out).
fn history_catmull_rom(uv: vec2<f32>, size: vec2<f32>) -> vec3<f32> {
    let pos = uv * size;
    let p1 = floor(pos - 0.5) + 0.5;
    let f = pos - p1;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let p0 = (p1 - 1.0) / size;
    let p3 = (p1 + 2.0) / size;
    let p12 = (p1 + w2 / w12) / size;
    var c = history_at(vec2(p12.x, p0.y)) * (w12.x * w0.y);
    c += history_at(vec2(p0.x, p12.y)) * (w0.x * w12.y);
    c += history_at(p12) * (w12.x * w12.y);
    c += history_at(vec2(p3.x, p12.y)) * (w3.x * w12.y);
    c += history_at(vec2(p12.x, p3.y)) * (w12.x * w3.y);
    let w = w12.x * w0.y + w0.x * w12.y + w12.x * w12.y + w3.x * w12.y + w12.x * w3.y;
    return max(c / w, vec3(0.0));
}

// `h` moved towards the box's centre until it lies inside the box.
fn clip_to_box(h: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>) -> vec3<f32> {
    let centre = 0.5 * (hi + lo);
    let extent = 0.5 * (hi - lo) + vec3(1e-5);
    let v = h - centre;
    let a = abs(v / extent);
    let m = max(a.x, max(a.y, a.z));
    return select(h, centre + v / m, m > 1.0);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> Out {
    let dims = vec2<i32>(textureDimensions(current));
    let size = vec2<f32>(dims);
    let p = vec2<i32>(floor(in.position.xy));
    let here = textureLoad(current, p, 0);

    // Points nearer than params.y move with the camera (the first-person arms and weapon).
    let own = textureLoad(depth, p, 0);
    let locked = own > 0.0 && taa.params.x / own < taa.params.y;

    // The 3 x 3 neighbourhood: colour moments in YCoCg, and the nearest depth (reverse z:
    // the largest value) among the pixels on the same side of the camera-locked distance,
    // whose point decides where the pixel was (edges keep to the object in front; the
    // world around the arms does not take their motion, which left a dark rim).
    var m1 = vec3(0.0);
    var m2 = vec3(0.0);
    var lo = vec3(1e9);
    var hi = vec3(-1e9);
    var nearest = select(0.0, own, !locked);
    var nearest_at = p;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let q = clamp(p + vec2(x, y), vec2(0), dims - 1);
            let c = rgb_to_ycocg(textureLoad(current, q, 0).rgb);
            m1 += c;
            m2 += c * c;
            lo = min(lo, c);
            hi = max(hi, c);
            let d = textureLoad(depth, q, 0);
            let same_side = (d > 0.0 && taa.params.x / d < taa.params.y) == locked;
            if same_side && d > nearest {
                nearest = d;
                nearest_at = q;
            }
        }
    }
    let mean = m1 / 9.0;
    let sigma = sqrt(max(m2 / 9.0 - mean * mean, vec3(0.0)));
    let box_lo = max(lo, mean - sigma * 1.25);
    let box_hi = min(hi, mean + sigma * 1.25);

    // Where this pixel's point was on the screen last frame.
    let uv = (vec2<f32>(p) + 0.5) / size;
    let at = (vec2<f32>(nearest_at) + 0.5) / size;
    let ndc = vec4(at.x * 2.0 - 1.0, 1.0 - at.y * 2.0, nearest, 1.0);
    let prev_clip = taa.reproject * ndc;
    var prev_uv = uv + (vec2(prev_clip.x, -prev_clip.y) / max(prev_clip.w, 1e-7) * 0.5 + 0.5 - at);
    var reset = taa.params.z > 0.5 || prev_clip.w <= 0.0;
    @if(MOTION_VECTORS) if !locked {
        prev_uv = uv - textureLoad(motion, nearest_at, 0).xy;
    }
    // The first-person arms and weapon move with the camera.
    if locked {
        prev_uv = uv;
        reset = taa.params.z > 0.5;
    }
    if any(prev_uv < vec2(0.0)) || any(prev_uv > vec2(1.0)) {
        reset = true;
    }

    var colour = here.rgb;
    if !reset {
        let h = ycocg_to_rgb(clip_to_box(rgb_to_ycocg(history_catmull_rom(prev_uv, size)), box_lo, box_hi));
        let moved = length((prev_uv - uv) * size);
        let a = mix(taa.blend.x, taa.blend.y, clamp(moved / taa.blend.z, 0.0, 1.0));
        // Weighted by inverse brightness so a single bright frame does not flicker.
        let wc = a / (1.0 + dot(here.rgb, vec3(0.2126, 0.7152, 0.0722)));
        let wh = (1.0 - a) / (1.0 + dot(h, vec3(0.2126, 0.7152, 0.0722)));
        colour = (here.rgb * wc + h * wh) / (wc + wh);
    }
    var out: Out;
    out.picture = vec4(colour, here.a);
    out.history = vec4(colour, 1.0);
    return out;
}
