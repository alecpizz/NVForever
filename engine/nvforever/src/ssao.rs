//! Screen-space ambient occlusion of the enhanced graphics (`graphics.rs`,
//! docs/design/graphics-enhanced.md §3.1): creases, corners and the ground under things
//! are darkened by how much of the space around them the depth prepass shows filled.
//!
//! Since #2897 the estimate is ground-truth ambient occlusion (Jimenez, Wu, Pesce, Jarabo,
//! "Practical Real-Time Strategies for Accurate Indirect Occlusion", SIGGRAPH 2016 course;
//! ssao.wesl `gtao_visibility`): per pixel a few slices through the view vector, the depth
//! marched on both sides of each for the highest horizon, the cosine-weighted visible arc
//! integrated, then a denoise that keeps to the pixel's own plane. The older estimate
//! described below (a spiral of samples) stays as the fallback until the new one is
//! measured: debug switch `NVF_PLAY_DEBUG_AO` ([`AoDebug`]).
//!
//! Bevy's own SSAO needs a normal prepass and a view without multisampling; the game's
//! views are multisampled (alpha-tested plants are smoothed with it) and the NIF, grass,
//! tree and water prepasses write depth only. So this pass works from the depth alone:
//! normals from the neighbouring depths, a 12-sample spiral turned by a 4 x 4 pattern,
//! worked out at half resolution (one pixel of each 2 x 2 block, a quarter of the
//! work: about 1.0-1.25 ms a frame at 1920x1080 at full resolution, #1370), the result
//! blurred over those 4 x 4 texels at about the same distance and multiplied
//! into the picture after tonemapping, before the image-space grade and its blur
//! (imagespace.rs). It darkens all light, not only the ambient light as the lighting
//! itself could [engineering choice: the strength is kept low for that]. While a temporal
//! upscaler (FSR, DLSS) upscales it runs at the render size on the HDR picture before the
//! upscaler instead ([`ao_before_fsr`]; the names say FSR, the first to use it). That is the
//! picture placement, since #2897 stage two the fallback (`NVF_PLAY_DEBUG_AO=post`, and what
//! runs while the lighting hook is not in place).
//!
//! On the main camera only, while it carries [`AmbientOcclusion`], at the Ambient occlusion
//! row's level ([`AoLevel`]: Performance marches one slice direction a pixel instead of
//! Quality's two). Under multisampling the passes read the depth resolved once to one sample
//! ([`resolve_depth`]), not the multisampled prepass.
//!
//! In the lighting (#2897 stage two, ssao_lighting.rs, docs/design/graphics-enhanced.md
//! §3.1.2; the default placement, [`AoDebug`]): the factor is not multiplied into the picture
//! but scales the ambient light and the lamps that cast no shadow, inside the lighting, with
//! its own strength and plant remedies ([`LIGHTING_STRENGTH`], [`LIGHTING_VARIANT`]). The
//! extraction ([`extract_lighting`]) gives the main camera's view bevy_pbr's
//! `NvfAmbientOcclusionBindings` (the view's depth-writing pipelines then read it under the
//! shader def `NVF_AMBIENT_OCCLUSION`); [`ao_lighting_pass`], between the depth prepass and
//! the main pass, works the occlusion out and writes the denoised factor per half-resolution
//! texel with its distance (ssao.wesl `denoise`) into a texture of its own, which [`prepare`]
//! binds; the post passes then do nothing for that view. Under a temporal upscaler the
//! passes run at its render size (upscaler/prep.rs), as before it. Whenever the pass cannot
//! run, the texture is cleared to "open" (factor 1); without the lighting hook (lights.rs,
//! ssao_lighting.rs `HookState`) the occlusion stays on the picture.

use crate::fly::FlyCam;
use crate::ssao_lighting::{hook_state, AmbientOcclusionLightingUniform, HookState};
use bevy::pbr::NvfAmbientOcclusionBindings;
use bevy::render::render_resource::UniformBuffer;
use bevy::render::renderer::RenderQueue;
use bevy::render::sync_world::RenderEntity;
use bevy::render::texture::FallbackImageZero;
use bevy::render::{Extract, ExtractSchedule};
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BindingResource, BlendComponent, BlendFactor, BlendOperation,
    BlendState, CachedRenderPipelineId, ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, Extent3d, FilterMode, FragmentState, LoadOp,
    Operations, PipelineCache, RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, StoreOp, TextureDescriptor, TextureDimension, TextureFormat,
    TextureSampleType, TextureUsages, TextureView,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::camera::ExtractedCamera;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

/// Radius of the occlusion around a point, in game units (about 0.9 m) [engineering
/// choice: the size of a doorstep or a crate's foot; larger darkened whole walls].
const RADIUS: f32 = 64.0;
/// How strongly a fully closed point is darkened (0..1), the old estimate [engineering
/// choice, by eye].
const STRENGTH: f32 = 0.55;
/// How much of the ground-truth estimate's darkening reaches the picture: factor
/// = 1 - strength x (1 - visibility). Open flat ground has visibility 1 (ssao.wesl
/// `gtao_visibility` divides by the slices' open integral), so it stays as bright as with the
/// switch off; a floor point at the foot of a wall has 0.5 (the wall hides half the
/// cosine-weighted hemisphere), which this darkens to 0.77 (0.8 until 2026-11-25), about what the old estimate gave
/// in such a crease [engineering choice, from that estimate, not yet from pictures: the
/// factor still multiplies all light, so it is kept low as the old one was]. The picture
/// placement's (`NVF_PLAY_DEBUG_AO=post`, and the fallback while the lighting hook is not in
/// place): stage one's 0.4, raised with the lighting placement's ([`LIGHTING_STRENGTH`]).
const GTAO_STRENGTH: f32 = 0.46;
/// The ground-truth estimate's strength in the lighting placement (the default): there the
/// factor reaches only the ambient light and the lamps that cast no shadow, never the sun,
/// the moon or highlights, so the reason for keeping the picture placement's low is gone
/// [engineering choice, the player's: at exponent 1 and lamps 1, indoors 0.4 and 0.8 looked
/// about the same and neither too dark, and outdoors 0.8 with `thin+soft` "feels about right"
/// (observed 2026-11-18, the player's play test)]. 0.92 since 2026-11-25, 0.8 until then
/// [design, set by eye: the factor darkens the ambient light, and once the display sum
/// showed its dark values as they are (lights/display_shader.rs `nvf_display_to_linear`)
/// the creases at 0.8 read as too light; both placements' strengths went up by 15%].
const LIGHTING_STRENGTH: f32 = 0.92;
/// The remedies for the shimmer on plants in the lighting placement: `thin` and `soft`
/// ([`AoVariant`]). Measured on the occlusion alone at the Goodsprings street, two bush crops
/// (median change per pixel between frames 0.12 s apart, levels of 255): the plain estimate
/// 1.96 / 2.87, `thin+soft` 0.87 / 1.11, against the old estimate's 0.67 / 0.92 [observed
/// 2026-11-18, the main session's pictures; docs/design/graphics-enhanced.md §3.1.2]; with it
/// at strength 0.8 the player saw no shimmer and no halos [observed 2026-11-18, the player's
/// play test]. `thin+wide+soft` measured a little steadier (0.74 / 0.86) but was not the one
/// played [engineering choice: the player's]. The picture placement keeps none.
const LIGHTING_VARIANT: AoVariant = AoVariant { slices: None, steps: None, wide: None, soft: true, thin: true, nearest: false };
/// Slice directions per pixel and depth samples on each side of a slice: the shader's
/// `SLICES` and `STEPS` (ssao.wesl; a test keeps them the same), for the log line. Two by
/// four at half resolution, about the old pass's twelve samples in cost [engineering choice:
/// to be traded once measured].
const GTAO_SLICES: u32 = 2;
const GTAO_STEPS: u32 = 4;
/// Slice directions per pixel at Performance ([`AoLevel`]): one, its sixteen turns over the
/// half circle across the denoise's 4 x 4 texels instead of Quality's thirty-two. Halves the
/// horizon march, the pass's scattered depth reads (16 to 8 a half-resolution texel) and most
/// of its arithmetic; the steps a side, the denoise and the remedies stay Quality's, so the
/// look stays close: the same contact shadows, a little more grain where the 4 x 4 texels
/// lie on different surfaces [engineering choice, from the shader's reads: ssao.wesl].
const PERFORMANCE_SLICES: u32 = 1;
/// Largest radius on the screen, as a share of the picture's height (near things, e.g. the
/// player's arms): 80 pixels of 1080. Until 2026-11-24 it was 80 pixels at any size, so at
/// 4K half as far as at 1080p: the radius was held to about 5% of a point's distance there,
/// 12 units (17 cm) at 3 m against the 64 meant, and a room's creases were barely shaded
/// [observed 2026-11-24, an audit of the pass against the lamp shadows' rewrite]. Under an
/// upscaler the share is of the render size's height, which the pass runs at.
const MAX_SCREEN_RADIUS: f32 = 80.0 / 1080.0;

/// [`MAX_SCREEN_RADIUS`], or `NVF_PLAY_DEBUG_AO_SCREEN=<share of the height>` (a test aid,
/// for comparing: 0.037 is what 4K had).
fn max_screen_radius() -> f32 {
    static SHARE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *SHARE.get_or_init(|| match std::env::var("NVF_PLAY_DEBUG_AO_SCREEN").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| *v > 0.0) {
        Some(v) => {
            info!("ambient occlusion: NVF_PLAY_DEBUG_AO_SCREEN={v}");
            v
        }
        None => MAX_SCREEN_RADIUS,
    })
}
/// The effect fades out between these distances (game units) [engineering choice: beyond
/// them the radius is a pixel or two and the fog takes over].
const FADE_START: f32 = 4000.0;
const FADE_END: f32 = 7000.0;

/// The Ambient occlusion row's choices, in the order the page and the INI keep them: an INI
/// written while the row was an on/off switch (`1`/`0`, also true/false, on/off, yes/no)
/// reads back as Quality or Off (settings.rs).
pub const AO_CHOICES: [&str; 3] = ["Off", "Performance", "Quality"];

/// How much the ambient occlusion costs while it is on: the Ambient occlusion row's
/// Performance and Quality ([`AO_CHOICES`]; `--ambient-occlusion[=performance|quality]`).
/// Quality is the estimate as it was before the row had levels; Performance the same pass
/// with one slice direction a pixel ([`PERFORMANCE_SLICES`]). An Ultra level, Quality's at
/// full resolution (a texel a pixel, four times the pass's pixels: 0.41 ms against 0.13 at 4K
/// with DLSS Quality), was built and dropped on 2026-11-24: "basically not noticeable even at
/// 4k native" [observed: a play test].
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AoLevel {
    Performance,
    #[default]
    Quality,
}

impl AoLevel {
    /// The name the row shows and the INI keeps.
    pub fn name(self) -> &'static str {
        match self {
            AoLevel::Performance => "Performance",
            AoLevel::Quality => "Quality",
        }
    }
}

/// Ambient occlusion on a camera; the projection part is kept up to date by
/// [`follow_projection`].
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType)]
#[extract_app(bevy::render::RenderApp)]
pub struct AmbientOcclusion {
    /// Radius, strength, near plane, fade end.
    pub params: Vec4,
    /// Projection x and y scale, largest screen radius, fade start.
    pub proj: Vec4,
    /// The size the depth prepass was drawn at (x, y) and the picture's size (z, w): all 1
    /// here; set in the render world while a temporal upscaler upscales (`upscaler/prep.rs`
    /// `prepare`), whose common passes expand that depth to the picture's size, so the normals
    /// are taken from the texels as drawn (ssao.wesl `normal_at`). The extraction puts 1 back
    /// every frame.
    pub render_scale: Vec4,
    /// While the occlusion is applied before the upscaler ([`ao_before_fsr`]): the render
    /// size (x, y), the part of the depth prepass drawn, and z 1. Set in the render world by
    /// `upscaler/prep.rs` `prepare`; 0 otherwise (the extraction puts 0 back every frame).
    pub area: Vec4,
    /// x 1 for the old estimate, y 1 for the factor shown alone as grey, z 1 for the
    /// occlusion asked for in the lighting ([`AoDebug`]), w 1 at Performance ([`AoLevel`];
    /// not read by the shaders: the level reaches them through `variant`).
    pub mode: Vec4,
    /// The candidate remedy for the shimmer on plants ([`AoVariant::uniform`]); all 0 for
    /// the estimate as shipped.
    pub variant: Vec4,
}

impl Default for AmbientOcclusion {
    fn default() -> Self {
        AmbientOcclusion::at(AoLevel::Quality)
    }
}

impl AmbientOcclusion {
    /// The occlusion at `level` (the row's Performance or Quality), with the debug aids.
    pub fn at(level: AoLevel) -> Self {
        AmbientOcclusion::with_debug_at(*AO_DEBUG, AO_DEBUG.in_lighting_now(), level)
    }

    /// The settings for the debug aids `debug`, in the lighting placement or on the picture
    /// (`lighting`: where the occlusion goes this frame, [`AoDebug::in_lighting_now`]), at
    /// Quality.
    #[cfg(test)]
    fn with_debug(debug: AoDebug, lighting: bool) -> Self {
        AmbientOcclusion::with_debug_at(debug, lighting, AoLevel::Quality)
    }

    /// The same at `level`.
    fn with_debug_at(debug: AoDebug, lighting: bool, level: AoLevel) -> Self {
        let mut mode = debug.mode();
        mode.w = if level == AoLevel::Performance { 1.0 } else { 0.0 };
        AmbientOcclusion {
            params: Vec4::new(RADIUS, debug.strength(lighting), 5.0, FADE_END),
            proj: Vec4::new(1.0, 1.0, MAX_SCREEN_RADIUS, FADE_START),
            render_scale: Vec4::ONE,
            area: Vec4::ZERO,
            mode,
            variant: debug.variant_at(lighting, level).uniform(),
        }
    }

    /// The level this occlusion runs at.
    pub(crate) fn level(&self) -> AoLevel {
        if self.mode.w > 0.5 {
            AoLevel::Performance
        } else {
            AoLevel::Quality
        }
    }

