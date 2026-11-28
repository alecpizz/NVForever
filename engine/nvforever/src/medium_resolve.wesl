// The volumetric medium's reconstruction (medium.rs; #3032, stage 1, the default mode): the
// half-size far buffer rebuilt from this frame's rays (one per 2x2 tile by default, one per
// texel with tiles of side 1: `pattern.x`; medium_trace.wesl `medium_trace_tiles`) and last
// frame's buffer, as Bauer's "Creating the Atmospheric World of
// Red Dead Redemption 2" (SIGGRAPH 2019, slide 52) describes it: a texel whose ray was cast
// this frame takes it; the three others take last frame's value where their view ray lay then
// (moved with the camera at the distance the nearest cast ray found, by direction alone for an
// empty ray), held to the fresh rays of the 3x3 tiles around by the clamp in force (by default
// none while the view is still or slow, a variance clip fading in with motion); with no valid
// history (off screen last frame, after a cut) the fresh rays around, weighed by distance. Where we
// differ from the talk [engineering choice]: the talk leans on TAA after it, our default is multisampling with
// no temporal pass, so a settled fresh texel also keeps most of its own history (the fresh
// share, 0.1). Fractional reprojection uses a bounded cubic filter and reduces the effective
// sample count when the transported signal spans multiple history texels.
// No depth rejection: this stage is sky only. Written from the talk's description; no code from any
// project. Entry `medium_resolve`. An engineering choice for the enhanced look, not the
// original game's.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

// medium.rs MediumResolveUniform: the same fields in the same order.
struct MediumResolve {
    // This frame's camera rotation times the inverse of its unjittered projection.
    world_from_clip: mat4x4<f32>,
    // Last frame's unjittered projection times the inverse of last frame's camera rotation.
    prev_clip_from_rel: mat4x4<f32>,
    // Last frame's camera rotation times the inverse of its unjittered projection.
    prev_world_from_clip: mat4x4<f32>,
    // xyz: this frame's camera position minus last frame's; w: 1 while the history holds.
    delta: vec4<f32>,
    // x, y: the buffer's size; z: the phase; w: the fresh share's floor.
    grid: vec4<f32>,
    // x: the debug view (0 none, 1 fresh, 2 age, 3 history, 4 clamped, 5 distance); y, z: the
    // motion (texels a frame) over which the fresh share rises; w: the moving share.
    params: vec4<f32>,
    // x: the history's clamp (0 the box, 1 the variance clip, 2 none, 3 the variance clip fading
    // in with motion, the default: NVF_PLAY_DEBUG_MEDIUM_CLAMP);
    // y: the variance clip's k; z: history filter (0 linear, 1 cubic, 2 adaptive); w unused.
    clamp_mode: vec4<f32>,
    // x: the tile side (1, 2 or 3: NVF_PLAY_DEBUG_MEDIUM_TILE); y: the distance view's white (the
    // source's range); z, w unused.
    pattern: vec4<f32>,
}

@group(0) @binding(0) var<uniform> resolve: MediumResolve;
@group(0) @binding(1) var trace_light: texture_2d<f32>;
@group(0) @binding(2) var trace_distance: texture_2d<f32>;
@group(0) @binding(3) var history_light: texture_2d<f32>;
@group(0) @binding(4) var history_sampler: sampler;
@group(0) @binding(5) var history_aux: texture_2d<f32>;

struct ResolveOutput {
    // The published image (or a debug view of it).
    @location(0) published: vec4<f32>,
    // The next frame's history: the light and transmittance, and the distance and age.
    @location(1) light: vec4<f32>,
    @location(2) aux: vec2<f32>,
}

