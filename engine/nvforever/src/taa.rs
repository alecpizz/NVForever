//! Temporal anti-aliasing of the enhanced graphics (`graphics.rs`,
//! docs/design/graphics-enhanced.md §3.6): the projection is shifted by a sub-pixel
//! amount every frame (Halton 2,3, eight steps) and each frame is blended into a history
//! of the frames before, found again where the camera's move put it.
//!
//! It is the other choice to multisampling, not on top of it: while it is on the main
//! view is drawn without multisampling (the alpha-tested plants' edges are then tested
//! per pixel and smoothed by the jitter over the frames instead of by alpha to coverage).
//!
//! Bevy's TAA needs motion vectors from every material's prepass; the game's NIF, grass,
//! tree and water prepasses write depth only. So this pass finds the history from the
//! depth and the camera alone: each pixel's point is carried from this frame's camera to
//! the last frame's (`reproject`, worked out on the CPU in double precision, the world
//! being up to hundreds of thousands of units across). What moves by itself (actors,
//! plants in the wind, the water) is found where it was not; the history is clipped to
//! the colours around the pixel this frame (variance clipping in YCoCg), so it is not
//! smeared but keeps less of the smoothing while it moves. The first-person arms move with
//! the camera: points nearer than [`CAMERA_LOCKED`] are taken as not moving on the
//! screen.
//!
//! On the tonemapped picture after the ambient occlusion, before bloom, sharpening and
//! the image-space grade and its blur. On the main camera only, while it carries
//! [`TemporalAa`].

use crate::fly::FlyCam;
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::math::{DMat4, DVec3};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::camera::{MipBias, TemporalJitter};
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer};
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

/// Share of this frame in the blend where the camera stands still [engineering choice:
/// about ten frames of history, the usual figure].
const BLEND: f32 = 0.1;
/// Share of this frame where the picture moves by [`FAST`] pixels or more a frame
/// [engineering choice: less history while moving, less blur].
const BLEND_MOVING: f32 = 0.5;
const FAST: f32 = 16.0;
/// Points nearer the camera than this (game units) are taken to move with it: the
/// first-person arms and the weapon [engineering choice; a wall this near also counts].
pub const CAMERA_LOCKED: f32 = 48.0;
/// Texture detail bias while the picture is jittered [engineering choice: a little sharper
/// than without, the jitter averages the extra detail out; Bevy's TAA uses -1].
const MIP_BIAS: f32 = -0.5;
/// The camera jumping further than this in a frame (game units) starts the history over
/// (doors, MoveTo, `--pos` test aids).
const CUT_DISTANCE: f64 = 400.0;

/// Temporal anti-aliasing on a camera. The fields are kept up to date by
/// [`follow_camera`]; only the camera that also has [`TemporalJitter`] is jittered.
#[derive(Component, ExtractComponent, Clone, Copy, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct TemporalAa {
    /// This frame's clip space (unjittered) to the last frame's.
    pub reproject: Mat4,
    /// x: the projection's near plane, y: [`CAMERA_LOCKED`], z: 1 when the history starts
    /// over this frame, w: unused.
    pub params: Vec4,
}

/// What the GPU gets (`taa.wesl` `Taa`).
#[derive(Clone, Copy, Default, ShaderType)]
struct TaaUniform {
    reproject: Mat4,
    /// x near, y camera-locked distance, z reset, w unused.
    params: Vec4,
    /// x still blend, y moving blend, z fast (pixels), w unused.
    blend: Vec4,
}

pub struct TaaPlugin;

impl Plugin for TaaPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "taa.wesl");
        app.add_plugins(ExtractComponentPlugin::<TemporalAa>::default())
            .add_systems(PostUpdate, follow_camera.after(bevy::camera::CameraUpdateSystems).after(bevy::transform::TransformSystems::Propagate));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_resource::<Histories>()
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources).after(bevy::core_pipeline::core_3d::prepare_prepass_textures))
            .add_systems(
                Core3d,
                taa_pass.in_set(Core3dSystems::PostProcess).after(tonemapping).after(crate::ssao::ao_pass).before(crate::imagespace::blur_pass),
            );
    }
}

/// The 2,3 Halton point `i` (1-based), each part in 0..1.
fn halton(i: u32) -> Vec2 {
    let radical = |mut n: u32, base: u32| {
        let (mut f, mut r) = (1.0f32, 0.0f32);
        while n > 0 {
            f /= base as f32;
            r += f * (n % base) as f32;
            n /= base;
        }
        r
    };
    Vec2::new(radical(i, 2), radical(i, 3))
}

