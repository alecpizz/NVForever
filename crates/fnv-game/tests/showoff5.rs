//! ShowOff NVSE batch 5 (#1010; docs/spec-notes/ext-showoff.md §8) on a synthetic plugin
//! written for the test (no game data, made-up names): the level-up menu's asks, the
//! pause menu ask, live explosions, the shot switches, fast travel, AP costs and
//! RemoveAllItemsShowOff's flags.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{menus, Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const GUN: u32 = 0x800;
const PIP: u32 = 0x801;
const VEST: u32 = 0x802;
const RIFLE: u32 = 0x803;
const MOD_MAGS: u32 = 0x804;
const MOD_SCOPE: u32 = 0x805;
const MOD_SILENCER: u32 = 0x806;
const VENDOR_BOX: u32 = 0x807;
const VENDOR_REF: u32 = 0x808;
const BOOM: u32 = 0x810;
const MELEE: u32 = 0x811;
const TEST_PERK: u32 = 0x812;
const TEST_CHEM: u32 = 0x813;
const UMON: u32 = 0x14F;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

/// WEAP DNAM: the animation type first (3 Pistol - Ballistic) [documented: schemas/fnv
/// objects.json WeaponDNAM]; the rest left zero.
fn weap(id: u32, edid: &str, anim: u32) -> Vec<u8> {
    let data = [0i32.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), 1.0f32.to_le_bytes().to_vec(), vec![0; 3]].concat();
    let mut dnam = anim.to_le_bytes().to_vec();
    dnam.resize(204, 0);
    record(b"WEAP", 0, id, &[sub(b"EDID", z(edid)), sub(b"DATA", data), sub(b"DNAM", dnam)])
}

fn modded_weap(id: u32, edid: &str, value: i32, mods: [u32; 3]) -> Vec<u8> {
    let data = [value.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), 5.0f32.to_le_bytes().to_vec(), vec![0; 3]].concat();
    let mut dnam = 5u32.to_le_bytes().to_vec();
    dnam.resize(204, 0);
    record(
        b"WEAP",
        0,
        id,
        &[
            sub(b"EDID", z(edid)),
            sub(b"WMI1", mods[0].to_le_bytes().to_vec()),
            sub(b"WMI2", mods[1].to_le_bytes().to_vec()),
            sub(b"WMI3", mods[2].to_le_bytes().to_vec()),
            sub(b"DATA", data),
            sub(b"DNAM", dnam),
        ],
    )
}

fn imod(id: u32, edid: &str, value: i32) -> Vec<u8> {
    let data = [value.to_le_bytes().to_vec(), 0.0f32.to_le_bytes().to_vec()].concat();
    record(b"IMOD", 0, id, &[sub(b"EDID", z(edid)), sub(b"DATA", data)])
}

fn armo(id: u32, edid: &str, biped: u32, general: u8) -> Vec<u8> {
    let bmdt = [biped.to_le_bytes().to_vec(), vec![general, 0, 0, 0]].concat();
    let data = [5i32.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), 1.0f32.to_le_bytes().to_vec()].concat();
    record(b"ARMO", 0, id, &[sub(b"EDID", z(edid)), sub(b"BMDT", bmdt), sub(b"DATA", data)])
}

