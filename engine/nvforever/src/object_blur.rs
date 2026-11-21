//! Object motion blur (Modern Feel, `nvf_play::modern_feel::ModernFeel::object_motion_blur`,
//! docs/design/modern-feel-gfx.md §2) [design: NVForever's own, not the original's; every
//! constant an engineering choice tuned by eye].
//!
//! Moving things blur by their own motion: an actor running across the view, a door
//! swinging, a body thrown by a hit. The camera's own blur stays the game's (the image
//! space modifiers' motion blur, imagespace.rs), so turning the view leaves the still world
//! sharp, while an actor the camera tracks stays sharp too (it does not move on the screen).
//!
//! How: the main camera gets Bevy's motion-vector prepass (the NIF, grass and tree prepasses
//! already write it for the temporal upscalers: previous transforms, previous skinning and
//! last frame's wind). Each pixel's motion on the screen is compared with the motion the
//! camera alone gives a still point at its depth (the depth reprojected with the last
//! frame's camera, worked out in double precision as for the TAA). Where they differ the
//! pixel belongs to something that moves in the world, and its whole motion on the screen
//! (the camera's share included, so a tracked actor stays sharp) is its blur over a short
//! exposure ([`EXPOSURE`], a fixed time, so the blur looks the same at any frame rate),
//! held to [`MAX_RADIUS`]. The blur itself is a reconstruction filter with tile maxima
//! (McGuire et al., "A Reconstruction Filter for Plausible Motion Blur", I3D 2012
//! [documented: the paper's published method; our own code from its description]): the
//! largest motion of each 16 × 16 tile and its neighbours sets the line the taps lie on, and
//! each tap is weighted by whether it lies in front of or behind the pixel and whether its
//! own blur reaches it, so a moving actor smears over the still background and not the
//! other way round.
//!
//! Passes, on the tonemapped picture after the anti-aliasing and the ambient occlusion and
//! before the sun shafts and the image space: velocity (full size), tile maxima (across
//! rows of 16, then down columns of 16),
//! neighbour maxima (1/16), gather (full size). On the main camera only, while it carries
//! [`ObjectBlur`].

use crate::fly::FlyCam;
use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass, ViewPrepassTextures};
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::math::{DMat4, DVec3};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_2d_multisampled, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer,
};
use bevy::render::render_resource::{
    AddressMode, BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState, ColorWrites,
    Extent3d, FilterMode, FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages, TextureView, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use nvf_play::modern_feel::ModernFeel;

/// The exposure, in seconds: each moving point is smeared over where it went in this long
/// [design: half of a 60 fps frame, a 180° shutter at 60 fps; fixed in time so the blur
/// does not vanish at high frame rates].
const EXPOSURE: f32 = 1.0 / 120.0;
/// The longest blur radius, as a share of the picture's height [design: 30 pixels at
/// 1440p; a sprinting actor across the view reaches it, a walking one at a few metres
/// stays well below].
const MAX_RADIUS: f32 = 0.021;
/// What is nearer to the eye than this (game units) is the player's own arms and weapon (or
/// the camera is in something): not blurred, not smeared over [design: as imagespace.rs
/// `MOTION_SHARP`].
const SHARP: f32 = 40.0;
/// A camera that moved further than this (game units) or turned more than [`CUT_TURN`]
/// (radians) in a frame was cut to a new view: no blur that frame [design: as imagespace.rs].
const CUT_DISTANCE: f64 = 300.0;
const CUT_TURN: f32 = std::f32::consts::FRAC_PI_4;
/// Tile side of the maxima (pixels; object_blur.wesl `TILE`).
const TILE: u32 = 16;

/// Object motion blur on a camera, kept up to date by [`follow_camera`].
#[derive(Component, ExtractComponent, Clone, Copy, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct ObjectBlur {
    /// This frame's clip space (unjittered) to the last frame's.
    pub reproject: Mat4,
    /// x: exposure in frames (0: none this frame), y: the depth (reverse) nearer than which
    /// nothing blurs, z: largest radius (share of the height), w: near plane.
    pub params: Vec4,
}

