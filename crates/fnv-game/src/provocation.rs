//! Being attacked: assault, friendly fire, yielding and assistance
//! (docs/design/combat-ai.md §10; owner's playtest list 2026-10-11).
//!
//! Who attacks whom on sight is [`Game::would_attack`] (aggression against faction
//! relations). This module adds the fights that start because someone was **hit**, and
//! how they end:
//!
//! - **Assault** [documented: GECK wiki Friendly Fire, "Neutral: aggro immediately on
//!   player when attacked, in or out of combat"]: an actor the player hits that would not
//!   attack the player holds a **grudge** against the player from then on, so it fights
//!   (`would_attack` is true for the pair) and says its Assault line [documented: GECK
//!   wiki Combat Tab, "Said when the actor is first attacked"]. Enemies that are not
//!   aggressive enough to attack on sight are treated the same [design].
//! - **Owner's exception** (playtest list 2026-10-11): a neutral actor already fighting
//!   someone else, hit by the player while the player is fighting too, does not turn on
//!   the player (a stray shot in a shared fight) [design].
//! - **Friendly fire** [documented: GECK wiki Friendly Fire]: a Friend (faction reaction 3)
//!   turns on the player at once out of combat (iFriendHitNonCombatAllowed hits allowed)
//!   and only after more than iFriendHitCombatAllowed hits in combat; an Ally (reaction 2,
//!   and the player's teammates [hypothesis]) after more than iAllyHitNonCombatAllowed
//!   hits out of combat and never in combat (iAllyHitCombatAllowed, 1000 or more meaning
//!   never). A hit counts only fFriendMinimumLastHitTime after the one before, and runs
//!   out after fFriendHitTimer. GetFriendHit returns the count; an ally in combat never
//!   counts above 1. SetIgnoreFriendlyHits makes a friend or ally ignore them all
//!   [hypothesis: what the function's name says].
//! - **Yielding** [documented: GECK wiki Yielding]: the player yields by putting the weapon
//!   away within fCombatYieldTime (10 s) of the fight's start. Only actors fighting
//!   because the player attacked them (a grudge, not their records or StartCombat)
//!   accept: they stop fighting, say AcceptYield, and forget the grudge. The owner's rule
//!   (playtest list 2026-10-11): an actor that forgave the player within
//!   fCombatYieldRetryTime (600 s) and was attacked again ignores the yield. The wiki's
//!   timer is the player's (no one accepts a yield again until it runs out); ours is each
//!   actor's [design: the owner's wording, "if they have already recently forgiven"].
//! - **Assistance** [documented: GECK wiki GetFactionCombatReaction, "An actor's
//!   Assistance setting determine whether it will help an Ally / a Friend in combat or
//!   not"; the codes 0 Helps Nobody, 1 Helps Allies, 2 Helps Friends and Allies:
//!   xEdit AIDT]: an actor that detects an ally (or a friend, with code 2) in a combat
//!   state takes up that ally's fight against each actor the ally fights, unless it is
//!   itself an ally or friend of that actor. It holds a grudge as an assaulted actor does
//!   and is alarmed at the attacker's position. Actors sharing a faction, or placed from
//!   the same base form, count as allies [engineering choice, as `would_attack` never
//!   lets them fight each other]. Only a fight the ally did not start alone is taken
//!   up: not one a script gave it (StartCombat) against a player who has not hit it and
//!   whom it would leave alone otherwise (`Game::unprovoked_scripted_fight`; no alarm
//!   either, `crate::combat_group`) [hypothesis: Assistance answers an ally being
//!   attacked; the Silver Rush intro's customer is made to run out by his StartCombat and
//!   only his OnHit player sets the Van Graffs against the player, #2520; observer-checks
//!   question 180]. The player is helped too (#2794) [hypothesis: whether Assistance
//!   counts the player as an ally to help is not documented; the maintainer expects it]:
//!   whoever is in a combat state with the player on its target list is fought by a
//!   watcher that detects the player and whose Assistance covers its reaction to the
//!   player, with the checks above, unless it is after the player itself (a crime, a
//!   grudge, a StartCombat, the player on its target list); the player's teammates keep their own rule
//!   (`crate::followers`). The scripted-fight rule does not keep them out: it concerns the
//!   allies of the actor a script set on the player, and the player is the one attacked
//!   (the Legate's camp Securitrons, PlayerFaction allies, against General Oliver, set on
//!   the player by `StartCombat player`; observer-checks question 217).
//! - **Cowards stay out** [documented: GECK wiki Confidence, "Cowardly … I will always
//!   flee from combat"; owner's playtest list 4, item 14: confidence and aggression must
//!   override the helps-allies join]: an actor whose Confidence is Cowardly never takes up
//!   another's fight, by assistance here or by an alarm (`crate::combat_group`). In a
//!   fight of its own it flees from the start (`Game::always_flees`, #2520). Avoiding a
//!   fight before it starts (running from a threat out of combat) is not built
//!   (combat-ai.md §11, "Avoiding threats out of combat") [design: Unaggressive alone does not keep a helper out; the wiki's
//!   Unaggressive only says whom an actor might avoid].
//! - **A danger to itself** [design: owner's playtest list 5 item 35, "Sunny and her dog
//!   should fight as soon as the geckos aggro on the player"]: an actor that detects an
//!   attacker fighting the player or itself, where that attacker would attack it too
//!   (Very Aggressive geckos attack everyone), takes up the fight against it at once, as
//!   if helping the attacked one, instead of waiting to be hit. Not cowards, not actors
//!   whose package is Defensive Combat, not against an ally or friend of its own. Fights
//!   between other actors are left to assistance and alarms (above, and
//!   `crate::combat_group`): with this rule there too, every NCR trooper near Cook-Cook's
//!   camp took the fight up on its own and fewer alarmed the others.
//! - **Escorts** [documented: GECK wiki Category:Packages, Defensive Combat, "The
//!   exception is for the Accompany, Escort and Follow packages. For these three, if the
//!   target is attacked, the actor will join into the combat"]: an actor whose current
//!   package is Accompany, Escort or Follow ([`CHARGE_PACKAGES`]; "escort" below for all
//!   three) takes up the fight of its charge (the package's target) against everyone
//!   attacking it: actors in a combat state with the charge on their target list, and
//!   whoever hurt it. Its Assistance does not matter; it never fights its own allies or
//!   friends, and a coward stays out. It starts no fight of its own: with Defensive
//!   Combat it still attacks nobody on sight (`Game::hostility`). Choices [hypothesis]:
//!   the escort must at least notice its charge (band Noticed or Detected); a fight the
//!   charge starts is joined only once the foe fights back; a charge that provoked the
//!   attacker is still helped; it holds for the whole package, also once at the
//!   destination; the player's teammates are left to `crate::followers` (#2793: the
//!   Hoover Dam escort Securitron, on a Defensive Combat Follow package, never fought the
//!   Legion attacking the player).
//!
//! - **An Evil faction's area** [design: owner decision 2026-11-13 on #1538, "attacking
//!   the fiends should alert all fiends in the area that you are to be attacked";
//!   docs/design/combat-ai.md §10.3.1]: when the player attacks (hits or kills) a member of
//!   a faction flagged Evil, every member of that faction in the area turns on the
//!   player at once, whether or not it saw the attack: it holds a grudge against the
//!   player and is alarmed at the player's position. The area is the loaded cells: the
//!   victim's own cell when that is an interior, else the exterior within
//!   [`crate::combat_group::ALARM_RADIUS`] of the victim. A faction that tracks crime
//!   also makes the player its enemy from then on (`Game::crime_enemy`), so its members
//!   elsewhere are hostile too. Before this an ally's first hits were tolerated,
//!   assistance never fought the helper's own ally, and an Evil victim's kill was no
//!   murder for witnesses (`crate::crime`), so the Fiends of Vault 3, allied by its
//!   door script's SetAlly, never turned [observed: FalloutNV.esm SCPT 0x000F2DF5;
//!   FiendsFactionNV flagged Hidden, Evil, Track Crime]. Not when the victim is already
//!   fighting someone else (a stray hit in a shared fight, as the owner's exception
//!   above), and not the player's teammates.
//!
//! A grudge ends when its holder goes back to Normal, on StopCombat, and on an accepted
//! yield. Hits by actors other than the player go to `Game::attacked_by_actor`
//! (`crate::combat_group`, NPC against NPC).

