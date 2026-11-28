//! The physics world: terrain heightfields, placed references' collision bodies,
//! characters, fixed-step stepping and scene queries over rapier3d.
//!
//! Determinism (docs/spec-notes/physics.md §5): rapier is built with
//! `enhanced-determinism`; the world advances only in fixed steps of `PhysParams::dt`;
//! bodies are created in the order the caller adds them, so callers add terrain by grid
//! and references by FormID (the `fnvtool phys` commands do). Query results that can
//! list several colliders are sorted. Shapes can be built away from the world
//! ([`PreparedRef`], [`PreparedTerrain`], e.g. on a loader thread) and inserted later;
//! the order of insertion is what counts.

use crate::convert::{self, arr3, vec3, ConvertError, Leaf};
use fnv_collide::{CollisionObject, Iso, Motion, Shape};
use rapier3d::math::{Pose, Vector};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::parry::shape::TypedCompositeShape;
use rapier3d::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// World parameters. The defaults are engineering choices or [hypothesis] values to be
/// replaced by captured ones (physics.md §6 lists the capture questions).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PhysParams {
    /// Fixed step in seconds [hypothesis: the original's physics rate is unknown].
    pub dt: f32,
    /// Downward acceleration in game units/s²: 9.81 m/s² at 70 units per metre, which the
    /// game agrees with [observed: 683.8 from a 300-unit fall and 685.5 from a jump, fitted
    /// to positions against the game clock, capture mv_trial, physics.md §6.3].
    pub gravity: f32,
    /// Game units per metre. rapier runs in metres internally because several of its
    /// tolerances are absolute and tuned for metres (e.g. the character controller's
    /// ground-detection margin); lengths are divided by this on the way in and multiplied
    /// on the way out. A numerical choice, not a game fact [hypothesis: 70].
    pub units_per_metre: f32,
    /// Most fixed steps one [`Stepper::advance`] call runs; time beyond is dropped.
    pub max_substeps: u32,
}

impl Default for PhysParams {
    fn default() -> Self {
        PhysParams { dt: 1.0 / 60.0, gravity: 9.81 * 70.0, units_per_metre: 70.0, max_substeps: 8 }
    }
}

/// Contact shell of the terrain, game units: actors rest about this high above the LAND
/// surface, as they rest about 0.7 units (a stored shape radius of 0.1 times the Havok
/// scale) above static models (physics.md §6.2). Derived from in-game rest heights
/// [observed: after door teleports and drops the player rested 3.32–3.85 units above the
/// triangulated LAND surface and 0.54–1.05 above static floors, the same band shifted by
/// 2.8 units; 0.7 + 2.8 = 3.5]. That the excess is a shell (a Havok convex radius of the
/// landscape shape) rather than a raised surface is [hypothesis]; queries still report
/// the bare surface.
pub const TERRAIN_SHELL: f32 = 3.5;

/// The speed (game units a second) of a placed dynamic body's farthest point under which
/// it may fall asleep, after rapier's half second: references' dynamic bodies, not
/// ragdolls, gore, characters or bodies the wind blows. Three times rapier's own
/// (0.1 m/s, 7 units/s) [design, #2663; documented: rapier3d 0.36.1
/// `RigidBodyActivation::update_energy`]. rapier solves the contacts of one pair whose
/// normals lie within about 5° with one normal [documented: rapier3d 0.36.1
/// `contact_clustering.rs`, `COS_MERGE_ANGLE`], so clutter lying across two terrain
/// triangles that meet at a small angle (LAND's 8-unit steps over 128 units make 3.6°)
/// rocked between two tilts at 10-27 units/s and never slept [observed: issue #2663;
/// also the set-up of test `placed_clutter_on_creased_ground_falls_asleep` with rapier's
/// default threshold, 3-8 of 256 drops awake]. It changes only when such a
/// body may sleep, not how it moves awake (physics.md §5.5).
pub const PLACED_SLEEP_SPEED: f32 = 21.0;

/// `NVF_PHYS_INDOOR_PREDICTION=0` (a test aid for comparing, read once): the contact
/// prediction distance stays raised by [`TERRAIN_SHELL`] with no terrain too, as before
/// 2026-11-13 (`PhysWorld::new`, `PhysWorld::fit_prediction`).
fn raised_prediction_always() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NVF_PHYS_INDOOR_PREDICTION").is_ok_and(|v| v == "0"))
}

/// Where [`Owner::encode_segment`] puts the segment in `user_data` (above the owner's
/// 104 bits).
const SEGMENT_SHIFT: u32 = 104;

/// What a collider belongs to. Stored in the collider's `user_data`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Owner {
    /// The heightfield of the exterior cell at this grid square.
    Terrain { x: i32, y: i32 },
    /// Collision object `object` (index in the list given to [`PhysWorld::add_ref`]) of a
    /// placed reference, with its Havok collision layer.
    Ref { form_id: u32, object: u16, layer: u8 },
    /// A character added with [`PhysWorld::add_character`].
    Character { id: u32 },
    /// The hit volume of a placed actor (combat, `crate::actors`), with its Havok layer
    /// (BIPED alive, DEAD_BIP dead) and the body part (`crate::actors::WHOLE` for the
    /// single capsule).
    Actor { form_id: u32, layer: u8, part: u8 },
}

impl Owner {
    pub fn encode(self) -> u128 {
        let (kind, a, b, c) = match self {
            Owner::Terrain { x, y } => (1u128, x as u32, y as u32, 0u32),
            Owner::Ref { form_id, object, layer } => (2, form_id, object as u32, layer as u32),
            Owner::Character { id } => (3, id, 0, 0),
            Owner::Actor { form_id, layer, part } => (4, form_id, part as u32, layer as u32),
        };
        kind | (a as u128) << 8 | (b as u128) << 40 | (c as u128) << 72
    }

    /// [`Self::encode`] with `segment`, which of the owner's colliders this is (the index
    /// of a part capsule in its actor's list, of a leaf in its collision object), in bits
    /// 104..120. [`Self::decode`] ignores it; first-hit queries break exact ties by it
    /// after the owner (physics.md §5.1, [design, #1459]).
    pub fn encode_segment(self, segment: u16) -> u128 {
        self.encode() | (segment as u128) << SEGMENT_SHIFT
    }

    /// The segment [`Self::encode_segment`] stored in `v` (0 for [`Self::encode`]).
    pub fn segment(v: u128) -> u16 {
        (v >> SEGMENT_SHIFT) as u16
    }

    pub fn decode(v: u128) -> Option<Owner> {
        let a = (v >> 8) as u32;
        let b = (v >> 40) as u32;
        let c = (v >> 72) as u32;
        match v & 0xFF {
            1 => Some(Owner::Terrain { x: a as i32, y: b as i32 }),
            2 => Some(Owner::Ref { form_id: a, object: b as u16, layer: c as u8 }),
            3 => Some(Owner::Character { id: a }),
            4 => Some(Owner::Actor { form_id: a, layer: c as u8, part: b as u8 }),
            _ => None,
        }
    }

    /// The entity this collider belongs to, ignoring which of its objects it is.
    pub fn key(self) -> Key {
        match self {
            Owner::Terrain { x, y } => Key::Terrain(x, y),
            Owner::Ref { form_id, .. } => Key::Ref(form_id),
            Owner::Character { id } => Key::Character(id),
            Owner::Actor { form_id, .. } => Key::Actor(form_id),
        }
    }
}

/// An entity in the world, for filters and removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Terrain(i32, i32),
    Ref(u32),
    Character(u32),
    Actor(u32),
}

/// Placement of a reference: `p' = position + rotation · (scale · p)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RefPose {
    pub form_id: u32,
    pub position: [f32; 3],
    /// Row-major 3×3.
    pub rotation: [f32; 9],
    pub scale: f32,
}

impl RefPose {
    /// From a Layer 3 placement, with the engine's rotation convention
    /// (docs/spec-notes/world.md §4).
    pub fn from_placement(p: &fnv_world::Placement) -> RefPose {
        RefPose {
            form_id: p.form_id,
            position: p.position,
            rotation: fnv_world::transform::ref_rotation(p.rotation),
            scale: p.scale,
        }
    }
}

/// What [`PhysWorld::add_ref`] built.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AddReport {
    pub bodies: usize,
    pub colliders: usize,
    /// Leaves (or whole objects, for a bad placement) that could not be converted.
    pub errors: Vec<ConvertError>,
}

/// Counts for reports.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Stats {
    pub terrain_cells: usize,
    pub refs: usize,
    pub bodies_fixed: usize,
    pub bodies_kinematic: usize,
    pub bodies_dynamic: usize,
    /// Colliders by rapier shape type name (`"cuboid"`, `"heightfield"`, ...).
    pub colliders: BTreeMap<String, usize>,
    pub steps: u64,
    /// The solver's load (#1798): bodies awake, contact pairs touching, their manifolds
    /// and contact points the solver works on, and the joints.
    pub bodies_awake: usize,
    pub contact_pairs: usize,
    pub manifolds: usize,
    pub solver_contacts: usize,
    pub joints: usize,
    /// rapier's stage timers summed over every step so far, microseconds (all 0 without
    /// the `solver-timers` feature): the solver's serial set-up (velocity assembly), its
    /// solve (velocity resolution: the constraint iterations and writeback), collision
    /// detection, island construction, CCD, the whole rapier step. Not serialized (timings
    /// differ run to run).
    #[serde(skip)]
    pub solver_us: [u64; 6],
    /// Bodies and colliders rapier disabled so far for state that went NaN or infinite,
    /// each counted once (`crate::quarantine`).
    pub quarantined_bodies: usize,
    pub quarantined_colliders: usize,
}

/// State of one body, for readback (e.g. dynamic clutter positions).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct BodyState {
    pub translation: [f32; 3],
    /// Row-major 3×3.
    pub rotation: [f32; 9],
    pub linvel: [f32; 3],
    pub sleeping: bool,
}

/// A reference's collision objects converted to rapier shapes and placed, but not in a
/// world yet: built by [`PreparedRef::new`] (pure, so it may run on another thread) and
/// added by [`PhysWorld::insert_ref`]. Building the shapes (triangle-mesh trees, convex
/// hulls) is most of the cost of adding a reference; inserting is cheap.
pub struct PreparedRef {
    pub form_id: u32,
    objects: Vec<PreparedObject>,
    /// Conversion errors, in the order [`PhysWorld::add_ref`] reports them.
    errors: Vec<ConvertError>,
    /// Its dynamic bodies are added asleep ([`PreparedRef::asleep`]).
    asleep: bool,
}

