//! AI package choice (`fnv_game::ai`) on a plugin built in memory (no game data):
//! schedules, list order, the linked-reference rule, keeping the current package, Once
//! Per Day, Must Complete, Continue If PC Near and the periodic choice. The rules and
//! their sources: docs/spec-notes/packages.md §3 and §4.

use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig};
use fnv_game::package::{flags, PackageCheck};
use fnv_game::trace::EventKind;
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

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

fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
    group(*sig, 0, body)
}

fn tes4() -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x900u32.to_le_bytes()].concat();
    let payload = write_subrecords(&[sub(b"HEDR", hedr)]);
    [&b"TES4"[..], &(payload.len() as u32).to_le_bytes(), &1u32.to_le_bytes(), &[0; 4], &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
}

fn glob(fid: u32, edid: &str, v: f32) -> Vec<u8> {
    record(b"GLOB", fid, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", v.to_le_bytes().to_vec())])
}

/// A package: general flags, type, location (type, value), schedule (hour, duration,
/// day of week) and one optional condition "GetGlobalValue `global` == 1".
struct Pack {
    id: u32,
    edid: &'static str,
    flags: u32,
    ty: u8,
    location: (i32, u32),
    hour: i8,
    duration: u32,
    dow: i8,
    global: Option<u32>,
}

const GET_GLOBAL_VALUE: u16 = 0x4A;

fn pack(p: &Pack) -> Vec<u8> {
    let mut subs = vec![
        sub(b"EDID", z(p.edid)),
        sub(b"PKDT", [&p.flags.to_le_bytes()[..], &[p.ty, 0], &[0; 6]].concat()),
        sub(b"PLDT", [p.location.0.to_le_bytes(), p.location.1.to_le_bytes(), 0i32.to_le_bytes()].concat()),
        sub(b"PSDT", [&[0xFF, p.dow as u8, 0, p.hour as u8][..], &p.duration.to_le_bytes()].concat()),
    ];
    if let Some(g) = p.global {
        let mut d = vec![0u8, 0, 0, 0];
        d.extend(1.0f32.to_le_bytes());
        d.extend(GET_GLOBAL_VALUE.to_le_bytes());
        d.extend([0, 0]);
        d.extend(g.to_le_bytes());
        d.extend([0u8; 12]);
        subs.push(sub(b"CTDA", d));
    }
    record(b"PACK", p.id, &subs)
}

fn npc(fid: u32, edid: &str, packages: &[u32]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid))];
    subs.extend(packages.iter().map(|p| sub(b"PKID", p.to_le_bytes().to_vec())));
    record(b"NPC_", fid, &subs)
}

fn placed(sig: &[u8; 4], fid: u32, edid: &str, base: u32, linked: Option<u32>) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec())];
    subs.extend(linked.map(|l| sub(b"XLKR", l.to_le_bytes().to_vec())));
    subs.push(sub(b"DATA", vec![0; 24]));
    record(sig, fid, &subs)
}

// Globals.
const G_HOUR: u32 = 0x804;
const G_DAY: u32 = 0x803;
const G_DAYS_PASSED: u32 = 0x805;
const G_SWITCH: u32 = 0x810;
// Packages.
const P_COND: u32 = 0x820;
const P_MORNING: u32 = 0x821;
const P_NIGHT: u32 = 0x822;
const P_DEFAULT: u32 = 0x823;
const P_PATROL: u32 = 0x824;
const P_NIGHT_COND: u32 = 0x825;
const P_DAY: u32 = 0x826;
const P_ONCE: u32 = 0x827;
const P_MUST: u32 = 0x828;
const P_SHOP: u32 = 0x829;
const P_SUNDAY: u32 = 0x82A;
const P_WEEKEND: u32 = 0x82B;
const P_FIND_2H: u32 = 0x82C;
const P_PATROL_3H: u32 = 0x82D;
const P_SCRIPT_3H: u32 = 0x82E;
const P_MUST_SANDBOX: u32 = 0x82F;
const P_PATROL_COND: u32 = 0x830;
// Actor bases and references.
const NPC_A: u32 = 0x840;
const NPC_B: u32 = 0x841;
const NPC_C: u32 = 0x842;
const NPC_D: u32 = 0x843;
const NPC_E: u32 = 0x844;
const NPC_F: u32 = 0x845;
const NPC_G: u32 = 0x846;
const NPC_H: u32 = 0x847;
const NPC_I: u32 = 0x848;
const NPC_J: u32 = 0x849;
const STAT: u32 = 0x850;
const CELL: u32 = 0x860;
const CELL_FAR: u32 = 0x861;
const MARKER: u32 = 0x870;
const REF_A: u32 = 0x871;
const REF_B_LINKED: u32 = 0x872;
const REF_B_PLAIN: u32 = 0x873;
const REF_C: u32 = 0x874;
const REF_D: u32 = 0x875;
const REF_E: u32 = 0x876;
const REF_F: u32 = 0x877;
const REF_G: u32 = 0x878;
const REF_FAR: u32 = 0x879;
const REF_H: u32 = 0x87A;
const REF_I: u32 = 0x87B;
const REF_J: u32 = 0x87C;

const TRAVEL: u8 = 6;
const SLEEP: u8 = 4;
const SANDBOX: u8 = 12;
const PATROL: u8 = 13;
const NEAR_EDITOR: (i32, u32) = (3, 0);
const NEAR_LINKED: (i32, u32) = (6, 0);

