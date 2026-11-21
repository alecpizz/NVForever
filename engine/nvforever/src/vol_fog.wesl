// Volumetric fog (vol_fog.rs; docs/design/modern-feel-gfx.md §1): `march` lights the dust
// along each half-size pixel's ray with the sun (its shadow cascades, through Bevy's view
// bindings) and the clustered point and spot lights; `composite` upsamples it by depth and
// adds it to the HDR picture. NVForever's own design; every constant tuned by eye.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
@if(MARCH) import bevy_pbr::render::mesh_view_bindings::{view, lights, globals, clustered_lights};
@if(MARCH) import bevy_pbr::render::mesh_view_types::{DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT, POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE};
@if(MARCH) import bevy_pbr::render::shadows::{get_cascade_index, world_to_directional_light_local};
@if(MARCH) import bevy_pbr::render::shadow_sampling::sample_shadow_map_hardware;
@if(MARCH) import bevy_pbr::render::clustered_forward as clustering;

struct Haze {
    // xyz: towards the sun, w: the sun's share (0: none).
    sun: vec4<f32>,
    // x: density, y: 1 / thinning height above the camera, z: forward scattering g,
    // w: lamps' strength.
    medium: vec4<f32>,
    // x: reach, y: steps, z: 1 to vary the offsets per frame, w: near plane.
    march: vec4<f32>,
    // xy: the half-size target, zw: the picture.
    sizes: vec4<f32>,
}


@if(MARCH) @group(1) @binding(0) var<uniform> haze: Haze;
@if(MARCH && DEPTH_MS) @group(1) @binding(1) var depth: texture_depth_multisampled_2d;
@if(MARCH && !DEPTH_MS) @group(1) @binding(1) var depth: texture_depth_2d;

@if(COMPOSITE) @group(0) @binding(0) var<uniform> haze: Haze;
@if(COMPOSITE && DEPTH_MS) @group(0) @binding(1) var depth: texture_depth_multisampled_2d;
@if(COMPOSITE && !DEPTH_MS) @group(0) @binding(1) var depth: texture_depth_2d;
@if(COMPOSITE) @group(0) @binding(2) var picture: texture_2d<f32>;
@if(COMPOSITE) @group(0) @binding(3) var picture_sampler: sampler;
@if(COMPOSITE) @group(0) @binding(4) var fog: texture_2d<f32>;

const PI: f32 = 3.141592653589793;
// The share of the scattering that is the same every way (the rest forward-peaked) [design].
const ISOTROPIC: f32 = 0.2;

// The reverse depth at `uv` (sample 0).
fn depth_at(uv: vec2<f32>) -> f32 {
    let dims = vec2<i32>(textureDimensions(depth));
    return textureLoad(depth, clamp(vec2<i32>(uv * vec2<f32>(dims)), vec2(0), dims - 1), 0);
}

// Distance along the view axis (game units) of a reverse depth.
fn view_distance(d: f32) -> f32 {
    return min(haze.march.w / max(d, 1.0e-7), 1.0e7);
}

// Henyey-Greenstein, times 4 pi (1 for scattering the same every way), with a share that
// scatters the same every way.
fn phase(g: f32, cos_theta: f32) -> f32 {
    let denom = 1.0 + g * g - 2.0 * g * cos_theta;
    let hg = (1.0 - g * g) / (denom * sqrt(denom));
    return mix(hg, 1.0, ISOTROPIC);
}

// A light colour uniform as the linear light a white surface facing it shows (the
// lights hold display values in a world view: lights.rs).
@if(MARCH)
fn linear_light(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c * view.exposure / PI, vec3(0.0)), vec3(2.2));
}

@if(MARCH)
fn noise(p: vec2<f32>) -> f32 {
    let frame = select(0.0, f32(globals.frame_count % 64u), haze.march.z > 0.5);
    let q = p + 5.588238 * frame;
    return fract(52.9829189 * fract(dot(q, vec2(0.06711056, 0.00583715))));
}

