//! Encounter zones on a synthetic plugin (no game data): a zone's level set on first
//! entry, leveled actors read at their reference's level, level modifiers, Never Resets.
//! docs/spec-notes/encounter-zones.md.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::functions::Ctx;
use fnv_game::game::{Game, GameConfig};
use fnv_game::zones::LevelNews;
use fnv_game::GameData;
use fnv_script::vm::{ArgValue, Value};
use fnv_script::FunctionTable;
use fnv_sim::zones::LevelAt;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const ZONE: u32 = 0x100;
const SAFE_ZONE: u32 = 0x101;
const RAT: u32 = 0x840;
const WOLF: u32 = 0x841;
const TIERS: u32 = 0x850;
const PELT: u32 = 0x880;
const TAIL: u32 = 0x881;
const ZONE_CELL: u32 = 0x200;
const PLAIN_CELL: u32 = 0x201;
const SAFE_CELL: u32 = 0x202;
/// A tiered list in the zone (minimum 5), one with the modifier Easy, one Boss.
const IN_ZONE: u32 = 0x320;
const EASY: u32 = 0x321;
const BOSS: u32 = 0x322;
/// The list outside every zone.
const OUTSIDE: u32 = 0x323;
/// The list in a zone of its own (reference XEZN) in the plain cell.
const OWN_ZONE: u32 = 0x324;
/// A leveled item list: level 1 a tail, level 5 a pelt, only the highest eligible level.
const LOOT: u32 = 0x860;

const GET_IN_ZONE: u16 = 0x11BE;
const SET_LEVEL: u16 = 0x1133;

fn acbs() -> fnv_esm::raw::Subrecord {
    let mut d = 0u32.to_le_bytes().to_vec();
    for v in [50u16, 0, 1, 0, 0, 100] {
        d.extend(v.to_le_bytes());
    }
    d.extend(0f32.to_le_bytes());
    d.extend(0i16.to_le_bytes());
    d.extend(0u16.to_le_bytes());
    sub(b"ACBS", d)
}

fn eczn(fid: u32, edid: &str, min: i8, flags: u8) -> Vec<u8> {
    record(b"ECZN", 0, fid, &[sub(b"EDID", z(edid)), sub(b"DATA", [0u32.to_le_bytes().to_vec(), vec![0, min as u8, flags, 0]].concat())])
}

fn placed(fid: u32, base: u32, extra: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    let mut subs = vec![sub(b"NAME", base.to_le_bytes().to_vec())];
    subs.extend(extra.iter().cloned());
    subs.push(sub(b"DATA", vec![0; 24]));
    record(b"ACRE", 0, fid, &subs)
}

fn cell(fid: u32, edid: &str, zone: Option<u32>, refs: &[u8]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"DATA", vec![1])];
    subs.extend(zone.map(|z| sub(b"XEZN", z.to_le_bytes().to_vec())));
    [record(b"CELL", 0, fid, &subs), group(fid.to_le_bytes(), 6, &group(fid.to_le_bytes(), 9, refs))].concat()
}

