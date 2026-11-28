//! Perk entry points: a perk the player has changes a number the game is about to use
//! (docs/spec-notes/perk-entry-points.md; aid items' in docs/spec-notes/aid-items.md §10,
//! experience's in docs/spec-notes/experience.md §2, combat's through
//! `fnv_combat::damage::PerkHooks`).
//!
//! An entry point effect of a PERK names the entry point, a function (Set, Add,
//! Multiply …) and its parameters; the function is applied to the value the game has
//! calculated [documented: GECK wiki Entry Point, "the Add Value function … modify the
//! current value"]. Which code is which entry point [documented: GECK wiki
//! GetNthPerkEntryFunction, "Entry Function Codes" 0–73; observed: the one perk of the
//! masters with code 23 is Chem Resistant, "50% less likely to develop an addiction",
//! Multiply 0.5; code 25 is Chemist, "chems … last twice as long", Multiply 2; code 44
//! is Lead Belly, "50% less radiation when consuming", Multiply 0.5].
//!
//! Each entry point has up to two argument tabs besides the perk owner's: the weapon and
//! the target for weapon damage, the attacker and its weapon for the defender's DT and so
//! on [documented: GECK wiki GetPerkModifier, each entry function's additional arguments;
//! observed: Cowboy's weapon damage entry tests IsInList on tab 1 and Animal Control's
//! GetIsCreatureType on tab 2; Hit the Deck's DT entry tests IsWeaponSkillType Explosives
//! on tab 2]. The conditions of each tab run on that tab's form with the owner as their
//! target, the owner's on the owner with the other actor as their target
//! ([`Game::entry_ops`]) [hypothesis: the targets].
//!
//! Rules that are ours [hypothesis]: of a perk with several ranks only the entries of the
//! highest rank owned apply (the masters' ranked entries each hold the whole bonus,
//! Demolition Expert 1.2, 1.4, 1.6); entries apply in ascending priority, then perk
//! FormID; an entry with conditions on a tab the caller has no form for is not applied;
//! every tab's conditions must pass.

use crate::data::{PerkEffect, PerkEntry};
use crate::game::Game;
use fnv_sim::FormId;
use std::collections::BTreeMap;

/// "Adjust Limb Damage" [documented: GECK wiki Entry Point, the name; observed: the
/// two perks of FalloutNV.esm with code 6 are the trait Small Frame, "your limbs are
/// more easily crippled", Multiply 1.25, and the challenge perk Tough Guy, "breaking
/// bones has led them to become stronger", Multiply 0.8; hypothesis: it runs for the
/// actor whose limb is hit] (`Game::damage_limb`, docs/spec-notes/crippled-limbs.md).
pub const ADJUST_LIMB_DAMAGE: u8 = 6;
/// "Adjust Experience Points" [documented: GECK wiki Entry Point, "Run when the player
/// gains any experience", the example Swift Learner; observed: Swift Learner's and
/// Skilled's entries have code 9] (`crate::experience::award`).
pub const ADJUST_EXPERIENCE_POINTS: u8 = 9;
/// "Adjust Gained Skill Points" [documented: GECK wiki Entry Point, "Run when the player
/// gains a level; Modifies the skill rate of the player", the example Educated; observed:
/// the two entries of the masters with code 10 are Educated's Add 2, "two more skill
/// points every time you advance in level", and Logan's Loophole's Set 0 under GetLevel >
/// 30, "after 30 you can kiss … skill points goodbye"] (`crate::levelup::points_for`).
pub const ADJUST_GAINED_SKILL_POINTS: u8 = 10;
/// "Adjust Book Skill Points" [documented: GECK wiki Entry Point, "Run when the actor
/// reads a book; Modifies the skill points gained", the example Comprehension; observed:
/// Comprehension's one entry has code 11, Add 1]. Read when a skill book is
/// (`crate::books`, docs/spec-notes/skill-books.md §3).
pub const ADJUST_BOOK_SKILL_POINTS: u8 = 11;
/// "Modify Addiction Chance" [observed: Chem Resistant; documented: GECK wiki Entry
/// Point, "Run when the actor takes a drug"].
pub const MODIFY_ADDICTION_CHANCE: u8 = 23;
/// "Modify Positive Chem Duration" (Vegas) [observed: Chemist; documented: GECK wiki
/// Entry Point, "Modifies the duration of the ingestible"].
pub const MODIFY_POSITIVE_CHEM_DURATION: u8 = 25;
/// "Modify Radiation Consumed" [observed: Lead Belly; documented: GECK wiki Entry
/// Point, "Modifies the radiation from ingestibles"].
pub const MODIFY_RADIATION_CONSUMED: u8 = 44;
/// Multiplies the health an ingestible restores (Better Healing × 1.2, Cardiac Arrest
/// × 1.25) [observed: the original game, captures `pk_better_healing*` 2026-10-25: a
/// Stimpak's and a Brahmin Wellington's healing × 1.2 at every Medicine and Survival
/// tried; the GECK wiki's "natural healing rate" has nothing to act on, the player does
/// not heal by itself] (perk-entry-points.md §14).
pub const MODIFY_RECOVERED_HEALTH: u8 = 12;
/// "Add Leveled List On Death": "Adds a leveled list to the target's inventory when the
/// player kills someone" [documented: GECK wiki Perk, the list of entry points; code 21,
/// GetNthPerkEntryFunction; tab 1 Target, GetPerkModifier] (`crate::death_items`).
pub const ADD_LEVELED_LIST_ON_DEATH: u8 = 21;

