//! Item condition (docs/spec-notes/combat-rules.md §17): how weapons and armour wear,
//! which armour a hit wears, and what a worn item is still worth.
//!
//! An item's condition is a fraction of its record's health (WEAP and ARMO `DATA`
//! health), kept with the carried item (`fnv_sim::inventory::Extra::condition`).
//!
//! - **A weapon** loses `fDamageToWeaponValue` health points per attack (0.2 in
//!   FalloutNV.esm [observed]), or its record's Damage to Weapon Mult instead when the
//!   record overrides it, with the ammunition's Weapon Condition effects applied
//!   [documented: GECK wiki SetWeaponDegradationMult (ppNVSE), the override "overrides
//!   the fDamageToWeaponValue when calculating the per shot degradation"; its example's
//!   0.5 "will degrade 0.5 per shot"]. It fits the published figure the owner quoted
//!   2026-10-11, "about 395 standard rounds" for the 10mm Pistol from full condition: its
//!   health is 80 [observed], and 80 ÷ 0.2 = 400. Every actor's weapon wears so, the
//!   player's and an NPC's [hypothesis: the documentation names no actor; the original's
//!   NPC weapons wear, combat-rules.md §17].
//! - **Armour** loses `fDamageToArmorPercentage` (0.35 [observed]) of a hit's damage
//!   before armour as health points [hypothesis: the setting's role, from its name]. A
//!   hit on the head wears what is worn on the head, any other hit what is worn on the
//!   upper body, a hit with no place (an explosion) both; pieces struck together share
//!   the loss evenly [design: the owner's rule, 2026-10-11].
//! - **A weapon that is struck** loses the hit's damage as health points [design: the
//!   owner's rule].
//! - **At zero** a weapon is broken (`fnv_sim::World::equip` refuses it). Armour below
//!   half condition protects less ([`armor_mult`]) and a weapon below 75 % deals less
//!   (`damage::condition_mult`) [documented: GECK wiki SetArmorConditionPenalty and
//!   SetConditionDamagePenalty].

use fnv_sim::inventory::{Extra, Inventory};
use fnv_sim::FormId;
use serde::Serialize;

use crate::damage::apply_effects;
use crate::data::{AmmoEffect, BodyPart, CombatForms, Limb, WeaponStats};
use crate::tuning::{CombatTuning, Rules};

/// Health points the weapon loses for one attack (see the module notes).
pub fn weapon_wear(r: &Rules, w: &WeaponStats, ammo_effects: &[&AmmoEffect]) -> f32 {
    let per_shot = w.damage_to_weapon_mult.unwrap_or(r.damage_to_weapon_value);
    apply_effects(per_shot, ammo_effects, "Weapon Condition Mod").max(0.0)
}

/// Health points the armour struck loses, together, for a hit of `damage` before armour.
pub fn armor_wear(r: &Rules, damage: f32) -> f32 {
    (damage * r.damage_to_armor_percentage).max(0.0)
}

/// Armour condition at and above which a piece gives all its DT and DR [documented:
/// GECK wiki SetArmorConditionPenalty (JIP), "the 50% threshold"].
pub const ARMOR_CONDITION_THRESHOLD: f32 = 0.5;
/// How fast DT and DR fall below [`ARMOR_CONDITION_THRESHOLD`]: the engine's hard-coded
/// multiplier [documented: GECK wiki SetArmorConditionPenalty, "1.0, by default"].
pub const ARMOR_CONDITION_PENALTY: f32 = 1.0;

/// The share of its DT and DR a piece of armour gives at `condition` (0..=1): all of it
/// at or above half condition, below it `1 − (0.5 − condition) × 1.0`, half at zero
/// [documented: GECK wiki SetArmorConditionPenalty, the formula the JIP function
/// overrides].
pub fn armor_mult(condition: f32) -> f32 {
    armor_mult_with(condition, ARMOR_CONDITION_PENALTY)
}

/// [`armor_mult`] with another `penalty` in place of the hard-coded 1.0 (NVForever's
/// Custom Armor Condition Penalty tweak, docs/design/tweaks.md), never below none.
pub fn armor_mult_with(condition: f32, penalty: f32) -> f32 {
    (1.0 - (ARMOR_CONDITION_THRESHOLD - condition.clamp(0.0, 1.0)).max(0.0) * penalty.max(0.0)).max(0.0)
}