/// The camera's world to clip matrix without jitter, in double precision.
fn clip_from_world(gt: &GlobalTransform, proj: &Projection) -> DMat4 {
    let clip_from_view = proj.get_clip_from_view().as_dmat4();
    clip_from_view * gt.to_matrix().as_dmat4().inverse()
}

/// Per frame: the next jitter step, the reprojection from this frame to the last, and
/// whether the history starts over (first frame, camera cut, window resized).
#[allow(clippy::type_complexity)]
fn follow_camera(
    mut cams: Query<(&GlobalTransform, &Projection, &Camera, Mut<TemporalAa>, Option<&mut TemporalJitter>), With<FlyCam>>,
    mut last: Local<Option<(DMat4, DVec3, UVec2)>>,
    mut step: Local<u32>,
) {
    let Ok((gt, proj, camera, mut taa, jitter)) = cams.single_mut() else {
        *last = None;
        return;
    };
    let Projection::Perspective(p) = proj else { return };
    *step = *step % 8 + 1;
    if let Some(mut j) = jitter {
        j.offset = halton(*step) - Vec2::splat(0.5);
    }
    let now = clip_from_world(gt, proj);
    let pos = gt.translation().as_dvec3();
    let size = camera.physical_viewport_size().unwrap_or(UVec2::ONE);
    let (reproject, reset) = match *last {
        Some((prev, prev_pos, prev_size)) if !taa.is_added() && prev_size == size && prev_pos.distance(pos) < CUT_DISTANCE => ((prev * now.inverse()).as_mat4(), 0.0),
        _ => (Mat4::IDENTITY, 1.0),
    };
    *last = Some((now, pos, size));
    taa.reproject = reproject;
    taa.params = Vec4::new(p.near, CAMERA_LOCKED, reset, 0.0);
}

/// The components that turn temporal anti-aliasing on for a camera (with `Msaa::Off`,
/// which `graphics.rs` sets).
pub fn components() -> (TemporalAa, TemporalJitter, MipBias) {
    (TemporalAa { params: Vec4::new(5.0, CAMERA_LOCKED, 1.0, 0.0), ..default() }, TemporalJitter::default(), MipBias(MIP_BIAS))
}

#[derive(Resource)]
pub(crate) struct TaaPipelines {
    /// Without and with the motion-vector prepass (Modern Feel's object motion blur puts it
    /// on the camera; object_blur.rs): with it, a pixel's history is found by its own motion.
    layouts: [BindGroupLayoutDescriptor; 2],
    sampler: Sampler,
    base: RenderPipelineDescriptor,
    by_format: HashMap<(TextureFormat, bool), CachedRenderPipelineId>,
}

/// Which history texture holds the last frame, whether it was written, and its size.
#[derive(Resource, Default)]
struct Histories(HashMap<Entity, (bool, bool, UVec2)>);

#[derive(Component)]
pub(crate) struct TaaView {
    pipeline: CachedRenderPipelineId,
    /// The picture's format the pipeline was built for.
    format: TextureFormat,
    /// The pipeline reads the motion-vector prepass.
    motion: bool,
    read: CachedTexture,
    write: CachedTexture,
    uniform: UniformBuffer<TaaUniform>,
}

const HISTORY_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

