//! A script moving the clock by more than an hour (`Game::take_hour_jump`,
//! `fnv_ai::Ai::hour_jumped`, issue #2353): everyone chooses its package for the new hour
//! at once, the actors the player cannot see are put where their new package has them,
//! and those in view only set off. A plugin and navmeshes built in memory (no game data):
//! a house and a street joined by a door; a sleeper at home in the house and a stander in
//! the street, each with a package for 00:00-08:00 and one for 08:00-24:00. The rule:
//! docs/spec-notes/packages.md §9 [design, #2353].

use fnv_ai::{Ai, AiConfig};
use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig};
use fnv_game::GameData;
use fnv_nav::record::{tri_flags, DoorLink, NavMeshRecord, Triangle};
use fnv_nav::{NavGraph, Space};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

// ---- plugin builders ---------------------------------------------------------------

fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
    Subrecord { sig: *sig, data, extended: false }
}

fn z(s: &str) -> Vec<u8> {
    [s.as_bytes(), &[0]].concat()
}

fn record(sig: &[u8; 4], fid: u32, subs: &[Subrecord]) -> Vec<u8> {
    let payload = write_subrecords(subs);
    [&sig[..], &(payload.len() as u32).to_le_bytes(), &0u32.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
}

fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
    [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
}

fn tes4() -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x900u32.to_le_bytes()].concat();
    let payload = write_subrecords(&[sub(b"HEDR", hedr)]);
    [&b"TES4"[..], &(payload.len() as u32).to_le_bytes(), &1u32.to_le_bytes(), &[0; 4], &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
}

fn glob(fid: u32, edid: &str, v: f32) -> Vec<u8> {
    record(b"GLOB", fid, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", v.to_le_bytes().to_vec())])
}

/// A Travel package to a reference (radius 0), run from `hour` for `hours` every day
/// (PSDT: any month, any day of the week, any date, the start hour, the duration
/// [documented: xEdit PACK PSDT; packages.md §1]).
fn travel_at(id: u32, edid: &str, to: u32, hour: u8, hours: u32) -> Vec<u8> {
    let subs = [
        sub(b"EDID", z(edid)),
        sub(b"PKDT", [&0u32.to_le_bytes()[..], &[TRAVEL, 0, 0, 0], &[0; 4]].concat()),
        sub(b"PLDT", [0i32.to_le_bytes(), to.to_le_bytes(), 0i32.to_le_bytes()].concat()),
        sub(b"PSDT", [&[0xFF, 0xFF, 0, hour][..], &hours.to_le_bytes()].concat()),
    ];
    record(b"PACK", id, &subs)
}

fn npc(fid: u32, edid: &str, packages: &[u32]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid))];
    subs.extend(packages.iter().map(|p| sub(b"PKID", p.to_le_bytes().to_vec())));
    record(b"NPC_", fid, &subs)
}