fn plugin_bytes() -> Vec<u8> {
    let item = |fid: u32, edid: &str| record(b"MISC", 0, fid, &[sub(b"EDID", z(edid)), sub(b"DATA", [1i32.to_le_bytes(), 1f32.to_le_bytes()].concat())]);
    let carries = |item: u32| sub(b"CNTO", [item.to_le_bytes(), 1i32.to_le_bytes()].concat());
    let creatures = [
        record(b"CREA", 0, RAT, &[sub(b"EDID", z("Rat")), acbs(), carries(TAIL)]),
        record(b"CREA", 0, WOLF, &[sub(b"EDID", z("Wolf")), acbs(), carries(PELT)]),
    ]
    .concat();
    let mut tiers = vec![sub(b"EDID", z("Tiers")), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0])];
    for (level, form) in [(1i16, RAT), (5, WOLF)] {
        tiers.push(sub(b"LVLO", [level.to_le_bytes().to_vec(), vec![0; 2], form.to_le_bytes().to_vec(), 1i16.to_le_bytes().to_vec(), vec![0; 2]].concat()));
    }
    let mut loot = vec![sub(b"EDID", z("Loot")), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0])];
    for (level, form) in [(1i16, TAIL), (5, PELT)] {
        loot.push(sub(b"LVLO", [level.to_le_bytes().to_vec(), vec![0; 2], form.to_le_bytes().to_vec(), 1i16.to_le_bytes().to_vec(), vec![0; 2]].concat()));
    }
    let xlcm = |m: i32| sub(b"XLCM", m.to_le_bytes().to_vec());
    let xezn = |z: u32| sub(b"XEZN", z.to_le_bytes().to_vec());
    let cells = [
        cell(ZONE_CELL, "ZoneCell", Some(ZONE), &[placed(IN_ZONE, TIERS, &[]), placed(EASY, TIERS, &[xlcm(0)]), placed(BOSS, TIERS, &[xlcm(3)])].concat()),
        cell(PLAIN_CELL, "PlainCell", None, &[placed(OUTSIDE, TIERS, &[]), placed(OWN_ZONE, TIERS, &[xezn(ZONE)])].concat()),
        cell(SAFE_CELL, "SafeCell", Some(SAFE_ZONE), &[]),
    ]
    .concat();
    [
        tes4(),
        top(b"MISC", &[item(PELT, "Pelt"), item(TAIL, "Tail")].concat()),
        top(b"CREA", &creatures),
        top(b"LVLC", &record(b"LVLC", 0, TIERS, &tiers)),
        top(b"LVLI", &record(b"LVLI", 0, LOOT, &loot)),
        // Minimum 5; the safe zone has "Never Resets" and "Match PC Below Minimum Level".
        top(b"ECZN", &[eczn(ZONE, "TestZone", 5, 0), eczn(SAFE_ZONE, "SafeZone", 8, 3)].concat()),
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
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Zones.esm".into()).unwrap()];
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

fn call(g: &mut Game, op: u16, on: u32, args: &[ArgValue]) -> f64 {
    let ctx = Ctx { unit: "test".into(), opcode: op, on: Some(FormId(on)), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    match g.call_function(&ctx, args) {
        Value::Num(x) => x,
        Value::Ref(f) => f as f64,
    }
}

#[test]
fn the_records_are_read() {
    let f = fixture();
    let z = f.forms.zones[&FormId(ZONE)];
    assert_eq!((z.min_level, z.never_resets, z.match_pc_below_min), (5, false, false));
    let s = f.forms.zones[&FormId(SAFE_ZONE)];
    assert_eq!((s.min_level, s.never_resets, s.match_pc_below_min), (8, true, true));
    assert_eq!(f.refs.cell_zone(FormId(ZONE_CELL)), Some(FormId(ZONE)));
    assert_eq!(f.refs.zone_of(FormId(IN_ZONE)), Some(FormId(ZONE)));
    assert_eq!(f.refs.zone_of(FormId(OUTSIDE)), None);
    // The reference's own zone over its cell's (none here).
    assert_eq!(f.refs.zone_of(FormId(OWN_ZONE)), Some(FormId(ZONE)));
    assert_eq!(f.refs.get(FormId(EASY)).unwrap().level_modifier, Some(0));
    assert_eq!(f.refs.get(FormId(IN_ZONE)).unwrap().level_modifier, None);
}

#[test]
fn a_zone_takes_its_minimum_level_on_entry_and_keeps_it() {
    let f = fixture();
    let mut g = game(&f);
    // A level-1 player: before entering, the level the zone would take.
    assert_eq!(g.leveled_at(FormId(IN_ZONE)), LevelAt::plain(5));
    g.player_entered(Some(FormId(ZONE_CELL)));
    assert_eq!(g.world.zone_level(FormId(ZONE)), Some(5));
    let news = g.take_level_news();
    assert!(news.contains(&LevelNews::Zone { zone: FormId(ZONE), level: 5, min_level: 5, player_level: 1 }), "{news:?}");
    assert!(news.iter().any(|n| matches!(n, LevelNews::Actor { actor, base: Some(b), at_display: Some(d), .. } if *actor == FormId(IN_ZONE) && *b == FormId(WOLF) && *d == FormId(RAT))), "{news:?}");
    // The tier for level 5: the wolf, its name and carried items.
    assert_eq!(g.display_base(FormId(TIERS), FormId(IN_ZONE)), FormId(WOLF));
    assert_eq!(g.world.item_count(FormId(IN_ZONE), FormId(PELT)).unwrap(), 1);
    // Easy: half the level (2), every entry at or under it: the rat.
    assert_eq!(g.leveled_at(FormId(EASY)), LevelAt { level: 2, all_levels: true });
    assert_eq!(g.display_base(FormId(TIERS), FormId(EASY)), FormId(RAT));
    // Boss: twice the level.
    assert_eq!(g.leveled_at(FormId(BOSS)).level, 10);
    // A later level-up changes neither the zone nor the references fixed in it.
    g.player.level = Some(20);
    g.player_entered(Some(FormId(PLAIN_CELL)));
    g.player_entered(Some(FormId(ZONE_CELL)));
    assert_eq!(g.world.zone_level(FormId(ZONE)), Some(5));
    assert_eq!(g.leveled_at(FormId(IN_ZONE)), LevelAt::plain(5));
    let zone = [ArgValue::Value(Value::Ref(ZONE))];
    assert_eq!((call(&mut g, GET_IN_ZONE, IN_ZONE, &zone), call(&mut g, GET_IN_ZONE, OUTSIDE, &zone)), (1.0, 0.0));
}

/// The tail or the pelt: which entry of the list the world rolls at now.
fn roll(g: &mut Game) -> Vec<(FormId, i32)> {
    g.world.add_item(FormId(OUTSIDE), FormId(LOOT), 1).unwrap()
}

#[test]
fn the_world_rolls_at_the_players_level_as_soon_as_it_changes() {
    let f = fixture();
    let mut g = game(&f);
    g.player_entered(Some(FormId(PLAIN_CELL)));
    assert_eq!(roll(&mut g), vec![(FormId(TAIL), 1)]);
    // SetLevel, with no tick before the roll: a console line or a script's next statement.
    call(&mut g, SET_LEVEL, FormId::PLAYER_REF.0, &[ArgValue::Value(Value::Num(20.0)), ArgValue::Value(Value::Num(0.0))]);
    assert_eq!(g.world.roll_level(), 20);
    assert_eq!(roll(&mut g), vec![(FormId(PELT), 1)]);
    // A level-up the same way.
    let mut g = game(&f);
    g.player_entered(Some(FormId(PLAIN_CELL)));
    g.player.level = Some(4);
    assert_eq!(fnv_game::levelup::advance(&mut g), 5);
    assert_eq!(roll(&mut g), vec![(FormId(PELT), 1)]);
}

#[test]
fn a_tick_brings_the_world_to_the_players_level() {
    let f = fixture();
    let mut g = game(&f);
    g.player_entered(Some(FormId(PLAIN_CELL)));
    // The level written with no call that tells the world.
    g.player.level = Some(20);
    assert_eq!(g.world.roll_level(), 1);
    g.tick();
    assert_eq!(g.world.roll_level(), 20);
    assert_eq!(roll(&mut g), vec![(FormId(PELT), 1)]);
    assert_eq!(g.snapshot().sim.config.player_level, 1);
}

/// The headless SaveLoad advances no frame: the roll right after a restore is at the
/// player's level.
#[test]
fn a_restored_game_rolls_at_the_players_level_before_any_tick() {
    let f = fixture();
    let mut g = game(&f);
    g.player_entered(Some(FormId(PLAIN_CELL)));
    // A saved game: the player at 20; the world saves its configured level, not this one.
    g.player.level = Some(20);
    let s = g.snapshot();
    assert_eq!(s.sim.config.player_level, 1);
    let world = World::restore(&f.forms, &f.refs, &s.sim).unwrap();
    assert_eq!(world.roll_level(), 1);
    let mut g2 = Game::restore(&f.data, world, GameConfig { strict: true, ..GameConfig::default() }, &s).unwrap();
    assert_eq!(g2.world.roll_level(), 20);
    assert_eq!(roll(&mut g2), vec![(FormId(PELT), 1)]);
    assert_eq!(g2.snapshot().sim.config.player_level, 1);
}

#[test]
fn a_game_with_no_level_set_keeps_the_configured_one() {
    let f = fixture();
    let world = World::new(&f.forms, &f.refs, SimConfig { player_level: 5, ..SimConfig::default() });
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() });
    g.tick();
    assert_eq!(g.world.roll_level(), 5);
    assert_eq!(g.world.config().player_level, 5);
}

