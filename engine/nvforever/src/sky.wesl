// Sky parts (sky.rs): Bevy's unlit standard fragment, then the colour is mapped so that
// the camera's tonemapper (Tony McMapface) gives back the sky's display value.
//
// The weather's sky colours are display values that the game grades directly; the
// renderer's lit surfaces go through the tonemapper first. The mapping is the shared
// tonemapper input (nvf_assets tone_input.wesl `to_tonemap_input`), the curve's code in
// sky.glow.w (lights.rs `ToneCurve`). Under Physical lighting the dome's own physical sky
// is linear light already and skips it.

import bevy_pbr::{
    render::pbr_fragment::pbr_input_from_standard_material,
    render::pbr_functions::{alpha_discard, premultiply_alpha},
    render::forward_io::{VertexOutput, FragmentOutput},
    render::pbr_types::{STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD},
    render::mesh_view_bindings::{view, globals},
    render::mesh_functions::get_world_from_local,
};
import nvf_assets::tone_input::to_tonemap_input;

// sky.rs SkyLight: daylight on the sky's colours, and the sun.
struct SkyLight {
    // x scale, y 1 additive weighted by texture alpha (Add) or moon disc linear-alpha/HDR mode (Blend),
    // z 1 if sky daylight applies to this part, w 2 sun disc and glare, 1 glare.
    params: vec4<f32>,
    glow: vec4<f32>,
    sun_dir: vec4<f32>,
    // rgb colour × strength, w tan(half size)
    sun: vec4<f32>,
    glare: vec4<f32>,
    // Night sky detail (night_sky.rs; all zero with its switch off): xyz where the moon disc is
    // drawn from the sky's centre (sky.rs SUN_DISTANCE out), w the Henyey-Greenstein g of the
    // moon's lobe.
    moon: vec4<f32>,
    // By part: the stars x the twinkle's amplitude, y lattice cells per unit of direction;
    // the dome and clouds rgb the moon term's gain, w its cap (0: none).
    night: vec4<f32>,
    // The sun's disc enlarged over the volume sky (sky.rs FairSun): x the magnification of the
    // texture's middle, 0 for none; y the radius of that middle in the texture (uv); z the
    // factor on the sun texture outside the disc.
    disc: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: SkyLight;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var sun_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var sun_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var glare_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var glare_sampler: sampler;

// A texture spread over the angular square of half-size atan(half_tan) around the sun
// (as on a billboard facing the camera), added as colour × alpha.
fn sun_texel(t: texture_2d<f32>, s: sampler, dir: vec3<f32>, half_tan: f32, disc: vec3<f32>) -> vec3<f32> {
    let sd = normalize(sky.sun_dir.xyz);
    let c = dot(dir, sd);
    if c <= 0.0 {
        return vec3(0.0);
    }
    // Point on the plane through the sun's direction, perpendicular to it.
    let p = dir / c - sd;
    var up = vec3(0.0, 1.0, 0.0);
    if abs(sd.y) > 0.999 {
        up = vec3(0.0, 0.0, 1.0);
    }
    let u = normalize(cross(up, sd));
    let v = cross(sd, u);
    let uv = vec2(dot(p, u), -dot(p, v)) / (2.0 * half_tan) + 0.5;
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return vec3(0.0);
    }
    if disc.x > 1.0 {
        // The middle (radius disc.y) drawn disc.x times as large, easing back to the texture's
        // own scale further out; the radius read never falls as the radius drawn grows.
        let q = uv - 0.5;
        let r = length(q);
        let edge = disc.x * disc.y;
        let scale = mix(1.0 / disc.x, 1.0, smoothstep(edge * 1.2, min(edge * 4.0, 0.48), r));
        let w = textureSampleLevel(t, s, q * scale + 0.5, 0.0);
        return w.rgb * w.a * mix(1.0, disc.z, smoothstep(edge * 0.85, edge * 1.15, r));
    }
    let texel = textureSampleLevel(t, s, uv, 0.0);
    return texel.rgb * texel.a;
}


const ATMO_RE: f32 = 6360.0;
const ATMO_RA: f32 = 6460.0;
const ATMO_H0: f32 = 1.1;
const ATMO_HR: f32 = 8.0;
const ATMO_HM: f32 = 1.0;
const ATMO_SUN_IRRADIANCE: f32 = 8.2;
const BETA_R: vec3<f32> = vec3<f32>(4.60e-3, 13.00e-3, 35.50e-3);
const BETA_O: vec3<f32> = vec3<f32>(1.10e-3, 2.35e-3, 0.06e-3);

