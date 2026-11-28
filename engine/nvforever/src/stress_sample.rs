//! The stress tests' samples (docs/design/stress-tests.md; the runner is `tools/stress`).
//!
//! `NVF_PLAY_TEST_SAMPLE=<seconds>`: every that many real seconds one line
//! `stress sample: t <s> s, frames <n>, frame mean <m> ms, frame worst <w> ms, entities <n>,
//! refs drawn <n>, meshes <n>, images <n>, materials <n>, models cached <n>, textures
//! cached <n>, audio <n>, cells <n>,
//! bodies <fixed>/<kinematic>/<dynamic>, bodies awake <n>, contact pairs <n>, manifolds <n>,
//! solver contacts <n>, joints <n>, solver ms <a>/<r>/<d>/<i>/<c>/<s>, actors <n>,
//! nan actors <n>, ragdolls <n>,
//! projectiles <n>[, gpu allocated <MB>, gpu reserved <MB>, gpu blocks <n>], mesh alloc
//! <ms> ms, mesh alloc worst <ms> ms, meshes written <n>, meshes freed <n>, slabs made <n>,
//! slab mb <MB>, slab mb copied <MB>, startup frames <n>, hidden frames <n>, phase changes
//! <n>, window <ms> ms, at <place>`
//! (the mesh allocator's work over the window: vendored bevy_render `nvf_mesh_alloc`,
//! #3057)
//! with what the engine holds now and where the camera is (the crash report's place text);
//! `bodies awake` to `joints` are the physics solver's load now, and `solver ms` rapier's
//! stage timers summed since the start (`fnv_phys::Stats::solver_us`: set-up, solve,
//! collision detection, islands, CCD, the whole step; 0 outside the profile build) (#1798),
//! and `stress gpu labels: <label> <MB> in <n>; …` (the GPU allocator's report, where the
//! backend gives one: [`gpu_memory`]). At the exit, `stress frames: <n> frames, mean <m> ms,
//! 1% low <l> ms (<fps> fps), worst <w> ms` over the run after its start-up ([`frame_stats`]),
//! and `stress actors: <n> drawn in <f> frames (<ms> ms, most in a frame <m> ms); those
//! frames mean <m> ms, worst <w> ms, <k> over 100 ms`.
//!
//! The frame measured (issue #1415): a frame is the time between two consecutive runs of
//! the sampler in `First`, each run a frame boundary. Each boundary takes its own instant
//! (`Instant::now()`), the phase it sees (start-up, behind a loading screen, seen), the
//! streamer's and `readahead::clock`'s counters and the place. A frame's time is the
//! difference between its two boundaries' instants, its cell and actor counts the counters'
//! differences, its phase the one both boundaries saw: time, phase, counts and place all
//! come from the same two boundaries ([`Samples::step`], [`frame`]). The time is the main
//! thread's whole loop from one `First` to the next: the frame's schedules, the wait for
//! the render thread's previous frame at extraction, extraction itself and the window
//! events [design: bevy_render 0.20.1 `pipelined_rendering.rs:115-136`].
//!
//! It is not `Time<Real>`'s delta, which the sampler used before issue #1415. That clock
//! takes the instant the render world sends after its render schedule [design: bevy_render
//! 0.20.1 `lib.rs:464,522-524`], received in `First` or `Instant::now()` when none is queued
//! [design: bevy_time 0.20.1 `lib.rs:163,180`]. With pipelined rendering (in bevy's default
//! plugins, which `main.rs` keeps [design: bevy_internal 0.20.1 `default_plugins.rs:56-57`])
//! the delta that arrives at a boundary can be the frame before's, so a long frame a loading
//! screen came down in could arrive one boundary late, when both boundaries saw the screen
//! down, and be counted as a seen hitch at the destination [design: code reading, not
//! reproduced live]. The first boundary has none before it: it only sets the baseline (in
//! start-up), no frame is counted for it.
//!
//! A frame whose two boundaries saw different phases (a loading screen went up or came down
//! in it, `loading::finish` in `Last` included) crossed a phase change: it is in no frame
//! time, and when slow it is logged as `stress hitch (across a phase change, <from> to
//! <to>): …, from <place> to <place>` with its raw duration, without saying whether the
//! loading or the game took it. Each sample window counts its start-up, hidden and crossing
//! frames and its real length (`startup frames`, `hidden frames`, `phase changes`, `window
//! <ms> ms`, before the place), so a window that held a transition can be told from a clean
//! one, and the window starts again when start-up ends. A line's `t` is its boundary's
//! instant on `Time<Real>`'s clock (seconds from that clock's first update).
//!
//! Start-up (the frames until the start's cells are all in, `Streamer::settled`; the Startup
//! schedules run before the first boundary and are in no frame [design: bevy_app 0.20.1
//! `main_schedule.rs:290-304`]; the first frame measured took 2.5 s in the stress runs of
//! 2026-10-18, by `Time<Real>`'s delta then) is left out of the samples' and the run's
//! frame times and logged once on its own: `stress start-up: <n> frames, <ms> ms, slowest
//! <w> ms`. After it, a frame slower than [`HITCH_MS`] logs `stress hitch: <ms> ms at t <s>
//! s, <k> cells loaded in it (<ms> ms), <a> actors drawn in it (<ms> ms), at <place>`, so a
//! stall can be told from a cell load or from actors coming into view (`readahead::clocked`;
//! actors a cell draws count in both). A frame behind a loading screen
//! (`Loading::unseen_frames`) is not seen: it is left out of the frame times and its stall
//! logged apart, `stress hitch (behind a loading screen): …`, with the count of such frames
//! (and of the frames that crossed a phase change) at the exit's `stress frames:` line
//! [observed 2026-10-27: every `world::stream` stall after a door or teleport on a soak run
//! was behind the screen, and counted as hitches]. The process's memory is the runner's to
//! read (it has the process).

