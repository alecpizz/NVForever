//! Aid items (ingestibles: chems, food, drink, medicine) and what magic effects do to
//! actor values. docs/spec-notes/aid-items.md has the rules and their sources.
//!
//! - Using an ingestible ([`Game::ingest`]): its effects apply to the actor
//!   ([`Game::apply_magic_item_with`]) with the beneficial Value Modifier magnitudes
//!   multiplied by the item's potency (Medicine and Food flags, §4); its usage monitor
//!   counts the use and the addiction roll may add the item's withdrawal spell (§5).
//! - A Value Modifier or Value And Parts effect becomes a [`ValueChange`] (§3): with the
//!   Recover flag a modifier held while the effect lasts; without it a restoration or
//!   damage, at once (duration 0) or by the second while it lasts.
//! - An addiction's effects are held back while an effect of a chem that names it as its
//!   withdrawal is active (§5.3); a Cure Addiction effect removes addictions (§5.4).

use crate::data::{
    EffectItem, IngestibleDef, MagicEffectDef, ARCHETYPE_CHAMELEON, ARCHETYPE_CURE_ADDICTION, ARCHETYPE_INVISIBILITY, ARCHETYPE_LIMB_CONDITION,
    ARCHETYPE_PARALYSIS, ARCHETYPE_TURBO, ARCHETYPE_VALUE_AND_PARTS, ARCHETYPE_VALUE_MODIFIER,
};
use crate::functions::actor as fa;
use crate::game::Game;
use crate::trace::EventKind;
use fnv_sim::FormId;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};

/// The usage monitor ("UMON") effect: the EFID 0x0000014F that chems carry with no MGEF
/// record behind it in any master [observed: FalloutNV.esm; documented: GECK wiki
/// Addiction Formula, UMON; hypothesis: that this FormID is the UMON effect, from
/// GetSpellUsageNum's example "the number of times Psycho has been used, multiplied by
/// 30" and the magnitude 30 it has on the chems]. aid-items.md §5.1.
pub const USAGE_MONITOR: FormId = FormId(0x14F);

/// SPEL SPIT type "Addiction" [documented: xEdit SPEL].
pub const SPELL_ADDICTION: i64 = 10;

/// Actor value codes read here [documented: xEdit ActorValue, `schemas/fnv/common.json`].
pub const RAD_LEVEL: u16 = 54;
pub const DEHYDRATION: u16 = 73;
pub const HUNGER: u16 = 74;
pub const SLEEP_DEPRIVATION: u16 = 75;
/// Perception Condition to Brain Condition.
pub const LIMB_CONDITIONS: std::ops::RangeInclusive<u16> = 25..=31;

/// The share of a Value And Parts restore (the Stimpak's "Restore Health & Conditions")
/// that each limb gets when the item is used on the whole body: Health gets the whole
/// amount, each limb this share of it [design: owner's request 2026-10-23, "a small
/// amount of limb health across all limbs"; the share is not in the data (no setting in
/// FalloutNV.esm names it), so this number stands until the formula is found]. Used on
/// one limb from the Pip-Boy (`Mods::limb`), that limb gets the whole amount.
pub const PARTS_LIMB_SHARE: f64 = 1.0 / 6.0;

/// Health a second that one point of a regeneration's magnitude gives back
/// ([`regenerates`]: Rad Child's 2, 4, 6 and 8 at the stages of radiation sickness, Solar
/// Powered's 1): the magnitude per real second, a little every frame [observed:
/// obs284_rad_child_regen, obs284_stage250]. Applied where the effect ticks
/// ([`Game::tick_change_at`]), so saves hold magnitudes.
pub const REGENERATION_RATE: f64 = 1.0;

/// The settings of potency and addiction, with the GECK wiki's defaults [documented:
/// Ingestible Settings, Addiction Formula].
pub const SETTINGS: [(&str, f64); 5] = [
    ("fMagicMedicineSkillBase", 1.0),
    ("fMagicMedicineSkillMult", 2.0),
    ("fMagicSurvivalSkillBase", 1.0),
    ("fMagicSurvivalSkillMult", 2.0),
    ("fAddictionUsageMonitorThreshold", 70.0),
];

/// How a value change is carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    /// Recover: a modifier while the effect lasts, taken away when it ends.
    Modifier,
    /// No Recover, duration 0 (or an ability): restored or damaged once, at the start.
    Once,
    /// No Recover, a duration: `amount` per second while it lasts. On a permanent effect
    /// a regeneration ([`regenerates`]): `amount` × [`REGENERATION_RATE`] per second while
    /// it is active.
    PerSecond,
}

/// What a Value Modifier (or Value And Parts) effect does to one actor value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ValueChange {
    pub code: u16,
    pub kind: ChangeKind,
    /// Signed: positive raises or restores, negative lowers or damages (per second for
    /// [`ChangeKind::PerSecond`]); potency included.
    pub amount: f64,
    /// Value And Parts: the limb conditions are restored with the value.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub parts: bool,
    /// Value And Parts aimed at one limb (its condition code): that limb alone gets the
    /// whole amount (see [`PARTS_LIMB_SHARE`]).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub limb: Option<u16>,
    /// What is in its pool now (Modifier), or what was applied so far (Once, PerSecond).
    #[serde(default)]
    pub applied: f64,
    /// The modifier sits in the permanent pool (a player's ability without conditions).
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub permanent_pool: bool,
    /// Held back: an addiction whose chem is active.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub suspended: bool,
}

