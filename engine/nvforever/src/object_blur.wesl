// Object motion blur (object_blur.rs; docs/design/modern-feel-gfx.md §2): `velocity` finds
// each pixel's blur from the motion-vector prepass where the pixel moves in the world,
// `tile_max` (across, then down) and `neighbour_max` the largest blur around each 16 x 16 tile, `gather` the
// reconstruction filter of McGuire et al. 2012 (I3D, "A Reconstruction Filter for
// Plausible Motion Blur"). NVForever's own design; every constant tuned by eye.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct Blur {
    // This frame's clip space to the last frame's (unjittered).
    reproject: mat4x4<f32>,
    // x: exposure in frames (0: none), y: the reverse depth nearer than which nothing
    // blurs, z: largest radius (pixels), w: near plane.
    params: vec4<f32>,
    // xy: the motion vectors' region (texels), zw: the picture's size (pixels).
    sizes: vec4<f32>,
}

@group(0) @binding(0) var<uniform> blur: Blur;

@if(VELOCITY && MULTISAMPLED) @group(0) @binding(1) var depth: texture_depth_multisampled_2d;
@if(VELOCITY && !MULTISAMPLED) @group(0) @binding(1) var depth: texture_depth_2d;
@if(VELOCITY && MULTISAMPLED) @group(0) @binding(2) var motion: texture_multisampled_2d<f32>;
@if(VELOCITY && !MULTISAMPLED) @group(0) @binding(2) var motion: texture_2d<f32>;

@if(TILE_MAX || NEIGHBOUR_MAX) @group(0) @binding(1) var source: texture_2d<f32>;

@if(GATHER) @group(0) @binding(1) var picture: texture_2d<f32>;
@if(GATHER) @group(0) @binding(2) var picture_sampler: sampler;
@if(GATHER) @group(0) @binding(3) var velocities: texture_2d<f32>;
@if(GATHER) @group(0) @binding(4) var neighbours: texture_2d<f32>;

const TILE: i32 = 16;
// A pixel blurs once it moves in the world by this many pixels over the exposure, fully
// from OWN_FULL [design: below it the wind in the grass and an idle actor's breathing].
const OWN_MIN: f32 = 0.6;
const OWN_FULL: f32 = 1.8;
// A move in the world of this share of the picture's height in one frame is a jump
// (a reference moved by a script, a ragdoll handed over): no blur [design].
const JUMP_FROM: f32 = 0.08;
const JUMP_TO: f32 = 0.15;
// Taps of the gather along the line [design].
const TAPS: i32 = 12;

// x, y: blur radius (pixels, its direction the motion's), z: distance from the eye
// (game units).
@if(VELOCITY) @fragment
fn velocity(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let ddims = vec2<i32>(textureDimensions(depth));
    let d = textureLoad(depth, clamp(vec2<i32>(uv * vec2<f32>(ddims)), vec2(0), ddims - 1), 0);
    let dist = min(blur.params.w / max(d, 1.0e-7), 1.0e7);
    if d > blur.params.y {
        return vec4(0.0, 0.0, dist, 0.0);
    }
    let region = vec2<i32>(blur.sizes.xy);
    let mv = textureLoad(motion, clamp(vec2<i32>(uv * blur.sizes.xy), vec2(0), region - 1), 0).xy;
    // Where a still point at this depth was last frame: the camera's own share.
    let before = blur.reproject * vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    var camera = mv;
    if before.w > 1.0e-6 {
        let ndc = before.xy / before.w;
        camera = uv - vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    }
    let px = blur.sizes.zw;
    let own = length((mv - camera) * px);
    let w = smoothstep(OWN_MIN, OWN_FULL, own * blur.params.x) * (1.0 - smoothstep(JUMP_FROM, JUMP_TO, own / px.y));
    var v = mv * px * (blur.params.x * 0.5 * w);
    let r = length(v);
    if r > blur.params.z {
        v *= blur.params.z / r;
    }
    return vec4(v, dist, 0.0);
}

