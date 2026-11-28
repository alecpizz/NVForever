//! The function families of the 2026-10-09 coverage round on a synthetic plugin (no
//! game data): reputations, effect scripts on a consumed ingestible, placed references,
//! typed item removal, actor flags from records, factions, ownership, destruction,
//! regions, challenges and the calendar. docs/spec-notes/scripts.md §7.6.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::functions::Ctx;
use fnv_game::game::{Game, GameConfig, InstKey};
use fnv_game::trace::EventKind;
use fnv_game::GameData;
use fnv_script::vm::{ArgValue, Value};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const G_YEAR: u32 = 0x801;
const G_EFF: u32 = 0x807;
const G_TIME: u32 = 0x808;
const G_DONE: u32 = 0x809;
const REP: u32 = 0x820;
const CHAL: u32 = 0x821;
const CHAL_ROBOTS: u32 = 0x825;
const RACE_CHILD: u32 = 0x822;
const F1: u32 = 0x823;
const F2: u32 = 0x824;
const NPC_CHILD: u32 = 0x830;
const NPC_ADULT: u32 = 0x831;
const CREA: u32 = 0x832;
const MISC: u32 = 0x840;
const WEAP: u32 = 0x841;
const FLST: u32 = 0x842;
const NOTE: u32 = 0x843;
const MESG: u32 = 0x844;
const MGEF: u32 = 0x850;
const EFFECT_SCRIPT: u32 = 0x851;
const ALCH: u32 = 0x852;
const ACTI: u32 = 0x860;
const CONT: u32 = 0x861;
const REGN: u32 = 0x870;
const IMAD_FADE: u32 = 0x880;
const IMAD_WHITE: u32 = 0x881;
const IMGS: u32 = 0x882;
const WRLD: u32 = 0x900;
const EXT_CELL: u32 = 0x910;
const CELL: u32 = 0x200;
const ACTI_REF: u32 = 0x300;
const SECOND_ACTI_REF: u32 = 0x302;
const CONT_REF: u32 = 0x301;
const CHILD_REF: u32 = 0x320;
const ADULT_REF: u32 = 0x321;
const CREA_REF: u32 = 0x322;
const TACT_ON: u32 = 0x890;
const TACT_OFF: u32 = 0x891;
const STATION_ON: u32 = 0x330;
const STATION_OFF: u32 = 0x331;

const SCRIPT_EFFECT_ELAPSED_SECONDS: u16 = 0x10BA;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn glob(fid: u32, edid: &str, v: f32) -> Vec<u8> {
    record(b"GLOB", 0, fid, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", f32b(v))])
}

fn placed(sig: &[u8; 4], fid: u32, base: u32, pos: [f32; 3]) -> Vec<u8> {
    let data = [f32b(pos[0]), f32b(pos[1]), f32b(pos[2]), vec![0; 12]].concat();
    record(sig, 0, fid, &[sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn npc(fid: u32, edid: &str, extra: Vec<fnv_esm::raw::Subrecord>) -> Vec<u8> {
    record(b"NPC_", 0, fid, &[vec![sub(b"EDID", z(edid))], extra].concat())
}

/// DSTD: health percent, index, damage stage, flags, then 16 bytes of zeros.
fn dstd(pct: u8, index: u8, flags: u8) -> fnv_esm::raw::Subrecord {
    sub(b"DSTD", [vec![pct, index, index, flags], vec![0; 16]].concat())
}

fn plugin_bytes() -> Vec<u8> {
    let globs = [
        glob(G_YEAR, "GameYear", 2281.0),
        glob(0x802, "GameMonth", 9.0),
        glob(0x803, "GameDay", 18.0),
        glob(0x804, "GameHour", 12.0),
        glob(0x805, "GameDaysPassed", 5.0),
        glob(0x806, "TimeScale", 30.0),
        glob(G_EFF, "GEff", 0.0),
        glob(G_TIME, "GTime", 0.0),
        glob(G_DONE, "GDone", 0.0),
    ]
    .concat();
    let repu = record(b"REPU", 0, REP, &[sub(b"EDID", z("RepTest")), sub(b"DATA", f32b(20.0))]);
    let chal = [
        record(b"CHAL", 0, CHAL, &[sub(b"EDID", z("ChalTest")), sub(b"DATA", [u32s(&[13, 1, 1, 0]), vec![0; 8]].concat())]),
        // Kill any in a category: robots (creature type 6, value2 7), 2 of them, a notice each.
        record(b"CHAL", 0, CHAL_ROBOTS, &[sub(b"EDID", z("ChalRobots")), sub(b"FULL", z("Robot Killer")), sub(b"DATA", [u32s(&[2, 2, 0, 1]), vec![0, 0, 7, 0, 0, 0, 0, 0]].concat())]),
    ]
    .concat();
    let race = record(b"RACE", 0, RACE_CHILD, &[sub(b"EDID", z("ChildRace")), sub(b"DATA", [vec![0; 16], vec![0; 16], u32s(&[4])].concat())]);
    let fact = [
        record(b"FACT", 0, F1, &[sub(b"EDID", z("F1")), sub(b"XNAM", u32s(&[F2, 0, 1])), sub(b"WMI1", REP.to_le_bytes().to_vec())]),
        record(b"FACT", 0, F2, &[sub(b"EDID", z("F2"))]),
    ]
    .concat();
    let snam = |f: u32| sub(b"SNAM", [f.to_le_bytes().to_vec(), vec![0; 4]].concat());
    let npcs = [
        npc(0x7, "Player", vec![]),
        // DATA: base health 50, S.P.E.C.I.A.L. 5 5 5 4 5 5 5 (Charisma 4).
        npc(NPC_CHILD, "ChildNpc", vec![sub(b"RNAM", RACE_CHILD.to_le_bytes().to_vec()), snam(F1), sub(b"DATA", [50i32.to_le_bytes().to_vec(), vec![5, 5, 5, 4, 5, 5, 5]].concat())]),
        npc(NPC_ADULT, "AdultNpc", vec![snam(F2)]),
    ]
    .concat();
    let crea_data = [vec![6u8, 0, 0, 0], vec![0; 6], vec![0; 7]].concat();
    let crea = record(b"CREA", 0, CREA, &[sub(b"EDID", z("Robot")), sub(b"DATA", crea_data)]);
    let misc = record(b"MISC", 0, MISC, &[sub(b"EDID", z("Junk")), sub(b"DATA", [1i32.to_le_bytes().to_vec(), f32b(1.0)].concat())]);
    let weap_data = [10i32.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), f32b(2.0), 5i16.to_le_bytes().to_vec(), vec![6]].concat();
    let weap = record(b"WEAP", 0, WEAP, &[sub(b"EDID", z("Gun")), sub(b"DATA", weap_data), sub(b"REPL", FLST.to_le_bytes().to_vec())]);
    let note = record(b"NOTE", 0, NOTE, &[sub(b"EDID", z("TestHolotape")), sub(b"FULL", z("Test Holotape"))]);
    let mesg = record(b"MESG", 0, MESG, &[sub(b"EDID", z("CellNameMsg")), sub(b"FULL", z("Renamed Cell")), sub(b"DESC", z("unused"))]);
    let flst = record(b"FLST", 0, FLST, &[sub(b"EDID", z("GunList")), sub(b"LNAM", WEAP.to_le_bytes().to_vec())]);
    // The base effect: archetype 1 (Script), its script 0x851 (scripts.md §7.6).
    let mgef_data = [u32s(&[0]), f32b(0.0), u32s(&[EFFECT_SCRIPT]), vec![0xFF; 8], vec![0; 4], vec![0; 4], f32b(1.0), vec![0; 24], vec![0; 8], u32s(&[1]), vec![0xFF; 4]].concat();
    assert_eq!(mgef_data.len(), 72);
    let mgef = record(b"MGEF", 0, MGEF, &[sub(b"EDID", z("TestEffect")), sub(b"DATA", mgef_data)]);
    // Effect script: Start sets GEff 1; Update adds ScriptEffectElapsedSeconds to GTime;
    // Finish sets GDone 1.
    let body_start = set(var(b'G', 1), ex(&[a("1")]));
    let body_update = set(var(b'G', 2), ex(&[var(b'G', 2), xcall(SCRIPT_EFFECT_ELAPSED_SECONDS, &[]), a("+")]));
    let body_finish = set(var(b'G', 3), ex(&[a("1")]));
    let escript = [
        vec![sub(b"EDID", z("TestEffectScript"))],
        unit(0x100, &script(&[(0x11, None, body_start), (0x13, None, body_update), (0x12, None, body_finish)]), &[], &[G_EFF, G_TIME, G_DONE]),
    ]
    .concat();
    let scpt = record(b"SCPT", 0, EFFECT_SCRIPT, &escript);
    let enit = [u32s(&[5]), vec![0; 4], u32s(&[0]), f32b(0.0), u32s(&[0])].concat();
    let efit = [u32s(&[0, 0, 1, 0]), (-1i32).to_le_bytes().to_vec()].concat();
    let alch = record(
        b"ALCH",
        0,
        ALCH,
        &[sub(b"EDID", z("TestChem")), sub(b"DATA", f32b(0.5)), sub(b"ENIT", enit), sub(b"EFID", MGEF.to_le_bytes().to_vec()), sub(b"EFIT", efit)],
    );
    let acti = record(
        b"ACTI",
        0,
        ACTI,
        &[
            sub(b"EDID", z("Crate")),
            sub(b"DEST", [100i32.to_le_bytes().to_vec(), vec![2, 0, 0, 0]].concat()),
            dstd(50, 1, 0),
            sub(b"DSTF", vec![]),
            dstd(0, 2, 4),
            sub(b"DSTF", vec![]),
        ],
    );
    let cnto = |item: u32, n: i32| sub(b"CNTO", [item.to_le_bytes().to_vec(), n.to_le_bytes().to_vec()].concat());
    let cont = record(b"CONT", 0, CONT, &[sub(b"EDID", z("Box")), cnto(MISC, 3), cnto(WEAP, 1), sub(b"DATA", [vec![0], f32b(0.0)].concat())]);
    let rpld = [f32b(0.0), f32b(0.0), f32b(4096.0), f32b(0.0), f32b(4096.0), f32b(4096.0), f32b(0.0), f32b(4096.0)].concat();
    let regn = record(
        b"REGN",
        0,
        REGN,
        &[sub(b"EDID", z("TestRegion")), sub(b"WNAM", WRLD.to_le_bytes().to_vec()), sub(b"RPLI", u32s(&[1024])), sub(b"RPLD", rpld)],
    );
    // IMAD DNAM: animatable, duration, then the key counts (not read here).
    let imad_dnam = |animatable: u32, seconds: f32| sub(b"DNAM", [u32s(&[animatable]), f32b(seconds), vec![0; 236]].concat());
    let imad = [
        record(b"IMAD", 0, IMAD_FADE, &[sub(b"EDID", z("FadeTest")), imad_dnam(1, 0.5)]),
        record(b"IMAD", 0, IMAD_WHITE, &[sub(b"EDID", z("WhiteTest")), imad_dnam(0, 1.0)]),
    ]
    .concat();
    let imgs = record(b"IMGS", 0, IMGS, &[sub(b"EDID", z("TestImageSpace"))]);
    // Radio stations: Radio Station and Continuous Broadcast, and Radio Station with
    // Non-Pipboy (the header bits of radio.md §2).
    let tact = [
        record(b"TACT", 0x4002_0000, TACT_ON, &[sub(b"EDID", z("StationOn"))]),
        record(b"TACT", 0x1002_0000, TACT_OFF, &[sub(b"EDID", z("StationOff"))]),
    ]
    .concat();
    let wrld = record(b"WRLD", 0, WRLD, &[sub(b"EDID", z("TestWorld"))]);
    let ext = record(b"CELL", 0, EXT_CELL, &[sub(b"EDID", z("TestExterior")), sub(b"XCLC", [0i32.to_le_bytes(), 0i32.to_le_bytes()].concat())]);
    let world_children = group(WRLD.to_le_bytes(), 1, &group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &ext)));
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    let refs = [
        placed(b"REFR", ACTI_REF, ACTI, [0.0; 3]),
        placed(b"REFR", SECOND_ACTI_REF, ACTI, [100.0, 0.0, 0.0]),
        placed(b"REFR", CONT_REF, CONT, [0.0; 3]),
        placed(b"ACHR", CHILD_REF, NPC_CHILD, [0.0; 3]),
        placed(b"ACHR", ADULT_REF, NPC_ADULT, [0.0; 3]),
        placed(b"ACRE", CREA_REF, CREA, [0.0; 3]),
        placed(b"REFR", STATION_ON, TACT_ON, [0.0; 3]),
        placed(b"REFR", STATION_OFF, TACT_OFF, [0.0; 3]),
    ]
    .concat();
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs));
    [
        tes4(),
        top(b"GLOB", &globs),
        top(b"REPU", &repu),
        top(b"CHAL", &chal),
        top(b"RACE", &race),
        top(b"FACT", &fact),
        top(b"NPC_", &npcs),
        top(b"CREA", &crea),
        top(b"MISC", &misc),
        top(b"WEAP", &weap),
        top(b"FLST", &flst),
        top(b"NOTE", &note),
        top(b"MESG", &mesg),
        top(b"MGEF", &mgef),
        top(b"SCPT", &scpt),
        top(b"ALCH", &alch),
        top(b"ACTI", &acti),
        top(b"CONT", &cont),
        top(b"REGN", &regn),
        top(b"IMAD", &imad),
        top(b"IMGS", &imgs),
        top(b"TACT", &tact),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat()))),
        top(b"WRLD", &[wrld, world_children].concat()),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Functions.esm".into()).unwrap()];
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

fn r(x: u32) -> ArgValue {
    ArgValue::Value(Value::Ref(x))
}
fn n(x: f64) -> ArgValue {
    ArgValue::Value(Value::Num(x))
}

