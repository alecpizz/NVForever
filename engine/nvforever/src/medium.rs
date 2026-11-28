//! The volumetric medium (LootJunkieAzza/NVForever#3032, stage 1: the far buffer). An
//! engineering choice for the enhanced look, never a fact about the original game (which
//! draws its clouds as flat layers on the sky dome) and never used to explain a difference
//! from it. Switched by the cloud source's row ("Volumetric clouds (experimental)",
//! volumetric_clouds.rs), off by default and in every preset.
//!
//! **What it is** [design]: one buffer of light gathered along the view rays beyond the near
//! field, made before the main pass, that the picture is composed from. The technique is the
//! far field of Bauer's "Creating the Atmospheric World of Red Dead Redemption 2" (SIGGRAPH
//! 2019, Advances in Real-Time Rendering): rays marched into a reduced-size target, rebuilt
//! over several frames [documented: Bauer 2019, slides 49-56]. Written from the talk's
//! description; no code from any project.
//!
//! **As built** (docs/design/volumetric-medium.md):
//! - [`follow_medium`] (main world) keeps the published image, a stable `Handle<Image>` the
//!   sky proxy's material binds (a new handle when the size changes: a material's bind group
//!   keeps the texture view it was made with), and decides each frame whether the buffer may
//!   be drawn ([`plan`]: the handover).
//! - The render world puts [`MediumView`] on the main camera's view with the camera's cut
//!   inputs ([`extract_medium`]), reports the size the buffer must have from the main pass's
//!   render size (the upscalers' `MainPassResolutionOverride`, else the viewport), writes the
//!   uniforms and keeps the history ([`prepare_medium`]). [`medium_trace_pass`] runs between
//!   the depth prepass and the main pass (where `ssao.rs` `ao_lighting_pass` sits). All rays
//!   are cast with the view's **unjittered** projection through the cloud source's march
//!   (`vc_trace_refined`, the seam). The published image is `Rgba16Float` (rgb the
//!   premultiplied light, a the transmittance; a miss is (0, 0, 0, 1)).
//!   - **Default** ([`MediumMode::Default`]): the buffer is the render size over the row's level's scale
//!     (a third at Normal, two fifths at High, a quarter at Performance); one ray per
//!     3x3 tile a frame (the corner of [`corner`]) into a trace target of one texel a tile (the scale
//!     and the tile side are parameters: [`scale_in_force`], [`TILE_SIDE`])
//!     (medium_trace.wesl `medium_trace_tiles`), then the reconstruction (medium_resolve.wesl)
//!     into the published image and the next history [documented: Bauer 2019, slides 51-54;
//!     adapted, see medium_resolve.wesl]. History is dropped on the cuts of [`history_cut`].
//!   - **Reference** (`NVF_PLAY_DEBUG_MEDIUM=reference`): every texel of the render size
//!     traced every frame, no history (medium_trace.wesl `medium_trace`): the first
//!     prototype's picture under MSAA.
//! - The sky proxy (volumetric_clouds.rs) reads the image at the screen position its own
//!   world position has under the unjittered projection (medium_upsample.wesl), bilinear,
//!   and writes it premultiplied over the sky as the prototype's march did.
//!
//! **The handover** [design]: the flat cloud layers give way only to a buffer
//! of the right size written by ready pipelines, the proxy's own draw pipeline among them. An
//! epoch counter ([`Ledger::epoch`]) moves on every change that could make the buffer wrong
//! (the source starts or stops asking for a trace, the main view's size, multisampling, HDR,
//! upscaler or jitter changes, a new image); the render world echoes the epoch it traced for,
//! and the volume is shown only while the echo is this epoch's and the write was whole
//! ([`Written::ok`]). Every loss the main world can see hands back to the flat layers in the
//! same frame. A frame the render world alone cannot draw leaves an image that has had a whole
//! write as it was (one frame of last frame's clouds), and clears one that never had.
//!
//! Debug aids (dev_aids.rs: not in release builds; read at the start, usable in a played run):
//! `NVF_PLAY_DEBUG_MEDIUM=reference`,
//! `NVF_PLAY_DEBUG_MEDIUM_VIEW=fresh|age|history|clamped|distance` ([`DebugView`]),
//! `NVF_PLAY_DEBUG_MEDIUM_HISTORY_FILTER=linear|cubic|adaptive` (history interpolation; adaptive by default),
//! `NVF_PLAY_DEBUG_MEDIUM_FRESH=<0..1>` ([`FRESH_SHARE`]), `NVF_PLAY_DEBUG_MEDIUM_TURN=<degrees
//! a second>` (the view turned at a constant rate, for pictures and cost runs mid-turn),
//! `NVF_PLAY_DEBUG_MEDIUM_SCALE=<1..4>` ([`scale_in_force`]), `NVF_PLAY_DEBUG_MEDIUM_TILE=<1|2|3>`
//! ([`TILE_SIDE`]), `NVF_PLAY_DEBUG_MEDIUM_TALLY=1` (every window of history cuts logged,
//! [`CutTally`]).
//!
//! Off: no image, no uniform buffer, no pipeline asked for, the render systems behind a closed
//! gate ([`MediumGate`]), nothing logged.

mod reflection;

use crate::fly::FlyCam;
use crate::upscaler::prep::{UpscaleCamera, UpscaleSettings};
use crate::volumetric_clouds::{needs_write, refined_rays_capped, variant_suffix, view_variant, CloudLevel, CloudVolumeUniform, RAY_BUDGET, VIEWS_DEF};
use bevy::asset::{RenderAssetUsages, UntypedAssetId};
use bevy::camera::MainPassResolutionOverride;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::diagnostic::FrameCount;
use bevy::image::ImageSampler;
use bevy::pbr::{PreparedMaterial, SpecializedMaterialPipelineCache};
use bevy::prelude::*;
use bevy::render::camera::TemporalJitter;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_3d, uniform_buffer};
use bevy::render::render_resource::{
    AddressMode, BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferId, CachedPipelineState, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, Extent3d, FilterMode, FragmentState, LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, SamplerId, ShaderStages, ShaderType, StoreOp, TextureDescriptor,
    TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewId, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::sync_world::{MainEntity, RenderEntity};
use bevy::render::texture::{CachedTexture, GpuImage, TextureCache};
use bevy::render::view::ExtractedView;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::shader::Shader;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The published image: the gathered light (rgb, premultiplied, linear) and the transmittance
/// (a). Half floats: the cloud source's range (hundreds of thousands of units) is beyond them, so the distance has its
/// own target ([`DISTANCE_FORMAT`]).
pub const FAR_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// The transmittance-weighted distance along the ray (world units).
pub const DISTANCE_FORMAT: TextureFormat = TextureFormat::R32Float;
/// The distance of a ray that met nothing (medium_trace.wesl, the march's `vc_empty`).
pub const EMPTY_DISTANCE: f32 = -1.0;
/// What a texel holds before anything is traced into it: no light, everything shows.
const CLEAR_LIGHT: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
const CLEAR_DISTANCE: wgpu::Color = wgpu::Color { r: EMPTY_DISTANCE as f64, g: 0.0, b: 0.0, a: 0.0 };
/// The far end of the medium's stretch of each ray (world units): in this stage the rays are
/// not cut by the scene (sky only), so the source's own range decides.
pub const RAY_END: f32 = 1.0e30;
/// Frames with nothing asked for before the render systems' gate closes.
const IDLE_FRAMES: u32 = 3;
/// The history of the default mode: the light and transmittance (as the published image) and
/// beside it the distance and the age of each texel's value (frames since a ray was cast for
/// it), full floats (the distance is beyond half floats).
pub const HISTORY_AUX_FORMAT: TextureFormat = TextureFormat::Rg32Float;
/// The fresh share: a texel cast this frame takes this share of its new ray and
/// keeps the rest of its history. [engineering choice, a play test of the experiment aids judged by eye,
/// 2026-11-19: the first default 0.5 let half of each new ray's grain through every fourth
/// frame, a shimmer still and moving; shares from 0.05 to 0.2 kept low in motion were all
/// acceptable.] Measured on one machine, still view, 8 shots 70 ms apart, per-pixel temporal
/// standard deviation in 8-bit levels: 0.36 at 0.5, 0.19 at 0.2, 0.14 at 0.1, 0.09 for the
/// reference [observed, 2026-11-19, the development build 992ec21f5: a played run standing at Goodsprings
/// looking up, volumetric-medium.md]. `NVF_PLAY_DEBUG_MEDIUM_FRESH` sets it.
pub const FRESH_SHARE: f32 = 0.1;
/// The aux target's second channel: a texel's own casts since its history became valid times
/// this, plus the frames since its last cast ([`pack_aux`]).
pub const AGE_SPAN: f32 = 16.0;
pub const AGE_CAP: f32 = 15.0;
/// The cast count is held here: from 9 on the floor share decides, and the packed value
/// (at most 255 x 16 + 15) stores exactly in a full float.
pub const COUNT_CAP: f32 = 255.0;

/// The aux channel's value for a cast count and an age (medium_resolve.wesl).
#[cfg_attr(not(test), allow(dead_code))]
pub fn pack_aux(count: f32, age: f32) -> f32 {
    count.clamp(0.0, COUNT_CAP).floor() * AGE_SPAN + age.clamp(0.0, AGE_CAP).floor()
}

/// The cast count and the age back from the aux channel.
#[cfg_attr(not(test), allow(dead_code))]
pub fn unpack_aux(packed: f32) -> (f32, f32) {
    let p = packed.max(0.0);
    let count = (p / AGE_SPAN).floor();
    (count, p - count * AGE_SPAN)
}

/// The share of a fresh cast whose texel has had `count` own casts since its history became
/// valid: max(floor, 1 / (max(count, 1) + 1)), the floor being [`fresh_share`]'s: a
/// running mean down to the floor, the fill of a texel with no history standing in for one cast
/// (the first own cast averages with it; from the second on the two weigh as one).
#[cfg_attr(not(test), allow(dead_code))]
pub fn counted_share(floor: f32, count: f32) -> f32 {
    floor.max(1.0 / (count.max(1.0) + 1.0))
}

/// CPU mirror of medium_resolve.wesl `medium_fresh_blend`: a fresh cast blended into its
/// history per channel; a channel whose step is below the half float's spacing at the
/// history's value moves one or two stored steps toward the cast (never past it); the cast is taken
/// whole only when the history is empty, the cast sees cloud and the blend would still store
/// as empty.
#[cfg_attr(not(test), allow(dead_code))]
pub fn fresh_blend(hist: Vec4, own: Vec4, share: f32) -> Vec4 {
    let empty_t = 1.0 - 1.0 / 4096.0;
    let blended = hist.lerp(own, share);
    if blended.w >= empty_t && own.w < empty_t {
        return own;
    }
    let spacing = (hist.abs() * 0.000_488_281_25).max(Vec4::splat(5.960_464_5e-8));
    let stalled = (blended - hist).abs().cmple(spacing);
    let gap = own - hist;
    let nudged = hist + Vec4::select(gap.cmplt(Vec4::ZERO), -Vec4::ONE, Vec4::ONE) * gap.abs().min(2.0 * spacing);
    Vec4::select(stalled, nudged, blended)
}

/// The share the motion ramp rises to (the same 0.1: a moving view keeps accumulating through
/// the reprojected history; mid-turn stills at 30°/s showed no trail [observed, 2026-11-19, judged by eye: the
/// same build and place, `NVF_PLAY_DEBUG_MEDIUM_TURN=30`]).
/// `NVF_PLAY_DEBUG_MEDIUM_FRESH_MOVING` sets it.
pub const FRESH_MOVING_SHARE: f32 = 0.1;
/// The motion ramp, between these many texels a frame of history motion: the fresh share rises
/// along it to [`FRESH_MOVING_SHARE`], and the default clamp ([`HistoryClamp::Moving`]) fades in
/// along it [engineering choice: one texel of motion is the grain's scale at half size; four is
/// a turn of about 90°/s at 1080p and 60 fps].
pub const MOTION_FROM: f32 = 1.0;
pub const MOTION_FULL: f32 = 4.0;
/// The sky clock's step (sky.rs `follow_game_clock` moves the sky in half-minute steps of game
/// time): a frame may advance the clock by up to two of them more than the time scale predicts.
pub const SKY_CLOCK_STEP: f32 = 1.0 / 120.0;
/// A step of the game clock may be this many times what the time scale and the frame's delta
/// predict before it is a jump [engineering choice: room for frame-time jitter between the
/// game clock's update and the sky's; a wait or sleep is hours, far beyond it].
pub const CLOCK_SLACK: f32 = 2.0;
/// The most `NVF_PLAY_DEBUG_MEDIUM_TURN` turns the view, degrees a second.
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub const MAX_TURN: f32 = 720.0;
/// The default mode's published image is the main pass's render size divided by this at the row's
/// level High, rounded up (2.5, two fifths, the look judged by eye on 2026-11-23; 2, half,
/// before); the other levels' own scales are [`CloudLevel::buffer_scale`]'s [design,
/// 2026-11-23]. `NVF_PLAY_DEBUG_MEDIUM_SCALE` sets it whatever the level, a whole number from
/// [`MIN_SCALE`] to [`MAX_SCALE`] or 2.5 ([`scale_in_force`]). The medium's own, whatever density
/// model the cloud source traces.
pub const BUFFER_SCALE: f64 = 2.5;
pub const MIN_SCALE: u32 = 1;
pub const MAX_SCALE: u32 = 4;
/// The side of the cast tile: one ray per tile of `TILE_SIDE` x `TILE_SIDE` texels a frame (3x3, one in
/// nine, since the look judged by eye on 2026-11-23: all nine texels are cast over nine frames; 2x2, one in
/// four, before). `NVF_PLAY_DEBUG_MEDIUM_TILE=1` casts every texel every frame,
/// still through the history. A side of 4 is not built (docs/design/volumetric-medium.md, "What is
/// not done"). The medium's own, as [`BUFFER_SCALE`].
pub const TILE_SIDE: u32 = 3;

/// How the far buffer is made (`NVF_PLAY_DEBUG_MEDIUM`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediumMode {
    /// Half the render size, one ray in four a frame, rebuilt with the history.
    Default,
    /// Every pixel of the render size traced every frame, no history: the first checkpoint's
    /// picture, which is the first prototype's.
    Reference,
}

/// What the buffer shows instead of the light (`NVF_PLAY_DEBUG_MEDIUM_VIEW`): opaque grey or
/// colour over the sky, for pictures of the reconstruction. Ignored in the reference mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugView {
    None,
    /// Green: a ray was cast for the texel this frame; grey: rebuilt from its history; red: no
    /// valid history (filled from the fresh rays around).
    Fresh,
    /// The age of the texel's value, grey, white at 8 frames.
    Age,
    /// Red where no valid history was found, the picture elsewhere.
    History,
    /// How much the clamp moved the history, grey (x4).
    Clamped,
    /// The weighted distance, grey (white at the source's range, `SourceFrame::distance_scale`),
    /// blue for an empty ray.
    Distance,
}

impl DebugView {
    /// The shader's code (medium_resolve.wesl `params.x`).
    pub fn code(self) -> f32 {
        match self {
            DebugView::None => 0.0,
            DebugView::Fresh => 1.0,
            DebugView::Age => 2.0,
            DebugView::History => 3.0,
            DebugView::Clamped => 4.0,
            DebugView::Distance => 5.0,
        }
    }
}

/// `NVF_PLAY_DEBUG_MEDIUM`: `reference`; anything else (or unset) the default.
pub fn debug_mode(value: Option<&str>) -> MediumMode {
    if value.is_some_and(|v| v.trim().eq_ignore_ascii_case("reference")) { MediumMode::Reference } else { MediumMode::Default }
}

/// `NVF_PLAY_DEBUG_MEDIUM_VIEW`: one of the names; anything else none.
pub fn debug_view(value: Option<&str>) -> DebugView {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("fresh") => DebugView::Fresh,
        Some("age") => DebugView::Age,
        Some("history") => DebugView::History,
        Some("clamped") => DebugView::Clamped,
        Some("distance") => DebugView::Distance,
        _ => DebugView::None,
    }
}

/// `NVF_PLAY_DEBUG_MEDIUM_FRESH`: a share from 0 to 1; unset or unreadable is [`FRESH_SHARE`].
pub fn debug_fresh(value: Option<&str>) -> f32 {
    value.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).map_or(FRESH_SHARE, |v| v.clamp(0.0, 1.0))
}

/// `NVF_PLAY_DEBUG_MEDIUM_TURN`: degrees a second, held to ±[`MAX_TURN`]; unset, unreadable or 0
/// is none.
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub fn debug_turn(value: Option<&str>) -> Option<f32> {
    value.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite() && *v != 0.0).map(|v| v.clamp(-MAX_TURN, MAX_TURN))
}

/// `NVF_PLAY_DEBUG_MEDIUM_SCALE`: a whole number held to its bounds, or 2.5 (two fifths of each render
/// axis); unset or unreadable is none (the level's own scale).
pub fn debug_scale(value: Option<&str>) -> Option<f64> {
    let text = value.map(str::trim).unwrap_or_default();
    if let Ok(whole) = text.parse::<u32>() {
        return Some(f64::from(whole.clamp(MIN_SCALE, MAX_SCALE)));
    }
    (text.parse::<f64>() == Ok(2.5)).then_some(2.5)
}

/// The scale the far buffer is made at: the debug aid's, else the level's
/// ([`CloudLevel::buffer_scale`]).
pub fn scale_in_force(level: CloudLevel) -> f64 {
    SCALE_AID.unwrap_or_else(|| level.buffer_scale())
}

/// The same bounds for direct callers of the size and description helpers.
fn scale_divisor(scale: f64) -> f64 {
    if scale.is_finite() { scale.clamp(f64::from(MIN_SCALE), f64::from(MAX_SCALE)) } else { BUFFER_SCALE }
}

/// `NVF_PLAY_DEBUG_MEDIUM_TILE`: `1`, `2` or `3`; anything else (unset, unreadable, or a side that is not
/// built) is [`TILE_SIDE`].
pub fn debug_tile(value: Option<&str>) -> u32 {
    match value.map(str::trim) {
        Some("1") => 1,
        Some("2") => 2,
        Some("3") => 3,
        _ => TILE_SIDE,
    }
}

/// A tile side as the pattern takes it, from one to three texels.
fn side_of(side: u32) -> u32 {
    side.clamp(1, 3)
}