/// What the GPU gets (object_blur.wesl `Blur`).
#[derive(Clone, Copy, Default, ShaderType)]
struct BlurUniform {
    reproject: Mat4,
    params: Vec4,
    /// xy: the motion vectors' region (texels; the render size under an upscaler), zw: the
    /// picture's size (pixels).
    sizes: Vec4,
}

pub struct ObjectBlurPlugin;

impl Plugin for ObjectBlurPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "object_blur.wesl");
        app.add_plugins(ExtractComponentPlugin::<ObjectBlur>::default())
            .add_systems(PostUpdate, follow_camera.after(bevy::camera::CameraUpdateSystems).after(bevy::transform::TransformSystems::Propagate));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(
                Core3d,
                blur_pass
                    .in_set(Core3dSystems::PostProcess)
                    .after(tonemapping)
                    .after(crate::ssao::ao_pass)
                    .after(crate::taa::taa_pass)
                    .after(bevy::anti_alias::fxaa::fxaa)
                    .after(bevy::anti_alias::smaa::smaa)
                    .before(crate::shafts::shafts_pass)
                    .before(crate::imagespace::blur_pass),
            );
    }
}

/// The camera's world to clip matrix without jitter, in double precision.
fn clip_from_world(gt: &GlobalTransform, proj: &Projection) -> DMat4 {
    proj.get_clip_from_view().as_dmat4() * gt.to_matrix().as_dmat4().inverse()
}

/// Per frame: put the switch's components on the main camera or take them off, and the
/// reprojection and exposure for this frame.
#[allow(clippy::type_complexity)]
fn follow_camera(
    mut commands: Commands,
    feel: Option<Res<ModernFeel>>,
    time: Res<Time>,
    mut cams: Query<
        (Entity, &GlobalTransform, &Projection, Option<&mut ObjectBlur>, Has<MotionVectorPrepass>, Has<crate::fsr::FsrUpscaling>, Has<crate::upscaler::DlssUpscaling>),
        With<FlyCam>,
    >,
    mut last: Local<Option<(DMat4, DVec3, Quat)>>,
    mut said: Local<Option<bool>>,
) {
    let on = feel.is_some_and(|f| f.object_motion_blur());
    let Ok((entity, gt, proj, blur, has_motion, fsr, dlss)) = cams.single_mut() else {
        *last = None;
        return;
    };
    if *said != Some(on) {
        info!("object motion blur: {}", if on { "on (motion vectors from the prepass, own motion only)" } else { "off" });
        *said = Some(on);
    }
    if !on {
        if blur.is_some() {
            let mut e = commands.entity(entity);
            e.remove::<ObjectBlur>();
            // The temporal upscalers keep the prepass they need (graphics.rs `apply`).
            if !fsr && !dlss {
                e.remove::<MotionVectorPrepass>();
            }
        }
        *last = None;
        return;
    }
    if !has_motion {
        // graphics.rs `apply` takes the prepass off whenever no upscaler needs it; put it
        // back (the pass skips the frame without it).
        commands.entity(entity).insert((DepthPrepass, MotionVectorPrepass));
    }
    let Projection::Perspective(p) = proj else { return };
    let now = clip_from_world(gt, proj);
    let pos = gt.translation().as_dvec3();
    let rot = gt.rotation();
    let dt = time.delta_secs();
    let (reproject, frames) = match *last {
        Some((prev, prev_pos, prev_rot)) if prev_pos.distance(pos) < CUT_DISTANCE && prev_rot.angle_between(rot) < CUT_TURN && dt > 0.0 => {
            ((prev * now.inverse()).as_mat4(), EXPOSURE / dt)
        }
        _ => (Mat4::IDENTITY, 0.0),
    };
    *last = Some((now, pos, rot));
    let new = ObjectBlur { reproject, params: Vec4::new(frames, p.near / SHARP, MAX_RADIUS, p.near) };
    match blur {
        Some(mut b) => *b = new,
        None => {
            commands.entity(entity).insert(new);
        }
    }
}

#[derive(Resource)]
pub(crate) struct BlurPipelines {
    /// The velocity pass's layouts with single-sampled and multisampled prepass textures.
    velocity_layouts: [BindGroupLayoutDescriptor; 2],
    /// Uniform and one texture (tile and neighbour maxima).
    single: BindGroupLayoutDescriptor,
    gather_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    base: RenderPipelineDescriptor,
    velocity: [CachedRenderPipelineId; 2],
    /// Tile maxima across, then down.
    tile: [CachedRenderPipelineId; 2],
    neighbour: CachedRenderPipelineId,
    gather: HashMap<TextureFormat, CachedRenderPipelineId>,
}

