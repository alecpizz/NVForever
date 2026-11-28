//! AI package choice (Layer 6): which package of its list an actor runs, and when it
//! chooses again. Facts and their sources: docs/spec-notes/packages.md §3 (documented)
//! and §4 (observed in the game); the design: docs/design/actor-behaviour.md.
//!
//! - An actor's packages are tried top to bottom; the first valid one runs
//!   ([`Game::select_package`], [`Game::check_package`]).
//! - A script choosing again (`EvaluatePackage`) is [`Game::evaluate_package`]; the
//!   game's own periodic choice is [`Game::update_package`], which lets a Must Complete
//!   package finish first ([`Game::package_goal_met`]).
//! - With [`crate::game::GameConfig::package_interval`] set, [`Game::tick`] makes the
//!   periodic choice for the actors around the player and runs the packages' begin,
//!   end and change scripts. What a package then does (travel, sandbox, …) is the next
//!   layer's work.

use crate::functions;
use crate::game::Game;
use crate::package::{flags, Clock, Day, PackageCheck, PackageDef, TargetKind};
use crate::trace::EventKind;
use fnv_sim::FormId;
use std::collections::BTreeSet;

/// A script's write that moves GameHour by more than this many game hours on the clock
/// face ([`clock_face_hours`]) is a jump of the clock: the engine has every actor choose
/// its package now and puts the actors the player cannot see where their packages have
/// them, as at the end of a wait ([`crate::game::Game::take_hour_jump`]) [design, #2353:
/// the original's handling of a script's GameHour write is not described by any source
/// read, docs/spec-notes/packages.md §9; one hour is more than the periodic choice's
/// ~10 s lag stands for at any TimeScale in use, and less than a schedule's shortest
/// window].
pub const SCRIPT_HOUR_JUMP: f32 = 1.0;

/// How far apart two hours of the day are on the clock face, 0 to 12: 23:00 and 01:00
/// are two hours apart, whichever way the clock went.
pub fn clock_face_hours(from: f32, to: f32) -> f32 {
    let d = (to - from).rem_euclid(24.0);
    d.min(24.0 - d)
}

/// What the layer that carries packages out (`fnv_ai`) has an actor do within its
/// current package, as it last told ([`Game::set_ai_phase`]); GetCurrentAIProcedure reads
/// it (`functions::ai_state`). `None` (the field unset): that layer does not run the
/// actor now (an actor out of the high process, or no such layer, as headless). Not
/// saved: told again on the actor's next update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiPhase {
    /// On its way to the package's location.
    Travelling,
    /// At the package's location, or the package has none to go to.
    There,
    /// In the area of a Sandbox package: what it does there.
    Sandbox(SandboxPhase),
}

/// What a sandboxing actor does ([`AiPhase::Sandbox`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxPhase {
    /// Standing with nothing in particular to do (between two things).
    Standing,
    /// Walking to a spot of the area, or standing at it.
    Wandering,
    /// Walking to something to use (furniture, an idle marker).
    ToObject,
    AtIdleMarker,
    Sitting,
    /// A meal (on a seat when one was free).
    Eating,
    Sleeping,
    /// Got up from furniture, until the next thing it does begins.
    GotUp,
}

/// The least time between two scans for actors whose choice is due, in seconds.
const SCAN_STEP: f64 = 0.25;