/// The chance one shot jams a weapon at `condition` (the owner's playtest list 5 item 9)
/// [design]: none at or above [`CombatTuning::jam_condition`] (75 %, where the HUD's
/// condition bar marks it), growing to [`CombatTuning::jam_chance_max`] at zero along
/// [`CombatTuning::jam_curve`].
pub fn jam_chance(t: &CombatTuning, condition: f32) -> f32 {
    if t.jam_condition <= 0.0 || condition >= t.jam_condition {
        return 0.0;
    }
    let short = ((t.jam_condition - condition.max(0.0)) / t.jam_condition).clamp(0.0, 1.0);
    t.jam_chance_max * short.powf(t.jam_curve.max(0.01))
}

/// A condition as the whole percentage shown to the player: 0 only when broken, 100 only
/// when undamaged.
pub fn percent(condition: f32) -> u32 {
    match condition {
        c if c <= 0.0 => 0,
        c if c >= 1.0 => 100,
        c => ((c * 100.0).round() as u32).clamp(1, 99),
    }
}

/// Where on the body a hit landed, for the armour it wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Zone {
    Head,
    Body,
    /// No one place (an explosion).
    All,
}

impl Zone {
    /// The zone of a hit on a body part: the head parts are the head, every other part
    /// and an actor without parts the body.
    pub fn of(part: Option<&BodyPart>) -> Zone {
        match part.map(BodyPart::limb) {
            Some(Limb::Head) => Zone::Head,
            _ => Zone::Body,
        }
    }
}

impl CombatForms {
    /// Resolve `id`'s [`ArmorStats`] using `base_id` (allowing `TempCloneForm` aliases)
    /// and an optional runtime [`crate::data::ArmorOverride`].
    pub fn armor_stats_with(&self, base_id: FormId, ov: Option<&crate::data::ArmorOverride>) -> Option<crate::data::ArmorStats> {
        let base = self.armor.get(&base_id)?;
        Some(match ov {
            Some(ov) => ov.apply(base),
            None => base.clone(),
        })
    }

    /// The worn armour a hit in `zone` wears (see the module notes), in `worn` order.
    pub fn armor_struck(&self, worn: &[FormId], zone: Zone) -> Vec<FormId> {
        self.armor_struck_with(worn, zone, |id| self.armor.get(&id).cloned())
    }

    /// [`CombatForms::armor_struck`] using a caller-supplied effective [`ArmorStats`] lookup
    /// (for live `SetEquipmentSlotsMask` and `TempCloneForm` armour).
    pub fn armor_struck_with(&self, worn: &[FormId], zone: Zone, lookup: impl Fn(FormId) -> Option<crate::data::ArmorStats>) -> Vec<FormId> {
        worn.iter()
            .copied()
            .filter(|&id| {
                lookup(id).is_some_and(|a| match zone {
                    Zone::Head => a.covers_head(),
                    Zone::Body => a.covers_body(),
                    Zone::All => a.covers_head() || a.covers_body(),
                })
            })
            .collect()
    }

    /// DT and DR (percent) of worn armour, each piece at its condition
    /// ([`CombatForms::armor_values`] with [`armor_mult`]).
    pub fn armor_values_worn(&self, worn: &[(FormId, f32)]) -> (f32, f32) {
        self.armor_values_worn_with(worn, ARMOR_CONDITION_PENALTY)
    }

    /// [`CombatForms::armor_values_worn`] with [`armor_mult_with`] `penalty`.
    pub fn armor_values_worn_with(&self, worn: &[(FormId, f32)], penalty: f32) -> (f32, f32) {
        self.armor_values_worn_with_lookup(worn, penalty, |id| self.armor.get(&id).cloned())
    }