/// Call a function by opcode on `on` (or nothing), failing the test on a call error.
fn call(g: &mut Game, op: u16, on: Option<u32>, args: &[ArgValue]) -> f64 {
    let ctx = Ctx { unit: "test".into(), opcode: op, on: on.map(FormId), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    let before = g.events.len();
    let v = g.call_function(&ctx, args);
    for e in &g.events[before..] {
        if let EventKind::CallFailed { name, error, .. } | EventKind::Unhandled { name, unit: error, .. } = &e.kind {
            panic!("0x{op:04X} {name}: {error}");
        }
    }
    match v {
        Value::Num(x) => x,
        Value::Ref(f) => f as f64,
    }
}

const PLAYER: u32 = 0x14;

/// Call a function as the character revision's script does (`fnv_game::revision`).
fn call_as(g: &mut Game, unit: &str, op: u16, on: Option<u32>, args: &[ArgValue]) -> f64 {
    let ctx = Ctx { unit: unit.into(), opcode: op, on: on.map(FormId), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    match g.call_function(&ctx, args) {
        Value::Num(x) => x,
        Value::Ref(f) => f as f64,
    }
}

#[test]
fn the_character_revision_keeps_level_perks_and_skill_points() {
    let f = fixture();
    let mut g = game(&f);
    let av = |c: u16| ArgValue::Enum(c);
    let (barter, perk) = (32u16, FormId(0x0009_0001));
    g.player.level = Some(4);
    g.player.xp = 600.0;
    g.player.perks.insert(perk, 1);
    g.player.skill_points.insert(barter, 10.0);
    let before = call(&mut g, 0x100E, Some(PLAYER), &[av(barter)]);
    let rev = fnv_game::revision::REVISION_UNIT;
    call_as(&mut g, rev, 0x1133, Some(PLAYER), &[n(1.0)]);
    call_as(&mut g, rev, 0x126D, None, &[]);
    call_as(&mut g, rev, 0x11A8, Some(PLAYER), &[r(perk.0)]);
    call_as(&mut g, rev, 0x100F, Some(PLAYER), &[av(barter), n(0.0)]);
    call_as(&mut g, rev, 0x1177, None, &[n(600.0)]);
    assert_eq!((g.player.level, g.player.xp, g.player.perks.get(&perk)), (Some(4), 600.0, Some(&1)));
    assert_eq!(call(&mut g, 0x100E, Some(PLAYER), &[av(barter)]), before);
    // The same calls from any other script reset as the GECK wiki says.
    call(&mut g, 0x1133, Some(PLAYER), &[n(1.0)]);
    call(&mut g, 0x126D, None, &[]);
    call(&mut g, 0x11A8, Some(PLAYER), &[r(perk.0)]);
    call(&mut g, 0x100F, Some(PLAYER), &[av(barter), n(0.0)]);
    assert_eq!((g.player.level, g.player.xp, g.player.perks.get(&perk)), (Some(1), 0.0, None));
    // SetActorValue 0 leaves the attributes' part of the skill [observed: gaps6, the
    // value set adds to it]: what a fresh player has.
    let mut fresh = game(&f);
    let formula = call(&mut fresh, 0x100E, Some(PLAYER), &[av(barter)]);
    assert!(formula > 0.0 && formula < before);
    assert_eq!(call(&mut g, 0x100E, Some(PLAYER), &[av(barter)]), formula);
}

/// Perks are kept per actor: `<npc>.AddPerk` gives the NPC the perk (a hired companion's
/// `AddPerk CompanionSuite`), HasPerk on it reads it, RemovePerk takes it back, and none of
/// it touches the player's perks; a teammate also holds the teammate perks.
#[test]
fn npc_perks_are_kept_per_actor() {
    let f = fixture();
    let mut g = game(&f);
    let (suite, reward) = (FormId(0x0009_0002), FormId(0x0009_0003));
    let npc = FormId(ADULT_REF);
    assert_eq!(call(&mut g, 0x11C1, Some(ADULT_REF), &[r(suite.0)]), 0.0);
    call(&mut g, 0x1176, Some(ADULT_REF), &[r(suite.0)]);
    assert_eq!(call(&mut g, 0x11C1, Some(ADULT_REF), &[r(suite.0)]), 1.0, "the NPC holds it");
    assert_eq!(call(&mut g, 0x11C1, Some(PLAYER), &[r(suite.0)]), 0.0, "the player does not");
    assert!(g.player.perks.is_empty());
    assert_eq!(g.held_perks(npc), vec![(suite, 1)]);
    // A teammate perk given on the player reaches the NPC while it is a teammate.
    call(&mut g, 0x1176, Some(PLAYER), &[r(reward.0), n(1.0)]);
    assert_eq!(call(&mut g, 0x11C1, Some(ADULT_REF), &[r(reward.0)]), 0.0, "not a teammate");
    g.extra.entry(npc).or_default().teammate = true;
    assert_eq!(call(&mut g, 0x11C1, Some(ADULT_REF), &[r(reward.0)]), 1.0, "a teammate");
    assert_eq!(g.held_perks(npc), vec![(suite, 1), (reward, 1)]);
    call(&mut g, 0x11A8, Some(ADULT_REF), &[r(suite.0)]);
    assert_eq!(call(&mut g, 0x11C1, Some(ADULT_REF), &[r(suite.0)]), 0.0, "removed");
    assert_eq!(g.player.teammate_perks.len(), 1, "the NPC's RemovePerk leaves the player's");
}

#[test]
fn records_read_for_the_new_functions() {
    let f = fixture();
    let d = &f.data;
    assert_eq!(d.reputations[&FormId(REP)], 20.0);
    assert_eq!(d.challenges[&FormId(CHAL)], 1);
    assert_eq!(d.challenge_thresholds[&FormId(CHAL)], 1);
    assert!(d.child_races.contains(&FormId(RACE_CHILD)));
    assert_eq!(d.faction_reactions[&FormId(F1)], vec![(FormId(F2), 1)]);
    assert_eq!(d.actors[&FormId(CREA)].creature_type, Some(6));
    let m = &d.magic_effects[&FormId(MGEF)];
    assert_eq!((m.archetype, m.script), (1, Some(FormId(EFFECT_SCRIPT))));
    assert_eq!(d.effect_items[&FormId(ALCH)][0].duration, 1);
    let dd = &d.destructibles[&FormId(ACTI)];
    let stages: Vec<(u8, u8, u8)> = dd.stages.iter().map(|s| (s.health_percent, s.index, s.flags)).collect();
    assert_eq!((dd.health, stages), (100, vec![(50, 1, 0), (0, 2, 4)]));
    let rg = &d.regions[&FormId(REGN)];
    assert_eq!(rg.world, Some(FormId(WRLD)));
    assert!(rg.contains(100.0, 100.0) && !rg.contains(5000.0, 100.0) && !rg.contains(-1.0, 2000.0));
    // The gun's repair list (REPL) is the form list holding the gun.
    assert_eq!(d.repair_lists[&FormId(WEAP)], FormId(FLST));
    let g = game(&f);
    assert_eq!(fnv_game::functions::repair_list(&g, FormId(WEAP)), vec![FormId(WEAP)]);
    assert!(fnv_game::functions::repair_list(&g, FormId(MISC)).is_empty());
}

#[test]
fn reputation_points_tiers_and_percentages() {
    let f = fixture();
    let mut g = game(&f);
    // Neutral: every axis 1.
    for axis in 0..3 {
        assert_eq!(call(&mut g, 0x123F, None, &[r(REP), n(axis as f64)]), 1.0);
    }
    // Deed size 3 = 4 points of fame (the documented default); twice = 8 of 20 = level 1.
    call(&mut g, 0x1239, None, &[r(REP), n(1.0), n(3.0)]);
    call(&mut g, 0x1239, None, &[r(REP), n(1.0), n(3.0)]);
    assert_eq!(g.player.reputation[&FormId(REP)], [0.0, 8.0]);
    assert_eq!(call(&mut g, 0x123E, None, &[r(REP), n(1.0)]), 0.4);
    assert_eq!(call(&mut g, 0x123F, None, &[r(REP), n(1.0)]), 4.0); // Accepted
    assert_eq!(call(&mut g, 0x123F, None, &[r(REP), n(0.0)]), 0.0);
    assert_eq!(call(&mut g, 0x123F, None, &[r(REP), n(2.0)]), 0.0);
    // Infamy at level 1 too: equal levels are Mixed (mixed axis 3).
    call(&mut g, 0x123B, None, &[r(REP), n(0.0), n(5.0)]);
    assert_eq!(call(&mut g, 0x123F, None, &[r(REP), n(0.0)]), 3.0);
    assert_eq!(call(&mut g, 0x123F, None, &[r(REP), n(1.0)]), 0.0);
    // RemoveReputation takes the same points back and does not go below 0.
    call(&mut g, 0x123A, None, &[r(REP), n(0.0), n(5.0)]);
    assert_eq!(g.player.reputation[&FormId(REP)][0], 0.0);
    // The grid: each name on its axis.
    use fnv_game::functions::reputation_tiers as t;
    assert_eq!(t(3, 0), [0, 6, 0]);
    assert_eq!(t(0, 2), [0, 0, 5]);
    assert_eq!(t(2, 1), [0, 2, 0]);
    assert_eq!(t(1, 3), [0, 0, 3]);
    assert_eq!(t(3, 3), [5, 0, 0]);
    assert_eq!(t(2, 3), [2, 0, 0]);
    assert_eq!(t(1, 1), [3, 0, 0]);
    assert_eq!(t(0, 0), [1, 1, 1]);
}

/// Faction armour's disguise: its OnEquip sets the reputation to 0 and its OnUnequip
/// sets it to what was kept plus what was earned meanwhile; the real reputation (the
/// one the boxes show) never moves for either.
#[test]
fn a_disguise_leaves_the_real_reputation() {
    use fnv_game::functions::{reputation_hide as hide, reputation_real as real};
    let f = fixture();
    let mut g = game(&f);
    let rep = FormId(REP);
    g.player.reputation.insert(rep, [2.0, 9.0]);
    // Put on: every axis to 0.
    let before = g.player.reputation.clone();
    g.player.reputation.insert(rep, [0.0, 0.0]);
    hide(&mut g, &before);
    assert_eq!(real(&g, rep), [2.0, 9.0]);
    // Fame earned while disguised counts.
    call(&mut g, 0x1239, None, &[r(REP), n(1.0), n(3.0)]);
    assert_eq!(real(&g, rep), [2.0, 13.0]);
    // Taken off: kept plus earned; nothing is hidden any more.
    let before = g.player.reputation.clone();
    g.player.reputation.insert(rep, [2.0, 13.0]);
    hide(&mut g, &before);
    assert_eq!(real(&g, rep), [2.0, 13.0]);
    assert!(g.player.reputation_hidden.is_empty());
}

/// The round 3 rules [observed: layer5-captures.md §9.3 a] on a scale of 20.
#[test]
fn reputation_limits_levels_and_ignored_sizes() {
    let f = fixture();
    let mut g = game(&f);
    let set = |g: &mut Game, axis: f64, v: f64| {
        call(g, 0x1238, None, &[r(REP), n(axis), n(v)]);
    };
    let fame = |g: &mut Game| g.player.reputation[&FormId(REP)][1];
    // AddReputation stops at the scale, also from above it.
    set(&mut g, 1.0, 19.0);
    call(&mut g, 0x1239, None, &[r(REP), n(1.0), n(5.0)]);
    assert_eq!(fame(&mut g), 20.0);
    set(&mut g, 1.0, 40.0);
    call(&mut g, 0x1239, None, &[r(REP), n(1.0), n(1.0)]);
    assert_eq!(fame(&mut g), 20.0);
    // RemoveReputation stops at 0 and leaves a value above the scale above it.
    set(&mut g, 1.0, 40.0);
    call(&mut g, 0x123A, None, &[r(REP), n(1.0), n(1.0)]);
    assert_eq!(fame(&mut g), 39.0);
    set(&mut g, 1.0, 1.0);
    call(&mut g, 0x123A, None, &[r(REP), n(1.0), n(5.0)]);
    assert_eq!(fame(&mut g), 0.0);
    // Sizes other than 1-5 change nothing.
    set(&mut g, 1.0, 5.0);
    for size in [0.0, 6.0, -2.0] {
        call(&mut g, 0x1239, None, &[r(REP), n(1.0), n(size)]);
        call(&mut g, 0x123A, None, &[r(REP), n(1.0), n(size)]);
    }
    assert_eq!(fame(&mut g), 5.0);
    // SetReputation keeps anything; the percentage is not capped.
    set(&mut g, 1.0, -3.0);
    assert_eq!(call(&mut g, 0x123D, None, &[r(REP), n(1.0)]), -3.0);
    assert_eq!(call(&mut g, 0x123E, None, &[r(REP), n(1.0)]), -0.15);
    set(&mut g, 1.0, 40.0);
    assert_eq!(call(&mut g, 0x123E, None, &[r(REP), n(1.0)]), 2.0);
    // Levels at 15 %, 50 %, 100 %, a boundary counting as the higher level.
    for (v, good) in [(2.9, 1.0), (3.0, 4.0), (9.9, 4.0), (10.0, 5.0), (19.9, 5.0), (20.0, 6.0)] {
        set(&mut g, 1.0, v);
        assert_eq!(call(&mut g, 0x123F, None, &[r(REP), n(1.0)]), good, "fame {v}");
    }
}

/// Axes other than 0 and 1 (FalloutNV.esm's scripts use 2 and 3) read 0 with no call
/// error (observer check 30); SetReputation with one changes nothing, fails and stops the
/// script: the line after it does not run (captures o5_00/o5_01, #1127). The other
/// writers change nothing and go on (not captured, observer check 62).
#[test]
fn reputation_other_axes_read_0_and_stop_a_write() {
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = game(&f);
    call(&mut g, 0x1238, None, &[r(REP), n(1.0), n(12.0)]);
    assert_eq!(call(&mut g, 0x123D, None, &[r(REP), n(2.0)]), 0.0);
    assert_eq!(call(&mut g, 0x123E, None, &[r(REP), n(2.0)]), 0.0);
    assert_eq!(call(&mut g, 0x123F, None, &[r(REP), n(3.0)]), 0.0);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut Game, body: &str| g.run_query(&format!("Begin Function {{ }}
{body}
SetFunctionValue 7
End
"), ctx());
    // Axes 0 and 1 set the value and go on.
    assert_eq!(q(&mut g, "SetReputation RepTest 0 5"), Ok(XVal::Num(7.0)));
    assert_eq!(g.player.reputation[&FormId(REP)], [5.0, 12.0]);
    // So do AddReputation, RemoveReputation and AddReputationExact (capture o11_00,
    // observer check 62).
    for line in [
        "SetReputation RepTest 2 5",
        "SetReputation RepTest 3 7",
        "AddReputation RepTest 3 1",
        "AddReputation RepTest 2 1",
        "RemoveReputation RepTest 2 1",
        "RemoveReputation RepTest 3 1",
        "AddReputationExact RepTest 2 5",
        "AddReputationExact RepTest 3 5",
    ] {
        let before = g.events.len();
        assert_ne!(q(&mut g, line), Ok(XVal::Num(7.0)), "{line}: the line after it does not run");
        assert!(g.events[before..].iter().any(|e| matches!(&e.kind, EventKind::CallFailed { .. })), "{line}: traced as a failed call");
        assert_eq!(g.player.reputation[&FormId(REP)], [5.0, 12.0], "{line}");
    }
    // The stop lasts for that run only.
    assert_eq!(q(&mut g, "SetReputation RepTest 1 3"), Ok(XVal::Num(7.0)));
    assert_eq!(g.player.reputation[&FormId(REP)], [5.0, 3.0]);
}

/// `RemoveItem <item> 0` removes nothing and is no failed call: the script goes on
/// (observer check 44, capture o5_00; #1113).
#[test]
fn remove_item_of_none_is_a_no_op() {
    let f = fixture();
    let mut g = game(&f);
    call(&mut g, 0x1002, Some(ADULT_REF), &[r(MISC), n(3.0)]);
    // `call` panics on a failed call.
    call(&mut g, 0x1052, Some(ADULT_REF), &[r(MISC), n(0.0)]);
    assert_eq!(call(&mut g, 0x102F, Some(ADULT_REF), &[r(MISC)]), 3.0);
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::ItemRemoved { .. })));
}

/// No DATA in the record: its Health is only `max_health`'s stand-in of 1, nothing to run
/// out of.
#[test]
fn damage_actor_value_does_not_kill_an_actor_whose_record_gives_no_health() {
    let f = fixture();
    let mut g = game(&f);
    call(&mut g, 0x1181, Some(ADULT_REF), &[ArgValue::Enum(16), n(5.0)]);
    assert!(!g.is_dead(FormId(ADULT_REF)));
    assert_eq!(call(&mut g, 0x102E, Some(ADULT_REF), &[]), 0.0);
}

/// Damage and restore [documented: catalogue; observed: layer5-captures.md §9.3 b].
#[test]
fn resurrect_generates_a_dead_actor_again_where_it_lies() {
    let f = fixture();
    let mut g = game(&f);
    let adult = FormId(ADULT_REF);
    let av = |c: u16| ArgValue::Enum(c);
    // Alive, given three items and hurt: ResurrectActor only heals it.
    call(&mut g, 0x1002, Some(ADULT_REF), &[r(MISC), n(3.0)]);
    call(&mut g, 0x1181, Some(ADULT_REF), &[av(16), n(5.0)]);
    call(&mut g, 0x108C, Some(ADULT_REF), &[]);
    assert_eq!(call(&mut g, 0x102F, Some(ADULT_REF), &[r(MISC)]), 3.0);
    assert!(g.extra[&adult].av_damage.is_empty());
    assert_eq!(g.rolls(adult), 0);
    let first = g.take_resurrections();
    assert_eq!((first.len(), first[0].regenerated, first[0].animate), (1, false, false));
    // Killed, one item taken from the corpse, moved by a script, then raised with the
    // animate flag: its own item list is not rolled again (what is left stays, capture
    // rs1), and it stands where it was put.
    call(&mut g, 0x108B, Some(ADULT_REF), &[r(PLAYER)]);
    call(&mut g, 0x1052, Some(ADULT_REF), &[r(MISC), n(1.0)]);
    let put = [150.0, -20.0, 8.0];
    g.world.get(adult).unwrap().pos = put;
    call(&mut g, 0x108C, Some(ADULT_REF), &[n(1.0)]);
    assert_eq!(call(&mut g, 0x102E, Some(ADULT_REF), &[]), 0.0);
    assert_eq!(call(&mut g, 0x102F, Some(ADULT_REF), &[r(MISC)]), 2.0);
    assert_eq!(g.world.peek(adult).unwrap().pos, put);
    // The engine tells where its body lay: the reference goes there, its walker starts
    // again from it.
    g.teleported.clear();
    g.stand_where_the_body_lies(adult, [180.0, -10.0, 9.0]);
    assert_eq!(g.world.peek(adult).unwrap().pos, [180.0, -10.0, 9.0]);
    assert!(g.teleported.contains(&adult));
    assert_eq!(g.rolls(adult), 1);
    assert_eq!(g.take_resurrections(), vec![fnv_game::resurrect::Resurrection { actor: adult, animate: true, regenerated: true, rolls: 1, restocked: 0 }]);
    assert!(g.take_resurrections().is_empty());
}

#[test]
fn roll_seeds_keep_the_first_pick_and_differ_after() {
    use fnv_sim::leveled::roll_seed;
    let r = FormId(ADULT_REF);
    assert_eq!(roll_seed(r, 0), r);
    let seeds: std::collections::BTreeSet<FormId> = (0..64).map(|n| roll_seed(r, n)).collect();
    assert_eq!(seeds.len(), 64);
}

#[test]
fn restore_heals_damage_only() {
    let f = fixture();
    let mut g = game(&f);
    let av = |c: u16| ArgValue::Enum(c);
    // Perception condition (25) starts at 100.
    call(&mut g, 0x1181, Some(PLAYER), &[av(25), n(40.0)]);
    assert_eq!(call(&mut g, 0x100E, Some(PLAYER), &[av(25)]), 60.0);
    call(&mut g, 0x1182, Some(PLAYER), &[av(25), n(100.0)]);
    assert_eq!(call(&mut g, 0x100E, Some(PLAYER), &[av(25)]), 100.0);
    call(&mut g, 0x1182, Some(PLAYER), &[av(25), n(100.0)]);
    assert_eq!(call(&mut g, 0x100E, Some(PLAYER), &[av(25)]), 100.0);
    // ForceActorValue moves the permanent modifier so the current value is the target;
    // the base stays.
    call(&mut g, 0x1181, Some(PLAYER), &[av(25), n(10.0)]);
    call(&mut g, 0x110E, Some(PLAYER), &[av(25), n(50.0)]);
    assert_eq!(call(&mut g, 0x100E, Some(PLAYER), &[av(25)]), 50.0);
    assert_eq!(call(&mut g, 0x1115, Some(PLAYER), &[av(25)]), 100.0);
    assert_eq!(call(&mut g, 0x11EF, Some(PLAYER), &[av(25)]), 60.0);
    call(&mut g, 0x1182, Some(PLAYER), &[av(25), n(100.0)]);
    assert_eq!(call(&mut g, 0x100E, Some(PLAYER), &[av(25)]), 60.0);
    // ModActorValue is permanent.
    call(&mut g, 0x1010, Some(PLAYER), &[av(25), n(5.0)]);
    assert_eq!(call(&mut g, 0x100E, Some(PLAYER), &[av(25)]), 65.0);
}

#[test]
fn a_consumed_ingestible_runs_its_effect_script_for_its_duration() {
    let f = fixture();
    let mut g = game(&f);
    let p = FormId::PLAYER_REF;
    g.world.add_item(p, FormId(ALCH), 2).unwrap();
    let ids = g.consume(p, FormId(ALCH)).unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(g.world.item_count(p, FormId(ALCH)).unwrap(), 1);
    // ScriptEffectStart ran inside the call.
    assert_eq!(g.world.global(FormId(G_EFF)).unwrap(), 1.0);
    assert_eq!(call(&mut g, 0x10DF, Some(PLAYER), &[r(ALCH)]), 1.0);
    assert_eq!(call(&mut g, 0x10D6, Some(PLAYER), &[r(MGEF)]), 1.0);
    // ScriptEffectElapsedSeconds is 0 outside an update.
    assert_eq!(call(&mut g, 0x10BA, None, &[]), 0.0);
    g.frames(30);
    let t = g.world.global(FormId(G_TIME)).unwrap();
    assert!((t - 0.5).abs() < 1e-3, "{t}");
    assert_eq!(g.world.global(FormId(G_DONE)).unwrap(), 0.0);
    // Menus stop the effect clock.
    g.menu = Some(1002);
    g.frames(120);
    g.menu = None;
    assert_eq!(g.world.global(FormId(G_TIME)).unwrap(), t);
    g.frames(30);
    assert_eq!(g.world.global(FormId(G_DONE)).unwrap(), 1.0);
    assert!(g.effects.is_empty());
    assert!(!g.instances.keys().any(|k| matches!(k, InstKey::Effect { .. })));
    assert_eq!(call(&mut g, 0x10DF, Some(PLAYER), &[r(ALCH)]), 0.0);
    // EquipItem of an aid item consumes it the same way.
    g.world.set_global(FormId(G_EFF), 0.0).unwrap();
    call(&mut g, 0x10EE, Some(PLAYER), &[r(ALCH)]);
    assert_eq!(g.world.item_count(p, FormId(ALCH)).unwrap(), 0);
    assert_eq!(g.world.global(FormId(G_EFF)).unwrap(), 1.0);
    // Dispel ends it at once, with its finish block.
    g.world.set_global(FormId(G_DONE), 0.0).unwrap();
    call(&mut g, 0x1142, Some(PLAYER), &[r(ALCH)]);
    assert!(g.effects.is_empty());
    assert_eq!(g.world.global(FormId(G_DONE)).unwrap(), 1.0);
}

/// The same item again while its effect is active adds its duration: one effect, one
/// Finish [observed: a second Cateye, layer5-captures.md §9.3 b].
#[test]
fn the_same_item_again_extends_the_active_effect() {
    let f = fixture();
    let mut g = game(&f);
    let p = FormId::PLAYER_REF;
    g.world.add_item(p, FormId(ALCH), 2).unwrap();
    g.consume(p, FormId(ALCH)).unwrap();
    g.frames(30);
    g.world.set_global(FormId(G_EFF), 0.0).unwrap();
    assert!(g.consume(p, FormId(ALCH)).unwrap().is_empty());
    assert_eq!(g.effects.len(), 1);
    assert_eq!(g.effects[0].duration, 2.0);
    // No second start.
    assert_eq!(g.world.global(FormId(G_EFF)).unwrap(), 0.0);
    // 2 s from the first dose: not done after 1.5 s, done after 2 s.
    g.frames(59);
    assert_eq!(g.world.global(FormId(G_DONE)).unwrap(), 0.0);
    g.frames(31);
    assert_eq!(g.world.global(FormId(G_DONE)).unwrap(), 1.0);
    assert!(g.effects.is_empty());
}

#[test]
fn place_at_reticle_places_where_the_crosshair_hits() {
    use fnv_game::functions::senses::Reticle;
    let f = fixture();
    let mut g = game(&f);
    // No crosshair told (headless): nothing placed.
    assert_eq!(call(&mut g, 0x1263, None, &[r(MISC), n(1.0)]), 0.0);
    assert_eq!(g.world.created().count(), 0);
    let me = g.world.get(FormId::PLAYER_REF).unwrap();
    let (heading, cell) = (me.rot[2], me.cell);
    g.senses.reticle = Some(Reticle { start: [100.0, 200.0, 50.0], dir: [0.0, 1.0, 0.0], hit: Some(300.0), limb: None });
    // An item: one stack of the count, at the hit, in the player's cell, turned as the
    // player.
    let junk = call(&mut g, 0x1263, Some(PLAYER), &[r(MISC), n(3.0)]) as u32;
    let s = g.world.peek(FormId(junk)).unwrap().clone();
    assert_eq!((s.base, s.count, s.cell, s.pos, s.rot), (FormId(MISC), 3, cell, [100.0, 500.0, 50.0], [0.0, 0.0, heading]));
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::Placed { reference, .. } if reference == FormId(junk))));
    // Anything else: one reference per count.
    call(&mut g, 0x1263, None, &[r(CONT), n(2.0)]);
    assert_eq!(g.world.created().filter(|(_, s)| s.base == FormId(CONT)).count(), 2);
    // Beyond maxDistance (the 4th parameter; the 3rd is unused): nothing.
    assert_eq!(call(&mut g, 0x1263, None, &[r(CONT), n(1.0), n(0.0), n(299.0)]), 0.0);
    assert!(call(&mut g, 0x1263, None, &[r(CONT), n(1.0), n(0.0), n(300.0)]) != 0.0);
    // The ray hit nothing.
    g.senses.reticle = Some(Reticle { start: [0.0; 3], dir: [1.0, 0.0, 0.0], hit: None, limb: None });
    assert_eq!(call(&mut g, 0x1263, None, &[r(CONT), n(1.0)]), 0.0);
    assert_eq!(g.world.created().filter(|(_, s)| s.base == FormId(CONT)).count(), 3);
}

#[test]
fn place_at_me_drop_and_typed_removal() {
    let f = fixture();
    let mut g = game(&f);
    // An item makes one reference with the count; another form one per count.
    let junk = call(&mut g, 0x1025, Some(ACTI_REF), &[r(MISC), n(5.0)]) as u32;
    let s = g.world.peek(FormId(junk)).unwrap().clone();
    assert_eq!((s.base, s.count, s.cell), (FormId(MISC), 5, Some(FormId(CELL))));
    call(&mut g, 0x1025, Some(ACTI_REF), &[r(CONT), n(2.0)]);
    let boxes = g.world.created().filter(|(_, s)| s.base == FormId(CONT)).count();
    assert_eq!(boxes, 2);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::Placed { .. })));
    // Weapons (type 40) move to the player; the junk stays.
    call(&mut g, 0x1249, Some(CONT_REF), &[r(PLAYER), n(0.0), n(0.0), n(40.0)]);
    assert_eq!(g.world.item_count(FormId::PLAYER_REF, FormId(WEAP)).unwrap(), 1);
    assert_eq!(g.world.item_count(FormId(CONT_REF), FormId(MISC)).unwrap(), 3);
    // With no type everything goes, destroyed without a target.
    call(&mut g, 0x1249, Some(CONT_REF), &[]);
    assert_eq!(g.world.item_count(FormId(CONT_REF), FormId(MISC)).unwrap(), 0);
    // Drop: the player's items fall into the world.
    g.world.add_item(FormId::PLAYER_REF, FormId(MISC), 2).unwrap();
    call(&mut g, 0x1057, Some(PLAYER), &[r(MISC), n(2.0)]);
    assert_eq!(g.world.item_count(FormId::PLAYER_REF, FormId(MISC)).unwrap(), 0);
    assert!(g.world.newest_created(FormId(MISC)).is_some());
    // AddItemHealthPercent and the equipped weapon's condition.
    call(&mut g, 0x11BC, Some(PLAYER), &[r(WEAP), n(1.0), n(0.5)]);
    call(&mut g, 0x10EE, Some(PLAYER), &[r(WEAP)]);
    let hp = call(&mut g, 0x11F4, Some(PLAYER), &[]);
    assert!(hp == 100.0 || hp == 50.0);
    call(&mut g, 0x11F5, Some(PLAYER), &[n(40.0)]);
    assert!((call(&mut g, 0x11F4, Some(PLAYER), &[]) - 40.0).abs() < 1e-3);
    call(&mut g, 0x11F6, Some(PLAYER), &[n(-15.0)]);
    assert!((call(&mut g, 0x11F4, Some(PLAYER), &[]) - 25.0).abs() < 1e-3);
    assert_eq!(call(&mut g, 0x118F, Some(PLAYER), &[r(FLST)]), 1.0);
}

#[test]
fn create_at_puts_an_actor_where_asked_and_makes_it_a_created_actor() {
    let f = fixture();
    let mut g = game(&f);
    let at = [1234.0, -567.0, 89.0];
    let r = g.create_at(FormId(NPC_ADULT), Some(FormId(CELL)), at, [0.0, 0.0, 1.5]).unwrap();
    let s = g.world.peek(r).unwrap().clone();
    assert_eq!((s.base, s.cell, s.pos), (FormId(NPC_ADULT), Some(FormId(CELL)), at));
    assert!(g.created_actors().contains(&r));
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::Placed { reference, .. } if reference == r)));
    // An unknown base is refused.
    assert!(g.create_at(FormId(0x00FF_FFF0), Some(FormId(CELL)), at, [0.0; 3]).is_err());
}

