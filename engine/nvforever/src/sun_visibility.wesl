// Sun visibility (nvforever sun_visibility.rs, docs/design/sun-visibility.md): a factor
// V in [0, 1] on the outdoor sun's direct light, V = K * Vsolid, with K the cloud
// transmittance and Vsolid the solid occluders' visibility: min(C, T) of the cascade
// shadow C and the terrain visibility T (the darker of the two, at every distance).
//
// Imported by bevy_pbr's `apply_pbr_lighting` (patched in by sun_visibility.rs
// `HOOK_EDITS`) only with the shader def NVF_SUN_VISIBILITY, which the view's pipelines
// have exactly when its mesh view bind group holds bindings 39-42 (bevy_pbr
// mesh_view_bindings.rs `NvfSunVisibilityBindings`). NVF_CLOUD_VISIBILITY and
// NVF_TERRAIN_VISIBILITY switch the two samples on; without them K and T are 1.
//
// Every texture read is textureSampleLevel or textureLoad: the lighting runs in non-uniform
// control flow (per-fragment branches here, and callers such as nif_material.wesl
// `with_shade_light`).

// The cloud shadows from the volumetric clouds' light volume (#3032 stage 2i M2): the shared
// reader of its completed fronts (light_volume_reader.wesl; its header is the contract), only
// with NVF_CLOUD_VOLUME, which the pipelines have exactly when the mesh view layout holds
// bindings 45-46 (without it, the weather-only layout of a small adapter, K is the weather's).
@if(NVF_CLOUD_VOLUME) import nvforever::light_volume_reader::{LightVolumeWindow, lvr_optical_depth};

// Bevy world axes throughout (y up); positions and heights in game units.
struct NvfSunVisibility {
    // World xz to cloud texture uv: xy scale, zw offset (uv = hit.xz * scale + offset).
    cloud_uv: vec4<f32>,
    // x: the cloud plane's height; y: strength (0 no shadow, 1 the texture's full
    // transmittance); z, w: the sine of the light's elevation from which the cloud shadow
    // starts (z) and at which it is whole (w).
    cloud: vec4<f32>,
    // World xz to terrain map uv: xy scale, zw offset (uv 0..1 inside the map).
    terrain_uv: vec4<f32>,
    // x, y: unused (0); z: the map's texel size (world units); w: its side (texels).
    terrain: vec4<f32>,
    // x: feature mask (1 cloud, 2 terrain, 4 cloud from the light volume, 8 the volume alone:
    // no weather layers where it has no data, 16 / 32 the debug views of the reader's covered share
    // / depth over 8 in place of K); y: debug view (0 off, 1 cloud K, 2 terrain T,
    // 3 combined V).
    mode: vec4<u32>,
    // The cloud from the light volume (mask bit 4): x its strength (the weather's, the same
    // setting), y, z the sine of the light's elevation from which its shadow starts and at which
    // it is whole; w unused.
    @if(NVF_CLOUD_VOLUME) volume: vec4<f32>,
    // The light volume's fronts as this frame reads them (light_volume_reader.wesl); the CPU
    // writes both in either variant, the weather-only one leaves them out of its view.
    @if(NVF_CLOUD_VOLUME) light_volume: LightVolumeWindow,
}

@group(0) @binding(39) var<uniform> nvf_sun: NvfSunVisibility;
// Cloud transmittance (R: 1 clear, 0 opaque), repeat-wrapped by the sampler.
@group(0) @binding(40) var nvf_cloud_texture: texture_2d<f32>;
// Terrain shadow-height map (#2671, terrain_shadows.rs): Rgba32Float, R the shadow height
// S, G the caster's horizontal distance, B the ground's relief around the texel (world
// units). Unfilterable: read with textureLoad only, never with the sampler.
@group(0) @binding(41) var nvf_terrain_texture: texture_2d<f32>;
@group(0) @binding(42) var nvf_sun_visibility_sampler: sampler;
// The light volume's near and far front sets (R16Float arrays: the optical depth toward the
// light), sampled with binding 42 inside their windows only (the sampler's repeat never reached);
// stand-ins, never read, while mask bit 4 is clear or a level is absent.
@if(NVF_CLOUD_VOLUME) @group(0) @binding(45) var nvf_light_near: texture_2d_array<f32>;
@if(NVF_CLOUD_VOLUME) @group(0) @binding(46) var nvf_light_far: texture_2d_array<f32>;

