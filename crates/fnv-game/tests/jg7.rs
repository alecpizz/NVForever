//! JohnnyGuitar NVSE batch 7 (#1007; docs/spec-notes/ext-jg7.md) on a synthetic plugin:
//! the interface, music, save and camera switches, barter hiding, the queued texts and
//! the colour conversions into script variables.

mod common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nvse_jg7::Request;
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const MISC: u32 = 0x900;
const SOUN: u32 = 0x901;
const REPU: u32 = 0x902;
const QUST: u32 = 0x903;

fn edid(sig: &[u8; 4], fid: u32, name: &str) -> Vec<u8> {
    record(sig, 0, fid, &[sub(b"EDID", z(name))])
}

fn plugin_bytes() -> Vec<u8> {
    [
        tes4(),
        top(b"NPC_", &edid(b"NPC_", 0x7, "Player")),
        top(b"MISC", &edid(b"MISC", MISC, "TestThing")),
        top(b"SOUN", &edid(b"SOUN", SOUN, "TestSound")),
        top(b"REPU", &edid(b"REPU", REPU, "TestReputation")),
        top(b"QUST", &edid(b"QUST", QUST, "TestQuest")),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jg7.esm".into()).unwrap()];
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

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

fn s(g: &mut Game, body: &str) -> String {
    match q(g, body) {
        XVal::Str(s) => s,
        v => panic!("{body}: {v:?}"),
    }
}

#[test]
fn switches() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: ToggleCombatMusic / IsCombatMusicEnabled; ToggleLevelUpMenu /
    // IsLevelUpMenuEnabled].
    assert_eq!(n(&mut g, "IsCombatMusicEnabled"), 1.0);
    q(&mut g, "ToggleCombatMusic 0");
    assert_eq!(n(&mut g, "IsCombatMusicEnabled"), 0.0);
    q(&mut g, "ToggleCombatMusic 1");
    assert_eq!(n(&mut g, "IsCombatMusicEnabled"), 1.0);
    assert_eq!(n(&mut g, "IsLevelUpMenuEnabled"), 1.0);
    assert!(g.level_up_menu_enabled());
    g.player.xp = 250.0;
    assert!(fnv_game::experience::level_up_due(&g));
    q(&mut g, "ToggleLevelUpMenu 0");
    assert_eq!(n(&mut g, "IsLevelUpMenuEnabled"), 0.0);
    assert!(!g.level_up_menu_enabled());
    assert!(!fnv_game::experience::level_up_due(&g), "ToggleLevelUpMenu 0 holds automatic level-up");
    q(&mut g, "ToggleLevelUpMenu 1");
    assert!(fnv_game::experience::level_up_due(&g), "ToggleLevelUpMenu 1 releases pending level-up");
    // [documented: Get/SetViewmodelClipDistance, 0 the default; observed:
    // jg_25_yield_timer_and_viewmodel_clip_neg and w4c_b3_05_jg7_bounds_heap store -5.0 verbatim].
    assert_eq!(n(&mut g, "GetViewmodelClipDistance"), 0.0);
    q(&mut g, "SetViewmodelClipDistance 2.5");
    assert_eq!(n(&mut g, "GetViewmodelClipDistance"), 2.5);
    q(&mut g, "SetViewmodelClipDistance -5.0");
    assert_eq!(n(&mut g, "GetViewmodelClipDistance"), -5.0);
    q(&mut g, "SetViewmodelClipDistance 0");
    assert_eq!(n(&mut g, "GetViewmodelClipDistance"), 0.0);
    // [observed: GetNiLODLevel -1 before any SetNiLODLevel, jg7 diagnostics].
    assert_eq!(n(&mut g, "GetNiLODLevel"), -1.0);
    q(&mut g, "SetNiLODLevel 2");
    assert_eq!(n(&mut g, "GetNiLODLevel"), 2.0);
    // [documented: Get/SetYieldTimer].
    assert_eq!(n(&mut g, "GetYieldTimer"), 0.0);
    q(&mut g, "SetYieldTimer 5");
    assert_eq!(n(&mut g, "GetYieldTimer"), 5.0);
    // [documented: ToggleDisableSaves].
    assert!(!g.saves_disabled());
    q(&mut g, "ToggleDisableSaves 1");
    assert!(g.saves_disabled());
    q(&mut g, "ToggleDisableSaves 0");
    assert!(!g.saves_disabled());
}

#[test]
fn interface_settings() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: HideItemBarterEx's flags, kept for the barter menu; observed:
    // IsItemBarterHiddenEx read 0 after every call, jg7.barter and jg7.barter.pistol].
    let thing = fnv_sim::FormId(MISC);
    q(&mut g, "HideItemBarterEx TestThing 1 1");
    assert_eq!(g.barter_hide_flag(thing, None), 1);
    assert_eq!(g.barter_hidden_for(fnv_sim::FormId(0x999)).get(&thing), Some(&1));
    assert_eq!(n(&mut g, "IsItemBarterHiddenEx TestThing"), 0.0);
    q(&mut g, "HideItemBarterEx TestThing 1");
    assert_eq!(g.barter_hide_flag(thing, None), 3);
    // Vendor-specific override takes precedence over the global entry.
    q(&mut g, "HideItemBarterEx TestThing 1 2 TestQuest");
    assert_eq!(g.barter_hide_flag(thing, Some(fnv_sim::FormId(QUST))), 2);
    assert_eq!(g.barter_hidden_for(fnv_sim::FormId(QUST)).get(&thing), Some(&2));
    q(&mut g, "HideItemBarterEx TestThing 0 0 TestQuest");
    q(&mut g, "HideItemBarterEx TestThing 0");
    assert_eq!(g.barter_hide_flag(thing, None), 0);
    assert!(g.barter_hidden_for(fnv_sim::FormId(QUST)).is_empty());
    // [documented: SetCustomReputationChangeIcon gives 1].
    assert_eq!(n(&mut g, "SetCustomReputationChangeIcon TestReputation 2 \"icons\\sad.dds\""), 1.0);
    assert_eq!(n(&mut g, "SetCustomReputationChangeIcon TestReputation 5 \"icons\\sad.dds\""), 0.0);
    q(&mut g, "SetUIUpdateSound TestSound 3");
    assert_eq!(g.ui_update_sound(3), Some(fnv_sim::FormId(SOUN)));
    assert_eq!(g.ui_update_sound(2), None);
    q(&mut g, "PushUIQuestToTop TestQuest");
    assert_eq!(g.top_ui_quest(), Some(fnv_sim::FormId(QUST)));
    q(&mut g, "ToggleNthPipboyLight 1 1");
    assert_eq!(g.other.jg7.pipboy_lights, [false, true, false]);
    // [documented: QueueCinematicText's priority 2 clears the queue].
    q(&mut g, "QueueCinematicText \"A\" \"a\"");
    q(&mut g, "QueueCinematicText \"B\" \"b\" \"\" 1");
    assert_eq!(g.other.jg7.cinematic.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["B", "A"]);
    q(&mut g, "QueueCinematicText \"C\" \"c\" \"\" 2");
    assert_eq!(g.other.jg7.cinematic.len(), 1);
    q(&mut g, "QueueObjectiveText \"Do it\" 1");
    assert!(g.other.jg7.objectives[0].completed);
    q(&mut g, "SetCameraShake 2 3");
    q(&mut g, "StopVATSCam");
    assert_eq!(g.take_jg7_requests(), vec![Request::CameraShake(2.0, 3.0), Request::StopVatsCamera]);
    assert_eq!(n(&mut g, "RefAddr"), 0.0);
    // [documented: SetCustomMapMarkerIcon / GetCustomMapMarkerIcon; empty string resets].
    q(&mut g, "SetCustomMapMarkerIcon PlayerREF \"icons/camp.dds\"");
    assert_eq!(s(&mut g, "SetFunctionValue (PlayerREF.GetCustomMapMarkerIcon)"), "icons/camp.dds");
    assert_eq!(g.custom_map_marker_icon(fnv_sim::FormId::PLAYER_REF), Some("icons/camp.dds"));
    q(&mut g, "SetCustomMapMarkerIcon PlayerREF \"\"");
    assert_eq!(g.custom_map_marker_icon(fnv_sim::FormId::PLAYER_REF), None);
    assert_eq!(s(&mut g, "SetFunctionValue (PlayerREF.GetCustomMapMarkerIcon)"), "");
    // [documented: GetLocationSpecificLoadScreensOnly; IsMenuPaused without Real Time
    // Menus; GetJohnnyPatch 1 always 1].
    assert_eq!(n(&mut g, "GetLocationSpecificLoadScreensOnly"), 0.0);
    q(&mut g, "SetLocationSpecificLoadScreensOnly 1");
    assert_eq!(n(&mut g, "GetLocationSpecificLoadScreensOnly"), 1.0);
    assert_eq!(n(&mut g, "IsMenuPaused"), 0.0);
    assert_eq!(n(&mut g, "IsMenuPaused 1002"), 1.0);
    assert_eq!(n(&mut g, "GetJohnnyPatch 1"), 1.0);
    assert_eq!(n(&mut g, "GetJohnnyPatch 2"), 0.0);
    // [observed: w4c_b3_05_jg7_bounds_heap: GetDefaultHeapSize returns 200 when nvse_config.ini is absent].
    assert_eq!(n(&mut g, "GetDefaultHeapSize"), 200.0);
    // [documented: TogglePipBoy opens on the tab asked for, or closes it].
    q(&mut g, "TogglePipBoy 1003");
    assert_eq!(n(&mut g, "IsMenuPaused"), 1.0);
    assert_eq!(n(&mut g, "MenuMode 1003"), 1.0);
    q(&mut g, "TogglePipBoy");
    assert_eq!(n(&mut g, "MenuMode 1003"), 0.0);
}

