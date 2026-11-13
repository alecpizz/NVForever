// Screen-space global illumination of the remastered look (gi.rs, docs/design/
// graphics-remaster.md). An engineering choice, not the original game's.
//
// Entries, in the order they run each frame:
// - `trace` (half resolution, after the depth prepass): for each 2 x 2 block's top-left
//   pixel, two slices of the hemisphere above it are marched in screen space; each slice
//   keeps a 32-sector bitmask of the angles the depth buffer closes (each sample a surface
//   of `thickness` behind what the depth shows, so thin things do not close the whole
//   horizon behind them: "screen space indirect lighting with visibility bitmask"). The
//   sectors a sample closes first take its light from last frame's picture (`history`,
//   reprojected), weighed by the cosine to the normal; the share of sectors left open is
//   how open the pixel is to the ambient light.
// - `temporal`: that blended with last frame's result at the same surface (90 % old while
//   the distance agrees), so 2 slices a frame grow to about 20.
// - `upsample` (full resolution): the 3 x 3 half-resolution texels around each pixel,
//   weighed by how near their distance is to the pixel's, into the texture the lighting
//   reads (bevy_pbr ScreenIndirectLight, lights.rs `nvf_open`, `nvf_bounce`).
// - `capture` (half resolution, after the main passes, before tonemapping): the HDR picture
//   averaged over each 2 x 2 block, with the block's distance, as next frame's `history`.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import nvf_assets::first_person_depth::unsqueeze;

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

@if(MULTISAMPLED) @group(0) @binding(0) var depth: texture_depth_multisampled_2d;
@if(!MULTISAMPLED) @group(0) @binding(0) var depth: texture_depth_2d;
@group(0) @binding(1) var<uniform> gi: Gi;
// trace: last frame's picture (rgb) and distance (a), half resolution.
// temporal: this frame's trace. upsample: the accumulated result. capture: the HDR picture.
@group(0) @binding(2) var input_a: texture_2d<f32>;
@group(0) @binding(3) var linear_sampler: sampler;
// temporal: last frame's accumulated result. upsample: this frame's distances.
@group(0) @binding(4) var input_b: texture_2d<f32>;
// temporal: last frame's distances (history alpha) and this frame's (r).
@group(0) @binding(5) var input_c: texture_2d<f32>;
@group(0) @binding(6) var input_d: texture_2d<f32>;
// The light probe's faces (probe.rs FACES order) and their mean light (6 x 1).
@group(0) @binding(7) var probe_faces: texture_2d_array<f32>;
@group(0) @binding(8) var probe_cube: texture_2d<f32>;
// The height field (probe.rs): heights (world y) and the lit colour seen from above.
@group(0) @binding(9) var hf_height: texture_2d<f32>;
@group(0) @binding(10) var hf_colour: texture_2d<f32>;

// The height field's texel under world (x, z) `p`, and whether it lies inside.
fn hf_texel(p: vec2<f32>) -> vec3<i32> {
    // The camera looks down with -z up: its picture's right is +x, its top is -z.
    let uv = (p - gi.hf.xy) / gi.hf.z + 0.5;
    let inside = all(uv >= vec2(0.0)) && all(uv < vec2(1.0));
    let dims = vec2<i32>(textureDimensions(hf_height));
    return vec3<i32>(vec2<i32>(uv * vec2<f32>(dims)), select(0, 1, inside));
}

