//! #3616: extender functions in a condition (a mod's CTDA) reach their named handlers
//! through `Game::condition_value`'s bridge, with parameters read by the catalogue's kinds.
use crate::common::*;
use crate::cond::{self, ParamTypes};
use crate::game::{GameConfig, RunCtx};
use crate::{Game, GameData};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::{json, Value as Json};

const QUEST: FormId = FormId(0x880);
const HEAVY_ARMOR: FormId = FormId(0x890);
const LIGHT_ARMOR: FormId = FormId(0x891);

/// The sixteen of #3616: opcode, name, parameter types in a CTDA.
const SIXTEEN: [(u16, &str, &[&str]); 16] = [
    (0x27EC, "IsSpellTargetAlt", &["ptForm"]),
    (0x27B1, "AuxVarGetFltCond", &["ptQuest", "ptInteger"]),
    (0x2264, "GetArmorClass", &["ptForm"]),
    (0x277C, "GetRadiationLevelAlt", &[]),
    (0x3165, "GetQuestFailed", &["ptForm"]),
    (0x142B, "GetWeaponIsAutomatic", &["ptForm"]),
    (0x2321, "ActorHasEffect", &["ptForm"]),
    (0x14F2, "GetEquippedWeaponModFlags", &[]),
    (0x2605, "GetIsPoisoned", &[]),
    (0x1659, "IsEquippedAmmoInList", &["ptForm"]),
    (0x146B, "IsControlPressed", &["ptInteger", "ptInteger"]),
    (0x2209, "GetLeftTrigger", &[]),
    (0x27B5, "IsInAir", &[]),
    (0x28B1, "GetNoteRead", &["ptForm"]),
    (0x165A, "GetEquippedWeaponUsesAmmoList", &["ptForm"]),
    (0x264C, "IsEquippedWeaponSilenced", &[]),
];

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
    catalogue: Json,
}

fn armor(id: FormId, edid: &str, general: u8) -> Vec<u8> {
    let mut bmdt = vec![0u8; 8];
    bmdt[0..4].copy_from_slice(&(1u32 << 2).to_le_bytes());
    bmdt[4] = general;
    record(b"ARMO", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"BMDT", bmdt)])
}

fn fixture() -> Fixture {
    let bytes = [
        tes4(),
        top(b"QUST", &record(b"QUST", 0, QUEST.0, &[sub(b"EDID", z("SomeQuest"))])),
        top(b"ARMO", &[armor(HEAVY_ARMOR, "HeavyThing", 1 << 7), armor(LIGHT_ARMOR, "LightThing", 0)].concat()),
        top(b"NPC_", &record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player"))])),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "ExtenderCtda.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let catalogue: Json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &catalogue, &refs);
    Fixture { data, forms, refs, catalogue }
}

fn game(f: &Fixture) -> Game<'_> {
    Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default())
}

/// A CTDA of `opcode` `== 1` with raw parameters `p1`, `p2`, read as the game reads one.
fn ctda(types: &ParamTypes, opcode: u16, p1: u32, p2: u32) -> cond::Condition {
    let v = json!({"type": 0, "function": opcode - 0x1000, "comparison_value_raw": 1.0f32.to_bits(), "parameter1_raw": p1, "parameter2_raw": p2});
    cond::parse(&v, types, &|local| Some(local)).unwrap()
}

#[test]
fn the_sixteen_have_condition_parameter_types() {
    let f = fixture();
    let types = ParamTypes::from_catalogue(&f.catalogue);
    for (op, name, want) in SIXTEEN {
        assert_eq!(crate::nvse_debug::opcode_of(name), Some(op as u32), "{name}'s opcode");
        let got = types.0.get(&(op - 0x1000)).unwrap_or_else(|| panic!("{name}: no parameter types"));
        assert_eq!(got, &want.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "{name}");
    }
}

#[test]
fn the_sixteen_reach_a_named_handler_and_none_is_registered() {
    let f = fixture();
    let mut g = game(&f);
    for (op, name, _) in SIXTEEN {
        assert!(crate::functions::lookup(op).is_none(), "{name} is registered: the bridge would not be asked");
        let handled = crate::nvse_debug::opcode_names(op as u32)
            .iter()
            .any(|n| crate::nvse::call(&mut g, &RunCtx::default(), &n.to_ascii_lowercase(), Some(FormId::PLAYER_REF), &[]).is_some());
        assert!(handled, "{name}: no named handler");
    }
}

#[test]
fn get_armor_class_as_a_condition_reads_the_armors_class() {
    let f = fixture();
    let types = ParamTypes::from_catalogue(&f.catalogue);
    let mut g = game(&f);
    let heavy = ctda(&types, 0x2264, HEAVY_ARMOR.0, 0);
    assert_eq!(heavy.params.len(), 1);
    assert_eq!(g.condition_value(&heavy, Some(FormId::PLAYER_REF), "test"), 3.0);
    let light = ctda(&types, 0x2264, LIGHT_ARMOR.0, 0);
    assert_eq!(g.condition_value(&light, Some(FormId::PLAYER_REF), "test"), 1.0);
    // A form that is no armour, and the player wearing none: 0.
    let quest = ctda(&types, 0x2264, QUEST.0, 0);
    assert_eq!(g.condition_value(&quest, Some(FormId::PLAYER_REF), "test"), 0.0);
}

#[test]
fn get_quest_failed_as_a_condition_follows_fail_quest() {
    let f = fixture();
    let types = ParamTypes::from_catalogue(&f.catalogue);
    let mut g = game(&f);
    let c = ctda(&types, 0x3165, QUEST.0, 0);
    assert_eq!(g.condition_value(&c, Some(FormId::PLAYER_REF), "test"), 0.0);
    crate::nvse::call(&mut g, &RunCtx::default(), "failquest", None, &[fnv_script::vm::XVal::Ref(QUEST.0)]);
    assert_eq!(g.condition_value(&c, Some(FormId::PLAYER_REF), "test"), 1.0);
}

#[test]
fn a_parameterless_one_gets_no_phantom_parameters() {
    let f = fixture();
    let types = ParamTypes::from_catalogue(&f.catalogue);
    let mut g = game(&f);
    let c = ctda(&types, 0x27B5, 7, 9);
    assert!(c.params.is_empty());
    assert_eq!(g.condition_value(&c, Some(FormId::PLAYER_REF), "test"), 0.0, "IsInAir: the player stands");
}

#[test]
fn aux_var_get_flt_cond_takes_a_quest_and_an_index() {
    let f = fixture();
    let types = ParamTypes::from_catalogue(&f.catalogue);
    let c = ctda(&types, 0x27B1, QUEST.0, 3);
    assert_eq!(c.params.len(), 2);
    assert!(matches!(c.params[0], fnv_script::vm::ArgValue::Value(fnv_script::vm::Value::Ref(q)) if q == QUEST.0));
    assert!(matches!(c.params[1], fnv_script::vm::ArgValue::Value(fnv_script::vm::Value::Num(n)) if n == 3.0));
}

#[test]
fn reaches_handler_answers_as_the_named_bridge_does() {
    let f = fixture();
    let mut g = game(&f);
    // The tools' probe (`fnvtool script coverage` / `stubs`, #1408) agrees with the
    // bridge on the sixteen, and an opcode no table names reaches nothing.
    for (op, name, _) in SIXTEEN {
        assert!(crate::nvse::reaches_handler(&mut g, &f.data.table, op), "{name}");
    }
    assert!(!crate::nvse::reaches_handler(&mut g, &f.data.table, 0xFFF0));
}
