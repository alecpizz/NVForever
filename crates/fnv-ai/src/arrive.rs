//! Actors already at their package's place when the player arrives (issue #1208).
//!
//! After a fast travel or a load door the original shows the destination's actors where
//! their current package has them: Boone at his post by the dinosaur when the player
//! fast travels to Novac at 5am, Cass already drinking at the Mojave Outpost's bar on
//! entering, the Goodsprings settler already seated in the Prospector Saloon [observed,
//! owner, issue #1208]. Ours walked there in front of the player: an actor of a cell
//! that was not loaded joined the AI at its editor place (or where the low process had
//! left it) and only then set off.
//!
//! So here the engine, while the loading screen of the player's own trip (a load door,
//! a fast travel; not a script's MoveTo) is up, hands the AI each actor near the
//! destination that was not in the loaded area shortly before the trip
//! ([`Ai::arrive`]); the AI takes up the actor's current package and, instead of
//! walking, puts it where the walk ends: at the package's place, on the furniture the
//! package or its sandbox gives it, at the patrol point it sets off for (one that goes
//! round: the nearest, as when its package begins) or at a patrol's last point (one that
//! goes once, its waits on the way over) [hypothesis: the original's actors were seen at
//! the place, not part-way; whether it puts them exactly at the end or somewhere within
//! the package's radius is for the observer, docs/handoff/observer-checks.md].
//!
//! Only a walk that starts near the player is skipped: in the loaded area, the cell
//! buffer or an interior behind a door of the loaded cells (the engine says which, the
//! `near` of [`Ai::arrive`]): Boone, in his room behind the motel's door, to the
//! dinosaur in the street. An actor farther away comes as the low process brings it,
//! stopping at the loaded area's edge ([`crate::process`]: Malcolm Holmes). Actors the
//! player could see shortly before the trip are never moved: the engine leaves them out
//! (a short fast travel inside the loaded area, or out of a house and straight back,
//! moves nobody [design, issue #1208]).
//!
//! Not put anywhere: an actor that is dead, unconscious, restrained, rooted, with its
//! AI off, not in the world, in a combat state (Alert, Combat, Lost) or just put
//! somewhere by a script (MoveTo, [`SCRIPT_MOVE_HOLD`]); one whose
//! package's place is the player or another actor (a hunter sent after the player would
//! land beside them); one following, escorting, searching, fleeing or in a
//! conversation (those walks depend on someone else, and go on as they do now).

use crate::carry::{Activity, Ai, Passed, Talking};
use crate::goal::{self, Goal};
use fnv_game::game::Game;
use fnv_game::package::{LocationKind, PackageDef, PackageType};
use fnv_nav::follow::heading_of;
use fnv_nav::{NavGraph, Place};
use fnv_sim::FormId;
use serde::Serialize;

/// At most this many walks are skipped for one actor on arrival: to its package's place,
/// then a sandbox's own walk from there to the furniture or marker it chose [our choice;
/// a bound so that a package that keeps sending its actor on cannot hold the loading].
pub const ARRIVE_WALKS: usize = 3;

/// An actor a script put somewhere (MoveTo, [`Ai::relocate`]) less than this long before
/// (`Game::time`, seconds, which stands still under a loading screen) stays where it was
/// put [design: a scene's MoveTo just before the player comes in is the script's word].
pub const SCRIPT_MOVE_HOLD: f64 = 10.0;

/// How far from a walk's end the navmesh is looked for to stand the actor on, game units
/// [our choice: the path's end is on the navmesh already, this only finds its triangle].
const ARRIVE_NAV_REACH: f32 = 64.0;

/// What [`Ai::arrive`] did with an actor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Arrival {
    /// Its package, where it was and where it is now.
    pub package: FormId,
    pub from: Place,
    pub to: Place,
    /// The walks skipped, and how long they were together (straight lines between the
    /// ends of those within one space), game units.
    pub walks: usize,
    pub skipped: f32,
    /// The furniture it now uses (the engine seats it at once), if any.
    pub furniture: Option<FormId>,
}

