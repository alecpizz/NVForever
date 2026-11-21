//! Volumetric fog (Modern Feel, `nvf_play::modern_feel::ModernFeel::volumetric_fog`,
//! docs/design/modern-feel-gfx.md §1) [design: NVForever's own, not the original's; every
//! constant an engineering choice tuned by eye].
//!
//! The air holds a little dust: sunlight lights it where the sun's shadow maps say the sun
//! reaches, so shafts stand where light falls past roofs, poles and rocks, and the haze
//! glows towards a low sun; lamps and spotlights (the flashlight) light the dust around
//! them. Subtle by design: only light is added (no extinction, the weather's distance fog
//! keeps that job), the dust thins above the camera so the sky is not greyed, and it is
//! strongest at dawn and dusk and in hazy weather.
//!
//! Density follows the place: outdoors the weather's fog (shafts.rs `haze`) and the sun's
//! height; indoors the cell's own fog distance (a near fog: dusty vaults and factories),
//! lamps only (the interior's directional light is a fill light, not a sun).
//!
//! How: a ray march at half the picture's size from the camera to the depth prepass's
//! surface (at most [`REACH_OUTDOORS`]), [`STEPS`] steps with a per-pixel offset, sampling
//! the sun's shadow cascades through Bevy's own view bindings (the same cascades the
//! world is lit with) and the clustered point and spot lights with the game's falloff;
//! then a depth-aware upsample adds it to the HDR picture before bloom and tonemapping, so
//! the eye adaptation and the grade see it as they see the scene. The screen-space sun
//! shafts (shafts.rs) are kept for the sky beyond the shadow maps, at
//! [`SHAFTS_SHARE`] of their strength while this is on.

