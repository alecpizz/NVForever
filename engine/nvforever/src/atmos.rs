//! Volumetric atmosphere of the remastered look (`graphics.rs` switch `volumetric-fog`,
//! docs/design/graphics-remaster.md §3): a thin fog that thickens towards the ground, lit
//! by the sun through its own shadow cascades (so buildings, rocks and trees cast shafts
//! into the air at dawn and dusk) and by the ambient light, ray-marched at half
//! resolution after the main passes and blended into the HDR picture before tonemapping.
//! It adds to the weather's distance fog (fog.rs), which the surfaces already carry; this
//! one is near the camera, where the weather's is thin. An engineering choice, not the
//! original game's.
//!
//! The pass reads the view's lights and shadow maps through Bevy's mesh view bind group
//! (group 0), as Bevy's own volumetric fog does; group 1 holds the depth prepass, the
//! uniform and its textures (atmos.wesl).
//!
//! On the main camera only, while it carries [`VolumetricAtmosphere`] and draws HDR.

use crate::fly::FlyCam;
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::math::{DMat4, DVec3};
use bevy::pbr::{MeshPipelineViewLayoutKey, MeshPipelineViewLayouts, MeshViewBindGroup, ViewKeyCache};
use bevy::platform::collections::HashMap;
use bevy::camera::CameraProjection;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent, BlendFactor, BlendOperation, BlendState,
    CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FilterMode, FragmentState, LoadOp, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
    ShaderType, StoreOp, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, RetainedViewEntity, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

/// Fog density at the reference height, per game unit: about 18 % of the light from
/// 5,000 units (70 m) away is lost [engineering choice, by eye; the weather's fog does the
/// distance].
const DENSITY: f32 = 0.00004;
/// The height (game units) over which the fog thins to 1/e above the reference height.
const HEIGHT_FALLOFF: f32 = 1200.0;
/// The reference height lies this far below the camera (game units): the fog follows the
/// ground the player stands on rather than one sea level for the whole Mojave.
const BELOW_CAMERA: f32 = 400.0;
/// The farthest the march goes (game units); beyond it the weather's fog takes over.
const REACH: f32 = 14000.0;
/// Forward scattering of the sun's light (Henyey-Greenstein g).
const ANISOTROPY: f32 = 0.55;
/// Strength of the sun's and the ambient light in the fog.
const SUN: f32 = 3.0;
const AMBIENT: f32 = 0.8;
/// The share of this frame's march in the accumulated result.
const NEW_SHARE: f32 = 0.2;
/// A camera that moved further than this in a frame (game units) starts again.
const JUMP: f64 = 1500.0;

/// `NVF_FOG="density,falloff,g,sun,ambient"` overrides the constants (a test aid for tuning).
static TUNING: std::sync::LazyLock<[f32; 5]> = std::sync::LazyLock::new(|| {
    let mut t = [DENSITY, HEIGHT_FALLOFF, ANISOTROPY, SUN, AMBIENT];
    if let Ok(v) = std::env::var("NVF_FOG") {
        for (slot, part) in t.iter_mut().zip(v.split(',')) {
            if let Ok(x) = part.trim().parse() {
                *slot = x;
            }
        }
    }
    t
});

/// The volumetric atmosphere on a camera (main world); [`follow_camera`] keeps the uniform
/// up to date.
#[derive(Component, Clone, Copy, Default)]
#[require(AtmosUniform)]
pub struct VolumetricAtmosphere {
    /// Last frame's clip space from world space, and the camera's position then.
    prev: Option<(DMat4, DVec3)>,
    frame: u32,
}

/// The passes' uniform (atmos.wesl `Atmos`).
#[derive(Component, ExtractComponent, Clone, Copy, Default, ShaderType)]
#[extract_app(bevy::render::RenderApp)]
pub struct AtmosUniform {
    pub prev_clip_from_rel: Mat4,
    pub params: Vec4,
    pub params2: Vec4,
    pub size: Vec4,
    pub params3: Vec4,
}

pub struct AtmosPlugin;

impl Plugin for AtmosPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "atmos.wesl");
        app.add_plugins((ExtractComponentPlugin::<AtmosUniform>::default(), UniformComponentPlugin::<AtmosUniform>::default()))
            .add_systems(PostUpdate, follow_camera.after(bevy::transform::TransformSystems::Propagate).after(bevy::camera::CameraUpdateSystems));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_resource::<AtmosTextures>()
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(
                Core3d,
                atmos_pass
                    .in_set(Core3dSystems::EarlyPostProcess)
                    .before(crate::fsr::fsr_pass)
                    .before(crate::ssao::ao_before_fsr),
            );
    }
}

