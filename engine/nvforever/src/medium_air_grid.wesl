// The volumetric medium's light in the air, the grid pass (medium_air_grid.rs; #3032 stage 3,
// the grid): a compute pass over the froxel grid's columns [documented: Wronski 2014,
// "Volumetric Fog", SIGGRAPH 2014 Advances; Bauer 2019, "Creating the Atmospheric World of Red
// Dead Redemption 2", SIGGRAPH 2019 Advances, slides 42-48]. A column (i, j) is walked front to
// back in four chunks of slices, one invocation each, whose sums are joined in order
// ("The chunks" below; without AIR_CHUNKED in one walk), storing at slice k the sums to the slice's
// far boundary: r = N = sum of dF x V, g = M = sum of dF, b = the slice's own V (the mean of the
// samples read: those whose stratum the fog changes in; 1 with none), a = 1 (with the phase taken out).
// Each visibility sample owns its depth stratum's fog step dF_s, so a narrow fog ramp counts
// the samples inside it, and its tap sits inside the part of its stratum where
// the share changes. The grid has no phase: the composite applies the
// pixel's own phase once to N / M (a column's phase interpolated across columns and divided
// by the pixel's would make shade out of clear air). An engineering choice of NVForever's
// own for the enhanced look, not the original game's behaviour. Entry `air_grid`, workgroup
// 8 x 8 x 4 (8 x 8 columns, four chunks of slices each; 8 x 8 x 1 without AIR_CHUNKED).
//
// Separable pieces (stage 4 adds density sources and the incident light): the visibility
// (`air_visibility`), the medium (`air_fog_share`) and the integration (the loop in
// `air_grid`). The visibility is V = K x min(C, T): the cascade shadow
// C of the outdoor sun light, chosen and compared as Bevy's own lighting does
// for a surface at that view depth (vendored bevy_pbr shadows.wesl
// `fetch_directional_shadow`); the terrain's visibility T from the terrain shadow map
// and the clouds' transmittance K, each read exactly as sun_visibility.wesl
// reads it for surfaces (`nvf_terrain_visibility`, `nvf_cloud_transmittance`: they read their
// own bindings, so they are mirrored here, constants and all); the occluders' code picks which
// parts count. K is the Cloud shadows image's or, in the variant
// AIR_LIGHT_VOLUME, the volumetric clouds' light volume through the surfaces' own reader
// (light_volume_reader.wesl, whose header is the contract), with the image where it has no
// data; the CPU binds that variant only in a frame whose surfaces read the volume
// (medium_air_grid.rs `air_cloud_source`). Without the def the module is the flat path alone:
// bindings 0-7.
//
// Mirrors medium_air.rs to the letter: `AirCurve::depth_at` (air_depth_at), `froxel_ndc` and
// `view_ray` (the column's ray), `fog_share` (air_fog_share); the loop, its chunks and
// their join are mirrored by medium_air_grid.rs's test `column_mirror_at` (slice 0 starts from a
// share of 0, not F(0)).

import bevy_pbr::render::mesh_view_types::{Lights, DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT, DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT};
@if(AIR_LIGHT_VOLUME) import nvforever::light_volume_reader::{LightVolumeWindow, lvr_optical_depth};

struct AirUniform {
    // The camera's world-from-view transform (unjittered).
    world_from_view: mat4x4<f32>,
    // x, y: the projection's x and y scale; z: its near plane; w: the sky's distance.
    proj: vec4<f32>,
    // nx, ny, nz, the grid's depth (view depth, game units).
    grid: vec4<f32>,
    // The slice curve: first (the thickness when linear), r, ln r (0: linear); w: RAYS (the
    // light writer's, not read here).
    curve: vec4<f32>,
    // The fog: near, far, power, strength.
    fog: vec4<f32>,
    // The phase: g, back g, back weight, cap (the composite's; the grid has no phase).
    phase: vec4<f32>,
    // The phase's norm, its largest value, strength x sunlit share, the view's code (the
    // composite's; not read here).
    phase2: vec4<f32>,
    // xyz: the direction to the light (world); w: 1 when a light is to be drawn.
    light: vec4<f32>,
    // rgb: the fog colour (linear); w: 1 for the display-value blend.
    fog_colour: vec4<f32>,
    // x: the occluders' code (0 all, 1 cascades, 2 terrain, 3 none, 4 clouds); y: the
    // visibility samples per froxel (1..8); z: the sky weight; w: the sky's phase share.
    misc: vec4<f32>,
    // The main pass's viewport: origin and size in pixels.
    viewport: vec4<f32>,
    // The terrain map: world xz to uv, xy scale, zw offset (uv = P.xz x scale + offset).
    terrain_uv: vec4<f32>,
    // x: 1 when the map is valid for the current light (else T is 1); y: the dot of its light
    // with the current one; z: its texel size (world units); w: its side (texels).
    terrain: vec4<f32>,
    // xyz: the light the map was computed for (towards the light); w unused.
    terrain_light: vec4<f32>,
    // The Cloud shadows image: world xz to uv, xy scale, zw offset (uv = hit.xz x scale +
    // offset, the sampler repeats).
    cloud_uv: vec4<f32>,
    // x: the cloud plane's height; y: strength (0..1); z, w: the sine of the light's
    // elevation from which the shadow starts (z) and at which it is whole (w).
    cloud: vec4<f32>,
    // x: 1 while the image is published and on the GPU (else K is 1); y: the mean of K over
    // the pattern's repeat for the current light (the composite's); z: the flat
    // layers' own mean transmittance (the composite's); here both only the mean pass's fallbacks.
    // With the light volume the two `air_cloud_mean` took are copied over y and z.
    cloud_mode: vec4<f32>,
}