/// What a restore of `amount` to `code` with the limbs too ("parts") gives the limbs,
/// besides `code` itself: a Limb Condition effect (`code` a limb, the Doctor's Bag) gives
/// every other limb the same amount when un-aimed [hypothesis: "NVRestoreLimbs" names no
/// actor value; aid-items.md §3], or only the aimed limb at full condition (`amount.max(100.0)`)
/// when aimed from the Pip-Boy's CND page [design: #3431, docs/spec-notes/crippled-limbs.md §1,
/// aid-items.md §3]; a Value And Parts one (the Stimpak) gives each limb
/// [`PARTS_LIMB_SHARE`] of it, or the limb it is aimed at the whole of it.
pub fn parts_restore(code: u16, amount: f64, limb: Option<u16>) -> Vec<(u16, f64)> {
    if LIMB_CONDITIONS.contains(&code) {
        return match limb {
            Some(l) if amount > 0.0 => vec![(l, amount.max(100.0))],
            Some(_) => Vec::new(),
            None => LIMB_CONDITIONS.filter(|&l| l != code).map(|l| (l, amount)).collect(),
        };
    }
    match limb {
        Some(l) => vec![(l, amount)],
        None => LIMB_CONDITIONS.map(|l| (l, amount * PARTS_LIMB_SHARE)).collect(),
    }
}

/// The values that grow as needs grow (rads, and Hardcore's dehydration, hunger and
/// sleep deprivation): a "Damage" effect raises them and a "Restore" effect lowers them,
/// never below 0 [hypothesis: RadAway's effect is "Restore Rads" and Nuka-Cola's
/// "Damage Rads"; GECK wiki GetRadiationLevel reads the player's rads with GetAV].
pub fn grows(code: u16) -> bool {
    matches!(code, RAD_LEVEL | DEHYDRATION | HUNGER | SLEEP_DEPRIVATION)
}

/// S.P.E.C.I.A.L. and skills: a negative temporary modifier does not take them below 0
/// [documented: GECK wiki ActorValue].
fn floored(code: u16) -> bool {
    (5..=11).contains(&code) || (32..=45).contains(&code)
}

/// Whether a base effect that restores `code` acts over and over while a permanent effect
/// (an ability, a stage of radiation sickness) is active: a Value Modifier on Health with
/// No Duration, without Recover, neither Hostile nor Detrimental [hypothesis; aid-items.md
/// §3]. In the masters that is one base effect, `Regen` ("Regeneration"), whose users'
/// own words ask for it ("you will regenerate more and more health", "slowly regenerate
/// lost Health") [observed: FalloutNV.esm and the add-ons]. The plain "Restore Health"
/// (no such flag) and the 10,000 damage of the fatal stages stay at once.
pub fn regenerates(m: &MagicEffectDef, code: u16) -> bool {
    const UNWANTED: u32 = MagicEffectDef::RECOVER | MagicEffectDef::HOSTILE | MagicEffectDef::DETRIMENTAL;
    m.archetype == ARCHETYPE_VALUE_MODIFIER && code == crate::actor_values::HEALTH && m.has(MagicEffectDef::NO_DURATION) && !m.has(UNWANTED)
}

/// The value change of one effect item, or `None` when its base effect changes no actor
/// value (another archetype, or no actor value named). The EFIT's actor value is used,
/// else the MGEF's [hypothesis; aid-items.md §2]. `potency` multiplies the magnitude of
/// an effect that is neither Hostile nor Detrimental [documented: GECK wiki Ingestible,
/// Medicine; Ingestible Settings], and the product is rounded down to a whole number
/// [hypothesis fitting the original: in Hardcore at Medicine 15 the Stimpak's 5 a second
/// gave about 6 a second, 36 in all, not 6.5 (o7_05, 2026-10-31, #1167); the normal one,
/// 30 at Medicine 17's potency 1.34, restored 40 and 48 with Better Healing, not 40.2
/// and 48.24 (captures pk_better_healing*); aid-items.md §4].
pub fn value_change(m: &MagicEffectDef, e: &EffectItem, potency: f64, permanent: bool) -> Option<ValueChange> {
    let (parts, code) = match m.archetype {
        // Invisibility and Chameleon change their actor value as a Value Modifier does
        // [hypothesis: the Stealth Boy's "Stealth Field" names the actor value Chameleon
        // with Recover; detection reads that value, docs/design/detection.md].
        // So do Paralysis and Turbo [hypothesis: their MGEFs name the actor values
        // Paralysis and Turbo with Recover; docs/spec-notes/magic-archetypes.md §2].
        ARCHETYPE_VALUE_MODIFIER | ARCHETYPE_INVISIBILITY | ARCHETYPE_CHAMELEON | ARCHETYPE_PARALYSIS | ARCHETYPE_TURBO => (false, e.actor_value.or(m.actor_value)?),
        ARCHETYPE_VALUE_AND_PARTS => (true, e.actor_value.or(m.actor_value)?),
        // Every limb condition (the first stands for them) [hypothesis: Doctor's Bag's
        // and Hydra's "NVRestoreLimbs" names no actor value; aid-items.md §3].
        ARCHETYPE_LIMB_CONDITION => (true, *LIMB_CONDITIONS.start()),
        _ => return None,
    };
    let harmful = m.has(MagicEffectDef::HOSTILE) || m.has(MagicEffectDef::DETRIMENTAL);
    let sign = if m.has(MagicEffectDef::DETRIMENTAL) { -1.0 } else { 1.0 };
    let magnitude = e.magnitude as f64;
    let amount = sign * if harmful { magnitude } else { (magnitude * potency + 1e-9).floor() };
    let kind = if m.has(MagicEffectDef::RECOVER) && m.archetype != ARCHETYPE_LIMB_CONDITION {
        ChangeKind::Modifier
    } else if permanent && regenerates(m, code) {
        ChangeKind::PerSecond
    } else if e.duration == 0 || permanent {
        ChangeKind::Once
    } else {
        ChangeKind::PerSecond
    };
    Some(ValueChange { code, kind, amount, parts, limb: None, applied: 0.0, permanent_pool: false, suspended: false })
}

/// The potency of an ingestible's beneficial effects for an actor with these Medicine
/// and Survival skills (0–100): `base + mult × skill / 100` per flag [documented: GECK
/// wiki Ingestible Settings]; with both flags the two multiply [hypothesis].
pub fn potency(flags: u8, medicine: f64, survival: f64, setting: impl Fn(&str) -> f64) -> f64 {
    let mut p = 1.0;
    if flags & IngestibleDef::MEDICINE != 0 {
        p *= setting("fMagicMedicineSkillBase") + setting("fMagicMedicineSkillMult") * medicine / 100.0;
    }
    if flags & IngestibleDef::FOOD != 0 {
        p *= setting("fMagicSurvivalSkillBase") + setting("fMagicSurvivalSkillMult") * survival / 100.0;
    }
    p
}