#[test]
fn get_starting_angle_on_dynamic_place_at_me_returns_creation_heading() {
    let f = fixture();
    let mut g = game(&f);
    // Player facing 270 deg; PlaceAtMe copies the player's Z heading onto the dynamic ref
    // [observed: b3_jip4_02_session_time_and_start_angle, dynZ=270.00].
    g.world.get(FormId::PLAYER_REF).unwrap().rot = [0.0, 0.0, 270.0f32.to_radians()];
    let dyn_ref = call(&mut g, 0x1025, Some(PLAYER), &[r(MISC), n(1.0)]) as u32;
    let z0 = call(&mut g, 0x100B, Some(dyn_ref), &[ArgValue::Axis('Z')]);
    assert!((z0 - 270.0).abs() < 1e-3, "expected 270.0 deg, got {z0}");
    // Rotating the ref afterwards does not alter GetStartingAngle.
    g.world.get(FormId(dyn_ref)).unwrap().rot[2] = 45.0f32.to_radians();
    let z1 = call(&mut g, 0x100B, Some(dyn_ref), &[ArgValue::Axis('Z')]);
    assert!((z1 - 270.0).abs() < 1e-3, "starting angle stays 270.0 after rotation, got {z1}");
    // Player still reads 0 on all axes [observed: jip4].
    assert_eq!(call(&mut g, 0x100B, Some(PLAYER), &[ArgValue::Axis('Z')]), 0.0);
    // Dynamic starting angle survives snapshot save/restore.
    let snap = g.snapshot();
    let mut restored = Game::restore(&f.data, World::restore(&f.forms, &f.refs, &snap.sim).unwrap(), GameConfig::default(), &snap).unwrap();
    let z2 = call(&mut restored, 0x100B, Some(dyn_ref), &[ArgValue::Axis('Z')]);
    assert!((z2 - 270.0).abs() < 1e-3, "starting angle survives save/restore, got {z2}");
    // Older saves without `made_rot` still deserialize cleanly and default to 0.0.
    let mut legacy = serde_json::to_value(&snap.sim).unwrap();
    let entry = legacy["refs"].as_object_mut().unwrap().get_mut(&format!("{}", FormId(dyn_ref))).unwrap();
    assert!(entry.as_object_mut().unwrap().remove("made_rot").is_some());
    let legacy_sim: fnv_sim::world::Snapshot = serde_json::from_value(legacy).unwrap();
    let mut legacy_game = Game::restore(&f.data, World::restore(&f.forms, &f.refs, &legacy_sim).unwrap(), GameConfig::default(), &snap).unwrap();
    assert_eq!(call(&mut legacy_game, 0x100B, Some(dyn_ref), &[ArgValue::Axis('Z')]), 0.0);
}

#[test]
fn actor_flags_factions_and_essential_state() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(call(&mut g, 0x116D, Some(CHILD_REF), &[]), 1.0);
    assert_eq!(call(&mut g, 0x116D, Some(ADULT_REF), &[]), 0.0);
    assert_eq!(call(&mut g, 0x11B6, Some(CREA_REF), &[n(6.0)]), 1.0);
    assert_eq!(call(&mut g, 0x11B6, Some(CREA_REF), &[n(5.0)]), 0.0);
    // F1 lists F2 as an enemy (FACT XNAM); SetAlly changes both directions.
    assert_eq!(call(&mut g, 0x11C2, Some(CHILD_REF), &[r(ADULT_REF)]), 1.0);
    assert_eq!(call(&mut g, 0x11C2, Some(ADULT_REF), &[r(CHILD_REF)]), 0.0);
    call(&mut g, 0x1079, None, &[r(F1), r(F2), n(1.0)]);
    assert_eq!(call(&mut g, 0x11C2, Some(CHILD_REF), &[r(ADULT_REF)]), 3.0);
    assert_eq!(call(&mut g, 0x119B, None, &[r(F2), r(F1)]), 2.0);
    // The player as a crime enemy of F2.
    call(&mut g, 0x10C6, None, &[r(F2), n(1.0)]);
    assert_eq!(call(&mut g, 0x11CC, Some(ADULT_REF), &[]), 1.0);
    assert_eq!(call(&mut g, 0x11C2, Some(ADULT_REF), &[r(PLAYER)]), 1.0);
    call(&mut g, 0x11C9, None, &[r(F2)]);
    assert_eq!(call(&mut g, 0x10C5, None, &[r(F2)]), 0.0);
    call(&mut g, 0x1214, Some(ADULT_REF), &[]);
    assert_eq!(call(&mut g, 0x1047, Some(ADULT_REF), &[r(F2)]), 0.0);
    // Essential: the base flag covers every reference; the reference flag adds to it.
    assert_eq!(call(&mut g, 0x1162, Some(ADULT_REF), &[]), 0.0);
    call(&mut g, 0x1273, Some(ADULT_REF), &[n(1.0)]);
    assert_eq!(call(&mut g, 0x1162, Some(ADULT_REF), &[]), 1.0);
    call(&mut g, 0x1003, None, &[r(NPC_CHILD), n(1.0)]);
    assert_eq!(call(&mut g, 0x1162, Some(CHILD_REF), &[]), 1.0);
    // KillActor keeps the killer (of an actor no longer essential: an essential one is
    // knocked out instead, docs/spec-notes/unconscious.md §2.2).
    call(&mut g, 0x1003, None, &[r(NPC_CHILD), n(0.0)]);
    call(&mut g, 0x108B, Some(CHILD_REF), &[r(PLAYER)]);
    assert_eq!(call(&mut g, 0x1199, Some(CHILD_REF), &[r(PLAYER)]), 1.0);
    assert_eq!(call(&mut g, 0x1199, Some(CHILD_REF), &[r(ADULT_REF)]), 0.0);
}

/// Seen thefts and murders against a faction's people add infamy with its reputation
/// (FACT WMI1), up to the scale; unseen ones and other factions' people none.
#[test]
fn crimes_against_a_factions_people_add_infamy() {
    use fnv_game::crime::Kind;
    use fnv_game::functions::{crime_infamy, reputations_of};
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(reputations_of(&g, FormId(F1)), vec![FormId(REP)]);
    assert_eq!(reputations_of(&g, FormId(CHILD_REF)), vec![FormId(REP)]);
    assert!(reputations_of(&g, FormId(ADULT_REF)).is_empty());
    assert!(crime_infamy(&mut g, Kind::Theft, FormId(F1), false).is_empty());
    assert_eq!(crime_infamy(&mut g, Kind::Theft, FormId(F1), true), vec![(FormId(REP), 0.0, 2.0)]);
    assert!(crime_infamy(&mut g, Kind::Trespass, FormId(F1), true).is_empty());
    assert!(crime_infamy(&mut g, Kind::Murder, FormId(ADULT_REF), true).is_empty());
    // 2 + 30 stops at the scale, 20.
    assert_eq!(crime_infamy(&mut g, Kind::Murder, FormId(CHILD_REF), true), vec![(FormId(REP), 2.0, 20.0)]);
    assert_eq!(g.player.reputation[&FormId(REP)], [20.0, 0.0]);
}

#[test]
fn ownership_destruction_regions_challenges_and_calendar() {
    let f = fixture();
    let mut g = game(&f);
    // Ownership: the player by default.
    assert_eq!(call(&mut g, 0x1116, Some(ACTI_REF), &[]), 0.0);
    call(&mut g, 0x1117, Some(ACTI_REF), &[]);
    assert_eq!(call(&mut g, 0x1116, Some(ACTI_REF), &[]), 1.0);
    call(&mut g, 0x1117, Some(ACTI_REF), &[r(F1)]);
    assert_eq!(call(&mut g, 0x1116, Some(ACTI_REF), &[r(F1)]), 1.0);
    call(&mut g, 0x118C, Some(ACTI_REF), &[]);
    assert_eq!(call(&mut g, 0x1116, Some(ACTI_REF), &[r(F1)]), 0.0);
    // Destruction: no damage −1; above every stage 0; 60 of 100 → index 1, GetDestructionStage
    // 2 (counted from 1, #3053); 40 more → index 2 (3), destroyed.
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), -1.0);
    call(&mut g, 0x116C, Some(ACTI_REF), &[n(10.0)]);
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), 0.0);
    call(&mut g, 0x116C, Some(ACTI_REF), &[n(50.0)]);
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), 2.0);
    call(&mut g, 0x116C, Some(ACTI_REF), &[n(40.0)]);
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), 3.0);
    assert_eq!(call(&mut g, 0x10CB, Some(ACTI_REF), &[]), 1.0);
    call(&mut g, 0x11D8, Some(ACTI_REF), &[]);
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), -1.0);
    assert_eq!(call(&mut g, 0x10CB, Some(ACTI_REF), &[]), 0.0);
    // Regions: never in an interior; in the exterior area, by position.
    assert_eq!(call(&mut g, 0x1260, None, &[r(REGN)]), 0.0);
    {
        let p = g.world.get(FormId::PLAYER_REF).unwrap();
        p.cell = Some(FormId(EXT_CELL));
        p.pos = [100.0, 200.0, 0.0];
    }
    assert_eq!(call(&mut g, 0x1260, None, &[r(REGN)]), 1.0);
    g.world.get(FormId::PLAYER_REF).unwrap().pos = [5000.0, 200.0, 0.0];
    assert_eq!(call(&mut g, 0x1264, None, &[r(REGN)]), 0.0);
    // Challenges and fast travel.
    call(&mut g, 0x125E, None, &[r(CHAL)]);
    assert!(g.player.challenges[&FormId(CHAL)].unlocked);
    assert_eq!(call(&mut g, 0x1266, None, &[r(CHAL)]), 0.0);
    call(&mut g, 0x1111, None, &[n(0.0), n(0.0), n(0.0)]);
    assert!(g.player.fast_travel.travel_disabled && g.player.fast_travel.wait_disabled);
    // From the interior back outside, without the keep flag: travel allowed again.
    g.player_entered(Some(FormId(CELL)));
    g.player_entered(Some(FormId(EXT_CELL)));
    assert!(!g.player.fast_travel.travel_disabled);
    // 18 October 2281 (month 9 counted from 0) is a Friday on the game's calendar, five
    // days after Sunday 13 October (observed: 25 October 2281 read 5, a Friday).
    assert_eq!(call(&mut g, 0x10AA, None, &[]), 5.0);
    // GetHeadingAngle: a target due east of a caller facing north is 90° to the right.
    g.world.get(FormId(ADULT_REF)).unwrap().pos = [0.0, 0.0, 0.0];
    g.world.get(FormId(CHILD_REF)).unwrap().pos = [100.0, 0.0, 0.0];
    assert!((call(&mut g, 0x1063, Some(ADULT_REF), &[r(CHILD_REF)]) - 90.0).abs() < 1e-3);
}

/// Combat C7: object damage through `Game::damage_object` (DamageObject's rule).
#[test]
fn destruction_stages_from_combat_damage() {
    let f = fixture();
    let mut g = game(&f);
    let acti = FormId(ACTI_REF);
    assert!(g.is_destructible(acti));
    // 60 of 100: 40 % left, at or below the 50 % stage.
    let c = g.damage_object(acti, 60.0).unwrap();
    assert_eq!((c.before, c.after, c.destroyed), (-1, 1, false));
    // A small hit changes nothing.
    let c = g.damage_object(acti, 5.0).unwrap();
    assert_eq!((c.before, c.after), (1, 1));
    // The rest reaches the 0 % stage, which destroys it.
    let c = g.damage_object(acti, 100.0).unwrap();
    assert_eq!((c.after, c.destroyed), (2, true));
    assert!(g.damage_object(acti, 10.0).is_none());
    // Index 2 counted from 1 (#3053).
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), 3.0);
}

fn capped_destruction_fixture() -> Fixture {
    use fnv_game::data::{DestructionDef, DestructionStage as Stage};
    let mut f = fixture();
    // Synthetic stages for the numerical regression; no model or script data needed.
    let stages = [98, 95, 75].into_iter().enumerate().map(|(index, health_percent)| {
        Stage {
            health_percent,
            index: index as u8,
            flags: Stage::CAP_DAMAGE | if index == 2 { Stage::DISABLE | Stage::DESTROY } else { 0 },
            self_damage_per_second: if index < 2 { 1 } else { 0 },
            explosion: (index == 2).then_some(FormId(0x893)),
            ..Stage::default()
        }
    }).collect();
    f.data.destructibles.insert(FormId(ACTI), DestructionDef { health: 20, stages, ..DestructionDef::default() });
    f
}

/// The Exploding Destructible Indicator tweak's list: an object burning toward a stage
/// that explodes, until it gets there.
#[test]
fn a_burning_object_is_listed_until_it_explodes() {
    let f = capped_destruction_fixture();
    let mut g = game(&f);
    let acti = FormId(ACTI_REF);
    assert!(g.burning_explosives().is_empty());
    // 0.5 of 20: stage 0 (98 %), which burns, its later stage 2 explodes.
    g.damage_object(acti, 0.5).unwrap();
    assert_eq!(g.burning_explosives(), [(acti, FormId(0x893))]);
    for _ in 0..8 {
        g.damage_object(acti, 20.0);
    }
    assert!(g.burning_explosives().is_empty());
}

/// #3053: GetDestructionStage counts the stage reached from 1, so Mothership Zeta's
/// generator core (eight stages; index 6 at 5 % Cap Damage + Destroy, index 7 at 1 %
/// Disable) reads 7 once destroyed, the value its script disables it at; a destroyed
/// object takes no more damage (GECK wiki Destructible Object Stage), so 8 is not reached.
#[test]
fn get_destruction_stage_counts_from_one_as_the_zeta_core_needs() {
    use fnv_game::data::{DestructionDef, DestructionStage as Stage};
    let mut f = fixture();
    let stages = [98, 90, 70, 40, 20, 10, 5, 1]
        .into_iter()
        .enumerate()
        .map(|(i, health_percent)| Stage {
            health_percent,
            index: i as u8,
            damage_stage: i as u8 + 1,
            flags: match i {
                6 => Stage::CAP_DAMAGE | Stage::DESTROY,
                7 => Stage::DISABLE,
                _ => 0,
            },
            ..Stage::default()
        })
        .collect();
    f.data.destructibles.insert(FormId(ACTI), DestructionDef { health: 800, stages, ..DestructionDef::default() });
    let mut g = game(&f);
    let acti = FormId(ACTI_REF);
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), -1.0);
    g.damage_object(acti, 20.0).unwrap();
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), 1.0, "98 %: the first stage, 1");
    let c = g.damage_object(acti, 1100.0).unwrap();
    assert!(c.destroyed && !c.disabled);
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), 7.0, "the Destroy stage reads 7");
    assert!(g.damage_object(acti, 100.0).is_none(), "destroyed: no more damage");
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), 7.0);
}

fn destruction_roundtrip<'a>(f: &'a Fixture, g: &Game<'_>) -> Game<'a> {
    let saved = serde_json::to_string(&g.snapshot()).unwrap();
    let back = serde_json::from_str::<fnv_game::game::GameSnapshot>(&saved).unwrap();
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    Game::restore(&f.data, world, g.cfg, &back).unwrap()
}

#[test]
fn cap_damage_allows_self_damage_to_reach_each_stage_once() {
    let f = capped_destruction_fixture();
    let mut g = game(&f);
    let acti = FormId(ACTI_REF);
    let first = g.damage_object(acti, 5.0).unwrap();
    assert_eq!((first.before, first.after), (-1, 0));
    assert_eq!(g.extra[&acti].damage, Some(0.4));
    assert_eq!(g.extra[&acti].burning, Some(1.0));
    assert!(first.explosions.is_empty());
    assert!(!first.destroyed && !first.disabled);
    for _ in 0..1000 {
        assert!(g.destruction_tick(0.0001).is_empty());
    }
    assert!((g.extra[&acti].damage.unwrap() - 0.5).abs() < 1e-12);
    let second = g.destruction_tick(2.0);
    assert_eq!(second.len(), 1);
    assert_eq!((second[0].0, second[0].1.before, second[0].1.after), (acti, 0, 1));
    assert_eq!(g.extra[&acti].damage, Some(1.0));
    assert_eq!(g.extra[&acti].burning, Some(1.0));
    assert!(second[0].1.explosions.is_empty());
    assert!(!second[0].1.destroyed && !second[0].1.disabled);
    let last = g.destruction_tick(10.0);
    assert_eq!(last.len(), 1);
    assert_eq!((last[0].0, last[0].1.before, last[0].1.after), (acti, 1, 2));
    assert_eq!(g.extra[&acti].damage, Some(5.0));
    assert_eq!(last[0].1.explosions, [FormId(0x893)]);
    assert!(last[0].1.destroyed && last[0].1.disabled);
    assert!(g.extra[&acti].destroyed);
    assert!(!g.world.get(acti).unwrap().enabled());
    assert_eq!(g.extra[&acti].burning, None);
    assert!(g.destruction_tick(100.0).is_empty());
    assert!(g.damage_object(acti, 100.0).is_none());
}

#[test]
fn cap_damage_continues_after_each_saved_threshold_including_old_rounding() {
    let f = capped_destruction_fixture();
    let acti = FormId(ACTI_REF);
    for legacy in [false, true] {
        let mut g = game(&f);
        g.damage_object(acti, 5.0).unwrap();
        for (stage, percent) in [98, 95, 75].into_iter().enumerate() {
            if legacy {
                g.extra.get_mut(&acti).unwrap().damage = Some(20.0 * (1.0 - f64::from(percent) / 100.0));
            }
            let damage = g.extra[&acti].damage;
            g = destruction_roundtrip(&f, &g);
            // The save's JSON may read a float back one unit in the last place off
            // (serde_json without its float_roundtrip feature), as a real save does: the
            // stage must hold through that, so compare loosely and go on from what loaded.
            let loaded = g.extra[&acti].damage;
            assert!((loaded.unwrap() - damage.unwrap()).abs() <= 1e-12, "{loaded:?} vs {damage:?}");
            let damage = loaded;
            assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), stage as f64 + 1.0);
            if stage == 2 {
                assert!(g.extra[&acti].destroyed);
                assert!(!g.world.get(acti).unwrap().enabled());
                assert!(g.destruction_tick(10.0).is_empty());
                assert!(g.damage_object(acti, 10.0).is_none());
                break;
            }
            assert_eq!(g.extra[&acti].burning, Some(1.0));
            let zero = g.damage_object(acti, 0.0).unwrap();
            assert_eq!((zero.before, zero.after), (stage as i32, stage as i32));
            assert_eq!(g.extra[&acti].damage, damage);
            assert!(g.destruction_tick(0.0).is_empty());
            assert!(g.destruction_tick(0.01).is_empty());
            assert!(g.extra[&acti].damage.unwrap() > damage.unwrap());
            let next = if stage == 0 {
                g.damage_object(acti, 10.0).unwrap()
            } else {
                let mut changes = g.destruction_tick(10.0);
                assert_eq!(changes.len(), 1);
                changes.remove(0).1
            };
            assert_eq!((next.before, next.after), (stage as i32, stage as i32 + 1));
            assert_eq!(g.extra[&acti].damage, Some(if stage == 0 { 1.0 } else { 5.0 }));
            assert_eq!(next.explosions, if stage == 0 { vec![] } else { vec![FormId(0x893)] });
            assert_eq!((next.destroyed, next.disabled), (stage == 1, stage == 1));
        }
    }
}

#[test]
fn cap_damage_and_stage_lookup_agree_for_roundoff_below_a_saved_threshold() {
    let f = capped_destruction_fixture();
    let acti = FormId(ACTI_REF);
    for (stage, damage) in [0.4 - 1e-15, 1.0 - 1e-15].into_iter().enumerate() {
        let mut g = game(&f);
        g.damage_object(acti, 5.0).unwrap();
        if stage == 1 {
            g.damage_object(acti, 5.0).unwrap();
        }
        g.extra.get_mut(&acti).unwrap().damage = Some(damage);
        g = destruction_roundtrip(&f, &g);
        assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), stage as f64 + 1.0);
        let next = g.damage_object(acti, 1e-6).unwrap();
        assert_eq!((next.before, next.after), (stage as i32, stage as i32));
        assert!(g.extra[&acti].damage.unwrap() > damage + 1e-7);
        let next = g.damage_object(acti, 10.0).unwrap();
        assert_eq!((next.before, next.after), (stage as i32, stage as i32 + 1));
    }
}

#[test]
fn cap_crossings_use_damage_even_when_the_stage_already_reads_zero() {
    let f = capped_destruction_fixture();
    let mut g = game(&f);
    let acti = FormId(ACTI_REF);
    let zero = g.damage_object(acti, 0.0).unwrap();
    assert_eq!((zero.before, zero.after), (-1, 0));
    assert_eq!(g.extra[&acti].damage, Some(0.0));
    // Stage 0 is the fallback above every threshold (functions/objects.rs), so it
    // cannot tell whether the first cap has been crossed [design].
    let below = g.damage_object(acti, 0.4 - 1e-10).unwrap();
    assert_eq!((below.before, below.after), (0, 0));
    assert!(g.extra[&acti].damage.unwrap() < 0.4);
    g.damage_object(acti, 5e-11).unwrap();
    assert!(g.extra[&acti].damage.unwrap() < 0.4);
    g.damage_object(acti, 10.0).unwrap();
    assert_eq!(g.extra[&acti].damage, Some(0.4));
    // A distinct, small interval below the later stage must not be rounded away: the
    // script still reads index 0, counted from 1 (#3053).
    g.damage_object(acti, 0.6 - 1e-10).unwrap();
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), 1.0);
    g.damage_object(acti, 5e-11).unwrap();
    assert_eq!(call(&mut g, 0x11D7, Some(ACTI_REF), &[]), 1.0);
    let next = g.damage_object(acti, 10.0).unwrap();
    assert_eq!((next.before, next.after), (0, 1));
    assert_eq!(g.extra[&acti].damage, Some(1.0));
}

