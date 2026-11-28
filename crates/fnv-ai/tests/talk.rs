//! Dialogue packages carried out (`fnv_ai`, docs/design/actor-behaviour.md §5.6) on a
//! plugin and a navmesh built in memory (no game data): one room, a greeting, a trigger
//! box, and actors with one Dialogue package each. The rules are the GECK wiki's
//! (Dialogue Package, "Behavior"). The greeting opens a conversation, also with the
//! Goodbye flag (`fixture_with`): the package's force greet holds the player.

use fnv_ai::errand::Act;
use fnv_ai::{Activity, Ai, AiConfig, AiEvent, Talking};
use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig};
use fnv_game::package::flags;
use fnv_game::GameData;
use fnv_nav::record::{NavMeshRecord, Triangle};
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

fn u32b(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn record(sig: &[u8; 4], fid: u32, subs: &[Subrecord]) -> Vec<u8> {
    let payload = write_subrecords(subs);
    [&sig[..], &(payload.len() as u32).to_le_bytes(), &0u32.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
}

fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
    [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
}

fn tes4() -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0xA00u32.to_le_bytes()].concat();
    let payload = write_subrecords(&[sub(b"HEDR", hedr)]);
    [&b"TES4"[..], &(payload.len() as u32).to_le_bytes(), &1u32.to_le_bytes(), &[0; 4], &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
}

fn glob(fid: u32, edid: &str, v: f32) -> Vec<u8> {
    record(b"GLOB", fid, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", v.to_le_bytes().to_vec())])
}

/// A location or a target: (type, value, radius or distance).
type Triple = (i32, u32, i32);

fn triple(t: Triple) -> Vec<u8> {
    [t.0.to_le_bytes(), t.1.to_le_bytes(), t.2.to_le_bytes()].concat()
}

/// A Dialogue package: general flags, the wait location, the trigger location, the
/// target with the activate distance, the topic and whether it is a Say To.
#[allow(clippy::too_many_arguments)]
fn talk_pack(id: u32, edid: &str, general: u32, wait: Option<Triple>, trigger: Option<Triple>, target: Triple, topic: u32, say_to: bool) -> Vec<u8> {
    talk_pack_flags(id, edid, general, wait, trigger, target, topic, say_to, 0)
}

/// PKDD flag "Don't Control Target Movement" (schemas/fnv/ai.json).
const DONT_CONTROL_TARGET: u32 = 0x100;

/// As [`talk_pack`], with the PKDD flags.
#[allow(clippy::too_many_arguments)]
fn talk_pack_flags(id: u32, edid: &str, general: u32, wait: Option<Triple>, trigger: Option<Triple>, target: Triple, topic: u32, say_to: bool, pkdd: u32) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"PKDT", [&general.to_le_bytes()[..], &[15, 0, 0, 0], &[0; 4]].concat())];
    subs.extend(wait.map(|l| sub(b"PLDT", triple(l))));
    subs.extend(trigger.map(|l| sub(b"PLD2", triple(l))));
    subs.push(sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat()));
    subs.push(sub(b"PTDT", triple(target)));
    // FOV, topic, flags, unused, dialogue type, unknown.
    subs.push(sub(b"PKDD", [75f32.to_le_bytes(), topic.to_le_bytes(), pkdd.to_le_bytes(), [0; 4], (say_to as u32).to_le_bytes(), [0; 4]].concat()));
    record(b"PACK", id, &subs)
}

fn npc(fid: u32, edid: &str, packages: &[u32]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid))];
    subs.extend(packages.iter().map(|p| sub(b"PKID", u32b(*p))));
    record(b"NPC_", fid, &subs)
}

fn placed(sig: &[u8; 4], fid: u32, edid: &str, base: u32, pos: [f32; 3], extra: Option<Subrecord>) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"NAME", u32b(base))];
    subs.extend(extra);
    subs.push(sub(b"DATA", [pos[0].to_le_bytes(), pos[1].to_le_bytes(), pos[2].to_le_bytes(), [0; 4], [0; 4], [0; 4]].concat()));
    record(sig, fid, &subs)
}

/// A box primitive on a reference (XPRM): half extents, a colour, the type.
fn box_primitive(half: [f32; 3]) -> Subrecord {
    let mut d: Vec<u8> = half.iter().flat_map(|v| v.to_le_bytes()).collect();
    d.extend([0u8; 16]);
    d.extend(1u32.to_le_bytes());
    sub(b"XPRM", d)
}

/// An empty result script.
fn no_script() -> Subrecord {
    sub(b"SCHR", [&[0u8; 16][..], &0u16.to_le_bytes(), &1u16.to_le_bytes()].concat())
}

/// A topic with one line of one response (or none), said by anyone.
fn topic(id: u32, edid: &str, line: Option<(u32, u16, &str)>) -> Vec<u8> {
    topic_of_type(id, edid, line, 0)
}

/// As [`topic`], its line of INFO type `kind` (0 Topic, 1 Conversation).
fn topic_of_type(id: u32, edid: &str, line: Option<(u32, u16, &str)>, kind: u8) -> Vec<u8> {
    let dial = record(b"DIAL", id, &[sub(b"EDID", z(edid)), sub(b"FULL", z(edid)), sub(b"PNAM", 50f32.to_le_bytes().to_vec()), sub(b"DATA", vec![0, 0])]);
    let lines = line.map_or(Vec::new(), |(info, info_flags, text)| {
        let trdt = [u32b(0), u32b(50), vec![0; 4], vec![1, 0, 0, 0], u32b(0), vec![0; 4]].concat();
        record(
            b"INFO",
            info,
            &[
                sub(b"DATA", vec![kind, 0, info_flags as u8, (info_flags >> 8) as u8]),
                sub(b"QSTI", u32b(QUEST)),
                sub(b"TRDT", trdt),
                sub(b"NAM1", z(text)),
                sub(b"NAM2", z("")),
                sub(b"NAM3", z("")),
                no_script(),
                sub(b"NEXT", vec![]),
                no_script(),
            ],
        )
    });
    [dial, group(id.to_le_bytes(), 7, &lines)].concat()
}

const G_DAYS: u32 = 0x805;
const QUEST: u32 = 0x810;
const GREETING: u32 = 0x811;
const T_SAY: u32 = 0x812;
const T_EMPTY: u32 = 0x813;
const I_HELLO: u32 = 0x818;
const I_SAY: u32 = 0x819;
const T_CHATTER: u32 = 0x814;
const I_CHATTER: u32 = 0x81A;
const T_HELLO: u32 = 0x815;
const I_HI: u32 = 0x81B;
const T_TALK: u32 = 0x816;
const I_TALK: u32 = 0x81C;
const STAT: u32 = 0x820;
const ACTI: u32 = 0x821;
const STOOL: u32 = 0x822;
// Packages.
const P_GREET: u32 = 0x830;
const P_COME: u32 = 0x831;
const P_WAIT: u32 = 0x832;
const P_SAY: u32 = 0x833;
const P_TO_NPC: u32 = 0x834;
const P_MUTE: u32 = 0x835;
const P_WANDER: u32 = 0x836;
const P_SEATED: u32 = 0x837;
const P_STAND: u32 = 0x838;
const P_FIND_PLAYER: u32 = 0x839;
const P_HOLD: u32 = 0x83A;
// Bases.
const N_GREET: u32 = 0x840;
const N_COME: u32 = 0x841;
const N_WAIT: u32 = 0x842;
const N_SAY: u32 = 0x843;
const N_TO_NPC: u32 = 0x844;
const N_MUTE: u32 = 0x845;
const N_IDLE: u32 = 0x846;
const N_SEATED: u32 = 0x847;
const N_STAND: u32 = 0x848;
const N_FINDER: u32 = 0x849;
const N_HOLD: u32 = 0x84A;
// The cell and its references.
const CELL: u32 = 0x860;
const TRIGGER: u32 = 0x870;
const MARK_WAIT: u32 = 0x871;
const MARK_TRIGGER: u32 = 0x872;
const R_STOOL: u32 = 0x873;
const R_GREET: u32 = 0x880;
const R_COME: u32 = 0x881;
const R_WAIT: u32 = 0x882;
const R_SAY: u32 = 0x883;
const R_TO_NPC: u32 = 0x884;
const R_MUTE: u32 = 0x885;
const R_IDLE: u32 = 0x886;
const R_SEATED: u32 = 0x887;
const R_STAND: u32 = 0x888;
const R_FINDER: u32 = 0x889;
const R_HOLD: u32 = 0x88A;

const PLAYER: u32 = 0x14;
const GOODBYE: u16 = 0x0001;
const ROOM: Space = Space::Interior(CELL);
/// Where the greeter stands, and its trigger box: x 300 to 900, y 250 to 950.
const GREETER: [f32; 3] = [600.0, 900.0, 0.0];
const CORNER: [f32; 3] = [100.0, 100.0, 0.0];
/// The stool a seated talker waits on.
const STOOL_AT: [f32; 3] = [600.0, 1100.0, 0.0];