    /// Under FSR: the occlusion at the render size, on the HDR picture before FSR
    /// ([`ao_before_fsr`]). The largest radius on the screen is a share of the height the
    /// pass runs at ([`MAX_SCREEN_RADIUS`]), so it covers what it does at the display size.
    pub(crate) fn before_fsr(&mut self, render_size: UVec2, _output_size: UVec2) {
        self.area = Vec4::new(render_size.x as f32, render_size.y as f32, 1.0, 0.0);
    }

    fn runs_before_fsr(&self) -> bool {
        self.area.z > 0.5
    }

    /// The occlusion is asked for in the lighting (the default; not `post`, `old`, `view`
    /// or `view-old`); whether it goes there also takes the lighting hook
    /// ([`extract_lighting`]).
    pub(crate) fn wants_lighting(&self) -> bool {
        self.mode.z > 0.5
    }
}

/// Whether the occlusion is applied before FSR while FSR upscales ([`ao_before_fsr`]);
/// `NVF_AO_BEFORE_FSR=0` applies it after tonemapping, at the display size, as without FSR
/// (the way of builds before 2026-11-08), for comparisons. The picture placement only: in the
/// lighting it always runs at the render size (upscaler/prep.rs).
pub(crate) static AO_BEFORE_FSR: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| !std::env::var("NVF_AO_BEFORE_FSR").is_ok_and(|v| v.trim() == "0"));

/// Whether a multisampled depth prepass is resolved once to one sample for the occlusion
/// ([`resolve_depth`]); `NVF_AO_RESOLVE_DEPTH=0` reads the multisampled depth in every pass
/// instead (the way of builds before 2026-11-19), for cost comparisons. The picture is the same
/// either way.
pub(crate) static AO_RESOLVE_DEPTH: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| !std::env::var("NVF_AO_RESOLVE_DEPTH").is_ok_and(|v| v.trim() == "0"));

/// Debug switch `NVF_PLAY_DEBUG_AO` (read once at the start; a development aid for
/// before/after pictures and cost runs, not a setting). Unset, empty, `0`, `off` or `false`:
/// the ground-truth estimate on the ambient light and the lamps inside the lighting
/// (ssao_lighting.rs; #2897 stage two), the placement the switch has by default.
/// `view-lighting`: the same, the factor each surface took drawn in place of its colour. On
/// the picture (stage one's placement, also what runs while the lighting hook is not in
/// place): `post` the ground-truth estimate multiplied into the picture, `old` the old
/// estimate (the spiral of samples and its blur), `view` the ground-truth estimate's factor
/// alone as a grey picture, `view-old` the old one's. Anything else (the stage's earlier
/// `lighting` among them) is unknown: a warning, and the default runs.
///
/// The remaining aids (each read once too) replace one default of the placement that runs:
///
/// - `NVF_PLAY_DEBUG_AO_STRENGTH=<number>`, held to 0..1: the ground-truth estimate's strength
///   ([`LIGHTING_STRENGTH`] in the lighting, [`GTAO_STRENGTH`] on the picture); the old
///   estimate keeps its own.
/// - `NVF_PLAY_DEBUG_AO_VARIANT=<names>`: the candidate remedies for the shimmer on plants
///   ([`AoVariant`]); unset or empty, [`LIGHTING_VARIANT`] in the lighting and none on the
///   picture; `none` (or `0`, `off`) the plain estimate in either; the old estimate ignores it.
/// - `NVF_PLAY_DEBUG_AO_LAMPS=<number>`, held to 0..1, the lighting only: how much of the
///   factor reaches the light of lamps that cast no shadow at the surface
///   (ssao_lighting.rs `LAMPS`; 0 puts the occlusion on the ambient light alone).
/// - `NVF_PLAY_DEBUG_AO_EXPONENT=<number>`, held to 0.1..2, the lighting only: the exponent
///   the factor is raised to before it scales the display-value sum (ssao_lighting.rs
///   `EXPONENT`).
/// - `NVF_PLAY_DEBUG_AO_MULTIBOUNCE=0` (or `1`), the lighting only: the ambient's factor
///   without (with) the multi-bounce fit by the surface's colour (ssao_lighting.rs
///   `MULTIBOUNCE`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct AoDebug {
    /// The old estimate instead of ground-truth ambient occlusion (always on the picture).
    old: bool,
    /// The factor alone instead of the lit picture: as grey on the picture (`view`,
    /// `view-old`), or drawn by the lighting (`view-lighting`).
    view: bool,
    /// On the picture (stage one's placement) instead of in the lighting.
    post: bool,
    /// The ground-truth estimate's strength from `NVF_PLAY_DEBUG_AO_STRENGTH`.
    gtao_strength: Option<f32>,
    /// The lamps' weight from `NVF_PLAY_DEBUG_AO_LAMPS` (the lighting only).
    lamps: Option<f32>,
    /// The exponent from `NVF_PLAY_DEBUG_AO_EXPONENT` (the lighting only).
    exponent: Option<f32>,
    /// The multi-bounce fit from `NVF_PLAY_DEBUG_AO_MULTIBOUNCE` (the lighting only).
    multibounce: Option<bool>,
    /// The candidate remedies from `NVF_PLAY_DEBUG_AO_VARIANT`; `None` when unset or empty
    /// (the placement's default).
    variant: Option<AoVariant>,
}

/// Debug aid `NVF_PLAY_DEBUG_AO_VARIANT` (#2897): candidate remedies for the ground-truth
/// estimate's shimmer on swaying alpha-tested plants, measured against each other in one
/// build (docs/design/graphics-enhanced.md §3.1.2); `thin+soft` is the lighting placement's
/// default ([`LIGHTING_VARIANT`]), the others stay for comparisons [engineering choice].
/// Names joined by `+`, any order, case ignored: `slices3` / `slices4` (3 or 4 slice
/// directions a texel instead of 2), `steps8` (8 depth samples a side instead of 4), `wide` /
/// `wide6` (where few texels lie on the pixel's plane, the denoise widens from 4 x 4 texels to
/// 8 x 8 / 6 x 6), `soft` (there, the result pulled towards the plain mean of the nearby
/// texels), `thin` (a lower sample lets the horizon sink: thin occluders), `nearest` (under
/// multisampling, every depth read takes the nearest of the pixel's samples instead of sample
/// 0, whose share of a leaf's soft edge changes from pixel to pixel). `0`, `off` or `none`:
/// the plain estimate, bit for bit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AoVariant {
    /// Slice directions per texel (3 or 4) instead of [`GTAO_SLICES`].
    slices: Option<u32>,
    /// Depth samples on each side of a slice (8) instead of [`GTAO_STEPS`].
    steps: Option<u32>,
    /// The widened denoise's footprint in texels a side (8 or 6).
    wide: Option<u32>,
    /// The pull towards the plain local mean where the plane is sparse.
    soft: bool,
    /// The thin-occluder horizon.
    thin: bool,
    /// The nearest of the depth's samples instead of sample 0.
    nearest: bool,
}

/// ssao.wesl's `VARIANT_*` flags (`Ao.variant.z`; a test keeps them the same).
const VARIANT_WIDE: u32 = 1;
const VARIANT_WIDE6: u32 = 2;
const VARIANT_SOFT: u32 = 4;
const VARIANT_THIN: u32 = 8;
const VARIANT_NEAREST: u32 = 16;

impl AoVariant {
    /// The names the aid takes, in the order [`AoVariant::name`] writes them.
    const NAMES: [&'static str; 8] = ["slices3", "slices4", "steps8", "wide", "wide6", "soft", "thin", "nearest"];

    /// The aid's value: `Err` with the value for a name it does not know, a name given twice
    /// or two that exclude each other (`slices3` and `slices4`, `wide` and `wide6`).
    fn parse(value: Option<&str>) -> Result<AoVariant, String> {
        let v = value.map(str::trim).unwrap_or("");
        let lower = v.to_ascii_lowercase();
        if matches!(lower.as_str(), "" | "0" | "off" | "none") {
            return Ok(AoVariant::default());
        }
        let mut out = AoVariant::default();
        for name in lower.split('+').map(str::trim) {
            let taken = match name {
                "slices3" | "slices4" => out.slices.replace(if name == "slices3" { 3 } else { 4 }).is_some(),
                "steps8" => out.steps.replace(8).is_some(),
                "wide" | "wide6" => out.wide.replace(if name == "wide" { 8 } else { 6 }).is_some(),
                "soft" => std::mem::replace(&mut out.soft, true),
                "thin" => std::mem::replace(&mut out.thin, true),
                "nearest" => std::mem::replace(&mut out.nearest, true),
                _ => return Err(v.to_string()),
            };
            if taken {
                return Err(v.to_string());
            }
        }
        Ok(out)
    }

    /// The aid as set: `None` unset or empty (the placement's default), else as
    /// [`AoVariant::parse`] takes it.
    fn parse_aid(value: Option<&str>) -> Result<Option<AoVariant>, String> {
        match value.map(str::trim) {
            None | Some("") => Ok(None),
            Some(v) => AoVariant::parse(Some(v)).map(Some),
        }
    }

    fn is_set(self) -> bool {
        self != AoVariant::default()
    }

    /// The uniform's `variant` (ssao.wesl `Ao.variant`): slices, steps (0 for the shader's
    /// own), the flags.
    fn uniform(self) -> Vec4 {
        let mut flags = 0;
        match self.wide {
            Some(8) => flags |= VARIANT_WIDE,
            Some(_) => flags |= VARIANT_WIDE6,
            None => {}
        }
        if self.soft {
            flags |= VARIANT_SOFT;
        }
        if self.thin {
            flags |= VARIANT_THIN;
        }
        if self.nearest {
            flags |= VARIANT_NEAREST;
        }
        Vec4::new(self.slices.unwrap_or(0) as f32, self.steps.unwrap_or(0) as f32, flags as f32, 0.0)
    }

    /// The names set, joined by `+` in the order of [`AoVariant::NAMES`].
    fn name(self) -> String {
        let set = [self.slices == Some(3), self.slices == Some(4), self.steps.is_some(), self.wide == Some(8), self.wide == Some(6), self.soft, self.thin, self.nearest];
        AoVariant::NAMES.iter().zip(set).filter(|(_, on)| *on).map(|(n, _)| *n).collect::<Vec<_>>().join("+")
    }
}

impl AoDebug {
    /// The switch's value: `Err` with the value for one it does not know.
    fn parse(value: Option<&str>) -> Result<AoDebug, String> {
        let v = value.map(str::trim).unwrap_or("");
        match v.to_ascii_lowercase().as_str() {
            "" | "0" | "off" | "false" => Ok(AoDebug::default()),
            "view-lighting" => Ok(AoDebug { view: true, ..AoDebug::default() }),
            "post" => Ok(AoDebug { post: true, ..AoDebug::default() }),
            "old" => Ok(AoDebug { old: true, post: true, ..AoDebug::default() }),
            "view" => Ok(AoDebug { view: true, post: true, ..AoDebug::default() }),
            "view-old" => Ok(AoDebug { old: true, view: true, post: true, ..AoDebug::default() }),
            _ => Err(v.to_string()),
        }
    }

    /// `NVF_PLAY_DEBUG_AO_STRENGTH`'s (and `NVF_PLAY_DEBUG_AO_LAMPS`') value: none when unset
    /// or empty, a finite number held to 0..1, `Err` with the value for anything else.
    fn parse_strength(value: Option<&str>) -> Result<Option<f32>, String> {
        AoDebug::parse_number(value, 0.0, 1.0)
    }

    /// `NVF_PLAY_DEBUG_AO_EXPONENT`'s value: none when unset or empty, a finite number held to
    /// 0.1..2, `Err` with the value for anything else.
    fn parse_exponent(value: Option<&str>) -> Result<Option<f32>, String> {
        AoDebug::parse_number(value, 0.1, 2.0)
    }

    fn parse_number(value: Option<&str>, low: f32, high: f32) -> Result<Option<f32>, String> {
        let v = value.map(str::trim).unwrap_or("");
        if v.is_empty() {
            return Ok(None);
        }
        match v.parse::<f32>() {
            Ok(s) if s.is_finite() => Ok(Some(s.clamp(low, high))),
            _ => Err(v.to_string()),
        }
    }

    /// The occlusion is asked for in the lighting (every value but the picture's).
    fn lighting(self) -> bool {
        !self.post
    }

    /// Whether the occlusion goes into the lighting now: asked for, and the lighting hook in
    /// place (lights.rs; until then, or if it failed, the picture placement runs).
    fn in_lighting_now(self) -> bool {
        self.lighting() && hook_state() == HookState::Installed
    }

    /// The strength in the placement that runs (`lighting`).
    fn strength(self, lighting: bool) -> f32 {
        match (self.old, self.gtao_strength) {
            (true, _) => STRENGTH,
            (false, Some(s)) => s,
            (false, None) if lighting => LIGHTING_STRENGTH,
            (false, None) => GTAO_STRENGTH,
        }
    }

    /// The remedies in the placement that runs (`lighting`): the aid's, or the placement's
    /// default; none for the old estimate.
    fn variant_for(self, lighting: bool) -> AoVariant {
        match (self.old, self.variant) {
            (true, _) => AoVariant::default(),
            (false, Some(v)) => v,
            (false, None) if lighting => LIGHTING_VARIANT,
            (false, None) => AoVariant::default(),
        }
    }

    /// The remedies and slice count at `level`: Performance takes [`PERFORMANCE_SLICES`]
    /// unless the aid set a slice count; the old estimate has no slices.
    fn variant_at(self, lighting: bool, level: AoLevel) -> AoVariant {
        let variant = self.variant_for(lighting);
        match level {
            AoLevel::Performance if !self.old && variant.slices.is_none() => AoVariant { slices: Some(PERFORMANCE_SLICES), ..variant },
            _ => variant,
        }
    }

    /// The lamps' weight in the lighting.
    fn lamps(self) -> f32 {
        self.lamps.unwrap_or(crate::ssao_lighting::LAMPS)
    }

    /// The exponent in the lighting.
    fn exponent(self) -> f32 {
        self.exponent.unwrap_or(crate::ssao_lighting::EXPONENT)
    }

    /// The multi-bounce fit in the lighting.
    fn multibounce(self) -> bool {
        self.multibounce.unwrap_or(crate::ssao_lighting::MULTIBOUNCE)
    }

