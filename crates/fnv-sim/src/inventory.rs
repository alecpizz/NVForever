//! Inventories: item stacks keyed by base form, each split into groups of identical
//! extra data (condition, owner, equipped, weapon mods).
//!
//! The data has per-item extra data in two places [observed, docs/spec-notes/
//! simulation.md §3]: COED after a carried-item entry (owner, condition as a fraction)
//! and XHLP on placed weapons and armour (condition as a fraction). A stack whose items
//! carry no extra data serialises as a bare count, so simple inventories read as
//! `{"0x0000000F": 12}`.
//!
//! Removal order [hypothesis]: items without extra data first, then unequipped items
//! with extra data in the order they arrived, equipped items last.
//!
//! The `_at` methods pick only the units a [`Pick`] names (a condition, mod flags): the
//! menus list a stack as one row per condition and mods, and moving or equipping a row
//! takes its own units [owner, 2026-10-16: six Varmint Rifles, one at 95 %, show as a row
//! of five and a row of one; weapon-mods.md §3 for the mods].
//!
//! A live unit ([`Extra::live`], a planted explosive) is in its own group: counted and
//! weighed with the rest, but no [`Pick`] names it, so nothing takes, equips or moves it
//! but the blast ([`Inventory::take_live`]) [design, #1640; pickpocket.md §5].

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::form::FormId;
use crate::forms::{FormStore, ItemKind};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Extra {
    /// Condition as a fraction of the item's full health.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub condition: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub owner: Option<FormId>,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub equipped: bool,
    /// A weapon's attached mods: 1 slot 1 (WEAP `WMI1`), 2 slot 2, 4 slot 3 [documented:
    /// GECK wiki GetEquippedWeaponModFlags; docs/spec-notes/weapon-mods.md §3].
    #[serde(skip_serializing_if = "is_zero", default)]
    pub mods: u8,
    /// A live explosive the player planted in this holder's pocket (`fnv_game::pickpocket`,
    /// docs/spec-notes/pickpocket.md §5): reserved for its blast, so no [`Pick`] names it
    /// and no equip, use, drop, transfer or removal takes it; only
    /// [`Inventory::take_live`] (the blast) and [`Inventory::set_live_count`] touch it
    /// [design, #1640]. Absent from saves when false, so older saves read as before.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub live: bool,
    /// Saved loaded-magazine round count for an unequipped weapon item (restored once on
    /// equip by `nvf-play` combat `equip_sync_step` when no active `load_magazine` takes
    /// precedence, then cleared so vanilla unequip/re-equip behavior is unchanged) [design].
    /// Absent from saves when `None`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub magazine: Option<u32>,
}

fn is_zero(m: &u8) -> bool {
    *m == 0
}

/// Weapon condition at and above which a weapon deals all its damage
/// [documented: GECK wiki SetConditionDamagePenalty (JIP), "the 75% threshold"].
pub const CONDITION_DAMAGE_THRESHOLD: f32 = 0.75;
/// How fast damage falls below [`CONDITION_DAMAGE_THRESHOLD`]: the engine's hard-coded
/// multiplier [documented: GECK wiki SetConditionDamagePenalty, "0.67, by default"].
pub const CONDITION_DAMAGE_PENALTY: f32 = 0.67;

/// The damage multiplier of a weapon at `condition` (0..=1): 1 at or above 75 %, below it
/// `1 − (0.75 − condition) × 0.67`, 0.4975 at zero [documented: GECK wiki
/// SetConditionDamagePenalty, the formula the JIP function overrides; observed: troopers'
/// Service Rifles at 30, 45 and 50 % dealt 0.698, 0.799 and 0.832 of the full-condition
/// hit, AI observer campaign fight_stats_round 2026-10-19, npc-behaviour-compare.md §9].
pub fn condition_mult(condition: f32) -> f32 {
    1.0 - (CONDITION_DAMAGE_THRESHOLD - condition.clamp(0.0, 1.0)).max(0.0) * CONDITION_DAMAGE_PENALTY
}

impl Extra {
    pub fn is_plain(&self) -> bool {
        *self == Extra::default()
    }

    fn unequipped(self) -> Extra {
        Extra { equipped: false, ..self }
    }

    /// At zero condition: a broken weapon cannot be equipped until it is repaired
    /// ([`crate::World::equip`]).
    pub fn broken(&self) -> bool {
        self.condition.is_some_and(|c| c <= 0.0)
    }

    /// The condition, 1 (whole) when the unit carries none.
    pub fn condition_or_whole(&self) -> f32 {
        self.condition.unwrap_or(1.0)
    }

    /// Whether this unit is one of those `at` picks: any unit (`None`), else the units at
    /// that condition.
    pub fn is_at(&self, at: Option<f32>) -> bool {
        at.is_none_or(|c| self.condition_or_whole() == c)
    }

    /// Whether this unit is one of those `pick` names. A live unit ([`Extra::live`]) is
    /// never picked.
    pub fn picked(&self, pick: Pick) -> bool {
        !self.live && self.is_at(pick.condition) && pick.mods.is_none_or(|m| self.mods == m)
    }
}

/// Which units of an item an action takes: those at one condition (a fraction, units
/// without one counting as whole) and with one set of mod flags, `None` for any. A menu
/// row's action picks the row's units ([`Pick::row`]).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pick {
    pub condition: Option<f32>,
    pub mods: Option<u8>,
}

impl Pick {
    /// Any unit.
    pub const ANY: Pick = Pick { condition: None, mods: None };

    /// The units at `condition` (any when `None`), whatever their mods.
    pub fn at(condition: Option<f32>) -> Pick {
        Pick { condition, mods: None }
    }

    /// A menu row's units: its condition (any when `None`) and exactly its mod flags.
    pub fn row(condition: Option<f32>, mods: u8) -> Pick {
        Pick { condition, mods: Some(mods) }
    }
}

/// One base item's groups, each (extra data, count > 0), in arrival order with the
/// plain group (if any) first.
#[derive(Debug, Clone, Default, PartialEq)]
struct Stack {
    groups: Vec<(Extra, i32)>,
}

impl Stack {
    fn total(&self) -> i32 {
        self.groups.iter().map(|g| g.1).sum()
    }

    fn add(&mut self, extra: Extra, n: i32) {
        match self.groups.iter_mut().find(|g| g.0 == extra) {
            Some(g) => g.1 = g.1.saturating_add(n),
            None if extra.is_plain() => self.groups.insert(0, (extra, n)),
            None => self.groups.push((extra, n)),
        }
    }

