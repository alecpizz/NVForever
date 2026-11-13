//! Screen-space global illumination of the remastered look (`graphics.rs` switch
//! `global-illumination`, docs/design/graphics-remaster.md §2): light bounced between the
//! surfaces on the screen, and how open each pixel is to the ambient light, worked out
//! before the main pass and given to the lighting of every surface that writes depth
//! (bevy_pbr `ScreenIndirectLight`, the vendored view binding; lights.rs `nvf_open`,
//! `nvf_bounce`). An engineering choice, not the original game's.
//!
//! Per frame (gi.wesl):
//! 1. after the depth prepass, at half resolution: a visibility-bitmask march of two
//!    slices per pixel through the depth, taking the light of what it finds from last
//!    frame's HDR picture (reprojected with the camera's motion); the share of the
//!    hemisphere left open is the ambient light's occlusion;
//! 2. that blended with last frame's result where the surface is the same;
//! 3. a depth-aware 3 x 3 upsample to the picture's size, which the main pass reads;
//! 4. after the main passes, before tonemapping: the HDR picture at half size with its
//!    distances, for the next frame's step 1.
//!
//! Multi-bounce comes for free: the picture step 1 reads already holds last frame's
//! bounce. Off the screen nothing bounces; the ambient light stands in for the sky.
//!
//! On the main camera only, while it carries [`GlobalIllumination`].

use crate::fly::FlyCam;
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::math::{DAffine3, DMat4};
use bevy::pbr::{ScreenIndirectLight, ScreenIndirectTexture};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_2d_array, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FilterMode,
    FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType,
    SamplerDescriptor, ShaderStages, ShaderType, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, RetainedViewEntity, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

/// Reach of the march around a pixel, in game units (about 4 m) [engineering choice, by
/// eye: a room's corner and the ground under a car; much more and walls darkened whole].
const RADIUS: f32 = 450.0;
/// How far behind what the depth shows a surface is taken to reach (game units).
const THICKNESS: f32 = 40.0;
/// Strength of the bounced light [engineering choice, by eye].
const BOUNCE: f32 = 3.0;
/// The most the ambient light is occluded (0..1): dim rooms lit mostly by their ambient
/// light keep their detail in the corners.
const OCCLUSION: f32 = 0.45;
/// The effect fades out by this distance (game units); beyond it a pixel is a few game
/// units wide and the fog takes over.
const FADE_END: f32 = 9000.0;
/// The share of this frame's march in the accumulated result.
const NEW_SHARE: f32 = 0.12;
/// The most light a texel of last frame's picture bounces (HDR, after exposure): the
/// Strip's neon still tints the walls beside it, a sun glint does not flash.
const MAX_LIGHT: f32 = 8.0;
/// Light added indoors where the surroundings are open (linear, after exposure): FNV's
/// dimmest rooms crush to black under the occlusion and the grade [engineering choice, by
/// eye on the Prospector Saloon].
const INTERIOR_FILL: f32 = 0.07;
/// Strength of the emissive glow field (gi.wesl `glow_*`): the light of bright
/// emitters (neon, bulbs, fire) spread wide onto what is near them.
const GLOW: f32 = 7.0;
/// Strength of the light the world rays (height field) bring back from what they hit.
const WORLD_BOUNCE: f32 = 0.8;
/// How much of the sky the world rays may shut out (0..1).
const WORLD_OCCLUSION: f32 = 0.25;
/// Strength of the light probe's light (probe.rs) on surfaces, by their normal.
const PROBE: f32 = 0.6;
/// A camera that moved further than this in a frame (game units) starts again.
const JUMP: f64 = 1500.0;

/// Radius, thickness, bounce, occlusion, debug view, its scale and the glow field's
/// strength; `NVF_GI="r,t,b,o,v,s,g"`
/// overrides them (a test aid for tuning by eye; gi.wesl `Gi::debug`).
static TUNING: std::sync::LazyLock<[f32; 7]> = std::sync::LazyLock::new(|| {
    let mut t = [RADIUS, THICKNESS, BOUNCE, OCCLUSION, 0.0, 1.0, GLOW];
    if let Ok(v) = std::env::var("NVF_GI") {
        for (slot, part) in t.iter_mut().zip(v.split(',')) {
            if let Ok(x) = part.trim().parse() {
                *slot = x;
            }
        }
    }
    t
});

