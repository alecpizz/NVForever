//! Detection in the running game (docs/design/detection.md): every living, drawn actor
//! (the loaded area) measures how well it perceives the player and the other actors within
//! its reach, a few times a second, with `fnv_game::detection`'s formula; so does the
//! player, from the camera. This module measures
//! what the formula needs from the world: distance, line of sight and field of view,
//! the light at the target, who is moving, running or sneaking, and the player's shots.
//!
//! The values go to the game (`Game::detection`), where GetDetected and
//! GetDetectionLevel read them; changes of band (hidden, noticed, detected) are logged,
//! and so is the player's highest band, the original's sneak meter (HIDDEN, CAUTION,
//! DANGER). Nothing acts on them yet: combat AI is the next step.
//!
//! What is measured here and how sure it is:
//! - **Who**: every two actors of the loaded area within detection reach of each other,
//!   near the player or not (`fnv_game::sight::pairs_within`) [observed: a Frenzied Chet
//!   6,682 units from the player fought Ringo within 1 s, capture detection-1 det2,
//!   2026-10-17]; the player measures every drawn actor, out of reach too (hidden there,
//!   but GetLineOfSight can read 1: 1 at 6,682 units, same capture). Actors out of the
//!   loaded area (the low process) do not detect anyone [observed: with the player in the
//!   gas station, a Frenzied Chet outdoors never fought Ringo in 155 s, same capture
//!   det3].
//! - **Line of sight**: a ray through the collision world from the detector's eyes to the
//!   target's chest, and when that is blocked a second to its eyes (a head over a low
//!   wall is seen); the world's solid shapes block them, actors do not [engineering
//!   choice; the second ray: hypothesis, fitted to capture detection-2 det4, where Ringo
//!   behind the gas station's checkout counter read GetLineOfSight 1 both ways while our
//!   ray to the chest from the player's eyes hit the counter]. Collision exists only around the player (3 × 3 cells): where either end is
//!   out of it, only the landscape blocks sight (`fnv_game::sight::over_land`, the
//!   heights read from the cells' LAND records) [engineering choice].
//!   One ray answers both ways where two actors ask the same two points (eyes to eyes),
//!   and a pair that stood still (within [`STILL`]) since the last measurement keeps
//!   that measurement's answer once (`sights`, [`KeptSight`]) [design].
//! - **Field of view**: 95° either side of where the body faces, for every actor and for
//!   the player's detection [documented: GECK wiki GetLineOfSight; observed for the
//!   player: player.GetDetected Ringo 1 to 90° off the facing on both sides and 0 from
//!   100°, capture detection-2 det4, 2026-10-17, third person]. The player's
//!   GetLineOfSight is the camera's instead: the same ±95° and wall check from where the
//!   camera is, along where it looks (`Situation::camera_sight`) [documented: same page,
//!   "the *camera*"; observed: same capture, 0 at 90° left but 1 to 110° right, the
//!   third-person camera being off to one side].
//! - **Bodies**: dying and dead actors are in no pair (they detect no one and are not
//!   detected), but the player's camera measures the drawn ones for GetLineOfSight: the
//!   same ±95° and wall check aimed at where the body lies (`body_point`: its ragdoll's
//!   trunk), told to the game as `Game::set_camera_sights` [documented: GECK wiki
//!   GetLineOfSight, on the player any object can be the target; Fallout3.esm's
//!   MS04Script advances on `Player.GetLOS FredWilksRef == 1` with Fred Wilks lying
//!   dead, #3232].
//! - **The player as a detector**: measured like an actor, its Perception against the
//!   target's Sneak [observed: player.GetDetected Ringo and Chet 1 in the original,
//!   function probes fp1/fp2 2026-10-17, also with `tdetect` on, so the toggle and
//!   `NVF_PLAY_TEST_UNSEEN` leave it measuring; 0 for Ringo out of view and at 6,682
//!   units, capture detection-1; hypothesis: the same formula]. It feeds
//!   GetDetected / GetLineOfSight only: no detection state.
//! - **Light at the target** (0–100), fitted to the original's GetActorLightAmount
//!   (docs/design/detection.md §3) [observed: o11_light 2026-11-01, o27_light
//!   2026-11-06]: outdoors 100 × the brightest channel of the weather's sunlight colour
//!   ÷ 255 (its times of day blended on the stored bytes), day or night, nothing for the
//!   ambient (Goodsprings 100 at noon and 82.35 = 210/255 at midnight, the night
//!   sunlight (166, 191, 210); NVWastelandHazy 99.2 and 64.3; HVDustStorm, black
//!   sunlight, 0); indoors the brightest channel of the cell's ambient colour (the
//!   Goodsprings cave 36.08 = 92/255, ambient (73, 92, 83)); plus each point light that
//!   reaches the target, its brightest channel with the game's falloff 1 − (d/r)²
//!   [hypothesis: the rooms with lamps read 99–100]; capped at 100. Outdoors the
//!   sunlight counts only when a ray from the target's chest towards the sun meets
//!   nothing within [`SUN_REACH`] (where the target is in the loaded collision;
//!   elsewhere it counts); a shaded target gets the weather's ambient instead, as indoors
//!   [hypothesis, docs/design/detection.md §8]. While the player's flashlight (the Pip-Boy
//!   light, flashlight.rs) is on, the game's Pip-Boy light counts as a point light at the
//!   player's chest: the LIGH [`PIPBOY_LIGHT`] with its own radius and colour
//!   [documented: FalloutNV.esm, the loading screen "The light generated from your Pip-Boy
//!   light actually makes you less stealthy" and Broad Daylight's "the light's sneak
//!   penalty"; observed: the base effect PipLight 00021EC4, archetype Light, names that
//!   LIGH; hypothesis: the penalty is that light in the light level, docs/design/detection.md §9].
//!   Our own flashlight cone is not counted: it is not the game's light.
//! - **Moving and running**: from how far an actor went since the last measurement
//!   [engineering choice]; the player moves only while its movement controls are held (a
//!   placement settling, a push or a slide makes no footsteps; #2286) and runs when
//!   moving at the run gait (not holding Walk) [hypothesis: a sneaking run counts as
//!   running]. Other actors sneak while
//!   `Game::sneaks` says so (SetForceSneak, a package's Always Sneak; `sneaking.rs`).
//! - **Turning** (IsTurning): the heading changed by more than [`TURN_RATE`] per second
//!   since the last measurement, left or right [hypothesis: the game's threshold is not
//!   documented].
//! - **AI off** (SetActorsAI 0): the actor does not detect anyone [documented: GECK wiki
//!   SetActorsAI, "will not process … detection"]; it can still be detected.
//! - **Action sound**: an actor's (or the player's) loudest shot since the last
//!   measurement, at the
//!   weapon's sound level (WEAP VNAM: iSoundLevelLoud, Normal, Silent) [documented:
//!   GECK wiki Category:Detection], of the weapon as it was fired: the mod flags on the
//!   shot's message, a Silence mod's Silent level ("Sets the weapon's Sound Detection
//!   Level to Silent" [documented: GECK wiki Weapons, mod effects]; `hear_shot`).
//! - **Detector state**: asleep when lying on a bed; alert in a combat state, busy when
//!   in Combat with someone other than the target (`fnv_game::awareness`).
//!
//! **Detection states** (docs/design/alert.md): each measurement goes to
//! `Game::observe_threat`, which keeps those of actors the detector would attack; the
//! impacts of shots and CreateDetectionEvent are detection events, owned by the shooter,
//! measured like a target that makes the event's sound and cannot be seen
//! (`Game::hear_event`); an actor that is hit is alarmed about whoever hit it
//! (`Game::hurt_by`); then `Game::settle_awareness` changes states. Changes and
//! Detection topic lines are logged here (`alert:` lines); how alerted actors look is
//! `alert.rs`.
//!
//! **The events' sound levels** (docs/design/detection.md §8) [hypothesis; the original's
//! are not documented]: an impact sounds at its IPCT's sound level, else the firing
//! weapon's; a projectile in flight raises an event where it is each measurement at its
//! PROJ's sound level (Beam for a beam, else Bullet), owned by the shooter; an explosion
//! raises an Explosion event at its EXPL's level, owned by whoever set it off (none
//! without a cause).

use crate::limbs::ActorRig;
use crate::locomotion::Dying;
use crate::ragdoll::Corpse;
use crate::sky::SkyState;
use crate::world::{bevy_to_game, WorldData};
use bevy::prelude::*;
use bevy::tasks::{ComputeTaskPool, ParallelSlice};
use fnv_combat::data::WeaponStats;
use fnv_game::awareness::{Change, DetectionEvent, EventType, State};
use fnv_game::detection::{Band, Settings, Situation};
use fnv_game::sight::in_view;
use fnv_phys::{layers, Filter, Owner, Queries};
use fnv_game::functions::senses::Motion;
use fnv_sim::FormId;
use nvf_play::combat::{ActorHit, Combat, ExplosionFx, ImpactFx, ShotFired, ShotVisuals};
use nvf_play::sfx::Sfx;
use nvf_play::control::PlayerControl;
use nvf_play::phys::PhysState;
use nvf_play::player::{MoveIntent, Player, EYE_HEIGHT};
use nvf_play::script::{with_world, ScriptState};
use nvf_play::sim::SimState;
use nvf_play::{Location, PlayerCamera};
use std::collections::{BTreeSet, HashMap};
use nvf_play::flashlight::Flashlight;

/// The game's Pip-Boy light, PipboyLight640: the light the base effect PipLight
/// (00021EC4) names [observed: FalloutNV.esm decode, 2026-10-24].
const PIPBOY_LIGHT: u32 = 0x0002923E;

/// Seconds between two measurements [engineering choice].
pub const INTERVAL: f32 = 0.25;
/// An actor turning faster than this (radians per second, about 30°/s) is turning for
/// IsTurning [hypothesis].
pub const TURN_RATE: f32 = 0.5;
/// Below this speed (units per second) an actor stands still [engineering choice].
const MOVING_SPEED: f32 = 10.0;
/// Above this speed an actor other than the player runs: halfway between the player's
/// measured walk and run (86.8 and 344 units per second, `MoveSpeeds`) [engineering choice].
const RUNNING_SPEED: f32 = 215.0;
/// The chest, as a fraction of the eye height [engineering choice].
const CHEST: f32 = 0.75;
/// A line of sight passes at most this many actors (their capsules and bodies; the
/// detector's own counts) [engineering choice].
const PAST_ACTORS: usize = 8;
/// How far towards the sun something shades a target [design].
const SUN_REACH: f32 = 8192.0;

pub struct DetectionPlugin;

impl Plugin for DetectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DetectionState>()
            .add_message::<crate::coop::PuppetShot>()
            .add_systems(Update, measure.after(nvf_play::PlaySet::Scripts).before(crate::residents::drive))
            .add_systems(Last, report);
    }
}

