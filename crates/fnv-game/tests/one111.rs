//! The 1111 NVSE plugin's functions (fnv_game::nvse_one111) through compiled scripts, on a
//! synthetic plugin written for the test (no game data, made-up names):
//! docs/spec-notes/ext-one111.md.

use crate::common;

use common::*;
use fnv_esm::raw::Subrecord;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig};
use fnv_game::nvse_one111::{DamageCalc, INT_MIN};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const GLOB: u32 = 0x810;
const MESG: u32 = 0x820;
const RCPE: u32 = 0x830;
const QUST: u32 = 0x840;
const WEAP: u32 = 0x850;
const EFSH: u32 = 0x860;
const AMEF_DAMAGE: u32 = 0x870;
const AMEF_DT: u32 = 0x871;
const AMMO: u32 = 0x880;
const ARMO: u32 = 0x890;

/// GetGlobalValue's condition index [documented: the catalogue, opcode 0x104A].
const GET_GLOBAL_VALUE: u16 = 0x4A;

/// A CTDA: comparison (top three bits of the type byte), value, function, parameter 1,
/// run on.
fn ctda(cmp: u8, value: f32, function: u16, p1: u32) -> Subrecord {
    let mut d = vec![cmp << 5, 0, 0, 0];
    d.extend(value.to_le_bytes());
    d.extend(function.to_le_bytes());
    d.extend([0, 0]);
    d.extend(p1.to_le_bytes());
    d.extend(0u32.to_le_bytes());
    d.extend(0u32.to_le_bytes());
    d.extend(0u32.to_le_bytes());
    sub(b"CTDA", d)
}

fn amef(id: u32, kind: u32, op: u32, value: f32) -> Vec<u8> {
    record(b"AMEF", 0, id, &[sub(b"EDID", z(&format!("Effect{id:X}"))), sub(b"DATA", [u32s(&[kind, op]), value.to_le_bytes().to_vec()].concat())])
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let glob = record(b"GLOB", 0, GLOB, &[sub(b"EDID", z("Gate")), sub(b"FNAM", vec![b's']), sub(b"FLTV", 5f32.to_le_bytes().to_vec())]);
    // Button A shows while Gate == 5; button B has no conditions.
    let mesg = record(b"MESG", 0, MESG, &[sub(b"EDID", z("Choice")), sub(b"DESC", z("Pick")), sub(b"ITXT", z("A")), ctda(0, 5.0, GET_GLOBAL_VALUE, GLOB), sub(b"ITXT", z("B"))]);
    let rcpe = record(b"RCPE", 0, RCPE, &[sub(b"EDID", z("Recipe")), ctda(3, 3.0, GET_GLOBAL_VALUE, GLOB), ctda(4, 9.0, GET_GLOBAL_VALUE, GLOB)]);
    let qust = record(b"QUST", 0, QUST, &[sub(b"EDID", z("Quest")), sub(b"DATA", vec![0; 8]), ctda(0, 1.0, GET_GLOBAL_VALUE, GLOB)]);
    let weap = record(b"WEAP", 0, WEAP, &[sub(b"EDID", z("Gun")), sub(b"DATA", vec![0; 15])]);
    let efsh = record(b"EFSH", 0, EFSH, &[sub(b"EDID", z("Glow"))]);
    let ammo = record(b"AMMO", 0, AMMO, &[sub(b"EDID", z("Rounds")), sub(b"RCIL", u32s(&[AMEF_DAMAGE])), sub(b"RCIL", u32s(&[AMEF_DT]))]);
    // DR 30 (stored ×100) and DT 5.
    let armo = record(b"ARMO", 0, ARMO, &[sub(b"EDID", z("Vest")), sub(b"DNAM", [3000i16.to_le_bytes().to_vec(), vec![0, 0], 5f32.to_le_bytes().to_vec(), vec![0; 4]].concat())]);
    let bytes = [
        tes4(),
        top(b"GLOB", &glob),
        top(b"MESG", &mesg),
        top(b"RCPE", &rcpe),
        top(b"QUST", &qust),
        top(b"WEAP", &weap),
        top(b"EFSH", &efsh),
        top(b"AMEF", &[amef(AMEF_DAMAGE, 0, 1, 1.75), amef(AMEF_DT, 2, 2, 10.0)].concat()),
        top(b"AMMO", &ammo),
        top(b"ARMO", &armo),
        top(b"NPC_", &record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))])),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "One111Test.esm".into()).unwrap()];
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
    Game::new(&f.data, world, GameConfig::default())
}

