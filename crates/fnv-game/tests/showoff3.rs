//! ShowOff NVSE batch 3 (#1010; docs/spec-notes/ext-showoff.md §6) on a synthetic plugin
//! written for the test (no game data, made-up names): the equipped-item family, the
//! seeded randomizer, easing by code, temporary faction relations, the player's flags and
//! the aux timers' countdowns.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nvse_showoff3::{Mt19937, DEFAULT_SEED};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::inventory::Extra;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const GUN: u32 = 0x800;
const VEST: u32 = 0x801;
const HAT: u32 = 0x802;
const PIP: u32 = 0x803;
const POWER_SUIT: u32 = 0x804;
const FLST: u32 = 0x810;
const FACT_A: u32 = 0x820;
const FACT_B: u32 = 0x821;

/// WEAP DATA: value, health, weight, damage, clip [documented: schemas/fnv WEAP DATA].
fn weap_data(weight: f32) -> Vec<u8> {
    [0i32.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), weight.to_le_bytes().to_vec(), vec![0; 3]].concat()
}

/// ARMO: BMDT biped flags and general flags (0x40 Non-Playable), DATA value, health,
/// weight [documented: schemas/fnv/objects.json ARMO].
fn armo(id: u32, edid: &str, biped: u32, general: u8, weight: f32) -> Vec<u8> {
    let bmdt = [biped.to_le_bytes().to_vec(), vec![general, 0, 0, 0]].concat();
    let data = [5i32.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), weight.to_le_bytes().to_vec()].concat();
    record(b"ARMO", 0, id, &[sub(b"EDID", z(edid)), sub(b"BMDT", bmdt), sub(b"DATA", data)])
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let weap = record(b"WEAP", 0, GUN, &[sub(b"EDID", z("TestGun")), sub(b"DATA", weap_data(6.0))]);
    let armos = [
        armo(VEST, "TestVest", 1 << 2, 0, 4.0),
        armo(HAT, "TestHat", 1 | 1 << 10, 0, 1.0),
        armo(PIP, "TestPip", 1 << 6, 0x40, 0.5),
        armo(POWER_SUIT, "TestPowerSuit", 1 << 2, 0x20, 20.0),
    ]
    .concat();
    let flst = record(b"FLST", 0, FLST, &[sub(b"EDID", z("TestList")), sub(b"LNAM", u32s(&[HAT])), sub(b"LNAM", u32s(&[VEST]))]);
    let facts = [record(b"FACT", 0, FACT_A, &[sub(b"EDID", z("TestFactionA"))]), record(b"FACT", 0, FACT_B, &[sub(b"EDID", z("TestFactionB"))])].concat();
    let bytes = [
        tes4(),
        top(b"FACT", &facts),
        top(b"WEAP", &weap),
        top(b"ARMO", &armos),
        top(b"FLST", &flst),
        top(b"NPC_", &record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))])),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "ShowOff3Test.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() })
}

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

/// An array's entries as numbers (FormIDs for forms), sorted.
fn sorted(g: &mut Game, expr: &str) -> Vec<f64> {
    let at = |i: &str| format!("array_var a\nlet a := {expr}\nSetFunctionValue {i}");
    let len = match q(g, &at("(ar_Size a)")) {
        XVal::Num(x) => x as usize,
        v => panic!("{expr}: {v:?}"),
    };
    let mut v: Vec<f64> = (0..len)
        .map(|i| match q(g, &at(&format!("(a[{i}])"))) {
            XVal::Ref(r) => f64::from(r),
            XVal::Num(x) => x,
            v => panic!("{expr}[{i}]: {v:?}"),
        })
        .collect();
    v.sort_by(f64::total_cmp);
    v
}