#[derive(Debug, Default)]
struct Stats {
    measurements: usize,
    pairs: usize,
    most_pairs: usize,
    /// Band changes of pairs with the player as the target, of pairs between actors, and
    /// of the player's own (the player as the detector), by the band changed to.
    player_changes: [usize; 3],
    actor_changes: [usize; 3],
    own_changes: [usize; 3],
    /// Pairs measured between actors both out of the player's reach; those detected.
    far_pairs: usize,
    far_detected: usize,
    /// Actors the player ever had detected.
    player_saw: BTreeSet<u32>,
    /// Actors that measured the player; those that had it noticed, and detected.
    watchers: BTreeSet<u32>,
    noticed_by: BTreeSet<u32>,
    detected_by: BTreeSet<u32>,
    /// Seconds the player spent in each band (the meter).
    meter_secs: [f32; 3],
    shots_heard: usize,
    /// The lowest and highest value any actor had the player at.
    range: Option<(f64, f64)>,
    /// State changes by (from, to); actors that were ever Alert; Detection topic lines
    /// said, and asked for without a line; detection events raised and actors alarmed
    /// by them; hits that alarmed.
    changes: std::collections::BTreeMap<(State, State), usize>,
    alerted: BTreeSet<u32>,
    barks: usize,
    barks_silent: usize,
    /// Detection lines held back by the group cooldown.
    barks_held: usize,
    events: usize,
    events_heard: usize,
    hurt_alarms: usize,
    /// Events raised by impacts, projectiles in flight and explosions (§8 of the design).
    raised: [usize; 3],
    /// Targets measured outdoors under the sun, and of those in shade.
    sun_measured: usize,
    shaded: usize,
    /// Measurements with the Pip-Boy light on, and the most light at the player then.
    pip_light_on: usize,
    pip_light_most: f64,
    /// The least and most light measured at the player.
    player_light: Option<(f64, f64)>,
    /// What the lines of sight cost.
    sight: SightCounts,
}

#[derive(Resource, Default)]
pub struct DetectionState {
    settings: Option<Settings>,
    next: f32,
    /// Whether the player's noise was under the sound cover at the last measurement
    /// (#2688): told in the log when it changes.
    covered: bool,
    /// Where each actor was at the last measurement, and when (elapsed seconds).
    last: HashMap<u32, ([f32; 3], f32)>,
    /// Each actor's compass heading at the last measurement, and when.
    headings: HashMap<u32, (f32, f32)>,
    /// The loudest shot each shooter fired since the last measurement.
    shots: HashMap<u32, f64>,
    /// The sound level of each shooter's last shot: its impacts make that sound.
    shot_levels: HashMap<u32, f64>,
    /// Impacts of shots (where, whose, the IPCT's sound), and actors hit (who, by whom),
    /// since the last measurement.
    impacts: Vec<([f32; 3], u32, Option<f64>)>,
    hits: Vec<(u32, u32)>,
    /// Explosions since the last measurement: where, whose, how loud.
    explosions: Vec<([f32; 3], u32, f64)>,
    /// Projectiles in flight since the last measurement, by their number: where they
    /// were last, whose, how loud, a beam.
    flying: HashMap<u64, ([f32; 3], u32, f64, bool)>,
    /// The player's highest band at the last measurement (the meter).
    meter: Option<Band>,
    /// Landscape heights of the exterior cells, for sight out of the collision world.
    land: LandCache,
    /// The pairs' lines of sight at the last measurement, for still pairs.
    kept: KeptSights,
    /// The Pip-Boy light's radius and strength, read from its LIGH once (`None` inside:
    /// not in the load order).
    pip_light: Option<Option<(f32, f32)>>,
    stats: Stats,
}

/// The landscape surface of each exterior cell (smoothed or stored, as drawn), read when
/// first needed (worldspace, grid square → surface; `None` for a square without land).
#[derive(Default)]
struct LandCache {
    cells: HashMap<(u32, i32, i32), Option<fnv_world::land_surface::Surface>>,
}

impl LandCache {
    /// The land's height at game (x, y) in worldspace `ws`.
    fn height(&mut self, world: &WorldData, ws: u32, x: f32, y: f32) -> Option<f32> {
        let g = fnv_world::cell_of(x, y);
        let s = self.cells.entry((ws, g.0, g.1)).or_insert_with(|| {
            let w = world.index.worldspaces.get(&ws)?;
            let ids = fnv_world::land_surface::land_ids(g, |n| w.grid.get(&n).and_then(|c| world.index.cells.get(c)).and_then(|c| c.land));
            fnv_world::land_surface::load_surface(world.lo, world.schema, &ids, nvf_play::smooth_terrain::spec())
        });
        let (ox, oy) = (g.0 as f32 * fnv_world::CELL_SIZE, g.1 as f32 * fnv_world::CELL_SIZE);
        s.as_ref().map(|s| s.height_at_clamped(x - ox, y - oy))
    }
}

/// An actor taking part in a measurement.
struct Person {
    id: u32,
    label: String,
    feet: [f32; 3],
    eyes: [f32; 3],
    /// Where it looks, horizontal unit vector (game space): the body's facing.
    forward: [f32; 2],
    /// Where it sees from: the eyes.
    view: [f32; 3],
    /// The player's camera (where it is, where it looks): GetLineOfSight's sight.
    camera: Option<([f32; 3], [f32; 2])>,
    moving: bool,
    running: bool,
    sneaking: bool,
    /// IsTurning's code: 0 not turning, 1 left, 2 right.
    turning: u8,
    /// Its character swims (the physics' word; IsSwimming).
    swimming: bool,
    /// Its character is airborne (physics: not grounded and not swimming; IsInAir).
    in_air: bool,
}

impl Person {
    fn chest(&self) -> [f32; 3] {
        let h = self.eyes[2] - self.feet[2];
        [self.feet[0], self.feet[1], self.feet[2] + h * CHEST]
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// A light's strength for the light level: its brightest channel, 0–1 (the original's
/// light level at an actor follows it: detection.md §3).
fn strength(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2])
}

/// The light level at a point, 0–100 (see the module notes).
fn light_at(sky: Option<&SkyState>, lights: &[([f32; 3], f32, f32)], at: [f32; 3], sunlit: bool) -> f64 {
    use fnv_world::sky::ColorType;
    let bytes = |c: [f32; 3]| strength(c) / 255.0;
    let mut l = match sky {
        Some(s) => match &s.interior {
            Some(c) => bytes(c.ambient.map(f32::from)),
            None if sunlit => bytes(s.color_bytes(ColorType::Sunlight)),
            None => bytes(s.color_bytes(ColorType::Ambient)),
        },
        None => 0.5,
    };
    for &(p, radius, lum) in lights {
        let d = distance(p, at);
        if d < radius {
            l += lum * (1.0 - (d / radius).powi(2));
        }
    }
    (l as f64 * 100.0).clamp(0.0, 100.0)
}

/// Layers sight passes through: the transparent layers, invisible walls and small loose
/// things [hypothesis: by the layers' names; a "Transparent Small" fence beside a Fiend's
/// path hid the player from it in a test, 2026-10-11].
fn see_through() -> layers::LayerSet {
    layers::LayerSet::from_layers(&[
        layers::TRANSPARENT,
        layers::TRANSPARENT_SMALL,
        layers::TRANSPARENT_SMALL_ANIM,
        layers::INVISIBLE_WALL,
        layers::CLUTTER,
        layers::DEBRIS_SMALL,
        layers::SHELL_CASING,
    ])
}

/// Test aid `NVF_PLAY_TEST_UNSEEN=1`: nobody perceives the player (its pairs and its
/// detection events are not measured), so fights between actors can be watched from
/// close by [design: as the original's console toggle for detection].
fn unseen() -> bool {
    static UNSEEN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *UNSEEN.get_or_init(|| std::env::var("NVF_PLAY_TEST_UNSEEN").is_ok_and(|v| v == "1"))
}

/// An actor's eyes, game space: `eye` above its feet (its origin), but for a body that is
/// drawn clear of its origin and ends below that eye height (its hit volume starts more
/// than [`LIFTED`] above the origin), halfway up that body. A ceiling turret's eyes were
/// 112 units over its origin, above its own top (body 7807–7853 for HELIOSOneTower's
/// LvlTurretCeiling2048Robot at z 7760 [observed in NVForever]) and past the ceiling, so
/// it never saw the player below and never turned or fired (#1537). Anyone on its feet,
/// standing or crouched, keeps the eye height (a crouched body is shorter than 112 but
/// starts at its feet).
pub(crate) fn eyes_of(phys: Option<&PhysState>, id: u32, feet: [f32; 3], eye: f32) -> [f32; 3] {
    let lifted = phys.and_then(|p| p.world.actor_volume(id)).filter(|v| v.height > 0.0 && v.feet[2] > feet[2] + LIFTED && v.feet[2] + v.height < feet[2] + eye);
    match lifted {
        Some(v) => [v.feet[0], v.feet[1], v.feet[2] + v.height / 2.0],
        None => [feet[0], feet[1], feet[2] + eye],
    }
}

/// How far above its origin a body must start to have its eyes on it, game units
/// [design: the ceiling turret's starts 47 up; feet and a crouch start at 0].
const LIFTED: f32 = 16.0;

/// Where the player's camera looks for a dead or dying actor, game space: its ragdoll's
/// trunk (the heaviest body's mass centre) where it has one, else the middle of its hit
/// volume as drawn (its widest part: a body lying in its death pose is low), else
/// [`BODY_LIFT`] above its origin. Not the eye or chest height of a standing actor:
/// Fred Wilks' body lay at z 454 with the player's feet at 449 (#3232) [observed: the
/// report's log, `ragdoll: actor 060393EA at rest at (1896, -1574, 454)`].
fn body_point(phys: Option<&PhysState>, id: u32, feet: [f32; 3]) -> [f32; 3] {
    let world = phys.map(|p| &p.world);
    world.and_then(|w| w.ragdoll_center(id)).or_else(|| world.and_then(|w| w.actor_aim_point(id))).unwrap_or([feet[0], feet[1], feet[2] + BODY_LIFT])
}

/// [`body_point`] of a body the physics knows nothing of, units above its origin
/// [design: about a lying body's thickness].
const BODY_LIFT: f32 = 12.0;

