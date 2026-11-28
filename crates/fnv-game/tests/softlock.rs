//! The soft-lock watchdog (#2365; `fnv_game::softlock`).

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::softlock::{story, Kind, Limits, Outside, Watchdog};
use fnv_game::trace::EventKind;
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let quest = record(b"QUST", 0, 0x900, &[sub(b"EDID", z("HoldQuest"))]);
    let topic = |fid: u32, name: &str, kind: u8| record(b"DIAL", 0, fid, &[sub(b"EDID", z(name)), sub(b"DATA", vec![kind, 0])]);
    let topics = [topic(0x910, "Attack", 2), topic(0x911, "Alarm", 4), topic(0x912, "HELLO", 1), topic(0x913, "SceneTopic", 0)].concat();
    let bytes = [tes4(), top(b"QUST", &quest), top(b"DIAL", &topics)].concat();
    let plugins = vec![Plugin::parse(&bytes, "Softlock.esm".into()).unwrap()];
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

fn run(g: &mut Game, body: &str) {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"));
}

/// `seconds` of frames; the findings on the way.
fn frames(dog: &mut Watchdog, g: &Game, seconds: f64) -> Vec<Kind> {
    let n = (seconds * 10.0).round() as usize;
    (0..n).filter_map(|_| dog.step(g, 0.1, Outside::default())).map(|f| f.kind).collect()
}

#[test]
fn controls_held_with_nothing_going_on_is_found_once() {
    let f = fixture();
    let mut g = game(&f);
    let mut dog = Watchdog::default();
    assert_eq!(frames(&mut dog, &g, 60.0), [], "a free player is not held");
    run(&mut g, "DisablePlayerControls");
    assert_eq!(frames(&mut dog, &g, 29.0), [], "under the limit");
    let found = (0..30).find_map(|_| dog.step(&g, 0.1, Outside::default())).expect("found at 30 s");
    assert_eq!(found.kind, Kind::ControlsHeld);
    assert!(found.text.contains("scripts hold the controls (movement, Pip-Boy, fighting, POV)"), "{}", found.text);
    assert_eq!(frames(&mut dog, &g, 120.0), [], "once per hold");
    run(&mut g, "EnablePlayerControls");
    assert_eq!(frames(&mut dog, &g, 0.1), [], "a frame with the controls back");
    run(&mut g, "DisablePlayerControls");
    assert_eq!(frames(&mut dog, &g, 31.0), [Kind::ControlsHeld], "a new hold is found again");
}

#[test]
fn a_story_event_starts_the_wait_again() {
    let f = fixture();
    let mut g = game(&f);
    let mut dog = Watchdog::default();
    run(&mut g, "DisablePlayerControls");
    assert_eq!(frames(&mut dog, &g, 25.0), []);
    g.emit(EventKind::StageSet { quest: FormId(0x900), stage: 10, entries: vec![] });
    assert_eq!(frames(&mut dog, &g, 25.0), [], "the scene went on 25 s in");
    let found = (0..100).find_map(|_| dog.step(&g, 0.1, Outside::default())).expect("found 30 s after the stage");
    assert!(found.text.contains("last stage set: HoldQuest 10"), "{}", found.text);
}

/// Issue #3676: a DLC's load message box shown and answered while the controls stay off
/// starts the wait again, but one hold is one finding until the player is free.
#[test]
fn a_message_box_within_a_reported_hold_is_no_second_finding() {
    let f = fixture();
    let mut g = game(&f);
    let mut dog = Watchdog::new(Limits { controls: 5.0, ..Limits::default() });
    run(&mut g, "DisablePlayerControls");
    assert_eq!(frames(&mut dog, &g, 5.5), [Kind::ControlsHeld]);
    g.menu = Some(1001);
    g.emit(EventKind::MenuOpened { menu: 1001 });
    assert_eq!(frames(&mut dog, &g, 3.0), [], "the box is the player's turn");
    g.menu = None;
    g.emit(EventKind::MenuClosed { menu: 1001 });
    assert_eq!(frames(&mut dog, &g, 60.0), [], "the same hold, reported once");
    // Free for a frame, then held again: a new hold.
    run(&mut g, "EnablePlayerControls");
    assert_eq!(frames(&mut dog, &g, 0.1), []);
    run(&mut g, "DisablePlayerControls");
    assert_eq!(frames(&mut dog, &g, 6.0), [Kind::ControlsHeld], "a new hold is found again");
}

