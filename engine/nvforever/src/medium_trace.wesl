// The volumetric medium's trace pass (medium.rs; #3032, stage 1). Two entries:
// `medium_trace_refined` (the reference mode) casts one ray through the centre of every texel of the
// far buffer; `medium_trace_tiles_refined` (the default) one ray per 2x2 tile of the half-size buffer
// (the tile side, 1 or 2, is `tiles.z`; the buffer's size the CPU's choice), through the corner
// the cast pattern picks this frame, into the trace target of one texel per tile,
// which medium_resolve.wesl rebuilds the buffer from. Both cast with the view's unjittered
// projection after the depth prepass and before the main pass, through the cloud source's march
// (volumetric_clouds_march.wesl `vc_trace_refined`). It writes the gathered light and transmittance
// (target 0, Rgba16Float: rgb the premultiplied colour, a the transmittance) and the
// transmittance-weighted distance (target 1, R32Float; -1 when the ray met nothing). Sky only
// in this stage: the ray is not cut by the scene's depth.
// An engineering choice for the enhanced look, not the original game's.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import nvforever::volumetric_clouds_march::{CloudVolume, MediumRay, MediumSample, vc_trace_refined, vc_view_sample, vc_view_code, vc_dither};

// medium.rs MediumTraceUniform: the same fields in the same order.
struct MediumTrace {
    // The camera's rotation times the inverse of its unjittered projection: clip space to a
    // direction in world axes from the camera (no translation, so no precision is lost far
    // from the world's origin).
    world_from_clip: mat4x4<f32>,
    // xyz: the camera's position (Bevy world), w: the cast ray's spread (medium.rs `ray_spread`:
    // the width per unit of distance one cast ray answers for, the buffer's texel times the tile
    // side), handed to the source in `MediumRay`.
    origin: vec4<f32>,
    // x, y: the buffer's size in texels; z: the frame number modulo 64 (the dither's turn);
    // w: the far end of the medium's stretch of each ray (world units).
    grid: vec4<f32>,
    // The default mode's cast pattern: x the phase; y the start offsets (0 turning, 1 fixed,
    // 2 blue, 3 centre: NVF_PLAY_DEBUG_MEDIUM_OFFSETS); z the tile side (1, 2 or 3:
    // NVF_PLAY_DEBUG_MEDIUM_TILE); w is the water plane's height above a mirror eye, zero for the main view.
    tiles: vec4<f32>,
}

@group(0) @binding(0) var<uniform> medium: MediumTrace;
@group(0) @binding(1) var<uniform> volume: CloudVolume;
// The cloud source's generated noise (#3032 stage 2b, cloud_noise.rs): the base scalar, a
// repeating linear sampler, the erosion bands; read with explicit levels.
@group(0) @binding(2) var noise_base: texture_3d<f32>;
@group(0) @binding(3) var noise_sampler: sampler;
@group(0) @binding(4) var noise_erosion: texture_3d<f32>;

struct TraceOutput {
    @location(0) light: vec4<f32>,
    @location(1) hit_distance: f32,
}

// A transmittance from this up stores as one in a half float (1 - 2^-12 rounds to 1).
const F16_ONE_FROM: f32 = 0.99975586;

// A ray's outputs, an empty one canonical where it is written: a transmittance that will store
// as one is written as (0, 0, 0, 1) with the empty distance, so no light stays in a texel that
// hides nothing and bilinear filtering cannot carry it to a neighbour.
fn medium_output(s: MediumSample) -> TraceOutput {
    var out: TraceOutput;
    if s.transmittance >= F16_ONE_FROM {
        out.light = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        out.hit_distance = -1.0;
    } else {
        out.light = vec4<f32>(s.colour, s.transmittance);
        out.hit_distance = s.hit_distance;
    }
    return out;
}

// The unit view ray through the buffer's pixel position `frag` (texel centres at .5).
fn medium_ray_dir(frag: vec2<f32>) -> vec3<f32> {
    let uv = frag / max(medium.grid.xy, vec2<f32>(1.0));
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let p = medium.world_from_clip * vec4<f32>(ndc, 1.0, 1.0);
    return normalize(p.xyz / p.w);
}