/// Screen-space global illumination on a camera (main world); [`follow_camera`] keeps
/// the uniform up to date.
#[derive(Component, Clone, Copy, Default)]
#[require(GiUniform, ScreenIndirectLight)]
pub struct GlobalIllumination {
    prev: Option<DAffine3>,
    frame: u32,
}

/// The passes' uniform (gi.wesl `Gi`).
#[derive(Component, ExtractComponent, Clone, Copy, Default, ShaderType)]
#[extract_app(bevy::render::RenderApp)]
pub struct GiUniform {
    pub prev_view_from_view: Mat4,
    pub proj: Vec4,
    pub size: Vec4,
    pub params: Vec4,
    pub params2: Vec4,
    pub debug: Vec4,
    /// This frame's world from view (rotation only): the probe's light by world normal.
    pub world_from_view: Mat4,
    /// The light probe (probe.rs): x its strength.
    pub probe: Vec4,
    /// The camera's world position (xyz).
    pub cam_world: Vec4,
    /// The height field (probe.rs `HeightField`): x, y its centre (world x, z), z its span,
    /// w 1 when it holds the world around the camera.
    pub hf: Vec4,
    /// x the height camera's height, y its far plane; z the world rays' bounce strength,
    /// w their occlusion strength.
    pub hf2: Vec4,
}

pub struct GiPlugin;

impl Plugin for GiPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "gi.wesl");
        bevy::asset::embedded_asset!(app, "gi_height.wesl");
        app.add_plugins((
            ExtractComponentPlugin::<GiUniform>::default(),
            UniformComponentPlugin::<GiUniform>::default(),
            ExtractComponentPlugin::<ScreenIndirectLight>::default(),
        ))
        .add_systems(PostUpdate, follow_camera.after(bevy::transform::TransformSystems::Propagate).after(bevy::camera::CameraUpdateSystems));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_resource::<GiTextures>()
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(Core3d, gi_pass.after(Core3dSystems::Prepass).before(Core3dSystems::MainPass))
            .add_systems(Core3d, capture_pass.after(Core3dSystems::MainPass).before(Core3dSystems::EarlyPostProcess));
    }
}

/// The camera's motion since last frame and its projection into the uniform.
fn follow_camera(sky: Option<Res<crate::sky::SkyState>>, field: Option<Res<crate::probe::HeightField>>, mut cams: Query<(&GlobalTransform, &Projection, &Camera, &mut GlobalIllumination, &mut GiUniform), With<FlyCam>>) {
    let indoors = sky.as_ref().is_some_and(|s| s.interior.is_some());
    for (transform, projection, camera, mut gi, mut uniform) in &mut cams {
        let Projection::Perspective(p) = projection else { continue };
        let a = transform.affine();
        let now = DAffine3::from_mat3_translation(a.matrix3.as_dmat3(), a.translation.as_dvec3());
        let (prev_from_now, valid) = match gi.prev {
            Some(prev) if (prev.translation - now.translation).length() < JUMP => (DMat4::from(prev.inverse() * now), 1.0),
            _ => (DMat4::IDENTITY, 0.0),
        };
        gi.prev = Some(now);
        gi.frame = gi.frame.wrapping_add(1);
        let y = 1.0 / (p.fov * 0.5).tan();
        let x = y / p.aspect_ratio.max(1e-3);
        let size = camera.physical_viewport_size().unwrap_or(UVec2::ONE).as_vec2();
        let half = ((size + 1.0) * 0.5).floor().max(Vec2::ONE);
        let t = *TUNING;
        *uniform = GiUniform {
            prev_view_from_view: prev_from_now.as_mat4(),
            proj: Vec4::new(x, y, p.near, (gi.frame % 4096) as f32),
            size: Vec4::new(size.x, size.y, half.x, half.y),
            params: Vec4::new(t[0], t[1], t[2], FADE_END),
            params2: Vec4::new(t[3], NEW_SHARE, MAX_LIGHT, valid),
            debug: Vec4::new(t[4], t[5], if indoors { INTERIOR_FILL } else { 0.0 }, t[6]),
            world_from_view: Mat4::from_mat3(Mat3::from(a.matrix3)),
            probe: Vec4::new(PROBE, 0.0, 0.0, 0.0),
            cam_world: a.translation.extend(1.0),
            hf: field.as_ref().map_or(Vec4::ZERO, |f| {
                // Valid while the field holds this camera's surroundings.
                let near = (Vec2::new(f.origin.x, f.origin.z) - Vec2::new(a.translation.x, a.translation.z)).length() < crate::probe::HEIGHT_SPAN * 0.25;
                Vec4::new(f.origin.x, f.origin.z, crate::probe::HEIGHT_SPAN, if f.origin.w > 0.5 && near && !indoors { 1.0 } else { 0.0 })
            }),
            hf2: Vec4::new(field.as_ref().map_or(0.0, |f| f.origin.y), crate::probe::HEIGHT_FAR, WORLD_BOUNCE, WORLD_OCCLUSION),
        };
    }
}