/// The bodies (`(reference, point)`) the player's camera sees, for GetLineOfSight
/// (#3232): within the camera's ±95° about where it looks and with nothing solid
/// between it and the point (`clear`, which answers each (from, [point, point]) at once
/// as [`sights`] does; actors and the corpses' own ragdolls do not block). Bodies are in
/// no detection pair, so this is their only measurement; the player sees drawn actors at
/// any distance, bodies too (the module notes). Without a camera nothing is seen.
fn bodies_seen(camera: Option<([f32; 3], [f32; 2])>, bodies: &[(u32, [f32; 3])], clear: impl FnOnce(&[([f32; 3], [[f32; 3]; 2])]) -> Vec<bool>) -> Vec<FormId> {
    let Some((at, ahead)) = camera else { return Vec::new() };
    let asked: Vec<(u32, [f32; 3])> = bodies.iter().copied().filter(|&(_, p)| in_view(at, ahead, p)).collect();
    if asked.is_empty() {
        return Vec::new();
    }
    let asks: Vec<_> = asked.iter().map(|&(_, p)| (at, [p, p])).collect();
    asked.iter().zip(clear(&asks)).filter(|(_, c)| *c).map(|(&(id, _), _)| FormId(id)).collect()
}

/// Whether the collision world lets `from` see `to` (also the combat AI's cover test,
/// `combat_tactics`).
pub(crate) fn clear_sight(phys: Option<&mut PhysState>, from: [f32; 3], to: [f32; 3]) -> bool {
    let Some(phys) = phys else { return true };
    let filters = SightFilters::of(phys);
    clear_sight_in(&phys.world.queries(), &filters, from, to, &mut 0)
}

/// What sight rays hit: not the layers sight passes through, actors' hit volumes or the
/// player's own capsule (`PhysState::ray`'s rule); `statics` neither the actors' capsules
/// and bodies, which sight passes ([`clear_sight_in`]).
struct SightFilters {
    walk: Filter,
    statics: Filter,
}

impl SightFilters {
    fn of(phys: &PhysState) -> SightFilters {
        let mut walk = Filter { exclude_layers: layers::character_ignores().union(layers::actor_volumes()).union(see_through()), ..Default::default() };
        walk.exclude.extend(phys.player.as_ref().map(|c| c.key()));
        let statics = Filter { no_actors: true, ..walk.clone() };
        SightFilters { walk, statics }
    }
}

/// [`clear_sight`] on read-only queries, so many can run at once ([`clear_sights`]);
/// `rays` counts the rays cast.
///
/// Actors (the detector's own capsule included: the ray starts inside it) do not hide
/// what is behind them: the ray goes on past them, up to [`PAST_ACTORS`] of them. First
/// one ray that ignores the actors: where it meets nothing, nothing but actors is in the
/// way and the walk past them could only end clear, so it is not cast (the same answer,
/// one ray instead of two or more for every actor on the line) [engineering choice].
fn clear_sight_in(q: &Queries, filters: &SightFilters, from: [f32; 3], to: [f32; 3], rays: &mut usize) -> bool {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1.0 {
        return true;
    }
    let dir = d.map(|v| v / len);
    *rays += 1;
    if q.ray(from, dir, len, &filters.statics).is_none() {
        return true;
    }
    walk_past_actors(q, &filters.walk, from, dir, len, rays)
}

/// The line of sight from `from` along `dir` for `len`, going on past actors: each actor
/// the ray meets is left out and the ray cast again. Stepping past a hit instead fails
/// for a ray that starts inside a capsule: the eyes are inside the detector's own (eyes
/// 112 units up a 128.9-unit hull), the collision world's rays are solid, so it is met at
/// distance 0 again and again, and the line ended "clear" whatever was in the way
/// [observed: a ray from inside a character in a test world met it at 0.0 ten times in a
/// row, 2026-10-23; docs/design/streaming-hitches.md §4].
fn walk_past_actors(q: &Queries, filter: &Filter, from: [f32; 3], dir: [f32; 3], len: f32, rays: &mut usize) -> bool {
    let mut filter = filter.clone();
    for _ in 0..PAST_ACTORS {
        *rays += 1;
        match q.ray(from, dir, len, &filter) {
            None => return true,
            Some(h) if matches!(h.owner, Owner::Actor { .. } | Owner::Character { .. }) => filter.exclude.push(h.owner.key()),
            Some(_) => return false,
        }
    }
    true
}

/// The walk past actors as it was before [`walk_past_actors`] left them out: a step of 1
/// unit past each hit (which a ray from inside a capsule never gets out of), for
/// `NVF_PLAY_TEST_SIGHT_CHECK`.
fn crawl_past_actors(q: &Queries, filter: &Filter, from: [f32; 3], dir: [f32; 3], len: f32, rays: &mut usize) -> bool {
    let mut at = 0.0;
    for _ in 0..PAST_ACTORS {
        let start = [from[0] + dir[0] * at, from[1] + dir[1] * at, from[2] + dir[2] * at];
        *rays += 1;
        match q.ray(start, dir, len - at, filter) {
            None => return true,
            Some(h) if matches!(h.owner, Owner::Actor { .. } | Owner::Character { .. }) => at += h.distance + 1.0,
            Some(_) => return false,
        }
        if at >= len {
            return true;
        }
    }
    true
}

/// The line of sight as it was worked out before [`clear_sight_in`]'s first ray and
/// before actors were left out ([`crawl_past_actors`]), for `NVF_PLAY_TEST_SIGHT_CHECK`.
fn clear_sight_walk(q: &Queries, filters: &SightFilters, from: [f32; 3], to: [f32; 3], rays: &mut usize) -> bool {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1.0 {
        return true;
    }
    crawl_past_actors(q, &filters.walk, from, d.map(|v| v / len), len, rays)
}

/// Test aid `NVF_PLAY_TEST_SIGHT_CHECK=1`: every line of sight cast is also worked out
/// the old way (the crawl past actors alone, [`clear_sight_walk`]), every answer shared between askers
/// ([`sights`]) is cast again from its own asker, and still pairs' lines of sight are not
/// kept ([`STILL`]) but counted where keeping them would have answered otherwise. The
/// detection summary gives the counts.
fn sight_check() -> bool {
    static CHECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CHECK.get_or_init(|| std::env::var("NVF_PLAY_TEST_SIGHT_CHECK").is_ok_and(|v| v == "1"))
}

/// Sight rays at or above this many are cast on the compute pool, in chunks of
/// [`RAY_CHUNK`] [engineering choice: fewer are cheaper on this thread].
const PARALLEL_RAYS: usize = 32;
const RAY_CHUNK: usize = 16;
/// Pairs whose hostility is worked out on the compute pool before a measurement's pairs
/// (`Game::round_hostility`): from this many, in chunks of [`HOSTILITY_CHUNK`]
/// [engineering choice: the first hostility check of ~11,000 pairs was 10-14 ms of a
/// 50-a-side battle's measurement on one thread, #1799].
const PARALLEL_HOSTILITY: usize = 256;
const HOSTILITY_CHUNK: usize = 256;

/// [`clear_sight`] for every (from, to) of `rays`, in order; the rays are cast on the
/// compute pool at once (the queries are read-only). A 50-a-side battle asked some
/// 20,000 lines of sight a measurement [observed: CPU profile, 2026-10-23].
fn clear_sights(phys: &mut PhysState, rays: &[([f32; 3], [f32; 3])], counts: &mut SightCounts) -> Vec<bool> {
    let filters = SightFilters::of(phys);
    let q = phys.world.queries();
    let check = sight_check();
    let one = |chunk: &[([f32; 3], [f32; 3])]| {
        let (mut cast, mut old_cast, mut differ) = (0, 0, 0);
        let out: Vec<bool> = chunk
            .iter()
            .map(|&(a, b)| {
                let clear = clear_sight_in(&q, &filters, a, b, &mut cast);
                if check && clear_sight_walk(&q, &filters, a, b, &mut old_cast) != clear {
                    differ += 1;
                }
                clear
            })
            .collect();
        (out, cast, old_cast, differ)
    };
    let parts = if rays.len() < PARALLEL_RAYS { vec![one(rays)] } else { rays.par_chunk_map(ComputeTaskPool::get(), RAY_CHUNK, |_, chunk| one(chunk)) };
    let mut out = Vec::with_capacity(rays.len());
    for (o, cast, old_cast, differ) in parts {
        out.extend(o);
        counts.rays += cast;
        counts.old_rays += old_cast;
        counts.walk_differs += differ;
    }
    out
}

/// What the lines of sight cost, for the summary.
#[derive(Debug, Default, Clone, Copy)]
struct SightCounts {
    /// Lines of sight asked through the collision world, and of them answered by a line
    /// already cast for another asker: the same two points either way round.
    asked: usize,
    shared: usize,
    /// Lines of sight of still pairs answered from the last measurement ([`STILL`]).
    kept: usize,
    /// Rays cast. With the sight check: the rays the old way cast for the same lines,
    /// the answers that differ from the old way's, the shared answers that differ from
    /// the asker's own ray, and the still pairs that could have been kept and of them
    /// those whose kept answer would have differed.
    rays: usize,
    old_rays: usize,
    walk_differs: usize,
    shared_differs: usize,
    kept_would: usize,
    kept_differs: usize,
    /// Lines of sight over a measurement's [`SIGHT_BUDGET`]: answered from their last
    /// measurement, or out of sight when there is none, and how many measurements had any.
    deferred: usize,
    deferred_new: usize,
    over_budget: usize,
}

