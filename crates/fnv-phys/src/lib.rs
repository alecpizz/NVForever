//! Physics for Layer 4: a rapier3d world built from terrain (LAND heights, via
//! `fnv-world`) and placed references' collision objects (`fnv_collide` shapes), with
//! ray and shape queries, a kinematic capsule character controller, dynamic bodies for
//! clutter and deterministic fixed-step stepping. Headless; no Bevy.
//!
//! - [`convert`]: `fnv_collide::Shape` trees to rapier shapes
//! - [`terrain`]: one heightfield per exterior cell
//! - [`world`]: the world, bodies, stepping, queries
//! - [`character`]: the character controller
//! - [`air`]: horizontal motion in the air and after landings
//! - [`arrival`]: where an actor comes to rest after a teleport (shared by the tools and
//!   the game)
//! - [`layers`]: Havok collision layer numbers and names
//! - [`primitive`]: collision primitives placed on references (CollisionMarker boxes)
//! - [`hull`]: convex hulls whose large faces stay whole in contacts
//! - [`placed`]: the collision objects of a placed reference (shared by the tools and the
//!   game)
//! - [`ragdoll`]: ragdolls, the jointed bodies of dead actors
//! - [`grab`]: a body held by the player's grab
//! - [`drawn`]: the moving bodies' poses before the last step, for drawing between steps
//! - [`quarantine`]: bodies and colliders whose state went NaN or infinite
//! - [`water`]: water areas (cell water surfaces) the character swims in
//! - [`buoyancy`]: ragdolls and gore pieces float in water
//! - [`swim`]: swimming speeds (the stroke cycle)
//! - `debug_lines` (feature `debug-lines`, on by default): the collision shapes as line
//!   segments, for the engine's collision view
//!
//! Units and axes are the game's: lengths in game units, Z up, rotations row-major 3×3
//! rotating column vectors (as in `fnv_collide::shape` and `fnv_world::transform`).
//! Design: docs/design/layer4.md §2.8 and §7 (workstream 2); facts, hypotheses and the
//! CLI contract: docs/spec-notes/physics.md.

pub mod actors;
pub mod air;
pub mod arrival;
pub mod buoyancy;
pub mod casings;
pub mod character;
pub mod convert;
#[cfg(feature = "debug-lines")]
pub mod debug_lines;
pub mod drawn;
pub mod gore;
pub mod grab;
pub mod hull;
mod islands;
pub mod layers;
pub mod placed;
pub mod primitive;
pub mod quarantine;
pub mod ragdoll;
pub mod script;
pub mod swim;
pub mod terrain;
pub mod water;
pub mod world;

pub use actors::{HitVolume, PartCapsule};
pub use arrival::Arrival;
pub use character::{Character, CharacterInput, CharacterParams, CharacterStep, Push, SpotRefused, secs_reached, STUCK_RETRY_SECS};
pub use convert::ConvertError;
pub use drawn::DRAWN_JUMP;
pub use grab::{Grip, GripOn, HoldParams};
pub use layers::LayerSet;
pub use ragdoll::{ContactTally, RagdollContacts, RagdollReport, RagdollStart};
pub use placed::{ref_collision, ref_collision_with, ModelCollision, RefCollision, RefCollisionOptions};
pub use quarantine::{QuarantineReport, Remedy};
pub use swim::Stroke;
pub use water::{PlacedWater, WaterArea};
pub use world::{
    AddReport, BodyState, DirtyBy, Filter, Key, Owner, PhysParams, PhysWorld, PreparedRef, PreparedTerrain, Queries, QueryConsumer, RayHit,
    RefPose, RefreshStats, Stats, Stepper, SweepHit, PLACED_SLEEP_SPEED, TERRAIN_SHELL,
};
