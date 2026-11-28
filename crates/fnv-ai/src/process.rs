//! Process levels: how much of the AI an actor gets by where it is (owner's playtest
//! 2026-10-11: Fiends near Camp McCarran were heard talking from Crimson Caravan).
//!
//! The original processes actors in levels [documented: GECK wiki (JIP)
//! GetActorsByProcessingLevel and GetActorProcessingLevel]:
//! - **High**: the actors of the loaded interior cell or of the exterior cells around
//!   (and including) the player's, uGridsToLoad (5 × 5 by default); "processed every
//!   frame";
//! - **Mid-High**: actors in exterior cells not loaded but visited recently and still
//!   in memory, "processed every ~18 seconds";
//! - **Mid-Low**: actors in interiors not visited but reached by doors from the loaded
//!   cells ("unknown at this time");
//! - **Low**: everyone else, "every ~120 seconds".
//!
//! An actor whose base has "No Low Level Processing" "does not update its AI unless it
//! is in the loaded area of the player" [documented: GECK wiki Category:NPC]; and with
//! SetSceneIsComplex "low and middle-low process levels do not update … they will not go
//! through load doors" [documented: GECK wiki SetSceneIsComplex], so outside it they do.
//!
//! Here the engine says where each actor is ([`Ai::set_level`], a [`ProcessLevel`]); the
//! AI keeps three levels [design, after the original's]:
//! - **high** (the loaded area): everything (packages, walks, sandboxes, combat states,
//!   Dialogue packages, hellos and random conversations), every update;
//! - **low**, out of the loaded area: an update now and then, in which it takes up its
//!   current package and walks on as far as the time owed since the last takes it, on
//!   the navmesh (doors as if open), as in an hour passed in the Wait menu
//!   ([`crate::Ai::pass_time`]): nothing that needs the player near: no sandbox
//!   behaviour, no combat state, no hello, no random conversation; a Dialogue package
//!   without a wait location only takes the actor towards its target, the talking waits
//!   for the high process.
//!   How often depends on where:
//!   - [`ProcessLevel::MidHigh`], cells left recently and still in the cell buffer (the
//!     engine keeps it: `uExterior Cell Buffer`, `uInterior Cell Buffer`): every
//!     [`LOW_PROCESS_SECONDS`];
//!   - [`ProcessLevel::Low`], everyone else: every [`FAR_PROCESS_SECONDS`];
//!   - [`ProcessLevel::Coming`], whatever the cell, an actor whose walk leads into the
//!     loaded area (the engine looks at the rest of its path): every frame, so it
//!     arrives when it would have walking in sight.
//!
//!   At each such update, and once in each hour of a wait, the actor's script runs its
//!   GameMode block where it is ([`Game::run_far_frame`], only for a block that tests
//!   "not in the player's cell") before the walk: a MoveTo or an
//!   EvaluatePackage there is what the update goes on from [hypothesis from the data,
//!   observer check 135: Beyond the Beef's Chauncey leaves his Must Complete patrol only
//!   through his own GameMode block, written for when the player is in another cell,
//!   issue #2445].
//!
//!   The original's Mid-Low (interiors reached by doors from the loaded cells, never
//!   visited, "unknown" rate) is [`ProcessLevel::Low`] here: the engine hands the AI
//!   the actors of those interiors with the loaded cells' own (Boone leaves his room
//!   for the dinosaur's mouth at 21:00 without the player having been in; owner
//!   2026-10-26). Actors farther away join when their cell loads, or a script moves
//!   them.
//!
//! How the original carries an actor at the Low level to the player, watched with the AI
//! observer [observed 2026-10-24: Malcolm Holmes after the first star cap,
//! `tools/capture/observe/holmes_far_travel.campaign.json` and `holmes_far_moving`,
//! docs/spec-notes/ai-observer.md §13]: he stood still between jumps once per game hour
//! (one [`FAR_PROCESS_SECONDS`] at TimeScale 30) of 9,050–10,170 units along a route, not
//! a straight line: about a walk's worth (86.8 units/s), though his package has Always
//! Run. The jump that would have taken him into the loaded area stopped short at its
//! edge (5,330 units), and from there he ran in the high process. So here:
//! - at the Low level an actor walks, whatever its package says [hypothesis: one actor
//!   seen];
//! - a low-process walk stops where it comes into the loaded area ([`Ai::set_loaded`]),
//!   dropping the rest of the time owed;
//! - a Dialogue package without a wait location goes towards its target out of the high
//!   process too, on the route planned when it set off (`Ai::talk_step_far`); one with a
//!   wait location keeps its actor there [hypothesis: Ringo stays in the gas station].
//!
//! An actor whose base has "No Low Level Processing" gets no update at all out of the
//! loaded area, not even for hours passed in the Wait menu.
//!
//! Going low ends what needs the loaded area: a random conversation it walks to or waits
//! for (the other one goes back to its package), a search. Low actors' updates are
//! spread over the interval (each starts with a share of it owed, by its FormID), so a
//! crowd leaving the loaded area is not updated in one frame.