struct PreparedObject {
    /// Index in the list given to [`PreparedRef::new`].
    index: u16,
    layer: u8,
    motion: Motion,
    /// A loose body on the Non Collidable layer, placed fixed and touching nothing
    /// ([`rooted`]).
    rooted: bool,
    material: Material,
    /// Body pose in the world, rapier units.
    pose: Pose,
    /// It responds to the weather's wind (`CollisionObject::wind`).
    wind: bool,
    leaves: Vec<Leaf>,
    /// Contact shell times the reference's scale, game units.
    shell: f32,
    /// The node the object hangs on (`CollisionObject::node`).
    node: Option<String>,
}

/// A dynamic body on the Non Collidable layer: the pickable plants White Horse Nettle and
/// Broc Flower carry one, a 45 kg box standing on its end (grab.md §3). In the original
/// nothing moved them: walking into them (the players' report, #4917), shots or a frag
/// grenade's blast beside them (observer check 278 [observed]); a nettle held with the
/// grab key and let go was read 170 units under the ground, so its body met nothing
/// there, terrain included. Placed as a dynamic body here it was knocked over when the
/// player walked into it and clinked on landing (#4917). It is placed as a fixed body that
/// has no contacts with any other (queries still see it, as they see the plants'
/// non-collidable statics) [design: the observed stillness; the grab the original allows
/// stays off, #3389].
fn rooted(o: &CollisionObject) -> bool {
    o.layer == crate::layers::NON_COLLIDABLE && o.motion == Motion::Dynamic
}

/// The surface and mass properties of a collision object.
struct Material {
    friction: f32,
    restitution: f32,
    mass: f32,
    /// Havok material code of the convex leaves (`CollisionObject::material`).
    code: Option<u32>,
}

impl PreparedRef {
    /// Convert `objects` placed by `pose` for a world with `params` (see
    /// [`PhysWorld::add_ref`]). Objects without convertible leaves are left out.
    pub fn new(params: &PhysParams, pose: &RefPose, objects: &[CollisionObject]) -> PreparedRef {
        PreparedRef::build(1.0 / params.units_per_metre, pose, objects)
    }

    /// With `k` metres per game unit.
    fn build(k: f32, pose: &RefPose, objects: &[CollisionObject]) -> PreparedRef {
        let mut out = PreparedRef { form_id: pose.form_id, objects: Vec::new(), errors: Vec::new(), asleep: false };
        let ref_rot = match convert::rotation(&pose.rotation) {
            Ok(r) => r,
            Err(e) => {
                out.errors.push(e);
                return out;
            }
        };
        let ref_t = vec3(pose.position) * k;
        if !ref_t.is_finite() {
            out.errors.push(ConvertError::BadDimension);
            return out;
        }
        let ref_pose = Pose::from_parts(ref_t, ref_rot);
        for (i, o) in objects.iter().enumerate() {
            let local = o.node_transform.compose(&o.body_transform);
            let body_pose = match convert::pose(&local, pose.scale * k) {
                Ok(p) => ref_pose * p,
                Err(e) => {
                    out.errors.push(e);
                    continue;
                }
            };
            let mut leaves = Vec::new();
            convert::shared_leaves(&o.shape, pose.scale * k, &mut leaves, &mut out.errors);
            if leaves.is_empty() {
                continue;
            }
            let rooted = rooted(o);
            out.objects.push(PreparedObject {
                index: i as u16,
                layer: o.layer,
                motion: if rooted { Motion::Static } else { o.motion },
                rooted,
                material: Material { friction: o.friction, restitution: o.restitution, mass: o.mass, code: o.material },
                pose: body_pose,
                wind: o.wind,
                leaves,
                shell: o.shell * pose.scale,
                node: o.node.clone(),
            });
        }
        out
    }

    /// Add its dynamic bodies asleep: they stay where they were put, whatever is (or is
    /// not) under them, until something wakes them — a body moving against them, a push
    /// ([`PhysWorld::push_ref`], [`PhysWorld::push_dynamic`]) or a body they touch being
    /// removed. For clutter placed in a cell (physics.md §4.3.1).
    pub fn asleep(mut self) -> PreparedRef {
        self.asleep = true;
        self
    }

    /// Colliders it will add.
    pub fn colliders(&self) -> usize {
        self.objects.iter().map(|o| o.leaves.len()).sum()
    }
}

/// The terrain collider of an exterior cell, built but not in a world yet (see
/// [`PreparedRef`]; added by [`PhysWorld::insert_terrain`]).
pub struct PreparedTerrain {
    pub grid: (i32, i32),
    collider: ColliderBuilder,
}

impl PreparedTerrain {
    /// The terrain of the cell at `grid` from its LAND heights, for a world with `params`.
    pub fn new(params: &PhysParams, grid: (i32, i32), heights: &[f32]) -> PreparedTerrain {
        PreparedTerrain::build(1.0 / params.units_per_metre, grid, heights)
    }

    /// The terrain of the cell at `grid` on its landscape surface (`surface`: the LAND grid
    /// as stored or smoothed, `fnv_world::land_surface`), for a world with `params`.
    pub fn from_surface(params: &PhysParams, grid: (i32, i32), surface: &fnv_world::land_surface::Surface) -> PreparedTerrain {
        PreparedTerrain { grid, collider: crate::terrain::collider_surface(grid, surface, 1.0 / params.units_per_metre) }
    }

    fn build(k: f32, grid: (i32, i32), heights: &[f32]) -> PreparedTerrain {
        PreparedTerrain { grid, collider: crate::terrain::collider(grid, heights, k) }
    }
}

/// What marked the world dirty, so that the next query refreshes the broad phase
/// (diagnostics only: counted in [`RefreshStats`], never read by the simulation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirtyBy {
    /// An actor's hit volume set or moved ([`PhysWorld::set_actor_volume`],
    /// [`PhysWorld::set_actor_parts`]).
    HitVolume = 0,
    /// Bodies or colliders added or removed (terrain, references, characters, gore,
    /// ragdolls, hit-volume bodies taken out).
    Bodies = 1,
    /// A body or collider moved, placed or switched on or off by hand (keyframed
    /// reference objects, placed characters and bodies).
    Transform = 2,
    /// Anything else (a ragdoll's joints cut; bodies added asleep found by a step).
    Other = 3,
}

/// Who asked for a broad-phase refresh (diagnostics only: [`RefreshStats`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryConsumer {
    /// [`PhysWorld::move_character`] (in the engine, the player's controller).
    Controller = 0,
    /// The walked actors' planned moves (the engine's `move_npcs`).
    Npc = 1,
    /// [`PhysWorld::step`], for bodies added asleep.
    Step = 2,
    /// Every other caller of [`PhysWorld::update_queries`] or [`PhysWorld::queries`]:
    /// rays, shape casts and overlaps from gameplay.
    Cast = 3,
}

/// Counts of the broad-phase refreshes ([`PhysWorld::update_queries`]) since the world was
/// made, for attributing their cost (diagnostics only: cumulative, never read by the
/// simulation; a reader takes differences between two reads).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RefreshStats {
    /// Refreshes that did work (the world was dirty).
    pub refreshes: u64,
    /// Refreshes by [`QueryConsumer`].
    pub by_consumer: [u64; 4],
    /// Refreshes whose only cause since the last refresh or step was [`DirtyBy::HitVolume`],
    /// by [`QueryConsumer`].
    pub hitvol_only: [u64; 4],
    /// Dirty marks by [`DirtyBy`] (every mark, whether or not a refresh followed).
    pub marks: [u64; 4],
    /// Refreshes with at least one mark of each [`DirtyBy`] since the last refresh or step.
    pub refreshes_with: [u64; 4],
    /// Steps that found the world dirty with no refresh since the marks: the step's own
    /// collision detection took them.
    pub step_absorbed: u64,
    /// Hit-volume capsules written by the hit-volume setters.
    pub hitvol_colliders: u64,
    /// Hit-volume capsules the setters actually wrote to rapier.
    pub hitvol_written: u64,
    /// Hit-volume capsules the setters left alone, bit-identical to the last write (the
    /// single capsule's equal-volume return counts one).
    pub hitvol_skipped: u64,
    /// Colliders in the world, summed over the refreshes (divide by `refreshes`).
    pub colliders: u64,
    /// New bodies presented to registration refreshes (historical field name).
    pub repair_new: u64,
    /// Bodies added asleep, summed over the refreshes (put back to sleep).
    pub repair_sleep: u64,
    /// Bodies struck since the last step, summed over the refreshes (woken again).
    pub repair_struck: u64,
    /// Bodies whose islands the refreshes put right after the detection
    /// (`crate::islands`): woken with their island, or a fixed body's type told again.
    pub repair_islands: u64,
}

