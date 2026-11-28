//! Orthogonal record- and recipe-backed ySI tag predicates (`docs/spec-notes/ysi-filename.md`).
//!
//! Covers documented predicates outside the core `mod`/`form`/`formlist`/`formType`/`questItem`
//! set and outside the core `WEAP`/`ALCH`/`ARMO` subset in `ysi_filename.rs`. Every supported
//! key name below appears verbatim in the public **Sorting Icons 1.4b** (`ySI - Sorting Ycons`,
//! Nexus mod 74358) `menus/ySI/ySI.json` configuration file; no ySI/yUI DLL or source code was
//! consulted:
//! - Extra `WEAP` traits (`weaponClipRounds`, `weaponNumProjectiles`, `weaponAttackAnim`,
//!   `weaponHandgrip`, `weaponSoundLevel`, `ammoMod` + `ammoForm`)
//! - `ARMO` threshold (`armorDT`) and `formlist: 2` repair-list / nested-`FLST` membership
//! - `ALCH` effect/classification traits (`aidRestoresAV`, `aidDamagesAV`, `aidIsWater`,
//!   `aidIsPoisonous`)
//! - Recipe membership (`miscComponent`, `miscProduct`)
//!
//! Keys absent from Sorting Icons 1.4b `menus/ySI/ySI.json` (including `bookSkill`,
//! `isSkillBook`, `noteType`, `ammoIgnoresDTDR`, `prefix`, `suffix`, `edid`, `edidPrefix`,
//! which were also observed to be unrecognized/ignored by `yUI 1.4d`) remain `Match::Unknown`.

use crate::aid::{self, value_change};
use crate::data::MagicEffectDef;
use crate::game::Game;
use crate::ysi_filename::{any, bool_pred, number_set, parse_local_form, Match};
use fnv_script::vm::XVal;
use fnv_sim::FormId;
use serde_json::{Map, Value as Json};
use std::collections::{HashSet, VecDeque};

/// Safety bound on nested `FLST` nodes visited during `formlist: 2` traversal [design].
const MAX_NESTED_FLST_VISITS: usize = 256;

