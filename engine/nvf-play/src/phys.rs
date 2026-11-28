//! Part P (docs/design/layer4-play.md §4): the physics world around the player — terrain
//! and references' collision streamed by cell, stepped at a fixed rate.
//!
//! Cells follow the player's feet ([`cells`]: 3×3 exterior squares with one square of
//! hysteresis, or the one interior). Reading and building a cell (records, models,
//! rapier shapes: [`prepare`]) happens on the loader thread ([`loader`]); the main thread
//! inserts at most [`CELLS_PER_FRAME`] prepared cells per frame, in the order they were
//! requested, and loads the player's own cell on the spot if it is not in yet (the
//! player never stands in a cell without collision). An exterior square holds the
//! references its cell lists, the worldspace's persistent references standing in it, and
//! the references placed in other squares whose bounds reach into it
//! (`fnv_world::reach::ReachIndex`, built once per worldspace; #2795: HELIOS One's plaza,
//! placed two squares north of its south end); one held by several loaded squares is in
//! the world once, removed with the last of them [design, #2795]. What a reference contributes is
//! `fnv_phys::ref_collision`, the rule `fnvtool phys cell` uses; whether it is in the
//! world is the simulation's call ([`in_world`]), made when the cell is inserted.
//! `RefChanged` removes or restores one reference's collision; `Travel` rebuilds
//! everything for the destination, which may lie in another worldspace (its own terrain,
//! persistent references and default water height; [`PhysState::worldspace`]). The
//! physics world sets [`Location`] when it applies a `Travel`.
//!
//! References the simulation created (dropped items, `crate::dropped`) load with the
//! physics cell containing them: their base form's model collision, dynamic when the
//! model says so ([`PhysState::add_created`]). While a dynamic one moves, its pose is
//! written back to the simulation ([`PhysState::settle_created`]).
//!
//! Placed references whose collision is one dynamic object (clutter) are written back
//! the same way once physics has moved them (pushed, knocked down: more than
//! [`MOVED_DISTANCE`] or [`MOVED_ROTATION`] from where they were added, so settling on
//! load is not a move): [`PhysState::settle_placed`]. Their cells then build their
//! collision at the simulation's pose (`prepare::Job::poses`), `crate::moved` draws them
//! there, and snapshots keep it. They stay in their placed cell (the cell lists come from
//! the plugins) [engineering choice]. That the game keeps a pushed item where it came to
//! rest is [hypothesis]. Placed clutter is added asleep, so it stays where it was placed
//! until something disturbs it, and a reference with more than one dynamic body (the
//! parts of a jointed object) keeps them keyframed (docs/spec-notes/physics.md §4.3.1;
//! `NVF_PLAY_TEST_PHYS_TRACE`, `phys/trace.rs`, logs what moves after a cell loads).

pub mod cells;
pub mod drawn;
mod loader;
pub mod prepare;
pub mod sinking;
pub mod step_lift;
#[cfg(test)]
mod tests;
mod trace;

use crate::dropped;
use crate::player::Player;
use crate::sim::SimState;
use crate::{Location, PlayData, PlayWorld, RefChanged, Travel};
use bevy::prelude::*;
use cells::{PhysCell, Space};
use fnv_collide::{Iso, Motion};
use drawn::DrawnFloors;
use fnv_phys::{Arrival, Character, CharacterParams, Filter, Owner, PhysParams, PhysWorld, RayHit, RefPose, Stepper, WaterArea};
use fnv_sim::{FormId, World};
use fnv_world::reach::ReachIndex;
use fnv_world::transform::{max_diff, ref_angles, ref_rotation};
use fnv_world::{cell_of, Placement, WorldIndex, CELL_SIZE};
use nvf_assets::GameData;
use prepare::{ms_since, Job, ModelCache, Prepared, RefShapes, Source};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Instant;

/// Prepared cells inserted per frame at most. Inserting one costs about a millisecond
/// (bodies, colliders and the broad-phase update); preparing it, 3–60 ms, mostly reading
/// and parsing models, happens on the loader thread (layer4-play.md §6).
pub const CELLS_PER_FRAME: usize = 1;

/// The same behind a loading screen (`crate::loading`), where frame time does not show.
pub const CELLS_BEHIND_LOADING_SCREEN: usize = 8;

/// The physics world. Ray queries for activation go through [`PhysState::ray`], which
/// keeps the player's capsule out (or `world.queries()` after `world.update_queries()`).
#[derive(Resource)]
pub struct PhysState {
    pub world: PhysWorld,
    pub stepper: Stepper,
    /// The player's character, once placed.
    pub player: Option<Character>,
    /// Characters of actors another system walks (`nvforever`'s residents), by
    /// reference: each is moved by its velocity at every fixed step, with the player.
    pub npcs: BTreeMap<u32, NpcBody>,
    /// The worldspace whose exterior cells and persistent references load outdoors
    /// (FormID; set with [`PhysState::set_world`]).
    pub worldspace: u32,
    /// The worldspace or the interior being played.
    pub space: Space,
    /// Loaded cells.
    pub cells: BTreeMap<PhysCell, LoadedCell>,
    /// The cell of every reference of the loaded cells, in the world or not (so a
    /// reference the simulation brings back can get its collision again).
    pub ref_cell: HashMap<u32, PhysCell>,
    /// References created at run time (dropped items) that have collision now.
    pub created: BTreeMap<u32, CreatedBody>,
    /// Placed references with one dynamic collision object, in the world now.
    pub placed_dynamic: BTreeMap<u32, PlacedDynamic>,
    /// Placed references whose pose [`PhysState::settle_placed`] wrote to the simulation
    /// this frame (for `crate::moved`).
    pub moved_now: Vec<u32>,
    /// Placed references whose pose another game owns: a co-op guest's copies of what the
    /// host's world moves (`nvforever::coop::replication`). [`PhysState::settle_placed`]
    /// does not write them back, so this game's own simulation of them never overrides
    /// the host's pose; their collision is built again where the host puts them. Always
    /// empty outside co-op.
    pub follow_only: BTreeSet<u32>,
    /// Placed clutter and dropped items drawn between physics steps this frame
    /// ([`PhysState::drawn_ref_pose`]).
    pub between: DrawnBetween,
    /// Doors that open in place, in the world now: their keyframed bodies follow
    /// `crate::doors::Doors` ([`pose_doors`]).
    pub doors: BTreeMap<u32, PosedDoor>,
    /// Other references whose keyframed bodies a sequence of their model moves, in the
    /// world now: they follow the group their drawn model plays ([`pose_groups`]).
    pub animated: BTreeMap<u32, PosedGroup>,
    /// Destroyed references whose destruction stage names a model (DSTD's DMDL as stored,
    /// a car's hulk): their collision is built from it ([`destruction_models`]).
    pub destruction_models: HashMap<u32, String>,
    /// The destruction model each reference was last given collision from.
    built_models: HashMap<u32, String>,
    /// The pose (position, Euler angles) each placed reference's collision was last built
    /// at, for [`Self::repose_ref`] to see a script's move of collision that physics does
    /// not move.
    built_at: HashMap<u32, ([f32; 3], [f32; 3])>,
    /// Script moves of fixed or keyframed collision not built yet, by when the last one
    /// came (seconds, the caller's clock): built once the reference rests
    /// ([`REPOSE_SETTLE`]) or strays far from where it was built ([`Self::repose_ref`]).
    repose_pending: BTreeMap<u32, f64>,
    /// Collision built again for a script's move ([`Self::repose_ref`]), for tests and
    /// the stats line.
    pub reposes: u64,
    /// References given collision so far (each add, and each build again): a reader that
    /// must act on rebuilt collision (nvforever `wrecks.rs`) looks again only when it grew.
    pub refs_built: u64,
    /// The drawn floors of the references in the world whose collision stays where it
    /// was placed, with their pose ([`PhysState::drawn_ground`]).
    drawn: HashMap<u32, (Arc<DrawnFloors>, RefPose)>,
    /// How far below the player character's feet its body is drawn ([`drawn_sink`]),
    /// as of its last physics step.
    player_sink: f32,
    /// How far the player's body and view are drawn above where its feet and drawn
    /// ground put them, after a step or a bump ([`step_lift::StepLift`]).
    pub player_lift: step_lift::StepLift,
    /// The height of each living actor's drawn bounds, game units, as `crate::hitvol`
    /// last measured them: an actor's character is made this tall ([`PhysState::add_npc`]).
    pub actor_heights: HashMap<u32, f32>,
    /// Scaled creature BBX radii for actor blocking only (`hitvol::sync_volumes`)
    /// [hypothesis: fits capture run15b_b2 spacing; original shape unseen, #2535].
    pub actor_block_radii: HashMap<u32, f32>,
    /// Actors drawn standing without a character: their feet, whether they may follow the
    /// landscape, and how far below them the body is drawn
    /// ([`PhysState::standing_drawn_feet`]).
    standing: HashMap<u32, ([f32; 3], bool, f32)>,
    /// Named NVForever option `sneak-bounding-box` (off by default; #3845): shrink crouch
    /// collider height for the player and crouching human companions, synced live from
    /// `crate::player::SneakBoundingBox`.
    pub sneak_bounding_box_fix: bool,
    /// Where each actor's character stood, and how far below it its body was drawn, when
    /// the character was taken away ([`PhysState::remove_npc`]): feet still there came
    /// from physics and are drawn as the character was, never lifted onto the landscape
    /// again ([`PhysState::standing_drawn_feet`], #2149).
    left: HashMap<u32, ([f32; 3], f32)>,
    pub stats: PhysStats,
    /// Feet before the last fixed step (the camera interpolates from it).
    pub(crate) prev_feet: [f32; 3],
    /// The worldspace's persistent references by the grid square they stand in.
    persistent_by_grid: HashMap<(i32, i32), Vec<u32>>,
    /// The worldspace's references placed in one square whose bounds reach into others
    /// (`fnv_world::reach`, #2795): each exterior square loads them too.
    pub(crate) reach: Arc<ReachIndex>,
    /// [`Self::reach`] of each worldspace played so far, built on the first entry.
    reach_by_world: HashMap<u32, Arc<ReachIndex>>,
    /// How many loaded cells list each placed reference ([`LoadedCell::refs`]): a
    /// reference whose bounds reach over several loaded squares is in the world once, added
    /// with the first of them and removed with the last (#2795).
    held: HashMap<u32, u16>,
    /// Collision objects per model path, shared with the loader thread.
    models: Arc<ModelCache>,
    /// Cells being prepared on the loader thread.
    loader: loader::Loader,
    /// Test aid: prepares cells instead of the plugins and archives.
    #[cfg(test)]
    fake: Option<fn(&Job) -> Prepared>,
}

/// The longest frame the game's virtual clock passes on, seconds (Bevy's `Time<Virtual>`
/// default `max_delta`): longer frames are slowed down to it.
pub const MAX_FRAME_SECONDS: f32 = 0.25;

/// The physics settings play starts with: as `PhysParams::default` (60 steps a second;
/// `crate::phys_rate` sets the step and the catch-up for the chosen rate each frame), with
/// enough fixed steps for a whole frame of the virtual clock ([`MAX_FRAME_SECONDS`]). With the default 8 steps of
/// 1/60 s a frame longer than 0.133 s lost the rest, so on a busy machine (6 frames a
/// second) the player and the actors walking near it covered less ground than the game's
/// clock passed: Goodsprings' settlers came into view seconds of game time late
/// (detection.md §5) [design].
pub fn play_params() -> PhysParams {
    let p = PhysParams::default();
    PhysParams { max_substeps: (MAX_FRAME_SECONDS / p.dt).ceil() as u32, ..p }
}

/// Switch back (any value but empty or `0`, e.g. `1`): actor hit volumes are attached to
/// their bodies again, the representation before 2026-11-03, instead of standalone colliders
/// (`PhysWorld::set_standalone_hit_volumes`, `fnv_phys::actors`: the cost of a write;
/// physics.md §5.1). For comparing; not a test aid: it does not make the run unattended.
pub const HITVOL_ATTACHED_VAR: &str = "NVF_PHYS_HITVOL_ATTACHED";

/// The physics world of play: [`play_params`], hit volumes standalone unless
/// [`HITVOL_ATTACHED_VAR`] is set.
fn play_world() -> PhysWorld {
    let mut w = PhysWorld::new(play_params());
    let attached = std::env::var(HITVOL_ATTACHED_VAR).is_ok_and(|v| !v.is_empty() && v != "0");
    if attached {
        info!("actor hit volumes: attached to their bodies ({HITVOL_ATTACHED_VAR})");
    } else {
        info!("actor hit volumes: standalone colliders ({HITVOL_ATTACHED_VAR}=1 attaches them)");
    }
    w.set_standalone_hit_volumes(!attached);
    w
}

impl Default for PhysState {
    fn default() -> Self {
        PhysState {
            world: play_world(),
            stepper: Stepper::default(),
            player: None,
            npcs: BTreeMap::new(),
            worldspace: 0,
            space: Space::Exterior,
            cells: BTreeMap::new(),
            ref_cell: HashMap::new(),
            created: BTreeMap::new(),
            placed_dynamic: BTreeMap::new(),
            moved_now: Vec::new(),
            follow_only: BTreeSet::new(),
            between: DrawnBetween::default(),
            doors: BTreeMap::new(),
            animated: BTreeMap::new(),
            destruction_models: HashMap::new(),
            built_models: HashMap::new(),
            built_at: HashMap::new(),
            repose_pending: BTreeMap::new(),
            reposes: 0,
            refs_built: 0,
            drawn: HashMap::new(),
            player_sink: 0.0,
            player_lift: step_lift::StepLift::default(),
            actor_heights: HashMap::new(),
            actor_block_radii: HashMap::new(),
            standing: HashMap::new(),
            sneak_bounding_box_fix: false,
            left: HashMap::new(),
            stats: PhysStats::default(),
            prev_feet: [0.0; 3],
            persistent_by_grid: HashMap::new(),
            reach: Arc::default(),
            reach_by_world: HashMap::new(),
            held: HashMap::new(),
            models: Arc::default(),
            loader: loader::Loader::default(),
            #[cfg(test)]
            fake: None,
        }
    }
}

/// Drawn geometry this far below or above the collision under a character is not the
/// floor it stands on [design: the Prospector Saloon's porch boards are drawn 2.5 above
/// their collision, floating-actors.md §7].
pub const DRAWN_FLOOR_WINDOW: f32 = 4.0;