#[test]
fn colours_into_variables() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: HSVtoRGB and RGBtoHSV write their script variables].
    let rgb = s(&mut g, "float r\nfloat gg\nfloat b\nHSVtoRGB 0 1 1 r gg b\nSetFunctionValue (sv_Construct \"%g %g %g\" r gg b)");
    assert_eq!(rgb, "255 0 0");
    let rgb = s(&mut g, "float r\nfloat gg\nfloat b\nHSVtoRGB 120 0.5 0.5 r gg b\nSetFunctionValue (sv_Construct \"%g %g %g\" r gg b)");
    assert_eq!(rgb, "64 128 64");
    let hsv = s(&mut g, "float h\nfloat sat\nfloat v\nRGBtoHSV 0 0 255 h sat v\nSetFunctionValue (sv_Construct \"%g %g %g\" h sat v)");
    assert_eq!(hsv, "240 1 1");
    assert_eq!(fnv_game::nvse_jg7::rgb_to_hsv(255.0, 255.0, 255.0), [0.0, 0.0, 1.0]);
    // Observed whole-channel rounding and unclamped RGBtoHSV [observed: w6_t1_07_jg_hsv_rgb].
    assert_eq!(fnv_game::nvse_jg7::hsv_to_rgb(30.0, 0.5, 0.5), [128.0, 96.0, 64.0]);
    let oob = fnv_game::nvse_jg7::rgb_to_hsv(300.0, -10.0, 128.0);
    assert!((oob[0] - 333.290).abs() < 1e-3 && (oob[1] - 1.0333).abs() < 1e-3 && (oob[2] - 1.1765).abs() < 1e-3, "{oob:?}");
}
