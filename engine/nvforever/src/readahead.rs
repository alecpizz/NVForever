//! Actors made ready before they come into range (docs/design/streaming-hitches.md §2).
//!
//! When the exterior cell the loaded grid is around changes, the actors of the band just
//! outside it (the ring between the loaded `(2r+1)²` cells and `(2r+3)²`: placed NPCs and
//! creatures there, and residents whose walkers stand there) are listed, their appearance
//! resolved and their parts, skeleton and weapon model read and parsed on a worker thread
//! (`ActorCtx::parts_ready`). What has been read is then prepared on this thread (meshes,
//! materials, textures) within [`PREPARE_BUDGET`] a frame, the NPCs' FaceGen `.egm` and
//! `.egt` files read on a worker thread, and once a skeleton is prepared the clip sets and weapon stance its actor needs
//! are loaded from their KF files on a worker thread (`actors::Anims::warm`). So an actor
//! walking into the loaded cells, or a cell of them loading, finds what it is drawn from
//! ready and is drawn at once without reading or preparing it in that frame [engineering
//! choice].
//!
//! An actor is drawn only once its parts are prepared (behind a loading screen: read, so
//! they can be prepared without reading a file) and its current stance and FaceGen files
//! have been read (missing files count as ready) ([`ready_to_draw`], used by
//! `nav::show_actor` and `world::load_cell` after the start); one that was not read ahead
//! waits a few frames for the worker and the queue (#1429).
//!
//! The clock: time spent drawing actors ([`clocked`]), which the stress hitch lines
//! report (`stress_sample.rs`), so a slow frame can be told from one that spawned actors.
//! At the exit `actor read-ahead summary: …` (smoke tests read it). Test aid
//! `NVF_PLAY_TEST_NO_READ_AHEAD=1` turns the read-ahead off (to measure what it saves).

use crate::actors::{ActorPose, ActorWorld};
use crate::world::{Streamer, WorldData};
use bevy::prelude::*;
use fnv_world::placed::{base_object, placement};
use bevy::tasks::{futures::check_ready, Task};
use fnv_actor::locomotion::Body;
use fnv_actor::weapon::AnimType;
use nvf_assets::actor::{ActorCtx, ActorParts, ClipReader, PartState};
use nvf_assets::model::ModelCtx;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Time a frame may spend resolving listed actors and preparing their parts read ahead
/// [engineering choice: a quarter of a 60 fps frame; one part can run over it].
pub const PREPARE_BUDGET: Duration = Duration::from_millis(4);

/// Actors drawn so far and the time it took (nanoseconds).
static DRAWN: AtomicUsize = AtomicUsize::new(0);
static DRAW_NS: AtomicU64 = AtomicU64::new(0);

/// Run `f`, which draws one actor, counting it and its time on the actor clock.
pub fn clocked<T>(f: impl FnOnce() -> T) -> T {
    let t0 = Instant::now();
    let out = f();
    DRAW_NS.fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
    DRAWN.fetch_add(1, Ordering::Relaxed);
    out
}

/// Actors drawn so far and the milliseconds it took.
pub fn clock() -> (usize, f64) {
    (DRAWN.load(Ordering::Relaxed), DRAW_NS.load(Ordering::Relaxed) as f64 / 1e6)
}

/// Actors asked for ([`ready_to_draw`]) and found not ready: since when, and when last
/// asked for.
static WAITING: Mutex<Option<HashMap<u32, (Instant, Instant)>>> = Mutex::new(None);
/// An actor asked for within this long and not ready is still wanted on screen
/// ([`actors_waiting`]): its drawer asks every frame it is in view [engineering choice:
/// a few frames at the slowest loading-screen frame rate].
const STILL_WANTED: Duration = Duration::from_millis(750);

/// Actors still wanted on screen whose parts are not read yet (asked for within
/// [`STILL_WANTED`]); a loading screen waits for them (`nvf_play::loading::Loading::actors_busy`).
pub fn actors_waiting() -> usize {
    let waiting = WAITING.lock().expect("read-ahead waits");
    waiting.as_ref().map_or(0, |w| w.values().filter(|(_, last)| last.elapsed() < STILL_WANTED).count())
}
/// Actors ready at the first ask, and after waiting; the longest wait (microseconds).
static AT_ONCE: AtomicUsize = AtomicUsize::new(0);
static AFTER_WAIT: AtomicUsize = AtomicUsize::new(0);
static LONGEST_WAIT_US: AtomicU64 = AtomicU64::new(0);
/// Frames an actor's cached equipment is kept without being asked for (`read_ahead`)
/// [engineering choice: waiting actors ask every frame].
const STILL_WANTED_FRAMES: u64 = 30;
/// A wait longer than this is an actor that left view before it was drawn.
const STALE_WAIT: Duration = Duration::from_secs(20);

