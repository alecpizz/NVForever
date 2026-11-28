//! The factions and AI package lists of templated and leveled actors on a synthetic
//! plugin (no game data). docs/spec-notes/factions.md §2 and §3.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::actor_values as av;
use fnv_game::awareness::{Cause, State, Timers};
use fnv_game::detection::{Band, Detection};
use fnv_game::provocation::Note;
use fnv_game::functions::Ctx;
use fnv_game::game::{Game, GameConfig};
use fnv_game::templates::Picks;
use fnv_game::GameData;
use fnv_script::vm::{ArgValue, Value};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const RAIDERS: u32 = 0x900;
const PLAYER_FACTION: u32 = 0x901;
const RATS: u32 = 0x902;
/// A utility faction hostile groups share (as GeneralIdleFaction).
const IDLERS: u32 = 0x903;
const GANG_A: u32 = 0x904;
const GANG_B: u32 = 0x905;
/// The hound's own faction, naming nobody.
const HOUNDS: u32 = 0x906;

const LEADER: u32 = 0x830;
const MEMBER: u32 = 0x831;
const SECOND: u32 = 0x832;
const OWN: u32 = 0x833;
/// A rat-faction NPC that helps its allies (Assistance 1).
const HELPER: u32 = 0x834;
/// A raider and a watchman (of the player's faction, which the raiders name an enemy),
/// both idlers; a loafer, only an idler.
const BANDIT: u32 = 0x835;
const WATCHMAN: u32 = 0x836;
const LOAFER: u32 = 0x837;
/// Members of two gangs neutral to each other, both idlers; a very aggressive one.
const GANGER_A: u32 = 0x838;
const GANGER_B: u32 = 0x839;
const WOLF: u32 = 0x840;
const RAT: u32 = 0x841;
const SPAWN: u32 = 0x842;
const SPAWN_MIXED: u32 = 0x843;
const SPAWN_HIGH: u32 = 0x844;
const LIST_WOLF: u32 = 0x850;
const LIST_MIXED: u32 = 0x851;
const LIST_HIGH: u32 = 0x852;
const LIST_NESTED: u32 = 0x853;
const PACK_WOLF: u32 = 0x860;
const PACK_RAT: u32 = 0x861;
const PACK_OWN: u32 = 0x862;
/// A Travel package that skips Fallout behaviour with every behaviour bit clear (as
/// HELIOS One's ProtectronNotActive).
const PACK_STILL: u32 = 0x863;
const ADD_SCRIPT_PACKAGE: u16 = 0x1097;
/// A rat that Helps Allies but is Cowardly (playtest list 4, item 14).
const COWARD: u32 = 0x870;
/// An unaggressive rat with an aggro radius of 650 (item 18).
const LURKER: u32 = 0x871;
/// A creature with no data of its own: name (Base Data), traits, inventory and script
/// from the wolf list (item 20).
const NAMELESS: u32 = 0x872;
/// What a wolf carries (a created leveled wolf carries it too).
/// A Very Aggressive guard dog with an aggro radius of 2,000 (as HELIOS One's
/// CrDogNCRAggressive, #2547), in a faction of its own.
const HOUND: u32 = 0x873;
const HOUND_REF: u32 = 0x331;
const PELT: u32 = 0x880;

const CELL: u32 = 0x200;
const LEADER_REF: u32 = 0x320;
const MEMBER_REF: u32 = 0x321;
const SECOND_REF: u32 = 0x322;
const OWN_REF: u32 = 0x323;
const SPAWN_REF: u32 = 0x324;
const SPAWN_HIGH_REF: u32 = 0x325;
const LIST_REF: u32 = 0x326;
const HELPER_REF: u32 = 0x327;
const BANDIT_REF: u32 = 0x328;
const WATCHMAN_REF: u32 = 0x329;
const LOAFER_REF: u32 = 0x32A;
const GANGER_A_REF: u32 = 0x32B;
const GANGER_B_REF: u32 = 0x32C;
const COWARD_REF: u32 = 0x32D;
const LURKER_REF: u32 = 0x32E;
const NAMELESS_REF: u32 = 0x32F;
/// Sixteen references of the creature whose template list holds a wolf and a rat.
const MIXED_REFS: std::ops::Range<u32> = 0x340..0x350;
const PLAYER: u32 = 0x14;

const GET_AV: u16 = 0x100E;
const GET_IN_FACTION: u16 = 0x1047;
const GET_FACTION_RANK: u16 = 0x1049;
const GET_FACTION_RELATION: u16 = 0x11C2;
const ADD_TO_FACTION: u16 = 0x117F;
const SET_ENEMY: u16 = 0x1078;
const GET_CURRENT_AI_PACKAGE: u16 = 0x106E;
const GET_CURRENT_AI_PROCEDURE: u16 = 0x108F;
const GET_IS_ALERTED: u16 = 0x105B;

/// Template flags: Factions (bit 2), AI Data (bit 4) and AI Packages (bit 5).
const FACTIONS: u16 = 0x4;
const AI_DATA: u16 = 0x10;
const AI_PACKAGES: u16 = 0x20;
/// Traits (bit 0), Base Data (bit 7), Inventory (bit 8), Script (bit 9).
const TRAITS: u16 = 0x1;
const BASE_DATA: u16 = 0x80;
const INVENTORY: u16 = 0x100;
const SCRIPT: u16 = 0x200;

fn acbs(template_flags: u16) -> fnv_esm::raw::Subrecord {
    let mut d = 0u32.to_le_bytes().to_vec();
    for v in [50u16, 0, 1, 0, 0, 100] {
        d.extend(v.to_le_bytes());
    }
    d.extend(0f32.to_le_bytes());
    d.extend(0i16.to_le_bytes());
    d.extend(template_flags.to_le_bytes());
    sub(b"ACBS", d)
}

fn snam(faction: u32, rank: u8) -> fnv_esm::raw::Subrecord {
    sub(b"SNAM", [faction.to_le_bytes().to_vec(), vec![rank, 0, 0, 0]].concat())
}

fn pkid(package: u32) -> fnv_esm::raw::Subrecord {
    sub(b"PKID", package.to_le_bytes().to_vec())
}

fn tplt(template: u32) -> fnv_esm::raw::Subrecord {
    sub(b"TPLT", template.to_le_bytes().to_vec())
}

fn aidt(aggression: u8) -> fnv_esm::raw::Subrecord {
    aidt_assisting(aggression, 0)
}

/// AIDT with an Assistance code (byte 14: 0 Helps Nobody, 1 Helps Allies, 2 Helps Friends
/// and Allies).
fn aidt_assisting(aggression: u8, assistance: u8) -> fnv_esm::raw::Subrecord {
    aidt_full(aggression, 0, assistance, None)
}

/// AIDT with a Confidence code (byte 1: 0 Cowardly … 4 Foolhardy) and an aggro radius
/// (byte 15 Aggro Radius Behavior, bytes 16–19 the radius).
fn aidt_full(aggression: u8, confidence: u8, assistance: u8, aggro: Option<i32>) -> fnv_esm::raw::Subrecord {
    let mut d = vec![aggression, confidence, 50, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, assistance, u8::from(aggro.is_some())];
    d.extend(aggro.unwrap_or(0).to_le_bytes());
    sub(b"AIDT", d)
}

/// FACT XNAM: faction, disposition modifier, group combat reaction (1 Enemy).
fn xnam(faction: u32, reaction: u32) -> fnv_esm::raw::Subrecord {
    sub(b"XNAM", [faction.to_le_bytes(), 0i32.to_le_bytes(), reaction.to_le_bytes()].concat())
}

fn list(fid: u32, edid: &str, entries: &[(i16, u32)]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0])];
    for (level, form) in entries {
        subs.push(sub(b"LVLO", [level.to_le_bytes().to_vec(), vec![0; 2], form.to_le_bytes().to_vec(), 1i16.to_le_bytes().to_vec(), vec![0; 2]].concat()));
    }
    record(b"LVLC", 0, fid, &subs)
}

fn placed(sig: &[u8; 4], fid: u32, base: u32) -> Vec<u8> {
    record(sig, 0, fid, &[sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])])
}

fn plugin_bytes() -> Vec<u8> {
    // The raiders name the player's faction an enemy; the player's faction names nobody.
    let factions = [
        record(b"FACT", 0, RAIDERS, &[sub(b"EDID", z("Raiders")), xnam(PLAYER_FACTION, 1)]),
        record(b"FACT", 0, PLAYER_FACTION, &[sub(b"EDID", z("PlayerFaction"))]),
        record(b"FACT", 0, RATS, &[sub(b"EDID", z("Rats"))]),
        record(b"FACT", 0, IDLERS, &[sub(b"EDID", z("Idlers"))]),
        // Two gangs that name nobody but a third faction, each other not at all.
        record(b"FACT", 0, GANG_A, &[sub(b"EDID", z("GangA")), xnam(RATS, 0)]),
        record(b"FACT", 0, GANG_B, &[sub(b"EDID", z("GangB")), xnam(RATS, 0)]),
        record(b"FACT", 0, HOUNDS, &[sub(b"EDID", z("Hounds"))]),
    ]
    .concat();
    let npcs = [
        record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player")), acbs(0), snam(PLAYER_FACTION, 0)]),
        record(b"NPC_", 0, LEADER, &[sub(b"EDID", z("Leader")), acbs(0), snam(RAIDERS, 2)]),
        // Factions from the template: its own list is not read.
        record(b"NPC_", 0, MEMBER, &[sub(b"EDID", z("Member")), acbs(FACTIONS), snam(RATS, 0), tplt(LEADER)]),
        record(b"NPC_", 0, SECOND, &[sub(b"EDID", z("Second")), acbs(FACTIONS), tplt(MEMBER)]),
        // A template, but not for the factions.
        record(b"NPC_", 0, OWN, &[sub(b"EDID", z("Own")), acbs(AI_DATA), snam(RATS, 1), tplt(LEADER)]),
        record(b"NPC_", 0, HELPER, &[sub(b"EDID", z("Helper")), acbs(0), snam(RATS, 0), aidt_full(0, 2, 1, None)]),
        record(b"NPC_", 0, BANDIT, &[sub(b"EDID", z("Bandit")), acbs(0), snam(RAIDERS, 0), snam(IDLERS, 0), aidt(1)]),
        record(b"NPC_", 0, WATCHMAN, &[sub(b"EDID", z("Watchman")), acbs(0), snam(PLAYER_FACTION, 0), snam(IDLERS, 0), aidt(1)]),
        record(b"NPC_", 0, LOAFER, &[sub(b"EDID", z("Loafer")), acbs(0), snam(IDLERS, 0), aidt(1)]),
        record(b"NPC_", 0, GANGER_A, &[sub(b"EDID", z("GangerA")), acbs(0), snam(GANG_A, 0), snam(IDLERS, 0), aidt(2)]),
        record(b"NPC_", 0, GANGER_B, &[sub(b"EDID", z("GangerB")), acbs(0), snam(GANG_B, 0), snam(IDLERS, 0), aidt(1)]),
    ]
    .concat();
    let creatures = [
        record(b"CREA", 0, WOLF, &[sub(b"EDID", z("Wolf")), acbs(0), snam(RAIDERS, 0), aidt(2), pkid(PACK_WOLF), sub(b"CNTO", [PELT.to_le_bytes(), 1i32.to_le_bytes()].concat())]),
        record(b"CREA", 0, RAT, &[sub(b"EDID", z("Rat")), acbs(0), snam(RATS, 0), aidt(0), pkid(PACK_RAT)]),
        record(b"CREA", 0, SPAWN, &[sub(b"EDID", z("Spawn")), acbs(FACTIONS | AI_DATA | AI_PACKAGES), tplt(LIST_NESTED), aidt(0)]),
        record(b"CREA", 0, SPAWN_MIXED, &[sub(b"EDID", z("SpawnMixed")), acbs(FACTIONS | AI_DATA | AI_PACKAGES), tplt(LIST_MIXED), aidt(1)]),
        // Its list has nothing for a level-1 player: it keeps its own.
        record(b"CREA", 0, SPAWN_HIGH, &[sub(b"EDID", z("SpawnHigh")), acbs(FACTIONS | AI_PACKAGES), snam(RATS, 3), pkid(PACK_OWN), tplt(LIST_HIGH)]),
        record(b"CREA", 0, COWARD, &[sub(b"EDID", z("Coward")), acbs(0), snam(RATS, 0), aidt_full(0, 0, 1, None)]),
        record(b"CREA", 0, LURKER, &[sub(b"EDID", z("Lurker")), acbs(0), snam(RATS, 0), aidt_full(0, 4, 1, Some(650))]),
        record(b"CREA", 0, HOUND, &[sub(b"EDID", z("Hound")), acbs(0), snam(HOUNDS, 0), aidt_full(2, 4, 1, Some(2000))]),
        record(b"CREA", 0, NAMELESS, &[sub(b"EDID", z("Nameless")), acbs(BASE_DATA | INVENTORY | SCRIPT | TRAITS), tplt(LIST_WOLF), aidt(0)]),
    ]
    .concat();
    let lists = [
        list(LIST_WOLF, "ListWolf", &[(1, WOLF)]),
        list(LIST_MIXED, "ListMixed", &[(1, WOLF), (1, RAT)]),
        list(LIST_HIGH, "ListHigh", &[(5, WOLF)]),
        list(LIST_NESTED, "ListNested", &[(1, LIST_WOLF), (9, RAT)]),
    ]
    .concat();
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    let mut refs = [
        placed(b"ACHR", LEADER_REF, LEADER),
        placed(b"ACHR", MEMBER_REF, MEMBER),
        placed(b"ACHR", SECOND_REF, SECOND),
        placed(b"ACHR", OWN_REF, OWN),
        placed(b"ACRE", SPAWN_REF, SPAWN),
        placed(b"ACRE", SPAWN_HIGH_REF, SPAWN_HIGH),
        placed(b"ACRE", LIST_REF, LIST_WOLF),
        placed(b"ACHR", HELPER_REF, HELPER),
        placed(b"ACHR", BANDIT_REF, BANDIT),
        placed(b"ACHR", WATCHMAN_REF, WATCHMAN),
        placed(b"ACHR", LOAFER_REF, LOAFER),
        placed(b"ACHR", GANGER_A_REF, GANGER_A),
        placed(b"ACHR", GANGER_B_REF, GANGER_B),
        placed(b"ACRE", COWARD_REF, COWARD),
        placed(b"ACRE", LURKER_REF, LURKER),
        placed(b"ACRE", NAMELESS_REF, NAMELESS),
        placed(b"ACRE", HOUND_REF, HOUND),
    ]
    .concat();
    for r in MIXED_REFS {
        refs.extend(placed(b"ACRE", r, SPAWN_MIXED));
    }
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs));
    [
        tes4(),
        top(b"FACT", &factions),
        top(b"MISC", &record(b"MISC", 0, PELT, &[sub(b"EDID", z("Pelt")), sub(b"DATA", [1i32.to_le_bytes(), 1f32.to_le_bytes()].concat())])),
        top(b"NPC_", &npcs),
        top(
            b"PACK",
            &record(
                b"PACK",
                0,
                PACK_STILL,
                &[
                    sub(b"EDID", z("Still")),
                    sub(b"PKDT", [&0x1000u32.to_le_bytes()[..], &[6, 0], &[0; 6]].concat()),
                    sub(b"PLDT", [0i32.to_le_bytes(), 0u32.to_le_bytes(), 0i32.to_le_bytes()].concat()),
                    sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat()),
                ],
            ),
        ),
        top(b"CREA", &creatures),
        top(b"LVLC", &lists),
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
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Factions.esm".into()).unwrap()];
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

fn form(f: u32) -> ArgValue {
    ArgValue::Value(Value::Ref(f))
}

fn call(g: &mut Game, op: u16, on: u32, args: &[ArgValue]) -> f64 {
    let ctx = Ctx { unit: "test".into(), opcode: op, on: Some(FormId(on)), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    match g.call_function(&ctx, args) {
        Value::Num(x) => x,
        Value::Ref(f) => f as f64,
    }
}

fn rank(g: &mut Game, on: u32, faction: u32) -> f64 {
    call(g, GET_FACTION_RANK, on, &[form(faction)])
}

#[test]
fn a_shared_ally_faction_wins_over_an_enemy_one() {
    // #2560: FortVulpesInculta is in VCaesarsLegionFaction (an Ally of itself) and in
    // ArmorNCRFactionNVEnemy (which VCaesarsLegionFaction names an Enemy); the Legion
    // never attacks him. Allies and friends "cannot attack each other under any
    // circumstances" [documented: GECK wiki GetFactionCombatReaction] (factions.md §4).
    // Here the raiders name the player's faction an enemy; the bandit (raider, idler)
    // attacks the watchman (player's faction, idler) until the idlers name themselves an
    // ally.
    let f = fixture();
    let mut g = game(&f);
    let (bandit, watchman) = (FormId(BANDIT_REF), FormId(WATCHMAN_REF));
    assert_eq!(call(&mut g, GET_FACTION_RELATION, BANDIT_REF, &[form(WATCHMAN_REF)]), 1.0);
    assert!(g.would_attack(bandit, watchman));
    g.forms.faction_reactions.entry(FormId(IDLERS)).or_default().insert(FormId(IDLERS), 2);
    assert_eq!(call(&mut g, GET_FACTION_RELATION, BANDIT_REF, &[form(WATCHMAN_REF)]), 2.0);
    assert_eq!(call(&mut g, GET_FACTION_RELATION, WATCHMAN_REF, &[form(BANDIT_REF)]), 2.0);
    assert!(!g.would_attack(bandit, watchman) && !g.would_attack(watchman, bandit));
    // A friend pair wins as well, and before an ally one (see crates/fnv-game/src/functions/actor.rs, `faction_relation`).
    g.forms.faction_reactions.entry(FormId(RAIDERS)).or_default().insert(FormId(IDLERS), 3);
    assert_eq!(call(&mut g, GET_FACTION_RELATION, BANDIT_REF, &[form(WATCHMAN_REF)]), 3.0);
    // The player's crime enemies still come first.
    g.forms.faction_reactions.entry(FormId(RAIDERS)).or_default().insert(FormId(PLAYER_FACTION), 2);
    g.player.enemy_of.insert(FormId(RAIDERS));
    assert_eq!(call(&mut g, GET_FACTION_RELATION, BANDIT_REF, &[form(PLAYER)]), 1.0);
}

#[test]
fn the_records_are_read() {
    let f = fixture();
    let a = &f.data.actors;
    assert!(a[&FormId(MEMBER)].template_factions && !a[&FormId(OWN)].template_factions && !a[&FormId(LEADER)].template_factions);
    assert_eq!(a[&FormId(MEMBER)].factions, vec![(FormId(RATS), 0)]);
    let l = &f.data.leveled_actors[&FormId(LIST_NESTED)];
    assert_eq!((l.all_levels, l.entries.clone()), (false, vec![(1, FormId(LIST_WOLF)), (9, FormId(RAT))]));
}

/// The extender's base faction getters read the Use Factions template's list through NPC
/// records, the record's own when the chain reaches a leveled list (actors.md §2.2;
/// observed: o5 and tmpl3, #1057).
#[test]
fn base_faction_getters_follow_the_factions_template() {
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = game(&f);
    let mut q = |body: &str| g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).expect("the query runs");
    // Member (own Rats) and Second (through Member) both answer Leader's Raiders, rank 2.
    for npc in ["Member", "Second"] {
        assert_eq!(q(&format!("SetFunctionValue (GetBaseNumFactions {npc})")), XVal::Num(1.0), "{npc}");
        assert_eq!(q(&format!("SetFunctionValue (GetBaseNthFaction 0 {npc})")), XVal::Ref(RAIDERS), "{npc}");
        assert_eq!(q(&format!("SetFunctionValue (GetBaseNthRank 0 {npc})")), XVal::Num(2.0), "{npc}");
        assert_eq!(q(&format!("SetFunctionValue (ar_Size (GetBaseFactions {npc}))")), XVal::Num(1.0), "{npc}");
    }
    // A template not used for the factions, and a leveled-list template: their own.
    assert_eq!(q("SetFunctionValue (GetBaseNthRank 0 Own)"), XVal::Num(1.0));
    assert_eq!(q("SetFunctionValue (GetBaseNthFaction 0 SpawnHigh)"), XVal::Ref(RATS));
    assert_eq!(q("SetFunctionValue (GetBaseNthRank 0 SpawnHigh)"), XVal::Num(3.0));
}

