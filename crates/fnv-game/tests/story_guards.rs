//! The story guards (`fnv_game::story_guards`): Honest Hearts' arrival, with two made-up
//! quests standing in for NVDLC02MQ00 and NVDLC02MQ01.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig};
use fnv_game::quest::{QuestDef, StageEntry};
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const MQ00: FormId = FormId(0x900);
const MQ01: FormId = FormId(0x901);

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture(with_quests: bool) -> Fixture {
    let plugins = vec![Plugin::parse(&tes4(), "Guards.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let mut data = GameData::build(&lo, &schema, table, &j, &refs);
    if with_quests {
        for (q, edid, stage) in [(MQ00, "NVDLC02MQ00", 100), (MQ01, "NVDLC02MQ01", 5)] {
            let mut def = QuestDef { form: q, edid: Some(edid.into()), ..QuestDef::default() };
            def.stages.insert(stage, vec![StageEntry { flags: 0, conditions: Vec::new(), log: None, unit: None, next_quest: None }]);
            data.quests.insert(q, def);
            data.edids.insert(edid.to_ascii_lowercase(), q);
            data.edid_of.insert(q, edid.into());
        }
    }
    Fixture { data, forms, refs }
}

/// A co-op game: the guards act only in one (`Game::coop_game`).
fn game(f: &Fixture) -> Game<'_> {
    let mut g = solo(f);
    g.coop_game = true;
    g
}

/// A single-player game: no guard acts, as on main.
fn solo(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    Game::new(&f.data, world, GameConfig::default())
}

fn run(g: &mut Game, seconds: f64) {
    let frames = (seconds / g.cfg.frame_seconds).ceil() as usize;
    for _ in 0..frames {
        g.tick();
    }
}

fn arrived(g: &Game) -> bool {
    g.quests.get(&MQ01).is_some_and(|s| s.stages_done.contains(&5))
}

#[test]
fn single_player_has_no_guard() {
    let f = fixture(true);
    let mut g = solo(&f);
    assert!(g.set_stage(MQ00, 100));
    run(&mut g, 120.0);
    assert!(!arrived(&g), "single player: the stalled scene stays as the game left it");
}

#[test]
fn the_guard_sets_the_arrival_stage_when_the_scene_stalls() {
    let f = fixture(true);
    let mut g = game(&f);
    run(&mut g, 5.0);
    assert!(!arrived(&g), "nothing happens before the slideshow ends");
    assert!(g.set_stage(MQ00, 100));
    run(&mut g, 25.0);
    assert!(!arrived(&g), "the guard waits 30 s");
    run(&mut g, 6.0);
    assert!(arrived(&g), "after 30 s the guard sets NVDLC02MQ01 stage 5");
}

#[test]
fn the_guard_stays_out_of_a_scene_that_goes_on() {
    let f = fixture(true);
    let mut g = game(&f);
    assert!(g.set_stage(MQ00, 100));
    run(&mut g, 3.0);
    assert!(g.set_stage(MQ01, 5), "Jed's lines set stage 5");
    run(&mut g, 40.0);
    assert_eq!(g.quest_state(MQ01).stages_done.len(), 1);
}

#[test]
fn the_clock_stands_still_while_a_line_is_said() {
    let f = fixture(true);
    let mut g = game(&f);
    assert!(g.set_stage(MQ00, 100));
    run(&mut g, 20.0);
    // Someone speaks for 30 s: the guard does not count it. (The test world has no
    // speaker in it, so the game cuts the line in each frame, after the guard saw it:
    // it is said again before every frame.)
    let id = |f: u32| serde_json::to_value(FormId(f)).unwrap();
    let line = serde_json::json!({ "speaker": id(0x14), "topic": id(0), "info": id(0), "remaining": 1e9 });
    for _ in 0..(30.0 / g.cfg.frame_seconds).ceil() as usize {
        g.speech = vec![serde_json::from_value(line.clone()).unwrap()];
        g.tick();
    }
    assert!(!arrived(&g));
    run(&mut g, 11.0);
    assert!(arrived(&g));
}

#[test]
fn a_load_order_without_honest_hearts_has_no_guard() {
    let f = fixture(false);
    let mut g = game(&f);
    run(&mut g, 40.0);
    assert!(g.quests.is_empty());
}