/// The plugin; `greeting` is the flags of the one GREETING line.
fn plugin_bytes(greeting: u16, hello: bool) -> Vec<u8> {
    let globs = [
        glob(0x801, "GameYear", 2281.0),
        glob(0x802, "GameMonth", 9.0),
        glob(0x803, "GameDay", 19.0),
        glob(0x804, "GameHour", 12.0),
        glob(G_DAYS, "GameDaysPassed", 5.0),
        glob(0x806, "TimeScale", 30.0),
    ]
    .concat();
    let player = (0, PLAYER, 0);
    let packs = [
        // Sunny Smiles's shape: wait at the editor location (radius 0), a trigger box,
        // the player within 256; here with Do Not Control Target Movement, so the
        // trigger location does not hold the greeting back (case 2 as documented).
        talk_pack_flags(P_GREET, "GreetInBox", flags::MUST_COMPLETE, Some((3, 0, 0)), Some((0, TRIGGER, 0)), (0, PLAYER, 256), GREETING, false, DONT_CONTROL_TARGET),
        // The same without that flag, as Sunny's and Paladin Ramos's packages are
        // (HVRamosEntryDialogPackage, issue #2453): the player is to be in the box.
        talk_pack(P_HOLD, "GreetInBoxHeld", flags::MUST_COMPLETE, Some((3, 0, 0)), Some((0, TRIGGER, 0)), (0, PLAYER, 256), GREETING, false),
        // No locations and no distance: go to the player at once, running.
        talk_pack(P_COME, "ComeAndTalk", flags::ALWAYS_RUN, None, None, player, 0, false),
        // A wait location with a radius and a trigger location with one.
        talk_pack(P_WAIT, "WaitThenTalk", 0, Some((0, MARK_WAIT, 100)), Some((0, MARK_TRIGGER, 200)), (0, PLAYER, 200), 0, false),
        talk_pack(P_SAY, "SayALine", 0, None, None, (0, PLAYER, 200), T_SAY, true),
        talk_pack(P_TO_NPC, "TalkToAnActor", 0, None, None, (0, R_GREET, 200), T_TALK, false),
        talk_pack(P_MUTE, "NothingToSay", flags::MUST_COMPLETE, None, None, (0, PLAYER, 200), T_EMPTY, false),
        // Benny's shape in the Tops' suite: wait at a stool (radius 128), no trigger
        // location, the player within 256.
        talk_pack(P_SEATED, "TalkFromTheStool", flags::MUST_COMPLETE, Some((0, R_STOOL, 128)), None, (0, PLAYER, 256), GREETING, false),
        // A Wander package about its editor location, no Fallout behaviour flags set (so all
        // of them allowed).
        record(b"PACK", P_WANDER, &[sub(b"EDID", z("WanderHere")), sub(b"PKDT", [&0u32.to_le_bytes()[..], &[5, 0, 0, 0], &[0; 4]].concat()), sub(b"PLDT", triple((3, 0, 0))), sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat())]),
        // A Find for the player near its editor location, radius 0: Cachino's
        // vGomorrahCachinoDialoguePackage (issue #2473, packages.md section 10).
        record(b"PACK", P_FIND_PLAYER, &[sub(b"EDID", z("FindThePlayer")), sub(b"PKDT", [&0u32.to_le_bytes()[..], &[0, 0, 0, 0], &[0; 4]].concat()), sub(b"PLDT", triple((3, 0, 0))), sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat()), sub(b"PTDT", triple(player))]),
        // A Travel package to its editor location: the actor stands where it was placed.
        record(b"PACK", P_STAND, &[sub(b"EDID", z("StandHere")), sub(b"PKDT", [&0u32.to_le_bytes()[..], &[6, 0, 0, 0], &[0; 4]].concat()), sub(b"PLDT", triple((3, 0, 0))), sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat())]),
    ]
    .concat();
    let npcs = [
        npc(0x7, "Player", &[]),
        npc(N_GREET, "Greeter", &[P_GREET]),
        npc(N_COME, "Comer", &[P_COME]),
        npc(N_WAIT, "Waiter", &[P_WAIT]),
        npc(N_SAY, "Sayer", &[P_SAY]),
        npc(N_TO_NPC, "Chatter", &[P_TO_NPC]),
        npc(N_MUTE, "Mute", &[P_MUTE]),
        npc(N_IDLE, "Idler", &[P_WANDER]),
        npc(N_SEATED, "Sitter", &[P_SEATED]),
        npc(N_STAND, "Stander", &[P_STAND]),
        npc(N_FINDER, "Finder", &[P_FIND_PLAYER]),
        npc(N_HOLD, "Holder", &[P_HOLD]),
    ]
    .concat();
    let quest = record(b"QUST", QUEST, &[sub(b"EDID", z("TalkQuest")), sub(b"DATA", [vec![1, 50, 0, 0], 0f32.to_le_bytes().to_vec()].concat())]);
    let dial = [topic(GREETING, "GREETING", Some((I_HELLO, greeting, "Hello there."))), topic(T_SAY, "SayTopic", Some((I_SAY, 0, "Mind the step."))), topic(T_EMPTY, "EmptyTopic", None), topic(T_CHATTER, "IdleChatter", Some((I_CHATTER, 0, "Hot one today."))), topic_of_type(T_TALK, "TalkToYou", Some((I_TALK, 0, "Got a minute?")), 1)].concat();
    let dial = if hello { [dial, topic(T_HELLO, "HELLO", Some((I_HI, 0, "Morning.")))].concat() } else { dial };
    let bases = [
        record(b"STAT", STAT, &[sub(b"EDID", z("Marker"))]),
        record(b"ACTI", ACTI, &[sub(b"EDID", z("TalkTrigger"))]),
        // Sit (bit 30), marker 0.
        record(b"FURN", STOOL, &[sub(b"EDID", z("Stool")), sub(b"MNAM", 0x4000_0001u32.to_le_bytes().to_vec())]),
    ];
    let refs = [
        placed(b"REFR", TRIGGER, "TriggerBox", ACTI, [600.0, 600.0, 100.0], Some(box_primitive([300.0, 350.0, 110.0]))),
        placed(b"REFR", MARK_WAIT, "WaitMarker", STAT, [200.0, 1000.0, 0.0], None),
        placed(b"REFR", MARK_TRIGGER, "TriggerMarker", STAT, [1000.0, 200.0, 0.0], None),
        placed(b"ACHR", R_GREET, "GreeterRef", N_GREET, GREETER, None),
        placed(b"ACHR", R_COME, "ComerRef", N_COME, CORNER, None),
        placed(b"ACHR", R_WAIT, "WaiterRef", N_WAIT, CORNER, None),
        placed(b"ACHR", R_SAY, "SayerRef", N_SAY, CORNER, None),
        placed(b"ACHR", R_TO_NPC, "ChatterRef", N_TO_NPC, CORNER, None),
        placed(b"ACHR", R_MUTE, "MuteRef", N_MUTE, CORNER, None),
        placed(b"ACHR", R_IDLE, "IdlerRef", N_IDLE, CORNER, None),
        placed(b"REFR", R_STOOL, "StoolRef", STOOL, STOOL_AT, None),
        placed(b"ACHR", R_SEATED, "SitterRef", N_SEATED, CORNER, None),
        placed(b"ACHR", R_STAND, "StanderRef", N_STAND, CORNER, None),
        placed(b"ACHR", R_FINDER, "FinderRef", N_FINDER, CORNER, None),
        placed(b"ACHR", R_HOLD, "HolderRef", N_HOLD, GREETER, None),
    ]
    .concat();
    let cell = [record(b"CELL", CELL, &[sub(b"EDID", z("Room"))]), group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs))].concat();
    let top = |sig: &[u8; 4], body: &[u8]| group(*sig, 0, body);
    [
        tes4(),
        top(b"GLOB", &globs),
        top(b"STAT", &bases[0]),
        top(b"ACTI", &bases[1]),
        top(b"FURN", &bases[2]),
        top(b"NPC_", &npcs),
        top(b"PACK", &packs),
        top(b"QUST", &quest),
        top(b"DIAL", &dial),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
    ]
    .concat()
}

/// The room: a flat navmesh of 12 × 12 squares of side 100 from the origin.
fn nav() -> NavGraph {
    let n = 12usize;
    let vid = |i: usize, j: usize| (j * (n + 1) + i) as u16;
    let mut vertices = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            vertices.push([i as f32 * 100.0, j as f32 * 100.0, 0.0]);
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
    let room = NavMeshRecord { form_id: 1, vertices, triangles, ..Default::default() };
    NavGraph::assemble(&[(&room, Some(CELL), Some(ROOM))])
}

// ---- fixture -----------------------------------------------------------------------

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

/// The greeting opens a conversation, as Sunny Smiles's does.
fn fixture() -> Fixture {
    fixture_with(0)
}

fn fixture_with(greeting: u16) -> Fixture {
    fixture_of(greeting, false)
}

/// The plugin with a HELLO topic too: one line anyone says to the player.
fn fixture_hello() -> Fixture {
    fixture_of(0, true)
}

fn fixture_of(greeting: u16, hello: bool) -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(greeting, hello), "Talk.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);
    assert!(data.issues.is_empty(), "{:?}", data.issues.to_json());
    Fixture { data, forms, refs }
}

struct Sim<'a> {
    game: Game<'a>,
    ai: Ai,
    nav: NavGraph,
}

/// A game with one actor handed to the AI and the player standing at `player`.
fn sim<'a>(f: &'a Fixture, actor: u32, player: [f32; 3]) -> Sim<'a> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut game = Game::new(&f.data, world, GameConfig { strict: true, frame_seconds: 0.25, package_interval: Some(10.0), ..GameConfig::default() });
    game.new_game();
    let mut ai = Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() });
    ai.add(&game, FormId(actor)).unwrap();
    let mut s = Sim { game, ai, nav: nav() };
    s.player_to(player);
    s
}

impl Sim<'_> {
    fn player_to(&mut self, pos: [f32; 3]) {
        self.game.set_actor_place(FormId(PLAYER), Some(FormId(CELL)), pos, 0.0).unwrap();
    }

    fn run(&mut self, seconds: f64) {
        for _ in 0..(seconds * 4.0) as usize {
            self.game.tick();
            self.ai.update(&mut self.game, &self.nav, 0.25);
        }
    }

    /// As [`Sim::run`], with the hellos and random conversations of each frame too
    /// (`Ai::social`, which turns bodies to whom they face), as the engine runs them.
    fn run_social(&mut self, seconds: f64) {
        for _ in 0..(seconds * 4.0) as usize {
            self.game.tick();
            self.ai.update(&mut self.game, &self.nav, 0.25);
            self.ai.social(&mut self.game, &self.nav, 0.25);
        }
    }

    fn events(&self) -> Vec<&AiEvent> {
        self.ai.events.iter().map(|e| &e.event).collect()
    }

    fn agent(&self, actor: u32) -> &fnv_ai::Agent {
        &self.ai.agents[&FormId(actor)]
    }

    fn talks(&self) -> usize {
        self.events().iter().filter(|e| matches!(e, AiEvent::Talk { .. })).count()
    }
}

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

// ---- tests -------------------------------------------------------------------------

#[test]
fn the_reference_table_reads_the_trigger_box() {
    let f = fixture();
    let prim = f.refs.get(FormId(TRIGGER)).unwrap().primitive.unwrap();
    assert!(!prim.sphere);
    assert_eq!(prim.bounds, [300.0, 350.0, 110.0]);
    assert!(f.refs.get(FormId(MARK_WAIT)).unwrap().primitive.is_none());
    let d = &f.data.package_defs[&FormId(P_GREET)];
    assert_eq!(d.target.unwrap().count_distance, 256);
    assert_eq!(d.dialogue.unwrap().topic, Some(FormId(GREETING)));
    assert!(f.data.package_defs[&FormId(P_SAY)].dialogue.unwrap().say_to);
}