use crate::limbs::ActorRig;
use bevy::audio::AudioSink;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use nvf_play::phys::PhysState;
use std::collections::HashMap;
use std::time::Instant;
use nvf_play::RefVisual;

pub struct StressSamplePlugin;

impl StressSamplePlugin {
    /// The sampling interval, when `NVF_PLAY_TEST_SAMPLE` asks for samples.
    pub fn interval() -> Option<f32> {
        std::env::var("NVF_PLAY_TEST_SAMPLE").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|s| *s > 0.0)
    }
}

impl Plugin for StressSamplePlugin {
    fn build(&self, app: &mut App) {
        let Some(every) = Self::interval() else { return };
        info!("test aid NVF_PLAY_TEST_SAMPLE: a stress sample every {every} s");
        // One frame boundary per frame, in `First`: the frame's time (the sampler's own
        // instants), the state it is judged by and its counts all come from the same two
        // boundaries (issue #1415). After the clock's update only so that a line's `t`
        // has `Time<Real>`'s origin from the first boundary on.
        app.insert_resource(Samples { every, ..default() })
            .add_systems(First, sample.after(bevy::time::TimeSystems))
            .add_systems(Last, (report, census, cache_census));
        if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render.add_systems(bevy::render::Render, render_census.in_set(bevy::render::RenderSystems::Cleanup));
        }
    }
}

/// Every [`CENSUS_SECS`], `stress render: gpu images <n>`: the images the render world
/// holds, to set against the main world's `images` and the GPU allocator's textures
/// (issue #903: allocator textures doubling over a tour while `images` stayed flat).
fn render_census(
    images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
    allocators: Option<Res<bevy::render::material_bind_groups::MaterialBindGroupAllocators>>,
    mut last: Local<Option<std::time::Instant>>,
) {
    // The render world has no clock of its own: real time.
    let now = std::time::Instant::now();
    if last.is_some_and(|t| now.duration_since(t).as_secs_f32() < CENSUS_SECS) {
        return;
    }
    *last = Some(now);
    let mut by_label: HashMap<&str, usize> = HashMap::new();
    for (_, g) in images.iter() {
        *by_label.entry(g.texture_descriptor.label.unwrap_or("(no label)")).or_default() += 1;
    }
    let mut labels: Vec<_> = by_label.into_iter().collect();
    labels.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    let listed: Vec<String> = labels.iter().map(|(l, n)| format!("{l} {n}")).collect();
    info!("stress render: gpu images {}; {}", images.iter().count(), listed.join("; "));
    if let Some(a) = allocators.as_deref().and_then(|a| a.get(&std::any::TypeId::of::<nvf_assets::NifMaterial>())) {
        info!("stress render: nif material slabs {}, resources in them {}", a.slab_count(), a.allocations());
    }
}

/// The caches outside the release sweep (issue #904), for [`cache_census`].
#[derive(bevy::ecs::system::SystemParam)]
struct Caches<'w> {
    materials: Res<'w, Assets<nvf_assets::NifMaterial>>,
    images: Res<'w, Assets<Image>>,
    models: Option<Res<'w, nvf_assets::ModelCache>>,
    textures: Option<Res<'w, nvf_assets::TextureCache>>,
    emittance: Option<Res<'w, crate::emittance::EmittanceState>>,
    faded: Option<Res<'w, crate::effect_shaders::Shaders>>,
    swapped: Option<Res<'w, crate::texture_swap::Swapped>>,
    particles: Option<Res<'w, crate::particles::Particles>>,
    terrain: Option<Res<'w, crate::terrain::TerrainCache>>,
    grass: Option<Res<'w, crate::grass::GrassCache>>,
    trees: Option<Res<'w, crate::tree::TreeCache>>,
    water: Option<Res<'w, crate::water::WaterCache>>,
    sfx: Option<Res<'w, nvf_play::sfx::Sfx>>,
    pcm: Option<Res<'w, Assets<nvf_play::sfx::PcmSound>>>,
    radio: Option<Res<'w, nvf_play::radio::Radio>>,
    music: Option<Res<'w, nvf_play::music::Music>>,
    ambient: Option<Res<'w, nvf_play::ambient::Ambient>>,
}