#[test]
fn outside_a_zone_the_players_level_counts() {
    let f = fixture();
    let mut g = game(&f);
    g.player_entered(Some(FormId(PLAIN_CELL)));
    assert_eq!(g.leveled_at(FormId(OUTSIDE)), LevelAt::plain(1));
    assert_eq!(g.display_base(FormId(TIERS), FormId(OUTSIDE)), FormId(RAT));
    assert_eq!(g.world.item_count(FormId(OUTSIDE), FormId(TAIL)).unwrap(), 1);
    // A reference with a zone of its own enters it.
    assert_eq!(g.world.zone_level(FormId(ZONE)), Some(5));
    assert_eq!(g.display_base(FormId(TIERS), FormId(OWN_ZONE)), FormId(WOLF));
}

#[test]
fn match_pc_below_minimum_takes_the_players_level_and_never_resets_is_read() {
    let f = fixture();
    let mut g = game(&f);
    g.player.level = Some(3);
    g.player_entered(Some(FormId(SAFE_CELL)));
    assert_eq!(g.world.zone_level(FormId(SAFE_ZONE)), Some(3));
    assert!(g.world.cell_never_resets(FormId(SAFE_CELL)));
    assert!(!g.world.cell_never_resets(FormId(ZONE_CELL)));
    // Fallout 3's SetZoneRespawns turns it either way, and a snapshot keeps it.
    let set = |g: &mut Game, zone: u32, flag: f64| call(g, 0x11EA, 0x14, &[ArgValue::Value(Value::Ref(zone)), ArgValue::Value(Value::Num(flag))]);
    set(&mut g, SAFE_ZONE, 1.0);
    set(&mut g, ZONE, 0.0);
    assert!(!g.world.cell_never_resets(FormId(SAFE_CELL)));
    assert!(g.world.cell_never_resets(FormId(ZONE_CELL)));
    let s = g.snapshot();
    let world = World::restore(&f.forms, &f.refs, &s.sim).unwrap();
    assert!(world.cell_never_resets(FormId(ZONE_CELL)) && !world.cell_never_resets(FormId(SAFE_CELL)));
}