/// Evaluates an extended ySI predicate key. Returns `Match::Unknown` for unrecognized keys
/// or malformed predicate values. When `explicit_form` is true (rule carries an explicit
/// `form` selector with `formlist: 0`), non-applicable record-type predicates on mismatched
/// record types are ignored (`Match::Yes`) instead of disqualifying (`Match::No`)
/// [hypothesis: matches yUI v1.4d runtime behavior].
pub(crate) fn eval(
    g: &Game<'_>,
    item: FormId,
    fields: &Map<String, Json>,
    name: &str,
    value: &Json,
    explicit_form: bool,
) -> Match {
    match name {
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (`"ammoMod": "FalloutNV.esm"`, companion to `ammoForm`)]
        "ammoMod" => Match::Yes,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json WEAP.NAM0]
        "ammoForm" => weapon_ammo_match(g, item, fields, value, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (values 3, 7); schemas/fnv/objects.json WEAP.DATA.clip_size;
        //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d, bSortingIcons=1): >= N threshold]
        "weaponClipRounds" => weapon_threshold_ge(g, item, "DATA", "clip_size", value, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (value 4); schemas/fnv/objects.json WEAP.DNAM.projectile_count;
        //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d, bSortingIcons=1): >= N threshold]
        "weaponNumProjectiles" => weapon_threshold_ge(g, item, "DNAM", "projectile_count", value, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (values 10, 11); schemas/fnv/objects.json WEAP.DNAM.attack_animation]
        "weaponAttackAnim" => weapon_int_field(g, item, "DNAM", "attack_animation", value, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (value 3); schemas/fnv/objects.json WEAP.DNAM.grip_animation]
        "weaponHandgrip" => weapon_int_field(g, item, "DNAM", "grip_animation", value, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (value 2); schemas/fnv/objects.json WEAP.VNAM]
        "weaponSoundLevel" => weapon_int_field(g, item, "VNAM", "", value, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json ARMO.DNAM.damage_threshold;
        //  GECK wiki SetArmorDT; observed: w4e_c01_ysi_armordt_sibling_formlist2 (#3803): >= N minimum DT threshold]
        "armorDT" => armor_dt(g, item, value, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (values 54, 20, 16); schemas/fnv/objects.json ALCH/MGEF]
        "aidRestoresAV" => aid_effect_av(g, item, value, true, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (value 54); schemas/fnv/objects.json ALCH/MGEF]
        "aidDamagesAV" => aid_effect_av(g, item, value, false, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (value 1);
        //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d, bSortingIcons=1): matches WaterPurified, rejects SalisburySteak/NukaCola]
        "aidIsWater" => aid_is_water(g, item, value, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (value 1);
        //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d, bSortingIcons=1): matches BleakVenom]
        "aidIsPoisonous" => aid_is_poisonous(g, item, value, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (value 1); schemas/fnv/gameplay.json RCPE.RCIL]
        "miscComponent" => recipe_membership(g, item, value, true, explicit_form),
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json (value 1); schemas/fnv/gameplay.json RCPE.RCOD]
        "miscProduct" => recipe_membership(g, item, value, false, explicit_form),
        _ => Match::Unknown,
    }
}

fn mismatched_type(explicit_form: bool) -> Match {
    if explicit_form {
        Match::Yes
    } else {
        Match::No
    }
}

/// Evaluates `formlist: 2` against `mod` + `form` [documented: Sorting Icons 1.4b
/// `menus/ySI/ySI.json` (`"formlist": 2`), `schemas/fnv/objects.json` `WEAP`/`ARMO.REPL`,
/// GECK wiki `SetRepairList`; hypothesis: `REPL` repair-list matching, recursive nested
/// `FLST` traversal, and mixed live non-`FLST` direct equality]:
/// - When a resolved target form is an `FLST`, matches if `item`'s effective repair list
///   (`REPL`, including `SetRepairList` overrides) equals that `FLST` OR if `item` is
///   reachable through `FLST` membership (including nested `FLST`s with a cycle guard).
/// - When a resolved target is a live non-`FLST` form (e.g. a direct item in a mixed
///   `formlist: 2` rule), matches direct equality `g.unclone(item) == target`.
/// - [design] Missing plugin or non-existent local form in a loaded plugin yields `Match::No`;
///   malformed hex or missing `mod` yields `Match::Unknown`.
pub(crate) fn formlist_mode2(
    g: &Game<'_>,
    item: FormId,
    fields: &Map<String, Json>,
    value: &Json,
) -> Match {
    let Some(plugin) = fields.get("mod").and_then(Json::as_str) else {
        return Match::Unknown;
    };
    let Some(index) = g
        .data
        .load_order
        .iter()
        .position(|p| p.eq_ignore_ascii_case(plugin))
    else {
        return Match::No;
    };
    if index > 255 {
        return Match::Unknown;
    }
    let orig_item = g.unclone(item);
    let item_repair_list = g.effective_repair_list_form(item);
    let matches_one = |v: &Json| {
        let Some(local) = parse_local_form(v) else {
            return Match::Unknown;
        };
        let target = FormId(((index as u32) << 24) | local);
        match g.data.sig(target) {
            Some(sig) if &sig == b"FLST" => {
                if item_repair_list == Some(target)
                    || flst_contains_recursive(g, target, item)
                    || (orig_item != item && flst_contains_recursive(g, target, orig_item))
                {
                    Match::Yes
                } else {
                    Match::No
                }
            }
            Some(_) => {
                if item == target || orig_item == target {
                    Match::Yes
                } else {
                    Match::No
                }
            }
            None => Match::No,
        }
    };
    match value {
        Json::Array(values) => any(values.iter().map(matches_one)),
        _ => matches_one(value),
    }
}

fn flst_contains_recursive(g: &Game<'_>, root_list: FormId, needle: FormId) -> bool {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(root_list);
    queue.push_back(root_list);
    let mut steps = 0usize;
    while let Some(list_id) = queue.pop_front() {
        steps += 1;
        if steps > MAX_NESTED_FLST_VISITS {
            break;
        }
        for member in crate::functions::refs::list_members(g, list_id) {
            if member == needle || g.unclone(member) == needle {
                return true;
            }
            if g.data.sig(member) == Some(*b"FLST") && visited.insert(member) {
                queue.push_back(member);
            }
        }
    }
    false
}

/// Evaluates `armorDT` minimum threshold against an `ARMO`'s effective Damage Threshold
/// [documented: Sorting Icons 1.4b `menus/ySI/ySI.json`, `schemas/fnv/objects.json`
/// `ARMO.DNAM.damage_threshold`, GECK wiki `SetArmorDT`; observed:
/// `w4e_c01_ysi_armordt_sibling_formlist2` (#3803): `dt >= threshold` on authored `ARMO`
/// records; hypothesis: honoring live `SetArmorDT` overrides and skipping non-applicable
/// `armorDT` (`Match::Yes`) on non-`ARMO` forms when paired with an explicit `form`
/// selector (disqualifying as `Match::No` otherwise)].
/// [design] Negative, non-finite, or non-numeric thresholds remain `Match::Unknown`.
fn armor_dt(g: &Game<'_>, item: FormId, value: &Json, explicit_form: bool) -> Match {
    let Some(threshold) = value.as_f64().filter(|n| n.is_finite() && *n >= 0.0) else {
        return Match::Unknown;
    };
    if g.data.sig(item) != Some(*b"ARMO") {
        return mismatched_type(explicit_form);
    }
    let base = g.unclone(item);
    let dt = g
        .world
        .effective_armor_dt(item)
        .or_else(|| g.world.effective_armor_dt(base))
        .map(f64::from)
        .or_else(|| match crate::nvse::record_value(g, item, "DNAM", "damage_threshold") {
            Some(XVal::Num(n)) if n.is_finite() => Some(n),
            _ => None,
        })
        .unwrap_or(0.0);
    if dt >= threshold {
        Match::Yes
    } else {
        Match::No
    }
}

/// Returns `true` when a companion selector (`ammoMod`) appears without its target (`ammoForm`).
pub(crate) fn has_dangling_companions(fields: &Map<String, Json>) -> bool {
    fields.contains_key("ammoMod") && !fields.contains_key("ammoForm")
}

/// Evaluate a scalar `>= N` threshold predicate [observed: yUI 1.4d on `weaponClipRounds`
/// and `weaponNumProjectiles`]. Non-integer values stay `Match::Unknown`.
fn threshold_ge(value: &Json, actual: i64) -> Match {
    match value.as_i64() {
        Some(min) if actual >= min => Match::Yes,
        Some(_) => Match::No,
        None => Match::Unknown,
    }
}

fn weapon_threshold_ge(g: &Game<'_>, item: FormId, sub: &str, path: &str, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"WEAP") {
        return mismatched_type(explicit_form);
    }
    match crate::nvse::record_value(g, item, sub, path) {
        Some(XVal::Num(n)) if n.is_finite() => threshold_ge(value, n.trunc() as i64),
        _ => Match::Unknown,
    }
}

fn weapon_int_field(g: &Game<'_>, item: FormId, sub: &str, path: &str, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"WEAP") {
        return mismatched_type(explicit_form);
    }
    match crate::nvse::record_value(g, item, sub, path) {
        Some(XVal::Num(n)) if n.is_finite() => number_set(value, n.trunc() as i64),
        _ => Match::Unknown,
    }
}