/// Entry point functions [observed: see `EntryPointDef::function`; hypothesis for the
/// names]. The masters use only 1–3 on numbers, 8 (EPFT 3, a leveled list) and 9 (EPFT
/// 4, a script: the activate choices) [observed: every PERK of FalloutNV.esm and the
/// five add-ons].
pub const SET_VALUE: u8 = 1;
pub const ADD_VALUE: u8 = 2;
pub const MULTIPLY_VALUE: u8 = 3;
/// The other number functions the GECK offers, numbered by its Function list's order
/// [hypothesis: that order gives 8 Add Leveled List and 9 Add Activate Choice, as the
/// masters' entries show; no entry of the masters or the add-ons uses 4–7 or 12–14, so
/// their codes and parameter layouts are not checked against data].
pub const ADD_RANGE_TO_VALUE: u8 = 4;
pub const ADD_ACTOR_VALUE_MULT: u8 = 5;
pub const ABSOLUTE_VALUE: u8 = 6;
pub const NEGATIVE_ABSOLUTE_VALUE: u8 = 7;
pub const SET_TO_ACTOR_VALUE_MULT: u8 = 12;
pub const MULTIPLY_ACTOR_VALUE_MULT: u8 = 13;
pub const MULTIPLY_1_PLUS_ACTOR_VALUE_MULT: u8 = 14;

/// The combat entry points (`fnv_combat::damage::PerkHooks` and the engine's weapon
/// figures) [documented: GECK wiki GetNthPerkEntryFunction, the codes; Entry Point, what
/// each modifies]. Which the engine asks for, with which tabs:
/// docs/spec-notes/perk-entry-points.md §2.
pub const CALCULATE_WEAPON_DAMAGE: u8 = 0;
pub const CALCULATE_MY_CRITICAL_HIT_CHANCE: u8 = 1;
pub const CALCULATE_MY_CRITICAL_HIT_DAMAGE: u8 = 2;
pub const CALCULATE_TO_HIT_CHANCE: u8 = 8;
pub const CALCULATE_GUN_SPREAD: u8 = 34;
pub const MODIFY_ENEMY_CRITICAL_HIT_CHANCE: u8 = 36;
pub const RELOAD_SPEED: u8 = 37;
pub const EQUIP_SPEED: u8 = 38;
pub const ACTION_POINT_COST: u8 = 40;
pub const MODIFY_ATTACK_SPEED: u8 = 43;
pub const KNOCKDOWN_CHANCE: u8 = 52;
pub const MODIFY_WEAPON_STRENGTH_REQ: u8 = 53;
pub const MODIFY_AIMING_MOVE_SPEED: u8 = 54;
pub const MODIFY_DAMAGE_THRESHOLD_DEFENDER: u8 = 56;
pub const MODIFY_CHANCE_FOR_AMMO_ITEM: u8 = 57;
pub const MODIFY_DAMAGE_THRESHOLD_ATTACKER: u8 = 58;
pub const MODIFY_THROWING_VELOCITY: u8 = 59;
pub const MODIFY_ITEM_DAMAGE: u8 = 68;

