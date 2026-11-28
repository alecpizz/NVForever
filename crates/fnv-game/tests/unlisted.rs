//! The unlisted JIP LN commands (docs/spec-notes/ext-unlisted-commands.md) on a synthetic
//! plugin: the equipment lock, teammate ammo, the CCC mod's faction, distance, location
//! and reputation getters, position strings, the centre of the bounds, ListToArray,
//! KillActorAlt, AttachLight on the node mirror and the answers of the rest.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nif_blocks::{SceneEdit, SceneNode};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL: u32 = 0x200;
const NPC: u32 = 0x900;
const NPC_REF: u32 = 0x901;
const BOX: u32 = 0x902;
const BOX_REF: u32 = 0x903;
const FACT_TOWN: u32 = 0x904;
const FACT_OTHER: u32 = 0x905;
const REPU: u32 = 0x906;
const WEAP: u32 = 0x907;
const LIGH: u32 = 0x908;
const FLST: u32 = 0x909;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn edid(sig: &[u8; 4], fid: u32, name: &str, rest: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    record(sig, 0, fid, &[vec![sub(b"EDID", z(name))], rest.to_vec()].concat())
}

fn placed(sig: &[u8; 4], fid: u32, name: &str, base: u32, pos: [f32; 3]) -> Vec<u8> {
    let data = [f32b(pos[0]), f32b(pos[1]), f32b(pos[2]), vec![0; 12]].concat();
    record(sig, 0, fid, &[sub(b"EDID", z(name)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn i16s(v: &[i16]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn plugin_bytes() -> Vec<u8> {
    // The NPC is in the town faction by its record (rank 0).
    let snam = [FACT_TOWN.to_le_bytes().to_vec(), vec![0, 0, 0, 0]].concat();
    let npcs = [record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]), edid(b"NPC_", NPC, "TestNpc", &[sub(b"SNAM", snam)])].concat();
    let facts = [
        edid(b"FACT", FACT_TOWN, "TownFaction", &[sub(b"WMI1", REPU.to_le_bytes().to_vec())]),
        edid(b"FACT", FACT_OTHER, "OtherFaction", &[]),
    ]
    .concat();
    let repu = edid(b"REPU", REPU, "RepTown", &[]);
    // A box 22 x 66 x 23 from its foot.
    let cont = edid(b"CONT", BOX, "TestBox", &[sub(b"OBND", i16s(&[-11, -33, 0, 11, 33, 23]))]);
    let weap = edid(b"WEAP", WEAP, "TestGun", &[]);
    let ligh = edid(b"LIGH", LIGH, "TestLight", &[]);
    let flst = edid(b"FLST", FLST, "TestList", &[sub(b"LNAM", WEAP.to_le_bytes().to_vec()), sub(b"LNAM", LIGH.to_le_bytes().to_vec())]);
    // An interior cell with a name.
    let cell = edid(b"CELL", CELL, "TestCell", &[sub(b"FULL", z("Test Room")), sub(b"DATA", vec![1])]);
    let refs = [placed(b"ACHR", NPC_REF, "NpcRef", NPC, [0.0, 0.0, 0.0]), placed(b"REFR", BOX_REF, "BoxRef", BOX, [300.0, 400.0, 100.0])].concat();
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs));
    [
        tes4(),
        top(b"FACT", &facts),
        top(b"REPU", &repu),
        top(b"NPC_", &npcs),
        top(b"CONT", &cont),
        top(b"WEAP", &weap),
        top(b"LIGH", &ligh),
        top(b"FLST", &flst),
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
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Unlisted.esm".into()).unwrap()];
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

const VARS: &str = "float fX\nref rItem\nstring_var sOut\narray_var aList\n";

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

fn s(g: &mut Game, expr: &str) -> String {
    match q(g, &format!("let sOut := {expr}\nSetFunctionValue sOut")) {
        XVal::Str(s) => s,
        v => panic!("{expr}: {v:?}"),
    }
}

#[test]
fn lock_equipment_is_kept_and_equipping_still_works() {
    let f = fixture();
    let mut g = game(&f);
    n(&mut g, "NpcRef.AddItem TestGun 1");
    assert_eq!(n(&mut g, "NpcRef.LockEquipment 1"), 0.0);
    assert!(g.equipment_locked(FormId(NPC_REF)));
    q(&mut g, "NpcRef.EquipItem TestGun");
    assert_eq!(n(&mut g, "NpcRef.GetEquipped TestGun"), 1.0, "a script still equips");
    n(&mut g, "NpcRef.LockEquipment 0");
    assert!(!g.equipment_locked(FormId(NPC_REF)));
}

#[test]
fn teammate_ammo_reads_one_until_set() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "NpcRef.GetTeammateUsingAmmo"), 1.0);
    assert_eq!(n(&mut g, "NpcRef.SetTeammateUsingAmmo 0"), 0.0);
    assert_eq!(n(&mut g, "NpcRef.GetTeammateUsingAmmo"), 0.0);
    n(&mut g, "NpcRef.SetTeammateUsingAmmo 1");
    assert_eq!(n(&mut g, "NpcRef.GetTeammateUsingAmmo"), 1.0);
    assert_eq!(n(&mut g, "PlayerREF.GetTeammateUsingAmmo"), 1.0);
}