// The longest blur along a row of TILE pixels (`tile_max` with TILE_ACROSS), then down a
// column of TILE of those (TILE_DOWN): the tile's longest blur, in two passes of 16 reads.
@if(TILE_MAX) @fragment
fn tile_max(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(source));
    let at = vec2<i32>(floor(in.position.xy));
    @if(TILE_ACROSS) let base = vec2(at.x * TILE, at.y);
    @if(TILE_ACROSS) let step = vec2(1, 0);
    @if(!TILE_ACROSS) let base = vec2(at.x, at.y * TILE);
    @if(!TILE_ACROSS) let step = vec2(0, 1);
    var best = vec2(0.0);
    var best_len = 0.0;
    for (var i = 0; i < TILE; i++) {
        let p = base + step * i;
        if p.x >= dims.x || p.y >= dims.y {
            break;
        }
        let v = textureLoad(source, p, 0).xy;
        let l = dot(v, v);
        if l > best_len {
            best_len = l;
            best = v;
        }
    }
    return vec4(best, 0.0, 0.0);
}

// The longest blur of the tile and its eight neighbours.
@if(NEIGHBOUR_MAX) @fragment
fn neighbour_max(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(source));
    let at = vec2<i32>(floor(in.position.xy));
    var best = vec2(0.0);
    var best_len = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let p = clamp(at + vec2(x, y), vec2(0), dims - 1);
            let v = textureLoad(source, p, 0).xy;
            let l = dot(v, v);
            if l > best_len {
                best_len = l;
                best = v;
            }
        }
    }
    return vec4(best, 0.0, 0.0);
}

// Interleaved gradient noise (Jimenez 2014), fixed per pixel.
fn noise(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}

fn cone(d: f32, r: f32) -> f32 {
    return clamp(1.0 - d / r, 0.0, 1.0);
}

fn cylinder(d: f32, r: f32) -> f32 {
    return 1.0 - smoothstep(0.95 * r, 1.05 * r, d);
}

// 1 when distance `a` lies in front of `b`, falling to 0 over a soft depth range.
fn in_front(a: f32, b: f32) -> f32 {
    let soft = max(0.04 * min(a, b), 6.0);
    return clamp(1.0 - (a - b) / soft, 0.0, 1.0);
}

@if(GATHER) @fragment
fn gather(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let centre = textureSampleLevel(picture, picture_sampler, uv, 0.0);
    let vdims = vec2<f32>(textureDimensions(velocities));
    let tdims = vec2<i32>(textureDimensions(neighbours));
    let pix = uv * vdims;
    let vn = textureLoad(neighbours, clamp(vec2<i32>(pix) / TILE, vec2(0), tdims - 1), 0).xy;
    let rn = length(vn);
    if rn < 0.5 {
        return centre;
    }
    let vmax = vec2<i32>(vdims) - 1;
    let x = textureLoad(velocities, clamp(vec2<i32>(pix), vec2(0), vmax), 0);
    let rx = max(length(x.xy), 0.5);
    let j = noise(in.position.xy) - 0.5;
    var total = 1.0 / rx;
    var sum = centre.rgb * total;
    for (var i = 0; i < TAPS; i++) {
        let t = mix(-1.0, 1.0, (f32(i) + 0.5 + j) / f32(TAPS));
        let at = pix + vn * t;
        let y = textureLoad(velocities, clamp(vec2<i32>(at), vec2(0), vmax), 0);
        let ry = max(length(y.xy), 0.5);
        let d = abs(t) * rn;
        let front = in_front(y.z, x.z);
        let back = in_front(x.z, y.z);
        let a = front * cone(d, ry) + back * cone(d, rx) + cylinder(d, ry) * cylinder(d, rx) * 2.0;
        sum += a * textureSampleLevel(picture, picture_sampler, at / vdims, 0.0).rgb;
        total += a;
    }
    return vec4(sum / total, centre.a);
}
