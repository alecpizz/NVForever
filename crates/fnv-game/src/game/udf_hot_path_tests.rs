//! Regression tests for zero-copy UDF/lambda dispatch and VM hot-path builtins:
//! recursive/re-entrant UDF calls, statement & expression-only lambdas with captured
//! state, multi-cell `GetRefs` walks, cached `GetLoadedTypeArray`, `GetFollowers`
//! filtering, and zero-alloc `SetUIFloatAlt`/`GetUIFloat` UI path walks.

use crate::common::*;
use crate::game::{Game, GameConfig, RunCtx};
use crate::GameData;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

const WRLD: FormId = FormId(0x100);
const CELL_00: FormId = FormId(0x200);
const CELL_10: FormId = FormId(0x201);
const CELL_20: FormId = FormId(0x202);
const NPC_BASE: FormId = FormId(0x400);
const WEAP_LOW: FormId = FormId(0x410);
const WEAP_A: FormId = FormId(0x810);
const WEAP_B: FormId = FormId(0x811);
const ACTOR_00: FormId = FormId(0x500);
const ACTOR_10: FormId = FormId(0x501);
const ACTOR_20: FormId = FormId(0x502);
const PKG_FOLLOW: FormId = FormId(0x600);
const PKG_WANDER: FormId = FormId(0x601);
const UDF_FACT: FormId = FormId(0x700);

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

struct TestEnv;

impl fnv_script::compile::Env for TestEnv {
    fn form(&self, name: &str) -> Option<fnv_script::compile::FormInfo> {
        if name.eq_ignore_ascii_case("UdfFact") {
            return Some(fnv_script::compile::FormInfo { form: UDF_FACT.0, sig: *b"SCPT" });
        }
        None
    }
    fn info(&self, form: u32) -> Option<fnv_script::compile::FormInfo> {
        Some(fnv_script::compile::FormInfo { form, sig: *b"SCPT" })
    }
    fn editor_id(&self, _: u32) -> Option<String> {
        None
    }
    fn local_form(&self, form: u32) -> Option<u32> {
        Some(form)
    }
    fn script_vars(&self, _: u32) -> Option<Vec<fnv_script::unit::VarDecl>> {
        None
    }
}

fn scpt(table: &FunctionTable, id: FormId, edid: &str, src: &str) -> Vec<u8> {
    let compiled = fnv_script::compile::compile(src, table, &TestEnv, None);
    assert!(compiled.errors().next().is_none(), "{edid}: {:?}", compiled.diags);
    let refs: Vec<u32> = compiled
        .refs
        .iter()
        .map(|r| match r {
            fnv_script::unit::RefSlot::Form(f) => *f,
            other => panic!("unexpected ref slot: {other:?}"),
        })
        .collect();
    let vars: Vec<(u32, &str, u8)> = compiled
        .variables
        .iter()
        .map(|v| {
            let ty = match v.ty {
                fnv_script::unit::VarType::Int => 1,
                _ => 0,
            };
            (v.index, v.name.as_str(), ty)
        })
        .collect();
    let mut fields = vec![sub(b"EDID", z(edid))];
    fields.extend(unit(0, &compiled.bytes, &vars, &refs));
    fields.push(sub(b"SCTX", src.as_bytes().to_vec()));
    record(b"SCPT", 0, id.0, &fields)
}

fn pack_follow(id: FormId, edid: &str, target: FormId) -> Vec<u8> {
    let mut pkdt = vec![0u8; 12];
    pkdt[4] = 1; // Follow
    let psdt = vec![0xFFu8, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0];
    let mut ptdt = Vec::with_capacity(16);
    ptdt.extend_from_slice(&0i32.to_le_bytes());
    ptdt.extend_from_slice(&target.0.to_le_bytes());
    ptdt.extend_from_slice(&0i32.to_le_bytes());
    ptdt.extend_from_slice(&0f32.to_le_bytes());
    record(b"PACK", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"PKDT", pkdt), sub(b"PSDT", psdt), sub(b"PTDT", ptdt)])
}

