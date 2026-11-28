// The volumetric medium's light in the air, the composite (medium_air_compose.rs; #3032 stage 3): a fullscreen pass after the main pass that reads the HDR picture, the
// depth prepass and the froxel volume the grid pass (medium_air_grid.wesl) wrote this frame, and
// writes the picture again. An engineering choice of NVForever's own for the enhanced look, not
// the original game's behaviour. Entry `air_compose`; `MULTISAMPLED` picks the depth's sample
// type.
//
// The volume's texel k holds the sums at slice k's far boundary (r N = sum of dF x V, no
// phase; g M = sum of dF; b the slice's own V), with a zero boundary at the camera
// (`air_lookup`, mirrored by medium_air_compose.rs `lookup_mirror`, which matches medium_air.rs
// `sums_at`).
//
// The light (views none, delta, rays), per depth sample, mirrored as a whole by medium_air_compose.rs `pixel_mirror`
// over medium_air.rs's functions:
// - the lit fraction Vbar = clamp(N / M, 0, 1) (`lit_fraction`; 1 while M is empty) and its
//   relative form Vbar_c = clamp(1 + RAYS (Vbar - V_ref) / V_ref, SHADE, LIFT) (`relative_lit`,
//   an artistic gain: average air unchanged, V_ref the clouds' mean or REF); the pixel's own
//   phase applied once, p_eff = max(p, FLOOR) (`phase_floor`);
// - the sample's blend t (`sample_blend`) = t_d + (1 - t_d) g, t_d the far blend (`far_blend`):
//   1 for cleared depth, else smoothstep(0.6 x sky_from, sky_from, d) of the sample's radial
//   distance; g the fog blend (`fog_blend`): smoothstep(AIR_FOG_BLEND_FROM, 1, F_near), so a
//   surface the fog covers is treated as the sky it stands against;
// - one form for every sample (`blended_correction`, `compose_blended`): in display values
//   Y'_new = Y' + corr_t x B_t, B_t = mix(C', Y', t), corr_t = max(k a F_near (Vbar_c x
//   mix(p_eff, mix(1, p_eff, SKYPHASE), t) - 1) x mix(1, SKY, t), -min(F_near, 1)), k a the
//   applied scale (phase2.z: the strength k, 0..2, the one overall scale, times the share a,
//   medium_air.rs `applied_scale`; here also times the deck's share D of the clouds' mean,
//   `air_deck_share`, once a pixel), applied before the floor, F_near the
//   fog share at the sample's distance held to the grid's end (`near_share`); near samples are
//   the additive world rule (budget C'), cleared depth the sky's multiplicative rule (budget
//   Y') [documented: Bauer 2019, slide 62: the ray's lit fraction modulates the sky's own
//   scattering], far and fogged terrain blends between them with no step;
// - a gain (corr_t above 0) is held off bright pixels by the sky's share t: corr_t (1 - t h), h
//   (`bright_hold`) the share of the gain that would not fit under AIR_BRIGHT_TO;
// - the picture is read held to AIR_HDR_MAX and linear results are held to 0..AIR_HDR_MAX per
//   channel on every computed path.
// The pixel takes the mean of its samples' results; they share one volume lookup when their
// view depths, held to the grid's end, lie within a quarter slice of each other
// (`slices_agree`), each with its own F_near and t. A declared opaque-world approximation for
// the first look.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import nvf_assets::first_person_depth::unsqueeze;