#[test]
fn the_template_supplies_the_factions_it_is_flagged_for() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(rank(&mut g, LEADER_REF, RAIDERS), 2.0);
    // The template's factions and ranks, not the actor's own list; through two templates.
    for r in [MEMBER_REF, SECOND_REF] {
        assert_eq!(rank(&mut g, r, RAIDERS), 2.0);
        assert_eq!(call(&mut g, GET_IN_FACTION, r, &[form(RATS)]), 0.0);
    }
    // Without the flag the actor's own list counts.
    assert_eq!((rank(&mut g, OWN_REF, RATS), rank(&mut g, OWN_REF, RAIDERS)), (1.0, -1.0));
    // An actor's faction towards the other's: the raiders name the player's faction an
    // enemy, the player's faction names nobody.
    assert_eq!(call(&mut g, GET_FACTION_RELATION, MEMBER_REF, &[form(PLAYER)]), 1.0);
    assert_eq!(call(&mut g, GET_FACTION_RELATION, PLAYER, &[form(MEMBER_REF)]), 0.0);
    assert_eq!(call(&mut g, GET_FACTION_RELATION, OWN_REF, &[form(PLAYER)]), 0.0);
}

#[test]
fn factions_kept_for_a_combat_ai_frame_follow_the_scripts_that_change_them() {
    let f = fixture();
    let mut g = game(&f);
    g.begin_faction_frame();
    assert_eq!(call(&mut g, GET_FACTION_RELATION, OWN_REF, &[form(PLAYER)]), 0.0);
    assert_eq!(call(&mut g, GET_IN_FACTION, OWN_REF, &[form(RAIDERS)]), 0.0);
    // Kept, then forgotten when a script adds the actor to the raiders.
    call(&mut g, ADD_TO_FACTION, OWN_REF, &[form(RAIDERS), ArgValue::Value(Value::Num(0.0))]);
    assert_eq!(call(&mut g, GET_IN_FACTION, OWN_REF, &[form(RAIDERS)]), 1.0);
    assert_eq!(call(&mut g, GET_FACTION_RELATION, OWN_REF, &[form(PLAYER)]), 1.0);
    // And when a script makes the raiders neutral to the player's faction.
    assert_eq!(call(&mut g, GET_FACTION_RELATION, MEMBER_REF, &[form(PLAYER)]), 1.0);
    let n = |x: f64| ArgValue::Value(Value::Num(x));
    let ctx = fnv_game::functions::Ctx { unit: "test".into(), opcode: SET_ENEMY, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[form(RAIDERS), form(PLAYER_FACTION), n(1.0), n(1.0)]);
    assert_eq!(call(&mut g, GET_FACTION_RELATION, MEMBER_REF, &[form(PLAYER)]), 0.0);
    g.end_faction_frame();
    assert_eq!(call(&mut g, GET_FACTION_RELATION, MEMBER_REF, &[form(PLAYER)]), 0.0);
}

#[test]
fn a_leveled_list_stands_for_the_entry_picked() {
    let f = fixture();
    let mut g = game(&f);
    // A template that is a list (here through a nested list) with one actor to pick, and
    // a list placed directly: the wolf's factions and AI data.
    for r in [SPAWN_REF, LIST_REF] {
        assert_eq!(rank(&mut g, r, RAIDERS), 0.0);
        assert_eq!(call(&mut g, GET_FACTION_RELATION, r, &[form(PLAYER)]), 1.0);
    }
    assert_eq!(call(&mut g, GET_AV, SPAWN_REF, &[ArgValue::Enum(av::AGGRESSION)]), 2.0);
    // A list with nothing at the level: the actor's own.
    assert_eq!(rank(&mut g, SPAWN_HIGH_REF, RATS), 3.0);
    // Two actors to pick from: each reference is one of them in every group, and the
    // same one every time; over the references both come up.
    let mut wolves = 0;
    for r in MIXED_REFS {
        let wolf = rank(&mut g, r, RAIDERS) == 0.0;
        assert_eq!(rank(&mut g, r, RATS), if wolf { -1.0 } else { 0.0 });
        assert_eq!(call(&mut g, GET_AV, r, &[ArgValue::Enum(av::AGGRESSION)]), if wolf { 2.0 } else { 0.0 });
        let picked = f.data.actor_base(FormId(LIST_MIXED), &mut Picks::new(FormId(r)));
        assert_eq!(picked, Some(FormId(if wolf { WOLF } else { RAT })));
        assert_eq!(f.data.base_factions(FormId(SPAWN_MIXED), FormId(r)), [(FormId(if wolf { RAIDERS } else { RATS }), 0)]);
        wolves += wolf as u32;
    }
    assert!(wolves > 0 && wolves < MIXED_REFS.len() as u32, "{wolves} wolves");
    assert_eq!(f.data.template_sources_any_pick(FormId(SPAWN_MIXED), |d| d.template_factions), vec![FormId(WOLF), FormId(RAT)]);
}

#[test]
fn a_resurrection_roll_moves_every_group_to_the_new_pick() {
    // ResurrectActor rolls a leveled actor's pick again (seed `roll_seed`), as the engine
    // draws it and its items come; its factions, AI data and packages follow that pick
    // too, not the first one (docs/spec-notes/actors.md §1.3).
    let f = fixture();
    let mut g = game(&f);
    let pick = |seed: FormId| f.data.actor_base(FormId(LIST_MIXED), &mut Picks::new(seed));
    let mut changed = 0;
    for r in MIXED_REFS {
        let first = pick(FormId(r));
        let Some(rolls) = (1..64).find(|&k| pick(fnv_sim::leveled::roll_seed(FormId(r), k)) != first) else { continue };
        g.extra.entry(FormId(r)).or_default().rolls = rolls;
        let wolf = pick(fnv_sim::leveled::roll_seed(FormId(r), rolls)) == Some(FormId(WOLF));
        assert_eq!(rank(&mut g, r, RAIDERS), if wolf { 0.0 } else { -1.0 }, "{r:#x}");
        assert_eq!(call(&mut g, GET_AV, r, &[ArgValue::Enum(av::AGGRESSION)]), if wolf { 2.0 } else { 0.0 });
        assert_eq!(g.package_list(FormId(r)), vec![FormId(if wolf { PACK_WOLF } else { PACK_RAT })]);
        changed += 1;
    }
    assert!(changed > 0);
}

#[test]
fn the_package_list_comes_from_the_same_source() {
    let f = fixture();
    let g = game(&f);
    // Through a nested list, and a list placed directly: the wolf's packages.
    for r in [SPAWN_REF, LIST_REF] {
        assert_eq!(g.package_list(FormId(r)), vec![FormId(PACK_WOLF)], "{r:#x}");
    }
    // A list with nothing at the level: the actor's own.
    assert_eq!(g.package_list(FormId(SPAWN_HIGH_REF)), vec![FormId(PACK_OWN)]);
    // Two actors to pick from: the packages of the one whose factions the reference has.
    for r in MIXED_REFS {
        let wolf = f.data.base_factions(FormId(SPAWN_MIXED), FormId(r)) == [(FormId(RAIDERS), 0)];
        assert_eq!(g.package_list(FormId(r)), vec![FormId(if wolf { PACK_WOLF } else { PACK_RAT })]);
    }
    // Without a template for the packages, an actor's own list (empty here).
    assert!(g.package_list(FormId(MEMBER_REF)).is_empty());
}

/// Every placed actor of the cell but `keep` stands 50,000 units off: alarms reach the
/// loaded cells' actors too, not only those measured (`Game::raise_alarms`).
fn only_near(g: &mut Game, keep: &[u32]) {
    let all = (LEADER_REF..=NAMELESS_REF).chain(MIXED_REFS);
    for r in all.filter(|r| !keep.contains(r)) {
        g.world.get(FormId(r)).unwrap().pos = [50_000.0, 0.0, 0.0];
    }
}

/// Put `actor` in Combat against `target`, detected now.
fn fighting(g: &mut Game, actor: u32, target: u32) {
    let (now, t) = (g.time, Timers::default());
    let a = g.awareness.actors.entry(FormId(actor)).or_default();
    a.observe(FormId(target), &Detection { value: 5.0, band: Band::Detected, ..Default::default() }, [0.0; 3], now, &t);
    a.state = State::Combat;
}

#[test]
fn a_neutral_hit_by_the_player_fights_and_accepts_one_yield() {
    // docs/design/combat-ai.md §10.1 and §10.2 (playtest list 2026-10-11, item 30).
    let f = fixture();
    let mut g = game(&f);
    let (own, player) = (FormId(OWN_REF), FormId::PLAYER_REF);
    assert!(!g.would_attack(own, player), "neutral and not aggressive");
    assert!(g.attacked_by(own, player), "a grudge");
    assert!(g.would_attack(own, player) && !g.attacked_by(own, player));
    assert!(g.provocations.take_notes().iter().any(|n| matches!(n, Note::Assaulted { actor, .. } if *actor == own)));
    // Not fighting yet: nothing to yield to.
    assert_eq!(g.player_yields(), 0);
    fighting(&mut g, OWN_REF, 0x14);
    g.time += 3.0;
    assert_eq!(g.player_yields(), 1);
    assert!(!g.would_attack(own, player) && g.awareness.state(own) == State::Normal);
    // Attacked again soon after: the next yield is ignored.
    g.time += 30.0;
    assert!(g.attacked_by(own, player));
    fighting(&mut g, OWN_REF, 0x14);
    g.provocations.take_notes();
    assert_eq!(g.player_yields(), 0);
    assert!(g.provocations.take_notes().iter().any(|n| matches!(n, Note::Refuses { why: "it forgave the player recently", .. })));
    assert!(g.would_attack(own, player));
    // A yield long after the fight began is too late, whoever it is.
    g.stop_combat(own);
    g.time += 700.0;
    assert!(g.attacked_by(own, player));
    fighting(&mut g, OWN_REF, 0x14);
    g.time += 11.0;
    g.provocations.take_notes();
    assert_eq!(g.player_yields(), 0);
    assert!(g.provocations.take_notes().iter().any(|n| matches!(n, Note::Refuses { why: "the fight is past the time to yield", .. })));
    // Past the retry time and within the yield time: accepted again.
    g.stop_combat(own);
    assert!(g.attacked_by(own, player));
    fighting(&mut g, OWN_REF, 0x14);
    assert_eq!(g.player_yields(), 1);
}

#[test]
fn a_neutral_fighting_someone_else_ignores_a_stray_hit_from_a_fighting_player() {
    // The owner's exception (combat-ai.md §10.1).
    let f = fixture();
    let mut g = game(&f);
    let (own, player) = (FormId(OWN_REF), FormId::PLAYER_REF);
    fighting(&mut g, OWN_REF, LEADER_REF);
    // The player is not fighting anyone: the hit counts.
    assert!(g.attacked_by(own, player));
    g.stop_combat(own);
    // Both fight someone else now: it does not turn.
    fighting(&mut g, OWN_REF, LEADER_REF);
    fighting(&mut g, LEADER_REF, 0x14);
    assert!(!g.attacked_by(own, player));
    assert!(!g.would_attack(own, player));
    assert!(g.provocations.take_notes().iter().any(|n| matches!(n, Note::Ignored { why: "both were fighting someone else", .. })));
}

#[test]
fn an_ally_that_sees_a_fight_helps() {
    // Assistance (combat-ai.md §10.3): the helper shares the Rats faction with the
    // victim, so it is its ally, and it Helps Allies.
    let f = fixture();
    let mut g = game(&f);
    let (own, helper, player) = (FormId(OWN_REF), FormId(HELPER_REF), FormId::PLAYER_REF);
    assert!(g.attacked_by(own, player));
    fighting(&mut g, OWN_REF, 0x14);
    // Not seen yet: nobody joins.
    assert_eq!(g.call_allies(), 0);
    g.detection.pairs.insert((helper, own), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, g.time));
    assert_eq!(g.call_allies(), 1);
    assert!(g.would_attack(helper, player));
    assert!(g.awareness.get(helper).is_some_and(|a| a.alarms.iter().any(|x| x.owner == player && x.cause == Cause::Ally)));
    // Once only; and never against its own ally (the other rat is not a foe).
    assert_eq!(g.call_allies(), 0);
    fighting(&mut g, OWN_REF, SPAWN_HIGH_REF);
    assert_eq!(g.call_allies(), 0);
    assert!(!g.would_attack(helper, FormId(SPAWN_HIGH_REF)));
}

#[test]
fn a_faction_friend_that_helps_friends_joins_a_fight_against_the_player() {
    // #2927 (combat-groups.md §4) [observed, base game: Gibson's dogs, Helps Friends and
    // Allies, whose faction names the mole rats' a Friend and who are neutral to the
    // player, went into combat with the player when mole rats attacked it beside the
    // yard, and stayed in it after the mole rats were dead]. The hounds are made friends
    // of the rats (SetAlly with both friend flags); a rat (Own) fights the player. The
    // hound, neutral to the player, detects the rat: with Helps Friends and Allies it
    // takes the fight up, by assistance and by the rat's alarm; with Helps Allies or
    // Helps Nobody it stays out; its grudge outlasts the rat.
    const SET_ALLY: u16 = 0x1079;
    const SET_AV: u16 = 0x100F;
    use fnv_game::awareness::Change;
    let (own, hound, player) = (FormId(OWN_REF), FormId(HOUND_REF), FormId::PLAYER_REF);
    let set_up = |g: &mut Game, assistance: f64| {
        let ctx = Ctx { unit: "test".into(), opcode: SET_ALLY, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, &[form(HOUNDS), form(RATS), ArgValue::Value(Value::Num(1.0)), ArgValue::Value(Value::Num(1.0))]);
        // Aggressive, as the dogs are (the fixture's hound is Very Aggressive).
        call(g, SET_AV, HOUND_REF, &[ArgValue::Enum(av::AGGRESSION), ArgValue::Value(Value::Num(1.0))]);
        call(g, SET_AV, HOUND_REF, &[ArgValue::Enum(av::ASSISTANCE), ArgValue::Value(Value::Num(assistance))]);
        assert_eq!(g.reaction_to(hound, own), fnv_game::provocation::FRIEND);
        assert!(!g.would_attack(hound, player), "neutral to the player");
        assert!(g.attacked_by(own, player));
        fighting(g, OWN_REF, PLAYER);
        let t = g.time;
        g.detection.pairs.insert((hound, own), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, t));
    };
    let f = fixture();
    // Helps Friends and Allies: assistance.
    let mut g = game(&f);
    set_up(&mut g, 2.0);
    assert_eq!(g.call_allies(), 1);
    assert!(g.would_attack(hound, player));
    assert!(g.awareness.get(hound).is_some_and(|a| a.alarms.iter().any(|x| x.owner == player && x.cause == Cause::Ally)));
    assert!(g.provocations.take_notes().iter().any(|n| matches!(n, Note::Assists { helper, victim, attacker } if *helper == hound && *victim == own && *attacker == player)));
    // The rat dies: the hound's grudge against the player stays.
    g.extra.entry(own).or_default().dead = true;
    assert!(g.would_attack(hound, player));
    // The same by the rat's alarm when it starts fighting (no detection needed).
    let mut g = game(&f);
    set_up(&mut g, 2.0);
    g.detection.pairs.clear();
    only_near(&mut g, &[OWN_REF]);
    let now = g.time;
    g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(player), cause: None, at: Some([500.0, 0.0, 0.0]), value: Some(5.0) });
    let near: std::collections::BTreeMap<FormId, [f32; 3]> = [(own, [0.0, 0.0, 0.0]), (hound, [1000.0, 0.0, 0.0]), (player, [500.0, 0.0, 0.0])].into_iter().collect();
    assert_eq!(g.raise_alarms(&near), 1);
    assert!(g.would_attack(hound, player));
    // Helps Allies (a friend is not an ally) and Helps Nobody: it stays out of both.
    for assistance in [1.0, 0.0] {
        let mut g = game(&f);
        set_up(&mut g, assistance);
        assert_eq!(g.call_allies(), 0, "Assistance {assistance}");
        only_near(&mut g, &[OWN_REF]);
        let now = g.time;
        g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(player), cause: None, at: Some([500.0, 0.0, 0.0]), value: Some(5.0) });
        assert_eq!(g.raise_alarms(&near), 0, "Assistance {assistance}");
        assert!(!g.would_attack(hound, player));
    }
}

#[test]
fn a_faction_ally_of_the_player_takes_up_the_players_fight() {
    // #2794 (see docs/design/combat-ai.md §10.3). The rats are made allies of the
    // player's faction (SetAlly); the bandit (a
    // raider) attacks the player. The helper (Helps Allies) detects the player and joins;
    // the coward (Helps Allies, Cowardly) stays out; neither while the rats are neutral to
    // the player. With Helps Nobody the helper stays out; it never fights its own ally (a
    // rat attacking the player); after the player itself (a crime) it stays out; as a
    // teammate it is left to `teammates_assist`.
    const SET_ALLY: u16 = 0x1079;
    const SET_AV: u16 = 0x100F;
    let (bandit, helper, coward, own, player) = (FormId(BANDIT_REF), FormId(HELPER_REF), FormId(COWARD_REF), FormId(OWN_REF), FormId::PLAYER_REF);
    let ally = |g: &mut Game| {
        let ctx = Ctx { unit: "test".into(), opcode: SET_ALLY, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, &[form(RATS), form(PLAYER_FACTION)]);
    };
    let sees_player = |g: &mut Game, who: FormId| {
        let t = g.time;
        g.detection.pairs.insert((who, player), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, t));
    };
    let f = fixture();
    let mut g = game(&f);
    assert!(g.would_attack(bandit, player) && !g.would_attack(helper, bandit));
    fighting(&mut g, BANDIT_REF, PLAYER);
    sees_player(&mut g, helper);
    sees_player(&mut g, coward);
    assert_eq!(g.call_allies(), 0, "neutral to the player");
    ally(&mut g);
    assert_eq!(g.reaction_to(helper, player), fnv_game::provocation::ALLY);
    assert_eq!(g.call_allies(), 1);
    assert!(g.would_attack(helper, bandit) && !g.would_attack(coward, bandit));
    assert!(g.awareness.get(helper).is_some_and(|a| a.alarms.iter().any(|x| x.owner == bandit && x.cause == Cause::Ally)));
    let notes = g.provocations.take_notes();
    assert!(notes.iter().any(|n| matches!(n, Note::Assists { helper: h, victim, attacker } if *h == helper && *victim == player && *attacker == bandit)));
    assert!(notes.iter().any(|n| matches!(n, Note::StaysOut { helper: h, victim } if *h == coward && *victim == player)));
    assert_eq!(g.call_allies(), 0, "once");

    // Helps Nobody: stays out.
    let mut g = game(&f);
    ally(&mut g);
    call(&mut g, SET_AV, HELPER_REF, &[ArgValue::Enum(av::ASSISTANCE), ArgValue::Value(Value::Num(0.0))]);
    fighting(&mut g, BANDIT_REF, PLAYER);
    sees_player(&mut g, helper);
    assert_eq!(g.call_allies(), 0);
    assert!(!g.would_attack(helper, bandit));

    // An attacker that is the helper's own ally (a fellow rat): not fought.
    let mut g = game(&f);
    ally(&mut g);
    fighting(&mut g, OWN_REF, PLAYER);
    sees_player(&mut g, helper);
    g.detection.pairs.insert((helper, own), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, g.time));
    assert_eq!(g.call_allies(), 0);
    assert!(!g.would_attack(helper, own) && !g.would_attack(helper, player));

    // A helper after the player itself (a crime of the player's against it), its faction
    // reaction still Ally: it does not join, so two of the player's attackers never turn
    // on each other.
    let mut g = game(&f);
    ally(&mut g);
    g.player.crime_enemies.insert(helper);
    assert_eq!(g.reaction_to(helper, player), fnv_game::provocation::ALLY);
    assert!(g.would_attack(helper, player));
    fighting(&mut g, BANDIT_REF, PLAYER);
    sees_player(&mut g, helper);
    assert_eq!(g.call_allies(), 0);
    assert!(!g.provocations.holds(helper, bandit));
    assert!(!g.provocations.take_notes().iter().any(|n| matches!(n, Note::Assists { helper: h, .. } if *h == helper)));

    // A teammate: its own rule, one note only.
    let mut g = game(&f);
    ally(&mut g);
    g.extra.entry(helper).or_default().teammate = true;
    fighting(&mut g, BANDIT_REF, PLAYER);
    sees_player(&mut g, helper);
    assert_eq!(g.call_allies(), 1);
    let notes = g.provocations.take_notes();
    assert_eq!(notes.iter().filter(|n| matches!(n, Note::Assists { helper: h, .. } if *h == helper)).count(), 1);
    assert!(!g.provocations.holds(helper, bandit), "no Assistance grudge: the teammate rule gives an alarm");
}