/// The player wears the vest (at 20 % health), the hat and the non-playable Pip-Boy and
/// holds the gun.
fn dressed(g: &mut Game) {
    let inv = g.world.inventory_mut(FormId::PLAYER_REF).unwrap();
    inv.add(FormId(GUN), 1);
    inv.add_extra(FormId(VEST), 1, Extra { condition: Some(0.2), ..Extra::default() });
    inv.add(FormId(HAT), 1);
    inv.add(FormId(PIP), 1);
    for i in [GUN, VEST, HAT, PIP] {
        assert!(inv.set_equipped(FormId(i)));
    }
}

#[test]
fn equipped_items_by_slot_mask() {
    let f = fixture();
    let mut g = game(&f);
    dressed(&mut g);
    let ids = |v: &[u32]| {
        let mut v: Vec<f64> = v.iter().map(|&x| f64::from(x)).collect();
        v.sort_by(f64::total_cmp);
        v
    };
    // No mask: the playable ones; a mask: those slots, playable or not [documented].
    assert_eq!(sorted(&mut g, "player.GetEquippedItems"), ids(&[GUN, VEST, HAT]));
    assert_eq!(sorted(&mut g, "player.GetEquippedItems 64"), ids(&[PIP]));
    assert_eq!(sorted(&mut g, "player.GetEquippedItems (1 << 10 | 1 << 2)"), ids(&[VEST, HAT]), "the hat once though it takes two slots");
    assert_eq!(n(&mut g, "player.GetEquippedItemsAsBitMask"), f64::from(1 | 4 | 32 | 64 | 1024));
    assert_eq!(n(&mut g, "player.GetOccupiedEquipSlots"), f64::from(1 | 4 | 32 | 64 | 1024));
    assert_eq!(sorted(&mut g, "player.GetEquippedItemRefs").len(), 3);
    // Weights: 6 + 4 + 1; at least 2; the Pip-Boy's slot alone.
    assert_eq!(n(&mut g, "player.GetBaseEquippedWeight"), 11.0);
    assert_eq!(n(&mut g, "player.GetBaseEquippedWeight 2"), 10.0);
    assert_eq!(n(&mut g, "player.GetBaseEquippedWeight 0 64"), 0.5);
    assert_eq!(n(&mut g, "player.GetCalculatedEquippedWeight"), 11.0, "no weight perks");
    // Nothing that heavy: 0, not -0 (which prints "-0.000") [observed: L5.ext rerun, #2040].
    for f in ["GetBaseEquippedWeight", "GetCalculatedEquippedWeight"] {
        let w = n(&mut g, &format!("player.{f} 1000"));
        assert!(w == 0.0 && w.is_sign_positive(), "{f}: {w}");
    }
    // The vest is at 20 %.
    assert_eq!(n(&mut g, "player.GetNumBrokenEquippedItems"), 0.0);
    assert_eq!(n(&mut g, "player.GetNumBrokenEquippedItems 30"), 1.0);
    assert_eq!(n(&mut g, "player.GetNumBrokenEq 30 32"), 0.0, "the weapon slot alone");
    // Inventory references: the weapon; the first of a list that is worn.
    assert_ne!(n(&mut g, "player.GetEquippedWeaponRef"), 0.0);
    let hat_ref = n(&mut g, "player.GetEquippedItemRefForItem TestHat");
    assert_ne!(hat_ref, 0.0);
    assert_eq!(n(&mut g, "player.GetEquippedItemRefForItem TestList"), hat_ref);
    // Unequip the upper body: the vest goes, the rest stay.
    q(&mut g, "player.UnequipItems 4 0 1");
    assert_eq!(sorted(&mut g, "player.GetEquippedItems"), ids(&[GUN, HAT]));
    assert_eq!(n(&mut g, "player.GetEquippedItemRefForItem TestVest"), 0.0);
    assert!(n(&mut g, "player.GetCalculatedMaxCarryWeight") > 0.0);
}

