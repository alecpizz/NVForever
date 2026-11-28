//! Actors in a combat state (`fnv_game::awareness`; docs/design/alert.md §4): what the
//! AI has them do instead of their package.
//!
//! - **Alert and Lost**: the actor searches [documented: GECK wiki Category:Detection,
//!   "Alert and Lost actors search … if their package allows"]. It gets off furniture,
//!   walks to where its latest alarm is (what it noticed, heard or last saw), looks
//!   that way for a few seconds, then searches on: at the alarm again if it has moved
//!   (the hostile was noticed somewhere else), else at a random point around it within
//!   the first search radius. It walks, it does not run [engineering choice]. The Combat
//!   Search page that describes the original's search is not in the owner's export; the
//!   radii are FalloutNV.esm's fCombatSearchInteriorMinRadius and
//!   fCombatSearchExteriorMinRadius [observed: 256 and 512; hypothesis: the radius of
//!   the nearest search ring].
//! - **Combat**: the actor closes in on its target until it is within the distance it
//!   attacks from (`Awarenesses::engage`, which the engine's combat AI sets from its
//!   weapon), running, the way planned again as the target moves; there it stops and
//!   faces it. Without such a distance it holds its ground [design]. The attacks
//!   themselves are the engine's (nvforever `npc_combat`). A target it does not detect
//!   now it closes in on whatever the distance, halving the gap each time the way is
//!   planned, until it sees it again [design]. While it aims down its sights
//!   (`Awarenesses::aiming`, set by the combat AI) it walks instead of running, as the
//!   player does while aiming [design: the owner's wish, playtest list 2026-10-11, "the
//!   player and NPCs should not be able to move at full speed whilst aiming"].
//! - **Continue During Combat** (docs/design/alert.md §5.2, #2155): an actor whose
//!   current package has the flag (`Game::continues_during_combat`) carries its package
//!   out as if nothing had happened only while the flag holds it ([`Ai::package_holds`]):
//!   on the way of a Travel, Patrol, Find, Ambush or Use Item At package, not yet
//!   arrived and not yet taken up by the fight; on a Guard while its target is outside
//!   the guard radius, measured from the post to the target; and on a Use Weapon package
//!   (#2120, #2145). Anywhere else it fights as any other actor does, and goes back to
//!   its package's place once the fight is over [observed: Lanius left his marker within
//!   1.2 s and fought up to 2,557 units from it, then stood on it again 3 to 5 s after
//!   the fight; a knife trooper on a stand-in-place Travel fought up to 2,043 units from
//!   its post; checks 105 to 109].
//! - **Fleeing** (docs/design/combat-ai.md §12.2) [documented: GECK wiki Combat Flee]:
//!   an actor the combat AI has fleeing (`fnv_game::combat_group::Flight`, any combat
//!   state) runs, in this order of tries, through a door within fCombatFleeMaxDoorDistance
//!   that is not nearer its attacker than it is, to cover between
//!   fCombatFleeMinCoverDistance and fCombatFleeMaxCoverDistance from the attacker (a
//!   navmesh cover edge the attacker is beyond, `fnv_nav::CoverEdge`), or
//!   fCombatFleeNormalDistance straight away from it (turning up to 90° either way when
//!   the navmesh ends there); at the spot it stays, facing the attacker. Cornered (no
//!   way out) it stays where it is [design: the choice among doors and cover, nearest
//!   first]. At its spot or cornered a person hides crouched (`Game::sneaks`,
//!   `Game::at_flee_spot`; combat-ai.md §12.2.1).
//! - **Posts** (§12.1): a place the combat AI wants a fighter at (cover, a peek out of
//!   it, a flank, a step aside, a step back; `fnv_game::combat_group::Post`): it goes
//!   there, running unless it aims down its sights, and stands facing its target. A post
//!   it has no way to is dropped and it closes in as before. So is one whose way ends, or
//!   runs out of time, with the actor still [`POST_ARRIVAL`] or more from it: it holds or
//!   closes in on that same update, and what comes next is the combat AI's to choose.
//! - **Shared searches** (§12.5) [documented: GECK wiki Combat Search]: a hostile only
//!   noticed is searched for midway between the actor and it; the comrades searching for
//!   the same actor's alarm (`Game::search_party`) each take their own sector of the
//!   circle round the search's centre [design: the wiki splits the navmesh area; sectors
//!   stand in], [`SEARCH_POINTS`] points making one complete search; the first within
//!   the minimum search radius, later ones within the medium one; after
//!   iCombatTargetLostRemoveSearchCount complete searches the alarm is dropped
//!   (`Game::searched_once`).
//! - **Warnings** (in Normal; aggro radius and trespass, `Game::warning_to`,
//!   `fnv_game::aggro` "The look"): an actor warning the player stops and turns to it
//!   until the warning is over (it attacks, or the player is out of its radius or of the
//!   owned cell, or out of its sight), then goes back to its package as after a search
//!   [hypothesis].
//! - Back in **Normal**, the actor goes back to its package's place, as after a random
//!   conversation.

use crate::carry::{Activity, AiEvent};
use crate::goal::Goal;
use fnv_game::package::PackageType;
use crate::goal;
use crate::Ai;
use fnv_game::awareness::{Cause, State};
use fnv_game::combat_group::{FleeBy, Flight, Post, PostKind, POST_ARRIVAL};
use fnv_game::game::Game;
use fnv_nav::{NavGraph, Place, Space};
use fnv_sim::FormId;
use serde::Serialize;

/// A creature in combat turns to face its target this many times faster than the
/// standing turn rate (240 degrees a second, its own × 1.25 on top): the engine's gunmen
/// fire only facing their target (nvforever npc_combat, owner's playtest list 5 item
/// 33), and at the standing rate a turn to a new target held their fire for up to 1.5 s
/// [design]. Kept for creatures when people took the measured rates below (#1352): only
/// a person was measured [hypothesis: a creature's combat turn is unmeasured].
pub const COMBAT_TURN_CREATURE: f32 = 2.0;

