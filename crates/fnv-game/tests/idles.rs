//! Idle animations asked for by the rules (`fnv_game::idles`) on a plugin built in
//! memory (no game data): a package's begin and change idles, `PlayIdle`,
//! `IsIdlePlaying`, and one script package at a time. The rules and their sources:
//! docs/spec-notes/idles.md.

use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::functions::Ctx;
use fnv_game::game::{Game, GameConfig};
use fnv_game::idles::{IdleRequest, IdleSource};
use fnv_game::trace::EventKind;
use fnv_game::GameData;
use fnv_script::vm::{ArgValue, Value};
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

const IDLE_WAKE: u32 = 0xA01;
const IDLE_SIT: u32 = 0xA02;
const IDLE_FOLDER: u32 = 0xA03;
const IDLE_OWN: u32 = 0xA04;
const IDLE_UNREAD: u32 = 0xA05;
const IDLE_COWER: u32 = 0xA06;
/// Two leaves under `IDLE_FOLDER`: one for Somebody only, then one for anybody.
const IDLE_LEAF_OWN: u32 = 0xA07;
const IDLE_LEAF_ANY: u32 = 0xA08;
/// A third leaf, of the Idle section (DATA section 0): a standing look.
const IDLE_LEAF_LOOK: u32 = 0xA09;
/// A fourth leaf, a special idle wanting GetUnconscious == 1: the switched-off pose
/// (FalloutNV.esm's `ProtectronTurnedOff`, #1977).
const IDLE_LEAF_OFF: u32 = 0xA0A;
const P_FIRST: u32 = 0xB01;
const P_SECOND: u32 = 0xB02;
const P_PLAIN: u32 = 0xB03;
const NPC: u32 = 0xC01;
const ACTOR: u32 = 0xD01;
const CELL: u32 = 0xE01;
/// A topic no line belongs to: saying it shows as `EventKind::NoLine`.
const TOPIC: u32 = 0xF01;

/// An IDLE: its file, loop minimum and maximum (the 8-byte DATA form).
fn idle(fid: u32, edid: &str, model: &str, loops: (u8, u8)) -> Vec<u8> {
    record(
        b"IDLE",
        fid,
        &[sub(b"EDID", z(edid)), sub(b"MODL", z(model)), sub(b"ANAM", vec![0; 8]), sub(b"DATA", vec![0x47, loops.0, loops.1, 0, 0, 0, 0, 0])],
    )
}

/// A condition `function(param) == 1` on the subject (the 28-byte CTDA form).
fn ctda(function: u16, param: u32) -> Subrecord {
    let mut d = vec![0u8; 4];
    d.extend(1.0f32.to_le_bytes());
    d.extend(function.to_le_bytes());
    d.extend([0, 0]);
    d.extend(param.to_le_bytes());
    d.extend([0u8; 12]);
    sub(b"CTDA", d)
}

/// An IDLE with a file and conditions.
fn idle_if(fid: u32, edid: &str, conds: Vec<Subrecord>) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"MODL", z("Characters\\_Male\\IdleAnims\\Own.kf"))];
    subs.extend(conds);
    subs.extend([sub(b"ANAM", vec![0; 8]), sub(b"DATA", vec![7, 0, 0, 0, 0, 0, 0, 0])]);
    record(b"IDLE", fid, &subs)
}

const GET_IS_ID: u16 = 72;
/// GetPlantedExplosive (condition function index).
const GET_PLANTED_EXPLOSIVE: u16 = 0x1AC;
const IDLE_PANIC: u32 = 0xC40;
const IDLE_PANIC_PA: u32 = 0xC41;
const IDLE_PANIC_PLAIN: u32 = 0xC42;
/// As FalloutNV.esm's activation branch (idles.md §16, #2349): a root wanting
/// GetUsedItemActivate == 1, a node below it wanting IsWaterObject == 1, and its two
/// leaves, the first for the player in first person, the second for anybody.
const IDLE_ACTIVATE: u32 = 0xC50;
const IDLE_ACT_WATER: u32 = 0xC51;
const IDLE_WATER_1ST: u32 = 0xC52;
const IDLE_WATER_3RD: u32 = 0xC53;
/// Condition function indices: GetUsedItemActivate, IsWaterObject, IsPC1stPerson.
const GET_USED_ITEM_ACTIVATE: u16 = 259;
const IS_WATER_OBJECT: u16 = 304;
const IS_PC_1ST_PERSON: u16 = 392;
/// An activator that gives water (ACTI WNAM, a sink) and one that does not, placed.
const SINK: u32 = 0xC60;
const CRATE: u32 = 0xC61;
const SINK_REF: u32 = 0xC62;
const CRATE_REF: u32 = 0xC63;
/// The water the sink names (no WATR record is needed for it to count as water).
const WATER: u32 = 0xC64;

/// An IDLE with a file below `parent` in the tree, after `previous` (ANAM), with conditions.
fn leaf(fid: u32, edid: &str, parent: u32, previous: u32, conds: Vec<Subrecord>) -> Vec<u8> {
    leaf_in(fid, edid, parent, previous, conds, 7)
}

/// As [`leaf`], of animation group section `section` (DATA's first byte).
fn leaf_in(fid: u32, edid: &str, parent: u32, previous: u32, conds: Vec<Subrecord>, section: u8) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"MODL", z("Characters\\_Male\\IdleAnims\\Leaf.kf"))];
    subs.extend(conds);
    subs.extend([sub(b"ANAM", [parent.to_le_bytes(), previous.to_le_bytes()].concat()), sub(b"DATA", vec![section, 0, 0, 0, 0, 0, 0, 0])]);
    record(b"IDLE", fid, &subs)
}

/// A condition `function == value` on the subject, joined to the next by OR when `or`.
fn ctda_eq(function: u16, value: f32, or: bool) -> Subrecord {
    let mut d = vec![u8::from(or), 0, 0, 0];
    d.extend(value.to_le_bytes());
    d.extend(function.to_le_bytes());
    d.extend([0u8; 18]);
    sub(b"CTDA", d)
}