/// How far below a character's feet its body is drawn: down to the drawn ground under
/// it (`drawn`, from [`PhysState::drawn_ground`]), so the soles touch what the player
/// sees while the feet, and with them the reference's position, the eye and every
/// query, stay where the game measures them. Over terrain the drawn ground is the
/// collision's bare surface, the rest gap (`CharacterParams::rest_gap`, 3.5 units) below
/// the feet [observed: rest heights, physics.md §6.2; that the game draws the body on
/// the bare surface is [hypothesis] from the owner's report of 2026-10-16 that the
/// player's boots hung in the air, floating-actors.md §6]. Over a model it is the top of
/// its drawn geometry, which can lie above its collision and so above the feet: the body
/// is then drawn raised (a negative sink) [hypothesis: the owner's report of 2026-10-17
/// that Easy Pete sank into the Prospector Saloon's porch, floating-actors.md §7]. With
/// no ground found (`drawn` is `None`), the rest gap. None while swimming (the float
/// depth counts from the feet).
pub fn drawn_sink(c: &Character, drawn: Option<f32>) -> f32 {
    if c.swimming {
        0.0
    } else {
        drawn.map_or(c.gap, |z| c.feet[2] - z)
    }
}

/// Feet this close to a character's rest height over the collision under them stand
/// where a character would rest, game units [design: the observed rest heights spread
/// 0.35 either side of the shell, physics.md §6.2].
pub const REST_TOLERANCE: f32 = 0.5;

/// Feet without a character this close to the stored LAND triangles under them, with
/// nothing but the landscape below, stand on the landscape: drawn as far above or below
/// the stored triangles on the smoothed surface (#2149) as they were on the stored one
/// [design, #2149: authored and navmesh heights follow the ground the data's triangles
/// make, within a few units; beyond this band an actor stands on something else].
pub const TERRAIN_STAND_BAND: f32 = 32.0;

/// The ground under a point ([`PhysState::ground_drawn`]): the bare collision surface,
/// the drawn ground and the collision's contact shell.
struct GroundUnder {
    bare: f32,
    drawn: f32,
    shell: f32,
}

/// Tilted more than this (the cosine of the reference's up axis against the world's), a
/// model's drawn floors are not looked at [design: placed floors stand upright].
const UPRIGHT_MIN_COS: f32 = 0.996;

/// The highest drawn floor of a placed model over game point `at` within
/// [`DRAWN_FLOOR_WINDOW`] of its height, in game space; `None` when there is none or the
/// reference is tilted (its floors are then not under the point the same way).
fn drawn_floor(floors: &DrawnFloors, pose: &RefPose, at: [f32; 3]) -> Option<f32> {
    let r = &pose.rotation;
    let s = pose.scale;
    if r[8] < UPRIGHT_MIN_COS || s <= 0.0 {
        return None;
    }
    // Into the model's frame: world = R·(s·v) + position, R row-major.
    let d = [at[0] - pose.position[0], at[1] - pose.position[1], at[2] - pose.position[2]];
    let local = |k: usize| (r[k] * d[0] + r[3 + k] * d[1] + r[6 + k] * d[2]) / s;
    let (x, y, z) = (local(0), local(1), local(2));
    let w = DRAWN_FLOOR_WINDOW / s;
    let top = floors.height_at(x, y, z - w, z + w)?;
    Some(at[2] + (top - z) * s)
}

/// The references whose bodies are drawn between physics steps (`fnv_phys::drawn`,
/// docs/spec-notes/physics.md §5.2): placed clutter physics has moved
/// ([`PlacedDynamic::moved`]) and dropped items with a dynamic body, while their body
/// moves. The systems that draw them (`crate::moved`, `crate::dropped`,
/// `crate::script_draw`) draw them at [`PhysState::drawn_ref_pose`]; the simulation keeps
/// the stepped pose ([`PhysState::settle_placed`], [`PhysState::settle_created`]).
#[derive(Debug, Default)]
pub struct DrawnBetween {
    /// Drawn between steps this frame, sorted.
    pub now: Vec<u32>,
    /// Those and the ones drawn between steps the frame before, sorted: a reference whose
    /// body stopped is drawn once more, where it came to rest.
    pub redraw: Vec<u32>,
    /// Drawn between steps the frame before.
    last: Vec<u32>,
    /// The most references drawn between steps in one frame, and the frames with any
    /// (the exit report).
    pub most: usize,
    pub frames: u64,
}

/// An actor's character in the physics world: who moves it sets `velocity`.
pub struct NpcBody {
    pub ch: Character,
    /// Feet before the last physics step, for drawing between steps
    /// ([`PhysState::npc_drawn_feet`]).
    pub prev_feet: [f32; 3],
    /// Wanted horizontal velocity (x, y), game units per second.
    pub velocity: [f32; 2],
    /// Jump at the next physics step (taken only standing on the ground, as the
    /// player's; cleared once stepped): an actor jumping a ledge or an obstacle
    /// (docs/spec-notes/actor-jumps.md).
    pub jump: bool,
    /// How far below the feet the body is drawn ([`drawn_sink`]), as of the last
    /// physics step.
    pub sink: f32,
    /// Drawn above that after a step or a bump ([`step_lift::StepLift`]).
    pub lift: step_lift::StepLift,
    /// Standing capsule height (for optional `sneak-bounding-box` crouch resizing, #3845).
    pub standing_height: f32,
    /// Whether this actor is a crouching human companion whose capsule should shrink under
    /// `sneak-bounding-box` (synced live from `Game` by `crate::player::sync_sneak_bounding_box`).
    pub sneak_crouch: bool,
}

/// Why [`PhysState::npc_spot`] found no place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcSpotRefused {
    /// The collision around it is not loaded (or the actor has no character).
    NotLoaded,
    /// Nothing to stand on at that height.
    NoSupport,
    /// There is ground, but the body does not fit over it.
    NoRoom,
}

/// How far around a place the collision must be loaded before an actor is put there,
/// game units [design: its body and what stands next to it; what reaches further over
/// the border is not known until that square is in].
const NPC_SPOT_MARGIN: f32 = 256.0;

/// Shared actor slope limit [design, #2511; physics.md §6.4.1/§6.9]: below the cone
/// face's angle so thin walls higher than the step height block actors too.
pub use fnv_phys::character::ACTOR_MAX_SLOPE_DEG as NPC_MAX_SLOPE_DEG;

/// The shortest character an actor gets, game units [design: a capsule at least twice
/// [`crate::blocking::MIN_RADIUS`] tall].
pub const NPC_MIN_HEIGHT: f32 = 2.0 * crate::blocking::MIN_RADIUS;

/// Actors drawn shorter than this share of the player's height get a character of their
/// own size ([`npc_params`]) [design].
pub const NPC_SMALL_SHARE: f32 = 0.75;

/// The character of an actor whose drawn bounds are `height` tall (`None`: not measured
/// yet): the player's, but for an actor under [`NPC_SMALL_SHARE`] of the player's height
/// as tall as it is drawn
/// and as wide as its block (`crate::blocking`: [`crate::blocking::RADIUS_SHARE`] of
/// its height, at least [`crate::blocking::MIN_RADIUS`]), so a small creature (a mantis
/// nymph drawn 30 units tall) is not walked in, and in the way as, a person's capsule
/// (owner's playtest list 7 item 35; physics.md §6.8) [design]. Larger actors keep the
/// player's capsule: a wider one would not fit through the doors and paths they walk.
pub fn npc_params(height: Option<f32>) -> CharacterParams {
    let base = CharacterParams { max_slope_deg: NPC_MAX_SLOPE_DEG, ..CharacterParams::default() };
    match height.filter(|h| h.is_finite() && *h < NPC_SMALL_SHARE * base.height) {
        Some(h) => {
            let height = h.max(NPC_MIN_HEIGHT);
            let radius = (crate::blocking::RADIUS_SHARE * height).clamp(crate::blocking::MIN_RADIUS, base.radius);
            CharacterParams { radius, height, step_width: base.step_width.min(radius), ..base }
        }
        None => base,
    }
}

/// A loaded cell.
#[derive(Debug, Clone, Default)]
pub struct LoadedCell {
    /// Its references (and, outdoors, the worldspace's persistent references standing in
    /// the square and the references placed in other squares whose bounds reach into it,
    /// `fnv_world::reach`), by FormID, in the world or not. A reference listed by several
    /// loaded cells is in the world once (`PhysState::held`).
    pub refs: Vec<u32>,
    /// The landscape surface its terrain collides as (exterior squares with terrain):
    /// the LAND grid as stored or smoothed (`crate::smooth_terrain`).
    pub surface: Option<fnv_world::land_surface::Surface>,
}

/// The collision of a reference created at run time (a dropped item).
#[derive(Debug, Clone, PartialEq)]
pub struct CreatedBody {
    /// The collision object's node and body transforms, when its model has exactly one
    /// object and it became one body: that body's pose gives the reference's. Otherwise
    /// the pose is not written back (a model's bodies are not joined in `fnv-phys`).
    pub local: Option<Iso>,
    /// Some object is dynamic (it can fall, be pushed and roll).
    pub dynamic: bool,
    /// Its position when its collision was added.
    pub start: [f32; 3],
    /// The game seconds the world had stepped then (`PhysWorld::stepped_secs`).
    pub added_at: f64,
    /// At rest (its body asleep) since it last moved.
    pub settled: bool,
    /// How many of the pose marks (`CREATED_MARKS`) were logged.
    pub marks: usize,
    /// The game seconds the world had stepped when it last came to rest, if it has
    /// (`sinking`, #2753).
    pub rested_at: Option<f64>,
    /// Its sinking under the terrain was logged (`sinking`, #2753): once.
    pub sank: bool,
}

/// The most a created dynamic reference may reach into the contact shell under it and be
/// lifted onto it ([`PhysState::lift_onto_support`]), game units [design: twice the
/// terrain's shell].
pub const LIFT_ONTO_SUPPORT_MOST: f32 = 7.0;

/// Seconds after its collision was added at which a moving dynamic created reference's
/// pose is logged, so a log shows where it went.
pub const CREATED_MARKS: [f32; 3] = [1.0, 3.0, 10.0];

/// A door whose keyframed bodies follow its animation.
#[derive(Clone)]
pub struct PosedDoor {
    pub pose: RefPose,
    pub bodies: prepare::DoorBodies,
    /// The ONAM flag of its placement (its state when no simulation knows it).
    pub open_by_default: bool,
    /// The sequence (true: "Open") and time its bodies were last posed at.
    pub shown: Option<(bool, f32)>,
    /// Whether its moving parts were last made solid (not while it swings).
    pub solid: Option<bool>,
}

impl PosedDoor {
    /// Pose the door's keyframed bodies for `seq` ("Open" when true) at `time` seconds,
    /// or in the model's stored pose when `stored` (a closed door, `DoorMotion::closed`).
    fn pose(&mut self, world: &mut PhysWorld, open: bool, time: f32, stored: bool) {
        if self.shown == Some((open, time)) {
            return;
        }
        let anim = &self.bodies.anim;
        let Some((o, c)) = anim.door() else { return };
        let seq = (!stored).then_some(if open { o } else { c });
        for (object, block, body) in &self.bodies.objects {
            if let Some(node) = anim.node_to_root(seq, time, *block) {
                world.pose_ref_object(&self.pose, *object, &node.iso, body);
            }
        }
        self.shown = Some((open, time));
    }

    /// The timing of its sequences, for [`crate::doors::Doors::register`].
    pub fn timing(&self) -> Option<crate::doors::DoorTiming> {
        self.bodies.anim.door().map(|(o, c)| crate::doors::DoorTiming::from_sequences(o, c))
    }
}

/// A reference (not a door that opens in place) whose keyframed bodies follow the
/// sequence its drawn model plays (issue #1396).
#[derive(Clone)]
pub struct PosedGroup {
    pub pose: RefPose,
    pub bodies: prepare::AnimatedBodies,
    /// The sequence (index in the model's, `None`: the bind pose) and the time its bodies
    /// were last posed at; `None` before the first pose (they were built in the bind pose).
    pub shown: Option<(Option<usize>, f32)>,
}

impl PosedGroup {
    /// Pose the bodies for sequence `seq` of the model (`None`: its bind pose) at `time`
    /// seconds after its start (held at its ends). Returns whether anything was posed.
    pub fn pose(&mut self, world: &mut PhysWorld, seq: Option<usize>, time: f32) -> bool {
        if self.shown == Some((seq, time)) || (self.shown.is_none() && seq.is_none()) {
            return false;
        }
        let anim = &self.bodies.anim;
        let sequence = seq.and_then(|i| anim.sequences.get(i));
        for (object, block, body) in &self.bodies.objects {
            if let Some(node) = anim.node_to_root(sequence, time, *block) {
                world.pose_ref_object(&self.pose, *object, &node.iso, body);
            }
        }
        self.shown = Some((seq, time));
        true
    }
}

/// The sequence each drawn object plays and how far into it (seconds after its start),
/// by reference: written by whoever draws them (nvforever `object_anims.rs`, from the
/// clip each rig plays), so the collision of [`PhysState::animated`] follows the drawn
/// model, not a clock of its own ([`pose_groups`]).
#[derive(Resource, Default, Debug)]
pub struct ObjectPoses {
    pub playing: HashMap<u32, (String, f32)>,
}

/// NIF cycle type "clamp": played once and held at its end [documented: nif.xml CycleType].
const CYCLE_CLAMP: i64 = 2;

/// Where an animated reference's bodies go: the sequence its drawn model plays
/// ([`ObjectPoses`]); not drawn, the last group a script asked of it (saved with the
/// game, `fnv_game::anims::ObjectAnims::asked`): a group played once at its end, a loop
/// at its start [design: as the renderer shows such a group when the object is drawn];
/// otherwise the bind pose. (sequence index, seconds after its start).
pub fn group_pose(anim: &fnv_collide::anim::ModelAnim, drawn: Option<&(String, f32)>, asked: Option<&str>) -> (Option<usize>, f32) {
    let find = |name: &str| anim.sequences.iter().position(|s| s.name.eq_ignore_ascii_case(name));
    if let Some((i, t)) = drawn.and_then(|(n, t)| Some((find(n)?, *t))) {
        return (Some(i), t);
    }
    match asked.and_then(find) {
        Some(i) => {
            let s = &anim.sequences[i];
            (Some(i), if s.cycle_type == CYCLE_CLAMP { s.length() } else { 0.0 })
        }
        None => (None, 0.0),
    }
}

/// Move the keyframed bodies of [`PhysState::animated`] to the pose of the sequence
/// their drawn model plays ([`group_pose`]). They stay solid while they move [engine
/// choice: a lift or a drawbridge carries and stops actors; a door's leaf is not solid
/// while it swings, `DoorMotion::swinging`]. Their drawn floors are not used
/// (`prepare::reference`): those are the file's unposed geometry.
fn pose_groups(poses: Option<Res<ObjectPoses>>, script: Option<Res<crate::script::ScriptState>>, mut phys: ResMut<PhysState>) {
    if phys.animated.is_empty() {
        return;
    }
    let anims = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game.anims);
    phys.pose_animated(poses.as_deref(), |id| anims.and_then(|a| a.asked(FormId(id))).map(|(_, n)| n));
}

