// Water surface (WATR), see water.rs for the model and docs/spec-notes/world.md §6 for
// which parts are documented and which are hypotheses.
//
// Bevy world space: y up. Game space (x, y, z) = Bevy (x, -z, y).

import bevy_pbr::{
    render::forward_io::VertexOutput,
    render::mesh_functions::get_tag,
    render::mesh_view_bindings::{view, globals, lights, view_transmission_texture, view_transmission_sampler},
    render::pbr_types,
    render::pbr_functions,
    render::view_transformations::{ndc_to_uv, position_ndc_to_world},
};
@if(DEPTH_PREPASS) import bevy_pbr::prepass::utils as prepass_utils;
@if(DEPTH_PREPASS) import bevy_pbr::render::mesh_view_bindings::depth_prepass_texture;
// The sun visibility (nvforever sun_visibility.rs): clouds (and later terrain) on the
// sun's highlight, only while the view has the plumbing.
@if(NVF_SUN_VISIBILITY) import bevy_pbr::render::mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT;
@if(NVF_SUN_VISIBILITY) import nvforever::sun_visibility::nvf_sun_visibility;
import nvforever::caustics::nvf_caustics_filtered;

// During temporal upscaling, the main pass occupies only the render-sized region
// of full-sized attachments. Geometry UVs and texture UVs are distinct.
fn frag_coord_to_uv(fc: vec2<f32>) -> vec2<f32> {
    return (fc - view.main_pass_viewport.xy) / view.main_pass_viewport.zw;
}
fn frag_coord_to_ndc(fc: vec4<f32>) -> vec3<f32> {
    let uv = frag_coord_to_uv(fc.xy);
    return vec3(uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), fc.z);
}
fn transmission_uv(uv: vec2<f32>) -> vec2<f32> {
    let pixel = view.main_pass_viewport.xy + uv * view.main_pass_viewport.zw;
    let bounded = clamp(pixel, view.main_pass_viewport.xy + 0.5,
        view.main_pass_viewport.xy + view.main_pass_viewport.zw - 0.5);
    return bounded / vec2<f32>(textureDimensions(view_transmission_texture));
}

struct ShotRing {
    point_time: vec4<f32>,
    travel: vec2<f32>,
    receiver: u32,
    kind: u32,
    blast_scale: vec2<f32>,
};

