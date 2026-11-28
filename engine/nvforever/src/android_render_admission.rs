//! Bounded numeric cache-admission evidence, never queue/submission/pixel proof.
use bevy::{asset::{AssetId, UntypedAssetId}, ecs::system::SystemParam, prelude::*};
use bevy::camera::visibility::ViewVisibility;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::{PreparedMaterial, RenderMaterialInstances, SpecializedMaterialPipelineCache, SpecializedPrepassMaterialPipelineCache};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::material_bind_groups::MaterialBindGroupAllocators;
use bevy::render::mesh::{allocator::MeshAllocator, RenderMesh};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::PipelineCache;
use bevy::render::sync_world::MainEntity;
use bevy::render::texture::GpuImage;
use bevy::window::AppLifecycle;
use nvf_assets::NifMaterial;
use nvf_play::{loading::Loading, PlayState};
use std::collections::{BTreeMap, BTreeSet};

const SKIN_LIMIT: usize = 128;
const RIGID_LIMIT: usize = 32;
const OFFSETS: [u16; 8] = [0, 1, 2, 4, 8, 16, 32, 64];
const BURSTS: u8 = 8;
const IDLE_DETECTION_FRAMES: u8 = 8;

#[derive(Resource, Clone, Copy)]
struct Gate(bool);
fn diagnostics_metadata(build_type: Option<&str>, requested: Option<&str>) -> bool {
    build_type == Some("debug") && requested == Some("enabled")
}
fn enabled() -> bool {
    #[cfg(target_os = "android")]
    {
        static DIAGNOSTICS_APK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *DIAGNOSTICS_APK.get_or_init(|| diagnostics_metadata(
            crate::android::activity_string("renderDiagnosticsBuildType", None).ok().as_deref(),
            crate::android::activity_string("renderAdmissionLog", None).ok().as_deref(),
        ))
    }
    #[cfg(not(target_os = "android"))]
    { diagnostics_metadata(None, None) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trigger { Load = 1, Resume = 2, NewSkin = 3 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ticket { burst: u8, trigger: Trigger, sample: u8 }
#[derive(Resource, Default)]
struct Sampler {
    bursts: u8,
    active: Option<Trigger>,
    age: u16,
    next: u8,
    running: bool,
    initialized: bool,
    ready: bool,
    resumed: bool,
    idle_frames: u8,
    visible: BTreeSet<Entity>,
    #[cfg(test)]
    captures: u16,
    #[cfg(test)]
    id_scans: u16,
}
impl Sampler {
    fn tick(&mut self, trigger: Option<Trigger>, ready: bool) -> Option<Ticket> {
        if !ready { self.active = None; self.idle_frames = 0; return None; }
        if self.active.is_none() && self.bursts < BURSTS {
            if let Some(trigger) = trigger {
                self.bursts += 1;
                self.active = Some(trigger);
                self.age = 0;
                self.next = 0;
            }
        }
        let active = self.active?;
        let ticket = (self.age == OFFSETS[usize::from(self.next)]).then_some(Ticket {
            burst: self.bursts, trigger: active, sample: self.next,
        });
        self.age += 1;
        if ticket.is_some() {
            self.next += 1;
            if usize::from(self.next) == OFFSETS.len() { self.active = None; }
        }
        ticket
    }
}
#[derive(Clone, Copy)]
struct Shape {
    entity: MainEntity,
    mesh: AssetId<Mesh>,
    material: UntypedAssetId,
    cpu_material: bool,
    diffuse: Option<AssetId<Image>>,
}
#[derive(Resource, Clone, Default)]
struct Snapshot {
    ticket: Option<Ticket>,
    camera: Option<MainEntity>,
    skin_total: u32,
    rigid_total: u32,
    skins: Vec<Shape>,
    rigid: Vec<Shape>,
}
pub(crate) struct AndroidRenderAdmissionPlugin;
impl Plugin for AndroidRenderAdmissionPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Gate(enabled())).init_resource::<Sampler>().init_resource::<Snapshot>()
            .add_message::<AppLifecycle>().add_systems(Last, watch);
    }
    fn finish(&self, app: &mut App) {
        // Local logging must be requested on a debuggable APK, irrespective of Rust optimization.
        if !app.world().resource::<Gate>().0 { return; }
        // Installed before DefaultPlugins; the render app exists only after plugin builds.
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.init_resource::<Snapshot>().init_resource::<Observed>()
                .add_systems(ExtractSchedule, extract)
                .add_systems(Render, observe.in_set(RenderSystems::Cleanup));
        }
    }
}
type ShapeData = (Entity, &'static Mesh3d, &'static MeshMaterial3d<NifMaterial>, &'static ViewVisibility);
#[derive(SystemParam)]
struct Watch<'w, 's> {
    gate: Res<'w, Gate>,
    sampler: ResMut<'w, Sampler>,
    snapshot: ResMut<'w, Snapshot>,
    lifecycle: MessageReader<'w, 's, AppLifecycle>,
    play: Option<Res<'w, State<PlayState>>>,
    loading: Option<Res<'w, Loading>>,
    streamer: Option<Res<'w, crate::world::Streamer>>,
    menu: Option<Res<'w, nvf_play::main_menu::MainMenu>>,
    materials: Option<Res<'w, Assets<NifMaterial>>>,
    skins: Query<'w, 's, ShapeData, With<SkinnedMesh>>,
    rigid: Query<'w, 's, ShapeData, Without<SkinnedMesh>>,
    camera: Query<'w, 's, Entity, With<nvf_play::PlayerCamera>>,
}
fn bounded_insert<T>(map: &mut BTreeMap<Entity, T>, entity: Entity, value: T, limit: usize) {
    if map.len() < limit || map.last_key_value().is_some_and(|(&last, _)| entity < last) {
        map.insert(entity, value);
        if map.len() > limit { map.pop_last(); }
    }
}
fn shape(entity: Entity, mesh: &Mesh3d, material: &MeshMaterial3d<NifMaterial>, materials: Option<&Assets<NifMaterial>>) -> Shape {
    let cpu = materials.and_then(|m| m.get(&material.0));
    Shape { entity: entity.into(), mesh: mesh.0.id(), material: material.0.id().untyped(),
        cpu_material: cpu.is_some(), diffuse: cpu.and_then(|m| m.base.base_color_texture.as_ref().map(Handle::id)) }
}
fn watch(mut p: Watch) {
    *p.snapshot = Snapshot::default();
    if !p.gate.0 { p.lifecycle.clear(); return; }
    if !p.sampler.initialized { p.sampler.running = true; p.sampler.initialized = true; }
    for event in p.lifecycle.read() {
        match event {
            AppLifecycle::Suspended => { p.sampler.running = false; p.sampler.active = None; p.sampler.visible.clear(); }
            AppLifecycle::Running => { p.sampler.running = true; p.sampler.resumed = true; }
            _ => {}
        }
    }
    if p.sampler.bursts >= BURSTS && p.sampler.active.is_none() { return; }
    let ready = p.sampler.running
        && p.play.as_ref().is_some_and(|s| matches!(s.get(), PlayState::Playing | PlayState::Menu | PlayState::Dialogue))
        && p.streamer.as_ref().is_some_and(|s| s.settled)
        && p.loading.as_ref().is_none_or(|l| !l.active() && !l.unseen)
        && p.menu.as_ref().is_none_or(|m| !m.open);
    if !ready {
        p.sampler.ready = false;
        p.sampler.visible.clear();
        p.sampler.tick(None, false);
        return;
    }
    let mut trigger = if p.sampler.resumed && p.sampler.bursts > 0 { Some(Trigger::Resume) }
        else if !p.sampler.ready { Some(Trigger::Load) }
        else { None };
    p.sampler.ready = true;
    p.sampler.resumed = false;
    if p.sampler.active.is_none() && trigger.is_none() {
        p.sampler.idle_frames += 1;
        if p.sampler.idle_frames < IDLE_DETECTION_FRAMES { return; }
        p.sampler.idle_frames = 0;
        #[cfg(test)]
        { p.sampler.id_scans += 1; }
        // Idle detection retains IDs only, and only the lowest 128 visible IDs.
        // Later entities outside that selection cannot trigger this observer.
        let mut ids = BTreeSet::new();
        for (entity, _, _, visible) in &p.skins {
            if !visible.get() { continue; }
            if ids.len() < SKIN_LIMIT || ids.last().is_some_and(|last| entity < *last) {
                ids.insert(entity);
                if ids.len() > SKIN_LIMIT { ids.pop_last(); }
            }
        }
        if ids.iter().any(|e| !p.sampler.visible.contains(e)) { trigger = Some(Trigger::NewSkin); }
        p.sampler.visible = ids;
    } else {
        p.sampler.idle_frames = 0;
    }
    let Some(ticket) = p.sampler.tick(trigger, true) else { return; };
    #[cfg(test)]
    { p.sampler.captures += 1; }
    // Active non-sample frames never scan meshes or look up CPU materials.
    let mut skins = BTreeMap::new();
    let mut skin_total = 0_u32;
    for (entity, mesh, material, visible) in &p.skins {
        if !visible.get() { continue; }
        skin_total = skin_total.saturating_add(1);
        bounded_insert(&mut skins, entity, (mesh, material), SKIN_LIMIT);
    }
    p.sampler.visible = skins.keys().copied().collect();
    let mut rigid = BTreeMap::new();
    let mut rigid_total = 0_u32;
    for (entity, mesh, material, visible) in &p.rigid {
        if !visible.get() { continue; }
        rigid_total = rigid_total.saturating_add(1);
        bounded_insert(&mut rigid, entity, (mesh, material), RIGID_LIMIT);
    }
    *p.snapshot = Snapshot { ticket: Some(ticket), camera: p.camera.single().ok().map(MainEntity::from),
        skin_total, rigid_total,
        skins: skins.into_iter().map(|(e, (mesh, material))| shape(e, mesh, material, p.materials.as_deref())).collect(),
        rigid: rigid.into_iter().map(|(e, (mesh, material))| shape(e, mesh, material, p.materials.as_deref())).collect() };
}
fn extract(main: Extract<Res<Snapshot>>, mut render: ResMut<Snapshot>) { render.clone_from(&**main); }

#[derive(Default, Clone, Copy)]
struct Flags {
    mesh: bool, slab: bool, instance: bool, diffuse: bool, prepared: bool, bound: bool,
    main: bool, main_ready: bool, prepass: bool, prepass_ready: bool,
}
#[derive(Default, Debug, PartialEq, Eq)]
struct Counts {
    sampled: u32, cpu_material: u32, diffuse_refs: u32, mesh: u32, slab: u32, instance: u32,
    diffuse: u32, prepared: u32, bound: u32, main: u32, main_ready: u32, prepass: u32, prepass_ready: u32,
}
fn count(shapes: &[Shape], mut lookup: impl FnMut(&Shape) -> Flags) -> Counts {
    let mut c = Counts::default();
    for shape in shapes {
        let f = lookup(shape);
        c.sampled += 1;
        c.cpu_material += u32::from(shape.cpu_material);
        c.diffuse_refs += u32::from(shape.diffuse.is_some());
        c.mesh += u32::from(f.mesh); c.slab += u32::from(f.slab); c.instance += u32::from(f.instance);
        c.diffuse += u32::from(f.diffuse); c.prepared += u32::from(f.prepared); c.bound += u32::from(f.bound);
        c.main += u32::from(f.main); c.main_ready += u32::from(f.main_ready);
        c.prepass += u32::from(f.prepass); c.prepass_ready += u32::from(f.prepass_ready);
    }
    c
}
fn fields(prefix: &str, total: u32, c: &Counts) -> String {
    format!("{prefix}_total={total} {prefix}_sampled={} {prefix}_cpu_mat={} {prefix}_diffuse_ref={} {prefix}_gpu_mesh={} {prefix}_slab={} {prefix}_instance={} {prefix}_gpu_diffuse={} {prefix}_prepared={} {prefix}_bound={} {prefix}_main={} {prefix}_main_ready={} {prefix}_prepass={} {prefix}_prepass_ready={}",
        c.sampled, c.cpu_material, c.diffuse_refs, c.mesh, c.slab, c.instance, c.diffuse, c.prepared, c.bound, c.main, c.main_ready, c.prepass, c.prepass_ready)
}
fn line(snapshot: &Snapshot, skin: &Counts, rigid: &Counts) -> String {
    let t = snapshot.ticket.expect("admitted sample");
    format!("render: gpu_admission_v1 burst={} trigger={} sample={} camera={} {} {}", t.burst, t.trigger as u8, t.sample,
        u8::from(snapshot.camera.is_some()), fields("skin", snapshot.skin_total, skin), fields("rigid", snapshot.rigid_total, rigid))
}
#[derive(Resource, Default)]
struct Observed(Option<Ticket>);
impl Observed {
    fn admit(&mut self, ticket: Option<Ticket>) -> bool {
        if ticket.is_none() || self.0 == ticket { return false; }
        self.0 = ticket;
        true
    }
}
#[derive(SystemParam)]
struct Observe<'w> {
    snapshot: Res<'w, Snapshot>,
    last: ResMut<'w, Observed>,
    instances: Res<'w, RenderMaterialInstances>,
    materials: Res<'w, ErasedRenderAssets<PreparedMaterial>>,
    allocators: Res<'w, MaterialBindGroupAllocators>,
    main: Res<'w, SpecializedMaterialPipelineCache>,
    prepass: Res<'w, SpecializedPrepassMaterialPipelineCache>,
    pipelines: Res<'w, PipelineCache>,
    meshes: Res<'w, RenderAssets<RenderMesh>>,
    allocator: Res<'w, MeshAllocator>,
    images: Res<'w, RenderAssets<GpuImage>>,
}
impl Observe<'_> {
    fn flags(&self, shape: &Shape) -> Flags {
        let prepared = self.materials.get(shape.material);
        let bound = prepared.is_some_and(|m| self.allocators.get(&shape.material.type_id())
            .and_then(|a| a.get(m.binding.group)).is_some_and(|g| g.bind_group().is_some()));
        let (main, main_ready) = self.main.iter().filter(|(view, _)| Some(view.main_entity) == self.snapshot.camera)
            .filter_map(|(_, entries)| entries.get(&shape.entity))
            .fold((false, true), |(_, ready), id| (true, ready && self.pipelines.get_render_pipeline(*id).is_some()));
        let (prepass, prepass_ready) = self.prepass.iter().filter(|(view, _)| Some(view.main_entity) == self.snapshot.camera)
            .filter_map(|(_, entries)| entries.get(&shape.entity))
            .fold((false, true), |(_, ready), (_, id, _)| (true, ready && self.pipelines.get_render_pipeline(*id).is_some()));
        Flags { mesh: self.meshes.get(shape.mesh).is_some(), slab: self.allocator.mesh_slabs(&shape.mesh).is_some(),
            instance: self.instances.instances.get(&shape.entity).is_some_and(|i| i.asset_id == shape.material),
            diffuse: shape.diffuse.is_some_and(|id| self.images.get(id).is_some()), prepared: prepared.is_some(), bound,
            main, main_ready: main && main_ready, prepass, prepass_ready: prepass && prepass_ready }
    }
}
fn observe(mut p: Observe) {
    let ticket = p.snapshot.ticket;
    if !p.last.admit(ticket) { return; }
    let skin = count(&p.snapshot.skins, |s| p.flags(s));
    let rigid = count(&p.snapshot.rigid, |s| p.flags(s));
    let message = line(&p.snapshot, &skin, &rigid);
    info!("{message}");
}

#[cfg(test)]
#[path = "android_render_admission/tests.rs"]
mod tests;