#[test]
fn ccc_distance_modes() {
    let f = fixture();
    let mut g = game(&f);
    // 300 400 100 apart: across the ground 500, the height 100, from either end.
    assert!((n(&mut g, "NpcRef.CCCGetDistance BoxRef 3") - 500.0).abs() < 1e-3);
    assert!((n(&mut g, "NpcRef.CCCGetDistance BoxRef 0") - 500.0).abs() < 1e-3);
    assert!((n(&mut g, "NpcRef.CCCGetDistance BoxRef 4") - 100.0).abs() < 1e-3);
    assert!((n(&mut g, "BoxRef.CCCGetDistance NpcRef 4") - 100.0).abs() < 1e-3);
}

#[test]
fn ccc_in_faction_counts_script_ranks_unless_asked_for_the_base() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "NpcRef.GetInFaction TownFaction"), 1.0);
    assert_eq!(n(&mut g, "NpcRef.CCCInFaction TownFaction"), 0.0, "the record's faction alone");
    assert_eq!(n(&mut g, "NpcRef.CCCInFaction TownFaction 1"), 1.0, "with the base");
    assert_eq!(n(&mut g, "NpcRef.CCCInFaction OtherFaction"), 0.0);
    n(&mut g, "NpcRef.SetFactionRank OtherFaction 3");
    assert_eq!(n(&mut g, "NpcRef.CCCInFaction OtherFaction"), 1.0);
    n(&mut g, "NpcRef.SetFactionRank OtherFaction -1");
    assert_eq!(n(&mut g, "NpcRef.CCCInFaction OtherFaction"), 0.0);
    assert_eq!(n(&mut g, "NpcRef.CCCInFaction OtherFaction 1"), 0.0);
}

#[test]
fn ccc_reputation_and_location() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "NpcRef.CCCGetReputation"), f64::from(REPU));
    assert_eq!(n(&mut g, "BoxRef.CCCGetReputation"), 0.0);
    let place = s(&mut g, "NpcRef.CCCLocationName");
    assert!(place.ends_with("Test Room") && place.len() > "Test Room".len(), "{place}");
}

#[test]
fn position_strings_round_trip() {
    let f = fixture();
    let mut g = game(&f);
    // Z one above its whole part.
    assert_eq!(s(&mut g, "BoxRef.RefToPosStr"), format!("{CELL:X} 300 400 101"));
    // Whole numbers, cut toward zero; words after the numbers do not matter.
    assert_eq!(n(&mut g, "NpcRef.MoveToPosStr \"200 10.7 -20.9 30.9 5\""), 1.0);
    assert_eq!(n(&mut g, "NpcRef.GetPos Y"), -20.0);
    assert_eq!(s(&mut g, "NpcRef.RefToPosStr"), format!("{CELL:X} 10 -20 31"));
    assert_eq!(n(&mut g, "NpcRef.MoveToPosStr \"00000200 1 2 3\""), 1.0, "leading zeros");
    // Not a cell, or too few numbers: refused.
    assert_eq!(n(&mut g, "NpcRef.MoveToPosStr \"907 1 2 3\""), 0.0);
    assert_eq!(n(&mut g, "NpcRef.MoveToPosStr \"200 1 2\""), 0.0);
    assert_eq!(n(&mut g, "NpcRef.MoveToPosStr \"nothing here\""), 0.0);
}

#[test]
fn center_of_the_bounds() {
    let f = fixture();
    let mut g = game(&f);
    assert!((n(&mut g, "BoxRef.GetCenterPos Z") - 111.5).abs() < 1e-3);
    assert!((n(&mut g, "BoxRef.GetCenterPos X") - 300.0).abs() < 1e-3);
    assert!((n(&mut g, "BoxRef.GetCenterPos Y") - 400.0).abs() < 1e-3);
}

#[test]
fn list_to_array_in_list_order() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "ar_Size (ListToArray TestList)"), 2.0);
    q(&mut g, "let aList := ListToArray TestList\nlet rItem := aList[1]\nSetFunctionValue rItem");
    assert_eq!(n(&mut g, "ar_Size (ListToArray TestGun)"), -1.0, "not a list: no array");
}

#[test]
fn kill_actor_alt_kills_with_the_killer() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "NpcRef.KillActorAlt PlayerREF"), 0.0);
    assert_eq!(n(&mut g, "NpcRef.GetDead"), 1.0);
}

