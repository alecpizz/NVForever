//! xNVSE delayed-call pause and MenuMode countdown behavior (`crates/fnv-game/src/delayed.rs`,
//! docs/spec-notes/ext-udf.md §6): `CallAfterSeconds` / `CallForSeconds` / `CallAfterFrames`
//! `runWhen` 0/1/2 (including MenuMode countdown and pause in Start/Main/Console) and
//! `CallWhile` / `CallWhilePerSeconds` `flags` (`DontRunInMenuMode`, `DontRunInGameMode`,
//! `DontRunWhilePaused`).
use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{
    game::{menus, RunCtx},
    Game, GameConfig, GameData,
};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

const G_MODE0: u32 = 0xA00;
const G_MODE1: u32 = 0xA01;
const G_MODE2: u32 = 0xA02;
const G_WHILE_CALLS: u32 = 0xA03;
const G_WHILE_ARG_SUM: u32 = 0xA04;
const G_FRAMES2: u32 = 0xA05;
const G_FRAMES0: u32 = 0xA06;
const G_FOR_SECS: u32 = 0xA07;
const G_PER_SECS: u32 = 0xA08;
const G_STEWIE_VER: u32 = 0xA09;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn global_rec(id: u32, edid: &str) -> Vec<u8> {
    record(
        b"GLOB",
        0,
        id,
        &[
            sub(b"EDID", z(edid)),
            sub(b"FNAM", vec![b'f']),
            sub(b"FLTV", 0f32.to_le_bytes().to_vec()),
        ],
    )
}

fn udf_rec(id: u32, edid: &str, src: &str) -> Vec<u8> {
    let src = src.replacen("\nBegin Function", "\nstring_var sourceMarker\nBegin Function", 1);
    let mut fields = vec![sub(b"EDID", z(edid))];
    fields.extend(unit(0, &script(&[(0, None, Vec::new())]), &[], &[]));
    fields.push(sub(b"SCTX", src.into_bytes()));
    record(b"SCPT", 0, id, &fields)
}

fn fixture() -> Fixture {
    let globals = [
        global_rec(G_MODE0, "Mode0Hits"),
        global_rec(G_MODE1, "Mode1Hits"),
        global_rec(G_MODE2, "Mode2Hits"),
        global_rec(G_WHILE_CALLS, "WhileCalls"),
        global_rec(G_WHILE_ARG_SUM, "WhileArgSum"),
        global_rec(G_FRAMES2, "Frames2Hits"),
        global_rec(G_FRAMES0, "Frames0Hits"),
        global_rec(G_FOR_SECS, "ForSecsHits"),
        global_rec(G_PER_SECS, "PerSecsHits"),
        global_rec(G_STEWIE_VER, "StewieVer"),
    ]
    .concat();

    let scripts = [
        udf_rec(0xB00, "HitMode0", "scn HitMode0\nBegin Function {}\n  set Mode0Hits to Mode0Hits + 1\nEnd\n"),
        udf_rec(0xB01, "HitMode1", "scn HitMode1\nBegin Function {}\n  set Mode1Hits to Mode1Hits + 1\nEnd\n"),
        udf_rec(0xB02, "HitMode2", "scn HitMode2\nBegin Function {}\n  set Mode2Hits to Mode2Hits + 1\nEnd\n"),
        udf_rec(
            0xB03,
            "OnWhileUpdate",
            "scn OnWhileUpdate\nint a\nint b\nBegin Function {a, b}\n  set WhileCalls to WhileCalls + 1\n  set WhileArgSum to a + b\nEnd\n",
        ),
        udf_rec(
            0xB04,
            "StartWhileOnKey",
            "scn StartWhileOnKey\nint keyCode\nBegin Function {keyCode}\n  CallWhile OnWhileUpdate ({} => IsKeyPressed keyCode == 1 || IsButtonPressed keyCode == 1) 25 1053 1008\nEnd\n",
        ),
        udf_rec(0xB05, "HitFrames2", "scn HitFrames2\nBegin Function {}\n  set Frames2Hits to Frames2Hits + 1\nEnd\n"),
        udf_rec(0xB06, "HitFrames0", "scn HitFrames0\nBegin Function {}\n  set Frames0Hits to Frames0Hits + 1\nEnd\n"),
        udf_rec(0xB07, "HitForSecs", "scn HitForSecs\nBegin Function {}\n  set ForSecsHits to ForSecsHits + 1\nEnd\n"),
        udf_rec(
            0xB08,
            "HitPerSecs",
            "scn HitPerSecs\nint bonus\nBegin Function {bonus}\n  set PerSecsHits to PerSecsHits + bonus\nEnd\n",
        ),
    ]
    .concat();

    let bytes = [tes4(), top(b"GLOB", &globals), top(b"SCPT", &scripts)].concat();
    let plugins = [Plugin::parse(&bytes, "DelayedCalls.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let functions = root.join("script/functions.json");
    let catalogue = serde_json::from_slice(&std::fs::read(&functions).unwrap()).unwrap();
    let forms = FormStore::build(&order, &schema);
    let refs = RefTable::build(&order, &schema);
    let data = GameData::build(&order, &schema, FunctionTable::load(&functions).unwrap(), &catalogue, &refs);
    Fixture { data, forms, refs }
}

fn new_game(f: &Fixture) -> Game<'_> {
    Game::new(
        &f.data,
        World::new(&f.forms, &f.refs, SimConfig::default()),
        GameConfig { strict: true, frame_seconds: 0.02, ..Default::default() },
    )
}

fn run(g: &mut Game<'_>, body: &str) {
    g.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), RunCtx::default()).unwrap();
}