/// A draw scaled onto `lo..=hi`, a biased one taken again [observed: showoff3
/// so3.seed.default, so3.seed.range.wide].
fn draw(m: &mut Mt19937, lo: u64, hi: u64) -> f64 {
    let span = hi - lo + 1;
    loop {
        let p = u64::from(m.next_u32()) * span;
        if p & 0xFFFF_FFFF >= (1u64 << 32) % span {
            return (lo + (p >> 32)) as f64;
        }
    }
}

#[test]
fn seeded_randomizer_follows_the_mersenne_twister() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: GetRandomizerSeed -1 with none; AdvanceSeed 0 with none].
    assert_eq!(n(&mut g, "GetRandomizerSeed"), -1.0);
    assert_eq!(n(&mut g, "AdvanceSeed 3"), 0.0);
    // A first draw makes the default seed.
    let mut m = Mt19937::new(DEFAULT_SEED);
    assert_eq!(n(&mut g, "RandSeeded 1 6"), draw(&mut m, 1, 6));
    assert_eq!(n(&mut g, "GetSeed"), f64::from(DEFAULT_SEED));
    // The original's first five 1-100 draws of the default seed [observed: showoff3
    // so3.seed.default].
    q(&mut g, "SetRandomizerSeed 0");
    let five: Vec<f64> = (0..5).map(|_| n(&mut g, "RandSeeded 1 100")).collect();
    assert_eq!(five, vec![82.0, 14.0, 91.0, 84.0, 13.0]);
    // The third output is thrown away for this span [observed: so3.seed.range.wide].
    q(&mut g, "SetRandomizerSeed 5489");
    let three: Vec<f64> = (0..3).map(|_| n(&mut g, "RandSeeded 0 1000000000")).collect();
    assert_eq!(three, vec![814723692.0, 135477004.0, 835008590.0]);
    // SetRandomizerSeed starts again; AdvanceSeed skips draws.
    q(&mut g, "SetRandomizerSeed 20");
    let mut m = Mt19937::new(20);
    m.next_u32();
    m.next_u32();
    assert_eq!(n(&mut g, "AdvanceSeed 2"), 1.0);
    assert_eq!(n(&mut g, "GetRandomPercentSeeded"), draw(&mut m, 1, 100));
    assert_eq!(n(&mut g, "RandSeeded 5 2"), -1.0, "min above max");
    q(&mut g, "SetSeedUsingForm TestHat");
    assert_eq!(n(&mut g, "GetRandomizerSeed"), f64::from(HAT));
    q(&mut g, "SetSeed 0");
    assert_eq!(n(&mut g, "GetRandomizerSeed"), f64::from(DEFAULT_SEED));
}

/// A huge or garbage AdvanceSeed count is capped (`ADVANCE_SEED_MAX`), so it returns at
/// once instead of holding the frame.
#[test]
fn advance_seed_caps_a_huge_count() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "SetRandomizerSeed 20");
    let t = std::time::Instant::now();
    assert_eq!(n(&mut g, "AdvanceSeed 1000000000000"), 1.0);
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "capped, not 1e12 draws");
    let mut m = Mt19937::new(20);
    for _ in 0..1_000_000 {
        m.next_u32();
    }
    assert_eq!(n(&mut g, "GetRandomPercentSeeded"), draw(&mut m, 1, 100));
}