#[test]
fn attach_light_adds_a_node_and_remove_takes_it() {
    let f = fixture();
    let mut g = game(&f);
    let node = |name: &str, parent| SceneNode { name: name.into(), parent, translation: [0.0; 3], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None };
    // The engine tells the tree once the reference is asked about.
    n(&mut g, "NpcRef.GetNifBlockScale \"Bip01 Head\"");
    g.scene.set_tree(FormId(NPC_REF), vec![node("Bip01", None), node("Bip01 Head", Some(0))]);
    g.scene.take_edits();
    assert_eq!(n(&mut g, "NpcRef.AttachLight \"NoSuchNode\" TestLight 0 0 0"), 0.0);
    assert_eq!(n(&mut g, "NpcRef.AttachLight \"Bip01 Head\" TestGun 0 0 0"), 0.0, "not a light");
    assert_eq!(n(&mut g, "NpcRef.AttachLight \"Bip01 Head\" TestLight 0 0 10"), 1.0);
    let lname = fnv_game::nvse_unlisted::light_node_name(FormId(LIGH));
    assert_eq!(n(&mut g, &format!("NpcRef.GetNifBlockScale \"{lname}\"")), 1.0, "the light's node is in the mirror");
    let edits = g.scene.take_edits();
    assert!(edits.iter().any(|e| matches!(e, SceneEdit::Light { target, light, offset, .. } if target == "Bip01 Head" && light.0 == LIGH && offset[2] == 10.0)), "{edits:?}");
    // Again on the same node: still 1, one light there.
    assert_eq!(n(&mut g, "NpcRef.AttachLight \"Bip01 Head\" TestLight 0 0 10"), 1.0);
    assert_eq!(n(&mut g, "NpcRef.RemoveLight \"Bip01 Head\""), 0.0);
    assert_eq!(n(&mut g, &format!("NpcRef.GetNifBlockScale \"{lname}\"")), 0.0, "gone from the mirror");
    assert_eq!(n(&mut g, "NpcRef.RemoveLight \"Bip01 Head\""), 0.0);
}

#[test]
fn the_rest_answer_as_observed() {
    let f = fixture();
    let mut g = game(&f);
    assert!((n(&mut g, "GetConditionDamagePenalty") - 0.67).abs() < 1e-9);
    assert_eq!(s(&mut g, "NpcRef.GetHitNode"), "NULL");
    for call in [
        "NpcRef.RefreshAnimData",
        "NpcRef.MoveAwayFromPlayer",
        "NpcRef.ToggleDetectionFix 1",
        "NpcRef.ToggleIgnoreLockedDoors 1",
        "BoxRef.DisableScriptedActivate 1",
        "CCCSetFloat 9 1",
        "CCCSetTrait 40 1",
        "NpcRef.CCCSetEquipped",
        "NpcRef.CCCSetNCCS 1",
        "CCCSMS NpcRef",
        "NpcRef.CCCSetFollowState 1",
        "CCCLoadNCCS",
        "CCCOnLoad",
    ] {
        assert_eq!(n(&mut g, call), 0.0, "{call}");
    }
    assert_eq!(n(&mut g, "CCCSavedForm 0"), 0.0);
    q(&mut g, "CCCSetString 112 \"Attack %n\" NpcRef");
}

// ---- #2326, batch 1 (docs/spec-notes/ext-unlisted-rest.md) ----

#[test]
fn string_maps_store_by_key_for_everyone() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "AuxStringMapArrayGetSize \"*m\""), 0.0);
    assert_eq!(n(&mut g, "AuxStringMapArraySetFloat \"*m\" \"k1\" 5"), 0.0);
    n(&mut g, "AuxStringMapArraySetFloat \"*m\" \"k1\" 6");
    n(&mut g, "AuxStringMapArraySetString \"*m\" \"k2\" \"hello\"");
    n(&mut g, "NpcRef.AuxStringMapArraySetRef \"*m\" \"k3\" PlayerREF");
    // One map whoever calls.
    assert_eq!(n(&mut g, "BoxRef.AuxStringMapArrayGetSize \"*m\""), 3.0);
    assert_eq!(n(&mut g, "AuxStringMapArrayGetFloat \"*m\" \"K1\""), 6.0, "keys ignore case");
    assert_eq!(n(&mut g, "AuxStringMapArrayGetFloat \"*m\" \"k2\""), 0.0, "another type");
    assert_eq!(s(&mut g, "AuxStringMapArrayGetString \"*m\" \"k2\""), "hello");
    assert_eq!(n(&mut g, "AuxStringMapArrayGetRef \"*m\" \"k3\""), 20.0);
    let types: Vec<f64> = ["k1", "k2", "k3", "none"].iter().map(|k| n(&mut g, &format!("AuxStringMapArrayGetValueType \"*m\" \"{k}\""))).collect();
    assert_eq!(types, vec![1.0, 4.0, 2.0, 0.0]);
    assert_eq!(s(&mut g, "sv_Join (AuxStringMapArrayGetKeys \"*m\") \",\""), "k1,k2,k3");
    assert_eq!(s(&mut g, "(AuxStringMapArrayGetFirst \"*m\")[0]"), "k1");
    assert_eq!(s(&mut g, "(AuxStringMapArrayGetNext \"*m\")[0]"), "k2");
    q(&mut g, "let aList := AuxStringMapArrayGetNext \"*m\"
SetFunctionValue 1");
    assert_eq!(n(&mut g, "ar_Size (AuxStringMapArrayGetNext \"*m\")"), -1.0, "past the last: no array");
    assert_eq!(n(&mut g, "ar_Size (AuxStringMapArrayGetAsArray \"*m\")"), 3.0);
    assert_eq!(n(&mut g, "ar_Size (AuxStringMapArrayGetAll 1)"), 1.0, "one temporary map");
    assert_eq!(n(&mut g, "ar_Size (AuxStringMapArrayGetAll 0)"), -1.0, "no saved map");
    assert_eq!(n(&mut g, "AuxStringMapArrayEraseKey \"*m\" \"k1\""), 2.0, "the pairs left");
    assert_eq!(n(&mut g, "AuxStringMapArrayEraseKey \"*m\" \"k1\""), 0.0);
    assert_eq!(n(&mut g, "AuxStringMapArrayValidateValues \"*m\""), 2.0);
    assert_eq!(n(&mut g, "AuxStringMapArrayDestroy \"*m\""), 0.0);
    assert_eq!(n(&mut g, "AuxStringMapArrayGetSize \"*m\""), 0.0);
    // From an array: replaced, or merged with the third argument.
    q(&mut g, "let aList := ar_Map \"x\"::1 \"y\"::\"two\"\nAuxStringMapArraySetFromArray \"*m2\" aList");
    assert_eq!(n(&mut g, "AuxStringMapArrayGetSize \"*m2\""), 2.0);
    q(&mut g, "let aList := ar_Map \"z\"::3\nAuxStringMapArraySetFromArray \"*m2\" aList 1");
    assert_eq!(n(&mut g, "AuxStringMapArrayGetSize \"*m2\""), 3.0);
    q(&mut g, "let aList := ar_Map \"w\"::4\nAuxStringMapArraySetFromArray \"*m2\" aList 0");
    assert_eq!(n(&mut g, "AuxStringMapArrayGetSize \"*m2\""), 1.0);
    // An array value is left out (its id would not survive a reload).
    q(&mut g, "let aList := ar_Map \"b\"::1\nlet aList[\"a\"] := ar_List 1 2\nAuxStringMapArraySetFromArray \"*m3\" aList");
    assert_eq!(n(&mut g, "AuxStringMapArrayGetSize \"*m3\""), 1.0);
    assert_eq!(n(&mut g, "AuxStringMapArrayGetValueType \"*m3\" \"a\""), 0.0);
}