#[test]
fn cap_damage_is_independent_for_two_references_across_a_save() {
    let f = capped_destruction_fixture();
    let mut g = game(&f);
    let first = FormId(ACTI_REF);
    let second = FormId(SECOND_ACTI_REF);
    g.damage_object(first, 10.0).unwrap();
    g.damage_object(first, 10.0).unwrap();
    g.damage_object(second, 10.0).unwrap();
    g = destruction_roundtrip(&f, &g);
    assert_eq!((g.extra[&first].damage, g.extra[&second].damage), (Some(1.0), Some(0.4)));
    let mut changes = g.destruction_tick(10.0);
    changes.sort_by_key(|(r, _)| *r);
    assert_eq!(changes.iter().map(|(r, c)| (*r, c.before, c.after)).collect::<Vec<_>>(), [(first, 1, 2), (second, 0, 1)]);
    assert_eq!((g.extra[&first].damage, g.extra[&second].damage), (Some(5.0), Some(1.0)));
    assert!(g.extra[&first].destroyed && !g.extra[&second].destroyed);
    let last = g.destruction_tick(10.0);
    assert_eq!(last.len(), 1);
    assert_eq!((last[0].0, last[0].1.before, last[0].1.after), (second, 1, 2));
    assert_eq!(last[0].1.explosions, [FormId(0x893)]);
    assert!(g.destruction_tick(10.0).is_empty());
}

/// The model damage stage follows the destruction stage reached (the fixture's DSTDs
/// give each stage its index as damage stage), 0 before any damage, after
/// ClearDestruction and for a reference without destruction data.
#[test]
fn model_damage_stage_follows_the_stage_reached() {
    let f = fixture();
    let mut g = game(&f);
    let acti = FormId(ACTI_REF);
    assert_eq!(g.model_damage_stage(acti), 0);
    g.damage_object(acti, 60.0);
    assert_eq!(g.model_damage_stage(acti), 1);
    g.damage_object(acti, 100.0);
    assert_eq!(g.model_damage_stage(acti), 2);
    call(&mut g, 0x11D8, Some(ACTI_REF), &[]);
    assert_eq!(g.model_damage_stage(acti), 0);
    assert_eq!(g.model_damage_stage(FormId(ADULT_REF)), 0);
}

/// DamageObject from a script queues the stage changes for the engine (their
/// explosions and models), as a hit's are; a call that reaches no new stage queues none.
#[test]
fn damage_object_queues_stage_changes_for_the_engine() {
    let f = fixture();
    let mut g = game(&f);
    call(&mut g, 0x116C, Some(ACTI_REF), &[n(60.0)]);
    let q = std::mem::take(&mut g.destruction_changes);
    assert_eq!(q.len(), 1);
    assert_eq!((q[0].0, q[0].1.before, q[0].1.after, q[0].1.destroyed), (FormId(ACTI_REF), -1, 1, false));
    call(&mut g, 0x116C, Some(ACTI_REF), &[n(5.0)]);
    assert!(g.destruction_changes.is_empty());
    call(&mut g, 0x116C, Some(ACTI_REF), &[n(100.0)]);
    let q = std::mem::take(&mut g.destruction_changes);
    assert_eq!((q.len(), q[0].1.after, q[0].1.destroyed), (1, 2, true));
}

/// SetDestroyed: a destroyed activator cannot be activated, by hand or by a script, and
/// works again once the flag is cleared [documented: GECK wiki SetDestroyed].
#[test]
fn destroyed_activator_refuses_activation() {
    use fnv_game::game::ActMode;
    let f = fixture();
    let mut g = game(&f);
    let acti = FormId(ACTI_REF);
    let activations = |g: &Game| g.events.iter().filter(|e| matches!(e.kind, EventKind::Activated { reference, .. } if reference == acti)).count();
    assert!(!g.activation_refused(acti));
    g.activate(FormId(PLAYER), acti, ActMode::Hand).unwrap();
    assert_eq!(activations(&g), 1);
    call(&mut g, 0x10CC, Some(ACTI_REF), &[n(1.0)]);
    assert!(g.activation_refused(acti));
    let v = g.activate(FormId(PLAYER), acti, ActMode::Hand).unwrap();
    assert_eq!(v["result"], "destroyed");
    // `Activate player` from a script goes through the same gate.
    call(&mut g, 0x100D, Some(ACTI_REF), &[r(PLAYER)]);
    assert_eq!(activations(&g), 1);
    // A container is not a door or activator: the flag does not stop it.
    call(&mut g, 0x10CC, Some(CONT_REF), &[n(1.0)]);
    assert!(!g.activation_refused(FormId(CONT_REF)));
    call(&mut g, 0x10CC, Some(ACTI_REF), &[n(0.0)]);
    g.activate(FormId(PLAYER), acti, ActMode::Hand).unwrap();
    assert_eq!(activations(&g), 2);
}

/// Image space modifiers (docs/spec-notes/image-modifiers.md §3): an animatable one
/// ends by itself after its duration, the other stays until removed; menus hold their
/// time; a snapshot keeps them.
#[test]
fn image_space_modifiers_play_end_and_are_removed() {
    const APPLY: u16 = 0x1191;
    const REMOVE: u16 = 0x1192;
    const ACTIVE: u16 = 0x1232;
    let f = fixture();
    assert_eq!(f.data.image_modifiers[&FormId(IMAD_FADE)], fnv_game::data::ImageModifierDef { animatable: true, duration: 0.5 });
    assert!(!f.data.image_modifiers[&FormId(IMAD_WHITE)].animatable);
    let mut g = game(&f);
    assert_eq!(call(&mut g, ACTIVE, None, &[r(IMAD_FADE)]), 0.0);
    call(&mut g, APPLY, None, &[r(IMAD_WHITE)]);
    call(&mut g, APPLY, None, &[r(IMAD_FADE)]);
    assert_eq!(g.image_modifiers.iter().map(|m| m.modifier.0).collect::<Vec<_>>(), [IMAD_WHITE, IMAD_FADE]);
    assert_eq!(call(&mut g, ACTIVE, None, &[r(IMAD_FADE)]), 1.0);
    g.frames(12);
    assert!((g.image_modifiers[1].elapsed - 0.2).abs() < 1e-9);
    // A menu holds the modifiers' time.
    g.menu = Some(1002);
    g.frames(120);
    g.menu = None;
    assert!((g.image_modifiers[1].elapsed - 0.2).abs() < 1e-9);
    let snap = g.snapshot();
    assert_eq!(snap.image_modifiers.len(), 2);
    // Applied again, it stacks a second instance at elapsed 0.0 [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9, 2026-11-23, GSDocMitchellHouse, probe w1_3656_objectives_unholster_imod in #3679].
    call(&mut g, APPLY, None, &[r(IMAD_FADE)]);
    assert_eq!(g.image_modifiers.len(), 3);
    assert!((g.image_modifiers[1].elapsed - 0.2).abs() < 1e-9);
    assert_eq!(g.image_modifiers[2].elapsed, 0.0);
    g.frames(29);
    assert_eq!(call(&mut g, ACTIVE, None, &[r(IMAD_FADE)]), 1.0);
    g.frames(1);
    // The animated one has ended; the other is still on the screen.
    assert_eq!(call(&mut g, ACTIVE, None, &[r(IMAD_FADE)]), 0.0);
    assert_eq!(call(&mut g, ACTIVE, None, &[r(IMAD_WHITE)]), 1.0);
    assert!(g.events.iter().any(|e| e.kind == EventKind::ImageModifierRemoved { modifier: FormId(IMAD_FADE), ended: true }));
    g.frames(600);
    assert_eq!(call(&mut g, ACTIVE, None, &[r(IMAD_WHITE)]), 1.0);
    call(&mut g, REMOVE, None, &[r(IMAD_WHITE)]);
    assert!(g.image_modifiers.is_empty());
    assert!(g.events.iter().any(|e| e.kind == EventKind::ImageModifierRemoved { modifier: FormId(IMAD_WHITE), ended: false }));
    // Removing one that is not on the screen does nothing.
    let n = g.events.len();
    call(&mut g, REMOVE, None, &[r(IMAD_WHITE)]);
    assert_eq!(g.events.len(), n);
    // A form that is no image space modifier fails the call.
    let ctx = Ctx { unit: "test".into(), opcode: APPLY, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[r(MISC)]);
    assert!(matches!(&g.events.last().unwrap().kind, EventKind::CallFailed { name, .. } if name == "ApplyImageSpaceModifier"));
    assert!(g.image_modifiers.is_empty());
}

fn s(text: &str) -> ArgValue {
    ArgValue::Str(text.into())
}

/// PlayBink hands the film to the engine with the page's defaults; the splatter
/// functions queue, clear and set the fade; SetCellImageSpace is kept in the save and
/// refuses what is not a cell or an image space.
#[test]
fn films_splatter_and_cell_image_space() {
    use fnv_game::functions::ui::{FilmRequest, SplatterTrigger};
    const PLAY_BINK: u16 = 0x1114;
    const TRIGGER: u16 = 0x1240;
    const CLEAR: u16 = 0x1241;
    const FADE: u16 = 0x1243;
    const SET_CELL_IMGS: u16 = 0x1196;
    let f = fixture();
    let mut g = game(&f);
    // The opening's calls (QUST 0x00102037 stages 0 and 7).
    call(&mut g, PLAY_BINK, None, &[s("fnvintro.bik"), n(1.0), n(1.0), n(0.0), n(1.0)]);
    call(&mut g, PLAY_BINK, None, &[s("FNVLogo1.bik"), n(0.0), n(0.0), n(0.0), n(1.0)]);
    call(&mut g, PLAY_BINK, None, &[s("..\\Sound\\Videos\\x.bik")]);
    assert_eq!(
        g.ui.films,
        [
            FilmRequest { path: "video\\fnvintro.bik".into(), interruptible: true, mute_audio: true, pause_music: false, letterboxed: true },
            FilmRequest { path: "video\\fnvlogo1.bik".into(), interruptible: false, mute_audio: false, pause_music: false, letterboxed: true },
            FilmRequest { path: "sound\\videos\\x.bik".into(), interruptible: false, mute_audio: true, pause_music: true, letterboxed: true },
        ]
    );
    // A name that leaves Data fails the call.
    let ctx = Ctx { unit: "test".into(), opcode: PLAY_BINK, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[s("..\\..\\x.bik")]);
    assert!(matches!(&g.events.last().unwrap().kind, EventKind::CallFailed { name, .. } if name == "PlayBink"));
    assert_eq!(g.ui.films.len(), 3);

    // Stage 50's calls: the fade, then the triggers in order.
    call(&mut g, FADE, None, &[n(0.5), n(1.0)]);
    call(&mut g, TRIGGER, None, &[n(100.0), n(20.0), n(2.0), n(2.0)]);
    call(&mut g, TRIGGER, None, &[n(50.0), n(0.0), n(2.5), n(2.0)]);
    assert_eq!(g.ui.splatter.fade, Some((0.5, true)));
    // Safe with any values (the original quit at this call twice): held to 0-1, a start
    // that is not a number is 0 (fade from the start), a missing switch on.
    call(&mut g, FADE, None, &[n(7.0), n(0.0)]);
    assert_eq!(g.ui.splatter.fade, Some((1.0, false)));
    call(&mut g, FADE, None, &[n(f64::NAN)]);
    assert_eq!(g.ui.splatter.fade, Some((0.0, true)));
    call(&mut g, FADE, None, &[]);
    assert_eq!(g.ui.splatter.fade, Some((0.0, true)));
    call(&mut g, FADE, None, &[n(0.5), n(1.0)]);
    assert_eq!(
        g.ui.splatter.triggers,
        [SplatterTrigger { count: 100, duration: Some(20.0), size: 2.0, opacity: 2.0 }, SplatterTrigger { count: 50, duration: None, size: 2.5, opacity: 2.0 }]
    );
    // Out-of-range values are bounded; one that is not a number counts as missing.
    call(&mut g, TRIGGER, None, &[n(1e12), n(1e9), n(1e6), n(f64::NAN)]);
    call(&mut g, TRIGGER, None, &[n(-5.0), n(f64::INFINITY), n(-2.0), n(-1.0)]);
    assert_eq!(
        g.ui.splatter.triggers[2..],
        [SplatterTrigger { count: u32::MAX, duration: Some(3600.0), size: 10.0, opacity: 1.0 }, SplatterTrigger { count: 0, duration: None, size: 0.0, opacity: 0.0 }]
    );
    call(&mut g, CLEAR, None, &[]);
    assert!(g.ui.splatter.triggers.is_empty());
    assert_eq!(g.ui.splatter.clears, 1);

    // SetCellImageSpace: kept in the form changes, so in the save.
    call(&mut g, SET_CELL_IMGS, None, &[r(CELL), r(IMGS)]);
    assert_eq!(g.forms.cell_image_spaces.get(&FormId(CELL)), Some(&FormId(IMGS)));
    let snap = g.snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back.forms.cell_image_spaces.get(&FormId(CELL)), Some(&FormId(IMGS)));
    let ctx = Ctx { opcode: SET_CELL_IMGS, ..ctx };
    g.call_function(&ctx, &[r(IMGS), r(IMGS)]);
    assert!(matches!(&g.events.last().unwrap().kind, EventKind::CallFailed { name, .. } if name == "SetCellImageSpace"));
    g.call_function(&ctx, &[r(CELL), r(IMAD_FADE)]);
    assert!(matches!(&g.events.last().unwrap().kind, EventKind::CallFailed { name, .. } if name == "SetCellImageSpace"));
    assert_eq!(g.forms.cell_image_spaces.len(), 1);
}

/// Radio functions (docs/spec-notes/radio.md §5, §8, §9): the broadcast state from the
/// Continuous Broadcast bit until set, conversations kept for the engine, the Pip-Boy
/// radio's commands and SetNPCRadio's on, off and "turn off first" rule.
#[test]
fn radio_functions() {
    const GET_BROADCAST: u16 = 0x11D4;
    const SET_BROADCAST: u16 = 0x11D5;
    const START_CONVERSATION: u16 = 0x11D6;
    const PIPBOY_RADIO: u16 = 0x11A7;
    const PIPBOY_RADIO_OFF: u16 = 0x1224;
    const SET_NPC_RADIO: u16 = 0x11C4;
    const FORCE_UPDATE: u16 = 0x122C;
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(call(&mut g, GET_BROADCAST, Some(STATION_ON), &[]), 1.0);
    assert_eq!(call(&mut g, GET_BROADCAST, Some(STATION_OFF), &[]), 0.0);
    call(&mut g, SET_BROADCAST, Some(STATION_ON), &[n(0.0)]);
    call(&mut g, SET_BROADCAST, Some(STATION_OFF), &[n(1.0)]);
    assert_eq!(call(&mut g, GET_BROADCAST, Some(STATION_ON), &[]), 0.0);
    assert!(g.broadcasting(FormId(STATION_OFF)));

    call(&mut g, START_CONVERSATION, Some(STATION_OFF), &[r(0x126)]);
    call(&mut g, START_CONVERSATION, Some(STATION_ON), &[]);
    assert_eq!(g.take_radio_conversations(), vec![(FormId(STATION_OFF), Some(FormId(0x126))), (FormId(STATION_ON), None)]);
    assert!(g.take_radio_conversations().is_empty());

    // Tune does nothing while the radio is off; 1 with a station turns it on and asks for it.
    call(&mut g, PIPBOY_RADIO, None, &[s("tune"), r(STATION_ON)]);
    assert_eq!((g.radio.pipboy.on, g.radio.pipboy.tune), (false, None));
    call(&mut g, PIPBOY_RADIO, None, &[s("1"), r(STATION_ON)]);
    assert_eq!((g.radio.pipboy.on, g.radio.pipboy.tune), (true, Some(FormId(STATION_ON))));
    g.radio.pipboy.tune = None;
    call(&mut g, PIPBOY_RADIO, None, &[s("tune"), r(STATION_OFF)]);
    assert_eq!(g.radio.pipboy.tune, Some(FormId(STATION_OFF)));
    call(&mut g, PIPBOY_RADIO, None, &[n(0.0)]);
    assert!(!g.radio.pipboy.on);
    call(&mut g, PIPBOY_RADIO, None, &[n(1.0)]);
    assert!(g.radio.pipboy.on);
    call(&mut g, PIPBOY_RADIO_OFF, None, &[]);
    assert!(!g.radio.pipboy.on);
    assert_eq!(g.radio.pipboy.script_calls, 6);

    call(&mut g, SET_NPC_RADIO, Some(ADULT_REF), &[n(1.0), r(STATION_ON)]);
    // A second station is not taken until the first is turned off.
    call(&mut g, SET_NPC_RADIO, Some(ADULT_REF), &[n(1.0), r(STATION_OFF)]);
    assert_eq!(g.radio.npc_radios.get(&FormId(ADULT_REF)), Some(&FormId(STATION_ON)));
    call(&mut g, SET_NPC_RADIO, Some(ADULT_REF), &[n(0.0), r(STATION_OFF)]);
    assert!(g.radio.npc_radios.is_empty());
    call(&mut g, FORCE_UPDATE, None, &[]);
    assert_eq!(g.radio.force_updates, 1);
}

/// The stubs implemented by the audit of 2026-10-12 (docs/design/script-stubs.md §3):
/// what the engine tells (movement, sight, the camera), heads held by Look, subtitles,
/// the weather, scripted challenges and the menus' requests.
/// PickIdle's queue, the AI's frame switches (cleared by every tick) and EnableLoadingMenu's
/// request (script-stubs.md §3.19).
#[test]
fn script_stubs_round_3_requests() {
    let f = fixture();
    let mut g = game(&f);
    let adult = FormId(ADULT_REF);
    call(&mut g, 0x1064, Some(ADULT_REF), &[]);
    assert_eq!(g.idle_picks, vec![adult]);
    // SetAllVisible and SetNoAvoidance hold until the next frame, which clears them.
    call(&mut g, 0x115A, None, &[n(1.0)]);
    call(&mut g, 0x115B, None, &[n(1.0)]);
    assert!(g.frame_switches.all_visible && g.frame_switches.no_avoidance);
    g.tick();
    assert!(!g.frame_switches.all_visible && !g.frame_switches.no_avoidance, "cleared every frame");
    call(&mut g, 0x115A, None, &[n(1.0)]);
    call(&mut g, 0x115A, None, &[n(0.0)]);
    assert!(!g.frame_switches.all_visible);
    // EnableLoadingMenu: a request for the engine; 0 asks for nothing.
    call(&mut g, 0x1270, None, &[n(0.0)]);
    assert!(!g.ui.loading_menu);
    call(&mut g, 0x1270, None, &[n(1.0)]);
    assert!(g.ui.loading_menu);
    for op in [0x1064, 0x115A, 0x115B, 0x1270, 0x10B9] {
        assert!(fnv_game::functions::implemented(op), "0x{op:04X}");
    }
}

#[test]
fn audited_stubs_answer_from_the_engine_state_and_leave_requests() {
    use fnv_game::chargen::{SpecialSheet, TagSheet};
    use fnv_game::functions::senses::{Look, Motion};
    use fnv_game::functions::ui::SpecialMenu;
    let f = fixture();
    let mut g = game(&f);
    let (adult, child) = (FormId(ADULT_REF), FormId(CHILD_REF));

    // Look holds the head on the target until StopLook; with the flag the body turns
    // to it too (`body_looks`, for the AI and the engine), without it only the head.
    call(&mut g, 0x105C, Some(ADULT_REF), &[r(CHILD_REF)]);
    assert_eq!(g.script_look(adult), Some(Look { target: child, rotate_body: false }));
    assert!(g.body_looks().is_empty());
    call(&mut g, 0x105C, Some(ADULT_REF), &[r(CHILD_REF), n(1.0)]);
    assert_eq!(g.script_look(adult), Some(Look { target: child, rotate_body: true }));
    assert_eq!(g.body_looks(), vec![(adult, child)]);
    call(&mut g, 0x105D, Some(ADULT_REF), &[]);
    assert_eq!(g.script_look(adult), None);
    assert!(g.body_looks().is_empty());

    // Movement and sneaking as the engine told them; nothing told is standing still.
    assert_eq!(call(&mut g, 0x111E, None, &[]), 0.0);
    g.set_motion(FormId::PLAYER_REF, Motion { moving: true, running: false, sneaking: true, turning: 0, run_mode: false, swimming: false, in_air: false });
    assert_eq!(call(&mut g, 0x111E, None, &[]), 1.0);
    assert_eq!(call(&mut g, 0x1019, None, &[]), 1.0);
    assert_eq!(call(&mut g, 0x111F, None, &[]), 0.0);
    g.set_motion(adult, Motion { moving: true, running: true, sneaking: false, turning: 0, run_mode: true, swimming: false, in_air: false });
    assert_eq!(call(&mut g, 0x111F, Some(ADULT_REF), &[]), 1.0);
    g.forget_motion(adult);
    assert_eq!(call(&mut g, 0x1019, Some(ADULT_REF), &[]), 0.0);
    // IsSwimming: an NPC's physics swim state; the player always reads 0 (wiki note).
    assert_eq!(call(&mut g, 0x10B9, Some(ADULT_REF), &[]), 0.0);
    g.set_motion(adult, Motion { swimming: true, ..Motion::default() });
    assert_eq!(call(&mut g, 0x10B9, Some(ADULT_REF), &[]), 1.0);
    let before = g.motion(FormId::PLAYER_REF);
    g.set_motion(FormId::PLAYER_REF, Motion { swimming: true, ..before });
    assert_eq!(call(&mut g, 0x10B9, None, &[]), 0.0);
    g.set_motion(FormId::PLAYER_REF, before);
    g.forget_motion(adult);

    // Line of sight from the detection the engine measured for the pair.
    assert_eq!(call(&mut g, 0x101B, Some(ADULT_REF), &[r(CHILD_REF)]), 0.0);
    let seen = fnv_game::detection::Detection { seen: true, ..Default::default() };
    g.detection.record(adult, child, seen, g.time);
    assert_eq!(call(&mut g, 0x101B, Some(ADULT_REF), &[r(CHILD_REF)]), 1.0);

    // The camera: first person until the engine says otherwise.
    assert_eq!(call(&mut g, 0x1188, None, &[]), 1.0);
    g.set_first_person(false);
    assert_eq!(call(&mut g, 0x1188, None, &[]), 0.0);

    // The grab: IsPlayerGrabbedRef answers what the engine says the player holds; a
    // reference without OnGrab / OnRelease blocks runs none.
    assert_eq!(call(&mut g, 0x11D0, None, &[r(CHILD_REF)]), 0.0);
    assert_eq!(g.set_player_grab(Some(child)), (false, false));
    assert_eq!(call(&mut g, 0x11D0, None, &[r(CHILD_REF)]), 1.0);
    assert_eq!(call(&mut g, 0x11D0, None, &[r(ADULT_REF)]), 0.0);
    g.set_player_grab(None);
    assert_eq!(call(&mut g, 0x11D0, None, &[r(CHILD_REF)]), 0.0);

    // Subtitles shown out of hearing too.
    call(&mut g, 0x1277, Some(ADULT_REF), &[n(1.0)]);
    assert!(g.always_subtitles(adult));
    call(&mut g, 0x1277, Some(ADULT_REF), &[n(0.0)]);
    assert!(!g.always_subtitles(adult));

    // The weather: a request for the sky, current from the call; then what the sky shows.
    let (rain, clear) = (0x999u32, 0x998u32);
    call(&mut g, 0x1104, None, &[r(rain), n(1.0)]);
    assert_eq!(call(&mut g, 0x1095, None, &[r(rain)]), 1.0);
    let req = g.take_weather_request().unwrap();
    assert_eq!((req.weather, req.instant, req.hold), (FormId(rain), false, true));
    call(&mut g, 0x112D, None, &[r(clear)]);
    assert!(g.take_weather_request().is_some_and(|r| r.instant && !r.hold));
    g.set_shown_weather(FormId(rain), true);
    assert_eq!(call(&mut g, 0x103E, None, &[]), 1.0);
    assert_eq!(call(&mut g, 0x1095, None, &[r(clear)]), 0.0);
    call(&mut g, 0x1158, None, &[]);
    assert!(g.take_weather_release() && !g.take_weather_release());

    // A Start Disabled challenge with threshold 1: no progress until unlocked, then done
    // and, not recurring, held at its threshold.
    call(&mut g, 0x1275, None, &[r(CHAL)]);
    assert_eq!(call(&mut g, 0x1266, None, &[r(CHAL)]), 0.0);
    call(&mut g, 0x125E, None, &[r(CHAL)]);
    call(&mut g, 0x1275, None, &[r(CHAL)]);
    assert_eq!(call(&mut g, 0x1266, None, &[r(CHAL)]), 1.0);
    call(&mut g, 0x1275, None, &[r(CHAL)]);
    assert_eq!(g.player.challenges[&FormId(CHAL)].progress, 1);

    // The S.P.E.C.I.A.L. menu with two points to add, the old values its floor.
    call(&mut g, 0x1219, None, &[n(2.0)]);
    assert_eq!((g.menu, g.ui.special), (Some(1048), Some(SpecialMenu::Add(2))));
    let s = SpecialSheet::for_menu(&mut g);
    assert_eq!((s.remaining(), s.floor), (2, s.values));
    assert_eq!(g.ui.special, None, "used up by the opening");
    call(&mut g, 0x1218, None, &[n(30.0)]);
    let s = SpecialSheet::for_menu(&mut g);
    assert_eq!(s.total, 30);
    // The tag-skill menu with one more to tag.
    g.player.tag_skills.extend([41u16, 43, 44]);
    call(&mut g, 0x121B, None, &[n(1.0)]);
    assert_eq!(g.menu, Some(1048));
    let t = TagSheet::for_menu(&mut g);
    assert_eq!((t.count, t.remaining(), t.locked.len()), (4, 1, 3));

    // A terminal's result script asks to go back; a quest that is no quest is refused.
    call(&mut g, 0x1223, None, &[]);
    assert!(g.terminals.back);
    let ctx = Ctx { unit: "test".into(), opcode: 0x11DD, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[r(CHAL)]);
    assert!(matches!(&g.events.last().unwrap().kind, EventKind::CallFailed { .. }));
    assert_eq!(g.ui.active_quest, None);

    // The deliberate no-ops: handled, no stub recorded.
    let stubbed = g.stubbed.len();
    call(&mut g, 0x11FB, None, &[n(1.0), n(1.0), n(0.5)]);
    call(&mut g, 0x1189, None, &[]);
    assert_eq!(g.stubbed.len(), stubbed);
}