/// Every [`CENSUS_SECS`] of real time, `stress caches: <cache> <entries> (<idle> idle, <n>
/// images <MB> MB); …`: what the caches the release sweep does not see hold, an entry
/// idle when nothing outside its cache holds it, and the images its materials sample
/// with their GPU bytes (an image two caches share counts in both).
fn cache_census(real: Res<Time<Real>>, c: Caches, mut last: Local<f32>) {
    let now = real.elapsed_secs();
    if now - *last < CENSUS_SECS {
        return;
    }
    *last = now;
    let m = &*c.materials;
    let mut listed: Vec<String> = Vec::new();
    let mut put = |name: &str, s: Option<nvf_assets::release::CacheShare>| {
        if let Some(s) = s {
            listed.push(format!("{name} {}", s.text(&c.images)));
        }
    };
    put("textures", c.textures.as_deref().map(nvf_assets::TextureCache::share));
    put("emittance copies", c.emittance.as_deref().map(|e| e.share(m)));
    put("see-through copies", c.faded.as_deref().map(|e| e.share(m)));
    put("swapped copies", c.swapped.as_deref().map(|e| e.share(m)));
    put("particle files", c.models.as_deref().map(|e| e.particle_share(m)));
    if let Some([placed, shared]) = c.particles.as_deref().map(|p| p.share(m)) {
        put("placed particle systems", Some(placed));
        put("shared particle systems", Some(shared));
    }
    put("terrain materials", c.terrain.as_deref().map(|e| e.share(m)));
    let sounds = match (c.sfx.as_deref(), c.pcm.as_deref()) {
        (Some(s), Some(p)) => {
            let (n, idle, bytes) = s.file_share(p);
            format!("; sound files {n} ({idle} idle, {:.0} MB, {} let go)", bytes as f64 / (1024.0 * 1024.0), s.files_released)
        }
        _ => String::new(),
    };
    let streamed = [
        ("radio files", c.radio.as_deref().map(|r| &r.files)),
        ("music files", c.music.as_deref().map(|m| &m.files)),
        ("ambient files", c.ambient.as_deref().map(|a| &a.files)),
    ];
    let mut sounds = sounds;
    for (name, f) in streamed {
        if let Some(f) = f {
            let (n, idle, bytes) = f.share();
            sounds.push_str(&format!("; {name} {n} ({idle} idle, {:.0} MB, {} let go)", bytes as f64 / (1024.0 * 1024.0), f.released));
        }
    }
    let counts = format!(
        "grass models {}; trees {}; water materials {}{sounds}",
        c.grass.as_deref().map_or(0, crate::grass::GrassCache::models),
        c.trees.as_deref().map_or(0, crate::tree::TreeCache::len),
        c.water.as_deref().map_or(0, crate::water::WaterCache::len)
    );
    info!("stress caches: {}; {counts}", listed.join("; "));
}

/// Seconds between entity censuses ([`census`]).
const CENSUS_SECS: f32 = 30.0;

/// Kinds listed in a census line.
const CENSUS_LISTED: usize = 14;

/// Every [`CENSUS_SECS`] of real time, `stress census: <n> entities; <count> <kind>; …`: the
/// entities by kind, the most first, so an entity count that grows can be told by what
/// grows [observed 2026-10-27: a two-lap soak ended its second lap with 9,900 entities
/// against 1,907, at the same place 9,886 against 1,754]. An entity's kind is the names of
/// the game's own components on it (`nvforever`, `nvf_*`, `fnv_*`), else of Bevy's, the
/// first four, with `(root)` when it has no parent.
fn census(world: &World, mut last: Local<f32>) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    if now - *last < CENSUS_SECS {
        return;
    }
    *last = now;
    let components = world.components();
    let parent = world.component_id::<ChildOf>();
    let mut kinds: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut total = 0usize;
    for a in world.archetypes().iter() {
        let n = a.len() as usize;
        if n == 0 {
            continue;
        }
        total += n;
        let names: Vec<String> = a.components().iter().filter_map(|id| components.get_name(*id)).map(|n| n.to_string()).collect();
        let ours: Vec<&String> = names.iter().filter(|n| n.starts_with("nvforever") || n.starts_with("nvf_") || n.starts_with("fnv_")).collect();
        let pick: Vec<&String> = if ours.is_empty() { names.iter().collect() } else { ours };
        let mut short: Vec<String> = pick.iter().map(|n| crate::profile::short_name(n)).collect();
        short.sort();
        short.truncate(4);
        let rooted = parent.is_some_and(|p| a.contains(p));
        let key = format!("{}{}", short.join("+"), if rooted { "" } else { " (root)" });
        *kinds.entry(key).or_default() += n;
    }
    let mut list: Vec<(String, usize)> = kinds.into_iter().collect();
    list.sort_by_key(|k| std::cmp::Reverse(k.1));
    let shown: Vec<String> = list.iter().take(CENSUS_LISTED).map(|(k, n)| format!("{n} {k}")).collect();
    info!("stress census: {total} entities; {}", shown.join("; "));
}

/// A frame slower than this (milliseconds) after start-up is logged as a hitch.
pub const HITCH_MS: f32 = 250.0;

/// What a frame boundary sees: still in start-up, a frame the player does not see (a loading
/// screen up, or nothing shown yet: `Loading::unseen_frames`), or a frame seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Startup,
    Hidden,
    Seen,
}

impl Phase {
    fn text(self) -> &'static str {
        match self {
            Phase::Startup => "start-up",
            Phase::Hidden => "hidden",
            Phase::Seen => "seen",
        }
    }
}

/// A frame (the time between two boundaries) by the phases its two boundaries saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Interval {
    /// Begun in start-up: start-up's, whatever its end saw.
    Startup,
    Hidden,
    Seen,
    /// The phase changed within it (from, to): neither phase's alone.
    Crossing(Phase, Phase),
}

fn classify(start: Phase, end: Phase) -> Interval {
    match (start, end) {
        (Phase::Startup, _) => Interval::Startup,
        (Phase::Hidden, Phase::Hidden) => Interval::Hidden,
        (Phase::Seen, Phase::Seen) => Interval::Seen,
        (a, b) => Interval::Crossing(a, b),
    }
}