use crate::fly::FlyCam;
use crate::sky::SkyState;
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::pbr::{MeshPipelineViewLayoutKey, MeshPipelineViewLayouts, MeshViewBindGroup, ViewKeyCache};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer};
use bevy::render::render_resource::{
    AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d,
    FilterMode, FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
    TextureUsages, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use nvf_play::modern_feel::ModernFeel;

/// Dust density outdoors in clear air with the sun high (per game unit) [design: an optical
/// depth of about 0.02 over the march; towards the sun the phase function makes that a soft
/// glow, away from it next to nothing].
const DENSITY_OUTDOORS: f32 = 2.5e-6;
/// Extra density with the sun low (times the base: dawn and dusk) [design].
const LOW_SUN_BOOST: f32 = 2.5;
/// Extra density in the haziest weather (times the base) [design].
const HAZE_BOOST: f32 = 2.0;
/// Dust density indoors at the reference fog distance (per game unit) [design].
const DENSITY_INDOORS: f32 = 4.0e-5;
/// Indoors: the density is `DENSITY_INDOORS × INTERIOR_FOG_REF / fog far`, held to
/// [`INTERIOR_RANGE`] times the base [design: a near fog is a dusty room].
const INTERIOR_FOG_REF: f32 = 8_000.0;
const INTERIOR_RANGE: (f32, f32) = (0.5, 1.75);
/// How far the march reaches (game units) outdoors and indoors [design: the sun's shadow
/// maps reach 8,000; beyond, the weather's own fog is the haze].
const REACH_OUTDOORS: f32 = 6_000.0;
const REACH_INDOORS: f32 = 3_000.0;
/// Outdoors the dust thins above the camera over this height (game units) [design].
const THIN_ABOVE: f32 = 2_500.0;
/// Forward scattering of the dust (Henyey-Greenstein g) outdoors and indoors [design].
const FORWARD_OUTDOORS: f32 = 0.55;
const FORWARD_INDOORS: f32 = 0.3;
/// How strongly lamps light the dust (times their colour) outdoors and indoors [design].
const LAMPS_OUTDOORS: f32 = 1.0;
const LAMPS_INDOORS: f32 = 1.0;
/// The screen-space sun shafts keep this share of their strength while the fog is on
/// [design: they still carry the light past far mountains, beyond the shadow maps].
const SHAFTS_SHARE: f32 = 0.6;
/// Steps of the march (vol_fog.wesl reads it from the uniform).
const STEPS: f32 = 16.0;

/// Volumetric fog on a camera, kept up to date by [`follow_sky`].
#[derive(Component, ExtractComponent, Clone, Copy, Default, ShaderType)]
#[extract_app(bevy::render::RenderApp)]
pub struct VolumetricHaze {
    /// xyz: towards the sun (Bevy axes), w: the sun's share (0: no sun this frame).
    pub sun: Vec4,
    /// x: density (per unit), y: 1 / thinning height above the camera (0: none), z: forward
    /// scattering g, w: lamps' strength.
    pub medium: Vec4,
    /// x: reach (units), y: steps, z: 1 to vary the offsets per frame (a temporal
    /// anti-aliasing or upscaler smooths them), w: near plane.
    pub march: Vec4,
}

/// What the GPU gets (vol_fog.wesl `Haze`).
#[derive(Clone, Copy, Default, ShaderType)]
struct HazeUniform {
    sun: Vec4,
    medium: Vec4,
    march: Vec4,
    /// xy: the half-size target, zw: the picture.
    sizes: Vec4,
}

pub struct VolFogPlugin;

impl Plugin for VolFogPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "vol_fog.wesl");
        app.add_plugins(ExtractComponentPlugin::<VolumetricHaze>::default()).add_systems(
            PostUpdate,
            follow_sky
                .after(bevy::camera::CameraUpdateSystems)
                .after(bevy::transform::TransformSystems::Propagate)
                .after(crate::shafts::follow_sun),
        );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipelines.after(bevy::pbr::MeshPipelineSystems))
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(
                Core3d,
                fog_pass.in_set(Core3dSystems::PostProcess).before(bevy::post_process::bloom::bloom).before(tonemapping),
            );
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The fog's settings for this frame (`None`: no fog).
pub fn haze_now(sky: &SkyState, near: f32, temporal: bool) -> VolumetricHaze {
    if let Some(cell) = sky.interior.as_ref() {
        let far = if cell.fog_far > 0.0 { cell.fog_far } else { INTERIOR_FOG_REF };
        let density = DENSITY_INDOORS * (INTERIOR_FOG_REF / far).clamp(INTERIOR_RANGE.0, INTERIOR_RANGE.1);
        return VolumetricHaze {
            sun: Vec4::ZERO,
            medium: Vec4::new(density, 0.0, FORWARD_INDOORS, LAMPS_INDOORS),
            march: Vec4::new(REACH_INDOORS, STEPS, if temporal { 1.0 } else { 0.0 }, near),
        };
    }
    let elevation = sky.sun_elevation();
    let up = smoothstep(-2.0, 3.0, elevation);
    let low = 1.0 - smoothstep(8.0, 35.0, elevation);
    let range = sky.fog_override.map(|(n, f)| (n, f, 1.0)).or_else(|| sky.fog());
    let haze = range.map_or(0.0, |(_, far, s)| crate::shafts::haze(far, s));
    let density = DENSITY_OUTDOORS * (1.0 + LOW_SUN_BOOST * low) * (1.0 + HAZE_BOOST * haze);
    VolumetricHaze {
        sun: sky.sun_dir().normalize_or_zero().extend(up),
        // Lamps light the dust once the sun is down (by day their halo is lost in the light,
        // and the clustered lights' loop is the march's dearest part).
        medium: Vec4::new(density, 1.0 / THIN_ABOVE, FORWARD_OUTDOORS, LAMPS_OUTDOORS * (1.0 - up)),
        march: Vec4::new(REACH_OUTDOORS, STEPS, if temporal { 1.0 } else { 0.0 }, near),
    }
}