impl<'a> Game<'a> {
    /// The AI package list of an actor reference: its base form's `PKID` list, or its
    /// template's when the template supplies the AI packages, followed through
    /// templates and leveled actor lists as the actor's factions are
    /// ([`GameData::base_package_list`]) [documented: xEdit template flags; hypothesis:
    /// which entry of a leveled list stands for the actor, `crate::templates::Picks`].
    ///
    /// [`GameData::base_package_list`]: crate::data::GameData::base_package_list
    pub fn package_list(&self, actor: FormId) -> Vec<FormId> {
        match functions::base_of(self, actor) {
            // AddPackageAt's, RemovePackageAt's and RemoveAllPackages' list first.
            Some(base) if self.other.package_lists.contains_key(&base) => self.other.package_lists[&base].clone(),
            Some(base) => self.template_source(base, actor, |d| d.template_packages).map(|(_, d)| d.packages.clone()).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// A package's definition: a script's change of it, else the record's.
    pub fn package_def(&self, p: &FormId) -> Option<&'a PackageDef> {
        match self.package_overrides.get(p) {
            Some(d) => Some(*d),
            None => self.data.package_defs.get(p),
        }
    }

    /// The time schedules are checked against: GameHour and the calendar day (the
    /// weekday as GetDayOfWeek counts it). `None` when the load order has no time
    /// globals; schedules are then not applied.
    pub fn package_clock(&self) -> Option<Clock> {
        let c = self.world.calendar().ok()?;
        let (year, month, day) = (c.year as i64, (c.month as i64).clamp(0, 11), (c.day as i64).max(1));
        let dow = functions::misc::weekday(year, month + 1, day);
        let today = Day { day_of_week: dow as u8, month: month as u8, day: day as u8 };
        let (ym, yd) = if day > 1 {
            (month, day - 1)
        } else {
            let m = (month + 11) % 12;
            (m, fnv_sim::clock::days_in_month(if month == 0 { year - 1 } else { year }, m))
        };
        let yesterday = Day { day_of_week: ((dow + 6) % 7) as u8, month: ym as u8, day: yd as u8 };
        Some(Clock { hour: (c.hour as f64).rem_euclid(24.0), today, yesterday })
    }

    /// A reference's linked reference, SetLinkedReference's first [observed: jip-n7;
    /// no target puts back the record's].
    pub fn linked_ref(&self, actor: FormId) -> Option<FormId> {
        match self.jip2.links.get(&actor) {
            Some(l) => Some(*l),
            None => self.world.placed.get(actor).and_then(|p| p.linked),
        }
    }

    /// The reference a package's conditions run on with "Run on Target": the
    /// package's first target when it names a reference, or the actor's linked
    /// reference when the target is "linked reference" [documented: GECK wiki,
    /// Category:Packages; observed: the game reported exactly that reference as the
    /// package target, 21,704 samples, packages.md §4].
    pub fn package_target(&self, actor: FormId, def: &PackageDef) -> Option<FormId> {
        match def.target.map(|t| t.kind) {
            Some(TargetKind::Reference(f)) => f,
            Some(TargetKind::LinkedReference) => self.linked_ref(actor),
            _ => None,
        }
    }

    /// Whether `actor` may choose `package` now, and if not, the first reason. Checked
    /// in this order:
    /// 1. the schedule (`Schedule::allows`) [documented: the schedule is an implicit
    ///    condition and the conditions count only while it holds; observed];
    /// 2. a package located at, or aimed at, the actor's linked reference needs one
    ///    [documented for Patrol and Ambush; observed: two coyotes without one ran
    ///    their sandbox past DefaultPatrolCasual, 15,518 samples; hypothesis for the
    ///    other types];
    /// 3. a package whose location the actor found unreachable is left out for a while
    ///    ([`Game::block_package`]) [hypothesis: what the game does with a package it
    ///    cannot carry out is not documented];
    /// 4. a Once Per Day package is not chosen again within 24 game hours of the
    ///    moment the actor chose it [documented]; one that is still running may go on
    ///    [hypothesis] until its duration ends it ([`Game::end_timed_packages`]) or its
    ///    goal is met: a finished one is "not immediately selected again after being
    ///    completed" [documented: GECK wiki, Category:Packages], so the next valid
    ///    package takes over [observed: TTW's CG02AndyCutCake, Must Complete and Once
    ///    Per Day, sets the cake stage CG02 16 only from its change script, which
    ///    needs Andy to leave it once he has cut the cake; issue #2221];
    /// 5. the conditions, with the actor as the subject.
    pub fn check_package(&mut self, actor: FormId, package: FormId) -> PackageCheck {
        let Some(def) = self.package_def(&package) else { return PackageCheck::Missing };
        if self.package_clock().is_some_and(|c| !def.schedule.allows(&c)) {
            return PackageCheck::OutOfSchedule;
        }
        if def.uses_linked_ref() && self.linked_ref(actor).is_none() {
            return PackageCheck::NoLinkedRef;
        }
        let now = self.world.calendar().map(|c| c.days_passed).unwrap_or(0.0);
        if self.extra.get(&actor).and_then(|e| e.package_blocked.get(&package)).is_some_and(|until| now < *until) {
            return PackageCheck::Unreachable;
        }
        if def.has(flags::ONCE_PER_DAY) {
            let e = self.extra.get(&actor);
            // Finished, it still runs while its schedule duration holds it: the original's
            // Goodsprings geckos kept a FindFood that reached DONE until its 2 h were up
            // (owner-decision-captures.md §7; `Game::end_timed_packages`).
            let timed = e.is_some_and(|e| e.package_ends_by_schedule && e.package_ends.is_some_and(|(p, until)| p == package && (now as f64) < until));
            let running = e.is_some_and(|e| e.current_package == Some(package) && (!e.package_goal_met || timed));
            let chosen = e.and_then(|e| e.package_chosen_at.get(&package)).copied();
            if !running && chosen.is_some_and(|t| now - t < 1.0) {
                return PackageCheck::OncePerDay;
            }
        }
        let target = self.package_target(actor, def);
        if self.conditions_pass(&def.conditions, Some(actor), target, &format!("PACK {package}")) {
            PackageCheck::Valid
        } else {
            PackageCheck::ConditionsFail
        }
    }

    /// Whether `package`'s own conditions pass for `actor`, regardless of its schedule.
    fn package_conditions_pass(&mut self, actor: FormId, package: FormId) -> bool {
        let Some(def) = self.package_def(&package) else { return false };
        let target = self.package_target(actor, def);
        self.conditions_pass(&def.conditions, Some(actor), target, &format!("PACK {package}"))
    }

    /// The package an actor runs when it evaluates its packages.
    ///
    /// - A script package (AddScriptPackage) overrides the list until its goal is met;
    ///   then this choice removes it [documented: catalogue summary of AddScriptPackage;
    ///   observed: RingoRef and SunnyRef ran packages that are in no list of theirs, and
    ///   od_06 (owner-decision-captures.md §7) saw a finished one let go].
    /// - Otherwise the first package of the list that is valid
    ///   ([`Game::check_package`]) [documented: GECK wiki, How Are Packages Evaluated;
    ///   observed: `L5.quest.condition_groups`, and 232,755 of 234,335 observer samples
    ///   show a package with no earlier unconditional package in its window; the rest
    ///   are the minutes a switch takes and the next rule, packages.md §4].
    /// - With no valid package the actor keeps the list package it runs [observed:
    ///   GoodspringsSettler01Ref kept GSSettlerAAMSandbox6x16 through the night, 1,466
    ///   samples outside its window, when no other package of its list applied;
    ///   hypothesis: that this is "keep the current one" rather than "the last of the
    ///   list", which are the same package there]; not when the current one's own
    ///   conditions fail: then the actor has no package and stands where it is, off any
    ///   furniture [hypothesis, from the data: Fallout 3's Mister Burke sits on his bar
    ///   chair under MS11BurkeBattle (MS11 battle 1); when the arrest talk sets battle 2
    ///   no package of his list applies, and Lucas Simms' script (Fallout 3's and TTW's
    ///   alike) has Burke shoot him only once Burke's GetSitting reads 0; issue #2235].
    pub fn select_package(&mut self, actor: FormId) -> Option<FormId> {
        let now = self.world.calendar().map(|c| c.days_passed as f64).unwrap_or(0.0);
        if let Some(e) = self.extra.get_mut(&actor) {
            // A script package whose goal was met is removed at the next choice
            // [documented: GECK wiki AddScriptPackage, "takes precedence over all other
            // packages until it is done"; observed: od_06_reset_ai 2026-10-19, RingoRef's
            // DefaultStayAtEditorLocation read DONE (17) for at least 600 game loops and
            // his list package by 900 loops later, with no ResetAI], unless it has a
            // schedule duration still running ([`Game::start_schedule_duration`]): then
            // it stays until that runs out [observed: the Strip's SS2 scene,
            // npc-behaviour-compare.md §10.1, the 3 h Must Complete travels
            // VStripSS2WallDrunk2 and VStripSS2WallDrunk3 stayed current after their
            // trooper arrived, 0.7 h for the latter until the next script package;
            // hypothesis: that the duration is what holds them].
            let timed = e.package_ends.is_some_and(|(p, until)| e.package_ends_by_schedule && Some(p) == e.script_package && now < until);
            if e.script_package.is_some() && e.script_package == e.current_package && e.package_goal_met && !timed {
                e.script_package = None;
            }
        }
        if let Some(p) = self.extra.get(&actor).and_then(|e| e.script_package) {
            return Some(p);
        }
        let list = self.package_list(actor);
        // The conditions may ask for the actor's current package (GetIsCurrentPackage):
        // `current_package` answers with the one it had rather than choosing again.
        self.choosing_package.push(actor);
        let valid = list.iter().copied().find(|&p| self.check_package(actor, p) == PackageCheck::Valid);
        let kept = match valid {
            Some(_) => None,
            // Keep the current list package when no other applies and its conditions
            // pass. Check them directly because a schedule or target problem can
            // short-circuit check_package before it reaches the conditions (#3041).
            None => self.extra.get(&actor).and_then(|e| e.current_package).filter(|c| list.contains(c)).filter(|&c| {
                self.check_package(actor, c) != PackageCheck::ConditionsFail
                    && self.package_conditions_pass(actor, c)
            }),
        };
        self.choosing_package.pop();
        valid.or(kept)
    }

    /// Whether the AI runs packages itself (their begin, end and change scripts and the
    /// periodic choice) rather than leaving them to the runner's inputs.
    pub fn runs_packages(&self) -> bool {
        self.cfg.package_interval.is_some()
    }

    /// Make `package` the actor's current package. A change is traced
    /// (`package_chosen`), starts the Once Per Day clock of the new package and, when
    /// the AI runs packages, runs the old package's change script and OnPackageChange
    /// and the new one's begin script and OnPackageStart [documented: GECK wiki,
    /// Category:Packages, Begin / End / Change]. The change script runs when the actor
    /// leaves a package for another, also one whose goal was met (its end script ran
    /// then) [hypothesis, from the data: TTW's CG02AndyCutCake (Must Complete, Once Per
    /// Day, nothing else ends it) has only its change script set the cake stage CG02 16
    /// the birthday party waits for; issue #2221. Most of the 99 packages of the TTW
    /// load order with both scripts give them the same lines].
    pub(crate) fn set_package(&mut self, actor: FormId, package: Option<FormId>) {
        let e = self.extra.entry(actor).or_default();
        let old = e.current_package;
        if old == package {
            return;
        }
        e.current_package = package;
        e.package_goal_met = false;
        e.package_relocated = false;
        if let Some(p) = package.filter(|p| self.package_def(p).is_some_and(|d| d.has(flags::ONCE_PER_DAY))) {
            let now = self.world.calendar().map(|c| c.days_passed).unwrap_or(0.0);
            self.extra.entry(actor).or_default().package_chosen_at.insert(p, now);
        }
        // Before the begin script runs, so that a SetPackDuration there replaces it.
        self.start_schedule_duration(actor);
        self.emit(EventKind::PackageChosen { actor, package });
        if self.runs_packages() {
            // Doors the old package locks or unlocks as it ends, the new one as it starts.
            if let Some(o) = old {
                self.package_doors(actor, o, flags::LOCK_DOORS_AT_END, flags::UNLOCK_DOORS_AT_END);
            }
            if let Some(p) = package {
                self.package_doors(actor, p, flags::LOCK_DOORS_AT_START, flags::UNLOCK_DOORS_AT_START);
                self.package_doors(actor, p, flags::LOCK_DOORS_AT_LOCATION, flags::UNLOCK_DOORS_AT_LOCATION);
            }
            // Keep `current_package` at `package` while the outgoing package's Change
            // script and OnPackageChange run: if either calls EvaluatePackage,
            // `set_package` sees the new package instead of `old` and does not re-enter
            // `old`'s Change script recursively.
            if let Some(o) = old {
                self.run_package_event(actor, o, 2);
            }
            // Only run the new package's Begin script if the Change script did not
            // already switch the actor to another package.
            if let Some(p) = package {
                if self.extra.get(&actor).and_then(|e| e.current_package) == package {
                    self.run_package_event(actor, p, 0);
                }
            }
        }
    }

    /// A package's door flags (`flags::LOCK_DOORS_AT_START` … `UNLOCK_DOORS_AT_LOCATION`):
    /// "the actor will lock or unlock all the doors in the cell" when the package starts or
    /// ends, and "for the duration of the package … will keep all doors in the cell locked
    /// or unlocked" [documented: GECK wiki Packages, Door Flags]. The cell is the actor's;
    /// only doors with an applicable lock (`World::lock_of`) are changed [design,
    /// #2538]. Lock creates a local lock; Unlock changes the source [hypothesis]. The "at
    /// location" flags are applied as the package starts and at each periodic choice
    /// ([`Game::tick_packages`]) [design: how often the original checks is not known].
    /// Returns how many doors changed.
    pub(crate) fn package_doors(&mut self, actor: FormId, package: FormId, lock: u32, unlock: u32) -> usize {
        let data = self.data;
        let Some(def) = self.package_def(&package) else { return 0 };
        let (locks, unlocks) = (def.has(lock), def.has(unlock));
        if !locks && !unlocks {
            return 0;
        }
        let Some(cell) = self.cell_of(actor) else { return 0 };
        let placed = self.world.placed;
        let doors: Vec<FormId> = placed.by_cell.get(&cell).into_iter().flatten().copied().filter(|r| data.sig(placed.get(*r).map_or(FormId(0), |p| p.base)).as_ref() == Some(b"DOOR")).collect();
        let mut changed = 0;
        for d in doors {
            let Some(l) = self.world.lock_of(d) else { continue };
            if locks && !l.locked {
                changed += usize::from(self.world.lock(d, None).is_ok());
            } else if unlocks && !locks && l.locked {
                changed += usize::from(self.world.unlock(d).is_ok());
            }
        }
        if changed > 0 {
            self.package_door_notes.push((actor, package, locks, changed));
        }
        changed
    }

    /// `EvaluatePackage`: the actor picks its package again ([`Game::select_package`])
    /// and keeps it as its current package, whatever it was running: even a Must
    /// Complete package is dropped [documented: GECK wiki, EvaluatePackage]. Without
    /// [`crate::game::GameConfig::package_interval`] the package is not started (its
    /// begin result script, OnPackageStart); the runner's `package` input stands in.
    pub fn evaluate_package(&mut self, actor: FormId) -> Option<FormId> {
        let p = self.select_package(actor);
        self.set_package(actor, p);
        self.extra.get(&actor).and_then(|e| e.current_package)
    }

    /// The actor's current package: the one last chosen, or chosen now when it never
    /// evaluated its packages (an actor always runs a package once loaded [hypothesis]).
    /// Asked while the actor chooses (a condition of one of its packages), it is the one
    /// the actor had, none on its first choice [hypothesis: choosing again from inside
    /// the choice would never end]; that case is traced as skipped, so that the sweep
    /// lists the packages whose conditions do it.
    pub fn current_package(&mut self, actor: FormId) -> Option<FormId> {
        match self.extra.get(&actor).and_then(|e| e.current_package) {
            Some(p) => Some(p),
            None if self.choosing_package.contains(&actor) => {
                let what = format!("current package of {actor} asked by a package condition while it chooses its first one");
                self.emit(EventKind::Skipped { unit: "packages".into(), what });
                None
            }
            None => self.evaluate_package(actor),
        }
    }

    /// The layer that carries packages out reports that the actor's current package
    /// reached its goal (the destination, the target found, the patrol's last point):
    /// a Must Complete or Must Reach Location package may now be replaced, and, when
    /// the AI runs packages, its end script and OnPackageDone run [documented: GECK
    /// wiki, Category:Packages].
    ///
    /// A package type with no done state (`PackageType::has_done_state`: Sandbox, Sleep,
    /// Eat, Guard, Wander, Follow, Accompany) never meets its goal: arriving changes
    /// nothing, a script package of that type stays until it is removed or replaced, and
    /// Must Complete keeps holding it (issue #2012).
    pub fn package_goal_met(&mut self, actor: FormId) {
        let Some(p) = self.extra.get(&actor).and_then(|e| e.current_package.filter(|_| !e.package_goal_met)) else { return };
        if !self.package_has_done_state(p) {
            return;
        }
        let Some(e) = self.extra.get_mut(&actor) else { return };
        e.package_goal_met = true;
        if self.runs_packages() {
            self.run_package_event(actor, p, 1);
        }
    }

    /// Whether `package`'s type has a done state (`PackageType::has_done_state`); a
    /// package with no record counts as having one.
    pub fn package_has_done_state(&self, package: FormId) -> bool {
        self.package_def(&package).is_none_or(|d| d.ty.has_done_state())
    }

    /// For the layer that carries packages out: whether ResetAI asked for `actor`'s
    /// package to be taken up afresh since it last asked (`RefExtra::ai_reset`), even when
    /// the same package is current; the request is cleared. Repeated ResetAI calls before
    /// that are one request [design].
    pub fn take_ai_reset(&mut self, actor: FormId) -> bool {
        self.extra.get_mut(&actor).is_some_and(|e| std::mem::take(&mut e.ai_reset))
    }

    /// The layer that carries packages out tells what `actor` does within its package
    /// ([`AiPhase`]; `None`: it does not run the actor now).
    pub fn set_ai_phase(&mut self, actor: FormId, phase: Option<AiPhase>) {
        match self.extra.get_mut(&actor) {
            Some(e) => e.ai_phase = phase,
            None if phase.is_some() => self.extra.entry(actor).or_default().ai_phase = phase,
            None => {}
        }
    }

    /// The layer that carries packages out found no way to `package`'s location: the
    /// actor does not choose it for the next `hours` game hours, and chooses again now
    /// (the next valid package of its list) [hypothesis: the game's behaviour is not
    /// documented; an actor standing still for ever would be the alternative].
    pub fn block_package(&mut self, actor: FormId, package: FormId, hours: f32) -> Option<FormId> {
        let now = self.world.calendar().map(|c| c.days_passed).unwrap_or(0.0);
        let e = self.extra.entry(actor).or_default();
        e.package_blocked.retain(|_, until| *until > now);
        e.package_blocked.insert(package, now + hours / 24.0);
        // The blocked package must not be kept as "the current one" either.
        if e.current_package == Some(package) && e.script_package != Some(package) {
            self.set_package(actor, None);
        }
        self.evaluate_package(actor)
    }

    /// A door was unlocked (a script's Unlock, a key, a lock picked): every package set
    /// aside because its place could not be reached ([`Game::block_package`]) may be
    /// chosen again, and the actors that set one aside choose now. Returns those actors
    /// [hypothesis: the original's handling is not documented; the actor would otherwise
    /// wait out the hour with the way already open].
    pub fn unblock_packages(&mut self) -> Vec<FormId> {
        let actors: Vec<FormId> = self.extra.iter().filter(|(_, e)| !e.package_blocked.is_empty()).map(|(a, _)| *a).collect();
        for &a in &actors {
            if let Some(e) = self.extra.get_mut(&a) {
                e.package_blocked.clear();
            }
            self.evaluate_package(a);
        }
        actors
    }

    /// `actor` was put somewhere else (a script's MoveTo, a door, brought along with the
    /// player): the packages it set aside because their place could not be reached
    /// ([`Game::block_package`]) may be chosen again, and it chooses now, as after an
    /// unlocked door ([`Game::unblock_packages`]). Returns whether it had any [hypothesis:
    /// the original's handling is not documented; #3472: Rex, his follow package set aside
    /// at the scrapyard, stood idle for half an hour next to the player in Jacobstown].
    pub fn unblock_packages_of(&mut self, actor: FormId) -> bool {
        let had = self.extra.get_mut(&actor).is_some_and(|e| {
            let had = !e.package_blocked.is_empty();
            e.package_blocked.clear();
            had
        });
        if had {
            self.evaluate_package(actor);
        }
        had
    }

    /// A list package with a schedule duration and no starting hour runs for that many
    /// game hours from the moment the actor chose it, then ends
    /// ([`Game::end_timed_packages`]) [documented: GECK wiki SetPackDuration "sets the
    /// duration of the actor's current package", the Schedule tab sets it in hours, and
    /// Continue If PC Near holds a package "even if the package should have expired";
    /// hypothesis: counted from the choice, as no starting hour gives another start;
    /// observed: such lists are built as chains, e.g. the Goodsprings geckos' Once Per
    /// Day VCrGeckoFindFood 2 h, VCrGeckoPatrolRunningLinkedRef 3 h, VCrGeckoFindFood02
    /// 2 h, VCrGeckoPatrolRunning02LinkedRef 3 h, then a sandbox with no duration].
    /// A package with a starting hour ends with its window instead (the schedule
    /// check). A script package (AddScriptPackage) with a duration and no starting hour
    /// is timed the same way: done before its time, it stays until the time is up
    /// ([`Game::select_package`]) [observed: the Strip's SS2 scene,
    /// npc-behaviour-compare.md §10.1; hypothesis: from the moment it was given, as for a
    /// list package from its choice].
    fn start_schedule_duration(&mut self, actor: FormId) {
        let Some(e) = self.extra.get(&actor) else { return };
        let Some(p) = e.current_package else { return };
        let Some(def) = self.package_def(&p) else { return };
        if def.schedule.hour >= 0 || def.schedule.duration == 0 {
            return;
        }
        let now = self.world.calendar().map(|c| c.days_passed as f64).unwrap_or(0.0);
        let e = self.extra.entry(actor).or_default();
        e.package_ends = Some((p, now + def.schedule.duration as f64 / 24.0));
        e.package_ends_by_schedule = true;
    }

    /// Packages whose time is up end (SetPackDuration's, or the schedule duration of
    /// [`Game::start_schedule_duration`] once the package's goal was met): the package counts as done (its end script
    /// and OnPackageDone run, as for a goal met), the actor lets it go and chooses again
    /// at once, by the usual rules [hypothesis: what "duration" ends into is not
    /// documented; choosing again may pick the same package, which then starts afresh
    /// with its begin script; a Once Per Day package is not chosen again, which is what
    /// the flag is for: "not immediately selected again after being completed",
    /// documented: GECK wiki, Category:Packages]. A schedule duration does not end a
    /// package that is held ([`Game::package_held`]: Must Complete or Must Reach
    /// Location before its goal, Continue If PC Near with the player in the cell)
    /// [documented: the flags "will not terminate" until then]. With no other valid
    /// package the actor keeps the ended one [hypothesis, as the keep rule of
    /// [`Game::select_package`]]. A package changed in the meantime drops the timer.
    pub fn end_timed_packages(&mut self) {
        let now = self.world.calendar().map(|c| c.days_passed as f64).unwrap_or(0.0);
        let due: Vec<FormId> = self
            .extra
            .iter()
            .filter_map(|(a, e)| e.package_ends.filter(|(p, until)| e.current_package == Some(*p) && now >= *until).map(|_| *a))
            .collect();
        self.extra.values_mut().filter(|e| e.package_ends.is_some_and(|(p, _)| e.current_package != Some(p))).for_each(|e| e.package_ends = None);
        for a in due {
            if self.extra.get(&a).is_some_and(|e| e.dead) || self.ai_off(a) {
                self.extra.entry(a).or_default().package_ends = None;
                continue;
            }
            if self.extra.get(&a).is_some_and(|e| e.package_ends_by_schedule) && self.package_held(a).is_some() {
                continue;
            }
            // A schedule duration ends the package only once its goal was met; until then
            // it runs on past its time, and ends as soon as the goal is met [observed:
            // owner-decision-captures.md §4 and §7, the Goodsprings geckos' Once Per Day
            // FindFood (2 h) and patrol (3 h) ran 4.7 to 13 h when they never finished,
            // and a FindFood that reached DONE ended 2 h after it began].
            // A type with no done state never meets its goal: its schedule duration ends
            // it on time, with no end script, and a script package of that type is let
            // go then (issue #2012) [design].
            let p = self.extra.get(&a).and_then(|e| e.current_package);
            let done_state = p.is_none_or(|p| self.package_has_done_state(p));
            if done_state && self.extra.get(&a).is_some_and(|e| e.package_ends_by_schedule && !e.package_goal_met) {
                continue;
            }
            let e = self.extra.entry(a).or_default();
            e.package_ends = None;
            if !done_state && e.script_package == p {
                e.script_package = None;
            }
            self.package_goal_met(a);
            if p.is_some_and(|p| self.extra.get(&a).is_some_and(|e| e.script_package != Some(p))) {
                self.set_package(a, None);
            }
            if self.evaluate_package(a).is_none() {
                // Nothing else is valid: the ended package goes on (no begin script).
                self.extra.entry(a).or_default().current_package = p;
                self.emit(EventKind::PackageChosen { actor: a, package: p });
            }
        }
    }

    /// Finish a discontinuous move after placement succeeded (MoveTo, a follower
    /// brought along, or a scripted door activation) [design: functions/refs.rs,
    /// followers.rs, game.rs]. Ordinary walking uses `set_actor_place` instead.
    pub(crate) fn note_relocated(&mut self, reference: FormId, cell: Option<FormId>, pos: [f32; 3]) {
        if self.ref_script(reference).is_some() {
            self.moved.insert(reference);
        }
        if reference != FormId::PLAYER_REF {
            self.teleported.insert(reference);
            // A script relocating an actor running a Must Complete / Must Reach Location
            // package lets the next choice drop it if it is no longer valid (issue #3041:
            // VERStalkersSouth01 moves the night stalkers to their holding interior and
            // clears StalkersOn without calling EvaluatePackage) [design].
            if let Some(e) = self.extra.get_mut(&reference).filter(|e| e.current_package.is_some()) {
                e.package_relocated = true;
            }
            // A drawn object is redrawn at its destination, as after SetPos.
            if !is_actor_base(self.world.forms, functions::base_of(self, reference).unwrap_or(FormId(0))) {
                self.turned.insert(reference);
            }
        }
        self.emit(EventKind::Moved { reference, cell, pos });
    }

    /// The references a script put somewhere since the last call (MoveTo or a door; see
    /// [`Game::teleported`]).
    pub fn take_teleported(&mut self) -> BTreeSet<FormId> {
        std::mem::take(&mut self.teleported)
    }

    /// Put an actor somewhere without a trace event: the layer that moves actors calls
    /// this as they walk (a script's MoveTo is traced; a step is not).
    pub fn set_actor_place(&mut self, actor: FormId, cell: Option<FormId>, pos: [f32; 3], heading: f32) -> Result<(), String> {
        let s = self.world.get(actor).map_err(|e| e.to_string())?;
        s.cell = cell;
        s.pos = pos;
        s.rot[2] = heading;
        if self.ref_script(actor).is_some() {
            self.moved.insert(actor);
        }
        Ok(())
    }

    /// Why the game's own periodic choice would leave the current package alone, if
    /// it would: the package is Must Complete or Must Reach Location and has not met
    /// its goal, or it is Continue If PC Near and the player is in the actor's cell
    /// [documented: GECK wiki, Category:Packages flags].
    pub fn package_held(&self, actor: FormId) -> Option<&'static str> {
        let e = self.extra.get(&actor)?;
        let def = self.package_def(&e.current_package?)?;
        if def.has(flags::MUST_COMPLETE | flags::MUST_REACH_LOCATION) && !e.package_goal_met && !e.package_relocated {
            return Some(if def.has(flags::MUST_COMPLETE) { "must complete" } else { "must reach location" });
        }
        let cell = self.cell_of(actor);
        if def.has(flags::CONTINUE_IF_PC_NEAR) && cell.is_some() && cell == self.cell_of(FormId::PLAYER_REF) {
            return Some("continue if PC near");
        }
        None
    }