fn plugin_bytes() -> Vec<u8> {
    let globs = [
        glob(0x801, "GameYear", 2281.0),
        glob(0x802, "GameMonth", 9.0),
        glob(G_DAY, "GameDay", 19.0),
        glob(G_HOUR, "GameHour", 12.0),
        glob(G_DAYS_PASSED, "GameDaysPassed", 5.0),
        glob(0x806, "TimeScale", 30.0),
        glob(G_SWITCH, "GSwitch", 0.0),
    ]
    .concat();
    let p = |id, edid, flags, ty, location, hour, duration, dow, global| pack(&Pack { id, edid, flags, ty, location, hour, duration, dow, global });
    let packs = [
        p(P_COND, "Conditional", 0, TRAVEL, NEAR_EDITOR, -1, 0, -1, Some(G_SWITCH)),
        p(P_MORNING, "Morning8x4", 0, TRAVEL, NEAR_EDITOR, 8, 4, -1, None),
        p(P_NIGHT, "Night22x8", 0, SLEEP, NEAR_EDITOR, 22, 8, -1, None),
        p(P_DEFAULT, "Default", 0, SANDBOX, NEAR_EDITOR, -1, 0, -1, None),
        p(P_PATROL, "PatrolLinked", 0, PATROL, NEAR_LINKED, -1, 0, -1, None),
        p(P_NIGHT_COND, "NightCond22x8", 0, SLEEP, NEAR_EDITOR, 22, 8, -1, Some(G_SWITCH)),
        p(P_DAY, "Day6x16", 0, SANDBOX, NEAR_EDITOR, 6, 16, -1, None),
        p(P_ONCE, "OnceADay", flags::ONCE_PER_DAY, TRAVEL, NEAR_EDITOR, -1, 0, -1, Some(G_SWITCH)),
        p(P_MUST, "MustComplete", flags::MUST_COMPLETE, TRAVEL, NEAR_EDITOR, -1, 0, -1, Some(G_SWITCH)),
        p(P_SHOP, "Shop8x12", flags::CONTINUE_IF_PC_NEAR, TRAVEL, NEAR_EDITOR, 8, 12, -1, None),
        p(P_SUNDAY, "Sunday", 0, TRAVEL, NEAR_EDITOR, -1, 0, 0, None),
        p(P_WEEKEND, "Weekend", 0, TRAVEL, NEAR_EDITOR, -1, 0, 8, None),
        p(P_FIND_2H, "FindOnceADay2h", flags::ONCE_PER_DAY, 0, NEAR_EDITOR, -1, 2, -1, None),
        p(P_PATROL_3H, "SandboxOnceADay3h", flags::ONCE_PER_DAY, SANDBOX, NEAR_EDITOR, -1, 3, -1, None),
        p(P_SCRIPT_3H, "MustTravel3h", flags::MUST_COMPLETE, TRAVEL, NEAR_EDITOR, -1, 3, -1, None),
        p(P_MUST_SANDBOX, "MustSandbox", flags::MUST_COMPLETE, SANDBOX, NEAR_EDITOR, -1, 0, -1, None),
        p(P_PATROL_COND, "ConditionalLinkedPatrol", 0, PATROL, NEAR_LINKED, -1, 0, -1, Some(G_SWITCH)),
    ]
    .concat();
    let npcs = [
        npc(0x7, "Player", &[]),
        npc(NPC_A, "A", &[P_COND, P_MORNING, P_NIGHT, P_DEFAULT]),
        npc(NPC_B, "B", &[P_PATROL, P_DEFAULT]),
        npc(NPC_C, "C", &[P_NIGHT_COND, P_DAY]),
        npc(NPC_D, "D", &[P_ONCE, P_DEFAULT]),
        npc(NPC_E, "E", &[P_MUST, P_DEFAULT]),
        npc(NPC_F, "F", &[P_SHOP, P_DEFAULT]),
        npc(NPC_G, "G", &[P_SUNDAY, P_WEEKEND, P_DEFAULT]),
        npc(NPC_H, "H", &[P_FIND_2H, P_PATROL_3H, P_DEFAULT]),
        npc(NPC_I, "I", &[P_NIGHT_COND]),
        npc(NPC_J, "J", &[P_PATROL_COND]),
    ]
    .concat();
    let stat = record(b"STAT", STAT, &[sub(b"EDID", z("Marker"))]);
    let refs = [
        placed(b"REFR", MARKER, "MarkerREF", STAT, None),
        placed(b"ACHR", REF_A, "ARef", NPC_A, None),
        placed(b"ACHR", REF_B_LINKED, "BLinkedRef", NPC_B, Some(MARKER)),
        placed(b"ACHR", REF_B_PLAIN, "BPlainRef", NPC_B, None),
        placed(b"ACHR", REF_C, "CRef", NPC_C, None),
        placed(b"ACHR", REF_D, "DRef", NPC_D, None),
        placed(b"ACHR", REF_E, "ERef", NPC_E, None),
        placed(b"ACHR", REF_F, "FRef", NPC_F, None),
        placed(b"ACHR", REF_G, "GRef", NPC_G, None),
        placed(b"ACHR", REF_H, "HRef", NPC_H, None),
        placed(b"ACHR", REF_I, "IRef", NPC_I, None),
        placed(b"ACHR", REF_J, "JRef", NPC_J, None),
    ]
    .concat();
    let cell = |id: u32, edid: &str, refs: &[u8]| {
        let rec = record(b"CELL", id, &[sub(b"EDID", z(edid))]);
        [rec, group(id.to_le_bytes(), 6, &group(id.to_le_bytes(), 9, refs))].concat()
    };
    let cells = [cell(CELL, "TestCell", &refs), cell(CELL_FAR, "FarCell", &placed(b"ACHR", REF_FAR, "FarRef", NPC_A, None))].concat();
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

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Packages.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture, package_interval: Option<f64>) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, package_interval, ..GameConfig::default() });
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(CELL));
    g.player_entered(Some(FormId(CELL)));
    g
}

fn set(g: &mut Game, global: u32, v: f32) {
    g.world.set_global(FormId(global), v).unwrap();
}

fn id(x: u32) -> FormId {
    FormId(x)
}

#[test]
fn the_first_package_in_its_window_whose_conditions_pass_runs() {
    let f = fixture();
    assert!(f.data.issues.is_empty(), "{:?}", f.data.issues.to_json());
    let mut g = game(&f, None);
    let a = id(REF_A);
    // Noon: the conditional package fails, the morning and night ones are outside
    // their windows.
    assert_eq!(g.check_package(a, id(P_COND)), PackageCheck::ConditionsFail);
    assert_eq!(g.check_package(a, id(P_MORNING)), PackageCheck::OutOfSchedule);
    assert_eq!(g.check_package(a, id(P_NIGHT)), PackageCheck::OutOfSchedule);
    assert_eq!(g.check_package(a, id(P_DEFAULT)), PackageCheck::Valid);
    assert_eq!(g.check_package(a, id(MARKER)), PackageCheck::Missing);
    assert_eq!(g.evaluate_package(a), Some(id(P_DEFAULT)));
    for (hour, want) in [(8.0, P_MORNING), (11.9, P_MORNING), (12.0, P_DEFAULT), (22.0, P_NIGHT), (23.5, P_NIGHT), (0.5, P_NIGHT), (5.9, P_NIGHT), (6.0, P_DEFAULT), (7.9, P_DEFAULT)] {
        set(&mut g, G_HOUR, hour);
        assert_eq!(g.evaluate_package(a), Some(id(want)), "at {hour}");
    }
    // A package earlier in the list wins as soon as its conditions pass, whatever the
    // hour.
    set(&mut g, G_SWITCH, 1.0);
    assert_eq!(g.evaluate_package(a), Some(id(P_COND)));
    set(&mut g, G_HOUR, 9.0);
    assert_eq!(g.evaluate_package(a), Some(id(P_COND)));
    set(&mut g, G_SWITCH, 0.0);
    assert_eq!(g.evaluate_package(a), Some(id(P_MORNING)));
    // Each change is traced once.
    let chosen = g.events.iter().filter(|e| matches!(e.kind, EventKind::PackageChosen { actor, .. } if actor == a)).count();
    assert_eq!(chosen, 7);
}

#[test]
fn a_package_at_the_linked_reference_needs_one() {
    let f = fixture();
    let mut g = game(&f, None);
    assert_eq!(g.check_package(id(REF_B_LINKED), id(P_PATROL)), PackageCheck::Valid);
    assert_eq!(g.evaluate_package(id(REF_B_LINKED)), Some(id(P_PATROL)));
    assert_eq!(g.check_package(id(REF_B_PLAIN), id(P_PATROL)), PackageCheck::NoLinkedRef);
    assert_eq!(g.evaluate_package(id(REF_B_PLAIN)), Some(id(P_DEFAULT)));
}

#[test]
fn with_no_valid_package_the_actor_keeps_the_one_it_runs() {
    let f = fixture();
    let mut g = game(&f, None);
    let c = id(REF_C);
    // By day the day package; at night the night package's condition fails and the day
    // package is outside its window: the actor keeps the day package.
    assert_eq!(g.evaluate_package(c), Some(id(P_DAY)));
    set(&mut g, G_HOUR, 23.0);
    assert_eq!(g.check_package(c, id(P_DAY)), PackageCheck::OutOfSchedule);
    assert_eq!(g.evaluate_package(c), Some(id(P_DAY)));
    set(&mut g, G_SWITCH, 1.0);
    assert_eq!(g.evaluate_package(c), Some(id(P_NIGHT_COND)));
    // An actor that never ran a package has none to keep.
    let f2 = fixture();
    let mut g2 = game(&f2, None);
    set(&mut g2, G_HOUR, 23.0);
    assert_eq!(g2.evaluate_package(c), None);
    assert_eq!(g2.current_package(c), None);
}

