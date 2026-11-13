// Volumetric atmosphere of the remastered look (atmos.rs, docs/design/graphics-remaster.md
// §3). An engineering choice, not the original game's.
//
// `march` (half resolution): from the camera to the surface (or `atmos.params.w` for the
// sky), 24 jittered steps through a fog that thins with height; at each the sun's light
// reaches the step unless its shadow cascades say something stands between (the sun's
// own shadow maps, so buildings, rocks and trees cast shafts into the air), scattered
// towards the camera by a Henyey-Greenstein phase, plus the ambient light scattered
// evenly. Writes the light gathered (rgb) and how much of what lies behind still shows
// (a), and the texel's distance.
// `temporal`: blended with last frame's at the same place on the screen (the steps' jitter
// averages out over frames).
// `composite` (full resolution, blended into the HDR picture before tonemapping): picture ×
// transmittance + light, from the four half-resolution texels around the pixel, those at
// about the pixel's distance weighed most.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_pbr::render::mesh_view_bindings::{view, lights, globals};
import bevy_pbr::render::mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT;
import bevy_pbr::render::shadow_sampling::sample_shadow_map_hardware;
import bevy_pbr::render::shadows::{get_cascade_index, world_to_directional_light_local};
import nvf_assets::first_person_depth::unsqueeze;

struct Atmos {
    // Last frame's clip space from this frame's world (camera-relative: world minus this
    // frame's camera position).
    prev_clip_from_rel: mat4x4<f32>,
    // x: density at the reference height (per game unit); y: 1 / the height over which it
    // falls to 1/e; z: the reference height; w: the farthest the march goes.
    params: vec4<f32>,
    // x: the phase's anisotropy g; y: sun strength; z: ambient strength; w: frame number.
    params2: vec4<f32>,
    // The picture's size (x, y) and the half-resolution size (z, w).
    size: vec4<f32>,
    // x: the share of the new frame in `temporal`; y: 1 when last frame's textures are
    // valid.
    params3: vec4<f32>,
}

@if(MULTISAMPLED) @group(1) @binding(0) var depth: texture_depth_multisampled_2d;
@if(!MULTISAMPLED) @group(1) @binding(0) var depth: texture_depth_2d;
@group(1) @binding(1) var<uniform> atmos: Atmos;
// temporal: this frame's march; composite: the accumulated result.
@group(1) @binding(2) var tex_a: texture_2d<f32>;
@group(1) @binding(3) var linear_sampler: sampler;
// temporal: last frame's accumulated result; composite: the half-resolution distances.
@group(1) @binding(4) var tex_b: texture_2d<f32>;

const PI: f32 = 3.14159265;
const STEPS: u32 = 24u;
const SKY: f32 = 60000.0;

fn depth_at(p: vec2<i32>) -> f32 {
    let dims = vec2<i32>(textureDimensions(depth));
    return unsqueeze(textureLoad(depth, clamp(p, vec2(0), dims - 1), 0));
}

fn ign(p: vec2<f32>, frame: f32) -> f32 {
    let q = p + 5.588238 * (frame % 64.0);
    return fract(52.9829189 * fract(dot(q, vec2(0.06711056, 0.00583715))));
}

// World position of the picture's point `p` (pixels) at depth `d`.
fn world_at(p: vec2<f32>, d: f32) -> vec3<f32> {
    let ndc = vec2(p.x / atmos.size.x * 2.0 - 1.0, 1.0 - p.y / atmos.size.y * 2.0);
    let w = view.world_from_clip * vec4(ndc, max(d, 1e-7), 1.0);
    return w.xyz / w.w;
}

fn phase_hg(g: f32, cos_theta: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(max(1.0 + g2 - 2.0 * g * cos_theta, 1e-4), 1.5));
}

// The sun's light reaching world point `p` (1 lit, 0 in shadow), from its shadow cascades.
fn sun_visible(sun: u32, p: vec3<f32>) -> f32 {
    let light = &lights.directional_lights[sun];
    if ((*light).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
        return 1.0;
    }
    let view_z = (view.view_from_world * vec4(p, 1.0)).z;
    let cascade = get_cascade_index(sun, view_z);
    if cascade >= (*light).num_cascades {
        return 1.0;
    }
    let local = world_to_directional_light_local(sun, cascade, vec4(p, 1.0));
    if local.w == 0.0 {
        return 1.0;
    }
    return sample_shadow_map_hardware(local.xy, local.z, i32((*light).depth_texture_base_index + cascade));
}

