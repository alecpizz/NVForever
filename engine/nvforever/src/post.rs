//! Bloom and sharpening of the enhanced graphics (`graphics.rs`,
//! docs/design/graphics-enhanced.md §3.4-§3.5), as passes of their own on the tonemapped
//! picture, after the ambient occlusion (`ssao.rs`) and before the image-space grade and
//! its blur (imagespace.rs).
//!
//! Bevy's bloom needs the main view to be HDR (Rgba16Float targets and the tonemapper as
//! a pass of its own): twice the memory traffic of the multisampled view on the owner's
//! GTX 1050, and a change under every pass that reads the picture (water, sky, grade).
//! Bevy's contrast-adaptive sharpening garbled the non-HDR view (yellow and white noise
//! over the whole picture) [observed, 2026-10-12]. So both are done here on
//! the picture as the vanilla pipeline leaves it: what is brighter than a threshold is
//! taken at a quarter of the size, blurred wide and added back (the way games of the
//! original's time did bloom); the sharpening is contrast-adaptive against the four
//! neighbours.
//!
//! On the main camera only, while it carries [`PostEffects`] with either part on.
//!
//! The light glow (`graphics.rs`, on in vanilla) is not drawn here: it is Bevy's bloom on
//! an HDR main view ([`glow_bloom`]), its strength set from the image space on cameras
//! with [`ImageSpaceGlow`].

use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FilterMode,
    FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType,
    SamplerDescriptor, ShaderStages, ShaderType, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
    AddressMode, BindGroupLayout,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

/// Bloom threshold on the tonemapped picture's linear values (the brightest channel), and
/// the soft knee around it [engineering choice, by eye: the sun and its glare, lamp bulbs
/// and lit windows glow; sunlit sand and walls do not].
pub const BLOOM_THRESHOLD: f32 = 0.82;
pub const BLOOM_KNEE: f32 = 0.12;
/// How much of the blurred light is added back [engineering choice, by eye].
pub const BLOOM_INTENSITY: f32 = 0.9;
/// Sharpening strength (0..1) [engineering choice: brings back some of the detail the
/// game's textures lose under multisampling and trilinear filtering, without rims].
pub const SHARPENING: f32 = 0.5;

/// Bloom and sharpening on a camera: x threshold, y knee, z bloom intensity (0 none),
/// w sharpening (0 none).
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType)]
#[extract_app(bevy::render::RenderApp)]
pub struct PostEffects {
    pub params: Vec4,
}

impl PostEffects {
    #[allow(dead_code)]
    pub fn new(bloom: bool, sharpen: bool) -> Self {
        Self::new_with_hdr_glow(bloom, sharpen, false)
    }

    pub fn new_with_hdr_glow(bloom: bool, sharpen: bool, physical_hdr_glow: bool) -> Self {
        let (threshold, knee, intensity) = if physical_hdr_glow {
            (0.94, 0.08, 0.25)
        } else {
            (BLOOM_THRESHOLD, BLOOM_KNEE, BLOOM_INTENSITY)
        };
        PostEffects { params: Vec4::new(threshold, knee, if bloom { intensity } else { 0.0 }, if sharpen { SHARPENING } else { 0.0 }) }
    }
}

fn effective_glow_intensity(base: f32, physical: bool, interior: bool) -> f32 {
    if physical && interior { base * 0.60 } else { base }
}

/// A camera whose bloom is the light glow ([`glow_bloom`], strength from the image space).
#[derive(Component, Clone, Copy)]
pub struct ImageSpaceGlow;

/// Bevy's bloom for the light glow: its NATURAL preset (energy-conserving: every pixel
/// scatters a share of its light, so only what is far brighter than its surroundings,
/// neon, bulbs, fire, glowing beams, shows a halo) [engineering choice, judged against
/// the original's frames at night, docs/spec-notes/world.md §6 "HDR and bloom"]; the
/// strength follows the image space ([`glow_intensity`]).
pub fn glow_bloom() -> bevy::post_process::bloom::Bloom {
    bevy::post_process::bloom::Bloom { intensity: glow_intensity(&fnv_world::imagespace::Hdr::default()), low_frequency_boost: GLOW_LOW_FREQUENCY_BOOST, ..bevy::post_process::bloom::Bloom::NATURAL }
}

