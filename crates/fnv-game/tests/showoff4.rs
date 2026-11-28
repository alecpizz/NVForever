//! ShowOff NVSE batch 4 (#1010; docs/spec-notes/ext-showoff.md §7) on a synthetic plugin
//! written for the test (no game data, made-up names): the matrix functions on the pages'
//! examples, JSON and the INI cache through NVForever's in-memory file layer, the
//! switches, SetOwnershipTemp kept out of the save, and the enable-parent functions.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const STAT: u32 = 0x800;
const LIST: u32 = 0x801;
const FACT: u32 = 0x802;
const WEAP: u32 = 0x803;
const NPC: u32 = 0x804;
const LVLN: u32 = 0x805;
const ROOM: u32 = 0x900;
const PARENT: u32 = 0x910;
const CHILD: u32 = 0x911;
const ACTOR: u32 = 0x912;
const LEVELED_ACTOR: u32 = 0x913;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn placed(fid: u32, edid: &str, base: u32, parent: Option<u32>) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])];
    if let Some(p) = parent {
        subs.push(sub(b"XESP", [p.to_le_bytes(), 0u32.to_le_bytes()].concat()));
    }
    record(b"REFR", 0, fid, &subs)
}

fn fixture() -> Fixture {
    let stat = record(b"STAT", 0, STAT, &[sub(b"EDID", z("TestStatic"))]);
    let flst = record(b"FLST", 0, LIST, &[sub(b"EDID", z("TestList")), sub(b"LNAM", u32s(&[STAT]))]);
    let fact = record(b"FACT", 0, FACT, &[sub(b"EDID", z("TestFaction"))]);
    let weap = record(b"WEAP", 0, WEAP, &[sub(b"EDID", z("TestWeapon")), sub(b"ICON", z("interface\\icons\\default_weap.dds"))]);
    let room = record(b"CELL", 0, ROOM, &[sub(b"EDID", z("TestRoom")), sub(b"DATA", vec![1])]);
    let actor_ref = record(b"ACHR", 0, ACTOR, &[sub(b"EDID", z("TestActorRef")), sub(b"NAME", NPC.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let leveled_actor_ref = record(b"ACHR", 0, LEVELED_ACTOR, &[sub(b"EDID", z("TestLeveledActorRef")), sub(b"NAME", LVLN.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let refs = [placed(PARENT, "TestParentRef", STAT, None), placed(CHILD, "TestChildRef", STAT, Some(PARENT)), actor_ref, leveled_actor_ref].concat();
    let children = group(ROOM.to_le_bytes(), 6, &group(ROOM.to_le_bytes(), 9, &refs));
    let npcs = [
        record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]),
        record(b"NPC_", 0, NPC, &[sub(b"EDID", z("TestNpc"))]),
    ]
    .concat();
    let lvlo = [1i16.to_le_bytes().to_vec(), vec![0; 2], NPC.to_le_bytes().to_vec(), 1i16.to_le_bytes().to_vec(), vec![0; 2]].concat();
    let lvln = record(b"LVLN", 0, LVLN, &[sub(b"EDID", z("TestLeveledNpc")), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0]), sub(b"LVLO", lvlo)]);
    let bytes = [
        tes4(),
        top(b"FACT", &fact),
        top(b"WEAP", &weap),
        top(b"STAT", &stat),
        top(b"FLST", &flst),
        top(b"NPC_", &npcs),
        top(b"LVLN", &lvln),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[room, children].concat()))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "ShowOff4Test.esm".into()).unwrap()];
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

