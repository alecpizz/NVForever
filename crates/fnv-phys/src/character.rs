//! Kinematic capsule character controller (player and NPC movement), over rapier's
//! `KinematicCharacterController` with Z up.
//!
//! The character is a vertical capsule standing on its feet point. Each fixed step the
//! caller gives a horizontal velocity and whether jump is pressed; the controller
//! applies gravity to its own vertical speed, sweeps the capsule through the world
//! (sliding along walls, climbing steps up to `step_height`, refusing slopes steeper than
//! `max_slope_deg`, snapping down to the ground on descents) and moves its kinematic body
//! so dynamic bodies collide with it. Every value of [`CharacterParams`] is an unknown of
//! the original game [hypothesis]; physics.md §6 lists what to capture.

use crate::air;
use crate::convert::{arr3, vec3};
use crate::world::{Key, Owner, PhysWorld, TERRAIN_SHELL};
use rapier3d::control::{CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController};
use rapier3d::math::{Pose, Vector};
use rapier3d::parry::query::{DefaultQueryDispatcher, PersistentQueryDispatcher, ShapeCastOptions};
use rapier3d::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct CharacterParams {
    /// Capsule radius, game units [hypothesis].
    pub radius: f32,
    /// Total height, feet to top of head, game units ([`HEIGHT`]).
    pub height: f32,
    /// Shape of the bottom: 0 for a capsule's hemisphere; else a cone with its apex at
    /// the feet rising this many units per unit of horizontal distance to the full
    /// `radius` (physics.md §6.4.1: on an edge the game's player rests 1.46 units lower
    /// per unit it stands off the edge, which no rounded bottom allows).
    pub bottom_slope: f32,
    /// Radius of the flat tip of a cone bottom (0 for a point), game units.
    pub bottom_flat: f32,
    /// Highest ledge climbed without jumping [hypothesis].
    pub step_height: f32,
    /// How much of a step's top the character needs ahead of it to step up, game units
    /// (rapier's autostep `min_width`).
    pub step_width: f32,
    /// Steepest walkable slope, degrees from horizontal; steeper slopes are slid down
    /// [hypothesis].
    pub max_slope_deg: f32,
    /// Apex height of a standing jump above the take-off point [observed: 61.6 above the
    /// standing feet, capture mv_trial, physics.md §6.3].
    pub jump_height: f32,
    /// Gap kept between the capsule and the world (numerical skin; not a game value).
    pub skin: f32,
    /// Fastest fall speed, units/s; `None` for no limit [hypothesis].
    pub max_fall_speed: Option<f32>,
    /// Mass used to push dynamic bodies; 0 disables pushing [hypothesis].
    pub push_mass: f32,
    /// Depth of the feet below a water surface at which a character that is not on the
    /// ground starts to swim [observed: a fall into Lake Mead stopped between 94.5 and
    /// 100.2 below the surface; physics.md §6.6].
    pub swim_depth: f32,
    /// Depth of the feet below a water surface at which a character on the ground starts
    /// to swim (walking into deep water), and below which a swimmer that reaches the
    /// ground keeps swimming. The game's player swims when it wades in deep enough
    /// [observed: the owner's play of the game, 2026-10-18], although no capture saw it
    /// (`sw_trial`, `sw_walkin`: the scripted player walked the bed to 308-355 below the
    /// surface; physics.md §6.6); the depth is [hypothesis]: deeper than the floating
    /// heights seen (102.5-107.4), so a swimmer that meets a bank walks out as the
    /// captures show and does not start swimming again; the eye is then just above the
    /// surface (about 123 above the feet, §6.3).
    pub wade_depth: f32,
    /// Depth of the feet below the surface where a swimming character floats; swimming up
    /// moves it only from this height or deeper [observed: 102.52; swimming up at 45° the
    /// feet rose one loop's step past it and sank back to it, physics.md §6.6].
    pub float_depth: f32,
    /// Speed at which a swimming character above its floating height sinks to it, units/s
    /// [observed: 12.0-12.5, physics.md §6.6].
    pub swim_sink_speed: f32,
    /// A jump out of the water rises this many times `jump_height` [observed: apex 124.2
    /// against 61.6 on land; `fJumpSwimmingMult` reads 2; physics.md §6.6].
    pub swim_jump_mult: f32,
    /// Horizontal motion in the air and after landings follows the game's rules
    /// ([`crate::air`]: the velocity approaches the keys' velocity in the air, a fixed
    /// speed profile after a landing) [observed, physics.md §6.4.1]; off, the keys'
    /// velocity applies at once everywhere.
    pub air_control: bool,
}

/// The player's height, feet to the top of the head [observed: capture pc_head, standing
/// jumps in Doc Mitchell's house stopped by a level beam (head 130.5 above the feet with
/// the beam's 0.7 shell) and by a ceiling sloping 20° (130.0 at the axis); with the cone
/// top this height puts all five apexes within 0.9 units, the beam's 0.9 high and the
/// slope's 0.8 low (physics.md §6, capsule height); the editor documentation says 128
/// (level-design.md)]. Crouching keeps it (sneaking
/// jumped as high; a crouched height is not captured).
pub const HEIGHT: f32 = 128.9;

/// Crouched capsule height share when the optional `sneak-bounding-box` setting (#3845)
/// is active [design: shrinks standing height 128.9 to ~79.9 game units, clearing low
/// openings while keeping cone ends and crouched eye height well inside the collider].
pub const CROUCH_HEIGHT_SCALE: f32 = 0.62;

/// Steepest slope walked by actors [design, #2511]: 56 degrees leaves about 1.7
/// degrees below the default cone face (atan(1.58), physics.md §6.4.1/§6.9), so a thin
/// wall's edge cannot be climbed along that face beyond `step_height`. It still admits
/// the 46–49-degree paths cited in docs/design/navmesh.md §5. Shared with the engine's
/// `npc_params` and the synthetic controller tests; not a measured original-game limit.
pub const ACTOR_MAX_SLOPE_DEG: f32 = 56.0;

/// A dynamic body heavier than this (its Havok mass, as the model stores it) is not
/// pushed by a character walking into it: the character stops at it as at a wall
/// [design: the cars (MSTT `Car01`–`Car03`, Havok mass 600–1000) and the motorcycles
/// (600) do not move when the game's player runs into them, owner's playtest lists 5
/// item 59 and 7 item 37; what the game's character does to a heavy body is not captured;
/// the clutter the `pc_push` capture pushed weighs a few units; physics.md §6.10].
/// Shots, explosions and grabs still move such a body.
pub const PUSH_MAX_MASS: f32 = 250.0;

/// The collision and solver group of the characters' kinematic bodies: bodies heavier
/// than [`PUSH_MAX_MASS`] leave it out of their filters (`PhysWorld::insert_ref`).
pub(crate) const CHARACTER_SOLVER: Group = Group::GROUP_32;

impl Default for CharacterParams {
    /// Placeholders sized for a human at 70 units per metre (physics.md §6), except the
    /// jump height, measured in game (§6.3).
    fn default() -> Self {
        CharacterParams {
            radius: 22.0,
            height: HEIGHT,
            bottom_slope: 1.58,
            bottom_flat: 3.2,
            step_height: 24.0,
            step_width: 22.0,
            max_slope_deg: 45.0,
            jump_height: 61.6,
            skin: 0.5,
            max_fall_speed: None,
            push_mass: 80.0,
            swim_depth: 100.0,
            wade_depth: 115.0,
            float_depth: 102.5,
            swim_sink_speed: 12.1,
            swim_jump_mult: 2.0,
            air_control: true,
        }
    }
}

impl CharacterParams {
    /// Gap the capsule keeps from a support whose contact shell is `shell` (game units):
    /// the shell, never less than the numerical `skin`. The one rule for where a
    /// character rests, used by the walking controller and by the arrival after a
    /// teleport (physics.md §6.2) [observed: rest heights above terrain and statics; that
    /// the excess is a shell is [hypothesis]].
    pub fn rest_gap(&self, shell: f32) -> f32 {
        if shell.is_finite() {
            shell.max(self.skin)
        } else {
            self.skin
        }
    }
}

/// Per-step input.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CharacterInput {
    /// Desired horizontal velocity (x, y), units/s.
    pub velocity: [f32; 2],
    pub jump: bool,
    /// Desired vertical velocity while swimming, units/s (positive up), e.g. the forward
    /// speed times the sine of the view pitch: the game swims along the view [observed,
    /// physics.md §6.6]. Ignored on land.
    pub vertical: f32,
    /// Horizontal view direction (x, y), when known: after a landing a key pointing behind
    /// it plays the backward landing profile ([`crate::air`]); `None` plays the forward one.
    pub view: Option<[f32; 2]>,
}

/// What one step did.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CharacterStep {
    /// Feet position after the step.
    pub feet: [f32; 3],
    pub grounded: bool,
    pub sliding: bool,
    /// Downward speed at touchdown, when this step landed: where the fall met the ground,
    /// not at the step's end, so the same at every physics rate.
    pub landed_speed: Option<f32>,
    /// Gap kept from the support this step ([`CharacterParams::rest_gap`]).
    pub gap: f32,
    /// Swimming after the step.
    pub swimming: bool,
    /// Entities touched during the step, sorted.
    pub touched: Vec<Owner>,
}