/// The entry points of locks, terminals, sneaking, weight and travel
/// (docs/spec-notes/perk-entry-points.md §7) [documented: GECK wiki
/// GetNthPerkEntryFunction, the codes; Entry Point, what each does; observed: the perks'
/// entries]: Ignore Locked Terminal (Computer Whiz, Set 1), Ignore Running During
/// Detection (Silent Running, Set 1), Ignore Broken Lock (Infiltrator, Set 1), Get Max
/// Carry Weight (Hoarder, Add 25), Has Fast Travel Always (Long Haul, Set 1), Modify Light
/// Items (Pack Rat, Set 1), Adjust Heavy Weapon Weight (Heavyweight, Multiply 0.5).
pub const IGNORE_LOCKED_TERMINAL: u8 = 20;
pub const GET_MAX_CARRY_WEIGHT: u8 = 22;
pub const IGNORE_RUNNING_DURING_DETECTION: u8 = 31;
pub const IGNORE_BROKEN_LOCK: u8 = 32;
pub const HAS_FAST_TRAVEL_ALWAYS: u8 = 51;
pub const MODIFY_LIGHT_ITEMS: u8 = 55;
pub const ADJUST_HEAVY_WEAPON_WEIGHT: u8 = 73;

/// The entry points of mines, explosions, throws and movement (docs/spec-notes/
/// perk-entry-points.md §8) [documented: GECK wiki GetNthPerkEntryFunction, the codes;
/// Entry Point, what each does; observed: the perks' entries]: Calculate Mine Explode
/// Chance (Light Step Set 0; In My Footsteps Set 0; ED-E's Light Step Set 0 under GetIsID
/// conditions), Modify Run Speed (Travel Light × 1.1, Tunnel Runner × 1.25, Implant M-5 ×
/// 1.2, Atomic! × 1.25, Stealth Suit Mk II × 1.2, each with its own conditions), Adjust
/// Explosion Radius (Splash Damage × 1.25).
pub const CALCULATE_MINE_EXPLODE_CHANCE: u8 = 4;
pub const MODIFY_RUN_SPEED: u8 = 42;
pub const ADJUST_EXPLOSION_RADIUS: u8 = 72;

/// The entry points of kills, the HUD, repair and drinking (docs/spec-notes/
/// perk-entry-points.md §9) [documented: GECK wiki GetNthPerkEntryFunction, the codes;
/// Entry Point, what each does; observed: the perks' one entry each, no conditions]:
/// Adjust Drinking Radiation (Implant Y-3 × 0.01), Has Meltdown (Meltdown Set 1), See Enemy
/// Health (Living Anatomy Set 1), Has Jury Rigging (Jury Rigging Set 1).
pub const ADJUST_DRINKING_RADIATION: u8 = 26;
pub const HAS_MELTDOWN: u8 = 46;
pub const SEE_ENEMY_HEALTH: u8 = 47;
pub const HAS_JURY_RIGGING: u8 = 48;

/// The unarmed special moves (docs/spec-notes/perk-entry-points.md §11) [documented: GECK
/// wiki GetNthPerkEntryFunction, codes 61–66; observed: Legion Assault 61, Ranger Takedown
/// 62, Scribe Counter 64, Khan Trick 65 and 66, each Set 1 with no conditions].
pub const HAS_UNARMED_FORWARD_POWER_ATTACK: u8 = 61;
/// The companion perks' entry points (Enhanced Sensors, Spotter, Search and Mark)
/// [documented: GECK wiki Perk entry points] (perk-entry-points.md §13).
pub const HAS_IMPROVED_DETECTION: u8 = 69;
pub const HAS_IMPROVED_SPOTTING: u8 = 70;
pub const HAS_IMPROVED_ITEM_DETECTION: u8 = 71;
pub const HAS_UNARMED_BACK_POWER_ATTACK: u8 = 62;
pub const HAS_UNARMED_CROUCHED_POWER_ATTACK: u8 = 63;
pub const HAS_UNARMED_COUNTER_ATTACK: u8 = 64;
pub const HAS_UNARMED_LEFT_POWER_ATTACK: u8 = 65;
pub const HAS_UNARMED_RIGHT_POWER_ATTACK: u8 = 66;

