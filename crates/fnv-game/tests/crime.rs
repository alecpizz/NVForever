//! Karma, ownership and crime on a synthetic plugin (no game data):
//! docs/spec-notes/crime-karma.md §2–§5.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::crime::{Alignment, Kind, Note};
use fnv_game::detection::{Band, Detection};
use fnv_game::functions::Ctx;
use fnv_game::game::{Game, GameConfig};
use fnv_game::GameData;
use fnv_script::vm::{ArgValue, Value};
use fnv_script::FunctionTable;
use fnv_sim::world::Outcome;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const TOWN: u32 = 0x900;
const GANG: u32 = 0x901;
/// A second faction counting towards the town's reputation.
const MARKET: u32 = 0x902;
const REP_TOWN: u32 = 0x920;
const REP_GANG: u32 = 0x921;
const SHOPKEEPER: u32 = 0x830;
const TOWNSMAN: u32 = 0x831;
const FIEND: u32 = 0x832;
const STRANGER: u32 = 0x833;
/// An NPC made from the Fiend's template (traits from the template), with 0 karma of its
/// own.
const CONVICT: u32 = 0x834;
const CONVICT_REF: u32 = 0x324;
/// A Very Evil NPC (−900) and a Very Good one (900), unseen, in no faction.
const BOSS: u32 = 0x835;
const BOSS_REF: u32 = 0x325;
const SAINT: u32 = 0x836;
const SAINT_REF: u32 = 0x326;
/// In the town, the market and the gang: two factions share a reputation.
const ELDER: u32 = 0x837;
/// In the gang by its own record, but its factions come from the shopkeeper (a template
/// with Use Factions).
const HEIR: u32 = 0x838;
/// In the town's list at rank -1: not a member.
const EXILE: u32 = 0x839;
const TRINKET: u32 = 0x850;
const SHOP: u32 = 0x200;
const BARRACKS: u32 = 0x201;
const SHOPKEEPER_REF: u32 = 0x320;
const TOWNSMAN_REF: u32 = 0x321;
const FIEND_REF: u32 = 0x322;
const STRANGER_REF: u32 = 0x323;
const TRINKET_REF: u32 = 0x330;
const GANG_TRINKET_REF: u32 = 0x331;
const HEIR_TRINKET_REF: u32 = 0x332;
const BARRACKS_TRINKET_REF: u32 = 0x333;
const SHOP_DOOR: u32 = 0x860;
/// Two teleport doors into the shop: the first flagged No Alarm (XTEL), the second not.
const QUIET_DOOR_REF: u32 = 0x340;
const LOUD_DOOR_REF: u32 = 0x341;
/// In the town only, unaggressive, out of sight of every crime (#2467): it attacks the
/// player only by the town's enemy flag.
const WATCHMAN: u32 = 0x83A;
const WATCHMAN_REF: u32 = 0x327;
/// A second placed townsman (the townsman's base).
const TOWNSMAN2_REF: u32 = 0x328;
/// The clock's globals (GameDaysPassed starts at 5).
const DAYS_PASSED_START: f64 = 5.0;
/// #2456: two sides, enemies by the republic's record, and a tag faction (Hidden, Track
/// Crime, naming only itself) that holds people of both, as FalloutNV.esm's
/// ArmorVCaesarsLegionFaction holds Legionaries and four NCR troopers.
const LEGION: u32 = 0x903;
const REPUBLIC: u32 = 0x904;
const TAG: u32 = 0x905;
const LEGIONARY: u32 = 0x83D;
const LEGIONARY_REF: u32 = 0x32A;
const DECANUS: u32 = 0x83E;
const DECANUS_REF: u32 = 0x32B;
const TROOPER: u32 = 0x83F;
const TROOPER_REF: u32 = 0x32C;

const GET_ACTOR_CRIME_PLAYER_ENEMY: u16 = 0x11CB;
const GET_MINOR_CRIME_COUNT: u16 = 0x1074;
const IS_PC_A_MURDERER: u16 = 0x10B0;
const IS_TRESPASSING: u16 = 0x1091;
const GET_TRESPASS_WARNING_LEVEL: u16 = 0x1090;
const IS_CELL_OWNER: u16 = 0x1118;
const REWARD_KARMA: u16 = 0x1204;
const GET_AV: u16 = 0x100E;
const KARMA: u16 = 23;

/// ACBS with a karma value.
fn acbs(karma: f32) -> fnv_esm::raw::Subrecord {
    acbs_templated(karma, 0)
}

/// ACBS with a karma value and template flags (0x01 Use Traits [documented: xEdit]).
fn acbs_templated(karma: f32, template_flags: u16) -> fnv_esm::raw::Subrecord {
    let mut d = 0u32.to_le_bytes().to_vec();
    for v in [50u16, 0, 1, 0, 0, 100] {
        d.extend(v.to_le_bytes());
    }
    d.extend(karma.to_le_bytes());
    d.extend(0i16.to_le_bytes());
    d.extend(template_flags.to_le_bytes());
    sub(b"ACBS", d)
}

fn snam(faction: u32) -> fnv_esm::raw::Subrecord {
    sub(b"SNAM", [faction.to_le_bytes().to_vec(), vec![0, 0, 0, 0]].concat())
}

fn snam_at_rank(faction: u32, rank: i8) -> fnv_esm::raw::Subrecord {
    sub(b"SNAM", [faction.to_le_bytes().to_vec(), vec![rank as u8, 0, 0, 0]].concat())
}