/// Light glow strength at the image space's "Bright Scale" 2 (the Mojave's
/// NVDefaultExterior) [engineering choice, by eye against the original's frames; 0.15 until
/// 2026-11-07, when the Lucky 38's and the Tops' letters ran together at it, #1540].
pub const GLOW_INTENSITY: f32 = 0.09;
/// The light glow's boost of its widest blur levels: none, where Bevy's preset has 0.7 and
/// the Strip's night sky turned from near black to a grey-brown haze [observed 2026-11-07:
/// the sky with the glow off, and the original's frames, are dark; world.md section 6].
pub const GLOW_LOW_FREQUENCY_BOOST: f32 = 0.0;

/// The bloom strength for an image space's HDR values: [`GLOW_INTENSITY`] scaled by its
/// "Bright Scale" over 2, held to half and twice [engineering choice: the name says how
/// strongly bright parts are scaled for the original's glow, xEdit IMGS DNAM]. An image
/// space without HDR values (bright scale 0 or less) keeps the base strength.
pub fn glow_intensity(hdr: &fnv_world::imagespace::Hdr) -> f32 {
    let scale = hdr.v[fnv_world::imagespace::HDR_BRIGHT_SCALE];
    if scale <= 0.0 {
        return GLOW_INTENSITY;
    }
    GLOW_INTENSITY * (scale / 2.0).clamp(0.5, 2.0)
}

/// The "Bright Scale" up to which the light glow alone stands for the original's glow
/// (NVDefaultExterior's, the Mojave's) [observed: IMGS 0x0008809D].
pub const BRIGHT_SCALE_BASE: f32 = 2.0;
/// Bright pass strength per unit of "Bright Scale" above [`BRIGHT_SCALE_BASE`], and the
/// most it reaches [engineering choice, fitted to the original's frames 2026-11-07, #1540:
/// 2 on the Strip at night, on what the threshold below lets through].
pub const BRIGHT_PASS_PER_SCALE: f32 = 0.4;
pub const BRIGHT_PASS_MAX: f32 = 2.0;
/// The HDR value (brightest channel, linear, before the tonemapper) over which a pixel
/// feeds the bright pass, and the soft knee around it [engineering choice, 2026-11-07,
/// #1540: neon tubes such as Gomorrah's are far over it, held to 16 at the pass's input;
/// cut-out signs stay under it (the Lucky 38's letters at 6), so their letters stay apart].
pub const BRIGHT_PASS_THRESHOLD: f32 = 7.0;
pub const BRIGHT_PASS_KNEE: f32 = 0.25;
/// The bright pass's blur step: a Gaussian about 17 x this quarter-size texels wide
/// [engineering choice, 2026-11-07: with only tubes feeding it, 1 gives them the width they
/// had; 0.5 from the owner's 2026-10-25 choice stays for a modifier's lower clamp below].
pub const BRIGHT_PASS_STEP: f32 = 1.0;
/// The bright pass while a modifier lowers Bright Clamp ([`modified_bright_pass`]): lamps
/// bloom over a sharp picture, as in the birth scene, so it takes all that is over white
/// [engineering choice, the owner's frames 2026-10-25 and 2026-11-02].
pub const CLAMPED_PASS_PER_SCALE: f32 = 0.08;
pub const CLAMPED_PASS_MAX: f32 = 1.0;
pub const CLAMPED_PASS_THRESHOLD: f32 = 1.0;
pub const CLAMPED_PASS_STEP: f32 = 0.5;

/// The strength of the bright pass ([`BrightPass`]) added to the light glow for a "Bright
/// Scale" above the Mojave's: none up to [`BRIGHT_SCALE_BASE`], then
/// [`BRIGHT_PASS_PER_SCALE`] per unit, up to [`BRIGHT_PASS_MAX`] (docs/spec-notes/world.md
/// §6 "HDR and bloom"). The Strip's night adapter (NVTheStripNightIS) sets Bright Scale 7,
/// and the original's neon there throws halos far wider than the light glow's
/// [observed: fx_audit fx_strip_b, owner's frame 2026-10-25]; only what is brighter than
/// [`BRIGHT_PASS_THRESHOLD`] feeds the pass, so the rest of the picture keeps its look
/// [hypothesis].
pub fn bright_pass(scale: f32) -> f32 {
    (BRIGHT_PASS_PER_SCALE * (scale - BRIGHT_SCALE_BASE)).clamp(0.0, BRIGHT_PASS_MAX)
}

