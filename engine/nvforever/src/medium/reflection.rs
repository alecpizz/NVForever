//! Water's planar mirror traces the same cloud field as the main camera, from the
//! reflected eye. Its small image is traced in full, before the mirror draws the dome.
//! There is no temporal history or light-volume producer for this camera.

use super::*;
use crate::reflection::{ReflectedClouds, ReflectionCamera};
use crate::volumetric_clouds::CloudVolumeMaterial;
use crate::volumetric_clouds_lightvol::gpu::{self as lightvol, LightVolumeReader};

pub(super) fn install(render: &mut SubApp) {
    render.init_resource::<MirrorFrame>()
        .init_resource::<MirrorGpu>()
        .add_systems(ExtractSchedule, extract)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups))
        .add_systems(Core3d, draw.after(Core3dSystems::Prepass).before(Core3dSystems::MainPass))
        .add_systems(Render, report.in_set(RenderSystems::Cleanup));
}

struct MirrorTarget {
    view: Entity,
    world_from_view: Mat4,
    clip_from_view: Mat4,
    image: AssetId<Image>,
    proxy: Entity,
    material: UntypedAssetId,
}

type Ready = Arc<Mutex<Option<AssetId<Image>>>>;
type MirrorGroupKey = (TraceGroupKey, Option<[TextureViewId; 2]>);

#[derive(Resource, Default)]
struct MirrorFrame {
    target: Option<MirrorTarget>,
    ready: Option<Ready>,
    written: Option<AssetId<Image>>,
}

#[derive(Component)]
struct MirrorPass {
    image: AssetId<Image>,
    far: TextureView,
    distance: CachedTexture,
    pipeline: RenderPipeline,
    group: BindGroup,
}

#[derive(Default)]
struct MirrorBuffers {
    trace: UniformBuffer<MediumTraceUniform>,
    clouds: UniformBuffer<CloudVolumeUniform>,
    group: Option<(MirrorGroupKey, BindGroup)>,
}

#[derive(Resource, Default)]
struct MirrorGpu {
    buffers: Option<MirrorBuffers>,
    // The regular reference pipeline is shared with the main view. This variant
    // adds the read-only fronts published for this frame to that same layout.
    volume: Option<(BindGroupLayoutDescriptor, CachedRenderPipelineId)>,
}

fn mirror_trace_matrices(pose: &GlobalTransform, projection: &PerspectiveProjection) -> (Mat4, Mat4) {
    // The water plane clips reflected geometry. Its intersection with a pixel's
    // ray can lie behind the virtual eye. Preserve the eye's negative scale and
    // unproject the ray through the ordinary lens.
    let lens = Projection::Perspective(PerspectiveProjection {
        near_clip_plane: PerspectiveProjection::default().near_clip_plane,
        ..projection.clone()
    });
    (pose.to_matrix(), lens.get_clip_from_view())
}