/// An expression's value, or a body's (one with lines, ending in its SetFunctionValue).
fn n(g: &mut Game, expr: &str) -> f64 {
    let body = if expr.contains('\n') { expr.to_string() } else { format!("SetFunctionValue ({expr})") };
    match q(g, &body) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

fn s(g: &mut Game, body: &str) -> String {
    match q(g, body) {
        XVal::Str(t) => t,
        v => panic!("{body}: {v:?}"),
    }
}

#[test]
fn matrices_follow_the_pages_examples() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: Matrix_IsMatrix's examples].
    assert_eq!(n(&mut g, r#"Mat_IsMat (ar_List "test")"#), 0.0);
    assert_eq!(n(&mut g, "Mat_IsMat (ar_List (ar_List 1))"), 1.0);
    assert_eq!(n(&mut g, "Mat_IsMat (ar_List (ar_List 1) (ar_List 2 3))"), 0.0, "ragged rows");
    assert_eq!(n(&mut g, "Mat_IsMat (ar_List 5 (ar_List 2 3))"), 0.0, "mixed");
    assert_eq!(n(&mut g, "Matrix_IsMatrix (ar_List (ar_List 1 2) (ar_List 2 3))"), 1.0);
    // Example 5 of Matrix_ApplyOperationWithMatrix: 57 64 / 89 100.
    let prod = "array_var a\nlet a := Mat_ApplyOpWithMat (ar_List (ar_List 3 4) (ar_List 5 6)) \"*\" (ar_List (ar_List 7 8) (ar_List 9 10))\n";
    assert_eq!(n(&mut g, &format!("{prod}SetFunctionValue a[0][0]")), 57.0);
    assert_eq!(n(&mut g, &format!("{prod}SetFunctionValue a[1][1]")), 100.0);
    // A 1x3 times a 1x3 is not defined: no array.
    assert_eq!(n(&mut g, r#"ar_Size (Mat_ApplyOpWithMat (ar_List 1 2 3) "*" (ar_List 1 2 3))"#), -1.0);
    assert_eq!(n(&mut g, "array_var a\nlet a := Mat_ApplyOpWithMat (ar_List 1 2 3) \"%\" (ar_List 1 2 3)\nSetFunctionValue a[2]"), 9.0);
    assert_eq!(n(&mut g, "array_var a\nlet a := Mat_ApplyOpWithScal (ar_List (ar_List 3 4) (ar_List 5 6)) \"/\" 2\nSetFunctionValue a[1][0]"), 2.5);
    assert_eq!(n(&mut g, "array_var a\nlet a := Matrix_ApplyOperationWithScalar (ar_List (ar_List 3 4) (ar_List 5 6)) \"+\" 2\nSetFunctionValue a[0][1]"), 6.0);
    assert_eq!(n(&mut g, "array_var a\nlet a := Mat_T (ar_List (ar_List 3 4) (ar_List 5 6))\nSetFunctionValue a[0][1]"), 5.0);
    // A quaternion of 90 degrees about Z: the game gives the transpose of the right-handed
    // form [observed 2026-11-08: showoff4.quat.z90] and Mat_GetQuat reads the right-handed
    // form, so the way back gives -z.
    let quat = "array_var m\narray_var b\nlet m := Quat_GetMat (ar_List 0.7071067811865476 0 0 0.7071067811865476)\nlet b := Mat_GetQuat m\n";
    assert!((n(&mut g, &format!("{quat}SetFunctionValue m[1][0]")) + 1.0).abs() < 1e-6);
    assert!((n(&mut g, &format!("{quat}SetFunctionValue b[3]")) + std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    let before = g.console_out.len();
    q(&mut g, "Mat_Dump (ar_List (ar_List 1 2) (ar_List 3 4))");
    let lines = &g.console_out[before..];
    assert_eq!(lines.len(), 3);
    assert!(lines[1].contains("1.0000") && lines[1].contains("2.0000"), "{lines:?}");
}

#[test]
fn json_and_the_ini_cache_go_through_nvforevers_layer() {
    let f = fixture();
    let mut g = game(&f);
    // No write root in a test: the files live in memory for the session.
    assert_eq!(n(&mut g, r#"WriteToJSON (ar_Map "a"::1 "b"::"text") "NVFTest\so4.json""#), 1.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\so4.json" "/a""#), 1.0);
    assert_eq!(s(&mut g, "string_var t\nlet t := ReadFromJSON \"NVFTest\\so4.json\" \"/b\"\nSetFunctionValue t"), "text");
    // A form is written as "@plugin:FormID" and read back as the form [documented].
    assert_eq!(n(&mut g, r#"WriteToJSON TestStatic "NVFTest\so4.json" "/form""#), 1.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\so4.json" "/form""#), f64::from(STAT));
    // WriteToJSON root creation vs missing intermediate segments (see docs/spec-notes/ext-showoff.md):
    // 1. Missing file + single-level "/k" creates the root object `{ "k": 42 }` and returns 1,
    //    while nested "/a/b/c" fails (returns 0) without creating missing intermediates or a file.
    assert_eq!(n(&mut g, r#"WriteToJSON 42 "NVFTest\missing_single_def.json" "/k""#), 1.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\missing_single_def.json" "/k" 0 0"#), 42.0);
    assert_eq!(n(&mut g, r#"WriteToJSON 42 "NVFTest\missing_single_exp.json" "/k" 0 1 1 0"#), 1.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\missing_single_exp.json" "/k" 0 1"#), 42.0);
    assert_eq!(n(&mut g, r#"WriteToJSON 123 "NVFTest\missing_nested.json" "/a/b/c" 0 1 1 0"#), 0.0);
    assert_eq!(n(&mut g, r#"TestExpr (ReadFromJSON "NVFTest\missing_nested.json" "/a/b/c" 0 1)"#), 0.0);
    // 2. Missing file + empty root pointer "" writing a StringMap returns 1 and creates the file.
    assert_eq!(n(&mut g, r#"WriteToJSON (ar_Map "k"::42) "NVFTest\missing_root.json" "" 0 1 1 0"#), 1.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\missing_root.json" "/k" 0 0"#), 42.0);
    // 3. Existing `{}` file + "/k" returns 1 while nested "/a/b/c" returns 0 without mutating.
    assert_eq!(n(&mut g, r#"WriteToJSON (ar_Construct "stringmap") "NVFTest\exist_obj.json" "" 0 0 1 0"#), 1.0);
    assert_eq!(n(&mut g, r#"WriteToJSON 42 "NVFTest\exist_obj.json" "/k" 0 1 1 0"#), 1.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\exist_obj.json" "/k" 0 1"#), 42.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\exist_obj.json" "/k" 0 0"#), 42.0);
    assert_eq!(n(&mut g, r#"WriteToJSON 99 "NVFTest\exist_obj.json" "/a/b/c" 0 1 1 0"#), 0.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\exist_obj.json" "/a/b/c" 0 0"#), 0.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\exist_obj.json" "/k" 0 0"#), 42.0);
    // 4. Existing `null` root + nested "/a/b/c" returns 0, while single-level "/k" promotes root to `{}`.
    assert_eq!(n(&mut g, "ref rNull\nSetFunctionValue (WriteToJSON rNull \"NVFTest\\exist_null.json\" \"\" 0 0 1 0)"), 1.0);
    assert_eq!(n(&mut g, r#"WriteToJSON 99 "NVFTest\exist_null.json" "/a/b/c" 0 1 1 0"#), 0.0);
    assert_eq!(n(&mut g, r#"WriteToJSON 42 "NVFTest\exist_null.json" "/k" 0 1 1 0"#), 1.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\exist_null.json" "/k" 0 1"#), 42.0);
    assert_eq!(n(&mut g, r#"WriteToJSON 123 "NVFTest\so4.json" "/missing/b/c" 0 1 1 0"#), 0.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "NVFTest\so4.json" "/missing/b/c" 0 1"#), 0.0);
    // A missing file gives no value [observed 2026-11-08: showoff4.readjson.missing].
    assert_eq!(n(&mut g, r#"TestExpr (ReadFromJSON "NVFTest\missing.json")"#), 0.0);
    assert_eq!(n(&mut g, r#"CreateFolder "NVFTest\Sub""#), 1.0);
    assert_eq!(n(&mut g, r#"CreateFolder "..\Outside""#), 0.0, "a path out of the layer is refused");
    // The INI cache: a key created in the cache, then saved and read by LU's reader.
    assert_eq!(n(&mut g, r#"HasINISetting_Cached "Main:fNum" "NVFTestSO4""#), 0.0);
    assert_eq!(n(&mut g, r#"GetINIFloatOrCreate_Cached "Main:fNum" "NVFTestSO4" 5"#), 5.0);
    assert_eq!(n(&mut g, r#"HasINISettingC "Main:fNum" "NVFTestSO4""#), 1.0);
    assert_eq!(n(&mut g, r#"GetINIFloat "Main:fNum" "NVFTestSO4""#), 0.0, "OrCreate changes only the cache");
    assert_eq!(n(&mut g, r#"SaveCachedIniFile "NVFTestSO4""#), 1.0);
    assert_eq!(n(&mut g, r#"GetINIFloat "Main:fNum" "NVFTestSO4""#), 5.0);
    assert_eq!(n(&mut g, r#"ReloadIniCache "NVFTestSO4""#), 1.0);
    assert_eq!(n(&mut g, r#"ReloadIniCache "NVFTestSO4Missing""#), 0.0);
    let before = g.console_out.len();
    q(&mut g, "DumpFormList TestList");
    assert_eq!(g.console_out.len() - before, 2, "a heading and one form");
}

#[test]
fn switches_and_session_only_ownership() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetPlayerCanPickpocketEquippedItems"), 0.0);
    q(&mut g, "SetPCCanStealEqItems 1");
    assert_eq!(n(&mut g, "GetPCCanStealEqItems"), 1.0);
    assert_eq!(n(&mut g, "ToggleQuestMessages"), 1.0, "on by default");
    assert_eq!(n(&mut g, "ToggleQuestMessages 0"), 1.0);
    assert_eq!(n(&mut g, "ToggleQuestMessages 0"), 0.0, "no change");
    assert!(g.ui.quest_messages_off);
    let killed = g.ui.quest_updates_killed;
    q(&mut g, "ClearQuestMessageQueue");
    assert_eq!(g.ui.quest_updates_killed, killed + 1);
    assert_eq!(n(&mut g, "FreezeAmmoRegen 1"), 0.0);
    assert_eq!(n(&mut g, "FreezeAmmoRegen"), 1.0);
    assert!(g.showoff.ammo_regen_frozen);
    // SetOwnershipTemp: the owner now, the one before in a save.
    assert_eq!(n(&mut g, "player.SetOwnershipTemp TestFaction"), 1.0);
    assert_eq!(g.world.get(FormId::PLAYER_REF).unwrap().extra.owner, Some(FormId(FACT)));
    let snap = g.snapshot();
    assert_eq!(snap.sim.refs.get(&FormId::PLAYER_REF).and_then(|r| r.extra.owner), None);
    // SetOwnership afterwards is saved.
    q(&mut g, "player.SetOwnership TestFaction");
    assert_eq!(g.snapshot().sim.refs.get(&FormId::PLAYER_REF).and_then(|r| r.extra.owner), Some(FormId(FACT)));
    assert_eq!(n(&mut g, "ResetInteriorAlt TestRoom"), 1.0);
    assert!(g.forms.expired_cells_full.contains(&FormId(ROOM)));
}

#[test]
fn enable_parents_can_be_changed_and_ignored() {
    let f = fixture();
    let mut g = game(&f);
    let on = |g: &Game, r: u32| g.world.is_enabled(FormId(r)).unwrap();
    // Disable on a child with a parent does nothing; DisableAlt does [documented].
    q(&mut g, "TestChildRef.Disable");
    assert!(on(&g, CHILD));
    q(&mut g, "TestChildRef.DisableAlt");
    assert!(!on(&g, CHILD));
    q(&mut g, "TestChildRef.EnableAlt 0");
    assert!(on(&g, CHILD));
    // SetEnableParent: the child follows its parent again, then none.
    q(&mut g, "TestChildRef.SetEnableParent TestParentRef");
    q(&mut g, "TestParentRef.Disable");
    assert!(!on(&g, CHILD), "follows the parent");
    assert_eq!(n(&mut g, "TestChildRef.GetParentRef"), f64::from(PARENT));
    q(&mut g, "TestChildRef.SetEnableParent");
    assert!(on(&g, CHILD), "its own flag again");
    assert_eq!(n(&mut g, "TestChildRef.GetParentRef"), 0.0);
}

#[test]
fn hotkey_icon_override_is_stored_read_back_and_cleared() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(g.item_hotkey_icon_path(FormId(WEAP)), None);
    assert_eq!(s(&mut g, "SetFunctionValue (GetItemHotkeyIconPath TestWeapon 1)"), "");
    assert_eq!(s(&mut g, "SetFunctionValue (GetItemHotkeyIconPath TestWeapon)"), "interface\\icons\\default_weap.dds");

    q(&mut g, r#"SetItemHotkeyIconPath TestWeapon "interface\icons\custom_hotkey.dds""#);
    assert_eq!(g.item_hotkey_icon_path(FormId(WEAP)), Some("interface\\icons\\custom_hotkey.dds"));
    assert_eq!(s(&mut g, "SetFunctionValue (GetItemHotkeyIconPath TestWeapon 1)"), "interface\\icons\\custom_hotkey.dds");
    assert_eq!(s(&mut g, "SetFunctionValue (GetItemHotkeyIconPath TestWeapon)"), "interface\\icons\\custom_hotkey.dds");

    // Live update replaces the existing override.
    q(&mut g, r#"SetItemHotkeyIconPath TestWeapon "Textures/Interface/Icons/updated.dds""#);
    assert_eq!(g.item_hotkey_icon_path(FormId(WEAP)), Some("Textures/Interface/Icons/updated.dds"));

    // Whitespace-only overrides are ignored by engine consumers.
    q(&mut g, r#"SetItemHotkeyIconPath TestWeapon "   ""#);
    assert_eq!(g.item_hotkey_icon_path(FormId(WEAP)), None);

    // Empty string removes the override and restores the item's default ICON.
    q(&mut g, r#"SetItemHotkeyIconPath TestWeapon """#);
    assert_eq!(g.item_hotkey_icon_path(FormId(WEAP)), None);
    assert!(!g.showoff.hotkey_icons.contains_key(&FormId(WEAP)));
    assert_eq!(s(&mut g, "SetFunctionValue (GetItemHotkeyIconPath TestWeapon 1)"), "");
    assert_eq!(s(&mut g, "SetFunctionValue (GetItemHotkeyIconPath TestWeapon)"), "interface\\icons\\default_weap.dds");
}

#[test]
fn get_actor_fade_state_distinguishes_loaded_and_unloaded_actors() {
    let f = fixture();
    let mut g = game(&f);
    // Player is always loaded and not fading -> 0.
    assert_eq!(n(&mut g, "player.GetActorFadeState"), 0.0);
    // Unloaded NPC and placed leveled NPC (cell not in loaded_cells) -> -1 (preserves showoff4.fadestate.doc).
    assert_eq!(n(&mut g, "TestActorRef.GetActorFadeState"), -1.0);
    assert_eq!(n(&mut g, "TestLeveledActorRef.GetActorFadeState"), -1.0);
    // Loaded NPC and placed leveled NPC (LVLN carrier resolved via template_source) -> 0.
    g.loaded_cells.insert(FormId(ROOM));
    assert_eq!(n(&mut g, "TestActorRef.GetActorFadeState"), 0.0);
    assert_eq!(n(&mut g, "TestLeveledActorRef.GetActorFadeState"), 0.0);
    // Non-actor ref in a loaded cell -> -1.
    assert_eq!(n(&mut g, "TestParentRef.GetActorFadeState"), -1.0);
    // Disabled NPC / leveled NPC in a loaded cell -> -1.
    q(&mut g, "TestActorRef.Disable\nTestLeveledActorRef.Disable");
    assert_eq!(n(&mut g, "TestActorRef.GetActorFadeState"), -1.0);
    assert_eq!(n(&mut g, "TestLeveledActorRef.GetActorFadeState"), -1.0);
}