/// The stubs implemented on 2026-10-12 by claude/script-stubs-2
/// (docs/design/script-stubs.md §3.2).
#[test]
fn second_round_stubs() {
    use fnv_game::functions::senses::Motion;
    let f = fixture();
    let mut g = game(&f);
    let adult = FormId(ADULT_REF);

    // GetXPForNextLevel: level 2 needs 200 in all (iXPBase, the wiki's defaults here);
    // what the player has counts.
    g.player.level = Some(1);
    assert_eq!(call(&mut g, 0x1215, None, &[]), 200.0);
    call(&mut g, 0x1177, None, &[n(50.0)]);
    assert_eq!(call(&mut g, 0x1215, None, &[]), 150.0);
    g.player.level = Some(2);
    assert_eq!(call(&mut g, 0x1215, None, &[]), 500.0, "level 3 at 550");

    // IsDLCInstalled: a plugin of the load order by name, spaces and case ignored.
    assert_eq!(call(&mut g, 0x126F, None, &[s("functions")]), 1.0);
    assert_eq!(call(&mut g, 0x126F, None, &[s("Tribal Pack")]), 0.0);

    // ShowAllMapMarkers 1 0 0 (the Explorer perk): every marker, the hidden ones too.
    let marker = FormId(ACTI_REF);
    assert_eq!(call(&mut g, 0x11EC, Some(ACTI_REF), &[]), 0.0);
    call(&mut g, 0x11F8, None, &[n(1.0), n(0.0), n(0.0)]);
    assert!(g.map_marker_shown(marker, true));
    assert_eq!(call(&mut g, 0x11EC, Some(ACTI_REF), &[]), 1.0);
    call(&mut g, 0x11F8, None, &[n(1.0)]);
    assert!(g.map_marker_shown(marker, false) && !g.map_marker_shown(marker, true), "skips hidden by default");
    call(&mut g, 0x11F8, None, &[n(0.0)]);
    assert!(!g.map_marker_shown(marker, false));

    // SetGlobalTimeMultiplier: kept for the engine; 0 refused.
    call(&mut g, 0x1186, None, &[n(0.5), n(1.0)]);
    assert_eq!(g.ui.time_multiplier, Some(0.5));

    // IsTurning and IsPlayerMovingIntoNewSpace from what the engine tells.
    assert_eq!(call(&mut g, 0x101A, Some(ADULT_REF), &[]), 0.0);
    g.set_motion(adult, Motion { turning: 2, ..Motion::default() });
    assert_eq!(call(&mut g, 0x101A, Some(ADULT_REF), &[]), 2.0);
    assert_eq!(call(&mut g, 0x1166, None, &[]), 0.0);
    g.loading = true;
    assert_eq!(call(&mut g, 0x1166, None, &[]), 1.0);
    g.loading = false;

    // Rotate: the frame's last call turns the reference by speed × frame time, once.
    g.cfg.frame_seconds = 0.5;
    let z0 = g.world.get(FormId(ACTI_REF)).unwrap().rot[2];
    call(&mut g, 0x1004, Some(ACTI_REF), &[ArgValue::Axis('Z'), n(10.0)]);
    call(&mut g, 0x1004, Some(ACTI_REF), &[ArgValue::Axis('Z'), n(90.0)]);
    g.tick();
    let z1 = g.world.get(FormId(ACTI_REF)).unwrap().rot[2];
    assert!((z1 - z0 - 45f32.to_radians()).abs() < 1e-5, "{z0} -> {z1}");
    assert!(g.turned.contains(&FormId(ACTI_REF)));
    g.tick();
    assert_eq!(g.world.get(FormId(ACTI_REF)).unwrap().rot[2], z1, "no call, no turn");
    assert!(g.turned.is_empty());
}

/// SwapTexture (playtest list 5 item 38): the shape's name and the texture's path kept
/// per reference for the engine, the last call per shape winning. The reference is the
/// first parameter, the calls having no calling reference (Doc Mitchell's lines:
/// `SwapTexture TempRorschachTest01REF "PipBoyCard01:0" "…"`).
#[test]
fn swap_texture_keeps_the_last_texture_per_shape() {
    let f = fixture();
    let mut g = game(&f);
    let card = FormId(ADULT_REF);
    call(&mut g, 0x1269, None, &[r(ADULT_REF), s("pipboycard01:0"), s(r"clutter\rorchachtest\inkblot03")]);
    call(&mut g, 0x1269, None, &[r(ADULT_REF), s("##PipBoyCard01:0"), s(r"clutter\rorchachtest\inkblot01")]);
    assert_eq!(g.texture_swaps.generation, 2);
    assert_eq!(g.texture_swaps.by_ref[&card], vec![("pipboycard01:0".to_string(), r"textures\clutter\rorchachtest\inkblot01.dds".to_string())]);
}

/// PlayMagicShaderVisuals / StopMagicShaderVisuals: one play per (reference, shader), a
/// second call starting it over; a duration kept; no calling reference plays it on the
/// player; Stop takes it away (docs/spec-notes/effect-shaders.md §3).
#[test]
fn magic_shader_visuals_play_and_stop() {
    let f = fixture();
    let mut g = game(&f);
    const SHADER: u32 = 0x0007_EA60;
    call(&mut g, 0x1123, Some(ADULT_REF), &[r(SHADER)]);
    call(&mut g, 0x1123, Some(ADULT_REF), &[r(SHADER), n(5.0)]);
    call(&mut g, 0x1123, None, &[r(SHADER)]);
    let plays = &g.script_looks.shaders;
    assert_eq!(plays.len(), 2);
    assert_eq!((plays[0].on, plays[0].duration), (FormId(ADULT_REF), Some(5.0)));
    assert_eq!((plays[1].on, plays[1].duration), (FormId::PLAYER_REF, None));
    assert!(plays[1].serial > plays[0].serial);
    call(&mut g, 0x1125, Some(ADULT_REF), &[r(SHADER)]);
    assert_eq!(g.script_looks.shaders.len(), 1);
    let serial = g.script_looks.shaders[0].serial;
    g.script_looks.end(serial);
    assert!(g.script_looks.shaders.is_empty());
}

/// SetActorAlpha: kept per actor below 1, held to 0..1; 1 takes it away.
#[test]
fn set_actor_alpha_is_kept_below_one() {
    let f = fixture();
    let mut g = game(&f);
    let who = FormId(ADULT_REF);
    call(&mut g, 0x1129, Some(ADULT_REF), &[n(0.5)]);
    assert_eq!(g.script_looks.alpha.get(&who), Some(&0.5));
    call(&mut g, 0x1129, Some(ADULT_REF), &[n(-2.0)]);
    assert_eq!(g.script_looks.alpha.get(&who), Some(&0.0));
    call(&mut g, 0x1129, Some(ADULT_REF), &[n(1.0)]);
    assert!(g.script_looks.alpha.is_empty());
}

/// SetSecuritronExpression: the face and expression name the screen's texture, swapped on
/// the screen shape of the target (effect-shaders.md §5).
#[test]
fn securitron_expression_swaps_the_screen() {
    let f = fixture();
    let mut g = game(&f);
    call(&mut g, 0x1248, None, &[r(ADULT_REF), s("Victor"), s("Alert")]);
    assert_eq!(
        g.texture_swaps.by_ref[&FormId(ADULT_REF)],
        vec![("screen01:0".to_string(), r"textures\creatures\securitron\victor_alert.dds".to_string())]
    );
}

/// Reset3DState: the reference's group no longer counts as playing, its waiting requests
/// are dropped and the engine is told once.
#[test]
fn reset_3d_state_forgets_the_animation() {
    let f = fixture();
    let mut g = game(&f);
    let on = FormId(ADULT_REF);
    g.anims.driven = true;
    g.ask_anim(fnv_game::anims::AnimRequest { on, group: 1, name: Some("Forward".into()), start: fnv_game::anims::Start::Normal });
    assert_eq!(g.anims.playing(on), Some(1));
    assert_eq!(g.anims.asked(on), Some((1, "Forward")));
    call(&mut g, 0x1146, Some(ADULT_REF), &[]);
    call(&mut g, 0x1146, Some(ADULT_REF), &[]);
    assert_eq!(g.anims.playing(on), None);
    assert!(g.anims.requests.is_empty());
    assert_eq!(g.anims.asked(on), None, "nothing to show when drawn again");
    assert_eq!(g.anims.resets, vec![on]);
}

/// #2420: a tripwire's script plays `Forward` and activates itself, which asks "Open",
/// a sequence its model lacks. The engine says so (`ObjectAnims::not_in_model`) and
/// `Forward` is again what the sprung wire shows when drawn again; a name the model has
/// and an unknown name the last ask does not carry change nothing.
#[test]
fn a_group_the_model_lacks_gives_back_the_one_it_replaced() {
    let f = fixture();
    let mut g = game(&f);
    let on = FormId(ADULT_REF);
    let ask = |g: &mut Game, group: u16, name: &str| g.ask_anim(fnv_game::anims::AnimRequest { on, group, name: Some(name.into()), start: fnv_game::anims::Start::Normal });
    ask(&mut g, 1, "Forward");
    ask(&mut g, 0xFF00, "Open");
    assert_eq!(g.anims.asked(on), Some((0xFF00, "Open")));
    g.anims.not_in_model(on, "Backward");
    assert_eq!(g.anims.asked(on), Some((0xFF00, "Open")), "not the last asked: nothing changes");
    g.anims.not_in_model(on, "open");
    assert_eq!(g.anims.asked(on), Some((1, "Forward")), "the sprung wire shows Forward's end");
    // Only one step back: a second missing name with nothing before it clears the ask.
    g.anims.not_in_model(on, "Forward");
    assert_eq!(g.anims.asked(on), None);
}

/// The HUD's compass leaves sneaking actors off (docs/spec-notes/hud.md §11): forced to
/// sneak (SetForceSneak) or measured sneaking by the engine, an actor gets no tick; standing
/// up again, or dead, as before.
#[test]
fn compass_leaves_sneaking_actors_off() {
    use fnv_game::functions::senses::Motion;
    let f = fixture();
    let mut g = game(&f);
    let adult = FormId(ADULT_REF);
    assert!(g.compass_shows(adult));
    call(&mut g, 0x10D3, Some(ADULT_REF), &[n(1.0)]);
    assert!(g.is_sneaking(adult));
    assert!(!g.compass_shows(adult));
    call(&mut g, 0x10D3, Some(ADULT_REF), &[n(0.0)]);
    assert!(g.compass_shows(adult));
    // What the engine measures this frame wins over the rule, as for IsSneaking.
    g.set_motion(adult, Motion { sneaking: true, ..Motion::default() });
    assert!(!g.compass_shows(adult));
    assert_eq!(call(&mut g, 0x111E, Some(ADULT_REF), &[]), 1.0);
    g.set_motion(adult, Motion::default());
    assert!(g.compass_shows(adult));
}

/// The gaps the script function audit fixed (docs/design/script-function-audit.md).
#[test]
fn script_function_audit_fixes() {
    use fnv_game::functions::senses::Motion;
    let f = fixture();
    let mut g = game(&f);
    let av = |c: u16| ArgValue::Enum(c);
    let adult = FormId(ADULT_REF);
    // Form lists: GetItemCount sums the members, GetEquipped asks whether any is worn.
    assert_eq!(call(&mut g, 0x102F, Some(CONT_REF), &[r(FLST)]), 1.0);
    call(&mut g, 0x1002, Some(PLAYER), &[r(WEAP), n(1.0)]);
    assert_eq!(call(&mut g, 0x10B6, Some(PLAYER), &[r(FLST)]), 0.0);
    call(&mut g, 0x10EE, Some(PLAYER), &[r(WEAP)]);
    assert_eq!(call(&mut g, 0x10B6, Some(PLAYER), &[r(FLST)]), 1.0);
    // A setting the masters do not define: the game's own value (observed).
    assert_eq!(call(&mut g, 0x1100, None, &[ArgValue::Str("iLockLevelMaxHard".into())]), 75.0);
    // GetAnimAction: none, unless the engine tells a drawing (0).
    assert_eq!(call(&mut g, 0x10DB, Some(ADULT_REF), &[]), -1.0);
    g.set_anim_state(adult, fnv_game::anim_state::AnimState { action: Some(fnv_game::anim_state::AnimAction::Equip), ..Default::default() });
    assert_eq!(call(&mut g, 0x10DB, Some(ADULT_REF), &[]), 0.0);
    g.forget_anim_state(adult);
    // Perks for the player's teammates are their own list.
    call(&mut g, 0x1176, None, &[r(MISC), n(1.0)]);
    assert_eq!(call(&mut g, 0x11C1, None, &[r(MISC), n(1.0)]), 1.0);
    assert_eq!(call(&mut g, 0x11C1, None, &[r(MISC)]), 0.0);
    call(&mut g, 0x11A8, None, &[r(MISC), n(1.0)]);
    assert_eq!(call(&mut g, 0x11C1, None, &[r(MISC), n(1.0)]), 0.0);
    // A note added as an item: GetHasNote 1, GetItemCount 0; RemoveNote takes it.
    call(&mut g, 0x1002, Some(PLAYER), &[r(NOTE), n(1.0)]);
    assert_eq!(call(&mut g, 0x117E, None, &[r(NOTE)]), 1.0);
    assert_eq!(call(&mut g, 0x102F, Some(PLAYER), &[r(NOTE)]), 0.0);
    call(&mut g, 0x117D, None, &[r(NOTE)]);
    assert_eq!(call(&mut g, 0x117E, None, &[r(NOTE)]), 0.0);
    // A note that reached the inventory (the container menu moves it as an item) is
    // filed with the player's notes on the next frame and is no longer carried.
    g.world.add_item(FormId(PLAYER), FormId(NOTE), 1).unwrap();
    g.tick();
    assert!(g.player.notes.contains(&FormId(NOTE)));
    assert_eq!(g.world.item_count(FormId(PLAYER), FormId(NOTE)).unwrap(), 0);
    // Its corner notice (#1163), by its FULL name; AddNote of a note the player has
    // shows no second one.
    assert_eq!(g.take_note_notices(), vec![FormId(NOTE)]);
    assert_eq!(g.note_notice_text(FormId(NOTE)), "Note: Test Holotape");
    call(&mut g, 0x117C, None, &[r(NOTE)]);
    assert!(g.take_note_notices().is_empty());
    call(&mut g, 0x117D, None, &[r(NOTE)]);
    // AddNote of a new note: its notice too [hypothesis: the original's, observer-checks].
    call(&mut g, 0x117C, None, &[r(NOTE)]);
    assert_eq!(g.take_note_notices(), vec![FormId(NOTE)]);
    call(&mut g, 0x117D, None, &[r(NOTE)]);
    // KillActor's cause of death; ResurrectActor brings the actor back unharmed.
    call(&mut g, 0x1181, Some(ADULT_REF), &[av(16), n(5.0)]);
    call(&mut g, 0x108B, Some(ADULT_REF), &[r(CHILD_REF), n(0.0), n(6.0)]);
    assert_eq!(call(&mut g, 0x118D, Some(ADULT_REF), &[]), 6.0);
    call(&mut g, 0x108C, Some(ADULT_REF), &[n(0.0)]);
    assert_eq!(call(&mut g, 0x102E, Some(ADULT_REF), &[]), 0.0);
    assert_eq!(call(&mut g, 0x118D, Some(ADULT_REF), &[]), -1.0);
    assert!(g.extra[&adult].av_damage.is_empty() && g.extra[&adult].killer.is_none());
    // SetGhost: no damage.
    assert!(!g.is_immune(adult));
    call(&mut g, 0x10EC, Some(ADULT_REF), &[n(1.0)]);
    assert!(g.is_immune(adult));
    // GetHealthPercentage: 1.0 with no base health; above 1 when effects raise Health.
    assert_eq!(call(&mut g, 0x11AF, Some(ADULT_REF), &[]), 1.0);
    call(&mut g, 0x100F, Some(ADULT_REF), &[av(16), n(100.0)]);
    call(&mut g, 0x1010, Some(ADULT_REF), &[av(16), n(50.0)]);
    assert_eq!(call(&mut g, 0x11AF, Some(ADULT_REF), &[]), 1.5);
    // GetLevel of an NPC without SetLevel: its record's level.
    assert_eq!(call(&mut g, 0x1050, Some(ADULT_REF), &[]), 1.0);
    // IsSneaking of an actor the engine does not measure; IsRunning's speed mode.
    call(&mut g, 0x10D3, Some(ADULT_REF), &[n(1.0)]);
    assert_eq!(call(&mut g, 0x111E, Some(ADULT_REF), &[]), 1.0);
    g.set_motion(FormId::PLAYER_REF, Motion { run_mode: true, ..Default::default() });
    assert_eq!(call(&mut g, 0x111F, None, &[]), 1.0);
    // GetMapMarkerVisible: 0 off the map, 1 shown, 2 a travel target or found.
    assert_eq!(call(&mut g, 0x11EC, Some(ACTI_REF), &[]), 0.0);
    call(&mut g, 0x1055, None, &[r(ACTI_REF)]);
    assert_eq!(call(&mut g, 0x11EC, Some(ACTI_REF), &[]), 1.0);
    call(&mut g, 0x1055, None, &[r(ACTI_REF), n(1.0)]);
    assert_eq!(call(&mut g, 0x11EC, Some(ACTI_REF), &[]), 2.0);
    g.set_found_markers([FormId(CONT_REF)]);
    assert_eq!(call(&mut g, 0x11EC, Some(CONT_REF), &[]), 2.0);
    // SetPos and SetAngle reach what moves things: an actor starts again from its new
    // place, an object is drawn there.
    let ax = |c: char| ArgValue::Axis(c);
    call(&mut g, 0x1007, Some(ADULT_REF), &[ax('X'), n(120.0)]);
    assert_eq!(g.world.get(adult).unwrap().pos[0], 120.0);
    assert!(g.teleported.contains(&adult));
    call(&mut g, 0x1009, Some(ACTI_REF), &[ax('Z'), n(90.0)]);
    assert!(g.turned.contains(&FormId(ACTI_REF)));
    g.turned.clear();
    call(&mut g, 0x109E, Some(ACTI_REF), &[r(CONT_REF)]);
    assert!(g.turned.contains(&FormId(ACTI_REF)), "MoveTo redraws an object too");
    // GetOpenState: any object reads 3 closed, an actor 0; what the engine animates, its
    // state.
    assert_eq!(call(&mut g, 0x109D, Some(CONT_REF), &[]), 3.0);
    assert_eq!(call(&mut g, 0x109D, Some(ACTI_REF), &[]), 3.0);
    assert_eq!(call(&mut g, 0x109D, Some(CHILD_REF), &[]), 0.0);
    g.senses.open_states.insert(FormId(ACTI_REF), 2);
    assert_eq!(call(&mut g, 0x109D, Some(ACTI_REF), &[]), 2.0);
    // The actors of the loaded cell were outfitted when it loaded (the game starts in
    // it); a far one was not.
    assert!(g.world.get(FormId(CHILD_REF)).unwrap().outfitted);
    let far = g.world.place(FormId(NPC_ADULT), Some(FormId(EXT_CELL)), [0.0; 3], [0.0; 3], 1).unwrap();
    assert!(g.world.get(far).unwrap().outfitted, "a created actor is outfitted at once");
}

