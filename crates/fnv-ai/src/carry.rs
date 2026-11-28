//! Carrying packages out: each actor the AI looks after goes where its current package
//! sends it ([`crate::goal`]), over the navmesh, and the game is told when it is there
//! (`Game::package_goal_met`). A package whose location cannot be reached is set aside
//! for a while and the actor chooses again (`Game::block_package`).
//!
//! A Dialogue package ([`goal::Talk`]) is carried out here too: the actor waits where
//! the package has it wait, goes up to its target when the target is where the package
//! says, and starts the conversation with the player (`Game::force_greet`: a greeting
//! with the Goodbye flag is a conversation too) or says its line (`Game::say`);
//! docs/design/actor-behaviour.md §5.6.
//!
//! Movement here is `fnv_nav::Walker` (on the navmesh surface, no physics): the engine
//! moves actors near the player with collision instead and uses this for the rest.
//!
//! Escort, Find, Use Item At, Ambush and Flee Not Combat packages are carried out in
//! `crate::errand`, on the walks planned here.
//!
//! An actor that stands ([`Ai::stands`]) can be pushed aside by one that walks into it
//! ([`Ai::push`]: over the navmesh, as far as it has room); left alone again it goes
//! back to where its package has it (docs/design/actor-behaviour.md §5.7).

use crate::alert::Search;
use crate::goal::{self, Goal, Route, Talk};
use crate::sandbox::{Doing, Sandbox};
use fnv_game::ai::AiPhase;
use fnv_game::companion_wheel::{BACK_UP, BACK_UP_SECONDS};
use fnv_game::game::Game;
use fnv_game::package::{fallout, flags, PackageType};
use fnv_game::use_weapon::Source;
use fnv_nav::follow::heading_of;
use fnv_nav::{NavGraph, PathError, PathParams, Place, Space, WalkEvent, Walker};
use fnv_sim::FormId;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct AiConfig {
    /// A person's walk and run in light armour at Speed Mult 100, game units per second
    /// ([`NPC_WALK`], [`NPC_RUN`]); each actor's own are set by the engine
    /// ([`Ai::set_speeds`]: creatures, armour, Speed Mult).
    pub walk_speed: f32,
    pub run_speed: f32,
    /// A place counts as reached within this distance even when the package's radius is
    /// smaller [our choice; the game's value is not known].
    pub arrive: f32,
    /// How long an unreachable package is set aside, game hours [our choice].
    pub retry_hours: f32,
    pub params: PathParams,
    /// Actors standing or walking inside each other are moved apart ([`Ai::keep_apart`],
    /// `crate::apart`) [design].
    pub keep_apart: bool,
    /// Test aid: random conversations start anywhere in the loaded area, not only next to
    /// the player (`crate::social`; the engine's `NVF_PLAY_TEST_CHATS_ANYWHERE`). Off in
    /// play.
    pub chats_anywhere: bool,
    /// Test aid: every Find package allows buying (`crate::errand`; the engine's
    /// `NVF_PLAY_TEST_FIND_BUYING`), as no shipped Find package has the flag. Off in play.
    pub find_buying: bool,
}

/// An actor without a package walks back to where it was placed from at most this far,
/// game units [design: a cell; farther away a script has put it there].
const HOME_REACH: f32 = 4096.0;

/// What a unit of water costs on a way planned for an actor whose package does not
/// allow swimming (`Ai::path_params`), against 1 on land [our choice].
pub const NO_SWIM_WATER_COST: f32 = 50.0;

/// Fallback safe drop height [design, #2532; documented: physics.md §6.10 and
/// engine/nvf-play/src/falls.rs use 600 when fJumpFallHeightMin is absent].
const SAFE_DROP_HEIGHT: f32 = 600.0;

/// A way through a radiation marker's radius costs this many times as much for an actor
/// that avoids radiation [design: kept off where a way round is not far longer, still
/// taken when there is none].
pub const AVOID_RADIATION_COST: f32 = 8.0;
/// `fRadiationResistanceAvoidLevel` when the load order does not set it [its value in
/// FalloutNV.esm].
const RAD_AVOID_LEVEL: f32 = 50.0;
/// Markers this far from an actor are left out of its paths' costs, game units [design:
/// a long way; paths farther are rare].
const RADIATION_SEARCH: f32 = 20000.0;

/// Why a package's place cannot be reached, for the log ([`AiEvent::Unreachable`]): the
/// path error, then the doors that close the way (`fnv_nav::NavGraph::doors_closing`,
/// each with its lock as the game holds it), or that no door at all joins the two places
/// (`NavGraph::joined_by_doors`) [design, #2405: "0 triangles searched" alone was read as
/// a missing navmesh, for a locked door in the same room and for a sealed scene cell
/// alike]. With the doors named. A place with no navmesh near it is named with its cell
/// or worldspace [design, #2756: Jean-Baptiste's "(0, 0, 0)", logged while he was in
/// Freeside, read as a goal looked for in his own worldspace; it is his marker's place in
/// FreesideSilverRush, the data's own].
fn why_unreachable(game: &Game, nav: &NavGraph, from: Place, to: Place, params: &PathParams, e: &PathError) -> (String, Vec<u32>) {
    let mut why = e.to_string();
    if let PathError::NoTriangle { which, .. } = e {
        let space = if *which == "start" { from.space } else { to.space };
        let (fnv_nav::Space::World(f) | fnv_nav::Space::Interior(f)) = space;
        why.push_str(&format!(" in {}", game.data.name(FormId(f))));
    }
    if !matches!(e, PathError::Unreachable { .. }) {
        return (why, Vec::new());
    }
    let mut closing = Vec::new();
    match nav.doors_closing(from, to, params) {
        Some(doors) if !doors.is_empty() => {
            let named: Vec<String> = doors
                .iter()
                .map(|&d| {
                    let what = match game.world.lock_of(FormId(d)).filter(|l| l.locked) {
                        Some(l) if l.key.is_some() => format!("locked, level {}, a key it lacks", l.level),
                        Some(l) => format!("locked, level {}", l.level),
                        None if params.refused_doors.contains(&d) => "its script refused it".to_string(),
                        None => "closed to it".to_string(),
                    };
                    format!("{} ({what})", FormId(d))
                })
                .collect();
            why.push_str(&format!("; the way is closed by door {}", named.join(", ")));
            closing = doors;
        }
        Some(_) => {}
        None if nav.joined_by_doors(from, to) == Some(false) => why.push_str("; no door at all joins the two places"),
        None => {}
    }
    (why, closing)
}

/// The radiation markers of `space` within [`RADIATION_SEARCH`] of `at`, as circles (centre,
/// radius) for the path parameters, disabled ones left out; then the explosion zones there
/// that have not faded ([`zones_near`]).
fn radiated_near(game: &Game, space: fnv_nav::Space, at: [f32; 3]) -> Vec<([f32; 3], f32)> {
    let key = match space {
        fnv_nav::Space::World(w) | fnv_nav::Space::Interior(w) => FormId(w),
    };
    let mut out: Vec<([f32; 3], f32)> = game
        .radiation_markers_in(key)
        .iter()
        .filter(|m| m.radius > 0.0 && goal::dist(m.pos, at) <= RADIATION_SEARCH + m.radius && game.world.is_enabled(m.id).unwrap_or(true))
        .map(|m| (m.pos, m.radius))
        .collect();
    out.extend(zones_near(game.radiation_zones(), key, at));
    out
}

/// The radiation zones explosions left in `space` (`Game::radiation_zones`) within
/// [`RADIATION_SEARCH`] of `at` that still give rads, as circles (centre, radius), costed
/// as a marker's are [design: issue #963; a zone fades in seconds, the Fat Man's over 12,
/// so a way planned before it went off is not planned again for it].
fn zones_near(zones: &[fnv_game::radiation::RadZone], space: FormId, at: [f32; 3]) -> impl Iterator<Item = ([f32; 3], f32)> + '_ {
    zones
        .iter()
        .filter(move |z| z.space == space && z.radius > 0.0 && z.level_now() > 0.0 && goal::dist(z.pos, at) <= RADIATION_SEARCH + z.radius)
        .map(|z| (z.pos, z.radius))
}

/// A person's walk, game units per second: the root motion of the people's walk clip
/// (`mtforward.kf`, 85.26 male, 85.06 female) [observed: the AI observer in the original
/// game, 2026-10-16 to 10-24 (docs/spec-notes/actors.md §8.10): NPCs in light armour
/// walking holstered held a median 85.5 (the medians of 40 actors, 76–86, most 85–86); the
/// player walked 86.8 (physics.md §6.3), which NPCs used before; `fMoveBaseSpeed` (77) is
/// not it: the Novac caravan merchant's 77 was a walk with its weapon drawn].
pub const NPC_WALK: f32 = 85.3;

/// A person's run, game units per second: the player's observed run (physics.md §6.3)
/// [observed: light-armoured NPCs ran 337–344 (VESGSGeckoman), and the slower runs fit it
/// with their penalties: fiends with a two-handed bat drawn 309.6 (344 × 0.9), a
/// medium-armoured initiate 316 (× 0.925), Cook-Cook in metal armour with the
/// Incinerator 255 (× (1 − 0.15 − 0.1) = 258)].
pub const NPC_RUN: f32 = 344.0;

/// fMoveSneakMult when the load order does not set it [observed: FalloutNV.esm].
const SNEAK_MULT: f32 = 0.57;

/// fMoveNoWeaponMult when the load order does not set it [documented: GECK wiki Movement
/// Settings, default 1.1, "Movement speed is multiplied by this when the actor's weapon is
/// holstered, or the actor has no weapon equipped"].
const NO_WEAPON_MULT: f32 = 1.1;

/// The weapons, by WEAP DNAM animation type, a person walks with drawn at the drawn walk
/// ([`drawn_walk_share`]): Pistol - Ballistic (1 Hand) 3 and Pistol - Energy (1 Hand) 4
/// [documented: xEdit wbWeaponAnimTypeEnum, schemas/fnv/objects.json]. [observed: walks
/// with a 10mm pistol drawn only, actors.md §8.10, People; the energy pistols share its
/// stance's clips, not measured.] [design, #2124: every other stance keeps the holstered
/// walk until it is measured; docs/owner-review.md.]
pub const DRAWN_WALK_ANIM_TYPES: [i64; 2] = [3, 4];

/// The share of its walk a person keeps walking outside combat with a weapon drawn
/// (#2124): 1 / fMoveNoWeaponMult (85.3 / 1.1 = 77.5 units/s) for a person, not a
/// creature, in light armour or clothes (`fnv_game::combat::worn_armor_class`) with a
/// weapon of [`DRAWN_WALK_ANIM_TYPES`] out (`Game::weapon_is_out`) and not in combat; 1
/// for everyone else. [observed: walking with a 10mm pistol drawn, in leather or a
/// mercenary outfit, the Novac merchant held 78.0 and two mercenaries 77.5 and 78.3,
/// against the holstered people's 85 (docs/spec-notes/actors.md §8.10, People).]
/// [documented: fMoveNoWeaponMult multiplies the speed of an actor whose weapon is
/// holstered, GECK wiki Movement Settings.] [design, #2124: our holstered walk
/// ([`NPC_WALK`]) is fitted to the holstered people, so the drawn walk divides it by the
/// setting; whether the original's drawn walk comes from the setting, the `1hm`/`h2h`
/// walk clip (77.5) or fMoveBaseSpeed (77) is open, the maintainer's choice in
/// docs/owner-review.md.]
pub fn drawn_walk_share(game: &Game, actor: FormId) -> f32 {
    if game.awareness.state(actor).in_combat() || !game.weapon_is_out(actor) {
        return 1.0;
    }
    // The weapon it holds by the rules (none: its fists); only an actor the simulation
    // holds no equipment for falls back on the one the animating side last drew for it,
    // which is not cleared when the weapon is taken off (`Game::set_held_weapon`).
    let weapon = match game.world.equipment(actor) {
        Some(items) => items.into_iter().find(|i| game.world.forms.item(*i).is_some_and(|it| it.kind == fnv_sim::forms::ItemKind::Weapon)),
        None => game.extra.get(&actor).and_then(|e| e.held_weapon),
    };
    let stance = weapon.and_then(|w| game.data.weapon_anim_types.get(&w).copied());
    let light = || fnv_game::combat::worn_armor_class(&game.world, actor) == fnv_sim::forms::ArmorClass::Light;
    if !stance.is_some_and(|t| DRAWN_WALK_ANIM_TYPES.contains(&t)) || crate::alert::creature(game, actor) || !light() {
        return 1.0;
    }
    1.0 / game.world.forms.setting_f32("fMoveNoWeaponMult").filter(|m| *m > 0.0).unwrap_or(NO_WEAPON_MULT)
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig { walk_speed: NPC_WALK, run_speed: NPC_RUN, arrive: 32.0, retry_hours: 1.0, params: PathParams::default(), keep_apart: true, chats_anywhere: false, find_buying: false }
    }
}

/// An actor's walking and running speeds, game units per second.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Speeds {
    pub walk: f32,
    pub run: f32,
}

/// An actor's own speeds ([`Ai::speeds`]): calm (its packages, conversations) and alert
/// (searching, combat, fleeing). Creatures move at their own clips' pace and some have
/// a separate alert stance (a cazador scurries along the ground while calm and flies
/// when alert [observed: owner's playtest and the AI observer, actors.md §8.10]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ActorSpeeds {
    pub calm: Speeds,
    pub alert: Speeds,
    /// Its own turn rate (radians per second, walking and standing) and how far off its
    /// way it may be before it turns on the spot (radians); `None`: the walker's defaults
    /// (`fnv_nav::follow::WALK_TURN_RATE`, `SPOT_TURN_RATE`, `SPOT_TURN_START`).
    pub turn_rate: Option<f32>,
    pub spot_turn_start: Option<f32>,
}

/// What an actor is doing about its package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// No package, or nothing decided yet.
    None,
    Travelling,
    /// Where the package wants it.
    There,
    /// The package is not carried out (its type, or its location kind): the actor
    /// stays where it is.
    Staying,
}