#[test]
fn an_actor_that_sees_a_danger_to_itself_fight_joins_against_it() {
    // Playtest list 5 item 35: the wolf (aggression 2, Raiders) would attack the helper
    // (aggression 0, Average confidence, Rats), who would not attack it; seeing it fight
    // the player, the helper takes the wolf on at once.
    let f = fixture();
    let mut g = game(&f);
    let (wolf, loafer) = (FormId(LIST_REF), FormId(HELPER_REF));
    assert!(g.would_attack(wolf, loafer) && !g.would_attack(loafer, wolf));
    fighting(&mut g, LIST_REF, 0x14);
    assert_eq!(g.call_allies(), 0, "not seen yet");
    g.detection.pairs.insert((loafer, wolf), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, g.time));
    assert_eq!(g.call_allies(), 1);
    assert!(g.would_attack(loafer, wolf));
    assert!(g.awareness.get(loafer).is_some_and(|a| a.alarms.iter().any(|x| x.owner == wolf && x.cause == Cause::Noticed)));
    assert_eq!(g.call_allies(), 0, "once");
}

#[test]
fn an_actor_hit_by_another_fights_it_but_not_an_ally() {
    // NPC against NPC (combat-ai.md §12.4): the leader (Raiders) is neutral to the helper
    // (Rats); hit by it, it holds a grudge and fights it. The helper's own ally (Own, a
    // rat too) lets the stray hit pass.
    let f = fixture();
    let mut g = game(&f);
    let (leader, own, helper) = (FormId(LEADER_REF), FormId(OWN_REF), FormId(HELPER_REF));
    assert!(!g.would_attack(leader, helper));
    assert!(g.attacked_by(leader, helper));
    assert!(g.would_attack(leader, helper));
    assert!(!g.attacked_by(own, helper));
    assert!(!g.would_attack(own, helper));
    let notes = g.provocations.take_notes();
    assert!(notes.iter().any(|n| matches!(n, Note::Assaulted { actor, by, .. } if *actor == leader && *by == helper)));
    assert!(notes.iter().any(|n| matches!(n, Note::Ignored { why: "a stray hit by an ally or friend", .. })));
    // Both fighting someone else (the player): a stray hit is ignored.
    let f2 = fixture();
    let mut g2 = game(&f2);
    fighting(&mut g2, LEADER_REF, PLAYER);
    fighting(&mut g2, HELPER_REF, PLAYER);
    assert!(!g2.attacked_by(leader, helper));
}

#[test]
fn an_actor_that_starts_fighting_alarms_its_allies_near_it() {
    // Alarms (combat-ai.md §12.3): Own starts fighting the player; the helper, its ally
    // that Helps Allies, 1,000 units off, is alarmed (Cause::Ally) and takes up the fight;
    // a second rat 3,000 units off is beyond ALARM_RADIUS.
    use fnv_game::awareness::Change;
    use fnv_game::combat_group::{GroupNote, ALARM_RADIUS};
    let f = fixture();
    let mut g = game(&f);
    let (own, helper, far, player) = (FormId(OWN_REF), FormId(HELPER_REF), FormId(SPAWN_HIGH_REF), FormId::PLAYER_REF);
    only_near(&mut g, &[OWN_REF, HELPER_REF, SPAWN_HIGH_REF]);
    assert!(g.attacked_by(own, player));
    fighting(&mut g, OWN_REF, PLAYER);
    let now = g.time;
    g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(player), cause: None, at: Some([500.0, 0.0, 0.0]), value: Some(5.0) });
    let near: std::collections::BTreeMap<FormId, [f32; 3]> =
        [(own, [0.0, 0.0, 0.0]), (helper, [1000.0, 0.0, 0.0]), (far, [ALARM_RADIUS + 1000.0, 0.0, 0.0]), (player, [500.0, 0.0, 0.0])].into_iter().collect();
    assert_eq!(g.raise_alarms(&near), 1);
    assert!(g.awareness.get(helper).is_some_and(|a| a.alarms.iter().any(|x| x.owner == player && x.cause == Cause::Ally)));
    assert!(g.would_attack(helper, player));
    assert!(g.awareness.get(far).is_none());
    assert!(g.awareness.tactics.take_notes().iter().any(|n| matches!(n, GroupNote::Alarm { receiver, combat: true, .. } if *receiver == helper)));
    // The helper's own change (cause Ally) is not passed on again.
    g.awareness.changes.clear();
    g.settle_awareness();
    assert_eq!(g.awareness.state(helper), State::Alert);
    assert_eq!(g.raise_alarms(&near), 0);
}

#[test]
fn an_ally_on_a_package_without_observe_combat_takes_no_alarm() {
    // HELIOS One's stasis protectrons (#1977, observed 2026-11-10): on ProtectronNotActive,
    // which skips Fallout behaviour with Observe Combat Behavior clear, four stayed on
    // their pods while the fifth fought. The helper, on such a package, is not alarmed.
    use fnv_game::awareness::Change;
    let f = fixture();
    let mut g = game(&f);
    let (own, helper, player) = (FormId(OWN_REF), FormId(HELPER_REF), FormId::PLAYER_REF);
    only_near(&mut g, &[OWN_REF, HELPER_REF]);
    call(&mut g, ADD_SCRIPT_PACKAGE, HELPER_REF, &[form(PACK_STILL)]);
    assert!(!g.package_allows(helper, fnv_game::package::fallout::OBSERVE_COMBAT));
    assert!(g.attacked_by(own, player));
    fighting(&mut g, OWN_REF, PLAYER);
    let now = g.time;
    g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(player), cause: None, at: Some([500.0, 0.0, 0.0]), value: Some(5.0) });
    let near: std::collections::BTreeMap<FormId, [f32; 3]> = [(own, [0.0, 0.0, 0.0]), (helper, [1000.0, 0.0, 0.0]), (player, [500.0, 0.0, 0.0])].into_iter().collect();
    assert_eq!(g.raise_alarms(&near), 0);
    assert!(g.awareness.get(helper).is_none_or(|a| a.alarms.is_empty()));
}

#[test]
fn an_alarm_reaches_far_allies_and_those_not_measured() {
    // #972 (docs/spec-notes/combat-groups.md §1): the helper 6,000 units off, beyond the
    // old 4,096, hears Own's alarm; so does it when the round did not measure it (not in
    // `near`), from where the world has it.
    use fnv_game::awareness::Change;
    let (own, helper, player) = (FormId(OWN_REF), FormId(HELPER_REF), FormId::PLAYER_REF);
    for measured in [true, false] {
        let f = fixture();
        let mut g = game(&f);
        only_near(&mut g, &[OWN_REF, HELPER_REF]);
        g.world.get(helper).unwrap().pos = [6000.0, 0.0, 0.0];
        assert!(g.attacked_by(own, player));
        fighting(&mut g, OWN_REF, PLAYER);
        let now = g.time;
        g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(player), cause: None, at: Some([500.0, 0.0, 0.0]), value: Some(5.0) });
        let mut near: std::collections::BTreeMap<FormId, [f32; 3]> = [(own, [0.0; 3]), (player, [500.0, 0.0, 0.0])].into_iter().collect();
        if measured {
            near.insert(helper, [6000.0, 0.0, 0.0]);
        }
        assert_eq!(g.raise_alarms(&near), 1, "measured: {measured}");
        assert!(g.awareness.get(helper).is_some_and(|a| a.alarms.iter().any(|x| x.owner == player && x.cause == Cause::Ally)));
    }
}

#[test]
fn a_joiner_is_called_against_the_first_of_the_foes_to_start() {
    // #972 (combat-groups.md §2): the leader (Raiders) started fighting Own first, the
    // wolf (Raiders too) later; Own, fighting the wolf, alarms the helper, who is called
    // against the leader, as the Goodsprings townsfolk all named GSPG03Ref.
    use fnv_game::awareness::Change;
    let f = fixture();
    let mut g = game(&f);
    let (leader, wolf, own, helper) = (FormId(LEADER_REF), FormId(LIST_REF), FormId(OWN_REF), FormId(HELPER_REF));
    only_near(&mut g, &[LEADER_REF, LIST_REF, OWN_REF, HELPER_REF]);
    assert!(g.comrades(leader, wolf));
    fighting(&mut g, LEADER_REF, OWN_REF);
    fighting(&mut g, LIST_REF, OWN_REF);
    g.awareness.tactics.started.insert(leader, 1.0);
    g.awareness.tactics.started.insert(wolf, 2.0);
    assert_eq!(g.first_starter(wolf), leader);
    assert_eq!(g.first_starter(FormId::PLAYER_REF), FormId::PLAYER_REF, "nobody of its side fighting: itself");
    fighting(&mut g, OWN_REF, LIST_REF);
    let now = g.time;
    g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(wolf), cause: None, at: Some([500.0, 0.0, 0.0]), value: Some(5.0) });
    let near: std::collections::BTreeMap<FormId, [f32; 3]> = [(own, [0.0; 3]), (helper, [1000.0, 0.0, 0.0]), (wolf, [500.0, 0.0, 0.0]), (leader, [800.0, 0.0, 0.0])].into_iter().collect();
    assert!(!g.would_attack(helper, wolf) && !g.would_attack(helper, leader), "the helper (Rats) is neutral to the Raiders");
    assert_eq!(g.raise_alarms(&near), 1);
    let a = g.awareness.get(helper).expect("alarmed");
    assert!(a.alarms.iter().any(|x| x.owner == leader && x.cause == Cause::Ally && x.at == [800.0, 0.0, 0.0]), "{:?}", a.alarms);
    assert!(g.would_attack(helper, leader));
    // The grudge goes against the foe it was called about too: with the starter dead,
    // the helper can still fight the wolf in front of it.
    assert!(g.would_attack(helper, wolf), "a grudge against the wolf it was called about");
    g.extra.entry(leader).or_default().dead = true;
    assert!(g.would_attack(helper, wolf));
}

#[test]
fn a_starter_out_cold_is_passed_over() {
    // #972 review: a knocked-out (or dead) first starter is not fighting; the next of its
    // side to start is named, or the foe itself when none is left.
    use fnv_game::awareness::Change;
    let f = fixture();
    let mut g = game(&f);
    let (leader, wolf, own, helper) = (FormId(LEADER_REF), FormId(LIST_REF), FormId(OWN_REF), FormId(HELPER_REF));
    only_near(&mut g, &[LEADER_REF, LIST_REF, OWN_REF, HELPER_REF]);
    fighting(&mut g, LEADER_REF, OWN_REF);
    fighting(&mut g, LIST_REF, OWN_REF);
    g.awareness.tactics.started.insert(leader, 1.0);
    g.awareness.tactics.started.insert(wolf, 2.0);
    assert_eq!(g.first_starter(wolf), leader);
    assert!(g.knock_out(leader, fnv_game::unconscious::Cause::Fatigue, Some(own)));
    assert_eq!(g.first_starter(wolf), wolf, "the leader lies out cold: the wolf is the first still fighting");
    fighting(&mut g, OWN_REF, LIST_REF);
    let now = g.time;
    g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(wolf), cause: None, at: Some([500.0, 0.0, 0.0]), value: Some(5.0) });
    let near: std::collections::BTreeMap<FormId, [f32; 3]> = [(own, [0.0; 3]), (helper, [1000.0, 0.0, 0.0]), (wolf, [500.0, 0.0, 0.0]), (leader, [800.0, 0.0, 0.0])].into_iter().collect();
    assert_eq!(g.raise_alarms(&near), 1);
    let a = g.awareness.get(helper).expect("alarmed");
    assert!(a.alarms.iter().any(|x| x.owner == wolf && x.at == [500.0, 0.0, 0.0]), "{:?}", a.alarms);
    assert!(!a.alarms.iter().any(|x| x.owner == leader), "{:?}", a.alarms);
}

#[test]
fn a_fighter_keeps_a_target_its_group_still_detects() {
    // #972 (combat-groups.md §3): Own and the helper (both rats) fight the player; the
    // helper has not detected it for longer than the lost and forget timers, but Own
    // still does: the helper stays in Combat, keeps it and learns where Own sees it. When
    // nobody detects it any more both go Lost after the lost timer.
    let f = fixture();
    let mut g = game(&f);
    let (own, helper, player) = (FormId(OWN_REF), FormId(HELPER_REF), FormId::PLAYER_REF);
    let t = Timers::default();
    fighting(&mut g, OWN_REF, PLAYER);
    fighting(&mut g, HELPER_REF, PLAYER);
    let hidden = Detection { value: -40.0, band: Band::Hidden, ..Default::default() };
    g.awareness.actors.get_mut(&helper).unwrap().observe(player, &hidden, [0.0; 3], g.time, &t);
    let start = g.time;
    let rounds = (t.lost.max(t.forget_target) * 2.0).ceil() as u32 + 2;
    for k in 1..=rounds {
        g.time = start + k as f64 * 0.5;
        let now = g.time;
        g.awareness.actors.get_mut(&own).unwrap().observe(player, &Detection { value: 5.0, band: Band::Detected, ..Default::default() }, [k as f32, 0.0, 0.0], now, &t);
        g.awareness.actors.get_mut(&helper).unwrap().observe(player, &hidden, [0.0; 3], now, &t);
        g.settle_awareness();
    }
    assert!(g.time - start > t.lost.max(t.forget_target));
    let a = g.awareness.get(helper).expect("still fighting");
    assert_eq!(a.state, State::Combat);
    assert_eq!(a.targets.get(&player).map(|x| x.at), Some([rounds as f32, 0.0, 0.0]), "where Own saw it");
    assert_eq!(a.shared.get(&player), Some(&g.time));
    // Nobody detects it: Lost together, the lost timer after Own's last sighting.
    let seen = g.time;
    let mut lost_at = std::collections::BTreeMap::new();
    for k in 1..=30 {
        g.time = seen + k as f64 * 0.5;
        let now = g.time;
        for x in [own, helper] {
            if let Some(a) = g.awareness.actors.get_mut(&x) {
                a.observe(player, &hidden, [0.0; 3], now, &t);
            }
        }
        g.settle_awareness();
        for x in [own, helper] {
            if g.awareness.state(x) == State::Lost {
                lost_at.entry(x).or_insert(now - seen);
            }
        }
    }
    assert_eq!(lost_at.get(&own), Some(&t.lost));
    assert_eq!(lost_at.get(&helper), Some(&t.lost));
}

#[test]
fn a_search_party_shares_the_search_and_gives_up_after_its_searches() {
    // Combat Search (combat-ai.md §12.5): Own and the helper search for the player
    // together; each has its place in the party; after its complete searches
    // (iCombatTargetLostRemoveSearchCount, 1 without the game setting) it drops the alarm.
    let f = fixture();
    let mut g = game(&f);
    let (own, helper, player) = (FormId(OWN_REF), FormId(HELPER_REF), FormId::PLAYER_REF);
    let t = Timers::default();
    for a in [own, helper] {
        let x = g.awareness.actors.entry(a).or_default();
        x.alarm(player, [0.0; 3], -5.0, Cause::Event(fnv_game::awareness::EventType::Bullet), 0.0, &t);
        x.state = State::Alert;
    }
    assert_eq!(g.search_party(own, player), (0, 2));
    assert_eq!(g.search_party(helper, player), (1, 2));
    assert!(g.searched_once(own, player));
    assert!(g.awareness.get(own).is_some_and(|a| a.alarms.is_empty()));
    assert_eq!(g.search_party(helper, player), (0, 1));
}

#[test]
fn a_shared_faction_does_not_make_enemies_kin() {
    // combat-ai.md §12.4: the bandit and the watchman share the Idlers faction, but the
    // raiders name the player's faction an enemy: they are not kin, and the (aggressive)
    // bandit attacks the watchman on sight. The loafer, an idler only, is kin to both.
    let f = fixture();
    let mut g = game(&f);
    let (bandit, watchman, loafer) = (FormId(BANDIT_REF), FormId(WATCHMAN_REF), FormId(LOAFER_REF));
    assert!(!g.kin(bandit, watchman) && !g.kin(watchman, bandit));
    assert!(g.would_attack(bandit, watchman));
    assert!(g.kin(bandit, loafer) && g.kin(watchman, loafer));
    assert!(!g.would_attack(bandit, loafer));
    assert!(!g.comrades(bandit, watchman) && g.comrades(bandit, loafer));
    // Two gangs neutral to each other, both idlers (the NCR and the Fiends in
    // GeneralIdleFaction): not kin, so the very aggressive one attacks the other.
    let (a, b) = (FormId(GANGER_A_REF), FormId(GANGER_B_REF));
    assert!(!g.kin(a, b));
    assert!(g.would_attack(a, b) && !g.would_attack(b, a));
}