/// The weight Adjust Heavy Weapon Weight starts at: "a Weapon which weighs 10.0 lbs or more
/// (after the DecreaseWeight Weapon Mod effect)" [documented: GECK wiki Entry Point].
pub const HEAVY_WEAPON_WEIGHT: f32 = 10.0;

/// What one entry does to the value, its parameters resolved for this use (an actor
/// value read, a range rolled).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Set(f64),
    Add(f64),
    Mul(f64),
    Abs,
    NegAbs,
}

impl Op {
    pub fn apply(self, v: f64) -> f64 {
        match self {
            Op::Set(x) => x,
            Op::Add(x) => v + x,
            Op::Mul(x) => v * x,
            Op::Abs => v.abs(),
            Op::NegAbs => -v.abs(),
        }
    }
}

/// The EPFD of Add Range To Value (low, high) and of the actor value functions (the
/// actor value's code as an integer, the factor) [hypothesis: two 4-byte fields].
fn two_fields(raw: &[u8]) -> Option<([u8; 4], [u8; 4])> {
    Some((<[u8; 4]>::try_from(raw.get(0..4)?).ok()?, <[u8; 4]>::try_from(raw.get(4..8)?).ok()?))
}

/// One entry's function as an [`Op`]: `av` reads the owner's actor value, `roll` gives a
/// number uniform in [0, 1). `None`: not a number function, or its parameters are
/// missing. Add Range To Value adds a number rolled between its two; the actor value
/// functions take the actor value × the factor and add it, set the value to it,
/// multiply by it, or multiply by 1 + it [hypothesis: as their names say].
pub fn resolve(ep: &crate::data::EntryPointDef, av: &mut dyn FnMut(u16) -> f64, roll: &mut dyn FnMut() -> f64) -> Option<Op> {
    let value = ep.value.map(f64::from);
    Some(match ep.function {
        SET_VALUE => Op::Set(value?),
        ADD_VALUE => Op::Add(value?),
        MULTIPLY_VALUE => Op::Mul(value?),
        ADD_RANGE_TO_VALUE => {
            let (lo, hi) = two_fields(&ep.raw)?;
            let (lo, hi) = (f32::from_le_bytes(lo) as f64, f32::from_le_bytes(hi) as f64);
            Op::Add(lo + roll() * (hi - lo))
        }
        ABSOLUTE_VALUE => Op::Abs,
        NEGATIVE_ABSOLUTE_VALUE => Op::NegAbs,
        ADD_ACTOR_VALUE_MULT | SET_TO_ACTOR_VALUE_MULT | MULTIPLY_ACTOR_VALUE_MULT | MULTIPLY_1_PLUS_ACTOR_VALUE_MULT => {
            let (code, k) = two_fields(&ep.raw)?;
            let x = av(u32::from_le_bytes(code) as u16) * f32::from_le_bytes(k) as f64;
            match ep.function {
                ADD_ACTOR_VALUE_MULT => Op::Add(x),
                SET_TO_ACTOR_VALUE_MULT => Op::Set(x),
                MULTIPLY_ACTOR_VALUE_MULT => Op::Mul(x),
                _ => Op::Mul(1.0 + x),
            }
        }
        _ => return None,
    })
}

/// One entry that applied: the perk, and the value after it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Applied {
    pub perk: FormId,
    pub value: f64,
}

/// The function of one entry on a value.
pub fn apply_function(function: u8, number: f64, value: f64) -> Option<f64> {
    match function {
        SET_VALUE => Some(number),
        ADD_VALUE => Some(value + number),
        MULTIPLY_VALUE => Some(value * number),
        _ => None,
    }
}