#[test]
fn easing_codes_flags_and_factions() {
    let f = fixture();
    let mut g = game(&f);
    // Quad in at 0.5; an unknown code -1 [documented: ApplyEasingAlt].
    assert_eq!(n(&mut g, "ApplyEasingAlt 0.5 1 0"), 0.25);
    assert_eq!(n(&mut g, "EaseAlt 0.5 1 0"), n(&mut g, "ApplyEasing 0.5 \"quad\" 0"));
    assert_eq!(n(&mut g, "ApplyEasingAlt 0.5 10 0"), -1.0);
    assert_eq!(n(&mut g, "IsOutsideMainThread"), 0.0);
    assert_eq!(n(&mut g, "player.ConditionPrint"), 1.0);
    // The player's flags.
    q(&mut g, "SetIsPCAMurderer 1");
    assert_eq!(n(&mut g, "IsPCAMurderer"), 1.0);
    assert_eq!(n(&mut g, "GetPCHasSleepWaitOverride"), 0.0);
    q(&mut g, "SetPCHasSleepWaitOverride 1");
    assert!(g.player.fast_travel.wait_disabled);
    assert_eq!(n(&mut g, "GetPCHasSleepWaitOverride"), 1.0);
    // Temporary relations, until SetEnemy changes the pair.
    assert_eq!(n(&mut g, "GetFactionCombatReaction TestFactionA TestFactionB"), 0.0);
    q(&mut g, "SetAllyTemp TestFactionA TestFactionB 0 1");
    assert_eq!(n(&mut g, "GetFactionCombatReaction TestFactionA TestFactionB"), 2.0);
    assert_eq!(n(&mut g, "GetFactionCombatReaction TestFactionB TestFactionA"), 3.0);
    assert!(g.forms.faction_reactions.is_empty(), "nothing saved");
    q(&mut g, "SetEnemyTemp TestFactionA TestFactionB 1 0");
    assert_eq!(n(&mut g, "GetFactionCombatReaction TestFactionA TestFactionB"), 0.0);
    assert_eq!(n(&mut g, "GetFactionCombatReaction TestFactionB TestFactionA"), 1.0);
    q(&mut g, "SetAlly TestFactionA TestFactionB");
    assert_eq!(n(&mut g, "GetFactionCombatReaction TestFactionB TestFactionA"), 2.0);
}

#[test]
fn aux_timer_countdowns() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "AuxTimerTimeToCountdown \"t\""), -1.0);
    q(&mut g, "AuxTimerStart \"t\" 5");
    assert_eq!(n(&mut g, "AuxTimerTimeToCountdown \"T\""), 5.0);
    q(&mut g, "AuxTimerStop \"t\"");
    assert_eq!(n(&mut g, "AuxTimerTimeToCountdown \"t\""), -1.0);
}

#[test]
fn small_getters_run() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(sorted(&mut g, "player.GetPosArray").len(), 3);
    assert_eq!(n(&mut g, "player.IsJumping"), n(&mut g, "player.IsInAir"));
    assert_eq!(n(&mut g, "player.IsAnimPlayingExCond 3"), 0.0);
    assert_eq!(n(&mut g, "player.GetNumActorsInRangeFromRef 500 1"), 0.0, "the player does not count itself");
    assert!(sorted(&mut g, "TopicInfoGetResponseStrings TestHat").is_empty());
    assert_eq!(n(&mut g, "ModNumericINISetting \"fNoSuchSetting:General\" 1"), 0.0);
    assert_eq!(n(&mut g, "GetChallengeProgress TestHat"), 0.0);
    // Level 1, the menu closed: evaluates `level + 1` (iLevelsPerPerk = 2), including
    // for explicit nonzero `levelOverride` values (`1/2/3/4` -> `1/0/1/0`).
    assert_eq!(n(&mut g, "GetCalculatedPerkPoints"), 1.0);
    assert_eq!(n(&mut g, "GetCalculatedPerkPoints 1"), 1.0);
    assert_eq!(n(&mut g, "GetCalculatedPerkPoints 2"), 0.0);
    assert_eq!(n(&mut g, "GetCalculatedPerkPoints 3"), 1.0);
    assert_eq!(n(&mut g, "GetCalculatedPerkPoints 4"), 0.0);
    assert!(n(&mut g, "GetCalculatedSkillPoints") >= 10.0);
    assert_eq!(n(&mut g, "player.GetRadiationExtraData"), 0.0);
    assert_eq!(n(&mut g, "player.SetRadiationExtraData 20"), 1.0);
    assert_eq!(n(&mut g, "player.GetRadiationExtraData"), 20.0);
}