    /// `NVF_PLAY_DEBUG_AO_MULTIBOUNCE`'s value: none when unset or empty, `0`/`off`/`false`
    /// or `1`/`on`/`true`, `Err` with the value for anything else.
    fn parse_switch(value: Option<&str>) -> Result<Option<bool>, String> {
        let v = value.map(str::trim).unwrap_or("");
        match v.to_ascii_lowercase().as_str() {
            "" => Ok(None),
            "0" | "off" | "false" => Ok(Some(false)),
            "1" | "on" | "true" => Ok(Some(true)),
            _ => Err(v.to_string()),
        }
    }

    /// The uniform's `mode` (ssao.wesl `Ao.mode`).
    fn mode(self) -> Vec4 {
        let on = |b: bool| if b { 1.0 } else { 0.0 };
        Vec4::new(on(self.old), on(self.view), on(self.lighting()), 0.0)
    }

    /// The lighting's uniform for an occlusion of `settings` (ssao_lighting.rs): the
    /// exponent, its fade end and strength, the lamps' weight, the debug view with
    /// `view-lighting`.
    fn lighting_uniform(self, settings: &AmbientOcclusion) -> AmbientOcclusionLightingUniform {
        AmbientOcclusionLightingUniform::new(self.exponent(), settings.params.w, settings.params.y, self.lamps(), self.multibounce(), self.view && self.lighting())
    }

    /// The log line for the placement that runs (`lighting`) at Quality, without the level.
    #[cfg(test)]
    fn describe(self, lighting: bool) -> String {
        self.describe_with(lighting, None)
    }

    /// The log line for the placement that runs (`lighting`) at `level`: the estimate, the
    /// remedies and the strength actually used, where it goes, then the level (`level
    /// Performance`; the old estimate has none); an aid's value is named after it. `None`:
    /// Quality's line without the level.
    fn describe_with(self, lighting: bool, level: Option<AoLevel>) -> String {
        let aid = |set: bool, name: &str| if set { format!(" ({name})") } else { String::new() };
        let estimate = if self.old {
            let variant = match self.variant {
                Some(v) if v.is_set() => format!(", variant {} ignored (NVF_PLAY_DEBUG_AO_VARIANT)", v.name()),
                _ => String::new(),
            };
            format!("old estimate (12-sample spiral, NVF_PLAY_DEBUG_AO){variant}")
        } else {
            let variant = self.variant_for(lighting);
            let slices = self.variant_at(lighting, level.unwrap_or_default()).slices.unwrap_or(GTAO_SLICES);
            let steps = variant.steps.unwrap_or(GTAO_STEPS);
            let slice_word = if slices == 1 { "slice" } else { "slices" };
            let remedies = match (variant.is_set(), lighting) {
                (true, _) => format!(", variant {}{}", variant.name(), aid(self.variant.is_some(), "NVF_PLAY_DEBUG_AO_VARIANT")),
                // The plain estimate in the lighting only by the aid's `none`.
                (false, true) => format!(", variant none{}", aid(self.variant.is_some(), "NVF_PLAY_DEBUG_AO_VARIANT")),
                // On the picture the plain estimate is the default: stage one's line.
                (false, false) if self.variant.is_some() => ", variant none (NVF_PLAY_DEBUG_AO_VARIANT)".to_string(),
                (false, false) => String::new(),
            };
            let strength = match (self.gtao_strength, lighting) {
                (Some(s), _) => format!(", strength {s} (NVF_PLAY_DEBUG_AO_STRENGTH)"),
                (None, true) => format!(", strength {LIGHTING_STRENGTH}"),
                (None, false) => String::new(),
            };
            format!("ground-truth estimate (GTAO), {slices} {slice_word} x {steps} steps a side at half resolution, depth-aware denoise{remedies}{strength}")
        };
        let placement = if lighting {
            let lamps = self.lamps();
            let reach = if lamps > 0.0 { format!("on the ambient light and on lamps without a shadow by {lamps}") } else { "on the ambient light alone".to_string() };
            let view = if self.view { ", the factor each surface took shown alone (NVF_PLAY_DEBUG_AO=view-lighting)" } else { "" };
            let bounce = if self.multibounce() { ", multi-bounce by surface colour" } else { ", no multi-bounce" };
            format!(
                ", in the lighting: {reach}{}, exponent {}{}{bounce}{}{view}",
                aid(self.lamps.is_some(), "NVF_PLAY_DEBUG_AO_LAMPS"),
                self.exponent(),
                aid(self.exponent.is_some(), "NVF_PLAY_DEBUG_AO_EXPONENT"),
                aid(self.multibounce.is_some(), "NVF_PLAY_DEBUG_AO_MULTIBOUNCE")
            )
        } else {
            let view = if self.view { ", factor shown alone as grey (NVF_PLAY_DEBUG_AO)" } else { "" };
            let post = if self.post && !self.old && !self.view { ", on the picture (NVF_PLAY_DEBUG_AO=post)" } else { "" };
            let ignored = match (self.lamps.is_some() || self.exponent.is_some(), self.lighting()) {
                (true, false) => ", lamps and exponent ignored on the picture (NVF_PLAY_DEBUG_AO_LAMPS, NVF_PLAY_DEBUG_AO_EXPONENT)",
                _ => "",
            };
            format!("{view}{post}{ignored}")
        };
        let level = match level {
            Some(level) if !self.old => format!(", level {}", level.name()),
            _ => String::new(),
        };
        format!("ambient occlusion: {estimate}{placement}{level}")
    }
}

static AO_DEBUG: std::sync::LazyLock<AoDebug> = std::sync::LazyLock::new(|| {
    let mut debug = match AoDebug::parse(std::env::var("NVF_PLAY_DEBUG_AO").ok().as_deref()) {
        Ok(debug) => debug,
        Err(v) => {
            warn!("debug switch NVF_PLAY_DEBUG_AO={v}: expected post, view-lighting, old, view or view-old; the default (in the lighting) runs");
            AoDebug::default()
        }
    };
    debug.gtao_strength = match AoDebug::parse_strength(std::env::var("NVF_PLAY_DEBUG_AO_STRENGTH").ok().as_deref()) {
        Ok(s) => s,
        Err(v) => {
            warn!("debug switch NVF_PLAY_DEBUG_AO_STRENGTH={v}: expected a number from 0 to 1; the placement's own strength");
            None
        }
    };
    debug.lamps = match AoDebug::parse_strength(std::env::var("NVF_PLAY_DEBUG_AO_LAMPS").ok().as_deref()) {
        Ok(w) => w,
        Err(v) => {
            warn!("debug switch NVF_PLAY_DEBUG_AO_LAMPS={v}: expected a number from 0 to 1; lamps {}", crate::ssao_lighting::LAMPS);
            None
        }
    };
    debug.exponent = match AoDebug::parse_exponent(std::env::var("NVF_PLAY_DEBUG_AO_EXPONENT").ok().as_deref()) {
        Ok(e) => e,
        Err(v) => {
            warn!("debug switch NVF_PLAY_DEBUG_AO_EXPONENT={v}: expected a number from 0.1 to 2; exponent {}", crate::ssao_lighting::EXPONENT);
            None
        }
    };
    debug.multibounce = match AoDebug::parse_switch(std::env::var("NVF_PLAY_DEBUG_AO_MULTIBOUNCE").ok().as_deref()) {
        Ok(m) => m,
        Err(v) => {
            warn!("debug switch NVF_PLAY_DEBUG_AO_MULTIBOUNCE={v}: expected 0 or 1; multi-bounce {}", crate::ssao_lighting::MULTIBOUNCE);
            None
        }
    };
    debug.variant = match AoVariant::parse_aid(std::env::var("NVF_PLAY_DEBUG_AO_VARIANT").ok().as_deref()) {
        Ok(variant) => variant,
        Err(v) => {
            warn!(
                "debug aid NVF_PLAY_DEBUG_AO_VARIANT={v}: expected names from {} joined by + (each once; not slices3 with slices4 nor wide with wide6), or none; the placement's default runs",
                AoVariant::NAMES.join(", ")
            );
            None
        }
    };
    debug
});

pub struct SsaoPlugin;

impl Plugin for SsaoPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "ssao.wesl");
        app.add_plugins((ExtractComponentPlugin::<AmbientOcclusion>::default(), UniformComponentPlugin::<AmbientOcclusion>::default()))
            .add_systems(PostUpdate, (follow_projection.after(bevy::camera::CameraUpdateSystems), log_estimate));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_resource::<AoLightingGpu>()
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(ExtractSchedule, extract_lighting)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            // In the lighting: after the depth prepass is complete, before the main pass reads
            // it (Bevy's own SSAO takes the same place).
            .add_systems(Core3d, ao_lighting_pass.after(Core3dSystems::Prepass).before(Core3dSystems::MainPass))
            .add_systems(Core3d, ao_pass.in_set(Core3dSystems::PostProcess).after(tonemapping).before(bevy::anti_alias::fxaa::fxaa).before(bevy::anti_alias::smaa::smaa).before(crate::imagespace::blur_pass))
            .add_systems(Core3d, ao_before_fsr.in_set(Core3dSystems::EarlyPostProcess).before(crate::fsr::fsr_pass).before(crate::dlss::dlss_pass));
    }
}

/// `NVF_PLAY_DEBUG_AO_STILL=1` (a test aid): the sample pattern stays where it lies under a
/// temporal method too, as before 2026-11-24, for comparing.
fn moving_pattern() -> bool {
    static MOVING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *MOVING.get_or_init(|| {
        let still = std::env::var("NVF_PLAY_DEBUG_AO_STILL").is_ok_and(|v| v.trim() == "1");
        if still {
            info!("ambient occlusion: NVF_PLAY_DEBUG_AO_STILL=1, the sample pattern does not move with the frame");
        }
        !still
    })
}

/// The uniform's `variant.w` for frame `frame`: 0 for the pattern as it lies, else 1 + the
/// frame's number in 0..15 (ssao.wesl `frame_shift`).
pub fn frame_code(frame: u32, moving: bool) -> f32 {
    if moving { (frame % 16 + 1) as f32 } else { 0.0 }
}

/// The camera's projection into the uniform (the aspect follows the window), and the
/// strength and remedies of the placement that runs this frame: on the picture until the
/// lighting hook is in place (or for good if it failed), then the lighting's.
pub(crate) fn follow_projection(mut cams: Query<(&Projection, &mut AmbientOcclusion, Has<bevy::render::camera::TemporalJitter>), With<FlyCam>>, frame: Res<bevy::diagnostic::FrameCount>) {
    let lighting = AO_DEBUG.in_lighting_now();
    for (proj, mut ao, jitters) in &mut cams {
        let Projection::Perspective(p) = proj else { continue };
        let y = 1.0 / (p.fov * 0.5).tan();
        let x = y / p.aspect_ratio.max(1e-3);
        let params = Vec4::new(RADIUS, AO_DEBUG.strength(lighting), p.near, FADE_END);
        let projv = Vec4::new(x, y, max_screen_radius(), FADE_START);
        let mut variant = AO_DEBUG.variant_at(lighting, ao.level()).uniform();
        // The sample pattern moves each frame while the camera jitters (ssao.wesl
        // `frame_shift`): a temporal method then averages what the 4 x 4 denoise cannot.
        variant.w = frame_code(frame.0, jitters && moving_pattern());
        if ao.params != params || ao.proj != projv || ao.variant != variant {
            ao.params = params;
            ao.proj = projv;
            ao.variant = variant;
        }
    }
}

/// Which estimate runs, where and at which level, said once each time the occlusion turns
/// on on the main camera or changes level (for the placement asked for; the extraction says
/// when it goes into the lighting, or that it stays on the picture).
fn log_estimate(cams: Query<&AmbientOcclusion, With<FlyCam>>, mut was: Local<Option<AoLevel>>) {
    let now = cams.iter().next().map(AmbientOcclusion::level);
    if let Some(level) = now.filter(|l| *was != Some(*l)) {
        info!("{}", AO_DEBUG.describe_with(AO_DEBUG.lighting(), Some(level)));
    }
    *was = now;
}

#[derive(Resource)]
pub(crate) struct AoPipelines {
    /// The occlusion pass's layouts with a single-sampled and a multisampled depth.
    occlusion_layouts: [BindGroupLayoutDescriptor; 2],
    /// The apply pass's.
    apply_layouts: [BindGroupLayoutDescriptor; 2],
    /// The multiply pass's before FSR (FSR runs without multisampling).
    multiply_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    occlusion_base: RenderPipelineDescriptor,
    apply_base: RenderPipelineDescriptor,
    /// Occlusion pipeline by multisampling; apply pipeline by target format and it.
    occlusion: HashMap<bool, CachedRenderPipelineId>,
    apply: HashMap<(TextureFormat, bool), CachedRenderPipelineId>,
    multiply_base: RenderPipelineDescriptor,
    /// Before FSR: the factor multiplied into FSR's HDR input and its opaque copy (queued
    /// the first time a view needs it).
    multiply: Option<CachedRenderPipelineId>,
    /// In the lighting: the denoise pass's layouts (single-sampled and multisampled depth),
    /// its base descriptor and its pipelines by multisampling (queued the first time a view
    /// needs one).
    denoise_layouts: [BindGroupLayoutDescriptor; 2],
    denoise_base: RenderPipelineDescriptor,
    denoise: HashMap<bool, CachedRenderPipelineId>,
    /// Under multisampling: the resolve of the depth to one sample ([`resolve_depth`]), its
    /// layout (the multisampled depth and the uniform) and pipeline (queued the first time a
    /// view needs it).
    resolve_layout: BindGroupLayoutDescriptor,
    resolve_base: RenderPipelineDescriptor,
    resolve: Option<CachedRenderPipelineId>,
}