/// IsMoving and GetCurrentAIProcedure (condition function indices).
const IS_MOVING: u16 = 25;
const GET_UNCONSCIOUS: u16 = 242;
const GET_CURRENT_AI_PROCEDURE: u16 = 143;

/// A Travel package at the editor location with a begin idle, a change idle and an idle
/// collection run in sequence; [`P_SECOND`] also has a begin topic ([`TOPIC`]).
fn pack(fid: u32, edid: &str, begin: Option<u32>, change: Option<u32>, collection: &[u32]) -> Vec<u8> {
    let mut subs = vec![
        sub(b"EDID", z(edid)),
        sub(b"PKDT", [&0u32.to_le_bytes()[..], &[6, 0], &[0; 6]].concat()),
        sub(b"PLDT", [0i32.to_le_bytes(), 0u32.to_le_bytes(), 0i32.to_le_bytes()].concat()),
        sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat()),
    ];
    if !collection.is_empty() {
        subs.push(sub(b"IDLF", vec![1]));
        subs.push(sub(b"IDLC", vec![collection.len() as u8]));
        subs.push(sub(b"IDLT", 0f32.to_le_bytes().to_vec()));
        subs.push(sub(b"IDLA", collection.iter().flat_map(|i| i.to_le_bytes()).collect()));
    }
    for (marker, idle) in [(b"POBA", begin), (b"POEA", None), (b"POCA", change)] {
        subs.push(sub(marker, vec![]));
        subs.push(sub(b"INAM", idle.unwrap_or(0).to_le_bytes().to_vec()));
        if fid == P_SECOND && marker == b"POBA" {
            subs.push(sub(b"TNAM", TOPIC.to_le_bytes().to_vec()));
        }
    }
    record(b"PACK", fid, &subs)
}