fn fixture() -> Fixture {
    let expl = record(b"EXPL", 0, BOOM, &[sub(b"EDID", z("TestBoom"))]);
    let cont = record(b"CONT", 0, VENDOR_BOX, &[sub(b"EDID", z("VendorStock"))]);
    let refr = record(b"REFR", 0, VENDOR_REF, &[sub(b"EDID", z("VendorStockRef")), sub(b"NAME", VENDOR_BOX.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let cell = record(b"CELL", 0, 0x820, &[sub(b"EDID", z("ShopCell"))]);
    let children = group(0x820u32.to_le_bytes(), 6, &group(0x820u32.to_le_bytes(), 8, &refr));
    let cells = top(b"CELL", &group(0u32.to_le_bytes(), 2, &group(0u32.to_le_bytes(), 3, &[cell, children].concat())));
    let umon_data = [
        0u32.to_le_bytes().as_slice(),
        &0.0f32.to_le_bytes(),
        &0u32.to_le_bytes(),
        &[0xFF; 8],
        &[0; 4],
        &[0; 4],
        &1.0f32.to_le_bytes(),
        &[0; 24],
        &[0; 8],
        &0u32.to_le_bytes(),
        &(-1i32).to_le_bytes(),
    ]
    .concat();
    let mgef = record(b"MGEF", 0, UMON, &[sub(b"EDID", z("UsageMonitor")), sub(b"DATA", umon_data)]);
    let chem_enit = [10u32.to_le_bytes().as_slice(), &[0, 0, 0, 0], &0u32.to_le_bytes(), &0.0f32.to_le_bytes(), &0u32.to_le_bytes()].concat();
    let chem_efit = [30u32.to_le_bytes().as_slice(), &0u32.to_le_bytes(), &60u32.to_le_bytes(), &0u32.to_le_bytes(), &(-1i32).to_le_bytes()].concat();
    let chem = record(
        b"ALCH",
        0,
        TEST_CHEM,
        &[
            sub(b"EDID", z("TestChem")),
            sub(b"DATA", 0.5f32.to_le_bytes().to_vec()),
            sub(b"ENIT", chem_enit),
            sub(b"EFID", UMON.to_le_bytes().to_vec()),
            sub(b"EFIT", chem_efit),
        ],
    );
    let perk = record(b"PERK", 0, TEST_PERK, &[sub(b"EDID", z("TestPerk")), sub(b"FULL", z("Test Perk")), sub(b"DATA", vec![0, 1, 1, 1, 0])]);
    let bytes = [
        tes4(),
        top(b"MGEF", &mgef),
        top(b"PERK", &perk),
        top(b"WEAP", &[weap(GUN, "TestPistol", 3), weap(MELEE, "TestKnife", 1), modded_weap(RIFLE, "TestRifle", 75, [MOD_MAGS, MOD_SCOPE, MOD_SILENCER])].concat()),
        top(b"IMOD", &[imod(MOD_MAGS, "ModMags", 75), imod(MOD_SCOPE, "ModScope", 125), imod(MOD_SILENCER, "ModSilencer", 100)].concat()),
        top(b"ARMO", &[armo(PIP, "TestPip", 1 << 6, 0x40), armo(VEST, "TestVest", 1 << 2, 0)].concat()),
        top(b"ALCH", &chem),
        top(b"CONT", &cont),
        top(b"EXPL", &expl),
        top(b"NPC_", &record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))])),
        cells,
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "ShowOff5Test.esm".into()).unwrap()];
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

#[test]
fn the_level_up_menu_and_the_pause_menu_take_the_asks() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetLevelUpMenuCurrentPage"), -1.0, "closed");
    assert_eq!(n(&mut g, "GetLevelUpMenuPoints"), -1.0);
    assert_eq!(n(&mut g, "SetLevelUpMenuCurrentPage 1"), 0.0, "closed");
    assert_eq!(n(&mut g, "SetLevelUpMenuCanExitEarly 1"), 0.0);
    // ShowSkillMenu 5: the skills page alone, five points [documented].
    assert_eq!(n(&mut g, "ShowSkillMenu 5"), 1.0);
    assert!(g.menu_open(menus::LEVEL_UP));
    let ask = g.showoff.b5.levelup.open_as.clone().unwrap();
    assert!(!ask.perks_only);
    assert_eq!(ask.count, Some(5));
    // Read at once, before the engine's first frame [observed 2026-11-08:
    // showoff5.levelup.skillmenu].
    assert_eq!((n(&mut g, "GetLevelUpMenuPage"), n(&mut g, "GetLevelUpMenuPoints"), n(&mut g, "GetLevelUpMenuPoints 1")), (0.0, 5.0, 0.0));
    // The engine reports the page and points each frame; here by hand.
    g.showoff.b5.levelup.page = Some(0);
    g.showoff.b5.levelup.points = [5, 2, 0, 0];
    assert_eq!(n(&mut g, "GetLevelUpMenuPage"), 0.0);
    assert_eq!(n(&mut g, "GetLevelUpMenuPoints 0 1"), 2.0);
    assert_eq!(n(&mut g, "SetLevelUpMenuCurrentPage 0"), 0.0, "already there");
    assert_eq!(n(&mut g, "SetLevelUpMenuPage 2"), 1.0);
    assert_eq!(g.showoff.b5.levelup.page_request, Some(2));
    assert_eq!(n(&mut g, "SetLevelUpMenuCanExitEarly 1"), 1.0);
    assert!(g.showoff.b5.levelup.can_exit_early);
    // With a menu open, no second menu (see docs/spec-notes/ext-showoff.md).
    assert_eq!(n(&mut g, "ShowSkillMenu 5"), 0.0);
    g.close_menu();
    // ShowSkillMenu 0 0: no points and not to be shown anyway.
    assert_eq!(n(&mut g, "ShowSkillMenu 0 0"), 0.0);
    let asks = g.showoff.b5.pause_asks;
    q(&mut g, "ShowPauseMenu 3");
    assert_eq!((g.showoff.b5.pause_asks, g.showoff.b5.pause_mode), (asks + 1, 3));
}