static MODE: std::sync::LazyLock<MediumMode> = std::sync::LazyLock::new(|| debug_mode(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM").as_deref()));
static VIEW: std::sync::LazyLock<DebugView> = std::sync::LazyLock::new(|| debug_view(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_VIEW").as_deref()));
static FRESH: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| debug_fresh(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_FRESH").as_deref()));
/// The sampler and the confidence assigned to history after transport across texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HistoryFilter {
    Linear = 0,
    Cubic = 1,
    Adaptive = 2,
}

fn history_filter(value: Option<&str>) -> HistoryFilter {
    match value.map(str::trim) {
        Some(s) if s.eq_ignore_ascii_case("linear") => HistoryFilter::Linear,
        Some(s) if s.eq_ignore_ascii_case("cubic") => HistoryFilter::Cubic,
        _ => HistoryFilter::Adaptive,
    }
}

static HISTORY_FILTER: std::sync::LazyLock<HistoryFilter> = std::sync::LazyLock::new(|| history_filter(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_HISTORY_FILTER").as_deref()));
/// The far buffer's mode for this run (`NVF_PLAY_DEBUG_MEDIUM`).
pub fn medium_mode() -> MediumMode {
    *MODE
}

static FRESH_MOVING: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| debug_fresh_moving(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_FRESH_MOVING").as_deref()));
static OFFSETS: std::sync::LazyLock<RayOffsets> = std::sync::LazyLock::new(|| debug_offsets(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_OFFSETS").as_deref()));
static CLAMP: std::sync::LazyLock<HistoryClamp> = std::sync::LazyLock::new(|| debug_clamp(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_CLAMP").as_deref()));
static CLAMP_K_SET: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| debug_clamp_k(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_CLAMP_K").as_deref()));
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
static TURN: std::sync::LazyLock<Option<f32>> = std::sync::LazyLock::new(|| debug_turn(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_TURN").as_deref()));
static SCALE_AID: std::sync::LazyLock<Option<f64>> = std::sync::LazyLock::new(|| debug_scale(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_SCALE").as_deref()));
static TILE: std::sync::LazyLock<u32> = std::sync::LazyLock::new(|| debug_tile(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_TILE").as_deref()));

/// The default mode's tile side for this run (`NVF_PLAY_DEBUG_MEDIUM_TILE`).
pub fn medium_tile() -> u32 {
    *TILE
}

/// The size the published image has for a render size: the render size in the reference mode,
/// the render size divided by `scale` (held to [`MIN_SCALE`]..[`MAX_SCALE`]), rounded up, by
/// default ([`scale_in_force`]). At least one texel.
pub fn buffer_size(render: UVec2, mode: MediumMode, scale: impl Into<f64>) -> UVec2 {
    let render = render.max(UVec2::ONE);
    match mode {
        MediumMode::Reference => render,
        MediumMode::Default => {
            let divisor = scale_divisor(scale.into());
            let extent = |n: u32| (f64::from(n) / divisor).ceil() as u32;
            UVec2::new(extent(render.x), extent(render.y))
        }
    }
}

/// The trace target's size for a buffer of `size`: one texel per tile of `side` x `side`,
/// rounded up.
pub fn trace_size(size: UVec2, side: u32) -> UVec2 {
    let s = side_of(side);
    (size.max(UVec2::ONE) + UVec2::splat(s - 1)) / s
}

/// The corner (0..side-1 in x and y) of tile `tile` of a buffer of `size` texels whose ray is
/// cast at `phase`, for tiles of `side` x `side` (1, 2 or 3) [documented: Bauer 2019, slides 51-54;
/// our own form]: at side 2 the corners in the order (0,0), (1,0), (1,1), (0,1), the index the
/// phase plus one on every other tile in a checkerboard, so neighbouring tiles never cast the
/// same corner in the same frame; a tile the buffer's edge cuts cycles through the corners it
/// has; at side 1 the one texel every frame (medium_trace.wesl and medium_resolve.wesl
/// `medium_corner`, the same text). Side 3 visits three distinct rows and columns in each
/// group of three phases in a full tile; adjacent rows and columns offset the pattern.
#[cfg_attr(not(test), allow(dead_code))]
pub fn corner(tile: UVec2, size: UVec2, phase: u32, side: u32) -> UVec2 {
    const ORDER: [UVec2; 4] = [UVec2::new(0, 0), UVec2::new(1, 0), UVec2::new(1, 1), UVec2::new(0, 1)];
    let side = side_of(side);
    let w = size.x.saturating_sub(side * tile.x).clamp(1, side);
    let h = size.y.saturating_sub(side * tile.y).clamp(1, side);
    if side == 3 {
        let p = phase % (w * h);
        let col = p % w;
        return UVec2::new((col + tile.y) % w, (p / w + col + tile.x) % h);
    }
    let k = (phase.wrapping_add((tile.x + tile.y) & 1)) & 3;
    let have: Vec<UVec2> = ORDER.into_iter().filter(|c| c.x < w && c.y < h).collect();
    have[k as usize % have.len()]
}

/// The fresh share for a texel cast this frame whose history moved `motion` texels: the floor,
/// rising to `moving` (1 by default; `NVF_PLAY_DEBUG_MEDIUM_FRESH_MOVING`) between
/// [`MOTION_FROM`] and [`MOTION_FULL`]: max(floor, ramp(motion) x moving) (medium_resolve.wesl).
#[cfg_attr(not(test), allow(dead_code))]
pub fn fresh_share(floor: f32, motion: f32, moving: f32) -> f32 {
    let m = if motion.is_finite() { motion } else { MOTION_FULL };
    let moving = if moving.is_finite() { moving.clamp(0.0, 1.0) } else { 1.0 };
    floor.clamp(0.0, 1.0).max(fnv_world::sky::smoothstep(MOTION_FROM, MOTION_FULL, m) * moving)
}

/// How the history is held to this frame's rays (`NVF_PLAY_DEBUG_MEDIUM_CLAMP`).
///
/// The default is [`HistoryClamp::Moving`] [engineering choice, judged by eye in a play test,
/// 2026-11-19: specks held steady with no clamp and shimmered again with a loose one]. Stage 1 is sky only, so
/// there is no disocclusion for a clamp to catch; standing still the history is exact (the
/// reprojection lands on texel centres) and a clamp can only harm a feature smaller than a tile:
/// it is hit one frame in four, and on the other frames a band built only from this frame's rays,
/// which missed it, pulls the remembered speck toward empty sky however loose the band (nine rays
/// that all miss give a band of no width). In motion the history is resampled every frame, and
/// the clip bounds the blur and any trail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryClamp {
    /// No clamp while the view is still or slow, the variance clip fading in with the motion
    /// ramp ([`MOTION_FROM`] to [`MOTION_FULL`]): the default.
    Moving,
    /// The box of the fresh rays of the 3x3 tiles (the first default).
    Box,
    /// Their mean plus or minus k standard deviations, per channel.
    Variance,
    /// None (for diagnosis: the history as reprojected).
    Off,
}

impl HistoryClamp {
    /// The shader's code (medium_resolve.wesl `clamp_mode.x`).
    pub fn code(self) -> f32 {
        match self {
            HistoryClamp::Box => 0.0,
            HistoryClamp::Variance => 1.0,
            HistoryClamp::Off => 2.0,
            HistoryClamp::Moving => 3.0,
        }
    }
}

/// The variance clip's k unset [engineering choice: the low end of the range the published
/// variance clipping uses, so it clips at least about as tightly as the box on smooth sky].
pub const CLAMP_K: f32 = 1.25;

/// `NVF_PLAY_DEBUG_MEDIUM_CLAMP`: `box`, `variance` or `off`; anything else (or unset) the
/// default, `moving`.
pub fn debug_clamp(value: Option<&str>) -> HistoryClamp {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("box") => HistoryClamp::Box,
        Some("variance") => HistoryClamp::Variance,
        Some("off") => HistoryClamp::Off,
        _ => HistoryClamp::Moving,
    }
}

/// `NVF_PLAY_DEBUG_MEDIUM_CLAMP_K`: 0.5 to 8; unset or unreadable is [`CLAMP_K`].
pub fn debug_clamp_k(value: Option<&str>) -> f32 {
    value.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).map_or(CLAMP_K, |v| v.clamp(0.5, 8.0))
}

/// CPU mirror of medium_resolve.wesl `medium_ray_weight`: the weight of a fresh ray `sq` squared
/// texels from a texel, tiles of `side`: exp(-sq / (2 x 0.36 x side^2)) [engineering choice]; 1 at
/// side 1, where no texel is filled and the clip keeps its nine equal weights.
#[cfg_attr(not(test), allow(dead_code))]
pub fn ray_weight(sq: f32, side: u32) -> f32 {
    let side = side_of(side);
    if side == 1 {
        return 1.0;
    }
    (-sq / (2.0 * 0.36 * (side * side) as f32)).exp()
}

/// CPU mirror of medium_resolve.wesl's `filled`: the weighted mean of the nine fresh rays (light
/// premultiplied, transmittance), the fill of a texel with no valid history.
#[cfg_attr(not(test), allow(dead_code))]
pub fn weighted_mean(rays: &[Vec4; 9], weights: &[f32; 9]) -> Vec4 {
    rays.iter().zip(weights).map(|(&r, &w)| w * r).sum::<Vec4>() / weights.iter().sum::<f32>()
}

/// CPU mirror of medium_resolve.wesl `medium_clip`: a reprojected history (light rgb,
/// transmittance a) held to the nine fresh rays around it, weighed by `weights` ([`ray_weight`];
/// the box does not look at them), `motion` texels a frame of history motion (for
/// [`HistoryClamp::Moving`]).
#[cfg_attr(not(test), allow(dead_code))]
pub fn clip_history(history: Vec4, rays: &[Vec4; 9], weights: &[f32; 9], clamp: HistoryClamp, k: f32, motion: f32) -> Vec4 {
    let lo = rays.iter().fold(Vec4::splat(f32::MAX), |a, &r| a.min(r));
    let hi = rays.iter().fold(Vec4::splat(f32::MIN), |a, &r| a.max(r));
    // The mean, then the spread from the deviations (no cancellation).
    let mean = weighted_mean(rays, weights);
    let sd = (rays.iter().zip(weights).map(|(&r, &w)| w * (r - mean) * (r - mean)).sum::<Vec4>() / weights.iter().sum::<f32>()).powf(0.5);
    let variance = || {
        let k = k.clamp(0.5, 8.0);
        let c = history.clamp(mean - k * sd, mean + k * sd);
        c.truncate().max(Vec3::ZERO).extend(c.w.clamp(0.0, 1.0))
    };
    match clamp {
        HistoryClamp::Off => history,
        HistoryClamp::Box => history.clamp(lo, hi),
        HistoryClamp::Variance => variance(),
        HistoryClamp::Moving => {
            let m = if motion.is_finite() { motion } else { MOTION_FULL };
            history.lerp(variance(), fnv_world::sky::smoothstep(MOTION_FROM, MOTION_FULL, m))
        }
    }
}

/// How the default mode's rays start (`NVF_PLAY_DEBUG_MEDIUM_OFFSETS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RayOffsets {
    /// The R2 offset of the texel, turned by the golden ratio at each cast (the default).
    Turning,
    /// The R2 offset of the texel, the same at every cast.
    Fixed,
    /// Interleaved gradient noise of the texel, turned by the golden ratio at each cast.
    Blue,
    /// 0.5 for every texel at every cast (#3032 B1b, a debug aid): no offset pattern, so a
    /// pattern still seen comes from the rays' directions or the reconstruction.
    Centre,
}

impl RayOffsets {
    /// The shader's code (medium_trace.wesl `tiles.y`).
    pub fn code(self) -> f32 {
        match self {
            RayOffsets::Turning => 0.0,
            RayOffsets::Fixed => 1.0,
            RayOffsets::Blue => 2.0,
            RayOffsets::Centre => 3.0,
        }
    }
}

/// `NVF_PLAY_DEBUG_MEDIUM_OFFSETS`: `fixed`, `blue` or `centre`; anything else (or unset)
/// turning.
pub fn debug_offsets(value: Option<&str>) -> RayOffsets {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("fixed") => RayOffsets::Fixed,
        Some("blue") => RayOffsets::Blue,
        Some("centre" | "center") => RayOffsets::Centre,
        _ => RayOffsets::Turning,
    }
}

/// `NVF_PLAY_DEBUG_MEDIUM_FRESH_MOVING`: a share from 0 to 1; unset or unreadable is
/// [`FRESH_MOVING_SHARE`].
pub fn debug_fresh_moving(value: Option<&str>) -> f32 {
    value.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).map_or(FRESH_MOVING_SHARE, |v| v.clamp(0.0, 1.0))
}

/// CPU mirror of medium_trace.wesl `medium_offset`: a ray's start offset, 0..1, for a texel at
/// a phase, with tiles of `side` x `side`: the offset turns once a cycle of the tile (every 4
/// phases at side 2, every phase at side 1), at each of the texel's casts.
#[cfg_attr(not(test), allow(dead_code))]
pub fn ray_offset(texel: UVec2, phase: u32, offsets: RayOffsets, side: u32) -> f32 {
    let p = texel.as_vec2();
    let cycle = side_of(side) * side_of(side);
    let turn = 0.618_034 * ((phase / cycle) % 64) as f32;
    match offsets {
        RayOffsets::Blue => ((52.982_918 * (p.dot(Vec2::new(0.067_110_56, 0.005_837_15))).fract()).fract() + turn).fract(),
        RayOffsets::Fixed => (0.5 + p.x * 0.754_877_7 + p.y * 0.569_840_3).fract(),
        RayOffsets::Turning => ((0.5 + p.x * 0.754_877_7 + p.y * 0.569_840_3).fract() + turn).fract(),
        RayOffsets::Centre => 0.5,
    }
}

/// What of the sky the reconstruction's history depends on, as a source last saw it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkyMark {
    pub weather: u32,
    /// The weather fading out, if a fade runs.
    pub fading_from: Option<u32>,
    pub indoors: bool,
    /// The simulation's continuous game time in hours (its days passed, a float, times 24),
    /// where there is one; and the sky's hour.
    pub game_hours: Option<f32>,
    pub hour: f32,
    /// The sky dome (another worldspace makes another).
    pub dome: Option<AssetId<Mesh>>,
}

/// The game hours the clock may advance in a frame of `dt` real seconds at `hours_per_second`
/// (the TimeScale over 3600, or the sky's own test scale) before the step is a jump: the
/// prediction times [`CLOCK_SLACK`] plus two of the sky clock's steps.
pub fn clock_allowance(hours_per_second: f32, dt: f32) -> f32 {
    let predicted = if hours_per_second.is_finite() && dt.is_finite() { (hours_per_second * dt).max(0.0) } else { 0.0 };
    predicted * CLOCK_SLACK + 2.0 * SKY_CLOCK_STEP
}

/// Whether the sky jumped between two frames, so the history no longer holds:
/// a weather changed with no fade (a script's ForceWeather, a mod's), indoors
/// and out, another dome (another worldspace), or the game clock stepped back at all, or ahead
/// by more than `allowance` hours ([`clock_allowance`]: wait, sleep, `set gamehour`, a jump of
/// whole days; a high time scale alone does not). A fade that starts, runs, turns round or ends
/// is not a cut, nor is the clock's own advance, midnight or a walk over a cell's edge.
pub fn sky_cut(before: &SkyMark, now: &SkyMark, allowance: f32) -> bool {
    let snapped = before.weather != now.weather && now.fading_from.is_none() && before.fading_from != Some(now.weather);
    let bad = |step: f32| !step.is_finite() || step < -1e-4 || step > allowance.max(0.0);
    // The hour, wrapped into -12..12 (midnight is a small step): a console `set gamehour` moves
    // it without the days. The continuous game time where there is one: a jump of whole days
    // to the same hour, with no day boundary to invent.
    let d = (now.hour - before.hour).rem_euclid(24.0);
    let hour_step = if d >= 12.0 { d - 24.0 } else { d };
    let jumped = bad(hour_step)
        || match (before.game_hours, now.game_hours) {
            (Some(a), Some(b)) => bad(b - a),
            _ => false,
        };
    snapped || before.indoors != now.indoors || before.dome != now.dome || jumped
}

/// medium_upsample.wesl, imported by the sky proxy's material as `nvforever::medium_upsample`.
const UPSAMPLE_SHADER: Handle<Shader> = bevy::asset::uuid_handle!("8d2f6b17-3c49-4e0a-b5d8-71a64c9e2f05");

// ---------------------------------------------------------------------------------------
// What a source asks for, and what the medium publishes (main world)
// ---------------------------------------------------------------------------------------

/// What the sources ask of the medium this frame, written by them in `Update` (today the cloud
/// source alone, volumetric_clouds.rs `follow_volume`).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MediumRequest {
    /// A source is switched on: the image is kept while it is (also indoors).
    pub wanted: bool,
    /// What to trace this frame; `None`: nothing to draw (indoors, no cover, a clear sky).
    pub trace: Option<SourceFrame>,
    /// Counts the sky's jumps ([`sky_cut`]) the source has seen: a change drops the history.
    pub sky_cuts: u32,
    /// The level the row asks for: the far buffer's scale ([`scale_in_force`]). A change is a change of
    /// the size the render world wants, so the image and the history are made again once ([`plan`]).
    pub level: CloudLevel,
    /// With no trace this frame: one is to come (outdoors, above water, with a cover that is
    /// composed or being composed). A loading screen waits for it ([`clouds_wait`]).
    pub pending: bool,
}

/// One frame of the cloud source's inputs to the trace.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceFrame {
    /// The march's uniform (volumetric_clouds.rs [`crate::volumetric_clouds::uniform`]).
    pub clouds: CloudVolumeUniform,
    /// The material that composes the buffer: the handover waits until the render world has
    /// it prepared (a material whose image was swapped is prepared again).
    pub composer: Option<UntypedAssetId>,
    /// The entity drawn with it (the sky proxy): the handover follows that entity's own
    /// specialised main-pass pipeline in the main view.
    pub composer_entity: Option<Entity>,
    /// The distance the distance debug view shows as white: the source's own range (world
    /// units).
    pub distance_scale: f32,
    /// The cloud source's generated noise textures (the base, the erosion: cloud_noise.rs),
    /// `None` while they are generated; the trace waits for both on the GPU (#3032 stage 2b).
    pub noise: Option<[AssetId<Image>; 2]>,
    /// The weather (form ID) once no fade runs: what the capped line's look is made anew for.
    pub weather: Option<u32>,
}

/// The medium's published image and whether it may be drawn this frame ([`follow_medium`],
/// `PostUpdate`).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MediumOutput {
    /// The far buffer, bound by the composing material (stable until the size changes).
    pub image: Option<Handle<Image>>,
    /// Its size in texels.
    pub size: Option<UVec2>,
    /// The epoch the render world must echo ([`plan`]).
    pub epoch: u32,
    /// The buffer of this epoch was written whole at the right size: the volume may be shown
    /// and the flat layers hidden.
    pub ready: bool,
}

/// The render world's side of the gate: its systems run only while it is open. Opened when a
/// source asks, closed [`IDLE_FRAMES`] frames after nothing does.
#[derive(Resource, Clone, Default)]
pub struct MediumGate(Arc<AtomicBool>);

impl MediumGate {
    fn is_open(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn set_open(&self, open: bool) {
        self.0.store(open, Ordering::Release);
    }
}

/// The size the render world wants the buffer to have, for the epoch it traced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    pub epoch: u32,
    pub size: UVec2,
}

/// Why a render frame's write is, or is not, one the volume may be shown from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WriteCause {
    /// Both passes completed with ready pipelines, at the wanted size, the proxy ready.
    Whole,
    /// No view of the main camera this frame.
    #[default]
    NoView,
    /// No published image on the GPU (yet).
    NoImage,
    /// The pass did not run this frame.
    PassNotRun,
    /// The image is not the size the render world wants (nothing written).
    Extent,
    /// The medium's own pipelines or the uniforms not ready (nothing written).
    PassesNotReady,
    /// Written whole, but the proxy's material or its own draw pipeline is not ready.
    ComposerNotReady,
    /// The source's noise textures are not generated or not on the GPU yet (nothing written).
    NoiseNotReady,
}

impl WriteCause {
    fn text(self) -> &'static str {
        match self {
            WriteCause::Whole => "written whole",
            WriteCause::NoView => "no view of the main camera",
            WriteCause::NoImage => "no published image on the GPU",
            WriteCause::PassNotRun => "the trace pass did not run",
            WriteCause::Extent => "the image is not the size the render world wants",
            WriteCause::PassesNotReady => "the medium's pipelines or the uniforms not ready",
            WriteCause::ComposerNotReady => "the proxy's material or its own pipeline not ready",
            WriteCause::NoiseNotReady => "the clouds' noise textures not generated or not uploaded yet",
        }
    }
}

/// A render frame's one verdict, published once at the end of that render frame (never a
/// provisional one the main world could read meanwhile): the
/// epoch it traced for, the render frame and the view (the main-world camera)
/// it was made for, the size of the image, and whether the write is one the volume may be shown
/// from ([`WriteCause::Whole`]): both passes completed with ready pipelines, the image at the
/// wanted size (checked before anything was written), the composing material prepared and the
/// proxy's own draw pipeline ready.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub epoch: u32,
    pub frame: u32,
    pub view: Option<Entity>,
    pub size: UVec2,
    pub ok: bool,
    pub cause: WriteCause,
}

/// How many frames old a report may be and still count as this frame's: the main world reads
/// the render world's verdicts one or two frames late (pipelined rendering) [design].
pub const REPORT_FRAMES: u32 = 2;
/// A whole report older than [`REPORT_FRAMES`] but at most this old keeps the volume as it is
/// (a late verdict is not a verdict "not written": swapping the flat layers in for a frame is a
/// flash of its own); older still, no pass has reported for that long and the flat layers come
/// back [design].
pub const REPORT_STALE: u32 = 8;

/// A report's age in frames at `now` (0 when it is stamped up to a frame ahead, the counters'
/// order between the worlds).
pub fn report_age(w: &Written, now: u32) -> u32 {
    if w.frame.wrapping_sub(now) <= 1 { 0 } else { now.wrapping_sub(w.frame) }
}

/// Whether a report is this frame's (within [`REPORT_FRAMES`]) and for the view `view`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn report_current(w: &Written, now: u32, view: Option<Entity>) -> bool {
    report_age(w, now) <= REPORT_FRAMES && view.is_some() && w.view == view
}

/// Why the volume is not drawn this frame ([`plan`]), for the hand-back line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandBack {
    /// The source asks for no trace (indoors, under water, no or a withheld cover, a weather
    /// the medium cannot use: the source's own line says which).
    NoTrace,
    /// The view's key changed (size, multisampling, HDR, upscaler, jitter).
    ViewChanged,
    /// A new image is being made.
    NewImage,
    /// No report yet for this epoch and view.
    NoReport,
    /// The last report is this many frames old.
    TooOld(u32),
    /// The render world's verdict for the frame.
    NotWritten(WriteCause),
}

impl HandBack {
    pub fn text(self) -> String {
        match self {
            HandBack::NoTrace => "the source asks for no trace (its own line says why)".into(),
            HandBack::ViewChanged => "the view changed (size, anti-aliasing, HDR, upscaler, jitter, shadow filter, prepasses, ambient occlusion or sun visibility)".into(),
            HandBack::NewImage => "a new far buffer is being made".into(),
            HandBack::NoReport => "no report for this view and epoch".into(),
            HandBack::TooOld(age) => format!("the last report is {age} frames old"),
            HandBack::NotWritten(cause) => format!("the render world reports: {}", cause.text()),
        }
    }
}

/// What a loading screen must know of the history behind the buffer ([`clouds_wait`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryRun {
    pub epoch: u32,
    /// Whole reconstructions since the history's last cut, the cut's own frame counted
    /// ([`casts_after`]).
    pub casts: u32,
    /// The sky's jumps the history had seen ([`MediumRequest::sky_cuts`]): another count in the
    /// main world is a cut still to come.
    pub sky_cuts: u32,
    /// A pipeline the buffer needs has failed for good and nothing is drawn ([`buffer_failed`]).
    pub failed: bool,
}

#[derive(Default)]
struct LinkState {
    wanted: Option<Wanted>,
    written: Option<Written>,
    run: Option<HistoryRun>,
}

/// The render world's answers to the main world (one `Arc` in each world, as
/// `upscaler::UpscaleDrawn`): what the main world reads may be a frame or two old, which the
/// epoch makes safe.
#[derive(Resource, Clone, Default)]
pub struct MediumLink(Arc<Mutex<LinkState>>);

impl MediumLink {
    fn with<R>(&self, f: impl FnOnce(&mut LinkState) -> R) -> R {
        let mut guard = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut guard)
    }

    pub fn set_wanted(&self, wanted: Wanted) {
        self.with(|s| s.wanted = Some(wanted));
    }

    pub fn set_written(&self, written: Written) {
        self.with(|s| s.written = Some(written));
    }

    pub fn set_run(&self, run: HistoryRun) {
        self.with(|s| s.run = Some(run));
    }

    fn run(&self) -> Option<HistoryRun> {
        self.with(|s| s.run)
    }

    fn read(&self) -> (Option<Wanted>, Option<Written>) {
        self.with(|s| (s.wanted, s.written))
    }

    fn clear(&self) {
        self.with(|s| *s = LinkState::default());
    }
}

// ---------------------------------------------------------------------------------------
// The handover (pure)
// ---------------------------------------------------------------------------------------

/// What decides the main pass's render size and jitter, and every input of the main view's mesh
/// pipeline key that can change in play, as the main world sees it this frame (after
/// `CameraUpdateSystems` and the systems that change them). The rule: anything
/// that enters the main view's mesh pipeline key (vendored bevy_pbr render/mesh.rs
/// `check_views_need_specialization`) and can change in play must be here; otherwise bevy_pbr
/// specialises the proxy again and skips it while the last whole verdict still stands, and the
/// hand-back arrives up to two frames late. Inputs constant for the run (deferred prepass,
/// environment maps, irradiance volumes, Bevy's own SSR, SSAO, OIT, atmosphere and contact
/// shadows, none of which the game uses; the projection, culling and fog presence of the main
/// camera; the view transmission texture, which nvf-assets nif_material.rs requires on every
/// `Camera3d` at its default quality and nothing changes in play; the tonemapping bits, which only
/// a non-HDR view takes and the row forces HDR) are not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewKey {
    pub viewport: Option<UVec2>,
    pub samples: u32,
    pub hdr: bool,
    pub fsr: Option<crate::upscaler::UpscaleQuality>,
    pub dlss: Option<crate::upscaler::UpscaleQuality>,
    pub jitter: bool,
    /// What else of the main view's mesh pipeline key changes in play: the shadow filter, the
    /// depth, normal and motion prepasses (graphics.rs `apply`), the ambient occlusion in the
    /// lighting as its view bindings are given (asked for in the lighting and the hook in place,
    /// ssao.rs `in_lighting`), and the sun visibility's bits (its plumbing active, the cloud and
    /// the terrain variants: the Cloud shadows and Terrain shadows rows, sun_visibility.rs).
    pub shadow_filter: Option<bevy::light::ShadowFilteringMethod>,
    pub prepasses: [bool; 3],
    pub occlusion: bool,
    pub sun: [bool; 3],
}

/// What [`plan`] keeps between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    /// Moves on every change that could make a written buffer wrong for this frame.
    pub epoch: u32,
    /// The published image's size, if one is kept.
    pub image: Option<UVec2>,
    /// A trace was asked for last frame.
    pub tracing: bool,
    pub key: Option<ViewKey>,
    /// The buffer was drawn last frame (a late whole report keeps it so).
    pub ready: bool,
}

/// What [`plan`] reads this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    /// A source is switched on.
    pub on: bool,
    /// A source asks for a trace this frame.
    pub tracing: bool,
    /// The main camera's view; `None`: no main camera.
    pub key: Option<ViewKey>,
    pub wanted: Option<Wanted>,
    pub written: Option<Written>,
    /// This frame's number (the main world's `FrameCount`) and the main camera's entity: a
    /// report counts only if it is recent and for that view ([`report_current`]).
    pub frame: u32,
    pub view: Option<Entity>,
}

/// What [`follow_medium`] does this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Make a new image of this size (a new handle).
    pub alloc: Option<UVec2>,
    /// Drop the image (the sources are off).
    pub release: bool,
    /// The buffer may be drawn this frame.
    pub ready: bool,
    /// Why not, when it is not.
    pub why: Option<HandBack>,
}

/// The handover's rule, one frame: see the module's documentation. The epoch moves when the
/// trace starts or stops, when the view's key changes and when a new image is made; a new
/// image is made when the render world wants another size for this very epoch; the buffer may
/// be drawn only while the render world's last report is for this epoch, whole and of the
/// kept image's size.
pub fn plan(prev: Ledger, now: Inputs) -> (Ledger, Plan) {
    let mut l = prev;
    let mut p = Plan::default();
    l.ready = false;
    if !now.on {
        p.release = l.image.is_some();
        if l.tracing || p.release || l.key.is_some() {
            l.epoch = l.epoch.wrapping_add(1);
        }
        l.image = None;
        l.tracing = false;
        l.key = None;
        p.why = Some(HandBack::NoTrace);
        return (l, p);
    }
    let tracing = now.tracing && now.key.is_some();
    let key_changed = now.key != l.key;
    if tracing != l.tracing || key_changed {
        l.epoch = l.epoch.wrapping_add(1);
    }
    l.tracing = tracing;
    l.key = now.key;
    if !tracing {
        p.why = Some(HandBack::NoTrace);
        return (l, p);
    }
    if key_changed {
        p.why = Some(HandBack::ViewChanged);
    }
    if let Some(w) = now.wanted.filter(|w| w.epoch == l.epoch && w.size.x > 0 && w.size.y > 0) {
        if l.image != Some(w.size) {
            p.alloc = Some(w.size);
            l.image = Some(w.size);
            l.epoch = l.epoch.wrapping_add(1);
            p.why = Some(HandBack::NewImage);
            return (l, p);
        }
    }
    // A verdict "not written" of this epoch counts with its own cause even when it names no
    // view or image yet (NoView, NoImage); a whole one only for this view and image.
    let ours = now.written.filter(|w| w.epoch == l.epoch && now.view.is_some() && (w.view == now.view || (!w.ok && w.view.is_none())));
    let (ready, why) = match ours {
        _ if l.image.is_none() => (false, HandBack::NewImage),
        None => (false, HandBack::NoReport),
        Some(w) if !w.ok => (false, HandBack::NotWritten(w.cause)),
        Some(w) if Some(w.size) != l.image => (false, HandBack::NoReport),
        Some(w) => match report_age(&w, now.frame) {
            age if age <= REPORT_FRAMES => (true, HandBack::NoReport),
            // Late but whole: kept as it was, neither taken over nor handed back.
            age if age <= REPORT_STALE => (prev.ready, HandBack::TooOld(age)),
            age => (false, HandBack::TooOld(age)),
        },
    };
    p.ready = ready;
    if !ready && p.why.is_none() {
        p.why = Some(why);
    }
    l.ready = ready;
    (l, p)
}

// ---------------------------------------------------------------------------------------
// What a loading screen waits for (pure, and the system that says it)
// ---------------------------------------------------------------------------------------

/// The casts of the history after a whole reconstruction: one at a cut (its own frame), else one
/// more.
pub fn casts_after(casts: u32, reset: bool) -> u32 {
    if reset { 1 } else { casts.saturating_add(1) }
}

/// How many whole reconstructions after a cut give every texel a ray of its own: the tile's texels,
/// one a frame ([`corner`]); none in the reference mode, whose every frame is whole. At side 3 eight
/// texels of nine have then had their first own cast over a fill (half of it each,
/// [`counted_share`]): the picture is whole but still settling.
pub fn casts_needed(mode: MediumMode, side: u32) -> u32 {
    match mode {
        MediumMode::Reference => 0,
        MediumMode::Default => side_of(side) * side_of(side),
    }
}

/// What [`clouds_wait`] reads of the medium this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloudsNow {
    /// The row is on.
    pub wanted: bool,
    /// A trace is asked for this frame, or one is to come ([`MediumRequest::pending`]).
    pub tracing: bool,
    pub pending: bool,
    /// The buffer is drawn this frame ([`MediumOutput::ready`]).
    pub drawn: bool,
    /// [`HistoryRun`]'s, for the epoch and the sky's jumps in force (none yet: 0 and false).
    pub casts: u32,
    pub failed: bool,
}

/// Whether the buffer cannot come: the proxy's own pipeline has failed for good, or one of the
/// passes' has and nothing stands in for it (`drew`: the passes drew this frame, as the light volume's
/// trace for a failed one without it).
pub fn buffer_failed(composer: bool, passes: bool, drew: bool) -> bool {
    composer || (passes && !drew)
}

/// What a loading screen has to wait for of the clouds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudsWait {
    /// Nothing: off, indoors, under water, nothing to draw, or a pipeline that failed for good with
    /// nothing drawn in its stead.
    Nothing,
    /// Not drawn yet, or drawn from too few casts since the last cut (the texels without a ray of
    /// their own filled from the ones around).
    Pending,
    /// Drawn, every texel with a ray of its own since the last cut (whole, still settling:
    /// [`casts_needed`]).
    Ready,
}

/// Whether a loading screen should wait for the clouds [design: a split
/// second of blocky clouds as the screen came down, judged by eye in a play test, 2026-11-23]. The screen's own limit ends the wait
/// (nvf-play loading.rs `clouds_hold`), so a thing that stays pending never hangs it.
pub fn clouds_wait(now: &CloudsNow, needed: u32) -> CloudsWait {
    if !now.wanted || !(now.tracing || now.pending) {
        CloudsWait::Nothing
    } else if now.tracing && now.drawn && now.casts >= needed {
        CloudsWait::Ready
    } else if now.failed && !now.drawn {
        CloudsWait::Nothing
    } else {
        CloudsWait::Pending
    }
}

/// Frames in a row the clouds must be [`CloudsWait::Ready`] before a loading screen may leave: the
/// render world's counts are a frame or two old ([`REPORT_FRAMES`]), so a cut in the last frames must
/// not let the old count pass [design]. Known limit: a camera cut in the very frame the screen leaves
/// can still be uncovered for a frame, shown with the smooth fill (a sky jump cannot).
pub const READY_FRAMES: u32 = 3;

/// What the loading screen is told this frame (busy, ready) from the wait and the frames in a row it
/// has been ready (`streak`, started again whenever it is not).
pub fn clouds_gate(streak: &mut u32, wait: CloudsWait) -> (bool, bool) {
    *streak = if wait == CloudsWait::Ready { streak.saturating_add(1) } else { 0 };
    let ready = *streak >= READY_FRAMES;
    (wait != CloudsWait::Nothing && !ready, ready)
}

/// The play state as the medium's cut test sees it: a loading screen as play. The world is drawn
/// under the screen with the camera where play goes on from, so its leaving is no cut (the
/// upscalers keep it as one; here it dropped the history in the first frame of play). Its coming up
/// still is (`UpscaleCamera::loading_generation`), as is the travel's jump behind it.
fn view_state(state: nvf_play::PlayState) -> nvf_play::PlayState {
    if state == nvf_play::PlayState::Loading { nvf_play::PlayState::Playing } else { state }
}

/// Tell the loading screen that is up whether the clouds are still to come ([`clouds_wait`],
/// [`clouds_gate`]). With no screen up, or no source on, nothing is read.
fn hold_loading_screen(loading: Option<ResMut<nvf_play::loading::Loading>>, request: Res<MediumRequest>, output: Res<MediumOutput>, link: Res<MediumLink>, mut streak: Local<u32>) {
    let Some(mut loading) = loading else { return };
    let wait = if loading.active() && request.wanted {
        let run = link.run().filter(|r| r.epoch == output.epoch).unwrap_or_default();
        // A jump of the sky the history has not seen yet: the next frame's cut.
        let casts = if run.sky_cuts == request.sky_cuts { run.casts } else { 0 };
        let now = CloudsNow { wanted: request.wanted, tracing: request.trace.is_some(), pending: request.pending, drawn: output.ready, casts, failed: run.failed };
        clouds_wait(&now, casts_needed(*MODE, *TILE))
    } else {
        CloudsWait::Nothing
    };
    let told = clouds_gate(&mut streak, wait);
    if (loading.clouds_busy, loading.clouds_ready) != told {
        (loading.clouds_busy, loading.clouds_ready) = told;
    }
}

/// The published image of `size`: written as a render attachment by the trace pass, read with
/// a linear, clamped sampler by the composing material. No data: it is never shown before the
/// trace pass has written it ([`plan`]).
pub fn far_image(size: UVec2) -> Image {
    let mut image = Image::new_uninit(
        Extent3d { width: size.x.max(1), height: size.y.max(1), depth_or_array_layers: 1 },
        TextureDimension::D2,
        FAR_FORMAT,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.label = Some("medium_far_light");
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::linear();
    image
}

/// The buffer's memory in MiB at `size`: the light and the distance targets.
pub fn far_mib(size: UVec2) -> f32 {
    let texels = size.x as f32 * size.y as f32;
    texels * (8.0 + 4.0) / (1024.0 * 1024.0)
}

/// The memory in MiB of all the medium's targets for a buffer of `size` in `mode` with tiles of
/// `side`: by default the published image, two histories (light 8 B, distance and age 8 B a
/// texel) and the trace targets (light 8 B, distance 4 B) at the trace size; in the reference
/// mode the image and its distance target.
pub fn medium_mib(size: UVec2, mode: MediumMode, side: u32) -> f32 {
    let mib = |s: UVec2, bytes: f32| s.x as f32 * s.y as f32 * bytes / (1024.0 * 1024.0);
    match mode {
        MediumMode::Reference => far_mib(size),
        MediumMode::Default => mib(size, 8.0 + 2.0 * (8.0 + 8.0)) + far_mib(trace_size(size, side)),
    }
}

/// The buffer's share of the render size in words, for the log.
fn scale_words(scale: f64) -> &'static str {
    match scale_divisor(scale) {
        1.0 => "the main pass's render size",
        2.0 => "half the main pass's render size",
        2.5 => "two fifths of the main pass's render size",
        3.0 => "a third of the main pass's render size",
        4.0 => "a quarter of the main pass's render size",
        _ => "a fraction of the main pass's render size",
    }
}

/// The rays a frame in words, for the log.
fn tile_words(side: u32) -> &'static str {
    match side_of(side) {
        1 => "one ray per texel",
        3 => "one ray in nine",
        _ => "one ray in four",
    }
}

/// The allocation line's text for a buffer of `size` made at `scale` with tiles of `side`
/// (smoke `volumetric-clouds*`).
pub fn describe_buffer(size: UVec2, mode: MediumMode, scale: impl Into<f64>, side: u32) -> String {
    match mode {
        MediumMode::Reference => format!(
            "{}x{} (the main pass's render size), every pixel traced each frame (reference), light and transmittance Rgba16Float, distance R32Float, {:.1} MiB",
            size.x,
            size.y,
            medium_mib(size, mode, side)
        ),
        MediumMode::Default => {
            let t = trace_size(size, side);
            format!(
                "{}x{} ({}), {} a frame into {}x{}, reconstructed with history, light and transmittance Rgba16Float, distance and age Rg32Float, {:.1} MiB",
                size.x,
                size.y,
                scale_words(scale.into()),
                tile_words(side),
                t.x,
                t.y,
                medium_mib(size, mode, side)
            )
        }
    }
}

// ---------------------------------------------------------------------------------------
// The trace's uniform (pure)
// ---------------------------------------------------------------------------------------

/// medium_trace.wesl `MediumTrace`: the same fields in the same order.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct MediumTraceUniform {
    /// The camera's rotation times the inverse of its unjittered projection.
    pub world_from_clip: Mat4,
    /// xyz: the camera's position, w: the cast ray's spread ([`ray_spread`]).
    pub origin: Vec4,
    /// x, y: the buffer's size; z: the frame number modulo 64; w: [`RAY_END`].
    pub grid: Vec4,
    /// The default mode's cast pattern: x the phase (the cycle of [`corner`], advanced on each
    /// whole reconstruction), y the start offsets ([`RayOffsets::code`]), z the tile side (1, 2 or
    /// 3, [`TILE_SIDE`]); w is the height of the water plane above a mirror eye, zero for the main view.
    pub tiles: Vec4,
}

/// The trace's uniform for a view (`world_from_view` its camera's transform, `clip_from_view`
/// its projection **without** the upscalers' jitter, as `ExtractedView` holds it) and a buffer
/// of `size`. Every value finite.
pub fn trace_uniform(world_from_view: Mat4, clip_from_view: Mat4, size: UVec2, frame: u32) -> MediumTraceUniform {
    let mut rotation = world_from_view;
    rotation.w_axis = Vec4::W;
    let world_from_clip = rotation * clip_from_view.inverse();
    let origin = world_from_view.w_axis.truncate();
    MediumTraceUniform {
        world_from_clip: if world_from_clip.is_finite() { world_from_clip } else { Mat4::IDENTITY },
        origin: if origin.is_finite() { origin.extend(0.0) } else { Vec4::ZERO },
        grid: Vec4::new(size.x.max(1) as f32, size.y.max(1) as f32, (frame % 64) as f32, RAY_END),
        tiles: Vec4::ZERO,
    }
}

/// A cast ray's spread (#3032 stage 2a, the seam's one added value, `MediumRay::spread`): the
/// width per unit of distance of what one cast ray answers for each frame, the cast texel's
/// angular width times the tile side (`side` x the buffer's texel at its size; 1 in the reference
/// mode, whose buffer is the render size), at the screen's centre, from the view's real
/// projection (`clip_from_view`, unjittered) and the buffer's extent `size`. On a non-square
/// texel (the buffer's sizes are rounded up per axis) the wider of the two. A source filters what
/// is finer than `spread x distance` (the cloud source: the erosion's fade and the cover's
/// level). Always finite and positive (a projection that gives none: one texel of a 90-degree
/// view over the extent).
pub fn ray_spread(clip_from_view: Mat4, size: UVec2, side: u32) -> f32 {
    let size = size.max(UVec2::ONE).as_vec2();
    let across = |scale: f32, n: f32| if scale.is_finite() && scale.abs() > 1e-6 { 2.0 / (scale.abs() * n) } else { 2.0 / n };
    let w = across(clip_from_view.x_axis.x, size.x).max(across(clip_from_view.y_axis.y, size.y)) * side_of(side) as f32;
    if w.is_finite() && w > 0.0 { w } else { 2.0 / size.y }
}