fn plugin_bytes() -> Vec<u8> {
    let idles = [
        idle(IDLE_WAKE, "LooseWake", "Characters\\_Male\\IdleAnims\\Wake.kf", (255, 255)),
        idle(IDLE_SIT, "LooseSit", "Characters\\_Male\\IdleAnims\\Sit.kf", (0, 0)),
        idle(IDLE_FOLDER, "Folder", "Characters\\_Male\\IdleAnims", (0, 0)),
        idle_if(IDLE_OWN, "OnlySomebody", vec![ctda(GET_IS_ID, NPC)]),
        // A function index the runtime has no function for.
        idle_if(IDLE_UNREAD, "Unknown", vec![ctda(4000, 0)]),
        // As FalloutNV.esm's CowerIdles: not moving, fleeing (16) or fleeing a non-combat
        // threat (34).
        idle_if(IDLE_COWER, "Cower", vec![ctda_eq(IS_MOVING, 0.0, false), ctda_eq(GET_CURRENT_AI_PROCEDURE, 16.0, true), ctda_eq(GET_CURRENT_AI_PROCEDURE, 34.0, false)]),
        leaf(IDLE_LEAF_OWN, "LeafOwn", IDLE_FOLDER, 0, vec![ctda(GET_IS_ID, NPC)]),
        leaf(IDLE_LEAF_ANY, "LeafAny", IDLE_FOLDER, IDLE_LEAF_OWN, vec![]),
        leaf_in(IDLE_LEAF_LOOK, "LeafLook", IDLE_FOLDER, IDLE_LEAF_ANY, vec![ctda(GET_IS_ID, NPC)], 0),
        leaf(IDLE_LEAF_OFF, "LeafOff", IDLE_FOLDER, IDLE_LEAF_LOOK, vec![ctda_eq(GET_UNCONSCIOUS, 1.0, false)]),
        // As FalloutNV.esm's PanicExplosivePlanted: a root of the folder that wants
        // GetPlantedExplosive == 1, its power armour leaf (here: never) before the plain one.
        record(
            b"IDLE",
            IDLE_PANIC,
            &[
                sub(b"EDID", z("PanicExplosivePlanted")),
                sub(b"MODL", z("Characters\\_Male\\IdleAnims")),
                ctda_eq(GET_PLANTED_EXPLOSIVE, 1.0, false),
                sub(b"ANAM", [0u32.to_le_bytes(), IDLE_FOLDER.to_le_bytes()].concat()),
                sub(b"DATA", vec![7, 0, 0, 0, 0, 0, 0, 0]),
            ],
        ),
        leaf(IDLE_PANIC_PA, "PanicGrenadePlantPA", IDLE_PANIC, 0, vec![ctda(GET_IS_ID, 0xBAD)]),
        leaf(IDLE_PANIC_PLAIN, "PanicGrenadePlant", IDLE_PANIC, IDLE_PANIC_PA, vec![]),
        record(
            b"IDLE",
            IDLE_ACTIVATE,
            &[
                sub(b"EDID", z("ActivateIdles")),
                sub(b"MODL", z("Characters\\_Male\\IdleAnims")),
                ctda_eq(GET_USED_ITEM_ACTIVATE, 1.0, false),
                sub(b"ANAM", [0u32.to_le_bytes(), IDLE_PANIC.to_le_bytes()].concat()),
                sub(b"DATA", vec![7, 0, 0, 0, 0, 0, 0, 0]),
            ],
        ),
        record(
            b"IDLE",
            IDLE_ACT_WATER,
            &[
                sub(b"EDID", z("ActWaterSource")),
                sub(b"MODL", z("Characters\\_Male\\IdleAnims")),
                ctda_eq(IS_WATER_OBJECT, 1.0, false),
                sub(b"ANAM", [IDLE_ACTIVATE.to_le_bytes(), 0u32.to_le_bytes()].concat()),
                sub(b"DATA", vec![7, 0, 0, 0, 0, 0, 0, 0]),
            ],
        ),
        leaf(IDLE_WATER_1ST, "1stPersonActWater", IDLE_ACT_WATER, 0, vec![ctda(GET_IS_ID, 0x7), ctda_eq(IS_PC_1ST_PERSON, 1.0, false)]),
        leaf(IDLE_WATER_3RD, "3rdPersonActWater", IDLE_ACT_WATER, IDLE_WATER_1ST, vec![]),
    ]
    .concat();
    let actis = [
        record(b"ACTI", SINK, &[sub(b"EDID", z("TestSink")), sub(b"MODL", z("Test\\Sink.nif")), sub(b"WNAM", WATER.to_le_bytes().to_vec())]),
        record(b"ACTI", CRATE, &[sub(b"EDID", z("TestCrate")), sub(b"MODL", z("Test\\Crate.nif"))]),
    ]
    .concat();
    let packs = [
        pack(P_FIRST, "First", Some(IDLE_WAKE), None, &[IDLE_WAKE, IDLE_SIT]),
        pack(P_SECOND, "Second", Some(IDLE_SIT), Some(IDLE_WAKE), &[]),
        pack(P_PLAIN, "Plain", None, None, &[]),
    ]
    .concat();
    let npcs = [
        record(b"NPC_", 0x7, &[sub(b"EDID", z("Player"))]),
        record(b"NPC_", NPC, &[sub(b"EDID", z("Somebody")), sub(b"PKID", P_PLAIN.to_le_bytes().to_vec())]),
    ]
    .concat();
    let actor = [
        record(b"ACHR", ACTOR, &[sub(b"EDID", z("SomebodyRef")), sub(b"NAME", NPC.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]),
        record(b"REFR", SINK_REF, &[sub(b"NAME", SINK.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]),
        record(b"REFR", CRATE_REF, &[sub(b"NAME", CRATE.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]),
    ]
    .concat();
    let cell = [record(b"CELL", CELL, &[sub(b"EDID", z("TestCell"))]), group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &actor))].concat();
    [
        tes4(),
        top(b"NPC_", &npcs),
        top(b"ACTI", &actis),
        top(b"IDLE", &idles),
        top(b"PACK", &packs),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Idles.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    Game::new(&f.data, world, GameConfig { strict: true, package_interval: Some(10.0), ..GameConfig::default() })
}

fn call(g: &mut Game, opcode: u16, on: FormId, args: &[ArgValue]) -> Value {
    let ctx = Ctx { unit: "test".into(), opcode, on: Some(on), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, args)
}

const PLAY_IDLE: u16 = 0x1190;
const SET_UNCONSCIOUS: u16 = 0x10F1;
const IS_IDLE_PLAYING: u16 = 0x1070;
const ADD_SCRIPT_PACKAGE: u16 = 0x1097;
const REMOVE_SCRIPT_PACKAGE: u16 = 0x1098;

fn asked(g: &mut Game) -> Vec<(u32, u32, IdleSource)> {
    std::mem::take(&mut g.idle_requests).into_iter().map(|IdleRequest { actor, idle, source, .. }| (actor.0, idle.0, source)).collect()
}

#[test]
fn idle_records_and_package_idle_data_are_read() {
    let f = fixture();
    let d = &f.data;
    assert_eq!(d.idles.len(), 17, "13, and the four of the activation branch");
    let wake = &d.idles[&FormId(IDLE_WAKE)];
    assert_eq!((wake.model.as_deref(), wake.loop_min, wake.loop_max, wake.has_file()), (Some("Characters\\_Male\\IdleAnims\\Wake.kf"), 255, 255, true));
    assert!(!d.idles[&FormId(IDLE_FOLDER)].has_file());
    let first = &d.package_defs[&FormId(P_FIRST)];
    assert_eq!(first.events[0].idle, Some(FormId(IDLE_WAKE)));
    assert_eq!(first.events[2].idle, None);
    let c = first.idles.as_ref().unwrap();
    assert_eq!((c.run_in_sequence, c.do_once, &c.idles), (true, false, &vec![FormId(IDLE_WAKE), FormId(IDLE_SIT)]));
}

#[test]
fn an_idle_is_allowed_by_its_own_conditions() {
    let f = fixture();
    let mut g = game(&f);
    let (actor, player) = (FormId(ACTOR), FormId::PLAYER_REF);
    // No conditions: anybody.
    assert!(g.idle_allowed(actor, FormId(IDLE_SIT)) && g.idle_allowed(player, FormId(IDLE_SIT)));
    // GetIsID Somebody == 1.
    assert_eq!(f.data.idles[&FormId(IDLE_OWN)].conditions.as_ref().map(Vec::len), Some(1));
    assert!(g.idle_allowed(actor, FormId(IDLE_OWN)));
    assert!(!g.idle_allowed(player, FormId(IDLE_OWN)));
    // A function the runtime does not have answers 0, as in any condition: `== 1` fails.
    // The idle can still be played by name.
    assert_eq!(f.data.idles[&FormId(IDLE_UNREAD)].conditions.as_ref().map(Vec::len), Some(1));
    assert!(!g.idle_allowed(actor, FormId(IDLE_UNREAD)));
    assert!(g.ask_idle(actor, FormId(IDLE_UNREAD), IdleSource::Script));
    // Not an idle at all.
    assert!(!g.idle_allowed(actor, FormId(NPC)));
}

#[test]
fn play_idle_takes_the_editor_id_and_ignores_what_is_not_an_animation() {
    let f = fixture();
    let mut g = game(&f);
    let actor = FormId(ACTOR);
    call(&mut g, PLAY_IDLE, actor, &[ArgValue::Str("loosesit".into())]);
    assert_eq!(asked(&mut g), [(ACTOR, IDLE_SIT, IdleSource::Script)]);
    assert!(g.events.iter().any(|e| e.kind == EventKind::IdleAsked { actor, idle: FormId(IDLE_SIT), source: IdleSource::Script }));
    // An inner node of the idle tree names a folder: nothing to play.
    call(&mut g, PLAY_IDLE, actor, &[ArgValue::Str("Folder".into())]);
    assert!(asked(&mut g).is_empty());
    let reason = g.events.iter().find_map(|e| match &e.kind {
        EventKind::IdleNotPlayed { idle, reason, .. } if *idle == FormId(IDLE_FOLDER) => Some(*reason),
        _ => None,
    });
    assert_eq!(reason, Some("names no animation file"));
    // A name that is no idle at all (here an NPC's).
    call(&mut g, PLAY_IDLE, actor, &[ArgValue::Str("Somebody".into())]);
    assert!(asked(&mut g).is_empty());
}

#[test]
fn is_idle_playing_answers_what_the_animating_layer_reports() {
    let f = fixture();
    let mut g = game(&f);
    let actor = FormId(ACTOR);
    // Asked for, but nothing animates: not playing.
    call(&mut g, PLAY_IDLE, actor, &[ArgValue::Str("LooseSit".into())]);
    assert_eq!(call(&mut g, IS_IDLE_PLAYING, actor, &[]).num(), 0.0);
    g.set_idle_playing(actor, Some(FormId(IDLE_SIT)));
    assert_eq!(call(&mut g, IS_IDLE_PLAYING, actor, &[]).num(), 1.0);
    assert_eq!(g.idle_playing(actor), Some(FormId(IDLE_SIT)));
    g.set_idle_playing(actor, None);
    assert_eq!(call(&mut g, IS_IDLE_PLAYING, actor, &[]).num(), 0.0);
}

#[test]
fn a_package_asks_for_its_begin_and_change_idles() {
    let f = fixture();
    let mut g = game(&f);
    let player = FormId::PLAYER_REF;
    let form = |p: u32| ArgValue::Value(Value::Ref(p));
    // The first script package starts: its begin idle.
    call(&mut g, ADD_SCRIPT_PACKAGE, player, &[form(P_FIRST)]);
    assert_eq!(asked(&mut g), [(player.0, IDLE_WAKE, IdleSource::PackageBegin)]);
    let (package, collection) = g.package_idles(player).unwrap();
    assert_eq!((package, collection.idles.len()), (FormId(P_FIRST), 2));
    // The same package given again changes nothing.
    call(&mut g, ADD_SCRIPT_PACKAGE, player, &[form(P_FIRST)]);
    assert!(asked(&mut g).is_empty());
    // Another one replaces it: the old package has no change idle, the new one's begin
    // idle is asked for.
    call(&mut g, ADD_SCRIPT_PACKAGE, player, &[form(P_SECOND)]);
    assert_eq!(asked(&mut g), [(player.0, IDLE_SIT, IdleSource::PackageBegin)]);
    assert!(g.package_idles(player).is_none());
    // An actor holds one script package: removing it leaves none (the first is not
    // back), and the package left asks for its change idle.
    call(&mut g, ADD_SCRIPT_PACKAGE, player, &[form(P_SECOND)]);
    call(&mut g, REMOVE_SCRIPT_PACKAGE, player, &[]);
    assert_eq!(asked(&mut g), [(player.0, IDLE_WAKE, IdleSource::PackageChange)]);
    assert_eq!(g.extra[&player].current_package, None);
    assert!(g.extra[&player].script_package.is_none());
}

/// A package's begin topic is said after its result script and before its begin idle
/// is asked for (docs/spec-notes/packages.md §7.2).
#[test]
fn a_package_says_its_begin_topic() {
    let f = fixture();
    let mut g = game(&f);
    let actor = FormId(ACTOR);
    assert_eq!(f.data.package_defs[&FormId(P_SECOND)].events[0].topic, Some(FormId(TOPIC)));
    let from = g.events.len();
    call(&mut g, ADD_SCRIPT_PACKAGE, actor, &[ArgValue::Value(Value::Ref(P_SECOND))]);
    let kinds: Vec<&EventKind> = g.events[from..].iter().map(|e| &e.kind).collect();
    let said = kinds.iter().position(|k| **k == EventKind::NoLine { speaker: actor, topic: FormId(TOPIC) }).expect("the topic is said");
    let idle = kinds.iter().position(|k| matches!(k, EventKind::IdleAsked { idle, .. } if *idle == FormId(IDLE_SIT))).expect("the idle is asked for");
    assert!(said < idle);
    // A package without a topic says nothing.
    let from = g.events.len();
    call(&mut g, ADD_SCRIPT_PACKAGE, actor, &[ArgValue::Value(Value::Ref(P_FIRST))]);
    assert!(!g.events[from..].iter().any(|e| matches!(e.kind, EventKind::NoLine { .. })));
}

/// The tree's root idles that want the flee procedure play on a fleeing actor standing
/// still (docs/spec-notes/idles.md §13).
#[test]
fn a_fleeing_actor_standing_still_gets_the_cower_idle() {
    let f = fixture();
    let mut g = game(&f);
    let actor = FormId(ACTOR);
    let folder = "characters\\_male";
    assert_eq!(g.flee_idle(actor, folder, &|_| false), None, "not fleeing");
    g.start_combat(actor, FormId::PLAYER_REF);
    assert_eq!(g.flee_idle(actor, folder, &|_| false), None, "fighting, not fleeing");
    assert!(g.wants_weapon_out(actor), "fighting: the weapon is out");
    g.start_flight(actor, FormId::PLAYER_REF, [0.0; 3]);
    assert!(!g.wants_weapon_out(actor), "fleeing: the weapon is holstered");
    assert_eq!(g.flee_idle(actor, folder, &|_| false), Some(FormId(IDLE_COWER)));
    assert_eq!(g.flee_idle(actor, folder, &|i| i == FormId(IDLE_COWER)), None, "held back");
    assert_eq!(g.flee_idle(actor, "creatures\\dog", &|_| false), None, "another folder");
    // On its way it stands; at its flee spot a person hides crouched (combat-ai.md §12.2.1).
    assert!(!g.at_flee_spot(actor) && !g.sneaks(actor));
    let now = g.time;
    g.awareness.tactics.flights.get_mut(&actor).unwrap().arrived = Some(now);
    assert!(g.at_flee_spot(actor) && g.sneaks(actor));
    g.end_flight(actor);
    assert_eq!(g.flee_idle(actor, folder, &|_| false), None, "fighting again");
    assert!(g.wants_weapon_out(actor), "fighting again: drawn again");
}

/// PickIdle: the actor is queued for the animating layer, which knows its folder; the
/// tree's pick is the first leaf whose conditions pass (script-stubs.md §3.19).
#[test]
fn pick_idle_walks_the_tree() {
    let f = fixture();
    let mut g = game(&f);
    let actor = FormId(ACTOR);
    call(&mut g, 0x1064, actor, &[]);
    call(&mut g, 0x1064, actor, &[]);
    assert_eq!(g.idle_picks, vec![actor], "each actor once");
    assert!(asked(&mut g).is_empty(), "nothing played before the animating layer picks");
    let folder = "characters\\_male";
    assert_eq!(g.pick_idle(actor, folder, &|_| false), Some(FormId(IDLE_LEAF_OWN)));
    assert_eq!(g.pick_idle(FormId::PLAYER_REF, folder, &|_| false), Some(FormId(IDLE_LEAF_ANY)), "the player is not Somebody");
    assert_eq!(g.pick_idle(actor, folder, &|i| i == FormId(IDLE_LEAF_OWN)), Some(FormId(IDLE_LEAF_ANY)), "held back");
    assert_eq!(g.pick_idle(actor, "creatures\\dog", &|_| false), None, "another folder: nothing");
}

/// The panic of a victim of a planted explosive: the root naming GetPlantedExplosive,
/// its first leaf whose conditions pass, only while the explosive is live
/// (pickpocket.md §5).
#[test]
fn the_panic_idle_is_the_root_naming_get_planted_explosive() {
    let f = fixture();
    let mut g = game(&f);
    let actor = FormId(ACTOR);
    let folder = "characters\\_male";
    assert_eq!(g.panic_idle(actor, folder, &|_| false), None, "nothing planted");
    let start = g.time;
    g.plant_explosive(actor, FormId(0x4330), fnv_game::pickpocket::Explosive { damage: 125.0, ..Default::default() }).unwrap();
    assert_eq!(g.panic_idle(actor, folder, &|_| false), None, "before the freak-out time");
    g.time = start + 0.2;
    assert_eq!(g.panic_idle(actor, folder, &|_| false), Some(FormId(IDLE_PANIC_PLAIN)), "the power armour leaf's conditions fail");
    assert_eq!(g.panic_idle(actor, "creatures\\dog", &|_| false), None, "a creature's folder has none");
    // The tree's other walks pass the panic root by.
    assert_eq!(g.pick_idle(FormId::PLAYER_REF, folder, &|_| false), Some(FormId(IDLE_LEAF_ANY)));
    g.time = start + 2.1;
    g.tick_planted();
    assert_eq!(g.panic_idle(actor, folder, &|_| false), None, "gone off");
}

/// A standing look: the tree walked as for PickIdle, only the Idle section's leaves
/// counted (#1542).
#[test]
fn standing_idle_takes_only_idle_section_leaves() {
    let f = fixture();
    let mut g = game(&f);
    let folder = "characters\\_male";
    assert_eq!(g.standing_idle(FormId(ACTOR), folder), Some(FormId(IDLE_LEAF_LOOK)), "the special idles before it are passed over");
    assert_eq!(g.standing_idle(FormId::PLAYER_REF, folder), None, "its conditions fail: no look");
    assert_eq!(g.standing_idle(FormId(ACTOR), "creatures\\dog"), None, "another folder: nothing");
}

/// SetUnconscious holds an actor standing in its tree's GetUnconscious idle, the
/// switched-off pose, in place of any other look, until SetUnconscious 0 (#1977).
#[test]
fn an_unconscious_actor_stands_in_its_switched_off_idle() {
    let f = fixture();
    let mut g = game(&f);
    let actor = FormId(ACTOR);
    let folder = "characters\\_male";
    assert_eq!(g.unconscious_idle(actor, folder), None, "conscious: no off pose");
    call(&mut g, SET_UNCONSCIOUS, actor, &[ArgValue::Value(Value::Num(1.0))]);
    assert_eq!(g.unconscious_idle(actor, folder), Some(FormId(IDLE_LEAF_OFF)));
    assert_eq!(g.standing_idle(actor, folder), Some(FormId(IDLE_LEAF_OFF)), "before the Idle-section look");
    assert_eq!(g.unconscious_idle(actor, "creatures\\dog"), None, "another folder: nothing");
    call(&mut g, SET_UNCONSCIOUS, actor, &[ArgValue::Value(Value::Num(0.0))]);
    assert_eq!(g.standing_idle(actor, folder), Some(FormId(IDLE_LEAF_LOOK)), "switched on: its look again");
}

/// #2349: the player's activation asks the tree's activation root, whose conditions
/// choose: while the player activates placeable water (an activator with a Water Type),
/// the way goes through ActWaterSource to the first-person leaf in first person and to
/// the third-person one in third; activating anything else, or nothing, gives no idle.
/// Only the player's activation is queued for the animating layer, and only that walk
/// sees the activation in GetUsedItemActivate: elsewhere, and for an NPC that has just
/// activated something, it answers and picks as before (idles.md §16).
#[test]
fn activating_placeable_water_picks_the_drinking_idle() {
    let f = fixture();
    let mut g = game(&f);
    let p = FormId::PLAYER_REF;
    let folder = "characters\\_male";
    let names = |g: &Game, way: &[FormId]| way.iter().map(|i| g.data.name(*i)).collect::<Vec<_>>();
    assert_eq!(g.activate_idle(p, folder, &|_| false), None, "activating nothing");
    assert_eq!(call(&mut g, 0x1103, p, &[]).num(), 0.0, "GetUsedItemActivate: nothing used or activated");
    // A crate: the root passes (activating), the water node does not.
    g.default_activation_by_hand(FormId(CRATE_REF)).unwrap().expect("not refused");
    assert_eq!(g.activate_idles, vec![p], "the player's activation asks the tree");
    assert_eq!(call(&mut g, 0x1103, p, &[]).num(), 0.0, "GetUsedItemActivate outside the activation walk: as before");
    assert_eq!(g.activate_idle(p, folder, &|_| false), None, "a crate is no water");
    g.activate_idles.clear();
    // The sink.
    g.default_activation_by_hand(FormId(SINK_REF)).unwrap().expect("not refused");
    assert_eq!(g.activate_idles, vec![p]);
    let (idle, way) = g.activate_idle(p, folder, &|_| false).expect("the drinking idle");
    assert_eq!(idle, FormId(IDLE_WATER_1ST), "first person (the default view)");
    assert_eq!(names(&g, &way), ["ActivateIdles", "ActWaterSource", "1stPersonActWater"]);
    g.set_first_person(false);
    let (idle, way) = g.activate_idle(p, folder, &|_| false).expect("the drinking idle");
    assert_eq!(idle, FormId(IDLE_WATER_3RD), "third person");
    assert_eq!(names(&g, &way), ["ActivateIdles", "ActWaterSource", "3rdPersonActWater"]);
    assert_eq!(g.activate_idle(p, folder, &|i| i == FormId(IDLE_WATER_3RD)), None, "held back");
    assert_eq!(g.activate_idle(p, "creatures\\dog", &|_| false), None, "another folder: nothing");
    // The tree's other walks pass the activation root by, even with the folder's own
    // leaves held back (the activation root next in chain order stays shut).
    let folder_leaves = |i: FormId| [IDLE_LEAF_OWN, IDLE_LEAF_ANY, IDLE_LEAF_LOOK, IDLE_LEAF_OFF].contains(&i.0);
    assert_eq!(g.pick_idle(p, folder, &|_| false), Some(FormId(IDLE_LEAF_ANY)));
    assert_eq!(g.pick_idle(p, folder, &folder_leaves), None, "PickIdle does not see the activation");
    // An NPC that has just activated the sink: not queued, GetUsedItemActivate as before,
    // and its picks unchanged.
    let npc = FormId(ACTOR);
    g.activate_idles.clear();
    g.default_activation(npc, FormId(SINK_REF), false).unwrap();
    assert!(g.activate_idles.is_empty(), "only the player's activation asks the tree");
    assert_eq!(g.activating(npc), Some(FormId(SINK_REF)));
    assert_eq!(call(&mut g, 0x1103, npc, &[]).num(), 0.0, "the NPC's GetUsedItemActivate as before");
    assert_eq!(g.pick_idle(npc, folder, &|_| false), Some(FormId(IDLE_LEAF_OWN)), "its PickIdle as before");
    assert_eq!(g.pick_idle(npc, folder, &folder_leaves), None, "not the drinking idle");
    assert_eq!(g.standing_idle(npc, folder), Some(FormId(IDLE_LEAF_LOOK)), "its standing look as before");
    // The player's drink still picks it.
    let (idle, way) = g.activate_idle(p, folder, &|_| false).expect("the drinking idle");
    assert_eq!((idle, names(&g, &way)[1].as_str()), (FormId(IDLE_WATER_3RD), "ActWaterSource"));
    assert_eq!(call(&mut g, 0x1103, p, &[]).num(), 0.0, "the walk over: as before");
    // The activation is over.
    g.time += fnv_game::functions::senses::ACTIVATING_SECONDS + 1.0;
    assert_eq!(g.activate_idle(p, folder, &|_| false), None, "no longer activating");
    assert_eq!(call(&mut g, 0x1103, p, &[]).num(), 0.0);
}

/// #2349 review: a `held_back` that panics during the activation walk lets the panic go on,
/// and GetUsedItemActivate gives its ordinary answer afterwards (the walk's scope is put
/// back on unwinding).
#[test]
fn a_panicking_walk_puts_get_used_item_activate_back() {
    let f = fixture();
    let mut g = game(&f);
    let p = FormId::PLAYER_REF;
    let folder = "characters\\_male";
    g.default_activation_by_hand(FormId(SINK_REF)).unwrap().expect("not refused");
    assert_eq!(call(&mut g, 0x1103, p, &[]).num(), 0.0, "outside the walk");
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        g.activate_idle(p, folder, &|_| panic!("held_back fails"));
    }));
    assert!(caught.is_err(), "the panic goes on to the caller");
    assert_eq!(call(&mut g, 0x1103, p, &[]).num(), 0.0, "the walk's scope was put back");
    assert_eq!(g.activate_idle(p, folder, &|_| false).map(|(i, _)| i), Some(FormId(IDLE_WATER_1ST)), "and the next walk works");
}

