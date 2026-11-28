//! JIP's Script Runner (#1460, docs/spec-notes/ext-script-runner.md): files under
//! `Data\NVSE\plugins\scripts` run at the moment their prefix names, RegisterSRScript adds
//! files from subfolders while the game restart's run, and a file that does not compile is
//! skipped. A synthetic plugin and made-up files (no game data).

mod common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig};
use fnv_game::script_runner::Moment;
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

fn glob(fid: u32, edid: &str) -> Vec<u8> {
    record(b"GLOB", 0, fid, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", 0f32.to_le_bytes().to_vec())])
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let globs = [
        glob(0x800, "SROrder"),
        glob(0x801, "SRLoads"),
        glob(0x802, "SRNew"),
        glob(0x803, "SRSize"),
        glob(0x804, "SRRegistered"),
        glob(0x805, "SRLate"),
        glob(0x806, "SRRefRead"),
        glob(0x807, "SRTplRead"),
        glob(0x808, "SRQuestRead"),
    ]
    .concat();
    let scpt = |fid, edid: &str, vars: &[(u32, &str, u8)]| {
        let mut fields = vec![sub(b"EDID", z(edid))];
        fields.extend(unit(0, &script(&[(0, None, vec![])]), vars, &[]));
        record(b"SCPT", 0, fid, &fields)
    };
    let scripts = [
        scpt(0x810, "SRDirectActorScript", &[(1, "ActorFlag", 1)]),
        scpt(0x811, "SRTplActorScript", &[(1, "TplFlag", 1)]),
        scpt(0x812, "SRQuestScript", &[(1, "QuestFlag", 1)]),
    ]
    .concat();
    let mut tpl_acbs = vec![0u8; 24];
    tpl_acbs[22..24].copy_from_slice(&(1u16 << 9).to_le_bytes());
    let npcs = [
        record(b"NPC_", 0, 0x820, &[sub(b"EDID", z("SRDirectNpc")), sub(b"SCRI", 0x810u32.to_le_bytes().to_vec())]),
        record(b"NPC_", 0, 0x821, &[sub(b"EDID", z("SRTemplateNpc")), sub(b"SCRI", 0x811u32.to_le_bytes().to_vec())]),
        record(b"NPC_", 0, 0x822, &[sub(b"EDID", z("SRChildNpc")), sub(b"ACBS", tpl_acbs), sub(b"TPLT", 0x821u32.to_le_bytes().to_vec())]),
    ]
    .concat();
    let quest = record(b"QUST", 0, 0x830, &[sub(b"EDID", z("SRControlQuest")), sub(b"SCRI", 0x812u32.to_le_bytes().to_vec())]);
    let refs = [
        record(b"ACHR", 0x400, 0x840, &[sub(b"EDID", z("SRDirectRef")), sub(b"NAME", 0x820u32.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]),
        record(b"ACHR", 0x400, 0x841, &[sub(b"EDID", z("SRTemplatedRef")), sub(b"NAME", 0x822u32.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]),
    ]
    .concat();
    let grp = |label: [u8; 4], kind: u32, body: &[u8]| [b"GRUP".as_slice(), &((24 + body.len()) as u32).to_le_bytes(), &label, &kind.to_le_bytes(), &0u64.to_le_bytes(), body].concat();
    let cell_id: u32 = 0x850;
    let cell = grp(
        0i32.to_le_bytes(),
        2,
        &grp(
            0i32.to_le_bytes(),
            3,
            &[
                record(b"CELL", 0, cell_id, &[sub(b"EDID", z("SRCell")), sub(b"DATA", vec![1])]),
                grp(cell_id.to_le_bytes(), 6, &grp(cell_id.to_le_bytes(), 8, &refs)),
            ]
            .concat(),
        ),
    );
    let bytes = [
        tes4(),
        top(b"GLOB", &globs),
        top(b"SCPT", &scripts),
        top(b"NPC_", &npcs),
        top(b"QUST", &quest),
        top(b"CELL", &cell),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "SR.esm".into()).unwrap()];
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

/// Serialize process-global `mod_files::ROOTS` across tests in this integration binary.
static ROOTS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The Data folder of the run, written once per test process.
fn files() -> std::sync::MutexGuard<'static, ()> {
    use fnv_game::mod_files::{set_roots, Roots};
    let guard = ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("nvf-script-runner-{}", std::process::id()));
    static WRITE: std::sync::Once = std::sync::Once::new();
    WRITE.call_once(|| {
        let scripts = dir.join("NVSE/Plugins/Scripts");
        std::fs::create_dir_all(scripts.join("optional")).unwrap();
        let w = |name: &str, text: &str| std::fs::write(scripts.join(name), text).unwrap();
        // Run in the order of their names: 1, then 2.
        w("gr_b.txt", "set SROrder to SROrder * 10 + 2\r\n");
        w("GR_a.txt", "set SROrder to SROrder * 10 + 1\r\n");
        // Inline declarations and an array that outlives the file.
        w("gr_c.txt", "let array_var aList := ar_List 1, 2, 3\r\nint iSize = ar_Size aList\r\nset SRSize to iSize\r\n");
        // Registers a subfolder's file; the same name again, a file at the top, a missing
        // file and one that does not compile are refused.
        w(
            "gr_d.txt",
            "set SRRegistered to RegisterSRScript \"optional\\gl_extra.txt\"\r\n\
             set SRRegistered to SRRegistered * 10 + RegisterSRScript \"optional\\gl_extra.txt\"\r\n\
             set SRRegistered to SRRegistered * 10 + RegisterSRScript \"gl_top.txt\"\r\n\
             set SRRegistered to SRRegistered * 10 + RegisterSRScript \"optional\\gl_missing.txt\"\r\n\
             set SRRegistered to SRRegistered * 10 + RegisterSRScript \"optional\\gl_broken.txt\"\r\n\
             set SRRegistered to SRRegistered * 10 + RegisterSRScript \"optional\\gr_late.txt\"\r\n",
        );
        w("optional/gl_extra.txt", "set SRLoads to SRLoads + 100\r\n");
        // Does not compile: Set without To (an unknown name alone is a call in a mod's file, #2275).
        w("optional/gl_broken.txt", "set SRLoads 1\r\n");
        // A registered gr_ file runs after the folder's gr_ files.
        w("optional/gr_late.txt", "set SRLate to SROrder\r\n");
        w("gl_top.txt", "set SRLoads to SRLoads + 1\r\n");
        w("ln_both.txt", "set SRLoads to SRLoads + 10\r\nset SRNew to SRNew + 10\r\n");
        // Remote variable reads and writes on persistent actor refs (direct and templated)
        // plus Quest.Var control and a non-existent var guard.
        w(
            "ln_refvars.txt",
            "if SRDirectRef.ActorFlag == 4\r\n\
             set SRRefRead to 4\r\n\
             set SRDirectRef.ActorFlag to 5\r\n\
             let SRDirectRef. ActorFlag := SRDirectRef.ActorFlag + 2\r\n\
             endif\r\n\
             if SRTemplatedRef.TplFlag == 9\r\n\
             set SRTplRead to 9\r\n\
             SRTemplatedRef.TplFlag = 11\r\n\
             endif\r\n\
             if SRControlQuest.QuestFlag == 3\r\n\
             set SRQuestRead to 3\r\n\
             let SRControlQuest.QuestFlag := 8\r\n\
             endif\r\n\
             if SRDirectRef.NoSuchVar == 1\r\n\
             set SRRefRead to 999\r\n\
             endif\r\n",
        );
        w("gn_new.txt", "set SRNew to SRNew + 1\r\n");
        // Outside the game restart RegisterSRScript does nothing.
        w("gn_register.txt", "set SRNew to SRNew + 1000 * RegisterSRScript \"optional\\gl_extra.txt\"\r\n");
        // Skipped: does not compile; no known prefix; not a text file.
        w("gr_broken.txt", "set SROrder 99\r\n");
        w("readme.txt", "set SROrder to 99\r\n");
        w("gr_notes.ini", "set SROrder to 99\r\n");
        // A private auxiliary variable (no `_`) the file writes, which a CompileScript'd
        // function reads back (MCM Extender's pattern).
        w("gr_e.txt", "SRSize.AuxVarSetFlt \"*Shared\" 7 -1\r\n");
        let udfs = dir.join("NVSE/user_defined_functions");
        std::fs::create_dir_all(&udfs).unwrap();
        std::fs::write(udfs.join("srprobe.txt"), "Begin Function { }\r\nSetFunctionValue (SRSize.AuxVarGetFlt \"*Shared\" 0)\r\nEnd\r\n").unwrap();
    });
    set_roots(Roots { read: vec![dir], ..Roots::default() });
    guard
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    Game::new(&f.data, world, GameConfig::default())
}

fn value(g: &mut Game, expr: &str) -> f64 {
    match g.run_query(&format!("Begin Function {{ }}\nSetFunctionValue {expr}\nEnd\n"), Default::default()).unwrap() {
        XVal::Num(n) => n,
        v => panic!("{expr}: {v:?}"),
    }
}

#[test]
fn each_prefix_runs_at_its_moment() {
    let _guard = files();
    let f = fixture();
    let mut g = game(&f);
    let runs = g.run_script_runner(Moment::Restart);
    let names: Vec<&str> = runs.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(names, ["GR_a.txt", "gr_b.txt", "gr_broken.txt", "gr_c.txt", "gr_d.txt", "gr_e.txt", "optional\\gr_late.txt"]);
    assert_eq!(value(&mut g, "SROrder"), 12.0, "the folder's gr_ files in the order of their names");
    assert_eq!(value(&mut g, "SRLate"), 12.0, "a registered gr_ file runs after them");
    assert_eq!(value(&mut g, "SRSize"), 3.0);
    assert_eq!(value(&mut g, "SRLoads") + value(&mut g, "SRNew"), 0.0, "no other prefix runs at the restart");
    let broken = runs.iter().find(|r| r.path == "gr_broken.txt").unwrap();
    assert!(broken.result.as_ref().is_err_and(|e| e.starts_with("line 1: ")), "{broken:?}");
    assert!(runs.iter().filter(|r| r.path != "gr_broken.txt").all(|r| r.result.is_ok()), "{runs:?}");

    g.run_script_runner(Moment::NewGame);
    assert_eq!(value(&mut g, "SRNew"), 11.0, "gn_ and ln_ at a new game; RegisterSRScript only at the restart");
    assert_eq!(value(&mut g, "SRLoads"), 10.0, "ln_ at a new game, gl_ not");

    g.run_script_runner(Moment::Load);
    // ln_ 10 more, gl_top 1, the registered gl_extra 100.
    assert_eq!(value(&mut g, "SRLoads"), 121.0);
    assert_eq!(value(&mut g, "SROrder"), 12.0, "gr_ files run once");
}

#[test]
fn register_sr_script_takes_subfolder_files_once() {
    let _guard = files();
    let f = fixture();
    let mut g = game(&f);
    g.run_script_runner(Moment::Restart);
    // Registered; the same name again 0; a file at the top 0; missing 0; does not compile 0;
    // the gr_ file 1.
    assert_eq!(value(&mut g, "SRRegistered"), 100001.0);
}

#[test]
fn registered_files_last_for_the_session() {
    let _guard = files();
    let f = fixture();
    let mut g = game(&f);
    g.run_script_runner(Moment::Restart);
    let session = g.take_session();
    let mut g2 = game(&f);
    g2.put_session(session);
    g2.run_script_runner(Moment::Load);
    assert_eq!(value(&mut g2, "SRLoads"), 111.0, "gl_top, ln_both and the registered gl_extra");
}

#[test]
fn script_runner_files_and_compiled_scripts_share_private_aux_vars() {
    // (scripts compiled at run time have no plugin of their own: see crates/fnv-game/src/game.rs; observed: MCM
    // Extender's gr_ file writes `*MCMExt_ModName`, its CompileScript'd handler reads it,
    // Begin Again list 2026-11-13)
    let _guard = files();
    let f = fixture();
    let mut g = game(&f);
    g.run_script_runner(Moment::Restart);
    assert_eq!(value(&mut g, "SRSize.AuxVarGetFlt \"*Shared\" 0"), 7.0, "a query reads it");
    assert_eq!(value(&mut g, "Call (CompileScript \"srprobe.txt\")"), 7.0, "so does a CompileScript'd function");
}

fn fresh_temp_dir(tag: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("nvf-sr-{tag}-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn exit_scripts_compile_wait_for_exit_run_once_in_order_and_flush_cached_ini() {
    use fnv_game::mod_files::{set_roots, Roots};
    let _guard = ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let base = fresh_temp_dir("gx-flush");
    let data = base.join("game/Data");
    let layer = base.join("mods");
    let scripts = data.join("NVSE/Plugins/Scripts");
    std::fs::create_dir_all(scripts.join("optional")).unwrap();
    std::fs::create_dir_all(&layer).unwrap();

    // Seed an in-memory cached INI key during game restart (not written to disk yet)
    // and register a subfolder exit script.
    std::fs::write(
        scripts.join("gr_seed.txt"),
        "GetINIFloatOrCreate_Cached \"General:fExitFlushVal\" \"exit_cache_test.ini\" 42.5\r\n\
         set SRRegistered to RegisterSRScript \"optional\\gx_late.txt\"\r\n",
    )
    .unwrap();
    // Uppercase prefix sorts before gx_b.txt; RegisterSRScript outside Restart must return 0.
    std::fs::write(
        scripts.join("GX_a.txt"),
        "set SROrder to SROrder * 10 + 1\r\n\
         set SRLate to RegisterSRScript \"optional\\gx_other.txt\"\r\n",
    )
    .unwrap();
    // Flush cached INI to disk and clear its cache entry on exit.
    std::fs::write(
        scripts.join("gx_b.txt"),
        "set SROrder to SROrder * 10 + 2\r\n\
         SaveCachedIni \"exit_cache_test.ini\"\r\n\
         CloseFileSO \"exit_cache_test.ini\"\r\n",
    )
    .unwrap();
    // Broken gx_ file is skipped without stopping neighboring or registered gx_ scripts.
    std::fs::write(scripts.join("gx_broken.txt"), "set SROrder 99\r\n").unwrap();
    std::fs::write(scripts.join("optional/gx_late.txt"), "set SROrder to SROrder * 10 + 3\r\n").unwrap();
    std::fs::write(scripts.join("optional/gx_other.txt"), "set SROrder to SROrder * 10 + 9\r\n").unwrap();

    set_roots(Roots { read: vec![layer.clone(), data.clone()], write: Some(layer.clone()), ..Roots::default() });
    let f = fixture();
    let mut g = game(&f);

    // Discovery and compilation include gx_ files alongside gr_ files.
    let compiled = g.compile_script_runner();
    let compiled_names: Vec<&str> = compiled.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(compiled_names, ["gr_seed.txt", "GX_a.txt", "gx_b.txt", "gx_broken.txt"]);
    assert!(compiled.iter().find(|r| r.path == "GX_a.txt").unwrap().result.is_ok());
    assert!(compiled.iter().find(|r| r.path == "gx_b.txt").unwrap().result.is_ok());
    assert!(compiled.iter().find(|r| r.path == "gx_broken.txt").unwrap().result.is_err());

    // Startup, new-game, and load stages must NOT run gx_ scripts.
    g.run_script_runner(Moment::Restart);
    g.run_script_runner(Moment::NewGame);
    g.run_script_runner(Moment::Load);
    assert_eq!(value(&mut g, "SRRegistered"), 1.0, "subfolder gx_ script registered during Restart");
    assert_eq!(value(&mut g, "SROrder"), 0.0, "gx_ files must not run during Restart/NewGame/Load");
    let flushed_ini = layer.join("Config/exit_cache_test.ini");
    assert!(!flushed_ini.exists(), "cached INI must not be flushed to disk before exit");

    // Exit stage runs folder gx_ files in name order, skips broken ones, runs registered gx_ files.
    let runs = g.run_exit_scripts();
    let run_names: Vec<&str> = runs.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(run_names, ["GX_a.txt", "gx_b.txt", "gx_broken.txt", "optional\\gx_late.txt"]);
    assert!(runs.iter().filter(|r| r.path != "gx_broken.txt").all(|r| r.result.is_ok()), "{runs:?}");
    assert!(runs.iter().find(|r| r.path == "gx_broken.txt").unwrap().result.is_err());
    assert_eq!(value(&mut g, "SROrder"), 123.0, "GX_a (1), gx_b (2), registered optional\\gx_late (3)");
    assert_eq!(value(&mut g, "SRLate"), 0.0, "RegisterSRScript does nothing during Moment::Exit");
    assert!(flushed_ini.is_file(), "gx_b flushed cached INI synchronously to disk");
    let ini_text = std::fs::read_to_string(&flushed_ini).unwrap();
    assert!(ini_text.contains("42.5"), "persisted INI text should hold flushed value: {ini_text:?}");

    // A fresh Game reads the flushed INI back from disk.
    let mut g_next = game(&f);
    assert_eq!(value(&mut g_next, "GetINIFloat \"General:fExitFlushVal\" \"exit_cache_test.ini\""), 42.5);

    // Idempotent: a second exit call runs nothing and leaves state untouched.
    let second = g.run_exit_scripts();
    assert!(second.is_empty(), "repeated run_exit_scripts must be a no-op: {second:?}");
    assert_eq!(value(&mut g, "SROrder"), 123.0);

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn registered_exit_scripts_survive_session_restore() {
    use fnv_game::mod_files::{set_roots, Roots};
    let _guard = ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let base = fresh_temp_dir("gx-session");
    let scripts = base.join("NVSE/Plugins/Scripts");
    std::fs::create_dir_all(scripts.join("optional")).unwrap();
    std::fs::write(scripts.join("gr_reg.txt"), "set SRRegistered to RegisterSRScript \"optional\\gx_kept.txt\"\r\n").unwrap();
    std::fs::write(scripts.join("optional/gx_kept.txt"), "set SROrder to 77\r\n").unwrap();

    set_roots(Roots { read: vec![base.clone()], ..Roots::default() });
    let f = fixture();
    let mut g = game(&f);
    g.run_script_runner(Moment::Restart);
    assert_eq!(value(&mut g, "SRRegistered"), 1.0);
    let session = g.take_session();
    let mut g2 = game(&f);
    g2.put_session(session);
    let runs = g2.run_exit_scripts();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].path, "optional\\gx_kept.txt");
    assert!(runs[0].result.is_ok());
    assert_eq!(value(&mut g2, "SROrder"), 77.0);

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn absent_exit_scripts_are_a_no_op() {
    use fnv_game::mod_files::{set_roots, Roots};
    let _guard = ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let base = fresh_temp_dir("gx-absent");
    set_roots(Roots { read: vec![base.clone()], ..Roots::default() });
    let f = fixture();
    let mut g = game(&f);
    let before_events = g.events.len();
    let runs = g.run_exit_scripts();
    assert!(runs.is_empty());
    assert_eq!(g.events.len(), before_events);
    assert_eq!(value(&mut g, "SROrder"), 0.0);

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn script_runner_and_queries_resolve_placed_ref_vars() {
    let _guard = files();
    let f = fixture();
    let mut g = game(&f);

    // Seed remote vars on both direct-scripted and template-scripted actor refs + quest control.
    g.run_query(
        "Begin Function { }\n\
         set SRDirectRef.ActorFlag to 4\n\
         let SRTemplatedRef.TplFlag := 9\n\
         set SRControlQuest.QuestFlag to 3\n\
         End\n",
        Default::default(),
    )
    .unwrap();

    let runs = g.run_script_runner(Moment::Load);
    let rv = runs.iter().find(|r| r.path == "ln_refvars.txt").expect("ln_refvars.txt ran");
    assert!(rv.result.is_ok(), "{rv:?}");

    assert_eq!(value(&mut g, "SRRefRead"), 4.0, "direct ref read matched and non-existent var guard did not overwrite");
    assert_eq!(value(&mut g, "SRDirectRef.ActorFlag"), 7.0, "set and let updated direct actor ref var");
    assert_eq!(value(&mut g, "SRTplRead"), 9.0, "templated ref read matched");
    assert_eq!(value(&mut g, "SRTemplatedRef.TplFlag"), 11.0, "let macro updated templated actor ref var");
    assert_eq!(value(&mut g, "SRQuestRead"), 3.0, "quest remote var control matched");
    assert_eq!(value(&mut g, "SRControlQuest.QuestFlag"), 8.0, "quest remote var write unchanged");

    // Negative test: strict compile rejects an undeclared remote variable on a placed ref.
    let err = g
        .run_query("Begin Function { }\nSetFunctionValue SRDirectRef.NoSuchVar\nEnd\n", Default::default())
        .unwrap_err();
    assert_eq!(err, "script did not compile");
    let compiled = fnv_script::compile::compile_runtime(
        "Begin Function { }\nSetFunctionValue SRDirectRef.NoSuchVar\nEnd\n",
        &f.data.table,
        &fnv_game::game::GameEnv { g: &g },
    );
    assert_eq!(compiled.errors().next().map(|e| e.kind), Some("unresolved_remote_variable"));
}