impl PhysState {
    /// Pose every reference of [`PhysState::animated`] ([`group_pose`]): `asked` gives the
    /// last group a script asked of a reference.
    pub fn pose_animated<'a>(&mut self, poses: Option<&ObjectPoses>, asked: impl Fn(u32) -> Option<&'a str>) {
        for (&id, g) in self.animated.iter_mut() {
            let drawn = poses.and_then(|p| p.playing.get(&id));
            let (seq, time) = group_pose(&g.bodies.anim, drawn, asked(id));
            let first = g.shown.is_none();
            if g.pose(&mut self.world, seq, time) && first {
                let name = seq.and_then(|i| g.bodies.anim.sequences.get(i)).map_or("-", |s| s.name.as_str());
                info!("phys: 0x{id:08X}: {} animated collision bodies follow {name} at {time:.2} s", g.bodies.objects.len());
            }
        }
    }
}

/// A placed reference that physics can move: its collision is one dynamic object.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedDynamic {
    /// The object's node and body transforms (see [`CreatedBody::local`]).
    pub local: Iso,
    pub scale: f32,
    /// Position and row-major rotation it was added at.
    pub added: ([f32; 3], [f32; 9]),
    /// Physics has moved it since (beyond [`MOVED_DISTANCE`] / [`MOVED_ROTATION`]).
    pub moved: bool,
    /// It has come to rest again since it moved (its body asleep), for the log.
    pub rested: bool,
}

/// A placed dynamic reference counts as moved by physics once its pose is this far (game
/// units) from where it was added [engineering choice: clutter placed a little above its
/// support settles by a unit or less when its cell loads, which is not a push].
pub const MOVED_DISTANCE: f32 = 1.0;

/// How far (game units) a script may move fixed or keyframed collision before it is
/// built again at once, rather than when the reference rests ([`PhysState::repose_ref`])
/// [engineering choice: about a quarter of an actor's height; a slow lift or a turning
/// sign rebuilds a few times a second at most].
pub const REPOSE_NOW_DISTANCE: f32 = 32.0;

/// How far (largest rotation-matrix entry difference, about radians for small turns) a
/// script may turn fixed or keyframed collision before it is built again at once
/// [engineering choice: about 15 degrees; a 10 degree per second turn rebuilds every
/// 1.5 s].
pub const REPOSE_NOW_ANGLE: f32 = 0.26;

/// Seconds with no script move after which held-back collision is built where its
/// reference rests ([`PhysState::flush_reposes`]) [engineering choice].
pub const REPOSE_SETTLE: f64 = 0.25;
/// Or turned this much (largest difference of a rotation matrix entry, about 1.1 degrees)
/// [engineering choice].
pub const MOVED_ROTATION: f32 = 0.02;

/// A dynamic created reference that falls this far below where its collision was added
/// has fallen through the world [engineering choice]: its collision is removed and it is
/// put back where it started (logged as a warning).
pub const CREATED_FELL_THROUGH: f32 = 10_000.0;

/// Running totals for logs.
#[derive(Debug, Clone, Default)]
pub struct PhysStats {
    pub cells_loaded: usize,
    pub load_ms: f64,
    /// Outcomes per reference: the `fnv.phys.cell/1` `refs.counts` names, plus
    /// `not_present` (deleted, disabled, or taken in the simulation).
    pub counts: BTreeMap<&'static str, usize>,
    /// Time spent per part of loading, all cells so far (preparing on whichever thread
    /// did it, inserting on the main thread).
    pub times: LoadTimes,
    /// Longest physics streaming work on the main thread in one frame once the player
    /// is placed (ms); the start loads the player's cell on the spot.
    pub worst_frame_ms: f64,
    /// Cells the player was about to stand in before the loader had them ready, loaded
    /// on the main thread.
    pub loaded_inline: usize,
}

/// Milliseconds per part of loading cells.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoadTimes {
    /// Plugin records: cell, placements, base objects, LAND.
    pub records: f64,
    /// Model files read from the archives.
    pub read: f64,
    /// NIF parsing.
    pub parse: f64,
    /// NIF collision blocks to `fnv_collide` collision objects.
    pub convert: f64,
    /// rapier shapes: terrain meshes, reference shapes (trimesh trees, hulls).
    pub shapes: f64,
    /// Bodies and colliders into the world.
    pub insert: f64,
    /// Broad-phase update after inserting (`PhysWorld::update_queries`).
    pub queries: f64,
}

impl LoadTimes {
    fn add(&mut self, o: &LoadTimes) {
        self.records += o.records;
        self.read += o.read;
        self.parse += o.parse;
        self.convert += o.convert;
        self.shapes += o.shapes;
        self.insert += o.insert;
        self.queries += o.queries;
    }
}

impl std::fmt::Display for LoadTimes {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "records {:.1}, read {:.1}, parse {:.1}, convert {:.1}, shapes {:.1}, insert {:.1}, queries {:.1} ms",
            self.records, self.read, self.parse, self.convert, self.shapes, self.insert, self.queries
        )
    }
}

/// The preparing parts of [`LoadTimes`].
struct PrepareTimes<'a>(&'a LoadTimes);

impl std::fmt::Display for PrepareTimes<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let t = self.0;
        write!(
            f,
            "records {:.1}, read {:.1}, parse {:.1}, convert {:.1}, shapes {:.1} ms",
            t.records, t.read, t.parse, t.convert, t.shapes
        )
    }
}

/// `job` with the simulation's poses of its references that physics or a script moved,
/// and the scales scripts set (SetScale).
fn with_poses(mut job: Job, sim: &SimState) -> Job {
    job.poses = job.refs.iter().filter_map(|&r| sim.moved_pose(r).map(|(p, a)| (r, p, a))).collect();
    job.scales = job.refs.iter().filter_map(|&r| sim.script_scale(r).map(|s| (r, s))).collect();
    job
}

/// What loading needs besides the physics state.
pub struct LoadCtx<'a> {
    pub d: &'a PlayData,
    pub sim: &'a SimState,
    /// The archives; without them only terrain and collision primitives load.
    pub data: Option<&'a GameData>,
}

/// Whether a placed reference is in the world now: not deleted in the plugins, and
/// present for the simulation, whether or not it holds state for the reference (under an
/// enable parent `World::is_enabled` ignores the reference's own Initially Disabled flag;
/// nothing sends a `RefChanged` for a reference whose state never changed, so this is
/// its only chance). A placed type the simulation's table leaves out, which
/// `SimState::present` takes for enabled, goes by its own flag.
pub fn in_world(sim: &SimState, pl: &Placement) -> bool {
    !pl.deleted() && sim.present(pl.form_id) && sim.world.is_enabled(FormId(pl.form_id)).unwrap_or(!pl.initially_disabled())
}

impl PhysState {
    /// Play outdoors in worldspace `world`: index its persistent references by grid
    /// square (the square containing the placement, as render streaming and `fnvtool
    /// phys` assign them). Its terrain, cells and default water height follow from
    /// `world` as cells load. Loaded exterior cells belong to the previous worldspace:
    /// [`PhysState::reset`] first.
    ///
    /// A child worldspace (WRLD `WNAM`) takes land, water and other data from its parent
    /// by its `PNAM` flags; the world index has followed them already
    /// (`fnv_world::WorldIndex::take_parent_data`, #2345: a cell has its parent's LAND
    /// where the parent has one, its own too since #3755; the default heights and water
    /// type are the parent's where the child has none), so the records read here are the
    /// ones to use. Logged once per start of
    /// play in such a worldspace.
    ///
    /// The first time a worldspace is played its reach index is built
    /// (`fnv_world::reach::ReachIndex`: every placed reference of its exterior cells read
    /// once, #2795) and kept for the session. What that costs in time and memory is
    /// [hypothesis: tens to hundreds of milliseconds for WastelandNV, not measured yet]; the
    /// log line below says.
    pub fn set_world(&mut self, d: &PlayData, world: u32) {
        self.worldspace = world;
        self.persistent_by_grid.clear();
        self.reach = match self.reach_by_world.get(&world) {
            Some(r) => r.clone(),
            None => {
                let t = Instant::now();
                let r = Arc::new(ReachIndex::build(d.lo, d.schema, d.index, world));
                info!(
                    "physics: worldspace {world:08X}: {} references reach beyond their own square ({} square entries), indexed in {:.0} ms",
                    r.refs,
                    r.entries(),
                    ms_since(t)
                );
                self.reach_by_world.insert(world, r.clone());
                r
            }
        };
        let ws = d.index.worldspaces.get(&world);
        if let Some(w) = ws.filter(|w| w.parent.is_some() && w.parent_flags & !PNAM_USE_MAP_DATA != 0) {
            let cells = w.grid.values().filter_map(|c| d.index.cells.get(c));
            let (own, theirs) = cells.fold((0, 0), |(o, t), c| match (c.land, c.land_of_parent) {
                (Some(_), false) => (o + 1, t),
                (Some(_), true) => (o, t + 1),
                _ => (o, t),
            });
            info!(
                "worldspace {world:08X} takes data from its parent {:08X} (PNAM 0x{:X}): land of {theirs} cell(s) from the parent, {own} of its own; default land height {:?}, water height {:?}",
                w.parent.unwrap_or(0),
                w.parent_flags,
                w.default_land_height,
                w.default_water_height
            );
        }
        if let Some(pc) = ws.and_then(|w| w.persistent_cell).and_then(|c| d.index.cells.get(&c)) {
            for &r in &pc.refs {
                if let Some(p) = d.index.placement(d.lo, d.schema, r) {
                    self.persistent_by_grid.entry(cell_of(p.position[0], p.position[1])).or_default().push(r);
                }
            }
        }
    }

    /// The cell the player stands in at `feet`.
    pub fn cell_at(&self, feet: [f32; 3]) -> PhysCell {
        self.space.cell_at(feet)
    }

    /// Whether the cell under `feet` is loaded (the player may move).
    pub fn ready_at(&self, feet: [f32; 3]) -> bool {
        self.cells.contains_key(&self.cell_at(feet))
    }

    /// Whether every cell kept around `feet` is loaded (outdoors the squares of
    /// [`cells::wanted`], indoors the one cell). The start waits for this, not only for
    /// the cell underfoot: a reference placed in a neighbouring square can reach over the
    /// start (the Bison Steve rollercoaster track over Primm, #1103), and a start placed
    /// before it is in finds only the terrain below.
    pub fn ready_around(&self, feet: [f32; 3]) -> bool {
        match self.space {
            Space::Interior(_) => self.ready_at(feet),
            Space::Exterior => cells::wanted(cell_of(feet[0], feet[1])).into_iter().all(|(x, y)| self.cells.contains_key(&PhysCell::Exterior(x, y))),
        }
    }

    /// Whether every cell within `margin` units of `feet` is loaded (indoors the one
    /// cell). For an actor anywhere in the loaded area, where [`PhysState::ready_around`]
    /// asks too much: the cells kept around the player reach one square past it, so the
    /// squares around a place one square off the player need not be in.
    pub fn ready_near(&self, feet: [f32; 3], margin: f32) -> bool {
        match self.space {
            Space::Interior(_) => self.ready_at(feet),
            Space::Exterior => {
                let (x0, y0) = cell_of(feet[0] - margin, feet[1] - margin);
                let (x1, y1) = cell_of(feet[0] + margin, feet[1] + margin);
                (x0..=x1).all(|x| (y0..=y1).all(|y| self.cells.contains_key(&PhysCell::Exterior(x, y))))
            }
        }
    }

    /// Where between the last two physics steps the frame is drawn: the share of a step
    /// the clock has gone since the last one (the stepper's remainder over the step), in
    /// [0, 1]. Everything physics moves is drawn this far from its pose before the last
    /// step to its pose after it: the characters ([`PhysState::npc_drawn_feet`]), the
    /// bodies (`fnv_phys::drawn`, [`PhysState::drawn_ref_pose`]), nvforever's corpses, gore,
    /// wrecks and swing traps. Worked out in one place, `fnv_phys::Stepper::alpha`.
    pub fn draw_alpha(&self) -> f32 {
        self.stepper.alpha(self.world.params.dt)
    }

    /// The reference pose (position, row-major rotation) to draw reference `id` at this
    /// frame, from its body drawn between the last two steps (`fnv_phys::drawn`,
    /// [`PhysState::draw_alpha`]): for placed clutter physics has moved and dropped items
    /// with one dynamic body. `None` for any other reference, which is drawn at the
    /// simulation's pose. For drawing only: the simulation's pose is the stepped one
    /// ([`PhysState::settle_placed`], [`PhysState::settle_created`]).
    pub fn drawn_ref_pose(&self, id: u32) -> Option<([f32; 3], [f32; 9])> {
        let (local, scale) = match self.placed_dynamic.get(&id) {
            // Not moved yet: drawn at its placement, which the simulation still holds.
            Some(p) if !p.moved => return None,
            Some(p) => (p.local, p.scale),
            None => self.created.get(&id).and_then(|c| c.local.filter(|_| c.dynamic)).map(|l| (l, 1.0))?,
        };
        let b = self.world.drawn_ref_body(id, 0, self.draw_alpha())?;
        Some(dropped::ref_pose_from_body(&b, &local, scale))
    }

    /// Bring [`PhysState::between`] up to date after the frame's steps.
    pub fn update_between(&mut self) {
        let b = &mut self.between;
        std::mem::swap(&mut b.last, &mut b.now);
        self.world.drawn_refs_into(&mut b.now);
        let (placed, created) = (&self.placed_dynamic, &self.created);
        b.now.retain(|id| match placed.get(id) {
            Some(p) => p.moved,
            None => created.get(id).is_some_and(|c| c.dynamic && c.local.is_some()),
        });
        b.redraw.clear();
        b.redraw.extend(b.now.iter().chain(&b.last).copied());
        b.redraw.sort_unstable();
        b.redraw.dedup();
        b.most = b.most.max(b.now.len());
        b.frames += !b.now.is_empty() as u64;
    }

    /// Where an actor's character is drawn this frame: between its feet before and after
    /// the last physics step, by the time accumulated towards the next (as the player's
    /// body), so a walk is as smooth on screen as the frame rate allows. `None` when the
    /// actor has no character.
    /// The body is drawn [`drawn_sink`] below the character's feet.
    pub fn npc_drawn_feet(&self, id: u32) -> Option<[f32; 3]> {
        let b = self.npcs.get(&id)?;
        let a = self.draw_alpha();
        Some(std::array::from_fn(|i| b.prev_feet[i] + (b.ch.feet[i] - b.prev_feet[i]) * a - if i == 2 { b.sink - b.lift.at(a) } else { 0.0 }))
    }

    /// How far below its character's feet actor `id`'s body is drawn ([`drawn_sink`]; the
    /// player is `FormId::PLAYER_REF`); for an actor drawn standing without one, how far
    /// below its feet it was last drawn ([`PhysState::standing_drawn_feet`]); else 0.
    pub fn drawn_sink_of(&self, id: u32) -> f32 {
        if id == fnv_sim::FormId::PLAYER_REF.0 {
            if self.player.is_some() { self.player_sink } else { 0.0 }
        } else {
            self.npcs.get(&id).map(|b| b.sink).or_else(|| self.standing.get(&id).map(|l| l.2)).unwrap_or(0.0)
        }
    }