// The light volume as the surfaces read it this frame (medium_air_grid.rs `AirLightVolume`).
@if(AIR_LIGHT_VOLUME)
struct AirLightVolume {
    // x: the strength (0..1); y, z: the sine of the light's elevation from which the shadow
    // starts and at which it is whole; w: 1 for the volume alone (no image where it has no data).
    cloud: vec4<f32>,
    // The surfaces' window: the levels' places, sizes, presences (light_volume_reader.wesl).
    window: LightVolumeWindow,
}

@group(0) @binding(0) var<uniform> air: AirUniform;
@group(0) @binding(1) var volume: texture_storage_3d<rgba16float, write>;
@group(0) @binding(2) var<uniform> lights: Lights;
@group(0) @binding(3) var shadow_maps: texture_depth_2d_array;
@group(0) @binding(4) var shadow_sampler: sampler_comparison;
// The terrain shadow map (terrain_shadows.rs): Rgba32Float, R the shadow height S, G the
// caster's horizontal distance, B the ground's relief; unfilterable, read with textureLoad.
// A 1 x 1 stand-in while none is published (terrain.x is then 0 and it is never read).
@group(0) @binding(5) var terrain_map: texture_2d<f32>;
// The Cloud shadows image (cloud_shadows.rs): the flat layers' transmittance in R (1 clear,
// 0 opaque), relative to its clear percentile; a 1 x 1 stand-in while none is published
// (cloud_mode.x is then 0 and it is never read). Sampled repeating and linear.
@group(0) @binding(6) var cloud_map: texture_2d<f32>;
@group(0) @binding(7) var cloud_sampler: sampler;
// The light volume: its uniform, its near and far levels (R16Float arrays: the optical depth
// toward the light; sampled with binding 7 inside their windows only, so its repeat is never
// reached; stand-ins, never read, for a level of presence 0) and the mean K (x) the mean entry
// writes.
@if(AIR_LIGHT_VOLUME) @group(0) @binding(8) var<uniform> air_volume: AirLightVolume;
@if(AIR_LIGHT_VOLUME) @group(0) @binding(9) var light_near: texture_2d_array<f32>;
@if(AIR_LIGHT_VOLUME) @group(0) @binding(10) var light_far: texture_2d_array<f32>;
@if(AIR_LIGHT_VOLUME) @group(0) @binding(11) var<storage, read_write> air_mean: vec4<f32>;

// z(c), the view depth of continuous slice coordinate c (medium_air.rs `AirCurve::depth_at`):
// linear `depth x c / nz` when ln r is 0, else `first x (exp(c x ln r) - 1) / (r - 1)`;
// clamped to the grid.
fn air_depth_at(c: f32) -> f32 {
    let nz = max(air.grid.z, 1.0);
    let cc = clamp(c, 0.0, nz);
    var z = air.grid.w * cc / nz;
    if air.curve.z > 0.0 {
        z = air.curve.x * (exp(cc * air.curve.z) - 1.0) / (air.curve.y - 1.0);
    }
    return clamp(z, 0.0, air.grid.w);
}

// The share of fog a surface at distance d (from the camera) is given (medium_air.rs
// `fog_share`, fog.rs GAME_FOG): x = clamp((d - near) / max(far - near, 1), 0, 1);
// x^power x strength for x > 0, else 0.
fn air_fog_share(d: f32) -> f32 {
    let x = clamp((d - air.fog.x) / max(air.fog.y - air.fog.x, 1.0), 0.0, 1.0);
    return select(0.0, pow(x, air.fog.z) * air.fog.w, x > 0.0);
}

// The index of the outdoor sun light: the directional light flagged NVF_OUTDOOR_SUN with its
// shadows on; -1 when there is none (never index 0 or a direction match).
fn air_outdoor_light() -> i32 {
    for (var i = 0u; i < lights.n_directional_lights; i += 1u) {
        let flags = lights.directional_lights[i].flags;
        let wanted = DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT | DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT;
        if (flags & wanted) == wanted {
            return i32(i);
        }
    }
    return -1;
}