/// Put the fog on the main camera or take it off, its settings from the sky, and the
/// screen-space shafts' share.
#[allow(clippy::type_complexity)]
fn follow_sky(
    mut commands: Commands,
    feel: Option<Res<ModernFeel>>,
    sky: Res<SkyState>,
    mut cams: Query<
        (Entity, &Projection, Option<&mut VolumetricHaze>, Option<&mut crate::shafts::LightShafts>, Has<crate::taa::TemporalAa>, Has<crate::fsr::FsrUpscaling>, Has<crate::upscaler::DlssUpscaling>),
        With<FlyCam>,
    >,
    mut said: Local<Option<String>>,
) {
    let on = feel.is_some_and(|f| f.volumetric_fog());
    let Ok((entity, proj, haze, shafts, taa, fsr, dlss)) = cams.single_mut() else { return };
    if !on {
        if haze.is_some() {
            commands.entity(entity).remove::<VolumetricHaze>();
        }
        if said.as_deref() != Some("off") {
            info!("volumetric fog: off");
            *said = Some("off".into());
        }
        return;
    }
    let Projection::Perspective(p) = proj else { return };
    let new = haze_now(&sky, p.near, taa || fsr || dlss);
    if new.sun.w > 0.0 {
        if let Some(mut s) = shafts {
            s.sun.z *= SHAFTS_SHARE;
        }
    }
    // The log says what the fog does when it changes noticeably (density in steps of 10%).
    let place = if sky.interior.is_some() { "indoors, lamps" } else if new.sun.w >= 1.0 { "outdoors, sun" } else if new.sun.w > 0.0 { "outdoors, sun and lamps" } else { "outdoors, lamps (sun down)" };
    let line = format!("volumetric fog: on, {place}, density {:.1e}/unit, reach {:.0}", (new.medium.x * 1e7).round() / 1e7, new.march.x);
    let step = format!("{place} {:.0}", (new.medium.x / DENSITY_OUTDOORS * 2.0).round());
    if said.as_deref() != Some(step.as_str()) {
        info!("{line}");
        *said = Some(step);
    }
    match haze {
        Some(mut h) => {
            if h.sun != new.sun || h.medium != new.medium || h.march != new.march {
                *h = new;
            }
        }
        None => {
            commands.entity(entity).insert(new);
        }
    }
}

#[derive(Resource)]
pub(crate) struct FogPipelines {
    view_layouts: MeshPipelineViewLayouts,
    /// The march's own group with a single-sampled and a multisampled depth.
    march_layouts: [BindGroupLayoutDescriptor; 2],
    composite_layouts: [BindGroupLayoutDescriptor; 2],
    sampler: Sampler,
    shader: Handle<Shader>,
    fullscreen: FullscreenShader,
    march: HashMap<(MeshPipelineViewLayoutKey, bool), CachedRenderPipelineId>,
    composite: HashMap<(TextureFormat, bool), CachedRenderPipelineId>,
}

#[derive(Component)]
pub(crate) struct FogView {
    march: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
    multi: bool,
    format: TextureFormat,
    half: CachedTexture,
    uniform: UniformBuffer<HazeUniform>,
}

const FOG_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

