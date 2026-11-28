//! The random encounters every co-op player sets off (`fnv_game::Game::coop_encounters`):
//! the rule picks them from what their scripts do. A synthetic plugin built in memory,
//! its scripts compiled from source at load; no game data.
//!
//! Picked: a den that comes back days after it was cleared, its trigger and its
//! creature, and a trigger that springs an ambush. Not picked: a trigger that moves a
//! quest's stage, a quest NPC's OnHit that reads a stage, a trigger that enables one actor
//! only, a timer that reads GameDaysPassed but brings nobody in, and the near misses of
//! the last two rules: a den built like the first whose actor a quest's script names, and
//! a trigger built like the ambush whose actors stand there from the start.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig};
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL: u32 = 0x200;
const DAYS: u32 = 0x300;
const QUEST: u32 = 0x310;
const STORY_QUEST: u32 = 0x311;
// Scripts.
const S_DEN: u32 = 0x900;
const S_DEN_TRIGGER: u32 = 0x901;
const S_CRITTER: u32 = 0x902;
const S_AMBUSH: u32 = 0x903;
const S_STORY_TRIGGER: u32 = 0x904;
const S_QUEST_NPC: u32 = 0x905;
const S_ONE_ACTOR: u32 = 0x906;
const S_TIMER: u32 = 0x907;
const S_CAMP: u32 = 0x908;
const S_STORY_QUEST: u32 = 0x909;
const S_SCENE: u32 = 0x90A;
// Bases.
const B_DEN: u32 = 0xA00;
const B_DEN_TRIGGER: u32 = 0xA01;
const B_CRITTER: u32 = 0xA02;
const B_PLAIN: u32 = 0xA03;
const B_AMBUSH: u32 = 0xA04;
const B_STORY_TRIGGER: u32 = 0xA05;
const B_QUEST_NPC: u32 = 0xA06;
const B_ONE_ACTOR: u32 = 0xA07;
const B_TIMER: u32 = 0xA08;
const B_CAMP: u32 = 0xA09;
const B_SCENE: u32 = 0xA0A;
// References.
const R_DEN: u32 = 0xB00;
const R_DEN_TRIGGER: u32 = 0xB01;
const R_CRITTER: u32 = 0xB02;
const R_BANDIT_A: u32 = 0xB03;
const R_BANDIT_B: u32 = 0xB04;
const R_AMBUSH: u32 = 0xB05;
const R_STORY_TRIGGER: u32 = 0xB06;
const R_QUEST_NPC: u32 = 0xB07;
const R_ONE_ACTOR: u32 = 0xB08;
const R_TIMER: u32 = 0xB09;
const R_CAMP: u32 = 0xB0A;
const R_GUARD: u32 = 0xB0B;
const R_SCENE: u32 = 0xB0C;
const R_LOCAL_A: u32 = 0xB0D;
const R_LOCAL_B: u32 = 0xB0E;

/// A placed reference's "initially disabled" record flag.
const INITIALLY_DISABLED: u32 = 0x800;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// A script compiled from its source at load (it declares a string variable, which
/// makes the build compile it).
fn source_script(fid: u32, edid: &str, source: &str) -> Vec<u8> {
    source_script_with(fid, edid, source, &[])
}

/// The same, its record listing its variables (index, name, type: 1 integer, 0 float) as
/// a compiled record does: another script that sets one of them by name finds it there.
/// `sSource` comes first, as the source declares it first.
fn source_script_with(fid: u32, edid: &str, source: &str, vars: &[(u32, &str, u8)]) -> Vec<u8> {
    let mut subs = unit(0, &script(&[(0, None, Vec::new())]), vars, &[]);
    subs.push(sub(b"SCTX", format!("scn {edid}\nstring_var sSource\n{source}").into_bytes()));
    record(b"SCPT", 0, fid, &[vec![sub(b"EDID", z(edid))], subs].concat())
}

fn acbs() -> fnv_esm::raw::Subrecord {
    let mut d = 0u32.to_le_bytes().to_vec();
    d.extend([0u8; 16]);
    d.extend(0i16.to_le_bytes());
    d.extend(0u16.to_le_bytes());
    sub(b"ACBS", d)
}

fn npc(fid: u32, edid: &str, script: Option<u32>) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), acbs(), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())];
    if let Some(s) = script {
        subs.push(sub(b"SCRI", s.to_le_bytes().to_vec()));
    }
    record(b"NPC_", 0, fid, &subs)
}

fn acti(fid: u32, edid: &str, script: u32) -> Vec<u8> {
    record(b"ACTI", 0, fid, &[sub(b"EDID", z(edid)), sub(b"SCRI", script.to_le_bytes().to_vec())])
}