/// A view's pipelines, whether its depth is multisampled, and its occlusion texture.
#[derive(Component)]
pub(crate) struct AoView {
    /// The occlusion, apply and denoise pipelines read the depth the passes are given: the
    /// single-sampled resolve while [`AoView::resolve`] is set, else the prepass's own.
    occlusion: CachedRenderPipelineId,
    apply: CachedRenderPipelineId,
    /// The view's depth prepass is multisampled.
    multi: bool,
    /// Under multisampling, the depth resolved once to one sample for the passes
    /// ([`resolve_depth`]); `None` without multisampling or with `NVF_AO_RESOLVE_DEPTH=0`.
    resolve: Option<AoResolve>,
    /// The picture's format the apply pipeline was built for.
    format: TextureFormat,
    texture: CachedTexture,
    /// Applied before FSR this frame ([`ao_before_fsr`]), not after tonemapping.
    before_fsr: bool,
    /// In the lighting this frame (the view has `NvfAmbientOcclusionBindings`): the denoise
    /// pipeline and the texture it writes, which the bindings point at; the post passes do
    /// nothing then.
    lighting: Option<AoLighting>,
}

/// A view's resolved depth ([`AoView::resolve`]): the pipeline and the single-sampled texture
/// at the depth prepass's size.
pub(crate) struct AoResolve {
    pipeline: CachedRenderPipelineId,
    texture: CachedTexture,
}

impl AoView {
    /// The passes' pipelines read a multisampled depth: the prepass's, not resolved.
    fn reads_multi(&self) -> bool {
        self.multi && self.resolve.is_none()
    }
}

/// A view's part of the occlusion in the lighting ([`AoView::lighting`]).
pub(crate) struct AoLighting {
    denoise: CachedRenderPipelineId,
    /// Per half-resolution texel the denoised factor and the distance (ssao.wesl `denoise`).
    denoised: CachedTexture,
}

/// What [`ao_lighting_pass`] clears the denoised texture to when it cannot run: open (factor
/// 1) at the farthest distance (ssao.wesl `SKY`).
const OPEN_TEXEL: wgpu::Color = wgpu::Color { r: 1.0, g: 60000.0, b: 0.0, a: 0.0 };

/// Render world: the lighting's uniform buffer (made the first time a view goes into the
/// lighting, then kept) and the value last written to it.
#[derive(Resource, Default)]
pub(crate) struct AoLightingGpu {
    buffer: Option<UniformBuffer<AmbientOcclusionLightingUniform>>,
    written: Option<AmbientOcclusionLightingUniform>,
}

/// Whether the main camera's view takes the occlusion in the lighting this frame: asked for
/// in its settings, and the lighting hook in place. A failed hook keeps the post placement.
pub(crate) fn in_lighting(settings: &AmbientOcclusion, hook: HookState) -> bool {
    settings.wants_lighting() && hook == HookState::Installed
}

/// What [`extract_lighting`] says, once per change: on, off, or the hook's failure.
#[derive(Default)]
pub(crate) struct LightingLog {
    on: bool,
    failure_said: bool,
}

/// Give the main camera's view bevy_pbr's `NvfAmbientOcclusionBindings` while its occlusion
/// goes into the lighting ([`in_lighting`]), and take them away otherwise (the row switched
/// off, the mode not asked for). Inserted and removed only here, as bevy_pbr requires
/// (mesh_view_bindings.rs): the pipelines' layout key and the view's bind group are read from
/// it in the same frame. The texture starts as the zero fallback (distance 0: open, see
/// ssao_lighting.wesl) until [`prepare`] points it at this frame's denoised texture.
#[allow(clippy::too_many_arguments)]
fn extract_lighting(
    mut commands: Commands,
    cams: Extract<Query<(RenderEntity, &AmbientOcclusion), With<FlyCam>>>,
    bound: Query<Entity, With<NvfAmbientOcclusionBindings>>,
    mut gpu: ResMut<AoLightingGpu>,
    (device, queue, zero): (Res<RenderDevice>, Res<RenderQueue>, Res<FallbackImageZero>),
    mut log: Local<LightingLog>,
) {
    let hook = hook_state();
    let cams: Vec<(Entity, AmbientOcclusion)> = cams.iter().map(|(e, s)| (e, *s)).collect();
    let bound: Vec<Entity> = bound.iter().collect();
    let plan = lighting_plan(&cams, &bound, hook, &mut log);
    for line in &plan.lines {
        match line {
            LightingLine::On | LightingLine::Off => info!("{}", line.text()),
            LightingLine::HookFailed => {
                let level = cams.first().map_or(AoLevel::Quality, |(_, s)| s.level());
                warn!("{}: {}", line.text(), AO_DEBUG.describe_with(false, Some(level)).trim_start_matches("ambient occlusion: "))
            }
        }
    }
    for e in &plan.remove {
        commands.entity(*e).try_remove::<NvfAmbientOcclusionBindings>();
    }
    let Some(&(_, uniform)) = plan.wanted.first() else { return };
    let gpu = &mut *gpu;
    let buffer = gpu.buffer.get_or_insert_with(|| {
        let mut buffer = UniformBuffer::from(uniform);
        buffer.set_label(Some("nvf_ambient_occlusion_lighting"));
        buffer.write_buffer(&device, &queue);
        buffer
    });
    if gpu.written.is_none() {
        gpu.written = Some(uniform);
    }
    let Some(buffer) = buffer.buffer() else { return };
    for e in &plan.insert {
        commands.entity(*e).try_insert(NvfAmbientOcclusionBindings { uniform: buffer.clone(), texture: zero.texture_view.clone() });
    }
}

/// A line [`extract_lighting`] logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LightingLine {
    On,
    Off,
    HookFailed,
}

impl LightingLine {
    fn text(self) -> &'static str {
        match self {
            LightingLine::On => "ambient occlusion: on the ambient light in the lighting (main camera)",
            LightingLine::Off => "ambient occlusion: off the lighting",
            LightingLine::HookFailed => "ambient occlusion: asked for in the lighting, but the lighting hook is not installed (lights.rs); it stays on the picture",
        }
    }
}

/// What [`extract_lighting`] does this frame.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct LightingPlan {
    /// The views in the lighting, with their uniform.
    wanted: Vec<(Entity, AmbientOcclusionLightingUniform)>,
    /// Views to give the bindings (in the lighting, not bound yet).
    insert: Vec<Entity>,
    /// Views to take them from (bound, no longer in the lighting: the row switched off, the
    /// mode not asked for, the camera gone).
    remove: Vec<Entity>,
    lines: Vec<LightingLine>,
}

/// From the main cameras' render entities and settings, the views bound now and the hook's
/// state: which views go into the lighting and what changes, each log line once per change.
fn lighting_plan(cams: &[(Entity, AmbientOcclusion)], bound: &[Entity], hook: HookState, log: &mut LightingLog) -> LightingPlan {
    let mut plan = LightingPlan {
        wanted: cams.iter().filter(|(_, s)| in_lighting(s, hook)).map(|(e, s)| (*e, AO_DEBUG.lighting_uniform(s))).collect(),
        ..LightingPlan::default()
    };
    if hook == HookState::Failed && !log.failure_said && cams.iter().any(|(_, s)| s.wants_lighting()) {
        plan.lines.push(LightingLine::HookFailed);
        log.failure_said = true;
    }
    plan.remove = bound.iter().copied().filter(|b| !plan.wanted.iter().any(|(w, _)| w == b)).collect();
    plan.insert = plan.wanted.iter().map(|(w, _)| *w).filter(|w| !bound.contains(w)).collect();
    let on = !plan.wanted.is_empty();
    if on != log.on {
        plan.lines.push(if on { LightingLine::On } else { LightingLine::Off });
        log.on = on;
    }
    plan
}

/// FSR's input and its opaque copy: the HDR format FSR requires (fsr.rs `prepare`).
const BEFORE_FSR_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// How open a texel is, and its distance (the apply pass weighs texels by it).
const OCCLUSION_FORMAT: TextureFormat = TextureFormat::Rg16Float;

/// The resolved depth's format: the prepass's (Bevy's `CORE_3D_DEPTH_FORMAT`), so a sample
/// is kept bit for bit and the single-sampled pipelines read it as they read an unsampled
/// prepass.
const RESOLVED_DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;

fn init_pipelines(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>) {
    let occlusion_layout = |multi: bool| {
        let depth = if multi { texture_depth_2d_multisampled() } else { texture_depth_2d() };
        BindGroupLayoutDescriptor::new("ssao_occlusion_layout", &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (depth, uniform_buffer::<AmbientOcclusion>(true))))
    };
    let apply_layout = |multi: bool| {
        let depth = if multi { texture_depth_2d_multisampled() } else { texture_depth_2d() };
        BindGroupLayoutDescriptor::new(
            "ssao_apply_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    depth,
                    uniform_buffer::<AmbientOcclusion>(true),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                ),
            ),
        )
    };
    // The same bindings as `apply`'s, without the picture and its sampler (ssao.wesl).
    let multiply_layout = BindGroupLayoutDescriptor::new(
        "ssao_multiply_layout",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            ((0, texture_depth_2d()), (1, uniform_buffer::<AmbientOcclusion>(true)), (4, texture_2d(TextureSampleType::Float { filterable: false }))),
        ),
    );
    // In the lighting: the same bindings as `multiply`'s, with either depth (ssao.wesl).
    let denoise_layout = |multi: bool| {
        let depth = if multi { texture_depth_2d_multisampled() } else { texture_depth_2d() };
        BindGroupLayoutDescriptor::new(
            "ssao_denoise_layout",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                ((0, depth), (1, uniform_buffer::<AmbientOcclusion>(true)), (4, texture_2d(TextureSampleType::Float { filterable: false }))),
            ),
        )
    };
    // Under multisampling: the resolve's (the multisampled depth and the uniform, ssao.wesl).
    let resolve_layout = BindGroupLayoutDescriptor::new(
        "ssao_resolve_layout",
        &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_depth_2d_multisampled(), uniform_buffer::<AmbientOcclusion>(true))),
    );
    let shader = assets.load("embedded://nvforever/ssao.wesl");
    let base = |label: &'static str, entry: &'static str| RenderPipelineDescriptor {
        label: Some(label.into()),
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState { shader: shader.clone(), entry_point: Some(entry.into()), ..default() }),
        ..default()
    };
    let mut multiply_base = base("ssao_multiply_pipeline", "multiply");
    multiply_base.layout = vec![multiply_layout.clone()];
    if let Some(f) = multiply_base.fragment.as_mut() {
        // The picture times the factor; its alpha (the marks the post passes read) kept. With
        // the grey view (NVF_PLAY_DEBUG_AO) the factor replaces the colour.
        let color = if AO_DEBUG.view {
            BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::Zero, operation: BlendOperation::Add }
        } else {
            BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::Src, operation: BlendOperation::Add }
        };
        let blend = BlendState {
            color,
            alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
        };
        f.targets = vec![Some(ColorTargetState { format: BEFORE_FSR_FORMAT, blend: Some(blend), write_mask: ColorWrites::ALL })];
    }
    // The resolve writes the depth itself: no colour, every pixel written (compare Always).
    let mut resolve_base = base("ssao_resolve_depth_pipeline", "resolve_depth");
    resolve_base.layout = vec![resolve_layout.clone()];
    resolve_base.depth_stencil = Some(DepthStencilState {
        format: RESOLVED_DEPTH_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(CompareFunction::Always),
        stencil: default(),
        bias: default(),
    });
    if let Some(f) = resolve_base.fragment.as_mut() {
        f.shader_defs.push("MULTISAMPLED".into());
    }
    let sampler = device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() });
    commands.insert_resource(AoPipelines {
        occlusion_layouts: [occlusion_layout(false), occlusion_layout(true)],
        apply_layouts: [apply_layout(false), apply_layout(true)],
        multiply_layout,
        sampler,
        occlusion_base: base("ssao_occlusion_pipeline", "occlusion"),
        apply_base: base("ssao_apply_pipeline", "apply"),
        occlusion: HashMap::default(),
        apply: HashMap::default(),
        multiply_base,
        multiply: None,
        denoise_layouts: [denoise_layout(false), denoise_layout(true)],
        denoise_base: base("ssao_denoise_pipeline", "denoise"),
        denoise: HashMap::default(),
        resolve_layout,
        resolve_base,
        resolve: None,
    });
}

