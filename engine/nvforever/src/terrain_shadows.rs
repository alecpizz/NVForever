//! Terrain shadow heights (#2671, stage T1 of `terrain-shadows`): from the sun heightfield
//! (stage T0, `sun_heightfield.rs`) a GPU pass computes the shadow-height map S, the height
//! under which a receiver over a texel lies in the landscape's own shadow:
//!
//! `S(x, z) = max over t > 0 of [H((x, z) + t·L) − t·tan(elevation)]`, L the horizontal unit
//! direction towards the light. A receiver at height y is in terrain shadow when y < S
//! ([`crate::sun_heightfield::shadowed`]). Stage T2 samples S on receivers through the sun
//! visibility's terrain slot ([`feed_sun_visibility`], sun_visibility.wesl
//! `nvf_terrain_visibility`, docs/design/terrain-shadows.md "T2: the receivers").
//!
//! **Notes for T2: sampling S on a receiver** (plain words; the map's facts below):
//! - The map: [`TerrainShadows::image`], `Rgba32Float`, R = S in world units (Bevy y), G = the
//!   horizontal distance in world units from the receiver texel's centre to the texel that
//!   gave S (0 when nothing casts), B = the ground's relief around the texel ([`texel_relief`],
//!   world units, 0 for an invalid texel; T2's bias), A unused. Where nothing casts, R is
//!   [`NO_SHADOW`] (−1e30, the
//!   heightfield's own `INVALID_HEIGHT`), so `y < S` is simply false: no separate mask.
//!   Rgba32Float is not filterable without an optional feature: read it with `textureLoad`
//!   (or a non-filtering sampler) and, for a soft edge, filter the *comparisons* of a few
//!   texels by hand, never S itself (S jumps by hundreds of units at a ridge's shadow end).
//! - Where: `uv = bevy.xz · uv_scale + uv_offset` ([`TerrainShadows::world_to_uv`]), uv in
//!   0..1 with v = 0 the southernmost row; texel = floor(uv · side). Outside 0..1 there is no
//!   terrain data: no terrain shadow. Fade the factor to 1 over the last few texels of the
//!   map (an edge fade), as the map's edge is where marches end early and nothing is known
//!   beyond it.
//! - Bias: the heightfield keeps each texel's *highest* point (`sun_heightfield.rs`,
//!   `downsample_max`), so on a slope facing the light the next texel sunward stands up to
//!   the relief inside one texel above the receiver's own ground, and S over a sloped texel
//!   can exceed the ground there by up to that relief (test
//!   `slope_overshoot_is_within_a_texel_of_relief`). Compare `y + bias < S` with a bias that
//!   grows with the receiver's slope, about `texel · |slope|` (256 units of texel times the
//!   ground's rise per unit), plus a small constant; flat ground needs none. Too large a bias
//!   detaches shadows from the ridge foot: keep it to that slope term. The receiver's own
//!   texel is already left out of the march (as in the CPU reference). The slope term is the
//!   map's B channel ([`texel_relief`]): per axis the smaller of the two neighbours' height
//!   differences, so a uniform slope gives its rise per texel but the flat foot (or top) of
//!   a cliff gives 0. As built (sun_visibility.wesl) the bias is `2 · B + 16`, a heuristic
//!   against speckle on planes, not an error bound: on terraced ground it can lift a
//!   receiver out of a real shadow, and relief inside a texel between equal neighbouring
//!   maxima gives B = 0 (`terrain_shadows.wesl` `relief` has the review's worked example).
//! - Penumbra: G times the light's angular size (radians) is a penumbra width in world
//!   units at the receiver, if a later stage softens the edge by distance.
//! - Freshness: [`TerrainShadows::version`] changes with every publication and the image
//!   alternates between two textures; bind by handle each frame (or on version change).
//!   `image == None` means "no terrain shadow anywhere" (off, indoors, no heightfield yet,
//!   the light at the horizon): use a factor of 1 then.
//!
//! **The pass** [engineering choice]: a compute pass, `terrain_shadows.wesl`, one invocation
//! per texel in 8 × 8 workgroups, reading T0's `Rg32Float` image with `textureLoad` (R
//! height, G validity) and writing a `Rgba32Float` storage texture (`write` access).
//! `Rgba32Float` and `Rg32Float` write-only storage and `R32Float` read-write storage are guaranteed formats
//! in wgpu 30 without optional features (`wgpu-types` `TextureFormat::guaranteed_format_features`:
//! Rg32Float `s_ro_wo`, storage usages) and in WebGPU core, and compute shaders are part of
//! every backend the game runs on (Vulkan by default, DX12 by `WGPU_BACKEND`, docs/dlss.md)
//! [documented: wgpu-types 30.0.2 texture/format.rs]. A fragment pass would need the same
//! format as a render target, which is also guaranteed; compute needs no vertex stage or
//! render pass for a full-map write. It runs in the render graph's `Begin` set (before any
//! view draws), only on a frame with a new job.
//!
//! **The march** ("The march" in docs/design/terrain-shadows.md): from the receiver texel's
//! centre along L, at most [`STEPS`] samples at distances `t_i = STEP_MIN·i + b·i²` texels,
//! b chosen so the last sample lands on the ray's reach: the nearer of the map's edge and the
//! distance `(max − min) / tan(elevation)` past which nothing can rise above the map's lowest
//! valid height. Every sample is held [`EDGE_INSET`] (1e-3 texel) short of the reach, so the
//! last one lies in the last texel the ray crosses before the edge, never on the edge (which
//! would read outside the map and end the march). So at high sun the reach is a few texels
//! and the march takes few samples (cheap, [hypothesis]: the pass's time is not measured),
//! and at low sun the steps
//! grow with distance (as the square root of it) to cover the whole map. Each sampled texel
//! is scored exactly as the CPU reference scores it, `H − entry·tan(e)` with the ray's exact
//! entry distance into that texel, and the march stops as soon as the ray, falling from the
//! map's highest height, cannot beat the best so far. The receiver's own texel is left out.
//! **Miss bound**: the march only visits texels the reference visits and scores them alike,
//! so its raw S is never above the reference's, for S at or above the map's lowest valid
//! height (the reach leaves out what falls below it: a constant-height map gives
//! [`NO_SHADOW`] where the reference gives a finite S under that height; the tests compare
//! above ground level). That ordering is of raw S only, not of the receivers' T: the soft
//! band grows with the winning caster's distance G, so a march that misses a narrow, farther
//! caster can keep a nearer one with a narrower band and come out darker after the filter
//! (the review's example: (S, G) = (90, 1,000) against the reference's (100, 20,000), a
//! receiver at 16 under tan(e) = 0.2 gives about 0.08 against 0.69), and the bilinear filter
//! has no pointwise reference at all. It is lighter in raw S only where it steps over a
//! texel. A caster at least as long along the ray as the local step (at most [`max_step`]:
//! 0.5 texel until the reach exceeds `STEPS·STEP_MIN` = 48 texels, 14.5 texels = 3,700
//! units at the full 724-texel diagonal) is always sampled, and for a flat-topped caster
//! the result is within `tan(e)·step` of the reference; a narrower one can be missed. A
//! caster the map's edge cuts shorter than the step is sampled too (by the last sample,
//! within [`EDGE_INSET`] of the edge; only a sliver under that is missed): tests
//! `march_reaches_casters_near_the_map_edge`, `march_matches_reference_at_low_sun_from_every_side`.
//! A receiver on a caster can see the caster's rest as narrower than the step (its ray
//! leaving it sideways right after its own texel). A
//! max-height pyramid (one level per step size, a sample reads the level whose texel
//! covers its step) would remove that miss: [`march_texel`] and the shader's `march` are
//! one function each with the texel read in one place, so a pyramid read replaces it there.
//!
//! **Cadence** ([`update_reason`]): a new map only when (a) T0's `version` changed, or
//! (b) the light moved enough that the predicted shadow-edge displacement exceeds half a
//! texel (128 units) since the light the published map was computed for, and never sooner
//! than [`MIN_INTERVAL_SECS`] after the last job was asked for (from the request, not the
//! publication: [`update_reason`] says why). The displacement ([`edge_displacement`]) for a
//! blocker of height h (the map's `max − min`, so a bound for every blocker on it): the
//! shadow's length `h·cot(e)` changes by `h·|Δcot(e)|`, to first order `h·csc²(e)·Δe`, not
//! capped (a shorter blocker's shadow can lie inside the map while the tallest one's runs off
//! it), and its tip swings by `L·Δazimuth`, L the length capped at the map's diagonal. This
//! is a prediction for the light's change between two maps, not a bound on what a receiver
//! sees: a map's age is not bounded (a job waits for its pipeline and for T0's upload, and
//! its light is the one asked for), and a faster game clock moves the sun further within the
//! 0.1 s floor, so a new map can move an edge by more than 128 units. The light is the `sky::Sun`
//! entity's own transform (it turns every frame), not the sky's quantized hour; at night
//! it is the moon, handled alike. While the light is within the horizon fade that stops the
//! sun's own shadows (`sky.rs` `HORIZON_FADE` 0.1 rad, `graphics.rs` `SHADOW_FADED` 0.1:
//! the light's sine under 0.1·sin 0.1, [`MIN_LIGHT_SIN`]) nothing is computed and
//! "no terrain shadow" is published.
//!
//! **Double buffering**: two target images, made on the first job; a job writes the one not
//! published, and the map is published only when the render world has recorded the whole
//! dispatch ([`JobsDone`], read by the main world a frame or two later), so a receiver never
//! sees a half-written map. Each job binds T0's image by handle at dispatch, in a bind group
//! made for that dispatch, so it reads whatever texture T0's image has then. A new T0
//! version is usually *not* a new texture: Bevy's `GpuImage` preparation (engine/vendor
//! bevy_render `texture/gpu_image.rs`) writes a version of the same size and usage into the
//! existing texture and keeps its view, so nothing here relies on a new texture or view per
//! version, and nothing needs rebuilding when the view is kept and its contents change. A
//! job's parameters and the heights it reads are extracted in the same frame (a pending job
//! is asked again with the new heights in the update T0 publishes them), so the in-place
//! write does not mix two versions in one dispatch.
//!
//! **The published resource**: [`TerrainShadows`], a main-world resource extracted to the
//! render world whenever it changes (T2 reads it there for its bind group).
//!
//! **Another worldspace** (review of f8d532cca, finding 2): T0 bumps its `generation` on
//! every emptying, a worldspace change always. A job carries the generation of its heights
//! and the publication keeps it ([`TerrainShadows::heightfield_generation`]); when T0's
//! generation differs, the published map and the job in flight are dropped ("heightfield
//! started over": no terrain shadow, the neutral input) and a completed job is accepted
//! only if it is of T0's current generation. So a map of the earlier world is never kept or
//! published, even when T0 empties and publishes the new world's heights within one update
//! (tests `world_change_drops_the_published_map`, `stale_world_job_is_not_published`).
//!
//! **Off path**: everything behind [`TerrainShadowsWanted`] (false by default), which also
//! sets T0's `SunHeightfieldWanted`. While false: no job, no target, no pipeline compiled,
//! the render system's run condition fails. The settings row Terrain shadows
//! (`Graphics::terrain_shadows`, [`follow_graphics`]) sets it; `NVF_PLAY_DEBUG_TERRAIN_SHADOWS=1`
//! turns it on whatever the row says ([`TerrainShadowsForced`]).
//!
//! **Producer and surfaces** (#3032 stage 3) [design]: the producer (T0's
//! heightfield, the jobs, the publication, the dump) follows
//! [`TerrainShadowsProducerWanted`] = the switch above or the light in the air
//! (`medium_air_demand::AirWanted`, which reads the publication in the render world); the
//! surfaces ([`feed_sun_visibility`]) follow [`TerrainShadowsWanted`] alone, so the air never
//! shades surfaces by the map nor changes their pipelines. Both are written by
//! [`follow_graphics`] only; with the air off the two are equal.
//!
//! **Into the lighting** ([`feed_sun_visibility`]): the switch decides
//! `SunVisibility::terrain_enabled` (the views' terrain variant and their pipelines), the
//! publication only fills `SunVisibility::terrain` (the uniform and the texture binding):
//! switched on with nothing published, the receivers get a neutral T = 1 and the pipelines
//! stay as they are (sun_visibility.rs module notes, the cloud side's rule).
//!
//! **Debug aids** (`NVF_PLAY_DEBUG_*`): `NVF_PLAY_DEBUG_TERRAIN_SHADOW_DUMP=<path.png>`
//! reads back the first map published at least [`DUMP_AFTER_SECS`] into the run and writes
//! it with the heightfield it was made from as greyscale PNGs, north up: `<path>.png` (S),
//! `<stem>-height.png` (H) and `<stem>-mask.png` (valid ground lit 255, in shadow 80,
//! invalid 0), with a log line giving the range mapped to 0..255. Each publication logs
//! `terrain shadows: published v<n> for sun elevation <deg> azimuth <deg>, heightfield v<m>,
//! <steps> steps, <reason>`, at most one a second; azimuth in degrees clockwise from north.