#[test]
fn a_teammate_reacts_as_if_in_the_player_faction() {
    // docs/design/followers.md §3 [documented: GECK wiki SetPlayerTeammate]. The loafer
    // (an idler, kin to the bandit) is made the player's teammate: the bandit, a raider
    // that would attack the player, now attacks it, and it attacks the bandit; it never
    // attacks the player, and leaves alone whoever leaves the player alone.
    let f = fixture();
    let mut g = game(&f);
    let (bandit, loafer, own, player) = (FormId(BANDIT_REF), FormId(LOAFER_REF), FormId(OWN_REF), FormId::PLAYER_REF);
    assert!(g.would_attack(bandit, player));
    assert!(!g.would_attack(bandit, loafer) && !g.would_attack(loafer, bandit));
    g.extra.entry(loafer).or_default().teammate = true;
    assert!(g.is_teammate(loafer) && g.teammates() == vec![loafer]);
    assert!(g.would_attack(bandit, loafer) && g.would_attack(loafer, bandit));
    assert!(!g.would_attack(loafer, player) && !g.would_attack(loafer, own) && !g.would_attack(own, loafer));
    // A neutral the player hits holds a grudge against the player: the teammate's foe.
    assert!(g.attacked_by(own, player));
    assert!(g.would_attack(loafer, own) && g.would_attack(own, loafer));
    // Its fights: only once it perceives the player, an alarm at each actor fighting it.
    fighting(&mut g, OWN_REF, PLAYER);
    assert_eq!(g.teammates_assist(), 0);
    g.detection.pairs.insert((loafer, player), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, g.time));
    assert_eq!(g.teammates_assist(), 1);
    assert!(g.awareness.get(loafer).is_some_and(|a| a.alarms.iter().any(|x| x.owner == own && x.cause == Cause::Ally)));
    assert!(g.provocations.take_notes().iter().any(|n| matches!(n, Note::Assists { helper, victim, attacker } if *helper == loafer && *victim == player && *attacker == own)));
    assert_eq!(g.teammates_assist(), 0, "once");
    // Friendly fire: an ally of the player; with SetIgnoreFriendlyHits every hit passes.
    g.extra.entry(loafer).or_default().ignore_friendly_hits = true;
    for _ in 0..10 {
        g.time += 1.0;
        assert!(!g.attacked_by(loafer, player));
    }
    assert!(!g.would_attack(loafer, player));
}

#[test]
fn a_teammate_takes_up_only_the_fights_within_its_reach() {
    // #3456: a foe of the player's in another space, or in the same one but beyond
    // `ALARM_RADIUS` of both the teammate and the player, gives the teammate no alarm; a
    // near one does, near the teammate or near the player only.
    let f = fixture();
    let (loafer, own, player) = (FormId(LOAFER_REF), FormId(OWN_REF), FormId::PLAYER_REF);
    let set_up = |g: &mut Game, foe_cell: FormId, foe_at: [f32; 3], mate_at: [f32; 3]| {
        g.extra.entry(loafer).or_default().teammate = true;
        assert!(g.attacked_by(own, player));
        fighting(g, OWN_REF, PLAYER);
        g.detection.pairs.insert((loafer, player), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, g.time));
        let p = g.world.get(player).unwrap();
        (p.cell, p.pos) = (Some(FormId(CELL)), [0.0; 3]);
        g.world.get(loafer).unwrap().pos = mate_at;
        let s = g.world.get(own).unwrap();
        (s.cell, s.pos) = (Some(foe_cell), foe_at);
    };
    let alarmed = |g: &Game| g.awareness.get(loafer).is_some_and(|a| a.alarms.iter().any(|x| x.owner == own && x.cause == Cause::Ally));
    // In another space (a cell of no known worldspace is a space of its own), right beside it.
    let mut g = game(&f);
    set_up(&mut g, FormId(0x00BA_DCE1), [100.0, 0.0, 0.0], [0.0; 3]);
    assert_eq!(g.teammates_assist(), 0, "another space");
    assert!(!alarmed(&g));
    // In the same space, far from both.
    let mut g = game(&f);
    set_up(&mut g, FormId(CELL), [50_000.0, 0.0, 0.0], [0.0; 3]);
    assert_eq!(g.teammates_assist(), 0, "far away");
    assert!(!alarmed(&g));
    // Near the teammate and the player.
    let mut g = game(&f);
    set_up(&mut g, FormId(CELL), [500.0, 0.0, 0.0], [0.0; 3]);
    assert_eq!(g.teammates_assist(), 1, "near");
    assert!(alarmed(&g));
    // Near the player, the teammate left behind far off in the same space.
    let mut g = game(&f);
    set_up(&mut g, FormId(CELL), [500.0, 0.0, 0.0], [30_000.0, 0.0, 0.0]);
    assert_eq!(g.teammates_assist(), 1, "near the player");
    // Near the teammate, the player far off.
    let mut g = game(&f);
    set_up(&mut g, FormId(CELL), [30_500.0, 0.0, 0.0], [30_000.0, 0.0, 0.0]);
    assert_eq!(g.teammates_assist(), 1, "near the teammate");
}

#[test]
fn a_teammate_does_not_go_for_a_foe_left_behind_for_good() {
    // #3390: the foe fighting the player is measured no more (its cell unloaded, here within
    // `ALARM_RADIUS` in the same space). It loses the player and calms down on the timers,
    // and with it the alarms that send the teammate after it end.
    let f = fixture();
    let mut g = game(&f);
    let (loafer, own, player) = (FormId(LOAFER_REF), FormId(OWN_REF), FormId::PLAYER_REF);
    g.extra.entry(loafer).or_default().teammate = true;
    assert!(g.attacked_by(own, player));
    fighting(&mut g, OWN_REF, PLAYER);
    g.detection.measures_player = true;
    let p = g.world.get(player).unwrap();
    (p.cell, p.pos) = (Some(FormId(CELL)), [0.0; 3]);
    g.world.get(loafer).unwrap().pos = [0.0; 3];
    let s = g.world.get(own).unwrap();
    (s.cell, s.pos) = (Some(FormId(CELL)), [9000.0, 0.0, 0.0]);
    let round = |g: &mut Game, dt: f64| {
        g.time += dt;
        g.detection.record(loafer, player, Detection { value: 5.0, band: Band::Detected, ..Default::default() }, g.time);
        g.detection.forget_older_than(g.time - 1.0);
        g.lose_unmeasured(&[loafer].into_iter().collect());
        let given = g.teammates_assist();
        g.settle_awareness();
        given
    };
    assert_eq!(round(&mut g, 0.0), 1, "the fight is the teammate's too");
    for _ in 0..80 {
        round(&mut g, 1.0);
    }
    assert_eq!(g.awareness.state(own), State::Normal);
    assert_eq!(g.awareness.state(loafer), State::Normal);
    assert!(g.awareness.get(loafer).is_none_or(|a| a.search_alarm().is_none()), "nothing to head for");
    assert_eq!(round(&mut g, 1.0), 0);
}

#[test]
fn an_escort_joins_its_charges_fights() {
    // packages.md §3.1 [documented: GECK wiki Defensive Combat, "if the target is
    // attacked, the actor will join into the combat"]. The helper (a rat, no enemy of the
    // bandit) escorts the player; the bandit attacks the player.
    use fnv_game::package::{PackageDef, PackageType, Target, TargetKind};
    const ESCORT_PACK: u32 = 0x8F0;
    let mut f = fixture();
    let def = PackageDef { ty: PackageType::Escort, target: Some(Target { kind: TargetKind::Reference(Some(FormId::PLAYER_REF)), count_distance: 0 }), ..Default::default() };
    f.data.package_defs.insert(FormId(ESCORT_PACK), def);
    let mut g = game(&f);
    let (bandit, escort, watchman, player) = (FormId(BANDIT_REF), FormId(HELPER_REF), FormId(WATCHMAN_REF), FormId::PLAYER_REF);
    assert!(!g.would_attack(escort, bandit));
    g.extra.entry(escort).or_default().current_package = Some(FormId(ESCORT_PACK));
    fighting(&mut g, BANDIT_REF, PLAYER);
    // Only once it at least notices its charge (hidden: no help).
    assert_eq!(g.escorts_assist(), 0);
    g.detection.pairs.insert((escort, player), (Detection { value: -30.0, band: Band::Hidden, ..Default::default() }, g.time));
    assert_eq!(g.escorts_assist(), 0);
    g.detection.pairs.insert((escort, player), (Detection { value: -10.0, band: Band::Noticed, ..Default::default() }, g.time));
    assert_eq!(g.escorts_assist(), 1);
    assert!(g.would_attack(escort, bandit), "it holds a grudge against the attacker");
    assert!(g.awareness.get(escort).is_some_and(|a| a.alarms.iter().any(|x| x.owner == bandit && x.cause == Cause::Ally)));
    assert!(g.provocations.take_notes().iter().any(|n| matches!(n, Note::Assists { helper, victim, attacker } if *helper == escort && *victim == player && *attacker == bandit)));
    assert_eq!(g.escorts_assist(), 0, "once");
    // A fight its charge starts is not joined until the other fights back (see crates/fnv-game/src/combat.rs):
    // the player fighting the watchman (which does not fight back) brings nothing.
    fighting(&mut g, PLAYER, WATCHMAN_REF);
    assert_eq!(g.escorts_assist(), 0);
    assert!(!g.provocations.holds(escort, watchman));
    // Another package: no help.
    g.extra.entry(escort).or_default().current_package = None;
    fighting(&mut g, WATCHMAN_REF, PLAYER);
    assert_eq!(g.escorts_assist(), 0);
}

#[test]
fn a_defensive_follower_joins_its_targets_fights_and_starts_none() {
    // #2793, packages.md §3.1 [documented: GECK wiki Category:Packages, Defensive Combat,
    // "The exception is for the Accompany, Escort and Follow packages. For these three, if
    // the target is attacked, the actor will join into the combat"]. Ganger A (Very
    // Aggressive, neutral to the rats) follows the loafer, its kin, on a Defensive Combat
    // package: it leaves the helper (a rat) alone on sight, and takes up the loafer's
    // fight once the rat attacks it. The same on an Accompany package; on a Travel
    // package (the default type) it does not.
    use fnv_game::package::{flags, PackageDef, PackageType, Target, TargetKind};
    const FOLLOW_PACK: u32 = 0x8F2;
    const ACCOMPANY_PACK: u32 = 0x8F3;
    const TRAVEL_PACK: u32 = 0x8F4;
    let mut f = fixture();
    let charge = Some(Target { kind: TargetKind::Reference(Some(FormId(LOAFER_REF))), count_distance: 0 });
    for (pack, ty) in [(FOLLOW_PACK, PackageType::Follow), (ACCOMPANY_PACK, PackageType::Accompany), (TRAVEL_PACK, PackageType::Travel)] {
        f.data.package_defs.insert(FormId(pack), PackageDef { ty, target: charge, flags: flags::DEFENSIVE_COMBAT, ..Default::default() });
    }
    // The ganger's AI data says Cowardly (the fixture's `aidt`), and a coward stays out:
    // Average here.
    const SET_AV: u16 = 0x100F;
    let average = |g: &mut Game| call(g, SET_AV, GANGER_A_REF, &[ArgValue::Enum(av::CONFIDENCE), ArgValue::Value(Value::Num(2.0))]);
    for pack in [FOLLOW_PACK, ACCOMPANY_PACK] {
        let mut g = game(&f);
        average(&mut g);
        let (ganger, loafer, rat) = (FormId(GANGER_A_REF), FormId(LOAFER_REF), FormId(HELPER_REF));
        assert!(g.would_attack(ganger, rat), "Very Aggressive: without the package it attacks the neutral rat");
        g.extra.entry(ganger).or_default().current_package = Some(FormId(pack));
        assert!(!g.would_attack(ganger, rat), "Defensive Combat: no attack on sight");
        g.detection.pairs.insert((ganger, loafer), (Detection { value: -10.0, band: Band::Noticed, ..Default::default() }, g.time));
        // Nobody attacks the loafer: nothing to join.
        fighting(&mut g, HELPER_REF, OWN_REF);
        assert_eq!(g.escorts_assist(), 0);
        assert!(!g.would_attack(ganger, rat));
        // The rat attacks the loafer: the follower takes the fight up.
        fighting(&mut g, HELPER_REF, LOAFER_REF);
        assert_eq!(g.escorts_assist(), 1);
        assert!(g.would_attack(ganger, rat), "it holds a grudge against the attacker");
        assert!(g.awareness.get(ganger).is_some_and(|a| a.alarms.iter().any(|x| x.owner == rat && x.cause == Cause::Ally)));
        assert!(g.provocations.take_notes().iter().any(|n| matches!(n, Note::Assists { helper, victim, attacker } if *helper == ganger && *victim == loafer && *attacker == rat)));
        assert_eq!(g.escorts_assist(), 0, "once");
    }
    let mut g = game(&f);
    average(&mut g);
    g.extra.entry(FormId(GANGER_A_REF)).or_default().current_package = Some(FormId(TRAVEL_PACK));
    g.detection.pairs.insert((FormId(GANGER_A_REF), FormId(LOAFER_REF)), (Detection { value: -10.0, band: Band::Noticed, ..Default::default() }, g.time));
    fighting(&mut g, HELPER_REF, LOAFER_REF);
    assert_eq!(g.escorts_assist(), 0);
}

#[test]
fn companions_seeing_the_player_do_not_set_the_sneak_word() {
    // #1159 (hud.md §9): a teammate, or an actor following the player, detecting the
    // player is no [DETECTED]; anyone else still is, and the pair stays for the AI.
    use fnv_game::package::{PackageDef, PackageType, Target, TargetKind};
    const FOLLOW_PACK: u32 = 0x8F1;
    let mut f = fixture();
    let def = PackageDef { ty: PackageType::Follow, target: Some(Target { kind: TargetKind::Reference(Some(FormId::PLAYER_REF)), count_distance: 0 }), ..Default::default() };
    f.data.package_defs.insert(FormId(FOLLOW_PACK), def);
    let mut g = game(&f);
    let (mate, follower, other, player) = (FormId(LOAFER_REF), FormId(HELPER_REF), FormId(WATCHMAN_REF), FormId::PLAYER_REF);
    let seen = |g: &mut Game, who: FormId| {
        let t = g.time;
        g.detection.pairs.insert((who, player), (Detection { value: 30.0, band: Band::Detected, ..Default::default() }, t));
    };
    assert!(!g.player_detected_for_sneak(), "no one measured");
    g.extra.entry(mate).or_default().teammate = true;
    seen(&mut g, mate);
    assert!(g.with_player(mate));
    assert!(!g.player_detected_for_sneak(), "the teammate alone: hidden");
    assert!(g.detection.get(mate, player).is_some(), "its own detection is kept");
    g.extra.entry(follower).or_default().current_package = Some(FormId(FOLLOW_PACK));
    seen(&mut g, follower);
    assert!(g.with_player(follower));
    assert!(!g.player_detected_for_sneak(), "a follower too");
    // A non-hostile actor that is not with the player still counts (hud.md §9).
    assert!(!g.with_player(other));
    seen(&mut g, other);
    assert!(g.player_detected_for_sneak());
    // A follower on another package is no longer with the player.
    g.detection.pairs.remove(&(other, player));
    g.extra.entry(follower).or_default().current_package = None;
    assert!(!g.with_player(follower));
    assert!(g.player_detected_for_sneak());
}

#[test]
fn open_teammate_container_asks_for_the_teammates_inventory() {
    // [documented: GECK wiki OpenTeammateContainer] only on teammates unless the flag is 1.
    const OPEN_TEAMMATE_CONTAINER: u16 = 0x11C8;
    let f = fixture();
    let mut g = game(&f);
    call(&mut g, OPEN_TEAMMATE_CONTAINER, LOAFER_REF, &[]);
    assert!(g.teammate_containers.is_empty());
    call(&mut g, OPEN_TEAMMATE_CONTAINER, LOAFER_REF, &[ArgValue::Value(Value::Num(1.0))]);
    assert_eq!(g.teammate_containers, vec![FormId(LOAFER_REF)]);
    g.teammate_containers.clear();
    g.extra.entry(FormId(LOAFER_REF)).or_default().teammate = true;
    call(&mut g, OPEN_TEAMMATE_CONTAINER, LOAFER_REF, &[]);
    assert_eq!(g.teammate_containers, vec![FormId(LOAFER_REF)]);
}

#[test]
fn a_cowardly_ally_stays_out_of_fights() {
    // Playtest list 4, item 14 (combat-ai.md §13): the coward is the victim's ally and
    // Helps Allies, but its confidence is Cowardly: it neither joins by assistance nor
    // answers an alarm; the helper (Average) does.
    use fnv_game::awareness::Change;
    let f = fixture();
    let mut g = game(&f);
    let (own, coward, helper, player) = (FormId(OWN_REF), FormId(COWARD_REF), FormId(HELPER_REF), FormId::PLAYER_REF);
    only_near(&mut g, &[OWN_REF, COWARD_REF, HELPER_REF]);
    assert!(g.attacked_by(own, player));
    fighting(&mut g, OWN_REF, PLAYER);
    for w in [coward, helper] {
        g.detection.pairs.insert((w, own), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, g.time));
    }
    assert_eq!(g.call_allies(), 1);
    assert!(g.would_attack(helper, player) && !g.would_attack(coward, player));
    let notes = g.provocations.take_notes();
    assert_eq!(notes.iter().filter(|n| matches!(n, Note::StaysOut { helper, .. } if *helper == coward)).count(), 1);
    // Noted once.
    assert_eq!(g.call_allies(), 0);
    assert!(!g.provocations.take_notes().iter().any(|n| matches!(n, Note::StaysOut { .. })));
    // An alarm does not bring it either.
    let now = g.time;
    g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(player), cause: None, at: Some([500.0, 0.0, 0.0]), value: Some(5.0) });
    let near: std::collections::BTreeMap<FormId, [f32; 3]> = [(own, [0.0; 3]), (coward, [300.0, 0.0, 0.0]), (player, [500.0, 0.0, 0.0])].into_iter().collect();
    assert_eq!(g.raise_alarms(&near), 0);
    assert!(!g.would_attack(coward, player));
    // In a fight of its own it flees from the start, hurt or not (#2520), until its
    // confidence is raised at a flee spot.
    assert!(g.always_flees(coward) && !g.always_flees(helper));
    assert!(g.flee_check(coward, 0.0).flees);
    g.boost_confidence(coward);
    assert!(!g.always_flees(coward));
}

#[test]
fn allies_stay_out_of_a_scripted_fight_the_player_did_not_provoke() {
    // #2520 (`fnv_game::provocation`, "Assistance"): the Silver Rush customer's
    // `startcombat player` drew Jean-Baptiste on the player. A StartCombat on a player the
    // actor would leave alone draws no ally and sends no alarm; once the player hits it,
    // its allies help as before.
    use fnv_game::awareness::Change;
    let f = fixture();
    let mut g = game(&f);
    let (own, helper, player) = (FormId(OWN_REF), FormId(HELPER_REF), FormId::PLAYER_REF);
    only_near(&mut g, &[OWN_REF, HELPER_REF]);
    assert!(!g.would_attack(own, player) && !g.would_attack(helper, player));
    g.start_combat(own, player);
    assert!(g.awareness.state(own).in_combat() && g.would_attack(own, player));
    assert!(g.unprovoked_scripted_fight(own, player));
    g.detection.pairs.insert((helper, own), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, g.time));
    assert_eq!(g.call_allies(), 0);
    assert!(!g.would_attack(helper, player));
    let notes = g.provocations.take_notes();
    assert_eq!(notes.iter().filter(|n| matches!(n, Note::Unprovoked { victim, .. } if *victim == own)).count(), 1);
    // No alarm either.
    let now = g.time;
    g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(player), cause: None, at: Some([500.0, 0.0, 0.0]), value: Some(5.0) });
    let near: std::collections::BTreeMap<FormId, [f32; 3]> = [(own, [0.0; 3]), (helper, [300.0, 0.0, 0.0]), (player, [500.0, 0.0, 0.0])].into_iter().collect();
    assert_eq!(g.raise_alarms(&near), 0);
    assert!(!g.would_attack(helper, player));
    // Noted once.
    assert_eq!(g.call_allies(), 0);
    assert!(!g.provocations.take_notes().iter().any(|n| matches!(n, Note::Unprovoked { .. })));
    // The player hits it: now its ally takes the fight up.
    g.attacked_by(own, player);
    assert!(g.hurt_by(own, player, [500.0, 0.0, 0.0]));
    assert!(!g.unprovoked_scripted_fight(own, player));
    assert_eq!(g.call_allies(), 1);
    assert!(g.would_attack(helper, player));
}