/// AIDT: aggression, and Assistance 0.
fn aidt(aggression: u8) -> fnv_esm::raw::Subrecord {
    let mut d = vec![aggression, 0, 50, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    d.extend(0i32.to_le_bytes());
    sub(b"AIDT", d)
}

fn placed(sig: &[u8; 4], fid: u32, base: u32, extra: Vec<fnv_esm::raw::Subrecord>) -> Vec<u8> {
    let mut subs = vec![sub(b"NAME", base.to_le_bytes().to_vec())];
    subs.extend(extra);
    subs.push(sub(b"DATA", vec![0; 24]));
    record(sig, 0, fid, &subs)
}

/// A quest whose Detection-type topics a witness can say: the NoCrime topics (#974).
const QUEST: u32 = 0x910;
/// A script naming Town and Market to SetPCEnemyOfFaction: their flags may be a quest's
/// (`Game::script_flaggable_factions`); Gang's and the others' only a crime's (#3335).
const HOSTILITY_SCRIPT: u32 = 0x911;
const NO_CRIME_TOPICS: [(u32, &str); 2] = [(0xA00, "StealNoCrime"), (0xA01, "AssaultNoCrime")];

/// A topic of type Combat (the GECK's Combat tab) with one line, no conditions.
fn topic(id: u32, edid: &str) -> Vec<u8> {
    let dial = record(b"DIAL", 0, id, &[sub(b"EDID", z(edid)), sub(b"FULL", z(edid)), sub(b"PNAM", 50f32.to_le_bytes().to_vec()), sub(b"DATA", vec![fnv_game::data::topic_type::COMBAT, 0])]);
    let trdt = [0u32.to_le_bytes().to_vec(), 50u32.to_le_bytes().to_vec(), vec![0; 4], vec![1, 0, 0, 0], 0u32.to_le_bytes().to_vec(), vec![0; 4]].concat();
    let info = record(
        b"INFO",
        0,
        id + 0x100,
        &[
            sub(b"DATA", vec![fnv_game::data::topic_type::COMBAT, 0, 0, 0]),
            sub(b"QSTI", QUEST.to_le_bytes().to_vec()),
            sub(b"TRDT", trdt),
            sub(b"NAM1", z(edid)),
            sub(b"NAM2", z("")),
            sub(b"NAM3", z("")),
            sub(b"NEXT", vec![]),
        ],
    );
    [dial, group(id.to_le_bytes(), 7, &info)].concat()
}

fn plugin_bytes() -> Vec<u8> {
    let quest = record(b"QUST", 0, QUEST, &[sub(b"EDID", z("CrimeTalk")), sub(b"DATA", [vec![1, 50, 0, 0], 0f32.to_le_bytes().to_vec()].concat())]);
    let dial: Vec<u8> = NO_CRIME_TOPICS.iter().flat_map(|&(id, edid)| topic(id, edid)).collect();
    let reputations = [
        record(b"REPU", 0, REP_TOWN, &[sub(b"EDID", z("RepTown")), sub(b"DATA", 20f32.to_le_bytes().to_vec())]),
        record(b"REPU", 0, REP_GANG, &[sub(b"EDID", z("RepGang")), sub(b"DATA", 20f32.to_le_bytes().to_vec())]),
    ]
    .concat();
    let wmi1 = |r: u32| sub(b"WMI1", r.to_le_bytes().to_vec());
    let factions = [
        // Track Crime (second byte, bit 0).
        record(b"FACT", 0, TOWN, &[sub(b"EDID", z("Town")), sub(b"DATA", vec![0, 1, 0, 0]), wmi1(REP_TOWN)]),
        // Evil (first byte, bit 1) and Track Crime.
        record(b"FACT", 0, GANG, &[sub(b"EDID", z("Gang")), sub(b"DATA", vec![2, 1, 0, 0]), wmi1(REP_GANG)]),
        record(b"FACT", 0, MARKET, &[sub(b"EDID", z("Market")), sub(b"DATA", vec![0, 1, 0, 0]), wmi1(REP_TOWN)]),
        record(b"FACT", 0, LEGION, &[sub(b"EDID", z("Legion")), sub(b"DATA", vec![0, 1, 0, 0])]),
        // XNAM: faction, modifier, reaction (1 Enemy, 3 Friend).
        record(b"FACT", 0, REPUBLIC, &[sub(b"EDID", z("Republic")), sub(b"DATA", vec![0, 1, 0, 0]), sub(b"XNAM", [LEGION.to_le_bytes(), 0i32.to_le_bytes(), 1u32.to_le_bytes()].concat())]),
        record(b"FACT", 0, TAG, &[sub(b"EDID", z("LegionTag")), sub(b"DATA", vec![1, 1, 0, 0]), sub(b"XNAM", [TAG.to_le_bytes(), 0i32.to_le_bytes(), 3u32.to_le_bytes()].concat())]),
    ]
    .concat();
    let npcs = [
        record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player")), acbs(0.0)]),
        record(b"NPC_", 0, SHOPKEEPER, &[sub(b"EDID", z("Shopkeeper")), acbs(0.0), snam(TOWN), aidt(0)]),
        record(b"NPC_", 0, TOWNSMAN, &[sub(b"EDID", z("Townsman")), acbs(500.0), snam(TOWN), aidt(0)]),
        record(b"NPC_", 0, FIEND, &[sub(b"EDID", z("Fiend")), acbs(-500.0), snam(GANG), aidt(0)]),
        record(b"NPC_", 0, STRANGER, &[sub(b"EDID", z("Stranger")), acbs(0.0), aidt(0)]),
        record(b"NPC_", 0, CONVICT, &[sub(b"EDID", z("Convict")), acbs_templated(0.0, 0x01), sub(b"TPLT", FIEND.to_le_bytes().to_vec()), aidt(0)]),
        record(b"NPC_", 0, BOSS, &[sub(b"EDID", z("Boss")), acbs(-900.0), aidt(0)]),
        record(b"NPC_", 0, SAINT, &[sub(b"EDID", z("Saint")), acbs(900.0), aidt(0)]),
        record(b"NPC_", 0, ELDER, &[sub(b"EDID", z("Elder")), acbs(0.0), snam(TOWN), snam(MARKET), snam(GANG), aidt(0)]),
        // Template flag 0x04: Use Factions [schema `gameplay.json`].
        record(b"NPC_", 0, HEIR, &[sub(b"EDID", z("Heir")), acbs_templated(0.0, 0x04), snam(GANG), sub(b"TPLT", SHOPKEEPER.to_le_bytes().to_vec()), aidt(0)]),
        record(b"NPC_", 0, EXILE, &[sub(b"EDID", z("Exile")), acbs(0.0), snam_at_rank(TOWN, -1), aidt(0)]),
        record(b"NPC_", 0, WATCHMAN, &[sub(b"EDID", z("Watchman")), acbs(0.0), snam(TOWN), aidt(0)]),
        record(b"NPC_", 0, LEGIONARY, &[sub(b"EDID", z("Legionary")), acbs(0.0), snam(LEGION), snam(TAG), aidt(0)]),
        record(b"NPC_", 0, DECANUS, &[sub(b"EDID", z("Decanus")), acbs(0.0), snam(LEGION), snam(TAG), aidt(0)]),
        // Aggressive (1): attacks enemies on sight.
        record(b"NPC_", 0, TROOPER, &[sub(b"EDID", z("Trooper")), acbs(0.0), snam(REPUBLIC), snam(TAG), aidt(1)]),
    ]
    .concat();
    let misc = record(b"MISC", 0, TRINKET, &[sub(b"EDID", z("Trinket")), sub(b"DATA", [5i32.to_le_bytes(), 1f32.to_le_bytes()].concat())]);
    let door = record(b"DOOR", 0, SHOP_DOOR, &[sub(b"EDID", z("ShopDoor"))]);
    // XTEL: destination door, position and rotation, No Alarm (u32) [xEdit REFR XTEL].
    let xtel = |to: u32, no_alarm: u32| sub(b"XTEL", [to.to_le_bytes().to_vec(), vec![0; 24], no_alarm.to_le_bytes().to_vec()].concat());
    // An interior cell, not public, owned by the shopkeeper.
    let cell = record(b"CELL", 0, SHOP, &[sub(b"EDID", z("Shop")), sub(b"DATA", vec![0x01]), sub(b"XOWN", SHOPKEEPER.to_le_bytes().to_vec())]);
    let refs = [
        placed(b"ACHR", SHOPKEEPER_REF, SHOPKEEPER, vec![sub(b"EDID", z("ShopkeeperRef"))]),
        placed(b"ACHR", TOWNSMAN_REF, TOWNSMAN, vec![sub(b"EDID", z("TownsmanRef"))]),
        placed(b"ACHR", FIEND_REF, FIEND, vec![sub(b"EDID", z("FiendRef"))]),
        placed(b"ACHR", STRANGER_REF, STRANGER, vec![sub(b"EDID", z("StrangerRef"))]),
        placed(b"ACHR", CONVICT_REF, CONVICT, vec![]),
        placed(b"ACHR", BOSS_REF, BOSS, vec![]),
        placed(b"ACHR", SAINT_REF, SAINT, vec![]),
        placed(b"ACHR", WATCHMAN_REF, WATCHMAN, vec![]),
        placed(b"ACHR", TOWNSMAN2_REF, TOWNSMAN, vec![]),
        placed(b"ACHR", LEGIONARY_REF, LEGIONARY, vec![]),
        placed(b"ACHR", DECANUS_REF, DECANUS, vec![]),
        placed(b"ACHR", TROOPER_REF, TROOPER, vec![]),
        placed(b"REFR", TRINKET_REF, TRINKET, vec![sub(b"EDID", z("TrinketRef"))]),
        placed(b"REFR", GANG_TRINKET_REF, TRINKET, vec![sub(b"EDID", z("GangTrinketRef")), sub(b"XOWN", GANG.to_le_bytes().to_vec()), sub(b"XRNK", 1i32.to_le_bytes().to_vec())]),
        placed(b"REFR", HEIR_TRINKET_REF, TRINKET, vec![sub(b"XOWN", HEIR.to_le_bytes().to_vec())]),
        placed(b"REFR", QUIET_DOOR_REF, SHOP_DOOR, vec![xtel(LOUD_DOOR_REF, 1)]),
        placed(b"REFR", LOUD_DOOR_REF, SHOP_DOOR, vec![xtel(QUIET_DOOR_REF, 0)]),
    ]
    .concat();
    let children = group(SHOP.to_le_bytes(), 6, &group(SHOP.to_le_bytes(), 9, &refs));
    // A second interior cell owned by Gang at required rank 2, with an unowned trinket inheriting it.
    let barracks = record(
        b"CELL",
        0,
        BARRACKS,
        &[sub(b"EDID", z("GangBarracks")), sub(b"DATA", vec![0x01]), sub(b"XOWN", GANG.to_le_bytes().to_vec()), sub(b"XRNK", 2i32.to_le_bytes().to_vec())],
    );
    let barracks_refs = placed(b"REFR", BARRACKS_TRINKET_REF, TRINKET, vec![sub(b"EDID", z("BarracksTrinketRef"))]);
    let barracks_children = group(BARRACKS.to_le_bytes(), 6, &group(BARRACKS.to_le_bytes(), 9, &barracks_refs));
    let glob = |fid: u32, name: &str, v: f32| record(b"GLOB", 0, fid, &[sub(b"EDID", z(name)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", v.to_le_bytes().to_vec())]);
    let globals = [
        glob(0x940, "GameYear", 2281.0),
        glob(0x941, "GameMonth", 9.0),
        glob(0x942, "GameDay", 18.0),
        glob(0x943, "GameHour", 12.0),
        glob(0x944, "GameDaysPassed", DAYS_PASSED_START as f32),
        glob(0x945, "TimeScale", 30.0),
    ]
    .concat();
    // `SetPCEnemyOfFaction Town 1` and `SetPCEnemyOfFaction Market 1` in a GameMode block.
    let flag = |r: u16| st(0x10C6, &[le16(2), vec![b'r'], le16(r), vec![b'n'], 1i32.to_le_bytes().to_vec()].concat());
    let mut hostility = unit(0, &script(&[(0, None, [flag(1), flag(2)].concat())]), &[], &[TOWN, MARKET]);
    hostility.push(sub(b"EDID", z("CrimeHostility")));
    let hostility = record(b"SCPT", 0, HOSTILITY_SCRIPT, &hostility);
    [
        tes4(),
        top(b"GLOB", &globals),
        top(b"SCPT", &hostility),
        top(b"REPU", &reputations),
        top(b"FACT", &factions),
        top(b"QUST", &quest),
        top(b"DIAL", &dial),
        top(b"NPC_", &npcs),
        top(b"MISC", &misc),
        top(b"DOOR", &door),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children, barracks, barracks_children].concat()))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Crime.esm".into()).unwrap()];
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
    g.player_entered(Some(FormId(SHOP)));
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(SHOP));
    g
}

fn call(g: &mut Game, op: u16, on: Option<u32>, args: &[ArgValue]) -> f64 {
    let ctx = Ctx { unit: "test".into(), opcode: op, on: on.map(FormId), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    match g.call_function(&ctx, args) {
        Value::Num(x) => x,
        Value::Ref(f) => f as f64,
    }
}

fn karma(g: &mut Game) -> f64 {
    call(g, GET_AV, Some(0x14), &[ArgValue::Enum(KARMA)])
}

/// `w` sees the player now.
fn sees(g: &mut Game, w: u32) {
    let t = g.time;
    g.detection.pairs.insert((FormId(w), FormId::PLAYER_REF), (Detection { value: 10.0, band: Band::Detected, ..Default::default() }, t));
}

fn crimes(g: &mut Game) -> Vec<(Kind, Vec<FormId>)> {
    g.crimes.take_notes().into_iter().filter_map(|n| if let Note::Crime { kind, hostile, .. } = n { Some((kind, hostile)) } else { None }).collect()
}

#[test]
fn the_records_are_read() {
    let f = fixture();
    let c = f.data.cells[&FormId(SHOP)];
    assert_eq!((c.owner, c.public, c.interior), (Some(FormId(SHOPKEEPER)), false, true));
    assert_eq!(f.data.faction_flags[&FormId(TOWN)], 256);
    assert_eq!(f.data.faction_flags[&FormId(GANG)], 256 | 2);
    assert_eq!(f.data.owner_ranks[&FormId(GANG_TRINKET_REF)], 1);
}

#[test]
fn ownership_comes_from_the_reference_then_the_cell() {
    let f = fixture();
    let mut g = game(&f);
    let o = g.ownership(FormId(TRINKET_REF)).unwrap();
    assert_eq!((o.owner, o.from_cell), (FormId(SHOPKEEPER), true));
    let o = g.ownership(FormId(GANG_TRINKET_REF)).unwrap();
    assert_eq!((o.owner, o.rank, o.from_cell), (FormId(GANG), Some(1), false));
    assert!(g.owned_by_other(FormId(TRINKET_REF)).is_some());
    assert!(g.counts_as_owner(FormId(SHOPKEEPER_REF), &g.ownership(FormId(TRINKET_REF)).unwrap()));
    // The player in the gang at rank 1 owns the gang's trinket; at rank 0 not.
    g.extra.entry(FormId::PLAYER_REF).or_default().factions.insert(FormId(GANG), 0);
    assert!(g.owned_by_other(FormId(GANG_TRINKET_REF)).is_some());
    g.extra.entry(FormId::PLAYER_REF).or_default().factions.insert(FormId(GANG), 1);
    assert!(g.owned_by_other(FormId(GANG_TRINKET_REF)).is_none());
    // SetOwnership with no owner: the player's.
    g.world.get(FormId(TRINKET_REF)).unwrap().extra.owner = Some(FormId::PLAYER_BASE);
    assert!(g.owned_by_other(FormId(TRINKET_REF)).is_none());
    assert_eq!(call(&mut g, IS_CELL_OWNER, None, &[ArgValue::Value(Value::Ref(SHOP)), ArgValue::Value(Value::Ref(SHOPKEEPER_REF))]), 1.0);
}

#[test]
fn an_unseen_theft_costs_karma_and_turns_nobody() {
    let f = fixture();
    let mut g = game(&f);
    assert!(g.player_stole(Some(FormId(TRINKET_REF)), None, FormId(TRINKET), 1));
    assert_eq!(karma(&mut g), -5.0);
    assert!(crimes(&mut g).iter().all(|(k, h)| *k == Kind::Theft && h.is_empty()));
    assert!(!g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    assert_eq!(call(&mut g, GET_MINOR_CRIME_COUNT, Some(SHOPKEEPER), &[]), 1.0);
    // Stealing from an Evil faction costs no karma.
    assert!(g.player_stole(Some(FormId(GANG_TRINKET_REF)), None, FormId(TRINKET), 1));
    assert_eq!(karma(&mut g), -5.0);
}

#[test]
fn the_owner_who_sees_a_theft_turns_on_the_player() {
    let f = fixture();
    let mut g = game(&f);
    // An unaggressive shopkeeper and a townsman of the same faction both see it; only the
    // owner turns (a minor crime).
    sees(&mut g, SHOPKEEPER_REF);
    sees(&mut g, TOWNSMAN_REF);
    assert!(!g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    g.player_stole(Some(FormId(TRINKET_REF)), None, FormId(TRINKET), 1);
    assert_eq!(crimes(&mut g), vec![(Kind::Theft, vec![FormId(SHOPKEEPER_REF)])]);
    assert!(g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    assert!(!g.would_attack(FormId(TOWNSMAN_REF), FormId::PLAYER_REF));
    assert_eq!(call(&mut g, GET_ACTOR_CRIME_PLAYER_ENEMY, Some(SHOPKEEPER_REF), &[]), 1.0);
    assert_eq!(call(&mut g, GET_MINOR_CRIME_COUNT, Some(SHOPKEEPER_REF), &[]), 1.0);
    // StopCombat calms it.
    g.stop_combat(FormId(SHOPKEEPER_REF));
    assert!(!g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
}

fn infamy(g: &mut Game) -> Vec<(FormId, f64, f64)> {
    g.crimes.take_notes().into_iter().filter_map(|n| if let Note::Infamy { reputation, before, after } = n { Some((reputation, before, after)) } else { None }).collect()
}

/// Goods owned by an NPC name its base form (XOWN), not a reference: a seen theft adds the
/// infamy with the reputations of the factions that base is in, and an unseen one none
/// (#1570).
#[test]
fn a_seen_theft_from_an_npc_owner_adds_infamy() {
    let f = fixture();
    let mut g = game(&f);
    assert!(g.player_stole(Some(FormId(TRINKET_REF)), None, FormId(TRINKET), 1));
    assert!(infamy(&mut g).is_empty(), "unseen");
    sees(&mut g, SHOPKEEPER_REF);
    assert!(g.player_stole(Some(FormId(TRINKET_REF)), None, FormId(TRINKET), 1));
    assert_eq!(infamy(&mut g), vec![(FormId(REP_TOWN), 0.0, 2.0)]);
    assert_eq!(g.player.reputation[&FormId(REP_TOWN)], [2.0, 0.0]);
}

/// The factions come from the owner's template when it has Use Factions (not its own list).
#[test]
fn a_seen_theft_from_a_templated_owner_counts_the_templates_factions() {
    let f = fixture();
    let mut g = game(&f);
    sees(&mut g, TOWNSMAN_REF);
    assert!(g.player_stole(Some(FormId(HEIR_TRINKET_REF)), None, FormId(TRINKET), 1));
    assert_eq!(infamy(&mut g), vec![(FormId(REP_TOWN), 0.0, 2.0)]);
}

#[test]
fn a_seen_theft_from_a_faction_adds_its_infamy() {
    let f = fixture();
    let mut g = game(&f);
    sees(&mut g, FIEND_REF);
    assert!(g.player_stole(Some(FormId(GANG_TRINKET_REF)), None, FormId(TRINKET), 1));
    assert_eq!(infamy(&mut g), vec![(FormId(REP_GANG), 0.0, 2.0)]);
}

#[test]
fn an_actor_base_counts_towards_its_records_reputations_once() {
    use fnv_game::functions::reputations_of;
    let f = fixture();
    let g = game(&f);
    // Town and market share one reputation.
    assert_eq!(reputations_of(&g, FormId(ELDER)), vec![FormId(REP_TOWN), FormId(REP_GANG)]);
    assert_eq!(reputations_of(&g, FormId(HEIR)), vec![FormId(REP_TOWN)]);
    assert!(reputations_of(&g, FormId(EXILE)).is_empty(), "rank -1 is not a member");
    assert!(reputations_of(&g, FormId(STRANGER)).is_empty());
    assert_eq!(reputations_of(&g, FormId(GANG)), vec![FormId(REP_GANG)]);
}

/// A reference follows its faction changes at run time; its base keeps what the record says.
#[test]
fn a_reference_counts_towards_the_factions_it_is_in_now() {
    use fnv_game::functions::reputations_of;
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(reputations_of(&g, FormId(SHOPKEEPER_REF)), vec![FormId(REP_TOWN)]);
    g.extra.entry(FormId(SHOPKEEPER_REF)).or_default().factions.insert(FormId(TOWN), -1);
    assert!(reputations_of(&g, FormId(SHOPKEEPER_REF)).is_empty(), "removed from the town");
    assert_eq!(reputations_of(&g, FormId(SHOPKEEPER)), vec![FormId(REP_TOWN)]);
    assert!(reputations_of(&g, FormId(STRANGER_REF)).is_empty());
    g.extra.entry(FormId(STRANGER_REF)).or_default().factions.insert(FormId(GANG), 0);
    assert_eq!(reputations_of(&g, FormId(STRANGER_REF)), vec![FormId(REP_GANG)]);
    assert!(reputations_of(&g, FormId(STRANGER)).is_empty());
}

/// #2456, #2498: a Legionary murdered before a fellow Legionary makes the player an enemy
/// of every crime-tracking faction of the victim's, the tag faction both are in too, which
/// also holds a trooper of the Legion's enemy, as the original flags
/// ArmorVCaesarsLegionFaction (observer check 203). The trooper turns on the player while
/// the flag lasts; 0.18 game hours later the flags are gone and only the witness's own
/// grudge is left.
#[test]
fn a_murder_flags_the_victims_factions_for_minutes() {
    let f = fixture();
    let mut g = game(&f);
    assert!(!g.would_attack(FormId(TROOPER_REF), FormId::PLAYER_REF));
    sees(&mut g, DECANUS_REF);
    g.crimes.take_notes();
    assert!(g.kill(FormId(LEGIONARY_REF), Some(FormId::PLAYER_REF)));
    let notes = g.crimes.take_notes();
    assert!(notes.iter().any(|n| matches!(n, Note::Crime { kind: Kind::Murder, hostile, .. } if hostile == &vec![FormId(DECANUS_REF)])));
    assert!(g.player.enemy_of.contains(&FormId(LEGION)));
    assert!(g.player.enemy_of.contains(&FormId(TAG)), "a faction whose members fight each other is flagged too");
    assert!(g.would_attack(FormId(TROOPER_REF), FormId::PLAYER_REF), "by the tag's flag");
    // 0.17 game hours: still flagged.
    pass_hours(&mut g, 0.17);
    assert!(g.would_attack(FormId(TROOPER_REF), FormId::PLAYER_REF));
    // 0.19: both flags cleared; the witness still fights by its own crime.
    pass_hours(&mut g, 0.02);
    let (mut factions, actors) = ended(&mut g);
    factions.sort();
    assert_eq!((factions, actors), (vec![FormId(LEGION), FormId(TAG)], vec![]));
    assert!(!g.would_attack(FormId(TROOPER_REF), FormId::PLAYER_REF));
    assert!(g.would_attack(FormId(DECANUS_REF), FormId::PLAYER_REF));
}

#[test]
fn killing_by_alignment_and_murder() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(g.alignment(FormId(FIEND_REF)), Alignment::Evil);
    assert_eq!(g.alignment(FormId(TOWNSMAN_REF)), Alignment::Good);
    // An Evil actor (−500 of its own): +fKarmaModKillingEvilActor, no murder (observed:
    // a Fiend and a Powder Ganger at −500 gave +100, capture o6_02, crime-karma.md §4.1).
    assert!(g.kill(FormId(FIEND_REF), Some(FormId::PLAYER_REF)));
    assert_eq!(karma(&mut g), 100.0);
    assert_eq!(call(&mut g, IS_PC_A_MURDERER, None, &[]), 0.0);
    // A Good townsman, seen by the shopkeeper of the same crime-tracking faction: karma
    // lost, murder, the shopkeeper turns and the town becomes the player's enemy.
    sees(&mut g, SHOPKEEPER_REF);
    g.crimes.take_notes();
    assert!(g.kill(FormId(TOWNSMAN_REF), Some(FormId::PLAYER_REF)));
    assert_eq!(karma(&mut g), 0.0);
    assert_eq!(call(&mut g, IS_PC_A_MURDERER, None, &[]), 1.0);
    let notes = g.crimes.take_notes();
    assert!(notes.iter().any(|n| matches!(n, Note::FactionEnemy { faction } if *faction == FormId(TOWN))));
    assert!(notes.iter().any(|n| matches!(n, Note::Crime { kind: Kind::Murder, hostile, .. } if hostile == &vec![FormId(SHOPKEEPER_REF)])));
    assert!(g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    // A Neutral stranger murdered unseen: a quarter of the setting.
    assert!(g.kill(FormId(STRANGER_REF), Some(FormId::PLAYER_REF)));
    assert_eq!(karma(&mut g), -25.0);
    // A convict made from the Fiend's template is as Evil as the Fiend to scripts, but
    // holds 0 itself: killing it gives no karma (the NCRCF convict of 2026-10-19 gave
    // none), and is no murder.
    assert_eq!(g.alignment(FormId(CONVICT_REF)), Alignment::Evil);
    assert_eq!(g.alignment(FormId(CONVICT)), Alignment::Evil);
    assert_eq!(g.kill_alignment(FormId(CONVICT_REF)), Alignment::Neutral);
    assert!(g.kill(FormId(CONVICT_REF), Some(FormId::PLAYER_REF)));
    assert_eq!(karma(&mut g), -25.0);
    assert_eq!(g.crimes.take_notes().iter().filter(|n| matches!(n, Note::Crime { kind: Kind::Murder, .. })).count(), 1);
    // Very Evil: + the setting; Very Good: − the setting (observed: a Sunny copy, 900).
    assert_eq!(g.alignment(FormId(BOSS_REF)), Alignment::VeryEvil);
    assert!(g.kill(FormId(BOSS_REF), Some(FormId::PLAYER_REF)));
    assert_eq!(karma(&mut g), 75.0);
    assert_eq!(g.alignment(FormId(SAINT_REF)), Alignment::VeryGood);
    assert!(g.kill(FormId(SAINT_REF), Some(FormId::PLAYER_REF)));
    assert_eq!(karma(&mut g), -25.0);
}

#[test]
fn kill_karma_by_alignment() {
    use fnv_game::crime::{kill_karma, Settings};
    let s = Settings::default();
    for (a, fight, murder) in [
        (Alignment::VeryEvil, 100.0, 100.0),
        (Alignment::Evil, 100.0, 100.0),
        (Alignment::Neutral, 0.0, -25.0),
        (Alignment::Good, -100.0, -100.0),
        (Alignment::VeryGood, -100.0, -100.0),
    ] {
        assert_eq!(kill_karma(a, false, true, &s), fight, "{a:?} in a fight");
        assert_eq!(kill_karma(a, true, true, &s), murder, "{a:?} murdered");
        // Creatures give none.
        assert_eq!(kill_karma(a, true, false, &s), 0.0, "{a:?} creature");
    }
}

#[test]
fn a_seen_trespasser_is_warned_twice_then_attacked() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(call(&mut g, IS_TRESPASSING, Some(0x14), &[]), 1.0);
    sees(&mut g, SHOPKEEPER_REF);
    let keeper = FormId(SHOPKEEPER_REF);
    assert_eq!(g.warning_to(keeper), None);
    g.crime_tick();
    assert_eq!(call(&mut g, GET_TRESPASS_WARNING_LEVEL, Some(SHOPKEEPER_REF), &[ArgValue::Value(Value::Ref(0x14))]), 1.0);
    // Warning, it draws its weapon, stops and faces the player (not alerted).
    assert_eq!(g.warning_to(keeper), Some(FormId::PLAYER_REF));
    assert!(g.draws_weapon(keeper) && !g.is_alerted(keeper));
    // Too soon for the next warning; it still warns while it sees the player.
    g.time += 1.0;
    sees(&mut g, SHOPKEEPER_REF);
    g.crime_tick();
    assert_eq!(call(&mut g, GET_TRESPASS_WARNING_LEVEL, Some(SHOPKEEPER_REF), &[ArgValue::Value(Value::Ref(0x14))]), 1.0);
    assert_eq!(g.warning_to(keeper), Some(FormId::PLAYER_REF));
    g.time += 5.0;
    sees(&mut g, SHOPKEEPER_REF);
    g.crime_tick();
    assert!(!g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    g.time += 5.0;
    sees(&mut g, SHOPKEEPER_REF);
    assert_eq!(g.crime_tick(), 1);
    assert!(g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    // It fights now: no longer a warning.
    assert_eq!(g.warning_to(keeper), None);
    // A public cell is no trespass.
    g.forms.public_cells.insert(FormId(SHOP), true);
    assert_eq!(call(&mut g, IS_TRESPASSING, Some(0x14), &[]), 0.0);
}

#[test]
fn a_no_alarm_door_excuses_trespassing_in_its_cell() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(call(&mut g, IS_TRESPASSING, Some(0x14), &[]), 1.0);
    // In through the door flagged No Alarm: not trespassing, so no warning.
    g.arrived_through(FormId(QUIET_DOOR_REF), Some(FormId(SHOP)));
    assert!(g.crimes.take_notes().contains(&Note::NoAlarm { door: FormId(QUIET_DOOR_REF), cell: FormId(SHOP) }));
    assert_eq!(call(&mut g, IS_TRESPASSING, Some(0x14), &[]), 0.0);
    sees(&mut g, SHOPKEEPER_REF);
    g.crime_tick();
    assert_eq!(call(&mut g, GET_TRESPASS_WARNING_LEVEL, Some(SHOPKEEPER_REF), &[ArgValue::Value(Value::Ref(0x14))]), 0.0);
    // In through the other door: trespassing again.
    g.arrived_through(FormId(LOUD_DOOR_REF), Some(FormId(SHOP)));
    assert!(g.crimes.take_notes().is_empty());
    assert_eq!(call(&mut g, IS_TRESPASSING, Some(0x14), &[]), 1.0);
}

/// #1615: the door the player uses by hand is the door arrived through, and the cell is
/// left to the engine's loading, unlike a script's `Activate player`.
#[test]
fn the_players_own_door_counts_and_leaves_the_cell_to_the_engine() {
    let f = fixture();
    let mut g = game(&f);
    let shop = FormId(SHOP);
    g.set_loaded(Default::default());
    let done = g.default_activation_by_hand(FormId(QUIET_DOOR_REF)).unwrap().expect("not refused");
    assert!(matches!(done.activation.outcome, Outcome::Teleported { cell: Some(c), .. } if c == shop));
    assert!(g.crimes.take_notes().contains(&Note::NoAlarm { door: FormId(QUIET_DOOR_REF), cell: shop }));
    assert_eq!(call(&mut g, IS_TRESPASSING, Some(0x14), &[]), 0.0);
    assert!(g.loaded_cells.is_empty(), "the engine's loading says when the player is there");
    g.player_entered(Some(shop));
    assert!(g.loaded_cells.contains(&shop));
    // A script's activation has no loading of its own to wait for.
    g.set_loaded(Default::default());
    g.default_activation(FormId::PLAYER_REF, FormId(LOUD_DOOR_REF), false).unwrap();
    assert!(g.loaded_cells.contains(&shop));
}

#[test]
fn reward_karma_leaves_a_note() {
    let f = fixture();
    let mut g = game(&f);
    call(&mut g, REWARD_KARMA, None, &[ArgValue::Value(Value::Num(-75.0))]);
    assert_eq!(karma(&mut g), -75.0);
    assert!(matches!(g.crimes.take_notes().as_slice(), [Note::Karma { change, now, .. }] if *change == -75.0 && *now == -75.0));
}

/// What `actor` said as a Detection-style line since the last look.
fn said(g: &mut Game, actor: u32) -> Vec<&'static str> {
    let barks = std::mem::take(&mut g.awareness.barks);
    barks.into_iter().filter(|b| b.1 == FormId(actor) && b.2.is_some()).map(|b| b.0).collect()
}

/// A witness that ignores crime says the act's NoCrime topic and does not turn on the
/// player [documented: GECK wiki Combat Tab, StealNoCrime, AssaultNoCrime; #974].
#[test]
fn a_witness_that_ignores_crime_says_so() {
    let f = fixture();
    let mut g = game(&f);
    // The lines' quest (start-game enabled) runs from a new game.
    g.new_game();
    g.player_entered(Some(FormId(SHOP)));
    g.extra.entry(FormId(SHOPKEEPER_REF)).or_default().ignore_crime = true;
    sees(&mut g, SHOPKEEPER_REF);
    g.player_stole(Some(FormId(TRINKET_REF)), None, FormId(TRINKET), 1);
    assert_eq!(said(&mut g, SHOPKEEPER_REF), vec!["StealNoCrime"]);
    assert!(!g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF), "it does not turn");
    // An assaulted victim that ignores crime says AssaultNoCrime.
    g.extra.entry(FormId(TOWNSMAN_REF)).or_default().ignore_crime = true;
    g.player_assaulted(FormId(TOWNSMAN_REF));
    assert_eq!(said(&mut g, TOWNSMAN_REF), vec!["AssaultNoCrime"]);
    // One that does not ignore crime says no NoCrime line.
    g.player_assaulted(FormId(STRANGER_REF));
    assert!(said(&mut g, STRANGER_REF).is_empty());
}

// ---- crimes expire (#2467; docs/spec-notes/crime-karma.md §3.7) -------------------------

const SET_PC_ENEMY_OF_FACTION: u16 = 0x10C6;
const GET_PC_ENEMY_OF_FACTION: u16 = 0x10C5;

fn days(g: &Game) -> f64 {
    g.game_days().unwrap()
}

/// Time passes by `hours` as waiting, sleeping and travel move the clock, then a detection
/// round starts (where the engine asks hostility afresh).
fn pass_hours(g: &mut Game, hours: f64) {
    g.world.advance_hours(hours).unwrap();
    g.begin_detection_round();
    g.end_detection_round();
}

/// A script sets the clock (`set GameDaysPassed to …`).
fn set_days(g: &mut Game, to: f64) {
    g.run_query(&format!("Begin Function {{ }}\nset GameDaysPassed to {to}\nEnd\n"), fnv_game::game::RunCtx::default()).unwrap();
}

fn faction_flag(g: &mut Game, faction: u32) -> f64 {
    call(g, GET_PC_ENEMY_OF_FACTION, None, &[ArgValue::Value(Value::Ref(faction))])
}

fn set_faction_flag(g: &mut Game, faction: u32, on: f64) {
    call(g, SET_PC_ENEMY_OF_FACTION, None, &[ArgValue::Value(Value::Ref(faction)), ArgValue::Value(Value::Num(on))]);
}

/// The townsman murdered in front of the shopkeeper: the shopkeeper turns, and the town
/// becomes the player's enemy, so the watchman, in the town only, unaggressive and out of
/// sight, attacks.
fn murder_townsman(g: &mut Game) {
    sees(g, SHOPKEEPER_REF);
    assert!(g.kill(FormId(TOWNSMAN_REF), Some(FormId::PLAYER_REF)));
    assert_eq!(g.player.enemy_of_crimes.get(&FormId(TOWN)).copied(), Some(days(g)), "dated by the crime");
    assert!(g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF), "the town's flag");
    assert!(g.player.crime_enemies.contains(&FormId(SHOPKEEPER_REF)));
}

/// The factions and actors whose crimes ended since the last call.
fn ended(g: &mut Game) -> (Vec<FormId>, Vec<FormId>) {
    let (mut factions, mut actors) = (Vec::new(), Vec::new());
    for n in g.crimes.take_notes() {
        match n {
            Note::FactionEnemyEnded { faction, .. } => factions.push(faction),
            Note::CrimeEnemyEnded { actor, .. } => actors.push(actor),
            _ => {}
        }
    }
    (factions, actors)
}

/// The faction's flag lasts 0.18 game hours (observer check 203, #2498); the witness's
/// own grudge iCoolDownTimerSetting days.
#[test]
fn a_seen_murders_flag_ends_in_minutes_and_the_witnesss_grudge_in_three_days() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(days(&g), DAYS_PASSED_START);
    assert!(!g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF), "before the crime");
    murder_townsman(&mut g);
    g.crimes.take_notes();
    // 0.17 game hours: the town's flag holds.
    pass_hours(&mut g, 0.17);
    assert!(g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF));
    assert_eq!(ended(&mut g), (vec![], vec![]));
    // 0.19: cleared; the witness still fights by its own crime.
    pass_hours(&mut g, 0.02);
    assert!(!g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF), "the flag is cleared");
    assert!(g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    assert_eq!(ended(&mut g), (vec![FormId(TOWN)], vec![]));
    assert_eq!(faction_flag(&mut g, TOWN), 0.0);
    // 71 hours: not yet three days for the witness.
    pass_hours(&mut g, 71.0 - 0.19);
    assert!(g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    assert_eq!(ended(&mut g), (vec![], vec![]));
    // Two more (a sleep): the crime is three days old, the witness forgives.
    pass_hours(&mut g, 2.0);
    assert!(!g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF), "the witness forgave");
    assert_eq!(ended(&mut g), (vec![], vec![FormId(SHOPKEEPER_REF)]));
    assert_eq!(call(&mut g, GET_ACTOR_CRIME_PLAYER_ENEMY, Some(SHOPKEEPER_REF), &[]), 0.0);
    assert!(g.player.enemy_of_crimes.is_empty() && g.player.crime_enemy_since.is_empty());
}