fn ray_sphere_exit(r: f32, mu: f32, rad: f32) -> f32 {
    let disc = max(r * r * (mu * mu - 1.0) + rad * rad, 0.0);
    return max(-r * mu + sqrt(disc), 0.0);
}

fn physical_sun_transmittance(r: f32, mu_s: f32, beta_m_ext: f32) -> vec3<f32> {
    var earth_vis = 1.0;
    if mu_s < 0.0 {
        let min_r = r * sqrt(max(1.0 - mu_s * mu_s, 0.0));
        earth_vis = smoothstep(ATMO_RE - 6.0, ATMO_RE + 1.0, min_r);
        if earth_vis <= 1e-4 {
            return vec3<f32>(0.0);
        }
    }
    let t_max = ray_sphere_exit(r, mu_s, ATMO_RA);
    var od_r = 0.0;
    var od_m = 0.0;
    var od_o = 0.0;
    var prev_t = 0.0;
    for (var j: u32 = 0u; j < 4u; j++) {
        let u = f32(j + 1u) / 4.0;
        let cur_t = t_max * u * u;
        let dt = cur_t - prev_t;
        let mid_t = 0.5 * (prev_t + cur_t);
        prev_t = cur_t;
        let h = max(sqrt(max(r * r + mid_t * mid_t + 2.0 * r * mid_t * mu_s, 0.0)) - ATMO_RE, 0.0);
        od_r += exp(-h / ATMO_HR) * dt;
        od_m += exp(-h / ATMO_HM) * dt;
        od_o += max(0.0, 1.0 - abs(h - 25.0) / 15.0) * dt;
    }
    let tau = BETA_R * od_r + vec3<f32>(beta_m_ext * od_m) + BETA_O * od_o;
    return exp(-tau) * earth_vis;
}

fn eval_physical_sky(dir: vec3<f32>, sun_dir: vec3<f32>, clearness: f32) -> vec3<f32> {
    let d = normalize(dir);
    let s = normalize(sun_dir);
    let c_clamped = clamp(clearness, 0.0, 1.0);
    var d_sky = d;
    if d.y < 0.004 {
        let h2 = d.x * d.x + d.z * d.z;
        let h_dir = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(d.x, 0.0, d.z) * inverseSqrt(max(h2, 1e-8)), h2 > 1e-8);
        d_sky = normalize(h_dir + vec3<f32>(0.0, 0.004, 0.0));
    }
    let mu_v = max(d_sky.y, 0.004);
    let cos_t = clamp(dot(d_sky, s), -1.0, 1.0);

    let beta_m_s = mix(14.0e-3, 1.0e-3, c_clamped);
    let beta_m_e = beta_m_s * 1.11;
    let sky_pi = 3.14159265;
    let phase_r = (3.0 / (16.0 * sky_pi)) * (1.0 + cos_t * cos_t);
    let g = 0.68 + 0.14 * c_clamped;
    let hg_d = max(1.0 + g * g - 2.0 * g * cos_t, 0.025);
    let phase_m = (3.0 / (8.0 * sky_pi)) * ((1.0 - g * g) * (1.0 + cos_t * cos_t)) / ((2.0 + g * g) * hg_d * sqrt(hg_d));

    let r0 = ATMO_RE + ATMO_H0;
    let t_max = ray_sphere_exit(r0, mu_v, ATMO_RA);
    var accum = vec3<f32>(0.0);
    var trans_v = vec3<f32>(1.0);
    var prev_t = 0.0;
    for (var i: u32 = 0u; i < 8u; i++) {
        let u = f32(i + 1u) / 8.0;
        let cur_t = t_max * u * u;
        let dt = cur_t - prev_t;
        let mid_t = 0.5 * (prev_t + cur_t);
        prev_t = cur_t;
        let p = vec3<f32>(d_sky.x * mid_t, r0 + d_sky.y * mid_t, d_sky.z * mid_t);
        let r = length(p);
        let h = max(r - ATMO_RE, 0.0);
        let rho_r = exp(-h / ATMO_HR);
        let rho_m = exp(-h / ATMO_HM);
        let rho_o = max(0.0, 1.0 - abs(h - 25.0) / 15.0);
        let mu_s = clamp(dot(p, s) / max(r, 1.0), -1.0, 1.0);
        let t_sun = physical_sun_transmittance(r, mu_s, beta_m_e);
        let ext = BETA_R * rho_r + vec3<f32>(beta_m_e * rho_m) + BETA_O * rho_o;
        let step_t = exp(-ext * dt);
        let ms_sun = smoothstep(-0.10, 0.20, mu_s);
        let scat = BETA_R * (rho_r * phase_r) + vec3<f32>(beta_m_s * rho_m * phase_m);
        let ms = (BETA_R * rho_r + vec3<f32>(beta_m_s * rho_m)) * (0.022 / (4.0 * sky_pi) * ms_sun);
        let s_tot = t_sun * scat + ms;
        let integ = (s_tot - s_tot * step_t) / max(ext, vec3<f32>(1e-6));
        accum += trans_v * integ;
        trans_v *= step_t;
    }

    let night_factor = 1.0 - mu_v;
    let night = vec3<f32>(0.0007, 0.0016, 0.0040) * (0.6 + 0.5 * night_factor * night_factor);
    var sky_col = accum * ATMO_SUN_IRRADIANCE + night;
    if d.y < 0.0 {
        let ground_tint = smoothstep(0.0, 0.15, -d.y);
        sky_col = mix(sky_col, sky_col * vec3<f32>(0.30, 0.24, 0.17), ground_tint);
    }
    return sky_col;
}

