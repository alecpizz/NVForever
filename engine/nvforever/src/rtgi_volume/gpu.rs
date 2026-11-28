//! The render world's side: what plan.rs decided, carried out. The component is inserted, changed
//! and removed in the extraction alone, as bevy_pbr requires (mesh_view_bindings.rs).

use super::admit::{self, Asked};
use super::plan::{older, plan, Admission, Key, Latch, Now, Pipelines, Plan, Why};
use super::{hook_state, place, Did, Place, RtgiCounts, RtgiRequest, RtgiVolumeUniform};
use bevy::app::SubApp;
use bevy::ecs::system::SystemParam;
use bevy::pbr::{MeshPipelineViewLayouts, NvfRtgiVolumeBindings};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AddressMode, CachedPipelineState, Extent3d, FilterMode, PipelineCache, PipelineDescriptor, Sampler, SamplerDescriptor, TextureDataOrder, TextureDescriptor, TextureDimension, TextureView, TextureViewDescriptor, UniformBuffer,
};
use bevy::render::renderer::{RenderAdapter, RenderDevice, RenderQueue};
use bevy::render::sync_world::RenderEntity;
use bevy::render::{Extract, ExtractSchedule, GpuResourceAppExt, Render, RenderSystems};
use nvf_assets::nif_material::RTGI_VOLUME_DEF;

pub(super) fn add(render_app: &mut SubApp) {
    render_app
        .init_gpu_resource::<RtgiGpu>()
        .add_systems(ExtractSchedule, extract.after(crate::sun_visibility::SunVisibilityExtracted))
        .add_systems(Render, observe.in_set(RenderSystems::Cleanup));
}

/// What is bound.
pub(super) struct Held {
    pub(super) key: Key,
    atlas: [u32; 3],
    buffer: UniformBuffer<RtgiVolumeUniform>,
    written: RtgiVolumeUniform,
    texture: TextureView,
    sampler: Sampler,
}

#[derive(Resource, Default)]
pub(super) struct RtgiGpu {
    pub(super) held: Option<Held>,
    latch: Latch,
    pub(super) asked: Option<Asked>,
    /// The visit that has the stock light to its end; the log has why.
    pub(super) ended: Option<(Place, Why)>,
    /// The visit whose pipelines the log has.
    told_ready: Option<Place>,
    /// The cache size when the pipelines for that visit were last all ready.
    ready_pipeline_count: Option<usize>,
}

/// The main world's, at the extraction.
#[derive(SystemParam)]
struct Main<'w, 's> {
    request: Option<Res<'w, RtgiRequest>>,
    streamer: Option<Res<'w, crate::world::Streamer>>,
    script: Option<Res<'w, nvf_play::script::ScriptState>>,
    loading: Option<Res<'w, nvf_play::loading::Loading>>,
    cams: Query<'w, 's, (RenderEntity, &'static Camera), crate::fly::WorldCamera>,
}

/// Whether a camera has a view in this frame, as bevy_render's `extract_cameras` decides it; the
/// render entity stays without one.
fn drawn(camera: &Camera) -> bool {
    camera.is_active && camera.physical_viewport_rect().is_some() && camera.physical_target_size().is_some_and(|s| s.x > 0 && s.y > 0)
}

/// The device, and what the question of admission needs beside it.
#[derive(SystemParam)]
struct Device<'w> {
    device: Option<Res<'w, RenderDevice>>,
    queue: Option<Res<'w, RenderQueue>>,
    adapter: Option<Res<'w, RenderAdapter>>,
    layouts: Option<Res<'w, MeshPipelineViewLayouts>>,
    sun: Option<Res<'w, crate::sun_visibility::SunVisibilityGpu>>,
}