#[test]
fn zone_levels_survive_a_snapshot() {
    let f = fixture();
    let mut g = game(&f);
    g.player_entered(Some(FormId(ZONE_CELL)));
    let s = g.snapshot();
    let world = World::restore(&f.forms, &f.refs, &s.sim).unwrap();
    let g2 = Game::restore(&f.data, world, GameConfig { strict: true, ..GameConfig::default() }, &s).unwrap();
    assert_eq!(g2.world.zone_level(FormId(ZONE)), Some(5));
    assert_eq!(g2.world.fixed_level(FormId(EASY)), Some(LevelAt { level: 2, all_levels: true }));
}

#[test]
fn zone_respawns_getters_setters_and_cell_expiry_stay_consistent() {
    let f = fixture();
    let mut g = game(&f);
    let q = |g: &mut Game, body: &str| {
        g.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), fnv_game::game::RunCtx::default()).unwrap()
    };
    let num = |g: &mut Game, expr: &str| match q(g, &format!("SetFunctionValue ({expr})")) {
        fnv_script::vm::XVal::Num(n) => n,
        other => panic!("{expr}: expected number, got {other:?}"),
    };

    // Authored baseline: TestZone respawns (flags=0), SafeZone never resets + match PC below min (flags=3).
    assert_eq!(num(&mut g, "GetZoneRespawns TestZone"), 1.0);
    assert_eq!(num(&mut g, "GetZoneRespawns SafeZone"), 0.0);
    assert_eq!(num(&mut g, "GetZoneFlag TestZone 1"), 0.0);
    assert_eq!(num(&mut g, "GetZoneFlag SafeZone 1"), 1.0);
    assert_eq!(num(&mut g, "GetZoneFlag SafeZone 2"), 1.0);
    assert_eq!(num(&mut g, "GetZoneRespawns ZoneCell"), 0.0, "non-ECZN form returns 0");
    assert!(!g.cell_never_resets(FormId(ZONE_CELL)));
    assert!(g.cell_never_resets(FormId(SAFE_CELL)));

    // Saved SetZoneRespawns updates GetZoneRespawns, GetZoneFlag bit 1, and cell_never_resets
    // without clobbering unrelated flag bit 2.
    q(&mut g, "SetZoneRespawns SafeZone 1\nSetZoneRespawns TestZone 0");
    assert_eq!(num(&mut g, "GetZoneRespawns SafeZone"), 1.0);
    assert_eq!(num(&mut g, "GetZoneRespawns TestZone"), 0.0);
    assert_eq!(num(&mut g, "GetZoneFlag SafeZone 1"), 0.0);
    assert_eq!(num(&mut g, "GetZoneFlag SafeZone 2"), 1.0);
    assert_eq!(num(&mut g, "GetZoneFlag TestZone 1"), 1.0);
    assert!(!g.cell_never_resets(FormId(SAFE_CELL)));
    assert!(g.cell_never_resets(FormId(ZONE_CELL)));

    // Session SetZoneFlag bit 1 overrides SetZoneRespawns for the current session and reaches
    // both GetZoneRespawns and Game::cell_never_resets.
    q(&mut g, "SetZoneFlag TestZone 1 0");
    assert_eq!(num(&mut g, "GetZoneRespawns TestZone"), 1.0);
    assert_eq!(num(&mut g, "GetZoneFlag TestZone 1"), 0.0);
    assert!(!g.cell_never_resets(FormId(ZONE_CELL)));

    // Session SetCellEncounterZone reaches Game::cell_never_resets using live zone state.
    assert!(!g.cell_never_resets(FormId(PLAIN_CELL)));
    q(&mut g, "SetZoneFlag SafeZone 1 1\nSetCellEncounterZone PlainCell SafeZone");
    assert!(g.cell_never_resets(FormId(PLAIN_CELL)));

    // Save/restore keeps SetZoneRespawns while resetting session-only SetZoneFlag changes.
    let snap: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.checked_snapshot().unwrap()).unwrap()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let mut restored = Game::restore(&f.data, world, GameConfig { strict: true, ..GameConfig::default() }, &snap).unwrap();
    assert_eq!(num(&mut restored, "GetZoneRespawns SafeZone"), 1.0);
    assert_eq!(num(&mut restored, "GetZoneRespawns TestZone"), 0.0);
    assert_eq!(num(&mut restored, "GetZoneFlag SafeZone 1"), 0.0);
    assert_eq!(num(&mut restored, "GetZoneFlag TestZone 1"), 1.0);
    assert!(!restored.cell_never_resets(FormId(SAFE_CELL)));
    assert!(restored.cell_never_resets(FormId(ZONE_CELL)));

    // A later SetZoneRespawns supersedes an earlier session SetZoneFlag bit-1 override.
    q(&mut restored, "SetZoneFlag TestZone 1 0");
    assert_eq!(num(&mut restored, "GetZoneRespawns TestZone"), 1.0);
    q(&mut restored, "SetZoneRespawns TestZone 0");
    assert_eq!(num(&mut restored, "GetZoneRespawns TestZone"), 0.0);
    assert_eq!(num(&mut restored, "GetZoneFlag TestZone 1"), 1.0);
    assert!(restored.cell_never_resets(FormId(ZONE_CELL)));
}