/// JIP's IsAnimPlayingEx movement sub-types and variation flags [documented:
/// IsAnimPlayingEx]: running forward is not a jump (Enhanced Movement's sprint stays off
/// while `IsAnimPlayingEx 2 5` holds, #2275).
#[test]
fn is_anim_playing_ex_tells_movement_sub_types_apart() {
    use fnv_game::anim_state::{AnimState, MoveDir};
    let f = fixture();
    let mut g = game(&f);
    let p = fnv_sim::FormId::PLAYER_REF;
    g.set_anim_state(p, AnimState { moving: MoveDir::Forward, group: Some("FastForward"), ..Default::default() });
    for (expr, want) in [("2", 1.0), ("2 1", 1.0), ("2 2", 0.0), ("2 5", 0.0), ("2 0 1", 1.0), ("2 1 4", 0.0)] {
        assert_eq!(n(&mut g, &format!("player.IsAnimPlayingEx {expr}")), want, "running forward: {expr}");
    }
    g.set_anim_state(p, AnimState { moving: MoveDir::Forward, group: Some("Forward"), ..Default::default() });
    assert_eq!(n(&mut g, "player.IsAnimPlayingEx 2 0 1"), 0.0, "walking is not fast");
    g.set_anim_state(p, AnimState { moving: MoveDir::None, group: Some("JumpLoop"), ..Default::default() });
    assert_eq!(n(&mut g, "player.IsAnimPlayingEx 2 5"), 1.0, "in a jump");
    assert_eq!(n(&mut g, "player.IsAnimPlayingEx 2"), 1.0);
}

#[test]
fn calculated_equipped_weight_honors_weightless_tweaks_and_perks() {
    use fnv_sim::inventory::{WeightRules, WEIGHTLESS_ARMOR, WEIGHTLESS_WEAPONS};
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetPluginVersion \"lStewieAl's Tweaks\""), 960.0);

    // Equip gun (6.0), power armor (20.0), and hat (1.0).
    let inv = g.world.inventory_mut(FormId::PLAYER_REF).unwrap();
    for id in [GUN, POWER_SUIT, HAT] {
        inv.add(FormId(id), 1);
        assert!(inv.set_equipped(FormId(id)));
    }
    assert_eq!(n(&mut g, "player.GetBaseEquippedWeight"), 27.0);
    assert_eq!(n(&mut g, "player.GetCalculatedEquippedWeight"), 27.0);
    assert_eq!(n(&mut g, "player.GetInventoryWeight"), 27.0);

    // bWeightlessWornPowerArmor zeroes worn power armor while keeping regular armor and weapon.
    g.tweaks.weightless_worn_power_armor = true;
    g.sync_perk_weights();
    assert_eq!(n(&mut g, "player.GetBaseEquippedWeight"), 27.0);
    assert_eq!(n(&mut g, "player.GetCalculatedEquippedWeight"), 7.0);
    assert_eq!(n(&mut g, "player.GetInventoryWeight"), 7.0);
    assert_eq!(n(&mut g, "player.GetCalculatedEquippedWeight 2"), 6.0, "minWeight filters weightless power armor out");

    // bWeightlessWornArmor zeroes all worn armor (both power armor and hat), leaving only the gun.
    g.tweaks.weightless_worn_power_armor = false;
    g.tweaks.weightless_worn_armor = true;
    g.sync_perk_weights();
    assert_eq!(n(&mut g, "player.GetCalculatedEquippedWeight"), 6.0);
    assert_eq!(n(&mut g, "player.GetInventoryWeight"), 6.0);

    // bWeightlessItems (weapons + armor) zeroes everything and keeps +0.0 (not -0.0).
    g.tweaks.weightless_worn_armor = false;
    g.tweaks.weightless_kinds = WEIGHTLESS_ARMOR | WEIGHTLESS_WEAPONS;
    g.sync_perk_weights();
    let zero_w = n(&mut g, "player.GetCalculatedEquippedWeight");
    assert!(zero_w == 0.0 && zero_w.is_sign_positive(), "expected +0.0, got {zero_w}");
    assert_eq!(n(&mut g, "player.GetInventoryWeight"), 0.0);

    // Perk rules still apply to non-weightless equipped items alongside weightless worn power armor.
    g.tweaks.weightless_kinds = 0;
    g.world.set_weight_rules(
        FormId::PLAYER_REF,
        WeightRules { light: Some((2.0, 0.5)), heavy_weapons: Some((6.0, 0.5)), weightless_worn_power_armor: true, ..Default::default() },
    );
    // Gun: 6 * 0.5 = 3.0; Hat: 1 * 0.5 = 0.5; Power Suit: 0.0.
    assert_eq!(n(&mut g, "player.GetCalculatedEquippedWeight"), 3.5);
    assert_eq!(n(&mut g, "player.GetInventoryWeight"), 3.5);
}