    /// Take up to `n` of the units `pick` names, in removal order; returns what was taken.
    fn take(&mut self, mut n: i32, pick: Pick) -> Vec<(Extra, i32)> {
        let mut order: Vec<usize> = (0..self.groups.len()).filter(|&i| self.groups[i].0.picked(pick)).collect();
        order.sort_by_key(|&i| (self.groups[i].0.equipped, !self.groups[i].0.is_plain()));
        let mut out = Vec::new();
        for i in order {
            if n <= 0 {
                break;
            }
            let g = &mut self.groups[i];
            let t = g.1.min(n);
            g.1 -= t;
            n -= t;
            out.push((g.0, t));
        }
        self.groups.retain(|g| g.1 > 0);
        out
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(into = "BTreeMap<FormId, StackRepr>", from = "BTreeMap<FormId, StackRepr>")]
pub struct Inventory {
    items: BTreeMap<FormId, Stack>,
    /// Sorted list of equipped item base FormIDs (one entry per equipped group, in FormID
    /// order), kept in sync on mutations so `equipped_iter()` / `equipped()` /
    /// `is_equipped()` run in O(equipped) instead of scanning every stack in `items` [design].
    equipped_forms: Vec<FormId>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum StackRepr {
    Plain(i32),
    Detailed { count: i32, groups: Vec<GroupRepr> },
}

#[derive(Serialize, Deserialize)]
struct GroupRepr {
    count: i32,
    #[serde(flatten)]
    extra: Extra,
}

impl From<Inventory> for BTreeMap<FormId, StackRepr> {
    fn from(inv: Inventory) -> Self {
        inv.items
            .into_iter()
            .map(|(id, s)| {
                let r = match s.groups.as_slice() {
                    [(e, n)] if e.is_plain() => StackRepr::Plain(*n),
                    g => StackRepr::Detailed {
                        count: s.total(),
                        groups: g.iter().map(|&(extra, count)| GroupRepr { count, extra }).collect(),
                    },
                };
                (id, r)
            })
            .collect()
    }
}

impl From<BTreeMap<FormId, StackRepr>> for Inventory {
    fn from(m: BTreeMap<FormId, StackRepr>) -> Self {
        let mut inv = Inventory::new();
        for (id, r) in m {
            match r {
                StackRepr::Plain(n) if n > 0 => inv.add(id, n),
                StackRepr::Plain(_) => 0,
                StackRepr::Detailed { groups, .. } => {
                    for g in groups.into_iter().filter(|g| g.count > 0) {
                        inv.add_extra(id, g.count, g.extra);
                    }
                    0
                }
            };
        }
        inv
    }
}

/// What a carrier's perks do to the weight of what it carries (the game layer's Modify
/// Light Items and Adjust Heavy Weapon Weight entry points; docs/spec-notes/perk-entry-points.md
/// §7): a unit at most `light.0` weighs × `light.1` (Pack Rat), a weapon unit of at least
/// `heavy_weapons.0` × `heavy_weapons.1` (Heavyweight), each unit's weight with its mods.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct WeightRules {
    pub light: Option<(f32, f32)>,
    pub heavy_weapons: Option<(f32, f32)>,
    /// The player's weightless tweaks (NVForever's settings, `fnv_game::tweaks`) [design: docs/design/tweaks.md]:
    /// item kinds that weigh nothing ([`WEIGHTLESS_ARMOR`] …, bits), and armour or
    /// power armour that weighs nothing while worn. Absent from older saves.
    #[serde(default)]
    pub weightless: u8,
    #[serde(default)]
    pub weightless_worn_armor: bool,
    #[serde(default)]
    pub weightless_worn_power_armor: bool,
    /// The player's Hardcore Tweaks (`fnv_game::tweaks`): ammunition weighs (`Some(true)`)
    /// or does not (`Some(false)`) whatever the Hardcore switch; `None` follows it
    /// [design: docs/design/tweaks.md]. Absent from older saves.
    #[serde(default)]
    pub ammo_weight: Option<bool>,
}

/// [`WeightRules::weightless`]'s bits.
pub const WEIGHTLESS_ARMOR: u8 = 1;
pub const WEIGHTLESS_AID: u8 = 2;
pub const WEIGHTLESS_AMMO: u8 = 4;
pub const WEIGHTLESS_WEAPONS: u8 = 8;
pub const WEIGHTLESS_MISC: u8 = 16;

impl WeightRules {
    pub fn is_none(&self) -> bool {
        self.light.is_none() && self.heavy_weapons.is_none() && self.weightless == 0 && !self.weightless_worn_armor && !self.weightless_worn_power_armor && self.ammo_weight.is_none()
    }

    /// Whether an item of `kind` weighs nothing under the weightless tweaks, `worn` and
    /// `power_armor` saying how it is held.
    pub fn weightless(&self, kind: ItemKind, worn: bool, power_armor: bool) -> bool {
        let bit = match kind {
            ItemKind::Armor => WEIGHTLESS_ARMOR,
            ItemKind::Aid => WEIGHTLESS_AID,
            ItemKind::Ammo => WEIGHTLESS_AMMO,
            ItemKind::Weapon => WEIGHTLESS_WEAPONS,
            ItemKind::Misc => WEIGHTLESS_MISC,
            _ => 0,
        };
        self.weightless & bit != 0 || (kind == ItemKind::Armor && worn && (self.weightless_worn_armor || (power_armor && self.weightless_worn_power_armor)))
    }

    /// One unit's weight `w` (its mods applied) of an item of `kind`.
    pub fn unit(&self, w: f32, kind: ItemKind) -> f32 {
        if let Some((least, mult)) = self.heavy_weapons {
            if kind == ItemKind::Weapon && w >= least {
                return w * mult;
            }
        }
        match self.light {
            Some((most, mult)) if w > 0.0 && w <= most => w * mult,
            _ => w,
        }
    }

    /// Effective weight of one unit after both weightless tweaks and perk rules.
    pub fn effective_unit(&self, w: f32, kind: ItemKind, worn: bool, power_armor: bool) -> f32 {
        if self.weightless(kind, worn, power_armor) {
            0.0
        } else {
            self.unit(w, kind)
        }
    }
}

/// Total weight and value, with the items whose weight or value is not known yet.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Totals {
    pub weight: f64,
    pub value: i64,
    pub unknown_weight: Vec<FormId>,
    pub unknown_value: Vec<FormId>,
}

impl Inventory {
    pub fn new() -> Inventory {
        Inventory::default()
    }