/// What an actor is drawn from (`actors::spawn`, `spawn_creature`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Its parts' models and its weapon's model (archive paths).
    pub parts: Vec<String>,
    pub skeleton: String,
    pub body: Body,
    /// The weapon stance it holds (`actors::arm`): its weapon's, else its fists'; none for
    /// a creature.
    pub prefix: Option<&'static str>,
    /// An NPC's FaceGen files (`facegen.rs`): the `.egm` beside each part, the head's
    /// `.egt` and the race's body and hand `.egt` (as `FaceGenCache::morph`,
    /// `face_offset` and `skin_offset` ask for them).
    pub facegen: Vec<String>,
    /// Parts morphed by FaceGen; prepared shape names may add fallback EGMs.
    pub morphs: Vec<String>,
    /// Textures its materials take beyond its parts' own (`actors::part_uses`): a part's
    /// given texture and the race's body and hand skin, which worn models' skin shapes
    /// take too (`ActorCtx::textures_ready`).
    pub textures: Vec<String>,
}

/// What actor `reference` (base `base`) is drawn from. `None`: it cannot be resolved
/// (spawning it says why). The ring uses the equipment available now; readiness asks
/// again before spawning, since the simulation may have acquired or changed it since
/// the ring was listed [design]. Initial cell spawns and explicit respawns can still
/// read missing parts synchronously through `ActorCtx`, as before.
pub fn actor_plan(world: &ActorWorld, sim: Option<&fnv_sim::World<'_>>, reference: u32, base: u32, creature: bool) -> Option<Plan> {
    let equipment = if creature { None } else { crate::outfits::Equipment::of(world, sim, reference) };
    plan_wearing(world, equipment, reference, base, creature)
}

/// [`actor_plan`] with the simulation's equipment already looked up (an NPC's; `None`
/// for a creature or an actor the simulation holds no inventory for).
fn plan_wearing(world: &ActorWorld, equipment: Option<crate::outfits::Equipment>, reference: u32, base: u32, creature: bool) -> Option<Plan> {
    if creature {
        let c = world.resolve_creature(base, reference).ok()?;
        Some(Plan { parts: c.parts.iter().map(|p| p.model.clone()).collect(), skeleton: c.skeleton, body: Body::Male, prefix: None, facegen: Vec::new(), morphs: Vec::new(), textures: Vec::new() })
    } else {
        let a = crate::outfits::appearance(world, base, reference, equipment.as_ref()).ok()?;
        let w = a.weapon.and_then(|id| world.actors.weapons.get(&id)).filter(|_| world.weapon_mode != crate::weapons::WeaponMode::None);
        let model = w.and_then(|w| w.model_for(false, a.weapon_mods)).map(fnv_actor::resolve::mesh_path);
        let held = w.filter(|_| model.is_some()).and_then(|w| w.anim_type.as_deref().and_then(AnimType::from_name));
        let parts = a.parts.iter().map(|p| p.model.clone()).chain(model).collect();
        let body = if a.female { Body::Female } else { Body::Male };
        let mut facegen = Vec::new();
        if world.facegen_enabled {
            use crate::facegen::sibling;
            facegen.extend(a.parts.iter().map(|p| sibling(&p.model, "egm")));
            facegen.extend(a.parts.iter().filter(|p| p.role == "head:head").map(|p| sibling(&p.model, "egt")));
            let body_egt = a.body_egt.clone();
            facegen.extend(body_egt.iter().flat_map(|e| [Some(e.clone()), fnv_actor::resolve::hand_egt(e, true)]).flatten());
        }
        let morphs = if world.facegen_enabled && a.shape_facegen(world.race_facegen).is_some() { a.parts.iter().map(|p| p.model.clone()).collect() } else { Vec::new() };
        let mut textures: Vec<String> = a.parts.iter().filter_map(|p| p.texture.clone()).chain(a.body_texture.clone()).chain(a.hand_texture.clone()).collect();
        textures.sort();
        textures.dedup();
        Some(Plan { parts, skeleton: a.skeleton, body, prefix: Some(held.unwrap_or(AnimType::HandToHand).prefix()), facegen, morphs, textures })
    }
}

/// Whether actor `reference` can be drawn from its current complete plan: models read
/// or prepared, animation warm-up finished, and FaceGen files read or known absent.
/// Missing work is queued even if an older appearance was already prefetched [design].
/// An actor that cannot be resolved counts as
/// ready: spawning it says why. Counts for the summary whether it was ready at once.
#[allow(clippy::too_many_arguments)]
pub fn ready_to_draw(world: &ActorWorld, sim: Option<&fnv_sim::World<'_>>, ctx: &mut ActorCtx, ahead: &mut ReadAhead, reference: u32, base: u32, creature: bool) -> bool {
    // The simulation's equipment is kept until the inventory changes: this is asked
    // every frame the actor waits (`outfits::EquipmentCache`).
    let equipment = if creature { None } else { ahead.equipment.of(world, sim, reference) };
    let ready = plan_wearing(world, equipment, reference, base, creature).is_none_or(|mut p| {
        let ready = ahead.request_plan(world, ctx, &mut p, creature);
        if !ready {
            ahead.want(&p);
        }
        ready
    });
    let mut waiting = WAITING.lock().expect("read-ahead waits");
    let waiting = waiting.get_or_insert_with(HashMap::new);
    if !ready {
        let now = Instant::now();
        waiting.entry(reference).or_insert((now, now)).1 = now;
    } else if let Some((since, _)) = waiting.remove(&reference) {
        AFTER_WAIT.fetch_add(1, Ordering::Relaxed);
        // One asked for long ago left view meanwhile: not a wait for its parts.
        if since.elapsed() < STALE_WAIT {
            LONGEST_WAIT_US.fetch_max(since.elapsed().as_micros() as u64, Ordering::Relaxed);
        }
    } else {
        AT_ONCE.fetch_add(1, Ordering::Relaxed);
    }
    ready
}