impl Device<'_> {
    /// Whether the view may have a texture of these sides, asked again where the answer may be
    /// another one: the sides, or the light volume's arrays, which this frame's extraction of the
    /// sun visibility has decided.
    fn admission(&self, asked: &mut Option<Asked>, atlas: [u32; 3]) -> Admission {
        let (Some(device), Some(adapter), Some(layouts), true) = (&self.device, &self.adapter, &self.layouts, self.queue.is_some()) else { return Admission::Deferred };
        let clouds = self.sun.as_ref().is_some_and(|s| s.light_volume_bound());
        if asked.as_ref().is_none_or(|a| !a.is_of(clouds, atlas)) {
            *asked = Some(Asked::of(device, adapter, layouts, clouds, atlas));
        }
        if asked.as_ref().is_some_and(|a| a.granted.is_ok()) {
            Admission::Granted
        } else {
            Admission::Refused
        }
    }
}

/// The device is needed for [`Plan::Bind`] alone, which is never planned without (it is admitted first).
fn extract(mut commands: Commands, main: Extract<Main>, mut bound: Query<(Entity, &mut NvfRtgiVolumeBindings)>, mut state: ResMut<RtgiGpu>, gpu: Device, counts: Res<RtgiCounts>) {
    let state = &mut *state;
    let loading = main.loading.as_deref();
    let open = state.latch.step(loading.is_some_and(|l| l.starting() || l.travelling()), loading.is_some_and(|l| l.settling()));
    let request = main.request.as_deref();
    if request.is_none() && state.held.is_none() && bound.is_empty() {
        return;
    }
    let here = place(main.streamer.as_deref(), main.script.as_deref());
    let cams: Vec<Entity> = main.cams.iter().map(|(e, _)| e).collect();
    let views = main.cams.iter().any(|(_, camera)| drawn(camera));
    let admission = match (request.filter(|r| here == Some(r.place)), &state.held) {
        (None, _) => Admission::Deferred,
        // What would stay bound is what is asked about.
        (Some(r), Some(h)) if older(r.key(), h.key) => gpu.admission(&mut state.asked, h.atlas),
        (Some(r), _) => gpu.admission(&mut state.asked, r.atlas().0),
    };
    let now = Now { here, hook: hook_state(), admission, open, views, ended: state.ended };
    let plan = plan(request.map(RtgiRequest::key), &now, state.held.as_ref().map(|h| h.key));
    tell(state, request, plan);
    match (plan, request) {
        (Plan::Bind { new }, Some(request)) => {
            let (device, queue) = gpu.device.as_deref().zip(gpu.queue.as_deref()).expect("a device that granted it");
            let uniform = request.uniform();
            if new {
                // The GPU lets go of what was held when it is done with it; the buffer and the
                // sampler stay.
                let atlas = request.atlas();
                let texture = make_texture(device, queue, atlas);
                counts.add(Did::Allocation, 1);
                counts.add(Did::Upload, 1);
                match &mut state.held {
                    Some(h) => (h.key, h.atlas, h.texture) = (request.key(), atlas.0, texture),
                    held => {
                        let mut buffer = UniformBuffer::from(uniform);
                        buffer.set_label(Some("nvf_rtgi_volume"));
                        buffer.write_buffer(device, queue);
                        counts.add(Did::Allocation, 2);
                        counts.add(Did::Upload, 1);
                        *held = Some(Held { key: request.key(), atlas: atlas.0, buffer, written: uniform, texture, sampler: make_sampler(device) });
                    }
                }
            }
            if let Some(h) = state.held.as_mut().filter(|h| h.written != uniform) {
                h.buffer.set(uniform);
                h.buffer.write_buffer(device, queue);
                h.written = uniform;
                counts.add(Did::Upload, 1);
            }
        }
        (Plan::Keep, _) => {}
        _ => state.held = None,
    }
    let shown = state.held.as_ref().and_then(|h| Some((h, h.buffer.buffer()?)));
    for (e, mut b) in bound.iter_mut() {
        match shown.filter(|_| cams.contains(&e)) {
            Some((h, buffer)) => {
                if b.texture.id() != h.texture.id() || b.uniform.id() != buffer.id() {
                    *b = NvfRtgiVolumeBindings { uniform: buffer.clone(), texture: h.texture.clone(), sampler: h.sampler.clone() };
                    counts.add(Did::Bind, 1);
                }
            }
            None => {
                commands.entity(e).try_remove::<NvfRtgiVolumeBindings>();
            }
        }
    }
    let Some((h, buffer)) = shown else { return };
    for e in cams.iter().filter(|e| !bound.contains(**e)) {
        commands.entity(*e).try_insert(NvfRtgiVolumeBindings { uniform: buffer.clone(), texture: h.texture.clone(), sampler: h.sampler.clone() });
        counts.add(Did::Bind, 1);
    }
}

