//! JIP LN NVSE batch 8 (#1006; docs/spec-notes/jip-records-8.md) on a synthetic plugin:
//! actor base data, actor effects, ingestibles and base effects, weapons, idles, body-part
//! names and limb models, cells and water.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const NPC: u32 = 0xA00;
const NPC_MULT: u32 = 0xA01;
const CREA: u32 = 0xA02;
const TEMPLATE: u32 = 0xA03;
const MISC: u32 = 0xA04;
const MISC2: u32 = 0xA05;
const FACT: u32 = 0xA06;
const FACT2: u32 = 0xA07;
const FLST: u32 = 0xA08;
const SPEL: u32 = 0xA09;
const SPEL2: u32 = 0xA0A;
const ALCH: u32 = 0xA0B;
const MGEF: u32 = 0xA0C;
const MGEF_SCRIPT: u32 = 0xA0D;
const SCPT: u32 = 0xA0E;
const SCPT2: u32 = 0xA0F;
const ENCH: u32 = 0xA10;
const WEAP: u32 = 0xA11;
const IMOD: u32 = 0xA12;
const IDLE: u32 = 0xA13;
const BPTD: u32 = 0xA14;
const BPTD2: u32 = 0xA15;
const CELL: u32 = 0xA16;
const WATR: u32 = 0xA17;
const WATR2: u32 = 0xA18;
const WEAP_CALC: u32 = 0xA19;
const CONT: u32 = 0xA1A;
const NPC_REF: u32 = 0xA1B;
const BOX_REF: u32 = 0xA1C;
const WRLD: u32 = 0xA1D;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn edid(sig: &[u8; 4], fid: u32, name: &str, rest: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    record(sig, 0, fid, &[vec![sub(b"EDID", z(name))], rest.to_vec()].concat())
}

/// ACBS [documented: xEdit NPC_/CREA]: flags, fatigue, barter gold, level (or the level
/// mult × 1000), calc min, calc max, speed mult, karma, disposition, template flags.
fn acbs(flags: u32, level: u16, min: u16, max: u16) -> Vec<u8> {
    [u32s(&[flags]), le16(0), le16(0), le16(level), le16(min), le16(max), le16(100), f32b(0.0), le16(0), le16(0)].concat()
}

/// SNAM: faction, rank, three unused bytes.
fn snam(f: u32, rank: i8) -> fnv_esm::raw::Subrecord {
    sub(b"SNAM", [u32s(&[f]), vec![rank as u8, 0, 0, 0]].concat())
}

/// MGEF DATA (72 bytes): flags, the associated item, the archetype; the rest empty.
fn mgef(fid: u32, name: &str, flags: u32, item: u32, archetype: u32) -> Vec<u8> {
    let data = [u32s(&[flags]), f32b(0.0), u32s(&[item]), vec![0xFF; 8], vec![0; 4], vec![0; 4], f32b(1.0), vec![0; 24], vec![0; 8], u32s(&[archetype]), (-1i32).to_le_bytes().to_vec()].concat();
    assert_eq!(data.len(), 72);
    edid(b"MGEF", fid, name, &[sub(b"DATA", data)])
}

/// BPND (84 bytes) of part type `part`, all else empty.
fn bpnd(part: i8) -> fnv_esm::raw::Subrecord {
    let data = [f32b(1.0), vec![0, part as u8, 100, 0xFF, 0, 0], vec![0; 2], vec![0; 72]].concat();
    assert_eq!(data.len(), 84);
    sub(b"BPND", data)
}

/// WATR DNAM: 49 four-byte slots; slot k (trait k + 2) holds k + 2, the three colours
/// (slots 10-12) RGB bytes.
fn watr_dnam() -> Vec<u8> {
    (0..49u8)
        .flat_map(|k| match k {
            10 => vec![10, 20, 30, 0],
            11 => vec![1, 2, 3, 0],
            12 => vec![255, 0, 128, 0],
            7 | 13 => vec![0; 4],
            _ => f32b(f32::from(k) + 2.0),
        })
        .collect()
}