// World rays through the height field (graphics-remaster.md §2 "Height field"): `count`
// cosine-distributed rays from world point `p` with normal `n`, each marched in growing
// steps out to about 2,700 units; a ray under the field takes the colour seen from above
// there (the lit top of what it hit), one that gets away is open sky. Returns the light
// brought back (rgb) and the share of rays shut in (a).
fn world_rays(p: vec3<f32>, n: vec3<f32>, noise_a: f32, noise_b: f32) -> vec4<f32> {
    let up = select(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), abs(n.y) > 0.9);
    let tx = normalize(cross(up, n));
    let ty = cross(n, tx);
    var light = vec3(0.0);
    var shut = 0.0;
    let count = 3u;
    for (var r = 0u; r < count; r++) {
        let u1 = fract(noise_a + f32(r) / f32(count));
        let u2 = fract(noise_b + f32(r) * 0.618034);
        let phi = 6.2831853 * u1;
        let sr = sqrt(u2);
        let dir = normalize(tx * (cos(phi) * sr) + ty * (sin(phi) * sr) + n * sqrt(max(1.0 - u2, 0.0)));
        let start = p + n * 40.0;
        var t = 60.0;
        for (var s = 0u; s < 10u; s++) {
            let q = start + dir * t;
            let c = hf_texel(q.xz);
            if c.z == 0 {
                break;
            }
            let h = textureLoad(hf_height, c.xy, 0).r;
            // Well under the field: grass and bushes (about 50 units) seen from above do
            // not cover their own bases.
            if q.y < h - 60.0 {
                light += min(textureLoad(hf_colour, c.xy, 0).rgb, vec3(gi.params2.z));
                shut += 1.0;
                break;
            }
            t *= 1.55;
        }
    }
    return vec4(light, shut) / f32(count);
}

const PI: f32 = 3.14159265;
const HALF_PI: f32 = 1.5707963;
const SLICES: u32 = 2u;
const STEPS: u32 = 8u;
// The far cascade: more steps beyond the near reach, out to FAR times it, light only (a
// screen-space take on radiance cascades: the near cascade fine and short, the far one
// coarse and long, so a lit sign down the street still lights the pavement).
const FAR_STEPS: u32 = 4u;
const FAR: f32 = 8.0;
// Distance written for the sky (depth 0) and the most a texel holds.
const SKY: f32 = 60000.0;

fn full_dims() -> vec2<i32> {
    return vec2<i32>(textureDimensions(depth));
}

fn depth_at(p: vec2<i32>) -> f32 {
    return unsqueeze(textureLoad(depth, clamp(p, vec2(0), full_dims() - 1), 0));
}

// View-space position of the picture's point `p` (pixels) at depth `d` (reverse
// infinite z: distance = near / d), looking down -z.
fn view_pos(p: vec2<f32>, d: f32) -> vec3<f32> {
    let size = gi.size.xy;
    let ndc = vec2(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0);
    let z = gi.proj.z / max(d, 1e-7);
    return vec3(ndc.x * z / gi.proj.x, ndc.y * z / gi.proj.y, -z);
}

fn pos_at(px: vec2<i32>) -> vec3<f32> {
    return view_pos(vec2<f32>(px) + 0.5, depth_at(px));
}

// Normal from the neighbouring depths, on each axis the side with the smaller step.
fn normal_at(px: vec2<i32>, p: vec3<f32>) -> vec3<f32> {
    let l = pos_at(px - vec2(1, 0));
    let r = pos_at(px + vec2(1, 0));
    let u = pos_at(px - vec2(0, 1));
    let b = pos_at(px + vec2(0, 1));
    let dx = select(p - l, r - p, abs(r.z - p.z) < abs(p.z - l.z));
    let dy = select(p - u, b - p, abs(b.z - p.z) < abs(p.z - u.z));
    var n = normalize(cross(dx, dy));
    if dot(n, -p) < 0.0 {
        n = -n;
    }
    return n;
}

// Where view-space point `p` was on the screen last frame (0..1), and its distance then.
fn prev_uv(p: vec3<f32>) -> vec3<f32> {
    let q = (gi.prev_view_from_view * vec4(p, 1.0)).xyz;
    let z = max(-q.z, 1e-3);
    let ndc = vec2(q.x * gi.proj.x / z, q.y * gi.proj.y / z);
    return vec3(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, -q.z);
}

fn inside(uv: vec2<f32>) -> bool {
    return all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
}

