//! Source registration and real dispatcher controls over authored records.
//! These do not supply an automatic animation-event producer.
mod common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::trace::EventKind;
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use fnv_ui::MemSource;

const CELL: u32 = 0x900;
const ACTOR_A: FormId = FormId(0x901);
const ACTOR_B: FormId = FormId(0x902);

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let npc = record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("OwnedRoom")), sub(b"DATA", vec![1])]);
    let actor = |id, name| record(b"ACHR", 0, id, &[sub(b"EDID", z(name)), sub(b"NAME", u32s(&[7])), sub(b"DATA", vec![0; 24])]);
    let refs = [actor(ACTOR_A.0, "OwnedActorA"), actor(ACTOR_B.0, "OwnedActorB")].concat();
    let cell = [cell, group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs))].concat();
    let list = record(b"FLST", 0, 0x910, &[sub(b"EDID", z("OwnedEmptyList"))]);
    let marker = record(b"STAT", 0, 0x3b, &[sub(b"EDID", z("OwnedXMarker"))]);
    let bytes = [tes4(), top(b"NPC_", &npc), top(b"FLST", &list), top(b"STAT", &marker), top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell)))].concat();
    let plugins = [Plugin::parse(&bytes, "OwnedAnimationCodes.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let catalogue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &catalogue, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    let mut game = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() });
    game.player_entered(Some(FormId(CELL)));
    let mut xml = String::from("<menu name=\"HUDMainMenu\"><rect name=\"Probe\">");
    for key in ["zero", "neighbor", "null", "marker", "key"] {
        for field in ["calls", "value", "a", "b"] {
            xml.push_str(&format!("<_{key}_{field}>0</_{key}_{field}>"));
        }
    }
    xml.push_str("</rect></menu>");
    let ui = MemSource::default().with("owned.xml", &xml);
    game.ui_tree.load_menu(&ui, "owned.xml").unwrap();
    game.ui_tree.update();
    game
}

fn register(game: &mut Game, event: &str, key: &str, actor: &str, code: i32) {
    let source = format!(
        "Begin Function {{ }}\nSetFunctionValue (SetOn{event}EventHandler (Begin Function {{int code}}\nSetUIFloat \"HUDMainMenu/Probe/_{key}_calls\" ((GetUIFloat \"HUDMainMenu/Probe/_{key}_calls\") + 1)\nSetUIFloat \"HUDMainMenu/Probe/_{key}_value\" code\nSetUIFloat \"HUDMainMenu/Probe/_{key}_a\" (GetSelf == OwnedActorA)\nSetUIFloat \"HUDMainMenu/Probe/_{key}_b\" (GetSelf == OwnedActorB)\nEnd) 1 {actor} {code})\nEnd\n"
    );
    assert_eq!(game.run_query(&source, RunCtx::default()).unwrap_or_else(|error| panic!("{source}: {error}")), XVal::Num(1.0), "source registration must be accepted");
}

fn read(game: &Game, key: &str, field: &str) -> f64 {
    game.ui_tree.get_float(&format!("HUDMainMenu/Probe/_{key}_{field}")).unwrap()
}

fn fire(game: &mut Game, event: &str, actor: FormId, code: i32) -> usize {
    game.jip_event(event, Some(actor), vec![XVal::Num(f64::from(code))])
}

fn assert_clean(game: &Game) {
    assert!(!game.events.iter().any(|event| matches!(event.kind, EventKind::CallFailed { .. } | EventKind::ScriptError { .. })), "callbacks must not hide script failures");
}

#[test]
fn action_zero_remains_specific_with_an_empty_actor_list() {
    let f = fixture(); let mut game = game(&f);
    register(&mut game, "AnimAction", "zero", "OwnedEmptyList", 0);
    register(&mut game, "AnimAction", "neighbor", "OwnedEmptyList", 9);
    // Start with a nonzero event so a zero-as-wildcard defect is observed directly.
    assert_eq!(fire(&mut game, "animaction", ACTOR_A, 9), 1);
    assert_eq!((read(&game, "zero", "calls"), read(&game, "neighbor", "calls")), (0.0, 1.0));
    assert_eq!(fire(&mut game, "animaction", ACTOR_B, 0), 1);
    assert_eq!((read(&game, "zero", "value"), read(&game, "zero", "a"), read(&game, "zero", "b")), (0.0, 0.0, 1.0));
    assert_eq!(fire(&mut game, "animaction", ACTOR_A, 0), 1);
    assert_eq!((read(&game, "zero", "calls"), read(&game, "neighbor", "calls"), read(&game, "zero", "a")), (2.0, 1.0, 1.0));
    assert_eq!(fire(&mut game, "animaction", ACTOR_B, 9), 1);
    assert_eq!((read(&game, "neighbor", "value"), read(&game, "neighbor", "b")), (9.0, 1.0));
    assert_clean(&game);
}

#[test]
fn action_zero_keeps_an_actor_specific_filter_and_valid_neighbor() {
    let f = fixture(); let mut game = game(&f);
    register(&mut game, "AnimAction", "zero", "OwnedActorA", 0);
    register(&mut game, "AnimAction", "neighbor", "OwnedEmptyList", 9);
    assert_eq!(fire(&mut game, "animaction", ACTOR_A, 0), 1);
    assert_eq!(fire(&mut game, "animaction", ACTOR_B, 0), 0);
    assert_eq!(fire(&mut game, "animaction", ACTOR_A, 9), 1);
    assert_eq!(fire(&mut game, "animaction", ACTOR_B, 9), 1);
    assert_eq!((read(&game, "zero", "calls"), read(&game, "zero", "a"), read(&game, "neighbor", "calls")), (1.0, 1.0, 2.0));
    assert_clean(&game);
}

#[test]
fn numeric_playgroup_zero_is_literal_and_preserves_callback_code_and_receiver() {
    let f = fixture(); let mut game = game(&f);
    register(&mut game, "PlayGroup", "zero", "OwnedEmptyList", 0);
    register(&mut game, "PlayGroup", "neighbor", "OwnedActorA", 24);
    assert_eq!(fire(&mut game, "playgroup", ACTOR_A, 24), 1);
    assert_eq!(fire(&mut game, "playgroup", ACTOR_B, 24), 0);
    assert_eq!(fire(&mut game, "playgroup", ACTOR_B, 0), 1);
    assert_eq!((read(&game, "zero", "calls"), read(&game, "zero", "value"), read(&game, "zero", "b")), (1.0, 0.0, 1.0));
    assert_eq!((read(&game, "neighbor", "calls"), read(&game, "neighbor", "value"), read(&game, "neighbor", "a")), (1.0, 24.0, 1.0));
    assert_eq!(fire(&mut game, "playgroup", ACTOR_A, 0), 1);
    assert_clean(&game);
}

#[test]
fn actor_zero_stays_wildcard_while_xmarker_does_not_wildcard_animaction() {
    let f = fixture(); let mut game = game(&f);
    register(&mut game, "AnimAction", "null", "0", 9);
    register(&mut game, "AnimAction", "marker", "OwnedXMarker", 9);
    assert_eq!(fire(&mut game, "animaction", ACTOR_A, 0), 0);
    assert_eq!(fire(&mut game, "animaction", ACTOR_B, 9), 1);
    assert_eq!((read(&game, "null", "calls"), read(&game, "null", "b"), read(&game, "marker", "calls"), read(&game, "marker", "b")), (1.0, 1.0, 0.0, 0.0));
    assert_clean(&game);
}

#[test]
fn unrelated_scalar_slots_retain_their_existing_zero_filter_policy() {
    let f = fixture(); let mut game = game(&f);
    // This is a compatibility boundary, not a native-parity assertion for key zero.
    let source = "Begin Function {}\nSetFunctionValue (SetOnKeyDownEventHandler (Begin Function {int key}\nSetUIFloat \"HUDMainMenu/Probe/_key_calls\" ((GetUIFloat \"HUDMainMenu/Probe/_key_calls\") + 1)\nSetUIFloat \"HUDMainMenu/Probe/_key_value\" key\nEnd) 1 0)\nEnd";
    assert_eq!(game.run_query(source, RunCtx::default()).unwrap(), XVal::Num(1.0));
    assert_eq!(game.jip_event("keydown", None, vec![XVal::Num(1.0)]), 1);
    assert_eq!((read(&game, "key", "calls"), read(&game, "key", "value")), (1.0, 1.0));
    assert_clean(&game);
}