    /// Synchronize `self.equipped_forms` for a single `item` after a mutation that may
    /// have added, removed, merged, or toggled equipped groups on `item` [design].
    fn sync_equipped(&mut self, item: FormId) {
        let want = self
            .items
            .get(&item)
            .map_or(0, |s| s.groups.iter().filter(|g| g.0.equipped).count());
        let start = self.equipped_forms.partition_point(|&f| f < item);
        let mut end = start;
        while end < self.equipped_forms.len() && self.equipped_forms[end] == item {
            end += 1;
        }
        let have = end - start;
        if have == want {
            return;
        }
        if want > have {
            let add = want - have;
            self.equipped_forms.splice(end..end, std::iter::repeat_n(item, add));
        } else {
            self.equipped_forms.drain((start + want)..end);
        }
    }

    /// Add `n` (> 0) of `item` without extra data; returns the new total.
    pub fn add(&mut self, item: FormId, n: i32) -> i32 {
        self.add_extra(item, n, Extra::default())
    }

    /// Add `n` (> 0) of `item` with extra data; returns the new total.
    pub fn add_extra(&mut self, item: FormId, n: i32, extra: Extra) -> i32 {
        debug_assert!(n > 0);
        let s = self.items.entry(item).or_default();
        s.add(extra, n);
        let total = s.total();
        if extra.equipped {
            self.sync_equipped(item);
        }
        total
    }

    /// Remove up to `n` of `item`; returns the groups removed.
    pub fn take(&mut self, item: FormId, n: i32) -> Vec<(Extra, i32)> {
        self.take_at(item, n, Pick::ANY)
    }

    /// Remove up to `n` of the units of `item` that `pick` names ([`Extra::picked`]);
    /// returns the groups removed.
    pub fn take_at(&mut self, item: FormId, n: i32, pick: Pick) -> Vec<(Extra, i32)> {
        let Some(s) = self.items.get_mut(&item) else { return Vec::new() };
        let out = s.take(n.max(0), pick);
        if s.groups.is_empty() {
            self.items.remove(&item);
        }
        if out.iter().any(|g| g.0.equipped) {
            self.sync_equipped(item);
        }
        out
    }

    /// Remove up to `n` of `item`; returns how many were removed.
    pub fn remove(&mut self, item: FormId, n: i32) -> i32 {
        self.take(item, n).iter().map(|g| g.1).sum()
    }

    /// Every unit of `item` carried, live ones ([`Extra::live`]) included.
    pub fn count(&self, item: FormId) -> i32 {
        self.items.get(&item).map_or(0, Stack::total)
    }

    /// The units of `item` an action may take: all but the live ones.
    pub fn free_count(&self, item: FormId) -> i32 {
        self.items.get(&item).map_or(0, |s| s.groups.iter().filter(|g| !g.0.live).map(|g| g.1).sum())
    }

    /// The live units of `item` ([`Extra::live`]).
    pub fn live_count(&self, item: FormId) -> i32 {
        self.items.get(&item).map_or(0, |s| s.groups.iter().filter(|g| g.0.live).map(|g| g.1).sum())
    }