use crate::awareness::{Cause, State};
use crate::detection::Band;
use crate::functions::actor::{actor_value, faction_relation, factions_of};
use crate::game::Game;
use crate::package::PackageType;
use fnv_sim::FormId;
use std::collections::{BTreeMap, HashMap};

const ASSISTANCE: u16 = crate::actor_values::ASSISTANCE;

/// Whom an actor fights, and where each was last known to be.
type Foes = Vec<(FormId, [f32; 3])>;

/// The package types whose actor takes up its target's fights (module notes, "Escorts")
/// [documented: GECK wiki Category:Packages, Defensive Combat].
pub const CHARGE_PACKAGES: [PackageType; 3] = [PackageType::Accompany, PackageType::Escort, PackageType::Follow];

/// Faction combat reactions [documented: GECK wiki GetFactionCombatReaction].
pub const NEUTRAL: i64 = 0;
pub const ENEMY: i64 = 1;
pub const ALLY: i64 = 2;
pub const FRIEND: i64 = 3;

/// Why an actor fights someone its records would leave alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Why {
    /// It was attacked by them.
    Assault,
    /// It helps `victim`, whom they fight.
    Assist { victim: FormId },
    /// The player stayed inside its aggro radius past its warnings (`crate::aggro`).
    Aggro,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Grudge {
    pub why: Why,
    /// `Game::time` the fight began for it: the yield timer counts from here.
    pub since: f64,
}

/// The game settings of friendly fire and yielding [documented: GECK wiki Friendly Fire
/// and Yielding, with the wiki's defaults; FalloutNV.esm sets iFriendHitCombatAllowed to
/// 4, observed 2026-10-12].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub friend_hit_timer: f64,
    pub friend_min_last_hit: f64,
    pub friend_combat: f64,
    pub friend_non_combat: f64,
    pub ally_combat: f64,
    pub ally_non_combat: f64,
    pub yield_time: f64,
    pub yield_retry: f64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            friend_hit_timer: 10.0,
            friend_min_last_hit: 0.5,
            friend_combat: 3.0,
            friend_non_combat: 0.0,
            ally_combat: 1000.0,
            ally_non_combat: 3.0,
            yield_time: 10.0,
            yield_retry: 600.0,
        }
    }
}