const LIGHT_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const DIST_FORMAT: TextureFormat = TextureFormat::R16Float;

#[derive(Resource)]
struct GiPipelines {
    /// The height field's pass: the top-down camera's depth into heights.
    height_layout: BindGroupLayoutDescriptor,
    height_id: CachedRenderPipelineId,
    /// One layout for every entry, by whether the depth is multisampled.
    layouts: [BindGroupLayoutDescriptor; 2],
    sampler: Sampler,
    /// trace, temporal, upsample, capture; by multisampling.
    ids: [[CachedRenderPipelineId; 8]; 2],
}

fn init_pipelines(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, cache: Res<PipelineCache>) {
    let layout = |multi: bool| {
        let depth = if multi { texture_depth_2d_multisampled() } else { texture_depth_2d() };
        let tex = || texture_2d(TextureSampleType::Float { filterable: true });
        BindGroupLayoutDescriptor::new(
            "gi_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    depth,
                    uniform_buffer::<GiUniform>(true),
                    tex(),
                    sampler(SamplerBindingType::Filtering),
                    tex(),
                    tex(),
                    tex(),
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                    tex(),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                    tex(),
                ),
            ),
        )
    };
    let layouts = [layout(false), layout(true)];
    let shader = assets.load("embedded://nvforever/gi.wesl");
    let target = |f: TextureFormat| Some(ColorTargetState { format: f, blend: None, write_mask: ColorWrites::ALL });
    let entries: [(&str, Vec<Option<ColorTargetState>>); 8] = [
        ("trace", vec![target(LIGHT_FORMAT), target(DIST_FORMAT)]),
        ("temporal", vec![target(LIGHT_FORMAT)]),
        ("upsample", vec![target(LIGHT_FORMAT)]),
        ("capture", vec![target(LIGHT_FORMAT)]),
        ("glow_down", vec![target(LIGHT_FORMAT)]),
        ("glow_across", vec![target(LIGHT_FORMAT)]),
        ("glow_down_blur", vec![target(LIGHT_FORMAT)]),
        ("probe_reduce", vec![target(LIGHT_FORMAT)]),
    ];
    let ids = [false, true].map(|multi| {
        let mut out = [CachedRenderPipelineId::INVALID; 8];
        for (i, (entry, targets)) in entries.iter().enumerate() {
            let mut shader_defs = Vec::new();
            if multi {
                shader_defs.push("MULTISAMPLED".into());
            }
            out[i] = cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(format!("gi_{entry}").into()),
                layout: vec![layouts[multi as usize].clone()],
                vertex: fullscreen.to_vertex_state(),
                fragment: Some(FragmentState { shader: shader.clone(), entry_point: Some((*entry).into()), shader_defs, targets: targets.clone(), ..default() }),
                ..default()
            });
        }
        out
    });
    let sampler = device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() });
    let height_layout = BindGroupLayoutDescriptor::new(
        "gi_height_layout",
        &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_depth_2d(), uniform_buffer::<GiUniform>(true))),
    );
    let height_id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("gi_height_from_depth".into()),
        layout: vec![height_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: assets.load("embedded://nvforever/gi_height.wesl"),
            entry_point: Some("height_from_depth".into()),
            targets: vec![Some(ColorTargetState { format: TextureFormat::R32Float, blend: None, write_mask: ColorWrites::ALL })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(GiPipelines { layouts, sampler, ids, height_layout, height_id });
}

struct Tex {
    texture: Texture,
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
    Tex { texture, view }
}

/// The light probe's faces as one 2D array (probe.rs `FACES` order), copied into.
fn make_probe_array(device: &RenderDevice) -> Tex {
    let size = crate::probe::FACE_SIZE;
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("gi_probe_faces"),
        size: Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: LIGHT_FORMAT,
        usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&bevy::render::render_resource::TextureViewDescriptor {
        dimension: Some(bevy::render::render_resource::TextureViewDimension::D2Array),
        ..default()
    });
    Tex { texture, view }
}