struct WaterParams {
    shallow: vec4<f32>,
    deep: vec4<f32>,
    // rgb = reflection colour, a = 1 when the type is reflective (reflects the sky).
    reflection: vec4<f32>,
    sky_upper: vec4<f32>,
    sky_horizon: vec4<f32>,
    // near, far, amount, 1 when the amount caps the opacity (placed water)
    above_fog: vec4<f32>,
    // near, far, amount, enhanced water effects enabled.
    under_fog: vec4<f32>,
    // reflectivity, fresnel amount, shininess, wave height (world units)
    surface: vec4<f32>,
    // per layer: uv velocity (per second), 1 / uv scale (per world unit), amplitude
    layers: array<vec4<f32>, 3>,
    // x = Bevy y of the plane the reflection texture shows, y = 1 when it is in use,
    // z = 1 when screen-space reflections stand in where it is not (enhanced graphics)
    reflection_plane: vec4<f32>,
    // ripple grid: game x, y of its corner, units per cell, height scale (0: none)
    ripple: vec4<f32>,
    // shore fade: depth falloff start, end (units of water crossed), 1 when in use
    edge: vec4<f32>,
    // x = counted plane height, y = enable, z/w = low/high halves of the probe generation
    seen_plane: vec4<f32>,
    // xyz = direction toward the sun (Bevy world), w = enabled daylight strength.
    water_sun: vec4<f32>,
    shot_rings: array<ShotRing, 8>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> water: WaterParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var noise_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var noise_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var reflection_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var reflection_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var ripple_texture: texture_2d<f32>;
// Reset/tagged before the draw and copied after it (reflection.rs WaterSample).
struct WaterVisibility {
    count: atomic<u32>,
    generation: u32,
    frame: u32,
    time_bits: u32,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var<storage, read_write> water_seen: WaterVisibility;

// Whether opaque surfaces (depth prepass) lie in front of this fragment in every sample
// of its pixel: the depth test would throw the whole pixel away. Reverse z: nearer is
// larger. False without a prepass.
fn hidden_by_scene(fc: vec4<f32>) -> bool {
    @if(DEPTH_PREPASS && MULTISAMPLED) if true {
        let n = textureNumSamples(depth_prepass_texture);
        for (var i = 0u; i < n; i += 1u) {
            if textureLoad(depth_prepass_texture, vec2<i32>(fc.xy), i32(i)) <= fc.z {
                return false;
            }
        }
        return true;
    }
    @if(DEPTH_PREPASS && !MULTISAMPLED) if true {
        return textureLoad(depth_prepass_texture, vec2<i32>(fc.xy), 0) > fc.z;
    }
    return false;
}

// Ripple height at grid position `t` (cells), bilinear (the texture is not filterable).
fn ripple_height(t: vec2<f32>) -> f32 {
    let n = vec2<i32>(textureDimensions(ripple_texture)) - 1;
    let i = vec2<i32>(floor(t));
    let f = t - floor(t);
    let a = textureLoad(ripple_texture, clamp(i, vec2(0), n), 0).r;
    let b = textureLoad(ripple_texture, clamp(i + vec2(1, 0), vec2(0), n), 0).r;
    let c = textureLoad(ripple_texture, clamp(i + vec2(0, 1), vec2(0), n), 0).r;
    let d = textureLoad(ripple_texture, clamp(i + vec2(1, 1), vec2(0), n), 0).r;
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// Slope (game space) of the ripples at game position `g`, faded out towards the grid edge.
fn ripple_slope(g: vec2<f32>) -> vec2<f32> {
    if water.ripple.w <= 0.0 {
        return vec2(0.0);
    }
    let size = f32(textureDimensions(ripple_texture).x);
    let t = (g - water.ripple.xy) / water.ripple.z;
    let edge = min(min(t.x, t.y), min(size - t.x, size - t.y));
    let fade = clamp((edge - 2.0) / 24.0, 0.0, 1.0);
    if fade <= 0.0 {
        return vec2(0.0);
    }
    let dx = ripple_height(t + vec2(1.0, 0.0)) - ripple_height(t - vec2(1.0, 0.0));
    let dy = ripple_height(t + vec2(0.0, 1.0)) - ripple_height(t - vec2(0.0, 1.0));
    return vec2(dx, dy) / (2.0 * water.ripple.z) * water.ripple.w * fade;
}

// Derivative of a compact bell-shaped crest. Its two slopes bend the reflected sky
// and the lakebed in opposite directions as the crest passes a fixed point.
fn crest_slope(offset: f32, width: f32) -> f32 {
    let u = clamp(offset / width, -1.0, 1.0);
    let v = 1.0 - u * u;
    return -u * v * v;
}

// Two groups of drops from the column land at different times and positions.
// Their tiny rings use the same surface normal as the outgoing wave.
fn fallout_slope(d: vec2<f32>, age: f32) -> vec2<f32> {
    if age <= 0.0 || age >= 0.65 {
        return vec2(0.0);
    }
    let radius = 1.0 + age * 30.0;
    let r2 = dot(d, d);
    if r2 > (radius + 3.5) * (radius + 3.5) {
        return vec2(0.0);
    }
    let r = sqrt(r2);
    return d / max(r, 1.0) * crest_slope(r - radius, 3.5) * (1.0 - smoothstep(0.12, 0.65, age));
}

// xy is the refracting slope and z is a signed crest/trough contrast.
fn blast_wave(delta: vec2<f32>, age: f32, footprint: f32, entry: ShotRing) -> vec3<f32> {
    let px = footprint / entry.blast_scale.x;
    if px >= 23.0 || age >= 3.4 {
        return vec3(0.0);
    }
    let d = delta / entry.blast_scale.x;
    let width = 14.0 + 3.0 * age;
    let front = 12.0 + age * 80.0 - 14.0 * max(age - 1.2, 0.0);
    let r2 = dot(d, d);
    // Both crests fit in this annulus, including the angular offset. The two
    // groups of drops can only disturb the inner 57 units during their lifetimes.
    let inner = max(front - 21.0 - width - 4.0, 0.0);
    let outer = front + width + 4.0;
    let near_crest = r2 >= inner * inner && r2 <= outer * outer;
    let near_fallout = px < 10.0 && age > 0.57 && age < 1.50 && r2 < 57.0 * 57.0;
    if !near_crest && !near_fallout {
        return vec3(0.0);
    }
    let axis = entry.travel;
    let side = vec2(-axis.y, axis.x);
    var slope = vec2(0.0);
    var contrast = 0.0;
    if near_crest {
        let r = sqrt(r2);
        let n = d / max(r, 1.0);
        let a = dot(n, axis);
        let b = dot(n, side);
        // Four uneven arcs near the column join into a longer front as it travels.
        let arc = (a * a - b * b) * (2.0 * a * b);
        let youth = 1.0 - smoothstep(0.24, 1.10, age);
        let spread = r - front - 6.0 * arc * youth;
        let broken = 1.0 - youth * (1.0 - smoothstep(-0.48, 0.04, arc + 0.2 * a));
        let leading = crest_slope(spread, width);
        let crest = leading + 0.23 * crest_slope(spread + 21.0, width * 0.7);
        let decay = (1.0 - smoothstep(0.35, 3.4, age)) / (1.0 + age * 0.9);
        let resolved = 1.0 - smoothstep(7.0, 23.0, px);
        slope = n * crest * broken * decay * resolved;
        // Keep the wave readable after the authored water column and its bright sheet clear.
        let visible = smoothstep(0.64, 1.55, age) * (1.0 - smoothstep(2.55, 3.4, age));
        contrast = leading * broken * resolved * visible * 1.65;
    }
    if near_fallout {
        let fallout = fallout_slope(d - axis * 31.0, age - 0.57)
            + fallout_slope(d + axis * 18.0 - side * 26.0, age - 0.85);
        slope += 0.36 * fallout * (1.0 - smoothstep(3.0, 10.0, px));
    }
    return vec3(slope, contrast) * entry.blast_scale.y;
}

fn shot_rings(g: vec2<f32>, surface_z: f32, t: f32, instance_index: u32) -> vec3<f32> {
    if water.shot_rings[0].point_time.w == 0.0 {
        return vec3(0.0);
    }
    let footprint = max(length(dpdx(g)), length(dpdy(g)));
    let resolved = 1.0 - smoothstep(3.0, 10.0, footprint);
    // Placed pool meshes carry their reference ID; loaded and distant cell water share zero.
    let receiver = get_tag(instance_index);
    var disturbance = vec3(0.0);
    for (var i = 0u; i < 8u; i += 1u) {
        let entry = water.shot_rings[i];
        if entry.point_time.w == 0.0 {
            break;
        }
        // Nearby stacked water surfaces can share a material.
        if receiver != entry.receiver || abs(surface_z - entry.point_time.z) > 1.0 {
            continue;
        }
        var age = t - (entry.point_time.w - 1.0);
        if age < -1800.0 {
            age += 3600.0; // Bevy Globals.time wraps each hour.
        }
        age = max(age, 0.0);
        let d = g - entry.point_time.xy;
        if entry.kind == 2u {
            disturbance += blast_wave(d, age, footprint, entry);
            continue;
        }
        let size = select(1.0, 1.35, entry.kind == 1u);
        let front = (3.0 + 54.0 * age) * size;
        if dot(d, d) > (front + 8.0 * size) * (front + 8.0 * size) {
            continue;
        }
        let r = length(d);
        let outward = d / max(r, 1.0);
        // An oblique entry initially pushes the leading crest further along the shot.
        let bias = min(3.0, age * 18.0) * (1.0 - smoothstep(0.16, 0.65, age));
        let spread = (r - front) / size - dot(outward, entry.travel) * bias;
        let pair = crest_slope(spread, 4.5) + 0.42 * crest_slope(spread + 9.0, 3.5);
        let decay = (1.0 - smoothstep(0.12, 1.2, age)) / (1.0 + age);
        disturbance += vec3(outward * (0.64 * size * pair * decay * resolved), 0.0);
    }
    return disturbance;
}

// Distance along the wavy reflected ray at which the reflection texture is sampled: the
// texture has no depth, so the wave offset is that of a point this far away [tuned by eye].
const REFLECTION_SAMPLE_DISTANCE: f32 = 500.0;

// Refraction: the scene behind the surface is sampled this far (in screen UV per unit of
// view-space normal tilt) from the pixel, growing to that over this much water
// [tuned by eye; the engine's refraction rule is unknown].
const REFRACTION_STRENGTH: f32 = 0.08;
const REFRACTION_DEPTH: f32 = 200.0;

// Water effects: the bed seen from above takes the water's shallow hue, down to this
// share of its brightness, over this length of ray in the water [design, by eye].
const SHALLOW_TINT_LENGTH: f32 = 90.0;
const SHALLOW_TINT_FLOOR: f32 = 0.62;

// The opaque surface behind screen position `fc` (depth prepass): world position, and
// w = 1 when there is one (0 for the sky or without a prepass).
fn scene_point(fc: vec2<f32>) -> vec4<f32> {
    @if(DEPTH_PREPASS) let d = prepass_utils::prepass_depth(vec4(fc, 0.0, 1.0), 0u);
    @if(DEPTH_PREPASS) if d > 0.0 {
        let ndc = frag_coord_to_ndc(vec4(fc, d, 1.0));
        return vec4(position_ndc_to_world(ndc), 1.0);
    }
    return vec4(0.0);
}

// Screen-space reflections (enhanced graphics, graphics-enhanced.md §3.8): where the
// mirrored camera does not show this plane (water at another height, interiors), the
// reflected ray is marched through the depth prepass and the opaque picture (the
// transmission copy) is taken where it passes behind something. Steps grow by SSR_GROWTH
// from SSR_FIRST units, SSR_STEPS of them (about 9,000 units in all), then four halvings
// [engineering choices, by eye]. Returns the colour (display values: fogged and
// tonemapped, as the copy holds them) and in w how much to trust it (0: no hit).
const SSR_STEPS: i32 = 28;
const SSR_FIRST: f32 = 24.0;
const SSR_GROWTH: f32 = 1.16;
fn ssr(pos: vec3<f32>, R: vec3<f32>) -> vec4<f32> {
    @if(DEPTH_PREPASS) let o = (view.view_from_world * vec4(pos, 1.0)).xyz;
    @if(DEPTH_PREPASS) let d = normalize((view.view_from_world * vec4(R, 0.0)).xyz);
    @if(DEPTH_PREPASS) let size = view.main_pass_viewport.zw;
    @if(DEPTH_PREPASS) var t0 = 0.0;
    @if(DEPTH_PREPASS) var t = SSR_FIRST;
    @if(DEPTH_PREPASS) var stride = SSR_FIRST;
    @if(DEPTH_PREPASS) for (var i = 0; i < SSR_STEPS; i++) {
        let p = o + d * t;
        if p.z >= -1.0 {
            return vec4(0.0);
        }
        let clip = view.clip_from_view * vec4(p, 1.0);
        let uv = ndc_to_uv(clip.xy / clip.w);
        if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
            return vec4(0.0);
        }
        let scene = prepass_utils::prepass_depth(vec4(view.main_pass_viewport.xy + uv * size, 0.0, 1.0), 0u);
        // Distance along the view axis of the ray's point and of the scene there
        // (reverse infinite z: distance = near / depth; the sky, depth 0, is never hit).
        let ray_z = -p.z;
        let near = view.clip_from_view[3][2];
        let scene_z = select(1.0e9, near / scene, scene > 0.0);
        // Behind the scene, by less than a step's length (else it passed behind an object
        // it does not touch).
        if ray_z > scene_z && ray_z - scene_z < stride * 2.0 + 20.0 {
            // Halve the last step four times.
            var lo = t0;
            var hi = t;
            for (var k = 0; k < 4; k++) {
                let m = 0.5 * (lo + hi);
                let pm = o + d * m;
                let cm = view.clip_from_view * vec4(pm, 1.0);
                let um = ndc_to_uv(cm.xy / cm.w);
                let sm = prepass_utils::prepass_depth(vec4(view.main_pass_viewport.xy + um * size, 0.0, 1.0), 0u);
                if sm > 0.0 && -pm.z > near / sm {
                    hi = m;
                } else {
                    lo = m;
                }
            }
            let ph = o + d * hi;
            let ch = view.clip_from_view * vec4(ph, 1.0);
            let uh = ndc_to_uv(ch.xy / ch.w);
            let c = textureSampleLevel(view_transmission_texture, view_transmission_sampler, transmission_uv(uh), 0.0).rgb;
            // Less trust near the picture's edges and towards the end of the march.
            let edge = min(min(uh.x, 1.0 - uh.x), min(uh.y, 1.0 - uh.y));
            let trust = smoothstep(0.0, 0.08, edge) * (1.0 - smoothstep(0.6, 1.0, f32(i) / f32(SSR_STEPS)));
            return vec4(c, trust);
        }
        t0 = t;
        stride *= SSR_GROWTH;
        t += stride;
    }
    return vec4(0.0);
}

// Slope (dh/dx, dh/dy in game space) of one noise layer: height = texture channel `mask`
// (0..1) times the layer amplitude times the wave height.
fn layer_slope(g: vec2<f32>, layer: vec4<f32>, mask: vec4<f32>, t: f32) -> vec2<f32> {
    let uv = g * layer.z + layer.xy * t;
    // The slope from three taps round the point, a texel apart near the camera and a
    // pixel's footprint apart beyond (a single texel's step there moved a sharp
    // reflection several pixels as the waves travelled). A wider span was tried and
    // flattened the short waves: two texels each side keeps 0.64 of an 8-texel wave.
    let dx = dpdx(uv);
    let dy = dpdy(uv);
    let e = max(1.0 / f32(textureDimensions(noise_texture).x), 0.5 * max(length(dx), length(dy)));
    let a = vec2(1.0, 0.0);
    let b = vec2(-0.5, 0.8660254);
    let c = vec2(-0.5, -0.8660254);
    let ha = dot(textureSampleGrad(noise_texture, noise_sampler, uv + e * a, dx, dy), mask);
    let hb = dot(textureSampleGrad(noise_texture, noise_sampler, uv + e * b, dx, dy), mask);
    let hc = dot(textureSampleGrad(noise_texture, noise_sampler, uv + e * c, dx, dy), mask);
    // d(uv)/d(world) = layer.z
    return (2.0 / (3.0 * e)) * (ha * a + hb * b + hc * c) * layer.z * layer.w * water.surface.w;
}

fn sky_colour(y: f32) -> vec3<f32> {
    // The same gradient as the sky dome above the horizon (sky.rs).
    return mix(water.sky_horizon.rgb, water.sky_upper.rgb, sqrt(clamp(y / 0.5, 0.0, 1.0)));
}

// [design] Use the reconstructed receiver normal so a vertical wall makes
// less wash. Reject widely separated samples at depth discontinuities.
fn shoreline_bed_fade(bed: vec3<f32>) -> f32 {
    let dx = dpdx(bed);
    let dy = dpdy(bed);
    let normal = cross(dx, dy);
    let upward = abs(normal.y) / max(length(normal), 1e-4);
    let support = 1.0 - smoothstep(9.0, 28.0, max(length(dx), length(dy)));
    return smoothstep(0.30, 0.82, upward) * support;
}

// [design] A broken wash advances and retreats over the undistorted bed depth.
// World-space phase and breakup keep sparse patches attached to the shallows.
fn shoreline_foam(g: vec2<f32>, depth: f32, t: f32) -> f32 {
    let phase = dot(g, vec2(0.023, -0.018));
    let wash = 0.5 + 0.5 * sin(2.1 * t + 1.4 * sin(phase));
    let crest_depth = mix(2.5, 12.0, wash);
    let crest = 1.0 - smoothstep(0.7, 5.6, abs(depth - crest_depth));
    let flow = g + t * vec2(2.2, -1.1);
    let breakup = sin(dot(flow, vec2(0.072, 0.049)))
        + 0.55 * sin(dot(flow, vec2(-0.051, 0.083)) + 0.7 * sin(phase));
    let patches = smoothstep(0.16, 1.22, breakup);
    return 0.24 * crest * smoothstep(0.8, 3.2, depth) * patches * mix(0.35, 1.0, wash);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let pos = in.world_position.xyz;
    // The counter's writes turn off the GPU's early depth test, so water behind the
    // scene is thrown away here, before the costly part, as the depth test would. What
    // is left counts, at the reflected height, one pixel in 16 (#1060).
    //
    // Keep the texture sampling below in helper functions (layer_slope and the rest):
    // naga checks uniform control flow per function, and a textureSample written
    // directly in fragment() after this discard would fail validation, so water would
    // stop drawing.
    if hidden_by_scene(in.position) {
        discard;
    }
    // Keep the cost at one candidate pixel in 16 per frame. Rotate the grid so a
    // thin strip of water cannot stay between the same four columns or rows forever.
    let grid = vec2<u32>(in.position.xy) & vec2(3u);
    let phase = vec2(globals.frame_count & 3u, (globals.frame_count >> 2u) & 3u);
    if water.seen_plane.y > 0.5 && abs(pos.y - water.seen_plane.x) < 2.0 && all(grid == phase) {
        let generation = u32(water.seen_plane.z) | (u32(water.seen_plane.w) << 16u);
        // A material awaiting GPU preparation can still have the previous probe's uniform.
        if generation == water_seen.generation {
            atomicAdd(&water_seen.count, 1u);
        }
    }
    let to_cam = view.world_position.xyz - pos;
    let dist = max(length(to_cam), 1e-3);
    let V = to_cam / dist;
    let above = V.y >= 0.0;

    let g = vec2(pos.x, -pos.z);
    let t = globals.time;
    let behind0 = scene_point(in.position.xy);
    var s = layer_slope(g, water.layers[0], vec4(1.0, 0.0, 0.0, 0.0), t);
    s += layer_slope(g, water.layers[1], vec4(0.0, 1.0, 0.0, 0.0), t);
    s += layer_slope(g, water.layers[2], vec4(0.0, 0.0, 1.0, 0.0), t);
    let shore_depth = pos.y - behind0.y;
    var shore_foam = 0.0;
    // A thin placed pool can be shallow everywhere. Cell water supplies the shoreline.
    if water.under_fog.w > 0.5 && water.above_fog.w < 0.5 {
        let bed_fade = shoreline_bed_fade(behind0.xyz);
        if above && behind0.w > 0.5 && shore_depth > 0.0 && shore_depth < 40.0 && dist < 2200.0 {
            let near = 1.0 - smoothstep(1200.0, 2200.0, dist);
            let wind_scale = mix(0.48, 1.0, smoothstep(0.0, 40.0, shore_depth));
            s *= mix(1.0, wind_scale, near * bed_fade);
            shore_foam = shoreline_foam(g, shore_depth, t) * (near * bed_fade);
        }
    }
    s += ripple_slope(g);
    let shot = shot_rings(g, pos.y, t, in.instance_index);
    s += shot.xy;
    // Game-space normal (-sx, -sy, 1) in Bevy space.
    var N = normalize(vec3(-s.x, 1.0, s.y));
    var up = vec3(0.0, 1.0, 0.0);
    if !above {
        N = -N;
        up = -up;
    }

    // The length of the view ray inside the water: from above, from the surface to the
    // opaque surface behind it (depth prepass), so 0 at the waterline; from below, from
    // the camera to the surface. It sets the body's opacity, the refraction and the shore
    // fade.
    //
    // The fog depth: from above, how far below the surface that opaque surface lies
    // (the above fog's near and far planes are "the distance in game units below the
    // surface of the water" of the shallow and the deep colour plane [documented: GECK
    // wiki WaterType]); it sets the body's colour, so a shallow pool keeps its shallow
    // colour at a grazing look;
    // from below, the distance from the camera (the under fog's planes are distances
    // "from the player" [documented: same page]). Nothing behind: the far end of both.
    let size = view.main_pass_viewport.zw;
    let uv0 = frag_coord_to_uv(in.position.xy);
    var receiver = behind0;
    var fog_distance = 1.0e7;
    var fog_depth = 1.0e7;
    if behind0.w > 0.5 {
        fog_distance = distance(behind0.xyz, pos);
        fog_depth = max(pos.y - behind0.y, 0.0);
    }
    // Refraction: shift the sample by the wave normal's tilt in view space, more through
    // deeper water; keep the unshifted sample where the shifted one lands on something in
    // front of the water (it would bleed into the surface).
    let tilt = (view.view_from_world * vec4(N - up, 0.0)).xy;
    let depth_scale = clamp(min(fog_distance, dist) / REFRACTION_DEPTH, 0.0, 1.0);
    let half_pixel = 0.5 / size;
    var uv = clamp(uv0 + vec2(tilt.x, -tilt.y) * REFRACTION_STRENGTH * depth_scale, half_pixel, 1.0 - half_pixel);
    if above {
        var behind = scene_point(view.main_pass_viewport.xy + uv * size);
        // The offset pixel has its own view ray. Test against the water plane
        // there. Comparing camera distances on two different rays can refract
        // a whole patch of dry shore or a building across the surface.
        if behind.w < 0.5 || behind.y >= pos.y {
            // Near silhouettes the waves repeatedly carry an offset across the
            // edge. Keep the last underwater sample along the offset instead of
            // switching the whole lookup back to its undistorted starting point.
            var outside = uv;
            uv = uv0;
            behind = behind0;
            if behind0.w > 0.5 && behind0.y < pos.y {
                for (var i = 0; i < 3; i += 1) {
                    let mid = 0.5 * (uv + outside);
                    let hit = scene_point(view.main_pass_viewport.xy + mid * size);
                    if hit.w > 0.5 && hit.y < pos.y {
                        uv = mid;
                        behind = hit;
                    } else {
                        outside = mid;
                    }
                }
            }
        }
        if behind.w > 0.5 && behind.y < pos.y {
            receiver = behind;
            // Length below the plane on that ray, from its own entry point.
            fog_depth = pos.y - behind.y;
            let to_receiver = behind.xyz - view.world_position.xyz;
            fog_distance = length(to_receiver) * (fog_depth / max(-to_receiver.y, 1e-3));
        }
    }
    if !above {
        fog_distance = dist;
        fog_depth = dist;
    }
    var refracted = textureSampleLevel(view_transmission_texture, view_transmission_sampler, transmission_uv(uv), 0.0).rgb;
    // Brighten the actual refracted opaque receiver, including non-reflective
    // water types. The existing body opacity/reflection then attenuates the light.
    if above && receiver.w > 0.5 && water.water_sun.w > 0.0 && dist < 4096.0 {
        // The projected light loses definition with viewing distance.
        let fade = 1.0 - smoothstep(2048.0, 4096.0, dist);
        // A pixel's width on the bed, stretched where the bed is seen at a slant (at most
        // six times): waves finer than that fade out instead of sparkling.
        let to_bed = receiver.xyz - view.world_position.xyz;
        let bed_distance = max(length(to_bed), 0.001);
        let footprint = 2.0 * bed_distance / (view.clip_from_view[1][1] * size.y) / max(abs(to_bed.y) / bed_distance, 1.0 / 6.0);
        refracted *= 1.0 + fade * nvf_caustics_filtered(receiver.xyz, pos.y, water.water_sun.xyz, water.water_sun.w, t, footprint);
    }
    let fog = select(water.under_fog, water.above_fog, above);
    // The colour: shallow to deep by the fog depth (#1787). The opacity: by the length of
    // the view ray in the water over the same near to far ramp, at most the above fog's
    // amount for placed water ("controls the transparency of the Deep Color water plane",
    // 1.0 opaque [documented: GECK wiki WaterType]; above_fog.w = 1). Cell and distant
    // water, and any water seen from below, are opaque beyond the far distance: capped,
    // Lake Mead's bed showed through far out [design, until a far-out Lake Mead
    // comparison is made, world.md §6].
    // [hypothesis, checked against the original at the Ultra-Luxe fountain, the
    // Goodsprings Source trough and Boulder Beach, world.md §6]: by the depth alone a
    // 14-unit pool under a 36-unit fog showed its floor where the original is opaque.
    let depth01 = clamp((fog_depth - fog.x) / max(fog.y - fog.x, 1.0), 0.0, 1.0);
    let ray01 = clamp((fog_distance - fog.x) / max(fog.y - fog.x, 1.0), 0.0, 1.0);
    // As the bed descends, scatter more light along the submerged part of the
    // ray. Preserve the shallow end, the authored far distance and PWAT opacity cap.
    var opacity = ray01;
    if above && water.under_fog.w > 0.5 && fog.y > max(fog.x, 0.0) {
        let column = smoothstep(max(fog.x, 0.0), fog.y, fog_depth);
        opacity += (1.0 - ray01) * ray01 * column;
        // The first stretch of water [design, #2674]: by the ramp alone a wading
        // depth of NVCleanWater is under a fifth opaque and the shallows cannot be
        // told from the dry shore. The bed seen through the water takes the water's
        // own shallow hue and darkens over SHALLOW_TINT_LENGTH of ray, and the body
        // gathers along the ramp twice as fast at its thin end (the same far end).
        opacity = 1.0 - (1.0 - opacity) * (1.0 - ray01);
        let hue = water.shallow.rgb / max(max(water.shallow.r, max(water.shallow.g, water.shallow.b)), 1e-3);
        let through = exp(-max(fog_distance, 0.0) / SHALLOW_TINT_LENGTH);
        refracted *= mix(hue * SHALLOW_TINT_FLOOR, vec3(1.0), through);
    }
    let body_alpha = select(opacity, opacity * clamp(water.above_fog.z, 0.0, 1.0), above && water.above_fog.w > 0.5);
    let body_colour = mix(water.shallow.rgb, water.deep.rgb, depth01);

    // The water body is lit like a diffuse surface facing up (from either side).
    var pbr = pbr_types::pbr_input_new();
    // The body and its surface flecks have the same diffuse, zero-reflectance lighting.
    // At covered pixels evaluate white once and apply each albedo before post-processing.
    let has_foam = shore_foam > 0.0;
    pbr.material.base_color = vec4(select(body_colour, vec3(1.0), has_foam), 1.0);
    pbr.material.perceptual_roughness = 1.0;
    pbr.material.metallic = 0.0;
    pbr.material.reflectance = vec3(0.0);
    pbr.frag_coord = in.position;
    pbr.world_position = in.world_position;
    pbr.world_normal = vec3(0.0, 1.0, 0.0);
    pbr.N = vec3(0.0, 1.0, 0.0);
    pbr.V = V;
    pbr.is_orthographic = view.clip_from_view[3].w == 1.0;
    let diffuse = pbr_functions::apply_pbr_lighting(pbr).rgb;
    let lit = select(diffuse, diffuse * body_colour, has_foam);

    // Reflection: the world (reflective types: the mirrored camera's texture when it shows
    // this plane, else the sky gradient) or the reflection colour, weighted by a Schlick
    // fresnel term blended with a constant by the fresnel amount.
    var fr = 0.0;
    var reflected = vec3(0.0);
    // Screen-space reflection where the mirror camera does not cover (display values).
    var traced = vec4(0.0);
    if above {
        let R = reflect(-V, N);
        let schlick = 0.02 + 0.98 * pow(1.0 - clamp(dot(N, V), 0.0, 1.0), 5.0);
        fr = clamp(water.surface.x * mix(1.0, schlick, water.surface.y), 0.0, 1.0);
        // Sampled unconditionally (uniform control flow), used when it shows this plane.
        // The mirror image of the wavy reflected ray's point, seen from the camera; for a
        // flat surface that is this very pixel. Projected without the view's temporal
        // jitter (FSR, TAA): the reflection camera draws without it, so the jittered
        // projection put each lookup up to half a render pixel off, differently in every
        // frame, and the reflections wobbled. Without jitter the two are the same.
        let mirrored = pos + vec3(R.x, -R.y, R.z) * REFLECTION_SAMPLE_DISTANCE;
        let clip = view.unjittered_clip_from_world * vec4(mirrored, 1.0);
        var uv = frag_coord_to_uv(in.position.xy);
        if clip.w > 0.0 {
            uv = ndc_to_uv(clip.xy / clip.w);
        }
        let world_reflection = textureSampleLevel(reflection_texture, reflection_sampler, clamp(uv, vec2(0.0), vec2(1.0)), 0.0).rgb;
        let mirror_here = water.reflection_plane.y > 0.5 && abs(pos.y - water.reflection_plane.x) < 2.0;
        let sky = select(sky_colour(R.y), world_reflection, mirror_here);
        if !mirror_here && water.reflection_plane.z > 0.5 && water.reflection.a > 0.5 {
            traced = ssr(pos, R);
        }
        reflected = select(water.reflection.rgb, sky, water.reflection.a > 0.5);
        if lights.n_directional_lights > 0u {
            // The sun's highlight: directional light 0, or with the sun visibility's
            // plumbing the light flagged as the outdoor sun (none flagged indoors: 0, as
            // without it).
            var sun_index = 0u;
            @if(NVF_SUN_VISIBILITY)
            for (var i = 0u; i < lights.n_directional_lights; i += 1u) {
                if (lights.directional_lights[i].flags & DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT) != 0u {
                    sun_index = i;
                    break;
                }
            }
            let sun = lights.directional_lights[sun_index];
            let spec = pow(max(dot(R, sun.direction_to_light), 0.0), max(water.surface.z, 1.0));
            // In a world view the sun's colour holds display values (nvforever lights.rs
            // DISPLAY_HELPERS): its light is that value raised to the display gamma.
            let sun_light = select(sun.color.rgb * view.exposure, 3.14159265 * pow(max(sun.color.rgb * view.exposure / 3.14159265, vec3(0.0)), vec3(2.2)), lights.ambient_color.a < 0.0);
            // The highlight takes the sun visibility once (no cascade shadow: water casts
            // and receives none, C = 1); the reflection image is left as it is.
            var sun_visibility = 1.0;
            @if(NVF_SUN_VISIBILITY)
            if (sun.flags & DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT) != 0u {
                let view_z = dot(vec4<f32>(view.view_from_world[0].z, view.view_from_world[1].z, view.view_from_world[2].z, view.view_from_world[3].z), vec4<f32>(pos, 1.0));
                sun_visibility = nvf_sun_visibility(pos, sun.direction_to_light, 1.0, view_z);
            }
            reflected += spec * sun_light * sun_visibility;
        }
    } else {
        // From below, the sky shows only within Snell's window (refractive index 1.33:
        // cos > 0.66 from the vertical); outside it the surface reflects the water body.
        let c = clamp(dot(N, V), 0.0, 1.0);
        fr = 1.0 - smoothstep(0.62, 0.70, c);
        reflected = lit;
    }

    // Shore fade (water.rs): from above, the surface fades in over the depth falloff.
    var edge = 1.0;
    if above && water.edge.z > 0.5 {
        edge = clamp((fog_distance - water.edge.x) / max(water.edge.y - water.edge.x, 1e-3), 0.0, 1.0);
    }
    let alpha = (fr + (1.0 - fr) * body_alpha) * edge;
    let premultiplied = (fr * reflected + (1.0 - fr) * body_alpha * lit) * edge;
    var colour = vec4(premultiplied / max(alpha, 1e-4), alpha);
    // Distance fog and (non-HDR) tonemapping as for the standard material, then over the
    // refracted scene (already fogged and tonemapped: a copy of the opaque pass).
    pbr.material.flags = pbr_types::STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT;
    colour = pbr_functions::main_pass_post_lighting_processing(pbr, colour);
    var result = colour.rgb * colour.a + refracted * (1.0 - colour.a);
    // The traced reflection takes the reflected share's place [approximation: mixed in
    // after fog and tonemapping, which the copy already has].
    result = mix(result, traced.rgb, fr * traced.w);
    if above && shot.z != 0.0 {
        result *= 1.0 + clamp(shot.z, -0.21, 0.35) * edge;
    }
    if has_foam {
        let shore_tint = mix(body_colour, vec3(0.52, 0.56, 0.49), 0.58);
        let foam = pbr_functions::main_pass_post_lighting_processing(pbr, vec4(diffuse * shore_tint, 1.0));
        result = mix(result, foam.rgb, shore_foam);
    }
    return vec4(result, 1.0);
}