#[test]
fn weapon_flag_overrides_sync_getters_playability_and_equipment() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetPluginVersion \"lStewieAl's Tweaks\""), 960.0);
    dressed(&mut g);

    let gun = FormId(GUN);
    assert_eq!(g.weapon_flags1_override(gun), None);
    assert_eq!(g.weapon_flags1(gun), Some(0));
    assert!(!g.weapon_is_automatic(gun) && !g.weapon_cant_drop(gun));
    assert!(g.is_item_playable(gun) && !g.is_item_non_playable(gun));
    assert_eq!(n(&mut g, "IsPlayable TestGun"), 1.0);
    assert_eq!(n(&mut g, "LNIsPlayable TestGun"), 1.0);
    assert_eq!(n(&mut g, "player.IsEquippedWeaponScoped"), 0.0);

    // Set Is Automatic (1), Has Scope (2), Can't Drop (3) via SetWeaponFlag and alias SetWeaponHasFlag.
    q(&mut g, "SetWeaponFlag 1 1 TestGun");
    q(&mut g, "SetWeaponHasFlag 2 1 TestGun");
    q(&mut g, "player.SetWeaponFlag 3 1");
    let want = (1u8 << 1) | (1u8 << 2) | (1u8 << 3);
    assert_eq!(g.weapon_flags1_override(gun), Some(want));
    assert_eq!(g.weapon_flags1(gun), Some(want));
    assert!(g.weapon_is_automatic(gun) && g.weapon_cant_drop(gun));
    assert_eq!(n(&mut g, "WeaponHasFlag 1 TestGun"), 1.0);
    assert_eq!(n(&mut g, "GetWeaponHasFlag 2 TestGun"), 1.0);
    assert_eq!(n(&mut g, "player.WeaponHasFlag 3"), 1.0);
    assert_eq!(n(&mut g, "GetWeaponFlags1 TestGun"), f64::from(want));
    assert_eq!(n(&mut g, "GetWeaponIsAutomatic TestGun"), 1.0);
    assert_eq!(n(&mut g, "GetWeaponHasScope TestGun"), 1.0);
    assert_eq!(n(&mut g, "player.IsEquippedWeaponScoped"), 1.0);

    // Calling SetWeaponFlag / WeaponHasFlag on an inventory reference resolves the underlying WEAP.
    q(&mut g, "ref ir\nlet ir := player.GetEquippedWeaponRef\nir.SetWeaponFlag 0 1");
    assert_eq!(n(&mut g, "WeaponHasFlag 0 TestGun"), 1.0);
    assert_eq!(q(&mut g, "ref ir\nlet ir := player.GetEquippedWeaponRef\nSetFunctionValue (ir.WeaponHasFlag 0)"), XVal::Num(1.0));

    // Toggling Non-Playable (bit 7) updates IsPlayable, LNIsPlayable, GetEquippedItems, and GetAllItemRefs.
    q(&mut g, "SetWeaponFlag 7 1 TestGun");
    assert!(!g.is_item_playable(gun) && g.is_item_non_playable(gun));
    assert_eq!(n(&mut g, "IsPlayable TestGun"), 0.0);
    assert_eq!(n(&mut g, "LNIsPlayable TestGun"), 0.0);
    assert!(!sorted(&mut g, "player.GetEquippedItems").contains(&f64::from(GUN)));
    assert!(sorted(&mut g, "player.GetEquippedItems 32").contains(&f64::from(GUN)), "explicit slot mask still includes non-playable weapon");
    assert_eq!(sorted(&mut g, "player.GetAllItemRefs 40 1").len(), 0, "noNonPlayable filters weapon out");

    // LNSetIsPlayable 1 clears bit 7 and stays in sync with WeaponHasFlag 7 and GetWeaponFlags1.
    q(&mut g, "LNSetIsPlayable 1 TestGun");
    assert!(g.is_item_playable(gun));
    assert_eq!(n(&mut g, "WeaponHasFlag 7 TestGun"), 0.0);
    assert_eq!(n(&mut g, "IsPlayable TestGun"), 1.0);
    assert!(sorted(&mut g, "player.GetEquippedItems").contains(&f64::from(GUN)));
    assert_eq!(sorted(&mut g, "player.GetAllItemRefs 40 1").len(), 1);

    // SetIsPlayable on a WEAP remains a no-op (observed: nvse-fourth).
    q(&mut g, "SetIsPlayable 0 TestGun");
    assert_eq!(n(&mut g, "IsPlayable TestGun"), 1.0);

    // TempCloneForm inherits original flags1 and isolates clone-specific edits.
    let clone_inherited = match q(&mut g, "SetFunctionValue (TempCloneForm TestGun)") {
        XVal::Ref(r) => FormId(r),
        v => panic!("TempCloneForm: {v:?}"),
    };
    assert_eq!(g.weapon_flags1_override(clone_inherited), g.weapon_flags1_override(gun));
    let clone_edited = match q(&mut g, "ref c\nlet c := TempCloneForm TestGun\nSetWeaponFlag 7 1 c\nSetFunctionValue c") {
        XVal::Ref(r) => FormId(r),
        v => panic!("TempCloneForm edit: {v:?}"),
    };
    assert!(!g.is_item_playable(clone_edited));
    assert!(g.is_item_playable(gun), "clone edit does not mutate original");

    // Non-weapon forms are refused without polluting field_overrides.
    q(&mut g, "SetWeaponFlag 1 1 TestHat");
    assert_eq!(n(&mut g, "WeaponHasFlag 1 TestHat"), 0.0);
    assert_eq!(g.weapon_flags1_override(FormId(HAT)), None);
    assert_eq!(g.weapon_flags1(FormId(HAT)), None);
    assert_eq!(g.num_field(FormId(HAT), "DNAM", "flags1"), 0.0);
}

