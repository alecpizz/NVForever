//! Authored source registrations and structured diagnostics; no installed mod inputs.
//! Standalone target owns one immutable file tree/reader for this test process.
mod common;
use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{Game, GameData};
use fnv_game::game::{GameConfig, RunCtx, LAMBDA_BASE};
use fnv_game::script_runner::Moment;
use fnv_game::trace::EventKind;
use fnv_script::{FunctionTable, VarDecl};
use fnv_script::compile::{Env, FormInfo};
use fnv_script::unit::RefSlot;
use fnv_script::vm::{EventCall, XVal};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use fnv_ui::MemSource;
use std::{collections::BTreeMap, sync::{Arc, Mutex, OnceLock}};
use tracing::{field::{Field, Visit}, Event, Subscriber};
use tracing_subscriber::{layer::{Context, Layer}, prelude::*};

const REGISTRAR: u32 = 0x900;
const CALLBACK: u32 = 0x901;
const OUTER: u32 = 0x902;
const BAD_A: u32 = LAMBDA_BASE + 90_001;
const BAD_B: u32 = LAMBDA_BASE + 90_002;
type Fields = BTreeMap<String, serde_json::Value>;

#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<Fields>>>);
struct Values(Fields);
impl Visit for Values {
    fn record_debug(&mut self, f: &Field, value: &dyn std::fmt::Debug) { self.0.insert(f.name().into(), format!("{value:?}").into()); }
    fn record_str(&mut self, f: &Field, value: &str) { self.0.insert(f.name().into(), value.into()); }
    fn record_u64(&mut self, f: &Field, value: u64) { self.0.insert(f.name().into(), value.into()); }
}
impl<S: Subscriber> Layer<S> for Warnings {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut fields = Values(BTreeMap::new());
        event.record(&mut fields);
        if fields.0.contains_key("handler_context") { self.0.lock().unwrap().push(fields.0); }
    }
}
fn capture(f: impl FnOnce()) -> Vec<Fields> {
    let warnings = Warnings::default();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(warnings.clone()), f);
    let out = std::mem::take(&mut *warnings.0.lock().unwrap());
    out
}

struct Names;
impl Env for Names {
    fn form(&self, name: &str) -> Option<FormInfo> {
        let form = match name { "OwnedRegistrar" => REGISTRAR, "OwnedCallback" => CALLBACK, "OwnedOuter" => OUTER, _ => return None };
        Some(FormInfo { form, sig: *b"SCPT" })
    }
    fn info(&self, form: u32) -> Option<FormInfo> { Some(FormInfo { form, sig: *b"SCPT" }) }
    fn editor_id(&self, _: u32) -> Option<String> { None }
    fn local_form(&self, form: u32) -> Option<u32> { Some(form) }
    fn script_vars(&self, _: u32) -> Option<Vec<VarDecl>> { None }
}
fn function(table: &FunctionTable, id: u32, name: &str, body: &str) -> Vec<u8> {
    let text = format!("scn {name}\nBegin Function {{}}\n{body}\nEnd\n");
    let compiled = fnv_script::compile::compile(&text, table, &Names, None);
    assert!(compiled.errors().next().is_none(), "{name}: {:?}", compiled.diags);
    let refs: Vec<_> = compiled.refs.iter().map(|r| match r { RefSlot::Form(f) => *f, _ => panic!("unexpected authored ref") }).collect();
    let mut fields = vec![sub(b"EDID", z(name))];
    fields.extend(unit(0, &compiled.bytes, &[], &refs));
    fields.push(sub(b"SCTX", text.into_bytes()));
    record(b"SCPT", 0, id, &fields)
}
struct Fixture { data: GameData, forms: FormStore, refs: RefTable }
fn fixture() -> Fixture {
    files();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let table = FunctionTable::load(&root.join("script/functions.json")).unwrap();
    let scripts = [
        function(&table, REGISTRAR, "OwnedRegistrar", "SetOnHitEventHandler OwnedCallback 1 0"),
        function(&table, CALLBACK, "OwnedCallback", &format!("Call {BAD_A}.0")),
        function(&table, OUTER, "OwnedOuter", "Call OwnedRegistrar"),
    ].concat();
    let bytes = [tes4(), top(b"SCPT", &scripts)].concat();
    let plugins = [Plugin::parse(&bytes, "OwnedRegistrars.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let catalogue = serde_json::from_str(&std::fs::read_to_string(root.join("script/functions.json")).unwrap()).unwrap();
    let data = GameData::build(&lo, &schema, table, &catalogue, &refs);
    Fixture { data, forms, refs }
}
fn game(f: &Fixture) -> Game<'_> {
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() });
    let src = MemSource::default().with("owned.xml", "<menu name=\"HUDMainMenu\"><rect name=\"Probe\"><_calls>0</_calls></rect></menu>");
    g.ui_tree.load_menu(&src, "owned.xml").unwrap(); g.ui_tree.update(); g
}
fn query(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}
fn compile(g: &mut Game, path: &str, force: bool) -> u32 {
    let value = fnv_game::nvse::call(g, &RunCtx::default(), "compilescript", None, &[XVal::Str(path.into()), XVal::Num(if force { 1.0 } else { 0.0 })]).unwrap().unwrap();
    match value { XVal::Ref(id) if id != 0 => id, _ => panic!("compile failed: {path}: {value:?}") }
}
fn run(g: &mut Game, id: u32) {
    g.run_function(&XVal::Ref(id), None, vec![], None,
        RunCtx { self_ref: Some(FormId(0xA10)), action_ref: Some(FormId(0xA11)), quest: Some(FormId(0xA12)) }).unwrap();
}
fn fire(g: &mut Game) -> usize { g.jip_event("hit", Some(FormId::PLAYER_REF), vec![]) }
fn neighbor(g: &mut Game) {
    query(g, "SetOnHitEventHandler (Begin Function {}\nSetUIFloat \"HUDMainMenu/Probe/_calls\" ((GetUIFloat \"HUDMainMenu/Probe/_calls\") + 1)\nEnd) 1 0");
}

