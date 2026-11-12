//! An in-engine CPU profile by span (docs/design/frame-rate-audit.md §1), for finding what
//! costs the frames without a sampling profiler (xperf needs elevation;
//! docs/design/streaming-hitches.md §1).
//!
//! `NVF_PLAY_TEST_PROFILE=<seconds>`: every that many real seconds one line
//! `profile: <s> s, <n> frames, frame mean <m> ms, cpu <c> ms a frame; per frame: <span>
//! <ms>, …` (cpu: the process's processor time over every thread, which other programs
//! on a busy machine do not inflate as they do wall times) with the
//! spans that took the most time in the window, summed over every thread and divided by
//! the frames (a system that runs on a worker thread beside others counts its own time,
//! not the frame's). The spans are Bevy's: each system (`system` spans, by the system's
//! name), the schedules, the render graph's nodes and the render app's systems. They exist
//! only in a build with Bevy's `trace` feature (`cargo build --release --features
//! profile`); without it the lines say no spans were seen. The layer is installed only
//! when the variable is set.
//!
//! A frame of 100 ms or more ([`HITCH_MS`], real time from one frame's report to the
//! next; `NVF_PLAY_TEST_PROFILE_HITCH_MS=<ms>` sets another threshold, to see what the
//! occasional 20 ms frames hold) also logs `profile hitch: <ms> ms; spans ms: <span> <ms> (<entries>), …`: the
//! spans that ended in it, over every thread, so a single stall is not lost in a window's
//! means. A line lists the [`LISTED_HITCH`] heaviest; `NVF_PLAY_TEST_PROFILE_HITCH_SPANS=<n>`
//! lists more, for the small spans inside a system (`tiles::write::<part>`, issue #1457).
//!
//! Each window also logs `profile scene: <n> mesh entities (<v> visible, <c> moved this
//! frame), <m> distinct meshes, <u> distinct materials in use (<a> material assets)`: what
//! the renderer batches by, and how many entities it extracts again because their
//! transform changed. And from the render world `profile batches: <phase> <v> views,
//! <m> multi-drawn sets, <b> batched bins, <u> unbatched bins; …` per binned phase,
//! summed over its views: each multi-drawn set is one indirect draw, each other bin one
//! draw, and the per-view preparing scales with them (graphics-enhanced.md §5.5); then
//! `transparent <v> views, <n> sorted draws`, the blended draws, one each.
//!
//! `profile particles: <n> frames; per frame: regular <p> published, <c> culled; effect
//! <p> published, <c> culled` (issue #1417): per frame of the window, the model particle
//! systems (`particles.rs`) and effect-shader bursts (`effect_particles.rs`) with live
//! particles whose mesh was rebuilt, and those left out because no active camera drawing
//! their layers could see them (their render entity hidden): [`ParticleCull`].
//!
//! `profile changes:` counts the meshes, images and game materials made, changed and
//! dropped per frame, and `profile change sources:` splits the changed meshes and game
//! materials by the system holding them when they changed (faces, vertex morphs,
//! particles, fades, effect shaders, rig and actor material animation, emittance; the rest
//! by the entity using them) and counts the changes of the other material types (water,
//! sky, grass, trees): `classify_changes`.
//!
//! `profile outfits:` counts successful NPC spawns by appearance source and committed
//! `follow_inventory` replacements per window, with their total synchronous time in ms.
//! A replacement counts one actor, even if both its armour and weapon change. Failed
//! appearance resolutions and unchanged outfits do not count [design].
//!
//! `NVF_PLAY_TEST_PROFILE_COUNTS=1` (a measuring aid of its own, with or without the
//! span profile): every window (`NVF_PLAY_TEST_PROFILE`'s seconds when that is set, else
//! [`COUNTS_SECONDS`]) two lines of counts, for attributing what the spans time:
//! `phys-refresh: secs=<s> frames=<n> refreshes=<n> by_consumer=player:<n>/npc:<n>/step:<n>/cast:<n> …`
//! (the broad-phase refreshes, `fnv_phys::RefreshStats`) and
//! `ai-counts: secs=<s> frames=<n> think=<n> path_searches=<n> …` (the residents' AI work).
//! Every field is `key=value`; the keys are listed in [`phys_refresh_line`] and
//! [`ai_counts_line`]. Without the variable nothing is logged and the AI counters (here,
//! `fnv_nav::path::probe` and `fnv_ai::apart::probe`) are not touched.

use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::span::{Attributes, Id};
use bevy::log::tracing::Subscriber;
use bevy::log::tracing_subscriber::layer::Context;
use bevy::log::tracing_subscriber::registry::LookupSpan;
use bevy::log::tracing_subscriber::Layer;
use bevy::log::BoxedLayer;
use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d};
use bevy::core_pipeline::prepass::{AlphaMask3dPrepass, Opaque3dPrepass};
use bevy::prelude::*;
use bevy::render::render_phase::ViewBinnedRenderPhases;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Spans listed in a line, the most costly first.
const LISTED: usize = 40;

/// A frame at least this long (milliseconds of real time from one report to the next)
/// logs its own spans (`profile hitch:`), which a window's means would hide.
const HITCH_MS: f64 = 100.0;

/// The hitch threshold: [`HITCH_MS`] or `NVF_PLAY_TEST_PROFILE_HITCH_MS`.
fn hitch_ms() -> f64 {
    static MS: OnceLock<f64> = OnceLock::new();
    *MS.get_or_init(|| {
        std::env::var("NVF_PLAY_TEST_PROFILE_HITCH_MS").ok().and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| *v > 0.0).unwrap_or(HITCH_MS)
    })
}

/// Spans listed in a hitch's line.
const LISTED_HITCH: usize = 25;

/// How many spans a hitch's line lists: [`LISTED_HITCH`] or `NVF_PLAY_TEST_PROFILE_HITCH_SPANS`.
fn listed_hitch() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| std::env::var("NVF_PLAY_TEST_PROFILE_HITCH_SPANS").ok().and_then(|v| v.trim().parse::<usize>().ok()).filter(|n| *n > 0).unwrap_or(LISTED_HITCH))
}

/// The window in seconds, when `NVF_PLAY_TEST_PROFILE` asks for a profile.
pub fn interval() -> Option<f32> {
    static EVERY: OnceLock<Option<f32>> = OnceLock::new();
    *EVERY.get_or_init(|| std::env::var("NVF_PLAY_TEST_PROFILE").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|s| *s > 0.0))
}

/// The counts' window in seconds when the span profile does not give one.
pub const COUNTS_SECONDS: f32 = 5.0;

/// Whether `NVF_PLAY_TEST_PROFILE_COUNTS` asks for the count lines.
pub fn counts_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("NVF_PLAY_TEST_PROFILE_COUNTS").is_ok_and(|v| !v.trim().is_empty() && v.trim() != "0"))
}

/// The residents' AI work counted for `ai-counts:` (cumulative; diagnostics only).
#[derive(Clone, Copy)]
pub enum AiCount {
    /// `Core::think` calls.
    Think,
    /// Residents `Core::process_levels` classified.
    Classified,
    /// Residents whose remaining path it scanned for the loaded area (the `Coming` test).
    ComingScans,
    /// Path points that scan tested.
    PathPointsTested,
    /// Process levels it changed.
    LevelChanges,
    /// `switch_navmeshes` calls (each polls the three below once).
    NavPolls,
    /// Navmesh switches in the game's list at each poll, summed.
    SwitchesPolled,
    /// Polls whose navmesh switches changed the graph.
    SwitchesChanged,
    /// Polls that saw a lock serial other than the last poll's.
    LockSerialChanges,
    /// Polls whose door locks changed the graph.
    LocksChanged,
    /// Nav obstacles visited at each poll, summed.
    ObstaclesPolled,
    /// Polls whose obstacles changed the graph.
    ObstaclesChanged,
    /// Actors `show_actor` asked to be drawn that were ready (spawn calls).
    Spawns,
    /// Scans of every reference root for an actor's root (`show_actor`).
    RootScans,
}

