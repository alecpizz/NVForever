//! ShowOff NVSE batch 3 (#1010; docs/spec-notes/ext-showoff.md §6) on a synthetic plugin
//! written for the test (no game data, made-up names): the equipped-item family, the
//! seeded randomizer, easing by code, temporary faction relations, the player's flags and
//! the aux timers' countdowns.

mod common;

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
    // Level 1, the menu closed: the points of level 2, a perk level
    // (iLevelsPerPerk 2).
    assert_eq!(n(&mut g, "GetCalculatedPerkPoints"), 1.0);
    assert_eq!(n(&mut g, "GetCalculatedPerkPoints 2"), 0.0);
    assert!(n(&mut g, "GetCalculatedSkillPoints") >= 10.0);
    assert_eq!(n(&mut g, "player.GetRadiationExtraData"), 0.0);
    assert_eq!(n(&mut g, "player.SetRadiationExtraData 20"), 1.0);
    assert_eq!(n(&mut g, "player.GetRadiationExtraData"), 20.0);
}