#[test]
fn temp_cloned_idles_admit_force_play_idle_and_preserve_clone_identity_and_paths() {
    use fnv_script::vm::XVal;

    let f = fixture();
    let mut g = game(&f);
    let actor = FormId(ACTOR);
    let orig = FormId(IDLE_SIT);

    // Clone an authored IDLE (`TempCloneForm LooseSit 1`) and verify `idle_def` and
    // `idle_animation_path` resolve through the clone while keeping the clone's own ID.
    let XVal::Ref(clone_raw) = fnv_game::nvse::call(&mut g, &Default::default(), "tempcloneform", None, &[XVal::Ref(orig.0), XVal::Num(1.0)]).unwrap().unwrap() else {
        panic!("expected clone FormId");
    };
    let clone = FormId(clone_raw);
    assert_ne!(clone, orig);
    assert_eq!(g.idle_def(clone).and_then(|d| d.model.as_deref()), Some("Characters\\_Male\\IdleAnims\\Sit.kf"));
    assert_eq!(g.idle_animation_path(clone), Some("Characters\\_Male\\IdleAnims\\Sit.kf"));

    // Override the clone's animation path via SetIdleAnimPath: the clone reads its own
    // override, while the original IDLE keeps its authored path.
    fnv_game::nvse::call(&mut g, &Default::default(), "setidleanimpath", None, &[XVal::Ref(clone.0), XVal::Str("Owned\\Inject.kf".into())]).unwrap().unwrap();
    assert_eq!(g.idle_animation_path(clone), Some("Owned\\Inject.kf"));
    assert_eq!(g.idle_animation_path(orig), Some("Characters\\_Male\\IdleAnims\\Sit.kf"));
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getidleanimpath", None, &[XVal::Ref(clone.0)]).unwrap().unwrap(),
        XVal::Str("Owned\\Inject.kf".into()),
    );
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getidleanimpath", None, &[XVal::Ref(orig.0)]).unwrap().unwrap(),
        XVal::Str("Characters\\_Male\\IdleAnims\\Sit.kf".into()),
    );

    // ForcePlayIdle admits the clone and queues IdleRequest with the clone's exact FormID.
    fnv_game::nvse::call(&mut g, &Default::default(), "forceplayidle", Some(actor), &[XVal::Ref(clone.0)]).unwrap().unwrap();
    assert_eq!(asked(&mut g), [(ACTOR, clone.0, IdleSource::Script)]);
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getplayedidle", Some(actor), &[]).unwrap().unwrap(),
        XVal::Ref(clone.0),
    );

    // Once the animating layer reports the clone playing, IsIdlePlayingEx distinguishes
    // the clone from its original.
    g.set_idle_playing(actor, Some(clone));
    assert_eq!(fnv_game::nvse::call(&mut g, &Default::default(), "isidleplayingex", Some(actor), &[XVal::Ref(clone.0)]).unwrap().unwrap(), XVal::Num(1.0));
    assert_eq!(fnv_game::nvse::call(&mut g, &Default::default(), "isidleplayingex", Some(actor), &[XVal::Ref(orig.0)]).unwrap().unwrap(), XVal::Num(0.0));

    // Clone-of-clone inherits the parent clone's SetIdleAnimPath override unless it sets its own.
    let XVal::Ref(clone2_raw) = fnv_game::nvse::call(&mut g, &Default::default(), "tempcloneform", None, &[XVal::Ref(clone.0), XVal::Num(1.0)]).unwrap().unwrap() else {
        panic!("expected second-generation clone FormId");
    };
    let clone2 = FormId(clone2_raw);
    assert_eq!(g.idle_animation_path(clone2), Some("Owned\\Inject.kf"));
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getidleanimpath", None, &[XVal::Ref(clone2.0)]).unwrap().unwrap(),
        XVal::Str("Owned\\Inject.kf".into()),
    );
    fnv_game::nvse::call(&mut g, &Default::default(), "playidleex", Some(actor), &[XVal::Ref(clone2.0)]).unwrap().unwrap();
    assert_eq!(asked(&mut g), [(ACTOR, clone2.0, IdleSource::Script)]);

    // Clearing the clone's animation path to an empty or non-.kf string causes ask_idle to refuse it.
    fnv_game::nvse::call(&mut g, &Default::default(), "setidleanimpath", None, &[XVal::Ref(clone2.0), XVal::Str(String::new())]).unwrap().unwrap();
    assert!(!g.ask_idle(actor, clone2, IdleSource::Script));
    assert!(asked(&mut g).is_empty());
    let reason = g.events.iter().rev().find_map(|e| match &e.kind {
        EventKind::IdleNotPlayed { idle, reason, .. } if *idle == clone2 => Some(*reason),
        _ => None,
    });
    assert_eq!(reason, Some("names no animation file"));

    // Cloning an inner folder node (no .kf) stays refused until SetIdleAnimPath assigns a .kf file.
    let XVal::Ref(folder_clone_raw) = fnv_game::nvse::call(&mut g, &Default::default(), "tempcloneform", None, &[XVal::Ref(IDLE_FOLDER), XVal::Num(1.0)]).unwrap().unwrap() else {
        panic!("expected folder clone FormId");
    };
    let folder_clone = FormId(folder_clone_raw);
    assert!(!g.ask_idle(actor, folder_clone, IdleSource::Script));
    fnv_game::nvse::call(&mut g, &Default::default(), "setidleanimpath", None, &[XVal::Ref(folder_clone.0), XVal::Str("Owned\\FromFolder.kf".into())]).unwrap().unwrap();
    assert!(g.ask_idle(actor, folder_clone, IdleSource::Script));
    assert_eq!(asked(&mut g), [(ACTOR, folder_clone.0, IdleSource::Script)]);

    // Cloning a non-IDLE form (an NPC) is still rejected as "not an idle animation".
    let XVal::Ref(npc_clone_raw) = fnv_game::nvse::call(&mut g, &Default::default(), "tempcloneform", None, &[XVal::Ref(NPC), XVal::Num(1.0)]).unwrap().unwrap() else {
        panic!("expected non-idle clone FormId");
    };
    let npc_clone = FormId(npc_clone_raw);
    assert!(!g.ask_idle(actor, npc_clone, IdleSource::Script));
    let non_idle_reason = g.events.iter().rev().find_map(|e| match &e.kind {
        EventKind::IdleNotPlayed { idle, reason, .. } if *idle == npc_clone => Some(*reason),
        _ => None,
    });
    assert_eq!(non_idle_reason, Some("not an idle animation"));

    // idle_allowed resolves conditions through the cloned IDLE's original definition.
    let XVal::Ref(own_clone_raw) = fnv_game::nvse::call(&mut g, &Default::default(), "tempcloneform", None, &[XVal::Ref(IDLE_OWN), XVal::Num(1.0)]).unwrap().unwrap() else {
        panic!("expected conditioned idle clone FormId");
    };
    let own_clone = FormId(own_clone_raw);
    assert!(g.idle_allowed(actor, own_clone));
    assert!(!g.idle_allowed(FormId::PLAYER_REF, own_clone));
}

