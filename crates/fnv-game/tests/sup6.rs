//! SUP NVSE batch 6 (#1008; docs/spec-notes/ext-sup6.md) on a synthetic plugin, called
//! from scripts: the mouse, the cursor's traits, the debug texts, the screen and fonts,
//! the plugin header texts, the HUD flags, the lockpick menu and the throw held, with the
//! engine's part played by the test (it fills `Game::sup` as the engine would).

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nvse_sup6::LockpickView;
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL: u32 = 0x200;

fn plugin_bytes() -> Vec<u8> {
    let npcs = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    let header = record(b"TES4", 0, 0, &[sub(b"HEDR", [1.34f32.to_le_bytes().to_vec(), 1i32.to_le_bytes().to_vec(), 0x800u32.to_le_bytes().to_vec()].concat()), sub(b"CNAM", z("An Author")), sub(b"SNAM", z("What it does"))]);
    [header, top(b"NPC_", &npcs), top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell)))].concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Sup6.esm".into()).unwrap()];
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

const VARS: &str = "float fX\nfloat fY\nstring_var sOut\n";

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("{VARS}Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
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
fn the_mouse() {
    let f = fixture();
    let mut g = game(&f);
    g.sup.mouse.pos = (640.0, 360.0);
    g.sup.mouse.moved = (3.0, -2.0);
    g.sup.mouse.last_moved = (3.0, -2.0);
    g.sup.mouse.wheel = 1.0;
    g.sup.mouse.last_wheel = 1.0;
    // [documented: GetMousePosition, into the variables].
    assert_eq!(s(&mut g, "\"\"\nGetMousePosition fX fY\nlet sOut := sv_Construct \"%g %g\" fX fY"), "640 360");
    // Only in a menu [documented: GetMouseMovement, GetMouseWheelMovement].
    assert_eq!(s(&mut g, "\"\"\nGetMouseMovement fX fY 0\nlet sOut := sv_Construct \"%g %g\" fX fY"), "0 0");
    assert_eq!(n(&mut g, "GetMouseWheelMovement 0"), 0.0);
    g.loading = true;
    assert_eq!(s(&mut g, "\"\"\nGetMouseMovement fX fY 0\nlet sOut := sv_Construct \"%g %g\" fX fY"), "3 -2");
    assert_eq!(n(&mut g, "GetMouseWheelMovement 1"), 1.0);
    g.loading = false;
    // For the engine [documented: FakeMouseMovement]; non-finite deltas are ignored [design].
    q(&mut g, "FakeMouseMovement 1 2\nFakeMouseMovement (IsNumberNAN) 5");
    assert_eq!(g.sup.fake_moves, vec![(1.0, 2.0)]);
}

#[test]
fn the_cursor_traits() {
    let f = fixture();
    let mut g = game(&f);
    // At rest: alpha 255, white, the arrow's size (see docs/spec-notes/ext-sup6.md).
    assert_eq!(n(&mut g, "GetCursorTrait 0"), 255.0);
    assert_eq!(n(&mut g, "GetCursorTrait 1"), 32.0);
    q(&mut g, "SetCursorTrait 0 100");
    assert_eq!(n(&mut g, "GetCursorTrait 0"), 100.0);
    // Out of menus a gradual change leaves the value read as it was [observed: sup6
    // cursor.gradual, cursor.gradual.next].
    q(&mut g, "SetCursorTraitGradual 0 200 50 4");
    assert_eq!(n(&mut g, "GetCursorTrait 0"), 100.0);
    g.time += 1.0;
    assert_eq!(n(&mut g, "GetCursorTrait 0"), 100.0);
    q(&mut g, "SetCursorTraitGradual 0");
    assert_eq!(n(&mut g, "GetCursorTrait 0"), 100.0);
    // In a menu [documented: SetCursorTraitGradual 0 200 50 4]: 200 at once, 50 after 4
    // seconds.
    g.loading = true;
    q(&mut g, "SetCursorTraitGradual 0 200 50 4");
    assert_eq!(n(&mut g, "GetCursorTrait 0"), 200.0);
    g.time += 2.0;
    assert_eq!(n(&mut g, "GetCursorTrait 0"), 125.0);
    // Stopped where it is [documented].
    q(&mut g, "SetCursorTraitGradual 0");
    g.time += 5.0;
    assert_eq!(n(&mut g, "GetCursorTrait 0"), 125.0);
    g.loading = false;
}

#[test]
fn the_debug_texts() {
    let f = fixture();
    let mut g = game(&f);
    // Fresh create returns 1; duplicate name updates x, y, and text in place (preserving
    // zoom, matching the documented page) while returning -1 [observed:
    // w4c_b4_03_sup6_bobbypin_debugtext: dt(1,-1)].
    assert_eq!(n(&mut g, "DebugTextCreate \"MyDebugText1\" 100 100 \"My Text\""), 1.0);
    q(&mut g, "DebugTextSetZoom \"MyDebugText1\" 24");
    assert_eq!(n(&mut g, "DebugTextCreate \"MyDebugText1\" 200 250 \"Updated\""), -1.0);
    let updated = g.sup.debug_texts["MyDebugText1"].clone();
    assert_eq!((updated.x, updated.y, updated.text.as_str(), updated.zoom), (200.0, 250.0, "Updated", 24.0));
    assert_eq!(n(&mut g, "DebugTextExists \"MyDebugText1\""), 1.0);
    q(&mut g, "DebugTextSetPos \"MyDebugText1\" 600 700\nDebugTextSetString \"MyDebugText1\" \"My New Text\"\nDebugTextSetZoom \"MyDebugText1\" 20");
    let t = g.sup.debug_texts["MyDebugText1"].clone();
    assert_eq!((t.x, t.y, t.text.as_str(), t.zoom), (600.0, 700.0, "My New Text", 20.0));
    q(&mut g, "DebugTextDestroy \"MyDebugText1\"");
    assert_eq!(n(&mut g, "DebugTextExists \"MyDebugText1\""), 0.0);
    assert_eq!(n(&mut g, "DebugTextCreate \"MyDebugText1\" 50 60 \"Recreated\""), 1.0);
}

#[test]
fn the_screen_fonts_plugins_and_the_hud() {
    let f = fixture();
    let mut g = game(&f);
    // No window: 0 [decision].
    assert_eq!(n(&mut g, "GetScreenTrait 0"), 0.0);
    g.sup.window = (1280.0, 720.0);
    // [observed: sup6 screen, 1706.67 960 1280 720 1.33333].
    let r = |x: f64| (x * 100.0).round() / 100.0;
    assert_eq!(r(n(&mut g, "GetScreenTrait 0")), 1706.67);
    assert_eq!(n(&mut g, "GetScreenTrait 1"), 960.0);
    assert_eq!(n(&mut g, "GetScreenTrait 2"), 1280.0);
    assert_eq!(n(&mut g, "GetScreenTrait 3"), 720.0);
    assert_eq!(r(n(&mut g, "GetScreenTrait 4")), 1.33);
    // Both traits the file's line height; font 9 0 [observed: sup6 fonts].
    g.sup.fonts = vec![31.0, 26.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0];
    assert_eq!(n(&mut g, "GetFontTrait 1 0"), 31.0);
    assert_eq!(n(&mut g, "GetFontTrait 1 1"), 31.0);
    assert_eq!(n(&mut g, "GetFontTrait 2 1"), 26.0);
    assert_eq!(n(&mut g, "GetFontTrait 9 0"), 0.0);
    assert_eq!(fnv_game::nvse_sup6::font_line_height(&31f32.to_le_bytes()), 31.0);
    // TES4 CNAM and SNAM [documented: GetModTraitSTR].
    assert_eq!(s(&mut g, "GetModTraitSTR 0 0"), "An Author");
    assert_eq!(s(&mut g, "GetModTraitSTR 0 1"), "What it does");
    assert_eq!(s(&mut g, "GetModTraitSTR 5 0"), "");
    // The same flags as SetHUDVisibilityOverride [documented].
    q(&mut g, "SetHUDVisibilityFlags 512");
    assert_eq!(n(&mut g, "GetHUDVisibilityOverride"), 512.0);
    assert_eq!(n(&mut g, "GetHUDVisibilityFlags"), 512.0);
    // [decision] the machine's state.
    assert_eq!(n(&mut g, "GetPCTrait 0"), 0.0);
}

#[test]
fn the_lockpick_menu_and_the_throw() {
    let f = fixture();
    let mut g = game(&f);
    // Shut: -1 for all requests including out-of-range 3 [documented: GetLockPickMenuValue;
    // observed: sup_20_lockpick_and_bobbypin_closed].
    assert_eq!(n(&mut g, "GetLockPickMenuValue 0"), -1.0);
    assert_eq!(n(&mut g, "GetLockPickMenuValue 3"), -1.0);
    // Request 1 returns the lock tier enum (0 Very Easy..4 Very Hard), not raw 0/25/50/75/100;
    // out-of-range request 3 returns 0.0 while open [observed: w4h_11_sup_lockpick_menu_tiers
    // and w4e_12_sup_lockpick_menu_value_tiers, #3802].
    for (raw_level, expected_tier) in [(0.0, 0.0), (1.0, 1.0), (25.0, 1.0), (50.0, 2.0), (75.0, 3.0), (100.0, 4.0)] {
        g.sup.lockpick = Some(LockpickView { angle: 90.0, level: raw_level, skill: 40.0, health: 1.0 });
        assert_eq!(n(&mut g, "GetLockPickMenuValue 1"), expected_tier, "raw lock level {raw_level}");
    }
    g.sup.lockpick = Some(LockpickView { angle: 90.0, level: 25.0, skill: 40.0, health: 1.0 });
    assert_eq!(n(&mut g, "GetLockPickMenuValue 0"), 90.0);
    assert_eq!(n(&mut g, "GetLockPickMenuValue 1"), 1.0);
    assert_eq!(n(&mut g, "GetLockPickMenuValue 2"), 40.0);
    assert_eq!(n(&mut g, "GetLockPickMenuValue 3"), 0.0);
    // [documented: SetBobbyPinHealth, without a value the health; above 1 allowed].
    assert_eq!(n(&mut g, "SetBobbyPinHealth"), 1.0);
    q(&mut g, "SetBobbyPinHealth 2");
    assert_eq!(n(&mut g, "SetBobbyPinHealth"), 2.0);
    assert_eq!(g.sup.pin_health, Some(2.0));
    g.sup.grenade_held = 1.25;
    assert_eq!(n(&mut g, "GetGrenadeTimeHeld"), 1.25);
}