#[test]
fn live_explosions_and_the_shot_switches() {
    let f = fixture();
    let mut g = game(&f);
    let r = g.explosion_placed(FormId(BOOM), Some(FormId::PLAYER_REF), [0.0; 3], 256.0).expect("placed");
    let on = |g: &mut Game, body: &str| {
        let ctx = RunCtx { self_ref: Some(r), ..RunCtx::default() };
        match g.run_query(&format!("Begin Function {{ }}\nSetFunctionValue ({body})\nEnd\n"), ctx).unwrap() {
            XVal::Num(x) => x,
            XVal::Ref(x) => f64::from(x),
            v => panic!("{v:?}"),
        }
    };
    assert_eq!(on(&mut g, "GetExplosionRefRadius"), 256.0);
    assert_eq!(n(&mut g, "player.GetExplosionRefRadius"), -1.0, "no explosion [observed]");
    assert_eq!(on(&mut g, "GetExplosionRefSource"), f64::from(FormId::PLAYER_REF.0));
    assert_eq!(on(&mut g, "SetExplosionRefRadius 100.5"), 1.0);
    assert_eq!(g.explosion_radius(r), Some(100.5));
    assert_eq!(n(&mut g, "SetForceDrawHitscanProjectiles 1"), 0.0);
    assert_eq!(n(&mut g, "SetForceDrawHitscanProjectiles"), 1.0, "-1 reads");
    assert_eq!(n(&mut g, "SetProjectileTracerChanceOverride 0"), -1.0);
    assert_eq!(n(&mut g, "SetProjectileTracerChanceOverride"), 0.0, "-2 changes nothing");
    assert_eq!(g.showoff.b5.tracer_override, Some(false));
    assert_eq!(n(&mut g, "SetProjectileTracerChanceOverride -1"), 0.0);
    assert_eq!(g.showoff.b5.tracer_override, None);
}

#[test]
fn travel_ap_and_removing_items() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetPCHasScriptedFastTravelOverride"), 0.0);
    q(&mut g, "EnableFastTravel 0");
    assert_eq!(n(&mut g, "GetPCHasScriptedFastTravelOverride"), 1.0);
    assert_eq!(n(&mut g, "GetPCCanFastTravel"), 0.0);
    // The pistol's AP: fActionPointsAttackPistol, 20 without the setting.
    assert_eq!(n(&mut g, "GetCalculatedAPCost 0 TestPistol"), 20.0);
    assert_eq!(n(&mut g, "GetCalculatedAPCost 1 TestPistol") + n(&mut g, "GetCalculatedAPCost 2 TestPistol"), 0.0, "a gun's 1 and 2 [observed]");
    assert_eq!(s(&mut g, "string_var t\nlet t := ToANSIChar 16\nSetFunctionValue t"), "");
    // RemoveAllItemsShowOff: the non-playable item stays unless flag 8.
    let inv = g.world.inventory_mut(FormId::PLAYER_REF).unwrap();
    inv.add(FormId(PIP), 1);
    inv.add(FormId(VEST), 1);
    assert_eq!(n(&mut g, "player.RemoveAllItemsShowOff"), 1.0);
    let count = |g: &mut Game, i: u32| g.world.get(FormId::PLAYER_REF).unwrap().inventory.as_ref().map_or(0, |inv| inv.count(FormId(i)));
    assert_eq!((count(&mut g, PIP), count(&mut g, VEST)), (1, 0));
    assert_eq!(n(&mut g, "player.RemoveAllItemsShowOff 8"), 1.0);
    assert_eq!(count(&mut g, PIP), 0);
}

fn s(g: &mut Game, body: &str) -> String {
    match q(g, body) {
        XVal::Str(t) => t,
        v => panic!("{body}: {v:?}"),
    }
}

