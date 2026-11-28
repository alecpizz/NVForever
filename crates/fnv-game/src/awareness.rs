//! Detection states: what an actor does with what it perceives (docs/design/alert.md,
//! facts in docs/spec-notes/actor-behaviour.md §2.2–§2.3).
//!
//! Every actor is in one of four states [documented: GECK wiki Category:Detection]:
//! **Normal**, **Alert** (it has noticed something of an actor it would attack, but has
//! not detected it: it draws its weapon and searches), **Combat** (it detects an actor
//! it would attack) and **Lost** (it was fighting, and every target has gone undetected:
//! it searches like an alert actor). Alert, Combat and Lost are the combat states.
//!
//! What moves an actor between them:
//! - the detection values the engine measures (`crate::detection`), passed in through
//!   [`Game::observe_threat`] for each pair: a hostile above fSneakNoticedMin (−20) and
//!   at or below 0 (noticed: heard moving, glimpsed far away or in the dark) puts an
//!   **alarm** on the actor's alert list at the hostile's position; a hostile above 0
//!   (detected) goes on its **target list**;
//! - detection events (CreateDetectionEvent, impacts and explosions the engine raises,
//!   [`Game::queue_detection_event`]) that the actor hears ([`Game::hear_event`]), and
//!   being hurt by a hostile ([`Game::hurt_by`]): an alarm at where it happened;
//! - [`Game::lose_unmeasured`], in each round of the engine's measurements: a target
//!   whose pair is measured no more counts as hidden (#3390);
//! - [`Game::settle_awareness`], after each round of measurements: alarms and targets
//!   time out, and the state follows the documented transitions.
//!
//! Only actors the detector would attack count ([`Game::would_attack`]): what others do
//! is never a threat.
//!
//! [design, #2010] [`Timers::read`] reads detection timers from the load order;
//! Lost searches finish through [`Game::searched_once`], with a separate stall bound.

use crate::detection::{Band, Detection};
use crate::functions::actor::{actor_value, faction_relation, factions_of};
use crate::functions::{none, yes, Args, Ctx};
use crate::game::{event, Game};
use crate::package::flags;
use fnv_script::vm::Value;
use fnv_sim::FormId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const AGGRESSION: u16 = crate::actor_values::AGGRESSION;

/// An actor's detection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub enum State {
    #[default]
    Normal,
    Alert,
    Combat,
    Lost,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Normal => "Normal",
            State::Alert => "Alert",
            State::Combat => "Combat",
            State::Lost => "Lost",
        }
    }

    /// Alert, Combat and Lost are combat states [documented: Category:Detection].
    pub fn in_combat(self) -> bool {
        self != State::Normal
    }

    /// The actor searches: Alert and Lost [documented: "Alert and Lost actors search …
    /// if their package allows"].
    pub fn searching(self) -> bool {
        matches!(self, State::Alert | State::Lost)
    }

    /// The Detection topic said on the change from `self` to `to`, named
    /// `<From>To<To>` [documented: GECK wiki Detection Tab]; `None` for changes that have
    /// no topic.
    pub fn topic_to(self, to: State) -> Option<&'static str> {
        Some(match (self, to) {
            (State::Normal, State::Alert) => "NormalToAlert",
            (State::Normal, State::Combat) => "NormalToCombat",
            (State::Alert, State::Combat) => "AlertToCombat",
            (State::Alert, State::Normal) => "AlertToNormal",
            (State::Combat, State::Normal) => "CombatToNormal",
            (State::Combat, State::Lost) => "CombatToLost",
            (State::Lost, State::Combat) => "LostToCombat",
            (State::Lost, State::Normal) => "LostToNormal",
            _ => return None,
        })
    }

    /// The topic said now and then while in this state [documented: Detection Tab,
    /// AlertIdle "while … searching for a target they have never detected", LostIdle for
    /// one they have lost].
    pub fn idle_topic(self) -> Option<&'static str> {
        match self {
            State::Alert => Some("AlertIdle"),
            State::Lost => Some("LostIdle"),
            _ => None,
        }
    }
}

/// CreateDetectionEvent's event types [documented: GECK wiki CreateDetectionEvent].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EventType {
    Bullet,
    Beam,
    Explosion,
    Generic,
}

impl EventType {
    pub fn from_code(c: i64) -> EventType {
        match c {
            0 => EventType::Bullet,
            1 => EventType::Beam,
            2 => EventType::Explosion,
            _ => EventType::Generic,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            EventType::Bullet => "bullet",
            EventType::Beam => "beam",
            EventType::Explosion => "explosion",
            EventType::Generic => "generic",
        }
    }
}

/// Why an alarm is on an actor's alert list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cause {
    /// A hostile it noticed (above −20, not detected).
    Noticed,
    /// A detection event it heard.
    Event(EventType),
    /// A hostile hurt it.
    Hurt,
    /// A target it was fighting went undetected: where it was last known to be.
    LostTarget,
    /// An ally it saw fighting is fighting this one (assistance, `crate::provocation`).
    Ally,
}

impl Cause {
    pub fn describe(self) -> String {
        match self {
            Cause::Noticed => "noticed".into(),
            Cause::Event(t) => format!("heard a {} event", t.name()),
            Cause::Hurt => "was hurt".into(),
            Cause::LostTarget => "lost sight".into(),
            Cause::Ally => "saw an ally fight".into(),
        }
    }
}

/// An entry of an actor's alert list: something of `owner` happened at `at`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Alarm {
    pub owner: FormId,
    /// Game space.
    pub at: [f32; 3],
    /// The detection value it was noticed with (the event's "size").
    pub size: f64,
    pub cause: Cause,
    /// `Game::time` it was raised, last refreshed, and runs out.
    pub raised: f64,
    pub refreshed: f64,
    pub expires: f64,
}

/// An entry of an actor's target list.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// The band and value of the latest measurement.
    pub band: Band,
    pub value: f64,
    /// Where it was last measured (game space).
    pub at: [f32; 3],
    /// When it was last detected (`Game::time`); `None`: never (put on the list by an
    /// event it made).
    pub detected: Option<f64>,
    /// Since when it has been hidden (at or below −20): the timer that takes it off the
    /// list.
    pub hidden_since: Option<f64>,
}

/// Detection timers, seconds of `Game::time` [design, #2010: setting-to-timer mapping;
/// observed defaults: issue #2010 capture, docs/spec-notes/npc-behaviour-compare.md §15.6].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timers {
    /// An alarm (noticed, hurt, an ally's call, an event heard) runs out this long after
    /// refresh [design: existing Alert policy]. fDetectionEventExpireTime is not read for
    /// it: by its name it is how long an event itself can be heard, and ours are heard in
    /// the round they are raised [hypothesis: the setting's name]; a 10 s alarm sent a
    /// camp's alarmer back to Normal before the party it called had searched (smoke
    /// `group-search`, #2010).
    pub alarm: f64,
    /// fCombatTargetLostRemoveTime; does not interrupt a Lost search [design, #2010].
    pub forget_target: f64,
    /// fCombatDetectionLostTime: no target detected for this long enters Lost
    /// [design, #2010; documented: GECK wiki Combat Search, a GameSetting delay].
    pub lost: f64,
    /// AlertIdle and LostIdle are said every this many seconds, a random time between the
    /// two [documented: Detection Tab, fCombatDetectionDialogueIdleMin/MaxElapsedTime;
    /// the defaults are FalloutNV.esm's values, observed 2026-10-11].
    pub idle_bark: (f64, f64),
    /// After an actor says a Detection line of one kind ([`bark_kind`]), neither it nor
    /// a member of its group ([`Game::bark_mates`]) within [`BARK_GROUP_RADIUS`] says one
    /// of the same kind for this many seconds [owner request 2026-10-11: "if an enemy in
    /// a group says a line … all the others don't shout the same line type"; design:
    /// the length is the longer idle-bark wait, fCombatDetectionDialogueIdleMaxElapsedTime,
    /// 15 s in FalloutNV.esm].
    pub bark_cooldown: f64,
}

impl Default for Timers {
    fn default() -> Timers {
        Timers { alarm: 20.0, forget_target: 30.0, lost: 15.0, idle_bark: (5.0, 15.0), bark_cooldown: 15.0 }
    }
}

impl Timers {
    /// [design, #2010] Defaults overridden by the load order's game settings.
    pub fn read(g: &Game) -> Timers {
        let d = Timers::default();
        let s = |name: &str, v: f64| crate::combat::setting(g, name, v);
        let idle_bark = (s("fCombatDetectionDialogueIdleMinElapsedTime", d.idle_bark.0), s("fCombatDetectionDialogueIdleMaxElapsedTime", d.idle_bark.1));
        Timers {
            lost: s("fCombatDetectionLostTime", d.lost),
            forget_target: s("fCombatTargetLostRemoveTime", d.forget_target),
            idle_bark,
            bark_cooldown: idle_bark.1.max(idle_bark.0),
            ..d
        }
    }
}

/// A Lost search without a completed share for this many seconds may expire
/// [design, #2010: safety for unavailable navigation/AI; not an original-game timer].
pub const LOST_SEARCH_STALL_SECONDS: f64 = 300.0;

/// Members of a group within this many game units of a Detection line's speaker do not
/// say a line of the same kind while its cooldown lasts [design: one exterior cell's
/// width, about the spread of a camp such as the Fiends' around Cook-Cook].
pub const BARK_GROUP_RADIUS: f32 = 4096.0;

/// The kind of a Detection topic, for the group cooldown: the three topics an actor says
/// on starting to fight (NormalToCombat, AlertToCombat, LostToCombat) are one kind, as
/// are the three on giving up (AlertToNormal, CombatToNormal, LostToNormal) and the two
/// idles (AlertIdle, LostIdle); NormalToAlert and CombatToLost are kinds of their own
/// [design: lines of one kind say the same thing].
pub fn bark_kind(topic: &'static str) -> &'static str {
    match topic {
        "NormalToCombat" | "AlertToCombat" | "LostToCombat" => "to combat",
        "AlertToNormal" | "CombatToNormal" | "LostToNormal" => "to normal",
        "AlertIdle" | "LostIdle" => "idle",
        t => t,
    }
}

/// Who of `said` (the lines said within the cooldown) holds back a line of `kind` by an
/// actor standing at `at`: the first that said a line of that kind within
/// [`BARK_GROUP_RADIUS`] of it and is of its group (`mates`, the actor itself included).
pub fn held_back_by(said: &[Said], kind: &str, at: Option<[f32; 3]>, mates: impl Fn(FormId) -> bool) -> Option<FormId> {
    let near = |s: &Said| at.is_none_or(|a| (a[0] - s.at[0]).hypot(a[1] - s.at[1]) <= BARK_GROUP_RADIUS);
    said.iter().find(|s| s.kind == kind && near(s) && mates(s.speaker)).map(|s| s.speaker)
}

/// A Detection line said: when, by whom, where, of what kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Said {
    pub kind: &'static str,
    pub speaker: FormId,
    pub at: [f32; 3],
    pub time: f64,
}

/// One actor's detection state, alert list and target list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Awareness {
    pub state: State,
    /// `Game::time` the state began.
    pub since: f64,
    pub alarms: Vec<Alarm>,
    pub targets: BTreeMap<FormId, Target>,
    /// When the next AlertIdle or LostIdle is due.
    pub next_idle_bark: Option<f64>,
    /// The target it fought last ([`Awareness::combat_target`] keeps it), remembered while
    /// it is out of sight, and since when it has fought it.
    pub fighting: Option<FormId>,
    pub fighting_since: f64,
    /// Within [`TARGET_COMMIT`] of turning to `fighting`: it keeps it while it detects it,
    /// unless another is much nearer ([`Awareness::settle`] sets it).
    pub committed: bool,
    /// Where it stood at its latest measurement (game space), for the commitment.
    pub at: Option<[f32; 3]>,
    /// When a member of its combat group last detected each of its targets
    /// ([`Game::share_sightings`]): a target a mate still detects is neither lost nor
    /// forgotten (docs/spec-notes/combat-groups.md §3).
    #[serde(default)]
    pub shared: BTreeMap<FormId, f64>,
    /// `Game::time` it went into Combat from Normal or Alert (not back from Lost): a
    /// fighter stands on its heading a moment before it turns to its target
    /// (`fnv_ai::alert::COMBAT_REACTION_SECONDS`, docs/spec-notes/actors.md §8.9).
    #[serde(default)]
    pub combat_began: Option<f64>,
    /// The player's Hostiles Prioritize Player tweak: it fights the player, while it
    /// detects it, over the companion it would have chosen ([`Game::prefer_player`]), and
    /// the companion that roll was made for (one roll each) [design: docs/design/tweaks.md].
    #[serde(default)]
    pub prefer_player: bool,
    #[serde(default)]
    pub rolled_for: Option<FormId>,
    /// Targets a script's StartCombat gave it that the engine has not measured since:
    /// not being measured yet does not hide them ([`Awareness::lose_unmeasured`]). Saved
    /// as `GameSnapshot::scripted_combat`.
    #[serde(skip)]
    pub scripted: BTreeSet<FormId>,
}