pub struct PhysWorld {
    pub params: PhysParams,
    pub(crate) w: PhysicsWorld,
    /// Metres per game unit (rapier's internal length unit).
    pub(crate) k: f32,
    terrain: BTreeMap<(i32, i32), ColliderHandle>,
    pub(crate) refs: BTreeMap<u32, Vec<RigidBodyHandle>>,
    /// The node each body of a reference hangs on, in `refs`' order (`crate::script`).
    pub(crate) ref_nodes: BTreeMap<u32, Vec<Option<String>>>,
    /// The keyframed bodies of each reference (a subset of `refs`), kept because they are
    /// fixed bodies like the static ones and so cannot be told apart by body type. A door's
    /// moving parts are keyframed; its static frame is not.
    pub(crate) keyframed: BTreeMap<u32, Vec<RigidBodyHandle>>,
    /// The dynamic bodies of each reference that respond to the wind ([`PhysWorld::blow`]).
    pub(crate) wind: BTreeMap<u32, Vec<RigidBodyHandle>>,
    /// Actor hit volumes by reference FormID (`crate::actors`).
    pub(crate) actors: BTreeMap<u32, crate::actors::ActorBody>,
    /// Ragdolls by actor reference FormID, and the ragdoll body each of their rigid
    /// bodies is (`crate::ragdoll`).
    pub(crate) ragdolls: BTreeMap<u32, crate::ragdoll::RagdollBodies>,
    pub(crate) ragdoll_of: HashMap<RigidBodyHandle, (u32, u16)>,
    /// References whose bodies pass through one actor's ragdoll, by reference FormID:
    /// that actor's ([`PhysWorld::set_ref_passes_ragdoll`]).
    pub(crate) passes_ragdoll: HashMap<u32, u32>,
    /// Gore pieces by their id (`crate::gore`).
    pub(crate) gore: BTreeMap<u32, RigidBodyHandle>,
    /// Per-triangle material codes of triangle-mesh colliders; one code for a convex
    /// collider.
    pub(crate) materials: HashMap<ColliderHandle, Arc<[u32]>>,
    /// Contact shells (game units) of reference colliders that have one
    /// (`CollisionObject::shell` times the reference's scale).
    shells: HashMap<ColliderHandle, f32>,
    /// Colliders changed since the broad phase was last updated.
    dirty: bool,
    /// Which [`DirtyBy`] marked it since the last refresh or step (a bit each;
    /// diagnostics only).
    dirty_by: u8,
    /// Diagnostics only ([`PhysWorld::refresh_stats`]).
    refresh_stats: RefreshStats,
    /// Bodies inserted since the last step or no-time registration refresh.
    pub(crate) new_bodies: Vec<RigidBodyHandle>,
    /// Bodies struck ([`PhysWorld::push_ref`], [`PhysWorld::push_dynamic`]) or moved by
    /// setting their position ([`PhysWorld::body_placed`]) since the last step (see
    /// [`PhysWorld::update_queries`]).
    pub(crate) struck: Vec<RigidBodyHandle>,
    /// Dynamic bodies added asleep ([`PreparedRef::asleep`]) whose contacts have not been
    /// found yet (see [`PhysWorld::update_queries`]).
    pending_sleep: Vec<RigidBodyHandle>,
    pub(crate) steps: u64,
    /// Game seconds stepped: each step's `dt` summed ([`PhysWorld::stepped_secs`]).
    stepped_secs: f64,
    /// Water: cell surfaces by area and placed volumes (`crate::water`).
    pub(crate) water: crate::water::Waters,
    /// The body held by the player's grab (`crate::grab`), driven before every step.
    pub(crate) grip: Option<crate::grab::Grip>,
    /// New actors' hit volumes get colliders of no body (`crate::actors`; on by default).
    pub(crate) standalone_hit_volumes: bool,
    /// Standalone hit-volume colliders written since the last refresh or step: (collider,
    /// actor body, pose on the body), placed by `flush_hit_volumes`.
    pub(crate) hitvol_pending: Vec<(ColliderHandle, RigidBodyHandle, Pose)>,
    /// [`Stats::solver_us`], milliseconds.
    pub(crate) solver_ms: [f64; 6],
    /// The moving bodies' poses before the last step, for drawing between steps
    /// (`crate::drawn`).
    pub(crate) before: crate::drawn::Before,
    /// Bodies and colliders rapier disabled for state that went NaN or infinite
    /// (`crate::quarantine`).
    pub(crate) quarantined: crate::quarantine::Quarantined,
}

impl PhysWorld {
    pub fn new(params: PhysParams) -> PhysWorld {
        let k = 1.0 / params.units_per_metre;
        let mut w = PhysicsWorld::new();
        w.gravity = Vector::new(0.0, 0.0, -params.gravity * k);
        w.integration_parameters.dt = params.dt;
        // Contacts are kept out to the prediction distance beyond the colliders' contact
        // skins, but bodies resting on a skin wider than the prediction distance (rapier's
        // default 0.02 m, 1.4 units) never fell asleep and crept along the ground at about
        // 10 units a second [observed: the terrain's 3.5-unit skin under Goodsprings'
        // motorcycles and crates; a skin of 1.4 rested, 2 crept; test
        // `clutter_on_the_terrain_shell_falls_asleep`]. rapier keeps at most four points of
        // a contact, the deepest and then those spread farthest from it among the points
        // whose distance, skins not taken off, is within the prediction distance
        // [documented: rapier3d 0.36.1 `reduce_manifold_naive`]: on a skin wider than the
        // prediction distance only the deepest point is left. A prediction distance of the
        // default plus the skin rests them (asleep within 2 s). It is raised only while the
        // world holds terrain (`fit_prediction`): indoors it let points of a surface a unit
        // or two below the one a body lies on (a shelf board's lower edge) displace the
        // points it lies on, and clutter trembled and stayed awake (test
        // `a_book_over_a_lower_lip_falls_asleep_indoors`).
        // rapier's own until terrain is added, unless the test aid asks for the raised one.
        if raised_prediction_always() {
            w.integration_parameters.normalized_prediction_distance += TERRAIN_SHELL * k;
        }
        #[cfg(feature = "solver-timers")]
        w.physics_pipeline.counters.enable();
        // Bootstrap rapier's persistent islands on the empty world so `update_queries_for`'s
        // zero-dt refresh never races with a deferred first-step bootstrap (which would mark
        // `pending_sleep` bodies' islands sleeping while they are still in `awake_island`).
        w.step();
        PhysWorld {
            params,
            w,
            k,
            terrain: BTreeMap::new(),
            refs: BTreeMap::new(),
            ref_nodes: BTreeMap::new(),
            keyframed: BTreeMap::new(),
            wind: BTreeMap::new(),
            actors: BTreeMap::new(),
            ragdolls: BTreeMap::new(),
            ragdoll_of: HashMap::new(),
            passes_ragdoll: HashMap::new(),
            gore: BTreeMap::new(),
            materials: HashMap::new(),
            shells: HashMap::new(),
            dirty: false,
            dirty_by: 0,
            refresh_stats: RefreshStats::default(),
            new_bodies: Vec::new(),
            struck: Vec::new(),
            pending_sleep: Vec::new(),
            steps: 0,
            stepped_secs: 0.0,
            water: Default::default(),
            grip: None,
            standalone_hit_volumes: true,
            hitvol_pending: Vec::new(),
            solver_ms: [0.0; 6],
            before: Default::default(),
            quarantined: Default::default(),
        }
    }

    /// Add (or replace) the terrain of the exterior cell at `grid` from its LAND heights.
    pub fn add_terrain(&mut self, grid: (i32, i32), heights: &[f32]) {
        self.insert_terrain(PreparedTerrain::build(self.k, grid, heights));
    }

    /// Add (or replace) terrain built by [`PreparedTerrain::new`]: the same result as
    /// [`PhysWorld::add_terrain`] with the same heights.
    pub fn insert_terrain(&mut self, t: PreparedTerrain) {
        let grid = t.grid;
        self.remove_terrain(grid);
        // Dynamic bodies rest on the terrain's contact shell too (see `TERRAIN_SHELL`).
        let c = t.collider.user_data(Owner::Terrain { x: grid.0, y: grid.1 }.encode()).contact_skin(TERRAIN_SHELL * self.k);
        let h = self.w.insert_collider(c, None);
        self.terrain.insert(grid, h);
        self.fit_prediction();
        self.mark_dirty(DirtyBy::Bodies);
    }

    pub fn remove_terrain(&mut self, grid: (i32, i32)) -> bool {
        let Some(h) = self.terrain.remove(&grid) else { return false };
        self.w.remove_collider(h);
        self.fit_prediction();
        self.mark_dirty(DirtyBy::Bodies);
        true
    }

    /// rapier's own contact prediction distance while the world holds no terrain, raised
    /// by the terrain's shell while it does (see `PhysWorld::new`). With
    /// `NVF_PHYS_INDOOR_PREDICTION=0` it stays raised.
    fn fit_prediction(&mut self) {
        if raised_prediction_always() {
            return;
        }
        let own = IntegrationParameters::default().normalized_prediction_distance;
        let raised = if self.terrain.is_empty() { 0.0 } else { TERRAIN_SHELL * self.k };
        self.w.integration_parameters.normalized_prediction_distance = own + raised;
    }

    /// Add the collision objects of a placed reference: one body per object, placed at
    /// `pose ∘ node_transform ∘ body_transform`, with the reference's uniform scale baked
    /// into the shapes. Static and keyframed objects become fixed bodies, dynamic ones
    /// dynamic with the object's mass, friction and restitution (physics.md §4). Replaces an
    /// earlier add of the same reference's bodies; its placed water
    /// ([`PhysWorld::add_placed_water`]) is kept, so the two can be added in either order.
    pub fn add_ref(&mut self, pose: &RefPose, objects: &[CollisionObject]) -> AddReport {
        self.insert_ref(PreparedRef::build(self.k, pose, objects))
    }

    /// Add a reference's collision converted by [`PreparedRef::new`]: the same bodies,
    /// colliders and report, in the same order, as [`PhysWorld::add_ref`] with the same
    /// pose and objects. Replaces an earlier add of the same reference's bodies; its placed
    /// water is kept (see [`PhysWorld::add_ref`]).
    pub fn insert_ref(&mut self, p: PreparedRef) -> AddReport {
        self.remove_bodies(p.form_id);
        let mut report = AddReport { errors: p.errors, ..Default::default() };
        let mut handles = Vec::new();
        for o in p.objects {
            let owner = Owner::Ref { form_id: p.form_id, object: o.index, layer: o.layer };
            // Keyframed bodies are teleported into place (`pose_ref_object` sets the
            // position), never moved smoothly, so a fixed body is the right model. A
            // kinematic body made rapier derive a phantom velocity from a degenerate
            // collision mesh's centre of mass and fling a character standing on it, and its
            // "carried by a kinematic body" rule froze a character walking down the slope
            // [observed: the Prospector Saloon's broken door 0x00108BD7, issue #1485].
            let builder = match o.motion {
                Motion::Static | Motion::Keyframed => RigidBodyBuilder::fixed(),
                Motion::Dynamic => RigidBodyBuilder::dynamic().sleeping(p.asleep),
            };
            // A body that responds to the wind starts awake, so the wind takes it on from
            // where it was placed until it comes to rest (`blow`) [hypothesis: the
            // original's tumbleweeds near the player were rolling when a game was
            // loaded and came to rest within 15 s, o27_tumble, docs/spec-notes/wind.md].
            let blown = o.wind && o.motion == Motion::Dynamic;
            let builder = if blown { builder.sleeping(false) } else { builder };
            // A dynamic body that has not moved yet would keep the clutter touching it
            // awake (a cleaner bottle on a container's lid): asleep too.
            let hold = p.asleep && o.motion == Motion::Dynamic && !blown;
            let builder = if hold { builder.sleeping(true) } else { builder };
            let body = self.insert_body(builder.pose(o.pose).user_data(owner.encode()));
            // Kept with the reference at once, so a caller that catches a panic part way
            // (`nvf_assets::guard`) can take every body out with `remove_ref`.
            self.refs.entry(p.form_id).or_default().push(body);
            self.ref_nodes.entry(p.form_id).or_default().push(o.node.clone());
            if o.motion == Motion::Keyframed {
                self.keyframed.entry(p.form_id).or_default().push(body);
            }
            if blown {
                self.wind.entry(p.form_id).or_default().push(body);
            }
            report.colliders += self.attach(body, owner, &o.material, o.leaves, o.shell);
            if o.rooted {
                for c in self.w.bodies[body].colliders().to_vec() {
                    self.w.colliders[c].set_collision_groups(crate::actors::NO_CONTACTS);
                    self.w.colliders[c].set_solver_groups(crate::actors::NO_CONTACTS);
                }
            }
            if o.motion == Motion::Dynamic {
                self.set_mass(body, o.material.mass);
                self.heavy_unpushed(body);
            }
            if o.motion == Motion::Dynamic && !blown {
                // Sleeps under `PLACED_SLEEP_SPEED`. rapier counts a step as still while
                // half the farthest point's displacement is under its threshold times the
                // step [documented: rapier3d 0.36.1 `RigidBodyActivation::update_energy`,
                // `relative_pose_drift`], so the threshold is half the speed.
                self.w.bodies[body].activation_mut().normalized_linear_threshold = PLACED_SLEEP_SPEED * self.k * 0.5;
            }
            if hold {
                self.pending_sleep.push(body);
            }
            handles.push(body);
        }
        report.bodies = handles.len();
        if !handles.is_empty() {
            self.mark_dirty(DirtyBy::Bodies);
        }
        report
    }