/// A person standing in combat turns to face its target at this rate, radians a second,
/// while the target is within a quarter turn of its heading (225 degrees a second), and
/// at [`COMBAT_TURN_WIDE`] beyond: an NCR trooper given StartCombat 45° and 90° off its
/// bearing to the player turned 3.5° a game loop, steadily up to the bearing, and 135°
/// (and 180°) off, 4.3° a loop [observed 2026-11-06, capture o25_turn,
/// docs/spec-notes/actors.md §8.9; hypothesis: the threshold between 90° and 135° lies at
/// 90°, and the rate goes by the angle still to turn: the original may pick it when the
/// turn begins, and the data cannot tell the two apart]. No ease-out: the slower end of a
/// long turn came from stepping off while turning (the walk's own turn rate).
pub const COMBAT_TURN_NEAR: f32 = 225.0 * std::f32::consts::PI / 180.0;
/// The combat turn rate with the target more than a quarter turn off (275 degrees a
/// second), [`COMBAT_TURN_NEAR`].
pub const COMBAT_TURN_WIDE: f32 = 275.0 * std::f32::consts::PI / 180.0;
/// Seconds a fighter stands on its heading after it goes into Combat (from Normal or
/// Alert) before it turns to its target and sets off: the trooper above stood still on
/// its first heading for about 1.1 s after StartCombat, already in combat [observed
/// 2026-11-06, capture o25_turn; hypothesis: the same when it goes into combat by
/// detecting its target]. People only ([`combat_began`]): creatures turn and charge at
/// once as before [hypothesis: a creature's reaction is unmeasured; o25's spawned
/// coyotes were in combat at once].
pub const COMBAT_REACTION_SECONDS: f64 = 1.1;

/// The combat turn rate ([`COMBAT_TURN_NEAR`], [`COMBAT_TURN_WIDE`]) with `off` radians
/// still to turn.
pub fn combat_turn_rate(off: f32) -> f32 {
    if off.abs() > std::f32::consts::FRAC_PI_2 {
        COMBAT_TURN_WIDE
    } else {
        COMBAT_TURN_NEAR
    }
}

/// The combat turn as a share of the standing turn rate ([`fnv_nav::follow::SPOT_TURN_RATE`];
/// a walker's own standing rate, a creature's × 1.25, scales with it).
pub fn combat_turn(off: f32) -> f32 {
    combat_turn_rate(off) / fnv_nav::follow::SPOT_TURN_RATE
}

/// Whether `actor` is a creature by its record (`fnv_game::nvse_nx::is_creature`); an
/// actor of no known record counts as a person.
pub fn creature(game: &Game, actor: FormId) -> bool {
    fnv_game::nvse_nx::is_creature(game, actor) == Some(true)
}

/// When a person went into Combat (`Awareness::combat_began`), for the moment it stands
/// ([`COMBAT_REACTION_SECONDS`]); `None` for a creature, which does not stand.
pub fn combat_began(game: &Game, actor: FormId) -> Option<f64> {
    let aw = game.awareness.get(actor)?;
    (aw.state == State::Combat && !creature(game, actor)).then_some(aw.combat_began).flatten()
}

/// Whether a person in Combat is still in its moment after going into it
/// ([`COMBAT_REACTION_SECONDS`]): it stands on its heading. Never a creature.
pub fn reacting(game: &Game, actor: FormId) -> bool {
    combat_began(game, actor).is_some_and(|t| game.time - t < COMBAT_REACTION_SECONDS)
}

/// Seconds an actor looks about where it searched before it searches on
/// [engineering choice].
pub const LOOK_SECONDS: f64 = 4.0;
/// After no way was found to a target, seconds before [`Ai::close_in`] looks for one
/// again, doubled at each failure in a row up to [`NO_WAY_RETRY_MAX`]; meanwhile the actor
/// holds its ground [design, #3785: an actor in combat with a target it could not reach
/// searched the navmesh for a way every frame, each search ending at its limit (50 to
/// 100 ms): two Powder Gangers by the NCR Correctional Facility, fighting ED-E, held the
/// game at 9 frames a second; a target that comes within reach is fought from where the
/// actor stands, and a way that opens is found within the wait].
pub const NO_WAY_RETRY_SECONDS: f64 = 2.0;
/// See [`NO_WAY_RETRY_SECONDS`].
pub const NO_WAY_RETRY_MAX: f64 = 16.0;
/// A walk to a search point that has not arrived after this long ends where it is
/// [engineering choice].
pub const SEARCH_WALK_SECONDS: f64 = 30.0;
/// An alarm that moved this far from the point last searched is searched at again
/// [engineering choice].
pub const RECENTRE: f32 = 64.0;
/// Closing in on a target: the way is planned again when the target has moved this
/// far from where it was planned to, or after this long [design].
pub const CLOSE_REPLAN_DISTANCE: f32 = 96.0;
pub const CLOSE_REPLAN_SECONDS: f64 = 1.5;
/// Near its target the way is planned again sooner: once the target has moved this share
/// of what the actor still has to come (the distance less where it stops), never less
/// than [`CLOSE_REPLAN_NEAR`] nor more than [`CLOSE_REPLAN_DISTANCE`] ([`replan_slack`])
/// [design: owner's playtest 2026-11-11, #2163: a player backing away was never hit.
/// With the 96 units at any distance, a chaser's way ended where its target had stood,
/// up to 96 units behind it and out of a melee reach; it stopped there and set off
/// again, in step with its target].
pub const CLOSE_REPLAN_SHARE: f32 = 0.25;
pub const CLOSE_REPLAN_NEAR: f32 = 12.0;
/// A target that moves is closed in on where it will be this many seconds on, at the
/// speed it moved since the way was last planned, at most [`CHASE_LEAD_MAX`] units
/// ahead of it; a target slower than [`CHASE_LEAD_MIN_SPEED`] units/s stands
/// ([`lead`]), while the actor is beyond its distance [design, #2163: the chaser
/// catches up with a target that keeps going instead of trailing it].
pub const CHASE_LEAD_SECONDS: f32 = 0.35;
pub const CHASE_LEAD_MAX: f32 = 96.0;
pub const CHASE_LEAD_MIN_SPEED: f32 = 40.0;
/// A chased target's speed is measured over at least this long, seconds ([`Drift`]).
pub const DRIFT_SECONDS: f32 = 0.05;
/// Closing in ends this share of the stop distance short of the target, inside it
/// [design].
pub const STOP_SHORT: f32 = 0.9;
/// In Combat, a target it does not detect now: it closes in to this share of the
/// distance, again each time the way is planned [design].
pub const BLIND_CLOSE_SHARE: f32 = 0.5;
/// A fighter standing at its distance closes in on a target it no longer detects only
/// once it has gone this long without detecting it, seconds [design: owner's playtest
/// list 7 item 45, a recorded fight between NCR troopers and Fiends (smoke `battle-ncr-fiends`,
/// 2026-10-23): a Fiend 1,500 units from a trooper, within its stop distance, stopped and
/// set off again 3 times a second for 15 s, as its sight of the trooper came and went
/// behind the other fighters; closing in at the first frame unseen made it run and stop
/// on every flicker]. One already closing in goes on.
pub const BLIND_CLOSE_AFTER: f64 = 1.5;
/// Search points that make one complete search of an actor's share [design].
pub const SEARCH_POINTS: u32 = 3;
/// A post that moved less than this from where the way to it was planned is kept, game
/// units [design].
pub const POST_SLACK: f32 = 48.0;
/// One that has arrived goes on counting as there until it is this far from the post, game
/// units; past that it goes back [design: wider than [`POST_ARRIVAL`], so an actor shoved
/// about at the edge does not go back and forth between the two].
pub const POST_RETAINED: f32 = 96.0;
/// A flight or a walk to a post that has not arrived after this long ends where it is
/// [design].
pub const FLEE_WALK_SECONDS: f64 = 30.0;
/// Behind a cover edge a fleeing actor stands this far in from it, game units [design:
/// about an actor's width].
pub const COVER_STANDOFF: f32 = 48.0;