use crate::sun_heightfield::{SunHeightfield, SunHeightfieldWanted, INVALID_HEIGHT};
use crate::world::bevy_to_game;
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::gpu_readback::{ReadbackComplete, ReadbackOnce};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{texture_2d, texture_storage_2d, uniform_buffer_sized};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor, BufferUsages, CachedComputePipelineId, ComputePassDescriptor,
    ComputePipelineDescriptor, Extent3d, PipelineCache, ShaderStages, StorageTextureAccess, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems};
use bevy::render::texture::GpuImage;
use bevy::render::RenderApp;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// R where nothing casts: the heightfield's invalid height, so `y < S` is false.
pub const NO_SHADOW: f32 = INVALID_HEIGHT;
/// The most samples per texel [engineering choice, #2671 plan: a fixed-step first version].
pub const STEPS: u32 = 96;
/// Length of the first steps in texels [engineering choice: half a texel, so a caster a
/// texel long is never stepped over while the reach is under 48 texels].
pub const STEP_MIN: f32 = 0.5;
/// The march's samples stop this many texels short of its reach (`terrain_shadows.wesl`
/// `EDGE_INSET`) [engineering choice: well over the f32 error of a position on a 512-texel
/// map, about 1e-4 texel, so the last sample lies in the last texel the ray crosses before
/// the map's edge, not on the edge where it would read outside the map].
pub const EDGE_INSET: f32 = 1.0e-3;
/// The shortest time between two computations (real seconds) [engineering choice, #2671
/// plan: 0.1 s].
pub const MIN_INTERVAL_SECS: f32 = 0.1;
/// A new map when the predicted shadow-edge movement exceeds this many texels
/// [engineering choice, #2671 plan: half a texel].
pub const EDGE_TOLERANCE_TEXELS: f32 = 0.5;
/// Below this sine of its elevation the light casts no terrain shadow: the fade under which
/// the sun's own shadow maps stop (`sky.rs` `HORIZON_FADE` = 0.1 rad, `graphics.rs`
/// `SHADOW_FADED` = 0.1: the light at a tenth of its strength, sine 0.1·sin 0.1)
/// [design: those files; mirrored here as they are private].
pub const MIN_LIGHT_SIN: f32 = 0.1 * 0.099_833_42;
/// Below this horizontal share the light is overhead: no direction to march in.
pub const MIN_HORIZONTAL: f32 = 1.0e-4;
/// The output image's format: R shadow height, G caster distance, B the ground's relief
/// ([`texel_relief`]), A unused.
pub const OUTPUT_FORMAT: TextureFormat = TextureFormat::Rgba32Float;
/// Bytes of one output texel (four `f32`).
pub const OUTPUT_TEXEL_BYTES: usize = 16;
/// Invocations per workgroup side (`terrain_shadows.wesl` `@workgroup_size`).
pub const WORKGROUP: u32 = 8;
/// Bytes of the shader's `Params` uniform.
pub const PARAMS_BYTES: usize = 48;
/// The dump aid waits this long into the run (real seconds).
pub const DUMP_AFTER_SECS: f32 = 3.0;
/// At most one publication log line this often (real seconds).
pub const LOG_INTERVAL_SECS: f32 = 1.0;

/// The march's inputs, in texel units; the shader's `Params`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarchParams {
    /// Unit horizontal direction towards the light: x east (+column), y north (+row).
    pub dir: Vec2,
    /// Height lost per texel of horizontal distance: tan(elevation) · texel.
    pub tan_texel: f32,
    /// World units per texel.
    pub texel: f32,
    /// Highest and lowest valid height of the map.
    pub top: f32,
    pub bottom: f32,
    pub side: u32,
    pub steps: u32,
    pub step_min: f32,
}

/// Elevation (radians above the horizon) and azimuth (radians clockwise from north) of a
/// direction towards the light in game axes (x east, y north, z up); with its unit
/// horizontal direction. `None` when it has no horizontal part.
pub fn light_angles(toward: Vec3) -> Option<(Vec2, f32, f32)> {
    let v = toward.normalize_or_zero();
    let flat = v.truncate();
    let horizontal = flat.length();
    if horizontal < MIN_HORIZONTAL {
        return None;
    }
    Some((flat / horizontal, v.z.atan2(horizontal), v.x.atan2(v.y)))
}

/// The march's inputs for a light `toward` (game axes, towards the light) over a map of
/// `side` texels of `texel` units whose valid heights span `min..max`; the reason there is
/// no terrain shadow otherwise.
pub fn march_params(toward: Vec3, texel: f32, side: u32, min: f32, max: f32) -> Result<MarchParams, &'static str> {
    let v = toward.normalize_or_zero();
    if v.z < MIN_LIGHT_SIN {
        return Err("light at or under the horizon");
    }
    let Some((dir, elevation, _)) = light_angles(v) else { return Err("light overhead") };
    Ok(MarchParams { dir, tan_texel: elevation.tan() * texel, texel, top: max, bottom: min, side, steps: STEPS, step_min: STEP_MIN })
}

/// Where the ray from `p` along `d` enters the span `[c, c + 1]` on one axis (texel units).
fn axis_entry(c: f32, p: f32, d: f32) -> f32 {
    if d.abs() < 1.0e-6 {
        return -1.0e30;
    }
    let edge = if d > 0.0 { c } else { c + 1.0 };
    (edge - p) / d
}

/// Horizontal distance (texels) from `p` along `d` to the map's edge.
fn edge_distance(p: Vec2, d: Vec2, side: f32) -> f32 {
    let mut t = 1.0e30f32;
    for (p, d) in [(p.x, d.x), (p.y, d.y)] {
        if d > 1.0e-6 {
            t = t.min((side - p) / d);
        } else if d < -1.0e-6 {
            t = t.min(-p / d);
        }
    }
    t
}

/// How far (texels) the march from texel (`col`, `row`) goes: the map's edge, or where the
/// ray falling from the highest height is below the lowest.
pub fn texel_reach(p: &MarchParams, (col, row): (usize, usize)) -> f32 {
    let pos = Vec2::new(col as f32 + 0.5, row as f32 + 0.5);
    edge_distance(pos, p.dir, p.side as f32).min((p.top - p.bottom).max(0.0) / p.tan_texel)
}

/// The steps' growth `b` for a march of `reach` texels.
fn step_growth(p: &MarchParams, reach: f32) -> f32 {
    let n = p.steps as f32;
    (reach - n * p.step_min).max(0.0) / (n * n)
}

/// The longest step (texels) of a march of `reach` texels: its last one.
#[cfg_attr(not(test), allow(dead_code))] // the miss bound the tests hold the march to
pub fn max_step(p: &MarchParams, reach: f32) -> f32 {
    p.step_min + step_growth(p, reach) * (2.0 * p.steps as f32 - 1.0)
}

/// CPU mirror of `terrain_shadows.wesl` `march`, line for line: (S, caster distance in world
/// units) for receiver texel (`col`, `row`); [`NO_SHADOW`], 0 where nothing casts.
pub fn march_texel(heights: &[f32], valid: &[u8], (col, row): (usize, usize), p: &MarchParams) -> (f32, f32) {
    let side = p.side as i64;
    let pos = Vec2::new(col as f32 + 0.5, row as f32 + 0.5);
    let d = p.dir;
    let reach = texel_reach(p, (col, row));
    let n = p.steps.min((reach.max(0.0) / p.step_min).ceil() as u32);
    let b = step_growth(p, reach);
    // The last sample lies just inside the reach, never on the map's edge itself.
    let end = (reach - EDGE_INSET).max(0.0);
    let mut best = NO_SHADOW;
    let mut best_t = 0.0;
    let mut last = (col as i64, row as i64);
    for i in 1..=n {
        let fi = i as f32;
        let t = (p.step_min * fi + b * fi * fi).min(end);
        let q = pos + t * d;
        let c = (q.x.floor() as i64, q.y.floor() as i64);
        if c.0 < 0 || c.1 < 0 || c.0 >= side || c.1 >= side {
            break;
        }
        if c == last {
            continue;
        }
        last = c;
        let entry = axis_entry(c.0 as f32, pos.x, d.x).max(axis_entry(c.1 as f32, pos.y, d.y)).max(0.0);
        let fall = entry * p.tan_texel;
        if p.top - fall <= best {
            break;
        }
        let at = (c.1 * side + c.0) as usize;
        if valid[at] != 0 {
            let s = heights[at] - fall;
            if s > best {
                best = s;
                best_t = entry;
            }
        }
    }
    if best <= NO_SHADOW {
        (NO_SHADOW, 0.0)
    } else {
        (best, best_t * p.texel)
    }
}

/// CPU mirror of the shader's entry point for one texel: an invalid receiver, or no light
/// (steps 0), gets no shadow.
pub fn produce_texel(heights: &[f32], valid: &[u8], (col, row): (usize, usize), p: &MarchParams) -> (f32, f32) {
    if valid[row * p.side as usize + col] == 0 || p.steps == 0 {
        return (NO_SHADOW, 0.0);
    }
    march_texel(heights, valid, (col, row), p)
}

/// The whole map on the CPU (tests; the GPU pass's mirror).
#[cfg_attr(not(test), allow(dead_code))]
pub fn produce_map(heights: &[f32], valid: &[u8], p: &MarchParams) -> Vec<(f32, f32)> {
    let side = p.side as usize;
    (0..side * side).map(|i| produce_texel(heights, valid, (i % side, i / side), p)).collect()
}

/// CPU mirror of `terrain_shadows.wesl` `relief`: the ground's relief around texel (`col`,
/// `row`) of a `side` × `side` map, the output's B. Per axis (east-west, north-south) the
/// smaller of the two neighbours' height differences (one side alone when the other is
/// outside the map or invalid, 0 when both are), the two axes summed; 0 for an invalid texel.
/// On a uniform slope rising `g` per unit it is `texel · (|g_x| + |g_z|)`; at the flat foot
/// or top of a cliff it is 0 (module notes, "Bias").
pub fn texel_relief(heights: &[f32], valid: &[u8], side: usize, (col, row): (usize, usize)) -> f32 {
    let own = row * side + col;
    if valid[own] == 0 {
        return 0.0;
    }
    let h0 = heights[own];
    let rise = |c: i64, r: i64| -> Option<f32> {
        if c < 0 || r < 0 || c >= side as i64 || r >= side as i64 {
            return None;
        }
        let i = r as usize * side + c as usize;
        (valid[i] != 0).then(|| (heights[i] - h0).abs())
    };
    let (c, r) = (col as i64, row as i64);
    let axis = |dc: i64, dr: i64| match (rise(c - dc, r - dr), rise(c + dc, r + dr)) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => 0.0,
    };
    axis(1, 0) + axis(0, 1)
}

/// The whole output on the CPU, as the GPU writes it: (S, G, relief) per texel (tests; the
/// receivers' mirror in sun_visibility.rs reads it).
#[cfg_attr(not(test), allow(dead_code))]
pub fn produce_output(heights: &[f32], valid: &[u8], p: &MarchParams) -> Vec<Vec4> {
    let side = p.side as usize;
    produce_map(heights, valid, p).into_iter().enumerate().map(|(i, (s, g))| Vec4::new(s, g, texel_relief(heights, valid, side, (i % side, i / side)), 0.0)).collect()
}