#[test]
fn is_base_form_accepts_temp_clone_forms_and_rejects_placed_refs() {
    let f = fixture();
    let mut g = game(&f);
    dressed(&mut g);
    // Authored base forms return 1.
    assert_eq!(n(&mut g, "IsBaseForm TestGun"), 1.0);
    assert_eq!(n(&mut g, "IsBaseForm TestList"), 1.0);
    assert_eq!(n(&mut g, "IsBaseForm TestFactionA"), 1.0);
    // TempCloneForm (0xFF base form) returns 1.
    assert_eq!(
        q(&mut g, "ref c\nlet c := TempCloneForm TestGun\nSetFunctionValue (IsBaseForm c)"),
        XVal::Num(1.0)
    );
    // Placed player ref, PlaceAtMe 0xFF ref, and temp inventory ref return 0.
    assert_eq!(n(&mut g, "IsBaseForm player"), 0.0);
    assert_eq!(
        q(&mut g, "ref r\nlet r := player.PlaceAtMe TestGun 1\nSetFunctionValue (IsBaseForm r)"),
        XVal::Num(0.0)
    );
    assert_eq!(
        q(&mut g, "ref ir\nlet ir := player.GetEquippedWeaponRef\nSetFunctionValue (IsBaseForm ir)"),
        XVal::Num(0.0)
    );
}