/// The detection value StartCombat gives its target, until it is measured
/// ([`Game::start_combat`]) [engineering choice].
const STARTED_VALUE: f64 = 100.0;

/// A fighter keeps its target while it detects it, unless another detected one is more
/// than this above it in detection value [design: owner 2026-10-16, NCR troopers
/// fighting Fiends turned between targets, the highest of values that change at every
/// measurement, every half second].
pub const TARGET_KEEP_MARGIN: f64 = 50.0;

/// For this many seconds after turning to a target a fighter keeps it while it detects
/// it, whatever the others' values; it turns earlier only when the target dies, goes or
/// is out of sight [design: owner 2026-10-19, fighters with several enemies "flicker
/// between targets"; a target switch cooldown of about 5 seconds], or when another is
/// nearer than [`TARGET_COMMIT_NEARER`] of its target's distance.
pub const TARGET_COMMIT: f64 = 5.0;

/// A fighter committed to a target turns to another nearer than this share of its
/// target's distance [design: with the commitment alone, NCR troopers kept firing at
/// Fiends 2,000 units off while melee Fiends charged them, took no cover and lost the
/// battle (smoke battle-ncr-fiends and combat-cover, merge run of PR #470, 2026-10-19);
/// detection value grows as the square of nearness, so the margin let them turn].
pub const TARGET_COMMIT_NEARER: f32 = 0.5;

/// A change of state, for the engine's log and the tools.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Change {
    pub actor: FormId,
    pub from: State,
    pub to: State,
    pub time: f64,
    /// Whom and what it is about: the target detected, the alarm's owner and cause.
    pub about: Option<FormId>,
    pub cause: Option<Cause>,
    pub at: Option<[f32; 3]>,
    pub value: Option<f64>,
}

impl Awareness {
    /// A measurement of a hostile `target`, at `at`.
    pub fn observe(&mut self, target: FormId, det: &Detection, at: [f32; 3], now: f64, t: &Timers) {
        self.scripted.remove(&target);
        match det.band {
            Band::Detected => {
                let e = self.targets.entry(target).or_insert(Target { band: det.band, value: det.value, at, detected: None, hidden_since: None });
                *e = Target { band: det.band, value: det.value, at, detected: Some(now), hidden_since: None };
            }
            Band::Noticed => {
                if let Some(e) = self.targets.get_mut(&target) {
                    e.band = det.band;
                    e.value = det.value;
                    e.hidden_since = None;
                }
                // Noticed and not detected: something to search for. An alarm of the
                // same hostile is moved to where it is now [documented: an Alert actor
                // re-centres its search on an enemy that stays within the alert band].
                self.alarm(target, at, det.value, Cause::Noticed, now, t);
            }
            Band::Hidden => {
                if let Some(e) = self.targets.get_mut(&target) {
                    e.band = det.band;
                    e.value = det.value;
                    e.hidden_since.get_or_insert(now);
                }
            }
        }
    }

    /// Every target that is not `measured` any more counts as hidden from `now`: the
    /// engine's round has no measurement of the pair (the actor or the target left the
    /// loaded area, or they are out of detection reach), and nothing but a measurement
    /// moves a target's band, so it would stay detected, and the actor in Combat, for good
    /// (#3390) [design: an actor that perceives nothing of a target does not detect it;
    /// documented: Category:Detection, Combat to Lost "whenever all my targets are
    /// undetected"]. The timers go on as for a target measured hidden. A target a
    /// StartCombat gave it and not measured since is kept as the script set it.
    pub fn lose_unmeasured(&mut self, now: f64, measured: impl Fn(FormId) -> bool) {
        for (id, x) in &mut self.targets {
            // Detected in this round: measured.
            if x.band == Band::Hidden || x.detected == Some(now) || self.scripted.contains(id) || measured(*id) {
                continue;
            }
            x.band = Band::Hidden;
            x.hidden_since.get_or_insert(now);
        }
    }

    /// A Lost actor out of the loaded area, where the AI carries out no search (`fnv_ai`
    /// process.rs), gives each target up once it has been hidden for
    /// [`Timers::forget_target`], as outside an active search [design, #3390: the search's
    /// own bound, [`LOST_SEARCH_STALL_SECONDS`], would have an enemy left in an unloaded
    /// cell hunt the player for 5 more minutes].
    pub fn give_up_search(&mut self, now: f64, t: &Timers) {
        if self.state != State::Lost {
            return;
        }
        let targets = &self.targets;
        let over = |o: &FormId| targets.get(o).is_none_or(|x| x.hidden_since.is_some_and(|h| now - h >= t.forget_target));
        self.alarms.retain(|a| a.cause != Cause::LostTarget || !over(&a.owner));
    }

    /// Put an alarm on the alert list, or refresh the one of the same owner and cause.
    /// An alert actor that hears an event adds its owner to its target list (not
    /// detected) [documented: Category:Detection].
    pub fn alarm(&mut self, owner: FormId, at: [f32; 3], size: f64, cause: Cause, now: f64, t: &Timers) {
        let expires = now + match cause {
            Cause::LostTarget => LOST_SEARCH_STALL_SECONDS,
            _ => t.alarm,
        };
        match self.alarms.iter_mut().find(|a| a.owner == owner && a.cause == cause) {
            Some(a) => {
                a.at = at;
                a.size = size;
                a.refreshed = now;
                a.expires = expires;
            }
            None => self.alarms.push(Alarm { owner, at, size, cause, raised: now, refreshed: now, expires }),
        }
        if self.state.searching() && matches!(cause, Cause::Event(_)) {
            self.targets.entry(owner).or_insert(Target { band: Band::Hidden, value: size, at, detected: None, hidden_since: Some(now) });
        }
    }

    /// Forget an actor (dead, gone, no longer hostile).
    pub fn forget(&mut self, who: FormId) {
        self.targets.remove(&who);
        self.shared.remove(&who);
        self.scripted.remove(&who);
        self.alarms.retain(|a| a.owner != who);
    }

    /// Who it fights: the target detected now with the highest value, or the one it
    /// fought last while that is still detected and either within [`TARGET_KEEP_MARGIN`]
    /// of the highest or within [`TARGET_COMMIT`] of turning to it, the highest not much
    /// nearer ([`TARGET_COMMIT_NEARER`]).
    pub fn combat_target(&self) -> Option<(FormId, &Target)> {
        if self.prefer_player {
            if let Some(t) = self.targets.get(&FormId::PLAYER_REF).filter(|t| t.band == Band::Detected) {
                return Some((FormId::PLAYER_REF, t));
            }
        }
        self.own_target()
    }

    /// [`Awareness::combat_target`] by its own choice, without the player's tweak.
    pub fn own_target(&self) -> Option<(FormId, &Target)> {
        let best = self.targets.iter().filter(|(_, t)| t.band == Band::Detected).max_by(|a, b| a.1.value.total_cmp(&b.1.value)).map(|(id, t)| (*id, t))?;
        let away = |t: &Target| self.at.map_or(f32::INFINITY, |m| (t.at[0] - m[0]).hypot(t.at[1] - m[1]));
        let held = |t: &Target| self.committed && away(best.1) >= away(t) * TARGET_COMMIT_NEARER;
        let kept = self.fighting.and_then(|f| self.targets.get(&f).map(|t| (f, t))).filter(|(_, t)| t.band == Band::Detected && (t.value + TARGET_KEEP_MARGIN >= best.1.value || held(t)));
        Some(kept.unwrap_or(best))
    }

    /// The alarm a searching actor goes to: the latest raised or refreshed.
    pub fn search_alarm(&self) -> Option<&Alarm> {
        self.alarms.iter().max_by(|a, b| a.refreshed.total_cmp(&b.refreshed))
    }

    /// Time alarms and targets out and follow the transitions [documented:
    /// Category:Detection; the timers' lengths: [`Timers`]]. `may_alert`: the actor may
    /// become Alert (its package does not forbid it). Returns the change, if any.
    pub fn settle(&mut self, actor: FormId, now: f64, t: &Timers, may_alert: bool) -> Option<Change> {
        self.alarms.retain(|a| a.expires > now);
        // [design, #2010] Previously detected targets survive the wait to enter Lost
        // and their active search. Group sightings also retain targets (§3 of
        // docs/spec-notes/combat-groups.md), even with inverted timer overrides.
        let shared = &self.shared;
        let pending_lost = self.state == State::Combat && self.combat_target().is_none();
        self.targets.retain(|id, x| {
            (pending_lost && x.detected.is_some())
                || self.alarms.iter().any(|a| a.owner == *id && a.cause == Cause::LostTarget)
                || !x.hidden_since.is_some_and(|h| now - h >= t.forget_target)
                || shared.get(id).is_some_and(|s| now - s < t.forget_target)
        });
        self.shared.retain(|id, _| self.targets.contains_key(id));
        self.scripted.retain(|id| self.targets.contains_key(id));
        // Called by an ally and not fighting yet: it takes the alarm's owner, the first of
        // the foe's side to start, while it detects it (§2).
        if self.fighting.is_none() {
            let called = self.alarms.iter().filter(|a| a.cause == Cause::Ally).max_by(|a, b| a.refreshed.total_cmp(&b.refreshed)).map(|a| a.owner);
            if let Some(owner) = called.filter(|o| self.targets.get(o).is_some_and(|x| x.band == Band::Detected)) {
                self.fighting = Some(owner);
                self.fighting_since = now;
                self.committed = true;
            }
        }
        let detected = self.combat_target().map(|(id, x)| (id, x.value, x.at));
        // A new target starts the commitment; one out of sight is remembered, so the
        // fighter goes back to it when it shows again.
        if let Some((id, ..)) = detected {
            if self.fighting != Some(id) {
                self.fighting = Some(id);
                self.fighting_since = now;
            }
        }
        self.committed = self.fighting.is_some() && now - self.fighting_since < TARGET_COMMIT;
        let latest = self.search_alarm().copied();
        let from = self.state;
        let (to, about, cause, at, value) = match (from, detected, latest) {
            // A hostile detected: Combat, from any state.
            (State::Normal | State::Alert | State::Lost, Some((id, v, at)), _) => (State::Combat, Some(id), None, Some(at), Some(v)),
            (State::Normal, None, Some(a)) if may_alert => (State::Alert, Some(a.owner), Some(a.cause), Some(a.at), Some(a.size)),
            (State::Alert, None, None) => (State::Normal, None, None, None, None),
            (State::Combat, None, _) => {
                if self.targets.is_empty() && latest.is_none() {
                    (State::Normal, None, None, None, None)
                } else if self.targets.iter().all(|(id, x)| x.detected.into_iter().chain(self.shared.get(id).copied()).all(|d| now - d >= t.lost)) {
                    // Every target undetected long enough, by it and by every member of
                    // its group: search where each was last known to be.
                    let lost: Vec<(FormId, Target)> = self.targets.iter().map(|(id, x)| (*id, *x)).collect();
                    for (id, x) in &lost {
                        self.alarm(*id, x.at, x.value, Cause::LostTarget, now, t);
                    }
                    let a = self.search_alarm().copied();
                    (State::Lost, a.map(|a| a.owner), a.map(|a| a.cause), a.map(|a| a.at), a.map(|a| a.size))
                } else {
                    return None;
                }
            }
            // Every target hidden (or gone) and the alert list empty.
            (State::Lost, None, None) if self.targets.values().all(|x| x.band == Band::Hidden) => (State::Normal, None, None, None, None),
            _ => return None,
        };
        if to == from {
            return None;
        }
        self.state = to;
        self.since = now;
        self.next_idle_bark = None;
        if to == State::Combat {
            // [design, #2010] Re-detection ends this search; a later loss starts anew.
            self.alarms.retain(|a| a.cause != Cause::LostTarget);
        }
        if to == State::Combat && from != State::Lost {
            self.combat_began = Some(now);
        }
        if to == State::Normal {
            self.targets.clear();
            self.alarms.clear();
            self.fighting = None;
            self.committed = false;
            self.shared.clear();
            self.scripted.clear();
        }
        Some(Change { actor, from, to, time: now, about, cause, at, value })
    }
}