/// Sunny Smiles's case: the player comes into the trigger box further away than the
/// activate distance; the actor walks up and starts the conversation at that distance.
#[test]
fn an_actor_walks_up_to_a_player_who_enters_its_trigger_box_and_greets() {
    let f = fixture();
    // Outside the box (it ends at y 250) and 700 units away: nothing happens.
    let mut s = sim(&f, R_GREET, [600.0, 200.0, 0.0]);
    s.run(5.0);
    assert_eq!(s.agent(R_GREET).talk, Some(Talking::Waiting));
    assert_eq!(s.agent(R_GREET).activity, Activity::There);
    assert_eq!(s.agent(R_GREET).walker.feet, GREETER);
    assert_eq!(s.events().len(), 1, "{:?}", s.events());
    assert_eq!(s.game.package_held(FormId(R_GREET)), Some("must complete"));
    // Into the box, 600 units away: it sets off.
    s.player_to([600.0, 300.0, 0.0]);
    s.run(0.25);
    assert!(matches!(s.events()[1], AiEvent::Approach { package, target, away: Some(a), running: false, .. } if package.0 == P_GREET && target.0 == PLAYER && (*a - 600.0).abs() < 1.0), "{:?}", s.events()[1]);
    assert_eq!(s.agent(R_GREET).activity, Activity::Travelling);
    assert!(s.game.dialogue.is_none());
    // It stops at the activate distance and the conversation starts with the greeting.
    s.run(5.0);
    let ev = s.events();
    assert!(matches!(ev[2], AiEvent::Talk { package, line, away, say_to: false, greeting: false, .. } if package.0 == P_GREET && line.0 == I_HELLO && *away <= 256.0 && *away > 220.0), "{:?}", ev[2]);
    let feet = s.agent(R_GREET).walker.feet;
    let walked = flat(feet, GREETER);
    assert!(walked > 340.0 && walked < 380.0, "{walked}");
    // The game knows where it stands.
    assert!(flat(s.game.world.peek(FormId(R_GREET)).unwrap().pos, feet) < 1.0);
    // The conversation ran its line and, with nothing for the player to choose, is
    // over: the package is done and does not start again.
    s.run(20.0);
    assert!(s.game.dialogue.is_none());
    assert!(matches!(s.events()[3], AiEvent::TalkOver { package } if package.0 == P_GREET), "{:?}", s.events()[3]);
    assert_eq!(s.agent(R_GREET).talk, Some(Talking::Done));
    assert!(s.game.extra[&FormId(R_GREET)].package_goal_met);
    assert_eq!(s.game.package_held(FormId(R_GREET)), None);
    assert!(s.game.extra[&FormId(R_GREET)].talked_to_pc);
    s.run(30.0);
    assert_eq!(s.talks(), 1);
    assert_eq!(s.events().len(), 4, "{:?}", s.events());
}

/// A pistol (WEAP animation type 3, Pistol - Ballistic [documented: xEdit
/// wbWeaponAnimTypeEnum]) for the drawn walk (#2124, `fnv_ai::carry::drawn_walk_share`).
const PISTOL: u32 = 0xA00;

fn with_pistol(mut f: Fixture) -> Fixture {
    use fnv_sim::forms::{ArmorClass, BaseData, BaseForm, Item, ItemKind};
    let it = Item {
        kind: ItemKind::Weapon,
        value: Some(1),
        money: None,
        weight: Some(1.0),
        slots: None,
        projectile: None,
        health: Some(100),
        non_playable: false,
        cant_drop: false,
        armor_class: ArmorClass::Light,
        power_armor: false,
        armor_dr: None,
        armor_dt: None,
        weapon_damage: Some(10.0),
        weight_mods: [0.0; 3],
        drawable: true,
        not_for_npcs: false,
        thrown: false,
        quest_item: false,
        icon: None,
        placed_count: None,
        teaches: None,
        cant_be_taken: false,
        modulates_voice: false,
        weapon_traits: None,
    };
    let id = FormId(PISTOL);
    f.forms.sigs.insert(id, *b"WEAP");
    f.forms.bases.insert(id, BaseForm { id, sig: *b"WEAP", edid: None, name: None, script: None, prompt: None, voice_filter: false, data: BaseData::Item(it) });
    f.data.weapon_anim_types.insert(id, 3);
    f
}

/// A Dialogue package's walk to the player takes the drawn walk while the actor's pistol
/// is out and gives it back when it is put away, the player standing (no new way).
#[test]
fn a_dialogue_approach_follows_the_pistol_drawn_and_put_away() {
    let f = with_pistol(fixture());
    let mut s = sim(&f, R_GREET, [600.0, 300.0, 0.0]);
    let me = FormId(R_GREET);
    s.game.world.add_item(me, FormId(PISTOL), 1).unwrap();
    s.game.world.outfit(me).unwrap();
    s.game.world.equip(me, FormId(PISTOL)).unwrap();
    s.game.hud.forced_out.insert(me);
    s.run(0.25);
    assert!(matches!(s.events()[1], AiEvent::Approach { running: false, .. }), "{:?}", s.events());
    let speed = |s: &Sim| s.agent(R_GREET).walker.speed;
    assert!((speed(&s) - 100.0 / 1.1).abs() < 1e-3, "{}", speed(&s));
    s.game.hud.forced_out.remove(&me);
    s.run(0.25);
    assert_eq!(s.agent(R_GREET).activity, Activity::Travelling);
    assert!((speed(&s) - 100.0).abs() < 1e-3, "{}", speed(&s));
    s.game.hud.forced_out.insert(me);
    s.run(0.25);
    assert!((speed(&s) - 100.0 / 1.1).abs() < 1e-3, "{}", speed(&s));
    assert_eq!(s.events().iter().filter(|e| matches!(e, AiEvent::Approach { .. })).count(), 1, "one way, not planned again");
}

/// ResetAI on an actor whose Dialogue package has talked and is chosen again (#2119):
/// the package starts over, so it greets once more and is then done again; ResetAI does
/// not make it greet over and over by itself [design: ResetAI takes the package up
/// afresh, docs/design/actor-behaviour.md §5; not settled by a capture of the original].
#[test]
fn reset_ai_after_a_force_greet_greets_once_more_then_is_done_again() {
    let f = fixture();
    let actor = FormId(R_GREET);
    let mut s = sim(&f, R_GREET, [750.0, 1050.0, 0.0]);
    s.run(25.0);
    assert_eq!((s.talks(), s.agent(R_GREET).talk), (1, Some(Talking::Done)), "{:?}", s.events());
    assert!(s.game.dialogue.is_none());
    // Done: the package stays and does not start over without ResetAI.
    s.run(30.0);
    assert_eq!(s.talks(), 1);
    let opcode = s.game.data.table.by_name("ResetAI").unwrap();
    let ctx = fnv_game::functions::Ctx { unit: "test".into(), opcode, on: Some(actor), self_ref: Some(actor), action_ref: None, quest: None, inst: None, in_condition: false };
    s.game.call_function(&ctx, &[]);
    s.run(25.0);
    assert_eq!(s.game.current_package(actor), Some(FormId(P_GREET)));
    assert_eq!(s.agent(R_GREET).restarts, 1);
    assert_eq!((s.talks(), s.agent(R_GREET).talk), (2, Some(Talking::Done)), "{:?}", s.events());
    s.run(60.0);
    assert_eq!(s.talks(), 2, "no greeting loop: {:?}", s.events());
}

/// The documented case 2: with a wait location of radius 0 the actor stays until the
/// target reaches the activate distance, wherever the trigger location is.
#[test]
fn a_player_within_the_activate_distance_is_greeted_on_the_spot() {
    let f = fixture();
    // Behind the actor and to its right, outside the box (it ends at y 950), 212 units
    // away.
    let mut s = sim(&f, R_GREET, [750.0, 1050.0, 0.0]);
    s.run(1.0);
    let ev = s.events();
    assert!(matches!(ev[1], AiEvent::Talk { line, away, .. } if line.0 == I_HELLO && (*away - 212.1).abs() < 1.0), "{ev:?}");
    assert_eq!(s.agent(R_GREET).walker.feet, GREETER);
    assert_eq!(s.game.dialogue.as_ref().map(|c| c.actor), Some(FormId(R_GREET)));
    assert_eq!(s.agent(R_GREET).talk, Some(Talking::Speaking));
    // While it speaks it turns to the player (north-east of it: heading 45 degrees)
    // and stands where it stood; the game has its heading.
    s.run(2.0);
    assert_eq!(s.agent(R_GREET).walker.feet, GREETER);
    let heading = s.agent(R_GREET).walker.heading;
    assert!((heading - std::f32::consts::FRAC_PI_4).abs() < 0.1, "{heading}");
    assert!((s.game.world.peek(FormId(R_GREET)).unwrap().rot[2] - heading).abs() < 1e-4);
}

/// Case 2 with "Do Not Control Target Movement" off: the player is walked into the
/// trigger location (`Game::player_lead`, which the engine carries out) and the
/// conversation opens there. Paladin Ramos's "Follow me" leaves the escort to the
/// trigger box's OnTrigger block, which runs only with the player inside it (#2453).
#[test]
fn a_controlled_target_is_walked_into_the_trigger_location_and_greeted_there() {
    let f = fixture();
    // Within the activate distance (212 units) but outside the box (it ends at y 950).
    let mut s = sim(&f, R_HOLD, [750.0, 1050.0, 0.0]);
    // (Within LEAD_STUCK_SECONDS: this test walks the player only once.)
    s.run(1.0);
    assert_eq!(s.talks(), 0, "{:?}", s.events());
    assert!(s.game.dialogue.is_none());
    assert_eq!(s.agent(R_HOLD).talk, Some(Talking::Waiting));
    // The lead: 48 units inside the box's north side, the player's x and height kept.
    let lead = s.game.player_lead.expect("the player is led");
    assert_eq!(lead.by, FormId(R_HOLD));
    assert!((lead.to[0] - 750.0).abs() < 0.01 && (lead.to[1] - 902.0).abs() < 0.01 && lead.to[2] == 0.0, "{:?}", lead.to);
    assert_eq!(s.events().iter().filter(|e| matches!(e, AiEvent::LeadsPlayer { .. })).count(), 1, "logged once: {:?}", s.events());
    // There (the engine's walk): the greeting comes and the lead is over.
    s.player_to(lead.to);
    s.run(1.0);
    assert_eq!(s.talks(), 1, "{:?}", s.events());
    assert_eq!(s.game.dialogue.as_ref().map(|c| c.actor), Some(FormId(R_HOLD)));
    assert!(s.game.player_lead.is_none());
}