// The cascade for view depth z, as Bevy picks it for a surface (shadows.wesl
// `get_cascade_index`): the first whose far bound lies beyond z; the cascade count when none.
fn air_cascade_index(light: u32, z: f32) -> u32 {
    let n = lights.directional_lights[light].num_cascades;
    for (var i = 0u; i < n; i += 1u) {
        if z < lights.directional_lights[light].cascades[i].far_bound {
            return i;
        }
    }
    return n;
}

// One cascade's hardware comparison at a world point (shadows.wesl
// `sample_directional_cascade` and `world_to_directional_light_local`): the point moved
// towards the light by the light's depth bias (air has no surface, so no normal bias), into
// the cascade's light clip space; lit (1) outside its box; else one textureSampleCompareLevel
// (the comparison sampler's 2 x 2 filter) at the cascade's layer.
fn air_cascade_lit(light: u32, cascade: u32, world: vec3<f32>) -> f32 {
    let offset = world + lights.directional_lights[light].shadow_depth_bias * lights.directional_lights[light].direction_to_light;
    let clip = lights.directional_lights[light].cascades[cascade].clip_from_world * vec4<f32>(offset, 1.0);
    if clip.w <= 0.0 {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    if any(ndc.xy < vec2<f32>(-1.0)) || ndc.z < 0.0 || any(ndc > vec3<f32>(1.0)) {
        return 1.0;
    }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    let layer = i32(lights.directional_lights[light].depth_texture_base_index + cascade);
    return textureSampleCompareLevel(shadow_maps, shadow_sampler, uv, layer, ndc.z);
}

// C, the cascade shadow at a world point of view depth z for light `found` (-1: no flagged
// light with shadows, lit): the cascade Bevy picks; within the overlap share of its far end
// mixed towards the next cascade's comparison as Bevy blends them (shadows.wesl
// `fetch_directional_shadow`); lit beyond the last cascade.
fn air_cascades(world: vec3<f32>, z: f32, found: i32) -> f32 {
    if found < 0 {
        return 1.0;
    }
    let light = u32(found);
    let n = lights.directional_lights[light].num_cascades;
    let i = air_cascade_index(light, z);
    if i >= n {
        return 1.0;
    }
    var c = air_cascade_lit(light, i, world);
    if i + 1u < n {
        let far = lights.directional_lights[light].cascades[i].far_bound;
        let next_near = (1.0 - lights.directional_lights[light].cascades_overlap_proportion) * far;
        if z >= next_near {
            c = mix(c, air_cascade_lit(light, i + 1u, world), (z - next_near) / (far - next_near));
        }
    }
    return c;
}

// The terrain receiver's constants, sun_visibility.wesl's `NVF_TERRAIN_*` word for word
// (sun_visibility.rs `TERRAIN_*`): the penumbra per unit of caster distance, its least height
// in texels, the texels of S's fall added along the light, the most tan(elevation) that term
// takes, the receiver's bias (relief times, plus a constant), the texels of the edge fade.
const AIR_TERRAIN_PENUMBRA: f32 = 0.01;
const AIR_TERRAIN_SOFT_MIN: f32 = 0.25;
const AIR_TERRAIN_SOFT_TEXELS: f32 = 0.5;
const AIR_TERRAIN_TAN_MAX: f32 = 1.0;
const AIR_TERRAIN_BIAS_RELIEF: f32 = 2.0;
const AIR_TERRAIN_BIAS_MIN: f32 = 16.0;
const AIR_TERRAIN_EDGE_TEXELS: f32 = 4.0;

// How lit a point at height y (bias included) is against one texel s of the map
// (sun_visibility.wesl `nvf_terrain_lit`).
fn air_terrain_lit(s: vec4<f32>, y: f32, extra: f32, texel: f32) -> f32 {
    let band = max(s.g * AIR_TERRAIN_PENUMBRA, AIR_TERRAIN_SOFT_MIN * texel) + extra;
    let t = saturate((y - s.r) / band + 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// T, the terrain's visibility of the light at P, the light from L (unit, towards the light),
// as sun_visibility.wesl `nvf_terrain_visibility` reads it for a surface: the four texels
// around P (indices held inside the map), P lifted by 16 + 2 x the bilinear relief, the four
// smooth comparisons combined bilinearly, lit fading in over the last four texels at the
// map's edge, 1 outside it; 1 when the map is not valid for this light.
fn air_terrain(P: vec3<f32>, L: vec3<f32>) -> f32 {
    if air.terrain.x < 0.5 {
        return 1.0;
    }
    let texel = air.terrain.z;
    let side = air.terrain.w;
    let x = (P.xz * air.terrain_uv.xy + air.terrain_uv.zw) * side;
    let inside = min(min(x.x, side - x.x), min(x.y, side - x.y));
    let e = saturate(inside / AIR_TERRAIN_EDGE_TEXELS);
    let edge = e * e * (3.0 - 2.0 * e);
    if edge <= 0.0 {
        return 1.0;
    }
    let c = x - vec2<f32>(0.5, 0.5);
    let base = floor(c);
    let f = c - base;
    let last = vec2<i32>(i32(side) - 1, i32(side) - 1);
    let i0 = clamp(vec2<i32>(base), vec2<i32>(0, 0), last);
    let i1 = clamp(vec2<i32>(base) + vec2<i32>(1, 1), vec2<i32>(0, 0), last);
    let t00 = textureLoad(terrain_map, i0, 0);
    let t10 = textureLoad(terrain_map, vec2<i32>(i1.x, i0.y), 0);
    let t01 = textureLoad(terrain_map, vec2<i32>(i0.x, i1.y), 0);
    let t11 = textureLoad(terrain_map, i1, 0);
    let w00 = (1.0 - f.x) * (1.0 - f.y);
    let w10 = f.x * (1.0 - f.y);
    let w01 = (1.0 - f.x) * f.y;
    let w11 = f.x * f.y;
    let relief = w00 * t00.b + w10 * t10.b + w01 * t01.b + w11 * t11.b;
    let y = P.y + AIR_TERRAIN_BIAS_MIN + AIR_TERRAIN_BIAS_RELIEF * relief;
    let tan_e = clamp(L.y / max(length(L.xz), 1.0e-4), 0.0, AIR_TERRAIN_TAN_MAX);
    let extra = AIR_TERRAIN_SOFT_TEXELS * texel * tan_e;
    let lit = w00 * air_terrain_lit(t00, y, extra, texel)
        + w10 * air_terrain_lit(t10, y, extra, texel)
        + w01 * air_terrain_lit(t01, y, extra, texel)
        + w11 * air_terrain_lit(t11, y, extra, texel);
    return mix(1.0, lit, edge);
}

// K from the Cloud shadows image: the share of the light the flat layers let through to P, the
// light from L (unit, towards the light), as sun_visibility.wesl `nvf_cloud_weather` reads it
// for a surface: the ray from P along L meets the cloud plane; the image there, weakened by the
// strength, is K; at or above the plane, or with the light too low (its elevation's sine at most
// cloud.z), 1; fading in between cloud.z and cloud.w. 1 when nothing is published.
fn air_cloud_weather(P: vec3<f32>, L: vec3<f32>) -> f32 {
    if air.cloud_mode.x < 0.5 {
        return 1.0;
    }
    let height = air.cloud.x;
    let up = L.y;
    if up <= air.cloud.z || P.y >= height {
        return 1.0;
    }
    let hit = P.xz + L.xz * ((height - P.y) / up);
    let uv = hit * air.cloud_uv.xy + air.cloud_uv.zw;
    let transmittance = textureSampleLevel(cloud_map, cloud_sampler, uv, 0.0).r;
    let k = saturate(1.0 - air.cloud.y * (1.0 - transmittance));
    return mix(1.0, k, smoothstep(air.cloud.z, air.cloud.w, up));
}

// K without the light volume: the image's.
@if(!AIR_LIGHT_VOLUME)
fn air_clouds(P: vec3<f32>, L: vec3<f32>) -> f32 {
    return air_cloud_weather(P, L);
}

// K with the light volume, as sun_visibility.wesl `nvf_cloud_transmittance` takes it for a
// surface (its debug views aside): exp(-depth) of the optical depth toward the light at P, read
// by the shared reader (no march: one layer of one level for a point under the slab floor, of
// both levels in the near one's guard band), weakened by the strength and faded in over the
// low-sun band; mixed over the image's K by the covered share, the image's alone where the
// volume has no data (nothing, with the volume alone asked for).
@if(AIR_LIGHT_VOLUME)
fn air_clouds(P: vec3<f32>, L: vec3<f32>) -> f32 {
    let only = air_volume.cloud.w > 0.5;
    let up = L.y;
    if up <= air_volume.cloud.y {
        return 1.0;
    }
    let r = lvr_optical_depth(light_near, light_far, cloud_sampler, air_volume.window, P, L);
    var other = 1.0;
    if r.y < 1.0 && !only {
        other = air_cloud_weather(P, L);
    }
    if r.y <= 0.0 {
        return other;
    }
    let k = saturate(1.0 - air_volume.cloud.x * (1.0 - exp(-r.x)));
    let faded = mix(1.0, k, smoothstep(air_volume.cloud.y, air_volume.cloud.z, up));
    return mix(other, faded, r.y);
}

// The light's visibility at a world point of view depth z, 0..1: by the occluders' code
// (misc.x: 0 all, 1 cascades, 2 terrain, 3 none, 4 clouds) the cascade shadow C, the
// terrain's T and the clouds' K; all: V = K x min(C, T); 1 for what is not
// counted.
// `reach` is the last cascade's far bound (0 without a flagged light): a sample beyond it
// reads no cascade.
fn air_visibility(world: vec3<f32>, z: f32, found: i32, reach: f32) -> f32 {
    let code = u32(air.misc.x + 0.5);
    var c = 1.0;
    var t = 1.0;
    var k = 1.0;
    if (code == 0u || code == 1u) && z < reach {
        c = air_cascades(world, z, found);
    }
    if code == 0u || code == 2u {
        t = air_terrain(world, air.light.xyz);
    }
    if code == 0u || code == 4u {
        k = air_clouds(world, air.light.xyz);
    }
    return k * min(c, t);
}

// The most visibility samples per froxel, and the additive recurrence of Roberts 2018 ("The
// unreasonable effectiveness of quasirandom sequences", the R2 sequence: 1 / g and 1 / g^2,
// g the plastic number 1.32471795...) for the samples' lateral pattern.
const AIR_MAX_SAMPLES: u32 = 8u;
const AIR_R2: vec2<f32> = vec2<f32>(0.7548776662, 0.5698402910);

// A fixed value in [0, 1) per column: interleaved gradient noise [documented: Jimenez 2014,
// "Next Generation Post Processing in Call of Duty: Advanced Warfare", SIGGRAPH 2014
// Advances: fract(52.9829189 x fract(0.06711056 x x + 0.00583715 x y)) of a pixel's (x, y)].
fn air_ign(c: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(c, vec2<f32>(0.06711056, 0.00583715))));
}

// The visibility samples per froxel (misc.y, held to 1..8).
fn air_samples() -> u32 {
    return clamp(u32(air.misc.y + 0.5), 1u, AIR_MAX_SAMPLES);
}

// Sample s of `count` as a fraction of its slice's depth: stratified (sample s in
// [s, s + 1) / count), moved off its stratum's centre by the column's dither (o - 0.5) with
// alternating sign, and the last sample of an odd count at its stratum's centre, so the
// samples' mean is the slice's middle exactly (a V linear in depth averages to its value at
// the middle); one sample is the slice's middle. Sample s lies inside stratum s (bounds
// included), whose fog step it carries (`air_stratum_end`).
fn air_sample_depth(s: u32, count: u32, o: f32) -> f32 {
    let centred = (count % 2u == 1u) && (s == count - 1u);
    let sign = select(1.0, -1.0, (s & 1u) == 1u);
    let d = select((o - 0.5) * sign, 0.0, centred);
    return (f32(s) + 0.5 + d) / f32(count);
}

// Sample s's place inside its own stratum as a fraction of it (0.5 + the dither's offset, as
// `air_sample_depth` places it), for a stratum clamped to where its fog changes.
fn air_sample_within(s: u32, count: u32, o: f32) -> f32 {
    let centred = (count % 2u == 1u) && (s == count - 1u);
    let sign = select(1.0, -1.0, (s & 1u) == 1u);
    return 0.5 + select((o - 0.5) * sign, 0.0, centred);
}

// The view depth where stratum s of `count` in the slice [z0, z1] ends: (s + 1) / count of
// the way, and z1 itself for the last stratum (so the slice's last fog evaluation is at its
// boundary exactly, the depth the next slice's first stratum starts from).
fn air_stratum_end(s: u32, count: u32, z0: f32, z1: f32) -> f32 {
    return select(mix(z0, z1, f32(s + 1u) / f32(count)), z1, s + 1u >= count);
}

// Sample s's lateral offset in column widths: the R2 point s, shifted on the torus by the
// column's two values `shift` (x by one, y by the other) and centred, unrotated: each axis in
// [-0.5, 0.5), so every sample stays inside its own column (a turned pattern reached about
// 0.71 and at the screen's edge read visibility outside the frustum). None for one
// sample.
fn air_sample_lateral(s: u32, count: u32, shift: vec2<f32>) -> vec2<f32> {
    if count <= 1u {
        return vec2<f32>(0.0, 0.0);
    }
    return fract(shift + AIR_R2 * (f32(s) + 0.5)) - vec2<f32>(0.5, 0.5);
}

// The chunks [design]: a column's slices are walked by AIR_CHUNKS invocations (the workgroup's z),
// each ceil(nz / AIR_CHUNKS) slices in a row (the last ones fewer or none), at once instead of one
// after the other; each leaves its own N and M in `air_chunk_sums`, and after the barrier each adds
// what the chunks before it left, in order, and stores its slices. A walk is one long chain of
// dependent reads and a grid has few columns, so shorter chains, more of them, can fill a wide GPU
// better [hypothesis]; the work is the same but for the column's own set-up (done four times), one fog share a
// chunk and the kept slices. No subgroup operation, so nothing depends on the subgroup's size.
// Without AIR_CHUNKED a column is one chunk, its slices stored as they are walked: the same walk,
// with no barrier and nothing kept. medium_air_grid.rs `GridWalk::choose` picks by the adapter: the
// chunks were measured faster on one card and slower on another.
// Why the parts join to the one walk's sums: all a later slice takes from the earlier ones besides
// N and M is `before`, the largest fog share so far. It follows the fog alone (a stratum whose
// share does not change skips its visibility read, never its `before`), and a share that does not
// fall along the ray is largest at the depth reached: the chunk's start, z(k0), the very float the
// slice before it ended at. (A share that falls, a fog no light is drawn with, has no step after
// its first, in the walk or in a chunk.) In exact arithmetic the sums are then equal; two things
// differ in f32. The additions are grouped otherwise (rounding). And WGSL gives `pow` an accuracy
// but does not promise that it never falls: where it rounds a farther place's share below a nearer
// one's, the one walk keeps the larger and a chunk starting there the smaller, so a stratum may
// take a step of a few units in the last place, and read its visibility, that the one walk would
// not. In N and M its weight is that small; b, the plain mean of the samples read, can move by a
// whole sample, and only the diagnostic view `visibility` shows it.
// N and M are added by the same operations in the same order, so N = M where all is lit, and M
// still never falls from slice to slice.
// Limits: AIR_CHUNK_SLICES is the most slices a chunk keeps until the barrier, chosen for the
// grid's slices (medium_air_grid.rs `GridWalk`): 16 to 64 slices, with AIR_LONG_CHUNKS 32 to the
// most a grid has (medium_air.rs AIR_SLICES, 128); without AIR_CHUNKED it only names that most.
// 256 invocations and 2,048 bytes of workgroup memory; every entry is written before the barrier,
// so it needs no clearing.
@if(AIR_CHUNKED) const AIR_CHUNKS: u32 = 4u;
@if(!AIR_CHUNKED) const AIR_CHUNKS: u32 = 1u;
@if(AIR_CHUNKED && !AIR_LONG_CHUNKS) const AIR_CHUNK_SLICES: u32 = 16u;
@if(AIR_CHUNKED && AIR_LONG_CHUNKS) const AIR_CHUNK_SLICES: u32 = 32u;
@if(!AIR_CHUNKED) const AIR_CHUNK_SLICES: u32 = 128u;
@if(AIR_CHUNKED) var<workgroup> air_chunk_sums: array<vec2<f32>, 256>;

@compute @workgroup_size(8, 8, AIR_CHUNKS)
fn air_grid(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    let nx = u32(air.grid.x);
    let ny = u32(air.grid.y);
    let nz = u32(air.grid.z);
    // This invocation's slices, [k0, k1): none in a column outside the grid (sides that are no
    // multiple of 8) or past the last slice. No early return: every invocation reaches the barrier.
    // The dispatch is one group deep, so lid.z is the chunk.
    let inside = id.x < nx && id.y < ny;
    let per = min((nz + AIR_CHUNKS - 1u) / AIR_CHUNKS, AIR_CHUNK_SLICES);
    let k0 = lid.z * per;
    let k1 = select(k0, min(k0 + per, nz), inside);
    // The column's centre in NDC (medium_air.rs `froxel_ndc`: x across, y down, row 0 the top)
    // and its view ray per unit of view depth (`view_ray`).
    let ndc = vec2<f32>((f32(id.x) + 0.5) / air.grid.x * 2.0 - 1.0, 1.0 - (f32(id.y) + 0.5) / air.grid.y * 2.0);
    let ray_view = vec3<f32>(ndc.x / air.proj.x, ndc.y / air.proj.y, -1.0);
    let ray_len = length(ray_view);
    let origin = air.world_from_view[3].xyz;
    let ray_world = (air.world_from_view * vec4<f32>(ray_view, 0.0)).xyz;
    let found = air_outdoor_light();
    var reach = 0.0;
    if found >= 0 {
        let cascades = lights.directional_lights[u32(found)].num_cascades;
        if cascades > 0u {
            reach = lights.directional_lights[u32(found)].cascades[cascades - 1u].far_bound;
        }
    }
    // The visibility samples: S per froxel at stratified depths dithered by the
    // column's o0, each on a ray moved inside the column's footprint (the R2 pattern shifted by
    // o1 across and o2 down); the three fixed in time. A sample's ray is the centre ray plus
    // its lateral offset along the view's x and y per unit of view depth (the ray is linear in
    // NDC).
    let count = air_samples();
    let cell = vec2<f32>(f32(id.x), f32(id.y));
    let o0 = air_ign(cell);
    let o1 = air_ign(cell + vec2<f32>(5.588238, 0.0));
    let o2 = air_ign(cell + vec2<f32>(0.0, 5.588238));
    let shift = vec2<f32>(o1, o2);
    let ray_dx = (air.world_from_view * vec4<f32>(2.0 / (air.grid.x * air.proj.x), 0.0, 0.0, 0.0)).xyz;
    let ray_dy = (air.world_from_view * vec4<f32>(0.0, -2.0 / (air.grid.y * air.proj.y), 0.0, 0.0)).xyz;
    var n = 0.0;
    var m = 0.0;
    // The largest fog share so far along the ray. Slice 0's first stratum starts from a share
    // of 0, not F(0): with a fog near below 0 the camera itself has a share, which slice 0
    // owns. Each step is taken against the largest share so far (never negative), so M does
    // not fall and N never exceeds M.
    // A later chunk starts where the slice before it ended, at the share there.
    var before = 0.0;
    var z0 = 0.0;
    @if(AIR_CHUNKED) if k0 > 0u {
        z0 = air_depth_at(f32(k0));
        before = max(air_fog_share(z0 * ray_len), 0.0);
    }
    // The chunk's slices as they will be stored, less what the chunks before add.
    @if(AIR_CHUNKED) var kept: array<vec3<f32>, AIR_CHUNK_SLICES>;
    // Where the fog share can change along this ray, in view depth: radial distances from near
    // to near + max(far - near, 1) (`air_fog_share`'s ramp), divided by the ray's length.
    let fog_lo = air.fog.x / ray_len;
    let fog_hi = (air.fog.x + max(air.fog.y - air.fog.x, 1.0)) / ray_len;
    for (var k = k0; k < k1; k += 1u) {
        let z1 = air_depth_at(f32(k) + 1.0);
        var v_sum = 0.0;
        var read = 0u;
        var za = z0;
        for (var s = 0u; s < count; s += 1u) {
            // The sample's own fog step: its stratum's, so all the fog of a narrow ramp is
            // weighted by the samples inside the ramp. With V = 1 the steps add up to the
            // slice's own step.
            let zb = air_stratum_end(s, count, z0, z1);
            let after = air_fog_share(zb * ray_len);
            let df = max(after - before, 0.0);
            before = max(before, after);
            // No fog change in the stratum: its visibility weighs nothing and is not read.
            if df > 0.0 {
                // The tap inside the part of the stratum where the share changes:
                // a ramp narrower than the stratum is read where it is. An unclamped stratum
                // keeps its place exactly; an empty intersection with a step (slice 0's share
                // of a fog starting behind the camera) keeps it too.
                let lo = max(za, fog_lo);
                let hi = min(zb, fog_hi);
                var z = mix(z0, z1, air_sample_depth(s, count, o0));
                if (lo > za || hi < zb) && lo < hi {
                    z = mix(lo, hi, air_sample_within(s, count, o0));
                }
                let lateral = air_sample_lateral(s, count, shift);
                let ray = ray_world + ray_dx * lateral.x + ray_dy * lateral.y;
                let v = air_visibility(origin + ray * z, z, found, reach);
                n += df * v;
                v_sum += v;
                read += 1u;
            }
            m += df;
            za = zb;
        }
        z0 = z1;
        // b: the mean V of the samples read (1 when the slice holds no fog change).
        let v_mean = select(1.0, v_sum / f32(max(read, 1u)), read > 0u);
        @if(AIR_CHUNKED) kept[k - k0] = vec3<f32>(n, m, v_mean);
        @if(!AIR_CHUNKED) textureStore(volume, vec3<u32>(id.x, id.y, k), vec4<f32>(n, m, v_mean, 1.0));
    }
    @if(AIR_CHUNKED) air_chunk_sums[li] = vec2<f32>(n, m);
    @if(AIR_CHUNKED) workgroupBarrier();
    // What the chunks before this one left (the local index is x + 8 y + 64 z).
    @if(AIR_CHUNKED) var carry = vec2<f32>(0.0, 0.0);
    @if(AIR_CHUNKED) for (var j = 0u; j < lid.z; j += 1u) {
        carry += air_chunk_sums[lid.x + 8u * lid.y + 64u * j];
    }
    @if(AIR_CHUNKED) for (var k = k0; k < k1; k += 1u) {
        let own = kept[k - k0];
        textureStore(volume, vec3<u32>(id.x, id.y, k), vec4<f32>(carry + own.xy, own.z, 1.0));
    }
}

// The mean of K about the camera (the composite's reference visibility and deck's share): the weather image's
// whole-pattern mean as the CPU has it (`cloud_mode.y`; 1 with the volume alone asked for, and
// from the weather plane up, where `air_cloud_weather` is 1) over the uncovered share, plus the
// covered share's own K (`air_volume_part`), both meant over 32 x 32 points 4,000 units apart
// on the camera's level (the far level's node spacing, so nothing of it falls between two
// points), one 8 x 8 group of 4 x 4 points each [design]. The image itself is never sampled here
// (a 4,000-unit lattice aliases its pattern): with no coverage the result is the CPU's mean bit
// for bit, under full coverage the volume's own mean alone, between them continuous. The two sums are
// kept apart (a mean near 0 under a thick deck must not be lost against a bright fallback), and a
// covered mean is held above 0, which the composite would take as no mean. Beside it the clouds' own
// mean transmittance for the deck's share. Dispatched
// once, before the grid, while the composite reads the clouds' mean; its two numbers are copied
// over `cloud_mode.yz` before the grid and the composite read the uniform.
@if(AIR_LIGHT_VOLUME) const AIR_MEAN_SIDE: u32 = 32u;
@if(AIR_LIGHT_VOLUME) const AIR_MEAN_STEP: f32 = 4000.0;
@if(AIR_LIGHT_VOLUME) const AIR_MEAN_LEAST: f32 = 1e-6;
// The deck's transmittance (medium_air.rs `AIR_DECK_UP`, `AIR_DECK_THIN`; the tests compare them).
@if(AIR_LIGHT_VOLUME) const AIR_DECK_UP: f32 = 0.1;
@if(AIR_LIGHT_VOLUME) const AIR_DECK_THIN: f32 = 1e-4;
@if(AIR_LIGHT_VOLUME) var<workgroup> air_mean_parts: array<vec3<f32>, 64>;

// The light volume's part of `air_clouds`' mix at P: x the covered share x the volume's K, y the
// share left to the fallback (1 with no data or the light under the band's foot), z the covered
// share x the clouds' own transmittance, with neither the strength nor the low-sun fade in it.
@if(AIR_LIGHT_VOLUME)
fn air_volume_part(P: vec3<f32>, L: vec3<f32>) -> vec3<f32> {
    let up = L.y;
    if up <= air_volume.cloud.y {
        return vec3<f32>(0.0, 1.0, 0.0);
    }
    let r = lvr_optical_depth(light_near, light_far, cloud_sampler, air_volume.window, P, L);
    if r.y <= 0.0 {
        return vec3<f32>(0.0, 1.0, 0.0);
    }
    let k = saturate(1.0 - air_volume.cloud.x * (1.0 - exp(-r.x)));
    let faded = mix(1.0, k, smoothstep(air_volume.cloud.y, air_volume.cloud.z, up));
    return vec3<f32>(r.y * faded, 1.0 - r.y, r.y * exp(-r.x));
}

@if(AIR_LIGHT_VOLUME)
@compute @workgroup_size(8, 8, 1)
fn air_cloud_mean(@builtin(local_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) index: u32) {
    let origin = air.world_from_view[3].xyz;
    let other = select(air.cloud_mode.y, 1.0, air_volume.cloud.w > 0.5 || origin.y >= air.cloud.x);
    var sum = vec3<f32>(0.0);
    for (var j = 0u; j < 4u; j += 1u) {
        for (var i = 0u; i < 4u; i += 1u) {
            let cell = vec2<f32>(f32(id.x * 4u + i), f32(id.y * 4u + j));
            let at = (cell + 0.5 - 0.5 * f32(AIR_MEAN_SIDE)) * AIR_MEAN_STEP;
            sum += air_volume_part(vec3<f32>(origin.x + at.x, origin.y, origin.z + at.y), air.light.xyz);
        }
    }
    air_mean_parts[index] = sum;
    workgroupBarrier();
    if index == 0u {
        var total = vec3<f32>(0.0);
        for (var n = 0u; n < 64u; n += 1u) {
            total += air_mean_parts[n];
        }
        // x the covered K, y the uncovered share; a share of exactly 1 leaves `other` as it is.
        let mean = total / f32(AIR_MEAN_SIDE * AIR_MEAN_SIDE);
        let mixed = clamp(mean.x + mean.y * other, AIR_MEAN_LEAST, 1.0);
        // The deck (medium_air.rs `deck_transmittance`): the covered share's own transmittance for a
        // light from straight above, the flat layers' over the rest, theirs alone bit for bit with
        // no coverage. The power is of the mean, not of each point's depth before it: of clouds about as
        // tall as wide a lower light meets more, and a point's power would keep an opaque one
        // opaque (medium_air.rs says what was measured, and the limit: a broken deck without height).
        var deck = air.cloud_mode.z;
        if mean.y < 1.0 {
            let covered = 1.0 - mean.y;
            let above = pow(clamp(mean.z / covered, AIR_DECK_THIN, 1.0), clamp(air.light.y, AIR_DECK_UP, 1.0));
            deck = covered * above + mean.y * air.cloud_mode.z;
        }
        air_mean = vec4<f32>(select(other, mixed, mean.y < 1.0), deck, 0.0, 0.0);
    }
}