    fn attach(&mut self, body: RigidBodyHandle, owner: Owner, o: &Material, leaves: Vec<Leaf>, shell: f32) -> usize {
        let n = leaves.len();
        for (i, leaf) in leaves.into_iter().enumerate() {
            let mut c = ColliderBuilder::new(leaf.shape).position(leaf.pose).user_data(owner.encode_segment(i as u16));
            if o.friction.is_finite() && o.friction >= 0.0 {
                c = c.friction(o.friction);
            }
            if o.restitution.is_finite() && o.restitution >= 0.0 {
                c = c.restitution(o.restitution);
            }
            let h = self.w.insert_collider(c, Some(body));
            if !leaf.materials.is_empty() {
                self.materials.insert(h, leaf.materials);
            } else if let Some(code) = o.code {
                // A convex leaf: one material for the whole of it.
                self.materials.insert(h, Arc::new([code]));
            }
            if shell.is_finite() && shell > 0.0 {
                self.shells.insert(h, shell);
            }
        }
        n
    }

    /// A dynamic body heavier than [`crate::character::PUSH_MAX_MASS`]: no contacts
    /// between it and the characters' kinematic bodies, so a character walking into it
    /// does not shove it (the character controller stops at it as at a wall; physics.md
    /// §6.10). Everything else still meets it as before.
    fn heavy_unpushed(&mut self, body: RigidBodyHandle) {
        let b = &self.w.bodies[body];
        if b.mass() <= crate::character::PUSH_MAX_MASS {
            return;
        }
        let groups = InteractionGroups::new(Group::ALL, Group::ALL.difference(crate::character::CHARACTER_SOLVER), InteractionTestMode::And);
        for c in b.colliders().to_vec() {
            // No contact pair at all: a character's touch neither shoves it nor wakes it
            // (awake, a car resting a little off its rest slid a few units). Queries see
            // it still (they test no groups), so the controller stops at it.
            self.w.colliders[c].set_collision_groups(groups);
            self.w.colliders[c].set_solver_groups(groups);
        }
    }

    /// Scale the colliders' densities so the body's mass is `mass`; leaves the
    /// volume-derived mass when `mass` is not positive or the shapes have no volume.
    pub(crate) fn set_mass(&mut self, body: RigidBodyHandle, mass: f32) {
        let b = &mut self.w.bodies[body];
        b.recompute_mass_properties_from_colliders(&self.w.colliders);
        let m0 = b.mass();
        if !(mass.is_finite() && mass > 0.0 && m0 > 0.0) {
            return;
        }
        let colliders: Vec<ColliderHandle> = b.colliders().to_vec();
        for c in colliders {
            let d = self.w.colliders[c].density();
            self.w.colliders[c].set_density(d * mass / m0);
        }
        self.w.bodies[body].recompute_mass_properties_from_colliders(&self.w.colliders);
    }

    /// Remove a reference from the world: its bodies and its placed water.
    pub fn remove_ref(&mut self, form_id: u32) -> bool {
        let water = self.water.remove_ref(form_id);
        self.remove_bodies(form_id) || water
    }

    /// Remove a reference's bodies only.
    fn remove_bodies(&mut self, form_id: u32) -> bool {
        self.passes_ragdoll.remove(&form_id);
        let Some(bodies) = self.refs.remove(&form_id) else { return false };
        self.ref_nodes.remove(&form_id);
        self.keyframed.remove(&form_id);
        self.wind.remove(&form_id);
        for b in bodies {
            self.remove_body(b);
        }
        self.mark_dirty(DirtyBy::Bodies);
        true
    }

    /// Let reference `form_id`'s bodies pass through actor `actor`'s ragdoll (no contacts
    /// between them) until the reference's bodies are removed. An ash pile starts where
    /// its source's body lies and would otherwise be shoved by it (ash-piles.md §3).
    pub fn set_ref_passes_ragdoll(&mut self, form_id: u32, actor: u32) {
        if self.refs.contains_key(&form_id) {
            self.passes_ragdoll.insert(form_id, actor);
        }
    }

    /// The actor whose ragdoll reference `form_id`'s bodies pass through, if any.
    pub fn ref_passes_ragdoll(&self, form_id: u32) -> Option<u32> {
        self.passes_ragdoll.get(&form_id).copied()
    }

    /// The colliders changed (the scripts' layer and collision switches, `crate::script`).
    pub(crate) fn mark_dirty_bodies(&mut self) {
        self.mark_dirty(DirtyBy::Bodies);
    }

    pub(crate) fn remove_body(&mut self, b: RigidBodyHandle) {
        if let Some(body) = self.w.bodies.get(b) {
            for c in body.colliders() {
                self.materials.remove(c);
                self.shells.remove(c);
            }
        }
        self.w.remove_body(b);
        self.before.forget(b);
        self.mark_dirty(DirtyBy::Bodies);
    }

    /// Move the keyframed body of collision object `object` (its index in the list given
    /// to [`PhysWorld::add_ref`]) of the reference placed by `pose`, as if its node's
    /// transform in the model root's frame were `node`: to `pose ∘ node ∘ body_transform`
    /// (`add_ref` uses the stored node transform). For collision that follows an animation
    /// (an opening door, `fnv_collide::anim`). The body is moved at once, so queries and
    /// the character controller see it before the next step. Returns false when the
    /// reference has no keyframed body for that object.
    pub fn pose_ref_object(&mut self, pose: &RefPose, object: u16, node: &Iso, body_transform: &Iso) -> bool {
        let Some(bodies) = self.keyframed.get(&pose.form_id) else { return false };
        let found = bodies.iter().copied().find(|&b| {
            matches!(Owner::decode(self.w.bodies[b].user_data), Some(Owner::Ref { object: o, .. }) if o == object)
        });
        let Some(b) = found else { return false };
        let (Ok(rot), Ok(local)) = (convert::rotation(&pose.rotation), convert::pose(&node.compose(body_transform), pose.scale * self.k)) else {
            return false;
        };
        let p = Pose::from_parts(vec3(pose.position) * self.k, rot) * local;
        if !p.translation.is_finite() {
            return false;
        }
        let body = &mut self.w.bodies[b];
        if *body.position() == p {
            return true;
        }
        body.set_position(p, true);
        self.before.forget(b);
        self.mark_dirty(DirtyBy::Transform);
        true
    }

    /// Whether collision object `object` of reference `form_id` is a keyframed body: one
    /// an animation or a script moves (a door's leaf, a gate, a force field), not a fixed
    /// one.
    pub fn ref_object_keyframed(&self, form_id: u32, object: u16) -> bool {
        self.keyframed.get(&form_id).is_some_and(|bodies| {
            bodies.iter().any(|&b| matches!(Owner::decode(self.w.bodies[b].user_data), Some(Owner::Ref { form_id: f, object: o, .. }) if f == form_id && o == object))
        })
    }

    /// Make a reference's keyframed bodies solid or not: when not, their colliders take
    /// no part in contacts, the character controller or queries. For a door while it
    /// swings, so its moving leaf cannot hit or push an actor (`fnv_collide::anim`).
    /// Returns how many colliders changed.
    pub fn set_ref_keyframed_solid(&mut self, form_id: u32, solid: bool) -> usize {
        let Some(bodies) = self.keyframed.get(&form_id) else { return 0 };
        let mut n = 0;
        for &b in bodies {
            let body = &self.w.bodies[b];
            for &c in body.colliders() {
                let col = &mut self.w.colliders[c];
                if col.is_enabled() != solid {
                    col.set_enabled(solid);
                    n += 1;
                }
            }
        }
        if n > 0 {
            self.mark_dirty(DirtyBy::Transform);
        }
        n
    }

    /// Push dynamic bodies within `radius` of `center` away from it (an explosion's force,
    /// combat C7) [design: the EXPL force read as a speed change in game units per second
    /// at the centre, falling linearly to 0 at the radius]. A settling ragdoll it strikes
    /// stops settling (`PhysWorld::settle_ragdoll`). Returns how many moved.
    pub fn push_dynamic(&mut self, center: [f32; 3], radius: f32, speed: f32) -> usize {
        self.push_dynamic_but(center, radius, speed, &|_| false)
    }

    /// [`World::push_dynamic`] leaving out the bodies of the references `spared` names
    /// (by FormID).
    pub fn push_dynamic_but(&mut self, center: [f32; 3], radius: f32, speed: f32, spared: &dyn Fn(u32) -> bool) -> usize {
        let k = self.k;
        let c = vec3(center) * k;
        let r = (radius * k).max(1e-4);
        // Corpses lying at rest in the blast are thrown too.
        for id in self.ragdolls_at_rest_near(center, radius) {
            self.rouse_ragdoll(id);
        }
        let mut n = 0;
        let mut struck_ragdolls = Vec::new();
        for (h, b) in self.w.bodies.iter_mut() {
            if !b.is_dynamic() {
                continue;
            }
            if matches!(Owner::decode(b.user_data), Some(Owner::Ref { form_id, .. }) if spared(form_id)) {
                continue;
            }
            let d = b.position().translation - c;
            let dist = d.length();
            if dist > r {
                continue;
            }
            let dir = if dist > 1e-4 { d / dist } else { Vector::Z };
            let dv = speed * k * (1.0 - dist / r);
            let m = b.mass();
            b.apply_impulse(dir * (dv * m), true);
            // A query before the next step must not lose the wake-up (`update_queries`).
            self.struck.push(h);
            struck_ragdolls.extend(self.ragdoll_of.get(&h).map(|o| o.0));
            n += 1;
        }
        for id in struck_ragdolls {
            self.unsettle_ragdoll(id);
        }
        n
    }

