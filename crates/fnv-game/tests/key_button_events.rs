//! Named `SetEventHandler` key (`OnKeyDown` / `OnKeyUp`) and gamepad button
//! (`OnButtonDown` / `OnButtonUp`) edge dispatch, including colon scancode / bitmask
//! filters, plain handlers, priority ordering, disabled-input delivery, late
//! registration history, and `page_tick` menu context (ext-events.md).
use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{game::RunCtx, Game, GameConfig, GameData};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

const KEY_COUNT: u32 = 0xA00;
const KEY_LAST: u32 = 0xA01;
const PLAIN_COUNT: u32 = 0xA02;
const FIRST_COUNT: u32 = 0xA03;
const UP_COUNT: u32 = 0xA04;
const JIP_COUNT: u32 = 0xA05;
const BTN_COUNT: u32 = 0xA06;
const BTN_LAST: u32 = 0xA07;
const BTN_MASK_COUNT: u32 = 0xA08;
const BTN_MASK_SUM: u32 = 0xA09;
const BTN_UP_COUNT: u32 = 0xA0A;
const BTN_ZERO_COUNT: u32 = 0xA0B;
const ORDER_LOG: u32 = 0xA0C;
const EVENT_MATCH: u32 = 0xA0D;
const STEWIE_VER: u32 = 0xA0E;

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
    // Include an unused `string_var` local so `Auto` source routing compiles SCTX
    // instead of executing the placeholder empty SCDA bytecode (see jip_remove_filters.rs).
    let src = src.replacen("\nBegin Function", "\nstring_var sourceMarker\nBegin Function", 1);
    let mut fields = vec![sub(b"EDID", z(edid))];
    fields.extend(unit(0, &script(&[(0, None, Vec::new())]), &[], &[]));
    fields.push(sub(b"SCTX", src.into_bytes()));
    record(b"SCPT", 0, id, &fields)
}