/// The trace's uniform as [`prepare_medium`] writes it: [`trace_uniform`] with the cast ray's
/// spread in `origin.w` ([`ray_spread`]: the buffer's texel at its size times the tile side `side`;
/// the reference casts every texel of the render size, side 1). The default mode's cast pattern
/// (`tiles`) is filled in by the caller with the history's phase.
pub fn prepared_trace_uniform(world_from_view: Mat4, clip_from_view: Mat4, size: UVec2, frame: u32, mode: MediumMode, side: u32) -> MediumTraceUniform {
    let mut uniform = trace_uniform(world_from_view, clip_from_view, size, frame);
    uniform.origin.w = ray_spread(clip_from_view, size, if mode == MediumMode::Reference { 1 } else { side });
    uniform
}

/// medium_resolve.wesl `MediumResolve`: the same fields in the same order.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct MediumResolveUniform {
    /// As [`MediumTraceUniform::world_from_clip`], this frame.
    pub world_from_clip: Mat4,
    /// Last frame's unjittered projection times the inverse of last frame's camera rotation: a
    /// point relative to last frame's camera into last frame's clip space.
    pub prev_clip_from_rel: Mat4,
    /// Last frame's [`MediumTraceUniform::world_from_clip`]: the history's own rays, to carry its
    /// distance into this frame's camera.
    pub prev_world_from_clip: Mat4,
    /// xyz: this frame's camera position minus last frame's; w: 1 while the history holds (no
    /// cut), else 0.
    pub delta: Vec4,
    /// x, y: the buffer's size; z: the phase; w: the fresh share's floor.
    pub grid: Vec4,
    /// x: the debug view's code ([`DebugView::code`]); y, z: [`MOTION_FROM`], [`MOTION_FULL`];
    /// w: the moving share (`NVF_PLAY_DEBUG_MEDIUM_FRESH_MOVING`).
    pub params: Vec4,
    /// x: the clamp's code ([`HistoryClamp::code`]); y: the variance clip's k; z: the history filter; w unused.
    pub clamp_mode: Vec4,
    /// x: the tile side (1, 2 or 3, as [`MediumTraceUniform::tiles`] z); y: the distance view's
    /// white (the source's range, [`SourceFrame::distance_scale`]); z, w unused.
    pub pattern: Vec4,
}

/// The camera-relative clip transform a reconstruction keeps for the next frame (`world_from_view`
/// the camera's transform, `clip_from_view` its unjittered projection). Finite, else `None`.
pub fn clip_from_rel(world_from_view: Mat4, clip_from_view: Mat4) -> Option<Mat4> {
    let mut rotation = world_from_view;
    rotation.w_axis = Vec4::W;
    let m = clip_from_view * rotation.inverse();
    m.is_finite().then_some(m)
}

// ---------------------------------------------------------------------------------------
// Plugin and main-world system
// ---------------------------------------------------------------------------------------

/// Whether [`follow_medium`] has anything to do: a source is on, or the image or the gate is
/// left to take down.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediumActive(pub bool);

fn medium_in_use(request: Res<MediumRequest>, active: Res<MediumActive>) -> bool {
    request.wanted || active.0
}

pub struct MediumPlugin;

impl Plugin for MediumPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "medium_trace.wesl");
        bevy::asset::embedded_asset!(app, "medium_resolve.wesl");
        bevy::asset::embedded_asset!(app, "volumetric_clouds_lightvol.wesl");
        bevy::asset::load_internal_asset!(app, UPSAMPLE_SHADER, "medium_upsample.wesl", |s, _| Shader::from_wesl(s, "embedded://nvforever/medium_upsample.wesl"));
        let link = MediumLink::default();
        let gate = MediumGate::default();
        app.insert_resource(link.clone())
            .insert_resource(gate.clone())
            .init_resource::<MediumRequest>()
            .init_resource::<MediumOutput>()
            .init_resource::<MediumActive>()
            // This frame's camera size (after CameraUpdateSystems), the image added before the
            // asset events are sent (so it is extracted this frame).
            .add_systems(
                PostUpdate,
                follow_medium
                    .run_if(medium_in_use)
                    .after(bevy::camera::CameraUpdateSystems)
                    .after(crate::ssao::follow_projection)
                    .before(bevy::asset::AssetEventSystems),
            )
            .add_systems(PostUpdate, hold_loading_screen.after(follow_medium));
        #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
        app.add_systems(Update, turn_aid.run_if(|| TURN.is_some()));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        reflection::install(render_app);
        let (a, b, c, d) = (gate.clone(), gate.clone(), gate.clone(), gate.clone());
        render_app
            .insert_resource(link)
            .insert_resource(gate)
            .init_resource::<MediumFrame>()
            .init_resource::<MediumGpu>()
            .init_resource::<MediumPipelines>()
            .init_resource::<MediumHistory>()
            .init_resource::<crate::volumetric_clouds_lightvol::gpu::LvGpu>()
            .add_systems(ExtractSchedule, extract_medium.run_if(move || a.is_open()))
            .add_systems(Render, prepare_medium.in_set(RenderSystems::PrepareResources).run_if(move || b.is_open()))
            .add_systems(Render, publish_verdict.in_set(RenderSystems::Cleanup).run_if(move || d.is_open()))
            // After the depth prepass, before the main pass reads the buffer (as ssao.rs
            // `ao_lighting_pass`).
            .add_systems(Core3d, medium_trace_pass.after(Core3dSystems::Prepass).before(Core3dSystems::MainPass).run_if(move || c.is_open()));
    }
}

/// What [`follow_medium`] keeps between frames.
#[derive(Default)]
pub(crate) struct MediumFollow {
    ledger: Ledger,
    /// Frames since nothing was asked for, until the gate closes.
    draining: Option<u32>,
    /// The image size whose first write was logged.
    told_ready: Option<UVec2>,
    /// The debug aids were logged.
    aids_told: bool,
    /// Hand-backs from a drawn volume so far; those not logged yet, with the last one's reason,
    /// and a take-over not logged yet; when the last line was logged.
    handbacks: u32,
    unsaid: u32,
    unsaid_why: Option<HandBack>,
    unsaid_takeover: bool,
    told_handback: Option<std::time::Instant>,
}

/// Each frame a source is on (and a few after): the handover ([`plan`]) on this frame's camera
/// and the render world's last report, the image made, replaced or dropped, the gate kept.
/// Log lines when the buffer is made or resized, when its first write is drawn and when it
/// is released.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn follow_medium(
    request: Res<MediumRequest>,
    link: Res<MediumLink>,
    gate: Res<MediumGate>,
    mut output: ResMut<MediumOutput>,
    mut active: ResMut<MediumActive>,
    mut images: ResMut<Assets<Image>>,
    frame: Res<FrameCount>,
    cams: Query<
        (
            (Entity, Option<&Camera>, Option<&Msaa>, Has<bevy::camera::Hdr>, Option<&crate::fsr::FsrUpscaling>, Option<&crate::upscaler::DlssUpscaling>, Has<TemporalJitter>),
            (
                Option<&bevy::light::ShadowFilteringMethod>,
                Has<bevy::core_pipeline::prepass::DepthPrepass>,
                Has<bevy::core_pipeline::prepass::NormalPrepass>,
                Has<bevy::core_pipeline::prepass::MotionVectorPrepass>,
                Option<&crate::ssao::AmbientOcclusion>,
            ),
        ),
        With<FlyCam>,
    >,
    sun: Option<Res<crate::sun_visibility::SunVisibility>>,
    mut state: Local<MediumFollow>,
) {
    let view = cams.single().ok().map(|c| c.0 .0);
    let key = cams.single().ok().map(|((_, cam, msaa, hdr, fsr, dlss, jitter), (filter, depth, normal, motion, occlusion))| ViewKey {
        viewport: cam.and_then(Camera::physical_viewport_size),
        samples: msaa.map_or(1, |m| m.samples()),
        hdr,
        fsr: fsr.map(|f| f.quality),
        dlss: dlss.map(|d| d.quality),
        jitter,
        shadow_filter: filter.copied(),
        prepasses: [depth, normal, motion],
        occlusion: occlusion.is_some_and(|ao| crate::ssao::in_lighting(ao, crate::ssao_lighting::hook_state())),
        sun: sun.as_ref().map_or([false; 3], |v| [v.active(), v.cloud_on(), v.terrain_on()]),
    });
    if request.wanted && !state.aids_told {
        state.aids_told = true;
        if *MODE == MediumMode::Reference {
            info!("debug switch NVF_PLAY_DEBUG_MEDIUM: reference, every pixel of the render size traced each frame, no history");
        }
        if *VIEW != DebugView::None {
            info!("debug switch NVF_PLAY_DEBUG_MEDIUM_VIEW: the far buffer shows {:?} instead of the light", *VIEW);
        }
        if *FRESH != FRESH_SHARE {
            info!("debug switch NVF_PLAY_DEBUG_MEDIUM_FRESH: fresh share {:.2}", *FRESH);
        }
        if *MODE != MediumMode::Reference {
            info!("volumetric medium: history filter {:?}", *HISTORY_FILTER);
        }
        if *FRESH_MOVING != FRESH_MOVING_SHARE {
            info!("debug switch NVF_PLAY_DEBUG_MEDIUM_FRESH_MOVING: the fresh share rises to {:.2} in motion", *FRESH_MOVING);
        }
        if *OFFSETS != RayOffsets::Turning {
            info!("debug switch NVF_PLAY_DEBUG_MEDIUM_OFFSETS: ray start offsets {:?}", *OFFSETS);
        }
        if *CLAMP != HistoryClamp::Moving {
            info!("debug switch NVF_PLAY_DEBUG_MEDIUM_CLAMP: history clamp {:?}, k {:.2}", *CLAMP, *CLAMP_K_SET);
        } else if *CLAMP_K_SET != CLAMP_K {
            info!("debug switch NVF_PLAY_DEBUG_MEDIUM_CLAMP_K: the moving clamp's k {:.2}", *CLAMP_K_SET);
        }
        if let Some(scale) = *SCALE_AID {
            info!("debug switch NVF_PLAY_DEBUG_MEDIUM_SCALE: the far buffer is the main pass's render size divided by {scale} whatever the row's level (rounded up; not in the reference mode)");
        }
        if *TILE != TILE_SIDE {
            info!("debug switch NVF_PLAY_DEBUG_MEDIUM_TILE: {} every frame, still blended with the history (not in the reference mode)", tile_words(*TILE));
        }
    }
    let (wanted, written) = link.read();
    let (ledger, p) = plan(state.ledger, Inputs { on: request.wanted, tracing: request.trace.is_some(), key, wanted, written, frame: frame.0, view });
    state.ledger = ledger;
    if p.release && output.image.take().is_some() {
        // The composing material is gone with the source's proxy; the last handle drops here.
        info!("volumetric medium: far buffer released");
        state.told_ready = None;
    }
    if let Some(size) = p.alloc {
        // The old image goes when the material lets go of it (show_volume swaps it this frame).
        output.image = Some(images.add(far_image(size)));
        info!("volumetric medium: far buffer {}", describe_buffer(size, *MODE, scale_in_force(request.level), *TILE));
    }
    let ready = p.ready && output.image.is_some();
    // The hand-back lines: each hand-back from a drawn volume and each take-over
    // after one is kept as a fixed pending note (the last reason and a count); the notes are
    // logged when the limiter admits a line (the first at once, then at most one batch a
    // second), and also when nothing further happens, so none is lost. Nothing is formatted
    // before then.
    if output.ready && !ready {
        state.handbacks = state.handbacks.wrapping_add(1);
        state.unsaid = state.unsaid.saturating_add(1);
        state.unsaid_why = p.why;
    } else if ready && !output.ready && state.handbacks > 0 {
        state.unsaid_takeover = true;
    }
    if state.unsaid > 0 || state.unsaid_takeover {
        let now = std::time::Instant::now();
        if state.told_handback.is_none_or(|t| now.duration_since(t).as_secs_f32() >= 1.0) {
            state.told_handback = Some(now);
            if state.unsaid > 0 {
                let why = state.unsaid_why.map_or_else(|| "the published image went".to_string(), HandBack::text);
                if state.unsaid > 1 {
                    info!("volumetric medium: hand-back to the flat layers: {why} ({} hand-backs since the last line, this the last)", state.unsaid);
                } else {
                    info!("volumetric medium: hand-back to the flat layers: {why}");
                }
            }
            if state.unsaid_takeover && ready {
                info!("volumetric medium: the volume takes over again ({} hand-backs so far)", state.handbacks);
            }
            state.unsaid = 0;
            state.unsaid_takeover = false;
        }
    }
    if output.epoch != ledger.epoch || output.ready != ready || output.size != ledger.image {
        output.epoch = ledger.epoch;
        output.ready = ready;
        output.size = ledger.image;
    }
    if ready && state.told_ready != ledger.image {
        state.told_ready = ledger.image;
        if let Some(size) = ledger.image {
            info!("volumetric medium: far buffer {}x{} written whole and {} ready, the clouds drawn from it", size.x, size.y, crate::volumetric_clouds::PIPELINE_LABEL);
        }
    }
    if request.wanted {
        state.draining = None;
        if !gate.is_open() {
            gate.set_open(true);
        }
        if !active.0 {
            active.0 = true;
        }
        return;
    }
    let frames = state.draining.get_or_insert(0);
    *frames += 1;
    if *frames >= IDLE_FRAMES {
        // A hand-back the limiter held back is logged now, before everything goes.
        if state.unsaid > 0 {
            let why = state.unsaid_why.map_or_else(|| "the published image went".to_string(), HandBack::text);
            info!("volumetric medium: hand-back to the flat layers: {why} ({} hand-backs since the last line)", state.unsaid);
            state.unsaid = 0;
            state.unsaid_takeover = false;
        }
        gate.set_open(false);
        link.clear();
        state.draining = None;
        active.0 = false;
    }
}

// ---------------------------------------------------------------------------------------
// Render world
// ---------------------------------------------------------------------------------------

/// The main camera's view while a trace is asked for (only ever on the one view that owns the
/// history: extraction takes it off every other).
#[derive(Component)]
pub struct MediumView;

/// This frame's trace (render world), from the main world's request and output.
#[derive(Resource, Default)]
struct MediumFrame(Option<TraceFrame>);

struct TraceFrame {
    epoch: u32,
    /// The far buffer's scale ([`scale_in_force`]).
    scale: f64,
    image: Option<AssetId<Image>>,
    clouds: CloudVolumeUniform,
    composer: Option<UntypedAssetId>,
    composer_entity: Option<Entity>,
    distance_scale: f32,
    noise: Option<[AssetId<Image>; 2]>,
    weather: Option<u32>,
    sky_cuts: u32,
    /// The main-world camera whose view owns the buffer and its history.
    owner: Option<Entity>,
    /// The main camera as the upscalers' cut test sees it (`UpscaleCamera::cut_from`, shared
    /// for every anti-aliasing mode); `None` without a perspective main camera.
    camera: Option<UpscaleCamera>,
}

/// The passes' uniform buffers, made on first use and dropped when nothing is traced.
#[derive(Resource, Default)]
pub(crate) struct MediumGpu {
    trace: Option<UniformBuffer<MediumTraceUniform>>,
    clouds: Option<UniformBuffer<CloudVolumeUniform>>,
    clouds_written: Option<CloudVolumeUniform>,
    resolve: Option<UniformBuffer<MediumResolveUniform>>,
    /// The noise textures' sampler (repeating on all three axes, linear within and between levels):
    /// made with the first trace and released with the activation.
    noise_sampler: Option<Sampler>,
}

/// The passes' pipelines, the layouts made and each pipeline asked for the first time its mode
/// traces (a run that never turns the row on never loads the shaders).
#[derive(Resource, Default)]
pub(crate) struct MediumPipelines(Option<Pipelines>);

pub(crate) struct Pipelines {
    trace_layout: BindGroupLayoutDescriptor,
    resolve_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    /// The trace without a view and its view variant (volumetric_clouds.rs `view_variant`), each queued the
    /// first time it is the one to draw with.
    reference: [Option<CachedRenderPipelineId>; 2],
    tiles: [Option<CachedRenderPipelineId>; 2],
    resolve: Option<CachedRenderPipelineId>,
}

/// What a history belongs to: the view (its main-world camera), the anti-aliasing actually in
/// force on it (samples, jitter), the full render extent, the buffer's size and the main
/// world's epoch (which moves with the upscaler, its quality, HDR and a new image). Any change
/// makes a new history. FXAA, SMAA and single-sampled anti-aliasing off
/// share a key on purpose: they draw the same unjittered single-sampled view, so the buffer's
/// history is the same picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryKey {
    pub view: Entity,
    pub samples: u32,
    pub jitter: bool,
    pub render: UVec2,
    pub size: UVec2,
    pub epoch: u32,
}

type TraceGroupKey = (BufferId, BufferId, TextureViewId, SamplerId, TextureViewId);
type ResolveGroupKey = (BufferId, TextureViewId, TextureViewId, TextureViewId, TextureViewId);

/// The default mode's history and what the render world remembers between frames.
#[derive(Resource, Default)]
pub(crate) struct MediumHistory {
    set: Option<HistorySet>,
    /// The published image that has had a whole write at its size: only it may be left as it is
    /// when a frame cannot draw (one frame of last frame's clouds, then the flat layers); any
    /// other is cleared, so uninitialised contents are never shown.
    whole: Option<AssetId<Image>>,
    /// The proxy's pipelines whose failure, or long wait, was logged.
    told_pipelines: Vec<CachedRenderPipelineId>,
    /// When the proxy's current pipeline was first seen not ready.
    waiting_since: Option<(CachedRenderPipelineId, std::time::Instant)>,
    /// This render frame's verdict, published to the link once at the end of the frame
    /// ([`publish_verdict`]).
    pending: Option<Written>,
    /// Since when the proxy has had no entry in the view's specialisation cache, and whether
    /// that was logged.
    missing_since: Option<std::time::Instant>,
    told_missing: bool,
    /// The bind groups kept while their buffers and views stay the same; the
    /// reconstruction's by which history set it reads.
    trace_group: Option<(TraceGroupKey, BindGroup)>,
    resolve_groups: [Option<(ResolveGroupKey, BindGroup)>; 2],
    /// The history's cuts, tallied for the log (#3032 B1b).
    cuts: CutTally,
    /// Whether this activation logged that the source's march caps a ray's work
    /// (`refined_rays_capped`).
    told_capped: bool,
    /// What it was last looked for at ([`CapLook`]).
    cap_look: CapLook,
    /// The noise images' wait for the GPU.
    upload_wait: UploadWait,
    /// The proxy's own pipeline, or one of the passes', has failed for good, and whether the passes
    /// drew this frame ([`buffer_failed`]).
    composer_failed: bool,
    passes_failed: bool,
    drew: bool,
}

/// How long the proxy's pipeline may stay unready before the log names it [design].
const PIPELINE_WAIT_SECS: f32 = 10.0;

/// How long the source's generated noise images may stay off the GPU before the log names them
/// (their generation's own wait is the source's line) [design].
pub const NOISE_UPLOAD_WAIT_SECS: f32 = 10.0;

/// Both of the source's noise textures' GPU views (`view` looks one up), or none while either is
/// missing: the trace writes nothing whole until both are there.
pub fn noise_views<T>(ids: Option<[AssetId<Image>; 2]>, view: impl Fn(AssetId<Image>) -> Option<T>) -> Option<[T; 2]> {
    let [b, e] = ids?;
    Some([view(b)?, view(e)?])
}

/// A frame's verdict ([`medium_trace_pass`]): whole when the passes drew and the proxy is ready;
/// otherwise why not, the extent first, then the noise textures, then the pipelines.
pub fn write_cause(drawn: bool, composer_ready: bool, size_ok: bool, noise_ready: bool) -> WriteCause {
    match (drawn, composer_ready) {
        (true, true) => WriteCause::Whole,
        (true, false) => WriteCause::ComposerNotReady,
        (false, _) if !size_ok => WriteCause::Extent,
        (false, _) if !noise_ready => WriteCause::NoiseNotReady,
        (false, _) => WriteCause::PassesNotReady,
    }
}

/// When the capped rays are looked for (`refined_rays_capped`, some thousand rays' plans on the CPU):
/// at the start of an activation and at a change of the level (the far buffer's scale), of the
/// weather or of a live aid the plan reads, each once, with no fade running.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CapLook(Option<(f64, u32, Vec4)>);

impl CapLook {
    /// Whether to look this frame; `weather`: the settled one, `None` in a fade; `aids`:
    /// [`plan_aids`].
    pub(crate) fn due(&mut self, scale: f64, weather: Option<u32>, aids: Vec4) -> bool {
        let Some(weather) = weather else { return false };
        self.0.replace((scale, weather, aids)) != Some((scale, weather, aids))
    }
}

/// What a live edit of the aids moves of a ray's plan (the weather moves none of it): the near step
/// (`STEPS`), the relief (`SHAPE`, the sampling ceiling's), the erosion and the wisps (the detail
/// bound's).
pub(crate) fn plan_aids(u: &CloudVolumeUniform) -> Vec4 {
    Vec4::new(u.march.x, u.nscale.z, u.nscale.w, u.dshape.z)
}

/// The wait for the source's generated noise images to reach the GPU:
/// a pair of images (made fresh by each activation) whose views are not both there after
/// [`NOISE_UPLOAD_WAIT_SECS`] is named once.
#[derive(Default)]
pub(crate) struct UploadWait {
    since: Option<([AssetId<Image>; 2], std::time::Instant)>,
    told: Option<[AssetId<Image>; 2]>,
}

impl UploadWait {
    /// This frame's images `ids` and which of their views (base, erosion) are on the GPU: the
    /// views to name, once per pair of images, when they have waited long enough.
    pub(crate) fn note(&mut self, ids: Option<[AssetId<Image>; 2]>, present: [bool; 2], now: std::time::Instant) -> Option<[bool; 2]> {
        let Some(ids) = ids.filter(|_| present != [true, true]) else {
            self.since = None;
            return None;
        };
        let since = match self.since {
            Some((waiting, at)) if waiting == ids => at,
            _ => {
                self.since = Some((ids, now));
                now
            }
        };
        if now.duration_since(since).as_secs_f32() >= NOISE_UPLOAD_WAIT_SECS && self.told != Some(ids) {
            self.told = Some(ids);
            return Some(present);
        }
        None
    }
}

/// Last frame's camera as a reconstruction keeps it: the camera-relative clip transform, its
/// inverse direction transform (clip to a world direction) and the camera's position.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PrevView {
    clip_from_rel: Mat4,
    world_from_clip: Mat4,
    origin: Vec3,
}

/// The history textures of one key (two of each: last frame's is read, this frame's written)
/// and the state of the last whole reconstruction.
pub(crate) struct HistorySet {
    key: HistoryKey,
    light: [TextureView; 2],
    aux: [TextureView; 2],
    /// Which of the two holds last frame's history.
    current: usize,
    /// The cast pattern's phase, advanced on each whole reconstruction only.
    phase: u32,
    /// The frame of the last whole reconstruction.
    last_frame: Option<u32>,
    prev: Option<PrevView>,
    camera: Option<UpscaleCamera>,
    sky_cuts: u32,
    /// Whole reconstructions since the last cut ([`casts_after`]).
    casts: u32,
}

impl HistorySet {
    fn new(device: &RenderDevice, key: HistoryKey) -> Self {
        let size = key.size;
        let texture = |label: &'static str, format: TextureFormat| {
            device
                .create_texture(&TextureDescriptor {
                    label: Some(label),
                    size: Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&default())
        };
        HistorySet {
            key,
            light: [texture("medium_history_light_a", FAR_FORMAT), texture("medium_history_light_b", FAR_FORMAT)],
            aux: [texture("medium_history_aux_a", HISTORY_AUX_FORMAT), texture("medium_history_aux_b", HISTORY_AUX_FORMAT)],
            current: 0,
            phase: 0,
            last_frame: None,
            prev: None,
            camera: None,
            sky_cuts: 0,
            casts: 0,
        }
    }
}

/// Whether the history no longer holds this frame: none yet, a gap
/// since the last whole reconstruction (a frame skipped, the row switched on again, a trace
/// asked for again outdoors), the camera's cut (the upscalers' test, for every anti-aliasing
/// mode: a teleport, a sharp turn, a lens jump, a long frame, a loading screen, dialogue), or
/// a jump of the sky. Another view, anti-aliasing, render extent or epoch makes a new history
/// (the set is made again, [`HistoryKey`]).
#[cfg_attr(not(test), allow(dead_code))]
pub fn history_cut(last_frame: Option<u32>, frame: u32, before: Option<&UpscaleCamera>, now: Option<&UpscaleCamera>, sky_cuts: (u32, u32)) -> bool {
    history_cut_causes(last_frame, frame, before, now, sky_cuts).any()
}

/// Why [`history_cut`] cuts, one flag per cause (#3032 B1b, for the log's tally).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CutCauses {
    pub gap: bool,
    pub camera: bool,
    /// Of the camera's cuts, a frame longer than the upscalers' limit (upscaler/prep.rs
    /// `cut_from`: 250 ms).
    pub long_frame: bool,
    pub sky: bool,
}

impl CutCauses {
    pub fn any(self) -> bool {
        self.gap || self.camera || self.sky
    }
}

/// [`history_cut`]'s causes.
pub fn history_cut_causes(last_frame: Option<u32>, frame: u32, before: Option<&UpscaleCamera>, now: Option<&UpscaleCamera>, sky_cuts: (u32, u32)) -> CutCauses {
    let gap = last_frame.is_none_or(|f| f.wrapping_add(1) != frame);
    let (camera, long_frame) = match (before, now) {
        (Some(b), Some(n)) => (n.cut_from(b), n.delta_ms > 250.0),
        _ => (true, false),
    };
    CutCauses { gap, camera, long_frame: camera && long_frame, sky: sky_cuts.0 != sky_cuts.1 }
}

/// The reconstruction's history cuts over a window of frames (#3032 B1b: a history dropped every
/// frame leaves eight texels of nine, three of four at side 2, filled from the cast rays around,
/// soft unsteady edges whatever the fresh share or the start offsets), logged at the end of the first
/// window, then only at the end of a window with cuts whose dominant cause differs from the last
/// logged one's (never a line every window in ordinary play, where a camera cut
/// now and then is normal); every window with a cut while `NVF_PLAY_DEBUG_MEDIUM_TALLY=1`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CutTally {
    pub frames: u32,
    pub cuts: u32,
    pub gap: u32,
    pub camera: u32,
    pub long_frame: u32,
    pub sky: u32,
    /// A new history set (another key) or none before.
    pub new: u32,
    pub windows: u32,
    /// The dominant cause of the last window logged with cuts.
    pub told: Option<CutCause>,
}

/// A window's dominant cause of history cuts ([`CutTally`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutCause {
    Gap,
    /// A camera cut on a frame no longer than 250 ms.
    Camera,
    /// A camera cut on a frame over 250 ms.
    LongFrame,
    Sky,
    New,
}

/// Frames in one window of [`CutTally`] [engineering choice: about two seconds at 60 frames a
/// second].
pub const CUT_WINDOW: u32 = 120;

impl CutTally {
    /// The window's dominant cause (the most counted; the first of equals in the order gap,
    /// camera, long frame, sky jump, new history); `None` without a cut.
    pub fn dominant(&self) -> Option<CutCause> {
        if self.cuts == 0 {
            return None;
        }
        let counts = [
            (self.gap, CutCause::Gap),
            (self.camera.saturating_sub(self.long_frame), CutCause::Camera),
            (self.long_frame, CutCause::LongFrame),
            (self.sky, CutCause::Sky),
            (self.new, CutCause::New),
        ];
        counts.iter().fold(None::<(u32, CutCause)>, |best, &(n, c)| if n > 0 && best.is_none_or(|(m, _)| n > m) { Some((n, c)) } else { best }).map(|(_, c)| c)
    }

    /// Count one frame; at the end of a window, the line to log (the first window always; later
    /// ones with cuts when their dominant cause changed, or every one with cuts when `every`),
    /// and the tally starts again.
    pub fn count(&mut self, causes: CutCauses, new: bool, every: bool) -> Option<String> {
        self.frames += 1;
        if causes.any() || new {
            self.cuts += 1;
        }
        self.gap += u32::from(causes.gap);
        self.camera += u32::from(causes.camera);
        self.long_frame += u32::from(causes.long_frame);
        self.sky += u32::from(causes.sky);
        self.new += u32::from(new);
        if self.frames < CUT_WINDOW {
            return None;
        }
        let dominant = self.dominant();
        let changed = dominant.is_some() && dominant != self.told;
        let line = (self.windows == 0 || changed || (every && self.cuts > 0)).then(|| {
            format!(
                "volumetric medium: history dropped on {} of the last {} frames (a gap {}, a camera cut {} of which a frame over 250 ms {}, a sky jump {}, a new or missing history {})",
                self.cuts, self.frames, self.gap, self.camera, self.long_frame, self.sky, self.new
            )
        });
        let told = if line.is_some() && dominant.is_some() { dominant } else { self.told };
        *self = CutTally { windows: self.windows.saturating_add(1), told, ..CutTally::default() };
        line
    }
}

/// `NVF_PLAY_DEBUG_MEDIUM_TALLY`: `1` logs every window of history cuts that had one ([`CutTally`]);
/// anything else (or unset) only the first window and changes of the dominant cause.
pub fn debug_tally(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.trim() == "1")
}

static TALLY_EVERY: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| debug_tally(crate::dev_aids::var("NVF_PLAY_DEBUG_MEDIUM_TALLY").as_deref()));

/// Upload an exact float phase. The 64 offset turns take four phases each at side 2 and
/// nine at side 3. Every clipped-tile corner cycle divides its respective upload period.
pub fn phase_upload(phase: u32, side: u32) -> f32 {
    (phase % if side == 3 { 576 } else { 256 }) as f32
}