// Interleaved gradient noise (Jimenez 2014), turned each frame.
fn ign(p: vec2<f32>, frame: f32) -> f32 {
    let q = p + 5.588238 * (frame % 64.0);
    return fract(52.9829189 * fract(dot(q, vec2(0.06711056, 0.00583715))));
}

fn sector_mask(t0: f32, t1: f32) -> u32 {
    let a = clamp(t0, 0.0, 1.0);
    let b = clamp(t1, 0.0, 1.0);
    let start = u32(floor(min(a, b) * 32.0));
    let count = u32(ceil((max(a, b) - min(a, b)) * 32.0));
    if count == 0u || start >= 32u {
        return 0u;
    }
    let bits = select(0xFFFFFFFFu >> (32u - min(count, 32u)), 0xFFFFFFFFu, count >= 32u);
    return bits << start;
}

struct TraceOut {
    @location(0) light: vec4<f32>,
    @location(1) dist: vec4<f32>,
}

@fragment
fn trace(in: FullscreenVertexOutput) -> TraceOut {
    var out: TraceOut;
    let half = vec2<i32>(floor(in.position.xy));
    let px = half * 2;
    let d = depth_at(px);
    let p = view_pos(vec2<f32>(px) + 0.5, d);
    let z = -p.z;
    out.dist = vec4(min(z, SKY), 0.0, 0.0, 0.0);
    out.light = vec4(0.0);
    if d <= 0.0 || z >= gi.params.w {
        return out;
    }
    let n = normal_at(px, p);
    let v = normalize(-p);
    let frame = gi.proj.w;
    let noise_a = ign(vec2<f32>(half), frame);
    let noise_b = ign(vec2<f32>(half) + vec2(17.0, 59.0), frame + 7.0);
    // The radius on the screen, in the picture's pixels.
    let radius_px = clamp(gi.params.x * gi.proj.y * 0.5 * gi.size.y / z, f32(STEPS), 0.3 * gi.size.y);
    let thickness = gi.params.y * max(1.0, z / 1500.0);
    let far_px = min(radius_px * FAR, 0.45 * gi.size.y);
    var light = vec3(0.0);
    var closed = 0.0;
    for (var s = 0u; s < SLICES; s++) {
        let phi = (f32(s) + noise_a) * PI / f32(SLICES);
        let dir = vec2(cos(phi), sin(phi));
        let dir_v = vec3(dir.x, -dir.y, 0.0);
        let ortho = dir_v - dot(dir_v, v) * v;
        let axis = normalize(cross(ortho, v));
        let proj_n = n - axis * dot(n, axis);
        let proj_len = max(length(proj_n), 1e-4);
        let sign_n = select(-1.0, 1.0, dot(ortho, proj_n) >= 0.0);
        let n_angle = sign_n * acos(clamp(dot(proj_n, v) / proj_len, -1.0, 1.0));
        var mask = 0u;
        var far_mask = 0u;
        for (var side = 0u; side < 2u; side++) {
            let sgn = select(-1.0, 1.0, side == 0u);
            for (var i = 0u; i < STEPS + FAR_STEPS; i++) {
                let far = i >= STEPS;
                var off: f32;
                if far {
                    var t = (f32(i - STEPS) + fract(noise_b + f32(i) * 0.618034)) / f32(FAR_STEPS);
                    off = radius_px + t * t * max(far_px - radius_px, 0.0);
                } else {
                    var t = (f32(i) + fract(noise_b + f32(i) * 0.618034)) / f32(STEPS);
                    t = t * t;
                    off = max(t * radius_px, f32(i) + 1.5);
                }
                let sp = vec2<f32>(px) + 0.5 + sgn * dir * off;
                if any(sp < vec2(0.0)) || any(sp >= gi.size.xy) {
                    break;
                }
                let spx = vec2<i32>(floor(sp));
                let sd = depth_at(spx);
                if sd <= 0.0 {
                    continue;
                }
                let q = view_pos(sp, sd);
                let delta = q - p;
                let len2 = dot(delta, delta);
                if len2 < 1e-4 {
                    continue;
                }
                let front = delta * inverseSqrt(len2);
                let back = normalize(delta - v * select(thickness, thickness * 4.0, far));
                let a_front = sgn * acos(clamp(dot(front, v), -1.0, 1.0));
                let a_back = sgn * acos(clamp(dot(back, v), -1.0, 1.0));
                // Sectors equal in projected solid angle (cosine-weighted): sin of the
                // angle from the normal, so the share of sectors is the share of the
                // irradiance a uniform light would give.
                let t_front = 0.5 + 0.5 * sin(clamp(a_front - n_angle, -HALF_PI, HALF_PI));
                let t_back = 0.5 + 0.5 * sin(clamp(a_back - n_angle, -HALF_PI, HALF_PI));
                let m = sector_mask(t_front, t_back);
                // The far cascade lights only sectors nothing nearer has closed, and leaves
                // the occlusion to the near one (no darkening of whole valleys).
                let fresh = m & ~(mask | far_mask);
                if fresh != 0u && gi.params2.w > 0.5 {
                    if dot(n, front) > 0.0 {
                        let h = prev_uv(q);
                        if inside(h.xy) {
                            let hist = textureSampleLevel(input_a, linear_sampler, h.xy, 0.0);
                            // Only light from the same surface last frame (not what moved in front).
                            if abs(hist.a - h.z) < 0.1 * h.z + 20.0 {
                                let falloff = select(1.0 - smoothstep(0.6, 1.0, sqrt(len2) / (gi.params.x * 1.5)), 1.0, far);
                                light += min(hist.rgb, vec3(gi.params2.z)) * (f32(countOneBits(fresh)) / 32.0) * falloff;
                            }
                        }
                    }
                }
                if far {
                    far_mask |= m;
                } else {
                    mask |= m;
                }
            }
        }
        closed += f32(countOneBits(mask)) / 32.0;
    }
    let fade = 1.0 - smoothstep(gi.params.w * 0.6, gi.params.w, z);
    var occlusion = min(saturate(closed / f32(SLICES)), gi.params2.x) * fade;
    var bounce = light * (gi.params.z / f32(SLICES)) * fade;
    // World rays through the height field: what the screen does not hold (behind the
    // camera, under eaves, the far side of a canyon) shuts out the sky and bounces light.
    if gi.hf.w > 0.5 {
        let pw = (gi.world_from_view * vec4(p, 0.0)).xyz + gi.cam_world.xyz;
        let nw = normalize((gi.world_from_view * vec4(n, 0.0)).xyz);
        let wr = world_rays(pw, nw, noise_a, noise_b);
        bounce += wr.rgb * gi.hf2.z;
        occlusion = max(occlusion, wr.a * gi.hf2.w);
    }
    out.light = vec4(bounce, occlusion);
    return out;
}

