// Sun shafts of the enhanced graphics (shafts.rs; docs/design/graphics-enhanced.md §3.7):
// `source` keeps the sky near the sun (where the depth prepass has only the sky dome) at a
// quarter of the size; `radial_fine` and `radial_coarse` blur it along the lines towards
// the sun (24 taps over 1/24 of the length, then 24 taps over the whole length: about
// 576 taps in all); `composite` adds it to the picture. Engineering choices, not the
// original game's.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct Shafts {
    // x, y: the sun on the screen (uv), z: strength, w: width / height.
    sun: vec4<f32>,
    // rgb: tint, w: length (share of the way to the sun).
    tint: vec4<f32>,
    // x: source radius (share of the picture's height), y: distance beyond which the depth
    // is the sky dome's, z: near plane.
    source: vec4<f32>,
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;
@group(0) @binding(2) var<uniform> shafts: Shafts;
// `source` (shader def SOURCE): the depth prepass; `composite`: the shafts.
@if(SOURCE && MULTISAMPLED) @group(0) @binding(3) var depth: texture_depth_multisampled_2d;
@if(SOURCE && !MULTISAMPLED) @group(0) @binding(3) var depth: texture_depth_2d;
@if(!SOURCE) @group(0) @binding(3) var rays: texture_2d<f32>;

const TAPS: i32 = 24;

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3(0.2126, 0.7152, 0.0722));
}

// Maximum Reverse-Z depth across MSAA samples so partial sky coverage on foreground
// silhouettes (such as the gun edge against the sun) does not leak into the shaft source.
@if(SOURCE && MULTISAMPLED)
fn sky_depth_at(q: vec2<i32>) -> f32 {
    let n = i32(textureNumSamples(depth));
    var d = 0.0;
    for (var s = 0; s < n; s++) {
        d = max(d, textureLoad(depth, q, s));
    }
    return d;
}

@if(SOURCE && !MULTISAMPLED)
fn sky_depth_at(q: vec2<i32>) -> f32 {
    return textureLoad(depth, q, 0);
}

@if(SOURCE) @fragment
fn source(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // Four pixels of the 4 x 4 under this one: sky only, the brighter part of it (linear
    // values: the picture's texture is sRGB).
    let dims = vec2<i32>(textureDimensions(depth));
    let base = vec2<i32>(floor(in.position.xy)) * 4;
    var c = vec3(0.0);
    for (var i = 0; i < 4; i++) {
        let q = clamp(base + vec2(1 + 2 * (i & 1), 1 + 2 * (i >> 1u)), vec2(0), dims - 1);
        let d = sky_depth_at(q);
        if d <= 0.0 || shafts.source.z / d > shafts.source.y {
            let s = textureLoad(picture, q, 0).rgb;
            c += s * smoothstep(0.04, 0.4, luminance(s));
        }
    }
    c *= 0.25;
    let d = length((in.uv - shafts.sun.xy) * vec2(shafts.sun.w, 1.0)) / shafts.source.x;
    let near_sun = clamp(1.0 - d, 0.0, 1.0);
    return vec4(c * near_sun * near_sun, 1.0);
}

// `TAPS` taps from `uv` towards the sun over `span` of the way, each weighted by `decay`
// to the power of its step; taps off the picture count as dark.
@if(!SOURCE) fn radial(uv: vec2<f32>, span: f32, decay: f32) -> vec4<f32> {
    let stride = (shafts.sun.xy - uv) * span / f32(TAPS);
    var c = vec3(0.0);
    var w = 1.0;
    var total = 0.0;
    var p = uv;
    for (var i = 0; i < TAPS; i++) {
        let inside = all(p >= vec2(0.0)) && all(p <= vec2(1.0));
        c += select(vec3(0.0), textureSampleLevel(picture, picture_sampler, p, 0.0).rgb, inside) * w;
        total += w;
        w *= decay;
        p += stride;
    }
    return vec4(c / total, 1.0);
}

@if(!SOURCE) @fragment
fn radial_fine(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return radial(in.uv, shafts.tint.w / f32(TAPS), 1.0);
}

@if(!SOURCE) @fragment
fn radial_coarse(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return radial(in.uv, shafts.tint.w, 0.97);
}

@if(!SOURCE) @fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.position.xy));
    let src = textureLoad(picture, p, 0);
    let add = textureSampleLevel(rays, picture_sampler, in.uv, 0.0).rgb * shafts.tint.rgb * shafts.sun.z;
    // Added as light, kept below white without a hard clip.
    let c = src.rgb + add * (1.0 - src.rgb * 0.5);
    return vec4(c, src.a);
}
