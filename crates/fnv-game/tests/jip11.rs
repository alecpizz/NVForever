//! JIP LN NVSE batch 11 (#1006; docs/spec-notes/jip-actors-11.md) on a synthetic plugin:
//! the actor functions (detection, traits, base values, effects, equipped items, armour,
//! base faction ranks, the unarmed effect, the dead) and the engine channels (breath,
//! falls, armour hits, the weapon condition penalty).

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL: u32 = 0x200;
const NPC: u32 = 0x900;
const MARK: u32 = 0x901;
const FACT: u32 = 0x902;
const OTHER_FACT: u32 = 0x903;
const MGEF_A: u32 = 0x904;
const MGEF_B: u32 = 0x905;
const TWO_SPELL: u32 = 0x906;
const PUNCH: u32 = 0x907;
const KICK: u32 = 0x908;
const HAT: u32 = 0x909;
const HELMET: u32 = 0x90A;
const GLOVES: u32 = 0x90B;
const HAT_REF: u32 = 0x90C;
const ARMOR_LIST: u32 = 0x90D;

const HEAD: u32 = 1;
const HAND: u32 = 1 << 3;
const HAT_SLOT: u32 = 1 << 10;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn placed(sig: &[u8; 4], fid: u32, edid: &str, base: u32, pos: [f32; 3]) -> Vec<u8> {
    let data = [f32b(pos[0]), f32b(pos[1]), f32b(pos[2]), vec![0; 12]].concat();
    record(sig, 0, fid, &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn mgef(fid: u32, edid: &str) -> Vec<u8> {
    let data = [u32s(&[0]), f32b(0.0), u32s(&[0]), vec![0xFF; 8], vec![0; 4], vec![0; 4], f32b(1.0), vec![0; 24], vec![0; 8], u32s(&[0]), (-1i32).to_le_bytes().to_vec()].concat();
    record(b"MGEF", 0, fid, &[sub(b"EDID", z(edid)), sub(b"DATA", data)])
}

/// EFID + EFIT (magnitude, area, duration, type Self, actor value).
fn effect(base: u32, magnitude: u32) -> Vec<fnv_esm::raw::Subrecord> {
    vec![sub(b"EFID", base.to_le_bytes().to_vec()), sub(b"EFIT", [u32s(&[magnitude, 0, 0, 0]), (-1i32).to_le_bytes().to_vec()].concat())]
}

/// ARMO in `slots` with DR and DT (DNAM: DR i16, 2 unused, DT f32, flags u16, 2 unused).
fn armo(fid: u32, edid: &str, slots: u32, dr: i16, dt: f32) -> Vec<u8> {
    let bmdt = [u32s(&[slots]), vec![0; 4]].concat();
    let data = [u32s(&[1, 100]), f32b(1.0)].concat();
    let dnam = [dr.to_le_bytes().to_vec(), vec![0; 2], f32b(dt), vec![0; 4]].concat();
    record(b"ARMO", 0, fid, &[sub(b"EDID", z(edid)), sub(b"BMDT", bmdt), sub(b"DATA", data), sub(b"DNAM", dnam)])
}

fn plugin_bytes() -> Vec<u8> {
    let npcs = [
        record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]),
        record(b"NPC_", 0, NPC, &[sub(b"EDID", z("TestNpc")), sub(b"SNAM", [u32s(&[FACT]), vec![2, 0, 0, 0]].concat()), sub(b"EITM", PUNCH.to_le_bytes().to_vec())]),
    ]
    .concat();
    let facts = [record(b"FACT", 0, FACT, &[sub(b"EDID", z("TestFaction"))]), record(b"FACT", 0, OTHER_FACT, &[sub(b"EDID", z("OtherFaction"))])].concat();
    let mgefs = [mgef(MGEF_A, "TestEffectA"), mgef(MGEF_B, "TestEffectB")].concat();
    let mut spel = vec![sub(b"EDID", z("TwoSpell")), sub(b"SPIT", [u32s(&[4, 0, 0]), vec![0; 4]].concat())];
    spel.extend(effect(MGEF_A, 5));
    spel.extend(effect(MGEF_B, 6));
    let spel = record(b"SPEL", 0, TWO_SPELL, &spel);
    let ench = |fid: u32, edid: &str| {
        let mut subs = vec![sub(b"EDID", z(edid)), sub(b"ENIT", [u32s(&[2, 0, 0]), vec![0; 4]].concat())];
        subs.extend(effect(MGEF_A, 1));
        record(b"ENCH", 0, fid, &subs)
    };
    let enchs = [ench(PUNCH, "TestPunch"), ench(KICK, "TestKick")].concat();
    let armos = [armo(HAT, "TestHat", HAT_SLOT, 10, 5.0), armo(HELMET, "TestHelmet", HAT_SLOT | HEAD, 20, 8.0), armo(GLOVES, "TestGloves", HAND, 2, 1.0)].concat();
    let flst = record(b"FLST", 0, ARMOR_LIST, &[sub(b"EDID", z("TestArmorList")), sub(b"LNAM", HELMET.to_le_bytes().to_vec()), sub(b"LNAM", HAT.to_le_bytes().to_vec())]);
    let dehy = record(b"DEHY", 0, 0x950, &[sub(b"EDID", z("TestDehydrationStage")), sub(b"DATA", u32s(&[200, TWO_SPELL]))]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    let refs = [placed(b"ACHR", MARK, "MarkRef", NPC, [100.0, 200.0, 0.0]), placed(b"REFR", HAT_REF, "HatRef", HAT, [0.0, 50.0, 0.0])].concat();
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs));
    [
        tes4(),
        top(b"FACT", &facts),
        top(b"MGEF", &mgefs),
        top(b"SPEL", &spel),
        top(b"ENCH", &enchs),
        top(b"NPC_", &npcs),
        top(b"ARMO", &armos),
        top(b"FLST", &flst),
        top(b"DEHY", &dehy),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat()))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jip11.esm".into()).unwrap()];
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
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() });
    g.player_entered(Some(FormId(CELL)));
    g
}