fn plugin_bytes() -> Vec<u8> {
    let npcs = [
        record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]),
        // Female (bit 0), level 5, 2-10; a faction rank 2; one actor effect; a death item,
        // a template; a skeleton.
        edid(b"NPC_", NPC, "TestNPC", &[sub(b"ACBS", acbs(1, 5, 2, 10)), snam(FACT, 2), sub(b"SPLO", u32s(&[SPEL])), sub(b"INAM", u32s(&[MISC])), sub(b"TPLT", u32s(&[TEMPLATE])), sub(b"MODL", z("characters\\_male\\skeleton.nif"))]),
        // PC Level Mult (bit 7), mult 0.5.
        edid(b"NPC_", NPC_MULT, "TestNPCMult", &[sub(b"ACBS", acbs(1 << 7, 500, 1, 0))]),
        edid(b"NPC_", TEMPLATE, "TestTemplate", &[sub(b"ACBS", acbs(0, 1, 0, 0))]),
    ]
    .concat();
    let crea = edid(b"CREA", CREA, "TestCreature", &[sub(b"ACBS", acbs(0, 3, 0, 0)), sub(b"PNAM", u32s(&[BPTD])), sub(b"SPLO", u32s(&[SPEL])), sub(b"SPLO", u32s(&[SPEL2])), sub(b"MODL", z("creatures\\gecko\\skeleton.nif"))]);
    let misc = [edid(b"MISC", MISC, "TestDeathItem", &[]), edid(b"MISC", MISC2, "TestDeathItem2", &[])].concat();
    let fact = [edid(b"FACT", FACT, "TestFaction", &[]), edid(b"FACT", FACT2, "TestFaction2", &[])].concat();
    let flst = edid(b"FLST", FLST, "TestFactionList", &[sub(b"LNAM", u32s(&[FACT2]))]);
    // SPIT: type, cost, level, flags (TestAbility's bit 4, so a set keeps the record's
    // other bits; TestAbility2's 0).
    let spit = |flags: u8| sub(b"SPIT", [u32s(&[4, 0, 0]), vec![flags, 0, 0, 0]].concat());
    let spel = [edid(b"SPEL", SPEL, "TestAbility", &[spit(16)]), edid(b"SPEL", SPEL2, "TestAbility2", &[spit(0)])].concat();
    // ENIT: value, flags 1 (food), unused, withdrawal SPEL, chance 0.1, sound.
    let alch = edid(b"ALCH", ALCH, "TestChem", &[sub(b"ENIT", [5i32.to_le_bytes().to_vec(), vec![2, 0, 0, 0], u32s(&[SPEL]), f32b(0.1), u32s(&[0])].concat())]);
    let mgefs = [mgef(MGEF, "TestEffect", 1 << 2, 0, 0), mgef(MGEF_SCRIPT, "TestScriptEffect", 0, SCPT, 1)].concat();
    let scpt = [edid(b"SCPT", SCPT, "TestScript", &[]), edid(b"SCPT", SCPT2, "TestScript2", &[])].concat();
    // ENIT: type, charge, cost, flags 1 (bit 0), unused.
    let ench = edid(b"ENCH", ENCH, "TestEnchant", &[sub(b"ENIT", [u32s(&[2, 0, 0]), vec![1, 0, 0, 0]].concat())]);
    // WEAP DATA (15 bytes): value 100, health 100, weight 2.0, base_damage 20, clip 10.
    // WEAP DNAM (204 bytes): animation_type 3 (offset 0), attack_shots_per_sec 2.0 (offset 88),
    // skill 41 / Guns (offset 104). CRDT (16 bytes): critical_damage 30, crit_percent_mult 2.0.
    let calc_data = [100i32.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), f32b(2.0), 20i16.to_le_bytes().to_vec(), vec![10]].concat();
    let mut calc_dnam = vec![0u8; 204];
    calc_dnam[0..4].copy_from_slice(&3u32.to_le_bytes());
    calc_dnam[88..92].copy_from_slice(&2.0f32.to_le_bytes());
    calc_dnam[104..108].copy_from_slice(&41u32.to_le_bytes());
    let calc_crdt = [30u16.to_le_bytes().to_vec(), vec![0, 0], f32b(2.0), vec![0; 4], u32s(&[0])].concat();
    let weap = [
        edid(b"WEAP", WEAP, "TestGun", &[sub(b"MODL", z("weapons\\gun.nif")), sub(b"VNAM", u32s(&[1]))]),
        edid(b"WEAP", WEAP_CALC, "CalcGun", &[sub(b"DATA", calc_data), sub(b"DNAM", calc_dnam), sub(b"CRDT", calc_crdt)]),
    ]
    .concat();
    let cont = edid(b"CONT", CONT, "TestBox", &[sub(b"DATA", [vec![0], f32b(0.0)].concat())]);
    let imod = edid(b"IMOD", IMOD, "TestMod", &[]);
    // IDLE DATA: section, loop 1-3, unused, replay delay, flags, unused.
    let idle = edid(b"IDLE", IDLE, "TestIdle", &[sub(b"DATA", vec![0, 1, 3, 0, 0, 0, 0, 0])]);
    // Two parts: the torso named "Body" with a limb model, the head (by BPNN, no name).
    let bptd = [
        edid(
            b"BPTD",
            BPTD,
            "TestBodyParts",
            &[sub(b"BPTN", z("Body")), sub(b"BPNN", z("Bip01 Spine")), bpnd(0), sub(b"NAM1", z("torso_rep.nif")), sub(b"BPNN", z("Bip01 Head")), bpnd(1)],
        ),
        edid(b"BPTD", BPTD2, "TestBodyParts2", &[sub(b"BPTN", z("Torso")), sub(b"BPNN", z("Bip01")), bpnd(0)]),
    ]
    .concat();
    // Interior (bit 0) and has water (bit 1); LNAM ambient colour (bit 0); a water.
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell")), sub(b"DATA", vec![0x03]), sub(b"LNAM", u32s(&[1])), sub(b"XCWT", u32s(&[WATR]))]);
    let placed_refs = [
        record(b"ACHR", 0, NPC_REF, &[sub(b"EDID", z("TestNpcRef")), sub(b"NAME", u32s(&[NPC])), sub(b"DATA", vec![0; 24])]),
        record(b"REFR", 0, BOX_REF, &[sub(b"EDID", z("TestBoxRef")), sub(b"NAME", u32s(&[CONT])), sub(b"DATA", vec![0; 24])]),
    ]
    .concat();
    let cell_children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 8, &placed_refs));
    let watr = [
        edid(b"WATR", WATR, "TestWater", &[sub(b"ANAM", vec![75]), sub(b"FNAM", vec![2]), sub(b"XNAM", u32s(&[SPEL])), sub(b"DATA", vec![0, 0]), sub(b"DNAM", watr_dnam())]),
        edid(b"WATR", WATR2, "TestWater2", &[sub(b"DATA", vec![0, 0])]),
    ]
    .concat();
    // Worldspace with DATA flags bit 0 (Small World) and bit 1 (Can't Fast Travel) set (#3757).
    let wrld = edid(b"WRLD", WRLD, "TestWorld", &[sub(b"DATA", vec![0x03])]);
    [
        tes4(),
        top(b"NPC_", &npcs),
        top(b"CREA", &crea),
        top(b"CONT", &cont),
        top(b"MISC", &misc),
        top(b"FACT", &fact),
        top(b"FLST", &flst),
        top(b"SPEL", &spel),
        top(b"ALCH", &alch),
        top(b"MGEF", &mgefs),
        top(b"SCPT", &scpt),
        top(b"ENCH", &ench),
        top(b"WEAP", &weap),
        top(b"IMOD", &imod),
        top(b"IDLE", &idle),
        top(b"BPTD", &bptd),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, cell_children].concat()))),
        top(b"WRLD", &wrld),
        top(b"WATR", &watr),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jip8.esm".into()).unwrap()];
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