#[derive(Debug, Clone)]
pub struct Character {
    pub id: u32,
    pub params: CharacterParams,
    body: RigidBodyHandle,
    pub(crate) shape: SharedShape,
    /// Feet position (bottom of the capsule).
    pub feet: [f32; 3],
    /// Vertical speed, units/s (positive up).
    pub vertical_speed: f32,
    pub grounded: bool,
    /// Reference colliders on these layers do not block the character; default
    /// [`crate::layers::character_ignores`] [hypothesis].
    pub ignore_layers: crate::layers::LayerSet,
    /// Gap kept from the support (the controller's offset), from the last support found
    /// ([`CharacterParams::rest_gap`]).
    pub gap: f32,
    /// Swimming (physics.md §6.6): no gravity, the input's vertical velocity applies.
    /// Kept by [`PhysWorld::place_character`], as the game keeps it across `SetPos`
    /// [observed].
    pub swimming: bool,
    /// Horizontal velocity of the last step, units/s (what the air rules start from).
    pub velocity: [f32; 2],
    /// A movement key was held when the character left the ground: in the air it
    /// approaches the keys' full velocity, not the small air control ([`crate::air`]).
    pub air_full: bool,
    /// Seconds since the last landing while on the ground, for the landing speed profile;
    /// `None` once it is over, or when the character never landed.
    pub since_landing: Option<f32>,
    /// Seconds since the character left the ground (0 on the ground).
    pub air_time: f32,
    /// Feet height where the character walked off the ground (left it without jumping,
    /// e.g. running down a hill or over a small edge); `None` on the ground, after a jump
    /// and after a placement. While the ground below is within `step_height` of this
    /// height the character is put on it, as a step down (see [`move_character`]).
    ///
    /// [`move_character`]: PhysWorld::move_character
    pub walked_off_z: Option<f32>,
    /// Highest feet height since the character left the ground (`None` on the ground):
    /// a landing starts the landing speed profile only after a drop of more than
    /// `step_height` from it (see [`landing_profile`]).
    pub air_top_z: Option<f32>,
    /// How far the feet were above the nearest support below at the start of the last
    /// step (beyond the gap kept from it), game units; `None` when nothing was within the
    /// support search (`step_height` and the terrain shell twice, plus the step's fall).
    /// For the animations: a character this close to the ground is not falling.
    pub ground_gap: Option<f32>,
    /// Other characters do not block this one: its caller keeps characters apart by
    /// other means. (Off, the controller also carries a character along with any
    /// kinematic body that comes within its contact distance, another character's
    /// included, as with a moving platform [observed: an actor walking past the standing
    /// player took it along at its own speed from 3.5 units away, rapier 0.36.0].)
    pub passes_characters: bool,
    /// Steps climbed by [`step_up_move`] (a count for reports).
    pub stepped: u32,
    /// The height a run of [`step_up_move`]'s steps started from ([`crest_base`]), kept
    /// while the steps perch the cone on edges: [`on_crest`] climbs edges up to
    /// `step_height` above it, not above feet that earlier steps of the run raised. `None`
    /// outside such a run: cleared when a step starts on walkable ground, in the air or
    /// with a jump, and by a placement.
    perch_base: Option<f32>,
    /// Hold this character where it is while it is stuck ([`Held`]): a step whose sweep
    /// ran out of the controller's sweeps without moving it is not swept again until
    /// [`STUCK_RETRY_SECS`] have passed. Off by default (the player, the tools' probes);
    /// the game turns it on for the actors it walks.
    pub hold_when_stuck: bool,
    held: Option<Held>,
    /// Steps held so far ([`Self::hold_when_stuck`], a count for reports).
    pub held_steps: u32,
}

/// A character held where it got stuck ([`Character::hold_when_stuck`]): rapier's
/// controller sweeps the shape at most [`CONTROLLER_SWEEPS`] times a move, sliding along
/// each surface it meets; pressed into a crease between two meshes every slide meets the
/// other surface and the controller's normal nudge keeps the motion left over above its
/// stop threshold, so each step used all of them, with the autostep's casts and a contact
/// pass for the ground after each, and went nowhere [observed: rapier 0.36.0 source
/// `move_shape`; in the 50-a-side battle an actor between terrain and a canyon rock's
/// mesh took 3 ms a step, one between two yuccas' meshes 22 ms, against about 0.1 ms,
/// 20 sweeps every step and the feet not moved, issue #1797]. The step that finds it so
/// holds it: the steps after it return the same standing result without sweeping, and
/// the step [`STUCK_RETRY_SECS`] after the one that held it is a full move again, which
/// moves it on when the way is free (or holds it again).
#[derive(Debug, Clone)]
struct Held {
    /// Where it stands; moved elsewhere (placed, or its feet set), it is not held.
    feet: [f32; 3],
    /// Seconds held since the move that found it stuck (the steps' `dt` summed).
    secs: f32,
    touched: Vec<Owner>,
}

/// The sweeps rapier's character controller makes at most in one move [documented:
/// rapier3d 0.36.0 `KinematicCharacterController::move_shape`, `max_iters = 20`]; a move
/// that met something on each of them used them all.
const CONTROLLER_SWEEPS: usize = 20;

/// A held character's full move comes this long after the move that held it, seconds: a
/// third of a second, which was 20 steps at 60 a second [engineering choice: the residents'
/// own stuck rule moves an actor on along its path after a second; a stuck actor's move
/// was 3-22 ms, issue #1797]. In seconds, so a held character waits as long at any step
/// rate (`PhysParams::dt`: 40 steps at 120 a second).
pub const STUCK_RETRY_SECS: f32 = 1.0 / 3.0;

/// Whether `elapsed` seconds, summed from steps of `dt`, have reached `secs`: true from the
/// step that brings the sum within half a step of it, so the sum's rounding never adds or
/// drops a step (20 steps of 1/60 s reach a third of a second however the sum rounds).
pub fn secs_reached(elapsed: f32, secs: f32, dt: f32) -> bool {
    elapsed + dt * 0.5 >= secs
}


impl Character {
    /// This character's key, for query filters (e.g. to keep its own activation ray from
    /// hitting its capsule).
    pub fn key(&self) -> Key {
        Key::Character(self.id)
    }

    /// Capsule centre in rapier's units (`k` per game unit).
    fn center(&self, k: f32) -> Vector {
        (vec3(self.feet) + Vector::Z * (self.params.height / 2.0)) * k
    }

    fn controller(&self, k: f32) -> KinematicCharacterController {
        let p = &self.params;
        KinematicCharacterController {
            up: Vector::Z,
            // Walls and anything swept against keep a static shell's gap; the support's own
            // (larger on terrain) is kept by `move_character`'s settling on the support.
            offset: CharacterLength::Absolute(self.gap.min(WALL_GAP).max(p.skin) * k),
            slide: true,
            autostep: (p.step_height > 0.0).then_some(CharacterAutostep {
                max_height: CharacterLength::Absolute(p.step_height * k),
                min_width: CharacterLength::Absolute(p.step_width * k),
                include_dynamic_bodies: false,
            }),
            max_slope_climb_angle: p.max_slope_deg.to_radians(),
            min_slope_slide_angle: p.max_slope_deg.to_radians(),
            // Snapping down onto the ground is `move_character`'s (with each support's gap).
            snap_to_ground: None,
            ..Default::default()
        }
    }
}

/// How far above and below a place [`PhysWorld::character_spot`] looks for what stands
/// under it, game units [design: up as far as `PhysState::add_npc`'s ray in the game,
/// down far less than its 400, to stay on the level asked for: navmesh heights follow the
/// collision only roughly].
const SPOT_UP: f32 = 48.0;
const SPOT_DOWN: f32 = 96.0;
/// Rays down a place, each from just under the nearest surface the last one met [design:
/// one ray meets a collider once, so a second finds another floor of the same mesh; a
/// third for a mesh with three in reach, more than a window this short holds].
const SPOT_RAYS: usize = 3;
/// The heights over its support a body is tried at, in shares of `step_height` [design:
/// a ray finds the support under the feet's middle; a cone that wide standing on a stair
/// rests on the next step's edge, higher than that, as it does walking there].
const SPOT_LIFTS: [f32; 3] = [0.0, 0.5, 1.0];
/// A ray up from the landscape meets a skin laid on it at its top within this, game units
/// ([`PhysWorld::character_spot`]) [design, #4826: the same triangle met from below].
const SKIN_SAME: f32 = 0.5;

/// Why [`PhysWorld::character_spot`] found no place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotRefused {
    /// Nothing to stand on near the height asked for.
    NoSupport,
    /// There is ground, but the body does not fit over it.
    NoRoom,
}

/// The character's shape around its centre (half the height above the feet): a vertical
/// capsule, or with `bottom_slope` a cylinder with a cone at each end (convex hull of
/// rings of [`RING_POINTS`] points circumscribing the radius). The top mirrors the bottom
/// [hypothesis: capture pc_head (physics.md §6, capsule height) does not separate the top's
/// shape; a flat top at this height stopped 5-8 units low under the sloping ceiling, the
/// cone 0.8].
fn capsule(p: &CharacterParams, k: f32) -> SharedShape {
    let half = p.height / 2.0;
    let flat = p.bottom_flat.clamp(0.0, p.radius);
    let cone = p.bottom_slope * (p.radius - flat);
    if p.bottom_slope > 0.0 && 2.0 * cone < p.height {
        let r = p.radius / (std::f32::consts::PI / RING_POINTS as f32).cos();
        let tip = flat / (std::f32::consts::PI / RING_POINTS as f32).cos();
        let mut points = vec![Vector::new(0.0, 0.0, -half) * k, Vector::new(0.0, 0.0, half) * k];
        for i in 0..RING_POINTS {
            let a = i as f32 / RING_POINTS as f32 * std::f32::consts::TAU;
            for (rr, z) in [(tip, -half), (r, cone - half), (r, half - cone), (tip, half)] {
                points.push(Vector::new(rr * a.cos(), rr * a.sin(), z) * k);
            }
        }
        if let Some(hull) = SharedShape::convex_hull(&points) {
            return hull;
        }
    }
    let half = (half - p.radius).max(0.0) * k;
    SharedShape::capsule(Vector::Z * -half, Vector::Z * half, p.radius * k)
}

/// Points per ring of a cone-bottomed character shape [engineering choice: the polygon
/// circumscribes the circle, so the radius is reached between points and exceeded by
/// under 0.3% at them].
const RING_POINTS: usize = 48;

