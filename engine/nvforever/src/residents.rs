//! NPCs living by their AI packages (docs/design/actor-behaviour.md §5.4):
//! `--npc-packages ACTOR[,ACTOR…]` hands placed actors to `fnv_ai`, and
//! `--npc-packages all` every actor and creature of the cells loaded around the camera
//! and of the interiors their load doors lead to.
//! Each chooses its package by the hour (`fnv_game::ai`), walks to where the package
//! sends it over the nav graph (`nav.rs` builds it), through doors into other spaces,
//! and stays there until its package changes. They are drawn and animated like
//! `--npc-travel` travellers (`nav::show_actor`), and wait for doors that open in place.
//!
//! **Whose game state.** With `--play` the package choice is the game's own
//! (`nvf_play::script`): quests, script packages and deaths count, package scripts run
//! in the game, and actors stand still while a menu or a conversation holds the game.
//! Without `--play` it runs in a game state of its own, as authored (no quest started).
//!
//! **The clock** starts at the sky's hour. Without `--play` it runs at the sky's
//! `--time-scale`, or at the game's TimeScale (30, `--npc-timescale`) when the sky stands
//! still; unlike the sky's it does not stop indoors. With `--play` it is the game's own
//! calendar, which the simulation advances at the TimeScale global (`nvf_play::sim`);
//! this module only sets it to the sky's hour once, at the start.
//!
//! **Furniture** (`furniture.rs`). `fnv_ai` says what furniture an actor uses
//! (`Agent::using`): what its package gives it (`fnv_ai::goal::package_furniture`) or
//! what its sandbox chose (`fnv_ai::sandbox`: seats, beds at night, a seat for a meal).
//! The actor is told to sit ([`SitOn`]) once it is drawn; when it is to use something
//! else, or nothing, it is told to get up ([`GetUp`]) and its walk waits until it has.
//!
//! **Sandboxing** (`fnv_ai::sandbox`) is decided headless; here its walks are walked,
//! its seats sat on, and at an idle marker the actor stands facing the marker's way
//! (the marker's idles are not played yet).
//!
//! **Hours that pass in the Wait menu** (`nvf_play::wait`, docs/spec-notes/wait-menu.md
//! §7). The menu holds the game, so nobody walks while it is up; for each hour that
//! passes in it every actor chooses its package for the new hour (with the packages'
//! begin, end and change scripts) and walks as far as it would have in an hour of play,
//! headless on the navmesh ([`Core::pass_hours`]). When the menu closes the residents
//! are where their packages have them; one that was on furniture and is to be elsewhere
//! is off it at once. [engineering choice: what the original does is not sourced.]
//!
//! **A script moving the clock** (`set GameHour to 8`, more than an hour on the clock
//! face; issue #2353, docs/spec-notes/packages.md §9) is handled like the end of a wait:
//! everybody chooses its package at once, and the residents out of sight are put where
//! their new packages have them, as on arrival; those in view walk there
//! ([`Core::after_hour_jump`]) [design, #2353].
//!
//! **Movement.** With `--play`, an actor walking where the collision world is loaded
//! (around the player) is moved by the character controller ([`PhysMover`]): it steers
//! along its path and physics decides where it ends up, so it goes round furniture and
//! follows the floor. Stuck for a few seconds, or anywhere else (other cells, other
//! spaces, without `--play`), it moves on the navmesh surface (`fnv_nav::Walker`).
//!
//! **Load doors** (`door_fade.rs`): an actor that goes through a door into another space
//! fades out at it and in at the other side, and the doors' open sounds play where the
//! listener is drawn (owner's playtest list 5 item 3).
//!
//! **The start's loading screen** (`nvf_play::loading::StartCover`): the residents in view
//! are drawn behind it, where the AI will take them up, and the screen waits for them; their
//! first package is still chosen on the first frame of play ([`StartPlacing`]).

use crate::actors::ActorWorld;
use crate::furniture::{GetUp, LeaveNow, OnFurniture, SitOn, SitRefused};
use crate::locomotion::Locomotion;
use crate::nav::{self, Lead, NavState};
use crate::world::{CellKey, Streamer, WorldData, WorldRoot};
use bevy::prelude::*;
use fnv_actor::furniture_retry as retry;
use fnv_ai::{Activity, Ai, AiConfig, AiEvent, Doing, Mover};
use fnv_game::game::{Game, GameConfig};
use fnv_game::GameData;
use fnv_nav::follow::{arc_step, heading_delta, heading_of, turn_towards, Advice, Unstuck};
use fnv_nav::{NavGraph, Space, Steer, WalkEvent, Walker};
use nvf_play::blocking::ActorBlocks;
use nvf_play::phys::{NpcSpotRefused, PhysState};
use fnv_sim::{clock, FormId, FormStore, RefTable, SimConfig, World};
use nvf_assets::actor::ActorParts;
use nvf_assets::model::ModelCtx;
use nvf_play::RefVisual;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::f32::consts::FRAC_PI_4;

mod guard_wander;
mod knock_run;
pub(crate) mod melee_knockback;
mod sandbox_interrupt;

/// Where the function catalogue is when `NVF_PLAY_SCRIPT_FUNCTIONS` is not set (as
/// `nvf_play::script`).
const DEFAULT_FUNCTIONS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/script/functions.json");

/// The game's periodic package choice, seconds [documented: GECK wiki; packages.md §3].
const PACKAGE_INTERVAL: f64 = 10.0;

/// Actors drawn for the first time per frame at most (drawing one loads its parts).
const SPAWNS_PER_FRAME: usize = 3;

/// Under the start's loading screen the residents in view are drawn without
/// [`SPAWNS_PER_FRAME`], for this long each frame (at least one a frame), since no frame
/// is seen there ([`StartPlacing`]) [engineering choice: half of the 200 ms a frame cells
/// load for behind a loading screen, `world::LOADING_SCREEN_FRAME`; the screen's emblem
/// turns on real time, so a long frame only makes it jump].
const START_PLACE_BUDGET: std::time::Duration = std::time::Duration::from_millis(100);

/// Test aid: `0` draws the start's residents after its loading screen, at most
/// [`SPAWNS_PER_FRAME`] a frame, as before they were drawn under it ([`StartPlacing`]).
const START_PLACE_AID: &str = "NVF_START_ACTORS_UNDER_SCREEN";

/// Whether the start's residents are drawn under its screen, by [`START_PLACE_AID`]'s value.
fn start_actors_under_screen(aid: Option<&str>) -> bool {
    aid.is_none_or(|v| v.trim() != "0")
}

/// The start's residents drawn under its loading screen (`nvf_play::loading::StartCover`,
/// docs/design/loading-screen.md §1a). Until then nobody was drawn before the screen came
/// down: the AI waited for the game to go on, then the first frames drew the residents in
/// view [`SPAWNS_PER_FRAME`] at a time, a frame of 65-106 ms and about a second of 20-54 ms
/// frames after a Novac start's screen (against 6.5-7 ms after) [observed 2026-11-19, 0.1 s
/// frame windows, 0 spawns before the screen came down and 15-17 after, the driver's
/// shader cache on and off].
///
/// Only the bodies come earlier: the residents are not handed to the AI under the screen,
/// so they choose their first package on the first frame of play, after the scripts' first
/// frame (OnLoad, GameMode), as before. Each is drawn where `fnv_ai::Ai::add` will put its
/// walker then (where the game has it), facing the same way; nobody walks under the screen
/// anyway, its clock is paused [design]. Choosing packages under the screen would run their
/// begin scripts and OnPackageStart before the scripts' first frame, and judge their
/// conditions before it changed anything (`fnv_game::ai`, `Game::set_package`).
struct StartPlacing {
    /// Off with [`START_PLACE_AID`]` = 0`.
    on: bool,
    /// Frames under the start's screen with the game state there. Drawing begins on the
    /// second: the frame the scripts are made, the appearance changes of a loaded game may
    /// not have reached the actors' resolver yet (`appearance::show_changes` runs before
    /// [`drive`], but not after the scripts).
    game_frames: u32,
    /// Residents drawn under the screen so far.
    placed: usize,
    /// The first frame of drawing was logged.
    told: bool,
}

/// A resident not handed to the AI yet, under the start's screen: where its walker will
/// stand ([`Core::start_bodies`]) and whether the game has it in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StartBody {
    /// Its index in [`Core::list`].
    index: usize,
    space: Space,
    feet: [f32; 3],
    heading: f32,
    present: bool,
}

/// What drawing one resident under the start's screen came to ([`place_frame`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placed {
    Drawn,
    /// Its parts are not read yet: asked again next frame.
    NotYet,
    /// It cannot be drawn (`nav::show_actor` failed): not waited for.
    Never,
}

/// One frame of drawing the residents in view under the start's screen, `want` in the
/// residents' order: while `may`, each is drawn (`draw`) until the frame has spent
/// [`START_PLACE_BUDGET`] (`spent`), at least one a frame. Returns how many were drawn and
/// how many are still to draw.
fn place_frame<T: Copy>(want: &[T], may: bool, mut spent: impl FnMut() -> std::time::Duration, mut draw: impl FnMut(T) -> Placed) -> (usize, usize) {
    let (mut drawn, mut never) = (0, 0);
    if may {
        for (i, &w) in want.iter().enumerate() {
            if i > 0 && spent() >= START_PLACE_BUDGET {
                break;
            }
            match draw(w) {
                Placed::Drawn => drawn += 1,
                Placed::Never => never += 1,
                Placed::NotYet => {}
            }
        }
    }
    (drawn, want.len() - drawn - never)
}

/// After the character controller could not get an actor along its path, the actor
/// keeps to the navmesh surface for this long, seconds [our choice].
const PHYSICS_RETRY: f32 = 8.0;

/// An actor left to the navmesh walker for a while: until when (elapsed seconds), and
/// whether only because the walker took the last stretch of a walk (residents' `walk`,
/// [`LAST_STRETCH`]). That one does not hold an actor that is fighting now: a creature
/// handed to the walker for the last few units of a search, just before its fight began,
/// ran its whole charge on the walker, which meets nobody, and ended inside the other
/// creature it ran past (#3341: two radscorpions 47 units apart where their blocks keep
/// them 113 apart). The other skips (stuck, fallen out of the world, nowhere to move it
/// on to) hold a fighter too.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Skip {
    until: f32,
    last_stretch: bool,
}

impl Skip {
    fn retry(until: f32) -> Skip {
        Skip { until, last_stretch: false }
    }

    /// Whether the actor is left to the walker at `now`; `fighting`: it is in a fight.
    fn holds(&self, now: f32, fighting: bool) -> bool {
        now < self.until && !(fighting && self.last_stretch)
    }
}

/// A stuck actor is moved on this far along its path (game units) [our choice: more than
/// a brahmin's length, the longest of the common actors].
const SKIP_AHEAD: f32 = 160.0;

/// Height above the feet at which the way to a skip's spot is checked for keyframed
/// collision, game units [design: below a protectron's waist, above a step].
const BARRIER_PROBE_HEIGHT: f32 = 50.0;

/// How far on along its path a stuck actor may be put down, from the first choice to the
/// nearest place that still helps (0: back onto the path, for feet off it), the first
/// that has ground and room for it [design, docs/design/navmesh.md §4: the same length of
/// path each time, not a search].
const SKIP_LADDER: [f32; 5] = [SKIP_AHEAD, 120.0, 80.0, 40.0, 0.0];

/// After this many rounds in a row with nowhere to put a stuck actor, it is left to the
/// navmesh surface for [`PHYSICS_RETRY`] as one that cannot be moved on at all is
/// [design: as many as `Unstuck::max_replans` allows skips, about a second apart, so what
/// only stood in the way for a moment (the player, another actor walking past) is
/// waited out, and a place that never fits costs an actor about four more seconds after
/// the first refused round and no more].
const SKIP_REFUSED_ROUNDS: u32 = 5;

/// A refused round this long after the last one starts the count again, seconds [design:
/// the actor has been walking since]. Not a new path: Follow packages plan one every half
/// second, and five rounds would never add up.
const SKIP_REFUSED_FORGET: f32 = 8.0;

/// Stuck actors the places of [`SKIP_LADDER`] were all refused for.
#[derive(Default)]
struct SkipRefusals {
    /// Per actor: rounds in a row and when the last one was (elapsed seconds).
    rounds: HashMap<u32, (u32, f32)>,
    /// Rounds refused, in all.
    total: usize,
}

impl SkipRefusals {
    /// One more round for `id` at `now`: how many in a row.
    fn note(&mut self, id: u32, now: f32) -> u32 {
        self.total += 1;
        let r = self.rounds.entry(id).or_insert((0, now));
        if now - r.1 > SKIP_REFUSED_FORGET {
            r.0 = 0;
        }
        *r = (r.0 + 1, now);
        r.0
    }

    /// `id`'s blocked episode is over: moved on, or its body made again, moved or dropped.
    fn forget(&mut self, id: u32) {
        self.rounds.remove(&id);
    }
}

/// That actor `id` has a character, made at `feet` if it had none; false when none could
/// be. Every character the residents make is made here: a new body is a new blocked
/// episode, so what the actor's last one found stuck and refused goes with it (a stuck rule
/// starts afresh at its next walk). A body made to coast or be carried is walked on later
/// without passing through here again.
fn ensure_body(phys: &mut PhysState, unstuck: &mut HashMap<u32, Unstuck>, refusals: &mut SkipRefusals, id: u32, feet: [f32; 3]) -> bool {
    if phys.npcs.contains_key(&id) {
        return true;
    }
    if !phys.add_npc(id, feet) {
        return false;
    }
    unstuck.remove(&id);
    refusals.forget(id);
    true
}

/// The first place of [`SKIP_LADDER`] that actor `id` at `feet` can be put down at, and its
/// feet there; else how many were refused for having no navmesh surface, no collision
/// loaded, nothing to stand on and no room. Changes nothing.
fn skip_spot(phys: &mut PhysState, nav: &NavGraph, f: &fnv_nav::Follower, id: u32, space: Space, feet: [f32; 3]) -> Result<(fnv_nav::SkipCandidate, [f32; 3]), [usize; 4]> {
    let mut refused = [0; 4];
    let Some(leg) = f.path.legs.get(f.leg) else { return Err(refused) };
    // The leg is not in the space whose collision is loaded.
    if leg.space != space {
        refused[1] = 1;
        return Err(refused);
    }
    for c in f.skip_candidates(feet, &SKIP_LADDER) {
        let Some(surface) = nav.skip_surface(leg, &c) else {
            refused[0] += 1;
            continue;
        };
        match phys.npc_spot(id, surface) {
            Ok(spot) => return Ok((c, spot)),
            Err(NpcSpotRefused::NotLoaded) => refused[1] += 1,
            Err(NpcSpotRefused::NoSupport) => refused[2] += 1,
            Err(NpcSpotRefused::NoRoom) => refused[3] += 1,
        }
    }
    Err(refused)
}

/// The stuck rule for actors moved by physics: detours, then moved on along the path.
fn unstuck(speed: f32) -> Unstuck {
    let mut u = Unstuck::new(speed);
    u.skip = true;
    u
}

/// Actor `id`'s stuck rule, kept in step with the speed it moves at now (`speed`:
/// `Walker::moving_speed`, its package's or fight's speed times its crippled legs' share,
/// so the progress asked of it is what it can make; `Unstuck::set_speed`). `None` at a
/// speed of 0 (a Speed Mult of 0, issue #2118): the actor stands where it is and is never
/// stuck, and its rule forgets the second before, so the time it stood does not count when
/// its speed comes back. Before, the rule kept the speed its walk had when the rule was
/// made: an actor at Speed Mult 0, or slowed under a fifth of that, was moved on along its
/// path every second, and one whose rule was made at 0 was never found stuck.
fn stuck_rule(rules: &mut HashMap<u32, Unstuck>, id: u32, speed: f32) -> Option<&mut Unstuck> {
    let moving = speed.is_finite() && speed > 0.0;
    let rule = rules.entry(id).or_insert_with(|| unstuck(0.0));
    rule.set_speed(if moving { speed } else { 0.0 });
    moving.then_some(rule)
}

/// Horizontal speed over a ledge at the most, game units per second [design: the arc's
/// own speed lands it on the landing point (`fnv_nav::jump::Flight`); a short wide drop
/// asks for more than any run].
const HOP_SPEED_MAX: f32 = 400.0;

/// An actor walking at least this share slower than it asks for, standing on the ground,
/// is walking into something (an obstacle it may jump) [design].
const BLOCKED_SHARE: f32 = 0.3;

/// The probes ahead of an actor walking into something: how far ahead they reach beyond
/// its radius, and the heights of the low one (above the step the controller climbs) and
/// the high one (just above a jump's apex), game units [design, actor-jumps.md §4].
const OBSTACLE_REACH: f32 = 24.0;
const OBSTACLE_LOW: f32 = 28.0;
const OBSTACLE_HIGH: f32 = fnv_nav::jump::JUMP_HEIGHT + 8.0;

/// What an actor walking with physics does at a ledge link or an obstacle: its velocity
/// over the ledge, and whether it jumps now.
fn hop_or_obstacle(phys: &mut PhysState, id: u32, f: &fnv_nav::Follower, walker: &Walker, want: [f32; 2], stats: &mut [usize; 6], hopped: &mut HashMap<u32, (usize, usize)>) -> ([f32; 2], bool) {
    let Some(b) = phys.npcs.get(&id) else { return (want, false) };
    let (feet, grounded, radius) = (b.ch.feet, b.ch.grounded && !b.ch.swimming, b.ch.params.radius);
    if let Some((hop, landing)) = f.hop() {
        // Over a ledge: the arc's speed to the landing, jumping up a climb and stepping
        // off a drop (the controller carries the body through the air).
        let flight = fnv_nav::jump::Flight::over_ledge(feet, landing, hop.up);
        let d = [landing[0] - feet[0], landing[1] - feet[1]];
        let len = d[0].hypot(d[1]);
        let speed = flight.speed().max(walker.moving_speed()).min(HOP_SPEED_MAX);
        let v = if len > 1e-3 { [d[0] / len * speed, d[1] / len * speed] } else { want };
        // Up: jumps while still below the ledge (landed on top short of the landing
        // point, it walks on).
        let jump = hop.up && grounded && feet[2] < landing[2] - fnv_phys::CharacterParams::default().step_height;
        if grounded && hopped.insert(id, (f.leg, hop.point)) != Some((f.leg, hop.point)) {
            stats[if hop.up { 3 } else { 4 }] += 1;
            info!(
                "actor jumps: {id:08X} {} a ledge at ({:.0}, {:.0}, {:.0}) to ({:.0}, {:.0}, {:.0})",
                if hop.up { "jumps up" } else { "drops off" },
                feet[0],
                feet[1],
                feet[2],
                landing[0],
                landing[1],
                landing[2]
            );
        }
        return (v, jump);
    }
    // Walking into something low with nothing above it: jump it.
    let asked = want[0].hypot(want[1]);
    if !grounded || asked < 1.0 {
        return (want, false);
    }
    let step = phys.world.params.dt;
    let got = (feet[0] - b.prev_feet[0]).hypot(feet[1] - b.prev_feet[1]);
    if got >= BLOCKED_SHARE * asked * step {
        return (want, false);
    }
    let dir = [want[0] / asked, want[1] / asked, 0.0];
    let reach = radius + OBSTACLE_REACH;
    let mut filter = fnv_phys::Filter { exclude_layers: fnv_phys::layers::character_ignores().union(fnv_phys::layers::actor_volumes()), ..Default::default() };
    filter.exclude.extend(phys.npcs.values().map(|n| n.ch.key()));
    let probe = |phys: &mut PhysState, h: f32| phys.ray([feet[0], feet[1], feet[2] + h], dir, reach * 2.0, filter.clone()).map(|hit| hit.distance);
    let (low, high) = (probe(phys, OBSTACLE_LOW), probe(phys, OBSTACLE_HIGH));
    if fnv_nav::jump::obstacle_jumpable(low, high, reach) {
        stats[5] += 1;
        info!("actor jumps: {id:08X} jumps an obstacle {:.0} ahead at ({:.0}, {:.0}, {:.0})", low.unwrap_or(0.0), feet[0], feet[1], feet[2]);
        return (want, true);
    }
    (want, false)
}

/// The walker takes the last stretch of a walk, game units [our choice].
const LAST_STRETCH: f32 = 64.0;

/// A co-op guest's drawn copy of the host's actor a frame's move longer than its run
/// would cover, up to this far (units), is caught up over a few frames rather than put
/// on at once [design; observed: buckallobill, in game, FNV 1.4.0.525: a runner at
/// about 516 units/s was put 49-57 units on in one frame on a guest; a script's move
/// goes further].
const CATCH_UP_MAX: f32 = 128.0;
/// The run speed (units/s) taken for an actor with none known.
const CATCH_UP_SPEED: f32 = 500.0;
/// How much faster than its run a guest's copy catches up [design: a 50-unit lag at a
/// 516 units/s run is gone in three frames at 60 fps].
const CATCH_UP_RATE: f32 = 2.0;

/// Where a guest draws the host's actor this frame: from where it was drawn (`from`)
/// towards where the host has it (`to`), at most [`CATCH_UP_RATE`] times its run speed
/// (`run`, units/s) in `dt` seconds; at `to` at once when that is further than
/// [`CATCH_UP_MAX`] (a placement) or within a frame's catch-up.
fn catch_up(from: [f32; 3], to: [f32; 3], run: f32, dt: f32) -> [f32; 3] {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let far = (d[0] * d[0] + d[1] * d[1]).sqrt();
    let reach = CATCH_UP_RATE * run.max(1.0) * dt.max(0.0);
    if far > CATCH_UP_MAX || far <= reach {
        return to;
    }
    let k = reach / far;
    [from[0] + d[0] * k, from[1] + d[1] * k, from[2] + d[2] * k]
}

/// The length of one update of the walks of an hour passed in the Wait menu, seconds
/// [our choice].
const WAITED_STEP: f32 = 0.25;

/// A drawn actor turning faster than this between two frames is counted in the turn
/// summary (radians per second: one and a half times the walking turn rate).
const SNAP_TURN: f32 = 1.5 * fnv_nav::follow::WALK_TURN_RATE;

/// A standing actor that had no room to give way when it was pushed is not pushed again
/// for this long, seconds: it stands in the way like a seated one [our choice].
const NO_ROOM_SECONDS: f32 = 1.5;
/// A push that moved the actor less than this share of its length found no room.
const NO_ROOM_SHARE: f32 = 0.25;
/// A push is over (and logged) when the actor has not been pushed for this long, seconds.
const PUSH_OVER_SECONDS: f32 = 0.5;

/// A standing actor being pushed aside: where it stood, how far it has moved, how many
/// updates found no room, and when it was last pushed (elapsed seconds).
struct Pushed {
    from: [f32; 3],
    units: f32,
    no_room: usize,
    last: f32,
}

/// The character controller for actors walking near the player, and the doors.
struct PhysMover<'a> {
    doors: &'a mut nvf_play::doors::Doors,
    phys: Option<&'a mut PhysState>,
    unstuck: &'a mut HashMap<u32, Unstuck>,
    /// Actors left to the walker until this time, and why ([`Skip`]).
    skip_until: &'a mut HashMap<u32, Skip>,
    /// The ledge each actor last went over (its path's leg and the hop's take-off point):
    /// counted and logged once.
    hopped: &'a mut HashMap<u32, (usize, usize)>,
    /// The keyframed barrier (a force field, a gate) each actor stands held at: logged once.
    barred: &'a mut HashMap<u32, u32>,
    now: f32,
    /// Actors moved by physics in this update; times physics gave up; stuck actors moved
    /// on along their paths; jumps up a ledge, drops off one and jumps over an obstacle
    /// (docs/spec-notes/actor-jumps.md).
    stats: &'a mut [usize; 6],
    /// Actors on furniture, or getting off it: they do not walk yet.
    held: &'a HashSet<u32>,
    /// Stuck actors there was nowhere to move on to ([`skip_spot`]).
    refusals: &'a mut SkipRefusals,
    /// Actors walking on with their AI off ([`Mover::coast`]).
    coasting: &'a mut HashMap<u32, Coasting>,
    /// People in combat (`Core::aims`, from the last update): physics walks them to the
    /// end of their walks.
    fighting: &'a HashMap<FormId, [f32; 3]>,
    /// Actors searching for a foe they lost or noticed (`fnv_ai::alert`): not moved on
    /// past what blocks them.
    searching: &'a HashSet<u32>,
    /// Creatures in Combat (`fighting` has people only, `Core::aims`): physics walks them
    /// to the end of their walks too, and the walker's last stretch given before the fight
    /// no longer holds them ([`Skip`], #3341).
    creatures_fighting: &'a HashSet<u32>,
}

/// An actor walking on with its AI off (SetActorsAI 0): where it was when the AI went
/// off, and whether it was carried on in this update (for the log).
struct Coasting {
    from: [f32; 3],
    heading: f32,
    speed: f32,
    now: bool,
}

/// Whether `walker` is in the space whose collision `phys` has loaded.
fn in_loaded_space(walker: &Walker, phys: &PhysState) -> bool {
    match (walker.space, phys.space) {
        (Space::Interior(c), nvf_play::phys::cells::Space::Interior(i)) => c == i,
        (Space::World(w), nvf_play::phys::cells::Space::Exterior) => w == phys.worldspace,
        _ => false,
    }
}

/// A door known here (drawn, with its animation) is opened and waited for.
fn open_door(doors: &mut nvf_play::doors::Doors, door: u32) -> bool {
    if doors.get(door).is_none() {
        return true;
    }
    doors.set_open(door, true);
    doors.is_open(door) == Some(true)
}

/// A held actor's physics body (if it has one) stands still; the speed it had.
fn stop_body(phys: Option<&mut PhysState>, id: u32) -> f32 {
    phys.and_then(|p| p.npcs.get_mut(&id)).map_or(0.0, |b| {
        let was = b.velocity[0].hypot(b.velocity[1]);
        b.velocity = [0.0, 0.0];
        was
    })
}