/// The value a query's last line sets.
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

fn s(g: &mut Game, expr: &str) -> String {
    match q(g, &format!("string_var sOut\nlet sOut := {expr}\nSetFunctionValue sOut")) {
        XVal::Str(s) => s,
        v => panic!("{expr}: {v:?}"),
    }
}

#[test]
fn actor_base_data() {
    let f = fixture();
    let mut g = game(&f);
    // Death item and template [documented: the pages' `F … baseForm` form].
    assert_eq!(n(&mut g, "GetDeathItem TestNPC"), f64::from(MISC));
    q(&mut g, "SetDeathItem TestDeathItem2 TestNPC");
    assert_eq!(n(&mut g, "GetDeathItem TestNPC"), f64::from(MISC2));
    assert_eq!(n(&mut g, "GetActorTemplate TestNPC"), f64::from(TEMPLATE));
    assert_eq!(n(&mut g, "GetActorTemplate TestCreature"), 0.0);
    // Not an actor: none.
    assert_eq!(n(&mut g, "GetDeathItem TestGun"), 0.0);
    // Leveling data [documented: the table, 0 level or mult, 1 calc min, 2 calc max].
    assert_eq!((n(&mut g, "GetActorLevelingData 0 TestNPC"), n(&mut g, "GetActorLevelingData 1 TestNPC"), n(&mut g, "GetActorLevelingData 2 TestNPC")), (5.0, 2.0, 10.0));
    assert_eq!(n(&mut g, "GetActorLevelingData 0 TestNPCMult"), 0.5);
    q(&mut g, "SetActorLevelingData 0 0.75 TestNPCMult");
    assert_eq!(n(&mut g, "GetActorLevelingData 0 TestNPCMult"), 0.75);
    assert_eq!(g.num_field(FormId(NPC_MULT), "ACBS", "level_or_level_mult_raw"), 750.0);
    q(&mut g, "SetActorLevelingData 1 5 TestNPC");
    assert_eq!(n(&mut g, "GetActorLevelingData 1 TestNPC"), 5.0);
    // Calc max not below calc min [observed: jip8].
    q(&mut g, "SetActorLevelingData 2 0 TestNPC");
    assert_eq!(n(&mut g, "GetActorLevelingData 2 TestNPC"), 5.0);
    assert_eq!(n(&mut g, "GetActorLevelingData 3 TestNPC"), 0.0);
    // Base faction rank, -1 when not a member [documented].
    assert_eq!(n(&mut g, "GetBaseFactionRank TestFaction TestNPC"), 2.0);
    assert_eq!(n(&mut g, "GetBaseFactionRank TestFaction2 TestNPC"), -1.0);
    // Body-part data and skeletons, read by the earlier getters.
    q(&mut g, "SetBodyPartData TestCreature TestBodyParts2");
    assert_eq!(n(&mut g, "GetBodyPartData TestCreature"), f64::from(BPTD2));
    q(&mut g, "SetCRESkeletonModel TestCreature \"creatures\\rat\\skeleton.nif\"");
    assert_eq!(s(&mut g, "GetSkeletonModel TestCreature"), "creatures\\rat\\skeleton.nif");
    q(&mut g, "SetNPCSkeletonModel TestNPC \"characters\\_male\\skeletonalt.nif\"");
    assert_eq!(s(&mut g, "GetSkeletonModel TestNPC"), "characters\\_male\\skeletonalt.nif");
}

#[test]
fn factions_and_combat_actions() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "PlayerREF.GetInFactionList TestFactionList"), 0.0);
    q(&mut g, "PlayerREF.AddToFaction TestFaction2 0");
    assert_eq!(n(&mut g, "PlayerREF.GetInFactionList TestFactionList"), 1.0);
    // The 27 bits [documented: the table]; past them dropped.
    assert_eq!(n(&mut g, "PlayerREF.GetExcludedCombatActions"), 0.0);
    q(&mut g, "PlayerREF.SetExcludedCombatActions 384");
    assert_eq!(n(&mut g, "PlayerREF.GetExcludedCombatActions"), 384.0);
    q(&mut g, "PlayerREF.SetExcludedCombatActions 268435457");
    assert_eq!(n(&mut g, "PlayerREF.GetExcludedCombatActions"), 1.0);
    q(&mut g, "PlayerREF.SetExcludedCombatActions 0");
    assert_eq!(n(&mut g, "PlayerREF.GetExcludedCombatActions"), 0.0);
}