    /// The height of the drawn ground under a character's feet ([`PhysState::ground_drawn`]
    /// from just above them, as far down as its gap and [`DRAWN_FLOOR_WINDOW`] reach).
    pub fn drawn_ground(&mut self, c: &Character) -> Option<f32> {
        self.ground_drawn([c.feet[0], c.feet[1], c.feet[2] + 1.0], 1.0 + c.gap + DRAWN_FLOOR_WINDOW, Some(c.key())).map(|(_, drawn)| drawn)
    }

    /// The ground under `at` within `down` (the first collision a vertical ray meets,
    /// leaving out characters, actor hit volumes and the layers characters pass
    /// through): its bare surface's height and the drawn ground's. The drawn ground is
    /// that surface, or, when it is a placed model with drawn floors, the highest of its
    /// drawn floors within [`DRAWN_FLOOR_WINDOW`] of it (`drawn::DrawnFloors`).
    pub fn ground_drawn(&mut self, at: [f32; 3], down: f32, exclude: Option<fnv_phys::Key>) -> Option<(f32, f32)> {
        self.ground_under(at, down, exclude, false).map(|g| (g.bare, g.drawn))
    }

    /// [`PhysState::ground_drawn`] under fixed collision only: dynamic bodies (clutter,
    /// wind-blown tumbleweeds) are left out. For measuring the ground itself (the
    /// `actor_feet` test aid), where a loose body resting on the spot is not the ground;
    /// characters still stand on dynamic bodies through `ground_drawn` [design, #2149].
    pub fn fixed_ground_drawn(&mut self, at: [f32; 3], down: f32) -> Option<(f32, f32)> {
        self.ground_under(at, down, None, true).map(|g| (g.bare, g.drawn))
    }

    fn ground_under(&mut self, at: [f32; 3], down: f32, exclude: Option<fnv_phys::Key>, no_dynamic: bool) -> Option<GroundUnder> {
        let mut filter = Filter { exclude_layers: fnv_phys::layers::character_ignores().union(fnv_phys::layers::actor_volumes()), no_dynamic, ..Default::default() };
        filter.exclude.extend(self.npcs.values().map(|b| b.ch.key()).chain(exclude));
        let hit = self.ray(at, [0.0, 0.0, -1.0], down, filter)?;
        Some(GroundUnder { bare: hit.point[2], drawn: self.drawn_over(&hit), shell: hit.shell })
    }

    /// The drawn ground at a downward ray's hit: the hit's height, or, on a placed model
    /// with drawn floors, the highest of them within [`DRAWN_FLOOR_WINDOW`] of it.
    pub fn drawn_over(&self, hit: &RayHit) -> f32 {
        let bare = hit.point[2];
        let drawn = match hit.owner {
            Owner::Ref { form_id, .. } => self.drawn.get(&form_id).and_then(|(floors, pose)| drawn_floor(floors, pose, hit.point)),
            _ => None,
        };
        drawn.unwrap_or(bare)
    }

    /// Bring every character's drawn sink up to date (after its physics step). A change
    /// of the drawn ground is taken up by the character's lift ([`step_lift::StepLift`]).
    pub fn update_drawn_sinks(&mut self) {
        if let Some(c) = self.player.clone() {
            let sink = drawn_sink(&c, self.drawn_ground(&c));
            if !c.swimming {
                self.player_lift.ground_moved(self.player_sink - sink);
            }
            self.player_sink = sink;
        }
        let chars: Vec<(u32, Character)> = self.npcs.iter().map(|(id, b)| (*id, b.ch.clone())).collect();
        for (id, ch) in chars {
            let sink = drawn_sink(&ch, self.drawn_ground(&ch));
            if let Some(b) = self.npcs.get_mut(&id) {
                if !ch.swimming {
                    b.lift.ground_moved(b.sink - sink);
                }
                b.sink = sink;
            }
        }
    }

    /// How far above its feet's drawn height actor `id`'s body is drawn this frame, after
    /// a step or a bump ([`step_lift::StepLift`]; the player is `FormId::PLAYER_REF`); 0
    /// for an actor without a character.
    pub fn drawn_lift_of(&self, id: u32) -> f32 {
        let a = self.draw_alpha();
        if id == fnv_sim::FormId::PLAYER_REF.0 {
            if self.player.is_some() { self.player_lift.at(a) } else { 0.0 }
        } else {
            self.npcs.get(&id).map_or(0.0, |b| b.lift.at(a))
        }
    }

    /// The share of [`PhysState::drawn_lift_of`] that changes of the drawn ground under
    /// the feet put there ([`step_lift::StepLift::ground_at`]): the view leaves it out, as
    /// it leaves out the drawn sink.
    pub fn drawn_ground_lift_of(&self, id: u32) -> f32 {
        let a = self.draw_alpha();
        if id == fnv_sim::FormId::PLAYER_REF.0 {
            if self.player.is_some() { self.player_lift.ground_at(a) } else { 0.0 }
        } else {
            self.npcs.get(&id).map_or(0.0, |b| b.lift.ground_at(a))
        }
    }

    /// Whether the player is placed and every cell asked for around them is in (the
    /// loading screen waits for this, `crate::loading`).
    pub fn loading_done(&self) -> bool {
        self.player.as_ref().is_some_and(|c| self.ready_at(c.feet)) && self.loader.pending().next().is_none()
    }

    /// Terrain height under (x, y), when that square is loaded and has terrain.
    pub fn terrain_z(&self, x: f32, y: f32) -> Option<f32> {
        if self.space != Space::Exterior {
            return None;
        }
        let (gx, gy) = cell_of(x, y);
        let s = self.cells.get(&PhysCell::Exterior(gx, gy))?.surface.as_ref()?;
        s.height_at(x - gx as f32 * CELL_SIZE, y - gy as f32 * CELL_SIZE)
    }

    /// Whether a reference has collision bodies in the world now.
    /// The trigger volumes (Trigger-layer phantoms) of a model, in the model's frame, once
    /// the model was read for a loaded cell; `None` until then (`crate::script_trigger`).
    pub fn model_triggers(&self, path: &str) -> Option<Arc<Vec<fnv_collide::CollisionObject>>> {
        self.models.triggers(path)
    }

    pub fn has_collision(&self, form_id: u32) -> bool {
        !self.world.ref_bodies(form_id).is_empty()
    }

    /// First hit of a ray (game space), never the player's own capsule. For the crosshair:
    /// `ray(player.eye(), player.look_dir(), reach, Filter::default())`.
    pub fn ray(&mut self, origin: [f32; 3], dir: [f32; 3], max_distance: f32, mut filter: Filter) -> Option<RayHit> {
        if let Some(c) = &self.player {
            filter.exclude.push(c.key());
        }
        self.world.update_queries();
        self.world.queries().ray(origin, dir, max_distance, &filter)
    }

    /// Every hit of a ray (game space), nearest first, never the player's own capsule.
    pub fn ray_all(&mut self, origin: [f32; 3], dir: [f32; 3], max_distance: f32, mut filter: Filter) -> Vec<RayHit> {
        if let Some(c) = &self.player {
            filter.exclude.push(c.key());
        }
        self.world.update_queries();
        self.world.queries().ray_all(origin, dir, max_distance, &filter)
    }

    /// What stands under `at`, from `up` above it to `down` below, ignoring the layers
    /// the character passes through and actor hit volumes.
    pub fn ground_below(&mut self, at: [f32; 3], up: f32, down: f32) -> Option<RayHit> {
        let filter = Filter { exclude_layers: fnv_phys::layers::character_ignores().union(fnv_phys::layers::actor_volumes()), ..Default::default() };
        self.ray([at[0], at[1], at[2] + up], [0.0, 0.0, -1.0], up + down, filter)
    }

    /// What preparing a cell reads.
    fn job(&self, d: &PlayData, cell: PhysCell) -> Job {
        let (record, persistent) = match cell {
            PhysCell::Exterior(x, y) => {
                let ws = d.index.worldspaces.get(&self.worldspace);
                (ws.and_then(|w| w.grid.get(&(x, y))).and_then(|id| d.index.cells.get(id)), self.persistent_by_grid.get(&(x, y)))
            }
            PhysCell::Interior(id) => (d.index.cells.get(&id), None),
        };
        let mut refs: Vec<u32> = record.map(|c| c.refs.clone()).unwrap_or_default();
        refs.extend(persistent.into_iter().flatten());
        // References placed in other squares whose bounds reach into this one (#2795).
        if let PhysCell::Exterior(x, y) = cell {
            refs.extend(self.reach.reaching((x, y)));
        }
        refs.sort_unstable();
        refs.dedup();
        let land = match cell {
            PhysCell::Exterior(..) => record.and_then(|c| c.land),
            PhysCell::Interior(_) => None,
        };
        // The neighbours' LAND, which a smoothed surface reads along its edges (#2149).
        let lands = match cell {
            PhysCell::Exterior(x, y) if land.is_some() => {
                let ws = d.index.worldspaces.get(&self.worldspace);
                fnv_world::land_surface::land_ids((x, y), |g| ws.and_then(|w| w.grid.get(&g)).and_then(|id| d.index.cells.get(id)).and_then(|c| c.land))
            }
            _ => Default::default(),
        };
        let models = refs.iter().filter_map(|r| self.destruction_models.get(r).map(|m| (*r, m.clone()))).collect();
        Job { cell, refs, land, lands, surface: crate::smooth_terrain::spec(), poses: Vec::new(), scales: Vec::new(), models }
    }

    fn source(&self, ctx: &LoadCtx) -> Source {
        Source { d: *ctx.d, data: ctx.data.cloned(), models: self.models.clone(), params: self.world.params }
    }

    /// The work of preparing `cell`, to run on any thread.
    fn work(&self, ctx: &LoadCtx, cell: PhysCell) -> loader::Work {
        let job = with_poses(self.job(ctx.d, cell), ctx.sim);
        #[cfg(test)]
        if let Some(fake) = self.fake {
            let job = Job { models: self.fake_models(), ..job };
            return Box::new(move || fake(&job));
        }
        let src = self.source(ctx);
        Box::new(move || prepare::cell(&src, &job))
    }

    /// Load one cell now, on this thread: terrain, then its references' collision by
    /// FormID (forgetting a request for it on the loader thread).
    pub fn load(&mut self, ctx: &LoadCtx, cell: PhysCell) {
        if self.cells.contains_key(&cell) {
            return;
        }
        self.loader.cancel(cell);
        let p = self.work(ctx, cell)();
        self.insert(ctx, p);
    }

    /// Put a prepared cell into the world: water, terrain, then its references in the
    /// job's (FormID) order, those not in the world now left out.
    fn insert(&mut self, ctx: &LoadCtx, p: Prepared) {
        let t0 = Instant::now();
        let Prepared { cell, surface, terrain, refs, times } = p;
        let d = ctx.d;
        self.forget_standing_near(cell);
        // An exterior cell's water (`Cell::water_surface`, the rule the renderer draws it
        // with); not an interior's XCLW, where the game swam at a placed water object's
        // height instead [observed: physics.md §6.6; placed water not modelled yet].
        if let PhysCell::Exterior(x, y) = cell {
            let ws = d.index.worldspaces.get(&self.worldspace);
            let record = ws.and_then(|w| w.grid.get(&(x, y))).and_then(|id| d.index.cells.get(id));
            let surface = record.and_then(|c| c.water_surface(ws.and_then(|w| w.default_water_height)));
            self.world.set_water(water_area(cell), surface);
        }
        if let Some(t) = terrain {
            self.world.insert_terrain(t);
        }
        let mut ids = Vec::with_capacity(refs.len());
        let mut with_collision = 0;
        for e in refs {
            ids.push(e.form_id);
            // Held by another loaded square already (its bounds reach over both, #2795):
            // that square added it, or left it out, and `sync_ref` keeps it up to date.
            let holders = self.held.entry(e.form_id).or_default();
            *holders += 1;
            if *holders > 1 {
                continue;
            }
            self.ref_cell.insert(e.form_id, cell);
            let shapes = e.shapes;
            with_collision += self.apply(ctx, e.placement, |_| shapes.expect("prepared with its placement")) as usize;
        }
        // Dropped items: the simulation's created references standing in this cell
        // (FormIDs above every placed one, so the order stays by FormID).
        let (space, world) = (self.space, self.worldspace);
        let created: Vec<u32> = ctx
            .sim
            .world
            .created()
            .filter(|(_, s)| dropped::present(s) && !fnv_game::ai::is_actor_base(ctx.sim.world.forms, s.base) && dropped::phys_cell(d.index, world, space, s) == Some(cell))
            .map(|(id, _)| id.0)
            .collect();
        for r in created {
            self.ref_cell.insert(r, cell);
            with_collision += self.add_created(ctx, r) as usize;
            ids.push(r);
        }
        let n = ids.len();
        self.cells.insert(cell, LoadedCell { refs: ids, surface });
        let insert_ms = ms_since(t0);
        self.stats.times.add(&times);
        self.stats.times.insert += insert_ms;
        let prepare_ms = times.records + times.read + times.parse + times.convert + times.shapes;
        self.stats.cells_loaded += 1;
        self.stats.load_ms += prepare_ms + insert_ms;
        debug!(
            "physics cell {cell:?} loaded: {n} references, {with_collision} with collision; prepared in {prepare_ms:.1} ms ({}), inserted in {insert_ms:.1} ms; all cells so far: {:?}",
            PrepareTimes(&times),
            self.stats.counts
        );
    }