/// The shader's `Params` bytes (little-endian, the WGSL uniform layout: offsets 0 dir, 8
/// tan_texel, 12 texel, 16 top, 20 bottom, 24 side, 28 steps, 32 step_min, 36..48 padding).
pub fn params_bytes(p: &MarchParams) -> [u8; PARAMS_BYTES] {
    let words = [p.dir.x.to_bits(), p.dir.y.to_bits(), p.tan_texel.to_bits(), p.texel.to_bits(), p.top.to_bits(), p.bottom.to_bits(), p.side, p.steps, p.step_min.to_bits(), 0, 0, 0];
    let mut out = [0u8; PARAMS_BYTES];
    for (i, w) in words.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

/// Predicted shadow-edge displacement (world units) when the light turns from `from` to
/// `to` (game axes, towards the light), for blockers up to `relief` high on a map whose
/// shadows can be at most `longest` long (module notes, "Cadence"): the change of the
/// shadow's length, `relief · |cot(e0) − cot(e1)|`, plus its tip's swing
/// `min(relief · cot(e), longest) · Δazimuth`.
///
/// The length term is not capped: a blocker of height h ≤ `relief` moves its shadow's end by
/// `h · |Δcot|`, and a shorter blocker's shadow can lie inside the map while the tallest one's
/// runs off it, so capping both lengths at `longest` before subtracting would call a large
/// movement zero at low sun (review of f8d532cca, finding 1). The swing is capped, as a tip
/// cannot swing on a longer arm than the longest shadow the map holds.
///
/// Called only for lights above the cut-off [`MIN_LIGHT_SIN`] (the caller's `march_params`
/// succeeded for `to`, and `from` is a published basis); there cot(e) is at most about 100, so
/// the result is finite. A direction under the cut-off, without elevation or without a
/// horizontal part gives infinity (always update) rather than a cotangent near the horizon.
pub fn edge_displacement(from: Vec3, to: Vec3, relief: f32, longest: f32) -> f32 {
    let (Some((_, e0, a0)), Some((_, e1, a1))) = (light_angles(from), light_angles(to)) else { return f32::INFINITY };
    if e0.sin() < MIN_LIGHT_SIN || e1.sin() < MIN_LIGHT_SIN {
        return f32::INFINITY;
    }
    let h = relief.max(0.0);
    let cot = |e: f32| 1.0 / e.tan();
    let mut da = (a1 - a0).abs() % std::f32::consts::TAU;
    if da > std::f32::consts::PI {
        da = std::f32::consts::TAU - da;
    }
    h * (cot(e0) - cot(e1)).abs() + (h * cot(e0.min(e1))).min(longest) * da
}

/// What the published map was computed for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Basis {
    /// Towards the light, game axes.
    pub toward: Vec3,
    pub heightfield_version: u64,
    /// When its computation was asked for (real seconds).
    pub requested_at: f32,
}

/// Why a new map is computed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reason {
    /// Nothing is published yet (or the light just came back).
    First,
    /// T0 published new heights.
    Heights,
    /// The light moved: the predicted edge displacement in world units.
    SunMoved(f32),
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reason::First => write!(f, "first"),
            Reason::Heights => write!(f, "heightfield changed"),
            Reason::SunMoved(d) => write!(f, "sun moved: edge {d:.0} units"),
        }
    }
}

/// The cadence rule (module notes, "Cadence"): whether to compute a new map now, against
/// the published one's `basis`, for heightfield `version`, the light `toward` (game axes)
/// and the map's `relief` (max − min), `texel` and longest shadow `longest` (world units).
pub fn update_reason(basis: Option<&Basis>, now: f32, version: u64, toward: Vec3, relief: f32, texel: f32, longest: f32) -> Option<Reason> {
    let Some(b) = basis else { return Some(Reason::First) };
    // The floor counts from when the published map was *asked for*, not from when it was
    // published [engineering choice]. Counted so, jobs start at most every 0.1 s, and as only
    // one job is in flight at a time (none asked for while `Producer::pending`), a job slower
    // than the floor is followed by the next as soon as it is published, if one is due.
    // Counting from the publication would add each job's own latency (the frame or two to
    // publish, longer while a pipeline compiles or an upload waits) to every interval, leaving
    // the map that much further behind a moving sun. The floor caps how often jobs start; it
    // bounds neither a publication's age nor the edge movement per publication (module notes,
    // "Cadence"). First jobs (no basis) skip it, and a pending job refreshed for new heights
    // is not held to it (T0 publishes at most every `PUBLISH_INTERVAL_SECS` while loading).
    if now - b.requested_at < MIN_INTERVAL_SECS {
        return None;
    }
    if b.heightfield_version != version {
        return Some(Reason::Heights);
    }
    let d = edge_displacement(b.toward, toward, relief, longest);
    (d > EDGE_TOLERANCE_TEXELS * texel).then_some(Reason::SunMoved(d))
}

/// Whether terrain shadows are computed and drawn: the settings row
/// (`Graphics::terrain_shadows`, off by default) or [`TerrainShadowsForced`]. Turning it on
/// turns T0's heightfield on.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainShadowsWanted(pub bool);

/// Whether the map is produced (T0's heightfield on, jobs given out, the map published and
/// extracted): [`TerrainShadowsWanted`] or the light in the air
/// (`medium_air_demand::AirWanted`), which reads the map but never shades surfaces by it
/// (module notes, "Producer and surfaces"). Written by [`follow_graphics`] only.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainShadowsProducerWanted(pub bool);

/// `NVF_PLAY_DEBUG_TERRAIN_SHADOWS=1`: terrain shadows on whatever the settings row says.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainShadowsForced(pub bool);

/// The settings row (`Graphics::terrain_shadows`) or the debug override decide the switch;
/// the switch or the light in the air (absent: off) decide the producer.
pub fn follow_graphics(
    g: Res<crate::graphics::Graphics>,
    forced: Res<TerrainShadowsForced>,
    air: Option<Res<crate::medium_air_demand::AirWanted>>,
    mut wanted: ResMut<TerrainShadowsWanted>,
    mut producer: ResMut<TerrainShadowsProducerWanted>,
) {
    let want = forced.0 || g.terrain_shadows;
    if wanted.0 != want {
        wanted.0 = want;
    }
    let produce = want || air.is_some_and(|a| a.0);
    if producer.0 != produce {
        producer.0 = produce;
    }
}

/// Run condition of [`follow_graphics`]: the settings changed, or the air's demand did.
fn graphics_or_air_changed(g: Option<Res<crate::graphics::Graphics>>, air: Option<Res<crate::medium_air_demand::AirWanted>>) -> bool {
    g.is_some_and(|g| g.is_changed() || air.is_some_and(|a| a.is_changed()))
}

/// Run condition of the production: the map is wanted.
fn producing(w: Res<TerrainShadowsProducerWanted>) -> bool {
    w.0
}

/// Run condition of [`stop_terrain_shadows`]: the production was just switched off.
fn production_stopped(w: Res<TerrainShadowsProducerWanted>) -> bool {
    w.is_changed() && !w.0
}

/// What the terrain slot of the sun visibility gets for this switch, map and sun light
/// (`cascades`: the sun's cascade configuration while its shadow maps are on).
/// `terrain_enabled` follows the switch alone; `terrain` is set only while switched on with
/// a map published (module notes, "Into the lighting").
pub fn terrain_slot(wanted: bool, out: &TerrainShadows) -> (bool, Option<crate::sun_visibility::TerrainVisibility>) {
    let terrain = out.image.as_ref().filter(|_| wanted && out.side > 0 && out.texel > 0.0).map(|image| crate::sun_visibility::TerrainVisibility {
        image: image.clone(),
        uv_scale: out.uv_scale,
        uv_offset: out.uv_offset,
        texel: out.texel,
        side: out.side,
    });
    (wanted, terrain)
}

/// Copy the switch and the published map into the sun visibility's terrain slot when they
/// changed (the plumbing of sun_visibility.rs samples it on the sun's light). The map's
/// image is bound by handle, so each publication's texture (the two targets alternate) is
/// what the views read from the frame it is extracted on (sun_visibility.rs
/// `prepare_sun_visibility` follows the texture view every frame).
#[allow(clippy::type_complexity)]
pub fn feed_sun_visibility(
    wanted: Res<TerrainShadowsWanted>,
    out: Res<TerrainShadows>,
    mut sink: ResMut<crate::sun_visibility::SunVisibility>,
) {
    let (enabled, terrain) = terrain_slot(wanted.0, &out);
    if sink.terrain_enabled != enabled {
        sink.terrain_enabled = enabled;
    }
    if sink.terrain != terrain {
        sink.terrain = terrain;
    }
}

/// Run condition of [`feed_sun_visibility`]: switched on, or a slot left to empty.
fn feeding_needed(wanted: Res<TerrainShadowsWanted>, sink: Res<crate::sun_visibility::SunVisibility>) -> bool {
    wanted.0 || sink.terrain_on()
}

/// The last complete shadow-height map (module notes, "Notes for T2"). A main-world
/// resource, extracted to the render world whenever it changes.
#[derive(Resource, Clone, Debug, Default)]
#[allow(dead_code)] // the receivers read the map and its uv; origin, the light and `steps` describe it (Debug) only
pub struct TerrainShadows {
    /// The map ([`OUTPUT_FORMAT`], `side` × `side`, row 0 the southernmost); `None`: no
    /// terrain shadow anywhere (`reason` says why).
    pub image: Option<Handle<Image>>,
    /// Game x, y of texel (0, 0)'s south-west corner (Bevy x, −z), from T0.
    pub origin: Vec2,
    /// World units per texel.
    pub texel: f32,
    /// Texels per side.
    pub side: u32,
    /// `uv = bevy.xz · uv_scale + uv_offset` ([`TerrainShadows::world_to_uv`]).
    pub uv_scale: Vec2,
    pub uv_offset: Vec2,
    /// The light it was computed for, towards the light, Bevy axes.
    pub light_dir: Vec3,
    /// That light's elevation and azimuth (radians; azimuth clockwise from north).
    pub elevation: f32,
    pub azimuth: f32,
    /// When that light direction was taken (real seconds since start).
    pub computed_at: f32,
    /// T0's version it was computed from.
    pub heightfield_version: u64,
    /// T0's discontinuity generation it was computed from (`SunHeightfield::generation`):
    /// a map of another generation describes another worldspace and is never kept.
    pub heightfield_generation: u64,
    /// Bumped with every change of this resource (publication or going dark).
    pub version: u64,
    /// The heightfield's valid height range it was computed with.
    pub min: f32,
    pub max: f32,
    /// The most samples per texel.
    pub steps: u32,
    /// Why there is no map ("" while there is one).
    pub reason: &'static str,
}

impl TerrainShadows {
    /// The world-to-uv transform for a map with south-west corner `origin` (game x, y) of
    /// `side` texels of `texel` units: Bevy (x, z) is game (x, −y), v grows northwards.
    pub fn uv_transform(origin: Vec2, texel: f32, side: u32) -> (Vec2, Vec2) {
        let extent = texel * side as f32;
        (Vec2::new(1.0 / extent, -1.0 / extent), Vec2::new(-origin.x / extent, -origin.y / extent))
    }

    /// The map's uv of a Bevy world position (0..1 inside the map).
    #[cfg_attr(not(test), allow(dead_code))] // T2's receivers do this in the shader
    pub fn world_to_uv(&self, bevy: Vec3) -> Vec2 {
        Vec2::new(bevy.x, bevy.z) * self.uv_scale + self.uv_offset
    }

    pub fn is_active(&self) -> bool {
        self.image.is_some()
    }
}

/// A computation for the render world.
#[derive(Clone, Debug)]
struct Job {
    id: u64,
    heights: Handle<Image>,
    target: Handle<Image>,
    params: MarchParams,
}

#[derive(Resource, Clone, Debug, Default)]
struct ShadowJob(Option<Job>);

impl ExtractResource<RenderApp> for ShadowJob {
    type Source = ShadowJob;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

impl ExtractResource<RenderApp> for TerrainShadows {
    type Source = TerrainShadows;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

/// The last job the render world has recorded completely (shared by both worlds). Ids only
/// grow, so a job is done once this reaches its id.
#[derive(Resource, Clone, Default)]
struct JobsDone(Arc<AtomicU64>);

/// The job in flight and what it was asked for.
#[derive(Clone, Debug)]
struct Pending {
    id: u64,
    target: usize,
    toward_bevy: Vec3,
    toward_game: Vec3,
    params: MarchParams,
    heightfield_version: u64,
    /// T0's generation the heights were of: a job of another one is never published.
    generation: u64,
    origin: Vec2,
    requested_at: f32,
    reason: String,
}

/// The main world's side of the producer.
#[derive(Resource, Default)]
struct Producer {
    /// The two targets (made on the first job) and their side.
    targets: Option<[Handle<Image>; 2]>,
    target_side: u32,
    /// The published target.
    front: Option<usize>,
    pending: Option<Pending>,
    /// What the published map was computed for.
    basis: Option<Basis>,
    /// The last job id given out (never reset: [`JobsDone`] keeps the last one done).
    last_id: u64,
    last_log: Option<f32>,
}

/// `NVF_PLAY_DEBUG_TERRAIN_SHADOW_DUMP` (module notes, "Debug aids").
#[derive(Resource, Default)]
struct DumpAid {
    path: Option<PathBuf>,
    asked: bool,
}

pub struct TerrainShadowsPlugin;

impl Plugin for TerrainShadowsPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "terrain_shadows.wesl");
        let forced = nvf_play::dialogue::debug_switch(std::env::var("NVF_PLAY_DEBUG_TERRAIN_SHADOWS").ok().as_deref());
        if forced {
            info!("terrain shadows: on by NVF_PLAY_DEBUG_TERRAIN_SHADOWS, whatever the settings row says");
            app.insert_resource(TerrainShadowsWanted(true))
                .insert_resource(TerrainShadowsProducerWanted(true))
                .insert_resource(SunHeightfieldWanted(true));
        } else {
            app.init_resource::<TerrainShadowsWanted>().init_resource::<TerrainShadowsProducerWanted>();
        }
        app.insert_resource(TerrainShadowsForced(forced));
        let path = std::env::var_os("NVF_PLAY_DEBUG_TERRAIN_SHADOW_DUMP").filter(|v| !v.is_empty()).map(PathBuf::from);
        let done = JobsDone::default();
        app.insert_resource(done.clone())
            .insert_resource(DumpAid { path, asked: false })
            .init_resource::<TerrainShadows>()
            .init_resource::<ShadowJob>()
            .init_resource::<Producer>()
            .add_plugins((ExtractResourcePlugin::<ShadowJob>::default(), ExtractResourcePlugin::<TerrainShadows>::default()))
            .add_systems(
                Update,
                (
                    (follow_graphics.run_if(graphics_or_air_changed), follow_wanted)
                        .chain()
                        .before(crate::sun_heightfield::stop_heightfield)
                        .before(crate::sun_heightfield::update_heightfield),
                    (
                        stop_terrain_shadows.run_if(production_stopped),
                        update_terrain_shadows.run_if(producing),
                        dump_terrain_shadows.run_if(producing).run_if(|aid: Res<DumpAid>| aid.path.is_some() && !aid.asked),
                        feed_sun_visibility.run_if(feeding_needed),
                    )
                        .chain()
                        .after(crate::sun_heightfield::update_heightfield),
                ),
            );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.insert_resource(done).init_resource::<ProducerPipeline>().add_systems(
            RenderGraph,
            // Before every view's passes, so a map published later is complete on the GPU
            // before anything samples it.
            produce_terrain_shadows.in_set(RenderGraphSystems::Begin).run_if(|job: Option<Res<ShadowJob>>| job.is_some_and(|j| j.0.is_some())),
        );
    }
}