const VARS: &str = "float fX\nfloat fY\nfloat fZ\nfloat fR\nref rItem\nstring_var sOut\narray_var aList\narray_var aEl\n";

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("{VARS}Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

/// An array's forms as text; "none" for no array at all (`ar_Size` -1).
fn ids(g: &mut Game, expr: &str) -> String {
    match q(g, &format!("let aList := {expr}\nlet sOut := \"none\"\nif (ar_Size aList) >= 0\nlet sOut := \"\"\nforeach aEl <- aList\nlet sOut := sOut + (sv_Construct \"%i \" aEl[\"value\"])\nloop\nendif\nSetFunctionValue sOut")) {
        XVal::Str(s) => s.trim().to_string(),
        v => panic!("{expr}: {v:?}"),
    }
}

#[test]
fn actor_traits_and_detection() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "MarkRef.GetActorAlpha"), 1.0);
    q(&mut g, "MarkRef.SetActorAlpha 0.5");
    assert_eq!(n(&mut g, "MarkRef.GetActorAlpha"), 0.5);
    assert_eq!(n(&mut g, "MarkRef.GetActorGravityMult"), 0.0, "none set [observed]");
    q(&mut g, "MarkRef.SetActorGravityMult 2");
    assert_eq!(n(&mut g, "MarkRef.GetActorGravityMult"), 2.0);
    assert_eq!(n(&mut g, "MarkRef.GetActorLeveledList"), 0.0, "placed from an NPC, not a list");
    // Not measured: -100; not in combat: no array.
    assert_eq!(n(&mut g, "MarkRef.GetDetectionValue player"), -100.0);
    assert_eq!(ids(&mut g, "MarkRef.GetCombatAllies"), "none");
    assert_eq!(ids(&mut g, "MarkRef.GetCombatTargets"), "none");
    assert_eq!(ids(&mut g, "MarkRef.GetDetectedActors"), "none");
    // Forced, as a modifier, and given back [documented].
    q(&mut g, "player.ForceActorDetectionValue -100");
    assert_eq!(g.forced_detection(FormId::PLAYER_REF, 30.0), -100.0);
    q(&mut g, "player.ForceActorDetectionValue -15 1");
    assert_eq!(g.forced_detection(FormId::PLAYER_REF, 30.0), 15.0);
    q(&mut g, "player.ForceActorDetectionValue");
    assert_eq!(g.forced_detection(FormId::PLAYER_REF, 30.0), 30.0);
    assert_eq!(n(&mut g, "MarkRef.GetKillXP"), g.kill_xp(FormId(MARK)));
    assert_eq!((n(&mut g, "MarkRef.GetInterruptPackage"), n(&mut g, "MarkRef.GetActorMovementFlags"), n(&mut g, "MarkRef.GetFallTimeRemaining")), (0.0, 0.0, 0.0));
    // The base value of the reference and of its base form agree.
    let own = n(&mut g, "MarkRef.GetBaseActorValueAlt Strength");
    assert_eq!(n(&mut g, "GetBaseActorValueAlt Strength TestNpc"), own);
}