/// IsWeaponOut of an actor the engine does not draw follows the rule the engine draws by;
/// one it draws reads what it told.
#[test]
fn is_weapon_out_answers_for_undrawn_actors() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(call(&mut g, 0x1065, Some(ADULT_REF), &[]), 0.0);
    call(&mut g, 0x105A, Some(ADULT_REF), &[n(1.0)]);
    assert_eq!(call(&mut g, 0x1065, Some(ADULT_REF), &[]), 1.0);
    // Drawn: the engine's word (still holstering) wins while it keeps telling.
    g.set_weapon_out(FormId(ADULT_REF), false);
    assert_eq!(call(&mut g, 0x1065, Some(ADULT_REF), &[]), 0.0);
    g.time += 2.0;
    assert_eq!(call(&mut g, 0x1065, Some(ADULT_REF), &[]), 1.0);
    // The dead hold nothing out.
    call(&mut g, 0x108B, Some(ADULT_REF), &[]);
    assert_eq!(call(&mut g, 0x1065, Some(ADULT_REF), &[]), 0.0);
}

/// The second audit round (docs/design/script-function-coverage.md §4): what the function
/// probes of 2026-10-17 observed in the original game.
#[test]
fn function_probe_round_two_fixes() {
    let mut f = fixture();
    // The adult's record: Disposition Base 35 (as every Goodsprings resident); the
    // activator stands in for a terminal of difficulty Easy (code 1).
    f.data.actors.get_mut(&FormId(NPC_ADULT)).unwrap().disposition_base = 35;
    f.data.terminals.insert(FormId(ACTI), fnv_game::data::TerminalDef { difficulty: 1, ..Default::default() });
    let mut g = game(&f);
    let av = |c: u16| ArgValue::Enum(c);
    let cha = fnv_game::actor_values::CHARISMA;
    // GetDisposition (towards the child): base + 10 x (target's Charisma - 5), within 0..100; SetDisposition wins.
    assert_eq!(call(&mut g, 0x104C, Some(ADULT_REF), &[r(CHILD_REF)]), 25.0);
    call(&mut g, 0x100F, Some(CHILD_REF), &[av(cha), n(5.0)]);
    assert_eq!(call(&mut g, 0x104C, Some(ADULT_REF), &[r(CHILD_REF)]), 35.0);
    call(&mut g, 0x100F, Some(CHILD_REF), &[av(cha), n(10.0)]);
    assert_eq!(call(&mut g, 0x104C, Some(ADULT_REF), &[r(CHILD_REF)]), 85.0);
    call(&mut g, 0x100F, Some(CHILD_REF), &[av(cha), n(1.0)]);
    assert_eq!(call(&mut g, 0x104C, Some(ADULT_REF), &[r(CHILD_REF)]), 0.0);
    call(&mut g, 0x1184, Some(ADULT_REF), &[r(CHILD_REF), n(20.0)]);
    assert_eq!(call(&mut g, 0x104C, Some(ADULT_REF), &[r(CHILD_REF)]), 20.0);
    // A terminal: nothing while its cell is not loaded; its difficulty once it is; Lock
    // 50 gives 2 and 5, Unlock 0 and -1, and the terminal then opens at its menu.
    let (get_locked, get_level) = (0x1005, 0x1041);
    g.set_loaded(Default::default());
    assert_eq!((call(&mut g, get_locked, Some(ACTI_REF), &[]), call(&mut g, get_level, Some(ACTI_REF), &[])), (0.0, 0.0));
    g.set_loaded([FormId(CELL)].into_iter().collect());
    assert_eq!((call(&mut g, get_locked, Some(ACTI_REF), &[]), call(&mut g, get_level, Some(ACTI_REF), &[])), (1.0, 1.0));
    call(&mut g, 0x1072, Some(ACTI_REF), &[n(50.0)]);
    assert_eq!((call(&mut g, get_locked, Some(ACTI_REF), &[]), call(&mut g, get_level, Some(ACTI_REF), &[])), (2.0, 5.0));
    call(&mut g, 0x1073, Some(ACTI_REF), &[]);
    assert_eq!((call(&mut g, get_locked, Some(ACTI_REF), &[]), call(&mut g, get_level, Some(ACTI_REF), &[])), (0.0, -1.0));
    assert_eq!(fnv_game::terminal::opening(&mut g, FormId(ACTI_REF), FormId(ACTI), 0.0), Some(fnv_game::terminal::Opening::Menu));
    // The terminal's own lock, not the world's: a container keeps its lock data.
    assert!(g.world.get(FormId(ACTI_REF)).unwrap().lock.is_none());
    // IsFacingUp / IsLeftUp read 1 on a standing actor.
    assert_eq!(call(&mut g, 0x106A, Some(ADULT_REF), &[]), 1.0);
    assert_eq!(call(&mut g, 0x111D, Some(CREA_REF), &[]), 1.0);
    // GetDayOfWeek follows GameDaysPassed, not the calendar globals: day 5 is a Friday
    // whatever the date says; day 13 a Saturday.
    g.world.set_global(FormId(G_YEAR), 2284.0).unwrap();
    assert_eq!(call(&mut g, 0x10AA, None, &[]), 5.0);
    g.world.set_global(FormId(0x805), 13.4).unwrap();
    assert_eq!(call(&mut g, 0x10AA, None, &[]), 6.0);
    // GetPCMiscStat "Locations Discovered" (code 1) counts the markers found.
    g.set_found_markers([FormId(ACTI_REF), FormId(CONT_REF)]);
    assert_eq!(call(&mut g, 0x1138, None, &[ArgValue::Enum(1)]), 2.0);
}

#[test]
fn the_player_detects_and_far_pairs_fight() {
    use fnv_game::awareness::State;
    use fnv_game::detection::{Settings, Situation};
    let f = fixture();
    let mut g = game(&f);
    let s = Settings::default();
    let (get_detected, get_los, in_combat) = (0x102D, 0x101B, 0x1121);
    // The player as the detector: what the engine measured from the camera, read by
    // player.GetDetected / player.GetLineOfSight [observed: Ringo and Chet read 1 in the
    // original, function probes fp1/fp2 2026-10-17].
    let seen = Situation { distance: 300.0, exterior: true, line_of_sight: true, in_view: true, light: 80.0, ..Default::default() };
    g.detect(&s, FormId(PLAYER), FormId(ADULT_REF), &seen);
    assert_eq!(call(&mut g, get_detected, Some(PLAYER), &[r(ADULT_REF)]), 1.0);
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 1.0);
    // Behind the camera: out of the field of view, no line of sight.
    g.detect(&s, FormId(PLAYER), FormId(ADULT_REF), &Situation { in_view: false, ..seen });
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 0.0);
    // The player's detection is the body's view, GetLineOfSight the camera's [observed:
    // capture detection-2, third person]: detected while the camera does not see it.
    g.detect(&s, FormId(PLAYER), FormId(ADULT_REF), &Situation { camera_sight: Some(false), ..seen });
    assert_eq!(call(&mut g, get_detected, Some(PLAYER), &[r(ADULT_REF)]), 1.0);
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 0.0);
    g.detect(&s, FormId(PLAYER), FormId(ADULT_REF), &Situation { in_view: false, camera_sight: Some(true), ..seen });
    assert_eq!(call(&mut g, get_detected, Some(PLAYER), &[r(ADULT_REF)]), 0.0);
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 1.0);
    // Nothing in the rules asks where the player is: a Frenzied actor that detects
    // another (the engine measures pairs far from the player too, `fnv_game::sight`)
    // goes to Combat, and IsInCombat reads 1.
    call(&mut g, 0x100F, Some(ADULT_REF), &[ArgValue::Enum(fnv_game::actor_values::AGGRESSION), n(3.0)]);
    assert_eq!(call(&mut g, in_combat, Some(ADULT_REF), &[]), 0.0);
    let (det, _) = g.detect(&s, FormId(ADULT_REF), FormId(CREA_REF), &seen);
    g.observe_threat(FormId(ADULT_REF), FormId(CREA_REF), &det, [8000.0, 300.0, 0.0]);
    g.settle_awareness();
    assert_eq!(g.awareness.state(FormId(ADULT_REF)), State::Combat);
    assert_eq!(call(&mut g, in_combat, Some(ADULT_REF), &[]), 1.0);
}

/// #3232: a dead actor is in no detection pair, so player.GetLineOfSight on a body read
/// 0 and TTW's "Those!" (MS04Script: `Player.GetLOS FredWilksRef == 1` with Fred Wilks
/// lying dead) never advanced. The engine tells which bodies the camera sees
/// (`Game::set_camera_sights`); the player reads those, nobody else does.
#[test]
fn the_player_sees_a_body_in_the_cameras_view() {
    use fnv_game::detection::{Settings, Situation};
    let f = fixture();
    let mut g = game(&f);
    g.set_loaded([FormId(CELL)].into_iter().collect());
    let get_los = 0x101B;
    // Seen while alive (a detection pair, kept up to a second), then killed: the pair
    // measured alive is no longer read; the camera's word on the body is.
    let seen = Situation { distance: 300.0, exterior: true, line_of_sight: true, in_view: true, light: 80.0, ..Default::default() };
    g.detect(&Settings::default(), FormId(PLAYER), FormId(ADULT_REF), &seen);
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 1.0);
    call(&mut g, 0x108B, Some(ADULT_REF), &[r(PLAYER)]);
    assert!(g.is_dead(FormId(ADULT_REF)));
    assert!(g.detection.get(FormId(PLAYER), FormId(ADULT_REF)).is_some_and(|d| d.seen));
    g.set_camera_sights([]);
    // Dead and not measured: 0.
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 0.0);
    // In the camera's view with nothing between: 1, for the player only.
    g.set_camera_sights([FormId(ADULT_REF)]);
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 1.0);
    assert_eq!(call(&mut g, get_los, Some(CHILD_REF), &[r(ADULT_REF)]), 0.0);
    // Behind a wall or behind the camera the engine does not name it: 0 again.
    g.set_camera_sights([]);
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 0.0);
    // A word not renewed goes stale (the engine stopped measuring: a load, a menu).
    g.set_camera_sights([FormId(ADULT_REF)]);
    g.time += fnv_game::functions::senses::CAMERA_SIGHT_SECONDS + 0.5;
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 0.0);
    // Disabled after the measurement, in the same frame: not seen [documented: GECK wiki
    // GetLineOfSight]; enabled again, seen; its cell unloaded, not seen.
    g.set_camera_sights([FormId(ADULT_REF)]);
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 1.0);
    call(&mut g, 0x1022, Some(ADULT_REF), &[]);
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 0.0);
    call(&mut g, 0x1021, Some(ADULT_REF), &[]);
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 1.0);
    g.set_loaded(Default::default());
    assert_eq!(call(&mut g, get_los, Some(PLAYER), &[r(ADULT_REF)]), 0.0);
}

#[test]
fn show_repair_menu_opens_the_vendors_repair_services() {
    use fnv_game::repair::{RepairRequest, REPAIR_SERVICES_MENU};
    let f = fixture();
    let mut g = game(&f);
    // Without a calling reference nothing happens [documented: GECK wiki ShowRepairMenu].
    call(&mut g, 0x117A, None, &[]);
    assert!(g.repair_requests.is_empty());
    assert_eq!(g.menu, None);
    // On an actor: that actor's skill and purse, the menu open for scripts.
    call(&mut g, 0x117A, Some(ADULT_REF), &[]);
    assert_eq!(g.repair_requests, vec![RepairRequest { vendor: FormId(ADULT_REF) }]);
    assert_eq!(g.menu, Some(REPAIR_SERVICES_MENU));
}

/// What the engine tells of the actors' bodies (`fnv_game::anim_state`): GetAnimAction,
/// IsMoving's way, IsAnimPlaying of an actor's movement group, IsIdlePlaying on
/// furniture, IsPlayerActionActive 9 (row 4 of docs/outstanding.md §1).
#[test]
fn animation_state_told_by_the_engine() {
    use fnv_game::anim_state::{AnimAction, AnimState, MoveDir};
    use fnv_game::game::FurnitureUse;
    let f = fixture();
    let mut g = game(&f);
    let adult = FormId(ADULT_REF);
    let group = |g: &Game, name: &str| ArgValue::Enum(fnv_game::functions::console::enum_code(g, "anim_group", name).unwrap());
    // Nothing told: none, still, nothing playing.
    assert_eq!(call(&mut g, 0x10DB, Some(ADULT_REF), &[]), -1.0);
    assert_eq!(call(&mut g, 0x1019, Some(ADULT_REF), &[]), 0.0);
    assert_eq!(call(&mut g, 0x1128, Some(ADULT_REF), &[]), 0.0);
    // A reload (9), walking backward, the Backward group playing.
    g.set_anim_state(adult, AnimState { action: Some(AnimAction::Reload), moving: MoveDir::Backward, group: Some("Backward"), ..Default::default() });
    assert_eq!(call(&mut g, 0x10DB, Some(ADULT_REF), &[]), 9.0);
    assert_eq!(call(&mut g, 0x1019, Some(ADULT_REF), &[]), 2.0);
    assert_eq!(call(&mut g, 0x1128, Some(ADULT_REF), &[]), 1.0);
    let backward = group(&g, "Backward");
    let idle = group(&g, "Idle");
    assert_eq!(call(&mut g, 0x1128, Some(ADULT_REF), &[backward]), 1.0);
    assert_eq!(call(&mut g, 0x1128, Some(ADULT_REF), std::slice::from_ref(&idle)), 0.0);
    // Standing: Idle plays (observed on Sunny Smiles); a shot's follow-through is 3.
    g.set_anim_state(adult, AnimState { action: Some(AnimAction::FollowThrough), group: Some("Idle"), ..Default::default() });
    assert_eq!(call(&mut g, 0x1128, Some(ADULT_REF), &[idle]), 1.0);
    assert_eq!(call(&mut g, 0x10DB, Some(ADULT_REF), &[]), 3.0);
    assert_eq!(call(&mut g, 0x1019, Some(ADULT_REF), &[]), 0.0);
    // It holds however long a menu keeps the engine from renewing it; an actor the
    // engine forgets (it left the view) reads as nothing.
    g.time += 30.0;
    assert_eq!(call(&mut g, 0x1128, Some(ADULT_REF), &[]), 1.0);
    g.forget_anim_state(adult);
    assert_eq!(call(&mut g, 0x10DB, Some(ADULT_REF), &[]), -1.0);
    assert_eq!(call(&mut g, 0x1128, Some(ADULT_REF), &[]), 0.0);
    // The player strafing left; aiming down the sights is action 9, other indices 0.
    g.set_anim_state(FormId::PLAYER_REF, AnimState { moving: MoveDir::Left, group: Some("FastLeft"), iron_sights: true, ..Default::default() });
    assert_eq!(call(&mut g, 0x1019, None, &[]), 3.0);
    assert_eq!(call(&mut g, 0x1170, None, &[n(9.0)]), 1.0);
    assert_eq!(call(&mut g, 0x1170, None, &[n(8.0)]), 0.0);
    g.set_anim_state(FormId::PLAYER_REF, AnimState::default());
    assert_eq!(call(&mut g, 0x1170, None, &[n(9.0)]), 0.0);
    // A grenade thrown: action 2 (observed), and not 9.
    g.set_anim_state(FormId::PLAYER_REF, AnimState { throwing: true, ..Default::default() });
    assert_eq!(call(&mut g, 0x1170, None, &[n(2.0)]), 1.0);
    assert_eq!(call(&mut g, 0x1170, None, &[n(4.0)]), 0.0);
    assert_eq!(call(&mut g, 0x1170, None, &[n(9.0)]), 0.0);
    // A mine laid: action 4 (observed), not 2.
    g.set_anim_state(FormId::PLAYER_REF, AnimState { throwing: true, mine: true, ..Default::default() });
    assert_eq!(call(&mut g, 0x1170, None, &[n(4.0)]), 1.0);
    assert_eq!(call(&mut g, 0x1170, None, &[n(2.0)]), 0.0);
    g.set_anim_state(FormId::PLAYER_REF, AnimState::default());
    // Seated on furniture: an idle plays (observed on Easy Pete); getting on does not.
    assert_eq!(call(&mut g, 0x1070, Some(ADULT_REF), &[]), 0.0);
    g.set_furniture_use(adult, Some(FurnitureUse { reference: FormId(CONT_REF), marker_id: 11, state: 2 }));
    assert_eq!(call(&mut g, 0x1070, Some(ADULT_REF), &[]), 0.0);
    g.set_furniture_use(adult, Some(FurnitureUse { reference: FormId(CONT_REF), marker_id: 11, state: 3 }));
    assert_eq!(call(&mut g, 0x1070, Some(ADULT_REF), &[]), 1.0);
}

/// The miscellaneous statistics the rules count (docs/spec-notes/misc-stats.md), and
/// SetScale kept on the reference's state.
#[test]
fn misc_stats_count_and_set_scale_is_kept_on_the_reference() {
    use fnv_game::misc_stats::{code, CREATURES_KILLED, PEOPLE_KILLED, TOTAL_THINGS_KILLED};
    let f = fixture();
    let mut g = game(&f);
    let stat = |g: &mut Game, c: u16| call(g, 0x1138, None, &[ArgValue::Enum(c)]);
    // Every New Vegas name compiles to its code (the console's table).
    let names = &g.data.table.enums["misc_stat"];
    assert_eq!(names.len(), 43);
    assert_eq!(names[&35], "Total Things Killed");
    assert_eq!(code("Total Things Killed"), Some(35));
    // The player kills a person and a creature; a kill by another counts nothing.
    call(&mut g, 0x108B, Some(ADULT_REF), &[r(PLAYER)]);
    call(&mut g, 0x108B, Some(CREA_REF), &[r(PLAYER)]);
    assert_eq!((stat(&mut g, PEOPLE_KILLED), stat(&mut g, CREATURES_KILLED), stat(&mut g, TOTAL_THINGS_KILLED)), (1.0, 1.0, 2.0));
    // ModPCMiscStat adds to the same counter.
    call(&mut g, 0x1137, None, &[ArgValue::Enum(PEOPLE_KILLED), n(2.0)]);
    assert_eq!(stat(&mut g, PEOPLE_KILLED), 3.0);
    // SetScale: kept on the reference's state (saved with it), read by GetScale, and the
    // reference is among this frame's redrawn ones; a Scaled event tells the engine.
    assert_eq!(call(&mut g, 0x1018, Some(ACTI_REF), &[]), 1.0);
    call(&mut g, 0x113C, Some(ACTI_REF), &[n(0.25)]);
    assert_eq!(g.world.peek(FormId(ACTI_REF)).unwrap().scale, Some(0.25));
    assert_eq!(call(&mut g, 0x1018, Some(ACTI_REF), &[]), 0.25);
    assert!(g.turned.contains(&FormId(ACTI_REF)));
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::Scaled { reference, scale } if reference == FormId(ACTI_REF) && scale == 0.25)));
}