/// The player gains one rank of `perk` (AddPerk, the level-up menu's Done): the rank is
/// counted, then each Quest + Stage effect of that rank sets its quest to its stage, which
/// runs the stage's result script [hypothesis: when the rank is gained; observed: the
/// stages' scripts say "triggered by Intense Training perk rank 1" for the effect of rank
/// 0, docs/spec-notes/levelup.md §8]. The rank the player has now.
pub fn gain_rank(g: &mut Game, perk: FormId) -> u8 {
    let rank = {
        let r = g.player.perks.entry(perk).or_default();
        *r = r.saturating_add(1);
        *r
    };
    let data = g.data;
    let stages: Vec<(FormId, u8)> = data
        .perks
        .get(&perk)
        .into_iter()
        .flat_map(|p| &p.entries)
        .filter(|e| u16::from(e.rank) + 1 == u16::from(rank))
        .filter_map(|e| match e.effect {
            PerkEffect::QuestStage { quest, stage } => Some((quest, stage)),
            _ => None,
        })
        .collect();
    for (quest, stage) in stages {
        g.set_stage(quest, i32::from(stage));
    }
    rank
}

/// The abilities a perk gives at `rank`: the Ability effects of the highest rank owned
/// that has any [documented: GECK wiki GetNthPerkEntryType, entry type "Ability";
/// observed: Action Boy's rank 1 names PerkActionBoy1, Action Points +15, and its rank 2
/// PerkActionBoy2, +30, the whole bonus, so the ranks do not add up; hypothesis: as the
/// entry points' ranks above].
pub fn rank_abilities(def: &crate::data::PerkDef, rank: u8) -> Vec<FormId> {
    let at = |e: &&PerkEntry| matches!(e.effect, PerkEffect::Ability(s) if s != FormId(0)) && e.rank < rank;
    let Some(top) = def.entries.iter().filter(at).map(|e| e.rank).max() else { return Vec::new() };
    def.entries
        .iter()
        .filter(at)
        .filter(|e| e.rank == top)
        .filter_map(|e| match e.effect {
            PerkEffect::Ability(s) => Some(s),
            _ => None,
        })
        .collect()
}

/// Whether `def` gives at least one Ability at `rank` matching `pred`, without
/// allocating a `Vec` (mirrors [`rank_abilities`]).
fn has_rank_ability(def: &crate::data::PerkDef, rank: u8, mut pred: impl FnMut(FormId) -> bool) -> bool {
    let at = |e: &&PerkEntry| matches!(e.effect, PerkEffect::Ability(s) if s != FormId(0)) && e.rank < rank;
    let Some(top) = def.entries.iter().filter(at).map(|e| e.rank).max() else { return false };
    def.entries
        .iter()
        .filter(at)
        .filter(|e| e.rank == top)
        .any(|e| matches!(e.effect, PerkEffect::Ability(s) if pred(s)))
}