#[test]
fn tempclone_idle_snapshots_modl_path_and_shares_set_model_path() {
    use fnv_script::vm::XVal;

    let f = fixture();
    let mut g = game(&f);
    let orig = FormId(IDLE_SIT);

    // SetModelPath and SetIdleAnimPath mutate the same underlying MODL path.
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getidleanimpath", None, &[XVal::Ref(orig.0)]).unwrap().unwrap(),
        XVal::Str("Characters\\_Male\\IdleAnims\\Sit.kf".into()),
    );
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getmodelpath", None, &[XVal::Ref(orig.0)]).unwrap().unwrap(),
        XVal::Str("Characters\\_Male\\IdleAnims\\Sit.kf".into()),
    );
    fnv_game::nvse::call(&mut g, &Default::default(), "setmodelpath", None, &[XVal::Str("Owned\\PreIdle.kf".into()), XVal::Ref(orig.0)]).unwrap().unwrap();
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getidleanimpath", None, &[XVal::Ref(orig.0)]).unwrap().unwrap(),
        XVal::Str("Owned\\PreIdle.kf".into()),
    );

    // Clone inherits pre-clone MODL edit at creation time.
    let XVal::Ref(cl1_raw) = fnv_game::nvse::call(&mut g, &Default::default(), "tempcloneform", None, &[XVal::Ref(orig.0), XVal::Num(1.0)]).unwrap().unwrap() else {
        panic!("expected IDLE clone FormId");
    };
    let cl1 = FormId(cl1_raw);
    assert_eq!(g.idle_animation_path(cl1), Some("Owned\\PreIdle.kf"));
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getidleanimpath", None, &[XVal::Ref(cl1.0)]).unwrap().unwrap(),
        XVal::Str("Owned\\PreIdle.kf".into()),
    );
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getmodelpath", None, &[XVal::Ref(cl1.0)]).unwrap().unwrap(),
        XVal::Str("Owned\\PreIdle.kf".into()),
    );

    // Post-clone SetIdleAnimPath edit on original does NOT bleed into existing clone.
    fnv_game::nvse::call(&mut g, &Default::default(), "setidleanimpath", None, &[XVal::Ref(orig.0), XVal::Str("Owned\\PostOrig.kf".into())]).unwrap().unwrap();
    assert_eq!(g.idle_animation_path(orig), Some("Owned\\PostOrig.kf"));
    assert_eq!(g.idle_animation_path(cl1), Some("Owned\\PreIdle.kf"));
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getidleanimpath", None, &[XVal::Ref(cl1.0)]).unwrap().unwrap(),
        XVal::Str("Owned\\PreIdle.kf".into()),
    );
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getmodelpath", None, &[XVal::Ref(cl1.0)]).unwrap().unwrap(),
        XVal::Str("Owned\\PreIdle.kf".into()),
    );

    // Clone-of-clone snapshots parent clone's path and stays isolated from later parent edits.
    let XVal::Ref(cl2_raw) = fnv_game::nvse::call(&mut g, &Default::default(), "tempcloneform", None, &[XVal::Ref(cl1.0), XVal::Num(1.0)]).unwrap().unwrap() else {
        panic!("expected second-generation IDLE clone FormId");
    };
    let cl2 = FormId(cl2_raw);
    assert_eq!(g.idle_animation_path(cl2), Some("Owned\\PreIdle.kf"));
    fnv_game::nvse::call(&mut g, &Default::default(), "setidleanimpath", None, &[XVal::Ref(cl1.0), XVal::Str("Owned\\PostCl1.kf".into())]).unwrap().unwrap();
    assert_eq!(g.idle_animation_path(cl1), Some("Owned\\PostCl1.kf"));
    assert_eq!(g.idle_animation_path(cl2), Some("Owned\\PreIdle.kf"));
    assert_eq!(
        fnv_game::nvse::call(&mut g, &Default::default(), "getmodelpath", None, &[XVal::Ref(cl2.0)]).unwrap().unwrap(),
        XVal::Str("Owned\\PreIdle.kf".into()),
    );

    // Cloning an unedited IDLE stays on authored model even after the original is edited later.
    let mut g2 = game(&f);
    let XVal::Ref(cl_unedited_raw) = fnv_game::nvse::call(&mut g2, &Default::default(), "tempcloneform", None, &[XVal::Ref(orig.0), XVal::Num(1.0)]).unwrap().unwrap() else {
        panic!("expected unedited IDLE clone FormId");
    };
    let cl_unedited = FormId(cl_unedited_raw);
    fnv_game::nvse::call(&mut g2, &Default::default(), "setidleanimpath", None, &[XVal::Ref(orig.0), XVal::Str("Owned\\AfterUnedited.kf".into())]).unwrap().unwrap();
    assert_eq!(g2.idle_animation_path(orig), Some("Owned\\AfterUnedited.kf"));
    assert_eq!(g2.idle_animation_path(cl_unedited), Some("Characters\\_Male\\IdleAnims\\Sit.kf"));
    assert_eq!(
        fnv_game::nvse::call(&mut g2, &Default::default(), "getmodelpath", None, &[XVal::Ref(cl_unedited.0)]).unwrap().unwrap(),
        XVal::Str("Characters\\_Male\\IdleAnims\\Sit.kf".into()),
    );
}