/// #3464 (Harland in the REPCONN basement): after a conversation, a package taken up
/// afresh (ResetAI, as a trigger on the way out does) does not walk the player back into
/// the trigger location; the actor greets where the player stands. After
/// `LEAD_AGAIN_SECONDS` it walks the player in again.
#[test]
fn after_a_conversation_the_player_is_not_walked_back_into_the_trigger_location() {
    let f = fixture();
    let mut s = sim(&f, R_HOLD, [750.0, 1050.0, 0.0]);
    s.run(1.0);
    let lead = s.game.player_lead.expect("the player is led");
    s.player_to(lead.to);
    s.run(1.0);
    assert_eq!(s.talks(), 1, "{:?}", s.events());
    assert!(s.game.end_dialogue());
    s.run(0.5);
    assert!(s.events().iter().any(|e| matches!(e, AiEvent::TalkOver { .. })), "{:?}", s.events());
    // The player walks out of the box; a trigger there resets the actor's AI.
    s.player_to([750.0, 1050.0, 0.0]);
    s.game.run_source("HolderRef.ResetAI", Default::default()).unwrap();
    s.run(1.0);
    assert!(s.game.player_lead.is_none(), "not walked back: {:?}", s.events());
    assert_eq!(s.talks(), 2, "greets where the player stands: {:?}", s.events());
    assert!(s.game.end_dialogue());
    s.run(0.5);
    // Long after: walked in again.
    s.run(fnv_ai::carry::LEAD_AGAIN_SECONDS);
    s.game.run_source("HolderRef.ResetAI", Default::default()).unwrap();
    s.run(1.0);
    assert!(s.game.player_lead.is_some_and(|l| l.by == FormId(R_HOLD)), "{:?}", s.events());
}

/// The walk into the trigger location gets no nearer (something solid in the way, here a
/// player the engine never moves): after `LEAD_STUCK_SECONDS` the lead ends and the
/// actor greets where the player stands, so the keys are not set aside for good (review
/// of #2465). A walk that does get nearer keeps the lead.
#[test]
fn a_lead_that_gets_no_nearer_ends_and_the_actor_greets_where_the_player_stands() {
    let f = fixture();
    let mut s = sim(&f, R_HOLD, [750.0, 1050.0, 0.0]);
    // Getting nearer, 15 units every half second: still led after 2.5 s.
    for i in 1..=5 {
        s.run(0.5);
        s.player_to([750.0, 1050.0 - 15.0 * i as f32, 0.0]);
    }
    assert!(s.game.player_lead.is_some_and(|l| l.by == FormId(R_HOLD)), "{:?}", s.events());
    assert_eq!(s.talks(), 0, "{:?}", s.events());
    // Stuck at y 975, 73 units short of the point (y 902).
    s.run(fnv_ai::carry::LEAD_STUCK_SECONDS + 1.0);
    assert!(s.events().iter().any(|e| matches!(e, AiEvent::LeadStuck { left, .. } if (left - 73.0).abs() < 0.5)), "{:?}", s.events());
    assert_eq!(s.talks(), 1, "{:?}", s.events());
    assert_eq!(s.game.dialogue.as_ref().map(|c| c.actor), Some(FormId(R_HOLD)));
    assert!(s.game.player_lead.is_none());
}

/// A wall or a closed door between the actor and the player holds the force greet back
/// (`Game::greet_sight`, owner's playtest list 7 items 41/42): the actor waits, logged
/// once, and greets once the engine measures a clear line.
#[test]
fn a_force_greet_waits_for_a_clear_line_to_the_player() {
    use fnv_game::detection::Detection;
    let f = fixture();
    let mut s = sim(&f, R_GREET, [750.0, 1050.0, 0.0]);
    s.game.detection.measures_player = true;
    let blocked = Detection { clear: false, ..Detection::default() };
    s.game.detection.record(FormId(R_GREET), FormId(PLAYER), blocked, 0.0);
    s.run(2.0);
    let unseen = s.events().iter().filter(|e| matches!(e, AiEvent::TalkUnseen { .. })).count();
    assert_eq!((s.talks(), unseen), (0, 1), "{:?}", s.events());
    assert!(s.game.dialogue.is_none());
    // Not measured while the engine measures: still held back.
    s.game.detection.forget(FormId(R_GREET));
    s.run(1.0);
    assert_eq!(s.talks(), 0);
    let clear = Detection { clear: true, ..Detection::default() };
    s.game.detection.record(FormId(R_GREET), FormId(PLAYER), clear, s.game.time);
    s.run(1.0);
    assert_eq!(s.talks(), 1, "{:?}", s.events());
    assert_eq!(s.game.dialogue.as_ref().map(|c| c.actor), Some(FormId(R_GREET)));
}

/// A greeting with the Goodbye flag, started by the actor's Dialogue package, is a force
/// greet: it opens a conversation that holds the player until the line is over, and the
/// Goodbye flag ends it then (`Game::force_greet`; docs/spec-notes/dialogue.md §5). The
/// actor turns to the player meanwhile and its package is done when the conversation is.
#[test]
fn a_goodbye_greeting_of_a_dialogue_package_holds_the_player_until_it_is_said() {
    let f = fixture_with(GOODBYE);
    let mut s = sim(&f, R_GREET, [750.0, 1050.0, 0.0]);
    s.run(1.0);
    let ev = s.events();
    assert!(matches!(ev[1], AiEvent::Talk { package, line, say_to: false, greeting: false, .. } if package.0 == P_GREET && line.0 == I_HELLO), "{ev:?}");
    assert_eq!(ev.len(), 2, "{ev:?}");
    // A conversation: menu mode, the line being said in it, nothing said outside it.
    assert_eq!(s.game.dialogue.as_ref().map(|c| c.actor), Some(FormId(R_GREET)));
    assert!(s.game.in_menu());
    assert!(s.game.speech.is_empty());
    assert_eq!(s.agent(R_GREET).talk, Some(Talking::Speaking));
    assert!(!s.game.extra[&FormId(R_GREET)].package_goal_met);
    // It has turned to the player (heading 45 degrees) where it stood.
    assert_eq!(s.agent(R_GREET).walker.feet, GREETER);
    let heading = s.agent(R_GREET).walker.heading;
    assert!((heading - std::f32::consts::FRAC_PI_4).abs() < 0.1, "{heading}");
    // The line is over (12 characters: under two seconds): the Goodbye flag ends the
    // conversation, and the package is done, once.
    s.run(2.0);
    assert!(s.game.dialogue.is_none());
    assert!(matches!(s.events()[2], AiEvent::TalkOver { package } if package.0 == P_GREET), "{:?}", s.events()[2]);
    assert_eq!(s.agent(R_GREET).talk, Some(Talking::Done));
    assert!(s.game.extra[&FormId(R_GREET)].package_goal_met);
    assert!(s.game.extra[&FormId(R_GREET)].talked_to_pc);
    s.run(30.0);
    assert_eq!(s.talks(), 1);
    assert_eq!(s.events().len(), 3, "{:?}", s.events());
}

/// The documented case 3: no trigger location, so the actor goes to its target at once;
/// a target that moves is followed.
#[test]
fn without_a_trigger_location_the_actor_comes_to_the_player_at_once() {
    let f = fixture();
    let mut s = sim(&f, R_COME, [1000.0, 1000.0, 0.0]);
    s.run(0.25);
    assert!(matches!(s.events()[1], AiEvent::Approach { running: true, doors, .. } if doors.is_empty()), "{:?}", s.events()[1]);
    // The player walks off before the actor is there.
    s.run(1.0);
    assert!(s.game.dialogue.is_none());
    s.player_to([1000.0, 200.0, 0.0]);
    s.run(6.0);
    let ev = s.events();
    // The package gives no distance: the standard 150.
    assert!(matches!(ev[2], AiEvent::Talk { away, .. } if *away <= 150.0), "{ev:?}");
    assert!(flat(s.agent(R_COME).walker.feet, [1000.0, 200.0, 0.0]) <= 150.0);
    assert_eq!(s.talks(), 1);
}

/// The documented case 1: the actor goes to its wait location and stays there until the
/// target is in the trigger location, however near the target comes meanwhile.
#[test]
fn an_actor_waits_at_its_wait_location_until_the_trigger_location_is_entered() {
    let f = fixture();
    // The player stands by the wait marker, far from the trigger marker.
    let mut s = sim(&f, R_WAIT, [200.0, 900.0, 0.0]);
    s.run(0.25);
    assert_eq!(s.agent(R_WAIT).talk, Some(Talking::ToWait));
    assert!(matches!(s.events()[1], AiEvent::Trip { .. }), "{:?}", s.events()[1]);
    s.run(15.0);
    assert!(matches!(s.events()[2], AiEvent::Arrived { .. }), "{:?}", s.events()[2]);
    assert_eq!(s.agent(R_WAIT).talk, Some(Talking::Waiting));
    let waiting_at = s.agent(R_WAIT).walker.feet;
    assert!(flat(waiting_at, [200.0, 1000.0, 0.0]) <= 100.0);
    assert!(flat(waiting_at, [200.0, 900.0, 0.0]) < 200.0, "the player is within the activate distance");
    assert_eq!(s.talks(), 0);
    assert_eq!(s.events().len(), 3);
    // The player goes to within 200 of the trigger marker: the actor comes and talks.
    s.player_to([1000.0, 300.0, 0.0]);
    s.run(15.0);
    let ev = s.events();
    assert!(matches!(ev[3], AiEvent::Approach { .. }), "{ev:?}");
    assert!(matches!(ev[4], AiEvent::Talk { away, .. } if *away <= 200.0), "{ev:?}");
}

#[test]
fn a_say_to_package_says_its_line_without_a_conversation() {
    let f = fixture();
    let mut s = sim(&f, R_SAY, [300.0, 100.0, 0.0]);
    s.run(1.0);
    let ev = s.events();
    assert!(matches!(ev[1], AiEvent::Talk { line, say_to: true, .. } if line.0 == I_SAY), "{ev:?}");
    assert!(s.game.dialogue.is_none());
    assert!(s.game.is_talking(FormId(R_SAY)));
    assert_eq!(s.agent(R_SAY).talk, Some(Talking::Done));
    assert!(s.game.extra[&FormId(R_SAY)].package_goal_met);
    s.run(20.0);
    assert_eq!(s.talks(), 1);
}

