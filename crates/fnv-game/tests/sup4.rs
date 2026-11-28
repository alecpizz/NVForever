//! SUP NVSE batch 4 (#1008; docs/spec-notes/ext-sup4.md) on a synthetic plugin, called
//! from scripts: random array keys, invalid references, floats from an array, a UDF some
//! frames later, the save names and sizes (the engine's part played by the test), the
//! cells near the player, the caught-pickpocketing flag and transliteration.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nvse_sup4::SaveFile;
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const NPC: u32 = 0x800;
const CELL: u32 = 0x900;
const NPC_REF: u32 = 0x910;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let acbs = [&0u32.to_le_bytes()[..], &[0; 4], &1u16.to_le_bytes(), &[0; 6], &0f32.to_le_bytes(), &[0; 4]].concat();
    let npcs = [record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]), record(b"NPC_", 0, NPC, &[sub(b"EDID", z("TestMark")), sub(b"ACBS", acbs)])].concat();
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestRoom")), sub(b"DATA", vec![1])]);
    let npc_ref = record(b"ACHR", 0, NPC_REF, &[sub(b"EDID", z("TestMarkRef")), sub(b"NAME", NPC.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &npc_ref));
    let bytes = [tes4(), top(b"NPC_", &npcs), top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat())))].concat();
    let plugins = vec![Plugin::parse(&bytes, "Sup4Test.esm".into()).unwrap()];
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
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(CELL));
    g
}

const VARS: &str = "float fA\nfloat fB\nfloat fC\nfloat fD\nref rNone\narray_var aR\narray_var aIn\nstring_var sOut\n";

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("{VARS}Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    let body = if expr.contains('\n') { expr.to_string() } else { format!("SetFunctionValue ({expr})") };
    match q(g, &body) {
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
fn arrays() {
    let f = fixture();
    let mut g = game(&f);
    for _ in 0..10 {
        let k = n(&mut g, "let aR := Ar_List 10, 20, 30, 40, 50\nSetFunctionValue (Ar_GetRandomKey aR)");
        assert!((0.0..5.0).contains(&k) && k.fract() == 0.0, "{k}");
        let m = s(&mut g, "Ar_GetRandomKeyMap (Ar_Map \"LALALA\"::1, \"TROLOLO\"::2)");
        assert!(m == "LALALA" || m == "TROLOLO", "{m}");
    }
    // An empty array 0 [observed: sup4 randomkey; 2026-11-23, GSDocMitchellHouse
    // (0x00103DF9), `sup_08_ar_getrandomkey_empty`].
    assert_eq!(n(&mut g, "let aR := Ar_List\nSetFunctionValue (Ar_GetRandomKey aR)"), 0.0);
    // Numeric map keys (including negative floats) and list indices format as 6-decimal
    // float strings [observed: 2026-11-23 & 2026-11-24, GSDocMitchellHouse (0x00103DF9),
    // `sup_26_ar_getrandomkey_num_map` and `w4d_36_sup_random_keymap_and_dialog`].
    assert_eq!(s(&mut g, "Ar_GetRandomKeyMap (Ar_Map 7::100)"), "7.000000");
    assert_eq!(s(&mut g, "Ar_GetRandomKeyMap (Ar_Map -3.5::99)"), "-3.500000");
    assert_eq!(s(&mut g, "Ar_GetRandomKeyMap (Ar_List 42)"), "0.000000");
    // An empty reference is an invalid one; a dimension searches the inner array.
    assert_eq!(n(&mut g, "let aR := Ar_List PlayerREF, TestMarkRef\nSetFunctionValue (Ar_HasInvalidRefs aR)"), 0.0);
    assert_eq!(n(&mut g, "let aR := Ar_List PlayerREF, rNone\nSetFunctionValue (Ar_HasInvalidRefs aR)"), 1.0);
    assert_eq!(n(&mut g, "let aIn := Ar_List rNone\nlet aR := Ar_List PlayerREF, aIn\nSetFunctionValue (Ar_HasInvalidRefs aR)"), 0.0);
    assert_eq!(n(&mut g, "let aIn := Ar_List rNone\nlet aR := Ar_List PlayerREF, aIn\nSetFunctionValue (Ar_HasInvalidRefs aR 1)"), 1.0);
    // The floats from the array [documented: the page's example].
    assert_eq!(n(&mut g, "let aR := Ar_List 3, 8, 5, 1\nAr_SetFloatsFrom aR fA fB fC\nSetFunctionValue fA * 100 + fB * 10 + fC"), 385.0);
    // Non-numeric elements and trailing variables beyond the array length stay unchanged
    // [observed: 2026-11-24, `w6_t1_13_sup_ar_setfloats`].
    assert_eq!(
        q(
            &mut g,
            "let aR := Ar_List 3.5, \"abc\", 8.25\nlet fA := 11\nlet fB := 22\nlet fC := 33\nlet fD := 44\nAr_SetFloatsFrom aR fA fB fC fD\nlet sOut := sv_Construct \"%.2f,%.2f,%.2f,%.2f\" fA fB fC fD\nSetFunctionValue sOut"
        ),
        XVal::Str("3.50,22.00,8.25,44.00".to_string())
    );
}

#[test]
fn a_function_frames_later() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "CallFunctionNextFrame (Begin Function { }\nPlayerREF.AuxVarSetFlt \"*sup4\" 1\nEnd) 3");
    for _ in 0..2 {
        g.tick();
    }
    assert_eq!(n(&mut g, "PlayerREF.AuxVarGetFlt \"*sup4\""), 0.0);
    g.tick();
    assert_eq!(n(&mut g, "PlayerREF.AuxVarGetFlt \"*sup4\""), 1.0);
}