/// The bright pass of the light glow's HDR view: what is brighter than a threshold, blurred
/// wide and added back before the tonemapper (`bright_pass_hdr`), so a thin neon tube far
/// over white still throws a halo. Same layout as [`PostEffects`]: x threshold, y knee,
/// z strength (0 none), w minus the blur's step in quarter-size texels ([`BRIGHT_PASS_STEP`]):
/// negative because the composite this shares with [`PostEffects`] sharpens by a positive w.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType)]
#[extract_app(bevy::render::RenderApp)]
pub struct BrightPass {
    pub params: Vec4,
}

impl Default for BrightPass {
    fn default() -> Self {
        BrightPass { params: Vec4::new(BRIGHT_PASS_THRESHOLD, BRIGHT_PASS_KNEE, 0.0, -BRIGHT_PASS_STEP) }
    }
}

/// The most the image space modifiers raise the light glow to (Bevy's bloom intensity)
/// [engineering choice: CGCameraFlashISFX multiplies Bright Scale by 5 and Bright Clamp by
/// 0.1, which this reading turns into a factor of 50; a full-strength glow is the flash].
pub const GLOW_MODIFIED_MAX: f32 = 1.0;

/// The light glow's strength with the image space modifiers on the screen: the image
/// space's ([`glow_intensity`]) times the modifiers' change to Bright Scale, up to
/// [`GLOW_MODIFIED_MAX`]; never below the image space's own when the modifiers raise it
/// [hypothesis: image-modifiers.md §11]. Their change to Bright Clamp goes to the bright
/// pass ([`modified_bright_pass`]): the light glow scatters every pixel, so at full
/// strength it blurs the whole picture, while a lower clamp lets more of what is bright
/// glow [observed: the birth scene (CG00BirthBaseISFX, Bright Scale ×2, Bright Clamp ×0.2)
/// blurred out entirely through the light glow, where the original keeps the people and
/// the frame sharp under blooming lamps; owner's TTW run and Fallout 3 frame, 2026-11-02].
pub fn modified_glow(base: f32, scale_ratio: f32) -> f32 {
    if (scale_ratio - 1.0).abs() < 1e-4 {
        return base;
    }
    (base * scale_ratio).clamp(0.0, GLOW_MODIFIED_MAX.max(base))
}

/// What the bright pass does this frame: its strength, threshold and blur step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassShape {
    pub strength: f32,
    pub threshold: f32,
    pub step: f32,
}

/// The bright pass ([`bright_pass`]) with the modifiers on the screen: the image space's
/// Bright Scale times the modifiers' change to it, divided by their change to Bright Clamp
/// ("the clamp for which lum levels bloom": a lower clamp lets more glow [documented: GECK
/// wiki ImageSpace]; the division is ours [hypothesis]). A lowered clamp also takes the
/// lower threshold ([`CLAMPED_PASS_THRESHOLD`]); the rest of the picture stays sharp.
pub fn modified_bright_pass(scale: f32, scale_ratio: f32, clamp_ratio: f32) -> PassShape {
    let s = scale * scale_ratio / clamp_ratio.max(0.01);
    if clamp_ratio < 1.0 - 1e-4 {
        return PassShape { strength: (CLAMPED_PASS_PER_SCALE * (s - BRIGHT_SCALE_BASE)).clamp(0.0, CLAMPED_PASS_MAX), threshold: CLAMPED_PASS_THRESHOLD, step: CLAMPED_PASS_STEP };
    }
    PassShape { strength: bright_pass(s), threshold: BRIGHT_PASS_THRESHOLD, step: BRIGHT_PASS_STEP }
}