impl Ai {
    /// `actor` is near the player as the player arrives somewhere (module notes): it
    /// takes up its current package, chosen again for the hour it is now, and is put
    /// where that package's walk would take it, on the furniture it would use there.
    /// `near` says whether a place is near the player (the loaded area, the cell buffer,
    /// an interior behind a door of the loaded cells): only a walk starting there is
    /// skipped. Returns what was done, or `None` when it was not put anywhere (already
    /// there, nowhere to go, or one of those left alone).
    pub fn arrive(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, near: &dyn Fn(Place) -> bool) -> Option<Arrival> {
        let a = self.agents.get(&actor)?;
        let held = game.extra.get(&actor).is_some_and(|e| e.dead || e.out_cold() || e.restrained);
        let put = self.script_moved.get(&actor).is_some_and(|t| game.time - t < SCRIPT_MOVE_HOLD);
        if a.rooted || held || put || !game.present(actor) || game.ai_off(actor) || game.awareness.state(actor).in_combat() {
            return None;
        }
        let from = Place { space: a.walker.space, pos: a.walker.feet };
        if !near(from) {
            return None;
        }
        // The hour may have moved on in the trip (a fast travel's hours): the package of
        // now, not of the last periodic choice.
        let package = game.update_package(actor)?;
        if game.package_def(&package).is_none_or(|d| placed_on_an_actor(game, actor, d)) {
            return None;
        }
        let (mut walks, mut skipped) = (0, 0.0);
        let mut limit = ARRIVE_WALKS;
        while walks < limit {
            // Takes up the package (its goal and walk), as an update in a menu does.
            self.step_actor(game, nav, actor, 0.0, &mut Passed, true);
            let a = &self.agents[&actor];
            // At the place of a sandbox: what it does there, chosen now.
            if a.activity == Activity::There && a.sandbox.as_ref().is_some_and(|s| s.pending.is_none()) {
                self.sandbox_step(game, nav, actor);
            }
            let Some(to) = self.skippable(actor) else { break };
            // Only a walk that starts near the player (module notes).
            let at = Place { space: self.agents[&actor].walker.space, pos: self.agents[&actor].walker.feet };
            if !near(at) {
                break;
            }
            let a = self.agents.get_mut(&actor).expect("listed");
            let w = &mut a.walker;
            // Facing the way the walk's last stretch went.
            let path = w.follower.as_ref().and_then(|f| f.path.legs.last()).map(|l| l.points.clone()).unwrap_or_default();
            let heading = match path.as_slice() {
                [.., p, q] if goal::dist(*p, *q) > 1.0 => heading_of([q[0] - p[0], q[1] - p[1]]),
                _ => w.heading,
            };
            if w.space == to.space {
                skipped += goal::dist(w.feet, to.pos);
            }
            w.space = to.space;
            w.feet = to.pos;
            w.heading = heading;
            w.tri = nav.nearest(to.space, to.pos, ARRIVE_NAV_REACH).map_or(fnv_nav::graph::NONE, |(t, _)| t);
            (w.turning, w.moving) = (false, false);
            let cell = goal::cell_of_place(game.world.placed, &to);
            let _ = game.set_actor_place(actor, cell, to.pos, heading);
            walks += 1;
            let patrol = match &a.goal {
                Some(Goal::Route(r)) => Some((r.repeatable, r.points.len())),
                _ => None,
            };
            self.arrived(game, nav, actor);
            match patrol {
                // A patrol that goes round is at the point it set off for (the nearest,
                // as when its package begins) and walks on from there in sight.
                Some((true, _)) => break,
                // One that goes once has been walked to its end (Boone's night patrol to
                // the dinosaur's mouth, through his room's and the gift shop's doors):
                // the waits at the points on the way are over too.
                Some((false, n)) => {
                    limit = limit.max(n + 1);
                    if self.agents.get_mut(&actor).expect("listed").wait_until.take().is_some() {
                        self.patrol_on(game, nav, actor);
                    }
                }
                None => {}
            }
        }
        if walks == 0 {
            return None;
        }
        let a = &self.agents[&actor];
        let to = Place { space: a.walker.space, pos: a.walker.feet };
        Some(Arrival { package: a.package?, from, to, walks, skipped, furniture: a.using })
    }

    /// A script moved the clock by more than an hour (`fnv_game::game::Game::take_hour_jump`,
    /// issue #2353: VCG01's `set GameHour to 8` after the player had already left Doc
    /// Mitchell's house, Goodsprings placed for the chargen clock's early morning): handled
    /// like the end of a wait. Every actor the periodic choice covers chooses its package
    /// now (`Game::choose_packages_now`), and every actor here that the player cannot see
    /// is put where its new package's walk ends, as on arrival ([`Ai::arrive`] with
    /// `unseen` for its `near`: only a walk starting out of sight is skipped, so one that
    /// lands in view walks on from there in sight). Actors in view are not moved: they
    /// set off on their new packages in the next updates. [design, #2353; what the
    /// original does after a script's GameHour write is not described by any source read,
    /// docs/spec-notes/packages.md §9.] Returns those put somewhere, in FormID order.
    pub fn hour_jumped(&mut self, game: &mut Game, nav: &NavGraph, unseen: &dyn Fn(Place) -> bool) -> Vec<(FormId, Arrival)> {
        game.choose_packages_now();
        let actors: Vec<FormId> = self.agents.keys().copied().collect();
        actors.into_iter().filter_map(|actor| self.arrive(game, nav, actor, unseen).map(|a| (actor, a))).collect()
    }

    /// Where `actor`'s walk ends, when it is one [`Ai::arrive`] skips: a walk of its
    /// package to its place (or a patrol's point), or of its sandbox to what it chose.
    fn skippable(&self, actor: FormId) -> Option<Place> {
        let a = &self.agents[&actor];
        let own = a.package.is_some()
            && a.activity == Activity::Travelling
            && a.chat.is_none()
            && a.search.is_none()
            && a.errand.is_none()
            && a.weapon_order.is_none()
            && a.backing.is_none()
            && matches!(a.talk, None | Some(Talking::ToWait));
        let kind = matches!(a.goal, Some(Goal::Near { .. } | Goal::Inside { .. } | Goal::Route(_) | Goal::Talk(_))) || a.sandbox.as_ref().is_some_and(|s| s.pending.is_some());
        if !own || !kind {
            return None;
        }
        a.walker.follower.as_ref().and_then(|f| f.destination())
    }
}

/// Whether `def` has its actor stay near the player or another actor (its location
/// names one, directly or as the actor's linked reference; a Guard package keeping near
/// what it guards): its place moves with that actor, so [`Ai::arrive`] does not put it
/// there (a hunter sent after the player would land beside them).
fn placed_on_an_actor(game: &Game, actor: FormId, def: &PackageDef) -> bool {
    let refs = game.world.placed;
    let is_actor = |r: FormId| {
        r == FormId::PLAYER_REF
            || refs.get(r).is_some_and(|p| game.data.actors.contains_key(&p.base))
            || game.world.peek(r).is_some_and(|s| game.data.actors.contains_key(&s.base))
    };
    if def.ty == PackageType::Guard && def.remain_near_reference() {
        return true;
    }
    match def.location.map(|l| l.kind) {
        Some(LocationKind::NearReference(Some(r))) => is_actor(r),
        Some(LocationKind::NearLinked) => game.linked_ref(actor).is_some_and(is_actor),
        _ => false,
    }
}