/// A view's textures, kept from frame to frame (history and accumulation).
struct ViewTextures {
    size: UVec2,
    history: Tex,
    trace: Tex,
    dist: Tex,
    accum: [Tex; 2],
    full: Tex,
    /// Placeholder for unused bindings.
    blank: Tex,
    /// The emissive glow field (gi.wesl `glow_*`), a sixteenth of the picture's size;
    /// ping-ponged by the blur.
    glow: [Tex; 2],
    /// The light probe's faces (probe.rs), and their mean light per face (6 x 1).
    probe: Tex,
    cube: Tex,
    /// The height field's heights (world y, R32Float), from the top-down camera's depth.
    height: Tex,
}

#[derive(Resource, Default)]
struct GiTextures(HashMap<RetainedViewEntity, ViewTextures>);

#[derive(Component)]
struct GiView(RetainedViewEntity);

fn prepare(
    mut commands: Commands,
    device: Res<RenderDevice>,
    pipelines: Option<Res<GiPipelines>>,
    mut textures: ResMut<GiTextures>,
    views: Query<(Entity, &ExtractedView), With<GiUniform>>,
    mut seen: Local<Vec<RetainedViewEntity>>,
) {
    seen.clear();
    for (entity, view) in &views {
        let size = view.viewport.zw().max(UVec2::ONE);
        let key = view.retained_view_entity;
        seen.push(key);
        if textures.0.get(&key).is_none_or(|t| t.size != size) {
            let half = (size + UVec2::ONE) / 2;
            textures.0.insert(
                key,
                ViewTextures {
                    size,
                    history: make(&device, "gi_history", half, LIGHT_FORMAT),
                    trace: make(&device, "gi_trace", half, LIGHT_FORMAT),
                    dist: make(&device, "gi_dist", half, DIST_FORMAT),
                    accum: [make(&device, "gi_accum_0", half, LIGHT_FORMAT), make(&device, "gi_accum_1", half, LIGHT_FORMAT)],
                    full: make(&device, "gi_full", size, LIGHT_FORMAT),
                    blank: make(&device, "gi_blank", UVec2::ONE, LIGHT_FORMAT),
                    glow: [make(&device, "gi_glow_0", (half + UVec2::splat(7)) / 8, LIGHT_FORMAT), make(&device, "gi_glow_1", (half + UVec2::splat(7)) / 8, LIGHT_FORMAT)],
                    probe: make_probe_array(&device),
                    cube: make(&device, "gi_probe_cube", UVec2::new(6, 1), LIGHT_FORMAT),
                    height: make(&device, "gi_height", UVec2::splat(crate::probe::HEIGHT_SIZE), TextureFormat::R32Float),
                },
            );
        }
        let t = &textures.0[&key];
        let probe = pipelines.as_ref().map(|p| (t.probe.view.clone(), p.sampler.clone()));
        commands.entity(entity).insert((ScreenIndirectTexture { texture: t.full.view.clone(), probe }, GiView(key)));
    }
    textures.0.retain(|k, _| seen.contains(k));
}