impl Settings {
    pub fn read(g: &Game) -> Settings {
        let d = Settings::default();
        let s = |name: &str, v: f64| crate::combat::setting(g, name, v);
        Settings {
            friend_hit_timer: s("fFriendHitTimer", d.friend_hit_timer),
            friend_min_last_hit: s("fFriendMinimumLastHitTime", d.friend_min_last_hit),
            friend_combat: s("iFriendHitCombatAllowed", d.friend_combat),
            friend_non_combat: s("iFriendHitNonCombatAllowed", d.friend_non_combat),
            ally_combat: s("iAllyHitCombatAllowed", d.ally_combat),
            ally_non_combat: s("iAllyHitNonCombatAllowed", d.ally_non_combat),
            yield_time: s("fCombatYieldTime", d.yield_time),
            yield_retry: s("fCombatYieldRetryTime", d.yield_retry),
        }
    }
}

/// "Any value 1000 or over is treated as infinite" [documented: GECK wiki Friendly Fire].
pub const INFINITE_HITS: f64 = 1000.0;

/// What happened, for the engine's log and summary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Note {
    /// `actor` turns on `by`, who hit it (its reaction to `by`; the friendly hits counted).
    Assaulted { actor: FormId, by: FormId, relation: i64, hits: u32 },
    /// A friend's or ally's hit counted, not yet enough (`allowed` hits are let pass).
    Tolerated { actor: FormId, by: FormId, relation: i64, hits: u32, allowed: f64, in_combat: bool },
    /// Ignored: both were fighting someone else (the owner's exception), or the actor
    /// ignores friendly hits.
    Ignored { actor: FormId, by: FormId, why: &'static str },
    /// `helper` takes up `victim`'s fight against `attacker`.
    Assists { helper: FormId, victim: FormId, attacker: FormId },
    /// `actor` accepted the player's yield and stopped fighting.
    Forgives { actor: FormId, after: f64 },
    /// `actor` ignored the yield.
    Refuses { actor: FormId, why: &'static str, after: f64 },
    /// `actor` warned the player inside its aggro radius (`level` warnings so far;
    /// `crate::aggro`).
    AggroWarned { actor: FormId, level: u32, distance: f64, radius: f64 },
    /// `actor` attacks the player, who stayed inside its aggro radius.
    AggroAttacks { actor: FormId, distance: f64, radius: f64 },
    /// `helper` stays out of `victim`'s fight: its confidence is Cowardly (module notes).
    StaysOut { helper: FormId, victim: FormId },
    /// The player attacked `victim`, of the Evil faction `faction`: `alerted` of its
    /// members in the area turn on the player (module notes, "An Evil faction's area"),
    /// each noted before as [`Note::Alerted`].
    AreaAlert { victim: FormId, faction: FormId, alerted: u32 },
    /// `actor` turns on the player, who attacked `victim` of its Evil faction.
    Alerted { actor: FormId, victim: FormId },
    /// `victim`'s allies stay out of its fight against `foe`: a script gave it the fight
    /// and `foe` (the player) did not provoke it (module notes, "Assistance"); noted once
    /// per pair.
    Unprovoked { victim: FormId, foe: FormId },
}

/// The grudges, friendly hits and forgiveness of every actor. The grudges and the
/// forgiveness times are saved (`GameSnapshot::grudges`, `forgiven`); the rest is not.
#[derive(Debug, Clone, Default)]
pub struct Provocations {
    /// (actor, whom it fights) → why.
    pub grudges: BTreeMap<(FormId, FormId), Grudge>,
    /// (actor, by) → the times of the friendly hits counted, oldest first.
    pub friend_hits: BTreeMap<(FormId, FormId), Vec<f64>>,
    /// When each actor last accepted the player's yield.
    pub forgiven: BTreeMap<FormId, f64>,
    /// Actors someone attacked (GetAttacked), until they leave combat.
    pub attacked: std::collections::BTreeSet<FormId>,
    pub notes: Vec<Note>,
    pub settings: Option<Settings>,
    /// Aggro radius warnings each actor gave the player inside its radius, and when it
    /// gave the last (`crate::aggro`).
    pub aggro_warnings: std::collections::BTreeMap<FormId, (u32, f64)>,
    /// Actors warning the player now: inside their aggro radius with the player watched
    /// at the last aggro tick (`crate::aggro`, `Game::warning_to`).
    pub aggro_inside: std::collections::BTreeSet<FormId>,
    /// (helper, victim) pairs whose staying out was noted ([`Note::StaysOut`], once).
    pub stayed_out: std::collections::BTreeSet<(FormId, FormId)>,
    /// (victim, foe) pairs whose unprovoked scripted fight was noted ([`Note::Unprovoked`],
    /// once).
    pub unprovoked: std::collections::BTreeSet<(FormId, FormId)>,
}

impl Provocations {
    /// Whether `actor` fights `other` because of a grudge.
    pub fn holds(&self, actor: FormId, other: FormId) -> bool {
        self.grudges.contains_key(&(actor, other))
    }

    /// Forget every grudge `actor` holds (it left its fight).
    pub fn calm(&mut self, actor: FormId) {
        self.grudges.retain(|(a, _), _| *a != actor);
        self.friend_hits.retain(|(a, _), _| *a != actor);
        self.attacked.remove(&actor);
    }

    pub fn take_notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }
}

/// How many friendly hits are let pass before a friend or ally turns (module notes):
/// `None` for an actor that is neither.
pub fn hits_allowed(s: &Settings, relation: i64, in_combat: bool) -> Option<f64> {
    match (relation, in_combat) {
        (FRIEND, true) => Some(s.friend_combat),
        (FRIEND, false) => Some(s.friend_non_combat),
        (ALLY, true) => Some(s.ally_combat),
        (ALLY, false) => Some(s.ally_non_combat),
        _ => None,
    }
}