// The corner (0..side-1 in x and y) of tile `tile` of a buffer of `size` texels cast at `phase`,
// tiles of `side` x `side` (1, 2 or 3) (medium.rs `corner`; medium_resolve.wesl has the same text):
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

// The march's start offset for a ray of the default mode, 0..1: the R2 low-discrepancy
// sequence over the texel (Roberts, "The Unreasonable Effectiveness of Quasirandom
// Sequences", 2018: the plastic number's additive recurrence), turned by the golden ratio at
// each of the texel's casts in a full tile (every 9 phases at side 3, 4 at side 2, 1 at side 1;
// a clipped edge tile can cast the same texel more than once in that sweep),
// so the history averages different offsets.
// Debug aid NVF_PLAY_DEBUG_MEDIUM_OFFSETS (medium.rs `ray_offset`, the same arithmetic): `fixed`
// keeps the R2 offset of each texel for every cast (no temporal grain, a fixed spatial one);
// `blue` takes interleaved gradient noise of the texel (Jimenez, "Next Generation Post
// Processing in Call of Duty: Advanced Warfare", SIGGRAPH 2014: neighbours' offsets well
// spread) turned by the golden ratio at each cast; `centre` starts every ray at 0.5, the same
// for every texel and cast (#3032 B1b: no offset pattern at all, so what is left of a pattern
// is not the offsets').
fn medium_offset(texel: vec2<u32>, phase: u32, mode: u32, cycle: u32) -> f32 {
    if mode == 3u {
        return 0.5;
    }
    let p = vec2<f32>(texel);
    let turn = 0.6180339887 * f32((phase / max(cycle, 1u)) % 64u);
    if mode == 2u {
        return fract(fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715)))) + turn);
    }
    let spatial = fract(0.5 + p.x * 0.7548776662 + p.y * 0.5698402910);
    if mode == 1u {
        return spatial;
    }
    return fract(spatial + turn);
}

@fragment
fn medium_trace_refined(in: FullscreenVertexOutput) -> TraceOutput {
    let dir = medium_ray_dir(in.position.xy);
    var start = 0.0;
    if medium.tiles.w > 0.0 {
        // Only the reflected line of sight from the water surface onward is visible.
        if dir.y <= 0.0 {
            return medium_output(MediumSample(vec3<f32>(0.0), 1.0, -1.0));
        }
        start = medium.tiles.w / dir.y;
    }
    let ray = MediumRay(medium.origin.xyz, dir, start, medium.grid.w, medium.origin.w);
    var s: MediumSample;
    let view = vc_view_code(volume);
    if view == 1u || view == 2u || view == 4u {
        s = vc_view_sample(ray, view, noise_base, noise_sampler, noise_erosion, volume);
    } else {
        // The first prototype's per-pixel start offset, from the same pixel position: at the
        // render size the buffer's texel is the main pass's pixel.
        let dither = vc_dither(in.position.xy, u32(medium.grid.z), volume.flags.y > 0.5);
        s = vc_trace_refined(ray, dither, noise_base, noise_sampler, noise_erosion, volume);
    }
    return medium_output(s);
}

@fragment
fn medium_trace_tiles_refined(in: FullscreenVertexOutput) -> TraceOutput {
    let tile = vec2<u32>(in.position.xy);
    let size = vec2<u32>(max(medium.grid.xy, vec2<f32>(1.0)));
    let phase = u32(medium.tiles.x);
    let side = clamp(u32(medium.tiles.z), 1u, 3u);
    let texel = side * tile + medium_corner(tile, size, phase, side);
    let ray = MediumRay(medium.origin.xyz, medium_ray_dir(vec2<f32>(texel) + 0.5), 0.0, medium.grid.w, medium.origin.w);
    var s: MediumSample;
    let view = vc_view_code(volume);
    if view == 1u || view == 2u || view == 4u {
        s = vc_view_sample(ray, view, noise_base, noise_sampler, noise_erosion, volume);
    } else {
        s = vc_trace_refined(ray, medium_offset(texel, phase, u32(medium.tiles.y), side * side), noise_base, noise_sampler, noise_erosion, volume);
    }
    return medium_output(s);
}
