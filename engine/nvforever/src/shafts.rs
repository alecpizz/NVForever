//! Sun shafts of the enhanced graphics (`graphics.rs`, docs/design/graphics-enhanced.md
//! §3.7): the sky around the sun, where nothing stands in front of it, is blurred along
//! the lines towards the sun and added to the picture, so light streams past the
//! mountains, roofs and poles that cut into it (screen-space light scattering, the way
//! games of the original's era and after drew "god rays").
//!
//! Tied to the weather: the source is the game's own sky and sun as drawn (their colours,
//! the clouds in front of the sun), and how strongly the light scatters follows the
//! weather's fog: the nearer its fog far distance, the hazier the air and the stronger
//! the shafts ([`HAZE_REF`]). None indoors, none once the sun is down, fading out as the
//! sun leaves the screen.
//!
//! Bevy's volumetric fog was tried for this and dropped (the design note §4).
//!
//! Quarter-size passes on the tonemapped picture after the temporal anti-aliasing and
//! before bloom and sharpening; on the main camera only, while it carries [`LightShafts`].

use crate::fly::FlyCam;
use crate::sky::SkyState;
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer};
use bevy::render::render_resource::{
    AddressMode, BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState, ColorWrites,
    Extent3d, FilterMode, FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages, TextureView,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

/// Strength of the shafts in the clearest air [engineering choice, by eye at Goodsprings
/// at dusk in NVWastelandClear, fog far 200,000].
const STRENGTH_CLEAR: f32 = 0.6;
/// Added strength in the haziest air (fog far at or below [`HAZE_REF`]).
const STRENGTH_HAZE: f32 = 0.9;
/// Fog far distance (game units) at which the air counts as fully hazy: the haze is
/// `HAZE_REF / fog far`, up to 1 [engineering choice].
const HAZE_REF: f32 = 20_000.0;
/// The shafts reach this share of the way from a pixel towards the sun [engineering choice].
const LENGTH: f32 = 0.85;
/// The source: sky within this distance of the sun on the screen (share of the picture's
/// height) [engineering choice].
const SOURCE_RADIUS: f32 = 0.3;

/// Sun shafts on a camera, kept up to date by [`follow_sun`].
#[derive(Component, ExtractComponent, Clone, Copy, Default, ShaderType)]
#[extract_app(bevy::render::RenderApp)]
pub struct LightShafts {
    /// x, y: the sun on the screen (uv, may lie outside 0..1), z: strength (0: none this
    /// frame), w: the picture's width / height.
    pub sun: Vec4,
    /// rgb: tint (display values), w: length (share of the way to the sun).
    pub tint: Vec4,
    /// x: source radius, y: distance (game units) beyond which the depth is the sky dome's,
    /// z: the projection's near plane, w unused.
    pub source: Vec4,
}

pub struct ShaftsPlugin;

impl Plugin for ShaftsPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shafts.wesl");
        app.add_plugins((ExtractComponentPlugin::<LightShafts>::default(), UniformComponentPlugin::<LightShafts>::default()))
            .add_systems(PostUpdate, follow_sun.after(bevy::camera::CameraUpdateSystems).after(bevy::transform::TransformSystems::Propagate));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(
                Core3d,
                shafts_pass
                    .in_set(Core3dSystems::PostProcess)
                    .after(tonemapping)
                    .after(crate::ssao::ao_pass)
                    .after(crate::taa::taa_pass)
                    .after(bevy::anti_alias::fxaa::fxaa)
                    .after(bevy::anti_alias::smaa::smaa)
                    .before(crate::imagespace::blur_pass),
            );
    }
}

/// How hazy the weather's air is (0..1) from its fog far distance and the fog's
/// strength (the alpha of a weather fade into or out of no fog).
pub fn haze(fog_far: f32, fog_strength: f32) -> f32 {
    (HAZE_REF / fog_far.max(1.0)).min(1.0) * fog_strength.clamp(0.0, 1.0)
}