/// Why a walk to a post ended away from it ([`AiEvent::PostFailed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PostFailure {
    /// [`FLEE_WALK_SECONDS`] passed with the actor still on its way.
    TimedOut,
    /// The way ended, or the actor stopped walking it, too far from the post.
    StoppedShort,
}

/// How far a target may move from where the way to it was planned before the way is
/// planned again, for an actor `away` from it that stops within `stop`
/// ([`CLOSE_REPLAN_SHARE`]).
pub fn replan_slack(away: f32, stop: f32) -> f32 {
    ((away - stop) * CLOSE_REPLAN_SHARE).clamp(CLOSE_REPLAN_NEAR, CLOSE_REPLAN_DISTANCE)
}

/// Whether an actor closing in keeps its way: planned at `since` for a target then at
/// `to`, now at `at`, `away` from the actor, which stops within `stop`.
pub fn keeps_way(to: [f32; 3], since: f64, at: [f32; 3], away: f32, stop: f32, now: f64) -> bool {
    goal::dist(to, at) < replan_slack(away, stop) && now - since < CLOSE_REPLAN_SECONDS
}

/// A chased target's speed on the ground as its chaser measures it ([`Search::Closing`]).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Drift {
    /// Units/s.
    pub speed: [f32; 2],
    /// Where the target stood at the last measurement, and when (`Game::time`).
    pub from: [f32; 2],
    pub at: f64,
    /// `speed` was measured at least once: until then it is only "standing, as far as is
    /// known", and the first measurement is taken as it is ([`Drift::on`]).
    pub measured: bool,
}

impl Drift {
    /// A target first seen at `at`: standing, as far as is known.
    pub fn begin(at: [f32; 3], now: f64) -> Self {
        Self { speed: [0.0; 2], from: [at[0], at[1]], at: now, measured: false }
    }

    /// Measured again with the target now at `at`: its speed since the last
    /// measurement, evened half and half with the one before; unchanged within
    /// [`DRIFT_SECONDS`] of the last (a way planned every frame measures nothing in
    /// one frame's step).
    ///
    /// The first measurement is taken as it is, not evened with the "standing" it began
    /// with [design, #3256]: evened, a player walking backwards at 69 units/s first read
    /// as 34.5, under [`CHASE_LEAD_MIN_SPEED`], so a chaser that had stood took its
    /// target for standing, held at the end of a 20-unit run and set off again a quarter
    /// of a second later: 47 to 78 stops and starts in 8 s for a feral ghoul, a gecko, a
    /// cazador and a person with a lead pipe [observed: runs of main 069ef9bde on the
    /// Searchlight Airport runway, 2026-11-21]. A running player (273, first read as
    /// 136) passed the threshold either way, which is why only a walk showed it.
    pub fn on(self, at: [f32; 3], now: f64) -> Self {
        let dt = (now - self.at) as f32;
        if dt < DRIFT_SECONDS {
            return self;
        }
        let share = if self.measured { 0.5 } else { 1.0 };
        Self { speed: [0, 1].map(|i| (at[i] - self.from[i]) / dt * share + self.speed[i] * (1.0 - share)), from: [at[0], at[1]], at: now, measured: true }
    }
}

/// Where a target at `at` moving at `drift` is closed in on ([`CHASE_LEAD_SECONDS`]).
pub fn lead(at: [f32; 3], drift: [f32; 2]) -> [f32; 3] {
    let speed = drift[0].hypot(drift[1]);
    if speed < CHASE_LEAD_MIN_SPEED {
        return at;
    }
    let ahead = (speed * CHASE_LEAD_SECONDS).min(CHASE_LEAD_MAX) / speed;
    [at[0] + drift[0] * ahead, at[1] + drift[1] * ahead, at[2]]
}

/// Where an actor in a combat state is ([`crate::Agent::search`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "search", rename_all = "snake_case")]
pub enum Search {
    /// Walking to a point to search, since `since` (`Game::time`): a search centred on
    /// `centre` for `owner`'s alarm that was at `alarm` then; `points` searched of this
    /// complete search [design: `Ai::search_at`].
    Going { owner: FormId, to: [f32; 3], since: f64, centre: [f32; 3], alarm: [f32; 3], points: u32 },
    /// Looking about at `at` until `until` for `owner`'s alarm [design: `Ai::alert_step`].
    Looking { owner: FormId, at: [f32; 3], until: f64, centre: [f32; 3], alarm: [f32; 3], points: u32 },
    /// [design, #2010] Finished its shares: wait for the party, another alarm or
    /// Normal. Keep the search marker so `Ai::search_over` returns to the package.
    Waiting,
    /// In Combat: standing, facing its target.
    Holding { target: FormId },
    /// In Normal, giving aggro radius or trespass warnings: standing, facing whom it warns.
    Warning { target: FormId },
    /// In Combat: running up to its target, last planned for where it stood (`to`) at
    /// `since`; `drift`: how fast the target moves on the ground.
    Closing { target: FormId, to: [f32; 3], since: f64, drift: Drift },
    /// In Combat: going to the post the combat AI chose, planned at `since`.
    Posted { kind: PostKind, target: FormId, to: [f32; 3], since: f64 },
    /// In Combat: at its post, facing its target.
    AtPost { kind: PostKind, target: FormId, at: [f32; 3] },
    /// Running from `from` to its flee spot `to`, since `since`.
    Fleeing { from: FormId, to: [f32; 3], since: f64 },
    /// At its flee spot (or cornered), facing `from`.
    Fled { from: FormId, at: [f32; 3] },
    /// Running to `from` (a weapon, a body, ammunition) at `to` to take up a better
    /// weapon (`crate::rearm`), since `since`.
    Fetching { from: FormId, to: [f32; 3], since: f64 },
}