#[test]
fn an_actor_hostile_by_its_records_is_helped_after_a_start_combat_too() {
    // #2520: the rule leaves alone a fight the actor would have started anyway.
    let f = fixture();
    let mut g = game(&f);
    let (own, player) = (FormId(OWN_REF), FormId::PLAYER_REF);
    assert!(g.attacked_by(own, player), "a grudge");
    g.start_combat(own, player);
    assert!(!g.unprovoked_scripted_fight(own, player));
    // Not the player: an NPC's scripted fight is not this rule's.
    g.start_combat(own, FormId(HELPER_REF));
    assert!(!g.unprovoked_scripted_fight(own, FormId(HELPER_REF)));
}

#[test]
fn an_actor_warns_then_attacks_a_player_inside_its_aggro_radius() {
    // Playtest list 4, item 18 (`fnv_game::aggro`): unaggressive and neutral to the
    // player, Aggro Radius Behavior on, radius 650. Two warnings 5 s apart (iGuardWarnings,
    // fAITrespassWarningTimer), then it attacks; outside the radius nothing happens.
    use fnv_game::provocation::Why;
    let f = fixture();
    let mut g = game(&f);
    let (lurker, player) = (FormId(LURKER_REF), FormId::PLAYER_REF);
    let d = &f.data.actors[&FormId(LURKER)];
    assert!(d.aggro_radius_behavior && d.aggro_radius == 650);
    assert_eq!(g.aggro_radius(lurker), Some(650.0));
    assert_eq!(g.aggro_radius(FormId(OWN_REF)), None);
    g.world.get(player).unwrap().pos = [0.0; 3];
    g.world.get(lurker).unwrap().pos = [700.0, 0.0, 0.0];
    let seen = |g: &mut Game| {
        let now = g.time;
        g.detection.pairs.insert((lurker, player), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, now));
    };
    assert!(!g.would_attack(lurker, player));
    seen(&mut g);
    assert_eq!(g.aggro_tick(), 0);
    assert!(g.provocations.take_notes().is_empty(), "outside the radius");
    assert_eq!(g.warning_to(lurker), None);
    assert!(!g.draws_weapon(lurker));
    g.world.get(lurker).unwrap().pos = [600.0, 0.0, 0.0];
    for (level, step) in [(1, 0.0), (2, 5.0)] {
        g.time += step;
        seen(&mut g);
        assert_eq!(g.aggro_tick(), 0);
        assert!(g.provocations.take_notes().iter().any(|n| matches!(n, Note::AggroWarned { actor, level: l, .. } if *actor == lurker && *l == level)));
        // While it warns it draws (a creature its stance; the AI stops it and turns it
        // to the player) but is not alerted: GetIsAlerted 0, as the original's gecko.
        assert_eq!(g.warning_to(lurker), Some(player));
        assert!(g.draws_weapon(lurker) && !g.is_alerted(lurker));
        // GetCurrentAIPackage 35 and GetCurrentAIProcedure 44 (GUARD), GetIsAlerted 0
        // (the original's young gecko, capture `warnings`).
        assert_eq!(call(&mut g, GET_CURRENT_AI_PROCEDURE, LURKER_REF, &[]), 44.0);
        assert_eq!(call(&mut g, GET_CURRENT_AI_PACKAGE, LURKER_REF, &[]), 35.0);
        assert_eq!(call(&mut g, GET_IS_ALERTED, LURKER_REF, &[]), 0.0);
    }
    // Not again before the timer; it still warns meanwhile. The attack comes 15 s after
    // the first warning for a creature, 21 s for a person (the original's geckos and
    // Gomorrah guard): here 10 or 16 s after the second.
    let wait = if g.creature_type(lurker).is_some() { 10.0 } else { 16.0 };
    g.time += 1.0;
    seen(&mut g);
    assert_eq!(g.aggro_tick(), 0);
    assert_eq!(g.warning_to(lurker), Some(player));
    g.time += wait - 2.0;
    seen(&mut g);
    assert_eq!(g.aggro_tick(), 0);
    g.time += 1.0;
    seen(&mut g);
    assert_eq!(g.aggro_tick(), 1);
    assert!(g.provocations.take_notes().iter().any(|n| matches!(n, Note::AggroAttacks { actor, .. } if *actor == lurker)));
    assert!(g.would_attack(lurker, player));
    assert_eq!(g.provocations.grudges[&(lurker, player)].why, Why::Aggro);
    // Until it is in Combat (a moment later) it still counts as warning, so its stance is
    // not put away and drawn again; after AGGRO_TURN_SECONDS no longer.
    assert_eq!(g.warning_to(lurker), Some(player));
    g.time += fnv_game::aggro::AGGRO_TURN_SECONDS;
    assert_eq!(g.warning_to(lurker), None);
}

#[test]
fn a_guard_package_alone_warns_nobody() {
    // `fnv_game::aggro` "Guard packages do not warn": Gomorrah's room guards on a Guard
    // package, with no aggro radius of their own, neither warned nor fought a player by the
    // door they guard (capture `warnings2`). The lurker on a Guard package guarding OWN_REF,
    // the player beside OWN_REF and far outside the lurker's own aggro radius: nothing.
    use fnv_game::package::{PackageDef, PackageType, Target, TargetKind};
    let mut f = fixture();
    let guard = PackageDef { ty: PackageType::Guard, target: Some(Target { kind: TargetKind::Reference(Some(FormId(OWN_REF))), count_distance: 100 }), ..PackageDef::default() };
    f.data.package_defs.insert(FormId(0x8F0), guard);
    let mut g = game(&f);
    let (lurker, player) = (FormId(LURKER_REF), FormId::PLAYER_REF);
    g.extra.entry(lurker).or_default().current_package = Some(FormId(0x8F0));
    g.world.get(player).unwrap().pos = [0.0; 3];
    g.world.get(FormId(OWN_REF)).unwrap().pos = [60.0, 0.0, 0.0];
    g.world.get(lurker).unwrap().pos = [2000.0, 0.0, 0.0];
    for _ in 0..4 {
        let now = g.time;
        g.detection.pairs.insert((lurker, player), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, now));
        assert_eq!(g.aggro_tick(), 0);
        g.time += 5.0;
    }
    assert!(g.provocations.take_notes().is_empty());
    assert_eq!(g.warning_to(lurker), None);
}

#[test]
fn inside_half_its_aggro_radius_an_actor_attacks_without_warning() {
    // `fnv_game::aggro` "The timing": the original's Young Gecko attacked on first
    // detection at 196 of its 500 units.
    let f = fixture();
    let mut g = game(&f);
    let (lurker, player) = (FormId(LURKER_REF), FormId::PLAYER_REF);
    g.world.get(player).unwrap().pos = [0.0; 3];
    g.world.get(lurker).unwrap().pos = [300.0, 0.0, 0.0];
    let now = g.time;
    g.detection.pairs.insert((lurker, player), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, now));
    assert_eq!(g.aggro_tick(), 1);
    let notes = g.provocations.take_notes();
    assert!(!notes.iter().any(|n| matches!(n, Note::AggroWarned { .. })), "{notes:?}");
    assert!(notes.iter().any(|n| matches!(n, Note::AggroAttacks { actor, .. } if *actor == lurker)));
}

#[test]
fn an_actor_created_at_run_time_attacks_inside_its_aggro_radius_without_warning() {
    // `fnv_game::aggro` "Spawned actors do not warn" (#1341): the original's CrCoyote
    // PlaceAtMe'd at a placed coyote's spot fought the player 700 of 1,024 units off at
    // once, where the placed one warned for 14.9 s. The lurker's base placed at run time,
    // 600 of its 650 units off (the placed one warns there): it attacks at the first tick.
    let f = fixture();
    let mut g = game(&f);
    let player = FormId::PLAYER_REF;
    g.world.get(player).unwrap().pos = [0.0; 3];
    let cell = g.world.get(FormId(LURKER_REF)).unwrap().cell;
    let spawned = g.world.place(FormId(LURKER), cell, [600.0, 0.0, 0.0], [0.0; 3], 1).unwrap();
    assert!(spawned.is_dynamic());
    assert_eq!(g.aggro_radius(spawned), Some(650.0));
    assert!(!g.would_attack(spawned, player));
    let now = g.time;
    g.detection.pairs.insert((spawned, player), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, now));
    assert_eq!(g.aggro_tick(), 1);
    let notes = g.provocations.take_notes();
    assert!(!notes.iter().any(|n| matches!(n, Note::AggroWarned { .. })), "{notes:?}");
    assert!(notes.iter().any(|n| matches!(n, Note::AggroAttacks { actor, .. } if *actor == spawned)));
    assert!(g.would_attack(spawned, player));
    // Outside its radius it leaves the player alone, as a placed one does.
    let far = g.world.place(FormId(LURKER), cell, [700.0, 0.0, 0.0], [0.0; 3], 1).unwrap();
    let now = g.time;
    g.detection.pairs.insert((far, player), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, now));
    assert_eq!(g.aggro_tick(), 0);
    assert!(!g.would_attack(far, player));
}

#[test]
fn an_aggro_warning_ends_when_the_player_leaves_the_radius() {
    // The warning look (`fnv_game::aggro`, "The look") lasts while the player is inside;
    // the count is kept for the grace, the look is not.
    let f = fixture();
    let mut g = game(&f);
    let (lurker, player) = (FormId(LURKER_REF), FormId::PLAYER_REF);
    g.world.get(player).unwrap().pos = [0.0; 3];
    g.world.get(lurker).unwrap().pos = [600.0, 0.0, 0.0];
    let seen = |g: &mut Game| {
        let now = g.time;
        g.detection.pairs.insert((lurker, player), (Detection { value: 5.0, band: Band::Detected, ..Default::default() }, now));
    };
    seen(&mut g);
    g.aggro_tick();
    assert_eq!(g.warning_to(lurker), Some(player));
    g.world.get(lurker).unwrap().pos = [700.0, 0.0, 0.0];
    g.time += 1.0;
    seen(&mut g);
    g.aggro_tick();
    assert_eq!(g.warning_to(lurker), None);
    assert!(!g.draws_weapon(lurker));
    assert_eq!(g.provocations.aggro_warnings[&lurker].0, 1, "the count is kept for the grace");
}

#[test]
fn a_created_leveled_actor_carries_what_its_pick_carries() {
    // The stress tests' battle (2026-10-18): troopers created from a leveled list carried
    // nothing and fought with their fists. A created reference of a leveled list, or of an
    // actor whose Inventory comes from one, carries the picked actor's items.
    let f = fixture();
    let mut g = game(&f);
    for base in [LIST_WOLF, NAMELESS] {
        let r = g.create_at(FormId(base), Some(FormId(CELL)), [10.0, 20.0, 0.0], [0.0; 3]).unwrap();
        assert_eq!(g.world.item_count(r, FormId(PELT)).unwrap(), 1, "{base:#x}");
    }
    // An actor with items of its own keeps them as before.
    let w = g.create_at(FormId(WOLF), Some(FormId(CELL)), [0.0; 3], [0.0; 3]).unwrap();
    assert_eq!(g.world.item_count(w, FormId(PELT)).unwrap(), 1);
}

#[test]
fn the_template_supplies_the_name_and_inventory_it_is_flagged_for() {
    // Playtest list 4, item 20: a creature with no data of its own, whose template is a
    // leveled list of one wolf, is named and carries as the wolf.
    let f = fixture();
    let (r, base) = (FormId(NAMELESS_REF), FormId(NAMELESS));
    assert_eq!(f.data.display_base(base, r), FormId(WOLF));
    // Without the Base Data flag the actor names itself.
    assert_eq!(f.data.display_base(FormId(SPAWN), FormId(SPAWN_REF)), FormId(SPAWN));
    let sources = f.data.inventory_sources(&f.refs);
    assert_eq!(sources.get(&r), Some(&FormId(WOLF)));
    // A leveled list placed directly carries its entry's items; actors with their own
    // inventory are not listed.
    assert_eq!(sources.get(&FormId(LIST_REF)), Some(&FormId(WOLF)));
    assert!(!sources.contains_key(&FormId(LEADER_REF)) && !sources.contains_key(&FormId(SPAWN_REF)));
}

#[test]
fn nobody_fights_or_detects_a_ghost_and_a_ghosted_player_is_still_hurt() {
    // GECK wiki SetGhost notes: "Enemies of this calling ref will not go into combat or
    // detect an actor with this flag"; "the player can" be damaged.
    let f = fixture();
    let mut g = game(&f);
    let (own, player) = (FormId(OWN_REF), FormId::PLAYER_REF);
    assert!(g.attacked_by(own, player), "a grudge");
    assert!(g.would_attack(own, player));
    g.extra.entry(player).or_default().ghost = true;
    assert!(!g.would_attack(own, player), "a ghost is no target");
    assert!(g.ghost_hidden_from(own, player));
    let s = fnv_game::detection::Settings::default();
    let seen = fnv_game::detection::Situation { distance: 100.0, line_of_sight: true, in_view: true, light: 100.0, ..Default::default() };
    let (det, _) = g.detect(&s, own, player, &seen);
    assert_eq!(det.band, Band::Hidden, "an enemy does not detect a ghost: {det:?}");
    assert!(!g.is_immune(player), "a ghosted player is still hurt");
    g.extra.entry(player).or_default().ghost = false;
    assert!(g.would_attack(own, player));
    let (det, _) = g.detect(&s, own, player, &seen);
    assert_ne!(det.band, Band::Hidden, "{det:?}");
}

/// #3479: a robot the player shut down (SetUnconscious 1) detects no one, as Old World
/// Blues' X-13 test needs; the sight itself stays measured.
#[test]
fn an_unconscious_actor_detects_no_one() {
    let f = fixture();
    let mut g = game(&f);
    let (own, player) = (FormId(OWN_REF), FormId::PLAYER_REF);
    let s = fnv_game::detection::Settings::default();
    let seen = fnv_game::detection::Situation { distance: 100.0, line_of_sight: true, in_view: true, light: 100.0, ..Default::default() };
    assert_eq!(g.detect(&s, own, player, &seen).0.band, Band::Detected);
    g.extra.entry(own).or_default().unconscious = true;
    let det = g.detect(&s, own, player, &seen).0;
    assert_eq!((det.band, det.seen), (Band::Hidden, true), "{det:?}");
    g.extra.entry(own).or_default().unconscious = false;
    assert_eq!(g.detect(&s, own, player, &seen).0.band, Band::Detected);
}

/// SetUnconscious 1 and GetDetected in one tick, with no measurement between: what the
/// actor last detected reads Hidden at once; what others detect does not change; awake
/// again (SetUnconscious 0), the next measurement decides.
#[test]
fn set_unconscious_hides_what_the_actor_last_detected() {
    const GET_DETECTED: u16 = 0x102D;
    const SET_UNCONSCIOUS: u16 = 0x10F1;
    let f = fixture();
    let mut g = game(&f);
    let (own, player) = (FormId(OWN_REF), FormId::PLAYER_REF);
    let s = fnv_game::detection::Settings::default();
    let seen = fnv_game::detection::Situation { distance: 100.0, line_of_sight: true, in_view: true, light: 100.0, ..Default::default() };
    g.detect(&s, own, player, &seen);
    let other = g.detect(&s, player, own, &seen).0;
    let detected = |g: &mut Game| call(g, GET_DETECTED, OWN_REF, &[form(player.0)]);
    let set = |g: &mut Game, on: f64| call(g, SET_UNCONSCIOUS, OWN_REF, &[ArgValue::Value(Value::Num(on))]);
    assert_eq!(detected(&mut g), 1.0);
    set(&mut g, 1.0);
    assert_eq!(detected(&mut g), 0.0);
    let pair = *g.detection.get(own, player).unwrap();
    assert!(pair.band == Band::Hidden && pair.value <= s.noticed_min && pair.seen && pair.clear, "{pair:?}");
    assert_eq!(g.detection.get(player, own), Some(&other), "only its own detections");
    set(&mut g, 0.0);
    assert_eq!(detected(&mut g), 0.0, "until it is measured again");
    g.detect(&s, own, player, &seen);
    assert_eq!(detected(&mut g), 1.0);
}

#[test]
fn a_mine_spares_its_owners_side() {
    let f = fixture();
    let mut g = game(&f);
    let spares = |g: &Game, owner: Option<u32>, actor: u32| fnv_game::functions::mine_spares(g, owner.map(FormId), FormId(actor));
    // A raiders' mine: their members are spared, others and the player are not.
    assert!(spares(&g, Some(RAIDERS), BANDIT_REF));
    assert!(spares(&g, Some(RAIDERS), LEADER_REF));
    assert!(!spares(&g, Some(RAIDERS), LOAFER_REF));
    assert!(!spares(&g, Some(RAIDERS), 0x14));
    // An ally of the owning faction is spared.
    g.forms.faction_reactions.entry(FormId(RAIDERS)).or_default().insert(FormId(IDLERS), 2);
    assert!(spares(&g, Some(RAIDERS), LOAFER_REF));
    // The player's own mine spares the player; one without an owner spares nobody.
    assert!(spares(&g, Some(0x14), 0x14));
    assert!(!spares(&g, Some(0x14), LOAFER_REF));
    assert!(!spares(&g, None, BANDIT_REF));
}

/// A frag grenade's blast (FalloutNV.esm GrenadeFragExplosion: 125 damage, no object
/// effect, the weapon's damage type None [observed]).
const FRAG: fnv_game::pickpocket::Explosive = fnv_game::pickpocket::Explosive { damage: 125.0, resist: None, enchantment: None };

#[test]
fn a_planted_explosive_goes_off_after_the_timer() {
    // GECK wiki Projectile notes: GetPlantedExplosive is true after
    // fProjectileInventoryGrenadeFreakoutTime; after fProjectileInventoryGrenadeTimer the
    // explosive goes off, "always killing it regardless of its HP".
    let f = fixture();
    let mut g = game(&f);
    let (victim, grenade) = (FormId(OWN_REF), FormId(0x0000_4330));
    let rules = fnv_game::pickpocket::Rules::default();
    let start = g.time;
    g.plant_explosive(victim, grenade, FRAG).unwrap();
    assert!(!g.has_planted_explosive(victim), "not before the freak-out time");
    g.time = start + rules.freakout_time + 0.01;
    assert!(g.has_planted_explosive(victim));
    assert!(g.tick_planted().is_empty(), "not before the timer");
    g.time = start + rules.grenade_timer + 0.01;
    let blasts = g.tick_planted();
    assert_eq!(blasts, vec![fnv_game::pickpocket::Blast { victim, item: grenade, killed: true }]);
    assert!(g.is_dead(victim));
    assert!(!g.has_planted_explosive(victim), "gone off");
    assert_eq!(g.misc_stat(fnv_game::misc_stats::PANTS_EXPLODED), 1.0);
    // A dead actor cannot be pickpocketed.
    assert_eq!(g.pickpocket_refusal(victim), Some(fnv_game::pickpocket::Refusal::NotAllowed));
}

/// Plant `blast` on `victim` and run the clock past the timer; the blasts.
fn plant_and_wait(g: &mut Game, victim: FormId, blast: fnv_game::pickpocket::Explosive) -> Vec<fnv_game::pickpocket::Blast> {
    let rules = fnv_game::pickpocket::Rules::default();
    let start = g.time;
    g.plant_explosive(victim, FormId(0x0000_4330), blast).unwrap();
    g.time = start + rules.grenade_timer + 0.01;
    g.tick_planted()
}