/// The sun's place on the screen and the strength and tint of its shafts this frame.
pub(crate) fn follow_sun(sky: Res<SkyState>, mut cams: Query<(&GlobalTransform, &Projection, &mut LightShafts), With<FlyCam>>) {
    let Ok((gt, proj, mut shafts)) = cams.single_mut() else { return };
    let Projection::Perspective(p) = proj else { return };
    let mut strength = 0.0;
    let mut uv = Vec2::splat(0.5);
    if sky.interior.is_none() {
        let to_sun = sky.sun_dir();
        // The sun as a point at infinity: its direction in view space, projected.
        let v = gt.compute_transform().rotation.inverse() * to_sun;
        let clip = proj.get_clip_from_view() * v.extend(0.0);
        if clip.w > 1e-4 {
            let ndc = clip.xy() / clip.w;
            uv = Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
            // Fades out as the sun leaves the screen, and as it sets [the sun disc's own
            // fade: sky.rs SkyState::light].
            let edge = ndc.x.abs().max(ndc.y.abs());
            let on_screen = 1.0 - smoothstep(1.0, 1.6, edge);
            let up = smoothstep(-2.0, 3.0, sky.sun_elevation());
            let range = sky.fog_override.map(|(n, f)| (n, f, 1.0)).or_else(|| sky.fog());
            let haze = range.map_or(0.0, |(_, far, s)| haze(far, s));
            strength = (STRENGTH_CLEAR + STRENGTH_HAZE * haze) * on_screen * up;
        }
    }
    let tint = Vec3::ONE;
    let new = LightShafts { sun: Vec4::new(uv.x, uv.y, strength, p.aspect_ratio), tint: tint.extend(LENGTH), source: Vec4::new(SOURCE_RADIUS, crate::sky::SKY_DEPTH_FROM, p.near, 0.0) };
    if shafts.sun != new.sun || shafts.tint != new.tint || shafts.source != new.source {
        *shafts = new;
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Resource)]
pub(crate) struct ShaftsPipelines {
    /// The source pass's layouts with a single-sampled and a multisampled depth.
    source_layouts: [BindGroupLayoutDescriptor; 2],
    /// One texture, sampler, uniform (radial blurs); and with the shafts (composite).
    single: BindGroupLayoutDescriptor,
    composite_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    base: RenderPipelineDescriptor,
    source: HashMap<bool, CachedRenderPipelineId>,
    /// radial_fine, radial_coarse (into RAYS_FORMAT).
    radial: [CachedRenderPipelineId; 2],
    composite: HashMap<TextureFormat, CachedRenderPipelineId>,
}

#[derive(Component)]
pub(crate) struct ShaftsView {
    source: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
    multi: bool,
    /// The picture's format the composite pipeline was built for.
    format: TextureFormat,
    a: CachedTexture,
    b: CachedTexture,
}

const RAYS_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

fn init_pipelines(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, cache: Res<PipelineCache>) {
    let tex = || texture_2d(TextureSampleType::Float { filterable: true });
    let source_layout = |multi: bool| {
        let depth = if multi { texture_depth_2d_multisampled() } else { texture_depth_2d() };
        BindGroupLayoutDescriptor::new(
            "shafts_source_layout",
            &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (tex(), sampler(SamplerBindingType::Filtering), uniform_buffer::<LightShafts>(true), depth)),
        )
    };
    let single = BindGroupLayoutDescriptor::new(
        "shafts_layout",
        &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (tex(), sampler(SamplerBindingType::Filtering), uniform_buffer::<LightShafts>(true))),
    );
    let composite_layout = BindGroupLayoutDescriptor::new(
        "shafts_composite_layout",
        &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (tex(), sampler(SamplerBindingType::Filtering), uniform_buffer::<LightShafts>(true), tex())),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        ..default()
    });
    let base = RenderPipelineDescriptor {
        label: Some("shafts_pipeline".into()),
        layout: vec![single.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState { shader: assets.load("embedded://nvforever/shafts.wesl"), ..default() }),
        ..default()
    };
    let radial = ["radial_fine", "radial_coarse"].map(|entry| {
        let mut d = base.clone();
        if let Some(f) = d.fragment.as_mut() {
            f.entry_point = Some(entry.into());
            f.targets = vec![Some(ColorTargetState { format: RAYS_FORMAT, blend: None, write_mask: ColorWrites::ALL })];
        }
        cache.queue_render_pipeline(d)
    });
    commands.insert_resource(ShaftsPipelines {
        source_layouts: [source_layout(false), source_layout(true)],
        single,
        composite_layout,
        sampler,
        base,
        source: HashMap::default(),
        radial,
        composite: HashMap::default(),
    });
}

fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<ShaftsPipelines>,
    mut textures: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    views: Query<(Entity, &ExtractedView, &Msaa), With<LightShafts>>,
) {
    for (entity, view, msaa) in &views {
        let multi = msaa.samples() > 1;
        let format = view.target_format;
        let source = match pipelines.source.get(&multi) {
            Some(id) => *id,
            None => {
                let mut d = pipelines.base.clone();
                d.layout = vec![pipelines.source_layouts[multi as usize].clone()];
                if let Some(f) = d.fragment.as_mut() {
                    f.entry_point = Some("source".into());
                    f.targets = vec![Some(ColorTargetState { format: RAYS_FORMAT, blend: None, write_mask: ColorWrites::ALL })];
                    f.shader_defs.push("SOURCE".into());
                    if multi {
                        f.shader_defs.push("MULTISAMPLED".into());
                    }
                }
                let id = cache.queue_render_pipeline(d);
                pipelines.source.insert(multi, id);
                id
            }
        };
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
                    format: RAYS_FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let a = quarter("shafts_a");
        let b = quarter("shafts_b");
        commands.entity(entity).insert(ShaftsView { source, composite, multi, format, a, b });
    }
}

fn draw(ctx: &mut RenderContext, label: &'static str, target: &TextureView, pipeline: &RenderPipeline, bind_group: &BindGroup, offset: u32) {
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

/// The sky near the sun into a quarter-size texture, blurred towards the sun twice, added
/// to the picture.
pub(crate) fn shafts_pass(
    view: ViewQuery<(&ViewTarget, &DynamicUniformIndex<LightShafts>, &LightShafts, &ShaftsView, &ViewPrepassTextures)>,
    pipelines: Res<ShaftsPipelines>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<LightShafts>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, index, shafts, sv, prepass) = view.into_inner();
    if shafts.sun.z <= 0.0 {
        return;
    }
    if crate::ssao::depth_multisampled(prepass) != Some(sv.multi) || !crate::ssao::format_matches(target, sv.format) {
        return;
    }
    let Some(depth) = prepass.depth_only_view() else { return };
    let (Some(source), Some(composite)) = (cache.get_render_pipeline(sv.source), cache.get_render_pipeline(sv.composite)) else { return };
    let radial: Option<Vec<_>> = pipelines.radial.iter().map(|id| cache.get_render_pipeline(*id)).collect();
    let Some(radial) = radial else { return };
    let Some(binding) = uniforms.uniforms().binding() else { return };
    // GPU time of the pass (recorded with --log-frame-times, graphics.rs).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_sun_shafts");
    let offset = index.index();
    let post = target.post_process_write();
    let layout = cache.get_bind_group_layout(&pipelines.source_layouts[sv.multi as usize]);
    let bg = device.create_bind_group("shafts_source", &layout, &BindGroupEntries::sequential((post.source, &pipelines.sampler, binding.clone(), depth)));
    draw(&mut ctx, "shafts_source_pass", &sv.a.default_view, source, &bg, offset);
    let single = cache.get_bind_group_layout(&pipelines.single);
    let bg = device.create_bind_group("shafts_fine", &single, &BindGroupEntries::sequential((&sv.a.default_view, &pipelines.sampler, binding.clone())));
    draw(&mut ctx, "shafts_fine_pass", &sv.b.default_view, radial[0], &bg, offset);
    let bg = device.create_bind_group("shafts_coarse", &single, &BindGroupEntries::sequential((&sv.b.default_view, &pipelines.sampler, binding.clone())));
    draw(&mut ctx, "shafts_coarse_pass", &sv.a.default_view, radial[1], &bg, offset);
    let layout = cache.get_bind_group_layout(&pipelines.composite_layout);
    let bg = device.create_bind_group("shafts_composite", &layout, &BindGroupEntries::sequential((post.source, &pipelines.sampler, binding, &sv.a.default_view)));
    draw(&mut ctx, "shafts_composite_pass", post.destination, composite, &bg, offset);
    span.end(ctx.command_encoder());
}

#[cfg(test)]
mod tests {
    use super::haze;

    #[test]
    fn haze_follows_the_fog_far_distance() {
        assert!((haze(200_000.0, 1.0) - 0.1).abs() < 1e-6);
        assert_eq!(haze(10_000.0, 1.0), 1.0);
        assert_eq!(haze(10_000.0, 0.0), 0.0);
    }
}