/// Session `SetZoneMinLevel`, `SetZoneFlag` (bits 1 & 2), `SetZoneOwner`,
/// `SetCellEncounterZone`, and `SetRefEncounterZone` overrides must reach `enter_zone`,
/// `fix_cell_levels` / `LevelNews`, leveled actor selection (`display_base`), reset on
/// fresh-run restore, and survive same-run `WorldSession` + `Game::Session` load (#2272).
#[test]
fn session_zone_level_and_assignment_overrides_reach_spawns_and_survive_same_run_session() {
    let f = fixture();
    let mut g = game(&f);
    let q = |g: &mut Game, body: &str| {
        g.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), fnv_game::game::RunCtx::default()).unwrap()
    };
    let num = |g: &mut Game, expr: &str| match q(g, &format!("SetFunctionValue ({expr})")) {
        fnv_script::vm::XVal::Num(n) => n,
        fnv_script::vm::XVal::Ref(r) => f64::from(r),
        other => panic!("{expr}: expected number/ref, got {other:?}"),
    };

    // 1. SetZoneMinLevel lowers TestZone from authored 5 -> 2: level-1 player enters at 2
    //    and IN_ZONE picks Rat (lvl 1) instead of Wolf (lvl 5).
    g.player.level = Some(1);
    q(&mut g, "SetZoneMinLevel TestZone 2");
    assert_eq!(num(&mut g, "GetZoneMinLevel TestZone"), 2.0);
    g.player_entered(Some(FormId(ZONE_CELL)));
    assert_eq!(g.world.zone_level(FormId(ZONE)), Some(2));
    assert_eq!(g.display_base(FormId(TIERS), FormId(IN_ZONE)), FormId(RAT), "min_level 2 must spawn Rat, not Wolf");
    let news = g.take_level_news();
    assert!(
        news.contains(&LevelNews::Zone { zone: FormId(ZONE), level: 2, min_level: 2, player_level: 1 }),
        "LevelNews::Zone must report overridden min_level and level: {news:?}"
    );

    // 2. SetZoneFlag bit 2 turns OFF Match PC Below Minimum Level on SafeZone (authored min 8, flags 3):
    //    level-3 player enters SafeZone at 8 instead of 3.
    g.player.level = Some(3);
    q(&mut g, "SetZoneFlag SafeZone 2 0");
    assert_eq!(num(&mut g, "GetZoneMatchLevel SafeZone"), 0.0);
    assert_eq!(num(&mut g, "GetZoneFlag SafeZone 2"), 0.0);
    g.player_entered(Some(FormId(SAFE_CELL)));
    assert_eq!(g.world.zone_level(FormId(SAFE_ZONE)), Some(8), "Match PC Below Min disabled -> level 8");

    // 3. SetCellEncounterZone PlainCell SafeZone + clearing OWN_ZONE's ref XEZN makes BOTH
    //    OUTSIDE and OWN_ZONE pick Wolf (SafeZone lvl 8), report zone=Some(SAFE_ZONE), and
    //    satisfy GetInZone SafeZone.
    assert_eq!(call(&mut g, GET_IN_ZONE, OUTSIDE, &[ArgValue::Value(Value::Ref(SAFE_ZONE))]), 0.0);
    q(&mut g, "SetCellEncounterZone PlainCell SafeZone");
    assert_eq!(call(&mut g, GET_IN_ZONE, OUTSIDE, &[ArgValue::Value(Value::Ref(SAFE_ZONE))]), 1.0);
    g.world.set_ref_zone(FormId(OWN_ZONE), None);
    let _ = g.take_level_news();
    g.player_entered(Some(FormId(PLAIN_CELL)));
    assert_eq!(g.display_base(FormId(TIERS), FormId(OUTSIDE)), FormId(WOLF), "PlainCell assigned to SafeZone (lvl 8) must spawn Wolf");
    assert_eq!(g.display_base(FormId(TIERS), FormId(OWN_ZONE)), FormId(WOLF), "cleared ref XEZN inherits PlainCell's SafeZone (lvl 8)");
    let news = g.take_level_news();
    assert!(
        news.iter().any(|n| matches!(n, LevelNews::Actor { actor, zone, .. } if *actor == FormId(OUTSIDE) && *zone == Some(FormId(SAFE_ZONE)))),
        "LevelNews::Actor must report overridden cell zone: {news:?}"
    );

    // 4. Session save/load boundary for ALL zone setters (SetZoneMinLevel, SetZoneFlag bits 1 & 2,
    //    SetZoneOwner, SetCellEncounterZone).
    let mut g2 = game(&f);
    g2.player.level = Some(3);
    q(
        &mut g2,
        "SetZoneMinLevel TestZone 9\nSetZoneFlag TestZone 1 1\nSetZoneFlag SafeZone 2 0\nSetZoneOwner TestZone Wolf\nSetCellEncounterZone PlainCell SafeZone",
    );
    assert_eq!(num(&mut g2, "GetZoneOwner TestZone"), f64::from(WOLF));
    // SetZoneOwner with a null owner (0) leaves the current owner unchanged [observed:
    // 2026-11-23, GSDocMitchellHouse (0x00103DF9), probe anh_11_zone_owner_readback].
    q(&mut g2, "SetZoneOwner TestZone 0");
    assert_eq!(num(&mut g2, "GetZoneOwner TestZone"), f64::from(WOLF));
    assert_eq!(call(&mut g2, GET_IN_ZONE, OUTSIDE, &[ArgValue::Value(Value::Ref(SAFE_ZONE))]), 1.0);
    let snap: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&g2.checked_snapshot().unwrap()).unwrap()).unwrap();

    // 4a. Fresh-run restore (without Session transfer) reverts all session overrides to authored values.
    let fresh_world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let mut fresh = Game::restore(&f.data, fresh_world, GameConfig { strict: true, ..GameConfig::default() }, &snap).unwrap();
    assert_eq!(num(&mut fresh, "GetZoneMinLevel TestZone"), 5.0);
    assert_eq!(num(&mut fresh, "GetZoneFlag TestZone 1"), 0.0);
    assert_eq!(num(&mut fresh, "GetZoneMatchLevel SafeZone"), 1.0);
    assert_eq!(num(&mut fresh, "GetZoneOwner TestZone"), 0.0);
    assert!(!fresh.cell_never_resets(FormId(ZONE_CELL)));
    assert!(!fresh.cell_never_resets(FormId(PLAIN_CELL)));
    assert_eq!(call(&mut fresh, GET_IN_ZONE, OUTSIDE, &[ArgValue::Value(Value::Ref(SAFE_ZONE))]), 0.0);

    // 4b. Same-run restore with WorldSession + Game::Session preserves ALL session zone overrides.
    let ws = g2.world.take_session();
    assert!(!ws.is_empty());
    let gs = g2.take_session();
    let mut same_world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    same_world.put_session(ws);
    let mut same_run = Game::restore(&f.data, same_world, GameConfig { strict: true, ..GameConfig::default() }, &snap).unwrap();
    same_run.put_session(gs);
    assert_eq!(num(&mut same_run, "GetZoneMinLevel TestZone"), 9.0);
    assert_eq!(num(&mut same_run, "GetZoneFlag TestZone 1"), 1.0);
    assert_eq!(num(&mut same_run, "GetZoneMatchLevel SafeZone"), 0.0);
    assert_eq!(num(&mut same_run, "GetZoneOwner TestZone"), f64::from(WOLF));
    assert!(same_run.cell_never_resets(FormId(ZONE_CELL)));
    assert!(same_run.cell_never_resets(FormId(PLAIN_CELL)));
    assert_eq!(call(&mut same_run, GET_IN_ZONE, OUTSIDE, &[ArgValue::Value(Value::Ref(SAFE_ZONE))]), 1.0);
    same_run.player_entered(Some(FormId(ZONE_CELL)));
    assert_eq!(same_run.world.zone_level(FormId(ZONE)), Some(9), "carried SetZoneMinLevel 9 must govern entry after load");
}