#[test]
fn a_current_package_with_a_target_problem_is_kept_while_its_conditions_pass() {
    let f = fixture();
    let actor = id(REF_J);
    let package = id(P_PATROL_COND);
    for check in [PackageCheck::NoLinkedRef, PackageCheck::Unreachable] {
        let mut g = game(&f, None);
        set(&mut g, G_SWITCH, 1.0);
        g.jip2.links.insert(actor, id(MARKER));
        assert_eq!(g.evaluate_package(actor), Some(package));

        if check == PackageCheck::NoLinkedRef {
            g.jip2.links.remove(&actor);
        } else {
            // Seed the cooldown with a current package to exercise the keep rule.
            // block_package clears the current package before reevaluation.
            g.extra.get_mut(&actor).unwrap().package_blocked.insert(package, 6.0);
        }
        assert_eq!(g.check_package(actor, package), check);
        assert_eq!(g.evaluate_package(actor), Some(package), "{check:?}");

        set(&mut g, G_SWITCH, 0.0);
        assert_eq!(g.check_package(actor, package), check);
        assert_eq!(g.evaluate_package(actor), None, "{check:?}");
        assert_eq!(g.current_package(actor), None);
    }
}

#[test]
fn once_per_day_is_not_chosen_again_within_a_day() {
    let f = fixture();
    let mut g = game(&f, None);
    let d = id(REF_D);
    assert_eq!(g.evaluate_package(d), Some(id(P_DEFAULT)));
    set(&mut g, G_SWITCH, 1.0);
    assert_eq!(g.evaluate_package(d), Some(id(P_ONCE)));
    // Still running: it may go on.
    assert_eq!(g.check_package(d, id(P_ONCE)), PackageCheck::Valid);
    assert_eq!(g.evaluate_package(d), Some(id(P_ONCE)));
    // It ends; its conditions pass again within the day: not chosen.
    set(&mut g, G_SWITCH, 0.0);
    assert_eq!(g.evaluate_package(d), Some(id(P_DEFAULT)));
    set(&mut g, G_SWITCH, 1.0);
    set(&mut g, G_DAYS_PASSED, 5.9);
    assert_eq!(g.check_package(d, id(P_ONCE)), PackageCheck::OncePerDay);
    assert_eq!(g.evaluate_package(d), Some(id(P_DEFAULT)));
    // A day after it was chosen (at 5.0): chosen again.
    set(&mut g, G_DAYS_PASSED, 6.01);
    assert_eq!(g.evaluate_package(d), Some(id(P_ONCE)));
}

/// With no valid package an actor keeps the one it runs while only the schedule rules
/// that one out, not once its own conditions fail: then it has none (Mister Burke after
/// MS11's arrest talk gets out of his chair, issue #2235).
#[test]
fn a_package_whose_conditions_fail_is_not_kept() {
    let f = fixture();
    let mut g = game(&f, None);
    let c = id(REF_C);
    set(&mut g, G_HOUR, 23.0);
    set(&mut g, G_SWITCH, 1.0);
    assert_eq!(g.evaluate_package(c), Some(id(P_NIGHT_COND)));
    set(&mut g, G_SWITCH, 0.0);
    assert_eq!(g.check_package(c, id(P_NIGHT_COND)), PackageCheck::ConditionsFail);
    assert_eq!(g.evaluate_package(c), None);
    assert_eq!(g.extra[&c].current_package, None);
}

/// A Once Per Day package that reached its goal is done for the day: though still
/// current and its conditions still passing, it is not chosen again and the next valid
/// package takes over (TTW's CG02AndyCutCake, whose change script sets the cake stage
/// once Andy leaves it; issue #2221).
#[test]
fn a_finished_once_per_day_package_gives_way() {
    let f = fixture();
    let mut g = game(&f, None);
    let d = id(REF_D);
    set(&mut g, G_SWITCH, 1.0);
    assert_eq!(g.evaluate_package(d), Some(id(P_ONCE)));
    g.package_goal_met(d);
    assert_eq!(g.check_package(d, id(P_ONCE)), PackageCheck::OncePerDay);
    assert_eq!(g.evaluate_package(d), Some(id(P_DEFAULT)));
}

#[test]
fn must_complete_holds_the_periodic_choice_but_not_evaluate_package() {
    let f = fixture();
    let mut g = game(&f, None);
    let e = id(REF_E);
    set(&mut g, G_SWITCH, 1.0);
    assert_eq!(g.update_package(e), Some(id(P_MUST)));
    // Its conditions fail now, but it has not reached its goal.
    set(&mut g, G_SWITCH, 0.0);
    assert_eq!(g.package_held(e), Some("must complete"));
    assert_eq!(g.update_package(e), Some(id(P_MUST)));
    // The goal is met: the next periodic choice replaces it.
    g.package_goal_met(e);
    assert_eq!(g.package_held(e), None);
    assert_eq!(g.update_package(e), Some(id(P_DEFAULT)));
    // EvaluatePackage drops even an unfinished Must Complete package.
    set(&mut g, G_SWITCH, 1.0);
    assert_eq!(g.update_package(e), Some(id(P_MUST)));
    set(&mut g, G_SWITCH, 0.0);
    assert_eq!(g.update_package(e), Some(id(P_MUST)));
    assert_eq!(g.evaluate_package(e), Some(id(P_DEFAULT)));
}

#[test]
fn continue_if_pc_near_keeps_the_package_while_the_player_is_in_the_cell() {
    let f = fixture();
    let mut g = game(&f, None);
    let shop = id(REF_F);
    assert_eq!(g.update_package(shop), Some(id(P_SHOP)));
    // Closing time with the player in the shop: it stays open.
    set(&mut g, G_HOUR, 21.0);
    assert_eq!(g.package_held(shop), Some("continue if PC near"));
    assert_eq!(g.update_package(shop), Some(id(P_SHOP)));
    // The player leaves the cell.
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(CELL_FAR));
    assert_eq!(g.package_held(shop), None);
    assert_eq!(g.update_package(shop), Some(id(P_DEFAULT)));
}

#[test]
fn day_of_week_schedules_follow_the_calendar() {
    let f = fixture();
    let mut g = game(&f, None);
    let a = id(REF_G);
    // Seven days in a row: the Sunday package on one of them, the weekend package on
    // the day before it (Saturday), the default on the other five.
    let mut seen = Vec::new();
    for day in 10..17 {
        set(&mut g, G_DAY, day as f32);
        seen.push(g.evaluate_package(a).unwrap().0);
    }
    assert_eq!(seen.iter().filter(|p| **p == P_SUNDAY).count(), 1, "{seen:x?}");
    assert_eq!(seen.iter().filter(|p| **p == P_WEEKEND).count(), 1, "{seen:x?}");
    assert_eq!(seen.iter().filter(|p| **p == P_DEFAULT).count(), 5, "{seen:x?}");
    let sunday = seen.iter().position(|p| *p == P_SUNDAY).unwrap();
    let saturday = seen.iter().position(|p| *p == P_WEEKEND).unwrap();
    assert_eq!((saturday + 1) % 7, sunday);
    // The weekday agrees with GetDayOfWeek's: 13 October 2281 is a Sunday on the game's
    // calendar (observed, `functions::misc::weekday`), so the Sunday of 10..17 is the 13th.
    assert_eq!(sunday, 3);
}