#[test]
fn aux_timer_stop_handler_defers_to_next_frame_and_passes_null_self() {
    let f = fixture();
    let mut g = game(&f);
    // Register Stop and Start handlers and trigger same-frame: neither should fire yet.
    q(
        &mut g,
        r#"ref cbStop
ref cbStart
let cbStop := (Begin Function {string_var nm, ref owner}
  player.AuxVarSetFlt "*stop_n" ((player.AuxVarGetFlt "*stop_n") + 1)
  if eval (GetSelfAlt == 0) && (owner == player)
    player.AuxVarSetFlt "*stop_ok" ((player.AuxVarGetFlt "*stop_ok") + 1)
  endif
End)
let cbStart := (Begin Function {string_var nm, ref owner}
  player.AuxVarSetFlt "*start_n" ((player.AuxVarGetFlt "*start_n") + 1)
  if eval (GetSelfAlt == 0) && (owner == player)
    player.AuxVarSetFlt "*start_ok" ((player.AuxVarGetFlt "*start_ok") + 1)
  endif
End)
player.AuxVarSetRef "*cb_stop" cbStop
player.AuxVarSetRef "*cb_start" cbStart
SetOnAuxTimerStopHandler 1 cbStop "*w6t" player
SetOnAuxTimerStartHandler 1 cbStart "*w6t" player
AuxTimerStart "*w6t" 10 0 player
AuxTimerStop "*w6t" 1 player"#,
    );
    assert_eq!(n(&mut g, "player.AuxVarGetFlt \"*stop_n\""), 0.0, "stop handler must defer to next frame");
    assert_eq!(n(&mut g, "player.AuxVarGetFlt \"*start_n\""), 0.0, "start handler must defer to next frame");

    // Next frame: both handlers are now active; AuxTimerStart & AuxTimerStop 1 fire with GetSelfAlt == 0.
    g.tick();
    q(
        &mut g,
        r#"AuxTimerStart "*w6t" 10 0 player
AuxTimerStop "*w6t" 0 player
AuxTimerStart "*w6t" 10 0 player
AuxTimerStop "*w6t" 1 player"#,
    );
    assert_eq!(n(&mut g, "player.AuxVarGetFlt \"*stop_n\""), 1.0);
    assert_eq!(n(&mut g, "player.AuxVarGetFlt \"*stop_ok\""), 1.0);
    assert_eq!(n(&mut g, "player.AuxVarGetFlt \"*start_n\""), 2.0);
    assert_eq!(n(&mut g, "player.AuxVarGetFlt \"*start_ok\""), 2.0);

    // Same-frame removal still lets stop fire in this frame, then stops firing after next tick.
    q(
        &mut g,
        r#"ref cbStop
let cbStop := player.AuxVarGetRef "*cb_stop"
SetOnAuxTimerStopHandler 0 cbStop "*w6t" player
AuxTimerStart "*w6t" 10 0 player
AuxTimerStop "*w6t" 1 player"#,
    );
    assert_eq!(n(&mut g, "player.AuxVarGetFlt \"*stop_n\""), 2.0, "removal takes effect next frame");
    g.tick();
    q(
        &mut g,
        r#"AuxTimerStart "*w6t" 10 0 player
AuxTimerStop "*w6t" 1 player"#,
    );
    assert_eq!(n(&mut g, "player.AuxVarGetFlt \"*stop_n\""), 2.0, "removed stop handler silent after tick");
}