/// The parameters scripts pass that were ignored until 2026-10-18
/// (docs/design/script-function-params.md §3).
#[test]
fn ignored_parameters_are_honoured() {
    use fnv_game::chargen::{SpecialSheet, TagSheet};
    use fnv_sim::world::{EquipLock, SimError};
    let f = fixture();
    let mut g = game(&f);
    let p = FormId::PLAYER_REF;
    let fades = |g: &Game| -> Vec<(bool, bool)> {
        g.events
            .iter()
            .filter_map(|e| match e.kind {
                EventKind::RefEnabled { fade, .. } => Some((true, fade)),
                EventKind::RefDisabled { fade, .. } => Some((false, fade)),
                _ => None,
            })
            .collect()
    };
    // Enable fades in by default, Disable does not; each flag turns it round.
    for (op, arg) in [(0x1022, None), (0x1021, None), (0x1022, Some(1.0)), (0x1021, Some(0.0))] {
        let args: Vec<ArgValue> = arg.map(n).into_iter().collect();
        call(&mut g, op, Some(ACTI_REF), &args);
    }
    assert_eq!(fades(&g), vec![(false, false), (true, true), (false, true), (true, false)]);

    // Disable 1 holds the reference enabled while it fades (GetDisabled 0, observed), then
    // disables it for the engine to take; an Enable during the hold ends it.
    let acti = FormId(ACTI_REF);
    call(&mut g, 0x1022, Some(ACTI_REF), &[n(1.0)]);
    assert_eq!(call(&mut g, 0x1023, Some(ACTI_REF), &[]), 0.0, "enabled while it fades out");
    g.frames(60);
    assert_eq!(call(&mut g, 0x1023, Some(ACTI_REF), &[]), 0.0, "still held after 1 s");
    let frames = (fnv_game::functions::FADE_OUT_HOLD / g.cfg.frame_seconds).ceil() as u64;
    g.frames(frames);
    assert_eq!(call(&mut g, 0x1023, Some(ACTI_REF), &[]), 1.0, "disabled once the hold ends");
    assert!(g.script_looks.disabled_after_fade.contains(&acti));
    call(&mut g, 0x1021, Some(ACTI_REF), &[n(0.0)]);
    call(&mut g, 0x1022, Some(ACTI_REF), &[n(1.0)]);
    call(&mut g, 0x1021, Some(ACTI_REF), &[]);
    g.frames(frames + 10);
    assert_eq!(call(&mut g, 0x1023, Some(ACTI_REF), &[]), 0.0, "an Enable ended the hold");
    assert!(g.script_looks.fading_out.is_empty());
    // A plain Disable during a hold disables at once.
    call(&mut g, 0x1022, Some(ACTI_REF), &[n(1.0)]);
    call(&mut g, 0x1022, Some(ACTI_REF), &[]);
    assert_eq!(call(&mut g, 0x1023, Some(ACTI_REF), &[]), 1.0);
    call(&mut g, 0x1021, Some(ACTI_REF), &[n(0.0)]);
    // A save during a hold: the reference is disabled on load (#1985); a save outside
    // one writes nothing for it.
    assert!(!serde_json::to_string(&g.snapshot()).unwrap().contains("fading_out"));
    call(&mut g, 0x1022, Some(ACTI_REF), &[n(1.0)]);
    let held = g.snapshot();
    assert_eq!(held.fading_out, [acti].into());
    assert!(held.sim.refs.get(&acti).is_none_or(|r| r.enabled()), "still enabled in the save itself");
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&held).unwrap()).unwrap();
    let mut h = Game::restore(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default(), &back).unwrap();
    assert_eq!(call(&mut h, 0x1023, Some(ACTI_REF), &[]), 1.0, "disabled after the load");
    call(&mut g, 0x1021, Some(ACTI_REF), &[n(0.0)]);

    // EquipItem's NoUnequipFlag: the inventory cannot take it off; UnequipItem can, and
    // its NoEquipFlag keeps it off until EquipItem.
    g.world.add_item(p, FormId(WEAP), 1).unwrap();
    call(&mut g, 0x10EE, Some(PLAYER), &[r(WEAP), n(1.0)]);
    assert_eq!(g.world.equip_lock(p, FormId(WEAP)), Some(EquipLock::NoUnequip));
    assert!(matches!(g.world.unequip(p, FormId(WEAP)), Err(SimError::EquipLocked { on: true, .. })));
    call(&mut g, 0x10EF, Some(PLAYER), &[r(WEAP), n(1.0), n(1.0)]);
    assert_eq!(g.world.equip_lock(p, FormId(WEAP)), Some(EquipLock::NoEquip));
    assert!(!g.world.peek(p).unwrap().inventory.as_ref().unwrap().is_equipped(FormId(WEAP)));
    assert!(matches!(g.world.equip(p, FormId(WEAP)), Err(SimError::EquipLocked { on: false, .. })));
    call(&mut g, 0x10EE, Some(PLAYER), &[r(WEAP)]);
    assert_eq!(g.world.equip_lock(p, FormId(WEAP)), None, "a plain EquipItem frees it");
    assert!(g.world.unequip(p, FormId(WEAP)).unwrap());

    // SetPlayerTagSkill: four slots; a slot's old skill is untagged; above 3 nothing.
    call(&mut g, 0x11CD, None, &[n(41.0), n(2.0)]);
    call(&mut g, 0x11CD, None, &[n(43.0), n(2.0)]);
    call(&mut g, 0x11CD, None, &[n(44.0), n(4.0)]);
    assert_eq!(g.player.tag_skills.iter().copied().collect::<Vec<_>>(), vec![43]);
    // SetTagSkills: the count in all; with 0 the tags before are not shown.
    call(&mut g, 0x121A, None, &[n(4.0)]);
    let t = TagSheet::for_menu(&mut g);
    assert_eq!((t.count, t.remaining(), t.locked.len()), (4, 3, 0));
    call(&mut g, 0x121A, None, &[n(3.0), n(0.0)]);
    let t = TagSheet::for_menu(&mut g);
    assert_eq!((t.count, t.tagged.len()), (3, 0));
    t.apply(&mut g);
    assert!(g.player.tag_slots.is_empty(), "the menu's choice replaces the slots");
    // ShowLoveTesterMenuParams: the attributes add up to the points.
    call(&mut g, 0x1245, None, &[n(33.0)]);
    assert_eq!(SpecialSheet::for_menu(&mut g).total, 33);

    // SetGlobalTimeMultiplier: the sound flag is kept for the engine.
    call(&mut g, 0x1186, None, &[n(0.5)]);
    assert!(!g.ui.time_keeps_sound);
    call(&mut g, 0x1186, None, &[n(0.5), n(1.0)]);
    assert!(g.ui.time_keeps_sound);
}

/// The console's way in (`call_words`, as `GSChetRef.IsAnimPlaying Idle` in the smoke
/// test `anim-state`) reads the same snapshot.
#[test]
fn animation_state_through_the_console() {
    use fnv_game::anim_state::{AnimAction, AnimState};
    let f = fixture();
    let mut g = game(&f);
    let adult = FormId(ADULT_REF);
    g.set_anim_state(adult, AnimState { action: Some(AnimAction::Reload), group: Some("Idle"), ..Default::default() });
    let c = fnv_game::functions::console::call_words(&mut g, Some(adult), "IsAnimPlaying", &["Idle".to_string()], "console").unwrap();
    assert_eq!(c.value, Value::Num(1.0), "{:?}", c.stub);
    let c = fnv_game::functions::console::call_words(&mut g, Some(adult), "GetAnimAction", &[], "console").unwrap();
    assert_eq!(c.value, Value::Num(9.0));
}

/// SetCriticalStage: GooEnd and DisintegrateEnd cull the reference (not drawn, no
/// collision) though it stays enabled; the engine is told when that turns on or off.
#[test]
fn critical_end_stages_cull_the_reference() {
    const SET: u16 = 0x1212;
    const IS_IN: u16 = 0x1213;
    let f = fixture();
    let mut g = game(&f);
    let on = FormId(ADULT_REF);
    call(&mut g, SET, Some(ADULT_REF), &[ArgValue::Enum(1)]);
    assert!(!g.culled(on), "GooStart only changes the ragdoll");
    assert!(g.script_looks.critical_changed.is_empty());
    call(&mut g, SET, Some(ADULT_REF), &[ArgValue::Enum(fnv_game::functions::CRITICAL_GOO_END)]);
    assert!(g.culled(on));
    assert_eq!(call(&mut g, IS_IN, Some(ADULT_REF), &[ArgValue::Enum(2)]), 1.0);
    assert_eq!(call(&mut g, 0x1023, Some(ADULT_REF), &[]), 0.0, "still enabled");
    call(&mut g, SET, Some(ADULT_REF), &[ArgValue::Enum(fnv_game::functions::CRITICAL_DISINTEGRATE_END)]);
    assert!(g.culled(on));
    assert_eq!(g.script_looks.critical_changed, vec![on], "still culled: no second notice");
    call(&mut g, SET, Some(ADULT_REF), &[ArgValue::Enum(0)]);
    assert!(!g.culled(on));
    assert_eq!(g.script_looks.critical_changed, vec![on, on]);
}

/// The player's X angle is its view's pitch as the engine writes it (GetAngle X), and
/// what is placed at or moved to the player stands upright.
#[test]
fn player_x_angle_is_the_view_pitch() {
    let f = fixture();
    let mut g = game(&f);
    g.world.get(FormId::PLAYER_REF).unwrap().rot = [45f32.to_radians(), 0.0, 90f32.to_radians()];
    let x = call(&mut g, 0x1008, Some(PLAYER), &[ArgValue::Axis('X')]);
    assert!((x - 45.0).abs() < 1e-3, "{x}");
    call(&mut g, 0x109E, Some(ACTI_REF), &[r(PLAYER)]);
    let rot = g.world.peek(FormId(ACTI_REF)).unwrap().rot;
    assert_eq!(rot[0], 0.0);
    assert!((rot[2] - 90f32.to_radians()).abs() < 1e-6);
}

/// MoveToMarkerWithFade: the reference stays while it fades out, then moves and fades in;
/// a save during the wait moves it on load.
#[test]
fn move_to_marker_with_fade_waits_for_the_fade() {
    const FADE_MOVE: u16 = 0x124F;
    let f = fixture();
    let mut g = game(&f);
    let on = FormId(ADULT_REF);
    let pos = |g: &mut Game| g.world.peek(on).unwrap().pos;
    call(&mut g, FADE_MOVE, Some(ADULT_REF), &[r(CONT_REF), n(100.0), n(0.0), n(0.0)]);
    assert_eq!(pos(&mut g), [0.0; 3], "not moved before the fade out");
    assert_eq!(g.script_looks.fades, vec![(on, false)]);
    let snap = g.snapshot();
    assert_eq!(snap.fade_moves.len(), 1);
    // A second call changes where it goes, not the wait.
    g.frames(30);
    call(&mut g, FADE_MOVE, Some(ADULT_REF), &[r(CONT_REF), n(200.0), n(0.0), n(0.0)]);
    assert_eq!(g.script_looks.fades, vec![(on, false)]);
    let frames = (fnv_game::functions::FADE_OUT_HOLD / g.cfg.frame_seconds).ceil() as u64;
    g.frames(frames - 30 - 2);
    assert_eq!(pos(&mut g), [0.0; 3]);
    g.frames(4);
    assert_eq!(pos(&mut g), [200.0, 0.0, 0.0]);
    assert_eq!(g.script_looks.fades, vec![(on, false), (on, true)]);
    assert!(g.script_looks.fade_moves.is_empty());
    // The player moves at once.
    call(&mut g, FADE_MOVE, Some(PLAYER), &[r(CONT_REF), n(0.0), n(50.0), n(0.0)]);
    assert_eq!(g.world.peek(FormId::PLAYER_REF).unwrap().pos, [0.0, 50.0, 0.0]);
    // The saved wait is made on load.
    let json = serde_json::to_string(&snap).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&json).unwrap();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let h = Game::restore(&f.data, world, GameConfig::default(), &back).unwrap();
    assert_eq!(h.world.peek(on).unwrap().pos, [100.0, 0.0, 0.0]);
}

/// SetSecuritronExpression's face is saved and put back on a load.
#[test]
fn securitron_expression_survives_a_load() {
    let f = fixture();
    let mut g = game(&f);
    call(&mut g, 0x1248, None, &[r(ADULT_REF), s("Victor"), s("Alert")]);
    let snap = g.snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&json).unwrap();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let h = Game::restore(&f.data, world, GameConfig::default(), &back).unwrap();
    assert_eq!(
        h.texture_swaps.by_ref[&FormId(ADULT_REF)],
        vec![("screen01:0".to_string(), r"textures\creatures\securitron\victor_alert.dds".to_string())]
    );
    assert!(h.texture_swaps.generation > 0, "the engine sees the restored swap");
    // An old save without the field still loads.
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v.as_object_mut().unwrap().remove("securitron_faces");
    let old: fnv_game::game::GameSnapshot = serde_json::from_value(v).unwrap();
    assert!(old.securitron_faces.is_empty());
}

/// #2797: a load stands every actor up, NPCs as the player (#1573): the saved furniture
/// use is cleared, so GetSitting reads 0 and the seat is free until the engine seats the
/// body again; the rest of the actor's saved state stays.
#[test]
fn a_load_clears_every_actors_furniture_use() {
    use fnv_game::game::FurnitureUse;
    let f = fixture();
    let mut g = game(&f);
    let (npc, chair) = (FormId(ADULT_REF), FormId(CONT_REF));
    let seated = FurnitureUse { reference: chair, marker_id: 11, state: 3 };
    g.set_furniture_use(npc, Some(seated));
    g.set_furniture_use(FormId::PLAYER_REF, Some(FurnitureUse { reference: FormId(CREA_REF), marker_id: 12, state: 3 }));
    g.extra.get_mut(&npc).unwrap().ai_off = true;
    assert_eq!(call(&mut g, 0x109F, Some(ADULT_REF), &[]), 3.0, "GetSitting before the save");
    let snap = g.snapshot();
    assert_eq!(snap.extra[&npc].furniture, Some(seated), "the save itself is unchanged");
    let json = serde_json::to_string(&snap).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&json).unwrap();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut h = Game::restore(&f.data, world, GameConfig::default(), &back).unwrap();
    assert_eq!(h.furniture_use(npc), None, "the NPC stands after the load");
    assert_eq!(h.furniture_use(FormId::PLAYER_REF), None, "and the player too");
    assert_eq!(call(&mut h, 0x109F, Some(ADULT_REF), &[]), 0.0, "GetSitting after the load");
    assert!(!h.furniture_used_by_other(FormId::PLAYER_REF, chair), "the chair is free");
    assert!(h.extra[&npc].ai_off, "the actor's other saved state stays");
    // #3991: the NPC's seat is kept for the engine to put it back on at once; the
    // player's is not (#1573), nor a seat being got on or off.
    assert_eq!(h.take_saved_seats().into_iter().collect::<Vec<_>>(), vec![(npc, chair)]);
    assert!(h.take_saved_seats().is_empty(), "taken once");
    // The engine seats it again and says so: GetSitting follows.
    h.set_furniture_use(npc, Some(seated));
    assert_eq!(call(&mut h, 0x109F, Some(ADULT_REF), &[]), 3.0);
}

/// Kill challenges count the player's kills by the record's filters, complete at the
/// threshold and queue their notices (docs/spec-notes/challenges.md).
#[test]
fn kill_challenges_count_the_players_kills() {
    use fnv_game::challenges::{kind, Notice};
    let f = fixture();
    let mut g = game(&f);
    let d = &g.data.challenge_defs[&FormId(CHAL_ROBOTS)];
    assert_eq!((d.kind, d.threshold, d.interval, d.value2), (kind::KILL_CATEGORY, 2, 1, 7));
    assert_eq!(d.name.as_deref(), Some("Robot Killer"));
    // A person killed is no robot; the robot is.
    call(&mut g, 0x108B, Some(ADULT_REF), &[r(PLAYER)]);
    assert!(!g.player.challenges.contains_key(&FormId(CHAL_ROBOTS)));
    call(&mut g, 0x108B, Some(CREA_REF), &[r(PLAYER)]);
    assert_eq!(g.player.challenges[&FormId(CHAL_ROBOTS)].progress, 1);
    assert_eq!(g.take_challenge_notices(), vec![Notice::Progress { challenge: FormId(CHAL_ROBOTS), progress: 1, threshold: 2 }]);
    // The second robot completes it: counted in Challenges Completed, not recurring.
    g.challenge_step(FormId(CHAL_ROBOTS), 1.0);
    let s = &g.player.challenges[&FormId(CHAL_ROBOTS)];
    assert!(s.completed);
    assert_eq!(s.progress, 2);
    assert_eq!(g.take_challenge_notices(), vec![Notice::Completed { challenge: FormId(CHAL_ROBOTS) }]);
    assert_eq!(g.misc_stat(fnv_game::misc_stats::CHALLENGES_COMPLETED), 1.0);
    g.challenge_step(FormId(CHAL_ROBOTS), 1.0);
    assert_eq!(g.player.challenges[&FormId(CHAL_ROBOTS)].progress, 2);
}

/// The last stubs (script-stubs.md §4): GetForceHitReaction from the engine's body state,
/// GetVATSTargetHeight from the shot leaving, SetVatsTarget over the base's DEST flag,
/// IsWaterObject from what the actor activates.
#[test]
fn forced_reactions_vats_targets_and_water_objects() {
    use fnv_game::anim_state::AnimState;
    use fnv_game::vats::{VatsShot, MODE_PLAYBACK};
    let f = fixture();
    let mut g = game(&f);
    let adult = FormId(ADULT_REF);
    // GetForceHitReaction: 1 only while the engine says a forced reaction plays.
    assert_eq!(call(&mut g, 0x1259, Some(ADULT_REF), &[]), 0.0);
    g.set_anim_state(adult, AnimState { forced_hit: true, ..Default::default() });
    assert_eq!(call(&mut g, 0x1259, Some(ADULT_REF), &[]), 1.0);
    g.set_anim_state(adult, AnimState::default());
    assert_eq!(call(&mut g, 0x1259, Some(ADULT_REF), &[]), 0.0);
    // GetVATSTargetHeight: the leaving shot's height, 0 with none.
    assert_eq!(call(&mut g, 0x10EB, None, &[]), 0.0);
    g.set_vats(MODE_PLAYBACK, Some(VatsShot { weapon: FormId(WEAP), target: adult, distance: 300.0, condition: None, success: true, height: 42.5 }));
    assert_eq!(call(&mut g, 0x10EB, None, &[]), 42.5);
    g.set_vats(0, None);
    assert_eq!(call(&mut g, 0x10EB, None, &[]), 0.0);
    // SetVatsTarget: the crate's DEST is not flagged VATS Targetable; the script's
    // setting overrides it either way, and a destroyed object is never a target.
    let crate_ref = FormId(ACTI_REF);
    assert!(!g.vats_targetable(crate_ref));
    call(&mut g, 0x11EB, Some(ACTI_REF), &[n(1.0)]);
    assert!(g.vats_targetable(crate_ref));
    call(&mut g, 0x11EB, Some(ACTI_REF), &[n(0.0)]);
    assert!(!g.vats_targetable(crate_ref));
    call(&mut g, 0x11EB, Some(ACTI_REF), &[n(1.0)]);
    g.extra.entry(crate_ref).or_default().destroyed = true;
    assert!(!g.vats_targetable(crate_ref));
    // IsWaterObject: the crate names no water, so activating it is not drinking.
    assert_eq!(call(&mut g, 0x1130, Some(FormId::PLAYER_REF.0), &[]), 0.0);
    g.note_activating(FormId::PLAYER_REF, crate_ref);
    assert_eq!(g.activating(FormId::PLAYER_REF), Some(crate_ref));
    assert_eq!(call(&mut g, 0x1130, Some(FormId::PLAYER_REF.0), &[]), 0.0);
    g.time += fnv_game::functions::senses::ACTIVATING_SECONDS + 1.0;
    assert_eq!(g.activating(FormId::PLAYER_REF), None);
}

/// A call that succeeds although its function could not read an argument it passed is
/// traced as `ArgUnread` (#1122), once per opcode, unit and parameter; a failed call and
/// an absent argument are not.
#[test]
fn an_argument_read_wrong_without_failing_is_traced_once() {
    let f = fixture();
    let mut g = game(&f);
    let unread = |g: &Game| g.events.iter().filter(|e| matches!(e.kind, EventKind::ArgUnread { .. })).count();
    // ShowBarterMenu [discount]: a string where the number goes, the discount defaults to 0.
    call(&mut g, 0x1173, Some(ADULT_REF), &[ArgValue::Str("ten".into())]);
    let e = g.events.iter().find_map(|e| match &e.kind {
        EventKind::ArgUnread { name, param, got, wanted, .. } => Some((name.clone(), *param, got.clone(), wanted.clone())),
        _ => None,
    });
    assert_eq!(e, Some(("ShowBarterMenu".into(), 0, "string".into(), "number".into())));
    call(&mut g, 0x1173, Some(ADULT_REF), &[ArgValue::Str("ten".into())]);
    assert_eq!(unread(&g), 1, "traced once per opcode, unit and parameter");
    call(&mut g, 0x1173, Some(ADULT_REF), &[]);
    assert_eq!(unread(&g), 1, "an absent optional argument is not a misread");
    // No calling reference: the call fails, which CallFailed already reports.
    let ctx = Ctx { unit: "other".into(), opcode: 0x1173, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[ArgValue::Str("ten".into())]);
    assert_eq!(unread(&g), 1);
}

/// Functions only Fallout 3's scripts call (Tale of Two Wastelands; the TTW audit of
/// 2026-11-02 §1): ModDisposition, GetForceSneak, SetMerchantContainer, GetPCSleepHours
/// and the player's childhood flags.
#[test]
fn fallout_3_functions() {
    let mut f = fixture();
    // A merchant container must be persistent: the fixture's container is made so.
    f.refs.refs.get_mut(&FormId(CONT_REF)).unwrap().in_persistent_group = true;
    let mut g = game(&f);
    let (get_disp, set_disp, mod_disp) = (0x104C, 0x1184, 0x1053);
    // ModDisposition from SetDisposition's value, kept within 0..100.
    call(&mut g, set_disp, Some(ADULT_REF), &[r(CHILD_REF), n(20.0)]);
    call(&mut g, mod_disp, Some(ADULT_REF), &[r(CHILD_REF), n(15.0)]);
    assert_eq!(call(&mut g, get_disp, Some(ADULT_REF), &[r(CHILD_REF)]), 35.0);
    call(&mut g, mod_disp, Some(ADULT_REF), &[r(CHILD_REF), n(-80.0)]);
    assert_eq!(call(&mut g, get_disp, Some(ADULT_REF), &[r(CHILD_REF)]), 0.0);
    // GetForceSneak reads SetForceSneak.
    assert_eq!(call(&mut g, 0x10D2, Some(ADULT_REF), &[]), 0.0);
    call(&mut g, 0x10D3, Some(ADULT_REF), &[n(1.0)]);
    assert_eq!(call(&mut g, 0x10D2, Some(ADULT_REF), &[]), 1.0);
    // SetMerchantContainer: a persistent container stands for the reference's own.
    assert_eq!(g.merchant_container(FormId(ADULT_REF)), None);
    call(&mut g, 0x11B7, Some(ADULT_REF), &[r(CONT_REF)]);
    assert_eq!(g.merchant_container(FormId(ADULT_REF)), Some(FormId(CONT_REF)));
    let ctx = Ctx { unit: "test".into(), opcode: 0x11B7, on: Some(FormId(CHILD_REF)), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[r(ACTI_REF)]);
    assert_eq!(g.merchant_container(FormId(CHILD_REF)), None, "a temporary reference is refused");
    // GetPCSleepHours: the hours of a sleep, none for a wait.
    assert_eq!(call(&mut g, 0x10BC, None, &[]), 0.0);
    g.rest.begin(true, 8);
    assert_eq!(call(&mut g, 0x10BC, None, &[]), 8.0);
    g.rest.begin(false, 3);
    assert_eq!(call(&mut g, 0x10BC, None, &[]), 0.0);
    // The childhood flags.
    call(&mut g, 0x11E6, None, &[n(1.0)]);
    call(&mut g, 0x1202, None, &[n(1.0)]);
    call(&mut g, 0x1200, None, &[n(1.0)]);
    assert!(g.player.young && g.player.toddler && g.player.location_load_screens_only);
    call(&mut g, 0x1202, None, &[n(0.0)]);
    assert!(!g.player.toddler);
    // SetLastExtDoorActivated is what GetPCLastExteriorDoor reads.
    call(&mut g, 0x1230, None, &[r(ACTI_REF)]);
    assert_eq!(g.gaps.jip3.last_exterior_door, Some(FormId(ACTI_REF)));
}