#[test]
fn actor_effects_ingestibles_and_base_effects() {
    let f = fixture();
    let mut g = game(&f);
    // SPLO [documented: GetNumActorEffects, GetNthActorEffect]; the record's last first,
    // an added one at the front [observed: jip8].
    assert_eq!((n(&mut g, "GetNumActorEffects TestCreature"), n(&mut g, "GetNthActorEffect 0 TestCreature"), n(&mut g, "GetNthActorEffect 1 TestCreature")), (2.0, f64::from(SPEL2), f64::from(SPEL)));
    assert_eq!((n(&mut g, "GetNumActorEffects TestNPC"), n(&mut g, "GetNthActorEffect 0 TestNPC")), (1.0, f64::from(SPEL)));
    q(&mut g, "AddBaseEffectListEffect TestAbility2 TestNPC");
    q(&mut g, "AddBaseEffectListEffect TestAbility2 TestNPC");
    assert_eq!((n(&mut g, "GetNumActorEffects TestNPC"), n(&mut g, "GetNthActorEffect 0 TestNPC"), n(&mut g, "GetNthActorEffect 1 TestNPC")), (2.0, f64::from(SPEL2), f64::from(SPEL)));
    q(&mut g, "RemoveBaseEffectListEffect TestAbility TestNPC");
    assert_eq!((n(&mut g, "GetNumActorEffects TestNPC"), n(&mut g, "GetNthActorEffect 0 TestNPC")), (1.0, f64::from(SPEL2)));
    assert_eq!(n(&mut g, "GetNthActorEffect 5 TestNPC"), 0.0);
    // Ingestible ENIT [documented: the pages].
    assert!((n(&mut g, "GetAddictionChance TestChem") - 0.1).abs() < 1e-6);
    q(&mut g, "SetAddictionChance TestChem 0.25");
    assert_eq!(n(&mut g, "GetAddictionChance TestChem"), 0.25);
    // Kept as given [observed: jip8, 50, 1.5, -0.5].
    q(&mut g, "SetAddictionChance TestChem 1.5");
    assert_eq!(n(&mut g, "GetAddictionChance TestChem"), 1.5);
    assert_eq!(n(&mut g, "GetAddictionEffect TestChem"), f64::from(SPEL));
    q(&mut g, "SetAddictionEffect TestChem TestAbility2");
    assert_eq!(n(&mut g, "GetAddictionEffect TestChem"), f64::from(SPEL2));
    // None is ignored [observed: jip8].
    q(&mut g, "SetAddictionEffect TestChem 0");
    assert_eq!(n(&mut g, "GetAddictionEffect TestChem"), f64::from(SPEL2));
    // A script effect's script; not a script effect, none [documented: "script-type"].
    assert_eq!(n(&mut g, "GetBaseEffectScript TestScriptEffect"), f64::from(SCPT));
    q(&mut g, "SetBaseEffectScript TestScriptEffect TestScript2");
    assert_eq!(n(&mut g, "GetBaseEffectScript TestScriptEffect"), f64::from(SCPT2));
    q(&mut g, "SetBaseEffectScript TestEffect TestScript2");
    assert_eq!(n(&mut g, "GetBaseEffectScript TestEffect"), 0.0);
    // Base effect flags, read by GetBaseEffectFlag; 11, not in the table, too [observed:
    // jip8].
    assert_eq!(n(&mut g, "GetBaseEffectFlag TestEffect 2"), 1.0);
    q(&mut g, "SetBaseEffectFlag TestEffect 2 0");
    q(&mut g, "SetBaseEffectFlag TestEffect 0 1");
    q(&mut g, "SetBaseEffectFlag TestEffect 11 1");
    assert_eq!((n(&mut g, "GetBaseEffectFlag TestEffect 2"), n(&mut g, "GetBaseEffectFlag TestEffect 0"), n(&mut g, "GetBaseEffectFlag TestEffect 11")), (0.0, 1.0, 1.0));
    // Effect flags of each kind, read by GetEffectFlag; any bit 0-7 [observed: jip8].
    assert_eq!(n(&mut g, "GetEffectFlag TestChem 1"), 1.0);
    q(&mut g, "SetEffectFlag TestChem 2 1");
    q(&mut g, "SetEffectFlag TestChem 1 0");
    assert_eq!((n(&mut g, "GetEffectFlag TestChem 1"), n(&mut g, "GetEffectFlag TestChem 2")), (0.0, 1.0));
    // An actor effect's SPIT flags and an object effect's ENIT flags are the record's
    // own, and a set keeps their other bits.
    assert_eq!((n(&mut g, "GetEffectFlag TestAbility 4"), n(&mut g, "GetEffectFlag TestAbility 7")), (1.0, 0.0));
    assert_eq!((n(&mut g, "GetEffectFlag TestEnchant 0"), n(&mut g, "GetEffectFlag TestEnchant 2")), (1.0, 0.0));
    q(&mut g, "SetEffectFlag TestAbility 7 1");
    q(&mut g, "SetEffectFlag TestAbility 1 1");
    q(&mut g, "SetEffectFlag TestAbility 8 1");
    assert_eq!((n(&mut g, "GetEffectFlag TestAbility 7"), n(&mut g, "GetEffectFlag TestAbility 1"), n(&mut g, "GetEffectFlag TestAbility 8")), (1.0, 1.0, 0.0));
    assert_eq!(n(&mut g, "GetEffectFlag TestAbility 4"), 1.0);
    q(&mut g, "SetEffectFlag TestEnchant 2 1");
    assert_eq!((n(&mut g, "GetEffectFlag TestEnchant 2"), n(&mut g, "GetEffectFlag TestEnchant 0")), (1.0, 1.0));
}