fn placed(sig: &[u8; 4], fid: u32, edid: &str, base: u32, pos: [f32; 3]) -> Vec<u8> {
    let subs = [sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", [pos[0].to_le_bytes(), pos[1].to_le_bytes(), pos[2].to_le_bytes(), [0; 4], [0; 4], [0; 4]].concat())];
    record(sig, fid, &subs)
}

const TRAVEL: u8 = 6;
const G_HOUR: u32 = 0x904;
const STAT: u32 = 0x910;
const P_HOME: u32 = 0x920;
const P_PORCH: u32 = 0x921;
const P_CORNER: u32 = 0x922;
const P_WELL: u32 = 0x923;
const N_SLEEPER: u32 = 0x930;
const N_STANDER: u32 = 0x931;
const HOUSE: u32 = 0x940;
const STREET: u32 = 0x941;
const MARK_HOME: u32 = 0x950;
const MARK_PORCH: u32 = 0x951;
const MARK_CORNER: u32 = 0x952;
const MARK_WELL: u32 = 0x953;
const DOOR_HOUSE: u32 = 0x954;
const DOOR_STREET: u32 = 0x955;
const R_SLEEPER: u32 = 0x960;
const R_STANDER: u32 = 0x961;
const HOME: [f32; 3] = [150.0, 150.0, 0.0];
const PORCH: [f32; 3] = [5250.0, 5250.0, 50.0];
const CORNER: [f32; 3] = [5050.0, 5150.0, 50.0];
const WELL: [f32; 3] = [5250.0, 5050.0, 50.0];
const IN_HOUSE: Space = Space::Interior(HOUSE);
const IN_STREET: Space = Space::Interior(STREET);

fn plugin_bytes() -> Vec<u8> {
    let globs = [
        glob(0x901, "GameYear", 2281.0),
        glob(0x902, "GameMonth", 9.0),
        glob(0x903, "GameDay", 19.0),
        glob(G_HOUR, "GameHour", 6.0),
        glob(0x905, "GameDaysPassed", 5.0),
        glob(0x906, "TimeScale", 30.0),
    ]
    .concat();
    let packs = [
        travel_at(P_HOME, "StayHomeAtNight", MARK_HOME, 0, 8),
        travel_at(P_PORCH, "PorchByDay", MARK_PORCH, 8, 16),
        travel_at(P_CORNER, "CornerAtNight", MARK_CORNER, 0, 8),
        travel_at(P_WELL, "WellByDay", MARK_WELL, 8, 16),
    ]
    .concat();
    let npcs = [npc(0x7, "Player", &[]), npc(N_SLEEPER, "Sleeper", &[P_HOME, P_PORCH]), npc(N_STANDER, "Stander", &[P_CORNER, P_WELL])].concat();
    let stat = record(b"STAT", STAT, &[sub(b"EDID", z("Marker"))]);
    let house = [
        placed(b"REFR", MARK_HOME, "MarkHome", STAT, HOME),
        // The door the graph's door link names (away from everything walked to).
        placed(b"REFR", DOOR_HOUSE, "HouseDoor", STAT, [-2000.0, -2000.0, 0.0]),
        placed(b"ACHR", R_SLEEPER, "SleeperRef", N_SLEEPER, HOME),
    ]
    .concat();
    let street = [
        placed(b"REFR", MARK_PORCH, "MarkPorch", STAT, PORCH),
        placed(b"REFR", MARK_CORNER, "MarkCorner", STAT, CORNER),
        placed(b"REFR", MARK_WELL, "MarkWell", STAT, WELL),
        placed(b"ACHR", R_STANDER, "StanderRef", N_STANDER, CORNER),
    ]
    .concat();
    let cell = |id: u32, edid: &str, refs: &[u8]| {
        let rec = record(b"CELL", id, &[sub(b"EDID", z(edid))]);
        [rec, group(id.to_le_bytes(), 6, &group(id.to_le_bytes(), 9, refs))].concat()
    };
    let cells = [cell(HOUSE, "House", &house), cell(STREET, "Street", &street)].concat();
    let top = |sig: &[u8; 4], body: &[u8]| group(*sig, 0, body);
    [
        tes4(),
        top(b"GLOB", &globs),
        top(b"STAT", &stat),
        top(b"NPC_", &npcs),
        top(b"PACK", &packs),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cells))),
    ]
    .concat()
}

// ---- navmeshes ---------------------------------------------------------------------

/// A flat navmesh over `n` × `n` squares of side 100 from `origin`, two triangles a
/// square, at height `z`.
fn grid(form_id: u32, origin: [f32; 2], n: usize, z: f32) -> NavMeshRecord {
    let vid = |i: usize, j: usize| (j * (n + 1) + i) as u16;
    let mut vertices = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            vertices.push([origin[0] + i as f32 * 100.0, origin[1] + j as f32 * 100.0, z]);
        }
    }
    let tri = |i: isize, j: isize, h: usize| -> i16 {
        if i < 0 || j < 0 || i >= n as isize || j >= n as isize {
            -1
        } else {
            ((j as usize * n + i as usize) * 2 + h) as i16
        }
    };
    let mut triangles = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (a, b, c, d) = (vid(i, j), vid(i + 1, j), vid(i + 1, j + 1), vid(i, j + 1));
            let (ii, jj) = (i as isize, j as isize);
            triangles.push(Triangle { v: [a, b, c], edges: [tri(ii, jj - 1, 1), tri(ii + 1, jj, 1), tri(ii, jj, 1)], flags: 0, cover: 0 });
            triangles.push(Triangle { v: [a, c, d], edges: [tri(ii, jj, 0), tri(ii, jj + 1, 0), tri(ii - 1, jj, 0)], flags: 0, cover: 0 });
        }
    }
    NavMeshRecord { form_id, vertices, triangles, ..Default::default() }
}