// ---- Night sky detail (night_sky.rs, docs/design/graphics-enhanced.md §3.19) ----
// An engineering choice for the enhanced look, not the original's behaviour. Every value
// comes from `sky.moon` / `sky.night`, zero with the switch off (the branches are skipped).

const NIGHT_TAU: f32 = 6.2831855;
// Below this added light (linear) a star fragment is not twinkled (most of the sky).
const NIGHT_TWINKLE_SKIP: f32 = 0.0001;

// MurmurHash3's 32-bit finaliser (fmix32, A. Appleby's published constants).
fn night_mix(x: u32) -> u32 {
    var h = x;
    h ^= h >> 16u;
    h *= 0x85ebca6bu;
    h ^= h >> 13u;
    h *= 0xc2b2ae35u;
    h ^= h >> 16u;
    return h;
}

// A lattice cell's hash: the cell's coordinates times Teschner et al.'s primes ("Optimized
// Spatial Hashing for Collision Detection of Deformable Objects", 2003), xor-ed, then mixed.
fn night_cell_hash(c: vec3<i32>) -> u32 {
    let u = bitcast<vec3<u32>>(c);
    return night_mix((u.x * 73856093u) ^ (u.y * 19349663u) ^ (u.z * 83492791u));
}

// One lattice corner's wave at `hour` (the clock's fraction of its one-hour wrap): the mean of
// two sines whose phases and rates (3600..14400 whole cycles an hour, 1 to 4 Hz) come from the
// corner's hash. Whole cycles an hour, so the clock's wrap at 3600 s joins without a jump.
// Mean 0 over time, within -1..1.
fn night_wave(cell: vec3<i32>, hour: f32) -> f32 {
    let h = night_cell_hash(cell);
    let r = night_mix(h ^ 0x9e3779b9u);
    let k1 = f32(3600u + r % 10801u);
    let k2 = f32(3600u + (r >> 16u) % 10801u);
    let p1 = f32(h & 0xffffu) / 65536.0;
    let p2 = f32(h >> 16u) / 65536.0;
    return 0.5 * (sin(NIGHT_TAU * fract(k1 * hour + p1)) + sin(NIGHT_TAU * fract(k2 * hour + p2)));
}

// The sky part's centre in the world: the main camera's place, which every sky part follows
// (sky.rs). Another camera (the water reflection's, mirrored below the water) sees the sky
// from elsewhere, so what belongs to the dome is measured from here, not from the eye.
fn night_centre(instance_index: u32) -> vec3<f32> {
    return get_world_from_local(instance_index)[3].xyz;
}