#[test]
fn a_restrained_player_is_held_and_a_dead_one_is_not() {
    let f = fixture();
    let mut g = game(&f);
    let mut dog = Watchdog::new(Limits { controls: 2.0, ..Limits::default() });
    g.extra.entry(FormId::PLAYER_REF).or_default().restrained = true;
    let found = (0..30).find_map(|_| dog.step(&g, 0.1, Outside::default())).expect("found at the shortened limit");
    assert!(found.text.starts_with("the player is restrained"), "{}", found.text);
    g.extra.entry(FormId::PLAYER_REF).or_default().dead = true;
    assert_eq!(frames(&mut dog, &g, 10.0), []);
}

#[test]
fn timers_package_choices_chatter_and_others_items_are_not_story() {
    let line = EventKind::LineStarted { speaker: FormId(1), target: Some(FormId::PLAYER_REF), topic: FormId(0x77), info: FormId(2), seconds: 1.0 };
    assert!(!story(&line, true), "a hello or an attack shout");
    assert!(story(&line, false), "a scene's line");
    assert!(!story(&EventKind::PackageChosen { actor: FormId(1), package: None }, false));
    assert!(story(&EventKind::MenuOpened { menu: 1001 }, false));
    let spent = |from| EventKind::ItemRemoved { from, item: FormId(3), count: 1, silent: false };
    assert!(!story(&spent(FormId(1)), false), "a fighter's spent round");
    assert!(story(&spent(FormId::PLAYER_REF), false), "an item a script takes from the player");
}

#[test]
fn the_lines_begun_since_a_count_are_the_new_ones() {
    let f = fixture();
    let mut g = game(&f);
    let line = |info| EventKind::LineStarted { speaker: FormId(1), target: None, topic: FormId(0x913), info: FormId(info), seconds: 1.0 };
    g.emit(line(1));
    let mark = g.softlock_seen.lines_begun;
    assert_eq!(g.softlock_seen.lines_since(mark).count(), 0);
    g.emit(line(2));
    g.emit(line(3));
    assert_eq!(g.softlock_seen.lines_since(mark).collect::<Vec<_>>(), [FormId(2), FormId(3)]);
    // More lines than are kept: the count still tells the new from the old.
    for i in 0..fnv_game::softlock::LINES_KEPT as u32 {
        g.emit(line(100 + i));
    }
    let mark = g.softlock_seen.lines_begun;
    g.emit(line(7));
    assert_eq!(g.softlock_seen.lines_since(mark).collect::<Vec<_>>(), [FormId(7)]);
}

#[test]
fn combat_and_detection_topics_and_hello_are_chatter() {
    let f = fixture();
    let g = game(&f);
    assert!(g.chatter_topic(FormId(0x910)), "a Combat topic");
    assert!(g.chatter_topic(FormId(0x911)), "a Detection topic");
    assert!(g.chatter_topic(FormId(0x912)), "HELLO");
    assert!(!g.chatter_topic(FormId(0x913)), "a Topic");
}

/// #3044: DisablePlayerControls keeps who called it (the unit's label) for the players'
/// reports; enabling every group again forgets it, enabling some does not.
#[test]
fn the_caller_of_disable_player_controls_is_kept_until_all_groups_are_back() {
    let f = fixture();
    let mut g = game(&f);
    assert!(g.player.controls_disabled_by.is_none());
    run(&mut g, "DisablePlayerControls");
    let caller = g.player.controls_disabled_by.clone().expect("the caller is kept");
    assert!(!caller.unit.is_empty(), "{caller:?}");
    run(&mut g, "EnablePlayerControls 1 0 0 0 0 0 0");
    assert!(g.player.controls_disabled_by.is_some(), "some groups still held");
    run(&mut g, "EnablePlayerControls");
    assert!(g.player.controls_disabled_by.is_none(), "all back: forgotten");
}