fn glob(g: &Game<'_>, id: u32) -> f32 {
    g.world.global(FormId(id)).unwrap()
}

#[test]
fn call_after_seconds_counts_in_menu_mode_and_pauses_in_start_and_console() {
    let f = fixture();
    let mut g = new_game(&f);

    // Queue 0.06s (3 ticks at dt = 0.02) under runWhen 0 (GameMode only),
    // 1 (always), and 2 (GameMode + MenuMode, paused in Start/Main/Console).
    run(
        &mut g,
        r#"
        CallAfterSeconds 0.06 HitMode0 0
        CallAfterSeconds 0.06 HitMode1 1
        CallAfter 0.06 HitMode2 2
        "#,
    );

    // 1 tick in StartMenu (1013): only runWhen 1 advances (remaining 0.04); 0 and 2 stay at 0.06.
    g.open_menu(menus::START);
    g.tick();
    assert_eq!((glob(&g, G_MODE0), glob(&g, G_MODE1), glob(&g, G_MODE2)), (0.0, 0.0, 0.0));

    // 2 ticks in Console (3): runWhen 1 reaches 0 and fires; runWhen 0 and 2 stay paused at 0.06!
    g.close_menu();
    g.open_menu(menus::CONSOLE);
    g.tick();
    g.tick();
    assert_eq!(glob(&g, G_MODE1), 1.0, "runWhen 1 counts down and fires even in paused menus");
    assert_eq!(glob(&g, G_MODE0), 0.0, "runWhen 0 pauses in MenuMode");
    assert_eq!(glob(&g, G_MODE2), 0.0, "runWhen 2 pauses in Start/Console menus");

    // 2 ticks in InventoryMenu (1002): runWhen 2 counts down (0.06 -> 0.02), still not fired;
    // runWhen 0 still paused at 0.06.
    g.close_menu();
    g.open_menu(menus::INVENTORY);
    g.tick();
    g.tick();
    assert_eq!((glob(&g, G_MODE0), glob(&g, G_MODE2)), (0.0, 0.0));

    // 3rd tick in InventoryMenu (1002): runWhen 2 fires in MenuMode! runWhen 0 still paused!
    g.tick();
    assert_eq!(glob(&g, G_MODE2), 1.0, "runWhen 2 counts down and fires in non-pause MenuMode");
    assert_eq!(glob(&g, G_MODE0), 0.0, "runWhen 0 must not count down in MenuMode");

    // Back to GameMode: runWhen 0 still has all 3 ticks (0.06s) left.
    g.close_menu();
    g.tick();
    g.tick();
    assert_eq!(glob(&g, G_MODE0), 0.0, "runWhen 0 did not elapse while paused in MenuMode");
    g.tick();
    assert_eq!(glob(&g, G_MODE0), 1.0, "runWhen 0 fires after 3 full GameMode ticks");
}