#[test]
fn the_periodic_choice_follows_the_clock() {
    let f = fixture();
    let mut g = game(&f, Some(10.0));
    let a = id(REF_A);
    // The actors of the loaded cell choose at the first tick; the far cell's do not.
    g.tick();
    assert_eq!(g.extra.get(&a).and_then(|e| e.current_package), Some(id(P_DEFAULT)));
    assert_eq!(g.extra.get(&id(REF_B_LINKED)).and_then(|e| e.current_package), Some(id(P_PATROL)));
    assert!(g.extra.get(&id(REF_FAR)).is_none_or(|e| e.current_package.is_none()));
    // The hour moves into the night window: within one interval the actor switches,
    // not at once.
    set(&mut g, G_HOUR, 22.0);
    g.tick();
    let mut frames = 1u64;
    while g.extra[&a].current_package != Some(id(P_NIGHT)) {
        g.tick();
        frames += 1;
        assert!(frames < 60 * 11, "no switch within the interval");
    }
    // Once a switch has happened the next one is a full interval later.
    set(&mut g, G_HOUR, 7.0);
    g.frames(60 * 9);
    assert_eq!(g.extra[&a].current_package, Some(id(P_NIGHT)));
    g.frames(60 * 2);
    assert_eq!(g.extra[&a].current_package, Some(id(P_DEFAULT)));
    // A dead actor no longer chooses.
    g.extra.entry(a).or_default().dead = true;
    set(&mut g, G_HOUR, 9.0);
    g.frames(60 * 12);
    assert_eq!(g.extra[&a].current_package, Some(id(P_DEFAULT)));
    // An actor that runs a package goes on choosing after the player has left.
    let far = id(REF_FAR);
    g.evaluate_package(far);
    assert_eq!(g.extra[&far].current_package, Some(id(P_MORNING)));
    set(&mut g, G_HOUR, 13.0);
    g.frames(60 * 11);
    assert_eq!(g.extra[&far].current_package, Some(id(P_DEFAULT)));
    // Without an interval nothing chooses by itself.
    let mut still = game(&f, None);
    still.frames(60 * 12);
    assert!(still.extra.get(&a).is_none_or(|e| e.current_package.is_none()));
}

/// ResetAI, GetCurrentAIPackage and GetCurrentAIProcedure (docs/design/script-stubs.md
/// §3; the codes are the GECK wiki's tables).
#[test]
fn reset_ai_drops_the_script_package_and_the_codes_follow_the_package() {
    use fnv_game::functions::Ctx;
    use fnv_game::game::FurnitureUse;
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    let mut g = game(&f, None);
    let a = id(REF_A);
    let call = |g: &mut Game, op: u16, args: &[ArgValue]| -> f64 {
        let ctx = Ctx { unit: "test".into(), opcode: op, on: Some(a), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, args).num()
    };
    let (package, procedure) = (0x106E, 0x108F);
    // Noon: the sandbox package.
    assert_eq!((call(&mut g, package, &[]), call(&mut g, procedure, &[])), (36.0, 45.0));
    // A script's travel package: travelling, then done at its goal.
    call(&mut g, 0x1097, &[ArgValue::Value(Value::Ref(P_MORNING))]);
    assert_eq!((call(&mut g, package, &[]), call(&mut g, procedure, &[])), (14.0, 0.0));
    g.package_goal_met(a);
    assert_eq!(call(&mut g, procedure, &[]), 17.0);
    // Seated at its goal it is still done (the original's Travel packages to chairs).
    g.set_furniture_use(a, Some(FurnitureUse { reference: id(MARKER), marker_id: 20, state: 3 }));
    assert_eq!(call(&mut g, procedure, &[]), 17.0);
    g.set_furniture_use(a, None);
    // ResetAI: the script package is gone and the list is chosen from again.
    call(&mut g, 0x11FA, &[]);
    assert!(g.extra[&a].script_package.is_none());
    assert_eq!((call(&mut g, package, &[]), call(&mut g, procedure, &[])), (36.0, 45.0));
    // The sandbox as the AI layer runs it (docs/spec-notes/packages.md §4, procedure codes): on the way
    // 0, by what it does in the area, and its goal met is not 17; a seat with no AI
    // layer 48.
    use fnv_game::ai::{AiPhase, SandboxPhase};
    let on = |marker_id, state| Some(FurnitureUse { reference: id(MARKER), marker_id, state });
    for (phase, code) in [
        (AiPhase::Travelling, 0.0),
        (AiPhase::Sandbox(SandboxPhase::Standing), 0.0),
        (AiPhase::Sandbox(SandboxPhase::Wandering), 7.0),
        (AiPhase::Sandbox(SandboxPhase::ToObject), 0.0),
        (AiPhase::Sandbox(SandboxPhase::AtIdleMarker), 46.0),
        (AiPhase::Sandbox(SandboxPhase::Sitting), 48.0),
        (AiPhase::Sandbox(SandboxPhase::Eating), 10.0),
        (AiPhase::Sandbox(SandboxPhase::Sleeping), 8.0),
        (AiPhase::Sandbox(SandboxPhase::GotUp), 21.0),
    ] {
        g.set_ai_phase(a, Some(phase));
        assert_eq!(call(&mut g, procedure, &[]), code, "{phase:?}");
    }
    g.package_goal_met(a);
    assert_eq!(call(&mut g, procedure, &[]), 21.0);
    g.set_ai_phase(a, None);
    g.set_furniture_use(a, on(20, 3));
    assert_eq!(call(&mut g, procedure, &[]), 48.0);
    g.set_furniture_use(a, None);
    // Night: the sleep package, 8 walking to the bed and lying on it, getting up 21.
    set(&mut g, G_HOUR, 23.0);
    g.evaluate_package(a);
    g.set_ai_phase(a, Some(AiPhase::Travelling));
    assert_eq!((call(&mut g, package, &[]), call(&mut g, procedure, &[])), (4.0, 8.0));
    g.set_furniture_use(a, on(1, 3));
    assert_eq!(call(&mut g, procedure, &[]), 8.0);
    g.set_furniture_use(a, on(1, 4));
    assert_eq!(call(&mut g, procedure, &[]), 21.0);
    // In combat 13; fleeing from it 16 FLEE, the package still 5 COMBAT
    // (docs/spec-notes/idles.md §13); fighting again 13.
    g.set_furniture_use(a, None);
    g.start_combat(a, FormId::PLAYER_REF);
    assert_eq!((call(&mut g, package, &[]), call(&mut g, procedure, &[])), (5.0, 13.0));
    g.start_flight(a, FormId::PLAYER_REF, [0.0; 3]);
    assert_eq!((call(&mut g, package, &[]), call(&mut g, procedure, &[])), (5.0, 16.0));
    g.end_flight(a);
    assert_eq!(call(&mut g, procedure, &[]), 13.0);
}

/// A script package stays chosen until its goal is met; the next choice after that
/// removes it and goes back to the list (owner-decision-captures.md §7, od_06: Ringo's
/// DefaultStayAtEditorLocation read DONE, then his list package with no ResetAI).
#[test]
fn a_script_package_whose_goal_was_met_is_removed_at_the_next_choice() {
    use fnv_game::functions::Ctx;
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    let mut g = game(&f, None);
    let a = id(REF_A);
    let call = |g: &mut Game, op: u16, args: &[ArgValue]| -> f64 {
        let ctx = Ctx { unit: "test".into(), opcode: op, on: Some(a), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, args).num()
    };
    let (package, procedure) = (0x106E, 0x108F);
    call(&mut g, 0x1097, &[ArgValue::Value(Value::Ref(P_MORNING))]);
    // Not done yet: a choice keeps it (od_08: EvaluatePackage keeps a script package).
    assert_eq!(g.evaluate_package(a), Some(id(P_MORNING)));
    g.package_goal_met(a);
    assert_eq!((call(&mut g, package, &[]), call(&mut g, procedure, &[])), (14.0, 17.0));
    // The next choice: back to the noon sandbox of the list.
    g.evaluate_package(a);
    assert!(g.extra[&a].script_package.is_none());
    assert_eq!((call(&mut g, package, &[]), call(&mut g, procedure, &[])), (36.0, 45.0));
}