/// The rest of the functions only Fallout 3's scripts call (the TTW audit of 2026-11-02
/// §1, after #1455): cells, water, factions, the screen and sound requests and
/// MatchFaceGeometry.
#[test]
fn fallout_3_functions_cells_screen_and_faces() {
    let f = fixture();
    let mut g = game(&f);
    let failed = |g: &mut Game, op: u16, args: &[ArgValue]| {
        let ctx = Ctx { unit: "test".into(), opcode: op, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        let before = g.events.len();
        g.call_function(&ctx, args);
        g.events[before..].iter().any(|e| matches!(e.kind, EventKind::CallFailed { .. }))
    };
    // SetCellFullName: an interior is renamed by the message's title; not an exterior.
    call(&mut g, 0x111B, None, &[r(CELL), r(MESG)]);
    assert_eq!(g.script_name(FormId(CELL)), Some("Renamed Cell"));
    assert!(failed(&mut g, 0x111B, &[r(EXT_CELL), r(MESG)]));
    assert_eq!(g.script_name(FormId(EXT_CELL)), None);
    // ResetInterior: kept for the next load; not an exterior.
    call(&mut g, 0x1127, None, &[r(CELL)]);
    assert!(g.forms.expired_cells.contains(&FormId(CELL)));
    assert!(failed(&mut g, 0x1127, &[r(EXT_CELL)]));
    // SetFactionReaction: the disposition modifier, not the combat reaction.
    call(&mut g, 0x10A8, None, &[r(F1), r(F2), n(100.0)]);
    assert_eq!(g.forms.faction_modifiers[&FormId(F1)][&FormId(F2)], 100);
    assert_eq!(call(&mut g, 0x119B, None, &[r(F1), r(F2)]), 1.0, "XNAM's combat reaction stays");
    // RemapWaterType, and back.
    call(&mut g, 0x1237, None, &[r(0x700), r(0x701)]);
    assert_eq!(g.water_type(FormId(0x700)), FormId(0x701));
    assert_eq!(g.water_type(FormId(0x702)), FormId(0x702));
    call(&mut g, 0x1237, None, &[r(0x700), r(0x700)]);
    assert_eq!(g.water_type(FormId(0x700)), FormId(0x700));
    // TriggerLODApocalypse, KillQuestUpdates, DisableAllMines.
    call(&mut g, 0x11F3, None, &[]);
    call(&mut g, 0x1231, None, &[]);
    call(&mut g, 0x122F, None, &[]);
    assert!(g.forms.lod_apocalypse);
    assert_eq!((g.ui.quest_updates_killed, g.ui.mines_disabled), (1, 1));
    // TriggerHitShader (strength 1 without one), FadeSFX, TriggerScreenBlood.
    call(&mut g, 0x1144, None, &[]);
    call(&mut g, 0x1144, None, &[n(4.0)]);
    call(&mut g, 0x1227, None, &[n(4.0)]);
    call(&mut g, 0x1205, None, &[n(10.0)]);
    assert_eq!(g.ui.hit_shaders, [1.0, 4.0]);
    assert_eq!(g.ui.sfx_fades, [4.0]);
    assert_eq!(g.ui.splatter.triggers.iter().map(|t| t.count).collect::<Vec<_>>(), [10]);
    // GetLastPlayerAction: nothing done.
    assert_eq!(call(&mut g, 0x116F, None, &[]), 0.0);
    // MatchFaceGeometry: the player's face is kept with the call; the actor is drawn anew.
    g.take_appearance_changes();
    let look = fnv_game::chargen::Look { race: FormId(RACE_CHILD), female: false, hair: None, hair_color: None, eyes: None, head_parts: vec![], fggs: vec![2.0; 50], fgga: vec![1.0; 30], fgts: vec![0.0; 50] };
    g.player.look = Some(look);
    call(&mut g, 0x11DF, Some(ADULT_REF), &[r(PLAYER), n(50.0)]);
    let m = &g.extra[&FormId(ADULT_REF)].face_matches;
    assert_eq!((m.len(), m[0].source, m[0].percent, m[0].fggs.len(), m[0].fgga[0]), (1, FormId(PLAYER), 50.0, 50, 1.0));
    assert!(g.take_appearance_changes().iter().any(|c| c.actor == FormId(ADULT_REF)));
    // An NPC source's face is the engine's to read from its record.
    call(&mut g, 0x11DF, Some(ADULT_REF), &[r(CHILD_REF), n(150.0)]);
    let m = &g.extra[&FormId(ADULT_REF)].face_matches[1];
    assert!(m.fggs.is_empty() && m.percent == 100.0);
    // On the player: only towards a face the game holds.
    let before = g.player.look.as_ref().unwrap().fggs.clone();
    let ctx = Ctx { unit: "test".into(), opcode: 0x11DF, on: Some(FormId(PLAYER)), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[r(CHILD_REF), n(50.0)]);
    assert_eq!(g.player.look.as_ref().unwrap().fggs, before);
    // The blend: halfway, a missing coefficient counting as 0.
    assert_eq!(fnv_game::appearance::blend_coefficients(&[0.0, 2.0], &[2.0], 50.0), [1.0, 1.0]);
}

/// A grenade placed in a child's pocket does not go live: it stays an ordinary item
/// [documented: Fallout Wiki, Fallout 3 Pickpocket; hypothesis for New Vegas]
/// (docs/spec-notes/pickpocket.md §5).
#[test]
fn a_planted_explosive_stays_an_item_on_a_child() {
    let f = fixture();
    let mut g = game(&f);
    let frag = fnv_game::pickpocket::Explosive { damage: 125.0, ..Default::default() };
    assert_eq!(g.plant_explosive(FormId(CHILD_REF), FormId(0x4330), frag), Err(fnv_game::pickpocket::NotLive::Child));
    assert!(g.pickpocket.planted.is_empty());
    assert_eq!(g.plant_explosive(FormId(ADULT_REF), FormId(0x4330), frag), Ok(()));
}

#[test]
fn generic_form_functions_edit_a_form_list_in_place() {
    // #1845: code 13 is a form list; an edit keeps the other members.
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut Game, body: &str| g.run_query(&format!("Begin Function {{ }}
{body}
End
"), ctx()).unwrap();
    let nth = |g: &mut Game, i: u32| q(g, &format!("SetFunctionValue (ListGetNthForm GunList {i})"));
    let count = |g: &mut Game| q(g, "SetFunctionValue (ListGetCount GunList)");
    q(&mut g, "GenericAddForm 13, GunList, Junk, -2");
    assert_eq!(count(&mut g), XVal::Num(2.0));
    assert_eq!((nth(&mut g, 0), nth(&mut g, 1)), (XVal::Ref(WEAP), XVal::Ref(MISC)), "added at the end");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericReplaceForm 13, GunList, TestHolotape, 0)"), XVal::Ref(WEAP));
    assert_eq!((nth(&mut g, 0), nth(&mut g, 1)), (XVal::Ref(NOTE), XVal::Ref(MISC)), "replaced in place");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 13, GunList, 0)"), XVal::Ref(NOTE));
    assert_eq!(count(&mut g), XVal::Num(1.0));
    assert_eq!(nth(&mut g, 0), XVal::Ref(MISC), "the other member stays");
}

#[test]
fn luck_skill_setting_fallback_preserves_loaded_overrides() {
    let mut f = fixture();
    let mut g = game(&f);
    assert_eq!(call(&mut g, 0x1100, None, &[ArgValue::Str("favdskillluckbonusmult".into())]), 0.5);
    drop(g);
    f.forms.settings.insert("favdskillluckbonusmult".into(), fnv_sim::settings::Setting::Float(0.75));
    let mut g = game(&f);
    assert_eq!(call(&mut g, 0x1100, None, &[ArgValue::Str("fAVDSkillLuckBonusMult".into())]), 0.75);
    g.run_source("SetNumericGameSetting \"fAVDSkillLuckBonusMult\" 0.25", Default::default()).unwrap();
    assert_eq!(call(&mut g, 0x1100, None, &[ArgValue::Str("fAVDSkillLuckBonusMult".into())]), 0.25);
    let before = g.events.len();
    let ctx = Ctx { unit: "owned setting".into(), opcode: 0x1100, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[ArgValue::Str("fOwnedUnknownSetting".into())]);
    assert!(g.events[before..].iter().any(|e| matches!(&e.kind, EventKind::CallFailed { error, .. } if error.contains("fOwnedUnknownSetting"))));

}

// IsActorDetected is catalogue-unnumbered: these controls go through the actual
// compiler/decode/VM named call, not an opcode or a mirrored query helper.
fn assert_actor_detected_query(g: &mut Game, target: u32, expected: f64) {
    use fnv_game::game::RunCtx;
    use fnv_script::vm::XVal;
    let caller = if target == ADULT_REF { CHILD_REF } else { ADULT_REF };
    let ctx = RunCtx { self_ref: Some(FormId(caller)), action_ref: Some(FormId(target)), ..Default::default() };
    assert!(g.data.table.unnumbered.contains_key("isactordetected"));
    assert_eq!(g.data.table.by_name("IsActorDetected"), None, "no invented native opcode");
    assert_eq!(g.run_query("Begin Function {}\nSetFunctionValue GetActionRef\nEnd\n", ctx), Ok(XVal::Ref(target)), "real receiver admission");
    let before = g.events.len();
    let result = g.run_query("ref receiver\nBegin Function {}\nlet receiver := GetActionRef\nSetFunctionValue (receiver.IsActorDetected)\nEnd\n", ctx);
    eprintln!("IsActorDetected target={target:08X} caller={caller:08X} result={result:?} events={:?}", &g.events[before..]);
    assert_eq!(result, Ok(XVal::Num(expected)), "actual compiled explicit-receiver result");
    assert!(!g.events[before..].iter().any(|e| matches!(e.kind,
        EventKind::Stub { .. } | EventKind::Unhandled { .. } | EventKind::CallFailed { .. }
        | EventKind::ScriptError { .. } | EventKind::Skipped { .. } | EventKind::ArgUnread { .. })),
        "a stub's zero is not a successful negative: {:?}", &g.events[before..]);
}

fn actor_detected_seen(g: &mut Game, detector: u32, target: u32) {
    use fnv_game::detection::{Band, Settings, Situation};
    let seen = Situation { distance: 300.0, exterior: true, line_of_sight: true, in_view: true, light: 80.0, ..Default::default() };
    let (d, _) = g.detect(&Settings::default(), FormId(detector), FormId(target), &seen);
    assert_eq!(d.band, Band::Detected, "real detection producer precondition: {d:?}");
}

#[test]
fn is_actor_detected_uses_incoming_pairs_and_explicit_receiver() {
    let f = fixture();
    let mut g = game(&f);
    actor_detected_seen(&mut g, CHILD_REF, ADULT_REF);
    assert_actor_detected_query(&mut g, ADULT_REF, 1.0);
    assert_actor_detected_query(&mut g, CHILD_REF, 0.0);
    let mut cold = game(&f);
    assert!(cold.detection.pairs.is_empty());
    assert_actor_detected_query(&mut cold, ADULT_REF, 0.0);
    actor_detected_seen(&mut cold, ADULT_REF, CHILD_REF);
    assert_actor_detected_query(&mut cold, ADULT_REF, 0.0);
    assert_actor_detected_query(&mut cold, CHILD_REF, 1.0);
}

#[test]
fn is_actor_detected_excludes_player_and_self_but_keeps_teammates() {
    let f = fixture();
    let mut g = game(&f);
    actor_detected_seen(&mut g, PLAYER, ADULT_REF);
    assert_eq!(call(&mut g, 0x102D, Some(PLAYER), &[r(ADULT_REF)]), 1.0, "GetDetected still accepts player perception");
    assert_actor_detected_query(&mut g, ADULT_REF, 0.0);
    actor_detected_seen(&mut g, ADULT_REF, ADULT_REF);
    assert_actor_detected_query(&mut g, ADULT_REF, 0.0);
    actor_detected_seen(&mut g, CHILD_REF, PLAYER);
    assert_actor_detected_query(&mut g, PLAYER, 0.0);
    call(&mut g, 0x11C5, Some(CHILD_REF), &[n(1.0)]);
    assert!(g.with_player(FormId(CHILD_REF)), "actual teammate setup");
    actor_detected_seen(&mut g, CHILD_REF, ADULT_REF);
    assert_actor_detected_query(&mut g, ADULT_REF, 1.0);
}

#[test]
fn is_actor_detected_checks_any_typed_band_without_value_ordering() {
    use fnv_game::detection::{Band, Detection};
    let f = fixture();
    let mut g = game(&f);
    // The public typed cache accepts these authored rows. Query semantics must not
    // assume that the largest numeric value has the only relevant band.
    g.detection.record(FormId(CHILD_REF), FormId(ADULT_REF), Detection { value: 1.0, band: Band::Detected, ..Default::default() }, g.time);
    g.detection.record(FormId(CREA_REF), FormId(ADULT_REF), Detection { value: 100.0, band: Band::Noticed, ..Default::default() }, g.time);
    assert_eq!(g.detection.highest(FormId(ADULT_REF)).unwrap().1.band, Band::Noticed, "guard the max-value shortcut");
    assert_actor_detected_query(&mut g, ADULT_REF, 1.0);
    g.detection.forget(FormId(CHILD_REF));
    assert_actor_detected_query(&mut g, ADULT_REF, 0.0);
    g.detection.record(FormId(CREA_REF), FormId(ADULT_REF), Detection { value: -100.0, band: Band::Hidden, ..Default::default() }, g.time);
    assert_actor_detected_query(&mut g, ADULT_REF, 0.0);
}

#[test]
fn is_actor_detected_reuses_overwrite_forget_and_owner_expiry() {
    use fnv_game::detection::{Band, Settings, Situation};
    let f = fixture();
    let mut g = game(&f);
    g.time = 10.0;
    actor_detected_seen(&mut g, CHILD_REF, ADULT_REF);
    let unseen = Situation { distance: 300.0, exterior: true, line_of_sight: true, in_view: false, light: 80.0, ..Default::default() };
    let (d, _) = g.detect(&Settings::default(), FormId(CHILD_REF), FormId(ADULT_REF), &unseen);
    assert_ne!(d.band, Band::Detected, "real remeasurement, not a cache erase");
    assert_actor_detected_query(&mut g, ADULT_REF, 0.0);
    actor_detected_seen(&mut g, CHILD_REF, ADULT_REF);
    actor_detected_seen(&mut g, CREA_REF, PLAYER);
    g.detection.forget(FormId(ADULT_REF));
    assert!(g.detection.get(FormId(CREA_REF), FormId(PLAYER)).is_some(), "unrelated pair survives target-forget");
    assert_actor_detected_query(&mut g, ADULT_REF, 0.0);
    actor_detected_seen(&mut g, CHILD_REF, ADULT_REF);
    g.time = 10.75;
    assert_actor_detected_query(&mut g, ADULT_REF, 1.0);
    assert_eq!(call(&mut g, 0x102D, Some(CHILD_REF), &[r(ADULT_REF)]), 1.0, "no private query TTL");
    g.detection.forget_older_than(10.0);
    assert_actor_detected_query(&mut g, ADULT_REF, 1.0);
    g.detection.forget_older_than(10.001);
    assert_actor_detected_query(&mut g, ADULT_REF, 0.0);
    assert_eq!(call(&mut g, 0x102D, Some(CHILD_REF), &[r(ADULT_REF)]), 0.0);
}

#[test]
fn is_actor_detected_restore_is_cold_until_actual_remeasurement() {
    let f = fixture();
    let mut g = game(&f);
    actor_detected_seen(&mut g, CHILD_REF, ADULT_REF);
    assert_actor_detected_query(&mut g, ADULT_REF, 1.0);
    let snapshot = g.checked_snapshot().unwrap();
    let world = World::restore(&f.forms, &f.refs, &snapshot.sim).unwrap();
    let mut restored = Game::restore(&f.data, world, GameConfig { strict: true, ..GameConfig::default() }, &snapshot).unwrap();
    assert!(restored.detection.pairs.is_empty());
    assert_actor_detected_query(&mut restored, ADULT_REF, 0.0);
    actor_detected_seen(&mut restored, CHILD_REF, ADULT_REF);
    assert_actor_detected_query(&mut restored, ADULT_REF, 1.0);
}

#[test]
fn is_actor_detected_keeps_get_detected_level_and_los_neighbors() {
    use fnv_game::detection::{Band, Detection};
    let f = fixture();
    let mut g = game(&f);
    for (band, value, expected, level) in [(Band::Hidden, -100.0, 0.0, 1.0), (Band::Noticed, -10.0, 0.0, 2.0), (Band::Detected, 10.0, 1.0, 3.0)] {
        g.detection.record(FormId(CHILD_REF), FormId(ADULT_REF), Detection { value, band, seen: true, ..Default::default() }, g.time);
        assert_actor_detected_query(&mut g, ADULT_REF, expected);
        assert_eq!(call(&mut g, 0x102D, Some(CHILD_REF), &[r(ADULT_REF)]), expected);
        assert_eq!(call(&mut g, 0x10B4, Some(CHILD_REF), &[r(ADULT_REF)]), level);
        assert_eq!(call(&mut g, 0x101B, Some(CHILD_REF), &[r(ADULT_REF)]), 1.0, "line of sight is not the detection band");
    }
}

#[test]
fn kill_actor_drops_dead_detector_same_tick_and_is_actor_detected_invalid_receivers_continue() {
    use fnv_game::game::RunCtx;
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = game(&f);
    // Two living actors detect Player. CHILD_REF also detects ADULT_REF
    // and is itself seen by CREA_REF.
    actor_detected_seen(&mut g, CHILD_REF, PLAYER);
    actor_detected_seen(&mut g, CREA_REF, PLAYER);
    actor_detected_seen(&mut g, CHILD_REF, ADULT_REF);
    actor_detected_seen(&mut g, CREA_REF, CHILD_REF);
    assert_eq!(g.detecting_actors(FormId::PLAYER_REF).len(), 2);
    assert_eq!(call(&mut g, 0x102D, Some(CHILD_REF), &[r(PLAYER)]), 1.0);
    assert_eq!(call(&mut g, 0x102D, Some(CHILD_REF), &[r(ADULT_REF)]), 1.0);
    assert_actor_detected_query(&mut g, ADULT_REF, 1.0);
    // Player.IsActorDetected always returns 0 even while detected by 2 actors.
    assert_actor_detected_query(&mut g, PLAYER, 0.0);

    // Same-tick KillActor on CHILD_REF immediately removes it from GetDetectingActors,
    // GetDetected, and IsActorDetected without waiting for deferred cache expiry.
    let same_tick = g.run_query(
        &format!(
            "ref rChild\nref rAdult\narray_var a\nint n\nint gdP\nint gdA\nint detA\nint detP\nBegin Function {{}}\n\
             let rChild := {CHILD_REF}\n\
             let rAdult := {ADULT_REF}\n\
             rChild.KillActor player\n\
             let a := player.GetDetectingActors\n\
             let n := ar_Size a\n\
             let gdP := rChild.GetDetected player\n\
             let gdA := rChild.GetDetected rAdult\n\
             let detA := rAdult.IsActorDetected\n\
             let detP := player.IsActorDetected\n\
             SetFunctionValue (n * 10000 + gdP * 1000 + gdA * 100 + detA * 10 + detP)\n\
             End\n"
        ),
        RunCtx { self_ref: Some(FormId(ADULT_REF)), ..Default::default() },
    );
    assert_eq!(same_tick, Ok(XVal::Num(10000.0)), "same-tick: nDet=1, gdP=0, gdA=0, detA=0, detP=0");
    assert_eq!(g.detecting_actors(FormId::PLAYER_REF), vec![FormId(CREA_REF)]);
    assert!(g.detection.get(FormId(CHILD_REF), FormId(PLAYER)).is_none());
    assert!(g.detection.get(FormId(CHILD_REF), FormId(ADULT_REF)).is_none());
    assert!(
        g.detection.get(FormId(CREA_REF), FormId(CHILD_REF)).is_some_and(|d| d.seen),
        "the living observer's sight of the dead actor survives",
    );
    assert_eq!(call(&mut g, 0x102D, Some(CREA_REF), &[r(CHILD_REF)]), 1.0);

    // Re-establish a live detection on ADULT_REF so we prove invalid/null receivers do
    // not fall back to self_ref (ADULT_REF) and always return 0 while continuing.
    actor_detected_seen(&mut g, CREA_REF, ADULT_REF);
    assert_actor_detected_query(&mut g, ADULT_REF, 1.0);
    let ctx = RunCtx { self_ref: Some(FormId(ADULT_REF)), ..Default::default() };
    // Non-actor object receiver returns 0 and continues.
    assert_eq!(
        g.run_query(
            &format!("ref rObj\nint v\nBegin Function {{}}\nlet rObj := {ACTI_REF}\nlet v := rObj.IsActorDetected\nSetFunctionValue (v + 10)\nEnd\n"),
            ctx,
        ),
        Ok(XVal::Num(10.0)),
    );
    // Explicit null receiver returns 0 without falling back to self_ref and continues.
    assert_eq!(
        g.run_query("ref rNull\nint v\nBegin Function {}\nlet rNull := 0\nlet v := rNull.IsActorDetected\nSetFunctionValue (v + 20)\nEnd\n", ctx),
        Ok(XVal::Num(20.0)),
    );
}

#[test]
fn aim_anim_groups_and_braced_call_args_execute_through_game() {
    let f = fixture();
    let mut g = game(&f);
    for (name, code) in [("AimUp", 18u16), ("AimDown", 19), ("AimIS", 20), ("AimISUp", 21), ("AimISDown", 22)] {
        assert_eq!(g.anim_group_code(name), Some(code), "{name}");
        assert_eq!(g.anim_group_name(code).as_deref(), Some(name), "{code}");
    }
    let res = g.run_query(
        "ref fnAdd\nint a\nint b\nBegin Function {}\n\
         let fnAdd := (begin Function {int x, int y}\n\
           SetFunctionValue (x * 10 + y)\n\
         end)\n\
         let a := 4\n\
         let b := 7\n\
         PlayerREF.PlayGroup AimIS 1\n\
         SetFunctionValue ((call fnAdd {a b}) + (call fnAdd {b, a}))\n\
         End\n",
        fnv_game::game::RunCtx::default(),
    );
    assert_eq!(res, Ok(fnv_script::vm::XVal::Num(121.0)));
}