@fragment
fn temporal(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let half = vec2<i32>(floor(in.position.xy));
    let current = textureLoad(input_a, half, 0);
    let z = textureLoad(input_c, half, 0).r;
    if gi.params2.w < 0.5 || z >= SKY {
        return current;
    }
    let px = half * 2;
    let p = view_pos(vec2<f32>(px) + 0.5, gi.proj.z / z);
    let h = prev_uv(p);
    if !inside(h.xy) {
        return current;
    }
    // Last frame's distance at that spot (history alpha) against this point's then.
    let then = textureSampleLevel(input_d, linear_sampler, h.xy, 0.0).a;
    if abs(then - h.z) > 0.05 * h.z + 10.0 {
        return current;
    }
    let old = textureSampleLevel(input_b, linear_sampler, h.xy, 0.0);
    return mix(old, current, gi.params2.y);
}

@fragment
fn upsample(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    if gi.debug.x > 0.5 {
        let hp = vec2<i32>(floor(in.position.xy * 0.5));
        let acc = textureLoad(input_a, hp, 0);
        let dd = textureLoad(input_b, hp, 0).r;
        if gi.debug.x < 1.5 { return vec4(vec3(dd / 3000.0), -1.0); }
        if gi.debug.x < 2.5 { return vec4(vec3(1.0 - acc.a), -1.0); }
        if gi.debug.x > 5.5 {
            // Modes 6 and 7: the height field under this pixel: its colour, and its height
            // above the pixel (red above, green below, 500 units full).
            let d6 = depth_at(vec2<i32>(floor(in.position.xy)));
            let p6 = view_pos(floor(in.position.xy) + 0.5, d6);
            let pw6 = (gi.world_from_view * vec4(p6, 0.0)).xyz + gi.cam_world.xyz;
            let c6 = hf_texel(pw6.xz);
            if c6.z == 0 || gi.hf.w < 0.5 { return vec4(0.0, 0.0, 1.0, -1.0); }
            if gi.debug.x < 6.5 { return vec4(textureLoad(hf_colour, c6.xy, 0).rgb, -1.0); }
            let dh = textureLoad(hf_height, c6.xy, 0).r - pw6.y;
            return vec4(saturate(dh / 500.0), saturate(-dh / 500.0), 0.0, -1.0);
        }
        if gi.debug.x > 3.5 {
            let g = textureSampleLevel(input_c, linear_sampler, in.position.xy / gi.size.xy, 0.0).rgb;
            // Mode 5: last frame's picture itself (its brightest values show the scale).
            if gi.debug.x > 4.5 { return vec4(textureSampleLevel(input_d, linear_sampler, in.position.xy / gi.size.xy, 0.0).rgb * gi.debug.y, -1.0); }
            return vec4(g * gi.debug.y, -1.0);
        }
        return vec4(acc.rgb * gi.debug.y, -1.0);
    }
    let px = vec2<i32>(floor(in.position.xy));
    let d = depth_at(px);
    if d <= 0.0 {
        return vec4(0.0);
    }
    let z = gi.proj.z / d;
    let half_dims = vec2<i32>(textureDimensions(input_a));
    let c = vec2<i32>(floor(vec2<f32>(px) * 0.5));
    var sum = vec4(0.0);
    var wsum = 0.0;
    var nearest = vec4(0.0);
    var nearest_dz = 1e9;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let t = clamp(c + vec2(x, y), vec2(0), half_dims - 1);
            let tz = textureLoad(input_b, t, 0).r;
            let v = textureLoad(input_a, t, 0);
            let dz = abs(tz - z);
            let spatial = select(0.5, 1.0, x == 0 && y == 0);
            let w = spatial * exp(-dz / (0.03 * z + 4.0));
            sum += v * w;
            wsum += w;
            if dz < nearest_dz {
                nearest_dz = dz;
                nearest = v;
            }
        }
    }
    var v = select(sum / max(wsum, 1e-4), nearest, wsum < 1e-3);
    // The light probe's light by the surface's world normal, where it is open.
    if gi.probe.x > 0.0 && z < gi.params.w {
        let p = view_pos(vec2<f32>(px) + 0.5, d);
        let n = normalize((gi.world_from_view * vec4(normal_at(px, p), 0.0)).xyz);
        v = vec4(v.rgb + probe_light(n) * gi.probe.x * (1.0 - max(v.a, 0.0)), v.a);
    }
    // The glow field (glow_*), faded out with distance: light from signs and lamps on
    // what is within a few dozen metres.
    let glow = textureSampleLevel(input_c, linear_sampler, (vec2<f32>(px) + 0.5) / gi.size.xy, 0.0).rgb;
    v = vec4(v.rgb + glow * gi.debug.w * (1.0 - smoothstep(3000.0, 9000.0, z)), v.a);
    // Indoors a little light fills what is open (dim rooms keep their detail; gi.rs
    // INTERIOR_FILL).
    v = vec4(v.rgb + vec3(gi.debug.z) * (1.0 - v.a), v.a);
    return v;
}