/// The same script package added again after its goal was met, before any choice dropped
/// it, begins afresh: its goal is forgotten and the AI is asked to take it up anew. Added
/// again while it still runs, nothing changes (issue #2424: the Lucky 38 elevator's
/// Dialogue package, added on every talk, never opened a second conversation after Stay
/// Here, and its script waited for one for ever).
#[test]
fn a_script_package_added_again_after_its_goal_begins_afresh() {
    use fnv_game::functions::Ctx;
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    for interval in [None, Some(10.0)] {
        let mut g = game(&f, interval);
        let a = id(REF_A);
        let add = |g: &mut Game| {
            let ctx = Ctx { unit: "test".into(), opcode: 0x1097, on: Some(a), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
            g.call_function(&ctx, &[ArgValue::Value(Value::Ref(P_MORNING))]);
        };
        add(&mut g);
        let _ = g.take_ai_reset(a);
        // Still running: a second call is no restart.
        add(&mut g);
        assert!(!g.take_ai_reset(a), "{interval:?}");
        g.package_goal_met(a);
        assert!(g.extra[&a].package_goal_met);
        add(&mut g);
        assert!(!g.extra[&a].package_goal_met, "{interval:?}");
        assert!(g.take_ai_reset(a), "{interval:?}");
        assert_eq!(g.extra[&a].script_package, Some(id(P_MORNING)));
        assert_eq!(g.current_package(a), Some(id(P_MORNING)));
        // Not dropped at the next choice: its goal is to be met again first.
        if interval.is_some() {
            assert_eq!(g.evaluate_package(a), Some(id(P_MORNING)));
        }
    }
}

/// A package type with no done state (Sandbox, Sleep, Eat, Guard, Wander, Follow,
/// Accompany) does not meet its goal on arrival: no end event, the procedure never reads
/// 17 DONE, a script package of that type stays through the choices, and Must Complete
/// keeps holding it (issue #2012: the original's GSChetRef kept a script-added
/// DefaultSandboxEditorLocation256 for 1.5 game hours with no OnPackageDone).
#[test]
fn a_package_with_no_done_state_does_not_end_on_arrival() {
    use fnv_game::functions::Ctx;
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    let mut g = game(&f, None);
    let a = id(REF_A);
    let call = |g: &mut Game, op: u16, args: &[ArgValue]| -> f64 {
        let ctx = Ctx { unit: "test".into(), opcode: op, on: Some(a), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, args).num()
    };
    let procedure = 0x108F;
    call(&mut g, 0x1097, &[ArgValue::Value(Value::Ref(P_DEFAULT))]);
    assert!(!g.package_has_done_state(id(P_DEFAULT)) && g.package_has_done_state(id(P_MORNING)));
    let from = g.events.len();
    g.package_goal_met(a);
    assert!(!g.extra[&a].package_goal_met);
    assert_ne!(call(&mut g, procedure, &[]), 17.0);
    for _ in 0..3 {
        assert_eq!(g.evaluate_package(a), Some(id(P_DEFAULT)));
    }
    assert_eq!(g.extra[&a].script_package, Some(id(P_DEFAULT)));
    assert!(!g.events[from..].iter().any(|e| matches!(e.kind, EventKind::PackageChosen { .. })), "no change after arrival");
    // Must Complete: still held after arriving.
    call(&mut g, 0x1097, &[ArgValue::Value(Value::Ref(P_MUST_SANDBOX))]);
    g.package_goal_met(a);
    assert_eq!(g.package_held(a), Some("must complete"));
    // A type with a done state still ends at its goal.
    call(&mut g, 0x1097, &[ArgValue::Value(Value::Ref(P_MORNING))]);
    g.package_goal_met(a);
    assert!(g.extra[&a].package_goal_met);
    assert_eq!(call(&mut g, procedure, &[]), 17.0);
}

/// A script package with no done state and a schedule duration ends when that runs out,
/// though its goal is never met (issue #2012).
#[test]
fn a_timed_script_package_with_no_done_state_ends_on_time() {
    use fnv_game::functions::Ctx;
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    let mut g = game(&f, Some(10.0));
    let a = id(REF_A);
    g.tick();
    let ctx = Ctx { unit: "test".into(), opcode: 0x1097, on: Some(a), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[ArgValue::Value(Value::Ref(P_PATROL_3H))]);
    assert_eq!(g.extra[&a].current_package, Some(id(P_PATROL_3H)));
    g.package_goal_met(a);
    set(&mut g, G_DAYS_PASSED, 5.1);
    g.tick();
    assert_eq!(g.extra[&a].current_package, Some(id(P_PATROL_3H)), "within its 3 hours");
    set(&mut g, G_DAYS_PASSED, 5.13);
    g.tick();
    assert!(g.extra[&a].script_package.is_none());
    assert_eq!(g.extra[&a].current_package, Some(id(P_DEFAULT)));
}

/// A script package with a schedule duration (no starting hour) stays after its goal is
/// met until that duration has run out, then the actor goes back to its list (the
/// Strip's SS2 scene: VStripSS2WallDrunk3, a 3 h Must Complete travel, stayed current
/// after its trooper arrived; npc-behaviour-compare.md §10.1).
#[test]
fn a_timed_script_package_stays_after_its_goal_until_its_duration_is_up() {
    use fnv_game::functions::Ctx;
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    let mut g = game(&f, Some(10.0));
    let a = id(REF_A);
    g.tick();
    let ctx = Ctx { unit: "test".into(), opcode: 0x1097, on: Some(a), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[ArgValue::Value(Value::Ref(P_SCRIPT_3H))]);
    assert_eq!(g.extra[&a].current_package, Some(id(P_SCRIPT_3H)));
    assert!(g.extra[&a].package_ends.is_some_and(|(p, t)| p == id(P_SCRIPT_3H) && (t - (5.0 + 3.0 / 24.0)).abs() < 1e-4));
    // Arrived an hour on: it stays, through the periodic choice and EvaluatePackage.
    set(&mut g, G_DAYS_PASSED, 5.04);
    g.package_goal_met(a);
    g.tick();
    assert_eq!(g.evaluate_package(a), Some(id(P_SCRIPT_3H)));
    assert_eq!(g.extra[&a].script_package, Some(id(P_SCRIPT_3H)));
    // Past its 3 hours: removed, back to the list's noon sandbox.
    set(&mut g, G_DAYS_PASSED, 5.13);
    g.tick();
    assert!(g.extra[&a].script_package.is_none());
    assert_eq!(g.extra[&a].current_package, Some(id(P_DEFAULT)));
}

/// SetPackDuration ends the current package that many game minutes after the call (it
/// is chosen again, here the same one, which starts afresh); SetActorsAI 0 keeps the
/// periodic choice away from the actor until SetActorsAI 1 (docs/design/script-stubs.md
/// §3.2).
#[test]
fn set_pack_duration_ends_the_package_and_set_actors_ai_stops_the_choice() {
    use fnv_game::functions::Ctx;
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    let mut g = game(&f, Some(10.0));
    let a = id(REF_A);
    let call = |g: &mut Game, op: u16, x: f64| {
        let ctx = Ctx { unit: "test".into(), opcode: op, on: Some(a), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, &[ArgValue::Value(Value::Num(x))]);
    };
    g.tick();
    assert_eq!(g.extra[&a].current_package, Some(id(P_DEFAULT)));
    call(&mut g, 0x1122, 30.0);
    let until = g.extra[&a].package_ends;
    assert!(until.is_some_and(|(p, t)| p == id(P_DEFAULT) && (t - (5.0 + 30.0 / 1440.0)).abs() < 1e-4), "{until:?}");
    let let_go = |g: &Game, from: usize| g.events[from..].iter().any(|e| matches!(e.kind, EventKind::PackageChosen { actor, package: None } if actor == a));
    // 14 game minutes on: still running.
    set(&mut g, G_DAYS_PASSED, 5.01);
    let from = g.events.len();
    g.tick();
    assert!(!let_go(&g, from) && g.extra[&a].package_ends.is_some());
    // 43 minutes on: it ends and the actor chooses again (the same package, afresh).
    set(&mut g, G_DAYS_PASSED, 5.03);
    let from = g.events.len();
    g.tick();
    assert!(let_go(&g, from), "the timed package was let go");
    assert_eq!((g.extra[&a].current_package, g.extra[&a].package_ends), (Some(id(P_DEFAULT)), None));

    // AI off: the night window comes and goes without a choice; on again, it chooses.
    call(&mut g, 0x118B, 0.0);
    assert!(g.extra[&a].ai_off);
    set(&mut g, G_HOUR, 22.0);
    g.frames(60 * 12);
    assert_eq!(g.extra[&a].current_package, Some(id(P_DEFAULT)));
    call(&mut g, 0x118B, 1.0);
    g.frames(60 * 12);
    assert_eq!(g.extra[&a].current_package, Some(id(P_NIGHT)));
}

/// A list package with a duration and no starting hour ends that many game hours after
/// it was chosen; Once Per Day keeps it from being chosen again that day, so the next
/// package of the list takes over (the Goodsprings geckos' chain: find food 2 h, patrol
/// 3 h, then a sandbox), and a day later the first one is valid again.
#[test]
fn a_schedule_duration_ends_the_package_and_the_next_one_takes_over() {
    let f = fixture();
    let mut g = game(&f, Some(10.0));
    let h = id(REF_H);
    g.tick();
    assert_eq!(g.extra[&h].current_package, Some(id(P_FIND_2H)));
    assert!(g.extra[&h].package_ends_by_schedule);
    // Past its 2 hours with its goal not met it runs on (the original's geckos,
    // owner-decision-captures.md §7); once the goal is met it ends at once.
    set(&mut g, G_DAYS_PASSED, 5.3);
    g.tick();
    assert_eq!(g.extra[&h].current_package, Some(id(P_FIND_2H)), "a find not done runs on");
    g.package_goal_met(h);
    g.tick();
    assert_eq!(g.extra[&h].current_package, Some(id(P_PATROL_3H)));
    // The patrol's own 3 hours count from its choice; done before them, it waits for them.
    g.package_goal_met(h);
    for (days, want) in [(5.35, P_PATROL_3H), (5.43, P_DEFAULT), (5.9, P_DEFAULT)] {
        set(&mut g, G_DAYS_PASSED, days);
        g.tick();
        assert_eq!(g.extra[&h].current_package, Some(id(want)), "at day {days}");
    }
    // A day after the first choice the find package is valid again.
    set(&mut g, G_DAYS_PASSED, 6.01);
    assert_eq!(g.evaluate_package(h), Some(id(P_FIND_2H)));
    // Its timer starts afresh.
    assert!(g.extra[&h].package_ends.is_some_and(|(p, t)| p == id(P_FIND_2H) && (t - (6.01 + 2.0 / 24.0)).abs() < 1e-4));
}

/// EnableNavMesh, DisableNavmesh and setAllReachable keep their switches, and the save
/// keeps them (docs/design/script-stubs.md §3.5).
#[test]
fn navmesh_switches_are_kept_and_saved() {
    use fnv_game::functions::Ctx;
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    let mut g = game(&f, None);
    let call = |g: &mut Game, op: u16, args: &[ArgValue]| {
        let ctx = Ctx { unit: "test".into(), opcode: op, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, args);
    };
    let (enable, disable, reachable) = (0x11C0, 0x11BF, 0x1159);
    call(&mut g, disable, &[ArgValue::Value(Value::Ref(0xA553))]);
    call(&mut g, disable, &[ArgValue::Value(Value::Ref(0xA555))]);
    call(&mut g, enable, &[ArgValue::Value(Value::Ref(0xA555))]);
    call(&mut g, reachable, &[ArgValue::Value(Value::Num(1.0))]);
    let want: std::collections::BTreeMap<FormId, bool> = [(FormId(0xA553), false), (FormId(0xA555), true)].into();
    assert_eq!(g.nav_switches.navmeshes, want);
    // A raw FormID as a number, and the console's words.
    call(&mut g, disable, &[ArgValue::Value(Value::Num(0xBE880 as f64))]);
    assert_eq!(g.nav_switches.navmeshes.get(&FormId(0xBE880)), Some(&false));
    fnv_game::functions::console::call_words(&mut g, None, "enablenavmesh", &["000BE880".into()], "test").unwrap();
    assert_eq!(g.nav_switches.navmeshes.get(&FormId(0xBE880)), Some(&true));
    fnv_game::functions::console::call_words(&mut g, None, "disablenavmesh", &["0010A1DA".into()], "test").unwrap();
    assert_eq!(g.nav_switches.navmeshes.get(&FormId(0x10A1DA)), Some(&false));
    g.nav_switches.navmeshes.remove(&FormId(0xBE880));
    g.nav_switches.navmeshes.remove(&FormId(0x10A1DA));
    assert!(g.nav_switches.all_reachable);
    assert_eq!(g.nav_switches.by_id(), [(0xA553, false), (0xA555, true)].into());
    let snap: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.snapshot()).unwrap()).unwrap();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let back = Game::restore(&f.data, world, GameConfig::default(), &snap).unwrap();
    assert_eq!(back.nav_switches, g.nav_switches);
    // None switched: nothing written to the save.
    let fresh = game(&f, None);
    assert!(serde_json::to_value(fresh.snapshot()).unwrap().get("nav_switches").is_none());
}