/// What one frame boundary reads, as plain data, so the sampler's judging runs without
/// Bevy (the tests drive it): the boundary's own instant, its `t` on `Time<Real>`'s clock,
/// whether the start's cells are in (`Streamer::settled`, or no streamer) and whether a
/// loading screen is up (`Loading::unseen_frames`), the streamer's cells loaded and their
/// load time so far, actors drawn and their time so far (`readahead::clock`), and the place.
#[derive(Clone, Debug)]
struct Reading {
    at: Instant,
    t: f32,
    settled: bool,
    unseen: bool,
    cells_loaded: usize,
    load_ms: f64,
    actors_drawn: usize,
    actors_ms: f64,
    place: String,
}

/// A frame boundary: what it read and the phase it saw.
#[derive(Clone, Debug)]
struct Boundary {
    phase: Phase,
    read: Reading,
}

/// Milliseconds between two boundaries' instants.
fn elapsed_ms(from: Instant, to: Instant) -> f32 {
    (to.saturating_duration_since(from).as_nanos() as f64 / 1e6) as f32
}

/// One frame, from one boundary to the next: its kind, its time and the cells loaded and
/// actors drawn in it (milliseconds spent on each), every one from those two boundaries.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    interval: Interval,
    ms: f32,
    new_cells: usize,
    new_ms: f64,
    new_actors: usize,
    new_actors_ms: f64,
}

/// The frame between two boundaries: its time is the time between their instants, its kind
/// the phases they saw, its counts the counters' differences. Nothing else goes in.
fn frame(prev: &Boundary, cur: &Boundary) -> Frame {
    let (a, b) = (&prev.read, &cur.read);
    Frame {
        interval: classify(prev.phase, cur.phase),
        ms: elapsed_ms(a.at, b.at),
        new_cells: b.cells_loaded.saturating_sub(a.cells_loaded),
        new_ms: b.load_ms - a.load_ms,
        new_actors: b.actors_drawn.saturating_sub(a.actors_drawn),
        new_actors_ms: b.actors_ms - a.actors_ms,
    }
}

/// One sample window: the seen frames' times, the frames of the other kinds, and its real
/// length in seconds (every frame's).
#[derive(Default, Debug)]
struct Window {
    frames: Vec<f32>,
    startup: usize,
    hidden: usize,
    crossed: usize,
    secs: f32,
}

impl Window {
    fn add(&mut self, interval: Interval, ms: f32) {
        self.secs += ms / 1000.0;
        match interval {
            Interval::Startup => self.startup += 1,
            Interval::Hidden => self.hidden += 1,
            Interval::Seen => self.frames.push(ms),
            Interval::Crossing(..) => self.crossed += 1,
        }
    }

    /// The window's own fields on the sample line, before its place.
    fn fields(&self) -> String {
        format!(
            ", startup frames {}, hidden frames {}, phase changes {}, window {:.0} ms",
            self.startup,
            self.hidden,
            self.crossed,
            self.secs * 1000.0
        )
    }
}

#[derive(Resource, Default)]
struct Samples {
    every: f32,
    /// The start's cells are in: frame times count from the next frame on.
    started: bool,
    /// Start-up's frame times (milliseconds).
    startup: Vec<f32>,
    /// The last frame boundary (none before the first, which is a baseline only).
    last: Option<Boundary>,
    /// After start-up, the frames that drew actors: (frame ms, actors, ms drawing them).
    actor_frames: Vec<(f32, usize, f64)>,
    /// This window, and every seen frame's time (milliseconds).
    window: Window,
    /// The mesh allocator's counts at the last sample (#3057).
    mesh_from: bevy::render::mesh::allocator::NvfMeshAlloc,
    all: Vec<f32>,
    /// After start-up, frames behind a loading screen and frames across a phase change
    /// (both left out of the frame times).
    hidden: usize,
    crossed: usize,
}