#[test]
fn force_play_and_stop_idle_respect_player_pov_argument() {
    use fnv_game::idles::IdlePov;
    use fnv_script::vm::XVal;

    let f = fixture();
    let mut g = game(&f);
    let me = FormId::PLAYER_REF;
    let idle = FormId(IDLE_SIT);

    // ForcePlayIdle with omitted POV keeps None (vanilla auto-routing); 1 -> Third, 2 -> First.
    fnv_game::nvse::call(&mut g, &Default::default(), "forceplayidle", Some(me), &[XVal::Ref(idle.0)]).unwrap().unwrap();
    fnv_game::nvse::call(&mut g, &Default::default(), "forceplayidle", Some(me), &[XVal::Ref(idle.0), XVal::Num(1.0)]).unwrap().unwrap();
    fnv_game::nvse::call(&mut g, &Default::default(), "forceplayidle", Some(me), &[XVal::Ref(idle.0), XVal::Num(2.0)]).unwrap().unwrap();
    let povs: Vec<Option<IdlePov>> = std::mem::take(&mut g.idle_requests).into_iter().map(|r| r.pov).collect();
    assert_eq!(povs, [None, Some(IdlePov::Third), Some(IdlePov::First)]);

    // ForceStopIdle with explicit playerPOV 1/2 records POV-scoped stops without queuing a full stop.
    fnv_game::nvse::call(&mut g, &Default::default(), "forcestopidle", Some(me), &[XVal::Num(2.0)]).unwrap().unwrap();
    fnv_game::nvse::call(&mut g, &Default::default(), "forcestopidle", Some(me), &[XVal::Num(1.0)]).unwrap().unwrap();
    assert_eq!(g.gaps.idle_stop_pov, vec![(me, IdlePov::First), (me, IdlePov::Third)]);
    assert!(g.gaps.idle_stops.is_empty());

    // ForceStopIdle without playerPOV (or StopIdle) queues a normal full stop.
    fnv_game::nvse::call(&mut g, &Default::default(), "forcestopidle", Some(me), &[]).unwrap().unwrap();
    assert_eq!(g.gaps.idle_stops, vec![me]);
}