/// What [`medium_trace_pass`] needs for the view this frame (made by [`prepare_medium`]).
#[derive(Component)]
pub(crate) struct MediumPass {
    epoch: u32,
    frame: u32,
    owner: Entity,
    mode: MediumMode,
    /// The trace draws its view variant (the clouds' uniform it was prepared with asks for a view).
    views: bool,
    image: AssetId<Image>,
    far: TextureView,
    /// The cloud source's noise textures on the GPU (the base, the erosion), when both are.
    noise: Option<[TextureView; 2]>,
    size: UVec2,
    wanted: UVec2,
    /// The main pass's render size (the upscalers' when they draw), for the log.
    render: UVec2,
    samples: u32,
    jitter: bool,
    /// The trace's targets: the light and the distance (the buffer's size in the reference
    /// mode, the trace size by default).
    trace_light: Option<CachedTexture>,
    distance: CachedTexture,
    /// The composing material is prepared and the proxy's own pipeline in this view is ready.
    composer_ready: bool,
    /// What the history becomes when this frame's reconstruction is whole.
    next: Option<NextHistory>,
}

pub(crate) struct NextHistory {
    frame: u32,
    prev: Option<PrevView>,
    camera: Option<UpscaleCamera>,
    sky_cuts: u32,
    /// The history was cut this frame.
    reset: bool,
}

/// The request and the published image onto the render world; [`MediumView`] on the main
/// camera's view while a trace is asked for, and off every other view, with the camera's cut
/// inputs. The last frame's pass state always goes (made again by [`prepare_medium`]), so the
/// pass never draws from a stale one.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn extract_medium(
    mut commands: Commands,
    request: Extract<Res<MediumRequest>>,
    output: Extract<Res<MediumOutput>>,
    cams: Extract<Query<(Entity, RenderEntity, &Projection, &GlobalTransform), With<FlyCam>>>,
    (frame_count, time): (Extract<Res<FrameCount>>, Extract<Res<Time<Real>>>),
    loading: Extract<Option<Res<nvf_play::loading::Loading>>>,
    play_state: Extract<Option<Res<State<nvf_play::PlayState>>>>,
    marked: Query<Entity, Or<(With<MediumView>, With<MediumPass>)>>,
    (mut frame, mut history): (ResMut<MediumFrame>, ResMut<MediumHistory>),
) {
    // The capped line once per activation of the row: re-armed
    // when no source wants the medium, not on a frame with nothing to trace (indoors, a clear sky).
    if !request.wanted && (history.told_capped || history.cap_look != CapLook::default()) {
        history.told_capped = false;
        history.cap_look = CapLook::default();
    }
    let main = cams.single().ok();
    let camera = main.and_then(|(_, _, projection, transform)| {
        let Projection::Perspective(p) = projection else { return None };
        Some(UpscaleCamera {
            settings: UpscaleSettings::default(),
            translation: transform.translation(),
            rotation: transform.rotation(),
            near: p.near,
            fov: p.fov,
            frame: frame_count.0,
            delta_ms: (time.delta_secs() * 1000.0).max(1.0),
            loading_generation: loading.as_ref().map_or(0, |l| l.shown),
            play_state: play_state.as_ref().map(|s| view_state(*s.get())),
            screen_mark: false,
        })
    });
    frame.0 = request.trace.as_ref().map(|t| TraceFrame {
        epoch: output.epoch,
        scale: scale_in_force(request.level),
        image: output.image.as_ref().map(Handle::id),
        clouds: t.clouds,
        composer: t.composer,
        composer_entity: t.composer_entity,
        distance_scale: t.distance_scale,
        noise: t.noise,
        weather: t.weather,
        sky_cuts: request.sky_cuts,
        owner: main.map(|m| m.0),
        camera,
    });
    let owner = main.filter(|_| frame.0.is_some()).map(|m| m.1);
    for e in &marked {
        if Some(e) == owner {
            commands.entity(e).try_remove::<MediumPass>();
        } else {
            commands.entity(e).try_remove::<(MediumView, MediumPass)>();
        }
    }
    if let Some(e) = owner {
        commands.entity(e).try_insert(MediumView);
    }
}

/// Whether the proxy's own main-pass pipeline in this view is ready: the id
/// bevy_pbr specialised for that entity in that view (`SpecializedMaterialPipelineCache`), read
/// every frame, so a live change of the mesh pipeline key (the shadow filter, multisampling,
/// HDR) hands back to the flat layers until the new variant is ready. A failed variant, or one
/// still not ready after [`PIPELINE_WAIT_SECS`], is named in the log once.
fn composer_pipeline_ready(cache: &PipelineCache, id: Option<CachedRenderPipelineId>, history: &mut MediumHistory) -> bool {
    let Some(id) = id else { return false };
    match cache.get_render_pipeline_state(id) {
        CachedPipelineState::Ok(_) => {
            history.waiting_since = None;
            true
        }
        CachedPipelineState::Err(e) => {
            if !history.told_pipelines.contains(&id) {
                history.told_pipelines.push(id);
                warn!("volumetric medium: the proxy's pipeline ({}, id {id:?}) failed: {e}; the flat layers stay", crate::volumetric_clouds::PIPELINE_LABEL);
            }
            false
        }
        _ => {
            let since = match history.waiting_since {
                Some((waiting, at)) if waiting == id => at,
                _ => {
                    let now = std::time::Instant::now();
                    history.waiting_since = Some((id, now));
                    now
                }
            };
            if since.elapsed().as_secs_f32() >= PIPELINE_WAIT_SECS && !history.told_pipelines.contains(&id) {
                history.told_pipelines.push(id);
                warn!("volumetric medium: the proxy's pipeline ({}, id {id:?}) not ready after {PIPELINE_WAIT_SECS:.0} s; the flat layers stay", crate::volumetric_clouds::PIPELINE_LABEL);
            }
            false
        }
    }
}

/// For the owner's view: every frame first reported "not written" (no view, no image, a pass
/// that does not run all leave it so), the size the buffer must have reported, the pipelines
/// asked for, the uniforms written, the trace targets taken from the texture cache, the history
/// kept (made again for a new [`HistoryKey`]) and its cut decided, and the pass's state.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn prepare_medium(
    mut commands: Commands,
    frame: Res<MediumFrame>,
    link: Res<MediumLink>,
    (cache, mut pipelines, assets, fullscreen): (Res<PipelineCache>, ResMut<MediumPipelines>, Res<AssetServer>, Res<FullscreenShader>),
    (mut gpu, mut history, mut lv): (ResMut<MediumGpu>, ResMut<MediumHistory>, ResMut<crate::volumetric_clouds_lightvol::gpu::LvGpu>),
    (device, queue): (Res<RenderDevice>, Res<RenderQueue>),
    (images, materials, specialized): (Res<RenderAssets<GpuImage>>, Option<Res<ErasedRenderAssets<PreparedMaterial>>>, Option<Res<SpecializedMaterialPipelineCache>>),
    mut textures: ResMut<TextureCache>,
    frame_count: Res<FrameCount>,
    views: Query<(Entity, &MainEntity, &ExtractedView, Option<&MainPassResolutionOverride>, &Msaa, Has<TemporalJitter>), With<MediumView>>,
) {
    let Some(trace) = frame.0.as_ref() else {
        if gpu.trace.is_some() || gpu.clouds.is_some() || gpu.resolve.is_some() || gpu.noise_sampler.is_some() {
            *gpu = MediumGpu::default();
        }
        // Off (or nothing to trace) frees everything that holds a texture view of the history,
        // the trace targets, the cover or the buffers.
        if history.set.is_some() || history.trace_group.is_some() || history.resolve_groups.iter().any(Option::is_some) {
            history.set = None;
            history.trace_group = None;
            history.resolve_groups = [None, None];
        }
        lv.clear();
        // (The capped line is re-armed only when the row goes off, in `extract_medium`:
        // once per row activation, not once per trace gap.)
        return;
    };
    let now = frame_count.0;
    // The noise images generated but not on the GPU: named once per activation after a while
    // (the generation's own wait is the source's line).
    let present = trace.noise.map_or([false; 2], |[b, e]| [images.get(b).is_some(), images.get(e).is_some()]);
    if let Some([base, erosion]) = history.upload_wait.note(trace.noise, present, std::time::Instant::now()) {
        let state = |there: bool| if there { "on the GPU" } else { "missing" };
        warn!(
            "volumetric medium: the clouds' noise textures were generated but are not both on the GPU after {NOISE_UPLOAD_WAIT_SECS:.0} s (base {}, erosion {}); the flat layers stay",
            state(base),
            state(erosion)
        );
    }
    history.pending = Some(Written { epoch: trace.epoch, frame: now, view: None, size: UVec2::ZERO, ok: false, cause: WriteCause::NoView });
    let mode = *MODE;
    for (entity, main, view, resolution, msaa, jitter) in &views {
        let owner = main.id();
        if trace.owner != Some(owner) {
            continue;
        }
        history.pending = Some(Written { epoch: trace.epoch, frame: now, view: Some(owner), size: UVec2::ZERO, ok: false, cause: WriteCause::NoImage });
        let render = resolution.map_or(view.viewport.zw(), |r| r.0).max(UVec2::ONE);
        let wanted = buffer_size(render, mode, trace.scale);
        link.set_wanted(Wanted { epoch: trace.epoch, size: wanted });
        let pl = pipelines.0.get_or_insert_with(|| make_pipelines(&device));
        let shader = || assets.load("embedded://nvforever/medium_trace.wesl");
        // This frame's uniform picks the variant (a live change of the view asks for the other one then;
        // both ids are kept).
        let (label, entry) = trace_entry(mode);
        let views = view_variant(&trace.clouds);
        match mode {
            MediumMode::Reference => {
                pl.reference[usize::from(views)].get_or_insert_with(|| cache.queue_render_pipeline(trace_pipeline(&pl.trace_layout, shader(), label, entry, &fullscreen, views)));
            }
            MediumMode::Default => {
                pl.tiles[usize::from(views)].get_or_insert_with(|| cache.queue_render_pipeline(trace_pipeline(&pl.trace_layout, shader(), label, entry, &fullscreen, views)));
                pl.resolve.get_or_insert_with(|| cache.queue_render_pipeline(resolve_pipeline(&pl.resolve_layout, assets.load("embedded://nvforever/medium_resolve.wesl"), &fullscreen)));
            }
        }
        let Some(image_id) = trace.image else { continue };
        let Some(image) = images.get(image_id).filter(|g| g.texture_descriptor.format == FAR_FORMAT) else { continue };
        let extent = image.texture_descriptor.size;
        let size = UVec2::new(extent.width, extent.height);
        let mut uniform = prepared_trace_uniform(view.world_from_view.to_matrix(), view.clip_from_view, size, now, mode, *TILE);
        // Once per activation, level, settled weather and live edit until one is found: whether these
        // settings make the march cap a ray's work (probes from the camera at its real spread, so once
        // the buffer has the wanted size, volumetric_clouds.rs). Its fine cells grow together (#3032
        // stage 2c, checkpoint C1).
        if !history.told_capped && size == wanted && history.cap_look.due(trace.scale, trace.weather, plan_aids(&trace.clouds)) && refined_rays_capped(uniform.origin.truncate(), uniform.origin.w, &trace.clouds) {
            history.told_capped = true;
            info!("volumetric clouds: the refined march is capped at {RAY_BUDGET} evaluations a ray (probes, fine samples and light taps): with these settings a ray found by sampling the elevations every 0.1 degree has fine cells wider than the detail bound asks");
        }
        let target = |label: &'static str, format: TextureFormat, at: UVec2| TextureDescriptor {
            label: Some(label),
            size: Extent3d { width: at.x, height: at.y, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let (trace_light, distance, next) = match mode {
            MediumMode::Reference => {
                history.set = None;
                (None, textures.get(&device, target("medium_far_distance", DISTANCE_FORMAT, size)), None)
            }
            MediumMode::Default => {
                let key = HistoryKey { view: owner, samples: msaa.samples(), jitter, render, size, epoch: trace.epoch };
                if history.set.as_ref().is_none_or(|s| s.key != key) {
                    history.set = Some(HistorySet::new(&device, key));
                    history.resolve_groups = [None, None];
                }
                let Some(set) = history.set.as_ref() else { continue };
                let causes = history_cut_causes(set.last_frame, now, set.camera.as_ref(), trace.camera.as_ref(), (set.sky_cuts, trace.sky_cuts));
                let reset = causes.any() || set.prev.is_none();
                let no_prev = set.prev.is_none();
                if let Some(line) = history.cuts.count(causes, no_prev, *TALLY_EVERY) {
                    info!("{line}");
                }
                let Some(set) = history.set.as_ref() else { continue };
                uniform.tiles = Vec4::new(phase_upload(set.phase, *TILE), OFFSETS.code(), *TILE as f32, 0.0);
                let origin = view.world_from_view.translation();
                let prev = set.prev.unwrap_or(PrevView { clip_from_rel: Mat4::IDENTITY, world_from_clip: Mat4::IDENTITY, origin });
                let delta = origin - prev.origin;
                let resolve = MediumResolveUniform {
                    world_from_clip: uniform.world_from_clip,
                    prev_clip_from_rel: prev.clip_from_rel,
                    prev_world_from_clip: prev.world_from_clip,
                    delta: delta.extend(if reset || !delta.is_finite() { 0.0 } else { 1.0 }),
                    grid: Vec4::new(size.x as f32, size.y as f32, phase_upload(set.phase, *TILE), *FRESH),
                    params: Vec4::new(VIEW.code(), MOTION_FROM, MOTION_FULL, *FRESH_MOVING),
                    clamp_mode: Vec4::new(CLAMP.code(), *CLAMP_K_SET, *HISTORY_FILTER as u32 as f32, 0.0),
                    pattern: Vec4::new(*TILE as f32, if trace.distance_scale.is_finite() { trace.distance_scale.max(1.0) } else { 1.0 }, 0.0, 0.0),
                };
                let buffer = gpu.resolve.get_or_insert_with(|| {
                    let mut b = UniformBuffer::from(resolve);
                    b.set_label(Some("nvf_medium_resolve"));
                    b
                });
                buffer.set(resolve);
                buffer.write_buffer(&device, &queue);
                let at = trace_size(size, *TILE);
                let next = NextHistory {
                    frame: now,
                    prev: clip_from_rel(view.world_from_view.to_matrix(), view.clip_from_view).map(|clip_from_rel| PrevView { clip_from_rel, world_from_clip: uniform.world_from_clip, origin }),
                    camera: trace.camera,
                    sky_cuts: trace.sky_cuts,
                    reset,
                };
                (Some(textures.get(&device, target("medium_trace_light", FAR_FORMAT, at))), textures.get(&device, target("medium_trace_distance", DISTANCE_FORMAT, at)), Some(next))
            }
        };
        let gpu = &mut *gpu;
        gpu.noise_sampler.get_or_insert_with(|| noise_sampler(&device));
        let buffer = gpu.trace.get_or_insert_with(|| {
            let mut b = UniformBuffer::from(uniform);
            b.set_label(Some("nvf_medium_trace"));
            b
        });
        buffer.set(uniform);
        buffer.write_buffer(&device, &queue);
        // The march's uniform: written when it changed (the sun's creep within a thousandth is
        // not, as the prototype's material was not rewritten for it).
        // With the light volume (#3032 stage 2g) its block for this camera (else as it came).
        let mut lv_clouds = crate::volumetric_clouds_lightvol::gpu::lv_prepare(&mut lv, &trace.clouds, &crate::volumetric_clouds_lightvol::gpu::LvPrepare { device: &device, queue: &queue, cache: &cache, assets: &assets, fullscreen: &fullscreen, entry: trace_entry(mode), cam: uniform.origin.truncate() });
        // A cast covers the tile this frame, but the history reconstructs its individual texels.
        // The view filters cloud placement and material at that width. Reference mode casts every texel.
        lv_clouds.dshape.w = uniform.tiles.z.max(1.0).recip();
        let clouds = gpu.clouds.get_or_insert_with(|| {
            let mut b = UniformBuffer::from(lv_clouds);
            b.set_label(Some("nvf_medium_clouds"));
            b
        });
        if gpu.clouds_written.is_none_or(|w| needs_write(&w, &lv_clouds)) || clouds.buffer().is_none() {
            clouds.set(lv_clouds);
            clouds.write_buffer(&device, &queue);
            gpu.clouds_written = Some(lv_clouds);
        }
        // The composing material prepared, and the proxy's own pipeline in this view ready.
        let prepared = trace.composer.is_some_and(|id| materials.as_ref().is_some_and(|m| m.get(id).is_some()));
        // The proxy's entry in the view's specialisation cache. bevy_pbr keeps it across frames
        // and removes it only when the entity or the view must be specialised again, and queues
        // nothing for an entity without one (vendored bevy_pbr material.rs: 1186-1193 the only
        // removals, 1208-1213 an existing entry kept, in `specialize_material_meshes`; 1404 the
        // transparent phase's entry dequeued, 1422-1426 no entry queues nothing, 1589 the
        // retained entry, in `queue_material_meshes`): an
        // absent entry means the proxy is not drawn this frame, so the verdict is not whole.
        let pipeline_id = trace.composer_entity.and_then(|e| specialized.as_ref()?.get(&view.retained_view_entity)?.get(&MainEntity::from(e)).copied());
        if pipeline_id.is_none() {
            let since = *history.missing_since.get_or_insert_with(std::time::Instant::now);
            if since.elapsed().as_secs_f32() >= PIPELINE_WAIT_SECS && !history.told_missing {
                history.told_missing = true;
                warn!("volumetric medium: the proxy has no specialised pipeline in the main view after {PIPELINE_WAIT_SECS:.0} s; the flat layers stay");
            }
        } else {
            history.missing_since = None;
            history.told_missing = false;
        }
        let composer_ready = prepared && composer_pipeline_ready(&cache, pipeline_id, &mut history);
        let failed = |id: Option<CachedRenderPipelineId>| id.is_some_and(|id| matches!(cache.get_render_pipeline_state(id), CachedPipelineState::Err(e) if !crate::pipeline_gate::retried(e)));
        history.composer_failed = failed(pipeline_id);
        history.passes_failed = match mode {
            MediumMode::Reference => failed(pl.reference[usize::from(views)]),
            MediumMode::Default => failed(pl.tiles[usize::from(views)]) || failed(pl.resolve),
        };
        history.drew = false;
        history.pending = Some(Written { epoch: trace.epoch, frame: now, view: Some(owner), size, ok: false, cause: WriteCause::PassNotRun });
        commands.entity(entity).insert(MediumPass {
            epoch: trace.epoch,
            frame: now,
            owner,
            mode,
            views,
            image: image_id,
            far: image.texture_view.clone(),
            noise: noise_views(trace.noise, |id| images.get(id).map(|g| g.texture_view.clone())),
            size,
            wanted,
            render,
            samples: msaa.samples(),
            jitter,
            trace_light,
            distance,
            composer_ready,
            next,
        });
    }
}

/// The bind group layouts and the history's sampler.
fn make_pipelines(device: &RenderDevice) -> Pipelines {
    // medium_trace.wesl's bindings 0-4, in order: the trace's uniform, the march's uniform, the
    // clouds' base noise (3D), its repeating linear sampler, the erosion noise (3D) (#3032 stage
    // 2b).
    let trace_layout = BindGroupLayoutDescriptor::new(
        "medium_trace_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<MediumTraceUniform>(false),
                uniform_buffer::<CloudVolumeUniform>(false),
                texture_3d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_3d(TextureSampleType::Float { filterable: true }),
            ),
        ),
    );
    // medium_resolve.wesl's bindings 0-5: the uniform, the trace's light and distance, the
    // history's light (sampled bilinear), its sampler, its distance and age.
    let resolve_layout = BindGroupLayoutDescriptor::new(
        "medium_resolve_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<MediumResolveUniform>(false),
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("medium_history_sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        ..default()
    });
    Pipelines { trace_layout, resolve_layout, sampler, reference: [None; 2], tiles: [None; 2], resolve: None }
}

/// The noise textures' sampler: repeating on all three axes, linear within and between levels
/// (the march reads an explicit level from the footprint).
fn noise_sampler(device: &RenderDevice) -> Sampler {
    device.create_sampler(&SamplerDescriptor {
        label: Some("medium_noise_sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        ..default()
    })
}

/// The trace pipeline's label and entry (medium_trace.wesl) for the medium's mode.
pub fn trace_entry(mode: MediumMode) -> (&'static str, &'static str) {
    match mode {
        MediumMode::Reference => ("medium_trace_refined_pipeline", "medium_trace_refined"),
        MediumMode::Default => ("medium_trace_tiles_refined_pipeline", "medium_trace_tiles_refined"),
    }
}

/// A trace pipeline (medium_trace.wesl `entry`): the uniforms and the cover in, the light and
/// the distance out, no blending (every texel written). `views`: the view variant (volumetric_clouds.rs
/// `view_variant`, the shader def `VIEWS_DEF`), its label told apart (`variant_suffix`).
pub(crate) fn trace_pipeline(layout: &BindGroupLayoutDescriptor, shader: Handle<Shader>, label: &'static str, entry: &'static str, fullscreen: &FullscreenShader, views: bool) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some(format!("{label}{}", variant_suffix(views)).into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader,
            shader_defs: if views { vec![VIEWS_DEF.into()] } else { Vec::new() },
            entry_point: Some(entry.into()),
            targets: vec![
                Some(ColorTargetState { format: FAR_FORMAT, blend: None, write_mask: ColorWrites::ALL }),
                Some(ColorTargetState { format: DISTANCE_FORMAT, blend: None, write_mask: ColorWrites::ALL }),
            ],
            ..default()
        }),
        ..default()
    }
}

/// The reconstruction's pipeline (medium_resolve.wesl `medium_resolve`): the published image,
/// the next history's light and its distance and age, no blending.
fn resolve_pipeline(layout: &BindGroupLayoutDescriptor, shader: Handle<Shader>, fullscreen: &FullscreenShader) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("medium_resolve_pipeline".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("medium_resolve".into()),
            targets: vec![
                Some(ColorTargetState { format: FAR_FORMAT, blend: None, write_mask: ColorWrites::ALL }),
                Some(ColorTargetState { format: FAR_FORMAT, blend: None, write_mask: ColorWrites::ALL }),
                Some(ColorTargetState { format: HISTORY_AUX_FORMAT, blend: None, write_mask: ColorWrites::ALL }),
            ],
            ..default()
        }),
        ..default()
    }
}

/// A full-screen pass into up to three `targets` (each cleared to its colour), kept in a fixed
/// array (no allocation per pass).
fn fullscreen_pass(ctx: &mut RenderContext, label: &'static str, targets: &[(&TextureView, wgpu::Color)], draw: Option<(&RenderPipeline, &BindGroup)>) {
    let mut attachments: [Option<RenderPassColorAttachment>; 3] = [None, None, None];
    for (slot, (view, clear)) in attachments.iter_mut().zip(targets) {
        *slot = Some(RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: Operations { load: LoadOp::Clear(*clear), store: StoreOp::Store } });
    }
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &attachments[..targets.len().min(3)],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some((pipeline, bind_group)) = draw {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// What draws the trace: the light volume's variant with its group when it drew (it needs nothing of the
/// legacy one, which a live change of the view may have left compiling), else the legacy one
/// when ready.
fn trace_draw<'a, P, G>(volume: Option<&'a (P, G)>, legacy: Option<&'a P>, group: &'a G) -> Option<(&'a P, &'a G)> {
    volume.map(|(p, g)| (p, g)).or_else(|| legacy.map(|p| (p, group)))
}