impl Samples {
    /// One frame boundary: judges the frame that ended at it against the last boundary and
    /// returns the lines to log (start-up's end, a hitch). The first boundary has none
    /// before it: it only sets the baseline (as the runner's first CPU read, issue #1412),
    /// in start-up, and no frame is counted.
    fn step(&mut self, read: Reading) -> Vec<String> {
        let mut lines = Vec::new();
        let Some(prev) = self.last.take() else {
            self.last = Some(Boundary { phase: Phase::Startup, read });
            return lines;
        };
        let mut startup_ended = false;
        if prev.phase == Phase::Startup {
            self.startup.push(elapsed_ms(prev.read.at, read.at));
            // A start that never settles still ends start-up after a minute.
            if read.settled || self.startup.iter().sum::<f32>() > 60_000.0 {
                self.started = true;
                startup_ended = true;
                let total: f32 = self.startup.iter().sum();
                let slowest = self.startup.iter().copied().fold(0.0, f32::max);
                lines.push(format!("stress start-up: {} frames, {total:.0} ms, slowest {slowest:.2} ms", self.startup.len()));
            }
        }
        let phase = if !self.started {
            Phase::Startup
        } else if read.unseen {
            Phase::Hidden
        } else {
            Phase::Seen
        };
        let cur = Boundary { phase, read };
        let f = frame(&prev, &cur);
        let Frame { ms, new_cells, new_ms, new_actors, new_actors_ms, .. } = f;
        let (t, place) = (cur.read.t, cur.read.place.as_str());
        match f.interval {
            Interval::Startup => {}
            Interval::Hidden => {
                // Behind a loading screen the player sees no frame: not a hitch, not a frame time.
                self.hidden += 1;
                if ms > HITCH_MS {
                    lines.push(format!("stress hitch (behind a loading screen): {ms:.0} ms at t {t:.1} s, {new_cells} cells loaded in it ({new_ms:.0} ms), at {place}"));
                }
            }
            Interval::Seen => {
                self.all.push(ms);
                if new_actors > 0 {
                    self.actor_frames.push((ms, new_actors, new_actors_ms));
                }
                if ms > HITCH_MS {
                    lines.push(format!(
                        "stress hitch: {ms:.0} ms at t {t:.1} s, {new_cells} cells loaded in it ({new_ms:.0} ms), {new_actors} actors drawn in it ({new_actors_ms:.0} ms), at {place}"
                    ));
                }
            }
            Interval::Crossing(from, to) => {
                // A loading screen went up or came down within the frame: its time is the
                // frame's, but which phase took it is not known. Kept apart, raw.
                self.crossed += 1;
                if ms > HITCH_MS {
                    lines.push(format!(
                        "stress hitch (across a phase change, {} to {}): {ms:.0} ms at t {t:.1} s, {new_cells} cells loaded in it ({new_ms:.0} ms), {new_actors} actors drawn in it ({new_actors_ms:.0} ms), from {} to {place}",
                        from.text(),
                        to.text(),
                        prev.read.place
                    ));
                }
            }
        }
        self.window.add(f.interval, ms);
        if startup_ended {
            // The first window after start-up holds only frames after it.
            self.window = Window::default();
        }
        self.last = Some(cur);
        lines
    }
}

/// (mean, 1 % low, worst) of frame times in milliseconds: the 1 % low is the mean of the
/// slowest hundredth of the frames (one frame at least). Zeros with no frames.
pub fn frame_stats(ms: &[f32]) -> (f32, f32, f32) {
    if ms.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mean = ms.iter().sum::<f32>() / ms.len() as f32;
    let mut sorted = ms.to_vec();
    sorted.sort_by(|a, b| b.total_cmp(a));
    let n = (ms.len() / 100).max(1);
    let low = sorted[..n].iter().sum::<f32>() / n as f32;
    (mean, low, sorted[0])
}

/// What is loaded now: the physics world's cells, the placed references drawn (with a
/// model), the actors with a rig, every entity ([`LoadedCounts`]). `refs` leaves out the
/// full-detail ring's draw-only meshes (`nvf_play::DrawOnly`, full_detail.rs), counted
/// apart in `ring_refs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    pub cells: usize,
    pub refs: usize,
    pub actors: usize,
    pub entities: usize,
    pub ring_refs: usize,
}

/// The counts a stress sample's `cells`, `refs drawn`, `actors` and `entities` are, read
/// also by the performance overlay's loaded row (`perf_overlay`). The queries only count
/// when [`LoadedCounts::read`] is called.
#[derive(SystemParam)]
pub struct LoadedCounts<'w, 's> {
    entities: Query<'w, 's, Entity>,
    refs: Query<'w, 's, (), (With<RefVisual>, Without<nvf_play::DrawOnly>)>,
    ring: Query<'w, 's, (), (With<RefVisual>, With<nvf_play::DrawOnly>)>,
    actors: Query<'w, 's, (), With<ActorRig>>,
    phys: Option<Res<'w, PhysState>>,
}

impl LoadedCounts<'_, '_> {
    pub fn read(&self) -> Loaded {
        Loaded {
            cells: self.phys.as_deref().map_or(0, |p| p.cells.len()),
            refs: self.refs.iter().count(),
            actors: self.actors.iter().count(),
            entities: self.entities.iter().count(),
            ring_refs: self.ring.iter().count(),
        }
    }
}

/// The caches a sample counts, and the device whose allocator it reports.
type Cached<'w> = (Option<Res<'w, nvf_assets::ModelCache>>, Option<Res<'w, nvf_assets::TextureCache>>, Option<Res<'w, bevy::render::renderer::RenderDevice>>);