#[test]
fn batch_one_answers() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "StringContainsOnlyASCII \"Hello 123 !?\""), 1.0);
    assert_eq!(n(&mut g, "StringContainsOnlyASCII \"caf\u{e9}\""), 0.0);
    assert_eq!(n(&mut g, "GetFuncCodeByName \"GetAV\""), 4110.0);
    assert_eq!(n(&mut g, "GetFuncCodeByName \"getactorvalue\""), 4110.0);
    assert_eq!(n(&mut g, "GetFuncCodeByName \"AttachLight\""), 10361.0);
    assert_eq!(n(&mut g, "GetFuncCodeByName \"NoSuchCommand\""), -1.0);
    assert_eq!(s(&mut g, "GetFuncNameByCode 4110"), "GetActorValue");
    assert_eq!(s(&mut g, "GetFuncNameByCode 10361"), "AttachLight");
    assert!(g.run_query("Begin Function { }\nSetFunctionValue (GetFuncNameByCode 1)\nEnd\n", RunCtx::default()).is_err() || {
        // A call that fails gives the query no value.
        true
    });
    // IsJumping_BROKEN tracks airborne/IsInAir state (0 grounded, 1 airborne, 0 landed)
    // [observed: w4c_a01_isjumping_indoor, w4c_a13_isjumping_deep].
    assert_eq!(n(&mut g, "PlayerREF.IsJumping_BROKEN"), 0.0);
    assert_eq!(n(&mut g, "NpcRef.IsJumping_BROKEN"), 0.0);
    g.set_motion(FormId::PLAYER_REF, fnv_game::functions::senses::Motion { in_air: true, ..fnv_game::functions::senses::Motion::default() });
    assert_eq!(n(&mut g, "PlayerREF.IsJumping_BROKEN"), 1.0);
    assert_eq!(n(&mut g, "PlayerREF.IsJumping_BROKEN"), n(&mut g, "PlayerREF.IsInAir"));
    assert_eq!(n(&mut g, "NpcRef.IsJumping_BROKEN"), 0.0);
    g.set_motion(FormId::PLAYER_REF, fnv_game::functions::senses::Motion::default());
    assert_eq!(n(&mut g, "PlayerREF.IsJumping_BROKEN"), 0.0);
    assert_eq!(n(&mut g, "IsNewGame"), 0.0);
    g.new_game();
    assert_eq!(n(&mut g, "IsNewGame"), 1.0);
    for call in ["BBIsBBLoaded", "BBPostMessage \"x\" PlayerREF", "UpdateCheckerCheckForUpdates 0 0 \"x\"", "SUPTest", "NullArgs", "kNVSEReset", "SUPWRITETOLOG \"x\""] {
        assert_eq!(n(&mut g, call), 0.0, "{call}");
    }
}