// The light probe: each face's mean light (texel i of a 6 x 1 target), from 8 x 8 taps.
@fragment
fn probe_reduce(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let face = i32(floor(in.position.x));
    var c = vec3(0.0);
    for (var y = 0; y < 8; y++) {
        for (var x = 0; x < 8; x++) {
            let uv = (vec2(f32(x), f32(y)) + 0.5) / 8.0;
            c += textureSampleLevel(probe_faces, linear_sampler, uv, face, 0.0).rgb;
        }
    }
    return vec4(min(c / 64.0, vec3(gi.params2.z)), 1.0);
}

// The probe's light on a surface facing world normal `n`: the faces' mean light weighed
// by the squared cosine to each face's direction (an "ambient cube"); the face looking up
// is left out, the game's ambient light already being the sky's.
fn probe_light(n: vec3<f32>) -> vec3<f32> {
    let w_pos = max(n, vec3(0.0));
    let w_neg = max(-n, vec3(0.0));
    let w = vec3(w_pos.x * w_pos.x, w_neg.x * w_neg.x, 0.0);
    var c = textureLoad(probe_cube, vec2(0, 0), 0).rgb * (w_pos.x * w_pos.x)
        + textureLoad(probe_cube, vec2(1, 0), 0).rgb * (w_neg.x * w_neg.x)
        + textureLoad(probe_cube, vec2(3, 0), 0).rgb * (w_neg.y * w_neg.y)
        + textureLoad(probe_cube, vec2(4, 0), 0).rgb * (w_pos.z * w_pos.z)
        + textureLoad(probe_cube, vec2(5, 0), 0).rgb * (w_neg.z * w_neg.z);
    let _unused = w;
    return c;
}