fn query(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).expect("the query runs")
}

/// The value of one expression.
fn val(g: &mut Game, expr: &str) -> XVal {
    query(g, &format!("SetFunctionValue ({expr})"))
}

fn x(v: f64) -> XVal {
    XVal::Num(v)
}

#[test]
fn condition_lists_read_the_records_and_count_what_is_wrong() {
    let f = fixture();
    let mut g = game(&f);
    // FormTwo: a message's buttons [documented: GetConditionFormTwoCount].
    assert_eq!(val(&mut g, "GetConditionFormTwoCount Choice 0"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoCount Choice 1"), x(0.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoCount Choice 7"), x(-1.0));
    // FormOne: a recipe; FormThree: a quest's own list with First −1.
    assert_eq!(val(&mut g, "GetConditionFormOneCount Recipe"), x(2.0));
    assert_eq!(val(&mut g, "GetConditionFormOneCount Gate"), x(-1.0));
    assert_eq!(val(&mut g, "GetConditionFormThreeCount Quest (-1) 0"), x(1.0));
    // Int value IDs: the opcode, the parameter types (GetGlobalValue takes a global, a
    // base form; no second parameter), the flags, the comparison, the run-on type
    // [documented: GetConditionFormOneInt].
    assert_eq!(val(&mut g, "GetConditionFormTwoInt Choice 0 0 0"), x(f64::from(0x1000 + GET_GLOBAL_VALUE)));
    assert_eq!(val(&mut g, "GetConditionFormTwoInt Choice 0 0 1"), x(3.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoInt Choice 0 0 2"), x(-1.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoInt Choice 0 0 3"), x(0.0));
    assert_eq!(val(&mut g, "GetConditionFormOneInt Recipe 0 4"), x(3.0));
    assert_eq!(val(&mut g, "GetConditionFormOneInt Recipe 1 4"), x(4.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoInt Choice 0 0 5"), x(0.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoInt Choice 0 3 0"), x(INT_MIN));
    // The value compared with; the global parameter as a form, not a number.
    assert_eq!(val(&mut g, "GetConditionFormTwoFloat Choice 0 0 0"), x(5.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoFloat Choice 0 0 1"), x(INT_MIN));
    assert_eq!(val(&mut g, "GetConditionFormTwoForm Choice 0 0 1"), XVal::Ref(GLOB));
    assert_eq!(val(&mut g, "GetConditionFormThreeFloat Quest (-1) 0 0 0"), x(1.0));
}

/// The three FormOne pages whose template carries the Int function's name, and
/// AddConditionFormTwo's `First`, compile under their own names and arguments (the
/// catalogue generator's NAME_FIXES / PARAM_FIXES rows).
#[test]
fn renamed_catalogue_rows_compile_and_answer() {
    let f = fixture();
    let mut g = game(&f);
    // The value compared with; the global parameter is a form, not a number
    // [documented: GetConditionFormOneFloat].
    assert_eq!(val(&mut g, "GetConditionFormOneFloat Recipe 0 0"), x(3.0));
    assert_eq!(val(&mut g, "GetConditionFormOneFloat Recipe 0 1"), x(INT_MIN));
    assert_eq!(val(&mut g, "SetConditionFormOneFloat Recipe 0 0 4"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormOneFloat Recipe 0 0"), x(4.0));
    // A parameter that is a form takes a form [documented: SetConditionFormOneForm].
    assert_eq!(val(&mut g, "SetConditionFormOneForm Recipe 0 1 Gate"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormOneForm Recipe 0 1"), XVal::Ref(GLOB));
    // `entity first cond`: before button 0's only condition, then at its end.
    assert_eq!(val(&mut g, "AddConditionFormTwo Choice 0 0"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoCount Choice 0"), x(2.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoInt Choice 0 0 0"), x(4136.0));
    assert_eq!(val(&mut g, "AddConditionFormTwo Choice 0 (-1)"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoCount Choice 0"), x(3.0));
    assert_eq!(val(&mut g, "AddConditionFormTwo Choice 0 5"), x(0.0));
}

#[test]
fn condition_lists_change_and_evaluate() {
    let f = fixture();
    let mut g = game(&f);
    // Gate is 5: the button's condition passes, the recipe's (>= 3 and < 9) too.
    assert_eq!(val(&mut g, "EvalConditionListFormTwo Choice 0 PlayerRef PlayerRef"), x(1.0));
    assert_eq!(val(&mut g, "EvalConditionListFormOne Recipe PlayerRef PlayerRef"), x(1.0));
    assert_eq!(val(&mut g, "EvalConditionListFormThree Quest (-1) 0 PlayerRef PlayerRef"), x(0.0));
    // A new compared value: Gate == 6 fails.
    assert_eq!(val(&mut g, "SetConditionFormTwoFloat Choice 0 0 0 6"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoFloat Choice 0 0 0"), x(6.0));
    assert_eq!(val(&mut g, "EvalConditionListFormTwo Choice 0 PlayerRef PlayerRef"), x(0.0));
    // != makes it pass; the parameter types cannot be set [documented].
    assert_eq!(val(&mut g, "SetConditionFormTwoInt Choice 0 0 4 1"), x(1.0));
    assert_eq!(val(&mut g, "EvalConditionListFormTwo Choice 0 PlayerRef PlayerRef"), x(1.0));
    assert_eq!(val(&mut g, "SetConditionFormTwoInt Choice 0 0 1 0"), x(0.0));
    assert_eq!(val(&mut g, "SetConditionFormTwoInt Choice 0 0 4 9"), x(0.0));
    // Use Global on: the global is the comparison's; a number can only clear it.
    assert_eq!(val(&mut g, "SetConditionFormTwoInt Choice 0 0 3 4"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoInt Choice 0 0 3"), x(4.0));
    assert_eq!(val(&mut g, "SetConditionFormTwoFloat Choice 0 0 0 2"), x(0.0));
    assert_eq!(val(&mut g, "SetConditionFormTwoForm Choice 0 0 0 Gate"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoForm Choice 0 0 0"), XVal::Ref(GLOB));
    // Gate != Gate fails.
    assert_eq!(val(&mut g, "EvalConditionListFormTwo Choice 0 PlayerRef PlayerRef"), x(0.0));
    // Add: GetVampire == 1 on the subject, at the end or before an existing one
    // [documented: AddConditionFormOne].
    assert_eq!(val(&mut g, "AddConditionFormTwo Choice 1"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoCount Choice 1"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoInt Choice 1 0 0"), x(4136.0));
    assert_eq!(val(&mut g, "GetConditionFormTwoFloat Choice 1 0 0"), x(1.0));
    assert_eq!(val(&mut g, "AddConditionFormOne Recipe 1"), x(1.0));
    assert_eq!(val(&mut g, "AddConditionFormOne Recipe 9"), x(0.0));
    assert_eq!(val(&mut g, "GetConditionFormOneCount Recipe"), x(3.0));
    assert_eq!(val(&mut g, "GetConditionFormOneInt Recipe 1 0"), x(4136.0));
    // Swap and delete: the others close the gap.
    assert_eq!(val(&mut g, "SwapConditionFormOne Recipe 0 1"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormOneInt Recipe 0 0"), x(4136.0));
    assert_eq!(val(&mut g, "SwapConditionFormOne Recipe 0 5"), x(0.0));
    assert_eq!(val(&mut g, "DeleteConditionFormOne Recipe 0"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormOneCount Recipe"), x(2.0));
    assert_eq!(val(&mut g, "GetConditionFormOneInt Recipe 0 4"), x(3.0));
    assert_eq!(val(&mut g, "DeleteConditionFormThree Quest (-1) 0 0"), x(1.0));
    assert_eq!(val(&mut g, "GetConditionFormThreeCount Quest (-1) 0"), x(0.0));
    // An empty list passes.
    assert_eq!(val(&mut g, "EvalConditionListFormThree Quest (-1) 0 PlayerRef PlayerRef"), x(1.0));
    // A new function (GetGlobalValue again): its global parameter is set as a form.
    assert_eq!(val(&mut g, &format!("SetConditionFormTwoInt Choice 1 0 0 {}", 0x1000 + GET_GLOBAL_VALUE)), x(1.0));
    assert_eq!(val(&mut g, "SetConditionFormTwoForm Choice 1 0 1 Gate"), x(1.0));
    assert_eq!(val(&mut g, "SetConditionFormTwoFloat Choice 1 0 0 5"), x(1.0));
    assert_eq!(val(&mut g, "EvalConditionListFormTwo Choice 1 PlayerRef PlayerRef"), x(1.0));
    assert_eq!(val(&mut g, "SetConditionFormTwoInt Choice 1 0 0 99999"), x(0.0));
    // An index past i32 is no list, not button 0 wrapped round.
    assert_eq!(val(&mut g, "SetConditionFormTwoInt Choice (2 ^ 32) 0 4 1"), x(0.0));
}

#[test]
fn damage_event_values_read_inside_the_event_only() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(val(&mut g, "GetOnDamageCalcNumb 0"), x(INT_MIN));
    // Outside an event [observed: one111].
    assert_eq!(val(&mut g, "GetOnDamageCalcAdditionalNumb 0"), x(0.0));
    assert_eq!(val(&mut g, "GetOnDamageCalcFlag"), x(-1.0));
    assert_eq!(val(&mut g, "GetOnDamageCalcRef 1"), XVal::Ref(0));
    assert_eq!(val(&mut g, "SetOnDamageCalcNumb 0 10"), x(0.0));
    let mut d = DamageCalc { flags: 4, ..DamageCalc::default() };
    d.numbers[0] = Some(22.0);
    d.refs[1] = Some(FormId::PLAYER_REF);
    g.one111_damage_calc_begin(d);
    assert_eq!(val(&mut g, "GetOnDamageCalcNumb 0"), x(22.0));
    assert_eq!(val(&mut g, "GetOnDamageCalcNumb 4"), x(INT_MIN));
    assert_eq!(val(&mut g, "GetOnDamageCalcFlag"), x(4.0));
    assert_eq!(val(&mut g, "GetOnDamageCalcRef 1"), XVal::Ref(FormId::PLAYER_REF.0));
    assert_eq!(val(&mut g, "GetOnDamageCalcAdditionalNumb 2"), x(INT_MIN));
    assert_eq!(val(&mut g, "SetOnDamageCalcNumb 0 10"), x(1.0));
    let out = g.one111_damage_calc_end().unwrap();
    assert_eq!(out.numbers[0], Some(10.0));
}

#[test]
fn weapon_stat_vectors_interpolate_between_pairs() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(val(&mut g, "GetWeapAdditionalVectorValueScale Gun 0 0.7"), x(-1.0));
    // An empty vector gives no array [observed: one111].
    assert_eq!(query(&mut g, "array_var a\nlet a := GetWeapAdditionalVectorArray Gun 0\nSetFunctionValue (ar_Size a)"), x(-1.0));
    // The page's example: (0.9; 1) and (0.5; 0.2) [documented].
    assert_eq!(val(&mut g, "SetWeapAdditionalVectorValue Gun 0 0.9 1"), x(1.0));
    assert_eq!(val(&mut g, "SetWeapAdditionalVectorValue Gun 0 0.5 0.2"), x(1.0));
    assert_eq!(val(&mut g, "SetWeapAdditionalVectorValue Gun 0 0.5 -1"), x(0.0));
    // Not a number, or infinite: refused, nothing stored.
    assert_eq!(val(&mut g, "SetWeapAdditionalVectorValue Gun 2 0.5 ((-1) ^ 0.5)"), x(0.0));
    assert_eq!(val(&mut g, "SetWeapAdditionalVectorValue Gun 2 0.5 (10 ^ 400)"), x(0.0));
    assert_eq!(val(&mut g, "SetWeapAdditionalVectorValue Gun 9 0.5 1"), x(0.0));
    assert_eq!(val(&mut g, "SetWeapAdditionalVectorValue Glow 0 0.5 1"), x(0.0));
    assert_eq!(val(&mut g, "GetWeapAdditionalVectorValue Gun 0 0.5"), x(0.2));
    assert_eq!(val(&mut g, "GetWeapAdditionalVectorValue Gun 0 0.7"), x(-1.0));
    let mid = val(&mut g, "GetWeapAdditionalVectorValueScale Gun 0 0.7");
    let XVal::Num(m) = mid else { panic!("{mid:?}") };
    assert!((m - 0.6).abs() < 1e-9, "{m}");
    assert_eq!(val(&mut g, "GetWeapAdditionalVectorValueScale Gun 0 1"), x(1.0));
    assert_eq!(val(&mut g, "GetWeapAdditionalVectorValueScale Gun 0 0.1"), x(0.2));
    assert_eq!(query(&mut g, "array_var a\nlet a := GetWeapAdditionalVectorArray Gun 0\nSetFunctionValue (ar_Size a)"), x(2.0));
    assert_eq!(query(&mut g, "array_var a\nlet a := GetWeapAdditionalVectorArray Gun 0\nSetFunctionValue a[1]"), x(0.9));
    assert_eq!(val(&mut g, "DeleteWeapAdditionalVectorValue Gun 0 0.5"), x(1.0));
    assert_eq!(val(&mut g, "DeleteWeapAdditionalVectorValue Gun 0 0.5"), x(0.0));
    assert_eq!(val(&mut g, "GetWeapAdditionalVectorValueScale Gun 0 0.1"), x(1.0));
    assert_eq!(val(&mut g, "GetWeapAdditionalFlag Gun -1"), x(-1.0));
}

#[test]
fn ammo_effects_and_weapon_fields() {
    let f = fixture();
    let mut g = game(&f);
    // ×1.75 damage, −10 DT [documented: ApplyAmmoEffects' examples].
    assert_eq!(val(&mut g, "ApplyAmmoEffects Rounds 0 2"), x(3.5));
    assert_eq!(val(&mut g, "ApplyAmmoEffects Rounds 2 30"), x(20.0));
    assert_eq!(val(&mut g, "ApplyAmmoEffects Rounds 3 30"), x(30.0));
    // One111's extra effect type, read back by GetAmmoEffectTraitNumeric.
    assert_eq!(val(&mut g, "SetAmmoEffectTraitNumericAlt Effect870 0 8"), x(1.0));
    assert_eq!(val(&mut g, "SetAmmoEffectTraitNumericAlt Effect870 0 10"), x(0.0));
    assert_eq!(val(&mut g, "SetAmmoEffectTraitNumericAlt Effect870 1 3"), x(0.0));
    assert_eq!(val(&mut g, "SetAmmoEffectTraitNumericAlt Effect870 2 ((-1) ^ 0.5)"), x(0.0));
    assert_eq!(val(&mut g, "GetAmmoEffectTraitNumeric Effect870 0"), x(8.0));
    assert_eq!(val(&mut g, "ApplyAmmoEffects Rounds 0 2"), x(2.0));
    assert_eq!(val(&mut g, "ApplyAmmoEffects Rounds 8 2"), x(3.5));
    // Mod effects and second values, read back by the xNVSE getters.
    assert_eq!(val(&mut g, "SetWeaponItemModEffectAlt Gun 2 26"), x(1.0));
    assert_eq!(val(&mut g, "SetWeaponItemModEffectAlt Gun 4 1"), x(0.0));
    assert_eq!(val(&mut g, "GetWeaponItemModEffect 2 Gun"), x(26.0));
    assert_eq!(val(&mut g, "SetWeaponItemModValue2 Gun 2 6"), x(1.0));
    assert_eq!(val(&mut g, "GetWeaponItemModValue2 2 Gun"), x(6.0));
    // The scope's effect shader.
    assert_eq!(val(&mut g, "GetWeaponScopeEffectShader Gun"), XVal::Ref(0));
    assert_eq!(val(&mut g, "SetWeaponScopeEffectShader Gun Gate"), x(0.0));
    assert_eq!(val(&mut g, "SetWeaponScopeEffectShader Gun Glow"), x(1.0));
    assert_eq!(val(&mut g, "GetWeaponScopeEffectShader Gun"), XVal::Ref(EFSH));
}

#[test]
fn worn_armor_adds_up() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(val(&mut g, "PlayerRef.GetActorTotalArmorDT"), x(0.0));
    let inv = g.world.inventory_mut(FormId::PLAYER_REF).unwrap();
    inv.add(FormId(ARMO), 1);
    inv.set_equipped(FormId(ARMO));
    assert_eq!(val(&mut g, "PlayerRef.GetActorTotalArmorDT"), x(5.0));
    // In the unit GetArmorArmorRating reads.
    let rating = val(&mut g, "GetArmorArmorRating Vest");
    assert_eq!(val(&mut g, "PlayerRef.GetActorTotalArmorDR"), rating);
    // Nothing equipped, no spread (see docs/spec-notes/ext-one111.md).
    assert_eq!(val(&mut g, "PlayerRef.GetCalculatedSpreadAlt 2"), x(0.0));
}

#[test]
fn a_changed_button_list_hides_the_button() {
    let f = fixture();
    let mut g = game(&f);
    assert!(g.hidden_buttons(FormId(MESG), None).is_empty());
    assert_eq!(val(&mut g, "SetConditionFormTwoFloat Choice 0 0 0 6"), x(1.0));
    assert_eq!(g.hidden_buttons(FormId(MESG), None), vec![0]);
}