struct AirUniform {
    // The camera's world-from-view transform (unjittered).
    world_from_view: mat4x4<f32>,
    // x, y: the projection's x and y scale; z: its near plane; w: the sky's distance.
    proj: vec4<f32>,
    // nx, ny, nz, the grid's depth (view depth, game units).
    grid: vec4<f32>,
    // The slice curve: first (the thickness when linear), r, ln r (0: linear); w: RAYS.
    curve: vec4<f32>,
    // The fog: near, far, power, strength.
    fog: vec4<f32>,
    // The phase: g, back g, back weight, the inner cap.
    phase: vec4<f32>,
    // The phase's norm, its largest value, the applied scale k x a (the strength times the
    // effective share, 0..2), the view's code.
    phase2: vec4<f32>,
    // xyz: the direction to the light (world); w: 0 when no light is to be drawn, else
    // 2 - FLOOR (medium_air.rs `light_flag`).
    light: vec4<f32>,
    // rgb: the fog colour (linear); w: 1 for the display-value blend.
    fog_colour: vec4<f32>,
    // x: the occluders' code; y: the grid's samples per froxel; z: SKY; w: SKYPHASE.
    misc: vec4<f32>,
    // The main pass's viewport: origin and size in pixels.
    viewport: vec4<f32>,
    // The grid pass's terrain map and Cloud shadows blocks (medium_air_grid.wesl; declared here
    // to reach rays2 at the end; of those only cloud_mode.yz are read:
    // y the clouds' mean K for the reference, z the clouds' own mean transmittance for the deck's
    // share (the flat layers' from the CPU or, with the light volume, the grid's mean pass's).
    terrain_uv: vec4<f32>,
    terrain: vec4<f32>,
    terrain_light: vec4<f32>,
    cloud_uv: vec4<f32>,
    cloud: vec4<f32>,
    cloud_mode: vec4<f32>,
    // x: LIFT; y: the REF override (-1 for auto: the clouds' mean); z: SHADE; w: 1 while SHARE is
    // set (the deck's share is then 1), else 0.
    rays2: vec4<f32>,
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@if(MULTISAMPLED) @group(0) @binding(1) var depth: texture_depth_multisampled_2d;
@if(!MULTISAMPLED) @group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var volume: texture_3d<f32>;
@group(0) @binding(3) var volume_sampler: sampler;
@group(0) @binding(4) var<uniform> air: AirUniform;

// The views' codes (medium_air.rs `AirView::code`).
const VIEW_NONE: u32 = 0u;
const VIEW_VISIBILITY: u32 = 1u;
const VIEW_SLICE: u32 = 2u;
const VIEW_SHARE: u32 = 3u;
const VIEW_DELTA: u32 = 4u;
const VIEW_LIT: u32 = 6u;
const VIEW_RAYS: u32 = 7u;

// medium_air.rs's constants (the tests compare them): the lit fraction's empty M, the display
// transfer, the samples' agreement in slices, the far blend's start, the fog blend's start, the
// deck's share's ends, its least, the
// bright hold's ends and the least excess it acts on, the largest linear value written (Rgba16Float's largest finite value is 65,504).
const RATIO_EMPTY: f32 = 1e-9;
const DISPLAY_GAMMA: f32 = 2.2;
const AIR_AGREE_SLICES: f32 = 0.25;
const SKY_TAPER_FROM: f32 = 0.6;
const AIR_FOG_BLEND_FROM: f32 = 0.35;
const AIR_DECK_FROM: f32 = 0.4;
const AIR_DECK_TO: f32 = 0.85;
const AIR_DECK_LEAST: f32 = 0.25;
const AIR_BRIGHT_FROM: f32 = 0.5;
const AIR_BRIGHT_TO: f32 = 0.76;
const AIR_BRIGHT_LEAST: f32 = 1e-6;
const AIR_HDR_MAX: f32 = 60000.0;
// The delta view: mid grey plus the scalar times this (display values).
const DELTA_GAIN: f32 = 4.0;

// One depth sample: whether its depth is cleared (the sky), its view depth and its radial
// distance (view depth x the pixel's ray length). A cleared sample's air is the grid's whole
// column: the grid's end as its view depth, the end's radial distance on this ray.
struct AirSample {
    cleared: bool,
    view_depth: f32,
    distance: f32,
}

// What a pixel shares between its samples: its uv, its ray's length, the floored phase of its
// ray (p_eff, applied once), the reference visibility, the scale of its corrections (k a D), the
// display values of its picture and of the fog colour, and each one's largest channel.
struct AirPixel {
    uv: vec2<f32>,
    ray_len: f32,
    p_eff: f32,
    v_ref: f32,
    scale: f32,
    src_d: vec3<f32>,
    fog_d: vec3<f32>,
    src_peak: f32,
    fog_peak: f32,
}

// The depth's sample count.
@if(MULTISAMPLED) fn air_samples() -> u32 { return textureNumSamples(depth); }
@if(!MULTISAMPLED) fn air_samples() -> u32 { return 1u; }

// Depth sample `s` of pixel `px`: the first-person shapes' squeezed depth undone, then
// reverse infinite z (view depth = near / depth); cleared where the depth is not above 0.
fn air_sample_at(px: vec2<i32>, s: u32, ray_len: f32) -> AirSample {
    let dims = vec2<i32>(textureDimensions(depth));
    let d = unsqueeze(textureLoad(depth, clamp(px, vec2<i32>(0), dims - vec2<i32>(1)), i32(s)));
    var found: AirSample;
    if d <= 0.0 {
        found.cleared = true;
        found.view_depth = air.grid.w;
        found.distance = air.grid.w * ray_len;
        return found;
    }
    found.cleared = false;
    found.view_depth = air.proj.z / d;
    found.distance = found.view_depth * ray_len;
    return found;
}

// The continuous slice coordinate of view depth z (medium_air.rs `AirCurve::coord`), 0..nz:
// nz exactly at and beyond the grid's end; linear `z / depth x nz` when ln r is 0, else
// `ln(1 + z (r - 1) / first) / ln r`.
fn air_coord(z: f32) -> f32 {
    let nz = air.grid.z;
    if z >= air.grid.w {
        return nz;
    }
    let zz = clamp(z, 0.0, air.grid.w);
    var c = zz / max(air.grid.w, 1.0e-3) * nz;
    if air.curve.z > 0.0 {
        c = log(1.0 + zz * (air.curve.y - 1.0) / air.curve.x) / air.curve.z;
    }
    return clamp(c, 0.0, nz);
}

// The slice's thickness at view depth z (medium_air.rs `AirCurve::slice_width`): dz/dc =
// (first / (r - 1) + z) x ln r, or depth / nz when linear.
fn air_slice_width(z: f32) -> f32 {
    if air.curve.z > 0.0 {
        return (air.curve.x / (air.curve.y - 1.0) + z) * air.curve.z;
    }
    return air.grid.w / max(air.grid.z, 1.0);
}

// The share of fog at distance d (medium_air.rs `fog_share`).
fn air_fog_share(d: f32) -> f32 {
    let x = clamp((d - air.fog.x) / max(air.fog.y - air.fog.x, 1.0), 0.0, 1.0);
    return select(0.0, pow(x, air.fog.z) * air.fog.w, x > 0.0);
}

// One Henyey-Greenstein lobe scaled to mean 1 over the sphere, g held to +-0.99.
fn air_lobe(g: f32, c: f32) -> f32 {
    let gg = clamp(g, -0.99, 0.99);
    let d = max(1.0 + gg * gg - 2.0 * gg * c, 1.0e-6);
    return (1.0 - gg * gg) / (d * sqrt(d));
}

// The normalised capped phase of the pixel's own ray (medium_air.rs `NormedPhase::phase`):
// the lobes mixed by the back weight, capped by the inner cap, times the norm. The composite's
// only phase: the volume's sums carry none.
fn air_phase(cos_theta: f32) -> f32 {
    let c = clamp(cos_theta, -1.0, 1.0);
    let w = clamp(air.phase.z, 0.0, 1.0);
    let mixed = (1.0 - w) * air_lobe(air.phase.x, c) + w * air_lobe(air.phase.y, c);
    return min(mixed, air.phase.w) * air.phase2.x;
}

// The sums (r N, g M) at boundary coordinate c in 0..nz along the column at uv, trilinear:
// boundary k + 1 is texel k's centre, w = (c - 0.5) / nz; below boundary 1 texel 0 scaled by
// c (the zero boundary at the camera); at and beyond the grid's end the last texel (the end's
// value carried). Mirrored by medium_air_compose.rs `lookup_mirror`.
fn air_lookup(uv: vec2<f32>, c: f32) -> vec4<f32> {
    let nz = air.grid.z;
    let cc = clamp(c, 0.0, nz);
    let w = (max(cc, 1.0) - 0.5) / nz;
    return textureSampleLevel(volume, volume_sampler, vec3<f32>(uv, w), 0.0) * min(cc, 1.0);
}

// The sums in front of a sample: at its slice coordinate, the grid's end's beyond it.
fn air_sums(uv: vec2<f32>, s: AirSample) -> vec4<f32> {
    return air_lookup(uv, air_coord(min(s.view_depth, air.grid.w)));
}

// The lit fraction Vbar (medium_air.rs `lit_fraction`): N / M held to 0..1; 1 while M is not
// above RATIO_EMPTY. No phase: the sums are phase-free.
fn air_lit_fraction(n: f32, m: f32) -> f32 {
    if !(m > RATIO_EMPTY) {
        return 1.0;
    }
    return clamp(n / m, 0.0, 1.0);
}

// The reference visibility V_ref (medium_air.rs `reference_visibility`): the REF override
// (rays2.y, held to 0.05..1) when set, else the clouds' mean transmittance (cloud_mode.y; 0 or
// not finite taken as 1; held to 0.25..1).
fn air_reference() -> f32 {
    if air.rays2.y > 0.0 {
        return clamp(air.rays2.y, 0.05, 1.0);
    }
    let m = air.cloud_mode.y;
    return clamp(select(1.0, m, m > 0.0 && m < 1.0e30), 0.25, 1.0);
}

// The deck's share D of the correction (medium_air.rs `deck_share`): 1 while SHARE is set (rays2.w:
// an absolute share), else AIR_DECK_LEAST + (1 - AIR_DECK_LEAST) x smoothstep(AIR_DECK_FROM,
// AIR_DECK_TO, T), T the clouds' own mean transmittance for a light from straight above
// (cloud_mode.z; 0 is an opaque deck).
fn air_deck_share() -> f32 {
    if air.rays2.w > 0.5 {
        return 1.0;
    }
    let x = clamp((air.cloud_mode.z - AIR_DECK_FROM) / (AIR_DECK_TO - AIR_DECK_FROM), 0.0, 1.0);
    return AIR_DECK_LEAST + (1.0 - AIR_DECK_LEAST) * (x * x * (3.0 - 2.0 * x));
}

// The relative lit fraction (medium_air.rs `relative_lit`): clamp(1 + RAYS x (Vbar - V_ref) /
// V_ref, SHADE, LIFT): average air is 1, shadowed air falls toward SHADE, lit air rises up to
// LIFT.
fn air_relative_lit(vbar: f32, v_ref: f32) -> f32 {
    return clamp(1.0 + max(air.curve.w, 0.0) * (clamp(vbar, 0.0, 1.0) - v_ref) / v_ref, clamp(air.rays2.z, 0.0, 1.0), max(air.rays2.x, 1.0));
}

// One volume lookup turned into the relative lit fraction of the air in front of view depth z
// (held to the grid's end).
fn air_relative_at_depth(px: AirPixel, z: f32) -> f32 {
    let nm = air_lookup(px.uv, air_coord(min(z, air.grid.w)));
    return air_relative_lit(air_lit_fraction(nm.r, nm.g), px.v_ref);
}

// The same for the air in front of a sample.
fn air_relative_at(px: AirPixel, s: AirSample) -> f32 {
    return air_relative_at_depth(px, s.view_depth);
}

// F_near (medium_air.rs `near_share`): the fog share at the distance held to the grid's end on
// this ray (the grid's depth x the ray's length).
fn air_near_share(d: f32, ray_len: f32) -> f32 {
    return air_fog_share(min(d, air.grid.w * ray_len));
}

// The far blend t (medium_air.rs `far_blend`): 1 for cleared depth or a degenerate sky
// distance; else x^2 (3 - 2x), x = clamp((d - 0.6 sky_from) / (0.4 sky_from), 0, 1).
fn air_far_blend(s: AirSample) -> f32 {
    let sky_from = air.proj.w;
    if s.cleared || !(sky_from > 0.0) {
        return 1.0;
    }
    let start = SKY_TAPER_FROM * sky_from;
    let x = clamp((s.distance - start) / (sky_from - start), 0.0, 1.0);
    return x * x * (3.0 - 2.0 * x);
}

// The sample's blend t (medium_air.rs `sample_blend`): the far blend joined with the fog blend g
// = smoothstep(AIR_FOG_BLEND_FROM, 1, F_near), t_d + (1 - t_d) g: one smooth union of the two, so
// no slope breaks where they cross.
fn air_sample_blend(t_distance: f32, f_near: f32) -> f32 {
    let x = clamp((f_near - AIR_FOG_BLEND_FROM) / (1.0 - AIR_FOG_BLEND_FROM), 0.0, 1.0);
    let g = x * x * (3.0 - 2.0 * x);
    return t_distance + (1.0 - t_distance) * g;
}

// One sample's bounded scalar (medium_air.rs `blended_correction`): k a F_near (Vbar_c x
// mix(p_eff, mix(1, p_eff, SKYPHASE), t) - 1) x mix(1, SKY, t) with k a = px.scale (phase2.z x the
// deck's share: the strength and the deck scale the whole correction), held to at least -min(F_near, 1) (shadow removes at
// most the budget share F_near, at any strength).
fn air_correction(px: AirPixel, f_near: f32, vc: f32, t: f32) -> f32 {
    let k = clamp(air.misc.w, 0.0, 1.0);
    let phase_t = mix(px.p_eff, mix(1.0, px.p_eff, k), t);
    let corr = px.scale * f_near * (vc * phase_t - 1.0) * mix(1.0, air.misc.z, t);
    return max(corr, -min(f_near, 1.0));
}

// The bright hold h (medium_air.rs `bright_hold`): the share of a gain (display values, above 0) to
// hold back from a pixel whose largest channel is `peak`: 0 while peak + gain is at most
// AIR_BRIGHT_FROM; what is over it fits softly into the room left under AIR_BRIGHT_TO, over x room
// / (over + room), so a brighter pixel under the sky's rule never comes out darker; 1 from AIR_BRIGHT_TO on (no room).
fn air_bright_hold(peak: f32, gain: f32) -> f32 {
    let over = gain - max(AIR_BRIGHT_FROM - peak, 0.0);
    if !(over > AIR_BRIGHT_LEAST) {
        return 0.0;
    }
    let room = max(AIR_BRIGHT_TO - max(peak, AIR_BRIGHT_FROM), 0.0);
    return over / (over + room) * (over / gain);
}

// A linear colour held to 0..AIR_HDR_MAX per channel (every colour this pass computes).
fn air_held(c: vec3<f32>) -> vec3<f32> {
    return clamp(c, vec3<f32>(0.0), vec3<f32>(AIR_HDR_MAX));
}

// One sample's result (medium_air.rs `sample_blend`, `hold_bright`, `compose_blended`): xyz the composed
// linear colour, max(Y' + delta, 0)^2.2 held with delta = corr_t x mix(C', Y', t); w its scalar corr_t,
// the bright hold taken of it when above 0 (one scalar, so a hue keeps). A
// sample with geometry holds delta per channel to at least -F_near x C' (the fog's own share of
// the fog colour), so far terrain keeps its surface light at every t: Y' + delta >= (1 - f) S'
// whenever F_near <= f. Cleared depth keeps the scalar bound.
// The sky dome writes depth and is held as terrain is (a known limit: medium_air_compose.rs).
fn air_result(px: AirPixel, s: AirSample, vc: f32) -> vec4<f32> {
    let f_near = air_near_share(s.distance, px.ray_len);
    let t = air_sample_blend(air_far_blend(s), f_near);
    var c = air_correction(px, f_near, vc, t);
    if c > 0.0 {
        c = c * (1.0 - t * air_bright_hold(px.src_peak, c * mix(px.fog_peak, px.src_peak, t)));
    }
    let budget = mix(px.fog_d, px.src_d, vec3<f32>(t));
    var delta = c * budget;
    if !s.cleared {
        delta = max(delta, -f_near * px.fog_d);
    }
    let shown = max(px.src_d + delta, vec3<f32>(0.0));
    return vec4<f32>(air_held(pow(shown, vec3<f32>(DISPLAY_GAMMA))), c);
}

// The pixel's result (medium_air_compose.rs `pixel_mirror`): the mean of its samples' results
// (colours in linear light, as resolved). When the samples' view depths, held to the grid's
// end, lie within AIR_AGREE_SLICES of a slice of each other (medium_air.rs `slices_agree`) they
// share ONE volume lookup at the farthest of them: free of the samples' order, and its sums
// hold all the fog in front of the nearer ones; otherwise (a
// silhouette against farther ground) each sample looks up its own. Each sample takes its own
// F_near and blend t from its own distance.
fn air_pixel(px: AirPixel, pixel: vec2<i32>) -> vec4<f32> {
    let count = air_samples();
    var lo = min(air_sample_at(pixel, 0u, px.ray_len).view_depth, air.grid.w);
    var hi = lo;
    for (var i = 1u; i < count; i += 1u) {
        let z = min(air_sample_at(pixel, i, px.ray_len).view_depth, air.grid.w);
        lo = min(lo, z);
        hi = max(hi, z);
    }
    let one_tap = hi - lo <= AIR_AGREE_SLICES * air_slice_width(lo);
    var shared_vc = 1.0;
    if one_tap {
        shared_vc = air_relative_at_depth(px, hi);
    }
    var sum = vec4<f32>(0.0);
    for (var i = 0u; i < count; i += 1u) {
        let s = air_sample_at(pixel, i, px.ray_len);
        var vc = shared_vc;
        if !one_tap {
            vc = air_relative_at(px, s);
        }
        sum += air_result(px, s, vc);
    }
    return sum / f32(count);
}

// FLOOR from the uniform's light.w (medium_air.rs `floor_from_flag`).
fn air_floor() -> f32 {
    return clamp(2.0 - air.light.w, 0.0, 1.0);
}

// The slice bands' tints: two alternating, a third on every eighth slice (0, 8, 16, ...).
fn air_slice_tint(k: u32) -> vec3<f32> {
    if (k % 8u) == 0u {
        return vec3<f32>(0.1, 0.3, 1.0);
    }
    return select(vec3<f32>(1.0, 0.45, 0.05), vec3<f32>(0.1, 0.85, 0.2), (k & 1u) == 1u);
}

// A grey shown as display value g (into linear, as the fog's display-value blend sees it).
fn air_grey(g: f32) -> vec3<f32> {
    return vec3<f32>(pow(clamp(g, 0.0, 1.0), 2.2));
}

// A fraction v in 0..1 as a linear grey that survives bloom and tonemapping: 0.02 + 0.5 v^2.
fn air_fraction_grey(v: f32) -> vec3<f32> {
    let vv = clamp(v, 0.0, 1.0);
    return vec3<f32>(0.02 + 0.5 * vv * vv);
}

@fragment
fn air_compose(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(floor(in.position.xy));
    let src = textureLoad(picture, pixel, 0);
    // Outside the main pass's viewport the picture passes through.
    let size = max(air.viewport.zw, vec2<f32>(1.0));
    let local = in.position.xy - air.viewport.xy;
    if any(local < vec2<f32>(0.0)) || any(local >= size) {
        return src;
    }
    let uv = local / size;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let ray_view = vec3<f32>(ndc.x / air.proj.x, ndc.y / air.proj.y, -1.0);
    let ray_len = length(ray_view);
    let code = u32(air.phase2.w + 0.5);
    // The light, its delta and its applied scalar (the CPU side runs this pass for them only
    // while the light's gate is open and the fog blends in display values).
    if code == VIEW_NONE || code == VIEW_DELTA || code == VIEW_RAYS {
        // The phase of the pixel's own ray against the light, applied once (floored).
        let ray_world = (air.world_from_view * vec4<f32>(ray_view, 0.0)).xyz;
        let p = air_phase(dot(ray_world / ray_len, air.light.xyz));
        var px: AirPixel;
        px.uv = uv;
        px.ray_len = ray_len;
        px.p_eff = max(max(p, 0.0), air_floor());
        px.v_ref = air_reference();
        px.scale = air.phase2.z * air_deck_share();
        // Held before the transfer: an infinity times a scalar of 0 would be no number.
        px.src_d = pow(air_held(src.rgb), vec3<f32>(1.0 / DISPLAY_GAMMA));
        px.fog_d = pow(max(air.fog_colour.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / DISPLAY_GAMMA));
        px.src_peak = max(px.src_d.r, max(px.src_d.g, px.src_d.b));
        px.fog_peak = max(px.fog_d.r, max(px.fog_d.g, px.fog_d.b));
        let result = air_pixel(px, pixel);
        if code == VIEW_DELTA {
            return vec4<f32>(air_held(air_grey(0.5 + result.w * DELTA_GAIN)), 1.0);
        }
        if code == VIEW_RAYS {
            return vec4<f32>(air_held(vec3<f32>(0.2 + 2.0 * result.w)), 1.0);
        }
        return vec4<f32>(air_held(result.rgb), src.a);
    }
    // The debug views read sample 0.
    let s = air_sample_at(pixel, 0u, ray_len);
    if code == VIEW_LIT {
        let nm = air_sums(uv, s);
        return vec4<f32>(air_held(air_fraction_grey(air_lit_fraction(nm.r, nm.g))), 1.0);
    }
    if code == VIEW_SHARE {
        return vec4<f32>(air_held(air_grey(air_fog_share(s.distance))), src.a);
    }
    // The other views show nothing on the sky and beyond the grid's end.
    if s.cleared || s.view_depth >= air.grid.w {
        return src;
    }
    let c = air_coord(s.view_depth);
    let k = min(u32(floor(c)), u32(air.grid.z) - 1u);
    if code == VIEW_VISIBILITY {
        // Slice k's own V (the b channel at the slice's centre, nearest in depth).
        let w = (f32(k) + 0.5) / air.grid.z;
        let v = textureSampleLevel(volume, volume_sampler, vec3<f32>(uv, w), 0.0).b;
        return vec4<f32>(air_held(mix(src.rgb, air_fraction_grey(v), 0.5)), src.a);
    }
    if code == VIEW_SLICE {
        return vec4<f32>(air_held(mix(src.rgb, air_slice_tint(k), 0.5)), src.a);
    }
    return src;
}