#[test]
fn calculated_item_value_includes_weapon_mods_and_live_barter_multipliers() {
    use fnv_game::nvse_jip12::BarterView;
    use fnv_sim::inventory::Extra;

    let f = fixture();
    let mut g = game(&f);

    // Add rifle to player and merchant container, and obtain their script inventory refs.
    g.world.inventory_mut(FormId::PLAYER_REF).unwrap().add_extra(FormId(RIFLE), 1, Extra { condition: Some(1.0), mods: 0, ..Extra::default() });
    g.world.inventory_mut(FormId(VENDOR_REF)).unwrap().add_extra(FormId(RIFLE), 1, Extra { condition: Some(1.0), mods: 7, ..Extra::default() });
    let inv_ref = |g: &mut Game, holder: &str| -> FormId {
        match q(g, &format!("array_var a\nlet a := {holder}.GetInvRefsForItem TestRifle\nSetFunctionValue a[0]")) {
            XVal::Ref(r) => FormId(r),
            v => panic!("expected ref for {holder}: {v:?}"),
        }
    };
    let player_rifle = inv_ref(&mut g, "player");
    let vendor_rifle = inv_ref(&mut g, "VendorStockRef");
    let on = |g: &mut Game, target: FormId, expr: &str| {
        let ctx = RunCtx { self_ref: Some(target), ..RunCtx::default() };
        match g.run_query(&format!("Begin Function {{ }}\nSetFunctionValue ({expr})\nEnd\n"), ctx).unwrap() {
            XVal::Num(x) => x,
            v => panic!("{expr}: {v:?}"),
        }
    };

    // Base rifle (value 75) at full condition: 75 * 0.0316 * 10^1.5 = 74.946 -> 75.
    // Attached mods (75, 125, 100) add their full IMOD values without condition scaling
    // [observed: o26_value, docs/spec-notes/weapon-mods.md §6].
    assert_eq!(on(&mut g, player_rifle, "GetCalculatedItemValue"), 75.0);
    for (flags, expected) in [(1, 150.0), (2, 200.0), (4, 175.0), (3, 275.0), (7, 375.0)] {
        assert_eq!(on(&mut g, player_rifle, &format!("SetWeaponRefModFlags {flags}")), 1.0);
        assert_eq!(on(&mut g, player_rifle, "GetCalculatedItemValue"), expected, "mods={flags}");
    }

    // Condition scales only the weapon's own value; attached mods keep their full +300.
    // 50%: 75 * 0.0316 * 5^1.5 = 26.497 -> 26 / 326.
    // 25%: 75 * 0.0316 * 2.5^1.5 = 9.368 -> 9 / 309.
    // 75%: 75 * 0.0316 * 7.5^1.5 = 48.679 -> 49 / 349.
    for (cond, bare, modded) in [(0.5_f64, 26.0, 326.0), (0.25, 9.0, 309.0), (0.75, 49.0, 349.0)] {
        assert_eq!(on(&mut g, player_rifle, &format!("SetItemRefCurrentHealth {cond} 1")), 1.0);
        assert_eq!(on(&mut g, player_rifle, "SetWeaponRefModFlags 0"), 1.0);
        assert_eq!(on(&mut g, player_rifle, "GetCalculatedItemValue"), bare, "bare cond={cond}");
        assert_eq!(on(&mut g, player_rifle, "SetWeaponRefModFlags 7"), 1.0);
        assert_eq!(on(&mut g, player_rifle, "GetCalculatedItemValue"), modded, "modded cond={cond}");
    }

    // Fresh 10mm pistol (Value 750) at full condition: 750 * 0.0316 * 10^1.5 = 749.46 -> 749
    // [observed 2026-11-08: showoff5.itemvalue.pistol].
    q(&mut g, "TestPistol.SetItemValue 750");
    g.world.inventory_mut(FormId::PLAYER_REF).unwrap().add_extra(FormId(GUN), 1, Extra { condition: Some(1.0), mods: 0, ..Extra::default() });
    let player_pistol = match q(&mut g, "array_var a\nlet a := player.GetInvRefsForItem TestPistol\nSetFunctionValue a[0]") {
        XVal::Ref(r) => FormId(r),
        v => panic!("expected ref for TestPistol: {v:?}"),
    };
    assert_eq!(on(&mut g, player_pistol, "GetCalculatedItemValue"), 749.0);

    // World-placed/dropped reference reads RefState.extra (condition + mods).
    g.world.inventory_mut(FormId(VENDOR_REF)).unwrap().add_extra(FormId(RIFLE), 1, Extra { condition: Some(0.5), mods: 7, ..Extra::default() });
    let dropped = g.world.drop_item_at(FormId(VENDOR_REF), FormId(RIFLE), 1, fnv_sim::inventory::Pick::row(Some(0.5), 7)).unwrap()[0];
    assert_eq!(on(&mut g, dropped, "GetCalculatedItemValue"), 326.0);

    // SetItemValue overrides base item value while preserving mod values.
    q(&mut g, "TestRifle.SetItemValue 100");
    // 50% of 100: 100 * 0.0316 * 5^1.5 = 35.33 -> 35 + 300 = 335.
    assert_eq!(on(&mut g, dropped, "GetCalculatedItemValue"), 335.0);
    q(&mut g, "TestRifle.SetItemValue 75");
    assert_eq!(on(&mut g, dropped, "GetCalculatedItemValue"), 326.0);

    // Barter closed: accountForBarter=1 does not apply barter multiplier.
    assert_eq!(on(&mut g, player_rifle, "SetItemRefCurrentHealth 0.5 1"), 1.0);
    assert_eq!(on(&mut g, player_rifle, "GetCalculatedItemValue 1"), 326.0);

    // Barter open with live BarterView: honours BarterView::mult and SetBarterPriceMult
    // overrides for both player sell (side 1) and merchant buy (side 0).
    g.open_menu(menus::BARTER);
    g.set_barter_view(Some(BarterView {
        merchant_caps: 500,
        stock: FormId(VENDOR_REF),
        bought: Vec::new(),
        sold: Vec::new(),
        mult: [1.4, 0.5],
    }));
    // Player selling: 326.497 * 0.5 = 163.249 -> 163.
    assert_eq!(on(&mut g, player_rifle, "GetCalculatedItemValue 1"), 163.0);
    // Merchant selling to player: 374.946 * 1.4 = 524.924 -> 525.
    assert_eq!(on(&mut g, vendor_rifle, "GetCalculatedItemValue 1"), 525.0);

    // SetBarterPriceMult overrides take precedence over BarterView::mult.
    q(&mut g, "SetBarterPriceMult 1 0.8\nSetBarterPriceMult 0 1.25");
    // Player selling: 326.497 * 0.8 = 261.198 -> 261.
    assert_eq!(on(&mut g, player_rifle, "GetCalculatedItemValue 1"), 261.0);
    // Merchant selling to player: 374.946 * 1.25 = 468.682 -> 469.
    assert_eq!(on(&mut g, vendor_rifle, "GetCalculatedItemValue 1"), 469.0);

    // Closing BarterView falls back to GMST Barter skill formula while menu stays open.
    g.set_barter_view(None);
    assert!(on(&mut g, player_rifle, "GetCalculatedItemValue 1") < 326.0);
    assert!(on(&mut g, vendor_rifle, "GetCalculatedItemValue 1") > 375.0);

    // Non-item returns -1; Stewie gate is at 960 (#3183).
    assert_eq!(n(&mut g, "player.GetCalculatedItemValue"), -1.0);
    assert_eq!(n(&mut g, "GetPluginVersion \"lStewieAl's Tweaks\""), 960.0);
}