/// A Say To line to the player waits for a clear line as a force greet does: Officer
/// Kendall's ambush bark in Tale of Two Wastelands (CG04Security02Ambush) came through
/// the walls of the player's room and its end script skipped the wake-up's stage 3.
#[test]
fn a_say_to_line_to_the_player_waits_for_a_clear_line() {
    use fnv_game::detection::Detection;
    let f = fixture();
    let mut s = sim(&f, R_SAY, [300.0, 100.0, 0.0]);
    s.game.detection.measures_player = true;
    let blocked = Detection { clear: false, ..Detection::default() };
    s.game.detection.record(FormId(R_SAY), FormId(PLAYER), blocked, 0.0);
    s.run(2.0);
    let unseen = s.events().iter().filter(|e| matches!(e, AiEvent::TalkUnseen { .. })).count();
    assert_eq!((s.talks(), unseen), (0, 1), "{:?}", s.events());
    assert!(!s.game.is_talking(FormId(R_SAY)));
    let clear = Detection { clear: true, ..Detection::default() };
    s.game.detection.record(FormId(R_SAY), FormId(PLAYER), clear, s.game.time);
    s.run(1.0);
    assert_eq!(s.talks(), 1, "{:?}", s.events());
    assert!(s.game.dialogue.is_none());
    assert!(s.game.is_talking(FormId(R_SAY)));
}

/// A conversation with another actor is carried out: the actor goes up to it and the
/// two talk in the world (`Game::start_actor_conversation`; Veronica's package at
/// Hidden Valley's intercom, issue #2451). No dialogue menu opens for the player.
#[test]
fn a_conversation_between_two_actors_is_carried_out() {
    let f = fixture();
    let mut s = sim(&f, R_TO_NPC, [300.0, 100.0, 0.0]);
    s.run(20.0);
    let ev = s.events();
    assert!(matches!(ev[1], AiEvent::Approach { target, .. } if target.0 == R_GREET), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Talk { target, line, say_to: false, .. } if target.0 == R_GREET && line.0 == I_TALK)), "{ev:?}");
    assert!(flat(s.agent(R_TO_NPC).walker.feet, GREETER) <= 200.0 + 1.0);
    assert!(s.game.dialogue.is_none());
}

/// An actor with no line to say does not try for ever: its package is done.
#[test]
fn an_actor_with_nothing_to_say_is_done_with_its_package() {
    let f = fixture();
    let mut s = sim(&f, R_MUTE, [200.0, 100.0, 0.0]);
    s.run(5.0);
    let ev = s.events();
    assert!(matches!(ev[1], AiEvent::NoTalk { package, .. } if package.0 == P_MUTE), "{ev:?}");
    assert_eq!(ev.len(), 2);
    assert!(s.game.dialogue.is_none());
    assert_eq!(s.game.package_held(FormId(R_MUTE)), None);
}

/// A player who is nowhere (a headless run without one) is waited for.
#[test]
fn an_actor_waits_for_a_player_who_is_nowhere() {
    let f = fixture();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut game = Game::new(&f.data, world, GameConfig { strict: true, frame_seconds: 0.25, package_interval: Some(10.0), ..GameConfig::default() });
    game.new_game();
    let mut ai = Ai::new(AiConfig::default());
    ai.add(&game, FormId(R_COME)).unwrap();
    let nav = nav();
    for _ in 0..40 {
        game.tick();
        ai.update(&mut game, &nav, 0.25);
    }
    assert_eq!(ai.agents[&FormId(R_COME)].talk, Some(Talking::Waiting));
    assert_eq!(ai.agents[&FormId(R_COME)].walker.feet, CORNER);
    assert_eq!(ai.events.len(), 1);
}

/// A conversation the player starts (activation, not the actor's package) holds the
/// game: the actor turns its body to the player on the spot while it lasts
/// (`Ai::update_held`), in the walker's heading, which the game is told and keeps after
/// the conversation (the owner's playtest of 2026-10-11: the actor turned back).
#[test]
fn an_actor_talked_to_turns_to_the_player_and_keeps_facing_the_player() {
    let f = fixture();
    // The player stands outside the greeter's trigger box: its package waits.
    let mut s = sim(&f, R_GREET, CORNER);
    s.run(0.5);
    assert_eq!(s.agent(R_GREET).talk, Some(Talking::Waiting));
    let before = s.agent(R_GREET).walker.heading;
    s.game.start_dialogue(FormId(R_GREET), None).unwrap();
    let want = fnv_nav::follow::heading_of([CORNER[0] - GREETER[0], CORNER[1] - GREETER[1]]);
    let off = |h: f32| (h - want + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    assert!(off(before).abs() > 1.0, "it starts facing away: {before} against {want}");
    // The game is held (nobody walks); the actor turns where it stands.
    for _ in 0..20 {
        s.ai.update_held(&mut s.game, 0.25);
    }
    let heading = s.agent(R_GREET).walker.heading;
    assert!(off(heading).abs() < 0.05, "{heading} against {want}");
    assert_eq!(s.agent(R_GREET).walker.feet, GREETER);
    assert!((s.game.world.peek(FormId(R_GREET)).unwrap().rot[2] - heading).abs() < 1e-4);
    // The conversation is over: nothing turns it back.
    assert!(s.game.end_dialogue());
    s.ai.update_held(&mut s.game, 0.25);
    assert_eq!(s.agent(R_GREET).walker.heading, heading);
}

/// A rooted actor (an Immobile creature such as Mr. House's screen, `Ai::root`) talked
/// to does not turn: its heading stays as placed for the whole conversation (issue
/// #1543).
#[test]
fn a_rooted_actor_talked_to_does_not_turn() {
    let f = fixture();
    let mut s = sim(&f, R_GREET, CORNER);
    s.run(0.5);
    s.ai.root(&s.game, FormId(R_GREET));
    let before = s.agent(R_GREET).walker.heading;
    let rot = s.game.world.peek(FormId(R_GREET)).map(|r| r.rot);
    s.game.start_dialogue(FormId(R_GREET), None).unwrap();
    for _ in 0..20 {
        s.ai.update_held(&mut s.game, 0.25);
    }
    assert_eq!(s.agent(R_GREET).walker.heading, before);
    assert_eq!(s.game.world.peek(FormId(R_GREET)).map(|r| r.rot), rot);
    assert_eq!(s.agent(R_GREET).walker.feet, GREETER);
}

/// A script's Look with `rotateBody` 1 turns the whole body of an actor the AI runs to
/// its target, at the walker's own turn rate, and keeps it there; StopLook ends it and
/// the body keeps its heading. Without the flag only the head turns (the engine's): the
/// body stays (docs/design/script-stubs.md §3, GECK wiki Look).
#[test]
fn a_look_turning_the_body_faces_the_target_until_stop_look() {
    use fnv_game::functions::senses::Look;
    let f = fixture();
    let mut s = sim(&f, R_GREET, CORNER);
    s.run(0.5);
    assert_eq!(s.agent(R_GREET).talk, Some(Talking::Waiting));
    let before = s.agent(R_GREET).walker.heading;
    let want = fnv_nav::follow::heading_of([CORNER[0] - GREETER[0], CORNER[1] - GREETER[1]]);
    let off = |h: f32| (h - want + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    assert!(off(before).abs() > 1.0, "it starts facing away: {before} against {want}");

    // The head only: the body stays.
    s.game.senses.looks.insert(FormId(R_GREET), Look { target: FormId(PLAYER), rotate_body: false });
    s.run_social(2.0);
    assert_eq!(s.agent(R_GREET).walker.heading, before);
    assert!(s.game.body_looks().is_empty());

    // The whole body: it turns, not at once (the walker's turn rate), then faces the player.
    s.game.senses.looks.insert(FormId(R_GREET), Look { target: FormId(PLAYER), rotate_body: true });
    assert_eq!(s.game.body_looks(), vec![(FormId(R_GREET), FormId(PLAYER))]);
    s.run_social(0.25);
    let first = s.agent(R_GREET).walker.heading;
    assert!(off(first).abs() < off(before).abs() && off(first).abs() > 0.05, "it turns over time: {first}");
    // At the observed rate, about 145 degrees a second.
    let turned = off(before).abs() - off(first).abs();
    assert!((turned - fnv_ai::social::LOOK_TURN_RATE * 0.25).abs() < 0.02, "turned {turned} in 0.25 s");
    s.run_social(5.0);
    let heading = s.agent(R_GREET).walker.heading;
    assert!(off(heading).abs() < 0.05, "{heading} against {want}");
    assert_eq!(s.agent(R_GREET).walker.feet, GREETER);
    assert!((s.game.world.peek(FormId(R_GREET)).unwrap().rot[2] - heading).abs() < 1e-4);

    // Logged once each: the turn starts, then it faces the player.
    let looks: Vec<&AiEvent> = s.events().into_iter().filter(|e| matches!(e, AiEvent::LookTurn { .. } | AiEvent::LookFaced { .. } | AiEvent::LookHeld { .. })).collect();
    assert!(matches!(looks.as_slice(), [AiEvent::LookTurn { target, .. }, AiEvent::LookFaced { .. }] if *target == FormId(PLAYER)), "{looks:?}");

    // StopLook: nothing turns it back.
    s.game.senses.looks.remove(&FormId(R_GREET));
    assert!(s.game.body_looks().is_empty());
    s.run_social(2.0);
    assert_eq!(s.agent(R_GREET).walker.heading, heading);
    assert!(s.events().iter().any(|e| matches!(e, AiEvent::LookOver { .. })));
}

/// A rooted actor that never turns (`Ai::fix_heading`: Mr. House's screen, Immobile with
/// a Turning Speed of 0) keeps its heading through a conversation, after it ends, and
/// when a script's Look asks for its whole body (issue #2150).
#[test]
fn a_fixed_actor_never_turns() {
    use fnv_game::functions::senses::Look;
    let f = fixture();
    let mut s = sim(&f, R_GREET, CORNER);
    s.run(0.5);
    s.ai.root(&s.game, FormId(R_GREET));
    s.ai.fix_heading(FormId(R_GREET));
    let before = s.agent(R_GREET).walker.heading;
    let want = fnv_nav::follow::heading_of([CORNER[0] - GREETER[0], CORNER[1] - GREETER[1]]);
    assert!(fnv_nav::follow::heading_delta(before, want).abs() > 1.0, "it starts facing away: {before} against {want}");

    s.game.start_dialogue(FormId(R_GREET), None).unwrap();
    for _ in 0..20 {
        s.ai.update_held(&mut s.game, 0.25);
    }
    assert!(s.game.end_dialogue());
    s.run_social(5.0);
    assert_eq!(s.agent(R_GREET).walker.heading, before);
    let placed = s.game.world.placed.get(FormId(R_GREET)).unwrap().rot;

    s.game.senses.looks.insert(FormId(R_GREET), Look { target: FormId(PLAYER), rotate_body: true });
    s.run_social(5.0);
    assert_eq!(s.agent(R_GREET).walker.heading, before);
    // The game's heading of it is the placed one still.
    assert!(s.game.world.peek(FormId(R_GREET)).is_none_or(|r| r.rot == placed));
    assert_eq!(s.agent(R_GREET).walker.feet, GREETER);
    let looks: Vec<&AiEvent> = s.events().into_iter().filter(|e| matches!(e, AiEvent::LookTurn { .. } | AiEvent::LookFaced { .. } | AiEvent::LookHeld { .. })).collect();
    assert!(matches!(looks.as_slice(), [AiEvent::LookHeld { .. }]), "{looks:?}");
}

// ---- out of the loaded area (fnv_ai::process) ---------------------------------------

/// [`sim`] with a slow walk (2 units/s) and the actor out of the cell buffer: a run there
/// would cross the room in one update, a walk does not.
fn far_sim<'a>(f: &'a Fixture, actor: u32, player: [f32; 3]) -> Sim<'a> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut game = Game::new(&f.data, world, GameConfig { strict: true, frame_seconds: 0.25, package_interval: Some(10.0), ..GameConfig::default() });
    game.new_game();
    let mut ai = Ai::new(AiConfig { walk_speed: 2.0, run_speed: 400.0, ..AiConfig::default() });
    ai.add(&game, FormId(actor)).unwrap();
    assert!(ai.set_level(&game, FormId(actor), fnv_ai::ProcessLevel::Low));
    let mut s = Sim { game, ai, nav: nav() };
    s.player_to(player);
    s
}