/// Whether an actor with Assistance `assistance` helps one it stands to as `relation`
/// [documented: codes 0 Helps Nobody, 1 Helps Allies, 2 Helps Friends and Allies].
pub fn helps(assistance: i64, relation: i64) -> bool {
    match relation {
        ALLY => assistance >= 1,
        FRIEND => assistance >= 2,
        _ => false,
    }
}

impl Game<'_> {
    fn provocation_settings(&mut self) -> Settings {
        match self.provocations.settings {
            Some(s) => s,
            None => {
                let s = Settings::read(self);
                self.provocations.settings = Some(s);
                s
            }
        }
    }

    /// How `actor` stands to `other` for friendly fire and assistance: its faction combat
    /// reaction, the player's teammates being allies of the player [hypothesis], and
    /// actors sharing a faction or a base form allies [engineering choice].
    pub fn reaction_to(&self, actor: FormId, other: FormId) -> i64 {
        // Another co-op player's actor stands where the player does (`Game::as_player`).
        let (actor, other) = (self.as_player(actor), self.as_player(other));
        if other == FormId::PLAYER_REF && self.extra.get(&actor).is_some_and(|e| e.teammate) {
            return ALLY;
        }
        if actor == FormId::PLAYER_REF && self.extra.get(&other).is_some_and(|e| e.teammate) {
            return ALLY;
        }
        let relation = faction_relation(self, actor, other);
        if relation != NEUTRAL || actor == FormId::PLAYER_REF || other == FormId::PLAYER_REF {
            return relation;
        }
        if self.kin(actor, other) {
            return ALLY;
        }
        NEUTRAL
    }

    /// Whether `helper` stays out of `victim`'s fight (a Cowardly helper, module notes);
    /// the first time for the pair it is noted ([`Note::StaysOut`]).
    pub fn stays_out(&mut self, helper: FormId, victim: FormId) -> bool {
        let cowardly = actor_value(self, helper, crate::actor_values::CONFIDENCE).unwrap_or(2.0) as i64 == 0;
        if cowardly && self.provocations.stayed_out.insert((helper, victim)) {
            self.provocations.notes.push(Note::StaysOut { helper, victim });
        }
        cowardly
    }

    /// Whether `victim`'s allies leave its fight against `foe` alone (module notes,
    /// "Assistance": a scripted fight against a player who did not provoke it); the first
    /// time for the pair it is noted ([`Note::Unprovoked`]).
    pub fn allies_stay_out(&mut self, victim: FormId, foe: FormId) -> bool {
        let out = self.unprovoked_scripted_fight(victim, foe);
        if out && self.provocations.unprovoked.insert((victim, foe)) {
            self.provocations.notes.push(Note::Unprovoked { victim, foe });
        }
        out
    }

    /// `actor` was hit by `by` (and lives): it may turn on `by` (module notes). Returns
    /// whether it now holds a grudge it did not hold before. The engine calls this for
    /// every hit before [`Game::hurt_by`], which then alarms the actor.
    pub fn attacked_by(&mut self, actor: FormId, by: FormId) -> bool {
        if actor == by || actor == FormId::PLAYER_REF || self.is_dead(actor) {
            return false;
        }
        self.provocations.attacked.insert(actor);
        // Hits by other actors: `crate::combat_group` (NPC against NPC).
        if by != FormId::PLAYER_REF {
            return self.attacked_by_actor(actor, by);
        }
        if self.provocations.holds(actor, by) || self.would_attack(actor, by) {
            return false;
        }
        let s = self.provocation_settings();
        let now = self.time;
        let state = self.awareness.state(actor);
        let in_combat = state.in_combat();
        let relation = self.reaction_to(actor, by);
        let area = self.evil_area_alert(actor);
        let hits = match relation {
            _ if area => 1,
            ALLY | FRIEND => {
                if self.extra.get(&actor).is_some_and(|e| e.ignore_friendly_hits) {
                    self.provocations.notes.push(Note::Ignored { actor, by, why: "it ignores friendly hits" });
                    return false;
                }
                let allowed = hits_allowed(&s, relation, in_combat).unwrap_or(0.0);
                let list = self.provocations.friend_hits.entry((actor, by)).or_default();
                list.retain(|t| now - t < s.friend_hit_timer);
                if list.last().is_none_or(|t| now - t >= s.friend_min_last_hit) {
                    list.push(now);
                }
                if allowed >= INFINITE_HITS {
                    // "The count never goes above 1".
                    list.drain(..list.len().saturating_sub(1));
                }
                let hits = list.len() as u32;
                if allowed >= INFINITE_HITS || hits as f64 <= allowed {
                    self.provocations.notes.push(Note::Tolerated { actor, by, relation, hits, allowed, in_combat });
                    return false;
                }
                hits
            }
            _ => {
                // The owner's exception: both already fighting someone else.
                let fights_other = state == State::Combat && self.awareness.get(actor).and_then(|a| a.combat_target()).is_some_and(|(t, _)| t != by);
                let player_fights = self.awareness.actors.iter().any(|(id, a)| *id != actor && a.state.in_combat() && a.targets.contains_key(&by));
                if fights_other && player_fights {
                    self.provocations.notes.push(Note::Ignored { actor, by, why: "both were fighting someone else" });
                    return false;
                }
                1
            }
        };
        self.provocations.grudges.insert((actor, by), Grudge { why: Why::Assault, since: now });
        self.provocations.notes.push(Note::Assaulted { actor, by, relation, hits });
        self.player_assaulted(actor);
        self.detection_bark(actor, Some(by), "Assault");
        true
    }

    /// The player attacked `victim` (a hit, or the kill, `Game::player_kills`): when it is
    /// in a faction flagged Evil, every member of that faction in the area turns on the
    /// player (module notes, "An Evil faction's area"). Returns whether the rule applied
    /// (the victim is such a member and was not fighting someone else).
    pub fn evil_area_alert(&mut self, victim: FormId) -> bool {
        use crate::crime::faction_flags::{EVIL, TRACK_CRIME};
        let flags = |g: &Game, f: FormId| g.faction_flags(f);
        let evil: Vec<FormId> = factions_of(self, victim).into_iter().filter(|&f| flags(self, f) & EVIL != 0).collect();
        if evil.is_empty() || self.is_teammate(victim) {
            return false;
        }
        // A stray hit in a fight the victim has with someone else.
        if self.awareness.get(victim).is_some_and(|a| a.state == State::Combat && a.combat_target().is_some_and(|(t, _)| t != FormId::PLAYER_REF)) {
            return false;
        }
        let cell = self.cell_of(victim);
        let indoors = cell.is_some_and(|c| !crate::functions::fo3::exterior(self, c));
        let hearers = self.alarm_hearers(&BTreeMap::new());
        let victim_at = hearers.get(&victim).copied().or_else(|| self.world.peek(victim).map(|s| s.pos)).unwrap_or_default();
        let player_at = self.world.peek(FormId::PLAYER_REF).map_or(victim_at, |s| s.pos);
        let now = self.time;
        let t = self.awareness.timers.unwrap_or_default();
        for faction in evil {
            let mut alerted = 0;
            for (&m, &at) in &hearers {
                if m == FormId::PLAYER_REF || m == victim || self.is_dead(m) || !self.present(m) || self.is_teammate(m) || self.combat_disabled(m) {
                    continue;
                }
                let near = if indoors {
                    self.cell_of(m) == cell
                } else {
                    self.cell_of(m).is_some_and(|c| crate::functions::fo3::exterior(self, c)) && (at[0] - victim_at[0]).hypot(at[1] - victim_at[1]) <= crate::combat_group::ALARM_RADIUS
                };
                if !near || !factions_of(self, m).contains(&faction) || self.provocations.holds(m, FormId::PLAYER_REF) {
                    continue;
                }
                self.provocations.grudges.insert((m, FormId::PLAYER_REF), Grudge { why: Why::Assist { victim }, since: now });
                self.awareness.actors.entry(m).or_default().alarm(FormId::PLAYER_REF, player_at, 0.0, Cause::Ally, now, &t);
                self.provocations.notes.push(Note::Alerted { actor: m, victim });
                alerted += 1;
            }
            if flags(self, faction) & TRACK_CRIME != 0 {
                self.make_player_enemy_of(faction);
            }
            self.provocations.notes.push(Note::AreaAlert { victim, faction, alerted });
        }
        true
    }

    /// GetFriendHit: the friendly hits `by` made on `actor` that still count.
    pub fn friend_hits(&self, actor: FormId, by: FormId) -> u32 {
        let window = self.provocations.settings.unwrap_or_default().friend_hit_timer;
        self.provocations.friend_hits.get(&(actor, by)).map_or(0, |l| l.iter().filter(|t| self.time - **t < window).count() as u32)
    }

    /// The player put the weapon away: a yield (module notes). Returns how many actors
    /// accepted it.
    pub fn player_yields(&mut self) -> usize {
        let s = self.provocation_settings();
        let now = self.time;
        let fighting: Vec<(FormId, Grudge)> = self.provocations.grudges.iter().filter(|((_, o), _)| *o == FormId::PLAYER_REF).map(|((a, _), g)| (*a, *g)).collect();
        let mut accepted = 0;
        for (actor, g) in fighting {
            if !self.awareness.state(actor).in_combat() && g.why == Why::Assault {
                // Not fighting yet (still to notice the attacker): nothing to yield to.
                continue;
            }
            let after = now - g.since;
            if after > s.yield_time {
                self.provocations.notes.push(Note::Refuses { actor, why: "the fight is past the time to yield", after });
                continue;
            }
            if self.provocations.forgiven.get(&actor).is_some_and(|t| now - t < s.yield_retry) {
                self.provocations.notes.push(Note::Refuses { actor, why: "it forgave the player recently", after });
                continue;
            }
            self.provocations.forgiven.insert(actor, now);
            self.provocations.notes.push(Note::Forgives { actor, after });
            self.stop_combat(actor);
            self.detection_bark(actor, Some(FormId::PLAYER_REF), "AcceptYield");
            accepted += 1;
        }
        accepted
    }

    /// Allies of actors in a fight take it up (module notes): called after a round of
    /// measurements, before [`Game::settle_awareness`]. Returns how many joined.
    pub fn call_allies(&mut self) -> usize {
        let mut fights: Vec<(FormId, Foes)> = self
            .awareness
            .actors
            .iter()
            .filter(|(_, a)| a.state.in_combat())
            .map(|(id, a)| {
                let mut foes: Vec<(FormId, [f32; 3])> = a.targets.iter().map(|(t, x)| (*t, x.at)).collect();
                for al in a.alarms.iter().filter(|x| x.cause == Cause::Hurt) {
                    if !foes.iter().any(|f| f.0 == al.owner) {
                        foes.push((al.owner, al.at));
                    }
                }
                (*id, foes)
            })
            .filter(|(_, f)| !f.is_empty())
            .collect();
        // The player as the one attacked (module notes, "Assistance", #2794): it has no
        // entry above, so its foes are gathered as `Game::teammates_assist` gathers them,
        // at their positions.
        let pos = |g: &Game, r: FormId| g.world.peek(r).map(|s| s.pos).or_else(|| g.world.placed.get(r).map(|p| p.pos));
        let player_foes: Foes = self
            .awareness
            .actors
            .iter()
            .filter(|(id, a)| a.state.in_combat() && a.targets.contains_key(&FormId::PLAYER_REF) && !self.is_teammate(**id))
            .filter_map(|(id, _)| pos(self, *id).map(|p| (*id, p)))
            .collect();
        if !player_foes.is_empty() {
            fights.push((FormId::PLAYER_REF, player_foes));
        }
        let mut joined = 0;
        // Who perceives whom (detected, seen or heard), in one pass over the pairs, each
        // victim's watchers in the pairs' order [engineering choice: one scan per victim
        // over every pair was a fifth of a 50-a-side battle's detection, CPU profile
        // 2026-10-23]. Each helper's Assistance is read once.
        let mut watching: HashMap<FormId, Vec<FormId>> = HashMap::new();
        for ((w, t), (d, _)) in &self.detection.pairs {
            if d.band == Band::Detected {
                watching.entry(*t).or_default().push(*w);
            }
        }
        let mut assistance_of: HashMap<FormId, i64> = HashMap::new();
        // Who would attack whom, worked out once per pair while no grudge is added: a
        // grudge is the only thing this loop changes that it depends on (`Game::would_attack`),
        // so the memo is emptied when one is. Asked again for every victim, it was 10 ms of
        // a 50-a-side battle's measurement: ~12,000 asks for ~2,500 pairs [observed
        // 2026-11-07, #1799].
        let mut attacks: HashMap<(FormId, FormId), bool> = HashMap::new();
        for (victim, mut foes) in fights {
            foes.retain(|&(foe, _)| !self.allies_stay_out(victim, foe));
            if foes.is_empty() {
                continue;
            }
            let watchers = watching.get(&victim).cloned().unwrap_or_default();
            for helper in watchers {
                if helper == FormId::PLAYER_REF || self.is_dead(helper) || !self.present(helper) || self.combat_disabled(helper) {
                    continue;
                }
                // The player's teammates take up its fights by their own rule
                // (`Game::teammates_assist`).
                if victim == FormId::PLAYER_REF && self.is_teammate(helper) {
                    continue;
                }
                // Nobody helps the player while it is after the player itself: a crime of
                // the player's against it, a grudge, a StartCombat on the player
                // (`Game::would_attack`), or the player on its own target list; its faction
                // reaction may still say Ally or Friend [design, #2794].
                if victim == FormId::PLAYER_REF && (self.would_attack(helper, FormId::PLAYER_REF) || self.awareness.get(helper).is_some_and(|a| a.targets.contains_key(&FormId::PLAYER_REF))) {
                    continue;
                }
                let assistance = match assistance_of.get(&helper) {
                    Some(&a) => a,
                    None => {
                        let a = actor_value(self, helper, ASSISTANCE).unwrap_or(0.0) as i64;
                        assistance_of.insert(helper, a);
                        a
                    }
                };
                if !helps(assistance, self.reaction_to(helper, victim)) {
                    continue;
                }
                if self.stays_out(helper, victim) {
                    continue;
                }
                for &(attacker, at) in &foes {
                    if attacker == helper || self.provocations.holds(helper, attacker) {
                        continue;
                    }
                    // Narrowed to the #3459 HELIOS One cross-faction creature bridge:
                    // a creature fighting only under an assist grudge for another creature
                    // does not pull a third-party watcher that is not a comrade of the
                    // original creature into the fight [design, #3459].
                    if victim != FormId::PLAYER_REF && self.cross_faction_creature_assist_relay(victim, attacker, helper) {
                        continue;
                    }
                    let would = match attacks.get(&(helper, attacker)) {
                        Some(&w) => w,
                        None => {
                            let w = self.would_attack(helper, attacker);
                            attacks.insert((helper, attacker), w);
                            w
                        }
                    };
                    if would {
                        continue;
                    }
                    // Never against its own allies and friends [documented: they "cannot
                    // attack each other under any circumstances"].
                    if matches!(self.reaction_to(helper, attacker), ALLY | FRIEND) {
                        continue;
                    }
                    let now = self.time;
                    self.provocations.grudges.insert((helper, attacker), Grudge { why: Why::Assist { victim }, since: now });
                    attacks.clear();
                    self.provocations.notes.push(Note::Assists { helper, victim, attacker });
                    let t = self.awareness.timers.unwrap_or_default();
                    self.awareness.actors.entry(helper).or_default().alarm(attacker, at, 0.0, Cause::Ally, now, &t);
                    joined += 1;
                }
            }
        }
        joined += self.join_against_dangers();
        joined += self.escorts_assist();
        // The player's teammates take up the player's fights (`crate::followers`).
        joined + self.teammates_assist()
    }

    /// An actor running an Accompany, Escort or Follow package ([`CHARGE_PACKAGES`]) takes
    /// up its charge's fights (module notes, "Escorts"; docs/spec-notes/packages.md §3.1):
    /// for each actor attacking the charge
    /// (in a combat state with the charge on its target list, or having hurt it), the
    /// escort holds a grudge and is alarmed at the attacker's position, as an assisting
    /// ally is. Returns how many joined.
    pub fn escorts_assist(&mut self) -> usize {
        let escorts: Vec<(FormId, FormId)> = self
            .extra
            .iter()
            .filter_map(|(id, e)| {
                let def = self.package_def(&e.current_package?)?;
                CHARGE_PACKAGES.contains(&def.ty).then_some((*id, def))
            })
            .filter_map(|(id, def)| self.package_target(id, def).map(|t| (id, t)))
            .filter(|(id, charge)| id != charge)
            .collect();
        let mut joined = 0;
        for (escort, charge) in escorts {
            if escort == FormId::PLAYER_REF || self.is_dead(escort) || !self.present(escort) || self.is_dead(charge) || self.is_teammate(escort) || self.combat_disabled(escort) {
                continue;
            }
            // It must at least notice its charge [hypothesis: an escort keeps track of the
            // one it leads even while walking ahead with its back to it, so Noticed is
            // enough where an ally's assistance needs Detected].
            if !self.detection.pairs.get(&(escort, charge)).is_some_and(|(d, _)| d.band != Band::Hidden) {
                continue;
            }
            for (attacker, at) in self.attackers_of(charge) {
                if attacker == escort || self.is_dead(attacker) {
                    continue;
                }
                let knows = self.awareness.get(escort).is_some_and(|a| a.targets.contains_key(&attacker) || a.alarms.iter().any(|x| x.owner == attacker));
                if knows || self.provocations.holds(escort, attacker) {
                    continue;
                }
                // Never against its own allies and friends [documented: they "cannot
                // attack each other under any circumstances"].
                if matches!(self.reaction_to(escort, attacker), ALLY | FRIEND) {
                    continue;
                }
                if self.stays_out(escort, charge) {
                    break;
                }
                let now = self.time;
                if !self.would_attack(escort, attacker) {
                    self.provocations.grudges.insert((escort, attacker), Grudge { why: Why::Assist { victim: charge }, since: now });
                }
                self.provocations.notes.push(Note::Assists { helper: escort, victim: charge, attacker });
                let t = self.awareness.timers.unwrap_or_default();
                self.awareness.actors.entry(escort).or_default().alarm(attacker, at, 0.0, Cause::Ally, now, &t);
                joined += 1;
            }
        }
        joined
    }

    /// Who attacks `victim`, with where they are: every actor in a combat state with
    /// `victim` on its target list, and whoever hurt `victim` (its `Hurt` alarms).
    fn attackers_of(&self, victim: FormId) -> Foes {
        let pos = |g: &Game, r: FormId| g.world.peek(r).map(|s| s.pos).or_else(|| g.world.placed.get(r).map(|p| p.pos));
        let mut out: Foes = self
            .awareness
            .actors
            .iter()
            .filter(|(id, a)| **id != victim && a.state.in_combat() && a.targets.contains_key(&victim))
            .filter_map(|(id, _)| pos(self, *id).map(|p| (*id, p)))
            .collect();
        if let Some(a) = self.awareness.get(victim) {
            for al in a.alarms.iter().filter(|x| x.cause == Cause::Hurt) {
                if al.owner != victim && !out.iter().any(|o| o.0 == al.owner) {
                    out.push((al.owner, al.at));
                }
            }
        }
        out
    }
}