#[test]
fn call_while_flags_25_runs_in_barter_pauses_in_start_and_console_and_passes_args() {
    let f = fixture();
    let mut g = new_game(&f);

    // Mirror BarterCategories' exact OnKeyDown:203 -> CallWhile ... 25 (1 | 8 | 16) flow.
    run(&mut g, r#"SetEventHandler "OnKeyDown:203" StartWhileOnKey"#);
    g.open_menu(1053); // BarterMenu
    g.input.held.insert(203);
    g.tick();
    // Same tick: jip_keys queues CallWhile, then run_delayed runs it once with args (1053, 1008).
    assert_eq!(glob(&g, G_WHILE_CALLS), 1.0);
    assert_eq!(glob(&g, G_WHILE_ARG_SUM), 2061.0);

    // Remove the key-down registrar now so re-pressing key 203 later cannot queue a
    // second CallWhile and mask whether the original one survived pause/GameMode.
    run(&mut g, r#"RemoveEventHandler "OnKeyDown:203" StartWhileOnKey"#);

    // Another tick in BarterMenu while key 203 is held -> runs again.
    g.tick();
    assert_eq!(glob(&g, G_WHILE_CALLS), 2.0);

    // Switch to StartMenu (1013) and release key 203 while paused: DontRunWhilePaused (16)
    // must keep the CallWhile alive without evaluating or dropping it.
    g.close_menu();
    g.open_menu(menus::START);
    g.input.held.remove(&203);
    g.tick();
    g.tick();
    assert_eq!(glob(&g, G_WHILE_CALLS), 2.0, "paused in StartMenu");

    // Also paused in Console (3).
    g.close_menu();
    g.open_menu(menus::CONSOLE);
    g.tick();
    assert_eq!(glob(&g, G_WHILE_CALLS), 2.0, "paused in Console");

    // In GameMode (no menu): DontRunInGameMode (8) holds it without evaluating/dropping.
    g.close_menu();
    g.tick();
    assert_eq!(glob(&g, G_WHILE_CALLS), 2.0, "held in GameMode by DontRunInGameMode");

    // Re-hold key 203 and re-open ContainerMenu (1008): the original CallWhile resumes!
    g.input.held.insert(203);
    g.open_menu(1008);
    g.tick();
    assert_eq!(glob(&g, G_WHILE_CALLS), 3.0, "original CallWhile survived pause and resumed");

    // Release key 203 while unpaused in ContainerMenu (1008): condition evaluates false and drops.
    g.input.held.remove(&203);
    g.tick();
    assert_eq!(glob(&g, G_WHILE_CALLS), 3.0);

    // Re-holding key 203 now does NOT re-run dropped CallWhile.
    g.input.held.insert(203);
    g.tick();
    assert_eq!(glob(&g, G_WHILE_CALLS), 3.0, "dropped cleanly once condition was false");
}

#[test]
fn call_after_frames_call_for_seconds_and_call_while_per_seconds_respect_pause() {
    let f = fixture();
    let mut g = new_game(&f);

    run(
        &mut g,
        r#"
        CallAfterFrames 2 HitFrames2 2
        CallAfterFrames 0 HitFrames0 1
        CallForSeconds 0.06 HitForSecs 2
        CallWhilePerSeconds 0.04 HitPerSecs ({} => 1) 17 5
        "#,
    );

    // Pause in StartMenu (1013) for 3 ticks:
    // - CallAfterFrames 0 runWhen 1 fires on tick 1
    // - CallAfterFrames 2 runWhen 2, CallForSeconds 0.06 runWhen 2, CallWhilePerSeconds flags 17 all pause!
    g.open_menu(menus::START);
    g.tick();
    assert_eq!(glob(&g, G_FRAMES0), 1.0, "CallAfterFrames 0 runWhen 1 runs next frame");
    g.tick();
    g.tick();
    assert_eq!(glob(&g, G_FRAMES2), 0.0, "CallAfterFrames 2 runWhen 2 paused in StartMenu");
    assert_eq!(glob(&g, G_FOR_SECS), 0.0, "CallForSeconds runWhen 2 paused in StartMenu");
    assert_eq!(glob(&g, G_PER_SECS), 0.0, "CallWhilePerSeconds flags 17 paused in StartMenu");

    // Open BarterMenu (1053): unpaused MenuMode!
    g.close_menu();
    g.open_menu(1053);
    // Tick 1: AfterFrames 2->1 (no), ForSecs 0.06->0.04 (runs: 1), PerSecs 0.04->0.02 (no)
    g.tick();
    assert_eq!(glob(&g, G_FRAMES2), 0.0);
    assert_eq!(glob(&g, G_FOR_SECS), 1.0);
    assert_eq!(glob(&g, G_PER_SECS), 0.0);

    // Tick 2: AfterFrames 1->0 (runs: 1), ForSecs 0.04->0.02 (runs: 2), PerSecs 0.02->0.00 (runs: +5)
    g.tick();
    assert_eq!(glob(&g, G_FRAMES2), 1.0);
    assert_eq!(glob(&g, G_FOR_SECS), 2.0);
    assert_eq!(glob(&g, G_PER_SECS), 5.0);

    // Tick 3: AfterFrames done, ForSecs 0.02->0.00 (runs 3rd/final tick and drops), PerSecs 0.04->0.02 (no)
    g.tick();
    assert_eq!(glob(&g, G_FRAMES2), 1.0);
    assert_eq!(glob(&g, G_FOR_SECS), 3.0, "CallForSeconds 0.06 at dt=0.02 runs all 3 ticks");
    assert_eq!(glob(&g, G_PER_SECS), 5.0);

    // Tick 4: ForSecs stays at 3, PerSecs 0.02->0.00 (runs again: +5 -> 10)
    g.tick();
    assert_eq!(glob(&g, G_FOR_SECS), 3.0);
    assert_eq!(glob(&g, G_PER_SECS), 10.0);

    // Short positive duration (<= frame_seconds) runs once and drops; 0.0 runs zero times.
    run(
        &mut g,
        r#"
        CallForSeconds 0.01 HitForSecs 2
        CallForSeconds 0.0 HitMode0 2
        "#,
    );
    g.tick();
    assert_eq!(glob(&g, G_FOR_SECS), 4.0, "CallForSeconds 0.01 at dt=0.02 runs once");
    assert_eq!(glob(&g, G_MODE0), 0.0, "CallForSeconds 0.0 runs zero times");
    g.tick();
    assert_eq!(glob(&g, G_FOR_SECS), 4.0, "CallForSeconds 0.01 dropped after its single run");
}

#[test]
fn stewie_plugin_version_stays_at_960_gate() {
    let f = fixture();
    let mut g = new_game(&f);
    run(&mut g, r#"set StewieVer to GetPluginVersion "lStewieAl's Tweaks""#);
    assert_eq!(glob(&g, G_STEWIE_VER), 960.0);
}