#[test]
fn show_perk_menu_force_recoil_and_spell_usage_follow_wave4_observations() {
    let f = fixture();
    let mut g = game(&f);

    // ShowPerkMenu returns 1 even while SkillMenu is already open [observed:
    // so_36_show_perk_menu_while_open: 1,1,1], and GetLevelUpMenuPoints 1 0 reads 0
    // same-frame after ShowPerkMenu 2 [observed: so_26_levelup_perk_menu_points: 1,0,0,1,-1].
    assert_eq!(n(&mut g, "ShowSkillMenu 3"), 1.0);
    assert_eq!(n(&mut g, "ShowPerkMenu 2"), 1.0);
    assert_eq!(n(&mut g, "GetLevelUpMenuPoints 1 0"), 0.0);
    assert_eq!(n(&mut g, "GetLevelUpMenuPoints 1 1"), 0.0);
    let ask = g.showoff.b5.levelup.open_as.clone().unwrap();
    assert!(ask.perks_only);
    assert_eq!(ask.count, Some(2));
    assert_eq!(n(&mut g, "SetLevelUpMenuCurrentPage 2"), 1.0);
    g.close_menu();
    assert_eq!(n(&mut g, "GetLevelUpMenuPoints 1 0"), -1.0);

    // ForceRecoilAnim returns 1 unarmed/melee and 0 with a ranged gun equipped
    // [observed: so_33_force_recoil_unarmed_vs_gun: 1,0,0].
    assert_eq!(n(&mut g, "player.ForceRecoilAnim"), 1.0);
    assert_eq!(n(&mut g, "player.ForceHitReaction"), 0.0);
    q(&mut g, "player.AddItem TestPistol 1 1\nplayer.EquipItem TestPistol 0 1");
    assert_eq!(n(&mut g, "player.ForceRecoilAnim"), 0.0);
    q(&mut g, "player.AddItem TestKnife 1 1\nplayer.EquipItem TestKnife 0 1");
    assert_eq!(n(&mut g, "player.ForceRecoilAnim"), 1.0);
    q(&mut g, "player.UnequipItem TestKnife 0 1");
    assert_eq!(n(&mut g, "player.ForceRecoilAnim"), 1.0);

    // GetSpellUsageNumEx returns 0 before ingestion and usage monitor magnitude (30)
    // after 1 use [observed: so_35_spell_usage_num_ex: 0,30].
    assert_eq!(n(&mut g, "player.GetSpellUsageNumEx TestChem"), 0.0);
    q(&mut g, "player.AddItem TestChem 1 1\nplayer.EquipItem TestChem 0 1");
    assert_eq!(n(&mut g, "player.GetSpellUsageNumEx TestChem"), 30.0);
}