fn run(ctx: &mut RenderContext, pipeline: &bevy::render::render_resource::RenderPipeline, bind_group: &bevy::render::render_resource::BindGroup, index: u32, label: &str, targets: &[&TextureView]) {
    let attachments: Vec<Option<RenderPassColorAttachment>> = targets
        .iter()
        .map(|v| Some(RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: Operations::default() }))
        .collect();
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[index]);
    pass.draw(0..3, 0..1);
}

/// Steps 1-3 (module notes): march, accumulate, upsample.
#[allow(clippy::too_many_arguments)]
fn gi_pass(
    view: ViewQuery<(&DynamicUniformIndex<GiUniform>, &GiUniform, &GiView, &ViewPrepassTextures)>,
    probe_images: Option<Res<crate::probe::ProbeImages>>,
    gpu_images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
    field: Option<Res<crate::probe::HeightField>>,
    height_cams: Query<&ViewPrepassTextures, With<crate::probe::HeightCamera>>,
    pipelines: Res<GiPipelines>,
    textures: Res<GiTextures>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<GiUniform>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (index, uniform, gi_view, prepass) = view.into_inner();
    let Some(multi) = crate::ssao::depth_multisampled(prepass) else { return };
    let Some(depth) = prepass.depth_only_view() else { return };
    let Some(t) = textures.0.get(&gi_view.0) else { return };
    let ids = pipelines.ids[multi as usize];
    let (Some(trace), Some(temporal), Some(upsample)) = (cache.get_render_pipeline(ids[0]), cache.get_render_pipeline(ids[1]), cache.get_render_pipeline(ids[2])) else {
        return;
    };
    let (Some(g0), Some(g1), Some(g2), Some(reduce)) =
        (cache.get_render_pipeline(ids[4]), cache.get_render_pipeline(ids[5]), cache.get_render_pipeline(ids[6]), cache.get_render_pipeline(ids[7]))
    else {
        return;
    };
    let glow = [g0, g1, g2];
    let Some(binding) = uniforms.uniforms().binding() else { return };
    let layout = cache.get_bind_group_layout(&pipelines.layouts[multi as usize]);
    let parity = (uniform.proj.w as u32 % 2) as usize;
    // The height field's colour (the top-down camera's picture), or a placeholder.
    let hf_colour = field.as_ref().and_then(|f| gpu_images.get(&f.image)).map_or(&t.blank.view, |g| &g.texture_view);
    // The frame the top-down camera drew: its depth into heights, before the march.
    if field.as_ref().is_some_and(|f| f.fresh) {
        if let (Some(depth_view), Some(pipeline)) = (height_cams.iter().find_map(|p| p.depth_only_view().cloned()), cache.get_render_pipeline(pipelines.height_id)) {
            let hl = cache.get_bind_group_layout(&pipelines.height_layout);
            let bg = device.create_bind_group("gi_height", &hl, &BindGroupEntries::sequential((&depth_view, binding.clone())));
            run(&mut ctx, pipeline, &bg, index.index(), "gi_height_from_depth", &[&t.height.view]);
        }
    }
    let (accum_now, accum_then) = (&t.accum[parity].view, &t.accum[1 - parity].view);
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_global_illumination");
    let s = &pipelines.sampler;
    let bg = device.create_bind_group("gi_trace", &layout, &BindGroupEntries::sequential((depth, binding.clone(), &t.history.view, s, &t.blank.view, &t.blank.view, &t.blank.view, &t.probe.view, &t.cube.view, &t.height.view, hf_colour)));
    run(&mut ctx, trace, &bg, index.index(), "gi_trace", &[&t.trace.view, &t.dist.view]);
    let bg = device.create_bind_group("gi_temporal", &layout, &BindGroupEntries::sequential((depth, binding.clone(), &t.trace.view, s, accum_then, &t.dist.view, &t.history.view, &t.probe.view, &t.cube.view, &t.height.view, hf_colour)));
    run(&mut ctx, temporal, &bg, index.index(), "gi_temporal", &[accum_now]);
    // The light probe: its faces into the array, then their mean light per face.
    if let Some(probe) = probe_images.as_ref() {
        let size = crate::probe::FACE_SIZE;
        for (layer, handle) in probe.faces.iter().enumerate() {
            let Some(face) = gpu_images.get(handle) else { continue };
            ctx.command_encoder().copy_texture_to_texture(
                face.texture.as_image_copy(),
                bevy::render::render_resource::TexelCopyTextureInfo {
                    texture: &t.probe.texture,
                    mip_level: 0,
                    origin: bevy::render::render_resource::Origin3d { x: 0, y: 0, z: layer as u32 },
                    aspect: bevy::render::render_resource::TextureAspect::All,
                },
                Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            );
        }
    }
    let bg = device.create_bind_group("gi_probe_reduce", &layout, &BindGroupEntries::sequential((depth, binding.clone(), &t.blank.view, s, &t.blank.view, &t.blank.view, &t.blank.view, &t.probe.view, &t.blank.view, &t.height.view, hf_colour)));
    run(&mut ctx, reduce, &bg, index.index(), "gi_probe_reduce", &[&t.cube.view]);
    // The glow field from last frame's picture: bright texels, then blurred both ways.
    let glow_steps = [(&t.history.view, &t.glow[0].view, "gi_glow_down"), (&t.glow[0].view, &t.glow[1].view, "gi_glow_across"), (&t.glow[1].view, &t.glow[0].view, "gi_glow_down_blur")];
    for (pipeline, (from, to, label)) in glow.into_iter().zip(glow_steps) {
        let bg = device.create_bind_group(label, &layout, &BindGroupEntries::sequential((depth, binding.clone(), from, s, &t.blank.view, &t.blank.view, &t.blank.view, &t.probe.view, &t.cube.view, &t.height.view, hf_colour)));
        run(&mut ctx, pipeline, &bg, index.index(), label, &[to]);
    }
    let bg = device.create_bind_group("gi_upsample", &layout, &BindGroupEntries::sequential((depth, binding, accum_now, s, &t.dist.view, &t.glow[0].view, &t.history.view, &t.probe.view, &t.cube.view, &t.height.view, hf_colour)));
    run(&mut ctx, upsample, &bg, index.index(), "gi_upsample", &[&t.full.view]);
    span.end(ctx.command_encoder());
}