// The factor on a star's added light along the dome direction `dir` (world axes, from the
// star dome's centre; the dome never turns, so the lattice is anchored to it, not to the
// screen or the eye): lattice value
// noise in time (the corner waves, smoothstep-interpolated over the 3D cell of dir × cells),
// times the amplitude, times 1 - 1/X with X Rozenberg's air mass, 0 at the zenith and 0.975
// at the horizon. Mean 1 over time; within 1 ± amplitude.
fn night_twinkle(dir: vec3<f32>) -> f32 {
    let p = dir * sky.night.y;
    let cell = floor(p);
    let f = p - cell;
    let w = f * f * (3.0 - 2.0 * f);
    let c0 = vec3<i32>(cell);
    let hour = globals.time / 3600.0;
    var s = 0.0;
    for (var i = 0u; i < 8u; i++) {
        let o = vec3<u32>(i & 1u, (i >> 1u) & 1u, (i >> 2u) & 1u);
        let wo = select(vec3(1.0) - w, w, o == vec3(1u));
        s += wo.x * wo.y * wo.z * night_wave(c0 + vec3<i32>(o), hour);
    }
    let up = clamp(dir.y, 0.0, 1.0);
    let air = clamp(1.0 - (up + 0.025 * exp(-11.0 * up)), 0.0, 1.0);
    return 1.0 + clamp(sky.night.x, 0.0, 1.0) * air * s;
}