    /// [`Inventory::free_count`] per base item in FormID order, items with none left out:
    /// what an actor may choose to use (a grenade to throw, a weapon to hold).
    pub fn free_iter(&self) -> impl Iterator<Item = (FormId, i32)> + '_ {
        self.items.keys().map(|&k| (k, self.free_count(k))).filter(|&(_, n)| n > 0)
    }

    /// The units of `item` that `pick` names ([`Extra::picked`]: live units never), what an
    /// action on a menu row may move.
    pub fn count_at(&self, item: FormId, pick: Pick) -> i32 {
        self.items.get(&item).map_or(0, |s| s.groups.iter().filter(|g| g.0.picked(pick)).map(|g| g.1).sum())
    }

    /// Reserve one unit of `item` as live (a planted explosive), keeping its other extra
    /// data: an unequipped one, plain units first, then the others in arrival order; when
    /// every unit is equipped, an equipped one, which is taken off (a save from before live
    /// units whose victim had put the planted grenade in its hands, #1640). Returns false
    /// when no unit that is not live already is carried.
    pub fn mark_live(&mut self, item: FormId) -> bool {
        let Some(s) = self.items.get_mut(&item) else { return false };
        let Some(i) = (0..s.groups.len()).filter(|&i| !s.groups[i].0.live).min_by_key(|&i| (s.groups[i].0.equipped, !s.groups[i].0.is_plain(), i)) else {
            return false;
        };
        let e = s.groups[i].0;
        s.groups[i].1 -= 1;
        s.groups.retain(|g| g.1 > 0);
        s.add(Extra { live: true, equipped: false, ..e }, 1);
        if e.equipped {
            self.sync_equipped(item);
        }
        true
    }

    /// Add the live units of `old` (planted explosives, [`Extra::live`]): an inventory
    /// rebuilt in place of `old` (ResetInventory, a respawn, a restock, a new roll) keeps
    /// them, so their plants still find them [design, #1640].
    pub fn keep_live_of(&mut self, old: &Inventory) {
        for (item, e, n) in old.groups().filter(|g| g.1.live) {
            self.add_extra(item, n, e);
        }
    }

    /// Whether any unit is live ([`Extra::live`]).
    pub fn has_live(&self) -> bool {
        self.groups().any(|g| g.1.live)
    }

    /// Remove one live unit of `item` (its blast). Returns false when none is live.
    pub fn take_live(&mut self, item: FormId) -> bool {
        let Some(s) = self.items.get_mut(&item) else { return false };
        let Some(g) = s.groups.iter_mut().find(|g| g.0.live) else { return false };
        let was_equipped = g.0.equipped;
        g.1 -= 1;
        s.groups.retain(|g| g.1 > 0);
        if s.groups.is_empty() {
            self.items.remove(&item);
        }
        if was_equipped {
            self.sync_equipped(item);
        }
        true
    }

    /// Make `n` units of `item` live, as far as the carried units allow: more are
    /// reserved ([`Inventory::mark_live`]) or live ones turned back into ordinary units
    /// (merging with their like). Returns how many are live after.
    pub fn set_live_count(&mut self, item: FormId, n: i32) -> i32 {
        let n = n.max(0);
        while self.live_count(item) < n && self.mark_live(item) {}
        while self.live_count(item) > n {
            let Some(s) = self.items.get_mut(&item) else { break };
            let Some(g) = s.groups.iter_mut().find(|g| g.0.live) else { break };
            let e = g.0;
            g.1 -= 1;
            s.groups.retain(|g| g.1 > 0);
            s.add(Extra { live: false, ..e }, 1);
            if e.equipped {
                self.sync_equipped(item);
            }
        }
        self.live_count(item)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Totals per base item in FormID order.
    pub fn iter(&self) -> impl Iterator<Item = (FormId, i32)> + '_ {
        self.items.iter().map(|(&k, s)| (k, s.total()))
    }

    /// Every group: (item, extra data, count), in FormID then group order.
    pub fn groups(&self) -> impl Iterator<Item = (FormId, Extra, i32)> + '_ {
        self.items.iter().flat_map(|(&k, s)| s.groups.iter().map(move |&(e, n)| (k, e, n)))
    }

    /// Zero-allocation iterator over equipped item base FormIDs (in FormID then group order).
    #[inline]
    pub fn equipped_iter(&self) -> impl Iterator<Item = FormId> + '_ {
        self.equipped_forms.iter().copied()
    }

    #[inline]
    pub fn equipped(&self) -> Vec<FormId> {
        self.equipped_forms.clone()
    }

    #[inline]
    pub fn is_equipped(&self, item: FormId) -> bool {
        if self.equipped_forms.len() <= 16 {
            self.equipped_forms.contains(&item)
        } else {
            self.equipped_forms.binary_search(&item).is_ok()
        }
    }

    /// The unequipped unit [`Inventory::set_equipped_at`] takes among those `pick` names:
    /// plain units first, broken ones last.
    fn next_to_equip(s: &Stack, pick: Pick) -> Option<usize> {
        (0..s.groups.len())
            .filter(|&i| !s.groups[i].0.equipped && s.groups[i].0.picked(pick))
            .min_by_key(|&i| (s.groups[i].0.broken(), !s.groups[i].0.is_plain()))
    }

    /// Whether an unequipped unit of `item` that is not broken is carried.
    pub fn has_sound(&self, item: FormId) -> bool {
        self.has_sound_at(item, Pick::ANY)
    }

    /// Whether [`Inventory::set_equipped_at`] would find a unit of `item` that `pick`
    /// names, once the equipped units of `item` are taken off first when `freed` (an
    /// item that clashes with itself, as [`crate::World::equip_at`] unequips it).
    pub fn can_equip_at(&self, item: FormId, pick: Pick, freed: bool) -> bool {
        self.items.get(&item).is_some_and(|s| s.groups.iter().any(|g| (freed || !g.0.equipped) && g.0.picked(pick)))
    }

    /// The same among the units `pick` names.
    pub fn has_sound_at(&self, item: FormId, pick: Pick) -> bool {
        self.items.get(&item).is_some_and(|s| s.groups.iter().any(|g| !g.0.equipped && !g.0.broken() && g.0.picked(pick)))
    }

    /// The condition (a fraction, 1 when undamaged) of the unit of `item` in use: the
    /// equipped one, else the one [`Inventory::set_equipped`] would take. `None` when
    /// none is carried.
    pub fn condition_in_use(&self, item: FormId) -> Option<f32> {
        let s = self.items.get(&item)?;
        let i = s.groups.iter().position(|g| g.0.equipped).or_else(|| Self::next_to_equip(s, Pick::ANY))?;
        Some(s.groups[i].0.condition.unwrap_or(1.0))
    }

    /// The mod flags of the unit of `item` in use, chosen as [`Inventory::condition_in_use`]
    /// chooses; 0 when none is carried.
    pub fn mods_in_use(&self, item: FormId) -> u8 {
        let Some(s) = self.items.get(&item) else { return 0 };
        s.groups.iter().position(|g| g.0.equipped).or_else(|| Self::next_to_equip(s, Pick::ANY)).map_or(0, |i| s.groups[i].0.mods)
    }

    /// Mark one unequipped unit of `item` as equipped (plain units first, broken ones
    /// last). Returns false when there is none.
    pub fn set_equipped(&mut self, item: FormId) -> bool {
        self.set_equipped_at(item, Pick::ANY)
    }

    /// The same among the units `pick` names.
    pub fn set_equipped_at(&mut self, item: FormId, pick: Pick) -> bool {
        let Some(s) = self.items.get_mut(&item) else { return false };
        let Some(i) = Self::next_to_equip(s, pick) else {
            return false;
        };
        let e = s.groups[i].0;
        s.groups[i].1 -= 1;
        s.groups.retain(|g| g.1 > 0);
        s.add(Extra { equipped: true, ..e }, 1);
        self.sync_equipped(item);
        true
    }

    /// Rewrite the extra data of the equipped units of `item` (e.g. their condition).
    /// Returns false when none is equipped.
    pub fn map_equipped(&mut self, item: FormId, f: impl Fn(Extra) -> Extra) -> bool {
        let Some(s) = self.items.get_mut(&item) else { return false };
        let eq: Vec<(Extra, i32)> = s.groups.iter().copied().filter(|g| g.0.equipped).collect();
        if eq.is_empty() {
            return false;
        }
        s.groups.retain(|g| !g.0.equipped);
        for (e, n) in eq {
            s.add(Extra { equipped: true, ..f(e) }, n);
        }
        self.sync_equipped(item);
        true
    }

    /// Give one unit of the group `from` of `item` another condition (a fraction), keeping
    /// its owner and whether it is worn: a repair. A condition of 1 or more is whole and
    /// kept as none. Returns false when no unit of `item` has that extra data.
    pub fn set_condition_one(&mut self, item: FormId, from: Extra, condition: f32) -> bool {
        let Some(s) = self.items.get_mut(&item) else { return false };
        let Some(g) = s.groups.iter_mut().find(|g| g.0 == from && g.1 > 0) else { return false };
        g.1 -= 1;
        s.groups.retain(|g| g.1 > 0);
        s.add(Extra { condition: (condition < 1.0).then_some(condition.max(0.0)), ..from }, 1);
        if from.equipped {
            self.sync_equipped(item);
        }
        true
    }

    /// Give one unit of the group `from` of `item` the mod flags `mods` (weapon-mods.md
    /// §3), keeping the rest of its extra data. Returns false when no unit of `item` has
    /// that extra data.
    pub fn set_mods_one(&mut self, item: FormId, from: Extra, mods: u8) -> bool {
        let Some(s) = self.items.get_mut(&item) else { return false };
        let Some(g) = s.groups.iter_mut().find(|g| g.0 == from && g.1 > 0) else { return false };
        g.1 -= 1;
        s.groups.retain(|g| g.1 > 0);
        s.add(Extra { mods, ..from }, 1);
        if from.equipped {
            self.sync_equipped(item);
        }
        true
    }

    /// The mod flags of the equipped unit of `item` (0 when none is equipped).
    pub fn equipped_mods(&self, item: FormId) -> u8 {
        self.items.get(&item).and_then(|s| s.groups.iter().find(|g| g.0.equipped)).map_or(0, |g| g.0.mods)
    }

    /// The saved loaded-magazine round count on the equipped unit of `item`, if any.
    pub fn equipped_magazine(&self, item: FormId) -> Option<u32> {
        self.items.get(&item).and_then(|s| s.groups.iter().find(|g| g.0.equipped)).and_then(|g| g.0.magazine)
    }

    /// Unequip every equipped unit of `item`. Returns false when none was equipped.
    pub fn clear_equipped(&mut self, item: FormId) -> bool {
        let Some(s) = self.items.get_mut(&item) else { return false };
        let eq: Vec<(Extra, i32)> = s.groups.iter().copied().filter(|g| g.0.equipped).collect();
        if eq.is_empty() {
            return false;
        }
        s.groups.retain(|g| !g.0.equipped);
        for (e, n) in eq {
            s.add(e.unequipped(), n);
        }
        self.sync_equipped(item);
        true
    }

    /// Remove the equipped units of `item`; returns the groups removed (still marked
    /// equipped).
    pub fn take_equipped(&mut self, item: FormId) -> Vec<(Extra, i32)> {
        let Some(s) = self.items.get_mut(&item) else { return Vec::new() };
        let out: Vec<(Extra, i32)> = s.groups.iter().copied().filter(|g| g.0.equipped).collect();
        s.groups.retain(|g| !g.0.equipped);
        if s.groups.is_empty() {
            self.items.remove(&item);
        }
        if !out.is_empty() {
            self.sync_equipped(item);
        }
        out
    }

    /// Weight and value. Ammunition weighs nothing outside Hardcore mode [observed in
    /// game: 100 9mm rounds left the carried weight unchanged, non-Hardcore —
    /// `L4.sim.trace.goodsprings_weight_and_time`; the Hardcore side is still hypothesis].
    /// A modded weapon weighs what its Decrease Weight mods leave (`Item::weight_with_mods`,
    /// weapon-mods.md §3).
    pub fn totals(&self, forms: &FormStore, hardcore: bool) -> Totals {
        self.totals_with(forms, hardcore, &WeightRules::default())
    }

    /// [`Inventory::totals`] with a carrier's [`WeightRules`].
    pub fn totals_with(&self, forms: &FormStore, hardcore: bool, rules: &WeightRules) -> Totals {
        self.totals_with_overrides(forms, hardcore, rules, |id| forms.item(id).map(|i| (i, None, i.power_armor)))
    }

    /// [`Inventory::totals_with`] using a caller-supplied item resolver for runtime
    /// `TempCloneForm` items and live `weight`/`power_armor` overrides.
    pub fn totals_with_overrides<'a, F>(&self, _forms: &'a FormStore, hardcore: bool, rules: &WeightRules, lookup: F) -> Totals
    where
        F: Fn(FormId) -> Option<(&'a crate::forms::Item, Option<f32>, bool)>,
    {
        let hardcore = rules.ammo_weight.unwrap_or(hardcore);
        let mut t = Totals::default();
        for (id, s) in &self.items {
            let (id, n) = (*id, s.total());
            match lookup(id) {
                Some((i, weight_ov, power_armor)) => {
                    let eff_weight = weight_ov.or(i.weight);
                    match eff_weight {
                        _ if i.kind == ItemKind::Ammo && !hardcore => {}
                        // The records have no weight field; in game they weigh nothing,
                        // like caps [observed by the owner, inventory screen, 2026-10-08].
                        None if matches!(
                            i.kind,
                            ItemKind::Note | ItemKind::CasinoChip | ItemKind::CaravanMoney | ItemKind::CaravanCard
                        ) => {}
                        Some(base_w) => {
                            let mod_delta = |mods: u8| {
                                let orig = i.weight.unwrap_or(base_w);
                                orig - i.weight_with_mods(mods).unwrap_or(orig)
                            };
                            for &(e, c) in &s.groups {
                                let unit = (base_w - mod_delta(e.mods)).max(0.0);
                                t.weight += rules.effective_unit(unit, i.kind, e.equipped, power_armor) as f64 * c as f64;
                            }
                        }
                        None => t.unknown_weight.push(id),
                    }
                    match i.worth() {
                        Some(v) => t.value += v as i64 * n as i64,
                        None => t.unknown_value.push(id),
                    }
                }
                None => {
                    t.unknown_weight.push(id);
                    t.unknown_value.push(id);
                }
            }
        }
        t
    }
}