// rgb: the light the dust scatters towards the eye (linear, the picture's units), a: the
// view distance of the surface (for the upsample).
@if(MARCH) @fragment
fn march(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = in.position.xy / haze.sizes.xy;
    let d = depth_at(uv);
    let z_end = view_distance(d);
    let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let near = view.world_from_clip * vec4(ndc, 1.0, 1.0);
    let eye = view.world_position;
    let dir = normalize(near.xyz / near.w - eye);
    let forward = -normalize(view.world_from_view[2].xyz);
    let along = max(dot(dir, forward), 1.0e-3);
    let reach = min(z_end / along, haze.march.x);
    let steps = max(haze.march.y, 1.0);
    let dt = reach / steps;
    let j = noise(in.position.xy);

    // The sun: the directional light towards it, the brightest if several (the arms'
    // shade light shares its direction at a sliver of its strength).
    var sun_index = -1;
    var sun_light = vec3(0.0);
    if haze.sun.w > 0.0 {
        var best = 0.0;
        for (var i = 0u; i < lights.n_directional_lights; i++) {
            let light = &lights.directional_lights[i];
            if dot((*light).direction_to_light, haze.sun.xyz) < 0.999 {
                continue;
            }
            let c = linear_light((*light).color.rgb);
            let l = dot(c, vec3(0.2126, 0.7152, 0.0722));
            if l > best {
                best = l;
                sun_index = i32(i);
                sun_light = c * haze.sun.w;
            }
        }
    }
    let sun_phase = phase(haze.medium.z, dot(dir, haze.sun.xyz));
    let main_viewport = view.main_pass_viewport;
    let cluster_xy = main_viewport.xy + uv * main_viewport.zw;

    var total = vec3(0.0);
    for (var s = 0.0; s < steps; s += 1.0) {
        let t = (s + j) * dt;
        let p = eye + dir * t;
        let density = haze.medium.x * exp(-max(p.y - eye.y, 0.0) * haze.medium.y);
        let amount = density * dt;
        let view_z = -t * along;
        if sun_index >= 0 {
            let li = u32(sun_index);
            let light = &lights.directional_lights[li];
            var lit = 1.0;
            if ((*light).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
                let cascade = get_cascade_index(li, view_z);
                if cascade < (*light).num_cascades {
                    let bias = (*light).shadow_depth_bias * (*light).direction_to_light;
                    let local = world_to_directional_light_local(li, cascade, vec4(p + bias, 1.0));
                    if local.w != 0.0 {
                        lit = sample_shadow_map_hardware(local.xy, local.z, i32((*light).depth_texture_base_index + cascade));
                    }
                }
            }
            total += sun_light * (lit * sun_phase * amount);
        }
        if haze.medium.w > 0.0 {
            let cluster = clustering::view_fragment_cluster_index(cluster_xy, view_z, false);
            let ranges = clustering::unpack_clusterable_object_index_ranges(cluster);
            for (var i = ranges.first_point_light_index_offset; i < ranges.first_rect_light_index_offset; i++) {
                let id = clustering::get_clusterable_object_id(i);
                let light = &clustered_lights.data[id];
                let to_light = (*light).position_radius.xyz - p;
                let d2 = dot(to_light, to_light);
                // The game's falloff (lights.rs), squared for a soft halo.
                let fall = saturate(1.0 - d2 * (*light).color_inverse_square_range.w);
                if fall <= 0.0 {
                    continue;
                }
                let l = to_light * inverseSqrt(max(d2, 1.0e-4));
                var att = fall * fall;
                if i >= ranges.first_spot_light_index_offset {
                    var spot_dir = vec3((*light).light_custom_data.x, 0.0, (*light).light_custom_data.y);
                    spot_dir.y = sqrt(max(0.0, 1.0 - spot_dir.x * spot_dir.x - spot_dir.z * spot_dir.z));
                    if ((*light).flags & POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE) != 0u {
                        spot_dir.y = -spot_dir.y;
                    }
                    let cone = saturate(dot(-spot_dir, -l) * (*light).light_custom_data.z + (*light).light_custom_data.w);
                    att *= cone * cone;
                }
                total += linear_light((*light).color_inverse_square_range.rgb) * (att * phase(haze.medium.z, dot(dir, l)) * amount * haze.medium.w);
            }
        }
    }
    return vec4(total, z_end);
}

// The fog of the 3 x 3 half-size texels around `hp` for a surface at distance `z`, weighted
// by nearness and by how close their surfaces' distances are to `z`.
@if(COMPOSITE)
fn fog_for(hp: vec2<f32>, z: f32) -> vec3<f32> {
    let dims = vec2<i32>(textureDimensions(fog));
    let base = vec2<i32>(round(hp));
    var sum = vec3(0.0);
    var weight = 0.0;
    var nearest = vec3(0.0);
    var nearest_gap = 1.0e20;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let p = clamp(base + vec2(x, y), vec2(0), dims - 1);
            let f = textureLoad(fog, p, 0);
            let off = vec2<f32>(p) - hp;
            let gap = abs(f.a - z);
            let w = exp(-dot(off, off) * 0.7) * exp(-gap / (0.03 * z + 4.0));
            sum += f.rgb * w;
            weight += w;
            if gap < nearest_gap {
                nearest_gap = gap;
                nearest = f.rgb;
            }
        }
    }
    return max(select(nearest, sum / weight, weight > 1.0e-3), vec3(0.0));
}

@if(COMPOSITE && !DEPTH_MS)
fn pixel_fog(hp: vec2<f32>, at: vec2<i32>) -> vec3<f32> {
    return fog_for(hp, view_distance(textureLoad(depth, at, 0)));
}

@if(COMPOSITE && DEPTH_MS)
fn pixel_fog(hp: vec2<f32>, at: vec2<i32>) -> vec3<f32> {
    let n = i32(textureNumSamples(depth));
    var lo = 1.0e20;
    var hi = 0.0;
    for (var i = 0; i < n; i++) {
        let z = view_distance(textureLoad(depth, at, i));
        lo = min(lo, z);
        hi = max(hi, z);
    }
    if hi < lo * 1.05 + 4.0 {
        return fog_for(hp, lo);
    }
    var far = 0.0;
    let mid = 0.5 * (lo + hi);
    for (var i = 0; i < n; i++) {
        far += select(0.0, 1.0, view_distance(textureLoad(depth, at, i)) > mid);
    }
    return mix(fog_for(hp, lo), fog_for(hp, hi), far / f32(n));
}

// The fog added to the picture. In a multisampled view a pixel can hold a near and a far
// surface (a plant's soft edge, the grass's distance fade): the fog of the nearest and the
// farthest sample, mixed by the share of samples nearer the far one, as the resolve mixes
// their colours (sample 0 alone left a dotted pattern over the fading grass).
@if(COMPOSITE) @fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let centre = textureSampleLevel(picture, picture_sampler, uv, 0.0);
    let hp = uv * vec2<f32>(textureDimensions(fog)) - 0.5;
    let ddims = vec2<i32>(textureDimensions(depth));
    let at = clamp(vec2<i32>(uv * vec2<f32>(ddims)), vec2(0), ddims - 1);
    let light = pixel_fog(hp, at);
    return vec4(centre.rgb + light, centre.a);
}