/// Runs until the actor's next far update, then returns.
fn next_far_update(s: &mut Sim) {
    let n = s.ai.process_stats.far_updates;
    let mut waited = 0;
    while s.ai.process_stats.far_updates == n {
        s.run(1.0);
        waited += 1;
        assert!(waited <= 121, "an update within {} s", fnv_ai::FAR_PROCESS_SECONDS);
    }
}

#[test]
fn far_away_a_dialogue_package_walks_its_actor_to_the_player_but_does_not_talk() {
    let f = fixture();
    let mut s = far_sim(&f, R_COME, [1100.0, 1100.0, 0.0]);
    // Malcolm Holmes's shape (ComeAndTalk: no locations, the player, Always Run). Out of
    // the high process it sets off, at a walk [observed 2026-10-24: about 10,000 units a
    // game hour though his package runs]. The first update takes up the package and
    // keeps the time owed for the next, which walks it.
    next_far_update(&mut s);
    assert_eq!(s.agent(R_COME).activity, Activity::Travelling);
    assert!(matches!(s.agent(R_COME).talk, Some(Talking::Approaching { .. })));
    next_far_update(&mut s);
    let walked = flat(s.agent(R_COME).walker.feet, CORNER);
    assert!(walked > 0.0 && walked <= 2.0 * 2.0 * fnv_ai::FAR_PROCESS_SECONDS + 1.0, "walked {walked}");
    // It keeps going in later updates, and does not talk out there.
    for _ in 0..3 {
        next_far_update(&mut s);
    }
    assert!(flat(s.agent(R_COME).walker.feet, CORNER) > walked);
    assert_eq!(s.talks(), 0);
    assert!(s.game.dialogue.is_none());
    // In the loaded area it runs on and talks.
    assert!(s.ai.set_level(&s.game, FormId(R_COME), fnv_ai::ProcessLevel::High));
    s.run(10.0);
    assert_eq!(s.talks(), 1);
    assert!(flat(s.agent(R_COME).walker.feet, [1100.0, 1100.0, 0.0]) <= 150.0);
}

#[test]
fn far_away_an_approach_keeps_its_route_when_the_player_moves() {
    let f = fixture();
    let mut s = far_sim(&f, R_COME, [1100.0, 1100.0, 0.0]);
    next_far_update(&mut s);
    next_far_update(&mut s);
    let Some(Talking::Approaching { to, .. }) = s.agent(R_COME).talk else { panic!("{:?}", s.agent(R_COME).talk) };
    // The player goes to the other corner: the walk goes on to where they were
    // [observed 2026-10-24: Holmes's jumps after the player moved landed where they had
    // without the move].
    s.player_to([1100.0, 100.0, 0.0]);
    next_far_update(&mut s);
    assert!(matches!(s.agent(R_COME).talk, Some(Talking::Approaching { to: now, .. }) if now == to), "no new plan");
    let follower = s.agent(R_COME).walker.follower.as_ref().expect("walking");
    let end = *follower.path.legs.last().unwrap().points.last().unwrap();
    assert!(flat(end, [1100.0, 1100.0, 0.0]) < 50.0, "still bound for {end:?}");
}

#[test]
fn far_away_a_dialogue_package_with_a_wait_location_keeps_its_actor_there() {
    let f = fixture();
    // Sunny Smiles's (and Ringo's) shape: wait at the editor location. The player is in
    // the trigger box, which in the loaded area would bring the actor over.
    let mut s = far_sim(&f, R_GREET, [500.0, 500.0, 0.0]);
    next_far_update(&mut s);
    next_far_update(&mut s);
    assert!(flat(s.agent(R_GREET).walker.feet, GREETER) < 1.0, "stays at {:?}", s.agent(R_GREET).walker.feet);
    assert_eq!(s.talks(), 0);
}

/// A wait while an actor is on its way to talk (issue #1597, Malcolm Holmes): the walk led
/// to the player's own spot and ran to its end, so the actor stood inside the player. It
/// now stops at its talking distance, says nothing while time passes, and talks once the
/// game goes on.
#[test]
fn a_wait_during_an_approach_leaves_the_actor_at_talking_distance() {
    let f = fixture();
    let player = [1100.0, 1100.0, 0.0];
    let mut s = sim(&f, R_COME, player);
    s.run(0.5);
    assert!(matches!(s.agent(R_COME).talk, Some(Talking::Approaching { .. })), "{:?}", s.agent(R_COME).talk);
    s.ai.pass_time(&mut s.game, &s.nav, 60.0, 0.5);
    let away = flat(s.agent(R_COME).walker.feet, player);
    assert!(away > 50.0 && away <= fnv_ai::goal::STANDARD_TALK_DISTANCE + 1.0, "{away} units from the player");
    assert_eq!(s.talks(), 0, "nobody talks in a wait");
    s.run(1.0);
    assert_eq!(s.talks(), 1, "{:?}", s.events());
    assert!(flat(s.agent(R_COME).walker.feet, player) > 50.0, "not inside the player");
}

/// A script's StartConversation from afar (issue #1544, `Game::walk_up_greet`): with the AI
/// running, the speaker runs to the player and opens the conversation there, not from
/// where it stood (the Legion explorers about Nelson greeted from 10,000 units off).
#[test]
fn start_conversation_from_afar_walks_up_to_the_player_first() {
    let f = fixture();
    let mut s = sim(&f, R_IDLE, [1100.0, 1100.0, 0.0]);
    assert!(s.game.cfg.package_interval.is_some(), "the AI runs packages");
    assert!(s.game.walk_up_greet(FormId(R_IDLE), None), "the speaker is far: it walks up");
    assert!(s.game.dialogue.is_none(), "no conversation from afar");
    s.run(1.0);
    assert!(s.events().iter().any(|e| matches!(e, AiEvent::WalkUp { .. })), "{:?}", s.events());
    assert!(s.game.dialogue.is_none());
    s.run(15.0);
    let walked = s.events().into_iter().find_map(|e| match e { AiEvent::WalkedUp { away, reached, line } => Some((*away, *reached, *line)), _ => None });
    assert!(matches!(walked, Some((away, true, Some(_))) if away <= 150.0), "{:?}", s.events());
    // Its greeting (the fixture's has no choices, so the conversation closes after it).
    assert!(s.game.walk_up_greets.is_empty());
    s.run(15.0);
    assert_eq!(s.game.events.iter().filter(|e| matches!(e.kind, fnv_game::trace::EventKind::DialogueStarted { .. })).count(), 1);
}

/// [design, #2724] Repeating StartConversation while walking must consume the pending
/// greeting when the nearer call opens it (`Game::open_dialogue`), before AI resumes.
#[test]
fn start_conversation_repeated_during_a_walk_up_opens_exactly_once() {
    use fnv_game::functions::Ctx;
    use fnv_game::trace::EventKind;
    use fnv_script::vm::{ArgValue, Value};

    let f = fixture();
    let player = [1100.0, 1100.0, 0.0];
    let actor = FormId(R_IDLE);
    let mut s = sim(&f, R_IDLE, player);
    let opcode = s.game.data.table.by_name("StartConversation").unwrap();
    let ctx = Ctx { unit: "test".into(), opcode, on: Some(actor), self_ref: Some(actor), action_ref: None, quest: None, inst: None, in_condition: false };
    let args = [ArgValue::Value(Value::Ref(FormId::PLAYER_REF.0))];
    assert!(s.game.walk_up_far(actor));
    let mut pending = false;
    for _ in 0..80 {
        s.game.call_function(&ctx, &args);
        if s.game.dialogue.is_some() {
            let away = flat(s.agent(R_IDLE).walker.feet, player);
            assert!(pending, "a previous frame recorded the walk-up");
            assert!(away > fnv_ai::goal::STANDARD_TALK_DISTANCE && away <= fnv_game::dialogue::WALK_UP_REACH, "opened at {away}");
            assert!(s.game.walk_up_greets.is_empty(), "the opener consumes the walk-up");
            break;
        }
        pending |= s.game.walk_up_pending(actor).is_some();
        s.run(0.25);
    }
    assert_eq!(s.game.dialogue_actor(), Some(actor), "repeated calls opened the talk");
    assert!(s.events().iter().any(|e| matches!(e, AiEvent::WalkUp { .. })));
    s.run(30.0);
    assert!(s.game.dialogue.is_none(), "the fixture's greeting ends");
    assert!(s.game.walk_up_greets.is_empty());
    assert!(!s.events().iter().any(|e| matches!(e, AiEvent::WalkedUp { .. })), "no stale arrival after dialogue ends");
    assert_eq!(s.game.events.iter().filter(|e| matches!(e.kind, EventKind::DialogueStarted { .. })).count(), 1);
}