fn midway(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0]
}

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

impl Ai {
    /// Whether `actor`'s package keeps it on its package in a combat state: Continue
    /// During Combat as the original was seen to read it (module notes; alert.md §5.2).
    pub(crate) fn package_holds(&self, game: &mut Game, actor: FormId) -> bool {
        if !game.continues_during_combat(actor) {
            return false;
        }
        let Some(def) = game.current_package(actor).and_then(|p| game.package_def(&p)) else { return false };
        let a = &self.agents[&actor];
        match def.ty {
            // On the way: still walking its package's walk, the fight not yet having
            // taken it up (a fighter's own walk sets `search`). Arrived, or once the
            // fight has it, it fights; the package choice after the fight sends it back.
            // [documented: GECK wiki "Combat Package Flags": a Travel actor goes on to
            // its destination; observed: on the way, Lanius kept on to his marker for
            // 10 s with the player out of his sight (one capture), check 107]
            PackageType::Travel | PackageType::Patrol | PackageType::Find | PackageType::Ambush | PackageType::UseItemAt => a.search.is_none() && a.activity == Activity::Travelling,
            // Not in the wiki's chart; its fire goes on in combat (`fnv_game::use_weapon`,
            // #2120, #2145) [design: left as it was].
            PackageType::UseWeapon => true,
            PackageType::Guard => self.guard_holds(game, actor, def),
            // Follow, Sandbox and the others: the flag does not hold them [documented:
            // the wiki, "the flags do not work" for Accompany, Dialogue, Eat, Escort,
            // Sandbox, Sleep, Wander; Follow (a non-teammate): design, §5.2 item 1].
            _ => false,
        }
    }

    /// A Guard with Continue During Combat holds its actor while its target is outside
    /// the guard radius of its post (`Game::guard_combat_radius`), measured from the post
    /// to the target, flat [documented: GECK wiki "will not move outside the guard radius
    /// in order to pursue the target"; observed: check 106, the target's distance from
    /// the post decided]. Not a Guard with type-specific bit 2 [hypothesis: three
    /// such packages did not hold their actor; a Guard with no type-specific flags was not
    /// measured beyond 259 units and is held, as the wiki says], not one near a reference
    /// (its post moves with the reference, not measured) and not one with no target known.
    /// The post: the location's place, a "near current location" guard's wander area or
    /// where it last stood out of combat [observed: "the post is where the trooper got
    /// the package"].
    fn guard_holds(&self, game: &Game, actor: FormId, def: &fnv_game::package::PackageDef) -> bool {
        if def.guard_bit_2() || def.remain_near_reference() {
            return false;
        }
        let a = &self.agents[&actor];
        let post = match (&a.goal, &a.sandbox) {
            (_, Some(s)) => Some(s.area().0),
            (Some(Goal::Near { place, .. }), None) => Some(*place),
            (Some(Goal::Here), None) => a.calm_at,
            _ => None,
        };
        let Some(post) = post else { return false };
        let Some(target) = game.combat_target_of(actor).or_else(|| game.awareness.get(actor).and_then(|aw| aw.combat_target().map(|(t, _)| t))) else { return false };
        let Some(at) = goal::place_of_ref(game, target) else { return false };
        at.space == post.space && flat(post.pos, at.pos) > game.guard_combat_radius(def)
    }