// Virtual reader input only: never create an OS filename containing controls.
fn long_path() -> String { format!("頭\n\t{}尾.gek", "界".repeat(205)) }
fn files() {
    static INIT: OnceLock<std::path::PathBuf> = OnceLock::new();
    let root = INIT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("nvf-jip-provenance-{}", std::process::id()));
        let sr = root.join("nvse/plugins/scripts"); std::fs::create_dir_all(&sr).unwrap();
        std::fs::write(sr.join("gr_owned_a.txt"), format!("SetOnHitEventHandler {BAD_A}.0 1 0\n")).unwrap();
        std::fs::write(sr.join("gr_owned_b.txt"), format!("SetOnHitEventHandler {BAD_B}.0 1 0\n")).unwrap();
        std::fs::write(
            sr.join("gn_owned_mainloop.txt"),
            "SetGameMainLoopCallback (CompileScript \"mainloop_valid.gek\") 1 1 1\nSetGameMainLoopCallback (CompileScript \"missing_mainloop.gek\") 1 1 1\n",
        ).unwrap();
        root
    });
    let reader = Arc::new(|path: &str| {
        let short = path.strip_prefix("nvse\\user_defined_functions\\")?;
        let body = match short {
            "owned_a.gek" => format!("SetOnHitEventHandler {BAD_A}.0 1 0"),
            "owned_b.gek" => format!("SetOnHitEventHandler {BAD_B}.0 1 0"),
            "register_remover.gek" => "SetOnHitEventHandler (GetModLocalData \"callback\") 1 0".into(),
            "removing_callback.gek" => format!("SetOnHitEventHandler (GetModLocalData \"callback\") 0 0\nCall {BAD_A}.0"),
            "remove_pending_callback.gek" => format!("SetOnHitEventHandler {BAD_B}.0 0 0\nCall {BAD_A}.0"),
            "mainloop_valid.gek" => "SetUIFloat \"HUDMainMenu/Probe/_calls\" ((GetUIFloat \"HUDMainMenu/Probe/_calls\") + 1)".into(),
            "mainloop_zero_reg.gek" => "SetGameMainLoopCallback 0 1 1 1".into(),
            _ if short == long_path() => format!("SetOnHitEventHandler {BAD_A}.0 1 0"),
            _ => return None,
        };
        Some(format!("Begin Function {{}}\n{body}\nEnd\n").into_bytes())
    });
    fnv_game::mod_files::set_roots(fnv_game::mod_files::Roots { read: vec![root.clone()], reader: Some(reader), ..Default::default() });
}