/// [design, #2724] Both public openers consume only their actor's walk after success;
/// a topic with no line leaves it pending (`Game::open_dialogue`).
#[test]
fn any_successful_player_conversation_opener_consumes_its_walk_up() {
    let f = fixture();
    for forced in [false, true] {
        let mut s = sim(&f, R_IDLE, [1100.0, 1100.0, 0.0]);
        let actor = FormId(R_IDLE);
        assert!(s.game.walk_up_greet(actor, None));
        assert!(s.game.walk_up_greet(FormId(R_WAIT), None));
        let open = |g: &mut Game, topic| if forced { g.force_greet(actor, topic) } else { g.start_dialogue(actor, topic) };
        assert!(open(&mut s.game, Some(FormId(0))).is_err());
        assert_eq!(s.game.walk_up_pending(actor), Some(None));
        open(&mut s.game, None).unwrap();
        assert_eq!(s.game.walk_up_pending(actor), None);
        assert_eq!(s.game.walk_up_pending(FormId(R_WAIT)), Some(None), "another actor's walk remains");
    }
}

/// A StartConversation speaker the AI does not walk (not handed to it: its cell has not
/// loaded) never talks from afar (issue #1544): its greeting waits until it is near the
/// player, here the player coming up to it.
#[test]
fn start_conversation_by_a_speaker_the_ai_does_not_walk_waits_until_near() {
    let f = fixture();
    let mut s = sim(&f, R_IDLE, [1100.0, 1100.0, 0.0]);
    assert!(!s.ai.agents.contains_key(&FormId(R_WAIT)));
    assert!(s.game.walk_up_greet(FormId(R_WAIT), None), "the speaker is far");
    s.run(5.0);
    assert!(s.game.dialogue.is_none(), "no conversation from afar");
    assert_eq!(s.game.walk_up_pending(FormId(R_WAIT)), Some(None), "its greeting waits");
    assert!(!s.game.extra.get(&FormId(R_WAIT)).is_some_and(|e| e.talked_to_pc));
    s.player_to([CORNER[0] + 100.0, CORNER[1], 0.0]);
    s.run(0.25);
    assert!(s.game.walk_up_greets.is_empty(), "near: it has talked");
    assert!(s.game.extra[&FormId(R_WAIT)].talked_to_pc);
}

/// Near the player, or without the AI, StartConversation opens the conversation at once.
#[test]
fn start_conversation_near_the_player_talks_at_once() {
    let f = fixture();
    let mut s = sim(&f, R_IDLE, [CORNER[0] + 100.0, CORNER[1], 0.0]);
    assert!(!s.game.walk_up_greet(FormId(R_IDLE), None), "near: no walk");
    s.game.cfg.package_interval = None;
    s.player_to([1100.0, 1100.0, 0.0]);
    assert!(!s.game.walk_up_greet(FormId(R_IDLE), None), "without the AI: no walk");
    assert!(s.game.walk_up_greets.is_empty());
}

/// Idle chatter (fnv-ai social.rs, packages.md §7.7): an actor whose package allows it
/// says its IdleChatter line next to the player once the area's timer (15 to 60 s by
/// default) runs out; far from the player it says nothing.
#[test]
fn an_actor_next_to_the_player_chatters_now_and_then() {
    let f = fixture();
    let mut s = sim(&f, R_IDLE, [CORNER[0] + 1000.0, CORNER[1] + 1000.0, 0.0]);
    s.run_social(70.0);
    assert!(!s.events().iter().any(|e| matches!(e, AiEvent::IdleChatter { .. })), "far from the player: {:?}", s.events());
    s.player_to([CORNER[0] + 100.0, CORNER[1], 0.0]);
    s.run_social(70.0);
    let said: Vec<&AiEvent> = s.events().into_iter().filter(|e| matches!(e, AiEvent::IdleChatter { .. })).collect();
    assert!(matches!(said.first(), Some(AiEvent::IdleChatter { line, away }) if line.0 == I_CHATTER && *away <= fnv_ai::social::HELLO_DISTANCE), "{:?}", s.events());
}

/// Hellos (fnv-ai social.rs; #2156, owner's playtest 2026-11-11): said from
/// `HELLO_DISTANCE` (200, a third closer than before), and only with nothing solid
/// between the actor and the player.
#[test]
fn a_hello_needs_the_player_close_and_in_the_open() {
    use fnv_ai::social::HELLO_DISTANCE;
    use fnv_game::detection::Detection;
    let f = fixture_hello();
    let hellos = |s: &Sim| s.events().iter().filter(|e| matches!(e, AiEvent::Hello { .. })).count();
    // 250 units away, where it was greeted before: nothing.
    let mut s = sim(&f, R_STAND, [CORNER[0] + 250.0, CORNER[1], 0.0]);
    s.run_social(5.0);
    assert_eq!(hellos(&s), 0, "{:?}", s.events());
    // Close, but the engine measured a wall between them: nothing, however long.
    s.game.detection.measures_player = true;
    let time = s.game.time;
    s.game.detection.record(FormId(R_STAND), FormId(PLAYER), Detection { clear: false, ..Detection::default() }, time);
    s.player_to([CORNER[0] + 150.0, CORNER[1], 0.0]);
    s.run_social(5.0);
    assert_eq!(hellos(&s), 0, "through a wall: {:?}", s.events());
    // The player comes into the open: the hello.
    s.game.detection.record(FormId(R_STAND), FormId(PLAYER), Detection { clear: true, ..Detection::default() }, time);
    s.run_social(5.0);
    let said: Vec<&AiEvent> = s.events().into_iter().filter(|e| matches!(e, AiEvent::Hello { .. })).collect();
    assert!(matches!(said[..], [AiEvent::Hello { line, away }] if line.0 == I_HI && *away <= HELLO_DISTANCE), "{:?}", s.events());
}

/// Idle lines keep apart (#2156): an actor that said hello says no other idle line for
/// `IDLE_LINE_AGAIN` seconds, though the player goes away and comes back and the
/// chatter timer (15 to 60 s) runs out; a hello line said of late is not said again
/// (`IDLE_LINE_MEMORY`), so the next hello comes only after that.
#[test]
fn idle_lines_wait_for_each_other_and_do_not_repeat() {
    use fnv_ai::social::{HELLO_DISTANCE, HELLO_REARM, IDLE_LINE_AGAIN};
    use fnv_game::dialogue::IDLE_LINE_MEMORY;
    let f = fixture_hello();
    let near = [CORNER[0] + 150.0, CORNER[1], 0.0];
    let far = [CORNER[0] + HELLO_DISTANCE * HELLO_REARM + 100.0, CORNER[1], 0.0];
    let count = |s: &Sim| {
        let e = s.events();
        (e.iter().filter(|e| matches!(e, AiEvent::Hello { .. })).count(), e.iter().filter(|e| matches!(e, AiEvent::IdleChatter { .. })).count())
    };
    let mut s = sim(&f, R_STAND, near);
    s.run_social(2.0);
    assert_eq!(count(&s), (1, 0), "{:?}", s.events());
    // Away and back, over and over, for the rest of the actor's wait: no idle line.
    let mut spent = 2.0;
    while spent + 10.0 < IDLE_LINE_AGAIN {
        s.player_to(far);
        s.run_social(1.0);
        s.player_to(near);
        s.run_social(9.0);
        spent += 10.0;
    }
    assert_eq!(count(&s), (1, 0), "within the actor's wait: {:?}", s.events());
    // Its wait over: it chatters when the area's timer is due, but its one hello line
    // is still remembered.
    while spent + 10.0 < IDLE_LINE_MEMORY {
        s.player_to(far);
        s.run_social(1.0);
        s.player_to(near);
        s.run_social(9.0);
        spent += 10.0;
    }
    let (hellos, chatter) = count(&s);
    assert_eq!(hellos, 1, "the hello line is not repeated: {:?}", s.events());
    assert_eq!(chatter, 1, "its one chatter line is said once and not repeated either: {:?}", s.events());
    // The memory over: the next approach is greeted again.
    s.player_to(far);
    s.run_social(IDLE_LINE_AGAIN + 20.0);
    s.player_to(near);
    s.run_social(2.0);
    assert_eq!(count(&s).0, 2, "{:?}", s.events());
}

/// Observe Combat's spectators "run away if they get too close" (fnv-ai social.rs,
/// packages.md §7.6b): an actor whose package allows the behaviour, within
/// `SPECTATOR_FLEE_DISTANCE` of a fighter (here the player, hunted), runs from it
/// without entering a combat state, stays at its spot, and goes back to its package once
/// the fight is over. A fight farther off sends nobody running, and so does a foe in
/// another interior at the same coordinates (#4891).
#[test]
fn a_spectator_runs_from_a_fight_that_comes_too_close() {
    use fnv_ai::social::{OBSERVE_DISTANCE, SPECTATOR_FLEE_DISTANCE};
    use fnv_game::awareness::Timers;
    use fnv_game::detection::{Band, Detection};
    use fnv_game::combat_group::GroupSettings;
    let f = fixture();
    let mut s = sim(&f, R_IDLE, [CORNER[0] + 1000.0, CORNER[1] + 1000.0, 0.0]);
    // The straight-away flee distance shortened to fit the room (1,200 units square).
    s.game.awareness.tactics.settings = Some(GroupSettings { flee_normal: 700.0, ..GroupSettings::default() });
    s.run_social(1.0);
    // Someone (no resident) fights the player.
    let foe = FormId(R_GREET);
    let (now, t) = (s.game.time, Timers::default());
    let dist = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let a = s.game.awareness.actors.entry(foe).or_default();
    a.observe(FormId::PLAYER_REF, &Detection { value: 5.0, band: Band::Detected, ..Default::default() }, [0.0; 3], now, &t);
    a.settle(foe, now, &t, true);
    assert!(s.game.awareness.hunted(FormId::PLAYER_REF));
    // #4891: it stands at the room's corner's coordinates in another interior (the NCR
    // Correctional Facility's cells overlap in x and y): no fight here, even close.
    let close = [10.0, 10.0, 0.0];
    s.game.set_actor_place(foe, Some(FormId(CELL + 1)), [0.0; 3], 0.0).unwrap();
    s.player_to(close);
    s.run_social(2.0);
    assert!(!s.events().iter().any(|e| matches!(e, AiEvent::SpectatorFlee { .. } | AiEvent::ObserveCombat { .. })), "{:?}", s.events());
    // It stands in the room's corner (a foe with no place is near no one, #3223).
    s.game.set_actor_place(foe, Some(FormId(CELL)), [0.0; 3], 0.0).unwrap();
    // Far off: it watches.
    let feet = s.agent(R_IDLE).walker.feet;
    let far = [feet[0], (feet[1] + SPECTATOR_FLEE_DISTANCE + 200.0).min(1150.0), 0.0];
    assert!(dist(far, feet) > SPECTATOR_FLEE_DISTANCE && dist(far, feet) < OBSERVE_DISTANCE);
    s.player_to(far);
    s.run_social(2.0);
    assert!(!s.events().iter().any(|e| matches!(e, AiEvent::SpectatorFlee { .. })), "{:?}", s.events());
    // Too close: it runs, out of a combat state.
    // Between it and the room's corner, so there is room to run (it idles near the corner).
    assert!(dist(close, s.agent(R_IDLE).walker.feet) <= SPECTATOR_FLEE_DISTANCE);
    s.player_to(close);
    s.run_social(0.5);
    let ev = s.events();
    assert!(ev.iter().any(|e| matches!(e, AiEvent::SpectatorFlee { fighter, away } if *fighter == FormId::PLAYER_REF && *away <= SPECTATOR_FLEE_DISTANCE)), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Flee { from, .. } if *from == FormId::PLAYER_REF)), "{ev:?}");
    let before = dist(s.agent(R_IDLE).walker.feet, close);
    s.run_social(6.0);
    assert!(!s.game.awareness.state(FormId(R_IDLE)).in_combat(), "it does not join in");
    assert!(s.events().iter().any(|e| matches!(e, AiEvent::FleeArrived { .. })), "{:?}", s.events());
    let fled = s.agent(R_IDLE).walker.feet;
    assert!(dist(fled, close) > before + 200.0, "{fled:?} from {close:?}");
    // The fight over: back to its package.
    s.game.awareness.actors.remove(&foe);
    s.run_social(1.0);
    assert!(s.events().iter().any(|e| matches!(e, AiEvent::SearchOver { why } if why.contains("fight"))), "{:?}", s.events());
    assert!(s.agent(R_IDLE).search.is_none());
}