#[test]
fn weapons() {
    let f = fixture();
    let mut g = game(&f);
    // VNAM [documented: 0 loud, 1 normal, 2 silent].
    assert_eq!(n(&mut g, "GetWeaponDetectionSoundLevel TestGun"), 1.0);
    q(&mut g, "SetWeaponDetectionSoundLevel TestGun 2");
    assert_eq!(n(&mut g, "GetWeaponDetectionSoundLevel TestGun"), 2.0);
    q(&mut g, "SetWeaponDetectionSoundLevel TestGun 9");
    assert_eq!(n(&mut g, "GetWeaponDetectionSoundLevel TestGun"), 2.0);
    // Mods, read by GetWeaponItemMod and the rest; left out or non-IMOD clears to 0
    // [documented; observed: jip_20_weapon_item_mod_non_imod].
    q(&mut g, "SetWeaponItemMod TestGun 2 TestMod");
    assert_eq!(n(&mut g, "GetWeaponItemMod 2 TestGun"), f64::from(IMOD));
    q(&mut g, "SetWeaponItemMod TestGun 2 TestGun");
    assert_eq!(n(&mut g, "GetWeaponItemMod 2 TestGun"), 0.0, "non-IMOD clears slot to 0");
    q(&mut g, "SetWeaponItemMod TestGun 2 TestMod");
    assert_eq!(n(&mut g, "GetWeaponItemMod 2 TestGun"), f64::from(IMOD));
    q(&mut g, "SetWeaponItemMod TestGun 2");
    assert_eq!(n(&mut g, "GetWeaponItemMod 2 TestGun"), 0.0);
    q(&mut g, "SetWeaponItemModEffect TestGun 3 11");
    q(&mut g, "SetWeaponItemModValue TestGun 3 6.5");
    assert_eq!((n(&mut g, "GetWeaponItemModEffect 3 TestGun"), n(&mut g, "GetWeaponItemModValue1 3 TestGun")), (11.0, 6.5));
    // 0 is ignored, 15 stored [observed: jip8].
    q(&mut g, "SetWeaponItemModEffect TestGun 3 0");
    assert_eq!(n(&mut g, "GetWeaponItemModEffect 3 TestGun"), 11.0);
    q(&mut g, "SetWeaponItemModEffect TestGun 1 15");
    assert_eq!(n(&mut g, "GetWeaponItemModEffect 1 TestGun"), 15.0);
    // The slot with the type [documented: WeaponHasModType].
    assert_eq!(n(&mut g, "WeaponHasModType TestGun 11"), 3.0);
    assert_eq!(n(&mut g, "WeaponHasModType TestGun 3"), 0.0);
    // On hit, semi-auto delays, models, reload animation; read by the VM's getters.
    q(&mut g, "SetWeaponOnHitSetting TestGun 2");
    assert_eq!(n(&mut g, "GetWeaponOnHitSetting TestGun"), 2.0);
    q(&mut g, "SetWeaponSemiAutoFireDelay TestGun 1 0.5");
    q(&mut g, "SetWeaponSemiAutoFireDelay TestGun 0 0.25");
    assert_eq!((n(&mut g, "GetWeaponSemiAutoFireDelay TestGun 1"), n(&mut g, "GetWeaponSemiAutoFireDelay TestGun 0")), (0.5, 0.25));
    // Raw negative delays store verbatim without clamping [observed: Wave 4
    // jip_22_weapon_semiauto_delay_neg].
    q(&mut g, "SetWeaponSemiAutoFireDelay TestGun 0 -0.5");
    assert_eq!(n(&mut g, "GetWeaponSemiAutoFireDelay TestGun 0"), -0.5);
    q(&mut g, "SetWeaponSemiAutoFireDelay TestGun 0 0.25");
    q(&mut g, "SetWeaponModel TestGun 3 \"weapons\\gun13.nif\"");
    assert_eq!((s(&mut g, "GetWeaponModel TestGun 3"), s(&mut g, "GetWeaponModel TestGun 0")), ("weapons\\gun13.nif".to_string(), "weapons\\gun.nif".to_string()));
    // The renderer is told (weapon-mods.md §8); no first-person model clears the form, as
    // WAP's script files call it.
    let stamp = g.weapon_model_stamp();
    q(&mut g, "SetWeapon1stPersonModel TestGun 0");
    assert_ne!(g.weapon_model_stamp(), stamp);
    let changes = g.weapon_model_changes();
    let c = &changes[&FormId(WEAP)];
    assert_eq!((c.world[3].clone(), c.world[0].clone(), c.first_person[0].clone(), c.first_person[3].clone()), (Some(Some("weapons\\gun13.nif".to_string())), None, Some((None, None)), None));
    assert_eq!(n(&mut g, "GetWeapon1stPersonModel TestGun 0"), 0.0);
    // A model argument that is no form (an empty reference) changes nothing; no model
    // argument clears.
    let set = |g: &mut Game, args: &[XVal]| fnv_game::nvse::call(g, &RunCtx::default(), "setweapon1stpersonmodel", None, args).unwrap().unwrap();
    set(&mut g, &[XVal::Ref(WEAP), XVal::Num(4.0), XVal::Ref(0)]);
    set(&mut g, &[XVal::Ref(WEAP), XVal::Num(5.0)]);
    let changes = g.weapon_model_changes();
    assert_eq!((changes[&FormId(WEAP)].first_person[4].clone(), changes[&FormId(WEAP)].first_person[5].clone()), (None, Some((None, None))));
    q(&mut g, "SetWeaponModReloadAnim TestGun 8");
    assert_eq!(n(&mut g, "GetWeaponModReloadAnim TestGun"), 8.0);
    // Embedded weapon: the condition actor values 0-6, and the node.
    q(&mut g, "SetEmbeddedWeaponAV TestGun BrainCondition");
    assert_eq!(g.num_field(FormId(WEAP), "DNAM", "embedded_weapon_actor_value"), 6.0);
    q(&mut g, "SetEmbeddedWeaponAV TestGun Luck");
    assert_eq!(g.num_field(FormId(WEAP), "DNAM", "embedded_weapon_actor_value"), 6.0);
    q(&mut g, "SetEmbeddedWeaponNode TestGun \"ProjectileNode_R\"");
}