/// One piece of read-ahead work, done within the frame's budget.
type AnimsKey = (String, Body, Option<&'static str>);

enum Job {
    /// Resolve the actor and start reading what it is drawn from. `door`: listed for a
    /// load door's destination (seamless doors, `door_ahead.rs`), kept indoors.
    Actor { reference: u32, base: u32, creature: bool, door: bool },
    /// Prepare the part (or with `true` the skeleton) at this path once it has been read.
    Prepare(String, bool),
    /// Read the FaceGen file (`.egm`, `.egt`) at this path on a worker thread.
    FaceGen(String),
    /// Once the skeleton at this path is prepared, load the clip sets and stance an actor
    /// of it needs on a worker thread (`actors::Anims::warm`).
    Anims { skeleton: String, body: Body, creature: bool, prefix: Option<&'static str> },
}

#[derive(Resource, Default)]
pub struct ReadAhead {
    /// Off (`NVF_PLAY_TEST_NO_READ_AHEAD`): actors are read only when they are drawn.
    off: bool,
    /// A frame would not be seen (a loading screen is up, `Loading::unseen_frames`; set
    /// by [`read_ahead`] each frame): an actor whose parts are read may be drawn and its
    /// parts prepared in that frame. Otherwise it waits until the budgeted queue has
    /// prepared them (#1429).
    unseen: bool,
    /// The cell the last ring was listed around.
    centre: Option<(i32, i32)>,
    jobs: VecDeque<Job>,
    /// Paths queued to prepare (not twice).
    queued: HashSet<String>,
    /// Clip sets and stances queued to load (not twice).
    anims_queued: HashSet<AnimsKey>,
    /// Keep tasks until all warm-up work has finished, even if some cache entries
    /// became visible earlier. Readiness waits for completion, not just task launch.
    /// The warm-up and FaceGen tasks stay polled with `check_ready`, not detached
    /// (#1359, decided): readiness needs to know a warm-up finished (`anims_queued`
    /// is released only then) and a FaceGen task returns the file read. A detached task
    /// that panicked would never release its key, so its actor would wait forever and a
    /// loading screen waiting on [`actors_waiting`] would never close; polled, the panic
    /// is resumed on the main thread when the task is polled (bevy's `Task`), as the same
    /// warm-up run synchronously by `actors::spawn` would panic there [design].
    anims: Vec<(AnimsKey, Task<()>)>,
    anims_loaded: usize,
    /// FaceGen files being read on a worker thread, and how many were read.
    facegen: Vec<(String, Task<crate::facegen::FaceGenFile>)>,
    facegen_read: usize,
    /// The simulation's equipment of the actors waiting to be drawn ([`ready_to_draw`]).
    equipment: crate::outfits::EquipmentCache,
    /// What actors waiting to be drawn asked for since the last [`read_ahead`]: parts,
    /// skeletons and FaceGen files (normalised paths), and clip sets. On going indoors
    /// the work the exterior ring queued that no one waits for is dropped (#1359).
    wanted: HashSet<String>,
    anims_wanted: HashSet<AnimsKey>,
    /// Whether the last [`read_ahead`] ran with an interior loaded.
    indoors: bool,
    /// Actors listed for doors' destinations ([`ReadAhead::list_for_door`]).
    door_listed: HashSet<u32>,
    rings: usize,
    actors_listed: usize,
    parts_read: usize,
    parts_prepared: usize,
    /// Frames that did read-ahead work, their total and the slowest (milliseconds).
    busy_frames: usize,
    busy_ms: f64,
    slowest_ms: f64,
}

impl ReadAhead {
    fn collect_anims(&mut self) {
        self.anims.retain_mut(|(key, task)| {
            if check_ready(task).is_some() {
                self.anims_queued.remove(key);
                self.anims_loaded += 1;
                false
            } else {
                true
            }
        });
    }

    /// Note what a waiting actor's plan needs, so going indoors keeps that work.
    fn want(&mut self, plan: &Plan) {
        self.wanted.extend(plan.parts.iter().chain([&plan.skeleton]).map(|p| fnv_vfs::normalize(p)));
        self.wanted.extend(plan.facegen.iter().cloned());
        self.anims_wanted.insert((fnv_vfs::normalize(&plan.skeleton), plan.body, plan.prefix));
    }

    /// Going indoors: the exterior ring's actors are not drawn there, so the work queued
    /// for them is dropped, except what an actor waiting to be drawn asked for (an
    /// interior's own, or one drawn behind the loading screen). A dropped path or clip
    /// set is no longer marked queued, so a later ask queues it again rather than waiting
    /// on a job that is gone [design]. Tasks already started finish.
    fn drop_exterior_work(&mut self) -> usize {
        let before = self.jobs.len();
        let (wanted, anims_wanted) = (&self.wanted, &self.anims_wanted);
        let (queued, anims_queued) = (&mut self.queued, &mut self.anims_queued);
        self.jobs.retain(|j| {
            let keep = match j {
                Job::Actor { door, .. } => *door,
                Job::Prepare(path, _) | Job::FaceGen(path) => wanted.contains(path),
                Job::Anims { skeleton, body, prefix, .. } => anims_wanted.contains(&(skeleton.clone(), *body, *prefix)),
            };
            if !keep {
                match j {
                    Job::Actor { .. } => {}
                    Job::Prepare(path, _) | Job::FaceGen(path) => {
                        queued.remove(path);
                    }
                    Job::Anims { skeleton, body, prefix, .. } => {
                        anims_queued.remove(&(skeleton.clone(), *body, *prefix));
                    }
                }
            }
            keep
        });
        before - self.jobs.len()
    }

    /// List actors for a load door's destination (seamless doors, `door_ahead.rs`):
    /// `(reference, base, creature)`, not listed again while their job waits. Returns how
    /// many were listed now.
    pub fn list_for_door(&mut self, actors: impl IntoIterator<Item = (u32, u32, bool)>) -> usize {
        if self.off {
            return 0;
        }
        let mut n = 0;
        for (reference, base, creature) in actors {
            if self.door_listed.insert(reference) {
                self.jobs.push_back(Job::Actor { reference, base, creature, door: true });
                n += 1;
            }
        }
        self.actors_listed += n;
        n
    }

    /// Forget which actors were listed for doors (the doors near the player changed: an
    /// actor listed again is resolved again, with what it wears now).
    pub fn forget_door_actors(&mut self) {
        self.door_listed.clear();
    }

    pub fn from_env() -> ReadAhead {
        let off = std::env::var("NVF_PLAY_TEST_NO_READ_AHEAD").is_ok_and(|v| v.trim() == "1");
        if off {
            info!("test aid NVF_PLAY_TEST_NO_READ_AHEAD: actors are not read ahead");
        }
        ReadAhead { off, ..default() }
    }

    /// Refresh every dependency of the appearance wanted now, not just the models
    /// listed when the actor first entered the ring [design]. Also used by ring jobs.
    fn request_plan(&mut self, world: &ActorWorld, ctx: &mut ActorCtx, plan: &mut Plan, creature: bool) -> bool {
        let read = ctx.parts_ready(plan.parts.iter().chain([&plan.skeleton]).map(String::as_str));
        if self.off {
            return read;
        }
        // Behind a loading screen the frame is not seen: read in the frame it is drawn.
        let textures = ctx.textures_ready(plan.textures.iter().map(String::as_str)) || self.unseen;
        // A known missing/broken skeleton cannot spawn; do not wait forever for clips
        // that cannot be bound. `spawn` will report the unresolved actor [design].
        if ctx.cache.part_state(&plan.skeleton) == PartState::Prepared && ctx.skeleton(&plan.skeleton).is_none() {
            return true;
        }
        let mut morphs_ready = true;
        let cache = world.facegen.lock().expect("facegen cache");
        for model in &plan.morphs {
            if cache.needs_shape_egms(model) {
                if ctx.cache.part_state(model) == PartState::Prepared {
                    if let Some(part) = ctx.part(model) {
                        plan.facegen.extend(part.shapes.iter().filter_map(|s| s.name.as_deref()).map(|name| crate::facegen::shape_sibling(model, name, "egm")));
                    }
                } else {
                    // Shape-specific EGM names are available after budgeted preparation.
                    morphs_ready = false;
                }
            }
        }
        let ready = self.queue_plan(plan, creature, world.pose == ActorPose::Idle,
            world.anims.loaded(&plan.skeleton, plan.body, plan.prefix),
            |p| ctx.cache.part_state(p), |p| cache.has(p));
        read && textures && morphs_ready && ready
    }

    /// Queue missing work once and check the whole plan. Separated from asset access
    /// so refreshed plans, missing files and in-flight work can be tested without data.
    fn queue_plan(
        &mut self,
        plan: &Plan,
        creature: bool,
        animated: bool,
        anims_ready: bool,
        mut part_state: impl FnMut(&str) -> PartState,
        mut facegen_ready: impl FnMut(&str) -> bool,
    ) -> bool {
        let mut ready = true;
        if animated {
            let key = (fnv_vfs::normalize(&plan.skeleton), plan.body, plan.prefix);
            if !anims_ready && self.anims_queued.insert(key.clone()) {
                self.jobs.push_back(Job::Anims { skeleton: key.0.clone(), body: plan.body, creature, prefix: plan.prefix });
            }
            ready &= anims_ready && !self.anims_queued.contains(&key);
        }
        for path in &plan.facegen {
            let loaded = facegen_ready(path);
            ready &= loaded;
            if !loaded && self.queued.insert(path.clone()) {
                self.jobs.push_back(Job::FaceGen(path.clone()));
            }
        }
        for (path, skeleton) in plan.parts.iter().map(|p| (p, false)).chain([(&plan.skeleton, true)]) {
            let path = fnv_vfs::normalize(path);
            let state = part_state(&path);
            // In play an actor waits for its parts to be prepared too: preparing them
            // (meshes, materials, GPU textures) in the frame it is drawn cost a resident
            // that came into an interior after the start 15.6 ms in one frame
            // [observed 2026-11-03, #1429, Sierra Madre lobby]. The queue prepares them
            // within `PREPARE_BUDGET` a frame [engineering choice: it is drawn a few
            // frames later]. Behind a loading screen the frame is not seen.
            ready &= state == PartState::Prepared || (self.unseen && state == PartState::Read);
            if state != PartState::Prepared && self.queued.insert(path.clone()) {
                self.parts_read += 1;
                self.jobs.push_back(Job::Prepare(path, skeleton));
            }
        }
        ready
    }
}

/// The exterior cells in the band just outside the loaded `(2r+1)²` grid around `centre`.
fn ring(centre: (i32, i32), r: i32) -> HashSet<(i32, i32)> {
    let n = r + 1;
    (-n..=n).flat_map(|dx| (-n..=n).map(move |dy| (dx, dy))).filter(|(dx, dy)| dx.abs() > r || dy.abs() > r).map(|(dx, dy)| (centre.0 + dx, centre.1 + dy)).collect()
}

/// List the ring's actors when the loaded grid moves, and work through the jobs within
/// the frame's budget (runs after `world::stream`).
#[allow(clippy::too_many_arguments)]
pub fn read_ahead(
    mut ahead: ResMut<ReadAhead>,
    streamer: Res<Streamer>,
    world: Res<WorldData>,
    actor_world: Option<Res<ActorWorld>>,
    sim: Option<Res<nvf_play::sim::SimState>>,
    residents: Option<Res<crate::residents::Residents>>,
    loading: Option<Res<nvf_play::loading::Loading>>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
) {
    let unseen = loading.as_deref().is_some_and(|l| l.unseen_frames());
    if ahead.unseen != unseen {
        ahead.unseen = unseen;
    }
    let Some(aw) = actor_world.as_deref().filter(|_| !ahead.off) else { return };
    let t0 = Instant::now();
    let ahead = &mut *ahead;
    // Outdoors, once the start's cells are in.
    let centre = streamer.centre.filter(|_| streamer.interior.is_none() && streamer.settled);
    if centre.is_some() && centre != ahead.centre {
        let cells = ring(centre.unwrap_or_default(), streamer.radius);
        let mut listed = HashSet::new();
        for &g in &cells {
            let placed = world.grid_cell(g).map(|c| c.refs.as_slice()).unwrap_or_default().iter().chain(world.persistent_by_grid.get(&g).into_iter().flatten());
            for &r in placed {
                let Some(p) = placement(world.lo, world.schema, r).filter(|p| !p.deleted() && world.enabled(p, 0)) else { continue };
                let Some(b) = p.base.and_then(|b| base_object(world.lo, world.schema, b)).filter(|b| matches!(&b.sig, b"NPC_" | b"CREA")) else { continue };
                if listed.insert(r) {
                    ahead.jobs.push_back(Job::Actor { reference: r, base: b.form_id, creature: &b.sig == b"CREA", door: false });
                }
            }
        }
        for (r, base, creature) in residents.as_deref().map(|res| res.undrawn_in(world.world, &cells)).unwrap_or_default() {
            if listed.insert(r) {
                ahead.jobs.push_back(Job::Actor { reference: r, base, creature, door: false });
            }
        }
        ahead.rings += 1;
        ahead.actors_listed += listed.len();
        let (x, y) = centre.unwrap_or_default();
        debug!("actor read-ahead: around cell {x},{y}: {} actors in the {} cells of the ring", listed.len(), cells.len());
    }
    let indoors = streamer.interior.is_some();
    if indoors && !ahead.indoors {
        let dropped = ahead.drop_exterior_work();
        if dropped > 0 {
            debug!("actor read-ahead: indoors: {dropped} job(s) queued for the exterior ring dropped");
        }
    } else if indoors {
        // Unresolved exterior actors listed since (none are listed indoors but a door's).
        ahead.jobs.retain(|j| matches!(j, Job::Actor { door: true, .. }) || !matches!(j, Job::Actor { .. }));
    }
    ahead.indoors = indoors;
    ahead.wanted.clear();
    ahead.anims_wanted.clear();
    // Waiting actors ask every frame; one not asked for in a few frames was drawn or left view.
    ahead.equipment.next_frame(STILL_WANTED_FRAMES);
    ahead.centre = centre;
    // FaceGen files read: into the cache (unless read here meanwhile).
    if !ahead.facegen.is_empty() {
        let mut cache = aw.facegen.lock().expect("facegen cache");
        let mut done = Vec::new();
        ahead.facegen.retain_mut(|(path, task)| match check_ready(task) {
            Some(file) => {
                cache.offer(path.clone(), file);
                done.push(std::mem::take(path));
                false
            }
            None => true,
        });
        ahead.facegen_read += done.len();
        for path in done {
            ahead.queued.remove(&path);
        }
    }
    ahead.collect_anims();
    if ahead.jobs.is_empty() {
        return;
    }
    // Through the queue once at most: a part still being read goes to the back.
    let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
    let mut left = ahead.jobs.len();
    while left > 0 && t0.elapsed() < PREPARE_BUDGET {
        left -= 1;
        let Some(job) = ahead.jobs.pop_front() else { break };
        match job {
            Job::Actor { reference, base, creature, .. } => {
                let shared = sim.as_deref().map(|s| &s.world);
                let equipment_world = residents.as_deref().map_or(shared, |r| r.prefetch_world(reference, shared));
                let Some(mut plan) = actor_plan(aw, equipment_world, reference, base, creature) else { continue };
                ahead.request_plan(aw, &mut ctx, &mut plan, creature);
            }
            Job::Prepare(path, skel) => match ctx.cache.part_state(&path) {
                PartState::Reading => ahead.jobs.push_back(Job::Prepare(path, skel)),
                PartState::Read => {
                    ahead.queued.remove(&path);
                    ahead.parts_prepared += 1;
                    if skel {
                        ctx.skeleton(&path);
                    } else {
                        ctx.part(&path);
                    }
                }
                // Prepared meanwhile (its actor was drawn), or read and dropped unused.
                PartState::Prepared | PartState::Unknown => {
                    ahead.queued.remove(&path);
                }
            },
            Job::FaceGen(path) => {
                if aw.facegen.lock().expect("facegen cache").has(&path) {
                    ahead.queued.remove(&path);
                    continue;
                }
                let (data, job) = ((*ctx.models.data).clone(), path.clone());
                let task = nvf_assets::read_pool::read_pool().spawn(async move { crate::facegen::read_file(&data, &job) });
                ahead.facegen.push((path, task));
            }
            Job::Anims { skeleton, body, creature, prefix } => match ctx.cache.part_state(&skeleton) {
                PartState::Reading | PartState::Read => ahead.jobs.push_back(Job::Anims { skeleton, body, creature, prefix }),
                PartState::Prepared => {
                    let key = (fnv_vfs::normalize(&skeleton), body, prefix);
                    let Some(skel) = ctx.skeleton(&skeleton) else {
                        ahead.anims_queued.remove(&key);
                        continue;
                    };
                    let (anims, data) = (aw.anims.clone(), (*ctx.models.data).clone());
                    let pool = nvf_assets::read_pool::read_pool();
                    let task = pool.spawn(async move { anims.warm(&mut ClipReader::new(data), &skeleton, &skel, body, creature, prefix) });
                    ahead.anims.push((key, task));
                }
                PartState::Unknown => {
                    ahead.anims_queued.remove(&(fnv_vfs::normalize(&skeleton), body, prefix));
                }
            },
        }
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    ahead.busy_frames += 1;
    ahead.busy_ms += ms;
    ahead.slowest_ms = ahead.slowest_ms.max(ms);
}

/// The read-ahead's summary when the app exits (smoke tests read it).
pub fn report(mut exit: MessageReader<AppExit>, ahead: Res<ReadAhead>) {
    if exit.read().next().is_none() {
        return;
    }
    let a = &*ahead;
    let (drawn, ms) = clock();
    info!(
        "actor read-ahead summary: {} rings, {} actors listed, {} parts read ahead, {} prepared ahead, {} skeletons' clip sets loaded ahead, {} FaceGen files read ahead ({:.0} ms in {} frames, slowest {:.1} ms); {} actors drawn ({:.0} ms): {} ready at once, {} after waiting for their parts (longest {:.0} ms)",
        a.rings,
        a.actors_listed,
        a.parts_read,
        a.parts_prepared,
        a.anims_loaded,
        a.facegen_read,
        a.busy_ms,
        a.busy_frames,
        a.slowest_ms,
        drawn,
        ms,
        AT_ONCE.load(Ordering::Relaxed),
        AFTER_WAIT.load(Ordering::Relaxed),
        LONGEST_WAIT_US.load(Ordering::Relaxed) as f64 / 1000.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn animated_spawn_waits_for_clip_sets_and_warm_task_completion() {
        use crate::outfits::tests::{asset_fixture, OTHER};
        use std::sync::{Arc, Mutex};
        use std::task::{Poll, Waker};
        const REFERENCE: u32 = 0x1234;
        fn ready(app: &mut App) -> bool {
            app.world_mut().run_system_once(|world: Res<ActorWorld>, mut ahead: ResMut<ReadAhead>, mut parts: ResMut<ActorParts>, mut models: ModelCtx| {
                ready_to_draw(&world, None, &mut ActorCtx { cache: &mut parts, models: &mut models }, &mut ahead, REFERENCE, OTHER.0, false)
            }).unwrap()
        }
        let (mut app, _files) = asset_fixture();
        app.init_resource::<ReadAhead>();
        // Isolate animation readiness: every model in this animated spawn's plan is
        // prepared already, and FaceGen is disabled in the synthetic fixture.
        let (anims, data, skel, plan) = app.world_mut().run_system_once(|world: Res<ActorWorld>, mut parts: ResMut<ActorParts>, mut models: ModelCtx| {
            assert_eq!(world.pose, ActorPose::Idle);
            let plan = actor_plan(&world, None, REFERENCE, OTHER.0, false).unwrap();
            let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
            let skel = ctx.skeleton(&plan.skeleton).unwrap();
            for p in &plan.parts { assert!(ctx.part(p).is_some()); }
            assert!(!world.anims.loaded(&plan.skeleton, plan.body, plan.prefix));
            (world.anims.clone(), (*ctx.models.data).clone(), skel, plan)
        }).unwrap();
        assert!(!ready(&mut app));
        assert!(WAITING.lock().unwrap().as_ref().unwrap().contains_key(&REFERENCE));
        {
            let ahead = app.world().resource::<ReadAhead>();
            assert_eq!(ahead.jobs.len(), 1);
            assert!(matches!(ahead.jobs.front(), Some(Job::Anims { prefix: Some("1hp"), .. })));
        }
        assert!(!ready(&mut app));
        let (skeleton, body, creature, prefix) = {
            let mut ahead = app.world_mut().resource_mut::<ReadAhead>();
            assert_eq!(ahead.jobs.len(), 1, "retries must not duplicate warm-up jobs");
            let Some(Job::Anims { skeleton, body, creature, prefix }) = ahead.jobs.pop_front() else { panic!("animation job") };
            (skeleton, body, creature, prefix)
        };
        let key = (skeleton.clone(), body, prefix);
        // Run the real warm-up, then hold its completion independently of the cache.
        // No sleeps or timing assumptions about when the worker gets scheduled.
        let gate: Arc<Mutex<(bool, Option<Waker>)>> = Arc::default();
        let worker_gate = gate.clone();
        let task = nvf_assets::read_pool::read_pool().spawn(async move {
            anims.warm(&mut ClipReader::new(data), &skeleton, &skel, body, creature, prefix);
            std::future::poll_fn(move |cx| {
                let mut gate = worker_gate.lock().unwrap();
                if gate.0 { Poll::Ready(()) } else {
                    gate.1 = Some(cx.waker().clone());
                    Poll::Pending
                }
            }).await;
        });
        app.world_mut().resource_mut::<ReadAhead>().anims.push((key.clone(), task));
        let deadline = Instant::now() + Duration::from_secs(5);
        while gate.lock().unwrap().1.is_none() {
            assert!(Instant::now() < deadline, "warm-up did not reach the completion gate");
            std::thread::yield_now();
        }
        let world = app.world().resource::<ActorWorld>();
        assert!(world.anims.loaded(&plan.skeleton, plan.body, plan.prefix));
        app.world_mut().resource_mut::<ReadAhead>().collect_anims();
        assert!(!ready(&mut app), "visible cache entries do not complete the task");
        let wake = { let mut gate = gate.lock().unwrap(); gate.0 = true; gate.1.take().unwrap() };
        wake.wake();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut ahead = app.world_mut().resource_mut::<ReadAhead>();
            ahead.collect_anims();
            if ahead.anims.is_empty() { break; }
            assert!(Instant::now() < deadline, "completed warm-up was not collected");
            std::thread::yield_now();
        }
        let ahead = app.world().resource::<ReadAhead>();
        assert_eq!(ahead.anims_loaded, 1);
        assert!(!ahead.anims_queued.contains(&key));
        assert!(ready(&mut app));
        assert!(!WAITING.lock().unwrap().as_ref().unwrap().contains_key(&REFERENCE));
        let parent = app.world_mut().spawn_empty().id();
        let stats = app.world_mut().run_system_once(move |mut commands: Commands, world: Res<ActorWorld>, mut parts: ResMut<ActorParts>, mut models: ModelCtx| {
            let mut stats = crate::actors::ActorStats::default();
            assert!(crate::actors::spawn(&mut commands, parent, bevy::math::Affine3A::IDENTITY, Vec3::ZERO, REFERENCE, OTHER.0, &world, None, &mut ActorCtx { cache: &mut parts, models: &mut models }, &mut stats, false));
            stats
        }).unwrap();
        assert_eq!((stats.animated, stats.armed), (1, 1));
    }

    #[test]
    fn the_ring_is_the_band_outside_the_loaded_grid() {
        let r = ring((10, -3), 2);
        assert_eq!(r.len(), 49 - 25);
        assert!(r.contains(&(13, -3)) && r.contains(&(7, -6)) && r.contains(&(13, 0)));
        assert!(!r.contains(&(12, -1)) && !r.contains(&(10, -3)));
    }

    #[test]
    fn refreshed_plan_waits_for_new_models_stance_and_facegen() {
        let old = Plan { parts: vec!["bare.nif".into()], skeleton: "skeleton.nif".into(),
            body: Body::Male, prefix: Some("h2h"), facegen: vec!["bare.egm".into()], morphs: vec![], textures: vec![] };
        let new = Plan { parts: vec!["armour.nif".into(), "rifle.nif".into()], prefix: Some("2hr"),
            facegen: vec!["armour.egm".into(), "head.egt".into()], ..old.clone() };
        let mut ahead = ReadAhead::default();
        assert!(ahead.queue_plan(&old, false, true, true, |_| PartState::Prepared, |_| true));
        assert!(!ahead.queue_plan(&new, false, true, false, |_| PartState::Reading, |_| false));
        assert!(ahead.jobs.iter().any(|j| matches!(j, Job::Anims { prefix: Some("2hr"), .. })));
        assert!(ahead.jobs.iter().any(|j| matches!(j, Job::FaceGen(p) if p == "armour.egm")));
        assert!(ahead.jobs.iter().any(|j| matches!(j, Job::FaceGen(p) if p == "head.egt")));
        let jobs = ahead.jobs.len();
        assert!(!ahead.queue_plan(&new, false, true, false, |_| PartState::Prepared, |_| false));
        assert_eq!(ahead.jobs.len(), jobs, "waiting does not enqueue the same work twice");
        // Entries can become visible while warm-up is still running. Wait for its task.
        assert!(!ahead.queue_plan(&new, false, true, true, |_| PartState::Prepared, |_| true));
        ahead.anims_queued.clear(); // completion collected by `read_ahead`
        assert!(!ahead.queue_plan(&new, false, true, true, |_| PartState::Prepared, |_| false));
        // `FaceGenCache::has` also returns true for files found absent, avoiding a hang.
        assert!(ahead.queue_plan(&new, false, true, true, |_| PartState::Prepared, |_| true));
    }

    #[test]
    fn going_indoors_drops_the_rings_work_but_not_a_waiting_actors() {
        let ring = Plan { parts: vec!["Outside.nif".into()], skeleton: "outside_skel.nif".into(),
            body: Body::Male, prefix: Some("h2h"), facegen: vec!["outside.egm".into()], morphs: vec![], textures: vec![] };
        let waiting = Plan { parts: vec!["inside.nif".into()], skeleton: "inside_skel.nif".into(),
            prefix: Some("2hr"), facegen: vec!["inside.egm".into()], ..ring.clone() };
        let mut ahead = ReadAhead::default();
        ahead.jobs.push_back(Job::Actor { reference: 7, base: 8, creature: false, door: false });
        assert!(!ahead.queue_plan(&ring, false, true, false, |_| PartState::Reading, |_| false));
        assert!(!ahead.queue_plan(&waiting, false, true, false, |_| PartState::Reading, |_| false));
        ahead.want(&waiting);
        let jobs = ahead.jobs.len();
        assert_eq!(ahead.drop_exterior_work(), 1 + 4, "the ring's actor, part, skeleton, FaceGen file and clip sets");
        assert_eq!(ahead.jobs.len(), jobs - 5);
        assert!(ahead.jobs.iter().all(|j| match j {
            Job::Prepare(p, _) | Job::FaceGen(p) => p.contains("inside"),
            Job::Anims { skeleton, .. } => skeleton.contains("inside"),
            Job::Actor { .. } => false,
        }));
        assert!(!ahead.queued.iter().any(|p| p.contains("outside")));
        assert!(ahead.anims_queued.iter().all(|k| k.0.contains("inside")));
        // Asked for again (an exterior actor drawn after all): queued again, not stranded.
        assert!(!ahead.queue_plan(&ring, false, true, false, |_| PartState::Reading, |_| false));
        assert_eq!(ahead.jobs.len(), jobs - 1);
    }

    #[test]
    fn in_play_an_actor_waits_for_its_parts_to_be_prepared() {
        let plan = Plan { parts: vec!["armour.nif".into()], skeleton: "skeleton.nif".into(),
            body: Body::Male, prefix: None, facegen: vec![], morphs: vec![], textures: vec![] };
        let mut ahead = ReadAhead::default();
        // Read but not prepared: not drawn in a frame that is seen, and queued to prepare.
        assert!(!ahead.queue_plan(&plan, false, false, true, |_| PartState::Read, |_| true));
        assert_eq!(ahead.jobs.iter().filter(|j| matches!(j, Job::Prepare(..))).count(), 2);
        // Behind a loading screen it may be prepared in the frame it is drawn.
        ahead.unseen = true;
        assert!(ahead.queue_plan(&plan, false, false, true, |_| PartState::Read, |_| true));
        ahead.unseen = false;
        assert!(ahead.queue_plan(&plan, false, false, true, |_| PartState::Prepared, |_| true));
        assert!(!ahead.queue_plan(&plan, false, false, true, |_| PartState::Reading, |_| true));
    }
}