impl Mover for PhysMover<'_> {
    fn open(&mut self, _actor: FormId, door: u32) -> bool {
        open_door(self.doors, door)
    }

    fn walk(&mut self, actor: FormId, walker: &mut Walker, nav: &NavGraph, dt: f32) -> Option<WalkEvent> {
        let id = actor.0;
        if self.held.contains(&id) {
            // Its body stops too: the character keeps the last velocity it was given, and
            // an actor staggering mid-walk slid on at its walking speed [observed: Cook-Cook
            // in a smoke run, 2026-10-25].
            let was = stop_body(self.phys.as_deref_mut(), id);
            if was > 0.0 {
                info!("actor {id:08X} held (staggering, seated or talking): its character stops (was walking at {was:.0} units/s)");
            }
            walker.moving = false;
            return Some(WalkEvent::Moved);
        }
        let phys = self.phys.as_deref_mut()?;
        let here = in_loaded_space(walker, phys);
        let fighting = self.fighting.contains_key(&actor) || self.creatures_fighting.contains(&id);
        if let Some(s) = self.skip_until.get(&id).copied().filter(|s| self.now < s.until && fighting && s.last_stretch) {
            info!("actor {id:08X} fights: back to the character controller from the walker's last stretch ({:.1} s of it left)", s.until - self.now);
            self.skip_until.remove(&id);
        }
        let skipped = self.skip_until.get(&id).is_some_and(|s| s.holds(self.now, fighting));
        if !here || skipped || walker.follower.is_none() || !phys.ready_at(walker.feet) {
            phys.remove_npc(id);
            return None;
        }
        if !ensure_body(phys, self.unstuck, self.refusals, id, walker.feet) {
            return None;
        }
        let feet = phys.npcs[&id].ch.feet;
        // A goal's height is judged on the ground only: in the air or in the water every
        // frame below goes as if it fitted, so a body that never lands is not held up by
        // it (docs/design/navmesh.md §4.1).
        let judged = phys.npcs[&id].ch.grounded && !phys.npcs[&id].ch.swimming;
        let airborne = !phys.npcs[&id].ch.grounded;
        let mut f = walker.follower.take()?;
        // Back to the walker: where physics left the actor, on the same path.
        let give_up = |phys: &mut PhysState, walker: &mut Walker, f| {
            phys.remove_npc(id);
            walker.follower = Some(f);
            walker.tri = fnv_nav::graph::NONE;
        };
        // Fallen out of the world, or far off the path's height.
        if (feet[2] - walker.feet[2]).abs() > 400.0 {
            self.skip_until.insert(id, Skip::retry(self.now + PHYSICS_RETRY));
            self.stats[1] += 1;
            give_up(phys, walker, f);
            return None;
        }
        let (event, velocity) = match f.steer(feet) {
            // Not at the goal's height: a new path from here, if it has one left.
            Steer::OffPath if let Some(new) = judged.then(|| walker.off_path(nav, feet, &f)).flatten() => {
                f = new;
                (WalkEvent::Moved, [0.0, 0.0])
            }
            Steer::Arrived | Steer::OffPath => {
                phys.remove_npc(id);
                walker.travelled += (feet[0] - walker.feet[0]).hypot(feet[1] - walker.feet[1]);
                walker.feet = feet;
                walker.turning = false;
                walker.moving = false;
                return Some(WalkEvent::Arrived);
            }
            Steer::OpenDoor { door, .. } => {
                if open_door(self.doors, door) {
                    f.door_opened();
                    (WalkEvent::Moved, [0.0, 0.0])
                } else {
                    (WalkEvent::WaitingForDoor(door), [0.0, 0.0])
                }
            }
            // At a door into another space: it stops and turns to the door first.
            Steer::Door(d) if walker.face_door(&d, feet, dt) => (WalkEvent::Turned, [0.0, 0.0]),
            Steer::Door(d) => {
                // Through a door into another space: the walker's from here (the
                // collision there is not the one loaded).
                match d.door_at {
                    Some(at) => info!(
                        "actor {id:08X} goes through load door {:08X} facing it ({:.0} degrees off)",
                        d.door,
                        heading_delta(walker.heading, heading_of([at[0] - feet[0], at[1] - feet[1]])).abs().to_degrees()
                    ),
                    None => info!("actor {id:08X} goes through load door {:08X} (where the door stands is not known)", d.door),
                }
                f.door_used();
                walker.space = f.space().unwrap_or(walker.space);
                walker.feet = d.arrival;
                walker.heading = d.arrival_heading;
                walker.tri = d.to;
                walker.follower = Some(f);
                phys.remove_npc(id);
                return Some(WalkEvent::UsedDoor(d));
            }
            // At a speed of 0 (a Speed Mult of 0, issue #2118) it stands where it is, its
            // walk and its body kept, as the walker does on the navmesh (no step, no turn
            // on the spot): not counted as stuck and not moved on along its path
            // (`stuck_rule`), nor handed to the walker for its last steps. Already in the
            // air over a ledge, the flight lands it first (`hop_or_obstacle`, which only
            // takes off from the ground).
            Steer::Walk { .. } if walker.moving_speed().is_nan() || walker.moving_speed() <= 0.0 => {
                if self.unstuck.get(&id).is_none_or(|u| u.speed > 0.0) {
                    info!("actor {id:08X} stands on its walk at ({:.0}, {:.0}, {:.0}): its speed is 0, not stuck", feet[0], feet[1], feet[2]);
                }
                let _ = stuck_rule(self.unstuck, id, 0.0);
                walker.turning = false;
                let v = if airborne && f.hop().is_some() { hop_or_obstacle(phys, id, &f, walker, [0.0, 0.0], self.stats, self.hopped).0 } else { [0.0, 0.0] };
                (if v == [0.0, 0.0] { WalkEvent::Idle } else { WalkEvent::Moved }, v)
            }
            // The last steps to the place itself are the walker's: packages send actors
            // to chairs, beds and markers, which the character cannot stand in. Not a
            // fighter's: its posts (cover, a peek point, a flank) are places to stand, and
            // the walker, which keeps to the navmesh and not the collision, left it half
            // inside walls by its cover (owner's screenshot 2026-10-23) [design]. Nor a
            // fighting creature's: the walker meets nobody, and a radscorpion running at the
            // player on it ran into the other one it passed (#3341).
            // On a mismatch the walker would end the walk on the navmesh under the feet, or
            // where there is none lift it towards the goal's height: a new path from here
            // first, as above.
            Steer::Walk { remaining, .. } if remaining < LAST_STRETCH && f.leg + 1 == f.path.legs.len() && !self.fighting.contains_key(&actor) && !self.creatures_fighting.contains(&id) => {
                match (judged && !f.fits_goal(feet)).then(|| walker.off_path(nav, feet, &f)).flatten() {
                    Some(new) => {
                        f = new;
                        (WalkEvent::Moved, [0.0, 0.0])
                    }
                    None => {
                        self.skip_until.insert(id, Skip { until: self.now + 4.0, last_stretch: true });
                        walker.feet = feet;
                        give_up(phys, walker, f);
                        return None;
                    }
                }
            }
            // Standing and facing away from the way to go: it turns on the spot first, as
            // the walker does (not counted as being stuck).
            Steer::Walk { dir, .. }
                if dir != [0.0, 0.0] && (walker.turning || (!walker.moving && heading_delta(walker.heading, heading_of(dir)).abs() > walker.spot_turn_start)) =>
            {
                walker.turning = !walker.face(heading_of(dir), dt);
                (WalkEvent::Turned, [0.0, 0.0])
            }
            Steer::Walk { dir, remaining, .. } => match stuck_rule(self.unstuck, id, walker.moving_speed()).map_or(Advice::Head(dir), |u| u.advise_moved(feet, dir, dt)) {
                Advice::Head(h) => {
                    // The heading turns at the walking rate and the actor walks along it
                    // (an arc round a corner), slowing while it is off the way to go
                    // (fnv_nav::follow::arc_step).
                    walker.heading = turn_towards(walker.heading, heading_of(h), walker.turn_rate * dt);
                    let (along, slow) = arc_step(walker.heading, h);
                    let speed = walker.moving_speed().min(remaining / dt.max(1e-3)) * slow;
                    let want = [along[0] * speed, along[1] * speed];
                    // Over a ledge link, or jumping what it walks into.
                    let (v, jump) = hop_or_obstacle(phys, id, &f, walker, want, self.stats, self.hopped);
                    if let Some(b) = phys.npcs.get_mut(&id) {
                        b.jump |= jump;
                    }
                    (WalkEvent::Moved, v)
                }
                // Stuck where detours did not help: moved on along the path, past what
                // blocks it, to the first place with ground and room for it.
                // Stuck at keyframed collision (a force field, a gate: what an animation or
                // a script moves away), or stuck while searching (a place to look, not one
                // it must reach): it stays where it is, pressing on, and is not moved past
                // what blocks it or left to the walker, which would carry it through.
                // [observed: HELIOS One's stasis protectrons stay in their pods behind the
                // field VHELIOSStasisField*REF, one moving 12 units in 30 s while it
                // fought, #1977 probe 3; the search: design]
                Advice::Skip
                    if let Some(gate) = f
                        .skip_candidate(feet, SKIP_AHEAD)
                        .and_then(|c| phys.keyframed_between(feet, c.pos, BARRIER_PROBE_HEIGHT))
                        .or_else(|| self.searching.contains(&id).then_some(0)) =>
                {
                    if self.barred.insert(id, gate) != Some(gate) {
                        if gate == 0 {
                            info!("actor {id:08X} stuck at ({:.0}, {:.0}, {:.0}) while it searches: held, not moved past what blocks it", feet[0], feet[1], feet[2]);
                        } else {
                            info!("actor {id:08X} stuck at ({:.0}, {:.0}, {:.0}): held at {gate:08X}'s moving collision, not moved past it", feet[0], feet[1], feet[2]);
                        }
                    }
                    if let Some(u) = self.unstuck.get_mut(&id) {
                        u.skip_refused();
                    }
                    (WalkEvent::Moved, [0.0, 0.0])
                }
                Advice::Skip if f.skip_candidate(feet, SKIP_AHEAD).is_some() => match skip_spot(phys, nav, &f, id, walker.space, feet) {
                    Ok((c, to)) => {
                        info!(
                            "actor {id:08X} stuck at ({:.0}, {:.0}, {:.0}): moved on along its path to ({:.0}, {:.0}, {:.0})",
                            feet[0], feet[1], feet[2], to[0], to[1], to[2]
                        );
                        self.stats[2] += 1;
                        self.refusals.forget(id);
                        f.commit_skip(&c);
                        let dir = [to[0] - feet[0], to[1] - feet[1]];
                        if dir != [0.0, 0.0] {
                            walker.heading = heading_of(dir);
                        }
                        walker.travelled += dir[0].hypot(dir[1]);
                        walker.feet = to;
                        walker.turning = false;
                        walker.tri = fnv_nav::graph::NONE;
                        walker.follower = Some(f);
                        phys.place_npc(id, to);
                        // Stuck again where it was put down, it is moved on again without
                        // the detours that failed already (`Unstuck::after_skip`).
                        let mut u = unstuck(walker.moving_speed());
                        u.after_skip();
                        self.unstuck.insert(id, u);
                        return Some(WalkEvent::Moved);
                    }
                    // Nowhere to put it: it stays as it is, body, feet and place on the
                    // path, and tries again after a second, for a while.
                    Err(why) => {
                        let rounds = self.refusals.note(id, self.now);
                        if rounds == 1 {
                            info!(
                                "actor {id:08X} stuck at ({:.0}, {:.0}, {:.0}): nowhere to move it on to ({} places with no navmesh, {} not loaded, {} with nothing to stand on, {} with no room)",
                                feet[0], feet[1], feet[2], why[0], why[1], why[2], why[3]
                            );
                        }
                        if rounds >= SKIP_REFUSED_ROUNDS {
                            info!("actor {id:08X} still has nowhere to be moved on to after {rounds} tries: on the navmesh surface for a while");
                            self.refusals.forget(id);
                            self.skip_until.insert(id, Skip::retry(self.now + PHYSICS_RETRY));
                            self.unstuck.remove(&id);
                            self.stats[1] += 1;
                            walker.feet = feet;
                            give_up(phys, walker, f);
                            return None;
                        }
                        if let Some(u) = self.unstuck.get_mut(&id) {
                            u.skip_refused();
                        }
                        (WalkEvent::Moved, [0.0, 0.0])
                    }
                },
                // Stuck: the navmesh surface from here, for a while.
                _ => {
                    self.skip_until.insert(id, Skip::retry(self.now + PHYSICS_RETRY));
                    self.unstuck.remove(&id);
                    self.stats[1] += 1;
                    walker.feet = feet;
                    give_up(phys, walker, f);
                    return None;
                }
            },
        };
        walker.travelled += (feet[0] - walker.feet[0]).hypot(feet[1] - walker.feet[1]);
        walker.feet = feet;
        walker.tri = fnv_nav::graph::NONE;
        walker.follower = Some(f);
        walker.moving = event == WalkEvent::Moved && velocity != [0.0, 0.0];
        if let Some(b) = phys.npcs.get_mut(&id) {
            b.velocity = velocity;
        }
        self.stats[0] += 1;
        Some(event)
    }

    /// SetActorsAI 0 while walking: the character controller carries the actor on along
    /// its heading at its walking speed, so it collides with walls, actors and the player
    /// and follows the ground as any walk near the player does (docs/spec-notes/
    /// set-actors-ai.md). It neither steers round nor jumps what it walks into: against
    /// a wall it keeps walking into it and goes nowhere [hypothesis].
    fn coast(&mut self, actor: FormId, walker: &mut Walker, _dt: f32) -> bool {
        let id = actor.0;
        if self.held.contains(&id) {
            let _ = stop_body(self.phys.as_deref_mut(), id);
            return false;
        }
        let Some(phys) = self.phys.as_deref_mut() else { return false };
        if !in_loaded_space(walker, phys) || !phys.ready_at(walker.feet) {
            phys.remove_npc(id);
            return false;
        }
        if !ensure_body(phys, self.unstuck, self.refusals, id, walker.feet) {
            return false;
        }
        let feet = phys.npcs[&id].ch.feet;
        // Fallen out of the world.
        if (feet[2] - walker.feet[2]).abs() > 400.0 {
            phys.remove_npc(id);
            return false;
        }
        let speed = walker.moving_speed();
        let d = fnv_nav::follow::dir_of(walker.heading);
        if let Some(b) = phys.npcs.get_mut(&id) {
            b.velocity = [d[0] * speed, d[1] * speed];
        }
        walker.travelled += (feet[0] - walker.feet[0]).hypot(feet[1] - walker.feet[1]);
        walker.feet = feet;
        walker.tri = fnv_nav::graph::NONE;
        walker.turning = false;
        let c = self.coasting.entry(id).or_insert_with(|| {
            info!(
                "actor {id:08X} AI off: walks on at heading {:.0} deg, {speed:.0} units/s, from ({:.0}, {:.0}, {:.0})",
                walker.heading.to_degrees().rem_euclid(360.0),
                feet[0],
                feet[1],
                feet[2]
            );
            Coasting { from: feet, heading: walker.heading, speed, now: false }
        });
        c.now = true;
        true
    }
}

/// Where the residents' clock starts and how fast it runs. A game restored from a save
/// keeps its own hour and day; after the start, everyone reads the game's calendar.
#[derive(Clone, Copy)]
pub struct Clock {
    pub hour: f32,
    pub days_passed: f32,
    /// Game hours per second.
    pub rate: f64,
}

/// A resident's frames drawn standing while on its way, for the walk summary. A stop is
/// standing between two frames drawn walking; the frames before a walk's first step are
/// not one: an actor turning on the spot to set off, or near the player waiting for its
/// new character's first physics step (it is drawn where the character was put down
/// until then, `PhysState::npc_drawn_feet`), stands at the start of its walk without
/// having walked yet.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct StopRun {
    /// Frames in a row drawn standing.
    standing: usize,
    /// Drawn walking since its walk began.
    walked: bool,
}

/// What a frame drawn walking ended ([`StopRun::frame`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum RunEnd {
    /// A stop of this many frames.
    Stop(usize),
    /// The standing before the walk's first step.
    SetOff,
}

impl StopRun {
    /// One frame drawn on its way, walking (`moving`) or standing.
    fn frame(&mut self, moving: bool) -> Option<RunEnd> {
        if !moving {
            self.standing += 1;
            return None;
        }
        let end = match (self.standing, self.walked) {
            (0, _) => None,
            (n, true) => Some(RunEnd::Stop(n)),
            (_, false) => Some(RunEnd::SetOff),
        };
        *self = StopRun { standing: 0, walked: true };
        end
    }
}

/// A resident and the furniture its package gives it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Seat {
    #[default]
    None,
    /// To use this furniture reference once the actor is drawn; `true` when it was on
    /// it already when its visual went away (it is put back on at once).
    Wanted(u32, bool),
    /// Told to; `true` once it was seen on (or walking up to) the furniture.
    Ordered(u32, bool),
    /// Told to get up; the walk waits.
    GettingUp(u32),
    /// Refused because it is occupied: asks again after a wait, these seconds so far
    /// ([`retry::waiting`]). It stands free meanwhile: no hold on its movement.
    Waiting(u32, f32),
    /// Refused for a reason waiting does not change: no order until something has changed
    /// ([`retry::unavailable`]). It stands free too.
    Unavailable(u32),
}

impl Seat {
    /// The furniture this is about.
    fn furniture(self) -> Option<u32> {
        match self {
            Seat::None => None,
            Seat::Wanted(f, _) | Seat::Ordered(f, _) | Seat::GettingUp(f) | Seat::Waiting(f, _) | Seat::Unavailable(f) => Some(f),
        }
    }

    /// The seat after a decision about a refused request.
    fn after(decision: retry::Decision) -> Seat {
        match decision {
            retry::Decision::Wait { target, waited } => Seat::Waiting(target, waited),
            retry::Decision::Unavailable { target } => Seat::Unavailable(target),
            // Through `Wanted`, which gives the order when it is drawn and asks nothing else
            // (a wait that is over gives its order itself: `drive`). Not at once: it never
            // got on.
            retry::Decision::Reissue { target } => Seat::Wanted(target, false),
            retry::Decision::Cancel => Seat::None,
        }
    }
}

/// What [`next_seat`] has the furniture do with a drawn resident.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SeatOrder {
    /// Sit on this furniture (`SitOn`); `true`: put on at once.
    Sit(u32, bool),
    /// Get up by the furniture's exit clip (`GetUp`); the walk waits.
    GetUp,
    /// It never got on: the order to sit is taken back.
    Cancel,
    /// Off the furniture: its walk goes on from where the furniture left it.
    Stood,
}

/// One frame of a resident's seat: the next state and what to tell the furniture.
/// `drawn`: its body is there to be told; `on`: it is on furniture (`OnFurniture`);
/// `restart`: ResetAI took its package up afresh since the last frame (`fnv_ai::Agent::
/// restarts`). Then an actor sitting where its package wants it gets up even when the
/// package gives it the same furniture again, and is told to sit again only once it is
/// off: the AI only says which furniture it is to use, so without this the reset showed
/// nothing [design, issue #2119; the original's seated RingoRef read GET_UP one loop
/// after ResetAI, observed: owner-decision-captures.md od_06].
fn next_seat(seat: Seat, drawn: bool, on: bool, frame: &retry::Now, restart: bool) -> (Seat, Option<SeatOrder>) {
    let wants = frame.wants;
    match (seat, drawn) {
        (Seat::Ordered(f, _), true) if restart && wants == Some(f) => {
            if on {
                (Seat::GettingUp(f), Some(SeatOrder::GetUp))
            } else {
                // Not on yet: told again from the start, through `Wanted`.
                (Seat::None, Some(SeatOrder::Cancel))
            }
        }
        (Seat::None, _) => (wants.map_or(Seat::None, |f| Seat::Wanted(f, false)), None),
        // The order goes out in the frame that found it eligible, not through `Wanted`:
        // by the next frame its AI may be off, or it may be held or out cold.
        (Seat::Waiting(f, so_far), _) => match (retry::waiting(f, so_far, frame), drawn) {
            (retry::Decision::Reissue { target }, true) => (Seat::Ordered(target, false), Some(SeatOrder::Sit(target, false))),
            (decision, _) => (Seat::after(decision), None),
        },
        (Seat::Unavailable(f), _) => (Seat::after(retry::unavailable(f, frame)), None),
        (Seat::Wanted(f, at_once), true) if wants == Some(f) => (Seat::Ordered(f, false), Some(SeatOrder::Sit(f, at_once))),
        (Seat::Wanted(f, _), _) if wants != Some(f) => (Seat::None, None),
        // Its visual went away (its cell unloaded): sit again when it is back, at once
        // when it was on the furniture (or walking up to it) then.
        (Seat::Ordered(f, was_on), false) if wants == Some(f) => (Seat::Wanted(f, was_on), None),
        (Seat::Ordered(f, _), true) if wants != Some(f) => {
            if on {
                (Seat::GettingUp(f), Some(SeatOrder::GetUp))
            } else {
                // Never got on: nothing to undo.
                (Seat::None, Some(SeatOrder::Cancel))
            }
        }
        (Seat::Ordered(f, _), _) if on => (Seat::Ordered(f, true), None),
        (Seat::Ordered(..) | Seat::GettingUp(_), false) => (Seat::None, None),
        (Seat::GettingUp(_), true) if !on => (Seat::None, Some(SeatOrder::Stood)),
        (s, _) => (s, None),
    }
}

/// The seat of a resident put at its package's place after a script moved the clock
/// ([`Core::after_hour_jump`]): the furniture it is to use now (`None` at a standing
/// place) and its seat. On new furniture it is sat on at once (no walk up, no sitting-down
/// clip); otherwise the old seat stays for [`stale_seat`] to drop in the same frame
/// (`Resident::jumped`), which also has a body still on the old furniture let go of it.
fn seat_after_jump(seat: Seat, furniture: Option<u32>) -> (Option<u32>, Seat) {
    match furniture {
        Some(f) => (Some(f), Seat::Wanted(f, true)),
        None => (None, seat),
    }
}

/// After hours passed in the Wait menu, a script's MoveTo or a jump of the clock
/// (`changed`): the furniture of `seat` the resident is to be off at once, when its AI
/// no longer has it use that one (`wants`).
fn stale_seat(seat: Seat, wants: Option<u32>, changed: bool) -> Option<u32> {
    seat.furniture().filter(|f| changed && wants != Some(*f))
}

struct Resident {
    actor: FormId,
    base: u32,
    creature: bool,
    /// The reference's scale (XSCL).
    scale: f32,
    label: String,
    /// Asked for by name (the log and the summary tell them apart from `all`'s).
    named: bool,
    /// Handed to the AI (once the game state is there).
    joined: bool,
    spawned: bool,
    /// Created by a script and not drawn yet: drawn in the first frame it is in view,
    /// past the read-ahead and the frame's spawn budget (`nav::Who::at_once`).
    at_once: bool,
    root: Option<Entity>,
    failed: bool,
    /// In the world and enabled, as the game has it.
    present: bool,
    /// The door it is waiting at and since when (elapsed seconds).
    waiting: Option<(u32, f32)>,
    doors_opened: usize,
    last_feet: [f32; 3],
    /// Where its body was drawn last frame (`npc_drawn_feet`).
    drawn_feet: [f32; 3],
    /// Frames in a row drawn standing while on its way (the walk summary).
    standing_frames: StopRun,
    seat: Seat,
    /// The furniture `fnv_ai` has the actor use now.
    wants: Option<u32>,
    /// ResetAI restarts of its package seen so far (`fnv_ai::Agent::restarts`).
    restarts: u32,
    /// ResetAI took its package up afresh since the seat was last driven: off its
    /// furniture even when the package gives it the same one again ([`next_seat`]).
    restart: bool,
    /// Waiting after a refusal, with nothing to hold it from being told to sit again: its
    /// AI runs, it is awake and alive, and no conversation, stagger or idle holds it.
    may_ask_again: bool,
    /// Standing where its package has it, with nothing else to do: its package's idles
    /// may play (`idles.rs`). Inside: the base of the idle marker it stands at.
    stand: Option<Option<FormId>>,
    /// The heading it was drawn with last frame (drawn, not on furniture), and where.
    last_drawn: Option<(f32, [f32; 3])>,
    /// A script put it somewhere (MoveTo) this frame: off its furniture at once.
    moved: bool,
    /// Put at its package's place this frame after a script moved the clock
    /// ([`Core::after_hour_jump`]): off its furniture at once, as `moved`.
    jumped: bool,
    /// Whether it cannot walk ([`cannot_walk`]), once looked up.
    rooted: Option<bool>,
    /// It cannot walk and its base has the Flies flag: a perched bird, in nobody's way
    /// ([`flies`]).
    perched: bool,
    /// Its base is flagged Immobile ([`ACBS_IMMOBILE`]): with a Turning Speed of 0 it
    /// never turns either ([`never_turns`]).
    immobile: bool,
    /// The space its walker was in at the last frame (the space of a door it has just
    /// gone through).
    last_space: Option<Space>,
    /// The heading it is drawn with while it fights, facing its target (`Core::aims`),
    /// until it is back on the walker's.
    aim_heading: Option<f32>,
    /// The target it was held on last frame ([`LOCK_HOLD`]): it stays on that target
    /// however far it moves in a frame.
    lock_held: Option<u32>,
    /// The heading it is drawn with while an idle holds it in place (`Core::posed`): the
    /// one it had when the idle began.
    posed_heading: Option<f32>,
    /// It has been drawn in the loaded area. Going from there through a door into an
    /// interior takes the interior into the cell buffer (`carries_cell`); an actor a
    /// script keeps in a holding cell (Malcolm Holmes in RandomEncounterNPCs) never was,
    /// so its cell is not buffered and it stays at the Low level.
    been_loaded: bool,
    /// Seen joined but not present (disabled): drawn again, it was enabled.
    absent: bool,
    /// The clips it plays in its first second drawn after a script made it, enabled it or
    /// put it somewhere ([`FirstSteps`]).
    first_steps: Option<FirstSteps>,
}

/// How long [`FirstSteps`] watches an actor's clips, seconds of it being drawn.
const FIRST_STEPS_SECONDS: f32 = 1.0;

/// The walk and run frames of an actor in its first [`FIRST_STEPS_SECONDS`] drawn after
/// a script created it (PlaceAtMe), enabled it or put it somewhere (MoveTo,
/// MoveToMarkerWithFade): it fades in meanwhile, and its package's gait holds from its
/// first frame (#1473). Logged at the end when it moved: `ai [hh:mm] <label>: first
/// second drawn after <why>: <n> walk frames, <n> run frames (walker at <s> units/s, drawn
/// up to <s>)`.
struct FirstSteps {
    why: &'static str,
    left: f32,
    walk: u32,
    run: u32,
    speed: f32,
    /// The fastest its body was drawn moving (`Locomotion::velocity`), units/s.
    drawn: f32,
}

impl FirstSteps {
    fn new(why: &'static str) -> FirstSteps {
        FirstSteps { why, left: FIRST_STEPS_SECONDS, walk: 0, run: 0, speed: 0.0, drawn: 0.0 }
    }
}

/// A fighter locked on its target (`npc_combat::Fighters::lock_on`) and turned within
/// this of it (radians, 10°) follows it exactly: the original's gunmen stayed within about
/// 1.5° of their target while they fought [observed 2026-10-25, observer campaign
/// fiend_facing]; a bigger turn (a new target) goes at the turn rate [design]. Once held,
/// it stays on that target even when the target sweeps past this in one frame (a melee
/// Fiend circling a trooper 70 units away, a long frame): smoke battle-ncr-fiends saw up
/// to 21 frames past 10° while such fighters turned back at the turn rate (#1162).
const LOCK_HOLD: f32 = 10.0 * std::f32::consts::PI / 180.0;

/// ACBS flag bits 5, "Flies", and 23, "Immobile" [documented: xEdit wbDefinitionsFNV.pas
/// CREA ACBS, schema `gameplay.json`].
const ACBS_FLIES: u32 = 1 << 5;
const ACBS_IMMOBILE: u32 = 1 << 23;

/// Whether the base form's ACBS flags have `bit`.
fn acbs_flag(world: &WorldData, base: u32, bit: u32) -> bool {
    let Some((plugin, r)) = fnv_world::fields::winner(world.lo, base) else { return false };
    let Ok(f) = fnv_world::fields::Fields::new(r, plugin, world.schema) else { return false };
    &f.sig == b"CREA" && f.raw(b"ACBS").and_then(|s| s.data.get(..4)).is_some_and(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) & bit != 0)
}

/// An Immobile creature whose Turning Speed (CREA TNAM, `Game::creature_turning_speed`)
/// is 0 never turns: a screen, not a turret. In FalloutNV.esm the Immobile creatures
/// with 0 are MrHouse, MrHouseOFFLINE, YesManMainframe (the penthouse screens) and
/// CRVoid; the 46 others, turrets and spore plants among them, have 30 to 300
/// [observed: the master's CREA ACBS and TNAM; hypothesis: that the original reads the
/// two so, it fits the owner's sight of a screen that stays still, issues #1543 and
/// #2150]. A creature that walks keeps its own rule for a TNAM of 0
/// (`creature_pace::creature_turn_rate`).
fn never_turns(immobile: bool, turning_speed: Option<f32>) -> bool {
    immobile && turning_speed == Some(0.0)
}

/// Whether the base form's ACBS flags have "Flies".
fn flies(world: &WorldData, base: u32) -> bool {
    acbs_flag(world, base, ACBS_FLIES)
}

/// A creature whose skeleton's folder has no movement animation (no `Forward` or
/// `FastForward` clip of its stance) cannot walk: the AI keeps it where it is
/// (`fnv_ai::Ai::root`). The game moves actors at their movement clips' speed
/// [hypothesis: the clips' accumulation roots carry the movement, actors.md §8], so
/// without one an actor has no way to move; the ravens outside Doc Mitchell's house
/// (NVCrRaven, `creatures\nvraven`: an idle and its fly-away idle only) sandboxed about
/// the editor location, sliding along the ground (owner's playtest, 2026-10-11).
///
/// The skeleton is the one the creature is drawn with: its Model/Animation source, a
/// template's pick when that flag is set ([`ActorWorld::resolve_creature`]). Its own model
/// may be only the editor's placeholder (`marker_creature.nif`): the Young Gecko Hunter
/// VSpawnSpecialTier1GeckoSmallRoachHunter (0x00168D0B) takes its model and animations
/// from its template 0x00159783 and was rooted by its placeholder (#1917) [observed:
/// fnvtool, the issue]. Without the actors' data, or when the creature does not resolve,
/// its own model is used.
fn cannot_walk(world: &WorldData, data: &nvf_assets::GameData, actors: Option<&ActorWorld>, base: u32, reference: u32) -> bool {
    use fnv_actor::locomotion::{Body, LocomotionSet, Move};
    let drawn = actors.and_then(|w| w.resolve_creature(base, reference).ok()).map(|c| c.skeleton);
    let Some(skeleton) = drawn.or_else(|| fnv_world::placed::base_object(world.lo, world.schema, base).and_then(|b| b.model_path())) else { return false };
    if !skeleton.to_ascii_lowercase().ends_with(".nif") {
        return false;
    }
    let s = fnv_vfs::normalize(&skeleton);
    let folder = s.rfind('\\').map_or("", |i| &s[..i]);
    let set = LocomotionSet::find(folder, Body::Male, crate::locomotion::STANCE, |p| data.vfs.exists(p));
    !set.clips.contains_key(&Move::Forward) && !set.clips.contains_key(&Move::FastForward)
}

/// A resident standing with nothing else to do ([`Residents::standing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    pub actor: u32,
    /// The base (IDLM) of the idle marker it stands at.
    pub marker: Option<FormId>,
}

/// Everything but the game state the choice runs in.
struct Core {
    ai: Ai,
    /// Opt-in phase-gated #1497 regression aid; absent in ordinary runs.
    sandbox_interrupt: Option<sandbox_interrupt::Probe>,
    /// Opt-in #2027 guard wander measurement; absent in ordinary runs.
    guard_wander: Option<guard_wander::Probe>,
    list: Vec<Resident>,
    known: HashSet<u32>,
    /// Cells whose actors were handed over (`all`), and the worldspace whose grid squares
    /// the exterior ones are: a grid square names another cell in another worldspace.
    cells_seen: HashSet<CellKey>,
    /// A co-op host: the exterior cells of other worldspaces (or of this one while the
    /// host is indoors) around guests away from it whose actors were handed over, by
    /// worldspace and grid square ([`ResidentsCore::take_cells`]).
    far_seen: HashSet<(u32, (i32, i32))>,
    /// Those worldspaces' persistent references by grid square, indexed once each.
    far_persistent: HashMap<u32, HashMap<(i32, i32), Vec<u32>>>,
    cells_seen_world: u32,
    /// The game's loaded cells when actors placed elsewhere were last looked for there.
    loaded_seen: BTreeSet<FormId>,
    clock: Clock,
    /// The game's clock was set and its periodic choice switched on.
    prepared: bool,
    /// The NPCs a load found seated, with their furniture (`Game::take_saved_seats`):
    /// each is put back on at once when it becomes a resident (#3991).
    saved_seats: HashMap<u32, u32>,
    /// The game's time of day at the last frame the choice ran.
    clock_text: String,
    /// Trips, arrivals, unreachable places, packages chosen.
    counts: [usize; 4],
    /// The character controller's stuck detection per actor, and who is left to the
    /// walker until when.
    unstuck: HashMap<u32, Unstuck>,
    skip_until: HashMap<u32, Skip>,
    hopped: HashMap<u32, (usize, usize)>,
    barred: HashMap<u32, u32>,
    coasting: HashMap<u32, Coasting>,
    skip_refusals: SkipRefusals,
    /// Updates in which an actor was moved by physics; times physics gave up; the most
    /// actors moved by physics in one frame; stuck actors moved on along their paths.
    physics: [usize; 4],
    /// Actors walked by physics that jumped up a ledge, dropped off one and jumped an
    /// obstacle (docs/spec-notes/actor-jumps.md).
    jumps: [usize; 3],
    /// Times an actor was told to sit, and to get up.
    seats: [usize; 2],
    /// Sandbox behaviours begun, by kind (standing, wandering, sitting, idle marker,
    /// sleeping, eating).
    sandbox: [usize; 6],
    /// Conversations and Say To lines started by Dialogue packages.
    talks: usize,
    /// Hellos said to the player, random conversations started, walks up to a partner,
    /// walks given up (`fnv_ai::social`).
    social: [usize; 4],
    /// Standing actors that had no room when pushed, and until when (elapsed seconds)
    /// each is left standing.
    no_room: HashMap<u32, f32>,
    /// Updates in which a standing actor was pushed aside, and in which one had no room.
    pushes: [usize; 2],
    /// How far standing actors were pushed in all, game units.
    pushed_units: f32,
    /// The pushes going on, by actor (for the log).
    pushing: HashMap<u32, Pushed>,
    /// Hours passed in the Wait menu, and the walks made in them.
    waited: [usize; 2],
    /// The fastest turn of a drawn resident between two frames (radians per second, who),
    /// and the frames in which one turned faster than [`SNAP_TURN`].
    fastest_turn: (f32, String),
    snaps: usize,
    /// Seconds drawn fighters facing their target (`aims`) moved forward, sideways and
    /// back (within 45° of their heading, within 135°, beyond).
    fight_moves: [f32; 3],
    /// Of those, seconds on the way to new cover (playtest list 7 item 43), and seconds
    /// on the way there facing within 45° of the way it goes (turned to its cover).
    cover_moves: [f32; 2],
    /// Fighters on their way to new cover this frame.
    to_cover: HashSet<FormId>,
    /// Frames of drawn residents on their way: all, those in which their character had
    /// not moved since the last frame (no physics step: drawn standing before the
    /// in-between drawing), and those drawn standing.
    walk_frames: [usize; 3],
    /// Of the frames drawn standing: with a physics character, turning on the spot, and
    /// the other walkers; the stops (standing between two frames walking, [`StopRun`]) of
    /// 1-3 frames and longer; and the walks that set off after standing first.
    standing_by: [usize; 6],
    /// The game was held this frame (menu, conversation, loading screen): nobody walks.
    held_now: bool,
    /// Actors playing a hit reaction (the stagger of a crippled limb, `deaths.rs`) this
    /// frame: they stand until it ends (owner's playtest list 4 item 21) [design].
    staggering: HashSet<u32>,
    /// Actors an idle holds in place (`idles::IdleHolds`, docs/spec-notes/idles.md §15):
    /// they stand as a staggering actor does while it shows.
    posed: HashSet<u32>,
    /// Creatures playing their unarmed stance's draw clip (`creature_stance.rs`: the
    /// ghoul's roar, the gecko's hiss) this frame: they stand until it ends, as a
    /// staggering actor does (#2686: an alerted ghoul slid across the floor roaring).
    drawing: HashSet<u32>,
    /// Of those, the ones whose idle carries the body away from the reference
    /// (`idles::IdleHolds::carried_off`): they are drawn facing as they did when it began.
    carried_off: HashSet<u32>,
    /// Of those, the ones whose AI has somewhere to go: they break out of the idle.
    breaking: HashSet<u32>,
    /// How far their attack clips carry actors this frame (`attack_anims::AttackTravel`,
    /// game x, y): their characters move them ([`Core::carry`]).
    travel: HashMap<u32, [f32; 2]>,
    /// The actors carried in the last update: their characters are kept for it. Each
    /// with where its feet were when the carry began and the travel asked of it since.
    carried: HashMap<u32, ([f32; 3], f32)>,
    /// Actors whose carry a hit reaction broke: the rest of that attack does not carry
    /// them (gone when the attack's travel ends).
    carry_broken: HashSet<u32>,
    /// Updates in which an attack carried an actor; times it could not be (no
    /// character); for the summary.
    carries: [usize; 2],
    /// Melee blows' knockbacks pushing actors (`nvf_play::combat::Knockback`), each with
    /// where the actor's feet were when it began ([`Core::knock`]).
    knocks: HashMap<u32, knock_run::KnockRun>,
    /// Knockbacks whose time was up when a new blow replaced them, with the feet then: told
    /// as finished by the next [`Core::knock`], which has the game state to say so.
    knocks_over: Vec<(knock_run::KnockRun, [f32; 3])>,
    /// Log each frame of a knockback (only with the melee knockback aid).
    knock_trace: bool,
    /// This frame's knockback travel (game x, y), kept apart from the attack clips'
    /// [`Core::travel`]: a hit reaction ends a clip's carry but not a knockback.
    knock_travel: HashMap<u32, [f32; 2]>,
    /// Actors on furniture this frame (their bodies drawn in the seat): no knockback.
    on_furniture: HashSet<u32>,
    /// Knockbacks begun and their distance in all (asked), for the summary.
    knocked: (usize, f32),
    /// Load doors used since the last frame: actor, door, the door it leads to.
    door_uses: Vec<(FormId, u32, u32)>,
    /// People in Combat (standing, closing in, going to a flank or a step back, in and
    /// out of their cover, and on their way to new cover; not fleeing): where their target is. They are drawn facing it
    /// and step sideways or back as they go, so they can fire on the move (owner's
    /// playtest list 5 item 33; owner 2026-10-16, strafing and backing away) [design].
    aims: HashMap<FormId, [f32; 3]>,
    /// Of those, the ones whose aim a weapon use owns, with the use's target
    /// ([`aim_target`]): no combat lock on another target turns them ([`keeps_lock`]).
    use_aims: HashMap<FormId, FormId>,
    /// The player's trip behind a loading screen under way (a fast travel, a load door):
    /// the actors coming into the loaded area are put at their package's place
    /// ([`Core::arrive`], issue #1208).
    trip: Option<Trip>,
    /// When (game hours: GameDaysPassed x 24) each resident was last in the loaded area,
    /// and the game hours at the last frame: a trip leaves alone whoever the player saw
    /// within [`RECENTLY_LOADED_HOURS`] ([`recently_loaded`]).
    loaded_at: HashMap<FormId, f64>,
    clock_hours: f64,
    /// Of all trips: actors put at their package's place, actors seated at once (put on
    /// their furniture, or found on it already), and actors left where they were because
    /// they were in the loaded area before the trip.
    arrivals: [usize; 3],
}