#[test]
fn only_an_explosion_that_hurts_the_victims_kind_kills() {
    // Owner 2026-11-05: "instantly killing them if it's a damaging type"; the rule
    // (`Game::explosive_harms`) is a hypothesis (observer-checks.md).
    use fnv_game::cond::{Cmp, Condition, Operand, RunOn};
    use fnv_game::data::{EffectItem, MagicEffectDef};
    const GET_IS_CREATURE: u16 = 0x40;
    let (ench, damage_health, man, rat) = (FormId(0x9A0), FormId(0x9A1), FormId(LOAFER_REF), FormId(LURKER_REF));
    let mut f = fixture();
    // As the Pulse Grenade's EMP: Damage Health on some kinds only (here: creatures).
    f.data.magic_effects.insert(damage_health, MagicEffectDef { flags: MagicEffectDef::HOSTILE | MagicEffectDef::DETRIMENTAL, actor_value: Some(av::HEALTH), ..Default::default() });
    let only_creatures = Condition { cmp: Cmp::Eq, or: false, run_on: RunOn::Subject, function: GET_IS_CREATURE, params: Vec::new(), rhs: Operand::Value(1.0) };
    f.data.effect_items.insert(ench, vec![EffectItem { effect: damage_health, magnitude: 200, area: 0, duration: 0, actor_value: Some(av::HEALTH), conditions: vec![only_creatures] }]);
    let pulse = fnv_game::pickpocket::Explosive { damage: 10.0, resist: Some(fnv_game::functions::av_by_name("EMP Resistance").unwrap()), enchantment: Some(ench) };

    // The EMP on a man: it goes off (one more Pants Exploded), he lives.
    let mut g = game(&f);
    let b = plant_and_wait(&mut g, man, pulse);
    assert_eq!(b.len(), 1);
    assert!(!b[0].killed && !g.is_dead(man), "{b:?}");
    assert_eq!(g.misc_stat(fnv_game::misc_stats::PANTS_EXPLODED), 1.0);
    // On a creature it kills.
    let b = plant_and_wait(&mut g, rat, pulse);
    assert!(b[0].killed && g.is_dead(rat), "{b:?}");

    // A blast without damage kills nobody; a frag kills both.
    let mut g = game(&f);
    let flash = fnv_game::pickpocket::Explosive { damage: 0.0, resist: None, enchantment: None };
    assert!(!plant_and_wait(&mut g, man, flash)[0].killed);
    assert!(!g.is_dead(man));
    assert!(plant_and_wait(&mut g, man, FRAG)[0].killed);
    assert!(plant_and_wait(&mut g, rat, FRAG)[0].killed);
}

#[test]
fn a_planted_explosive_does_not_go_live_on_an_essential_actor() {
    // Fallout Wiki, Fallout 3 Pickpocket: a child just gets the explosive, no explosion;
    // New Vegas and essential actors: hypothesis (observer-checks.md).
    use fnv_game::pickpocket::NotLive;
    let essential = FormId(WATCHMAN_REF);
    let f = fixture();
    let mut g = game(&f);
    g.extra.entry(essential).or_default().essential = Some(true);
    assert_eq!(g.plant_explosive(essential, FormId(0x4330), FRAG), Err(NotLive::Essential));
    assert!(g.pickpocket.planted.is_empty(), "nothing goes live");
    g.time += 10.0;
    assert!(g.tick_planted().is_empty());
    assert!(!g.is_dead(essential));
    // A child: `functions.rs`, a_planted_explosive_stays_an_item_on_a_child.
}

#[test]
fn a_planted_kill_is_seen_by_whoever_saw_the_player_while_it_was_live() {
    // Fallout Wiki, New Vegas Pickpocket: staying hidden from the plant to the blast, no
    // one turns hostile and no infamy is gained; the kill is judged with those who
    // detected the player while it was live (pickpocket.md §5).
    use fnv_game::crime::{Kind, Note as CrimeNote};
    let (victim, witness, player) = (FormId(LOAFER_REF), FormId(GANGER_A_REF), FormId::PLAYER_REF);
    let rules = fnv_game::pickpocket::Rules::default();
    let murders = |g: &Game| -> Vec<Vec<FormId>> {
        g.crimes.notes.iter().filter_map(|n| match n {
            CrimeNote::Crime { kind: Kind::Murder, seen_by, .. } => Some(seen_by.clone()),
            _ => None,
        }).collect()
    };
    let f = fixture();

    // Hidden all along: a murder nobody saw.
    let mut g = game(&f);
    plant_and_wait(&mut g, victim, FRAG);
    assert!(g.is_dead(victim));
    assert_eq!(murders(&g), vec![Vec::<FormId>::new()], "seen by nobody");

    // Seen in the middle of the fuse, gone from view at the blast: still a witness.
    let mut g = game(&f);
    let start = g.time;
    g.plant_explosive(victim, FormId(0x4330), FRAG).unwrap();
    g.time = start + 0.5;
    let t = g.time;
    g.detection.pairs.insert((witness, player), (Detection { value: 30.0, band: Band::Detected, ..Default::default() }, t));
    assert!(g.tick_planted().is_empty());
    g.detection.pairs.remove(&(witness, player));
    g.time = start + rules.grenade_timer + 0.01;
    assert!(g.tick_planted()[0].killed);
    assert_eq!(murders(&g), vec![vec![witness]]);
    assert!(g.pickpocket.blast_witnesses.is_empty(), "only while the kill is judged");
    assert!(g.crime_witnesses().is_empty());
}

#[test]
fn a_planted_kill_counts_for_the_weapon_the_player_holds() {
    // Fallout Wiki, New Vegas Pickpocket: the kill counts as one with the weapon type
    // the player has equipped. Here the player holds nothing (fists, Unarmed), and the
    // victim's last hit by the player was with a gun: the fists get the kill.
    use fnv_game::challenges::{kind, ChallengeDef};
    const UNARMED: u32 = 45;
    const GUNS: u32 = 41;
    let (victim, gun, by_fists, by_gun) = (FormId(LOAFER_REF), FormId(0x9B0), FormId(0x9B1), FormId(0x9B2));
    let mut f = fixture();
    f.data.weapon_skills.insert(gun, i64::from(GUNS));
    for (c, skill) in [(by_fists, UNARMED), (by_gun, GUNS)] {
        let def = ChallengeDef { form: c, kind: kind::KILL_FROM_LIST, threshold: 100, flags: 0, interval: 0, value1: 0, value2: skill, value3: 0, form1: None, form2: None, name: None, description: None };
        f.data.challenge_defs.insert(c, def);
    }
    let mut g = game(&f);
    g.extra.entry(victim).or_default().last_hit = Some(fnv_game::game::LastHit { attacker: Some(FormId::PLAYER_REF), weapon: Some(gun), location: 0, critical: false });
    assert!(plant_and_wait(&mut g, victim, FRAG)[0].killed);
    let progress = |c: FormId| g.player.challenges.get(&c).map_or(0, |c| c.progress);
    assert_eq!((progress(by_fists), progress(by_gun)), (1, 0));
    assert_eq!(g.misc_stat(fnv_game::misc_stats::PEOPLE_KILLED), 1.0);
}

/// The durable state crosses the actual JSON and world restore boundaries.
fn restore_plants<'a>(f: &'a Fixture, g: &Game) -> Game<'a> {
    let snap: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.snapshot()).unwrap()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    Game::restore(&f.data, world, g.cfg, &snap).unwrap()
}

#[test]
fn a_saved_plant_keeps_its_remaining_fuse_before_and_after_freakout() {
    use fnv_game::misc_stats::{PANTS_EXPLODED, PEOPLE_KILLED};
    use fnv_game::trace::EventKind;
    let f = fixture();
    let (victim, item) = (FormId(OWN_REF), FormId(PELT));
    let rules = fnv_game::pickpocket::Rules::default();
    for elapsed in [0.05, 0.5] {
        let mut g = game(&f);
        g.time = 20.0;
        g.world.add_item(victim, item, 2).unwrap();
        g.plant_explosive(victim, item, FRAG).unwrap();
        let at = g.time;
        g.time += elapsed;
        assert_eq!(g.has_planted_explosive(victim), elapsed >= rules.freakout_time);
        // Transient witnesses and cached rules must not survive a load.
        g.pickpocket.blast_witnesses.push(FormId(GANGER_A_REF));
        g.pickpocket.rules.as_mut().unwrap().grenade_timer = 99.0;
        let mut h = restore_plants(&f, &g);
        assert_eq!(h.time, g.time);
        assert_eq!(h.pickpocket.planted, g.pickpocket.planted);
        assert!(h.pickpocket.rules.is_none());
        assert!(h.pickpocket.blast_witnesses.is_empty());
        assert_eq!(h.has_planted_explosive(victim), elapsed >= rules.freakout_time);
        // Loading and an open menu do not age the fuse, even over many ticks.
        h.loading = true;
        h.frames(180);
        h.loading = false;
        h.menu = Some(1001);
        h.frames(180);
        h.menu = None;
        assert_eq!(h.time, at + elapsed);
        assert!(h.tick_planted().is_empty());
        h.time = at + rules.grenade_timer - 0.01;
        assert!(h.tick_planted().is_empty());
        h.time += 0.02;
        assert_eq!(h.tick_planted(), vec![fnv_game::pickpocket::Blast { victim, item, killed: true }]);
        assert_eq!(h.world.item_count(victim, item).unwrap(), 1);
        assert!(h.is_dead(victim));
        assert_eq!((h.misc_stat(PANTS_EXPLODED), h.misc_stat(PEOPLE_KILLED)), (1.0, 1.0));
        h.time += 10.0;
        assert!(h.tick_planted().is_empty());
        assert_eq!(h.world.item_count(victim, item).unwrap(), 1);
        assert_eq!((h.misc_stat(PANTS_EXPLODED), h.misc_stat(PEOPLE_KILLED)), (1.0, 1.0));
        // Completion stays complete through another save too.
        let mut again = restore_plants(&f, &h);
        assert!(again.tick_planted().is_empty());
        assert_eq!(again.misc_stat(PANTS_EXPLODED), 1.0);
        assert!(h.events.iter().all(|e| !matches!(e.kind, EventKind::CallFailed { .. })));
    }
}

#[test]
fn saved_multiple_plants_keep_their_blasts_and_independent_times() {
    use fnv_game::misc_stats::{PANTS_EXPLODED, PEOPLE_KILLED};
    let f = fixture();
    let mut g = game(&f);
    let (a, b, item) = (FormId(OWN_REF), FormId(LOAFER_REF), FormId(PELT));
    let harmless = fnv_game::pickpocket::Explosive { damage: 0.0, resist: Some(av::POISON_RESISTANCE), enchantment: None };
    for victim in [a, b] {
        g.world.add_item(victim, item, 1).unwrap();
    }
    g.plant_explosive(a, item, harmless).unwrap();
    let at = g.time;
    g.time += 0.5;
    g.plant_explosive(b, item, FRAG).unwrap();
    g.time += 0.5;
    let mut h = restore_plants(&f, &g);
    assert_eq!(h.pickpocket.planted, g.pickpocket.planted);
    h.time = at + 2.01;
    assert_eq!(h.tick_planted(), vec![fnv_game::pickpocket::Blast { victim: a, item, killed: false }]);
    assert!(!h.is_dead(a) && !h.is_dead(b));
    assert_eq!(h.world.item_count(a, item).unwrap(), 0);
    assert_eq!(h.world.item_count(b, item).unwrap(), 1);
    let mut h = restore_plants(&f, &h);
    h.time = at + 2.51;
    assert_eq!(h.tick_planted(), vec![fnv_game::pickpocket::Blast { victim: b, item, killed: true }]);
    assert!(!h.is_dead(a) && h.is_dead(b));
    assert_eq!(h.world.item_count(b, item).unwrap(), 0);
    assert_eq!((h.misc_stat(PANTS_EXPLODED), h.misc_stat(PEOPLE_KILLED)), (2.0, 1.0));
    assert!(h.tick_planted().is_empty());
}

#[test]
fn a_saved_plant_retains_witnesses_who_no_longer_see_the_player() {
    use fnv_game::crime::{Kind, Note as CrimeNote};
    let f = fixture();
    let mut g = game(&f);
    let (victim, witness, player) = (FormId(LOAFER_REF), FormId(GANGER_A_REF), FormId::PLAYER_REF);
    g.plant_explosive(victim, FormId(PELT), FRAG).unwrap();
    let at = g.time;
    g.time += 0.5;
    g.detection.pairs.insert((witness, player), (Detection { value: 30.0, band: Band::Detected, ..Default::default() }, g.time));
    assert!(g.tick_planted().is_empty());
    assert_eq!(g.pickpocket.planted[0].seen_by, vec![witness]);
    let mut h = restore_plants(&f, &g);
    assert!(h.crime_witnesses().is_empty());
    h.time = at + 2.01;
    assert!(h.tick_planted()[0].killed);
    let murders: Vec<_> = h.crimes.notes.iter().filter_map(|n| match n {
        CrimeNote::Crime { kind: Kind::Murder, seen_by, .. } => Some(seen_by.clone()),
        _ => None,
    }).collect();
    assert_eq!(murders, vec![vec![witness]]);
    assert!(h.pickpocket.blast_witnesses.is_empty());
}

/// The victim's units of `item`: (all, live, ordinary).
fn units(g: &Game, victim: FormId, item: FormId) -> (i32, i32, i32) {
    g.world.peek(victim).and_then(|s| s.inventory.as_ref()).map_or((0, 0, 0), |inv| (inv.count(item), inv.live_count(item), inv.free_count(item)))
}

#[test]
fn each_plant_reserves_one_unit_that_its_blast_takes() {
    // #1640 item 5: the live explosive is a unit apart from the victim's own of the same
    // item; the victim cannot equip, use, drop or give it away; the blast takes exactly it.
    let f = fixture();
    let mut g = game(&f);
    let (victim, item) = (FormId(OWN_REF), FormId(PELT));
    g.world.add_item(victim, item, 3).unwrap();
    let start = g.time;
    g.plant_explosive(victim, item, FRAG).unwrap();
    assert_eq!(units(&g, victim, item), (3, 1, 2));
    g.time = start + 0.5;
    g.plant_explosive(victim, item, FRAG).unwrap();
    assert_eq!(units(&g, victim, item), (3, 2, 1));
    // The victim's own unit goes; the live ones stay.
    assert_eq!(g.world.remove_item(victim, item, 3).unwrap(), 1);
    assert!(g.world.drop_item(victim, item, 1).unwrap().is_empty());
    assert_eq!(g.world.transfer(victim, FormId::PLAYER_REF, item, 1).unwrap(), 0);
    assert_eq!(units(&g, victim, item), (2, 2, 0));
    // The first blast takes one live unit, not more.
    g.time = start + 2.01;
    let b = g.tick_planted();
    assert_eq!(b.len(), 1);
    assert!(b[0].killed && g.is_dead(victim));
    assert_eq!(units(&g, victim, item), (1, 1, 0));
    // The second plant's victim died before it went off: let off, its unit ordinary.
    g.time = start + 2.51;
    assert!(g.tick_planted().is_empty());
    assert_eq!(units(&g, victim, item), (1, 0, 1));
    assert_eq!(g.misc_stat(fnv_game::misc_stats::PANTS_EXPLODED), 1.0);
}

#[test]
fn a_plant_on_an_essential_actor_stays_an_ordinary_item() {
    use fnv_game::pickpocket::NotLive;
    let f = fixture();
    let mut g = game(&f);
    let (essential, item) = (FormId(WATCHMAN_REF), FormId(PELT));
    g.extra.entry(essential).or_default().essential = Some(true);
    g.world.add_item(essential, item, 1).unwrap();
    assert_eq!(g.plant_explosive(essential, item, FRAG), Err(NotLive::Essential));
    assert_eq!(units(&g, essential, item), (1, 0, 1));
}

#[test]
fn live_units_follow_the_saved_plants() {
    let f = fixture();
    let (victim, other, item) = (FormId(OWN_REF), FormId(LOAFER_REF), FormId(PELT));
    let mut g = game(&f);
    g.world.add_item(victim, item, 2).unwrap();
    g.plant_explosive(victim, item, FRAG).unwrap();
    // Saved and loaded: one live unit, one ordinary.
    let h = restore_plants(&f, &g);
    assert_eq!(units(&h, victim, item), (2, 1, 1));

    // A save from before live units: its plant's unit is reserved on load.
    let mut snap = g.snapshot();
    let inv = snap.sim.refs.get_mut(&victim).unwrap().inventory.as_mut().unwrap();
    assert_eq!(inv.set_live_count(item, 0), 0);
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &snap).unwrap();
    assert_eq!(units(&h, victim, item), (2, 1, 1));
    h.time += 10.0;
    assert_eq!(h.tick_planted().len(), 1);
    assert_eq!(units(&h, victim, item), (1, 0, 1), "exactly the reserved unit");

    // A live unit without its plant (the plant dropped) is ordinary again on load.
    let mut snap = g.snapshot();
    snap.planted.clear();
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let h = Game::restore(&f.data, world, g.cfg, &snap).unwrap();
    assert_eq!(units(&h, victim, item), (2, 0, 2));
    // A plant whose unit is not carried still loads, reserving nothing.
    let mut g = game(&f);
    g.plant_explosive(other, item, FRAG).unwrap();
    let h = restore_plants(&f, &g);
    assert_eq!(h.pickpocket.planted.len(), 1);
    assert_eq!(units(&h, other, item).1, 0);
}

#[test]
fn an_old_save_reserves_a_planted_grenade_its_victim_had_put_on() {
    // Before live units the victim could equip the planted grenade: a save with the plant
    // and its only unit equipped gets that unit taken off and reserved on load, and the
    // blast takes it.
    let f = fixture();
    let (victim, item) = (FormId(OWN_REF), FormId(PELT));
    let mut g = game(&f);
    g.world.add_item(victim, item, 1).unwrap();
    g.plant_explosive(victim, item, FRAG).unwrap();
    let mut snap: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.snapshot()).unwrap()).unwrap();
    let inv = snap.sim.refs.get_mut(&victim).unwrap().inventory.as_mut().unwrap();
    assert_eq!(inv.set_live_count(item, 0), 0);
    assert!(inv.set_equipped(item));
    assert!(!serde_json::to_string(&*inv).unwrap().contains("live"), "the old format");
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &snap).unwrap();
    assert_eq!(units(&h, victim, item), (1, 1, 0));
    assert!(!h.world.peek(victim).unwrap().inventory.as_ref().unwrap().is_equipped(item), "taken off");
    h.time += 10.0;
    assert!(h.tick_planted()[0].killed);
    assert_eq!(units(&h, victim, item), (0, 0, 0), "the blast took it");
}

#[test]
fn reset_inventory_keeps_a_planted_explosive() {
    // ResetInventory during the fuse rolls the victim's own items again; the planted unit
    // stays, and the blast takes it.
    const RESET_INVENTORY: u16 = 0x11ED;
    let f = fixture();
    let (victim, item) = (FormId(OWN_REF), FormId(PELT));
    let mut g = game(&f);
    // Its carried items are the Wolf's (one Pelt), so the reset rolls something.
    g.world.set_inventory_source(victim, Some(FormId(WOLF)));
    g.world.add_item(victim, item, 3).unwrap();
    g.plant_explosive(victim, item, FRAG).unwrap();
    assert_eq!(units(&g, victim, item), (3, 1, 2));
    call(&mut g, RESET_INVENTORY, OWN_REF, &[]);
    assert_eq!(units(&g, victim, item), (2, 1, 1), "its own two went for the roll's one; the planted one stayed");
    g.time += 10.0;
    assert_eq!(g.tick_planted().len(), 1);
    assert_eq!(units(&g, victim, item), (1, 0, 1), "the blast took the planted one");
}