/// Set the light glow's strength on [`ImageSpaceGlow`] cameras from the image space now,
/// and the bright pass ([`bright_pass`]) that goes with a high "Bright Scale".
fn image_space_glow(
    sky: Option<Res<crate::sky::SkyState>>,
    screen: Option<Res<crate::imagespace::ScreenModifiers>>,
    mut cams: Query<(&mut bevy::post_process::bloom::Bloom, Option<&mut BrightPass>), With<ImageSpaceGlow>>,
    mut told: Local<f32>,
) {
    let Some(sky) = sky else { return };
    let ratio = |i: usize| screen.as_deref().map_or(1.0, |s| s.hdr_ratio(i));
    let hdr = &sky.cinematic().hdr;
    let physical = nvf_assets::authored::physical_pbr();
    let interior = sky.interior.is_some();
    let intensity = effective_glow_intensity(
        modified_glow(glow_intensity(hdr), ratio(fnv_world::imagespace::HDR_BRIGHT_SCALE)),
        physical,
        interior,
    );
    let pass = modified_bright_pass(hdr.v[fnv_world::imagespace::HDR_BRIGHT_SCALE], ratio(fnv_world::imagespace::HDR_BRIGHT_SCALE), ratio(fnv_world::imagespace::HDR_BRIGHT_CLAMP));
    if (pass.strength - *told).abs() > 0.05 {
        info!("light glow: bright pass {:.2} (Bright Scale {:.1})", pass.strength, hdr.v[fnv_world::imagespace::HDR_BRIGHT_SCALE]);
        *told = pass.strength;
    }
    for (mut b, effects) in &mut cams {
        if b.intensity != intensity {
            b.intensity = intensity;
        }
        if let Some(mut e) = effects {
            let params = Vec4::new(pass.threshold, e.params.y, pass.strength, -pass.step);
            if e.params != params {
                e.params = params;
            }
        }
    }
}

pub struct PostPlugin;

impl Plugin for PostPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "post.wesl");
        app.add_plugins((
            ExtractComponentPlugin::<PostEffects>::default(),
            UniformComponentPlugin::<PostEffects>::default(),
            ExtractComponentPlugin::<BrightPass>::default(),
            UniformComponentPlugin::<BrightPass>::default(),
        ))
        .add_systems(PostUpdate, image_space_glow);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, (prepare::<PostEffects>, prepare::<BrightPass>).chain().in_set(RenderSystems::PrepareResources))
            .add_systems(Core3d, bright_pass_hdr.in_set(Core3dSystems::PostProcess).after(bevy::post_process::bloom::bloom).before(tonemapping))
            .add_systems(
                Core3d,
                post_pass
                    .in_set(Core3dSystems::PostProcess)
                    .after(tonemapping)
                    .after(crate::ssao::ao_pass)
                    .after(crate::taa::taa_pass)
                    .after(bevy::anti_alias::fxaa::fxaa)
                    .after(bevy::anti_alias::smaa::smaa)
                    .after(crate::shafts::shafts_pass)
                    .before(crate::imagespace::blur_pass),
            );
    }
}

#[derive(Resource)]
pub(crate) struct PostPipelines {
    /// One texture, sampler, uniform (bright, blurs); and with the glow (composite).
    single: BindGroupLayoutDescriptor,
    composite_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    base: RenderPipelineDescriptor,
    /// bright, blur_across, blur_down (all into GLOW_FORMAT).
    glow: [CachedRenderPipelineId; 3],
    /// The same for the HDR bright pass: bright_hdr, blur_across_fine, blur_down_fine.
    glow_hdr: [CachedRenderPipelineId; 3],
    composite: HashMap<TextureFormat, CachedRenderPipelineId>,
}

/// A view's textures and composite pipeline for one pass: `C` is [`PostEffects`] (on the
/// tonemapped picture) or [`BrightPass`] (on the HDR picture).
#[derive(Component)]
pub(crate) struct PostView<C> {
    composite: CachedRenderPipelineId,
    /// The picture's format the composite pipeline was built for.
    format: TextureFormat,
    /// Quarter-size textures: the bright part and its blur go back and forth.
    a: CachedTexture,
    b: CachedTexture,
    pass: std::marker::PhantomData<C>,
}

const GLOW_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