/// The log's line of a change, and the end of a visit's light.
fn tell(state: &mut RtgiGpu, request: Option<&RtgiRequest>, plan: Plan) {
    match (plan, request) {
        (Plan::Bind { new: true }, Some(r)) => {
            let what = if r.solved.is_some() { "a solved texture" } else { "the empty texture" };
            info!("rtgi: cell {:08X}, revision {}: {what} of {:?} bound", r.place.cell, r.revision, r.atlas().0);
        }
        (Plan::Stock(why), _) => {
            if state.held.is_some() {
                info!("rtgi: unbound ({why:?}), the stock light");
            }
            let Some(r) = request.filter(|r| why.ends_visit() && state.ended.is_none_or(|e| e.0 != r.place)) else { return };
            state.ended = Some((r.place, why));
            let refusal = state.asked.as_ref().and_then(|a| a.granted.as_ref().err());
            match (why, refusal) {
                (Why::Hook, _) => error!("rtgi: the hook is not in the lighting; the stock light"),
                (Why::Refused, Some(refusal)) => warn!("rtgi: {refusal}; the stock light"),
                _ => info!("rtgi: cell {:08X}: asked for with no screen up that waits for its pipelines; the stock light", r.place.cell),
            }
        }
        _ => {}
    }
}

fn make_texture(device: &RenderDevice, queue: &RenderQueue, (sides, bytes): ([u32; 3], &[u8])) -> TextureView {
    let [width, height, depth_or_array_layers] = sides;
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("nvf_rtgi_volume"),
            size: Extent3d { width, height, depth_or_array_layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D3,
            format: admit::FORMAT,
            usage: admit::USAGE,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        bytes,
    );
    texture.create_view(&TextureViewDescriptor::default())
}

fn make_sampler(device: &RenderDevice) -> Sampler {
    device.create_sampler(&SamplerDescriptor {
        label: Some("nvf_rtgi_volume"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    })
}

/// Whether a pipeline's fragment reads the volume, by its def.
fn reads(descriptor: &PipelineDescriptor) -> bool {
    let PipelineDescriptor::RenderPipelineDescriptor(d) = descriptor else { return false };
    d.fragment.as_ref().is_some_and(|f| f.shader_defs.contains(&RTGI_VOLUME_DEF.into()))
}

/// The pipelines that read, once a frame while a texture is bound: one that failed for good ends the
/// visit's light (the next extraction takes the component away, before anything is specialised). It
/// is told by the def alone, so one whose stock twin is broken too counts as well [design].
fn observe(cache: Res<PipelineCache>, mut state: ResMut<RtgiGpu>) {
    let Some(place) = state.held.as_ref().map(|h| h.key.place).filter(|p| state.ended.is_none_or(|e| e.0 != *p)) else { return };
    let pipeline_count = cache.pipelines().count();
    // Shader reloads can requeue the existing pipelines without growing the cache.
    if state.told_ready == Some(place) && state.ready_pipeline_count == Some(pipeline_count)
        && cache.waiting_pipelines().next().is_none() {
        return;
    }
    let mut n = Pipelines::default();
    for p in cache.pipelines().filter(|p| reads(&p.descriptor)) {
        n.count(&p.state);
        if let CachedPipelineState::Err(e) = &p.state {
            debug!("rtgi: a pipeline that reads the volume: {e}");
        }
    }
    let ready = n.there > 0 && n.waiting == 0 && n.failed == 0;
    state.ready_pipeline_count = ready.then_some(pipeline_count);
    if n.failed > 0 {
        state.ended = Some((place, Why::Failed));
        error!("rtgi: {} pipelines that read the volume failed ({} are there); the stock light in this visit", n.failed, n.there);
    } else if ready && state.told_ready != Some(place) {
        state.told_ready = Some(place);
        info!("rtgi: pipelines that read the volume: {} there, 0 to come, 0 failed", n.there);
    }
}