// The sun among the directional lights: the one whose shadow cascades reach furthest
// (the first-person arms' shade light reaches a few metres), else the first.
fn find_sun() -> i32 {
    var best = -1;
    var reach = -1.0;
    for (var i = 0u; i < lights.n_directional_lights; i++) {
        let l = &lights.directional_lights[i];
        var r = 0.0;
        if (*l).num_cascades > 0u {
            r = (*l).cascades[(*l).num_cascades - 1u].far_bound;
        }
        if r > reach {
            reach = r;
            best = i32(i);
        }
    }
    return best;
}

// NVForever remaster (graphics-remaster.md §4): drifting cloud shadows on the sun's light,
// from value noise at a cloud layer 30,000 units up, where the ray to the sun crosses it
// [engineering choice, by eye]. 1 open sky, down to 0.4 under a cloud.
fn nvf_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
fn nvf_value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(nvf_hash(i), nvf_hash(i + vec2(1.0, 0.0)), u.x), mix(nvf_hash(i + vec2(0.0, 1.0)), nvf_hash(i + vec2(1.0, 1.0)), u.x), u.y);
}
fn nvf_cloud_shadow(p: vec3<f32>, to_sun: vec3<f32>, time: f32) -> f32 {
    let up = max(to_sun.y, 0.08);
    let c = p.xz + to_sun.xz / up * (30000.0 - p.y);
    var q = mat2x2<f32>(0.87, 0.5, -0.5, 0.87) * (c / 11000.0) + vec2(time * 0.04, time * 0.016);
    var v = 0.0;
    var a = 0.5;
    for (var o = 0; o < 4; o++) {
        v += a * nvf_value_noise(q);
        // Each octave turned (about 37 degrees) so the lattice of value noise never lines up.
        q = vec2(0.8 * q.x - 0.6 * q.y, 0.6 * q.x + 0.8 * q.y) * 2.03 + vec2(1.7, 9.2);
        a *= 0.5;
    }
    return 1.0 - 0.6 * smoothstep(0.52, 0.64, v);
}

struct MarchOut {
    @location(0) light: vec4<f32>,
    @location(1) dist: vec4<f32>,
}