/// The runner's `package {event: done}` stands in for the AI reaching the goal of the
/// actor's current package: GetCurrentAIProcedure then reads 17 DONE, as after
/// `Game::package_goal_met`; a package the actor is not running is not marked. Camp
/// Golf's range packages (FalloutNV.esm PACK 0x0012FEED end) wait for the other three
/// misfits to read DONE before the range opens.
#[test]
fn the_runners_package_done_marks_the_current_packages_goal_met() {
    let f = fixture();
    let (a, morning, night) = (format!("0x{REF_A:08X}"), format!("0x{P_MORNING:08X}"), format!("0x{P_NIGHT:08X}"));
    let input: fnv_game::runner::Input = serde_json::from_value(serde_json::json!({
        "format": "fnv.script.actions/1",
        "actions": [
            {"op": "call", "line": format!("{a}.AddScriptPackage {morning}")},
            {"op": "call", "line": format!("{a}.GetCurrentAIProcedure"), "label": "travelling"},
            {"op": "package", "actor": a, "package": night, "event": "done"},
            {"op": "call", "line": format!("{a}.GetCurrentAIProcedure"), "label": "another package done"},
            {"op": "package", "actor": a, "package": morning, "event": "done"},
            {"op": "call", "line": format!("{a}.GetCurrentAIProcedure"), "label": "arrived"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let obs = out["observations"].as_array().unwrap();
    let by = |l: &str| obs.iter().find(|o| o["label"] == l).unwrap_or_else(|| panic!("no {l}"))["value"].clone();
    assert_eq!(by("travelling"), 0.0);
    assert_eq!(by("another package done"), 0.0);
    assert_eq!(by("arrived"), 17.0);
}

fn scda_unit(kind: u16, scda: &[u8], refs: &[u32]) -> Vec<Subrecord> {
    let mut schr = vec![0u8; 4];
    schr.extend((refs.len() as u32).to_le_bytes());
    schr.extend((scda.len() as u32).to_le_bytes());
    schr.extend(0u32.to_le_bytes());
    schr.extend(kind.to_le_bytes());
    schr.extend(1u16.to_le_bytes());
    let mut out = vec![sub(b"SCHR", schr)];
    if !scda.is_empty() {
        out.push(sub(b"SCDA", scda.to_vec()));
    }
    for r in refs {
        out.push(sub(b"SCRO", r.to_le_bytes().to_vec()));
    }
    out
}

fn bc_st(op: u16, body: &[u8]) -> Vec<u8> {
    [op.to_le_bytes().to_vec(), (body.len() as u16).to_le_bytes().to_vec(), body.to_vec()].concat()
}

fn bc_set_global(ref_idx: u16, expr_ascii: &[u8]) -> Vec<u8> {
    let mut target = vec![b'G'];
    target.extend(ref_idx.to_le_bytes());
    let mut body = target;
    body.extend((expr_ascii.len() as u16).to_le_bytes());
    body.extend(expr_ascii);
    bc_st(0x15, &body)
}

fn bc_inc_global(ref_idx: u16) -> Vec<u8> {
    // Compiled FNV expressions are space-prefixed postfix (RPN): ` G<idx> 1 +`.
    let mut expr = vec![b' ', b'G'];
    expr.extend(ref_idx.to_le_bytes());
    expr.extend(b" 1 +");
    bc_set_global(ref_idx, &expr)
}

fn bc_evp() -> Vec<u8> {
    // EvaluatePackage (0x105E), 0 arguments.
    bc_st(0x105E, &0u16.to_le_bytes())
}

fn pack_with_scripts(
    p: &Pack,
    begin: Option<(&[u8], &[u32])>,
    change: Option<(&[u8], &[u32])>,
) -> Vec<u8> {
    let mut subs = vec![
        sub(b"EDID", z(p.edid)),
        sub(b"PKDT", [&p.flags.to_le_bytes()[..], &[p.ty, 0], &[0; 6]].concat()),
        sub(b"PLDT", [p.location.0.to_le_bytes(), p.location.1.to_le_bytes(), 0i32.to_le_bytes()].concat()),
        sub(b"PSDT", [&[0xFF, p.dow as u8, 0, p.hour as u8][..], &p.duration.to_le_bytes()].concat()),
    ];
    if let Some(g) = p.global {
        let mut d = vec![0u8, 0, 0, 0];
        d.extend(1.0f32.to_le_bytes());
        d.extend(GET_GLOBAL_VALUE.to_le_bytes());
        d.extend([0, 0]);
        d.extend(g.to_le_bytes());
        d.extend([0u8; 12]);
        subs.push(sub(b"CTDA", d));
    }
    if let Some((scda, refs)) = begin {
        subs.push(sub(b"POBA", vec![]));
        subs.extend(scda_unit(0, scda, refs));
    }
    if let Some((scda, refs)) = change {
        subs.push(sub(b"POCA", vec![]));
        subs.extend(scda_unit(0, scda, refs));
    }
    record(b"PACK", p.id, &subs)
}

fn reentrancy_fixture(bytes: &[u8]) -> Fixture {
    let plugins = vec![Plugin::parse(bytes, "Reentrancy.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);
    Fixture { data, forms, refs }
}

fn run_reentrancy_plugin(
    packs: Vec<Vec<u8>>,
    npc_script: Option<(u32, Vec<u8>)>,
    npc_packages: &[u32],
    globals: &[(u32, &str, f32)],
) -> (Fixture, u32) {
    const NPC_ID: u32 = 0x940;
    const REF_ID: u32 = 0x950;
    const CELL_ID: u32 = 0x960;
    let mut scpt_records = Vec::new();
    let mut npc_subs = vec![sub(b"EDID", z("ReentrancyNpc"))];
    if let Some((sid, scda)) = npc_script {
        scpt_records.extend(record(b"SCPT", sid, &{
            let mut s = vec![sub(b"EDID", z("ReentrancyNpcScript"))];
            let g_refs: Vec<u32> = globals.iter().map(|(id, _, _)| *id).collect();
            s.extend(scda_unit(0, &scda, &g_refs));
            s
        }));
        npc_subs.push(sub(b"SCRI", sid.to_le_bytes().to_vec()));
    }
    npc_subs.extend(npc_packages.iter().map(|p| sub(b"PKID", p.to_le_bytes().to_vec())));
    let mut bytes = tes4();
    bytes.extend(top(b"GLOB", &globals.iter().flat_map(|(id, ed, v)| glob(*id, ed, *v)).collect::<Vec<_>>()));
    if !scpt_records.is_empty() {
        bytes.extend(top(b"SCPT", &scpt_records));
    }
    bytes.extend(top(b"PACK", &packs.concat()));
    bytes.extend(top(b"NPC_", &record(b"NPC_", NPC_ID, &npc_subs)));
    let cell = record(b"CELL", CELL_ID, &[sub(b"EDID", z("TestCell"))]);
    let achr = placed(b"ACHR", REF_ID, "ReentrancyRef", NPC_ID, None);
    let cell_body = [cell, group(CELL_ID.to_le_bytes(), 6, &group(CELL_ID.to_le_bytes(), 9, &achr))].concat();
    bytes.extend(top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell_body))));
    (reentrancy_fixture(&bytes), REF_ID)
}

/// Regression: when a package's Change result script calls EvaluatePackage (`evp`),
/// `set_package` must keep `current_package` at the newly chosen package so the inner
/// `set_package` does not re-enter the outgoing package's Change script recursively
/// until `MAX_DEPTH` (`nesting deeper than 256`).
#[test]
fn package_change_script_calling_evaluate_package_does_not_recurse() {
    const G_FIRST: u32 = 0x901;
    const G_CHANGE: u32 = 0x902;
    const G_BEGIN: u32 = 0x903;
    const P_FIRST: u32 = 0x910;
    const P_SECOND: u32 = 0x911;

    let change_scda = [bc_inc_global(1), bc_evp()].concat();
    let begin_scda = bc_inc_global(1);
    let p_first = Pack { id: P_FIRST, edid: "FirstPack", flags: 0, ty: 0, location: (0, 0), hour: -1, duration: 0, dow: -1, global: Some(G_FIRST) };
    let p_second = Pack { id: P_SECOND, edid: "SecondPack", flags: 0, ty: 0, location: (0, 0), hour: -1, duration: 0, dow: -1, global: None };

    let (f, ref_id) = run_reentrancy_plugin(
        vec![
            pack_with_scripts(&p_first, None, Some((&change_scda, &[G_CHANGE]))),
            pack_with_scripts(&p_second, Some((&begin_scda, &[G_BEGIN])), None),
        ],
        None,
        &[P_FIRST, P_SECOND],
        &[(G_FIRST, "GFirst", 1.0), (G_CHANGE, "GChange", 0.0), (G_BEGIN, "GBegin", 0.0)],
    );
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, package_interval: Some(10.0), ..Default::default() });
    let actor = FormId(ref_id);

    assert_eq!(g.evaluate_package(actor), Some(FormId(P_FIRST)));
    set(&mut g, G_FIRST, 0.0);
    assert_eq!(g.evaluate_package(actor), Some(FormId(P_SECOND)));

    let errors: Vec<_> = g.events.iter().filter(|e| matches!(e.kind, EventKind::ScriptError { .. })).collect();
    assert!(errors.is_empty(), "unexpected script errors: {errors:?}");
    assert_eq!(g.world.global(FormId(G_CHANGE)), Ok(1.0), "outgoing Change script runs once");
    assert_eq!(g.world.global(FormId(G_BEGIN)), Ok(1.0), "incoming Begin script runs once");
    assert_eq!(g.extra[&actor].current_package, Some(FormId(P_SECOND)));
}

/// Regression: an actor script's `OnPackageChange` block calling `EvaluatePackage`
/// must also not re-enter the outgoing package's change event.
#[test]
fn on_package_change_block_calling_evaluate_package_does_not_recurse() {
    const G_FIRST: u32 = 0x901;
    const G_CHANGE: u32 = 0x902;
    const P_FIRST: u32 = 0x910;
    const P_SECOND: u32 = 0x911;
    const NPC_SCRIPT: u32 = 0x920;

    let body = [bc_inc_global(2), bc_evp()].concat();
    let end = bc_st(0x11, &[]);
    let mut block_hdr = fnv_game::game::event::ON_PACKAGE_CHANGE.to_le_bytes().to_vec();
    block_hdr.extend(((body.len() + end.len()) as u32).to_le_bytes());
    let mut scda = bc_st(0x1D, &[]);
    scda.extend(bc_st(0x10, &block_hdr));
    scda.extend(body);
    scda.extend(end);

    let p_first = Pack { id: P_FIRST, edid: "FirstPack", flags: 0, ty: 0, location: (0, 0), hour: -1, duration: 0, dow: -1, global: Some(G_FIRST) };
    let p_second = Pack { id: P_SECOND, edid: "SecondPack", flags: 0, ty: 0, location: (0, 0), hour: -1, duration: 0, dow: -1, global: None };

    let (f, ref_id) = run_reentrancy_plugin(
        vec![pack(&p_first), pack(&p_second)],
        Some((NPC_SCRIPT, scda)),
        &[P_FIRST, P_SECOND],
        &[(G_FIRST, "GFirst", 1.0), (G_CHANGE, "GChange", 0.0)],
    );
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, package_interval: Some(10.0), ..Default::default() });
    let actor = FormId(ref_id);

    assert_eq!(g.evaluate_package(actor), Some(FormId(P_FIRST)));
    set(&mut g, G_FIRST, 0.0);
    assert_eq!(g.evaluate_package(actor), Some(FormId(P_SECOND)));

    let errors: Vec<_> = g.events.iter().filter(|e| matches!(e.kind, EventKind::ScriptError { .. })).collect();
    assert!(errors.is_empty(), "unexpected script errors: {errors:?}");
    assert_eq!(g.world.global(FormId(G_CHANGE)), Ok(1.0), "OnPackageChange runs once");
    assert_eq!(g.extra[&actor].current_package, Some(FormId(P_SECOND)));
}