impl Game<'_> {
    /// The perks `actor` holds, with their ranks: the player's own (traits included); a
    /// teammate of the player holds the teammate perks (`AddPerk perk 1` on the player),
    /// each at rank 1 [documented: GECK wiki AddPerk, forTeammates "adds the perk to the
    /// player's teammates"; hypothesis: one rank, and held only while a teammate — the
    /// masters' teammate perks (Companion Suite, the companions' quest rewards Old
    /// Vaquero, Hand of Vengeance, Calm Heart, Bonds of Steel, Causeless Rebel) have one
    /// rank, and their entry points test GetIsID of the companion they are for]. Another
    /// actor also holds the perks given to it itself (`<actor>.AddPerk perk`,
    /// `RefExtra::perks`), a perk it holds both ways counted once at the higher rank.
    pub fn held_perks(&self, actor: FormId) -> Vec<(FormId, u8)> {
        if actor == FormId::PLAYER_REF {
            return self.player.perks.iter().map(|(&p, &r)| (p, r)).collect();
        }
        let mut held: BTreeMap<FormId, u8> = self.extra.get(&actor).map(|x| x.perks.clone()).unwrap_or_default();
        if !self.player.teammate_perks.is_empty() && self.is_teammate(actor) {
            for &p in &self.player.teammate_perks {
                held.entry(p).or_insert(1);
            }
        }
        held.into_iter().collect()
    }

    /// The abilities the player's perks give now (traits included), with their perks.
    pub fn perk_abilities(&self) -> Vec<(FormId, FormId)> {
        self.perk_abilities_of(FormId::PLAYER_REF)
    }

    /// The abilities `actor`'s perks ([`Self::held_perks`]) give now, with their perks.
    pub fn perk_abilities_of(&self, actor: FormId) -> Vec<(FormId, FormId)> {
        let data = self.data;
        let mut out = Vec::new();
        for (perk, rank) in self.held_perks(actor) {
            let Some(def) = self.lu3.perk_edits.get(&perk).or_else(|| data.perks.get(&perk)) else { continue };
            out.extend(rank_abilities(def, rank.max(1)).into_iter().map(|s| (perk, s)));
        }
        out
    }

    /// Whether `actor`'s perk abilities need bringing up to date: the player always; an
    /// actor that is a teammate now, or that still has an ability of a teammate perk (a
    /// teammate that left the team). An ability of a teammate perk the player lost
    /// (`RemovePerk perk 1`) stays on the teammates [design: no shipped script takes one
    /// back from the player; the masters remove Companion Suite from each companion,
    /// which needs nothing here].
    pub(crate) fn wants_perk_sync(&self, actor: FormId) -> bool {
        if actor == FormId::PLAYER_REF || self.extra.get(&actor).is_some_and(|x| !x.perks.is_empty()) {
            return true;
        }
        if self.player.teammate_perks.is_empty() {
            return false;
        }
        if self.is_teammate(actor) {
            return true;
        }
        let Some(spells) = self.extra.get(&actor).map(|x| &x.spells).filter(|s| !s.is_empty()) else { return false };
        let data = self.data;
        self.player.teammate_perks.iter().filter_map(|p| data.perks.get(p)).any(|d| has_rank_ability(d, 1, |a| spells.contains(&a)))
    }

    /// The player's perks' abilities follow the perks: an ability of a perk the player
    /// has joins the player's spell list (so `crate::abilities` keeps its effects on, as
    /// an authored ability's), and one of a perk the player no longer has, or a rank it
    /// has passed, leaves it and its effects end [documented: GECK wiki GetNthPerkEntryType
    /// "Ability"; observed: Good Natured's PerkGoodNatured, +5 to five non-combat skills
    /// and −5 to five combat ones, the trait's description; hypothesis: on while the perk
    /// is held, whichever way it was gained: AddPerk, the level-up menu, a trait].
    /// Logged `perks: <perk>'s ability <spell> on|off`.
    pub fn sync_perk_abilities(&mut self) {
        self.sync_perk_abilities_of(FormId::PLAYER_REF);
    }

    /// [`Self::sync_perk_abilities`] for any actor: a teammate's abilities follow the
    /// teammate perks ([`Self::held_perks`]) and end when it leaves the team.
    pub fn sync_perk_abilities_of(&mut self, actor: FormId) {
        let wanted = self.perk_abilities_of(actor);
        let data = self.data;
        let have: Vec<FormId> = self.extra.get(&actor).map(|x| x.spells.clone()).unwrap_or_default();
        let stale: Vec<FormId> = have
            .iter()
            .copied()
            .filter(|s| !wanted.iter().any(|w| w.1 == *s))
            .filter(|s| data.perks.values().any(|p| p.entries.iter().any(|e| matches!(e.effect, PerkEffect::Ability(a) if a == *s))))
            .collect();
        let new: Vec<(FormId, FormId)> = wanted.into_iter().filter(|w| !have.contains(&w.1)).collect();
        if stale.is_empty() && new.is_empty() {
            return;
        }
        let x = self.extra.entry(actor).or_default();
        x.spells.retain(|s| !stale.contains(s));
        x.dispelled.retain(|s| !stale.contains(s));
        for &(_, spell) in &new {
            if !x.spells.contains(&spell) {
                x.spells.push(spell);
            }
        }
        let who = data.name(actor);
        for spell in stale {
            tracing::info!("perks: {who}'s ability {} off", data.name(spell));
            self.dispel(actor, None, |e| e.source == spell && e.worn.is_none());
        }
        for (perk, spell) in new {
            tracing::info!("perks: {who}: {}'s ability {} on", data.name(perk), data.name(spell));
        }
    }

    /// Whether `actor` holds a perk with an entry of `point` at the rank it has, its
    /// conditions not asked: for flag entry points whose entries have none (Jury Rigging),
    /// where only a shared borrow of the game is at hand.
    pub fn holds_entry_point(&self, actor: FormId, point: u8) -> bool {
        self.held_perks(actor).into_iter().any(|(perk, rank)| {
            self.perk_def(perk).is_some_and(|d| d.entries.iter().any(|e| matches!(&e.effect, PerkEffect::EntryPoint(ep) if ep.point == point) && e.rank < rank.max(1)))
        })
    }

    /// Whether the player's entries of a flag entry point (Set 1: Silent Running, Long
    /// Haul, Infiltrator, Computer Whiz) are on now.
    pub fn player_has_entry(&mut self, point: u8) -> bool {
        self.entry_point(FormId::PLAYER_REF, point, 0.0).0 != 0.0
    }

    /// The player's weight perks and carry capacity brought up to date (every frame, with
    /// the abilities): Pack Rat's Modify Light Items halves units of at most
    /// `fPackRatThreshold` (× `fPackRatModifier`) [documented: GECK wiki Entry Point, the
    /// settings' names; observed: FalloutNV.esm sets neither; the perk's text, "items with
    /// a weight of 2 or less weigh half as much", gives the defaults 2 and 0.5]; Adjust
    /// Heavy Weapon Weight multiplies weapons of [`HEAVY_WEAPON_WEIGHT`] or more
    /// (Heavyweight × 0.5); Get Max Carry Weight changes the capacity (Hoarder + 25,
    /// `crate::functions::actor::sync_carry`). Logged `perks: the player's item weights: …`
    /// when they change.
    pub fn sync_perk_weights(&mut self) {
        use fnv_sim::inventory::WeightRules;
        let p = FormId::PLAYER_REF;
        let light = self.player_has_entry(MODIFY_LIGHT_ITEMS).then(|| {
            (crate::combat::setting(self, "fPackRatThreshold", 2.0) as f32, crate::combat::setting(self, "fPackRatModifier", 0.5) as f32)
        });
        let heavy = self.entry_point(p, ADJUST_HEAVY_WEAPON_WEIGHT, 1.0).0 as f32;
        // The player's weightless tweaks ride along [design: docs/design/tweaks.md].
        let t = &self.tweaks;
        let rules = WeightRules {
            light,
            heavy_weapons: ((heavy - 1.0).abs() > 1e-6).then_some((HEAVY_WEAPON_WEIGHT, heavy)),
            weightless: t.weightless_kinds,
            weightless_worn_armor: t.weightless_worn_armor,
            weightless_worn_power_armor: t.weightless_worn_power_armor,
            ammo_weight: t.hardcore_tweaks.then_some(t.hardcore_ammo_weight),
        };
        if self.world.weight_rules(p) != rules {
            let light = rules.light.map_or("light items as they are".to_string(), |(w, m)| format!("items of {w} or less x {m}"));
            let heavy = rules.heavy_weapons.map_or("heavy weapons as they are".to_string(), |(w, m)| format!("weapons of {w} or more x {m}"));
            tracing::info!("perks: the player's item weights: {light}, {heavy}");
            self.world.set_weight_rules(p, rules);
        }
        crate::functions::actor::sync_carry(self, p, crate::actor_values::CARRY_WEIGHT);
    }

    /// The entries of `point` that apply to `actor` now, in the order they apply. `tabs`:
    /// the forms of the entry point's argument tabs 1 and 2 (`None`: none known here);
    /// `other`: the actor the owner's conditions take as their target.
    pub(crate) fn entry_point_entries(&mut self, actor: FormId, point: u8, tabs: &[Option<FormId>], other: Option<FormId>) -> Vec<(FormId, PerkEntry)> {
        // The player's perks, or a teammate's teammate perks ([`Self::held_perks`];
        // `AddPerk` on another actor is refused).
        let owned = self.held_perks(actor);
        if owned.is_empty() {
            return Vec::new();
        }
        let data = self.data;
        let mut found: Vec<(FormId, PerkEntry)> = Vec::new();
        for (perk, rank) in owned {
            // A perk whose entries a script changed reads its session copy
            // (`crate::nvse_lu3`).
            let edited = self.lu3.perk_edits.get(&perk).cloned();
            let Some(def) = edited.as_ref().or_else(|| data.perks.get(&perk)) else { continue };
            let at = |e: &&PerkEntry| matches!(&e.effect, PerkEffect::EntryPoint(ep) if ep.point == point) && e.rank < rank.max(1);
            let Some(top) = def.entries.iter().filter(at).map(|e| e.rank).max() else { continue };
            'entry: for e in def.entries.iter().filter(at).filter(|e| e.rank == top) {
                let PerkEffect::EntryPoint(ep) = &e.effect else { continue };
                let Some(conds) = ep.conditions.as_ref() else { continue };
                let label = format!("{} entry point {point}", data.name(perk));
                for tab in tab_order(conds) {
                    let list: Vec<_> = conds.iter().filter(|(t, _)| *t == tab).map(|(_, c)| c.clone()).collect();
                    let (subject, target) = match tab {
                        0 => (Some(actor), other),
                        t if t > 0 => match tabs.get(t as usize - 1).copied().flatten() {
                            Some(f) => (Some(f), Some(actor)),
                            None => continue 'entry,
                        },
                        _ => continue 'entry,
                    };
                    if !self.conditions_pass(&list, subject, target, &label) {
                        continue 'entry;
                    }
                }
                found.push((perk, e.clone()));
            }
        }
        found.sort_by_key(|(p, e)| (e.priority, *p));
        found
    }

    /// The number entries of `point` that apply to `actor` with the argument tabs `tabs`
    /// and the other actor `other` ([`Self::entry_point_entries`]), in the order they
    /// apply, with their perks.
    pub fn entry_ops(&mut self, actor: FormId, point: u8, tabs: &[Option<FormId>], other: Option<FormId>) -> Vec<(FormId, Op)> {
        let mut out = Vec::new();
        for (perk, e) in self.entry_point_entries(actor, point, tabs, other) {
            let PerkEffect::EntryPoint(ep) = &e.effect else { continue };
            // Only Add Range To Value rolls; only the actor value functions read a value.
            let op = if ep.function == ADD_RANGE_TO_VALUE {
                let rng = &mut self.rng;
                resolve(ep, &mut |_| 0.0, &mut || {
                    use rand_core::RngCore;
                    (rng.next_u32() >> 8) as f64 / (1u32 << 24) as f64
                })
            } else {
                resolve(ep, &mut |code| crate::functions::actor_value_capped(self, actor, code), &mut || 0.0)
            };
            if let Some(op) = op {
                out.push((perk, op));
            }
        }
        out
    }

    /// The value after the perk entries of `point` the actor has (no argument tabs); the
    /// perks that changed it, in order.
    pub fn entry_point(&mut self, actor: FormId, point: u8, value: f64) -> (f64, Vec<Applied>) {
        self.entry_point_with(actor, point, &[], value)
    }

    /// [`Self::entry_point`] with the argument tabs `tabs` (a weapon's entry points: the
    /// weapon at tab 1).
    pub fn entry_point_with(&mut self, actor: FormId, point: u8, tabs: &[Option<FormId>], value: f64) -> (f64, Vec<Applied>) {
        let mut v = value;
        let mut applied = Vec::new();
        for (perk, op) in self.entry_ops(actor, point, tabs, None) {
            v = op.apply(v);
            applied.push(Applied { perk, value: v });
        }
        (v, applied)
    }
}

/// The tabs an entry's conditions name, in the order they first appear.
fn tab_order(conds: &[(i8, crate::cond::Condition)]) -> Vec<i8> {
    let mut tabs: Vec<i8> = Vec::new();
    for (t, _) in conds {
        if !tabs.contains(t) {
            tabs.push(*t);
        }
    }
    tabs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn functions_set_add_and_multiply() {
        assert_eq!(apply_function(SET_VALUE, 0.0, 5.0), Some(0.0));
        assert_eq!(apply_function(ADD_VALUE, 2.0, 5.0), Some(7.0));
        assert_eq!(apply_function(MULTIPLY_VALUE, 0.5, 0.3), Some(0.15));
        assert_eq!(apply_function(9, 1.0, 5.0), None);
    }
}