fn init_pipelines(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, cache: Res<PipelineCache>) {
    let tex = || texture_2d(TextureSampleType::Float { filterable: true });
    let single = BindGroupLayoutDescriptor::new(
        "post_layout",
        &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (tex(), sampler(SamplerBindingType::Filtering), uniform_buffer::<PostEffects>(true))),
    );
    let composite_layout = BindGroupLayoutDescriptor::new(
        "post_composite_layout",
        &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (tex(), sampler(SamplerBindingType::Filtering), uniform_buffer::<PostEffects>(true), tex())),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        ..default()
    });
    let base = RenderPipelineDescriptor {
        label: Some("post_pipeline".into()),
        layout: vec![single.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState { shader: assets.load("embedded://nvforever/post.wesl"), ..default() }),
        ..default()
    };
    let queue = |entries: [&str; 3]| {
        entries.map(|entry| {
            let mut d = base.clone();
            if let Some(f) = d.fragment.as_mut() {
                f.entry_point = Some(entry.to_string().into());
                f.targets = vec![Some(ColorTargetState { format: GLOW_FORMAT, blend: None, write_mask: ColorWrites::ALL })];
            }
            cache.queue_render_pipeline(d)
        })
    };
    let glow = queue(["bright", "blur_across", "blur_down"]);
    let glow_hdr = queue(["bright_hdr", "blur_across_fine", "blur_down_fine"]);
    commands.insert_resource(PostPipelines { single, composite_layout, sampler, base, glow, glow_hdr, composite: HashMap::default() });
}

fn prepare<C: Component>(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<PostPipelines>,
    mut textures: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    views: Query<(Entity, &ExtractedView), With<C>>,
) {
    for (entity, view) in &views {
        let format = view.target_format;
        let composite = match pipelines.composite.get(&format) {
            Some(id) => *id,
            None => {
                let mut d = pipelines.base.clone();
                d.layout = vec![pipelines.composite_layout.clone()];
                if let Some(f) = d.fragment.as_mut() {
                    f.entry_point = Some("composite".into());
                    f.targets = vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })];
                }
                let id = cache.queue_render_pipeline(d);
                pipelines.composite.insert(format, id);
                id
            }
        };
        let size = (view.viewport.zw() / 4).max(UVec2::ONE);
        let mut quarter = |label: &'static str| {
            textures.get(
                &device,
                TextureDescriptor {
                    label: Some(label),
                    size: Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: GLOW_FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let a = quarter("post_glow_a");
        let b = quarter("post_glow_b");
        commands.entity(entity).insert(PostView::<C> { composite, format, a, b, pass: std::marker::PhantomData });
    }
}

fn draw(ctx: &mut RenderContext, label: &'static str, target: &TextureView, pipeline: &bevy::render::render_resource::RenderPipeline, bind_group: &bevy::render::render_resource::BindGroup, offset: u32) {
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[offset]);
    pass.draw(0..3, 0..1);
}

/// The bright part of the picture, blurred (when `bloom`), then the picture written back
/// with it added (and sharpened, by the uniform's w).
#[allow(clippy::too_many_arguments)]
fn glow_and_composite<C>(
    ctx: &mut RenderContext,
    target: &ViewTarget,
    pv: &PostView<C>,
    pipelines: &PostPipelines,
    cache: &PipelineCache,
    device: &RenderDevice,
    binding: bevy::render::render_resource::BindingResource,
    offset: u32,
    bloom: bool,
    hdr: bool,
    label: &'static str,
) {
    let Some(composite) = cache.get_render_pipeline(pv.composite) else { return };
    let ids = if hdr { &pipelines.glow_hdr } else { &pipelines.glow };
    let glow: Option<Vec<_>> = ids.iter().map(|id| cache.get_render_pipeline(*id)).collect();
    let Some(glow) = glow else { return };
    let single: BindGroupLayout = cache.get_bind_group_layout(&pipelines.single);
    // GPU time of the pass (recorded with --log-frame-times, graphics.rs).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), label);
    let post = target.post_process_write();
    if bloom {
        let bright = device.create_bind_group("post_bright", &single, &BindGroupEntries::sequential((post.source, &pipelines.sampler, binding.clone())));
        draw(ctx, "post_bright_pass", &pv.a.default_view, glow[0], &bright, offset);
        let across = device.create_bind_group("post_across", &single, &BindGroupEntries::sequential((&pv.a.default_view, &pipelines.sampler, binding.clone())));
        draw(ctx, "post_blur_across_pass", &pv.b.default_view, glow[1], &across, offset);
        let down = device.create_bind_group("post_down", &single, &BindGroupEntries::sequential((&pv.b.default_view, &pipelines.sampler, binding.clone())));
        draw(ctx, "post_blur_down_pass", &pv.a.default_view, glow[2], &down, offset);
    }
    let layout = cache.get_bind_group_layout(&pipelines.composite_layout);
    let bind_group = device.create_bind_group("post_composite", &layout, &BindGroupEntries::sequential((post.source, &pipelines.sampler, binding, &pv.a.default_view)));
    draw(ctx, "post_composite_pass", post.destination, composite, &bind_group, offset);
    span.end(ctx.command_encoder());
}