    /// The references with bodies that respond to the wind, and the centre of mass of
    /// their first such body (game units).
    pub fn wind_refs(&self) -> Vec<(u32, [f32; 3])> {
        self.wind.iter().filter_map(|(&id, b)| Some((id, arr3(self.w.bodies.get(*b.first()?)?.center_of_mass() / self.k)))).collect()
    }

    /// Blow reference `form_id`'s bodies that respond to the wind for `dt` seconds: each
    /// awake one's horizontal velocity is drawn towards `wind` (game units a second,
    /// horizontal) by at most `accel` × `dt` (game units a second squared), so a slope it
    /// cannot climb or something it lodges against stops it, and once it has come to rest
    /// and fallen asleep the wind leaves it there [hypothesis: the original's tumbleweeds,
    /// docs/spec-notes/wind.md]. A body asleep is not woken. Returns how many it blew.
    pub fn blow(&mut self, form_id: u32, wind: [f32; 2], accel: f32, dt: f32) -> usize {
        let k = self.k;
        let Some(bodies) = self.wind.get(&form_id) else { return 0 };
        let mut n = 0;
        for &b in bodies {
            let Some(body) = self.w.bodies.get_mut(b).filter(|x| x.is_dynamic() && !x.is_sleeping()) else { continue };
            let v = body.linvel();
            let want = Vector::new(wind[0] * k, wind[1] * k, v.z);
            let dv = (want - v).clamp_length_max((accel * dt).max(0.0) * k);
            body.set_linvel(v + Vector::new(dv.x, dv.y, 0.0), false);
            n += 1;
        }
        n
    }

    /// Strike a reference's dynamic body at `at` with `impulse` (mass × game units per
    /// second), as a shot that hits loose clutter does (combat) [design]: the body nearest
    /// to `at` gains `impulse ÷ mass` of speed, at most `max_dv`, and is turned as a blow
    /// at `at` turns it, by at most [`crate::ragdoll::MAX_PUSH_SPIN`] (as
    /// [`PhysWorld::push_ragdoll`] bounds it: a small body struck off centre would
    /// otherwise spin fast enough to pass through what it rests on). Returns the speed
    /// gained, `None` when the reference has no dynamic body.
    pub fn push_ref(&mut self, form_id: u32, at: [f32; 3], impulse: [f32; 3], max_dv: f32) -> Option<f32> {
        let k = self.k;
        let p = vec3(at) * k;
        let (b, _) = self
            .refs
            .get(&form_id)?
            .iter()
            .filter_map(|&b| {
                let body = self.w.bodies.get(b).filter(|x| x.is_dynamic())?;
                Some((b, (body.center_of_mass() - p).length_squared()))
            })
            .min_by(|x, y| x.1.total_cmp(&y.1))?;
        let body = &mut self.w.bodies[b];
        let m = body.mass();
        if !(m.is_finite() && m > 0.0) {
            return None;
        }
        let dv = (vec3(impulse) * k / m).clamp_length_max(max_dv.max(0.0) * k);
        let r = p - body.center_of_mass();
        let spin = r.cross(dv) / r.length_squared().max((crate::ragdoll::PUSH_ARM * k).powi(2));
        let spin = spin.clamp_length_max(crate::ragdoll::MAX_PUSH_SPIN);
        body.set_linvel(body.linvel() + dv, true);
        body.set_angvel(body.angvel() + spin, true);
        // A query before the next step must not lose the wake-up (`update_queries`).
        self.struck.push(b);
        Some(dv.length() / k)
    }

    /// Set one axis (0 x, 1 y, 2 z) of the velocity of every dynamic body of a reference
    /// to `speed` game units per second, the other axes kept (the console's SetVel);
    /// how many bodies it set, 0 when the reference has no dynamic body.
    pub fn set_ref_velocity(&mut self, form_id: u32, axis: usize, speed: f32) -> usize {
        let k = self.k;
        let Some(bodies) = self.refs.get(&form_id) else { return 0 };
        let mut set = 0;
        for &b in bodies {
            let Some(body) = self.w.bodies.get_mut(b).filter(|x| x.is_dynamic()) else { continue };
            let mut v = body.linvel();
            v[axis.min(2)] = speed * k;
            body.set_linvel(v, true);
            self.struck.push(b);
            set += 1;
        }
        set
    }

    /// The mass of a reference's heaviest dynamic body; `None` without one.
    pub fn ref_mass(&self, form_id: u32) -> Option<f32> {
        self.refs.get(&form_id)?.iter().filter_map(|&b| self.w.bodies.get(b).filter(|x| x.is_dynamic()).map(|x| x.mass())).max_by(f32::total_cmp)
    }

    /// The centre of mass of a reference's first body, game units.
    pub fn ref_center(&self, form_id: u32) -> Option<[f32; 3]> {
        let b = *self.refs.get(&form_id)?.first()?;
        Some(arr3(self.w.bodies.get(b)?.center_of_mass() / self.k))
    }

    /// The middle of a reference's colliders' bounding box, game units: where a shot at
    /// it meets its shape, which can lie well off the reference's own origin (a thin
    /// wall ~97 units off the camera target's origin, Classic Inspiration's photo
    /// targets [observed]). `None` without colliders.
    pub fn ref_collision_middle(&self, form_id: u32) -> Option<[f32; 3]> {
        let k = self.k;
        let colliders = self.refs.get(&form_id)?.iter().filter_map(|&b| self.w.bodies.get(b)).flat_map(|b| b.colliders().iter().copied());
        let (lo, hi) = colliders.filter_map(|c| self.w.colliders.get(c).map(|c| c.compute_aabb())).fold((None::<[f32; 3]>, None::<[f32; 3]>), |(lo, hi), a| {
            let (m, x) = ([a.mins.x, a.mins.y, a.mins.z], [a.maxs.x, a.maxs.y, a.maxs.z]);
            (Some(lo.map_or(m, |l| [l[0].min(m[0]), l[1].min(m[1]), l[2].min(m[2])])), Some(hi.map_or(x, |h| [h[0].max(x[0]), h[1].max(x[1]), h[2].max(x[2])])))
        });
        let (lo, hi) = (lo?, hi?);
        Some([(lo[0] + hi[0]) / 2.0 / k, (lo[1] + hi[1]) / 2.0 / k, (lo[2] + hi[2]) / 2.0 / k])
    }

    /// The Havok material code of a reference's first collider that has one (a convex
    /// collider's, or a triangle mesh's first triangle's); `None` without one.
    pub fn ref_material(&self, form_id: u32) -> Option<u32> {
        let bodies = self.refs.get(&form_id)?;
        bodies.iter().filter_map(|&b| self.w.bodies.get(b)).flat_map(|b| b.colliders().iter()).find_map(|c| self.materials.get(c)?.first().copied())
    }

    /// The lowest point of a reference's colliders with their contact shells, game units:
    /// how low it reaches against what it rests on (its shape's lowest point less its own
    /// shell). `None` without colliders.
    pub fn ref_lowest_z(&self, form_id: u32) -> Option<f32> {
        let k = self.k;
        let colliders = self.refs.get(&form_id)?.iter().filter_map(|&b| self.w.bodies.get(b)).flat_map(|b| b.colliders().iter().copied());
        colliders.filter_map(|c| Some(self.w.colliders.get(c)?.compute_aabb().mins.z / k - self.shell_of(c))).reduce(f32::min)
    }

    /// Lift every body of a reference by `dz` game units, as a placement (a teleport: each
    /// body wakes, and the next query sees it there). How many bodies moved.
    pub fn lift_ref(&mut self, form_id: u32, dz: f32) -> usize {
        let Some(bodies) = self.refs.get(&form_id).cloned() else { return 0 };
        let by = Vector::Z * dz * self.k;
        let mut moved = 0;
        for b in bodies {
            let Some(body) = self.w.bodies.get_mut(b) else { continue };
            let mut moved_to = *body.position();
            moved_to.translation += by;
            body.set_position(moved_to, true);
            for &c in body.colliders() {
                if let Some(co) = self.w.colliders.get_mut(c) {
                    let mut cp = *co.position();
                    cp.translation += by;
                    co.set_position(cp);
                }
            }
            self.body_placed(b);
            moved += 1;
        }
        moved
    }

    /// Current state of each body of a reference, in object order.
    pub fn ref_bodies(&self, form_id: u32) -> Vec<BodyState> {
        self.refs.get(&form_id).into_iter().flatten().map(|&b| body_state(&self.w.bodies[b], self.k)).collect()
    }

    /// Angular velocity (radians a second) of each body of a reference, in object order;
    /// `None` for a body that is not dynamic (diagnostics).
    pub fn ref_spins(&self, form_id: u32) -> Vec<Option<[f32; 3]>> {
        let spin = |b: &RigidBody| b.is_dynamic().then(|| arr3(b.angvel()));
        self.refs.get(&form_id).into_iter().flatten().map(|&b| spin(&self.w.bodies[b])).collect()
    }

    /// The deepest contact of each pair the reference's colliders are in, as of the last
    /// narrow-phase update: the index of the reference's body, what the other collider
    /// belongs to and the contact's distance in game units (negative: overlapping by that
    /// much). Diagnostics: an overlap at placement shows what pushes a body apart.
    pub fn ref_contacts(&self, form_id: u32) -> Vec<(usize, Option<Owner>, f32)> {
        let mut out = Vec::new();
        for (i, &b) in self.refs.get(&form_id).into_iter().flatten().enumerate() {
            let Some(body) = self.w.bodies.get(b) else { continue };
            for &c in body.colliders() {
                for pair in self.w.contact_pairs_with(c) {
                    let other = if pair.collider1 == c { pair.collider2 } else { pair.collider1 };
                    let Some((_, contact)) = pair.find_deepest_contact() else { continue };
                    let owner = self.w.colliders.get(other).and_then(|o| Owner::decode(o.user_data));
                    out.push((i, owner, contact.dist / self.k));
                }
            }
        }
        out
    }

    pub(crate) fn insert_body(&mut self, b: impl Into<RigidBody>) -> RigidBodyHandle {
        let h = self.w.insert_body(b);
        self.new_bodies.push(h);
        h
    }

    /// A body was moved by setting its position (a teleport), which wakes it: like a
    /// body struck since the last step, the next query must not lose the wake-up for
    /// the step ([`PhysWorld::update_queries`]).
    pub(crate) fn body_placed(&mut self, h: RigidBodyHandle) {
        self.struck.push(h);
        // Drawn where it was put, not on the way there (`crate::drawn`).
        self.before.forget(h);
        self.mark_dirty(DirtyBy::Transform);
    }