    /// One update of an actor in a combat state (module notes). Returns whether the state
    /// holds the actor: its package's own doings wait meanwhile.
    pub(crate) fn alert_step(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, dt: f32) -> bool {
        let state = game.awareness.state(actor);
        let held = state.in_combat() && self.package_holds(game, actor);
        let a = self.agents.get_mut(&actor).expect("listed");
        if !state.in_combat() {
            a.calm_at = Some(Place { space: a.walker.space, pos: a.walker.feet });
        }
        if a.held != held {
            a.held = held;
            if let Some(package) = game.current_package(actor).filter(|_| state.in_combat()) {
                self.log(game, actor, AiEvent::PackageHolds { package, held });
            }
        }
        if !state.in_combat() || held {
            game.calm_rearm(actor);
            // A spectator running from a fight that came too close (`crate::social`).
            if !state.in_combat() && self.spectator_step(game, nav, actor, dt) {
                return true;
            }
            if let Some(target) = game.warning_to(actor).filter(|_| !state.in_combat()) {
                if self.warn_step(game, actor, target, dt) {
                    return true;
                }
            }
            if let Some(s) = self.agents[&actor].search {
                let why = if matches!(s, Search::Warning { .. }) {
                    "its warning is over"
                } else if state.in_combat() {
                    "its package continues during combat"
                } else {
                    "back to Normal"
                };
                self.search_over(game, nav, actor, why);
            }
            return false;
        }
        // Relinquish the sandbox before any combat movement (or an early return
        // without a target). Repeated updates keep the new combat walk intact.
        self.interrupt_sandbox(game, actor);
        // Off furniture first: the engine gets it up, and its walk waits until it has.
        if self.agents[&actor].using.is_some() {
            self.set_using(game, actor, None);
            self.log(game, actor, AiEvent::Furniture { furniture: None });
        }
        // Fleeing comes first, in every combat state.
        if let Some(flight) = game.awareness.tactics.flights.get(&actor).copied() {
            self.flee_step(game, nav, actor, flight, dt);
            return true;
        }
        // Then a better weapon (`crate::rearm`).
        if self.rearm_step(game, nav, actor, dt) {
            return true;
        }
        let Some(aw) = game.awareness.get(actor) else { return true };
        let now = game.time;
        let a = &self.agents[&actor];
        let space = a.walker.space;
        if state == State::Combat {
            // Just in combat it stands a moment on its heading before it turns and sets
            // off ([`COMBAT_REACTION_SECONDS`]).
            if reacting(game, actor) {
                let a = self.agents.get_mut(&actor).expect("listed");
                if a.activity == Activity::Travelling {
                    a.walker.follower = None;
                    a.activity = Activity::There;
                }
                return true;
            }
            // The target it detects; just after losing it, the one detected last.
            let latest = aw.targets.iter().filter(|(_, t)| t.detected.is_some()).max_by(|x, y| x.1.detected.unwrap_or(0.0).total_cmp(&y.1.detected.unwrap_or(0.0)));
            let Some((target, at)) = aw.combat_target().or(latest.map(|(id, t)| (*id, t))).map(|(id, t)| (id, t.at)) else { return true };
            let sees = aw.combat_target().is_some();
            // A target it detects now is followed where it is, not where it was when
            // it was last measured [design: owner's playtest 2026-11-11, #2163: a player
            // backing away was never hit. Detection is measured a few times a second; a
            // feral ghoul ran to where the player had been measured, stopped there
            // "119 units away" and set off again a quarter of a second later with the
            // player 215 away, 19 times in 7 s, without a blow (smoke
            // `melee-chases-backing-player`)].
            let at = match goal::place_of_ref(game, target) {
                Some(p) if sees && p.space == space => p.pos,
                _ => at,
            };
            // Unseen only briefly: it counts as seen (`BLIND_CLOSE_AFTER`).
            let last_seen = aw.targets.get(&target).and_then(|t| t.detected);
            let sees = sees || last_seen.is_some_and(|t| now - t < BLIND_CLOSE_AFTER);
            let away = goal::dist(a.walker.feet, at);
            // A fighter on its way stands up: it never crouch-walks across a fight
            // (docs/design/combat-ai.md §14.2).
            if a.activity == Activity::Travelling {
                game.awareness.crouching.remove(&actor);
            }
            // A post the combat AI chose (cover, a flank …) comes before closing in.
            if let Some(post) = game.awareness.tactics.posts.get(&actor).copied() {
                if self.post_step(game, nav, actor, target, at, post, dt) {
                    return true;
                }
            }
            let a = &self.agents[&actor];
            // It stops within its distance and, standing, sets off again only when the
            // target is beyond what it can attack from, so it does not stop and start at
            // the edge.
            let holding = a.search == Some(Search::Holding { target });
            let engage = game.awareness.engage.get(&actor).copied();
            // A target it does not detect now (out of sight behind a rock, say) it closes
            // in on, whatever the distance, halving the gap, until it sees it again
            // [design: an actor that stood where it could not see its target went Lost
            // and back to Combat every few seconds at Cook-Cook's camp].
            // On the way to a target that keeps moving it does not stop at its
            // distance to set off again a few units later: it keeps on to where its
            // way ends, [`STOP_SHORT`] of the distance, and so keeps its target's pace
            // there [design, #2163].
            // Its target's speed as of now ([`Drift::on`]), not as of the last time the way
            // was planned: after a short run from standing the speed had not been measured
            // yet when the way ended, so the target counted as standing and the chaser held
            // (#3256).
            let keeps_pace = matches!(a.search, Some(Search::Closing { target: t, drift, .. }) if t == target && {
                let d = drift.on(at, now);
                d.speed[0].hypot(d.speed[1]) >= CHASE_LEAD_MIN_SPEED
            });
            let close_in = (!sees && engage.is_some())
                || engage.is_some_and(|(stop, resume)| away > if holding { resume } else if keeps_pace { stop * STOP_SHORT } else { stop });
            let stop = if sees { engage.map_or(0.0, |e| e.0) } else { away * BLIND_CLOSE_SHARE };
            let going = a.activity == Activity::Travelling;
            match (close_in, a.search) {
                // On its way, and the way still leads to the target.
                (true, Some(Search::Closing { target: t, to, since, .. })) if t == target && going && keeps_way(to, since, at, away, stop, now) => {}
                (true, _) => self.close_in(game, nav, actor, target, at, away, stop),
                (false, Some(Search::Holding { target: t })) if t == target => {}
                // At the end of its way behind a target that moves: it waits there for
                // the target's next step, measuring its speed, and holds once the
                // target has stopped. So too until the target's speed has first been
                // measured (a short way from standing ends before [`DRIFT_SECONDS`]).
                (false, Some(Search::Closing { target: t, to, since, drift })) if t == target && (keeps_pace || now - drift.at < f64::from(DRIFT_SECONDS)) => {
                    let a = self.agents.get_mut(&actor).expect("listed");
                    a.search = Some(Search::Closing { target, to, since, drift: drift.on(at, now) });
                }
                (false, _) => {
                    let a = self.agents.get_mut(&actor).expect("listed");
                    a.walker.follower = None;
                    a.activity = Activity::There;
                    a.search = Some(Search::Holding { target });
                    self.log(game, actor, AiEvent::Hold { target, away });
                }
            }
            // Aiming down the sights slows it to a walk, also on a way already planned.
            let aiming = game.awareness.aiming.contains(&actor);
            let own = self.alert_speeds(actor);
            let (walk, run) = (own.walk, own.run * game.awareness.pace_of(actor));
            let a = self.agents.get_mut(&actor).expect("listed");
            if let Some(Search::Closing { target, .. }) = a.search {
                let speed = if aiming { walk } else { run };
                if a.walker.speed != speed {
                    (a.walker.speed, a.walk_pace) = (speed, None);
                    self.log(game, actor, AiEvent::Pace { target, aiming });
                }
            }
            if self.agents[&actor].activity != Activity::Travelling {
                self.combat_face(game, actor, Place { space, pos: at }, dt);
            }
            return true;
        }
        let Some(alarm) = aw.search_alarm().copied() else { return true };
        // [design, #2010] A finished member waits for its Lost party's other shares.
        if aw.state == State::Lost && game.searches_done(actor, alarm.owner) >= game.group_settings().remove_search_count {
            game.searched_once(actor, alarm.owner);
            let a = self.agents.get_mut(&actor).expect("listed");
            a.search = Some(Search::Waiting);
            a.walker.follower = None;
            a.activity = Activity::There;
            return true;
        }
        // A hostile only noticed is searched for midway between the actor and it
        // [documented: Combat Search]; anything else where it happened.
        let centre_for = |feet: [f32; 3]| if alarm.cause == Cause::Noticed { midway(feet, alarm.at) } else { alarm.at };
        let feet = a.walker.feet;
        match a.search {
            // [design, #2010] Point progress belongs to one alarm owner, even when
            // a replacement alarm is within RECENTRE of the previous one.
            Some(Search::Going { owner, to, since, centre, alarm: was, points }) if owner == alarm.owner => {
                if a.activity != Activity::Travelling || now - since > SEARCH_WALK_SECONDS {
                    let a = self.agents.get_mut(&actor).expect("listed");
                    a.walker.follower = None;
                    a.activity = Activity::There;
                    a.search = Some(Search::Looking { owner, at: to, until: now + LOOK_SECONDS, centre, alarm: was, points });
                    self.log(game, actor, AiEvent::SearchLook { at: to, seconds: LOOK_SECONDS as f32 });
                }
            }
            Some(Search::Looking { owner, until, centre, alarm: was, points, .. }) if owner == alarm.owner => {
                self.face(game, actor, Place { space, pos: alarm.at }, dt);
                if now >= until {
                    if goal::dist(was, alarm.at) > RECENTRE {
                        // The alarm moved (the hostile was noticed elsewhere): search there.
                        let c = centre_for(feet);
                        self.search_at(game, nav, actor, c, c, alarm.at, 0, alarm.owner);
                    } else {
                        // On round its share; each complete search counts.
                        let mut points = points + 1;
                        if points >= SEARCH_POINTS {
                            points = 0;
                            if game.searched_once(actor, alarm.owner)
                                || (game.awareness.state(actor) == State::Lost && game.searches_done(actor, alarm.owner) >= game.group_settings().remove_search_count) {
                                self.agents.get_mut(&actor).expect("listed").search = Some(Search::Waiting);
                                return true;
                            }
                        }
                        let (index, party) = game.search_party(actor, alarm.owner);
                        let indoors = matches!(space, Space::Interior(_));
                        let gs = game.group_settings();
                        let radii = if indoors { gs.search_interior } else { gs.search_exterior };
                        let r = if game.searches_done(actor, alarm.owner) == 0 { radii[0] } else { radii[1] };
                        let share = std::f32::consts::TAU / party.max(1) as f32;
                        let angle = share * (index as f32 + self.social.rng.range(0.0, 1.0));
                        let d = self.social.rng.range(0.4, 1.0) * r;
                        let to = [centre[0] + d * angle.cos(), centre[1] + d * angle.sin(), centre[2]];
                        self.search_at(game, nav, actor, to, centre, was, points, alarm.owner);
                    }
                }
            }
            _ => {
                let c = centre_for(feet);
                self.search_at(game, nav, actor, c, c, alarm.at, 0, alarm.owner);
            }
        }
        true
    }