#[test]
fn script_runner_files_report_distinct_registrars_while_valid_neighbor_keeps_running() {
    let f = fixture(); let mut g = game(&f);
    let runs = g.run_script_runner(Moment::Restart);
    assert_eq!(runs.len(), 2); assert!(runs.iter().all(|r| r.result.is_ok()));
    neighbor(&mut g);
    let rows = capture(|| { assert_eq!(fire(&mut g), 1); assert_eq!(fire(&mut g), 1); });
    assert_eq!(rows.len(), 4);
    for pair in rows.as_chunks::<2>().0 {
        assert_eq!(pair[0].get("registrar_kind"), Some(&serde_json::json!("script_runner")));
        assert_eq!(pair[0]["registrar_source"], "gr_owned_b.txt");
        assert_eq!(pair[1]["registrar_source"], "gr_owned_a.txt");
        assert_eq!(pair[0]["registrar_plugin"], "<none>");
        assert!(pair[0]["handler_context"].as_str().unwrap().contains(&BAD_B.to_string()));
    }
    assert_eq!(g.ui_tree.get_float("HUDMainMenu/Probe/_calls"), Some(2.0));
}

#[test]
fn force_compiled_old_ids_keep_their_own_file_and_run_context_within_the_bound() {
    let f = fixture(); let mut g = game(&f);
    let old = compile(&mut g, "owned_a.gek", false);
    let newer = compile(&mut g, "owned_a.gek", true); assert_ne!(old, newer);
    let other = compile(&mut g, "owned_b.gek", false);
    run(&mut g, old); run(&mut g, other);
    let rows = capture(|| { assert_eq!(fire(&mut g), 0); });
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get("registrar_kind"), Some(&serde_json::json!("compiled_file")));
    assert_eq!(rows[0]["registrar_compiled_id"], other);
    assert_eq!(rows[1]["registrar_compiled_id"], old);
    assert!(rows[1]["registrar_source"].as_str().unwrap().ends_with("owned_a.gek"));
    assert_eq!(rows[1]["registrar_plugin"], "<none>");
    assert_eq!(rows[1]["registrar_self_ref"], 0xA10);
    assert_eq!(rows[1]["registrar_action_ref"], 0xA11);
    assert_eq!(rows[1]["registrar_quest"], 0xA12);
}

#[test]
fn nested_record_registration_identifies_the_registrar_not_its_caller_or_callback() {
    let f = fixture(); let mut g = game(&f);
    query(&mut g, "Call OwnedOuter");
    let rows = capture(|| { assert_eq!(fire(&mut g), 0); });
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("registrar_kind"), Some(&serde_json::json!("record_unit")));
    assert_eq!(rows[0]["registrar_owner_form"], REGISTRAR);
    assert_ne!(rows[0]["registrar_owner_form"], CALLBACK);
    assert_ne!(rows[0]["registrar_owner_form"], OUTER);
    assert_eq!(rows[0]["registrar_plugin"], "ownedregistrars.esm");
    assert!(rows[0]["registrar_unit"].as_str().unwrap().contains("OwnedRegistrars.esm"));
}

#[test]
fn duplicate_preserves_first_registrar_and_removal_allows_a_fresh_query_registration() {
    let f = fixture(); let mut g = game(&f);
    let file = compile(&mut g, "owned_a.gek", false); run(&mut g, file);
    assert_eq!(query(&mut g, &format!("SetFunctionValue (SetOnHitEventHandler {BAD_A}.0 1 0)")), XVal::Num(1.0));
    let before = capture(|| { assert_eq!(fire(&mut g), 0); });
    assert_eq!(before.len(), 1); assert_eq!(before[0].get("registrar_compiled_id"), Some(&serde_json::json!(file)));
    for expected in [1.0, 0.0] {
        assert_eq!(query(&mut g, &format!("SetFunctionValue (SetOnHitEventHandler {BAD_A}.0 0 0)")), XVal::Num(expected));
    }
    assert!(capture(|| { assert_eq!(fire(&mut g), 0); }).is_empty());
    query(&mut g, &format!("SetOnHitEventHandler {BAD_A}.0 1 0"));
    let after = capture(|| { assert_eq!(fire(&mut g), 0); });
    assert_eq!(after[0]["registrar_kind"], "query");
    assert_eq!(after[0]["registrar_plugin"], "<none>");
    assert_eq!(after[0]["registrar_source_status"], "no_file");
    assert!(before[0]["handler_context"].as_str().unwrap().starts_with("registration=1 "));
    assert!(after[0]["handler_context"].as_str().unwrap().starts_with("registration=2 "));
}