/// The map needs T0's heightfield: switching its production on switches it on, switching it
/// off switches it off again, but only when the production was what switched it on
/// (`turned_on`): a heightfield already on by its own demand (`NVF_PLAY_DEBUG_SUN_HEIGHTFIELD`)
/// stays on through any on/off cycle of the row or the light in the air.
fn follow_wanted(wanted: Res<TerrainShadowsProducerWanted>, mut heightfield: ResMut<SunHeightfieldWanted>, mut turned_on: Local<bool>) {
    if !wanted.is_changed() {
        return;
    }
    if wanted.0 {
        if !heightfield.0 {
            heightfield.0 = true;
            *turned_on = true;
        }
    } else if *turned_on {
        *turned_on = false;
        if heightfield.0 {
            heightfield.0 = false;
        }
    }
}

/// Switched off: nothing published, no job, the targets freed.
fn stop_terrain_shadows(mut producer: ResMut<Producer>, mut job: ResMut<ShadowJob>, mut out: ResMut<TerrainShadows>, mut images: ResMut<Assets<Image>>) {
    let had = producer.targets.is_some() || out.is_active() || job.0.is_some();
    if job.0.is_some() {
        job.0 = None;
    }
    if let Some(targets) = producer.targets.take() {
        for t in &targets {
            images.remove(t);
        }
    }
    let last_id = producer.last_id;
    *producer = Producer { last_id, ..default() };
    if had {
        let version = out.version + 1;
        *out = TerrainShadows { version, reason: "off", ..default() };
        info!("terrain shadows: off");
    }
}

/// A target: `side` × `side`, written by the pass, sampled with loads or nearest.
fn target_image(side: u32) -> Image {
    let mut image = Image::new_uninit(Extent3d { width: side, height: side, depth_or_array_layers: 1 }, TextureDimension::D2, OUTPUT_FORMAT, RenderAssetUsages::RENDER_WORLD);
    image.texture_descriptor.usage |= TextureUsages::STORAGE_BINDING;
    image.sampler = ImageSampler::nearest();
    image
}

/// No terrain shadow for `why`: drop the job in flight, publish "none" once.
fn go_dark(why: &'static str, producer: &mut Producer, job: &mut ResMut<ShadowJob>, out: &mut ResMut<TerrainShadows>) {
    producer.pending = None;
    producer.basis = None;
    if job.0.is_some() {
        job.0 = None;
    }
    if out.is_active() || out.reason != why {
        out.image = None;
        out.reason = why;
        out.version += 1;
        info!("terrain shadows: none ({why})");
    }
}

#[allow(clippy::too_many_arguments)]
fn update_terrain_shadows(
    field: Res<SunHeightfield>,
    sun: Query<&Transform, With<crate::sky::Sun>>,
    time: Res<Time<Real>>,
    done: Res<JobsDone>,
    mut producer: ResMut<Producer>,
    mut job: ResMut<ShadowJob>,
    mut out: ResMut<TerrainShadows>,
    mut images: ResMut<Assets<Image>>,
) {
    let now = time.elapsed_secs();
    let producer = &mut *producer;
    // T0 started over (another worldspace, an interior, switched off and on): a published
    // map or a job in flight of the earlier heights describes other terrain. Drop both and
    // show no terrain shadow (the neutral input) until a map of the new heights is complete;
    // T0 can empty and publish again within one update, so its emptiness alone is not seen.
    let stale_pending = producer.pending.as_ref().is_some_and(|p| p.generation != field.generation);
    let stale_map = out.is_active() && out.heightfield_generation != field.generation;
    if stale_pending || stale_map {
        go_dark("heightfield started over", producer, &mut job, &mut out);
    }
    // A job the render world has recorded becomes the publication, if it is of these heights'
    // generation (always so after the check above; checked again where it is accepted).
    if producer.pending.as_ref().is_some_and(|p| p.generation == field.generation && done.0.load(Ordering::Acquire) >= p.id) {
        if let (Some(p), Some(targets)) = (producer.pending.take(), producer.targets.as_ref()) {
            publish(&p, targets[p.target].clone(), now, producer, &mut out);
        }
        if job.0.is_some() {
            job.0 = None;
        }
    }
    let Ok(sun) = sun.single() else { return go_dark("no sun light", producer, &mut job, &mut out) };
    let toward_bevy = sun.back().as_vec3();
    let toward_game = bevy_to_game(toward_bevy);
    let Some(heights) = field.image.clone().filter(|_| !field.is_empty() && field.valid_texels > 0) else {
        return go_dark("no heightfield", producer, &mut job, &mut out);
    };
    let side = field.side as u32;
    let params = match march_params(toward_game, field.texel, side, field.min, field.max) {
        Ok(p) => p,
        Err(why) => return go_dark(why, producer, &mut job, &mut out),
    };
    // One job at a time; one asked for older heights is asked again with the new ones.
    if let Some(p) = producer.pending.as_mut() {
        if p.heightfield_version != field.version {
            producer.last_id += 1;
            p.id = producer.last_id;
            p.params = params;
            p.heightfield_version = field.version;
            p.origin = field.origin;
            p.toward_bevy = toward_bevy;
            p.toward_game = toward_game;
            p.requested_at = now;
            p.reason = "heightfield changed while computing".into();
            if let Some(targets) = producer.targets.as_ref() {
                job.0 = Some(Job { id: p.id, heights, target: targets[p.target].clone(), params });
            }
        }
        return;
    }
    let basis = if out.is_active() { producer.basis.as_ref() } else { None };
    let longest = field.side as f32 * field.texel * std::f32::consts::SQRT_2;
    let Some(reason) = update_reason(basis, now, field.version, toward_game, field.max - field.min, field.texel, longest) else { return };
    if producer.targets.is_none() || producer.target_side != side {
        producer.targets = Some([images.add(target_image(side)), images.add(target_image(side))]);
        producer.target_side = side;
        producer.front = None;
    }
    let Some(targets) = producer.targets.as_ref() else { return };
    let target = producer.front.map_or(0, |f| 1 - f);
    producer.last_id += 1;
    let id = producer.last_id;
    job.0 = Some(Job { id, heights, target: targets[target].clone(), params });
    producer.pending = Some(Pending {
        id,
        target,
        toward_bevy,
        toward_game,
        params,
        heightfield_version: field.version,
        generation: field.generation,
        origin: field.origin,
        requested_at: now,
        reason: reason.to_string(),
    });
}

/// Publish a finished job's map and log it (at most one line a second).
fn publish(p: &Pending, image: Handle<Image>, now: f32, producer: &mut Producer, out: &mut TerrainShadows) {
    let (_, elevation, azimuth) = light_angles(p.toward_game).unwrap_or_default();
    let (uv_scale, uv_offset) = TerrainShadows::uv_transform(p.origin, p.params.texel, p.params.side);
    *out = TerrainShadows {
        image: Some(image),
        origin: p.origin,
        texel: p.params.texel,
        side: p.params.side,
        uv_scale,
        uv_offset,
        light_dir: p.toward_bevy,
        elevation,
        azimuth,
        computed_at: p.requested_at,
        heightfield_version: p.heightfield_version,
        heightfield_generation: p.generation,
        version: out.version + 1,
        min: p.params.bottom,
        max: p.params.top,
        steps: p.params.steps,
        reason: "",
    };
    producer.front = Some(p.target);
    producer.basis = Some(Basis { toward: p.toward_game, heightfield_version: p.heightfield_version, requested_at: p.requested_at });
    if producer.last_log.is_none_or(|t| now - t >= LOG_INTERVAL_SECS) {
        producer.last_log = Some(now);
        info!(
            "terrain shadows: published v{} for sun elevation {:.2} azimuth {:.1}, heightfield v{}, {} steps, {}",
            out.version,
            elevation.to_degrees(),
            azimuth.to_degrees().rem_euclid(360.0),
            p.heightfield_version,
            p.params.steps,
            p.reason
        );
    }
}

/// The render world's side: the pipeline (asked for on the first job) and the last job run.
#[derive(Resource, Default)]
struct ProducerPipeline {
    layout: Option<BindGroupLayoutDescriptor>,
    id: Option<CachedComputePipelineId>,
    last_dispatched: u64,
    warned: bool,
}

/// The pass's bindings: 0 the heightfield (`texture_2d<f32>`, unfilterable), 1 the target
/// (`texture_storage_2d<rg32float, write>`), 2 the parameters (uniform, [`PARAMS_BYTES`]).
fn layout_descriptor() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "terrain_shadows_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_storage_2d(OUTPUT_FORMAT, StorageTextureAccess::WriteOnly),
                uniform_buffer_sized(false, std::num::NonZeroU64::new(PARAMS_BYTES as u64)),
            ),
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn produce_terrain_shadows(
    job: Res<ShadowJob>,
    done: Res<JobsDone>,
    mut pipe: ResMut<ProducerPipeline>,
    cache: Res<PipelineCache>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    assets: Res<AssetServer>,
    mut ctx: RenderContext,
) {
    let Some(job) = job.0.as_ref() else { return };
    if job.id <= pipe.last_dispatched {
        return;
    }
    let id = match pipe.id {
        Some(id) => id,
        None => {
            let layout = layout_descriptor();
            let id = cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some("terrain_shadows_pipeline".into()),
                layout: vec![layout.clone()],
                shader: assets.load("embedded://nvforever/terrain_shadows.wesl"),
                entry_point: Some("produce".into()),
                zero_initialize_workgroup_memory: false,
                ..default()
            });
            pipe.layout = Some(layout);
            pipe.id = Some(id);
            id
        }
    };
    let (Some(pipeline), Some(layout)) = (cache.get_compute_pipeline(id), pipe.layout.as_ref()) else { return };
    let (Some(heights), Some(target)) = (gpu_images.get(&job.heights), gpu_images.get(&job.target)) else { return };
    let side = job.params.side;
    let fits = heights.texture.width() == side
        && heights.texture.height() == side
        && heights.texture.format() == crate::sun_heightfield::IMAGE_FORMAT
        && target.texture.width() == side
        && target.texture.height() == side
        && target.texture.format() == OUTPUT_FORMAT
        && target.texture.usage().contains(TextureUsages::STORAGE_BINDING);
    if !fits {
        if !pipe.warned {
            pipe.warned = true;
            warn!(
                "terrain shadows: textures do not fit the job (side {side}; heights {}x{} {:?}, target {}x{} {:?}); not computed",
                heights.texture.width(),
                heights.texture.height(),
                heights.texture.format(),
                target.texture.width(),
                target.texture.height(),
                target.texture.format()
            );
        }
        return;
    }
    let layout = cache.get_bind_group_layout(layout);
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor { label: Some("terrain_shadows_params"), contents: &params_bytes(&job.params), usage: BufferUsages::UNIFORM });
    let group = device.create_bind_group("terrain_shadows", &layout, &BindGroupEntries::sequential((&heights.texture_view, &target.texture_view, buffer.as_entire_binding())));
    let groups = side.div_ceil(WORKGROUP);
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut pass = ctx.command_encoder().begin_compute_pass(&ComputePassDescriptor { label: Some("terrain_shadows"), timestamp_writes: None });
    let span = diagnostics.time_span(&mut pass, "terrain_shadows");
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &group, &[]);
    pass.dispatch_workgroups(groups, groups, 1);
    span.end(&mut pass);
    pipe.last_dispatched = job.id;
    // Recorded in this frame's command buffers, submitted with them: from here on anything
    // drawn later reads a complete map.
    done.0.store(job.id, Ordering::Release);
}