// The corner (0..side-1 in x and y) of tile `tile` of a buffer of `size` texels cast at `phase`,
// tiles of `side` x `side` (1, 2 or 3) (medium.rs `corner`; medium_trace.wesl has the same text):
// at side 2 the corners in the order (0,0), (1,0), (1,1), (0,1), the index the phase plus one on
// every other tile in a checkerboard (Bauer 2019, slides 53-54); a tile the buffer's edge cuts
// cycles through the corners it has; at side 1 the one texel.
fn medium_corner(tile: vec2<u32>, size: vec2<u32>, phase: u32, side: u32) -> vec2<u32> {
    let w = clamp(size.x - min(side * tile.x, size.x), 1u, side);
    let h = clamp(size.y - min(side * tile.y, size.y), 1u, side);
    if side == 3u {
        // Orthogonal neighbours keep a full tile's separation along their common axis.
        // The nine phases visit different rows and columns within each three-phase group.
        if w == 3u && h == 3u {
            let p = phase % 9u;
            let col = p % 3u;
            return vec2<u32>((col + tile.y) % 3u, (p / 3u + col + tile.x) % 3u);
        }
        // At the right/bottom boundary cycle over precisely the texels that exist.
        let p = phase % (w * h);
        let col = p % w;
        return vec2<u32>((col + tile.y) % w, (p / w + col + tile.x) % h);
    }
    let k = (phase + ((tile.x + tile.y) & 1u)) & 3u;
    var order = array<vec2<u32>, 4>(vec2<u32>(0u, 0u), vec2<u32>(1u, 0u), vec2<u32>(1u, 1u), vec2<u32>(0u, 1u));
    var have = array<vec2<u32>, 4>(vec2<u32>(0u, 0u), vec2<u32>(0u, 0u), vec2<u32>(0u, 0u), vec2<u32>(0u, 0u));
    var n = 0u;
    for (var i = 0u; i < 4u; i++) {
        if order[i].x < w && order[i].y < h {
            have[n] = order[i];
            n += 1u;
        }
    }
    return have[k % max(n, 1u)];
}

// A transmittance from this up stores as one in a half float (1 - 2^-12 rounds to 1): empty sky.
const EMPTY_T: f32 = 0.99975586;

// The distance from this frame's camera of the point at `dist` along last frame's ray through
// `prev_uv` (last frame's buffer coordinates), the camera having moved by `delta` (medium.rs
// `carried_distance`, the same arithmetic); an empty distance stays empty.
fn medium_carried_distance(prev_world_from_clip: mat4x4<f32>, prev_uv: vec2<f32>, dist: f32, delta: vec3<f32>) -> f32 {
    if dist <= 0.0 {
        return -1.0;
    }
    let ndc = vec2<f32>(prev_uv.x * 2.0 - 1.0, 1.0 - prev_uv.y * 2.0);
    let q = prev_world_from_clip * vec4<f32>(ndc, 1.0, 1.0);
    let prev_dir = normalize(q.xyz / q.w);
    return max(length(prev_dir * dist - delta), 1.0);
}

// The weight of a fresh ray `sq` squared texels from a texel, tiles of `side` (medium.rs
// `ray_weight`, the same arithmetic): a Gaussian of 0.6 of the tile's side, so the nine rays'
// shares change from texel to texel instead of all at once midway between two casts. At side 1
// every texel has its own ray and is never filled; the clip keeps its nine equal weights.
fn medium_ray_weight(sq: f32, side: u32) -> f32 {
    if side == 1u {
        return 1.0;
    }
    return exp(-sq / (2.0 * 0.36 * f32(side * side)));
}

// The variance clip: the weighted mean plus or minus k standard deviations of the fresh rays per
// channel (Salvi, "An Excursion in Temporal Supersampling", GDC 2016), kept to what the stored
// format holds (light not below 0, transmittance in 0..1).
fn medium_variance_clip(h: vec4<f32>, mean: vec4<f32>, sd: vec4<f32>, k: f32) -> vec4<f32> {
    let kk = clamp(k, 0.5, 8.0);
    let c = clamp(h, mean - kk * sd, mean + kk * sd);
    return vec4<f32>(max(c.rgb, vec3<f32>(0.0)), clamp(c.a, 0.0, 1.0));
}

// The history held by the clamp in force (medium.rs `clip_history`, the same arithmetic):
// `moving` (the default) none while the view is still or slow, the variance clip fading in with
// the motion ramp (`ramp`, 0 below MOTION_FROM texels a frame, 1 from MOTION_FULL); the box of
// the fresh rays; the variance clip; or none.
fn medium_clip(h: vec4<f32>, lo: vec4<f32>, hi: vec4<f32>, mean: vec4<f32>, sd: vec4<f32>, ramp: f32, mode: vec4<f32>) -> vec4<f32> {
    let m = u32(mode.x);
    if m == 3u {
        return mix(h, medium_variance_clip(h, mean, sd, mode.y), clamp(ramp, 0.0, 1.0));
    }
    if m == 2u {
        return h;
    }
    if m == 1u {
        return medium_variance_clip(h, mean, sd, mode.y);
    }
    return clamp(h, lo, hi);
}