/// `bNoLockEncounterZoneLevels` (#3878): off by default (vanilla locking unchanged);
/// when enabled, entering or querying a zone recomputes its level against the current
/// player level while respecting `min_level`, `Match PC Below Minimum Level`, and
/// session `SetZoneMinLevel` / `SetZoneFlag` overrides.
#[test]
fn no_lock_encounter_zone_levels_defaults_off_and_scales_on_reentry_and_query() {
    let f = fixture();
    let q = |g: &mut Game, body: &str| {
        g.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), fnv_game::game::RunCtx::default()).unwrap()
    };
    let num = |g: &mut Game, expr: &str| match q(g, &format!("SetFunctionValue ({expr})")) {
        fnv_script::vm::XVal::Num(n) => n,
        other => panic!("{expr}: expected number, got {other:?}"),
    };

    // 1. Default OFF: vanilla locking keeps TestZone at 5 for both GetZoneLevel and later cells.
    let mut off = game(&f);
    assert!(!off.tweaks.no_lock_encounter_zone_levels);
    off.player.level = Some(1);
    off.player_entered(Some(FormId(ZONE_CELL)));
    assert_eq!(num(&mut off, "GetZoneLevel TestZone"), 5.0);
    off.player.level = Some(20);
    assert_eq!(off.leveled_at(FormId(OWN_ZONE)), LevelAt::plain(5));
    off.player_entered(Some(FormId(PLAIN_CELL)));
    assert_eq!(off.world.zone_level(FormId(ZONE)), Some(5));
    assert_eq!(num(&mut off, "GetZoneLevel TestZone"), 5.0);
    assert_eq!(off.leveled_at(FormId(OWN_ZONE)), LevelAt::plain(5));

    // 2. Enabled ON: zone recomputes as player levels up, respecting min_level and Match PC Below Min.
    let mut g = game(&f);
    g.tweaks.no_lock_encounter_zone_levels = true;
    g.player.level = Some(1);
    // Unvisited zone still reports 0 via GetZoneLevel.
    assert_eq!(num(&mut g, "GetZoneLevel TestZone"), 0.0);

    // First entry at level 1 respects TestZone min_level 5.
    g.player_entered(Some(FormId(ZONE_CELL)));
    assert_eq!(g.world.zone_level(FormId(ZONE)), Some(5));
    assert_eq!(num(&mut g, "GetZoneLevel TestZone"), 5.0);
    let news = g.take_level_news();
    assert!(news.contains(&LevelNews::Zone { zone: FormId(ZONE), level: 5, min_level: 5, player_level: 1 }), "{news:?}");

    // Leveling up to 20 updates live queries (GetZoneLevel and unfixed OWN_ZONE) immediately,
    // while already-fixed IN_ZONE keeps its fixed level until cell reset.
    g.player.level = Some(20);
    assert_eq!(g.zone_level(FormId(ZONE)), Some(20));
    assert_eq!(num(&mut g, "GetZoneLevel TestZone"), 20.0);
    assert_eq!(g.leveled_at(FormId(OWN_ZONE)), LevelAt::plain(20));
    assert_eq!(g.leveled_at(FormId(IN_ZONE)), LevelAt::plain(5));

    // Entering PLAIN_CELL (containing OWN_ZONE with XEZN=ZONE) updates ZONE to 20 and fixes OWN_ZONE at 20.
    g.player_entered(Some(FormId(PLAIN_CELL)));
    assert_eq!(g.world.zone_level(FormId(ZONE)), Some(20));
    assert_eq!(g.world.fixed_level(FormId(OWN_ZONE)), Some(LevelAt::plain(20)));
    let news = g.take_level_news();
    assert!(news.contains(&LevelNews::Zone { zone: FormId(ZONE), level: 20, min_level: 5, player_level: 20 }), "{news:?}");

    // Re-entering ZONE_CELL at the same level 20 emits no duplicate LevelNews::Zone.
    g.player_entered(Some(FormId(ZONE_CELL)));
    assert!(g.take_level_news().is_empty());

    // Session SetZoneMinLevel 25 raises the zone above player level 20 on query and re-entry.
    q(&mut g, "SetZoneMinLevel TestZone 25");
    assert_eq!(num(&mut g, "GetZoneLevel TestZone"), 25.0);
    g.player_entered(Some(FormId(PLAIN_CELL)));
    assert_eq!(g.world.zone_level(FormId(ZONE)), Some(25));

    // Match PC Below Minimum Level on SafeZone (min 8, flags 3): scales at 3 -> 6 -> 12,
    // then respects SetZoneFlag SafeZone 2 0 (back to min 8 when player is 3).
    g.player.level = Some(3);
    g.player_entered(Some(FormId(SAFE_CELL)));
    assert_eq!(g.world.zone_level(FormId(SAFE_ZONE)), Some(3));
    assert_eq!(num(&mut g, "GetZoneLevel SafeZone"), 3.0);
    g.player.level = Some(6);
    g.player_entered(Some(FormId(PLAIN_CELL)));
    g.player_entered(Some(FormId(SAFE_CELL)));
    assert_eq!(g.world.zone_level(FormId(SAFE_ZONE)), Some(6));
    g.player.level = Some(12);
    g.player_entered(Some(FormId(PLAIN_CELL)));
    g.player_entered(Some(FormId(SAFE_CELL)));
    assert_eq!(g.world.zone_level(FormId(SAFE_ZONE)), Some(12));
    g.player.level = Some(3);
    q(&mut g, "SetZoneFlag SafeZone 2 0");
    assert_eq!(num(&mut g, "GetZoneLevel SafeZone"), 8.0);
    g.player_entered(Some(FormId(PLAIN_CELL)));
    g.player_entered(Some(FormId(SAFE_CELL)));
    assert_eq!(g.world.zone_level(FormId(SAFE_ZONE)), Some(8));
}