    /// Advance one fixed step.
    pub fn step(&mut self) {
        // Standalone hit volumes written since the last refresh, placed as rapier's step
        // would place attached ones (`crate::actors`).
        self.flush_hit_volumes();
        if !self.pending_sleep.is_empty() {
            // Find the contacts of bodies added asleep first (`update_queries`).
            self.mark_dirty(DirtyBy::Other);
            self.update_queries_for(QueryConsumer::Step);
        }
        // Spans for profiling (the engine's Tracy build shows them as zones) [design].
        {
            let _span = tracing::info_span!("phys::grip_buoy").entered();
            self.drive_grip();
            self.buoy();
        }
        // The poses the coming step starts from, for drawing between steps (`crate::drawn`):
        // read only by the drawing.
        self.keep_poses_before();
        {
            let _span = tracing::info_span!("phys::rapier_step").entered();
            self.step_world();
            let c = &self.w.physics_pipeline.counters;
            for (a, t) in self.solver_ms.iter_mut().zip([c.solver.velocity_assembly_time, c.solver.velocity_resolution_time, c.stages.collision_detection_time, c.stages.island_construction_time, c.stages.ccd_time, c.step_time]) {
                *a += t.time_ms();
            }
        }
        // Diagnostics: marks the step's own detection took, with no refresh between.
        if self.dirty {
            self.refresh_stats.step_absorbed += 1;
        }
        self.dirty_by = 0;
        self.dirty = false;
        self.new_bodies.clear();
        self.struck.clear();
        // The positive step consumed the old changes. Keep any marks/new bodies
        // produced by quarantine remedies for the next query registration.
        if !self.w.quarantine().is_empty() {
            self.handle_quarantine();
        }
        self.steps += 1;
        self.stepped_secs += f64::from(self.params.dt);
    }

    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Game seconds the world has stepped: every step's `dt` summed, so a step made short
    /// by a slowed clock counts for what it was. The time base for anything that judges
    /// how long physics has had (a body settling, a corpse calming): not the frames' time,
    /// which also runs while no step is taken, nor the steps times the step now.
    pub fn stepped_secs(&self) -> f64 {
        self.stepped_secs
    }

    /// Change the fixed step, seconds (`PhysParams::dt`): the next [`PhysWorld::step`] and
    /// everything that reads the step (characters, buoyancy, the grab) go by it. For a
    /// clock that runs slow: a step of the same share of the usual one keeps the steps as
    /// many a real second as at full speed. Not positive: left as it is.
    pub fn set_step(&mut self, dt: f32) {
        if dt > 0.0 && dt.is_finite() {
            self.params.dt = dt;
            self.w.integration_parameters.dt = dt;
        }
    }

    /// Bring the broad phase up to date after adding or removing bodies, so that queries
    /// see them, without advancing time.
    pub fn update_queries(&mut self) {
        self.update_queries_for(QueryConsumer::Cast);
    }

    /// [`PhysWorld::update_queries`], counted for `by` in [`RefreshStats`] (the same work
    /// whoever asks).
    pub fn update_queries_for(&mut self, by: QueryConsumer) {
        if self.dirty {
            // Only when there is work: this is called before every query.
            let _span = tracing::info_span!("phys::query_refresh").entered();
            self.count_refresh(by);
            // Standalone hit volumes written since the last refresh (`crate::actors`).
            self.flush_hit_volumes();
            let pending_sleep = !self.pending_sleep.is_empty();
            let awake_before: HashSet<RigidBodyHandle> = if pending_sleep {
                self.w
                    .bodies
                    .iter()
                    .filter(|(_, b)| b.is_enabled() && !b.is_fixed() && !b.is_sleeping())
                    .map(|(h, _)| h)
                    .collect()
            } else {
                HashSet::new()
            };
            {
                // Zero-dt physics step: registers bodies/changes with islands and updates
                // broad- and narrow-phase contacts without advancing time or forces.
                let _span = tracing::info_span!("phys::refresh_detect").entered();
                self.register_query_changes();
            }
            let _repair = tracing::info_span!("phys::refresh_repair").entered();
            // A body added asleep wakes when its first contacts are found: rapier wakes
            // both sides of every contact that starts [rapier 0.36.1, `NarrowPhase`'s
            // pair transitions], so clutter placed on a shelf, touching it or its
            // neighbours, woke at once and was pushed about. Its contacts have been found
            // and linked into the persistent islands now; put it (and any previously
            // sleeping partner woken only by it) back to sleep so the next step's
            // `update_islands` moves the island to a sleeping chunk before the solver runs
            // [observed: test `clutter_added_asleep_on_a_shelf_it_overlaps_stays_asleep`].
            // If its island also holds any body that was awake before `detect()`, leave
            // the whole island awake so struck/woken bodies keep their impulse.
            if pending_sleep {
                let pending_set: HashSet<RigidBodyHandle> = self.pending_sleep.drain(..).collect();
                let mut awake_islands: HashSet<u32> = HashSet::new();
                for h in self.w.islands.active_bodies() {
                    if awake_before.contains(&h) {
                        if let Some(id) = self.w.islands.persistent_island_of(&self.w.bodies, h) {
                            awake_islands.insert(id);
                        }
                    }
                }
                let mut sleep_islands: HashSet<u32> = HashSet::new();
                for &h in &pending_set {
                    if let Some(id) = self.w.islands.persistent_island_of(&self.w.bodies, h) {
                        if !awake_islands.contains(&id) {
                            sleep_islands.insert(id);
                        }
                    }
                }
                if !sleep_islands.is_empty() {
                    let to_sleep: Vec<RigidBodyHandle> = self
                        .w
                        .islands
                        .active_bodies()
                        .filter(|&h| {
                            self.w
                                .islands
                                .persistent_island_of(&self.w.bodies, h)
                                .is_some_and(|id| sleep_islands.contains(&id))
                        })
                        .collect();
                    for h in to_sleep {
                        if let Some(b) = self.w.bodies.get_mut(h) {
                            b.sleep();
                        }
                    }
                }
            }
            // Any remaining island inconsistency (`crate::islands`).
            let repaired = self.repair_islands();
            self.refresh_stats.repair_islands += repaired as u64;
            if !self.w.quarantine().is_empty() {
                self.handle_quarantine();
            }
        }
    }

    /// Register current changes and immediately retain each pipeline's quarantine
    /// report. The selected backend's remedies need at most one republish: they
    /// restore sanitized finite bodies or rebuild an at-rest ragdoll. Any malformed
    /// rebuilt geometry stays disabled on the second pass (see the topic note).
    fn register_query_changes(&mut self) {
        for _ in 0..2 {
            self.detect();
            // Only these changes were consumed by this pass. A remedy below may
            // add bodies/marks; do not clear those before their own registration.
            self.new_bodies.clear();
            self.dirty = false;
            self.dirty_by = 0;
            if !self.w.quarantine().is_empty() {
                self.handle_quarantine();
            }
            if !self.dirty { return; }
        }
        // No recursive or unbounded registration. This would violate the closed
        // remedy invariant above: retain the pending marks for the next query,
        // and do not advertise its partially published state as clean.
        tracing::warn!("physics: quarantine remedies still dirty after two query registration passes; changes remain pending");
    }