/// A pipeline of `base` drawing into an [`OCCLUSION_FORMAT`] texture, with `layout` and the
/// `MULTISAMPLED` def for a multisampled depth.
fn occlusion_format_pipeline(base: &RenderPipelineDescriptor, layout: &BindGroupLayoutDescriptor, multi: bool) -> RenderPipelineDescriptor {
    let mut d = base.clone();
    d.layout = vec![layout.clone()];
    if let Some(f) = d.fragment.as_mut() {
        f.targets = vec![Some(ColorTargetState { format: OCCLUSION_FORMAT, blend: None, write_mask: ColorWrites::ALL })];
        if multi {
            f.shader_defs.push("MULTISAMPLED".into());
        }
    }
    d
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<AoPipelines>,
    mut textures: ResMut<TextureCache>,
    (device, queue): (Res<RenderDevice>, Res<RenderQueue>),
    mut views: Query<(Entity, &ExtractedView, &Msaa, &AmbientOcclusion, Option<&mut NvfAmbientOcclusionBindings>, Option<&ExtractedCamera>)>,
    stale: Query<Entity, (With<AoView>, Without<AmbientOcclusion>)>,
    mut orphans: Query<&mut NvfAmbientOcclusionBindings, Without<AmbientOcclusion>>,
    mut gpu: ResMut<AoLightingGpu>,
    zero: Res<FallbackImageZero>,
    mut resolving: Local<Option<bool>>,
) {
    // The row switched off while the game runs: the view's extracted settings go, but what
    // this system and the uniform plugin put on the view stays, and the passes would go on
    // drawing with an offset into a uniform buffer nothing writes any more. The lighting's
    // part (`AoView::lighting`, its texture) goes with it; its bindings go in the extraction.
    for entity in &stale {
        commands.entity(entity).remove::<(AoView, DynamicUniformIndex<AmbientOcclusion>)>();
    }
    // Bindings without settings (not met: the extraction takes both away in the same frame)
    // read the zero fallback, which is open, never a texture nothing writes.
    for mut b in &mut orphans {
        if b.texture.id() != zero.texture_view.id() {
            b.texture = zero.texture_view.clone();
        }
    }
    for (entity, view, msaa, settings, bindings, camera) in &mut views {
        let before_fsr = settings.runs_before_fsr();
        if before_fsr && pipelines.multiply.is_none() {
            pipelines.multiply = Some(cache.queue_render_pipeline(pipelines.multiply_base.clone()));
        }
        let multi = msaa.samples() > 1;
        // Under multisampling the depth resolved once, at the prepass's size (Bevy's
        // `prepare_prepass_textures`: the camera's physical target size), and the passes read
        // that with their single-sampled pipelines.
        let resolve = match camera.and_then(|c| c.physical_target_size) {
            Some(target) if multi && *AO_RESOLVE_DEPTH => {
                let pipeline = match pipelines.resolve {
                    Some(id) => id,
                    None => {
                        let id = cache.queue_render_pipeline(pipelines.resolve_base.clone());
                        pipelines.resolve = Some(id);
                        id
                    }
                };
                let texture = textures.get(
                    &device,
                    TextureDescriptor {
                        label: Some("ssao_resolved_depth"),
                        size: Extent3d { width: target.x, height: target.y, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format: RESOLVED_DEPTH_FORMAT,
                        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                );
                Some(AoResolve { pipeline, texture })
            }
            _ => None,
        };
        let reads_multi = multi && resolve.is_none();
        let format = view.target_format;
        let occlusion = match pipelines.occlusion.get(&reads_multi) {
            Some(id) => *id,
            None => {
                let id = cache.queue_render_pipeline(occlusion_format_pipeline(&pipelines.occlusion_base, &pipelines.occlusion_layouts[reads_multi as usize], reads_multi));
                pipelines.occlusion.insert(reads_multi, id);
                id
            }
        };
        let apply = match pipelines.apply.get(&(format, reads_multi)) {
            Some(id) => *id,
            None => {
                let mut d = pipelines.apply_base.clone();
                d.layout = vec![pipelines.apply_layouts[reads_multi as usize].clone()];
                if let Some(f) = d.fragment.as_mut() {
                    f.targets = vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })];
                    if reads_multi {
                        f.shader_defs.push("MULTISAMPLED".into());
                    }
                }
                let id = cache.queue_render_pipeline(d);
                pipelines.apply.insert((format, reads_multi), id);
                id
            }
        };
        // Half resolution, rounded up (texel q stands for the picture's pixel 2q); before FSR
        // (and in the lighting under an upscaler), half its render size.
        let picture = if before_fsr { settings.area.truncate().truncate().as_uvec2() } else { view.viewport.zw() };
        let size = ((picture + UVec2::ONE) / 2).max(UVec2::ONE);
        let half = |label: &'static str| TextureDescriptor {
            label: Some(label),
            size: Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: OCCLUSION_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let texture = textures.get(&device, half("ssao_openness"));
        // In the lighting: the denoised texture, bound for this frame's main pass (before
        // RenderSystems::PrepareBindGroups, as bevy_pbr allows), and the uniform.
        let lighting = bindings.map(|mut b| {
            let denoise = match pipelines.denoise.get(&reads_multi) {
                Some(id) => *id,
                None => {
                    let id = cache.queue_render_pipeline(occlusion_format_pipeline(&pipelines.denoise_base, &pipelines.denoise_layouts[reads_multi as usize], reads_multi));
                    pipelines.denoise.insert(reads_multi, id);
                    id
                }
            };
            let denoised = textures.get(&device, half("ssao_denoised"));
            if b.texture.id() != denoised.default_view.id() {
                b.texture = denoised.default_view.clone();
            }
            let uniform = AO_DEBUG.lighting_uniform(settings);
            let gpu = &mut *gpu;
            let stale_uniform = gpu.written != Some(uniform);
            match gpu.buffer.as_mut() {
                Some(buffer) if stale_uniform => {
                    buffer.set(uniform);
                    buffer.write_buffer(&device, &queue);
                    gpu.written = Some(uniform);
                }
                _ => {}
            }
            AoLighting { denoise, denoised }
        });
        // Said once per change: whether the passes read the depth resolved to one sample.
        if multi && *resolving != Some(resolve.is_some()) {
            if resolve.is_some() {
                info!("ambient occlusion: the multisampled depth resolved once to one sample for the passes");
            } else {
                let why = if *AO_RESOLVE_DEPTH { " (the view has no target size yet)" } else { " (NVF_AO_RESOLVE_DEPTH=0)" };
                info!("ambient occlusion: the passes read the multisampled depth{why}");
            }
            *resolving = Some(resolve.is_some());
        }
        commands.entity(entity).insert(AoView { occlusion, apply, multi, resolve, format, texture, before_fsr, lighting });
    }
}

/// Whether a pipeline built for `format` may draw into the view's picture now. In the
/// frame the view turns HDR or back (the light glow switched, `graphics.rs`) the view's
/// format and its main texture can disagree for a frame; a pipeline set on a pass of
/// the other format is a validation error that ends the run, so the pass is skipped.
pub(crate) fn format_matches(target: &ViewTarget, format: TextureFormat) -> bool {
    target.main_texture_format() == format
}

/// Whether the view's depth prepass texture is multisampled this frame. Read from the
/// texture itself: in the frame the multisampling changes (temporal anti-aliasing switched
/// on or off, `graphics.rs`) the view's `Msaa` and its prepass texture may disagree, and a
/// pass bound to the other kind of texture is a validation error that ends the run.
pub(crate) fn depth_multisampled(prepass: &ViewPrepassTextures) -> Option<bool> {
    prepass.depth.as_ref().map(|d| d.texture.texture.sample_count() > 1)
}

/// The depth the passes read this frame, `None` when they cannot run: the prepass's own, or,
/// while the view resolves it ([`AoView::resolve`]), the prepass's multisampled depth
/// resolved here to one sample, sample 0 as every pass read it (ssao.wesl `resolve_depth`).
/// The passes then read single-sampled texels close together in memory instead of sample 0
/// of the multisampled texture at each of their 26 reads a half-resolution texel
/// (docs/design/frame-rate-audit.md §9: 0.3 of AO's 0.6 ms at 1440p 4x) [engineering
/// choice]. Checked against the prepass texture itself: in the frame the multisampling or
/// the window's size changes the view's settings and its textures can disagree.
#[allow(clippy::too_many_arguments)]
fn resolve_depth<'a>(
    ctx: &mut RenderContext,
    device: &RenderDevice,
    pipelines: &AoPipelines,
    cache: &PipelineCache,
    ao: &'a AoView,
    prepass: &'a ViewPrepassTextures,
    binding: BindingResource<'_>,
    index: u32,
) -> Option<&'a TextureView> {
    let Some(resolve) = ao.resolve.as_ref() else {
        return prepass.depth_only_view().filter(|_| depth_multisampled(prepass) == Some(ao.multi));
    };
    let source = prepass.depth.as_ref()?;
    if source.texture.texture.sample_count() <= 1 || source.texture.texture.size() != resolve.texture.texture.size() {
        return None;
    }
    let depth = prepass.depth_only_view()?;
    let pipeline = cache.get_render_pipeline(resolve.pipeline)?;
    let layout = cache.get_bind_group_layout(&pipelines.resolve_layout);
    let bind_group = device.create_bind_group("ssao_resolve_bind_group", &layout, &BindGroupEntries::sequential((depth, binding)));
    let ops = Operations { load: LoadOp::Clear(0.0), store: StoreOp::Store };
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("ssao_resolve_depth_pass"),
        color_attachments: &[],
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment { view: &resolve.texture.default_view, depth_ops: Some(ops), stencil_ops: None }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[index]);
    pass.draw(0..3, 0..1);
    drop(pass);
    Some(&resolve.texture.default_view)
}

/// The occlusion into the view's texture.
#[allow(clippy::too_many_arguments)]
fn occlusion_pass(
    ctx: &mut RenderContext,
    device: &RenderDevice,
    layout: &BindGroupLayout,
    pipeline: &RenderPipeline,
    depth: &TextureView,
    binding: BindingResource<'_>,
    index: u32,
    ao: &AoView,
) {
    let bind_group = device.create_bind_group("ssao_occlusion_bind_group", layout, &BindGroupEntries::sequential((depth, binding)));
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("ssao_occlusion_pass"),
        color_attachments: &[Some(RenderPassColorAttachment { view: &ao.texture.default_view, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[index]);
    pass.draw(0..3, 0..1);
}

/// While a temporal upscaler upscales: the occlusion worked out at its render size from the
/// depth as drawn (before the common passes expand it) and multiplied into the render-size
/// rectangle of the HDR picture the upscaler reads, and, under FSR, of FSR's opaque copy, so
/// its masks compare the two as before. The upscaler then accumulates it with everything
/// else: the occlusion is jittered with the scene and smoothed by it, at about half the cost
/// of the pass at the display size (FSR Quality: the render size has 0.44 of the pixels). It
/// darkens the HDR light before tonemapping, so bright surfaces lose less than when it is
/// multiplied into the tonemapped picture. Runs before every upscaler's pass
/// (`fsr::fsr_pass`, `dlss::dlss_pass`).
#[allow(clippy::type_complexity)]
pub(crate) fn ao_before_fsr(
    view: ViewQuery<(&ViewTarget, &DynamicUniformIndex<AmbientOcclusion>, &AoView, &ViewPrepassTextures, &crate::upscaler::prep::PreparedUpscale, Option<&crate::fsr::PreparedFsr>)>,
    pipelines: Res<AoPipelines>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<AmbientOcclusion>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, index, ao, prepass, prepared, fsr) = view.into_inner();
    // In the lighting the factor is on the ambient light already ([`ao_lighting_pass`]).
    if ao.lighting.is_some() || !ao.before_fsr || depth_multisampled(prepass) != Some(false) || !format_matches(target, BEFORE_FSR_FORMAT) {
        return;
    }
    let Some(depth) = prepass.depth_only_view() else { return };
    let Some(multiply) = pipelines.multiply.and_then(|id| cache.get_render_pipeline(id)) else { return };
    let Some(occlusion) = cache.get_render_pipeline(ao.occlusion) else { return };
    let Some(binding) = uniforms.uniforms().binding() else { return };
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_ambient_occlusion");
    let layout = cache.get_bind_group_layout(&pipelines.occlusion_layouts[0]);
    occlusion_pass(&mut ctx, &device, &layout, occlusion, depth, binding.clone(), index.index(), ao);
    let layout = cache.get_bind_group_layout(&pipelines.multiply_layout);
    let bind_group = device.create_bind_group("ssao_multiply_bind_group", &layout, &BindGroupEntries::with_indices(((0, depth), (1, binding), (4, &ao.texture.default_view))));
    // FSR's opaque copy, only where FSR draws: DLSS has none.
    let opaque = fsr.map(|fsr| fsr.opaque().create_view(&default()));
    let picture: &wgpu::TextureView = target.main_texture_view();
    let size = prepared.render_size.as_vec2();
    // Loaded, not cleared (wgpu's default operations clear): the blend multiplies what is there.
    let ops = Operations { load: LoadOp::Load, store: StoreOp::Store };
    let targets = std::iter::once(("ssao_multiply_picture", picture)).chain(opaque.as_ref().map(|opaque| ("ssao_multiply_fsr_opaque", opaque)));
    for (label, view) in targets {
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(0.0, 0.0, size.x, size.y, 0.0, 1.0);
        pass.set_pipeline(multiply);
        pass.set_bind_group(0, &bind_group, &[index.index()]);
        pass.draw(0..3, 0..1);
    }
    span.end(ctx.command_encoder());
}

/// The occlusion into the view's texture, then the picture multiplied by it.
#[allow(clippy::type_complexity)]
pub(crate) fn ao_pass(
    view: ViewQuery<(&ViewTarget, &DynamicUniformIndex<AmbientOcclusion>, &AoView, &ViewPrepassTextures, Has<crate::upscaler::prep::PreparedUpscale>)>,
    pipelines: Res<AoPipelines>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<AmbientOcclusion>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, index, ao, prepass, upscaled) = view.into_inner();
    // Applied before the upscaler already ([`ao_before_fsr`]), or in the lighting
    // ([`ao_lighting_pass`]).
    if (ao.before_fsr && upscaled) || ao.lighting.is_some() {
        return;
    }
    if !format_matches(target, ao.format) {
        return;
    }
    let (Some(occlusion), Some(apply)) = (cache.get_render_pipeline(ao.occlusion), cache.get_render_pipeline(ao.apply)) else { return };
    let Some(binding) = uniforms.uniforms().binding() else { return };
    // GPU time of the pass (recorded with --log-frame-times, graphics.rs).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_ambient_occlusion");
    let Some(depth) = resolve_depth(&mut ctx, &device, &pipelines, &cache, ao, prepass, binding.clone(), index.index()) else {
        span.end(ctx.command_encoder());
        return;
    };
    let layout = cache.get_bind_group_layout(&pipelines.occlusion_layouts[ao.reads_multi() as usize]);
    occlusion_pass(&mut ctx, &device, &layout, occlusion, depth, binding.clone(), index.index(), ao);
    let post = target.post_process_write();
    let layout = cache.get_bind_group_layout(&pipelines.apply_layouts[ao.reads_multi() as usize]);
    let bind_group = device.create_bind_group(
        "ssao_apply_bind_group",
        &layout,
        &BindGroupEntries::sequential((depth, binding, post.source, &pipelines.sampler, &ao.texture.default_view)),
    );
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("ssao_apply_pass"),
        color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(apply);
    pass.set_bind_group(0, &bind_group, &[index.index()]);
    pass.draw(0..3, 0..1);
    drop(pass);
    span.end(ctx.command_encoder());
}