// The moon's light added along the view ray `dir` to a part whose colour is `base` (1 on the
// dome: the glow; the cloud's own colour on the clouds): `night.rgb` × a Henyey-Greenstein
// lobe about `to_moon`, the ray from the eye to the disc's centre, divided by its value on the
// axis (1 there), the result scaled down so that no channel exceeds `night.w`.
fn night_moon(base: vec3<f32>, dir: vec3<f32>, to_moon: vec3<f32>) -> vec3<f32> {
    if dot(to_moon, to_moon) < 1e-8 {
        return vec3(0.0);
    }
    let g = clamp(sky.moon.w, 0.0, 0.99);
    let cos_t = dot(dir, normalize(to_moon));
    // (1 - g)² + 2 g (1 - cos): 1 + g² - 2 g cos without its cancellation near the axis.
    let d = max((1.0 - g) * (1.0 - g) + 2.0 * g * (1.0 - cos_t), 1e-6);
    let lobe = pow(min((1.0 - g) * (1.0 - g) / d, 1.0), 1.5);
    var add = base * sky.night.rgb * lobe;
    let most = max(add.r, max(add.g, add.b));
    if most > sky.night.w {
        add *= sky.night.w / most;
    }
    return max(add, vec3(0.0));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var c = pbr_input.material.base_color;
    let base_rgb = c.rgb;
    if sky.params.z > 0.5 {
        c = vec4(c.rgb * sky.params.x + sky.glow.rgb, c.a);
    }
    let dir = normalize(in.world_position.xyz - view.world_position.xyz);
    let clearness = sky.sun_dir.w;
    if clearness > 0.0 {
        let sd = normalize(sky.sun_dir.xyz);
        let day_vis = smoothstep(-0.04, 0.25, sd.y);
        let phys_weight = smoothstep(0.10, 0.85, clearness);
        var out: FragmentOutput;
        let alpha_mode = pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS;
        if alpha_mode == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD {
            let a = c.rgb * select(1.0, c.a, sky.params.y > 0.5);
            out.color = vec4(a * 1.35, 1.0);
        } else if sky.params.y > 0.5 {
            let a = clamp(c.a, 0.0, 1.0);
            let moon_alpha = select(0.0, pow(a, 2.6), a > 1e-5);
            out.color = vec4(c.rgb * 4.5, moon_alpha);
        } else if sky.params.w > 1.5 {
            let phys_sky = eval_physical_sky(dir, sd, clearness);
            let scaled_base = select(base_rgb, base_rgb * sky.params.x, sky.params.z > 0.5);
            let weather_sky = scaled_base * mix(0.45, 1.35, day_vis);
            var sky_rad = mix(weather_sky, phys_sky, phys_weight) + sky.glow.rgb;
            let sun_tex = sun_texel(sun_texture, sun_sampler, dir, sky.sun.w, sky.disc.xyz);
            let glare_tex = sun_texel(glare_texture, glare_sampler, dir, sky.glare.w, vec3(0.0));
            let disc_core = smoothstep(0.99978, 0.99993, dot(dir, sd)) * 18.0;
            sky_rad += sky.sun.rgb * (sun_tex * 2.4 + vec3<f32>(disc_core)) + sky.glare.rgb * glare_tex * 1.1;
            out.color = vec4(sky_rad, 1.0);
        } else if sky.params.w > 0.5 {
            let clear_day = day_vis * phys_weight;
            let shaped_a = smoothstep(0.22, 0.72, c.a);
            let cloud_alpha = mix(c.a, shaped_a, clear_day);
            let density_gain = mix(0.65, 2.30, shaped_a);
            let cloud_gain = mix(1.0, density_gain, clear_day) * mix(1.0, 1.55, day_vis * (1.0 - phys_weight));
            let silver = 1.0 + 0.50 * pow(max(dot(dir, sd), 0.0), 8.0) * day_vis;
            let glare_tex = sun_texel(glare_texture, glare_sampler, dir, sky.glare.w, vec3(0.0));
            let cloud_rad = c.rgb * (cloud_gain * silver) + sky.glare.rgb * glare_tex * 1.1;
            out.color = vec4(cloud_rad, cloud_alpha);
        } else {
            out.color = c;
        }
        @if(PREMULTIPLY_ALPHA) out.color = premultiply_alpha(pbr_input.material.flags, out.color);
        return out;
    }
    if sky.params.w > 0.5 {
        // The sky is centred on the camera: the view ray is the fragment's direction.
        var add = sky.glare.rgb * sun_texel(glare_texture, glare_sampler, dir, sky.glare.w, vec3(0.0));
        if sky.params.w > 1.5 {
            add += sky.sun.rgb * sun_texel(sun_texture, sun_sampler, dir, sky.sun.w, sky.disc.xyz);
        }
        // Night sky detail: the moon's glow on the dome (behind the clouds), moonlight on the
        // clouds (their colour; the blend weighs it by the texture's alpha).
        // Aimed from this eye at the disc (drawn at the sky's centre + moon.xyz), so a mirrored
        // eye sees the glow round the disc. The clouds' own colour without the lightning flash
        // in `glow` (the flash lights the sky once, not again through the moon).
        if sky.night.w > 0.0 {
            let to_moon = night_centre(in.instance_index) + sky.moon.xyz - view.world_position.xyz;
            let own = pbr_input.material.base_color.rgb * sky.params.x;
            add += night_moon(select(own, vec3(1.0), sky.params.w > 1.5), dir, to_moon);
        }
        c = vec4(c.rgb + add, c.a);
    }
    var out: FragmentOutput;
    let alpha_mode = pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS;
    if alpha_mode == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD {
        // Additive parts: map what is added (colour, × alpha when params.y is 1), not the colour.
        var a = c.rgb * select(1.0, c.a, sky.params.y > 0.5);
        // Night sky detail: the stars (the additive parts the light does not apply to) twinkle.
        if sky.params.z < 0.5 && sky.night.x > 0.0 && max(a.r, max(a.g, a.b)) > NIGHT_TWINKLE_SKIP {
            let dir = normalize(in.world_position.xyz - night_centre(in.instance_index));
            a *= night_twinkle(dir);
        }
        out.color = vec4(to_tonemap_input(a, sky.glow.w), 1.0);
    } else if sky.params.y > 0.5 {
        // Moon disc (masser_*.dds): authored with a bright background behind a faint DXT5
        // alpha halo intended for D3D9 sRGB blending (~alpha^2.2 in linear light). Convert
        // alpha to linear space so the noisy DXT5 halo disappears; under Tony McMapface
        // (sky.glow.w < 0.5) emit direct HDR lunar radiance so optical Bloom produces a
        // smooth phase-matched halo without tony_inverse asymptote shifts.
        let a = clamp(c.a, 0.0, 1.0);
        let moon_exp = select(2.6, 2.2, sky.glow.w > 0.5);
        let moon_gain = select(4.5, 1.5, sky.glow.w > 0.5);
        let moon_alpha = select(0.0, pow(a, moon_exp), a > 1e-5);
        out.color = vec4(c.rgb * moon_gain, moon_alpha);
    } else {
        out.color = vec4(to_tonemap_input(c.rgb, sky.glow.w), c.a);
    }
    @if(PREMULTIPLY_ALPHA) out.color = premultiply_alpha(pbr_input.material.flags, out.color);
    return out;
}
