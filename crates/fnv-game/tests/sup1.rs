//! SUP NVSE batch 1 (#1008; docs/spec-notes/ext-sup1.md) on a synthetic plugin, called
//! from scripts: rounding, points and angles, numbers in words, the actor finders and
//! the topic getters.

mod common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL: u32 = 0x200;
const NPC: u32 = 0x900;
const NEAR: u32 = 0x901;
const FAR: u32 = 0x902;
const DEAD: u32 = 0x903;
const QUEST: u32 = 0x910;
const OTHER_QUEST: u32 = 0x911;
const TOPIC: u32 = 0x920;
const CHOICE: u32 = 0x921;
const INFO_A: u32 = 0x930;
const INFO_B: u32 = 0x931;

fn le(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn placed(fid: u32, edid: &str, base: u32, pos: [f32; 3]) -> Vec<u8> {
    let data = [f32b(pos[0]), f32b(pos[1]), f32b(pos[2]), vec![0; 12]].concat();
    record(b"ACHR", 0, fid, &[sub(b"EDID", z(edid)), sub(b"NAME", le(base)), sub(b"DATA", data)])
}

/// An INFO of `quest`, type 0, with the topics it offers (TCLT).
fn info(id: u32, edid: &str, quest: u32, choices: &[u32]) -> Vec<u8> {
    let mut s = vec![sub(b"EDID", z(edid)), sub(b"DATA", vec![0, 0, 0, 0]), sub(b"QSTI", le(quest))];
    s.extend(choices.iter().map(|&t| sub(b"TCLT", le(t))));
    record(b"INFO", 0, id, &s)
}

fn plugin_bytes() -> Vec<u8> {
    let npcs = [record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]), record(b"NPC_", 0, NPC, &[sub(b"EDID", z("TestNpc"))])].concat();
    let qust = [record(b"QUST", 0, QUEST, &[sub(b"EDID", z("TestQuest"))]), record(b"QUST", 0, OTHER_QUEST, &[sub(b"EDID", z("OtherQuest"))])].concat();
    // A Conversation topic (type 1) of OtherQuest then TestQuest with two lines, and the
    // topic one of them offers.
    let dial = record(
        b"DIAL",
        0,
        TOPIC,
        &[sub(b"EDID", z("TestTopic")), sub(b"QSTI", le(OTHER_QUEST)), sub(b"QSTI", le(QUEST)), sub(b"FULL", z("Hello")), sub(b"PNAM", f32b(50.0)), sub(b"DATA", vec![1, 0])],
    );
    let lines = [info(INFO_A, "TestInfoA", QUEST, &[CHOICE]), info(INFO_B, "TestInfoB", OTHER_QUEST, &[])].concat();
    let choice = record(b"DIAL", 0, CHOICE, &[sub(b"EDID", z("ChoiceTopic")), sub(b"FULL", z("Bye")), sub(b"PNAM", f32b(50.0)), sub(b"DATA", vec![0, 0])]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    // Three actors: one 100 units from the origin, one 1000, and a dead one 50.
    let refs = [
        placed(NEAR, "NearRef", NPC, [100.0, 0.0, 0.0]),
        placed(FAR, "FarRef", NPC, [1000.0, 0.0, 0.0]),
        placed(DEAD, "DeadRef", NPC, [50.0, 0.0, 0.0]),
    ]
    .concat();
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs));
    [
        tes4(),
        top(b"NPC_", &npcs),
        top(b"QUST", &qust),
        top(b"DIAL", &[dial, group(TOPIC.to_le_bytes(), 7, &lines), choice].concat()),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat()))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Sup1.esm".into()).unwrap()];
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

fn s(g: &mut Game, expr: &str) -> String {
    match q(g, &format!("string_var sOut\nlet sOut := {expr}\nSetFunctionValue sOut")) {
        XVal::Str(s) => s,
        v => panic!("{expr}: {v:?}"),
    }
}