#[test]
fn show_perk_menu_menu_guard_allows_none_and_levelup_only() {
    let f = fixture();
    let mut g = game(&f);

    // (a) Succeeds when no menu is open.
    assert_eq!(g.menu, None);
    assert_eq!(n(&mut g, "ShowPerkMenu 0"), 1.0);
    assert_eq!(g.menu, Some(menus::LEVEL_UP));
    let ask = g.showoff.b5.levelup.open_as.clone().unwrap();
    assert!(ask.perks_only);
    assert_eq!(ask.count, Some(0));
    g.close_menu();
    g.showoff.b5.levelup.open_as = None;

    // (b) Succeeds while SkillMenu or LevelUpMenu (both menus::LEVEL_UP) is open.
    assert_eq!(n(&mut g, "ShowSkillMenu 3"), 1.0);
    assert_eq!(g.menu, Some(menus::LEVEL_UP));
    assert_eq!(n(&mut g, "ShowPerkMenu 1"), 1.0);
    assert_eq!(g.menu, Some(menus::LEVEL_UP));
    let ask = g.showoff.b5.levelup.open_as.clone().unwrap();
    assert!(ask.perks_only);
    assert_eq!(ask.count, Some(1));
    g.close_menu();
    g.showoff.b5.levelup.open_as = None;

    g.open_menu(menus::LEVEL_UP);
    assert_eq!(n(&mut g, "ShowPerkMenu 2"), 1.0);
    assert_eq!(g.menu, Some(menus::LEVEL_UP));
    assert_eq!(g.showoff.b5.levelup.open_as.as_ref().and_then(|a| a.count), Some(2));
    g.close_menu();
    g.showoff.b5.levelup.open_as = None;

    // (c) Refuses without replacing the open menu when Container, Pip-Boy, or Barter is open,
    // preserving the active menu and its close event.
    for other in [fnv_game::runner::CONTAINER_MENU, menus::INVENTORY, menus::STATS, menus::MAP, menus::BARTER] {
        g.events.clear();
        g.open_menu(other);
        assert_eq!(g.menu, Some(other));
        assert_eq!(n(&mut g, "ShowPerkMenu 1"), 0.0, "must refuse while menu {other} is open");
        assert_eq!(g.menu, Some(other), "menu {other} must not be overwritten");
        assert!(g.showoff.b5.levelup.open_as.is_none(), "levelup.open_as must stay untouched");
        assert!(
            !g.events.iter().any(|e| matches!(e.kind, fnv_game::trace::EventKind::MenuOpened { menu: menus::LEVEL_UP })),
            "must not emit MenuOpened(LEVEL_UP) while menu {other} is open"
        );
        g.close_menu();
        assert!(
            g.events.iter().any(|e| matches!(e.kind, fnv_game::trace::EventKind::MenuClosed { menu } if menu == other)),
            "closing menu {other} must still emit MenuClosed"
        );
    }
}