#[test]
fn a_script_jumping_the_clock_or_the_wait_menus_check_ends_crimes_too() {
    let f = fixture();
    let mut g = game(&f);
    murder_townsman(&mut g);
    // A script moves the clock on four days; a script function reads the flag at once.
    set_days(&mut g, DAYS_PASSED_START + 4.0);
    assert_eq!(faction_flag(&mut g, TOWN), 0.0);
    assert!(!g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF));
    // The Wait menu's hourly check reads it after the hours waited, no round between.
    let mut g = game(&f);
    murder_townsman(&mut g);
    g.world.advance_hours(80.0).unwrap();
    assert!(!fnv_game::rest::hostile_to_player(&mut g, FormId(WATCHMAN_REF)));
    // Moving the clock back ends nothing.
    let mut g = game(&f);
    murder_townsman(&mut g);
    set_days(&mut g, DAYS_PASSED_START - 3.0);
    pass_hours(&mut g, 1.0);
    assert!(g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF));
}

#[test]
fn a_scripts_enemy_flag_outlasts_the_cooldown() {
    let f = fixture();
    let mut g = game(&f);
    // SetPCEnemyOfFaction Market 1: no crime, it stays.
    set_faction_flag(&mut g, MARKET, 1.0);
    // The town's flag set by a crime and then by a script: the script's holds it. (A
    // script's flag first would make the townsman's killing self-defence, no crime.)
    murder_townsman(&mut g);
    set_faction_flag(&mut g, TOWN, 1.0);
    g.crimes.take_notes();
    pass_hours(&mut g, 24.0 * 4.0);
    assert_eq!(faction_flag(&mut g, MARKET), 1.0);
    assert_eq!(faction_flag(&mut g, TOWN), 1.0);
    assert!(g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF));
    // The witness's own enmity was the crime's: it ended.
    assert_eq!(ended(&mut g), (vec![], vec![FormId(SHOPKEEPER_REF)]), "no faction flag ended");
    // SetPCEnemyOfFaction 0 clears it.
    set_faction_flag(&mut g, TOWN, 0.0);
    assert!(!g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF));
}

