//! Owned script/record fixtures; no game data. A separate binary keeps the process-wide
//! file-reader fixture independent of the synthetic suite's parallel tests.
mod common;

use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{Game, GameConfig, GameData};
use fnv_script::FunctionTable;
use fnv_sim::{FormStore, RefTable, SimConfig, World};
use std::sync::Mutex;

static ROOTS_LOCK: Mutex<()> = Mutex::new(());

struct Fixture { data: GameData, forms: FormStore, refs: RefTable }
fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&common::tes4(), "Owned.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &json, &refs);
    Fixture { data, forms, refs }
}
fn game(f: &Fixture) -> Game<'_> {
    Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() })
}

#[test]
fn compile_script_sibling_paths_share_a_cache_entry_and_reject_root_escapes() {
    use fnv_script::vm::XVal;
    let _guard = ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = requests.clone();
    let reader: fnv_game::mod_files::Reader = std::sync::Arc::new(move |path: &str| {
        seen.lock().unwrap().push(path.to_string());
        path.eq_ignore_ascii_case("nvse\\compilescript\\owned\\answer.gek").then(|| b"Begin Function {}\nSetFunctionValue 42\nEnd\n".to_vec())
    });
    fnv_game::mod_files::set_roots(fnv_game::mod_files::Roots { reader: Some(reader), ..Default::default() });
    let f = fixture();
    let mut g = game(&f);
    let query = |g: &mut Game, body: &str| g.run_query(&format!("ref first\nref second\nBegin Function {{}}\n{body}\nEnd"), Default::default());
    assert_eq!(query(&mut g, "SetFunctionValue (Call (CompileScript \"../CompileScript/Owned/answer.gek\"))"), Ok(XVal::Num(42.0)));
    assert_eq!(query(&mut g, "let first := CompileScript \"../CompileScript/Owned/answer.gek\"\nlet second := CompileScript \"../CompileScript/Owned/Sub/../answer.gek\"\nSetFunctionValue (first == second)"), Ok(XVal::Num(1.0)));

    assert_eq!(query(&mut g, "let first := CompileScript \"../CompileScript/Owned/answer.gek\"\nlet second := GetUDFFromFile \"../CompileScript/Owned/Sub/../answer.gek\"\nSetFunctionValue (first == second)"), Ok(XVal::Num(1.0)));
    assert_eq!(query(&mut g, "let first := CompileScript \"../CompileScript/Owned/answer.gek\"\nlet second := CompileScript \"../CompileScript/Owned/answer.gek\" 1\nSetFunctionValue (first != second)"), Ok(XVal::Num(1.0)), "force recompilation keeps its behavior");
    let reads_before_invalid_paths = requests.lock().unwrap().len();
    for bad in ["../../../outside.gek", "C:/outside.gek", "//server/file.gek", "name:stream"] {
        assert_eq!(query(&mut g, &format!("SetFunctionValue (CompileScript \"{bad}\")")), Ok(XVal::Ref(0)), "{bad}");
    }
    assert_eq!(requests.lock().unwrap().len(), reads_before_invalid_paths, "rejected paths must not reach the reader");
    fnv_game::mod_files::set_roots(Default::default());
}

#[test]
fn compile_script_caches_missing_and_failing_lookups_until_forced() {
    use fnv_script::vm::XVal;
    use std::sync::{Arc, Mutex};
    let _guard = ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let fixed = Arc::new(Mutex::new(false));
    let seen = requests.clone();
    let is_fixed = fixed.clone();
    let reader: fnv_game::mod_files::Reader = Arc::new(move |path: &str| {
        seen.lock().unwrap().push(path.to_string());
        if path.eq_ignore_ascii_case("nvse\\user_defined_functions\\broken.gek") {
            return Some(if *is_fixed.lock().unwrap() {
                b"Begin Function {}\nSetFunctionValue 99\nEnd\n".to_vec()
            } else {
                b"Begin Function {}\nloop\nEnd\n".to_vec()
            });
        }
        None
    });
    fnv_game::mod_files::set_roots(fnv_game::mod_files::Roots { reader: Some(reader), ..Default::default() });
    let f = fixture();
    let mut g = game(&f);
    let query = |g: &mut Game, body: &str| g.run_query(&format!("Begin Function {{}}\n{body}\nEnd"), Default::default());

    // Missing and failing CompileScript lookups are cached when force == 0 (one disk read each).
    requests.lock().unwrap().clear();
    assert_eq!(query(&mut g, "SetFunctionValue (CompileScript \"missing.gek\")"), Ok(XVal::Ref(0)));
    assert_eq!(query(&mut g, "SetFunctionValue (CompileScript \"missing.gek\")"), Ok(XVal::Ref(0)));
    assert_eq!(query(&mut g, "SetFunctionValue (CompileScript \"broken.gek\")"), Ok(XVal::Ref(0)));
    assert_eq!(query(&mut g, "SetFunctionValue (CompileScript \"broken.gek\")"), Ok(XVal::Ref(0)));
    let reads: Vec<String> = requests.lock().unwrap().clone();
    assert_eq!(reads.len(), 2, "repeated non-forced CompileScript failures must be cached: {reads:?}");

    // force == 1 bypasses the cached failure and compiles once the file is fixed.
    *fixed.lock().unwrap() = true;
    assert_eq!(query(&mut g, "SetFunctionValue (Call (CompileScript \"broken.gek\" 1))"), Ok(XVal::Num(99.0)));
    // Subsequent non-forced call reuses the newly cached compiled ID without re-reading disk.
    let reads_after_force = requests.lock().unwrap().len();
    assert_eq!(query(&mut g, "SetFunctionValue (Call (CompileScript \"broken.gek\"))"), Ok(XVal::Num(99.0)));
    assert_eq!(requests.lock().unwrap().len(), reads_after_force);

    fnv_game::mod_files::set_roots(Default::default());
}