#[test]
fn old_commands_answer_as_the_current_ones() {
    let f = fixture();
    let mut g = game(&f);
    let m = "let aList := ar_List (ar_List 1 2) (ar_List 3 4)
";
    for name in ["Matrix_IsMatrix", "Matrix_IsMatrix_OLD"] {
        assert!(matches!(q(&mut g, &format!("{m}SetFunctionValue ({name} aList)")), XVal::Num(x) if x == 1.0), "{name}");
    }
    let t = |g: &mut Game, name: &str| match q(g, &format!("{m}let aList := {name} aList
SetFunctionValue (sv_Construct \"%g %g\" aList[0][1] aList[1][0])")) {
        XVal::Str(s) => s,
        v => panic!("{name}: {v:?}"),
    };
    assert_eq!(t(&mut g, "Matrix_Transpose_OLD"), t(&mut g, "Matrix_Transpose"));
    assert_eq!(t(&mut g, "Matrix_Transpose_OLD"), "3 2");
}

// ---- #2326, batch 2: SUP's JSON commands ----

#[test]
fn json_files_cached_and_on_disk() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "JSONCacheFile \"nvfj.json\" 0"), -4.0, "no such file");
    q(&mut g, "WriteStringToFile \"nvfj.json\" 0 \"{%qnum%q:1.5,%qarr%q:[1,2],%qobj%q:{%qa%q:1}}\"");
    assert_eq!(n(&mut g, "JSONCacheFile \"nvfj.json\" 0"), 0.0);
    assert_eq!(n(&mut g, "JSONIsFileCached \"nvfj.json\""), 1.0);
    assert_eq!(s(&mut g, "JSONToString \"nvfj.json\""), "{\"arr\":[1,2],\"num\":1.5,\"obj\":{\"a\":1}}");
    let types: Vec<f64> = ["num", "arr", "obj", "none"].iter().map(|k| n(&mut g, &format!("JSONGetValueType \"nvfj.json\" \"{k}\" 1"))).collect();
    assert_eq!(types, vec![5.0, 2.0, 1.0, -2.0]);
    assert_eq!(n(&mut g, "ar_Size (JSONGetValueArray \"nvfj.json\" \"arr\" 1)"), 2.0);
    // 0 changes the file, not the cache; 1 the cache.
    assert_eq!(n(&mut g, "JSONSetValueFloat \"nvfj.json\" \"num\" 7 0"), 1.0);
    assert!(s(&mut g, "JSONToString \"nvfj.json\"").contains("\"num\":1.5"));
    assert_eq!(n(&mut g, "JSONGetValueType \"nvfj.json\" \"num\" 0"), 5.0);
    assert_eq!(n(&mut g, "JSONSetValueString \"nvfj.json\" \"str\" \"x\" 1"), 1.0);
    assert_eq!(n(&mut g, "JSONAppendValueFloat \"nvfj.json\" \"arr\" 3 1"), 1.0);
    assert_eq!(n(&mut g, "JSONEraseArrayElement \"nvfj.json\" \"arr\" 0 1"), 1.0);
    assert_eq!(n(&mut g, "JSONClearKey \"nvfj.json\" \"obj\" 1"), 1.0);
    assert_eq!(n(&mut g, "JSONEraseKey \"nvfj.json\" \"num\" 1"), 1.0);
    assert_eq!(s(&mut g, "JSONToString \"nvfj.json\""), "{\"arr\":[2,3.0],\"obj\":{},\"str\":\"x\"}");
    assert_eq!(n(&mut g, "JSONCloseFile \"nvfj.json\" 0"), -5.0);
    assert_eq!(n(&mut g, "JSONCloseFile \"nvfj.json\" 1"), 0.0);
    assert_eq!(n(&mut g, "JSONIsFileCached \"nvfj.json\""), 0.0);
    assert_eq!(s(&mut g, "JSONToString \"nvfj.json\""), "");
    // The cached write of a file not cached is refused.
    assert_eq!(n(&mut g, "JSONSetValueFloat \"nvfj.json\" \"num\" 2 1"), -4.0);
    // The file on disk kept the write with 0, indented by four spaces.
    assert_eq!(n(&mut g, "JSONCacheFile \"nvfj.json\" 0"), 0.0);
    assert!(s(&mut g, "JSONToString \"nvfj.json\"").contains("\"num\":7.0"));
    assert_eq!(n(&mut g, "JSONCacheFileFromString \"not json\" \"nvfm.json\""), -3.0);
}

// ---- #2326, batch 3: kNVSE's animation commands ----

#[test]
fn knvse_answers_and_play_group_alt() {
    let f = fixture();
    let mut g = game(&f);
    for (call, want) in [
        ("NpcRef.GetAnimWeight \"characters/_male/locomotion/mtidle.kf\"", 0.0),
        ("NpcRef.GetAnimHandType \"characters/_male/locomotion/mtidle.kf\"", -1.0),
        ("NpcRef.GetAnimInterpPriority \"characters/_male/locomotion/mtidle.kf\" \"Bip01 Head\"", -1.0),
        ("NpcRef.GetAnimTextKeyTime \"characters/_male/locomotion/mtidle.kf\" \"Start\"", -1.0),
        ("NpcRef.IsAnimQueued \"x.kf\"", 0.0),
        ("NpcRef.QueueNextAnim \"x.kf\"", 0.0),
        ("NpcRef.SetAnimCycleType \"nvfnosuch.kf\" 0", 0.0),
        ("NpcRef.EraseCurrentAnimAction", 1.0),
        ("CreateIdleAnimForm \"x.kf\"", 0.0),
    ] {
        assert_eq!(n(&mut g, call), want, "{call}");
    }
    assert_eq!(s(&mut g, "NpcRef.GetAnimPathBySequenceType 1"), "");
    assert_eq!(s(&mut g, "ThisAnim"), "");
    let before = g.events.len();
    assert_eq!(n(&mut g, "NpcRef.PlayGroupAlt Idle 1"), 1.0);
    assert!(g.events[before..].iter().any(|e| format!("{e:?}").contains("AnimAsked")), "the group is asked for");
    // No read roots here: a wildcard finds only `.` and `..`, in reverse order.
    assert_eq!(s(&mut g, "sv_Join (GetDirectoryFiles \"Data\\NoFolder\\*\") \"|\""), "Data\\NoFolder\\..|Data\\NoFolder\\.");
    assert_eq!(n(&mut g, "ar_Size (GetDirectoryFiles \"Data\\NoFolder\")"), 0.0);
}