/// Something worth telling about an actor's package (for traces, tools and tests).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AiEvent {
    /// The actor has another current package (`None`: none).
    Package { package: Option<FormId> },
    /// Pushed aside and left alone again, the actor goes back to its package's place,
    /// `away` units from where it stands (a [`AiEvent::Trip`] follows).
    PushedBack { package: FormId, away: f32 },
    /// A walk was planned.
    Trip { package: FormId, to: [f32; 3], space: Space, length: f32, doors: Vec<FormId>, running: bool, patrol_point: Option<FormId> },
    /// Already where the package wants it.
    AlreadyThere { package: FormId },
    Door { door: FormId, to_door: FormId },
    /// After a load, the package goes on where the save had it (`crate::progress`): at
    /// this patrol point, or with the sandbox's day as it was.
    Resumed { package: FormId, patrol_point: Option<FormId>, sandbox: bool },
    /// The script of the next door on the way did not let the actor through
    /// (`fnv_game::npc_doors`): it plans again without that door.
    DoorRefused { door: FormId },
    /// Its walk was given up while it waited for `door` to open: it did not go through.
    DoorWaitCancelled { door: FormId },
    /// The walk ended at the package's place, after `seconds` and `travelled` units.
    Arrived { package: FormId, seconds: f64, travelled: f32, patrol_point: Option<FormId> },
    /// Arrival's standing heading target, radians [design, #2526; `Ai::settle`].
    ArrivalFacing { heading: f32 },
    /// No way to the package's location: the package is set aside.
    Unreachable { package: FormId, why: String },
    /// Out of sight, with no door joining where it is and where its package sends it: it
    /// is put there (docs/spec-notes/packages.md §11).
    PutThere { package: FormId, from: Place, to: Place },
    /// The way there needs a load door and the creature cannot open doors (Can't Open
    /// Doors, packages.md §7.1): it stays, and the package is set aside as for no way.
    DoorsClosedToIt { package: FormId },
    /// Its feet at `at` do not fit its goal's height and no new path helped: its walk ends
    /// as if they did (`fnv_nav::Walker::off_path`).
    HeightWaived { at: [f32; 3] },
    NotCarriedOut { package: FormId, why: &'static str },
    /// The actor uses this furniture now (`None`: it leaves the furniture it used).
    Furniture { furniture: Option<FormId> },
    /// Whether it may be given this furniture came out other than yes: once per actor and
    /// piece when excluded, once per actor when unknown (`crate::furniture`).
    FurnitureEligibility { furniture: FormId, decision: &'static str, reason: &'static str },
    /// A sandboxing actor chose what to do next; `walk` is the length of the way there
    /// (0: it is there already).
    Sandbox {
        #[serde(flatten)]
        doing: Doing,
        walk: f32,
    },
    /// A patrolling actor waits at a patrol point for its idle time.
    PatrolWait { patrol_point: FormId, seconds: f32 },
    /// An actor with a Dialogue package sets off towards the one it is to talk to, who
    /// is `away` units from it (`None`: in another space).
    Approach { package: FormId, target: FormId, away: Option<f32>, length: f32, doors: Vec<FormId>, running: bool },
    /// It starts talking, `away` units from its target: a conversation with the player
    /// (`line` is what it says first), or the one line of a Say To package. `greeting`:
    /// the line is a greeting with the Goodbye flag, said at the player without a
    /// conversation (`Game::start_dialogue`).
    Talk {
        package: FormId,
        target: FormId,
        line: FormId,
        away: f32,
        say_to: bool,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        greeting: bool,
    },
    /// The conversation it started is over, or its greeting is said.
    TalkOver { package: FormId },
    /// A Follow or Accompany package: it sets off after `target`, `away` units from it
    /// (`None`: in another space), a way of `length` units through `doors`.
    Follow { package: FormId, target: FormId, away: Option<f32>, length: f32, doors: Vec<FormId>, running: bool },
    /// It has caught up with the one it follows and stands, `away` units from it, having
    /// been to stop within `stop_at` ([`follow_stop_at`]).
    CaughtUp { package: FormId, target: FormId, away: f32, stop_at: f32 },
    /// It was to talk and did not (it has no line to say).
    NoTalk { package: FormId, why: String },
    /// It is near enough to force-greet `target`, `away` units off, but something solid
    /// is between them (a wall, a closed door): it waits ([`Game::greet_sight`]).
    TalkUnseen { package: FormId, target: FormId, away: f32 },
    /// Its Dialogue package walks the player, `away` units off, into its trigger location
    /// before it talks, to `to` (`Game::player_lead`; "Do Not Control Target Movement"
    /// off).
    LeadsPlayer { package: FormId, to: [f32; 3], away: f32 },
    /// The walk of [`AiEvent::LeadsPlayer`] got no nearer its point for
    /// [`LEAD_STUCK_SECONDS`] (a rail, furniture, a step in the way): the actor talks
    /// where the player stands, `left` units short of the point.
    LeadStuck { package: FormId, left: f32 },
    /// A script's StartConversation sent it to the player (`Game::walk_up_greet`): it sets
    /// off, `away` units from the player, a way of `length` units.
    WalkUp { away: Option<f32>, length: f32 },
    /// It has walked up to the player (or could get no nearer, `reached` false) and
    /// opens the conversation, `away` units off; `line` is what it says first, none when
    /// it had nothing to say.
    WalkedUp { away: f32, reached: bool, line: Option<FormId> },
    /// It said hello to the passing player, `away` units from it (`crate::social`).
    Hello { line: FormId, away: f32 },
    /// It said an IdleChatter line, `away` units from the player (`crate::social`).
    IdleChatter { line: FormId, away: f32 },
    /// It commented on what the player did (a PLAYER… topic), `away` units from the
    /// player (`crate::social`).
    PlayerAction { topic: &'static str, line: FormId, away: f32 },
    /// It commented on the fight of `fighter`, `away` units from it (`crate::social`).
    ObserveCombat { fighter: FormId, line: FormId, away: f32 },
    /// A spectator (Observe Combat) runs from the fight of `fighter`, come within `away`
    /// units of it (`crate::social`); its flee spot follows as [`AiEvent::Flee`].
    SpectatorFlee { fighter: FormId, away: f32 },
    /// It started a random conversation with `partner`, `away` units from it
    /// (`crate::social`); `line` is its first line.
    Chat { partner: FormId, line: FormId, away: f32 },
    /// It sets off to `partner` for a random conversation: `away` units from it, a way of
    /// `length` units.
    ChatApproach { partner: FormId, away: f32, length: f32 },
    /// It gave up the random conversation with `partner` before it began.
    ChatGaveUp { partner: FormId, why: &'static str },
    /// A script's Look with `rotateBody` turns its body from heading `from` to `to`
    /// (radians), facing `target` (`crate::social`, `Ai::face_looks`).
    LookTurn { target: FormId, from: f32, to: f32 },
    /// The body turned for a Look faces `target`, at `heading`.
    LookFaced { target: FormId, heading: f32 },
    /// A Look with `rotateBody` on it does not turn its body: why.
    LookHeld { target: FormId, why: &'static str },
    /// The Look turning its body is over (StopLook); the body keeps `heading`.
    LookOver { heading: f32 },
    /// After a random conversation it goes back to its package's place, `away` units off.
    ChatBack { package: FormId, away: f32 },
    /// In Alert or Lost it sets off to search at `to`, `away` units from it, a way of
    /// `length` units, for what `about` did (`crate::alert`).
    Search { about: FormId, to: [f32; 3], away: f32, length: f32 },
    /// It looks about at `at` for `seconds`.
    SearchLook { at: [f32; 3], seconds: f32 },
    /// In Combat it stops and faces `target`, `away` units from it.
    Hold { target: FormId, away: f32 },
    /// Giving aggro radius warnings, it stops and faces `target`, `away` units from it
    /// (`crate::alert`, `fnv_game::aggro`).
    Warn { target: FormId, away: f32 },
    /// In Combat it runs up to `target`, `away` units off, a way of `length` units to
    /// `to` (`crate::alert`); `aiming`: it aims down its sights and walks.
    Close { target: FormId, to: [f32; 3], away: f32, length: f32, aiming: bool },
    /// Closing in on `target`, or running to a post while fighting it, it slows to a walk
    /// as it raises its sights (`aiming`), or runs again as it lowers them (`crate::alert`).
    Pace { target: FormId, aiming: bool },
    /// In Combat it goes to the post the combat AI chose (`crate::alert`): `to`, `away`
    /// units off, a way of `length` units.
    Post { post: fnv_game::combat_group::PostKind, target: FormId, to: [f32; 3], away: f32, length: f32 },
    /// It is at its post: its feet at `feet`, `off` units from where the post asks for it
    /// (`at`).
    AtPost { post: fnv_game::combat_group::PostKind, target: FormId, off: f32, at: [f32; 3], feet: [f32; 3] },
    /// It has no way to its post: it closes in instead.
    NoPost { post: fnv_game::combat_group::PostKind },
    /// Its walk to its post ended away from it after `elapsed` seconds: its feet at `feet`,
    /// `off` units from where the post asks for it (`at`). The post is dropped.
    PostFailed { post: fnv_game::combat_group::PostKind, target: FormId, at: [f32; 3], feet: [f32; 3], off: f32, elapsed: f32, reason: crate::alert::PostFailure },
    /// It flees `from` (`how`): to `to`, `away` units from the one it flees, a way of
    /// `length` units.
    Flee { from: FormId, how: fnv_game::combat_group::FleeBy, to: [f32; 3], away: f32, length: f32 },
    /// It reached its flee spot after `seconds`, `off` units from it; whether it hides
    /// there crouched (a person, `Game::sneaks`).
    FleeArrived { from: FormId, seconds: f32, off: f32, hides: bool },
    /// It holds a better weapon it carries (`crate::rearm`); `from`: the weapon it fetched
    /// for it (`None`: one it had).
    Rearmed { weapon: FormId, from: Option<FormId> },
    /// It sets off for a better weapon: its damage per second, the stops and the trip's
    /// length (`crate::rearm`).
    Fetch { weapon: FormId, dps: f32, stops: u32, trip: f32 },
    /// At a stop of its fetch: whether it took (or copied) `item` from `from`.
    FetchGot { item: FormId, from: FormId, take: fnv_game::rearm::Take, got: bool },
    /// It gives up its fetch at `from` (`why`).
    FetchDropped { from: FormId, why: &'static str },
    /// Its weapon does not fire at its target `distance` away: it holds `weapon`, which
    /// does (`crate::rearm`).
    RangeSwap { weapon: FormId, distance: f32 },
    /// Its target `distance` away stayed inside its explosive `weapon`'s blast and it
    /// carries nothing that fires there: it puts `weapon` away and fights with its fists
    /// (`crate::rearm`, #2745).
    RangeFists { weapon: FormId, distance: f32 },
    /// It has nowhere to flee to and stays where it is; whether it hides there crouched.
    Cornered { from: FormId, hides: bool },
    /// It stops searching or holding (`why`).
    SearchOver { why: &'static str },
    /// In a combat state, its package's Continue During Combat flag now holds it on the
    /// package (`held`), or no longer does and it fights (`crate::alert`).
    PackageHolds { package: FormId, held: bool },
    /// After a search it goes back to its package's place, `away` units off.
    SearchBack { package: FormId, away: f32 },
    /// Back to its package inside the package's place (after a search, a push or a
    /// chat), it settles there again: its bed or chair, a sandbox ([`Ai::back_to_package`]).
    Resettles { package: FormId },
    /// Back to an Escort, Find or Use Item At package whose walk a search, a push or a
    /// chat cut off: it takes the package up afresh ([`Ai::back_to_package`]).
    TakesUpAgain { package: FormId },
    /// An actor without a package, moved away by a search, a fight or a push, walks back
    /// to where it was placed ([`Ai::back_to_package`]).
    BackHome { away: f32, length: f32 },
    /// Told to back up on the companion wheel, `away` units from the player: it walks
    /// back a way of `length` units ([`Ai::back_up`]).
    BackUp { away: f32, length: f32 },
    /// A script gave it a weapon use (`fnv_game::use_weapon`, UseWeapon): it goes to
    /// where it is to stand, a way of `length` units (0: it stands there), to shoot at
    /// `target`.
    WeaponUse { target: FormId, length: f32 },
    /// The script's weapon use is over: it goes back to its package.
    WeaponUseOver,
    /// An Escort package: the one it leads is `away` units behind (`None`: in another
    /// space): it stops and waits (`crate::errand`).
    EscortWaits { package: FormId, target: FormId, away: Option<f32> },
    /// The one it leads has come up: it goes on.
    EscortOn { package: FormId, target: FormId },
    /// A Find package: it sets off to `object`, a way of `length` units (0: it is there).
    FindGoes { package: FormId, object: FormId, length: f32 },
    /// A Find package: it has reached `object` and did `act` with it.
    Found { package: FormId, object: FormId, act: crate::errand::Act },
    /// A Find package with Allow Buying: it bought one `item` from `vendor` for `price`
    /// caps (`crate::errand`).
    Bought { package: FormId, vendor: FormId, item: FormId, price: u32 },
    /// A Find package: nothing (more) to find; `found` so far.
    NothingFound { package: FormId, found: u32 },
    /// A Use Item At package: it uses `thing` (furniture, or stands at it), or stays at
    /// the location (`None`: nothing to use found).
    UsesAt { package: FormId, thing: Option<FormId> },
    /// A Use Item At package naming an item: it uses one it carries (`had` false: it
    /// has none).
    UsesItem { package: FormId, item: FormId, had: bool },
    /// An Ambush package: it lies in wait (`hidden`: sneaking).
    AmbushWaits { package: FormId, hidden: bool },
    /// An Ambush package: `enemy` came into the trigger location, `away` units off: it
    /// attacks.
    AmbushSprung { package: FormId, enemy: FormId, away: f32 },
    /// A Flee Not Combat package: it runs from `from` to `to`, a way of `length` units.
    FleeRuns { package: FormId, from: FormId, to: [f32; 3], length: f32 },
    /// A Flee Not Combat package: it is where it fled to, after `seconds`, or nothing
    /// threatens it (`from` the threat it got away from, if any).
    FleeSafe { package: FormId, from: Option<FormId>, seconds: f64 },
}

/// How far an actor is with its Dialogue package ([`goal::Talk`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Talking {
    /// On its way to the package's wait location.
    ToWait,
    /// Waiting for its target to come near, or into the trigger location.
    Waiting,
    /// Going up to its target, last planned for where the target stood (`to`); from
    /// `replan` on (`Game::time`) the way is planned again when the target has moved.
    Approaching { to: Place, replan: f64 },
    /// In the conversation it started, or saying its greeting at the player (a Goodbye
    /// line, which opens no conversation).
    Speaking,
    /// It has talked: the package has met its goal.
    Done,
}

/// Whether `actor` is saying its greeting at the player outside a conversation
/// (`Speech::greeting`).
pub(crate) fn greets(game: &Game, actor: FormId) -> bool {
    game.speech.iter().any(|s| s.greeting && s.speaker == actor)
}

/// A follower stands until its target is this much farther than its distance, so that
/// it does not set off at every step the target takes, game units [our choice].
pub const FOLLOW_SLACK: f32 = 64.0;
/// A follower on its way stops only this much nearer than its package's distance, though
/// not nearer than [`COLUMN_GAP`] (nor farther than the distance itself), so that it
/// comes in well inside the distance once its target stands ([`follow_stop_at`])
/// [design, #2122: in the original Boone (distance 330) came to 177 units once the player
/// stopped, docs/spec-notes/npc-behaviour-compare.md, and the Novac pack brahmin (about
/// 450) closed to about 300 when its merchant stopped, docs/spec-notes/actors.md §8.10,
/// both about 150 inside [observed]; the original's own rule is not known, and a share
/// fits the two less well (0.54 and 0.67)].
pub const FOLLOW_STOP_INSET: f32 = 150.0;

/// How near to the one it walks after a follower on its way stops, its distance being
/// `keep` (at least the AI's arrive distance, `arrive`); `in_step`: with a target on the
/// move; `in_column`: walking after the one ahead of it in a column. One walking after
/// the leader itself stops [`FOLLOW_STOP_INSET`] inside its distance ([design, #2122]); it starts again
/// only past the distance plus [`FOLLOW_SLACK`]. In a column the gap to the one ahead
/// stays as it was: the distance, or [`FOLLOW_SLACK`] inside it while in step.
pub fn follow_stop_at(keep: f32, arrive: f32, in_step: bool, in_column: bool) -> f32 {
    let slack = if in_step { (keep - FOLLOW_SLACK).max(arrive) } else { keep };
    if in_column {
        return slack;
    }
    slack.min((keep - FOLLOW_STOP_INSET).max(COLUMN_GAP).min(keep))
}
/// A follower this much farther from its target than its distance runs, game units
/// [our choice: 512 from the target at the usual distance of 128; owner's playtest
/// 2026-10-24: the Novac pack brahmin, whose package keeps it about 450 units back,
/// set off past a fixed 512 at once and so ran at every start].
pub const FOLLOW_RUN_BEHIND: f32 = 384.0;
/// A follower on its way after a moving target goes at the target's own pace plus the
/// gap beyond its distance over this many seconds, between [`FOLLOW_SLOWEST`] of its
/// walk and its run ([`keep_step_speed`]) [our choice; owner's playtest 2026-10-24:
/// the Novac pack brahmin swapped between running and walking all the way after its
/// merchant, where it should run only to catch up and then keep step].
pub const FOLLOW_CLOSE_SECONDS: f32 = 3.0;
/// The slowest a follower keeps step with its target, a share of its walk [our choice].
pub const FOLLOW_SLOWEST: f32 = 0.5;
/// A target going at least this fast, game units per second, is one a follower keeps
/// step with: it goes on at the target's pace within its distance rather than stopping
/// there and setting off again [our choice].
pub const FOLLOW_MOVING: f32 = 20.0;
/// In a column of followers ([`Ai::ahead_in_column`]), how near each one after the first
/// keeps to the one ahead of it, game units, at most the package's own distance [our
/// choice: the owner's playtest of 2026-10-24 asked for followers of one leader to walk
/// in a line, three mercenaries a few steps apart behind their leader].
pub const COLUMN_GAP: f32 = 128.0;
/// A follower more than [`FOLLOW_RUN_BEHIND`] beyond its distance goes faster than it can
/// run, to catch up: by up to this share more, reached [`CATCH_UP_SPAN`] units beyond
/// that [our choice: owner's playtest of 2026-10-24, "a speed boost to
/// allow them to catch up ... faster than their max speed"; observed since: in the
/// original, creatures given DefaultFollowPlayerFar 2,700 units from the player ran at
/// 1.50–1.51 times their own run (AI observer, campaign `creature_flee`, 2026-10-25,
/// docs/spec-notes/actors.md §8.10); the span over which it grows is our choice].
pub const CATCH_UP_BOOST: f32 = 0.5;
/// See [`CATCH_UP_BOOST`]; game units.
pub const CATCH_UP_SPAN: f32 = 1024.0;

/// How much faster than its own pace a follower `gap` units beyond its distance goes
/// ([`CATCH_UP_BOOST`]): 1 within [`FOLLOW_RUN_BEHIND`].
pub fn catch_up(gap: f32) -> f32 {
    1.0 + CATCH_UP_BOOST * ((gap - FOLLOW_RUN_BEHIND) / CATCH_UP_SPAN).clamp(0.0, 1.0)
}

/// The speed of a follower in step with a target going at `target_pace`, `away` from
/// it, that is to keep within `keep`: the target's pace plus the gap beyond `keep`
/// closed in [`FOLLOW_CLOSE_SECONDS`], between [`FOLLOW_SLOWEST`] of `walk` and `run`.
/// It changes smoothly with the gap, so the walk clip is played faster or slower and the
/// run clip comes only when catching up calls for it (the gait's own switch point,
/// `locomotion.rs`), rather than the actor swapping between its walk and its run at
/// every plan or stopping and setting off again every few steps.
pub fn keep_step_speed(target_pace: f32, away: f32, keep: f32, walk: f32, run: f32) -> f32 {
    let slowest = walk * FOLLOW_SLOWEST;
    (target_pace + (away - keep).max(0.0) / FOLLOW_CLOSE_SECONDS).clamp(slowest, run.max(slowest))
}

/// A target that has moved this far from where the way to it was planned to is planned
/// for again, and not more often than every [`REPLAN_SECONDS`] [our choice].
const REPLAN_DISTANCE: f32 = 64.0;
const REPLAN_SECONDS: f64 = 0.5;

/// A walking actor is asked by the next door of its way when that door is within this
/// many units beyond the step it takes (`fnv_game::npc_doors`) [design].
const DOOR_ASK_MARGIN: f32 = 96.0;

/// An actor pushed aside goes back to where its package has it once nothing has pushed
/// it for this long, seconds [our choice; whether and when the game's actors go back is
/// the capture `pc_actor_push`].
pub const PUSHED_BACK_SECONDS: f64 = 2.0;
/// A push moves an actor onto navmesh no higher or lower than this from where it
/// stands, game units [our choice: about a step].
const PUSH_MAX_STEP: f32 = 32.0;

/// An event with its actor and when it happened.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Logged {
    /// `Game::time`, seconds.
    pub time: f64,
    /// GameDaysPassed and GameHour.
    pub days: f32,
    pub hour: f32,
    pub actor: FormId,
    #[serde(flatten)]
    pub event: AiEvent,
}

/// What the caller of [`Ai::update_moved`] does itself.
pub trait Mover {
    /// A door that opens in place stands in `actor`'s way: start it opening, and say
    /// whether it is fully open.
    fn open(&mut self, _actor: FormId, _door: u32) -> bool {
        true
    }

    /// Move `actor` along its walker's path for `dt` seconds by other means than the
    /// walker's own (which keeps to the navmesh surface): update the walker's place,
    /// heading and follower as `fnv_nav::Walker::update_with` would and say what
    /// happened. `None`: leave this actor to the walker for this update.
    fn walk(&mut self, _actor: FormId, _walker: &mut Walker, _nav: &NavGraph, _dt: f32) -> Option<WalkEvent> {
        None
    }

    /// `actor`'s AI is off (SetActorsAI 0) while it walks: carry it on along
    /// `walker.heading` at the walker's speed for `dt` seconds by the caller's character
    /// movement, which collides and follows the ground, and update the walker's place.
    /// `false`: it cannot be carried on here (no character movement where it is), and it
    /// stands (docs/spec-notes/set-actors-ai.md).
    fn coast(&mut self, _actor: FormId, _walker: &mut Walker, _dt: f32) -> bool {
        false
    }
}

/// [`Ai::pass_time`]'s mover: the walker's own steps, every door open.
pub(crate) struct Passed;

impl Mover for Passed {}

/// One actor the AI moves.
pub struct Agent {
    pub walker: Walker,
    pub activity: Activity,
    /// The package the actor was last seen with.
    pub(crate) package: Option<FormId>,
    pub(crate) goal: Option<Goal>,
    /// Index of the patrol point being walked to.
    pub(crate) route_at: usize,
    pub(crate) trip_started: f64,
    pub(crate) trip_from: f32,
    /// The door that opens in place the actor is standing at, waiting for it to open.
    pub waiting_at: Option<u32>,
    /// The furniture the actor uses, or is to use where it stands (a seat, a bed): the
    /// engine sits it down and gets it up; headless it is only known.
    pub using: Option<FormId>,
    /// Furniture the actor would use but a script's Look turning its body keeps it from
    /// until StopLook (`crate::social`, `Ai::face_looks`).
    pub(crate) held_seat: Option<FormId>,
    /// The actor's sandbox, while it is in the area of a Sandbox or Wander package.
    pub sandbox: Option<Sandbox>,
    /// Waiting at a patrol point until this `Game::time`.
    pub(crate) wait_until: Option<f64>,
    /// Its Dialogue package's progress.
    pub talk: Option<Talking>,
    /// When it was last pushed aside (`Game::time`), until it has gone back or need not.
    pushed_at: Option<f64>,
    /// A random conversation it is going to, waiting for or in (`crate::social`): its
    /// package's own doings wait meanwhile.
    pub chat: Option<crate::social::Chat>,
    /// What it does in a combat state (`crate::alert`): its package's own doings wait
    /// meanwhile.
    pub search: Option<crate::alert::Search>,
    /// A target it found no way to (`Ai::close_in`), when it may look for one again and
    /// how long it waited this time: until then it holds its ground without a new search
    /// (#3785).
    pub(crate) no_way_to: Option<(FormId, f64, f64)>,
    /// It cannot walk (a creature without movement animations, a raven): it stays where
    /// it is, whatever its packages say ([`Ai::root`]).
    pub rooted: bool,
    /// A rooted actor that never turns either ([`Ai::fix_heading`]): nothing changes its
    /// heading, in or out of combat.
    pub fixed: bool,
    /// The heading it is to stand facing where it arrived (an idle marker's, a sandbox
    /// spot's): turned to on the spot at the standing turn rate, not taken at once
    /// [design; owner's playtest 2026-10-11: actors turned half about in a frame].
    pub face_to: Option<f32>,
    /// A Follow or Accompany package: where the target was when the way to it was last
    /// planned, from when (`Game::time`) it may be planned again, and how fast the
    /// target went before that plan, when known.
    following: Option<(Place, f64, Option<f32>)>,
    /// The number of the script's weapon use it carries out ([`Ai::weapon_step`]).
    pub(crate) weapon_order: Option<u64>,
    /// An Escort, Find, Use Item At, Ambush or Flee Not Combat package's progress
    /// (`crate::errand`).
    pub errand: Option<crate::errand::Errand>,
    /// Told to back up on the companion wheel: on its way back until then (`Game::time`)
    /// ([`Ai::back_up`]).
    pub(crate) backing: Option<f64>,
    /// Walking up to the player for a script's StartConversation ([`Ai::walk_up_step`]):
    /// where the player was when the way was planned, and from when it may be planned
    /// again.
    pub(crate) walk_up: Option<(Place, f64)>,
    /// An ordinary walk's drawn-weapon share ([`drawn_walk_share`]) as last applied and
    /// the walker speed it gave: set where a package's walk is planned at the walk, kept
    /// up by [`Ai::repace_walk`] and by [`Ai::set_speeds`] (new speeds on the way, the
    /// share kept); given up once anything else sets the walker's speed.
    pub(crate) walk_pace: Option<(f32, f32)>,
    /// How many times ResetAI had its package taken up afresh ([`Ai::step_actor`];
    /// `Game::take_ai_reset`): the engine gets a seated actor up when this changes, even
    /// when the package gives it the same furniture again (issue #2119).
    pub restarts: u32,
    /// The door it was last asked at and has not yet left behind, with the package it
    /// was walking for (`fnv_game::Game::npc_door_left`, [`Ai::ask_door`]).
    pub(crate) asked_door: Option<(u32, Option<FormId>)>,
    /// Where it last stood out of a combat state: the post of a "near current location"
    /// Guard that has no wander area (`crate::alert`, Continue During Combat).
    pub(crate) calm_at: Option<Place>,
    /// Its package's Continue During Combat flag held it at its last update in a combat
    /// state (`crate::alert`): a change is logged.
    pub(crate) held: bool,
}

#[derive(Default)]
pub struct Ai {
    pub cfg: AiConfig,
    pub agents: BTreeMap<FormId, Agent>,
    pub events: Vec<Logged>,
    /// Furniture in use: the reference and its user.
    taken: BTreeMap<FormId, FormId>,
    /// Which furniture an actor may be given ([`Ai::set_furniture_facts`]).
    pub(crate) furniture: crate::furniture::Eligibility,
    /// Hellos and random conversations ([`Ai::social`]).
    pub(crate) social: crate::social::Social,
    /// The actors out of the high process, with the seconds owed to each since its last
    /// update, `None` for one that is not updated there at all ([`crate::process`]).
    pub(crate) low: BTreeMap<FormId, Option<f32>>,
    /// Updates given to actors out of the high process, and changes of level.
    pub process_stats: crate::process::ProcessStats,
    /// Of the actors out of the high process, those in the original's Low level (cells
    /// neither loaded nor buffered): updated every [`crate::process::FAR_PROCESS_SECONDS`]
    /// instead of [`crate::process::LOW_PROCESS_SECONDS`] ([`crate::process`]).
    pub(crate) far: std::collections::BTreeSet<FormId>,
    /// Of the actors out of the high process, those walking into the loaded area:
    /// updated every frame ([`crate::process::ProcessLevel::Coming`]).
    pub(crate) coming: std::collections::BTreeSet<FormId>,
    /// The loaded area, from the engine ([`Ai::set_loaded`]).
    pub(crate) loaded: Option<crate::process::LoadedArea>,
    /// Actors with speeds of their own (creatures, from their clips; people, by their
    /// current Speed Mult and armour); the others move at [`AiConfig::walk_speed`] and
    /// [`AiConfig::run_speed`].
    pub speeds: BTreeMap<FormId, ActorSpeeds>,
    /// Actors slowed by crippled legs: the share of their speed they keep
    /// ([`Ai::set_pace`]; `fnv_game::Game::leg_speed_mult`).
    pub paces: BTreeMap<FormId, f32>,
    /// The scripts' navmesh switches last applied to the graph
    /// ([`Ai::apply_navmesh_switches`]).
    applied_switches: Option<BTreeMap<u32, bool>>,
    /// The world's lock serial last brought into the graph ([`Ai::apply_door_locks`]).
    applied_locks: Option<(u64, u64)>,
    /// Doors the game has locked that are not in the world now: their locks are left out
    /// of the graph until they are enabled ([`Ai::apply_door_locks`]).
    withheld_locks: Vec<u32>,
    /// The world's presence serial and the graph's obstacle stamp after the obstacles
    /// were last brought into the graph ([`Ai::obstacles_in_line`]).
    applied_obstacles: Option<(u64, u64)>,
    /// The world's presence serial last brought into the graph's load doors
    /// ([`Ai::apply_door_presence`]).
    applied_presence: Option<u64>,
    /// Doors that open in place, closed, that an unreachable report named as closing a
    /// package's way ([`Ai::why_unreachable`]); one that opens brings the packages set
    /// aside back ([`Ai::apply_doors_opened`]).
    closing_doors: Vec<u32>,
    /// The graph's locked doors as last brought in ([`Ai::apply_door_locks`]): the ones an
    /// actor owns are open to its paths ([`Ai::path_params`]).
    locked_doors: Vec<u32>,
    /// Actors near enough to force-greet the one their Dialogue package names but with a
    /// wall or a closed door between ([`Game::greet_sight`]), logged once until they talk
    /// or the line clears.
    pub(crate) unseen_greets: std::collections::BTreeSet<FormId>,
    /// Actors leading the player into their trigger location ([`Ai::lead_player`]): the
    /// player's nearest approach to the lead's point so far and when it last grew
    /// nearer, so that a walk that gets no nearer (something solid in the way) ends.
    lead_progress: BTreeMap<FormId, (f32, f64)>,
    /// When each actor's latest conversation from a Dialogue package ended (game
    /// seconds): it does not walk the player into its trigger location again for
    /// [`LEAD_AGAIN_SECONDS`] after it (#3464).
    talk_ended: BTreeMap<FormId, f64>,
    /// Progress a loaded save kept, by actor, until the actor takes up that package again
    /// ([`crate::progress`]).
    pub(crate) restored: BTreeMap<FormId, crate::progress::Progress>,
    /// Restrained actors, whose package begins again once they are released
    /// ([`Ai::begin_in_place`]).
    restrained: std::collections::BTreeSet<FormId>,
    /// Actors the player woke and who stay up (`Game::woken`).
    woken: std::collections::BTreeSet<FormId>,
    /// The columns of followers: for each one followed, its followers in their order
    /// in the line ([`Ai::ahead_in_column`]).
    columns: BTreeMap<FormId, Vec<FormId>>,
    /// When (`Game::time`) a script last put each actor somewhere ([`Ai::relocate`]):
    /// [`Ai::arrive`] leaves such an actor where it was put for a while.
    pub(crate) script_moved: BTreeMap<FormId, f64>,
    /// Actors whose walk under way stands because their speeds are 0 (a Speed Mult of 0),
    /// with the gait it goes on in when they come back ([`Ai::set_speeds`], issue #2118).
    /// Not saved, as the walks are not (`crate::progress`): after a load the walk is
    /// planned again at 0, and when the speeds come back a package's goes on in the gait
    /// `Ai::plan` chose, any other at the calm walk until it is planned again, whatever
    /// gait it stood in before the save [design].
    stalled: BTreeMap<FormId, Stall>,
}

/// A walk standing at speeds of 0 ([`Ai::stalled`]): which of its actor's speeds it goes at
/// ([`gait_speeds`]: 0 calm walk, 1 calm run, 2 alert walk, 3 alert run), its share of that
/// speed (a sneak's, a follower's catch-up), and where the walk goes (`Walker::trip`):
/// another walk does not take it up.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stall {
    gait: usize,
    share: f32,
    to: Option<Place>,
}

/// An actor's speeds in the order [`Stall::gait`] counts them.
fn gait_speeds(s: &ActorSpeeds) -> [f32; 4] {
    [s.calm.walk, s.calm.run, s.alert.walk, s.alert.run]
}

/// The gait of a walk going at `speed` with the speeds `old`, and its share of that gait's
/// speed: the walk or run, calm or alert, nearest to `speed` by ratio (a sneaking run,
/// × 0.57, or a follower's catch-up stays a run or a walk). `None` when it cannot be told:
/// `speed` or every speed of `old` is 0 [design, issue #2118].
fn gait_of(speed: f32, old: &ActorSpeeds) -> Option<(usize, f32)> {
    if !(speed.is_finite() && speed > 0.0) {
        return None;
    }
    let off = |was: f32| (speed / was).ln().abs();
    let (gait, was) = gait_speeds(old).into_iter().enumerate().filter(|(_, was)| was.is_finite() && *was > 0.0).min_by(|a, b| off(a.1).total_cmp(&off(b.1)))?;
    Some((gait, speed / was))
}

/// The share of its speed a sneaking actor moves at: fMoveSneakMult (0.57 in
/// FalloutNV.esm [observed]) while it sneaks (`Game::sneaks`), 1 otherwise ([`Ai::pace`]).
fn sneak_share(game: &Game, actor: FormId) -> f32 {
    if game.sneaks(actor) {
        game.world.forms.setting_f32("fMoveSneakMult").unwrap_or(SNEAK_MULT)
    } else {
        1.0
    }
}

/// A walker turning at its actor's own rate ([`ActorSpeeds::turn_rate`]).
fn own_turning(w: &mut Walker, s: &ActorSpeeds) {
    if let Some(r) = s.turn_rate {
        (w.turn_rate, w.spot_turn_rate) = (r, r);
    }
    if let Some(t) = s.spot_turn_start {
        w.spot_turn_start = t;
    }
}

fn hdist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

impl Ai {
    pub fn new(cfg: AiConfig) -> Ai {
        Ai { cfg, agents: BTreeMap::new(), events: Vec::new(), taken: BTreeMap::new(), furniture: Default::default(), social: Default::default(), low: BTreeMap::new(), process_stats: Default::default(), far: Default::default(), coming: Default::default(), loaded: None, speeds: BTreeMap::new(), paces: BTreeMap::new(), applied_switches: None, applied_locks: None, withheld_locks: Vec::new(), applied_obstacles: None, applied_presence: None, closing_doors: Vec::new(), locked_doors: Vec::new(), unseen_greets: Default::default(), lead_progress: BTreeMap::new(), talk_ended: BTreeMap::new(), restrained: Default::default(), woken: Default::default(), columns: BTreeMap::new(), script_moved: BTreeMap::new(), restored: BTreeMap::new(), stalled: BTreeMap::new() }
    }

    /// Give the AI what the assets say about furniture and skeletons ([`crate::furniture`]),
    /// before the first choice. Without it every actor stays eligible for everything.
    pub fn set_furniture_facts(&mut self, facts: std::sync::Arc<dyn fnv_sim::furniture_facts::FurnitureFacts>) {
        self.furniture.set_facts(facts);
    }

    /// The furniture kept for, or used by, `actor`.
    pub fn furniture_of(&self, actor: FormId) -> Option<FormId> {
        self.taken.iter().find(|(_, user)| **user == actor).map(|(f, _)| *f)
    }

    /// Log what eligibility decided since the last call.
    fn report_eligibility(&mut self, game: &Game) {
        for n in std::mem::take(&mut self.furniture.notes) {
            self.log(game, n.actor, AiEvent::FurnitureEligibility { furniture: n.furniture, decision: n.decision, reason: n.reason });
        }
    }

    /// Bring the graph's navmeshes in line with the scripts' switches (EnableNavMesh,
    /// DisableNavmesh: `fnv_game::functions::navigation`), when they changed since the
    /// last call or a game was loaded. Every actor whose way crosses a navmesh disabled
    /// now takes up its package afresh from where it stands, so it plans a way around,
    /// or sets the package aside when there is none (`Game::block_package`) [design: the
    /// original's behaviour for an actor on a navmesh being disabled is not documented].
    /// Returns the actors sent to plan again, `None` when no navmesh changed.
    pub fn apply_navmesh_switches(&mut self, game: &mut Game, nav: &mut NavGraph) -> Option<Vec<FormId>> {
        if self.applied_switches.as_ref().is_some_and(|a| a.len() == game.nav_switches.navmeshes.len() && a.iter().zip(&game.nav_switches.navmeshes).all(|((k, v), (k2, v2))| *k == k2.0 && v == v2)) {
            return None;
        }
        let want = game.nav_switches.by_id();
        let changed = nav.apply_switches(&want);
        self.applied_switches = Some(want);
        if !changed {
            return None;
        }
        let blocked: Vec<FormId> = self.agents.iter().filter(|(_, a)| a.walker.follower.as_ref().is_some_and(|f| f.blocked(nav))).map(|(id, _)| *id).collect();
        for &actor in &blocked {
            let a = self.agents.get_mut(&actor).expect("listed");
            a.walker.follower = None;
            a.walker.tri = fnv_nav::graph::NONE;
            a.activity = Activity::None;
            // Seen afresh by `step_actor`, which begins the package again from here.
            a.package = None;
            a.goal = None;
            a.sandbox = None;
            a.errand = None;
            a.following = None;
        }
        Some(blocked)
    }

    /// Bring the graph's door locks in line with the game's (`fnv_sim::World::lock`,
    /// `unlock`: scripts' Lock and Unlock, packages' lock and unlock flags, keys, picked
    /// locks) when one changed since the last call, or a new world came in. The graph
    /// starts from the doors' authored XLOC, so without this a door a quest unlocked
    /// stays shut to every path search [observed: gap found 2026-10-23,
    /// docs/handoff/project-audit-2026-10-23.md §4.1 item 3].
    ///
    /// A door unlocked: the packages set aside as out of reach may be chosen again
    /// (`Game::unblock_packages`) and their actors take up their package afresh. A door
    /// locked: the actors whose way goes through it plan again (a locked door they cannot
    /// open is no longer a way) [hypothesis: as for a navmesh switched off]. Returns the
    /// actors sent to plan again, `None` when no lock changed.
    ///
    /// A door that is not in the world ([`Game::present`]: disabled by a script or an
    /// enable parent, or placed Initially Disabled) holds no lock here: "a disabled
    /// reference is not in the world" [documented: GECK wiki Disable], so it shuts no way,
    /// and the graph's authored XLOC of it is taken out. Broken Steel's in-place door
    /// 0x0D009B07 (lock level 255, no key, disabled when Death From Above gets there) made
    /// every path past it unreachable (issue #2766). Its lock comes back when it is
    /// enabled, so the world's presence serial is watched as well as the lock serial.
    pub fn apply_door_locks(&mut self, game: &mut Game, nav: &mut NavGraph) -> Option<Vec<FormId>> {
        let serial = (game.world.lock_serial(), game.world.presence_serial());
        if self.applied_locks == Some(serial) {
            return None;
        }
        // Only the presence moved (references enabled, disabled, created): the doors to
        // look at are those that hold a lock or have one withheld, not every door.
        let locks_moved = self.applied_locks.is_none_or(|a| a.0 != serial.0);
        self.applied_locks = Some(serial);
        // [design, #2538] Include cold load doors: their applicable lock can live on
        // the other side, even when this side has never been materialised.
        let doors: Vec<u32> = if locks_moved {
            nav.door_refs()
        } else {
            let mut v: Vec<u32> = nav.door_locks.keys().copied().chain(self.withheld_locks.iter().copied()).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let mut withheld = Vec::new();
        let states: Vec<_> = doors
            .into_iter()
            .map(|id| {
                // A door the world does not know (no placed reference) is taken as there.
                let there = game.world.is_enabled(FormId(id)).is_err() || game.present(FormId(id));
                let lock = game.world.lock_of(FormId(id)).filter(|l| l.locked);
                if lock.is_some() && !there {
                    withheld.push(id);
                }
                (id, lock.filter(|_| there).map(|l| fnv_nav::graph::DoorLock { level: l.level, key: l.key.map(|k| k.0) }))
            })
            .collect();
        self.withheld_locks = withheld;
        let changes = nav.sync_door_locks(states);
        self.locked_doors = nav.door_locks.keys().copied().collect();
        self.locked_doors.sort_unstable();
        if changes.is_empty() {
            return None;
        }
        let mut again: Vec<FormId> = Vec::new();
        if !changes.unlocked.is_empty() {
            again.extend(game.unblock_packages());
        }
        if !changes.locked.is_empty() {
            again.extend(self.agents.iter().filter(|(_, a)| a.walker.follower.as_ref().is_some_and(|f| f.through_doors(&changes.locked))).map(|(id, _)| *id));
        }
        again.sort();
        again.dedup();
        for actor in &again {
            if let Some(a) = self.agents.get_mut(actor) {
                a.walker.follower = None;
                a.walker.tri = fnv_nav::graph::NONE;
                a.activity = Activity::None;
                a.package = None;
                a.goal = None;
                a.sandbox = None;
                a.errand = None;
                a.following = None;
            }
        }
        Some(again)
    }

    /// Bring the graph's load doors in line with whether the game has them in the world
    /// now ([`Game::present`]: a script's Enable or Disable, an enable parent's), when the
    /// world's presence serial moved since the last call (`fnv_sim::World::presence_serial`,
    /// which every such change moves; a new world starts from a number of its own). A door
    /// that is not there takes no part in paths (`fnv_nav::NavGraph::disabled_doors`), as
    /// a locked one an actor cannot open: Nellis's hangar has a door into each of its two
    /// versions on the same spot, one disabled until the B-29 is up, and Loyal, whose
    /// package after a failed Volare! is in the B-29's hangar, was walked through the
    /// disabled one into a hangar the player cannot enter (issue #2566).
    ///
    /// A door gone: the actors whose way goes through it plan again. A door back: the
    /// packages set aside as out of reach may be chosen again (`Game::unblock_packages`)
    /// [design: as for a door locked and unlocked, [`Ai::apply_door_locks`]]. Returns the
    /// actors sent to plan again, `None` when no door changed.
    pub fn apply_door_presence(&mut self, game: &mut Game, nav: &mut NavGraph) -> Option<Vec<FormId>> {
        let serial = game.world.presence_serial();
        if self.applied_presence == Some(serial) {
            return None;
        }
        self.applied_presence = Some(serial);
        // A door the world does not know (no placed reference) is left as the graph has it.
        // Doors that open in place too (#2405): one not there bars nothing.
        let states: Vec<(u32, bool)> = nav.door_refs().into_iter().filter(|&d| game.world.is_enabled(FormId(d)).is_ok()).map(|d| (d, game.present(FormId(d)))).collect();
        let changes = nav.sync_door_presence(states);
        if changes.is_empty() {
            return None;
        }
        // A load door gone closes a way and one back opens one; a door that opens in place
        // is the other way round: gone, it no longer stands in a way; back, it may.
        let in_place = |d: &u32| nav.in_place.contains_key(d);
        let (opened, closed): (Vec<u32>, Vec<u32>) = {
            let (gone_here, gone_load): (Vec<u32>, Vec<u32>) = changes.gone.iter().partition(|d| in_place(d));
            let (back_here, back_load): (Vec<u32>, Vec<u32>) = changes.back.iter().partition(|d| in_place(d));
            ([gone_here, back_load].concat(), [gone_load, back_here].concat())
        };
        let mut again: Vec<FormId> = Vec::new();
        if !opened.is_empty() {
            again.extend(game.unblock_packages());
        }
        if !closed.is_empty() {
            // The triangles a door back in place stands on: a way across them was planned
            // without it.
            let tris: std::collections::HashSet<u32> = nav.door_tris.iter().filter(|(_, ds)| ds.iter().any(|d| closed.contains(d))).map(|(&t, _)| t).collect();
            again.extend(self.agents.iter().filter(|(_, a)| a.walker.follower.as_ref().is_some_and(|f| f.through_doors(&closed) || f.crosses(&tris))).map(|(id, _)| *id));
        }
        again.sort();
        again.dedup();
        for actor in &again {
            if let Some(a) = self.agents.get_mut(actor) {
                a.walker.follower = None;
                a.walker.tri = fnv_nav::graph::NONE;
                a.activity = Activity::None;
                a.package = None;
                a.goal = None;
                a.sandbox = None;
                a.errand = None;
                a.following = None;
            }
        }
        Some(again)
    }

    /// Bring the graph's obstacles (`fnv_nav::obstacles`: objects flagged Obstacle that
    /// can be enabled or disabled) in line with whether the game has them enabled now: an
    /// enabled one cuts the navmesh under it. Every actor whose way now crosses a cut
    /// triangle takes up its package afresh from where it stands, as for a navmesh
    /// switched off ([`Ai::apply_navmesh_switches`]). Returns the obstacles that changed
    /// and the actors sent to plan again, `None` when none changed.
    ///
    /// Nothing is polled while [`Ai::obstacles_in_line`] holds: then every obstacle's
    /// state in the graph is the game's already, and the poll would change nothing.
    pub fn apply_obstacles(&mut self, game: &Game, nav: &mut NavGraph) -> Option<(Vec<u32>, Vec<FormId>)> {
        if nav.obstacles.is_empty() {
            return None;
        }
        if self.obstacles_in_line(game, nav) {
            // A missed writer of the presence serial would show here in debug builds.
            debug_assert!(nav.obstacles.iter().all(|(&r, o)| o.enabled == game.present(FormId(r))), "obstacle cache out of line with the game");
            return None;
        }
        let states: Vec<(u32, bool)> = nav.obstacles.keys().map(|&r| (r, game.present(FormId(r)))).collect();
        let changed = nav.sync_obstacles(states);
        self.applied_obstacles = Some((game.world.presence_serial(), nav.obstacles_stamp()));
        if changed.is_empty() {
            return None;
        }
        let blocked: Vec<FormId> = self.agents.iter().filter(|(_, a)| a.walker.follower.as_ref().is_some_and(|f| f.blocked(nav))).map(|(id, _)| *id).collect();
        for actor in &blocked {
            if let Some(a) = self.agents.get_mut(actor) {
                a.walker.follower = None;
                a.walker.tri = fnv_nav::graph::NONE;
                a.activity = Activity::None;
                a.package = None;
                a.goal = None;
                a.sandbox = None;
                a.errand = None;
                a.following = None;
            }
        }
        Some((changed, blocked))
    }

    /// Whether the graph's obstacles were brought in line with this game's world and
    /// nothing that could change either happened since: the world's presence serial
    /// (`fnv_sim::World::presence_serial`: every write of a reference's `enabled` or
    /// `deleted`, every state made unlike its placement, created or reset, a new world)
    /// and the graph's obstacle stamp (`fnv_nav::NavGraph::obstacles_stamp`: obstacles
    /// learnt again or switched) are the ones seen after the last poll. [`Game::present`]
    /// reads nothing else, so a poll now would find every obstacle as the graph has it.
    pub fn obstacles_in_line(&self, game: &Game, nav: &NavGraph) -> bool {
        self.applied_obstacles == Some((game.world.presence_serial(), nav.obstacles_stamp()))
    }

    /// Give `actor` speeds of its own; its walker turns at its own rate from now on, and
    /// a walk under way goes on at once at the new speeds, in its gait (`gait_of`):
    /// otherwise a package's walk keeps the speed it was planned at until its next plan
    /// (issue #2118: a SetAV SpeedMult 200 follower ran on at 100). Speeds of 0 stop the
    /// walk where it is, and it goes on in the same gait when they come back (a package's
    /// walk planned while they were 0 in the gait `Ai::plan` chose, any other at the
    /// calm walk). Crippled
    /// legs' share ([`Ai::set_pace`]) is the walker's own and stays as it is. A package's
    /// walk with a drawn-weapon share ([`Agent::walk_pace`], #2124) keeps it: the share is
    /// in the gait's (`gait_of` finds the walk at that share), and the walk stays its own,
    /// at the new speed, for [`Ai::repace_walk`] to follow the weapon from [design].
    pub fn set_speeds(&mut self, actor: FormId, speeds: ActorSpeeds) {
        let default = Speeds { walk: self.cfg.walk_speed, run: self.cfg.run_speed };
        let old = self.speeds.insert(actor, speeds).unwrap_or(ActorSpeeds { calm: default, alert: default, turn_rate: None, spot_turn_start: None });
        let Some(a) = self.agents.get_mut(&actor) else {
            self.stalled.remove(&actor);
            return;
        };
        own_turning(&mut a.walker, &speeds);
        if a.walker.follower.is_none() {
            // Nothing under way: the next walk is planned at the new speeds.
            self.stalled.remove(&actor);
            return;
        }
        let to = a.walker.trip.as_ref().map(|t| t.to);
        // A package's walk at its drawn-weapon share (#2124), still at the speed it set.
        let owned = a.walk_pace.filter(|&(_, set)| set == a.walker.speed).map(|(applied, _)| applied);
        // The gait this walk was stopped in, or planned in at 0 (`Ai::plan`); none kept
        // for another walk.
        let stall = self.stalled.get(&actor).copied().filter(|s| s.to == to && a.walker.speed <= 0.0);
        let (gait, share) = match (stall, gait_of(a.walker.speed, &old)) {
            (Some(s), _) => (s.gait, s.share),
            (None, Some(g)) => g,
            // Planned at 0 where the gait was not kept: it walks [design] until its next
            // plan rather than standing on its way, at its drawn-weapon share.
            (None, None) if a.walker.speed <= 0.0 => (0, owned.unwrap_or(1.0)),
            (None, None) => return,
        };
        let now = gait_speeds(&speeds)[gait].max(0.0) * share;
        a.walker.speed = now;
        if let Some(applied) = owned {
            a.walk_pace = Some((applied, now));
        }
        if now > 0.0 {
            self.stalled.remove(&actor);
        } else {
            self.stalled.insert(actor, Stall { gait, share, to });
        }
    }

    /// The share of its speed `actor` moves at from now on, whatever it does (crippled
    /// legs; 1 for all of it).
    pub fn set_pace(&mut self, actor: FormId, pace: f32) {
        let pace = pace.clamp(0.0, 1.0);
        if pace < 1.0 {
            self.paces.insert(actor, pace);
        } else {
            self.paces.remove(&actor);
        }
        if let Some(a) = self.agents.get_mut(&actor) {
            a.walker.pace = pace;
        }
    }

    /// `actor`'s calm speeds: its own, or the configured ones.
    pub fn calm_speeds(&self, actor: FormId) -> Speeds {
        self.speeds.get(&actor).map_or(Speeds { walk: self.cfg.walk_speed, run: self.cfg.run_speed }, |s| s.calm)
    }

    /// `actor`'s alert speeds: its own, or the configured ones.
    pub fn alert_speeds(&self, actor: FormId) -> Speeds {
        self.speeds.get(&actor).map_or(Speeds { walk: self.cfg.walk_speed, run: self.cfg.run_speed }, |s| s.alert)
    }

    /// Start looking after a placed actor, from where it is now.
    pub fn add(&mut self, game: &Game, actor: FormId) -> Result<(), String> {
        let place = goal::place_of_ref(game, actor).ok_or_else(|| format!("{actor} is in no cell"))?;
        let heading = game.world.peek(actor).map(|s| s.rot[2]).or_else(|| game.world.placed.get(actor).map(|p| p.rot[2])).unwrap_or(0.0);
        let mut walker = Walker::new(place.space, place.pos, heading, self.calm_speeds(actor).walk);
        if let Some(s) = self.speeds.get(&actor) {
            own_turning(&mut walker, s);
        }
        walker.pace = self.paces.get(&actor).copied().unwrap_or(1.0);
        self.agents.insert(actor, Agent { walker, activity: Activity::None, package: None, goal: None, route_at: 0, trip_started: 0.0, trip_from: 0.0, waiting_at: None, using: None, held_seat: None, sandbox: None, wait_until: None, talk: None, pushed_at: None, chat: None, search: None, no_way_to: None, face_to: None, following: None, rooted: false, fixed: false, asked_door: None, weapon_order: None, errand: None, backing: None, walk_up: None, walk_pace: None, restarts: 0, calm_at: None, held: false });
        Ok(())
    }

    /// The references a script put somewhere since the last call (MoveTo,
    /// `Game::take_teleported`): each actor the AI looks after goes on from where it was
    /// put ([`Ai::relocate`]). Returns them all, each with whether the AI looks after it,
    /// so the caller can hand over the others if it wants (an actor moved into the cells
    /// around the player from somewhere else).
    pub fn moved_by_scripts(&mut self, game: &mut Game) -> Vec<(FormId, bool)> {
        let mut out = Vec::new();
        for r in game.take_teleported() {
            let ours = self.agents.contains_key(&r);
            if ours {
                self.relocate(game, r);
            }
            out.push((r, ours));
        }
        out
    }

    /// `actor` was put somewhere by a script: its walker starts again where the game has
    /// it now, facing the way the game has it, and it takes up its package afresh (the
    /// way it was on is gone; furniture, a sandbox, a search or a random conversation
    /// are given up) [design: until 2026-10-11 the walker kept the old place, so the
    /// actor was drawn, and went on, where it had stood: Sunny Smiles stayed in the
    /// Prospector Saloon after VMS16's MoveTo to the gas station, owner playtest].
    pub fn relocate(&mut self, game: &mut Game, actor: FormId) {
        let Some(place) = goal::place_of_ref(game, actor) else { return };
        // From the new place a package set aside as out of reach may be reachable (#3472).
        game.unblock_packages_of(actor);
        let heading = game.world.peek(actor).map_or(0.0, |s| s.rot[2]);
        self.script_moved.insert(actor, game.time);
        // Keep a low-process actor from firing a far update in the brief window while a
        // multi-frame script batch is still moving actors and updating quest flags (issue
        // #3041: VERStalkersSouth01 moves 18 stalkers at 0.1 s intervals before clearing
        // StalkersOn 1.8 s later) [design: matches `SCRIPT_MOVE_HOLD`].
        if !self.coming.contains(&actor) {
            let every = if self.far.contains(&actor) {
                crate::process::FAR_PROCESS_SECONDS
            } else {
                crate::process::LOW_PROCESS_SECONDS
            };
            let cap = (every - crate::arrive::SCRIPT_MOVE_HOLD as f32).max(0.0);
            if let Some(Some(owed)) = self.low.get_mut(&actor) {
                *owed = owed.min(cap);
            }
        }
        self.set_using(game, actor, None);
        let Some(a) = self.agents.get_mut(&actor) else { return };
        let w = &mut a.walker;
        (w.space, w.feet, w.heading, w.tri) = (place.space, place.pos, heading, fnv_nav::graph::NONE);
        (w.follower, w.turning, w.moving) = (None, false, false);
        a.activity = Activity::None;
        // A package seen afresh: `step` begins it again from here.
        a.package = None;
        a.goal = None;
        a.sandbox = None;
        a.wait_until = None;
        a.waiting_at = None;
        a.talk = None;
        a.pushed_at = None;
        a.chat = None;
        a.search = None;
        a.following = None;
        a.errand = None;
        self.taken.retain(|_, user| *user != actor);
    }

    /// Keep `actor` where it is from now on: no package moves it, no sandbox, no walk,
    /// no search (an actor that cannot walk, e.g. a creature without movement
    /// animations; the engine decides who). Its scripts still run.
    pub fn root(&mut self, game: &Game, actor: FormId) {
        let Some(a) = self.agents.get_mut(&actor) else { return };
        a.rooted = true;
        a.goal = None;
        a.walker.follower = None;
        a.activity = Activity::None;
        a.sandbox = None;
        a.search = None;
        a.chat = None;
        a.errand = None;
        self.set_using(game, actor, None);
        self.taken.retain(|_, user| *user != actor);
    }

    /// A rooted actor that never turns either: no conversation, spoken line, script Look
    /// or combat target changes its heading from now on ([`Ai::face`], the Look turn of
    /// `crate::social`). The engine decides who: an Immobile creature whose Turning Speed
    /// is 0, which in FalloutNV.esm are Mr. House's and Yes Man's screens (MrHouse,
    /// MrHouseOFFLINE, YesManMainframe) and CRVoid, while every Immobile turret has 30 or
    /// more [observed: the masters' CREA ACBS and TNAM; documented: GECK wiki Stats Tab
    /// for Creatures, Turning Speed "How quickly the creature rotates"]. The owner saw
    /// the screen turn when a conversation ended and in a fight, where it is to stay
    /// still under any circumstance [observed: playtest 2026-11-11, issue #2150]. An
    /// actor that is not rooted is left as it is.
    pub fn fix_heading(&mut self, actor: FormId) {
        if let Some(a) = self.agents.get_mut(&actor).filter(|a| a.rooted) {
            a.fixed = true;
            a.face_to = None;
        }
    }

    /// Whether `actor` stands where an actor walking into it pushes it aside: it is in
    /// the world and alive, not on its way, not on furniture or about to use some, not
    /// waiting at a door, not in a conversation it started and not fighting (in Combat it
    /// stands its ground, `crate::alert`), and able to walk ([`Ai::root`]) [design].
    pub fn stands(&self, game: &Game, actor: FormId) -> bool {
        let Some(a) = self.agents.get(&actor) else { return false };
        game.present(actor)
            && !a.rooted
            && !game.extra.get(&actor).is_some_and(|e| e.dead)
            && a.activity != Activity::Travelling
            && a.using.is_none()
            && a.waiting_at.is_none()
            && a.talk != Some(Talking::Speaking)
            && !matches!(a.search, Some(crate::alert::Search::Holding { .. } | crate::alert::Search::Closing { .. }))
    }

    /// Push a standing actor `by` (x, y), game units: it moves over the navmesh as far
    /// as there is navmesh to stand on (along its edge when the push points off it), and
    /// not at all where it stands on none. Returns how far it moved.
    pub fn push(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, by: [f32; 2]) -> f32 {
        let len = by[0].hypot(by[1]);
        if len < 1e-6 || !self.stands(game, actor) {
            return 0.0;
        }
        let a = self.agents.get_mut(&actor).expect("stands");
        let w = &mut a.walker;
        let want = [w.feet[0] + by[0], w.feet[1] + by[1], w.feet[2]];
        let Some((tri, to)) = nav.nearest(w.space, want, len) else { return 0.0 };
        let d = [to[0] - w.feet[0], to[1] - w.feet[1]];
        // Not back against the push, and not onto another floor.
        if d[0] * by[0] + d[1] * by[1] <= 0.0 || (to[2] - w.feet[2]).abs() > PUSH_MAX_STEP {
            return 0.0;
        }
        w.feet = to;
        w.tri = tri;
        a.pushed_at = Some(game.time);
        let (feet, heading) = (Place { space: w.space, pos: w.feet }, w.heading);
        let cell = goal::cell_of_place(game.world.placed, &feet);
        let _ = game.set_actor_place(actor, cell, feet.pos, heading);
        d[0].hypot(d[1])
    }

    /// Left alone after a push: back to the place of its package when the push took it
    /// out of it [hypothesis: the game's package has the actor at its location again;
    /// not captured]. A sandboxing actor goes on from where it is, a patrol from its
    /// next point.
    fn back_after_push(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) {
        self.agents.get_mut(&actor).expect("listed").pushed_at = None;
        self.back_to_package(game, nav, actor, |package, away| AiEvent::PushedBack { package, away });
    }

    /// An actor taken out of its package's place (a push, a random conversation) goes back
    /// there, unless it sandboxes (it goes on from where it is), waits at a patrol point
    /// or carries out a Dialogue package. `why` is logged before the trip, with the
    /// package and how far away its place is.
    pub(crate) fn back_to_package(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, why: impl FnOnce(FormId, f32) -> AiEvent) {
        let a = &self.agents[&actor];
        if a.package.is_none() {
            self.back_home(game, nav, actor);
            return;
        }
        // A guard's wander off its area (in another cell after a flight through a door,
        // or outside its circle) is given up: the guard goes back to the wander's area,
        // its goal from now, and wanders again from there (`settle`). A "near current
        // location" guard (`Goal::Here`) so keeps the area it began in [design, #2027].
        let here = Place { space: a.walker.space, pos: a.walker.feet };
        if let Some((place, radius)) = a.sandbox.as_ref().filter(|s| s.off_guard_area(here)).map(Sandbox::area) {
            let a = self.agents.get_mut(&actor).expect("listed");
            a.sandbox = None;
            a.goal = Some(Goal::Near { place, radius, reference: None });
        }
        let a = &self.agents[&actor];
        let (Some(package), Activity::There, None, None, None) = (a.package, a.activity, &a.sandbox, a.wait_until, a.talk) else { return };
        let feet = Place { space: a.walker.space, pos: a.walker.feet };
        let Some(goal) = a.goal.clone() else { return };
        let free = a.using.is_none();
        let goal_met = game.extra.get(&actor).is_some_and(|e| e.package_goal_met);
        let Some(to) = self.way_to(feet, &goal) else {
            match goal {
                // A patrol whose walk to a point was cut off, or whose wait at one was
                // cleared, walks on (issue #2137) [documented: "Actor moves along patrol
                // route continuously until another package evaluates", GECK wiki Patrol
                // Package]: to the point it was at or going to, or, standing at it, on
                // to the next one [design: which point the original resumes at is not
                // captured]. A route walked to its end stays done.
                Goal::Route(route) if !goal_met => {
                    let Some(&(point, place)) = route.points.get(self.agents[&actor].route_at) else { return };
                    if self.within(feet, place, 0.0) {
                        self.patrol_on(game, nav, actor);
                    } else {
                        self.log(game, actor, why(package, hdist(feet.pos, place.pos)));
                        self.plan(game, nav, actor, package, place, Some(point));
                    }
                }
                // An ambusher goes back to its wait location [documented: "When combat
                // ends, the actor will return to the Wait Location", GECK wiki Ambush
                // Package]; one sprung stands there, not lying in wait again (whether
                // the original re-arms it is not known; issue #2137).
                Goal::Ambush(am) => {
                    if let Some(to) = self.way_to(feet, &am.wait) {
                        self.log(game, actor, why(package, hdist(feet.pos, to.pos)));
                        self.plan(game, nav, actor, package, to, None);
                    }
                }
                // An Escort, Find or Use Item At walk not finished is taken up afresh at
                // the next update, as after a search that leaves the loaded area
                // (`leave_loaded_area`) [design: issue #2137]. One whose goal was met
                // stays done.
                Goal::Escort(_) | Goal::Find(_) | Goal::UseAt(_) if !goal_met => {
                    self.log(game, actor, AiEvent::TakesUpAgain { package });
                    let a = self.agents.get_mut(&actor).expect("listed");
                    a.walker.follower = None;
                    a.activity = Activity::None;
                    a.package = None;
                }
                Goal::Route(_) | Goal::Escort(_) | Goal::Find(_) | Goal::UseAt(_) => {}
                // Already in the package's place: taken off its bed or chair (an alert
                // takes an actor off its furniture), it settles again as on arrival. In
                // the original Easy Pete, fought off his chair, had his chair package
                // back at the fight's end and sat again about 5 real seconds later
                // [observed: AI observer 2026-11-10, issue #2013].
                _ if free => {
                    self.log(game, actor, AiEvent::Resettles { package });
                    self.settle(game, actor);
                }
                _ => {}
            }
            return;
        };
        self.log(game, actor, why(package, hdist(feet.pos, to.pos)));
        self.plan(game, nav, actor, package, to, None);
    }

    /// An actor with no package keeps to where it was placed in the editor: moved off it
    /// (by a search, a fight, a push), it walks back there [hypothesis: as the game's
    /// DefaultStayAtEditorLocation; owner's playtest list 5 item 29: Ranger Ghost, who has
    /// no packages, was found on the ground by the Mojave Outpost's chain-link fence
    /// instead of on her post on the roof]. One that walks back stands where it arrives,
    /// facing as placed.
    fn back_home(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) {
        let a = &self.agents[&actor];
        if a.rooted || a.activity == Activity::Travelling || a.talk.is_some() || a.chat.is_some() {
            return;
        }
        let Some(home) = goal::editor_place_of(game, actor) else { return };
        let feet = Place { space: a.walker.space, pos: a.walker.feet };
        // Not one a script put somewhere else (MoveTo): only back from nearby.
        let away = if feet.space == home.space { hdist(feet.pos, home.pos) } else { f32::INFINITY };
        if self.within(feet, home, 0.0) || away > HOME_REACH {
            return;
        }
        let params = self.path_params(game, actor);
        let speed = self.calm_speeds(actor).walk;
        let a = self.agents.get_mut(&actor).expect("listed");
        (a.walker.speed, a.walk_pace) = (speed, None);
        let Ok(length) = a.walker.go_to(nav, home, &params).map(|p| p.length) else { return };
        a.activity = Activity::Travelling;
        a.trip_started = game.time;
        a.trip_from = a.walker.travelled;
        a.face_to = game.world.placed.get(actor).map(|p| p.rot[2]);
        self.log(game, actor, AiEvent::BackHome { away, length });
    }

    /// Told to back up on the companion wheel: it walks [`BACK_UP`] units straight away
    /// from the player (along the player's view when it stands where the player does),
    /// onto the navmesh, and stands there; meanwhile its Follow package does not call it
    /// back, and after [`BACK_UP_SECONDS`] the walk ends where it is
    /// [hypothesis: the distance; design: the rest]. One already on its way is not.
    pub fn back_up(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) {
        let Some(player) = goal::place_of_ref(game, FormId::PLAYER_REF) else { return };
        let a = &self.agents[&actor];
        if a.rooted || a.walker.space != player.space {
            return;
        }
        let feet = a.walker.feet;
        let d = [feet[0] - player.pos[0], feet[1] - player.pos[1]];
        let len = d[0].hypot(d[1]);
        let dir = if len > 1.0 {
            [d[0] / len, d[1] / len]
        } else {
            let h = game.world.peek(FormId::PLAYER_REF).map_or(0.0, |p| p.rot[2]);
            fnv_nav::follow::dir_of(h)
        };
        let want = [feet[0] + dir[0] * BACK_UP, feet[1] + dir[1] * BACK_UP, feet[2]];
        let Some((_, to)) = nav.nearest(player.space, want, BACK_UP) else { return };
        let params = self.path_params(game, actor);
        let speed = self.calm_speeds(actor).walk;
        let a = self.agents.get_mut(&actor).expect("listed");
        (a.walker.speed, a.walk_pace) = (speed, None);
        let Ok(length) = a.walker.go_to(nav, Place { space: player.space, pos: to }, &params).map(|p| p.length) else { return };
        a.activity = Activity::Travelling;
        a.trip_started = game.time;
        a.trip_from = a.walker.travelled;
        (a.following, a.face_to) = (None, None);
        a.backing = Some(game.time + BACK_UP_SECONDS);
        self.log(game, actor, AiEvent::BackUp { away: len, length });
    }

    pub(crate) fn log(&mut self, game: &Game, actor: FormId, event: AiEvent) {
        let c = game.world.calendar().ok();
        self.events.push(Logged { time: game.time, days: c.map_or(0.0, |c| c.days_passed), hour: c.map_or(0.0, |c| c.hour), actor, event });
    }

    /// Move every agent for `dt` seconds. Call after `Game::tick` (which makes the
    /// periodic package choice when `package_interval` is set). Doors that open in
    /// place are passed as if open.
    pub fn update(&mut self, game: &mut Game, nav: &NavGraph, dt: f32) {
        self.update_with(game, nav, dt, &mut |_, _| true)
    }

    /// [`Ai::update`] where doors that open in place are the caller's: at such a door
    /// `open(actor, door)` is asked every update until it says the door is fully open
    /// (it should start the door opening); the actor stands meanwhile
    /// (`fnv_nav::Walker::update_with`).
    pub fn update_with(&mut self, game: &mut Game, nav: &NavGraph, dt: f32, open: &mut dyn FnMut(FormId, u32) -> bool) {
        struct Doors<'a>(&'a mut dyn FnMut(FormId, u32) -> bool);
        impl Mover for Doors<'_> {
            fn open(&mut self, actor: FormId, door: u32) -> bool {
                (self.0)(actor, door)
            }
        }
        self.update_moved(game, nav, dt, &mut Doors(open))
    }

    /// [`Ai::update`] with the caller's [`Mover`]: its doors, and its own way of moving
    /// the actors it chooses to (the engine's character controller near the player).
    /// Actors out of the high process get their own, less frequent update
    /// ([`crate::process`]).
    pub fn update_moved(&mut self, game: &mut Game, nav: &NavGraph, dt: f32, open: &mut dyn Mover) {
        self.step(game, nav, dt, open, false);
        self.update_low(game, nav, dt);
        self.report_eligibility(game);
    }

    /// Game time that passed while a menu held the game (an hour of a wait or a sleep,
    /// docs/spec-notes/wait-menu.md §7): the actors take up the packages they have now
    /// and walk for `seconds`, `step` seconds at a time, on the navmesh, doors that open
    /// in place passed as if open; it ends early when nobody is on its way. Meanwhile
    /// nobody starts talking (a Dialogue package waits for the game to go on), a
    /// sandboxing actor does not choose something new and a patrol does not leave the
    /// point it waits at: those go by [`Game::time`], which stands still in a menu.
    /// Returns how many actors walked. [engineering choice: nothing read says what the
    /// original's actors do while the player waits.]
    pub fn pass_time(&mut self, game: &mut Game, nav: &NavGraph, seconds: f32, step: f32) -> usize {
        let step = step.max(1e-3);
        // The actors processed out of the loaded cells run their scripts once in the
        // hour, as at a low-process update (`Game::run_far_frame`) [design: the loaded
        // cells' scripts wait for the game to go on; issue #2445, Chauncey's warp to the
        // bathhouse at 15 h had to come while the player waited there].
        let far: Vec<FormId> = self.low.iter().filter(|(_, owed)| owed.is_some()).map(|(a, _)| *a).collect();
        for a in far {
            self.process_stats.far_scripts += usize::from(game.run_far_frame(a));
        }
        let mut walked: std::collections::BTreeSet<FormId> = Default::default();
        let (mut left, mut still) = (seconds, 0);
        // Two updates running with nobody on its way end it: one may only have set an
        // unreachable package aside, and the next takes up the one chosen instead.
        while left > 0.0 && still < 2 {
            self.step(game, nav, step.min(left), &mut Passed, true);
            left -= step;
            let before = walked.len();
            // Not counted: who is not updated at all where it is (`crate::process`).
            let on_way: Vec<FormId> = self.agents.iter().filter(|(id, a)| a.activity == Activity::Travelling && self.low.get(id).is_none_or(Option::is_some)).map(|(id, _)| *id).collect();
            let nobody = on_way.is_empty();
            walked.extend(on_way);
            still = if nobody && walked.len() == before { still + 1 } else { 0 };
        }
        self.report_eligibility(game);
        walked.len()
    }

    /// One update: `quiet` is [`Ai::pass_time`]'s (no talking, no new sandbox behaviour,
    /// no patrol leaving its point, nobody going back after a push).
    /// Actors out of the high process ([`crate::process`]) are left out of an update that
    /// is not quiet ([`Ai::update_moved`] gives them their own, now and then), and those
    /// not updated there at all out of every update.
    fn step(&mut self, game: &mut Game, nav: &NavGraph, dt: f32, open: &mut dyn Mover, quiet: bool) {
        // A StartConversation speaker the AI does not walk (not handed to it yet: its
        // cell has not loaded, or it could not be) talks once it is near the player, a
        // script having moved it or the player having come up; until then its greeting
        // waits, and once it joins the AI it walks up (`Ai::walk_up_step`). It never talks
        // from afar, as it did before the walk-up (issue #1544). The AI keeps an agent
        // once added (an unloaded cell's actors go to the low process, which leaves the
        // walk-up waiting), so this is not a walk cut short.
        let unwalked: Vec<FormId> = game.walk_up_greets.keys().copied().filter(|a| !self.agents.contains_key(a) && !game.walk_up_far(*a)).collect();
        for a in unwalked {
            game.walk_up_arrived(a);
        }
        let actors: Vec<FormId> = self.agents.keys().copied().filter(|a| self.low.get(a).is_none_or(|owed| quiet && owed.is_some())).collect();
        for actor in actors {
            self.step_actor(game, nav, actor, dt, open, quiet);
        }
    }

    /// One update of one actor (see [`Ai::step`]); the game is told what it does now
    /// ([`Ai::publish_phase`]).
    pub(crate) fn step_actor(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, dt: f32, open: &mut dyn Mover, quiet: bool) {
        self.step_actor_inner(game, nav, actor, dt, open, quiet);
        // A walk ended or set aside: the door it was coming up to is behind it too
        // ([`Ai::ask_door`]).
        if let Some(a) = self.agents.get_mut(&actor).filter(|a| a.activity != Activity::Travelling) {
            if let Some((left, _)) = a.asked_door.take() {
                game.npc_door_left(actor, FormId(left));
            }
        }
        self.publish_phase(game, actor);
    }

    /// Tell the game what `actor` does within its package (`fnv_game::ai::AiPhase`), for
    /// GetCurrentAIProcedure: nothing for an actor out of the high process (the original
    /// reads its package's main procedure there, 45 for a Sandbox; docs/spec-notes/packages.md
    /// §4, procedure codes), its sandbox's doing in a sandbox, else on its way or there.
    pub(crate) fn publish_phase(&self, game: &mut Game, actor: FormId) {
        let Some(a) = self.agents.get(&actor) else { return };
        let phase = if self.low.contains_key(&actor) || a.package.is_none() {
            None
        } else if let Some(sb) = &a.sandbox {
            Some(AiPhase::Sandbox(sb.phase()))
        } else if a.activity == Activity::Travelling {
            Some(AiPhase::Travelling)
        } else {
            Some(AiPhase::There)
        };
        if game.extra.get(&actor).and_then(|e| e.ai_phase) != phase {
            game.set_ai_phase(actor, phase);
        }
    }

    fn step_actor_inner(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, dt: f32, open: &mut dyn Mover, quiet: bool) {
        // SetActorsAI 0: no package choice, no steering and no turning until it is
        // switched on again, but a walk in progress goes on straight ahead ([`Ai::coast`]).
        // An unconscious actor does nothing [hypothesis]. A restrained one stays in place
        // but still picks packages [documented: GECK wiki SetRestrained, restrained
        // actors "will not move from their current position, or enter combat, but will
        // continue to \"think\" (pick packages ...)"; "will still attempt to run the
        // package and perform various animations"]: see [`RESTRAINED_REACH`].
        let held = game.extra.get(&actor).is_some_and(|e| e.dead || e.out_cold());
        if !game.present(actor) || held {
            return;
        }
        if self.agents[&actor].rooted {
            // A rooted actor's weapon use (a script's UseWeapon, or its Use Weapon
            // package) is used from where it stands: it cannot walk to a place [design,
            // as `weapon_step` does with no way there]. Before, a rooted turret's use
            // never reached `weapon_step`, so it never fired: ARCHIMEDES at HELIOS One
            // drew and holstered for good and That Lucky Old Sun's strike never came
            // (#2544).
            if game.active_weapon_use(actor).is_some_and(|w| !w.in_place) && !game.ai_off(actor) {
                game.weapon_use_in_place(actor);
            }
            self.rooted_turn(game, actor, dt);
            return;
        }
        if game.ai_off(actor) {
            self.coast(game, actor, dt, open);
            return;
        }
        let restrained = game.extra.get(&actor).is_some_and(|e| e.restrained);
        // Released: its package begins again, now with the walk to its place. Not a
        // Dialogue package that has already started its conversation in place: it would
        // greet a second time (the line that releases the actor is said in that
        // conversation, as in Hardin's power armor training, #2309).
        if !restrained && self.restrained.remove(&actor) {
            let a = self.agents.get_mut(&actor).expect("listed");
            if !matches!(a.talk, Some(Talking::Speaking | Talking::Done)) {
                a.package = None;
            }
        }
        // Woken by the player talking to it (`Game::wake`): off its bed while it stays
        // up; afterwards its package begins again, and takes it back to bed.
        if game.woken(actor) {
            self.woken.insert(actor);
            let on_bed = self.agents[&actor].using.is_some_and(|f| is_bed(game, f));
            if on_bed {
                self.set_using(game, actor, None);
            }
        } else if self.woken.remove(&actor) {
            self.agents.get_mut(&actor).expect("listed").package = None;
        }
        // ResetAI (`Game::take_ai_reset`): the package is taken up afresh even when the
        // same one is chosen again: off its furniture, the old goal, route and walk gone,
        // what a loaded save kept of its progress forgotten, and begun again. The request
        // waits while the AI does not run (off, dead, out cold, rooted: returned above). A
        // plain evaluation choosing the same package changes nothing [design,
        // actor-behaviour.md §5; the original's seated RingoRef read GET_UP one loop after
        // ResetAI, then TRAVEL, observed: owner-decision-captures.md od_06].
        let reset = game.take_ai_reset(actor);
        if reset {
            self.restored.remove(&actor);
            let a = self.agents.get_mut(&actor).expect("listed");
            a.restarts = a.restarts.wrapping_add(1);
        }
        let current = game.current_package(actor);
        if reset || self.agents[&actor].package != current {
            self.log(game, actor, AiEvent::Package { package: current });
            let a = self.agents.get_mut(&actor).expect("listed");
            a.package = current;
            a.goal = None;
            a.walker.follower = None;
            a.activity = Activity::None;
            a.sandbox = None;
            a.wait_until = None;
            a.talk = None;
            a.pushed_at = None;
            // A random conversation is given up (`crate::social` lets the other
            // one go).
            a.chat = None;
            a.search = None;
            a.face_to = None;
            a.following = None;
            a.errand = None;
            a.backing = None;
            // A walk-up still owed is planned again (`Ai::walk_up_step`): the new
            // package's walk must not be taken for the end of the walk-up's, which would
            // open the conversation from afar (a package changed in the low process).
            a.walk_up = None;
            a.walk_pace = None;
            self.set_using(game, actor, None);
            self.taken.retain(|_, user| *user != actor);
            // What the old package had it use ends with it.
            if let Some(e) = game.extra.get_mut(&actor) {
                e.used_item = None;
            }
            // The old package's weapon use ends with it.
            if game.weapon_use(actor).is_some_and(|w| matches!(w.source, Source::Package(_))) {
                game.drop_weapon_use(actor);
            }
            match current {
                Some(p) if restrained => self.begin_in_place(game, nav, actor, p),
                Some(p) => self.begin(game, nav, actor, p),
                None => {}
            }
        }
        if restrained {
            self.restrained.insert(actor);
            // Restrained while walking: it stops where it is.
            let a = self.agents.get_mut(&actor).expect("listed");
            if a.activity == Activity::Travelling {
                (a.walker.follower, a.activity, a.following) = (None, Activity::Staying, None);
            }
            // A Dialogue package still runs: the actor waits where it stands and talks
            // once its target is within the activate distance, without going up to it
            // [documented: GECK wiki SetRestrained, restrained actors "will not move from
            // their current position, or enter combat, but will continue to \"think\"
            // ... and go into dialogue"; docs/spec-notes/script-functions-catalogue.md,
            // "still thinks and can talk"]. Stopped on its way to the wait location or
            // the target, it waits where it stopped [hypothesis]. Before, the update ended
            // here and Hardin's training package never spoke, so the player stayed
            // restrained (#2309).
            if matches!(a.talk, Some(Talking::ToWait | Talking::Approaching { .. })) {
                a.talk = Some(Talking::Waiting);
            }
            if !quiet && self.agents[&actor].talk.is_some() {
                self.talk_step(game, nav, actor, dt);
            }
            return;
        }
        // A script's weapon use comes before the fight and the package
        // (`fnv_game::use_weapon`); a combat state before the package (`crate::alert`).
        let ordered = !quiet && (self.walk_up_step(game, nav, actor) || self.weapon_step(game, nav, actor, dt));
        let alerted = !quiet && !ordered && self.alert_step(game, nav, actor, dt);
        let chatting = self.agents[&actor].chat.is_some();
        // The companion wheel's Back Up (`fnv_game::companion_wheel`).
        if !quiet && !chatting && !alerted && !ordered && game.take_back_up(actor).is_some() {
            self.back_up(game, nav, actor);
        }
        if let Some(until) = self.agents[&actor].backing {
            let a = self.agents.get_mut(&actor).expect("listed");
            if a.activity != Activity::Travelling {
                a.backing = None;
            } else if game.time >= until {
                (a.backing, a.walker.follower, a.activity) = (None, None, Activity::There);
            }
        }
        if !quiet && !chatting && !alerted && self.agents[&actor].pushed_at.is_some_and(|t| game.time - t >= PUSHED_BACK_SECONDS) {
            self.back_after_push(game, nav, actor);
        }
        if self.agents[&actor].activity != Activity::Travelling {
            // Its walk is over: the next one is owned only if planned as a package's.
            self.agents.get_mut(&actor).expect("listed").walk_pace = None;
        }
        match self.agents[&actor].activity {
            Activity::Travelling => {
                if !quiet && !alerted && !ordered {
                    self.repace_walk(game, actor);
                }
                self.walk(game, nav, actor, dt, open)
            }
            Activity::There if !quiet && !chatting && !alerted && !ordered => {
                if let Some(h) = self.agents[&actor].face_to {
                    self.face_heading(game, actor, h, dt);
                }
                if self.agents[&actor].wait_until.is_some_and(|t| game.time >= t) {
                    self.agents.get_mut(&actor).expect("listed").wait_until = None;
                    self.patrol_on(game, nav, actor);
                } else if self.agents[&actor].sandbox.is_some() && self.awake(game, actor) && !game.woken(actor) {
                    self.sandbox_step(game, nav, actor);
                }
            }
            _ => {}
        }
        if !alerted && !ordered && self.agents[&actor].talk.is_some() {
            if quiet {
                // Wait-menu hours in the loaded area leave it as it is.
                if !self.is_high(actor) {
                    self.talk_step_far(game, nav, actor);
                }
            } else {
                self.talk_step(game, nav, actor, dt);
            }
        }
        let backing = self.agents[&actor].backing.is_some();
        if !alerted && !chatting && !ordered && !backing && matches!(self.agents[&actor].goal, Some(Goal::Follow(_))) {
            self.follow_step(game, nav, actor);
        }
        if !quiet && !alerted && !chatting && !ordered && self.agents[&actor].errand.is_some() {
            self.errand_step(game, nav, actor, dt);
        }
    }

    /// While a conversation with the player holds the game (nobody walks, [`Ai::update`]
    /// is not called): the actor in it turns to the player on the spot and keeps facing
    /// the player for the whole conversation, however it started (the player's
    /// activation, StartConversation, its Dialogue package) [documented for a Dialogue
    /// package: speaker and target are "forced to look at each other"; design for the
    /// others: the owner's playtest of 2026-10-11 found an actor talked to turning back
    /// to its old heading after its first turn, because the engine drew it at this
    /// walker's heading]. The heading is the walker's, and the game's, so it stays
    /// after the conversation until a package turns the actor. Not turned: an actor on
    /// furniture (its head turns, the engine's), a dead one, one in another space, one
    /// whose Dialogue package says No Headtracking, and a rooted one ([`Ai::root`]: an
    /// Immobile creature such as Mr. House's screen, which "prevents movement"
    /// [documented: GECK wiki Animation Tab]; the owner saw the screen turn 180 degrees
    /// to the player where the original's stays still [observed: playtest 2026-11-04,
    /// issue #1543]).
    pub fn update_held(&mut self, game: &mut Game, dt: f32) {
        let Some(actor) = game.dialogue.as_ref().map(|c| c.actor) else { return };
        let Some(a) = self.agents.get(&actor) else { return };
        if a.using.is_some() || a.rooted || game.extra.get(&actor).is_some_and(|e| e.dead) {
            return;
        }
        if let (Some(Goal::Talk(talk)), Some(Talking::Speaking)) = (&a.goal, a.talk) {
            if talk.no_headtracking {
                return;
            }
        }
        let space = a.walker.space;
        if let Some(t) = goal::place_of_ref(game, FormId::PLAYER_REF).filter(|t| t.space == space) {
            self.face(game, actor, t, dt);
        }
    }

    /// One update of a Dialogue package [documented: GECK wiki, Dialogue Package,
    /// "Behavior"; the cases are numbered as there]:
    /// - the actor first goes to the wait location, if the package has one;
    /// - (3) without a trigger location it then goes up to its target at once;
    /// - (1) with one, it stays until the target is in the trigger location and goes up
    ///   to it then;
    /// - (2) with one and a wait location of radius 0, it stays "until the target reaches
    ///   the Activate Distance". It also goes up to a target that is in the trigger
    ///   location, as in (1) [hypothesis: the page does not say so for this case, but the
    ///   trigger locations of such packages are boxes placed around the spot (32 of 33 in
    ///   FalloutNV.esm), and the player comes into the Prospector Saloon inside Sunny
    ///   Smiles's box, 343 units from her, with an activate distance of 256 and a greeting
    ///   that starts as the player comes in (owner, 2026-10-10)];
    /// - within the activate distance it "will engage in conversation with the target (or
    ///   dialogue, if the target is the player)".
    ///
    /// An actor seated at a wait location that is furniture stays on it: it does not
    /// go up to the target in any case, and talks from its seat once the target is
    /// within the activate distance [observed: the owner's frame of Benny in the
    /// Tops' presidential suite (VTopsBennyPresidentialTalk: wait location the bar
    /// stool 0x001110C4, no trigger location, so case 3) greeting the player from his
    /// stool, never getting up; hypothesis: that every seated waiter does so,
    /// docs/spec-notes/packages.md §3].
    ///
    /// The wait location's radius is not wandered in; the target is not made to stand
    /// still or to walk to the trigger location ("Do Not Control Target Movement" off).
    /// Except that with that flag off, in case 2 a player within the activate distance
    /// but outside the trigger location is walked into it first (issue #2453, below).
    fn talk_step(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, dt: f32) {
        let a = &self.agents[&actor];
        let (Some(package), Some(Goal::Talk(talk)), Some(phase)) = (a.package, a.goal.clone(), a.talk) else { return };
        let feet = Place { space: a.walker.space, pos: a.walker.feet };
        let travelling = a.activity == Activity::Travelling;
        // Seated at a wait location that is furniture (`settle`).
        let seated = a.using.is_some();
        // Seated or restrained: it does not go up to the target (`step_actor_inner`).
        let stays = seated || game.extra.get(&actor).is_some_and(|e| e.restrained);
        let target = goal::place_of_ref(game, talk.target).filter(|_| game.present(talk.target) && !game.extra.get(&talk.target).is_some_and(|e| e.dead));
        match phase {
            Talking::ToWait | Talking::Done => return,
            Talking::Speaking => {
                // A greeting with the Goodbye flag opens no conversation
                // (`Game::start_dialogue`): the actor talks for as long as it says the
                // line [hypothesis: the package lasts as long as the line, as it lasts
                // as long as a conversation].
                // A conversation with another actor lasts while either says its lines.
                let with_actor = talk.target != FormId::PLAYER_REF && game.in_actor_conversation(actor);
                if game.dialogue.as_ref().is_some_and(|c| c.actor == actor) || greets(game, actor) || with_actor {
                    // It turns to the one it talks to [documented: unless No
                    // Headtracking, speaker and target are "forced to look at each
                    // other by the package"; how fast is our choice]. Seated, only its
                    // head turns (the engine's); rooted, nothing: as in `update_held`.
                    let rooted = self.agents[&actor].rooted;
                    if let (Some(t), false, false, false) = (target.filter(|t| t.space == feet.space), talk.no_headtracking, seated, rooted) {
                        self.face(game, actor, t, dt);
                    }
                } else {
                    if talk.target == FormId::PLAYER_REF {
                        self.talk_ended.insert(actor, game.time);
                    }
                    self.talk_done(game, actor);
                    self.log(game, actor, AiEvent::TalkOver { package });
                }
                return;
            }
            Talking::Waiting | Talking::Approaching { .. } => {}
        }
        // A target that is nowhere, gone or dead: the actor waits where it is.
        let Some(target) = target else {
            if matches!(phase, Talking::Approaching { .. }) {
                let a = self.agents.get_mut(&actor).expect("listed");
                a.walker.follower = None;
                a.activity = Activity::There;
                a.talk = Some(Talking::Waiting);
            }
            return;
        };
        let away = if feet.space == target.space { goal::dist(feet.pos, target.pos) } else { f32::INFINITY };
        let near = away <= talk.distance.max(self.cfg.arrive);
        match phase {
            Talking::Waiting => {
                let triggered = talk.trigger.holds(game, feet, target);
                // Case 2: a trigger location and a wait location without a radius.
                let on_the_spot = talk.trigger != goal::Trigger::None && talk.wait_radius == 0.0;
                // With "Do Not Control Target Movement" off the target "will walk
                // towards that location [the trigger location] when the dialogue
                // package starts" [documented: GECK wiki, Dialogue Package]. End scripts
                // count on the player standing in it: Ramos's "Follow me" (INFO
                // 000A3CE0) leaves the escort to an OnTrigger block of the trigger
                // location itself (HVRamosTargetDialogTrigger), which runs only with the
                // player inside it (issue #2453). So once the player is within the
                // activate distance (and in sight, as for the greeting itself) but not
                // in the trigger location, the player is walked into it
                // (`Game::player_lead`) and the conversation opens there [hypothesis:
                // when the walk starts, and that it ends inside the location;
                // docs/spec-notes/packages.md §3].
                // Not again soon after a conversation with the same actor [our choice,
                // #3464]: Harland's room in the REPCONN basement has a trigger on the way
                // out that calls ResetAI on him (REPCONHarlandResetTrigger), and with an
                // activate distance of 1200 his package, taken up afresh, walked the
                // player straight back into the doorway trigger and greeted again, every
                // time: the player could not leave. Without the walk he talks where the
                // player stands, once (its end script then holds the doorway's evp).
                let recent = self.talk_ended.get(&actor).is_some_and(|&t| game.time - t < LEAD_AGAIN_SECONDS);
                let leads = on_the_spot && talk.control_target && talk.target == FormId::PLAYER_REF && talk.trigger.has_room(game) && !recent;
                if near && triggered {
                    self.talk_start(game, actor, package, &talk, away);
                } else if near && leads {
                    self.lead_player(game, actor, package, &talk, feet, target, away);
                } else if near && on_the_spot {
                    self.talk_start(game, actor, package, &talk, away);
                } else if triggered && !stays {
                    self.approach(game, nav, actor, package, &talk, target, true);
                }
            }
            Talking::Approaching { to, replan } => {
                let moved = to.space != target.space || goal::dist(to.pos, target.pos) > REPLAN_DISTANCE;
                if near || (!travelling && !moved) {
                    // Near enough, or as near as the navmesh lets it get.
                    self.talk_start(game, actor, package, &talk, away);
                } else if moved && game.time >= replan {
                    self.approach(game, nav, actor, package, &talk, target, false);
                }
            }
            _ => {}
        }
    }

    /// A Dialogue package out of the high process ([`crate::process`]): the actor
    /// sets off towards its target, but does not talk. The original's Malcolm Holmes,
    /// sent at the player across the Mojave by the first star cap, kept to the route he
    /// took at the start for 3 hours after the player moved 66,000 units away
    /// [observed 2026-10-24, holmes_far_moving]: so no new plan while the old one
    /// lasts. At its end, with the target gone elsewhere, it plans again [hypothesis:
    /// not seen past 3 hours]. Talking waits for the high process.
    ///
    /// Only a package without a wait location does this (Holmes's has none); one with a
    /// wait location keeps its actor there [hypothesis: Ringo's GSRingoDialoguePackage
    /// waits at his editor location and should not follow the player out of the gas
    /// station, smoke `ringo-comes-out`].
    fn talk_step_far(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) {
        let a = &self.agents[&actor];
        let (Some(package), Some(Goal::Talk(talk)), Some(phase)) = (a.package, a.goal.clone(), a.talk) else { return };
        if !matches!(*talk.wait, Goal::Here) {
            return;
        }
        let feet = Place { space: a.walker.space, pos: a.walker.feet };
        let travelling = a.activity == Activity::Travelling;
        let Some(target) = goal::place_of_ref(game, talk.target).filter(|_| game.present(talk.target) && !game.extra.get(&talk.target).is_some_and(|e| e.dead)) else {
            return;
        };
        let away = if feet.space == target.space { goal::dist(feet.pos, target.pos) } else { f32::INFINITY };
        if away <= talk.distance.max(self.cfg.arrive) {
            return;
        }
        match phase {
            Talking::Waiting if talk.trigger.holds(game, feet, target) => self.approach(game, nav, actor, package, &talk, target, true),
            Talking::Approaching { to, .. } if !travelling => {
                let moved = to.space != target.space || goal::dist(to.pos, target.pos) > REPLAN_DISTANCE;
                if moved {
                    self.approach(game, nav, actor, package, &talk, target, false);
                }
            }
            _ => {}
        }
    }

    /// One update of a Follow or Accompany package ([`Goal::Follow`]): an actor that
    /// stands sets off after its target once the target is more than its distance away
    /// (in another space: through the doors between), and the way is planned again when
    /// the target has moved [`REPLAN_DISTANCE`] since, at most every
    /// [`REPLAN_SECONDS`]; it stops once [`FOLLOW_STOP_INSET`] inside the distance
    /// ([`follow_stop_at`], [design, #2122]). It runs when the package
    /// says Always Run or it is more than [`FOLLOW_RUN_BEHIND`] beyond its distance, else
    /// walks; on its way after a target that moves it keeps step with it
    /// ([`keep_step_speed`]) near its distance, so it stops only once the target stops or
    /// slows [our choice]. A target that is gone or dead is not followed: the actor stands.
    /// Without a way to the target the package is set aside like any other that cannot
    /// be reached. Several actors following one target walk after it in a line, each
    /// after the one ahead of it ([`Ai::ahead_in_column`]).
    fn follow_step(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) {
        let a = &self.agents[&actor];
        let (Some(package), Some(Goal::Follow(mut f))) = (a.package, a.goal.clone()) else { return };
        let feet = Place { space: a.walker.space, pos: a.walker.feet };
        let travelling = a.activity == Activity::Travelling;
        // Told to wait (Fallout 3's Wait, `fnv_game::functions::fo3`): it stands where it
        // is until StopWaiting, then follows again.
        if game.waits(actor) {
            if travelling {
                let a = self.agents.get_mut(&actor).expect("listed");
                (a.walker.follower, a.activity, a.following) = (None, Activity::There, None);
            }
            return;
        }
        let leader = goal::place_of_ref(game, f.target).filter(|_| game.present(f.target) && !game.extra.get(&f.target).is_some_and(|e| e.dead));
        // In a column: the one ahead is followed instead, nearer than the package says.
        // It runs when the leader is far, so that the line keeps together.
        let ahead = leader.and_then(|l| self.ahead_in_column(game, actor, f.target, l));
        let target = match ahead {
            Some((one, at, _)) => {
                (f.target, f.distance) = (one, f.distance.min(COLUMN_GAP));
                Some(at)
            }
            None => leader,
        };
        // How far the leader is beyond where this one's place in the line would be.
        let place = ahead.map_or(0, |(_, _, i)| i) as f32;
        let lead_away = match leader {
            Some(l) if l.space == feet.space => hdist(feet.pos, l.pos) - place * (COLUMN_GAP + FOLLOW_SLACK),
            // In another space: the way to the one ahead decides alone.
            _ => 0.0,
        };
        let Some(target) = target else {
            if travelling {
                let a = self.agents.get_mut(&actor).expect("listed");
                (a.walker.follower, a.activity, a.following) = (None, Activity::There, None);
            }
            return;
        };
        let away = if feet.space == target.space { hdist(feet.pos, target.pos) } else { f32::INFINITY };
        let keep = f.distance.max(self.cfg.arrive);
        if travelling {
            let following = self.agents[&actor].following;
            let moved = following.is_none_or(|(to, _, _)| to.space != target.space || goal::dist(to.pos, target.pos) > REPLAN_DISTANCE);
            // In step with a target on the move: one that went at least FOLLOW_MOVING and
            // has not stood about since (a second more without the way planned again).
            let in_step = following.is_some_and(|(_, replan, pace)| pace.is_some_and(|v| v >= FOLLOW_MOVING) && (moved || game.time < replan + 1.0));
            let stop_at = follow_stop_at(keep, self.cfg.arrive, in_step, ahead.is_some());
            if away <= stop_at && (feet.pos[2] - target.pos[2]).abs() <= 256.0 {
                let a = self.agents.get_mut(&actor).expect("listed");
                (a.walker.follower, a.activity, a.following) = (None, Activity::There, None);
                self.log(game, actor, AiEvent::CaughtUp { package, target: f.target, away, stop_at });
                return;
            }
            let Some((to, replan, pace)) = following else { return };
            if moved && game.time >= replan {
                // The target's pace since the way was last planned.
                let since = (game.time - (replan - REPLAN_SECONDS)) as f32;
                let target_pace = (to.space == target.space && since > 0.0).then(|| hdist(to.pos, target.pos) / since);
                self.follow_plan(game, nav, actor, package, f, target, away, lead_away, target_pace, false);
            } else if away.is_finite() {
                // The pace follows the gap: the catch-up boost eases off as it closes, and
                // in step it keeps to the target's.
                let (speed, _) = self.follow_speed(game, actor, package, away, keep, lead_away, pace);
                let a = self.agents.get_mut(&actor).expect("listed");
                (a.walker.speed, a.walk_pace) = (speed, None);
            }
            return;
        }
        if self.agents[&actor].activity == Activity::There && away > keep + FOLLOW_SLACK {
            self.follow_plan(game, nav, actor, package, f, target, away, lead_away, None, true);
        }
    }

    /// The one `actor` walks after when it follows `leader` (at `leader_at`) along with
    /// other actors: their column, in which each walks after the one ahead of it, the
    /// first after the leader: that one, where it is, and `actor`'s place in the line
    /// (1 for the second). Returns `None` for the first, or when `actor` is not in the
    /// column. Members are the actors whose package follows `leader`, present and
    /// alive. One ahead that is neither in the leader's space nor in `actor`'s (left
    /// behind elsewhere) is passed over. Their order stays as it was; newcomers take
    /// their places at the end, the nearer the leader the further forward, those in
    /// another space last [our choice: a line that kept reordering by distance would
    /// have followers overtake each other].
    fn ahead_in_column(&mut self, game: &Game, actor: FormId, leader: FormId, leader_at: Place) -> Option<(FormId, Place, usize)> {
        let member = |id: FormId, a: &Agent| matches!(&a.goal, Some(Goal::Follow(g)) if g.target == leader) && game.present(id) && !game.extra.get(&id).is_some_and(|e| e.dead);
        let members: Vec<FormId> = self.agents.iter().filter(|(id, a)| member(**id, a)).map(|(id, _)| *id).collect();
        if !members.contains(&actor) {
            return None;
        }
        let column = self.columns.entry(leader).or_default();
        column.retain(|id| members.contains(id));
        let away = |w: &fnv_nav::Walker| if w.space == leader_at.space { hdist(w.feet, leader_at.pos) } else { f32::INFINITY };
        let mut new: Vec<(f32, FormId)> = members.iter().filter(|id| !column.contains(id)).map(|id| (away(&self.agents[id].walker), *id)).collect();
        new.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        column.extend(new.into_iter().map(|(_, id)| id));
        let i = column.iter().position(|id| *id == actor)?;
        let mine = self.agents[&actor].walker.space;
        let one = column[..i].iter().rev().copied().find(|id| [leader_at.space, mine].contains(&self.agents[id].walker.space))?;
        let a = &self.agents[&one];
        Some((one, Place { space: a.walker.space, pos: a.walker.feet }, i))
    }

    /// The pace of a follower `away` from the one it walks after, which it is to keep
    /// within `keep` (`lead_away`: see [`Ai::follow_plan`]), and whether it runs: its run
    /// with the catch-up boost ([`catch_up`]) when far behind, else in step with a target
    /// going at `target_pace` ([`keep_step_speed`]), else its walk.
    #[allow(clippy::too_many_arguments)]
    fn follow_speed(&self, game: &Game, actor: FormId, package: FormId, away: f32, keep: f32, lead_away: f32, target_pace: Option<f32>) -> (f32, bool) {
        let behind = (away - keep).max(lead_away - keep);
        let running = game.package_def(&package).is_some_and(|d| d.has(flags::ALWAYS_RUN)) || behind > FOLLOW_RUN_BEHIND;
        let speed = match target_pace {
            Some(v) if !running => keep_step_speed(v, away, keep, self.pace(game, actor, false), self.pace(game, actor, true)),
            _ => self.pace(game, actor, running) * catch_up(behind),
        };
        (speed, running)
    }

    /// Plan the way after the one `actor` follows. It runs when the package says
    /// Always Run, the one it walks after is far, or the leader of its column is far
    /// beyond its place in the line (`lead_away`); `target_pace` is how fast the target
    /// has gone since the last plan, when known.
    #[allow(clippy::too_many_arguments)]
    fn follow_plan(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, package: FormId, f: goal::Follow, target: Place, away: f32, lead_away: f32, target_pace: Option<f32>, first: bool) {
        let keep = f.distance.max(self.cfg.arrive);
        let (speed, running) = self.follow_speed(game, actor, package, away, keep, lead_away, target_pace);
        let pace = self.pace(game, actor, running);
        let params = self.path_params(game, actor);
        let a = self.agents.get_mut(&actor).expect("listed");
        (a.walker.speed, a.walk_pace) = (speed, None);
        match a.walker.go_to(nav, target, &params) {
            Ok(path) => {
                let (length, doors) = (path.length, path.legs.iter().filter_map(|l| l.door.map(|d| FormId(d.door))).collect());
                // In another space the length of the way is the gap the boost goes by.
                if !away.is_finite() {
                    (a.walker.speed, a.walk_pace) = (pace * catch_up(length - keep), None);
                }
                if a.activity != Activity::Travelling {
                    a.trip_started = game.time;
                    a.trip_from = a.walker.travelled;
                }
                a.activity = Activity::Travelling;
                a.following = Some((target, game.time + REPLAN_SECONDS, target_pace));
                if first {
                    let away = away.is_finite().then_some(away);
                    self.log(game, actor, AiEvent::Follow { package, target: f.target, away, length, doors, running });
                }
            }
            Err(e) => {
                a.following = None;
                let from = Place { space: a.walker.space, pos: a.walker.feet };
                self.no_way_after(game, nav, actor, package, from, target, e);
            }
        }
    }

    /// Plan the way to the one `actor` is to talk to; without one the package is set
    /// aside like any other that cannot be reached.
    #[allow(clippy::too_many_arguments)]
    fn approach(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, package: FormId, talk: &Talk, target: Place, first: bool) {
        let running = game.package_def(&package).is_some_and(|d| d.has(flags::ALWAYS_RUN));
        let params = self.path_params(game, actor);
        let speed = self.pace(game, actor, running);
        let own = Ai::walk_pace_of(game, actor, running, speed);
        let a = self.agents.get_mut(&actor).expect("listed");
        (a.walker.speed, a.walk_pace) = (speed, own);
        let from = Place { space: a.walker.space, pos: a.walker.feet };
        match a.walker.go_to(nav, target, &params) {
            Ok(path) => {
                let (length, doors): (f32, Vec<FormId>) = (path.length, path.legs.iter().filter_map(|l| l.door.map(|d| FormId(d.door))).collect());
                // The way ends short of the target, inside the talking distance, not on
                // its spot: a wait walks it to its end (issue #1597) [design: three
                // quarters of the distance].
                if let Some(f) = a.walker.follower.as_mut() {
                    f.goal_radius = f.goal_radius.max(talk.distance * 0.75);
                }
                if a.activity != Activity::Travelling {
                    a.trip_started = game.time;
                    a.trip_from = a.walker.travelled;
                }
                a.activity = Activity::Travelling;
                a.talk = Some(Talking::Approaching { to: target, replan: game.time + REPLAN_SECONDS });
                if first {
                    let away = (from.space == target.space).then(|| goal::dist(from.pos, target.pos));
                    self.log(game, actor, AiEvent::Approach { package, target: talk.target, away, length, doors, running });
                }
            }
            Err(e) => {
                a.talk = None;
                self.no_way_after(game, nav, actor, package, from, target, e);
            }
        }
    }

    /// `actor` is near enough to the one it is to talk to: it stops and talks. A
    /// conversation that cannot start now because the game is held (another
    /// conversation, a menu) starts when it can.
    fn talk_start(&mut self, game: &mut Game, actor: FormId, package: FormId, talk: &Talk, away: f32) {
        // A force greet needs a clear line to the target: not through a wall or a closed
        // door (`Game::greet_sight`). So does a Say To line to the player: Officer
        // Kendall's "There he is!" (TTW, CG04Security02Ambush, activate distance 1500)
        // came through the walls while the player was still in bed, and its end script's
        // stage 12 kept the wake-up's stage 3 from coming (docs/design/actor-behaviour.md).
        // Only the player is measured, so a Say To line or a conversation to anyone else
        // (an actor, a talking activator) is not held back. The actor stays as it is
        // (waiting, or at the end of its walk) and tries again every update.
        let needs_sight = talk.target == FormId::PLAYER_REF;
        // The player in another space than the actor (a load door just took them out,
        // which moves the player's cell at once while the walkers' places catch up a
        // frame later) is not talked to. Gomorrah's entrance guard started its weapons
        // check with a player leaving by the inner door, the door's script having reset
        // iInGomorrah; the hand-over set it again and the next way in kept the controls
        // the outer door took (#2835).
        if needs_sight && !same_space(game, actor, talk.target) {
            return;
        }
        if needs_sight && !game.greet_sight(actor, talk.target) {
            if self.unseen_greets.insert(actor) {
                self.log(game, actor, AiEvent::TalkUnseen { package, target: talk.target, away });
            }
            return;
        }
        self.unseen_greets.remove(&actor);
        let a = self.agents.get_mut(&actor).expect("listed");
        a.walker.follower = None;
        a.activity = Activity::There;
        if talk.say_to {
            let Some(topic) = talk.topic else { return };
            match game.say_package_line(actor, talk.target, topic) {
                Some(line) => self.log(game, actor, AiEvent::Talk { package, target: talk.target, line, away, say_to: true, greeting: false }),
                None => {
                    let why = format!("it has no line of {} to say", game.data.name(topic));
                    self.log(game, actor, AiEvent::NoTalk { package, why });
                }
            }
            self.talk_done(game, actor);
            return;
        }
        if game.in_menu() || game.loading {
            return;
        }
        // A conversation with another actor, or with a talking activator, which speaks
        // for its talking actor (`Game::start_actor_conversation`): said in the world.
        if talk.target != FormId::PLAYER_REF {
            match game.start_actor_conversation(actor, talk.target, talk.topic) {
                Ok(line) => {
                    self.agents.get_mut(&actor).expect("listed").talk = Some(Talking::Speaking);
                    self.log(game, actor, AiEvent::Talk { package, target: talk.target, line, away, say_to: false, greeting: false });
                }
                Err(why) => {
                    self.log(game, actor, AiEvent::NoTalk { package, why });
                    self.talk_done(game, actor);
                }
            }
            return;
        }
        // The force greet: a conversation also for a greeting with the Goodbye flag
        // (`Game::force_greet`).
        match game.force_greet(actor, talk.topic) {
            Ok(line) => {
                Self::end_lead(game, actor);
                self.agents.get_mut(&actor).expect("listed").talk = Some(Talking::Speaking);
                let greeting = greets(game, actor);
                self.log(game, actor, AiEvent::Talk { package, target: talk.target, line, away, say_to: false, greeting });
            }
            // Nothing to say: the package is over, or the actor would try every frame
            // [our choice].
            Err(why) => {
                self.log(game, actor, AiEvent::NoTalk { package, why });
                self.talk_done(game, actor);
            }
        }
    }

    /// The Dialogue package has done what it is for: its goal is met [documented: a
    /// Must Complete Dialogue package "will not reevaluate until actor has engaged in
    /// dialogue with the target"]. The actor stays in the package until it chooses
    /// another; it does not start over [our choice].
    /// Walk the player into the trigger location of `actor`'s Dialogue package
    /// ([`Self::talk_step`]): set [`Game::player_lead`] again, once the actor sees the
    /// player (as for a force greet, `Game::greet_sight`). Without a point to walk to,
    /// the actor talks where the player is.
    #[allow(clippy::too_many_arguments)]
    fn lead_player(&mut self, game: &mut Game, actor: FormId, package: FormId, talk: &Talk, feet: Place, target: Place, away: f32) {
        if !game.greet_sight(actor, talk.target) {
            if self.unseen_greets.insert(actor) {
                self.log(game, actor, AiEvent::TalkUnseen { package, target: talk.target, away });
            }
            return;
        }
        self.unseen_greets.remove(&actor);
        let Some(to) = talk.trigger.lead_point(game, feet, target) else {
            self.talk_start(game, actor, package, talk, away);
            return;
        };
        let new = !game.player_lead.is_some_and(|l| l.by == actor && l.fresh(game.time));
        // A walk that gets no nearer gives up: the player is never held walking into
        // something solid for as long as the actor sees them [our choice, review of
        // #2465]; the actor greets where the player stands.
        let left = (to[0] - target.pos[0]).hypot(to[1] - target.pos[1]);
        let now = game.time;
        let p = self.lead_progress.entry(actor).or_insert((left, now));
        if new {
            *p = (left, now);
        }
        if left < p.0 - LEAD_GAIN {
            *p = (left, now);
        } else if now - p.1 > LEAD_STUCK_SECONDS {
            self.lead_progress.remove(&actor);
            Self::end_lead(game, actor);
            self.log(game, actor, AiEvent::LeadStuck { package, left });
            self.talk_start(game, actor, package, talk, away);
            return;
        }
        game.player_lead = Some(fnv_game::game::PlayerLead { by: actor, to, at: game.time });
        if new {
            self.log(game, actor, AiEvent::LeadsPlayer { package, to, away });
        }
    }

    /// The lead of `actor`'s package is over ([`Self::lead_player`]).
    fn end_lead(game: &mut Game, actor: FormId) {
        if game.player_lead.is_some_and(|l| l.by == actor) {
            game.player_lead = None;
        }
    }

    fn talk_done(&mut self, game: &mut Game, actor: FormId) {
        Self::end_lead(game, actor);
        let a = self.agents.get_mut(&actor).expect("listed");
        a.talk = Some(Talking::Done);
        a.activity = Activity::There;
        game.package_goal_met(actor);
    }

    /// Turn a standing actor towards the heading it was given on arriving
    /// ([`Agent::face_to`]) for `dt` seconds, and tell the game its heading.
    fn face_heading(&mut self, game: &mut Game, actor: FormId, heading: f32, dt: f32) {
        let a = self.agents.get_mut(&actor).expect("listed");
        if a.walker.face(heading, dt) {
            a.face_to = None;
        }
        let (feet, heading) = (Place { space: a.walker.space, pos: a.walker.feet }, a.walker.heading);
        let cell = goal::cell_of_place(game.world.placed, &feet);
        let _ = game.set_actor_place(actor, cell, feet.pos, heading);
    }

    /// A rooted actor (an Immobile turret, [`Ai::root`]) stays where it is but turns in
    /// place, at the combat turn rate, to its combat target (in Combat: the target it
    /// detects, else the one detected last) or to the place of its alarm (Alert, Lost), as
    /// a fighter standing its ground does (`crate::alert`). Before, it never turned, so a
    /// HELIOS One ceiling turret kept its placed heading and the player stayed out of its
    /// view (#1537). A weapon use's target comes before the fight's (#2544). SetActorsAI 0
    /// holds it as any other actor. One that never turns ([`Ai::fix_heading`], Mr.
    /// House's screen) does not here either (#2150).
    fn rooted_turn(&mut self, game: &mut Game, actor: FormId, dt: f32) {
        if game.ai_off(actor) || game.extra.get(&actor).is_some_and(|e| e.restrained) {
            return;
        }
        // Its weapon use's target first, unless the fight took the use over (#2544): the
        // shooter fires only once it faces the target (`use_weapon::shot_facing`).
        let space = self.agents[&actor].walker.space;
        if let Some(t) = game.active_weapon_use(actor).filter(|w| w.in_place && !w.taken).map(|w| w.target) {
            if let Some(to) = goal::weapon_target_place(game, t, space) {
                self.combat_face(game, actor, to, dt);
                return;
            }
        }
        let state = game.awareness.state(actor);
        if !state.in_combat() {
            return;
        }
        let Some(aw) = game.awareness.get(actor) else { return };
        let at = if state == fnv_game::awareness::State::Combat {
            let latest = aw.targets.iter().filter(|(_, t)| t.detected.is_some()).max_by(|x, y| x.1.detected.unwrap_or(0.0).total_cmp(&y.1.detected.unwrap_or(0.0)));
            aw.combat_target().or(latest.map(|(id, t)| (*id, t))).map(|(_, t)| t.at)
        } else {
            aw.search_alarm().map(|a| a.at)
        };
        let Some(at) = at else { return };
        self.combat_face(game, actor, Place { space, pos: at }, dt);
    }

    /// [`Self::face`] at the combat turn rate: a person's `crate::alert::combat_turn`, a
    /// creature's `crate::alert::COMBAT_TURN_CREATURE`.
    pub(crate) fn combat_face(&mut self, game: &mut Game, actor: FormId, to: Place, dt: f32) {
        let w = &self.agents[&actor].walker;
        let off = fnv_nav::follow::heading_delta(w.heading, heading_of([to.pos[0] - w.feet[0], to.pos[1] - w.feet[1]]));
        let rate = if crate::alert::creature(game, actor) { crate::alert::COMBAT_TURN_CREATURE } else { crate::alert::combat_turn(off) };
        self.face(game, actor, to, dt * rate);
    }

    /// Turn a standing actor towards a place for `dt` seconds, and tell the game its
    /// heading. One that never turns ([`Ai::fix_heading`]) keeps its heading, whoever
    /// asks: its conversation, a line it says to someone, a fight.
    pub(crate) fn face(&mut self, game: &mut Game, actor: FormId, to: Place, dt: f32) {
        let a = self.agents.get_mut(&actor).expect("listed");
        if a.fixed {
            return;
        }
        let d = [to.pos[0] - a.walker.feet[0], to.pos[1] - a.walker.feet[1]];
        if d[0].hypot(d[1]) < 1.0 {
            return;
        }
        a.walker.face(heading_of(d), dt);
        let (feet, heading) = (Place { space: a.walker.space, pos: a.walker.feet }, a.walker.heading);
        let cell = goal::cell_of_place(game.world.placed, &feet);
        let _ = game.set_actor_place(actor, cell, feet.pos, heading);
    }

    /// Whether `actor` is where the game is loaded: in one of the cells around the
    /// player, or anywhere when the game has no loaded cells (a headless run without a
    /// player). Sandboxing goes on only there [observed: sandboxing actors in unloaded
    /// cells stood on one spot for hours (GSChetRef, RingoRef, DocMitchellREF in the
    /// owner's captures), while package changes still reached them; packages.md §4].
    fn awake(&self, game: &Game, actor: FormId) -> bool {
        if game.loaded_cells.is_empty() {
            return true;
        }
        let a = &self.agents[&actor];
        let place = Place { space: a.walker.space, pos: a.walker.feet };
        goal::cell_of_place(game.world.placed, &place).is_some_and(|c| game.loaded_cells.contains(&c))
    }

    /// Furniture kept for, or used by, actors other than `actor`.
    pub(crate) fn taken_by_others(&self, actor: FormId) -> Vec<FormId> {
        self.taken.iter().filter(|(_, u)| **u != actor).map(|(f, _)| *f).collect()
    }

    /// Keep furniture for `actor` (it is on its way to it).
    pub(crate) fn reserve(&mut self, actor: FormId, furniture: FormId) {
        self.taken.retain(|_, user| *user != actor);
        self.taken.insert(furniture, actor);
    }

    /// The furniture `actor` uses changes (to none: it gets up).
    pub(crate) fn set_using(&mut self, game: &Game, actor: FormId, furniture: Option<FormId>) {
        let Some(a) = self.agents.get_mut(&actor) else { return };
        // A script's Look turning the body keeps the actor standing by the seat, kept for
        // it, until StopLook [observed: RingoRef sat down 0.15 s after StopLook, beside
        // his chair since the Look began, campaign `knockdown_look` 2026-10-19].
        a.held_seat = None;
        if let Some(f) = furniture.filter(|_| a.using.is_none() && game.body_looks().iter().any(|(w, _)| *w == actor)) {
            a.held_seat = Some(f);
            self.taken.retain(|_, user| *user != actor);
            self.taken.insert(f, actor);
            return;
        }
        if a.using == furniture {
            return;
        }
        a.using = furniture;
        // What the actor had, or had its eye on, is free again.
        self.taken.retain(|_, user| *user != actor);
        if let Some(f) = furniture {
            self.taken.insert(f, actor);
        }
        self.log(game, actor, AiEvent::Furniture { furniture });
    }

    /// The actor is where its package wants it: a Sandbox or Wander package starts its
    /// sandbox there; another package may give it furniture
    /// ([`goal::package_furniture`]); a Guard whose location has a radius wanders within
    /// it ([`guard_wander_radius`], [`Sandbox::guard`]).
    fn settle(&mut self, game: &mut Game, actor: FormId) {
        let a = &self.agents[&actor];
        let Some(package) = a.package else { return };
        let Some(def) = game.package_def(&package) else { return };
        let at = Place { space: a.walker.space, pos: a.walker.feet };
        // A guard wanders round its location's place; "near current location" and the
        // other places without one, round where it settled [design, #2027].
        let guard = guard_wander_radius(def).map(|radius| (if let Some(Goal::Near { place, .. }) = &a.goal { *place } else { at }, radius));
        let mut sandbox = None;
        if matches!(def.ty, PackageType::Sandbox | PackageType::Wander) {
            sandbox = Some(Sandbox::start(game, actor, package, def, a.goal.as_ref().unwrap_or(&Goal::Here), at));
        } else if let Some(f) = goal::package_furniture(game, &self.taken, actor, def, at, &mut self.furniture) {
            self.set_using(game, actor, Some(f));
        } else if let Some((centre, radius)) = guard {
            sandbox = Some(Sandbox::guard(game, actor, package, def, centre, radius));
        } else if let Some(heading) = marker_heading(game, actor, def, a.goal.as_ref()) {
            // At an xMarkerHeading it stands the way the marker faces [documented: GECK
            // wiki, NPC population, level-design.md: the heading version "shows which way
            // an actor standing there will face"]. Mr. Brotch's teaching spot in the
            // G.O.A.T. (CG03MrBrotchTeachMarker, 22°) faces the desks; before, he kept the
            // heading he walked in with and taught the wall.
            self.agents.get_mut(&actor).expect("listed").face_to = Some(heading);
            self.log(game, actor, AiEvent::ArrivalFacing { heading });
        }
        if let Some(mut sandbox) = sandbox {
            if let Some(s) = self.restored_schedule(actor, package) {
                sandbox.set_schedule(&s);
                self.log(game, actor, AiEvent::Resumed { package, patrol_point: None, sandbox: true });
            }
            self.agents.get_mut(&actor).expect("listed").sandbox = Some(sandbox);
        }
    }

    /// Whether `actor` keeps away from radiation now: its package allows the Fallout
    /// behaviour Avoid Radiation ("Allow actors to avoid entering radiated areas"
    /// [documented: Category:Packages]; `crate::social::allows`) and its Rad Resistance
    /// is below `fRadiationResistanceAvoidLevel` (50 in FalloutNV.esm) [hypothesis: the
    /// setting's name: who resists that much need not keep away].
    fn avoids_radiation(&self, game: &mut Game, actor: FormId) -> bool {
        let Some(def) = self.agents.get(&actor).and_then(|a| a.package).and_then(|p| game.package_def(&p)) else { return false };
        if !crate::social::allows(def, fallout::AVOID_RADIATION) {
            return false;
        }
        let level = game.world.forms.setting_f32("fRadiationResistanceAvoidLevel").unwrap_or(RAD_AVOID_LEVEL) as f64;
        fnv_game::functions::actor_value_capped(game, actor, fnv_game::actor_values::RAD_RESISTANCE) < level
    }

    /// Path parameters for `actor`: everything it carries may be a key [documented:
    /// locks name a key form].
    pub(crate) fn path_params(&self, game: &mut Game, actor: FormId) -> PathParams {
        let mut params = self.cfg.params.clone();
        // "Allow Swimming: Actors will not swim to reach destinations unless this flag is
        // set"; "Allow Falls: Actors will not jump off of an elevation that will cause
        // damage unless this flag is set" [documented: GECK wiki Packages, Flags tab].
        // Without Allow Swimming water costs [`NO_SWIM_WATER_COST`] times as much (a way
        // through water is still taken when there is no other) [design].
        if game.package_has(actor, flags::ALLOW_SWIMMING) {
            params.water_cost = params.water_cost.min(self.cfg.params.water_cost);
        } else {
            params.water_cost = params.water_cost.max(NO_SWIM_WATER_COST);
        }
        if game.package_has(actor, flags::ALLOW_FALLS) {
            params.ledges = true;
        } else if !params.ledges {
            // [hypothesis, #2532] An authored drop no higher than the damage threshold
            // is safe without Allow Falls; use the planned takeoff and landing. The
            // threshold/fallback follows nvf-play falls.rs (physics.md §6.10).
            params.safe_drop_max = Some(game.world.forms.setting_f32("fJumpFallHeightMin").unwrap_or(SAFE_DROP_HEIGHT).max(0.0));
        }
        // Avoid Radiation: the radiation markers near it cost a way through them more
        // (`avoids_radiation`).
        if let Some(a) = self.agents.get(&actor).filter(|_| self.avoids_radiation(game, actor)) {
            params.avoid = radiated_near(game, a.walker.space, a.walker.feet);
            params.avoid_cost = AVOID_RADIATION_COST;
        }
        // A creature without the Swims flag keeps off water triangles altogether
        // (`Game::keeps_out_of_water`).
        if game.keeps_out_of_water(actor) {
            params.water = false;
        }
        if let Ok(s) = game.world.get(actor) {
            params.keys.extend(s.inventory.iter().flat_map(|inv| inv.iter()).map(|(item, _)| item.0));
        }
        // Locked doors whose own owner it is (its base, or a faction it is in at the rank
        // asked for; not by the cell's ownership) are open to it as if it carried the key
        // [hypothesis: Fallout 3's baby stage has Dad
        // walk out of the playroom through CG01PlayroomDoor, locked Requires Key with no
        // key and owned by Dad's base (TTW's Fallout3.esm REFR 0602EA4A); without this his
        // package was out of reach and the scene stopped, owner's TTW run 2026-11-02].
        params.owned_doors.extend(self.locked_doors.iter().copied().filter(|&d| game.ownership(FormId(d)).is_some_and(|o| !o.from_cell && game.counts_as_owner(actor, &o))));
        // [design, #2716 / fnv_sim::World::initial] An untouched locked door uses its
        // default-open placement; a materialized reference's live state wins below.
        // Open locked doors do not refuse nav triangles [hypothesis,
        // docs/spec-notes/navmesh.md; observer check 206].
        params.open_doors.extend(self.locked_doors.iter().copied().filter(|&d| {
            game.world.peek(FormId(d)).is_none() && game.world.default_open(FormId(d))
        }));
        params.open_doors.extend(game.world.changed().filter(|(_, s)| s.open).map(|(id, _)| id.0));
        // A creature flagged "Can't Open Doors" [documented: GECK wiki
        // GetActorBaseFlagsHigh, Creature column] uses no teleport door [hypothesis: what
        // not opening doors means for a path; docs/spec-notes/packages.md §7.1].
        if game.cant_open_doors(actor) {
            params.opens_doors = false;
        }
        // Scripted doors that did not let it through lately (`fnv_game::npc_doors`).
        params.refused_doors = game.refused_doors(actor);
        params
    }

    /// Before `actor` reaches the next door of its way, the door's OnActivate block runs
    /// for it (`fnv_game::npc_doors`): one that refuses it is closed to its paths and it
    /// takes up its package afresh from where it stands, as when a door on its way is
    /// locked ([`Ai::apply_door_locks`]). `reach`: how near the door the actor is asked.
    /// Whether it may walk on.
    fn ask_door(&mut self, game: &mut Game, actor: FormId, reach: f32) -> bool {
        let a = self.agents.get_mut(&actor).expect("listed");
        let follower = a.walker.follower.as_ref();
        // The next door of its way, however far, and that door when it is within reach.
        let next = follower.and_then(|f| f.door_ahead(a.walker.feet, f32::INFINITY));
        let ahead = follower.and_then(|f| f.door_ahead(a.walker.feet, reach));
        // Asked every step while it comes up to the door, the extender hears it once a
        // crossing (`fnv_game::npc_doors`) [design, #2123]. The crossing ends where the
        // follower goes through the door ([`Ai::walk`], `Follower::passed`), and here when
        // it is called off: the way cleared or planned without that door next, or another
        // package taken up; not when a shorter step leaves the door just out of reach.
        if let Some((left, _)) = a.asked_door.filter(|&(d, p)| Some(d) != next || p != a.package) {
            a.asked_door = None;
            game.npc_door_left(actor, FormId(left));
        }
        let Some(door) = ahead else { return true };
        a.asked_door = Some((door, a.package));
        let answer = game.npc_door(actor, FormId(door));
        // [hypothesis, #2716] An already-open in-place door is no barrier, even with
        // a cached or fresh script refusal (navmesh.md; observer check 206). [design]
        // Ask first to preserve crossing events and use live state after the script.
        let in_place = game.world.placed.get(FormId(door)).is_some_and(|r| r.teleport.is_none());
        let open_in_place = in_place && game.world.get(FormId(door)).is_ok_and(|s| s.open);
        // [design, #2716] SetOpenState can close a door without changing its lock
        // (`Game::set_ref_open`). A path planned while open grants no permission to
        // reopen it: recheck the live lock, key and own ownership after activation,
        // with the same configured overrides as `path_params`. Cell ownership does
        // not authorize opening a locked door (`path_params`).
        let p = &self.cfg.params;
        let locked_out = in_place && !open_in_place && game.world.lock_of(FormId(door)).is_some_and(|lock| {
            lock.locked
                && !p.open_locked
                && !p.owned_doors.contains(&door)
                && !game.ownership(FormId(door)).is_some_and(|o| !o.from_cell && game.counts_as_owner(actor, &o))
                && !lock.key.is_some_and(|key| p.keys.contains(&key.0) || game.world.item_count(actor, key).is_ok_and(|n| n > 0))
        });
        if open_in_place || (answer != fnv_game::npc_doors::DoorAnswer::Refused && !locked_out) {
            return true;
        }
        a.walker.follower = None;
        a.walker.tri = fnv_nav::graph::NONE;
        a.walker.moving = false;
        a.activity = Activity::None;
        a.package = None;
        a.goal = None;
        a.sandbox = None;
        a.errand = None;
        a.following = None;
        a.waiting_at = None;
        self.log(game, actor, AiEvent::DoorRefused { door: FormId(door) });
        false
    }

    /// Combat, a warning or flight takes the sandbox's movement and object intent
    /// away [design]. The sandbox chooses afresh when its package next runs, without
    /// losing its schedule or treating the interrupted object as unreachable.
    pub(crate) fn interrupt_sandbox(&mut self, game: &Game, actor: FormId) {
        let a = self.agents.get_mut(&actor).expect("listed");
        if !a.sandbox.as_mut().is_some_and(Sandbox::interrupt) {
            return;
        }
        a.walker.follower = None;
        a.activity = Activity::There;
        a.face_to = None;
        self.set_using(game, actor, None);
        // A pending choice or a Look-held seat can reserve furniture without using
        // it. set_using(None) alone does not release those reservations.
        self.taken.retain(|_, user| *user != actor);
    }

    /// A sandboxing actor whose behaviour is over chooses the next one and sets off.
    pub(crate) fn sandbox_step(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) {
        let a = self.agents.get_mut(&actor).expect("listed");
        let Some(sb) = a.sandbox.as_mut() else { return };
        if sb.pending.is_some() || !sb.due(game) {
            return;
        }
        let at = Place { space: a.walker.space, pos: a.walker.feet };
        let choice = sb.next(game, nav, &self.taken, actor, at, &mut self.furniture);
        // A guard's small area reaches its spots closer than the usual arrival distance.
        let reach = sb.reach();
        // Up from what it was on, unless it stays on the same thing; the furniture it
        // is going to is kept for it from now.
        if a.using != choice.doing.furniture() {
            sb.got_up = a.using.is_some();
            self.set_using(game, actor, None);
        }
        self.taken.retain(|_, user| *user != actor);
        if let Some(f) = choice.doing.furniture() {
            self.taken.insert(f, actor);
        }
        let near = choice.target.is_none_or(|to| match reach {
            Some(r) => to.space == at.space && hdist(at.pos, to.pos) <= r && (at.pos[2] - to.pos[2]).abs() <= 256.0,
            None => self.within(at, to, 0.0),
        });
        if near {
            self.sandbox_begin(game, actor, choice, 0.0);
            return;
        }
        let params = self.path_params(game, actor);
        let to = choice.target.expect("checked");
        // A sandbox with Always Run runs between its spots, as the package's other walks
        // do (`Ai::plan`): Freeside's chased rat (VFSChasedRatEscapingPackage, Sandbox,
        // Always Run) walked at 56 units/s with the child running after it (#4487).
        let running = self.agents[&actor].package.and_then(|p| game.package_def(&p)).is_some_and(|d| d.has(flags::ALWAYS_RUN));
        let speed = if running { self.pace(game, actor, true) } else { self.calm_speeds(actor).walk };
        let a = self.agents.get_mut(&actor).expect("listed");
        // A package's walk: [`Ai::repace_walk`] gives it the drawn walk; a run is not one.
        (a.walker.speed, a.walk_pace) = (speed, (!running).then_some((1.0, speed)));
        match a.walker.go_to(nav, to, &params) {
            Ok(path) => {
                let walk = path.length;
                a.activity = Activity::Travelling;
                a.trip_started = game.time;
                a.trip_from = a.walker.travelled;
                let doing = choice.doing;
                a.sandbox.as_mut().expect("checked").pending = Some(choice);
                self.log(game, actor, AiEvent::Sandbox { doing, walk });
            }
            Err(_) => {
                a.sandbox.as_mut().expect("checked").failed(game, &choice);
                self.taken.retain(|_, user| *user != actor);
            }
        }
    }

    /// The behaviour a sandboxing actor chose begins (it is at the place).
    fn sandbox_begin(&mut self, game: &mut Game, actor: FormId, choice: crate::sandbox::Choice, walked: f32) {
        let a = self.agents.get_mut(&actor).expect("listed");
        a.activity = Activity::There;
        a.walker.follower = None;
        a.face_to = choice.heading;
        if let Some(sb) = a.sandbox.as_mut() {
            sb.begin(game, &choice);
        }
        if walked == 0.0 {
            self.log(game, actor, AiEvent::Sandbox { doing: choice.doing, walk: 0.0 });
        }
        self.set_using(game, actor, choice.doing.furniture());
    }

    /// The package is new: work out its goal and set off.
    fn begin(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, package: FormId) {
        let Some(def) = game.package_def(&package) else { return };
        let goal = goal::resolve(game, nav, actor, def);
        // A Use Weapon package's weapon use, unless a script's comes first.
        if def.ty == PackageType::UseWeapon && game.weapon_use(actor).is_none_or(|w| w.source != Source::Script) {
            match game.package_weapon_use(actor, package) {
                Some(w) => game.give_weapon_use(actor, w),
                None => game.drop_weapon_use(actor),
            }
        }
        let feet = Place { space: self.agents[&actor].walker.space, pos: self.agents[&actor].walker.feet };
        // The goal is kept first: settling reads it (a sandbox's centre and radius).
        self.agents.get_mut(&actor).expect("listed").goal = Some(goal.clone());
        let there = |a: &mut Ai, game: &mut Game| {
            a.agents.get_mut(&actor).expect("listed").activity = Activity::There;
            a.log(game, actor, AiEvent::AlreadyThere { package });
            reached(game, actor);
            a.settle(game, actor);
        };
        match &goal {
            Goal::Here => there(self, game),
            Goal::Near { place, radius, .. } => {
                if self.within(feet, *place, *radius) {
                    there(self, game)
                } else {
                    self.plan(game, nav, actor, package, *place, None)
                }
            }
            Goal::Inside { space, at } => {
                if feet.space == *space {
                    there(self, game)
                } else {
                    self.plan(game, nav, actor, package, *at, None)
                }
            }
            Goal::Route(route) => {
                // A repeatable patrol starts at the nearest point, another at the first
                // [documented: GECK wiki, Patrol Package].
                // After a load, where the save had it ([`crate::progress`]).
                let start = match self.restored_patrol(actor, package, route.points.len()) {
                    Some(i) => {
                        self.log(game, actor, AiEvent::Resumed { package, patrol_point: Some(route.points[i].0), sandbox: false });
                        i
                    }
                    None if route.repeatable => nearest_point(route, feet),
                    None => 0,
                };
                self.agents.get_mut(&actor).expect("listed").route_at = start;
                let (point, place) = route.points[start];
                self.plan(game, nav, actor, package, place, Some(point));
            }
            Goal::Talk(talk) => match self.way_to(feet, &talk.wait) {
                Some(to) => {
                    self.agents.get_mut(&actor).expect("listed").talk = Some(Talking::ToWait);
                    self.plan(game, nav, actor, package, to, None);
                }
                None => {
                    let a = self.agents.get_mut(&actor).expect("listed");
                    a.talk = Some(Talking::Waiting);
                    a.activity = Activity::There;
                    self.settle(game, actor);
                }
            },
            // `follow_step` sets off after the target when it is away.
            Goal::Follow(_) => {
                self.agents.get_mut(&actor).expect("listed").activity = Activity::There;
                game.package_goal_met(actor);
            }
            Goal::Escort(_) | Goal::Find(_) | Goal::UseAt(_) | Goal::Ambush(_) | Goal::Flee(_) => self.errand_begin(game, nav, actor, package, &goal),
            Goal::NotCarriedOut(why) => {
                self.agents.get_mut(&actor).expect("listed").activity = Activity::Staying;
                self.log(game, actor, AiEvent::NotCarriedOut { package, why });
            }
            Goal::Missing(why) => self.unreachable(game, actor, package, why.to_string()),
        }
    }

    /// A restrained actor's new package (it does not move): when the place the package
    /// sends it to is within [`RESTRAINED_REACH`], it is there, and uses the furniture
    /// the package gives it (`settle`): Deputy Beagle's Travel package to the captive
    /// marker (FFCaptiveMarker, furniture "Restrained Sit") he stands 84 units from in
    /// the Bison Steve hotel, his OnLoad block having restrained him [observed: the
    /// records, PrimmDeputyScript; owner's playtest list 7 item 30, he stood beside the
    /// marker]. Farther, it stays where it stands.
    fn begin_in_place(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, package: FormId) {
        let Some(def) = game.package_def(&package) else { return };
        let goal = goal::resolve(game, nav, actor, def);
        let feet = Place { space: self.agents[&actor].walker.space, pos: self.agents[&actor].walker.feet };
        let at = |place: &Goal| match place {
            Goal::Here => true,
            Goal::Near { place, radius, .. } => place.space == feet.space && hdist(feet.pos, place.pos) <= radius.max(RESTRAINED_REACH) && (feet.pos[2] - place.pos[2]).abs() <= 256.0,
            Goal::Inside { space, .. } => *space == feet.space,
            _ => false,
        };
        // A Dialogue package: the actor waits where it stands, on its wait location's
        // furniture when that is within reach, and talks from there (`step_actor_inner`)
        // [documented: GECK wiki SetRestrained, restrained actors still "go into
        // dialogue"; hypothesis: a wait location out of reach is not waited for].
        if let Goal::Talk(talk) = &goal {
            let settle = at(&talk.wait);
            let a = self.agents.get_mut(&actor).expect("listed");
            (a.talk, a.activity) = (Some(Talking::Waiting), Activity::There);
            a.goal = Some(goal);
            if settle {
                self.settle(game, actor);
            }
            return;
        }
        let near = at(&goal);
        self.agents.get_mut(&actor).expect("listed").goal = Some(goal);
        if near {
            self.agents.get_mut(&actor).expect("listed").activity = Activity::There;
            self.log(game, actor, AiEvent::AlreadyThere { package });
            reached(game, actor);
            self.settle(game, actor);
        } else {
            self.agents.get_mut(&actor).expect("listed").activity = Activity::Staying;
            self.log(game, actor, AiEvent::NotCarriedOut { package, why: "restrained: it does not move to the package's place" });
        }
    }

    /// Where an actor standing at `feet` has to go to be at a place to stay at
    /// ([`Goal::Near`], [`Goal::Inside`]); `None`: it is there.
    pub(crate) fn way_to(&self, feet: Place, stay: &Goal) -> Option<Place> {
        match stay {
            Goal::Near { place, radius, .. } => (!self.within(feet, *place, *radius)).then_some(*place),
            Goal::Inside { space, at } => (feet.space != *space).then_some(*at),
            _ => None,
        }
    }

    fn within(&self, feet: Place, place: Place, radius: f32) -> bool {
        feet.space == place.space && hdist(feet.pos, place.pos) <= radius.max(self.cfg.arrive) && (feet.pos[2] - place.pos[2]).abs() <= 256.0
    }

    /// The pace of a package's walk: the walking or running speed, the walk times
    /// [`drawn_walk_share`] (a person walking with a pistol drawn outside combat, #2124),
    /// times fMoveSneakMult (0.57 in FalloutNV.esm [observed]) while the actor sneaks
    /// (`Game::sneaks`: SetForceSneak, the package's Always Sneak) [hypothesis: the
    /// setting's name; the player's sneaking pace uses it too].
    pub(crate) fn pace(&self, game: &Game, actor: FormId, running: bool) -> f32 {
        let own = self.calm_speeds(actor);
        let speed = if running { own.run } else { own.walk * drawn_walk_share(game, actor) };
        speed * sneak_share(game, actor)
    }

    /// A package's walk under way keeps up with the weapon being drawn or put away
    /// ([`drawn_walk_share`]): its speed is scaled from the share last applied to the share
    /// now ([`Agent::walk_pace`]), whatever its base (new speeds on the way, an armour or
    /// Speed Mult change, come through [`Ai::set_speeds`], which keeps the share and the
    /// walk's ownership). The walk is owned from where
    /// a package's walk is planned (`Ai::plan`, `Ai::approach`, a sandbox's or a Find
    /// errand's walk) until it ends (`Ai::arrived`, or the actor is not travelling); every
    /// other setter of the walker's speed gives it up, as does another owner of the walk
    /// (a random conversation, a search, a follow) or a speed it did not set. Such a
    /// walk is left as it is [design, #2124: the animating side tells the weapon out only
    /// once it is drawn, after the walk was planned].
    fn repace_walk(&mut self, game: &Game, actor: FormId) {
        let a = &self.agents[&actor];
        let Some((applied, set)) = a.walk_pace else { return };
        // Another owner of the walk (a random conversation's, a search's, a Follow
        // package's, a Back Up, a walk-up) gives it up, whatever speed it has.
        let other = a.chat.is_some() || a.search.is_some() || a.following.is_some() || a.backing.is_some() || a.walk_up.is_some();
        if other || a.walker.speed != set {
            self.agents.get_mut(&actor).expect("listed").walk_pace = None;
            return;
        }
        let want = drawn_walk_share(game, actor);
        if want != applied && applied > 0.0 {
            let speed = set / applied * want;
            let a = self.agents.get_mut(&actor).expect("listed");
            (a.walker.speed, a.walk_pace) = (speed, Some((want, speed)));
            // Standing at speeds of 0: the gait it goes on in when they come back takes
            // the new share too (`Ai::set_speeds`).
            if speed <= 0.0 {
                if let Some(stall) = self.stalled.get_mut(&actor) {
                    stall.share = stall.share / applied * want;
                }
            }
        }
    }

    /// What [`Agent::walk_pace`] holds for a walk planned at `speed` by [`Ai::pace`]: the
    /// share applied for a walk, nothing for a run.
    pub(crate) fn walk_pace_of(game: &Game, actor: FormId, running: bool, speed: f32) -> Option<(f32, f32)> {
        (!running).then(|| (drawn_walk_share(game, actor), speed))
    }

    /// Plan a walk to `to`; without a way there the package is set aside.
    pub(crate) fn plan(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, package: FormId, to: Place, patrol_point: Option<FormId>) {
        // Pretend In Combat: it goes as in a fight, running [our choice; documented: the
        // actor "move[s] from cover point to cover point", Combat Package Flags, which
        // is not built].
        let running = game.package_def(&package).is_some_and(|d| d.has(flags::ALWAYS_RUN) || d.has(flags::PRETEND_IN_COMBAT));
        let params = self.path_params(game, actor);
        let speed = self.pace(game, actor, running);
        let own = Ai::walk_pace_of(game, actor, running, speed);
        let a = self.agents.get_mut(&actor).expect("listed");
        (a.walker.speed, a.walk_pace) = (speed, own);
        let from = Place { space: a.walker.space, pos: a.walker.feet };
        match a.walker.go_to(nav, to, &params) {
            Ok(path) => {
                let doors = path.legs.iter().filter_map(|l| l.door.map(|d| FormId(d.door))).collect();
                let event = AiEvent::Trip { package, to: to.pos, space: to.space, length: path.length, doors, running, patrol_point };
                // A walk planned at speeds of 0 keeps its gait for when they come back; an
                // earlier walk's stop is not this one's (issue #2118).
                let to = a.walker.trip.as_ref().map(|t| t.to);
                if speed > 0.0 {
                    self.stalled.remove(&actor);
                } else {
                    // A walk's drawn-weapon share (#2124) is kept with the sneak's.
                    let share = sneak_share(game, actor) * own.map_or(1.0, |(drawn, _)| drawn);
                    self.stalled.insert(actor, Stall { gait: usize::from(running), share, to });
                }
                a.activity = Activity::Travelling;
                a.trip_started = game.time;
                a.trip_from = a.walker.travelled;
                self.log(game, actor, event);
            }
            Err(e) => {
                a.talk = None;
                self.no_way(game, nav, actor, package, from, to, patrol_point, e, &params)
            }
        }
    }

    /// No way from `from` to `to` for a package's walk ([`Ai::plan`]); packages.md §11:
    /// - the way needs a load door and the creature cannot open doors: it stays
    ///   ([`AiEvent::DoorsClosedToIt`]);
    /// - out of sight (out of the high process: no body is drawn for it), going to its
    ///   package's one place (a Travel's, a Sleep's, a Sandbox's centre; not a patrol
    ///   point, not a walk of its own), with no door at all between the two places,
    ///   locked or disabled: it is put there and has arrived [hypothesis: the data moves
    ///   such actors only by scripts or not at all, Gomez in the NCR safehouse's side
    ///   room, Malcolm Holmes back to his holding cell; observer check 145];
    /// - otherwise the package is set aside ([`Ai::unreachable`]).
    #[allow(clippy::too_many_arguments)]
    fn no_way(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, package: FormId, from: Place, to: Place, patrol_point: Option<FormId>, e: PathError, params: &PathParams) {
        if !matches!(e, PathError::Unreachable { .. }) {
            let (why, _) = why_unreachable(game, nav, from, to, params, &e);
            return self.unreachable(game, actor, package, why);
        }
        if !params.opens_doors && nav.can_reach(from, to, &PathParams { opens_doors: true, ..params.clone() }) {
            self.log(game, actor, AiEvent::DoorsClosedToIt { package });
            return self.set_aside(game, actor, package);
        }
        // Only the walk to the package's own place: not a patrol's, a sandbox's own walk
        // to its furniture, a wait location's.
        let own_place = match &self.agents[&actor].goal {
            Some(Goal::Near { place, .. }) => *place == to,
            Some(Goal::Inside { at, .. }) => *at == to,
            _ => false,
        };
        if own_place && patrol_point.is_none() && !self.is_high(actor) && nav.joined_by_doors(from, to) == Some(false) {
            return self.put_there(game, nav, actor, package, from, to);
        }
        self.unreachable_why(game, nav, actor, package, from, to, params, &e)
    }

    /// [`Ai::no_way`] for a walk after someone (a Follow, the way up to whom it talks to):
    /// only the creature that cannot open doors is told apart.
    #[allow(clippy::too_many_arguments)]
    fn no_way_after(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, package: FormId, from: Place, to: Place, e: PathError) {
        let params = self.path_params(game, actor);
        if matches!(e, PathError::Unreachable { .. }) && !params.opens_doors && nav.can_reach(from, to, &PathParams { opens_doors: true, ..params.clone() }) {
            self.log(game, actor, AiEvent::DoorsClosedToIt { package });
            return self.set_aside(game, actor, package);
        }
        self.unreachable_why(game, nav, actor, package, from, to, &params, &e)
    }

    /// [`Ai::unreachable`] with [`why_unreachable`]'s reasons; the closed doors that open
    /// in place among them are kept for [`Ai::apply_doors_opened`].
    #[allow(clippy::too_many_arguments)]
    fn unreachable_why(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, package: FormId, from: Place, to: Place, params: &PathParams, e: &PathError) {
        let (why, doors) = why_unreachable(game, nav, from, to, params, e);
        for d in doors.into_iter().filter(|d| nav.in_place.contains_key(d)) {
            if !self.closing_doors.contains(&d) {
                self.closing_doors.push(d);
            }
        }
        self.unreachable(game, actor, package, why)
    }

    /// A door that opens in place that closed a package's way (named by an unreachable
    /// report, [`Ai::unreachable_why`]) is open now, by a script (SetOpenState, its
    /// OnActivate block), the player or another actor: the packages set aside as out of
    /// reach may be chosen again (`Game::unblock_packages`) and their actors take them up
    /// afresh, as for a door unlocked ([`Ai::apply_door_locks`]) [design, #2405: Sally on
    /// Mothership Zeta, refused by a force field's script that opened it the same moment,
    /// stood for a game hour with her package set aside]. Returns those actors, `None`
    /// when none of those doors opened.
    pub fn apply_doors_opened(&mut self, game: &mut Game) -> Option<Vec<FormId>> {
        if self.closing_doors.is_empty() {
            return None;
        }
        let open = |d: u32| game.world.peek(FormId(d)).is_some_and(|s| s.open);
        let (opened, still): (Vec<u32>, Vec<u32>) = self.closing_doors.iter().partition(|&&d| open(d));
        if opened.is_empty() {
            return None;
        }
        self.closing_doors = still;
        let again = game.unblock_packages();
        for actor in &again {
            if let Some(a) = self.agents.get_mut(actor) {
                a.walker.follower = None;
                a.walker.tri = fnv_nav::graph::NONE;
                a.activity = Activity::None;
                a.package = None;
                a.goal = None;
                a.sandbox = None;
                a.errand = None;
                a.following = None;
            }
        }
        Some(again)
    }

    /// Out of sight, `actor` is put at `to`, the place of its package, and arrives there
    /// ([`Ai::no_way`]).
    fn put_there(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, package: FormId, from: Place, to: Place) {
        self.log(game, actor, AiEvent::PutThere { package, from, to });
        let a = self.agents.get_mut(&actor).expect("listed");
        let w = &mut a.walker;
        (w.space, w.feet) = (to.space, to.pos);
        w.tri = nav.nearest(to.space, to.pos, 64.0).map_or(fnv_nav::graph::NONE, |(t, _)| t);
        (w.turning, w.moving, w.follower) = (false, false, None);
        let heading = w.heading;
        a.trip_started = game.time;
        a.trip_from = a.walker.travelled;
        let cell = goal::cell_of_place(game.world.placed, &to);
        let _ = game.set_actor_place(actor, cell, to.pos, heading);
        self.arrived(game, nav, actor);
    }

    /// A script's StartConversation with the player from afar (`Game::walk_up_greet`):
    /// the actor runs to the player, planning the way again as the player moves, and
    /// opens the conversation once within [`goal::STANDARD_TALK_DISTANCE`]; with no way
    /// there, or once it gets no nearer, it opens it where it is [design]. Meanwhile its
    /// package waits, and it takes it up again after. Returns whether it is walking up.
    fn walk_up_step(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) -> bool {
        if game.walk_up_pending(actor).is_none() {
            if self.agents.get_mut(&actor).expect("listed").walk_up.take().is_some() {
                self.after_walk_up(game, nav, actor);
            }
            return false;
        }
        let a = &self.agents[&actor];
        let feet = Place { space: a.walker.space, pos: a.walker.feet };
        let travelling = a.activity == Activity::Travelling;
        let planned = a.walk_up;
        let Some(player) = goal::place_of_ref(game, FormId::PLAYER_REF) else {
            game.walk_up_arrived(actor);
            return false;
        };
        let away = if feet.space == player.space { goal::dist(feet.pos, player.pos) } else { f32::INFINITY };
        let moved = planned.is_some_and(|(to, _)| to.space != player.space || goal::dist(to.pos, player.pos) > REPLAN_DISTANCE);
        let near = away <= goal::STANDARD_TALK_DISTANCE.max(self.cfg.arrive);
        // Near enough, or at the end of its way with the player still where it led.
        if near || (planned.is_some() && !travelling && !moved) {
            let a = self.agents.get_mut(&actor).expect("listed");
            (a.walker.follower, a.activity, a.walk_up) = (None, Activity::There, None);
            let line = game.walk_up_arrived(actor);
            self.log(game, actor, AiEvent::WalkedUp { away, reached: near, line });
            self.after_walk_up(game, nav, actor);
            return true;
        }
        if planned.is_some_and(|(_, at)| !moved || game.time < at) {
            return true;
        }
        self.set_using(game, actor, None);
        let params = self.path_params(game, actor);
        let speed = self.pace(game, actor, true);
        let a = self.agents.get_mut(&actor).expect("listed");
        (a.search, a.sandbox, a.talk, a.face_to, a.following) = (None, None, None, None, None);
        (a.walker.speed, a.walk_pace) = (speed, None);
        match a.walker.go_to(nav, player, &params).map(|p| p.length) {
            Ok(length) => {
                if a.activity != Activity::Travelling {
                    a.trip_started = game.time;
                    a.trip_from = a.walker.travelled;
                }
                a.activity = Activity::Travelling;
                a.walk_up = Some((player, game.time + REPLAN_SECONDS));
                if planned.is_none() {
                    let away = (feet.space == player.space).then_some(away);
                    self.log(game, actor, AiEvent::WalkUp { away, length });
                }
            }
            Err(_) => {
                (a.walker.follower, a.activity, a.walk_up) = (None, Activity::There, None);
                let line = game.walk_up_arrived(actor);
                self.log(game, actor, AiEvent::WalkedUp { away, reached: false, line });
                self.after_walk_up(game, nav, actor);
            }
        }
        true
    }

    /// The walk-up is over: the actor's package goes on.
    fn after_walk_up(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) {
        if let Some(p) = self.agents[&actor].package {
            self.begin(game, nav, actor, p);
        }
    }

    /// A script's weapon use (`fnv_game::use_weapon`): a new one sends the actor to
    /// where it is to stand (it stays where it is when it is there already, is to stand
    /// on its own reference, or has no way there); one that is over sends it back to its
    /// package. In place, a weapon use (a script's, or its package's while it is not in
    /// combat or the package continues during combat) turns it to its target at the
    /// standing turn rate; one the fight took over (`WeaponUse::taken`) turns it, standing,
    /// to its combat target at the combat turn rate instead. Returns whether a
    /// script's use holds the actor (its package's own doings and its fight wait).
    fn weapon_step(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, dt: f32) -> bool {
        let script = game.weapon_use(actor).filter(|w| w.source == Source::Script).map(|w| (w.seq, w.stand, w.target));
        let order = self.agents[&actor].weapon_order;
        match (script, order) {
            (None, None) => {}
            (None, Some(_)) => {
                let a = self.agents.get_mut(&actor).expect("listed");
                a.weapon_order = None;
                a.walker.follower = None;
                (a.activity, a.goal) = (Activity::None, None);
                // A melee use's closing in ends with it.
                if matches!(a.search, Some(Search::Closing { .. } | Search::Holding { .. })) {
                    a.search = None;
                }
                self.log(game, actor, AiEvent::WeaponUseOver);
                if let Some(p) = self.agents[&actor].package {
                    self.begin(game, nav, actor, p);
                }
                return false;
            }
            (Some((seq, stand, target)), order) if order != Some(seq) => {
                self.set_using(game, actor, None);
                let params = self.path_params(game, actor);
                let speed = self.pace(game, actor, false);
                let arrive = self.cfg.arrive;
                let a = self.agents.get_mut(&actor).expect("listed");
                (a.weapon_order, a.search, a.sandbox, a.talk, a.face_to, a.following) = (Some(seq), None, None, None, None, None);
                let feet = Place { space: a.walker.space, pos: a.walker.feet };
                let to = stand.and_then(|s| goal::place_of_ref(game, s)).filter(|t| t.space != feet.space || hdist(t.pos, feet.pos) > arrive);
                (a.walker.speed, a.walk_pace) = (speed, None);
                let way = to.map(|t| a.walker.go_to(nav, t, &params).map(|p| p.length));
                let length = match way {
                    Some(Ok(length)) => {
                        a.activity = Activity::Travelling;
                        a.trip_started = game.time;
                        a.trip_from = a.walker.travelled;
                        length
                    }
                    // There already, or no way there: it shoots from where it is [design].
                    _ => {
                        a.walker.follower = None;
                        a.activity = Activity::There;
                        game.weapon_use_in_place(actor);
                        0.0
                    }
                };
                self.log(game, actor, AiEvent::WeaponUse { target, length });
            }
            _ => {}
        }
        // A script's melee use in place closes in on its target within its reach, as a
        // fighter does (`crate::alert`): it stops within the first distance and sets off
        // again beyond the second (docs/design/use-weapon.md §3.1).
        let melee = game.active_weapon_use(actor).filter(|w| w.in_place && w.source == Source::Script).and_then(|w| w.reach.map(|r| (w.target, r)));
        if let Some((target, [stop, resume])) = melee {
            let a = &self.agents[&actor];
            if let Some(at) = goal::place_of_ref(game, target).filter(|p| p.space == a.walker.space).map(|p| p.pos) {
                let away = hdist(a.walker.feet, at);
                let holding = a.search == Some(Search::Holding { target });
                let going = a.activity == Activity::Travelling;
                let close = away > if holding { resume } else { stop };
                match (close, a.search) {
                    (true, Some(Search::Closing { target: t, to, since, .. })) if t == target && going && crate::alert::keeps_way(to, since, at, away, stop, game.time) => {}
                    (true, _) => self.close_in(game, nav, actor, target, at, away, stop),
                    (false, Some(Search::Holding { target: t })) if t == target => {}
                    (false, _) => {
                        let a = self.agents.get_mut(&actor).expect("listed");
                        a.walker.follower = None;
                        a.activity = Activity::There;
                        a.search = Some(Search::Holding { target });
                        self.log(game, actor, AiEvent::Hold { target, away });
                    }
                }
            }
        }
        let a = &self.agents[&actor];
        let space = a.walker.space;
        let standing = a.activity != Activity::Travelling;
        // A use the fight took over (`WeaponUse::taken`: in Combat, nothing for the
        // engine's shooter to act on) turns it to its enemy as a standing fighter turns
        // (`crate::alert`), people and creatures alike: its package's doings go on in
        // combat, so the combat steering that would turn it does not run (#2145 review)
        // [design].
        // Read only in Combat: one left set as the actor leaves it, before the fight's next
        // frame clears it, means nothing.
        let taken = game.active_weapon_use(actor).is_some_and(|w| w.taken) && game.awareness.state(actor) == fnv_game::awareness::State::Combat;
        if taken {
            let enemy = game.awareness.get(actor).and_then(|aw| aw.combat_target()).map(|(_, t)| t.at);
            if let Some(at) = enemy.filter(|_| standing) {
                self.combat_face(game, actor, Place { space, pos: at }, dt);
            }
        } else if let Some(t) = game.active_weapon_use(actor).filter(|w| w.in_place && standing).map(|w| w.target) {
            if let Some(at) = goal::weapon_target_place(game, t, space) {
                self.face(game, actor, at, dt);
            }
        }
        script.is_some()
    }

    fn unreachable(&mut self, game: &mut Game, actor: FormId, package: FormId, why: String) {
        self.log(game, actor, AiEvent::Unreachable { package, why });
        self.set_aside(game, actor, package);
    }

    /// The package cannot be carried out from here: the actor stays and chooses again,
    /// the package blocked for a while.
    fn set_aside(&mut self, game: &mut Game, actor: FormId, package: FormId) {
        let a = self.agents.get_mut(&actor).expect("listed");
        a.activity = Activity::Staying;
        a.walker.follower = None;
        // The actor chooses again; `update` sees the new package at its next call.
        game.block_package(actor, package, self.cfg.retry_hours);
    }

    /// An actor whose AI is off (SetActorsAI 0, ToggleActorsAI): one that was walking
    /// keeps walking straight on along its heading at its speed, its package, path and
    /// heading left as they were [observed: owner-decision-captures.md §7, od_05], moved by
    /// the caller's character movement ([`Mover::coast`]); where there is none it stands
    /// [hypothesis]. When the AI comes back on, its walk steers from where it got to back
    /// to its path's next point (`fnv_nav::Follower::steer`) [observed: the settler turned
    /// and walked back].
    fn coast(&mut self, game: &mut Game, actor: FormId, dt: f32, mover: &mut dyn Mover) {
        let a = self.agents.get_mut(&actor).expect("listed");
        if a.activity != Activity::Travelling || !a.walker.moving {
            return;
        }
        if !mover.coast(actor, &mut a.walker, dt) {
            a.walker.moving = false;
            return;
        }
        let feet = Place { space: a.walker.space, pos: a.walker.feet };
        let heading = a.walker.heading;
        let cell = goal::cell_of_place(game.world.placed, &feet);
        let _ = game.set_actor_place(actor, cell, feet.pos, heading);
    }

    fn walk(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, dt: f32, mover: &mut dyn Mover) {
        // Within this step's reach of the next door, and a margin [design].
        let reach = self.agents[&actor].walker.moving_speed() * dt + DOOR_ASK_MARGIN;
        if !self.ask_door(game, actor, reach) {
            return;
        }
        let a = self.agents.get_mut(&actor).expect("listed");
        a.face_to = None;
        let waivers = a.walker.height_waivers;
        let ev = match mover.walk(actor, &mut a.walker, nav, dt) {
            Some(ev) => ev,
            None => a.walker.update_with(nav, dt, &mut |door| mover.open(actor, door)),
        };
        // The door asked last gone through on this step (`Walker::passed` for the walker's
        // own steps, `Follower::passed` for a mover's): its crossing is over here, before
        // an arrival can plan the way back through it (#2123).
        let mut passed = std::mem::take(&mut a.walker.passed);
        if let Some(f) = a.walker.follower.as_mut() {
            passed.append(&mut f.passed);
        }
        if let Some((left, _)) = a.asked_door.filter(|(d, _)| passed.contains(d)) {
            a.asked_door = None;
            game.npc_door_left(actor, FormId(left));
        }
        let waived = a.walker.height_waivers != waivers;
        a.waiting_at = match ev {
            WalkEvent::WaitingForDoor(d) => Some(d),
            _ => None,
        };
        let feet = Place { space: a.walker.space, pos: a.walker.feet };
        let heading = a.walker.heading;
        let cell = goal::cell_of_place(game.world.placed, &feet);
        // An actor that cannot be put there (a reference the world does not know) is
        // left where the game has it.
        let _ = game.set_actor_place(actor, cell, feet.pos, heading);
        if waived {
            self.log(game, actor, AiEvent::HeightWaived { at: feet.pos });
        }
        if let WalkEvent::UsedDoor(d) = ev {
            self.log(game, actor, AiEvent::Door { door: FormId(d.door), to_door: FormId(d.to_door) });
        }
        // Reached: the end of the path, or already inside the place's radius or cell.
        let a = &self.agents[&actor];
        let own_walk = a.sandbox.as_ref().is_some_and(|s| s.pending.is_some());
        let reached = match (&a.goal, ev) {
            (_, WalkEvent::Arrived) => true,
            (_, WalkEvent::Idle) => a.walker.follower.is_none(),
            // A sandbox's own walks, searches and walks up to a conversation partner end
            // where they lead, not at the package's place (a starter inside its sandbox's
            // radius had "arrived" in the walk's first frame and talked from where it
            // stood, docs/spec-notes/npc-behaviour-compare.md §8).
            _ if own_walk || a.search.is_some() || a.chat.is_some() => false,
            (Some(Goal::Near { place, radius, .. }), _) => *radius > self.cfg.arrive && self.within(feet, *place, *radius),
            (Some(Goal::Inside { space, .. }), _) => feet.space == *space,
            _ => false,
        };
        if reached {
            self.arrived(game, nav, actor);
        }
    }

    pub(crate) fn arrived(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) {
        let a = self.agents.get_mut(&actor).expect("listed");
        // The walk is over: what comes next owns its own speed.
        (a.walker.follower, a.walk_pace) = (None, None);
        // The end of the way to a conversation partner: `crate::social` starts the
        // conversation.
        if a.chat.is_some() {
            a.activity = Activity::There;
            return;
        }
        // The end of a search walk: `crate::alert` has it look about.
        if a.search.is_some() {
            a.activity = Activity::There;
            return;
        }
        // At the place of a script's weapon use: it shoots from there (`weapon_step`).
        if a.weapon_order.is_some() && game.weapon_use(actor).is_some_and(|w| w.source == Source::Script) {
            a.activity = Activity::There;
            game.weapon_use_in_place(actor);
            return;
        }
        // The end of the way to the one it follows: it stands until the target is away
        // again (`follow_step`).
        if matches!(a.goal, Some(Goal::Follow(_))) {
            a.activity = Activity::There;
            a.following = None;
            return;
        }
        let Some(package) = a.package else {
            // Back where it was placed (`back_home`): it stands there, facing as placed.
            a.activity = Activity::There;
            a.face_to = game.world.placed.get(actor).map(|p| p.rot[2]);
            return;
        };
        let (seconds, travelled) = (game.time - a.trip_started, a.walker.travelled - a.trip_from);
        // The end of a sandbox's own walk: the behaviour it chose begins.
        if let Some(choice) = a.sandbox.as_mut().and_then(|s| s.pending.take()) {
            self.sandbox_begin(game, actor, choice, travelled.max(1.0));
            return;
        }
        // A Dialogue package: at the wait location the wait begins (on it, when it is
        // furniture: `settle`); at the end of the way to its target `talk_step` looks at
        // where the target is now.
        if let Some(phase) = a.talk {
            a.activity = Activity::There;
            if phase == Talking::ToWait {
                a.talk = Some(Talking::Waiting);
                self.log(game, actor, AiEvent::Arrived { package, seconds, travelled, patrol_point: None });
                self.settle(game, actor);
            }
            return;
        }
        if a.errand.is_some() {
            self.errand_arrived(game, nav, actor, package);
            return;
        }
        let route = match &a.goal {
            Some(Goal::Route(r)) => Some((r.clone(), a.route_at)),
            _ => None,
        };
        let point = route.as_ref().map(|(r, i)| r.points[*i].0);
        self.log(game, actor, AiEvent::Arrived { package, seconds, travelled, patrol_point: point });
        if let Some(point) = point {
            // Its result script and topic first (`Game::patrol_point_reached`).
            game.patrol_point_reached(actor, point);
            // The point's idle time is waited out before the next one [documented: GECK
            // wiki, Idle Markers "Patrol Data: Idle Time"].
            let idle = game.world.placed.get(point).and_then(|p| p.patrol_idle).filter(|s| *s > 0.0);
            if let Some(seconds) = idle {
                let a = self.agents.get_mut(&actor).expect("listed");
                a.activity = Activity::There;
                a.wait_until = Some(game.time + seconds as f64);
                self.log(game, actor, AiEvent::PatrolWait { patrol_point: point, seconds });
                // A patrol point that is furniture is used for the idle time
                // [observed: Gomorrah's courtesans (vGOMVarProstitutes, the template's
                // DefaultPatrolCasual to "Near linked reference") are linked to
                // DanceMarker03 references carrying 800 s of patrol idle time (XPRD);
                // owner's playtest list 7 item 30: they did not use their markers;
                // hypothesis: the original uses the furniture at the point, as the
                // dance idles conditioned on it (furniture.md §3) say].
                let furniture = game.world.placed.get(point).is_some_and(|p| game.data.furniture.contains_key(&p.base));
                if furniture && self.taken.get(&point).is_none_or(|u| *u == actor) && self.furniture.fits_assigned(game, actor, point) {
                    self.set_using(game, actor, Some(point));
                }
                return;
            }
            self.patrol_on(game, nav, actor);
            return;
        }
        self.agents.get_mut(&actor).expect("listed").activity = Activity::There;
        reached(game, actor);
        self.settle(game, actor);
    }

    /// From the patrol point just reached (and waited at) to the next: along the chain;
    /// where the chain loops, round again; a repeatable patrol whose chain ends starts
    /// over at its first point [hypothesis: the documentation says "loop or go back and
    /// forth"]; else the patrol is done.
    pub(crate) fn patrol_on(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) {
        let a = &self.agents[&actor];
        let (Some(package), Some(Goal::Route(route))) = (a.package, a.goal.clone()) else { return };
        let at = a.route_at;
        let on_point = a.using.is_some() && a.using == route.points.get(at).map(|p| p.0);
        let next = if at + 1 < route.points.len() {
            Some(at + 1)
        } else {
            route.loops_to.or(route.repeatable.then_some(0)).filter(|_| route.points.len() > 1)
        };
        match next {
            Some(n) => {
                // Off the patrol point's furniture before walking on (`arrived`).
                if on_point {
                    self.set_using(game, actor, None);
                }
                self.agents.get_mut(&actor).expect("listed").route_at = n;
                let (point, place) = route.points[n];
                self.plan(game, nav, actor, package, place, Some(point));
            }
            None => {
                self.agents.get_mut(&actor).expect("listed").activity = Activity::There;
                game.package_goal_met(actor);
            }
        }
    }
}

/// The engine's xMarkerHeading base (`XMarkerHeading`, `meshes\markerxheading.nif`)
/// [observed: Fallout3.esm and FalloutNV.esm references name it; the base is the
/// engine's own, in no plugin].
const X_MARKER_HEADING: FormId = FormId(0x0000_0034);

/// The radius a Guard package's actor wanders within once it is at its location: the
/// location's radius, when over 0 ("If it has a radius greater than 0, the actor will
/// wander within that radius" [documented: GECK wiki, Guard Package, Guard Location]).
/// `None` for other types, for a radius of 0 or an "in cell" location (the guard stands
/// where it arrived), for a radius below [`GUARD_WANDER_MIN_RADIUS`] (likewise) and for a
/// Guard that remains near the reference it guards, whose place is that reference's and
/// whose area may be the target's distance ([`goal::resolve`]) [design, #2027: that one
/// is left as it was; packages.md §7.7].
pub fn guard_wander_radius(def: &fnv_game::package::PackageDef) -> Option<f32> {
    if def.ty != PackageType::Guard || def.remain_near_reference() {
        return None;
    }
    // "In cell" has no radius: the editor disables it [documented: GECK wiki,
    // Category:Packages, Standard Location], and `goal::resolve` ignores what is stored.
    def.location.filter(|l| !matches!(l.kind, fnv_game::package::LocationKind::InCell(_))).map(|l| l.radius as f32).filter(|r| *r >= GUARD_WANDER_MIN_RADIUS)
}

/// The smallest location radius a guard wanders in, game units; a smaller one stands like
/// a radius of 0 [design, #2027]. A guard's spots lie at least half the radius from where
/// it stands (`Sandbox::guard`): at 16 that is 8 units, still well past the 1 unit within
/// which a walk's end counts as reached (`fnv_nav::follow::GOAL_RADIUS`); a smaller radius
/// is less than about a frame's step at a run (`fnv_nav::follow::TAKE_OFF_SNAP`, 16), and
/// steps of a few units would only twitch the actor on the spot.
pub const GUARD_WANDER_MIN_RADIUS: f32 = 16.0;

/// Arrival's standing heading [documented: GECK wiki, Bethsoft Tutorial NPC population,
/// linked xMarkerHeading destinations; docs/spec-notes/packages.md §3].
fn marker_heading(game: &Game, actor: FormId, def: &fnv_game::package::PackageDef, goal: Option<&Goal>) -> Option<f32> {
    // At a small editor location, Travel and standing Guard restore the placed
    // heading [hypothesis, #2526, observer check 213]. Sandbox/wander choose their own;
    // a Guard remaining near its target uses that target's place (`goal::resolve`).
    if def.location.is_some_and(|loc| loc.kind == fnv_game::package::LocationKind::NearEditor && (loc.radius as f32) < GUARD_WANDER_MIN_RADIUS)
        && (def.ty == PackageType::Travel || def.ty == PackageType::Guard && !def.remain_near_reference())
    {
        return game.world.placed.get(actor).map(|p| p.rot[2]);
    }
    // Read the effective destination kept when travel began, including a Guard's
    // target, rather than selecting the location again [design, #2526; goal::resolve].
    let destination = match goal? {
        Goal::Talk(talk) => talk.wait.as_ref(),
        goal => goal,
    };
    let Goal::Near { reference: Some(r), .. } = destination else { return None };
    game.world.placed.get(*r).filter(|p| p.base == X_MARKER_HEADING).map(|p| p.rot[2])
}

/// The actor is where its package wants it: the package's goal is met, or, for a Use
/// Weapon package, its shooting begins (its goal is met when the shots are spent,
/// `fnv_game::use_weapon`).
fn reached(game: &mut Game, actor: FormId) {
    if game.weapon_use(actor).is_some_and(|w| matches!(w.source, Source::Package(_))) {
        game.weapon_use_in_place(actor);
    } else {
        game.package_goal_met(actor);
    }
}

/// Whether a furniture reference is one to sleep on (FURN marker flags).
fn is_bed(game: &Game, furniture: FormId) -> bool {
    let base = game.world.placed.get(furniture).map(|p| p.base);
    base.and_then(|b| game.data.furniture.get(&b)).is_some_and(|f| f & fnv_game::package::furniture::SLEEP != 0)
}

/// How far from its package's place a restrained actor counts as there, game units
/// ([`Ai::begin_in_place`]) [our choice: the furniture code walks an actor the last steps
/// to an entry point; a captive is placed beside its marker].
pub const RESTRAINED_REACH: f32 = 256.0;

/// The patrol point nearest the actor (points in its own space first).
fn nearest_point(route: &Route, feet: Place) -> usize {
    let key = |p: &Place| (p.space != feet.space, hdist(p.pos, feet.pos));
    (0..route.points.len())
        .min_by(|a, b| {
            let (ka, kb) = (key(&route.points[*a].1), key(&route.points[*b].1));
            ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
        })
        .unwrap_or(0)
}


/// How long the player's walk into a Dialogue package's trigger location may get no
/// nearer its point before the actor talks where the player stands
/// ([`Ai::lead_player`]) [our choice].
pub const LEAD_STUCK_SECONDS: f64 = 1.5;

/// How long after a conversation from a Dialogue package its actor does not walk the
/// player into its trigger location again ([`Ai::lead_player`], #3464) [our choice:
/// longer than a walk out of a room; observer check 280 asks what the original does].
pub const LEAD_AGAIN_SECONDS: f64 = 30.0;

/// How much nearer the point counts as progress on that walk, in units [our choice].
const LEAD_GAIN: f32 = 8.0;

/// Whether two references are in the same space now, as the world has their cells: one
/// worldspace (any of its exterior cells), or one interior cell. Unknown cells count as the
/// same.
fn same_space(game: &Game, a: FormId, b: FormId) -> bool {
    let cell = |r: FormId| game.world.peek(r).and_then(|s| s.cell).or_else(|| game.world.placed.get(r).and_then(|p| p.cell));
    match (cell(a), cell(b)) {
        (Some(x), Some(y)) => x == y || game.world.placed.world_of(x).is_some_and(|w| game.world.placed.world_of(y) == Some(w)),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn explosion_zones_that_still_give_rads_are_ways_to_avoid() {
        use fnv_game::radiation::RadZone;
        let zone = |space: u32, x: f32, age: f64| RadZone { serial: 1, explosion: FormId(0x10), space: FormId(space), pos: [x, 0.0, 0.0], level: 10.0, radius: 616.0, duration: 12.0, age };
        // A Fat Man's zone across the way in this world; a faded one, one in another
        // space and one past the search are left out.
        let zones = [zone(0x3C, 500.0, 2.0), zone(0x3C, 500.0, 12.5), zone(0x99, 500.0, 2.0), zone(0x3C, RADIATION_SEARCH + 1000.0, 2.0)];
        let got: Vec<([f32; 3], f32)> = super::zones_near(&zones, FormId(0x3C), [0.0, 0.0, 0.0]).collect();
        assert_eq!(got, vec![([500.0, 0.0, 0.0], 616.0)]);
    }

    use super::*;

    /// In step: at its distance a follower goes at its target's pace, a little faster
    /// the farther behind, and its run only when well behind.
    #[test]
    fn a_follower_keeps_step_and_runs_only_to_catch_up() {
        let (walk, run, keep) = (60.0, 200.0, 192.0);
        assert_eq!(keep_step_speed(80.0, keep, keep, walk, run), 80.0);
        assert_eq!(keep_step_speed(80.0, keep + 30.0, keep, walk, run), 90.0);
        assert_eq!(keep_step_speed(80.0, 700.0, keep, walk, run), run);
        // Nearer than its distance it does not slow below the target's pace.
        assert_eq!(keep_step_speed(80.0, 50.0, keep, walk, run), 80.0);
        // A target slower than half its walk: it keeps to half its walk.
        assert_eq!(keep_step_speed(10.0, keep, keep, walk, run), 30.0);
        // The speed changes smoothly with the gap: no jump between walk and run.
        let mut last = keep_step_speed(80.0, keep, keep, walk, run);
        for gap in (0..400).map(|g| keep + g as f32) {
            let v = keep_step_speed(80.0, gap, keep, walk, run);
            assert!(v >= last && v - last <= 0.34, "{gap}: {last} to {v}");
            last = v;
        }
    }
}