// The emissive glow field (gi.rs, third tier): last frame's light brighter than
// GLOW_FLOOR (neon, bulbs, fire; never the sky) at an eighth of the half resolution,
// then blurred wide, so a sign lights what is around it whatever the march found.
const GLOW_FLOOR: f32 = 2.5;

@fragment
fn glow_down(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let base = vec2<i32>(floor(in.position.xy)) * 8;
    let dims = vec2<i32>(textureDimensions(input_a));
    var c = vec3(0.0);
    for (var y = 0; y < 8; y += 2) {
        for (var x = 0; x < 8; x += 2) {
            let h = textureLoad(input_a, min(base + vec2(x, y), dims - 1), 0);
            if h.a < SKY * 0.99 {
                let l = max(h.r, max(h.g, h.b));
                c += h.rgb * smoothstep(GLOW_FLOOR * 0.6, GLOW_FLOOR * 1.6, l);
            }
        }
    }
    return vec4(min(c / 16.0, vec3(gi.params2.z)), 1.0);
}

fn glow_blur(uv: vec2<f32>, dir: vec2<f32>) -> vec4<f32> {
    let w = array<f32, 5>(0.2270, 0.1945, 0.1216, 0.0540, 0.0162);
    let texel = dir * 4.0 / vec2<f32>(textureDimensions(input_a));
    var c = textureSampleLevel(input_a, linear_sampler, uv, 0.0).rgb * w[0];
    for (var i = 1; i < 5; i++) {
        c += (textureSampleLevel(input_a, linear_sampler, uv + texel * f32(i), 0.0).rgb + textureSampleLevel(input_a, linear_sampler, uv - texel * f32(i), 0.0).rgb) * w[i];
    }
    return vec4(c, 1.0);
}

@fragment
fn glow_across(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return glow_blur(in.uv, vec2(1.0, 0.0));
}

@fragment
fn glow_down_blur(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return glow_blur(in.uv, vec2(0.0, 1.0));
}

@fragment
fn capture(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let half = vec2<i32>(floor(in.position.xy));
    let px = half * 2;
    let uv = (vec2<f32>(px) + 1.0) / gi.size.xy;
    let c = textureSampleLevel(input_a, linear_sampler, uv, 0.0).rgb;
    let d = depth_at(px);
    let z = select(SKY, min(gi.proj.z / max(d, 1e-7), SKY), d > 0.0);
    return vec4(max(c, vec3(0.0)), z);
}