/// A detection event waiting to be heard: the engine measures who hears it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectionEvent {
    pub owner: FormId,
    pub at: [f32; 3],
    /// Its sound level, in place of the action sound in the formula.
    pub level: f64,
    pub kind: EventType,
}

/// Every actor's [`Awareness`], the events not yet heard and the changes not yet told.
/// Saves keep the actors in a fight and `forced` (#971); the rest the engine measures again.
#[derive(Debug, Clone, Default)]
pub struct Awarenesses {
    pub actors: BTreeMap<FormId, Awareness>,
    pub events: Vec<DetectionEvent>,
    /// State changes since the engine last took them.
    pub changes: Vec<Change>,
    /// Detection topic lines said: the change's or idle topic, the speaker and the line.
    pub barks: Vec<(&'static str, FormId, Option<FormId>)>,
    /// Detection lines said within the last cooldown, for the group cooldown.
    pub said: Vec<Said>,
    /// Detection lines not said for the group cooldown: the topic, the actor held back
    /// and the one that had said a line of that kind.
    pub held_back: Vec<(&'static str, FormId, FormId)>,
    pub timers: Option<Timers>,
    /// How close each actor wants to be to its target, and how far it may be before it
    /// has to close in again (its weapon's range, its reach), game units: the engine's
    /// combat AI says, the AI that moves actors closes in (`fnv_ai::alert`). Not set: it
    /// holds its ground.
    pub engage: BTreeMap<FormId, (f32, f32)>,
    /// Actors aiming down their sights now (the engine's combat AI says): the AI that
    /// moves actors has them walk, not run (`fnv_ai::alert`).
    pub aiming: std::collections::BTreeSet<FormId>,
    /// Fighters crouched to fire (the engine's combat AI says, docs/design/combat-ai.md
    /// §14.2): `Game::sneaks` reads it while they are in Combat; the AI that moves actors
    /// stands them up when it sets off (`fnv_ai::alert`).
    pub crouching: std::collections::BTreeSet<FormId>,
    /// The share of the running speed each actor keeps (the engine's combat AI says):
    /// below 1 with a two-handed weapon drawn, or for a creature whose own run is slower
    /// (the owner's playtest list 5 items 61 and 25). Not set: 1.
    pub pace: BTreeMap<FormId, f32>,
    /// (actor, target) pairs StartCombat made hostile whatever the records say, until
    /// StopCombat.
    pub forced: std::collections::BTreeSet<(FormId, FormId)>,
    /// Combat groups' state: strengths, posts, flights, searches (`crate::combat_group`).
    pub tactics: crate::combat_group::Tactics,
    rng: u64,
}

impl Awarenesses {
    /// The share of the running speed `actor` keeps ([`Self::pace`]).
    pub fn pace_of(&self, actor: FormId) -> f32 {
        self.pace.get(&actor).copied().unwrap_or(1.0)
    }

    pub fn state(&self, actor: FormId) -> State {
        self.actors.get(&actor).map_or(State::Normal, |a| a.state)
    }

    pub fn get(&self, actor: FormId) -> Option<&Awareness> {
        self.actors.get(&actor)
    }

    /// Whether any actor in a combat state has `who` on its target list or alert list.
    pub fn hunted(&self, who: FormId) -> bool {
        self.actors.values().any(|a| a.state.in_combat() && (a.targets.contains_key(&who) || a.alarms.iter().any(|x| x.owner == who)))
    }

    /// Take the changes since the last call.
    pub fn take_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    /// A number in [0, 1) from the module's own sequence (the bark timer), so that the
    /// game's random numbers stay as they were.
    pub(crate) fn unit(&mut self) -> f64 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Where a reference is now, game space.
pub(crate) fn position(g: &Game, r: FormId) -> Option<[f32; 3]> {
    g.world.peek(r).map(|s| s.pos).or_else(|| g.world.placed.get(r).map(|p| p.pos))
}

/// What [`Game::would_attack`]'s rule decided for a pair, and by which of its rules: the
/// first that held, in the order the rule asks them. Only [`Hostile::No`] is not hostile.
/// For the log of a fight's start ([`Game::hostility_text`]): a player's report showed an
/// actor going for the companion with nothing in the log saying why (#3335).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hostile {
    No,
    /// StartCombat made the pair hostile.
    Forced,
    /// `actor` holds a grudge against `other` (it was attacked, `crate::provocation`).
    Grudge,
    /// `other` is the player's teammate, and `actor` would attack the player.
    AsAgainstThePlayer,
    /// `actor` is the player's teammate and `other` would attack the player.
    DefendsThePlayer,
    /// `actor` is a passive teammate and `other` fights the player's side.
    FightsThePlayersSide,
    /// The player's crimes made `actor` an enemy (GetActorCrimePlayerEnemy).
    CrimeEnemy,
    /// The player is an enemy of one of `actor`'s factions (GetActorFactionPlayerEnemy).
    FactionFlag,
    /// `actor`'s Aggression against its relation to `other` (`GetFactionRelation`'s codes:
    /// 0 neutral, 1 enemy, 2 ally, 3 friend).
    Aggression { aggression: i64, relation: i64 },
}

impl Hostile {
    pub fn is(self) -> bool {
        self != Hostile::No
    }

    fn of(hostile: bool, why: Hostile) -> Hostile {
        if hostile {
            why
        } else {
            Hostile::No
        }
    }
}

/// What [`Game::kin`] reads of one actor (kept by `detection::Round::kin`).
#[derive(Debug, Clone)]
pub(crate) struct KinFacts {
    base: Option<FormId>,
    factions: Vec<FormId>,
    /// One of its factions names reactions (not only utility factions).
    fights: bool,
}

impl Game<'_> {
    fn awareness_timers(&mut self) -> Timers {
        match self.awareness.timers {
            Some(t) => t,
            None => {
                let t = Timers::read(self);
                self.awareness.timers = Some(t);
                t
            }
        }
    }

    /// Whether `actor` would attack `other` on detecting it: what its records say
    /// [documented: GECK wiki GetFactionCombatReaction: "Enemy actors are attacked on sight
    /// by Aggressive and Very Aggressive actors", "Neutrals will be attacked by Very
    /// Aggressive actors", allies and friends are not], the factions combined as
    /// `GetFactionRelation` does; a Frenzied actor attacks anyone [hypothesis]. Neither may
    /// be dead, disabled, unconscious or restrained, and `actor` not the player's
    /// teammate when `other` is the player. Actors with a faction in common, or of the
    /// same base form, never attack each other [engineering choice: without it Very
    /// Aggressive creatures of one kind, neutral to each other, would fight].
    ///
    /// Nobody attacks a ghost (SetGhost 1): "Enemies of this calling ref will not go into
    /// combat or detect an actor with this flag" [documented: GECK wiki SetGhost notes];
    /// that holds for a ghosted player too, the note naming no exception (the player's
    /// exception is about damage only, `Game::is_immune`). Not even StartCombat or a
    /// grudge makes a ghost a target [hypothesis: the note names no exception for them].
    /// A ghost still attacks others ("Can inflict damage").
    pub fn would_attack(&mut self, actor: FormId, other: FormId) -> bool {
        !self.is_ghost(other) && self.hostile_to(actor, other)
    }

    /// Whether `r` is a ghost now: SetGhost 1, or its base is flagged Is Ghost (CREA ACBS
    /// bit 29, from the definition supplying its base data) and no SetGhost 0 cleared it
    /// (docs/spec-notes/actor-flags.md §3).
    pub fn is_ghost(&self, r: FormId) -> bool {
        let e = self.extra.get(&r);
        if e.is_some_and(|e| e.ghost) {
            return true;
        }
        if e.is_some_and(|e| e.ghost_off) {
            return false;
        }
        crate::functions::base_of(self, r).is_some_and(|b| self.base_is_ghost(b, r))
    }

    /// Whether `base` (of the reference `r`) is flagged Is Ghost, through the definition
    /// supplying its base data. For callers that know the base while the game's world
    /// does not hold the reference (the engine, outside `with_world`).
    pub fn base_is_ghost(&self, base: FormId, r: FormId) -> bool {
        // Only creatures carry the flag: an NPC base skips the template walk (detection
        // asks this for every pair of actors).
        if self.data.actors.get(&base).is_some_and(|d| &d.sig == b"NPC_") {
            return false;
        }
        self.template_source(base, r, |d| d.template_base)
            .is_some_and(|(id, d)| &d.sig == b"CREA" && self.acbs_flag(id, d, crate::actor_values::acbs::IS_GHOST, |d| d.is_ghost))
    }

    /// `target` is a ghost that `detector` would otherwise attack: such an enemy does not
    /// detect it [documented: GECK wiki SetGhost notes, "Enemies of this calling ref will
    /// not go into combat or detect an actor with this flag"]. Others (and the player)
    /// detect a ghost as anyone.
    pub fn ghost_hidden_from(&mut self, detector: FormId, target: FormId) -> bool {
        self.is_ghost(target) && self.hostile_to(detector, target)
    }

    /// [`Game::would_attack`] without the ghost rule; kept for the rest of a detection
    /// round while one runs ([`Game::begin_detection_round`]).
    fn hostile_to(&mut self, actor: FormId, other: FormId) -> bool {
        if let Some(&h) = self.detection_round.as_ref().and_then(|r| r.hostile.as_ref()?.get(&(actor, other))) {
            return h;
        }
        let h = self.hostile_to_now(actor, other);
        if let Some(kept) = self.detection_round.as_mut().and_then(|r| r.hostile.as_mut()) {
            kept.insert((actor, other), h);
        }
        h
    }

    fn hostile_to_now(&mut self, actor: FormId, other: FormId) -> bool {
        // What the rule reads that is looked up with `&mut`: both actors' Aggression (the
        // rule asks it of `actor`, and of `other` when it asks how `other` stands towards
        // the player), and in a round their kinship facts, kept for it.
        let (of_actor, of_other) = (self.aggression_kept(actor), self.aggression_kept(other));
        self.keep_kin_facts(actor);
        self.keep_kin_facts(other);
        let aggression = |r: FormId| (r == actor).then_some(of_actor).or((r == other).then_some(of_other));
        self.hostility(actor, other, &aggression).is_some_and(Hostile::is)
    }

    /// Why `actor` would attack `other` now, for the log ([`Hostile`]); `None` when it
    /// would not. Worked out afresh, outside any detection round's kept answers.
    pub fn hostility_reason(&mut self, actor: FormId, other: FormId) -> Option<Hostile> {
        if self.is_ghost(other) {
            return None;
        }
        let (of_actor, of_other) = (self.aggression_kept(actor), self.aggression_kept(other));
        let aggression = |r: FormId| (r == actor).then_some(of_actor).or((r == other).then_some(of_other));
        self.hostility(actor, other, &aggression).filter(|h| h.is())
    }

    /// [`Game::hostility_reason`] in words, for the `hostility:` log line: the rule, and for a
    /// faction's flag which factions and since when; for a teammate, the reason against
    /// the player too.
    pub fn hostility_text(&mut self, actor: FormId, other: FormId) -> Option<String> {
        let why = self.hostility_reason(actor, other)?;
        let name = |g: &Game, r: FormId| if r == FormId::PLAYER_REF { "the player".to_string() } else { g.data.name(r) };
        Some(match why {
            Hostile::No => unreachable!("filtered"),
            Hostile::Forced => "StartCombat made them hostile".to_string(),
            Hostile::Grudge => {
                let other_name = name(self, other);
                match self.provocations.grudges.get(&(actor, other)).map(|g| g.why) {
                    Some(crate::provocation::Why::Assist { victim }) => {
                        format!("it holds a grudge against {other_name} (helps {} against {other_name})", name(self, victim))
                    }
                    Some(crate::provocation::Why::Aggro) => {
                        format!("it holds a grudge against {other_name} (stayed inside its aggro radius)")
                    }
                    Some(crate::provocation::Why::Assault) | None => {
                        format!("it holds a grudge against {other_name} (it was attacked)")
                    }
                }
            }
            Hostile::AsAgainstThePlayer => {
                let inner = self.hostility_text(actor, FormId::PLAYER_REF).unwrap_or_else(|| "?".to_string());
                format!("{} is the player's teammate, and it would attack the player: {inner}", name(self, other))
            }
            Hostile::DefendsThePlayer => {
                let inner = self.hostility_text(other, FormId::PLAYER_REF).unwrap_or_else(|| "?".to_string());
                format!("it is the player's teammate, and {} would attack the player: {inner}", name(self, other))
            }
            Hostile::FightsThePlayersSide => format!("a passive teammate, and {} fights the player's side", name(self, other)),
            Hostile::CrimeEnemy => {
                let since = self.player.crime_enemy_since.get(&actor).copied();
                let days = since.zip(self.game_days()).map_or(String::new(), |(d, now)| format!(", {:.2} days ago", now - d));
                format!("the player's crimes made it an enemy{days}")
            }
            Hostile::FactionFlag => {
                let now = self.game_days();
                let flags: Vec<String> = crate::functions::actor::factions_of(self, actor)
                    .into_iter()
                    .filter(|f| self.player.enemy_of.contains(f))
                    .map(|f| {
                        let by = match (self.player.enemy_of_crimes.get(&f), self.player.enemy_of_script.contains(&f)) {
                            (Some(&d), _) => now.map_or("a crime's flag".to_string(), |n| format!("a crime's flag, {:.2} days old", n - d)),
                            (None, true) => "a script's flag (or one kept from a save made before crimes were dated)".to_string(),
                            (None, false) => "flagged".to_string(),
                        };
                        format!("{} ({by})", name(self, f))
                    })
                    .collect();
                format!("the player is an enemy of its faction {}", flags.join(", "))
            }
            Hostile::Aggression { aggression, relation } => {
                let rel = match relation {
                    1 => "enemy",
                    2 => "ally",
                    3 => "friend",
                    _ => "neutral",
                };
                format!("its Aggression {aggression} against its relation {rel} ({relation}) to {}", name(self, other))
            }
        })
    }

    /// Whether `r` is a fixture that never fights: a creature flagged Immobile whose AI
    /// data (Aggression Unaggressive, Assistance Helps Nobody) never starts or joins a
    /// fight and that carries no weapon. It cannot move, turn to a foe or hurt one, so
    /// it never takes a combat target, never answers an alarm or a call for help and is
    /// never hostile ([`Game::hostility`], [`Game::start_combat`],
    /// [`Game::settle_awareness`]), whatever the player did to it or to its factions,
    /// and the engine never turns it (`nvforever` residents.rs). [documented: GECK wiki
    /// Stats Tab for Creatures, Immobile; AI Data, Aggression "Unaggressive: will not
    /// initiate combat", Assistance "Helps Nobody"; observed: in FalloutNV.esm these are
    /// the placed MrHouse and MrHouseOFFLINE (his screens), NVCRMrHouse (his life-support
    /// chamber) and NVCRFestus (a ghosted robot), YesManMainframe too though it is placed
    /// nowhere; the turrets, also Immobile and Unaggressive, carry their guns (CNTO), and
    /// no other placed actor of the masters is one (tests/fixtures_data.rs); hypothesis: that the original lets such an actor take no part in a
    /// fight, as the owner ruled for the screens and the chamber, issue #2431.] The
    /// screens are Invulnerable by their records ([`Game::is_immune`]); the chamber is a
    /// ghost while sealed (its script's SetGhost 1) and House's body killable once the
    /// terminal unseals it (owner ruling 2026-11-13).
    pub fn is_fixture(&self, r: FormId) -> bool {
        if r == FormId::PLAYER_REF
            || !self
                .base_data_source(r)
                .is_some_and(|(id, d)| &d.sig == b"CREA" && self.acbs_flag(id, d, crate::actor_values::acbs::IMMOBILE, |d| d.immobile))
        {
            return false;
        }
        let calm = self.def_following(r, |d| d.template_ai).is_some_and(|d| d.aggression == Some(0) && d.assistance == Some(0));
        // Armed: a weapon in its inventory, or, before it has one (not loaded yet), a
        // weapon or leveled item list among its base's items (CNTO, from the definition
        // supplying its inventory): the turrets carry their guns so.
        let armed = || match self.world.peek(r).and_then(|s| s.inventory.as_ref()) {
            Some(inv) => inv.iter().any(|(item, n)| n > 0 && self.data.sig(item).as_ref() == Some(b"WEAP")),
            None => crate::functions::base_of(self, r)
                .and_then(|b| self.template_source(b, r, |d| d.template_inventory))
                .and_then(|(id, _)| self.world.forms.entries(id))
                .is_some_and(|items| items.iter().any(|e| matches!(self.data.sig(e.item).as_ref(), Some(b"WEAP") | Some(b"LVLI")))),
        };
        calm && !armed()
    }

    /// Whether `SetCombatDisabled 1` keeps `r` out of combat: the actor never enters
    /// combat with other actors (though others may still attack it, and UseWeapon /
    /// Use Weapon packages still work); reset on every load [documented: GECK wiki
    /// SetCombatDisabled].
    pub fn combat_disabled(&self, r: FormId) -> bool {
        self.gaps.combat_disabled.contains(&r)
    }

    /// The rule of [`Game::hostile_to`], read-only: `aggression` gives an actor's
    /// Aggression; `None` when it gives none for an actor the rule asks about. Another
    /// player's actor (co-op) is never hostile by this rule.
    fn hostility(&self, actor: FormId, other: FormId, aggression: &dyn Fn(FormId) -> Option<i64>) -> Option<Hostile> {
        if actor == other || actor == FormId::PLAYER_REF || self.is_coop_actor(actor) || !self.world.is_enabled(actor).unwrap_or(false) {
            return Some(Hostile::No);
        }
        // A fixture or combat-disabled actor never fights (issue #2431; SetCombatDisabled).
        if self.is_fixture(actor) || self.combat_disabled(actor) {
            return Some(Hostile::No);
        }
        let out = |g: &Game, r: FormId| g.extra.get(&r).is_some_and(|e| e.dead || e.out_cold() || e.restrained);
        // A knocked-down player stays a target (unconscious.md §5.4) [design].
        let player_down = other == FormId::PLAYER_REF && self.extra.get(&other).is_some_and(|e| e.knocked_out.is_some() && !e.unconscious && !e.restrained);
        if out(self, actor) || (out(self, other) && !player_down) || self.is_dead(other) {
            return Some(Hostile::No);
        }
        if self.awareness.forced.contains(&(actor, other)) {
            return Some(Hostile::Forced);
        }
        if self.provocations.holds(actor, other) {
            return Some(Hostile::Grudge);
        }
        // Another player of a co-op game is attacked whenever the player would be
        // (`crate::coop`) [design].
        if self.is_coop_actor(other) {
            return self.hostility(actor, FormId::PLAYER_REF, aggression);
        }
        // The player's teammates "act and react as if in the player faction for the
        // purposes of combat. Other actors will treat the actor as if he were in the
        // player faction" [documented: GECK wiki SetPlayerTeammate]: a teammate never
        // attacks the player or another teammate; others attack a teammate when they would
        // attack the player; a teammate attacks whoever would attack the player, and
        // whoever its own records say [design: the companion keeps its own enmities, as
        // Boone's for the Legion] (`crate::followers`).
        let (mate, other_mate) = (self.is_teammate(actor), self.is_teammate(other));
        if mate && (other_mate || other == FormId::PLAYER_REF) {
            return Some(Hostile::No);
        }
        if other_mate {
            return Some(Hostile::of(self.hostility(actor, FormId::PLAYER_REF, aggression)?.is(), Hostile::AsAgainstThePlayer));
        }
        // A companion set passive on the wheel (`FollowerSwitchAggressive` 0) leaves alone
        // whoever does not fight the player's side, its own enmities too; a grudge (a hit)
        // counted above [hypothesis: the wheel's Passive "follows your lead",
        // docs/spec-notes/companion-wheel.md §4].
        if mate && self.is_passive_teammate(actor) {
            return Some(Hostile::of(self.fights_the_players_side(other), Hostile::FightsThePlayersSide));
        }
        if mate && self.hostility(other, FormId::PLAYER_REF, aggression)?.is() {
            return Some(Hostile::DefendsThePlayer);
        }
        if other == FormId::PLAYER_REF {
            if self.extra.get(&actor).is_some_and(|e| e.teammate) {
                return Some(Hostile::No);
            }
            // The player's crimes (`crate::crime`): its own crime enmity, else a flag on
            // one of its factions.
            if self.crime_enemy(actor) {
                return Some(if self.player.crime_enemies.contains(&actor) { Hostile::CrimeEnemy } else { Hostile::FactionFlag });
            }
        } else {
            if !self.world.is_enabled(other).unwrap_or(false) {
                return Some(Hostile::No);
            }
            if self.kin_in_round(actor, other) {
                return Some(Hostile::No);
            }
            // "Defensive Combat": "The actor will not attack an NPC or creature, unless he
            // is attacked first" [documented: GECK wiki Packages, Flags tab]; being
            // attacked is a grudge, which counts above.
            if self.package_has(actor, flags::DEFENSIVE_COMBAT) {
                return Some(Hostile::No);
            }
        }
        let relation = self.relation_in_round(actor, other);
        let aggression = aggression(actor)?;
        let hostile = match aggression {
            a if a >= 3 => true,
            2 => relation != 2 && relation != 3,
            1 => relation == 1,
            _ => false,
        };
        Some(Hostile::of(hostile, Hostile::Aggression { aggression, relation }))
    }

    /// Whether `actor`'s current package keeps it out of Alert: the package flag "No
    /// Combat Alert" [hypothesis: what the flag's name says; no source describes it].
    fn no_combat_alert(&mut self, actor: FormId) -> bool {
        self.current_package(actor).and_then(|p| self.package_def(&p)).is_some_and(|d| d.has(flags::NO_COMBAT_ALERT))
    }

    /// Whether `actor`'s current package has the flag "Continue During Combat" in a way
    /// that may keep it on its package while it is in a combat state (Alert, Combat,
    /// Lost). 399 of FalloutNV.esm's 4,163 packages set it [observed 2026-10-11]. When
    /// it does hold the actor is the AI's to decide by the package type and where the
    /// actor and its target are (`fnv_ai` alert, docs/design/alert.md §5.2, #2155): only
    /// on the way of a Travel-kind package, and on a Guard while the target is outside
    /// [`Game::guard_combat_radius`]. A held actor keeps carrying its package out instead
    /// of searching or closing in; its state, weapon and barks are as for any other actor.
    ///
    /// Not for a teammate of the player on a Follow package: it fights as any other
    /// actor does (closes in, its combat style's radius holding it) and follows again
    /// once the fight is over. Every follower's Follow package sets the flag [observed:
    /// FollowersVeronicaFollowPlayerDEFAULT, FollowersBooneFollowPlayerDEFAULT], and in
    /// the original a melee companion goes after its targets [observed: the owner's
    /// playtest 2026-11-04, #1534]; read as "follow on", Veronica only struck enemies
    /// that came to the player.
    pub fn continues_during_combat(&mut self, actor: FormId) -> bool {
        let Some(p) = self.current_package(actor) else { return false };
        let Some((on, follow)) = self.package_def(&p).map(|d| (d.has(flags::CONTINUE_DURING_COMBAT), d.ty == crate::package::PackageType::Follow)) else { return false };
        on && !(follow && self.is_teammate(actor))
    }

    /// How far from its post a Guard package with Continue During Combat lets its actor
    /// pursue a target: the package's location radius × fCombatGuardRadiusMult (1.5) or
    /// fCombatGuardRadiusMin (384), whichever is higher [documented: GECK wiki export,
    /// "Combat Package Flags"; the defaults are FalloutNV.esm's, read from the load order
    /// when a plugin sets them; observed: two Guard packages held their trooper so,
    /// docs/handoff/observer-checks.md check 106].
    pub fn guard_combat_radius(&self, def: &crate::package::PackageDef) -> f32 {
        let mult = crate::combat::setting(self, "fCombatGuardRadiusMult", 1.5) as f32;
        let min = crate::combat::setting(self, "fCombatGuardRadiusMin", 384.0) as f32;
        let radius = def.location.filter(|l| !matches!(l.kind, crate::package::LocationKind::InCell(_))).map_or(0, |l| l.radius.max(0));
        (radius as f32 * mult).max(min)
    }

    /// The engine measured `target` from `detector` at `at` (game space): when the
    /// detector would attack the target, the measurement feeds its detection state.
    /// What others do is no threat; a target it no longer would attack is forgotten.
    pub fn observe_threat(&mut self, detector: FormId, target: FormId, det: &Detection, at: [f32; 3]) {
        if !self.would_attack(detector, target) {
            if let Some(a) = self.awareness.actors.get_mut(&detector) {
                a.forget(target);
            }
            return;
        }
        if self.neutral_out_of_reach(detector, target, at) {
            return;
        }
        let t = self.awareness_timers();
        let now = self.time;
        // Nothing to keep for a hidden hostile the actor knows nothing of.
        if det.band == Band::Hidden && !self.awareness.actors.contains_key(&detector) {
            return;
        }
        let me = position(self, detector);
        let a = self.awareness.actors.entry(detector).or_default();
        a.at = me.or(a.at);
        a.observe(target, det, at, now, &t);
    }

    /// `detector` would attack `target` only as a Very Aggressive actor attacks a
    /// Neutral (no grudge, no forced fight, not the player), it has an aggro radius
    /// that its package allows, and `at` (where it measured the target) is outside that
    /// radius, and it is not already fighting `target`: it leaves the target alone for
    /// now [hypothesis, #2547: HELIOS One's guard dogs (CrDogNCRAggressive, Very
    /// Aggressive, aggro radius 2,000 on) went for the NCR troopers 2,800 units off,
    /// which the owner never saw in the original; the radius is the one distance the
    /// records give such an actor; observer checks 144 and 163]. A fight with a third
    /// party (such as the player) does not drop the bound for unrelated Neutral
    /// bystanders [design, #3459].
    /// The player is left to the aggro radius rules ([`Game::aggro_tick`]) and the
    /// on-sight rule as before.
    pub(crate) fn neutral_out_of_reach(&mut self, detector: FormId, target: FormId, at: [f32; 3]) -> bool {
        // A fight once begun with `target` goes on wherever `target` runs [design].
        if target == FormId::PLAYER_REF
            || detector == FormId::PLAYER_REF
            || self.awareness.get(detector).is_some_and(|a| matches!(a.state, State::Combat | State::Lost) && a.targets.contains_key(&target))
        {
            return false;
        }
        if self.awareness.forced.contains(&(detector, target)) || self.provocations.holds(detector, target) {
            return false;
        }
        if self.aggression_kept(detector) != 2 || self.relation_in_round(detector, target) != 0 {
            return false;
        }
        let Some(radius) = self.aggro_radius(detector) else { return false };
        if !self.package_allows_aggro(detector) {
            return false;
        }
        let Some(me) = position(self, detector) else { return false };
        let d = ((me[0] - at[0]).powi(2) + (me[1] - at[1]).powi(2) + (me[2] - at[2]).powi(2)).sqrt() as f64;
        d > radius
    }

    /// Raise a detection event for the engine to measure (CreateDetectionEvent, an
    /// impact, an explosion).
    pub fn queue_detection_event(&mut self, ev: DetectionEvent) {
        self.awareness.events.push(ev);
    }

    /// Take the events raised since the last call.
    pub fn take_detection_events(&mut self) -> Vec<DetectionEvent> {
        std::mem::take(&mut self.awareness.events)
    }

    /// `detector` measured `ev` at `det`: one that would attack the event's owner, is not
    /// in Combat and detects it (above −20) is alarmed [documented: Category:Detection,
    /// "Actors ignore the event if they would not act aggressively toward its owner, or if
    /// they are already in Combat"]. Returns whether it was.
    pub fn hear_event(&mut self, detector: FormId, ev: &DetectionEvent, det: &Detection) -> bool {
        if det.band == Band::Hidden || self.awareness.state(detector) == State::Combat || !self.would_attack(detector, ev.owner) {
            return false;
        }
        if self.neutral_out_of_reach(detector, ev.owner, ev.at) {
            return false;
        }
        let t = self.awareness_timers();
        let now = self.time;
        self.awareness.actors.entry(detector).or_default().alarm(ev.owner, ev.at, det.value, Cause::Event(ev.kind), now, &t);
        true
    }

    /// `actor` was hurt by `by`, who stands at `at`: an alarm at the attacker's position
    /// when the actor would attack it [documented: Normal → Alert when a hostile hurts the
    /// detector, an event at the hostile's position]. Returns whether it was alarmed.
    pub fn hurt_by(&mut self, actor: FormId, by: FormId, at: [f32; 3]) -> bool {
        if !self.would_attack(actor, by) {
            return false;
        }
        let t = self.awareness_timers();
        let now = self.time;
        self.awareness.actors.entry(actor).or_default().alarm(by, at, 0.0, Cause::Hurt, now, &t);
        true
    }

    /// After a round's pairs, before [`Game::settle_awareness`], with the old pairs
    /// forgotten (`Detections::forget_older_than`): a target whose pair is not in the table
    /// counts as hidden ([`Awareness::lose_unmeasured`]), and a Lost actor that is not
    /// among `drawn` (the actors this round measured: the loaded area) gives its search up
    /// in time ([`Awareness::give_up_search`]) (#3390). Only the engine's rounds call this,
    /// and a round measures every pair within reach (none waits its turn): without them
    /// (headless runs) nothing is ever measured, and that hides nobody.
    ///
    /// Left as they are: actors with their AI off (SetActorsAI 0, `tai`), which measure
    /// nothing and do not search; and the player as a target while nobody's sight of the
    /// player is measured (`tdetect`, the test aid that leaves the player unseen).
    pub fn lose_unmeasured(&mut self, drawn: &BTreeSet<FormId>) {
        let t = self.awareness_timers();
        let now = self.time;
        let ids: Vec<FormId> = self.awareness.actors.keys().copied().filter(|&id| !self.ai_off(id)).collect();
        let detection = &self.detection;
        for id in ids {
            let Some(a) = self.awareness.actors.get_mut(&id) else { continue };
            a.lose_unmeasured(now, |o| (o == FormId::PLAYER_REF && !detection.measures_player) || detection.pairs.contains_key(&(id, o)));
            if !drawn.contains(&id) {
                a.give_up_search(now, &t);
            }
        }
    }

    /// After a round of measurements: every actor's alarms and targets time out and its
    /// state changes as documented; the changes are kept for the engine
    /// ([`Awarenesses::take_changes`]) and the Detection topics are said
    /// ([`Game::detection_bark`]). Actors that are gone or dead are dropped.
    pub fn settle_awareness(&mut self) {
        let t = self.awareness_timers();
        let now = self.time;
        self.share_sightings();
        let ids: Vec<FormId> = self.awareness.actors.keys().copied().collect();
        for id in ids {
            if !self.present(id) || self.is_dead(id) || self.extra.get(&id).is_some_and(|e| e.dead) {
                self.awareness.actors.remove(&id);
                self.provocations.calm(id);
                self.awareness.tactics.calm(id);
                continue;
            }
            // A fixture or combat-disabled actor keeps no targets or alarms, from whatever
            // gave them (an ally's call, a hit, a crime): it stays Normal
            // (`Game::is_fixture`, `Game::combat_disabled`, issue #2431).
            if self.is_fixture(id) || self.combat_disabled(id) {
                if let Some(a) = self.awareness.actors.remove(&id).filter(|a| a.state != State::Normal) {
                    self.awareness.changes.push(Change { actor: id, from: a.state, to: State::Normal, time: now, about: None, cause: None, at: None, value: None });
                }
                self.awareness.forced.retain(|(a, _)| *a != id);
                self.provocations.calm(id);
                self.awareness.tactics.calm(id);
                continue;
            }
            // Targets and alarm owners that died or went are forgotten, and so are those
            // knocked out: no longer a threat until they are up and seen again
            // (docs/spec-notes/unconscious.md §3) [design, owner request 2026-10-17]. A
            // dead player is forgotten too, whether or not the engine measures it again:
            // a fighter whose targets are all dead goes back to Normal and says
            // CombatToNormal, not CombatToLost [documented: Category:Detection, "Combat
            // --> Normal … no targets in my combat list (i.e. they're all dead …)";
            // docs/spec-notes/combat-dialogue.md §5].
            let gone: Vec<FormId> = {
                let a = &self.awareness.actors[&id];
                let gone = |o: FormId| if o == FormId::PLAYER_REF { self.is_dead(o) } else { !self.present(o) || self.is_dead(o) || self.knockout(o).is_some() };
                a.targets.keys().copied().chain(a.alarms.iter().map(|x| x.owner)).filter(|&o| gone(o)).collect()
            };
            let may_alert = !self.no_combat_alert(id);
            let a = self.awareness.actors.get_mut(&id).expect("listed");
            for o in gone {
                a.forget(o);
            }
            let change = a.settle(id, now, &t, may_alert);
            if self.tweaks.hostiles_prefer_player {
                self.prefer_player(id, change.is_some_and(|c| c.to == State::Combat));
            }
            let a = self.awareness.actors.get_mut(&id).expect("listed");
            let idle_due = a.state.idle_topic().is_some() && a.next_idle_bark.is_some_and(|due| now >= due);
            let schedule = a.state.idle_topic().is_some() && (a.next_idle_bark.is_none() || idle_due);
            if schedule {
                let wait = t.idle_bark.0 + (t.idle_bark.1 - t.idle_bark.0).max(0.0) * self.awareness.unit();
                self.awareness.actors.get_mut(&id).expect("listed").next_idle_bark = Some(now + wait);
            }
            let a = &self.awareness.actors[&id];
            let (state, about) = (a.state, a.search_alarm().map(|x| x.owner).or(a.combat_target().map(|x| x.0)));
            if let Some(c) = change {
                // Back in Normal: the fights it took up because it was hit are over.
                if c.to == State::Normal {
                    self.provocations.calm(id);
                    self.awareness.tactics.calm(id);
                }
                // When it first fought in this fight, for the alarms' first starter.
                if c.to == State::Combat {
                    self.awareness.tactics.searched.retain(|(actor, _), _| *actor != id);
                    self.awareness.tactics.started.entry(id).or_insert(now);
                }
                self.awareness.changes.push(c);
                if let Some(topic) = c.from.topic_to(c.to) {
                    self.detection_bark(id, c.about, topic);
                }
            } else if idle_due {
                if let Some(topic) = state.idle_topic() {
                    self.detection_bark(id, about, topic);
                }
            }
            if state == State::Normal && self.awareness.actors.get(&id).is_some_and(|a| a.alarms.is_empty() && a.targets.is_empty()) {
                self.awareness.actors.remove(&id);
            }
        }
    }

    /// `actor` says its line of the Detection topic `topic` about `about` (the
    /// conditions' target), when the load order has the topic, the actor is not already
    /// speaking and no conversation or menu holds the game [documented: Detection Tab],
    /// and neither it nor a member of its group near it has said a line of the same kind
    /// within the cooldown ([`Timers::bark_cooldown`], [`Game::bark_mates`]). A warning
    /// (GuardTrespass) has no cooldown: each of the warnings, `fAITrespassWarningTimer`
    /// apart, is said (`crate::aggro`, `crate::crime`) [design: the 15 s cooldown kept the
    /// second warning silent]; and it cuts a hello the actor is still saying [design: a
    /// Gomorrah guard's hello as the player came near silenced his first warning, smoke
    /// `warning-guard-draws`, 2026-10-25].
    pub fn detection_bark(&mut self, actor: FormId, about: Option<FormId>, topic: &'static str) {
        let warning = topic == "GuardTrespass";
        let hello_only = self.speech.iter().filter(|s| s.speaker == actor).all(|s| s.hello) && !self.dialogue.as_ref().is_some_and(|c| c.actor == actor);
        if self.menu.is_some() || self.dialogue.is_some() || (self.is_talking(actor) && !(warning && hello_only)) {
            return;
        }
        let data = self.data;
        let Some(t) = data.by_edid(topic).filter(|t| data.topic_defs.contains_key(t)) else { return };
        let cooldown = self.awareness_timers().bark_cooldown;
        let now = self.time;
        let kind = bark_kind(topic);
        let at = position(self, actor);
        self.awareness.said.retain(|s| now - s.time < cooldown);
        let said = self.awareness.said.clone();
        if let Some(by) = held_back_by(&said, kind, at, |other| self.bark_mates(actor, other)).filter(|_| !warning) {
            self.awareness.held_back.push((topic, actor, by));
            return;
        }
        let line = self.say(actor, about, t, false);
        if line.is_some() {
            self.awareness.said.push(Said { kind, speaker: actor, at: at.unwrap_or_default(), time: now });
            // A nearby group-mate answers it (FleeResponse, StartCombatResponse …;
            // `crate::combat_lines`).
            self.queue_response(actor, topic);
        }
        self.awareness.barks.push((topic, actor, line));
    }

    /// Whether two actors are of one group for the Detection lines' cooldown: the same
    /// actor, the same base form, or a faction in common [design: the rule by which
    /// [`Game::would_attack`] keeps them from fighting each other; the game's combat
    /// groups are not built].
    pub fn bark_mates(&self, a: FormId, b: FormId) -> bool {
        a == b || self.kin(a, b)
    }

    /// Whether two actors are of one kind, so that they never fight each other: placed
    /// from the same base form, or with a faction in common [engineering choice: without
    /// it Very Aggressive creatures of one kind, neutral to each other, would fight].
    /// Since 2026-10-12 (combat-ai.md §12.4) never when the factions of either name the
    /// other's an enemy, and a faction in common that names no reactions of its own (a
    /// utility faction: GeneralIdleFaction, voice and dialogue factions) counts only when
    /// one of the two has no faction that names reactions [design: NCR troopers and Fiends
    /// are both in GeneralIdleFaction, which names none, and their own factions,
    /// NCRFactionNV and FiendsFactionNV, name 23 and 6 others, but not each other:
    /// observed].
    pub fn kin(&self, a: FormId, b: FormId) -> bool {
        if a == b || self.as_player(a) == FormId::PLAYER_REF || self.as_player(b) == FormId::PLAYER_REF {
            return false;
        }
        self.kin_with(a, b, &self.kin_facts(a), &self.kin_facts(b))
    }

    /// [`Game::kin`] from the facts a detection round keeps while it keeps its hostility
    /// (`detection::Round::kin`, [`Game::keep_kin_facts`]); worked out here without them.
    fn kin_in_round(&self, a: FormId, b: FormId) -> bool {
        if a == b || a == FormId::PLAYER_REF || b == FormId::PLAYER_REF {
            return false;
        }
        let kept = self.detection_round.as_ref().filter(|r| r.hostile.is_some()).map(|r| (r.kin.get(&a), r.kin.get(&b)));
        match kept {
            Some((Some(fa), Some(fb))) => self.kin_with(a, b, fa, fb),
            _ => self.kin(a, b),
        }
    }

    /// `faction_relation(a, b)` from the factions a detection round keeps with its
    /// hostility (`detection::Round::kin`), without the kept relations' lock: a round's
    /// relations are new, so on its threads each pair's first ask took the lock to write
    /// and they waited on each other (#1799). Worked out as `faction_relation` does
    /// otherwise.
    fn relation_in_round(&self, a: FormId, b: FormId) -> i64 {
        let kept = self.detection_round.as_ref().filter(|r| r.hostile.is_some()).map(|r| (r.kin.get(&a), r.kin.get(&b)));
        match kept {
            Some((Some(fa), Some(fb))) => crate::functions::actor::factions_between(self, &fa.factions, b, &fb.factions),
            _ => faction_relation(self, a, b),
        }
    }

    /// Keep `r`'s kinship facts for the rest of a detection round's hostility (the
    /// player's too: its factions serve the relations, `Game::relation_in_round`).
    fn keep_kin_facts(&mut self, r: FormId) {
        let wanted = self.detection_round.as_ref().is_some_and(|round| round.hostile.is_some() && !round.kin.contains_key(&r));
        if wanted {
            let f = self.kin_facts(r);
            if let Some(round) = self.detection_round.as_mut() {
                round.kin.insert(r, f);
            }
        }
    }

    /// Before a round's pairs: each of `actors`' Aggression and kinship facts kept, so
    /// that [`Game::round_hostility`] can work out who would attack whom among them
    /// without `&mut`, on other threads (#1799).
    pub fn prepare_round_hostility(&mut self, actors: &[FormId]) {
        if self.detection_round.as_ref().is_none_or(|r| r.hostile.is_none()) {
            return;
        }
        for &a in actors {
            self.aggression_kept(a);
            self.keep_kin_facts(a);
        }
    }

    /// [`Game::hostile_to`]`(actor, other)` as the round would work it out, read-only
    /// (for threads): `None` outside a round's hostility, or when it lacks a fact
    /// ([`Game::prepare_round_hostility`]). Nothing a round's pairs do changes what it
    /// reads, so it is what the pair would get when it is asked.
    pub fn round_hostility(&self, actor: FormId, other: FormId) -> Option<bool> {
        let round = self.detection_round.as_ref()?;
        let kept = round.hostile.as_ref()?;
        if let Some(&h) = kept.get(&(actor, other)) {
            return Some(h);
        }
        self.hostility(actor, other, &|r| round.aggression.get(&r).copied()).map(Hostile::is)
    }

    /// Keep answers of [`Game::round_hostility`] for the rest of the round's hostility.
    pub fn keep_round_hostility(&mut self, answers: impl IntoIterator<Item = ((FormId, FormId), bool)>) {
        if let Some(kept) = self.detection_round.as_mut().and_then(|r| r.hostile.as_mut()) {
            kept.extend(answers);
        }
    }

    /// What [`Game::kin`] reads of one actor: its base (placed or created at run time,
    /// PlaceAtMe), its factions, and whether one of them names reactions.
    fn kin_facts(&self, r: FormId) -> KinFacts {
        let factions = factions_of(self, r);
        let fights = factions.iter().any(|f| self.faction_names_reactions(f));
        KinFacts { base: crate::functions::base_of(self, r), factions, fights }
    }

    fn faction_names_reactions(&self, f: &FormId) -> bool {
        self.data.faction_reactions.get(f).is_some_and(|v| !v.is_empty())
    }

    fn kin_with(&self, a: FormId, b: FormId, fa: &KinFacts, fb: &KinFacts) -> bool {
        if self.relation_in_round(a, b) == 1 || self.relation_in_round(b, a) == 1 {
            return false;
        }
        if fa.base.is_some() && fa.base == fb.base {
            return true;
        }
        let mut shared = fa.factions.iter().filter(|f| fb.factions.contains(f)).peekable();
        let any_shared = shared.peek().is_some();
        shared.any(|f| self.faction_names_reactions(f)) || (any_shared && (!fa.fights || !fb.fights))
    }

    /// `actor`'s Aggression, kept for the rest of a detection round's hostility
    /// (`detection::Round::aggression`).
    fn aggression_kept(&mut self, actor: FormId) -> i64 {
        let keep = self.detection_round.as_ref().is_some_and(|r| r.hostile.is_some());
        if let Some(&a) = self.detection_round.as_ref().filter(|_| keep).and_then(|r| r.aggression.get(&actor)) {
            return a;
        }
        let a = actor_value(self, actor, AGGRESSION).unwrap_or(0.0) as i64;
        if let Some(round) = self.detection_round.as_mut().filter(|_| keep) {
            round.aggression.insert(actor, a);
        }
        a
    }
}

impl Game<'_> {
    /// The StartCombat targets not measured since, for a save.
    pub(crate) fn scripted_combat(&self) -> BTreeSet<(FormId, FormId)> {
        self.awareness.actors.iter().flat_map(|(&a, x)| x.scripted.iter().map(move |&t| (a, t))).collect()
    }