#[test]
fn a_later_crime_restarts_the_count() {
    let f = fixture();
    let mut g = game(&f);
    murder_townsman(&mut g);
    // 0.1 game hours on: the shopkeeper, a crime enemy, killed in front of the second
    // townsman (still murder: a crime of the player's made it hostile) dates the town's
    // flag again.
    pass_hours(&mut g, 0.1);
    g.detection.pairs.clear();
    sees(&mut g, TOWNSMAN2_REF);
    assert!(g.kill(FormId(SHOPKEEPER_REF), Some(FormId::PLAYER_REF)));
    assert!((g.player.enemy_of_crimes[&FormId(TOWN)] - (DAYS_PASSED_START + 0.1 / 24.0)).abs() < 1e-4);
    // 0.25 hours after the first murder, 0.15 after the second: still an enemy.
    pass_hours(&mut g, 0.15);
    assert!(g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF));
    // 0.19 after the second: no more.
    pass_hours(&mut g, 0.04);
    assert!(!g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF));
    // A witness's own enmity restarts with a later crime it sees: a theft on day 2.
    let mut g = game(&f);
    sees(&mut g, SHOPKEEPER_REF);
    g.player_stole(Some(FormId(TRINKET_REF)), None, FormId(TRINKET), 1);
    assert!(g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    pass_hours(&mut g, 48.0);
    sees(&mut g, SHOPKEEPER_REF);
    g.player_stole(Some(FormId(TRINKET_REF)), None, FormId(TRINKET), 1);
    pass_hours(&mut g, 48.0);
    assert!(g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF), "two days after the second theft");
    pass_hours(&mut g, 25.0);
    assert!(!g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
}