fn pack_wander(id: FormId, edid: &str) -> Vec<u8> {
    let mut pkdt = vec![0u8; 12];
    pkdt[4] = 4; // Wander
    let psdt = vec![0xFFu8, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0];
    record(b"PACK", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"PKDT", pkdt), sub(b"PSDT", psdt)])
}

fn ext_cell(id: FormId, gx: i32, gy: i32, achr_id: FormId, edid: &str, flags: u32) -> Vec<u8> {
    let x = gx as f32 * 4096.0 + 100.0;
    let y = gy as f32 * 4096.0 + 100.0;
    let mut data = Vec::with_capacity(24);
    data.extend_from_slice(&x.to_le_bytes());
    data.extend_from_slice(&y.to_le_bytes());
    data.extend_from_slice(&0.0f32.to_le_bytes());
    data.extend_from_slice(&[0u8; 12]);
    let placed = record(b"ACHR", flags, achr_id.0, &[sub(b"EDID", z(edid)), sub(b"NAME", NPC_BASE.0.to_le_bytes().to_vec()), sub(b"DATA", data)]);
    let cell_rec = record(b"CELL", 0, id.0, &[sub(b"DATA", vec![0]), sub(b"XCLC", [gx.to_le_bytes(), gy.to_le_bytes()].concat())]);
    [cell_rec, group(id.0.to_le_bytes(), 6, &group(id.0.to_le_bytes(), 9, &placed))].concat()
}

fn fixture() -> Fixture {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let table = FunctionTable::load(&path).unwrap();
    let fact_src = "scn UdfFact\nint n\nBegin Function {n}\n  if n <= 1\n    SetFunctionValue 1\n  else\n    int sub = Call UdfFact (n - 1)\n    SetFunctionValue (n * sub)\n  endif\nEnd\n";
    let wrld_children = [
        ext_cell(CELL_00, 0, 0, ACTOR_00, "Actor00", 0),
        ext_cell(CELL_10, 1, 0, ACTOR_10, "Actor10", 0),
        ext_cell(CELL_20, 2, 0, ACTOR_20, "Actor20", 0),
    ]
    .concat();
    let wrld_block = [
        record(b"WRLD", 0, WRLD.0, &[sub(b"EDID", z("TestWorld"))]),
        group(WRLD.0.to_le_bytes(), 1, &group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &wrld_children))),
    ]
    .concat();
    let bytes = [
        tes4(),
        top(
            b"WEAP",
            &[
                record(b"WEAP", 0, WEAP_LOW.0, &[sub(b"EDID", z("WeapLow"))]),
                record(b"WEAP", 0, WEAP_A.0, &[sub(b"EDID", z("WeapA"))]),
                record(b"WEAP", 0, WEAP_B.0, &[sub(b"EDID", z("WeapB"))]),
            ]
            .concat(),
        ),
        top(b"PACK", &[pack_follow(PKG_FOLLOW, "PkgFollowPlayer", FormId::PLAYER_REF), pack_wander(PKG_WANDER, "PkgWander")].concat()),
        top(
            b"NPC_",
            &[
                record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player"))]),
                record(b"NPC_", 0, NPC_BASE.0, &[sub(b"EDID", z("Scout"))]),
            ]
            .concat(),
        ),
        top(b"SCPT", &scpt(&table, UDF_FACT, "UdfFact", fact_src)),
        top(b"WRLD", &wrld_block),
    ]
    .concat();

    let plugins = vec![Plugin::parse(&bytes, "UdfHotPath.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let catalogue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &catalogue, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() })
}

fn eval_num(g: &mut Game<'_>, body: &str) -> f64 {
    let v = g
        .run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() })
        .unwrap_or_else(|e| panic!("{body}: {e}"));
    match v {
        XVal::Num(n) => n,
        XVal::Ref(r) => f64::from(r),
        other => panic!("expected numeric result, got {other:?}"),
    }
}