fn follow_camera(sky: Option<Res<crate::sky::SkyState>>, mut cams: Query<(&GlobalTransform, &Projection, &Camera, &mut VolumetricAtmosphere, &mut AtmosUniform), With<FlyCam>>) {
    // Indoors the cell's own fog stands (no sky light comes in to scatter).
    let indoors = sky.is_some_and(|s| s.interior.is_some());
    for (transform, projection, camera, mut atmos, mut uniform) in &mut cams {
        let Projection::Perspective(p) = projection else { continue };
        let a = transform.affine();
        let world_from_view = DMat4::from_cols(
            a.matrix3.x_axis.as_dvec3().extend(0.0),
            a.matrix3.y_axis.as_dvec3().extend(0.0),
            a.matrix3.z_axis.as_dvec3().extend(0.0),
            a.translation.as_dvec3().extend(1.0),
        );
        let clip_from_world = p.get_clip_from_view().as_dmat4() * world_from_view.inverse();
        let camera_pos: DVec3 = a.translation.as_dvec3();
        // A camera that jumped (a door, fast travel) starts the accumulation again.
        let (prev, valid) = match atmos.prev {
            Some((prev, at)) if (at - camera_pos).length() < JUMP => (prev, 1.0),
            _ => (clip_from_world, 0.0),
        };
        atmos.prev = Some((clip_from_world, camera_pos));
        atmos.frame = atmos.frame.wrapping_add(1);
        let size = camera.physical_viewport_size().unwrap_or(UVec2::ONE).as_vec2();
        let half = ((size + 1.0) * 0.5).floor().max(Vec2::ONE);
        let t = *TUNING;
        let rel = DMat4::from_translation(camera_pos);
        let prev_clip_from_rel = (prev * rel).as_mat4();
        *uniform = AtmosUniform {
            prev_clip_from_rel,
            params: Vec4::new(if indoors { 0.0 } else { t[0] }, 1.0 / t[1].max(1.0), a.translation.y - BELOW_CAMERA, REACH),
            params2: Vec4::new(t[2], t[3], t[4], (atmos.frame % 4096) as f32),
            size: Vec4::new(size.x, size.y, half.x, half.y),
            params3: Vec4::new(NEW_SHARE, valid, 0.0, 0.0),
        };
    }
}

const LIGHT_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const DIST_FORMAT: TextureFormat = TextureFormat::R16Float;

#[derive(Resource)]
struct AtmosPipelines {
    /// Group 1 by whether the depth is multisampled.
    layouts: [BindGroupLayoutDescriptor; 2],
    sampler: Sampler,
    /// march, temporal, composite: by the view's mesh layout key, multisampling and the
    /// picture's format.
    ids: HashMap<(MeshPipelineViewLayoutKey, bool, TextureFormat), [CachedRenderPipelineId; 3]>,
    shader: Handle<Shader>,
    vertex: bevy::render::render_resource::VertexState,
}

fn init_pipelines(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>) {
    let layout = |multi: bool| {
        let depth = if multi { texture_depth_2d_multisampled() } else { texture_depth_2d() };
        BindGroupLayoutDescriptor::new(
            "atmos_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    depth,
                    uniform_buffer::<AtmosUniform>(true),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                ),
            ),
        )
    };
    let sampler = device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() });
    commands.insert_resource(AtmosPipelines {
        layouts: [layout(false), layout(true)],
        sampler,
        ids: HashMap::default(),
        shader: assets.load("embedded://nvforever/atmos.wesl"),
        vertex: fullscreen.to_vertex_state(),
    });
}

struct Tex {
    _texture: Texture,
    view: TextureView,
}

fn make(device: &RenderDevice, label: &'static str, size: UVec2, format: TextureFormat) -> Tex {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d { width: size.x.max(1), height: size.y.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&default());
    Tex { _texture: texture, view }
}

struct ViewTextures {
    size: UVec2,
    march: Tex,
    dist: Tex,
    accum: [Tex; 2],
}

#[derive(Resource, Default)]
struct AtmosTextures(HashMap<RetainedViewEntity, ViewTextures>);