use crate::carry::{Activity, Ai, Mover};
use crate::social::Chat;
use fnv_game::game::Game;
use fnv_nav::{NavGraph, Space};
use fnv_sim::FormId;
use serde::Serialize;

/// An actor out of the loaded area but in the cell buffer is updated this often,
/// seconds [documented: GECK wiki (JIP) GetActorProcessingLevel, Mid-High "processed
/// every ~18 seconds"].
pub const LOW_PROCESS_SECONDS: f32 = 18.0;

/// An actor neither in the loaded area nor in the cell buffer is updated this often,
/// seconds [documented: GECK wiki (JIP) GetActorProcessingLevel, Low "processed every
/// ~120 seconds"].
pub const FAR_PROCESS_SECONDS: f32 = 120.0;

/// Where an actor is, for its process level (the engine tells, [`Ai::set_level`])
/// [documented: GECK wiki (JIP) GetActorsByProcessingLevel].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProcessLevel {
    /// In the loaded interior, or the exterior cells loaded around the player.
    High,
    /// In a cell not loaded but "recently visited by the player and still buffered in
    /// memory".
    MidHigh,
    /// In a cell "neither currently loaded nor buffered".
    Low,
    /// Out of the loaded area but on its way into it (the rest of its path ends there):
    /// moved every frame, as in the low process (no sandbox, combat state or
    /// conversation; doors as if open), so it comes through the door when it would
    /// [design, owner's playtest list 4 item 13: Ringo, sent from the gas station to the
    /// Powder Ganger fight by RingoTravelPackage, came out only after it ended; the
    /// original's rule for actors heading to the player's area is not documented].
    Coming,
}

/// A low-process update walks the time owed in steps this long, seconds [our choice: as
/// coarse as the walker's path following allows; Wait-menu hours use 0.25].
pub const LOW_STEP_SECONDS: f32 = 0.5;

/// What the process levels did, for the engine's summary.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProcessStats {
    /// Actors that left the high process, and came back to it.
    pub left: usize,
    pub came_back: usize,
    /// Low-process updates given, and the walking steps in them.
    pub low_updates: usize,
    pub low_steps: usize,
    /// The most actors in the high process at once, and out of it.
    pub most_high: usize,
    pub most_low: usize,
    /// Random conversations and searches given up because their actor left the loaded
    /// area.
    pub dropped: usize,
    /// Times an actor with "No Low Level Processing" left the loaded area (it is not
    /// updated out of it).
    pub frozen: usize,
    /// Times an actor fell to the Low level (out of the cell buffer), the low-process
    /// updates given at that level, and the most actors at it at once.
    pub fell_far: usize,
    pub far_updates: usize,
    pub most_far: usize,
    /// Times an actor out of the loaded area set off into it ([`ProcessLevel::Coming`]),
    /// and the updates given to such actors.
    pub coming: usize,
    pub coming_updates: usize,
    /// Low-process walks cut short where they came into the loaded area.
    pub reached_loaded: usize,
    /// GameMode runs of the scripts of actors out of the loaded cells
    /// ([`Game::run_far_frame`]): at their low-process updates and in each hour of a wait.
    pub far_scripts: usize,
}