fn placed(sig: &[u8; 4], fid: u32, edid: &str, base: u32, x: f32) -> Vec<u8> {
    placed_with(sig, 0, fid, edid, base, x)
}

fn placed_with(sig: &[u8; 4], flags: u32, fid: u32, edid: &str, base: u32, x: f32) -> Vec<u8> {
    let data = [x, 0.0, 0.0, 0.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    record(sig, flags, fid, &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn plugin_bytes() -> Vec<u8> {
    let scripts = [
        // A den that comes back three days after it was cleared.
        source_script_with(
            S_DEN,
            "TestDenScript",
            "short bEnters\nfloat fDays\nbegin GameMode\nif GameDaysPassed >= fDays + 3\nif TestCritterRef.GetDead\nTestCritterRef.Enable\nset fDays to GameDaysPassed\nendif\nendif\nend\n",
            &[(1, "sSource", 0), (2, "bEnters", 1), (3, "fDays", 0)],
        ),
        // Its trigger tells it the player came in.
        source_script(S_DEN_TRIGGER, "TestDenTriggerScript", "begin OnTriggerEnter player\nset TestDenRef.bEnters to 1\nend\n"),
        // Its creature's own script.
        source_script(S_CRITTER, "TestCritterScript", "short iDead\nbegin OnDeath\nset iDead to 1\nend\n"),
        // An ambush sprung by a trigger.
        source_script(S_AMBUSH, "TestAmbushScript", "short bDone\nbegin OnTriggerEnter player\nif bDone == 0\nTestBanditARef.Enable\nTestBanditBRef.Enable\nset bDone to 1\nendif\nend\n"),
        // The same, but it moves a quest's story.
        source_script(
            S_STORY_TRIGGER,
            "TestStoryTriggerScript",
            "short bDone\nbegin OnTriggerEnter player\nif bDone == 0\nTestBanditARef.Enable\nTestBanditBRef.Enable\nSetStage TestQuest 10\nset bDone to 1\nendif\nend\n",
        ),
        // A quest NPC's hit block that reads its quest's stage.
        source_script(S_QUEST_NPC, "TestQuestNpcScript", "short iHit\nbegin OnHit player\nif GetStage TestQuest >= 10\nset iHit to 1\nendif\nend\n"),
        // A trigger that brings one actor in.
        source_script(S_ONE_ACTOR, "TestOneActorScript", "short bDone\nbegin OnTriggerEnter player\nTestBanditARef.Enable\nset bDone to 1\nend\n"),
        // A timer on GameDaysPassed that brings nobody in.
        source_script(S_TIMER, "TestTimerScript", "float fDays\nshort iTicks\nbegin GameMode\nif GameDaysPassed >= fDays + 1\nset iTicks to iTicks + 1\nset fDays to GameDaysPassed\nendif\nend\n"),
        // Built like the den, but its actor is one a quest's script names.
        source_script(
            S_CAMP,
            "TestCampScript",
            "float fDays\nbegin GameMode\nif GameDaysPassed >= fDays + 3\nif TestGuardRef.GetDead\nTestGuardRef.Enable\nset fDays to GameDaysPassed\nendif\nendif\nend\n",
        ),
        // That quest's script.
        source_script(S_STORY_QUEST, "TestStoryQuestScript", "short iDone\nbegin GameMode\nif TestGuardRef.GetDead\nset iDone to 1\nendif\nend\n"),
        // Built like the ambush, but its actors stand there from the start.
        source_script(S_SCENE, "TestSceneScript", "short bDone\nbegin OnTriggerEnter player\nif bDone == 0\nTestLocalARef.Enable\nTestLocalBRef.Enable\nset bDone to 1\nendif\nend\n"),
    ]
    .concat();
    let npcs = [npc(0x7, "Player", None), npc(B_CRITTER, "TestCritter", Some(S_CRITTER)), npc(B_PLAIN, "TestBandit", None), npc(B_QUEST_NPC, "TestQuestNpc", Some(S_QUEST_NPC))].concat();
    let actis = [
        acti(B_DEN, "TestDen", S_DEN),
        acti(B_DEN_TRIGGER, "TestDenTrigger", S_DEN_TRIGGER),
        acti(B_AMBUSH, "TestAmbush", S_AMBUSH),
        acti(B_STORY_TRIGGER, "TestStoryTrigger", S_STORY_TRIGGER),
        acti(B_ONE_ACTOR, "TestOneActor", S_ONE_ACTOR),
        acti(B_TIMER, "TestTimer", S_TIMER),
        acti(B_CAMP, "TestCamp", S_CAMP),
        acti(B_SCENE, "TestScene", S_SCENE),
    ]
    .concat();
    let glob = record(b"GLOB", 0, DAYS, &[sub(b"EDID", z("GameDaysPassed")), sub(b"FNAM", vec![b'f']), sub(b"FLTV", f32b(0.0))]);
    let quest = [
        record(b"QUST", 0, QUEST, &[sub(b"EDID", z("TestQuest")), sub(b"DATA", [vec![0, 50, 0, 0], f32b(0.0)].concat())]),
        record(b"QUST", 0, STORY_QUEST, &[sub(b"EDID", z("TestStoryQuest")), sub(b"SCRI", S_STORY_QUEST.to_le_bytes().to_vec()), sub(b"DATA", [vec![0, 50, 0, 0], f32b(0.0)].concat())]),
    ]
    .concat();
    let refs = [
        placed(b"REFR", R_DEN, "TestDenRef", B_DEN, 0.0),
        placed(b"REFR", R_DEN_TRIGGER, "TestDenTriggerRef", B_DEN_TRIGGER, 100.0),
        placed(b"ACHR", R_CRITTER, "TestCritterRef", B_CRITTER, 200.0),
        placed_with(b"ACHR", INITIALLY_DISABLED, R_BANDIT_A, "TestBanditARef", B_PLAIN, 300.0),
        placed_with(b"ACHR", INITIALLY_DISABLED, R_BANDIT_B, "TestBanditBRef", B_PLAIN, 400.0),
        placed(b"REFR", R_AMBUSH, "TestAmbushRef", B_AMBUSH, 500.0),
        placed(b"REFR", R_STORY_TRIGGER, "TestStoryTriggerRef", B_STORY_TRIGGER, 600.0),
        placed(b"ACHR", R_QUEST_NPC, "TestQuestNpcRef", B_QUEST_NPC, 700.0),
        placed(b"REFR", R_ONE_ACTOR, "TestOneActorRef", B_ONE_ACTOR, 800.0),
        placed(b"REFR", R_TIMER, "TestTimerRef", B_TIMER, 900.0),
        placed(b"REFR", R_CAMP, "TestCampRef", B_CAMP, 1000.0),
        placed_with(b"ACHR", INITIALLY_DISABLED, R_GUARD, "TestGuardRef", B_PLAIN, 1100.0),
        placed(b"REFR", R_SCENE, "TestSceneRef", B_SCENE, 1200.0),
        placed(b"ACHR", R_LOCAL_A, "TestLocalARef", B_PLAIN, 1300.0),
        placed(b"ACHR", R_LOCAL_B, "TestLocalBRef", B_PLAIN, 1400.0),
    ]
    .concat();
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    let cells = [cell, group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs))].concat();
    [
        tes4(),
        top(b"GLOB", &glob),
        top(b"NPC_", &npcs),
        top(b"ACTI", &actis),
        top(b"SCPT", &scripts),
        top(b"QUST", &quest),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cells))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "CoopEncounters.esm".into()).unwrap()];
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
    let mut g = Game::new(&f.data, world, GameConfig::default());
    g.new_game();
    g.player_entered(Some(FormId(CELL)));
    g
}