    /// Scene queries on the current state (updates the broad phase first if needed).
    pub fn queries(&mut self) -> Queries<'_> {
        self.update_queries();
        Queries { world: self }
    }

    pub(crate) fn mark_dirty(&mut self, by: DirtyBy) {
        self.dirty = true;
        self.dirty_by |= 1 << by as u8;
        self.refresh_stats.marks[by as usize] += 1;
    }

    /// Diagnostics: a refresh asked for by `by` (before its work; reads only sizes).
    fn count_refresh(&mut self, by: QueryConsumer) {
        let s = &mut self.refresh_stats;
        s.refreshes += 1;
        s.by_consumer[by as usize] += 1;
        for (i, n) in s.refreshes_with.iter_mut().enumerate() {
            if self.dirty_by & (1 << i) != 0 {
                *n += 1;
            }
        }
        if self.dirty_by == 1 << DirtyBy::HitVolume as u8 {
            s.hitvol_only[by as usize] += 1;
        }
        s.colliders += self.w.colliders.len() as u64;
        s.repair_new += self.new_bodies.len() as u64;
        s.repair_sleep += self.pending_sleep.len() as u64;
        s.repair_struck += self.struck.len() as u64;
        self.dirty_by = 0;
    }

    /// Counts of the broad-phase refreshes since the world was made (diagnostics).
    pub fn refresh_stats(&self) -> RefreshStats {
        self.refresh_stats
    }

    /// Diagnostics: `n` hit-volume capsules written ([`RefreshStats::hitvol_colliders`]).
    pub(crate) fn count_hitvol_colliders(&mut self, n: usize) {
        self.refresh_stats.hitvol_colliders += n as u64;
    }

    /// Diagnostics: hit-volume capsules written to rapier and left alone as bit-identical
    /// ([`RefreshStats::hitvol_written`], [`RefreshStats::hitvol_skipped`]).
    pub(crate) fn count_hitvol_writes(&mut self, written: u64, skipped: u64) {
        self.refresh_stats.hitvol_written += written;
        self.refresh_stats.hitvol_skipped += skipped;
    }

    /// Bounding box (game units: min and max corners) of every collider on a fixed body —
    /// terrain and references' static and keyframed collision, not characters or dynamic
    /// bodies; `None` when there are none.
    pub fn fixed_bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut out: Option<([f32; 3], [f32; 3])> = None;
        for (_, c) in self.w.colliders.iter() {
            if !c.parent().is_some_and(|b| self.w.bodies[b].is_fixed()) {
                continue;
            }
            let a = c.compute_aabb();
            let (lo, hi) = (arr3(a.mins / self.k), arr3(a.maxs / self.k));
            let b = out.get_or_insert((lo, hi));
            for i in 0..3 {
                b.0[i] = b.0[i].min(lo[i]);
                b.1[i] = b.1[i].max(hi[i]);
            }
        }
        out
    }

    pub fn stats(&self) -> Stats {
        let mut s = Stats { terrain_cells: self.terrain.len(), refs: self.refs.len(), steps: self.steps, ..Default::default() };
        for (_, b) in self.w.bodies.iter() {
            match b.body_type() {
                RigidBodyType::Fixed => s.bodies_fixed += 1,
                RigidBodyType::Dynamic => s.bodies_dynamic += 1,
                _ => s.bodies_kinematic += 1,
            }
        }
        for (_, c) in self.w.colliders.iter() {
            // A hull with large faces (`crate::hull`) counts as the hull it is.
            let t = if c.shape().as_shape::<crate::hull::Hull>().is_some() { ShapeType::ConvexPolyhedron } else { c.shape().shape_type() };
            *s.colliders.entry(shape_name(t)).or_default() += 1;
        }
        s.bodies_awake = self.w.islands.num_active_bodies();
        for pair in self.w.narrow_phase.contact_pairs().filter(|p| p.has_any_active_contact()) {
            s.contact_pairs += 1;
            for m in pair.solver_manifolds().iter().filter(|m| !m.data.solver_contacts.is_empty()) {
                s.manifolds += 1;
                s.solver_contacts += m.data.solver_contacts.len();
            }
        }
        s.joints = self.w.impulse_joints.len();
        (s.quarantined_bodies, s.quarantined_colliders) = self.quarantined.counts();
        s.solver_us = self.solver_ms.map(|v| (v * 1000.0) as u64);
        s
    }

    /// A full copy of the world (bodies, colliders, contacts, islands, broad phase and the
    /// tables here). Stepping the copy gives bit-identical results to stepping the
    /// original [observed: test `snapshot_continues_identically`]; characters keep their
    /// handles, so a cloned [`crate::Character`] drives the copy.
    pub fn snapshot(&self) -> PhysWorld {
        let w = &self.w;
        PhysWorld {
            params: self.params,
            w: PhysicsWorld {
                gravity: w.gravity,
                integration_parameters: w.integration_parameters,
                physics_pipeline: PhysicsPipeline::new(),
                collision_pipeline: CollisionPipeline::new(),
                islands: w.islands.clone(),
                broad_phase: w.broad_phase.clone(),
                narrow_phase: w.narrow_phase.clone(),
                bodies: w.bodies.clone(),
                colliders: w.colliders.clone(),
                impulse_joints: w.impulse_joints.clone(),
                multibody_joints: w.multibody_joints.clone(),
                soft_bodies: w.soft_bodies.clone(),
                ccd_solver: CCDSolver::new(),
            },
            k: self.k,
            terrain: self.terrain.clone(),
            refs: self.refs.clone(),
            ref_nodes: self.ref_nodes.clone(),
            keyframed: self.keyframed.clone(),
            wind: self.wind.clone(),
            actors: self.actors.clone(),
            ragdolls: self.ragdolls.clone(),
            ragdoll_of: self.ragdoll_of.clone(),
            passes_ragdoll: self.passes_ragdoll.clone(),
            gore: self.gore.clone(),
            materials: self.materials.clone(),
            shells: self.shells.clone(),
            dirty: self.dirty,
            dirty_by: self.dirty_by,
            refresh_stats: self.refresh_stats,
            new_bodies: self.new_bodies.clone(),
            struck: self.struck.clone(),
            pending_sleep: self.pending_sleep.clone(),
            steps: self.steps,
            stepped_secs: self.stepped_secs,
            water: self.water.clone(),
            grip: self.grip.clone(),
            standalone_hit_volumes: self.standalone_hit_volumes,
            hitvol_pending: self.hitvol_pending.clone(),
            solver_ms: self.solver_ms,
            before: self.before.clone(),
            quarantined: self.quarantined.clone(),
        }
    }

    /// A digest of every body's position and velocity bits, in handle order: equal
    /// digests mean bit-identical states (determinism checks).
    pub fn state_digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |v: f32| {
            for byte in v.to_bits().to_le_bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        };
        for (_, b) in self.w.bodies.iter() {
            let p = b.position();
            for v in [p.translation.x, p.translation.y, p.translation.z, p.rotation.x, p.rotation.y, p.rotation.z, p.rotation.w] {
                eat(v);
            }
            let l = b.linvel();
            for v in [l.x, l.y, l.z] {
                eat(v);
            }
        }
        h
    }

    /// Contact shell of a collider, game units: [`TERRAIN_SHELL`] for terrain, the
    /// collision object's shell for a reference's collider, 0 otherwise.
    pub(crate) fn shell_of(&self, c: ColliderHandle) -> f32 {
        match self.w.colliders.get(c).and_then(|co| Owner::decode(co.user_data)) {
            Some(Owner::Terrain { .. }) => TERRAIN_SHELL,
            _ => self.shells.get(&c).copied().unwrap_or(0.0),
        }
    }

    /// Material code of triangle `triangle` of a triangle-mesh collider, or of a convex
    /// collider (which has one).
    pub(crate) fn material_of(&self, c: ColliderHandle, triangle: u32) -> Option<u32> {
        let m = self.materials.get(&c)?;
        m.get(triangle as usize).or(if m.len() == 1 { m.first() } else { None }).copied()
    }
}

fn shape_name(t: ShapeType) -> String {
    format!("{t:?}").to_ascii_lowercase()
}

pub(crate) fn body_state(b: &RigidBody, k: f32) -> BodyState {
    body_state_at(b, b.position(), k)
}

/// [`body_state`] with the body at `p` (`crate::drawn`).
pub(crate) fn body_state_at(b: &RigidBody, p: &Pose, k: f32) -> BodyState {
    let m = rapier3d::math::Matrix::from_quat(p.rotation).transpose().to_cols_array();
    BodyState { translation: arr3(p.translation / k), rotation: m, linvel: arr3(b.linvel() / k), sleeping: b.is_sleeping() }
}

/// Runs a variable frame time as whole fixed steps; the remainder carries over.
///
/// The remainder is kept in `f64`: each frame adds its time and each step takes `dt` off,
/// and in `f32` the sum's rounding (about 2e-9 s at a step's size) would build up over
/// hours of frames [design]. [`Stepper::alpha`] is the one place the share of a step
/// that the remainder stands for is worked out: everything drawn between the last two
/// steps blends by it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stepper {
    /// Game time towards the next step, seconds.
    pub accumulated: f64,
}

impl Stepper {
    /// How many fixed steps `elapsed` seconds call for (at most `params.max_substeps`;
    /// the time beyond is dropped, leaving at most one step's).
    pub fn advance(&mut self, params: &PhysParams, elapsed: f32) -> u32 {
        let dt = f64::from(params.dt);
        self.accumulated += f64::from(elapsed.max(0.0));
        let mut n = 0;
        while self.accumulated >= dt && n < params.max_substeps {
            self.accumulated -= dt;
            n += 1;
        }
        if n == params.max_substeps {
            self.accumulated = self.accumulated.min(dt);
        }
        n
    }

    /// How far the remainder is into the next step of `dt` seconds, 0 to 1: what is
    /// drawn between the last two steps sits this share of the way to the last one.
    pub fn alpha(&self, dt: f32) -> f32 {
        if dt > 0.0 {
            (self.accumulated / f64::from(dt)).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    }

    /// The step changes from `from` to `to` seconds (another rate, or a clock that runs
    /// slow): the remainder keeps its share of a step, so [`Stepper::alpha`], and with it
    /// everything drawn between steps, does not jump. The game time it stands for changes
    /// by less than one step's [design: a jump on screen would show; a fraction of a step
    /// of game time, once, does not].
    pub fn rescale(&mut self, from: f32, to: f32) {
        if from > 0.0 && to > 0.0 && from.is_finite() && to.is_finite() {
            self.accumulated = (self.accumulated * f64::from(to) / f64::from(from)).min(f64::from(to));
        }
    }
}

/// Which colliders a query considers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    /// Entities to ignore (e.g. the character casting the ray).
    pub exclude: Vec<Key>,
    /// Ignore terrain.
    pub no_terrain: bool,
    /// Ignore dynamic bodies.
    pub no_dynamic: bool,
    /// Ignore every actor: characters' capsules, hit volumes and body parts, ragdolls.
    pub no_actors: bool,
    /// Ignore reference colliders on these Havok layers (`crate::layers`).
    pub exclude_layers: crate::layers::LayerSet,
}

impl Filter {
    pub fn excluding(key: Key) -> Filter {
        Filter { exclude: vec![key], ..Default::default() }
    }