#[derive(Component)]
pub(crate) struct BlurView {
    gather: CachedRenderPipelineId,
    format: TextureFormat,
    velocity: CachedTexture,
    rows: CachedTexture,
    tiles: CachedTexture,
    neighbours: CachedTexture,
    uniform: UniformBuffer<BlurUniform>,
    on: bool,
}

const VELOCITY_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

fn init_pipelines(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, cache: Res<PipelineCache>) {
    let unfiltered = || texture_2d(TextureSampleType::Float { filterable: false });
    let velocity_layout = |multi: bool| {
        if multi {
            BindGroupLayoutDescriptor::new(
                "object_blur_velocity_ms_layout",
                &BindGroupLayoutEntries::sequential(
                    ShaderStages::FRAGMENT,
                    (uniform_buffer::<BlurUniform>(false), texture_depth_2d_multisampled(), texture_2d_multisampled(TextureSampleType::Float { filterable: false })),
                ),
            )
        } else {
            BindGroupLayoutDescriptor::new(
                "object_blur_velocity_layout",
                &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (uniform_buffer::<BlurUniform>(false), texture_depth_2d(), unfiltered())),
            )
        }
    };
    let single = BindGroupLayoutDescriptor::new("object_blur_tiles_layout", &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (uniform_buffer::<BlurUniform>(false), unfiltered())));
    let gather_layout = BindGroupLayoutDescriptor::new(
        "object_blur_gather_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<BlurUniform>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                unfiltered(),
                unfiltered(),
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
        label: Some("object_blur_pipeline".into()),
        layout: vec![single.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState { shader: assets.load("embedded://nvforever/object_blur.wesl"), ..default() }),
        ..default()
    };
    let make = |layout: &BindGroupLayoutDescriptor, entry: &'static str, defs: &[&'static str]| {
        let mut d = base.clone();
        d.layout = vec![layout.clone()];
        if let Some(f) = d.fragment.as_mut() {
            f.entry_point = Some(entry.into());
            f.targets = vec![Some(ColorTargetState { format: VELOCITY_FORMAT, blend: None, write_mask: ColorWrites::ALL })];
            f.shader_defs = defs.iter().map(|d| (*d).into()).collect();
        }
        cache.queue_render_pipeline(d)
    };
    let velocity_layouts = [velocity_layout(false), velocity_layout(true)];
    let velocity = [make(&velocity_layouts[0], "velocity", &["VELOCITY"]), make(&velocity_layouts[1], "velocity", &["VELOCITY", "MULTISAMPLED"])];
    let tile = [make(&single, "tile_max", &["TILE_MAX", "TILE_ACROSS"]), make(&single, "tile_max", &["TILE_MAX"])];
    let neighbour = make(&single, "neighbour_max", &["NEIGHBOUR_MAX"]);
    commands.insert_resource(BlurPipelines { velocity_layouts, single, gather_layout, sampler, base, velocity, tile, neighbour, gather: HashMap::default() });
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<BlurPipelines>,
    mut textures: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    views: Query<(Entity, &ExtractedView, &ObjectBlur, Option<&bevy::camera::MainPassResolutionOverride>)>,
) {
    for (entity, view, blur, render_size) in &views {
        let format = view.target_format;
        let gather = match pipelines.gather.get(&format) {
            Some(id) => *id,
            None => {
                let mut d = pipelines.base.clone();
                d.layout = vec![pipelines.gather_layout.clone()];
                if let Some(f) = d.fragment.as_mut() {
                    f.entry_point = Some("gather".into());
                    f.targets = vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })];
                    f.shader_defs.push("GATHER".into());
                }
                let id = cache.queue_render_pipeline(d);
                pipelines.gather.insert(format, id);
                id
            }
        };
        let size = view.viewport.zw().max(UVec2::ONE);
        let tiles = (size + UVec2::splat(TILE - 1)) / TILE;
        let mut texture = |label: &'static str, s: UVec2| {
            textures.get(
                &device,
                TextureDescriptor {
                    label: Some(label),
                    size: Extent3d { width: s.x, height: s.y, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: VELOCITY_FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let velocity = texture("object_blur_velocity", size);
        let rows = texture("object_blur_rows", UVec2::new(tiles.x, size.y));
        let tile_tex = texture("object_blur_tiles", tiles);
        let neighbours = texture("object_blur_neighbours", tiles);
        let region = render_size.map_or(size, |r| r.0).max(UVec2::ONE);
        let params = blur.params;
        let mut uniform = UniformBuffer::from(BlurUniform {
            reproject: blur.reproject,
            params: Vec4::new(params.x, params.y, params.z * size.y as f32, params.w),
            sizes: Vec4::new(region.x as f32, region.y as f32, size.x as f32, size.y as f32),
        });
        uniform.write_buffer(&device, &queue);
        commands.entity(entity).insert(BlurView { gather, format, velocity, rows, tiles: tile_tex, neighbours, uniform, on: params.x > 0.0 });
    }
}

fn draw(ctx: &mut RenderContext, label: &'static str, target: &TextureView, pipeline: &RenderPipeline, bind_group: &BindGroup) {
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// Velocity, tile maxima, neighbour maxima, then the gather into the picture.
pub(crate) fn blur_pass(
    view: ViewQuery<(&ViewTarget, &BlurView, &ViewPrepassTextures)>,
    pipelines: Res<BlurPipelines>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, bv, prepass) = view.into_inner();
    if !bv.on || !crate::ssao::format_matches(target, bv.format) {
        return;
    }
    let (Some(depth), Some(motion), Some(depth_multi)) = (prepass.depth_only_view(), prepass.motion_vectors_view(), crate::ssao::depth_multisampled(prepass)) else {
        return;
    };
    let motion_multi = prepass.motion_vectors.as_ref().is_some_and(|m| m.texture.texture.sample_count() > 1);
    if motion_multi != depth_multi {
        return;
    }
    let (Some(velocity), Some(across), Some(down), Some(neighbour), Some(gather)) = (
        cache.get_render_pipeline(pipelines.velocity[depth_multi as usize]),
        cache.get_render_pipeline(pipelines.tile[0]),
        cache.get_render_pipeline(pipelines.tile[1]),
        cache.get_render_pipeline(pipelines.neighbour),
        cache.get_render_pipeline(bv.gather),
    ) else {
        return;
    };
    let Some(uniform) = bv.uniform.binding() else { return };
    // GPU time of the pass (recorded with --log-frame-times, graphics.rs).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_object_blur");
    let layout = cache.get_bind_group_layout(&pipelines.velocity_layouts[depth_multi as usize]);
    let bg = device.create_bind_group("object_blur_velocity", &layout, &BindGroupEntries::sequential((uniform.clone(), depth, motion)));
    draw(&mut ctx, "object_blur_velocity_pass", &bv.velocity.default_view, velocity, &bg);
    let single = cache.get_bind_group_layout(&pipelines.single);
    let bg = device.create_bind_group("object_blur_rows", &single, &BindGroupEntries::sequential((uniform.clone(), &bv.velocity.default_view)));
    draw(&mut ctx, "object_blur_rows_pass", &bv.rows.default_view, across, &bg);
    let bg = device.create_bind_group("object_blur_tiles", &single, &BindGroupEntries::sequential((uniform.clone(), &bv.rows.default_view)));
    draw(&mut ctx, "object_blur_tiles_pass", &bv.tiles.default_view, down, &bg);
    let bg = device.create_bind_group("object_blur_neighbours", &single, &BindGroupEntries::sequential((uniform.clone(), &bv.tiles.default_view)));
    draw(&mut ctx, "object_blur_neighbours_pass", &bv.neighbours.default_view, neighbour, &bg);
    let post = target.post_process_write();
    let layout = cache.get_bind_group_layout(&pipelines.gather_layout);
    let bg = device.create_bind_group(
        "object_blur_gather",
        &layout,
        &BindGroupEntries::sequential((uniform, post.source, &pipelines.sampler, &bv.velocity.default_view, &bv.neighbours.default_view)),
    );
    draw(&mut ctx, "object_blur_gather_pass", post.destination, gather, &bg);
    span.end(ctx.command_encoder());
}