    /// Add a reference's collision if it is in the world and has any (`shapes` builds it
    /// once it is known to be). Returns whether it now has collision.
    fn apply(&mut self, ctx: &LoadCtx, pl: Option<Placement>, shapes: impl FnOnce(&Placement) -> RefShapes) -> bool {
        let counts = &mut self.stats.counts;
        let Some(pl) = pl else {
            *counts.entry("unreadable").or_default() += 1;
            return false;
        };
        if !in_world(ctx.sim, &pl) {
            *counts.entry("not_present").or_default() += 1;
            return false;
        }
        let RefShapes { tallies, water, prepared, dynamic_local, door, animated, drawn, model } = shapes(&pl);
        for what in tallies {
            *counts.entry(what).or_default() += 1;
        }
        // Placed water (physics.md §6.6): volumes only, removed with the reference; added
        // before the bodies, in the order `fnvtool phys` uses.
        if !water.is_empty() {
            self.world.add_placed_water(&RefPose::from_placement(&pl), &water);
            *counts.entry("water_volume_refs").or_default() += 1;
        }
        let Some(prepared) = prepared else { return false };
        // A panic putting the bodies in the world (bad data met by a bug of ours,
        // `nvf_assets::guard`): the reference has no collision; the bodies added before
        // it stopped are taken out.
        let rep = match nvf_assets::guard::guarded(|| Ok(self.world.insert_ref(prepared))) {
            Ok(rep) => rep,
            Err(e) => {
                warn!("phys: 0x{:08X}: collision not added ({e}); the reference has none", pl.form_id);
                self.world.remove_ref(pl.form_id);
                *self.stats.counts.entry("add_failed").or_default() += 1;
                return false;
            }
        };
        match model {
            Some(m) => {
                self.built_models.insert(pl.form_id, m);
            }
            None => {
                self.built_models.remove(&pl.form_id);
            }
        }
        self.refs_built += 1;
        self.built_at.insert(pl.form_id, (pl.position, pl.rotation));
        let counts = &mut self.stats.counts;
        *counts.entry("with_collision").or_default() += 1;
        match drawn {
            Some(d) => {
                self.drawn.insert(pl.form_id, (d, RefPose::from_placement(&pl)));
            }
            None => {
                self.drawn.remove(&pl.form_id);
            }
        }
        if !rep.errors.is_empty() {
            *counts.entry("object_errors").or_default() += rep.errors.len();
        }
        if let (Some(local), 1) = (dynamic_local, rep.bodies) {
            let added = (pl.position, ref_rotation(pl.rotation));
            self.placed_dynamic.insert(pl.form_id, PlacedDynamic { local, scale: pl.scale, added, moved: false, rested: false });
        }
        if let Some(bodies) = door.filter(|d| !d.objects.is_empty()) {
            *counts.entry("animated_doors").or_default() += 1;
            let pose = RefPose::from_placement(&pl);
            self.doors.insert(pl.form_id, PosedDoor { pose, bodies, open_by_default: pl.open_by_default, shown: None, solid: None });
        }
        match animated.filter(|a| !a.objects.is_empty()) {
            Some(bodies) => {
                *counts.entry("animated_collision_refs").or_default() += 1;
                self.animated.insert(pl.form_id, PosedGroup { pose: RefPose::from_placement(&pl), bodies, shown: None });
            }
            None => {
                self.animated.remove(&pl.form_id);
            }
        }
        rep.bodies > 0
    }

    /// Add a reference's collision if it is in the world and has any. Returns whether it
    /// now has collision.
    fn add_ref(&mut self, ctx: &LoadCtx, r: u32) -> bool {
        // Test aid: the reference as its made-up cell has it.
        #[cfg(test)]
        if let (Some(fake), Some(&cell)) = (self.fake, self.ref_cell.get(&r)) {
            let job = with_poses(Job { refs: vec![r], models: self.fake_models(), ..self.job(ctx.d, cell) }, ctx.sim);
            let Some(e) = fake(&job).refs.into_iter().find(|e| e.form_id == r) else { return false };
            let shapes = e.shapes;
            return self.apply(ctx, e.placement, |_| shapes.expect("made up with its placement"));
        }
        let src = self.source(ctx);
        let mut times = LoadTimes::default();
        let t = Instant::now();
        let mut pl = ctx.d.index.placement(ctx.d.lo, ctx.d.schema, r).map(|pl| prepare::at_pose(pl, ctx.sim.moved_pose(r)));
        // A script's scale (SetScale) in place of the placement's; one too small to see
        // has no collision (`crate::sim::MIN_DRAWN_SCALE`).
        if let (Some(p), Some(s)) = (pl.as_mut(), ctx.sim.script_scale(r)) {
            if s <= crate::sim::MIN_DRAWN_SCALE {
                return false;
            }
            p.scale = s;
        }
        times.records += ms_since(t);
        let model = self.destruction_models.get(&r).cloned();
        let added = self.apply(ctx, pl, |pl| prepare::reference(&src, pl, model.as_deref(), &mut times));
        self.stats.times.add(&times);
        added
    }

    /// Drop a cell: its terrain and its references' collision (or its request).
    pub fn unload(&mut self, cell: PhysCell) {
        if self.loader.cancel(cell) {
            debug!("physics cell {cell:?} no longer wanted before it was ready");
        }
        let Some(c) = self.cells.remove(&cell) else { return };
        self.forget_standing_near(cell);
        self.world.set_water(water_area(cell), None);
        if let PhysCell::Exterior(x, y) = cell {
            self.world.remove_terrain((x, y));
        }
        for r in c.refs {
            // Still held by another loaded square (#2795): its collision stays, owned by
            // that square now.
            if let Some(n) = self.held.get_mut(&r) {
                *n -= 1;
                if *n > 0 {
                    if self.ref_cell.get(&r) == Some(&cell) {
                        if let Some(other) = self.cells.iter().find(|(_, o)| o.refs.contains(&r)).map(|(k, _)| *k) {
                            self.ref_cell.insert(r, other);
                        }
                    }
                    continue;
                }
                self.held.remove(&r);
            }
            self.world.remove_ref(r);
            self.created.remove(&r);
            self.placed_dynamic.remove(&r);
            self.doors.remove(&r);
            self.animated.remove(&r);
            self.drawn.remove(&r);
            self.built_models.remove(&r);
            self.built_at.remove(&r);
            self.repose_pending.remove(&r);
            if self.ref_cell.get(&r) == Some(&cell) {
                self.ref_cell.remove(&r);
            }
        }
        debug!("physics cell {cell:?} unloaded");
    }

    /// Bring a reference's collision in line with the simulation: removed when it is no
    /// longer present, added back when it is present again and its cell is loaded.
    pub fn sync_ref(&mut self, ctx: &LoadCtx, form_id: u32) {
        if FormId(form_id).is_dynamic() {
            self.sync_created(ctx, form_id);
            return;
        }
        if !ctx.sim.present(form_id) {
            self.placed_dynamic.remove(&form_id);
            self.doors.remove(&form_id);
            self.animated.remove(&form_id);
            self.drawn.remove(&form_id);
            if self.world.remove_ref(form_id) {
                debug!("collision of {form_id:08X} removed");
            }
        } else if self.ref_cell.contains_key(&form_id) && ctx.sim.script_scale(form_id).is_some() {
            // A script set its scale (SetScale): built again at that scale.
            self.placed_dynamic.remove(&form_id);
            self.doors.remove(&form_id);
            self.animated.remove(&form_id);
            self.drawn.remove(&form_id);
            self.world.remove_ref(form_id);
            if self.add_ref(ctx, form_id) {
                debug!("collision of {form_id:08X} built at scale {}", ctx.sim.ref_scale(form_id));
            }
        } else if self.ref_cell.contains_key(&form_id) && !self.has_collision(form_id) && self.add_ref(ctx, form_id) {
            debug!("collision of {form_id:08X} restored");
        }
    }

    /// Where a reference with one dynamic body is now, by its body (position and angles,
    /// as the settle write-back would put them), and whether the body sleeps. Co-op
    /// replication sends a guest's carried object to the host with it.
    pub fn ref_pose_now(&self, id: u32) -> Option<([f32; 3], [f32; 3], bool)> {
        let (local, scale) = match self.created.get(&id) {
            Some(cb) => cb.local.filter(|_| cb.dynamic).map(|l| (l, 1.0))?,
            None => self.placed_dynamic.get(&id).map(|p| (p.local, p.scale))?,
        };
        let b = self.world.ref_bodies(id).first().copied()?;
        let (pos, rot) = dropped::ref_pose_from_body(&b, &local, scale);
        Some((pos, ref_angles(&rot), b.sleeping))
    }

    /// A script moved or turned reference `id` (SetPos, SetAngle, Rotate, SetScale, the
    /// console's too) at time `now` (seconds): its collision is built again at the
    /// simulation's pose when it is not there already.
    ///
    /// - One dynamic body (placed or created clutter): built again at once, since the
    ///   settle write-back ([`Self::settle_created`], [`Self::settle_placed`]) would copy
    ///   the body's old pose over the move. Found 2026-11-01: a raised bottle read its old
    ///   height again at once [observed: GetPos Z 8396.80 after SetPos Z 8461].
    /// - Any other collision of a placed reference (fixed bodies of a STAT or SCOL,
    ///   keyframed bodies, the jointed bodies of an XRGD-posed skeleton held by
    ///   `prepare::hold_jointed`, a door's or an animated group's): physics never moves
    ///   it, so it stayed where it was built while the model was drawn at the new place
    ///   (issue #1466). A script may turn such a reference a little every frame, so it is
    ///   not built again on every move: at once only when the simulation's pose is more
    ///   than [`REPOSE_NOW_DISTANCE`] or [`REPOSE_NOW_ANGLE`] from where it was built
    ///   (`built_at`), otherwise once it has rested [`REPOSE_SETTLE`]
    ///   ([`Self::flush_reposes`]). A skeleton's stored pose is laid out again from the
    ///   new reference pose, as at load.
    ///
    /// A created reference whose collision is not one dynamic body is left where it was
    /// [engineering choice: not met yet].
    pub fn repose_ref(&mut self, ctx: &LoadCtx, id: u32, now: f64) {
        let Some(s) = ctx.sim.world.peek(FormId(id)) else { return };
        let single = match self.created.get(&id) {
            Some(cb) => cb.local.filter(|_| cb.dynamic).map(|l| (l, 1.0)),
            None => self.placed_dynamic.get(&id).map(|p| (p.local, p.scale)),
        };
        let (pos, rot) = match single {
            Some((local, scale)) => {
                let Some(b) = self.world.ref_bodies(id).first().copied() else { return };
                dropped::ref_pose_from_body(&b, &local, scale)
            }
            None if FormId(id).is_dynamic() || !self.has_collision(id) => return,
            None => {
                let Some(&(pos, angles)) = self.built_at.get(&id) else { return };
                (pos, ref_rotation(angles))
            }
        };
        let dist = (0..3).map(|i| (pos[i] - s.pos[i]).powi(2)).sum::<f32>().sqrt();
        let turn = max_diff(&ref_rotation(s.rot), &rot);
        if dist <= MOVED_DISTANCE && turn <= 1e-3 {
            self.repose_pending.remove(&id);
            return;
        }
        if single.is_none() && dist <= REPOSE_NOW_DISTANCE && turn <= REPOSE_NOW_ANGLE {
            self.repose_pending.insert(id, now);
            return;
        }
        self.rebuild_moved(ctx, id, dist);
    }

    /// Build again the collision of the references [`Self::repose_ref`] held back that
    /// no script has moved for [`REPOSE_SETTLE`] by time `now` (seconds).
    pub fn flush_reposes(&mut self, ctx: &LoadCtx, now: f64) {
        let rested: Vec<u32> = self.repose_pending.iter().filter(|(_, &t)| now - t >= REPOSE_SETTLE).map(|(&r, _)| r).collect();
        for id in rested {
            self.repose_pending.remove(&id);
            if !self.has_collision(id) {
                continue;
            }
            let (Some(s), Some(&(pos, angles))) = (ctx.sim.world.peek(FormId(id)), self.built_at.get(&id)) else { continue };
            let dist = (0..3).map(|i| (pos[i] - s.pos[i]).powi(2)).sum::<f32>().sqrt();
            if dist > MOVED_DISTANCE || max_diff(&ref_rotation(s.rot), &ref_rotation(angles)) > 1e-3 {
                self.rebuild_moved(ctx, id, dist);
            }
        }
    }

    /// Whether a script move is waiting on reference `id` to rest ([`Self::repose_ref`]).
    pub fn repose_waiting(&self, id: u32) -> bool {
        self.repose_pending.contains_key(&id)
    }

    /// Build reference `id`'s collision again at the simulation's pose, `dist` from where
    /// it was. Keyframed collision switched off (a swing trap's or a wreck's held bodies
    /// after let-go, while their object ragdoll swings: `set_ref_keyframed_solid`) stays
    /// off in the new build, so the move does not bring back a solid copy beside the
    /// ragdoll.
    fn rebuild_moved(&mut self, ctx: &LoadCtx, id: u32, dist: f32) {
        self.repose_pending.remove(&id);
        // Turning them solid reports whether any were off; they are removed next anyway.
        let off = self.world.set_ref_keyframed_solid(id, true) > 0;
        self.world.remove_ref(id);
        let built = if FormId(id).is_dynamic() {
            self.created.remove(&id);
            self.add_created(ctx, id)
        } else {
            self.placed_dynamic.remove(&id);
            self.doors.remove(&id);
            self.animated.remove(&id);
            self.drawn.remove(&id);
            self.built_at.remove(&id);
            self.add_ref(ctx, id)
        };
        if built && off {
            self.world.set_ref_keyframed_solid(id, false);
        }
        self.reposes += 1;
        // At info: the smoke scenario `script-move-dynamic-clutter` reads this line, and
        // rebuilds are few now (a held-back move waits for the reference to rest).
        info!("collision of 0x{id:08X} built again where a script moved it ({dist:.1} from its body){}", if built { "" } else { ": none now" });
    }

    /// Test aid: every destruction model, by FormID (made-up cells have no index for
    /// [`Self::job`] to list their references from).
    #[cfg(test)]
    fn fake_models(&self) -> Vec<(u32, String)> {
        let mut m: Vec<(u32, String)> = self.destruction_models.iter().map(|(r, m)| (*r, m.clone())).collect();
        m.sort_unstable();
        m
    }

    /// The destruction model reference `r`'s collision was last built from (`None`: its
    /// base object's).
    pub fn built_model(&self, r: u32) -> Option<&str> {
        self.built_models.get(&r).map(String::as_str)
    }

    /// Take the game's destruction models (`want`, by reference) and build again the
    /// collision of each reference in the world whose model changed: a car reaching the
    /// stage that names its hulk collides as the hulk, whose loose parts
    /// (`prepare::hold_jointed`) are held until the drawing lets them fall (nvforever
    /// `wrecks.rs`); ClearDestruction brings the base model's back.
    pub fn set_destruction_models(&mut self, ctx: &LoadCtx, want: HashMap<u32, String>) {
        let changed: BTreeSet<u32> = want
            .keys()
            .chain(self.built_models.keys())
            .copied()
            .filter(|r| want.get(r).map(String::as_str) != self.built_model(*r))
            .collect();
        self.destruction_models = want;
        for r in changed {
            if !self.ref_cell.contains_key(&r) || !self.has_collision(r) {
                continue;
            }
            self.placed_dynamic.remove(&r);
            self.doors.remove(&r);
            self.animated.remove(&r);
            self.drawn.remove(&r);
            self.world.remove_ref(r);
            let built = self.add_ref(ctx, r);
            match self.built_model(r) {
                Some(m) => info!("phys: 0x{r:08X} collides as its destruction model {m}{}", if built { "" } else { ": none now" }),
                None => info!("phys: 0x{r:08X} collides as its base model again{}", if built { "" } else { ": none now" }),
            }
        }
    }