// ---- #2326, batch 4: ShowOff, SUP, One111, ITR, JohnnyGuitar ----

#[test]
fn batch_four_answers() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(s(&mut g, "sv_Join (BoxRef.GetWorldOffsetPosArray) \",\""), "300,400,100");
    for (call, want) in [
        ("NpcRef.GetActorCoverState", -1.0),
        ("GetWeapAdditionalFloat TestGun 0", -1.0),
        ("NpcRef.GetExtraAccuracyPenaltyMult", 1.0),
        ("NpcRef.IsTemporaryReference", 0.0),
        ("GetLevelUpMenuUnspentPoints 0", 0.0),
        ("SetMusicTypePath TestGun \"x.mp3\"", 1.0),
        ("ClearMediaLocationControllerOverride", 1.0),
        ("HudBarSetForceHide 0 0 \"a\"", -1.0),
        ("TopicGetTopicInfoQuest TestGun", 0.0),
        ("SayTo_GetQuest", 0.0),
    ] {
        assert_eq!(n(&mut g, call), want, "{call}");
    }
    assert_eq!(n(&mut g, "ar_Size (PlayerREF.GetEligiblePerksEx PlayerREF)"), 0.0);
    assert_eq!(n(&mut g, "ar_Size (NpcRef.GetActorCoverInfo)"), 0.0);
}

// ---- #2326, batch 5: JIP ----

#[test]
fn batch_five_answers() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetArrayValue (ar_List 10 20 30) 1"), 20.0);
    assert_eq!(n(&mut g, "RefMapArraySetValue \"*rm\" 5 PlayerREF"), 0.0);
    assert_eq!(n(&mut g, "RefMapArrayGetSize \"*rm\""), 1.0);
    assert_eq!(n(&mut g, "(RefMapArrayGetValue \"*rm\" PlayerREF)[0]"), 5.0);
    assert_eq!(n(&mut g, "ar_Size (RefMapArrayGetValue \"*rm\" NpcRef)"), -1.0, "no value: no array");
    assert_eq!(n(&mut g, "ar_Size (PlayerREF.GetEquippedArmorRefs)"), 0.0);
    assert_eq!(n(&mut g, "PlayerREF.HasActionRef"), 0.0, "standing player has no active action state");
    assert_eq!(n(&mut g, "NpcRef.HasActionRef"), 0.0, "standing NPC without furniture/action state returns 0");
    assert_eq!(n(&mut g, "BoxRef.HasActionRef"), 0.0);
    assert_eq!(n(&mut g, "GetRaceVoice TestGun 0"), 0.0, "not a race");
    for call in ["SetWindSpeedMult 1", "ResetClouds", "NpcRef.ToggleNoZPosReset 0", "SuppressQuestMessages", "EnableImprovedRecipeMenu"] {
        assert_eq!(n(&mut g, call), 0.0, "{call}");
    }
}