impl Game<'_> {
    /// Whether `receiver` should stay out of a second-hop assist relay from `sender`
    /// fighting `foe` [design, #3459]: narrowed to the HELIOS One cross-faction creature
    /// bridge (`CrDogNCRAggressive` -> `CrDogNCRFollow` -> `HeliosExtLvlNCRTrooperGun`).
    ///
    /// Returns true only when `sender` fights `foe` (or the player, when `foe` is the
    /// player's teammate) solely under `Why::Assist { victim: orig }` (`orig != PLAYER_REF`,
    /// not `Assault` or `StartCombat`), both `sender` and `orig` are creatures (`CREA`),
    /// and `receiver` is not a comrade of `orig`. Normal NPC assist relays, same-pack
    /// creature relays, one-hop direct assists, and direct assaults on `sender` are
    /// unaffected.
    pub(crate) fn cross_faction_creature_assist_relay(&self, sender: FormId, foe: FormId, receiver: FormId) -> bool {
        let key = if self.is_teammate(foe) && !self.provocations.holds(sender, foe) { FormId::PLAYER_REF } else { foe };
        if self.awareness.forced.contains(&(sender, key)) {
            return false;
        }
        let Some(Why::Assist { victim: orig }) = self.provocations.grudges.get(&(sender, key)).map(|g| g.why) else {
            return false;
        };
        if orig == FormId::PLAYER_REF {
            return false;
        }
        if crate::nvse_nx::is_creature(self, sender) != Some(true) || crate::nvse_nx::is_creature(self, orig) != Some(true) {
            return false;
        }
        !self.comrades(receiver, orig)
    }

    /// Actors that detect an attacker which would attack them too join the fight against
    /// it (module notes, "A danger to itself"). Returns how many joined.
    fn join_against_dangers(&mut self) -> usize {
        // Who fights whom: each attacker in a combat state and its targets.
        let fights: Vec<(FormId, FormId)> = self
            .awareness
            .actors
            .iter()
            .filter(|(_, a)| a.state == State::Combat)
            .flat_map(|(id, a)| a.targets.keys().map(move |t| (*id, *t)))
            .collect();
        let mut joined = 0;
        // Each attacker's watchers in the pairs' order, from one pass over the pairs: a
        // pass per fight was 7.5-10.6 ms of a 50-a-side battle's measurement [observed
        // 2026-11-07, #1799]. Nothing below changes the pairs.
        let mut watching: HashMap<FormId, Vec<FormId>> = HashMap::new();
        for ((w, t), (d, _)) in &self.detection.pairs {
            if d.band == Band::Detected {
                watching.entry(*t).or_default().push(*w);
            }
        }
        for (attacker, victim) in fights {
            let Some(at) = self.world.peek(attacker).map(|s| s.pos).or_else(|| self.world.placed.get(attacker).map(|p| p.pos)) else { continue };
            let watchers = watching.get(&attacker).cloned().unwrap_or_default();
            for helper in watchers {
                // Another actor's fight counts only when it is the watcher's own.
                if victim != FormId::PLAYER_REF && helper != victim {
                    continue;
                }
                if helper == FormId::PLAYER_REF || self.is_dead(helper) || !self.present(helper) || self.awareness.state(helper) == State::Combat || self.combat_disabled(helper) {
                    continue;
                }
                if self.provocations.holds(helper, attacker) || self.would_attack(helper, attacker) || !self.would_attack(attacker, helper) {
                    continue;
                }
                // A Very Aggressive attacker whose aggro radius leaves `helper` (a Neutral
                // bystander) out of reach is not attacking `helper` [design, #3459].
                if crate::awareness::position(self, helper).is_some_and(|helper_at| self.neutral_out_of_reach(attacker, helper, helper_at)) {
                    continue;
                }
                if matches!(self.reaction_to(helper, attacker), ALLY | FRIEND) || self.stays_out(helper, attacker) || self.package_has(helper, crate::package::flags::DEFENSIVE_COMBAT) {
                    continue;
                }
                let now = self.time;
                // Gone for itself: as if attacked; else as helping the one attacked.
                let why = if victim == helper { Why::Assault } else { Why::Assist { victim } };
                self.provocations.grudges.insert((helper, attacker), Grudge { why, since: now });
                self.provocations.notes.push(Note::Assists { helper, victim, attacker });
                // It noticed the danger itself, so it alarms its own allies in turn
                // (`crate::combat_group`, which passes on no alarm from an ally).
                let t = self.awareness.timers.unwrap_or_default();
                self.awareness.actors.entry(helper).or_default().alarm(attacker, at, 0.0, Cause::Noticed, now, &t);
                joined += 1;
            }
        }
        joined
    }
}