    /// [`Game::scripted_combat`] from a save, after its actors and forced pairs. `None`: a
    /// save from before it was kept (#3390). There a forced pair's target counts as not
    /// measured while it still has what StartCombat gave it, the value above all: a
    /// measurement overwrites it, and a measured value (a sum of unclamped terms,
    /// `Settings::evaluate`) is exactly that only by coincidence. So an enemy a script set
    /// on the player that was measured and then left behind is not taken for one [design].
    pub(crate) fn restore_scripted_combat(&mut self, saved: Option<&BTreeSet<(FormId, FormId)>>) {
        let untouched = |x: &Target| x.band == Band::Detected && x.value == STARTED_VALUE && x.detected.is_some() && x.hidden_since.is_none();
        for &(actor, target) in saved.unwrap_or(&self.awareness.forced) {
            let Some(a) = self.awareness.actors.get_mut(&actor) else { continue };
            if a.targets.get(&target).is_some_and(|x| saved.is_some() || untouched(x)) {
                a.scripted.insert(target);
            }
        }
    }

    /// `actor.StartCombat target`: the actor fights `target` from now on, whatever the
    /// records say of hostility, until StopCombat [documented: catalogue notes, "Forces
    /// the calling actor to start combat with a target actor"]. It counts as detecting the
    /// target at once (Combat); one it cannot see it loses and searches for, as any
    /// other (`Awareness::settle`) [engineering choice]. The target's combat group (the
    /// catalogue's second case) is not built: there are no combat groups yet.
    pub fn start_combat(&mut self, actor: FormId, target: FormId) {
        // A fixture or combat-disabled actor takes no combat target (`Game::is_fixture`,
        // `Game::combat_disabled`, issue #2431).
        if actor == FormId::PLAYER_REF || actor == target || self.is_dead(actor) || self.is_dead(target) || self.is_fixture(actor) || self.combat_disabled(actor) {
            return;
        }
        self.awareness.forced.insert((actor, target));
        let t = self.awareness_timers();
        let now = self.time;
        let at = position(self, target).unwrap_or_default();
        let a = self.awareness.actors.entry(actor).or_default();
        a.targets.insert(target, Target { band: Band::Detected, value: STARTED_VALUE, at, detected: Some(now), hidden_since: None });
        a.scripted.insert(target);
        if let Some(c) = a.settle(actor, now, &t, true) {
            self.awareness.changes.push(c);
            if let Some(topic) = c.from.topic_to(c.to) {
                self.detection_bark(actor, Some(target), topic);
            }
        }
    }