/// The house: 4 × 4 squares at the origin; the street: 3 × 3 at (5000, 5000), height 50.
/// A door in the house's far corner leads to the street's near corner and back.
fn nav() -> NavGraph {
    let mut house = grid(1, [0.0, 0.0], 4, 0.0);
    let mut street = grid(2, [5000.0, 5000.0], 3, 50.0);
    let corner = house.triangles.len() - 2;
    house.door_links.push(DoorLink { door: DOOR_HOUSE, triangle: corner as u16 });
    house.triangles[corner].flags |= tri_flags::DOOR;
    street.door_links.push(DoorLink { door: DOOR_STREET, triangle: 0 });
    let input = [(&house, Some(HOUSE), Some(IN_HOUSE)), (&street, Some(STREET), Some(IN_STREET))];
    let mut g = NavGraph::assemble(&input);
    g.join_doors(&input, |door| match door {
        DOOR_HOUSE => Some((DOOR_STREET, [5050.0, 5030.0, 50.0], 0.5, IN_STREET)),
        DOOR_STREET => Some((DOOR_HOUSE, [340.0, 340.0, 0.0], 3.0, IN_HOUSE)),
        _ => None,
    });
    g
}

// ---- fixture -----------------------------------------------------------------------

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "HourJump.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);
    Fixture { data, forms, refs }
}

struct Sim<'a> {
    game: Game<'a>,
    ai: Ai,
    nav: NavGraph,
}

fn sim(f: &Fixture) -> Sim<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let game = Game::new(&f.data, world, GameConfig { strict: true, frame_seconds: 0.5, package_interval: Some(10.0), ..GameConfig::default() });
    let mut ai = Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() });
    for a in [R_SLEEPER, R_STANDER] {
        ai.add(&game, FormId(a)).unwrap();
    }
    Sim { game, ai, nav: nav() }
}

impl Sim<'_> {
    fn run(&mut self, seconds: f64) {
        for _ in 0..(seconds * 2.0) as usize {
            self.game.tick();
            self.ai.update(&mut self.game, &self.nav, 0.5);
        }
    }

    fn feet(&self, actor: u32) -> [f32; 3] {
        self.ai.agents[&FormId(actor)].walker.feet
    }

    fn package(&mut self, actor: u32) -> Option<FormId> {
        self.game.current_package(FormId(actor))
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The house is out of sight (an interior behind a door of the loaded cells); the street
/// is drawn.
fn unseen(p: fnv_nav::Place) -> bool {
    p.space == IN_HOUSE
}

/// Both on their night packages at 06:00, where those have them.
fn at_six(f: &Fixture) -> Sim<'_> {
    let mut s = sim(f);
    s.run(2.0);
    assert_eq!(s.package(R_SLEEPER), Some(FormId(P_HOME)));
    assert_eq!(s.package(R_STANDER), Some(FormId(P_CORNER)));
    assert!(dist(s.feet(R_SLEEPER), HOME) < 5.0 && dist(s.feet(R_STANDER), CORNER) < 5.0);
    s
}

// ---- tests -------------------------------------------------------------------------

/// The issue's case: the clock set from 06:00 to 08:00 by a script. Before, nobody chose
/// again until its periodic turn (up to 10 s on), and then walked from its night place.
/// Now everyone chooses at once; the sleeper, out of sight in the house, is on the porch
/// without walking; the stander, in view in the street, is not moved but sets off.
#[test]
fn a_jump_of_the_clock_rechooses_everyone_and_puts_the_unseen_at_their_place() {
    let f = fixture();
    let mut s = at_six(&f);
    s.game.set_global_by_script(FormId(G_HOUR), 8.0).unwrap();
    let jump = s.game.take_hour_jump().expect("two hours is a jump");
    assert_eq!((jump.from, jump.to), (6.0, 8.0));
    // The write alone changes no package (the periodic choice is not due).
    assert_eq!(s.package(R_SLEEPER), Some(FormId(P_HOME)));
    let put = s.ai.hour_jumped(&mut s.game, &s.nav, &unseen);
    assert_eq!(put.len(), 1, "{put:?}");
    let (who, arr) = &put[0];
    assert_eq!((*who, arr.package), (FormId(R_SLEEPER), FormId(P_PORCH)));
    assert_eq!((arr.from.space, arr.to.space), (IN_HOUSE, IN_STREET));
    assert!(dist(s.feet(R_SLEEPER), PORCH) < 5.0, "sleeper at {:?}", s.feet(R_SLEEPER));
    assert_eq!(s.ai.agents[&FormId(R_SLEEPER)].walker.travelled, 0.0, "not walked");
    assert_eq!(s.game.world.peek(FormId(R_SLEEPER)).unwrap().cell, Some(FormId(STREET)));
    // In view: its day package chosen, still where it stood, and walking there.
    assert_eq!(s.package(R_STANDER), Some(FormId(P_WELL)));
    assert!(dist(s.feet(R_STANDER), CORNER) < 5.0);
    s.run(4.0);
    assert!(dist(s.feet(R_STANDER), WELL) < 20.0, "stander at {:?}", s.feet(R_STANDER));
    assert!(s.ai.agents[&FormId(R_STANDER)].walker.travelled > 150.0, "walked, not put there");
    // The sleeper stays on the porch.
    assert!(dist(s.feet(R_SLEEPER), PORCH) < 5.0);
}