/// Two points as bits, the same whichever comes first: the key of a line of sight that
/// serves both ways ([`sights`]).
fn segment_key(a: [f32; 3], b: [f32; 3]) -> ([u32; 3], [u32; 3]) {
    let (a, b) = (a.map(f32::to_bits), b.map(f32::to_bits));
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Whether `from` sees for detection the first point, else the second, for each
/// (from, [first, second]) of `asks`: through the collision world where both ends are in
/// it, else over the landscape alone (the module notes). The rays of each step are cast at
/// once ([`clear_sights`]).
///
/// One line answers every asker of the same two points, either way round: A's eyes to
/// B's eyes is B's eyes to A's (the second step of both, where both chests were hidden),
/// and a detection event's two points are one, so its second step is its first. The
/// collision world's solid shapes block a line whichever end it is cast from
/// [hypothesis: parry's ray casts hit triangles from both sides and solid shapes from
/// inside; checked with `NVF_PLAY_TEST_SIGHT_CHECK`].
fn sights(phys: Option<&mut PhysState>, land: &mut LandCache, world: Option<&WorldData>, exterior: bool, asks: &[([f32; 3], [[f32; 3]; 2])], counts: &mut SightCounts) -> Vec<bool> {
    let Some(phys) = phys else { return vec![true; asks.len()] };
    let mut seen = vec![false; asks.len()];
    // The answers of the lines cast so far, by their two points.
    let mut known: HashMap<([u32; 3], [u32; 3]), bool> = HashMap::new();
    let check = sight_check();
    for k in 0..2 {
        let mut rays = Vec::new();
        let mut index: HashMap<([u32; 3], [u32; 3]), usize> = HashMap::new();
        let mut cast = Vec::new();
        let mut shared = Vec::new();
        for i in 0..asks.len() {
            let (from, to) = (asks[i].0, asks[i].1[k]);
            if seen[i] {
                continue;
            }
            if phys.ready_at(from) && phys.ready_at(to) {
                counts.asked += 1;
                let key = segment_key(from, to);
                if let Some(&clear) = known.get(&key) {
                    seen[i] = clear;
                    counts.shared += 1;
                    shared.push(i);
                    continue;
                }
                let n = rays.len();
                let r = *index.entry(key).or_insert(n);
                if r == n {
                    rays.push((from, to));
                } else {
                    counts.shared += 1;
                    shared.push(i);
                }
                cast.push((i, r));
            } else {
                // Not both in the collision world: over the landscape alone.
                seen[i] = match (world, exterior) {
                    (Some(world), true) => {
                        let ws = phys.worldspace;
                        fnv_game::sight::over_land(from, to, |x, y| land.height(world, ws, x, y))
                    }
                    _ => true,
                };
            }
        }
        let clear = clear_sights(phys, &rays, counts);
        for (i, r) in cast {
            seen[i] = clear[r];
        }
        for (key, r) in index {
            known.insert(key, clear[r]);
        }
        if check && !shared.is_empty() {
            // The shared answers against a ray of the asker's own.
            let own: Vec<_> = shared.iter().map(|&i| (asks[i].0, asks[i].1[k])).collect();
            let mut spare = SightCounts::default();
            for (&i, clear) in shared.iter().zip(clear_sights(phys, &own, &mut spare)) {
                counts.shared_differs += usize::from(clear != seen[i]);
            }
            counts.old_rays += spare.old_rays;
        }
    }
    seen
}

/// A line of sight of the last measurement, kept for still pairs ([`STILL`]): where it
/// was seen from, the target's chest and eyes, the answer, and whether it was cast then
/// (not itself kept).
#[derive(Clone, Copy)]
struct KeptSight {
    from: [f32; 3],
    chest: [f32; 3],
    eyes: [f32; 3],
    clear: bool,
    cast: bool,
    /// Measurements since it was cast (0: cast at this one).
    age: u32,
}

/// A pair's line of sight is kept for the next measurement when neither the detector's
/// eyes (or the player's camera) nor the target's chest and eyes moved more than this
/// many units since it was cast. A kept answer is not kept again, so every line of sight
/// is cast at least every second measurement (half a second), and a door that opened
/// between two shows at most one measurement late [design: 4 units, about 6 cm, less
/// than a step].
const STILL: f32 = 4.0;

/// The most lines of sight a measurement casts besides the always-cast pairs below
/// [design, #1426: at 118 actors (the stress battle at 50 a side) every pair's line was
/// cast in one measurement, about 6,900, and `detection::measure` took 140-166 ms in its
/// slow frames, 21.6 ms a frame on average; with 1,024 it took 7.3 ms on average and
/// about 70 in a measurement's frame, with 512 5.0 ms and at most 39, no longer among the
/// slow frames' causes (profile build, battle-50, 2026-11-06)]. The pairs with the player and those whose
/// detector fights the target are always cast; over the budget the others are cast oldest
/// answer first, and the rest keep their last answer for this measurement (out of sight
/// when they have none). Under it nothing changes.
const SIGHT_BUDGET: usize = 512;

impl KeptSight {
    fn still(&self, from: [f32; 3], chest: [f32; 3], eyes: [f32; 3]) -> bool {
        self.cast && distance(self.from, from) <= STILL && distance(self.chest, chest) <= STILL && distance(self.eyes, eyes) <= STILL
    }
}

/// The lines of sight of the last measurement, by (detector, target, the player's
/// camera rather than its eyes), and when it was.
#[derive(Default)]
struct KeptSights {
    at: f32,
    sights: HashMap<(u32, u32, bool), KeptSight>,
}

/// A band change: detector and target (indices into the people), distance, the band
/// before, the detection, and how the target was in sight.
type BandChange = (usize, usize, f64, Option<Band>, fnv_game::detection::Detection, &'static str);

type Watched = (&'static ActorRig, &'static GlobalTransform, &'static InheritedVisibility, Has<Dying>, Has<Corpse>, Option<&'static Name>);

/// The light sources, the camera and the worldspace's records.
type Scene<'w, 's> = (
    Query<'w, 's, (&'static PointLight, &'static GlobalTransform, &'static InheritedVisibility)>,
    Query<'w, 's, (&'static SpotLight, &'static GlobalTransform, &'static InheritedVisibility), Without<crate::flashlight::PlayerFlashlight>>,
    Query<'w, 's, &'static GlobalTransform, With<PlayerCamera>>,
    Option<Res<'w, WorldData>>,
);

/// What the player is doing and where, the player's weapons and the sky.
type Inputs<'w> = (
    Option<Res<'w, Player>>,
    Option<Res<'w, MoveIntent>>,
    Option<Res<'w, PlayerControl>>,
    Option<Res<'w, Location>>,
    Option<Res<'w, Combat>>,
    Option<Res<'w, SkyState>>,
    Option<Res<'w, Flashlight>>,
);

/// A shot, heard at the next measurement: its loudness is the sound level of the weapon
/// as it was fired, the mods on the message on (`WeaponStats::modded`: a Silence mod
/// makes it Silent), not of whatever is equipped when the shot is measured; no weapon
/// data, or no level, is silent. The shooter's loudest shot since the last measurement
/// is its action sound (`shots`); its last shot's level is what its impacts sound like
/// (`shot_levels`).
fn hear_shot(state: &mut DetectionState, weapon: Option<&WeaponStats>, s: &ShotFired) {
    let fired = weapon.map(|w| if s.mods == 0 { w.sound_level.clone() } else { w.modded(s.mods).sound_level });
    let loud = fired.flatten().map_or(0.0, |l| state.settings.unwrap_or_default().firing_sound(&l));
    let heard = state.shots.entry(s.shooter.0).or_default();
    *heard = heard.max(loud);
    state.shot_levels.insert(s.shooter.0, loud);
}

/// Whether the player moves, for its footstep sound and IsMoving: faster than
/// [`MOVING_SPEED`] with its movement controls held (`intent`; without one, by speed
/// alone) (the comment at the call, #2286).
fn player_moving(speed: f32, intent: Option<&MoveIntent>) -> bool {
    speed > MOVING_SPEED && intent.is_none_or(|i| i.forward != 0.0 || i.right != 0.0)
}

/// fSneakCombatMod distracts a fighter only from actors outside its target list
/// [observed: captures run15b_t1 and run15b_t2, 2026-11-15;
/// docs/spec-notes/actor-behaviour.md §2.1a]. Lost keeps 1.2 [design: detection.md §9].
fn in_combat_with_someone_else(aware: Option<&fnv_game::awareness::Awareness>, target: FormId) -> bool {
    aware.is_some_and(|a| a.state == State::Combat && !a.targets.contains_key(&target))
}

/// Measure every pair (the module notes).
#[allow(clippy::too_many_arguments)]
fn measure(
    time: Res<Time>,
    mut state: ResMut<DetectionState>,
    mut script: Option<ResMut<ScriptState>>,
    mut sim: Option<ResMut<SimState>>,
    mut phys: Option<ResMut<PhysState>>,
    (player, intent, control, location, combat, sky, flashlight): Inputs,
    mut shots: MessageReader<ShotFired>,
    (mut impacts, mut hits, mut booms, mut puppet_shots): (MessageReader<ImpactFx>, MessageReader<ActorHit>, MessageReader<ExplosionFx>, MessageReader<crate::coop::PuppetShot>),
    (sfx, visuals): (Option<Res<Sfx>>, Option<Res<ShotVisuals>>),
    // Co-op puppets (coop.rs) are other players: on the host, targets as the player is
    // (their actors' AI is off, so they watch nobody); elsewhere, not measured at all.
    actors: Query<(Watched, Has<crate::coop::Puppet>)>,
    (point_lights, spot_lights, camera, world): Scene,
    mut gauges: Option<ResMut<nvf_play::sneak_gauges::SneakGauges>>,
    mut cover: Option<ResMut<nvf_play::sound_cover::SoundCover>>,
) {
    let state = &mut *state;
    // Shots, heard at the next measurement.
    for s in shots.read() {
        hear_shot(state, combat.as_deref().and_then(|c| c.forms.weapons.get(&s.weapon)), s);
    }
    // The other players' shots too (coop.rs), heard as the player's are.
    for s in puppet_shots.read() {
        let level = combat.as_deref().and_then(|c| c.forms.weapons.get(&s.weapon)).and_then(|w| w.sound_level.as_deref());
        let loud = level.map_or(0.0, |l| state.settings.unwrap_or_default().firing_sound(l));
        let heard = state.shots.entry(s.shooter.0).or_default();
        *heard = heard.max(loud);
        state.shot_levels.insert(s.shooter.0, loud);
    }
    let settings_now = state.settings.unwrap_or_default();
    let level = |l: Option<&str>| l.map(|l| settings_now.firing_sound(l));
    for i in impacts.read().filter(|i| i.shooter.0 != 0) {
        let own = level(sfx.as_deref().and_then(|s| s.forms.impacts.get(&i.impact)).and_then(|x| x.sound_level.as_deref()));
        state.impacts.push((i.point, i.shooter.0, own));
    }
    for b in booms.read() {
        let (Some(by), Some(c)) = (b.cause, combat.as_deref()) else { continue };
        let loud = level(c.forms.explosions.get(&b.explosion).and_then(|x| x.sound_level.as_deref())).unwrap_or(0.0);
        state.explosions.push((b.point, by.0, loud));
    }
    if let (Some(v), Some(c)) = (visuals.as_deref(), combat.as_deref()) {
        for f in v.flying.iter().filter(|f| f.owner.0 != 0) {
            let loud = level(c.forms.projectiles.get(&f.projectile).and_then(|p| p.sound_level.as_deref())).unwrap_or(0.0);
            state.flying.insert(f.id, (f.pos, f.owner.0, loud, f.beam.is_some()));
        }
    }
    for h in hits.read() {
        if let (false, Some(by)) = (h.died, h.by) {
            state.hits.push((h.form_id, by.0));
        }
    }
    // Whether the actors' sight of the player is measured: force greets ask for it
    // (`Game::greet_sight`). Told every frame, so that it holds from the first frame the
    // residents run, before the first measurement.
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        rt.game.detection.measures_player = !(unseen() || rt.game.toggles.detection_off);
    }
    let now = time.elapsed_secs();
    if now < state.next {
        return;
    }
    state.next = now + INTERVAL;
    let (Some(player), Some(sim)) = (player, sim.as_deref_mut()) else { return };
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let settings = *state.settings.get_or_insert_with(|| {
        let s = Settings::read(&rt.game);
        info!(
            "detection: base {}, skill × {}, reach {} ({} outdoors), noticed above {}, detected above 0",
            s.base_value,
            s.skill_mult,
            s.max_distance(false),
            s.max_distance(true),
            s.noticed_min
        );
        s
    });
    let exterior = location.as_deref().is_none_or(|l| l.interior.is_none());
    let reach = settings.max_distance(exterior) as f32;

    // Who takes part: the player and the living actors of the loaded area (drawn).
    let motion = |id: u32, feet: [f32; 3], last: &mut HashMap<u32, ([f32; 3], f32)>| -> f32 {
        let speed = match last.get(&id) {
            Some(&(p, t)) if now > t => distance([p[0], p[1], 0.0], [feet[0], feet[1], 0.0]) / (now - t),
            _ => 0.0,
        };
        last.insert(id, (feet, now));
        speed
    };
    // Compass heading (0 north, rising clockwise): rising is turning right.
    let turning = |id: u32, heading: f32, headings: &mut HashMap<u32, (f32, f32)>| -> u8 {
        let code = match headings.get(&id) {
            Some(&(h, t)) if now > t => {
                let d = (heading - h + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                let rate = d / (now - t);
                if rate.abs() <= TURN_RATE {
                    0
                } else if rate > 0.0 {
                    2
                } else {
                    1
                }
            }
            _ => 0,
        };
        headings.insert(id, (heading, now));
        code
    };
    let mut people = Vec::new();
    let player_run_mode = !intent.as_deref().is_some_and(|i| i.walk);
    let pspeed = motion(FormId::PLAYER_REF.0, player.feet, &mut state.last);
    // The player moves (footsteps, for the sound term and IsMoving) only while it walks or
    // runs by its own controls: being set down, settling on the floor after a placement or
    // a load, a push or a slide is no footstep [hypothesis, #2286: placed at HELIOS One's
    // stasis pods, our player drifted 4 units while its capsule settled, read as running,
    // and two protectrons the original leaves undetected noticed it by that sound].
    let pmoving = player_moving(pspeed, intent.as_deref());
    // The player detects from its eyes along its heading; GetLineOfSight is the camera's,
    // first or third person [documented: GECK wiki GetLineOfSight; observed: capture
    // detection-2 det4, the detection's edge symmetric about the facing, the line of
    // sight's not, in third person].
    let cam = camera.iter().next().map(|c| {
        let ahead = bevy_to_game(c.forward().as_vec3());
        let flat = Vec2::new(ahead.x, ahead.y).normalize_or_zero();
        (bevy_to_game(c.translation()).to_array(), [flat.x, flat.y])
    });
    people.push(Person {
        id: FormId::PLAYER_REF.0,
        label: "the player".into(),
        feet: player.feet,
        eyes: player.eye(),
        forward: [player.heading.sin(), player.heading.cos()],
        view: player.eye(),
        camera: cam,
        moving: pmoving,
        running: pmoving && !intent.as_deref().is_some_and(|i| i.walk),
        sneaking: control.as_deref().is_some_and(|c| c.sneak),
        turning: turning(FormId::PLAYER_REF.0, player.heading, &mut state.headings),
        swimming: phys.as_deref().and_then(|p| p.player.as_ref()).is_some_and(|c| c.swimming),
        in_air: phys.as_deref().and_then(|p| p.player.as_ref()).is_some_and(|c| !c.grounded && !c.swimming),
    });
    // The drawn bodies (dying or dead), measured from the player's camera alone for
    // GetLineOfSight ([`bodies_seen`]), at the point the camera looks for.
    let mut bodies: Vec<(u32, [f32; 3])> = Vec::new();
    for ((rig, g, vis, dying, corpse, name), puppet) in &actors {
        if !vis.get() || (puppet && !rt.game.is_coop_actor(FormId(rig.reference))) {
            continue;
        }
        if dying || corpse || rt.game.is_dead(FormId(rig.reference)) {
            let feet = bevy_to_game(g.translation()).to_array();
            bodies.push((rig.reference, body_point(phys.as_deref(), rig.reference, feet)));
            continue;
        }
        let feet = bevy_to_game(g.translation()).to_array();
        let ahead = bevy_to_game(g.affine().transform_vector3(Vec3::Y));
        let flat = Vec2::new(ahead.x, ahead.y).normalize_or_zero();
        let scale = g.affine().transform_vector3(Vec3::Z).length().max(0.1);
        let speed = motion(rig.reference, feet, &mut state.last);
        // "Actor 0010634A EasyPete" → "EasyPete 0010634A".
        let edid = name.and_then(|n| n.as_str().split_whitespace().nth(2)).unwrap_or("");
        let label = format!("{edid} {:08X}", rig.reference).trim().to_string();
        let eyes = eyes_of(phys.as_deref(), rig.reference, feet, EYE_HEIGHT * scale);
        people.push(Person {
            id: rig.reference,
            label,
            feet,
            eyes,
            forward: [flat.x, flat.y],
            view: eyes,
            camera: None,
            moving: speed > MOVING_SPEED,
            running: speed > RUNNING_SPEED,
            sneaking: rt.game.sneaks(FormId(rig.reference)),
            turning: turning(rig.reference, flat.x.atan2(flat.y), &mut state.headings),
            swimming: phys.as_deref().and_then(|p| p.npcs.get(&rig.reference)).is_some_and(|n| n.ch.swimming),
            in_air: phys.as_deref().and_then(|p| p.npcs.get(&rig.reference)).is_some_and(|n| !n.ch.grounded && !n.ch.swimming),
        });
    }
    state.last.retain(|_, (_, t)| now - *t < 5.0);
    state.headings.retain(|_, (_, t)| now - *t < 5.0);

    // The light sources that are on: game-space centre, radius, strength.
    let mut lights = Vec::new();
    for (l, g, v) in &point_lights {
        if v.get() {
            lights.push((bevy_to_game(g.translation()).to_array(), l.range, strength(l.color.to_srgba().to_f32_array_no_alpha())));
        }
    }
    for (l, g, v) in &spot_lights {
        if v.get() {
            lights.push((bevy_to_game(g.translation()).to_array(), l.range, strength(l.color.to_srgba().to_f32_array_no_alpha())));
        }
    }
    // The Pip-Boy light, on the player while it is on (the module notes).
    let pip_on = flashlight.as_deref().is_some_and(|f| f.on);
    if pip_on {
        let pip = *state.pip_light.get_or_insert_with(|| {
            let w = world.as_deref()?;
            let l = fnv_world::lighting::light_data(w.lo, w.schema, PIPBOY_LIGHT)?;
            Some((l.radius, strength(l.color.map(|b| b as f32 / 255.0))))
        });
        if let (Some((radius, lum)), Some(p)) = (pip, people.first()) {
            lights.push((p.chest(), radius, lum));
        }
    }
    // Outdoors with the sun up, whether something shades each one from it.
    let sun = sky.as_deref().filter(|s| s.interior.is_none()).map(SkyState::sun_dir_game).filter(|d| d[2] > 0.0);
    let mut sunlit = vec![true; people.len()];
    if let Some(d) = sun {
        // Who stands in the loaded collision is lit when nothing is between it and the
        // sun; the rest count as lit.
        let loaded: Vec<usize> = (0..people.len()).filter(|&i| phys.as_deref().is_some_and(|ph| ph.ready_at(people[i].feet))).collect();
        let rays: Vec<_> = loaded.iter().map(|&i| {
            let chest = people[i].chest();
            (chest, std::array::from_fn(|k| chest[k] + d[k] * SUN_REACH))
        }).collect();
        if let Some(ph) = phys.as_deref_mut() {
            for (&i, lit) in loaded.iter().zip(clear_sights(ph, &rays, &mut state.stats.sight)) {
                sunlit[i] = lit;
            }
        }
        state.stats.sun_measured += people.len();
        state.stats.shaded += sunlit.iter().filter(|lit| !**lit).count();
    }
    let light: Vec<f64> = people.iter().zip(&sunlit).map(|(p, &lit)| light_at(sky.as_deref(), &lights, p.chest(), lit)).collect();
    if let Some(g) = gauges.as_deref_mut() {
        g.light = light.first().copied().unwrap_or(0.0) as f32;
    }
    if let Some(&l) = light.first() {
        let (lo, hi) = state.stats.player_light.unwrap_or((l, l));
        state.stats.player_light = Some((lo.min(l), hi.max(l)));
    }
    if pip_on {
        state.stats.pip_light_on += 1;
        state.stats.pip_light_most = state.stats.pip_light_most.max(light.first().copied().unwrap_or(0.0));
    }
    // GetLightAmountAtPoint (`fnv_game::nvse_jip12`): this round's lights and base light,
    // for a point measured as [`light_at`] measures an actor (in the sun outdoors while
    // it is up).
    {
        use fnv_world::sky::ColorType;
        let bytes = |c: [f32; 3]| strength(c) / 255.0;
        let s = sky.as_deref();
        let outdoors = s.filter(|s| s.interior.is_none()).map(|s| {
            let shade = bytes(s.color_bytes(ColorType::Ambient));
            (if sun.is_some() { bytes(s.color_bytes(ColorType::Sunlight)) } else { shade }, shade)
        });
        let interior = s.and_then(|s| s.interior.as_ref()).map_or(0.5, |c| bytes(c.ambient.map(f32::from)));
        rt.game.set_light_field(fnv_game::nvse_jip12::LightField { outdoors, interior, lights: lights.clone() });
    }

    // Every detector against every target within reach.
    let shots = std::mem::take(&mut state.shots);
    if shots.get(&FormId::PLAYER_REF.0).is_some_and(|s| *s > 0.0) {
        state.stats.shots_heard += 1;
    }
    let mut phys = phys.as_deref_mut();
    let mut changes: Vec<BandChange> = Vec::new();
    let mut player_bands: Vec<(usize, fnv_game::detection::Detection)> = Vec::new();
    let (mut pairs, mut far_pairs, mut far_detected) = (0, 0, 0);
    let land = &mut state.land;
    let mut counts = state.stats.sight;
    let kept_all = std::mem::take(&mut state.kept);
    // Kept lines of sight serve still pairs at the measurement right after theirs only;
    // over the budget ([`SIGHT_BUDGET`]) a pair keeps its last answer however old, or
    // after a slow frame every pair would read as never measured and be out of sight.
    let fresh = now - kept_all.at <= INTERVAL * 1.5;
    let last_answers = kept_all.sights;
    let empty = HashMap::new();
    let kept_before = if fresh { &last_answers } else { &empty };
    let mut kept_now: HashMap<(u32, u32, bool), KeptSight> = HashMap::new();
    let check = sight_check();
    let world = world.as_deref();
    let impacts = std::mem::take(&mut state.impacts);
    let explosions = std::mem::take(&mut state.explosions);
    let flying: Vec<_> = std::mem::take(&mut state.flying).into_values().collect();
    let mut raised = [0usize; 3];
    let hits = std::mem::take(&mut state.hits);
    let shot_levels = state.shot_levels.clone();
    let (mut events, mut hurt) = (0, 0);
    let mut heard: Vec<(String, DetectionEvent, f64, f32)> = Vec::new();
    let player_shot = shots.get(&FormId::PLAYER_REF.0).copied().unwrap_or(0.0);
    let mut player_noise = 0.0;
    let mut player_cover = 0.0;
    let (alert_changes, barks, held) = with_world(rt, sim, |rt| {
        let g = &mut rt.game;
        // The player's own noise, for the sneak gauges (nvf_play::sneak_gauges).
        let me = &people[0];
        player_noise = settings.noise(&g.detection_pair(FormId::PLAYER_REF, FormId::PLAYER_REF), me.moving, me.running, player_shot);
        // The loudest sound of the world heard at the player: a noise under it is not
        // heard (`nvf_play::sound_cover`, #2688).
        player_cover = cover.as_deref_mut().map_or(0.0, |c| f64::from(c.cover(std::time::Instant::now(), player_noise)));
        // What the scripts read of movement, sneaking and swimming (IsMoving, IsRunning,
        // IsSneaking, IsSwimming): the people measured now; the others are forgotten.
        g.senses.motion.retain(|k, _| people.iter().any(|p| p.id == k.0));
        for p in &people {
            // The player's speed mode is the Walk toggle, moving or not (IsRunning).
            let run_mode = if p.id == FormId::PLAYER_REF.0 { player_run_mode } else { p.running };
            g.set_motion(FormId(p.id), Motion { moving: p.moving, running: p.running, sneaking: p.sneaking, turning: p.turning, run_mode, swimming: p.swimming, in_air: p.in_air });
        }
        // Every two within reach of each other, both ways; the player detects too.
        let feet: Vec<[f32; 3]> = people.iter().map(|p| p.feet).collect();
        let near_player: Vec<bool> = feet.iter().map(|f| distance(*f, player.feet) <= reach).collect();
        let within = fnv_game::sight::pairs_within(&feet, reach);
        // The player sees every drawn actor, whatever the distance (out of reach it is
        // hidden): player.GetLineOfSight Ringo read 1 at 6,682 units [observed: capture
        // detection-1 det2, 2026-10-17].
        let beyond: Vec<(usize, usize)> = (1..people.len()).filter(|&j| !near_player[j]).map(|j| (0, j)).collect();
        let ordered = within.into_iter().flat_map(|(a, b)| [(a, b), (b, a)]).chain(beyond);
        // SetAllVisible this frame: NPCs skip the line-of-sight rays and see what they
        // look at [documented: GECK wiki SetAllVisible]; the player's sight is the
        // camera's and keeps its rays [hypothesis].
        let all_visible = g.frame_switches.all_visible;
        // Each actor's detection inputs, factions and who would attack whom, worked out
        // once for the pairs, the events and the alarms below
        // (`Game::begin_detection_round`).
        g.begin_detection_round();
        let player_unseen = unseen() || g.toggles.detection_off;
        let ai_off: Vec<bool> = people.iter().enumerate().map(|(i, p)| i != 0 && g.ai_off(FormId(p.id))).collect();
        let ordered: Vec<(usize, usize)> = ordered.filter(|&(i, j)| !(j == 0 && player_unseen) && !ai_off[i]).collect();
        // Who would attack whom, for every pair an NPC measures, worked out at once on the
        // compute pool and kept for the round (`Game::round_hostility`): nothing the pairs
        // do below changes what it reads, so each pair gets the answer it would have
        // worked out when asked (`Game::observe_threat`).
        {
            let _span = info_span!("detection::hostility").entered();
            let ids: Vec<FormId> = people.iter().map(|p| FormId(p.id)).collect();
            g.prepare_round_hostility(&ids);
            let asks: Vec<(FormId, FormId)> = ordered.iter().filter(|&&(i, _)| i != 0).map(|&(i, j)| (FormId(people[i].id), FormId(people[j].id))).collect();
            let shared: &fnv_game::game::Game = g;
            let answer = |chunk: &[(FormId, FormId)]| chunk.iter().filter_map(|&(a, b)| shared.round_hostility(a, b).map(|h| ((a, b), h))).collect::<Vec<_>>();
            let answers: Vec<((FormId, FormId), bool)> =
                if asks.len() < PARALLEL_HOSTILITY { answer(&asks) } else { asks.par_chunk_map(ComputeTaskPool::get(), HOSTILITY_CHUNK, |_, chunk| answer(chunk)).into_iter().flatten().collect() };
            g.keep_round_hostility(answers);
        }
        // The lines of sight, cast at once (`sights`): the detector's eyes to the target's
        // chest, else its eyes; for the player also the camera's, where the target is in
        // the camera's view.
        // A still pair's line of sight of the last measurement serves again ([`STILL`]).
        // Pairs whose line is cast first when a measurement has more than its budget: the
        // player's, and a fighter's on its combat target ([`SIGHT_BUDGET`]).
        let first: Vec<bool> = ordered
            .iter()
            .map(|&(i, j)| i == 0 || j == 0 || g.awareness.get(FormId(people[i].id)).is_some_and(|a| a.state.in_combat() && a.targets.contains_key(&FormId(people[j].id))))
            .collect();
        let mut budget = SIGHT_BUDGET;
        let mut look = |asked: Vec<(usize, [f32; 3], bool)>, answers: &mut Vec<bool>| {
            let mut cast = Vec::new();
            let mut would = Vec::new();
            for (k, from, camera) in asked {
                let (d, t) = (&people[ordered[k].0], &people[ordered[k].1]);
                let key = (d.id, t.id, camera);
                let (chest, eyes) = (t.chest(), t.eyes);
                match kept_before.get(&key).filter(|s| s.still(from, chest, eyes)) {
                    Some(s) if !check => {
                        answers[k] = s.clear;
                        counts.kept += 1;
                        kept_now.insert(key, KeptSight { cast: false, age: s.age + 1, ..*s });
                    }
                    kept => {
                        if let Some(s) = kept {
                            would.push((k, s.clear));
                        }
                        cast.push((k, from, key, chest, eyes));
                    }
                }
            }
            // Over the budget: the first pairs, then the oldest answers, are cast; the
            // others keep their last answer (out of sight without one) and are older next
            // time, so every pair is cast in turn.
            let firsts = cast.iter().filter(|c| first[c.0]).count();
            if cast.len() > budget.max(firsts) {
                counts.over_budget += 1;
                let age = |key: &(u32, u32, bool)| last_answers.get(key).map_or(u32::MAX, |s| s.age);
                cast.sort_by_key(|&(k, _, key, ..)| (!first[k], std::cmp::Reverse(age(&key))));
                for (k, _, key, ..) in cast.split_off(budget.max(firsts)) {
                    match last_answers.get(&key) {
                        Some(s) => {
                            answers[k] = s.clear;
                            kept_now.insert(key, KeptSight { cast: false, age: s.age.saturating_add(1), ..*s });
                        }
                        None => {
                            answers[k] = false;
                            counts.deferred_new += 1;
                        }
                    }
                    counts.deferred += 1;
                }
            }
            budget = budget.saturating_sub(cast.len());
            let asks: Vec<_> = cast.iter().map(|&(_, from, _, chest, eyes)| (from, [chest, eyes])).collect();
            for (&(k, from, key, chest, eyes), clear) in cast.iter().zip(sights(phys.as_deref_mut(), land, world, exterior, &asks, &mut counts)) {
                answers[k] = clear;
                kept_now.insert(key, KeptSight { from, chest, eyes, clear, cast: true, age: 0 });
            }
            counts.kept_would += would.len();
            counts.kept_differs += would.iter().filter(|&&(k, clear)| answers[k] != clear).count();
        };
        let eye_asks: Vec<_> = (0..ordered.len()).filter(|&k| !(all_visible && ordered[k].0 != 0)).map(|k| (k, people[ordered[k].0].view, false)).collect();
        let mut eye_sight = vec![true; ordered.len()];
        look(eye_asks, &mut eye_sight);
        let camera_asks: Vec<_> = (0..ordered.len())
            .filter_map(|k| {
                let (d, t) = (&people[ordered[k].0], &people[ordered[k].1]);
                d.camera.filter(|&(at, ahead)| in_view(at, ahead, t.chest())).map(|(at, _)| (k, at, true))
            })
            .collect();
        let mut camera_sight = vec![false; ordered.len()];
        look(camera_asks, &mut camera_sight);
        // The bodies the camera sees: player.GetLineOfSight on a dead actor (#3232).
        let seen = bodies_seen(people[0].camera, &bodies, |asks| sights(phys.as_deref_mut(), land, world, exterior, asks, &mut counts));
        g.set_camera_sights(seen);
        for (k, &(i, j)) in ordered.iter().enumerate() {
            let (d, t) = (&people[i], &people[j]);
            let by_player = i == 0;
            let aware = g.awareness.get(FormId(d.id));
            let alert = aware.is_some_and(|a| a.state.in_combat());
            let in_combat_with_someone_else = in_combat_with_someone_else(aware, FormId(t.id));
            let dist = distance(d.feet, t.feet);
            let target = t.chest();
            let sit = Situation {
                distance: dist as f64,
                exterior,
                line_of_sight: eye_sight[k],
                in_view: in_view(d.view, d.forward, target),
                light: light[j],
                moving: t.moving,
                running: t.running,
                sneaking: t.sneaking,
                action_sound: shots.get(&t.id).copied().unwrap_or(0.0),
                sound_cover: if j == 0 { player_cover } else { 0.0 },
                detector_alert: alert,
                detector_asleep: !by_player && g.asleep(FormId(d.id)),
                in_combat_with_someone_else,
                camera_sight: d.camera.map(|_| camera_sight[k]),
            };
            let (det, before) = g.detect(&settings, FormId(d.id), FormId(t.id), &sit);
            // The player's own detection feeds the script functions only.
            if !by_player {
                g.observe_threat(FormId(d.id), FormId(t.id), &det, t.feet);
            }
            pairs += 1;
            if !near_player[i] && !near_player[j] {
                far_pairs += 1;
                far_detected += usize::from(det.band == Band::Detected);
            }
            if j == 0 {
                player_bands.push((i, det));
            }
            if before != Some(det.band) {
                let sight = match (sit.line_of_sight, sit.in_view) {
                    (true, true) => "in sight",
                    (false, _) => "out of sight (blocked)",
                    (true, false) => "out of sight (out of view)",
                };
                changes.push((i, j, dist as f64, before, det, sight));
            }
        }
        g.detection.forget_older_than(g.time - 1.0);
        // A target out of the round (its cell unloaded, out of reach) is not detected any
        // more: the fight goes Lost and ends on the timers (#3390).
        let drawn: BTreeSet<FormId> = people.iter().map(|p| FormId(p.id)).collect();
        g.lose_unmeasured(&drawn);

        // Detection events: impacts, projectiles in flight, explosions (the module
        // notes), CreateDetectionEvent.
        for (at, by, own) in impacts {
            let level = own.or_else(|| shot_levels.get(&by).copied()).unwrap_or(0.0);
            g.queue_detection_event(DetectionEvent { owner: FormId(by), at, level, kind: EventType::Bullet });
            raised[0] += 1;
        }
        for (at, by, level, beam) in flying {
            g.queue_detection_event(DetectionEvent { owner: FormId(by), at, level, kind: if beam { EventType::Beam } else { EventType::Bullet } });
            raised[1] += 1;
        }
        for (at, by, level) in explosions {
            g.queue_detection_event(DetectionEvent { owner: FormId(by), at, level, kind: EventType::Explosion });
            raised[2] += 1;
        }
        let evs: Vec<DetectionEvent> = g.take_detection_events().into_iter().filter(|ev| !((unseen() || g.toggles.detection_off) && ev.owner == FormId::PLAYER_REF)).collect();
        // Actors hear events; the player (first) does not take part.
        let mut hearers: Vec<(usize, usize, f32)> = Vec::new();
        for (e, ev) in evs.iter().enumerate() {
            for (i, d) in people.iter().enumerate().skip(1) {
                let dist = distance(d.feet, ev.at);
                if FormId(d.id) != ev.owner && dist <= reach {
                    hearers.push((e, i, dist));
                }
            }
        }
        let asks: Vec<_> = hearers.iter().map(|&(e, i, _)| (people[i].eyes, [evs[e].at, evs[e].at])).collect();
        let heard_sight = sights(phys.take(), land, world, exterior, &asks, &mut counts);
        events += evs.len();
        for (&(e, i, dist), clear) in hearers.iter().zip(heard_sight) {
            let (ev, d) = (evs[e], &people[i]);
            {
                let sit = Situation {
                    distance: dist as f64,
                    exterior,
                    line_of_sight: clear,
                    // An event is heard, not seen [hypothesis].
                    in_view: false,
                    sneaking: ev.owner == FormId::PLAYER_REF && people[0].sneaking,
                    action_sound: ev.level,
                    detector_alert: g.awareness.state(FormId(d.id)).in_combat(),
                    detector_asleep: g.asleep(FormId(d.id)),
                    ..Default::default()
                };
                let pair = g.detection_pair(FormId(d.id), ev.owner);
                let det = settings.evaluate(&pair, &sit);
                if g.hear_event(FormId(d.id), &ev, &det) {
                    heard.push((d.label.clone(), ev, det.value, dist));
                }
            }
        }
        // Hits and allies can change who would attack whom: worked out afresh from here.
        g.end_detection_hostility();
        for (id, by) in hits {
            // Hit by someone it would leave alone: it may turn on them (assault,
            // friendly fire; `fnv_game::provocation`).
            g.attacked_by(FormId(id), FormId(by));
            let Some(at) = people.iter().find(|p| p.id == by).map(|p| p.feet) else { continue };
            if g.hurt_by(FormId(id), FormId(by), at) {
                hurt += 1;
            }
        }
        // Allies that see one of their own fighting take up the fight (Assistance).
        g.call_allies();
        g.settle_awareness();
        // Who started fighting or noticed a hostile alarms its comrades near it
        // (`fnv_game::combat_group`); only those measured here: drawn (the loaded area).
        let near: std::collections::BTreeMap<FormId, [f32; 3]> = people.iter().map(|p| (FormId(p.id), p.feet)).collect();
        g.raise_alarms(&near);
        g.end_detection_round();
        let held = std::mem::take(&mut g.awareness.held_back);
        for (topic, who, by) in &held {
            debug!("alert: {} does not say {topic}: {} said a line of that kind {:.0} s ago (group cooldown)", g.data.name(*who), g.data.name(*by), g.awareness.said.iter().rev().find(|s| s.speaker == *by).map_or(0.0, |s| g.time - s.time));
        }
        let changes = g.awareness.take_changes();
        // Why each one that went into Combat about someone fights them (#3335: a player's
        // report showed an actor going for the companion and nothing saying why).
        let why: Vec<Option<String>> = changes.iter().map(|c| (c.to == State::Combat).then_some(c.about).flatten().and_then(|o| g.hostility_text(c.actor, o))).collect();
        (changes.into_iter().zip(why).collect::<Vec<_>>(), std::mem::take(&mut g.awareness.barks), held.len())
    });
    if let Some(g) = gauges.as_deref_mut() {
        g.noise = player_noise as f32;
        g.cover = player_cover as f32;
    }
    let covered = fnv_game::detection::masked(player_noise, player_cover);
    if covered != state.covered {
        state.covered = covered;
        if covered {
            info!("sound cover: the player's noise {player_noise:.0} is under the world's {player_cover:.0}: not heard");
        } else {
            info!("sound cover: the player's noise {player_noise:.0} is heard again (cover {player_cover:.0})");
        }
    }

    // The log and the counts.
    state.kept = KeptSights { at: now, sights: kept_now };
    let st = &mut state.stats;
    st.sight = counts;
    st.measurements += 1;
    st.barks_held += held;
    st.pairs += pairs;
    st.far_pairs += far_pairs;
    st.far_detected += far_detected;
    st.most_pairs = st.most_pairs.max(pairs);
    for (i, j, dist, before, det, sight) in changes {
        let (d, t) = (&people[i], &people[j]);
        let line = format!(
            "detection: {} → {}: {} → {} (value {:.1}: sound {:.1}, visual {:.1}, skills {:.1} against {:.1}; {:.0} units, {}, light {:.0}{}{})",
            d.label,
            t.label,
            before.map_or("unmeasured", Band::name),
            det.band.name(),
            det.value,
            det.sound,
            det.visual,
            det.detector_skill,
            det.target_skill,
            dist,
            sight,
            light[j],
            if t.sneaking { ", sneaking" } else { "" },
            if t.running {
                ", running"
            } else if t.moving {
                ", moving"
            } else {
                ""
            },
        );
        let k = det.band as usize;
        if j == 0 {
            st.player_changes[k] += 1;
            info!("{line}");
        } else if i == 0 {
            st.own_changes[k] += 1;
            if det.band == Band::Detected {
                st.player_saw.insert(t.id);
            }
            debug!("{line}");
        } else {
            st.actor_changes[k] += 1;
            debug!("{line}");
        }
    }
    log_alerts(st, &people, &alert_changes, &barks, &heard, events, hurt);
    for (n, r) in st.raised.iter_mut().zip(raised) {
        *n += r;
    }
    for (i, det) in &player_bands {
        let id = people[*i].id;
        st.watchers.insert(id);
        match det.band {
            Band::Noticed => {
                st.noticed_by.insert(id);
            }
            Band::Detected => {
                st.detected_by.insert(id);
            }
            Band::Hidden => {}
        }
        st.range = Some(st.range.map_or((det.value, det.value), |(lo, hi)| (lo.min(det.value), hi.max(det.value))));
    }
    let top = player_bands.iter().max_by(|a, b| a.1.value.total_cmp(&b.1.value));
    let meter = top.map_or(Band::Hidden, |(_, d)| d.band);
    st.meter_secs[meter as usize] += INTERVAL;
    if state.meter != Some(meter) {
        match top {
            Some((i, d)) => info!("detection: the player is {} ({}: {:.1})", meter.meter(), people[*i].label, d.value),
            None => info!("detection: the player is {} (no actor within reach)", meter.meter()),
        }
        state.meter = Some(meter);
    }
}

type Bark = (&'static str, FormId, Option<FormId>);

/// The `alert:` log lines and their counts.
fn log_alerts(st: &mut Stats, people: &[Person], changes: &[(Change, Option<String>)], barks: &[Bark], heard: &[(String, DetectionEvent, f64, f32)], events: usize, hurt: usize) {
    let label = |id: FormId| people.iter().find(|p| p.id == id.0).map_or_else(|| format!("{:08X}", id.0), |p| p.label.clone());
    st.events += events;
    st.events_heard += heard.len();
    st.hurt_alarms += hurt;
    for (who, ev, value, dist) in heard {
        info!("alert: {who} heard a {} event of {} (level {:.0}, value {value:.1}, {dist:.0} units away)", ev.kind.name(), label(ev.owner), ev.level);
    }
    for (c, why) in changes {
        *st.changes.entry((c.from, c.to)).or_default() += 1;
        if c.to == State::Alert {
            st.alerted.insert(c.actor.0);
        }
        let what = match (c.about, c.cause) {
            (Some(o), Some(cause)) => format!(": {} {}", cause.describe(), label(o)),
            (Some(o), None) => format!(": detects {}", label(o)),
            _ => String::new(),
        };
        let me = people.iter().find(|p| p.id == c.actor.0).map(|p| p.feet);
        let away = match (me, c.at) {
            (Some(f), Some(at)) => format!(", {:.0} units away", distance(f, at)),
            _ => String::new(),
        };
        let value = c.value.map_or(String::new(), |v| format!(", value {v:.1}"));
        info!("alert: {} {} → {}{what}{value}{away}", label(c.actor), c.from.name(), c.to.name());
        if let (Some(why), Some(o)) = (why, c.about) {
            info!("hostility: {} against {}: {why}", label(c.actor), label(o));
        }
    }
    for (topic, who, line) in barks {
        match line {
            Some(l) => {
                st.barks += 1;
                info!("alert: {} says {topic} line {:08X}", label(*who), l.0);
            }
            None => {
                st.barks_silent += 1;
                debug!("alert: {} has no {topic} line", label(*who));
            }
        }
    }
}

fn report(mut exit: MessageReader<AppExit>, state: Res<DetectionState>) {
    if exit.read().next().is_none() {
        return;
    }
    let s = &state.stats;
    let bands = |c: [usize; 3]| format!("{} to hidden, {} to noticed, {} to detected", c[0], c[1], c[2]);
    info!(
        "detection summary: {} measurements, {} pairs ({} at most at once), {} actors measured the player, {} had it noticed, {} detected; \
         the player's changes: {}; between actors: {}; the player was hidden {:.1} s, CAUTION {:.1} s, DANGER {:.1} s; {} shots heard; values {}; {} of {} measured under the sun in shade; the Pip-Boy light on at {} measurements, the player lit up to {:.0}; light at the player {}",
        s.measurements,
        s.pairs,
        s.most_pairs,
        s.watchers.len(),
        s.noticed_by.len(),
        s.detected_by.len(),
        bands(s.player_changes),
        bands(s.actor_changes),
        s.meter_secs[0],
        s.meter_secs[1],
        s.meter_secs[2],
        s.shots_heard,
        s.range.map_or("none".into(), |(lo, hi)| format!("{lo:.1} to {hi:.1}")),
        s.shaded,
        s.sun_measured,
        s.pip_light_on,
        s.pip_light_most,
        s.player_light.map_or("none".into(), |(lo, hi)| format!("{lo:.1} to {hi:.1}")),
    );
    info!(
        "detection summary, out of the player's reach: {} pairs, {} measured detected; the player as a detector: {}; {} actors detected by the player",
        s.far_pairs,
        s.far_detected,
        bands(s.own_changes),
        s.player_saw.len(),
    );
    let c = &s.sight;
    info!(
        "detection summary, lines of sight: {} asked through the collision world, {} answered by a line cast for another asker, {} kept for still pairs; {} rays cast; {} over the budget of {SIGHT_BUDGET} a measurement ({} without a last answer, in {} measurements){}",
        c.asked,
        c.shared,
        c.kept,
        c.rays,
        c.deferred,
        c.deferred_new,
        c.over_budget,
        if sight_check() {
            format!(
                "; sight check: the old way cast {} rays, {} answers differ from it, {} shared answers differ from the asker's own ray; {} still pairs could have been kept, {} of them would have answered otherwise",
                c.old_rays, c.walk_differs, c.shared_differs, c.kept_would, c.kept_differs
            )
        } else {
            String::new()
        },
    );
    let changes: Vec<String> = s.changes.iter().map(|((f, t), n)| format!("{} → {} {n}", f.name(), t.name())).collect();
    info!(
        "alert summary: {} actors became Alert; changes: {}; {} detection events ({} impacts, {} projectiles in flight, {} explosions), heard {} times; {} hits alarmed; {} Detection topic lines said, {} asked for without a line, {} held back by the group cooldown",
        s.alerted.len(),
        if changes.is_empty() { "none".into() } else { changes.join(", ") },
        s.events,
        s.raised[0],
        s.raised[1],
        s.raised[2],
        s.events_heard,
        s.hurt_alarms,
        s.barks,
        s.barks_silent,
        s.barks_held,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combat_distraction_excludes_own_targets_and_lost_detectors() {
        use fnv_game::awareness::{Awareness, Target};

        let foe = FormId(1);
        let bystander = FormId(2);
        let mut aware = Awareness::default();
        aware.targets.insert(foe, Target { band: Band::Detected, value: 10.0, at: [0.0; 3], detected: Some(0.0), hidden_since: None });
        assert!(!in_combat_with_someone_else(None, bystander));
        for state in [State::Normal, State::Alert, State::Combat, State::Lost] {
            aware.state = state;
            assert!(!in_combat_with_someone_else(Some(&aware), foe), "{state:?}: own foe");
            assert_eq!(in_combat_with_someone_else(Some(&aware), bystander), state == State::Combat, "{state:?}: bystander");
        }
        // An unseen foe still on the list is not a bystander [design: detection.md §9].
        aware.state = State::Combat;
        aware.targets.get_mut(&foe).unwrap().band = Band::Hidden;
        assert!(!in_combat_with_someone_else(Some(&aware), foe));
        aware.targets.remove(&foe);
        assert!(in_combat_with_someone_else(Some(&aware), foe));
    }

    /// A player drifting while its capsule settles makes no footsteps; one walking does,
    /// and so does one stuck to a wall only when it gets anywhere (#2286).
    #[test]
    fn the_player_moves_only_by_its_own_controls() {
        let still = MoveIntent::default();
        let walking = MoveIntent { forward: 1.0, ..MoveIntent::default() };
        let strafing = MoveIntent { right: -1.0, ..MoveIntent::default() };
        assert!(!player_moving(16.0, Some(&still)), "a settle drift of 4 units in 0.25 s");
        assert!(player_moving(16.0, Some(&walking)));
        assert!(player_moving(344.0, Some(&strafing)));
        assert!(!player_moving(5.0, Some(&walking)), "held against a wall");
        assert!(player_moving(16.0, None), "no controls: by speed alone, as before");
    }

    #[test]
    fn light_adds_point_lights_with_the_game_falloff_and_caps_at_100() {
        let lamp = ([0.0, 0.0, 0.0], 200.0, 1.0);
        assert!((light_at(None, &[], [0.0; 3], true) - 50.0).abs() < 1e-6);
        // Half the radius away: 1 − 0.25 of the lamp's strength.
        assert!((light_at(None, &[lamp], [100.0, 0.0, 0.0], true) - 100.0).abs() < 1e-6);
        let dim = ([0.0, 0.0, 0.0], 200.0, 0.2);
        assert!((light_at(None, &[dim], [100.0, 0.0, 0.0], true) - 65.0).abs() < 1e-4);
        assert!((light_at(None, &[dim], [300.0, 0.0, 0.0], true) - 50.0).abs() < 1e-6);
    }

    /// People keep main's eye height, standing or crouched, at any scale: their hit
    /// volume starts at their feet, so `eyes_of` gives feet + eye, as before #1537. Only a
    /// body drawn clear of its origin and ending below its eyes (a ceiling turret) has
    /// its eyes on it.
    #[test]
    fn eyes_of_keeps_people_at_their_eye_height_and_puts_a_turrets_on_its_body() {
        let main = |feet: [f32; 3], eye: f32| [feet[0], feet[1], feet[2] + eye];
        let mut phys = PhysState::default();
        let feet = [100.0, -50.0, 20.0];
        let crouch = EYE_HEIGHT * crate::combat_stance::CROUCH_EYE;
        // Standing (128 tall), crouched (80 tall, below the 112 eye height), crouched
        // with bounds a little off the ground (an animation's feet), and scaled up.
        for (id, lo, height, eye) in [(1, 0.0, 128.0, EYE_HEIGHT), (2, 0.0, 80.0, EYE_HEIGHT), (3, 0.0, 80.0, crouch), (4, 6.0, 74.0, EYE_HEIGHT), (5, 0.0, 154.0, EYE_HEIGHT * 1.2)] {
            phys.world.set_actor_volume(id, fnv_phys::HitVolume { feet: [feet[0] + 3.0, feet[1], feet[2] + lo], radius: 20.0, height, dead: false });
            assert_eq!(eyes_of(Some(&phys), id, feet, eye), main(feet, eye), "actor {id}");
        }
        // No volume, or no physics: as on main.
        assert_eq!(eyes_of(Some(&phys), 99, feet, EYE_HEIGHT), main(feet, EYE_HEIGHT));
        assert_eq!(eyes_of(None, 1, feet, EYE_HEIGHT), main(feet, EYE_HEIGHT));
        // The ceiling turret: body 47-93 over its origin [observed, #1537], eyes halfway up.
        phys.world.set_actor_volume(6, fnv_phys::HitVolume { feet: [0.0, 0.0, 7807.0], radius: 20.0, height: 46.0, dead: false });
        assert_eq!(eyes_of(Some(&phys), 6, [0.0, 0.0, 7760.0], EYE_HEIGHT), [0.0, 0.0, 7830.0]);
    }

    /// #3232: the bodies the player's camera sees, for player.GetLineOfSight on a dead
    /// actor. In front of the camera and clear: seen; behind it: not asked; in view but
    /// behind a wall (the line answered blocked): not seen; no camera: nothing.
    #[test]
    fn the_camera_sees_bodies_in_view_and_clear() {
        // Fred Wilks' body 193 units ahead of the camera, as in the report; one behind
        // the camera; one in view with a wall between.
        let eye = [1964.0, -1393.0, 570.0];
        let camera = Some((eye, [0.0, -1.0]));
        let fred = (0x060393EA, [1896.0, -1574.0, 454.0]);
        let behind = (2, [1964.0, -1200.0, 460.0]);
        let walled = (3, [2100.0, -1600.0, 460.0]);
        let mut asked = Vec::new();
        let seen = bodies_seen(camera, &[fred, behind, walled], |asks| {
            asked = asks.to_vec();
            asks.iter().map(|(_, [p, _])| *p != walled.1).collect()
        });
        assert_eq!(seen, vec![FormId(fred.0)]);
        // The line is cast from the camera to where the body lies, not to a standing
        // actor's chest; the body behind the camera is never asked.
        assert_eq!(asked, vec![(eye, [fred.1, fred.1]), (eye, [walled.1, walled.1])]);
        assert!(bodies_seen(None, &[fred], |_| panic!("no camera, no rays")).is_empty());
        // Where the physics knows nothing of the body: a little above its origin.
        assert_eq!(body_point(None, 7, [10.0, 20.0, 30.0]), [10.0, 20.0, 30.0 + BODY_LIFT]);
    }

    /// #2117: a shot is heard at the loudness of the weapon as fired, its mod flags on
    /// the message: a Normal weapon with a Silence mod in slot 3 is Normal without the
    /// mod and Silent with it, in the action sound and in the level its impacts get; a
    /// mod that is not a silencer changes nothing, a naturally silent weapon stays
    /// silent, and the loudest shot since the last measurement is kept.
    #[test]
    fn a_shot_is_heard_at_the_modded_weapons_level() {
        use fnv_combat::data::WeaponMod;
        let slot = |slot: u8, effect: &str| WeaponMod { slot, item: Some(FormId(0x100 + slot as u32)), effect: Some(effect.into()), value_a: 4.0, value_b: 0.0 };
        let pistol = WeaponStats { sound_level: Some("Normal".into()), mods: vec![slot(1, "Increase Clip Capacity"), slot(3, "Silence")], ..Default::default() };
        let quiet = WeaponStats { sound_level: Some("Silent".into()), mods: vec![slot(1, "Increase Clip Capacity")], ..Default::default() };
        let d = Settings::default();
        let (normal, silent) = (d.firing_sound("Normal"), d.firing_sound("Silent"));
        assert!(silent < normal);
        let shot = |shooter: u32, mods: u8| ShotFired { projectile: FormId(1), weapon: FormId(2), mods, shooter: FormId(shooter), at: None, dir: None };
        let heard = |w: &WeaponStats, mods: u8| {
            let mut state = DetectionState::default();
            hear_shot(&mut state, Some(w), &shot(7, mods));
            (state.shots[&7], state.shot_levels[&7])
        };
        assert_eq!(heard(&pistol, 0), (normal, normal), "no mods: the base level");
        assert_eq!(heard(&pistol, 4), (silent, silent), "the Silence slot's flag");
        assert_eq!(heard(&pistol, 5), (silent, silent), "the Silence slot among others");
        assert_eq!(heard(&pistol, 1), (normal, normal), "an unrelated mod");
        assert_eq!(heard(&quiet, 0), (silent, silent), "a naturally silent weapon");
        assert_eq!(heard(&quiet, 1), (silent, silent), "a naturally silent weapon, modded");
        // A weapon without data is not heard.
        let mut state = DetectionState::default();
        hear_shot(&mut state, None, &shot(8, 4));
        assert_eq!((state.shots[&8], state.shot_levels[&8]), (0.0, 0.0));
        // Loud then silenced: the action sound keeps the loudest, the impacts the last.
        let mut state = DetectionState::default();
        hear_shot(&mut state, Some(&pistol), &shot(9, 0));
        hear_shot(&mut state, Some(&pistol), &shot(9, 4));
        assert_eq!((state.shots[&9], state.shot_levels[&9]), (normal, silent));
    }
}