/// Toggling `no_lock_encounter_zone_levels` on/off and round-tripping through snapshots
/// keeps saved zone state clean and restores vanilla default (off) on load.
#[test]
fn no_lock_encounter_zone_levels_toggle_and_snapshot_stay_clean() {
    let f = fixture();
    let mut g = game(&f);
    let q = |g: &mut Game, body: &str| {
        g.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), fnv_game::game::RunCtx::default()).unwrap()
    };
    let num = |g: &mut Game, expr: &str| match q(g, &format!("SetFunctionValue ({expr})")) {
        fnv_script::vm::XVal::Num(n) => n,
        other => panic!("{expr}: expected number, got {other:?}"),
    };

    // Visit TestZone at level 1 while tweak is OFF (locked at 5), plus simulate an
    // offline-imported zone level on SafeZone (14).
    g.player.level = Some(1);
    g.player_entered(Some(FormId(ZONE_CELL)));
    g.world.set_zone_level(FormId(SAFE_ZONE), 14);
    g.player.level = Some(20);
    assert_eq!(num(&mut g, "GetZoneLevel TestZone"), 5.0);
    assert_eq!(num(&mut g, "GetZoneLevel SafeZone"), 14.0);
    assert_eq!(g.leveled_at(FormId(OWN_ZONE)), LevelAt::plain(5));

    // Toggle ON without re-entering: queries recompute to 20, underlying stored levels untouched.
    g.tweaks.no_lock_encounter_zone_levels = true;
    g.tick();
    assert!(g.world.no_lock_encounter_zone_levels());
    assert_eq!(num(&mut g, "GetZoneLevel TestZone"), 20.0);
    assert_eq!(num(&mut g, "GetZoneLevel SafeZone"), 20.0);
    assert_eq!(g.leveled_at(FormId(OWN_ZONE)), LevelAt::plain(20));

    // Toggle back OFF without re-entering: prior locked levels return unchanged.
    g.tweaks.no_lock_encounter_zone_levels = false;
    g.tick();
    assert!(!g.world.no_lock_encounter_zone_levels());
    assert_eq!(num(&mut g, "GetZoneLevel TestZone"), 5.0);
    assert_eq!(num(&mut g, "GetZoneLevel SafeZone"), 14.0);
    assert_eq!(g.leveled_at(FormId(OWN_ZONE)), LevelAt::plain(5));

    // Re-enable, enter PLAIN_CELL at level 20 (updating TestZone to 20), then snapshot/restore.
    g.tweaks.no_lock_encounter_zone_levels = true;
    g.player_entered(Some(FormId(PLAIN_CELL)));
    assert_eq!(g.world.zone_level(FormId(ZONE)), Some(20));
    let snap = g.snapshot();
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let mut restored = Game::restore(&f.data, world, GameConfig { strict: true, ..GameConfig::default() }, &snap).unwrap();
    assert!(!restored.tweaks.no_lock_encounter_zone_levels, "tweak is a setting, not saved");
    assert!(!restored.world.no_lock_encounter_zone_levels());
    assert_eq!(restored.world.zone_level(FormId(ZONE)), Some(20));
    restored.player.level = Some(30);
    assert_eq!(num(&mut restored, "GetZoneLevel TestZone"), 20.0, "restored game defaults to vanilla locking");
}