#[allow(clippy::too_many_arguments)]
fn sample(
    real: Res<Time<Real>>,
    mut s: ResMut<Samples>,
    counts: LoadedCounts,
    actors: Query<&ActorRig>,
    globals: Query<&GlobalTransform>,
    audio: Query<(), With<AudioSink>>,
    meshes: Res<Assets<Mesh>>,
    images: Res<Assets<Image>>,
    materials: Res<Assets<nvf_assets::NifMaterial>>,
    cached: Cached,
    phys: Option<Res<PhysState>>,
    combat: Option<Res<nvf_play::combat::Combat>>,
    streamer: Option<Res<crate::world::Streamer>>,
    loading: Option<Res<nvf_play::loading::Loading>>,
) {
    // This frame boundary: its own instant first, then what it sees.
    let at = Instant::now();
    let t = real.first_update().map_or(0.0, |o| at.saturating_duration_since(o).as_secs_f32());
    let (cells_loaded, load_ms) = streamer.as_deref().map_or((0, 0.0), |st| (st.stats.cells_loaded, st.stats.load_ms));
    let (actors_drawn, actors_ms) = crate::readahead::clock();
    let read = Reading {
        at,
        t,
        settled: streamer.as_deref().is_none_or(|st| st.settled),
        unseen: loading.as_deref().is_some_and(nvf_play::loading::Loading::unseen_frames),
        cells_loaded,
        load_ms,
        actors_drawn,
        actors_ms,
        place: crate::crash::place_text(),
    };
    for line in s.step(read) {
        info!("{line}");
    }
    if s.window.secs < s.every {
        return;
    }
    let window = std::mem::take(&mut s.window);
    let (mean, _, worst) = frame_stats(&window.frames);
    let frames = window.frames.len();
    let window = window.fields();
    let mesh_now = bevy::render::mesh::allocator::nvf_mesh_alloc();
    let m = mesh_now.since(&s.mesh_from);
    s.mesh_from = mesh_now;
    let mesh = format!(
        ", mesh alloc {:.1} ms, mesh alloc worst {:.1} ms, meshes written {}, meshes freed {}, slabs made {}, slab mb {:.1}, slab mb copied {:.1}",
        m.ms,
        m.worst_ms,
        m.written,
        m.freed,
        m.slabs_made,
        m.slab_bytes as f64 / 1e6,
        m.copied_bytes as f64 / 1e6
    );
    let nan = actors.iter().filter(|r| r.nodes.first().and_then(|&e| globals.get(e).ok()).is_some_and(|g| !g.translation().is_finite())).count();
    let loaded = counts.read();
    let cells = loaded.cells;
    let (bodies, ragdolls) = phys.as_deref().map_or(("0/0/0".to_string(), 0), |p| {
        let st = p.world.stats();
        let solver = format!(", bodies awake {}, contact pairs {}, manifolds {}, solver contacts {}, joints {}, solver ms {:.0}/{:.0}/{:.0}/{:.0}/{:.0}/{:.0}", st.bodies_awake, st.contact_pairs, st.manifolds, st.solver_contacts, st.joints, st.solver_us[0] as f64 / 1000.0, st.solver_us[1] as f64 / 1000.0, st.solver_us[2] as f64 / 1000.0, st.solver_us[3] as f64 / 1000.0, st.solver_us[4] as f64 / 1000.0, st.solver_us[5] as f64 / 1000.0);
        (format!("{}/{}/{}{solver}", st.bodies_fixed, st.bodies_kinematic, st.bodies_dynamic), p.world.ragdolls().count())
    });
    let gpu = cached.2.as_deref().map_or(String::new(), gpu_memory);
    // The full-detail ring's draw-only meshes, only while it draws any (full_detail.rs).
    let ring = if loaded.ring_refs > 0 { format!(", ring meshes {}", loaded.ring_refs) } else { String::new() };
    info!(
        "stress sample: t {:.1} s, frames {frames}, frame mean {mean:.2} ms, frame worst {worst:.2} ms, entities {}, refs drawn {}, meshes {}, images {}, materials {}, models cached {}, textures cached {}, audio {}, cells {cells}, bodies {bodies}, actors {}, nan actors {nan}, ragdolls {ragdolls}, projectiles {}{ring}{gpu}{mesh}{window}, at {}",
        t,
        loaded.entities,
        loaded.refs,
        meshes.len(),
        images.len(),
        materials.len(),
        cached.0.as_deref().map_or(0, nvf_assets::ModelCache::held),
        cached.1.as_deref().map_or(0, nvf_assets::TextureCache::len),
        audio.iter().count(),
        loaded.actors,
        combat.as_deref().map_or(0, |c| c.in_flight()),
        s.last.as_ref().map_or("", |b| b.read.place.as_str()),
    );
}

/// The GPU allocator's totals as sample fields (`, gpu allocated <MB>, gpu reserved <MB>,
/// gpu blocks <n>`), and its allocations by label, largest first, on a line of their own:
/// reserved beyond allocated is room in blocks kept, allocated growing with the asset
/// counts flat is something still holding GPU memory (issue #903). Empty where the
/// backend gives no report.
fn gpu_memory(device: &bevy::render::renderer::RenderDevice) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let Some(r) = device.wgpu_device().generate_allocator_report() else { return String::new() };
    let mut by_label: HashMap<&str, (u64, usize)> = HashMap::new();
    for a in &r.allocations {
        let e = by_label.entry(if a.name.is_empty() { "(no label)" } else { &a.name }).or_default();
        e.0 += a.size;
        e.1 += 1;
    }
    let mut labels: Vec<_> = by_label.into_iter().collect();
    labels.sort_by_key(|&(_, (bytes, _))| std::cmp::Reverse(bytes));
    let listed: Vec<String> = labels.iter().take(40).map(|(l, (b, n))| format!("{l} {:.0} MB in {n}", *b as f64 / MB)).collect();
    info!("stress gpu labels: {}", listed.join("; "));
    format!(
        ", gpu allocated {:.0}, gpu reserved {:.0}, gpu blocks {}",
        r.total_allocated_bytes as f64 / MB,
        r.total_reserved_bytes as f64 / MB,
        r.blocks.len()
    )
}