const AI_COUNTS: usize = AiCount::RootScans as usize + 1;
static AI: [AtomicU64; AI_COUNTS] = [const { AtomicU64::new(0) }; AI_COUNTS];

/// Count `n` of `c` (nothing without `NVF_PLAY_TEST_PROFILE_COUNTS`).
pub fn ai_count(c: AiCount, n: usize) {
    if counts_on() {
        AI[c as usize].fetch_add(n as u64, Relaxed);
    }
}

/// The door-lock serial a navigation poll saw: counts a change from the last poll's.
pub fn ai_lock_serial(serial: u64) {
    static LAST: AtomicU64 = AtomicU64::new(u64::MAX);
    if counts_on() && LAST.swap(serial, Relaxed) != serial {
        AI[AiCount::LockSerialChanges as usize].fetch_add(1, Relaxed);
    }
}

/// Time per span key in the current window: (seconds, entries).
static TOTALS: Mutex<Option<HashMap<String, (f64, u64)>>> = Mutex::new(None);

/// NPC outfit work in this report window. Callers accumulate locally, then add once
/// per system call; no per-actor locks and no timings when profiling is disabled.
#[derive(Default)]
struct Outfits {
    equipment: usize,
    record: usize,
    replacements: usize,
    replacement_ms: f64,
}

static OUTFITS: Mutex<Outfits> = Mutex::new(Outfits { equipment: 0, record: 0, replacements: 0, replacement_ms: 0.0 });

pub fn actor_spawns(equipment: usize, record: usize) {
    if interval().is_none() || equipment + record == 0 {
        return;
    }
    let mut o = OUTFITS.lock().unwrap_or_else(|e| e.into_inner());
    o.equipment += equipment;
    o.record += record;
}

pub fn outfit_replacements(count: usize, ms: f64) {
    if interval().is_none() || count == 0 {
        return;
    }
    let mut o = OUTFITS.lock().unwrap_or_else(|e| e.into_inner());
    o.replacements += count;
    o.replacement_ms += ms;
}

/// The render world's pipeline cache, as of its last frame: pipelines made, and those
/// still waiting to compile ([`pipeline_counts`]); a hitch line gives both, so a stall can
/// be told apart from one that waited on pipelines compiling (2026-10-28: material writes
/// and the window's frame took 100-500 ms on long tours with nothing else in them).
static PIPELINES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PIPELINES_WAITING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Time per span key since the last frame's report: (seconds, entries).
static FRAME: Mutex<Option<HashMap<String, (f64, u64)>>> = Mutex::new(None);

/// The layer, when the profile is asked for.
pub fn layer() -> Option<BoxedLayer> {
    interval()?;
    Some(Box::new(SpanTimes))
}

struct SpanTimes;

/// A span's key (its name, with the `name` field of Bevy's system spans, or the `system`
/// field of the render app's `finish_command_buffer` spans: which render system recorded the
/// encoder being finished) and when it was last entered.
struct Timing {
    key: String,
    entered: Option<Instant>,
}

/// Reads the `name` and `system` fields of a span.
#[derive(Default)]
struct NameField {
    name: Option<String>,
    system: Option<String>,
}

impl NameField {
    fn slot(&mut self, field: &Field) -> Option<&mut Option<String>> {
        match field.name() {
            "name" => Some(&mut self.name),
            "system" => Some(&mut self.system),
            _ => None,
        }
    }
}

impl Visit for NameField {
    fn record_str(&mut self, field: &Field, value: &str) {
        if let Some(slot) = self.slot(field) {
            *slot = Some(value.to_string());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if let Some(slot) = self.slot(field).filter(|s| s.is_none()) {
            *slot = Some(format!("{value:?}").trim_matches('"').to_string());
        }
    }
}

/// The key a span's time is summed under: a system span by its system's short name, a
/// span with a `system` field (`finish_command_buffer`) by its own name and that system's.
fn span_key(base: &str, fields: NameField) -> String {
    match fields {
        NameField { name: Some(n), .. } => short_name(&n),
        NameField { system: Some(s), .. } => format!("{base}[{}]", short_name(&s)),
        _ => base.to_string(),
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for SpanTimes {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = NameField::default();
        attrs.record(&mut fields);
        let key = span_key(attrs.metadata().name(), fields);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Timing { key, entered: None });
        }
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            if let Some(t) = span.extensions_mut().get_mut::<Timing>() {
                t.entered = Some(Instant::now());
            }
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut ext = span.extensions_mut();
        let Some(t) = ext.get_mut::<Timing>() else { return };
        let Some(at) = t.entered.take() else { return };
        let secs = at.elapsed().as_secs_f64();
        let mut totals = TOTALS.lock().unwrap_or_else(|e| e.into_inner());
        let e = totals.get_or_insert_with(HashMap::new).entry(t.key.clone()).or_default();
        e.0 += secs;
        e.1 += 1;
        drop(totals);
        let mut frame = FRAME.lock().unwrap_or_else(|e| e.into_inner());
        let e = frame.get_or_insert_with(HashMap::new).entry(t.key.clone()).or_default();
        e.0 += secs;
        e.1 += 1;
    }
}

/// A system's type path without its modules: `nvforever::residents::drive` →
/// `residents::drive` (the last two segments outside generics).
pub fn short_name(full: &str) -> String {
    let head = full.split('<').next().unwrap_or(full);
    let parts: Vec<&str> = head.split("::").collect();
    let keep = parts.len().saturating_sub(2);
    let mut s = parts[keep..].join("::");
    if head.len() < full.len() {
        s.push_str("<…>");
    }
    s
}

pub struct ProfilePlugin;