#[test]
fn idles_and_body_parts() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!((n(&mut g, "GetIdleLoopTimes TestIdle 0"), n(&mut g, "GetIdleLoopTimes TestIdle 1")), (1.0, 3.0));
    q(&mut g, "SetIdleLoopTimes TestIdle 2 4");
    assert_eq!((n(&mut g, "GetIdleLoopTimes TestIdle 0"), n(&mut g, "GetIdleLoopTimes TestIdle 1")), (2.0, 4.0));
    // The min a u8, the max not below it [observed: jip8, 300 2 reads 255 255].
    q(&mut g, "SetIdleLoopTimes TestIdle 300 2");
    assert_eq!((n(&mut g, "GetIdleLoopTimes TestIdle 0"), n(&mut g, "GetIdleLoopTimes TestIdle 1")), (255.0, 255.0));
    // The torso's limb model; the head has none; no brain part.
    assert_eq!(s(&mut g, "GetBodyPartReplacementModel TestBodyParts 0"), "torso_rep.nif");
    assert_eq!(s(&mut g, "GetBodyPartReplacementModel TestBodyParts 1"), "");
    q(&mut g, "SetBodyPartReplacementModel TestBodyParts 1 \"head_rep.nif\"");
    assert_eq!(s(&mut g, "GetBodyPartReplacementModel TestBodyParts 1"), "head_rep.nif");
    q(&mut g, "SetBodyPartReplacementModel TestBodyParts 13 \"brain.nif\"");
    assert_eq!(s(&mut g, "GetBodyPartReplacementModel TestBodyParts 13"), "");
    // Names, read by GetBodyPartName [documented: SetBodyPartName's example].
    assert_eq!(s(&mut g, "GetBodyPartName TestBodyParts 0"), "Body");
    q(&mut g, "SetBodyPartName TestBodyParts 0 \"Chassis\"");
    assert_eq!(s(&mut g, "GetBodyPartName TestBodyParts 0"), "Chassis");
    assert_eq!(s(&mut g, "GetBodyPartName TestBodyParts2 0"), "Torso");
}