// The debug view asked for (NVF_PLAY_DEBUG_SUN_VISIBILITY), 0 when off.
fn nvf_sun_debug_mode() -> u32 {
    return nvf_sun.mode.y;
}

// K from the weather's layers: the share of the sun's light the clouds let through to a
// receiver at P, the light coming from L (unit, towards the light). The ray from P along L
// meets the cloud plane at height cloud.x; the transmittance there, weakened by the strength,
// is K. A receiver at or above the plane, or a light too low (its elevation's sine at most
// cloud.z), has no cloud shadow; between cloud.z and cloud.w the shadow fades in, so the hit
// point does not run off to infinity at the horizon [engineering choice].
@if(NVF_CLOUD_VISIBILITY)
fn nvf_cloud_weather(P: vec3<f32>, L: vec3<f32>) -> f32 {
    let height = nvf_sun.cloud.x;
    let up = L.y;
    if up <= nvf_sun.cloud.z || P.y >= height {
        return 1.0;
    }
    let hit = P.xz + L.xz * ((height - P.y) / up);
    let uv = hit * nvf_sun.cloud_uv.xy + nvf_sun.cloud_uv.zw;
    let transmittance = textureSampleLevel(nvf_cloud_texture, nvf_sun_visibility_sampler, uv, 0.0).r;
    let k = saturate(1.0 - nvf_sun.cloud.y * (1.0 - transmittance));
    return mix(1.0, k, smoothstep(nvf_sun.cloud.z, nvf_sun.cloud.w, up));
}

// K (#3032 stage 2i M2): with mask bit 4 clear, the weather's layers exactly as before. With it,
// from the volumetric clouds' light volume where it has data: the transmittance exp(-depth) of
// the optical depth toward the light at P (light_volume_reader.wesl: a receiver under the slab
// floor is projected along L to it, one above it read at its own height), weakened by the same
// strength and faded in over the same low-sun band as the weather's; where the reader covers P
// only in part (a window's guard band, a level fading in) mixed over the weather's K by the
// covered share, and where it has no data the weather's K (with mask bit 8: no shadow instead).
@if(NVF_CLOUD_VISIBILITY && NVF_CLOUD_VOLUME)
fn nvf_cloud_transmittance(P: vec3<f32>, L: vec3<f32>) -> f32 {
    if (nvf_sun.mode.x & 4u) == 0u {
        return nvf_cloud_weather(P, L);
    }
    let only = (nvf_sun.mode.x & 8u) != 0u;
    let up = L.y;
    let r = lvr_optical_depth(nvf_light_near, nvf_light_far, nvf_sun_visibility_sampler, nvf_sun.light_volume, P, L);
    // The debug views (NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE=share|depth): the covered share, the
    // depth over 8, in place of K.
    if (nvf_sun.mode.x & 16u) != 0u {
        return r.y;
    }
    if (nvf_sun.mode.x & 32u) != 0u {
        return select(0.0, saturate(r.x / 8.0), r.y > 0.0);
    }
    if up <= nvf_sun.volume.y {
        return 1.0;
    }
    var other = 1.0;
    if r.y < 1.0 && !only {
        other = nvf_cloud_weather(P, L);
    }
    if r.y <= 0.0 {
        return other;
    }
    let k = saturate(1.0 - nvf_sun.volume.x * (1.0 - exp(-r.x)));
    let faded = mix(1.0, k, smoothstep(nvf_sun.volume.y, nvf_sun.volume.z, up));
    return mix(other, faded, r.y);
}

// K without bindings 45-46 (the weather-only layout): the weather's layers (the CPU never sets
// mask bit 4 then).
@if(NVF_CLOUD_VISIBILITY && !NVF_CLOUD_VOLUME)
fn nvf_cloud_transmittance(P: vec3<f32>, L: vec3<f32>) -> f32 {
    return nvf_cloud_weather(P, L);
}