impl PhysWorld {
    /// Add a character standing with its feet at `feet`.
    pub fn add_character(&mut self, id: u32, params: CharacterParams, feet: [f32; 3]) -> Character {
        let shape = capsule(&params, self.k);
        let owner = Owner::Character { id };
        let mut c = Character {
            id,
            params,
            body: RigidBodyHandle::invalid(),
            shape,
            feet,
            vertical_speed: 0.0,
            grounded: false,
            ignore_layers: crate::layers::character_ignores(),
            gap: params.skin,
            swimming: false,
            velocity: [0.0, 0.0],
            air_full: false,
            since_landing: None,
            air_time: 0.0,
            walked_off_z: None,
            air_top_z: None,
            ground_gap: None,
            passes_characters: false,
            stepped: 0,
            perch_base: None,
            hold_when_stuck: false,
            held: None,
            held_steps: 0,
        };
        let body =
            self.insert_body(RigidBodyBuilder::kinematic_position_based().translation(c.center(self.k)).user_data(owner.encode()));
        let groups = InteractionGroups::new(CHARACTER_SOLVER, Group::ALL, InteractionTestMode::And);
        self.w.insert_collider(ColliderBuilder::new(c.shape.clone()).user_data(owner.encode()).collision_groups(groups).solver_groups(groups), Some(body));
        c.body = body;
        self.mark_dirty(crate::world::DirtyBy::Bodies);
        c
    }

    pub fn remove_character(&mut self, c: Character) {
        self.remove_body(c.body);
    }

    /// Resize `c`'s capsule height (keeping `c.feet` fixed), for the optional
    /// `sneak-bounding-box` setting (#3845). When growing taller (standing up), verifies
    /// headroom above `c.feet` and returns `false` without resizing if overhead geometry
    /// blocks it [design: `docs/spec-notes/sneak-bounding-box.md`].
    pub fn set_character_height(&mut self, c: &mut Character, height: f32) -> bool {
        if !height.is_finite() || height <= 2.0 * c.params.radius {
            return false;
        }
        if (c.params.height - height).abs() < 1e-3 {
            return true;
        }
        let mut next_params = c.params;
        next_params.height = height;
        let next_shape = capsule(&next_params, self.k);
        if height > c.params.height {
            let mut probe = c.clone();
            probe.params = next_params;
            probe.shape = next_shape.clone();
            let lift = c.params.skin.max(0.5);
            let test_feet = [c.feet[0], c.feet[1], c.feet[2] + lift];
            if !self.queries().character_overlap(&probe, test_feet).is_empty() {
                return false;
            }
        }
        c.params.height = height;
        c.shape = next_shape;
        let colliders: Vec<ColliderHandle> = self.w.bodies[c.body].colliders().to_vec();
        for col in colliders {
            self.w.colliders[col].set_shape(c.shape.clone());
        }
        let center = c.center(self.k);
        self.w.bodies[c.body].set_translation(center, true);
        self.body_placed(c.body);
        self.mark_dirty(crate::world::DirtyBy::Bodies);
        true
    }

    /// Move a character teleport-style (no sweep), e.g. on a door transition.
    pub fn place_character(&mut self, c: &mut Character, feet: [f32; 3]) {
        c.feet = feet;
        c.vertical_speed = 0.0;
        c.grounded = false;
        c.velocity = [0.0, 0.0];
        c.since_landing = None;
        c.walked_off_z = None;
        c.air_top_z = None;
        c.held = None;
        c.perch_base = None;
        let center = c.center(self.k);
        self.w.bodies[c.body].set_translation(center, true);
        // A body that had gone to sleep (a character not moved for a while) stayed where
        // it was put while the character walked on [observed: test
        // `a_placed_character_takes_its_body_along`, rapier 0.36.0; the query before the
        // next step consumed its wake-up, as for a sleeping body that is struck].
        self.body_placed(c.body);
    }

    /// Feet where `c` could be put down near `at`, without the way there counting (for
    /// moving it past what blocks it): a walkable support on the level of `at` (of the
    /// surfaces found from [`SPOT_UP`] above to [`SPOT_DOWN`] below, the one nearest its
    /// height; never an actor or a dynamic body) and room for `c`'s own shape over it,
    /// at the gap a character rests at or, on uneven ground, up to a step above that.
    /// [`Queries::character_overlap`] says what counts as in the way.
    ///
    /// [`Queries::character_overlap`]: crate::world::Queries::character_overlap
    pub fn character_spot(&mut self, c: &Character, at: [f32; 3]) -> Result<[f32; 3], SpotRefused> {
        if !at.iter().all(|v| v.is_finite()) {
            return Err(SpotRefused::NoSupport);
        }
        let p = &c.params;
        let filter = crate::world::Filter { exclude_layers: c.ignore_layers, no_dynamic: true, no_actors: true, ..Default::default() };
        let q = self.queries();
        let (mut top, bottom) = (at[2] + SPOT_UP, at[2] - SPOT_DOWN);
        // Walkable surfaces met: (rest height, distance from `at`, the hit).
        let mut supports: Vec<(f32, f32, crate::world::RayHit)> = Vec::new();
        for _ in 0..SPOT_RAYS {
            let hits = q.ray_all([at[0], at[1], top], [0.0, 0.0, -1.0], top - bottom, &filter);
            for hit in hits.iter().filter(|h| h.normal[2] >= p.max_slope_deg.to_radians().cos()) {
                supports.push((hit.point[2] + p.rest_gap(hit.shell), (hit.point[2] - at[2]).abs(), *hit));
            }
            let Some(nearest) = hits.first() else { break };
            top = nearest.point[2] - 0.5;
            if top <= bottom {
                break;
            }
        }
        let Some(&(z, _, low)) = supports.iter().min_by(|a, b| a.1.total_cmp(&b.1)) else { return Err(SpotRefused::NoSupport) };
        let fits = |z: f32| SPOT_LIFTS.iter().map(|share| [at[0], at[1], z + share * p.step_height]).find(|&feet| q.character_overlap(c, feet).is_empty());
        if let Some(feet) = fits(z) {
            return Ok(feet);
        }
        // The landscape under a skin laid on it (a road or path mesh over the ground): the
        // skin's top, when the first thing over the landscape there is that very surface
        // [design, #4826: a navmesh triangle is coarse and runs under a road; on the
        // landscape below the road a body overlaps the road, so the place was refused and
        // a follower stuck at the road's edge was left to the navmesh surface, under the
        // road. A slab, table or upper floor has its underside below its top, a space a
        // body would be put into: those still leave no room, the level asked for kept].
        if matches!(low.owner, crate::world::Owner::Terrain { .. }) {
            let skin = supports.iter().filter(|s| s.2.point[2] > low.point[2] && !matches!(s.2.owner, crate::world::Owner::Terrain { .. })).min_by(|a, b| a.2.point[2].total_cmp(&b.2.point[2]));
            if let Some(&(z, _, upper)) = skin {
                let rise = upper.point[2] - low.point[2];
                let over = q.ray([at[0], at[1], low.point[2] + 0.1], [0.0, 0.0, 1.0], rise + 1.0, &filter);
                let laid_on = over.is_some_and(|h| h.owner == upper.owner && (h.point[2] - upper.point[2]).abs() < SKIN_SAME);
                if laid_on {
                    return fits(z).ok_or(SpotRefused::NoRoom);
                }
            }
        }
        Err(SpotRefused::NoRoom)
    }

    /// One fixed step of character movement (call once per [`PhysWorld::step`], before
    /// it, with the same `dt`).
    ///
    /// The capsule keeps the contact shell of what it stands on between itself and the
    /// bare surface ([`CharacterParams::rest_gap`], the rule the arrival uses, physics.md
    /// §6.2): before moving, a short downward sweep finds the support; its gap becomes the
    /// controller's offset for the step, and a capsule closer to a walkable support than
    /// that gap (e.g. having walked from a static floor onto terrain) is lifted to it.
    /// Wedged into a concave corner of walls, the character does not move horizontally
    /// (see [`wedged`]).
    pub fn move_character(&mut self, c: &mut Character, input: CharacterInput) -> CharacterStep {
        self.update_queries_for(crate::world::QueryConsumer::Controller);
        let (step, push) = self.plan_move(c, input);
        self.apply_move(c, push);
        step
    }