@fragment
fn march(in: FullscreenVertexOutput) -> MarchOut {
    var out: MarchOut;
    let half = vec2<i32>(floor(in.position.xy));
    let px = half * 2;
    let d = depth_at(px);
    let camera = view.world_position;
    let far_point = world_at(vec2<f32>(px) + 0.5, select(1e-6, d, d > 0.0));
    let to = far_point - camera;
    var dist = length(to);
    out.dist = vec4(select(SKY, min(dist, SKY), d > 0.0), 0.0, 0.0, 0.0);
    let dir = to / max(dist, 1e-3);
    if atmos.params.x <= 0.0 {
        out.light = vec4(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    dist = min(dist, atmos.params.w);
    let frame = atmos.params2.w;
    let jitter = ign(vec2<f32>(half), frame);
    let sun_index = find_sun();
    var sun_colour = vec3(0.0);
    var cos_sun = 0.0;
    if sun_index >= 0 {
        let light = &lights.directional_lights[u32(sun_index)];
        // The light's colour as display values (lights.rs DISPLAY_HELPERS), to linear.
        sun_colour = pow(max((*light).color.rgb * view.exposure / PI, vec3(0.0)), vec3(2.2)) * atmos.params2.y;
        cos_sun = dot(dir, (*light).direction_to_light.xyz);
    }
    // The air's own blue: the ambient light scattered more in blue than in red (Rayleigh's
    // tint, held to the ambient light's brightness) [engineering choice].
    let ambient_lin = pow(max(lights.ambient_color.rgb * view.exposure, vec3(0.0)), vec3(2.2));
    let ambient_lum = dot(ambient_lin, vec3(0.2126, 0.7152, 0.0722));
    let ambient = mix(ambient_lin, ambient_lum * vec3(0.62, 0.92, 1.55), 0.6) * atmos.params2.z;
    let phase = phase_hg(atmos.params2.x, cos_sun);
    // The cloud shadow at the ray's two ends, blended along it (the clouds are kilometres
    // wide; once per step cost a quarter of the pass).
    var cloud_near = 1.0;
    var cloud_far = 1.0;
    if sun_index >= 0 {
        let to_sun = lights.directional_lights[u32(sun_index)].direction_to_light.xyz;
        cloud_near = nvf_cloud_shadow(camera, to_sun, globals.time);
        cloud_far = nvf_cloud_shadow(camera + dir * dist, to_sun, globals.time);
    }
    var light = vec3(0.0);
    var transmittance = 1.0;
    var t0 = 0.0;
    for (var i = 0u; i < STEPS; i++) {
        // More steps near the camera, where shafts are sharpest.
        let f1 = (f32(i) + 1.0) / f32(STEPS);
        let t1 = dist * f1 * f1;
        let t = mix(t0, t1, jitter);
        let p = camera + dir * t;
        // Thin near the camera (about 15 m to 70 m), so what is close stays crisp.
        let near = smoothstep(1000.0, 5000.0, t);
        let density = atmos.params.x * near * exp(-max(p.y - atmos.params.z, 0.0) * atmos.params.y);
        let dt = t1 - t0;
        var sun = vec3(0.0);
        if sun_index >= 0 {
            sun = sun_colour * (phase * sun_visible(u32(sun_index), p) * mix(cloud_near, cloud_far, t / max(dist, 1.0)));
        }
        let step_t = exp(-density * dt);
        // In-scattered light, integrated over the step (energy-conserving form).
        light += transmittance * (sun + ambient / (4.0 * PI)) * (1.0 - step_t);
        transmittance *= step_t;
        t0 = t1;
    }
    out.light = vec4(light, transmittance);
    return out;
}

@fragment
fn temporal(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let half = vec2<i32>(floor(in.position.xy));
    let current = textureLoad(tex_a, half, 0);
    if atmos.params3.y < 0.5 {
        return current;
    }
    let px = half * 2;
    let d = depth_at(px);
    let p = world_at(vec2<f32>(px) + 0.5, select(1e-6, d, d > 0.0)) - view.world_position;
    let c = atmos.prev_clip_from_rel * vec4(p, 1.0);
    if c.w <= 0.0 {
        return current;
    }
    let ndc = c.xy / c.w;
    let uv = vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return current;
    }
    let old = textureSampleLevel(tex_b, linear_sampler, uv, 0.0);
    // Held near this frame's (what moved in front of the light changes it at once).
    let lo = min(current, vec4(0.0, 0.0, 0.0, 0.0)) + current * 0.5;
    let hi = current * 2.0 + vec4(0.02, 0.02, 0.02, 0.05);
    return mix(clamp(old, lo, hi), current, atmos.params3.x);
}

@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let px = vec2<i32>(floor(in.position.xy));
    let d = depth_at(px);
    let camera = view.world_position;
    let z = select(SKY, min(length(world_at(vec2<f32>(px) + 0.5, max(d, 1e-6)) - camera), SKY), d > 0.0);
    let dims = vec2<i32>(textureDimensions(tex_a));
    let base = vec2<i32>(floor((vec2<f32>(px) + 0.5) * 0.5 - 0.5));
    var sum = vec4(0.0);
    var wsum = 0.0;
    var nearest = vec4(0.0, 0.0, 0.0, 1.0);
    var nearest_dz = 1e9;
    for (var y = 0; y <= 1; y++) {
        for (var x = 0; x <= 1; x++) {
            let t = clamp(base + vec2(x, y), vec2(0), dims - 1);
            let v = textureLoad(tex_a, t, 0);
            let tz = textureLoad(tex_b, t, 0).r;
            let dz = abs(tz - z);
            let w = exp(-dz / (0.05 * z + 8.0));
            sum += v * w;
            wsum += w;
            if dz < nearest_dz {
                nearest_dz = dz;
                nearest = v;
            }
        }
    }
    let v = select(nearest, sum / max(wsum, 1e-4), wsum > 1e-3);
    // Blended as picture × a + rgb (atmos.rs `composite` blend state).
    return vec4(v.rgb, saturate(v.a));
}