    /// The periodic choice the game makes for an actor (about every 10 seconds
    /// [documented: "seem to be", GECK wiki; observed: actors switched 0.4 to 26 real
    /// seconds after a schedule boundary]): choose again unless the current package is
    /// held ([`Game::package_held`]).
    pub fn update_package(&mut self, actor: FormId) -> Option<FormId> {
        if self.package_held(actor).is_some() {
            return self.extra.get(&actor).and_then(|e| e.current_package);
        }
        self.evaluate_package(actor)
    }

    /// The actors scripts created at run time (PlaceAtMe of an NPC_, CREA or leveled
    /// actor list) that are still in the world: they take part in the AI, detection and
    /// combat as placed actors do.
    pub fn created_actors(&self) -> Vec<FormId> {
        let forms = self.world.forms;
        self.world.created().filter(|(_, s)| !s.deleted() && is_actor_base(forms, s.base)).map(|(id, _)| id).collect()
    }

    /// Placed actors whose packages the periodic choice covers: those in loaded cells
    /// and those that already run a package [hypothesis: the game goes on choosing for
    /// actors it has started; observed: actors in unloaded interiors changed package at
    /// their schedule, together, ai-observer.md §7].
    pub(crate) fn package_actors(&self) -> BTreeSet<FormId> {
        let placed = self.world.placed;
        let is_actor = |r: &FormId| placed.get(*r).is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE"));
        let mut out: BTreeSet<FormId> = BTreeSet::new();
        for c in &self.loaded_cells {
            out.extend(placed.by_cell.get(c).into_iter().flatten().filter(|r| is_actor(r)));
        }
        out.extend(self.moved.iter().filter(|r| is_actor(r)));
        // Actors scripts created (PlaceAtMe) choose packages as placed ones do.
        out.extend(self.created_actors());
        out.retain(|r| self.cell_of(*r).is_some_and(|c| self.loaded_cells.contains(&c)));
        out.extend(self.extra.iter().filter(|(r, e)| e.current_package.is_some() && is_actor(r)).map(|(r, _)| *r));
        out
    }