#[test]
fn an_older_save_dates_its_crime_enemies_at_the_load_and_keeps_every_faction_flag() {
    let f = fixture();
    let mut g = game(&f);
    // A crime's flag (the town: its member murdered, a counted major crime), a quest's
    // flag on the same faction and a quest's flag alone (the market), and an assault
    // counted against a stranger: none of it tells the old save's flags apart.
    murder_townsman(&mut g);
    set_faction_flag(&mut g, TOWN, 1.0);
    set_faction_flag(&mut g, MARKET, 1.0);
    g.player_assaulted(FormId(STRANGER_REF));
    // Saved a day after the crime, as a save from before #2467: flags and crime enemies
    // undated.
    set_days(&mut g, DAYS_PASSED_START + 1.0);
    let mut old = serde_json::to_value(g.snapshot()).unwrap();
    let player = old["player"].as_object_mut().unwrap();
    for k in ["enemy_of_crimes", "enemy_of_script", "crime_enemy_since"] {
        assert!(player.remove(k).is_some(), "{k} is saved");
    }
    let old: fnv_game::game::GameSnapshot = serde_json::from_value(old).unwrap();
    let world = World::restore(&f.forms, &f.refs, &old.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &old).unwrap();
    let load_day = days(&h);
    assert!((load_day - (DAYS_PASSED_START + 1.0)).abs() < 1e-3);
    // The witness is dated at the load; every faction flag is kept as a script's.
    assert_eq!(h.player.crime_enemy_since.get(&FormId(SHOPKEEPER_REF)).copied(), Some(load_day));
    assert!(h.player.enemy_of_crimes.is_empty());
    assert!(h.player.enemy_of_script.contains(&FormId(TOWN)) && h.player.enemy_of_script.contains(&FormId(MARKET)));
    // Three days after the crime, two after the load: the witness is still a crime enemy.
    pass_hours(&mut h, 48.0);
    assert_eq!(call(&mut h, GET_ACTOR_CRIME_PLAYER_ENEMY, Some(SHOPKEEPER_REF), &[]), 1.0);
    // Three days after the load the witness forgives its own crime (it still fights by the
    // town's flag, kept); the flags stay.
    pass_hours(&mut h, 25.0);
    assert_eq!(call(&mut h, GET_ACTOR_CRIME_PLAYER_ENEMY, Some(SHOPKEEPER_REF), &[]), 0.0);
    assert!(h.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    pass_hours(&mut h, 24.0 * 10.0);
    assert!(h.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF), "the town's flag is kept");
    assert_eq!(faction_flag(&mut h, TOWN), 1.0);
    assert_eq!(faction_flag(&mut h, MARKET), 1.0);
    // A save made now keeps the dates as they are.
    let snap = h.snapshot();
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let k = Game::restore(&f.data, world, h.cfg, &snap).unwrap();
    assert_eq!(k.player, h.player);
}