    /// Whether `actor` fights `foe` only because a script told it to: a StartCombat on the
    /// player, who has not hit it and whom it would not attack otherwise (no grudge, its
    /// records leave the player alone). Such a fight is the actor's own, not an attack on
    /// it, so its allies do not take it up and it sends them no alarm
    /// (`crate::provocation` module notes, "Assistance") [hypothesis: Assistance answers
    /// an ally being attacked; the Silver Rush intro's customer, whose StartCombat makes
    /// him run out and whose OnHit player alone sets the Van Graffs against the player,
    /// FalloutNV.esm SCPT 0x0012424C, #2520; observer-checks.md question 180].
    pub fn unprovoked_scripted_fight(&mut self, actor: FormId, foe: FormId) -> bool {
        // A dead actor's fight is over; it calls no allies either way.
        if foe != FormId::PLAYER_REF || self.is_dead(actor) || !self.awareness.forced.contains(&(actor, foe)) || self.provocations.holds(actor, foe) {
            return false;
        }
        if self.awareness.get(actor).is_some_and(|a| a.alarms.iter().any(|x| x.cause == Cause::Hurt && x.owner == foe)) {
            return false;
        }
        self.awareness.forced.remove(&(actor, foe));
        let own = self.hostile_to_now(actor, foe);
        self.awareness.forced.insert((actor, foe));
        !own
    }