fn report(mut exit: MessageReader<AppExit>, s: Res<Samples>) {
    if exit.read().next().is_none() {
        return;
    }
    let (mean, low, worst) = frame_stats(&s.all);
    let fps = if low > 0.0 { 1000.0 / low } else { 0.0 };
    info!(
        "stress frames: {} frames, mean {mean:.2} ms, 1% low {low:.2} ms ({fps:.1} fps), worst {worst:.2} ms; {} behind loading screens left out, {} across a phase change left out",
        s.all.len(),
        s.hidden,
        s.crossed
    );
    let f = &s.actor_frames;
    let (drawn, ms) = f.iter().fold((0, 0.0), |(n, t), &(_, k, m)| (n + k, t + m));
    let most = f.iter().map(|x| x.2).fold(0.0, f64::max);
    let frame_ms: Vec<f32> = f.iter().map(|x| x.0).collect();
    let (mean, _, worst) = frame_stats(&frame_ms);
    let slow = frame_ms.iter().filter(|&&m| m > 100.0).count();
    info!(
        "stress actors: {drawn} drawn in {} frames ({ms:.0} ms, most in a frame {most:.1} ms); those frames mean {mean:.1} ms, worst {worst:.0} ms, {slow} over 100 ms",
        f.len()
    );
}