#[test]
fn an_old_save_has_no_live_plants_and_empty_plants_are_omitted() {
    let f = fixture();
    let mut g = game(&f);
    assert!(serde_json::to_value(g.snapshot()).unwrap().get("planted").is_none());
    let (victim, item) = (FormId(OWN_REF), FormId(PELT));
    g.world.add_item(victim, item, 1).unwrap();
    g.plant_explosive(victim, item, FRAG).unwrap();
    let mut value = serde_json::to_value(g.snapshot()).unwrap();
    assert!(value.as_object_mut().unwrap().remove("planted").is_some());
    let snap: fnv_game::game::GameSnapshot = serde_json::from_value(value).unwrap();
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &snap).unwrap();
    assert!(h.pickpocket.planted.is_empty());
    h.time += 10.0;
    assert!(h.tick_planted().is_empty());
    assert!(!h.is_dead(victim));
    assert_eq!(h.world.item_count(victim, item).unwrap(), 1);
}

#[test]
fn the_pickpocket_tweak_leaves_the_caught_player_detected() {
    // bPickpocketCaughtCausesDetection (docs/design/tweaks.md): without the tweak being
    // caught changes nothing the sneak word reads; with it the victim detects the player.
    let f = fixture();
    for on in [false, true] {
        let mut g = game(&f);
        g.tweaks.pickpocket_caught_detects = on;
        g.pickpocket_caught(FormId(WATCHMAN_REF));
        assert_eq!(g.player_detected_for_sneak(), on, "tweak {on}");
    }
}

#[test]
fn the_frenzy_tweak_gives_a_frenzied_kill_to_the_player() {
    // bFrenzyGrantsPlayerXP (docs/design/tweaks.md): Aggression 3 is Frenzied.
    const SET_AV: u16 = 0x100F;
    let f = fixture();
    let watchman = FormId(WATCHMAN_REF);
    for on in [false, true] {
        let mut g = game(&f);
        g.tweaks.frenzy_kill_xp = on;
        assert!(!g.kill_earns_xp(watchman), "not frenzied, tweak {on}");
        call(&mut g, SET_AV, WATCHMAN_REF, &[ArgValue::Enum(av::AGGRESSION), ArgValue::Value(fnv_script::vm::Value::Num(3.0))]);
        assert_eq!(g.kill_earns_xp(watchman), on, "frenzied, tweak {on}");
    }
}

/// #1538 (owner decision 2026-11-13; combat-ai.md §10.3.1): the player's attack on a member
/// of an Evil faction turns every member in the area (the victim's interior cell) on the
/// player, seen or not, and a crime-tracking one makes the player its enemy. Others in
/// the cell are left alone.
#[test]
fn attacking_an_evil_factions_member_alerts_its_members_in_the_area() {
    use fnv_game::crime::faction_flags::{EVIL, TRACK_CRIME};
    let player = FormId::PLAYER_REF;
    let (own, helper, coward, loafer) = (FormId(OWN_REF), FormId(HELPER_REF), FormId(COWARD_REF), FormId(LOAFER_REF));
    let mut f = fixture();
    f.data.faction_flags.insert(FormId(RATS), EVIL | TRACK_CRIME);
    // A hit: nobody sees it, yet the victim's faction fellows all turn on the player.
    {
        let mut g = game(&f);
        only_near(&mut g, &[OWN_REF, HELPER_REF, LOAFER_REF]);
        assert!(!g.would_attack(helper, player));
        assert!(g.attacked_by(own, player));
        for m in [own, helper, coward] {
            assert!(g.would_attack(m, player), "{m}");
        }
        assert!(!g.would_attack(loafer, player), "not of the faction");
        assert!(g.player.enemy_of.contains(&FormId(RATS)));
        let notes = g.provocations.take_notes();
        assert!(notes.iter().any(|n| matches!(n, Note::Alerted { actor, victim } if *actor == helper && *victim == own)), "{notes:?}");
        assert!(notes.iter().any(|n| matches!(n, Note::AreaAlert { victim, faction, alerted } if *victim == own && *faction == FormId(RATS) && *alerted >= 2)), "{notes:?}");
        // Alarmed at the player: in a combat state.
        assert!(g.awareness.get(helper).is_some_and(|a| a.alarms.iter().any(|x| x.owner == player)));
    }
    // A kill in one blow does the same.
    {
        let mut g = game(&f);
        g.player_kills(own);
        assert!(g.would_attack(helper, player));
    }
    // Not a stray hit on one fighting someone else.
    {
        let mut g = game(&f);
        fighting(&mut g, OWN_REF, LOAFER_REF);
        g.evil_area_alert(own);
        assert!(!g.provocations.holds(helper, player));
    }
    // Not for a faction that is not Evil.
    {
        f.data.faction_flags.insert(FormId(RATS), TRACK_CRIME);
        let mut g = game(&f);
        assert!(g.attacked_by(own, player));
        assert!(!g.would_attack(helper, player));
    }
}

#[test]
fn a_very_aggressive_actor_leaves_a_neutral_outside_its_aggro_radius_alone() {
    // #2547: HELIOS One's guard dogs (Very Aggressive, aggro radius 2,000) went for the
    // NCR troopers 2,800 units off. A Very Aggressive actor attacks a Neutral on sight
    // [documented: GetFactionCombatReaction] only inside its aggro radius when it has one
    // (see crates/fnv-game/src/combat.rs); once fighting it follows the target anywhere.
    let f = fixture();
    let mut g = game(&f);
    let (hound, ganger) = (FormId(HOUND_REF), FormId(GANGER_B_REF));
    assert!(g.would_attack(hound, ganger), "a Neutral to a Very Aggressive actor");
    let seen = Detection { value: 5.0, band: Band::Detected, ..Default::default() };
    g.observe_threat(hound, ganger, &seen, [2800.0, 0.0, 0.0]);
    g.settle_awareness();
    assert_eq!(g.awareness.state(hound), State::Normal, "outside the radius");
    g.observe_threat(hound, ganger, &seen, [900.0, 0.0, 0.0]);
    g.settle_awareness();
    assert_eq!(g.awareness.state(hound), State::Combat, "inside the radius");
    g.observe_threat(hound, ganger, &seen, [5000.0, 0.0, 0.0]);
    g.settle_awareness();
    assert_eq!(g.awareness.state(hound), State::Combat, "a fight goes on outside it");
    // An Enemy is attacked on sight at any range.
    let mut g = game(&f);
    g.forms.faction_reactions.entry(FormId(HOUNDS)).or_default().insert(FormId(GANG_B), 1);
    g.observe_threat(hound, ganger, &seen, [2800.0, 0.0, 0.0]);
    g.settle_awareness();
    assert_eq!(g.awareness.state(hound), State::Combat);
}

// ------------------------------------------------------------------ faction disguises (#2611)

const SET_ALLY: u16 = 0x1079;
const REMOVE_FROM_FACTION: u16 = 0x1180;
const SET_PC_ENEMY_OF_FACTION: u16 = 0x10C6;

fn num(x: f64) -> ArgValue {
    ArgValue::Value(Value::Num(x))
}

#[test]
fn a_disguise_friendship_outranks_a_faction_enemy_flag() {
    // docs/spec-notes/disguises.md §3: the loafer (Idlers, Aggressive) stands for an NCR
    // soldier, Idlers for NCRFactionNV, Rats for ArmorNCRFactionNV. Flagged an enemy of
    // the Idlers, the player is attacked; in a faction that is the player's friend (the
    // armour's SetAlly PlayerFaction ArmorNCRFactionNV 1 1) it is left alone; out of it
    // again (the pulse on a sniffer) it attacks.
    let f = fixture();
    let mut g = game(&f);
    let (loafer, player) = (FormId(LOAFER_REF), FormId::PLAYER_REF);
    assert!(!g.would_attack(loafer, player), "neutral by its factions");
    call(&mut g, SET_PC_ENEMY_OF_FACTION, PLAYER, &[form(IDLERS), num(1.0)]);
    assert!(g.crime_enemy(loafer) && g.would_attack(loafer, player), "the flag on its faction");
    call(&mut g, ADD_TO_FACTION, LOAFER_REF, &[form(RATS), num(0.0)]);
    assert!(g.would_attack(loafer, player), "Rats are no friend of the player yet");
    call(&mut g, SET_ALLY, PLAYER, &[form(PLAYER_FACTION), form(RATS), num(1.0), num(1.0)]);
    assert!(!g.crime_enemy(loafer), "the disguise faction is the player's friend");
    assert!(!g.would_attack(loafer, player));
    assert_eq!(call(&mut g, GET_FACTION_RELATION, LOAFER_REF, &[form(PLAYER)]), 3.0, "Friend");
    call(&mut g, REMOVE_FROM_FACTION, LOAFER_REF, &[form(RATS)]);
    assert!(g.would_attack(loafer, player), "a sniffer the pulse took out of it sees through");
}

#[test]
fn a_flag_on_the_friendly_faction_itself_still_counts() {
    // A crime among the player's friends stays a crime (disguises.md §3): with the
    // disguise faction flagged too, its friendship does not shield its members.
    let f = fixture();
    let mut g = game(&f);
    let (loafer, player) = (FormId(LOAFER_REF), FormId::PLAYER_REF);
    call(&mut g, ADD_TO_FACTION, LOAFER_REF, &[form(RATS), num(0.0)]);
    call(&mut g, SET_ALLY, PLAYER, &[form(PLAYER_FACTION), form(RATS), num(1.0), num(1.0)]);
    call(&mut g, SET_PC_ENEMY_OF_FACTION, PLAYER, &[form(IDLERS), num(1.0)]);
    assert!(!g.would_attack(loafer, player));
    call(&mut g, SET_PC_ENEMY_OF_FACTION, PLAYER, &[form(RATS), num(1.0)]);
    assert!(g.would_attack(loafer, player));
    // A faction reaction of Enemy yields to a shared friendly faction as well (#2560,
    // factions.md §4: allies and friends "cannot attack each other under any
    // circumstances"): the bandit (Raiders, who name the player's faction an enemy) is
    // shielded once one of its factions is the player's ally.
    let bandit = FormId(BANDIT_REF);
    assert!(g.would_attack(bandit, player));
    call(&mut g, ADD_TO_FACTION, BANDIT_REF, &[form(GANG_A), num(0.0)]);
    call(&mut g, SET_ALLY, PLAYER, &[form(PLAYER_FACTION), form(GANG_A), num(1.0), num(1.0)]);
    assert!(!g.would_attack(bandit, player));
}

#[test]
fn an_alarm_about_a_disguised_player_turns_nobody_the_disguise_fools() {
    // #2611: Own (Rats, flagged) would attack the player and notices it; the helper
    // (Rats too, Helps Allies) is in the disguise faction (Idlers here, the player's
    // friend), so it takes the player for a friend: neither the noticed alarm nor the
    // fight's gives it a grudge (`comrades`). Without the disguise faction the same
    // fight's alarm brings it in (an_actor_that_starts_fighting_alarms_its_allies_near_it).
    use fnv_game::awareness::Change;
    let f = fixture();
    let mut g = game(&f);
    let (own, helper, player) = (FormId(OWN_REF), FormId(HELPER_REF), FormId::PLAYER_REF);
    only_near(&mut g, &[OWN_REF, HELPER_REF]);
    call(&mut g, SET_PC_ENEMY_OF_FACTION, PLAYER, &[form(RATS), num(1.0)]);
    call(&mut g, ADD_TO_FACTION, HELPER_REF, &[form(IDLERS), num(0.0)]);
    call(&mut g, SET_ALLY, PLAYER, &[form(PLAYER_FACTION), form(IDLERS), num(1.0), num(1.0)]);
    assert!(g.would_attack(own, player) && !g.would_attack(helper, player), "the helper is fooled");
    let now = g.time;
    let near: std::collections::BTreeMap<FormId, [f32; 3]> = [(own, [0.0; 3]), (helper, [1000.0, 0.0, 0.0]), (player, [3500.0, 0.0, 0.0])].into_iter().collect();
    g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Alert, time: now, about: Some(player), cause: Some(Cause::Noticed), at: Some([3500.0, 0.0, 0.0]), value: Some(-14.9) });
    assert_eq!(g.raise_alarms(&near), 0, "a glimpse");
    g.awareness.changes.clear();
    fighting(&mut g, OWN_REF, PLAYER);
    g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(player), cause: None, at: Some([500.0, 0.0, 0.0]), value: Some(5.0) });
    assert_eq!(g.raise_alarms(&near), 0, "a fight");
    assert!(!g.would_attack(helper, player), "no grudge");
}

#[test]
fn set_combat_disabled_blocks_combat_entry_allows_being_attacked_and_resets_on_load() {
    use fnv_game::awareness::Change;
    use fnv_game::game::RunCtx;
    use fnv_script::vm::XVal;

    let f = fixture();
    let mut g = game(&f);
    let (bandit, own, helper, player) = (FormId(BANDIT_REF), FormId(OWN_REF), FormId(HELPER_REF), FormId::PLAYER_REF);
    let eval_on = |g: &mut Game, actor: FormId, expr: &str| -> f64 {
        let src = format!("Begin Function {{ }}\nSetFunctionValue ({expr})\nEnd\n");
        let ctx = RunCtx { self_ref: Some(actor), ..Default::default() };
        match g.run_query(&src, ctx) {
            Ok(XVal::Num(n)) => n,
            other => panic!("expected Num for `{expr}`, got {other:?} (events: {:?})", g.events),
        }
    };
    let run_on = |g: &mut Game, actor: FormId, stmt: &str| {
        let src = format!("Begin Function {{ }}\n{stmt}\nEnd\n");
        let ctx = RunCtx { self_ref: Some(actor), ..Default::default() };
        g.run_query(&src, ctx).unwrap_or_else(|e| panic!("stmt `{stmt}` failed: {e:?} (events: {:?})", g.events));
    };

    // Baseline: Bandit is hostile to player and enters Combat via StartCombat.
    assert_eq!(eval_on(&mut g, bandit, "GetCombatDisabled"), 0.0);
    assert!(g.would_attack(bandit, player));
    g.start_combat(bandit, player);
    assert_eq!(g.awareness.state(bandit), State::Combat);

    // SetCombatDisabled 1 leaves IsInCombat == 1 on the same frame (t0) and ends active
    // combat on the next AI awareness tick (`Game::settle_awareness`), then blocks new
    // combat entry via StartCombat [observed: `core_20_set_combat_disabled_stops_combat`,
    // `w4d_07_setcombatdisabled_new_combat`, and `w4h_05_set_combat_disabled_multitick`
    // (PR #3803 / #3802)].
    g.awareness.changes.clear();
    run_on(&mut g, bandit, "SetCombatDisabled 1");
    assert_eq!(eval_on(&mut g, bandit, "GetCombatDisabled"), 1.0);
    assert_eq!(g.awareness.state(bandit), State::Combat);
    assert_eq!(eval_on(&mut g, bandit, "IsInCombat"), 1.0, "same-frame t0 keeps IsInCombat == 1");
    assert!(g.present(bandit), "bandit must be present so settle_awareness exits via combat_disabled");
    g.settle_awareness();
    assert_eq!(g.awareness.state(bandit), State::Normal, "AI awareness tick ends active combat");
    assert_eq!(eval_on(&mut g, bandit, "IsInCombat"), 0.0);
    assert_eq!(eval_on(&mut g, bandit, "GetCombatDisabled"), 1.0);
    assert!(
        g.awareness.changes.iter().any(|c| c.actor == bandit && c.from == State::Combat && c.to == State::Normal),
        "combat_disabled tick must emit Combat -> Normal transition: {:?}",
        g.awareness.changes
    );
    assert!(!g.would_attack(bandit, player));
    assert!(!g.hurt_by(bandit, player, [0.0; 3]));
    g.start_combat(bandit, player);
    assert_eq!(g.awareness.state(bandit), State::Normal);
    assert_eq!(eval_on(&mut g, bandit, "IsInCombat"), 0.0);
    g.observe_threat(bandit, player, &Detection { value: 100.0, band: Band::Detected, ..Default::default() }, [0.0; 3]);
    g.settle_awareness();
    assert_eq!(g.awareness.state(bandit), State::Normal);
    assert_eq!(eval_on(&mut g, bandit, "IsInCombat"), 0.0);

    // Other actors can still attack a combat-disabled actor (documented: "though they can still be attacked").
    g.start_combat(own, bandit);
    assert_eq!(g.awareness.state(own), State::Combat);
    assert_eq!(g.combat_target_of(own), Some(bandit));
    assert!(g.would_attack(own, bandit));
    assert_eq!(g.awareness.state(bandit), State::Normal);

    // Ally combat alarms do not recruit a combat-disabled helper.
    only_near(&mut g, &[OWN_REF, HELPER_REF]);
    call(&mut g, SET_PC_ENEMY_OF_FACTION, PLAYER, &[form(RATS), num(1.0)]);
    run_on(&mut g, helper, "SetCombatDisabled 1");
    let now = g.time;
    let near: std::collections::BTreeMap<FormId, [f32; 3]> = [(own, [0.0; 3]), (helper, [500.0, 0.0, 0.0]), (player, [600.0, 0.0, 0.0])].into_iter().collect();
    g.awareness.changes.clear();
    fighting(&mut g, OWN_REF, PLAYER);
    g.awareness.changes.push(Change { actor: own, from: State::Normal, to: State::Combat, time: now, about: Some(player), cause: None, at: Some([600.0, 0.0, 0.0]), value: Some(5.0) });
    assert_eq!(g.raise_alarms(&near), 0, "combat-disabled helper ignores ally alarm");
    assert_eq!(g.awareness.state(helper), State::Normal);

    // SetCombatDisabled 0 re-enables combat.
    run_on(&mut g, bandit, "SetCombatDisabled 0");
    assert_eq!(eval_on(&mut g, bandit, "GetCombatDisabled"), 0.0);
    assert!(g.would_attack(bandit, player));
    g.start_combat(bandit, player);
    assert_eq!(g.awareness.state(bandit), State::Combat);

    // Save/load resets SetCombatDisabled on BOTH fresh-run restore and same-run load (documented:
    // "automatically reset (combat is re-enabled) every time a saved game is loaded").
    run_on(&mut g, bandit, "SetCombatDisabled 1");
    assert_eq!(eval_on(&mut g, bandit, "GetCombatDisabled"), 1.0);
    let snap = g.snapshot();
    let session = g.take_session();

    let mut fresh = Game::restore(&f.data, World::restore(&f.forms, &f.refs, &snap.sim).unwrap(), GameConfig { strict: true, ..GameConfig::default() }, &snap).unwrap();
    assert_eq!(eval_on(&mut fresh, bandit, "GetCombatDisabled"), 0.0);
    assert!(fresh.would_attack(bandit, player));

    let mut same_run = Game::restore(&f.data, World::restore(&f.forms, &f.refs, &snap.sim).unwrap(), GameConfig { strict: true, ..GameConfig::default() }, &snap).unwrap();
    same_run.put_session(session);
    assert_eq!(eval_on(&mut same_run, bandit, "GetCombatDisabled"), 0.0);
    assert!(same_run.would_attack(bandit, player));
}