    /// An actor giving aggro radius or trespass warnings (`Game::warning_to`) stops where
    /// it is and turns to whom it warns, at the standing turn rate, until the warning is
    /// over; then it goes back to its package as after a search [hypothesis: the
    /// creatures' warning idles want the actor standing, IsMoving != 1]. One on furniture
    /// stays on it. Returns whether the warning holds the actor.
    fn warn_step(&mut self, game: &mut Game, actor: FormId, target: FormId, dt: f32) -> bool {
        let Some(at) = goal::place_of_ref(game, target) else { return false };
        let a = &self.agents[&actor];
        if a.using.is_some() {
            return false;
        }
        if a.search != Some(Search::Warning { target }) {
            let away = goal::dist(a.walker.feet, at.pos);
            self.interrupt_sandbox(game, actor);
            let a = self.agents.get_mut(&actor).expect("listed");
            a.wait_until = None;
            a.walker.follower = None;
            a.activity = Activity::There;
            a.search = Some(Search::Warning { target });
            self.log(game, actor, AiEvent::Warn { target, away });
        }
        self.face(game, actor, at, dt);
        true
    }

    /// Set off to search at `to`, the navmesh point nearest it, in the search centred on
    /// `centre` for the alarm that was at `alarm`, `points` of its complete search done;
    /// without a way there the actor looks about where it stands.
    #[allow(clippy::too_many_arguments)]
    fn search_at(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, to: [f32; 3], centre: [f32; 3], alarm: [f32; 3], points: u32, about: FormId) {
        let params = self.path_params(game, actor);
        let speed = self.alert_speeds(actor).walk;
        let now = game.time;
        let a = self.agents.get_mut(&actor).expect("listed");
        a.wait_until = None;
        let space = a.walker.space;
        let from = a.walker.feet;
        let point = nav.nearest(space, to, 512.0).map(|(_, p)| p);
        (a.walker.speed, a.walk_pace) = (speed, None);
        let path = point.and_then(|p| a.walker.go_to(nav, Place { space, pos: p }, &params).ok()).map(|path| path.length);
        match (point, path) {
            (Some(p), Some(length)) => {
                a.activity = Activity::Travelling;
                a.trip_started = now;
                a.trip_from = a.walker.travelled;
                a.search = Some(Search::Going { owner: about, to: p, since: now, centre, alarm, points });
                self.log(game, actor, AiEvent::Search { about, to: p, away: goal::dist(from, p), length });
            }
            _ => {
                a.walker.follower = None;
                a.activity = Activity::There;
                a.search = Some(Search::Looking { owner: about, at: from, until: now + LOOK_SECONDS, centre, alarm, points });
                self.log(game, actor, AiEvent::SearchLook { at: from, seconds: LOOK_SECONDS as f32 });
            }
        }
    }