/// An array's entries as FormIDs.
fn forms(g: &mut Game, expr: &str) -> Vec<u32> {
    let at = |i: &str| format!("array_var a\nlet a := {expr}\nSetFunctionValue {i}");
    let len = match q(g, &at("(ar_Size a)")) {
        XVal::Num(x) => x as usize,
        v => panic!("{expr}: {v:?}"),
    };
    (0..len)
        .map(|i| match q(g, &at(&format!("(a[{i}])"))) {
            XVal::Ref(r) => r,
            XVal::Num(x) => x as u32,
            v => panic!("{expr}[{i}]: {v:?}"),
        })
        .collect()
}

#[test]
fn numbers_points_and_words() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: Round, RoundAlt].
    assert_eq!((n(&mut g, "Round 37.4"), n(&mut g, "Round 37.8")), (37.0, 38.0));
    assert_eq!(n(&mut g, "RoundAlt 37.55 1 0"), 37.5);
    assert_eq!(n(&mut g, "RoundAlt 37.665654 3 1"), f64::from(37.666f32));
    assert_eq!(n(&mut g, "IsNumberNAN 2.5"), 0.0);
    assert_eq!(q(&mut g, "float f\nlet f := IsNumberNAN\nSetFunctionValue (IsNumberNAN f)"), XVal::Num(1.0));
    assert_eq!(n(&mut g, "GetDistanceBetweenPoints 0 0 0 3 4 12"), 13.0);
    assert_eq!(n(&mut g, "GetHeadingAngleBetweenPoints 0 0 0 10 0"), 90.0);
    assert_eq!(s(&mut g, "NumberToWords 332323"), "three hundred thirty two thousand three hundred twenty three");
    assert_eq!(n(&mut g, "GetSUPVersion"), -1.0);
}

#[test]
fn actor_finders() {
    let f = fixture();
    let mut g = game(&f);
    g.kill(FormId(DEAD), None);
    // The closest living actor; the dead one only when asked for; within a distance.
    assert_eq!(n(&mut g, "FindClosestActor 0 0 0"), f64::from(NEAR));
    assert_eq!(n(&mut g, "FindClosestActor 0 0 0 1"), f64::from(DEAD));
    assert_eq!(n(&mut g, "FindClosestActor 900 0 0"), f64::from(FAR));
    assert_eq!(n(&mut g, "FindClosestActor 500 0 0 0 0 50"), 0.0);
    // From a reference: not itself.
    assert_eq!(n(&mut g, "NearRef.FindClosestActorFromRef"), f64::from(FAR));
    // A random one is one of the living two.
    for _ in 0..8 {
        let r = n(&mut g, "FindRandomActor 0 0 0") as u32;
        assert!(r == NEAR || r == FAR, "{r:X}");
    }
}

#[test]
fn topic_getters() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "TopicGetType TestTopic"), 1.0);
    assert_eq!(forms(&mut g, "TopicGetAllTopicInfos TestTopic"), vec![INFO_A, INFO_B]);
    // With a quest: the lines of the topic's first quest, whichever quest is named
    // [observed: sup1 infos.greeting.quest, infos.creatures.otherquest].
    assert_eq!(forms(&mut g, "TopicGetAllTopicInfos TestTopic OtherQuest"), vec![INFO_B]);
    assert_eq!(forms(&mut g, "TopicGetAllTopicInfos TestTopic TestQuest"), vec![INFO_B]);
    assert_eq!(n(&mut g, "TopicInfoGetChoiceCount TestInfoA"), 1.0);
    // No choices: -1 [observed: sup1 count.bye, has.bye].
    assert_eq!(n(&mut g, "TopicInfoGetChoiceCount TestInfoB"), -1.0);
    assert_eq!(n(&mut g, "TopicInfoHasChoice TestInfoB ChoiceTopic"), -1.0);
    assert_eq!(n(&mut g, "TopicInfoHasChoice TestInfoA ChoiceTopic"), 1.0);
    assert_eq!(n(&mut g, "TopicInfoHasChoice TestInfoA TestTopic"), 0.0);
    assert_eq!(forms(&mut g, "TopicInfoGetAllChoices TestInfoA"), vec![CHOICE]);
}