    /// [`Self::move_character`]'s work that only reads the world: the character's new
    /// state, the step, and what it pushes. Call [`Self::update_queries`] first, then
    /// [`Self::apply_move`] with the result. A character's move is put into the world as
    /// its next kinematic position, which takes effect at [`PhysWorld::step`], and the
    /// pushes change only loose bodies' velocities, which no move reads: so the moves of
    /// many characters in one step can be planned at once and applied in turn with the
    /// result each would have had alone [observed: the code; a 50-a-side battle spent
    /// about 300 ms of CPU a second moving the characters one by one, CPU profile
    /// 2026-10-23].
    pub fn plan_move(&self, c: &mut Character, input: CharacterInput) -> (CharacterStep, Push) {
        if let Some(held) = self.held_move(c, &input) {
            return held;
        }
        let start_feet = c.feet;
        let dt = self.params.dt;
        let g = self.params.gravity;
        let surface = self.water_surface_for(c.feet);
        // Out of the water: no water here [hypothesis]. Feet above the surface do not end
        // swimming: placed 101 above an interior's water while swimming the game's player
        // sank at 12 units/s for 10 s, and at Lake Las Vegas it swam on 74 above the
        // surface [observed: swim_h2o_drops, swim_lake_shore; physics.md §6.6].
        if c.swimming && surface.is_none() {
            c.swimming = false;
        }
        if let (true, Some(s)) = (c.swimming, surface) {
            // Jumping works only above the floating height, and leaves the water [observed:
            // tapped 61 below the surface while sinking, a jump of twice the land height;
            // tapped twice at the floating height, nothing; physics.md §6.6].
            if input.jump && c.feet[2] > s - c.params.float_depth + FLOAT_TOLERANCE {
                c.swimming = false;
                c.vertical_speed = (2.0 * g * (c.params.jump_height * c.params.swim_jump_mult).max(0.0)).sqrt();
            } else {
                c.vertical_speed = swim_vertical(&c.params, c.feet[2], s, input.vertical, dt);
            }
        }
        let jumping = !c.swimming && input.jump && c.grounded;
        // The step's vertical motion. Out of the water it is gravity's, exact over the step
        // ([`gravity_step`]); a swimmer moves at its stroke's speed. `air_start` is the
        // vertical speed the step started from under gravity, for the touchdown speed.
        let (fall, air_start) = if c.swimming {
            (c.vertical_speed * dt, None)
        } else {
            if jumping {
                c.vertical_speed = (2.0 * g * c.params.jump_height.max(0.0)).sqrt();
            }
            let start = c.vertical_speed;
            let (rise, end) = gravity_step(start, g, dt, c.params.max_fall_speed);
            c.vertical_speed = end;
            (rise, Some(start))
        };
        let k = self.k;
        let ignore = c.ignore_layers;
        let passes = c.passes_characters;
        let solid = solid_for(ignore, passes);
        let filter = QueryFilter::default().exclude_sensors().exclude_rigid_body(c.body).predicate(&solid);
        let walkable = c.params.max_slope_deg.to_radians().cos();

        // The support and the gap to keep from it.
        let reach = c.params.step_height + 2.0 * TERRAIN_SHELL + (-fall).max(0.0);
        let support = {
            let q = self.w.query_pipeline_with_filter(filter);
            let opts = ShapeCastOptions {
                max_time_of_impact: reach * k,
                target_distance: 0.0,
                stop_at_penetration: true,
                compute_impact_geometry_on_penetration: true,
            };
            q.cast_shape(&Pose::from_translation(c.center(k)), -Vector::Z, c.shape.as_ref(), opts)
                .map(|(h, hit)| (h, hit.time_of_impact / k, hit.normal1))
        };
        if let Some((h, toi, n)) = support {
            c.gap = c.params.rest_gap(self.shell_of(h));
            let want = c.gap / n.z.max(walkable);
            if n.z >= walkable && c.vertical_speed <= 0.0 && toi < want - 1e-3 {
                let mut lift = want - toi;
                let q = self.w.query_pipeline_with_filter(filter);
                let opts = ShapeCastOptions {
                    max_time_of_impact: lift * k,
                    target_distance: c.params.skin * k,
                    stop_at_penetration: false,
                    compute_impact_geometry_on_penetration: true,
                };
                if let Some((_, up)) = q.cast_shape(&Pose::from_translation(c.center(k)), Vector::Z, c.shape.as_ref(), opts) {
                    lift = lift.min(up.time_of_impact / k);
                }
                c.feet[2] += lift;
            }
        }
        c.ground_gap = support.map(|(_, toi, n)| (toi - c.gap / n.z.max(walkable)).max(0.0));
        let on_walkable = support.is_some_and(|(_, toi, n)| n.z >= walkable && toi <= c.gap / n.z + SUPPORT_MARGIN);
        if on_walkable || !c.grounded || jumping {
            c.perch_base = None;
        }

        let key = input.velocity;
        // Horizontal velocity this step: the keys' on the ground (after a landing, its
        // speed profile) and while swimming; in the air it approaches them (physics.md
        // §6.4.1).
        let horizontal = if c.swimming || !c.params.air_control {
            key
        } else if (c.grounded || on_walkable) && !jumping {
            air::ground_velocity(key, input.view, c.since_landing.map(|t| (t * 60.0).round() as usize))
        } else {
            if jumping {
                c.air_full = !air::is_zero(key);
                c.air_time = 0.0;
            } else if !c.air_full && !air::is_zero(key) && c.air_time <= air::AIR_FULL_WINDOW + 1e-4 {
                // Pressed just after leaving the ground: as if held at take-off.
                c.air_full = true;
            }
            air::air_step(c.velocity, key, c.air_full, dt)
        };
        let mut desired = Vector::new(horizontal[0] * dt, horizontal[1] * dt, fall);

        // Up a walkable slope the character keeps its speed along the surface: the game
        // ran up a 32-degree ramp at 288 units/s horizontally, its run speed times the
        // cosine, where sliding the flat velocity along the slope gives the cosine
        // squared (244) [observed: L4.phys.walk.porch_steps capture, physics.md §6.3].
        // Down a slope the flat velocity is kept, as before [hypothesis: not captured].
        if let Some((_, _, n)) = support.filter(|&(_, toi, n)| !c.swimming && c.grounded && c.vertical_speed <= 0.0 && n.z >= walkable && n.z < LEVEL && toi < c.gap / n.z + 1.0) {
            // In the vertical plane of the heading: the game kept its heading across a
            // slope that also fell to the side [observed: L4.phys.walk.hill_run], where
            // sliding along the surface turned the character downhill.
            let v = Vector::new(desired.x, desired.y, 0.0);
            let rise = -(v.x * n.x + v.y * n.y) / n.z;
            let along = Vector::new(v.x, v.y, rise);
            if rise > 0.0 && along.length() > 1e-9 {
                let t = along * (v.length() / along.length());
                desired = Vector::new(t.x, t.y, t.z + desired.z);
            }
        }
        let pos = Pose::from_translation(c.center(k));
        let kcc = c.controller(k);
        let mut collisions: Vec<CharacterCollision> = Vec::new();
        // Off walkable ground the vertical motion does not slide: a falling player that
        // meets a branch or an edge stays where it touched, at any steepness [observed:
        // the B11 tree drops and `mv_diag_fence_edge` kept their x and y within 0-5 units,
        // physics.md §6.4.1], where rapier's controller slides it off contacts steeper than
        // the slope limit. The horizontal motion is made first, sliding along walls.
        let two_phase = !c.swimming && !on_walkable;
        // Hits of the controller's first move (one a sweep), for `Held`.
        let sweeps;
        let mut moved = if two_phase {
            let q = self.w.query_pipeline_with_filter(filter);
            let air = KinematicCharacterController { snap_to_ground: None, ..kcc };
            let h = air.move_shape(dt, &q, c.shape.as_ref(), &pos, Vector::new(desired.x, desired.y, 0.0) * k, |hit| collisions.push(hit));
            sweeps = collisions.len();
            let fall = KinematicCharacterController { slide: false, autostep: None, ..air };
            let from = Pose::from_translation(pos.translation + h.translation);
            let mut hits = Vec::new();
            let v = fall.move_shape(dt, &q, c.shape.as_ref(), &from, Vector::Z * desired.z * k, |hit| hits.push(hit));
            // Moving down and stopped short by something facing up: it holds the character.
            let perched = desired.z < 0.0 && v.translation.z / k > desired.z + 1e-3 && hits.iter().any(|h| h.hit.normal1.z > 0.0);
            collisions.extend(hits);
            rapier3d::control::EffectiveCharacterMovement {
                translation: h.translation + v.translation,
                grounded: v.grounded || perched,
                is_sliding_down_slope: false,
            }
        } else {
            let q = self.w.query_pipeline_with_filter(filter);
            let m = kcc.move_shape(dt, &q, c.shape.as_ref(), &pos, desired * k, |hit| collisions.push(hit));
            sweeps = collisions.len();
            m
        };
        let mut touched_handles: Vec<ColliderHandle> = collisions.iter().map(|h| h.handle).collect();
        // Walls hit while sliding, and walls the capsule already touches (within its gap
        // and a margin): a step's slide does not always reach both faces of a corner.
        let wall = |n: Vector| {
            let l = n.x.hypot(n.y);
            (n.z < walkable && n.z > -walkable && l > 1e-3).then(|| [n.x / l, n.y / l])
        };
        let mut walls: Vec<[f32; 2]> = collisions.iter().filter_map(|h| wall(h.hit.normal1)).collect();
        let mut horizontal = [desired.x, desired.y];
        // `walls` is read only by `wedged` below, which is not asked in two phases and
        // answers no for a motion under its own 1e-6 floor: the scan is skipped then, with
        // the same result [observed: the code; issue #1086].
        let may_wedge = !two_phase && horizontal[0].hypot(horizontal[1]) >= WEDGE_MIN_SPEED;
        if may_wedge {
            let q = self.w.query_pipeline_with_filter(filter);
            let reach = (c.gap.max(c.params.skin) + WALL_MARGIN) * k;
            let aabb = c.shape.compute_aabb(&pos).loosened(reach);
            let mut manifolds: Vec<ContactManifold> = Vec::new();
            for (_, co) in q.intersect_aabb_conservative(aabb) {
                let pos12 = pos.inv_mul(co.position());
                manifolds.clear();
                let _ = DefaultQueryDispatcher.contact_manifolds(&pos12, c.shape.as_ref(), co.shape(), reach, &mut manifolds, &mut None);
                for m in &manifolds {
                    if m.points.iter().any(|p| p.dist <= reach) {
                        // `local_n1` points from the capsule towards the obstacle.
                        walls.extend(wall(-(pos.rotation * m.local_n1)));
                    }
                }
            }
        }
        // The motion a wedged step gave up, for the step-up below: a wedge of a door jamb
        // and a threshold's lip is a step, not a corner (issue #2494).
        let mut wedged_from = None;
        if may_wedge && wedged(horizontal, &walls) {
            wedged_from = Some(horizontal);
            horizontal = [0.0, 0.0];
            collisions.clear();
            let q = self.w.query_pipeline_with_filter(filter);
            moved = kcc.move_shape(dt, &q, c.shape.as_ref(), &pos, Vector::new(0.0, 0.0, desired.z) * k, |hit| collisions.push(hit));
            touched_handles.extend(collisions.iter().map(|h| h.handle));
        }
        // rapier 0.36 drops the whole remaining horizontal motion when its sweep meets an
        // exactly level surface and the slide's tangent comes out a hair below horizontal
        // (the slope tangent direction is degenerate for a vertical normal): about one step
        // in sixty of a walk across flat ground went nowhere [observed: tests]. Continue
        // such a step with the horizontal motion it lost.
        for _ in 0..(if two_phase { 0 } else { 2 }) {
            let want = Vector::new(horizontal[0], horizontal[1], 0.0) * k;
            let missing = want - Vector::new(moved.translation.x, moved.translation.y, 0.0);
            let level_only = !collisions.is_empty() && collisions.iter().all(|h| h.hit.normal1.z > LEVEL);
            if !level_only || want.length() < 1e-9 || missing.dot(want) <= 1e-3 * want.length_squared() {
                break;
            }
            collisions.clear();
            let from = Pose::from_translation(pos.translation + moved.translation);
            let q = self.w.query_pipeline_with_filter(filter);
            let more = kcc.move_shape(dt, &q, c.shape.as_ref(), &from, missing, |hit| collisions.push(hit));
            touched_handles.extend(collisions.iter().map(|h| h.handle));
            moved.translation += more.translation;
            moved.grounded = more.grounded;
            moved.is_sliding_down_slope = more.is_sliding_down_slope;
        }
        // How far the sweep moved the feet up, before a step-up or the settle below
        // places them: where a fall met what stopped it.
        let swept_z = moved.translation.z / k;
        // Where the feet rest on the walkable support the settle finds, for the touchdown
        // speed.
        let mut rest_z = None;
        // Up a step the controller's own autostep refused (`step_up_move`).
        let blocked = {
            let [x, y] = wedged_from.unwrap_or(horizontal);
            let want = Vector::new(x, y, 0.0) * k;
            let got = Vector::new(moved.translation.x, moved.translation.y, 0.0);
            want.length() > 1e-6 && got.dot(want) < STEP_BLOCKED * want.length_squared()
        };
        let may_step = !c.swimming && !jumping && c.vertical_speed <= 0.0 && (c.grounded || on_walkable) && c.params.step_height > 0.0;
        if may_step && blocked && (wedged_from.is_some() || collisions.iter().any(|h| h.hit.normal1.z < walkable && h.hit.normal1.z > -walkable)) {
            let q = self.w.query_pipeline_with_filter(filter);
            // Lifted by the headroom above the head, at most `step_height`: a doorway's
            // lintel or a roof's edge leaves less.
            let offset = c.gap.min(WALL_GAP).max(c.params.skin);
            let opts = ShapeCastOptions {
                max_time_of_impact: (c.params.step_height + offset) * k,
                target_distance: offset * k,
                stop_at_penetration: false,
                compute_impact_geometry_on_penetration: true,
            };
            let lift = q
                .cast_shape(&pos, Vector::Z, c.shape.as_ref(), opts)
                .map_or(c.params.step_height, |(_, hit)| (hit.time_of_impact / k - STEP_UP_MARGIN).clamp(0.0, c.params.step_height));
            let want = Vector::new(desired.x, desired.y, 0.0) * k;
            let base = crest_base(&q, c, walkable, k);
            if let Some((t, hits)) = (lift >= MIN_STEP_UP).then(|| step_up_move(&q, &kcc, c, &pos, want, lift, walkable, base, k, dt)).flatten() {
                if t.x * want.x + t.y * want.y > moved.translation.x * want.x + moved.translation.y * want.y + 1e-3 * k {
                    collisions = hits;
                    touched_handles.extend(collisions.iter().map(|h| h.handle));
                    moved.translation = t;
                    moved.grounded = true;
                    c.stepped += 1;
                    c.perch_base.get_or_insert(base);
                }
            }
        }
        // Settle on the support: the feet rest its contact shell above it (`rest_gap`,
        // physics.md §6.2) on walkable ground, lifted when closer (landing, or after walking
        // onto terrain from a static floor) and pulled down within `step_height` when the
        // step started on the ground (descents; a swimming or rising character is left):
        // on walkable support within `SUPPORT_MARGIN`, or grounded by the controller's
        // verdict. rapier calls the capsule grounded up to about 3.4 units beyond the small
        // gap kept over statics, so a fall onto a static floor lands that high; it is put
        // on the floor on the next step [observed: tests, physics.md §5.4]. Until 2026-11-11
        // it sank by a step's gravity from rest each step, g·dt²: about 0.2 s to reach the
        // margin at 60 Hz, 0.8 s at 240 (half as fast again with the exact gravity).
        // A character that walked off the ground lands as soon as walkable ground is
        // within `step_height` of where it left it (`walked_off_z`): running down a hill
        // or over a small edge, a step can miss the ground (the edge of a mesh, a crease
        // of the terrain), and without this the character fell freely until its arc met
        // the slope again, 0.2-0.4 s later, and the landing slowed it [observed: `fnvtool
        // phys walk` at the owner's steep road, physics.md §6.4.2].
        let walked_off = c.walked_off_z.filter(|_| !c.swimming && !jumping && c.vertical_speed <= 0.0);
        if !c.swimming && c.vertical_speed <= 0.0 && (on_walkable || moved.grounded || walked_off.is_some()) {
            let at = Pose::from_translation(pos.translation + moved.translation);
            let q = self.w.query_pipeline_with_filter(filter);
            let opts = ShapeCastOptions {
                max_time_of_impact: (c.params.step_height + 2.0 * TERRAIN_SHELL) * k,
                target_distance: 0.0,
                stop_at_penetration: true,
                compute_impact_geometry_on_penetration: true,
            };
            let below = q.cast_shape(&at, -Vector::Z, c.shape.as_ref(), opts).map(|(h, hit)| (h, hit.time_of_impact / k, hit.normal1));
            if let Some((h, toi, n)) = below.filter(|&(_, _, n)| n.z >= walkable) {
                let gap = c.params.rest_gap(self.shell_of(h));
                let mut d = toi - gap / n.z;
                let feet_after = c.feet[2] + moved.translation.z / k;
                let step_down = walked_off.is_some_and(|z| z - (feet_after - d) <= c.params.step_height);
                // A fall touches down where the feet rest on the support, whether the sweep
                // stopped short of it (rapier's grounded band) or went past and is lifted
                // back; a walked-off character's step down is not a fall.
                if !step_down && (moved.grounded || on_walkable || d <= 0.0) && d <= c.params.step_height {
                    rest_z = Some(feet_after - d);
                }
                // `c.grounded` is still the step's start here.
                if d > 0.0 && !(((on_walkable || c.grounded) && d <= c.params.step_height) || step_down) {
                    d = 0.0;
                }
                if d < 0.0 {
                    let opts = ShapeCastOptions { max_time_of_impact: -d * k, target_distance: c.params.skin * k, stop_at_penetration: false, compute_impact_geometry_on_penetration: true };
                    if let Some((_, up)) = q.cast_shape(&at, Vector::Z, c.shape.as_ref(), opts) {
                        d = d.max(-up.time_of_impact / k);
                    }
                }
                if d != 0.0 || on_walkable || step_down {
                    moved.translation.z -= d * k;
                    moved.grounded = true;
                    c.gap = gap;
                }
            }
        }
        // Bodies heavier than `PUSH_MAX_MASS` do not give way to a character (the
        // cars, physics.md §6.10).
        {
            let w = &self.w;
            collisions.retain(|h| {
                let body = w.colliders.get(h.handle).and_then(|col| col.parent()).and_then(|b| w.bodies.get(b));
                body.is_none_or(|b| !b.is_dynamic() || b.mass() <= PUSH_MAX_MASS)
            });
        }
        let push = Push { kcc, dt, ignore, passes, collisions: if c.params.push_mass > 0.0 { collisions } else { Vec::new() } };
        // Downward speed where the feet met the ground, for a landing. The speed at the
        // step's end comes up to a step's gravity (g·dt) after the touchdown, and where a
        // step's end falls in rapier's grounded band depends on the step, so both differed
        // with the physics rate. Under gravity v² + 2·g·z is the same all along the fall, so
        // the speed at the touchdown height follows from the step's starting speed and
        // height: the height the feet rest at on the support (`rest_z`), else where the
        // sweep stopped (on something not walkable, or after a step down), and a step-up
        // adds nothing. A swimmer's speed is its stroke's.
        let falling_speed = match air_start {
            Some(v) => {
                let touchdown = rest_z.unwrap_or(c.feet[2] + swept_z.max(fall));
                let speed = (v * v + 2.0 * g * (start_feet[2] - touchdown)).max(0.0).sqrt();
                c.params.max_fall_speed.map_or(speed, |max| speed.min(max))
            }
            None => -c.vertical_speed,
        };
        let was_grounded = c.grounded;
        c.grounded = moved.grounded;
        // The velocity the next step starts from: what was asked, or what the world let
        // through when a wall took part of it.
        let asked = [desired.x / dt, desired.y / dt];
        let got = [moved.translation.x / k / dt, moved.translation.y / k / dt];
        c.velocity = if got[0].hypot(got[1]) < 0.95 * asked[0].hypot(asked[1]) { got } else { asked };
        if was_grounded && !c.grounded && !jumping {
            // Walked off an edge: the keys held now decide the air target.
            c.air_full = !air::is_zero(key);
            c.air_time = 0.0;
            c.walked_off_z = Some(c.feet[2]);
        }
        if c.grounded || jumping || c.swimming {
            c.walked_off_z = None;
        }
        let top = c.air_top_z.unwrap_or(c.feet[2]);
        c.air_time = if c.grounded { 0.0 } else { c.air_time + dt };
        let landing_z = c.feet[2] + moved.translation.z / k;
        c.since_landing = if c.grounded && !was_grounded {
            landing_profile(falling_speed, top - landing_z, c.params.step_height).then_some(0.0)
        } else if c.grounded {
            c.since_landing.map(|t| t + dt).filter(|&t| ((t * 60.0).round() as usize) < air::LANDING_SPEEDS.len())
        } else {
            None
        };
        let mut landed_speed = None;
        if c.swimming {
            // Swimming down onto the ground ends swimming, with no landing [observed: a
            // swimmer reaching the bed walked up it; no landing is [hypothesis]], where
            // the ground is shallower than `wade_depth`; deeper (a dive to the bed) the
            // character swims on, held at the bed [hypothesis: sw_breath's swimmer stayed
            // pressed against the bed 800 down, its state not readable].
            if moved.grounded && c.vertical_speed <= 0.0 {
                let wading = surface.is_some_and(|s| c.feet[2] + moved.translation.z / k <= s - c.params.wade_depth);
                if wading {
                    // A swimmer is never on the ground [design].
                    c.grounded = false;
                } else {
                    c.swimming = false;
                }
                c.vertical_speed = 0.0;
            }
        } else if moved.grounded && c.vertical_speed <= 0.0 {
            if !was_grounded {
                landed_speed = Some(falling_speed.max(0.0));
            }
            c.vertical_speed = 0.0;
        } else if c.vertical_speed > 0.0 && moved.translation.z / k < desired.z - 1e-3 {
            // Head hit a ceiling.
            c.vertical_speed = 0.0;
        }
        c.feet = arr3(vec3(c.feet) + moved.translation / k);
        c.air_top_z = if c.grounded || c.swimming { None } else { Some(top.max(c.feet[2])) };
        // Falling (not on the ground) with the feet `swim_depth` below the surface starts
        // swimming: the fall stops there, with no landing [observed: physics.md §6.6].
        if !c.swimming && !c.grounded && c.vertical_speed < 0.0 {
            let water = self.water_surface_for(c.feet);
            if water.is_some_and(|s| c.feet[2] <= s - c.params.swim_depth) {
                c.swimming = true;
                c.vertical_speed = 0.0;
            }
        }
        // On the ground with the feet `wade_depth` below the surface (walked in): swimming
        // starts where the character is, with no buoyancy to lift it (§6.6) [hypothesis,
        // `CharacterParams::wade_depth`].
        if !c.swimming && c.grounded && !jumping && c.vertical_speed <= 0.0 {
            let water = self.water_surface_for(c.feet);
            if water.is_some_and(|s| c.feet[2] <= s - c.params.wade_depth) {
                c.swimming = true;
                c.grounded = false;
                c.vertical_speed = 0.0;
                c.since_landing = None;
            }
        }
        let mut touched: Vec<Owner> =
            touched_handles.iter().filter_map(|&h| Owner::decode(self.w.colliders.get(h)?.user_data)).collect();
        touched.sort();
        touched.dedup();
        let step = CharacterStep {
            feet: c.feet,
            grounded: c.grounded,
            sliding: moved.is_sliding_down_slope,
            landed_speed,
            gap: c.gap,
            swimming: c.swimming,
            touched,
        };
        let stuck = c.hold_when_stuck && sweeps >= CONTROLLER_SWEEPS && c.feet == start_feet && c.grounded && !c.swimming && !jumping;
        c.held = stuck.then(|| Held { feet: c.feet, secs: 0.0, touched: step.touched.clone() });
        (step, push)
    }