#[test]
fn neutral_bystanders_outside_aggro_radius_stay_out_when_enemy_attacks_player() {
    // #3459: a Neutral actor (bystander trooper, Aggressive) watches a Very Aggressive
    // actor with an aggro radius of 2,000 (HELIOS One's CrDogNCRAggressive) fight the
    // player (its Enemy). Outside the dog's 2,000-unit aggro radius:
    // 1. Being in Combat with the player does not drop `neutral_out_of_reach` for the
    //    unrelated Neutral bystander (the dog still leaves the trooper alone).
    // 2. `join_against_dangers` respects `neutral_out_of_reach`, so the trooper does not
    //    take a grudge against the dog or alarm its own faction.
    // Inside the 2,000-unit aggro radius, the danger is real and the bystander still joins.
    let mut f = fixture();
    // Make HOUNDS an Enemy of PLAYER_FACTION, matching CrDogNCRAggressive's
    // GeneralHostileToPlayerFaction -> PlayerFaction Enemy.
    f.data.faction_reactions.entry(FormId(HOUNDS)).or_default().push((FormId(PLAYER_FACTION), 1));
    let mut g = game(&f);
    // `trooper` (HELPER_REF, RATS) shares no faction with `hound` (HOUNDS), `mate`
    // (IDLERS, player's teammate), or `player` (PLAYER_FACTION): Neutral to all three.
    let (hound, trooper, mate, player) = (FormId(HOUND_REF), FormId(HELPER_REF), FormId(LOAFER_REF), FormId::PLAYER_REF);
    only_near(&mut g, &[HOUND_REF, HELPER_REF, LOAFER_REF]);
    // Give the bystander trooper Aggressive, Foolhardy, Helps Friends and Allies
    // (matching HeliosExtLvlNCRTrooperGun), so `stays_out` cannot mask the aggro-radius check.
    const SET_AV: u16 = 0x100F;
    call(&mut g, SET_AV, HELPER_REF, &[ArgValue::Enum(av::AGGRESSION), ArgValue::Value(Value::Num(1.0))]);
    call(&mut g, SET_AV, HELPER_REF, &[ArgValue::Enum(av::CONFIDENCE), ArgValue::Value(Value::Num(4.0))]);
    call(&mut g, SET_AV, HELPER_REF, &[ArgValue::Enum(av::ASSISTANCE), ArgValue::Value(Value::Num(2.0))]);
    g.extra.entry(mate).or_default().teammate = true;
    g.world.get(hound).unwrap().pos = [0.0, 0.0, 0.0];
    g.world.get(player).unwrap().pos = [1200.0, 0.0, 0.0];
    g.world.get(mate).unwrap().pos = [1100.0, 0.0, 0.0];
    g.world.get(trooper).unwrap().pos = [2123.0, 0.0, 0.0];

    let seen = Detection { value: 20.0, band: Band::Detected, ..Default::default() };
    // Hound enters Combat against the player (its Enemy).
    g.observe_threat(hound, player, &seen, [1200.0, 0.0, 0.0]);
    g.settle_awareness();
    assert_eq!(g.awareness.state(hound), State::Combat);
    assert_eq!(g.combat_target_of(hound), Some(player));

    // While fighting the player, the hound still leaves the Neutral trooper at 2,123 units
    // (outside its 2,000-unit aggro radius) off its target list.
    g.observe_threat(hound, trooper, &seen, [2123.0, 0.0, 0.0]);
    assert!(
        g.awareness.get(hound).is_some_and(|a| !a.targets.contains_key(&trooper)),
        "combat with the player must not lift the aggro-radius bound for an unrelated Neutral bystander"
    );

    // The trooper detects the hound fighting the player (and the companion defending) at
    // 2,123 units: outside the hound's aggro radius, the trooper stays out and sends no
    // alarm about the hound.
    g.detection.pairs.insert((trooper, hound), (seen, g.time));
    g.detection.pairs.insert((trooper, player), (seen, g.time));
    g.detection.pairs.insert((trooper, mate), (seen, g.time));
    fighting(&mut g, LOAFER_REF, HOUND_REF);
    assert_eq!(g.call_allies(), 0, "bystander outside aggro radius must not join against the hound");
    assert!(!g.provocations.holds(trooper, hound));
    assert!(!g.would_attack(trooper, hound));
    g.settle_awareness();
    assert_eq!(g.awareness.state(trooper), State::Normal);
    let near: std::collections::BTreeMap<FormId, [f32; 3]> =
        [(hound, [0.0, 0.0, 0.0]), (trooper, [2123.0, 0.0, 0.0]), (player, [1200.0, 0.0, 0.0])].into_iter().collect();
    g.awareness.changes.clear();
    assert_eq!(g.raise_alarms(&near), 0);

    // Control: once the bystander stands inside the hound's 2,000-unit aggro radius
    // (as Sunny Smiles beside geckos), `join_against_dangers` still lets it join at once.
    g.world.get(trooper).unwrap().pos = [900.0, 0.0, 0.0];
    assert_eq!(g.call_allies(), 1, "bystander inside aggro radius joins against the danger");
    assert!(g.provocations.holds(trooper, hound));
}

#[test]
fn shared_faction_bridge_does_not_relay_assist_fight_to_neutral_third_party_faction() {
    // #3459: HELIOS One topology — `hound` (CREA in HOUNDS, Enemy of player) alarms
    // `bridge` (CREA in both HOUNDS and RATS, Neutral to player, Aggressive, Helps Allies),
    // while `trooper` (NPC in RATS only, Neutral to both hound and player, Helps Allies)
    // stands nearby. `bridge` takes up `hound`'s fight against the player (one hop, matching
    // #2927), but when `bridge` enters Combat against the player (or the player's teammate),
    // neither `raise_alarms` nor `call_allies` relays that assist fight to `trooper`.
    // Meanwhile, a same-pack creature comrade (`pack_mate`, also in HOUNDS) and normal human
    // NPC assist relays (OWN_REF assisting BANDIT_REF) still relay normally.
    use fnv_game::awareness::Change;
    let mut f = fixture();
    f.data.faction_reactions.entry(FormId(HOUNDS)).or_default().push((FormId(HOUNDS), 2));
    let mut g = game(&f);
    let (hound, bridge, pack_mate, trooper, mate, player) = (FormId(HOUND_REF), FormId(LURKER_REF), FormId(COWARD_REF), FormId(HELPER_REF), FormId(LOAFER_REF), FormId::PLAYER_REF);
    only_near(&mut g, &[HOUND_REF, LURKER_REF, COWARD_REF, HELPER_REF, LOAFER_REF]);
    // `hound` is a CREA in HOUNDS + RAIDERS (Enemy of player, Neutral to RATS), matching
    // CrDogNCRAggressive; `bridge` (LURKER_REF) is a CREA in HOUNDS + RATS (Ally of hound,
    // Ally of trooper, Neutral to player, Aggressive, Brave, Helps Friends and Allies),
    // matching CrDogNCRFollow; `pack_mate` (COWARD_REF) is a CREA in HOUNDS placed out of
    // hound's alarm range so it only hears `bridge`.
    const SET_AV: u16 = 0x100F;
    call(&mut g, ADD_TO_FACTION, HOUND_REF, &[form(RAIDERS), num(0.0)]);
    call(&mut g, ADD_TO_FACTION, LURKER_REF, &[form(HOUNDS), num(0.0)]);
    call(&mut g, SET_AV, LURKER_REF, &[ArgValue::Enum(av::AGGRESSION), ArgValue::Value(Value::Num(1.0))]);
    call(&mut g, SET_AV, LURKER_REF, &[ArgValue::Enum(av::CONFIDENCE), ArgValue::Value(Value::Num(3.0))]);
    call(&mut g, SET_AV, LURKER_REF, &[ArgValue::Enum(av::ASSISTANCE), ArgValue::Value(Value::Num(2.0))]);
    call(&mut g, ADD_TO_FACTION, COWARD_REF, &[form(HOUNDS), num(0.0)]);
    call(&mut g, SET_AV, COWARD_REF, &[ArgValue::Enum(av::AGGRESSION), ArgValue::Value(Value::Num(1.0))]);
    call(&mut g, SET_AV, COWARD_REF, &[ArgValue::Enum(av::CONFIDENCE), ArgValue::Value(Value::Num(3.0))]);
    call(&mut g, SET_AV, COWARD_REF, &[ArgValue::Enum(av::ASSISTANCE), ArgValue::Value(Value::Num(2.0))]);
    call(&mut g, SET_AV, HELPER_REF, &[ArgValue::Enum(av::AGGRESSION), ArgValue::Value(Value::Num(1.0))]);
    call(&mut g, SET_AV, HELPER_REF, &[ArgValue::Enum(av::CONFIDENCE), ArgValue::Value(Value::Num(4.0))]);
    call(&mut g, SET_AV, HELPER_REF, &[ArgValue::Enum(av::ASSISTANCE), ArgValue::Value(Value::Num(2.0))]);
    g.extra.entry(mate).or_default().teammate = true;
    g.world.get(hound).unwrap().pos = [0.0, 0.0, 0.0];
    g.world.get(bridge).unwrap().pos = [6000.0, 0.0, 0.0];
    g.world.get(pack_mate).unwrap().pos = [10000.0, 0.0, 0.0];
    g.world.get(trooper).unwrap().pos = [6200.0, 0.0, 0.0];
    g.world.get(player).unwrap().pos = [5600.0, 0.0, 0.0];
    g.world.get(mate).unwrap().pos = [5650.0, 0.0, 0.0];

    assert!(!g.would_attack(bridge, player) && !g.would_attack(trooper, player) && !g.would_attack(pack_mate, player));
    assert!(!g.would_attack(trooper, hound));

    // Step 1: hound enters Combat against the player and alarms its HOUNDS ally `bridge`
    // (within ALARM_RADIUS), not `trooper` (Neutral) and not `pack_mate` (out of hound's
    // 9,216-unit alarm radius).
    let seen = Detection { value: 85.0, band: Band::Detected, ..Default::default() };
    let near: std::collections::BTreeMap<FormId, [f32; 3]> =
        [(hound, [0.0, 0.0, 0.0]), (bridge, [6000.0, 0.0, 0.0]), (pack_mate, [10000.0, 0.0, 0.0]), (trooper, [6200.0, 0.0, 0.0]), (player, [5600.0, 0.0, 0.0]), (mate, [5650.0, 0.0, 0.0])].into_iter().collect();
    g.observe_threat(hound, player, &seen, [5600.0, 0.0, 0.0]);
    g.settle_awareness();
    assert_eq!(g.awareness.state(hound), State::Combat);
    assert_eq!(g.raise_alarms(&near), 1, "hound alarms bridge (shared HOUNDS)");
    assert!(g.would_attack(bridge, player));
    assert!(!g.would_attack(trooper, player), "trooper is Neutral to hound and must not be alarmed");
    assert!(!g.would_attack(pack_mate, player), "pack_mate is beyond hound's alarm radius");

    // Step 2: bridge later detects the player and enters Combat (`Alert -> Combat`, cause None).
    g.awareness.changes.clear();
    g.settle_awareness();
    assert_eq!(g.awareness.state(bridge), State::Alert);
    g.awareness.changes.clear();
    g.observe_threat(bridge, player, &seen, [5600.0, 0.0, 0.0]);
    g.observe_threat(bridge, mate, &seen, [5650.0, 0.0, 0.0]);
    g.settle_awareness();
    assert_eq!(g.awareness.state(bridge), State::Combat);

    // Bridge's hostility reason names the assist grudge ("helps ... against the player")
    // rather than claiming it was attacked, both for the player and for the teammate.
    let why_player = g.hostility_text(bridge, player).expect("bridge hostile to player");
    assert!(why_player.contains("helps") && why_player.contains("against the player"), "unexpected hostility text: {why_player}");
    let why_mate = g.hostility_text(bridge, mate).expect("bridge hostile to teammate");
    assert!(why_mate.contains("helps") && why_mate.contains("against the player"), "unexpected teammate hostility text: {why_mate}");

    // Step 3: bridge relays to same-pack `pack_mate` (comrade of `hound`), but must NOT
    // relay the alarm across factions to its RATS ally `trooper` (Neutral to `hound`).
    assert_eq!(g.raise_alarms(&near), 1, "bridge relays to same-pack comrade only, not neutral third-party faction");
    assert!(g.would_attack(pack_mate, player), "same-pack creature comrade still receives relay");
    assert!(!g.would_attack(trooper, player));
    assert!(!g.would_attack(trooper, mate));

    // Step 4: `trooper` directly watching `bridge` fight the player (and teammate, even
    // after teammate hits `bridge` in self-defence) must NOT join via `call_allies`.
    g.hurt_by(bridge, mate, [650.0, 0.0, 0.0]);
    g.detection.pairs.insert((trooper, bridge), (seen, g.time));
    assert_eq!(g.call_allies(), 0, "trooper must not transitively join bridge's assisted fight");
    assert!(!g.would_attack(trooper, player));
    assert!(!g.would_attack(trooper, mate));

    // Control 1: if the player directly assaults a calm `bridge` (`Why::Assault`), `bridge`
    // DOES alarm `trooper` and its hostility text says "(it was attacked)".
    let mut g2 = game(&f);
    only_near(&mut g2, &[LURKER_REF, HELPER_REF]);
    call(&mut g2, ADD_TO_FACTION, LURKER_REF, &[form(HOUNDS), num(0.0)]);
    call(&mut g2, SET_AV, LURKER_REF, &[ArgValue::Enum(av::AGGRESSION), ArgValue::Value(Value::Num(1.0))]);
    call(&mut g2, SET_AV, LURKER_REF, &[ArgValue::Enum(av::CONFIDENCE), ArgValue::Value(Value::Num(3.0))]);
    call(&mut g2, SET_AV, LURKER_REF, &[ArgValue::Enum(av::ASSISTANCE), ArgValue::Value(Value::Num(2.0))]);
    call(&mut g2, SET_AV, HELPER_REF, &[ArgValue::Enum(av::AGGRESSION), ArgValue::Value(Value::Num(1.0))]);
    call(&mut g2, SET_AV, HELPER_REF, &[ArgValue::Enum(av::CONFIDENCE), ArgValue::Value(Value::Num(4.0))]);
    call(&mut g2, SET_AV, HELPER_REF, &[ArgValue::Enum(av::ASSISTANCE), ArgValue::Value(Value::Num(2.0))]);
    assert!(g2.attacked_by(bridge, player));
    let why_assault = g2.hostility_text(bridge, player).expect("assaulted bridge hostile to player");
    assert!(why_assault.contains("it was attacked"), "unexpected assault hostility text: {why_assault}");
    g2.observe_threat(bridge, player, &seen, [600.0, 0.0, 0.0]);
    g2.settle_awareness();
    let near2: std::collections::BTreeMap<FormId, [f32; 3]> =
        [(bridge, [1000.0, 0.0, 0.0]), (trooper, [1200.0, 0.0, 0.0]), (player, [600.0, 0.0, 0.0])].into_iter().collect();
    assert!(g2.raise_alarms(&near2) >= 1, "directly assaulted bridge alarms its RATS ally");
    assert!(g2.awareness.get(trooper).is_some_and(|a| a.alarms.iter().any(|x| x.owner == player && x.cause == Cause::Ally)));
    assert!(g2.would_attack(trooper, player));

    // Control 2: normal human NPC assist relays are completely unaffected (not narrowed out).
    let mut g3 = game(&f);
    let (raider_npc, bridge_npc) = (FormId(BANDIT_REF), FormId(OWN_REF));
    only_near(&mut g3, &[BANDIT_REF, OWN_REF, HELPER_REF]);
    call(&mut g3, ADD_TO_FACTION, OWN_REF, &[form(RAIDERS), num(0.0)]);
    call(&mut g3, SET_AV, OWN_REF, &[ArgValue::Enum(av::AGGRESSION), ArgValue::Value(Value::Num(1.0))]);
    call(&mut g3, SET_AV, OWN_REF, &[ArgValue::Enum(av::CONFIDENCE), ArgValue::Value(Value::Num(3.0))]);
    call(&mut g3, SET_AV, OWN_REF, &[ArgValue::Enum(av::ASSISTANCE), ArgValue::Value(Value::Num(2.0))]);
    call(&mut g3, SET_AV, HELPER_REF, &[ArgValue::Enum(av::AGGRESSION), ArgValue::Value(Value::Num(1.0))]);
    call(&mut g3, SET_AV, HELPER_REF, &[ArgValue::Enum(av::CONFIDENCE), ArgValue::Value(Value::Num(4.0))]);
    call(&mut g3, SET_AV, HELPER_REF, &[ArgValue::Enum(av::ASSISTANCE), ArgValue::Value(Value::Num(2.0))]);
    g3.world.get(raider_npc).unwrap().pos = [0.0, 0.0, 0.0];
    g3.world.get(bridge_npc).unwrap().pos = [1000.0, 0.0, 0.0];
    g3.world.get(trooper).unwrap().pos = [1200.0, 0.0, 0.0];
    g3.world.get(player).unwrap().pos = [600.0, 0.0, 0.0];
    let near3: std::collections::BTreeMap<FormId, [f32; 3]> =
        [(raider_npc, [0.0, 0.0, 0.0]), (bridge_npc, [1000.0, 0.0, 0.0]), (trooper, [1200.0, 0.0, 0.0]), (player, [600.0, 0.0, 0.0])].into_iter().collect();
    g3.awareness.changes.push(Change { actor: raider_npc, from: State::Normal, to: State::Alert, time: g3.time, about: Some(player), cause: Some(Cause::Noticed), at: Some([600.0, 0.0, 0.0]), value: Some(-10.0) });
    assert_eq!(g3.raise_alarms(&near3), 1, "raider NPC alarms bridge NPC");
    g3.awareness.changes.clear();
    g3.settle_awareness();
    g3.awareness.changes.clear();
    g3.observe_threat(bridge_npc, player, &seen, [600.0, 0.0, 0.0]);
    g3.settle_awareness();
    assert_eq!(g3.awareness.state(bridge_npc), State::Combat);
    assert!(g3.raise_alarms(&near3) >= 1, "human NPC assist relay remains unaffected");
    assert!(g3.would_attack(trooper, player));
}

#[test]
fn hostility_text_names_every_grudge_kind_and_neutral_aggression() {
    // #3459: every Combat transition against a non-enemy logs its specific reason:
    // Assault, Assist (helps X against Y), Aggro (aggro radius), and Aggression on Neutral.
    let f = fixture();
    let mut g = game(&f);
    let (own, helper, hound, ganger, player) = (FormId(OWN_REF), FormId(HELPER_REF), FormId(HOUND_REF), FormId(GANGER_B_REF), FormId::PLAYER_REF);

    // 1. Neutral attacked by Very Aggressive actor.
    let neutral_text = g.hostility_text(hound, ganger).expect("Very Aggressive vs Neutral");
    assert!(neutral_text.contains("Aggression 2") && neutral_text.contains("neutral (0)"), "{neutral_text}");

    // 2. Assault grudge.
    assert!(g.attacked_by(own, player));
    let assault_text = g.hostility_text(own, player).expect("assault grudge");
    assert!(assault_text.contains("it was attacked"), "{assault_text}");

    // 3. Assist grudge (from call_allies / raise_alarms).
    fighting(&mut g, OWN_REF, PLAYER);
    g.detection.pairs.insert((helper, own), (Detection { value: 50.0, band: Band::Detected, ..Default::default() }, g.time));
    assert_eq!(g.call_allies(), 1);
    let assist_text = g.hostility_text(helper, player).expect("assist grudge");
    assert!(assist_text.contains("helps") && assist_text.contains("against the player"), "{assist_text}");

    // 4. Aggro-radius grudge.
    let lurker = FormId(LURKER_REF);
    g.provocations.grudges.insert((lurker, player), fnv_game::provocation::Grudge { why: fnv_game::provocation::Why::Aggro, since: g.time });
    let aggro_text = g.hostility_text(lurker, player).expect("aggro grudge");
    assert!(aggro_text.contains("stayed inside its aggro radius"), "{aggro_text}");
}