#[test]
fn cells_and_water() {
    let f = fixture();
    let mut g = game(&f);
    // DATA 0-7, LNAM 8-16, read by GetCellFlag [documented: the tables].
    assert_eq!((n(&mut g, "GetCellFlag TestCell 0"), n(&mut g, "GetCellFlag TestCell 1"), n(&mut g, "GetCellFlag TestCell 8"), n(&mut g, "GetCellFlag TestCell 9")), (1.0, 1.0, 1.0, 0.0));
    assert_eq!((n(&mut g, "GetCellFlag TestCell -1"), n(&mut g, "GetCellFlag TestCell 17")), (0.0, 0.0));
    q(&mut g, "SetCellFlag TestCell 1 0");
    q(&mut g, "SetCellFlag TestCell 5 1");
    q(&mut g, "SetCellFlag TestCell 16 1");
    q(&mut g, "SetCellFlag TestCell 4 1");
    assert_eq!(n(&mut g, "GetCellFlag TestCell 4"), 1.0);
    assert_eq!((n(&mut g, "GetCellFlag TestCell 1"), n(&mut g, "GetCellFlag TestCell 5"), n(&mut g, "GetCellFlag TestCell 16")), (0.0, 1.0, 1.0));
    // Non-CELL records (WRLD with DATA bits 0-1 set, FLST with LNAM, null form) must not leak
    // their own DATA/LNAM bits through GetCellFlag, while GetWorldspaceFlag still reads WRLD (#3757).
    assert_eq!((n(&mut g, "GetWorldspaceFlag TestWorld 0"), n(&mut g, "GetWorldspaceFlag TestWorld 1")), (1.0, 1.0));
    assert_eq!((n(&mut g, "GetCellFlag TestWorld 0"), n(&mut g, "GetCellFlag TestWorld 1")), (0.0, 0.0));
    q(&mut g, "SetWorldspaceFlag TestWorld 5 1");
    assert_eq!(n(&mut g, "GetWorldspaceFlag TestWorld 5"), 1.0);
    assert_eq!(n(&mut g, "GetCellFlag TestWorld 5"), 0.0);
    // SetCellFlag also ignores a WRLD and must not mutate its flags.
    q(&mut g, "SetCellFlag TestWorld 0 0");
    assert_eq!(n(&mut g, "GetWorldspaceFlag TestWorld 0"), 1.0);
    assert_eq!(n(&mut g, "GetCellFlag TestFactionList 8"), 0.0);
    assert_eq!(n(&mut g, "GetCellFlag 0 0"), 0.0);
    // Water form and effect, read by GetCellWaterForm and GetWaterFormEffect.
    assert_eq!(n(&mut g, "GetCellWaterForm TestCell"), f64::from(WATR));
    q(&mut g, "SetCellWaterForm TestCell TestWater2");
    assert_eq!(n(&mut g, "GetCellWaterForm TestCell"), f64::from(WATR2));
    assert_eq!(n(&mut g, "GetWaterFormEffect TestWater"), f64::from(SPEL));
    q(&mut g, "SetWaterFormEffect TestWater TestAbility2");
    assert_eq!(n(&mut g, "GetWaterFormEffect TestWater"), f64::from(SPEL2));
    // Water traits [documented: the table].
    assert_eq!((n(&mut g, "GetWaterTrait TestWater 0"), n(&mut g, "GetWaterTrait TestWater 1")), (75.0, 2.0));
    assert_eq!((n(&mut g, "GetWaterTrait TestWater 6"), n(&mut g, "GetWaterTrait TestWater 41"), n(&mut g, "GetWaterTrait TestWater 50")), (6.0, 41.0, 50.0));
    assert_eq!((n(&mut g, "GetWaterTrait TestWater 12"), n(&mut g, "GetWaterTrait TestWater 14")), (10_020_030.0, 255_000_128.0));
    assert_eq!((n(&mut g, "GetWaterTrait TestWater 9"), n(&mut g, "GetWaterTrait TestWater 51")), (0.0, 0.0));
}