/// In the lighting: between the depth prepass and the main pass, the occlusion into the
/// view's texture, then the denoised factor per half-resolution texel with its distance
/// (ssao.wesl `denoise`) into the texture the view's mesh bind group holds at binding 44.
/// Every frame the view is in the lighting that texture is written whole: by the passes, or,
/// when they cannot run (a pipeline still compiling, the depth's sample count changing for a
/// frame, no uniform), cleared to open, so the main pass never reads a stale or undefined
/// texture. The settings' uniform offset and the prepass textures are optional here for that
/// reason: the view is never left out of this system while its bindings point at the texture.
#[allow(clippy::type_complexity)]
pub(crate) fn ao_lighting_pass(
    view: ViewQuery<(&AoView, Option<&DynamicUniformIndex<AmbientOcclusion>>, Option<&ViewPrepassTextures>)>,
    pipelines: Res<AoPipelines>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<AmbientOcclusion>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (ao, index, prepass) = view.into_inner();
    let Some(lighting) = ao.lighting.as_ref() else { return };
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_ambient_occlusion");
    let ready = prepass.and_then(|p| {
        Some((p, cache.get_render_pipeline(ao.occlusion)?, cache.get_render_pipeline(lighting.denoise)?, uniforms.uniforms().binding()?, index?.index()))
    });
    let ready = ready.and_then(|(p, occlusion, denoise, binding, index)| {
        let depth = resolve_depth(&mut ctx, &device, &pipelines, &cache, ao, p, binding.clone(), index)?;
        Some((depth, occlusion, denoise, binding, index))
    });
    let Some((depth, occlusion, denoise, binding, index)) = ready else {
        clear_open(&mut ctx, &lighting.denoised);
        span.end(ctx.command_encoder());
        return;
    };
    let layout = cache.get_bind_group_layout(&pipelines.occlusion_layouts[ao.reads_multi() as usize]);
    occlusion_pass(&mut ctx, &device, &layout, occlusion, depth, binding.clone(), index, ao);
    let layout = cache.get_bind_group_layout(&pipelines.denoise_layouts[ao.reads_multi() as usize]);
    let bind_group = device.create_bind_group("ssao_denoise_bind_group", &layout, &BindGroupEntries::with_indices(((0, depth), (1, binding), (4, &ao.texture.default_view))));
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("ssao_denoise_pass"),
        color_attachments: &[Some(RenderPassColorAttachment { view: &lighting.denoised.default_view, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(denoise);
    pass.set_bind_group(0, &bind_group, &[index]);
    pass.draw(0..3, 0..1);
    drop(pass);
    span.end(ctx.command_encoder());
}

/// The denoised texture cleared to open ([`OPEN_TEXEL`]).
fn clear_open(ctx: &mut RenderContext, texture: &CachedTexture) {
    let ops = Operations { load: LoadOp::Clear(OPEN_TEXEL), store: StoreOp::Store };
    let pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("ssao_denoised_clear"),
        color_attachments: &[Some(RenderPassColorAttachment { view: &texture.default_view, depth_slice: None, resolve_target: None, ops })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    drop(pass);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ssao.wesl as the game's WESL loader would hand it to naga for one value of the
    /// `MULTISAMPLED` condition: the two imports replaced by stand-ins of the same signature
    /// (Bevy's fullscreen vertex output; nvf_assets' `unsqueeze`, its body left out), the
    /// conditional bindings resolved.
    fn resolved_shader(multisampled: bool) -> String {
        let source = include_str!("ssao.wesl").replace("\r\n", "\n");
        let mut out = String::from(
            "struct FullscreenVertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32>, };\nfn unsqueeze(d: f32) -> f32 { return d; }\n",
        );
        for line in source.lines() {
            if line.starts_with("import ") {
                continue;
            }
            let line = if let Some(rest) = line.strip_prefix("@if(MULTISAMPLED) ") {
                if !multisampled {
                    continue;
                }
                rest
            } else if let Some(rest) = line.strip_prefix("@if(!MULTISAMPLED) ") {
                if multisampled {
                    continue;
                }
                rest
            } else {
                line
            };
            assert!(line.trim_start().starts_with("//") || !line.contains("@if"), "a condition the test does not resolve: {line}");
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_shader_is_valid_wgsl_with_either_depth() {
        use wgpu::naga;
        for multisampled in [false, true] {
            let source = resolved_shader(multisampled);
            let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("multisampled {multisampled}: {}", e.emit_to_string(&source)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
                .validate(&module)
                .unwrap_or_else(|e| panic!("multisampled {multisampled}: {e:?}"));
            for entry in ["occlusion", "apply", "multiply", "denoise", "resolve_depth"] {
                assert!(module.entry_points.iter().any(|e| e.name == entry), "entry point {entry}");
            }
        }
    }

    #[test]
    fn the_shader_declares_what_the_rust_side_mirrors() {
        let source = include_str!("ssao.wesl");
        for line in [format!("const SLICES: i32 = {GTAO_SLICES};"), format!("const STEPS: i32 = {GTAO_STEPS};")] {
            assert!(source.contains(&line), "ssao.wesl must declare `{line}` (ssao.rs mirrors it)");
        }
        // The uniform's six fields in the order of the Rust struct.
        let fields = ["params: vec4<f32>,", "proj: vec4<f32>,", "render_scale: vec4<f32>,", "area: vec4<f32>,", "mode: vec4<f32>,", "variant: vec4<f32>,"];
        let at: Vec<usize> = fields.iter().map(|f| source.find(f).unwrap_or_else(|| panic!("{f}"))).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "the uniform's fields in the Rust order");
        assert_eq!(<AmbientOcclusion as ShaderType>::min_size().get(), 6 * 16, "six vec4 fields, nothing else");
        // The variant flags.
        for (name, value) in [("VARIANT_WIDE", VARIANT_WIDE), ("VARIANT_WIDE6", VARIANT_WIDE6), ("VARIANT_SOFT", VARIANT_SOFT), ("VARIANT_THIN", VARIANT_THIN), ("VARIANT_NEAREST", VARIANT_NEAREST)] {
            let line = format!("const {name}: u32 = {value}u;");
            assert!(source.contains(&line), "ssao.wesl must declare `{line}` (ssao.rs mirrors it)");
        }
    }

    #[test]
    fn the_variant_aid_takes_names_joined_by_plus() {
        for off in [None, Some(""), Some("0"), Some("off"), Some(" None ")] {
            assert_eq!(AoVariant::parse(off), Ok(AoVariant::default()), "{off:?}");
        }
        let one = |name: &str| AoVariant::parse(Some(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(one("slices3"), AoVariant { slices: Some(3), ..AoVariant::default() });
        assert_eq!(one("slices4"), AoVariant { slices: Some(4), ..AoVariant::default() });
        assert_eq!(one("steps8"), AoVariant { steps: Some(8), ..AoVariant::default() });
        assert_eq!(one("wide"), AoVariant { wide: Some(8), ..AoVariant::default() });
        assert_eq!(one("wide6"), AoVariant { wide: Some(6), ..AoVariant::default() });
        assert_eq!(one("soft"), AoVariant { soft: true, ..AoVariant::default() });
        assert_eq!(one("thin"), AoVariant { thin: true, ..AoVariant::default() });
        assert_eq!(one("nearest"), AoVariant { nearest: true, ..AoVariant::default() });
        let all = AoVariant { slices: Some(4), steps: Some(8), wide: Some(8), soft: true, thin: true, nearest: true };
        assert_eq!(one(" Thin + WIDE+soft+nearest+steps8+slices4 "), all);
        // Each name written back as it is taken, in one order.
        for name in AoVariant::NAMES {
            assert_eq!(one(name).name(), name);
        }
        assert_eq!(all.name(), "slices4+steps8+wide+soft+thin+nearest");
        assert_eq!(one("soft+slices3").name(), "slices3+soft");
        for bad in ["slices5", "wide+wide", "slices3+slices4", "wide+wide6", "soft+", "+thin", "soft thin", "old", "nearest+nearest"] {
            assert_eq!(AoVariant::parse(Some(bad)), Err(bad.to_string()), "{bad}");
        }
    }

    #[test]
    fn the_variant_aid_tells_unset_from_none() {
        // Unset or empty: the placement's default; `none` (`0`, `off`): the plain estimate.
        for unset in [None, Some(""), Some("  ")] {
            assert_eq!(AoVariant::parse_aid(unset), Ok(None), "{unset:?}");
        }
        for none in ["none", "0", "off", " None "] {
            assert_eq!(AoVariant::parse_aid(Some(none)), Ok(Some(AoVariant::default())), "{none}");
        }
        assert_eq!(AoVariant::parse_aid(Some("thin+soft")), Ok(Some(LIGHTING_VARIANT)));
        assert_eq!(AoVariant::parse_aid(Some("thick")), Err("thick".to_string()));
        assert_eq!(LIGHTING_VARIANT.name(), "soft+thin");
    }

    #[test]
    fn the_variant_reaches_the_uniform_and_the_log_line() {
        // On the picture with none set: the uniform's variant all 0 (the shader's own SLICES,
        // STEPS, no flags) and stage one's log line.
        let post = AoDebug { post: true, ..AoDebug::default() };
        let shipped = AmbientOcclusion::with_debug(post, false);
        assert_eq!(shipped.variant, Vec4::ZERO);
        // In the lighting with none set: thin and soft.
        let lighting = AmbientOcclusion::with_debug(AoDebug::default(), true);
        assert_eq!(lighting.variant, Vec4::new(0.0, 0.0, (VARIANT_THIN | VARIANT_SOFT) as f32, 0.0));
        let with = |variant: &str| AoDebug { variant: AoVariant::parse_aid(Some(variant)).unwrap(), ..post };
        let picture = |d: AoDebug| AmbientOcclusion::with_debug(d, false);
        assert_eq!(picture(with("slices3")).variant, Vec4::new(3.0, 0.0, 0.0, 0.0));
        assert_eq!(picture(with("slices4+steps8")).variant, Vec4::new(4.0, 8.0, 0.0, 0.0));
        assert_eq!(picture(with("wide")).variant, Vec4::new(0.0, 0.0, VARIANT_WIDE as f32, 0.0));
        assert_eq!(picture(with("wide6")).variant, Vec4::new(0.0, 0.0, VARIANT_WIDE6 as f32, 0.0));
        assert_eq!(picture(with("wide+soft+thin")).variant.z, (VARIANT_WIDE | VARIANT_SOFT | VARIANT_THIN) as f32);
        assert_eq!(picture(with("nearest+wide6")).variant.z, (VARIANT_NEAREST | VARIANT_WIDE6) as f32);
        // `none` in the lighting: the plain estimate there too; a name: that one, not thin+soft.
        let none = AoDebug { variant: Some(AoVariant::default()), ..AoDebug::default() };
        assert_eq!(AmbientOcclusion::with_debug(none, true).variant, Vec4::ZERO);
        assert_eq!(AmbientOcclusion::with_debug(AoDebug { variant: Some(AoVariant::parse(Some("wide")).unwrap()), ..AoDebug::default() }, true).variant.z, VARIANT_WIDE as f32);
        // The old estimate gets none of it.
        assert_eq!(picture(AoDebug { old: true, ..with("nearest+slices4") }).variant, Vec4::ZERO);
        assert_eq!(picture(AoDebug { old: true, ..post }).variant, Vec4::ZERO);
        // Nothing else moves with it.
        let soft = picture(with("soft"));
        assert_eq!((soft.params, soft.proj, soft.mode), (shipped.params, shipped.proj, shipped.mode));
        assert_eq!(
            with("slices4+wide").describe(false),
            "ambient occlusion: ground-truth estimate (GTAO), 4 slices x 4 steps a side at half resolution, depth-aware denoise, variant slices4+wide (NVF_PLAY_DEBUG_AO_VARIANT), on the picture (NVF_PLAY_DEBUG_AO=post)"
        );
        assert!(with("steps8").describe(false).contains("2 slices x 8 steps a side"));
        let strong = AoDebug { gtao_strength: Some(0.6), ..with("soft") };
        assert!(strong.describe(false).ends_with("denoise, variant soft (NVF_PLAY_DEBUG_AO_VARIANT), strength 0.6 (NVF_PLAY_DEBUG_AO_STRENGTH), on the picture (NVF_PLAY_DEBUG_AO=post)"));
        assert_eq!(
            AoDebug { old: true, ..with("thin") }.describe(false),
            "ambient occlusion: old estimate (12-sample spiral, NVF_PLAY_DEBUG_AO), variant thin ignored (NVF_PLAY_DEBUG_AO_VARIANT)"
        );
        // The lighting's line names the remedies that run: the default without the aid's
        // name, `none` and a chosen one with it.
        assert!(AoDebug::default().describe(true).contains("depth-aware denoise, variant soft+thin, strength 0.92, in the lighting"));
        assert!(none.describe(true).contains("depth-aware denoise, variant none (NVF_PLAY_DEBUG_AO_VARIANT), strength 0.92"));
        assert!(AoDebug { variant: Some(AoVariant::default()), ..post }.describe(false).contains("denoise, variant none (NVF_PLAY_DEBUG_AO_VARIANT), on the picture"));
    }

    /// ssao.wesl `pattern`: the 4 x 4 ordered turns (0..1).
    fn pattern_turns() -> [f32; 16] {
        let b = [0.0f32, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0];
        b.map(|v| (v + 0.5) / 16.0)
    }

    /// ssao.wesl's slice angle: slice k of n at (k + turn) pi / n.
    fn slice_angle(k: u32, turn: f32, slices: u32) -> f32 {
        (k as f32 + turn) * std::f32::consts::PI / slices as f32
    }

    #[test]
    fn the_slices_stay_evenly_spread_for_every_count() {
        let source = include_str!("ssao.wesl");
        assert!(source.contains("var b = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);"));
        assert!(source.contains("let phi = (f32(k) + turn) * PI / f32(slices);"));
        use std::f32::consts::PI;
        for slices in [2u32, 3, 4] {
            // Over the 4 x 4 texels the denoise reads: 16 x n directions, evenly spread over
            // the half turn (a slice is a line, both sides marched), one step apart.
            let mut angles: Vec<f32> = pattern_turns().iter().flat_map(|&t| (0..slices).map(move |k| slice_angle(k, t, slices))).collect();
            angles.sort_by(f32::total_cmp);
            let step = PI / (16 * slices) as f32;
            assert!((angles[0] - step * 0.5).abs() < 1e-5, "{slices} slices: first {}", angles[0]);
            for w in angles.windows(2) {
                assert!((w[1] - w[0] - step).abs() < 1e-5, "{slices} slices: gap {}", w[1] - w[0]);
            }
            assert!(*angles.last().unwrap() < PI);
            // Within one texel the slices are a half turn / n apart.
            for t in pattern_turns() {
                for k in 1..slices {
                    assert!((slice_angle(k, t, slices) - slice_angle(k - 1, t, slices) - PI / slices as f32).abs() < 1e-5);
                }
            }
        }
    }

    /// ssao.wesl `gtao_visibility` for a pixel on an endless plane with normal `n`, seen
    /// along `v` (from the pixel to the camera), with nothing else in the way: each slice's
    /// horizons raised (or, `thin`, lowered) by samples of the plane itself on both sides at
    /// the shader's step distances, then the ratio of the visible arcs to the open ones.
    /// `wall` instead puts a horizon at the view vector on the side towards each slice's
    /// direction, the depth closing half of every slice there.
    fn gtao_on_plane(n: Vec3, v: Vec3, slices: u32, turn: f32, offset: f32, thin: bool, wall: bool) -> f32 {
        use std::f32::consts::FRAC_PI_2;
        const HORIZON_BIAS: f32 = 0.03;
        const FALLOFF_START: f32 = 0.25;
        const THIN_DECAY: f32 = 0.25;
        let steps = 4;
        let (mut visible, mut unblocked) = (0.0f32, 0.0f32);
        for k in 0..slices {
            let phi = slice_angle(k, turn, slices);
            let along = Vec3::new(phi.cos(), -phi.sin(), 0.0);
            let tangent = (along - v * along.dot(v)).normalize();
            let (nt, nv) = (n.dot(tangent), n.dot(v));
            let np_len = Vec2::new(nt, nv).length();
            let gamma = if np_len > 1e-6 { nt.atan2(nv) } else { 0.0 };
            let (sin_g, cos_g) = gamma.sin_cos();
            // The plane's own points in the slice: the line where the slice's plane (through
            // the pixel, spanned by v and the tangent) meets the surface.
            // (None when the surface stands across the slice: np_len is 0, the slice counts nothing.)
            let line = n.cross(v.cross(tangent)).normalize_or_zero();
            let line = if line.dot(tangent) >= 0.0 { line } else { -line };
            let (mut c_pos, mut c_neg) = (-sin_g, sin_g);
            for j in 0..steps {
                let f = (j as f32 + offset) / steps as f32;
                // A share of the radius out along the plane, as the screen's step reaches.
                let share = 0.05 + f * f * 0.9;
                let weight = 1.0 - smooth(FALLOFF_START, 1.0, share * share);
                // ssao.wesl `horizon_cosine`: mix(lowest, cosine - bias, weight).
                let s_pos = -sin_g + weight * (line.dot(v) - HORIZON_BIAS + sin_g);
                let s_neg = sin_g + weight * ((-line).dot(v) - HORIZON_BIAS - sin_g);
                let rise = |h: f32, s: f32, lowest: f32| {
                    if !thin {
                        h.max(s)
                    } else if s > h {
                        s
                    } else {
                        h + (s.max(lowest) - h) * THIN_DECAY
                    }
                };
                c_pos = rise(c_pos, s_pos, -sin_g);
                c_neg = rise(c_neg, s_neg, sin_g);
            }
            if wall {
                c_pos = 1.0;
            }
            let h_neg = (-c_neg.clamp(-1.0, 1.0).acos()).max(gamma - FRAC_PI_2);
            let h_pos = c_pos.clamp(-1.0, 1.0).acos().min(gamma + FRAC_PI_2);
            visible += np_len * (visible_arc(h_neg, gamma) + visible_arc(h_pos, gamma));
            unblocked += np_len * (cos_g + gamma * sin_g);
        }
        if unblocked <= 1e-5 { 1.0 } else { (visible / unblocked).clamp(0.0, 1.0) }
    }

    /// WGSL's smoothstep.
    fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    #[test]
    fn flat_ground_stays_unshaded_for_every_slice_count() {
        let source = include_str!("ssao.wesl");
        for line in ["const HORIZON_BIAS: f32 = 0.03;", "const FALLOFF_START: f32 = 0.25;", "const THIN_DECAY: f32 = 0.25;"] {
            assert!(source.contains(line), "the mirror's constant: {line}");
        }
        // Views from straight on to grazing, from the screen's centre and its corners
        // (view space, y up, the camera looking down -z: v points back at the camera).
        let views = [Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.4, 0.3, 1.0).normalize(), Vec3::new(-0.6, 0.35, 1.0).normalize()];
        let normals = [Vec3::new(0.0, 1.0, 0.1), Vec3::new(0.0, 0.9, 0.3), Vec3::new(0.7, 0.2, 0.6), Vec3::new(-0.3, -0.5, 0.8), Vec3::new(0.0, 0.0, 1.0)];
        for slices in [2u32, 3, 4] {
            for thin in [false, true] {
                for v in views {
                    for n in normals {
                        let n = n.normalize();
                        // The normal faces the camera (ssao.wesl `normal_at` turns it so).
                        let n = if n.dot(v) < 0.0 { -n } else { n };
                        for (i, turn) in pattern_turns().into_iter().enumerate() {
                            let offset = pattern_turns()[(i * 7 + 5) % 16];
                            let open = gtao_on_plane(n, v, slices, turn, offset, thin, false);
                            assert!((open - 1.0).abs() < 1e-4, "{slices} slices, thin {thin}, v {v}, n {n}, turn {turn}: {open}");
                        }
                    }
                }
            }
            // Not open by construction: a surface facing the camera with half of every
            // slice closed keeps half, for every slice count.
            let half = gtao_on_plane(Vec3::Z, Vec3::Z, slices, 0.25, 0.5, false, true);
            assert!((half - 0.5).abs() < 1e-4, "{slices} slices: {half}");
        }
    }

    #[test]
    fn the_debug_switch_picks_the_placement_the_estimate_and_the_view() {
        // Unset or off: the default, in the lighting.
        for off in [None, Some(""), Some("0"), Some("off"), Some("false"), Some(" OFF ")] {
            assert_eq!(AoDebug::parse(off), Ok(AoDebug::default()), "{off:?}");
        }
        assert!(AoDebug::default().lighting());
        assert_eq!(AoDebug::parse(Some("view-lighting")), Ok(AoDebug { view: true, ..AoDebug::default() }));
        // On the picture: post, and the old estimate and the grey views as before.
        assert_eq!(AoDebug::parse(Some("post")), Ok(AoDebug { post: true, ..AoDebug::default() }));
        assert_eq!(AoDebug::parse(Some("old")), Ok(AoDebug { old: true, post: true, ..AoDebug::default() }));
        assert_eq!(AoDebug::parse(Some("view")), Ok(AoDebug { view: true, post: true, ..AoDebug::default() }));
        assert_eq!(AoDebug::parse(Some("View-Old")), Ok(AoDebug { old: true, view: true, post: true, ..AoDebug::default() }));
        for picture in ["post", "old", "view", "view-old"] {
            assert!(!AoDebug::parse(Some(picture)).unwrap().lighting(), "{picture}");
        }
        // The stage's temporary `lighting` is gone: unknown, as anything else.
        for bad in ["gtao", "lighting", "old-lighting"] {
            assert_eq!(AoDebug::parse(Some(bad)), Err(bad.to_string()), "{bad}");
        }
    }

    #[test]
    fn the_lighting_mode_reaches_the_settings_and_the_uniform() {
        let post = AoDebug { post: true, ..AoDebug::default() };
        let picture = AmbientOcclusion::with_debug(post, false);
        assert!(!picture.wants_lighting());
        let lighting = AoDebug::default();
        let settings = AmbientOcclusion::with_debug(lighting, true);
        assert!(settings.wants_lighting());
        assert_eq!(settings.mode, Vec4::new(0.0, 0.0, 1.0, 0.0));
        // The lighting's own strength and remedies; the rest of the estimate as on the picture.
        assert_eq!((settings.params.y, picture.params.y), (LIGHTING_STRENGTH, GTAO_STRENGTH));
        assert_eq!((settings.params.x, settings.params.w, settings.proj), (picture.params.x, picture.params.w, picture.proj));
        // Asked for but not in the lighting yet (the hook pending or failed): stage one's.
        let waiting = AmbientOcclusion::with_debug(lighting, false);
        assert!(waiting.wants_lighting());
        assert_eq!((waiting.params, waiting.variant), (picture.params, picture.variant));
        // The lighting's uniform: the exponent, the fade end, the strength of the settings,
        // the lamps' weight; the debug view only with view-lighting (the post `view` draws its
        // own grey).
        let u = lighting.lighting_uniform(&settings);
        assert_eq!(u.params, Vec4::new(crate::ssao_lighting::EXPONENT, 0.0, FADE_END, crate::ssao_lighting::DISTANCE_TOLERANCE));
        assert_eq!((u.extra.x, u.extra.y), (LIGHTING_STRENGTH, crate::ssao_lighting::LAMPS));
        let view = AoDebug { view: true, ..AoDebug::default() };
        assert_eq!(view.lighting_uniform(&AmbientOcclusion::with_debug(view, true)).params.y, 1.0);
        let grey = AoDebug { view: true, post: true, ..AoDebug::default() };
        assert_eq!(grey.lighting_uniform(&AmbientOcclusion::with_debug(grey, false)).params.y, 0.0);
        let strong = AoDebug { gtao_strength: Some(0.6), ..lighting };
        assert_eq!(strong.lighting_uniform(&AmbientOcclusion::with_debug(strong, true)).extra.x, 0.6);
        // The lamps: 1 unless NVF_PLAY_DEBUG_AO_LAMPS says otherwise; 0 is the ambient alone.
        let ambient_only = AoDebug { lamps: Some(0.0), ..lighting };
        assert_eq!(ambient_only.lighting_uniform(&settings).extra.y, 0.0);
        let half = AoDebug { lamps: Some(0.5), ..lighting };
        assert_eq!(half.lighting_uniform(&settings).extra.y, 0.5);
        // The exponent: 1 unless NVF_PLAY_DEBUG_AO_EXPONENT says otherwise.
        let gamma = AoDebug { exponent: Some(1.0 / 2.2), ..lighting };
        assert_eq!(gamma.lighting_uniform(&settings).params.x, 1.0 / 2.2);
        // The texel the pass clears to when it cannot run is open at the farthest distance.
        assert!(include_str!("ssao.wesl").contains(&format!("const SKY: f32 = {:.1};", OPEN_TEXEL.g)));
        assert_eq!(OPEN_TEXEL.r, 1.0);
    }

    /// The bindings follow the mode, the row and the hook, in both directions, with each
    /// log line once per change: on with the mode asked for and the hook in place; off (and
    /// taken away) when the row goes off or the mode is not asked for; on again with the row;
    /// never while the hook is pending; a failed hook said once and the bindings kept away.
    #[test]
    fn the_bindings_follow_the_row_the_mode_and_the_hook() {
        let cam = Entity::from_raw_u32(7).unwrap();
        let lighting = AmbientOcclusion::with_debug(AoDebug::default(), true);
        let post = AmbientOcclusion::with_debug(AoDebug { post: true, ..AoDebug::default() }, false);
        let mut log = LightingLog::default();
        let ids = |p: &LightingPlan| p.wanted.iter().map(|(e, _)| *e).collect::<Vec<_>>();

        // The hook not installed yet: nothing, silently.
        let p = lighting_plan(&[(cam, lighting)], &[], HookState::Pending, &mut log);
        assert!(p.wanted.is_empty() && p.insert.is_empty() && p.remove.is_empty() && p.lines.is_empty());
        // Installed: bound, said once.
        let p = lighting_plan(&[(cam, lighting)], &[], HookState::Installed, &mut log);
        assert_eq!((ids(&p), p.insert.clone(), p.remove.clone(), p.lines.clone()), (vec![cam], vec![cam], vec![], vec![LightingLine::On]));
        let p = lighting_plan(&[(cam, lighting)], &[cam], HookState::Installed, &mut log);
        assert!(p.insert.is_empty() && p.remove.is_empty() && p.lines.is_empty(), "kept, nothing said again");
        // The row switched off (the camera loses its settings): taken away, said once.
        let p = lighting_plan(&[], &[cam], HookState::Installed, &mut log);
        assert_eq!((p.remove.clone(), p.lines.clone()), (vec![cam], vec![LightingLine::Off]));
        let p = lighting_plan(&[], &[], HookState::Installed, &mut log);
        assert!(p.lines.is_empty() && p.remove.is_empty());
        // On again with the row.
        let p = lighting_plan(&[(cam, lighting)], &[], HookState::Installed, &mut log);
        assert_eq!((p.insert.clone(), p.lines.clone()), (vec![cam], vec![LightingLine::On]));
        // The post placement with the row on: taken away.
        let p = lighting_plan(&[(cam, post)], &[cam], HookState::Installed, &mut log);
        assert_eq!((p.remove.clone(), p.lines.clone(), p.wanted.len()), (vec![cam], vec![LightingLine::Off], 0));
        // A failed hook: the post placement, the failure said once, never bound.
        let mut log = LightingLog::default();
        let p = lighting_plan(&[(cam, lighting)], &[], HookState::Failed, &mut log);
        assert_eq!((p.insert.clone(), p.lines.clone()), (vec![], vec![LightingLine::HookFailed]));
        let p = lighting_plan(&[(cam, lighting)], &[], HookState::Failed, &mut log);
        assert!(p.lines.is_empty() && p.insert.is_empty());
        // Not asked for: no failure to report.
        let mut log = LightingLog::default();
        assert!(lighting_plan(&[(cam, post)], &[], HookState::Failed, &mut log).lines.is_empty());
        assert_eq!(LightingLine::On.text(), "ambient occlusion: on the ambient light in the lighting (main camera)");
        assert_eq!(LightingLine::Off.text(), "ambient occlusion: off the lighting");
    }

    /// ssao.wesl `denoise` writes the factor of full pixel 2q through the apply pass's own
    /// filter, with the occlusion pass's distance.
    #[test]
    fn the_denoise_entry_uses_the_apply_filter() {
        let source = include_str!("ssao.wesl").replace("\r\n", "\n");
        let at = source.find("fn denoise(in: FullscreenVertexOutput)").expect("denoise entry");
        let body = &source[at..at + source[at..].find("\n}\n").expect("its end")];
        for line in [
            "let px = vec2<i32>(floor(in.position.xy)) * 2;",
            "let d = depth_at(px);",
            "return texel(1.0, SKY);",
            "let open = openness_at(px, d);",
            "return texel(safe, ao.params.z / d);",
        ] {
            assert!(body.contains(line), "{line}");
        }
        // The occlusion pass writes the same distance (near / depth value) for the same pixel.
        assert!(source.contains("let z = ao.params.z / max(d, 1e-7);"));
    }

    #[test]
    fn the_number_aids_are_held_to_their_ranges() {
        for none in [None, Some(""), Some("  ")] {
            assert_eq!(AoDebug::parse_strength(none), Ok(None), "{none:?}");
            assert_eq!(AoDebug::parse_exponent(none), Ok(None), "{none:?}");
        }
        assert_eq!(AoDebug::parse_strength(Some("0.6")), Ok(Some(0.6)));
        assert_eq!(AoDebug::parse_strength(Some(" 0.8 ")), Ok(Some(0.8)));
        assert_eq!(AoDebug::parse_strength(Some("2")), Ok(Some(1.0)));
        assert_eq!(AoDebug::parse_strength(Some("-1")), Ok(Some(0.0)));
        // The exponent's: 0.1..2.
        assert_eq!(AoDebug::parse_exponent(Some("1")), Ok(Some(1.0)));
        assert_eq!(AoDebug::parse_exponent(Some("0.4545")), Ok(Some(0.4545)));
        assert_eq!(AoDebug::parse_exponent(Some("0")), Ok(Some(0.1)));
        assert_eq!(AoDebug::parse_exponent(Some("5")), Ok(Some(2.0)));
        for bad in ["strong", "NaN", "inf"] {
            assert_eq!(AoDebug::parse_strength(Some(bad)), Err(bad.to_string()));
            assert_eq!(AoDebug::parse_exponent(Some(bad)), Err(bad.to_string()));
        }
        // Only the ground-truth estimate takes the strength, in either placement.
        let set = AoDebug { gtao_strength: Some(0.6), ..AoDebug::default() };
        assert_eq!(AmbientOcclusion::with_debug(set, true).params.y, 0.6);
        assert_eq!(AmbientOcclusion::with_debug(set, false).params.y, 0.6);
        let old = AoDebug { old: true, post: true, ..set };
        assert_eq!(AmbientOcclusion::with_debug(old, false).params.y, STRENGTH);
        let post = AoDebug { post: true, ..set };
        assert!(post.describe(false).ends_with("depth-aware denoise, strength 0.6 (NVF_PLAY_DEBUG_AO_STRENGTH), on the picture (NVF_PLAY_DEBUG_AO=post)"));
        assert!(!old.describe(false).contains("STRENGTH"));
    }

    #[test]
    fn the_uniform_carries_the_estimates_strength_and_mode() {
        let new = AmbientOcclusion::with_debug(AoDebug { post: true, ..AoDebug::default() }, false);
        assert_eq!(new.params.y, GTAO_STRENGTH);
        assert_eq!(new.mode, Vec4::ZERO);
        let old = AmbientOcclusion::with_debug(AoDebug { old: true, post: true, ..AoDebug::default() }, false);
        assert_eq!(old.params.y, STRENGTH);
        assert_eq!(old.mode, Vec4::new(1.0, 0.0, 0.0, 0.0));
        let view = AmbientOcclusion::with_debug(AoDebug { view: true, post: true, ..AoDebug::default() }, false);
        assert_eq!(view.mode, Vec4::new(0.0, 1.0, 0.0, 0.0));
        let lighting = AmbientOcclusion::with_debug(AoDebug::default(), true);
        assert_eq!((lighting.params.y, lighting.mode), (LIGHTING_STRENGTH, Vec4::new(0.0, 0.0, 1.0, 0.0)));
        // Everything else as before the change.
        for ao in [new, old, view, lighting] {
            assert_eq!((ao.params.x, ao.params.w, ao.proj.z, ao.proj.w), (RADIUS, FADE_END, MAX_SCREEN_RADIUS, FADE_START));
            assert_eq!((ao.render_scale, ao.area), (Vec4::ONE, Vec4::ZERO));
        }
    }

    /// The log line says what runs. The default, in the lighting: the remedies, the strength,
    /// the lamps and the exponent built in. On the picture the line is stage one's with the
    /// placement named; the old estimate's and the grey view's as before. The smoke scenarios
    /// match the common start, "ambient occlusion: ground-truth estimate (GTAO), 2 slices x 4
    /// steps a side at half resolution, depth-aware denoise".
    #[test]
    fn the_log_line_names_the_estimate_and_the_placement() {
        let start = "ambient occlusion: ground-truth estimate (GTAO), 2 slices x 4 steps a side at half resolution, depth-aware denoise";
        assert_eq!(AoDebug::default().describe(true), format!("{start}, variant soft+thin, strength 0.92, in the lighting: on the ambient light and on lamps without a shadow by 1, exponent 1, multi-bounce by surface colour"));
        let post = AoDebug { post: true, ..AoDebug::default() };
        assert_eq!(post.describe(false), format!("{start}, on the picture (NVF_PLAY_DEBUG_AO=post)"));
        // The default asked for, on the picture while the hook is not in place: stage one's.
        assert_eq!(AoDebug::default().describe(false), start);
        assert_eq!(AoDebug { old: true, post: true, ..AoDebug::default() }.describe(false), "ambient occlusion: old estimate (12-sample spiral, NVF_PLAY_DEBUG_AO)");
        assert!(AoDebug { view: true, post: true, ..AoDebug::default() }.describe(false).ends_with("depth-aware denoise, factor shown alone as grey (NVF_PLAY_DEBUG_AO)"));
        assert!(AoDebug { view: true, ..AoDebug::default() }
            .describe(true)
            .ends_with(", exponent 1, multi-bounce by surface colour, the factor each surface took shown alone (NVF_PLAY_DEBUG_AO=view-lighting)"));
        // The aids that were set are named.
        let aids = AoDebug { lamps: Some(0.5), exponent: Some(0.5), gtao_strength: Some(0.4), multibounce: Some(false), ..AoDebug::default() };
        assert_eq!(
            aids.describe(true),
            format!("{start}, variant soft+thin, strength 0.4 (NVF_PLAY_DEBUG_AO_STRENGTH), in the lighting: on the ambient light and on lamps without a shadow by 0.5 (NVF_PLAY_DEBUG_AO_LAMPS), exponent 0.5 (NVF_PLAY_DEBUG_AO_EXPONENT), no multi-bounce (NVF_PLAY_DEBUG_AO_MULTIBOUNCE)")
        );
        assert!(!post.describe(false).contains("bounce"), "the picture placement has no multi-bounce");
        assert!(AoDebug { lamps: Some(0.0), ..AoDebug::default() }.describe(true).contains("in the lighting: on the ambient light alone (NVF_PLAY_DEBUG_AO_LAMPS), exponent 1"));
        assert!(AoDebug { lamps: Some(1.0), ..post }.describe(false).ends_with(", lamps and exponent ignored on the picture (NVF_PLAY_DEBUG_AO_LAMPS, NVF_PLAY_DEBUG_AO_EXPONENT)"));
    }

    #[test]
    fn the_multibounce_aid_reads_on_and_off_and_feeds_the_uniform() {
        assert_eq!(AoDebug::parse_switch(None), Ok(None));
        assert_eq!(AoDebug::parse_switch(Some(" ")), Ok(None));
        assert_eq!(AoDebug::parse_switch(Some("0")), Ok(Some(false)));
        assert_eq!(AoDebug::parse_switch(Some("OFF")), Ok(Some(false)));
        assert_eq!(AoDebug::parse_switch(Some("1")), Ok(Some(true)));
        assert_eq!(AoDebug::parse_switch(Some("maybe")), Err("maybe".to_string()));
        assert!(AoDebug::default().multibounce(), "on by default");
        let settings = AmbientOcclusion::default();
        assert_eq!(AoDebug::default().lighting_uniform(&settings).extra.z, 1.0);
        assert_eq!(AoDebug { multibounce: Some(false), ..AoDebug::default() }.lighting_uniform(&settings).extra.z, 0.0);
    }

    /// ssao.wesl `visible_arc`, the same formula: the integral of cos(t - g) |sin t| from 0 to h.
    fn visible_arc(h: f32, g: f32) -> f32 {
        0.25 * (g.cos() - (2.0 * h - g).cos() + 2.0 * h * g.sin())
    }

    /// The same integral by the midpoint rule.
    fn arc_numeric(h: f32, g: f32) -> f32 {
        let n = 20_000;
        let step = h / n as f32;
        // From 0 towards h, whichever side: the integral over the arc between them.
        let width = step.abs();
        (0..n).map(|i| (i as f32 + 0.5) * step).map(|t| (t - g).cos() * t.sin().abs() * width).sum()
    }

    #[test]
    fn the_visible_arc_is_the_integral_it_stands_for() {
        assert!(include_str!("ssao.wesl").contains("return 0.25 * (cos_g - cos(2.0 * h - gamma) + 2.0 * h * sin_g);"));
        use std::f32::consts::FRAC_PI_2;
        for g in [-1.3f32, -0.6, 0.0, 0.4, 1.2] {
            for h in [-(FRAC_PI_2 - g), -0.7, -0.1, 0.0, 0.3, 0.9, FRAC_PI_2 + g] {
                assert!((visible_arc(h, g) - arc_numeric(h, g)).abs() < 1e-3, "g {g} h {h}");
            }
            // Nothing in the way: the whole arc, cos g + g sin g (the shader's `unblocked`), at
            // least 1 and never 0, so the ratio never divides by zero.
            let open = visible_arc(g - FRAC_PI_2, g) + visible_arc(g + FRAC_PI_2, g);
            assert!((open - (g.cos() + g * g.sin())).abs() < 1e-5, "g {g}");
            assert!(open >= 1.0 - 1e-6);
            // A horizon raised on either side only takes away (monotone, never negative).
            let mut last = open;
            for i in 1..=20 {
                let h = g + FRAC_PI_2 - (g + FRAC_PI_2) * i as f32 / 20.0;
                let v = visible_arc(g - FRAC_PI_2, g) + visible_arc(h, g);
                assert!(v <= last + 1e-6 && v >= -1e-6, "g {g} h {h}");
                last = v;
            }
        }
        // A surface facing the camera with one side hidden up to the view vector keeps half.
        let half = visible_arc(-FRAC_PI_2, 0.0) + visible_arc(0.0, 0.0);
        assert!((half - 0.5).abs() < 1e-6);
        assert!((1.0 - GTAO_STRENGTH * (1.0 - half) - 0.77).abs() < 1e-6, "the crease's factor GTAO_STRENGTH's comment gives");
    }

    /// Quality is the estimate as it was before the row had levels, uniform for uniform;
    /// Performance differs only in its slice count (one), which the aid's own count still
    /// overrides; the level rides in `mode.w` and the log line ends with it.
    #[test]
    fn the_level_sets_only_the_slice_count() {
        for (debug, lighting) in [(AoDebug::default(), true), (AoDebug { post: true, ..AoDebug::default() }, false)] {
            let quality = AmbientOcclusion::with_debug_at(debug, lighting, AoLevel::Quality);
            let before = AmbientOcclusion::with_debug(debug, lighting);
            assert_eq!((quality.params, quality.proj, quality.mode, quality.variant), (before.params, before.proj, before.mode, before.variant));
            assert_eq!(quality.level(), AoLevel::Quality);
            let performance = AmbientOcclusion::with_debug_at(debug, lighting, AoLevel::Performance);
            assert_eq!(performance.level(), AoLevel::Performance);
            assert_eq!(performance.variant, Vec4::new(PERFORMANCE_SLICES as f32, 0.0, quality.variant.z, 0.0), "only the slices differ");
            assert_eq!((performance.params, performance.mode.truncate()), (quality.params, quality.mode.truncate()));
        }
        let slices4 = AoDebug { variant: Some(AoVariant::parse(Some("slices4")).unwrap()), ..AoDebug::default() };
        assert_eq!(AmbientOcclusion::with_debug_at(slices4, true, AoLevel::Performance).variant.x, 4.0, "the aid's count wins");
        let old = AoDebug { old: true, post: true, ..AoDebug::default() };
        assert_eq!(AmbientOcclusion::with_debug_at(old, false, AoLevel::Performance).variant, Vec4::ZERO, "the old estimate has no slices");
        let line = AoDebug::default().describe_with(true, Some(AoLevel::Performance));
        assert_eq!(
            line,
            "ambient occlusion: ground-truth estimate (GTAO), 1 slice x 4 steps a side at half resolution, depth-aware denoise, variant soft+thin, strength 0.92, in the lighting: on the ambient light and on lamps without a shadow by 1, exponent 1, multi-bounce by surface colour, level Performance"
        );
        assert_eq!(AoDebug::default().describe_with(true, Some(AoLevel::Quality)), format!("{}, level Quality", AoDebug::default().describe(true)));
        // The aid's `none` at Performance: one slice, no remedies named.
        let none = AoDebug { variant: Some(AoVariant::default()), ..AoDebug::default() };
        assert!(none.describe_with(true, Some(AoLevel::Performance)).contains("1 slice x 4 steps a side at half resolution, depth-aware denoise, variant none (NVF_PLAY_DEBUG_AO_VARIANT), strength 0.92"));
        assert!(!old.describe_with(false, Some(AoLevel::Performance)).contains("level"), "the old estimate has no level");
        assert_eq!(AO_CHOICES, ["Off", AoLevel::Performance.name(), AoLevel::Quality.name()]);
    }

    /// The resolve returns sample 0 as it is (no unsqueeze: the passes unsqueeze what they
    /// read), the nearest sample only for variant `nearest`, and writes the depth itself.
    #[test]
    fn the_resolve_keeps_sample_zero_as_it_is() {
        let source = include_str!("ssao.wesl").replace("\r\n", "\n");
        let at = source.find("fn resolve_depth(in: FullscreenVertexOutput) -> @builtin(frag_depth) f32 {").expect("resolve entry");
        let body = &source[at..at + source[at..].find("\n}\n").expect("its end")];
        assert!(body.contains("var raw = textureLoad(depth, p, 0);") && body.trim_end().ends_with("return raw;"));
        assert!(body.contains("if (u32(ao.variant.z) & VARIANT_NEAREST) != 0u {"));
        assert_eq!(RESOLVED_DEPTH_FORMAT, bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT, "the prepass's format, bit for bit");
    }
}