#[test]
fn nested_self_removal_preserves_attempt_provenance_without_preserving_registration() {
    let f = fixture(); let mut g = game(&f);
    let callback = compile(&mut g, "removing_callback.gek", false);
    query(&mut g, &format!("SetModLocalData \"callback\" {callback}.0"));
    let registrar = compile(&mut g, "register_remover.gek", false); run(&mut g, registrar);
    let rows = capture(|| { assert_eq!(fire(&mut g), 0); });
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("registrar_compiled_id"), Some(&serde_json::json!(registrar)));
    assert!(rows[0]["registrar_source"].as_str().unwrap().ends_with("register_remover.gek"));
    assert!(capture(|| { assert_eq!(fire(&mut g), 0); }).is_empty());
}

#[test]
fn removing_an_older_pending_handler_keeps_its_dispatch_snapshot_origin() {
    let f = fixture(); let mut g = game(&f);
    let older = compile(&mut g, "owned_b.gek", false); run(&mut g, older);
    let remover = compile(&mut g, "remove_pending_callback.gek", false);
    query(&mut g, &format!("SetOnHitEventHandler {remover}.0 1 0"));
    let rows = capture(|| { assert_eq!(fire(&mut g), 0); });
    assert_eq!(rows.len(), 2, "the original handler snapshot still attempts the removed older handler");
    assert_eq!(rows[0].get("registrar_kind"), Some(&serde_json::json!("query")));
    assert_eq!(rows[1]["registrar_kind"], "compiled_file");
    assert_eq!(rows[1]["registrar_compiled_id"], older);
    assert!(rows[1]["registrar_source"].as_str().unwrap().ends_with("owned_b.gek"));
    let next = capture(|| { assert_eq!(fire(&mut g), 0); });
    assert_eq!(next.len(), 1, "removal applies to the next dispatch");
    assert_eq!(next[0]["registrar_kind"], "query");
}

#[test]
fn eviction_never_guesses_file_origin_or_changes_old_callable_execution() {
    let f = fixture(); let mut g = game(&f);
    let old = compile(&mut g, "owned_a.gek", false); run(&mut g, old);
    for _ in 0..128 { compile(&mut g, "owned_b.gek", true); }
    let retained = capture(|| { assert_eq!(fire(&mut g), 0); });
    assert_eq!(retained[0].get("registrar_kind"), Some(&serde_json::json!("compiled_file")), "an active registration owns its bounded metadata");
    query(&mut g, &format!("SetOnHitEventHandler {BAD_A}.0 0 0"));
    run(&mut g, old); // The old compiled unit remains executable after diagnostic eviction.
    let evicted = capture(|| { assert_eq!(fire(&mut g), 0); });
    assert_eq!(evicted[0]["registrar_kind"], "compiled_unavailable");
    assert_eq!(evicted[0]["registrar_source_status"], "not_retained");
    assert_eq!(evicted[0]["registrar_compiled_id"], old);
    assert_eq!(evicted[0]["registrar_source"], "<unavailable>");
}

#[test]
fn long_unicode_source_metadata_is_bounded_and_direct_api_sender_is_not_attribution() {
    let f = fixture(); let mut g = game(&f);
    let compiled = compile(&mut g, &long_path(), false); run(&mut g, compiled);
    let rows = capture(|| { assert_eq!(fire(&mut g), 0); });
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("registrar_kind"), Some(&serde_json::json!("compiled_file")));
    let source = rows[0]["registrar_source"].as_str().unwrap();
    assert!(source.ends_with("<truncated>"));
    assert!(!source.contains(['\n', '\t']));
    assert!(source.contains("頭\\n\\t"), "retained controls are escaped, not discarded");
    assert!(source.chars().count() <= 192 * 10 + 11);
    let mut direct = game(&f);
    assert_eq!(direct.event_call(EventCall::JipSet { event: "hit".into(), func: XVal::Num(f64::from(BAD_A)), add: true, filters: vec![XVal::Num(0.0)] }, "FakeRegistrar.esm").unwrap(), XVal::Num(1.0));
    let rows = capture(|| { assert_eq!(fire(&mut direct), 0); });
    assert_eq!(rows[0]["registrar_kind"], "unavailable");
    assert!(!rows[0].contains_key("registrar_plugin"));
    assert!(capture(|| { assert_eq!(fire(&mut game(&f)), 0); }).is_empty(), "a new Game inherits no registrations");
}