#[test]
fn only_town_and_market_can_be_flagged_by_a_script() {
    let f = fixture();
    let g = game(&f);
    assert_eq!(g.script_flaggable_factions(), Some([FormId(TOWN), FormId(MARKET)].into_iter().collect()));
}

/// A save from before #2467 holding a flag on a faction no script can set: the flag was
/// a crime's, dated at the load, and ends FACTION_FLAG_DAYS later (#3335).
#[test]
fn an_older_saves_flag_no_script_can_set_is_dated_as_a_crimes() {
    let f = fixture();
    let mut g = game(&f);
    set_faction_flag(&mut g, MARKET, 1.0);
    g.player.enemy_of.insert(FormId(GANG));
    let mut old = serde_json::to_value(g.snapshot()).unwrap();
    let player = old["player"].as_object_mut().unwrap();
    for k in ["enemy_of_crimes", "enemy_of_script", "crime_enemy_since"] {
        player.remove(k);
    }
    let old: fnv_game::game::GameSnapshot = serde_json::from_value(old).unwrap();
    let world = World::restore(&f.forms, &f.refs, &old.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &old).unwrap();
    let load_day = days(&h);
    assert_eq!(h.player.enemy_of_crimes.get(&FormId(GANG)).copied(), Some(load_day));
    assert!(h.player.enemy_of_script.contains(&FormId(MARKET)) && !h.player.enemy_of_script.contains(&FormId(GANG)));
    assert_eq!(faction_flag(&mut h, GANG), 1.0);
    pass_hours(&mut h, 0.2);
    assert_eq!(faction_flag(&mut h, GANG), 0.0, "Gang's flag ended as a crime's");
    assert_eq!(faction_flag(&mut h, MARKET), 1.0, "Market's stays: a script can set it");
}

/// A save made by a build that kept every undated flag as a script's (#2467 to #3335)
/// holds a crime's flag among the script's: on a faction no script can set, it ends at
/// the load; on one a script can set, it stays (#3335, player report R-833D3F5X).
#[test]
fn a_crimes_flag_kept_as_a_scripts_ends_at_the_load_when_no_script_sets_it() {
    let f = fixture();
    let mut g = game(&f);
    set_faction_flag(&mut g, MARKET, 1.0);
    g.player.enemy_of.insert(FormId(GANG));
    g.player.enemy_of_script.insert(FormId(GANG));
    let snap = g.snapshot();
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &snap).unwrap();
    assert!(!h.player.enemy_of.contains(&FormId(GANG)) && !h.player.enemy_of_script.contains(&FormId(GANG)));
    assert_eq!(ended(&mut h).0, vec![FormId(GANG)]);
    assert_eq!(faction_flag(&mut h, GANG), 0.0);
    assert_eq!(faction_flag(&mut h, MARKET), 1.0);
}