fn init_pipelines(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "taa_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_depth_2d(),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<TaaUniform>(false),
            ),
        ),
    );
    let motion_layout = BindGroupLayoutDescriptor::new(
        "taa_motion_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_depth_2d(),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<TaaUniform>(false),
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        ..default()
    });
    let base = RenderPipelineDescriptor {
        label: Some("taa_pipeline".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState { shader: assets.load("embedded://nvforever/taa.wesl"), ..default() }),
        ..default()
    };
    commands.insert_resource(TaaPipelines { layouts: [layout, motion_layout], sampler, base, by_format: HashMap::default() });
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<TaaPipelines>,
    mut textures: ResMut<TextureCache>,
    mut histories: ResMut<Histories>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    views: Query<(Entity, &ExtractedView, &Msaa, &TemporalAa, Option<&ViewPrepassTextures>)>,
) {
    let mut seen = Vec::new();
    for (entity, view, msaa, taa, prepass) in &views {
        seen.push(entity);
        let format = view.target_format;
        let motion = prepass.is_some_and(|p| p.motion_vectors.is_some());
        let pipeline = match pipelines.by_format.get(&(format, motion)) {
            Some(id) => *id,
            None => {
                let mut d = pipelines.base.clone();
                d.layout = vec![pipelines.layouts[motion as usize].clone()];
                if let Some(f) = d.fragment.as_mut() {
                    if motion {
                        f.shader_defs.push("MOTION_VECTORS".into());
                    }
                    f.targets = vec![
                        Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL }),
                        Some(ColorTargetState { format: HISTORY_FORMAT, blend: None, write_mask: ColorWrites::ALL }),
                    ];
                }
                let id = cache.queue_render_pipeline(d);
                pipelines.by_format.insert((format, motion), id);
                id
            }
        };
        let size = view.viewport.zw().max(UVec2::ONE);
        let mut history = |label: &'static str| {
            textures.get(
                &device,
                TextureDescriptor {
                    label: Some(label),
                    size: Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: HISTORY_FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let (a, b) = (history("taa_history_a"), history("taa_history_b"));
        let state = histories.0.entry(entity).or_insert((false, false, UVec2::ZERO));
        let (a_holds_last, written, last_size) = *state;
        // The pass runs this frame when its pipeline is ready and the view and its depth
        // single-sampled (the same test as `taa_pass`).
        let runs = cache.get_render_pipeline(pipeline).is_some() && *msaa == Msaa::Off && prepass.and_then(crate::ssao::depth_multisampled) == Some(false);
        let reset = taa.params.z > 0.5 || !written || last_size != size;
        *state = if runs { (!a_holds_last, true, size) } else { (a_holds_last, false, size) };
        let (read, write) = if a_holds_last { (a, b) } else { (b, a) };
        let mut uniform = UniformBuffer::from(TaaUniform {
            reproject: taa.reproject,
            params: taa.params.with_z(if reset { 1.0 } else { 0.0 }),
            blend: Vec4::new(BLEND, BLEND_MOVING, FAST, 0.0),
        });
        uniform.write_buffer(&device, &queue);
        commands.entity(entity).insert(TaaView { pipeline, format, motion, read, write, uniform });
    }
    histories.0.retain(|e, _| seen.contains(e));
}

/// This frame blended into the history: into the picture and into the next history.
pub(crate) fn taa_pass(
    view: ViewQuery<(&ViewTarget, &TaaView, &ViewPrepassTextures, &Msaa)>,
    pipelines: Res<TaaPipelines>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, taa, prepass, msaa) = view.into_inner();
    if *msaa != Msaa::Off || crate::ssao::depth_multisampled(prepass) != Some(false) || !crate::ssao::format_matches(target, taa.format) {
        return;
    }
    let Some(depth) = prepass.depth_only_view() else { return };
    let Some(pipeline) = cache.get_render_pipeline(taa.pipeline) else { return };
    let Some(uniform) = taa.uniform.binding() else { return };
    // GPU time of the pass (recorded with --log-frame-times, graphics.rs).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_taa");
    let post = target.post_process_write();
    let layout = cache.get_bind_group_layout(&pipelines.layouts[taa.motion as usize]);
    let bind_group = if taa.motion {
        let Some(motion) = prepass.motion_vectors_view() else { return };
        device.create_bind_group("taa_bind_group", &layout, &BindGroupEntries::sequential((post.source, &taa.read.default_view, depth, &pipelines.sampler, uniform, motion)))
    } else {
        device.create_bind_group("taa_bind_group", &layout, &BindGroupEntries::sequential((post.source, &taa.read.default_view, depth, &pipelines.sampler, uniform)))
    };
    let attachment = |view| Some(RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: Operations::default() });
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("taa_pass"),
        color_attachments: &[attachment(post.destination), attachment(&taa.write.default_view)],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.draw(0..3, 0..1);
    drop(pass);
    span.end(ctx.command_encoder());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halton_steps_cover_the_pixel() {
        let pts: Vec<Vec2> = (1..=8).map(halton).collect();
        assert_eq!(pts[0], Vec2::new(0.5, 1.0 / 3.0));
        assert!(pts.iter().all(|p| p.x > 0.0 && p.x < 1.0 && p.y > 0.0 && p.y < 1.0));
        let mean = pts.iter().copied().sum::<Vec2>() / 8.0;
        assert!((mean - Vec2::splat(0.5)).length() < 0.1, "{mean}");
    }

    #[test]
    fn reprojection_of_a_still_camera_is_identity() {
        let gt = GlobalTransform::from(Transform::from_xyz(-69000.0, 8900.0, 3500.0).looking_to(Vec3::X, Vec3::Y));
        let proj = Projection::Perspective(PerspectiveProjection { near: 5.0, ..default() });
        let m = clip_from_world(&gt, &proj);
        let r = (m * m.inverse()).as_mat4();
        assert!(r.abs_diff_eq(Mat4::IDENTITY, 1e-5));
    }
}