    /// The step of a character [`Held`] where it got stuck: standing where it is, as the
    /// move that found it stuck left it, without sweeping; `None` when it is not held, or
    /// when this step is a full move ([`STUCK_RETRY_SECS`] after the hold, a jump, moved
    /// elsewhere).
    fn held_move(&self, c: &mut Character, input: &CharacterInput) -> Option<(CharacterStep, Push)> {
        let dt = self.params.dt;
        let held = c.held.as_mut()?;
        held.secs += dt;
        if !c.hold_when_stuck || input.jump || c.swimming || !c.grounded || c.feet != held.feet || secs_reached(held.secs, STUCK_RETRY_SECS, dt) {
            c.held = None;
            return None;
        }
        let touched = held.touched.clone();
        // As the stuck move left it: on the ground, not moving.
        c.vertical_speed = 0.0;
        c.velocity = [0.0, 0.0];
        c.air_time = 0.0;
        c.since_landing = c.since_landing.map(|t| t + dt).filter(|&t| ((t * 60.0).round() as usize) < air::LANDING_SPEEDS.len());
        c.held_steps += 1;
        let push = Push { kcc: c.controller(self.k), dt, ignore: c.ignore_layers, passes: c.passes_characters, collisions: Vec::new() };
        let step = CharacterStep { feet: c.feet, grounded: true, sliding: false, landed_speed: None, gap: c.gap, swimming: false, touched };
        Some((step, push))
    }