    /// `actor.StopCombat`: the actor leaves its combat state [documented: catalogue
    /// notes, "Ends the calling actor's combat state so it stops fighting"]; what
    /// StartCombat forced on it is undone. It may start again on what it perceives next.
    pub fn stop_combat(&mut self, actor: FormId) {
        self.awareness.forced.retain(|(a, _)| *a != actor);
        self.provocations.calm(actor);
        self.awareness.tactics.calm(actor);
        self.forgive_crimes(actor);
        if let Some(a) = self.awareness.actors.remove(&actor) {
            if a.state != State::Normal {
                self.awareness.changes.push(Change { actor, from: a.state, to: State::Normal, time: self.time, about: None, cause: None, at: None, value: None });
            }
        }
    }
}

impl Game<'_> {
    /// The OnStartCombat and OnCombatEnd blocks, run at the start of each
    /// [`Game::tick`] for the changes of state since the last one:
    ///
    /// - an actor that reaches Combat runs OnStartCombat with its combat target as the
    ///   block's subject (`Begin OnStartCombat player` runs only when it fights the
    ///   player) [documented: GECK wiki event handler table, OnStartCombat's Target and
    ///   Starter; observed: 86 of the 140 OnStartCombat blocks of the six masters name
    ///   the player, the rest nothing or one Dead Money actor, decompiled];
    /// - one that then goes back to Normal runs OnCombatEnd [documented: the table's
    ///   OnCombatEnd, Target and Ender]. Losing the target and searching (Lost) and
    ///   finding it again is the same fight [hypothesis]: the blocks run once a fight;
    /// - Alert alone (noticed, not fighting) runs neither [hypothesis: Alert has no
    ///   target to start combat with];
    /// - an actor that dies or is disabled while fighting runs no OnCombatEnd
    ///   [hypothesis: OnDeath tells the end].
    pub fn combat_blocks(&mut self) {
        let started: Vec<(FormId, Option<FormId>)> = self
            .awareness
            .actors
            .iter()
            .filter(|(id, a)| a.state == State::Combat && !self.fighting.contains(id))
            .map(|(id, a)| (*id, a.combat_target().map(|(t, _)| t)))
            .collect();
        let ended: Vec<FormId> = self.fighting.iter().copied().filter(|id| self.awareness.state(*id) == State::Normal).collect();
        for r in ended {
            self.fighting.remove(&r);
            if !self.is_dead(r) && self.world.is_enabled(r).unwrap_or(false) {
                self.ref_event(r, event::ON_COMBAT_END, None, None);
            }
        }
        for (r, target) in started {
            self.fighting.insert(r);
            self.ref_event(r, event::ON_START_COMBAT, target.map(|t| Value::Ref(t.0)), target);
        }
    }
}