    /// [`CombatForms::armor_values_worn_with`] using a caller-supplied effective
    /// [`ArmorStats`] lookup (for live `SetArmorDT`, `SetArmorArmorRating`, and
    /// `TempCloneForm` armour).
    pub fn armor_values_worn_with_lookup(
        &self,
        worn: &[(FormId, f32)],
        penalty: f32,
        lookup: impl Fn(FormId) -> Option<crate::data::ArmorStats>,
    ) -> (f32, f32) {
        worn.iter()
            .filter_map(|&(id, c)| Some((lookup(id)?, armor_mult_with(c, penalty))))
            .fold((0.0, 0.0), |(dt, dr), (a, m)| (dt + a.dt * m, dr + a.dr_raw as f32 / 100.0 * m))
    }

    /// DT and DR (percent) of worn armour covering `zone`, for the optional
    /// `localized-dt-dr` setting (#3843): head hits benefit only from worn head-armour
    /// (`covers_head`) and body hits only from worn body-armour (`covers_body`) [documented:
    /// JIP LN NVSE v57.30 / Settings INI v56.24 `jip_nvse.ini` `bLocalizedDTDR`; no DLL or
    /// source consulted]; `Zone::All` sums all worn armour for explosions [observed:
    /// `tools/capture/nvse/probes/localized_dt_dr_probe.json`,
    /// `w4e_c02_jip_localized_dtdr_explosions_innate`, 2026-11-24].
    pub fn armor_values_worn_in_zone_with(&self, worn: &[(FormId, f32)], penalty: f32, zone: Zone) -> (f32, f32) {
        self.armor_values_worn_in_zone_with_lookup(worn, penalty, zone, |id| self.armor.get(&id).cloned())
    }

    /// [`CombatForms::armor_values_worn_in_zone_with`] using a caller-supplied effective
    /// [`ArmorStats`] lookup.
    pub fn armor_values_worn_in_zone_with_lookup(
        &self,
        worn: &[(FormId, f32)],
        penalty: f32,
        zone: Zone,
        lookup: impl Fn(FormId) -> Option<crate::data::ArmorStats>,
    ) -> (f32, f32) {
        worn.iter()
            .filter_map(|&(id, c)| {
                let a = lookup(id)?;
                let matches_zone = match zone {
                    Zone::Head => a.covers_head(),
                    Zone::Body => a.covers_body(),
                    Zone::All => true,
                };
                matches_zone.then(|| (a, armor_mult_with(c, penalty)))
            })
            .fold((0.0, 0.0), |(dt, dr), (a, m)| (dt + a.dt * m, dr + a.dr_raw as f32 / 100.0 * m))
    }

    /// The full health of a weapon or a piece of armour (its record's `DATA` health); 0
    /// for anything else.
    pub fn item_health(&self, item: FormId) -> i32 {
        self.item_health_with(item, |id| self.armor.get(&id).cloned())
    }

    /// [`CombatForms::item_health`] using a caller-supplied effective [`ArmorStats`] lookup
    /// (for live `SetObjectHealth` and `TempCloneForm` armour).
    pub fn item_health_with(&self, item: FormId, armor_lookup: impl Fn(FormId) -> Option<crate::data::ArmorStats>) -> i32 {
        self.weapons.get(&item).map(|w| w.health).or_else(|| armor_lookup(item).map(|a| a.health)).unwrap_or(0)
    }
}

/// What wear did to one item.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Wear {
    pub item: FormId,
    /// Condition before and after, fractions of full health.
    pub before: f32,
    pub after: f32,
    /// It reached zero now.
    pub broke: bool,
}

/// Less than this many health points left counts as none (rounding of many small losses).
const NO_HEALTH: f32 = 0.01;

/// Take `points` health points from the unit of `item` in use in `inv`: the equipped
/// one; when none is equipped, one is marked equipped first (an NPC's weapon in hand and
/// the armour it wears are not marked until they wear) [design]. `None` when the item is
/// not carried or has no health (`max_health`, the record's): such items do not wear.
pub fn wear_item(inv: &mut Inventory, item: FormId, points: f32, max_health: i32) -> Option<Wear> {
    if max_health <= 0 || (!inv.is_equipped(item) && !inv.set_equipped(item)) {
        return None;
    }
    let before = inv.condition_in_use(item)?.clamp(0.0, 1.0);
    let mut after = (before - points.max(0.0) / max_health as f32).max(0.0);
    if after * max_health as f32 <= NO_HEALTH {
        after = 0.0;
    }
    if after != before {
        inv.map_equipped(item, |e| Extra { condition: Some(after), ..e });
    }
    Some(Wear { item, before, after, broke: before > 0.0 && after <= 0.0 })
}