/// The chance (0–1) that this use addicts: 0 without a withdrawal effect or with an
/// Addiction Chance of 0; with a usage monitor `(usage − threshold) / 100` once the
/// usage reaches the threshold, else 0; without one the Addiction Chance itself
/// [documented: GECK wiki Addiction Formula]. `usage` is the monitor's value after this
/// use [hypothesis]. Perks' "Modify Addiction Chance" is applied by [`Game::ingest`].
pub fn addiction_chance(def: &IngestibleDef, usage: Option<f64>, threshold: f64) -> f64 {
    if def.withdrawal.is_none() || def.addiction_chance == 0.0 {
        return 0.0;
    }
    match usage {
        Some(u) if u < threshold => 0.0,
        Some(u) => ((u - threshold) / 100.0).clamp(0.0, 1.0),
        None => (def.addiction_chance as f64).clamp(0.0, 1.0),
    }
}

/// ETYP "Chems" [documented: xEdit EquipType; GECK wiki Equip Type].
pub const EQUIP_CHEMS: i64 = 10;

/// What changes an ingestible's effects for one use (aid-items.md §4, §10).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mods {
    /// Multiplies the beneficial Value Modifier magnitudes (Medicine and Food potency).
    pub potency: f64,
    /// Multiplies the durations of the beneficial effects that have one, the usage
    /// monitor's excepted (perks' "Modify Positive Chem Duration", chems only).
    pub duration: f64,
    /// Multiplies the rads an effect gives (perks' "Modify Radiation Consumed").
    pub rads: f64,
    /// Multiplies the health a beneficial effect restores (perks' "Modify Recovered
    /// Health": Better Healing).
    pub health: f64,
    /// Applied by a hit (a weapon's, a creature's or a blast's effect,
    /// [`Game::apply_hit_effect`]): the target's resistances cut the harmful magnitudes,
    /// and the same item's active effect is refreshed rather than lengthened
    /// (docs/spec-notes/hit-effects.md §2, §3).
    pub hit: bool,
    /// A worn enchantment: the equipped item it comes from (`crate::abilities`).
    pub worn: Option<FormId>,
    /// Used on one limb (its condition code, 25-30) from the Pip-Boy's CND page: a Value
    /// And Parts restore (Stimpak) gives that limb the whole amount ([`PARTS_LIMB_SHARE`]),
    /// and an instant Limb Condition restore (Doctor's Bag in Hardcore) heals that limb alone.
    pub limb: Option<u16>,
    /// Only this effect of the item (an area effect reaching an actor around the target,
    /// `crate::area_effects`).
    pub only: Option<usize>,
}

impl Default for Mods {
    fn default() -> Mods {
        Mods { potency: 1.0, duration: 1.0, rads: 1.0, health: 1.0, hit: false, worn: None, limb: None, only: None }
    }
}

impl Mods {
    pub fn potency(potency: f64) -> Mods {
        Mods { potency, ..Mods::default() }
    }

    /// The seconds an effect item lasts under these mods: a beneficial effect (neither
    /// Hostile nor Detrimental) with a duration is lengthened by `duration`, the usage
    /// monitor not [hypothesis: "positive" effects; the monitor counts uses, it is no
    /// effect of the chem].
    pub fn seconds(&self, m: Option<&MagicEffectDef>, e: &EffectItem) -> f64 {
        let beneficial = m.is_some_and(|m| !m.has(MagicEffectDef::HOSTILE) && !m.has(MagicEffectDef::DETRIMENTAL));
        let d = e.duration as f64;
        if beneficial && e.effect != USAGE_MONITOR && d > 0.0 {
            d * self.duration
        } else {
            d
        }
    }

    /// A value change with the rads factor: an effect that gives rads (a "Damage Rads",
    /// whose amount lowers the rads' "health") gives `rads` times as many.
    pub fn on_change(&self, mut c: ValueChange) -> ValueChange {
        if c.code == RAD_LEVEL && c.amount < 0.0 {
            c.amount *= self.rads;
        }
        if c.code == crate::actor_values::HEALTH && c.amount > 0.0 {
            c.amount *= self.health;
        }
        // Value And Parts (Stimpak) and instant Limb Condition (Hardcore Doctor's Bag) accept
        // a single targeted limb; duration Limb Condition (Hydra) restores all limbs over time.
        if c.parts && (!LIMB_CONDITIONS.contains(&c.code) || c.kind == ChangeKind::Once) {
            c.limb = self.limb;
        }
        c
    }
}

/// One effect a magic item started, or an active one it extended
/// ([`Game::apply_magic_item_with`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Applied {
    pub id: u32,
    /// The base effect.
    pub effect: FormId,
    /// Its value change as it took hold (a modifier's amount as cut, potency included).
    pub change: Option<ValueChange>,
    /// Seconds: its own duration, or the extended effect's whole duration.
    pub duration: f64,
    /// The same item's effect was active: this use lengthened it.
    pub extended: bool,
    /// Its conditions failed: it is listed inactive and changes nothing until they pass
    /// (`Game::apply_magic_item_with`).
    pub inactive: bool,
}

/// What using an ingestible did ([`Game::ingest`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ingested {
    /// The effects started (an effect merged into an active one is not listed).
    pub effects: Vec<u32>,
    /// Every effect started or extended, with its value change.
    pub applied: Vec<Applied>,
    pub potency: f64,
    /// The factors this use had (potency, perks).
    pub mods: Mods,
    /// The perks whose entry points changed this use: the perk and the entry point.
    pub perks: Vec<(FormId, u8)>,
    /// The usage monitor's value after this use, when the item has one.
    pub usage: Option<f64>,
    /// The addiction roll: its chance and whether it addicted.
    pub addiction: Option<(f64, bool)>,
}