/// `actor.StartCombat target` ([`Game::start_combat`]).
/// A co-op random encounter's `StartCombat player` is with the player nearest the
/// attacker (`crate::coop`, the encounter notes).
pub fn start_combat(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let mut target = a.form(0)?;
    if target == FormId::PLAYER_REF && ctx.self_ref.is_some_and(|s| g.coop_encounter(s)) {
        if let Some((p, _)) = g.nearest_player(r) {
            target = p;
        }
    }
    g.start_combat(r, target);
    none()
}

/// `actor.StopCombat` ([`Game::stop_combat`]).
pub fn stop_combat(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.stop_combat(r);
    none()
}

/// `actor.StopCombatAlarmOnActor`: every actor fighting or hunting the calling actor
/// stops ([`Game::stop_combat`]) [documented: GECK wiki StopCombatAlarmOnActor, "Ends the
/// fighting flag for all enemies targeting the specified actor"].
pub fn stop_combat_alarm_on_actor(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let hunters: Vec<FormId> = g.awareness.actors.iter().filter(|(_, a)| a.state.in_combat() && (a.targets.contains_key(&r) || a.alarms.iter().any(|x| x.owner == r))).map(|(id, _)| *id).collect();
    for h in hunters {
        g.stop_combat(h);
    }
    none()
}

/// `actor.IsInCombat`: 1 in a combat state (Alert, Combat, Lost) [documented: GECK wiki
/// Category:Detection, the three are combat states]; the player is in combat while an
/// actor in a combat state has it on its target or alert list [documented: GECK wiki
/// IsInCombat, "will return 1 if enemies are actively searching for the player"].
pub fn is_in_combat(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if r == FormId::PLAYER_REF {
        return yes(g.awareness.hunted(r));
    }
    yes(g.awareness.state(r).in_combat())
}

/// `actor.GetCombatTarget`: the actor it fights ([`Game::combat_target_of`]): the
/// detected target with the highest value [engineering choice: no source says how the
/// original picks among several], kept while it is out of sight; 0 outside Combat and
/// Lost.
pub fn get_combat_target(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(g.combat_target_of(r).map(|t| Value::Ref(t.0)).unwrap_or(Value::NONE))
}

impl Game<'_> {
    /// The player's Hostiles Prioritize Player tweak, after `id`'s targets settled: when
    /// the target it would choose is one of the player's companions and the player is
    /// detected at the threshold or above, one roll (the start chance when it has just
    /// gone into Combat, else the mid-fight one) sets it on the player instead while it
    /// detects the player. Out of Combat the choice is cleared [design: docs/design/tweaks.md].
    pub fn prefer_player(&mut self, id: FormId, started: bool) {
        let Some(a) = self.awareness.actors.get(&id) else { return };
        if a.state != State::Combat {
            if a.prefer_player || a.rolled_for.is_some() {
                let a = self.awareness.actors.get_mut(&id).expect("listed");
                (a.prefer_player, a.rolled_for) = (false, None);
            }
            return;
        }
        let own = a.own_target().map(|(t, _)| t);
        let player = a.targets.get(&FormId::PLAYER_REF).filter(|t| t.band == Band::Detected).map(|t| t.value);
        let Some(companion) = own.filter(|&t| t != FormId::PLAYER_REF && self.is_teammate(t)) else { return };
        if a.rolled_for == Some(companion) {
            return;
        }
        let tw = &self.tweaks;
        let (chance, threshold) = (if started { tw.prefer_player_start } else { tw.prefer_player_mid }, tw.prefer_player_detection);
        let Some(_) = player.filter(|&v| v >= f64::from(threshold)) else { return };
        let roll = self.awareness.unit();
        let a = self.awareness.actors.get_mut(&id).expect("listed");
        a.rolled_for = Some(companion);
        a.prefer_player = roll < f64::from(chance);
        tracing::info!("awareness: {id} would fight companion {companion}: {} (Hostiles Prioritize Player, roll {roll:.2} against {chance:.2})", if a.prefer_player { "fights the player" } else { "keeps it" });
    }

    /// The actor `r` fights, as scripts read it ([`get_combat_target`]); also what a
    /// condition run on "Combat Target" runs on (docs/spec-notes/packages.md §7.5) and
    /// what the extender queries that ask for it read.
    ///
    /// In Combat or Lost: ITR's ForceCombatTarget first, then the detected target
    /// [`Awareness::combat_target`] picks, and while it detects none, the one it fought last
    /// ([`Awareness::fighting`]) as long as that is alive and still on its target list or
    /// alert list. An actor of the original fighting the player kept the player as its
    /// combat target, procedure COMBAT, while GetDetected player read 0, for stretches
    /// of under 10 real s up to 293 s [observed: npc-behaviour-compare.md §11, 73
    /// stretches]. When it lets go is our own rule [design, #2121]: when the target dies
    /// or is forgotten (dead, gone, no longer hostile, `Awareness::forget`, or off both
    /// lists by their timers), when it turns to another, at StopCombat and on going back
    /// to Normal; no timer of its own (how long the original keeps it: #2010).
    pub fn combat_target_of(&self, r: FormId) -> Option<FormId> {
        let a = self.awareness.get(r).filter(|a| matches!(a.state, State::Combat | State::Lost))?;
        // ITR's ForceCombatTarget over the actor's own choice (`crate::nvse_itr2`).
        if let Some(t) = self.itr_forced_target(r) {
            return Some(t);
        }
        a.combat_target().map(|(t, _)| t).or_else(|| a.fighting.filter(|&f| !self.is_dead(f) && (a.targets.contains_key(&f) || a.alarms.iter().any(|x| x.owner == f))))
    }

    /// The actor `r` fights now, as its combat AI sees it: in Combat, ITR's
    /// ForceCombatTarget, else the detected target [`Awareness::combat_target`] picks;
    /// none once it detects none. What [`Game::combat_target_of`] read before #2121; kept
    /// for the player reports' snapshot (`nvforever` `reports/snapshot.rs`), whose
    /// `combat_target` field keeps its meaning.
    pub fn fighting_target_of(&self, r: FormId) -> Option<FormId> {
        let a = self.awareness.get(r).filter(|a| a.state == State::Combat)?;
        if let Some(t) = self.itr_forced_target(r) {
            return Some(t);
        }
        a.combat_target().map(|(t, _)| t)
    }
}

/// `location.CreateDetectionEvent owner level [type]`: a detection event at the calling
/// reference [documented: GECK wiki CreateDetectionEvent; the type 0 bullet, 1 beam, 2
/// explosion, 3 generic; generic when not given: hypothesis]. Who hears it is the
/// engine's to measure.
pub fn create_detection_event(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let at_ref = ctx.subject()?;
    let owner = a.form(0)?;
    let level = a.num_or(1, 0.0).max(0.0);
    let kind = EventType::from_code(a.num_or(2, 3.0) as i64);
    let at = if at_ref == FormId::PLAYER_REF { g.player_pos() } else { position(g, at_ref) };
    if let Some(at) = at {
        g.queue_detection_event(DetectionEvent { owner, at, level, kind });
    }
    none()
}

/// `actor.SetAlert flag`: the alerted animation state (weapon drawn), kept with the
/// reference [documented: GECK wiki SetAlert, "purely an animation state", savebaked].
pub fn set_alert(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.extra.entry(r).or_default().alerted = a.num_or(0, 0.0) != 0.0;
    none()
}

/// `actor.GetIsAlerted`: 1 in the alerted animation state [documented: catalogue notes;
/// GECK wiki SetAlert, "purely an animation state"], set with SetAlert or ITR's
/// SetAlertNS ([`Game::is_alerted`]); a combat state does not set it. In the original
/// GetIsAlerted read 0 in every sample of about 480,000 living actors, 14,312 of them
/// in combat [observed: npc-behaviour-compare.md §11; owner-decision-captures.md §3].
/// Not while it warns (`Game::warning_to`) either: a young gecko warning the player read
/// 0 [observed: capture `warnings`, young_gecko, 2026-10-24]. What an actor draws is
/// [`Game::draws_weapon`], which still counts the combat states.
pub fn get_is_alerted(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.is_alerted(r))
}