    /// A created reference (dropped item): collision while it is present and the physics
    /// cell containing it is loaded, none otherwise; tracked in that cell so it unloads
    /// with it.
    fn sync_created(&mut self, ctx: &LoadCtx, id: u32) {
        let s = ctx.sim.world.peek(FormId(id)).filter(|s| dropped::present(s));
        let cell = s.and_then(|s| dropped::phys_cell(ctx.d.index, self.worldspace, self.space, s)).filter(|c| self.cells.contains_key(c));
        match cell {
            None => {
                if self.world.remove_ref(id) {
                    debug!("collision of {id:08X} removed");
                }
                self.created.remove(&id);
                self.move_created(id, None);
            }
            Some(c) => {
                self.move_created(id, Some(c));
                if !self.has_collision(id) {
                    self.add_created(ctx, id);
                }
            }
        }
    }

    /// Track a created reference in loaded physics cell `to` (or in none).
    fn move_created(&mut self, id: u32, to: Option<PhysCell>) {
        let from = self.ref_cell.get(&id).copied();
        if from == to {
            return;
        }
        if let Some(c) = from.and_then(|f| self.cells.get_mut(&f)) {
            c.refs.retain(|&r| r != id);
        }
        self.ref_cell.remove(&id);
        if let Some(c) = to.and_then(|t| self.cells.get_mut(&t).map(|c| (t, c))) {
            c.1.refs.push(id);
            self.ref_cell.insert(id, c.0);
        }
    }

    /// Add a created reference's collision: its base form's model at the simulation's
    /// pose, scale 1. Returns whether it now has collision. A model whose collision is
    /// several dynamic bodies falls as one ([`one_dynamic_body`]).
    pub fn add_created(&mut self, ctx: &LoadCtx, id: u32) -> bool {
        let Some(s) = ctx.sim.world.peek(FormId(id)).filter(|s| dropped::present(s)) else { return false };
        let counts = &mut self.stats.counts;
        let Some(path) = dropped::created_model_path(ctx.d, &ctx.sim.world, s.base, s.extra.mods) else {
            *counts.entry("created_no_model").or_default() += 1;
            return false;
        };
        let Some(data) = ctx.data else {
            *counts.entry("awaiting_collision").or_default() += 1;
            return false;
        };
        let mut times = LoadTimes::default();
        let read = self.models.objects(data, &path, &mut times);
        self.stats.times.add(&times);
        let objects = match read {
            Ok(o) if !o.is_empty() => o,
            _ => {
                *self.stats.counts.entry("created_without_collision").or_default() += 1;
                return false;
            }
        };
        let joined = one_dynamic_body(&objects).map(|o| (objects.len(), o));
        let objects = match &joined {
            Some((_, o)) => Arc::new(vec![o.clone()]),
            None => objects,
        };
        let pose = RefPose { form_id: id, position: s.pos, rotation: ref_rotation(s.rot), scale: 1.0 };
        // A panic converting or adding the bodies (`nvf_assets::guard`): no collision for
        // this reference, and nothing of it left in the world.
        let rep = match nvf_assets::guard::guarded(|| Ok(self.world.add_ref(&pose, &objects))) {
            Ok(rep) => rep,
            Err(e) => {
                warn!("collision of dropped 0x{id:08X} not added ({e}): {path}");
                self.world.remove_ref(id);
                *self.stats.counts.entry("created_without_collision").or_default() += 1;
                return false;
            }
        };
        if rep.bodies == 0 {
            *self.stats.counts.entry("object_errors").or_default() += rep.errors.len();
            return false;
        }
        *self.stats.counts.entry("created_with_collision").or_default() += 1;
        let local = (objects.len() == 1 && rep.bodies == 1).then(|| objects[0].node_transform.compose(&objects[0].body_transform));
        let dynamic = objects.iter().any(|o| o.motion == Motion::Dynamic);
        info!(
            "collision of dropped 0x{id:08X} added: {path}, {} bodies ({}), at ({:.1}, {:.1}, {:.1}){}",
            rep.bodies,
            if dynamic { "dynamic" } else { "not dynamic" },
            s.pos[0],
            s.pos[1],
            s.pos[2],
            joined.map_or_else(String::new, |(n, _)| format!(": the model's {n} dynamic bodies joined into one"))
        );
        if dynamic {
            self.lift_onto_support(id);
            self.lift_out_of_terrain(id);
        }
        let added_at = self.world.stepped_secs();
        self.created.insert(id, CreatedBody { local, dynamic, start: s.pos, added_at, settled: false, marks: 0, rested_at: None, sank: false });
        true
    }

    /// A created dynamic reference made reaching into the contact shell of what it rests on
    /// is lifted onto that shell, at its rest height: an ash or goo pile is made at its
    /// actor's feet (`fnv_game::ash_piles`), and a standing actor's feet rest the ground's
    /// shell above the bare surface, which put the pile's own shell about 1.7 units into
    /// the terrain's; pushed out a little each step, it crept at about 2 units/s and never
    /// rested [observed: smoke goo-pile-roach, 2026-11-11]. Only a reach into the shell of
    /// up to [`LIFT_ONTO_SUPPORT_MOST`] is taken up (more is not a body resting there).
    fn lift_onto_support(&mut self, id: u32) {
        let Some(low) = self.world.ref_lowest_z(id) else { return };
        let Some(c) = self.world.ref_center(id) else { return };
        let filter = Filter { exclude: vec![fnv_phys::Key::Ref(id)], no_actors: true, exclude_layers: fnv_phys::layers::character_ignores(), ..Default::default() };
        let Some(hit) = self.ray([c[0], c[1], c[2] + 100.0], [0.0, 0.0, -1.0], 100.0 + (c[2] - low) + LIFT_ONTO_SUPPORT_MOST + 10.0, filter) else { return };
        let rest = hit.point[2] + hit.shell;
        let reach = rest - low;
        if reach > 0.01 && reach <= LIFT_ONTO_SUPPORT_MOST {
            self.world.lift_ref(id, reach);
            info!("collision of dropped 0x{id:08X} lifted {reach:.1} units onto the contact shell of what it rests on (made reaching into it)");
        }
    }

    /// Write the poses of moving dynamic created references back to the simulation (and,
    /// outdoors, the grid cell they are now in), so a cell reload, a snapshot or a resume
    /// finds them where physics put them. Logs each one coming to rest (its body asleep).
    /// Returns the references whose pose changed.
    pub fn settle_created(&mut self, index: &WorldIndex, world_id: u32, sim: &mut World) -> Vec<u32> {
        let mut moved = Vec::new();
        let mut lost = Vec::new();
        // Moving bodies to check against the terrain, and those that woke after resting
        // (`sinking`, #2753).
        let mut awake = Vec::new();
        let mut woke = Vec::new();
        let now = self.world.stepped_secs();
        // Game seconds stepped since: under a slowed clock the steps are shorter, so the steps
        // times the step now would count the ones before at the wrong length.
        let elapsed = |since: f64| (self.world.stepped_secs() - since) as f32;
        for (&id, cb) in self.created.iter_mut() {
            let Some(local) = cb.local.filter(|_| cb.dynamic) else { continue };
            let Some(b) = self.world.ref_bodies(id).first().copied() else { continue };
            let (pos, rot) = dropped::ref_pose_from_body(&b, &local, 1.0);
            if !b.sleeping && !cb.sank {
                awake.push((id, b.linvel));
            }
            let t = elapsed(cb.added_at);
            if CREATED_MARKS.get(cb.marks).is_some_and(|&m| t >= m) {
                cb.marks += 1;
                info!(
                    "dropped 0x{id:08X} {t:.1} s after its collision was added: at ({:.1}, {:.1}, {:.1}), {:.1} below the start, speed {:.0} units/s{}",
                    pos[0],
                    pos[1],
                    pos[2],
                    cb.start[2] - pos[2],
                    b.linvel.iter().map(|v| v * v).sum::<f32>().sqrt(),
                    if b.sleeping { ", at rest" } else { "" }
                );
            }
            if pos[2] < cb.start[2] - CREATED_FELL_THROUGH {
                lost.push(id);
                continue;
            }
            let Ok(s) = sim.get(FormId(id)) else { continue };
            let dist = (0..3).map(|i| (pos[i] - s.pos[i]).powi(2)).sum::<f32>().sqrt();
            if dist > 0.01 || max_diff(&ref_rotation(s.rot), &rot) > 1e-4 {
                s.pos = pos;
                s.rot = ref_angles(&rot);
                // Outdoors the reference is in the grid cell containing it (as an actor
                // is after a teleport into a worldspace [observed, simulation.md §5]).
                let world = s.cell.and_then(|c| index.cells.get(&c.0)).and_then(|c| c.world);
                if let Some(&cell) = world.and_then(|w| index.worldspaces.get(&w)).and_then(|w| w.grid.get(&cell_of(pos[0], pos[1]))) {
                    s.cell = Some(FormId(cell));
                }
                moved.push(id);
            }
            if b.sleeping && !cb.settled {
                cb.settled = true;
                cb.rested_at = Some(now);
                info!(
                    "dropped 0x{id:08X} came to rest at ({:.1}, {:.1}, {:.2}), {:.1} below where its collision was added, {:.2} s after",
                    pos[0],
                    pos[1],
                    pos[2],
                    cb.start[2] - pos[2],
                    elapsed(cb.added_at)
                );
            } else if !b.sleeping {
                if cb.settled {
                    woke.push((id, cb.rested_at.map_or(0.0, |r| (now - r) as f32), pos));
                }
                cb.settled = false;
            }
        }
        self.note_created_sinking(woke, awake);
        for id in lost {
            let start = self.created.remove(&id).map(|c| c.start);
            self.world.remove_ref(id);
            if let (Ok(s), Some(start)) = (sim.get(FormId(id)), start) {
                s.pos = start;
            }
            warn!("dropped 0x{id:08X} fell through the world: collision removed, put back where it was dropped");
        }
        // Outdoors a reference that rolled into another grid square moves to that
        // square's physics cell (when it is loaded), so it unloads with it.
        if self.space == Space::Exterior {
            for &id in &moved {
                let to = sim.peek(FormId(id)).and_then(|p| dropped::phys_cell(index, world_id, self.space, p));
                if to.is_some_and(|c| self.cells.contains_key(&c)) {
                    self.move_created(id, to);
                }
            }
        }
        moved
    }

    /// Write the poses of placed dynamic references that physics has moved back to the
    /// simulation (see the module notes); logs each one's first move. Returns the
    /// references whose pose changed.
    pub fn settle_placed(&mut self, sim: &mut World) -> Vec<u32> {
        let mut changed = Vec::new();
        for (&id, p) in self.placed_dynamic.iter_mut() {
            if self.follow_only.contains(&id) {
                continue;
            }
            let Some(b) = self.world.ref_bodies(id).first().copied() else { continue };
            if b.sleeping && !p.moved {
                continue;
            }
            let (pos, rot) = dropped::ref_pose_from_body(&b, &p.local, p.scale);
            if !p.moved {
                let dist = (0..3).map(|i| (pos[i] - p.added.0[i]).powi(2)).sum::<f32>().sqrt();
                if dist < MOVED_DISTANCE && max_diff(&rot, &p.added.1) < MOVED_ROTATION {
                    continue;
                }
                p.moved = true;
                debug!("placed {id:08X} moved by physics: at ({:.1}, {:.1}, {:.1}), {dist:.1} from where it was added", pos[0], pos[1], pos[2]);
            }
            if b.sleeping != p.rested {
                p.rested = b.sleeping;
                if b.sleeping {
                    let from = (0..3).map(|i| (pos[i] - p.added.0[i]).powi(2)).sum::<f32>().sqrt();
                    debug!("placed {id:08X} came to rest at ({:.1}, {:.1}, {:.1}), {from:.1} from where it was added", pos[0], pos[1], pos[2]);
                }
            }
            let Ok(s) = sim.get(FormId(id)) else { continue };
            let dist = (0..3).map(|i| (pos[i] - s.pos[i]).powi(2)).sum::<f32>().sqrt();
            if dist > 0.01 || max_diff(&ref_rotation(s.rot), &rot) > 1e-4 {
                s.pos = pos;
                s.rot = ref_angles(&rot);
                changed.push(id);
            }
        }
        changed
    }

    /// Unload everything and switch to `space`.
    /// Give actor `id` a character standing on what is under `feet` (a ray from a little
    /// above them, as actors are placed under furniture and roofs). False when the
    /// collision around `feet` is not loaded or nothing is under them. The character
    /// passes through actor hit volumes (its own included).
    pub fn add_npc(&mut self, id: u32, feet: [f32; 3]) -> bool {
        if self.npcs.contains_key(&id) {
            return true;
        }
        if !self.ready_at(feet) {
            return false;
        }
        let Some(hit) = self.ground_below(feet, 48.0, 400.0) else { return false };
        let params = npc_params(self.actor_heights.get(&id).copied());
        let start = [feet[0], feet[1], hit.point[2] + params.rest_gap(hit.shell)];
        let mut ch = self.world.add_character(id, params, start);
        ch.ignore_layers = fnv_phys::layers::character_ignores().union(fnv_phys::layers::actor_volumes());
        // A stuck actor's move is not swept again every step (`fnv_phys` `Held`, issue #1797).
        ch.hold_when_stuck = true;
        let standing_height = ch.params.height;
        let mut b = NpcBody { prev_feet: ch.feet, ch, velocity: [0.0, 0.0], jump: false, sink: 0.0, lift: Default::default(), standing_height, sneak_crouch: false };
        b.sink = drawn_sink(&b.ch, self.drawn_ground(&b.ch));
        self.npcs.insert(id, b);
        true
    }

    /// The reference whose keyframed collision (a gate, a force field, not a door:
    /// something an animation or a script moves away) stands on the straight way from
    /// `from` to `to`, feet to feet, seen at `height` above them; actors left out.
    pub fn keyframed_between(&mut self, from: [f32; 3], to: [f32; 3], height: f32) -> Option<u32> {
        let a = [from[0], from[1], from[2] + height];
        let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 1e-3 {
            return None;
        }
        let dir = [d[0] / len, d[1] / len, d[2] / len];
        let filter = Filter { no_actors: true, no_terrain: true, ..Default::default() };
        let hits = self.ray_all(a, dir, len, filter);
        // A door's leaf is the doors' own (`crate::doors`): an actor opens it and walks
        // through, and the open leaf's edge is skipped past like any other obstacle
        // (Boone after his room's door, follower-boone smoke).
        hits.into_iter().find_map(|h| match h.owner {
            fnv_phys::world::Owner::Ref { form_id, object, .. } if !self.doors.contains_key(&form_id) && self.world.ref_object_keyframed(form_id, object) => Some(form_id),
            _ => None,
        })
    }