#[test]
fn effect_lists_and_active_effects() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "player.AddSpell TwoSpell");
    assert_eq!(ids(&mut g, "player.GetActiveEffects 2"), format!("{TWO_SPELL:08X}"));
    assert_eq!(ids(&mut g, "player.GetActiveEffects 4"), "none");
    // The entry goes; the last one cannot [documented].
    assert_eq!(n(&mut g, "RemoveNthEffect TwoSpell 0"), 1.0);
    assert_eq!(n(&mut g, "GetNumEffects TwoSpell"), 1.0);
    assert_eq!(n(&mut g, "RemoveNthEffect TwoSpell 0"), -1.0);
    assert_eq!(n(&mut g, "RemoveNthEffect TwoSpell 5"), -1.0);
    assert!(g.effect_removed(FormId(TWO_SPELL), 0) && !g.effect_removed(FormId(TWO_SPELL), 1));
}

#[test]
fn equipped_items_and_armour() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "player.AddItem TestHat 1\nplayer.EquipItem TestHat");
    assert_eq!(n(&mut g, "player.GetEquippedEx TestArmorList"), f64::from(HAT));
    assert_eq!(n(&mut g, "player.TestEquippedSlots TestHat"), 1.0, "the currently equipped armor itself overlaps [observed: w6d2p3_52]");
    assert_eq!(n(&mut g, "player.TestEquippedSlots TestHelmet"), 1.0, "the hat slot is taken");
    assert_eq!(n(&mut g, "player.TestEquippedSlots TestGloves"), 0.0);
    assert_eq!((n(&mut g, "player.IsEquippedWeaponScoped"), n(&mut g, "player.IsEquippedWeaponSilenced")), (0.0, 0.0), "no weapon");
    // A placed hat at full condition gives all its DR and DT.
    assert_eq!((n(&mut g, "HatRef.GetArmorEffectiveDR"), n(&mut g, "HatRef.GetArmorEffectiveDT")), (10.0, 5.0));
    // The last hit's DT [documented: from the OnHit block]; the engine tells it.
    g.note_armour_hit(FormId(MARK), 10.0, 5.0);
    assert_eq!(n(&mut g, "MarkRef.GetArmourPenetrated"), 1.0);
    g.note_armour_hit(FormId(MARK), 4.0, 5.0);
    assert_eq!(n(&mut g, "MarkRef.GetArmourPenetrated"), 0.0);
}

#[test]
fn base_forms_factions_and_unarmed() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetBaseFactionRank TestFaction TestNpc"), 2.0);
    q(&mut g, "SetBaseFactionRank TestFaction 3 TestNpc\nMarkRef.SetBaseFactionRank OtherFaction 0");
    assert_eq!((n(&mut g, "GetBaseFactionRank TestFaction TestNpc"), n(&mut g, "GetBaseFactionRank OtherFaction TestNpc")), (3.0, 0.0));
    assert_eq!(n(&mut g, "GetActorUnarmedEffect TestNpc"), f64::from(PUNCH));
    assert_eq!(n(&mut g, "SetActorUnarmedEffect TestKick TestNpc"), 1.0);
    assert_eq!(n(&mut g, "MarkRef.GetActorUnarmedEffect"), f64::from(KICK));
    assert_eq!(n(&mut g, "SetActorUnarmedEffect TestFaction TestNpc"), 0.0, "not an effect");
    // Hardcore stage effect: non-SPEL and explicit null 0 are no-ops; omitted arg clears
    // [observed: w6d2_30_actor_unarmed_and_hardcore_stage_effect].
    assert_eq!(n(&mut g, "GetHardcoreStageEffect TestDehydrationStage"), f64::from(TWO_SPELL));
    q(&mut g, "SetHardcoreStageEffect TestDehydrationStage TestHat");
    assert_eq!(n(&mut g, "GetHardcoreStageEffect TestDehydrationStage"), f64::from(TWO_SPELL));
    q(&mut g, "SetHardcoreStageEffect TestDehydrationStage 0");
    assert_eq!(n(&mut g, "GetHardcoreStageEffect TestDehydrationStage"), f64::from(TWO_SPELL));
    q(&mut g, "SetHardcoreStageEffect TestDehydrationStage");
    assert_eq!(n(&mut g, "GetHardcoreStageEffect TestDehydrationStage"), 0.0);
    q(&mut g, "SetHardcoreStageEffect TestDehydrationStage TwoSpell");
    assert_eq!(n(&mut g, "GetHardcoreStageEffect TestDehydrationStage"), f64::from(TWO_SPELL));
}