/// Step 4: the HDR picture and its distances at half size, for the next frame.
fn capture_pass(
    view: ViewQuery<(&DynamicUniformIndex<GiUniform>, &GiView, &ViewPrepassTextures, &ViewTarget)>,
    pipelines: Res<GiPipelines>,
    textures: Res<GiTextures>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<GiUniform>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (index, gi_view, prepass, target) = view.into_inner();
    let Some(multi) = crate::ssao::depth_multisampled(prepass) else { return };
    let Some(depth) = prepass.depth_only_view() else { return };
    let Some(t) = textures.0.get(&gi_view.0) else { return };
    let Some(capture) = cache.get_render_pipeline(pipelines.ids[multi as usize][3]) else { return };
    let Some(binding) = uniforms.uniforms().binding() else { return };
    let layout = cache.get_bind_group_layout(&pipelines.layouts[multi as usize]);
    let picture: &TextureView = target.main_texture_view();
    let s = &pipelines.sampler;
    let bg = device.create_bind_group("gi_capture", &layout, &BindGroupEntries::sequential((depth, binding, picture, s, &t.blank.view, &t.blank.view, &t.blank.view, &t.probe.view, &t.cube.view, &t.height.view, &t.blank.view)));
    run(&mut ctx, capture, &bg, index.index(), "gi_capture", &[&t.history.view]);
}