impl Plugin for ProfilePlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("NVF_PLAY_TEST_ACTOR_NO_SHADOWS").is_ok_and(|v| v == "1") {
            info!("test aid NVF_PLAY_TEST_ACTOR_NO_SHADOWS: actors cast no shadows");
            app.add_systems(Last, actors_cast_no_shadows);
        }
        if counts_on() {
            info!("test aid NVF_PLAY_TEST_PROFILE_COUNTS: physics refresh and AI counts every {} s", interval().unwrap_or(COUNTS_SECONDS));
            // The library counters count only from here, like `ai_count`.
            fnv_nav::path::probe::enable();
            fnv_ai::apart::probe::enable();
            app.add_systems(Last, report_counts);
        }
        let Some(every) = interval() else { return };
        info!("test aid NVF_PLAY_TEST_PROFILE: a span profile every {every} s");
        // GPU time per render pass (timestamp queries, Vulkan and DX12): `profile gpu:`.
        crate::graphics::ensure_render_diagnostics(app);
        app.insert_resource(ProfileWindow { every, ..default() }).init_resource::<AssetChanges>().init_resource::<ChangedAssets>().add_systems(
            Last,
            (
                count_asset_changes,
                count_material_type::<crate::water::WaterMaterial>,
                count_material_type::<crate::sky::SkyMaterial>,
                count_material_type::<crate::grass::GrassMaterial>,
                count_material_type::<crate::tree::LeafMaterial>,
                count_material_type::<crate::tree::BarkMaterial>,
                count_material_type::<crate::tree::BillboardMaterial>,
                classify_changes,
                report,
                report_change_sources,
            )
                .chain(),
        );
        // The particle builds count into it when it is there (`profile particles:`).
        app.insert_resource(ParticleCull { every, ..default() }).add_systems(Last, report_particles);
        if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render.add_systems(bevy::render::Render, pipeline_counts.in_set(bevy::render::RenderSystems::Render));
            render.add_systems(bevy::render::Render, (move |mut last: Local<Option<Instant>>, shadow: Res<ViewBinnedRenderPhases<bevy::pbr::Shadow>>, opaque: Res<ViewBinnedRenderPhases<Opaque3d>>, mask: Res<ViewBinnedRenderPhases<AlphaMask3d>>, prepass: Res<ViewBinnedRenderPhases<Opaque3dPrepass>>, mask_prepass: Res<ViewBinnedRenderPhases<AlphaMask3dPrepass>>, transparent: Res<bevy::render::render_phase::ViewSortedRenderPhases<bevy::core_pipeline::core_3d::Transparent3d>>| {
                // The render world has no clock of its own: real time.
                let now = Instant::now();
                let at = *last.get_or_insert(now);
                if now.duration_since(at).as_secs_f32() < every {
                    return;
                }
                *last = Some(now);
                info!(
                    "profile batches: {}; {}; {}; {}; {}; transparent {} views, {} sorted draws",
                    batches("shadow", &shadow),
                    batches("opaque", &opaque),
                    batches("alpha-mask", &mask),
                    batches("opaque prepass", &prepass),
                    batches("alpha-mask prepass", &mask_prepass),
                    transparent.len(),
                    transparent.values().map(|p| p.items.len()).sum::<usize>()
                );
            })
            .in_set(bevy::render::RenderSystems::Render));
        }
    }
}

/// Keep the pipeline counts for the main world's hitch lines.
fn pipeline_counts(cache: Res<bevy::render::render_resource::PipelineCache>) {
    use std::sync::atomic::Ordering::Relaxed;
    PIPELINES.store(cache.pipelines().count() as u64, Relaxed);
    PIPELINES_WAITING.store(cache.waiting_pipelines().count() as u64, Relaxed);
}

/// `<phase> <v> views, <m> multi-drawn sets, <b> batched bins, <u> unbatched bins`, summed
/// over the phase's views.
fn batches<BPI: bevy::render::render_phase::BinnedPhaseItem>(name: &str, phases: &ViewBinnedRenderPhases<BPI>) -> String {
    let (mut m, mut b, mut u) = (0, 0, 0);
    for phase in phases.values() {
        m += phase.multidrawable_meshes.len();
        b += phase.batchable_meshes.len();
        u += phase.unbatchable_meshes.len();
    }
    format!("{name} {} views, {m} multi-drawn sets, {b} batched bins, {u} unbatched bins", phases.len())
}

/// The asset changes of a window, for `profile changes:`: each made, changed or dropped
/// mesh, image and game material is sent to the GPU again (its staging memory included);
/// the night stalls came with GPU memory made and freed hundreds of times a second
/// (Nsight Systems, 2026-10-29), so this says what the frames send.
#[derive(Resource, Default)]
struct AssetChanges {
    /// (added, modified, removed) per kind: meshes, images, materials.
    counts: [[u64; 3]; 3],
    /// Bytes of the added and modified meshes' vertices and the images' pixels.
    mesh_bytes: u64,
    image_bytes: u64,
}

/// Which of (made, changed, dropped) an asset event counts as.
fn slot<A: Asset>(e: &AssetEvent<A>) -> Option<usize> {
    match e {
        AssetEvent::Added { .. } => Some(0),
        AssetEvent::Modified { .. } => Some(1),
        AssetEvent::Removed { .. } | AssetEvent::Unused { .. } => Some(2),
        _ => None,
    }
}

fn count_asset_changes(
    mut c: ResMut<AssetChanges>,
    mut by_asset: ResMut<ChangedAssets>,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    mut image_events: MessageReader<AssetEvent<Image>>,
    mut material_events: MessageReader<AssetEvent<nvf_assets::NifMaterial>>,
    meshes: Res<Assets<Mesh>>,
    images: Res<Assets<Image>>,
) {
    for e in mesh_events.read() {
        if let Some(i) = slot(e) {
            c.counts[0][i] += 1;
        }
        if let AssetEvent::Modified { id } = e {
            by_asset.frame_meshes.push(*id);
        }
        if let AssetEvent::Added { id } | AssetEvent::Modified { id } = e {
            if let Some(m) = meshes.get(*id) {
                c.mesh_bytes += (m.count_vertices() * m.get_vertex_size() as usize) as u64;
            }
        }
    }
    for e in image_events.read() {
        if let Some(i) = slot(e) {
            c.counts[1][i] += 1;
        }
        if let AssetEvent::Added { id } | AssetEvent::Modified { id } = e {
            if let Some(d) = images.get(*id).and_then(|im| im.data.as_ref()) {
                c.image_bytes += d.len() as u64;
            }
        }
    }
    for e in material_events.read() {
        if let Some(i) = slot(e) {
            c.counts[2][i] += 1;
        }
        if let AssetEvent::Modified { id } = e {
            by_asset.frame_materials.push(*id);
        }
    }
}

/// Which particle build a count comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParticleKind {
    /// The model particle systems (`particles.rs`).
    Regular = 0,
    /// The effect shaders' particles (`effect_particles.rs`).
    Effect = 1,
}

/// The particle builds' counts of a window for `profile particles:`. Present only while
/// the profile aid runs; the builds (`particles::draw`, `effect_particles::build`) count
/// into it when it is there and do nothing more without it.
#[derive(Resource, Default)]
pub struct ParticleCull {
    every: f32,
    /// Real seconds and frames of the window so far.
    since: f32,
    frames: u32,
    /// Per kind ([`ParticleKind`]), summed over the window's frames: systems with live
    /// particles whose mesh was rebuilt, and those left out (no camera could see them).
    published: [u64; 2],
    culled: [u64; 2],
}

impl ParticleCull {
    /// A system with live particles this frame: rebuilt when `seen`, else left out.
    pub fn count(&mut self, kind: ParticleKind, seen: bool) {
        let n = if seen { &mut self.published } else { &mut self.culled };
        n[kind as usize] += 1;
    }

    /// The window's sums so far: (published, culled) per kind.
    #[cfg(test)]
    pub fn totals(&self) -> ([u64; 2], [u64; 2]) {
        (self.published, self.culled)
    }
}

/// `profile particles: <n> frames; per frame: regular <p> published, <c> culled; effect
/// <p> published, <c> culled`.
fn particles_line(frames: u32, published: [u64; 2], culled: [u64; 2]) -> String {
    let per = |n: u64| n as f64 / f64::from(frames.max(1));
    format!(
        "profile particles: {frames} frames; per frame: regular {:.2} published, {:.2} culled; effect {:.2} published, {:.2} culled",
        per(published[0]),
        per(culled[0]),
        per(published[1]),
        per(culled[1])
    )
}