#[allow(clippy::type_complexity)]
fn extract(
    mut commands: Commands,
    clouds: Extract<Option<Res<ReflectedClouds>>>,
    cameras: Extract<Query<(RenderEntity, &Camera, &GlobalTransform, &Projection), With<ReflectionCamera>>>,
    materials: Extract<Query<&MeshMaterial3d<CloudVolumeMaterial>>>,
    stale: Query<Entity, With<MirrorPass>>,
    mut frame: ResMut<MirrorFrame>,
) {
    for e in &stale {
        commands.entity(e).try_remove::<MirrorPass>();
    }
    frame.written = None;
    frame.target = None;
    // Teardown retires both the image and its report. Idle frames have no
    // current target to acknowledge or invalidate.
    frame.ready = clouds.as_ref().filter(|clouds| clouds.image.is_some()).map(|clouds| clouds.ready.clone());
    let Some(clouds) = clouds.as_ref() else { return };
    let (Some(image), Some(proxy)) = (&clouds.image, clouds.proxy) else { return };
    let Ok((view, camera, pose, Projection::Perspective(p))) = cameras.single() else { return };
    if !camera.is_active {
        return;
    }
    let Ok(material) = materials.get(proxy) else { return };
    let (world_from_view, clip_from_view) = mirror_trace_matrices(pose, p);
    frame.target = Some(MirrorTarget {
        view, world_from_view, clip_from_view,
        image: image.id(), proxy, material: material.id().untyped(),
    });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn prepare(
    mut commands: Commands,
    frame: Res<MirrorFrame>,
    main_frame: Res<MediumFrame>,
    main_gpu: Res<MediumGpu>,
    mut mirror: ResMut<MirrorGpu>,
    mut pipelines: ResMut<MediumPipelines>,
    (device, queue, cache, fullscreen, assets): (Res<RenderDevice>, Res<RenderQueue>, Res<PipelineCache>, Res<FullscreenShader>, Res<AssetServer>),
    (images, materials, specialized): (Res<RenderAssets<GpuImage>>, Option<Res<ErasedRenderAssets<PreparedMaterial>>>, Option<Res<SpecializedMaterialPipelineCache>>),
    reader: Option<Res<LightVolumeReader>>,
    mut textures: ResMut<TextureCache>,
    views: Query<&ExtractedView>,
) {
    let (Some(target), Some(source)) = (&frame.target, &main_frame.0) else {
        mirror.buffers = None;
        return;
    };
    let Ok(view) = views.get(target.view) else { return };
    let Some(image) = images.get(target.image).filter(|i| i.texture_descriptor.format == FAR_FORMAT) else { return };
    let Some([nb, ne]) = noise_views(source.noise, |id| images.get(id).map(|i| i.texture_view.clone())) else { return };
    let Some(ns) = main_gpu.noise_sampler.as_ref() else { return };
    let Some(pl) = pipelines.0.as_mut() else { return };
    let (label, entry) = trace_entry(MediumMode::Reference);
    let legacy = *pl.reference[0].get_or_insert_with(|| {
        cache.queue_render_pipeline(trace_pipeline(&pl.trace_layout, assets.load("embedded://nvforever/medium_trace.wesl"), label, entry, &fullscreen, false))
    });
    let mut u = source.clouds;
    u.flags.z = 0.0; // Reflected water always uses the lit cloud material.
    u.dshape.w = 1.0; // One reflected ray per texel; no tile reconstruction.
    let mut fronts = None;
    // Use completed sets guaranteed not to be overwritten during either camera.
    // Both eyes have the same xz. Their bent heights use their own world y.
    if let Some(reader) = reader.as_deref().filter(|r| r.ready().contains(&true) && r.cam.xz() == target.world_from_view.w_axis.xz()) {
        let (layout, id) = mirror.volume.get_or_insert_with(|| {
            let layout = lightvol::trace_layout();
            let mut desc = trace_pipeline(&layout, assets.load("embedded://nvforever/medium_trace.wesl"), "mirror_cloud_light_volume", entry, &fullscreen, false);
            desc.fragment.as_mut().unwrap().shader_defs.push(lightvol::LV_DEF.into());
            let id = cache.queue_render_pipeline(desc);
            (layout, id)
        });
        if let Some(pipeline) = cache.get_render_pipeline(*id) {
            let [near, far] = reader.levels.each_ref().map(Option::as_ref);
            if let (Some(a), Some(b)) = (near.or(far), far.or(near)) {
                let window = reader.window([1.0, 1.0]);
                u.lv = Vec4::new(crate::volumetric_clouds_lightvol::lv_config().taps as f32, window.floor.z, window.floor.w, window.floor.y);
                u.lv_near = window.near;
                u.lv_far = window.far;
                u.lv_size = window.size;
                if let Some(main) = main_gpu.clouds_written {
                    u.lcell = main.lcell;
                }
                // A missing level has zero presence. Bind the other array there
                // to satisfy the layout without creating a stand-in texture.
                fronts = Some((layout.clone(), pipeline.clone(), a.view.clone(), b.view.clone()));
            }
        }
    }
    let (layout, pipeline, levels) = match fronts {
        Some((layout, pipeline, near, far)) => (layout, pipeline, Some([near, far])),
        None => {
            let Some(pipeline) = cache.get_render_pipeline(legacy) else { return };
            (pl.trace_layout.clone(), pipeline.clone(), None)
        }
    };
    let prepared = materials.as_ref().is_some_and(|m| m.get(target.material).is_some());
    let proxy_pipeline = specialized.as_ref().and_then(|s| s.get(&view.retained_view_entity)?.get(&MainEntity::from(target.proxy)).copied());
    if !prepared || proxy_pipeline.and_then(|id| cache.get_render_pipeline(id)).is_none() {
        return;
    }
    let extent = image.texture_descriptor.size;
    let size = UVec2::new(extent.width, extent.height);
    // A fixed dither phase avoids swimming while stationary. Angular material
    // filtering uses this image's actual texel width.
    let mut trace = prepared_trace_uniform(target.world_from_view, target.clip_from_view, size, 0, MediumMode::Reference, 1);
    if let Some(main) = &source.camera {
        trace.tiles.w = (0.5 * (main.translation.y - trace.origin.y)).max(0.0);
    }
    let buf = mirror.buffers.get_or_insert_with(MirrorBuffers::default);
    buf.trace.set(trace);
    buf.trace.write_buffer(&device, &queue);
    buf.clouds.set(u);
    buf.clouds.write_buffer(&device, &queue);
    let (Some(t), Some(c)) = (buf.trace.buffer(), buf.clouds.buffer()) else { return };
    let key = ((t.id(), c.id(), nb.id(), ns.id(), ne.id()), levels.as_ref().map(|l| [l[0].id(), l[1].id()]));
    if buf.group.as_ref().is_none_or(|(k, _)| *k != key) {
        let layout = cache.get_bind_group_layout(&layout);
        let (Some(t), Some(c)) = (buf.trace.binding(), buf.clouds.binding()) else { return };
        let group = match &levels {
            Some([a, b]) => device.create_bind_group("mirror_cloud_trace", &layout, &BindGroupEntries::sequential((t, c, &nb, ns, &ne, a, b))),
            None => device.create_bind_group("mirror_cloud_trace", &layout, &BindGroupEntries::sequential((t, c, &nb, ns, &ne))),
        };
        buf.group = Some((key, group));
    }
    let distance = textures.get(&device, TextureDescriptor {
        label: Some("mirror_cloud_distance"), size: extent, mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2,
        format: DISTANCE_FORMAT, usage: TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
    });
    commands.entity(target.view).insert(MirrorPass {
        image: target.image, far: image.texture_view.clone(), distance, pipeline, group: buf.group.as_ref().unwrap().1.clone(),
    });
}

fn draw(view: ViewQuery<&MirrorPass>, mut frame: ResMut<MirrorFrame>, mut ctx: RenderContext) {
    let p = view.into_inner();
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_mirror_clouds");
    fullscreen_pass(&mut ctx, "mirror_cloud_trace", &[(&p.far, CLEAR_LIGHT), (&p.distance.default_view, CLEAR_DISTANCE)], Some((&p.pipeline, &p.group)));
    span.end(ctx.command_encoder());
    frame.written = Some(p.image);
}

fn report(frame: Res<MirrorFrame>) {
    if let Some(ready) = &frame.ready {
        *ready.lock().unwrap_or_else(|e| e.into_inner()) = frame.written;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilted_mirror_rays_reach_clouds_outside_the_main_view() {
        let water_y = 30.0;
        let eye = Transform::from_xyz(100.0, 110.0, -220.0).with_rotation(
            Quat::from_rotation_y(0.6) * Quat::from_rotation_x(-20f32.to_radians()),
        );
        let reflect = Mat4::from_translation(Vec3::Y * (2.0 * water_y)) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
        let virtual_eye = GlobalTransform::from(Transform::from_matrix(reflect * eye.to_matrix()));
        let lens = PerspectiveProjection { fov: 70f32.to_radians(), aspect_ratio: 16.0 / 9.0, near: 5.0, ..default() };
        let oblique = PerspectiveProjection {
            near_clip_plane: (eye.rotation.inverse() * Vec3::NEG_Y).extend(water_y - 1.0 - eye.translation.y),
            ..lens.clone()
        };
        let (world_from_view, clip_from_view) = mirror_trace_matrices(&virtual_eye, &oblique);
        let trace = prepared_trace_uniform(world_from_view, clip_from_view, UVec2::new(160, 90), 0, MediumMode::Reference, 1);
        assert_eq!(trace.origin.truncate(), Vec3::new(100.0, -50.0, -220.0));
        let main_clip = Projection::Perspective(lens.clone()).get_clip_from_view() * eye.to_matrix().inverse();
        let half_height = (0.5 * lens.fov).tan();
        for ndc in [Vec2::ZERO, Vec2::new(0.65, -0.7), Vec2::new(-0.55, 0.9)] {
            let incident = eye.rotation * Vec3::new(ndc.x * lens.aspect_ratio * half_height, ndc.y * half_height, -1.0).normalize();
            let expected = Vec3::new(incident.x, -incident.y, incident.z);
            // medium_trace.wesl's medium_ray_dir, starting at the pixel's NDC.
            let point = trace.world_from_clip * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
            let actual = (point.truncate() / point.w).normalize();
            assert!(actual.dot(expected) > 0.99999, "{ndc}: {actual} against {expected}");
            if ndc.y <= 0.0 {
                let cloud = trace.origin.truncate() + 100_000.0 * actual;
                let seen = main_clip * cloud.extend(1.0);
                assert!(cloud.y > eye.translation.y && seen.w > 0.0 && seen.y > seen.w, "{ndc}: cloud above the main frame");
            }
        }
    }
}