/// A trip of the player behind a loading screen ([`Core::arrive`]).
struct Trip {
    /// The residents in the loaded area before it: they are never moved (a short fast
    /// travel inside the loaded area moves nobody, issue #1208).
    before: HashSet<FormId>,
    /// The residents looked at since.
    seen: HashSet<FormId>,
    /// Frames since the loaded cells settled with the game no longer held by the screen.
    settled: u32,
    /// Residents put at their package's place in this trip, and those seated at once
    /// (put on their furniture, or found on it already).
    put: [usize; 2],
    /// The interiors behind the doors of the loaded cells, once they settled: actors in
    /// them are near the player ([`interiors_behind_doors`]).
    doors: Option<HashSet<u32>>,
}

/// A resident the player saw in the loaded area within this many game hours before a
/// trip is not moved by it: out of a house and straight back, the street is as it was
/// [design: coordinator review of #1269; the cell buffer's own life in the original is
/// not known in hours].
const RECENTLY_LOADED_HOURS: f64 = 1.0;

/// The residents in the loaded area within [`RECENTLY_LOADED_HOURS`] of `now` (game
/// hours), from when each was last there.
fn recently_loaded(loaded_at: &HashMap<FormId, f64>, now: f64) -> HashSet<FormId> {
    loaded_at.iter().filter(|(_, at)| now - **at <= RECENTLY_LOADED_HOURS).map(|(a, _)| *a).collect()
}

/// The interiors a load door of the loaded cells leads into (the original's Mid-Low
/// level, as `Core::take_cells` finds them).
fn interiors_behind_doors(world: &WorldData, streamer: &Streamer, refs: &RefTable) -> HashSet<u32> {
    let keys: Vec<CellKey> = match streamer.interior {
        Some(c) => vec![CellKey::Interior(c)],
        None => streamer.loaded.keys().map(|g| CellKey::Exterior(*g)).collect(),
    };
    let mut out = HashSet::new();
    for key in keys {
        let (cell, persistent) = match key {
            CellKey::Exterior(g) => (world.grid_cell(g), world.persistent_by_grid.get(&g)),
            CellKey::Interior(id) => (world.index.cells.get(&id), None),
        };
        for r in cell.into_iter().flat_map(|c| c.refs.iter()).chain(persistent.into_iter().flatten()) {
            if let Some(tp) = refs.get(FormId(*r)).and_then(|d| d.teleport.as_ref()) {
                out.extend(refs.teleport_cell(tp).filter(|c| refs.world_of(*c).is_none() && key != CellKey::Interior(c.0)).map(|c| c.0));
            }
        }
    }
    out
}

/// The cells of `now` (the loaded cells of worldspace `world`, or its interior) not
/// handed over yet: `seen` becomes `now`. A grid square of another worldspace is another
/// cell, so a change of worldspace forgets the exterior squares seen (`seen_world` is
/// the worldspace they were seen in). Sibling worldspaces lie on the same squares: TTW's
/// MegatonWorld on the Wasteland cells around its gate (issue #2233), Freeside's
/// FreesideWorld, FreesideNorthWorld and FreesideFortWorld on one another (#2432: Orris
/// and the King's bodyguard at the north gate; #2434: the Old Mormon Fort's people)
/// [observed: their cells share grid squares, `fnvtool world list`]. Kept by grid square
/// alone, the squares of the worldspace left counted as handed over, and the people of
/// the one entered never joined, so were never drawn.
fn newly_loaded(seen: &mut HashSet<CellKey>, seen_world: &mut u32, world: u32, now: HashSet<CellKey>) -> Vec<CellKey> {
    if *seen_world != world {
        seen.retain(|k| !matches!(k, CellKey::Exterior(_)));
        *seen_world = world;
    }
    let new: Vec<CellKey> = now.difference(seen).copied().collect();
    *seen = now;
    new
}

/// A trip ends this many frames after the loaded cells settled and the loading screen
/// let the game go [our choice: the frame the last cells' actors join, and the one
/// they are put in].
const TRIP_SETTLE_FRAMES: u32 = 2;

#[derive(Resource)]
pub struct Residents {
    /// The game state of the package choice without `--play`.
    own: Option<Box<Game<'static>>>,
    core: Core,
    /// A co-op guest's (`crate::coop`): the host runs the world, so no actor here thinks
    /// (no package, walk or fight); each is drawn where the host draws it, by reference.
    /// An actor the host has not drawn stays where it is. `None` in any other game.
    pub remote: Option<HashMap<u32, RemotePose>>,
    /// Every actor of the loaded cells.
    pub all: bool,
    started: bool,
    warned: bool,
    startup: Vec<(bool, String)>,
    /// The residents drawn under the start's loading screen.
    start: StartPlacing,
}

/// Standalone packages own their simulation; in play mode `SimState` owns it
/// [design: `drive`]. Shared by drawing and prefetch so neither selects a stale world.
impl Residents {
    /// The fastest an actor runs here (its alert or calm run speed, units/s), for a
    /// guest to tell a host NPC's script move from its run (`coop::NpcTrack::pose`).
    pub fn top_speed(&self, id: u32) -> Option<f32> {
        let id = FormId(id);
        self.core.ai.agents.contains_key(&id).then(|| self.core.ai.alert_speeds(id).run.max(self.core.ai.calm_speeds(id).run))
    }

    /// Where actor `id`'s AI has it now (space, feet, heading), when it is a resident
    /// this game runs (`coop::share_npcs`: an NPC the host stopped drawing).
    pub fn walker_of(&self, id: u32) -> Option<(Space, [f32; 3], f32)> {
        let a = self.core.ai.agents.get(&FormId(id))?;
        Some((a.walker.space, a.walker.feet, a.walker.heading))
    }

    /// The furniture actor `id` is being put on at once (wanted so and not on it yet):
    /// drawn but without `OnFurniture` for a frame or two, a co-op host sends it as on
    /// it, not as off (`coop::sent_furniture`) [design].
    pub fn being_put_on(&self, id: u32) -> Option<u32> {
        let r = self.core.list.iter().find(|r| r.actor.0 == id)?;
        match r.seat {
            Seat::Wanted(f, true) => Some(f),
            _ => None,
        }
    }

    /// The furniture actor `id`'s AI has it on where it stands (a seat, a bed, a cross),
    /// when it is a resident this game runs and is there, not on its way: the host sends
    /// it for one it does not draw (`coop::share_npcs`), so a guest seats it there.
    pub fn furniture_of(&self, id: u32) -> Option<u32> {
        let a = self.core.ai.agents.get(&FormId(id))?;
        seated_by_ai(a.activity, a.using.map(|f| f.0))
    }

    /// Co-op replication, the host's: the residents it simulates in its guests'
    /// surroundings (`Streamer::coop_areas`) and does not draw itself, each as
    /// (reference, base, space, feet, heading, walking). A guest draws them by these.
    pub fn simulated_undrawn(&self, world: &WorldData, streamer: &Streamer) -> Vec<(u32, u32, Space, [f32; 3], f32, bool)> {
        if streamer.coop_areas.is_empty() {
            return Vec::new();
        }
        self.core
            .list
            .iter()
            .filter(|r| r.joined && !r.failed && r.present)
            .filter_map(|r| {
                let a = self.core.ai.agents.get(&r.actor)?;
                let w = &a.walker;
                (!drawn(world, streamer, w.space, w.feet) && streamer.coop_areas.iter().any(|c| c.holds(w.space, w.feet))).then(|| {
                    // Not under the land (`actor_feet::on_the_land`).
                    let land = match w.space {
                        Space::World(ws) => crate::actor_feet::land_height_in(world, ws, w.feet[0], w.feet[1]),
                        Space::Interior(_) => None,
                    };
                    (r.actor.0, r.base, w.space, crate::actor_feet::on_the_land(w.feet, land), w.heading, a.activity == Activity::Travelling && w.moving)
                })
            })
            .collect()
    }
}

/// Where the host of a co-op game draws one of its actors ([`Residents::remote`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemotePose {
    pub feet: [f32; 3],
    pub heading: f32,
    pub moving: bool,
    /// The furniture the host has it use (a chair, a bed, a cross), and whether it is on
    /// it already (not walking up to it): a guest sits it there too.
    pub furniture: Option<(u32, bool)>,
    /// Where the host has it: its interior cell or the worldspace it stands in
    /// (`nvf_coop::net::NpcState::space`; 0: not told, where it was here). It is drawn
    /// only where that is loaded here [observed: buckallobill, in game, FNV 1.4.0.525:
    /// MPs fighting inside on the host were drawn outside on a guest].
    pub space: u32,
}

impl RemotePose {
    /// What a guest takes the actor to be doing: there (its furniture wanted, so kept on
    /// it) when standing, or whenever the host has it on furniture (going to it, getting
    /// on, seated) even as its feet move: getting on moves the body, and taken as a walk
    /// the guest's own getting on was told to get up [observed: buckallobill, in game,
    /// FNV 1.4.0.525; the rule: design].
    pub fn activity(&self) -> Activity {
        if self.moving && self.furniture.is_none() {
            Activity::Travelling
        } else {
            Activity::There
        }
    }
}

/// The furniture an actor's AI has it on ([`Residents::furniture_of`]): the one it
/// uses, once it is there; none while it travels (on its way to it, or away).
fn seated_by_ai(activity: Activity, using: Option<u32>) -> Option<u32> {
    using.filter(|_| activity == Activity::There)
}

pub(crate) fn equipment_world<'a, 'w>(private: Option<&'a World<'w>>, shared: Option<&'a World<'w>>) -> Option<&'a World<'w>> {
    private.or(shared)
}

fn set_global(g: &mut Game, edid: &str, v: f32) -> Result<(), String> {
    let (id, _) = g.world.forms.global_by_edid(edid).ok_or_else(|| format!("the load order has no {edid} global"))?;
    g.world.set_global(id, v).map_err(|e| e.to_string())
}

fn clock_text(g: &Game) -> String {
    hour_text(g.world.calendar().map(|c| c.hour).unwrap_or(0.0))
}

/// `h` (GameHour) as HH:MM.
fn hour_text(h: f32) -> String {
    format!("{:02}:{:02}", h.floor() as u32, ((h.fract() * 60.0).floor() as u32).min(59))
}

fn is_actor(refs: &RefTable, id: FormId) -> bool {
    refs.get(id).is_some_and(|r| matches!(&r.sig, b"ACHR" | b"ACRE"))
}

impl Residents {
    /// The residents in the high process now (FormIDs), for the game digest test aid
    /// (game_digest.rs).
    pub fn high_process_ids(&self) -> Vec<u32> {
        self.core.list.iter().filter(|r| self.core.ai.is_high(r.actor)).map(|r| r.actor.0).collect()
    }

    /// Put an actor's walker where its body now stands (game space feet, heading), as
    /// after getting up from furniture: a knocked-out actor laid over its ragdoll to get up
    /// (`knockouts.rs`) is drawn there from now on. False for an actor that is not a
    /// resident.
    pub fn put_walker(&mut self, actor: FormId, feet: [f32; 3], heading: f32) -> bool {
        let Some(a) = self.core.ai.agents.get_mut(&actor) else { return false };
        a.walker.feet = feet;
        a.walker.heading = heading;
        a.walker.tri = fnv_nav::graph::NONE;
        true
    }

    /// The walking and running speeds of actors without their own.
    pub fn default_speeds(&self) -> fnv_ai::Speeds {
        fnv_ai::Speeds { walk: self.core.ai.cfg.walk_speed, run: self.core.ai.cfg.run_speed }
    }

    /// `actor`'s alert walking and running speeds (searching, combat).
    pub fn alert_speeds(&self, actor: FormId) -> fnv_ai::Speeds {
        self.core.ai.alert_speeds(actor)
    }

    /// `actor`'s walk as the AI has it: whether it is on its way, its walker's speed and
    /// its calm run (`walk_probe.rs`).
    pub fn walk_of(&self, actor: FormId) -> Option<(bool, f32, f32)> {
        let a = self.core.ai.agents.get(&actor)?;
        Some((a.activity == fnv_ai::Activity::Travelling, a.walker.speed, self.core.ai.calm_speeds(actor).run))
    }

    /// The share of its speed `actor` moves at (crippled legs, `limping.rs`).
    pub fn set_pace(&mut self, actor: FormId, pace: f32) {
        self.core.ai.set_pace(actor, pace);
    }

    /// Give `actor` speeds of its own (`creature_pace.rs`).
    pub fn set_speeds(&mut self, actor: FormId, speeds: fnv_ai::ActorSpeeds) {
        self.core.ai.set_speeds(actor, speeds);
    }

    /// Whether actors standing or walking inside each other are moved apart
    /// (`fnv_ai::apart`; `--actors-pass-through` switches it off).
    pub fn set_keep_apart(&mut self, on: bool) {
        self.core.ai.cfg.keep_apart = on;
    }

    /// What the assets say about who can use which furniture (`fnv_ai::furniture`).
    pub fn set_furniture_facts(&mut self, facts: std::sync::Arc<dyn fnv_sim::furniture_facts::FurnitureFacts>) {
        self.core.ai.set_furniture_facts(facts);
    }

    /// Whether the AI runs this reference (it turns and moves its body).
    pub fn runs(&self, reference: u32) -> bool {
        self.core.ai.agents.contains_key(&FormId(reference))
    }

    /// Resolve the named actors and, without `--play` (`shared` false), build the game
    /// state the package choice runs in.
    pub fn build(world: &WorldData, names: &[String], clock: Clock, walk_speed: f32, shared: bool) -> Result<Residents, String> {
        let t0 = std::time::Instant::now();
        let refs: &'static RefTable = Box::leak(Box::new(RefTable::from_index(world.lo, world.schema, world.index)));
        let mut all = false;
        let mut core = Core {
            // Test aid: conversations between NPCs anywhere in the loaded area.
            ai: Ai::new(AiConfig { walk_speed, chats_anywhere: std::env::var("NVF_PLAY_TEST_CHATS_ANYWHERE").is_ok_and(|v| v == "1"), find_buying: std::env::var("NVF_PLAY_TEST_FIND_BUYING").is_ok_and(|v| v == "1"), ..AiConfig::default() }),
            sandbox_interrupt: sandbox_interrupt::Probe::from_env(refs),
            guard_wander: guard_wander::Probe::from_env(refs),
            list: Vec::new(),
            known: HashSet::new(),
            cells_seen: HashSet::new(),
            far_seen: HashSet::new(),
            far_persistent: HashMap::new(),
            cells_seen_world: 0,
            loaded_seen: BTreeSet::new(),
            clock,
            prepared: false,
            saved_seats: HashMap::new(),
            clock_text: String::new(),
            counts: [0; 4],
            unstuck: HashMap::new(),
            skip_until: HashMap::new(),
            hopped: HashMap::new(),
            barred: HashMap::new(),
            coasting: HashMap::new(),
            skip_refusals: SkipRefusals::default(),
            physics: [0; 4],
            jumps: [0; 3],
            seats: [0; 2],
            sandbox: [0; 6],
            talks: 0,
            social: [0; 4],
            no_room: HashMap::new(),
            pushes: [0; 2],
            pushed_units: 0.0,
            pushing: HashMap::new(),
            waited: [0; 2],
            fastest_turn: (0.0, String::new()),
            snaps: 0,
            fight_moves: [0.0; 3],
            cover_moves: [0.0; 2],
            to_cover: HashSet::new(),
            walk_frames: [0; 3],
            standing_by: [0; 6],
            held_now: false,
            staggering: HashSet::new(),
            posed: HashSet::new(),
            drawing: HashSet::new(),
            carried_off: HashSet::new(),
            breaking: HashSet::new(),
            travel: HashMap::new(),
            carried: HashMap::new(),
            carry_broken: HashSet::new(),
            carries: [0; 2],
            knocks: HashMap::new(),
            knocks_over: Vec::new(),
            knock_trace: std::env::var_os(nvf_play::MeleeKnockbackAid::ENV).is_some(),
            knock_travel: HashMap::new(),
            on_furniture: HashSet::new(),
            knocked: (0, 0.0),
            door_uses: Vec::new(),
            aims: HashMap::new(),
            use_aims: HashMap::new(),
            trip: None,
            loaded_at: HashMap::new(),
            clock_hours: 0.0,
            arrivals: [0; 3],
        };
        for name in names.iter().flat_map(|n| n.split(',')).map(str::trim).filter(|n| !n.is_empty()) {
            if name.eq_ignore_ascii_case("all") {
                all = true;
                continue;
            }
            let id = refs
                .by_edid(name)
                .or_else(|| u32::from_str_radix(name.trim_start_matches("0x").trim_start_matches("0X"), 16).ok().map(FormId))
                .filter(|id| is_actor(refs, *id))
                .ok_or_else(|| format!("--npc-packages {name}: not a placed actor"))?;
            core.add(refs, id, Some(name));
        }
        let mut startup = Vec::new();
        let own = if shared {
            startup.push((true, format!("ai: {} named residents{}; the package choice is the game's (--play)", core.list.len(), if all { " and every actor of the loaded cells" } else { "" })));
            None
        } else {
            let forms: &'static FormStore = Box::leak(Box::new(FormStore::build(world.lo, world.schema)));
            let path = std::env::var_os("NVF_PLAY_SCRIPT_FUNCTIONS").map(std::path::PathBuf::from).unwrap_or_else(|| DEFAULT_FUNCTIONS.into());
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let table = fnv_script::FunctionTable::load(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let data: &'static GameData = Box::leak(Box::new(GameData::build(world.lo, world.schema, table, &json, refs)));
            let game = Game::new(data, World::new(forms, refs, SimConfig::default()), GameConfig::default());
            for r in &core.list {
                let packages = game.package_list(r.actor);
                startup.push((
                    !packages.is_empty(),
                    format!("ai: {} ({}) has {} packages: {}", r.label, r.actor, packages.len(), packages.iter().map(|p| data.name(*p)).collect::<Vec<_>>().join(", ")),
                ));
            }
            startup.insert(
                0,
                (
                    true,
                    format!(
                        "ai: package choice ready in {:.2}s; {} named residents{}; the clock starts at {:02}:{:02} and runs {:.1} game minutes a second",
                        t0.elapsed().as_secs_f64(),
                        core.list.len(),
                        if all { " and every actor of the loaded cells" } else { "" },
                        clock.hour.floor() as u32,
                        (clock.hour.fract() * 60.0).floor() as u32,
                        clock.rate * 60.0
                    ),
                ),
            );
            Some(Box::new(game))
        };
        let under_screen = start_actors_under_screen(std::env::var(START_PLACE_AID).ok().as_deref());
        if !under_screen {
            startup.push((true, format!("test aid {START_PLACE_AID}=0: the start's residents are drawn after its loading screen, {SPAWNS_PER_FRAME} a frame")));
        }
        let start = StartPlacing { on: under_screen, game_frames: 0, placed: 0, told: false };
        Ok(Residents { own, core, remote: None, all, started: false, warned: false, startup, start })
    }

    /// The named references the cell streamer must leave to this module (with `all` it
    /// leaves every actor, `Streamer::manage_actors`).
    /// The residents standing where their packages have them, with nothing else to do
    /// (not walking, not on furniture, not talking).
    pub fn standing(&self) -> impl Iterator<Item = Standing> + '_ {
        self.core.list.iter().filter(|r| r.present && r.seat == Seat::None).filter_map(|r| Some(Standing { actor: r.actor.0, marker: r.stand? }))
    }

    /// This module's own game state (without `--play`).
    pub fn own_game(&mut self) -> Option<&mut Game<'static>> {
        self.own.as_deref_mut()
    }

    /// Use the same simulation as `drive` for resident prefetch, including a resident
    /// also listed among its cell's placed references [design: `read_ahead`].
    pub fn prefetch_world<'a>(&'a self, reference: u32, shared: Option<&'a World<'static>>) -> Option<&'a World<'static>> {
        let private = self.core.known.contains(&reference).then(|| self.own.as_deref().map(|g| &g.world)).flatten();
        equipment_world(private, shared)
    }

    pub fn references(&self) -> impl Iterator<Item = u32> + '_ {
        self.core.list.iter().map(|r| r.actor.0)
    }

    /// The residents present and not drawn whose walkers stand in exterior `cells` of
    /// worldspace `world`: (reference, base, creature), for reading their parts ahead
    /// (`readahead.rs`).
    pub fn undrawn_in(&self, world: u32, cells: &HashSet<(i32, i32)>) -> Vec<(u32, u32, bool)> {
        let at = |r: &Resident| {
            let w = &self.core.ai.agents.get(&r.actor)?.walker;
            (w.space == Space::World(world) && cells.contains(&fnv_world::cell_of(w.feet[0], w.feet[1]))).then_some((r.actor.0, r.base, r.creature))
        };
        self.core.list.iter().filter(|r| r.present && !r.spawned && !r.failed).filter_map(at).collect()
    }

    /// Draw `actor` anew (a resurrected corpse, `resurrect.rs`): its body is taken away and
    /// [`drive`] spawns a fresh one where its walker stands. Whether it is a resident.
    pub fn redraw(&mut self, commands: &mut Commands, actor: u32) -> bool {
        let Some(r) = self.core.list.iter_mut().find(|r| r.actor.0 == actor) else { return false };
        if let Some(e) = r.root.take() {
            commands.entity(e).despawn();
        } else if r.spawned {
            // Diagnostic for #2461: a body spawned and not yet found is not taken away.
            warn!("ai: {} drawn anew before its body spawned earlier was found: that body is not taken away", r.label);
        }
        r.spawned = false;
        r.last_drawn = None;
        r.seat = Seat::None;
        true
    }
}

/// A resident went through a load door: the body it leaves at the door is let go (its
/// `RefVisual` and `ActorRig` taken, so nothing takes it for the resident's any more) and
/// fades out (`door_fade.rs` despawns it); the resident is drawn anew at the far side
/// (`nav::show_actor`, which takes the new body as its root). The body faded, if one was
/// drawn and known.
fn hand_off_body(commands: &mut Commands, (spawned, root): (&mut bool, &mut Option<Entity>), now: f32) -> Option<Entity> {
    let faded = root.take().filter(|_| *spawned);
    if let Some(e) = faded {
        commands.entity(e).remove::<(RefVisual, crate::limbs::ActorRig)>().insert(crate::door_fade::DoorFade { since: now, out: true });
    }
    *spawned = false;
    faded
}

/// A conversation or a greeting holds `actor` where it stands (`Core::think`).
fn held_by_talk(game: &Game, ai: &Ai, actor: FormId) -> bool {
    (game.in_actor_conversation(actor) && !game.conversation_moves(actor)) || ai.holds_for_chat(actor) || game.greets_player(actor)
}

impl Core {
    /// Look up who of the listed creatures cannot walk ([`cannot_walk`], or flagged
    /// Immobile), before they are handed to the AI.
    fn check_rooted(&mut self, world: &WorldData, data: &nvf_assets::GameData, actors: Option<&ActorWorld>) {
        let mut by_base: HashMap<u32, (bool, bool, bool)> = HashMap::new();
        for r in self.list.iter_mut().filter(|r| r.rooted.is_none()) {
            let reference = r.actor.0;
            let (rooted, perched, immobile) = *by_base.entry(r.base).or_insert_with(|| {
                // An Immobile creature (the turrets, Mr. House's screens) is held too:
                // the flag "prevents movement" [documented: GECK wiki Animation Tab].
                // A leveled template's pick is the first resident's of that base
                // [hypothesis: the picks of one base share a skeleton family].
                let immobile = acbs_flag(world, r.base, ACBS_IMMOBILE);
                let rooted = cannot_walk(world, data, actors, r.base, reference) || immobile;
                (rooted, rooted && flies(world, r.base), immobile)
            });
            r.rooted = Some(rooted);
            r.perched = perched;
            r.immobile = immobile;
        }
    }

    fn add(&mut self, refs: &RefTable, id: FormId, name: Option<&str>) {
        let Some(placed) = refs.get(id) else { return };
        let label = name.map(str::to_string).or_else(|| placed.edid.clone()).unwrap_or_else(|| id.to_string());
        self.add_resident(id, placed.base.0, &placed.sig == b"ACRE", placed.scale.unwrap_or(1.0), placed.pos, label, name.is_some());
    }

    /// The actors scripts created since the last frame (PlaceAtMe of an NPC_, CREA or
    /// leveled actor list, `fnv_game::Game::created_actors`): residents from where they
    /// were put, so they are drawn, walk, sense and fight as placed actors do [until
    /// 2026-10-16 they were drawn as a static model and never fought: owner's playtest].
    fn take_created(&mut self, game: &Game) {
        for id in game.created_actors() {
            if self.known.contains(&id.0) {
                continue;
            }
            let Some(s) = game.world.peek(id) else { continue };
            let (base, pos) = (s.base, s.pos);
            let creature = matches!(game.world.forms.sig(base), Some(sig) if &sig == b"CREA" || &sig == b"LVLC");
            let label = format!("{} {id}", game.data.name(base));
            info!("ai [{}] {label}: created by a script (PlaceAtMe) at ({:.0}, {:.0}, {:.0}): a resident from there", self.clock_text, pos[0], pos[1], pos[2]);
            self.add_resident(id, base.0, creature, 1.0, pos, label, true);
            if let Some(r) = self.list.last_mut().filter(|r| r.actor == id) {
                r.first_steps = Some(FirstSteps::new("its creation by a script"));
                r.at_once = true;
            }
        }
    }