/// `profile particles:` every `NVF_PLAY_TEST_PROFILE` real seconds; the frame's builds ran
/// in `PostUpdate`, before this.
fn report_particles(time: Res<Time<Real>>, mut c: ResMut<ParticleCull>) {
    c.since += time.delta_secs();
    c.frames += 1;
    if c.since < c.every {
        return;
    }
    info!("{}", particles_line(c.frames, c.published, c.culled));
    *c = ParticleCull { every: c.every, ..default() };
}

/// The changed (modified) meshes and game materials of a window, by the source holding
/// them when they changed, and the other material types' changes by type, for `profile
/// change sources:` (which systems the `profile changes:` counts come from). Filled only
/// while the profile aid runs.
#[derive(Resource, Default)]
struct ChangedAssets {
    frames: u32,
    /// This frame's Modified events, classified by `classify_changes` the same frame.
    frame_meshes: Vec<AssetId<Mesh>>,
    frame_materials: Vec<AssetId<nvf_assets::NifMaterial>>,
    /// Changes of the window by (source, detail); the detail is set for [`OTHER`] only.
    meshes: HashMap<(&'static str, String), u32>,
    materials: HashMap<(&'static str, String), u32>,
    other_types: std::collections::BTreeMap<&'static str, u64>,
    /// The first user's label of each `other` asset seen, kept across windows so that
    /// the users are looked for once per asset ([`user_label`]).
    mesh_labels: HashMap<AssetId<Mesh>, String>,
    material_labels: HashMap<AssetId<nvf_assets::NifMaterial>, String>,
    /// The `report` window last listed.
    reported: u32,
}

/// Count the modified assets of another material type, under [`type_label`].
fn count_material_type<M: Asset>(mut c: ResMut<ChangedAssets>, mut events: MessageReader<AssetEvent<M>>) {
    let n = events.read().filter(|e| matches!(e, AssetEvent::Modified { .. })).count();
    if n > 0 {
        *c.other_types.entry(type_label::<M>()).or_default() += n as u64;
    }
}

/// A material type's own name: the last path segment, inside an `ExtendedMaterial`'s
/// generics the extension's (`GrassSway`, `WaterMaterial`).
fn type_label<M>() -> &'static str {
    let full = std::any::type_name::<M>().trim_end_matches('>');
    full.rsplit("::").next().unwrap_or(full)
}

/// The source the `other` entries of a split fall under; their details are listed.
const OTHER: &str = "other";

/// Changes per frame by source, the most first, with the busiest `other` details after:
/// `faces 3.00, other 1.50 (ref 0010ABCD 1.00, …)`. `items`: (source, detail, changes);
/// the detail counts only for [`OTHER`].
fn sources_text(items: &[(&'static str, String, u32)], frames: f64, details: usize) -> String {
    let mut sources: HashMap<&str, u64> = HashMap::new();
    let mut others: HashMap<&str, u64> = HashMap::new();
    for (source, detail, n) in items {
        *sources.entry(source).or_default() += u64::from(*n);
        if *source == OTHER {
            *others.entry(detail.as_str()).or_default() += u64::from(*n);
        }
    }
    fn sorted(m: HashMap<&str, u64>) -> Vec<(&str, u64)> {
        let mut v: Vec<(&str, u64)> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        v
    }
    let per = |n: u64| n as f64 / frames.max(1.0);
    let others = sorted(others);
    let parts: Vec<String> = sorted(sources)
        .into_iter()
        .map(|(s, n)| {
            if s != OTHER {
                return format!("{s} {:.2}", per(n));
            }
            let mut listed: Vec<String> = others.iter().take(details).map(|(d, n)| format!("{d} {:.2}", per(*n))).collect();
            if others.len() > details {
                listed.push(format!("{} more", others.len() - details));
            }
            format!("{s} {:.2} ({})", per(n), listed.join(", "))
        })
        .collect();
    if parts.is_empty() {
        "none".into()
    } else {
        parts.join(", ")
    }
}

/// The source of a changed asset: the first of `sources` (name, the ids that source
/// holds) that holds it, else [`OTHER`].
fn source_of<I: PartialEq>(id: &I, sources: &[(&'static str, Vec<I>)]) -> &'static str {
    sources.iter().find(|(_, ids)| ids.contains(id)).map_or(OTHER, |(name, _)| *name)
}

type UserLabels<'w, 's> = Query<'w, 's, (Option<&'static Name>, Option<&'static nvf_play::RefVisual>)>;

/// What a changed asset's first user is called: its own or an ancestor's `Name` (a NIF
/// node or file, an actor), or the placed reference it belongs to.
fn user_label(e: Entity, parents: &Query<&ChildOf>, labels: &UserLabels) -> String {
    for a in std::iter::once(e).chain(parents.iter_ancestors(e)) {
        match labels.get(a) {
            Ok((Some(n), _)) => return n.as_str().rsplit(['\\', '/']).next().unwrap_or(n.as_str()).to_string(),
            Ok((None, Some(r))) => return format!("ref {:08X}", r.0),
            _ => {}
        }
    }
    "(unnamed)".into()
}

/// Classify this frame's changed meshes and game materials by the asset ids the writing
/// systems hold now, so a fade, face or particle burst that ends inside the window is
/// still counted as what it was when it changed.
///
/// Meshes: the moving faces (`lips`), the vertex morphs, the particle systems, the
/// effect-shader bursts, the sky dome. Game materials: the see-through copies of fading
/// meshes (`nvf_play::fade`), the effect shaders' overlay and hole copies, the copies a
/// rig's clips drive (`material_anim`, `RigMaterials` slots it owns), the materials an
/// actor's clips drive (`actor_materials`: each driven shape's own copy and the material
/// it wears now, which is what posing writes while the shape fades or is drawn faded by
/// actor alpha; NPCs' rigs and the player's, `PlayerRigMaterials`), the emittance copies.
/// These are the assets
/// each system writes, but holding is not writing: an asset two of them hold is counted
/// under the first in that order (an actor's material copy that is fading is written by
/// the fade and may be by `actor_materials` too: it counts as a fade), and a write by any
/// other system to an asset one of them holds counts under that one. The rest is `other`,
/// by its first user's [`user_label`], looked for once per asset (a scan of the users
/// only on frames that bring an `other` asset not seen before).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn classify_changes(
    mut changed: ResMut<ChangedAssets>,
    (lips, models, particles, effect_particles, shaders, emittance): (
        Option<Res<crate::lips::LipState>>,
        Option<Res<nvf_assets::ModelCache>>,
        Option<Res<crate::particles::Particles>>,
        Option<Res<crate::effect_particles::EffectParticles>>,
        Option<Res<crate::effect_shaders::Shaders>>,
        Option<Res<crate::emittance::EmittanceState>>,
    ),
    (domes, fading, rigs, actors, player_rigs): (
        Query<&Mesh3d, With<crate::sky::SkyDome>>,
        Query<&nvf_play::fade::Fading>,
        Query<&nvf_assets::material_anim::RigMaterials>,
        Query<&crate::actor_materials::ActorMaterials>,
        Option<Res<crate::actor_materials::PlayerRigMaterials>>,
    ),
    mesh_users: Query<(Entity, &Mesh3d)>,
    material_users: Query<(Entity, &MeshMaterial3d<nvf_assets::NifMaterial>)>,
    parents: Query<&ChildOf>,
    labels: UserLabels,
) {
    let c = &mut *changed;
    c.frames += 1;
    if !c.frame_meshes.is_empty() {
        let sources = [
            ("faces", lips.iter().flat_map(|l| l.face_meshes()).collect::<Vec<_>>()),
            ("vertex morphs", models.iter().flat_map(|m| m.morphs.ids()).collect()),
            ("particles", particles.iter().flat_map(|p| p.mesh_ids()).collect()),
            ("effect particles", effect_particles.iter().flat_map(|p| p.mesh_ids()).collect()),
            ("sky dome", domes.iter().map(|m| m.0.id()).collect()),
        ];
        let ids = std::mem::take(&mut c.frame_meshes);
        let classified: Vec<(&'static str, AssetId<Mesh>)> = ids.iter().map(|id| (source_of(id, &sources), *id)).collect();
        if classified.iter().any(|(s, id)| *s == OTHER && !c.mesh_labels.contains_key(id)) {
            if c.mesh_labels.len() > 20_000 {
                c.mesh_labels.clear();
            }
            for (e, m) in &mesh_users {
                let id = m.0.id();
                if !c.mesh_labels.contains_key(&id) && classified.iter().any(|(s, i)| *s == OTHER && *i == id) {
                    c.mesh_labels.insert(id, user_label(e, &parents, &labels));
                }
            }
        }
        for (source, id) in classified {
            let detail = if source == OTHER { c.mesh_labels.get(&id).cloned().unwrap_or_else(|| "(no user)".into()) } else { String::new() };
            *c.meshes.entry((source, detail)).or_default() += 1;
        }
    }
    if !c.frame_materials.is_empty() {
        let sources = [
            ("fades", fading.iter().map(|f| f.copy().id()).collect::<Vec<_>>()),
            ("effect shaders", shaders.iter().flat_map(|s| s.active.values().flat_map(|a| a.materials()).collect::<Vec<_>>()).collect()),
            ("rig material animation", rigs.iter().flat_map(|r| r.slots.iter().filter(|s| s.own).map(|s| s.handle.id())).collect()),
            ("actor material animation", {
                // Each driven shape's own copy and the material it wears now (a faded
                // shape's posing writes that one), NPCs' and the player's rigs alike.
                let player = player_rigs.iter().flat_map(|p| p.0.values());
                let shapes = actors.iter().chain(player).flat_map(|a| a.shapes());
                shapes.flat_map(|(e, mine)| mine.into_iter().chain(material_users.get(e).ok().map(|(_, m)| m.0.id()))).collect()
            }),
            ("emittance", emittance.iter().flat_map(|e| e.copies()).collect()),
        ];
        let ids = std::mem::take(&mut c.frame_materials);
        let classified: Vec<(&'static str, AssetId<nvf_assets::NifMaterial>)> = ids.iter().map(|id| (source_of(id, &sources), *id)).collect();
        if classified.iter().any(|(s, id)| *s == OTHER && !c.material_labels.contains_key(id)) {
            if c.material_labels.len() > 20_000 {
                c.material_labels.clear();
            }
            for (e, m) in &material_users {
                let id = m.0.id();
                if !c.material_labels.contains_key(&id) && classified.iter().any(|(s, i)| *s == OTHER && *i == id) {
                    c.material_labels.insert(id, user_label(e, &parents, &labels));
                }
            }
        }
        for (source, id) in classified {
            let detail = if source == OTHER { c.material_labels.get(&id).cloned().unwrap_or_else(|| "(no user)".into()) } else { String::new() };
            *c.materials.entry((source, detail)).or_default() += 1;
        }
    }
}

/// `profile change sources:` after each `report` window (`classify_changes` says what
/// the sources are).
fn report_change_sources(w: Res<ProfileWindow>, mut changed: ResMut<ChangedAssets>) {
    if w.reports == changed.reported {
        return;
    }
    let c = &mut *changed;
    c.reported = w.reports;
    let frames = f64::from(c.frames.max(1));
    let items = |m: &HashMap<(&'static str, String), u32>| m.iter().map(|((s, d), n)| (*s, d.clone(), *n)).collect::<Vec<_>>();
    let types: Vec<String> = c.other_types.iter().map(|(t, n)| format!("{t} {:.2}", *n as f64 / frames)).collect();
    info!(
        "profile change sources: per frame: meshes changed: {}; game materials changed: {}; other material types changed: {}",
        sources_text(&items(&c.meshes), frames, 6),
        sources_text(&items(&c.materials), frames, 6),
        if types.is_empty() { "none".into() } else { types.join(", ") }
    );
    c.frames = 0;
    c.meshes.clear();
    c.materials.clear();
    c.other_types.clear();
}

#[derive(Resource, Default)]
struct ProfileWindow {
    every: f32,
    /// Windows reported so far (`report_change_sources` follows them).
    reports: u32,
    /// The process's processor time at the window's start (seconds).
    cpu_at: Option<f64>,
    since: f32,
    frames: u32,
    frame_secs: f32,
}

/// Test aid `NVF_PLAY_TEST_ACTOR_NO_SHADOWS=1`: every mesh under an actor's skeleton stops
/// casting shadows (still drawn), for measuring what the actors' shadows cost.
fn actors_cast_no_shadows(
    mut commands: Commands,
    meshes: Query<Entity, (With<Mesh3d>, Without<bevy::light::NotShadowCaster>)>,
    parents: Query<&ChildOf>,
    rigs: Query<(), With<crate::limbs::ActorRig>>,
    mut checked: Local<std::collections::HashSet<Entity>>,
    mut marked: Local<usize>,
) {
    let before = *marked;
    for e in &meshes {
        if !checked.insert(e) {
            continue;
        }
        if parents.iter_ancestors(e).any(|a| rigs.contains(a)) {
            commands.entity(e).insert(bevy::light::NotShadowCaster);
            *marked += 1;
        }
    }
    if *marked / 100 != before / 100 || (before == 0 && *marked > 0) {
        info!("test aid NVF_PLAY_TEST_ACTOR_NO_SHADOWS: {} actor meshes cast no shadows, {} rigs", *marked, rigs.iter().count());
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn report(
    time: Res<Time<Real>>,
    mut w: ResMut<ProfileWindow>,
    meshes: Query<(&Mesh3d, Option<&MeshMaterial3d<nvf_assets::NifMaterial>>, &ViewVisibility)>,
    moved: Query<(), (With<Mesh3d>, Changed<GlobalTransform>)>,
    materials: Res<Assets<nvf_assets::NifMaterial>>,
    mut last: Local<Option<Instant>>,
    loading: Option<Res<nvf_play::loading::Loading>>,
    mut pipelines_before: Local<u64>,
    (diagnostics, mut changes): (Res<bevy::diagnostic::DiagnosticsStore>, ResMut<AssetChanges>),
    (suns, visibles): (
        Query<(Entity, &bevy::camera::visibility::CascadesVisibleEntities), With<DirectionalLight>>,
        Query<(&InheritedVisibility, Has<Mesh3d>, Has<Children>)>,
    ),
    (transforms, changed_transforms, same_transforms): (
        Query<(), With<Transform>>,
        Query<(Entity, Has<Mesh3d>, Option<&Name>), Changed<Transform>>,
        Query<Ref<GlobalTransform>, Changed<Transform>>,
    ),
) {
    let pipelines = PIPELINES.load(std::sync::atomic::Ordering::Relaxed);
    let new_pipelines = pipelines.saturating_sub(*pipelines_before);
    *pipelines_before = pipelines;
    // A slow frame's own spans: those ended since the last report, whatever the thread.
    let now = Instant::now();
    let frame = FRAME.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default();
    if let Some(at) = last.replace(now) {
        let ms = now.duration_since(at).as_secs_f64() * 1000.0;
        if ms >= hitch_ms() {
            let line = format!(
                "{}; pipelines {pipelines} ({new_pipelines} new since the frame before, {} waiting)",
                hitch_line(ms, &frame, listed_hitch()),
                PIPELINES_WAITING.load(std::sync::atomic::Ordering::Relaxed)
            );
            if loading.as_deref().is_some_and(nvf_play::loading::Loading::unseen_frames) {
                // Not seen: kept apart from the hitches (`profile hitch (behind a loading screen):`).
                info!("{}", line.replacen("profile hitch:", "profile hitch (behind a loading screen):", 1));
            } else {
                info!("{line}");
            }
        }
    }
    let dt = time.delta_secs();
    w.since += dt;
    w.frames += 1;
    w.frame_secs += dt;
    if w.since < w.every {
        return;
    }
    let totals = TOTALS.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default();
    let frames = w.frames.max(1) as f64;
    let cpu = process_cpu_secs();
    let cpu_ms = cpu.zip(w.cpu_at).map(|(now, at)| (now - at) * 1000.0 / frames);
    let text = line(w.since, w.frames, w.frame_secs, &totals, frames);
    info!("{}", match cpu_ms {
        Some(c) => text.replacen(" ms; ", &format!(" ms, cpu {c:.1} ms a frame; "), 1),
        None => text,
    });
    let (mut visible, mut ids, mut mats) = (0usize, std::collections::HashSet::new(), std::collections::HashSet::new());
    for (mesh, material, vis) in &meshes {
        visible += usize::from(vis.get());
        ids.insert(mesh.0.id());
        if let Some(m) = material {
            mats.insert(m.0.id());
        }
    }
    info!(
        "profile scene: {} mesh entities ({visible} visible, {} moved this frame), {} distinct meshes, {} distinct materials in use ({} material assets)",
        meshes.iter().count(),
        moved.iter().count(),
        ids.len(),
        mats.len(),
        materials.len()
    );
    // Shadow casters each cascade of each directional light drew this frame: whether a nearer,
    // smaller cascade really holds fewer casters than the far one.
    let shadows: Vec<String> = suns
        .iter()
        .flat_map(|(light, c)| c.entities.values().map(move |cascades| {
            let counts: Vec<String> = cascades.iter().map(|v| v.entities.len().to_string()).collect();
            format!("light {light}: {}", counts.join("/"))
        }))
        .collect();
    // Every entity with visibility is walked by Bevy's visibility systems each frame, meshes or not.
    let (mut with_vis, mut shown, mut bare) = (0usize, 0usize, 0usize);
    for (inherited, mesh, children) in &visibles {
        with_vis += 1;
        shown += usize::from(inherited.get());
        bare += usize::from(!mesh && !children);
    }
    info!("profile visibility: {with_vis} entities with visibility ({shown} not hidden), {bare} with neither a mesh nor children");
    // Transforms written this frame (each makes its tree dirty for propagation), how many
    // of them carry a mesh, the most common names, and how many ended where they were.
    let mut names: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let (mut changed, mut with_mesh) = (0usize, 0usize);
    for (_, mesh, name) in &changed_transforms {
        changed += 1;
        with_mesh += usize::from(mesh);
        let key = name.map(|n| n.as_str().split(|c: char| c.is_ascii_digit()).next().unwrap_or("").to_string()).unwrap_or_else(|| "(no name)".into());
        *names.entry(key).or_default() += 1;
    }
    let unmoved = same_transforms.iter().filter(|g| !g.is_changed()).count();
    let mut top: Vec<(String, usize)> = names.into_iter().collect();
    top.sort_by_key(|t| std::cmp::Reverse(t.1));
    let top: Vec<String> = top.iter().take(8).map(|(n, c)| format!("{n} {c}")).collect();
    info!(
        "profile transforms: {changed} of {} written this frame ({with_mesh} with a mesh; {unmoved} not yet propagated), most: {}",
        transforms.iter().count(),
        top.join(", ")
    );
    info!("profile shadows: casters per cascade, nearest first: {}", shadows.join("; "));
    let per = |n: u64| n as f64 / frames;
    let o = std::mem::take(&mut *OUTFITS.lock().unwrap_or_else(|e| e.into_inner()));
    info!(
        "profile outfits: {} NPCs spawned ({} simulation equipment, {} record fallback); {} follow_inventory replacements ({:.2} ms)",
        o.equipment + o.record, o.equipment, o.record, o.replacements, o.replacement_ms
    );
    let c = std::mem::take(&mut *changes);
    info!(
        "profile changes: per frame: meshes {:.1} made {:.1} changed {:.1} dropped ({:.2} MB of vertices), images {:.1} made {:.1} changed {:.1} dropped ({:.2} MB of pixels), materials {:.1} made {:.1} changed {:.1} dropped",
        per(c.counts[0][0]),
        per(c.counts[0][1]),
        per(c.counts[0][2]),
        per(c.mesh_bytes) / 1e6,
        per(c.counts[1][0]),
        per(c.counts[1][1]),
        per(c.counts[1][2]),
        per(c.image_bytes) / 1e6,
        per(c.counts[2][0]),
        per(c.counts[2][1]),
        per(c.counts[2][2]),
    );
    // The render passes' GPU time (Bevy's render diagnostics, averaged over their last
    // samples), the costliest first: which pass grows when the frame waits on the GPU.
    let mut gpu: Vec<(String, f64)> = diagnostics
        .iter()
        .filter(|d| d.path().as_str().ends_with("/elapsed_gpu"))
        .filter_map(|d| Some((d.path().as_str().trim_start_matches("render/").trim_end_matches("/elapsed_gpu").to_string(), d.average()?)))
        .collect();
    gpu.sort_by(|a, b| b.1.total_cmp(&a.1));
    if !gpu.is_empty() {
        let listed: Vec<String> = gpu.iter().take(if bevy::render::renderer::encoder_spans_on() { 80 } else { 16 }).map(|(k, ms)| format!("{k} {ms:.2}")).collect();
        info!("profile gpu: ms a frame: {}", listed.join(", "));
    }
    *w = ProfileWindow { every: w.every, cpu_at: cpu, reports: w.reports.wrapping_add(1), ..default() };
}

/// The counts at the start of a window (`report_counts`).
#[derive(Default)]
struct CountsWindow {
    since: f32,
    frames: u32,
    phys: fnv_phys::RefreshStats,
    nav: (u64, u64),
    apart: (u64, u64, u64),
    ai: [u64; AI_COUNTS],
}

/// `NVF_PLAY_TEST_PROFILE_COUNTS`: the `phys-refresh:` and `ai-counts:` lines, each the
/// differences of cumulative counts over the window. Reads the physics state only (no
/// change detection is set off).
fn report_counts(time: Res<Time<Real>>, phys: Option<Res<nvf_play::phys::PhysState>>, mut w: Local<CountsWindow>) {
    w.since += time.delta_secs();
    w.frames += 1;
    if w.since < interval().unwrap_or(COUNTS_SECONDS) {
        return;
    }
    let (secs, frames) = (w.since, w.frames);
    if let Some(p) = phys {
        let now = p.world.refresh_stats();
        // A new world starts its counts again.
        let before = if now.refreshes < w.phys.refreshes || now.marks.iter().zip(&w.phys.marks).any(|(a, b)| a < b) { fnv_phys::RefreshStats::default() } else { w.phys };
        info!("{}", phys_refresh_line(secs, frames, &now, &before, p.world.standalone_hit_volumes()));
        w.phys = now;
    }
    let nav = fnv_nav::path::probe::searches();
    let apart = fnv_ai::apart::probe::separations();
    let ai: [u64; AI_COUNTS] = std::array::from_fn(|i| AI[i].load(Relaxed));
    info!("{}", ai_counts_line(secs, frames, (nav.0 - w.nav.0, nav.1 - w.nav.1), (apart.0 - w.apart.0, apart.1 - w.apart.1, apart.2 - w.apart.2), &std::array::from_fn(|i| ai[i] - w.ai[i])));
    *w = CountsWindow { phys: w.phys, nav, apart, ai, ..default() };
}

/// `phys-refresh: secs=<s> frames=<n> refreshes=<n> by_consumer=player:<n>/npc:<n>/step:<n>/cast:<n>
/// hitvol_only=player:<n>/npc:<n>/step:<n>/cast:<n> dirty_hitvol=<n> dirty_bodies=<n>
/// dirty_transform=<n> dirty_other=<n> with_hitvol=<n> with_bodies=<n> with_transform=<n>
/// with_other=<n> step_absorbed=<n> hitvol_colliders=<n> colliders_mean=<x>
/// repair_new=<n> repair_sleep=<n> repair_struck=<n> hitvol_written=<n> hitvol_skipped=<n>
/// hitvol_standalone=<0|1>`, the counts of `now` less `before`; `hitvol_standalone` is the
/// world's setting (1 unless `nvf_play::phys::HITVOL_ATTACHED_VAR`), not a count.
fn phys_refresh_line(secs: f32, frames: u32, now: &fnv_phys::RefreshStats, before: &fnv_phys::RefreshStats, standalone: bool) -> String {
    let d = |a: u64, b: u64| a.saturating_sub(b);
    let by = |a: &[u64; 4], b: &[u64; 4]| format!("player:{}/npc:{}/step:{}/cast:{}", d(a[0], b[0]), d(a[1], b[1]), d(a[2], b[2]), d(a[3], b[3]));
    let refreshes = d(now.refreshes, before.refreshes);
    let colliders = d(now.colliders, before.colliders) as f64 / refreshes.max(1) as f64;
    format!(
        "phys-refresh: secs={secs:.1} frames={frames} refreshes={refreshes} by_consumer={} hitvol_only={} dirty_hitvol={} dirty_bodies={} dirty_transform={} dirty_other={} with_hitvol={} with_bodies={} with_transform={} with_other={} step_absorbed={} hitvol_colliders={} colliders_mean={colliders:.0} repair_new={} repair_sleep={} repair_struck={} hitvol_written={} hitvol_skipped={} hitvol_standalone={}",
        by(&now.by_consumer, &before.by_consumer),
        by(&now.hitvol_only, &before.hitvol_only),
        d(now.marks[0], before.marks[0]),
        d(now.marks[1], before.marks[1]),
        d(now.marks[2], before.marks[2]),
        d(now.marks[3], before.marks[3]),
        d(now.refreshes_with[0], before.refreshes_with[0]),
        d(now.refreshes_with[1], before.refreshes_with[1]),
        d(now.refreshes_with[2], before.refreshes_with[2]),
        d(now.refreshes_with[3], before.refreshes_with[3]),
        d(now.step_absorbed, before.step_absorbed),
        d(now.hitvol_colliders, before.hitvol_colliders),
        d(now.repair_new, before.repair_new),
        d(now.repair_sleep, before.repair_sleep),
        d(now.repair_struck, before.repair_struck),
        d(now.hitvol_written, before.hitvol_written),
        d(now.hitvol_skipped, before.hitvol_skipped),
        u8::from(standalone),
    )
}

/// `ai-counts: secs=<s> frames=<n> think=<n> path_searches=<n> path_hits=<n>
/// separation_passes=<n> separation_bodies=<n> separation_pairs=<n> classified=<n>
/// coming_scans=<n> path_points_tested=<n> level_changes=<n> nav_polls=<n>
/// switches_polled=<n> switches_changed=<n> lock_serial_changes=<n> locks_changed=<n>
/// obstacles_polled=<n> obstacles_changed=<n> spawns=<n> root_scans=<n>`.
fn ai_counts_line(secs: f32, frames: u32, nav: (u64, u64), apart: (u64, u64, u64), ai: &[u64; AI_COUNTS]) -> String {
    let c = |k: AiCount| ai[k as usize];
    format!(
        "ai-counts: secs={secs:.1} frames={frames} think={} path_searches={} path_hits={} separation_passes={} separation_bodies={} separation_pairs={} classified={} coming_scans={} path_points_tested={} level_changes={} nav_polls={} switches_polled={} switches_changed={} lock_serial_changes={} locks_changed={} obstacles_polled={} obstacles_changed={} spawns={} root_scans={}",
        c(AiCount::Think),
        nav.0,
        nav.1,
        apart.0,
        apart.1,
        apart.2,
        c(AiCount::Classified),
        c(AiCount::ComingScans),
        c(AiCount::PathPointsTested),
        c(AiCount::LevelChanges),
        c(AiCount::NavPolls),
        c(AiCount::SwitchesPolled),
        c(AiCount::SwitchesChanged),
        c(AiCount::LockSerialChanges),
        c(AiCount::LocksChanged),
        c(AiCount::ObstaclesPolled),
        c(AiCount::ObstaclesChanged),
        c(AiCount::Spawns),
        c(AiCount::RootScans),
    )
}

/// The line for a window.
fn line(secs: f32, frames: u32, frame_secs: f32, totals: &HashMap<String, (f64, u64)>, per: f64) -> String {
    let mut spans: Vec<(&String, &(f64, u64))> = totals.iter().collect();
    spans.sort_by(|a, b| b.1 .0.total_cmp(&a.1 .0));
    let listed: Vec<String> = spans.iter().take(if bevy::render::renderer::encoder_spans_on() { 140 } else { LISTED }).map(|(k, (s, _))| format!("{k} {:.2}", s * 1000.0 / per)).collect();
    let mean = if frames > 0 { frame_secs * 1000.0 / frames as f32 } else { 0.0 };
    if listed.is_empty() {
        format!("profile: {secs:.1} s, {frames} frames, frame mean {mean:.1} ms; no spans seen (build with --features profile)")
    } else {
        format!("profile: {secs:.1} s, {frames} frames, frame mean {mean:.1} ms; per frame ms: {}", listed.join(", "))
    }
}

/// The line for one slow frame: `profile hitch: <ms> ms; spans ms: <span> <ms> (<n>), …`,
/// each span's time in that frame and how often it was entered.
fn hitch_line(ms: f64, frame: &HashMap<String, (f64, u64)>, listed: usize) -> String {
    let mut spans: Vec<(&String, &(f64, u64))> = frame.iter().collect();
    spans.sort_by(|a, b| b.1 .0.total_cmp(&a.1 .0));
    let listed: Vec<String> = spans.iter().take(listed).map(|(k, (s, n))| format!("{k} {:.1} ({n})", s * 1000.0)).collect();
    if listed.is_empty() {
        format!("profile hitch: {ms:.0} ms; no spans seen (build with --features profile)")
    } else {
        format!("profile hitch: {ms:.0} ms; spans ms: {}", listed.join(", "))
    }
}

/// The processor time the process has used so far, user and kernel, seconds.
#[cfg(windows)]
pub(crate) fn process_cpu_secs() -> Option<f64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let mut t = [FILETIME::default(); 4];
    // SAFETY: the pseudo handle of the current process needs no closing; the four out
    // pointers are to live FILETIMEs.
    let ok = unsafe { GetProcessTimes(GetCurrentProcess(), &mut t[0], &mut t[1], &mut t[2], &mut t[3]) };
    let secs = |f: &FILETIME| ((u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)) as f64 * 1e-7;
    (ok != 0).then(|| secs(&t[2]) + secs(&t[3]))
}

#[cfg(not(windows))]
pub(crate) fn process_cpu_secs() -> Option<f64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particles_line_gives_per_frame_counts() {
        let mut c = ParticleCull::default();
        for _ in 0..21 {
            c.count(ParticleKind::Regular, true);
        }
        for _ in 0..123 {
            c.count(ParticleKind::Regular, false);
        }
        c.count(ParticleKind::Effect, false);
        assert_eq!(
            particles_line(10, c.published, c.culled),
            "profile particles: 10 frames; per frame: regular 2.10 published, 12.30 culled; effect 0.00 published, 0.10 culled"
        );
        assert!(particles_line(0, [0; 2], [0; 2]).starts_with("profile particles: 0 frames; per frame: regular 0.00 published"));
    }

    #[test]
    fn change_sources_sum_per_source_and_list_the_others() {
        let items = vec![
            ("faces", String::new(), 30),
            ("faces", String::new(), 10),
            (OTHER, "ref 00000001".to_string(), 5),
            (OTHER, "ref 00000002".to_string(), 15),
            (OTHER, "ref 00000002".to_string(), 5),
            (OTHER, "Flag".to_string(), 1),
        ];
        assert_eq!(sources_text(&items, 10.0, 2), "faces 4.00, other 2.60 (ref 00000002 2.00, ref 00000001 0.50, 1 more)");
        assert_eq!(sources_text(&[], 10.0, 2), "none");
    }

    #[test]
    fn actor_material_writes_count_under_their_writer_faded_or_the_player_s() {
        use crate::actor_materials::{ActorMaterials, PlayerRigMaterials};
        use bevy::ecs::system::RunSystemOnce;
        let h = |n: u128| Handle::<nvf_assets::NifMaterial>::Uuid(bevy::asset::uuid::Uuid::from_u128(n), Default::default());
        let mut world = World::new();
        // An NPC's driven shape with no copy of its own: drawn faded (actor alpha), posing
        // writes the see-through material it wears.
        let faded = world.spawn(MeshMaterial3d(h(1))).id();
        world.spawn(ActorMaterials::with_shape(faded));
        // The player's weapon in hand, driven by the first-person rig's clips.
        let weapon = world.spawn(MeshMaterial3d(h(2))).id();
        let mut player = PlayerRigMaterials::default();
        player.0.insert(Entity::PLACEHOLDER, ActorMaterials::with_shape(weapon));
        world.insert_resource(player);
        world.insert_resource(ChangedAssets { frame_materials: vec![h(1).id(), h(2).id(), h(3).id()], ..default() });
        world.run_system_once(classify_changes).unwrap();
        let c = world.resource::<ChangedAssets>();
        assert_eq!(c.materials.get(&("actor material animation", String::new())), Some(&2));
        assert_eq!(c.materials.get(&(OTHER, "(no user)".to_string())), Some(&1));
    }

    #[test]
    fn a_change_counts_under_the_first_source_holding_it() {
        let sources = [("fades", vec![1, 2]), ("rig material animation", vec![2, 3])];
        assert_eq!(source_of(&2, &sources), "fades");
        assert_eq!(source_of(&3, &sources), "rig material animation");
        assert_eq!(source_of(&9, &sources), OTHER);
    }

    #[test]
    fn material_types_are_named_by_their_own_part() {
        assert_eq!(type_label::<crate::water::WaterMaterial>(), "WaterMaterial");
        assert_eq!(type_label::<crate::grass::GrassMaterial>(), "GrassSway");
    }

    #[test]
    fn system_names_keep_their_module() {
        assert_eq!(short_name("nvforever::residents::drive"), "residents::drive");
        assert_eq!(short_name("drive"), "drive");
        assert_eq!(short_name("bevy_ecs::event::update<nvf_play::Foo>"), "event::update<…>");
    }

    #[test]
    fn finished_encoders_are_keyed_by_the_system_that_recorded_them() {
        let f = |name: Option<&str>, system: Option<&str>| NameField { name: name.map(Into::into), system: system.map(Into::into) };
        assert_eq!(span_key("system", f(Some("nvforever::residents::drive"), None)), "residents::drive");
        assert_eq!(span_key("finish_command_buffer", f(None, Some("bevy_pbr::ssao::ssao"))), "finish_command_buffer[ssao::ssao]");
        assert_eq!(span_key("queue_submit", f(None, None)), "queue_submit");
    }

    #[test]
    fn a_line_lists_the_costliest_first_per_frame() {
        let mut t = HashMap::new();
        t.insert("a".to_string(), (0.010, 10));
        t.insert("b".to_string(), (0.100, 10));
        let l = line(1.0, 10, 1.0, &t, 10.0);
        assert!(l.contains("per frame ms: b 10.00, a 1.00"), "{l}");
        assert!(line(1.0, 0, 0.0, &HashMap::new(), 1.0).contains("no spans seen"));
    }

    #[test]
    fn count_lines_are_key_value_pairs_after_their_prefix() {
        let mut now = fnv_phys::RefreshStats { refreshes: 7, colliders: 70, ..Default::default() };
        now.by_consumer = [4, 2, 0, 1];
        let l = phys_refresh_line(5.0, 300, &now, &fnv_phys::RefreshStats::default(), true);
        assert!(l.starts_with("phys-refresh: secs=5.0 frames=300 refreshes=7 by_consumer=player:4/npc:2/step:0/cast:1 "), "{l}");
        assert!(l.contains(" colliders_mean=10 "), "{l}");
        assert!(l.ends_with(" hitvol_written=0 hitvol_skipped=0 hitvol_standalone=1"), "{l}");
        let a = ai_counts_line(5.0, 300, (3, 2), (1, 4, 6), &[0; AI_COUNTS]);
        assert!(a.starts_with("ai-counts: secs=5.0 frames=300 think=0 path_searches=3 path_hits=2 separation_passes=1 separation_bodies=4 separation_pairs=6 "), "{a}");
        for l in [&l, &a] {
            assert!(l.split_once(": ").unwrap().1.split(' ').all(|kv| kv.split_once('=').is_some_and(|(k, v)| !k.is_empty() && !v.is_empty())), "{l}");
        }
    }

    #[test]
    fn a_hitch_lists_its_own_spans_costliest_first() {
        let mut t = HashMap::new();
        t.insert("a".to_string(), (0.010, 1));
        t.insert("b".to_string(), (0.500, 3));
        let l = hitch_line(612.4, &t, LISTED_HITCH);
        assert!(l.starts_with("profile hitch: 612 ms; spans ms: b 500.0 (3), a 10.0 (1)"), "{l}");
        assert!(hitch_line(150.0, &HashMap::new(), LISTED_HITCH).contains("no spans seen"));
    }
}