    /// Every actor the periodic choice covers chooses now, as at its turn
    /// ([`Game::update_package`]), with the packages' begin, end and change scripts; its
    /// turns in the interval stay as they were. For game time that passes while a menu
    /// holds the game (an hour of a wait or a sleep): the periodic choice goes by
    /// [`Game::time`], which stands still there. [engineering choice: what the original's
    /// actors do while the player waits is not described by any source read;
    /// docs/spec-notes/wait-menu.md §7.]
    pub fn choose_packages_now(&mut self) {
        if !self.runs_packages() {
            return;
        }
        self.end_timed_packages();
        for a in self.package_actors() {
            if self.present(a) && !self.extra.get(&a).is_some_and(|e| e.dead) && !self.ai_off(a) {
                self.update_package(a);
            }
        }
    }

    /// The periodic package choice, from [`Game::tick`]: every actor chooses when it is
    /// first seen and then every `package_interval` seconds. Actors do not all choose
    /// in the same frame: each has a fixed phase within the interval, from its FormID
    /// [our choice; observed: switch delays after a boundary are spread, not equal].
    pub(crate) fn tick_packages(&mut self) {
        let Some(interval) = self.cfg.package_interval.filter(|i| *i > 0.0) else { return };
        self.end_timed_packages();
        if self.time < self.package_scan_due {
            return;
        }
        let now = self.time;
        let mut next = now + interval;
        for a in self.package_actors() {
            if !self.present(a) || self.extra.get(&a).is_some_and(|e| e.dead) || self.ai_off(a) {
                continue;
            }
            // An actor walking into a loaded cell for the first time dresses as one loaded
            // there (`Game::outfit_loaded_actors`).
            let _ = self.world.outfit(a);
            self.maybe_apply_npc_weapon_mods(a);
            let due = match self.extra.get(&a).and_then(|e| e.package_due) {
                Some(d) => d,
                None => {
                    // First sight: choose now, then keep to the actor's phase.
                    let phase = (a.0.wrapping_mul(2_654_435_761) >> 22) as f64 / 1024.0;
                    self.update_package(a);
                    now + interval * phase
                }
            };
            let due = if now >= due {
                self.update_package(a);
                // Doors kept locked or unlocked while the package runs.
                if let Some(p) = self.extra.get(&a).and_then(|e| e.current_package) {
                    self.package_doors(a, p, flags::LOCK_DOORS_AT_LOCATION, flags::UNLOCK_DOORS_AT_LOCATION);
                }
                due + interval * ((now - due) / interval).floor().max(0.0) + interval
            } else {
                due
            };
            self.extra.entry(a).or_default().package_due = Some(due);
            next = next.min(due);
        }
        self.package_scan_due = next.max(now + SCAN_STEP);
    }
}

/// Whether `base` is an actor base: an NPC_, a CREA, or a leveled list of them (LVLN,
/// LVLC), whose placed reference stands for one of its entries (`crate::templates`).
pub fn is_actor_base(forms: &fnv_sim::FormStore, base: FormId) -> bool {
    matches!(forms.sig(base), Some(s) if [b"NPC_", b"CREA", b"LVLN", b"LVLC"].contains(&&s))
}