#[derive(Component)]
struct AtmosView {
    key: RetainedViewEntity,
    ids: [CachedRenderPipelineId; 3],
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    mut commands: Commands,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<AtmosPipelines>,
    mut textures: ResMut<AtmosTextures>,
    layouts: Res<MeshPipelineViewLayouts>,
    keys: Res<ViewKeyCache>,
    views: Query<(Entity, &ExtractedView, &Msaa), With<AtmosUniform>>,
    mut seen: Local<Vec<RetainedViewEntity>>,
) {
    seen.clear();
    for (entity, view, msaa) in &views {
        if view.target_format != TextureFormat::Rgba16Float {
            continue;
        }
        let Some(mesh_key) = keys.get(&view.retained_view_entity) else { continue };
        let view_key: MeshPipelineViewLayoutKey = (*mesh_key).into();
        let multi = msaa.samples() > 1;
        let format = view.target_format;
        let p = &mut *pipelines;
        let ids = *p.ids.entry((view_key, multi, format)).or_insert_with(|| {
            let mesh_layout = layouts.get_view_layout(view_key).main_layout.clone();
            let mut shader_defs: Vec<bevy::shader::ShaderDefVal> = Vec::new();
            if multi {
                shader_defs.push("MULTISAMPLED".into());
            }
            let blend = BlendState {
                color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::SrcAlpha, operation: BlendOperation::Add },
                alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
            };
            let entries: [(&str, Vec<Option<ColorTargetState>>); 3] = [
                (
                    "march",
                    vec![
                        Some(ColorTargetState { format: LIGHT_FORMAT, blend: None, write_mask: ColorWrites::ALL }),
                        Some(ColorTargetState { format: DIST_FORMAT, blend: None, write_mask: ColorWrites::ALL }),
                    ],
                ),
                ("temporal", vec![Some(ColorTargetState { format: LIGHT_FORMAT, blend: None, write_mask: ColorWrites::ALL })]),
                ("composite", vec![Some(ColorTargetState { format, blend: Some(blend), write_mask: ColorWrites::ALL })]),
            ];
            entries.map(|(entry, targets)| {
                cache.queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some(format!("atmos_{entry}").into()),
                    layout: vec![mesh_layout.clone(), p.layouts[multi as usize].clone()],
                    vertex: p.vertex.clone(),
                    fragment: Some(FragmentState {
                        shader: p.shader.clone(),
                        entry_point: Some(entry.into()),
                        shader_defs: shader_defs.clone(),
                        targets,
                        ..default()
                    }),
                    ..default()
                })
            })
        });
        let size = view.viewport.zw().max(UVec2::ONE);
        let key = view.retained_view_entity;
        seen.push(key);
        if textures.0.get(&key).is_none_or(|t| t.size != size) {
            let half = (size + UVec2::ONE) / 2;
            textures.0.insert(
                key,
                ViewTextures {
                    size,
                    march: make(&device, "atmos_march", half, LIGHT_FORMAT),
                    dist: make(&device, "atmos_dist", half, DIST_FORMAT),
                    accum: [make(&device, "atmos_accum_0", half, LIGHT_FORMAT), make(&device, "atmos_accum_1", half, LIGHT_FORMAT)],
                },
            );
        }
        commands.entity(entity).insert(AtmosView { key, ids });
    }
    textures.0.retain(|k, _| seen.contains(k));
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn atmos_pass(
    view: ViewQuery<(&DynamicUniformIndex<AtmosUniform>, &AtmosUniform, &AtmosView, &ViewPrepassTextures, &ViewTarget, &MeshViewBindGroup)>,
    pipelines: Res<AtmosPipelines>,
    textures: Res<AtmosTextures>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<AtmosUniform>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (index, uniform, av, prepass, target, mesh_view) = view.into_inner();
    let Some(multi) = crate::ssao::depth_multisampled(prepass) else { return };
    let Some(depth) = prepass.depth_only_view() else { return };
    let Some(t) = textures.0.get(&av.key) else { return };
    let (Some(march), Some(temporal), Some(composite)) = (cache.get_render_pipeline(av.ids[0]), cache.get_render_pipeline(av.ids[1]), cache.get_render_pipeline(av.ids[2])) else {
        return;
    };
    if !crate::ssao::format_matches(target, target.main_texture_format()) || target.main_texture_format() != TextureFormat::Rgba16Float {
        return;
    }
    let Some(binding) = uniforms.uniforms().binding() else { return };
    let layout = cache.get_bind_group_layout(&pipelines.layouts[multi as usize]);
    let parity = (uniform.params2.w as u32 % 2) as usize;
    let (accum_now, accum_then) = (&t.accum[parity].view, &t.accum[1 - parity].view);
    let s = &pipelines.sampler;
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_volumetric_atmosphere");
    let passes: [(&str, &bevy::render::render_resource::RenderPipeline, bevy::render::render_resource::BindGroup, Vec<&TextureView>, bool); 3] = [
        (
            "atmos_march",
            march,
            // The textures are unused here; any but the targets.
            device.create_bind_group("atmos_march", &layout, &BindGroupEntries::sequential((depth, binding.clone(), accum_then, s, accum_then))),
            vec![&t.march.view, &t.dist.view],
            false,
        ),
        (
            "atmos_temporal",
            temporal,
            device.create_bind_group("atmos_temporal", &layout, &BindGroupEntries::sequential((depth, binding.clone(), &t.march.view, s, accum_then))),
            vec![accum_now],
            false,
        ),
        (
            "atmos_composite",
            composite,
            device.create_bind_group("atmos_composite", &layout, &BindGroupEntries::sequential((depth, binding, accum_now, s, &t.dist.view))),
            vec![target.main_texture_view()],
            true,
        ),
    ];
    for (label, pipeline, bg, targets, load) in passes {
        let ops = if load { Operations { load: LoadOp::Load, store: StoreOp::Store } } else { Operations::default() };
        let attachments: Vec<Option<RenderPassColorAttachment>> =
            targets.iter().map(|v| Some(RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops })).collect();
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &mesh_view.main, &mesh_view.main_offsets);
        pass.set_bind_group(1, &bg, &[index.index()]);
        pass.draw(0..3, 0..1);
    }
    span.end(ctx.command_encoder());
}