    /// Actors scripts created since the last frame first stand on a free spot near where
    /// they were put, not inside the object that made them or each other
    /// (`nvf_play::created_spots`; playtest list 7 item 4).
    fn spread_created(&mut self, game: &mut Game, phys: &mut nvf_play::phys::PhysState) {
        let new: Vec<FormId> = game.created_actors().into_iter().filter(|id| !self.known.contains(&id.0)).collect();
        let refs = game.world.placed;
        for (id, how) in nvf_play::created_spots::place(&mut game.world, refs, phys, &new) {
            match how {
                nvf_play::created_spots::Placement::Kept => {}
                nvf_play::created_spots::Placement::Moved { from, to } => info!(
                    "ai [{}] {} {id}: created at ({:.0}, {:.0}, {:.0}), stands at ({:.0}, {:.0}, {:.0}), {:.0} units away (the nearest free spot)",
                    self.clock_text,
                    game.data.name(game.world.peek(id).map_or(id, |s| s.base)),
                    from[0],
                    from[1],
                    from[2],
                    to[0],
                    to[1],
                    to[2],
                    (from[0] - to[0]).hypot(from[1] - to[1])
                ),
                nvf_play::created_spots::Placement::NoSpot => {
                    info!("ai [{}] {id}: created where no free spot is near: left where it was put", self.clock_text)
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_resident(&mut self, id: FormId, base: u32, creature: bool, scale: f32, pos: [f32; 3], label: String, named: bool) {
        if !self.known.insert(id.0) {
            return;
        }
        self.list.push(Resident {
            actor: id,
            base,
            creature,
            scale,
            label,
            named,
            joined: false,
            spawned: false,
            at_once: false,
            root: None,
            failed: false,
            present: false,
            waiting: None,
            doors_opened: 0,
            last_feet: pos,
            drawn_feet: pos,
            standing_frames: StopRun::default(),
            seat: self.saved_seats.remove(&id.0).map_or(Seat::None, |f| Seat::Wanted(f, true)),
            may_ask_again: false,
            stand: None,
            last_drawn: None,
            wants: None,
            restarts: 0,
            restart: false,
            // Only creatures: people walk with the race's clips.
            rooted: (!creature).then_some(false),
            perched: false,
            immobile: false,
            moved: false,
            jumped: false,
            last_space: None,
            aim_heading: None,
            lock_held: None,
            posed_heading: None,
            been_loaded: false,
            absent: false,
            first_steps: None,
        });
    }

    /// The actors of exterior cells, given by worldspace and grid square, that are not the
    /// cells the host draws (`take_cells`): the cell's own references and the worldspace's
    /// persistent ones standing in that square, as for the host's own cells.
    fn take_far_cells(&mut self, world: &WorldData, refs: &RefTable, now: Vec<(u32, (i32, i32))>) {
        let now: HashSet<(u32, (i32, i32))> = now.into_iter().collect();
        let new: Vec<(u32, (i32, i32))> = now.difference(&self.far_seen).copied().collect();
        self.far_seen = now;
        let had = self.list.len();
        for (w, g) in new {
            let Some(ws) = world.index.worldspaces.get(&w) else { continue };
            let persistent = self.far_persistent.entry(w).or_insert_with(|| {
                let mut by_grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
                let pc = ws.persistent_cell.and_then(|c| world.index.cells.get(&c));
                for &r in pc.map_or(&[][..], |c| &c.refs) {
                    if let Some(p) = refs.get(FormId(r)) {
                        by_grid.entry(fnv_world::cell_of(p.pos[0], p.pos[1])).or_default().push(r);
                    }
                }
                by_grid
            });
            let mine: Vec<u32> = persistent.get(&g).cloned().unwrap_or_default();
            let cell = ws.grid.get(&g).and_then(|c| world.index.cells.get(c));
            for r in cell.into_iter().flat_map(|c| c.refs.iter().copied()).chain(mine) {
                if is_actor(refs, FormId(r)) {
                    self.add(refs, FormId(r), None);
                }
            }
        }
        if self.list.len() > had {
            info!("ai: {} actors of the cells around co-op guests away from the host join ({} in all)", self.list.len() - had, self.list.len());
        }
    }

    /// With `all`: hand over the actors of the cells that have loaded since last time.
    fn take_cells(&mut self, world: &WorldData, streamer: &Streamer, refs: &RefTable) {
        let mut now: HashSet<CellKey> = match streamer.interior {
            Some(c) => [CellKey::Interior(c)].into(),
            None => streamer.loaded.keys().map(|g| CellKey::Exterior(*g)).collect(),
        };
        // A co-op host: the cells around each guest away from it are simulated too
        // (`crate::coop::areas`), so their actors join as the host's own cells' do. With only
        // the host's cells and the interiors behind their doors taken, a guest away at Primm
        // found it empty [observed: buckallobill, in game, FNV 1.4.0.525]. Exterior cells of the worldspace the
        // host draws go by its grid (`WorldData::grid_cell`); those of any other worldspace
        // (Freeside, the Strip, a DLC's), or of this one while the host is indoors, by
        // [`Self::take_far_cells`].
        let mut far = Vec::new();
        for area in &streamer.coop_areas {
            match area.space {
                Space::Interior(c) => {
                    now.insert(CellKey::Interior(c));
                }
                Space::World(w) if w == world.world && streamer.interior.is_none() => now.extend(area.cells.iter().map(|g| CellKey::Exterior(*g))),
                Space::World(w) => far.extend(area.cells.iter().map(|g| (w, *g))),
            }
        }
        self.take_far_cells(world, refs, far);
        let new = newly_loaded(&mut self.cells_seen, &mut self.cells_seen_world, world.world, now);
        let before = self.list.len();
        for key in new {
            let (cell, persistent) = match key {
                CellKey::Exterior(g) => (world.grid_cell(g), world.persistent_by_grid.get(&g)),
                CellKey::Interior(id) => (world.index.cells.get(&id), None),
            };
            let mut behind_doors = Vec::new();
            for r in cell.into_iter().flat_map(|c| c.refs.iter()).chain(persistent.into_iter().flatten()) {
                if is_actor(refs, FormId(*r)) {
                    self.add(refs, FormId(*r), None);
                } else if let Some(tp) = refs.get(FormId(*r)).and_then(|d| d.teleport.as_ref()) {
                    behind_doors.extend(refs.teleport_cell(tp).filter(|c| refs.world_of(*c).is_none() && key != CellKey::Interior(c.0)));
                }
            }
            // The original's Mid-Low level: the actors of the interiors reached by doors
            // from the loaded cells, never visited, are processed too [documented: GECK
            // wiki (JIP) GetActorProcessingLevel; `fnv_ai::process`]. Without them an
            // actor placed indoors never left until the player had been in: Boone's
            // night patrol to the dinosaur's mouth (owner 2026-10-26).
            for id in behind_doors {
                let Some(c) = world.index.cells.get(&id.0) else { continue };
                let had = self.list.len();
                for r in c.refs.iter().filter(|r| is_actor(refs, FormId(**r))) {
                    self.add(refs, FormId(*r), None);
                }
                if self.list.len() > had {
                    let names: Vec<&str> = self.list[had..].iter().map(|r| r.label.as_str()).collect();
                    info!("ai: {} behind a door from the loaded cells, in {}: {}", names.len(), c.edid.as_deref().unwrap_or("an interior"), names.join(", "));
                }
            }
        }
        if self.list.len() > before {
            debug!("ai: {} more residents ({} in all)", self.list.len() - before, self.list.len());
        }
    }

    /// The actors a script put somewhere (MoveTo) since the last frame: a resident goes
    /// on from where it was put (`fnv_ai::Ai::relocate`), off its furniture and without
    /// its character; with `all`, an actor that is not a resident yet becomes one, from
    /// where it was put (an actor placed in a cell that is not loaded, moved into one
    /// that is or will be: Sunny Smiles to the gas station). [Until 2026-10-11 the AI
    /// kept such an actor where it had stood: owner playtest.]
    fn take_moved(&mut self, game: &mut Game, all: bool, mut phys: Option<&mut PhysState>) {
        let refs = game.world.placed;
        for (id, ours) in self.ai.moved_by_scripts(game) {
            // An actor a script made (PlaceAtMe) is in no plugin's table of placed
            // references, yet the AI looks after it (`ours`): it is moved as any other.
            // Left out here, its character stayed where it was made, and the walker, which
            // reads its place from its character, went on from there: a radscorpion made
            // at the player and moved 600 units out 0.2 s later fought from beside the
            // player in about half the runs, whenever its character already existed
            // [observed: smoke `radscorpions-spacing-cadence`, 21 probe runs, #3366].
            if !ours && !is_actor(refs, id) {
                continue;
            }
            if let Some(run) = self.knocks.get_mut(&id.0) {
                run.moved = true;
            }
            let place = game.world.peek(id).map(|s| (s.cell, s.pos));
            let cell = place.and_then(|p| p.0).map_or_else(|| "no cell".to_string(), |c| game.data.name(c));
            let pos = place.map_or([0.0; 3], |p| p.1);
            // A teammate the player's change of cells brought along (`fnv_game::followers`).
            let why = match game.brought_along.remove(&id) {
                true if game.is_teammate(id) => "with the player (teammate)",
                true => "with the player (follower)",
                false => "by a script (MoveTo)",
            };
            if ours {
                if let Some(p) = phys.as_deref_mut() {
                    p.remove_npc(id.0);
                }
                self.unstuck.remove(&id.0);
                self.skip_until.remove(&id.0);
                self.skip_refusals.forget(id.0);
                if let Some(r) = self.list.iter_mut().find(|r| r.actor == id) {
                    r.moved = true;
                    info!("ai [{}] {}: put at ({:.0}, {:.0}, {:.0}) in {cell} {why}: goes on from there", self.clock_text, r.label, pos[0], pos[1], pos[2]);
                }
            } else if all && !self.known.contains(&id.0) {
                self.add(refs, id, None);
                let label = self.list.last().map_or_else(|| id.to_string(), |r| r.label.clone());
                info!("ai [{}] {label}: put at ({:.0}, {:.0}, {:.0}) in {cell} {why}: a resident from there", self.clock_text, pos[0], pos[1], pos[2]);
            }
        }
    }

    /// With `all`: the placed actors that stand in a loaded cell other than the one they
    /// were placed in (walked or moved there before a save, which puts them back there):
    /// residents from where they are. [Until 2026-10-26 a loaded save left them out, so
    /// they were not drawn while the compass still showed them: Jeannie May Crawford
    /// and Victor in Novac, owner playtest.] Looked for when the loaded cells change.
    fn take_placed_elsewhere(&mut self, game: &Game) {
        if self.loaded_seen == game.loaded_cells {
            return;
        }
        self.loaded_seen = game.loaded_cells.clone();
        let refs = game.world.placed;
        let found: Vec<FormId> = game
            .world
            .changed()
            .filter(|(id, s)| *id < FormId::FIRST_DYNAMIC && !s.deleted() && !self.known.contains(&id.0) && is_actor(refs, *id))
            .filter(|(id, s)| s.cell.is_some_and(|c| game.loaded_cells.contains(&c)) && s.cell != refs.get(*id).and_then(|p| p.cell))
            .map(|(id, _)| id)
            .collect();
        for id in found {
            self.add(refs, id, None);
            let Some(s) = game.world.peek(id) else { continue };
            let label = self.list.last().map_or_else(|| id.to_string(), |r| r.label.clone());
            let cell = s.cell.map_or_else(|| "no cell".to_string(), |c| game.data.name(c));
            info!("ai [{}] {label}: at ({:.0}, {:.0}, {:.0}) in {cell}, away from the cell it was placed in: a resident from there", self.clock_text, s.pos[0], s.pos[1], s.pos[2]);
        }
    }

    /// Who is in the high process: the residents in the loaded area (the cells drawn
    /// around the camera, or its interior; `fnv_ai::process`). The others get a cheap
    /// update now and then: no conversations, hellos, searches or sandboxes far away
    /// (owner's playtest 2026-10-11: Fiends talked near Camp McCarran, heard from
    /// Crimson Caravan).
    fn process_levels(&mut self, game: &Game, world: &WorldData, streamer: &Streamer) {
        // A far walk stops where it comes into the loaded area (`fnv_ai::process`).
        let loaded = match streamer.interior {
            Some(c) => fnv_ai::LoadedArea { space: Space::Interior(c), cells: Default::default() },
            None => fnv_ai::LoadedArea { space: Space::World(world.world), cells: streamer.loaded.keys().copied().collect() },
        };
        if self.ai.loaded() != Some(&loaded) {
            self.ai.set_loaded(Some(loaded));
        }
        if let Ok(c) = game.world.calendar() {
            self.clock_hours = c.days_passed as f64 * 24.0;
        }
        // Diagnostics (`profile::AiCount`): residents classified, remaining paths scanned
        // for the loaded area and their points tested, levels changed.
        let (mut classified, mut scans, mut points, mut changes) = (0usize, 0usize, 0usize, 0usize);
        for r in self.list.iter_mut().filter(|r| r.joined && !r.failed) {
            let Some(a) = self.ai.agents.get(&r.actor) else { continue };
            classified += 1;
            let (space, feet) = (a.walker.space, a.walker.feet);
            let high = simulated(world, streamer, space, feet);
            r.been_loaded |= high;
            if high {
                self.loaded_at.insert(r.actor, self.clock_hours);
            }
            // Out of the loaded area: Mid-High in the cell buffer, Low beyond (`cell_buffer.rs`).
            // Walking out of the loaded area into a cell the buffer does not hold drops the
            // actor to Low: the original's Victor went from process level 0 straight to 3,
            // never 1 [observed: docs/handoff/observer-jip-audit-2026-10-25.md §3.1]. One
            // already at Mid-High walking on into another cell takes that cell along, and so
            // does one going through a door into an interior: interiors behind the loaded
            // area's doors read Mid-Low there (same §3.1; Benny into the presidential suite).
            let new_cell = fnv_world::cell_of(r.last_feet[0], r.last_feet[1]) != fnv_world::cell_of(feet[0], feet[1]);
            let into_interior = r.been_loaded && matches!(space, Space::Interior(_));
            if carries_cell(high, streamer.buffer.holds(space, feet), new_cell, into_interior, self.ai.level(r.actor)) {
                streamer.buffer.carry(space, feet);
            }
            // On its way into the loaded area (a corner of the rest of its path is in
            // it): moved every frame, so it comes through the door when it would (owner's
            // playtest list 4 item 13: Ringo out of the gas station). Only from the cell
            // buffer: from beyond, the original moves it in its hourly jumps up to the
            // edge of the loaded area (Malcolm Holmes, `fnv_ai::process`), except one
            // following a leader in the loaded area (Cheyenne after Sunny Smiles).
            let coming = !high
                && (streamer.buffer.holds(space, feet) || self.ai.follows_near_leader(r.actor))
                && a.activity == fnv_ai::Activity::Travelling
                && a.walker.follower.as_ref().is_some_and(|f| {
                    let _span = info_span!("ai::coming_scan").entered();
                    scans += 1;
                    f.path.legs.iter().skip(f.leg).any(|l| {
                        l.points.iter().any(|p| {
                            points += 1;
                            simulated(world, streamer, l.space, *p)
                        })
                    })
                });
            let level = level_for(high, coming, streamer.buffer.holds(space, feet));
            let was_high = self.ai.is_high(r.actor);
            if self.ai.set_level(game, r.actor, level) {
                changes += 1;
                let line = format!(
                    "ai [{}] {}: {} at ({:.0}, {:.0}, {:.0}) in {}",
                    self.clock_text,
                    r.label,
                    match level {
                        fnv_ai::ProcessLevel::High => "back in the loaded area",
                        fnv_ai::ProcessLevel::MidHigh if was_high => "out of the loaded area",
                        fnv_ai::ProcessLevel::MidHigh => "back in the cell buffer (low process every 18 s)",
                        fnv_ai::ProcessLevel::Low if was_high => "out of the loaded area and the cell buffer (low process every 120 s)",
                        fnv_ai::ProcessLevel::Low => "out of the cell buffer (low process every 120 s)",
                        fnv_ai::ProcessLevel::Coming => "on its way into the loaded area (moved every frame)",
                    },
                    feet[0],
                    feet[1],
                    feet[2],
                    nav::space_name(world, space)
                );
                if r.named {
                    info!("{line}");
                } else {
                    debug!("{line}");
                }
            }
        }
        crate::profile::ai_count(crate::profile::AiCount::Classified, classified);
        crate::profile::ai_count(crate::profile::AiCount::ComingScans, scans);
        crate::profile::ai_count(crate::profile::AiCount::PathPointsTested, points);
        crate::profile::ai_count(crate::profile::AiCount::LevelChanges, changes);
    }

    /// The residents not handed to the AI yet are.
    fn join(&mut self, game: &Game) {
        for r in self.list.iter_mut().filter(|r| !r.joined) {
            r.joined = true;
            if let Err(e) = self.ai.add(game, r.actor) {
                if r.named {
                    warn!("ai: {} cannot be handed to the AI: {e}", r.label);
                }
                r.failed = true;
            } else if r.rooted == Some(true) {
                self.ai.root(game, r.actor);
                info!("ai: {} cannot move (no movement animations, or flagged Immobile): it stays where it is{}", r.label, if r.perched { "; it flies, so it is in nobody's way" } else { "" });
                if never_turns(r.immobile, game.creature_turning_speed(FormId(r.base), r.actor)) {
                    self.ai.fix_heading(r.actor);
                    info!("ai: {} never turns (flagged Immobile, Turning Speed 0): it keeps its heading", r.label);
                } else if game.is_fixture(r.actor) {
                    // A fixture that never fights (`fnv_game::Game::is_fixture`): Mr.
                    // House's chamber, Turning Speed 30, never turns either (#2431).
                    self.ai.fix_heading(r.actor);
                    info!("ai: {} never turns (a fixture: Immobile, Unaggressive, Helps Nobody, unarmed): it keeps its heading", r.label);
                }
            }
        }
    }

    /// The residents not handed to the AI yet (none is under the start's screen), each
    /// where [`Core::join`] will put its walker (`fnv_ai::Ai::add`: where the game has it,
    /// facing the same way) and whether the game has it in the world ([`StartPlacing`]).
    fn start_bodies(&self, game: &Game) -> Vec<StartBody> {
        let mut out = Vec::new();
        for (index, r) in self.list.iter().enumerate().filter(|(_, r)| !r.joined && !r.failed) {
            let Some(place) = fnv_ai::goal::place_of_ref(game, r.actor) else { continue };
            // The heading as `fnv_ai::Ai::add` gives its walker.
            let heading = game.world.peek(r.actor).map(|s| s.rot[2]).or_else(|| game.world.placed.get(r.actor).map(|p| p.rot[2])).unwrap_or(0.0);
            out.push(StartBody { index, space: place.space, feet: place.pos, heading, present: game.present(r.actor) });
        }
        out
    }

    /// The player sets off through a load door or on a fast travel, behind a loading
    /// screen (`nvf_play::Travel` with `Loading::arriving`): who was in the loaded area
    /// within [`RECENTLY_LOADED_HOURS`] stays where it is through the trip. The clock is
    /// the last frame's: a fast travel's hours have passed already, so the residents of
    /// the place left are not kept.
    fn begin_trip(&mut self) {
        let before = recently_loaded(&self.loaded_at, self.clock_hours);
        info!("ai [{}] trip behind a loading screen: {} residents in the loaded area in the last {RECENTLY_LOADED_HOURS} game hour(s) stay where they are", self.clock_text, before.len());
        self.arrivals[2] += before.len();
        self.trip = Some(Trip { before, seen: HashSet::new(), settled: 0, put: [0; 2], doors: None });
    }

    /// While a trip is under way: the residents near the player (in the loaded area, the
    /// cell buffer, or an interior behind a door of the loaded cells) whose package's
    /// walk starts there are put where that walk ends, on its furniture (seated at once),
    /// before the loading screen comes down (`fnv_ai::arrive`; issue #1208: Boone at the
    /// dinosaur on a fast travel to Novac at 5am, Cass at the Mojave Outpost's bar on
    /// entering) [observed, owner; the placing itself: hypothesis]. They join the AI and
    /// are drawn under the screen, so the screen waits for their bodies
    /// (`readahead::actors_waiting`). They are put once the cells around the player have
    /// all loaded (`Streamer::settled`, which the screen waits for too), each resident
    /// looked at once; the others are not touched (the low process keeps them). A
    /// resident a script moved this frame (MoveTo) stays where it was put. The trip ends
    /// [`TRIP_SETTLE_FRAMES`] after that and the screen let the game go.
    fn arrive(&mut self, game: &mut Game, nav: &NavState, world: &WorldData, streamer: &Streamer, mut phys: Option<&mut PhysState>) {
        if self.trip.is_none() {
            return;
        }
        self.join(game);
        // Once the cells around the player are all in (the screen stays up for them):
        // before, a walk's end could be in a cell not loaded yet and the walk left to be
        // made in sight (Boone stopped in the gift shop, the dinosaur's cell not in).
        if !streamer.settled {
            return;
        }
        let trip = self.trip.as_mut().expect("checked");
        let doors = trip.doors.get_or_insert_with(|| interiors_behind_doors(world, streamer, game.world.placed));
        let near = |p: fnv_nav::Place| drawn(world, streamer, p.space, p.pos) || streamer.buffer.holds(p.space, p.pos) || matches!(p.space, Space::Interior(c) if doors.contains(&c));
        for r in self.list.iter_mut().filter(|r| r.joined && !r.failed) {
            // Each resident once; those seen shortly before, and one a script has just
            // moved (MoveTo), are left as they are.
            if trip.before.contains(&r.actor) || !trip.seen.insert(r.actor) || r.moved {
                continue;
            }
            let Some(a) = self.ai.agents.get(&r.actor) else { continue };
            if !near(fnv_nav::Place { space: a.walker.space, pos: a.walker.feet }) {
                continue;
            }
            let arrival = self.ai.arrive(game, &nav.graph, r.actor, &near);
            r.present = game.present(r.actor);
            let Some(arr) = arrival else {
                // Already at its place and on its furniture (the low process had it
                // there, behind the door): seated at once too, not walked up to it in
                // sight (the Goodsprings settler's bar stool in the Prospector Saloon).
                let a = &self.ai.agents[&r.actor];
                // Its seat not yet taken up (`Seat::Wanted`: it was never drawn there).
                let unseated = matches!(r.seat, Seat::None | Seat::Wanted(..) | Seat::Waiting(..) | Seat::Unavailable(_));
                if let (Some(f), Activity::There, true) = (a.using, a.activity, unseated) {
                    r.wants = Some(f.0);
                    r.seat = Seat::Wanted(f.0, true);
                    self.arrivals[1] += 1;
                    trip.put[1] += 1;
                    let line = format!("ai [{}] {}: already on {} at its package's place on arrival: seated at once", self.clock_text, r.label, game.data.name(f));
                    if r.named {
                        info!("{line}");
                    } else {
                        debug!("{line}");
                    }
                }
                continue;
            };
            self.arrivals[0] += 1;
            trip.put[0] += 1;
            // Its character, if it had one, is made again where it is now (as after a
            // MoveTo, [`Core::take_moved`]).
            if let Some(p) = phys.as_deref_mut() {
                p.remove_npc(r.actor.0);
            }
            self.unstuck.remove(&r.actor.0);
            self.skip_until.remove(&r.actor.0);
            self.skip_refusals.forget(r.actor.0);
            if let Some(f) = arr.furniture {
                // Sat down at once, as found sitting (no walk up, no sitting-down clip).
                r.wants = Some(f.0);
                r.seat = Seat::Wanted(f.0, true);
                self.arrivals[1] += 1;
                trip.put[1] += 1;
            }
            let line = format!(
                "ai [{}] {}: put at its package's place on arrival ({}): ({:.0}, {:.0}, {:.0}) in {}, {:.0} units from ({:.0}, {:.0}, {:.0}) in {}, {} walk(s) skipped{}",
                self.clock_text,
                r.label,
                game.data.name(arr.package),
                arr.to.pos[0],
                arr.to.pos[1],
                arr.to.pos[2],
                nav::space_name(world, arr.to.space),
                arr.skipped,
                arr.from.pos[0],
                arr.from.pos[1],
                arr.from.pos[2],
                nav::space_name(world, arr.from.space),
                arr.walks,
                arr.furniture.map_or_else(String::new, |f| format!(", seated on {}", game.data.name(f)))
            );
            // Every one: a trip puts few, and the smoke tests read where they came from.
            info!("{line}");
        }
        if streamer.settled && !game.loading {
            trip.settled += 1;
        }
        if trip.settled >= TRIP_SETTLE_FRAMES {
            info!("ai [{}] trip over: {} residents put at their package's place, {} seated at once", self.clock_text, trip.put[0], trip.put[1]);
            self.trip = None;
        }
    }

    /// A script moved the clock by more than [`fnv_game::ai::SCRIPT_HOUR_JUMP`] since the
    /// last frame (`Game::take_hour_jump`; issue #2353: VCG01 stage 200's `set GameHour to
    /// 8` after the player rushed out of Doc Mitchell's house, Goodsprings already placed
    /// for the early morning, Easy Pete in bed): as at the end of a wait, everybody
    /// chooses its package now, and the residents the player cannot see (in the cell
    /// buffer or an interior behind a door of the loaded cells, not in a drawn cell) are
    /// put where their new package's walk ends, on its furniture, as on arrival
    /// (`fnv_ai::Ai::hour_jumped`). Residents in view are not moved: they walk to their
    /// new packages in sight [design, #2353]. Runs while a menu holds the game too (the
    /// console's `set GameHour to`).
    fn after_hour_jump(&mut self, game: &mut Game, nav: &NavState, world: &WorldData, streamer: &Streamer, mut phys: Option<&mut PhysState>) {
        let Some(jump) = game.take_hour_jump() else { return };
        self.join(game);
        let doors = interiors_behind_doors(world, streamer, game.world.placed);
        let unseen = |p: fnv_nav::Place| !drawn(world, streamer, p.space, p.pos) && (streamer.buffer.holds(p.space, p.pos) || matches!(p.space, Space::Interior(c) if doors.contains(&c)));
        let arrivals = self.ai.hour_jumped(game, &nav.graph, &unseen);
        self.clock_text = clock_text(game);
        info!(
            "ai [{}] the clock was set from {} to {} by a script: packages chosen again; {} of {} residents out of sight put at their package's place",
            self.clock_text,
            hour_text(jump.from),
            hour_text(jump.to),
            arrivals.len(),
            self.ai.agents.len()
        );
        for (actor, arr) in arrivals {
            let Some(r) = self.list.iter_mut().find(|r| r.actor == actor) else { continue };
            r.present = game.present(actor);
            r.jumped = true;
            // Its character, if it had one, is made again where it is now (as on arrival).
            if let Some(p) = phys.as_deref_mut() {
                p.remove_npc(actor.0);
            }
            self.unstuck.remove(&actor.0);
            self.skip_until.remove(&actor.0);
            self.skip_refusals.forget(actor.0);
            // The furniture it is to use now, none at a standing place: set for every one,
            // as `note` does not run while a menu holds the game (the console's jump), and
            // an old wish would sit it back on its old furniture once drawn.
            (r.wants, r.seat) = seat_after_jump(r.seat, arr.furniture.map(|f| f.0));
            // Every one: a jump puts few, and the smoke tests read where they came from.
            info!(
                "ai [{}] {}: put at its package's place after the clock was set ({}): ({:.0}, {:.0}, {:.0}) in {}, from ({:.0}, {:.0}, {:.0}) in {}, {} walk(s) skipped{}",
                self.clock_text,
                r.label,
                game.data.name(arr.package),
                arr.to.pos[0],
                arr.to.pos[1],
                arr.to.pos[2],
                nav::space_name(world, arr.to.space),
                arr.from.pos[0],
                arr.from.pos[1],
                arr.from.pos[2],
                nav::space_name(world, arr.from.space),
                arr.walks,
                arr.furniture.map_or_else(String::new, |f| format!(", seated on {}", game.data.name(f)))
            );
        }
    }

    /// One frame of the package choice and the walks in `game`. `own`: the game state is
    /// this module's (it is ticked here).
    /// The scripts' navmesh switches (EnableNavMesh, DisableNavmesh) and the game's door
    /// locks on the nav graph; actors whose way crossed a navmesh disabled or a door
    /// locked now plan again, and those that set a package aside as out of reach choose
    /// again when a door was unlocked (`fnv_ai::Ai::apply_navmesh_switches`,
    /// `apply_door_locks`).
    fn switch_navmeshes(&mut self, game: &mut Game, nav: &mut NavState) {
        use crate::profile::{ai_count, AiCount};
        ai_count(AiCount::NavPolls, 1);
        ai_count(AiCount::SwitchesPolled, game.nav_switches.navmeshes.len());
        let span = info_span!("ai::nav_switches").entered();
        let switched = self.ai.apply_navmesh_switches(game, &mut nav.graph);
        drop(span);
        if let Some(replanned) = switched {
            ai_count(AiCount::SwitchesChanged, 1);
            let off: Vec<String> = game.nav_switches.navmeshes.iter().filter(|(_, on)| !**on).map(|(m, _)| format!("{:08X}", m.0)).collect();
            let on = game.nav_switches.navmeshes.len() - off.len();
            info!("nav: navmeshes switched by scripts: {on} enabled, disabled [{}]; {} actors plan their way again", off.join(", "), replanned.len());
        }
        // Doors locked and unlocked since (scripts, keys, picked locks) reach the paths
        // (`fnv_ai::Ai::apply_door_locks`).
        crate::profile::ai_lock_serial(game.world.lock_serial());
        let span = info_span!("ai::door_locks").entered();
        let locks = self.ai.apply_door_locks(game, &mut nav.graph);
        drop(span);
        if let Some(again) = locks {
            ai_count(AiCount::LocksChanged, 1);
            let who: Vec<String> = again.iter().map(|a| format!("{:08X}", a.0)).collect();
            info!("nav: door locks changed; {} actors choose their package again [{}]", again.len(), who.join(", "));
        }
        // Doors enabled or disabled since open or close their ways: load doors (issue
        // #2566) and doors that open in place (#2405) (`fnv_ai::Ai::apply_door_presence`).
        let span = info_span!("ai::door_presence").entered();
        let doors = self.ai.apply_door_presence(game, &mut nav.graph);
        drop(span);
        if let Some(again) = doors {
            let who: Vec<String> = again.iter().map(|a| format!("{:08X}", a.0)).collect();
            info!("nav: doors enabled or disabled; {} not in the world now; {} actors choose their package again [{}]", nav.graph.disabled_doors.len(), again.len(), who.join(", "));
        }
        // A door that opens in place that closed someone's way has opened (#2405,
        // `fnv_ai::Ai::apply_doors_opened`): the packages set aside come back.
        if let Some(again) = self.ai.apply_doors_opened(game) {
            let who: Vec<String> = again.iter().map(|a| format!("{:08X}", a.0)).collect();
            info!("nav: a door that closed a way is open; {} actors choose their package again [{}]", again.len(), who.join(", "));
        }
        // Obstacles enabled or disabled since cut or free the navmesh under them
        // (`fnv_ai::Ai::apply_obstacles`, docs/spec-notes/navmesh.md §7).
        ai_count(AiCount::ObstaclesPolled, nav.graph.obstacles.len());
        let span = info_span!("ai::obstacles").entered();
        let obstacles = self.ai.apply_obstacles(game, &mut nav.graph);
        drop(span);
        if let Some((changed, again)) = obstacles {
            ai_count(AiCount::ObstaclesChanged, 1);
            let which: Vec<String> = changed.iter().map(|r| format!("{r:08X} {}", if nav.graph.obstacles.get(r).is_some_and(|o| o.enabled) { "on" } else { "off" })).collect();
            info!("nav: obstacles switched [{}]; {} triangles cut now; {} actors plan their way again", which.join(", "), nav.graph.cut_triangles(), again.len());
        }
    }

    /// Melee blows' knockbacks (docs/spec-notes/combat-rules.md, "Knockback"): each
    /// pushes its actor over this frame by its falling speed, through the same character
    /// as an attack clip's travel ([`Core::carry`]), so walls and actors stop it. Logged
    /// when it ends: how far the actor got of the distance asked ([`knock_run`]).
    fn knock(&mut self, game: &Game, phys: Option<&PhysState>, dt: f32) {
        // Alive here is alive and conscious: anything else stops its character.
        let alive = |id: u32| !game.extra.get(&FormId(id)).is_some_and(|e| e.dead || e.out_cold());
        let steps = phys.map_or(0, |p| p.world.steps());
        for (run, to) in std::mem::take(&mut self.knocks_over) {
            info!("{}", run.report(to, alive(run.k.form_id), steps));
        }
        let mut over = Vec::new();
        for (&id, run) in self.knocks.iter_mut() {
            let agent = self.ai.agents.get(&FormId(id));
            let feet = phys.and_then(|p| p.npcs.get(&id)).map(|b| b.ch.feet).or_else(|| agent.map(|a| a.walker.feet)).unwrap_or(run.from);
            if run.over() {
                if self.knock_trace {
                    info!("{}", run.trace_line(feet, 0.0, 0.0, steps));
                }
                over.push((id, feet));
                continue;
            }
            let walked = agent.is_some_and(|a| a.walker.moving) || self.travel.contains_key(&id);
            let d = run.advance(dt, walked);
            if self.knock_trace {
                info!("{}", run.trace_line(feet, dt, d, steps));
            }
            let t = self.knock_travel.entry(id).or_default();
            t[0] += run.k.dir[0] * d;
            t[1] += run.k.dir[1] * d;
        }
        for (id, to) in over {
            let Some(run) = self.knocks.remove(&id) else { continue };
            info!("{}", run.report(to, alive(id), steps));
        }
    }

    /// A melee blow's knockback on `k.form_id` begins, measured from where the actor's
    /// feet are now (one still going is replaced [design], and told as interrupted: its
    /// travel is not this blow's; one whose time was up is told as finished); not for the player
    /// (`nvf_play::player::Lunge::knock`) nor an actor that is not a resident.
    fn begin_knock(&mut self, k: nvf_play::combat::Knockback, phys: Option<&PhysState>) {
        let Some(a) = self.ai.agents.get(&FormId(k.form_id)) else { return };
        let feet = phys.and_then(|p| p.npcs.get(&k.form_id)).map_or(a.walker.feet, |b| b.ch.feet);
        if let Some(old) = knock_run::begin(&mut self.knocks, k, feet, phys.map_or(0, |p| p.world.steps())) {
            if old.over() {
                self.knocks_over.push((old, feet));
            } else {
                info!("{}", old.interrupted(&k, feet));
            }
        }
        self.knocked.0 += 1;
        self.knocked.1 += k.distance();
        info!("knockback: {} knocked back at {:.0} units/s for {:.2} s ({:.3} units)", knock_run::KnockRun::tag(&k), k.speed, k.secs, k.distance());
    }

    /// Attack clips that travel carry their actors (docs/spec-notes/actors.md §8.13):
    /// each one carried gets a character, if it has none, moved at this frame's travel
    /// over the frame, so walls, the player and other actors stop it as they stop a walk
    /// (`nvf_play::player`), and while its clip plays its walk waits. Where its character
    /// got to is where its walker stands (one walking reads it there itself), so it stays
    /// where the clip ended, or was stopped. Its character goes once the clip is over and
    /// it is not on its way.
    fn carry(&mut self, game: &mut Game, phys: Option<&mut PhysState>, dt: f32) {
        let mut travel = std::mem::take(&mut self.travel);
        let pushes = std::mem::take(&mut self.knock_travel);
        let Some(phys) = phys else {
            self.carried.clear();
            return;
        };
        let last = std::mem::take(&mut self.carried);
        for (&id, &(from, asked)) in &last {
            let Some(feet) = phys.npcs.get(&id).map(|b| b.ch.feet) else { continue };
            if let Some(a) = self.ai.agents.get_mut(&FormId(id)).filter(|a| a.activity != Activity::Travelling) {
                a.walker.feet = feet;
                a.walker.tri = fnv_nav::graph::NONE;
                let place = fnv_nav::path::Place { space: a.walker.space, pos: feet };
                let cell = fnv_ai::goal::cell_of_place(game.world.placed, &place);
                let _ = game.set_actor_place(FormId(id), cell, feet, a.walker.heading);
            }
            if !travel.contains_key(&id) {
                let _ = stop_body(Some(&mut *phys), id);
                info!(
                    "ai [{}] {} {id:08X}: its attack clip carried it {:.0} units of {asked:.0}, from ({:.0}, {:.0}, {:.0}) to ({:.0}, {:.0}, {:.0})",
                    self.clock_text,
                    game.data.name(FormId(id)),
                    (feet[0] - from[0]).hypot(feet[1] - from[1]),
                    from[0],
                    from[1],
                    from[2],
                    feet[0],
                    feet[1],
                    feet[2]
                );
            }
        }
        self.carry_broken.retain(|id| travel.contains_key(id));
        // A hit reaction (a crippling hit's stagger) stops the lunge where it is and ends
        // its carry: the stagger's hold would not stop a velocity set here [design: review
        // of #1209]. A knockback still pushes the staggering actor (#1070).
        let broken: Vec<u32> = travel.keys().copied().filter(|id| self.staggering.contains(id) || self.carry_broken.contains(id)).collect();
        for id in broken {
            travel.remove(&id);
            if self.carry_broken.insert(id) {
                if !pushes.contains_key(&id) {
                    let _ = stop_body(Some(&mut *phys), id);
                }
                info!("ai [{}] {} {id:08X}: a hit reaction ends its attack clip's carry", self.clock_text, game.data.name(FormId(id)));
            }
            self.carries[1] += 1;
        }
        // A knockback on a walking actor adds to its walk (the walk set the velocity this
        // frame and follows its character), so a punched actor still closes in or gets
        // away [design: review of #1219]; on one standing, or carried by a clip, it is
        // carried as the clip's travel is.
        let clips: Vec<u32> = travel.keys().copied().collect();
        for (id, p) in pushes {
            // On furniture its body stays in the seat: not pushed [design: review of #1219].
            if self.on_furniture.contains(&id) {
                continue;
            }
            let walking = !clips.contains(&id) && self.ai.agents.get(&FormId(id)).is_some_and(|a| a.walker.moving);
            if let Some(b) = phys.npcs.get_mut(&id).filter(|_| walking && dt > 0.0) {
                b.velocity = [b.velocity[0] + p[0] / dt, b.velocity[1] + p[1] / dt];
                continue;
            }
            let t = travel.entry(id).or_default();
            t[0] += p[0];
            t[1] += p[1];
        }
        for (id, by) in travel {
            let carried = self.ai.agents.get(&FormId(id)).filter(|_| !game.extra.get(&FormId(id)).is_some_and(|e| e.dead || e.restrained || e.out_cold())).is_some_and(|a| {
                in_loaded_space(&a.walker, phys) && phys.ready_at(a.walker.feet) && ensure_body(phys, &mut self.unstuck, &mut self.skip_refusals, id, a.walker.feet)
            });
            let Some(b) = phys.npcs.get_mut(&id).filter(|_| carried && dt > 0.0) else {
                self.carries[1] += 1;
                continue;
            };
            b.velocity = [by[0] / dt, by[1] / dt];
            let (from, asked) = last.get(&id).copied().unwrap_or((b.ch.feet, 0.0));
            self.carried.insert(id, (from, asked + by[0].hypot(by[1])));
            self.carries[0] += 1;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn think(&mut self, game: &mut Game, own: bool, restored: bool, nav: &NavState, world: &WorldData, doors: &mut nvf_play::doors::Doors, mut phys: Option<&mut PhysState>, mut blocks: Option<&mut ActorBlocks>, dt: f32, now: f32, waited: u32) {
        crate::profile::ai_count(crate::profile::AiCount::Think, 1);
        // Who a walker pushed since the last frame, and (below) who may be pushed in
        // the next: nobody while the game is held.
        let pushed = blocks.as_deref_mut().map(|b| {
            b.standing.clear();
            b.take_pushes()
        });
        // Whoever is on furniture, or getting off it, does not walk yet; nor do two actors
        // talking to each other (`fnv_ai::social`), unless the script that started their
        // conversation allowed movement (`Game::conversation_moves`); nor one saying its
        // Goodbye greeting at the player (`Game::greets_player`).
        let mut held: HashSet<u32> = self
            .list
            .iter()
            .filter(|r| {
                matches!(r.seat, Seat::Ordered(..) | Seat::GettingUp(_))
                    || held_by_talk(game, &self.ai, r.actor)
                    || self.staggering.contains(&r.actor.0)
                    || self.drawing.contains(&r.actor.0)
            })
            .map(|r| r.actor.0)
            .collect();
        // One whose legs an idle poses (idles.md §15) stands while it shows; when its AI
        // has somewhere to go (nothing else holds it and it travels: its package walks on,
        // a patrol wait ended, it follows or flees, a conversation that moves) it breaks
        // out of the idle, which fades out (`idles.rs`), and walks once it has gone
        // [design: owner 2026-10-31].
        self.breaking = self.posed.iter().copied().filter(|id| !held.contains(id) && self.ai.agents.get(&FormId(*id)).is_some_and(|a| a.activity == Activity::Travelling)).collect();
        held.extend(self.posed.iter().copied());
        // The victim of a planted explosive stands where it is from its panic to the blast
        // (`GetPlantedExplosive`; docs/spec-notes/pickpocket.md §5) [hypothesis: the issue
        // reads "slows or stops"; stopping is the plainer of the two].
        let panicking: Vec<u32> = if game.pickpocket.planted.is_empty() {
            Vec::new()
        } else {
            let actors: Vec<FormId> = self.list.iter().map(|r| r.actor).collect();
            actors.into_iter().filter(|&a| game.has_planted_explosive(a)).map(|a| a.0).collect()
        };
        held.extend(panicking.iter().copied());
        self.prepare(game, restored);
        // A menu, a conversation or a loading screen holds the game: nobody moves.
        self.held_now = game.menu.is_some() || game.loading || game.dialogue.is_some();
        if self.held_now {
            for b in phys.iter_mut().flat_map(|p| p.npcs.values_mut()) {
                b.velocity = [0.0, 0.0];
            }
            // Hours passed in the Wait menu: the residents go on with their days.
            if waited > 0 && game.menu == Some(nvf_play::script::MENU_SLEEP_WAIT) && !self.ai.agents.is_empty() {
                self.pass_hours(game, nav, world, waited, phys.as_deref_mut(), &held, now);
            }
            // Who started the conversation with its Dialogue package goes on turning
            // to the player.
            self.ai.update_held(game, dt);
            // Enabled or disabled by a script the held game ran (a terminal's result, a
            // dialogue result): drawn or taken away now, not when the game goes on.
            self.note_presence(game);
            return;
        }
        {
            let _span = info_span!("ai::join").entered();
            self.join(game);
        }
        if own {
            // The game being played advances its own calendar (`nvf_play::sim`) and
            // ticks itself; this module's game state is moved on here.
            if let Err(e) = game.world.advance_hours(self.clock.rate * dt as f64) {
                warn!("ai: the clock cannot advance: {e}");
            }
            game.cfg.frame_seconds = dt as f64;
            game.tick();
        }
        self.clock_text = clock_text(game);
        // Standing actors give way to who walked into them, where they have room.
        for (id, by) in pushed.into_iter().flatten() {
            let from = self.ai.agents.get(&FormId(id)).map_or([0.0; 3], |a| a.walker.feet);
            let moved = self.ai.push(game, &nav.graph, FormId(id), by);
            let no_room = moved < NO_ROOM_SHARE * by[0].hypot(by[1]);
            self.pushes[0] += 1;
            self.pushed_units += moved;
            if no_room {
                self.pushes[1] += 1;
                self.no_room.insert(id, now + NO_ROOM_SECONDS);
            }
            let p = self.pushing.entry(id).or_insert(Pushed { from, units: 0.0, no_room: 0, last: now });
            p.units += moved;
            p.no_room += usize::from(no_room);
            p.last = now;
        }
        // Pushes that are over, for the log.
        let over: Vec<u32> = self.pushing.iter().filter(|(_, p)| now - p.last >= PUSH_OVER_SECONDS).map(|(id, _)| *id).collect();
        for id in over {
            let p = self.pushing.remove(&id).expect("listed");
            let (Some(r), Some(a)) = (self.list.iter().find(|r| r.actor.0 == id), self.ai.agents.get(&FormId(id))) else { continue };
            let to = a.walker.feet;
            info!(
                "ai [{}] {}: was pushed aside {:.0} units, from ({:.0}, {:.0}, {:.0}) to ({:.0}, {:.0}, {:.0}){}",
                self.clock_text,
                r.label,
                p.units,
                p.from[0],
                p.from[1],
                p.from[2],
                to[0],
                to[1],
                to[2],
                if p.no_room > 0 { format!("; no room in {} updates", p.no_room) } else { String::new() }
            );
        }
        let mut moved = [0usize; 6];
        let searching: HashSet<u32> = self.ai.agents.iter().filter(|(_, a)| a.search.is_some()).map(|(f, _)| f.0).collect();
        let creatures_fighting: HashSet<u32> = self.list.iter().filter(|r| r.creature && game.awareness.get(r.actor).is_some_and(|x| x.state == fnv_game::awareness::State::Combat)).map(|r| r.actor.0).collect();
        let mut mover = PhysMover { doors, phys: phys.as_deref_mut(), unstuck: &mut self.unstuck, skip_until: &mut self.skip_until, hopped: &mut self.hopped, barred: &mut self.barred, now, stats: &mut moved, held: &held, refusals: &mut self.skip_refusals, coasting: &mut self.coasting, fighting: &self.aims, searching: &searching, creatures_fighting: &creatures_fighting };
        {
            // Every high-process actor's update and the low-process ones due (package,
            // goal and walk; path searches are `nav::find_path` inside).
            let _span = info_span!("ai::update_moved").entered();
            self.ai.update_moved(game, &nav.graph, dt, &mut mover);
        }
        if let Some(mut probe) = self.sandbox_interrupt.take() {
            probe.tick(self, game, phys.as_deref());
            self.sandbox_interrupt = Some(probe);
        }
        if let Some(mut probe) = self.guard_wander.take() {
            probe.tick(self, game);
            self.guard_wander = Some(probe);
        }
        // Knockbacks push their actors, and attack clips that travel carry theirs, over
        // their walks.
        {
            let _span = info_span!("ai::knock_carry").entered();
            self.knock(game, phys.as_deref(), dt);
            self.carry(game, phys.as_deref_mut(), dt);
        }
        if let Some(b) = blocks.as_deref_mut().filter(|b| b.carried.len() != self.carried.len() || !b.carried.iter().all(|id| self.carried.contains_key(id))) {
            b.carried = self.carried.keys().copied().collect();
        }
        // Walks with the AI off that ended in this update (the AI back on, or nothing
        // left to carry the actor on), for the log.
        let ended: Vec<u32> = self.coasting.iter().filter(|(_, c)| !c.now).map(|(id, _)| *id).collect();
        for id in ended {
            let c = self.coasting.remove(&id).expect("listed");
            let Some(a) = self.ai.agents.get(&FormId(id)) else { continue };
            let to = a.walker.feet;
            let why = if game.ai_off(FormId(id)) { "stopped" } else { "AI back on" };
            info!(
                "actor {id:08X} AI off walk ended ({why}): {:.0} units straight from ({:.0}, {:.0}, {:.0}) to ({:.0}, {:.0}, {:.0}), heading {:.0} deg kept, {:.0} units/s",
                (to[0] - c.from[0]).hypot(to[1] - c.from[1]),
                c.from[0],
                c.from[1],
                c.from[2],
                to[0],
                to[1],
                to[2],
                c.heading.to_degrees().rem_euclid(360.0),
                c.speed
            );
        }
        for c in self.coasting.values_mut() {
            c.now = false;
        }
        self.physics[0] += moved[0];
        self.physics[1] += moved[1];
        self.physics[2] = self.physics[2].max(moved[0]);
        self.physics[3] += moved[2];
        for k in 0..3 {
            self.jumps[k] += moved[3 + k];
        }
        // Nobody stands inside anybody else (fnv_ai::apart): those the character
        // controller walks, and those on furniture, are kept clear of but not moved.
        let mut fixed = held.clone();
        fixed.extend(phys.as_deref().into_iter().flat_map(|p| p.npcs.keys().copied()));
        {
            let _span = info_span!("ai::keep_apart").entered();
            self.ai.keep_apart(game, &nav.graph, dt, &fixed);
        }
        // Hellos to the player and random conversations.
        {
            let _span = info_span!("ai::social").entered();
            self.ai.social(game, &nav.graph, dt);
        }
        // No character is kept for an actor that is not on its way, nor for one that is
        // dead, restrained or out cold: the AI no longer walks it (`fnv_ai`), so its
        // character would go on at the velocity of its last step, the body drawn gliding
        // along with it and its block pushing the player (playtest list 7 item 14).
        if let Some(p) = phys {
            let held = |id: FormId| game.extra.get(&id).is_some_and(|e| e.dead || e.restrained || e.out_cold());
            for (&id, b) in p.npcs.iter().filter(|(id, _)| held(FormId(**id))) {
                let f = b.ch.feet;
                info!("ai [{}] {} {id:08X}: its character stops (dead, restrained or out cold) at ({:.0}, {:.0}, {:.0}), walking at {:.0} units/s", self.clock_text, game.data.name(FormId(id)), f[0], f[1], f[2], b.velocity[0].hypot(b.velocity[1]));
            }
            let idle: Vec<u32> = p.npcs.keys().copied().filter(|id| held(FormId(*id)) || (!self.carried.contains_key(id) && self.ai.agents.get(&FormId(*id)).is_none_or(|a| a.activity != Activity::Travelling))).collect();
            for id in idle {
                p.remove_npc(id);
                self.unstuck.remove(&id);
                self.skip_refusals.forget(id);
            }
            // Nor does one go on while it staggers, whether or not the AI walked it this
            // frame.
            // (A knockback or an attack clip moving it goes on.)
            for &id in self.staggering.iter().filter(|id| !self.carried.contains_key(id)) {
                let was = stop_body(Some(&mut *p), id);
                if was > 0.0 {
                    info!("ai [{}] {} {id:08X}: its character stops while it staggers (was walking at {was:.0} units/s)", self.clock_text, game.data.name(FormId(id)));
                }
            }
            // Nor a creature while it draws its stance (the ghoul's roar, #2686).
            for &id in self.drawing.iter().filter(|id| !self.carried.contains_key(id)) {
                let was = stop_body(Some(&mut *p), id);
                if was > 0.0 {
                    info!("ai [{}] {} {id:08X}: its character stops while it draws its stance (was walking at {was:.0} units/s)", self.clock_text, game.data.name(FormId(id)));
                }
            }
            // Nor while an idle holds it in place (idles.md §15).
            for &id in self.posed.iter().filter(|id| !self.carried.contains_key(id)) {
                let was = stop_body(Some(&mut *p), id);
                if was > 0.0 {
                    info!("ai [{}] {} {id:08X}: its character stops while an idle holds it (was walking at {was:.0} units/s)", self.clock_text, game.data.name(FormId(id)));
                }
            }
            // Nor one panicking with a live explosive in its pocket.
            for &id in panicking.iter().filter(|id| !self.carried.contains_key(id)) {
                let was = stop_body(Some(&mut *p), id);
                if was > 0.0 {
                    info!("ai [{}] {} {id:08X}: its character stops: a live explosive in its pocket (was walking at {was:.0} units/s)", self.clock_text, game.data.name(FormId(id)));
                }
            }
        }
        if own {
            game.events.clear();
        }
        // Packages' door flags (`fnv_game::ai`).
        for (actor, package, locked, n) in std::mem::take(&mut game.package_door_notes) {
            info!("ai [{}] {}: {} {n} doors of its cell ({})", self.clock_text, game.data.name(actor), if locked { "locks" } else { "unlocks" }, game.data.name(package));
        }
        // The events told, the fighters' aims, and the notes (to the end).
        let _tell_span = info_span!("ai::tell_note").entered();
        self.tell(game, world);
        // Fighters face their target, on the move as well as standing: they strafe and
        // back away instead of turning to where they go (owner 2026-10-16: NCR troopers
        // fighting Fiends turned between their way and their target, as if they could
        // not decide whether to move or shoot) [design]. Not while fleeing (it runs
        // away). Going to new cover it strafes too, keeping its target in front (owner's
        // playtest list 7 item 43: Violet turned to face the cover she ran to; before,
        // a fighter turned to its way to cover); out to its peek point and back behind
        // its cover it strafes (`PostKind::Duck`).
        self.aims.clear();
        self.use_aims.clear();
        self.to_cover.clear();
        for r in self.list.iter().filter(|r| !r.creature) {
            let Some(a) = self.ai.agents.get(&r.actor) else { continue };
            let tactics = &game.awareness.tactics;
            if tactics.flights.contains_key(&r.actor) {
                continue;
            }
            let aw = game.awareness.get(r.actor).filter(|x| x.state == fnv_game::awareness::State::Combat);
            // Just gone into Combat it stands a moment on its heading (`fnv_ai::alert::reacting`;
            // nor is it locked on its target then, `npc_combat`).
            if fnv_ai::alert::reacting(game, r.actor) {
                continue;
            }
            let Some(aw) = aw else { continue };
            let active = game.active_weapon_use(r.actor);
            let use_target = use_aim(active.map(|w| w.target), active.is_some_and(|w| w.taken));
            let Some(target) = aim_target(use_target, aw.combat_target().map(|(t, _)| t)) else { continue };
            let at = if use_target.is_some() {
                fnv_ai::goal::weapon_target_place(game, target, a.walker.space)
            } else {
                fnv_ai::goal::place_of_ref(game, target).filter(|p| p.space == a.walker.space)
            };
            if let Some(p) = at {
                self.aims.insert(r.actor, p.pos);
                if let Some(t) = use_target {
                    self.use_aims.insert(r.actor, t);
                }
                if tactics.posts.get(&r.actor).is_some_and(|p| p.kind == fnv_game::combat_group::PostKind::Cover) {
                    self.to_cover.insert(r.actor);
                }
            }
        }
        self.no_room.retain(|_, until| now < *until);
        self.note(game, blocks, &held, now);
    }

    /// The first think with a game: the package interval, and the start clock (`--hour`,
    /// `--day`, which reach a new game only here). A game `restored` from a save has the
    /// save's clock and keeps it (#1607).
    fn prepare(&mut self, game: &mut Game, restored: bool) {
        if self.prepared {
            return;
        }
        self.prepared = true;
        game.cfg.package_interval = Some(PACKAGE_INTERVAL);
        // The seats the save had NPCs on: back on at once, no walk and no getting-on
        // clip, when the body is drawn and its package still wants that furniture (#3991,
        // observer check 220). Residents already known take theirs now, the rest when
        // they join (`Core::add_resident`).
        self.saved_seats = game.take_saved_seats().into_iter().map(|(a, f)| (a.0, f.0)).collect();
        if !self.saved_seats.is_empty() {
            info!("ai: {} NPC(s) the save had seated go back on their furniture at once", self.saved_seats.len());
        }
        for r in &mut self.list {
            if let Some(f) = self.saved_seats.remove(&r.actor.0) {
                r.seat = Seat::Wanted(f, true);
            }
        }
        if !restored {
            for (edid, v) in [(clock::GAME_HOUR, self.clock.hour.rem_euclid(24.0)), (clock::GAME_DAYS_PASSED, self.clock.days_passed)] {
                if let Err(e) = set_global(game, edid, v) {
                    warn!("ai: {e}");
                }
            }
        }
        if let Ok(c) = game.world.calendar() {
            let whose = if restored { "the save's" } else { "the start clock's" };
            info!("ai: the game's clock is {whose}: GameHour {:.3}, GameDaysPassed {:.3}", c.hour, c.days_passed);
        }
    }

    /// `hours` passed in the Wait menu (the calendar is already there): for each, every
    /// actor chooses its package and walks for as long as an hour of game time lasts in
    /// play, 3600 seconds over the TimeScale global (two real minutes at 30), on the
    /// navmesh (`fnv_ai::Ai::pass_time`). Nobody is left to the character controller:
    /// each goes on from where this put it.
    #[allow(clippy::too_many_arguments)]
    fn pass_hours(&mut self, game: &mut Game, nav: &NavState, world: &WorldData, hours: u32, phys: Option<&mut PhysState>, held: &HashSet<u32>, now: f32) {
        let scale = game.world.forms.global_by_edid(clock::TIME_SCALE).map(|(id, authored)| game.world.global(id).unwrap_or(authored)).filter(|s| *s > 0.0).unwrap_or(30.0);
        for _ in 0..hours {
            game.choose_packages_now();
            let walked = self.ai.pass_time(game, &nav.graph, 3600.0 / scale, WAITED_STEP);
            self.waited[0] += 1;
            self.waited[1] += walked;
            info!("ai [{}] an hour passed in the wait menu: {walked} of {} residents walked", clock_text(game), self.ai.agents.len());
            self.tell(game, world);
        }
        if let Some(p) = phys {
            let ids: Vec<u32> = p.npcs.keys().copied().collect();
            for id in ids {
                p.remove_npc(id);
            }
        }
        self.unstuck.clear();
        self.skip_until.clear();
        self.skip_refusals.rounds.clear();
        self.clock_text = clock_text(game);
        self.note(game, None, held, now);
    }

    /// Count and log what the AI did since the last call.
    fn tell(&mut self, game: &Game, world: &WorldData) {
        let hour = clock_text(game);
        for e in &self.ai.events {
            // For anyone, not only those told in full; once per actor and piece.
            if let AiEvent::FurnitureEligibility { furniture, decision, reason } = e.event {
                info!("ai furniture eligibility: actor={:08X} furniture={:08X} decision={decision} reason={reason}", e.actor.0, furniture.0);
                continue;
            }
            match e.event {
                AiEvent::Trip { .. } => self.counts[0] += 1,
                AiEvent::Arrived { .. } => self.counts[1] += 1,
                AiEvent::Unreachable { .. } => self.counts[2] += 1,
                AiEvent::Package { .. } => self.counts[3] += 1,
                AiEvent::Talk { .. } => self.talks += 1,
                AiEvent::Hello { .. } => self.social[0] += 1,
                AiEvent::Chat { .. } => self.social[1] += 1,
                AiEvent::ChatApproach { .. } => self.social[2] += 1,
                AiEvent::ChatGaveUp { .. } => self.social[3] += 1,
                AiEvent::Door { door, to_door } => self.door_uses.push((e.actor, door.0, to_door.0)),
                AiEvent::Sandbox { doing, .. } => {
                    self.sandbox[match doing {
                        Doing::Standing => 0,
                        Doing::Wandering => 1,
                        Doing::Sitting(_) => 2,
                        Doing::AtIdleMarker(_) => 3,
                        Doing::Sleeping(_) => 4,
                        Doing::Eating(_) => 5,
                    }] += 1
                }
                _ => {}
            }
            let Some(r) = self.list.iter_mut().find(|r| r.actor == e.actor) else { continue };
            // Told before `note`, which takes a wait that ended unannounced for a passage.
            if matches!(e.event, AiEvent::DoorWaitCancelled { .. }) {
                r.waiting = None;
            }
            // Named residents in full; the crowd of `all` only when it sets off or fails.
            let line = format!("ai [{hour}] {}: {}", r.label, describe(game.data, world, &e.event));
            match e.event {
                // The player's companions in full too (`fnv_game::followers`).
                _ if r.named || game.is_teammate(e.actor) => info!("{line}"),
                AiEvent::Trip { .. }
                | AiEvent::Unreachable { .. }
                | AiEvent::PutThere { .. }
                | AiEvent::DoorsClosedToIt { .. }
                | AiEvent::HeightWaived { .. }
                | AiEvent::DoorRefused { .. }
                | AiEvent::DoorWaitCancelled { .. }
                | AiEvent::Resumed { .. }
                | AiEvent::Approach { .. }
                | AiEvent::Follow { .. }
                | AiEvent::Talk { .. }
                | AiEvent::NoTalk { .. }
                | AiEvent::TalkUnseen { .. }
                | AiEvent::LeadsPlayer { .. }
                | AiEvent::LeadStuck { .. }
                | AiEvent::WalkUp { .. }
                | AiEvent::WalkedUp { .. }
                | AiEvent::PushedBack { .. }
                | AiEvent::Hello { .. }
                | AiEvent::IdleChatter { .. }
                | AiEvent::PlayerAction { .. }
                | AiEvent::ObserveCombat { .. }
                | AiEvent::SpectatorFlee { .. }
                | AiEvent::Chat { .. }
                | AiEvent::ChatApproach { .. }
                | AiEvent::ChatGaveUp { .. }
                | AiEvent::ChatBack { .. }
                | AiEvent::LookTurn { .. }
                | AiEvent::LookFaced { .. }
                | AiEvent::LookHeld { .. }
                | AiEvent::LookOver { .. }
                | AiEvent::Search { .. }
                | AiEvent::Hold { .. }
                | AiEvent::Warn { .. }
                | AiEvent::Close { .. }
                | AiEvent::Pace { .. }
                | AiEvent::Post { .. }
                | AiEvent::NoPost { .. }
                | AiEvent::PostFailed { .. }
                | AiEvent::Flee { .. }
                | AiEvent::FleeArrived { .. }
                | AiEvent::Cornered { .. }
                | AiEvent::SearchOver { .. }
                | AiEvent::PackageHolds { .. }
                | AiEvent::SearchBack { .. }
                | AiEvent::Resettles { .. }
                | AiEvent::TakesUpAgain { .. }
                | AiEvent::BackHome { .. }
                | AiEvent::BackUp { .. }
                | AiEvent::WeaponUse { .. }
                | AiEvent::WeaponUseOver
                | AiEvent::EscortWaits { .. }
                | AiEvent::EscortOn { .. }
                | AiEvent::FindGoes { .. }
                | AiEvent::Found { .. }
                | AiEvent::Bought { .. }
                | AiEvent::NothingFound { .. }
                | AiEvent::UsesAt { .. }
                | AiEvent::UsesItem { .. }
                | AiEvent::AmbushWaits { .. }
                | AiEvent::AmbushSprung { .. }
                | AiEvent::FleeRuns { .. }
                | AiEvent::Rearmed { .. }
                | AiEvent::Fetch { .. }
                | AiEvent::FetchGot { .. }
                | AiEvent::FetchDropped { .. }
                | AiEvent::RangeSwap { .. }
                | AiEvent::RangeFists { .. }
                | AiEvent::FleeSafe { .. } => info!("{line}"),
                _ => debug!("{line}"),
            }
        }
        self.ai.events.clear();
    }

    /// A co-op guest's frame in place of [`Core::think`] (`Residents::remote`): nobody
    /// thinks; each actor the host draws is put where the host has it, with no physics
    /// body of its own here to walk it anywhere else. The game is told that place too:
    /// what is measured from the actor (where its lines are heard, a script's
    /// GetDistance) is measured from where it is drawn [observed: buckallobill, in game,
    /// FNV 1.4.0.525: with only the body moved, a guest heard the host's NPCs from where
    /// its own game last had them].
    fn follow(&mut self, game: &mut Game, remote: &HashMap<u32, RemotePose>, mut phys: Option<&mut PhysState>, blocks: Option<&mut ActorBlocks>, now: f32) {
        self.join(game);
        for (&id, pose) in remote {
            let Some(a) = self.ai.agents.get_mut(&FormId(id)) else { continue };
            if pose.space != 0 {
                let space = if game.world.placed.world_grid.contains_key(&FormId(pose.space)) { Space::World(pose.space) } else { Space::Interior(pose.space) };
                if a.walker.space != space {
                    info!("ai [{}] {id:#010X}: in {:#010X} in the host's world: drawn there only", clock_text(game), pose.space);
                    a.walker.space = space;
                }
            }
            a.walker.feet = pose.feet;
            a.walker.heading = pose.heading;
            a.walker.moving = pose.moving;
            a.walker.turning = false;
            a.walker.air = None;
            a.walker.tri = fnv_nav::graph::NONE;
            a.activity = pose.activity();
            let place = fnv_nav::path::Place { space: a.walker.space, pos: pose.feet };
            // Its cell as the walker's space gives it, or the one it has (no cell found).
            let cell = fnv_ai::goal::cell_of_place(game.world.placed, &place).or_else(|| game.world.peek(FormId(id)).and_then(|s| s.cell));
            let _ = game.set_actor_place(FormId(id), cell, pose.feet, pose.heading);
            // On the furniture the host has it on: the drawing sits it there
            // ([`Core::note`] makes it the one it wants), at once when it is on already.
            a.using = pose.furniture.map(|(f, _)| FormId(f));
            if let (Some((f, on)), Some(r)) = (pose.furniture, self.list.iter_mut().find(|r| r.actor.0 == id)) {
                if r.seat == Seat::None {
                    r.seat = Seat::Wanted(f, on);
                }
            }
            if let Some(p) = phys.as_deref_mut() {
                p.remove_npc(id);
            }
        }
        self.note(game, blocks, &HashSet::new(), now);
    }

    /// Whether each resident is in the world and enabled, as the game has it now. Also
    /// while a menu or a conversation holds the game ([`Core::think`]): a script run there
    /// (a terminal's result script, a dialogue result) that enables an actor disabled
    /// when its cell loaded has it drawn at once, so a conversation the same script starts
    /// shows it speaking; until #2616 the presence was only read after the AI's update,
    /// which a held game skips, so the hatch terminal's Centurion at Hoover Dam was never
    /// drawn through its conversation (docs/spec-notes/enable-disabled-actor.md).
    fn note_presence(&mut self, game: &Game) {
        for r in self.list.iter_mut() {
            r.present = r.joined && !r.failed && game.present(r.actor);
            if r.joined && !r.failed && !r.present {
                r.absent = true;
            } else if r.present && std::mem::take(&mut r.absent) {
                r.first_steps = Some(FirstSteps::new("being enabled"));
            }
        }
    }

    /// Each resident's state for whoever draws and seats it, after the AI's update.
    fn note(&mut self, game: &Game, mut blocks: Option<&mut ActorBlocks>, held: &HashSet<u32>, now: f32) {
        let hour = clock_text(game);
        self.note_presence(game);
        for r in self.list.iter_mut() {
            let Some(agent) = self.ai.agents.get(&r.actor) else { continue };
            // A perched bird is in nobody's way (actor-behaviour.md §5.8).
            if let (Some(b), true) = (blocks.as_deref_mut(), r.perched) {
                b.in_nobodys_way.insert(r.actor.0);
            }
            // Drawn and standing: a walker pushes it aside (`nvf_play::blocking`).
            if let Some(b) = blocks.as_deref_mut() {
                if r.present && r.spawned && r.seat == Seat::None && !held.contains(&r.actor.0) && !self.no_room.contains_key(&r.actor.0) && self.ai.stands(game, r.actor) {
                    b.standing.insert(r.actor.0, [agent.walker.feet[0], agent.walker.feet[1]]);
                }
            }
            // The furniture it is to use, once it stands there.
            r.wants = agent.using.filter(|_| agent.activity == Activity::There).map(|f| f.0);
            if agent.restarts != r.restarts {
                r.restarts = agent.restarts;
                r.restart = true;
            }
            // `held` dates from before this frame's `social`: a talk begun since counts too.
            r.may_ask_again = matches!(r.seat, Seat::Waiting(..)) && !held.contains(&r.actor.0) && !held_by_talk(game, &self.ai, r.actor) && !game.ai_off(r.actor) && !game.extra.get(&r.actor).is_some_and(|e| e.dead || e.out_cold());
            let still = agent.activity != Activity::Travelling && agent.walker.follower.is_none() && agent.using.is_none() && agent.talk.is_none();
            r.stand = still.then(|| match agent.sandbox.as_ref().map(|s| s.doing) {
                Some(Doing::AtIdleMarker(m)) => game.world.placed.get(m).map(|p| p.base),
                _ => None,
            });
            // Doors that open in place: when it starts waiting and when it walks on.
            match (agent.waiting_at, r.waiting) {
                (Some(d), w) if w.is_none_or(|w| w.0 != d) => {
                    info!("ai [{hour}] {}: opens door {d:08X} and waits", r.label);
                    r.waiting = Some((d, now));
                }
                (None, Some((d, since))) => {
                    info!("ai [{hour}] {}: walks through door {d:08X} after waiting {:.2}s", r.label, now - since);
                    r.doors_opened += 1;
                    r.waiting = None;
                }
                _ => {}
            }
        }
    }
}

/// The residents' package progress in saves (`fnv_ai::progress`; docs/design/save-load.md
/// §3.3): patrol points and sandbox days, put back as each actor takes up its package
/// again after the load.
pub const SAVE_HOOK: nvf_play::save::SaveHook = nvf_play::save::SaveHook {
    key: "ai_progress",
    save: |w| serde_json::to_value(w.get_resource::<Residents>()?.core.ai.progress()).ok(),
    load: |w, v| {
        let Some(mut r) = w.get_resource_mut::<Residents>() else { return };
        match serde_json::from_value::<Vec<fnv_ai::progress::Progress>>(v.clone()) {
            Ok(p) => {
                info!("load: package progress of {} residents kept", p.len());
                r.core.ai.restore(p);
            }
            Err(e) => warn!("load: the residents' package progress could not be read: {e}"),
        }
    },
    // Actors and packages by FormID (hex text): moved to the current load order, an entry
    // naming a plugin no longer loaded dropped.
    remap: Some(|v, r| {
        let Some(a) = v.as_array_mut() else { return };
        let map = |x: &serde_json::Value| x.as_str().and_then(|s| s.parse::<FormId>().ok()).and_then(|f| r.map(f.0)).map(|n| serde_json::Value::from(FormId(n).to_string()));
        a.retain_mut(|e| {
            let (Some(actor), Some(package)) = (e.get("actor").and_then(map), e.get("package").and_then(map)) else { return false };
            e["actor"] = actor;
            e["package"] = package;
            true
        });
    }),
};

pub fn log_startup(mut res: ResMut<Residents>) {
    for (ok, line) in std::mem::take(&mut res.startup) {
        if ok {
            info!("{line}");
        } else {
            warn!("{line}");
        }
    }
}

/// One line for the log.
fn describe(data: &GameData, world: &WorldData, e: &AiEvent) -> String {
    let n = |f: &FormId| data.name(*f);
    match e {
        AiEvent::Package { package: Some(p) } => format!("package {}", n(p)),
        AiEvent::Package { package: None } => "no package".into(),
        AiEvent::Trip { to, space, length, doors, running, patrol_point, .. } => format!(
            "{} to ({:.0}, {:.0}, {:.0}) in {}{}: {length:.0} units, {} doors",
            if *running { "runs" } else { "walks" },
            to[0],
            to[1],
            to[2],
            nav::space_name(world, *space),
            patrol_point.map(|p| format!(" (patrol point {})", n(&p))).unwrap_or_default(),
            doors.len()
        ),
        AiEvent::AlreadyThere { .. } => "is where its package wants it".into(),
        AiEvent::PushedBack { package, away } => format!("was pushed {away:.0} units from the place of {}: goes back", n(package)),
        AiEvent::Door { door, .. } => format!("used door {door}"),
        AiEvent::DoorRefused { door } => format!("was not let through door {door} by its script: plans again"),
        AiEvent::DoorWaitCancelled { door } => format!("stops waiting at door {door}: its walk was given up"),
        AiEvent::Resumed { package, patrol_point: Some(p), .. } => format!("goes on with {} at patrol point {} (from the save)", n(package), n(p)),
        AiEvent::Resumed { package, .. } => format!("goes on with {}, its sandbox day as saved", n(package)),
        // [design, #2532] Identify the reached patrol point for scene smoke diagnostics.
        AiEvent::Arrived { seconds, travelled, patrol_point: Some(point), .. } => format!("arrived after {seconds:.1}s, {travelled:.0} units (patrol point {})", n(point)),
        AiEvent::Arrived { seconds, travelled, .. } => format!("arrived after {seconds:.1}s, {travelled:.0} units"),
        AiEvent::ArrivalFacing { heading } => format!("faces {heading:.4} on arrival"),
        AiEvent::Unreachable { package, why } => format!("cannot reach the place of {}: {why}", n(package)),
        AiEvent::PutThere { package, from, to } => format!(
            "put at the place of {} out of sight, no door joining the two: ({:.0}, {:.0}, {:.0}) in {}, from ({:.0}, {:.0}, {:.0}) in {}",
            n(package),
            to.pos[0],
            to.pos[1],
            to.pos[2],
            nav::space_name(world, to.space),
            from.pos[0],
            from.pos[1],
            from.pos[2],
            nav::space_name(world, from.space)
        ),
        AiEvent::DoorsClosedToIt { package } => format!("stays: the way to the place of {} needs a load door, and it cannot open doors", n(package)),
        AiEvent::HeightWaived { at } => format!("ends its walk off its goal's height, at ({:.0}, {:.0}, {:.0}): no new path helped", at[0], at[1], at[2]),
        AiEvent::NotCarriedOut { package, why } => format!("stays: {} is {why}, not carried out yet", n(package)),
        AiEvent::Furniture { furniture: Some(f) } => format!("is to use furniture {f}"),
        AiEvent::Furniture { furniture: None } => "leaves its furniture".into(),
        AiEvent::FurnitureEligibility { furniture, decision, reason } => format!("furniture {furniture}: {decision} ({reason})"),
        AiEvent::Sandbox { doing, walk } => {
            let what = match doing {
                Doing::Standing => "stands".to_string(),
                Doing::Wandering => "wanders".to_string(),
                Doing::Sitting(f) => format!("goes to sit on {f}"),
                Doing::AtIdleMarker(m) => format!("goes to idle marker {m}"),
                Doing::Sleeping(f) => format!("goes to sleep on {f}"),
                Doing::Eating(Some(f)) => format!("has a meal on {f}"),
                Doing::Eating(None) => "has a meal standing".to_string(),
            };
            if *walk > 0.0 { format!("sandbox: {what}, {walk:.0} units away") } else { format!("sandbox: {what}") }
        }
        AiEvent::PatrolWait { patrol_point, seconds } => format!("waits {seconds:.0}s at patrol point {}", n(patrol_point)),
        AiEvent::Approach { package, target, away, length, doors, running } => format!(
            "{} up to {} to talk ({}): {}{length:.0} units to go, {} doors",
            if *running { "runs" } else { "walks" },
            n(target),
            n(package),
            away.map(|a| format!("{a:.0} units away, ")).unwrap_or_default(),
            doors.len()
        ),
        AiEvent::Talk { package, target, line, away, say_to: false, greeting: false } => format!("starts a conversation with {} from {away:.0} units ({}), line {line}", n(target), n(package)),
        AiEvent::Talk { package, target, line, away, say_to: false, greeting: true } => format!("greets {} from {away:.0} units without a conversation ({}), line {line}", n(target), n(package)),
        AiEvent::Talk { package, target, line, away, say_to: true, .. } => format!("says line {line} to {} from {away:.0} units ({})", n(target), n(package)),
        AiEvent::TalkOver { package } => format!("has talked ({})", n(package)),
        AiEvent::WalkUp { away, length } => format!("walks up to the player for a script's StartConversation: {} units away, {length:.0} units to go", away.map_or("in another space".to_string(), |d| format!("{d:.0}"))),
        AiEvent::WalkedUp { away, reached, line } => format!(
            "{} the player ({away:.0} units) and starts the conversation{}",
            if *reached { "has walked up to" } else { "gets no nearer to" },
            line.map_or(" (nothing to say)".to_string(), |l| format!(", line {l}"))
        ),
        AiEvent::Follow { package, target, away, length, doors, running } => format!(
            "{} after {} ({}): {}{length:.0} units to go, {} doors",
            if *running { "runs" } else { "walks" },
            n(target),
            n(package),
            away.map(|a| format!("{a:.0} units away, ")).unwrap_or_else(|| "in another cell, ".into()),
            doors.len()
        ),
        AiEvent::CaughtUp { target, away, stop_at, .. } => format!("has caught up with {}: {away:.0} units away, stops within {stop_at:.0}", n(target)),
        AiEvent::NoTalk { package, why } => format!("does not talk ({}): {why}", n(package)),
        AiEvent::LeadsPlayer { package, to, away } => format!("walks the player into its trigger location ({}): {away:.0} units away, to ({:.0}, {:.0}, {:.0})", n(package), to[0], to[1], to[2]),
        AiEvent::LeadStuck { package, left } => format!("stops walking the player into its trigger location ({}): no nearer for {} s, {left:.0} units short; talks where the player stands", n(package), fnv_ai::carry::LEAD_STUCK_SECONDS),
        AiEvent::TalkUnseen { package, target, away } => format!("waits to talk to {} ({}): {away:.0} units away, with something solid between", n(target), n(package)),
        AiEvent::Hello { line, away } => format!("says hello to the player from {away:.0} units, line {line}"),
        AiEvent::IdleChatter { line, away } => format!("chatters ({away:.0} units from the player), line {line}"),
        AiEvent::PlayerAction { topic, line, away } => format!("comments on the player ({topic}, {away:.0} units away), line {line}"),
        AiEvent::ObserveCombat { fighter, line, away } => format!(
            "comments on the fight of {} ({away:.0} units away), line {line}",
            if *fighter == FormId::PLAYER_REF { "the player".to_string() } else { n(fighter) }
        ),
        AiEvent::SpectatorFlee { fighter, away } => format!(
            "runs from the fight of {} ({away:.0} units away): too close to watch",
            if *fighter == FormId::PLAYER_REF { "the player".to_string() } else { n(fighter) }
        ),
        AiEvent::Chat { partner, line, away } => format!("starts a conversation with {} from {away:.0} units, line {line}", n(partner)),
        AiEvent::ChatApproach { partner, away, length } => format!("walks up to {} to talk: {away:.0} units away, {length:.0} units to go", n(partner)),
        AiEvent::ChatGaveUp { partner, why } => format!("gives up talking to {}: {why}", n(partner)),
        AiEvent::LookTurn { target, from, to } => format!(
            "turns its body to {} for a script's Look: from heading {:.0}° to {:.0}°",
            if *target == FormId::PLAYER_REF { "the player".to_string() } else { n(target) },
            from.to_degrees(),
            to.to_degrees()
        ),
        AiEvent::LookFaced { target, heading } => format!(
            "faces {} for a script's Look, at heading {:.0}°",
            if *target == FormId::PLAYER_REF { "the player".to_string() } else { n(target) },
            heading.to_degrees()
        ),
        AiEvent::LookHeld { target, why } => format!(
            "keeps its body for a script's Look at {}: {why}",
            if *target == FormId::PLAYER_REF { "the player".to_string() } else { n(target) }
        ),
        AiEvent::LookOver { heading } => format!("is let go by a script's Look; the body keeps heading {:.0}°", heading.to_degrees()),
        AiEvent::ChatBack { package, away } => format!("has talked: back to the place of {}, {away:.0} units off", n(package)),
        AiEvent::Search { about, to, away, length } => format!(
            "searches for {} at ({:.0}, {:.0}, {:.0}): {away:.0} units away, {length:.0} units to go",
            if *about == FormId::PLAYER_REF { "the player".to_string() } else { n(about) },
            to[0],
            to[1],
            to[2]
        ),
        AiEvent::SearchLook { seconds, .. } => format!("looks about for {seconds:.0}s"),
        AiEvent::Hold { target, away } => format!(
            "stops and faces {}, {away:.0} units away",
            if *target == FormId::PLAYER_REF { "the player".to_string() } else { n(target) }
        ),
        AiEvent::Warn { target, away } => format!(
            "stops and faces {} to warn it, {away:.0} units away",
            if *target == FormId::PLAYER_REF { "the player".to_string() } else { n(target) }
        ),
        AiEvent::Close { target, away, length, aiming, .. } => format!(
            "{} at {}, {away:.0} units away, {length:.0} units to go{}",
            if *aiming { "walks" } else { "runs" },
            if *target == FormId::PLAYER_REF { "the player".to_string() } else { n(target) },
            if *aiming { ", aiming down the sights" } else { "" }
        ),
        AiEvent::Pace { target, aiming } => format!(
            "{} {}",
            if *aiming { "walks on, aiming down the sights at" } else { "runs on, sights lowered, at" },
            if *target == FormId::PLAYER_REF { "the player".to_string() } else { n(target) }
        ),
        AiEvent::Post { post, target, to, away, length } => format!(
            "goes to {} against {} at ({:.0}, {:.0}, {:.0}): {away:.0} units away, {length:.0} units to go",
            post.name(),
            if *target == FormId::PLAYER_REF { "the player".to_string() } else { n(target) },
            to[0],
            to[1],
            to[2]
        ),
        AiEvent::AtPost { post, off, at, feet, .. } => format!(
            "is at {} ({off:.0} units off): asked for ({:.0}, {:.0}, {:.0}), stands at ({:.0}, {:.0}, {:.0})",
            post.name(),
            at[0],
            at[1],
            at[2],
            feet[0],
            feet[1],
            feet[2]
        ),
        AiEvent::NoPost { post } => format!("has no way to {}: closes in instead", post.name()),
        AiEvent::PostFailed { post, at, feet, off, elapsed, reason, .. } => format!(
            "post failed: {} ({}) after {elapsed:.1} s, {off:.0} units from ({:.0}, {:.0}, {:.0}), stands at ({:.0}, {:.0}, {:.0})",
            post.name(),
            match reason {
                fnv_ai::alert::PostFailure::TimedOut => "timed out",
                fnv_ai::alert::PostFailure::StoppedShort => "stopped short",
            },
            at[0],
            at[1],
            at[2],
            feet[0],
            feet[1],
            feet[2]
        ),
        AiEvent::Flee { from, how, to, away, length } => format!(
            "flees from {} {}: to ({:.0}, {:.0}, {:.0}), {} from it, {length:.0} units to go",
            if *from == FormId::PLAYER_REF { "the player".to_string() } else { n(from) },
            match how {
                fnv_game::combat_group::FleeBy::Door { door } => format!("through door {door}"),
                fnv_game::combat_group::FleeBy::Cover => "to cover".to_string(),
                fnv_game::combat_group::FleeBy::Away => "away".to_string(),
            },
            to[0],
            to[1],
            to[2],
            if away.is_finite() { format!("{away:.0} units") } else { "in another cell".to_string() }
        ),
        AiEvent::FleeArrived { from, seconds, off, hides } => format!(
            "reached its flee spot from {} after {seconds:.1}s ({off:.0} units off): {}",
            if *from == FormId::PLAYER_REF { "the player".to_string() } else { n(from) },
            if *hides { "hides there, crouched" } else { "stays there" }
        ),
        AiEvent::Cornered { from, hides } => format!(
            "is cornered by {}: nowhere to flee, {}",
            if *from == FormId::PLAYER_REF { "the player".to_string() } else { n(from) },
            if *hides { "hides where it is, crouched" } else { "stands" }
        ),
        AiEvent::SearchOver { why } => format!("stops searching: {why}"),
        AiEvent::PackageHolds { package, held: true } => format!("keeps to its package {} in the fight (Continue During Combat)", n(package)),
        AiEvent::PackageHolds { package, held: false } => format!("is let go by its package {} to fight (Continue During Combat)", n(package)),
        AiEvent::SearchBack { package, away } => format!("has searched: back to the place of {}, {away:.0} units off", n(package)),
        AiEvent::Resettles { package } => format!("is back in the place of {}: settles there again", n(package)),
        AiEvent::TakesUpAgain { package } => format!("takes up {} afresh", n(package)),
        AiEvent::BackHome { away, length } => format!(
            "has no package: walks back to where it was placed, {} off, {length:.0} units to go",
            if away.is_finite() { format!("{away:.0} units") } else { "in another space".to_string() }
        ),
        AiEvent::BackUp { away, length } => format!("backs up from the player ({away:.0} units off), {length:.0} units to go"),
        AiEvent::WeaponUse { target, length } => format!(
            "is to use its weapon on {}: {}",
            if *target == FormId::PLAYER_REF { "the player".to_string() } else { n(target) },
            if *length > 0.0 { format!("walks to its place, {length:.0} units to go") } else { "from where it stands".to_string() }
        ),
        AiEvent::WeaponUseOver => "has used its weapon: back to its package".into(),
        AiEvent::Rearmed { weapon, from } => match from {
            Some(f) if f == weapon => format!("rearm: now holds {} it fetched", n(weapon)),
            Some(f) => format!("rearm: fetched {}, holds {} it carries", n(f), n(weapon)),
            None => format!("rearm: holds {} it carries, a better weapon", n(weapon)),
        },
        AiEvent::Fetch { weapon, dps, stops, trip } => format!("rearm: goes for {} ({dps:.1} damage a second), {stops} stop(s), {trip:.0} units", n(weapon)),
        AiEvent::FetchGot { item, from, take, got } => format!(
            "rearm: {} {} {} {}",
            if *got { "picks up" } else { "finds nothing to take:" },
            n(item),
            match take {
                fnv_game::rearm::Take::Ground => "from the ground",
                fnv_game::rearm::Take::FromBody => "from the body of",
                fnv_game::rearm::Take::CopyAmmo => "(a copy) from",
            },
            n(from)
        ),
        AiEvent::FetchDropped { from, why } => format!("rearm: gives up on {}: {why}", n(from)),
        AiEvent::RangeSwap { weapon, distance } => format!("rearm: takes up {} for its target {distance:.0} units away", n(weapon)),
        AiEvent::RangeFists { weapon, distance } => format!("rearm: puts away {} for its fists: its target {distance:.0} units away is inside its blast", n(weapon)),
        AiEvent::EscortWaits { package, target, away } => format!(
            "escorts {}: waits, it is {} behind ({})",
            n(target),
            away.map_or("in another place".to_string(), |a| format!("{a:.0} units")),
            n(package)
        ),
        AiEvent::EscortOn { target, .. } => format!("escorts {}: goes on", n(target)),
        AiEvent::FindGoes { package, object, length } => format!("finds {}: goes to it, {length:.0} units ({})", n(object), n(package)),
        AiEvent::Found { package, object, act } => format!("found {}: {} ({})", n(object), serde_json::to_value(act).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(), n(package)),
        AiEvent::Bought { package, vendor, item, price } => format!("buys {} from {} for {price} caps ({})", n(item), n(vendor), n(package)),
        AiEvent::NothingFound { package, found } => format!("finds nothing more ({found} found, {})", n(package)),
        AiEvent::UsesAt { package, thing: Some(t) } => format!("uses {} ({})", n(t), n(package)),
        AiEvent::UsesAt { package, thing: None } => format!("has nothing to use: stays ({})", n(package)),
        AiEvent::UsesItem { package, item, had } => format!("uses item {}{} ({})", n(item), if *had { "" } else { ": has none" }, n(package)),
        AiEvent::AmbushWaits { package, hidden } => format!("lies in wait{} ({})", if *hidden { ", hidden" } else { "" }, n(package)),
        AiEvent::AmbushSprung { package, enemy, away } => format!("springs its ambush on {} from {away:.0} units ({})", if *enemy == FormId::PLAYER_REF { "the player".to_string() } else { n(enemy) }, n(package)),
        AiEvent::FleeRuns { package, from, length, .. } => format!("flees from {}: runs {length:.0} units ({})", if *from == FormId::PLAYER_REF { "the player".to_string() } else { n(from) }, n(package)),
        AiEvent::FleeSafe { package, from, .. } => format!(
            "is safe{} ({})",
            from.map(|f| format!(" from {}", if f == FormId::PLAYER_REF { "the player".to_string() } else { n(&f) })).unwrap_or_default(),
            n(package)
        ),
    }
}

/// Whether a place is drawn here or in a co-op guest's surroundings the host simulates
/// (`Streamer::coop_areas`, empty outside co-op): its residents are in the high process.
fn simulated(world: &WorldData, streamer: &Streamer, space: Space, feet: [f32; 3]) -> bool {
    drawn(world, streamer, space, feet) || streamer.coop_areas.iter().any(|a| a.holds(space, feet))
}

/// Whether a place is in the cells drawn now.
fn drawn(world: &WorldData, streamer: &Streamer, space: Space, feet: [f32; 3]) -> bool {
    match (space, streamer.interior) {
        (Space::Interior(c), Some(i)) => c == i,
        (Space::World(w), None) => w == world.world && streamer.loaded.contains_key(&fnv_world::cell_of(feet[0], feet[1])),
        _ => false,
    }
}

/// The game being played, when there is one: its scripts' state, its simulation and
/// its collision world.
type PlayState<'w> = (
    Option<ResMut<'w, nvf_play::script::ScriptState>>,
    Option<ResMut<'w, nvf_play::sim::SimState>>,
    Option<ResMut<'w, PhysState>>,
    Option<ResMut<'w, ActorBlocks>>,
);

/// Who is on furniture, the hours passed in the Wait menu, who plays a hit reaction, and
/// who is locked on its target (`npc_combat`).
/// What moves actors besides their walk, and the doors they pass: [`drive`]'s.
type Moves<'w, 's> = (
    ResMut<'w, crate::door_fade::DoorFades>,
    MessageWriter<'w, nvf_play::world_sounds::DoorPassed>,
    Option<ResMut<'w, crate::attack_anims::AttackTravel>>,
    Option<MessageReader<'w, 's, nvf_play::combat::Knockback>>,
    Option<MessageReader<'w, 's, nvf_play::Travel>>,
    Option<ResMut<'w, nvf_play::loading::Loading>>,
    Option<MessageReader<'w, 's, nvf_play::loading::TravelRequest>>,
);

type Held<'w, 's> = (
    Query<'w, 's, &'static RefVisual, With<OnFurniture>>,
    Option<MessageReader<'w, 's, nvf_play::wait::HourPassed>>,
    Query<'w, 's, (&'static RefVisual, &'static crate::locomotion::Flinch)>,
    Option<Res<'w, crate::npc_combat::Fighters>>,
    Option<ResMut<'w, crate::idles::IdleHolds>>,
    Query<'w, 's, &'static mut SitRefused>,
    Query<'w, 's, (&'static RefVisual, &'static crate::creature_stance::CreatureStance), Without<crate::locomotion::Dying>>,
);

/// Run the package choice and the walks, log what happens, keep the visuals in step.
#[allow(clippy::too_many_arguments)]
pub fn drive(
    mut commands: Commands,
    time: Res<Time>,
    mut res: ResMut<Residents>,
    mut nav: ResMut<NavState>,
    world: Res<WorldData>,
    streamer: Res<Streamer>,
    (actor_world, mut ahead, pace): (Option<Res<ActorWorld>>, ResMut<crate::readahead::ReadAhead>, Res<crate::stream_pace::StreamPace>),
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
    root: Query<Entity, With<WorldRoot>>,
    roots: Query<(Entity, &RefVisual, &ChildOf)>,
    mut actors: Query<(&mut Transform, Option<&mut Locomotion>), Without<OnFurniture>>,
    mut doors: ResMut<nvf_play::doors::Doors>,
    (script, mut sim, mut phys, mut blocks): PlayState,
    (seated, hours, flinches, fighters, idle_holds, mut refusals, stances): Held,
    (mut door_fades, mut door_sounds, mut travel, mut knocks, trips, mut screen, asked): Moves,
) {
    // Hours passed in the Wait menu since the last frame (`--play` only: without it
    // there is no menu and no such message).
    let waited = hours.map_or(0, |mut h| h.read().count() as u32);
    let Ok(root) = root.single() else { return };
    // The frame's time, at most a whole frame of the virtual clock (as physics,
    // `nvf_play::phys::play_params`). It was capped at 0.1 s: below 10 frames a second the
    // actors walked and turned slower than the game's clock passed (Goodsprings at 6
    // frames a second: the settler of detection.md §5 came into view after the shot).
    let dt = time.delta_secs().min(nvf_play::phys::MAX_FRAME_SECONDS);
    let now = time.elapsed_secs();
    let res = &mut *res;
    // Start once the world around the camera has loaded (the actors can be seen).
    if !res.started {
        if !streamer.settled {
            return;
        }
        res.started = true;
    }
    // The player set off behind a loading screen through a load door or on a fast
    // travel (not a script's MoveTo: `Loading::arriving`): who was in the loaded area
    // shortly before stays put; the others near the destination are put at their
    // package's place (`Core::arrive`).
    // A travel asked for this frame counts too: a door whose script runs Activate (Gomorrah's
    // inner door, SCPT 0x00127959) asks in the scripts' pass, and its trip is sent only
    // after this system has run; the guard started its weapons check in that frame again
    // (#2835, pilot sweep 2026-11-18).
    let asked = asked.is_some_and(|mut t| t.read().count() > 0);
    if trips.is_some_and(|mut t| t.read().count() > 0) || asked {
        let arriving = screen.is_some_and(|mut s| std::mem::take(&mut s.arriving));
        if arriving {
            res.core.begin_trip();
        }
        // The player is leaving: nobody thinks this frame (the loading screen holds the
        // game from the next). Gomorrah's inner door resets iInGomorrah as the player
        // goes out, and the entrance guard's GomorrahGreeterPackage, judging the player
        // still at the door, started its weapons check with a player already outside;
        // the hand-over set iInGomorrah again, so on the next way in the guard never
        // greeted and the door's DisablePlayerControls was never undone.
        return;
    }
    // The start's loading screen is up: the residents in view are drawn behind it
    // ([`StartPlacing`]).
    let starting = screen.as_deref().is_some_and(|l| l.starting());
    let all = res.all;
    // Who staggers (a hit reaction playing) stands until it ends.
    res.core.on_furniture.clear();
    res.core.on_furniture.extend(seated.iter().map(|v| v.0));
    res.core.staggering.clear();
    res.core.staggering.extend(flinches.iter().filter(|(_, f)| now < f.at + f.clip.clip.length()).map(|(v, _)| v.0));
    // So does a creature drawing its stance (#2686), from the frame before.
    let drawing: HashSet<u32> = stances.iter().filter(|(_, s)| s.holds_in_place()).map(|(v, _)| v.0).collect();
    for id in drawing.difference(&res.core.drawing) {
        info!("actor {id:08X} holds still while it draws its stance");
    }
    for id in res.core.drawing.difference(&drawing) {
        info!("actor {id:08X} drew its stance: free to move");
    }
    res.core.drawing = drawing;
    // How far attack clips carry actors (`attack_anims.rs`), from the frame before.
    if let Some(t) = travel.as_deref_mut() {
        res.core.travel = t.take();
    }
    // Melee blows' knockbacks on actors (`nvf_play::combat`).
    for k in knocks.iter_mut().flat_map(|k| k.read()).filter(|k| k.form_id != FormId::PLAYER_REF.0) {
        res.core.begin_knock(*k, phys.as_deref());
    }
    // Who an idle holds in place (`idles.rs`, from the frame before) stands too.
    // and those of them with somewhere to go (the last update's) are told to break out.
    match idle_holds {
        Some(mut h) => {
            h.breaking = std::mem::take(&mut res.core.breaking);
            res.core.posed = h.held.clone();
            res.core.carried_off = h.carried_off.clone();
        }
        None => {
            res.core.posed.clear();
            res.core.carried_off.clear();
        }
    }
    // The game state: this module's, or the game being played (once its data is in).
    // Under the start's screen, the residents not handed to the AI yet, to draw.
    let start_bodies = match (res.own.as_deref_mut(), script, sim.as_deref_mut()) {
        (Some(game), _, _) => {
            if all {
                res.core.take_cells(&world, &streamer, game.world.placed);
            }
            res.core.check_rooted(&world, &models.data, actor_world.as_deref());
            res.core.take_moved(game, all, None);
            if all {
                res.core.take_placed_elsewhere(game);
                res.core.take_created(game);
            }
            res.core.process_levels(game, &world, &streamer);
            res.core.after_hour_jump(game, &nav, &world, &streamer, None);
            res.core.arrive(game, &nav, &world, &streamer, None);
            res.core.switch_navmeshes(game, &mut nav);
            let _span = info_span!("ai::think").entered();
            res.core.think(game, true, false, &nav, &world, &mut doors, None, None, dt, now, 0);
            // Without `--play` there is no start's screen.
            None
        }
        (None, Some(mut script), Some(sim)) => {
            let script = &mut *script;
            let Some(rt) = script.rt.as_mut() else {
                if let (Some(why), false) = (&script.off, res.warned) {
                    warn!("ai: no package choice: the game's scripts are not running ({why})");
                    res.warned = true;
                }
                return;
            };
            if all {
                res.core.take_cells(&world, &streamer, sim.refs);
            }
            res.core.check_rooted(&world, &models.data, actor_world.as_deref());
            let remote = res.remote.as_ref();
            let core = &mut res.core;
            let (phys, blocks) = (phys.as_deref_mut(), blocks.as_deref_mut());
            let (world_ref, streamer_ref) = (&*world, &*streamer);
            nvf_play::script::with_world(rt, sim, |rt| {
                let mut phys = phys;
                let span = info_span!("ai::take_moved").entered();
                core.take_moved(&mut rt.game, all, phys.as_deref_mut());
                drop(span);
                if all {
                    core.take_placed_elsewhere(&rt.game);
                    if let Some(p) = phys.as_deref_mut() {
                        core.spread_created(&mut rt.game, p);
                    }
                    core.take_created(&rt.game);
                }
                let span = info_span!("ai::process_levels").entered();
                core.process_levels(&rt.game, world_ref, streamer_ref);
                drop(span);
                let span = info_span!("ai::arrive").entered();
                core.after_hour_jump(&mut rt.game, &nav, world_ref, streamer_ref, phys.as_deref_mut());
                core.arrive(&mut rt.game, &nav, world_ref, streamer_ref, phys.as_deref_mut());
                drop(span);
                let span = info_span!("ai::switch_navmeshes").entered();
                core.switch_navmeshes(&mut rt.game, &mut nav);
                drop(span);
                let span = info_span!("ai::think").entered();
                let restored = rt.restored_from_save;
                // A co-op guest: the host thinks for every actor (`Residents::remote`, None
                // outside co-op).
                match remote {
                    Some(remote) => core.follow(&mut rt.game, remote, phys, blocks, now),
                    None => core.think(&mut rt.game, false, restored, &nav, &world, &mut doors, phys, blocks, dt, now, waited),
                }
                drop(span);
                starting.then(|| core.start_bodies(&rt.game))
            })
        }
        _ => return,
    };
    let equipment_world = equipment_world(res.own.as_deref().map(|g| &g.world), sim.as_deref().map(|s| &s.world));
    let mut spawn_stats = crate::actors::ActorStats::default();
    let remote = res.remote.as_ref();
    let core = &mut res.core;
    // Load doors used: the doors' sounds where the listener is, the body left at the door
    // fades out, the one drawn at the other side fades in (`door_fade.rs`).
    for (actor, door, to_door) in std::mem::take(&mut core.door_uses) {
        let Some(r) = core.list.iter_mut().find(|r| r.actor == actor) else { continue };
        let now_space = core.ai.agents.get(&actor).map(|a| a.walker.space);
        let at = |d: u32| fnv_world::placed::placement(world.lo, world.schema, d).map(|p| p.position);
        for (space, d) in [(r.last_space, door), (now_space, to_door)] {
            if let (Some(space), Some(p)) = (space, at(d)) {
                if drawn(&world, &streamer, space, p) {
                    door_sounds.write(nvf_play::world_sounds::DoorPassed { door: d, at: p });
                }
            }
        }
        let was_spawned = r.spawned;
        if hand_off_body(&mut commands, (&mut r.spawned, &mut r.root), now).is_some() {
            door_fades.outs += 1;
        } else if was_spawned {
            // Diagnostic for #2461: a body spawned and not yet found is not taken away.
            warn!("ai: {} went through a door before its body spawned earlier was found: that body is not taken away", r.label);
        }
        r.last_drawn = None;
        door_fades.fade_in.insert(actor.0);
    }
    if nav.travellers.is_empty() {
        nav.lead = core.list.first().and_then(|r| core.ai.agents.get(&r.actor)).map(|a| Lead { space: a.walker.space, feet: a.walker.feet, heading: a.walker.heading });
    }
    // Reuse `core.on_furniture` already populated from `seated` above instead of allocating a duplicate HashSet [design].
    // Above 60 fps a frame gets the same share of itself (stream_pace.rs).
    let mut budget = pace.count(SPAWNS_PER_FRAME);
    let _span = info_span!("ai::draw_and_place").entered();
    for r in core.list.iter_mut().filter(|r| r.joined && !r.failed) {
        // Furniture: sit once drawn; get up before leaving.
        let on = core.on_furniture.contains(&r.actor.0);
        let entity = r.root.filter(|_| r.spawned);
        let wants = r.wants;
        let restart = std::mem::take(&mut r.restart);
        // Hours passed in the Wait menu, or a script moved it (MoveTo), and its package
        // has it elsewhere: it is off its furniture at once (the AI has walked or put it
        // on already).
        let moved = std::mem::take(&mut r.moved);
        if moved {
            r.first_steps = Some(FirstSteps::new("its move by a script"));
        }
        // Or it was put at its package's place after a script moved the clock.
        let moved = moved || std::mem::take(&mut r.jumped);
        if stale_seat(r.seat, wants, waited > 0 || moved).is_some() {
            r.seat = Seat::None;
            match entity {
                Some(e) if on => {
                    commands.entity(e).insert(LeaveNow);
                    core.seats[1] += 1;
                    // Its visual stays for the furniture to let go of it; from the
                    // next frame it is drawn where it is now, or not at all.
                    continue;
                }
                Some(e) => {
                    commands.entity(e).remove::<(SitOn, GetUp)>();
                }
                None => {}
            }
        }
        // The furniture's answer to the last order, acted on once. Only an answer says it
        // was refused: being without `OnFurniture` does not.
        if let Some(mut refused) = entity.and_then(|e| refusals.get_mut(e).ok()).filter(|x| x.unread) {
            refused.unread = false;
            if matches!(r.seat, Seat::Ordered(f, _) if f == refused.furniture && wants == Some(f)) && !on {
                r.seat = Seat::after(retry::on_refusal(refused.furniture, refused.refusal));
            }
        }
        let frame = retry::Now {
            wants,
            eligible: entity.is_some() && r.present && !core.held_now && r.may_ask_again,
            seconds: dt,
            changed: waited > 0 || moved,
        };
        let (seat, order) = next_seat(r.seat, entity.is_some(), on, &frame, restart);
        r.seat = seat;
        if let (Some(e), Some(order)) = (entity, order) {
            match order {
                SeatOrder::Sit(furniture, at_once) => {
                    commands.entity(e).insert(SitOn { furniture, stay: None, at_once });
                    core.seats[0] += 1;
                }
                SeatOrder::GetUp => {
                    commands.entity(e).insert(GetUp);
                    core.seats[1] += 1;
                }
                SeatOrder::Cancel => {
                    commands.entity(e).remove::<SitOn>();
                }
                SeatOrder::Stood => {
                    // Up: the walk goes on from where the furniture left the actor, turned
                    // as it left it (its back to the furniture): the walker's heading was
                    // the one it sat down with, and the body snapped half about to it.
                    if let (Ok((t, _)), Some(a)) = (actors.get(e), core.ai.agents.get_mut(&r.actor)) {
                        a.walker.feet = t.translation.to_array();
                        a.walker.heading = nav::tr_heading(t);
                        a.walker.tri = fnv_nav::graph::NONE;
                    }
                }
            }
        }
        // A refusal of another request is over.
        if let Some(e) = entity.filter(|e| refusals.get(*e).is_ok_and(|x| r.seat.furniture() != Some(x.furniture))) {
            commands.entity(e).remove::<SitRefused>();
        }
        let Some(agent) = core.ai.agents.get(&r.actor) else { continue };
        let w = &agent.walker;
        let visible = r.present && drawn(&world, &streamer, w.space, w.feet);
        if visible && !r.spawned && !r.at_once {
            if budget == 0 {
                continue;
            }
            budget -= 1;
        }
        // Drawn where its character is between two physics steps (as the player is): the
        // steps run at a fixed rate, and frames that held none showed the actor standing
        // (no movement this frame, so the idle clip), which made walks stop and start.
        // Without a character, standing: on the drawn ground when its feet rest on the
        // collision as a character's would; where its character left it, as it was
        // drawn; on the bare landscape, with the smoothed surface (#2149), unless the
        // furniture has it (walking up to it, on it or getting off: `on`). One waiting for
        // a seat or refused one stands free and follows the landscape.
        let on_landscape = !on;
        // A co-op guest's actor the host draws: where the host draws it, a short jump
        // caught up at a run's pace ([`catch_up`]), a long one (a script's move) at once.
        let feet = if remote.is_some_and(|m| m.contains_key(&r.actor.0)) {
            let top = if core.ai.agents.contains_key(&r.actor) { core.ai.alert_speeds(r.actor).run.max(core.ai.calm_speeds(r.actor).run) } else { CATCH_UP_SPEED };
            if r.spawned && visible && !on {
                catch_up(r.drawn_feet, w.feet, top, dt)
            } else {
                w.feet
            }
        } else {
            phys.as_deref_mut().map_or(w.feet, |p| p.npc_drawn_feet(r.actor.0).unwrap_or_else(|| p.standing_drawn_feet(r.actor.0, w.feet, on_landscape)))
        };
        let step = [feet[0] - r.drawn_feet[0], feet[1] - r.drawn_feet[1]];
        let moving = agent.activity == Activity::Travelling && step[0].hypot(step[1]) > 1e-4;
        if visible && r.spawned && !on && !core.held_now && agent.activity == Activity::Travelling {
            let unmoved = (w.feet[0] - r.last_feet[0]).hypot(w.feet[1] - r.last_feet[1]) <= 1e-4;
            core.walk_frames[0] += 1;
            core.walk_frames[1] += usize::from(unmoved);
            core.walk_frames[2] += usize::from(!moving);
            if !moving {
                let in_phys = phys.as_deref().is_some_and(|p| p.npcs.contains_key(&r.actor.0));
                core.standing_by[if in_phys { 0 } else if w.turning { 1 } else { 2 }] += 1;
            }
            match r.standing_frames.frame(moving) {
                Some(RunEnd::Stop(n)) => core.standing_by[if n <= 3 { 3 } else { 4 }] += 1,
                Some(RunEnd::SetOff) => core.standing_by[5] += 1,
                None => {}
            }
        } else if !core.held_now {
            // Not on its way, out of sight or on furniture: its next walk starts afresh.
            r.standing_frames = StopRun::default();
        }
        r.drawn_feet = feet;
        r.last_feet = w.feet;
        r.last_space = Some(w.space);
        // Not drawn at the other side of its door: nothing to fade in.
        if !visible {
            door_fades.fade_in.remove(&r.actor.0);
        }
        // How fast its drawn body turns (the turn summary): the AI's heading of an actor
        // drawn in this frame and the last, not on furniture, not through a door.
        let drawn_now = visible && r.spawned && !on;
        r.last_drawn = match (r.last_drawn, drawn_now) {
            (Some((h, at)), true) => {
                if dt > 0.0 && (at[0] - w.feet[0]).hypot(at[1] - w.feet[1]) < 100.0 {
                    let rate = fnv_nav::follow::heading_delta(h, w.heading).abs() / dt;
                    if rate > core.fastest_turn.0 {
                        core.fastest_turn = (rate, r.label.clone());
                    }
                    core.snaps += usize::from(rate > SNAP_TURN);
                }
                Some((w.heading, w.feet))
            }
            (None, true) => Some((w.heading, w.feet)),
            (_, false) => None,
        };
        // A fighter is drawn turning to face its target (its legs walk the way it goes
        // in its own axes: strafing, backing away); afterwards it turns back to its
        // walker's heading, at twice the walking turn rate. Locked on a target it sees
        // (`npc_combat::Fighters::lock_on`): it faces where that target is drawn this
        // frame, not where the AI last placed it (a charging Fiend was drawn 110 units
        // from its AI place, and the trooper shooting at it faced 16° off), and once
        // turned to it, it stays on it.
        // A weapon use's aim is not turned by a lock on another target (one left from
        // before the use took the actor over, [`keeps_lock`]).
        let lock = fighters.as_deref().and_then(|f| f.lock_on(r.actor.0)).filter(|(t, _)| keeps_lock(*t, core.use_aims.get(&r.actor).copied()));
        let lock_target = lock.map(|(t, _)| t);
        let held = lock_target.is_some() && r.lock_held == lock_target;
        let lock = lock.map(|(_, at)| at);
        r.lock_held = None;
        // One that never turns (`fnv_ai::Ai::fix_heading`: Mr. House's screen) is drawn
        // at its walker's heading, which nothing changes, with a target or without (#2150).
        let aim = lock.or_else(|| core.aims.get(&r.actor).copied()).filter(|_| !agent.fixed);
        let heading = match (aim, r.aim_heading.filter(|_| !agent.fixed)) {
            (Some(t), from) => {
                let want = heading_of([t[0] - feet[0], t[1] - feet[1]]);
                let from = from.unwrap_or(w.heading);
                // Standing, at the rate the AI turns a fighter (`fnv_ai::alert::combat_turn_rate`),
                // so one that never moves is drawn as its walker turns; on the move, at the
                // walking turn rate: the original's trooper turned slower from the moment it
                // stepped off (275 down to about 90°/s) [observed 2026-11-06, capture
                // o25_turn, docs/spec-notes/actors.md §8.9].
                let rate = if agent.activity == Activity::Travelling { fnv_nav::follow::WALK_TURN_RATE } else { fnv_ai::alert::combat_turn_rate(heading_delta(from, want)) };
                let hold = lock.is_some() && (held || heading_delta(from, want).abs() <= LOCK_HOLD);
                let h = if hold { want } else { turn_towards(from, want, rate * dt) };
                if hold {
                    r.lock_held = lock_target;
                }
                r.aim_heading = Some(h);
                h
            }
            (None, Some(from)) if heading_delta(from, w.heading).abs() > 0.05 && !on => {
                let h = turn_towards(from, w.heading, fnv_nav::follow::WALK_TURN_RATE * 2.0 * dt);
                r.aim_heading = Some(h);
                h
            }
            _ => {
                r.aim_heading = None;
                w.heading
            }
        };
        // An idle that holds its actor in place and carries the body away from the
        // reference keeps the body facing as it did when the idle began: the AI turning
        // the reference to whom it speaks swung the body sideways about it (Dad in Fallout
        // 3's birth scene, `Bip01 NonAccum` about 144 units forward of his marker)
        // [observed: owner's TTW run 2026-11-02, Dad turned 27° at his first line;
        // hypothesis: the original does not turn such an actor]. An idle that leaves the
        // body over its reference (a dialogue response's) still lets it turn. Let go, it
        // turns back to the walker's heading as a fighter does.
        let heading = match (core.carried_off.contains(&r.actor.0), r.posed_heading) {
            (true, Some(h)) => h,
            (true, None) => *r.posed_heading.insert(heading),
            (false, Some(h)) => {
                r.posed_heading = None;
                r.aim_heading = Some(h);
                h
            }
            (false, None) => heading,
        };
        let strafe = r.aim_heading.is_some();
        // How a fighter facing its target moves (the fight summary): forward, sideways or
        // back, by the angle between its step and its heading.
        if strafe && moving && visible && core.aims.contains_key(&r.actor) {
            let off = heading_delta(heading, heading_of(step)).abs();
            core.fight_moves[usize::from(off > FRAC_PI_4) + usize::from(off > 3.0 * FRAC_PI_4)] += dt;
            if core.to_cover.contains(&r.actor) {
                core.cover_moves[0] += dt;
                if off <= FRAC_PI_4 {
                    core.cover_moves[1] += dt;
                }
            }
        }
        let stage = (&mut r.spawned, &mut r.root);
        if let Err(e) = nav::show_actor(&mut commands, root, visible, actor_world.as_deref(), equipment_world, &mut ahead, &mut spawn_stats, &mut parts, &mut models, &roots, &mut actors, nav::Who { reference: r.actor.0, base: r.base, creature: r.creature, scale: r.scale, strafe, air: agent.walker.air.is_some(), jumped: moved, at_once: r.at_once }, stage, (feet, heading), moving, dt) {
            if r.named {
                warn!("ai: {} could not be drawn ({e})", r.label);
            }
            r.failed = true;
        }
        if r.spawned {
            r.at_once = false;
        }
        // Its first second drawn after a script made, enabled or moved it: the clips it
        // walks or runs with (`FirstSteps`).
        if let (Some(fs), true) = (r.first_steps.as_mut(), visible && r.spawned) {
            if let Some(lo) = r.root.and_then(|e| actors.get(e).ok()).and_then(|(_, lo)| lo) {
                use fnv_actor::locomotion::Move;
                let mv = lo.blend.cur.mv;
                if matches!(mv, Move::FastForward | Move::FastBackward | Move::FastLeft | Move::FastRight) {
                    fs.run += 1;
                } else if mv.is_moving() {
                    fs.walk += 1;
                }
                if moving {
                    fs.speed = fs.speed.max(agent.walker.moving_speed());
                }
                fs.drawn = fs.drawn.max(lo.velocity[0].hypot(lo.velocity[1]));
                fs.left -= dt;
            }
            if fs.left <= 0.0 {
                if fs.walk + fs.run > 0 {
                    info!("ai [{}] {}: first second drawn after {}: {} walk frames, {} run frames (walker at {:.0} units/s, drawn up to {:.0})", core.clock_text, r.label, fs.why, fs.walk, fs.run, fs.speed, fs.drawn);
                }
                r.first_steps = None;
            }
        }
    }
    // Under the start's screen: the residents in view are drawn behind it, not
    // [`SPAWNS_PER_FRAME`] a frame once it is down, and the screen waits for them
    // ([`StartPlacing`]; `nvf_play::loading::StartActors`). Only once the cells around the
    // player are in, drawn and in physics (the drawn feet follow the collision).
    if let (Some(bodies), Some(aw)) = (start_bodies, actor_world.as_deref()) {
        let start = &mut res.start;
        start.game_frames += 1;
        let capped = screen.as_deref().is_some_and(|l| l.placing_capped());
        let may = start.on && start.game_frames > 1 && !capped && streamer.settled && phys.as_deref().is_none_or(|p| p.loading_done());
        let mut want = Vec::new();
        for b in bodies {
            let r = &mut core.list[b.index];
            let visible = b.present && drawn(&world, &streamer, b.space, b.feet);
            if visible && !r.spawned {
                want.push(b);
            } else if !visible && r.spawned {
                // Out of view since (a travel under the start's screen): its body goes.
                let who = nav::Who { reference: r.actor.0, base: r.base, creature: r.creature, scale: r.scale, strafe: false, air: false, jumped: false, at_once: r.at_once };
                let _ = nav::show_actor(&mut commands, root, false, Some(aw), equipment_world, &mut ahead, &mut spawn_stats, &mut parts, &mut models, &roots, &mut actors, who, (&mut r.spawned, &mut r.root), (b.feet, b.heading), false, 0.0);
            }
        }
        if may && !start.told && !want.is_empty() {
            start.told = true;
            info!("ai: the start's screen waits for {} residents in view to be drawn under it", want.len());
        }
        let _span = info_span!("ai::draw_under_start").entered();
        let t0 = std::time::Instant::now();
        let (placed, left) = place_frame(&want, may, || t0.elapsed(), |b| {
            let r = &mut core.list[b.index];
            // Its drawn feet as the first frame of play will have them (no character yet).
            let feet = phys.as_deref_mut().map_or(b.feet, |p| p.npc_drawn_feet(r.actor.0).unwrap_or_else(|| p.standing_drawn_feet(r.actor.0, b.feet, true)));
            let who = nav::Who { reference: r.actor.0, base: r.base, creature: r.creature, scale: r.scale, strafe: false, air: false, jumped: false, at_once: r.at_once };
            match nav::show_actor(&mut commands, root, true, Some(aw), equipment_world, &mut ahead, &mut spawn_stats, &mut parts, &mut models, &roots, &mut actors, who, (&mut r.spawned, &mut r.root), (feet, b.heading), false, 0.0) {
                Err(e) => {
                    if r.named {
                        warn!("ai: {} could not be drawn ({e})", r.label);
                    }
                    r.failed = true;
                    Placed::Never
                }
                Ok(()) if r.spawned => {
                    r.at_once = false;
                    // The first frame of play goes on from this body, as the draw loop
                    // would from its last frame: its drawn feet and its walker's. And it
                    // is put at its walker's place then (`jumped`), not turned or walked
                    // there: a script that moves it on the scripts' first frame (MoveTo)
                    // is not seen by `Core::take_moved`, which runs before it is handed
                    // to the AI [#3081 review]. Its seat is still `Seat::None`, so
                    // nothing else changes.
                    r.drawn_feet = feet;
                    r.last_feet = b.feet;
                    r.jumped = true;
                    Placed::Drawn
                }
                Ok(()) => Placed::NotYet,
            }
        });
        start.placed += placed;
        if let Some(l) = screen.as_deref_mut() {
            l.start_actors = Some(nvf_play::loading::StartActors { placed: start.placed, left, under_screen: start.on, drawing: may });
        }
    }
    crate::profile::actor_spawns(spawn_stats.equipment_spawns, spawn_stats.record_spawns);
}

/// The residents' state when the app exits (smoke tests read it).
pub fn report(
    mut exit: MessageReader<AppExit>,
    res: Res<Residents>,
    world: Res<WorldData>,
    script: Option<Res<nvf_play::script::ScriptState>>,
    bodies: Query<(Entity, &crate::limbs::ActorRig, &GlobalTransform)>,
    world_root: Query<&GlobalTransform, With<WorldRoot>>,
) {
    if exit.read().next().is_none() {
        return;
    }
    let game: Option<&Game> = res.own.as_deref().or_else(|| script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game));
    let Some(game) = game else {
        info!("ai summary: the package choice never started");
        return;
    };
    let core = &res.core;
    let data = game.data;
    // Actor bodies drawn more than once for one reference (#2461), in game space.
    let to_game = world_root.single().map_or(bevy::math::Affine3A::IDENTITY, |g| g.affine().inverse());
    let twice = drawn_twice(bodies.iter().map(|(e, rig, g)| (rig.reference, e, to_game.transform_point3(g.translation()).to_array())));
    for (reference, places) in &twice {
        let r = core.list.iter().find(|r| r.actor.0 == *reference);
        let label = r.map_or_else(|| data.name(game.world.placed.get(FormId(*reference)).map_or(FormId(*reference), |p| p.base)), |r| r.label.clone());
        let tracked = r.and_then(|r| r.root.filter(|_| r.spawned));
        let each: Vec<String> = places.iter().map(|(e, p)| format!("({:.0}, {:.0}, {:.0}){}", p[0], p[1], p[2], if Some(*e) == tracked { " driven" } else { "" })).collect();
        info!("actor bodies: {label} ({reference:08X}) is drawn by {} bodies: {}", places.len(), each.join(", "));
    }
    info!("actor bodies summary: {} references drawn by more than one body ({} bodies in all)", twice.len(), twice.iter().map(|(_, p)| p.len()).sum::<usize>());
    let one = |r: &Resident| -> Option<String> {
        let a = core.ai.agents.get(&r.actor)?;
        let package = game.extra.get(&r.actor).and_then(|e| e.current_package).map(|p| data.name(p)).unwrap_or_else(|| "no package".into());
        let state = match a.activity {
            Activity::None => "idle",
            Activity::Travelling => "on its way",
            Activity::There => "there",
            Activity::Staying => "staying",
        };
        Some(format!("{}: {package}, {state} in {}, walked {:.0} units, {} doors opened", r.label, nav::space_name(&world, a.walker.space), a.walker.travelled, r.doors_opened))
    };
    // Named sandboxers' choices in a meal window with no free seat, so no meal (#2011;
    // the smoke test sandbox-no-seat-meals reads them).
    for r in core.list.iter().filter(|r| r.named) {
        if let Some(sb) = core.ai.agents.get(&r.actor).and_then(|a| a.sandbox.as_ref()) {
            info!("ai sandbox meals: {}: {} choices in a meal window with no free seat", r.label, sb.seatless_meal_choices());
        }
    }
    // The named ones, then those of the crowd that walked the most.
    let mut each: Vec<String> = core.list.iter().filter(|r| r.named).filter_map(one).collect();
    let mut crowd: Vec<&Resident> = core.list.iter().filter(|r| !r.named && core.ai.agents.get(&r.actor).is_some_and(|a| a.walker.travelled > 0.0)).collect();
    crowd.sort_by(|a, b| core.ai.agents[&b.actor].walker.travelled.total_cmp(&core.ai.agents[&a.actor].walker.travelled));
    each.extend(crowd.into_iter().take(6).filter_map(one));
    let by = |a: Activity| core.ai.agents.values().filter(|x| x.activity == a).count();
    info!(
        "turn summary: the fastest turn of a drawn resident was {:.0}°/s{}; {} frames with a turn faster than {:.0}°/s",
        core.fastest_turn.0.to_degrees(),
        if core.fastest_turn.1.is_empty() { String::new() } else { format!(" ({})", core.fastest_turn.1) },
        core.snaps,
        SNAP_TURN.to_degrees()
    );
    info!(
        "fight move summary: drawn fighters facing their target moved {:.1} s forward, {:.1} s sideways (strafing), {:.1} s back; {:.1} s of it on the way to new cover, {:.1} s of that facing the way to it",
        core.fight_moves[0], core.fight_moves[1], core.fight_moves[2], core.cover_moves[0], core.cover_moves[1]
    );
    // Flights over ledges of actors on the navmesh surface (no physics), and landings.
    let (flights, landings) = core.ai.agents.values().fold((0, 0), |(f, l), a| (f + a.walker.flights, l + a.walker.landings));
    info!(
        "actor jumps summary (residents): walked by physics, {} jumps up a ledge, {} drops off one, {} jumps over an obstacle; on the navmesh surface, {flights} flights over a ledge, {landings} landings",
        core.jumps[0], core.jumps[1], core.jumps[2]
    );
    info!(
        "walk summary: {} frames of drawn residents on their way (the game not held); their character had not moved in {} of them (no physics step that frame), drawn standing in {} ({:.1}%: {} with a physics character, {} turning on the spot, {} other walkers); {} stops of 1-3 frames, {} longer; {} walks set off after standing first (turning on the spot, or waiting for their character's first physics step)",
        core.walk_frames[0],
        core.walk_frames[1],
        core.walk_frames[2],
        100.0 * core.walk_frames[2] as f64 / core.walk_frames[0].max(1) as f64,
        core.standing_by[0],
        core.standing_by[1],
        core.standing_by[2],
        core.standing_by[3],
        core.standing_by[4],
        core.standing_by[5]
    );
    info!(
        "ai summary at {}: {} residents, {} packages chosen, {} trips, {} arrivals, {} unreachable; {} there, {} on their way, {} staying, {} drawn; {} told to use furniture, {} to get up, {} on furniture now; sandbox: {} wanders, {} seats, {} idle markers, {} sleeps, {} meals, {} times nothing to do; {} talks started by dialogue packages; physics moved actors in {} updates (at most {} at once), gave up {} times, moved {} stuck actors on, {} times nowhere to move one to; standing actors pushed aside {} times ({:.0} units in all, {} times no room); {} hours passed in the wait menu ({} walks in them); {}",
        core.clock_text,
        core.list.len(),
        core.counts[3],
        core.counts[0],
        core.counts[1],
        core.counts[2],
        by(Activity::There),
        by(Activity::Travelling),
        by(Activity::Staying),
        core.list.iter().filter(|r| r.spawned).count(),
        core.seats[0],
        core.seats[1],
        core.list.iter().filter(|r| matches!(r.seat, Seat::Ordered(_, true))).count(),
        core.sandbox[1],
        core.sandbox[2],
        core.sandbox[3],
        core.sandbox[4],
        core.sandbox[5],
        core.sandbox[0],
        core.talks,
        core.physics[0],
        core.physics[2],
        core.physics[1],
        core.physics[3],
        core.skip_refusals.total,
        core.pushes[0],
        core.pushed_units,
        core.pushes[1],
        core.waited[0],
        core.waited[1],
        each.join("; ")
    );
    info!("ai: attack clips carried actors in {} actor updates, {} times there was no character to carry one", core.carries[0], core.carries[1]);
    info!("knockback summary: {} knockbacks on actors, {:.0} units asked in all", core.knocked.0, core.knocked.1);
    let st = core.ai.social_stats();
    info!(
        "social summary: {} hellos said to the player, {} random conversations started, {} walks up to a partner ({} given up); {} tries next to the player, {} chances won; {:.0} actor-seconds next to the player",
        core.social[0], core.social[1], core.social[2], core.social[3], st.tries, st.chances_won, st.near_player_seconds
    );
    let p = &core.ai.process_stats;
    let (high, low) = core.ai.process_counts();
    let far = core.ai.far_count();
    info!(
        "process summary: {high} residents in the loaded area (high process) and {low} out of it at the end ({far} of them beyond the cell buffer), at most {}, {} and {}; {} left the loaded area, {} came back; {} fell out of the cell buffer; {} low-process updates every {:.0} s and {} every {:.0} s ({} walking steps); {} conversations or searches ended by leaving; {} set off into the loaded area from out of it ({} updates every frame); {} far walks stopped at the edge of the loaded area",
        p.most_high,
        p.most_low,
        p.most_far,
        p.left,
        p.came_back,
        p.fell_far,
        p.low_updates - p.far_updates - p.coming_updates,
        fnv_ai::LOW_PROCESS_SECONDS,
        p.far_updates,
        fnv_ai::FAR_PROCESS_SECONDS,
        p.low_steps,
        p.dropped,
        p.coming,
        p.coming_updates,
        p.reached_loaded
    );
    info!("far script summary: {} GameMode runs of residents out of the loaded cells (at their low-process updates and in waited hours)", p.far_scripts);
    info!(
        "arrival summary: {} residents put at their package's place behind a loading screen, {} seated at once on their furniture; {} left where they were (in the loaded area before the trip)",
        core.arrivals[0], core.arrivals[1], core.arrivals[2]
    );
}

/// Whether an actor out of the loaded area takes its cell into the cell buffer, when the
/// buffer does not hold it: one already at Mid-High walking on into another cell, and one
/// going from the loaded area through a door into an interior (`into_interior`: it has
/// been drawn there, and is in an interior now). One walking out of the
/// loaded area in a worldspace does not, so it drops to Low (#958).
fn carries_cell(high: bool, buffered: bool, new_cell: bool, into_interior: bool, level: fnv_ai::ProcessLevel) -> bool {
    use fnv_ai::ProcessLevel::{High, MidHigh};
    !high && !buffered && ((new_cell && level == MidHigh) || (into_interior && level == High))
}

/// The process level for an actor: High in the loaded area, Coming on its way in, Mid-High
/// in the cell buffer, Low beyond.
fn level_for(high: bool, coming: bool, buffered: bool) -> fnv_ai::ProcessLevel {
    match (high, coming, buffered) {
        (true, _, _) => fnv_ai::ProcessLevel::High,
        (false, true, _) => fnv_ai::ProcessLevel::Coming,
        (false, false, true) => fnv_ai::ProcessLevel::MidHigh,
        (false, false, false) => fnv_ai::ProcessLevel::Low,
    }
}

/// Whom a person in Combat is drawn facing (`Core::aims`): the target of a weapon use
/// it is carrying out (`Game::active_weapon_use`) and the fight did not take over
/// ([`use_aim`]), else its combat target. The shooter
/// (`use_weapon`) fires only once the body faces the use's target, and the use keeps
/// the ordinary attacks off (`npc_combat::weapon_use_holds_fight`), so facing the combat
/// target instead would leave neither firing (#2120) [design].
fn aim_target(use_target: Option<FormId>, combat_target: Option<FormId>) -> Option<FormId> {
    use_target.or(combat_target)
}

/// The target a person's weapon use (`Game::active_weapon_use`) aims it at
/// ([`aim_target`]): none when the fight took the actor over (`taken_by_fight`,
/// `WeaponUse::taken`, set by `npc_combat::fight`, this frame's or the last: the two
/// systems are not ordered; the shooter has nothing to act on, as a marker target, one
/// not drawn, or not yet at its stand), so it faces its enemy and the fight's lock on
/// that enemy holds ([`keeps_lock`]) (#2145 review) [design]. Only people are aimed here;
/// a creature's heading is its walker's, which `fnv_ai::carry` turns the same way.
fn use_aim(use_target: Option<FormId>, taken_by_fight: bool) -> Option<FormId> {
    use_target.filter(|_| !taken_by_fight)
}

/// Whether a combat lock on `lock_target` (`npc_combat::Fighters::lock_on`) may hold a
/// person's drawn heading: not when a weapon use owns its aim at another target
/// (`use_target`, [`aim_target`]).
fn keeps_lock(lock_target: u32, use_target: Option<FormId>) -> bool {
    use_target.is_none_or(|t| t.0 == lock_target)
}

/// One reference's drawn bodies, each with where it stands.
type Bodies<K> = Vec<(K, [f32; 3])>;

/// The references drawn by more than one actor body at once, in FormID order, each with
/// its bodies (`K`: the body, and where it stands), from every drawn body's reference;
/// the player's own body is left out. One reference has one body; a second one stands
/// where something left it and answers to the reference (#2461, Lt. Boyd at Camp
/// McCarran) [design: a diagnostic for the smoke tests].
fn drawn_twice<K: Copy>(bodies: impl IntoIterator<Item = (u32, K, [f32; 3])>) -> Vec<(u32, Bodies<K>)> {
    let mut by_ref: std::collections::BTreeMap<u32, Bodies<K>> = std::collections::BTreeMap::new();
    for (reference, body, at) in bodies.into_iter().filter(|b| b.0 != FormId::PLAYER_REF.0) {
        by_ref.entry(reference).or_default().push((body, at));
    }
    by_ref.into_iter().filter(|(_, b)| b.len() > 1).collect()
}

#[cfg(test)]
mod tests {
    use super::{carries_cell, drawn_twice, level_for, newly_loaded, recently_loaded, RunEnd, StopRun, RECENTLY_LOADED_HOURS};
    use crate::world::CellKey;
    use fnv_ai::ProcessLevel;
    use fnv_sim::FormId;
    use std::collections::{HashMap, HashSet};
    use super::Skip;

    /// #3341: the last stretch's skip lets a fighter go back to physics; a stuck one's
    /// holds it, fighting or not, and every skip ends at its time.
    #[test]
    fn a_last_stretch_skip_does_not_hold_a_fighter() {
        let stretch = Skip { until: 4.0, last_stretch: true };
        assert!(stretch.holds(1.0, false));
        assert!(!stretch.holds(1.0, true));
        assert!(!stretch.holds(4.0, false));
        let stuck = Skip::retry(8.0);
        assert!(stuck.holds(1.0, false) && stuck.holds(1.0, true));
        assert!(!stuck.holds(8.0, true));
    }

    /// #2432, #2434: Freeside's worldspaces lie on the same grid squares. Going through a
    /// door from one into another (FreesideWorld to FreesideNorthWorld, that to
    /// FreesideFortWorld) hands over every loaded cell of the one entered on the first
    /// arrival, though the same squares were loaded in the one left; a square seen
    /// before in the same worldspace is not handed over twice.
    #[test]
    fn sibling_worldspaces_on_the_same_squares_hand_over_their_cells_on_the_first_arrival() {
        const FREESIDE: u32 = 0x0010_BEEA;
        const NORTH: u32 = 0x0012_D94D;
        const FORT: u32 = 0x0012_D94E;
        let squares = |s: &[(i32, i32)]| -> HashSet<CellKey> { s.iter().map(|g| CellKey::Exterior(*g)).collect() };
        let set = |v: Vec<CellKey>| -> HashSet<CellKey> { v.into_iter().collect() };
        let (mut seen, mut world) = (HashSet::new(), 0);
        let around_gate = [(0, 0), (1, 0), (0, 1), (1, 1)];
        assert_eq!(newly_loaded(&mut seen, &mut world, FREESIDE, squares(&around_gate)).len(), 4, "the start");
        assert!(newly_loaded(&mut seen, &mut world, FREESIDE, squares(&around_gate)).is_empty(), "the same squares again");
        // Through the door into the north side, on the same squares: the loading screen
        // empties the loaded cells, then they come in one by one; all of them are new.
        assert!(newly_loaded(&mut seen, &mut world, NORTH, HashSet::new()).is_empty());
        let mut got = newly_loaded(&mut seen, &mut world, NORTH, squares(&[(0, 0), (1, 1)]));
        got.extend(newly_loaded(&mut seen, &mut world, NORTH, squares(&around_gate)));
        assert_eq!(set(got), squares(&around_gate), "Orris's square (1, 1) is handed over");
        // A frame that still shows the squares of the worldspace left counts them for
        // the one entered (its cells are looked up there), not as seen already.
        let (mut seen2, mut world2) = (HashSet::new(), 0);
        newly_loaded(&mut seen2, &mut world2, NORTH, squares(&around_gate));
        assert_eq!(set(newly_loaded(&mut seen2, &mut world2, FORT, squares(&around_gate))), squares(&around_gate), "the fort's people join");
        // Into the fort and back out: each worldspace's squares on each arrival.
        assert_eq!(newly_loaded(&mut seen, &mut world, FORT, squares(&[(1, 0)])), vec![CellKey::Exterior((1, 0))]);
        assert_eq!(newly_loaded(&mut seen, &mut world, NORTH, squares(&[(1, 0)])), vec![CellKey::Exterior((1, 0))]);
        // An interior and back to the same worldspace.
        assert_eq!(newly_loaded(&mut seen, &mut world, NORTH, [CellKey::Interior(0x0010_E03B)].into()), vec![CellKey::Interior(0x0010_E03B)]);
        assert_eq!(newly_loaded(&mut seen, &mut world, NORTH, squares(&[(1, 0), (2, 0)])).len(), 2);
    }

    /// #2461: a reference with two bodies is reported with both, one with one body is
    /// not, and the player's own body never counts.
    #[test]
    fn a_reference_drawn_by_two_bodies_is_reported() {
        let boyd = 0x000F_0678;
        let silus = 0x000F_2DEF;
        let bodies = [(boyd, 1, [296.0, 1665.0, 18671.0]), (silus, 2, [228.0, 1726.0, 18671.0]), (boyd, 3, [325.0, 719.0, 18672.0]), (FormId::PLAYER_REF.0, 4, [0.0; 3]), (FormId::PLAYER_REF.0, 5, [0.0; 3])];
        let twice = drawn_twice(bodies);
        assert_eq!(twice.len(), 1, "{twice:?}");
        assert_eq!(twice[0].0, boyd);
        assert_eq!(twice[0].1.iter().map(|b| b.0).collect::<Vec<_>>(), vec![1, 3]);
        assert!(drawn_twice([(silus, 2, [0.0; 3])]).is_empty());
    }

    /// #2120: a person carrying out a weapon use (a Use Weapon package with Continue
    /// During Combat, or a script's) is drawn facing the use's target, not its combat
    /// enemy, and a lock on the enemy left from its fight does not turn it back: the
    /// shooter fires only facing the use's target, and the fight is held meanwhile.
    #[test]
    fn a_weapon_use_owns_the_aim_over_the_combat_target_and_its_lock() {
        use super::{aim_target, keeps_lock};
        let (package_target, enemy) = (FormId(0x11), FormId(0x22));
        // No weapon use: its combat target, and its lock holds.
        assert_eq!(aim_target(None, Some(enemy)), Some(enemy));
        assert!(keeps_lock(enemy.0, None));
        // A weapon use being carried out: its target, though the enemy is elsewhere.
        assert_eq!(aim_target(Some(package_target), Some(enemy)), Some(package_target));
        assert_eq!(aim_target(Some(package_target), None), Some(package_target), "no combat target needed");
        assert!(!keeps_lock(enemy.0, Some(package_target)), "a stale lock on the enemy is let go");
        assert!(keeps_lock(package_target.0, Some(package_target)), "a lock on the use's own target holds");
        // Neither: nothing to face.
        assert_eq!(aim_target(None, None), None);
    }

    /// #2145 review: a use the fight took over (a marker target, one not drawn, not yet at
    /// its stand: the shooter has nothing to act on) owns no aim; the body faces its enemy
    /// and the fight's lock on it holds, as with no use at all.
    #[test]
    fn a_weapon_use_the_fight_took_over_leaves_the_aim_to_the_combat_target() {
        use super::{aim_target, keeps_lock, use_aim};
        let (marker, enemy) = (FormId(0x33), FormId(0x22));
        assert_eq!(use_aim(Some(marker), false), Some(marker), "the shooter acts on it: its target");
        let taken = use_aim(Some(marker), true);
        assert_eq!(taken, None);
        assert_eq!(aim_target(taken, Some(enemy)), Some(enemy));
        assert!(keeps_lock(enemy.0, taken), "the fight's lock on its enemy holds");
        assert_eq!(use_aim(None, true), None);
    }

    /// Only an Immobile creature with a Turning Speed of 0 never turns: Mr. House's
    /// screen does not, a turret (Immobile, 30 and more) and a walking creature whose
    /// record carries 0 do (issue #2150).
    #[test]
    fn only_an_immobile_creature_with_no_turning_speed_never_turns() {
        use super::never_turns;
        assert!(never_turns(true, Some(0.0)));
        assert!(!never_turns(true, Some(30.0)));
        assert!(!never_turns(false, Some(0.0)));
        assert!(!never_turns(true, None));
    }

    /// ResetAI on a seated actor whose package gives it the same chair again: it gets up
    /// by the exit clip, nothing tells it to sit while that plays, and once off it may sit
    /// down again (issue #2119). Without the restart the same chair changes nothing.
    #[test]
    fn a_reset_gets_a_seated_actor_up_before_it_sits_on_the_same_chair_again() {
        use super::{next_seat, retry::Now, Seat, SeatOrder};
        let now = Now { wants: Some(7), eligible: true, seconds: 0.1, changed: false };
        assert_eq!(next_seat(Seat::Ordered(7, true), true, true, &now, false), (Seat::Ordered(7, true), None));
        let (seat, order) = next_seat(Seat::Ordered(7, true), true, true, &now, true);
        assert_eq!((seat, order), (Seat::GettingUp(7), Some(SeatOrder::GetUp)));
        // The exit clip plays: no order, even with the same chair wanted.
        assert_eq!(next_seat(seat, true, true, &now, false), (Seat::GettingUp(7), None));
        // Off: the walk goes on from where it stands; then it is told to sit again.
        let (seat, order) = next_seat(seat, true, false, &now, false);
        assert_eq!((seat, order), (Seat::None, Some(SeatOrder::Stood)));
        let (seat, order) = next_seat(seat, true, false, &now, false);
        assert_eq!((seat, order), (Seat::Wanted(7, false), None));
        assert_eq!(next_seat(seat, true, false, &now, false), (Seat::Ordered(7, false), Some(SeatOrder::Sit(7, false))));
    }

    /// A reset of an actor not yet on its chair takes the order back (it is given again
    /// through `Wanted`); one not drawn, or standing with no furniture, is left as it is;
    /// another chair wanted is the usual getting up.
    #[test]
    fn a_reset_off_the_chair_or_unseen_orders_nothing_new() {
        use super::{next_seat, retry::Now, Seat, SeatOrder};
        let now = Now { wants: Some(7), eligible: true, seconds: 0.1, changed: false };
        assert_eq!(next_seat(Seat::Ordered(7, false), true, false, &now, true), (Seat::None, Some(SeatOrder::Cancel)));
        assert_eq!(next_seat(Seat::Ordered(7, true), false, false, &now, true), (Seat::Wanted(7, true), None));
        assert_eq!(next_seat(Seat::None, true, false, &Now { wants: None, ..now }, true), (Seat::None, None));
        let other = Now { wants: Some(8), ..now };
        assert_eq!(next_seat(Seat::Ordered(7, true), true, true, &other, true), (Seat::GettingUp(7), Some(SeatOrder::GetUp)));
        assert_eq!(next_seat(Seat::Ordered(7, true), true, true, &other, false), (Seat::GettingUp(7), Some(SeatOrder::GetUp)));
    }

    /// Review of #2353: a resident that was seated (its visual gone, so `Wanted` at once)
    /// and is put at a standing place by a jump of the clock while the console stays open
    /// (no `note` to refresh what it wants) ends standing: it is not sat back on its old
    /// furniture once drawn. One put on new furniture is sat on that at once.
    #[test]
    fn a_seated_resident_put_at_a_standing_place_by_a_jump_is_not_seated_again() {
        use super::{next_seat, retry::Now, seat_after_jump, stale_seat, Seat};
        for before in [Seat::Wanted(7, true), Seat::Ordered(7, true)] {
            // The jump's frame: nothing to use at the new place.
            let (wants, seat) = seat_after_jump(before, None);
            assert_eq!(wants, None);
            // The same frame's cleanup (`Resident::jumped`) drops the old seat.
            assert_eq!(stale_seat(seat, wants, true), Some(7));
            let mut seat = Seat::None;
            // Held by the console for a while, then drawn and free: never told to sit.
            for (drawn, eligible) in [(false, false), (false, false), (true, false), (true, true), (true, true)] {
                let frame = Now { wants, eligible, seconds: 0.1, changed: false };
                let (next, order) = next_seat(seat, drawn, false, &frame, false);
                assert_eq!(order, None, "{before:?}");
                seat = next;
            }
            assert_eq!(seat, Seat::None, "{before:?}");
        }
        // Before the fix the old wish stayed: drawn, it would be sat on its old seat again.
        let stale = Now { wants: Some(7), eligible: true, seconds: 0.1, changed: false };
        assert!(next_seat(Seat::Wanted(7, true), true, false, &stale, false).1.is_some());
        // To new furniture: on it at once, and the old seat is not left (it is replaced).
        let (wants, seat) = seat_after_jump(Seat::Wanted(7, true), Some(9));
        assert_eq!((wants, seat), (Some(9), Seat::Wanted(9, true)));
        assert_eq!(stale_seat(seat, wants, true), None);
        // Nothing changed this frame: no seat is dropped.
        assert_eq!(stale_seat(Seat::Ordered(7, true), None, false), None);
    }

    #[test]
    fn refused_skips_add_up_over_one_blocked_episode_only() {
        use super::{SkipRefusals, SKIP_REFUSED_FORGET};
        let mut r = SkipRefusals::default();
        // A second apart, whatever paths were planned meanwhile.
        assert_eq!((r.note(1, 10.0), r.note(1, 11.0), r.note(1, 12.0)), (1, 2, 3));
        assert_eq!(r.note(2, 12.0), 1, "each actor its own");
        // Its body made again or moved, or moved on: a new episode. The summary's count stays.
        r.forget(1);
        assert_eq!(r.note(1, 13.0), 1);
        // Walking long enough since the last one: a new episode too.
        assert_eq!(r.note(1, 14.0), 2);
        assert_eq!(r.note(1, 14.0 + SKIP_REFUSED_FORGET + 1.0), 1);
        assert_eq!(r.total, 7);
    }

    /// The maintainer's review of #2143: a walk under way at Speed Mult 0 (speeds of 0, the
    /// walk kept) stands and is never moved on along its path; one slowed under a fifth of
    /// the speed its walk started at is judged by its new speed; and a rule made while the
    /// speed was 0 still finds the actor stuck once its speed is back. Each frame as
    /// `PhysMover::walk` asks it: no advice at 0, else `advise_moved` with the feet.
    #[test]
    fn a_walk_at_speed_zero_is_never_moved_on_along_its_path() {
        use super::stuck_rule;
        use fnv_nav::follow::Advice;
        let dt = 1.0 / 60.0;
        let mut rules = HashMap::new();
        // Frames at `speed` (the feet moving at `moves` units/s along +x): any skip advised.
        let frames = |rules: &mut HashMap<u32, fnv_nav::follow::Unstuck>, x: &mut f32, speed: f32, moves: f32, seconds: f32| {
            let mut skipped = false;
            for _ in 0..(seconds / dt).round() as usize {
                *x += moves * dt;
                if let Some(u) = stuck_rule(rules, 7, speed) {
                    skipped |= matches!(u.advise_moved([*x, 0.0, 0.0], [1.0, 0.0], dt), Advice::Skip | Advice::GiveUp);
                }
            }
            skipped
        };
        let mut x = 0.0;
        assert!(!frames(&mut rules, &mut x, 100.0, 100.0, 2.0));
        // SetAV SpeedMult 0 mid-walk, held ten seconds: the rule is not asked, its speed is 0.
        assert!(!frames(&mut rules, &mut x, 0.0, 0.0, 10.0));
        assert_eq!((rules[&7].speed, rules[&7].detours, rules[&7].replans), (0.0, 0, 0));
        // Back to 100, then down to 15 (under a fifth of 100) and walking at 15.
        assert!(!frames(&mut rules, &mut x, 100.0, 100.0, 3.0));
        assert!(!frames(&mut rules, &mut x, 15.0, 15.0, 5.0));
        assert_eq!((rules[&7].detours, rules[&7].replans), (0, 0));
        // A rule first made at 0 finds a blocked actor stuck once its speed is back.
        let mut late = HashMap::new();
        let mut y = 0.0;
        assert!(!frames(&mut late, &mut y, 0.0, 0.0, 3.0));
        assert!(frames(&mut late, &mut y, 100.0, 0.0, 10.0), "stuck at 100 and moved on");
    }

    #[test]
    fn private_world_prefetch_matches_resident_spawn_even_with_a_shared_world() {
        use super::{Clock, Residents};
        use crate::outfits::tests::{actor_forms, appearance_world, empty_world_data, equipped, game, OTHER};
        let world = appearance_world();
        let forms = Box::leak(Box::new(actor_forms(vec![])));
        let refs = Box::leak(Box::new(fnv_sim::RefTable::default()));
        let mut private = fnv_sim::World::new(forms, refs, Default::default());
        let mut shared = fnv_sim::World::new(forms, refs, Default::default());
        let id = private.place(OTHER, None, [0.0; 3], [0.0; 3], 1).unwrap();
        assert_eq!(shared.place(OTHER, None, [0.0; 3], [0.0; 3], 1).unwrap(), id);
        private.get(id).unwrap().inventory = Some(equipped());
        let outsider = private.place(OTHER, None, [0.0; 3], [0.0; 3], 1).unwrap();
        assert_eq!(shared.place(OTHER, None, [0.0; 3], [0.0; 3], 1).unwrap(), outsider);
        private.get(outsider).unwrap().inventory = Some(equipped());
        let mut residents = Residents::build(&empty_world_data(), &[], Clock { hour: 12.0, days_passed: 1.0, rate: 0.0 }, 100.0, true).unwrap();
        residents.own = Some(Box::new(game(private)));
        residents.core.known.insert(id.0);
        let private = &residents.own.as_ref().unwrap().world;
        let plan = crate::readahead::actor_plan(&world, residents.prefetch_world(id.0, Some(&shared)), id.0, OTHER.0, false).unwrap();
        assert_eq!(plan, crate::readahead::actor_plan(&world, Some(private), id.0, OTHER.0, false).unwrap());
        assert_ne!(plan, crate::readahead::actor_plan(&world, Some(&shared), id.0, OTHER.0, false).unwrap());
        assert!(plan.parts.iter().any(|p| p.ends_with("modded.nif")));
        assert!(std::ptr::eq(residents.prefetch_world(id.0, None).unwrap(), private));
        // Even a reference present in the private simulation is not a resident until
        // it is in known. The placed-reference prefetch must use the shared world.
        assert!(std::ptr::eq(residents.prefetch_world(outsider.0, Some(&shared)).unwrap(), &shared));
        assert!(residents.prefetch_world(outsider.0, None).is_none());
        residents.own = None;
        assert!(std::ptr::eq(residents.prefetch_world(id.0, Some(&shared)).unwrap(), &shared));
        assert!(residents.prefetch_world(id.0, None).is_none());
    }

    /// Under the start's screen each resident not handed to the AI is drawn where
    /// `fnv_ai::Ai::add` puts its walker when the first frame of play hands it over (where
    /// the game has it, facing its way), whether the game has it in the world or not; one
    /// handed over already, or one that failed, is not.
    #[test]
    fn under_the_start_screen_residents_are_drawn_where_the_ai_will_take_them_up() {
        use super::{Clock, Residents, StartBody};
        use crate::outfits::tests::{actor_forms, empty_world_data, game, OTHER};
        use fnv_nav::Space;
        let forms: &'static fnv_sim::FormStore = Box::leak(Box::new(actor_forms(vec![])));
        let refs: &'static fnv_sim::RefTable = Box::leak(Box::new(fnv_sim::RefTable::default()));
        let mut world = fnv_sim::World::new(forms, refs, Default::default());
        let room = FormId(0x500);
        let (a_at, b_at, c_at) = ([10.0, 20.0, 30.0], [-40.0, 5.0, 0.0], [1.0, 1.0, 1.0]);
        let a = world.place(OTHER, Some(room), a_at, [0.0, 0.0, 1.25], 1).unwrap();
        let b = world.place(OTHER, Some(room), b_at, [0.0, 0.0, -2.0], 1).unwrap();
        let c = world.place(OTHER, Some(room), c_at, [0.0; 3], 1).unwrap();
        let failed = world.place(OTHER, Some(room), c_at, [0.0; 3], 1).unwrap();
        world.set_enabled(b, false).unwrap();
        let g = game(world);
        let mut residents = Residents::build(&empty_world_data(), &[], Clock { hour: 22.0, days_passed: 5.0, rate: 0.0 }, 100.0, true).unwrap();
        for (id, at) in [(a, a_at), (b, b_at), (c, c_at), (failed, c_at)] {
            residents.core.add_resident(id, OTHER.0, false, 1.0, at, id.to_string(), false);
        }
        residents.core.list[2].joined = true;
        residents.core.list[3].failed = true;
        let bodies = residents.core.start_bodies(&g);
        assert_eq!(
            bodies,
            vec![
                StartBody { index: 0, space: Space::Interior(room.0), feet: a_at, heading: 1.25, present: true },
                StartBody { index: 1, space: Space::Interior(room.0), feet: b_at, heading: -2.0, present: false },
            ]
        );
        // The AI takes them up there on the first frame of play.
        for (id, body) in [(a, bodies[0]), (b, bodies[1])] {
            residents.core.ai.add(&g, id).unwrap();
            let w = &residents.core.ai.agents[&id].walker;
            assert_eq!((w.space, w.feet, w.heading), (body.space, body.feet, body.heading), "{id}");
        }
    }

    /// One frame of drawing under the start's screen: the residents in view in order,
    /// within the frame's time (at least one a frame), those whose parts are not read yet
    /// asked again the next frame and still waited for, one that cannot be drawn not
    /// waited for; nothing before it may begin (the cells, the scripts, the aid off, the cap).
    #[test]
    fn the_start_screen_draws_its_residents_within_a_frame_budget() {
        use super::{place_frame, Placed, START_PLACE_BUDGET};
        use std::time::Duration;
        let want = [1, 2, 3, 4, 5];
        // Quick: everyone in one frame.
        let mut asked = Vec::new();
        assert_eq!(place_frame(&want, true, || Duration::ZERO, |i| { asked.push(i); Placed::Drawn }), (5, 0));
        assert_eq!(asked, want);
        // Not ready, and one that cannot be drawn: 3 left to wait for.
        let fate = |i: i32| match i {
            2 => Placed::Never,
            4 | 5 => Placed::NotYet,
            _ => Placed::Drawn,
        };
        assert_eq!(place_frame(&want, true, || Duration::ZERO, fate), (2, 2));
        // Slow: each spawn 40 ms, so three a frame within 100 ms; the rest the next frame.
        let spent = std::cell::Cell::new(Duration::ZERO);
        let mut asked = Vec::new();
        let step = START_PLACE_BUDGET / 5 * 2;
        let frame = place_frame(&want, true, || spent.get(), |i| {
            asked.push(i);
            spent.set(spent.get() + step);
            Placed::Drawn
        });
        assert_eq!((frame, asked), ((3, 2), vec![1, 2, 3]));
        // One spawn longer than the budget: still one a frame.
        assert_eq!(place_frame(&want[3..], true, || START_PLACE_BUDGET * 3, |_| Placed::Drawn), (1, 1));
        // Not yet allowed: nobody drawn, all of them still to draw (the screen waits).
        assert_eq!(place_frame(&want, false, || Duration::ZERO, |_| panic!("drawn before it may")), (0, 5));
        assert_eq!(place_frame::<i32>(&[], true, || Duration::ZERO, |_| Placed::Drawn), (0, 0));
    }

    /// Test aid `NVF_START_ACTORS_UNDER_SCREEN=0` draws the start's residents after its
    /// screen, as before; anything else (unset, `1`) draws them under it.
    #[test]
    fn the_aid_draws_the_starts_residents_after_its_screen() {
        use super::start_actors_under_screen;
        assert!(start_actors_under_screen(None));
        assert!(start_actors_under_screen(Some("1")));
        assert!(!start_actors_under_screen(Some("0")));
        assert!(!start_actors_under_screen(Some(" 0 ")));
    }

    /// A game restored from a save keeps the save's clock; a new game takes the start
    /// clock, which is how `--hour` and `--day` reach it. The package interval is set for
    /// both (#1607).
    #[test]
    fn only_a_new_game_takes_the_start_clock() {
        use super::{clock, set_global, Clock, Residents, PACKAGE_INTERVAL};
        use crate::outfits::tests::{actor_forms, empty_world_data, game};
        let mut forms = actor_forms(vec![]);
        for (id, edid) in [(0x200, clock::GAME_HOUR), (0x201, clock::GAME_DAYS_PASSED)] {
            forms.edids.insert(edid.to_ascii_lowercase(), FormId(id));
            forms.globals.insert(FormId(id), 0.0);
        }
        let forms: &'static fnv_sim::FormStore = Box::leak(Box::new(forms));
        let refs: &'static fnv_sim::RefTable = Box::leak(Box::new(fnv_sim::RefTable::default()));
        for (restored, hour, days) in [(true, 20.0, 25.0), (false, 9.0, 5.0)] {
            let mut g = game(fnv_sim::World::new(forms, refs, Default::default()));
            set_global(&mut g, clock::GAME_HOUR, 20.0).unwrap();
            set_global(&mut g, clock::GAME_DAYS_PASSED, 25.0).unwrap();
            let mut residents = Residents::build(&empty_world_data(), &[], Clock { hour: 9.0, days_passed: 5.0, rate: 0.0 }, 100.0, true).unwrap();
            residents.core.prepare(&mut g, restored);
            assert_eq!(g.world.global(FormId(0x200)).unwrap(), hour);
            assert_eq!(g.world.global(FormId(0x201)).unwrap(), days);
            assert_eq!(g.cfg.package_interval, Some(PACKAGE_INTERVAL));
        }
    }

    /// Out of a house and straight back: the residents seen in the street before are not
    /// moved by the trip back; those not seen for longer than the memory are (coordinator
    /// review of #1269).
    #[test]
    fn residents_seen_shortly_before_a_trip_stay_where_they_are() {
        let (street, earlier, never) = (FormId(0x100), FormId(0x101), FormId(0x102));
        // GameDaysPassed 5 at 12:00, as game hours; the street seen until 12:00, the
        // other a day before.
        let at: HashMap<FormId, f64> = [(street, 5.5 * 24.0), (earlier, 4.5 * 24.0)].into();
        // Out to Doc Mitchell's and back 20 game minutes later.
        let back = 5.5 * 24.0 + 20.0 / 60.0;
        let kept = recently_loaded(&at, back);
        assert!(kept.contains(&street));
        assert!(!kept.contains(&earlier) && !kept.contains(&never));
        // A fast travel's hours later, nobody of the place left is kept.
        assert!(recently_loaded(&at, 5.5 * 24.0 + RECENTLY_LOADED_HOURS + 0.1).is_empty());
    }

    fn ends(frames: &[bool]) -> Vec<RunEnd> {
        let mut run = StopRun::default();
        frames.iter().filter_map(|&moving| run.frame(moving)).collect()
    }

    #[test]
    fn standing_before_the_first_step_is_no_stop() {
        // Two frames waiting for the character's first physics step, then a walk.
        assert_eq!(ends(&[false, false, true, true, true]), vec![RunEnd::SetOff]);
        // A spot turn of 12 frames before setting off.
        let mut turn = vec![false; 12];
        turn.extend([true, true]);
        assert_eq!(ends(&turn), vec![RunEnd::SetOff]);
        assert_eq!(ends(&[true, true]), vec![]);
    }

    #[test]
    fn standing_between_two_frames_walking_is_a_stop() {
        assert_eq!(ends(&[true, false, true]), vec![RunEnd::Stop(1)]);
        assert_eq!(ends(&[false, true, false, false, false, true, false, false, false, false, true]), vec![RunEnd::SetOff, RunEnd::Stop(3), RunEnd::Stop(4)]);
        // Standing at the end of a walk ends nothing.
        assert_eq!(ends(&[true, true, false, false]), vec![]);
    }

    /// The original's Victor walked out of the loaded area from process level 0 straight to
    /// 3, never 1 (#958): a High actor stepping out into a cell the buffer does not hold
    /// does not carry the cell, so it is Low on the next pass.
    #[test]
    fn walking_out_of_the_loaded_area_drops_to_low() {
        // The frame it steps out: it was High, the new cell is neither drawn nor buffered.
        assert!(!carries_cell(false, false, true, false, ProcessLevel::High));
        assert!(!carries_cell(false, false, false, false, ProcessLevel::High));
        assert_eq!(level_for(false, false, false), ProcessLevel::Low);
        // Into a cell the buffer already holds: Mid-High.
        assert_eq!(level_for(false, false, true), ProcessLevel::MidHigh);
    }

    #[test]
    fn mid_high_walking_on_into_another_cell_takes_it_along() {
        assert!(carries_cell(false, false, true, false, ProcessLevel::MidHigh));
        // Same cell, a held cell, or back in the loaded area: nothing to carry.
        assert!(!carries_cell(false, false, false, false, ProcessLevel::MidHigh));
        assert!(!carries_cell(false, true, true, false, ProcessLevel::MidHigh));
        assert!(!carries_cell(true, false, true, false, ProcessLevel::MidHigh));
        // Low actors walking about beyond the buffer never pull cells in.
        assert!(!carries_cell(false, false, true, false, ProcessLevel::Low));
        assert_eq!(level_for(true, false, false), ProcessLevel::High);
        assert_eq!(level_for(false, true, false), ProcessLevel::Coming);
    }

    /// Interiors behind the loaded area's doors read Mid-Low in the original (audit §3.1):
    /// Benny riding the lift from the casino into the presidential suite keeps the suite.
    #[test]
    fn going_through_a_door_into_an_interior_takes_it_along() {
        assert!(carries_cell(false, false, true, true, ProcessLevel::High));
        assert!(carries_cell(false, false, false, true, ProcessLevel::High));
        // Already Low, or the interior is held or loaded: nothing to carry.
        assert!(!carries_cell(false, false, true, true, ProcessLevel::Low));
        assert!(!carries_cell(false, true, true, true, ProcessLevel::High));
        assert!(!carries_cell(true, false, true, true, ProcessLevel::High));
    }

    /// A resident's body drawn by `nav::show_actor`, one frame (a system run): the
    /// resident goes through a load door first when `door` (`hand_off_body`, as `drive`'s
    /// door loop). Its parts are read at once (`at_once`), so a body hand-over spawns the
    /// new body in the same frame.
    mod bodies {
        use bevy::ecs::system::RunSystemOnce;
        use bevy::prelude::*;
        use nvf_play::RefVisual;

        pub(super) const BOYD: u32 = 0x000F_0678;

        pub(super) fn frame(app: &mut App, world_root: Entity, state: (bool, Option<Entity>), door: bool, feet: [f32; 3]) -> (bool, Option<Entity>) {
            app.world_mut()
                .run_system_once(
                    move |mut commands: Commands,
                          aw: Res<crate::actors::ActorWorld>,
                          mut ahead: ResMut<crate::readahead::ReadAhead>,
                          mut parts: ResMut<nvf_assets::actor::ActorParts>,
                          mut models: nvf_assets::model::ModelCtx,
                          roots: Query<(Entity, &RefVisual, &ChildOf)>,
                          mut actors: Query<(&mut Transform, Option<&mut crate::locomotion::Locomotion>), Without<crate::furniture::OnFurniture>>| {
                        let (mut spawned, mut root) = state;
                        if door {
                            super::super::hand_off_body(&mut commands, (&mut spawned, &mut root), 1.0);
                        }
                        let mut stats = crate::actors::ActorStats::default();
                        let who = crate::nav::Who { reference: BOYD, base: crate::outfits::tests::OTHER.0, creature: false, scale: 1.0, strafe: false, air: false, jumped: false, at_once: true };
                        crate::nav::show_actor(&mut commands, world_root, true, Some(&aw), None, &mut ahead, &mut stats, &mut parts, &mut models, &roots, &mut actors, who, (&mut spawned, &mut root), (feet, 0.0), false, 1.0 / 60.0).unwrap();
                        (spawned, root)
                    },
                )
                .unwrap()
        }

        /// The app with the actor fixture (`outfits::tests::asset_fixture`) and a world root.
        pub(super) fn app() -> (App, Entity, crate::outfits::tests::TestFiles) {
            let (mut app, files) = crate::outfits::tests::asset_fixture();
            app.init_resource::<crate::readahead::ReadAhead>();
            let world_root = app.world_mut().spawn(Transform::default()).id();
            (app, world_root, files)
        }

        /// The bodies of `BOYD` (`ActorRig`) and those of them its `RefVisual` marks
        /// under the world root (the root scan's candidates).
        pub(super) fn bodies(app: &mut App, world_root: Entity) -> (Vec<Entity>, Vec<Entity>) {
            let w = app.world_mut();
            let rigs = w.query::<(Entity, &crate::limbs::ActorRig)>().iter(w).filter(|(_, r)| r.reference == BOYD).map(|(e, _)| e).collect();
            let marked = w.query::<(Entity, &RefVisual, &ChildOf)>().iter(w).filter(|(_, v, p)| v.0 == BOYD && p.parent() == world_root).map(|(e, ..)| e).collect();
            (rigs, marked)
        }
    }

    /// #2461: a resident handed through a load door with its parts read spawns its new
    /// body in the same frame as the old one is let go. Its root is the new body, not the
    /// one fading at the door (which still had its `RefVisual` in that frame), and it is
    /// the only body of the reference afterwards; once the faded body is despawned the
    /// resident is still driven by the new one.
    #[test]
    fn a_resident_handed_through_a_load_door_keeps_one_body() {
        use bevy::prelude::{Transform, Vec3};
        use bodies::{app, bodies, frame};
        let (mut app, world_root, _files) = app();
        let state = frame(&mut app, world_root, (false, None), false, [470.0, 1499.0, 18672.0]);
        let (true, Some(first)) = state else { panic!("not drawn: {state:?}") };
        let state = frame(&mut app, world_root, state, false, [471.0, 1490.0, 18672.0]);
        assert_eq!(state, (true, Some(first)), "drawn and placed by the same body");
        assert_eq!(bodies(&mut app, world_root), (vec![first], vec![first]));
        // Through the door: the new body spawns at the far side in the same frame.
        let state = frame(&mut app, world_root, state, true, [466.0, 1378.0, 18672.0]);
        let (true, Some(second)) = state else { panic!("not drawn anew: {state:?}") };
        assert_ne!(second, first, "the root is the new body, not the one left at the door");
        assert_eq!(bodies(&mut app, world_root), (vec![second], vec![second]), "one body of the reference, and it is the root");
        let w = app.world();
        assert!(w.get::<crate::door_fade::DoorFade>(first).is_some_and(|f| f.out), "the body left at the door fades out");
        assert!(w.get::<nvf_play::RefVisual>(first).is_none());
        // `door_fade.rs` despawns the faded body; the new one goes on being driven.
        app.world_mut().despawn(first);
        let state = frame(&mut app, world_root, state, false, [466.0, 1370.0, 18672.0]);
        assert_eq!(state, (true, Some(second)));
        assert_eq!(app.world().get::<Transform>(second).unwrap().translation, Vec3::new(466.0, 1370.0, 18672.0), "placed at the walker's feet");
        assert_eq!(bodies(&mut app, world_root), (vec![second], vec![second]));
    }

    /// #2461: a root whose body is gone is let go: another body of the reference under the
    /// world root is taken up, and with none the resident is drawn anew.
    #[test]
    fn a_resident_whose_body_is_gone_is_drawn_anew_or_takes_up_another() {
        use bevy::prelude::{Transform, Vec3};
        use bodies::{app, bodies, frame};
        let (mut app, world_root, _files) = app();
        let feet = [296.0, 1665.0, 18672.0];
        let state = frame(&mut app, world_root, (false, None), false, feet);
        let (true, Some(first)) = state else { panic!("not drawn: {state:?}") };
        app.world_mut().despawn(first);
        let state = frame(&mut app, world_root, state, false, feet);
        assert_eq!(state, (false, None), "the lost root is let go and nothing else found: drawn anew next");
        let state = frame(&mut app, world_root, state, false, feet);
        let (true, Some(second)) = state else { panic!("not drawn anew: {state:?}") };
        assert_ne!(second, first);
        let state = frame(&mut app, world_root, state, false, feet);
        assert_eq!(state, (true, Some(second)));
        assert_eq!(bodies(&mut app, world_root), (vec![second], vec![second]));
        // Another body of the same reference stands under the world root (one left
        // standing): the resident's body gone, that one is taken up rather than a third
        // drawn.
        let (true, Some(stray)) = frame(&mut app, world_root, (false, None), false, [325.0, 719.0, 18672.0]) else { panic!("stray not drawn") };
        app.world_mut().despawn(second);
        let state = frame(&mut app, world_root, state, false, feet);
        assert_eq!(state, (true, Some(stray)));
        assert_eq!(app.world().get::<Transform>(stray).unwrap().translation, Vec3::from_array(feet));
        assert_eq!(bodies(&mut app, world_root), (vec![stray], vec![stray]));
    }
}