@if(!NVF_CLOUD_VISIBILITY)
fn nvf_cloud_transmittance(P: vec3<f32>, L: vec3<f32>) -> f32 {
    return 1.0;
}

// Terrain receiver constants (sun_visibility.rs mirrors each; docs/design/terrain-shadows.md
// "T2: the receivers") [engineering choices]:
// the penumbra's height per unit of caster distance, about the sun's apparent diameter
// (0.53 degrees, 0.0093 rad) rounded;
const NVF_TERRAIN_PENUMBRA: f32 = 0.01;
// the least penumbra height, in texels of the map (a quarter texel, 64 units at 256);
const NVF_TERRAIN_SOFT_MIN: f32 = 0.25;
// the height S falls over this many texels along the light, added to the penumbra so the
// texel grid does not show as stair steps;
const NVF_TERRAIN_SOFT_TEXELS: f32 = 0.5;
// the most tan(elevation) that term takes (at a high sun the band would otherwise grow
// past a texel's fall);
const NVF_TERRAIN_TAN_MAX: f32 = 1.0;
// the receiver's bias: this many times the ground's relief around it (B), plus a constant
// (world units);
const NVF_TERRAIN_BIAS_RELIEF: f32 = 2.0;
const NVF_TERRAIN_BIAS_MIN: f32 = 16.0;
// T fades to 1 over this many texels at the map's edge.
const NVF_TERRAIN_EDGE_TEXELS: f32 = 4.0;