    /// Put a move planned by [`Self::plan_move`] into the world: the bodies heavy enough
    /// to matter are not pushed (filtered in the plan); the rest take the character's
    /// impulses, and the character goes to its new place at the next step.
    pub fn apply_move(&mut self, c: &Character, push: Push) {
        if !push.collisions.is_empty() {
            let solid = solid_for(push.ignore, push.passes);
            let filter = QueryFilter::default().exclude_sensors().exclude_rigid_body(c.body).predicate(&solid);
            let w = &mut self.w;
            let mut q = w.broad_phase.as_query_pipeline_mut(
                w.narrow_phase.query_dispatcher(),
                &mut w.bodies,
                &mut w.colliders,
                filter,
            );
            push.kcc.solve_character_collision_impulses(push.dt, &mut q, c.shape.as_ref(), c.params.push_mass, &push.collisions);
        }
        let k = self.k;
        self.w.bodies[c.body].set_next_kinematic_translation(c.center(k));
    }
}

/// What a planned move pushes ([`PhysWorld::plan_move`]): the controller and step it was
/// planned with, which colliders count, and the collisions with bodies light enough to
/// give way (none when the character pushes nothing).
pub struct Push {
    kcc: KinematicCharacterController,
    dt: f32,
    ignore: crate::layers::LayerSet,
    passes: bool,
    collisions: Vec<CharacterCollision>,
}

/// What a character collides with: placed objects not on the layers it ignores, other
/// characters unless it passes through them; never actor hit volumes (shots only,
/// combat.md §5.2).
fn solid_for(ignore: crate::layers::LayerSet, passes: bool) -> impl Fn(ColliderHandle, &Collider) -> bool {
    move |_: ColliderHandle, co: &Collider| match Owner::decode(co.user_data) {
        Some(Owner::Ref { layer, .. }) => !ignore.contains(layer),
        Some(Owner::Actor { .. }) => false,
        Some(Owner::Character { .. }) => !passes,
        _ => true,
    }
}

/// Whether a landing at `speed` (downward, units/s) after a drop of `drop` units from the
/// highest point in the air starts the landing speed profile ([`air::LANDING_SPEEDS`]):
/// from [`air::LANDING_MIN_SPEED`], and only after a drop of more than `step_height`, so
/// a short hop down a slope or off a small edge does not slow a running character
/// [hypothesis: the profile was seen after jumps of 61.6 (physics.md §6.4.1); the original
/// running down the owner's steep road kept its speed, playtest 2026-10-12].
pub fn landing_profile(speed: f32, drop: f32, step_height: f32) -> bool {
    speed >= air::LANDING_MIN_SPEED && drop > step_height
}

/// One step of `dt` s under gravity `g` (units/s², pulling down) from vertical speed `v`
/// (units/s, up positive): the rise over the step (negative for a drop) and the speed at
/// its end, exact for a constant pull at any step length: `v·dt − ½·g·dt²` and `v − g·dt`.
/// A jump at `sqrt(2·g·h)` thus peaks at `h`, and a fall's time and touchdown speed for a
/// given drop are the same at every physics rate (physics.md §5.4). With a `max_fall`
/// speed the fall holds that speed from the moment it reaches it.
///
/// [design: until 2026-11-11 the speed was lowered first and the feet moved at the new
/// speed (semi-implicit Euler), which falls `½·g·dt²` farther each step: a jump peaked
/// `v0·dt/2` low (59.2 at 60 Hz, 61.0 at 240, against the original's 61.5-61.6, §6), and a
/// fall went about `½·g·t·dt` farther than gravity takes it (5.7 units after 1 s at 60 Hz).]
pub fn gravity_step(v: f32, g: f32, dt: f32, max_fall: Option<f32>) -> (f32, f32) {
    let end = v - g * dt;
    match max_fall {
        Some(max) if end < -max => {
            // The speed reaches -max `t` into the step (at its start when already past it).
            let t = if g > 0.0 { ((v + max) / g).clamp(0.0, dt) } else { 0.0 };
            (v * t - 0.5 * g * t * t - max * (dt - t), -max)
        }
        _ => (v * dt - 0.5 * g * dt * dt, end),
    }
}

/// Vertical speed of a swimming character with its feet at `feet_z` under a surface at
/// `surface`, asked to move vertically at `want` (physics.md §6.6, all [observed] at Lake
/// Mead): diving (`want` < 0) always applies; swimming up applies from the floating height
/// (`float_depth` under the surface) or deeper; otherwise, above the floating height, the
/// character sinks at `swim_sink_speed` and stops at that height; at or below it, it stays
/// at its depth (no buoyancy).
pub fn swim_vertical(p: &CharacterParams, feet_z: f32, surface: f32, want: f32, dt: f32) -> f32 {
    let float = surface - p.float_depth;
    if want < 0.0 || (want > 0.0 && feet_z <= float + FLOAT_TOLERANCE) {
        want
    } else if feet_z > float {
        -p.swim_sink_speed.min((feet_z - float) / dt.max(1e-6))
    } else {
        0.0
    }
}

/// How close above the floating height still counts as at it, game units (rounding of the
/// sink's last step) [engineering choice].
const FLOAT_TOLERANCE: f32 = 0.01;

/// Gap kept from walls and other obstacles met while moving, game units [observed: the
/// game stopped 21.9 from the yard fence's face (`L4.phys.walk.fence_stop`): a radius of
/// 21.2 (§6.4.1) and the fence's 0.7 shell; that each obstacle's own shell is kept is
/// [hypothesis], as is the terrain's 3.5 against terrain walls].
const WALL_GAP: f32 = 0.7;

/// Headroom kept above the head when a step is climbed under something, and the lowest
/// step worth trying, game units [engineering choice].
const STEP_UP_MARGIN: f32 = 0.1;
const MIN_STEP_UP: f32 = 1.0;

/// A step's horizontal motion counts as blocked below this share of what was asked
/// [engineering choice].
const STEP_BLOCKED: f32 = 0.9;

/// The least rise, game units, that counts [`step_up_move`]'s result as a step up,
/// measured from where flat ground would put the shape (the feet less the support's gap):
/// flat ground and descents come out at 0 or lower, a cone resting on a step's edge with
/// its flat tip just short of the top at about the gap or more. The rise was measured from
/// the feet with a minimum of 0.5, which refused rises of a few tenths down to slightly
/// below zero on such edges and held the player on 24-unit catwalk steps until a jump
/// [observed: `fnvtool phys walk` up the REPCONN Facility's stairs, physics.md §6.9.1];
/// [engineering choice].
const MIN_STEP_RISE: f32 = 0.25;