/// GetNearCells' rule at the three outdoor spots of ext-sup4.md §5 (#2039): only loaded
/// cells; a cell with y < 0 is counted at x + 1, the player's own cell as it is; each
/// axis apart.
#[test]
fn near_cells_rule() {
    use fnv_game::nvse_sup4::near_in_grid;
    // A loaded grid, x and y ranges inclusive; each cell's id packs its coordinates.
    let grid = |xs: std::ops::RangeInclusive<i32>, ys: std::ops::RangeInclusive<i32>| -> Vec<((i32, i32), FormId)> {
        ys.flat_map(|y| xs.clone().map(move |x| ((x, y), FormId(((x + 100) as u32) << 8 | (y + 100) as u32)))).collect()
    };
    let coords = |cells: Vec<FormId>| -> Vec<(i32, i32)> { cells.into_iter().map(|c| ((c.0 >> 8) as i32 - 100, (c.0 & 0xFF) as i32 - 100)).collect() };
    let near = |g: &[((i32, i32), FormId)], at, d: f64| coords(near_in_grid(g.iter().copied(), at, d));
    let xs = |cells: &[(i32, i32)], y: i32| -> Vec<i32> { cells.iter().filter(|c| c.1 == y).map(|c| c.0).collect() };

    // At 9,-29, every cell south of the origin: depth 1 x 7..9, depth 2 x 7..10 (20 cells,
    // the sup4 row), depth 3, 4 and -1 the 25 loaded.
    let at_airport = grid(7..=11, -31..=-27);
    let d1 = near(&at_airport, (9, -29), 1.0);
    assert_eq!((d1.len(), xs(&d1, -29)), (9, vec![7, 8, 9]));
    let d2 = near(&at_airport, (9, -29), 2.0);
    assert_eq!((d2.len(), xs(&d2, -29)), (20, vec![7, 8, 9, 10]));
    for d in [3.0, 4.0, -1.0] {
        assert_eq!(near(&at_airport, (9, -29), d).len(), 25, "depth {d}");
    }
    assert_eq!(near(&at_airport, (9, -29), 0.0).len(), 1);

    // At 0,0: depth 1 gives row -1 as x -2..0 and rows 0 and 1 as -1..1; depth 2 gives 23,
    // rows -2 and -1 losing x 2.
    let at_origin = grid(-2..=2, -2..=2);
    let d1 = near(&at_origin, (0, 0), 1.0);
    assert_eq!((xs(&d1, -1), xs(&d1, 0), xs(&d1, 1)), (vec![-2, -1, 0], vec![-1, 0, 1], vec![-1, 0, 1]));
    let d2 = near(&at_origin, (0, 0), 2.0);
    assert_eq!((d2.len(), xs(&d2, -2), xs(&d2, -1)), (23, vec![-2, -1, 0, 1], vec![-2, -1, 0, 1]));

    // At -10,4 no cell has y < 0: a centred square cut to the 5 × 6 loaded grid.
    let north = grid(-12..=-8, 2..=7);
    assert_eq!(near(&north, (-10, 4), 1.0).len(), 9);
    assert_eq!(near(&north, (-10, 4), 2.0).len(), 25);
    for d in [3.0, 4.0, -1.0] {
        assert_eq!(near(&north, (-10, 4), d).len(), 30, "depth {d}");
    }
}

#[test]
fn saves_cells_and_the_rest() {
    let f = fixture();
    let mut g = game(&f);
    // Before any save or load: -1 and "" [documented].
    assert_eq!(n(&mut g, "GetSavedSaveSize 0"), -1.0);
    assert_eq!(s(&mut g, "GetSaveName 1"), "");
    g.save_files.saved = Some(SaveFile { name: "Save 3".into(), bytes: 12345 });
    g.save_files.loaded = Some(SaveFile { name: "quicksave".into(), bytes: 999 });
    assert_eq!(n(&mut g, "GetSavedSaveSize 0"), 12345.0);
    assert_eq!(n(&mut g, "GetLoadedSaveSize 0"), 999.0);
    assert_eq!(n(&mut g, "GetLoadedSaveSize 1"), 0.0);
    assert_eq!(s(&mut g, "GetSaveName 0"), "Save 3");
    assert_eq!(s(&mut g, "GetSaveName 1"), "quicksave");
    // A load keeps them.
    let session = g.take_session();
    g.put_session(session);
    assert_eq!(s(&mut g, "GetSaveName 1"), "quicksave");
    // Indoors the player's cell alone; no map markers.
    assert_eq!(n(&mut g, "let aR := GetNearCells 2\nSetFunctionValue aR[0]"), f64::from(CELL));
    assert_eq!(n(&mut g, "let aR := GetNearMapMarkers -1\nSetFunctionValue (Ar_Size aR)"), 0.0);
    // The caught flag, set and read.
    assert_eq!(n(&mut g, "TestMarkRef.SetCaughtPCPickpocketting"), 0.0);
    assert_eq!(n(&mut g, "TestMarkRef.SetCaughtPCPickpocketting 1"), 1.0);
    assert_eq!(n(&mut g, "TestMarkRef.SetCaughtPCPickpocketting"), 1.0);
    // No script runs a query: nothing marked, 1 all the same [observed: sup4 markonload].
    assert_eq!(n(&mut g, "MarkScriptOnLoad"), 1.0);
    assert_eq!(s(&mut g, "TranslitText \"Привет\""), "Privet");
    assert_eq!(n(&mut g, "AllocatePlayer"), 0.0);
}