    /// Feet where actor `id`'s character could be put down near `at`, a place on the
    /// navmesh ([`PhysWorld::character_spot`]): collision loaded around it, ground on that
    /// level, and room for the body, which the player and other characters take up.
    /// Changes nothing: [`PhysState::place_npc`] puts it there.
    pub fn npc_spot(&mut self, id: u32, at: [f32; 3]) -> Result<[f32; 3], NpcSpotRefused> {
        if !self.ready_near(at, NPC_SPOT_MARGIN) {
            return Err(NpcSpotRefused::NotLoaded);
        }
        let Some(b) = self.npcs.get(&id) else { return Err(NpcSpotRefused::NotLoaded) };
        self.world.character_spot(&b.ch, at).map_err(|e| match e {
            fnv_phys::SpotRefused::NoSupport => NpcSpotRefused::NoSupport,
            fnv_phys::SpotRefused::NoRoom => NpcSpotRefused::NoRoom,
        })
    }

    /// Move actor `id`'s character to exactly `feet` (no ground search: what
    /// [`PhysState::npc_spot`] found is where it goes), standing still and not drawn
    /// sliding there. False when it has no character.
    pub fn place_npc(&mut self, id: u32, feet: [f32; 3]) -> bool {
        let Some(mut b) = self.npcs.remove(&id) else { return false };
        self.world.place_character(&mut b.ch, feet);
        b.prev_feet = feet;
        b.velocity = [0.0, 0.0];
        b.jump = false;
        b.lift = Default::default();
        b.sink = drawn_sink(&b.ch, self.drawn_ground(&b.ch));
        self.npcs.insert(id, b);
        true
    }

    pub fn remove_npc(&mut self, id: u32) {
        if let Some(b) = self.npcs.remove(&id) {
            self.left.insert(id, (b.ch.feet, b.sink));
            self.world.remove_character(b.ch);
        }
    }

    /// Forget what [`PhysState::standing_drawn_feet`] found for actors standing in or next
    /// to `cell`, whose collision came or went (the support under them may have changed).
    fn forget_standing_near(&mut self, cell: PhysCell) {
        match cell {
            PhysCell::Exterior(x, y) => self.standing.retain(|_, (feet, _, _)| {
                let (gx, gy) = cell_of(feet[0], feet[1]);
                (gx - x).abs() > 1 || (gy - y).abs() > 1
            }),
            PhysCell::Interior(_) => self.standing.clear(),
        }
    }

    /// Where actor `id`, standing at `feet` without a character, is drawn. Feet where a
    /// character rests (the contact shell above the collision under them, within
    /// [`REST_TOLERANCE`]: where a walk by physics, the arrival rule or a placement on
    /// the ground left them) are drawn as a character's would be, on the drawn ground
    /// ([`PhysState::ground_drawn`]). Feet where its character was taken away from it
    /// ([`PhysState::remove_npc`]) are drawn as the character was. Others (a placed
    /// height, the navmesh's height) are drawn at `feet`, except that with `on_landscape`
    /// (not for an actor under the furniture's control) feet standing on the bare
    /// landscape follow the smoothed surface: drawn as far above it as they were above the
    /// stored triangles ([`PhysState::landscape_lift`], #2149). Looked up again when the
    /// feet or `on_landscape` change and when collision comes or goes next to them.
    pub fn standing_drawn_feet(&mut self, id: u32, feet: [f32; 3], on_landscape: bool) -> [f32; 3] {
        let sink = match self.standing.get(&id) {
            Some((at, on, sink)) if *at == feet && *on == on_landscape => *sink,
            _ => {
                let down = 1.0 + fnv_phys::TERRAIN_SHELL + REST_TOLERANCE;
                let physics = match self.left.get(&id) {
                    Some((at, sink)) if *at == feet => Some(*sink),
                    Some(_) => {
                        self.left.remove(&id);
                        None
                    }
                    None => None,
                };
                let sink = match physics {
                    Some(sink) => sink,
                    None => self.authored_sink(feet, down, on_landscape),
                };
                self.standing.insert(id, (feet, on_landscape, sink));
                sink
            }
        };
        [feet[0], feet[1], feet[2] - sink]
    }

    /// [`PhysState::standing_drawn_feet`]'s sink for feet that did not come from a
    /// character: on the drawn ground when they rest on the collision, else lifted with
    /// the landscape when `on_landscape` and they stand on it, else none.
    fn authored_sink(&mut self, feet: [f32; 3], down: f32, on_landscape: bool) -> f32 {
        match self.ground_under([feet[0], feet[1], feet[2] + 1.0], down, None, false) {
            Some(g) if (feet[2] - g.bare - g.shell).abs() <= REST_TOLERANCE => feet[2] - g.drawn,
            _ if on_landscape => self.landscape_lift(feet).map_or(0.0, |lift| -lift),
            _ => 0.0,
        }
    }

    /// How far the smoothed landscape surface lies above the stored LAND triangles under
    /// `feet` (#2149; `Surface::lift_at`), when the feet stand on the bare landscape:
    /// outdoors, within [`TERRAIN_STAND_BAND`] of the stored triangles, and with the
    /// landscape the first collision below them (not a static, furniture or another
    /// placed floor). `None` otherwise, and on the stored surface.
    pub fn landscape_lift(&mut self, feet: [f32; 3]) -> Option<f32> {
        if self.space != Space::Exterior {
            return None;
        }
        let (gx, gy) = cell_of(feet[0], feet[1]);
        let s = self.cells.get(&PhysCell::Exterior(gx, gy))?.surface.as_ref().filter(|s| s.is_smoothed())?;
        let (x, y) = (feet[0] - gx as f32 * CELL_SIZE, feet[1] - gy as f32 * CELL_SIZE);
        let (stored, lift) = (s.stored_height_at(x, y)?, s.lift_at(x, y)?);
        if (feet[2] - stored).abs() > TERRAIN_STAND_BAND {
            return None;
        }
        // From over the feet and the surface down past the surface: the landscape first.
        let top = feet[2].max(stored + lift) + 1.0;
        let mut filter = Filter { exclude_layers: fnv_phys::layers::character_ignores().union(fnv_phys::layers::actor_volumes()), ..Default::default() };
        filter.exclude.extend(self.npcs.values().map(|b| b.ch.key()));
        let hit = self.ray([feet[0], feet[1], top], [0.0, 0.0, -1.0], top - (stored + lift) + 2.0, filter)?;
        matches!(hit.owner, Owner::Terrain { .. }).then_some(lift)
    }

    pub fn reset(&mut self, space: Space) {
        for id in self.npcs.keys().copied().collect::<Vec<_>>() {
            self.remove_npc(id);
        }
        self.standing.clear();
        self.left.clear();
        self.loader.clear();
        let all: Vec<PhysCell> = self.cells.keys().copied().collect();
        for c in all {
            self.unload(c);
        }
        self.space = space;
    }

    /// Unload what is out of range, request the missing cells around `feet` from the
    /// loader (nearest first), load the cell under `feet` now if it is not in, and insert
    /// up to `budget` prepared cells, in request order.
    pub fn stream(&mut self, ctx: &LoadCtx, feet: [f32; 3], budget: usize) {
        let mut known: Vec<PhysCell> = self.cells.keys().copied().collect();
        known.extend(self.loader.pending());
        let plan = cells::plan(self.space, &known, feet);
        for c in plan.unload {
            self.unload(c);
        }
        let own = self.cell_at(feet);
        for c in plan.load {
            if c != own {
                let work = self.work(ctx, c);
                if !self.loader.request(c, work) {
                    self.load(ctx, c);
                }
            }
        }
        if !self.cells.contains_key(&own) {
            // The start, or the loader fell behind the player: never let the player stand
            // where there is no collision yet.
            if self.player.is_some() {
                self.stats.loaded_inline += 1;
                info!("physics cell {own:?} under the player was not ready: loading it now");
            }
            self.load(ctx, own);
        }
        self.loader.poll();
        for _ in 0..budget {
            let Some(p) = self.loader.pop_ready() else { break };
            self.insert(ctx, p);
        }
    }

    /// Wait for the loader to finish every requested cell and insert them all (tests).
    #[cfg(test)]
    pub(crate) fn finish_loading(&mut self, ctx: &LoadCtx) {
        self.loader.wait_all();
        while let Some(p) = self.loader.pop_ready() {
            self.insert(ctx, p);
        }
    }

    /// Put the player's character (created on first use) with its feet on whatever is
    /// under `feet`, searching from `up` above to `down` below, on that support's contact
    /// shell (`CharacterParams::rest_gap`, where the controller keeps it and where
    /// `fnvtool phys walk` starts), or at `feet` when nothing is there. Returns the hit.
    pub fn place_player(&mut self, feet: [f32; 3], up: f32, down: f32) -> Option<RayHit> {
        let hit = self.ground_below(feet, up, down);
        let params = self.player.as_ref().map_or_else(CharacterParams::default, |c| c.params);
        let at = hit.as_ref().map_or(feet, |h| [feet[0], feet[1], h.point[2] + params.rest_gap(h.shell)]);
        match self.player.as_mut() {
            Some(c) => self.world.place_character(c, at),
            None => self.player = Some(self.world.add_character(0, CharacterParams::default(), at)),
        }
        self.stands_placed(hit.is_some());
        self.prev_feet = at;
        self.stepper = Stepper::default();
        hit
    }

    /// A player put on something stands on it: it counts as on the ground until its next
    /// physics step says otherwise. Placed characters start in the air (`fnv_phys`), and
    /// the steps wait behind a loading screen, dialogue or menus: the body played the
    /// jump loop meanwhile and a landing after it, as if the player had dropped from a
    /// height on arrival (playtest 2026-10-12, item 10).
    fn stands_placed(&mut self, on_support: bool) {
        if let Some(c) = self.player.as_mut().filter(|_| on_support) {
            c.grounded = true;
            c.ground_gap = Some(0.0);
        }
    }

    /// Put the player's character (created on first use) where an actor teleported to
    /// `feet` comes to rest: `fnv_phys`'s arrival rule, the one `fnvtool sim run` applies
    /// (physics.md §6.2).
    pub fn arrive_player(&mut self, feet: [f32; 3]) -> Arrival {
        let params = self.player.as_ref().map_or_else(CharacterParams::default, |c| c.params);
        let mut filter = Filter { exclude_layers: fnv_phys::layers::character_ignores(), ..Default::default() };
        if let Some(c) = &self.player {
            filter.exclude.push(c.key());
        }
        let a = self.world.arrival(feet, &params, &filter);
        match self.player.as_mut() {
            Some(c) => self.world.place_character(c, a.feet),
            None => self.player = Some(self.world.add_character(0, params, a.feet)),
        }
        self.stands_placed(a.support.is_some());
        self.prev_feet = a.feet;
        self.stepper = Stepper::default();
        a
    }
}

/// WRLD PNAM bit 2, "Use Map Data" [documented: xEdit, schemas/fnv/world.json]: the map
/// is not physics' concern.
const PNAM_USE_MAP_DATA: u16 = 1 << 2;

/// The water area of a physics cell.
fn water_area(cell: PhysCell) -> WaterArea {
    match cell {
        PhysCell::Exterior(x, y) => WaterArea::Exterior(x, y),
        PhysCell::Interior(_) => WaterArea::Interior,
    }
}

/// The physics streaming systems (Physics set); the player's systems run after them.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysSystems;

pub struct PhysPlugin;

impl Plugin for PhysPlugin {
    fn build(&self, app: &mut App) {
        if let Some(t) = trace::PhysTrace::from_env() {
            app.insert_resource(t);
        }
        app.init_resource::<PhysState>()
            .init_resource::<ObjectPoses>()
            .init_resource::<FrameTimes>()
            .add_systems(Startup, init_phys)
            .add_systems(Last, (frame_times, report).chain())
            .add_message::<crate::RefRepose>()
            .add_systems(Update, (travel, ref_changed, destruction_models, stream, pose_doors, pose_groups).chain().in_set(PhysSystems).in_set(crate::PlaySet::Physics))
            .add_systems(PostUpdate, trace::trace);
        // The physics rate, the slow clock's pace and the cost guard (`crate::phys_rate`).
        crate::phys_rate::setup(app);
    }
}

/// The slowest clock the physics step follows down: under this share of full speed the
/// step stays this share of the usual one [design: a kill camera's slowest world,
/// `kill_cam::SLOWEST`].
pub const SLOWEST_PACED: f32 = 0.02;

/// The fastest clock the physics adds steps for: faster than this many times full speed
/// (a script's SetGlobalTimeMultiplier) the step grows instead, so the steps a real second
/// never pass this many times the rate [design: bounds the physics' cost at any clock;
/// until 2026-11-11 a faster clock kept the usual step at any speed, which at 10× ran 600
/// steps a real second].
pub const FASTEST_PACED: f32 = 4.0;

/// The shortest step the physics takes, seconds: 1/240 s, the full-speed step of the
/// highest rate the menu offered until 2026-11-11 (it now stops at 160,
/// `crate::phys_rate::MAX_HZ`); it stays as the floor of a slowed clock's step. rapier keeps positions in f32 metres from the
/// worldspace's origin, and a shorter step moves a slow body by less than the position can
/// show, so it stands still [observed: `probe_slow_steps_far_from_the_origin`,
/// crates/fnv-phys/tests/synthetic.rs, 2026-11-11: at Goodsprings (x -68,000) a body at
/// 5-20 units/s did not move at all over steps of 1/3000 s, the step of a 2 % kill camera
/// at 60 Hz; at 1/240 s it moved 82-102 % of its way; 150,000 units out, 1/3000 s stopped
/// everything under 200 units/s]. Under a clock slower than a quarter (VATS, kill cameras)
/// there are fewer steps a real second than the rate: what moves is drawn between steps
/// (`fnv_phys::drawn`), so it moves smoothly still, and each step stays one the positions
/// can resolve.
pub const SHORTEST_STEP: f32 = 1.0 / 240.0;

/// The fixed step for a clock running at `speed` of full speed, the usual step `full`
/// (`1/rate`, `crate::phys_rate`): that share of it while the clock runs slow, so the
/// physics takes as many steps a real second as at full speed, down to [`SHORTEST_STEP`];
/// the usual one at full speed, stopped or up to [`FASTEST_PACED`]; past that, longer by
/// what the clock is over it.
pub fn paced_step(full: f32, speed: f32) -> f32 {
    if speed > 0.0 && speed < 1.0 {
        (full * speed.max(SLOWEST_PACED)).max(SHORTEST_STEP.min(full))
    } else if speed > FASTEST_PACED {
        full * speed / FASTEST_PACED
    } else {
        full
    }
}

fn init_phys(d: Res<PlayWorld>, mut phys: ResMut<PhysState>) {
    phys.set_world(&d, d.world);
    phys.space = match d.interior {
        Some(id) => Space::Interior(id),
        None => Space::Exterior,
    };
}