    /// Run towards `target`, at `at`: to the point `stop` units short of it on the line
    /// from the target to the actor (the navmesh point nearest that), so that it ends
    /// within attacking distance and not against the target's body [design; owner
    /// 2026-10-11: a dog gets into range and bites, it does not push the player around].
    /// Without a way there the actor holds its ground.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn close_in(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, target: FormId, at: [f32; 3], away: f32, stop: f32) {
        let params = self.path_params(game, actor);
        let aiming = game.awareness.aiming.contains(&actor);
        let own = self.alert_speeds(actor);
        let speed = if aiming { own.walk } else { own.run * game.awareness.pace_of(actor) };
        let now = game.time;
        let a = self.agents.get_mut(&actor).expect("listed");
        let space = a.walker.space;
        // A target on the move: where it will be shortly (`lead`).
        let drift = match a.search {
            Some(Search::Closing { target: t, drift, .. }) if t == target => drift.on(at, now),
            _ => Drift::begin(at, now),
        };
        // Led only while it has ground to make up: at its distance it keeps there.
        let ahead = if away > stop { lead(at, drift.speed) } else { at };
        let back = [a.walker.feet[0] - ahead[0], a.walker.feet[1] - ahead[1]];
        let len = back[0].hypot(back[1]).max(1e-3);
        let short = (stop * STOP_SHORT).min(len);
        let goal = [ahead[0] + back[0] / len * short, ahead[1] + back[1] / len * short, at[2]];
        // No way found to this target a moment ago: hold, without searching again.
        let retry_later = a.no_way_to.is_some_and(|(t, until, _)| t == target && now < until);
        let point = if retry_later { None } else { nav.nearest(space, goal, 512.0).map(|(_, p)| p) };
        (a.walker.speed, a.walk_pace) = (speed, None);
        let path = point.and_then(|p| a.walker.go_to(nav, Place { space, pos: p }, &params).ok()).map(|path| path.length);
        if !retry_later {
            a.no_way_to = match (path, a.no_way_to) {
                (Some(_), _) => None,
                // Failed again for the same target: wait twice as long.
                (None, Some((t, _, wait))) if t == target => Some((target, now + (wait * 2.0).min(NO_WAY_RETRY_MAX), (wait * 2.0).min(NO_WAY_RETRY_MAX))),
                (None, _) => Some((target, now + NO_WAY_RETRY_SECONDS, NO_WAY_RETRY_SECONDS)),
            };
        }
        let first = !matches!(a.search, Some(Search::Closing { target: t, .. }) if t == target);
        match (point, path) {
            (Some(p), Some(length)) => {
                a.activity = Activity::Travelling;
                a.trip_started = now;
                a.trip_from = a.walker.travelled;
                a.search = Some(Search::Closing { target, to: at, since: now, drift });
                if first {
                    self.log(game, actor, AiEvent::Close { target, to: p, away, length, aiming });
                }
            }
            _ => {
                a.walker.follower = None;
                a.activity = Activity::There;
                if a.search != Some(Search::Holding { target }) {
                    a.search = Some(Search::Holding { target });
                    self.log(game, actor, AiEvent::Hold { target, away });
                }
            }
        }
    }

    /// How fast `actor` goes to `post`: running if the post asks for it, walking while
    /// it aims down its sights (module notes).
    fn post_speed(&self, game: &Game, actor: FormId, post: Post) -> f32 {
        let own = self.alert_speeds(actor);
        if post.run && !game.awareness.aiming.contains(&actor) { own.run * game.awareness.pace_of(actor) } else { own.walk }
    }

    /// Go to the combat AI's `post`, or stand there facing `target` (at `at`). Returns
    /// false when the post is dropped: there is no way to it, or the walk there ended
    /// [`POST_ARRIVAL`] or more from it. The caller then holds or closes in; there is no
    /// retry here, the combat AI sees the post gone and chooses again.
    #[allow(clippy::too_many_arguments)]
    fn post_step(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, target: FormId, at: [f32; 3], post: Post, dt: f32) -> bool {
        let now = game.time;
        let a = &self.agents[&actor];
        let space = a.walker.space;
        match a.search {
            Some(Search::Posted { kind, to, since, .. }) if kind == post.kind && flat(to, post.at) < POST_SLACK => {
                let stopped = a.activity != Activity::Travelling;
                if stopped || now - since > FLEE_WALK_SECONDS {
                    let a = self.agents.get_mut(&actor).expect("listed");
                    a.walker.follower = None;
                    a.activity = Activity::There;
                    let feet = a.walker.feet;
                    let off = flat(feet, post.at);
                    // A door it was waiting at stays shut for all it knows: it did not pass.
                    if let Some(door) = a.waiting_at.take() {
                        self.log(game, actor, AiEvent::DoorWaitCancelled { door: FormId(door) });
                    }
                    let a = self.agents.get_mut(&actor).expect("listed");
                    if off >= POST_ARRIVAL {
                        a.search = None;
                        game.awareness.tactics.posts.remove(&actor);
                        let reason = if stopped { PostFailure::StoppedShort } else { PostFailure::TimedOut };
                        self.log(game, actor, AiEvent::PostFailed { post: kind, target, at: post.at, feet, off, elapsed: (now - since) as f32, reason });
                        return false;
                    }
                    a.search = Some(Search::AtPost { kind, target, at: post.at });
                    self.log(game, actor, AiEvent::AtPost { post: kind, target, off, at: post.at, feet });
                } else {
                    // Sights raised or lowered on the way change its pace there.
                    let speed = self.post_speed(game, actor, post);
                    let a = self.agents.get_mut(&actor).expect("listed");
                    if a.walker.speed != speed {
                        (a.walker.speed, a.walk_pace) = (speed, None);
                        let aiming = game.awareness.aiming.contains(&actor);
                        self.log(game, actor, AiEvent::Pace { target, aiming });
                    }
                }
                return true;
            }
            // Moved off it (shoved, say), it goes back: the way below ends here or fails.
            Some(Search::AtPost { kind, at: here, .. }) if kind == post.kind && flat(here, post.at) < POST_SLACK && flat(a.walker.feet, post.at) < POST_RETAINED => {
                self.combat_face(game, actor, Place { space, pos: at }, dt);
                return true;
            }
            _ => {}
        }
        let params = self.path_params(game, actor);
        let speed = self.post_speed(game, actor, post);
        let a = self.agents.get_mut(&actor).expect("listed");
        let from = a.walker.feet;
        let point = nav.nearest(space, post.at, 256.0).map(|(_, p)| p);
        (a.walker.speed, a.walk_pace) = (speed, None);
        let path = point.and_then(|p| a.walker.go_to(nav, Place { space, pos: p }, &params).ok()).map(|path| path.length);
        match (point, path) {
            (Some(p), Some(length)) => {
                a.activity = Activity::Travelling;
                a.trip_started = now;
                a.trip_from = a.walker.travelled;
                a.search = Some(Search::Posted { kind: post.kind, target, to: post.at, since: now });
                self.log(game, actor, AiEvent::Post { post: post.kind, target, to: p, away: flat(from, p), length });
                true
            }
            _ => {
                game.awareness.tactics.posts.remove(&actor);
                self.log(game, actor, AiEvent::NoPost { post: post.kind });
                false
            }
        }
    }

    /// One update of a fleeing actor (module notes).
    fn flee_step(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, flight: Flight, dt: f32) {
        let now = game.time;
        // Where the one it flees is now (else where it was when the flight began).
        let threat = game.awareness.get(actor).and_then(|a| a.targets.get(&flight.from)).map_or(flight.from_at, |t| t.at);
        let a = &self.agents[&actor];
        let space = a.walker.space;
        match a.search {
            Some(Search::Fleeing { from, to, since }) if from == flight.from => {
                if a.activity != Activity::Travelling || now - since > FLEE_WALK_SECONDS {
                    let a = self.agents.get_mut(&actor).expect("listed");
                    a.walker.follower = None;
                    a.activity = Activity::There;
                    let feet = a.walker.feet;
                    a.search = Some(Search::Fled { from, at: feet });
                    if let Some(f) = game.awareness.tactics.flights.get_mut(&actor) {
                        f.arrived = Some(now);
                    }
                    let hides = game.sneaks(actor);
                    self.log(game, actor, AiEvent::FleeArrived { from, seconds: (now - since) as f32, off: flat(feet, to), hides });
                }
            }
            // At its spot until the combat AI has it flee again (a new flight: not arrived).
            Some(Search::Fled { from, .. }) if from == flight.from && flight.arrived.is_some() => {
                self.face(game, actor, Place { space, pos: threat }, dt);
            }
            _ => self.flee_from(game, nav, actor, flight.from, threat),
        }
    }

    /// Choose a flee spot away from `threat` and set off there, running (module notes).
    pub(crate) fn flee_from(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, from: FormId, threat: [f32; 3]) {
        // Spectators also enter here while Normal, outside alert_step's takeover.
        self.interrupt_sandbox(game, actor);
        let params = self.path_params(game, actor);
        let gs = game.group_settings();
        let now = game.time;
        let speed = self.alert_speeds(actor).run * game.awareness.pace_of(actor);
        let a = self.agents.get_mut(&actor).expect("listed");
        let (space, feet) = (a.walker.space, a.walker.feet);
        a.wait_until = None;
        (a.walker.speed, a.walk_pace) = (speed, None);
        let mut tries: Vec<(Place, FleeBy)> = Vec::new();
        // 1. A door that leads elsewhere, not nearer the threat than the actor is.
        let mut doors: Vec<(f32, Place, FleeBy)> = Vec::new();
        for t in nav.triangles_near(space, feet[0], feet[1], gs.flee_max_door) {
            for e in nav.doors.get(&t).map_or(&[][..], |v| v.as_slice()) {
                let c = nav.centre(t);
                let d = flat(c, feet);
                if d > gs.flee_max_door || flat(c, threat) < flat(feet, threat) {
                    continue;
                }
                if let Some(to_space) = nav.space_of_tri(e.to) {
                    doors.push((d, Place { space: to_space, pos: e.arrival }, FleeBy::Door { door: FormId(e.door) }));
                }
            }
        }
        doors.sort_by(|x, y| x.0.total_cmp(&y.0));
        tries.extend(doors.into_iter().take(3).map(|d| (d.1, d.2)));
        // 2. Cover away from the threat, between the flee cover distances from it.
        let mut cover: Vec<(f32, [f32; 3])> = nav
            .cover_edges_near(space, threat, gs.flee_max_cover)
            .into_iter()
            .filter(|e| {
                let d = flat(e.mid, threat);
                d >= gs.flee_min_cover && d >= flat(feet, threat) && e.faces(threat)
            })
            .map(|e| (flat(e.mid, feet), e.behind(COVER_STANDOFF)))
            .collect();
        cover.sort_by(|x, y| x.0.total_cmp(&y.0));
        tries.extend(cover.into_iter().take(4).filter_map(|(_, p)| nav.nearest(space, p, 128.0)).map(|(_, p)| (Place { space, pos: p }, FleeBy::Cover)));
        // 3. Straight away from it, turning when the navmesh ends there.
        let (dx, dy) = (feet[0] - threat[0], feet[1] - threat[1]);
        let base = if dx.hypot(dy) > 1.0 { dy.atan2(dx) } else { 0.0 };
        for turn in [0.0f32, 30.0, -30.0, 60.0, -60.0, 90.0, -90.0] {
            let h = base + turn.to_radians();
            let p = [feet[0] + gs.flee_normal * h.cos(), feet[1] + gs.flee_normal * h.sin(), feet[2]];
            if let Some((_, p)) = nav.nearest(space, p, 512.0) {
                if flat(p, threat) > flat(feet, threat) {
                    tries.push((Place { space, pos: p }, FleeBy::Away));
                }
            }
        }
        for (place, how) in tries {
            let Ok(length) = a.walker.go_to(nav, place, &params).map(|p| p.length) else { continue };
            a.activity = Activity::Travelling;
            a.trip_started = now;
            a.trip_from = a.walker.travelled;
            a.search = Some(Search::Fleeing { from, to: place.pos, since: now });
            if let Some(f) = game.awareness.tactics.flights.get_mut(&actor) {
                f.spot = Some((place.pos, how));
            }
            let away = if place.space == space { flat(place.pos, threat) } else { f32::INFINITY };
            self.log(game, actor, AiEvent::Flee { from, how, to: place.pos, away, length });
            return;
        }
        // Nowhere to go: it stands, cornered.
        a.walker.follower = None;
        a.activity = Activity::There;
        a.search = Some(Search::Fled { from, at: feet });
        if let Some(f) = game.awareness.tactics.flights.get_mut(&actor) {
            f.arrived = Some(now);
        }
        let hides = game.sneaks(actor);
        self.log(game, actor, AiEvent::Cornered { from, hides });
    }

    /// The search or the stand ends: the actor goes back to its package.
    pub(crate) fn search_over(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, why: &'static str) {
        let a = self.agents.get_mut(&actor).expect("listed");
        a.search = None;
        if a.activity == Activity::Travelling {
            a.walker.follower = None;
            a.activity = Activity::There;
        }
        self.log(game, actor, AiEvent::SearchOver { why });
        self.back_to_package(game, nav, actor, |package, away| AiEvent::SearchBack { package, away });
    }
}