#[test]
fn engine_channels_and_the_dead() {
    let f = fixture();
    let mut g = game(&f);
    // Breath: a full breath before the engine tells it [observed: 10 on land]; the
    // engine tells the player's; a script's set under water goes back to it.
    let full = f64::from(g.breath_rules(FormId::PLAYER_REF).seconds);
    assert_eq!(n(&mut g, "player.GetActorDiveBreath"), full);
    assert_eq!(n(&mut g, "MarkRef.GetActorDiveBreath"), 0.0);
    q(&mut g, "player.SetActorDiveBreath 3");
    assert_eq!(g.sync_player_breath(7.5, true), None, "not under water when set");
    assert_eq!(n(&mut g, "player.GetActorDiveBreath"), 7.5);
    q(&mut g, "player.SetActorDiveBreath 3");
    assert_eq!(g.sync_player_breath(7.0, true), Some(3.0));
    q(&mut g, "MarkRef.ResetFallTime");
    assert_eq!(g.take_fall_resets(), [FormId(MARK)]);
    // The weapon condition penalty [documented: the page's example, 0.25 at 0 %].
    assert!((g.weapon_condition_mult(0.0) - (1.0 - 0.75 * 0.67)).abs() < 1e-6);
    q(&mut g, "SetConditionDamagePenalty 1");
    assert!((g.weapon_condition_mult(0.0) - 0.25).abs() < 1e-6);
    // The dead in the loaded cells go [documented].
    q(&mut g, "MarkRef.Kill");
    q(&mut g, "ClearDeadActors");
    assert_eq!(n(&mut g, "MarkRef.GetDisabled"), 1.0);
}