/// Door travel: drop every cell, switch worldspace if the destination lies in another,
/// load the destination's own cell now and put the player there by the arrival rule (the
/// rest streams in over the next frames).
#[allow(clippy::too_many_arguments)]
fn travel(
    mut msgs: MessageReader<Travel>,
    d: Res<PlayWorld>,
    sim: Res<SimState>,
    data: Option<Res<GameData>>,
    mut phys: ResMut<PhysState>,
    mut player: ResMut<Player>,
    mut location: ResMut<Location>,
) {
    let Some(t) = msgs.read().last().copied() else { return };
    let to = t.destination(*location);
    let space = match to.interior {
        Some(id) => Space::Interior(id),
        None => Space::Exterior,
    };
    phys.reset(space);
    if to.world != phys.worldspace {
        info!("physics: worldspace {:08X} -> {:08X} ({})", phys.worldspace, to.world, world_name(&d, to.world));
        phys.set_world(&d, to.world);
    }
    *location = to;
    let ctx = LoadCtx { d: &d, sim: &sim, data: data.as_deref() };
    // Outdoors the squares around the arrival too, before the player is placed: a
    // reference placed in a neighbouring square can be the floor at the door (#1584:
    // Vault 3's way out (0x000EC6EB to WastelandNVmini) lands on NVVault3Entrance, placed
    // in the square to the west, so the player was put on the terrain inside it), as the start waits for them
    // (`ready_around`, #1103).
    let centre = phys.cell_at(t.feet);
    let around: Vec<PhysCell> = match centre {
        PhysCell::Exterior(cx, cy) => cells::wanted((cx, cy)).into_iter().map(|(x, y)| PhysCell::Exterior(x, y)).collect(),
        c => vec![c],
    };
    for cell in around {
        phys.load(&ctx, cell);
    }
    let hit = phys.arrive_player(t.feet).support;
    let feet = phys.player.as_ref().map_or(t.feet, |c| c.feet);
    player.feet = feet;
    player.placed = Some(feet);
    player.heading = t.heading_deg.to_radians();
    if let Some(p) = t.pitch_deg {
        player.pitch = p.to_radians().clamp(-crate::player::MAX_PITCH, crate::player::MAX_PITCH);
    }
    info!(
        "travel to {}: feet ({:.0}, {:.0}, {:.2}), floor {}, terrain {}",
        to.interior.map_or_else(|| format!("worldspace {:08X} ({})", to.world, world_name(&d, to.world)), |c| format!("interior {c:08X}")),
        feet[0],
        feet[1],
        feet[2],
        hit.map_or("not found".to_string(), |h| format!("z {:.2} (shell {:.2})", h.point[2], h.shell)),
        phys.terrain_z(feet[0], feet[1]).map_or("-".to_string(), |z| format!("z {z:.2}"))
    );
}

/// A worldspace's Editor ID for logs.
fn world_name(d: &PlayData, world: u32) -> &str {
    d.index.worldspaces.get(&world).and_then(|w| w.edid.as_deref()).unwrap_or("?")
}

/// References whose collision should follow the simulation: those named by
/// `RefChanged`, and the weapons actors drop when they die, are disarmed or break one
/// (`crate::combat::WeaponBroken`, whose reference would otherwise get no collision and
/// hang where it was placed until its cell loads again).
fn ref_changed(
    mut msgs: MessageReader<RefChanged>,
    mut repose: MessageReader<crate::RefRepose>,
    (broken, mut broken_seen): (Option<Res<Messages<crate::combat::WeaponBroken>>>, Local<bevy::ecs::message::MessageCursor<crate::combat::WeaponBroken>>),
    d: Res<PlayWorld>,
    (sim, time): (Res<SimState>, Res<Time>),
    data: Option<Res<GameData>>,
    mut phys: ResMut<PhysState>,
) {
    let mut ids: Vec<u32> = msgs.read().map(|m| m.form_id).collect();
    if let Some(broken) = broken.as_deref() {
        ids.extend(broken_seen.read(broken).filter_map(|b| b.dropped).map(|r| r.0));
    }
    let moved: BTreeSet<u32> = repose.read().map(|m| m.form_id).collect();
    if ids.is_empty() && moved.is_empty() && phys.repose_pending.is_empty() {
        return;
    }
    let now = time.elapsed_secs_f64();
    let ctx = LoadCtx { d: &d, sim: &sim, data: data.as_deref() };
    for id in ids {
        phys.sync_ref(&ctx, id);
    }
    for id in moved {
        phys.repose_ref(&ctx, id, now);
    }
    phys.flush_reposes(&ctx, now);
}

/// How often the game's destruction models are looked at, seconds [design: as nvforever's
/// `destroyed.rs` swaps the drawn model].
const DESTRUCTION_LOOK_EVERY: f32 = 0.25;

/// Destroyed references collide as their destruction model ([`PhysState::set_destruction_models`]).
fn destruction_models(
    time: Res<Time>,
    mut wait: Local<f32>,
    script: Option<Res<crate::script::ScriptState>>,
    d: Res<PlayWorld>,
    sim: Res<SimState>,
    data: Option<Res<GameData>>,
    mut phys: ResMut<PhysState>,
) {
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    *wait = DESTRUCTION_LOOK_EVERY;
    let Some(game) = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game) else { return };
    let want: HashMap<u32, String> = game.damaged_refs().filter_map(|r| game.destruction_model(r).map(|m| (r.0, m.to_string()))).collect();
    if want.is_empty() && phys.destruction_models.is_empty() && phys.built_models.is_empty() {
        return;
    }
    let ctx = LoadCtx { d: &d, sim: &sim, data: data.as_deref() };
    phys.set_destruction_models(&ctx, want);
}

/// An ash or goo pile passes through the ragdoll of the actor it came from: it is made
/// where that body lies, and the body would otherwise shove it aside or down through the
/// floor before the critical stage culls it (issue #2030; ash-piles.md §3) [design]. Runs
/// before the frame's fixed steps, so a pile's first step already passes.
pub(crate) fn piles_pass_their_corpses(mut phys: ResMut<PhysState>, script: Option<Res<crate::script::ScriptState>>) {
    let Some(rt) = script.as_deref().and_then(|s| s.rt.as_ref()) else { return };
    let phys = &mut *phys;
    for &id in phys.created.keys() {
        if phys.world.ref_passes_ragdoll(id).is_some() {
            continue;
        }
        if let Some(source) = rt.game.ash_pile_source(FormId(id)) {
            phys.world.set_ref_passes_ragdoll(id, source.0);
            info!("ash pile 0x{id:08X} passes through the body of {source}");
        }
    }
}

/// The settle write-back for dropped items ([`PhysState::settle_created`]) after the
/// frame's fixed steps (in the player's chain, after its step).
pub(crate) fn settle_created(d: Res<PlayWorld>, mut sim: ResMut<SimState>, mut phys: ResMut<PhysState>) {
    phys.moved_now = if phys.placed_dynamic.is_empty() { Vec::new() } else { phys.settle_placed(&mut sim.world) };
    // What is drawn between the steps this frame (after `settle_placed`, which marks
    // clutter moved).
    phys.update_between();
    if phys.created.is_empty() {
        return;
    }
    let world = phys.worldspace;
    phys.settle_created(d.index, world, &mut sim.world);
}

/// Keep the cells around the player loaded (around the start position until the
/// character is placed).
fn stream(
    d: Res<PlayWorld>,
    sim: Res<SimState>,
    data: Option<Res<GameData>>,
    player: Res<Player>,
    loading: Option<Res<crate::loading::Loading>>,
    mut phys: ResMut<PhysState>,
) {
    let t0 = Instant::now();
    let playing = phys.player.is_some();
    let feet = phys.player.as_ref().map_or(player.feet, |c| c.feet);
    let ctx = LoadCtx { d: &d, sim: &sim, data: data.as_deref() };
    // Behind a loading screen a long frame does not show: take more prepared cells.
    let budget = if loading.is_some_and(|l| l.unseen_frames()) { CELLS_BEHIND_LOADING_SCREEN } else { CELLS_PER_FRAME };
    phys.stream(&ctx, feet, budget);
    // The broad phase takes in what was inserted now rather than in the player's first
    // step, so that the frame's streaming cost is measured in one place.
    let t = Instant::now();
    phys.world.update_queries();
    phys.stats.times.queries += ms_since(t);
    let ms = ms_since(t0);
    if playing && ms > phys.stats.worst_frame_ms {
        phys.stats.worst_frame_ms = ms;
        debug!("physics streaming took {ms:.1} ms this frame on the main thread (the longest so far)");
    }
}

/// Frame times while playing, for the exit report.
#[derive(Resource, Default)]
struct FrameTimes {
    frames: u32,
    worst_ms: f64,
    over_33: u32,
}

fn frame_times(time: Res<Time>, phys: Res<PhysState>, mut f: ResMut<FrameTimes>) {
    // From the frame after the player is placed: startup loading is not walking.
    if phys.player.is_none() {
        return;
    }
    let ms = time.delta_secs_f64() * 1000.0;
    f.frames += 1;
    if f.frames > 1 {
        f.worst_ms = f.worst_ms.max(ms);
        f.over_33 += (ms > 33.3) as u32;
    }
}

fn report(mut exit: MessageReader<AppExit>, phys: Res<PhysState>, f: Res<FrameTimes>) {
    if exit.read().last().is_none() {
        return;
    }
    let s = &phys.stats;
    info!(
        "physics streaming: {} cells loaded in {:.0} ms ({}), {} on the main thread for the player; longest streaming frame while playing {:.1} ms on the main thread; frames while playing {} (longest {:.1} ms, {} over 33 ms)",
        s.cells_loaded, s.load_ms, s.times, s.loaded_inline, s.worst_frame_ms, f.frames, f.worst_ms, f.over_33
    );
    if let Some(c) = phys.player.as_ref() {
        // Ledges climbed by the controller's own step-up (`fnv_phys` `step_up_move`).
        info!("physics: the player stepped up {} times, feet at ({:.0}, {:.0}, {:.0})", c.stepped, c.feet[0], c.feet[1], c.feet[2]);
        // Steps and bumps the drawn body and view climbed smoothly (`step_lift`).
        let l = &phys.player_lift;
        info!("physics: the player's drawn height took up {} steps and bumps, lifted {:.1} units at most", l.taken_up, l.most);
        let (feet, drawn) = l.rough.rms();
        info!("physics: walking on the ground for {} steps, vertical acceleration {feet:.0} units/s2 of the feet and {drawn:.0} of the drawn height (rms), drawn/feet {:.2}", l.rough.steps, drawn / feet.max(1.0));
        // The view counts from the feet: the lift without the drawn ground's share.
        info!("physics: the view's vertical acceleration while walking {:.0} units/s2 (rms)", l.rough.view_rms());
    }
    // Loose items in water (`fnv_phys::buoyancy`): floating at the surface or sunk.
    let wet = phys.world.loose_in_water();
    if !wet.is_empty() {
        let list: Vec<String> = wet.iter().map(|(id, floats, top)| format!("{id:08X} ({}, its top {top:.0} units from the surface)", if *floats { "floats" } else { "sinks" })).collect();
        info!("physics: {} loose item bodies in water at exit: {}", wet.len(), list.join(", "));
    }
    // Drawing between steps (`fnv_phys::drawn`) and bodies taken out for NaN state
    // (`fnv_phys::quarantine`), over the run.
    let (_, kept) = phys.world.drawn_counts();
    let st = phys.world.stats();
    info!(
        "physics: drawn between steps: {} references at most in a frame, over {} frames; {kept} bodies at most kept before a step; quarantined {} bodies and {} colliders",
        phys.between.most, phys.between.frames, st.quarantined_bodies, st.quarantined_colliders
    );
    // Bodies a query's refresh had to put back into rapier's islands (`fnv_phys::islands`):
    // without it they froze, and could crash rapier later.
    info!("physics: island repairs after query refreshes: {} bodies", phys.world.refresh_stats().repair_islands);
    // Placed bodies physics has moved (pushed, shot, knocked over), as they are now.
    for (&id, p) in phys.placed_dynamic.iter().filter(|(_, p)| p.moved) {
        if let Some(b) = phys.world.ref_bodies(id).first() {
            let (pos, _) = dropped::ref_pose_from_body(b, &p.local, p.scale);
            let d = (0..3).map(|i| (pos[i] - p.added.0[i]).powi(2)).sum::<f32>().sqrt();
            info!("physics: placed {id:08X} moved by physics: {d:.1} units from where it was added, at exit");
        }
    }
}

/// Move the keyframed bodies of the loaded doors to their animation's pose
/// (`crate::doors`), not solid while the door swings, registering doors the renderer has
/// not drawn yet.
fn pose_doors(sim: Res<SimState>, mut doors: ResMut<crate::doors::Doors>, mut phys: ResMut<PhysState>) {
    let phys = &mut *phys;
    for (&id, d) in phys.doors.iter_mut() {
        if doors.get(id).is_none() {
            let Some(timing) = d.timing() else { continue };
            doors.register(id, crate::doors::initial_open(Some(&sim), id, d.open_by_default), timing);
        }
        let Some(door) = doors.get(id) else { continue };
        let (open, time) = door.motion.pose();
        d.pose(&mut phys.world, open, time, door.motion.closed());
        // The moving parts are not solid while the door swings (`DoorMotion::swinging`).
        let solid = !door.motion.swinging();
        if d.solid != Some(solid) {
            phys.world.set_ref_keyframed_solid(id, solid);
            d.solid = Some(solid);
        }
    }
}

/// One dynamic body for a created reference's model whose collision is several dynamic
/// bodies, `None` for any other (one body, or a static or keyframed one among them).
///
/// Clothing's folded ground models (armour MOD2, `GO.NIF`) are two dynamic bodies on a
/// limited hinge with the mesh skinned to them [observed: `fnvtool collide ragdoll` on
/// `armor\vaultsuit\m\go.nif`, Bone01 and Bone02]. Built apart, the bodies fell on
/// their own while the drawn item, which follows a single body only
/// ([`PhysState::settle_created`]), hung where it was dropped (issue #1604). Joined, the
/// item falls and comes to rest as one rigid piece in the folded pose its file draws
/// [design: the hinge is not simulated; a dropped item does not flop].
///
/// The first body's pose (node and body transform) is the joined body's; every body's
/// shape is a child at its pose from there; the masses add up; material and layer are
/// the first body's, the contact shell the largest.
pub fn one_dynamic_body(objects: &[fnv_collide::CollisionObject]) -> Option<fnv_collide::CollisionObject> {
    if objects.len() < 2 || objects.iter().any(|o| o.motion != Motion::Dynamic) {
        return None;
    }
    let pose = |o: &fnv_collide::CollisionObject| o.node_transform.compose(&o.body_transform);
    let first = &objects[0];
    let base = pose(first);
    let back = base.inverse();
    let children = objects.iter().map(|o| (back.compose(&pose(o)), (*o.shape).clone())).collect();
    Some(fnv_collide::CollisionObject {
        node_transform: base,
        body_transform: Iso::IDENTITY,
        mass: objects.iter().map(|o| o.mass).sum(),
        shell: objects.iter().map(|o| o.shell).fold(0.0, f32::max),
        shape: Arc::new(fnv_collide::Shape::Compound { children }),
        ..first.clone()
    })
}