// Recover the curvature a bilinear transport loses. In one dimension the three bilinear taps
// expand exactly to the four Catmull-Rom weights. For a diagonal pan apply the two axis
// corrections to the same centre (five taps total). A correction cannot extend the range of
// the measured current rays and the bilinear history, including at a cloud/empty boundary.
fn medium_history_cubic(uv: vec2<f32>, size: vec2<f32>, f: vec2<f32>, centre: vec4<f32>, lo: vec4<f32>, hi: vec4<f32>) -> vec4<f32> {
    let weight = 0.5 * f * (vec2<f32>(1.0) - f);
    if max(weight.x, weight.y) < 0.001 || (all(centre == lo) && all(centre == hi)) {
        return centre;
    }
    let x = vec2<f32>(1.0 / size.x, 0.0);
    let y = vec2<f32>(0.0, 1.0 / size.y);
    let left = textureSampleLevel(history_light, history_sampler, uv - x, 0.0);
    let right = textureSampleLevel(history_light, history_sampler, uv + x, 0.0);
    let up = textureSampleLevel(history_light, history_sampler, uv - y, 0.0);
    let down = textureSampleLevel(history_light, history_sampler, uv + y, 0.0);
    let cubic = centre + weight.x * (2.0 * centre - left - right) + weight.y * (2.0 * centre - up - down);
    return clamp(cubic, min(centre, lo), max(centre, hi));
}

// The aux target's second channel holds a texel's own casts since its history became valid and
// the frames since its last cast: count x AGE_SPAN + age (medium.rs `pack_aux`), both capped so
// the sum stores exactly in a full float.
const AGE_SPAN: f32 = 16.0;
const AGE_CAP: f32 = 15.0;
const COUNT_CAP: f32 = 255.0;
// A half float's spacing, relative (the lower bound within a binade), and its subnormal step.
const F16_STEP: f32 = 0.00048828125;
const F16_MIN_STEP: f32 = 0.000000059604645;

// A fresh cast blended into its history at `share` (medium.rs `fresh_blend`, the same
// arithmetic), per channel: where the step would not move the stored half float (it is below
// the format's spacing at the history's value), that channel moves toward the cast by the gap
// or twice that spacing, whichever is smaller (one or two stored steps), never past it, so it
// still converges and jumps by no more than two steps (the threshold is
// conservative, r7). All four channels go together in one case only: empty history and a cast that sees
// cloud, whose blend would still store as empty and be wiped by the empty rule; then the cast
// is taken whole (its distance is the cast's on every fresh cast).
fn medium_fresh_blend(hist: vec4<f32>, own: vec4<f32>, share: f32) -> vec4<f32> {
    let blended = mix(hist, own, share);
    if blended.a >= EMPTY_T && own.a < EMPTY_T {
        return own;
    }
    // At or below the spacing: a blend landing on the midpoint between two half
    // floats rounds back to the even one, so equality stalls too.
    let spacing = max(abs(hist) * F16_STEP, vec4<f32>(F16_MIN_STEP));
    let stalled = abs(blended - hist) <= spacing;
    let nudged = hist + sign(own - hist) * min(abs(own - hist), 2.0 * spacing);
    return select(blended, nudged, stalled);
}

// An opaque debug colour (premultiplied colour, transmittance 0).
fn debug_colour(c: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(c, 0.0);
}