/// With the whole place in view, a jump only re-chooses: nobody is put anywhere.
#[test]
fn a_jump_in_full_view_moves_nobody() {
    let f = fixture();
    let mut s = at_six(&f);
    s.game.set_global_by_script(FormId(G_HOUR), 8.0).unwrap();
    assert!(s.game.take_hour_jump().is_some());
    assert!(s.ai.hour_jumped(&mut s.game, &s.nav, &|_| false).is_empty());
    assert_eq!(s.package(R_SLEEPER), Some(FormId(P_PORCH)));
    assert!(dist(s.feet(R_SLEEPER), HOME) < 5.0);
}

/// Half an hour is no jump: the engine is told nothing.
#[test]
fn a_small_move_of_the_clock_is_no_jump() {
    let f = fixture();
    let mut s = at_six(&f);
    s.game.set_global_by_script(FormId(G_HOUR), 6.5).unwrap();
    assert_eq!(s.game.take_hour_jump(), None);
}

/// Issue #3041: Goodsprings night stalkers (`VERStalkersSouth01`). A Low-process actor
/// running a 23:00 `Must Reach Location` patrol package conditioned on a quest/global flag
/// is still en route when a script jumps `GameHour` to 8 (`VCG01` stage 200). Later, the
/// quest's day branch calls `MoveTo` to put the actor in a closed holding interior cell
/// (no doors to the patrol route) and 1.8 s later clears the flag without calling
/// `EvaluatePackage`. Even across `FAR_PROCESS_SECONDS` (120 s) of subsequent Low-process
/// updates — and even if the actor's low-process timer was nearly due when `MoveTo` ran —
/// it drops the raid package cleanly and logs zero `Unreachable` path failures.
#[test]
fn relocated_low_process_night_raider_drops_expired_must_reach_patrol_at_0800() {
    use fnv_ai::{AiEvent, ProcessLevel};
    use fnv_game::package::flags;

    const G_ON: u32 = 0x907;
    const P_RAID: u32 = 0x924;
    const N_STALKER: u32 = 0x932;
    const HOLDING: u32 = 0x942;
    const MARK_HOLD: u32 = 0x956;
    const MARK_PATROL_A: u32 = 0x957;
    const MARK_PATROL_B: u32 = 0x958;
    const R_STALKER: u32 = 0x962;
    const PATROL: u8 = 13;
    const GET_GLOBAL_VALUE: u16 = 0x4A;

    let mut cond = vec![0u8, 0, 0, 0];
    cond.extend(1.0f32.to_le_bytes());
    cond.extend(GET_GLOBAL_VALUE.to_le_bytes());
    cond.extend([0, 0]);
    cond.extend(G_ON.to_le_bytes());
    cond.extend([0u8; 12]);
    let raid_pack = record(
        b"PACK",
        P_RAID,
        &[
            sub(b"EDID", z("NightRaidMWF3")),
            sub(b"PKDT", [&flags::MUST_REACH_LOCATION.to_le_bytes()[..], &[PATROL, 0, 0, 0], &[0; 4]].concat()),
            sub(b"PLDT", [0i32.to_le_bytes(), MARK_PATROL_A.to_le_bytes(), 0i32.to_le_bytes()].concat()),
            sub(b"PSDT", [&[0xFF, 0xFF, 0, 23][..], &1u32.to_le_bytes()].concat()),
            sub(b"CTDA", cond),
        ],
    );
    let linked_placed = |fid: u32, edid: &str, pos: [f32; 3], linked: Option<u32>| {
        let mut subs = vec![sub(b"EDID", z(edid)), sub(b"NAME", STAT.to_le_bytes().to_vec())];
        if let Some(l) = linked {
            subs.push(sub(b"XLKR", l.to_le_bytes().to_vec()));
        }
        subs.push(sub(b"DATA", [pos[0].to_le_bytes(), pos[1].to_le_bytes(), pos[2].to_le_bytes(), [0; 4], [0; 4], [0; 4]].concat()));
        record(b"REFR", fid, &subs)
    };
    let globs = [
        glob(0x901, "GameYear", 2281.0),
        glob(0x902, "GameMonth", 9.0),
        glob(0x903, "GameDay", 19.0),
        glob(G_HOUR, "GameHour", 23.0),
        glob(0x905, "GameDaysPassed", 5.0),
        glob(0x906, "TimeScale", 30.0),
        glob(G_ON, "StalkersOn", 1.0),
    ]
    .concat();
    let stat = record(b"STAT", STAT, &[sub(b"EDID", z("Marker"))]);
    let npcs = [npc(0x7, "Player", &[]), npc(N_STALKER, "NightStalker", &[P_RAID])].concat();
    let street_refs = [
        linked_placed(MARK_PATROL_A, "PatrolA", [5250.0, 5250.0, 50.0], Some(MARK_PATROL_B)),
        linked_placed(MARK_PATROL_B, "PatrolB", [5050.0, 5250.0, 50.0], None),
        placed(b"ACHR", R_STALKER, "StalkerRef", N_STALKER, [5050.0, 5050.0, 50.0]),
    ]
    .concat();
    let hold_refs = [placed(b"REFR", MARK_HOLD, "RestMarker", STAT, [150.0, 150.0, 0.0])].concat();
    let cell = |id: u32, edid: &str, refs: &[u8]| {
        let rec = record(b"CELL", id, &[sub(b"EDID", z(edid))]);
        [rec, group(id.to_le_bytes(), 6, &group(id.to_le_bytes(), 9, refs))].concat()
    };
    let cells = [cell(STREET, "Exterior", &street_refs), cell(HOLDING, "RestCell", &hold_refs)].concat();
    let top = |sig: &[u8; 4], body: &[u8]| group(*sig, 0, body);
    let bytes = [
        tes4(),
        top(b"GLOB", &globs),
        top(b"STAT", &stat),
        top(b"NPC_", &npcs),
        top(b"PACK", &raid_pack),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cells))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "StalkerRaid.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);

    // Two disconnected navmeshes: the exterior street and the closed holding cell (no doors).
    let street_nm = grid(1, [5000.0, 5000.0], 3, 50.0);
    let hold_nm = grid(2, [0.0, 0.0], 2, 0.0);
    let nav = NavGraph::assemble(&[(&street_nm, Some(STREET), Some(IN_STREET)), (&hold_nm, Some(HOLDING), Some(Space::Interior(HOLDING)))]);

    let world = World::new(&forms, &refs, SimConfig::default());
    let mut game = Game::new(&data, world, GameConfig { strict: true, frame_seconds: 0.5, package_interval: Some(10.0), ..GameConfig::default() });
    let mut ai = Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() });
    let stalker = FormId(R_STALKER);
    ai.add(&game, stalker).unwrap();

    // 23:00 with StalkersOn == 1: stalker chooses the Must Reach Location raid patrol while
    // outside and moves to Low process (unloaded exterior).
    assert_eq!(game.evaluate_package(stalker), Some(FormId(P_RAID)));
    ai.set_level(&game, stalker, ProcessLevel::Low);

    // VCG01 stage 200 jumps the clock from 23:00 to 08:00 while StalkersOn is still 1.
    game.set_global_by_script(FormId(G_HOUR), 8.0).unwrap();
    assert!(game.take_hour_jump().is_some());
    ai.hour_jumped(&mut game, &nav, &|_| false);

    // Advance close to a Low-process update boundary, then VERStalkersSouth01's day branch
    // moves the stalker to the closed holding cell (`MoveTo`), and 2 s later clears
    // `StalkersOn = 0` without calling `EvaluatePackage`.
    for _ in 0..10 {
        game.tick();
        ai.update(&mut game, &nav, 0.5);
    }
    fnv_game::functions::console::call_words(&mut game, Some(stalker), "moveto", &[format!("{MARK_HOLD:08X}")], "test").unwrap();
    assert_eq!(ai.moved_by_scripts(&mut game), vec![(stalker, true)]);
    // 2 s gap between MoveTo and setting StalkersOn = 0 (VERStalkersSouth01 loops 18 actors).
    for _ in 0..4 {
        game.tick();
        ai.update(&mut game, &nav, 0.5);
    }
    game.set_global_by_script(FormId(G_ON), 0.0).unwrap();

    // Run for > FAR_PROCESS_SECONDS (140 s) so both periodic package evaluation and Low-process
    // updates run well past the initial smoke window.
    for _ in 0..280 {
        game.tick();
        ai.update(&mut game, &nav, 0.5);
    }

    assert_eq!(game.extra[&stalker].current_package, None, "expired raid package must be dropped once relocated and StalkersOn == 0");
    let unreachable: Vec<_> = ai.events.iter().filter(|e| matches!(e.event, AiEvent::Unreachable { .. })).collect();
    assert!(unreachable.is_empty(), "stalker must not log Unreachable from its holding cell: {unreachable:?}");
}