fn weapon_ammo_match(g: &Game<'_>, item: FormId, fields: &Map<String, Json>, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"WEAP") {
        return mismatched_type(explicit_form);
    }
    let Some(plugin) = fields.get("ammoMod").and_then(Json::as_str) else {
        return Match::Unknown;
    };
    let Some(index) = g
        .data
        .load_order
        .iter()
        .position(|p| p.eq_ignore_ascii_case(plugin))
    else {
        return Match::No;
    };
    if index > 255 {
        return Match::Unknown;
    }
    let ammo = g.weapon_ammo(item);
    let check = |v: &Json| {
        let Some(local) = parse_local_form(v) else {
            return Match::Unknown;
        };
        let target = FormId(((index as u32) << 24) | local);
        if ammo.contains(&target) {
            Match::Yes
        } else {
            Match::No
        }
    };
    match value {
        Json::Array(values) => any(values.iter().map(check)),
        _ => check(value),
    }
}

fn aid_effect_av(g: &Game<'_>, item: FormId, value: &Json, restores: bool, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"ALCH") {
        return mismatched_type(explicit_form);
    }
    let base = g.unclone(item);
    let effects = g.data.effect_items.get(&base).map(Vec::as_slice).unwrap_or(&[]);
    let matches_code = |code: i64| {
        effects.iter().any(|e| {
            let Some(m) = g.magic_effect_def(e.effect) else {
                return false;
            };
            let Some(change) = value_change(&m, e, 1.0, false) else {
                return false;
            };
            if i64::from(change.code) != code {
                return false;
            }
            let detrimental = m.has(MagicEffectDef::DETRIMENTAL);
            if restores {
                !detrimental && change.amount > 0.0
            } else {
                detrimental || change.amount < 0.0
            }
        })
    };
    let one = |v: &Json| match v.as_i64() {
        Some(code) => if matches_code(code) { Match::Yes } else { Match::No },
        None => Match::Unknown,
    };
    match value {
        Json::Array(values) => any(values.iter().map(one)),
        _ => one(value),
    }
}