/// The loaded area, as the engine has it ([`Ai::set_loaded`]): an interior, or the
/// exterior cells loaded around the player in one worldspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedArea {
    pub space: Space,
    /// The loaded cells (grid X, Y) of an exterior; empty for an interior.
    pub cells: std::collections::BTreeSet<(i32, i32)>,
}

impl LoadedArea {
    /// Whether a place is in the loaded area.
    pub fn holds(&self, space: Space, feet: [f32; 3]) -> bool {
        space == self.space && (matches!(space, Space::Interior(_)) || self.cells.contains(&fnv_world::cell_of(feet[0], feet[1])))
    }
}

/// [`crate::Ai::update_low`]'s mover: the walker's own steps, every door open.
struct Low;

impl Mover for Low {}

impl Ai {
    /// The engine says what is loaded, so that a low-process walk stops where it comes
    /// into the loaded area ([`Ai::low_update`]). `None`: nothing is (headless runs).
    pub fn set_loaded(&mut self, loaded: Option<LoadedArea>) {
        self.loaded = loaded;
    }

    /// The loaded area last given by [`Ai::set_loaded`].
    pub fn loaded(&self) -> Option<&LoadedArea> {
        self.loaded.as_ref()
    }

    /// Whether `actor` is in the high process (an actor the AI does not look after is).
    pub fn is_high(&self, actor: FormId) -> bool {
        !self.low.contains_key(&actor)
    }

    /// How many of the actors the AI looks after are in the high process, and out of it.
    pub fn process_counts(&self) -> (usize, usize) {
        let low = self.agents.keys().filter(|a| self.low.contains_key(a)).count();
        (self.agents.len() - low, low)
    }

    /// The process level of `actor` (an actor the AI does not look after is high).
    pub fn level(&self, actor: FormId) -> ProcessLevel {
        match (self.is_high(actor), self.far.contains(&actor), self.coming.contains(&actor)) {
            (true, _, _) => ProcessLevel::High,
            (false, _, true) => ProcessLevel::Coming,
            (false, false, false) => ProcessLevel::MidHigh,
            (false, true, false) => ProcessLevel::Low,
        }
    }

    /// An actor following another one (a Follow or Accompany package) out of the loaded
    /// area is processed at least as often as its leader, up to Mid-High [design,
    /// coordinator 2026-10-12: teleports (fast travel, MoveTo) can leave a follower
    /// beyond the cell buffer while its leader is near: Cheyenne after Sunny Smiles].
    fn with_leader(&self, actor: FormId, level: ProcessLevel) -> ProcessLevel {
        let Some(leader) = self.leader_of(actor) else { return level };
        match (level, self.level(leader)) {
            (ProcessLevel::Low, ProcessLevel::High | ProcessLevel::MidHigh | ProcessLevel::Coming) => ProcessLevel::MidHigh,
            _ => level,
        }
    }

    /// The one `actor` follows on a Follow or Accompany package, when the AI looks
    /// after it.
    fn leader_of(&self, actor: FormId) -> Option<FormId> {
        match self.agents.get(&actor).and_then(|a| a.goal.as_ref()) {
            Some(crate::goal::Goal::Follow(f)) if self.agents.contains_key(&f.target) => Some(f.target),
            _ => None,
        }
    }

    /// Whether `actor` follows one in the loaded area or coming into it: out of the
    /// loaded area such a follower comes in frame by frame ([`ProcessLevel::Coming`]) from
    /// beyond the cell buffer too, not in the far walk's jumps at a walk [design: as
    /// [`Ai::with_leader`]; smoke sunny-wells on main 8ec4062d: Cheyenne, beyond the
    /// buffer in the saloon, took about 20 game minutes to come out after Sunny Smiles,
    /// who had gone on to the wells].
    pub fn follows_near_leader(&self, actor: FormId) -> bool {
        self.leader_of(actor).is_some_and(|l| matches!(self.level(l), ProcessLevel::High | ProcessLevel::Coming))
    }