@fragment
fn medium_resolve(in: FullscreenVertexOutput) -> ResolveOutput {
    let size = vec2<u32>(max(resolve.grid.xy, vec2<f32>(1.0)));
    // The tile side (2: one ray per 2x2 tile a frame; 1: every texel every frame, through the
    // history all the same).
    let side = clamp(u32(resolve.pattern.x), 1u, 3u);
    let tiles = (size + vec2<u32>(side - 1u)) / vec2<u32>(side);
    let texel = vec2<u32>(in.position.xy);
    let tile = texel / vec2<u32>(side);
    let phase = u32(resolve.grid.z);
    let fresh = all(texel == side * tile + medium_corner(tile, size, phase, side));
    let own = textureLoad(trace_light, vec2<i32>(tile), 0);

    // The box of the fresh rays of the 3x3 tiles around (the light and the transmittance), and
    // the cast ray nearest this texel among them (its distance seeds the history's
    // lookup). The own tile is visited first, so of two at the same distance it wins.
    var lo = own;
    var hi = own;
    // The nine fresh rays and their weights, kept for their mean (the fill of a texel with no
    // history) and spread (the variance clip).
    var rays = array<vec4<f32>, 9>(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
    var ray_w = array<f32, 9>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    var ray_count = 0u;
    var near_d = textureLoad(trace_distance, vec2<i32>(tile), 0).r;
    let own_cast = vec2<i32>(side * tile + medium_corner(tile, size, phase, side));
    var near_sq = dot(vec2<f32>(own_cast - vec2<i32>(texel)), vec2<f32>(own_cast - vec2<i32>(texel)));
    let last = vec2<i32>(tiles) - 1;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let at = vec2<i32>(tile) + vec2<i32>(dx, dy);
            let t = clamp(at, vec2<i32>(0), last);
            let s = textureLoad(trace_light, t, 0);
            lo = min(lo, s);
            hi = max(hi, s);
            rays[ray_count] = s;
            let cast_at = vec2<i32>(side * vec2<u32>(t) + medium_corner(vec2<u32>(t), size, phase, side));
            let off = vec2<f32>(cast_at - vec2<i32>(texel));
            let sq = dot(off, off);
            // A tile held to the buffer's counts where it lies, not here again (side 1 as before).
            ray_w[ray_count] = select(0.0, medium_ray_weight(sq, side), side == 1u || all(t == at));
            ray_count += 1u;
            if sq < near_sq {
                near_sq = sq;
                near_d = textureLoad(trace_distance, t, 0).r;
            }
        }
    }

    // The rays' weighted mean: premultiplied light and transmittance are both linear in what is
    // composed (light + transmittance x behind), so the mean composes to the mean of the rays.
    var weight_sum = 0.0;
    var mean = vec4<f32>(0.0);
    for (var i = 0u; i < 9u; i++) {
        mean += ray_w[i] * rays[i];
        weight_sum += ray_w[i];
    }
    mean = mean / weight_sum;
    let filled = mean;

    // This texel's view ray, and where the point on it at the nearest ray's distance (or, for
    // an empty ray, its direction alone) lay in last frame's view.
    let fsize = vec2<f32>(size);
    let uv = (vec2<f32>(texel) + 0.5) / fsize;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let p = resolve.world_from_clip * vec4<f32>(ndc, 1.0, 1.0);
    let dir = normalize(p.xyz / p.w);
    var rel = vec4<f32>(dir, 1.0);
    if near_d > 0.0 {
        rel = vec4<f32>(dir * near_d + resolve.delta.xyz, 1.0);
    }
    let clip = resolve.prev_clip_from_rel * rel;
    var valid = resolve.delta.w > 0.5 && clip.w > 1e-6;
    var prev_uv = uv;
    if valid {
        let n = clip.xy / clip.w;
        prev_uv = vec2<f32>(n.x * 0.5 + 0.5, 0.5 - n.y * 0.5);
        valid = all(prev_uv >= vec2<f32>(0.0)) && all(prev_uv <= vec2<f32>(1.0));
    }

    var hist = filled;
    var hist_d = near_d;
    var hist_age = 0.0;
    var hist_count = 0.0;
    var moved = 0.0;
    var motion = 0.0;
    if valid {
        var h = textureSampleLevel(history_light, history_sampler, prev_uv, 0.0);
        let at = min(vec2<i32>(prev_uv * fsize), vec2<i32>(size) - 1);
        let a = textureLoad(history_aux, at, 0).rg;
        motion = length((prev_uv - uv) * fsize);
        let fraction = fract(prev_uv * fsize - 0.5);
        if resolve.clamp_mode.z >= 1.0 && motion > 0.02 {
            h = medium_history_cubic(prev_uv, fsize, fraction, h, lo, hi);
        }
        // The rays' spread from the deviations (no cancellation: nine equal rays give a band of
        // no width, to rounding), under the mean's weights: the band is not one over a tile.
        var spread = vec4<f32>(0.0);
        for (var i = 0u; i < 9u; i++) {
            let d = rays[i] - mean;
            spread += ray_w[i] * d * d;
        }
        let sd = sqrt(spread / weight_sum);
        let ramp = smoothstep(resolve.params.y, resolve.params.z, motion);
        hist = medium_clip(h, lo, hi, mean, sd, ramp, resolve.clamp_mode);
        moved = length(hist - h);
        // Last frame's distance in this frame's camera's terms: the point on
        // the history texel's own ray (last frame's camera) at its distance, moved by the
        // camera's displacement, measured from here. Clamped to empty sky, it is empty again.
        hist_d = medium_carried_distance(resolve.prev_world_from_clip, (vec2<f32>(at) + 0.5) / fsize, a.x, resolve.delta.xyz);
        if hist.a >= EMPTY_T {
            hist_d = -1.0;
        }
        // The nearest texel's count, as the distance (the least count of the bilinear
        // taps spread low counts in a turn and raised the grain; design document, known limits).
        let packed = max(a.y, 0.0);
        hist_count = floor(packed / AGE_SPAN);
        hist_age = packed - hist_count * AGE_SPAN;
        if resolve.clamp_mode.z >= 2.0 && motion > 0.02 {
            // The sum of squared bilinear weights is one at a texel centre, half on an edge
            // midpoint and a quarter between four centres. Limit the number of independent
            // casts we credit to the transported value. As the view stops, new exact casts
            // increase that count again through the normal running mean.
            let energy = vec2<f32>(1.0) - 2.0 * fraction * (vec2<f32>(1.0) - fraction);
            let loss = 1.0 - energy.x * energy.y;
            let support = max(2.0, floor(1.0 / max(loss, 1.0 / COUNT_CAP)));
            hist_count = min(hist_count, support);
        }
    }

    // A fresh cast takes share max(floor, 1 / (max(count, 1) + 1)) of its history, the count its
    // own casts since the history became valid: a running mean down to the floor. A
    // texel filled from the rays around has a count of 0; the fill stands in for one cast, so the
    // first own cast averages with it and the two weigh as one cast from the second on.
    var light = filled;
    var dist = near_d;
    var age = 0.0;
    var count = 0.0;
    if fresh {
        light = own;
        dist = near_d;
        count = 1.0;
        if valid {
            let floor_share = max(clamp(resolve.grid.w, 0.0, 1.0), smoothstep(resolve.params.y, resolve.params.z, motion) * clamp(resolve.params.w, 0.0, 1.0));
            let share = max(floor_share, 1.0 / (max(hist_count, 1.0) + 1.0));
            light = medium_fresh_blend(hist, own, share);
            count = min(hist_count + 1.0, COUNT_CAP);
        }
    } else if valid {
        light = hist;
        dist = hist_d;
        age = min(hist_age + 1.0, AGE_CAP);
        count = hist_count;
    }

    // Canonical where it is written: a texel that will store as empty holds no
    // light and the empty distance, so filtering cannot carry light out of it.
    if light.a >= EMPTY_T {
        light = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        dist = -1.0;
    }
    var out: ResolveOutput;
    out.light = light;
    out.aux = vec2<f32>(dist, count * AGE_SPAN + age);
    let view = u32(resolve.params.x);
    if view == 1u {
        if fresh {
            out.published = debug_colour(vec3<f32>(0.0, 0.6, 0.0));
        } else if valid {
            out.published = debug_colour(vec3<f32>(0.25));
        } else {
            out.published = debug_colour(vec3<f32>(0.8, 0.0, 0.0));
        }
    } else if view == 2u {
        out.published = debug_colour(vec3<f32>(clamp(age / 8.0, 0.0, 1.0)));
    } else if view == 3u {
        if valid {
            out.published = light;
        } else {
            out.published = debug_colour(vec3<f32>(0.8, 0.0, 0.0));
        }
    } else if view == 4u {
        out.published = debug_colour(vec3<f32>(clamp(moved * 4.0, 0.0, 1.0)));
    } else if view == 5u {
        if dist > 0.0 {
            out.published = debug_colour(vec3<f32>(clamp(dist / max(resolve.pattern.y, 1.0), 0.0, 1.0)));
        } else {
            out.published = debug_colour(vec3<f32>(0.0, 0.0, 0.6));
        }
    } else {
        out.published = light;
    }
    return out;
}