/// `actor.GetFriendHit target`: the friendly hits `target` made on the actor that still
/// count [documented: GECK wiki GetFriendHit, Friendly Fire].
///
/// In a parameterless CTDA condition (function index 288, empty CTDA parameter list in
/// `FalloutNV.esm`, e.g. dialogue INFOs with `GetFriendHit > 0`), the implicit attacker
/// is the condition's target (`Game::condition_target`, or `0` when absent) [hypothesis:
/// `eval_conditions` supplies the conversation/interaction target in `condition_target`,
/// matching `Exists` in `functions::fighting`; native behavior when `target` is absent
/// or differs from the player is unobserved]. Ordinary script calls still require an
/// explicit actor argument (`a.form(0)?`).
pub fn get_friend_hit(g: &mut Game, ctx: &crate::functions::Ctx, a: &crate::functions::Args) -> Result<fnv_script::vm::Value, String> {
    let r = ctx.subject()?;
    let by = if ctx.in_condition && a.0.is_empty() {
        let Some(target) = g.condition_target else {
            return Ok(fnv_script::vm::Value::ZERO);
        };
        target
    } else {
        a.form(0)?
    };
    Ok(fnv_script::vm::Value::Num(g.friend_hits(r, by) as f64))
}

/// `actor.GetAttacked`: 1 once someone attacked the actor, cleared when it drops out of
/// combat [documented: GECK wiki GetAttacked].
pub fn get_attacked(g: &mut Game, ctx: &crate::functions::Ctx, _a: &crate::functions::Args) -> Result<fnv_script::vm::Value, String> {
    let r = ctx.subject()?;
    crate::functions::yes(g.provocations.attacked.contains(&r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_hits_allowed_follow_the_settings() {
        let s = Settings { friend_combat: 4.0, ..Settings::default() };
        assert_eq!(hits_allowed(&s, FRIEND, true), Some(4.0));
        assert_eq!(hits_allowed(&s, FRIEND, false), Some(0.0));
        assert_eq!(hits_allowed(&s, ALLY, true), Some(1000.0));
        assert_eq!(hits_allowed(&s, ALLY, false), Some(3.0));
        assert_eq!(hits_allowed(&s, NEUTRAL, false), None);
    }

    #[test]
    fn assistance_codes() {
        assert!(!helps(0, ALLY) && !helps(0, FRIEND));
        assert!(helps(1, ALLY) && !helps(1, FRIEND));
        assert!(helps(2, ALLY) && helps(2, FRIEND));
        assert!(!helps(2, NEUTRAL) && !helps(2, ENEMY));
    }
}