#[cfg(test)]
mod ring_count_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// #2701: the loaded-reference count (stress samples, the overlay's loaded row) leaves
    /// out the full-detail ring's draw-only meshes, which are counted apart.
    #[test]
    fn draw_only_meshes_are_counted_apart() {
        let mut world = World::new();
        world.spawn(RefVisual(1));
        world.spawn(RefVisual(2));
        world.spawn((RefVisual(3), nvf_play::DrawOnly));
        let l = world.run_system_once(|c: LoadedCounts| c.read()).unwrap();
        assert_eq!((l.refs, l.ring_refs), (2, 1));
        // Every entity counts in the total, ring meshes too (running the system may add its own).
        assert!(l.entities >= 3, "{}", l.entities);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_stats_take_the_slowest_hundredth_as_the_low() {
        assert_eq!(frame_stats(&[]), (0.0, 0.0, 0.0));
        // 198 frames of 10 ms and two of 50 ms: the slowest 1 % (2 frames) are the 50s.
        let mut ms = vec![10.0; 198];
        ms.extend([50.0, 50.0]);
        let (mean, low, worst) = frame_stats(&ms);
        assert!((mean - 10.4).abs() < 1e-4, "{mean}");
        assert_eq!((low, worst), (50.0, 50.0));
        // Fewer than 100 frames: the worst one.
        assert_eq!(frame_stats(&[5.0, 20.0, 8.0]).1, 20.0);
    }

    #[test]
    fn a_frame_is_judged_by_both_its_boundaries() {
        assert_eq!(classify(Phase::Startup, Phase::Startup), Interval::Startup);
        // The frame start-up ended in is start-up's, whatever its end saw.
        assert_eq!(classify(Phase::Startup, Phase::Seen), Interval::Startup);
        assert_eq!(classify(Phase::Startup, Phase::Hidden), Interval::Startup);
        assert_eq!(classify(Phase::Hidden, Phase::Hidden), Interval::Hidden);
        assert_eq!(classify(Phase::Seen, Phase::Seen), Interval::Seen);
        // A loading screen came down (issue #1415: a 674 ms frame then logged as a seen
        // hitch at the destination) or went up within the frame.
        assert_eq!(classify(Phase::Hidden, Phase::Seen), Interval::Crossing(Phase::Hidden, Phase::Seen));
        assert_eq!(classify(Phase::Seen, Phase::Hidden), Interval::Crossing(Phase::Seen, Phase::Hidden));
    }

    #[test]
    fn a_window_whose_phase_changes_is_flagged_and_keeps_only_seen_frames() {
        // Hidden frames behind the screen, the long frame the screen came down in, then
        // seen frames: the long frame is in no frame time, the window says it crossed.
        let mut w = Window::default();
        let frames = [(Phase::Hidden, Phase::Hidden, 335.0), (Phase::Hidden, Phase::Seen, 674.0), (Phase::Seen, Phase::Seen, 16.0), (Phase::Seen, Phase::Seen, 18.0)];
        for (a, b, ms) in frames {
            w.add(classify(a, b), ms);
        }
        assert_eq!(w.frames, vec![16.0, 18.0]);
        assert_eq!((w.startup, w.hidden, w.crossed), (0, 1, 1));
        assert_eq!(frame_stats(&w.frames).2, 18.0);
        assert_eq!(w.fields(), ", startup frames 0, hidden frames 1, phase changes 1, window 1043 ms");
        // A steady window keeps every frame and says it crossed nothing.
        let mut steady = Window::default();
        for ms in [16.0, 300.0, 17.0] {
            steady.add(classify(Phase::Seen, Phase::Seen), ms);
        }
        assert_eq!(steady.frames, vec![16.0, 300.0, 17.0]);
        assert_eq!(steady.fields(), ", startup frames 0, hidden frames 0, phase changes 0, window 333 ms");
    }

    /// A boundary `ms` milliseconds after `t0`: the start's cells in, nothing loaded or drawn.
    fn read(t0: Instant, ms: u64, unseen: bool, place: &str) -> Reading {
        Reading {
            at: t0 + std::time::Duration::from_millis(ms),
            t: ms as f32 / 1000.0,
            settled: true,
            unseen,
            cells_loaded: 0,
            load_ms: 0.0,
            actors_drawn: 0,
            actors_ms: 0.0,
            place: place.to_string(),
        }
    }

    #[test]
    fn a_long_frame_a_loading_screen_changed_in_is_the_crossing_never_a_seen_hitch() {
        // Issue #1415, the review of its first fix: a long main-thread frame lowers the
        // loading screen in `Last`. Under pipelined rendering `Time<Real>`'s delta at the
        // next boundary can be an older, short render interval, and the long one arrive a
        // boundary later, when both boundaries see the screen down: a seen hitch at the
        // destination. With the sampler's own instants the long interval is the frame
        // between the boundary before the screen came down and the one after.
        let t0 = Instant::now();
        let mut s = Samples::default();
        let mut lines = Vec::new();
        // The baseline, then start-up ends with the start's screen still up.
        lines.extend(s.step(read(t0, 0, true, "start")));
        assert_eq!(s.step(read(t0, 20, true, "start")), vec!["stress start-up: 1 frames, 20 ms, slowest 20.00 ms".to_string()]);
        // A hidden frame, then the long frame the screen came down in, three cells loaded in it.
        lines.extend(s.step(read(t0, 36, true, "start")));
        let mut down = read(t0, 710, false, "destination");
        (down.cells_loaded, down.load_ms) = (3, 250.0);
        let crossing = s.step(down);
        assert_eq!(
            crossing,
            vec![
                "stress hitch (across a phase change, hidden to seen): 674 ms at t 0.7 s, 3 cells loaded in it (250 ms), 0 actors drawn in it (0 ms), from start to destination"
                    .to_string()
            ]
        );
        // The next boundaries, where the old pairing could have put the 674 ms: 16 ms each.
        for ms in [726, 742] {
            let mut r = read(t0, ms, false, "destination");
            (r.cells_loaded, r.load_ms) = (3, 250.0);
            lines.extend(s.step(r));
        }
        // And the other way: the screen goes up in a long frame (a door in `Update`).
        let mut up = read(t0, 1142, true, "destination");
        (up.cells_loaded, up.load_ms) = (3, 250.0);
        let rising = s.step(up);
        assert_eq!(rising.len(), 1);
        assert!(rising[0].starts_with("stress hitch (across a phase change, seen to hidden): 400 ms at t 1.1 s, 0 cells loaded in it (0 ms)"), "{}", rising[0]);
        let mut behind = read(t0, 1158, true, "destination");
        (behind.cells_loaded, behind.load_ms) = (3, 250.0);
        lines.extend(s.step(behind));
        // Nothing else logged: no ordinary hitch, no hitch behind the screen.
        assert!(lines.is_empty(), "{lines:?}");
        // The seen frames are the two short ones; the long ones are counted apart.
        assert_eq!(s.all, vec![16.0, 16.0]);
        assert_eq!((s.hidden, s.crossed), (2, 2));
        assert_eq!(frame_stats(&s.all).2, 16.0);
        assert_eq!(s.window.frames, vec![16.0, 16.0]);
        assert_eq!(s.window.fields(), ", startup frames 0, hidden frames 2, phase changes 2, window 1138 ms");
        // For contrast, the old pairing: the same boundaries' kinds with the render-fed
        // deltas (16 ms at the screen's boundary, 674 ms one later) puts the long frame in
        // the seen frames.
        let mut old = Window::default();
        old.add(classify(Phase::Hidden, Phase::Seen), 16.0);
        old.add(classify(Phase::Seen, Phase::Seen), 674.0);
        assert_eq!(old.frames, vec![674.0]);
    }

    #[test]
    fn the_first_boundary_is_a_baseline_and_ordinary_hitches_stay() {
        let t0 = Instant::now();
        let mut s = Samples::default();
        let unsettled = |ms| Reading { settled: false, ..read(t0, ms, false, "start") };
        // The first boundary: no frame before it, nothing counted.
        assert!(s.step(unsettled(0)).is_empty());
        assert!(s.startup.is_empty());
        assert_eq!(s.last.as_ref().map(|b| b.phase), Some(Phase::Startup));
        assert!(s.step(unsettled(2500)).is_empty());
        assert_eq!(s.startup, vec![2500.0]);
        // Start-up ends at the boundary that sees the start settled; the window starts again.
        assert_eq!(s.step(read(t0, 2530, false, "start")), vec!["stress start-up: 2 frames, 2530 ms, slowest 2500.00 ms".to_string()]);
        assert!(s.all.is_empty());
        assert_eq!(s.window.secs, 0.0);
        assert_eq!(s.last.as_ref().map(|b| b.phase), Some(Phase::Seen));
        // A seen frame, and a slow seen frame a cell loaded in: still an ordinary hitch.
        assert!(s.step(read(t0, 2546, false, "start")).is_empty());
        let mut slow = read(t0, 2846, false, "Goodsprings");
        (slow.cells_loaded, slow.load_ms) = (1, 120.0);
        assert_eq!(
            s.step(slow),
            vec!["stress hitch: 300 ms at t 2.8 s, 1 cells loaded in it (120 ms), 0 actors drawn in it (0 ms), at Goodsprings".to_string()]
        );
        assert_eq!(s.all, vec![16.0, 300.0]);
        assert_eq!(s.window.fields(), ", startup frames 0, hidden frames 0, phase changes 0, window 316 ms");
        // A start that never settles still ends start-up after a minute.
        let mut never = Samples::default();
        never.step(unsettled(0));
        assert_eq!(never.step(unsettled(61_000)), vec!["stress start-up: 1 frames, 61000 ms, slowest 61000.00 ms".to_string()]);
    }
}