/// How far ahead of the axis a step's top must be walkable for [`step_up_move`] to climb
/// onto it, game units: the cone bottom first rests on the edge with the axis this far
/// short of it at most (3.2 + 24 / 1.58 for the highest step) [engineering choice].
const STEP_PROBE: f32 = 19.0;

/// A step up that rapier's autostep refuses: its last check casts the shape down onto the
/// step and refuses a steep contact normal, and the character's cone bottom (radius 3.2,
/// rising 1.58 per unit, physics.md §6.4.1) meets a step's edge with its own face, 57.7°
/// from level, whenever the flat tip is not yet over the step: so every ledge with a
/// shallow top (the camper's door steps at the Powder Ganger camp, playtest list 5 item
/// 17) needed a jump.
///
/// Here: lift the shape by `lift` (the headroom, at most `step_height`), move it by the
/// horizontal `want` (sliding along walls), and put it down again onto what is below. It
/// climbs only onto a walkable top: straight down from the axis, or from `STEP_PROBE`
/// ahead of it (the cone resting on the edge, the tip short of the top), a walkable
/// surface within a few units of where the feet came down; or onto a crest no higher
/// than `step_height` above the walkable surface the character last stood on
/// ([`on_crest`]: a threshold's lip with no top, a
/// ramp's upper end beyond a gap, which the original walks over without a jump [observed:
/// capture of the original, 2026-11-14, physics.md §6.9.2]); a steep slope rising less
/// than a step per frame is not climbed so [hypothesis: the original climbs such ledges
/// without a jump, the owner's playtest; its step height is not measured]. A step the
/// wedge rule stopped ([`wedged`]) is tried too: a door jamb and the lip beside it wedged
/// the character in the doorway's corner. Actors and creatures walk with the same rules
/// (`plan_move`).
/// Returns the translation (rapier units) and the contacts.
#[allow(clippy::too_many_arguments)]
fn step_up_move(
    q: &rapier3d::pipeline::QueryPipeline,
    kcc: &KinematicCharacterController,
    c: &Character,
    pos: &Pose,
    want: Vector,
    lift: f32,
    walkable: f32,
    base: f32,
    k: f32,
    dt: f32,
) -> Option<(Vector, Vec<CharacterCollision>)> {
    let flat = KinematicCharacterController { autostep: None, snap_to_ground: None, ..*kcc };
    let up = Vector::Z * lift * k;
    let lifted = Pose::from_translation(pos.translation + up);
    let mut hits = Vec::new();
    let h = flat.move_shape(dt, q, c.shape.as_ref(), &lifted, want, |hit| hits.push(hit));
    let across = Vector::new(h.translation.x, h.translation.y, 0.0);
    if across.length() < 1e-3 * k {
        return None;
    }
    let ahead = Pose::from_translation(lifted.translation + across);
    let opts = ShapeCastOptions { max_time_of_impact: (lift + TERRAIN_SHELL) * k, target_distance: 0.0, stop_at_penetration: false, compute_impact_geometry_on_penetration: true };
    // [design, #2511] If the bare cast misses or only reaches level ground without a
    // rise, try the horizontal sweep's gap: a small cone can still be just outside the
    // edge. This fallback must meet a crest off the flat tip, never just flat ground.
    let (down, offset) = match q.cast_shape(&ahead, -Vector::Z, c.shape.as_ref(), opts) {
        Some((_, down)) if down.normal1.z < LEVEL || lift * k - down.time_of_impact + c.gap * k >= MIN_STEP_RISE * k => (down, 0.0),
        _ => {
            let offset = c.gap.min(WALL_GAP).max(c.params.skin);
            let (_, down) = q.cast_shape(&ahead, -Vector::Z, c.shape.as_ref(), ShapeCastOptions { target_distance: offset * k, ..opts })?;
            (down, offset)
        }
    };
    let drop = down.time_of_impact;
    // [design, #2511] Subtract the cast clearance from the rise above flat ground:
    // clearance alone must not turn a level support into a step.
    let rise = lift * k - drop + (c.gap - offset) * k;
    // Or an edge met by the cone's face, off its flat tip, which the shape crossing a gap
    // comes down onto a little lower than it was held before: a plank ramp's upper end
    // beyond a threshold's lip (issue #2494). It must then be a crest (`on_crest`).
    let off_axis = (down.witness1.x - ahead.translation.x).hypot(down.witness1.y - ahead.translation.y) / k;
    if offset > 0.0 && off_axis <= c.params.bottom_flat + CREST_OFF_TIP {
        return None;
    }
    let low = rise < MIN_STEP_RISE * k;
    let sunk_onto_edge = off_axis > c.params.bottom_flat + CREST_OFF_TIP && rise > -CREST_SINK * k;
    if low && !sunk_onto_edge {
        return None;
    }
    let feet = ahead.translation - Vector::Z * (drop + c.params.height / 2.0 * k);
    let dir = across / across.length();
    let top = |from: Vector| {
        let ray = rapier3d::geometry::Ray::new(from + Vector::Z * (c.params.step_height * k), -Vector::Z);
        q.cast_ray_and_get_normal(&ray, (c.params.step_height + STEP_TOP_BELOW) * k, true).filter(|(_, r)| r.normal.z >= walkable).map(|(_, r)| (from.z + c.params.step_height * k - r.time_of_impact) / k)
    };
    let feet_z = feet.z / k;
    let on_top = [feet, feet + dir * (STEP_PROBE * k)].into_iter().any(|p| top(p).is_some_and(|z| z > feet_z - STEP_TOP_BELOW && z < feet_z + STEP_TOP_ABOVE));
    if (low || !on_top || offset > 0.0) && !on_crest(q, c, &down, dir, base, k) {
        return None;
    }
    let lowered = Vector::new(across.x, across.y, lift * k - drop);
    // The step's contacts, for pushing and the touched list.
    Some((lowered, hits))
}

/// Whether the shape [`step_up_move`] put down rests on a crest: an edge with no walkable
/// top behind it, such as the zero-thickness lip of a caravan's threshold (issue #2494,
/// physics.md §6.9.2) or the upper end of a plank ramp hanging over a gap. The cone
/// bottom meets such an edge with its own steep face, so the contact is not a walkable
/// top and the step's other checks find none (the floor under the axis lies more than
/// [`STEP_TOP_BELOW`] under the perched feet, beyond the edge is a gap). It counts when
/// the contact `down` holds the shape up, lies at most `step_height` above `base`
/// ([`crest_base`]: a higher edge stays a wall, however thin, also after earlier steps
/// perched the cone on it), and just beyond it along
/// `dir` the surface falls away more steeply than the slope limit or is walkable and no
/// higher than the edge: a steep slope continues upwards beyond its contact, and across
/// one sideways the surface stays about as high and is not walkable, so neither counts
/// [design].
fn on_crest(q: &rapier3d::pipeline::QueryPipeline, c: &Character, down: &ShapeCastHit, dir: Vector, base: f32, k: f32) -> bool {
    let p = &c.params;
    let edge = down.witness1;
    if down.normal1.z <= 0.0 || edge.z / k > base + p.step_height + CREST_EPS {
        return false;
    }
    let beyond = Vector::new(edge.x, edge.y, edge.z + p.step_height * k) + dir * (CREST_PROBE * k);
    let ray = rapier3d::geometry::Ray::new(beyond, -Vector::Z);
    let reach = p.step_height + CREST_PROBE + CREST_DROP;
    match q.cast_ray_and_get_normal(&ray, reach * k, true) {
        None => true,
        Some((_, r)) => {
            let z = (beyond.z - r.time_of_impact) / k;
            let edge_z = edge.z / k;
            z < edge_z - CREST_DROP || (r.normal.z >= p.max_slope_deg.to_radians().cos() && z <= edge_z + CREST_LEVEL)
        }
    }
}

/// The height [`on_crest`]'s limit counts from, game units: within a run of step-up
/// perches ([`Character::perch_base`]) the height the run started from (or the feet less
/// the gap, when lower); else the walkable surface straight under the feet within
/// `step_height`, as standing there (the floor under a cone placed or walked onto an
/// edge, which a perch's raised feet must not stand in for), or, with none so near, the
/// feet less the gap (a cone that jumped or fell onto an edge high above the ground steps
/// on from where it rests) [design].
fn crest_base(q: &rapier3d::pipeline::QueryPipeline, c: &Character, walkable: f32, k: f32) -> f32 {
    let start = c.feet[2] - c.gap;
    if let Some(base) = c.perch_base {
        return base.min(start);
    }
    let ray = rapier3d::geometry::Ray::new(vec3(c.feet) * k, -Vector::Z);
    q.cast_ray_and_get_normal(&ray, c.params.step_height * k, true)
        .filter(|(_, r)| r.normal.z >= walkable)
        .map_or(start, |(_, r)| (c.feet[2] - r.time_of_impact / k).min(start))
}

/// How far beyond the edge [`on_crest`] looks, and how much lower the surface there must be
/// to fall away (steeper than 45° over that distance), game units [design: past a
/// zero-thickness lip, short of the 10-22 units of gap behind a caravan's threshold].
const CREST_PROBE: f32 = 2.0;
const CREST_DROP: f32 = 2.0;
/// How much higher than the edge a walkable surface beyond it may lie and still count as
/// the edge's own top (a rounding allowance), game units [design].
const CREST_LEVEL: f32 = 0.25;
/// Rounding allowance on the crest's height limit, game units [design: an edge exactly
/// `step_height` high is taken].
const CREST_EPS: f32 = 0.01;
/// How far off the flat tip a crest's contact must lie, and how much lower than it was held
/// before the shape may come down onto it, game units [design: the shape crossing a
/// threshold's gap onto a ramp's end sank 0.56 (the test
/// `character_walks_out_over_a_threshold_lip`); on flat ground the contact lies under the
/// tip, so a slide along a wall corner is not taken as a step].
const CREST_OFF_TIP: f32 = 0.5;
const CREST_SINK: f32 = 2.0;