#[test]
fn recursive_udf_and_reentrant_lambdas_work_without_ast_cloning() {
    let f = fixture();
    let mut g = game(&f);

    // Recursive UDF call (5! = 120) exercises borrowed Block::body across nested frames.
    let fact = eval_num(&mut g, "SetFunctionValue (Call UdfFact 5)");
    assert_eq!(fact, 120.0);

    // Statement lambda with captured local mutation + nested expression-only lambda call.
    let lam_res = eval_num(
        &mut g,
        "int acc\n\
         int x\n\
         int y\n\
         ref add\n\
         ref dbl\n\
         let acc := 0\n\
         let add := (Begin Function {x}\n\
           let acc := acc + x\n\
           SetFunctionValue acc\n\
         End)\n\
         let dbl := ({y} => y * 2)\n\
         Call add (Call dbl 3)\n\
         Call add (Call dbl 4)\n\
         SetFunctionValue acc",
    );
    assert_eq!(lam_res, 14.0);
    // Expression-only lambda sets udf_value_set on direct call.
    let expr_res = eval_num(&mut g, "int a\nint b\nref f\nlet f := ({a, b} => a + b)\nSetFunctionValue (Call f 7 8)");
    assert_eq!(expr_res, 15.0);
}

#[test]
fn multi_cell_getrefs_cached_loaded_type_array_and_followers_match_semantics() {
    let f = fixture();
    let mut g = game(&f);

    // Place player in CELL_00; move ACTOR_20 from CELL_20 into CELL_10 so depth=1 sees both ACTOR_00, ACTOR_10, ACTOR_20.
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(CELL_00);
    g.world.get(ACTOR_20).unwrap().cell = Some(CELL_10);
    let depth1 = g.ref_walk_list(Some(CELL_00), 200, 1, false);
    assert_eq!(depth1, vec![ACTOR_00, ACTOR_10, ACTOR_20]);

    // Deleting ACTOR_10 hides it unless taken=true.
    g.world.set_deleted(ACTOR_10, true).unwrap();
    assert_eq!(g.ref_walk_list(Some(CELL_00), 200, 1, false), vec![ACTOR_00, ACTOR_20]);
    assert_eq!(g.ref_walk_list(Some(CELL_00), 200, 1, true), vec![ACTOR_00, ACTOR_10, ACTOR_20]);

    // Cached GameData::forms_of_type returns identical ordered list on first and repeat calls:
    // forms < 0x800 first, then file order.
    let first = g.data.forms_of_type(40);
    let second = g.data.forms_of_type(40);
    assert_eq!(first, vec![WEAP_LOW, WEAP_A, WEAP_B]);
    assert_eq!(first, second);

    // GetFollowers filters Follow/Accompany packages and present state accurately.
    g.extra.entry(ACTOR_00).or_default().current_package = Some(PKG_FOLLOW);
    g.extra.entry(ACTOR_20).or_default().current_package = Some(PKG_WANDER);
    g.extra.entry(ACTOR_10).or_default().current_package = Some(PKG_FOLLOW); // deleted -> excluded
    assert_eq!(g.followers_of(FormId::PLAYER_REF), vec![ACTOR_00]);

    // Zero-alloc UI path walk: SetUIFloatAlt / GetUIFloat / indexed FromFirst vs FromLast.
    let xml = r#"<menu name="TestHUD">
        <rect name="List">
            <rect name="Row"><x>10</x></rect>
            <rect name="Row"><x>20</x></rect>
            <rect name="Row"><x>30</x></rect>
        </rect>
    </menu>"#;
    let src = fnv_ui::MemSource::default().with("testhud.xml", xml);
    g.ui_tree.load_menu(&src, "testhud.xml").unwrap();
    assert_eq!(eval_num(&mut g, r#"SetFunctionValue (GetUIFloatAlt "TestHUD/List/Row:0/x")"#), 30.0);
    assert_eq!(eval_num(&mut g, r#"SetFunctionValue (GetUIFloat "TestHUD/List/Row:1/x")"#), 10.0);
    assert_eq!(eval_num(&mut g, r#"SetFunctionValue (GetUIFloat "TestHUD/List/Row:3/x")"#), -999.0);
    assert_eq!(eval_num(&mut g, r#"SetFunctionValue (SetUIFloatAlt "TestHUD/List/*:1/x" 99)"#), 1.0);
    assert_eq!(eval_num(&mut g, r#"SetFunctionValue (GetUIFloatAlt "TestHUD/List/Row:1/x")"#), 99.0);
}