/// The heightfield a dumped map was made from.
struct DumpSnapshot {
    heights: Vec<f32>,
    valid: Vec<u8>,
    side: usize,
    min: f32,
    max: f32,
    version: u64,
}

/// Read back the first map published [`DUMP_AFTER_SECS`] into the run (module notes).
fn dump_terrain_shadows(mut aid: ResMut<DumpAid>, out: Res<TerrainShadows>, field: Res<SunHeightfield>, time: Res<Time<Real>>, mut commands: Commands) {
    let Some(path) = aid.path.clone() else { return };
    let Some(image) = out.image.clone() else { return };
    if time.elapsed_secs() < DUMP_AFTER_SECS || field.version != out.heightfield_version || field.side as u32 != out.side {
        return;
    }
    aid.asked = true;
    let mut snapshot = Some(DumpSnapshot { heights: field.heights.clone(), valid: field.valid.clone(), side: field.side, min: out.min, max: out.max, version: out.version });
    info!("terrain shadows: reading back v{} for {}", out.version, path.display());
    commands.spawn((Name::new("Terrain shadow dump"), ReadbackOnce::texture(image))).observe(move |ev: On<ReadbackComplete>| {
        if let Some(s) = snapshot.take() {
            write_dump(&path, &ev.data, &s);
        }
    });
}

/// `<stem>-<tag>.png` beside `path`.
fn sibling(path: &Path, tag: &str) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "terrain-shadows".into());
    path.with_file_name(format!("{stem}-{tag}.png"))
}