fn aid_is_water(g: &Game<'_>, item: FormId, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"ALCH") {
        return mismatched_type(explicit_form);
    }
    let base = g.unclone(item);
    let restores_dehydration = g
        .data
        .effect_items
        .get(&base)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .any(|e| {
            let Some(m) = g.magic_effect_def(e.effect) else {
                return false;
            };
            value_change(&m, e, 1.0, false).is_some_and(|c| {
                c.code == aid::DEHYDRATION && !m.has(MagicEffectDef::DETRIMENTAL) && c.amount > 0.0
            })
        });
    bool_pred(value, restores_dehydration, false)
}

fn aid_is_poisonous(g: &Game<'_>, item: FormId, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"ALCH") {
        return mismatched_type(explicit_form);
    }
    bool_pred(value, g.is_poison(item), false)
}

fn recipe_membership(g: &Game<'_>, item: FormId, value: &Json, ingredient: bool, explicit_form: bool) -> Match {
    if g.world.forms.item(g.unclone(item)).is_none() {
        return mismatched_type(explicit_form);
    }
    let target = g.unclone(item);
    let has = g.data.recipes.keys().copied().any(|id| {
        let recipe = g
            .other
            .lutana
            .recipes
            .get(&id)
            .or_else(|| g.data.recipes.get(&id));
        let Some(r) = recipe else { return false };
        let list = if ingredient { &r.ingredients } else { &r.outputs };
        list.iter().any(|&(f, _)| g.unclone(f) == target)
    });
    bool_pred(value, has, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_ge_covers_bounds_and_rejects_non_scalars() {
        assert_eq!(threshold_ge(&Json::from(7), 12), Match::Yes);
        assert_eq!(threshold_ge(&Json::from(7), 7), Match::Yes);
        assert_eq!(threshold_ge(&Json::from(7), 6), Match::No);
        assert_eq!(threshold_ge(&serde_json::json!([7, 12]), 12), Match::Unknown);
        assert_eq!(threshold_ge(&Json::from("7"), 12), Match::Unknown);
    }

    #[test]
    fn dangling_ammo_mod_without_ammo_form_is_flagged() {
        let mut only_mod = Map::new();
        only_mod.insert("ammoMod".into(), Json::from("FalloutNV.esm"));
        assert!(has_dangling_companions(&only_mod));

        only_mod.insert("ammoForm".into(), Json::from("00004241"));
        assert!(!has_dangling_companions(&only_mod));
    }
}