pub(crate) fn post_pass(
    view: ViewQuery<(&ViewTarget, &DynamicUniformIndex<PostEffects>, &PostEffects, &PostView<PostEffects>)>,
    pipelines: Res<PostPipelines>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<PostEffects>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, index, effects, pv) = view.into_inner();
    let (bloom, sharpen) = (effects.params.z > 0.0, effects.params.w > 0.0);
    if (!bloom && !sharpen) || !crate::ssao::format_matches(target, pv.format) {
        return;
    }
    let Some(binding) = uniforms.uniforms().binding() else { return };
    glow_and_composite(&mut ctx, target, pv, &pipelines, &cache, &device, binding, index.index(), bloom, false, "nvf_bloom_sharpen");
}

/// The light glow's bright pass ([`BrightPass`]) on the HDR picture, before the tonemapper.
pub(crate) fn bright_pass_hdr(
    view: ViewQuery<(&ViewTarget, &DynamicUniformIndex<BrightPass>, &BrightPass, &PostView<BrightPass>)>,
    pipelines: Res<PostPipelines>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<BrightPass>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, index, pass, pv) = view.into_inner();
    if pass.params.z <= 0.0 || !crate::ssao::format_matches(target, pv.format) {
        return;
    }
    let Some(binding) = uniforms.uniforms().binding() else { return };
    glow_and_composite(&mut ctx, target, pv, &pipelines, &cache, &device, binding, index.index(), true, true, "nvf_bright_pass");
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_world::imagespace::{Hdr, HDR_BRIGHT_SCALE};

    #[test]
    fn glow_follows_bright_scale() {
        let at = |scale: f32| {
            let mut h = Hdr::default();
            h.v[HDR_BRIGHT_SCALE] = scale;
            glow_intensity(&h)
        };
        assert_eq!(at(2.0), GLOW_INTENSITY);
        assert!((at(2.4) - GLOW_INTENSITY * 1.2).abs() < 1e-6);
        assert_eq!(at(10.0), GLOW_INTENSITY * 2.0);
        assert_eq!(at(0.1), GLOW_INTENSITY * 0.5);
        assert_eq!(at(0.0), GLOW_INTENSITY);
    }

    #[test]
    fn bright_pass_only_above_the_mojaves_bright_scale() {
        assert_eq!(bright_pass(2.0), 0.0);
        assert_eq!(bright_pass(1.0), 0.0);
        // The Strip's night adapter, Bright Scale 7.
        assert!((bright_pass(7.0) - 2.0).abs() < 1e-6);
        assert_eq!(bright_pass(1000.0), BRIGHT_PASS_MAX);
    }

    #[test]
    fn a_lower_bright_clamp_feeds_the_bright_pass_not_the_whole_picture() {
        // The birth scene: the room's Bright Scale 1.6, CG00BirthBaseISFX ×2 and clamp ×0.2.
        let base = GLOW_INTENSITY * 0.8;
        assert!((modified_glow(base, 2.0) - base * 2.0).abs() < 1e-6);
        assert_eq!(modified_bright_pass(1.6, 2.0, 0.2), PassShape { strength: CLAMPED_PASS_MAX, threshold: CLAMPED_PASS_THRESHOLD, step: CLAMPED_PASS_STEP });
        // No modifier: as the image space has it.
        assert_eq!(modified_glow(base, 1.0), base);
        assert_eq!(modified_bright_pass(7.0, 1.0, 1.0), PassShape { strength: bright_pass(7.0), threshold: BRIGHT_PASS_THRESHOLD, step: BRIGHT_PASS_STEP });
    }

    #[test]
    fn physical_interiors_restrain_double_bloom_and_glow() {
        let std_fx = PostEffects::new_with_hdr_glow(true, true, false);
        let phys_fx = PostEffects::new_with_hdr_glow(true, true, true);
        assert!(phys_fx.params.x > std_fx.params.x && phys_fx.params.z < std_fx.params.z);
        assert!((effective_glow_intensity(0.10, true, true) - 0.06).abs() < 1e-6);
        assert_eq!(effective_glow_intensity(0.10, true, false), 0.10);
        assert_eq!(effective_glow_intensity(0.10, false, true), 0.10);
    }
}