fn write_grey(path: &Path, pixels: &[u8], side: u32) -> Result<(), String> {
    fnv_install::guard_write(path, None).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut bytes = Vec::new();
    let mut e = png::Encoder::new(&mut bytes, side, side);
    e.set_color(png::ColorType::Grayscale);
    e.set_depth(png::BitDepth::Eight);
    let mut w = e.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(pixels).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

/// The S map, its heightfield and the shaded-ground mask as greyscale PNGs, north up.
fn write_dump(path: &Path, data: &[u8], s: &DumpSnapshot) {
    let (side, n) = (s.side, s.side * s.side);
    let stride = OUTPUT_TEXEL_BYTES;
    if data.len() < n * stride || s.heights.len() < n || s.valid.len() < n {
        warn!("terrain shadows: dump read back {} bytes for a {side}x{side} map; nothing written", data.len());
        return;
    }
    let shadow_at = |i: usize| f32::from_le_bytes([data[i * stride], data[i * stride + 1], data[i * stride + 2], data[i * stride + 3]]);
    let (lo, hi) = (s.min, s.max.max(s.min + 1.0));
    let grey = |v: f32| (((v - lo) / (hi - lo)).clamp(0.0, 1.0) * 255.0).round() as u8;
    let (mut shadow, mut height, mut mask) = (vec![0u8; n], vec![0u8; n], vec![0u8; n]);
    let (mut valid, mut shaded) = (0usize, 0usize);
    for row in 0..side {
        for col in 0..side {
            let (i, o) = (row * side + col, (side - 1 - row) * side + col);
            let sv = shadow_at(i);
            if sv > NO_SHADOW * 0.1 {
                shadow[o] = grey(sv);
            }
            if s.valid[i] != 0 {
                valid += 1;
                height[o] = grey(s.heights[i]);
                let dark = crate::sun_heightfield::shadowed(s.heights[i], sv);
                shaded += usize::from(dark);
                mask[o] = if dark { 80 } else { 255 };
            }
        }
    }
    let files = [(path.to_path_buf(), shadow), (sibling(path, "height"), height), (sibling(path, "mask"), mask)];
    for (file, pixels) in &files {
        if let Err(e) = write_grey(file, pixels, side as u32) {
            warn!("terrain shadows: cannot write {}: {e}", file.display());
            return;
        }
    }
    info!(
        "terrain shadows: dumped v{} to {} (and -height, -mask): S and heights {lo:.0}..{hi:.0} mapped to 0..255 (no shadow and invalid 0), north up; mask lit 255, shaded 80, invalid 0; {:.1}% of valid ground shaded",
        s.version,
        path.display(),
        if valid == 0 { 0.0 } else { 100.0 * shaded as f32 / valid as f32 }
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sun_heightfield::{shadow_height_reference, shadowed, HeightGrid, SIDE, TEXEL};
    use fnv_world::CELL_SIZE;

    /// Towards the light at `elevation` and `azimuth` (radians, clockwise from north), game axes.
    fn toward(elevation: f32, azimuth: f32) -> Vec3 {
        Vec3::new(azimuth.sin() * elevation.cos(), azimuth.cos() * elevation.cos(), elevation.sin())
    }

    fn range(heights: &[f32], valid: &[u8]) -> (f32, f32) {
        let mut it = heights.iter().zip(valid).filter(|(_, &v)| v != 0).map(|(&h, _)| h);
        let first = it.next().unwrap_or(0.0);
        it.fold((first, first), |(lo, hi), h| (lo.min(h), hi.max(h)))
    }

    fn params_for(heights: &[f32], valid: &[u8], side: usize, light: Vec3) -> MarchParams {
        let (lo, hi) = range(heights, valid);
        march_params(light, TEXEL, side as u32, lo, hi).unwrap()
    }

    #[test]
    fn update_error_bound() {
        let (h, longest) = (2_000.0f32, 1.0e9f32);
        for deg in [5.0f32, 15.0, 45.0] {
            let e = deg.to_radians();
            let de = 1.0e-4;
            let from = toward(e, 1.0);
            // Elevation: first order h·csc²(e)·Δe.
            let d = edge_displacement(from, toward(e + de, 1.0), h, longest);
            let first = h / e.sin().powi(2) * de;
            assert!((d - first).abs() / first < 0.01, "{deg}°: {d} against {first}");
            // Azimuth: the tip swings by h·cot(e)·Δa (linear in Δa: a larger step keeps the
            // f32 noise of the recomputed angles well under the tolerance).
            let da = 1.0e-3;
            let d = edge_displacement(from, toward(e, 1.0 + da), h, longest);
            let swing = h / e.tan() * da;
            assert!((d - swing).abs() / swing < 0.01, "{deg}°: {d} against {swing}");
            assert_eq!(edge_displacement(from, from, h, longest), 0.0);
        }
        // Across north the azimuth difference wraps.
        let d = edge_displacement(toward(0.3, 0.01), toward(0.3, std::f32::consts::TAU - 0.01), h, longest);
        assert!((d - h / 0.3f32.tan() * 0.02).abs() < 1.0, "{d}");
        // The swing's arm is capped at the longest shadow the map holds: 4,000 units of relief
        // at 0.015 rad cast 266,664 units, longer than the 185,364-unit diagonal.
        let map = SIDE as f32 * TEXEL * std::f32::consts::SQRT_2;
        let da = 1.0e-3f32;
        let d = edge_displacement(toward(0.015, 0.0), toward(0.015, da), 4_000.0, map);
        assert!((d - map * da).abs() / (map * da) < 0.01, "{d} against {}", map * da);
        // No horizontal part, or under the cut-off: always update.
        assert!(edge_displacement(Vec3::Z, toward(1.0, 0.0), h, map).is_infinite());
        assert!(edge_displacement(toward(0.005, 0.0), toward(0.3, 0.0), h, map).is_infinite());
        assert!(edge_displacement(toward(0.3, 0.0), toward(0.005, 0.0), h, map).is_infinite());
    }

    /// Review of f8d532cca, finding 1: at a grazing sun the map's tallest blocker casts past
    /// the map's diagonal at both elevations, but a shorter blocker's shadow lies inside the
    /// map and moves by tens of thousands of units. Capping both lengths before subtracting
    /// called that zero; the bound must count it.
    #[test]
    fn low_sun_movement_is_not_capped_away() {
        let map = SIDE as f32 * TEXEL * std::f32::consts::SQRT_2;
        assert!((map - 185_364.0).abs() < 10.0, "{map}");
        let relief = 4_000.0f32;
        let (e0, e1) = (0.010f32, 0.020f32);
        // Both above the cut-off, and the tallest blocker's shadow longer than the map at both.
        assert!(e0.sin() >= MIN_LIGHT_SIN && e1.sin() >= MIN_LIGHT_SIN);
        assert!(relief / e0.tan() > map && relief / e1.tan() > map);
        // A 1,000-unit blocker: about 100,000 units of shadow at 0.010, 50,000 at 0.020, both
        // inside the map.
        let shorter = |e: f32| 1_000.0 / e.tan();
        assert!(shorter(e0) < map && shorter(e1) < map);
        let moved = shorter(e0) - shorter(e1);
        assert!(moved > 49_000.0, "{moved}");
        let d = edge_displacement(toward(e0, 0.0), toward(e1, 0.0), relief, map);
        assert!(d.is_finite() && d >= moved, "predicted {d}, a 1,000-unit blocker moves {moved}");
        // The whole relief's own movement, uncapped.
        let full = relief * (1.0 / e0.tan() - 1.0 / e1.tan());
        assert!((d - full).abs() / full < 1e-3, "{d} against {full}");
        // So the cadence asks for a new map ...
        let basis = Basis { toward: toward(e0, 0.0), heightfield_version: 1, requested_at: 0.0 };
        assert!(matches!(update_reason(Some(&basis), 1.0, 1, toward(e1, 0.0), relief, TEXEL, map), Some(Reason::SunMoved(m)) if m >= moved));
        // ... and does so for small steps just above the cut-off too: 0.0001 rad at 0.0105
        // moves a 4,000-unit relief's shadow end by about 3,600 units.
        let step = edge_displacement(toward(0.0105, 0.0), toward(0.0106, 0.0), relief, map);
        assert!(step > 3_000.0 && step.is_finite(), "{step}");
        assert!(update_reason(Some(&Basis { toward: toward(0.0105, 0.0), ..basis }), 1.0, 1, toward(0.0106, 0.0), relief, TEXEL, map).is_some());
    }

    #[test]
    fn no_update_while_sun_and_heights_stand() {
        let light = toward(0.4, 2.0);
        let basis = Basis { toward: light, heightfield_version: 7, requested_at: 0.0 };
        let (relief, longest) = (3_000.0, 185_000.0);
        assert_eq!(update_reason(None, 0.0, 7, light, relief, TEXEL, longest), Some(Reason::First));
        assert_eq!(update_reason(Some(&basis), 10.0, 7, light, relief, TEXEL, longest), None);
        // A move whose edge shift stays under half a texel.
        let small = toward(0.4 + 1.0e-5, 2.0);
        assert!(edge_displacement(light, small, relief, longest) < 0.5 * TEXEL);
        assert_eq!(update_reason(Some(&basis), 10.0, 7, small, relief, TEXEL, longest), None);
        // New heights: a new map, but not within the floor.
        assert_eq!(update_reason(Some(&basis), 10.0, 8, light, relief, TEXEL, longest), Some(Reason::Heights));
        assert_eq!(update_reason(Some(&basis), 0.05, 8, light, relief, TEXEL, longest), None);
        // A move past half a texel.
        let big = toward(0.41, 2.0);
        assert!(matches!(update_reason(Some(&basis), 10.0, 7, big, relief, TEXEL, longest), Some(Reason::SunMoved(d)) if d > 0.5 * TEXEL));
    }

    /// The smallest elevation step (radians) that asks for a new map at `deg` degrees.
    fn allowed_step(deg: f32, relief: f32) -> f32 {
        let e = deg.to_radians();
        let basis = Basis { toward: toward(e, 1.0), heightfield_version: 1, requested_at: 0.0 };
        let (mut lo, mut hi) = (0.0f32, 0.5f32);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if update_reason(Some(&basis), 1.0, 1, toward(e + mid, 1.0), relief, TEXEL, 1.0e9).is_some() {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        hi
    }

    #[test]
    fn low_sun_updates_more_often() {
        let relief = 2_000.0;
        let steps: Vec<f32> = [5.0, 15.0, 45.0].iter().map(|&d| allowed_step(d, relief)).collect();
        assert!(steps[0] < steps[1] && steps[1] < steps[2], "{steps:?}");
        // Half a texel over h·csc²(e): 0.028°, 0.246°, 1.83° for 2,000 units of relief.
        for (got, want) in steps.iter().zip([0.0279f32, 0.2456, 1.833]) {
            assert!((got.to_degrees() - want).abs() / want < 0.05, "{} against {want}", got.to_degrees());
        }
        // Twice the relief, half the step.
        assert!((allowed_step(15.0, 2.0 * relief) / steps[1] - 0.5).abs() < 0.02);
    }

    /// The producer's main-world system alone, with a sun low in the west (0.2 rad) and no
    /// render world: a test stores [`JobsDone`] itself where the render world would.
    fn producer_app() -> App {
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<JobsDone>()
            .init_resource::<Producer>()
            .init_resource::<ShadowJob>()
            .init_resource::<TerrainShadows>()
            .init_resource::<SunHeightfield>()
            .add_systems(Update, update_terrain_shadows);
        // Game (x, y, z) is Bevy (x, z, −y); the sun's back is the direction towards it.
        let light = toward(0.2, -std::f32::consts::FRAC_PI_2);
        let bevy = Vec3::new(light.x, light.z, -light.y);
        app.world_mut().spawn((crate::sky::Sun, Transform::IDENTITY.looking_to(-bevy, Vec3::Y)));
        app
    }

    /// T0 publishes 16 × 16 heights of generation `generation`, version `version`: flat
    /// ground with a north-south ridge `ridge` high (another worldspace: another ridge).
    fn publish_heights(app: &mut App, generation: u64, version: u64, ridge: f32) {
        let side = 16usize;
        let heights: Vec<f32> = (0..side * side).map(|i| if i % side == 3 { ridge } else { 0.0 }).collect();
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
        *app.world_mut().resource_mut::<SunHeightfield>() = SunHeightfield {
            heights,
            valid: vec![1; side * side],
            origin: Vec2::ZERO,
            texel: TEXEL,
            side,
            version,
            generation,
            min: 0.0,
            max: ridge,
            valid_texels: side * side,
            image: Some(image),
        };
    }

    fn job_id(app: &App) -> u64 {
        app.world().resource::<ShadowJob>().0.as_ref().expect("a job for the render world").id
    }

    /// The render world has recorded job `id` ([`produce_terrain_shadows`]).
    fn record(app: &App, id: u64) {
        app.world().resource::<JobsDone>().0.store(id, Ordering::Release);
    }

    /// Review of f8d532cca, finding 2: travel to another worldspace, T0 emptying and
    /// publishing the new world's first heights within one update (no empty frame for T1 to
    /// see). The old world's map goes at once (the neutral input), and the next map is of
    /// the new world.
    #[test]
    fn world_change_drops_the_published_map() {
        let mut app = producer_app();
        publish_heights(&mut app, 1, 1, 500.0);
        app.update();
        let first = job_id(&app);
        record(&app, first);
        app.update();
        let out = app.world().resource::<TerrainShadows>();
        assert!(out.is_active() && out.heightfield_generation == 1 && out.heightfield_version == 1);
        // Another worldspace: a new generation, the heights already there.
        publish_heights(&mut app, 2, 3, 900.0);
        app.update();
        let out = app.world().resource::<TerrainShadows>();
        assert!(!out.is_active(), "the earlier worldspace's map is no longer published");
        assert_eq!(out.reason, "heightfield started over");
        // A job of the new heights is asked for at once; until it is recorded, nothing shows.
        let second = job_id(&app);
        assert!(second > first);
        assert_eq!(app.world().resource::<Producer>().pending.as_ref().map(|p| (p.generation, p.heightfield_version)), Some((2, 3)));
        app.update();
        assert!(!app.world().resource::<TerrainShadows>().is_active());
        record(&app, second);
        app.update();
        let out = app.world().resource::<TerrainShadows>();
        assert!(out.is_active() && out.heightfield_generation == 2 && out.heightfield_version == 3);
    }

    /// Review of f8d532cca, finding 2: a job asked for in one worldspace and recorded by the
    /// render world after the travel is never published; the new world's job is.
    #[test]
    fn stale_world_job_is_not_published() {
        let mut app = producer_app();
        publish_heights(&mut app, 1, 1, 500.0);
        app.update();
        let old = job_id(&app);
        // Travel, and the render world finishes the old world's job in the same frame.
        publish_heights(&mut app, 2, 2, 900.0);
        record(&app, old);
        app.update();
        let out = app.world().resource::<TerrainShadows>();
        assert!(!out.is_active(), "the earlier worldspace's job was published");
        let new = job_id(&app);
        assert!(new > old);
        assert_eq!(app.world().resource::<Producer>().pending.as_ref().map(|p| p.generation), Some(2));
        app.update();
        assert!(!app.world().resource::<TerrainShadows>().is_active());
        record(&app, new);
        app.update();
        let out = app.world().resource::<TerrainShadows>();
        assert!(out.is_active() && out.heightfield_generation == 2 && out.heightfield_version == 2);
        // Same generation, new heights (T0's ordinary update): the map stays until the next.
        publish_heights(&mut app, 2, 3, 950.0);
        app.update();
        assert!(app.world().resource::<TerrainShadows>().is_active(), "new heights of the same world keep the map");
    }

    #[test]
    fn world_to_shadow_uv_matches_heightfield() {
        let mut g = HeightGrid::new(TEXEL, SIDE);
        g.follow((-3, -2));
        let origin = g.origin().unwrap();
        let (uv_scale, uv_offset) = TerrainShadows::uv_transform(origin, TEXEL, SIDE as u32);
        let map = TerrainShadows { origin, texel: TEXEL, side: SIDE as u32, uv_scale, uv_offset, ..default() };
        let c = CELL_SIZE;
        let points = [
            (-1.0, -1.0),
            (-c + 3.5 * TEXEL, -c + 2.5 * TEXEL),
            (0.0, 0.0),
            (-19.0 * c + 0.1, -18.0 * c + 0.1),
            (13.0 * c - 0.5, 14.0 * c - 0.5),
            (5_000.0, -60_000.0),
        ];
        for (gx, gy) in points {
            // Game (x, y) is Bevy (x, −z); the height does not matter.
            let uv = map.world_to_uv(Vec3::new(gx, 123.0, -gy));
            assert!((0.0..1.0).contains(&uv.x) && (0.0..1.0).contains(&uv.y), "({gx}, {gy}): {uv}");
            let (col, row) = ((uv.x * SIDE as f32).floor() as usize, (uv.y * SIDE as f32).floor() as usize);
            assert_eq!(Some(row * SIDE + col), g.texel_index(gx, gy), "({gx}, {gy})");
        }
        // North is +v, east is +u: Bevy −z is north.
        let a = map.world_to_uv(Vec3::new(0.0, 0.0, 0.0));
        let b = map.world_to_uv(Vec3::new(TEXEL, 0.0, -TEXEL));
        assert!(((b - a) * SIDE as f32 - Vec2::ONE).length() < 1e-3);
        // Outside the window: outside 0..1.
        for (gx, gy) in [(-19.0 * c - 1.0, 0.0), (13.0 * c, 0.0), (0.0, 14.0 * c + 1.0)] {
            let uv = map.world_to_uv(Vec3::new(gx, 0.0, -gy));
            assert!(!((0.0..1.0).contains(&uv.x) && (0.0..1.0).contains(&uv.y)), "({gx}, {gy}): {uv}");
            assert_eq!(g.texel_index(gx, gy), None);
        }
    }

    /// `side` × `side` texels at height 0 with flat-topped ridges: columns `cols` at `h`.
    fn ridges(side: usize, bands: &[(std::ops::RangeInclusive<usize>, f32)]) -> (Vec<f32>, Vec<u8>) {
        let heights = (0..side * side)
            .map(|i| bands.iter().filter(|(c, _)| c.contains(&(i % side))).map(|&(_, h)| h).fold(0.0f32, f32::max))
            .collect();
        (heights, vec![1; side * side])
    }

    #[test]
    fn march_matches_reference_on_ridge() {
        let side = 96;
        // A two-texel ridge and a wide one, both flat-topped, at least as long along every
        // ray as the march's longest step here (asserted below).
        let (heights, valid) = ridges(side, &[(40..=41, 500.0), (60..=67, 800.0)]);
        let lights = [
            toward(0.25f32.atan(), -std::f32::consts::FRAC_PI_2),  // from the west
            toward(0.12f32.atan(), -1.9),                          // west-south-west, oblique
            toward(0.03f32.atan(), -1.4),                          // low, west-north-west
            toward(0.06f32.atan(), std::f32::consts::FRAC_PI_2),   // from the east
        ];
        for (k, light) in lights.into_iter().enumerate() {
            let p = params_for(&heights, &valid, side, light);
            let reference = shadow_height_reference(&heights, &valid, side, TEXEL, light);
            let ours = produce_map(&heights, &valid, &p);
            let mut shaded = 0;
            for row in 0..side {
                for col in 0..side {
                    let i = row * side + col;
                    let reach = texel_reach(&p, (col, row));
                    let step = max_step(&p, reach);
                    assert!(step <= 2.0, "light {k}: step {step} longer than the narrow ridge");
                    // Below the lowest height (0) nothing is shaded: compare above it.
                    let (a, r) = (ours[i].0.max(0.0), reference[i].max(0.0));
                    assert!(a <= r + 1e-2, "light {k} ({col}, {row}): march {a} darker than the reference {r}");
                    let bound = p.tan_texel * step + 1e-2;
                    assert!(r - a <= bound, "light {k} ({col}, {row}): march {a}, reference {r}, bound {bound}");
                    if k == 0 {
                        // Along a row with half-texel steps every texel is visited: exact.
                        assert!((a - r).abs() < 1e-2, "({col}, {row}): {a} against {r}");
                    }
                    assert!(!shadowed(0.0, ours[i].0) || shadowed(0.0, reference[i]), "light {k} ({col}, {row}): shaded where the reference is lit");
                    shaded += usize::from(shadowed(0.0, ours[i].0));
                    // G: the caster's distance, consistent with S's fall.
                    if ours[i].0 > NO_SHADOW {
                        let h = heights.iter().copied().fold(0.0f32, f32::max);
                        assert!(ours[i].1 >= 0.0 && ours[i].0 <= h - ours[i].1 * p.tan_texel / TEXEL + 1e-2);
                    } else {
                        assert_eq!(ours[i].1, 0.0);
                    }
                }
            }
            assert!(shaded > side * 4, "light {k}: the ridges shade something ({shaded})");
        }
    }

    #[test]
    fn march_reaches_casters_near_the_map_edge() {
        let side = 96;
        // Receiver (50, 94), two rows from the north edge, the light low in the west-north-west
        // (direction (−0.98545, 0.16997), 7.68 units lost per texel): its ray enters the narrow
        // ridge's column 41 at (50.5 − 42) / 0.98545 = 8.626 texels and leaves the map through
        // the north edge at (96 − 94.5) / 0.16997 = 8.825, so inside the map the ridge is 0.2
        // texel long along it. With half-texel steps the sample at 8.5 lies in column 42 and the
        // one at 9.0 outside the map: before the edge inset the march found nothing there.
        let (heights, valid) = ridges(side, &[(40..=41, 500.0), (60..=67, 800.0)]);
        let light = toward(0.03f32.atan(), -1.4);
        let p = params_for(&heights, &valid, side, light);
        let reach = texel_reach(&p, (50, 94));
        assert!((reach - 8.825).abs() < 1e-2, "{reach}");
        let reference = shadow_height_reference(&heights, &valid, side, TEXEL, light)[94 * side + 50];
        assert!((reference - (500.0 - 8.626 * 7.68)).abs() < 2e-2, "{reference}");
        let (s, g) = produce_texel(&heights, &valid, (50, 94), &p);
        assert!((s - reference).abs() < 1e-2, "march {s}, reference {reference}");
        assert!((g - 8.626 * TEXEL).abs() < 0.5, "{g}");
        // A wall one texel wide in the map's last column, the light low in the east-north-east:
        // along a ray the wall is 1 / 0.932 = 1.07 texels long, shorter than the last step of a
        // march over 48 texels (1.6 texels at the west edge). The last sample, just inside the
        // edge, still lands in it: within the stated bound everywhere.
        let (heights, valid) = ridges(side, &[(side - 1..=side - 1, 1_000.0)]);
        let light = toward(0.02f32.atan(), 1.2);
        let p = params_for(&heights, &valid, side, light);
        let reference = shadow_height_reference(&heights, &valid, side, TEXEL, light);
        let ours = produce_map(&heights, &valid, &p);
        let mut long = 0;
        for row in 0..side {
            for col in 0..side - 1 {
                let i = row * side + col;
                let reach = texel_reach(&p, (col, row));
                let step = max_step(&p, reach);
                let (a, r) = (ours[i].0.max(0.0), reference[i].max(0.0));
                assert!(a <= r + 1e-2, "wall ({col}, {row}): march {a} darker than the reference {r}");
                assert!(r - a <= p.tan_texel * step + 1e-2, "wall ({col}, {row}): march {a}, reference {r}, step {step}");
                long += usize::from(r > 0.0 && reach > 48.0 && step > 1.0 / p.dir.x);
            }
        }
        // About 1,600 receivers whose last step is longer than the wall.
        assert!(long > 1_000, "{long}");
    }

    #[test]
    fn march_matches_reference_at_low_sun_from_every_side() {
        let side = 96;
        let (heights, valid) = ridges(side, &[(40..=41, 500.0), (60..=67, 800.0)]);
        for tan_e in [0.03f32, 0.06] {
            for azimuth in [-2.8f32, -2.2, -1.0, -0.4, 0.3, 0.9, 1.3, 2.0, 2.6, 3.0] {
                let light = toward(tan_e.atan(), azimuth);
                let p = params_for(&heights, &valid, side, light);
                let reference = shadow_height_reference(&heights, &valid, side, TEXEL, light);
                let ours = produce_map(&heights, &valid, &p);
                let mut shaded = 0;
                for row in 0..side {
                    for col in 0..side {
                        let i = row * side + col;
                        let step = max_step(&p, texel_reach(&p, (col, row)));
                        // The narrow ridge is at least 2 texels long along any ray.
                        assert!(step <= 2.0, "{tan_e} {azimuth}: step {step} longer than the narrow ridge");
                        let (a, r) = (ours[i].0.max(0.0), reference[i].max(0.0));
                        assert!(a <= r + 1e-2, "{tan_e} {azimuth} ({col}, {row}): march {a} darker than the reference {r}");
                        // The bound holds for receivers on the ground, whose rays cross a ridge
                        // whole or until the map's edge. A ray from a receiver on a ridge can
                        // leave it sideways right after its own texel: a caster shorter than the
                        // step, which the bound leaves out (at azimuth 2.6 the ray from (41, 1)
                        // crosses (41, 0) for 0.39 texel between samples at 0.5 and 1.0).
                        if heights[i] == 0.0 {
                            let bound = p.tan_texel * step + 1e-2;
                            assert!(r - a <= bound, "{tan_e} {azimuth} ({col}, {row}): march {a}, reference {r}, bound {bound}");
                        }
                        assert!(!shadowed(0.0, ours[i].0) || shadowed(0.0, reference[i]), "{tan_e} {azimuth} ({col}, {row}): shaded where the reference is lit");
                        shaded += usize::from(shadowed(0.0, ours[i].0));
                    }
                }
                assert!(shaded > side * 4, "{tan_e} {azimuth}: the ridges shade something ({shaded})");
            }
        }
    }

    #[test]
    fn march_never_shadows_flat_ground() {
        let side = 48;
        let mut heights = vec![100.0f32; side * side];
        let mut valid = vec![1u8; side * side];
        // A hole of unknown terrain: casts nothing, gets nothing.
        for row in 10..20 {
            for col in 30..40 {
                heights[row * side + col] = INVALID_HEIGHT;
                valid[row * side + col] = 0;
            }
        }
        for light in [toward(0.05, 0.3), toward(0.2, -2.0), toward(1.0, 3.0), toward(0.011, 1.57)] {
            let p = params_for(&heights, &valid, side, light);
            let map = produce_map(&heights, &valid, &p);
            assert!(map.iter().all(|&(s, g)| !shadowed(100.0, s) && s == NO_SHADOW && g == 0.0), "light {light}");
        }
        // A step of terrain far lower than the plateau's edge does not matter to the plateau.
        heights.iter_mut().zip(&valid).filter(|(_, &v)| v != 0).take(side).for_each(|(h, _)| *h = 0.0);
        let p = params_for(&heights, &valid, side, toward(0.1, 0.0));
        let map = produce_map(&heights, &valid, &p);
        for (i, &(s, _)) in map.iter().enumerate() {
            if valid[i] != 0 && heights[i] == 100.0 {
                assert!(!shadowed(100.0, s), "texel {i}: {s}");
            }
        }
        // No light to march towards: nothing anywhere.
        assert_eq!(march_params(toward(0.005, 0.0), TEXEL, side as u32, 0.0, 100.0), Err("light at or under the horizon"));
        assert_eq!(march_params(Vec3::Z, TEXEL, side as u32, 0.0, 100.0), Err("light overhead"));
        let off = MarchParams { steps: 0, ..p };
        assert!(produce_map(&heights, &valid, &off).iter().all(|&(s, _)| s == NO_SHADOW));
    }

    #[test]
    fn slope_overshoot_is_within_a_texel_of_relief() {
        // Ground rising towards the light at slope g < tan(e): never shaded in truth, but each
        // texel holds its highest (sunward) edge, so S may stand above the receiver's own
        // texel by up to the relief inside one texel (module notes, "Bias").
        let side = 40;
        let (g, tan_e) = (0.08f32, 0.2f32);
        let heights: Vec<f32> = (0..side * side).map(|i| ((i % side) as f32 + 1.0) * TEXEL * g).collect();
        let valid = vec![1u8; side * side];
        let p = params_for(&heights, &valid, side, toward(tan_e.atan(), std::f32::consts::FRAC_PI_2));
        let map = produce_map(&heights, &valid, &p);
        for row in 0..side {
            for col in 0..side - 1 {
                let i = row * side + col;
                let (s, own) = (map[i].0, heights[i]);
                assert!(s <= own + g * TEXEL + 1e-2, "({col}, {row}): S {s} over {own} by more than a texel's relief");
                // The true ground at the texel's centre lies half a texel's relief under the
                // texel's maximum: unbiased it reads as shaded (acne) ...
                let ground = own - 0.5 * g * TEXEL;
                assert!(shadowed(ground, s), "({col}, {row}): S {s}, ground {ground}");
                // ... and a bias of texel · slope lights it.
                assert!(!shadowed(ground + g * TEXEL, s), "({col}, {row})");
            }
        }
    }

    #[test]
    fn march_ends_at_the_map_edge() {
        let side = 32;
        // A wall along the west edge, the light in the west.
        let (heights, valid) = ridges(side, &[(0..=0, 1_000.0)]);
        let west = toward(0.1f32.atan(), -std::f32::consts::FRAC_PI_2);
        let p = params_for(&heights, &valid, side, west);
        let reference = shadow_height_reference(&heights, &valid, side, TEXEL, west);
        for row in 0..side {
            // The wall itself has nothing west of it inside the map: no shadow.
            assert_eq!(produce_texel(&heights, &valid, (0, row), &p), (NO_SHADOW, 0.0));
            for col in 1..side {
                let (s, g) = produce_texel(&heights, &valid, (col, row), &p);
                let d = (col as f32 - 0.5) * TEXEL;
                assert!((s - reference[row * side + col]).abs() < 1e-2 || (s.max(0.0) - reference[row * side + col].max(0.0)).abs() < 1e-2, "({col}, {row})");
                if s > 0.0 {
                    assert!((s - (1_000.0 - d * 0.1)).abs() < 1e-1 && (g - d).abs() < 1e-1, "({col}, {row}): {s} {g}");
                }
            }
        }
        // The light in the east: the march from the east column leaves the map at once.
        let east = toward(0.1f32.atan(), std::f32::consts::FRAC_PI_2);
        let p = params_for(&heights, &valid, side, east);
        assert!((texel_reach(&p, (side - 1, 5)) - 0.5).abs() < 1e-4);
        for row in 0..side {
            assert_eq!(produce_texel(&heights, &valid, (side - 1, row), &p), (NO_SHADOW, 0.0));
            // The wall is behind every receiver: nothing shades them.
            for col in 1..side {
                assert!(!shadowed(0.0, produce_texel(&heights, &valid, (col, row), &p).0));
            }
        }
        // Diagonal lights towards every corner: the march stays inside (an index outside
        // the map would panic in the mirror).
        for az in [0.7f32, 2.4, -2.4, -0.7] {
            let p = params_for(&heights, &valid, side, toward(0.02, az));
            let _ = produce_map(&heights, &valid, &p);
        }
    }

    #[test]
    fn steps_grow_with_reach() {
        let p = MarchParams { dir: Vec2::X, tan_texel: 1.0, texel: TEXEL, top: 0.0, bottom: 0.0, side: 512, steps: STEPS, step_min: STEP_MIN };
        assert_eq!(max_step(&p, 30.0), STEP_MIN);
        assert_eq!(max_step(&p, 48.0), STEP_MIN);
        let full = 512.0 * std::f32::consts::SQRT_2;
        assert!((max_step(&p, full) - 14.5).abs() < 0.1, "{}", max_step(&p, full));
        // The last sample lands on the reach.
        let b = step_growth(&p, full);
        let n = STEPS as f32;
        assert!((STEP_MIN * n + b * n * n - full).abs() < 1e-2);
    }

    #[test]
    fn shader_is_valid_wgsl() {
        use wgpu::naga;
        // No shader definitions: this one text is the only variant.
        let source = include_str!("terrain_shadows.wesl");
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
        // The bindings the Rust layout makes (`layout_descriptor`), in its order.
        let binding = |name: &str| {
            let (_, v) = module.global_variables.iter().find(|(_, v)| v.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("{name}"));
            let b = v.binding.as_ref().unwrap();
            (b.group, b.binding, v.ty)
        };
        assert_eq!((binding("heights").0, binding("heights").1), (0, 0));
        assert_eq!((binding("shadow_out").0, binding("shadow_out").1), (0, 1));
        let (group, slot, ty) = binding("params");
        assert_eq!((group, slot), (0, 2));
        match &module.types[ty].inner {
            naga::TypeInner::Struct { members, span } => {
                assert_eq!(*span as usize, PARAMS_BYTES);
                let offsets: Vec<(&str, u32)> = members.iter().map(|m| (m.name.as_deref().unwrap_or(""), m.offset)).collect();
                for (name, offset) in [("dir", 0), ("tan_texel", 8), ("texel", 12), ("top", 16), ("bottom", 20), ("side", 24), ("steps", 28), ("step_min", 32)] {
                    assert!(offsets.contains(&(name, offset)), "{name} at {offset}: {offsets:?}");
                }
            }
            other => panic!("params is {other:?}"),
        }
        match &module.types[binding("shadow_out").2].inner {
            naga::TypeInner::Image { class: naga::ImageClass::Storage { format, access }, .. } => {
                assert_eq!(*format, naga::StorageFormat::Rgba32Float);
                assert_eq!(OUTPUT_FORMAT, TextureFormat::Rgba32Float);
                assert_eq!(OUTPUT_FORMAT.block_copy_size(None), Some(OUTPUT_TEXEL_BYTES as u32));
                assert!(access.contains(naga::StorageAccess::STORE));
            }
            other => panic!("shadow_out is {other:?}"),
        }
        let entry = module.entry_points.iter().find(|e| e.name == "produce").expect("entry point `produce`");
        assert_eq!(entry.stage, naga::ShaderStage::Compute);
        assert_eq!(entry.workgroup_size, [WORKGROUP, WORKGROUP, 1]);
        // The CPU mirror's constants.
        assert!(source.contains("const NO_SHADOW: f32 = -1.0e30;"));
        assert_eq!(NO_SHADOW, -1.0e30);
        assert!(source.contains("const EDGE_INSET: f32 = 1.0e-3;"));
        assert_eq!(EDGE_INSET, 1.0e-3);
        assert!(source.contains("let end = max(reach - EDGE_INSET, 0.0);") && source.contains("let t = min(params.step_min * fi + b * fi * fi, end);"));
        // The uniform bytes put each field where the shader reads it.
        let p = MarchParams { dir: Vec2::new(0.6, -0.8), tan_texel: 51.2, texel: TEXEL, top: 900.0, bottom: -20.0, side: 512, steps: STEPS, step_min: STEP_MIN };
        let bytes = params_bytes(&p);
        let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let u = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        assert_eq!((f(0), f(4), f(8), f(12), f(16), f(20), u(24), u(28), f(32)), (0.6, -0.8, 51.2, TEXEL, 900.0, -20.0, 512, STEPS, STEP_MIN));
        // B: the relief, mirrored by `texel_relief`.
        assert!(source.contains("result.z = relief(own, h0.r);"));
        assert!(source.contains("return axis_relief(own, vec2<i32>(1, 0), h0) + axis_relief(own, vec2<i32>(0, 1), h0);"));
    }

    /// The relief (the output's B, the receivers' bias): a uniform slope's rise per texel on
    /// each axis, summed; 0 on flat ground and at a cliff's flat foot or top; one side alone
    /// at the map's edge or beside an invalid texel; 0 for an invalid texel.
    #[test]
    fn relief_is_the_slope_not_the_cliff() {
        let side = 8;
        // A plane rising 0.1 per unit east and 0.05 north.
        let plane: Vec<f32> = (0..side * side).map(|i| 0.1 * TEXEL * (i % side) as f32 + 0.05 * TEXEL * (i / side) as f32).collect();
        let valid = vec![1u8; side * side];
        for (col, row) in [(3, 3), (0, 0), (7, 4), (2, 7)] {
            let r = texel_relief(&plane, &valid, side, (col, row));
            assert!((r - 0.15 * TEXEL).abs() < 1e-2, "({col}, {row}): {r}");
        }
        // A cliff 900 units high between columns 3 and 4 on flat ground.
        let cliff: Vec<f32> = (0..side * side).map(|i| if i % side >= 4 { 900.0 } else { 0.0 }).collect();
        for col in 0..side {
            assert_eq!(texel_relief(&cliff, &valid, side, (col, 2)), 0.0, "column {col}: flat on one side at least");
        }
        // A one-texel wall: both its sides drop, so it keeps its relief.
        let wall: Vec<f32> = (0..side * side).map(|i| if i % side == 4 { 900.0 } else { 0.0 }).collect();
        assert_eq!(texel_relief(&wall, &valid, side, (4, 2)), 900.0);
        // An invalid neighbour counts as missing; an invalid texel has none.
        let mut holed = valid.clone();
        holed[3 * side + 2] = 0;
        assert!((texel_relief(&plane, &holed, side, (3, 3)) - 0.15 * TEXEL).abs() < 1e-2, "the east side alone");
        assert_eq!(texel_relief(&plane, &holed, side, (2, 3)), 0.0);
        // The output carries it beside S and G.
        let p = march_params(Vec3::new(0.0, 0.8, 0.6), TEXEL, side as u32, 0.0, 900.0).unwrap();
        let out = produce_output(&cliff, &valid, &p);
        assert_eq!(out.len(), side * side);
        assert!(out.iter().all(|t| t.z == 0.0 && t.w == 0.0));
        let out = produce_output(&plane, &valid, &p);
        assert!((out[3 * side + 3].z - 0.15 * TEXEL).abs() < 1e-2);
    }

    /// The settings row decides whether the views take the terrain variant, not whether a
    /// map is published: switched on, the views keep their markers through a publication,
    /// its loss (indoors, the light at the horizon) and a new one in the other target; the
    /// slot is neutral meanwhile. Switching off takes everything away; the debug override
    /// holds it on whatever the row says.
    #[test]
    fn terrain_bindings_follow_the_switch() {
        use crate::graphics::{Graphics, Preset};
        use crate::sun_visibility::{mark_targets, marking_needed, SunVisibility, SunVisibilityUniform, SunVisibilityView, MASK_TERRAIN};
        let mut app = App::new();
        app.insert_resource(Graphics::preset(Preset::Vanilla))
            .insert_resource(TerrainShadowsForced(false))
            .init_resource::<TerrainShadowsWanted>()
            .init_resource::<TerrainShadowsProducerWanted>()
            .init_resource::<TerrainShadows>()
            .init_resource::<SunVisibility>()
            .insert_resource(crate::sky::SkyState::default())
            .add_systems(
                Update,
                (follow_graphics.run_if(resource_exists_and_changed::<Graphics>), feed_sun_visibility.run_if(feeding_needed), mark_targets.run_if(marking_needed)).chain(),
            );
        let light = DirectionalLight { shadow_maps_enabled: true, ..default() };
        let sun = app.world_mut().spawn((crate::sky::Sun, light, crate::sky::shadow_cascades(8_000.0, 1.0, crate::sky::CascadeLayout::Spaced(3), 2048))).id();
        let cam = app.world_mut().spawn(crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }).id();
        let vis = |app: &App| app.world().resource::<SunVisibility>().clone();
        app.update();
        assert!(!app.world().resource::<TerrainShadowsWanted>().0, "off by default");
        assert!(!vis(&app).active());
        assert!(app.world().get::<SunVisibilityView>(cam).is_none());

        // The row on, nothing published yet: the variant and the markers, a neutral slot.
        app.world_mut().resource_mut::<Graphics>().terrain_shadows = true;
        app.update();
        assert!(app.world().resource::<TerrainShadowsWanted>().0);
        let v = vis(&app);
        assert!(v.terrain_enabled && v.terrain.is_none() && v.terrain_on());
        assert_eq!(SunVisibilityUniform::pack(&v).mode.x & MASK_TERRAIN, 0, "neutral: T = 1");
        assert!(app.world().get::<SunVisibilityView>(cam).is_some());

        // A map published: the slot carries it, the band from the sun's cascades.
        let mut images = Assets::<Image>::default();
        let (front, back) = (images.add(Image::default()), images.add(Image::default()));
        let (uv_scale, uv_offset) = TerrainShadows::uv_transform(Vec2::new(-1_000.0, 2_000.0), TEXEL, 512);
        let published = |image: Handle<Image>, version: u64| TerrainShadows { image: Some(image), texel: TEXEL, side: 512, uv_scale, uv_offset, version, ..default() };
        *app.world_mut().resource_mut::<TerrainShadows>() = published(front.clone(), 1);
        app.update();
        let t = vis(&app).terrain.expect("published");
        assert_eq!((t.image.id(), t.texel, t.side, t.uv_scale, t.uv_offset), (front.id(), TEXEL, 512, uv_scale, uv_offset));
        assert_eq!(SunVisibilityUniform::pack(&vis(&app)).mode.x & MASK_TERRAIN, MASK_TERRAIN);

        // The next publication is the other target: the slot follows the handle.
        *app.world_mut().resource_mut::<TerrainShadows>() = published(back.clone(), 2);
        app.update();
        assert_eq!(vis(&app).terrain.expect("published").image.id(), back.id());

        // Gone dark (indoors, the light at the horizon): neutral again, markers kept.
        *app.world_mut().resource_mut::<TerrainShadows>() = TerrainShadows { version: 3, reason: "light at or under the horizon", ..default() };
        app.update();
        let v = vis(&app);
        assert!(v.terrain_enabled && v.terrain.is_none());
        assert!(app.world().get::<SunVisibilityView>(cam).is_some(), "the views keep the terrain variant");

        // Sun shadow maps off: the map is still fed (C = 1 then, so Vsolid = T).
        *app.world_mut().resource_mut::<TerrainShadows>() = published(front.clone(), 4);
        app.world_mut().get_mut::<DirectionalLight>(sun).unwrap().shadow_maps_enabled = false;
        app.update();
        assert_eq!(vis(&app).terrain.expect("published").image.id(), front.id());

        // The row off: nothing, even with a map still standing.
        app.world_mut().resource_mut::<Graphics>().terrain_shadows = false;
        app.update();
        let v = vis(&app);
        assert!(!v.terrain_on() && !v.active());
        app.update();
        assert!(app.world().get::<SunVisibilityView>(cam).is_none());

        // The override wins over the row.
        app.world_mut().resource_mut::<TerrainShadowsForced>().0 = true;
        app.world_mut().resource_mut::<Graphics>().set_changed();
        app.update();
        assert!(app.world().resource::<TerrainShadowsWanted>().0 && vis(&app).terrain_enabled);
    }

    /// The light in the air (#3032 stage 3) keeps the producer alive without the surfaces:
    /// off by default; the air alone turns the map's production and T0's heightfield on and
    /// leaves the sun visibility's terrain slot off even with a map published; the row alone
    /// is as before; the row off while the air holds the producer clears the surfaces only;
    /// the air off again with the row off stops the production as the row going off does.
    #[test]
    fn light_in_the_air_drives_the_producer_not_the_surfaces() {
        use crate::graphics::{Graphics, Preset};
        use crate::medium_air_demand::AirWanted;
        use crate::sun_visibility::SunVisibility;
        let mut app = App::new();
        app.insert_resource(Graphics::preset(Preset::Vanilla))
            .insert_resource(TerrainShadowsForced(false))
            .init_resource::<TerrainShadowsWanted>()
            .init_resource::<TerrainShadowsProducerWanted>()
            .init_resource::<SunHeightfieldWanted>()
            .init_resource::<TerrainShadows>()
            .init_resource::<SunVisibility>()
            .init_resource::<Producer>()
            .init_resource::<ShadowJob>()
            .init_resource::<Assets<Image>>()
            .add_systems(
                Update,
                (
                    follow_graphics.run_if(graphics_or_air_changed),
                    follow_wanted,
                    stop_terrain_shadows.run_if(production_stopped),
                    feed_sun_visibility.run_if(feeding_needed),
                )
                    .chain(),
            );
        let state = |app: &App| {
            let w = app.world();
            (w.resource::<TerrainShadowsWanted>().0, w.resource::<TerrainShadowsProducerWanted>().0, w.resource::<SunHeightfieldWanted>().0)
        };
        let vis = |app: &App| app.world().resource::<SunVisibility>().clone();
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
        let (uv_scale, uv_offset) = TerrainShadows::uv_transform(Vec2::new(-1_000.0, 2_000.0), TEXEL, 512);
        let publish = |app: &mut App, version: u64| {
            *app.world_mut().resource_mut::<TerrainShadows>() = TerrainShadows { image: Some(image.clone()), texel: TEXEL, side: 512, uv_scale, uv_offset, version, ..default() };
        };

        // Off by default, with no demand resource and with a false one.
        app.update();
        assert_eq!(state(&app), (false, false, false), "off by default");
        app.insert_resource(AirWanted(false));
        app.update();
        assert_eq!(state(&app), (false, false, false), "air off: off");

        // The air alone: the producer and T0 on, the surfaces off even with a map published.
        app.insert_resource(AirWanted(true));
        app.update();
        assert_eq!(state(&app), (false, true, true), "the air turns the producer on, not the surfaces");
        publish(&mut app, 1);
        app.update();
        let v = vis(&app);
        assert!(!v.terrain_enabled && v.terrain.is_none() && !v.terrain_on(), "the surfaces are not fed");
        assert!(app.world().resource::<TerrainShadows>().is_active(), "the publication stands for the air");

        // The row on as well: the surfaces as before; the row off again: only they are cleared.
        app.world_mut().resource_mut::<Graphics>().terrain_shadows = true;
        app.update();
        assert_eq!(state(&app), (true, true, true));
        assert!(vis(&app).terrain_enabled && vis(&app).terrain.is_some());
        app.world_mut().resource_mut::<Graphics>().terrain_shadows = false;
        app.update();
        assert_eq!(state(&app), (false, true, true), "the air keeps the producer");
        assert!(!vis(&app).terrain_on(), "the surfaces are cleared as when the row goes off");
        assert!(app.world().resource::<TerrainShadows>().is_active(), "the publication is kept");

        // The air off with the row off: the production stops as the row going off stops it.
        app.insert_resource(AirWanted(false));
        app.update();
        assert_eq!(state(&app), (false, false, false));
        let out = app.world().resource::<TerrainShadows>();
        assert!(!out.is_active() && out.reason == "off" && out.version == 2, "unpublished as the row going off does");
        assert!(app.world().resource::<ShadowJob>().0.is_none());

        // The row alone, as before: everything on, then off with the production.
        app.world_mut().resource_mut::<Graphics>().terrain_shadows = true;
        app.update();
        assert_eq!(state(&app), (true, true, true));
        assert!(vis(&app).terrain_enabled);
        publish(&mut app, 3);
        app.world_mut().resource_mut::<Graphics>().terrain_shadows = false;
        app.update();
        assert_eq!(state(&app), (false, false, false));
        assert!(!vis(&app).terrain_on() && !app.world().resource::<TerrainShadows>().is_active());

        // T0's heightfield asked for on its own (`NVF_PLAY_DEBUG_SUN_HEIGHTFIELD=1` sets it at
        // start) with the row off: an air on/off cycle and a row on/off cycle both leave it on
        // (the production inherited it, so it is not theirs to clear).
        app.world_mut().resource_mut::<SunHeightfieldWanted>().0 = true;
        app.update();
        assert_eq!(state(&app), (false, false, true));
        app.insert_resource(AirWanted(true));
        app.update();
        assert_eq!(state(&app), (false, true, true));
        app.insert_resource(AirWanted(false));
        app.update();
        assert_eq!(state(&app), (false, false, true), "the heightfield's own demand survives the air going off");
        app.world_mut().resource_mut::<Graphics>().terrain_shadows = true;
        app.update();
        app.world_mut().resource_mut::<Graphics>().terrain_shadows = false;
        app.update();
        assert_eq!(state(&app), (false, false, true), "and the row going off");
    }
}