#[test]
fn condition_penalties_honor_stewie_tweaks_and_script_overrides() {
    use fnv_sim::inventory::Extra;
    let f = fixture();
    let mut g = game(&f);

    assert_eq!(n(&mut g, "GetPluginVersion \"lStewieAl's Tweaks\""), 960.0);

    // Equip a 20%-condition TestHat (DNAM DR 10, DT 5).
    let inv = g.world.inventory_mut(FormId::PLAYER_REF).unwrap();
    inv.add_extra(FormId(HAT), 1, Extra { condition: Some(0.2), ..Default::default() });
    assert!(inv.set_equipped(FormId(HAT)));

    let inv_eff = |g: &mut Game, fn_name: &str| -> f64 {
        let script = format!("array_var a\nref r\nlet a := player.GetInvRefsForItem TestHat\nlet r := a[0]\nSetFunctionValue (r.{fn_name})");
        match q(g, &script) {
            XVal::Num(x) => x,
            v => panic!("{fn_name}: {v:?}"),
        }
    };

    // Default penalty 1.0: share = 1 - (0.5 - 0.2) * 1.0 = 0.7 -> DR 7.0, DT 3.5.
    assert!((inv_eff(&mut g, "GetArmorEffectiveDR") - 7.0).abs() < 1e-6);
    assert!((inv_eff(&mut g, "GetArmorEffectiveDT") - 3.5).abs() < 1e-6);
    assert!((n(&mut g, "player.GetActorTotalArmorDR") - 7.0).abs() < 1e-6);
    assert!((n(&mut g, "player.GetActorTotalArmorDT") - 3.5).abs() < 1e-6);

    // Stewie's bCustomArmorConditionPenalty (0.0 -> no penalty, 2.0 -> doubled slope).
    g.tweaks.armor_condition_penalty = 0.0;
    assert_eq!(n(&mut g, "SetArmorConditionPenalty"), 0.0);
    assert!((inv_eff(&mut g, "GetArmorEffectiveDR") - 10.0).abs() < 1e-6);
    assert!((inv_eff(&mut g, "GetArmorEffectiveDT") - 5.0).abs() < 1e-6);
    assert!((n(&mut g, "player.GetActorTotalArmorDR") - 10.0).abs() < 1e-6);
    assert!((n(&mut g, "player.GetActorTotalArmorDT") - 5.0).abs() < 1e-6);

    g.tweaks.armor_condition_penalty = 2.0;
    assert_eq!(n(&mut g, "SetArmorConditionPenalty"), 2.0);
    assert!((inv_eff(&mut g, "GetArmorEffectiveDR") - 4.0).abs() < 1e-6);
    assert!((inv_eff(&mut g, "GetArmorEffectiveDT") - 2.0).abs() < 1e-6);
    assert!((n(&mut g, "player.GetActorTotalArmorDR") - 4.0).abs() < 1e-6);
    assert!((n(&mut g, "player.GetActorTotalArmorDT") - 2.0).abs() < 1e-6);

    // Explicit script SetArmorConditionPenalty overrides the tweak default.
    assert_eq!(n(&mut g, "SetArmorConditionPenalty 0.5"), 2.0);
    assert!((inv_eff(&mut g, "GetArmorEffectiveDR") - 8.5).abs() < 1e-6);
    assert!((inv_eff(&mut g, "GetArmorEffectiveDT") - 4.25).abs() < 1e-6);
    assert!((n(&mut g, "player.GetActorTotalArmorDR") - 8.5).abs() < 1e-6);
    assert!((n(&mut g, "player.GetActorTotalArmorDT") - 4.25).abs() < 1e-6);

    // Extreme multiplier clamps effective DR/DT at 0.0 instead of going negative.
    q(&mut g, "SetArmorConditionPenalty 5.0");
    assert_eq!(inv_eff(&mut g, "GetArmorEffectiveDR"), 0.0);
    assert_eq!(inv_eff(&mut g, "GetArmorEffectiveDT"), 0.0);
    assert_eq!(n(&mut g, "player.GetActorTotalArmorDR"), 0.0);
    assert_eq!(n(&mut g, "player.GetActorTotalArmorDT"), 0.0);

    // Weapon condition multiplier honors Stewie's bNoWeaponConditionDamagePenalty and bRemoveWeaponDamageBuffer.
    assert!((g.weapon_condition_mult(0.5) - (1.0 - 0.25 * 0.67)).abs() < 1e-6);
    g.tweaks.no_condition_damage_penalty = true;
    assert_eq!(g.weapon_condition_mult(0.0), 1.0);
    assert_eq!(g.weapon_condition_mult(0.5), 1.0);
    g.tweaks.no_condition_damage_penalty = false;
    assert_eq!(g.weapon_condition_mult(0.8), 1.0);
    g.tweaks.no_condition_damage_buffer = true;
    assert!((g.weapon_condition_mult(0.8) - (1.0 - 0.2 * 0.67)).abs() < 1e-6);
    g.tweaks.no_condition_damage_buffer = false;
    q(&mut g, "SetConditionDamagePenalty 3.0");
    assert_eq!(g.weapon_condition_mult(0.0), 0.0);
}

#[test]
fn get_actor_velocity_alt_honors_get_local_flag() {
    let f = fixture();
    let mut g = game(&f);
    // Player facing East (Z = 90 deg) moving at world velocity (100, 20, 5):
    // world mode (0) -> (100, 20, 5), local mode (1) -> (-20 right, 100 forward, 5 up).
    q(&mut g, "player.SetAngle Z 90");
    g.tick();
    let pos = g.ref_pos_rot(FormId::PLAYER_REF).unwrap().0;
    g.jip2.velocity.insert(FormId::PLAYER_REF, (pos, [100.0, 20.0, 5.0]));
    let script_w = "float vx\nfloat vy\nfloat vz\nint ok = player.GetActorVelocityAlt vx vy vz 0\nSetFunctionValue (sv_Construct \"%g %.1f %.1f %.1f\" ok vx vy vz)";
    let script_l = "float vx\nfloat vy\nfloat vz\nint ok = player.GetActorVelocityAlt vx vy vz 1\nSetFunctionValue (sv_Construct \"%g %.1f %.1f %.1f\" ok vx vy vz)";
    assert_eq!(q(&mut g, script_w), XVal::Str("1 100.0 20.0 5.0".into()));
    assert_eq!(q(&mut g, script_l), XVal::Str("1 -20.0 100.0 5.0".into()));
}