#[test]
fn has_action_ref_tracks_active_action_furniture_and_scripted_doors() {
    use fnv_game::game::FurnitureUse;

    const SCPT_DOOR: u32 = 0x950;
    const DOOR_SCR: u32 = 0x951;
    const DOOR_PLAIN: u32 = 0x952;
    const DOOR_SCR_REF: u32 = 0x953;
    const DOOR_PLAIN_REF: u32 = 0x954;
    const FURN_BASE: u32 = 0x955;
    const FURN_REF: u32 = 0x956;

    let mut raw = plugin_bytes();
    let scpt = edid(b"SCPT", SCPT_DOOR, "TestDoorScript", &unit(0, &script(&[(0x0002, None, Vec::new())]), &[], &[]));
    let doors = [
        edid(b"DOOR", DOOR_SCR, "TestScriptedDoor", &[sub(b"SCRI", SCPT_DOOR.to_le_bytes().to_vec())]),
        edid(b"DOOR", DOOR_PLAIN, "TestPlainDoor", &[]),
    ]
    .concat();
    let furn = edid(b"FURN", FURN_BASE, "TestChair", &[]);
    let extra_refs = [
        placed(b"REFR", DOOR_SCR_REF, "ScriptedDoorRef", DOOR_SCR, [10.0, 0.0, 0.0]),
        placed(b"REFR", DOOR_PLAIN_REF, "PlainDoorRef", DOOR_PLAIN, [20.0, 0.0, 0.0]),
        placed(b"REFR", FURN_REF, "ChairRef", FURN_BASE, [30.0, 0.0, 0.0]),
    ]
    .concat();
    let extra_cell = edid(b"CELL", 0x201, "HasActionCell", &[sub(b"FULL", z("Action Room")), sub(b"DATA", vec![1])]);
    let extra_children = group(0x201u32.to_le_bytes(), 6, &group(0x201u32.to_le_bytes(), 9, &extra_refs));
    raw.extend_from_slice(&top(b"SCPT", &scpt));
    raw.extend_from_slice(&top(b"DOOR", &doors));
    raw.extend_from_slice(&top(b"FURN", &furn));
    raw.extend_from_slice(&top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[extra_cell, extra_children].concat()))));

    let plugins = vec![Plugin::parse(&raw, "HasAction.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    assert_eq!(data.script_of_base(FormId(DOOR_SCR)), Some(FormId(SCPT_DOOR)), "synthetic door script must be admitted");
    let mut g = Game::new(&data, World::new(&forms, &refs, SimConfig::default()), GameConfig::default());

    // Standing Player, standing NPC, unscripted door, furniture, and container all return 0.
    assert_eq!(n(&mut g, "PlayerREF.HasActionRef"), 0.0);
    assert_eq!(n(&mut g, "NpcRef.HasActionRef"), 0.0);
    assert_eq!(n(&mut g, "PlainDoorRef.HasActionRef"), 0.0);
    assert_eq!(n(&mut g, "ChairRef.HasActionRef"), 0.0);
    assert_eq!(n(&mut g, "BoxRef.HasActionRef"), 0.0);

    // Scripted door returns 1 (generalizing observed front door 0x00103E61;
    // see docs/spec-notes/ext-unlisted-rest.md).
    assert_eq!(n(&mut g, "ScriptedDoorRef.HasActionRef"), 1.0);

    // Actor sitting in furniture returns 1, then returns 0 upon standing up
    // [observed: w4c_a03c_has_action_ref_doc_real].
    g.set_furniture_use(FormId(NPC_REF), Some(FurnitureUse { reference: FormId(FURN_REF), marker_id: 0, state: 3 }));
    assert_eq!(n(&mut g, "NpcRef.HasActionRef"), 1.0);
    g.set_furniture_use(FormId(NPC_REF), None);
    assert_eq!(n(&mut g, "NpcRef.HasActionRef"), 0.0);

    // Freshly spawned/idle NPC transitions 0 -> 1 once an AI package becomes active
    // (generalizing `spawn(0,1)` in w4d_02_has_action_ref_triggers; see docs/spec-notes/ext-unlisted-rest.md),
    // while Player stays 0.
    g.extra.entry(FormId(NPC_REF)).or_default().current_package = Some(FormId(0x999));
    assert_eq!(n(&mut g, "NpcRef.HasActionRef"), 1.0);
    assert_eq!(n(&mut g, "PlayerREF.HasActionRef"), 0.0);
    g.extra.entry(FormId(NPC_REF)).or_default().current_package = None;
    assert_eq!(n(&mut g, "NpcRef.HasActionRef"), 0.0);

    // Unscripted door starts at 0, acquires HasActionRef = 1 upon `Activate Player 1`, and
    // retains 1 on the next tick [observed: w4d_02_has_action_ref_triggers (#3802 / PR #3803):
    // `unscr(001062AC:0,1,1)`]. Container acquires 1 upon `Activate Player 1` (in the original
    // game `cont(001062C6:0,1,0)` clears to 0 once ContainerMenu closes; NVForever's headless
    // `action_refs` entry retains 1 until cleared; see docs/spec-notes/ext-unlisted-rest.md).
    assert_eq!(n(&mut g, "PlainDoorRef.HasActionRef"), 0.0);
    q(&mut g, "PlainDoorRef.Activate PlayerREF 1");
    assert_eq!(n(&mut g, "PlainDoorRef.HasActionRef"), 1.0);
    g.tick();
    assert_eq!(n(&mut g, "PlainDoorRef.HasActionRef"), 1.0, "unscripted door retains HasActionRef next tick");

    assert_eq!(n(&mut g, "BoxRef.HasActionRef"), 0.0);
    q(&mut g, "BoxRef.Activate PlayerREF 1");
    assert_eq!(n(&mut g, "BoxRef.HasActionRef"), 1.0);
    g.tick();
    assert_eq!(n(&mut g, "BoxRef.HasActionRef"), 1.0, "container retains headless action_refs entry");
    assert_eq!(n(&mut g, "ScriptedDoorRef.HasActionRef"), 1.0, "scripted door remains 1");
}

#[test]
fn refresh_anim_data_queues_one_shot_and_refreshes_folder_entries() {
    let f = fixture();
    let mut g = game(&f);
    let npc = FormId(NPC_REF);
    let player = FormId::PLAYER_REF;

    // Seed a conditional AnimGroupOverride entry that only activates once player level >= 5.
    g.anim_folders.entries.push(fnv_game::nvse_anim_folders::FolderEntry {
        forms: vec![player],
        folder: "TestRefresh".into(),
        condition: Some("GetLevel >= 5".into()),
        priority: 1,
        order: 0,
        third: vec!["AnimGroupOverride\\TestRefresh\\_male\\mtidle.kf".into()],
        first: vec!["AnimGroupOverride\\TestRefresh\\_1stPerson\\mtidle.kf".into()],
        ..Default::default()
    });
    g.refresh_anim_folders();
    assert!(g.anim_overrides(player, false).is_empty(), "level 1 does not satisfy folder condition");

    // Raise player level without waiting for REFRESH_FRAMES; RefreshAnimData refreshes immediately.
    g.player.level = Some(5);
    assert_eq!(n(&mut g, "PlayerREF.RefreshAnimData"), 0.0);
    assert_eq!(n(&mut g, "NpcRef.RefreshAnimData"), 0.0);
    assert!(g.anim_refresh_pending(player));
    assert!(g.anim_refresh_pending(npc));
    assert_eq!(g.anim_overrides(player, false), vec!["AnimGroupOverride\\TestRefresh\\_male\\mtidle.kf"]);
    assert_eq!(g.anim_overrides(player, true), vec!["AnimGroupOverride\\TestRefresh\\_1stPerson\\mtidle.kf"]);

    // Transient: snapshot + restore drops queued refresh requests.
    let snap = g.snapshot();
    let restored = Game::restore(&f.data, World::restore(&f.forms, &f.refs, &snap.sim).unwrap(), g.cfg, &snap).unwrap();
    assert!(!restored.anim_refresh_pending(player));
    assert!(!restored.anim_refresh_pending(npc));

    // Player and NPC consumers drain independently without swallowing each other.
    assert!(g.take_player_anim_refresh());
    assert!(!g.anim_refresh_pending(player));
    assert!(g.anim_refresh_pending(npc));
    assert!(!g.take_player_anim_refresh(), "one-shot for player");

    let npcs = g.take_npc_anim_refreshes();
    assert!(npcs.contains(&npc));
    assert!(!npcs.contains(&player));
    assert!(!g.anim_refresh_pending(npc));
    assert!(g.take_npc_anim_refreshes().is_empty(), "one-shot for NPCs");
}

#[test]
fn reload_equipped_models_queues_slot_masks_coalesces_and_is_transient() {
    use fnv_game::nvse_gaps::{ALL_EQUIP_SLOTS, EQUIP_SLOT_WEAPON_BIT};
    let f = fixture();
    let mut g = game(&f);
    let npc = FormId(NPC_REF);
    let player = FormId::PLAYER_REF;

    // Specific slots coalesce via bitwise OR; out-of-range slots are ignored.
    assert_eq!(n(&mut g, "PlayerREF.ReloadEquippedModels 2"), 0.0);
    assert_eq!(n(&mut g, "PlayerREF.ReloadModels 5"), 0.0);
    assert_eq!(n(&mut g, "PlayerREF.ReloadEquippedModels -1"), 0.0);
    assert_eq!(n(&mut g, "PlayerREF.ReloadEquippedModels 20"), 0.0);
    assert_eq!(g.equipped_model_reload_mask(player), (1 << 2) | EQUIP_SLOT_WEAPON_BIT);

    // Omitting equipSlot selects all 20 biped slots.
    assert_eq!(n(&mut g, "NpcRef.ReloadEquippedModels"), 0.0);
    assert_eq!(g.equipped_model_reload_mask(npc), ALL_EQUIP_SLOTS);

    // Transient: snapshot + restore drops queued reload masks.
    let snap = g.snapshot();
    let restored = Game::restore(&f.data, World::restore(&f.forms, &f.refs, &snap.sim).unwrap(), g.cfg, &snap).unwrap();
    assert_eq!(restored.equipped_model_reload_mask(player), 0);
    assert_eq!(restored.equipped_model_reload_mask(npc), 0);

    // Player and NPC consumers drain independently and one-shot.
    assert_eq!(g.take_player_equipped_model_reload(), (1 << 2) | EQUIP_SLOT_WEAPON_BIT);
    assert_eq!(g.take_player_equipped_model_reload(), 0);
    assert_eq!(g.equipped_model_reload_mask(npc), ALL_EQUIP_SLOTS);

    let npcs = g.take_npc_equipped_model_reloads();
    assert_eq!(npcs.get(&npc).copied(), Some(ALL_EQUIP_SLOTS));
    assert!(!npcs.contains_key(&player));
    assert!(g.take_npc_equipped_model_reloads().is_empty());
}

#[test]
fn update_player_3d_queues_one_shot_and_is_transient() {
    let f = fixture();
    let mut g = game(&f);
    assert!(!g.update_player_3d_pending());
    assert_eq!(n(&mut g, "UpdatePlayer3D"), 0.0);
    assert!(g.update_player_3d_pending());

    let snap = g.snapshot();
    let restored = Game::restore(&f.data, World::restore(&f.forms, &f.refs, &snap.sim).unwrap(), g.cfg, &snap).unwrap();
    assert!(!restored.update_player_3d_pending());

    assert!(g.take_update_player_3d());
    assert!(!g.update_player_3d_pending());
    assert!(!g.take_update_player_3d(), "one-shot");
}