/// A second murder seen only by witnesses already hostile is still a crime against the
/// faction: its flag's count restarts (#2467 review).
#[test]
fn a_murder_seen_only_by_hostile_witnesses_restarts_the_count() {
    let f = fixture();
    let mut g = game(&f);
    // Day 0: the townsman murdered in front of the shopkeeper and the second townsman;
    // both turn.
    sees(&mut g, TOWNSMAN2_REF);
    murder_townsman(&mut g);
    assert!(g.player.crime_enemies.contains(&FormId(TOWNSMAN2_REF)));
    // Day 2: the second townsman (a crime enemy: still murder) killed in front of the
    // shopkeeper alone, who is a crime enemy already and turns no further. The town's
    // flag, gone since minutes after the first murder, is set again.
    pass_hours(&mut g, 48.0);
    assert!(!g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF));
    g.detection.pairs.clear();
    sees(&mut g, SHOPKEEPER_REF);
    g.crimes.take_notes();
    assert!(g.kill(FormId(TOWNSMAN2_REF), Some(FormId::PLAYER_REF)));
    assert!(g.crimes.take_notes().iter().any(|n| matches!(n, Note::Crime { kind: Kind::Murder, hostile, .. } if hostile.is_empty())));
    assert!((g.player.enemy_of_crimes[&FormId(TOWN)] - (DAYS_PASSED_START + 2.0)).abs() < 1e-3, "the town's count restarts");
    assert!((g.player.crime_enemy_since[&FormId(SHOPKEEPER_REF)] - (DAYS_PASSED_START + 2.0)).abs() < 1e-3, "so does the witness's");
    assert!(g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF));
    // Four days after the first murder, two after the second: the witness still fights,
    // the town's flag is long gone.
    pass_hours(&mut g, 48.0);
    assert!(!g.would_attack(FormId(WATCHMAN_REF), FormId::PLAYER_REF));
    assert!(g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
    // Three days after the second: no more.
    pass_hours(&mut g, 25.0);
    assert!(!g.would_attack(FormId(SHOPKEEPER_REF), FormId::PLAYER_REF));
}

const SEND_ASSAULT_ALARM: u16 = 0x105F;

/// `stranger.SendAssaultAlarm Player Gang`, as a freed Techatticup hostage calls it on the
/// mine's Legion faction (#2926).
fn alarm_on_gang(g: &mut Game) {
    call(g, SEND_ASSAULT_ALARM, Some(STRANGER_REF), &[ArgValue::Value(Value::Ref(0x14)), ArgValue::Value(Value::Ref(GANG))]);
}

/// The trace's last `assault_alarm` event: (victim, alive, absent, dead).
fn last_alarm(g: &Game) -> Option<(Option<FormId>, usize, usize, usize)> {
    g.events.iter().rev().find_map(|e| match &e.kind {
        fnv_game::trace::EventKind::AssaultAlarm { caller, faction, victim, alive, absent, dead } => {
            assert_eq!((*caller, *faction), (Some(FormId(STRANGER_REF)), Some(FormId(GANG))));
            Some((*victim, *alive, *absent, *dead))
        }
        _ => None,
    })
}

/// #2926: with the player named and a faction given, the victim is a present member of the
/// faction [documented: GECK wiki SendAssaultAlarm], never the calling actor, which is in
/// no faction.
#[test]
fn an_assault_alarm_on_a_faction_turns_a_member_not_the_caller() {
    let f = fixture();
    let mut g = game(&f);
    g.crimes.take_notes();
    alarm_on_gang(&mut g);
    assert_eq!(crimes(&mut g), vec![(Kind::Assault, vec![FormId(FIEND_REF)])]);
    assert!(g.crime_enemy(FormId(FIEND_REF)));
    assert!(!g.crime_enemy(FormId(STRANGER_REF)));
    assert_eq!(last_alarm(&g), Some((Some(FormId(FIEND_REF)), 1, 0, 0)));
}

/// #2926: with no member present (disabled) or alive, nobody is the victim: no crime, and
/// the caller is not turned [design, #2926; hypothesis: the original does nothing].
#[test]
fn an_assault_alarm_on_a_faction_with_no_member_turns_nobody() {
    let f = fixture();
    let mut g = game(&f);
    g.world.set_enabled(FormId(FIEND_REF), false).unwrap();
    g.crimes.take_notes();
    alarm_on_gang(&mut g);
    assert_eq!(crimes(&mut g), vec![]);
    assert!(!g.crime_enemy(FormId(STRANGER_REF)) && !g.crime_enemy(FormId(FIEND_REF)));
    assert_eq!(last_alarm(&g), Some((None, 0, 1, 0)));

    let mut g = game(&f);
    assert!(g.kill(FormId(FIEND_REF), None));
    g.crimes.take_notes();
    alarm_on_gang(&mut g);
    assert_eq!(crimes(&mut g), vec![]);
    assert!(!g.crime_enemy(FormId(STRANGER_REF)));
    assert_eq!(last_alarm(&g), Some((None, 0, 0, 1)));
}

/// The form with no faction keeps the calling actor as the victim (SCPT 0x0002A43C's
/// OnHit calls it alone).
#[test]
fn an_assault_alarm_without_a_faction_turns_the_caller() {
    let f = fixture();
    let mut g = game(&f);
    g.crimes.take_notes();
    call(&mut g, SEND_ASSAULT_ALARM, Some(STRANGER_REF), &[]);
    assert_eq!(crimes(&mut g), vec![(Kind::Assault, vec![FormId(STRANGER_REF)])]);
    assert!(g.crime_enemy(FormId(STRANGER_REF)));
    assert_eq!(last_alarm(&g), None, "the form without a faction picks no member");
}

/// The kill loop of the headless walkthrough techatticup-hostages-dead-2926.actions.json
/// (every actor of a faction in the player's cell killed, as when the player has cleared a
/// mine): with the faction's one member killed, the alarm on the faction turns nobody, not
/// the caller (#2926, the reporter's case).
#[test]
fn an_assault_alarm_after_the_factions_members_were_killed_turns_nobody() {
    let f = fixture();
    let mut g = game(&f);
    let script = "Begin Function { }\nref rActor\nint iKilled\nlet rActor := GetFirstRef 200 0\nwhile rActor\n  if rActor.GetInFaction Gang\n    rActor.Kill\n    let iKilled += 1\n  endif\n  let rActor := GetNextRef\nloop\nSetFunctionValue iKilled\nEnd\n";
    let killed = g.run_query(script, Default::default());
    assert_eq!(killed, Ok(fnv_script::vm::XVal::Num(1.0)), "{:?}", g.events);
    assert!(g.is_dead(FormId(FIEND_REF)));
    g.crimes.take_notes();
    alarm_on_gang(&mut g);
    assert_eq!(crimes(&mut g), vec![]);
    assert!(!g.crime_enemy(FormId(STRANGER_REF)));
}

fn restore<'a>(f: &'a Fixture, snap: &fnv_game::game::GameSnapshot) -> Game<'a> {
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    Game::restore(&f.data, world, GameConfig { strict: true, ..GameConfig::default() }, snap).unwrap()
}

fn eval_num(g: &mut Game, expr: &str) -> f64 {
    let src = format!("Begin Function {{ }}\nSetFunctionValue ({expr})\nEnd\n");
    match g.run_query(&src, Default::default()) {
        Ok(fnv_script::vm::XVal::Num(n)) => n,
        other => panic!("expected Num for `{expr}`, got {other:?} (events: {:?})", g.events),
    }
}

fn eval_ref(g: &mut Game, expr: &str) -> u32 {
    let src = format!("Begin Function {{ }}\nSetFunctionValue ({expr})\nEnd\n");
    match g.run_query(&src, Default::default()) {
        Ok(fnv_script::vm::XVal::Ref(r)) => r,
        other => panic!("expected Ref for `{expr}`, got {other:?} (events: {:?})", g.events),
    }
}

fn run_stmt(g: &mut Game, stmt: &str) {
    let src = format!("Begin Function {{ }}\n{stmt}\nEnd\n");
    g.run_query(&src, Default::default()).unwrap_or_else(|e| panic!("stmt `{stmt}` failed: {e:?} (events: {:?})", g.events));
}