/// Who does not run from a fight (review of #1216): nobody while its foe only searches
/// (Alert), not the player's teammate, and a fight between residents sends a spectator
/// running from the resident fighting.
#[test]
fn spectators_run_only_from_a_real_fight_and_not_the_players_side() {
    use fnv_game::awareness::{State, Timers};
    use fnv_game::detection::{Band, Detection};
    use fnv_game::combat_group::GroupSettings;
    let f = fixture();
    let close = [10.0, 10.0, 0.0];
    let fights = |s: &Sim| s.events().iter().any(|e| matches!(e, AiEvent::SpectatorFlee { .. }));
    let hunt = |s: &mut Sim, state: State| {
        let foe = FormId(0xF00);
        let (now, t) = (s.game.time, Timers::default());
        let a = s.game.awareness.actors.entry(foe).or_default();
        a.observe(FormId::PLAYER_REF, &Detection { value: 5.0, band: Band::Detected, ..Default::default() }, [0.0; 3], now, &t);
        a.settle(foe, now, &t, true);
        a.state = state;
    };
    // A foe that only searches for the player (Alert): no fight to run from.
    let mut s = sim(&f, R_IDLE, [CORNER[0] + 1000.0, CORNER[1] + 1000.0, 0.0]);
    s.game.awareness.tactics.settings = Some(GroupSettings { flee_normal: 700.0, ..GroupSettings::default() });
    s.run_social(1.0);
    hunt(&mut s, State::Alert);
    s.player_to(close);
    s.run_social(1.0);
    assert!(!fights(&s), "an Alert hunt is no fight: {:?}", s.events());
    // In Combat, the player's teammate stays (it joins the fight, it does not watch it).
    hunt(&mut s, State::Combat);
    s.game.extra.entry(FormId(R_IDLE)).or_default().teammate = true;
    s.run_social(1.0);
    assert!(!fights(&s), "a teammate does not run: {:?}", s.events());
    // A resident fighting (no player in it) next to a spectator: it runs from that one.
    let mut s = sim(&f, R_IDLE, [CORNER[0] + 1000.0, CORNER[1] + 1000.0, 0.0]);
    s.game.awareness.tactics.settings = Some(GroupSettings { flee_normal: 700.0, ..GroupSettings::default() });
    s.run_social(1.0);
    let feet = s.agent(R_IDLE).walker.feet;
    s.game.set_actor_place(FormId(R_GREET), Some(FormId(CELL)), [feet[0] + 150.0, feet[1], 0.0], 0.0).unwrap();
    s.ai.add(&s.game, FormId(R_GREET)).unwrap();
    s.game.awareness.actors.entry(FormId(R_GREET)).or_default().state = State::Combat;
    s.run_social(0.5);
    let ev = s.events();
    assert!(ev.iter().any(|e| matches!(e, AiEvent::SpectatorFlee { fighter, .. } if *fighter == FormId(R_GREET))), "{ev:?}");
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::SpectatorFlee { fighter, .. } if *fighter == FormId::PLAYER_REF)), "the player fights nobody: {ev:?}");
}

/// A wait location that is furniture: the actor sits on it and stays seated. Without a
/// trigger location (case 3) it does not go up to the player, but greets the player
/// from its seat once within the activate distance, and its body does not turn
/// [observed: Benny on his bar stool in the Tops' presidential suite,
/// VTopsBennyPresidentialTalk, owner's frame 2026-11-03].
#[test]
fn an_actor_waiting_on_furniture_greets_from_its_seat() {
    let f = fixture();
    // 800 units from the stool.
    let mut s = sim(&f, R_SEATED, [600.0, 300.0, 0.0]);
    s.run(15.0);
    let a = s.agent(R_SEATED);
    assert_eq!(a.talk, Some(Talking::Waiting), "{:?}", s.events());
    assert_eq!(a.using, Some(FormId(R_STOOL)), "{:?}", s.events());
    assert!(flat(a.walker.feet, STOOL_AT) <= 128.0, "{:?}", a.walker.feet);
    assert!(!s.events().iter().any(|e| matches!(e, AiEvent::Approach { .. })), "{:?}", s.events());
    // Still seated a while later, the player as far.
    s.run(10.0);
    assert_eq!(s.agent(R_SEATED).using, Some(FormId(R_STOOL)));
    assert_eq!(s.talks(), 0);
    let (feet, heading) = (s.agent(R_SEATED).walker.feet, s.agent(R_SEATED).walker.heading);
    // The player walks up to 200 units: the greeting, from the seat.
    s.player_to([600.0, 900.0, 0.0]);
    s.run(2.0);
    let ev = s.events();
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Talk { package, line, say_to: false, .. } if package.0 == P_SEATED && line.0 == I_HELLO)), "{ev:?}");
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::Approach { .. })), "{ev:?}");
    let a = s.agent(R_SEATED);
    assert_eq!(a.using, Some(FormId(R_STOOL)));
    assert_eq!(a.walker.feet, feet);
    assert_eq!(a.walker.heading, heading);
}

/// A restrained actor (SetRestrained 1) given a Dialogue package does not move but still
/// talks [documented: GECK wiki SetRestrained, restrained actors "will not move from
/// their current position, or enter combat, but will continue to \"think\" ... and go
/// into dialogue"]: it waits where it stands, greets the player once within the activate
/// distance, and released during that conversation (the line that frees it, as Hardin's
/// power armor training does) it does not greet again (#2309). The player is restrained
/// too, as in that scene.
#[test]
fn a_restrained_actor_with_a_dialogue_package_talks_without_moving() {
    let f = fixture();
    let mut s = sim(&f, R_COME, [1000.0, 1000.0, 0.0]);
    s.game.extra.entry(FormId(R_COME)).or_default().restrained = true;
    s.game.extra.entry(FormId(PLAYER)).or_default().restrained = true;
    // The player out of reach: it does not go up to the player (case 3 would).
    s.run(5.0);
    assert_eq!(s.agent(R_COME).walker.feet, CORNER);
    assert_eq!(s.agent(R_COME).talk, Some(Talking::Waiting), "{:?}", s.events());
    assert!(!s.events().iter().any(|e| matches!(e, AiEvent::Approach { .. } | AiEvent::Trip { .. })), "{:?}", s.events());
    assert_eq!(s.talks(), 0);
    // Put beside it (the training's MoveTo): the force greet, from where it stands.
    s.player_to([200.0, 100.0, 0.0]);
    s.run(1.0);
    assert_eq!(s.talks(), 1, "{:?}", s.events());
    assert_eq!(s.game.dialogue.as_ref().map(|c| c.actor), Some(FormId(R_COME)));
    assert_eq!(s.agent(R_COME).talk, Some(Talking::Speaking));
    assert_eq!(s.agent(R_COME).walker.feet, CORNER);
    // The line releases both; the conversation ends.
    s.game.extra.get_mut(&FormId(R_COME)).unwrap().restrained = false;
    s.game.extra.get_mut(&FormId(PLAYER)).unwrap().restrained = false;
    assert!(s.game.end_dialogue());
    s.run(10.0);
    assert_eq!(s.talks(), 1, "{:?}", s.events());
    assert_eq!(s.agent(R_COME).talk, Some(Talking::Done));
}

/// Cachino's case (issue #2473): a Find package for the player near the editor location
/// with radius 0 goes after the player well beyond the 512 of a radius 0 search area, and
/// on reaching them starts a conversation (a force greet, packages.md section 10). Before,
/// the player 1,400 units off was not in the area: "finds nothing more", and it stood.
#[test]
fn a_find_package_for_the_player_goes_to_them_anywhere_and_greets() {
    let f = fixture();
    let far = [1100.0, 1100.0, 0.0];
    let mut s = sim(&f, R_FINDER, far);
    s.run(30.0);
    let ev = s.events();
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::NothingFound { .. })), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::FindGoes { object, .. } if *object == FormId::PLAYER_REF)), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Found { object, act: Act::Greets, .. } if *object == FormId::PLAYER_REF)), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Talk { target, say_to: false, .. } if *target == FormId::PLAYER_REF)), "{ev:?}");
    // The conversation opened (its one line is over by now): GetTalkedToPC reads 1,
    // which ends Cachino's package.
    assert!(s.game.extra.get(&FormId(R_FINDER)).is_some_and(|e| e.talked_to_pc), "it talked to the player");
    assert!(flat(s.agent(R_FINDER).walker.feet, far) < 120.0, "it went up to the player");
}