    /// How many of the actors the AI looks after are at the Low level.
    pub fn far_count(&self) -> usize {
        self.far.iter().filter(|a| self.agents.contains_key(a)).count()
    }

    /// Put `actor` at `level`. Returns whether its level changed. Between the two
    /// levels out of the loaded area only the rate changes, and the share of its
    /// interval it is owed.
    pub fn set_level(&mut self, game: &Game, actor: FormId, level: ProcessLevel) -> bool {
        let level = self.with_leader(actor, level);
        if !self.agents.contains_key(&actor) || level == self.level(actor) {
            return false;
        }
        self.set_high(game, actor, level == ProcessLevel::High);
        let was_far = self.far.contains(&actor);
        if level == ProcessLevel::Coming {
            self.coming.insert(actor);
            self.process_stats.coming += 1;
            // It walks in real time from where it is: no time owed is caught up at once.
            if let Some(Some(owed)) = self.low.get_mut(&actor) {
                *owed = 0.0;
            }
        } else {
            self.coming.remove(&actor);
        }
        if level == ProcessLevel::Low {
            self.far.insert(actor);
            self.process_stats.fell_far += 1;
        } else {
            self.far.remove(&actor);
        }
        // The share of its interval owed stays (updates stay spread over the interval).
        let rate = match (was_far, level) {
            (false, ProcessLevel::Low) => FAR_PROCESS_SECONDS / LOW_PROCESS_SECONDS,
            (true, ProcessLevel::MidHigh) => LOW_PROCESS_SECONDS / FAR_PROCESS_SECONDS,
            _ => 1.0,
        };
        if let Some(Some(owed)) = self.low.get_mut(&actor) {
            *owed *= rate;
        }
        true
    }

    /// Put `actor` in the high process (it is in the loaded area) or out of it (at the
    /// Mid-High level unless [`Ai::set_level`] puts it lower). Returns whether it went
    /// in or out.
    pub fn set_high(&mut self, game: &Game, actor: FormId, high: bool) -> bool {
        if !self.agents.contains_key(&actor) || high == self.is_high(actor) {
            return false;
        }
        if high {
            self.low.remove(&actor);
            self.far.remove(&actor);
            self.coming.remove(&actor);
            self.process_stats.came_back += 1;
            return true;
        }
        // Spread over the interval [design]: a share of it is owed already. "No Low Level
        // Processing": no update at all.
        let share = (actor.0.wrapping_mul(0x9E37_79B9) >> 8) as f32 / (1u32 << 24) as f32;
        let owed = (!game.no_low_level_processing(actor)).then_some(share * LOW_PROCESS_SECONDS);
        self.process_stats.frozen += usize::from(owed.is_none());
        self.low.insert(actor, owed);
        self.process_stats.left += 1;
        self.leave_loaded_area(actor);
        true
    }

    /// What needs the loaded area ends: a random conversation (the partner, or the
    /// starter walking up to it, goes back to its package at its next update), a search.
    fn leave_loaded_area(&mut self, actor: FormId) {
        let Some(a) = self.agents.get_mut(&actor) else { return };
        let chat = a.chat.take();
        let search = a.search.take();
        if search.is_some() && a.activity == Activity::Travelling {
            a.walker.follower = None;
            a.activity = Activity::None;
            // Its package is taken up afresh at its next update.
            a.package = None;
        }
        let partner = match chat {
            Some(Chat::Going { partner, .. }) => {
                if a.activity == Activity::Travelling {
                    a.walker.follower = None;
                    a.activity = Activity::None;
                    a.package = None;
                }
                Some(partner)
            }
            Some(Chat::Waiting { starter }) => Some(starter),
            Some(Chat::Talking { with }) => Some(with),
            None => None,
        };
        // The other one: a starter walking to it gives up when it finds nobody waiting
        // (`crate::social`); a partner waiting for it stops waiting.
        if let Some(p) = partner.and_then(|p| self.agents.get_mut(&p)) {
            if matches!(p.chat, Some(Chat::Waiting { starter }) if starter == actor) {
                p.chat = None;
            }
        }
        if chat.is_some() || search.is_some() {
            self.process_stats.dropped += 1;
        }
    }