#[test]
fn set_owning_faction_required_rank_drives_ownership_theft_and_session_transfer() {
    let f = fixture();
    let mut g = game(&f);

    // Baseline: GangTrinketRef has authored XOWN = Gang, XRNK = 1.
    // Player at Gang rank 1 owns it across Game::ownership, IsOwned, and HasOwnership.
    g.extra.entry(FormId::PLAYER_REF).or_default().factions.insert(FormId(GANG), 1);
    assert_eq!(eval_num(&mut g, "GangTrinketRef.GetOwningFactionRequiredRank"), 1.0);
    assert_eq!(g.ownership(FormId(GANG_TRINKET_REF)).map(|o| (o.owner, o.rank)), Some((FormId(GANG), Some(1))));
    assert!(g.owned_by_other(FormId(GANG_TRINKET_REF)).is_none());
    assert_eq!(eval_num(&mut g, "GangTrinketRef.IsOwned PlayerREF"), 1.0);
    assert_eq!(eval_num(&mut g, "PlayerREF.HasOwnership GangTrinketRef"), 1.0);

    // Raise required rank to 2: rank-1 player loses ownership everywhere, and a seen theft turns FiendRef hostile.
    run_stmt(&mut g, "GangTrinketRef.SetOwningFactionRequiredRank 2");
    assert_eq!(eval_num(&mut g, "GangTrinketRef.GetOwningFactionRequiredRank"), 2.0);
    assert_eq!(g.ownership(FormId(GANG_TRINKET_REF)).map(|o| (o.owner, o.rank)), Some((FormId(GANG), Some(2))));
    assert!(g.owned_by_other(FormId(GANG_TRINKET_REF)).is_some());
    assert_eq!(eval_num(&mut g, "GangTrinketRef.IsOwned PlayerREF"), 0.0);
    assert_eq!(eval_num(&mut g, "PlayerREF.HasOwnership GangTrinketRef"), 0.0);

    sees(&mut g, FIEND_REF);
    assert!(!g.would_attack(FormId(FIEND_REF), FormId::PLAYER_REF));
    assert!(g.player_stole(Some(FormId(GANG_TRINKET_REF)), None, FormId(TRINKET), 1));
    assert_eq!(crimes(&mut g), vec![(Kind::Theft, vec![FormId(FIEND_REF)])]);
    assert!(g.would_attack(FormId(FIEND_REF), FormId::PLAYER_REF));

    // Fresh-run restore without put_session resets session-only rank override back to authored XRNK = 1.
    let snap = g.snapshot();
    let mut fresh = restore(&f, &snap);
    assert_eq!(eval_num(&mut fresh, "GangTrinketRef.GetOwningFactionRequiredRank"), 1.0);
    assert_eq!(fresh.ownership(FormId(GANG_TRINKET_REF)).map(|o| o.rank), Some(Some(1)));
    assert!(fresh.owned_by_other(FormId(GANG_TRINKET_REF)).is_none());
    assert_eq!(eval_num(&mut fresh, "GangTrinketRef.IsOwned PlayerREF"), 1.0);

    // Same-run save load transfers session-only owner_ranks through Game::take_session / put_session.
    let session = g.take_session();
    let mut same_run = restore(&f, &snap);
    same_run.put_session(session);
    assert_eq!(eval_num(&mut same_run, "GangTrinketRef.GetOwningFactionRequiredRank"), 2.0);
    assert_eq!(same_run.ownership(FormId(GANG_TRINKET_REF)).map(|o| o.rank), Some(Some(2)));
    assert!(same_run.owned_by_other(FormId(GANG_TRINKET_REF)).is_some());
    assert_eq!(eval_num(&mut same_run, "GangTrinketRef.IsOwned PlayerREF"), 0.0);
    assert_eq!(eval_num(&mut same_run, "PlayerREF.HasOwnership GangTrinketRef"), 0.0);

    // Negative required rank does not grant ownership to a non-member (faction_rank == -1), only members (>= 0).
    run_stmt(&mut same_run, "GangTrinketRef.SetOwningFactionRequiredRank -1");
    assert_eq!(eval_num(&mut same_run, "GangTrinketRef.GetOwningFactionRequiredRank"), -1.0);
    assert_eq!(eval_num(&mut same_run, "GangTrinketRef.IsOwned StrangerRef"), 0.0);
    assert_eq!(eval_num(&mut same_run, "StrangerRef.HasOwnership GangTrinketRef"), 0.0);
    assert_eq!(eval_num(&mut same_run, "GangTrinketRef.IsOwned FiendRef"), 1.0);
    assert_eq!(eval_num(&mut same_run, "FiendRef.HasOwnership GangTrinketRef"), 1.0);
}

#[test]
fn live_ownership_edits_flow_to_get_owner_is_owned_and_get_owner_of_cell() {
    let f = fixture();
    let mut g = game(&f);

    // Baseline authored ownership.
    assert_eq!(eval_ref(&mut g, "GangTrinketRef.GetOwner"), GANG);
    assert_eq!(eval_ref(&mut g, "TrinketRef.GetOwner"), 0);
    assert_eq!(eval_ref(&mut g, "GetOwnerOfCell Shop"), SHOPKEEPER);
    assert_eq!(eval_num(&mut g, "TrinketRef.IsOwned ShopkeeperRef"), 1.0);
    assert_eq!(eval_num(&mut g, "ShopkeeperRef.HasOwnership TrinketRef"), 1.0);
    assert_eq!(eval_num(&mut g, "TrinketRef.IsOwned PlayerREF"), 0.0);

    // Inherited cell XRNK: GangBarracks requires Gang rank 2.
    g.extra.entry(FormId::PLAYER_REF).or_default().factions.insert(FormId(GANG), 1);
    assert_eq!(eval_num(&mut g, "BarracksTrinketRef.IsOwned PlayerREF"), 0.0);
    assert_eq!(eval_num(&mut g, "PlayerREF.HasOwnership BarracksTrinketRef"), 0.0);
    g.extra.entry(FormId::PLAYER_REF).or_default().factions.insert(FormId(GANG), 2);
    assert_eq!(eval_num(&mut g, "BarracksTrinketRef.IsOwned PlayerREF"), 1.0);
    assert_eq!(eval_num(&mut g, "PlayerREF.HasOwnership BarracksTrinketRef"), 1.0);
    g.extra.entry(FormId::PLAYER_REF).or_default().factions.remove(&FormId(GANG));

    // SetOwnership on a reference flows to GetOwner, IsOwned, and HasOwnership.
    run_stmt(&mut g, "TrinketRef.SetOwnership");
    assert_eq!(eval_ref(&mut g, "TrinketRef.GetOwner"), FormId::PLAYER_BASE.0);
    assert_eq!(eval_num(&mut g, "TrinketRef.IsOwned PlayerREF"), 1.0);
    assert_eq!(eval_num(&mut g, "PlayerREF.HasOwnership TrinketRef"), 1.0);
    assert_eq!(eval_num(&mut g, "TrinketRef.IsOwned ShopkeeperRef"), 0.0);

    // ClearOwnership clears live reference ownership so GetOwner returns 0 and IsOwned falls back to the cell.
    run_stmt(&mut g, "GangTrinketRef.ClearOwnership");
    assert_eq!(eval_ref(&mut g, "GangTrinketRef.GetOwner"), 0);
    assert_eq!(eval_num(&mut g, "GangTrinketRef.IsOwned ShopkeeperRef"), 1.0);
    assert_eq!(eval_num(&mut g, "GangTrinketRef.IsOwned FiendRef"), 0.0);

    // SetCellOwnership flows to GetOwnerOfCell and inherited reference ownership.
    run_stmt(&mut g, "SetCellOwnership Shop Town");
    assert_eq!(eval_ref(&mut g, "GetOwnerOfCell Shop"), TOWN);
    assert_eq!(eval_num(&mut g, "GangTrinketRef.IsOwned TownsmanRef"), 1.0);
    assert_eq!(eval_num(&mut g, "TownsmanRef.HasOwnership GangTrinketRef"), 1.0);
    assert_eq!(eval_num(&mut g, "GangTrinketRef.IsOwned FiendRef"), 0.0);

    // SetOwnershipTemp flows to GetOwner / GetOwnerOfCell / IsOwned, and snapshot restore reverts it.
    run_stmt(&mut g, "TrinketRef.SetOwnershipTemp Gang");
    run_stmt(&mut g, "SetOwnershipTemp Gang Shop");
    assert_eq!(eval_ref(&mut g, "TrinketRef.GetOwner"), GANG);
    assert_eq!(eval_num(&mut g, "TrinketRef.IsOwned FiendRef"), 1.0);
    assert_eq!(eval_num(&mut g, "TrinketRef.IsOwned PlayerREF"), 0.0);
    assert_eq!(eval_ref(&mut g, "GetOwnerOfCell Shop"), GANG);

    let snap = g.snapshot();
    let mut restored = restore(&f, &snap);
    assert_eq!(eval_ref(&mut restored, "TrinketRef.GetOwner"), FormId::PLAYER_BASE.0);
    assert_eq!(eval_ref(&mut restored, "GetOwnerOfCell Shop"), TOWN);
}