impl Game<'_> {
    /// Whether `actor` is alerted as scripts read it ([`get_is_alerted`]): SetAlert's
    /// flag or ITR's SetAlertNS, nothing else [design, #2121].
    pub fn is_alerted(&self, actor: FormId) -> bool {
        self.extra.get(&actor).is_some_and(|e| e.alerted) || self.itr_alert(actor)
    }

    /// Whether `actor` stands in its alerted stance: alerted ([`Game::is_alerted`]) or in
    /// a combat state (Alert, Combat, Lost) [hypothesis: Alert is "weapon out",
    /// Category:Detection]. What [`Game::is_alerted`] read before #2121: the weapon
    /// drawing ([`Game::draws_weapon`]) and the creatures' stance (`nvforever`
    /// `creature_stance.rs`) keep it, so drawn weapons and stances are as before; the
    /// IDLE tree's conditions read GetIsAlerted as scripts do (docs/design/alert.md §6).
    pub fn alert_stance(&self, actor: FormId) -> bool {
        self.is_alerted(actor) || self.awareness.state(actor).in_combat()
    }

    /// Whether `actor` draws its weapon (a creature its unarmed stance): in its alerted
    /// stance ([`Game::alert_stance`]), or warning the player (`Game::warning_to`)
    /// [observed: the young gecko's GetAnimAction read 0, "equip weapon", from its first
    /// warning until it fought, IsWeaponOut 0 until then (capture `warnings`,
    /// 2026-10-24); owner 2026-10-24: warning guards draw].
    pub fn draws_weapon(&self, actor: FormId) -> bool {
        self.alert_stance(actor) || self.warning_to(actor).is_some()
    }

    fn player_pos(&self) -> Option<[f32; 3]> {
        position(self, FormId::PLAYER_REF)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: FormId = FormId(0x100);
    const FOE: FormId = FormId(0x200);
    const FOE2: FormId = FormId(0x300);

    fn det(value: f64) -> Detection {
        let band = if value > 0.0 {
            Band::Detected
        } else if value > -20.0 {
            Band::Noticed
        } else {
            Band::Hidden
        };
        Detection { value, band, ..Default::default() }
    }

    fn step(a: &mut Awareness, now: f64) -> Option<(State, State)> {
        a.settle(ME, now, &Timers::default(), true).map(|c| (c.from, c.to))
    }

    #[test]
    fn a_group_says_one_detection_line_of_a_kind_at_a_time() {
        let (a, b, c, stranger) = (FormId(0xA), FormId(0xB), FormId(0xC), FormId(0xD));
        let group = |x: FormId| x != stranger;
        // A fiend said its "to combat" line here 3 s ago.
        let said = [Said { kind: bark_kind("NormalToCombat"), speaker: a, at: [0.0, 0.0, 0.0], time: 10.0 }];
        // Its mates nearby do not say one of that kind (AlertToCombat and LostToCombat
        // are the same kind), nor does it itself.
        assert_eq!(held_back_by(&said, bark_kind("AlertToCombat"), Some([500.0, 0.0, 0.0]), group), Some(a));
        assert_eq!(held_back_by(&said, bark_kind("LostToCombat"), Some([0.0, 3000.0, 0.0]), group), Some(a));
        assert_eq!(held_back_by(&said, bark_kind("NormalToCombat"), Some([0.0, 0.0, 0.0]), |x| x == a), Some(a));
        // Another kind, an actor of another group, a mate far away: they speak.
        assert_eq!(held_back_by(&said, bark_kind("CombatToLost"), Some([500.0, 0.0, 0.0]), group), None);
        assert_eq!(held_back_by(&said, bark_kind("NormalToCombat"), Some([500.0, 0.0, 0.0]), |x| x == b || x == c), None);
        assert_eq!(held_back_by(&said, bark_kind("NormalToCombat"), Some([BARK_GROUP_RADIUS + 1.0, 0.0, 0.0]), group), None);
        assert_eq!((bark_kind("AlertIdle"), bark_kind("LostIdle"), bark_kind("NormalToAlert")), ("idle", "idle", "NormalToAlert"));
        // The cooldown is the longer idle-bark wait.
        assert_eq!(Timers::default().bark_cooldown, Timers::default().idle_bark.1);
    }

    /// Hostiles Prioritize Player: with the choice set, the player while detected, else
    /// its own choice.
    #[test]
    fn a_fighter_set_on_the_player_fights_it_while_detected() {
        let t = Timers::default();
        let mut a = Awareness::default();
        a.observe(FOE, &det(50.0), [100.0, 0.0, 0.0], 0.0, &t);
        a.observe(FormId::PLAYER_REF, &det(10.0), [900.0, 0.0, 0.0], 0.0, &t);
        assert_eq!(a.combat_target().map(|x| x.0), Some(FOE));
        a.prefer_player = true;
        assert_eq!(a.combat_target().map(|x| x.0), Some(FormId::PLAYER_REF));
        assert_eq!(a.own_target().map(|x| x.0), Some(FOE));
        a.forget(FormId::PLAYER_REF);
        assert_eq!(a.combat_target().map(|x| x.0), Some(FOE));
    }

    #[test]
    fn a_noticed_hostile_alerts_and_the_alert_runs_out() {
        let t = Timers::default();
        let mut a = Awareness::default();
        a.observe(FOE, &det(-10.0), [100.0, 0.0, 0.0], 0.0, &t);
        assert_eq!(step(&mut a, 0.0), Some((State::Normal, State::Alert)));
        assert_eq!(a.search_alarm().map(|x| x.at), Some([100.0, 0.0, 0.0]));
        // Still noticed: the alarm moves with it and stays.
        a.observe(FOE, &det(-12.0), [150.0, 0.0, 0.0], 15.0, &t);
        assert_eq!(step(&mut a, 15.0), None);
        assert_eq!(a.search_alarm().map(|x| x.at), Some([150.0, 0.0, 0.0]));
        // Not noticed again: the alarm runs out, back to Normal.
        a.observe(FOE, &det(-40.0), [150.0, 0.0, 0.0], 20.0, &t);
        assert_eq!(step(&mut a, 34.9), None);
        assert_eq!(step(&mut a, 35.0), Some((State::Alert, State::Normal)));
        assert!(a.alarms.is_empty() && a.targets.is_empty());
    }

    #[test]
    fn a_hidden_hostile_does_nothing() {
        let t = Timers::default();
        let mut a = Awareness::default();
        a.observe(FOE, &det(-30.0), [0.0; 3], 0.0, &t);
        assert_eq!(step(&mut a, 0.0), None);
        assert_eq!(a.state, State::Normal);
    }

    #[test]
    fn a_fighter_keeps_its_target_within_the_margin() {
        let mut a = Awareness::default();
        let t = |value: f64| Target { band: Band::Detected, value, at: [0.0; 3], detected: Some(0.0), hidden_since: None };
        a.targets.insert(FormId(1), t(60.0));
        a.targets.insert(FormId(2), t(40.0));
        assert_eq!(a.combat_target().map(|x| x.0), Some(FormId(1)));
        a.fighting = Some(FormId(1));
        // The other's value goes above, within the margin: it keeps its target.
        a.targets.insert(FormId(2), t(100.0));
        assert_eq!(a.combat_target().map(|x| x.0), Some(FormId(1)));
        // Beyond the margin, or its target no longer detected: it turns to the other.
        a.targets.insert(FormId(2), t(60.0 + TARGET_KEEP_MARGIN + 1.0));
        assert_eq!(a.combat_target().map(|x| x.0), Some(FormId(2)));
        a.targets.insert(FormId(2), t(70.0));
        a.targets.insert(FormId(1), Target { band: Band::Noticed, ..t(60.0) });
        assert_eq!(a.combat_target().map(|x| x.0), Some(FormId(2)));
    }

    #[test]
    fn a_fighter_commits_to_a_new_target() {
        let t = Timers::default();
        let mut a = Awareness::default();
        a.observe(FormId(1), &det(60.0), [0.0; 3], 0.0, &t);
        a.observe(FormId(2), &det(40.0), [0.0; 3], 0.0, &t);
        step(&mut a, 0.0);
        assert_eq!(a.fighting, Some(FormId(1)));
        // Far above its target within the commitment: it keeps its target, unless the
        // other is much nearer.
        a.at = Some([0.0; 3]);
        a.observe(FormId(1), &det(60.0), [1000.0, 0.0, 0.0], 1.0, &t);
        a.observe(FormId(2), &det(60.0 + TARGET_KEEP_MARGIN + 10.0), [600.0, 0.0, 0.0], 1.0, &t);
        step(&mut a, 1.0);
        assert_eq!(a.combat_target().map(|x| x.0), Some(FormId(1)));
        a.observe(FormId(2), &det(60.0 + TARGET_KEEP_MARGIN + 10.0), [400.0, 0.0, 0.0], 1.0, &t);
        assert_eq!(a.combat_target().map(|x| x.0), Some(FormId(2)), "a much nearer one breaks it");
        a.observe(FormId(2), &det(60.0 + TARGET_KEEP_MARGIN + 10.0), [600.0, 0.0, 0.0], 1.0, &t);
        // The commitment over: the margin decides.
        step(&mut a, TARGET_COMMIT);
        assert_eq!(a.combat_target().map(|x| x.0), Some(FormId(2)));
        // Its new target out of sight: it turns back at once, committed again.
        let s = TARGET_COMMIT + 1.0;
        step(&mut a, s);
        a.observe(FormId(2), &det(-5.0), [0.0; 3], s, &t);
        step(&mut a, s);
        assert_eq!((a.fighting, a.committed), (Some(FormId(1)), true));
        // Its target dead (forgotten): it turns to the other at once.
        a.observe(FormId(2), &det(10.0), [0.0; 3], s + 1.0, &t);
        a.forget(FormId(1));
        step(&mut a, s + 1.0);
        assert_eq!(a.combat_target().map(|x| x.0), Some(FormId(2)));
    }

    #[test]
    fn detected_goes_to_combat_from_normal_and_from_alert() {
        let t = Timers::default();
        let mut a = Awareness::default();
        a.observe(FOE, &det(5.0), [0.0; 3], 0.0, &t);
        assert_eq!(step(&mut a, 0.0), Some((State::Normal, State::Combat)));
        let mut b = Awareness::default();
        b.observe(FOE, &det(-5.0), [0.0; 3], 0.0, &t);
        step(&mut b, 0.0);
        b.observe(FOE, &det(3.0), [0.0; 3], 1.0, &t);
        assert_eq!(step(&mut b, 1.0), Some((State::Alert, State::Combat)));
        assert_eq!(b.combat_target().map(|x| x.0), Some(FOE));
    }

    #[test]
    fn combat_loses_its_target_searches_and_gives_up() {
        let t = Timers::default();
        let mut a = Awareness::default();
        a.observe(FOE, &det(10.0), [10.0, 0.0, 0.0], 0.0, &t);
        step(&mut a, 0.0);
        // Undetected (noticed) for less than the lost timer: still Combat.
        a.observe(FOE, &det(-5.0), [20.0, 0.0, 0.0], 1.0, &t);
        assert_eq!(step(&mut a, t.lost - 0.5), None);
        // Long enough: Lost, searching where it was last seen.
        assert_eq!(step(&mut a, t.lost), Some((State::Combat, State::Lost)));
        assert!(a.alarms.iter().any(|x| x.cause == Cause::LostTarget && x.owner == FOE));
        // Seen again: Combat.
        let s = t.lost + 1.0;
        a.observe(FOE, &det(1.0), [30.0, 0.0, 0.0], s, &t);
        assert_eq!(step(&mut a, s), Some((State::Lost, State::Combat)));
        // [design, #2010] Hidden: Lost after the lost timer, retained through a search;
        // without AI progress the safety deadline eventually releases it.
        a.observe(FOE, &det(-50.0), [30.0, 0.0, 0.0], s + 1.0, &t);
        assert_eq!(step(&mut a, s + t.lost - 0.5), None);
        assert_eq!(step(&mut a, s + t.lost), Some((State::Combat, State::Lost)));
        assert!(a.targets.contains_key(&FOE), "lost before it is forgotten");
        assert_eq!(step(&mut a, s + t.lost + 30.0), None, "ordinary timers cannot interrupt the search");
        assert!(a.targets.contains_key(&FOE));
        assert_eq!(step(&mut a, s + t.lost + LOST_SEARCH_STALL_SECONDS), Some((State::Lost, State::Normal)));
    }

    #[test]
    fn a_target_a_mate_detects_is_neither_lost_nor_forgotten() {
        // #972 (docs/spec-notes/combat-groups.md §3): hidden from it for longer than both
        // timers, but a member of its group saw it half a second ago.
        let t = Timers::default();
        let mut a = Awareness::default();
        a.observe(FOE, &det(10.0), [0.0; 3], 0.0, &t);
        step(&mut a, 0.0);
        a.observe(FOE, &det(-50.0), [0.0; 3], 0.5, &t);
        let end = t.lost.max(t.forget_target) + 5.0;
        let mut s = 1.0;
        while s <= end {
            a.shared.insert(FOE, s - 0.5);
            assert_eq!(step(&mut a, s), None, "at {s}");
            s += 1.0;
        }
        assert!(a.targets.contains_key(&FOE));
        // The mates lose it too: Lost the lost timer after their last sighting.
        let last = s - 1.5;
        assert_eq!(step(&mut a, last + t.lost - 0.5), None);
        assert_eq!(step(&mut a, last + t.lost), Some((State::Combat, State::Lost)));
    }

    #[test]
    fn a_fighter_called_by_an_ally_takes_the_alarms_owner() {
        // #972 (combat-groups.md §2): alarmed about FOE2 (the first of the foes to start),
        // it detects FOE far better, yet takes FOE2 while it detects it.
        let t = Timers::default();
        let mut a = Awareness::default();
        a.alarm(FOE2, [0.0; 3], 0.0, Cause::Ally, 0.0, &t);
        step(&mut a, 0.0);
        a.observe(FOE, &det(10.0 + TARGET_KEEP_MARGIN + 30.0), [0.0; 3], 1.0, &t);
        a.observe(FOE2, &det(10.0), [0.0; 3], 1.0, &t);
        assert_eq!(step(&mut a, 1.0), Some((State::Alert, State::Combat)));
        assert_eq!(a.combat_target().map(|x| x.0), Some(FOE2));
        // Without the call it takes the one it detects best.
        let mut b = Awareness::default();
        b.observe(FOE, &det(10.0 + TARGET_KEEP_MARGIN + 30.0), [0.0; 3], 1.0, &t);
        b.observe(FOE2, &det(10.0), [0.0; 3], 1.0, &t);
        step(&mut b, 1.0);
        assert_eq!(b.combat_target().map(|x| x.0), Some(FOE));
    }

    #[test]
    fn combat_ends_when_no_target_is_left() {
        let t = Timers::default();
        let mut a = Awareness::default();
        a.observe(FOE, &det(10.0), [0.0; 3], 0.0, &t);
        step(&mut a, 0.0);
        a.forget(FOE);
        assert_eq!(step(&mut a, 0.5), Some((State::Combat, State::Normal)));
    }

    #[test]
    fn an_event_heard_while_alert_puts_its_owner_on_the_target_list() {
        let t = Timers::default();
        let mut a = Awareness::default();
        a.alarm(FOE, [0.0; 3], -10.0, Cause::Event(EventType::Bullet), 0.0, &t);
        assert!(a.targets.is_empty(), "not yet alert");
        assert_eq!(step(&mut a, 0.0), Some((State::Normal, State::Alert)));
        a.alarm(FOE2, [5.0, 0.0, 0.0], -10.0, Cause::Event(EventType::Generic), 1.0, &t);
        assert!(a.targets.contains_key(&FOE2) && a.targets[&FOE2].detected.is_none());
        assert_eq!(a.search_alarm().map(|x| x.owner), Some(FOE2), "the latest alarm is searched");
    }

    #[test]
    fn no_combat_alert_keeps_the_actor_normal_but_not_out_of_combat() {
        let t = Timers::default();
        let mut a = Awareness::default();
        a.observe(FOE, &det(-10.0), [0.0; 3], 0.0, &t);
        assert_eq!(a.settle(ME, 0.0, &t, false), None);
        a.observe(FOE, &det(10.0), [0.0; 3], 1.0, &t);
        assert_eq!(a.settle(ME, 1.0, &t, false).map(|c| c.to), Some(State::Combat));
    }

    #[test]
    fn the_topics_are_named_from_and_to() {
        assert_eq!(State::Normal.topic_to(State::Alert), Some("NormalToAlert"));
        assert_eq!(State::Combat.topic_to(State::Lost), Some("CombatToLost"));
        assert_eq!(State::Alert.topic_to(State::Lost), None);
        assert_eq!(State::Lost.idle_topic(), Some("LostIdle"));
        assert!(State::Lost.in_combat() && !State::Normal.in_combat() && !State::Combat.searching());
    }
}