    /// The low-process updates due after `dt` more seconds ([`Ai::update_moved`] calls
    /// this after the high-process update).
    pub(crate) fn update_low(&mut self, game: &mut Game, nav: &NavGraph, dt: f32) {
        let (high, low) = self.process_counts();
        self.process_stats.most_high = self.process_stats.most_high.max(high);
        self.process_stats.most_low = self.process_stats.most_low.max(low);
        self.process_stats.most_far = self.process_stats.most_far.max(self.far_count());
        let mut due = Vec::new();
        for (actor, owed) in self.low.iter_mut().filter_map(|(a, o)| Some((a, o.as_mut()?))) {
            *owed += dt;
            let every = if self.coming.contains(actor) {
                0.0
            } else if self.far.contains(actor) {
                FAR_PROCESS_SECONDS
            } else {
                LOW_PROCESS_SECONDS
            };
            if *owed >= every {
                due.push((*actor, std::mem::take(owed)));
            }
        }
        for (actor, owed) in due {
            self.process_stats.far_updates += usize::from(self.far.contains(&actor));
            self.process_stats.coming_updates += usize::from(self.coming.contains(&actor));
            self.low_update(game, nav, actor, owed);
        }
    }

    /// One low-process update of `actor` for `seconds` (module notes).
    fn low_update(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, seconds: f32) {
        self.process_stats.low_updates += 1;
        // Its script runs where it is, before it takes up its package: a MoveTo or an
        // EvaluatePackage there is what this update goes on from (`Game::run_far_frame`).
        self.process_stats.far_scripts += usize::from(game.run_far_frame(actor));
        // Put somewhere by it (MoveTo): it goes on from there now, and the engine still
        // hears of the move (`Ai::moved_by_scripts`).
        if game.teleported.contains(&actor) && self.agents.contains_key(&actor) {
            self.relocate(game, actor);
        }
        if !self.agents.contains_key(&actor) {
            return;
        }
        // Re-check the actor's package before walking a whole low-process interval so a
        // quest flag or schedule change that happened since the last periodic choice takes
        // effect before path planning (issue #3041) [design].
        if game.runs_packages() && game.present(actor) && !game.extra.get(&actor).is_some_and(|e| e.dead) && !game.ai_off(actor) {
            game.update_package(actor);
        }
        let mut left = seconds;
        let package = self.agents.get(&actor).and_then(|a| a.package);
        let far = self.far.contains(&actor);
        let walk = self.calm_speeds(actor).walk;
        while left > 0.0 {
            let step = LOW_STEP_SECONDS.min(left);
            // At the Low level it goes at a walk, also when its package runs (module notes).
            let kept = self.agents.get_mut(&actor).filter(|_| far).map(|a| {
                let kept = a.walker.speed;
                a.walker.speed = kept.min(walk);
                (kept, a.walker.speed)
            });
            self.step_actor(game, nav, actor, step, &mut Low, true);
            self.process_stats.low_steps += 1;
            left -= step;
            let Some(a) = self.agents.get_mut(&actor) else { break };
            // A speed the step set (a new walk) is the one kept.
            if let Some((kept, capped)) = kept {
                if a.walker.speed == capped {
                    a.walker.speed = kept;
                }
            }
            if a.activity != Activity::Travelling {
                break;
            }
            // Into the loaded area: the rest of the time owed is dropped, and the engine
            // puts it in the high process from there (module notes).
            if self.loaded.as_ref().is_some_and(|l| l.holds(a.walker.space, a.walker.feet)) {
                self.process_stats.reached_loaded += 1;
                break;
            }
            // A walk just begun (a new package) waits for the next update with the rest of
            // the time owed, so the engine can see where it leads first: into the loaded
            // area, it is walked frame by frame ([`ProcessLevel::Coming`]) [design, owner's
            // playtest list 4 item 13: Ringo's whole trip out of the gas station was walked
            // in one update].
            if a.package != package {
                if let Some(Some(owed)) = self.low.get_mut(&actor) {
                    *owed += left;
                }
                break;
            }
        }
    }
}