/// Between the depth prepass and the main pass. The image's extent is checked first: at a size
/// other than the one wanted nothing is written and nothing of the history
/// moves. Reference mode: every texel of the published image traced. Default: one ray per tile
/// into the trace targets, then the reconstruction into the published image and the next
/// history. When that cannot be done this frame (a pipeline still compiling, the
/// size), an image that has had a whole write is left as it is (one frame of last frame's
/// clouds; the report says not whole and the flat layers come back), any other is cleared to
/// "nothing in the way", so uninitialised contents are never shown; the history is left as it
/// is too (the next whole frame finds the gap and starts it again). Then the report
/// ([`Written`]): whole only when the passes completed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn medium_trace_pass(
    view: ViewQuery<&MediumPass>,
    pipelines: Res<MediumPipelines>,
    gpu: Res<MediumGpu>,
    mut history: ResMut<MediumHistory>,
    mut lv: ResMut<crate::volumetric_clouds_lightvol::gpu::LvGpu>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let p = view.into_inner();
    let Some(pl) = pipelines.0.as_ref() else { return };
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let history = &mut *history;
    let buffers = (gpu.trace.as_ref(), gpu.clouds.as_ref());
    // The source reads no texture (the cover image is gone), so nothing but the
    // two uniforms and the extent waits here.
    let inputs = match (buffers, p.noise.as_ref(), gpu.noise_sampler.as_ref()) {
        ((Some(t), Some(c)), Some(noise), Some(ns)) if p.size == p.wanted => Some((t, c, noise, ns)),
        _ => None,
    };
    // The trace's bind group, kept while its buffers, the noise textures' views and their sampler
    // stay the same (the sampler lives as long as the activation).
    let trace_group = inputs.and_then(|(t, c, [nb, ne], ns)| {
        let key = (t.buffer()?.id(), c.buffer()?.id(), nb.id(), ns.id(), ne.id());
        if history.trace_group.as_ref().is_none_or(|(k, _)| *k != key) {
            let layout = cache.get_bind_group_layout(&pl.trace_layout);
            let group = device.create_bind_group("medium_trace_bind_group", &layout, &BindGroupEntries::sequential((t.binding()?, c.binding()?, nb, ns, ne)));
            history.trace_group = Some((key, group));
        }
        history.trace_group.as_ref().map(|(_, g)| g.clone())
    });
    // The light volume's producer on this encoder before the trace (#3032 stage 2g): its trace
    // variant and group when it drew, else the legacy ones (a pipeline compiling, the volume off).
    let lv_draw = match (inputs, trace_group.is_some()) {
        (Some((t, c, [nb, ne], ns)), true) => crate::volumetric_clouds_lightvol::gpu::lv_encode(&mut lv, &mut ctx, (&cache, &device), (t, c, nb, ns, ne), diagnostics),
        _ => None,
    };
    let drawn = match (p.mode, trace_group) {
        (MediumMode::Reference, Some(group)) => match trace_draw(lv_draw.as_ref(), pl.reference[usize::from(p.views)].and_then(|id| cache.get_render_pipeline(id)), &group) {
            Some(draw) => {
                let span = diagnostics.time_span(ctx.command_encoder(), "nvf_medium_trace");
                fullscreen_pass(&mut ctx, "medium_trace_pass", &[(&p.far, CLEAR_LIGHT), (&p.distance.default_view, CLEAR_DISTANCE)], Some(draw));
                span.end(ctx.command_encoder());
                true
            }
            None => false,
        },
        (MediumMode::Default, Some(group)) => {
            let ready = (
                trace_draw(lv_draw.as_ref(), pl.tiles[usize::from(p.views)].and_then(|id| cache.get_render_pipeline(id)), &group),
                pl.resolve.and_then(|id| cache.get_render_pipeline(id)),
                gpu.resolve.as_ref(),
                p.trace_light.as_ref(),
                history.set.as_ref(),
            );
            match ready {
                (Some(draw), Some(resolve), Some(resolve_buffer), Some(trace_light), Some(set)) => {
                    let (read, write) = (set.current, 1 - set.current);
                    let key = resolve_buffer.buffer().map(|b| (b.id(), trace_light.default_view.id(), p.distance.default_view.id(), set.light[read].id(), set.aux[read].id()));
                    let resolve_found = match (key, resolve_buffer.binding()) {
                        (Some(key), Some(binding)) => {
                            if history.resolve_groups[read].as_ref().is_none_or(|(k, _)| *k != key) {
                                let layout = cache.get_bind_group_layout(&pl.resolve_layout);
                                let g = device.create_bind_group(
                                    "medium_resolve_bind_group",
                                    &layout,
                                    &BindGroupEntries::sequential((binding, &trace_light.default_view, &p.distance.default_view, &set.light[read], &pl.sampler, &set.aux[read])),
                                );
                                history.resolve_groups[read] = Some((key, g));
                            }
                            history.resolve_groups[read].as_ref().map(|(_, g)| g.clone())
                        }
                        _ => None,
                    };
                    match (resolve_found, history.set.as_ref()) {
                        (Some(resolve_group), Some(set)) => {
                            let span = diagnostics.time_span(ctx.command_encoder(), "nvf_medium_trace");
                            fullscreen_pass(&mut ctx, "medium_trace_tiles_pass", &[(&trace_light.default_view, CLEAR_LIGHT), (&p.distance.default_view, CLEAR_DISTANCE)], Some(draw));
                            span.end(ctx.command_encoder());
                            let span = diagnostics.time_span(ctx.command_encoder(), "nvf_medium_reconstruct");
                            let aux_clear = wgpu::Color { r: EMPTY_DISTANCE as f64, g: 0.0, b: 0.0, a: 0.0 };
                            fullscreen_pass(&mut ctx, "medium_resolve_pass", &[(&p.far, CLEAR_LIGHT), (&set.light[write], CLEAR_LIGHT), (&set.aux[write], aux_clear)], Some((resolve, &resolve_group)));
                            span.end(ctx.command_encoder());
                            true
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        (_, None) => false,
    };
    history.drew = drawn;
    if drawn && lv_draw.is_some() {
        // The light volume's variant drew, so with a completed set of at least one level.
        for ((level, read), told) in ["near", "far"].into_iter().zip(lv.fronts()).zip(&LIGHT_READ) {
            if read && !told.swap(true, Ordering::Relaxed) {
                info!("volumetric medium: the trace read a completed {level} set of the light volume");
            }
        }
    }
    if drawn {
        if let (Some(set), Some(next)) = (history.set.as_mut(), p.next.as_ref()) {
            set.current = 1 - set.current;
            set.phase = set.phase.wrapping_add(1);
            set.last_frame = Some(next.frame);
            set.prev = next.prev;
            set.camera = next.camera;
            set.sky_cuts = next.sky_cuts;
            set.casts = casts_after(set.casts, next.reset);
        }
        history.whole = Some(p.image);
    } else if history.whole != Some(p.image) {
        // Never written whole at this size: cleared, never shown as it was made.
        fullscreen_pass(&mut ctx, "medium_far_clear", &[(&p.far, CLEAR_LIGHT)], None);
    }
    let cause = write_cause(drawn, p.composer_ready, p.size == p.wanted, p.noise.is_some());
    history.pending = Some(Written { epoch: p.epoch, frame: p.frame, view: Some(p.owner), size: p.size, ok: cause == WriteCause::Whole, cause });
    if drawn && !DRAWN.swap(true, Ordering::Relaxed) {
        info!(
            "volumetric medium: trace pass drawn ({}x{} buffer, render size {}x{}, {}; view: {} samples, {})",
            p.size.x,
            p.size.y,
            p.render.x,
            p.render.y,
            match p.mode {
                MediumMode::Reference => "reference: every pixel each frame".to_string(),
                MediumMode::Default => {
                    let t = trace_size(p.size, *TILE);
                    format!("{} into {}x{}, reconstructed", tile_words(*TILE), t.x, t.y)
                }
            },
            p.samples,
            if p.jitter { "jittered" } else { "not jittered" }
        );
    }
}

/// The trace pass has drawn once in this run (its one-time log line).
static DRAWN: AtomicBool = AtomicBool::new(false);
/// The trace has read a completed set of the light volume once in this run (near, far: one log
/// line each).
static LIGHT_READ: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

/// At the end of the render frame (after the graph ran): the frame's one verdict published to
/// the link, so the main world never reads a provisional one; a frame whose pass never ran
/// publishes "the trace pass did not run" (#3032).
fn publish_verdict(mut history: ResMut<MediumHistory>, link: Res<MediumLink>) {
    if let Some(verdict) = history.pending.take() {
        link.set_written(verdict);
        let (casts, sky_cuts) = history.set.as_ref().filter(|s| s.key.epoch == verdict.epoch).map_or((0, 0), |s| (s.casts, s.sky_cuts));
        link.set_run(HistoryRun { epoch: verdict.epoch, casts, sky_cuts, failed: buffer_failed(history.composer_failed, history.passes_failed, history.drew) });
    }
}

/// `NVF_PLAY_DEBUG_MEDIUM_TURN`: the view turned at a constant rate (degrees a second, positive
/// to the right), the player's heading in a played run, the free camera's yaw in the viewer,
/// so pictures and cost runs of the reconstruction can be taken mid-turn. Unset: not added.
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
fn turn_aid(time: Res<Time>, player: Option<ResMut<nvf_play::player::Player>>, mut fly: Query<&mut FlyCam>, mut told: Local<bool>) {
    let Some(rate) = *TURN else { return };
    if !*told {
        *told = true;
        info!("debug switch NVF_PLAY_DEBUG_MEDIUM_TURN: the view turns {rate:.1} degrees a second");
    }
    let step = rate.to_radians() * time.delta_secs();
    if let Some(mut player) = player {
        player.heading = (player.heading + step).rem_euclid(std::f32::consts::TAU);
    } else if let Ok(mut cam) = fly.single_mut() {
        cam.yaw = (cam.yaw - step).rem_euclid(std::f32::consts::TAU);
    }
}

#[cfg(test)]
mod cap_look_tests {
    use super::{plan_aids, CapLook};
    use crate::volumetric_clouds::{uniform, VolumeInputs};

    /// At the start, at a change of the level and at a change of the weather once its fade is over,
    /// each once, at a live edit, and again in a new activation; never every frame, capped rays or none.
    #[test]
    fn the_capped_rays_are_looked_for_once_per_start_level_and_weather() {
        let mut look = CapLook::default();
        let frames = [(3.0, Some(1)), (3.0, Some(1)), (3.0, Some(1)), (2.5, Some(1)), (2.5, Some(1)), (2.5, None), (2.5, None), (2.5, Some(2)), (2.5, Some(2)), (4.0, None), (4.0, Some(2)), (4.0, Some(2))];
        let aids = plan_aids(&uniform(&VolumeInputs { steps: 28, ..Default::default() }));
        let due: Vec<usize> = frames.iter().enumerate().filter(|(_, f)| look.due(f.0, f.1, aids)).map(|(k, _)| k).collect();
        assert_eq!(due, [0, 3, 7, 10]);
        let edited = plan_aids(&uniform(&VolumeInputs { steps: 56, ..Default::default() }));
        assert_eq!([look.due(4.0, Some(2), edited), look.due(4.0, Some(2), edited)], [true, false], "a live STEPS edit, once");
        // The weather's own inputs are none of the edits.
        let i = VolumeInputs { steps: 56, amount: 0.7, density: 2.0, ..Default::default() };
        assert!(!look.due(4.0, Some(2), plan_aids(&uniform(&i))));
        look = CapLook::default();
        assert!(look.due(4.0, Some(2), edited));
        // Only from a buffer of the wanted size (the spread is the buffer's).
        assert!(include_str!("medium.rs").contains(&["if !history.told_capped && size == wanted", " && history.cap_look.due("].concat()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(w: u32, h: u32) -> Option<ViewKey> {
        Some(ViewKey { viewport: Some(UVec2::new(w, h)), samples: 4, hdr: true, fsr: None, dlss: None, jitter: false, shadow_filter: None, prepasses: [false; 3], occlusion: false, sun: [false; 3] })
    }

    /// Runs `plan` with a stand-in render world: it wants `size` for the epoch it last saw and
    /// reports a whole write of the image it was given (a frame behind, as extraction is).
    struct Sim {
        ledger: Ledger,
        wanted: Option<Wanted>,
        written: Option<Written>,
        frame: u32,
        /// Whether the render world wrote the image in its last frame.
        drew: bool,
    }

    /// The main camera's entity in these tests.
    fn cam() -> Option<Entity> {
        Some(Entity::from_raw_u32(7).expect("an entity"))
    }

    impl Sim {
        fn new() -> Self {
            Sim { ledger: Ledger::default(), wanted: None, written: None, frame: 100, drew: false }
        }

        /// One main-world frame, then the render world's answer for it (stamped with that frame
        /// and view; the extent checked before anything is written). Returns the plan.
        fn frame(&mut self, on: bool, tracing: bool, key: Option<ViewKey>, render_size: UVec2, write_ok: bool) -> Plan {
            self.frame += 1;
            let (l, p) = plan(self.ledger, Inputs { on, tracing, key, wanted: self.wanted, written: self.written, frame: self.frame, view: cam() });
            self.ledger = l;
            self.drew = false;
            if tracing && on {
                self.wanted = Some(Wanted { epoch: l.epoch, size: render_size });
                self.written = Some(Written { epoch: l.epoch, frame: self.frame, view: cam(), size: UVec2::ZERO, ok: false, cause: WriteCause::NoImage });
                if let Some(image) = l.image {
                    self.drew = write_ok && image == render_size;
                    let cause = if self.drew {
                        WriteCause::Whole
                    } else if image != render_size {
                        WriteCause::Extent
                    } else {
                        WriteCause::PassesNotReady
                    };
                    self.written = Some(Written { epoch: l.epoch, frame: self.frame, view: cam(), size: image, ok: self.drew, cause });
                }
            }
            p
        }
    }

    /// A loading screen waits for the clouds only while they are asked for and not yet drawn from a
    /// whole tile of casts; never with the row off, with nothing to draw (indoors, under water, a
    /// clear sky) or with a pipeline that failed and nothing drawn.
    #[test]
    fn a_loading_screen_waits_for_the_clouds_only_while_they_are_to_come() {
        use CloudsWait::{Nothing, Pending, Ready};
        let needed = casts_needed(MediumMode::Default, TILE_SIDE);
        assert_eq!((needed, casts_needed(MediumMode::Default, 1), casts_needed(MediumMode::Reference, 3)), (9, 1, 0));
        let on = CloudsNow { wanted: true, tracing: true, drawn: true, casts: needed, ..Default::default() };
        assert_eq!(clouds_wait(&on, needed), Ready);
        assert_eq!(clouds_wait(&CloudsNow { casts: needed - 1, ..on }, needed), Pending, "a texel without a ray of its own");
        assert_eq!(clouds_wait(&CloudsNow { drawn: false, ..on }, needed), Pending, "the flat layers still stand in (a pipeline compiling, the noise, a new image)");
        assert_eq!(clouds_wait(&CloudsNow { casts: 0, ..on }, casts_needed(MediumMode::Reference, 3)), Ready, "the reference: every frame whole");
        // Off, whatever else stands.
        assert_eq!(clouds_wait(&CloudsNow { wanted: false, ..on }, needed), Nothing);
        assert_eq!(clouds_wait(&CloudsNow::default(), needed), Nothing);
        // No trace: indoors, under water, a clear sky; or one to come (the cover still composed).
        let none = CloudsNow { tracing: false, drawn: false, casts: 0, ..on };
        assert_eq!(clouds_wait(&none, needed), Nothing);
        assert_eq!(clouds_wait(&CloudsNow { pending: true, ..none }, needed), Pending);
        assert_eq!(clouds_wait(&CloudsNow { pending: true, drawn: true, casts: 99, ..none }, needed), Pending, "a report from before does not count");
        // Failed: not waited for while nothing is drawn; a stand-in that draws is waited for as any.
        assert_eq!(clouds_wait(&CloudsNow { failed: true, drawn: false, ..on }, needed), Nothing);
        assert_eq!(clouds_wait(&CloudsNow { failed: true, ..on }, needed), Ready);
        assert_eq!(clouds_wait(&CloudsNow { failed: true, casts: needed - 1, ..on }, needed), Pending);
        assert_eq!([(true, false, true), (true, false, false), (false, true, false)].map(|(c, p, d)| buffer_failed(c, p, d)), [true; 3], "the proxy's own, or a pass's with nothing drawn");
        assert_eq!([(false, true, true), (false, false, false), (false, false, true)].map(|(c, p, d)| buffer_failed(c, p, d)), [false; 3], "a stand-in draws, or nothing failed");
        // The count: a cut's own frame is the first cast.
        assert_eq!((casts_after(0, true), casts_after(40, true), casts_after(8, false), casts_after(u32::MAX, false)), (1, 1, 9, u32::MAX));
    }

    /// The screen may leave only after the clouds were ready READY_FRAMES frames in a row: a cut seen in
    /// the last of them starts the count again.
    #[test]
    fn a_loading_screen_leaves_the_clouds_after_three_ready_frames_in_a_row() {
        use CloudsWait::{Nothing, Pending, Ready};
        const { assert!(READY_FRAMES > REPORT_FRAMES, "longer than a report is late") };
        let mut streak = 0;
        let mut told = |wait| clouds_gate(&mut streak, wait);
        let (held, free, off) = ((true, false), (false, true), (false, false));
        assert_eq!([told(Pending), told(Ready), told(Ready), told(Ready), told(Ready)], [held, held, held, free, free]);
        assert_eq!([told(Pending), told(Ready), told(Ready)], [held; 3], "a cut: counted afresh");
        assert_eq!([told(Pending), told(Ready), told(Ready), told(Ready)], [held, held, held, free], "and again in the third frame");
        assert_eq!([told(Nothing), told(Ready)], [off, held], "nothing to wait for, at once, starts it again too");
        let mut streak = u32::MAX;
        assert_eq!(clouds_gate(&mut streak, Ready), free, "no overflow");
    }

    /// The loading screen's leaving is no cut for the medium; its coming up is (the generation), and
    /// so are the camera's jump behind it and dialogue.
    #[test]
    fn a_loading_screen_coming_down_keeps_the_history() {
        use nvf_play::PlayState::{Dialogue, Loading, Menu, Playing};
        let at = |frame: u32, state: nvf_play::PlayState, generation: u32| UpscaleCamera {
            settings: UpscaleSettings::default(),
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            near: 5.0,
            fov: 1.2,
            frame,
            delta_ms: 16.0,
            loading_generation: generation,
            play_state: Some(view_state(state)),
            screen_mark: false,
        };
        let cut = |before: UpscaleCamera, now: UpscaleCamera| history_cut(Some(before.frame), now.frame, Some(&before), Some(&now), (0, 0));
        assert!(!cut(at(7, Loading, 3), at(8, Playing, 3)), "down: kept");
        assert!(!cut(at(7, Loading, 3), at(8, Menu, 3)), "down to the menu: kept");
        assert!(cut(at(7, Playing, 2), at(8, Loading, 3)), "up: a cut");
        assert!(cut(at(7, Loading, 3), UpscaleCamera { translation: Vec3::X * 500.0, ..at(8, Loading, 3) }), "the travel behind it");
        assert!(cut(at(7, Playing, 3), at(8, Dialogue, 3)) && cut(at(7, Dialogue, 3), at(8, Playing, 3)), "dialogue as before");
        // The upscalers' own test is as it was.
        let before = UpscaleCamera { play_state: Some(Loading), ..at(7, Playing, 3) };
        assert!(at(8, Playing, 3).cut_from(&before));
    }

    #[test]
    fn the_buffer_is_drawn_only_after_a_whole_write_of_the_right_size() {
        let size = UVec2::new(1280, 720);
        let mut sim = Sim::new();
        // First frame: the render world has not said its size yet.
        let p = sim.frame(true, true, key(1280, 720), size, true);
        assert!(!p.ready && p.alloc.is_none() && !p.release);
        // It wants 1280x720: the image is made, not drawn yet.
        let p = sim.frame(true, true, key(1280, 720), size, true);
        assert_eq!(p.alloc, Some(size));
        assert!(!p.ready);
        // Written for the new epoch: drawn from the next frame on.
        let p = sim.frame(true, true, key(1280, 720), size, true);
        assert!(p.ready && p.alloc.is_none());
        assert!(sim.frame(true, true, key(1280, 720), size, true).ready);
    }

    #[test]
    fn a_pipeline_not_ready_keeps_the_flat_layers() {
        let size = UVec2::new(64, 36);
        let mut sim = Sim::new();
        for _ in 0..10 {
            assert!(!sim.frame(true, true, key(64, 36), size, false).ready);
        }
        // Ready from the first whole write.
        sim.frame(true, true, key(64, 36), size, true);
        assert!(sim.frame(true, true, key(64, 36), size, true).ready);
    }

    #[test]
    fn a_resize_hands_back_in_the_same_frame_and_makes_a_new_image() {
        let mut sim = Sim::new();
        for _ in 0..3 {
            sim.frame(true, true, key(1280, 720), UVec2::new(1280, 720), true);
        }
        assert!(sim.frame(true, true, key(1280, 720), UVec2::new(1280, 720), true).ready);
        // The window grows: hidden this very frame (the key changed), whatever the old report.
        let p = sim.frame(true, true, key(1920, 1080), UVec2::new(1920, 1080), true);
        assert!(!p.ready);
        let p = sim.frame(true, true, key(1920, 1080), UVec2::new(1920, 1080), true);
        assert_eq!(p.alloc, Some(UVec2::new(1920, 1080)));
        assert!(!p.ready);
        assert!(sim.frame(true, true, key(1920, 1080), UVec2::new(1920, 1080), true).ready);
    }

    #[test]
    fn a_render_size_change_the_main_world_cannot_see_is_not_drawn_at_the_old_size() {
        let mut sim = Sim::new();
        for _ in 0..3 {
            sim.frame(true, true, key(1920, 1080), UVec2::new(1920, 1080), true);
        }
        // The render world now wants 1280x720 (a size the key does not show) for the same
        // epoch. The main world still has the frame before's whole report; the render world
        // checks the extent first and writes nothing at the old size (the last whole image is
        // kept: one frame of last frame's clouds), and its report is not whole, so the next
        // frame is not drawn and a new image follows.
        let p = sim.frame(true, true, key(1920, 1080), UVec2::new(1280, 720), true);
        assert!(p.ready, "the frame before's report, whole at its own size");
        assert!(!sim.drew, "nothing written at the old size");
        assert!(sim.written.is_some_and(|w| !w.ok));
        let p = sim.frame(true, true, key(1920, 1080), UVec2::new(1280, 720), true);
        assert_eq!(p.alloc, Some(UVec2::new(1280, 720)));
        assert!(!p.ready);
        assert!(sim.frame(true, true, key(1920, 1080), UVec2::new(1280, 720), true).ready);
    }

    #[test]
    fn losing_the_trace_hands_back_at_once_and_coming_back_waits_for_a_new_write() {
        let size = UVec2::new(64, 36);
        let mut sim = Sim::new();
        for _ in 0..3 {
            sim.frame(true, true, key(64, 36), size, true);
        }
        // Indoors (or the cover gone): not drawn this frame, the image kept.
        let p = sim.frame(true, false, key(64, 36), size, true);
        assert!(!p.ready && p.alloc.is_none() && !p.release);
        assert_eq!(sim.ledger.image, Some(size));
        // Back out: the old report (another epoch) does not count.
        let p = sim.frame(true, true, key(64, 36), size, true);
        assert!(!p.ready && p.alloc.is_none());
        assert!(sim.frame(true, true, key(64, 36), size, true).ready);
        // No main camera: not drawn.
        assert!(!sim.frame(true, true, None, size, true).ready);
    }

    #[test]
    fn switching_off_drops_the_image_and_a_stale_report_never_draws() {
        let size = UVec2::new(64, 36);
        let mut sim = Sim::new();
        for _ in 0..3 {
            sim.frame(true, true, key(64, 36), size, true);
        }
        let p = sim.frame(false, false, key(64, 36), size, true);
        assert!(p.release && !p.ready);
        assert_eq!(sim.ledger.image, None);
        // On again with the old link state: a report of an old epoch is never this one's.
        let stale = sim.written;
        let (l, p) = plan(sim.ledger, Inputs { on: true, tracing: true, key: key(64, 36), wanted: sim.wanted, written: stale, frame: sim.frame + 1, view: cam() });
        assert!(!p.ready);
        assert!(p.alloc.is_none(), "a wanted size of an old epoch makes no image");
        assert_ne!(Some(l.epoch), stale.map(|w| w.epoch));
        // A zero size is never allocated.
        let (_, p) = plan(l, Inputs { on: true, tracing: true, key: key(64, 36), wanted: Some(Wanted { epoch: l.epoch, size: UVec2::ZERO }), written: None, frame: sim.frame + 2, view: cam() });
        assert_eq!(p.alloc, None);
    }

    /// A report is good for one frame and one view: with no report since (no
    /// image, no view, a pass that did not run) the flat layers come back.
    #[test]
    fn a_report_counts_only_for_its_frame_and_view() {
        let size = UVec2::new(64, 36);
        let mut sim = Sim::new();
        for _ in 0..3 {
            sim.frame(true, true, key(64, 36), size, true);
        }
        assert!(sim.frame(true, true, key(64, 36), size, true).ready);
        // No pass for three frames: the last whole report ages out.
        let last = sim.written;
        let l = sim.ledger;
        let at = |frame: u32| plan(l, Inputs { on: true, tracing: true, key: key(64, 36), wanted: sim.wanted, written: last, frame, view: cam() }).1.ready;
        let made = last.map(|w| w.frame).unwrap_or_default();
        assert!(at(made + 1) && at(made + REPORT_FRAMES), "one or two frames late is the render world's lag");
        assert!(at(made + REPORT_FRAMES + 1) && at(made + REPORT_STALE), "a late whole report keeps the volume (no flash of flat layers)");
        assert!(!at(made + REPORT_STALE + 1), "older: no pass ran for that long");
        // A late report never takes over from the flat layers.
        let flat = Ledger { ready: false, ..l };
        assert!(!plan(flat, Inputs { on: true, tracing: true, key: key(64, 36), wanted: sim.wanted, written: last, frame: made + 4, view: cam() }).1.ready);
        // A verdict "not written" hands back at once, with its cause.
        let bad = last.map(|w| Written { ok: false, cause: WriteCause::ComposerNotReady, ..w });
        let (_, p) = plan(l, Inputs { on: true, tracing: true, key: key(64, 36), wanted: sim.wanted, written: bad, frame: made, view: cam() });
        assert!(!p.ready);
        assert_eq!(p.why, Some(HandBack::NotWritten(WriteCause::ComposerNotReady)));
        assert!(p.why.is_some_and(|w| w.text().contains("its own pipeline not ready")));
        // NoView and NoImage keep their own causes though they name no view or image.
        let no_view = last.map(|w| Written { ok: false, view: None, size: UVec2::ZERO, cause: WriteCause::NoView, ..w });
        let (_, p) = plan(l, Inputs { on: true, tracing: true, key: key(64, 36), wanted: sim.wanted, written: no_view, frame: made, view: cam() });
        assert_eq!(p.why, Some(HandBack::NotWritten(WriteCause::NoView)));
        let no_image = last.map(|w| Written { ok: false, size: UVec2::ZERO, cause: WriteCause::NoImage, ..w });
        let (_, p) = plan(l, Inputs { on: true, tracing: true, key: key(64, 36), wanted: sim.wanted, written: no_image, frame: made, view: cam() });
        assert_eq!(p.why, Some(HandBack::NotWritten(WriteCause::NoImage)));
        // Another view's report never counts.
        let other = Some(Entity::from_raw_u32(8).expect("an entity"));
        let (_, p) = plan(l, Inputs { on: true, tracing: true, key: key(64, 36), wanted: sim.wanted, written: last, frame: made + 1, view: other });
        assert!(!p.ready);
        assert!(!report_current(&Written { view: None, ..Written::default() }, 0, None));
    }

    #[test]
    fn the_image_is_a_sampled_render_target_of_the_far_format() {
        let image = far_image(UVec2::new(3, 1));
        assert_eq!(image.texture_descriptor.format, FAR_FORMAT);
        assert_eq!((image.texture_descriptor.size.width, image.texture_descriptor.size.height), (3, 1));
        assert!(image.texture_descriptor.usage.contains(TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING));
        assert!(image.data.is_none());
        let none = far_image(UVec2::ZERO);
        assert_eq!((none.texture_descriptor.size.width, none.texture_descriptor.size.height), (1, 1));
        assert!((far_mib(UVec2::new(2560, 1440)) - 42.1875).abs() < 1e-3);
    }

    // --- The trace's ray and the composition's screen position (CPU mirrors of
    // medium_trace.wesl `medium_ray_dir` and medium_upsample.wesl `medium_screen_uv`) ---

    fn ray_dir(u: &MediumTraceUniform, frag: Vec2) -> Vec3 {
        let uv = frag / u.grid.truncate().truncate().max(Vec2::ONE);
        let ndc = Vec2::new(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
        let p = u.world_from_clip * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        (p.truncate() / p.w).normalize()
    }

    fn screen_uv(clip_from_world: Mat4, world: Vec3) -> Vec2 {
        let clip = clip_from_world * world.extend(1.0);
        let ndc = clip.truncate().truncate() / clip.w.max(1e-6);
        Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5)
    }

    /// The projection Bevy makes for the main camera (an infinite reverse-Z perspective).
    fn projection(size: UVec2) -> Mat4 {
        use bevy::camera::CameraProjection;
        PerspectiveProjection { fov: 1.2, aspect_ratio: size.x as f32 / size.y as f32, near: 5.0, ..default() }.get_clip_from_view()
    }

    #[test]
    fn a_still_camera_traces_each_texel_where_the_proxy_reads_it() {
        // An odd, small buffer at the render size, a camera far from the origin and turned.
        let size = UVec2::new(7, 5);
        let camera = Transform::from_xyz(123_456.0, 2_000.0, -98_765.0).looking_to(Vec3::new(0.3, 0.4, -1.0), Vec3::Y);
        let world_from_view = camera.to_matrix();
        let proj = projection(size);
        let u = trace_uniform(world_from_view, proj, size, 130);
        assert_eq!(u.grid, Vec4::new(7.0, 5.0, 2.0, RAY_END));
        assert_eq!(u.origin.truncate(), camera.translation);
        let clip_from_world = proj * world_from_view.inverse();
        // The centre texel looks along the camera's forward.
        let centre = ray_dir(&u, Vec2::new(3.5, 2.5));
        assert!(centre.dot(camera.forward().as_vec3()) > 0.99999, "{centre}");
        for y in 0..size.y {
            for x in 0..size.x {
                let frag = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let dir = ray_dir(&u, frag);
                assert!((dir.length() - 1.0).abs() < 1e-5);
                // A point 300,000 units out on that ray (the dome's distance) lands on that
                // texel's centre in the proxy's lookup.
                let uv = screen_uv(clip_from_world, camera.translation + dir * 300_000.0);
                let texel = uv * size.as_vec2();
                assert!((texel - frag).abs().max_element() < 2e-3, "texel {x},{y}: {texel}");
            }
        }
    }

    #[test]
    fn a_jittered_fragment_reads_the_buffer_where_its_ray_lies_for_either_sign() {
        let size = UVec2::new(9, 7);
        let camera = Transform::from_xyz(-40_000.0, 500.0, 70_000.0).looking_to(Vec3::new(-0.2, 0.25, -1.0), Vec3::Y);
        let world_from_view = camera.to_matrix();
        let proj = projection(size);
        let clip_from_world = proj * world_from_view.inverse();
        for offset in [Vec2::new(0.3, -0.2), Vec2::new(-0.45, 0.4)] {
            let mut jittered = proj;
            TemporalJitter { offset }.jitter_projection(&mut jittered, size.as_vec2());
            // Camera-relative (rotation only), as the trace's own matrix.
            let mut rotation = world_from_view;
            rotation.w_axis = Vec4::W;
            let world_from_jittered_clip = rotation * jittered.inverse();
            for (x, y) in [(0u32, 0u32), (4, 3), (8, 6)] {
                // The fragment rasterised at pixel (x, y) with the jittered projection: the
                // world point it shows (on the dome, far out).
                let frag = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let ndc = Vec2::new(frag.x / size.x as f32 * 2.0 - 1.0, 1.0 - frag.y / size.y as f32 * 2.0);
                let near = world_from_jittered_clip * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
                let dir = (near.truncate() / near.w).normalize();
                let world = camera.translation + dir * 300_000.0;
                // Through the unjittered matrix it lands at its pixel plus Bevy's signed offset.
                let texel = screen_uv(clip_from_world, world) * size.as_vec2();
                assert!((texel - (frag + offset)).abs().max_element() < 2e-3, "offset {offset}, pixel {x},{y}: {texel}");
            }
        }
    }

    #[test]
    fn the_trace_uniform_stays_finite() {
        let u = trace_uniform(Mat4::from_cols(Vec4::NAN, Vec4::Y, Vec4::Z, Vec4::W), Mat4::ZERO, UVec2::ZERO, u32::MAX);
        assert!(u.world_from_clip.is_finite() && u.origin.is_finite() && u.grid.is_finite());
        assert_eq!(u.grid.truncate().truncate(), Vec2::ONE);
        assert_eq!(u.grid.z, (u32::MAX % 64) as f32);
    }

    /// #3032 stage 2a: the cast ray's spread is the buffer's texel at its size times the tile
    /// side, from the real projection, the wider axis on a non-square texel; it follows the scale
    /// (1..4), the side (1, 2) and the reference's full size, and is always finite and positive.
    #[test]
    fn the_ray_spread_follows_the_buffer_and_the_tile() {
        let render = UVec2::new(1280, 720);
        let proj = projection(render);
        let f = 1.0 / (1.2f32 * 0.5).tan();
        for scale in MIN_SCALE..=MAX_SCALE {
            let size = buffer_size(render, MediumMode::Default, scale);
            for side in [1, 2, 3] {
                let s = ray_spread(proj, size, side);
                let across = 2.0 * (render.x as f32 / render.y as f32) / (f * size.x as f32);
                let up = 2.0 / (f * size.y as f32);
                assert!((s - across.max(up) * side as f32).abs() < 1e-6, "scale {scale}, side {side}: {s}");
                // About the vertical field's tangent over the buffer's rows, per tile.
                assert!((s * size.y as f32 / side as f32 - 2.0 * (0.6f32).tan()).abs() < 0.01 * 2.0 * (0.6f32).tan(), "scale {scale}: {s}");
            }
            assert!(ray_spread(proj, size, 2) > ray_spread(proj, size, 1));
        }
        // A wider buffer has a smaller footprint; the reference (every texel of the render size,
        // side 1) the smallest.
        let reference = ray_spread(proj, buffer_size(render, MediumMode::Reference, 2), 1);
        assert!(reference < ray_spread(proj, buffer_size(render, MediumMode::Default, 2), 2));
        assert!((reference - 2.0 / (f * 720.0)).abs() < 1e-7);
        // A non-square extent (rounded up per axis): the wider of the two.
        let odd = UVec2::new(1281, 721);
        let p = projection(odd);
        let size = buffer_size(odd, MediumMode::Default, 2);
        let across = 2.0 * (odd.x as f32 / odd.y as f32) / (f * size.x as f32);
        let up = 2.0 / (f * size.y as f32);
        assert!(across != up);
        assert!((ray_spread(p, size, 2) - 2.0 * across.max(up)).abs() < 1e-7);
        // A tall extent too.
        let tall = UVec2::new(300, 1000);
        assert!((ray_spread(projection(tall), tall, 1) - 2.0 / (f * 1000.0)).abs() < 1e-6);
        // Nothing to read: still a finite, positive footprint.
        for m in [Mat4::ZERO, Mat4::from_cols(Vec4::NAN, Vec4::NAN, Vec4::Z, Vec4::W)] {
            let s = ray_spread(m, UVec2::ZERO, 0);
            assert!(s.is_finite() && s > 0.0, "{s}");
        }
        // The trace hands it to the source in both entries; prepare_medium writes it.
        let trace = include_str!("medium_trace.wesl").replace("\r\n", "\n");
        for entry in ["medium_trace_refined", "medium_trace_tiles_refined"] {
            let body = trace.split_once(&format!("fn {entry}(")).unwrap().1.split("\n}").next().unwrap();
            let ray = body.lines().find(|line| line.contains("let ray = MediumRay(")).unwrap();
            assert!(ray.ends_with(", medium.grid.w, medium.origin.w);"), "{entry} must pass origin.w as the spread");
        }
        // The prepared uniform carries it (the value, not the source's text).
        let size = buffer_size(render, MediumMode::Default, 3);
        let prepared = prepared_trace_uniform(Mat4::IDENTITY, proj, size, 9, MediumMode::Default, 2);
        assert_eq!(prepared.origin.w, ray_spread(proj, size, 2));
        let reference = prepared_trace_uniform(Mat4::IDENTITY, proj, render, 9, MediumMode::Reference, 2);
        assert_eq!(reference.origin.w, ray_spread(proj, render, 1), "the reference: side 1 at the render size");
        assert!(prepared.origin.truncate() == Vec3::ZERO && prepared.grid.x == size.x as f32);
        // The march module's ray carries it.
        let march = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        assert!(march.contains("    t_max: f32,\n    spread: f32,\n}"));
    }

    /// The trace draws with the light volume's variant whether or not the legacy one is ready, with the
    /// legacy one when the volume did not draw, with nothing when neither can.
    #[test]
    fn the_light_volume_trace_waits_for_no_legacy_pipeline() {
        let volume = ("volume", "its group");
        for (lv, legacy, want) in [
            (Some(&volume), Some(&"legacy"), Some((&"volume", &"its group"))),
            (Some(&volume), None, Some((&"volume", &"its group"))),
            (None, Some(&"legacy"), Some((&"legacy", &"group"))),
            (None, None, None),
        ] {
            assert_eq!(trace_draw(lv, legacy, &"group"), want);
        }
    }

    /// The trace shader with its WESL imports resolved: the fullscreen vertex output declared
    /// in place of the import and the march module pasted in, as plain WGSL.
    /// `views`: the view variant (volumetric_clouds.rs `VIEWS_DEF`).
    fn plain_trace_wgsl(views: bool) -> String {
        let shader = include_str!("medium_trace.wesl").replace("\r\n", "\n");
        let mut out = String::from("struct FullscreenVertexOutput {\n    @builtin(position) position: vec4<f32>,\n    @location(0) uv: vec2<f32>,\n};\n");
        // The march with its one import (the light's leaf, #3032 stage 2e) resolved.
        out.push_str(&crate::volumetric_clouds_lightvol::lv_resolve_variant(&crate::volumetric_clouds_light::plain_march_wgsl(), false, views));
        out.push('\n');
        for line in shader.lines() {
            if line.starts_with("import ") {
                continue;
            }
            assert!(!line.contains("@if"), "a WESL condition this test does not resolve: {line}");
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// The trace's descriptors: the plain one has no shader def, the view variant's one def is `VIEWS_DEF`,
    /// their labels the entry's and the entry's with `_views`; nothing else differs. The callers hand it this
    /// frame's choice, and the light volume's trace adds its own def to it.
    #[test]
    fn the_view_variants_descriptor_has_the_view_def() {
        use bevy::shader::ShaderDefVal;
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default())).init_asset::<Shader>();
        let fullscreen = FullscreenShader::from_world(app.world_mut());
        let layout = BindGroupLayoutDescriptor::new("the test's layout", &[]);
        for mode in [MediumMode::Default, MediumMode::Reference] {
            let (label, entry) = trace_entry(mode);
            let of = |views: bool| trace_pipeline(&layout, Handle::default(), label, entry, &fullscreen, views);
            let defs = |d: &RenderPipelineDescriptor| d.fragment.as_ref().expect("a fragment").shader_defs.clone();
            let (plain, views) = (of(false), of(true));
            assert_eq!(defs(&plain), Vec::<ShaderDefVal>::new(), "{label}");
            assert_eq!(defs(&views), vec![ShaderDefVal::Bool("CLOUD_VIEWS".into(), true)], "{label}");
            assert_eq!(plain.label.as_deref(), Some(label));
            assert_eq!(views.label.as_deref(), Some(format!("{label}_views").as_str()));
            let mut same = views.clone();
            same.label = plain.label.clone();
            same.fragment.as_mut().expect("a fragment").shader_defs.clear();
            assert!(same == plain, "the variants differ in the def and the label alone");
        }
        let medium = include_str!("medium.rs").replace("\r\n", "\n");
        let call = ["trace_pipeline(&pl.trace_layout, shader(), label, ", "entry, &fullscreen, views)"].concat();
        assert_eq!(medium.matches(&call).count(), 2, "both modes hand the frame's choice");
        let volume = include_str!("volumetric_clouds_lightvol.rs").replace("\r\n", "\n");
        let text = "cx.entry.0, cx.entry.1, cx.fullscreen, views);\n            desc.label = Some(format!(\"{}_light_volume{}\", cx.entry.0, variant_suffix(views)).into());\n            if let Some(f) = desc.fragment.as_mut() {\n                f.shader_defs.push(LV_DEF.into());\n            }";
        assert_eq!(volume.matches(text).count(), 1, "the light volume's trace hands it too, and adds its def");
    }

    #[test]
    fn the_trace_shader_is_valid_wgsl() {
        use wgpu::naga;
        for views in [false, true] {
            let source = plain_trace_wgsl(views);
            let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{e:?}"));
            assert_eq!(module.entry_points.len(), 2);
            for name in ["medium_trace_refined", "medium_trace_tiles_refined"] {
                let entry = module.entry_points.iter().find(|e| e.name == name).expect("the entry points the pipelines name");
                assert_eq!(entry.stage, naga::ShaderStage::Fragment);
            }
        }
    }

    #[test]
    fn the_trace_shader_declares_what_the_rust_side_binds() {
        let source = include_str!("medium_trace.wesl").replace("\r\n", "\n");
        let start = source.find("struct MediumTrace {").expect("struct MediumTrace");
        let end = start + source[start..].find("\n}").unwrap();
        let fields: Vec<&str> = source[start..end].lines().skip(1).map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")).collect();
        assert_eq!(fields, ["world_from_clip: mat4x4<f32>,", "origin: vec4<f32>,", "grid: vec4<f32>,", "tiles: vec4<f32>,"]);
        assert_eq!(MediumTraceUniform::min_size().get(), 64 + 3 * 16);
        // The layout's order (trace_pipeline): the trace's uniform, the march's, the base noise,
        // its sampler, the erosion noise (#3032 stage 2b; no 2D texture).
        for binding in [
            "@group(0) @binding(0) var<uniform> medium: MediumTrace;",
            "@group(0) @binding(1) var<uniform> volume: CloudVolume;",
            "@group(0) @binding(2) var noise_base: texture_3d<f32>;",
            "@group(0) @binding(3) var noise_sampler: sampler;",
            "@group(0) @binding(4) var noise_erosion: texture_3d<f32>;",
        ] {
            assert!(source.contains(binding), "{binding}");
        }
        assert!(!source.contains("@binding(5)") && !source.contains("texture_2d"), "the trace binds the two noise textures and nothing else");
        // Two targets: the light (FAR_FORMAT, vec4) and the distance (DISTANCE_FORMAT, f32).
        assert!(source.contains("@location(0) light: vec4<f32>,") && source.contains("@location(1) hit_distance: f32,"));
        assert!(source.contains("import nvforever::volumetric_clouds_march::"), "the march module is imported by its path");
        // The entry of each mode, through the cloud source's march.
        for (mode, label, entry) in [
            (MediumMode::Reference, "medium_trace_refined_pipeline", "medium_trace_refined"),
            (MediumMode::Default, "medium_trace_tiles_refined_pipeline", "medium_trace_tiles_refined"),
        ] {
            assert_eq!(trace_entry(mode), (label, entry));
            assert!(source.contains(&format!("fn {entry}(in: FullscreenVertexOutput) -> TraceOutput")), "{entry}");
            let start = source.find(&format!("fn {entry}(")).expect("the entry");
            let body = &source[start..start + source[start..].find("\n}\n").expect("its end")];
            assert!(body.contains("s = vc_trace_refined(ray, "), "{entry}: the march");
        }
    }

    #[test]
    fn the_upsample_module_is_valid_wgsl() {
        use wgpu::naga;
        let source = format!(
            "{}\n@group(0) @binding(0) var far_light: texture_2d<f32>;\n@group(0) @binding(1) var far_sampler: sampler;\n@fragment\nfn test_entry(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{\n    let uv = medium_screen_uv(mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0)), p.xyz);\n    return medium_far(far_light, far_sampler, uv);\n}}\n",
            include_str!("medium_upsample.wesl").replace("\r\n", "\n")
        );
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    }

    /// An empty ray and a half-faded one, as the composition reads them: a miss adds nothing
    /// (alpha 0, no colour); a stored transmittance T gives alpha 1 - T.
    #[test]
    fn the_stored_values_compose_as_the_prototype_blended() {
        // medium_upsample.wesl `medium_far`: no light where nothing is hidden.
        let compose = |rgb: Vec3, t: f32| {
            let alpha = (1.0 - t).clamp(0.0, 1.0);
            if alpha <= 0.0 { (Vec3::ZERO, 0.0) } else { (rgb.max(Vec3::ZERO), alpha) }
        };
        assert_eq!(compose(Vec3::ZERO, CLEAR_LIGHT.a as f32), (Vec3::ZERO, 0.0));
        // A bright thin edge whose transmittance rounds to one in half floats adds nothing.
        let thin_t = half::f16::from_f32(1.0 - 0.0001).to_f32();
        assert_eq!(thin_t, 1.0, "rounds to one");
        assert_eq!(compose(Vec3::splat(0.5), thin_t), (Vec3::ZERO, 0.0));
        let src = include_str!("medium_upsample.wesl").replace("\r\n", "\n");
        assert!(src.contains("if alpha <= 0.0 {\n        return vec4<f32>(0.0);"));
        // The march's (colour x fade, (1 - trans) x fade) at trans 0.2, fade 0.5.
        let (trans, fade) = (0.2f32, 0.5f32);
        let stored_t = 1.0 - (1.0 - trans) * fade;
        let (_, alpha) = compose(Vec3::splat(0.3), stored_t);
        assert!((alpha - (1.0 - trans) * fade).abs() < 1e-6);
        assert_eq!(CLEAR_DISTANCE.r as f32, EMPTY_DISTANCE);
    }

    // --- Checkpoint B: the default mode ---

    #[test]
    fn the_buffer_sizes_round_up_at_odd_sizes() {
        // Stage 1's parameters (half, 2x2), set explicitly.
        let (d, side) = (2u32, 2u32);
        assert_eq!(buffer_size(UVec2::new(2560, 1440), MediumMode::Default, d), UVec2::new(1280, 720));
        assert_eq!(buffer_size(UVec2::new(1279, 721), MediumMode::Default, d), UVec2::new(640, 361));
        assert_eq!(buffer_size(UVec2::new(1, 1), MediumMode::Default, d), UVec2::ONE);
        assert_eq!(buffer_size(UVec2::ZERO, MediumMode::Default, d), UVec2::ONE);
        assert_eq!(buffer_size(UVec2::new(1281, 721), MediumMode::Reference, d), UVec2::new(1281, 721));
        assert_eq!(trace_size(UVec2::new(1280, 720), side), UVec2::new(640, 360));
        assert_eq!(trace_size(UVec2::new(641, 361), side), UVec2::new(321, 181));
        assert_eq!(trace_size(UVec2::ONE, side), UVec2::ONE);
        // The memory at 2560x1440: the published image, two histories and the trace set.
        let half = buffer_size(UVec2::new(2560, 1440), MediumMode::Default, d);
        assert!((medium_mib(half, MediumMode::Default, side) - 37.79).abs() < 0.01, "{}", medium_mib(half, MediumMode::Default, side));
        assert!(describe_buffer(half, MediumMode::Default, d, side).starts_with("1280x720 (half the main pass's render size), one ray in four a frame into 640x360"));
        assert!(describe_buffer(UVec2::new(7, 5), MediumMode::Reference, d, side).starts_with("7x5 (the main pass's render size), every pixel traced each frame (reference)"));
        // High: two fifths, one ray per 3x3 tile (the look accepted on 2026-11-23).
        assert_eq!((BUFFER_SCALE, TILE_SIDE), (2.5, 3));
        let size = buffer_size(UVec2::new(1280, 720), MediumMode::Default, BUFFER_SCALE);
        assert!(describe_buffer(size, MediumMode::Default, BUFFER_SCALE, TILE_SIDE).starts_with("512x288 (two fifths of the main pass's render size), one ray in nine a frame into 171x96"));
        let size = buffer_size(UVec2::new(853, 480), MediumMode::Default, BUFFER_SCALE);
        assert_eq!((size, trace_size(size, TILE_SIDE)), (UVec2::new(342, 192), UVec2::new(114, 64)), "FSR Quality at 1280x720");
        // The row's levels: their scales' sizes, and the log's words for each.
        for (level, render, words) in [
            (CloudLevel::High, UVec2::new(1280, 720), "512x288 (two fifths of the main pass's render size), one ray in nine a frame into 171x96, "),
            (CloudLevel::Normal, UVec2::new(1280, 720), "427x240 (a third of the main pass's render size), one ray in nine a frame into 143x80, "),
            (CloudLevel::Performance, UVec2::new(1280, 720), "320x180 (a quarter of the main pass's render size), one ray in nine a frame into 107x60, "),
            (CloudLevel::Performance, UVec2::new(853, 480), "214x120 (a quarter of the main pass's render size), one ray in nine a frame into 72x40, "),
        ] {
            let scale = level.buffer_scale();
            let line = describe_buffer(buffer_size(render, MediumMode::Default, scale), MediumMode::Default, scale, TILE_SIDE);
            assert!(line.starts_with(words), "{line}");
        }
    }

    /// A live change of the row's level is a change of the size the render world wants for the same
    /// epoch, as a render size change the view's key does not show: one hand-back, one new image, drawn
    /// from its first whole write; a quick change and back settles on the last.
    #[test]
    fn a_level_change_makes_the_image_again_once() {
        let render = UVec2::new(1280, 720);
        let size = |l: CloudLevel| buffer_size(render, MediumMode::Default, l.buffer_scale());
        let mut sim = Sim::new();
        for _ in 0..4 {
            sim.frame(true, true, key(1280, 720), size(CloudLevel::Normal), true);
        }
        assert!(sim.frame(true, true, key(1280, 720), size(CloudLevel::Normal), true).ready);
        for level in [CloudLevel::High, CloudLevel::Performance, CloudLevel::Normal] {
            let plans: Vec<Plan> = (0..8).map(|_| sim.frame(true, true, key(1280, 720), size(level), true)).collect();
            assert_eq!(plans.iter().filter_map(|p| p.alloc).collect::<Vec<_>>(), [size(level)], "{level:?}: one new image");
            assert!(plans.iter().all(|p| !p.release));
            assert!(plans.iter().skip(2).all(|p| p.ready), "{level:?}: drawn again in the third frame, and from then on");
            assert_eq!(plans.iter().find_map(|p| p.why), Some(HandBack::NewImage), "{level:?}");
        }
        sim.frame(true, true, key(1280, 720), size(CloudLevel::High), true);
        let plans: Vec<Plan> = (0..8).map(|_| sim.frame(true, true, key(1280, 720), size(CloudLevel::Normal), true)).collect();
        assert!(plans.last().is_some_and(|p| p.ready));
        assert_eq!(sim.ledger.image, Some(size(CloudLevel::Normal)));
    }

    /// #3032 stage 2a, step 0: the buffer is the render size over the scale, rounded up, at least
    /// one texel, for every scale; the reference ignores the scale; the trace target is one texel
    /// per tile at either side; the log names both.
    #[test]
    fn the_scale_and_the_tile_side_size_the_targets() {
        let ceil = |a: u32, b: u32| a.div_ceil(b);
        for scale in MIN_SCALE..=MAX_SCALE {
            for render in [UVec2::new(2560, 1440), UVec2::new(1279, 721), UVec2::new(853, 480), UVec2::new(3, 2), UVec2::ONE] {
                let b = buffer_size(render, MediumMode::Default, scale);
                assert_eq!(b, UVec2::new(ceil(render.x, scale), ceil(render.y, scale)), "{render} at {scale}");
                assert_eq!(buffer_size(render, MediumMode::Reference, scale), render);
                for side in [1, 2, 3] {
                    let t = trace_size(b, side);
                    assert_eq!(t, UVec2::new(ceil(b.x, side), ceil(b.y, side)));
                    // Every texel of the buffer lies in a tile, and every tile holds a texel.
                    assert!(t.x * side >= b.x && (t.x - 1) * side < b.x && t.y * side >= b.y && (t.y - 1) * side < b.y);
                }
            }
            assert_eq!(buffer_size(UVec2::ZERO, MediumMode::Default, scale), UVec2::ONE);
        }
        // Out of range: held.
        assert_eq!(buffer_size(UVec2::new(100, 100), MediumMode::Default, 0), UVec2::new(100, 100));
        assert_eq!(buffer_size(UVec2::new(100, 100), MediumMode::Default, 9), UVec2::new(25, 25));
        assert_eq!(trace_size(UVec2::new(7, 5), 1), UVec2::new(7, 5));
        assert_eq!(trace_size(UVec2::new(7, 5), 0), UVec2::new(7, 5), "a side below 1 is 1");
        assert_eq!(trace_size(UVec2::new(7, 5), 4), UVec2::new(3, 2), "a side above 3 is held to 3");
        // The memory at side 1: the trace targets at the buffer's own size.
        let b = UVec2::new(1280, 720);
        let texels = (b.x * b.y) as f32 / (1024.0 * 1024.0);
        assert!((medium_mib(b, MediumMode::Default, 1) - texels * (8.0 + 32.0 + 12.0)).abs() < 1e-3);
        assert!(medium_mib(b, MediumMode::Default, 1) > medium_mib(b, MediumMode::Default, 2));
        // The log line names the scale and the tile in words.
        let line = |scale, side| describe_buffer(buffer_size(UVec2::new(1280, 720), MediumMode::Default, scale), MediumMode::Default, scale, side);
        assert!(line(1, 2).starts_with("1280x720 (the main pass's render size), one ray in four a frame into 640x360, reconstructed with history"));
        assert!(line(3, 2).starts_with("427x240 (a third of the main pass's render size), one ray in four a frame into 214x120"));
        assert!(line(4, 2).starts_with("320x180 (a quarter of the main pass's render size), one ray in four a frame into 160x90"));
        assert!(line(2, 1).starts_with("640x360 (half the main pass's render size), one ray per texel a frame into 640x360, reconstructed with history"));
        assert_eq!((tile_words(2), tile_words(TILE_SIDE)), ("one ray in four", "one ray in nine"));
    }

    #[test]
    fn two_fifths_sizes_the_real_extent_and_logs_the_drawn_trace() {
        let divisor = debug_scale(Some(" 2.50 ")).expect("read");
        let render = UVec2::new(1920, 1080);
        let buf = buffer_size(render, MediumMode::Default, divisor);
        assert_eq!(buf, UVec2::new(768, 432));
        assert_eq!(trace_size(buf, 3), UVec2::new(256, 144));
        assert!(describe_buffer(buf, MediumMode::Default, divisor, 3)
            .starts_with("768x432 (two fifths of the main pass's render size), one ray in nine a frame into 256x144"));
        assert_eq!(buffer_size(UVec2::new(1281, 721), MediumMode::Default, divisor), UVec2::new(513, 289));
        assert_eq!(buffer_size(UVec2::ZERO, MediumMode::Default, divisor), UVec2::ONE);
        assert_eq!(buffer_size(render, MediumMode::Reference, divisor), render);
        for text in ["NaN", "inf", "-inf", "2.75"] {
            assert_eq!(debug_scale(Some(text)), None);
        }
        assert_eq!(buffer_size(render, MediumMode::Default, f64::NAN), buffer_size(render, MediumMode::Default, BUFFER_SCALE));
    }

    #[test]
    fn every_texel_of_a_tile_is_cast_once_in_four_frames_and_neighbours_differ() {
        let size = UVec2::new(9, 7);
        let tiles = trace_size(size, 2);
        for phase0 in [0u32, 5, u32::MAX - 1] {
            let mut cast = vec![0u32; (size.x * size.y) as usize];
            for f in 0..4u32 {
                let phase = phase0.wrapping_add(f);
                for ty in 0..tiles.y {
                    for tx in 0..tiles.x {
                        let tile = UVec2::new(tx, ty);
                        let c = corner(tile, size, phase, 2);
                        let texel = 2 * tile + c;
                        assert!(texel.x < size.x && texel.y < size.y, "a corner inside the buffer");
                        cast[(texel.y * size.x + texel.x) as usize] += 1;
                        // Edge-adjacent full tiles never cast the same corner in a frame.
                        let right = tile + UVec2::X;
                        if 2 * right.x + 1 < size.x && 2 * tile.y + 1 < size.y {
                            assert_ne!(corner(right, size, phase, 2), c, "tiles {tile} and {right} at phase {phase}");
                        }
                    }
                }
            }
            for y in 0..size.y {
                for x in 0..size.x {
                    let n = cast[(y * size.x + x) as usize];
                    if x == size.x - 1 || y == size.y - 1 {
                        // A partial tile cycles through the corners it has: each at least once.
                        assert!(n >= 1, "edge texel {x},{y} cast {n} times");
                    } else {
                        assert_eq!(n, 1, "texel {x},{y} cast {n} times in four frames");
                    }
                }
            }
        }
        // A 1x1 buffer casts its one texel every frame.
        for phase in 0..4 {
            assert_eq!(corner(UVec2::ZERO, UVec2::ONE, phase, 2), UVec2::ZERO);
        }
    }

    /// Stage 1's corner and offset, as they were before the tile side became a parameter
    /// (a2a2481ce), for the bit-identity test below.
    fn stage1_corner(tile: UVec2, size: UVec2, phase: u32) -> UVec2 {
        const ORDER: [UVec2; 4] = [UVec2::new(0, 0), UVec2::new(1, 0), UVec2::new(1, 1), UVec2::new(0, 1)];
        let w = size.x.saturating_sub(2 * tile.x).clamp(1, 2);
        let h = size.y.saturating_sub(2 * tile.y).clamp(1, 2);
        let k = (phase.wrapping_add((tile.x + tile.y) & 1)) & 3;
        let have: Vec<UVec2> = ORDER.into_iter().filter(|c| c.x < w && c.y < h).collect();
        have[k as usize % have.len()]
    }

    fn stage1_offset(texel: UVec2, phase: u32, offsets: RayOffsets) -> f32 {
        let p = texel.as_vec2();
        let turn = 0.618_034 * ((phase / 4) % 64) as f32;
        match offsets {
            RayOffsets::Blue => ((52.982_918 * (p.dot(Vec2::new(0.067_110_56, 0.005_837_15))).fract()).fract() + turn).fract(),
            RayOffsets::Fixed => (0.5 + p.x * 0.754_877_7 + p.y * 0.569_840_3).fract(),
            RayOffsets::Turning => ((0.5 + p.x * 0.754_877_7 + p.y * 0.569_840_3).fract() + turn).fract(),
            RayOffsets::Centre => 0.5,
        }
    }

    /// #3032 stage 2a, step 0: at scale 2 and side 2 (stage 1's, the defaults until 2026-11-23) the sizes, every corner and every
    /// start offset are stage 1's to the bit, edges and the phase's wrap included.
    #[test]
    fn the_half_size_2x2_pattern_is_stage_ones_to_the_bit() {
        for render in [UVec2::new(2560, 1440), UVec2::new(1920, 1080), UVec2::new(1279, 721), UVec2::new(853, 480), UVec2::new(3, 1), UVec2::ONE] {
            let old = (render + UVec2::ONE) / 2;
            assert_eq!(buffer_size(render, MediumMode::Default, 2), old);
            assert_eq!(trace_size(old, 2), (old + UVec2::ONE) / 2);
        }
        for size in [UVec2::new(9, 7), UVec2::new(8, 8), UVec2::new(1, 5), UVec2::new(2, 1), UVec2::ONE] {
            let tiles = trace_size(size, 2);
            for phase in (0..12u32).chain([255, 256, 257, u32::MAX - 2, u32::MAX]) {
                for ty in 0..tiles.y {
                    for tx in 0..tiles.x {
                        let tile = UVec2::new(tx, ty);
                        assert_eq!(corner(tile, size, phase, 2), stage1_corner(tile, size, phase), "{tile} of {size} at {phase}");
                    }
                }
            }
        }
        for texel in [UVec2::ZERO, UVec2::new(3, 7), UVec2::new(639, 359), UVec2::new(1279, 719)] {
            for phase in (0..300u32).step_by(7).chain([u32::MAX]) {
                for mode in [RayOffsets::Turning, RayOffsets::Fixed, RayOffsets::Blue] {
                    assert_eq!(ray_offset(texel, phase, mode, 2).to_bits(), stage1_offset(texel, phase, mode).to_bits());
                }
            }
        }
    }

    /// #3032 stage 2a, step 0: with tiles of side 1 every texel is cast every frame (its own
    /// corner), its start offset turns at every cast, and it is its own nearest cast ray.
    #[test]
    fn a_tile_of_one_texel_casts_every_texel_every_frame() {
        let size = UVec2::new(9, 7);
        assert_eq!(trace_size(size, 1), size);
        for phase in [0u32, 1, 2, 3, 77, u32::MAX] {
            for y in 0..size.y {
                for x in 0..size.x {
                    let t = UVec2::new(x, y);
                    assert_eq!(corner(t, size, phase, 1), UVec2::ZERO);
                    assert_eq!(nearest_cast_side(t, size, phase, 1), t);
                }
            }
        }
        for texel in [UVec2::ZERO, UVec2::new(5, 2)] {
            for phase in 0..63u32 {
                let (a, b) = (ray_offset(texel, phase, RayOffsets::Turning, 1), ray_offset(texel, phase + 1, RayOffsets::Turning, 1));
                assert!((a - b).abs() > 1e-3, "side 1 turns at every cast");
            }
            // The turn's period is 64 casts; the upload's modulus (256) keeps it.
            for phase in [3u32, 64, 1_000_003, u32::MAX - 7] {
                assert_eq!(ray_offset(texel, phase, RayOffsets::Turning, 1), ray_offset(texel, phase_upload(phase, 1) as u32, RayOffsets::Turning, 1));
                assert_eq!(ray_offset(texel, phase, RayOffsets::Turning, 1), ray_offset(texel, phase.wrapping_add(64), RayOffsets::Turning, 1));
            }
            assert_eq!(ray_offset(texel, 0, RayOffsets::Fixed, 1), ray_offset(texel, 9, RayOffsets::Fixed, 1));
        }
        // The shaders take the side from the uniforms and pass the cycle to the offset.
        let trace = include_str!("medium_trace.wesl").replace("\r\n", "\n");
        assert!(trace.contains("let side = clamp(u32(medium.tiles.z), 1u, 3u);"));
        assert!(trace.contains("let texel = side * tile + medium_corner(tile, size, phase, side);"));
        assert!(trace.contains("medium_offset(texel, phase, u32(medium.tiles.y), side * side)"));
        let resolve = include_str!("medium_resolve.wesl").replace("\r\n", "\n");
        assert!(resolve.contains("let side = clamp(u32(resolve.pattern.x), 1u, 3u);"));
        assert!(resolve.contains("let tiles = (size + vec2<u32>(side - 1u)) / vec2<u32>(side);"));
        assert!(resolve.contains("let tile = texel / vec2<u32>(side);"));
        assert!(resolve.contains("let fresh = all(texel == side * tile + medium_corner(tile, size, phase, side));"));
        assert!(resolve.contains("let cast_at = vec2<i32>(side * vec2<u32>(t) + medium_corner(vec2<u32>(t), size, phase, side));"));
        assert!(!resolve.contains("2u * tile") && !trace.contains("2u * tile"), "no tile side written as a literal");
    }

    #[test]
    fn nine_phase_tiles_cover_every_texel_and_seed_from_the_nearest_cast() {
        let render = UVec2::new(1920, 1080);
        assert_eq!(trace_size(buffer_size(render, MediumMode::Default, 2), 3), UVec2::new(320, 180));
        assert_eq!(trace_size(buffer_size(render, MediumMode::Default, 3), 3), UVec2::new(214, 120));
        assert_eq!(tile_words(3), "one ray in nine");

        // Includes full 3x3 tiles, every right/bottom remainder, and buffers smaller than one tile.
        for width in 1..=8 {
            for height in 1..=8 {
                let size = UVec2::new(width, height);
                let tiles = trace_size(size, 3);
                for ty in 0..tiles.y {
                    for tx in 0..tiles.x {
                        let tile = UVec2::new(tx, ty);
                        let remaining = (size - tile * 3).min(UVec2::splat(3));
                        let cycle = remaining.x * remaining.y;
                        let mut visited = [false; 9];
                        for phase in 0..cycle {
                            let c = corner(tile, size, phase, 3);
                            assert!(c.cmplt(remaining).all());
                            let index = (c.x + c.y * 3) as usize;
                            assert!(!visited[index]);
                            visited[index] = true;
                            assert_eq!(corner(tile, size, phase + cycle, 3), c);
                            assert_eq!(nearest_cast_side(tile * 3 + c, size, phase, 3), tile * 3 + c);
                        }
                        let phase = 1_000_003;
                        assert_eq!(corner(tile, size, phase, 3), corner(tile, size, phase_upload(phase, 3) as u32, 3));
                    }
                }
            }
        }

        let full = UVec2::splat(12);
        for group in [0, 3, 6] {
            let three = [0, 1, 2].map(|i| corner(UVec2::new(1, 2), full, group + i, 3));
            assert!(three[0].x != three[1].x && three[1].x != three[2].x && three[0].x != three[2].x);
            assert!(three[0].y != three[1].y && three[1].y != three[2].y && three[0].y != three[2].y);
        }
        let size = UVec2::new(7, 8);
        let tiles = trace_size(size, 3);
        for phase in 0..9 {
            let casts: Vec<_> = (0..tiles.y).flat_map(|y| (0..tiles.x).map(move |x| UVec2::new(x, y))).map(|t| t * 3 + corner(t, size, phase, 3)).collect();
            for y in 0..size.y {
                for x in 0..size.x {
                    let p = UVec2::new(x, y);
                    let dist = |c: UVec2| (c.as_ivec2() - p.as_ivec2()).length_squared();
                    assert_eq!(dist(nearest_cast_side(p, size, phase, 3)), casts.iter().map(|&c| dist(c)).min().unwrap());
                }
            }
        }
        assert_eq!(unpack_aux(pack_aux(5.0, 8.0)), (5.0, 8.0));
    }

    #[test]
    fn the_cast_pattern_is_the_same_text_in_both_shaders() {
        let take = |src: &str| {
            let src = src.replace("\r\n", "\n");
            let start = src.find("fn medium_corner(").expect("medium_corner");
            let end = start + src[start..].find("\n}\n").expect("end");
            src[start..end].to_string()
        };
        assert_eq!(take(include_str!("medium_trace.wesl")), take(include_str!("medium_resolve.wesl")));
        // And the CPU mirror's order and checkerboard.
        let src = include_str!("medium_trace.wesl").replace("\r\n", "\n");
        assert!(src.contains("let k = (phase + ((tile.x + tile.y) & 1u)) & 3u;"));
        assert!(src.contains("var order = array<vec2<u32>, 4>(vec2<u32>(0u, 0u), vec2<u32>(1u, 0u), vec2<u32>(1u, 1u), vec2<u32>(0u, 1u));"));
    }

    #[test]
    fn the_fresh_share_has_its_floor_and_reaches_one_on_a_brisk_turn() {
        assert_eq!(fresh_share(FRESH_SHARE, 0.0, 1.0), FRESH_SHARE);
        assert_eq!(fresh_share(FRESH_SHARE, MOTION_FROM, 1.0), FRESH_SHARE);
        assert_eq!(fresh_share(FRESH_SHARE, MOTION_FULL, 1.0), 1.0);
        assert_eq!(fresh_share(0.0, f32::NAN, 1.0), 1.0);
        assert_eq!(fresh_share(2.0, 0.0, 1.0), 1.0);
        // NVF_PLAY_DEBUG_MEDIUM_FRESH_MOVING: the ramp rises to that share; the floor still holds.
        assert!((fresh_share(0.1, MOTION_FULL, 0.1) - 0.1).abs() < 1e-6, "a moving view keeps accumulating");
        assert!((fresh_share(0.1, MOTION_FULL, 0.3) - 0.3).abs() < 1e-6);
        assert_eq!(fresh_share(0.5, MOTION_FULL, 0.2), 0.5, "never below the floor");
        assert_eq!(fresh_share(0.1, MOTION_FULL, f32::NAN), 1.0, "unreadable: as by default");
        assert_eq!(debug_fresh_moving(None), FRESH_MOVING_SHARE);
        assert_eq!(debug_fresh_moving(Some(" 0.3 ")), 0.3);
        assert_eq!(debug_fresh_moving(Some("7")), 1.0);
        assert_eq!(debug_fresh_moving(Some("x")), FRESH_MOVING_SHARE);
        // The defaults: 0.1 still and moving.
        assert_eq!((FRESH_SHARE, FRESH_MOVING_SHARE), (0.1, 0.1));
        for m in [0.0, MOTION_FROM, 2.5, MOTION_FULL, 50.0] {
            assert!((fresh_share(FRESH_SHARE, m, FRESH_MOVING_SHARE) - 0.1).abs() < 1e-6, "0.1 at {m} texels a frame");
        }
        let mut last = 0.0;
        for i in 0..50 {
            let s = fresh_share(0.2, i as f32 * 0.1, 1.0);
            assert!(s + 1e-6 >= last && (0.0..=1.0).contains(&s));
            last = s;
        }
        let src = include_str!("medium_resolve.wesl").replace("\r\n", "\n");
        assert!(src.contains("let floor_share = max(clamp(resolve.grid.w, 0.0, 1.0), smoothstep(resolve.params.y, resolve.params.z, motion) * clamp(resolve.params.w, 0.0, 1.0));"), "the shader's share is the CPU's");
    }

    /// NVF_PLAY_DEBUG_MEDIUM_CLAMP: the variance clip leaves a history inside the band alone,
    /// puts one outside on the band's edge, keeps to what the format holds, and with nine equal
    /// rays gives the box's own result; `off` returns the history; the box is the default.
    #[test]
    fn the_variance_clip_holds_the_history_to_the_band() {
        assert_eq!(debug_clamp(None), HistoryClamp::Moving);
        assert_eq!(debug_clamp(Some("box")), HistoryClamp::Box);
        assert_eq!(debug_clamp(Some("Variance")), HistoryClamp::Variance);
        assert_eq!(debug_clamp(Some("off")), HistoryClamp::Off);
        assert_eq!(debug_clamp(Some("x")), HistoryClamp::Moving);
        assert_eq!(debug_clamp_k(None), CLAMP_K);
        assert_eq!(debug_clamp_k(Some("3")), 3.0);
        assert_eq!(debug_clamp_k(Some("100")), 8.0);
        assert_eq!(debug_clamp_k(Some("0")), 0.5);
        // Rays alternating between two values: mean 0.5 (ish), a spread of about 0.5.
        let mut rays = [Vec4::new(0.2, 0.2, 0.2, 0.6); 9];
        for r in rays.iter_mut().step_by(2) {
            *r = Vec4::new(0.8, 0.8, 0.8, 0.2);
        }
        let mean = rays.iter().copied().sum::<Vec4>() / 9.0;
        let sd = (rays.iter().map(|&r| (r - mean) * (r - mean)).sum::<Vec4>() / 9.0).powf(0.5);
        // Equal weights: the plain mean and spread.
        let clip_history = |h: Vec4, rays: &[Vec4; 9], clamp: HistoryClamp, k: f32, motion: f32| super::clip_history(h, rays, &[1.0; 9], clamp, k, motion);
        let inside = mean + 0.5 * sd;
        assert!((clip_history(inside, &rays, HistoryClamp::Variance, 1.25, 0.0) - inside).abs().max_element() < 1e-6, "inside: untouched");
        let far = Vec4::new(5.0, 5.0, 5.0, 0.95);
        let clipped = clip_history(far, &rays, HistoryClamp::Variance, 1.25, 0.0);
        let edge = mean + 1.25 * sd;
        assert!((clipped.x - edge.x).abs() < 1e-5 && (clipped.w - edge.w).abs() < 1e-5, "outside: on the band's edge {clipped} {edge}");
        // Kept to what the format holds: no negative light, transmittance in 0..1.
        let wide = clip_history(Vec4::new(-3.0, -3.0, -3.0, 5.0), &rays, HistoryClamp::Variance, 8.0, 0.0);
        assert!(wide.truncate().min_element() >= 0.0 && (0.0..=1.0).contains(&wide.w));
        // Nine equal rays: no spread, the box's own result.
        let equal = [Vec4::new(0.3, 0.4, 0.5, 0.7); 9];
        for h in [Vec4::ZERO, Vec4::new(1.0, 0.1, 0.9, 0.2), Vec4::splat(0.35)] {
            let (v, b) = (clip_history(h, &equal, HistoryClamp::Variance, 1.25, 0.0), clip_history(h, &equal, HistoryClamp::Box, 1.25, 0.0));
            assert!((v - b).abs().max_element() < 1e-4, "{v} {b}");
        }
        assert_eq!(clip_history(far, &rays, HistoryClamp::Off, 1.25, 0.0), far);
        assert_eq!(clip_history(far, &rays, HistoryClamp::Box, 1.25, 0.0), Vec4::new(0.8, 0.8, 0.8, 0.6));
        // The default, `moving`: the history as reprojected while still or slow, the variance
        // clip at full motion, a blend between.
        assert_eq!(clip_history(far, &rays, HistoryClamp::Moving, 1.25, 0.0), far, "still: untouched");
        assert_eq!(clip_history(far, &rays, HistoryClamp::Moving, 1.25, MOTION_FROM), far);
        let full = clip_history(far, &rays, HistoryClamp::Moving, 1.25, MOTION_FULL);
        assert!((full - clipped).abs().max_element() < 1e-6, "full motion: the variance clip");
        let half = clip_history(far, &rays, HistoryClamp::Moving, 1.25, 0.5 * (MOTION_FROM + MOTION_FULL));
        assert!((half - far.lerp(clipped, 0.5)).abs().max_element() < 1e-5, "halfway: an even blend");
        // A speck the nine rays all missed is kept while still, pulled to the rays in motion.
        let empty = [Vec4::new(0.0, 0.0, 0.0, 1.0); 9];
        let speck = Vec4::new(0.4, 0.4, 0.4, 0.5);
        assert_eq!(clip_history(speck, &empty, HistoryClamp::Moving, 1.25, 0.0), speck);
        assert_eq!(clip_history(speck, &empty, HistoryClamp::Moving, 1.25, MOTION_FULL), Vec4::new(0.0, 0.0, 0.0, 1.0));
        assert_eq!(HistoryClamp::Moving.code(), 3.0);
        let src = include_str!("medium_resolve.wesl").replace("\r\n", "\n");
        assert!(src.contains("hist = medium_clip(h, lo, hi, mean, sd, ramp, resolve.clamp_mode);"));
        assert!(src.contains("let c = clamp(h, mean - kk * sd, mean + kk * sd);"));
        assert!(src.contains("return mix(h, medium_variance_clip(h, mean, sd, mode.y), clamp(ramp, 0.0, 1.0));"));
        assert!(src.contains("let d = rays[i] - mean;"), "the spread from the deviations");
        assert!(src.contains("spread += ray_w[i] * d * d;") && src.contains("let sd = sqrt(spread / weight_sum);"), "under the mean's weights");
        // Weighed: the band lies nearer the ray that weighs most than under equal weights; the box,
        // no clamp and a still view do not look at the weights.
        let mut near_w = [0.1f32; 9];
        near_w[0] = 1.0;
        let dense = Vec4::new(0.8, 0.8, 0.8, 0.2);
        let mut edge_rays = empty;
        edge_rays[0] = dense;
        let near = super::clip_history(dense, &edge_rays, &near_w, HistoryClamp::Variance, 1.25, 0.0);
        let plain = clip_history(dense, &edge_rays, HistoryClamp::Variance, 1.25, 0.0);
        assert!((near - dense).abs().max_element() < 1e-6 && plain.x < 0.5, "{near} {plain}");
        for (clamp, motion) in [(HistoryClamp::Box, 9.0), (HistoryClamp::Off, 9.0), (HistoryClamp::Moving, 0.0), (HistoryClamp::Moving, MOTION_FROM)] {
            assert_eq!(super::clip_history(far, &edge_rays, &near_w, clamp, 1.25, motion), clip_history(far, &edge_rays, clamp, 1.25, motion), "{clamp:?}");
        }
    }

    /// A young history takes a running mean of its own casts, an old one the floor.
    #[test]
    fn a_young_history_takes_a_running_mean_of_its_own_casts() {
        // The bookkeeping the resolve shader does on a fresh cast with valid history.
        let cast = |hist: f32, count: f32, own: f32| {
            let share = counted_share(FRESH_SHARE, count);
            let next = hist + (own - hist) * share;
            (next, (count + 1.0).min(COUNT_CAP))
        };
        // After a cut a texel is filled from the rays around (count 0); the fill stands in for one
        // cast: the first own cast averages with it.
        let (v, c) = cast(0.9, 0.0, 0.3);
        assert!((v - 0.6).abs() < 1e-6 && c == 1.0, "{v} {c}");
        // A texel its own cast began (count 1, no fill): ten casts give the mean of the ten, each
        // weighted the same, within rounding.
        let rays = [0.3f32, 0.5, 0.1, 0.7, 0.2, 0.6, 0.4, 0.8, 0.0, 0.9];
        let (mut v, mut c) = (rays[0], 1.0);
        for &r in &rays[1..] {
            (v, c) = cast(v, c, r);
        }
        let mean = rays.iter().sum::<f32>() / 10.0;
        assert!((v - mean).abs() < 1e-5 && c == 10.0, "{v} against {mean}");
        // Over a fill of 0.9 the fill and the first cast weigh as one.
        let (mut v, mut c) = (0.9, 0.0);
        for r in rays {
            (v, c) = cast(v, c, r);
        }
        let want = mean + (0.9 - rays[0]) / 20.0;
        assert!((v - want).abs() < 1e-5 && c == 10.0, "{v} against {want}");
        // What is left of the fill after n casts: 1 / (2n) to the tenth, then a tenth less a cast.
        let (mut left, mut c) = (1.0, 0.0);
        for n in 1..=60 {
            (left, c) = cast(left, c, 0.0);
            let want = if n <= 10 { 0.5 / n as f32 } else { 0.05 * 0.9f32.powi(n - 10) };
            assert!((left - want).abs() < 1e-5, "{left} of the fill after {n} casts, {want} expected");
        }
        // With a cast of its own behind it the share is as before the fill counted.
        for n in 1..=COUNT_CAP as u32 {
            assert_eq!(counted_share(FRESH_SHARE, n as f32), FRESH_SHARE.max(1.0 / (n as f32 + 1.0)));
        }
        assert_eq!(counted_share(FRESH_SHARE, 0.0), 0.5);
        // A long-lived texel takes the floor: a speck with a long count is as steady as before.
        assert_eq!(counted_share(FRESH_SHARE, 9.0), FRESH_SHARE);
        assert_eq!(counted_share(FRESH_SHARE, COUNT_CAP), FRESH_SHARE);
        assert_eq!(counted_share(0.5, 3.0), 0.5, "the moving share still a floor");
        // The count and the age pack exactly and come back.
        for (count, age) in [(0.0, 0.0), (1.0, 3.0), (9.0, 15.0), (COUNT_CAP, AGE_CAP), (1000.0, 99.0)] {
            let (c, a) = unpack_aux(pack_aux(count, age));
            assert_eq!((c, a), (count.min(COUNT_CAP), age.min(AGE_CAP)));
        }
        assert_eq!(unpack_aux(-1.0), (0.0, 0.0), "a cleared aux texel: no casts, no age");
        let src = include_str!("medium_resolve.wesl").replace("\r\n", "\n");
        assert!(src.contains("let share = max(floor_share, 1.0 / (max(hist_count, 1.0) + 1.0));"));
        assert!(src.contains("out.aux = vec2<f32>(dist, count * AGE_SPAN + age);"));
        // The count is the nearest history texel's, read with the distance.
        assert!(src.contains("let a = textureLoad(history_aux, at, 0).rg;"));
        assert!(src.contains("hist_count = floor(packed / AGE_SPAN);"));
        assert!(!src.contains("medium_history_count"));
        assert!(src.contains("const AGE_SPAN: f32 = 16.0;") && src.contains("const COUNT_CAP: f32 = 255.0;"));
    }

    /// A fresh blend that a half float may not store moves a stored step or two toward the
    /// cast per channel, so a thin cloud entering empty sky appears and a faint residue fades,
    /// while a steady transmittance never drags a noisy light to the raw ray.
    #[test]
    fn a_blend_below_the_half_floats_spacing_takes_the_cast() {
        let store = |v: Vec4| Vec4::from_array(v.to_array().map(|c| half::f16::from_f32(c).to_f32()));
        let empty_rule = |v: Vec4| if v.w >= 1.0 - 1.0 / 4096.0 { Vec4::new(0.0, 0.0, 0.0, 1.0) } else { v };
        let empty = Vec4::new(0.0, 0.0, 0.0, 1.0);
        // 0.998046875 = 1 - 2^-9, a transmittance a half float stores exactly.
        let thin_t = 1.0 - 1.0 / 512.0;
        let thin = Vec4::new(0.001, 0.001, 0.001, thin_t);
        assert_eq!(half::f16::from_f32(thin_t).to_f32(), thin_t, "stored exactly");
        // History empty, a thin cloud cast: the 0.1 blend (0.9998) would store as empty again.
        let blended = empty.lerp(thin, 0.1);
        assert!(blended.w >= 1.0 - 2f32.powi(-12), "the plain blend stalls: {}", blended.w);
        assert_eq!(fresh_blend(empty, thin, 0.1), thin, "the cast is taken");
        // The reverse: a faint residue toward empty sky. The plain blend rounds back for ever; the
        // per-channel step and the empty rule clear it within a few casts.
        let stalled = thin.lerp(empty, 0.1);
        assert_eq!(half::f16::from_f32(stalled.w).to_f32(), half::f16::from_f32(thin.w).to_f32(), "the plain blend rounds back");
        let mut v = thin;
        let mut casts = 0;
        while v != empty {
            v = empty_rule(store(fresh_blend(v, empty, 0.1)));
            casts += 1;
            assert!(casts <= 4, "the residue fades: {v}");
        }
        // A steady transmittance under a noisy light: the transmittance moves one stored step
        // toward the cast, the light is blended at the share, not replaced.
        let hist = Vec4::new(1.0, 1.0, 1.0, 0.5);
        let cast = Vec4::new(1.4, 1.4, 1.4, 0.501);
        let out = fresh_blend(hist, cast, 0.1);
        assert!((out.x - 1.04).abs() < 1e-6, "the light is blended: {}", out.x);
        assert!(out.w > 0.5 && out.w <= 0.501, "toward the cast, not past it: {}", out.w);
        assert_ne!(store(out).w, 0.5, "the stored transmittance moves");
        // An ordinary step blends as before.
        let a = Vec4::new(0.4, 0.4, 0.4, 0.5);
        let b = Vec4::new(0.6, 0.5, 0.2, 0.3);
        assert!((fresh_blend(a, b, 0.1) - a.lerp(b, 0.1)).abs().max_element() < 1e-7);
        // Equal values: nothing to take.
        assert!((fresh_blend(a, a, 0.1) - a).abs().max_element() < 1e-7);
        // A texel's first own cast, the fill counting as one: a near-empty cloud over an empty fill
        // stays, whatever its transmittance (half of some would store as empty).
        let first = counted_share(FRESH_SHARE, 0.0);
        for k in [1.0f32, 1.5, 2.0, 3.0, 8.0, 100.0] {
            let cast = Vec4::new(0.0005, 0.0005, 0.0005, 1.0 - (k + 0.01) / 4096.0);
            let v = store(empty_rule(fresh_blend(empty, cast, first)));
            assert!(v.w < 1.0 && v.x > 0.0, "{cast}: {v}");
        }
        assert_eq!(fresh_blend(empty, empty, first), empty);
        let src = include_str!("medium_resolve.wesl").replace("\r\n", "\n");
        assert!(src.contains("light = medium_fresh_blend(hist, own, share);"));
        assert!(src.contains("let stalled = abs(blended - hist) <= spacing;"));
        // A step equal to the spacing lands on the midpoint between two half floats and
        // rounds back to the even one; it is a stall too and is nudged.
        let mid_hist = Vec4::new(0.0, 0.0, 0.0, 0.5);
        let mid_cast = Vec4::new(0.0, 0.0, 0.0, 0.502_441_4);
        assert_eq!(store(mid_hist.lerp(mid_cast, 0.1)).w, 0.5, "the plain blend rounds back");
        let mut v = mid_hist;
        for _ in 0..8 {
            v = store(fresh_blend(v, mid_cast, 0.1));
        }
        assert!(v.w > 0.501, "the stored transmittance converges: {}", v.w);
        assert!(src.contains("if blended.a >= EMPTY_T && own.a < EMPTY_T {\n        return own;"));
    }

    /// NVF_PLAY_DEBUG_MEDIUM_OFFSETS: `fixed` gives each texel the same start at every cast;
    /// `turning` (the default) and `blue` turn at each cast; all stay in 0..1.
    /// #3032 B1b: over a whole cycle of casts (256 phases) the four texels of a 2x2 tile, of
    /// either colour of the checkerboard, are each cast 64 times, and the start offsets each
    /// receives are its own R2 offset plus the same set of turns; with a made-up hard edge (a ray
    /// that sees cloud only when its offset falls in a window, as a thin crest between two steps)
    /// the four corners see it as often, within the 64 turns' discrepancy. The centre aid gives
    /// every texel 0.5 at every cast.
    #[test]
    fn a_tiles_corners_get_the_same_turns_and_the_same_mean() {
        let size = UVec2::new(64, 36);
        for tile in [UVec2::new(3, 5), UVec2::new(4, 5)] {
            let mut turns: Vec<Vec<f32>> = Vec::new();
            for c in [UVec2::new(0, 0), UVec2::new(1, 0), UVec2::new(0, 1), UVec2::new(1, 1)] {
                let texel = 2 * tile + c;
                let spatial = ray_offset(texel, 0, RayOffsets::Fixed, 2);
                let got: Vec<f32> = (0..256u32).filter(|&phase| 2 * tile + corner(tile, size, phase, 2) == texel).map(|phase| ray_offset(texel, phase, RayOffsets::Turning, 2)).collect();
                assert_eq!(got.len(), 64, "{texel}: cast once in four phases");
                for (lo, w) in [(0.2f32, 0.25f32), (0.7, 0.1), (0.0, 0.5)] {
                    let seen = got.iter().filter(|&&o| (o - lo).rem_euclid(1.0) < w).count() as f32 / 64.0;
                    assert!((seen - w).abs() <= 0.035, "{texel}: a window {lo}+{w} seen in {seen} of the casts");
                }
                let mut t: Vec<f32> = got.iter().map(|&o| (o - spatial).rem_euclid(1.0)).collect();
                t.sort_by(f32::total_cmp);
                turns.push(t);
                assert!((0..256).all(|phase| ray_offset(texel, phase, RayOffsets::Centre, 2) == 0.5));
            }
            for t in &turns[1..] {
                for (a, b) in t.iter().zip(&turns[0]) {
                    let d = (a - b).abs();
                    assert!(d.min(1.0 - d) < 1e-4, "tile {tile}: the corners' turns differ: {a} {b}");
                }
            }
        }
        assert_eq!(debug_offsets(Some("centre")), RayOffsets::Centre);
        assert_eq!(debug_offsets(Some("Center")), RayOffsets::Centre);
        assert_eq!(RayOffsets::Centre.code(), 3.0);
        let src = include_str!("medium_trace.wesl").replace("\r\n", "\n");
        assert!(src.contains("    if mode == 3u {\n        return 0.5;\n    }"));
    }

    /// #3032 B1b: the history's cuts by cause, tallied over a window and logged at its end (the
    /// first window always; later ones only when the dominant cause of a window
    /// with cuts changes, every window with a cut behind NVF_PLAY_DEBUG_MEDIUM_TALLY=1).
    #[test]
    fn the_history_cuts_are_tallied_by_cause() {
        let mut tally = CutTally::default();
        let still = CutCauses::default();
        let long = CutCauses { camera: true, long_frame: true, ..CutCauses::default() };
        let mut lines = Vec::new();
        for frame in 0..3 * CUT_WINDOW {
            let causes = if frame >= 2 * CUT_WINDOW { long } else { still };
            if let Some(l) = tally.count(causes, frame == 0, false) {
                lines.push(l);
            }
        }
        assert_eq!(lines.len(), 2, "the first window and the one with cuts: {lines:?}");
        assert!(lines[0].contains("history dropped on 1 of the last 120 frames") && lines[0].contains("a new or missing history 1"), "{}", lines[0]);
        assert!(lines[1].contains("history dropped on 120 of the last 120 frames") && lines[1].contains("a frame over 250 ms 120"), "{}", lines[1]);
        // Ordinary play: a camera cut now and then, a window in two; one line for the first such
        // window, none after while the cause stays the camera's; a line again when it changes.
        let camera = CutCauses { camera: true, ..CutCauses::default() };
        let mut play = CutTally::default();
        let mut quiet = Vec::new();
        for frame in 0..20 * CUT_WINDOW {
            let window = frame / CUT_WINDOW;
            let causes = if window < 16 && window % 2 == 1 && frame % CUT_WINDOW == 7 { camera } else if window >= 16 && frame % CUT_WINDOW == 9 { CutCauses { sky: true, ..still } } else { still };
            if let Some(l) = play.count(causes, frame == 0, false) {
                quiet.push(l);
            }
        }
        assert_eq!(quiet.len(), 3, "the first window, the first camera cut, the change to sky jumps: {quiet:?}");
        assert!(quiet[1].contains("a camera cut 1 ") && quiet[2].contains("a sky jump 1,"), "{quiet:?}");
        // Behind the switch: every window with a cut.
        let mut every = CutTally::default();
        let n = (0..4 * CUT_WINDOW).filter(|&f| every.count(if f % CUT_WINDOW == 3 { camera } else { still }, false, true).is_some()).count();
        assert_eq!(n, 4);
        assert!(debug_tally(Some(" 1 ")) && !debug_tally(Some("0")) && !debug_tally(None));
        assert_eq!(CutTally { cuts: 3, camera: 3, long_frame: 3, ..CutTally::default() }.dominant(), Some(CutCause::LongFrame));
        assert_eq!(CutTally::default().dominant(), None);
        assert!(!still.any() && long.any() && CutCauses { sky: true, ..still }.any() && CutCauses { gap: true, ..still }.any());
        assert_eq!(history_cut_causes(None, 5, None, None, (0, 0)), CutCauses { gap: true, camera: true, long_frame: false, sky: false });
        assert!(history_cut_causes(Some(4), 5, None, None, (1, 2)).sky);
    }

    #[test]
    fn the_ray_offsets_are_stable_per_texel_when_fixed() {
        assert_eq!(debug_offsets(None), RayOffsets::Turning);
        assert_eq!(debug_offsets(Some("Fixed")), RayOffsets::Fixed);
        assert_eq!(debug_offsets(Some("blue")), RayOffsets::Blue);
        assert_eq!(debug_offsets(Some("other")), RayOffsets::Turning);
        assert_eq!((RayOffsets::Turning.code(), RayOffsets::Fixed.code(), RayOffsets::Blue.code()), (0.0, 1.0, 2.0));
        for texel in [UVec2::new(0, 0), UVec2::new(3, 7), UVec2::new(639, 359)] {
            let fixed: Vec<f32> = (0..64).map(|phase| ray_offset(texel, phase, RayOffsets::Fixed, 2)).collect();
            assert!(fixed.iter().all(|&o| o == fixed[0]), "fixed: one offset per texel");
            for mode in [RayOffsets::Turning, RayOffsets::Blue] {
                let a = ray_offset(texel, 0, mode, 2);
                let b = ray_offset(texel, 4, mode, 2);
                assert!((a - b).abs() > 1e-3, "{mode:?} turns at the next cast");
                // Within one cycle of a 2x2 tile it does not turn.
                assert_eq!(ray_offset(texel, 4, mode, 2), ray_offset(texel, 7, mode, 2));
            }
            for mode in [RayOffsets::Turning, RayOffsets::Fixed, RayOffsets::Blue] {
                for phase in 0..32 {
                    for side in [1, 2, 3] {
                        assert!((0.0..1.0).contains(&ray_offset(texel, phase, mode, side)));
                    }
                }
            }
            // The default's arithmetic is unchanged: turning at phase 0 is the fixed offset.
            assert_eq!(ray_offset(texel, 0, RayOffsets::Turning, 2), ray_offset(texel, 0, RayOffsets::Fixed, 2));
        }
        let src = include_str!("medium_trace.wesl").replace("\r\n", "\n");
        assert!(src.contains("fn medium_offset(texel: vec2<u32>, phase: u32, mode: u32, cycle: u32) -> f32 {"));
        assert!(src.contains("let turn = 0.6180339887 * f32((phase / max(cycle, 1u)) % 64u);"));
        assert!(src.contains("medium_offset(texel, phase, u32(medium.tiles.y), side * side)"));
    }

    #[test]
    fn the_debug_aids_parse() {
        assert_eq!(debug_mode(None), MediumMode::Default);
        assert_eq!(debug_mode(Some(" Reference ")), MediumMode::Reference);
        assert_eq!(debug_mode(Some("full")), MediumMode::Default);
        for (name, view, code) in [("fresh", DebugView::Fresh, 1.0), ("age", DebugView::Age, 2.0), ("HISTORY", DebugView::History, 3.0), ("clamped", DebugView::Clamped, 4.0), ("distance", DebugView::Distance, 5.0)] {
            assert_eq!(debug_view(Some(name)), view);
            assert_eq!(view.code(), code);
        }
        assert_eq!(debug_view(Some("cover")), DebugView::None);
        assert_eq!(debug_view(None).code(), 0.0);
        assert_eq!(debug_fresh(None), FRESH_SHARE);
        assert_eq!(debug_fresh(Some("0.25")), 0.25);
        assert_eq!(debug_fresh(Some("3")), 1.0);
        assert_eq!(debug_fresh(Some("-1")), 0.0);
        assert_eq!(debug_fresh(Some("NaN")), FRESH_SHARE);
        assert_eq!(debug_turn(None), None);
        assert_eq!(debug_turn(Some("0")), None);
        assert_eq!(debug_turn(Some("45")), Some(45.0));
        assert_eq!(debug_turn(Some("-90.5")), Some(-90.5));
        assert_eq!(debug_turn(Some("1e9")), Some(MAX_TURN));
        assert_eq!(debug_turn(Some("fast")), None);
        // NVF_PLAY_DEBUG_MEDIUM_SCALE and NVF_PLAY_DEBUG_MEDIUM_TILE (#3032 stage 2a).
        assert_eq!(debug_scale(None), None, "the level's own");
        assert_eq!(debug_scale(Some(" 3 ")), Some(3.0));
        assert_eq!(debug_scale(Some("1")), Some(1.0));
        assert_eq!(debug_scale(Some("4")), Some(4.0));
        assert_eq!(debug_scale(Some("0")), Some(f64::from(MIN_SCALE)));
        assert_eq!(debug_scale(Some("16")), Some(f64::from(MAX_SCALE)));
        assert_eq!(debug_scale(Some("2.5")), Some(2.5));
        assert_eq!(debug_scale(Some("-1")), None);
        assert_eq!(debug_scale(Some("half")), None);
        assert_eq!(debug_tile(None), TILE_SIDE);
        assert_eq!(debug_tile(Some(" 1 ")), 1);
        assert_eq!(debug_tile(Some("2")), 2);
        assert_eq!(debug_tile(Some(" 3 ")), 3);
        assert_eq!(debug_tile(Some("4")), TILE_SIDE, "a side of 4 is not built");
        assert_eq!(debug_tile(Some("x")), TILE_SIDE);
        // The defaults are the launch's of 2026-11-23, judged by eye: HISTORY_FILTER=adaptive TILE=3, and the
        // row's High its SCALE=2.5.
        assert_eq!((history_filter(None), debug_tile(None), Some(CloudLevel::High.buffer_scale())), (history_filter(Some("adaptive")), debug_tile(Some("3")), debug_scale(Some("2.5"))));
    }

    fn mark(weather: u32, fading_from: Option<u32>, hour: f32) -> SkyMark {
        SkyMark { weather, fading_from, indoors: false, game_hours: Some(240.0 + hour), hour, dome: None }
    }

    #[test]
    fn the_sky_cuts_on_jumps_and_not_on_fades_or_the_clock() {
        // The game's own scale (30) at 60 fps: a few seconds of game time a frame.
        let normal = clock_allowance(30.0 / 3600.0, 1.0 / 60.0);
        // The clock's advance (one sky step), midnight, walking over a cell's edge: no cut.
        assert!(!sky_cut(&mark(1, None, 12.0), &mark(1, None, 12.0 + SKY_CLOCK_STEP), normal));
        assert!(!sky_cut(&mark(1, None, 12.0), &mark(1, None, 12.0), normal));
        assert!(!sky_cut(&mark(1, None, 23.999), &SkyMark { game_hours: Some(264.001), ..mark(1, None, 0.001) }, normal));
        assert!(!sky_cut(&SkyMark { game_hours: None, ..mark(1, None, 23.999) }, &SkyMark { game_hours: None, ..mark(1, None, 0.001) }, normal), "midnight on a clock without a simulation");
        // The simulation's day count rolling over at another hour than midnight invents nothing.
        assert!(!sky_cut(&SkyMark { game_hours: Some(287.999), ..mark(1, None, 15.0) }, &SkyMark { game_hours: Some(288.001), ..mark(1, None, 15.002) }, normal));
        // A one-hour wait, a small backward console set, a jump of a whole day: cuts.
        assert!(sky_cut(&mark(1, None, 12.0), &mark(1, None, 13.0), normal));
        assert!(sky_cut(&mark(1, None, 12.0), &mark(1, None, 11.95), normal));
        assert!(sky_cut(&mark(1, None, 12.0), &SkyMark { game_hours: Some(276.0), ..mark(1, None, 12.0) }, normal), "a jump of a whole day to the same hour");
        assert!(sky_cut(&mark(1, None, 12.0), &mark(1, None, f32::NAN), normal));
        // A mod's TimeScale of 18000 at 30 fps: 0.167 game hours a frame, every frame, no cut.
        let fast = clock_allowance(18000.0 / 3600.0, 1.0 / 30.0);
        assert!(!sky_cut(&mark(1, None, 12.0), &mark(1, None, 12.0 + 0.1667), fast));
        // A console `set gamehour` moves the hour, not the days: still a cut.
        assert!(sky_cut(&mark(1, None, 12.0), &SkyMark { game_hours: Some(252.0), ..mark(1, None, 6.0) }, normal));
        assert!(sky_cut(&mark(1, None, 12.0), &mark(1, None, 13.0), fast), "a wait still cuts");
        // A fade that starts, runs, turns round and ends: no cut.
        assert!(!sky_cut(&mark(1, None, 12.0), &mark(2, Some(1), 12.0), normal));
        assert!(!sky_cut(&mark(2, Some(1), 12.0), &mark(2, Some(1), 12.0), normal));
        assert!(!sky_cut(&mark(2, Some(1), 12.0), &mark(1, Some(2), 12.0), normal));
        assert!(!sky_cut(&mark(2, Some(1), 12.0), &mark(2, None, 12.0), normal));
        // A snap (ForceWeather, a mod's script): a cut.
        assert!(sky_cut(&mark(1, None, 12.0), &mark(2, None, 12.0), normal));
        // Indoors and out, another worldspace's dome: cuts.
        assert!(sky_cut(&mark(1, None, 12.0), &SkyMark { indoors: true, ..mark(1, None, 12.0) }, normal));
        let dome = AssetId::<Mesh>::from(bevy::asset::uuid::Uuid::from_u128(7));
        assert!(sky_cut(&mark(1, None, 12.0), &SkyMark { dome: Some(dome), ..mark(1, None, 12.0) }, normal));
        // A broken scale or delta allows only the sky's own steps.
        assert_eq!(clock_allowance(f32::NAN, 0.016), 2.0 * SKY_CLOCK_STEP);
    }

    fn camera(frame: u32) -> UpscaleCamera {
        UpscaleCamera {
            settings: UpscaleSettings::default(),
            translation: Vec3::new(1000.0, 50.0, -2000.0),
            rotation: Quat::IDENTITY,
            near: 5.0,
            fov: 1.2,
            frame,
            delta_ms: 16.0,
            loading_generation: 0,
            play_state: None,
            screen_mark: false,
        }
    }

    #[test]
    fn the_history_is_dropped_on_a_gap_a_camera_cut_or_a_sky_jump() {
        let (a, b) = (camera(10), camera(11));
        assert!(!history_cut(Some(10), 11, Some(&a), Some(&b), (3, 3)));
        assert!(history_cut(None, 11, Some(&a), Some(&b), (3, 3)), "no history yet (first frame, row switched on)");
        assert!(history_cut(Some(9), 11, Some(&a), Some(&b), (3, 3)), "a frame without a whole reconstruction");
        assert!(history_cut(Some(10), 11, Some(&a), Some(&b), (3, 4)), "the sky jumped");
        assert!(history_cut(Some(10), 11, None, Some(&b), (3, 3)), "no camera before");
        let teleport = UpscaleCamera { translation: a.translation + Vec3::new(500.0, 0.0, 0.0), ..b };
        assert!(history_cut(Some(10), 11, Some(&a), Some(&teleport), (3, 3)));
        let turn = UpscaleCamera { rotation: Quat::from_rotation_y(1.0), ..b };
        assert!(history_cut(Some(10), 11, Some(&a), Some(&turn), (3, 3)), "more than 45 degrees in a frame");
        let pan = UpscaleCamera { rotation: Quat::from_rotation_y(0.05), ..b };
        assert!(!history_cut(Some(10), 11, Some(&a), Some(&pan), (3, 3)), "a turn keeps the history");
        let loading = UpscaleCamera { loading_generation: 1, ..b };
        assert!(history_cut(Some(10), 11, Some(&a), Some(&loading), (3, 3)));
    }

    /// CPU mirror of medium_resolve.wesl's reprojection: where texel `frag` of the buffer lay
    /// last frame (in texels), for a point at `dist` along its ray (empty: direction alone).
    fn reproject(now: &MediumTraceUniform, prev_clip: Mat4, delta: Vec3, frag: Vec2, dist: f32) -> Option<Vec2> {
        let dir = ray_dir(now, frag);
        let rel = if dist > 0.0 { (dir * dist + delta).extend(1.0) } else { dir.extend(1.0) };
        let clip = prev_clip * rel;
        if clip.w <= 1e-6 {
            return None;
        }
        let n = clip.truncate().truncate() / clip.w;
        let uv = Vec2::new(n.x * 0.5 + 0.5, 0.5 - n.y * 0.5);
        (uv.cmpge(Vec2::ZERO).all() && uv.cmple(Vec2::ONE).all()).then_some(uv * now.grid.truncate().truncate())
    }

    #[test]
    fn the_reprojection_holds_still_follows_a_pan_and_rejects_what_was_not_seen() {
        let size = UVec2::new(11, 9);
        let proj = projection(size);
        let still = Transform::from_xyz(50_000.0, 900.0, -30_000.0).looking_to(Vec3::new(0.2, 0.3, -1.0), Vec3::Y);
        let now = trace_uniform(still.to_matrix(), proj, size, 0);
        let prev = clip_from_rel(still.to_matrix(), proj).expect("finite");
        for (x, y) in [(0u32, 0u32), (5, 4), (10, 8)] {
            let frag = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            for dist in [EMPTY_DISTANCE, 40_000.0] {
                let at = reproject(&now, prev, Vec3::ZERO, frag, dist).expect("on screen");
                assert!((at - frag).abs().max_element() < 2e-3, "a still camera maps {frag} onto itself: {at}");
            }
        }
        // A pan to the left: what is at the centre now was left of the centre last frame.
        let panned = Transform { rotation: still.rotation * Quat::from_rotation_y(0.05), ..still };
        let now = trace_uniform(panned.to_matrix(), proj, size, 0);
        let centre = Vec2::new(5.5, 4.5);
        let at = reproject(&now, prev, Vec3::ZERO, centre, EMPTY_DISTANCE).expect("on screen");
        assert!(at.x < centre.x - 0.3 && (at.y - centre.y).abs() < 0.3, "{at}");
        // The camera moved sideways: empty sky reprojects by rotation alone, a near cloud shows
        // parallax.
        let moved = Transform::from_translation(still.translation + still.right().as_vec3() * 2_000.0).with_rotation(still.rotation);
        let now = trace_uniform(moved.to_matrix(), proj, size, 0);
        let delta = moved.translation - still.translation;
        let far = reproject(&now, prev, delta, centre, EMPTY_DISTANCE).expect("on screen");
        assert!((far - centre).abs().max_element() < 2e-3, "empty sky: rotation only, {far}");
        let near = reproject(&now, prev, delta, centre, 20_000.0).expect("on screen");
        assert!(near.x > centre.x + 0.1, "parallax of a near cloud: {near}");
        // A turn of 90 degrees: the old view never saw the new centre; behind: rejected.
        for angle in [std::f32::consts::FRAC_PI_2, std::f32::consts::PI] {
            let turned = Transform { rotation: still.rotation * Quat::from_rotation_y(angle), ..still };
            let now = trace_uniform(turned.to_matrix(), proj, size, 0);
            assert_eq!(reproject(&now, prev, Vec3::ZERO, centre, EMPTY_DISTANCE), None, "turned {angle}");
        }
    }

    #[test]
    fn the_resolve_shader_is_valid_wgsl_and_declares_what_the_rust_side_binds() {
        use wgpu::naga;
        let shader = include_str!("medium_resolve.wesl").replace("\r\n", "\n");
        let mut source = String::from("struct FullscreenVertexOutput {\n    @builtin(position) position: vec4<f32>,\n    @location(0) uv: vec2<f32>,\n};\n");
        for line in shader.lines() {
            if line.starts_with("import ") {
                continue;
            }
            assert!(!line.contains("@if"), "a WESL condition this test does not resolve: {line}");
            source.push_str(line);
            source.push('\n');
        }
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(module.entry_points.iter().any(|e| e.name == "medium_resolve"));
        let start = shader.find("struct MediumResolve {").expect("struct");
        let end = start + shader[start..].find("\n}").unwrap();
        let fields: Vec<&str> = shader[start..end].lines().skip(1).map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")).collect();
        assert_eq!(fields, ["world_from_clip: mat4x4<f32>,", "prev_clip_from_rel: mat4x4<f32>,", "prev_world_from_clip: mat4x4<f32>,", "delta: vec4<f32>,", "grid: vec4<f32>,", "params: vec4<f32>,", "clamp_mode: vec4<f32>,", "pattern: vec4<f32>,"]);
        assert_eq!(MediumResolveUniform::min_size().get(), 3 * 64 + 5 * 16);
        // make_pipelines' layout order and resolve_pipeline's targets.
        for binding in [
            "@group(0) @binding(0) var<uniform> resolve: MediumResolve;",
            "@group(0) @binding(1) var trace_light: texture_2d<f32>;",
            "@group(0) @binding(2) var trace_distance: texture_2d<f32>;",
            "@group(0) @binding(3) var history_light: texture_2d<f32>;",
            "@group(0) @binding(4) var history_sampler: sampler;",
            "@group(0) @binding(5) var history_aux: texture_2d<f32>;",
            "@location(0) published: vec4<f32>,",
            "@location(1) light: vec4<f32>,",
            "@location(2) aux: vec2<f32>,",
        ] {
            assert!(shader.contains(binding), "{binding}");
        }
        // The distance view's scale is the source's range (pattern.y, SourceFrame::distance_scale).
        assert!(shader.contains("clamp(dist / max(resolve.pattern.y, 1.0), 0.0, 1.0)"));
        assert!(!shader.contains("240000.0"));
    }

    #[test]
    fn the_clip_transform_kept_for_the_next_frame_is_finite_or_none() {
        let size = UVec2::new(5, 3);
        assert!(clip_from_rel(Transform::default().to_matrix(), projection(size)).is_some());
        assert!(clip_from_rel(Mat4::from_cols(Vec4::NAN, Vec4::Y, Vec4::Z, Vec4::W), projection(size)).is_none());
    }


    // --- The reconstruction ---

    /// CPU mirror of medium_resolve.wesl `medium_carried_distance`.
    fn carried_distance(prev_world_from_clip: Mat4, prev_uv: Vec2, dist: f32, delta: Vec3) -> f32 {
        if dist <= 0.0 {
            return EMPTY_DISTANCE;
        }
        let ndc = Vec2::new(prev_uv.x * 2.0 - 1.0, 1.0 - prev_uv.y * 2.0);
        let q = prev_world_from_clip * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let prev_dir = (q.truncate() / q.w).normalize();
        (prev_dir * dist - delta).length().max(1.0)
    }

    /// A sideways step below the cut (300 units) with a point now 400 units
    /// straight ahead: last frame it was 500 away; carried into this frame it is 400 again.
    #[test]
    fn the_carried_distance_is_measured_from_this_frames_camera() {
        let size = UVec2::new(9, 7);
        let proj = projection(size);
        let prev_cam = Transform::from_xyz(-300.0, 0.0, 0.0);
        let now_cam = Transform::from_xyz(0.0, 0.0, 0.0);
        let delta = now_cam.translation - prev_cam.translation;
        assert!(delta.length() < 400.0, "below the camera cut");
        let prev_world_from_clip = trace_uniform(prev_cam.to_matrix(), proj, size, 0).world_from_clip;
        let prev_clip = clip_from_rel(prev_cam.to_matrix(), proj).expect("finite");
        // The point 400 ahead of the camera now, seen from last frame's camera.
        let point_prev_rel = Vec3::new(0.0, 0.0, -400.0) + delta;
        assert!((point_prev_rel.length() - 500.0).abs() < 1e-3);
        let clip = prev_clip * point_prev_rel.extend(1.0);
        let n = clip.truncate().truncate() / clip.w;
        let prev_uv = Vec2::new(n.x * 0.5 + 0.5, 0.5 - n.y * 0.5);
        let carried = carried_distance(prev_world_from_clip, prev_uv, point_prev_rel.length(), delta);
        assert!((carried - 400.0).abs() < 0.5, "{carried}");
        // The old formula (radial distance minus the step along the ray) kept 500.
        assert_eq!(carried_distance(prev_world_from_clip, prev_uv, EMPTY_DISTANCE, delta), EMPTY_DISTANCE, "empty stays empty");
        let src = include_str!("medium_resolve.wesl").replace("\r\n", "\n");
        assert!(src.contains("return max(length(prev_dir * dist - delta), 1.0);"), "the shader's arithmetic is the mirror's");
        assert!(src.contains("if hist.a >= EMPTY_T {\n            hist_d = -1.0;"), "a history clamped to empty sky is empty again");
    }

    /// CPU mirror of the reconstruction's nearest cast ray among the 3x3 tiles (own tile first,
    /// strictly nearer wins), tiles of 2x2.
    fn nearest_cast(texel: UVec2, size: UVec2, phase: u32) -> UVec2 {
        nearest_cast_side(texel, size, phase, 2)
    }

    /// As [`nearest_cast`], for tiles of `side` x `side`.
    fn nearest_cast_side(texel: UVec2, size: UVec2, phase: u32, side: u32) -> UVec2 {
        let tiles = trace_size(size, side);
        let tile = texel / side;
        let at = |t: UVec2| side * t + corner(t, size, phase, side);
        let sq = |c: UVec2| (c.as_ivec2() - texel.as_ivec2()).as_vec2().length_squared();
        let mut best = at(tile);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let t = (tile.as_ivec2() + IVec2::new(dx, dy)).clamp(IVec2::ZERO, tiles.as_ivec2() - 1).as_uvec2();
                if sq(at(t)) < sq(best) {
                    best = at(t);
                }
            }
        }
        best
    }

    /// CPU mirror of the reconstruction's nine fresh rays around `texel` (the 3x3 tiles, held to the
    /// buffer's): where each was cast, and its weight (0 for a tile held to the buffer's, which counts
    /// where it lies; at side 1 the clip's nine equal weights).
    fn rays_around(texel: UVec2, size: UVec2, phase: u32, side: u32) -> ([UVec2; 9], [f32; 9]) {
        let tiles = trace_size(size, side);
        let tile = texel / side;
        let rays: [(UVec2, f32); 9] = std::array::from_fn(|i| {
            let at = tile.as_ivec2() + IVec2::new(i as i32 % 3 - 1, i as i32 / 3 - 1);
            let t = at.clamp(IVec2::ZERO, tiles.as_ivec2() - 1);
            let c = side * t.as_uvec2() + corner(t.as_uvec2(), size, phase, side);
            let counts = side == 1 || t == at;
            (c, if counts { ray_weight((c.as_ivec2() - texel.as_ivec2()).as_vec2().length_squared(), side) } else { 0.0 })
        });
        (rays.map(|r| r.0), rays.map(|r| r.1))
    }

    /// A texel with no valid history, as the resolve shader leaves it: its own ray where one was cast
    /// this frame, else the weighted mean, then the empty rule.
    fn unremembered(texel: UVec2, size: UVec2, phase: u32, side: u32, field: &dyn Fn(UVec2) -> Vec4) -> Vec4 {
        let tile = texel / side;
        let (casts, weights) = rays_around(texel, size, phase, side);
        let fresh = side * tile + corner(tile, size, phase, side) == texel;
        let light = if fresh { field(texel) } else { weighted_mean(&casts.map(field), &weights) };
        if light.w >= 1.0 - 1.0 / 4096.0 { Vec4::new(0.0, 0.0, 0.0, 1.0) } else { light }
    }

    /// Blocks seen in a play test of 2026-11-23: a texel with no history is filled from the nine fresh rays
    /// weighed by distance. The weights are positive, each tile's once (a tile the buffer's edge gives
    /// twice has 0 for the second), and sum to one once normalised, the nearest ray's is the greatest, a constant field stays constant, empty sky stays canonical empty and a
    /// near-empty cloud is not wiped; at side 1 each texel is its own ray and the clip's weights are
    /// equal, as before.
    #[test]
    fn the_fill_weighs_the_nine_fresh_rays() {
        let empty = Vec4::new(0.0, 0.0, 0.0, 1.0);
        let cloud = Vec4::new(0.3, 0.35, 0.4, 0.45);
        let wisp = Vec4::new(0.0004, 0.0005, 0.0006, 0.998);
        let own = |c: UVec2| Vec4::new(c.x as f32, c.y as f32, 0.0, 0.5);
        for side in [1u32, 2, 3] {
            for size in [UVec2::new(1, 1), UVec2::new(2, 7), UVec2::new(13, 11)] {
                for phase in 0..36 {
                    for texel in (0..size.x * size.y).map(|i| UVec2::new(i % size.x, i / size.x)) {
                        let (casts, weights) = rays_around(texel, size, phase, side);
                        let sum: f32 = weights.iter().sum();
                        assert!(weights.iter().all(|&w| (0.0..=1.0).contains(&w)) && weights[4] > 0.0, "side {side} {texel}: {weights:?}");
                        if side > 1 {
                            let once: std::collections::HashSet<UVec2> = casts.iter().copied().collect();
                            let counted: Vec<UVec2> = casts.iter().zip(&weights).filter(|(_, w)| **w > 0.0).map(|(c, _)| *c).collect();
                            assert_eq!((counted.len(), counted.iter().copied().collect::<std::collections::HashSet<_>>()), (once.len(), once), "side {side} {texel}: each ray once");
                        }
                        assert!((weights.iter().map(|w| w / sum).sum::<f32>() - 1.0).abs() < 1e-6);
                        if side == 1 {
                            assert_eq!(weights, [1.0; 9], "the clip's weights are equal");
                            assert_eq!(unremembered(texel, size, phase, side, &own), own(texel), "its own ray");
                        } else {
                            let nearest = nearest_cast_side(texel, size, phase, side);
                            let greatest = weights.iter().copied().fold(0.0, f32::max);
                            assert!(casts.iter().zip(&weights).any(|(&c, &w)| c == nearest && w == greatest), "side {side} {texel}");
                        }
                        for (v, tol) in [(cloud, 1e-6), (wisp, 1e-6), (empty, 0.0)] {
                            let out = unremembered(texel, size, phase, side, &|_| v);
                            assert!((out - v).abs().max_element() <= tol, "side {side} {texel}: {out}, a field of {v}");
                        }
                    }
                }
            }
        }
        // The mean composes to the mean of what the rays compose to, over any background.
        let (_, weights) = rays_around(UVec2::new(4, 4), UVec2::new(13, 11), 5, 3);
        let rays: [Vec4; 9] = std::array::from_fn(|i| if i % 2 == 0 { cloud } else { empty });
        let over = |v: Vec4, sky: f32| v.x + v.w * sky;
        for sky in [0.0f32, 0.3, 2.0] {
            let want = rays.iter().zip(&weights).map(|(&r, &w)| w * over(r, sky)).sum::<f32>() / weights.iter().sum::<f32>();
            assert!((over(weighted_mean(&rays, &weights), sky) - want).abs() < 1e-6);
        }
        let src = include_str!("medium_resolve.wesl").replace("\r\n", "\n");
        assert!(src.contains("    if side == 1u {\n        return 1.0;\n    }\n    return exp(-sq / (2.0 * 0.36 * f32(side * side)));"));
        assert!(src.contains("            ray_w[ray_count] = select(0.0, medium_ray_weight(sq, side), side == 1u || all(t == at));\n            ray_count += 1u;"));
        assert!(src.contains("            let at = vec2<i32>(tile) + vec2<i32>(dx, dy);\n            let t = clamp(at, vec2<i32>(0), last);"));
        assert!(src.contains("        mean += ray_w[i] * rays[i];\n        weight_sum += ray_w[i];"));
        assert!(src.contains("    mean = mean / weight_sum;\n    let filled = mean;"));
        assert!(src.contains("    var hist = filled;\n    var hist_d = near_d;"));
        assert!(src.contains("    var light = filled;\n    var dist = near_d;"));
    }

    /// The fill of a step edge (upright, level or slanted) has no block: no 2x2 texels, all with rays
    /// on both sides of the edge, store the same value, and no two neighbours differ by the whole
    /// step. The nearest ray's fill, which this replaced, has both.
    #[test]
    fn a_filled_step_edge_has_no_blocks() {
        let size = UVec2::splat(24);
        for side in [2u32, 3] {
            let (mut blocks, mut greatest, mut near_blocks, mut whole_steps) = (0, 0.0f32, 0, 0);
            for (phase, axis, at) in (0..side * side).flat_map(|p| (0..3usize).flat_map(move |a| (6..18u32).map(move |x| (p, a, x)))) {
                let dense = |c: UVec2| if [2 * c.x, 2 * c.y, c.x + c.y][axis] >= 2 * at { 1.0f32 } else { 0.0 };
                // The weighted fill as stored, the nearest ray's, and whether the rays differ.
                let image: Vec<(f32, f32, bool)> = (0..size.x * size.y)
                    .map(|i| {
                        let texel = UVec2::new(i % size.x, i / size.x);
                        let (casts, weights) = rays_around(texel, size, phase, side);
                        let rays = casts.map(|c| Vec4::splat(dense(c)));
                        let fill = half::f16::from_f32(weighted_mean(&rays, &weights).x).to_f32();
                        (fill, dense(nearest_cast_side(texel, size, phase, side)), rays.contains(&Vec4::ZERO) && rays.contains(&Vec4::ONE))
                    })
                    .collect();
                for i in (0..size.x * (size.y - 1)).filter(|i| i % size.x + 1 < size.x) {
                    let quad = [i, i + 1, i + size.x, i + size.x + 1].map(|j| image[j as usize]);
                    if quad.iter().all(|q| q.2) {
                        blocks += quad.iter().all(|q| q.0 == quad[0].0) as u32;
                        near_blocks += quad.iter().all(|q| q.1 == quad[0].1) as u32;
                    }
                    for other in [quad[1], quad[2]] {
                        greatest = greatest.max((other.0 - quad[0].0).abs());
                        whole_steps += (other.1 != quad[0].1) as u32;
                    }
                }
            }
            assert_eq!(blocks, 0, "side {side}");
            assert!(greatest < if side == 3 { 0.35 } else { 0.6 }, "side {side}: the greatest step between neighbours {greatest}");
            assert!(near_blocks > 1000 && whole_steps > 1000, "side {side}: the nearest ray's fill has blocks and whole steps ({near_blocks}, {whole_steps})");
        }
    }

    /// At phase 0, texel (1,1) is nearest the ray cast at (1,2), one texel away, not
    /// its own tile's at (0,0); a cast texel is its own nearest. That ray's distance seeds the
    /// history's lookup (it no longer fills the texel alone).
    #[test]
    fn a_skipped_texel_seeds_its_lookup_from_the_nearest_cast_ray() {
        let size = UVec2::new(8, 8);
        assert_eq!(corner(UVec2::ZERO, size, 0, 2), UVec2::ZERO);
        assert_eq!(nearest_cast(UVec2::new(1, 1), size, 0), UVec2::new(1, 2));
        for phase in 0..4 {
            for y in 0..size.y {
                for x in 0..size.x {
                    let texel = UVec2::new(x, y);
                    let tile = texel / 2;
                    let near = nearest_cast(texel, size, phase);
                    if 2 * tile + corner(tile, size, phase, 2) == texel {
                        assert_eq!(near, texel, "a cast texel is its own nearest");
                    }
                    assert!((near.as_ivec2() - texel.as_ivec2()).abs().max_element() <= 2);
                }
            }
        }
        let src = include_str!("medium_resolve.wesl").replace("\r\n", "\n");
        assert!(src.contains("            if sq < near_sq {\n                near_sq = sq;\n                near_d = textureLoad(trace_distance, t, 0).r;"));
    }

    /// An empty texel is canonical where it is written, so a bilinear read
    /// between it and a covered neighbour carries none of its light.
    #[test]
    fn an_empty_texel_carries_no_light_into_a_filtered_neighbour() {
        let one_from = 1.0 - 2f32.powi(-12);
        assert_eq!(half::f16::from_f32(one_from).to_f32(), 1.0, "stores as one from here");
        assert!(half::f16::from_f32(one_from - 1e-4).to_f32() < 1.0);
        // The shaders' canonical form (medium_trace.wesl `medium_output`, medium_resolve.wesl).
        let canonical = |rgb: Vec3, t: f32| if t >= one_from { (Vec3::ZERO, 1.0) } else { (rgb, t) };
        let bright_empty = canonical(Vec3::splat(0.5), 0.9999);
        assert_eq!(bright_empty, (Vec3::ZERO, 1.0));
        let dark = canonical(Vec3::splat(0.1), 0.2);
        // Halfway between the two texels: the colour per unit of alpha is the dark texel's own.
        let (rgb, t) = ((bright_empty.0 + dark.0) * 0.5, (bright_empty.1 + dark.1) * 0.5);
        let alpha = 1.0 - t;
        assert!((rgb.x / alpha - dark.0.x / (1.0 - dark.1)).abs() < 1e-6, "no tint from the empty texel");
        let trace = include_str!("medium_trace.wesl").replace("\r\n", "\n");
        let resolve = include_str!("medium_resolve.wesl").replace("\r\n", "\n");
        assert!(trace.contains("const F16_ONE_FROM: f32 = 0.99975586;") && resolve.contains("const EMPTY_T: f32 = 0.99975586;"));
        assert!((one_from - 0.99975586).abs() < 1e-7);
        assert_eq!(trace.matches("return medium_output(s);").count(), 2, "both trace entries");
        assert!(resolve.contains("if light.a >= EMPTY_T {\n        light = vec4<f32>(0.0, 0.0, 0.0, 1.0);\n        dist = -1.0;"));
    }

    #[test]
    fn the_phase_uploads_within_the_shaders_period() {
        assert_eq!(phase_upload(0, 2), 0.0);
        assert_eq!(phase_upload(257, 2), 1.0);
        assert_eq!(phase_upload(u32::MAX, 2), 255.0);
        // The pattern repeats every 256 phases: the corner every 4, the offset's turn every 64 casts.
        for phase in [3u32, 1_000_003, u32::MAX - 7] {
            assert_eq!(corner(UVec2::new(3, 2), UVec2::new(9, 7), phase, 2), corner(UVec2::new(3, 2), UVec2::new(9, 7), phase_upload(phase, 2) as u32, 2));
            for side in [1, 2, 3] {
                let texel = UVec2::new(6, 5);
                assert_eq!(ray_offset(texel, phase, RayOffsets::Turning, side), ray_offset(texel, phase_upload(phase, side) as u32, RayOffsets::Turning, side));
            }
        }
    }

    /// The noise images generated but not both on the GPU are named
    /// once per pair of images (each activation makes fresh ones) after NOISE_UPLOAD_WAIT_SECS,
    /// with which view is missing; both views there, or no images, is no wait. And the verdict
    /// without both views is "not uploaded yet", after the extent.
    #[test]
    fn a_stalled_noise_upload_is_named_once_per_activation() {
        let id = |k: u128| AssetId::<Image>::Uuid { uuid: bevy::asset::uuid::Uuid::from_u128(k) };
        let (first, second) = ([id(1), id(2)], [id(3), id(4)]);
        let t0 = std::time::Instant::now();
        let at = |s: f32| t0 + std::time::Duration::from_secs_f32(s);
        let mut wait = UploadWait::default();
        assert_eq!(wait.note(Some(first), [true, false], at(0.0)), None);
        assert_eq!(wait.note(Some(first), [true, false], at(9.0)), None, "not yet");
        assert_eq!(wait.note(Some(first), [true, false], at(10.5)), Some([true, false]), "named, the erosion missing");
        assert_eq!(wait.note(Some(first), [false, false], at(30.0)), None, "once");
        assert_eq!(wait.note(Some(first), [true, true], at(31.0)), None, "both there");
        assert_eq!(wait.note(None, [false, false], at(60.0)), None, "no images: the generation's wait, not this");
        // The next activation's images wait again from their own start, and are named again.
        assert_eq!(wait.note(Some(second), [false, true], at(100.0)), None);
        assert_eq!(wait.note(Some(second), [false, true], at(111.0)), Some([false, true]), "the base missing");
        // A view found before the wait ends: nothing named.
        let mut quick = UploadWait::default();
        assert_eq!(quick.note(Some(first), [false, false], at(0.0)), None);
        assert_eq!(quick.note(Some(first), [true, true], at(5.0)), None);
        assert_eq!(quick.note(Some(first), [true, true], at(50.0)), None);
        // The lookup and the verdict.
        fn views(have: &[AssetId<Image>]) -> impl Fn(AssetId<Image>) -> Option<AssetId<Image>> + '_ {
            move |i| have.contains(&i).then_some(i)
        }
        assert_eq!(noise_views(Some(first), views(&[first[0]])), None);
        assert_eq!(noise_views(Some(first), views(&[first[1]])), None);
        assert_eq!(noise_views(Some(first), views(&first)), Some(first));
        assert_eq!(noise_views(None, views(&first)), None);
        assert_eq!(write_cause(false, true, true, false), WriteCause::NoiseNotReady);
        assert_eq!(write_cause(false, true, false, false), WriteCause::Extent);
        assert_eq!(write_cause(false, true, true, true), WriteCause::PassesNotReady);
        assert_eq!(write_cause(true, true, true, true), WriteCause::Whole);
        assert_eq!(write_cause(true, false, true, true), WriteCause::ComposerNotReady);
    }
}