fn init_pipelines(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, view_layouts: Res<MeshPipelineViewLayouts>) {
    let march_layout = |multi: bool| {
        let depth = if multi { texture_depth_2d_multisampled() } else { texture_depth_2d() };
        BindGroupLayoutDescriptor::new("vol_fog_march_layout", &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (uniform_buffer::<HazeUniform>(false), depth)))
    };
    let composite_layout = |multi: bool| {
        let depth = if multi { texture_depth_2d_multisampled() } else { texture_depth_2d() };
        BindGroupLayoutDescriptor::new(
            "vol_fog_composite_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<HazeUniform>(false),
                    depth,
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                ),
            ),
        )
    };
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        ..default()
    });
    commands.insert_resource(FogPipelines {
        view_layouts: view_layouts.clone(),
        march_layouts: [march_layout(false), march_layout(true)],
        composite_layouts: [composite_layout(false), composite_layout(true)],
        sampler,
        shader: assets.load("embedded://nvforever/vol_fog.wesl"),
        fullscreen: fullscreen.clone(),
        march: HashMap::default(),
        composite: HashMap::default(),
    });
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<FogPipelines>,
    mut textures: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    keys: Res<ViewKeyCache>,
    views: Query<(Entity, &ExtractedView, &Msaa, &VolumetricHaze)>,
) {
    for (entity, view, msaa, haze) in &views {
        let Some(key) = keys.get(&view.retained_view_entity) else { continue };
        let view_key = MeshPipelineViewLayoutKey::from(*key);
        let multi = msaa.samples() > 1;
        let format = view.target_format;
        let p = &mut *pipelines;
        let march = *p.march.entry((view_key, multi)).or_insert_with(|| {
            let mut defs: Vec<bevy::shader::ShaderDefVal> = vec!["MARCH".into(), "SHADOW_FILTER_METHOD_HARDWARE_2X2".into()];
            if multi {
                defs.push("DEPTH_MS".into());
            }
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("vol_fog_march_pipeline".into()),
                layout: vec![p.view_layouts.get_view_layout(view_key).main_layout, p.march_layouts[multi as usize].clone()],
                vertex: p.fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: p.shader.clone(),
                    shader_defs: defs,
                    entry_point: Some("march".into()),
                    targets: vec![Some(ColorTargetState { format: FOG_FORMAT, blend: None, write_mask: ColorWrites::ALL })],
                    ..default()
                }),
                ..default()
            })
        });
        let composite = *p.composite.entry((format, multi)).or_insert_with(|| {
            let mut defs: Vec<bevy::shader::ShaderDefVal> = vec!["COMPOSITE".into()];
            if multi {
                defs.push("DEPTH_MS".into());
            }
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("vol_fog_composite_pipeline".into()),
                layout: vec![p.composite_layouts[multi as usize].clone()],
                vertex: p.fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: p.shader.clone(),
                    shader_defs: defs,
                    entry_point: Some("composite".into()),
                    targets: vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })],
                    ..default()
                }),
                ..default()
            })
        });
        let size = view.viewport.zw().max(UVec2::ONE);
        let half_size = (size + UVec2::ONE) / 2;
        let half = textures.get(
            &device,
            TextureDescriptor {
                label: Some("vol_fog_half"),
                size: Extent3d { width: half_size.x, height: half_size.y, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: FOG_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        let mut uniform = UniformBuffer::from(HazeUniform {
            sun: haze.sun,
            medium: haze.medium,
            march: haze.march,
            sizes: Vec4::new(half_size.x as f32, half_size.y as f32, size.x as f32, size.y as f32),
        });
        uniform.write_buffer(&device, &queue);
        commands.entity(entity).insert(FogView { march, composite, multi, format, half, uniform });
    }
}

/// The march at half size, then the upsample onto the picture.
pub(crate) fn fog_pass(
    view: ViewQuery<(&ViewTarget, &FogView, &VolumetricHaze, &ViewPrepassTextures, &MeshViewBindGroup)>,
    pipelines: Res<FogPipelines>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, fv, haze, prepass, view_bind_group) = view.into_inner();
    if haze.medium.x <= 0.0 {
        return;
    }
    if crate::ssao::depth_multisampled(prepass) != Some(fv.multi) || !crate::ssao::format_matches(target, fv.format) {
        return;
    }
    let Some(depth) = prepass.depth_only_view() else { return };
    let (Some(march), Some(composite)) = (cache.get_render_pipeline(fv.march), cache.get_render_pipeline(fv.composite)) else { return };
    let Some(uniform) = fv.uniform.binding() else { return };
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_volumetric_fog");
    let layout = cache.get_bind_group_layout(&pipelines.march_layouts[fv.multi as usize]);
    let bg = device.create_bind_group("vol_fog_march", &layout, &BindGroupEntries::sequential((uniform.clone(), depth)));
    {
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some("vol_fog_march_pass"),
            color_attachments: &[Some(RenderPassColorAttachment { view: &fv.half.default_view, depth_slice: None, resolve_target: None, ops: Operations::default() })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(march);
        pass.set_bind_group(0, &view_bind_group.main, &view_bind_group.main_offsets);
        pass.set_bind_group(1, &bg, &[]);
        pass.draw(0..3, 0..1);
    }
    let post = target.post_process_write();
    let layout = cache.get_bind_group_layout(&pipelines.composite_layouts[fv.multi as usize]);
    let bg = device.create_bind_group(
        "vol_fog_composite",
        &layout,
        &BindGroupEntries::sequential((uniform, depth, post.source, &pipelines.sampler, &fv.half.default_view)),
    );
    {
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some("vol_fog_composite_pass"),
            color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(composite);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..3, 0..1);
    }
    span.end(ctx.command_encoder());
}