// How lit a receiver at height y (bias included) is against one texel `s` of the map (R the
// shadow height, G the caster's distance), with `extra` added to the penumbra: lit at and
// above S, in full shadow a penumbra's height under it, smooth between. Where nothing casts
// S is -1e30 and the result is 1 (the penumbra is never 0, so no division by zero).
fn nvf_terrain_lit(s: vec4<f32>, y: f32, extra: f32, texel: f32) -> f32 {
    let band = max(s.g * NVF_TERRAIN_PENUMBRA, NVF_TERRAIN_SOFT_MIN * texel) + extra;
    let t = saturate((y - s.r) / band + 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// T: the terrain's visibility of the sun at P, the light from L (unit, towards the light).
// The map is read at the four texels around P (textureLoad, indices clamped to the map:
// the shared sampler repeats and the format is unfilterable) and the four comparisons are
// filtered bilinearly, never S itself (S jumps by hundreds of units at a shadow's end). The
// receiver is lifted by a bias of NVF_TERRAIN_BIAS_RELIEF times the ground's relief there
// (B, bilinear) plus NVF_TERRAIN_BIAS_MIN, against the map's max-per-texel heights on
// slopes; a heuristic, not an error bound (it can lift a receiver on terraced ground out of
// a real shadow, and B is 0 for relief inside a texel between equal maxima:
// terrain_shadows.wesl `relief`). Outside the map T = 1, and it fades to 1 over the last NVF_TERRAIN_EDGE_TEXELS
// texels; invalid texels hold S = -1e30 and shade nothing. With no map published (mask
// bit 2 clear: switched on, nothing yet) T = 1 before any read.
@if(NVF_TERRAIN_VISIBILITY)
fn nvf_terrain_visibility(P: vec3<f32>, L: vec3<f32>) -> f32 {
    if (nvf_sun.mode.x & 2u) == 0u {
        return 1.0;
    }
    let texel = nvf_sun.terrain.z;
    let side = nvf_sun.terrain.w;
    // Texel-space position: texel (i, j) spans [i, i + 1] x [j, j + 1].
    let x = (P.xz * nvf_sun.terrain_uv.xy + nvf_sun.terrain_uv.zw) * side;
    let inside = min(min(x.x, side - x.x), min(x.y, side - x.y));
    let e = saturate(inside / NVF_TERRAIN_EDGE_TEXELS);
    let edge = e * e * (3.0 - 2.0 * e);
    if edge <= 0.0 {
        return 1.0;
    }
    // The four texel centres around x, their indices held inside the map.
    let c = x - vec2<f32>(0.5, 0.5);
    let base = floor(c);
    let f = c - base;
    let last = vec2<i32>(i32(side) - 1, i32(side) - 1);
    let i0 = clamp(vec2<i32>(base), vec2<i32>(0, 0), last);
    let i1 = clamp(vec2<i32>(base) + vec2<i32>(1, 1), vec2<i32>(0, 0), last);
    let t00 = textureLoad(nvf_terrain_texture, i0, 0);
    let t10 = textureLoad(nvf_terrain_texture, vec2<i32>(i1.x, i0.y), 0);
    let t01 = textureLoad(nvf_terrain_texture, vec2<i32>(i0.x, i1.y), 0);
    let t11 = textureLoad(nvf_terrain_texture, i1, 0);
    let w00 = (1.0 - f.x) * (1.0 - f.y);
    let w10 = f.x * (1.0 - f.y);
    let w01 = (1.0 - f.x) * f.y;
    let w11 = f.x * f.y;
    let relief = w00 * t00.b + w10 * t10.b + w01 * t01.b + w11 * t11.b;
    let y = P.y + NVF_TERRAIN_BIAS_MIN + NVF_TERRAIN_BIAS_RELIEF * relief;
    let tan_e = clamp(L.y / max(length(L.xz), 1.0e-4), 0.0, NVF_TERRAIN_TAN_MAX);
    let extra = NVF_TERRAIN_SOFT_TEXELS * texel * tan_e;
    let lit = w00 * nvf_terrain_lit(t00, y, extra, texel)
        + w10 * nvf_terrain_lit(t10, y, extra, texel)
        + w01 * nvf_terrain_lit(t01, y, extra, texel)
        + w11 * nvf_terrain_lit(t11, y, extra, texel);
    return mix(1.0, lit, edge);
}

@if(!NVF_TERRAIN_VISIBILITY)
fn nvf_terrain_visibility(P: vec3<f32>, L: vec3<f32>) -> f32 {
    return 1.0;
}

// Vsolid from the cascade shadow C and the terrain visibility T: min(C, T), the darker of
// the two [design: the maintainer's decision on #2920, 2026-11-18]. Where both see the same
// ridge the darker one counts once, never C * T (which would square one filtered edge).
// Beyond the last cascade Bevy's C is 1, so T alone; object shadows end where the cascades
// end, as with the row off (no fade-out). Without terrain, or switched on with no map
// published (mask bit 2 clear), C exactly.
@if(NVF_TERRAIN_VISIBILITY)
fn nvf_solid_visibility(cascade: f32, terrain: f32) -> f32 {
    if (nvf_sun.mode.x & 2u) == 0u {
        return cascade;
    }
    return min(cascade, terrain);
}

@if(!NVF_TERRAIN_VISIBILITY)
fn nvf_solid_visibility(cascade: f32, terrain: f32) -> f32 {
    return cascade;
}

// The parts of the sun's visibility at P (light from L, cascade shadow C; view_z, the view
// depth, is not read since the combine takes no distance, and stays for the callers' one
// signature): x K, y T, z Vsolid, w V = K * Vsolid.
fn nvf_sun_visibility_parts(P: vec3<f32>, L: vec3<f32>, cascade_shadow: f32, view_z: f32) -> vec4<f32> {
    let k = nvf_cloud_transmittance(P, L);
    let t = nvf_terrain_visibility(P, L);
    let solid = nvf_solid_visibility(cascade_shadow, t);
    return vec4<f32>(k, t, solid, k * solid);
}

// V, the factor that takes the place of the sun's cascade shadow in its direct light.
fn nvf_sun_visibility(P: vec3<f32>, L: vec3<f32>, cascade_shadow: f32, view_z: f32) -> f32 {
    return nvf_sun_visibility_parts(P, L, cascade_shadow, view_z).w;
}

// The value the debug view shows from the parts: K, T or V.
fn nvf_sun_debug_value(parts: vec4<f32>) -> f32 {
    switch nvf_sun.mode.y {
        case 1u: {
            return parts.x;
        }
        case 2u: {
            return parts.y;
        }
        default: {
            return parts.w;
        }
    }
}