/// How far a step's top may lie below and above the feet set down on it (the cone
/// resting on the edge holds the feet up to `STEP_TOP_BELOW` under the top), game units
/// [engineering choice: the cone at the probe's distance is 1.58 × (19 − 3.2) = 25 above
/// its tip].
const STEP_TOP_BELOW: f32 = 4.0;
const STEP_TOP_ABOVE: f32 = 26.0;

/// How much farther than its gap a walkable support may be and still count as standing
/// on it, game units [engineering choice: covers a step's gravity from rest (½·g·dt²:
/// 0.095 units at 60 Hz, 0.38 at 30) and the controller's nudges].
const SUPPORT_MARGIN: f32 = 1.0;

/// Normal z above which a hit counts as an exactly level surface (see `move_character`).
const LEVEL: f32 = 0.99999;

/// Distance beyond the gap within which a wall the capsule already touches counts for
/// [`wedged`], game units [engineering choice].
const WALL_MARGIN: f32 = 0.5;

/// Below this horizontal motion (game units x scale) `wedged` answers no.
const WEDGE_MIN_SPEED: f32 = 1e-6;

/// Whether a horizontal motion `v` is wedged between the walls whose horizontal normals
/// (unit, pointing out of each wall) are given: every wall it pushes into would, after
/// sliding along it, push into another. The only motion along both is then up the crease
/// of the walls, which for walls a character cannot climb means no horizontal motion
/// (the multi-plane clip of sliding controllers). rapier's controller slides along one
/// wall at a time and, in a concave corner, alternated between the faces: the capsule
/// went back and forth about 2 units per step [observed: `fnvtool phys walk` into rock
/// 0x000EDE82, physics.md §6.1].
pub fn wedged(v: [f32; 2], walls: &[[f32; 2]]) -> bool {
    let speed = v[0].hypot(v[1]);
    if speed < WEDGE_MIN_SPEED {
        return false;
    }
    let eps = 1e-3 * speed;
    let dot = |a: [f32; 2], b: [f32; 2]| a[0] * b[0] + a[1] * b[1];
    let pushed: Vec<[f32; 2]> = walls.iter().copied().filter(|&n| dot(v, n) < -eps).collect();
    if pushed.len() < 2 {
        return false;
    }
    pushed.iter().all(|&a| {
        let d = dot(v, a);
        let slid = [v[0] - d * a[0], v[1] - d * a[1]];
        walls.iter().any(|&b| dot(a, b) < 0.999 && dot(slid, b) < -eps)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_wedge_but_single_walls_and_open_sides_do_not() {
        let west = [-1.0, 0.0]; // a wall east of the character, facing west
        let south = [0.0, -1.0]; // a wall north of it, facing south
        // Into the inside corner (north-east): stuck.
        assert!(wedged([1.0, 0.2], &[west, south]));
        assert!(wedged([1.0, 1.0], &[west, south]));
        // Along the corner's open side: slides.
        assert!(!wedged([1.0, -0.2], &[west, south]));
        // One wall, or the same wall twice: slides.
        assert!(!wedged([1.0, 0.2], &[west]));
        assert!(!wedged([1.0, 0.2], &[west, west]));
        // A V-shaped notch: two faces at 30 degrees either side of the motion.
        let (s, c) = 30f32.to_radians().sin_cos();
        assert!(wedged([1.0, 0.0], &[[-c, -s], [-c, s]]));
        // Not moving: nothing to decide.
        assert!(!wedged([0.0, 0.0], &[west, south]));
    }

    const G: f32 = 686.7;
    const RATES: [f32; 7] = [30.0, 60.0, 120.0, 144.0, 152.0, 160.0, 240.0];

    /// A jump's whole arc, step by step: (heights at each step's end, speeds).
    fn arc(v0: f32, dt: f32, steps: usize, max: Option<f32>) -> (Vec<f32>, Vec<f32>) {
        let (mut z, mut v) = (0.0f32, v0);
        let (mut zs, mut vs) = (Vec::new(), Vec::new());
        for _ in 0..steps {
            let (rise, end) = gravity_step(v, G, dt, max);
            z += rise;
            v = end;
            zs.push(z);
            vs.push(v);
        }
        (zs, vs)
    }

    #[test]
    fn gravity_steps_follow_the_parabola_at_every_rate() {
        let h = 61.6;
        let v0 = (2.0 * G * h).sqrt();
        for hz in RATES {
            let dt = 1.0 / hz;
            let n = (1.0 * hz).round() as usize;
            let (zs, vs) = arc(v0, dt, n, None);
            for (i, (z, v)) in zs.iter().zip(&vs).enumerate() {
                let t = (i + 1) as f32 * dt;
                let (want_z, want_v) = (v0 * t - 0.5 * G * t * t, v0 - G * t);
                assert!((z - want_z).abs() < 2e-3, "{hz} Hz, step {i}: z {z} vs {want_z}");
                assert!((v - want_v).abs() < 2e-3, "{hz} Hz, step {i}: v {v} vs {want_v}");
                // v² + 2·g·z holds at every step's end: the touchdown speed's rule.
                assert!((v * v + 2.0 * G * z - v0 * v0).abs() < 4.0, "{hz} Hz, step {i}: energy");
            }
            // The highest step's end is within a half step's drop of the apex (g·dt²/8).
            let apex = zs.iter().copied().fold(f32::MIN, f32::max);
            assert!(apex <= h + 1e-3 && h - apex <= G * dt * dt / 8.0 + 1e-3, "{hz} Hz: apex {apex}");
        }
    }

    #[test]
    fn gravity_step_holds_the_fall_at_its_max_speed() {
        let max = 500.0;
        for hz in RATES {
            let dt = 1.0 / hz;
            // From rest for 2 s: max speed reached at t1 = max/g, the drop exact either side.
            let n = (2.0 * hz).round() as usize;
            let (zs, vs) = arc(0.0, dt, n, Some(max));
            let t1 = max / G;
            for (i, (z, v)) in zs.iter().zip(&vs).enumerate() {
                let t = (i + 1) as f32 * dt;
                let want = if t <= t1 { -0.5 * G * t * t } else { -0.5 * G * t1 * t1 - max * (t - t1) };
                assert!((z - want).abs() < 0.02, "{hz} Hz, step {i}: z {z} vs {want}");
                assert!(*v >= -max && (*v - (-G * t).max(-max)).abs() < 2e-3, "{hz} Hz, step {i}: v {v}");
            }
        }
        // Already past the max (a speed set from outside): held at it for the whole step.
        let (rise, end) = gravity_step(-800.0, G, 1.0 / 60.0, Some(max));
        assert!((rise + max / 60.0).abs() < 1e-5 && end == -max, "{rise} {end}");
        // No gravity: the speed carries the feet.
        assert_eq!(gravity_step(-10.0, 0.0, 0.5, Some(max)), (-5.0, -10.0));
    }

    #[test]
    fn crouch_height_passes_low_opening_and_blocks_standup_under_lintel() {
        use crate::world::{PhysParams, PhysWorld, RefPose};
        use fnv_collide::{CollisionObject, Iso, Motion, Shape};

        let box_obj = |half_extents: [f32; 3]| CollisionObject {
            node: None,
            node_block: 0,
            node_transform: Iso::IDENTITY,
            body_transform: Iso::IDENTITY,
            motion: Motion::Static,
            layer: 1,
            mass: 0.0,
            friction: 0.5,
            restitution: 0.0,
            shape: Shape::Box { half_extents }.into(),
            shell: 0.0,
            material: None,
            wind: false,
        };
        let pose = |id: u32, position: [f32; 3]| RefPose {
            form_id: id,
            position,
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            scale: 1.0,
        };

        let mut world = PhysWorld::new(PhysParams::default());
        // Wide static floor with top surface at z = 0.
        world.add_ref(&pose(1, [0.0, 0.0, -10.0]), &[box_obj([500.0, 500.0, 10.0])]);
        // Low lintel spanning x = 80..180, bottom at z = 100 (blocks standing 128.9, clears crouched ~79.9).
        world.add_ref(&pose(2, [130.0, 0.0, 125.0]), &[box_obj([50.0, 200.0, 25.0])]);
        world.step();

        let mut ch = world.add_character(10, CharacterParams::default(), [0.0, 0.0, 1.0]);
        for _ in 0..10 {
            world.move_character(&mut ch, CharacterInput::default());
            world.step();
        }
        // Standing (128.9) cannot pass x = 80 - radius (~58).
        for _ in 0..45 {
            world.move_character(&mut ch, CharacterInput { velocity: [200.0, 0.0], ..Default::default() });
            world.step();
        }
        assert!(ch.feet[0] < 70.0, "standing character must be blocked by lintel, got x={}", ch.feet[0]);

        // Shrink for sneak-bounding-box -> walks cleanly under the lintel.
        let crouched = HEIGHT * CROUCH_HEIGHT_SCALE;
        assert!(world.set_character_height(&mut ch, crouched));
        assert!((ch.params.height - crouched).abs() < 1e-3);
        for _ in 0..45 {
            world.move_character(&mut ch, CharacterInput { velocity: [120.0, 0.0], ..Default::default() });
            world.step();
        }
        assert!(ch.feet[0] > 95.0 && ch.feet[0] < 170.0, "crouched character should be under lintel, got x={}", ch.feet[0]);

        // Headroom check refuses to stand back up while directly underneath the lintel.
        assert!(!world.set_character_height(&mut ch, HEIGHT), "must not stand up into low lintel");
        assert!((ch.params.height - crouched).abs() < 1e-3);

        // Walk clear past the lintel (x > 210), then standing back up succeeds.
        for _ in 0..90 {
            world.move_character(&mut ch, CharacterInput { velocity: [150.0, 0.0], ..Default::default() });
            world.step();
        }
        assert!(ch.feet[0] > 210.0, "should clear lintel, got x={}", ch.feet[0]);
        assert!(world.set_character_height(&mut ch, HEIGHT), "should stand back up once clear");
        assert!((ch.params.height - HEIGHT).abs() < 1e-3);
    }
}