impl Game<'_> {
    fn aid_setting(&self, name: &str) -> f64 {
        let default = SETTINGS.iter().find(|(n, _)| *n == name).map_or(0.0, |s| s.1);
        crate::combat::setting(self, name, default)
    }

    /// What using an ingestible would change for `actor`, one entry per actor value in the
    /// order the effects name them: only the effects whose conditions pass now (the perk
    /// checks of skill magazines, Stimpaks and Blood Packs, Hardcore), with the potency
    /// [`Game::ingest`] would use; an effect over time counted whole (its amount a second
    /// times its seconds); effects on the same value added up. Nothing changes.
    /// For the item's EFFECTS figure (docs/spec-notes/aid-items.md §14) [hypothesis: the
    /// original's Stimpak shows "HP +40", 30 at a Medicine potency near 1.33, owner's
    /// screenshot; one line per value, owner's playtest list 7 item 5].
    pub fn aid_preview(&mut self, actor: FormId, item: FormId) -> Vec<(u16, f64)> {
        let data = self.data;
        let base = self.unclone(item);
        let Some(items) = data.effect_items.get(&base) else { return Vec::new() };
        let def = self.ingestible_def(item).unwrap_or_default();
        let medicine = fa::actor_value_capped(self, actor, 37);
        let survival = fa::actor_value_capped(self, actor, 44);
        let potency = potency(def.flags, medicine, survival, |n| self.aid_setting(n));
        let duration = if def.equip_type == EQUIP_CHEMS { self.entry_point(actor, crate::perks::MODIFY_POSITIVE_CHEM_DURATION, 1.0).0 } else { 1.0 };
        let health = self.entry_point(actor, crate::perks::MODIFY_RECOVERED_HEALTH, 1.0).0;
        let mods = Mods { potency, duration, health, ..Mods::default() };
        let mut out: Vec<(u16, f64)> = Vec::new();
        for (index, e) in items.iter().enumerate() {
            let label = format!("{} effect {index} (preview)", data.name(base));
            if !self.conditions_pass(&e.conditions, Some(actor), Some(actor), &label) {
                continue;
            }
            let m = self.magic_effect_def(e.effect);
            let Some(c) = m.as_ref().and_then(|m| value_change(m, e, potency, false)).map(|c| mods.on_change(c)) else { continue };
            let amount = match c.kind {
                ChangeKind::PerSecond => c.amount * mods.seconds(m.as_ref(), e),
                ChangeKind::Once | ChangeKind::Modifier => c.amount,
            };
            match out.iter_mut().find(|(code, _)| *code == c.code) {
                Some(line) => line.1 += amount,
                None => out.push((c.code, amount)),
            }
        }
        out
    }

    /// The base effects of an ingestible shown by their name alone
    /// ([`crate::data::MagicEffectDef::shows_name`]) whose conditions pass for `actor` now,
    /// once each in the order the item names them: for the item's EFFECTS figure beside
    /// [`Game::aid_preview`]'s values (#3119) [hypothesis: the original lists the name;
    /// its layout is not captured].
    pub fn aid_preview_names(&mut self, actor: FormId, item: FormId) -> Vec<FormId> {
        let data = self.data;
        let base = self.unclone(item);
        let Some(items) = data.effect_items.get(&base) else { return Vec::new() };
        let mut out: Vec<FormId> = Vec::new();
        for (index, e) in items.iter().enumerate() {
            if out.contains(&e.effect) || !self.magic_effect_def(e.effect).is_some_and(|m| m.shows_name()) {
                continue;
            }
            let label = format!("{} effect {index} (preview)", data.name(base));
            if self.conditions_pass(&e.conditions, Some(actor), Some(actor), &label) {
                out.push(e.effect);
            }
        }
        out
    }

    /// An actor uses an ingestible (the caller has taken it from the inventory): its
    /// effects apply with the item's potency, its usage monitor counts the use, and the
    /// addiction roll is made. docs/spec-notes/aid-items.md §4–§5.
    pub fn ingest(&mut self, actor: FormId, item: FormId) -> Ingested {
        self.ingest_on(actor, item, None)
    }

    /// [`Game::ingest`] on one limb (its condition code, 25-30), as the Pip-Boy's CND
    /// page uses a Stimpak on the limb chosen: a Value And Parts restore gives that limb
    /// its whole amount and the other limbs nothing ([`PARTS_LIMB_SHARE`]).
    pub fn ingest_on(&mut self, actor: FormId, item: FormId, limb: Option<u16>) -> Ingested {
        // [documented: SetOnUseAidItemEventHandler] this = the actor, the item.
        self.jip_event("useaiditem", Some(actor), vec![fnv_script::vm::XVal::Ref(item.0)]);
        let def = self.ingestible_def(item).unwrap_or_default();
        let medicine = fa::actor_value_capped(self, actor, 37);
        let survival = fa::actor_value_capped(self, actor, 44);
        let potency = potency(def.flags, medicine, survival, |n| self.aid_setting(n));
        let mut perks: Vec<(FormId, u8)> = Vec::new();
        let mut perk = |g: &mut Game, point: u8, value: f64| -> f64 {
            let (v, by) = g.entry_point(actor, point, value);
            perks.extend(by.iter().map(|a| (a.perk, point)));
            v
        };
        // Chemist and the like lengthen chems only [hypothesis: the perk's "any chems you
        // take"; the GECK wiki's entry point says "any ingestible"].
        let duration = if def.equip_type == EQUIP_CHEMS { perk(self, crate::perks::MODIFY_POSITIVE_CHEM_DURATION, 1.0) } else { 1.0 };
        let rads = perk(self, crate::perks::MODIFY_RADIATION_CONSUMED, 1.0);
        let health = perk(self, crate::perks::MODIFY_RECOVERED_HEALTH, 1.0);
        let mods = Mods { potency, duration, rads, health, limb, ..Mods::default() };
        self.emit(EventKind::Ingested { actor, item, potency });
        // Effects whose conditions fail (a Stimpak's Hardcore effects outside Hardcore) are
        // listed inactive and changed nothing: not part of what the use did.
        let applied: Vec<Applied> = self.apply_magic_item_with(actor, item, Some(actor), false, &mods).into_iter().filter(|a| !a.inactive).collect();
        if actor == FormId::PLAYER_REF {
            for (code, n) in crate::misc_stats::ingest_counts(&def, &applied) {
                self.count_stat(code, n);
            }
            self.challenge_used(item);
        }
        let effects = applied.iter().filter(|a| !a.extended).map(|a| a.id).collect();
        let has_monitor = self.data.effect_items.get(&self.unclone(item)).is_some_and(|l| l.iter().any(|e| e.effect == USAGE_MONITOR));
        let usage = has_monitor.then(|| self.spell_usage(actor, item));
        let threshold = self.aid_setting("fAddictionUsageMonitorThreshold");
        let mut chance = addiction_chance(&def, usage, threshold);
        // The player's tweak: only the player gets addicted [design: docs/design/tweaks.md].
        if self.tweaks.no_npc_addiction && actor != FormId::PLAYER_REF {
            chance = 0.0;
        }
        // "Addiction Chance accounts for the Modify Addiction Chance Perk Entry Point"
        // [documented: GECK wiki Addiction Formula].
        if chance > 0.0 {
            chance = perk(self, crate::perks::MODIFY_ADDICTION_CHANCE, chance).clamp(0.0, 1.0);
        }
        let mut addiction = None;
        if let Some(spell) = def.withdrawal.filter(|_| def.addiction_chance != 0.0) {
            let already = self.extra.get(&actor).is_some_and(|e| e.spells.contains(&spell));
            if !already {
                let hit = chance > 0.0 && ((self.rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64) < chance;
                if hit {
                    self.add_spell(actor, spell);
                    if actor == FormId::PLAYER_REF {
                        self.count_stat(crate::misc_stats::TIMES_ADDICTED, 1.0);
                    }
                    self.emit(EventKind::Addicted { actor, item, spell });
                }
                addiction = Some((chance, hit));
            }
        }
        Ingested { effects, applied, potency, mods, perks, usage, addiction }
    }

    /// AddSpell: the spell's effects apply at once and last until removed; false when
    /// the actor already has it.
    pub fn add_spell(&mut self, actor: FormId, spell: FormId) -> bool {
        let e = self.extra.entry(actor).or_default();
        if e.spells.contains(&spell) {
            return false;
        }
        e.spells.push(spell);
        // Conditions may read actor values, which sync listed abilities. The new
        // spell is already listed but has no ActiveEffect until admission completes.
        let syncing = self.syncing_effects;
        self.syncing_effects = true;
        self.apply_magic_item(actor, spell, None, true);
        self.syncing_effects = syncing;
        true
    }

    /// GetSpellUsageNum: the usage monitor's value for an ingestible on the actor, 0
    /// when none is active [documented: GECK wiki GetSpellUsageNum].
    pub fn spell_usage(&self, actor: FormId, item: FormId) -> f64 {
        self.effects.iter().find(|e| e.target == actor && e.source == item && e.effect == USAGE_MONITOR).map_or(0.0, |e| e.usage)
    }

    /// The addictions (spells of type Addiction) an actor has.
    pub fn addictions(&self, actor: FormId) -> Vec<FormId> {
        let Some(e) = self.extra.get(&actor) else { return Vec::new() };
        e.spells.iter().copied().filter(|&s| self.spell_type(s) == Some(SPELL_ADDICTION)).collect()
    }

    /// Whether an addiction is sated: an effect (not the usage monitor) of an ingestible
    /// whose withdrawal effect is this spell is active on the actor [hypothesis: GECK
    /// wiki IsSpellTarget, an addicted player who has just had the chem "is not presently
    /// undergoing withdrawal"].
    fn sated(&self, actor: FormId, spell: FormId) -> bool {
        self.effects.iter().any(|e| {
            e.target == actor
                && e.effect != USAGE_MONITOR
                && self.ingestible_def(e.source).is_some_and(|d| d.withdrawal == Some(spell))
        })
    }

    /// Change an actor value at once by `amount` (positive restores, negative damages);
    /// a value that grows (rads) moves the other way (see [`grows`]).
    /// In god mode (`tgm`) the player takes no damage from an effect: a need does not
    /// rise either [hypothesis fitting the original: Beer and Nuka-Cola, whose "Damage
    /// Dehydration" holds in Hardcore, left Dehydration at its normal rise when drunk
    /// with tgm on (o7_00, 2026-10-31, #1167)].
    fn change_value(&mut self, actor: FormId, code: u16, amount: f64, parts: bool, limb: Option<u16>, caster: Option<FormId>) {
        if amount < 0.0 && actor == FormId::PLAYER_REF && self.toggles.god_mode {
            return;
        }
        // ToggleImmortalMode (`crate::nvse_jip9`): 2 no Health damage, 1 none below 1 HP.
        let mut amount = amount;
        if actor == FormId::PLAYER_REF && code == crate::actor_values::HEALTH && amount < 0.0 {
            match self.immortal_mode() {
                2 => return,
                1 => {
                    let now = fa::actor_value(self, actor, code).unwrap_or(0.0);
                    amount = amount.max(-(now - 1.0).max(0.0));
                }
                _ => {}
            }
        }
        if code == crate::actor_values::HEALTH && amount < 0.0 {
            // Health damage can kill (a burn, a poison; hit-effects.md §4).
            self.effect_health_damage(actor, -amount, caster);
        } else if grows(code) {
            let now = fa::actor_value(self, actor, code).unwrap_or(0.0);
            let to = (now - amount).max(0.0);
            let _ = fa::shift_base(self, actor, code, to - now);
        } else if amount >= 0.0 {
            if parts && LIMB_CONDITIONS.contains(&code) && limb.is_some() {
                for (l, a) in parts_restore(code, amount, limb) {
                    fa::restore(self, actor, l, a);
                }
            } else {
                fa::restore(self, actor, code, amount);
                if parts {
                    for (l, a) in parts_restore(code, amount, limb) {
                        fa::restore(self, actor, l, a);
                    }
                }
            }
        } else {
            fa::add_damage(self, actor, code, -amount);
        }
    }

    fn change_of(&self, id: u32) -> Option<(FormId, ValueChange)> {
        self.effects.iter().find(|e| e.id == id).and_then(|e| Some((e.target, e.change?)))
    }

    fn set_change(&mut self, id: u32, c: ValueChange) {
        self.set_change_at(id, None, c);
    }

    /// [`Game::set_change`] with the effect's position, if known (checked by id).
    fn set_change_at(&mut self, id: u32, hint: Option<usize>, c: ValueChange) {
        if let Some(i) = crate::game::effect_index(&self.effects, id, hint) {
            self.effects[i].change = Some(c);
        }
    }

    /// Put a modifier in its pool; a negative one on S.P.E.C.I.A.L. or a skill is cut so
    /// the value stays at 0 or more [documented: GECK wiki ActorValue].
    pub(crate) fn hold_modifier(&mut self, target: FormId, c: &mut ValueChange) {
        let mut amount = c.amount;
        if amount < 0.0 && floored(c.code) {
            let now = fa::actor_value(self, target, c.code).unwrap_or(0.0).max(0.0);
            amount = amount.max(-now);
        }
        if c.permanent_pool {
            let _ = fa::add_modifier(self, target, c.code, amount);
        } else {
            fa::add_temporary(self, target, c.code, amount);
        }
        c.applied = amount;
    }

    fn release_modifier(&mut self, target: FormId, c: &mut ValueChange) {
        if c.applied != 0.0 {
            if c.permanent_pool {
                let _ = fa::add_modifier(self, target, c.code, -c.applied);
            } else {
                fa::add_temporary(self, target, c.code, -c.applied);
            }
        }
        c.applied = 0.0;
    }

    /// An effect started: its value change takes hold.
    pub(crate) fn start_change(&mut self, id: u32) {
        let Some((target, mut c)) = self.change_of(id) else { return };
        let (permanent, source, index) = match self.effects.iter().find(|e| e.id == id) {
            Some(e) => (e.permanent, e.source, e.index),
            None => return,
        };
        match c.kind {
            ChangeKind::Modifier => {
                // A player's ability without conditions changes the permanent pool
                // [documented: GECK wiki ActorValue, the Life Giver example].
                let unconditional = self.data.effect_items.get(&self.unclone(source)).and_then(|l| l.get(index)).is_none_or(|e| e.conditions.is_empty());
                let ability = permanent && self.spell_type(source) == Some(4);
                c.permanent_pool = ability && target == FormId::PLAYER_REF && unconditional;
                c.suspended = self.spell_type(source) == Some(SPELL_ADDICTION) && self.sated(target, source);
                if !c.suspended {
                    self.hold_modifier(target, &mut c);
                }
            }
            ChangeKind::Once => {
                let caster = self.effects.iter().find(|e| e.id == id).and_then(|e| e.caster);
                self.change_value(target, c.code, c.amount, c.parts, c.limb, caster);
                c.applied = c.amount;
            }
            ChangeKind::PerSecond => {}
        }
        self.set_change(id, c);
    }

    /// An effect is about to end: a modifier leaves its pool.
    pub(crate) fn end_change(&mut self, id: u32) {
        let Some((target, mut c)) = self.change_of(id) else { return };
        if c.kind == ChangeKind::Modifier {
            self.release_modifier(target, &mut c);
            self.set_change(id, c);
        }
    }

    /// One frame of an effect that changes a value by the second: the share of the
    /// seconds from `before` to its elapsed time that lie inside its duration. A permanent
    /// one (a regeneration, [`regenerates`]) has no duration: all the seconds count, at
    /// [`REGENERATION_RATE`], on a living actor that is hurt [hypothesis: the dead do not
    /// regenerate].
    /// `hint` is the effect's position, if known (checked by id): one lookup serves the
    /// whole tick (#1389); the change is written back by id again, as changing the value
    /// can run scripts that move the effect.
    pub(crate) fn tick_change_at(&mut self, id: u32, hint: Option<usize>, before: f64) {
        let Some(i) = crate::game::effect_index(&self.effects, id, hint) else { return };
        let e = &self.effects[i];
        let Some(mut c) = e.change else { return };
        if c.kind != ChangeKind::PerSecond {
            return;
        }
        let (target, caster, permanent) = (e.target, e.caster, e.permanent);
        let seconds = if permanent { (e.elapsed - before) * REGENERATION_RATE } else { e.elapsed.min(e.duration) - before.min(e.duration) };
        if seconds <= 0.0 {
            return;
        }
        if permanent && (self.is_dead(target) || fa::damage(self, target, c.code) <= 0.0) {
            return;
        }
        let amount = c.amount * seconds;
        self.change_value(target, c.code, amount, c.parts, c.limb, caster);
        c.applied += amount;
        self.set_change_at(id, Some(i), c);
    }

    /// After a load: a regeneration saved as one restoration at the start (before #3471)
    /// goes on by the second, rather than waiting for its stage to be entered again.
    pub(crate) fn renew_regenerations(&mut self) {
        for i in 0..self.effects.len() {
            let e = &self.effects[i];
            let Some(c) = e.change.filter(|c| e.permanent && c.kind == ChangeKind::Once) else { continue };
            if self.magic_effect_def(e.effect).is_some_and(|m| regenerates(&m, c.code)) {
                self.effects[i].change = Some(ValueChange { kind: ChangeKind::PerSecond, applied: 0.0, ..c });
            }
        }
    }

    /// Addictions are held back while their chem is active, and come back when it ends.
    /// An effect its conditions hold is left alone: when they pass, [`Game::start_change`]
    /// looks at the chem itself, and a modifier put in its pool here would go in twice.
    pub(crate) fn refresh_suspended(&mut self, _dt: f64) {
        let ids: Vec<(u32, FormId, FormId)> = self
            .effects
            .iter()
            .filter(|e| e.permanent && !e.inactive && e.change.is_some_and(|c| c.kind == ChangeKind::Modifier))
            .filter(|e| self.spell_type(e.source) == Some(SPELL_ADDICTION))
            .map(|e| (e.id, e.target, e.source))
            .collect();
        for (id, target, spell) in ids {
            let Some((_, mut c)) = self.change_of(id) else { continue };
            let sated = self.sated(target, spell);
            if sated == c.suspended {
                continue;
            }
            if sated {
                self.release_modifier(target, &mut c);
            } else {
                self.hold_modifier(target, &mut c);
            }
            c.suspended = sated;
            self.set_change(id, c);
        }
    }

    /// What an effect's archetype does when it starts, besides value changes and scripts:
    /// Cure Addiction removes the target's addictions [hypothesis from the archetype's
    /// name; aid-items.md §5.4]; Paralysis, the cures and Dispel:
    /// `crate::magic_archetypes`.
    pub(crate) fn effect_archetype_start(&mut self, id: u32) {
        let Some(e) = self.effects.iter().find(|e| e.id == id) else { return };
        let (target, effect) = (e.target, e.effect);
        let mgef = self.magic_effect_def(effect);
        let archetype = mgef.as_ref().map(|m| m.archetype);
        if let Some(a) = archetype.filter(|_| mgef.as_ref().is_some_and(crate::magic_archetypes::acts_on_start)) {
            self.archetype_start(id, a);
        }
        if archetype == Some(ARCHETYPE_CURE_ADDICTION) {
            for spell in self.addictions(target) {
                if let Some(x) = self.extra.get_mut(&target) {
                    x.spells.retain(|&s| s != spell);
                }
                self.dispel(target, None, |e| e.source == spell);
                self.emit(EventKind::AddictionCured { actor: target, spell });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgef(archetype: i64, flags: u32, av: Option<u16>) -> MagicEffectDef {
        MagicEffectDef { flags, archetype, script: None, actor_value: av, shader: None, resistance: None, effect_sound: None, hit_sound: None }
    }

    fn item(magnitude: u32, duration: u32, av: Option<u16>) -> EffectItem {
        EffectItem { effect: FormId(0x800), magnitude, area: 0, duration, actor_value: av, conditions: Vec::new() }
    }

    const R: u32 = MagicEffectDef::RECOVER;
    const D: u32 = MagicEffectDef::DETRIMENTAL;
    const H: u32 = MagicEffectDef::HOSTILE;

    #[test]
    fn the_flags_and_duration_pick_the_kind() {
        // Buffout's Strength: Recover, a duration.
        let c = value_change(&mgef(0, R, Some(5)), &item(2, 240, Some(5)), 1.0, false).unwrap();
        assert_eq!((c.kind, c.amount, c.code), (ChangeKind::Modifier, 2.0, 5));
        // Hardcore Stimpak: no Recover, 5 a second for 6 s; 6.5 rounds down to 6.
        let c = value_change(&mgef(0, 0, Some(16)), &item(5, 6, Some(16)), 1.3, false).unwrap();
        assert_eq!((c.kind, c.amount), (ChangeKind::PerSecond, 6.0));
        // Value And Parts at once; withdrawal's Reduce Strength is a negative modifier.
        let c = value_change(&mgef(34, 0, Some(16)), &item(30, 0, Some(16)), 1.0, false).unwrap();
        assert_eq!((c.kind, c.parts), (ChangeKind::Once, true));
        let c = value_change(&mgef(0, R | D, Some(5)), &item(1, 0, Some(5)), 2.0, true).unwrap();
        assert_eq!((c.kind, c.amount), (ChangeKind::Modifier, -1.0));
        // Harmful effects ignore potency; other archetypes change no value.
        let c = value_change(&mgef(0, H | D, Some(54)), &item(3, 0, Some(54)), 2.0, false).unwrap();
        assert_eq!(c.amount, -3.0);
        assert!(value_change(&mgef(1, 0, None), &item(0, 0, None), 1.0, false).is_none());
        // Limb Condition: every limb, by the second here (Hydra).
        let c = value_change(&mgef(35, 0, None), &item(10, 60, None), 1.0, false).unwrap();
        assert_eq!((c.code, c.parts, c.kind), (25, true, ChangeKind::PerSecond));
        // The EFIT's actor value, else the MGEF's.
        assert_eq!(value_change(&mgef(0, 0, Some(16)), &item(1, 0, None), 1.0, false).unwrap().code, 16);
        assert_eq!(value_change(&mgef(0, 0, Some(16)), &item(1, 0, Some(20)), 1.0, false).unwrap().code, 20);
    }

    /// #3471: only a permanent "Regeneration" effect (Health, No Duration, no Recover,
    /// harmless) goes by the second; the rest of a permanent effect's restorations and
    /// damage stay at once, and so does the same base effect cast once.
    #[test]
    fn only_a_permanent_regeneration_goes_by_the_second() {
        let n = MagicEffectDef::NO_DURATION;
        let kind = |flags: u32, av: u16, permanent: bool| value_change(&mgef(0, flags, Some(av)), &item(6, 0, Some(av)), 1.0, permanent).unwrap().kind;
        assert_eq!(kind(n, 16, true), ChangeKind::PerSecond);
        assert_eq!(kind(n, 16, false), ChangeKind::Once);
        // "Restore Health" (GhoulRadHeal), the fatal stages' damage, a drain, another value.
        assert_eq!(kind(0, 16, true), ChangeKind::Once);
        assert_eq!(kind(H | D, 16, true), ChangeKind::Once);
        assert_eq!(kind(n | D, 16, true), ChangeKind::Once);
        assert_eq!(kind(n | H, 16, true), ChangeKind::Once);
        assert_eq!(kind(n, 15, true), ChangeKind::Once);
        assert_eq!(kind(n | R, 16, true), ChangeKind::Modifier);
    }

    #[test]
    fn a_stimpak_spreads_a_share_over_the_limbs_unless_aimed() {
        // On the whole body: each limb a share of the amount.
        let spread = parts_restore(16, 39.0, None);
        assert_eq!(spread.len(), 7);
        assert!(spread.iter().all(|&(_, a)| (a - 39.0 * PARTS_LIMB_SHARE).abs() < 1e-9));
        // Aimed at the left arm (27): that limb alone, the whole amount.
        assert_eq!(parts_restore(16, 39.0, Some(27)), vec![(27, 39.0)]);
        // Limb Condition (the Doctor's Bag): every other limb the whole amount when un-aimed;
        // aimed at one limb, only that limb at full condition (#3431).
        let bag = parts_restore(25, 46.0, None);
        assert_eq!(bag.len(), 6);
        assert!(bag.iter().all(|&(l, a)| l != 25 && a == 46.0));
        assert_eq!(parts_restore(25, 46.0, Some(27)), vec![(27, 100.0)]);
        assert_eq!(parts_restore(25, 108.0, Some(27)), vec![(27, 108.0)]);
        // Value And Parts (Stimpak) and instant Limb Condition (Doctor's Bag) take an aimed
        // limb; duration Limb Condition (Hydra) restores all limbs over time.
        let m = Mods { limb: Some(27), ..Mods::default() };
        let stim = value_change(&mgef(34, 0, Some(16)), &item(30, 0, Some(16)), 1.0, false).unwrap();
        assert_eq!(m.on_change(stim).limb, Some(27));
        let drbag = value_change(&mgef(35, 0, None), &item(36, 0, None), 1.3, false).unwrap();
        assert_eq!(m.on_change(drbag).limb, Some(27));
        let hydra = value_change(&mgef(35, 0, None), &item(10, 60, None), 1.0, false).unwrap();
        assert_eq!(m.on_change(hydra).limb, None);
    }

    #[test]
    fn better_healing_multiplies_restored_health_only() {
        // The original game (captures pk_better_healing*): a Stimpak, 30 at Medicine 17's
        // potency, restored 40 and 48 with Better Healing; Medicine 100: 90 and 108.
        let s = |n: &str| SETTINGS.iter().find(|x| x.0 == n).unwrap().1;
        let m = Mods { health: 1.2, ..Mods::default() };
        for (medicine, without, with) in [(17.0, 40.0, 48.0), (100.0, 90.0, 108.0)] {
            let p = potency(IngestibleDef::MEDICINE, medicine, 0.0, s);
            let stim = value_change(&mgef(34, 0, Some(16)), &item(30, 0, Some(16)), p, false).unwrap();
            assert!((stim.amount - without).abs() < 1e-6, "{}", stim.amount);
            assert!((m.on_change(stim).amount - with).abs() < 1e-6);
        }
        // A Brahmin Wellington, 5 a second at Survival 50's potency 2: 10, with the perk 12.
        let p = potency(IngestibleDef::FOOD, 0.0, 50.0, s);
        let food = value_change(&mgef(0, 0, Some(16)), &item(5, 10, Some(16)), p, false).unwrap();
        assert!((m.on_change(food).amount - 12.0).abs() < 1e-9);
        // Other values, and harm to health, are left alone.
        let water = value_change(&mgef(0, 0, Some(5)), &item(5, 0, Some(5)), 1.0, false).unwrap();
        assert_eq!(m.on_change(water).amount, water.amount);
        let harm = value_change(&mgef(0, MagicEffectDef::DETRIMENTAL, Some(16)), &item(5, 0, Some(16)), 1.0, false).unwrap();
        assert_eq!(m.on_change(harm).amount, harm.amount);
    }

    #[test]
    fn the_hardcore_stimpak_heals_whole_points() {
        // The original game in Hardcore at Medicine 15 (o7_05, #1167): about 6 a second
        // for 6 s, 36 in all, from the record's 5 a second at potency 1.3.
        let s = |n: &str| SETTINGS.iter().find(|x| x.0 == n).unwrap().1;
        let p = potency(IngestibleDef::MEDICINE, 15.0, 0.0, s);
        let c = value_change(&mgef(0, 0, Some(16)), &item(5, 6, Some(16)), p, false).unwrap();
        assert_eq!((c.kind, c.amount * 6.0), (ChangeKind::PerSecond, 36.0));
        // The normal Stimpak's 30 at once stays 39.
        let c = value_change(&mgef(34, 0, Some(16)), &item(30, 0, Some(16)), p, false).unwrap();
        assert_eq!(c.amount, 39.0);
    }

    #[test]
    fn potency_follows_the_wiki_examples() {
        let s = |n: &str| SETTINGS.iter().find(|x| x.0 == n).unwrap().1;
        // Ingestible Settings: Survival 55 → 2.1; Medicine 10 → 1.2.
        assert!((potency(IngestibleDef::FOOD, 0.0, 55.0, s) - 2.1).abs() < 1e-9);
        assert!((potency(IngestibleDef::MEDICINE, 10.0, 0.0, s) - 1.2).abs() < 1e-9);
        assert_eq!(potency(0, 100.0, 100.0, s), 1.0);
    }

    #[test]
    fn addiction_chance_follows_the_formula() {
        let def = IngestibleDef { withdrawal: Some(FormId(0x900)), addiction_chance: 0.1, ..Default::default() };
        assert!((addiction_chance(&def, None, 70.0) - 0.1).abs() < 1e-6);
        assert_eq!(addiction_chance(&def, Some(60.0), 70.0), 0.0);
        assert!((addiction_chance(&def, Some(90.0), 70.0) - 0.2).abs() < 1e-9);
        let none = IngestibleDef { addiction_chance: 0.1, ..Default::default() };
        assert_eq!(addiction_chance(&none, Some(300.0), 70.0), 0.0);
        let zero = IngestibleDef { withdrawal: Some(FormId(0x900)), ..Default::default() };
        assert_eq!(addiction_chance(&zero, Some(300.0), 70.0), 0.0);
    }
}