#[test]
fn calculated_weapon_damage_and_dps_resolve_inv_and_world_refs() {
    let f = fixture();
    let mut g = game(&f);

    // Stewie plugin gate promoted to 960 (#3183); 970 (#3184) remains held.
    assert_eq!(n(&mut g, "GetPluginVersion \"lStewieAl's Tweaks\""), 960.0);

    // Player: Guns 50 (factor 0.75), CritChance 10 (0.10 * 30 * 2.0 = 6.0 crit contrib).
    // NPC: Guns 100 (factor 1.0), CritChance 0.
    q(&mut g, "PlayerREF.ForceAV Guns 50\nPlayerREF.ForceAV CritChance 10\nTestNpcRef.ForceAV Guns 100\nTestNpcRef.ForceAV CritChance 0");

    // Base-form call uses player's stats (20 * 0.75 = 15, DPS = (15 + 6) * 2 = 42).
    assert!((n(&mut g, "GetCalculatedWeaponDamage CalcGun") - 15.0).abs() < 1e-6);
    assert!((n(&mut g, "GetCalculatedWeaponDPS CalcGun") - 42.0).abs() < 1e-6);

    // Put CalcGun into player, NPC, and non-actor container inventories.
    q(&mut g, "PlayerREF.AddItem CalcGun 1 1\nTestNpcRef.AddItem CalcGun 1 1\nTestBoxRef.AddItem CalcGun 1 1");

    // Inventory reference in player's inventory (the PipBoyUITweaks calling pattern:
    // `rWeap.GetCalculatedWeaponDamage` with 0 args).
    let inv_dmg = |g: &mut Game, holder: &str, fn_name: &str| -> f64 {
        let script = format!("array_var a\nref r\nlet a := {holder}.GetInvRefsForItem CalcGun\nlet r := a[0]\nSetFunctionValue (r.{fn_name})");
        match q(g, &script) {
            XVal::Num(x) => x,
            v => panic!("{holder} {fn_name}: {v:?}"),
        }
    };
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDamage") - 15.0).abs() < 1e-6);
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDPS") - 42.0).abs() < 1e-6);

    // Inventory reference in an NPC uses the owning NPC's stats (20 * 1.0 = 20, DPS = 40).
    assert!((inv_dmg(&mut g, "TestNpcRef", "GetCalculatedWeaponDamage") - 20.0).abs() < 1e-6);
    assert!((inv_dmg(&mut g, "TestNpcRef", "GetCalculatedWeaponDPS") - 40.0).abs() < 1e-6);

    // Inventory reference in a non-actor container falls back to player's stats.
    assert!((inv_dmg(&mut g, "TestBoxRef", "GetCalculatedWeaponDamage") - 15.0).abs() < 1e-6);
    assert!((inv_dmg(&mut g, "TestBoxRef", "GetCalculatedWeaponDPS") - 42.0).abs() < 1e-6);

    // Attach weapon mods: slot 1 Increase Weapon Damage (+10), slot 2 Increase Rate of Fire (+0.25).
    q(
        &mut g,
        "SetWeaponItemModEffect CalcGun 1 1\nSetWeaponItemModValue CalcGun 1 10\nSetWeaponItemModEffect CalcGun 2 8\nSetWeaponItemModValue CalcGun 2 0.25",
    );
    q(&mut g, "array_var a\nref r\nlet a := PlayerREF.GetInvRefsForItem CalcGun\nlet r := a[0]\nr.SetWeaponRefModFlags 1");
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDamage") - 22.5).abs() < 1e-6);
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDPS") - 57.0).abs() < 1e-6);

    q(&mut g, "array_var a\nref r\nlet a := PlayerREF.GetInvRefsForItem CalcGun\nlet r := a[0]\nr.SetWeaponRefModFlags 3");
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDamage") - 22.5).abs() < 1e-6);
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDPS") - 71.25).abs() < 1e-6);

    // Condition scaling below 75% (0.5 -> factor = 1 - 0.25 * 0.67 = 0.8325 -> 30 * 0.8325 * 0.75 = 18.73125).
    q(&mut g, "array_var a\nref r\nlet a := PlayerREF.GetInvRefsForItem CalcGun\nlet r := a[0]\nr.SetItemRefCurrentHealth 0.5 1");
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDamage") - 18.73125).abs() < 1e-5);

    // SetConditionDamagePenalty overrides the slope (1.0 -> factor = 0.75 -> 30 * 0.75 * 0.75 = 16.875).
    q(&mut g, "SetConditionDamagePenalty 1.0");
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDamage") - 16.875).abs() < 1e-5);

    // Stewie's bNoWeaponConditionDamagePenalty restores full damage regardless of condition.
    g.tweaks.no_condition_damage_penalty = true;
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDamage") - 22.5).abs() < 1e-5);
    g.tweaks.no_condition_damage_penalty = false;
    q(&mut g, "SetConditionDamagePenalty 0.67");

    // Stewie's bRemoveWeaponDamageBuffer drops the threshold from 75% to 100%.
    q(&mut g, "array_var a\nref r\nlet a := PlayerREF.GetInvRefsForItem CalcGun\nlet r := a[0]\nr.SetItemRefCurrentHealth 0.8 1");
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDamage") - 22.5).abs() < 1e-5);
    g.tweaks.no_condition_damage_buffer = true;
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDamage") - (30.0 * (1.0 - 0.2 * 0.67) * 0.75)).abs() < 1e-4);
    g.tweaks.no_condition_damage_buffer = false;

    // Stewie's crit_damage_scale scales the critical contribution in GetCalculatedWeaponDPS.
    g.tweaks.crit_damage_scale = 2.0;
    assert!((inv_dmg(&mut g, "PlayerREF", "GetCalculatedWeaponDPS") - ((22.5 + 12.0) * 2.5)).abs() < 1e-5);

    // Actor calling with 0 args uses the equipped weapon's condition and attached mods.
    q(&mut g, "PlayerREF.EquipItem CalcGun 0 1");
    assert!((n(&mut g, "PlayerREF.GetCalculatedWeaponDamage") - 22.5).abs() < 1e-5);
    assert!((n(&mut g, "PlayerREF.GetCalculatedWeaponDPS") - 86.25).abs() < 1e-5);
    g.tweaks.crit_damage_scale = 1.0;

    // Dropped world reference preserves RefState.extra condition and mods and uses player's stats.
    g.world
        .inventory_mut(FormId::PLAYER_REF)
        .unwrap()
        .add_extra(FormId(WEAP_CALC), 1, fnv_sim::inventory::Extra { condition: Some(0.5), mods: 1, ..Default::default() });
    let dropped = g
        .world
        .drop_item_at(FormId::PLAYER_REF, FormId(WEAP_CALC), 1, fnv_sim::inventory::Pick::row(Some(0.5), 1))
        .unwrap()[0];
    let drop_dmg = g
        .run_query("Begin Function { }\nSetFunctionValue (GetCalculatedWeaponDamage)\nEnd\n", RunCtx { self_ref: Some(dropped), ..Default::default() })
        .unwrap();
    match drop_dmg {
        XVal::Num(x) => assert!((x - 18.73125).abs() < 1e-5, "dropped weapon damage: {x}"),
        v => panic!("dropped weapon damage: {v:?}"),
    }
    let drop_dps = g
        .run_query("Begin Function { }\nSetFunctionValue (GetCalculatedWeaponDPS)\nEnd\n", RunCtx { self_ref: Some(dropped), ..Default::default() })
        .unwrap();
    match drop_dps {
        XVal::Num(x) => assert!((x - ((18.73125 + 6.0) * 2.0)).abs() < 1e-4, "dropped weapon dps: {x}"),
        v => panic!("dropped weapon dps: {v:?}"),
    }
}

/// Script-set models are the session's: a load starts from the records again (`Game::restore`
/// builds a new game), and the Script Runner's `gr_` files set them again after it
/// (ext-script-runner.md; weapon-mods.md §8).
#[test]
fn a_load_drops_the_models_scripts_set() {
    let f = fixture();
    let mut g = game(&f);
    let empty = g.weapon_model_stamp();
    q(&mut g, r#"SetWeaponModel TestGun 0 "weapons\other.nif"
SetWeapon1stPersonModel TestGun 0"#);
    assert_eq!(g.weapon_model_changes().len(), 1);
    let snapshot = g.snapshot();
    let back = Game::restore(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..GameConfig::default() }, &snapshot).unwrap();
    assert!(back.weapon_model_changes().is_empty());
    assert_eq!(back.weapon_model_stamp(), empty);
}