#[cfg(test)]
mod armor_override_tests {
    use super::*;
    use crate::data::{ArmorOverride, ArmorStats};

    #[test]
    fn armor_overrides_reach_hit_zones_dt_dr_health_and_clones() {
        let base_id = FormId(0x100);
        let clone_id = FormId(0xFF00_0001);
        let mut cf = CombatForms::default();
        cf.armor.insert(
            base_id,
            ArmorStats {
                edid: Some("TestArmor".into()),
                name: Some("Test Armor".into()),
                health: 100,
                dt: 6.0,
                dr_raw: 500,
                slots: ArmorStats::UPPER_BODY,
            },
        );
        let clone_ov = ArmorOverride {
            health: Some(250),
            dt: Some(18.0),
            dr_raw: Some(2000),
            slots: Some(ArmorStats::HEAD_SLOTS),
        };
        let lookup = |id: FormId| {
            if id == clone_id {
                cf.armor_stats_with(base_id, Some(&clone_ov))
            } else {
                cf.armor_stats_with(id, None)
            }
        };
        assert_eq!(cf.armor_struck_with(&[base_id, clone_id], Zone::Head, lookup), vec![clone_id]);
        assert_eq!(cf.armor_struck_with(&[base_id, clone_id], Zone::Body, lookup), vec![base_id]);
        let (dt, dr) = cf.armor_values_worn_with_lookup(&[(base_id, 1.0), (clone_id, 1.0)], ARMOR_CONDITION_PENALTY, lookup);
        assert!((dt - 24.0).abs() < 1e-5, "6 + 18 = 24 DT");
        assert!((dr - 25.0).abs() < 1e-5, "5 + 20 = 25 DR");
        assert_eq!(cf.item_health_with(base_id, lookup), 100);
        assert_eq!(cf.item_health_with(clone_id, lookup), 250);
    }

    #[test]
    fn localized_dt_dr_separates_head_and_body_armor() {
        let helmet = FormId(0x201);
        let chest = FormId(0x202);
        let mut cf = CombatForms::default();
        cf.armor.insert(
            helmet,
            ArmorStats { health: 100, dt: 4.0, dr_raw: 500, slots: ArmorStats::HEAD_SLOTS, ..Default::default() },
        );
        cf.armor.insert(
            chest,
            ArmorStats { health: 100, dt: 14.0, dr_raw: 1500, slots: ArmorStats::UPPER_BODY, ..Default::default() },
        );
        let worn = [(helmet, 1.0), (chest, 1.0)];
        let (head_dt, head_dr) = cf.armor_values_worn_in_zone_with(&worn, ARMOR_CONDITION_PENALTY, Zone::Head);
        assert!((head_dt - 4.0).abs() < 1e-5 && (head_dr - 5.0).abs() < 1e-5, "head only: ({head_dt}, {head_dr})");
        let (body_dt, body_dr) = cf.armor_values_worn_in_zone_with(&worn, ARMOR_CONDITION_PENALTY, Zone::Body);
        assert!((body_dt - 14.0).abs() < 1e-5 && (body_dr - 15.0).abs() < 1e-5, "body only: ({body_dt}, {body_dr})");
        let (all_dt, all_dr) = cf.armor_values_worn_in_zone_with(&worn, ARMOR_CONDITION_PENALTY, Zone::All);
        assert!((all_dt - 18.0).abs() < 1e-5 && (all_dr - 20.0).abs() < 1e-5, "all: ({all_dt}, {all_dr})");
        // No helmet worn: head hit gets 0 worn DT/DR, body hit still gets chest DT/DR.
        assert_eq!(cf.armor_values_worn_in_zone_with(&[(chest, 1.0)], ARMOR_CONDITION_PENALTY, Zone::Head), (0.0, 0.0));
        assert_eq!(cf.armor_values_worn_in_zone_with(&[(helmet, 1.0)], ARMOR_CONDITION_PENALTY, Zone::Body), (0.0, 0.0));
    }
}