#[test]
fn dynamic_evaluation_and_console_have_distinct_runtime_origins_without_plugin_guesses() {
    let f = fixture();
    for (kind, register) in [("compiled_source", true), ("console", false)] {
        let mut g = game(&f);
        if register {
            assert_eq!(query(&mut g, &format!("SetFunctionValue (EvaluateString \"SetOnHitEventHandler {BAD_A}.0 1 0\")")), XVal::Num(1.0));
        } else {
            assert!(g.run_source(&format!("SetOnHitEventHandler {BAD_A}.0 1 0"), RunCtx::default()).unwrap().is_empty());
        }
        let rows = capture(|| { assert_eq!(fire(&mut g), 0); });
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("registrar_kind"), Some(&serde_json::json!(kind)));
        assert_eq!(rows[0]["registrar_plugin"], "<none>");
        assert_eq!(rows[0]["registrar_source_status"], "no_file");
        assert_eq!(rows[0]["registrar_source"], "<unavailable>");
        if register { assert_ne!(rows[0]["registrar_compiled_id"], 0); }
        else { assert_eq!(rows[0]["registrar_compiled_id"], 0); }
    }
}

#[test]
fn actual_source_provenance_preserves_the_unusable_error_gate_and_neighbor_frequency() {
    let f = fixture(); let mut g = game(&f);
    neighbor(&mut g);
    // Null cannot be a function script. Registration succeeds, then the original
    // error-key gate reports this unusable callback once across repeated events.
    query(&mut g, "SetOnHitEventHandler 0 1 0");
    let rows = capture(|| { for _ in 0..3 { assert_eq!(fire(&mut g), 1); } });
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("registrar_kind"), Some(&serde_json::json!("query")));
    assert_eq!(rows[0]["error"], "0x00000000 is not a function script");
    assert_eq!(g.ui_tree.get_float("HUDMainMenu/Probe/_calls"), Some(3.0));
    let errors: Vec<_> = g.events.iter().filter_map(|event| match &event.kind {
        EventKind::ScriptError { unit, error } => Some((unit.as_str(), error.as_str())), _ => None,
    }).collect();
    assert_eq!(errors, [("JIP hit handler", "0x00000000 is not a function script")]);
}

#[test]
fn main_loop_script_runner_and_compile_script_report_registrars_while_valid_callback_runs() {
    let f = fixture();
    let mut g = game(&f);

    // Script Runner NewGame file registers one valid CompileScript callback and one
    // missing CompileScript (form 0). The zero token is refused once with exact
    // ScriptRunner provenance while the valid callback runs live on Game::tick().
    let sr_rows = capture(|| {
        let runs = g.run_script_runner(Moment::NewGame);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].result.is_ok(), "{:?}", runs[0]);
    });
    assert_eq!(sr_rows.len(), 1, "expected one ScriptRunner zero-token warning: {sr_rows:?}");
    assert_eq!(sr_rows[0].get("registrar_kind"), Some(&serde_json::json!("script_runner")));
    assert_eq!(sr_rows[0]["registrar_source"], "gn_owned_mainloop.txt");
    assert_eq!(sr_rows[0]["registrar_plugin"], "<none>");
    assert!(sr_rows[0]["handler_context"].as_str().unwrap().contains("callable=ref:0x00000000"));

    let tick_rows = capture(|| {
        g.tick();
        g.tick();
    });
    assert!(tick_rows.is_empty(), "valid CompileScript callback must run without warnings: {tick_rows:?}");
    assert_eq!(g.ui_tree.get_float("HUDMainMenu/Probe/_calls"), Some(2.0));

    // A CompileScript file registering a zero main-loop callback reports compiled_file
    // provenance with its compiled ID, file origin, and RunCtx.
    let reg_id = compile(&mut g, "mainloop_zero_reg.gek", false);
    let cs_rows = capture(|| {
        run(&mut g, reg_id);
    });
    assert_eq!(cs_rows.len(), 1, "expected one CompileScript zero-token warning: {cs_rows:?}");
    assert_eq!(cs_rows[0].get("registrar_kind"), Some(&serde_json::json!("compiled_file")));
    assert_eq!(cs_rows[0]["registrar_compiled_id"], reg_id);
    assert!(cs_rows[0]["registrar_source"].as_str().unwrap().ends_with("mainloop_zero_reg.gek"));
    assert_eq!(cs_rows[0]["registrar_plugin"], "<none>");
    assert_eq!(cs_rows[0]["registrar_self_ref"], 0xA10);
    assert_eq!(cs_rows[0]["registrar_action_ref"], 0xA11);
    assert_eq!(cs_rows[0]["registrar_quest"], 0xA12);
}