    fn accepts(&self, c: &Collider, bodies: &RigidBodySet) -> bool {
        let Some(owner) = Owner::decode(c.user_data) else { return false };
        if self.no_terrain && matches!(owner, Owner::Terrain { .. }) {
            return false;
        }
        if self.no_dynamic && c.parent().is_some_and(|b| bodies[b].is_dynamic()) {
            return false;
        }
        if self.no_actors && matches!(owner, Owner::Character { .. } | Owner::Actor { .. }) {
            return false;
        }
        if let Owner::Ref { layer, .. } | Owner::Actor { layer, .. } = owner {
            if self.exclude_layers.contains(layer) {
                return false;
            }
        }
        !self.exclude.contains(&owner.key())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct RayHit {
    pub owner: Owner,
    pub distance: f32,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    /// Havok material code of the triangle hit, for triangle meshes.
    pub material: Option<u32>,
    /// Contact shell of the collider hit, game units ([`TERRAIN_SHELL`],
    /// `CollisionObject::shell`); the hit point itself is on the bare surface.
    pub shell: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct SweepHit {
    pub owner: Owner,
    /// Fraction of the motion travelled before contact (0..=1).
    pub fraction: f32,
    /// Contact point on the hit collider, world space.
    pub point: [f32; 3],
    /// Contact normal pointing out of the hit collider.
    pub normal: [f32; 3],
}

/// Read-only scene queries on an up-to-date world (from [`PhysWorld::queries`]).
pub struct Queries<'a> {
    world: &'a PhysWorld,
}

impl Queries<'_> {
    /// The surface of the water a point is in or over ([`PhysWorld::water_surface_for`]).
    pub fn water_surface_for(&self, p: [f32; 3]) -> Option<f32> {
        self.world.water_surface_for(p)
    }

    fn with_filter<R>(&self, filter: &Filter, f: impl FnOnce(QueryFilter) -> R) -> R {
        let w = &self.world.w;
        let pred = |_: ColliderHandle, c: &Collider| filter.accepts(c, &w.bodies);
        f(QueryFilter::default().exclude_sensors().predicate(&pred))
    }

    /// First hit along the ray from `origin` in `direction` (normalised here) within
    /// `max_distance`. Activation targeting casts this from the eye along the view.
    pub fn ray(&self, origin: [f32; 3], direction: [f32; 3], max_distance: f32, filter: &Filter) -> Option<RayHit> {
        let dir = vec3(direction).try_normalize()?;
        let ray = Ray::new(vec3(origin) * self.world.k, dir);
        let w = &self.world.w;
        let (c, hit) = self.with_filter(filter, |f| {
            let qp = w.query_pipeline_with_filter(f);
            let found = first_hit(
                qp.bvh,
                max_distance * self.world.k,
                |node, bound| node.cast_ray(&ray, bound),
                // `bound` lets a candidate tied with the best so far through (`first_hit`).
                |id, bound| qp.map_typed_part_at(id, |pose, part, _| part.cast_ray_and_get_normal(pose.expect("a collider has a pose"), &ray, bound, true).map(|h| (h.time_of_impact, h)))?,
                |a, ha, b, hb| tie_key(qp.colliders, a).cmp(&tie_key(qp.colliders, b)).then_with(|| ha.normal.to_array().map(f32::to_bits).cmp(&hb.normal.to_array().map(f32::to_bits))).then(ha.subshape.cmp(&hb.subshape)).is_lt(),
            )?;
            Some((qp.colliders.get_unknown_gen(found.0)?.1, found.1))
        })?;
        Some(self.ray_hit(c, &ray, hit))
    }

    fn ray_hit(&self, c: ColliderHandle, ray: &Ray, hit: RayIntersection) -> RayHit {
        let owner = Owner::decode(self.world.w.colliders[c].user_data).expect("every collider has an owner");
        let k = self.world.k;
        RayHit {
            owner,
            distance: hit.time_of_impact / k,
            point: arr3(ray.point_at(hit.time_of_impact) / k),
            normal: arr3(hit.normal),
            material: self.world.material_of(c, hit.subshape),
            shell: self.world.shell_of(c),
        }
    }

    /// Every hit along the ray, nearest first (ties by owner).
    pub fn ray_all(&self, origin: [f32; 3], direction: [f32; 3], max_distance: f32, filter: &Filter) -> Vec<RayHit> {
        let Some(dir) = vec3(direction).try_normalize() else { return Vec::new() };
        let ray = Ray::new(vec3(origin) * self.world.k, dir);
        let w = &self.world.w;
        // Ties as in `ray` (`first_hit`): owner, segment, then the hit's normal and subshape.
        let mut hits: Vec<(RayHit, u16, u32)> = self.with_filter(filter, |f| {
            w.intersect_ray(ray, max_distance * self.world.k, true, f).map(|(c, co, hit)| (self.ray_hit(c, &ray, hit), Owner::segment(co.user_data), hit.subshape)).collect()
        });
        hits.sort_by(|(a, sa, ta), (b, sb, tb)| {
            a.distance.total_cmp(&b.distance).then(a.owner.cmp(&b.owner)).then(sa.cmp(sb)).then_with(|| a.normal.map(f32::to_bits).cmp(&b.normal.map(f32::to_bits))).then(ta.cmp(tb))
        });
        hits.into_iter().map(|(h, ..)| h).collect()
    }

    /// Sweep a convex `shape` placed at `from` along `motion`; the first contact.
    pub fn sweep(&self, shape: &Shape, from: &Iso, motion: [f32; 3], filter: &Filter) -> Result<Option<SweepHit>, ConvertError> {
        let k = self.world.k;
        let (pose, shape) = probe(shape, from, k)?;
        let w = &self.world.w;
        let opts = ShapeCastOptions { max_time_of_impact: 1.0, stop_at_penetration: true, ..Default::default() };
        let vel = vec3(motion) * k;
        let hit = self.with_filter(filter, |f| {
            let qp = w.query_pipeline_with_filter(f);
            // As parry's composite shape cast does it [design: parry3d 0.31.1
            // `query/shape_cast/shape_cast_composite_shape_shape.rs:15-60`]: a node's bound is
            // where the motion meets the node's box grown by the shape's box; each collider
            // is cast with the full options (every leaf answers as it did before #1459).
            let ls_aabb = shape.compute_aabb(&pose);
            let ray = Ray::new(Vector::ZERO, vel);
            let (shift, margin) = (-ls_aabb.center(), ls_aabb.half_extents() + Vector::splat(opts.target_distance));
            let found = first_hit(
                qp.bvh,
                opts.max_time_of_impact,
                |node, bound| Aabb { mins: node.mins() + shift - margin, maxs: node.maxs() + shift + margin }.cast_local_ray(&ray, bound, true).unwrap_or(f32::MAX),
                |id, _| {
                    qp.map_untyped_part_at(id, |part_pose, part, _| {
                        let part_pose = part_pose.expect("a collider has a pose");
                        let hit = qp.dispatcher.cast_shapes(&part_pose.inv_mul(&pose), part_pose.rotation.inverse() * vel, part, shape.as_ref(), opts).ok()??.transform1_by(part_pose);
                        Some((hit.time_of_impact, hit))
                    })?
                },
                |a, ha, b, hb| {
                    let geom = |h: &ShapeCastHit| (h.witness1.to_array().map(f32::to_bits), h.normal1.to_array().map(f32::to_bits));
                    tie_key(qp.colliders, a).cmp(&tie_key(qp.colliders, b)).then_with(|| geom(ha).cmp(&geom(hb))).is_lt()
                },
            )?;
            Some((qp.colliders.get_unknown_gen(found.0)?.1, found.1))
        });
        Ok(hit.map(|(c, hit)| {
            let co = &self.world.w.colliders[c];
            SweepHit {
                owner: Owner::decode(co.user_data).expect("every collider has an owner"),
                fraction: hit.time_of_impact,
                // The hit collider's side of the hit is `witness1`/`normal1`, in world space.
                point: arr3(hit.witness1 / k),
                normal: arr3(hit.normal1),
            }
        }))
    }

    /// What would keep `c` from standing with its feet at `feet`, sorted: whatever its
    /// shape (the controller's own, not a capsule's stand-in) overlaps there, but for what
    /// the controller passes through or pushes aside (its own body, its ignored layers,
    /// dynamic bodies up to [`crate::character::PUSH_MAX_MASS`]). Other characters count.
    pub fn character_overlap(&self, c: &crate::character::Character, feet: [f32; 3]) -> Vec<Owner> {
        let k = self.world.k;
        let pose = Pose::from_translation((vec3(feet) + Vector::Z * (c.params.height / 2.0)) * k);
        let filter = Filter { exclude: vec![c.key()], exclude_layers: c.ignore_layers, ..Default::default() };
        let w = &self.world.w;
        let blocks = |co: &Collider| co.parent().and_then(|b| w.bodies.get(b)).is_none_or(|b| !b.is_dynamic() || b.mass() > crate::character::PUSH_MAX_MASS);
        let mut out: Vec<Owner> = self.with_filter(&filter, |f| {
            w.intersect_shape(pose, c.shape.as_ref(), f).filter(|(_, co)| blocks(co)).filter_map(|(_, co)| Owner::decode(co.user_data)).collect()
        });
        out.sort();
        out.dedup();
        out
    }

    /// Every entity whose collision overlaps a convex `shape` placed at `at`, sorted.
    pub fn overlap(&self, shape: &Shape, at: &Iso, filter: &Filter) -> Result<Vec<Owner>, ConvertError> {
        let (pose, shape) = probe(shape, at, self.world.k)?;
        let w = &self.world.w;
        let mut out: Vec<Owner> = self.with_filter(filter, |f| {
            w.intersect_shape(pose, shape.as_ref(), f).filter_map(|(_, c)| Owner::decode(c.user_data)).collect()
        });
        out.sort();
        out.dedup();
        Ok(out)
    }
}

/// The first hit of a query over the broad phase's tree, the same whatever the tree's
/// shape (physics.md §5.1, [design, #1459]): the candidate with the smallest time of impact
/// (below `max_cost`) and, among exactly tied ones, the one `before` puts first (owner,
/// segment, then the hit's own geometry; the caller's order must be total).
///
/// parry's `Bvh::find_best` keeps a result only on a strictly smaller cost and visits a
/// node only when its bound is strictly below the best cost [design: parry3d 0.31.1
/// `partitioning/bvh/bvh_traverse.rs:402-422`], so a tied candidate would be dropped or
/// never reached, and which one stays depends on where the tree put it. Here the best is
/// kept beside the traversal: a leaf the rule takes reports the next float above its time
/// of impact to `find_best`, so `find_best`'s best cost is always just above the best time
/// so far. Nodes whose bound equals it are still visited and `leaf` is asked with that
/// bound, so it can return a tie; nothing is collected or sorted, and `before` runs only on
/// exact ties. A leaf `find_best` would have kept before is kept now, so answers without a
/// tie are the same as parry's.
fn first_hit<T>(
    bvh: &rapier3d::parry::partitioning::Bvh,
    max_cost: f32,
    node_cost: impl Fn(&rapier3d::parry::partitioning::BvhNode, f32) -> f32,
    leaf: impl Fn(u32, f32) -> Option<(f32, T)>,
    before: impl Fn(u32, &T, u32, &T) -> bool,
) -> Option<(u32, T)> {
    let best: std::cell::RefCell<Option<(u32, f32, T)>> = std::cell::RefCell::new(None);
    bvh.find_best(max_cost, node_cost, |id, bound| {
        let (toi, hit) = leaf(id, bound)?;
        let mut best = best.borrow_mut();
        let take = match &*best {
            None => toi < max_cost,
            Some((b, t, h)) => toi < *t || (toi == *t && before(id, &hit, *b, h)),
        };
        if !take {
            return None;
        }
        *best = Some((id, toi, hit));
        Some(toi.next_up())
    });
    best.into_inner().map(|(id, _, hit)| (id, hit))
}

/// What breaks an exact tie first, after the time of impact: the collider's owner (form id,
/// layer, part for an actor's capsule), then its segment ([`Owner::encode_segment`]).
fn tie_key(colliders: &ColliderSet, id: u32) -> (Option<Owner>, u16) {
    colliders.get_unknown_gen(id).map_or((None, 0), |(c, _)| (Owner::decode(c.user_data), Owner::segment(c.user_data)))
}

/// A single query shape (compounds are not supported as probes), scaled by `k`.
fn probe(shape: &Shape, at: &Iso, k: f32) -> Result<(Pose, SharedShape), ConvertError> {
    let mut leaves = Vec::new();
    let mut errors = Vec::new();
    convert::leaves(shape, at, k, &mut leaves, &mut errors);
    if let Some(e) = errors.into_iter().next() {
        return Err(e);
    }
    match leaves.len() {
        1 => {
            let l = leaves.pop().expect("one leaf");
            Ok((l.pose, l.shape))
        }
        _ => Err(ConvertError::NotSingleShape),
    }
}

#[cfg(test)]
#[path = "world_query_refresh_tests.rs"]
mod query_refresh_tests;

#[cfg(test)]
#[path = "world_query_motion_tests.rs"]
mod query_motion_tests;