/// When an outgoing package's Change script flips conditions and calls `EvaluatePackage`
/// to select a third package `P_THIRD`, the superseded `P_SECOND`'s Begin script must not
/// run afterwards and `current_package` stays at `P_THIRD`.
#[test]
fn package_change_script_switching_to_third_package_skips_superseded_begin() {
    const G_FIRST: u32 = 0x901;
    const G_THIRD: u32 = 0x902;
    const G_CHANGE: u32 = 0x903;
    const G_SECOND_BEGIN: u32 = 0x904;
    const G_THIRD_BEGIN: u32 = 0x905;
    const P_THIRD: u32 = 0x910;
    const P_FIRST: u32 = 0x911;
    const P_SECOND: u32 = 0x912;

    // P_FIRST Change script: G_THIRD = 1; G_CHANGE = G_CHANGE + 1; EvaluatePackage
    let change_scda = [bc_set_global(1, b" 1"), bc_inc_global(2), bc_evp()].concat();
    let second_begin = bc_inc_global(1);
    let third_begin = bc_inc_global(1);

    let p_third = Pack { id: P_THIRD, edid: "ThirdPack", flags: 0, ty: 0, location: (0, 0), hour: -1, duration: 0, dow: -1, global: Some(G_THIRD) };
    let p_first = Pack { id: P_FIRST, edid: "FirstPack", flags: 0, ty: 0, location: (0, 0), hour: -1, duration: 0, dow: -1, global: Some(G_FIRST) };
    let p_second = Pack { id: P_SECOND, edid: "SecondPack", flags: 0, ty: 0, location: (0, 0), hour: -1, duration: 0, dow: -1, global: None };

    let (f, ref_id) = run_reentrancy_plugin(
        vec![
            pack_with_scripts(&p_third, Some((&third_begin, &[G_THIRD_BEGIN])), None),
            pack_with_scripts(&p_first, None, Some((&change_scda, &[G_THIRD, G_CHANGE]))),
            pack_with_scripts(&p_second, Some((&second_begin, &[G_SECOND_BEGIN])), None),
        ],
        None,
        &[P_THIRD, P_FIRST, P_SECOND],
        &[
            (G_FIRST, "GFirst", 1.0),
            (G_THIRD, "GThird", 0.0),
            (G_CHANGE, "GChange", 0.0),
            (G_SECOND_BEGIN, "GSecondBegin", 0.0),
            (G_THIRD_BEGIN, "GThirdBegin", 0.0),
        ],
    );
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, package_interval: Some(10.0), ..Default::default() });
    let actor = FormId(ref_id);

    assert_eq!(g.evaluate_package(actor), Some(FormId(P_FIRST)));
    set(&mut g, G_FIRST, 0.0);
    // Outer evaluate_package chose P_SECOND, whose transition ran P_FIRST's Change script,
    // which flipped G_THIRD and evaluated into P_THIRD.
    assert_eq!(g.evaluate_package(actor), Some(FormId(P_THIRD)));

    let errors: Vec<_> = g.events.iter().filter(|e| matches!(e.kind, EventKind::ScriptError { .. })).collect();
    assert!(errors.is_empty(), "unexpected script errors: {errors:?}");
    assert_eq!(g.world.global(FormId(G_CHANGE)), Ok(1.0), "P_FIRST Change runs once");
    assert_eq!(g.world.global(FormId(G_THIRD_BEGIN)), Ok(1.0), "P_THIRD Begin runs once");
    assert_eq!(g.world.global(FormId(G_SECOND_BEGIN)), Ok(0.0), "superseded P_SECOND Begin does not run");
    assert_eq!(g.extra[&actor].current_package, Some(FormId(P_THIRD)));
}