fn fixture() -> Fixture {
    let globals = [
        global_rec(KEY_COUNT, "KeyCount"),
        global_rec(KEY_LAST, "KeyLast"),
        global_rec(PLAIN_COUNT, "PlainCount"),
        global_rec(FIRST_COUNT, "FirstCount"),
        global_rec(UP_COUNT, "UpCount"),
        global_rec(JIP_COUNT, "JipCount"),
        global_rec(BTN_COUNT, "BtnCount"),
        global_rec(BTN_LAST, "BtnLast"),
        global_rec(BTN_MASK_COUNT, "BtnMaskCount"),
        global_rec(BTN_MASK_SUM, "BtnMaskSum"),
        global_rec(BTN_UP_COUNT, "BtnUpCount"),
        global_rec(BTN_ZERO_COUNT, "BtnZeroCount"),
        global_rec(ORDER_LOG, "OrderLog"),
        global_rec(EVENT_MATCH, "EventMatch"),
        global_rec(STEWIE_VER, "StewieVer"),
    ]
    .concat();

    let scripts = [
        udf_rec(
            0xB00,
            "OnKeyColonUDF",
            "scn OnKeyColonUDF\nint iKey\nBegin Function {iKey}\n  set KeyCount to KeyCount + 1\n  set KeyLast to iKey\n  if eval (GetCurrentEventName == \"OnKeyDown:203\")\n    set EventMatch to EventMatch + 1\n  endif\nEnd\n",
        ),
        udf_rec(
            0xB01,
            "OnKeyPlainUDF",
            "scn OnKeyPlainUDF\nint iKey\nBegin Function {iKey}\n  set PlainCount to PlainCount + 1\n  if eval (GetCurrentEventName == \"OnKeyDown\")\n    set EventMatch to EventMatch + 10\n  endif\nEnd\n",
        ),
        udf_rec(
            0xB02,
            "OnKeyFirstUDF",
            "scn OnKeyFirstUDF\nint iKey\nBegin Function {iKey}\n  set FirstCount to FirstCount + 1\nEnd\n",
        ),
        udf_rec(
            0xB03,
            "OnKeyUpUDF",
            "scn OnKeyUpUDF\nint iKey\nBegin Function {iKey}\n  set UpCount to UpCount + 1\n  set KeyLast to iKey\nEnd\n",
        ),
        udf_rec(
            0xB04,
            "OnKeyJipUDF",
            "scn OnKeyJipUDF\nint iKey\nBegin Function {iKey}\n  set JipCount to JipCount + 1\nEnd\n",
        ),
        udf_rec(
            0xB05,
            "OnBtnSingleUDF",
            "scn OnBtnSingleUDF\nint iBtn\nBegin Function {iBtn}\n  set BtnCount to BtnCount + 1\n  set BtnLast to iBtn\n  set OrderLog to (OrderLog * 10) + 1\n  if eval (GetCurrentEventName == \"OnButtonDown:4096\")\n    set EventMatch to EventMatch + 100\n  endif\nEnd\n",
        ),
        udf_rec(
            0xB06,
            "OnBtnMaskUDF",
            "scn OnBtnMaskUDF\nint iBtn\nBegin Function {iBtn}\n  set BtnMaskCount to BtnMaskCount + 1\n  set BtnMaskSum to BtnMaskSum + iBtn\n  set OrderLog to (OrderLog * 10) + 2\n  if eval (GetCurrentEventName == \"OnButtonDown:12288\")\n    set EventMatch to EventMatch + 1000\n  endif\nEnd\n",
        ),
        udf_rec(
            0xB07,
            "OnBtnUpUDF",
            "scn OnBtnUpUDF\nint iBtn\nBegin Function {iBtn}\n  set BtnUpCount to BtnUpCount + 1\n  set BtnLast to iBtn\nEnd\n",
        ),
        udf_rec(
            0xB08,
            "OnBtnZeroUDF",
            "scn OnBtnZeroUDF\nint iBtn\nBegin Function {iBtn}\n  set BtnZeroCount to BtnZeroCount + 1\nEnd\n",
        ),
        udf_rec(
            0xB09,
            "OnPageKeyUDF",
            "scn OnPageKeyUDF\nint iKey\nBegin Function {iKey}\n  if MenuMode 1013\n    set KeyCount to KeyCount + 100\n  endif\nEnd\n",
        ),
    ]
    .concat();

    let bytes = [tes4(), top(b"GLOB", &globals), top(b"SCPT", &scripts)].concat();
    let plugins = [Plugin::parse(&bytes, "KeyButtonEvents.esm".into()).unwrap()];
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
fn on_key_down_and_up_colon_plain_first_and_remove() {
    let f = fixture();
    let mut g = new_game(&f);

    // Arrow Left (203), Arrow Right (205), A (30), D (32): the exact navigation keys
    // used by category-cycling scripts, plus plain and "first"-filtered listeners.
    run(
        &mut g,
        r#"
        SetEventHandler "OnKeyDown:203" OnKeyColonUDF
        SetEventHandler "OnKeyDown:30" OnKeyColonUDF
        SetEventHandler "OnKeyDown" OnKeyPlainUDF
        SetEventHandler "OnKeyDown" OnKeyFirstUDF "first"::205
        SetEventHandler "OnKeyUp:203" OnKeyUpUDF
        "#,
    );

    // Press Left Arrow (203): fires colon handler and plain handler once; not 205 or up.
    g.input.held.insert(203);
    g.tick();
    assert_eq!(glob(&g, KEY_COUNT), 1.0);
    assert_eq!(glob(&g, KEY_LAST), 203.0);
    assert_eq!(glob(&g, PLAIN_COUNT), 1.0);
    assert_eq!(glob(&g, FIRST_COUNT), 0.0);
    assert_eq!(glob(&g, UP_COUNT), 0.0);
    assert_eq!(glob(&g, EVENT_MATCH), 11.0, "GetCurrentEventName reports OnKeyDown and OnKeyDown:203");

    // Holding 203 across another frame does not re-fire.
    g.tick();
    assert_eq!(glob(&g, KEY_COUNT), 1.0);
    assert_eq!(glob(&g, PLAIN_COUNT), 1.0);

    // Pressing Right Arrow (205) while 203 stays held fires plain and first::205 only.
    g.input.held.insert(205);
    g.tick();
    assert_eq!(glob(&g, KEY_COUNT), 1.0);
    assert_eq!(glob(&g, PLAIN_COUNT), 2.0);
    assert_eq!(glob(&g, FIRST_COUNT), 1.0);

    // Releasing 203 fires OnKeyUp:203 with scancode 203.
    g.input.held.remove(&203);
    g.tick();
    assert_eq!(glob(&g, UP_COUNT), 1.0);
    assert_eq!(glob(&g, KEY_LAST), 203.0);

    // Pressing A (30) fires OnKeyDown:30.
    g.input.held.insert(30);
    g.tick();
    assert_eq!(glob(&g, KEY_COUNT), 2.0);
    assert_eq!(glob(&g, KEY_LAST), 30.0);

    // RemoveEventHandler stops OnKeyDown:203 while keeping OnKeyDown:30 intact.
    run(&mut g, r#"RemoveEventHandler "OnKeyDown:203" OnKeyColonUDF"#);
    g.input.held.insert(203);
    g.tick();
    assert_eq!(glob(&g, KEY_COUNT), 2.0, "removed OnKeyDown:203 must not fire");
    assert_eq!(glob(&g, PLAIN_COUNT), 4.0, "unfiltered OnKeyDown still fires");
}

#[test]
fn on_key_down_coexists_with_jip_disabled_keys_late_registration_and_page_tick() {
    let f = fixture();
    let mut g = new_game(&f);

    // Press key 30 before any handler is registered; edge history must advance.
    g.input.held.insert(30);
    g.tick();

    run(
        &mut g,
        r#"
        SetOnKeyDownEventHandler OnKeyJipUDF 1 30
        SetEventHandler "OnKeyDown:30" OnKeyColonUDF
        "#,
    );
    // Still held -> neither handler fires on the next tick.
    g.tick();
    assert_eq!(glob(&g, JIP_COUNT), 0.0);
    assert_eq!(glob(&g, KEY_COUNT), 0.0);

    // Release, disable key 30, then press again: both JIP and named handlers still fire.
    g.input.held.remove(&30);
    g.tick();
    run(&mut g, "DisableKey 30");
    g.input.held.insert(30);
    g.tick();
    assert_eq!(glob(&g, JIP_COUNT), 1.0);
    assert_eq!(glob(&g, KEY_COUNT), 1.0);

    // Held StartMenu page_tick(1013) delivers edges in MenuMode 1013 and does not replay on tick().
    run(&mut g, r#"SetEventHandler "OnKeyDown:32" OnPageKeyUDF"#);
    g.input.held.insert(32);
    g.page_tick(1013);
    assert_eq!(glob(&g, KEY_COUNT), 101.0);
    g.tick();
    assert_eq!(glob(&g, KEY_COUNT), 101.0, "edge consumed during page_tick must not replay");
}

#[test]
fn on_button_down_and_up_single_bitmask_plain_priority_disabled_and_remove() {
    let f = fixture();
    let mut g = new_game(&f);

    // Hold DPad Left (4) before any handler exists to verify late registration does not replay.
    g.jip2.told.pad.buttons = 4;
    g.tick();

    run(
        &mut g,
        r#"
        SetEventHandler "OnButtonDown:4" OnBtnSingleUDF
        SetEventHandler "OnButtonDown:4096" OnBtnSingleUDF "priority"::1
        SetEventHandler "OnButtonDown:12288" OnBtnMaskUDF "priority"::5
        SetEventHandler "OnButtonDown" OnKeyPlainUDF
        SetEventHandler "OnButtonDown" OnKeyFirstUDF "first"::8
        SetEventHandler "OnButtonUp:4" OnBtnUpUDF
        SetEventHandler "OnButtonUp:12288" OnBtnUpUDF
        SetEventHandler "OnButtonDown:0" OnBtnZeroUDF
        "#,
    );

    // Button 4 was already held -> no initial fire.
    g.tick();
    assert_eq!(glob(&g, BTN_COUNT), 0.0);
    assert_eq!(glob(&g, PLAIN_COUNT), 0.0);

    // Release 4 -> OnButtonUp:4 fires with 4.
    g.jip2.told.pad.buttons = 0;
    g.tick();
    assert_eq!(glob(&g, BTN_UP_COUNT), 1.0);
    assert_eq!(glob(&g, BTN_LAST), 4.0);

    // Press A (4096) and B (8192) together:
    // - 4096 runs OnBtnMaskUDF (priority 5 -> digit 2) before OnBtnSingleUDF (priority 1 -> digit 1)
    // - 8192 runs OnBtnMaskUDF (digit 2)
    // - plain OnButtonDown runs once per changed button (2 times)
    // - OnButtonDown:0 never fires
    g.jip2.told.pad.buttons = 4096 | 8192;
    g.tick();
    assert_eq!(glob(&g, BTN_COUNT), 1.0);
    assert_eq!(glob(&g, BTN_MASK_COUNT), 2.0);
    assert_eq!(glob(&g, BTN_MASK_SUM), 12288.0);
    assert_eq!(glob(&g, PLAIN_COUNT), 2.0);
    assert_eq!(glob(&g, FIRST_COUNT), 0.0);
    assert_eq!(glob(&g, BTN_ZERO_COUNT), 0.0);
    assert_eq!(glob(&g, ORDER_LOG), 212.0, "priority 5 bitmask handler runs before priority 1 single-button handler");
    assert_eq!(
        glob(&g, EVENT_MATCH),
        2100.0,
        "GetCurrentEventName reports OnButtonDown:12288 twice and OnButtonDown:4096 once"
    );

    // Holding A+B across another tick does not re-fire.
    g.tick();
    assert_eq!(glob(&g, BTN_MASK_COUNT), 2.0);

    // Release A (4096) while B (8192) stays held -> OnButtonUp:12288 fires once with 4096.
    g.jip2.told.pad.buttons = 8192;
    g.tick();
    assert_eq!(glob(&g, BTN_UP_COUNT), 2.0);
    assert_eq!(glob(&g, BTN_LAST), 4096.0);

    // Script-injected HoldButton + DisableButton on DPad Right (8) fires OnButtonDown with 8.
    run(&mut g, "DisableButton 8\nHoldButton 8");
    g.tick();
    assert_eq!(glob(&g, FIRST_COUNT), 1.0);
    assert_eq!(glob(&g, PLAIN_COUNT), 3.0);

    // RemoveEventHandler removes the bitmask handler cleanly.
    run(&mut g, r#"RemoveEventHandler "OnButtonDown:12288" OnBtnMaskUDF"#);
    g.jip2.told.pad.buttons = 4096 | 8192;
    g.tick();
    assert_eq!(glob(&g, BTN_COUNT), 2.0, "OnButtonDown:4096 still fires");
    assert_eq!(glob(&g, BTN_MASK_COUNT), 2.0, "removed OnButtonDown:12288 must not fire");
}

#[test]
fn stewie_plugin_version_stays_at_960_gate() {
    let f = fixture();
    let mut g = new_game(&f);
    run(&mut g, r#"set StewieVer to GetPluginVersion "lStewieAl's Tweaks""#);
    assert_eq!(glob(&g, STEWIE_VER), 960.0);
}