#[test]
fn the_encounters_are_picked_by_what_their_scripts_do() {
    let f = fixture();
    let g = game(&f);
    let found = g.coop_encounters();
    for (s, what) in [(S_DEN, "the den"), (S_DEN_TRIGGER, "the den's trigger"), (S_CRITTER, "the den's creature"), (S_AMBUSH, "the ambush trigger")] {
        assert!(found.contains(&FormId(s)), "{what} is an encounter: {found:?}");
    }
    for (s, what) in [
        (S_STORY_TRIGGER, "a trigger that sets a stage"),
        (S_QUEST_NPC, "a quest NPC's OnHit"),
        (S_ONE_ACTOR, "a trigger for one actor"),
        (S_TIMER, "a timer that brings nobody"),
        (S_CAMP, "a den whose actor the story names"),
        (S_SCENE, "a trigger for actors already there"),
    ] {
        assert!(!found.contains(&FormId(s)), "{what} is no encounter: {found:?}");
    }
}

#[test]
fn an_encounter_counts_other_players_only_with_other_players_in_the_game() {
    let f = fixture();
    let mut g = game(&f);
    // Alone: nothing changes (single player).
    assert!(!g.coop_encounter(FormId(R_AMBUSH)));
    g.coop_actors.insert(FormId(R_BANDIT_B), "B".into());
    assert!(g.coop_encounter(FormId(R_AMBUSH)));
    assert!(!g.coop_encounter(FormId(R_STORY_TRIGGER)));
}