/// Issue #3041: a package that is BOTH outside its schedule and failing its own
/// conditions must not be kept when no other package in the list applies (`check_package`
/// returns `OutOfSchedule` before testing conditions, so `select_package` tests the
/// package's own conditions directly). While its conditions still pass outside its
/// schedule it is still kept.
#[test]
fn an_out_of_schedule_package_whose_conditions_fail_is_not_kept() {
    let f = fixture();
    let mut g = game(&f, None);
    let actor = id(REF_I);
    set(&mut g, G_HOUR, 23.0);
    set(&mut g, G_SWITCH, 1.0);
    assert_eq!(g.evaluate_package(actor), Some(id(P_NIGHT_COND)));
    // Out of schedule (08:00), conditions still passing: kept.
    set(&mut g, G_HOUR, 8.0);
    assert_eq!(g.check_package(actor, id(P_NIGHT_COND)), PackageCheck::OutOfSchedule);
    assert_eq!(g.evaluate_package(actor), Some(id(P_NIGHT_COND)));
    // Out of schedule (08:00) AND conditions now failing: dropped, not kept.
    set(&mut g, G_SWITCH, 0.0);
    assert_eq!(g.check_package(actor, id(P_NIGHT_COND)), PackageCheck::OutOfSchedule);
    assert_eq!(g.evaluate_package(actor), None);
    assert_eq!(g.extra[&actor].current_package, None);
}

/// Issue #3041: a script relocating an actor (`MoveTo` / `MoveToMarkerWithFade`) releases
/// the Must Complete / Must Reach Location hold on its pre-relocation package so the next
/// periodic choice can drop it once its conditions fail, while keeping it unchanged if it
/// is still valid at the new spot.
#[test]
fn script_relocation_releases_must_complete_hold_without_dropping_a_still_valid_package() {
    let f = fixture();
    let mut g = game(&f, None);
    let e = id(REF_E);
    set(&mut g, G_SWITCH, 1.0);
    assert_eq!(g.update_package(e), Some(id(P_MUST)));
    assert_eq!(g.package_held(e), Some("must complete"));
    // Script moves the actor while the package is still valid: hold is cleared, but the
    // choice re-picks the same package unchanged.
    fnv_game::functions::console::call_words(&mut g, Some(e), "moveto", &[format!("{MARKER:08X}")], "test").unwrap();
    assert_eq!(g.package_held(e), None);
    assert_eq!(g.update_package(e), Some(id(P_MUST)));
    // Once its conditions also fail, the next periodic choice drops it to the fallback.
    set(&mut g, G_SWITCH, 0.0);
    assert_eq!(g.update_package(e), Some(id(P_DEFAULT)));
}