/// Strip the equipped flag from removed groups (items leaving an inventory).
pub fn unequipped(groups: Vec<(Extra, i32)>) -> Vec<(Extra, i32)> {
    groups.into_iter().map(|(e, n)| (e.unequipped(), n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn perk_weight_rules_halve_light_items_and_heavy_weapons() {
        let rules = WeightRules { light: Some((2.0, 0.5)), heavy_weapons: Some((10.0, 0.5)), ..Default::default() };
        // Pack Rat: 2 or less halves, more stays; nothing weighs nothing.
        assert_eq!(rules.unit(2.0, ItemKind::Misc), 1.0);
        assert_eq!(rules.unit(0.5, ItemKind::Aid), 0.25);
        assert_eq!(rules.unit(2.5, ItemKind::Misc), 2.5);
        assert_eq!(rules.unit(0.0, ItemKind::Misc), 0.0);
        // Heavyweight: weapons of 10 or more halve; armour of 15 does not.
        assert_eq!(rules.unit(10.0, ItemKind::Weapon), 5.0);
        assert_eq!(rules.unit(9.9, ItemKind::Weapon), 9.9);
        assert_eq!(rules.unit(15.0, ItemKind::Armor), 15.0);
        // A light weapon under Pack Rat alone.
        assert_eq!(rules.unit(1.0, ItemKind::Weapon), 0.5);
        assert!(WeightRules::default().is_none());
        assert_eq!(WeightRules::default().unit(3.0, ItemKind::Weapon), 3.0);
    }

    #[test]
    fn the_hardcore_tweaks_rule_counts_as_a_rule() {
        assert!(!WeightRules { ammo_weight: Some(false), ..Default::default() }.is_none());
        assert!(WeightRules { ammo_weight: None, ..Default::default() }.is_none());
    }

    #[test]
    fn weightless_tweaks_pick_kinds_and_worn_armor() {
        let rules = WeightRules { weightless: WEIGHTLESS_AID | WEIGHTLESS_MISC, ..Default::default() };
        assert!(!rules.is_none());
        assert!(rules.weightless(ItemKind::Aid, false, false));
        assert!(rules.weightless(ItemKind::Misc, false, false));
        assert!(!rules.weightless(ItemKind::Weapon, false, false));
        let worn = WeightRules { weightless_worn_power_armor: true, ..Default::default() };
        assert!(worn.weightless(ItemKind::Armor, true, true));
        assert!(!worn.weightless(ItemKind::Armor, false, true), "carried, not worn");
        assert!(!worn.weightless(ItemKind::Armor, true, false), "not power armour");
        let any = WeightRules { weightless_worn_armor: true, ..Default::default() };
        assert!(any.weightless(ItemKind::Armor, true, false));
        assert!(!WeightRules::default().weightless(ItemKind::Armor, true, true));
        let combined = WeightRules { light: Some((2.0, 0.5)), weightless_worn_power_armor: true, ..Default::default() };
        assert_eq!(combined.effective_unit(20.0, ItemKind::Armor, true, true), 0.0);
        assert_eq!(combined.effective_unit(20.0, ItemKind::Armor, false, true), 20.0);
        assert_eq!(combined.effective_unit(2.0, ItemKind::Armor, true, false), 1.0);
    }

    #[test]
    fn broken_units_are_equipped_last() {
        let mut inv = Inventory::new();
        let id = FormId(0x10);
        inv.add_extra(id, 1, Extra { condition: Some(0.0), ..Extra::default() });
        assert!(!inv.has_sound(id));
        assert_eq!(inv.condition_in_use(id), Some(0.0));
        inv.add_extra(id, 1, Extra { condition: Some(0.4), ..Extra::default() });
        assert!(inv.has_sound(id));
        assert_eq!(inv.condition_in_use(id), Some(0.4));
        assert!(inv.set_equipped(id));
        assert_eq!(inv.condition_in_use(id), Some(0.4));
        assert!(!inv.has_sound(id));
        assert_eq!(inv.condition_in_use(FormId(0x11)), None);
    }

    #[test]
    fn a_repair_rewrites_one_unit() {
        let mut inv = Inventory::new();
        let id = FormId(0x10);
        let worn = Extra { condition: Some(0.4), ..Extra::default() };
        inv.add_extra(id, 2, worn);
        assert!(inv.set_condition_one(id, worn, 0.9));
        let groups: Vec<(Extra, i32)> = inv.groups().map(|g| (g.1, g.2)).collect();
        assert_eq!(groups, vec![(worn, 1), (Extra { condition: Some(0.9), ..Extra::default() }, 1)]);
        // To full: a plain unit; the equipped flag stays.
        let on = Extra { equipped: true, ..worn };
        inv.add_extra(id, 1, on);
        assert!(inv.set_condition_one(id, on, 1.0));
        assert!(inv.groups().any(|g| g.1 == Extra { equipped: true, ..Extra::default() }));
        assert!(!inv.set_condition_one(id, on, 1.0));
        assert!(!inv.set_condition_one(FormId(0x11), worn, 1.0));
        assert_eq!(inv.count(id), 3);
    }

    #[test]
    fn a_mod_marks_one_unit_and_is_saved() {
        let mut inv = Inventory::new();
        let id = FormId(0x10);
        inv.add(id, 2);
        assert!(inv.set_equipped(id));
        let on = Extra { equipped: true, ..Extra::default() };
        assert!(inv.set_mods_one(id, on, 0b101));
        assert_eq!(inv.equipped_mods(id), 0b101);
        // The other unit is untouched: two groups.
        let groups: Vec<(Extra, i32)> = inv.groups().map(|g| (g.1, g.2)).collect();
        assert_eq!(groups, vec![(Extra::default(), 1), (Extra { mods: 0b101, ..on }, 1)]);
        assert!(!inv.set_mods_one(FormId(0x11), on, 1));
        // Kept through the serde form, and absent when 0.
        let json = serde_json::to_string(&inv).unwrap();
        assert!(json.contains("\"mods\":5"), "{json}");
        let back: Inventory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, inv);
        assert!(!serde_json::to_string(&Extra::default()).unwrap().contains("mods"));
        assert!(!serde_json::to_string(&Extra::default()).unwrap().contains("magazine"));

        // Per-item saved magazine round count round-trips and is read via equipped_magazine.
        let mut minv = Inventory::new();
        minv.add_extra(id, 1, Extra { magazine: Some(7), ..Extra::default() });
        assert_eq!(minv.equipped_magazine(id), None);
        assert!(minv.set_equipped(id));
        assert_eq!(minv.equipped_magazine(id), Some(7));
        let mjson = serde_json::to_string(&minv).unwrap();
        assert!(mjson.contains("\"magazine\":7"), "{mjson}");
        let mback: Inventory = serde_json::from_str(&mjson).unwrap();
        assert_eq!(mback, minv);
    }

    #[test]
    fn a_row_picks_units_by_mods_too() {
        let id = FormId(0x10);
        let scoped = Extra { mods: 2, ..Extra::default() };
        let mut inv = Inventory::new();
        inv.add(id, 1);
        inv.add_extra(id, 1, scoped);
        // Both whole: the condition alone would equip the plain unit first.
        assert!(inv.set_equipped_at(id, Pick::row(Some(1.0), 2)));
        assert_eq!(inv.equipped_mods(id), 2);
        assert_eq!(inv.mods_in_use(id), 2);
        assert!(!inv.has_sound_at(id, Pick::row(Some(1.0), 2)));
        assert!(inv.has_sound_at(id, Pick::row(Some(1.0), 0)));
        assert_eq!(inv.take_at(id, 1, Pick::row(None, 4)), vec![]);
        assert_eq!(inv.take_at(id, 5, Pick::row(None, 2)), vec![(Extra { equipped: true, ..scoped }, 1)]);
        assert_eq!(inv.mods_in_use(id), 0);
        assert_eq!(inv.take_at(id, 5, Pick::at(Some(1.0))), vec![(Extra::default(), 1)]);
        assert_eq!(inv.mods_in_use(id), 0);
    }

    #[test]
    fn decrease_weight_mods_lighten_the_carried_weight() {
        use crate::forms::{BaseData, BaseForm, Item};
        let id = FormId(0x10);
        let it = Item {
            kind: ItemKind::Weapon,
            value: Some(10),
            money: None,
            weight: Some(6.0),
            slots: None,
            projectile: None,
            health: None,
            non_playable: false,
            cant_drop: false,
            armor_class: Default::default(),
            power_armor: false,
            armor_dr: None,
            armor_dt: None,
            weapon_damage: None,
            weight_mods: [0.0, 2.5, 9.0],
            drawable: false, not_for_npcs: false, thrown: false,
            quest_item: false,
            icon: None,
            placed_count: None,
            teaches: None,
            cant_be_taken: false,
            modulates_voice: false,
            weapon_traits: None,
        };
        assert_eq!([0, 1, 2, 4, 6].map(|m| it.weight_with_mods(m)), [Some(6.0), Some(6.0), Some(3.5), Some(0.0), Some(0.0)]);
        let mut forms = FormStore::default();
        forms.bases.insert(id, BaseForm { id, sig: *b"WEAP", edid: None, name: None, script: None, prompt: None, voice_filter: false, data: BaseData::Item(it) });
        let mut inv = Inventory::new();
        inv.add(id, 2);
        inv.add_extra(id, 1, Extra { mods: 2, ..Extra::default() });
        inv.add_extra(id, 1, Extra { mods: 4, condition: Some(0.5), ..Extra::default() });
        let t = inv.totals(&forms, false);
        assert_eq!((t.weight, t.value), (2.0 * 6.0 + 3.5, 40));
    }

    #[test]
    fn units_at_one_condition() {
        let id = FormId(0x10);
        let worn = Extra { condition: Some(0.95), ..Extra::default() };
        let mut inv = Inventory::new();
        inv.add(id, 5);
        inv.add_extra(id, 1, worn);
        inv.add_extra(id, 1, Extra { condition: Some(1.0), ..Extra::default() });
        // Units without a condition count as whole.
        assert_eq!(inv.take_at(id, 9, Pick::at(Some(1.0))).iter().map(|g| g.1).sum::<i32>(), 6);
        inv.add(id, 2);
        assert!(inv.set_equipped_at(id, Pick::at(Some(0.95))));
        assert_eq!(inv.condition_in_use(id), Some(0.95));
        assert!(!inv.has_sound_at(id, Pick::at(Some(0.95))));
        assert_eq!(inv.take_at(id, 1, Pick::at(Some(0.5))), vec![]);
        assert_eq!(inv.take_at(id, 3, Pick::at(Some(0.95))), vec![(Extra { equipped: true, ..worn }, 1)]);
        assert_eq!(inv.count(id), 2);
    }

    #[test]
    fn add_remove_count() {
        let mut inv = Inventory::new();
        let a = FormId(0x10);
        assert_eq!(inv.add(a, 3), 3);
        assert_eq!(inv.add(a, 2), 5);
        assert_eq!(inv.remove(a, 4), 4);
        assert_eq!(inv.count(a), 1);
        assert_eq!(inv.remove(a, 9), 1);
        assert!(inv.is_empty());
        assert_eq!(inv.remove(a, 1), 0);
        assert_eq!(inv.remove(FormId(0x11), -3), 0);
        let j = serde_json::to_string(&{
            let mut i = Inventory::new();
            i.add(FormId(0x2), 1);
            i.add(FormId(0x1), 7);
            i
        })
        .unwrap();
        assert_eq!(j, r#"{"0x00000001":7,"0x00000002":1}"#);
        let back: Inventory = serde_json::from_str(&j).unwrap();
        assert_eq!(back.count(FormId(0x1)), 7);
    }

    #[test]
    fn extra_data_groups() {
        let (gun, owner) = (FormId(0x20), FormId(0x99));
        let worn = Extra { condition: Some(0.5), ..Extra::default() };
        let mut inv = Inventory::new();
        inv.add_extra(gun, 1, worn);
        inv.add(gun, 2);
        inv.add_extra(gun, 1, Extra { owner: Some(owner), ..worn });
        assert_eq!(inv.count(gun), 4);
        // Plain units are equipped first.
        assert!(inv.set_equipped(gun));
        assert!(inv.is_equipped(gun));
        let j = serde_json::to_value(&inv).unwrap();
        assert_eq!(j["0x00000020"]["count"], 4);
        assert_eq!(serde_json::from_value::<Inventory>(j.clone()).unwrap(), inv);
        // Removal: plain first, then unequipped extras in arrival order, equipped last.
        let taken = inv.take(gun, 3);
        assert_eq!(taken, vec![(Extra::default(), 1), (worn, 1), (Extra { owner: Some(owner), ..worn }, 1)]);
        assert_eq!(inv.equipped(), vec![gun]);
        let last = unequipped(inv.take(gun, 1));
        assert_eq!(last, vec![(Extra::default(), 1)]);
        assert!(inv.is_empty());
        // Unequip merges back.
        inv.add(gun, 2);
        inv.set_equipped(gun);
        assert!(inv.clear_equipped(gun));
        assert!(!inv.clear_equipped(gun));
        assert_eq!(serde_json::to_string(&inv).unwrap(), r#"{"0x00000020":2}"#);
        assert!(!Inventory::new().set_equipped(gun));
    }

    #[test]
    fn a_live_unit_is_reserved_from_every_pick() {
        let id = FormId(0x10);
        let mut inv = Inventory::new();
        inv.add(id, 2);
        assert!(inv.mark_live(id));
        assert_eq!((inv.count(id), inv.free_count(id), inv.live_count(id)), (2, 1, 1));
        assert_eq!(inv.free_iter().collect::<Vec<_>>(), vec![(id, 1)]);
        // Only the ordinary unit is taken, equipped or moved.
        assert!(inv.set_equipped(id));
        assert!(!inv.can_equip_at(id, Pick::ANY, false));
        assert_eq!(inv.take(id, 5), vec![(Extra { equipped: true, ..Extra::default() }, 1)]);
        assert_eq!(inv.take(id, 5), vec![]);
        assert!(!inv.set_equipped(id) && !inv.has_sound(id));
        assert_eq!(inv.count(id), 1);
        assert_eq!(inv.free_iter().count(), 0);
        // Kept through the serde form; absent when false.
        let json = serde_json::to_string(&inv).unwrap();
        assert!(json.contains("\"live\":true"), "{json}");
        assert_eq!(serde_json::from_str::<Inventory>(&json).unwrap(), inv);
        assert!(!serde_json::to_string(&Extra::default()).unwrap().contains("live"));
        // The blast takes it once.
        assert!(inv.take_live(id));
        assert!(!inv.take_live(id));
        assert!(inv.is_empty());
    }

    #[test]
    fn the_live_count_is_set_from_the_plants() {
        let id = FormId(0x10);
        let worn = Extra { condition: Some(0.5), ..Extra::default() };
        let mut inv = Inventory::new();
        inv.add_extra(id, 1, worn);
        inv.add(id, 1);
        // The plain unit is reserved first, the worn one keeps its condition when it is.
        assert!(inv.mark_live(id));
        assert!(inv.groups().any(|g| g.1 == Extra { live: true, ..Extra::default() }));
        assert_eq!(inv.set_live_count(id, 5), 2, "only what is carried");
        assert!(inv.groups().any(|g| g.1 == Extra { live: true, ..worn }));
        assert_eq!(inv.set_live_count(id, 0), 0);
        assert_eq!((inv.count(id), inv.free_count(id)), (2, 2));
        // Back to ordinary units, merged with their like.
        assert_eq!(serde_json::to_value(&inv).unwrap()["0x00000010"]["groups"].as_array().unwrap().len(), 2);
        assert!(!inv.mark_live(FormId(0x11)));
        assert_eq!(inv.set_live_count(FormId(0x11), 1), 0);
    }

    #[test]
    fn an_equipped_unit_is_reserved_last_and_taken_off() {
        let id = FormId(0x10);
        let mut inv = Inventory::new();
        inv.add(id, 2);
        assert!(inv.set_equipped(id));
        // The unequipped unit first.
        assert!(inv.mark_live(id));
        assert!(inv.is_equipped(id));
        // Then the equipped one, taken off.
        assert!(inv.mark_live(id));
        assert!(!inv.is_equipped(id));
        assert_eq!((inv.live_count(id), inv.free_count(id)), (2, 0));
        assert!(!inv.mark_live(id));
        // A rebuilt inventory keeps them.
        let mut fresh = Inventory::new();
        fresh.add(id, 1);
        fresh.keep_live_of(&inv);
        assert!(fresh.has_live() && !Inventory::new().has_live());
        assert_eq!((fresh.count(id), fresh.live_count(id)), (3, 2));
        assert_eq!(fresh.count_at(id, Pick::ANY), 1);
    }

    #[test]
    fn equipped_forms_cache_matches_groups_across_all_mutations_and_binary_search_threshold() {
        let check = |inv: &Inventory| {
            let expected: Vec<FormId> = inv.groups().filter(|g| g.1.equipped).map(|g| g.0).collect();
            assert_eq!(inv.equipped(), expected);
            assert_eq!(inv.equipped_iter().collect::<Vec<_>>(), expected);
            for raw in 1u32..=24 {
                let id = FormId(raw);
                assert_eq!(inv.is_equipped(id), expected.contains(&id), "item {id}");
            }
        };

        let mut inv = Inventory::new();
        // >16 equipped items exercises the binary_search branch in `is_equipped`.
        for raw in (1u32..=20).rev() {
            inv.add_extra(FormId(raw), 1, Extra { equipped: true, ..Extra::default() });
            check(&inv);
        }
        // Multiple equipped groups on the same FormId (different conditions), then merged via map_equipped.
        let multi = FormId(5);
        let worn_damaged = Extra { equipped: true, condition: Some(0.5), ..Extra::default() };
        inv.add_extra(multi, 1, worn_damaged);
        check(&inv);
        assert!(inv.set_condition_one(multi, worn_damaged, 0.75));
        check(&inv);
        let worn_75 = Extra { equipped: true, condition: Some(0.75), ..Extra::default() };
        assert!(inv.set_mods_one(multi, worn_75, 0b01));
        check(&inv);
        assert!(inv.map_equipped(multi, |_| Extra::default()));
        check(&inv);
        // mark_live strips equipped when only equipped units remain.
        let live_id = FormId(2);
        assert!(inv.mark_live(live_id));
        check(&inv);
        // clear_equipped, take_equipped, take_at, and serde round-trip.
        assert!(inv.clear_equipped(FormId(3)));
        check(&inv);
        assert!(!inv.take_equipped(FormId(4)).is_empty());
        check(&inv);
        assert!(!inv.take(FormId(6), 1).is_empty());
        check(&inv);
        let round: Inventory = serde_json::from_value(serde_json::to_value(&inv).unwrap()).unwrap();
        assert_eq!(round, inv);
        check(&round);
    }

    #[test]
    fn set_live_count_decrease_merges_equipped_groups_without_stale_cache_duplicates() {
        let id = FormId(0x30);
        let mut inv = Inventory::new();
        inv.add_extra(id, 1, Extra { equipped: true, live: false, ..Extra::default() });
        inv.add_extra(id, 1, Extra { equipped: true, live: true, ..Extra::default() });
        assert_eq!(inv.equipped(), vec![id, id]);

        assert_eq!(inv.set_live_count(id, 0), 0);
        let expected: Vec<FormId> = inv.groups().filter(|g| g.1.equipped).map(|g| g.0).collect();
        assert_eq!(expected, vec![id]);
        assert_eq!(inv.equipped(), expected);
        assert_eq!(inv.equipped_iter().collect::<Vec<_>>(), expected);
        assert!(inv.is_equipped(id));
        let round: Inventory = serde_json::from_value(serde_json::to_value(&inv).unwrap()).unwrap();
        assert_eq!(round, inv);
    }

    proptest! {
        /// Counts never go negative and the total is conserved by add/remove/equip.
        #[test]
        fn conservation(ops in proptest::collection::vec((0u32..4, 1i32..50, 0u8..4), 0..200)) {
            let mut inv = Inventory::new();
            let mut expect = [0i64; 4];
            for (item, n, op) in ops {
                let id = FormId(item);
                match op {
                    0 => {
                        inv.add(id, n);
                        expect[item as usize] += n as i64;
                    }
                    1 => {
                        inv.add_extra(id, n, Extra { condition: Some(0.5), ..Extra::default() });
                        expect[item as usize] += n as i64;
                    }
                    2 => {
                        inv.set_equipped(id);
                    }
                    _ => {
                        let got = inv.remove(id, n);
                        prop_assert!(got >= 0 && got <= n);
                        expect[item as usize] -= got as i64;
                    }
                }
                for (i, e) in expect.iter().enumerate() {
                    prop_assert_eq!(inv.count(FormId(i as u32)) as i64, *e);
                    prop_assert!(*e >= 0);
                }
                let back: Inventory = serde_json::from_value(serde_json::to_value(&inv).unwrap()).unwrap();
                prop_assert_eq!(&back, &inv);
            }
        }
    }
}
