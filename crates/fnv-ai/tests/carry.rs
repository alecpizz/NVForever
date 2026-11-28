//! Carrying packages out (`fnv_ai`) on a plugin and navmeshes built in memory (no game
//! data): two interior cells joined by a door, markers, and actors with one package
//! each. The rules and their sources: docs/design/actor-behaviour.md §5.

use fnv_ai::{Activity, Ai, AiConfig, AiEvent, Doing};
use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_game::ai::{AiPhase, SandboxPhase};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig};
use fnv_game::package::{flags, type_flags, LocationKind, PackageCheck};
use fnv_game::GameData;
use fnv_nav::graph::DoorLock;
use fnv_nav::record::{tri_flags, DoorLink, NavMeshRecord, Triangle};
use fnv_nav::{NavGraph, PathParams, Place, Space};
use fnv_script::FunctionTable;
use fnv_sim::furniture_facts::{ActorKind, Fit, FurnitureFacts};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::sync::Arc;

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

/// A package without schedule or conditions: flags, type, location (type, value,
/// radius) and, for a patrol, whether it repeats.
fn pack(id: u32, edid: &str, general: u32, ty: u8, location: (i32, u32, i32), repeatable: Option<bool>) -> Vec<u8> {
    pack_with(id, edid, general, ty, 0, location, repeatable)
}

/// [`pack`] with the type-specific flags (a sandbox's "No ..." switches).
fn pack_with(id: u32, edid: &str, general: u32, ty: u8, specific: u16, location: (i32, u32, i32), repeatable: Option<bool>) -> Vec<u8> {
    let mut subs = vec![
        sub(b"EDID", z(edid)),
        sub(b"PKDT", [&general.to_le_bytes()[..], &[ty, 0, 0, 0], &specific.to_le_bytes(), &[0, 0]].concat()),
        sub(b"PLDT", [location.0.to_le_bytes(), location.1.to_le_bytes(), location.2.to_le_bytes()].concat()),
        sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat()),
    ];
    subs.extend(repeatable.map(|r| sub(b"PKPT", vec![r as u8, 0])));
    record(b"PACK", id, &subs)
}

/// An Accompany package (type 7) whose target is a reference, kept within `distance`.
fn accompany(id: u32, edid: &str, target: u32, distance: i32) -> Vec<u8> {
    let subs = [
        sub(b"EDID", z(edid)),
        sub(b"PKDT", [&0u32.to_le_bytes()[..], &[7, 0, 0, 0], &[0; 4]].concat()),
        sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat()),
        sub(b"PTDT", [0i32.to_le_bytes(), target.to_le_bytes(), distance.to_le_bytes()].concat()),
    ];
    record(b"PACK", id, &subs)
}

/// A Use Weapon package (type 16): stand near `stand`, shoot at the reference `target`
/// two volleys of one shot, half a second apart, always hitting and doing no damage
/// (PKW3 [documented: xEdit PACK PKW3]); the weapon is the object type "Weapons: None".
fn shoot(id: u32, edid: &str, stand: u32, target: u32) -> Vec<u8> {
    let pkw3 = [&0x0000_0101u32.to_le_bytes()[..], &[1, 0], &2u16.to_le_bytes(), &1u16.to_le_bytes(), &1u16.to_le_bytes(), &0.5f32.to_le_bytes(), &0.5f32.to_le_bytes(), &[0; 4]].concat();
    let subs = [
        sub(b"EDID", z(edid)),
        sub(b"PKDT", [&0u32.to_le_bytes()[..], &[16, 0, 0, 0], &[0; 4]].concat()),
        sub(b"PLDT", [0i32.to_le_bytes(), stand.to_le_bytes(), 0i32.to_le_bytes()].concat()),
        sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat()),
        sub(b"PTDT", [2i32.to_le_bytes(), 23u32.to_le_bytes(), 0i32.to_le_bytes()].concat()),
        sub(b"PKW3", pkw3),
        sub(b"PTD2", [0i32.to_le_bytes(), target.to_le_bytes(), 0i32.to_le_bytes()].concat()),
    ];
    record(b"PACK", id, &subs)
}

fn npc(fid: u32, edid: &str, packages: &[u32]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid))];
    subs.extend(packages.iter().map(|p| sub(b"PKID", p.to_le_bytes().to_vec())));
    record(b"NPC_", fid, &subs)
}

fn placed(sig: &[u8; 4], fid: u32, edid: &str, base: u32, pos: [f32; 3], linked: Option<u32>) -> Vec<u8> {
    placed_with(sig, fid, edid, base, pos, 0.0, linked, None)
}

/// [`placed`] with a heading (z rotation, radians) and a patrol idle time (XPRD); a
/// patrol point with an idle time also has patrol data (XPPA) naming [`PATROL_TOPIC`].
#[allow(clippy::too_many_arguments)]
fn placed_with(sig: &[u8; 4], fid: u32, edid: &str, base: u32, pos: [f32; 3], heading: f32, linked: Option<u32>, idle: Option<f32>) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec())];
    subs.extend(idle.map(|s| sub(b"XPRD", s.to_le_bytes().to_vec())));
    if idle.is_some() {
        subs.push(sub(b"XPPA", Vec::new()));
        subs.push(sub(b"TNAM", PATROL_TOPIC.to_le_bytes().to_vec()));
    }
    subs.extend(linked.map(|l| sub(b"XLKR", l.to_le_bytes().to_vec())));
    subs.push(sub(b"DATA", [pos[0].to_le_bytes(), pos[1].to_le_bytes(), pos[2].to_le_bytes(), [0; 4], [0; 4], heading.to_le_bytes()].concat()));
    record(sig, fid, &subs)
}

/// A topic no line belongs to: saying it shows as `EventKind::NoLine`.
const PATROL_TOPIC: u32 = 0xF0F;
const G_HOUR: u32 = 0x804;
const G_DAYS: u32 = 0x805;
const STAT: u32 = 0x810;
const CHAIR: u32 = 0x811;
const BED: u32 = 0x812;
const IDLE_MARKER: u32 = 0x813;
// Packages.
const P_TRAVEL_B: u32 = 0x820;
const P_DEFAULT: u32 = 0x821;
const P_IN_B: u32 = 0x822;
const P_RADIUS: u32 = 0x823;
const P_PATROL: u32 = 0x824;
const P_LOOP: u32 = 0x825;
const P_FOLLOW: u32 = 0x826;
const P_HERE: u32 = 0x827;
const P_RUN: u32 = 0x828;
const P_SB_ALL: u32 = 0x829;
const P_SB_WANDER: u32 = 0x82A;
const P_SB_OBJECTS: u32 = 0x82B;
const P_SB_NOTHING: u32 = 0x82C;
const P_TO_CHAIR: u32 = 0x82D;
const P_SLEEP_C: u32 = 0x82E;
const P_STEADY: u32 = 0x82F;
const P_ACCOMPANY: u32 = 0x830;
const P_WITH_PLAYER: u32 = 0x831;
const P_SHOOT: u32 = 0x832;
const P_NEAR_OBJECT: u32 = 0x833;
const P_GUARD_NEAR: u32 = 0x834;
const P_HUNT: u32 = 0x835;
// Bases.
const N_TRAVEL: u32 = 0x840;
const N_IN: u32 = 0x841;
const N_RADIUS: u32 = 0x842;
const N_PATROL: u32 = 0x843;
const N_LOOP: u32 = 0x844;
const N_FOLLOW: u32 = 0x845;
const N_HERE: u32 = 0x846;
const N_RUN: u32 = 0x847;
const N_SB_ALL: u32 = 0x848;
const N_SB_WANDER: u32 = 0x849;
const N_SB_OBJECTS: u32 = 0x84A;
const N_SB_NOTHING: u32 = 0x84B;
const N_TO_CHAIR: u32 = 0x84C;
const N_SLEEP_C: u32 = 0x84D;
const N_STEADY: u32 = 0x84E;
const N_DOG: u32 = 0x84F;
const N_COMPANION: u32 = 0x850;
const N_LONER: u32 = 0x851;
const N_SHOOTER: u32 = 0x852;
const N_SEEKER: u32 = 0x853;
const N_KEEPER: u32 = 0x854;
const N_HUNTER: u32 = 0x855;
// Cells and references.
const CELL_A: u32 = 0x860;
const CELL_B: u32 = 0x861;
const CELL_C: u32 = 0x862;
const MARK_C: u32 = 0x890;
const CHAIR_C: u32 = 0x891;
const BED_C: u32 = 0x892;
const IDLE_C: u32 = 0x893;
const R_SB_ALL: u32 = 0x894;
const R_SB_WANDER: u32 = 0x895;
const R_SB_OBJECTS: u32 = 0x896;
const R_SB_OBJECTS_2: u32 = 0x897;
const R_SB_NOTHING: u32 = 0x898;
const R_TO_CHAIR: u32 = 0x899;
const R_SLEEP_C: u32 = 0x89A;
const MARK_B: u32 = 0x870;
const MARK_A: u32 = 0x871;
const M1: u32 = 0x872;
const M2: u32 = 0x873;
const M3: u32 = 0x874;
const L1: u32 = 0x875;
const L2: u32 = 0x876;
const L3: u32 = 0x877;
const R_TRAVEL: u32 = 0x880;
const R_IN: u32 = 0x881;
const R_RADIUS: u32 = 0x882;
const R_PATROL: u32 = 0x883;
const R_LOOP: u32 = 0x884;
const R_FOLLOW: u32 = 0x885;
const R_HERE: u32 = 0x886;
const R_RUN: u32 = 0x887;
const R_STEADY: u32 = 0x888;
const R_DOG: u32 = 0x889;
const R_COMPANION: u32 = 0x89B;
const R_LONER: u32 = 0x89C;
const R_SHOOTER: u32 = 0x89D;
const R_DOG2: u32 = 0x89E;
const R_SEEKER: u32 = 0x89F;
const R_KEEPER: u32 = 0x8A0;
const R_HUNTER: u32 = 0x8A1;
const DOOR_A: u32 = 0x8D1;
const DOOR_B: u32 = 0x8D2;
const SCRIPT_DOOR_BASE: u32 = 0xF10;
const SCRIPT_DOOR: u32 = 0xF11;
const SCRIPT_DEST_DOOR: u32 = 0xF12;
const SCRIPT_DEST: [f32; 3] = [5200.0, 5200.0, 50.0];
// Cell D (furniture eligibility, issue #1522): 10 x 10 squares from (20000, 20000).
const CELL_D: u32 = 0x8E0;
const MARK_D: u32 = 0x8E1;
const CHAIR_NEAR: u32 = 0x8E2;
const CHAIR_FAR: u32 = 0x8E3;
const BED_D: u32 = 0x8E4;
const CROSS_D: u32 = 0x8E5;
const CROSS_LINK: u32 = 0x8E6;
const R_HOUND: u32 = 0x8E7;
const R_PERSON_D: u32 = 0x8E8;
const R_VICTIM: u32 = 0x8E9;
const R_HOUND_LINKED: u32 = 0x8EA;
const R_EATER_CHAIRS: u32 = 0x8EB;
const R_EATER_CROSS: u32 = 0x8EC;
const R_HOUND_MAN: u32 = 0x8ED;
const R_HOUND_COPY: u32 = 0x8EE;
const R_LEVELED_MAN: u32 = 0x8EF;
const R_MAN_HOUND: u32 = 0x8A2;
const CHAIR_2: u32 = 0x8C0;
const CROSS: u32 = 0x8C1;
const P_SB_D: u32 = 0x8C2;
const P_TO_LINKED: u32 = 0x8C3;
const P_EAT_D: u32 = 0x8C4;
const N_HOUND: u32 = 0x8C5;
const N_PERSON_D: u32 = 0x8C6;
const N_VICTIM: u32 = 0x8C7;
const N_HOUND_LINKED: u32 = 0x8C8;
const N_EATER: u32 = 0x8C9;
const N_HOUND_MAN: u32 = 0x8CA;
const N_HOUND_COPY: u32 = 0x8CB;
const N_LEVELED_MAN: u32 = 0x8CC;
const N_MODELLED: u32 = 0x8CD;
const N_MAN_HOUND: u32 = 0x8CE;
const PERSON_MODEL: &str = r"Characters\_Male\Skeleton.nif";
const HOUND_MODEL: &str = r"Creatures\Hound\Skeleton.nif";

const TRAVEL: u8 = 6;
const EAT: u8 = 3;
const SLEEP: u8 = 4;
const SANDBOX: u8 = 12;
const PATROL: u8 = 13;
const FOLLOW: u8 = 1;
const GUARD: u8 = 14;
const START: [f32; 3] = [50.0, 50.0, 0.0];
/// An xMarkerHeading (the engine's base 0x34) facing 2.0 rad, the package that sends
/// an actor to it, the actor's base and the actor.
const HEAD_MARK: u32 = 0x8B0;
const P_TO_HEADING: u32 = 0x8B1;
const N_TO_HEADING: u32 = 0x8B2;
const R_TO_HEADING: u32 = 0x8B3;
/// A sandbox's switches for everything it does with objects and for talking.
const NO_OBJECTS: u16 = type_flags::NO_EATING | type_flags::NO_SLEEPING | type_flags::NO_CONVERSATION | type_flags::NO_IDLE_MARKERS | type_flags::NO_FURNITURE;
const C: Space = Space::Interior(CELL_C);
const MIDDLE_C: [f32; 3] = [10500.0, 10500.0, 0.0];

fn plugin_bytes() -> Vec<u8> {
    let globs = [
        glob(0x801, "GameYear", 2281.0),
        glob(0x802, "GameMonth", 9.0),
        glob(0x803, "GameDay", 19.0),
        glob(0x804, "GameHour", 12.0),
        glob(G_DAYS, "GameDaysPassed", 5.0),
        glob(0x806, "TimeScale", 30.0),
        // Counted by the tests' extender handlers.
        glob(0x807, "TestGlobal", 0.0),
    ]
    .concat();
    let packs = [
        pack(P_TRAVEL_B, "TravelToB", flags::MUST_REACH_LOCATION, TRAVEL, (0, MARK_B, 0), None),
        pack(P_DEFAULT, "StayHome", 0, TRAVEL, (3, 0, 64), None),
        pack(P_IN_B, "TravelIntoB", 0, TRAVEL, (1, CELL_B, 0), None),
        pack(P_RADIUS, "TravelNearA", 0, TRAVEL, (0, MARK_A, 150), None),
        pack(P_TO_HEADING, "TravelToHeading", 0, TRAVEL, (0, HEAD_MARK, 0), None),
        pack(P_PATROL, "PatrolOnce", 0, PATROL, (0, M1, 0), Some(false)),
        // Sent after the player: a Travel package near the player's reference.
        pack(P_HUNT, "HuntThePlayer", 0, TRAVEL, (0, 0x14, 0), None),
        pack(P_LOOP, "PatrolRound", 0, PATROL, (6, 0, 0), Some(true)),
        pack(P_FOLLOW, "FollowSomeone", 0, FOLLOW, (3, 0, 0), None),
        pack(P_HERE, "TravelHere", flags::MUST_COMPLETE, TRAVEL, (2, 0, 0), None),
        pack(P_RUN, "RunToA", flags::ALWAYS_RUN, TRAVEL, (0, MARK_A, 0), None),
        // Sandboxes around the middle of cell C, 450 units: everything allowed; only
        // wandering; only furniture and idle markers; nothing.
        pack(P_SB_ALL, "SandboxAll", 0, SANDBOX, (0, MARK_C, 450), None),
        pack_with(P_SB_WANDER, "SandboxWander", 0, SANDBOX, NO_OBJECTS, (0, MARK_C, 450), None),
        pack_with(P_SB_OBJECTS, "SandboxObjects", 0, SANDBOX, type_flags::NO_WANDERING | type_flags::NO_EATING | type_flags::NO_SLEEPING, (0, MARK_C, 450), None),
        pack_with(P_SB_NOTHING, "SandboxNothing", 0, SANDBOX, NO_OBJECTS | type_flags::NO_WANDERING, (0, MARK_C, 450), None),
        pack(P_TO_CHAIR, "TravelToChair", 0, TRAVEL, (0, CHAIR_C, 0), None),
        pack(P_SLEEP_C, "SleepInC", 0, SLEEP, (1, CELL_C, 0), None),
        pack(P_STEADY, "TravelToBInCombat", flags::MUST_REACH_LOCATION | flags::CONTINUE_DURING_COMBAT, TRAVEL, (0, MARK_B, 0), None),
        pack(P_SB_D, "SandboxInD", 0, SANDBOX, (0, MARK_D, 450), None),
        pack(P_TO_LINKED, "TravelNearLinked", 0, TRAVEL, (6, 0, 64), None),
        pack(P_EAT_D, "EatWhereItStands", 0, EAT, (3, 0, 64), None),
        accompany(P_ACCOMPANY, "AccompanyTheTraveller", R_TRAVEL, 128),
        accompany(P_WITH_PLAYER, "AccompanyThePlayer", 0x14, 256),
        shoot(P_SHOOT, "ShootAtTheLoner", MARK_A, R_LONER),
        // Travel near an object: the idle marker's base (location type 4, Near Object ID).
        pack(P_NEAR_OBJECT, "TravelNearAnIdleMarker", 0, TRAVEL, (4, IDLE_MARKER, 100), None),
        // A Guard staying near the reference it guards (Remain Near Reference to Guard),
        // its location only "near current location".
        record(
            b"PACK",
            P_GUARD_NEAR,
            &[
                sub(b"EDID", z("GuardTheMiddle")),
                sub(b"PKDT", [&0u32.to_le_bytes()[..], &[GUARD, 0, 0, 0], &type_flags::REMAIN_NEAR_REFERENCE.to_le_bytes(), &[0, 0]].concat()),
                sub(b"PLDT", [2i32.to_le_bytes(), 0u32.to_le_bytes(), 0i32.to_le_bytes()].concat()),
                sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat()),
                sub(b"PTDT", [0i32.to_le_bytes(), MARK_C.to_le_bytes(), 120i32.to_le_bytes()].concat()),
            ],
        ),
    ]
    .concat();
    let npcs = [
        npc(0x7, "Player", &[]),
        npc(N_TRAVEL, "Traveller", &[P_TRAVEL_B, P_DEFAULT]),
        npc(N_IN, "Lodger", &[P_IN_B]),
        npc(N_RADIUS, "Visitor", &[P_RADIUS]),
        npc(N_TO_HEADING, "Teacher", &[P_TO_HEADING]),
        npc(N_PATROL, "Guard", &[P_PATROL]),
        npc(N_LOOP, "Sentry", &[P_LOOP]),
        npc(N_FOLLOW, "Follower", &[P_FOLLOW]),
        npc(N_HERE, "Stander", &[P_HERE]),
        npc(N_RUN, "Runner", &[P_RUN]),
        npc(N_SB_ALL, "Idler", &[P_SB_ALL]),
        npc(N_SB_WANDER, "Wanderer", &[P_SB_WANDER]),
        npc(N_SB_OBJECTS, "Sitter", &[P_SB_OBJECTS]),
        npc(N_SB_NOTHING, "Statue", &[P_SB_NOTHING]),
        npc(N_TO_CHAIR, "ChairUser", &[P_TO_CHAIR]),
        npc(N_SLEEP_C, "Sleeper", &[P_SLEEP_C]),
        // ACBS flag "No Low Level Processing" (bit 9).
        record(b"NPC_", N_STEADY, &[sub(b"EDID", z("Courier")), sub(b"ACBS", [0x200u32.to_le_bytes().to_vec(), vec![0; 20]].concat()), sub(b"PKID", P_STEADY.to_le_bytes().to_vec())]),
        npc(N_DOG, "Dog", &[P_ACCOMPANY]),
        npc(N_COMPANION, "Companion", &[P_WITH_PLAYER]),
        npc(N_LONER, "Loner", &[]),
        npc(N_SHOOTER, "Shooter", &[P_SHOOT]),
        npc(N_SEEKER, "Seeker", &[P_NEAR_OBJECT]),
        npc(N_KEEPER, "Keeper", &[P_GUARD_NEAR]),
        npc(N_HUNTER, "Hunter", &[P_HUNT]),
        npc(N_PERSON_D, "PersonInD", &[P_SB_D]),
        npc(N_VICTIM, "Victim", &[P_TO_LINKED]),
        npc(N_EATER, "Eater", &[P_EAT_D]),
    ]
    .concat();
    // Creatures with a skeleton of their own.
    let hound = |id: u32, edid: &str, package: u32| record(b"CREA", id, &[sub(b"EDID", z(edid)), sub(b"MODL", z(HOUND_MODEL)), sub(b"PKID", package.to_le_bytes().to_vec())]);
    // ACBS with the template flag "Model/Animation" [documented: xEdit]: what the template
    // gives is its skeleton; its own MODL says nothing.
    let by_template = |sig: &[u8; 4], id: u32, edid: &str, template: u32| {
        let acbs = [&0u32.to_le_bytes()[..], &[0; 18], &0x40u16.to_le_bytes()].concat();
        record(sig, id, &[sub(b"EDID", z(edid)), sub(b"ACBS", acbs), sub(b"TPLT", template.to_le_bytes().to_vec()), sub(b"MODL", z("marker_placeholder.nif")), sub(b"PKID", P_SB_D.to_le_bytes().to_vec())])
    };
    let npcs = [npcs, by_template(b"NPC_", N_HOUND_MAN, "HoundMan", N_HOUND), record(b"NPC_", N_MODELLED, &[sub(b"EDID", z("Modelled")), sub(b"MODL", z(PERSON_MODEL))])].concat();
    // A leveled list that picks the person on a creature's model.
    let lists = record(
        b"LVLN",
        N_LEVELED_MAN,
        &[sub(b"EDID", z("LeveledMan")), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0]), sub(b"LVLO", [1i16.to_le_bytes().to_vec(), vec![0; 2], N_HOUND_MAN.to_le_bytes().to_vec(), 1i16.to_le_bytes().to_vec(), vec![0; 2]].concat())],
    );
    let creatures = [hound(N_HOUND, "Hound", P_SB_D), hound(N_HOUND_LINKED, "LinkedHound", P_TO_LINKED), by_template(b"CREA", N_HOUND_COPY, "HoundCopy", N_HOUND), by_template(b"CREA", N_MAN_HOUND, "CreatureOnAPersonsModel", N_MODELLED)].concat();
    // Furniture: a chair (marker flags bit 30) and a bed (bit 31); an idle marker with
    // two idles, ten seconds apart.
    let furn = [
        record(b"FURN", CHAIR, &[sub(b"EDID", z("Chair")), sub(b"MNAM", 0x4000_0001u32.to_le_bytes().to_vec())]),
        record(b"FURN", BED, &[sub(b"EDID", z("Bed")), sub(b"MNAM", 0x8000_0001u32.to_le_bytes().to_vec())]),
        record(b"FURN", CHAIR_2, &[sub(b"EDID", z("SecondChair")), sub(b"MNAM", 0x4000_0001u32.to_le_bytes().to_vec())]),
        // Its flags say what a chair's say: only its model's marker tells them apart.
        record(b"FURN", CROSS, &[sub(b"EDID", z("Cross")), sub(b"MNAM", 0x4000_0001u32.to_le_bytes().to_vec())]),
    ]
    .concat();
    let idlm = record(
        b"IDLM",
        IDLE_MARKER,
        &[sub(b"EDID", z("LeanMarker")), sub(b"IDLF", vec![0]), sub(b"IDLC", vec![2]), sub(b"IDLT", 10f32.to_le_bytes().to_vec()), sub(b"IDLA", [0x8F0u32.to_le_bytes(), 0x8F1u32.to_le_bytes()].concat())],
    );
    let stat = record(b"STAT", STAT, &[sub(b"EDID", z("Marker"))]);
    let a_refs = [
        placed(b"REFR", MARK_A, "MarkA", STAT, [350.0, 350.0, 0.0], None),
        placed_with(b"REFR", HEAD_MARK, "TeachMarker", 0x34, [300.0, 200.0, 0.0], 2.0, None, None),
        placed(b"REFR", M1, "M1", STAT, [350.0, 50.0, 0.0], Some(M2)),
        placed_with(b"REFR", M2, "M2", STAT, [350.0, 350.0, 0.0], 0.0, Some(M3), Some(6.0)),
        placed(b"REFR", M3, "M3", STAT, [50.0, 350.0, 0.0], None),
        placed(b"REFR", L1, "L1", STAT, [250.0, 50.0, 0.0], Some(L2)),
        placed(b"REFR", L2, "L2", STAT, [250.0, 250.0, 0.0], Some(L3)),
        placed(b"REFR", L3, "L3", STAT, [50.0, 250.0, 0.0], Some(L1)),
        // The door the graph's door link names, placed so the game can lock it (away
        // from everything the tests walk to).
        placed(b"REFR", DOOR_A, "DoorA", STAT, [-2000.0, -2000.0, 0.0], None),
        // A separate synthetic load door for script activation, off the walking route.
        record(b"REFR", SCRIPT_DOOR, &[
            sub(b"EDID", z("ScriptDoor")), sub(b"NAME", SCRIPT_DOOR_BASE.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24]),
            // XTEL: destination door, position and rotation, No Alarm (u32) [xEdit REFR XTEL].
            sub(b"XTEL", [SCRIPT_DEST_DOOR.to_le_bytes().to_vec(),
                SCRIPT_DEST.into_iter().chain([0.0, 0.0, 1.0]).flat_map(f32::to_le_bytes).collect(), 0u32.to_le_bytes().to_vec()].concat()),
        ]),
        placed(b"ACHR", R_TRAVEL, "TravellerRef", N_TRAVEL, START, None),
        placed(b"ACHR", R_HUNTER, "HunterRef", N_HUNTER, [150.0, 150.0, 0.0], None),
        placed(b"ACHR", R_IN, "LodgerRef", N_IN, START, None),
        placed(b"ACHR", R_RADIUS, "VisitorRef", N_RADIUS, START, None),
        placed(b"ACHR", R_TO_HEADING, "TeacherRef", N_TO_HEADING, START, None),
        placed(b"ACHR", R_PATROL, "GuardRef", N_PATROL, START, None),
        placed(b"ACHR", R_LOOP, "SentryRef", N_LOOP, [60.0, 240.0, 0.0], Some(L1)),
        placed(b"ACHR", R_FOLLOW, "FollowerRef", N_FOLLOW, START, None),
        placed(b"ACHR", R_HERE, "StanderRef", N_HERE, START, None),
        placed(b"ACHR", R_RUN, "RunnerRef", N_RUN, START, None),
        placed(b"ACHR", R_STEADY, "CourierRef", N_STEADY, START, None),
        placed(b"ACHR", R_DOG, "DogRef", N_DOG, [150.0, 50.0, 0.0], None),
        placed(b"ACHR", R_DOG2, "SecondDogRef", N_DOG, [150.0, 250.0, 0.0], None),
        placed(b"ACHR", R_COMPANION, "CompanionRef", N_COMPANION, [250.0, 50.0, 0.0], None),
        placed(b"ACHR", R_LONER, "LonerRef", N_LONER, [350.0, 50.0, 0.0], None),
        placed(b"ACHR", R_SHOOTER, "ShooterRef", N_SHOOTER, START, None),
    ]
    .concat();
    let b_refs = [
        placed(b"REFR", MARK_B, "MarkB", STAT, [5250.0, 5250.0, 50.0], None),
        placed(b"REFR", SCRIPT_DEST_DOOR, "ScriptDestDoor", SCRIPT_DOOR_BASE, SCRIPT_DEST, None),
    ].concat();
    let cell = |id: u32, edid: &str, refs: &[u8]| {
        let rec = record(b"CELL", id, &[sub(b"EDID", z(edid))]);
        [rec, group(id.to_le_bytes(), 6, &group(id.to_le_bytes(), 9, refs))].concat()
    };
    // Cell C: 10 x 10 squares from (10000, 10000); the sandboxers stand near its middle.
    let c_refs = [
        placed(b"REFR", MARK_C, "MarkC", STAT, [10500.0, 10500.0, 0.0], None),
        placed(b"REFR", CHAIR_C, "ChairC", CHAIR, [10700.0, 10500.0, 0.0], None),
        placed(b"REFR", BED_C, "BedC", BED, [10300.0, 10700.0, 0.0], None),
        placed_with(b"REFR", IDLE_C, "IdleC", IDLE_MARKER, [10500.0, 10250.0, 0.0], 1.25, None, None),
        placed(b"ACHR", R_SB_ALL, "IdlerRef", N_SB_ALL, [10480.0, 10480.0, 0.0], None),
        placed(b"ACHR", R_SB_WANDER, "WandererRef", N_SB_WANDER, [10480.0, 10520.0, 0.0], None),
        placed(b"ACHR", R_SB_OBJECTS, "SitterRef", N_SB_OBJECTS, [10520.0, 10480.0, 0.0], None),
        placed(b"ACHR", R_SB_OBJECTS_2, "Sitter2Ref", N_SB_OBJECTS, [10520.0, 10520.0, 0.0], None),
        placed(b"ACHR", R_SB_NOTHING, "StatueRef", N_SB_NOTHING, [10450.0, 10450.0, 0.0], None),
        placed(b"ACHR", R_TO_CHAIR, "ChairUserRef", N_TO_CHAIR, [10050.0, 10050.0, 0.0], None),
        placed(b"ACHR", R_SLEEP_C, "SleeperRef", N_SLEEP_C, [10050.0, 10950.0, 0.0], None),
        placed(b"ACHR", R_SEEKER, "SeekerRef", N_SEEKER, [10050.0, 10500.0, 0.0], None),
        placed(b"ACHR", R_KEEPER, "KeeperRef", N_KEEPER, [10950.0, 10050.0, 0.0], None),
    ]
    .concat();
    // Cell D: around its middle, two seats of two bases, a bed and a cross; a cross of its
    // own out of the sandbox's radius; actors linked to it, and eaters by a seat or the cross.
    let d_refs = [
        placed(b"REFR", MARK_D, "MarkD", STAT, [20500.0, 20500.0, 0.0], None),
        placed(b"REFR", CHAIR_NEAR, "ChairNear", CHAIR, [20650.0, 20500.0, 0.0], None),
        placed(b"REFR", CHAIR_FAR, "ChairFar", CHAIR_2, [20800.0, 20700.0, 0.0], None),
        placed(b"REFR", BED_D, "BedD", BED, [20300.0, 20700.0, 0.0], None),
        placed(b"REFR", CROSS_D, "CrossD", CROSS, [20400.0, 20300.0, 0.0], None),
        placed(b"REFR", CROSS_LINK, "CrossLink", CROSS, [20950.0, 20050.0, 0.0], None),
        placed(b"ACRE", R_HOUND, "HoundRef", N_HOUND, [20480.0, 20480.0, 0.0], None),
        placed(b"ACHR", R_PERSON_D, "PersonInDRef", N_PERSON_D, [20520.0, 20480.0, 0.0], None),
        placed(b"ACHR", R_HOUND_MAN, "HoundManRef", N_HOUND_MAN, [20540.0, 20520.0, 0.0], None),
        placed(b"ACRE", R_HOUND_COPY, "HoundCopyRef", N_HOUND_COPY, [20460.0, 20520.0, 0.0], None),
        placed(b"ACHR", R_LEVELED_MAN, "LeveledManRef", N_LEVELED_MAN, [20440.0, 20460.0, 0.0], None),
        placed(b"ACRE", R_MAN_HOUND, "ManHoundRef", N_MAN_HOUND, [20560.0, 20460.0, 0.0], None),
        placed(b"ACHR", R_VICTIM, "VictimRef", N_VICTIM, [20900.0, 20100.0, 0.0], Some(CROSS_LINK)),
        placed(b"ACRE", R_HOUND_LINKED, "LinkedHoundRef", N_HOUND_LINKED, [20900.0, 20150.0, 0.0], Some(CROSS_LINK)),
        placed(b"ACHR", R_EATER_CHAIRS, "ChairEaterRef", N_EATER, [20700.0, 20560.0, 0.0], None),
        placed(b"ACHR", R_EATER_CROSS, "CrossEaterRef", N_EATER, [20350.0, 20250.0, 0.0], None),
    ]
    .concat();
    let cells = [cell(CELL_A, "CellA", &a_refs), cell(CELL_B, "CellB", &b_refs), cell(CELL_C, "CellC", &c_refs), cell(CELL_D, "CellD", &d_refs)].concat();
    let top = |sig: &[u8; 4], body: &[u8]| group(*sig, 0, body);
    [
        tes4(),
        top(b"GLOB", &globs),
        top(b"STAT", &stat),
        top(b"DOOR", &record(b"DOOR", SCRIPT_DOOR_BASE, &[sub(b"EDID", z("ScriptDoorBase"))])),
        top(b"FURN", &furn),
        top(b"IDLM", &idlm),
        top(b"NPC_", &npcs),
        top(b"CREA", &creatures),
        top(b"LVLN", &lists),
        top(b"PACK", &packs),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cells))),
    ]
    .concat()
}

// ---- navmeshes ---------------------------------------------------------------------

/// A flat navmesh over `n` × `n` squares of side 100 from `origin`, two triangles a
/// square, at height `z`.
fn grid(form_id: u32, origin: [f32; 2], n: usize, z: f32) -> NavMeshRecord {
    grid_of(form_id, origin, n, 100.0, z)
}

fn grid_of(form_id: u32, origin: [f32; 2], n: usize, size: f32, z: f32) -> NavMeshRecord {
    let vid = |i: usize, j: usize| (j * (n + 1) + i) as u16;
    let mut vertices = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            vertices.push([origin[0] + i as f32 * size, origin[1] + j as f32 * size, z]);
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

const A: Space = Space::Interior(CELL_A);
const B: Space = Space::Interior(CELL_B);
const D: Space = Space::Interior(CELL_D);

/// Cell A: 4 × 4 squares at the origin; cell B: 3 × 3 at (5000, 5000), height 50. A door
/// in A's far corner leads to B's near corner and back.
fn nav() -> NavGraph {
    let mut a = grid(1, [0.0, 0.0], 4, 0.0);
    let mut b = grid(2, [5000.0, 5000.0], 3, 50.0);
    let corner = a.triangles.len() - 2;
    a.door_links.push(DoorLink { door: DOOR_A, triangle: corner as u16 });
    a.triangles[corner].flags |= tri_flags::DOOR;
    b.door_links.push(DoorLink { door: DOOR_B, triangle: 0 });
    let c = grid_of(3, [10000.0, 10000.0], 10, 100.0, 0.0);
    let d = grid_of(4, [20000.0, 20000.0], 10, 100.0, 0.0);
    let input = [(&a, Some(CELL_A), Some(A)), (&b, Some(CELL_B), Some(B)), (&c, Some(CELL_C), Some(C)), (&d, Some(CELL_D), Some(D))];
    let mut g = NavGraph::assemble(&input);
    g.join_doors(&input, |door| match door {
        DOOR_A => Some((DOOR_B, [5050.0, 5030.0, 50.0], 0.5, B)),
        DOOR_B => Some((DOOR_A, [340.0, 340.0, 0.0], 3.0, A)),
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
    fixture_of(plugin_bytes())
}

fn fixture_of(bytes: Vec<u8>) -> Fixture {
    let plugins = vec![Plugin::parse(&bytes, "Carry.esm".into()).unwrap()];
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

fn sim<'a>(f: &'a Fixture, actors: &[u32]) -> Sim<'a> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let game = Game::new(&f.data, world, GameConfig { strict: true, frame_seconds: 0.5, package_interval: Some(10.0), ..GameConfig::default() });
    let mut ai = Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() });
    for a in actors {
        ai.add(&game, FormId(*a)).unwrap();
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

    fn events(&self, actor: u32) -> Vec<&AiEvent> {
        self.ai.events.iter().filter(|e| e.actor == FormId(actor)).map(|e| &e.event).collect()
    }

    fn feet(&self, actor: u32) -> [f32; 3] {
        self.ai.agents[&FormId(actor)].walker.feet
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

// ---- tests -------------------------------------------------------------------------

#[test]
fn a_travel_package_walks_through_the_door_and_meets_its_goal() {
    let f = fixture();
    assert!(f.data.issues.is_empty(), "{:?}", f.data.issues.to_json());
    let mut s = sim(&f, &[R_TRAVEL]);
    s.run(1.0);
    let actor = FormId(R_TRAVEL);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    // The package holds the periodic choice until the actor is there.
    assert_eq!(s.game.package_held(actor), Some("must reach location"));
    s.run(30.0);
    let ev = s.events(R_TRAVEL);
    assert!(matches!(ev[0], AiEvent::Package { package: Some(p) } if p.0 == P_TRAVEL_B));
    assert!(matches!(ev[1], AiEvent::Trip { doors, running: false, space: B, .. } if doors == &[FormId(DOOR_A)]), "{:?}", ev[1]);
    assert!(matches!(ev[2], AiEvent::Door { door, to_door } if door.0 == DOOR_A && to_door.0 == DOOR_B));
    assert!(matches!(ev[3], AiEvent::Arrived { travelled, .. } if *travelled > 600.0 && *travelled < 900.0), "{:?}", ev[3]);
    assert_eq!(ev.len(), 4);
    assert_eq!(s.ai.agents[&actor].activity, Activity::There);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
    // The game knows where the actor is and that the package met its goal.
    let st = s.game.world.peek(actor).unwrap();
    assert_eq!(st.cell, Some(FormId(CELL_B)));
    assert!(dist(st.pos, [5250.0, 5250.0, 50.0]) < 5.0);
    assert!(s.game.extra[&actor].package_goal_met);
    assert_eq!(s.game.package_held(actor), None);
}

/// #2123: the extender's OnActivate hears an actor the AI walks through a door once a
/// crossing. Asked on every short step as it comes up to DoorA (no script), it is heard
/// once; put back before the door and sent through it again with no game time gone (so
/// not by `ASKED_SECONDS` running out), it is heard again, as the first crossing was
/// closed when the door fell behind it.
#[test]
fn the_extender_hears_each_crossing_of_a_door_once() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    s.game.run_source("SetEventHandler \"OnActivate\" (begin function {ref rT, ref rA}
if rT == DoorA
let TestGlobal := TestGlobal + 1
endif
end)", Default::default()).expect("compiles");
    let heard = |s: &Sim| s.game.world.global(FormId(0x807)).unwrap();
    let crossings = |s: &Sim| s.events(R_TRAVEL).iter().filter(|e| matches!(e, AiEvent::Door { door, .. } if door.0 == DOOR_A)).count();
    s.run(1.0);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    // Short steps, the game's clock standing.
    let step = |s: &mut Sim| s.ai.update(&mut s.game, &s.nav, 0.1);
    let mut steps = 0;
    while heard(&s) == 0.0 {
        step(&mut s);
        steps += 1;
        assert!(steps < 400, "never asked at the door");
    }
    // The walker as it stands at its first asking, to send it through again.
    let before = s.ai.agents[&actor].walker.clone();
    let time = s.game.time;
    let mut asks = 1;
    while crossings(&s) == 0 {
        step(&mut s);
        asks += 1;
        assert!(asks < 100, "never went through");
    }
    assert!(asks >= 5, "asked over several steps, {asks}");
    assert_eq!(heard(&s), 1.0, "one crossing, one event");
    step(&mut s);
    let a = s.ai.agents.get_mut(&actor).unwrap();
    a.walker = before;
    a.activity = Activity::Travelling;
    let mut steps = 0;
    while crossings(&s) < 2 {
        step(&mut s);
        steps += 1;
        assert!(steps < 100, "never went through again");
    }
    assert_eq!(s.game.time, time, "no game time gone");
    assert_eq!(heard(&s), 2.0, "a second crossing, a second event");
}

/// #2123, review: the asking distance is a step's length plus a margin, so it changes
/// with the frame time. An actor held at its edge (turning on the spot, `NoProgress`) by
/// long and short frames by turns, then taken on through the door in long and short
/// steps by turns, is on one crossing of the door: one event.
#[test]
fn frame_times_at_the_asking_edge_make_no_second_event() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    s.game.run_source("SetEventHandler \"OnActivate\" (begin function {ref rT, ref rA}
if rT == DoorA
let TestGlobal := TestGlobal + 1
endif
end)", Default::default()).expect("compiles");
    let heard = |s: &Sim| s.game.world.global(FormId(0x807)).unwrap();
    let crossings = |s: &Sim| s.events(R_TRAVEL).iter().filter(|e| matches!(e, AiEvent::Door { door, .. } if door.0 == DOOR_A)).count();
    // How far the actor is from where its leg ends at the door.
    let to_door = |s: &Sim| {
        let w = &s.ai.agents[&actor].walker;
        let f = w.follower.as_ref().expect("on its way");
        dist(w.feet, *f.path.legs[f.leg].points.last().unwrap())
    };
    s.run(1.0);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    // Short steps (reach 97 units) up to 120 units from the door: not asked yet.
    let mut steps = 0;
    while to_door(&s) > 120.0 {
        s.ai.update(&mut s.game, &s.nav, 0.01);
        steps += 1;
        assert!(steps < 100_000, "never came near the door");
    }
    assert_eq!(heard(&s), 0.0);
    // Held there: a long frame (reach 146) asks it, a short one (reach 97) leaves it out
    // of reach, by turns.
    for i in 0..8 {
        let dt = if i % 2 == 0 { 0.5 } else { 0.01 };
        s.ai.update_moved(&mut s.game, &s.nav, dt, &mut NoProgress);
    }
    assert!(to_door(&s) > 100.0, "held where it stood, {}", to_door(&s));
    assert_eq!(heard(&s), 1.0, "held at the edge: one crossing");
    // On in long and short steps by turns, through the door.
    let mut steps = 0;
    while crossings(&s) == 0 {
        let dt = if steps % 2 == 0 { 0.2 } else { 0.01 };
        s.ai.update(&mut s.game, &s.nav, dt);
        steps += 1;
        assert!(steps < 400, "never went through");
    }
    assert_eq!(heard(&s), 1.0, "one crossing, one event");
}

/// A door that opens in place across the corner at L2 of the sentry's round, at 45°:
/// the way up from L1 crosses it 20 units before L2, the way on to L3 20 units after.
fn door_at_l2() -> fnv_nav::InPlaceDoor {
    let (c, si) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    fnv_nav::InPlaceDoor {
        door: DOOR_A,
        space: A,
        position: [240.0, 240.0, 0.0],
        rotation: [c, si, 0.0, -si, c, 0.0, 0.0, 0.0, 1.0],
        scale: 1.0,
        bounds: ([-40.0, -4.0, 0.0], [40.0, 4.0, 200.0]),
    }
}

/// #2123, review: the sentry (PatrolRound, no idle time at its points) goes through the
/// door at L2, reaches L2 and is sent on to L3 through the same door, under the same
/// package, at once (its arrival plans the next way in the same update). Two crossings,
/// two events, whatever the frame time: the first crossing ends where the follower went
/// through the door, not when a later step finds the door no longer next.
#[test]
fn a_patrol_back_through_the_door_it_just_passed_is_heard_again() {
    let f = fixture();
    let mut nav = nav();
    nav.set_in_place_doors(vec![door_at_l2()], |_| None);
    let at = |p: [f32; 3]| fnv_nav::Place { space: A, pos: p };
    for (from, to) in [([250.0, 50.0, 0.0], [250.0, 250.0, 0.0]), ([250.0, 250.0, 0.0], [50.0, 250.0, 0.0])] {
        let way = nav.find_path(at(from), at(to), &fnv_nav::PathParams::default()).unwrap();
        assert_eq!(way.legs[0].door_stops.len(), 1, "the way from {from:?} to {to:?} stops at the door");
    }
    // Short frames, and the long ones of an actor out of the high process, which can
    // take it from the door's stop (104 units before the door) to L2 in one step.
    for dt in [0.5f32, 0.3, 1.8, 2.0, 2.5, 3.0, 4.0] {
        let mut s = sim(&f, &[R_LOOP]);
        s.nav.set_in_place_doors(vec![door_at_l2()], |_| None);
        s.game.run_source("SetEventHandler \"OnActivate\" (begin function {ref rT, ref rA}
if rT == DoorA
let TestGlobal := TestGlobal + 1
endif
end)", Default::default()).expect("compiles");
        let points = |s: &Sim| -> Vec<u32> {
            s.events(R_LOOP).iter().filter_map(|e| match e {
                AiEvent::Arrived { patrol_point: Some(p), .. } => Some(p.0),
                _ => None,
            }).collect()
        };
        let mut steps = 0;
        while points(&s).len() < 4 {
            s.game.tick();
            s.ai.update(&mut s.game, &s.nav, dt);
            steps += 1;
            assert!(steps < 2000, "the round did not come round at {dt}: {:x?}", points(&s));
        }
        assert_eq!(&points(&s)[..4], &[L3, L1, L2, L3], "at {dt}");
        let heard = s.game.world.global(FormId(0x807)).unwrap();
        assert_eq!(heard, 2.0, "up to L2 and on from it, through the door both times, at {dt}");
    }
}

#[test]
fn a_restrained_actor_stays_in_place_until_released() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let start = s.feet(R_TRAVEL);
    s.game.extra.entry(FormId(R_TRAVEL)).or_default().restrained = true;
    s.run(30.0);
    assert!(dist(s.feet(R_TRAVEL), start) < 1.0, "a restrained actor walked");
    s.game.extra.get_mut(&FormId(R_TRAVEL)).unwrap().restrained = false;
    s.run(30.0);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
}

/// A caller whose character movement carries an actor straight on along its heading
/// (as the engine's character controller does on open ground).
struct StraightOn;

impl fnv_ai::Mover for StraightOn {
    fn coast(&mut self, _actor: FormId, walker: &mut fnv_nav::Walker, dt: f32) -> bool {
        let d = fnv_nav::follow::dir_of(walker.heading);
        let s = walker.moving_speed() * dt;
        walker.feet = [walker.feet[0] + d[0] * s, walker.feet[1] + d[1] * s, walker.feet[2]];
        true
    }
}

#[test]
fn set_actors_ai_0_keeps_a_walking_actor_going_straight_on_until_switched_back_on() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    s.run(3.0);
    assert!(s.ai.agents[&actor].walker.moving, "walking before the AI goes off");
    s.game.extra.entry(actor).or_default().ai_off = true;
    let (start, heading) = (s.feet(R_TRAVEL), s.ai.agents[&actor].walker.heading);
    for _ in 0..4 {
        s.game.tick();
        s.ai.update_moved(&mut s.game, &s.nav, 0.5, &mut StraightOn);
    }
    // Two seconds at 100 units/s, on its heading, the heading kept, the game told.
    let a = &s.ai.agents[&actor];
    assert!((dist(a.walker.feet, start) - 200.0).abs() < 1.0, "{:?} from {start:?}", a.walker.feet);
    assert_eq!(a.walker.heading, heading);
    assert_eq!(a.activity, Activity::Travelling);
    assert!(dist(s.game.world.peek(actor).unwrap().pos, a.walker.feet) < 1.0);
    // Without character movement (headless) it stands, and stays standing.
    let here = s.feet(R_TRAVEL);
    s.run(2.0);
    assert!(dist(s.feet(R_TRAVEL), here) < 1e-3);
    assert!(!s.ai.agents[&actor].walker.moving);
    s.run(2.0);
    assert!(dist(s.feet(R_TRAVEL), here) < 1e-3);
    // Back on: it walks on to its package's place.
    s.game.extra.get_mut(&actor).unwrap().ai_off = false;
    s.run(40.0);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
}

#[test]
fn time_passed_in_a_menu_takes_the_actor_to_its_packages_place() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    // A menu holds the game (the Wait menu): no tick moves anyone, game time stands.
    s.game.open_menu(1012);
    let before = s.game.time;
    s.game.choose_packages_now();
    // Too little time: on its way, not there.
    assert_eq!(s.ai.pass_time(&mut s.game, &s.nav, 2.0, 0.5), 1);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) > 100.0);
    // An hour's worth: through the door and there, and it stops as soon as it is.
    assert_eq!(s.ai.pass_time(&mut s.game, &s.nav, 120.0, 0.5), 1);
    assert_eq!(s.ai.agents[&actor].activity, Activity::There);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
    let ev = s.events(R_TRAVEL);
    assert!(matches!(ev[0], AiEvent::Package { package: Some(p) } if p.0 == P_TRAVEL_B));
    assert!(matches!(ev[3], AiEvent::Arrived { .. }), "{:?}", ev);
    // The game knows: its place, and the package's goal (its end script would have run).
    assert_eq!(s.game.world.peek(actor).unwrap().cell, Some(FormId(CELL_B)));
    assert!(s.game.extra[&actor].package_goal_met);
    assert_eq!(s.game.time, before);
    // Nobody left to walk: nothing more happens.
    assert_eq!(s.ai.pass_time(&mut s.game, &s.nav, 120.0, 0.5), 0);
    assert_eq!(s.events(R_TRAVEL).len(), 4);
}

#[test]
fn a_place_that_cannot_be_reached_sets_the_package_aside() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    s.nav.set_door_lock(DOOR_A, Some(DoorLock { level: 50, key: Some(0x4B) }));
    s.run(1.0);
    let actor = FormId(R_TRAVEL);
    let ev = s.events(R_TRAVEL);
    assert!(matches!(ev[0], AiEvent::Package { package: Some(p) } if p.0 == P_TRAVEL_B));
    assert!(matches!(ev[1], AiEvent::Unreachable { package, .. } if package.0 == P_TRAVEL_B), "{:?}", ev[1]);
    // The actor took the next package of its list, which keeps it at home.
    assert!(matches!(ev[2], AiEvent::Package { package: Some(p) } if p.0 == P_DEFAULT), "{:?}", ev[2]);
    assert!(matches!(ev[3], AiEvent::AlreadyThere { .. }));
    assert_eq!(s.game.check_package(actor, FormId(P_TRAVEL_B)), PackageCheck::Unreachable);
    assert_eq!(s.feet(R_TRAVEL), START);
    // Within the hour it does not try again.
    s.run(30.0);
    assert_eq!(s.events(R_TRAVEL).len(), 4);
    // After it, it tries again; with the key it gets through.
    s.game.world.set_global(FormId(G_DAYS), 5.0 + 1.1 / 24.0).unwrap();
    s.ai.cfg.params.keys.push(0x4B);
    s.run(40.0);
    let ev = s.events(R_TRAVEL);
    assert!(matches!(ev[4], AiEvent::Package { package: Some(p) } if p.0 == P_TRAVEL_B), "{:?}", ev[4]);
    assert!(matches!(ev[5], AiEvent::Trip { .. }));
    assert!(matches!(ev.last().unwrap(), AiEvent::Arrived { .. }));
    assert_eq!(s.ai.agents[&actor].walker.space, B);
}

/// A package's place that no door leads to (issue #2405: Gomez in the NCR safehouse's
/// side room, a navmesh island, sent to the door by his Leave package; Malcolm Holmes
/// sent back to his holding cell). In sight the package is set aside as before; out of
/// sight the actor is put there and has arrived (docs/spec-notes/packages.md §11). A
/// disabled door between the two still joins them: the actor stays (Loyal, #2566).
#[test]
fn out_of_sight_an_actor_is_put_at_a_place_no_door_leads_to() {
    let f = fixture();
    let actor = FormId(R_TRAVEL);
    // Cells A and B, and no door between them.
    let no_door = || {
        let a = grid(1, [0.0, 0.0], 4, 0.0);
        let b = grid(2, [5000.0, 5000.0], 3, 50.0);
        NavGraph::assemble(&[(&a, Some(CELL_A), Some(A)), (&b, Some(CELL_B), Some(B))])
    };
    let out_of_sight = fnv_ai::FAR_PROCESS_SECONDS as f64 + 5.0;
    // In the loaded area: set aside, where it stands.
    let mut s = sim(&f, &[R_TRAVEL]);
    s.nav = no_door();
    s.run(1.0);
    let ev = s.events(R_TRAVEL);
    assert!(matches!(ev[1], AiEvent::Unreachable { package, .. } if package.0 == P_TRAVEL_B), "{ev:?}");
    assert_eq!(s.feet(R_TRAVEL), START);
    // Out of sight: put at the package's place in B, arrived, its goal met.
    let mut s = sim(&f, &[R_TRAVEL]);
    s.nav = no_door();
    assert!(s.ai.set_level(&s.game, actor, fnv_ai::ProcessLevel::Low));
    s.run(out_of_sight);
    let ev = s.events(R_TRAVEL);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::PutThere { package, from, to } if package.0 == P_TRAVEL_B && from.space == A && to.space == B)), "{ev:?}");
    assert!(matches!(ev.last().unwrap(), AiEvent::Arrived { package, .. } if package.0 == P_TRAVEL_B), "{ev:?}");
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::Unreachable { .. })), "{ev:?}");
    assert_eq!(s.ai.agents[&actor].walker.space, B);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
    assert_eq!(s.game.world.peek(actor).unwrap().cell, Some(FormId(CELL_B)));
    assert!(s.game.extra[&actor].package_goal_met);
    // The door there but disabled: no way, yet joined, so it stays and the package is set
    // aside.
    let mut s = sim(&f, &[R_TRAVEL]);
    s.nav.sync_door_presence([(DOOR_A, false)]);
    assert!(s.ai.set_level(&s.game, actor, fnv_ai::ProcessLevel::Low));
    s.run(out_of_sight);
    let ev = s.events(R_TRAVEL);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Unreachable { package, .. } if package.0 == P_TRAVEL_B)), "{ev:?}");
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::PutThere { .. })), "{ev:?}");
    assert_eq!(s.ai.agents[&actor].walker.space, A);
}

/// The obstacle poll is skipped while nothing that could change an obstacle's presence or
/// the graph's obstacles happened since the last one (`Ai::obstacles_in_line`), and runs
/// again after each such change: a script's Disable/Enable, a deletion, any reference
/// state created, the obstacles learnt again. Whenever it is skipped, polling would have
/// found every obstacle as the graph has it.
#[test]
fn the_obstacle_poll_is_skipped_only_while_nothing_changed() {
    let f = fixture();
    let mut s = sim(&f, &[]);
    let ob = FormId(M3);
    let sandbag = || fnv_nav::obstacles::Obstacle { reference: M3, space: A, position: [150.0, 150.0, 0.0], rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0, bounds: ([-40.0, -40.0, 0.0], [40.0, 40.0, 90.0]), initially_enabled: false };
    s.nav.set_obstacles(vec![sandbag()], |_| false);
    assert_eq!(s.nav.obstacles.len(), 1);
    // Skipped means a poll would change nothing.
    let in_line = |s: &Sim| {
        let skip = s.ai.obstacles_in_line(&s.game, &s.nav);
        if skip {
            assert!(s.nav.obstacles.iter().all(|(&r, o)| o.enabled == s.game.present(FormId(r))), "skipped while out of line");
        }
        skip
    };
    let changed = |s: &mut Sim| s.ai.apply_obstacles(&s.game, &mut s.nav).map(|(c, _)| c);
    assert!(!in_line(&s), "never polled");
    // M3 stands enabled in the game: the first poll brings it in and cuts.
    assert_eq!(changed(&mut s), Some(vec![M3]));
    assert!(s.nav.cut_triangles() > 0);
    assert!(in_line(&s));
    assert_eq!(changed(&mut s), None);
    // A script's Disable.
    s.game.world.set_enabled(ob, false).unwrap();
    assert!(!in_line(&s));
    assert_eq!(changed(&mut s), Some(vec![M3]));
    assert_eq!(s.nav.cut_triangles(), 0);
    assert!(in_line(&s));
    // The same flag again changes nothing, and nothing is polled.
    s.game.world.set_enabled(ob, false).unwrap();
    assert!(in_line(&s));
    // Any reference state created: polled again, nothing changed.
    s.game.world.place(FormId(STAT), Some(FormId(CELL_A)), [0.0; 3], [0.0; 3], 1).unwrap();
    assert!(!in_line(&s));
    assert_eq!(changed(&mut s), None);
    assert!(in_line(&s));
    // Enabled again, then deleted (MarkForDelete).
    s.game.world.set_enabled(ob, true).unwrap();
    assert_eq!(changed(&mut s), Some(vec![M3]));
    s.game.world.set_deleted(ob, true).unwrap();
    assert!(!in_line(&s));
    assert_eq!(changed(&mut s), Some(vec![M3]));
    assert_eq!(s.nav.cut_triangles(), 0);
    // The obstacles learnt again (as enabled): the graph's stamp changed, polled again.
    s.nav.set_obstacles(vec![sandbag()], |_| true);
    assert!(!in_line(&s));
    assert_eq!(changed(&mut s), Some(vec![M3]));
    assert!(in_line(&s));
    // Another AI has polled nothing yet.
    let fresh = Ai::new(AiConfig::default());
    assert!(!fresh.obstacles_in_line(&s.game, &s.nav));
}

/// A door the game locks (a script's Lock) shuts the way: the actor sets its package
/// aside. Unlocked again (a script's Unlock, a key, a picked lock), the graph learns it,
/// the package is chosen again at once and the actor gets through
/// (`Ai::apply_door_locks`; docs/handoff/project-audit-2026-10-23.md §4.1 item 3).
#[test]
fn a_door_locked_and_unlocked_in_the_game_reaches_the_paths() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    // Nothing changed yet: nothing to apply, and once applied nothing again.
    assert_eq!(s.ai.apply_door_locks(&mut s.game, &mut s.nav), None);
    s.game.world.lock(FormId(DOOR_A), Some(50)).unwrap();
    assert_eq!(s.ai.apply_door_locks(&mut s.game, &mut s.nav), Some(vec![]));
    assert!(s.nav.door_locks.contains_key(&DOOR_A));
    assert_eq!(s.ai.apply_door_locks(&mut s.game, &mut s.nav), None, "applied once");
    s.run(1.0);
    assert!(s.events(R_TRAVEL).iter().any(|e| matches!(e, AiEvent::Unreachable { package, .. } if package.0 == P_TRAVEL_B)), "{:?}", s.events(R_TRAVEL));
    s.run(30.0);
    assert_eq!(s.feet(R_TRAVEL), START);
    // Unlocked: within the hour, the package is chosen again and the way is open.
    s.game.world.unlock(FormId(DOOR_A)).unwrap();
    assert_eq!(s.ai.apply_door_locks(&mut s.game, &mut s.nav), Some(vec![actor]));
    assert!(!s.nav.door_locks.contains_key(&DOOR_A));
    s.run(40.0);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0, "{:?}", s.feet(R_TRAVEL));
    assert_eq!(s.ai.agents[&actor].walker.space, B);
}

/// #2766: a locked door that is disabled holds no lock in the graph (it is not in the
/// world, so it shuts no way), and its lock is back when it is enabled again; the poll
/// follows the world's presence as well as its locks.
#[test]
fn a_disabled_doors_lock_is_left_out_of_the_graph() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    assert_eq!(s.ai.apply_door_locks(&mut s.game, &mut s.nav), None);
    s.game.world.lock(FormId(DOOR_A), Some(255)).unwrap();
    assert!(s.ai.apply_door_locks(&mut s.game, &mut s.nav).is_some());
    assert_eq!(s.nav.door_locks[&DOOR_A], DoorLock { level: 255, key: None });
    // Disabled: no lock changed in the world, yet the graph lets go of this one.
    s.game.world.set_enabled(FormId(DOOR_A), false).unwrap();
    assert!(s.ai.apply_door_locks(&mut s.game, &mut s.nav).is_some(), "the presence change is seen");
    assert!(!s.nav.door_locks.contains_key(&DOOR_A));
    assert_eq!(s.ai.apply_door_locks(&mut s.game, &mut s.nav), None, "applied once");
    // The game still has it locked.
    assert!(s.game.world.lock_of(FormId(DOOR_A)).is_some_and(|l| l.locked));
    // Enabled again: locked as before.
    s.game.world.set_enabled(FormId(DOOR_A), true).unwrap();
    assert!(s.ai.apply_door_locks(&mut s.game, &mut s.nav).is_some());
    assert_eq!(s.nav.door_locks[&DOOR_A], DoorLock { level: 255, key: None });
}

#[test]
fn an_open_planned_door_closed_before_arrival_rechecks_live_permissions() {
    // [design, #2716] Count the engine's opening requests, including ones a stale
    // open-door path would make after SetOpenState closes its still-locked door.
    #[derive(Default)]
    struct OpenRequests(Vec<u32>);
    impl fnv_ai::Mover for OpenRequests {
        fn open(&mut self, _: FormId, door: u32) -> bool {
            self.0.push(door);
            true
        }
    }
    // [design] Hold the first movement step so closure happens after planning but
    // before an open door's approach stop can be consumed at the starting point.
    struct PlanOnly;
    impl fnv_ai::Mover for PlanOnly {
        fn walk(&mut self, _: FormId, _: &mut fnv_nav::Walker, _: &NavGraph, _: f32) -> Option<fnv_nav::WalkEvent> {
            Some(fnv_nav::WalkEvent::Moved)
        }
    }

    for permission in ["none", "key", "lost-key", "owner", "lost-owner", "unlocked", "configured-locks", "configured-key", "configured-owner"] {
        // [design] An authored key record lets inventory changes use World APIs.
        let key = FormId(0xF20);
        let mut bytes = plugin_bytes();
        bytes.extend(record(b"KEYM", key.0, &[sub(b"EDID", z("ApproachKey"))]));
        let mut f = fixture_of(bytes);
        let actor = FormId(R_TRAVEL);
        let door = FormId(M1);
        f.refs.refs.get_mut(&door).unwrap().lock = Some(fnv_sim::refs::Lock { level: 255, key: Some(key), leveled: false });
        let mut s = sim(&f, &[R_TRAVEL]);
        s.nav.set_in_place_doors(vec![fnv_nav::InPlaceDoor {
            door: M1, space: A, position: [200.0, 200.0, 0.0],
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0,
            bounds: ([-4.0, -200.0, 0.0], [4.0, 200.0, 200.0]),
        }], |_| Some(DoorLock { level: 255, key: Some(key.0) }));
        s.ai.apply_door_locks(&mut s.game, &mut s.nav);
        if permission == "lost-key" {
            s.game.world.add_item(actor, key, 1).unwrap();
        }
        if permission == "lost-owner" {
            s.game.world.get(door).unwrap().extra.owner = Some(actor);
        }
        s.game.set_ref_open(door, true).unwrap();
        s.game.tick();
        s.ai.update_moved(&mut s.game, &s.nav, 0.5, &mut PlanOnly);
        let a = &s.ai.agents[&actor];
        assert_eq!(a.activity, Activity::Travelling, "{permission}");
        assert_eq!(a.walker.follower.as_ref().unwrap().door_ahead(a.walker.feet, f32::INFINITY), Some(M1), "{permission}");
        let lock_serial = s.game.world.lock_serial();
        s.game.set_ref_open(door, false).unwrap();
        assert_eq!(s.game.world.lock_serial(), lock_serial, "closure does not invalidate the path");
        match permission {
            "key" => { s.game.world.add_item(actor, key, 1).unwrap(); }
            "lost-key" => { s.game.world.remove_item(actor, key, 1).unwrap(); }
            "owner" => s.game.world.get(door).unwrap().extra.owner = Some(actor),
            "lost-owner" => s.game.world.get(door).unwrap().extra.owner = None,
            "unlocked" => { s.game.world.unlock(door).unwrap(); }
            "configured-locks" => s.ai.cfg.params.open_locked = true,
            "configured-key" => s.ai.cfg.params.keys.push(key.0),
            "configured-owner" => s.ai.cfg.params.owned_doors.push(M1),
            _ => {}
        }
        let mut mover = OpenRequests::default();
        for _ in 0..80 {
            s.game.tick();
            s.ai.update_moved(&mut s.game, &s.nav, 0.5, &mut mover);
        }
        let events = s.events(R_TRAVEL);
        if matches!(permission, "none" | "lost-key" | "lost-owner") {
            assert!(events.iter().any(|e| matches!(e, AiEvent::DoorRefused { door: d } if *d == door)), "{permission}: {events:?}");
            assert!(!events.iter().any(|e| matches!(e, AiEvent::Arrived { .. } | AiEvent::Door { .. })), "{permission}: {events:?}");
            assert!(s.feet(R_TRAVEL)[0] < 196.0, "{permission}: {:?}", s.feet(R_TRAVEL));
            assert!(!mover.0.contains(&M1), "{permission}: reopening was requested");
            assert!(!s.game.world.peek(door).unwrap().open);
            assert!(s.game.world.lock_of(door).unwrap().locked);
        } else {
            assert!(events.iter().any(|e| matches!(e, AiEvent::Arrived { .. })), "{permission}: {events:?}");
            assert!(!events.iter().any(|e| matches!(e, AiEvent::DoorRefused { .. })), "{permission}: {events:?}");
            assert_eq!(s.ai.agents[&actor].walker.space, B, "{permission}");
        }
    }
}

#[test]
fn open_locked_in_place_door_allows_arrival_despite_script_refusal() {
    use fnv_game::data::Compiled;
    use fnv_game::npc_doors::DoorAnswer;
    use fnv_script::compile::{compile, env::LoadOrderIndex};
    use fnv_script::unit::{Header, ScriptKind, ScriptUnit, UnitOwner};

    // [design, #2716] Compile an authored synthetic refusal: it counts attempts but
    // never calls Activate. Exercise both a cached closed-door refusal and a fresh one.
    for cached in [true, false] {
        let mut f = fixture();
        let plugins = vec![Plugin::parse(&plugin_bytes(), "Carry.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let names = LoadOrderIndex::build(&lo, &[]);
        let source = "scn RefusingDoor\nbegin OnActivate\nset TestGlobal to TestGlobal + 1\nend\n";
        let c = compile(source, &f.data.table, &names.env(0), None);
        assert_eq!(c.errors().count(), 0, "{:?}", c.diags);
        let decoded = fnv_script::decode(&c.bytes, &f.data.table);
        assert_eq!(decoded.report.raw_total(), 0);
        let script = FormId(0xF00);
        let unit = ScriptUnit {
            plugin: "Carry.esm".into(), owner: UnitOwner::Script { form: script.0 }, kind: ScriptKind::Object,
            header: Header { ref_count: c.refs.len() as u32, compiled_size: c.bytes.len() as u32, variable_count: c.variables.len() as u32, flags: 0 },
            variables: c.variables, refs: c.refs, has_scda: true, bytes: c.bytes, source: Some(source.into()),
        };
        let index = f.data.units.len();
        let compiled = Compiled::new(unit, decoded.program, 0);
        f.data.unit_by_key.insert(compiled.key(), index);
        f.data.units.push(compiled);
        f.data.scripts.insert(script, index);
        f.data.form_script.insert(FormId(SCRIPT_DOOR_BASE), script);
        let placed = f.refs.refs.get_mut(&FormId(M1)).unwrap();
        placed.base = FormId(SCRIPT_DOOR_BASE);
        placed.lock = Some(fnv_sim::refs::Lock { level: 255, key: None, leveled: false });

        let mut s = sim(&f, &[R_TRAVEL]);
        let actor = FormId(R_TRAVEL);
        let door = FormId(M1);
        s.nav.set_in_place_doors(vec![fnv_nav::InPlaceDoor {
            door: M1, space: A, position: [200.0, 200.0, 0.0],
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0,
            bounds: ([-4.0, -200.0, 0.0], [4.0, 200.0, 200.0]),
        }], |_| Some(DoorLock { level: 255, key: None }));
        s.ai.apply_door_locks(&mut s.game, &mut s.nav);
        if cached {
            assert_eq!(s.game.npc_door(actor, door), DoorAnswer::Refused);
            s.game.npc_door_left(actor, door);
        }
        s.game.set_ref_open(door, true).unwrap();
        s.run(40.0);
        let events = s.events(R_TRAVEL);
        assert!(events.iter().any(|e| matches!(e, AiEvent::Arrived { .. })), "cached={cached}: {events:?}");
        assert!(!events.iter().any(|e| matches!(e, AiEvent::DoorRefused { .. } | AiEvent::Unreachable { .. })), "cached={cached}: {events:?}");
        assert_eq!(s.ai.agents[&actor].activity, Activity::There);
        assert_eq!(s.ai.agents[&actor].walker.space, B);
        assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
        assert!(s.game.refused_doors(actor).contains(&M1), "the refusal remains cached");
        assert_eq!(s.game.world.global(FormId(0x807)).unwrap(), 1.0, "the refusal block ran once");
        assert!(s.game.world.peek(door).unwrap().open);
        assert!(s.game.world.lock_of(door).unwrap().locked);
    }
}

#[test]
fn open_locked_in_place_doors_reach_actor_path_parameters() {
    // [design, #2716] Synthetic barrier across cell A, using a fixture reference:
    // exercise closed, script-opened, untouched default-open, and script-closed states.
    for mode in ["closed", "script", "default", "closed-default", "set-default-open", "clear-default-open"] {
        let mut f = fixture();
        let placed = f.refs.refs.get_mut(&FormId(M1)).unwrap();
        placed.lock = Some(fnv_sim::refs::Lock { level: 255, key: None, leveled: false });
        placed.open_by_default = matches!(mode, "default" | "closed-default" | "clear-default-open");
        let mut s = sim(&f, &[R_TRAVEL]);
        s.nav.set_in_place_doors(vec![fnv_nav::InPlaceDoor {
            door: M1,
            space: A,
            position: [200.0, 200.0, 0.0],
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            scale: 1.0,
            bounds: ([-4.0, -200.0, 0.0], [4.0, 200.0, 200.0]),
        }], |_| Some(DoorLock { level: 255, key: None }));
        s.ai.apply_door_locks(&mut s.game, &mut s.nav);
        assert!(s.game.world.peek(FormId(M1)).is_none());
        if matches!(mode, "script" | "closed-default") {
            s.game.set_ref_open(FormId(M1), mode == "script").unwrap();
        } else if matches!(mode, "set-default-open" | "clear-default-open") {
            s.game.world.set_default_open(FormId(M1), mode == "set-default-open").unwrap();
            assert!(s.game.world.peek(FormId(M1)).is_none(), "SetDefaultOpen does not materialize an untouched door");
        }
        s.run(1.0);
        let events = s.events(R_TRAVEL);
        if matches!(mode, "closed" | "closed-default" | "clear-default-open") {
            assert!(events.iter().any(|e| matches!(e, AiEvent::Unreachable { .. })), "{mode}: {events:?}");
        } else {
            assert!(events.iter().any(|e| matches!(e, AiEvent::Trip { .. })), "{mode}: {events:?}");
            assert!(!events.iter().any(|e| matches!(e, AiEvent::Unreachable { .. })), "{mode}: {events:?}");
        }
        assert!(s.game.world.lock_of(FormId(M1)).unwrap().locked);
    }
}

/// A load door the game disables (a script's Disable, an enable parent's) is no way: the
/// actor sets its package aside rather than going through it. Enabled again, the graph
/// learns it, the package is chosen again at once and the actor gets through
/// (`Ai::apply_door_presence`; issue #2566, Loyal put behind a disabled Nellis hangar door).
#[test]
fn a_door_disabled_and_enabled_in_the_game_reaches_the_paths() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    // The first poll learns the doors as they are, all there: nothing changed.
    assert_eq!(s.ai.apply_door_presence(&mut s.game, &mut s.nav), None);
    s.game.world.set_enabled(FormId(DOOR_A), false).unwrap();
    assert_eq!(s.ai.apply_door_presence(&mut s.game, &mut s.nav), Some(vec![]));
    assert!(s.nav.disabled_doors.contains(&DOOR_A));
    assert_eq!(s.ai.apply_door_presence(&mut s.game, &mut s.nav), None, "applied once");
    s.run(1.0);
    assert!(s.events(R_TRAVEL).iter().any(|e| matches!(e, AiEvent::Unreachable { package, .. } if package.0 == P_TRAVEL_B)), "{:?}", s.events(R_TRAVEL));
    s.run(30.0);
    assert_eq!(s.feet(R_TRAVEL), START);
    // Enabled: the package is chosen again and the way is open.
    s.game.world.set_enabled(FormId(DOOR_A), true).unwrap();
    assert_eq!(s.ai.apply_door_presence(&mut s.game, &mut s.nav), Some(vec![actor]));
    assert!(!s.nav.disabled_doors.contains(&DOOR_A));
    s.run(40.0);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0, "{:?}", s.feet(R_TRAVEL));
    assert_eq!(s.ai.agents[&actor].walker.space, B);
}

/// #2405 (Point Lookout's mansion, Desmond): a locked door that opens in place across the
/// way names itself in the unreachable report, and the search's count is not 0; once the
/// game disables it (not in the world), it bars nothing and the package set aside is
/// chosen again, so the actor gets through without stopping at a door that is not there.
/// Enabled again, it closes the way of an actor whose walk crosses it.
#[test]
fn a_door_that_opens_in_place_bars_the_way_only_while_it_is_in_the_world() {
    let mut f = fixture();
    let actor = FormId(R_TRAVEL);
    let door = FormId(M1);
    f.refs.refs.get_mut(&door).unwrap().lock = Some(fnv_sim::refs::Lock { level: 255, key: None, leveled: false });
    let mut s = sim(&f, &[R_TRAVEL]);
    s.nav.set_in_place_doors(vec![fnv_nav::InPlaceDoor {
        door: M1, space: A, position: [200.0, 200.0, 0.0],
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0,
        bounds: ([-4.0, -200.0, 0.0], [4.0, 200.0, 200.0]),
    }], |_| Some(DoorLock { level: 255, key: None }));
    s.ai.apply_door_locks(&mut s.game, &mut s.nav);
    assert_eq!(s.ai.apply_door_presence(&mut s.game, &mut s.nav), None, "all there as placed");
    s.run(1.0);
    let why = s.events(R_TRAVEL).iter().find_map(|e| match e {
        AiEvent::Unreachable { package, why } if package.0 == P_TRAVEL_B => Some(why.clone()),
        _ => None,
    });
    let why = why.unwrap_or_else(|| panic!("{:?}", s.events(R_TRAVEL)));
    assert!(why.contains(&format!("the way is closed by door {door} (locked, level 255)")), "{why}");
    assert!(!why.contains("(0 triangles searched)"), "the failed search is counted: {why}");
    assert_eq!(s.feet(R_TRAVEL), START);
    // Disabled: it bars nothing; the package comes back at once.
    s.game.world.set_enabled(door, false).unwrap();
    let again = s.ai.apply_door_presence(&mut s.game, &mut s.nav);
    assert!(s.nav.disabled_doors.contains(&M1));
    assert!(again.is_some(), "the presence change is seen");
    s.ai.apply_door_locks(&mut s.game, &mut s.nav);
    s.run(1.0);
    let a = &s.ai.agents[&actor];
    assert_eq!(a.activity, Activity::Travelling, "{:?}", s.events(R_TRAVEL));
    assert_eq!(a.walker.follower.as_ref().unwrap().door_ahead(a.walker.feet, f32::INFINITY), Some(DOOR_A), "no stop at the door that is not there");
    // Enabled again (locked as placed) before it gets there: its walk crosses the door,
    // so it plans again and finds the way closed.
    s.game.world.set_enabled(door, true).unwrap();
    assert_eq!(s.ai.apply_door_presence(&mut s.game, &mut s.nav), Some(vec![actor]));
    s.ai.apply_door_locks(&mut s.game, &mut s.nav);
    s.run(40.0);
    assert!(s.feet(R_TRAVEL)[0] < 196.0, "{:?}", s.feet(R_TRAVEL));
    assert_eq!(s.ai.agents[&actor].walker.space, A);
}

/// #2405 (Sally on Mothership Zeta): a closed door that opens in place named by an
/// unreachable report brings the package back the moment it opens (a script's
/// SetOpenState here), not an hour later; an open door is no barrier even locked (#2716).
#[test]
fn a_door_that_closed_the_way_opening_brings_the_package_back() {
    let mut f = fixture();
    let actor = FormId(R_TRAVEL);
    let door = FormId(M1);
    f.refs.refs.get_mut(&door).unwrap().lock = Some(fnv_sim::refs::Lock { level: 255, key: None, leveled: false });
    let mut s = sim(&f, &[R_TRAVEL]);
    s.nav.set_in_place_doors(vec![fnv_nav::InPlaceDoor {
        door: M1, space: A, position: [200.0, 200.0, 0.0],
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0,
        bounds: ([-4.0, -200.0, 0.0], [4.0, 200.0, 200.0]),
    }], |_| Some(DoorLock { level: 255, key: None }));
    s.ai.apply_door_locks(&mut s.game, &mut s.nav);
    s.run(1.0);
    assert!(s.events(R_TRAVEL).iter().any(|e| matches!(e, AiEvent::Unreachable { package, .. } if package.0 == P_TRAVEL_B)));
    assert_eq!(s.ai.apply_doors_opened(&mut s.game), None, "still closed");
    s.run(5.0);
    assert_eq!(s.feet(R_TRAVEL), START, "set aside, it stays");
    s.game.set_ref_open(door, true).unwrap();
    assert_eq!(s.ai.apply_doors_opened(&mut s.game), Some(vec![actor]));
    assert_eq!(s.ai.apply_doors_opened(&mut s.game), None, "once");
    s.run(40.0);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0, "{:?} {:?}", s.feet(R_TRAVEL), s.events(R_TRAVEL));
    assert_eq!(s.ai.agents[&actor].walker.space, B);
}

/// #3472 (Rex in Jacobstown): a package set aside as out of reach comes back the moment a
/// script puts the actor somewhere else, not an hour later: here the way to its place is
/// shut by a locked door, and a scripted door puts the actor on the far side.
#[test]
fn a_package_set_aside_comes_back_when_a_script_moves_the_actor() {
    let mut f = fixture();
    let actor = FormId(R_TRAVEL);
    let door = FormId(M1);
    f.refs.refs.get_mut(&door).unwrap().lock = Some(fnv_sim::refs::Lock { level: 255, key: None, leveled: false });
    let mut s = sim(&f, &[R_TRAVEL]);
    s.nav.set_in_place_doors(vec![fnv_nav::InPlaceDoor {
        door: M1, space: A, position: [200.0, 200.0, 0.0],
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0,
        bounds: ([-4.0, -200.0, 0.0], [4.0, 200.0, 200.0]),
    }], |_| Some(DoorLock { level: 255, key: None }));
    s.ai.apply_door_locks(&mut s.game, &mut s.nav);
    s.run(1.0);
    assert!(s.events(R_TRAVEL).iter().any(|e| matches!(e, AiEvent::Unreachable { package, .. } if package.0 == P_TRAVEL_B)));
    assert!(s.game.extra.get(&actor).is_some_and(|e| e.package_blocked.contains_key(&FormId(P_TRAVEL_B))), "set aside");
    s.run(5.0);
    assert_eq!(s.feet(R_TRAVEL), START, "set aside, it stays");
    // A script puts it in cell B, where its package's place is.
    s.game.run_source("ScriptDoor.Activate TravellerRef", Default::default()).unwrap();
    assert_eq!(s.game.cell_of(actor), Some(FormId(CELL_B)));
    assert_eq!(s.ai.moved_by_scripts(&mut s.game), vec![(actor, true)]);
    assert!(s.game.extra.get(&actor).is_none_or(|e| e.package_blocked.is_empty()), "the move lifts the block");
    assert_eq!(s.game.extra.get(&actor).and_then(|e| e.current_package), Some(FormId(P_TRAVEL_B)), "chosen again at once");
    s.run(40.0);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0, "{:?} {:?}", s.feet(R_TRAVEL), s.events(R_TRAVEL));
}

/// A scripted door that does not let the actor through (its OnActivate block called no
/// `Activate` for it, `fnv_game::npc_doors`) shuts the way as a lock does: asked as the
/// actor comes near, it plans again without the door and, with no other way, sets the
/// package aside. The refusal lapses and the way is open again.
#[test]
fn a_door_whose_script_refuses_the_actor_shuts_its_way() {
    use fnv_game::npc_doors::{DoorAnswer, Gate};
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    s.run(1.0);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    // The door's answer, as its block would give it (the fixture's door has no script).
    let until = s.game.time + 60.0;
    s.game.door_gates.insert((actor, FormId(DOOR_A)), Gate { answer: DoorAnswer::Refused, until });
    s.run(20.0);
    let ev = s.events(R_TRAVEL);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::DoorRefused { door } if door.0 == DOOR_A)), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Unreachable { package, .. } if package.0 == P_TRAVEL_B)), "{ev:?}");
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::Door { .. })), "{ev:?}");
    assert_eq!(s.ai.agents[&actor].walker.space, A);
    // Another actor's paths are not touched by it.
    assert!(s.game.refused_doors(FormId(R_IN)).is_empty());
    // Lapsed (and the package's hour over): through the door.
    s.game.door_gates.clear();
    s.game.world.set_global(FormId(G_DAYS), 5.0 + 1.1 / 24.0).unwrap();
    s.run(40.0);
    assert_eq!(s.ai.agents[&actor].walker.space, B);
}

#[test]
fn an_in_cell_location_is_met_on_entering_the_cell() {
    let f = fixture();
    let mut s = sim(&f, &[R_IN]);
    s.run(30.0);
    let agent = &s.ai.agents[&FormId(R_IN)];
    assert_eq!(agent.activity, Activity::There);
    assert_eq!(agent.walker.space, B);
    // It stopped where the door let it in, not in the middle of the cell.
    assert!(dist(s.feet(R_IN), [5050.0, 5030.0, 50.0]) < 60.0, "{:?}", s.feet(R_IN));
    assert!(matches!(s.events(R_IN).last().unwrap(), AiEvent::Arrived { .. }));
}

#[test]
fn a_radius_is_reached_at_its_edge() {
    let f = fixture();
    let mut s = sim(&f, &[R_RADIUS]);
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_RADIUS)].activity, Activity::There);
    let d = dist(s.feet(R_RADIUS), [350.0, 350.0, 0.0]);
    assert!(d <= 150.0 && d > 90.0, "stopped {d} from the marker");
}

/// At an xMarkerHeading it stands the way the marker faces (level-design.md: the
/// heading version "shows which way an actor standing there will face"; Mr. Brotch in
/// the G.O.A.T. kept the heading he walked in with and faced the wall).
#[test]
fn an_actor_at_an_xmarkerheading_turns_to_its_heading() {
    let f = fixture();
    let mut s = sim(&f, &[R_TO_HEADING]);
    s.run(30.0);
    let a = &s.ai.agents[&FormId(R_TO_HEADING)];
    assert_eq!(a.activity, Activity::There);
    assert!(a.face_to == Some(2.0) || (a.walker.heading - 2.0).abs() < 1e-3, "{:?} {}", a.face_to, a.walker.heading);
}

// [design, #2526] Synthetic variants of the heading fixture exercise the same
// destination lookup for walking and facing, including a missing linked reference.
#[test]
fn arrival_heading_uses_linked_and_object_locations() {
    for kind in [LocationKind::NearLinked, LocationKind::ObjectId(Some(FormId(0x34)))] {
        let mut f = fixture();
        f.data.package_defs.get_mut(&FormId(P_TO_HEADING)).unwrap().location.as_mut().unwrap().kind = kind;
        f.refs.refs.get_mut(&FormId(R_TO_HEADING)).unwrap().linked = Some(FormId(HEAD_MARK));
        let mut s = sim(&f, &[R_TO_HEADING]);
        s.run(30.0);
        let a = &s.ai.agents[&FormId(R_TO_HEADING)];
        assert_eq!(a.activity, Activity::There, "{kind:?}");
        assert!(dist(a.walker.feet, [300.0, 200.0, 0.0]) < 1.0, "{kind:?}");
        assert!(fnv_nav::follow::heading_delta(a.walker.heading, 2.0).abs() < 1e-3, "{kind:?}: {}", a.walker.heading);
        assert!(s.events(R_TO_HEADING).iter().any(|e| matches!(e, AiEvent::ArrivalFacing { heading } if *heading == 2.0)));
    }
}

// [design, #2526; goal::resolve] A remain-near Guard travels to its target, even
// with a different linked heading marker stored as its location.
#[test]
fn arrival_heading_for_a_guard_uses_its_target_not_its_location() {
    use fnv_game::package::{PackageType, Target, TargetKind};
    for heading_target in [false, true] {
        let mut f = fixture();
        let def = f.data.package_defs.get_mut(&FormId(P_TO_HEADING)).unwrap();
        def.ty = PackageType::Guard;
        def.type_flags = type_flags::REMAIN_NEAR_REFERENCE;
        def.location.as_mut().unwrap().kind = LocationKind::NearLinked;
        def.target = Some(Target { kind: TargetKind::Reference(Some(FormId(MARK_A))), count_distance: 0 });
        f.refs.refs.get_mut(&FormId(R_TO_HEADING)).unwrap().linked = Some(FormId(HEAD_MARK));
        let target = f.refs.refs.get_mut(&FormId(MARK_A)).unwrap();
        target.rot[2] = 0.75;
        if heading_target {
            target.base = FormId(0x34);
        }
        let destination = target.pos;
        let mut s = sim(&f, &[R_TO_HEADING]);
        s.run(30.0);
        let a = &s.ai.agents[&FormId(R_TO_HEADING)];
        assert_eq!(a.activity, Activity::There);
        assert!(dist(a.walker.feet, destination) < 1.0);
        let headings: Vec<_> = s.events(R_TO_HEADING).into_iter().filter_map(|e| match e {
            AiEvent::ArrivalFacing { heading } => Some(*heading),
            _ => None,
        }).collect();
        if heading_target {
            assert_eq!(headings, vec![0.75]);
            assert!(fnv_nav::follow::heading_delta(a.walker.heading, 0.75).abs() < 1e-3);
        } else {
            assert!(headings.is_empty(), "plain target must not use linked marker: {headings:?}");
        }
    }
}

// [design, #2526] Simulate a detour by moving the travelling actor into the chosen
// marker's radius from the far side, closer to a competing marker with another heading.
#[test]
fn arrival_heading_keeps_the_object_reference_selected_before_travel() {
    for kind in [LocationKind::ObjectId(Some(FormId(0x34))), LocationKind::ObjectType(9)] {
        let mut f = fixture();
        // [design: synthetic fixture] Classify the test marker base as Misc so the
        // object-type variant can select these same two references (`errand::of_type`).
        f.data.sigs.insert(FormId(0x34), *b"MISC");
        let loc = f.data.package_defs.get_mut(&FormId(P_TO_HEADING)).unwrap().location.as_mut().unwrap();
        loc.kind = kind;
        loc.radius = 150;
        f.refs.refs.get_mut(&FormId(HEAD_MARK)).unwrap().pos = [250.0, 200.0, 0.0];
        let other = f.refs.refs.get_mut(&FormId(MARK_A)).unwrap();
        other.base = FormId(0x34);
        other.pos = [390.0, 200.0, 0.0];
        other.rot[2] = 1.25;
        let mut s = sim(&f, &[R_TO_HEADING]);
        let actor = FormId(R_TO_HEADING);
        s.run(0.5);
        assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling, "{kind:?}");
        let detour = [380.0, 200.0, 0.0];
        s.game.world.get(actor).unwrap().pos = detour;
        s.ai.agents.get_mut(&actor).unwrap().walker.feet = detour;
        s.ai.update(&mut s.game, &s.nav, 0.05);
        assert_eq!(s.ai.agents[&actor].activity, Activity::There, "{kind:?}");
        let at = s.feet(R_TO_HEADING);
        assert!(dist(at, [390.0, 200.0, 0.0]) < dist(at, [250.0, 200.0, 0.0]), "{at:?}");
        s.run(10.0);
        assert!(s.events(R_TO_HEADING).iter().any(|e| matches!(e, AiEvent::ArrivalFacing { heading } if *heading == 2.0)), "{kind:?}: {:?}", s.events(R_TO_HEADING));
        assert!(fnv_nav::follow::heading_delta(s.ai.agents[&actor].walker.heading, 2.0).abs() < 1e-3);
    }
}

// [documented: #2526 w2 brief, main-session fnvtool ai show / quest SLSD/SCVR,
// FalloutNV.esm, 2026-11-15] Subject-run AND conditions and list order reproduced
// with synthetic quests, actors and package IDs; no record bytes or script text.
#[test]
fn speech_guard_conditions_select_the_linked_travel_without_an_override() {
    use fnv_game::cond::{Cmp, Condition, Operand, RunOn};
    use fnv_game::game::InstKey;
    use fnv_game::package::PackageType;
    use fnv_game::quest::QuestDef;
    use fnv_script::vm::{ArgValue, Value};

    let mut f = fixture();
    let (main, speech) = (FormId(0xA10), FormId(0xA11));
    for form in [main, speech] {
        f.data.quests.insert(form, QuestDef { form, ..Default::default() });
    }
    let cond = |function, params, cmp, rhs| Condition { function, params, cmp, rhs: Operand::Value(rhs), or: false, run_on: RunOn::Subject };
    let var = |quest: FormId, index: u16, cmp, rhs| cond(79, vec![ArgValue::Value(Value::Ref(quest.0)), ArgValue::Value(Value::Num(index as f64))], cmp, rhs);
    let is = |actor| cond(136, vec![ArgValue::Value(Value::Ref(actor))], Cmp::Eq, 1.0);
    let front = FormId(0xA00);
    let dead = FormId(0xA01);
    let idle = FormId(0xA02);
    let safe_one = FormId(0xA03);
    let safe_two = FormId(0xA04);
    let follow_one = FormId(0xA05);
    let follow_two = FormId(0xA06);
    let defs = [
        (front, vec![var(main, 2, Cmp::Ne, 1.0), var(main, 11, Cmp::Lt, 2.0), var(main, 9, Cmp::Lt, 3.0), var(main, 5, Cmp::Eq, 2.0), is(R_RADIUS)]),
        (dead, vec![var(main, 26, Cmp::Eq, 1.0)]),
        (idle, vec![var(main, 26, Cmp::Eq, 0.0), var(main, 2, Cmp::Eq, 0.0), var(speech, 4, Cmp::Eq, 2.0)]),
        (safe_one, vec![var(main, 3, Cmp::Eq, 1.0), var(main, 2, Cmp::Eq, 1.0), is(R_TO_HEADING)]),
        (safe_two, vec![var(main, 3, Cmp::Eq, 1.0), var(main, 2, Cmp::Eq, 1.0), is(R_RADIUS)]),
        (follow_one, vec![is(R_TO_HEADING)]),
        (follow_two, vec![is(R_RADIUS)]),
    ];
    let template = f.data.package_defs[&FormId(P_TO_HEADING)].clone();
    for (id, conditions) in defs {
        let mut def = template.clone();
        def.conditions = conditions;
        if id == idle {
            def.location.as_mut().unwrap().kind = LocationKind::NearLinked;
        }
        if id == dead {
            def.ty = PackageType::Guard;
        }
        f.data.package_defs.insert(id, def);
    }
    let list = vec![front, dead, idle, safe_one, safe_two, follow_one, follow_two];
    for (base, actor) in [(N_TO_HEADING, R_TO_HEADING), (N_RADIUS, R_RADIUS)] {
        f.data.actors.get_mut(&FormId(base)).unwrap().packages = list.clone();
        f.refs.refs.get_mut(&FormId(actor)).unwrap().linked = Some(FormId(HEAD_MARK));
    }
    let mut s = sim(&f, &[R_TO_HEADING, R_RADIUS]);
    let set = |s: &mut Sim, quest, index, value| s.game.store(InstKey::Quest { quest }, None, index, Value::Num(value), "synthetic speech state", false);
    let picks = |s: &mut Sim| [s.game.select_package(FormId(R_TO_HEADING)), s.game.select_package(FormId(R_RADIUS))];
    assert_eq!(picks(&mut s), [Some(follow_one), Some(follow_two)]);
    set(&mut s, speech, 4, 2.0);
    for actor in [R_TO_HEADING, R_RADIUS] {
        assert_eq!(s.game.check_package(FormId(actor), front), PackageCheck::ConditionsFail);
        assert_eq!(s.game.check_package(FormId(actor), dead), PackageCheck::ConditionsFail);
        assert_eq!(s.game.check_package(FormId(actor), idle), PackageCheck::Valid);
    }
    assert_eq!(picks(&mut s), [Some(idle), Some(idle)]);
    s.run(30.0);
    for actor in [R_TO_HEADING, R_RADIUS] {
        assert!(s.game.extra[&FormId(actor)].script_package.is_none());
        assert_eq!(s.game.extra[&FormId(actor)].current_package, Some(idle));
        assert!(s.events(actor).iter().any(|e| matches!(e, AiEvent::ArrivalFacing { heading } if *heading == 2.0)));
    }
    // [design: condition audit] StageFront wins only for actor two during attack;
    // each other conjunct can veto it, leaving SpeechIdle valid in these states.
    set(&mut s, main, 5, 2.0);
    assert_eq!(picks(&mut s), [Some(idle), Some(front)]);
    for (index, threshold) in [(11, 2.0), (9, 3.0)] {
        set(&mut s, main, index, threshold);
        assert_eq!(picks(&mut s), [Some(idle), Some(idle)]);
        set(&mut s, main, index, 0.0);
    }
    set(&mut s, main, 26, 1.0);
    assert_eq!(picks(&mut s), [Some(dead), Some(front)]);
    set(&mut s, main, 5, 0.0);
    assert_eq!(picks(&mut s), [Some(dead), Some(dead)]);
    set(&mut s, main, 26, 0.0);
    set(&mut s, main, 2, 1.0);
    assert_eq!(picks(&mut s), [Some(follow_one), Some(follow_two)]);
    set(&mut s, main, 3, 1.0);
    assert_eq!(picks(&mut s), [Some(safe_one), Some(safe_two)]);
}

#[test]
fn arrival_heading_leaves_plain_xmarker_facing_unchanged() {
    let mut f = fixture();
    f.data.package_defs.get_mut(&FormId(P_TO_HEADING)).unwrap().location.as_mut().unwrap().kind = LocationKind::NearLinked;
    f.refs.refs.get_mut(&FormId(R_TO_HEADING)).unwrap().linked = Some(FormId(HEAD_MARK));
    // [design: fixture] A plain marker base, with an arbitrary placed heading.
    f.refs.refs.get_mut(&FormId(HEAD_MARK)).unwrap().base = FormId(STAT);
    let mut s = sim(&f, &[R_TO_HEADING]);
    s.run(30.0);
    let a = &s.ai.agents[&FormId(R_TO_HEADING)];
    assert_eq!(a.activity, Activity::There);
    assert_eq!(a.face_to, None);
    assert!(!s.events(R_TO_HEADING).iter().any(|e| matches!(e, AiEvent::ArrivalFacing { .. })));
    let heading = a.walker.heading;
    s.run(10.0);
    assert_eq!(s.ai.agents[&FormId(R_TO_HEADING)].walker.heading, heading);
    assert!(fnv_nav::follow::heading_delta(heading, 2.0).abs() > 0.1);
}

#[test]
fn arrival_heading_needs_an_existing_link() {
    let mut f = fixture();
    f.data.package_defs.get_mut(&FormId(P_TO_HEADING)).unwrap().location.as_mut().unwrap().kind = LocationKind::NearLinked;
    let mut s = sim(&f, &[R_TO_HEADING]);
    s.run(30.0);
    assert!(!s.events(R_TO_HEADING).iter().any(|e| matches!(e, AiEvent::ArrivalFacing { .. })));
    assert_eq!(s.ai.agents[&FormId(R_TO_HEADING)].face_to, None);
}

// (#2526, observer check 213; see crates/fnv-ai/src/carry.rs) Travel and standing Guard restore the
// editor heading on returning to a small editor location, using the standing turn.
#[test]
fn arrival_heading_at_small_editor_locations_restores_editor_rotation() {
    use fnv_game::package::PackageType;
    for ty in [PackageType::Travel, PackageType::Guard] {
        for radius in [0, 15] {
            let mut f = fixture();
            let def = f.data.package_defs.get_mut(&FormId(P_TO_HEADING)).unwrap();
            def.ty = ty;
            let loc = def.location.as_mut().unwrap();
            loc.kind = LocationKind::NearEditor;
            loc.radius = radius;
            f.refs.refs.get_mut(&FormId(R_TO_HEADING)).unwrap().rot[2] = 1.25;
            let mut s = sim(&f, &[R_TO_HEADING]);
            let actor = FormId(R_TO_HEADING);
            let away = [200.0, 200.0, 0.0];
            s.game.world.get(actor).unwrap().pos = away;
            s.ai.agents.get_mut(&actor).unwrap().walker.feet = away;
            s.run(30.0);
            let a = &s.ai.agents[&actor];
            assert_eq!(a.activity, Activity::There, "{ty:?}, {radius}");
            assert!(dist(a.walker.feet, START) <= radius as f32 + 1.0);
            assert!(fnv_nav::follow::heading_delta(a.walker.heading, 1.25).abs() < 1e-3, "{ty:?}, {radius}: {}", a.walker.heading);
            assert!(s.events(R_TO_HEADING).iter().any(|e| matches!(e, AiEvent::ArrivalFacing { heading } if *heading == 1.25)));
            assert!(a.sandbox.is_none());
        }
    }
}

#[test]
fn arrival_heading_does_not_override_editor_sandbox_or_guard_wander() {
    use fnv_game::package::PackageType;
    for (ty, radius) in [(PackageType::Travel, 16), (PackageType::Guard, 16), (PackageType::Sandbox, 0), (PackageType::Wander, 0)] {
        let mut f = fixture();
        let def = f.data.package_defs.get_mut(&FormId(P_TO_HEADING)).unwrap();
        def.ty = ty;
        let loc = def.location.as_mut().unwrap();
        loc.kind = LocationKind::NearEditor;
        loc.radius = radius;
        let mut s = sim(&f, &[R_TO_HEADING]);
        s.run(30.0);
        assert!(!s.events(R_TO_HEADING).iter().any(|e| matches!(e, AiEvent::ArrivalFacing { .. })), "{ty:?}, {radius}");
        if ty != PackageType::Travel {
            assert!(s.ai.agents[&FormId(R_TO_HEADING)].sandbox.is_some(), "{ty:?}");
        }
    }
}

#[test]
fn always_run_packages_run() {
    let f = fixture();
    let mut s = sim(&f, &[R_RUN, R_RADIUS]);
    s.run(1.0);
    assert!(matches!(s.events(R_RUN)[1], AiEvent::Trip { running: true, .. }));
    // One second at 400 units a second against 100.
    assert!(dist(s.feet(R_RUN), START) > 3.0 * dist(s.feet(R_RADIUS), START));
}

/// Speeds of `walk` and `run`, calm and alert alike, as the engine gives people
/// (`creature_pace.rs`).
fn speeds(walk: f32, run: f32) -> fnv_ai::ActorSpeeds {
    let s = fnv_ai::Speeds { walk, run };
    fnv_ai::ActorSpeeds { calm: s, alert: s, turn_rate: None, spot_turn_start: None }
}

/// How far `actor` goes in one update of half a second.
fn step_length(s: &mut Sim, actor: u32) -> f32 {
    let before = s.ai.agents[&FormId(actor)].walker.travelled;
    s.run(0.5);
    s.ai.agents[&FormId(actor)].walker.travelled - before
}

/// The package and trip events so far: a new speed must not make the actor choose or
/// plan again.
fn plans(s: &Sim, actor: u32) -> usize {
    s.events(actor).iter().filter(|e| matches!(e, AiEvent::Package { .. } | AiEvent::Trip { .. })).count()
}

/// A walk under way takes new speeds at the next step, in its gait, without a new
/// package or plan: Speed Mult 100 → 200 → 0 → 100 (issue #2118: `plan` fixes the
/// walker's speed when it plans, and `set_speeds` did not reach it). Crippled legs'
/// pace stays its own.
#[test]
fn a_walk_under_way_takes_new_speeds_at_once_in_its_gait() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    // Speed Mult 100 of speeds half the configured ones, given before it plans, so the
    // way to the door lasts the test; planned at that walk, past its first turn.
    s.ai.set_speeds(actor, speeds(50.0, 200.0));
    s.run(1.0);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    let planned = plans(&s, R_TRAVEL);
    let near = |got: f32, want: f32| assert!((got - want).abs() <= want.max(1.0) * 0.1, "{got} against {want}");
    near(step_length(&mut s, R_TRAVEL), 25.0);
    // Speed Mult 200.
    s.ai.set_speeds(actor, speeds(100.0, 400.0));
    near(step_length(&mut s, R_TRAVEL), 50.0);
    // Speed Mult 0: it stands on its way, still travelling.
    s.ai.set_speeds(actor, speeds(0.0, 0.0));
    assert!(step_length(&mut s, R_TRAVEL).abs() < 1e-3);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    // Back to 100: it walks on, not runs (200 units/s would be 100 a step).
    s.ai.set_speeds(actor, speeds(50.0, 200.0));
    near(step_length(&mut s, R_TRAVEL), 25.0);
    // A crippled leg's pace is kept across a new speed: half of Speed Mult 200's walk.
    s.ai.set_pace(actor, 0.5);
    s.ai.set_speeds(actor, speeds(100.0, 400.0));
    near(step_length(&mut s, R_TRAVEL), 25.0);
    assert_eq!(s.ai.agents[&actor].walker.pace, 0.5);
    assert_eq!(plans(&s, R_TRAVEL), planned, "{:?}", s.events(R_TRAVEL));
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
}

/// A run under way keeps running through new speeds and a stop at 0 (an Always Run
/// package), at the new run speed.
#[test]
fn a_run_under_way_keeps_its_gait_through_a_stop() {
    let f = fixture();
    let mut s = sim(&f, &[R_RUN]);
    let actor = FormId(R_RUN);
    // Slow speeds before it plans, so the way to its marker lasts the test.
    s.ai.set_speeds(actor, speeds(10.0, 40.0));
    s.run(1.0);
    assert!(matches!(s.events(R_RUN)[1], AiEvent::Trip { running: true, .. }));
    let planned = plans(&s, R_RUN);
    let near = |got: f32, want: f32| assert!((got - want).abs() <= want.max(1.0) * 0.1, "{got} against {want}");
    near(step_length(&mut s, R_RUN), 20.0);
    s.ai.set_speeds(actor, speeds(20.0, 80.0));
    near(step_length(&mut s, R_RUN), 40.0);
    s.ai.set_speeds(actor, speeds(0.0, 0.0));
    assert!(step_length(&mut s, R_RUN).abs() < 1e-3);
    // Back: the run's 40 units/s (20 a step), not the walk's 10.
    s.ai.set_speeds(actor, speeds(10.0, 40.0));
    near(step_length(&mut s, R_RUN), 20.0);
    assert_eq!(plans(&s, R_RUN), planned, "{:?}", s.events(R_RUN));
}

/// A walk planned while the actor's speeds are 0 (Speed Mult 0 before its package's
/// walk) does not stay standing when they come back: it goes on at the calm walk.
#[test]
fn a_walk_planned_at_speed_0_sets_off_when_the_speed_comes_back() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    s.ai.set_speeds(actor, speeds(0.0, 0.0));
    s.run(1.0);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(step_length(&mut s, R_TRAVEL).abs() < 1e-3);
    let planned = plans(&s, R_TRAVEL);
    s.ai.set_speeds(actor, speeds(50.0, 200.0));
    // Its first step may still turn it towards its way.
    s.run(1.0);
    let near = |got: f32, want: f32| assert!((got - want).abs() <= want * 0.1, "{got} against {want}");
    near(step_length(&mut s, R_TRAVEL), 25.0);
    assert_eq!(plans(&s, R_TRAVEL), planned);
}

/// A package's walk planned while the speeds are 0 goes on in its own gait when they come
/// back: an Always Run package runs; and a walking package taken up at 0 after a run was
/// stopped walks, not at the stopped run's speed (the stop belongs to the walk, not to the
/// actor).
#[test]
fn a_package_walk_planned_at_speed_0_keeps_its_own_gait() {
    let f = fixture();
    let near = |got: f32, want: f32| assert!((got - want).abs() <= want * 0.1, "{got} against {want}");
    // Planned at 0: the run, 40 units/s (20 a step), not the walk's 10.
    let mut s = sim(&f, &[R_RUN]);
    let actor = FormId(R_RUN);
    s.ai.set_speeds(actor, speeds(0.0, 0.0));
    s.run(1.0);
    assert!(matches!(s.events(R_RUN)[1], AiEvent::Trip { running: true, .. }));
    assert!(step_length(&mut s, R_RUN).abs() < 1e-3);
    s.ai.set_speeds(actor, speeds(10.0, 40.0));
    s.run(1.0);
    near(step_length(&mut s, R_RUN), 20.0);
    // Stopped running, then given a walking package while still at 0.
    s.ai.set_speeds(actor, speeds(0.0, 0.0));
    assert!(step_length(&mut s, R_RUN).abs() < 1e-3);
    s.game.extra.entry(actor).or_default().script_package = Some(FormId(P_TRAVEL_B));
    assert_eq!(s.game.evaluate_package(actor), Some(FormId(P_TRAVEL_B)));
    s.run(0.5);
    assert!(s.events(R_RUN).iter().any(|e| matches!(e, AiEvent::Trip { package, running: false, .. } if package.0 == P_TRAVEL_B)), "{:?}", s.events(R_RUN));
    assert!(step_length(&mut s, R_RUN).abs() < 1e-3);
    // Back: the new walk's 10 units/s (5 a step), not the old run's 40.
    s.ai.set_speeds(actor, speeds(10.0, 40.0));
    s.run(1.0);
    near(step_length(&mut s, R_RUN), 5.0);
}

#[test]
fn a_patrol_walks_its_linked_references_in_order_once() {
    let f = fixture();
    let mut s = sim(&f, &[R_PATROL]);
    s.run(60.0);
    let points: Vec<u32> = s.events(R_PATROL).iter().filter_map(|e| match e {
        AiEvent::Arrived { patrol_point: Some(p), .. } => Some(p.0),
        _ => None,
    }).collect();
    assert_eq!(points, vec![M1, M2, M3]);
    let actor = FormId(R_PATROL);
    assert_eq!(s.ai.agents[&actor].activity, Activity::There);
    assert!(dist(s.feet(R_PATROL), [50.0, 350.0, 0.0]) < 5.0);
    assert!(s.game.extra[&actor].package_goal_met);
}

#[test]
fn a_patrol_without_allow_falls_takes_only_harmless_authored_drops() {
    use fnv_nav::record::{link_kind, EdgeLink};

    for (min, height, falls, reaches) in [
        (None, 600.0, false, true),
        (None, 601.0, false, false),
        (Some(250.0f32), 300.0, false, false),
        (Some(300.0), 300.0, false, true),
        (Some(250.0), 700.0, true, true),
    ] {
        let general = flags::MUST_COMPLETE | flags::MUST_REACH_LOCATION | flags::ALWAYS_RUN | if falls { flags::ALLOW_FALLS } else { 0 };
        let refs = [
            placed(b"ACHR", R_PATROL, "DropPatrollerRef", N_PATROL, START, None),
            placed(b"REFR", M1, "DropMarkerRef", STAT, [650.0, 50.0, -height], None),
        ].concat();
        let cell = [
            record(b"CELL", CELL_A, &[sub(b"EDID", z("DropCell")), sub(b"DATA", 1u16.to_le_bytes().to_vec())]),
            group(CELL_A.to_le_bytes(), 6, &group(CELL_A.to_le_bytes(), 9, &refs)),
        ].concat();
        let mut bytes = [
            tes4(),
            group(*b"STAT", 0, &record(b"STAT", STAT, &[sub(b"EDID", z("DropMarker"))])),
            group(*b"PACK", 0, &pack(P_PATROL, "DropPatrol", general, PATROL, (0, M1, 0), Some(false))),
            group(*b"NPC_", 0, &npc(N_PATROL, "DropPatroller", &[P_PATROL])),
            group(*b"CELL", 0, &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
        ].concat();
        if let Some(min) = min {
            bytes.extend(group(*b"GMST", 0, &record(b"GMST", 0xF80, &[sub(b"EDID", z("fJumpFallHeightMin")), sub(b"DATA", min.to_le_bytes().to_vec())])));
        }
        let f = fixture_of(bytes);
        let mut s = sim(&f, &[R_PATROL]);
        let mut top = grid(1, [0.0, 0.0], 4, 0.0);
        let bottom = grid(2, [400.0, 0.0], 4, -height);
        top.triangles[6].flags |= tri_flags::EDGE_LINK_0 << 1;
        top.triangles[6].edges[1] = 0;
        top.edge_links.push(EdgeLink { kind: link_kind::LEDGE_DOWN, navmesh: Some(2), triangle: 1 });
        s.nav = NavGraph::assemble(&[(&top, Some(CELL_A), Some(A)), (&bottom, Some(CELL_A), Some(A))]);
        s.run(20.0);
        let arrived = s.events(R_PATROL).iter().any(|e| matches!(e, AiEvent::Arrived { patrol_point: Some(p), .. } if p.0 == M1));
        let unreachable = s.events(R_PATROL).iter().any(|e| matches!(e, AiEvent::Unreachable { .. }));
        assert_eq!(arrived, reaches, "min={min:?}, height={height}, falls={falls}: {:?}", s.events(R_PATROL));
        assert_eq!(unreachable, !reaches);
        if reaches {
            assert!((s.feet(R_PATROL)[2] + height).abs() < 1.0);
        }
    }
}

/// After a load (a new AI from the same game state) a patrol that does not repeat goes
/// on to the point the save had it walk to, instead of starting again at its first
/// (`fnv_ai::progress`, docs/design/save-load.md §5 item 4).
#[test]
fn a_loaded_patrol_goes_on_from_the_saved_point() {
    let f = fixture();
    let mut s = sim(&f, &[R_PATROL]);
    let actor = FormId(R_PATROL);
    let arrivals = |s: &Sim| -> Vec<u32> {
        s.events(R_PATROL).iter().filter_map(|e| match e {
            AiEvent::Arrived { patrol_point: Some(p), .. } => Some(p.0),
            _ => None,
        }).collect()
    };
    while arrivals(&s).len() < 2 {
        s.run(0.5);
    }
    assert_eq!(arrivals(&s), vec![M1, M2]);
    // Once it has set off for M3 (after its wait at M2), save.
    let point = |s: &Sim| s.ai.progress().iter().find(|p| p.actor == actor).and_then(|p| p.patrol_point);
    while point(&s) == Some(1) {
        s.run(0.5);
    }
    s.run(1.0);
    let saved = s.ai.progress();
    let p = saved.iter().find(|p| p.actor == actor).expect("a patroller has progress");
    assert_eq!(p.patrol_point, Some(2));
    let json = serde_json::to_string(&saved).unwrap();
    // Loaded: a new AI over the same game, the progress put back.
    let mut ai = Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() });
    ai.add(&s.game, actor).unwrap();
    ai.restore(serde_json::from_str(&json).unwrap());
    s.ai = ai;
    s.run(60.0);
    assert_eq!(arrivals(&s), vec![M3], "no walk back to M1");
    assert!(dist(s.feet(R_PATROL), [50.0, 350.0, 0.0]) < 5.0);
    // Without the progress it starts again at M1.
    let mut s = sim(&f, &[R_PATROL]);
    while arrivals(&s).len() < 2 {
        s.run(0.5);
    }
    s.run(1.0);
    let mut ai = Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() });
    ai.add(&s.game, actor).unwrap();
    s.ai = ai;
    s.run(60.0);
    assert_eq!(arrivals(&s)[0], M1);
}

#[test]
fn a_repeatable_patrol_starts_at_the_nearest_point_and_goes_round() {
    let f = fixture();
    let mut s = sim(&f, &[R_LOOP]);
    s.run(40.0);
    let points: Vec<u32> = s.events(R_LOOP).iter().filter_map(|e| match e {
        AiEvent::Arrived { patrol_point: Some(p), .. } => Some(p.0),
        _ => None,
    }).collect();
    // The sentry stands next to L3: there first, then round the loop, again and again.
    assert!(points.len() >= 6, "{points:x?}");
    assert_eq!(&points[..6], &[L3, L1, L2, L3, L1, L2]);
    let actor = FormId(R_LOOP);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(!s.game.extra[&actor].package_goal_met);
}

#[test]
fn packages_that_are_not_carried_out_leave_the_actor_where_it_is() {
    let f = fixture();
    let mut s = sim(&f, &[R_FOLLOW, R_HERE]);
    s.run(15.0);
    assert_eq!(s.ai.agents[&FormId(R_FOLLOW)].activity, Activity::Staying);
    assert!(matches!(s.events(R_FOLLOW)[1], AiEvent::NotCarriedOut { .. }));
    assert_eq!(s.events(R_FOLLOW).len(), 2);
    assert!(!s.game.extra[&FormId(R_FOLLOW)].package_goal_met);
    assert_eq!(s.feet(R_FOLLOW), START);
    // "Near current location": there at once, and the goal is met.
    assert_eq!(s.ai.agents[&FormId(R_HERE)].activity, Activity::There);
    assert!(matches!(s.events(R_HERE)[1], AiEvent::AlreadyThere { .. }));
    assert!(s.game.extra[&FormId(R_HERE)].package_goal_met);
}

/// An Accompany package: the actor keeps near its target wherever it goes, through the
/// door into the other cell too, and stands while the target stands (`goal::Follow`;
/// the owner's playtest of 2026-10-11: Sunny Smiles's dog stayed behind).
#[test]
fn an_accompanying_actor_follows_its_target_through_a_door() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL, R_DOG]);
    s.run(1.0);
    // Within its distance of the target at first: it stands.
    assert_eq!(s.ai.agents[&FormId(R_DOG)].activity, Activity::There);
    assert!(matches!(s.events(R_DOG)[..], [AiEvent::Package { .. }]), "{:?}", s.events(R_DOG));
    // The traveller walks off to cell B; the dog sets off after it and goes through.
    s.run(40.0);
    let ev = s.events(R_DOG);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Follow { target, .. } if target.0 == R_TRAVEL)), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Door { door, .. } if door.0 == DOOR_A)), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::CaughtUp { .. })), "{ev:?}");
    assert_eq!(s.ai.agents[&FormId(R_DOG)].walker.space, B);
    assert!(dist(s.feet(R_DOG), s.feet(R_TRAVEL)) <= 128.0 + 64.0 + 1.0, "{:?} {:?}", s.feet(R_DOG), s.feet(R_TRAVEL));
    assert_eq!(s.ai.agents[&FormId(R_DOG)].activity, Activity::There);
    assert_eq!(s.game.world.peek(FormId(R_DOG)).unwrap().cell, Some(FormId(CELL_B)));
    // The traveller stands at its marker: so does the dog, and it does not start over.
    let follows = ev.iter().filter(|e| matches!(e, AiEvent::Follow { .. })).count();
    s.run(20.0);
    assert_eq!(s.events(R_DOG).iter().filter(|e| matches!(e, AiEvent::Follow { .. })).count(), follows);
}

/// A follower after a target that runs, stops and then walks on catches up and stands
/// while the target stands, and sets off again when it goes on (smoke sunny-wells:
/// Cheyenne after Sunny; she never counted as caught up on main 8ec4062d).
#[test]
fn a_follower_stands_when_its_running_target_stops() {
    let f = fixture();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let game = Game::new(&f.data, world, GameConfig { strict: true, frame_seconds: 0.5, package_interval: Some(10.0), ..GameConfig::default() });
    let mut s = Sim { game, ai: Ai::new(AiConfig { walk_speed: 77.0, run_speed: 308.0, ..AiConfig::default() }), nav: nav() };
    let (mate, player) = (FormId(R_COMPANION), FormId::PLAYER_REF);
    for r in [player, mate] {
        let y = if r == player { 10250.0 } else { 10050.0 };
        let p = s.game.world.get(r).unwrap();
        (p.cell, p.pos, p.rot) = (Some(FormId(CELL_C)), [10100.0, y, 0.0], [0.0; 3]);
    }
    s.ai.add(&s.game, mate).unwrap();
    let own = fnv_ai::Speeds { walk: 57.0, run: 333.0 };
    s.ai.set_speeds(mate, fnv_ai::ActorSpeeds { calm: own, alert: own, turn_rate: None, spot_turn_start: None });
    s.run(1.0);
    // The player runs at 300 units/s for 2 seconds, then stands for 10.
    for _ in 0..4 {
        s.game.world.get(player).unwrap().pos[1] += 150.0;
        s.run(0.5);
    }
    s.run(10.0);
    let ev = s.events(R_COMPANION);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::CaughtUp { .. })), "{ev:?}");
    assert_eq!(s.ai.agents[&mate].activity, Activity::There, "{ev:?}");
    let follows = ev.iter().filter(|e| matches!(e, AiEvent::Follow { .. })).count();
    // It walks on: the follower sets off after it again.
    for _ in 0..8 {
        s.game.world.get(player).unwrap().pos[1] += 40.0;
        s.run(0.5);
    }
    let ev = s.events(R_COMPANION);
    assert_eq!(ev.iter().filter(|e| matches!(e, AiEvent::Follow { .. })).count(), follows + 1, "{ev:?}");
}

/// A follower keeps step with a target that walks on, rather than catching up, stopping
/// and setting off again every few steps, or swapping between its walk and its run (owner's
/// playtest of 2026-10-24: the Novac pack brahmin after its merchant).
#[test]
fn a_follower_keeps_step_with_a_walking_target() {
    let f = fixture();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let game = Game::new(&f.data, world, GameConfig { strict: true, frame_seconds: 0.5, package_interval: Some(10.0), ..GameConfig::default() });
    let mut s = Sim { game, ai: Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() }), nav: nav() };
    let (mate, player) = (FormId(R_COMPANION), FormId::PLAYER_REF);
    // Both in the large cell C, the companion 200 units behind the player.
    for (r, x) in [(player, 10100.0), (mate, 10100.0)] {
        let y = if r == player { 10250.0 } else { 10050.0 };
        let p = s.game.world.get(r).unwrap();
        (p.cell, p.pos, p.rot) = (Some(FormId(CELL_C)), [x, y, 0.0], [0.0; 3]);
    }
    s.ai.add(&s.game, mate).unwrap();
    // The companion's own walk is slower than the player's 100 units/s.
    let own = fnv_ai::Speeds { walk: 70.0, run: 300.0 };
    s.ai.set_speeds(mate, fnv_ai::ActorSpeeds { calm: own, alert: own, turn_rate: None, spot_turn_start: None });
    s.run(1.0);
    let (mut speeds, mut gaps) = (Vec::new(), Vec::new());
    // The player walks on in a straight line at 100 units/s for 8 seconds.
    for _ in 0..16 {
        s.game.world.get(player).unwrap().pos[1] += 50.0;
        s.run(0.5);
        if s.ai.agents[&mate].activity == Activity::Travelling {
            speeds.push(s.ai.agents[&mate].walker.speed);
        }
        gaps.push(dist(s.feet(R_COMPANION), s.game.world.peek(player).unwrap().pos));
    }
    let ev = s.events(R_COMPANION);
    let follows = ev.iter().filter(|e| matches!(e, AiEvent::Follow { .. })).count();
    // It sets off once, walking, and keeps going: no stop and start, never its run.
    assert_eq!(follows, 1, "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Follow { running: false, .. })), "{ev:?}");
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::CaughtUp { .. })), "{ev:?}");
    assert!(speeds.len() >= 10, "{speeds:?}");
    assert!(speeds.iter().all(|v| *v < 200.0), "{speeds:?}");
    // In step: at the player's pace, not its own slower walk, and the gap stays near its
    // distance (256) rather than growing.
    let last = *speeds.last().unwrap();
    assert!((95.0..=140.0).contains(&last), "it goes at {last}: {speeds:?}");
    assert!(gaps.iter().all(|g| *g < 256.0 + 64.0 + 80.0), "{gaps:?}");
}

/// Two actors following one target walk after it in a line: the one nearer the target
/// at first after the target, the other after it, nearer than the package's distance
/// (`Ai::ahead_in_column`, `COLUMN_GAP`; the owner's playtest of 2026-10-24).
#[test]
fn two_actors_following_one_target_walk_in_a_line() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL, R_DOG, R_DOG2]);
    s.run(40.0);
    let first = s.events(R_DOG);
    assert!(first.iter().any(|e| matches!(e, AiEvent::Follow { target, .. } if target.0 == R_TRAVEL)), "{first:?}");
    let second = s.events(R_DOG2);
    assert!(second.iter().any(|e| matches!(e, AiEvent::Follow { target, .. } if target.0 == R_DOG)), "{second:?}");
    assert!(!second.iter().any(|e| matches!(e, AiEvent::Follow { target, .. } if target.0 == R_TRAVEL)), "{second:?}");
    // Both went through to cell B and stand there in a line.
    for d in [R_DOG, R_DOG2] {
        assert_eq!(s.ai.agents[&FormId(d)].walker.space, B);
        assert_eq!(s.ai.agents[&FormId(d)].activity, Activity::There);
    }
    let (t, d1, d2) = (s.feet(R_TRAVEL), s.feet(R_DOG), s.feet(R_DOG2));
    assert!(dist(d1, t) <= 128.0 + 64.0 + 1.0, "{d1:?} {t:?}");
    assert!(dist(d2, d1) <= fnv_ai::carry::COLUMN_GAP + 64.0 + 1.0, "{d2:?} {d1:?}");
    assert!(dist(d2, t) > dist(d1, t), "{d2:?} {d1:?} {t:?}");
}

/// A follower far behind goes faster than it runs to catch up, more the farther it is,
/// up to half as fast again (`carry::catch_up`; the owner's playtest of 2026-10-24).
#[test]
fn a_follower_far_behind_catches_up_faster_than_it_runs() {
    use fnv_ai::carry::{catch_up, CATCH_UP_BOOST, CATCH_UP_SPAN, FOLLOW_RUN_BEHIND};
    assert_eq!(catch_up(100.0), 1.0);
    assert_eq!(catch_up(FOLLOW_RUN_BEHIND), 1.0);
    assert!((catch_up(FOLLOW_RUN_BEHIND + CATCH_UP_SPAN / 2.0) - (1.0 + CATCH_UP_BOOST / 2.0)).abs() < 1e-6);
    assert_eq!(catch_up(FOLLOW_RUN_BEHIND + CATCH_UP_SPAN), 1.0 + CATCH_UP_BOOST);
    assert_eq!(catch_up(f32::INFINITY), 1.0 + CATCH_UP_BOOST);
}

/// [design: fnv-game/src/game.rs, fnv-ai/src/carry.rs] A script's door activation
/// restarts an active walker before movement or social processing can write it back.
#[test]
fn a_walking_actor_sent_through_a_scripted_door_goes_on_at_the_destination() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    s.run(1.0);
    assert_eq!(s.ai.agents[&actor].walker.space, A);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(s.ai.agents[&actor].walker.follower.is_some());
    assert!(s.game.take_teleported().is_empty(), "walking is no discontinuous move");
    s.game.run_source("ScriptDoor.Activate TravellerRef", Default::default()).unwrap();
    assert_eq!(s.game.cell_of(actor), Some(FormId(CELL_B)));
    assert_eq!(s.game.world.peek(actor).unwrap().pos, SCRIPT_DEST);
    assert_eq!(s.ai.moved_by_scripts(&mut s.game), vec![(actor, true)]);
    let walker = &s.ai.agents[&actor].walker;
    assert_eq!((walker.space, walker.feet, walker.heading), (B, SCRIPT_DEST, 1.0));
    assert!(walker.follower.is_none(), "the source route was dropped");
    assert!(s.ai.moved_by_scripts(&mut s.game).is_empty(), "the move is consumed once");
    for _ in 0..20 {
        s.run(0.5);
        s.ai.social(&mut s.game, &s.nav, 0.5);
        assert_eq!(s.ai.agents[&actor].walker.space, B);
        assert_eq!(s.game.cell_of(actor), Some(FormId(CELL_B)));
        assert_eq!(s.game.world.peek(actor).unwrap().pos, s.ai.agents[&actor].walker.feet);
    }
}

#[test]
fn an_actor_sent_through_a_scripted_door_before_ai_ownership_starts_there() {
    let f = fixture();
    let mut s = sim(&f, &[]);
    let actor = FormId(R_TRAVEL);
    s.game.run_source("ScriptDoor.Activate TravellerRef", Default::default()).unwrap();
    assert_eq!(s.ai.moved_by_scripts(&mut s.game), vec![(actor, false)]);
    assert_eq!(s.game.cell_of(actor), Some(FormId(CELL_B)));
    s.ai.add(&s.game, actor).unwrap();
    assert_eq!((s.ai.agents[&actor].walker.space, s.feet(R_TRAVEL)), (B, SCRIPT_DEST));
    for _ in 0..20 {
        s.run(0.5);
        s.ai.social(&mut s.game, &s.nav, 0.5);
        assert_eq!(s.ai.agents[&actor].walker.space, B);
        assert_eq!(s.game.cell_of(actor), Some(FormId(CELL_B)));
        assert_eq!(s.game.world.peek(actor).unwrap().pos, s.feet(R_TRAVEL));
    }
    assert!(s.ai.moved_by_scripts(&mut s.game).is_empty());
}

/// The player's teammate is brought along when the player changes cells (SetPlayerTeammate,
/// "Changing cells will teleport the teammate to the player"; docs/design/followers.md), and
/// so is an actor following the player on a Follow package (owner decision 2026-10-23).
#[test]
fn a_teammate_following_the_player_is_brought_along_to_another_cell() {
    let f = fixture();
    let mut s = sim(&f, &[R_COMPANION]);
    let (mate, player) = (FormId(R_COMPANION), FormId::PLAYER_REF);
    {
        let p = s.game.world.get(player).unwrap();
        (p.cell, p.pos, p.rot) = (Some(FormId(CELL_A)), [100.0, 50.0, 0.0], [0.0; 3]);
    }
    s.run(1.0);
    assert!(s.game.follows_player(mate));
    let move_to = |s: &mut Sim, marker: u32| {
        let opcode = s.game.data.table.by_name("MoveTo").unwrap();
        let ctx = fnv_game::functions::Ctx { unit: "test".into(), opcode, on: Some(player), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        s.game.call_function(&ctx, &[fnv_script::vm::ArgValue::Value(fnv_script::vm::Value::Ref(marker))]);
    };
    // Not a teammate, but following the player on its Follow package from the player's
    // cell: it comes along at once too (owner decision 2026-10-23, Deputy Beagle).
    move_to(&mut s, MARK_B);
    assert_eq!(s.game.world.peek(mate).and_then(|m| m.cell), Some(FormId(CELL_B)));
    assert!(s.game.brought_along.contains(&mate));
    assert!(!s.game.is_teammate(mate));
    s.game.brought_along.clear();
    s.ai.moved_by_scripts(&mut s.game);
    s.run(1.0);
    assert!(s.game.follows_player(mate));
    // A teammate: put beside the player in cell B, and the AI takes it up from there.
    s.game.extra.entry(mate).or_default().teammate = true;
    move_to(&mut s, MARK_B);
    let st = s.game.world.peek(mate).unwrap();
    assert_eq!(st.cell, Some(FormId(CELL_B)));
    assert!((dist(st.pos, [5250.0, 5250.0, 50.0]) - fnv_game::followers::BESIDE).abs() < 1.0, "{:?}", st.pos);
    // On the player's left (the original's Boone through a door, od_03).
    let (pp, h) = s.game.world.peek(player).map(|p| (p.pos, p.rot[2])).unwrap();
    let left = [pp[0] - h.cos() * fnv_game::followers::BESIDE, pp[1] + h.sin() * fnv_game::followers::BESIDE];
    assert!((st.pos[0] - left[0]).hypot(st.pos[1] - left[1]) < 1.0, "{:?} not left of {:?} at {h}", st.pos, pp);
    assert!(s.game.brought_along.contains(&mate));
    s.ai.moved_by_scripts(&mut s.game);
    assert_eq!(s.ai.agents[&mate].walker.space, B);
    s.run(2.0);
    assert!(s.game.follows_player(mate));
    // On a package that does not follow (the original's Boone on his own PATROL), a
    // teammate still comes along (od_03, the `followers` campaign).
    s.game.extra.entry(mate).or_default().script_package = Some(FormId(P_DEFAULT));
    s.game.evaluate_package(mate);
    assert!(!s.game.follows_player(mate) && !s.game.told_to_wait(mate));
    move_to(&mut s, MARK_A);
    assert_eq!(s.game.world.peek(mate).and_then(|m| m.cell), s.game.world.peek(player).and_then(|p| p.cell));
    // Dead, it is no teammate any more and stays where it fell.
    s.game.kill(mate, None);
    assert!(!s.game.is_teammate(mate) && !s.game.extra[&mate].teammate);
}

/// The companion wheel's Back Up (docs/spec-notes/companion-wheel.md §5): the companion
/// walks about `BACK_UP` units straight away from the player and stands; its Follow package
/// does not call it back while it is within the follow distance.
#[test]
fn a_companion_told_to_back_up_steps_away_from_the_player() {
    let f = fixture();
    let mut s = sim(&f, &[R_COMPANION]);
    let (mate, player) = (FormId(R_COMPANION), FormId::PLAYER_REF);
    {
        let p = s.game.world.get(player).unwrap();
        (p.cell, p.pos, p.rot) = (Some(FormId(CELL_A)), [100.0, 50.0, 0.0], [0.0; 3]);
    }
    s.game.extra.entry(mate).or_default().teammate = true;
    s.run(20.0);
    assert!(s.game.follows_player(mate));
    let before = s.ai.agents[&mate].walker.feet;
    let ppos = [100.0, 50.0, 0.0];
    let away = |p: [f32; 3]| (p[0] - ppos[0]).hypot(p[1] - ppos[1]);
    s.game.companions.back_ups.insert(mate, s.game.time);
    s.run(6.0);
    let after = s.ai.agents[&mate].walker.feet;
    assert!(s.ai.events.iter().any(|e| e.actor == mate && matches!(e.event, AiEvent::BackUp { .. })), "backed up");
    assert!(away(after) > away(before) + 100.0, "{before:?} -> {after:?}");
    assert!(s.game.companions.back_ups.is_empty(), "the order was taken");
}

// ---- patrol waits, package furniture, sandboxes -------------------------------------------

#[test]
fn a_patrol_waits_out_a_points_idle_time() {
    let f = fixture();
    let mut s = sim(&f, &[R_PATROL]);
    s.run(60.0);
    let log: Vec<(f64, &AiEvent)> = s.ai.events.iter().filter(|e| e.actor == FormId(R_PATROL)).map(|e| (e.time, &e.event)).collect();
    let wait = log.iter().position(|(_, e)| matches!(e, AiEvent::PatrolWait { patrol_point, seconds } if patrol_point.0 == M2 && *seconds == 6.0)).expect("waits at M2");
    // The walk to M3 is planned six seconds after the wait began.
    let (t0, _) = log[wait];
    let (t1, next) = log[wait + 1];
    assert!(matches!(next, AiEvent::Trip { patrol_point: Some(p), .. } if p.0 == M3), "{next:?}");
    assert!((6.0..7.0).contains(&(t1 - t0)), "waited {}", t1 - t0);
    // M1 and M3 have no idle time: no wait there.
    assert_eq!(log.iter().filter(|(_, e)| matches!(e, AiEvent::PatrolWait { .. })).count(), 1);
    // On arriving at M2 the patroller says M2's topic (docs/spec-notes/packages.md §7.4).
    assert_eq!(s.game.world.placed.get(FormId(M2)).and_then(|p| p.patrol_topic), Some(FormId(PATROL_TOPIC)));
    let said = |k: &fnv_game::trace::EventKind| matches!(k, fnv_game::trace::EventKind::NoLine { speaker, topic } if speaker.0 == R_PATROL && topic.0 == PATROL_TOPIC);
    assert!(s.game.events.iter().any(|e| said(&e.kind)), "says M2's topic");
}

#[test]
fn a_package_whose_place_is_furniture_gives_it_to_the_actor() {
    let f = fixture();
    let mut s = sim(&f, &[R_TO_CHAIR, R_SLEEP_C]);
    s.run(30.0);
    // Travel to a chair: the actor uses it once there.
    let sitter = &s.ai.agents[&FormId(R_TO_CHAIR)];
    assert_eq!(sitter.activity, Activity::There);
    assert_eq!(sitter.using, Some(FormId(CHAIR_C)));
    assert!(matches!(s.events(R_TO_CHAIR).last().unwrap(), AiEvent::Furniture { furniture: Some(c) } if c.0 == CHAIR_C));
    // A Sleep package "in cell": the cell's bed, however far.
    assert_eq!(s.ai.agents[&FormId(R_SLEEP_C)].using, Some(FormId(BED_C)));
    // Nobody sandboxes under these packages.
    assert!(sitter.sandbox.is_none());
}

/// A script's Look turning the body keeps an actor from sitting down until StopLook; the
/// seat stays kept for it meanwhile [observed: RingoRef, campaign `knockdown_look`
/// 2026-10-19, docs/design/script-stubs.md §3].
#[test]
fn a_look_turning_the_body_keeps_an_actor_off_its_seat_until_stop_look() {
    use fnv_game::functions::senses::Look;
    let f = fixture();
    let mut s = sim(&f, &[R_TO_CHAIR]);
    s.game.senses.looks.insert(FormId(R_TO_CHAIR), Look { target: FormId(R_SLEEP_C), rotate_body: true });
    s.run(30.0);
    let sitter = &s.ai.agents[&FormId(R_TO_CHAIR)];
    assert_eq!(sitter.activity, Activity::There);
    assert_eq!(sitter.using, None, "held standing by the chair");
    assert!(!s.events(R_TO_CHAIR).iter().any(|e| matches!(e, AiEvent::Furniture { furniture: Some(_) })));
    // StopLook: it sits at the next social update.
    s.game.senses.looks.remove(&FormId(R_TO_CHAIR));
    s.game.tick();
    s.ai.social(&mut s.game, &s.nav, 0.5);
    assert_eq!(s.ai.agents[&FormId(R_TO_CHAIR)].using, Some(FormId(CHAIR_C)));
}

/// ResetAI on `actor`, as a script calls it.
fn reset_ai(s: &mut Sim, actor: FormId) {
    let opcode = s.game.data.table.by_name("ResetAI").unwrap();
    let ctx = fnv_game::functions::Ctx { unit: "test".into(), opcode, on: Some(actor), self_ref: Some(actor), action_ref: None, quest: None, inst: None, in_condition: false };
    s.game.call_function(&ctx, &[]);
}

/// ResetAI on a seated actor whose package is chosen again: the package is taken up
/// afresh (its furniture let go, begun again, the chair given anew), where before nothing
/// happened and it stayed seated [design, issue #2119; the original's seated RingoRef read
/// GET_UP one loop after ResetAI, observed: owner-decision-captures.md od_06]. A plain
/// evaluation choosing the same package changes nothing.
#[test]
fn reset_ai_takes_the_same_package_up_afresh_and_lets_the_chair_go() {
    let f = fixture();
    let mut s = sim(&f, &[R_TO_CHAIR]);
    let actor = FormId(R_TO_CHAIR);
    s.run(30.0);
    assert_eq!(s.ai.agents[&actor].using, Some(FormId(CHAIR_C)));
    assert_eq!(s.ai.agents[&actor].restarts, 0);
    // A plain evaluation, and the periodic choices, choose the same package: no restart.
    let before = s.events(R_TO_CHAIR).len();
    assert_eq!(s.game.evaluate_package(actor), Some(FormId(P_TO_CHAIR)));
    s.run(15.0);
    assert_eq!(s.events(R_TO_CHAIR).len(), before, "{:?}", &s.events(R_TO_CHAIR)[before..]);
    assert_eq!(s.ai.agents[&actor].restarts, 0);
    // ResetAI: the same package, begun again.
    reset_ai(&mut s, actor);
    assert_eq!(s.game.current_package(actor), Some(FormId(P_TO_CHAIR)));
    s.run(0.5);
    let ev: Vec<AiEvent> = s.events(R_TO_CHAIR)[before..].iter().copied().cloned().collect();
    assert!(matches!(ev[0], AiEvent::Package { package: Some(p) } if p.0 == P_TO_CHAIR), "{ev:?}");
    assert!(matches!(ev[1], AiEvent::Furniture { furniture: None }), "the chair let go: {ev:?}");
    assert!(ev[2..].iter().any(|e| matches!(e, AiEvent::AlreadyThere { package } if package.0 == P_TO_CHAIR)), "{ev:?}");
    assert!(matches!(ev.last().unwrap(), AiEvent::Furniture { furniture: Some(c) } if c.0 == CHAIR_C), "{ev:?}");
    let a = &s.ai.agents[&actor];
    assert_eq!((a.restarts, a.activity, a.using), (1, Activity::There, Some(FormId(CHAIR_C))));
    assert!(a.walker.follower.is_none());
    assert!(!s.game.extra[&actor].ai_reset, "the request is taken once");
    // Two calls before the AI's update are one restart; the next updates are quiet.
    reset_ai(&mut s, actor);
    reset_ai(&mut s, actor);
    s.run(5.0);
    assert_eq!(s.ai.agents[&actor].restarts, 2);
    let n = s.events(R_TO_CHAIR).iter().filter(|e| matches!(e, AiEvent::Package { .. })).count();
    s.run(15.0);
    assert_eq!(s.events(R_TO_CHAIR).iter().filter(|e| matches!(e, AiEvent::Package { .. })).count(), n);
}

/// ResetAI on an actor walking its package's way: the way is planned again from where it
/// stands. While its AI is off the request waits, and is carried out once it is on.
#[test]
fn reset_ai_plans_a_walk_again_and_waits_while_the_ai_is_off() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    s.run(2.0);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    let trips = |s: &Sim| s.events(R_TRAVEL).iter().filter(|e| matches!(e, AiEvent::Trip { .. })).count();
    assert_eq!(trips(&s), 1);
    s.game.extra.entry(actor).or_default().ai_off = true;
    reset_ai(&mut s, actor);
    s.run(1.0);
    assert_eq!((trips(&s), s.ai.agents[&actor].restarts), (1, 0), "nothing while its AI is off");
    s.game.extra.get_mut(&actor).unwrap().ai_off = false;
    s.run(0.5);
    assert_eq!((trips(&s), s.ai.agents[&actor].restarts), (2, 1), "{:?}", s.events(R_TRAVEL));
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
}

/// ResetAI, then a save before the AI's next update: the request is saved with the game
/// (`RefExtra::ai_reset`), so after the load the actor's first update takes the package up
/// afresh and the progress the save kept is forgotten: the patrol starts again at its
/// first point rather than resuming at the saved one (`fnv_ai::progress`;
/// docs/design/save-load.md §3.3).
#[test]
fn a_reset_saved_before_the_ai_took_it_is_carried_out_after_the_load() {
    let f = fixture();
    let mut s = sim(&f, &[R_PATROL]);
    let actor = FormId(R_PATROL);
    let point = |s: &Sim| s.ai.progress().iter().find(|p| p.actor == actor).and_then(|p| p.patrol_point);
    while point(&s).is_none_or(|p| p < 2) {
        s.run(0.5);
    }
    reset_ai(&mut s, actor);
    // Saved with the request pending: the game's state and the AI's progress, as text.
    let game_text = serde_json::to_string(&s.game.snapshot()).unwrap();
    let progress_text = serde_json::to_string(&s.ai.progress()).unwrap();
    assert!(game_text.contains("\"ai_reset\":true"), "the request is in the save");
    // Loaded: a new game and a new AI from the text.
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&game_text).unwrap();
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let game = Game::restore(&f.data, world, s.game.cfg, &back).unwrap();
    assert!(game.extra[&actor].ai_reset);
    let mut ai = Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() });
    ai.add(&game, actor).unwrap();
    let saved: Vec<fnv_ai::progress::Progress> = serde_json::from_str(&progress_text).unwrap();
    assert_eq!(saved.iter().find(|p| p.actor == actor).and_then(|p| p.patrol_point), Some(2));
    ai.restore(saved);
    let mut s = Sim { game, ai, nav: nav() };
    s.run(0.5);
    assert_eq!(s.ai.agents[&actor].restarts, 1);
    assert!(!s.game.extra[&actor].ai_reset, "taken by the first update");
    assert!(!s.events(R_PATROL).iter().any(|e| matches!(e, AiEvent::Resumed { .. })), "{:?}", s.events(R_PATROL));
    assert!(matches!(s.events(R_PATROL)[1], AiEvent::Trip { patrol_point: Some(p), .. } if p.0 == M1), "{:?}", s.events(R_PATROL));
    // Without the reset the same save resumes at the saved point.
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&game_text.replace("\"ai_reset\":true", "\"ai_reset\":false")).unwrap();
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let game = Game::restore(&f.data, world, s.game.cfg, &back).unwrap();
    let mut ai = Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() });
    ai.add(&game, actor).unwrap();
    ai.restore(serde_json::from_str(&progress_text).unwrap());
    let mut s = Sim { game, ai, nav: nav() };
    s.run(0.5);
    assert!(s.events(R_PATROL).iter().any(|e| matches!(e, AiEvent::Resumed { .. })), "{:?}", s.events(R_PATROL));
}

/// What a sandboxing actor did, in order.
fn doings(s: &Sim, actor: u32) -> Vec<Doing> {
    s.events(actor).iter().filter_map(|e| match e {
        AiEvent::Sandbox { doing, .. } => Some(*doing),
        _ => None,
    }).collect()
}

#[test]
fn a_sandbox_that_only_wanders_walks_about_inside_its_radius() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_WANDER]);
    let mut farthest = 0.0f32;
    for _ in 0..240 {
        s.run(1.0);
        farthest = farthest.max(dist(s.feet(R_SB_WANDER), MIDDLE_C));
    }
    let did = doings(&s, R_SB_WANDER);
    assert!(did.len() >= 6, "{did:?}");
    assert!(did.iter().all(|d| *d == Doing::Wandering), "{did:?}");
    let agent = &s.ai.agents[&FormId(R_SB_WANDER)];
    assert!(agent.walker.travelled > 500.0, "walked {}", agent.walker.travelled);
    assert!(farthest <= 450.0 + 1.0, "went {farthest} from the middle");
    assert_eq!(agent.using, None);
    let allowed = agent.sandbox.as_ref().unwrap().allowed;
    assert!(allowed.wandering && !allowed.furniture && !allowed.idle_markers && !allowed.eating && !allowed.sleeping && !allowed.conversation);
}

#[test]
fn a_sandbox_with_always_run_runs_between_its_spots() {
    // Freeside's chased rat: a Sandbox package with Always Run (#4487). Without the flag
    // the wanderer walks at its walk speed (100); with it, at its run (400).
    let f = fixture();
    let fastest = |s: &mut Sim| {
        let mut top = 0.0f32;
        for _ in 0..60 {
            s.run(1.0);
            let a = &s.ai.agents[&FormId(R_SB_WANDER)];
            if a.activity == Activity::Travelling {
                top = top.max(a.walker.speed);
            }
        }
        top
    };
    let mut walking = sim(&f, &[R_SB_WANDER]);
    let walk = fastest(&mut walking);
    assert!(walk > 0.0 && walk <= 100.0 + 1e-3, "walks at its walk: {walk}");
    let mut running = sim(&f, &[R_SB_WANDER]);
    running.game.run_query("Begin Function { }
SetPackageFlag SandboxWander 13 1
End
", Default::default()).unwrap();
    let run = fastest(&mut running);
    assert!((run - 400.0).abs() < 1.0, "runs at its run: {run}");
    assert!(doings(&running, R_SB_WANDER).iter().all(|d| *d == Doing::Wandering));
}

#[test]
fn a_sandbox_without_wandering_takes_turns_at_the_chair_and_the_idle_marker() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    s.run(600.0);
    let did = doings(&s, R_SB_OBJECTS);
    assert!(did.len() >= 4, "{did:?}");
    // Only the two kinds the package leaves on, never the same kind twice running.
    assert!(did.iter().all(|d| matches!(d, Doing::Sitting(c) if c.0 == CHAIR_C) || matches!(d, Doing::AtIdleMarker(m) if m.0 == IDLE_C)), "{did:?}");
    assert!(did.windows(2).all(|w| std::mem::discriminant(&w[0]) != std::mem::discriminant(&w[1])), "{did:?}");
    // The chair is taken while sat on and given back afterwards.
    let furniture: Vec<Option<u32>> = s.events(R_SB_OBJECTS).iter().filter_map(|e| match e {
        AiEvent::Furniture { furniture } => Some(furniture.map(|f| f.0)),
        _ => None,
    }).collect();
    assert!(furniture.len() >= 2 && furniture[0] == Some(CHAIR_C) && furniture[1].is_none(), "{furniture:?}");
    assert!(s.ai.agents[&FormId(R_SB_OBJECTS)].walker.travelled > 100.0);
}

#[test]
fn an_idle_marker_turns_the_actor_its_way() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    // Run until it stands at the marker.
    let mut at_marker = false;
    for _ in 0..600 {
        s.run(1.0);
        let a = &s.ai.agents[&FormId(R_SB_OBJECTS)];
        if a.activity == Activity::There && matches!(a.sandbox.as_ref().map(|x| x.doing), Some(Doing::AtIdleMarker(_))) {
            assert!(dist(a.walker.feet, [10500.0, 10250.0, 0.0]) < 40.0, "{:?}", a.walker.feet);
            assert_eq!(a.using, None);
            // It turns to the marker's heading on the spot (not at once), within the
            // time a half turn takes at the standing rate.
            assert!(a.face_to == Some(1.25) || (a.walker.heading - 1.25).abs() < 1e-4, "{:?} {}", a.face_to, a.walker.heading);
            s.run(2.0);
            let a = &s.ai.agents[&FormId(R_SB_OBJECTS)];
            assert!((a.walker.heading - 1.25).abs() < 1e-4 && a.face_to.is_none(), "heading {} {:?}", a.walker.heading, a.face_to);
            at_marker = true;
            break;
        }
    }
    assert!(at_marker);
}

#[test]
fn two_sandboxers_do_not_share_a_chair() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS, R_SB_OBJECTS_2]);
    let mut both_busy = 0;
    for _ in 0..600 {
        s.run(1.0);
        let (a, b) = (&s.ai.agents[&FormId(R_SB_OBJECTS)], &s.ai.agents[&FormId(R_SB_OBJECTS_2)]);
        assert!(!(a.using.is_some() && a.using == b.using), "both on {:?}", a.using);
        both_busy += (a.using.is_some() || b.using.is_some()) as u32;
    }
    assert!(both_busy > 60, "the chair was used for {both_busy} s");
    assert!(!doings(&s, R_SB_OBJECTS).is_empty() && !doings(&s, R_SB_OBJECTS_2).is_empty());
}

#[test]
fn a_sandbox_with_everything_off_stands_still() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(120.0);
    let did = doings(&s, R_SB_NOTHING);
    assert!(!did.is_empty() && did.iter().all(|d| *d == Doing::Standing), "{did:?}");
    assert_eq!(s.ai.agents[&FormId(R_SB_NOTHING)].walker.travelled, 0.0);
}

#[test]
fn a_sandboxer_walking_up_to_a_conversation_partner_walks_the_whole_way() {
    // Until 2026-10-17 the walk ended in its first frame: the starter stood inside its
    // sandbox's radius, which counted as arriving, and it talked from where it stood.
    let f = fixture();
    let mut s = sim(&f, &[R_SB_NOTHING, R_SB_WANDER]);
    s.run(5.0);
    let (a, to) = (FormId(R_SB_NOTHING), [10750.0, 10450.0, 0.0]);
    let agent = s.ai.agents.get_mut(&a).unwrap();
    agent.walker.go_to(&s.nav, fnv_nav::path::Place { space: C, pos: to }, &fnv_nav::path::PathParams::default()).unwrap();
    agent.activity = Activity::Travelling;
    agent.chat = Some(fnv_ai::Chat::Going { partner: FormId(R_SB_WANDER), since: 0.0 });
    s.run(1.0);
    assert_eq!(s.ai.agents[&a].activity, Activity::Travelling, "the walk goes on inside the sandbox's radius");
    s.run(6.0);
    assert_eq!(s.ai.agents[&a].activity, Activity::There);
    assert!(dist(s.feet(R_SB_NOTHING), to) < 20.0, "{:?}", s.feet(R_SB_NOTHING));
}

/// After a load a sandboxer keeps the day's sleep and meal times it had and rolls the
/// next days as it would have (`fnv_ai::progress`); started afresh it rolls them anew.
#[test]
fn a_loaded_sandbox_keeps_its_days_times() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_ALL]);
    let actor = FormId(R_SB_ALL);
    s.game.world.set_global(FormId(G_HOUR), 2.0).unwrap();
    s.run(30.0);
    // A day later: the times rolled again, from numbers further on.
    s.game.world.set_global(FormId(G_DAYS), 6.1).unwrap();
    s.run(20.0);
    let times = s.ai.agents[&actor].sandbox.as_ref().unwrap().times();
    let json = serde_json::to_string(&s.ai.progress()).unwrap();
    let fresh = |s: &mut Sim, restore: Option<&str>| {
        let mut ai = Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() });
        ai.add(&s.game, actor).unwrap();
        if let Some(j) = restore {
            ai.restore(serde_json::from_str(j).unwrap());
        }
        s.ai = ai;
        s.run(30.0);
        s.ai.agents[&actor].sandbox.as_ref().unwrap().times()
    };
    assert_eq!(fresh(&mut s, Some(&json)), times);
    assert_ne!(fresh(&mut s, None), times, "a fresh sandbox rolls the day from its first numbers");
}

#[test]
fn a_sandboxer_sleeps_at_night_and_eats_at_mealtimes() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_ALL]);
    // Two in the morning is inside every sleep window (start 19:00 to 01:00, six hours
    // or more): to the bed.
    s.game.world.set_global(FormId(G_HOUR), 2.0).unwrap();
    s.run(30.0);
    let actor = FormId(R_SB_ALL);
    let a = &s.ai.agents[&actor];
    assert_eq!(a.sandbox.as_ref().unwrap().doing, Doing::Sleeping(FormId(BED_C)));
    assert_eq!(a.using, Some(FormId(BED_C)));
    let ((sleep_start, sleep_hours), meals) = a.sandbox.as_ref().unwrap().times();
    assert!((19.0..24.0).contains(&sleep_start) || (0.0..1.0).contains(&sleep_start), "sleep starts {sleep_start}");
    assert!((6.0..=10.0).contains(&sleep_hours));
    assert!((6.0..=9.0).contains(&meals[0].0) && (11.0..=14.0).contains(&meals[1].0) && (17.0..=20.0).contains(&meals[2].0), "{meals:?}");
    assert!(meals.iter().all(|m| (0.5..=2.0).contains(&m.1)));
    // It stays in bed while the night lasts, whatever time passes on the wall.
    s.run(300.0);
    assert_eq!(s.ai.agents[&actor].using, Some(FormId(BED_C)));
    // The middle of the afternoon, between lunch and dinner (15:30 to 16:59 is in
    // neither window): up, and doing the things of the day.
    s.game.world.set_global(FormId(G_HOUR), 16.5).unwrap();
    s.game.world.set_global(FormId(G_DAYS), 5.7).unwrap();
    s.run(20.0);
    let a = &s.ai.agents[&actor];
    assert_ne!(a.using, Some(FormId(BED_C)));
    assert!(!matches!(a.sandbox.as_ref().unwrap().doing, Doing::Sleeping(_) | Doing::Eating(_)), "{:?}", a.sandbox.as_ref().unwrap().doing);
    // Lunch, at the hour rolled for it: a meal on the chair.
    let lunch = meals[1].0 + 0.1;
    s.game.world.set_global(FormId(G_HOUR), lunch).unwrap();
    // Whatever it is doing ends within three minutes.
    s.run(240.0);
    assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().doing, Doing::Eating(Some(FormId(CHAIR_C))));
    assert_eq!(s.ai.agents[&actor].using, Some(FormId(CHAIR_C)));
    // With the chair free, no choice was a meal window's without a seat.
    assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().seatless_meal_choices(), 0);
}

/// Run `seconds` of a meal window in which `actor` has no seat to eat on, checking each
/// second that it is not eating (#2011); the choices it made meanwhile. Each of them is
/// counted as a choice in a meal window with no free seat (`seatless_meal_choices`, what
/// the engine's report and fnvtool ai run show).
fn through_a_seatless_meal(s: &mut Sim, actor: u32, seconds: usize) -> Vec<Doing> {
    let before = doings(s, actor).len();
    let counted = s.ai.agents[&FormId(actor)].sandbox.as_ref().map_or(0, |sb| sb.seatless_meal_choices());
    for _ in 0..seconds {
        s.run(1.0);
        let sb = s.ai.agents[&FormId(actor)].sandbox.as_ref().unwrap();
        assert!(!matches!(sb.doing, Doing::Eating(_)) && sb.phase() != fnv_game::ai::SandboxPhase::Eating, "{:?} at {}", sb.doing, s.game.time);
    }
    let did = doings(s, actor)[before..].to_vec();
    let counted = s.ai.agents[&FormId(actor)].sandbox.as_ref().unwrap().seatless_meal_choices() - counted;
    assert!(counted > 0 && counted as usize >= did.len(), "{counted} counted for {did:?}");
    did
}

/// With furniture off there is no seat: the meal window passes in wandering and idle
/// markers, not standing on the spot [design, #2011].
#[test]
fn a_sandboxer_without_furniture_goes_on_through_its_meal() {
    let mut f = fixture();
    f.data.package_defs.get_mut(&FormId(P_SB_ALL)).unwrap().type_flags |= type_flags::NO_FURNITURE;
    // The day's lunch, as this sandbox rolls it (the same numbers every run); the run
    // itself starts inside it.
    let mut probe = sim(&f, &[R_SB_ALL]);
    set_clock(&mut probe, 16.5, 5.7);
    probe.run(0.5);
    let at = lunch(&probe, R_SB_ALL);
    let mut s = sim(&f, &[R_SB_ALL]);
    set_clock(&mut s, at, 5.7);
    let did = through_a_seatless_meal(&mut s, R_SB_ALL, 300);
    assert!(did.len() >= 3, "{did:?}");
    assert!(did.iter().all(|d| matches!(d, Doing::Wandering | Doing::AtIdleMarker(_))), "{did:?}");
    // Both kinds (the one marker is not chosen twice running, so mostly walks).
    assert!(did.iter().any(|d| matches!(d, Doing::Wandering)) && did.contains(&Doing::AtIdleMarker(FormId(IDLE_C))), "{did:?}");
}

/// The only seat kept for another actor: no meal while it is (the day's choices go on);
/// once it is free, the meal on it [design, #2011].
#[test]
fn a_sandboxer_eats_only_once_a_seat_is_free() {
    let f = fixture();
    let mut s = sim(&f, &[R_TO_CHAIR]);
    set_clock(&mut s, 16.5, 5.7);
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_TO_CHAIR)].using, Some(FormId(CHAIR_C)));
    s.ai.add(&s.game, FormId(R_SB_ALL)).unwrap();
    s.run(20.0);
    let at = lunch(&s, R_SB_ALL);
    set_clock(&mut s, at, 5.7);
    let did = through_a_seatless_meal(&mut s, R_SB_ALL, 300);
    assert!(did.len() >= 3, "{did:?}");
    assert!(did.iter().any(|d| matches!(d, Doing::Wandering | Doing::AtIdleMarker(_))), "{did:?}");
    assert!(!did.contains(&Doing::Sitting(FormId(CHAIR_C))), "{did:?}");
    assert_eq!(s.ai.agents[&FormId(R_TO_CHAIR)].using, Some(FormId(CHAIR_C)));
    // The chair is let go: within the meal window, the meal on it.
    s.ai.root(&s.game, FormId(R_TO_CHAIR));
    assert_eq!(s.ai.furniture_of(FormId(R_TO_CHAIR)), None);
    let actor = FormId(R_SB_ALL);
    for _ in 0..240 {
        if s.ai.agents[&actor].using.is_some() {
            break;
        }
        s.run(1.0);
    }
    assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().doing, Doing::Eating(Some(FormId(CHAIR_C))), "{:?}", s.events(R_SB_ALL));
    assert_eq!(s.ai.agents[&actor].using, Some(FormId(CHAIR_C)));
}

#[test]
fn a_sandbox_is_the_same_every_time() {
    let f = fixture();
    let run = || {
        let mut s = sim(&f, &[R_SB_ALL]);
        s.run(300.0);
        (doings(&s, R_SB_ALL), s.feet(R_SB_ALL))
    };
    let (a, b) = (run(), run());
    assert!(a.0.len() >= 3, "{:?}", a.0);
    assert_eq!(a, b);
}

#[test]
fn a_standing_actor_is_pushed_over_the_navmesh_and_goes_back() {
    let f = fixture();
    let mut s = sim(&f, &[R_RADIUS, R_TO_CHAIR]);
    let actor = FormId(R_RADIUS);
    // On its way: not pushed.
    s.run(1.0);
    assert!(!s.ai.stands(&s.game, actor));
    assert_eq!(s.ai.push(&mut s.game, &s.nav, actor, [5.0, 0.0]), 0.0);
    s.run(30.0);
    // There (within 150 of the marker at (350, 350)) and standing: a push moves it, and
    // the game's reference with it.
    let at = s.feet(R_RADIUS);
    assert!(s.ai.stands(&s.game, actor));
    let moved = s.ai.push(&mut s.game, &s.nav, actor, [6.0, 8.0]);
    assert!((moved - 10.0).abs() < 1e-3, "{moved}");
    let pushed = s.feet(R_RADIUS);
    assert_eq!(pushed, [at[0] + 6.0, at[1] + 8.0, at[2]]);
    assert_eq!(s.game.world.get(actor).unwrap().pos, pushed);
    // Still where its package has it: it stays there.
    s.run(10.0);
    assert_eq!(s.feet(R_RADIUS), pushed);
    assert!(!s.events(R_RADIUS).iter().any(|e| matches!(e, AiEvent::PushedBack { .. })));
    // Pushed at the navmesh's edge (y 0): as far as the edge, no further, and along it
    // when the push points off it at an angle.
    let x = s.feet(R_RADIUS)[0];
    let moved = s.ai.push(&mut s.game, &s.nav, actor, [0.0, -600.0]);
    assert!((moved - (at[1] + 8.0)).abs() < 1e-2 && s.feet(R_RADIUS)[1].abs() < 1e-3, "{moved} {:?}", s.feet(R_RADIUS));
    assert_eq!(s.ai.push(&mut s.game, &s.nav, actor, [0.0, -10.0]), 0.0);
    let moved = s.ai.push(&mut s.game, &s.nav, actor, [10.0, -10.0]);
    assert!((moved - 10.0).abs() < 1e-2 && (s.feet(R_RADIUS)[0] - (x + 10.0)).abs() < 1e-2, "{moved} {:?}", s.feet(R_RADIUS));
    // Out of its package's place now. Left alone for a while it walks back into it.
    s.run(1.0);
    assert!(s.feet(R_RADIUS)[1].abs() < 1e-3);
    s.run(40.0);
    let events = s.events(R_RADIUS);
    let back = events.iter().position(|e| matches!(e, AiEvent::PushedBack { .. })).expect("goes back");
    assert!(matches!(events[back + 1], AiEvent::Trip { .. }) && matches!(events.last().unwrap(), AiEvent::Arrived { .. }), "{events:?}");
    assert!(dist(s.feet(R_RADIUS), [350.0, 350.0, 0.0]) <= 150.0);
    assert_eq!(s.ai.agents[&actor].activity, Activity::There);
    // Who sits is not pushed.
    assert_eq!(s.ai.agents[&FormId(R_TO_CHAIR)].using, Some(FormId(CHAIR_C)));
    assert!(!s.ai.stands(&s.game, FormId(R_TO_CHAIR)));
    assert_eq!(s.ai.push(&mut s.game, &s.nav, FormId(R_TO_CHAIR), [5.0, 0.0]), 0.0);
}

// ---- combat states (fnv_ai::alert) ----------------------------------------------------

use fnv_game::awareness::{Cause, State, Timers};
use fnv_game::detection::{Band, Detection};

/// Put `actor` in Alert, about something of the player's at `at`.
fn alert(s: &mut Sim, actor: u32, at: [f32; 3]) {
    let (now, t) = (s.game.time, Timers::default());
    let a = s.game.awareness.actors.entry(FormId(actor)).or_default();
    a.alarm(FormId::PLAYER_REF, at, -10.0, Cause::Noticed, now, &t);
    a.settle(FormId(actor), now, &t, true);
    assert_eq!(a.state, State::Alert);
}

#[test]
fn an_alerted_actor_searches_where_the_alarm_is_and_goes_back_after() {
    let f = fixture();
    let mut s = sim(&f, &[R_RADIUS]);
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_RADIUS)].activity, Activity::There);
    let stood = s.feet(R_RADIUS);
    alert(&mut s, R_RADIUS, [150.0, 100.0, 0.0]);
    s.run(1.0);
    assert!(s.events(R_RADIUS).iter().any(|e| matches!(e, AiEvent::Search { about, .. } if *about == FormId::PLAYER_REF)));
    s.run(8.0);
    // It looked about midway between where it stood and the hostile it only noticed
    // [documented: GECK wiki Combat Search; until 2026-10-12 at the alarm itself], then
    // searched on around there.
    let looked = s.events(R_RADIUS).into_iter().find_map(|e| match e {
        AiEvent::SearchLook { at, .. } => Some(*at),
        _ => None,
    });
    let midway = [(stood[0] + 150.0) / 2.0, (stood[1] + 100.0) / 2.0, 0.0];
    assert!(looked.is_some_and(|at| dist(at, midway) < 40.0), "{looked:?} {midway:?}");
    assert!(s.events(R_RADIUS).iter().filter(|e| matches!(e, AiEvent::Search { .. })).count() >= 2);
    // Back to Normal: the search ends and the actor goes back to its package's place.
    s.game.awareness.actors.remove(&FormId(R_RADIUS));
    s.run(30.0);
    let ev = s.events(R_RADIUS);
    let over = ev.iter().position(|e| matches!(e, AiEvent::SearchOver { .. })).expect("search over");
    assert!(matches!(ev[over + 1], AiEvent::SearchBack { .. }), "{ev:?}");
    assert!(dist(s.feet(R_RADIUS), [350.0, 350.0, 0.0]) <= 150.0);
    assert!(s.ai.agents[&FormId(R_RADIUS)].search.is_none());
}

#[test]
fn a_lost_search_walks_two_complete_shares_before_returning_to_normal() {
    // [design, #2010] Synthetic navmesh and alarms exercise the walk/look completion
    // path through Game::searched_once, rather than expiring an ordinary alarm.
    use fnv_game::awareness::Target;
    use fnv_game::combat_group::{GroupNote, GroupSettings};
    let f = fixture();
    let mut s = sim(&f, &[R_RADIUS]);
    s.run(30.0);
    let (actor, player, now) = (FormId(R_RADIUS), FormId::PLAYER_REF, s.game.time);
    s.game.awareness.tactics.settings = Some(GroupSettings { remove_search_count: 2, search_interior: [80.0; 3], ..Default::default() });
    let a = s.game.awareness.actors.entry(actor).or_default();
    a.state = State::Lost;
    a.since = now;
    a.targets.insert(player, Target { band: Band::Hidden, value: -50.0, at: [150.0, 100.0, 0.0], detected: Some(now - 15.0), hidden_since: Some(now) });
    a.alarm(player, [150.0, 100.0, 0.0], -50.0, Cause::LostTarget, now, &Timers::default());
    for _ in 0..240 {
        s.run(0.5);
        s.game.settle_awareness();
        if s.game.awareness.state(actor) == State::Normal {
            break;
        }
    }
    assert_eq!(s.game.awareness.state(actor), State::Normal);
    let counts: Vec<_> = s.game.awareness.tactics.notes.iter().filter_map(|n| match n {
        GroupNote::Searched { actor: who, count, needed, .. } if *who == actor => Some((*count, *needed)),
        _ => None,
    }).collect();
    assert_eq!(counts, vec![(1, 2), (2, 2)]);
    assert!(s.events(R_RADIUS).iter().filter(|e| matches!(e, AiEvent::SearchLook { .. })).count() >= 6);
    s.run(0.5);
    assert!(s.ai.agents[&actor].search.is_none());
}

#[test]
fn nearby_lost_targets_each_need_two_complete_searches() {
    // [design, #2010] Nearby synthetic alarms must not share a completed look or
    // point count when finishing one target selects the other.
    use fnv_game::awareness::Target;
    use fnv_game::combat_group::{GroupNote, GroupSettings};
    let f = fixture();
    let mut s = sim(&f, &[R_RADIUS]);
    s.run(30.0);
    let (actor, player, other, now) = (FormId(R_RADIUS), FormId::PLAYER_REF, FormId(R_HERE), s.game.time);
    s.game.awareness.tactics.settings = Some(GroupSettings { remove_search_count: 2, search_interior: [80.0; 3], ..Default::default() });
    let a = s.game.awareness.actors.entry(actor).or_default();
    a.state = State::Lost;
    a.since = now;
    for (target, at, refreshed) in [(other, [160.0, 100.0, 0.0], now - 0.5), (player, [150.0, 100.0, 0.0], now)] {
        a.targets.insert(target, Target { band: Band::Hidden, value: -50.0, at, detected: Some(now - 15.0), hidden_since: Some(now) });
        a.alarm(target, at, -50.0, Cause::LostTarget, refreshed, &Timers::default());
    }
    let mut completed = Vec::new();
    let mut notes_at = s.game.awareness.tactics.notes.len();
    for _ in 0..480 {
        s.run(0.5);
        let looks = s.events(R_RADIUS).iter().filter(|e| matches!(e, AiEvent::SearchLook { .. })).count();
        for note in &s.game.awareness.tactics.notes[notes_at..] {
            if let GroupNote::Searched { actor: who, about, count, needed } = note {
                if *who == actor {
                    assert_eq!(*needed, 2);
                    completed.push((*about, *count, looks));
                }
            }
        }
        notes_at = s.game.awareness.tactics.notes.len();
        s.game.settle_awareness();
        if s.game.awareness.state(actor) == State::Normal {
            break;
        }
    }
    assert_eq!(completed, vec![(player, 1, 3), (player, 2, 6), (other, 1, 9), (other, 2, 12)]);
    assert_eq!(s.game.awareness.state(actor), State::Normal);
    s.run(0.5);
    let events = s.events(R_RADIUS);
    let over = events.iter().position(|e| matches!(e, AiEvent::SearchOver { .. })).expect("search ends");
    assert!(matches!(events[over + 1], AiEvent::SearchBack { .. }), "{events:?}");
    assert!(s.ai.agents[&actor].search.is_none());
}

#[test]
fn switching_nearby_lost_targets_restarts_the_walk_and_look_progress() {
    // [design, #2010] A newly selected owner starts fresh even within RECENTRE,
    // whether the previous owner's third point is being walked to or looked at.
    use fnv_ai::alert::Search;
    use fnv_game::awareness::Target;
    use fnv_game::combat_group::GroupSettings;
    for looking in [false, true] {
        let f = fixture();
        let mut s = sim(&f, &[R_RADIUS]);
        s.run(30.0);
        let (actor, player, other, now) = (FormId(R_RADIUS), FormId::PLAYER_REF, FormId(R_HERE), s.game.time);
        s.game.awareness.tactics.settings = Some(GroupSettings { remove_search_count: 2, search_interior: [80.0; 3], ..Default::default() });
        let a = s.game.awareness.actors.entry(actor).or_default();
        a.state = State::Lost;
        a.since = now;
        for (target, at, refreshed) in [(other, [160.0, 100.0, 0.0], now - 0.5), (player, [150.0, 100.0, 0.0], now)] {
            a.targets.insert(target, Target { band: Band::Hidden, value: -50.0, at, detected: Some(now - 15.0), hidden_since: Some(now) });
            a.alarm(target, at, -50.0, Cause::LostTarget, refreshed, &Timers::default());
        }
        let at_third_point = |s: &Sim<'_>| match s.ai.agents[&actor].search {
            Some(Search::Going { points: 2, .. }) => !looking,
            Some(Search::Looking { points: 2, .. }) => looking,
            _ => false,
        };
        for _ in 0..120 {
            s.run(0.5);
            if at_third_point(&s) {
                break;
            }
        }
        assert!(at_third_point(&s), "looking={looking}");
        let before = s.events(R_RADIUS).iter().filter(|e| matches!(e, AiEvent::SearchLook { .. })).count();
        s.game.awareness.actors.get_mut(&actor).unwrap().alarms.iter_mut().find(|a| a.owner == other).unwrap().refreshed = s.game.time;
        for _ in 0..120 {
            s.run(0.5);
            if s.game.searches_done(actor, other) > 0 {
                break;
            }
        }
        assert_eq!(s.game.searches_done(actor, other), 1, "looking={looking}");
        assert_eq!(s.game.searches_done(actor, player), 0);
        let after = s.events(R_RADIUS).iter().filter(|e| matches!(e, AiEvent::SearchLook { .. })).count();
        assert_eq!(after - before, 3, "the replacement target needs three fresh looks; looking={looking}");
    }
}

/// An alert takes an actor off its bed or chair; when the search ends inside its
/// package's place it settles again (issue #2013) [observed: Easy Pete, fought off his
/// chair, sat again about 5 real seconds after the fight, AI observer 2026-11-10].
#[test]
fn an_actor_taken_off_its_furniture_by_an_alert_settles_again() {
    let f = fixture();
    let mut s = sim(&f, &[R_TO_CHAIR, R_SLEEP_C]);
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_SLEEP_C)].using, Some(FormId(BED_C)));
    assert_eq!(s.ai.agents[&FormId(R_TO_CHAIR)].using, Some(FormId(CHAIR_C)));
    for r in [R_SLEEP_C, R_TO_CHAIR] {
        let at = s.feet(r);
        alert(&mut s, r, [at[0] + 40.0, at[1], 0.0]);
    }
    s.run(2.0);
    assert_eq!(s.ai.agents[&FormId(R_SLEEP_C)].using, None, "the alert gets it up");
    assert_eq!(s.ai.agents[&FormId(R_TO_CHAIR)].using, None, "the alert gets it up");
    for r in [R_SLEEP_C, R_TO_CHAIR] {
        s.game.awareness.actors.remove(&FormId(r));
    }
    s.run(30.0);
    // The Sleep package "in cell": anywhere in the cell is its place, so nothing to walk
    // back to; it goes back to its bed.
    let ev = s.events(R_SLEEP_C);
    let over = ev.iter().rposition(|e| matches!(e, AiEvent::SearchOver { .. })).expect("search over");
    assert!(ev[over..].iter().any(|e| matches!(e, AiEvent::Resettles { .. })), "{ev:?}");
    assert_eq!(s.ai.agents[&FormId(R_SLEEP_C)].using, Some(FormId(BED_C)), "{ev:?}");
    // The chair's sitter sits again, walking back first if the search took it out of
    // its package's place.
    assert_eq!(s.ai.agents[&FormId(R_TO_CHAIR)].using, Some(FormId(CHAIR_C)), "{:?}", s.events(R_TO_CHAIR));
}

/// A patroller whose walk to a point an alert cut off walks on along its route once the
/// search ends (issue #2137: Nipton's patroller stood 118 s after a fight) [documented:
/// "Actor moves along patrol route continuously until another package evaluates", GECK
/// wiki Patrol Package].
#[test]
fn a_patroller_whose_walk_an_alert_cut_off_walks_on_after_the_search() {
    let f = fixture();
    let mut s = sim(&f, &[R_LOOP]);
    s.run(3.0);
    assert_eq!(s.ai.agents[&FormId(R_LOOP)].activity, Activity::Travelling);
    let at = s.feet(R_LOOP);
    alert(&mut s, R_LOOP, [at[0] + 40.0, at[1], 0.0]);
    s.run(2.0);
    s.game.awareness.actors.remove(&FormId(R_LOOP));
    s.run(45.0);
    let ev = s.events(R_LOOP);
    let over = ev.iter().rposition(|e| matches!(e, AiEvent::SearchOver { .. })).expect("search over");
    let points: Vec<u32> = ev[over..].iter().filter_map(|e| match e {
        AiEvent::Arrived { patrol_point: Some(p), .. } => Some(p.0),
        _ => None,
    }).collect();
    assert!(points.len() >= 2, "it goes round again: {ev:?}");
    assert!(matches!(ev[over + 1], AiEvent::SearchBack { .. }), "{ev:?}");
}

/// An actor without packages (Ranger Ghost, playtest list 5 item 29) walks back to where
/// it was placed after a search.
#[test]
fn an_actor_without_packages_goes_back_to_where_it_was_placed() {
    let f = fixture();
    let mut s = sim(&f, &[R_LONER]);
    s.run(5.0);
    assert_eq!(s.feet(R_LONER), [350.0, 50.0, 0.0]);
    alert(&mut s, R_LONER, [350.0, 400.0, 0.0]);
    s.run(9.0);
    assert!(dist(s.feet(R_LONER), [350.0, 50.0, 0.0]) > 60.0, "{:?}", s.feet(R_LONER));
    s.game.awareness.actors.remove(&FormId(R_LONER));
    s.run(30.0);
    let ev = s.events(R_LONER);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::BackHome { .. })), "{ev:?}");
    assert!(dist(s.feet(R_LONER), [350.0, 50.0, 0.0]) <= 40.0, "{:?}", s.feet(R_LONER));
    assert_eq!(s.ai.agents[&FormId(R_LONER)].activity, Activity::There);
}

#[test]
fn continue_during_combat_keeps_the_package_going() {
    let f = fixture();
    let mut s = sim(&f, &[R_STEADY, R_TRAVEL]);
    s.run(1.0);
    for a in [R_STEADY, R_TRAVEL] {
        assert_eq!(s.ai.agents[&FormId(a)].activity, Activity::Travelling);
        alert(&mut s, a, [100.0, 100.0, 0.0]);
    }
    s.run(30.0);
    // The courier's package continues during combat while it is on its way: it walks on
    // to cell B without searching; there the flag no longer holds it (alert.md §5.2).
    let steady = s.events(R_STEADY);
    let arrived = steady.iter().position(|e| matches!(e, AiEvent::Arrived { .. })).unwrap_or_else(|| panic!("{steady:?}"));
    assert!(!steady[..arrived].iter().any(|e| matches!(e, AiEvent::Search { .. } | AiEvent::Hold { .. })), "{steady:?}");
    // The traveller's does not: it breaks off to search and never gets there.
    let travel = s.events(R_TRAVEL);
    assert!(travel.iter().any(|e| matches!(e, AiEvent::Search { .. })), "{travel:?}");
    assert!(!travel.iter().any(|e| matches!(e, AiEvent::Arrived { .. })), "{travel:?}");
}

#[test]
fn an_actor_warning_the_player_stops_faces_it_then_walks_on() {
    // `fnv_game::aggro` "The look" / `crate::alert` Warnings: in Normal, warning the
    // player, a walker stops and turns to the player; once the warning is over it goes
    // back to its package.
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    s.run(1.0);
    assert_eq!(s.ai.agents[&FormId(R_TRAVEL)].activity, Activity::Travelling);
    let (me, player) = (FormId(R_TRAVEL), FormId::PLAYER_REF);
    let at = s.feet(R_TRAVEL);
    let cell = s.game.world.peek(me).and_then(|m| m.cell);
    {
        let p = s.game.world.get(player).unwrap();
        (p.cell, p.pos) = (cell, [at[0] + 300.0, at[1], at[2]]);
    }
    s.game.provocations.aggro_inside.insert(me);
    assert!(s.game.draws_weapon(me));
    s.run(3.0);
    assert!(s.events(R_TRAVEL).iter().any(|e| matches!(e, AiEvent::Warn { target, .. } if *target == player)), "{:?}", s.events(R_TRAVEL));
    let a = &s.ai.agents[&me];
    assert_eq!(a.activity, Activity::There);
    assert!(dist(s.feet(R_TRAVEL), at) < 1.0, "it stands");
    // Facing the player (east of it: heading 0 or π/2 by the walker's convention, so
    // measured as the heading towards the player).
    let want = fnv_nav::follow::heading_of([300.0, 0.0]);
    assert!(fnv_nav::follow::heading_delta(a.walker.heading, want).abs() < 0.05, "{} vs {want}", a.walker.heading);
    // The warning is over: back to its package.
    s.game.provocations.aggro_inside.clear();
    s.run(2.0);
    let ev = s.events(R_TRAVEL);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::SearchOver { why } if why.contains("warning"))), "{ev:?}");
    assert_eq!(s.ai.agents[&me].activity, Activity::Travelling);
}

#[test]
fn an_actor_in_combat_stops_and_faces_its_target() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    s.run(1.0);
    let (now, t) = (s.game.time, Timers::default());
    let a = s.game.awareness.actors.entry(FormId(R_TRAVEL)).or_default();
    a.observe(FormId::PLAYER_REF, &Detection { value: 5.0, band: Band::Detected, ..Default::default() }, [50.0, 300.0, 0.0], now, &t);
    a.settle(FormId(R_TRAVEL), now, &t, true);
    let at = s.feet(R_TRAVEL);
    s.run(3.0);
    assert!(s.events(R_TRAVEL).iter().any(|e| matches!(e, AiEvent::Hold { target, .. } if *target == FormId::PLAYER_REF)));
    assert_eq!(s.ai.agents[&FormId(R_TRAVEL)].activity, Activity::There);
    assert!(dist(s.feet(R_TRAVEL), at) < 1.0, "it stands");
}

#[test]
fn an_actor_going_into_combat_stands_a_moment_then_turns_at_the_combat_rate() {
    // `crate::alert::COMBAT_REACTION_SECONDS` and `combat_turn_rate` (#1352, capture
    // o25_turn): it stands on its heading for 1.1 s, then turns 275°/s with its target
    // more than a quarter turn off and 225°/s within it.
    use fnv_ai::alert::{combat_turn_rate, COMBAT_TURN_NEAR, COMBAT_TURN_WIDE};
    assert_eq!(combat_turn_rate(0.2), COMBAT_TURN_NEAR);
    assert_eq!(combat_turn_rate(-1.5), COMBAT_TURN_NEAR);
    assert_eq!(combat_turn_rate(2.0), COMBAT_TURN_WIDE);
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    s.run(1.0);
    assert_eq!(s.ai.agents[&FormId(R_TRAVEL)].activity, Activity::Travelling);
    let (now, t) = (s.game.time, Timers::default());
    let at = s.feet(R_TRAVEL);
    let target = [at[0] + 300.0, at[1], at[2]];
    let a = s.game.awareness.actors.entry(FormId(R_TRAVEL)).or_default();
    a.observe(FormId::PLAYER_REF, &Detection { value: 5.0, band: Band::Detected, ..Default::default() }, target, now, &t);
    a.settle(FormId(R_TRAVEL), now, &t, true);
    assert_eq!(a.combat_began, Some(now));
    let want = fnv_nav::follow::heading_of([300.0, 0.0]);
    s.ai.agents.get_mut(&FormId(R_TRAVEL)).unwrap().walker.heading = want + 3.0;
    let off = |s: &Sim| fnv_nav::follow::heading_delta(want, s.ai.agents[&FormId(R_TRAVEL)].walker.heading);
    // A second in: it stands where it was, on its heading.
    s.run(1.0);
    assert_eq!(s.ai.agents[&FormId(R_TRAVEL)].activity, Activity::There);
    assert!(dist(s.feet(R_TRAVEL), at) < 1.0, "it stands");
    assert!((off(&s) - 3.0).abs() < 1e-4, "{}", off(&s));
    // Then half a second at the wide rate, and on to face it at the near one.
    s.run(0.5);
    assert!((off(&s) - (3.0 - COMBAT_TURN_WIDE * 0.5)).abs() < 1e-3, "{}", off(&s));
    s.run(0.5);
    assert!(off(&s).abs() < 1e-3, "{}", off(&s));
}

#[test]
fn a_creature_going_into_combat_turns_at_once() {
    // People only stand 1.1 s on going into combat (#1352); a creature turns to its
    // target at once, as before (see crates/fnv-ai/src/carry.rs).
    let f = fixture();
    let mut s = sim(&f, &[R_HOUND]);
    s.run(1.0);
    let hound = FormId(R_HOUND);
    assert!(fnv_ai::alert::creature(&s.game, hound));
    let (now, t) = (s.game.time, Timers::default());
    let at = s.feet(R_HOUND);
    let target = [at[0] + 300.0, at[1], at[2]];
    let a = s.game.awareness.actors.entry(hound).or_default();
    a.observe(FormId::PLAYER_REF, &Detection { value: 5.0, band: Band::Detected, ..Default::default() }, target, now, &t);
    a.settle(hound, now, &t, true);
    assert_eq!(a.state, State::Combat);
    assert_eq!(a.combat_began, Some(now));
    assert!(!fnv_ai::alert::reacting(&s.game, hound));
    assert_eq!(fnv_ai::alert::combat_began(&s.game, hound), None);
    let want = fnv_nav::follow::heading_of([300.0, 0.0]);
    s.ai.agents.get_mut(&hound).unwrap().walker.heading = want + 3.0;
    s.run(0.5);
    let off = fnv_nav::follow::heading_delta(want, s.ai.agents[&hound].walker.heading);
    assert!(off.abs() < 2.5, "it turned within the half second a person stands: {off}");
}

#[test]
fn an_actor_in_combat_closes_in_on_a_target_it_cannot_see() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    s.run(1.0);
    let (now, t) = (s.game.time, Timers::default());
    let at = s.feet(R_TRAVEL);
    let target = [at[0] + 300.0, at[1], at[2]];
    let a = s.game.awareness.actors.entry(FormId(R_TRAVEL)).or_default();
    a.observe(FormId::PLAYER_REF, &Detection { value: 5.0, band: Band::Detected, ..Default::default() }, target, now, &t);
    a.settle(FormId(R_TRAVEL), now, &t, true);
    // Within the distance it fights from, but the target is out of sight now.
    s.game.awareness.engage.insert(FormId(R_TRAVEL), (1000.0, 1200.0));
    let a = s.game.awareness.actors.get_mut(&FormId(R_TRAVEL)).unwrap();
    a.observe(FormId::PLAYER_REF, &Detection { value: -10.0, band: Band::Noticed, ..Default::default() }, target, now, &t);
    s.run(2.0);
    assert!(s.events(R_TRAVEL).iter().any(|e| matches!(e, AiEvent::Close { target, .. } if *target == FormId::PLAYER_REF)), "{:?}", s.events(R_TRAVEL));
    assert!(dist(s.feet(R_TRAVEL), target) < 280.0, "it moved towards the target");
}

/// Put `actor` in Combat with the player at `at`, detected.
fn fighting(s: &mut Sim, actor: u32, at: [f32; 3]) {
    let (now, t) = (s.game.time, Timers::default());
    let a = s.game.awareness.actors.entry(FormId(actor)).or_default();
    a.observe(FormId::PLAYER_REF, &Detection { value: 5.0, band: Band::Detected, ..Default::default() }, at, now, &t);
    a.settle(FormId(actor), now, &t, true);
    assert_eq!(a.state, State::Combat);
    // Past the moment it stands on going into combat (`fnv_ai::alert::reacting`): what
    // the tests below look at is what a fighter does once it fights.
    a.combat_began = Some(now - fnv_ai::alert::COMBAT_REACTION_SECONDS - 1.0);
}

#[test]
fn a_fleeing_actor_runs_away_and_stays_at_its_spot() {
    // Combat Flee (combat-ai.md §12.2): no door or cover near, so it runs straight away
    // from the player; the flee distance shortened to fit cell C (1,000 units square).
    use fnv_game::combat_group::{FleeBy, Flight, GroupSettings};
    let f = fixture();
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let threat = [10450.0, 10150.0, 0.0];
    s.game.awareness.tactics.settings = Some(GroupSettings { flee_normal: 400.0, ..GroupSettings::default() });
    fighting(&mut s, R_SB_NOTHING, threat);
    let now = s.game.time;
    s.game.awareness.tactics.flights.insert(FormId(R_SB_NOTHING), Flight { from: FormId::PLAYER_REF, from_at: threat, since: now, spot: None, arrived: None });
    let before = dist(s.feet(R_SB_NOTHING), threat);
    s.run(4.0);
    let ev = s.events(R_SB_NOTHING);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Flee { how: FleeBy::Away, .. })), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::FleeArrived { .. })), "{ev:?}");
    let fled = s.feet(R_SB_NOTHING);
    assert!(dist(fled, threat) > before + 300.0, "{fled:?}");
    assert!(s.game.awareness.tactics.flights[&FormId(R_SB_NOTHING)].arrived.is_some());
    // It stays at its spot.
    s.run(5.0);
    assert!(dist(s.feet(R_SB_NOTHING), fled) < 1.0);
}

#[test]
fn a_fleeing_actor_takes_a_door_near_it() {
    // Combat Flee's first try: a door within fCombatFleeMaxDoorDistance, not nearer the
    // attacker than the actor is. The traveller in cell A has the door to B 400 units off.
    use fnv_game::combat_group::{FleeBy, Flight};
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    s.run(0.5);
    let threat = [0.0, 0.0, 0.0];
    fighting(&mut s, R_TRAVEL, threat);
    let now = s.game.time;
    s.game.awareness.tactics.flights.insert(FormId(R_TRAVEL), Flight { from: FormId::PLAYER_REF, from_at: threat, since: now, spot: None, arrived: None });
    s.run(1.0);
    let ev = s.events(R_TRAVEL);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Flee { how: FleeBy::Door { door }, .. } if door.0 == DOOR_A)), "{ev:?}");
}

#[test]
fn a_fighter_goes_to_its_post_and_faces_its_target() {
    // A post (combat-ai.md §12.1): the combat AI wants it on its target's flank.
    use fnv_game::combat_group::{Post, PostKind};
    let f = fixture();
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let target = [10500.0, 10900.0, 0.0];
    fighting(&mut s, R_SB_NOTHING, target);
    s.game.awareness.engage.insert(FormId(R_SB_NOTHING), (2000.0, 2500.0));
    let post = [10200.0, 10600.0, 0.0];
    s.game.awareness.tactics.posts.insert(FormId(R_SB_NOTHING), Post { at: post, kind: PostKind::Flank, run: true });
    s.run(3.0);
    let ev = s.events(R_SB_NOTHING);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Post { post: PostKind::Flank, .. })), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::AtPost { post: PostKind::Flank, .. })), "{ev:?}");
    assert!(dist(s.feet(R_SB_NOTHING), post) < 40.0);
    // A post it cannot reach is dropped.
    s.game.awareness.tactics.posts.insert(FormId(R_SB_NOTHING), Post { at: [20000.0, 20000.0, 0.0], kind: PostKind::Cover, run: true });
    s.run(1.0);
    assert!(s.events(R_SB_NOTHING).iter().any(|e| matches!(e, AiEvent::NoPost { post: PostKind::Cover })));
    assert!(!s.game.awareness.tactics.posts.contains_key(&FormId(R_SB_NOTHING)));
}

#[test]
fn a_fighter_running_to_its_post_walks_once_it_raises_its_sights() {
    // Aiming down the sights slows it to a walk (alert.rs module notes), also on a run to
    // a post already under way.
    use fnv_game::combat_group::{Post, PostKind};
    let f = fixture();
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let target = [10500.0, 10900.0, 0.0];
    fighting(&mut s, R_SB_NOTHING, target);
    s.game.awareness.engage.insert(FormId(R_SB_NOTHING), (2000.0, 2500.0));
    let post = [10200.0, 10600.0, 0.0];
    s.game.awareness.tactics.posts.insert(FormId(R_SB_NOTHING), Post { at: post, kind: PostKind::Flank, run: true });
    s.run(0.5);
    assert!(s.events(R_SB_NOTHING).iter().any(|e| matches!(e, AiEvent::Post { post: PostKind::Flank, .. })), "{:?}", s.events(R_SB_NOTHING));
    assert!(!s.events(R_SB_NOTHING).iter().any(|e| matches!(e, AiEvent::AtPost { .. })), "still on its way");
    s.game.awareness.aiming.insert(FormId(R_SB_NOTHING));
    s.run(0.5);
    let ev = s.events(R_SB_NOTHING);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Pace { aiming: true, .. })), "{ev:?}");
}

// A walk to a post that ends away from it (combat-ai.md §12.1). `R_SB_NOTHING` stands at
// (10450, 10450) in cell C, whose navmesh ends at 11000.

const POST_TARGET: [f32; 3] = [10500.0, 10900.0, 0.0];

/// Takes the actors over and gets nowhere: it turns on the spot.
struct NoProgress;
impl fnv_ai::Mover for NoProgress {
    fn walk(&mut self, _: FormId, _: &mut fnv_nav::Walker, _: &NavGraph, _: f32) -> Option<fnv_nav::WalkEvent> {
        Some(fnv_nav::WalkEvent::Turned)
    }
}

/// Leaves the actors to their walkers: the way is traversable.
struct OnFoot;
impl fnv_ai::Mover for OnFoot {}

/// A fighter that keeps seeing its target at `POST_TARGET`, stopping within `stop`, with a
/// post of the given kind at `at`.
fn fighter_with_post<'a>(f: &'a Fixture, stop: f32, at: [f32; 3], kind: fnv_game::combat_group::PostKind) -> Sim<'a> {
    let mut s = sim(f, &[R_SB_NOTHING]);
    s.run(1.0);
    fighting(&mut s, R_SB_NOTHING, POST_TARGET);
    s.game.awareness.engage.insert(FormId(R_SB_NOTHING), (stop, stop + 500.0));
    ask_post(&mut s, at, kind);
    s
}

fn ask_post(s: &mut Sim, at: [f32; 3], kind: fnv_game::combat_group::PostKind) {
    s.game.awareness.tactics.posts.insert(FormId(R_SB_NOTHING), fnv_game::combat_group::Post { at, kind, run: true });
}

fn fight_on(s: &mut Sim, seconds: f64, mover: &mut dyn fnv_ai::Mover) {
    for _ in 0..(seconds * 2.0) as usize {
        s.game.tick();
        fighting(s, R_SB_NOTHING, POST_TARGET);
        s.ai.update_moved(&mut s.game, &s.nav, 0.5, mover);
    }
}

/// Update by update until the first failure; on its way until then. Returns on the very
/// update that fails, so what is checked next is that update's doing.
fn fight_until_post_fails(s: &mut Sim, mover: &mut dyn fnv_ai::Mover) {
    for _ in 0..80 {
        fight_on(s, 0.5, mover);
        if post_counts(s).2 > 0 {
            return;
        }
        assert!(matches!(search(s), Some(fnv_ai::alert::Search::Posted { .. })), "{:?}", search(s));
        assert!(has_post(s));
    }
    panic!("no failure: {:?}", s.events(R_SB_NOTHING));
}

/// Takes the actors over and has them wait for a door that does not open.
struct AtShutDoor;
impl fnv_ai::Mover for AtShutDoor {
    fn walk(&mut self, _: FormId, _: &mut fnv_nav::Walker, _: &NavGraph, _: f32) -> Option<fnv_nav::WalkEvent> {
        Some(fnv_nav::WalkEvent::WaitingForDoor(DOOR_A))
    }
}

/// The door waits given up.
fn door_waits_cancelled(s: &Sim) -> Vec<u32> {
    s.events(R_SB_NOTHING).iter().filter_map(|e| if let AiEvent::DoorWaitCancelled { door } = e { Some(door.0) } else { None }).collect()
}

/// How many walks to a post were planned, how many arrived, how many failed.
fn post_counts(s: &Sim) -> (usize, usize, usize) {
    let ev = s.events(R_SB_NOTHING);
    let n = |f: fn(&AiEvent) -> bool| ev.iter().filter(|e| f(e)).count();
    (n(|e| matches!(e, AiEvent::Post { .. })), n(|e| matches!(e, AiEvent::AtPost { .. })), n(|e| matches!(e, AiEvent::PostFailed { .. })))
}

fn has_post(s: &Sim) -> bool {
    s.game.awareness.tactics.posts.contains_key(&FormId(R_SB_NOTHING))
}

fn search(s: &Sim) -> Option<fnv_ai::alert::Search> {
    s.ai.agents[&FormId(R_SB_NOTHING)].search
}

fn holding() -> Option<fnv_ai::alert::Search> {
    Some(fnv_ai::alert::Search::Holding { target: FormId::PLAYER_REF })
}

/// The one failure logged: requested point, feet, distance, seconds, why.
fn the_failure(s: &Sim) -> ([f32; 3], [f32; 3], f32, f32, fnv_ai::alert::PostFailure) {
    let ev = s.events(R_SB_NOTHING);
    let mut failed = ev.iter().filter_map(|e| match e {
        AiEvent::PostFailed { at, feet, off, elapsed, reason, target, .. } => {
            assert_eq!(*target, FormId::PLAYER_REF);
            Some((*at, *feet, *off, *elapsed, *reason))
        }
        _ => None,
    });
    let first = failed.next().unwrap_or_else(|| panic!("no failure: {ev:?}"));
    assert!(failed.next().is_none(), "{ev:?}");
    first
}

#[test]
fn a_post_walked_to_is_arrival_and_is_kept() {
    // The traversable control for the tests below.
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let post = [10200.0, 10600.0, 0.0];
    let mut s = fighter_with_post(&f, 2000.0, post, PostKind::Flank);
    fight_on(&mut s, 3.0, &mut OnFoot);
    let feet = s.feet(R_SB_NOTHING);
    assert!(dist(feet, post) < 40.0);
    let ev = s.events(R_SB_NOTHING);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::AtPost { post: PostKind::Flank, off, at, feet: logged, .. } if *off < 40.0 && *at == post && *logged == feet)), "{ev:?}");
    assert!(has_post(&s));
    assert!(matches!(search(&s), Some(fnv_ai::alert::Search::AtPost { kind: PostKind::Flank, at, .. }) if at == post));
    // Standing there outlasts the deadline, which only a walk has.
    fight_on(&mut s, 35.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (1, 1, 0));
    assert!(has_post(&s));
}

#[test]
fn a_fighter_that_gets_nowhere_fails_its_post_at_the_deadline_and_fights_on() {
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let post = [10200.0, 10600.0, 0.0];
    let mut s = fighter_with_post(&f, 2000.0, post, PostKind::Flank);
    let start = s.feet(R_SB_NOTHING);
    // It has its back to its target, and gets no chance to turn on its way.
    let to = [POST_TARGET[0] - start[0], POST_TARGET[1] - start[1]];
    let towards = |heading: f32| {
        let d = fnv_nav::follow::dir_of(heading);
        (d[0] * to[0] + d[1] * to[1]) / to[0].hypot(to[1])
    };
    s.ai.agents.get_mut(&FormId(R_SB_NOTHING)).unwrap().walker.heading = fnv_nav::follow::heading_of([-to[0], -to[1]]);
    // Until the deadline it is on its way; on the update that passes it, it holds.
    fight_until_post_fails(&mut s, &mut NoProgress);
    // On the update that failed it turned, as far as a turn goes in that update.
    let h = s.ai.agents[&FormId(R_SB_NOTHING)].walker.heading;
    assert!(towards(h) > 0.4, "{h} {}", towards(h));
    assert_eq!(post_counts(&s), (1, 0, 1), "{:?}", s.events(R_SB_NOTHING));
    let (at, feet, off, elapsed, reason) = the_failure(&s);
    assert_eq!(reason, fnv_ai::alert::PostFailure::TimedOut);
    assert_eq!((at, feet), (post, start));
    assert!((off - dist(start, post)).abs() < 0.01 && off > 200.0, "{off}");
    assert!(elapsed > 30.0 && elapsed < 31.0, "{elapsed}");
    assert_eq!(s.feet(R_SB_NOTHING), start);
    // Released, the walk abandoned, and holding on that same update.
    assert!(!has_post(&s));
    let a = &s.ai.agents[&FormId(R_SB_NOTHING)];
    assert!(a.walker.follower.is_none() && a.waiting_at.is_none());
    assert_eq!(a.activity, Activity::There);
    assert_eq!(search(&s), holding());
    assert!(s.events(R_SB_NOTHING).iter().any(|e| matches!(e, AiEvent::Hold { target, .. } if *target == FormId::PLAYER_REF)));
    assert!(door_waits_cancelled(&s).is_empty(), "it was not waiting at a door");
    // The next one finishes the turn.
    fight_on(&mut s, 0.5, &mut NoProgress);
    assert!(towards(s.ai.agents[&FormId(R_SB_NOTHING)].walker.heading) > 0.99);
    // Past another deadline: no retry, the next try is the combat AI's.
    fight_on(&mut s, 35.0, &mut NoProgress);
    assert_eq!(post_counts(&s), (1, 0, 1));
    assert!(!has_post(&s));
    assert_eq!(search(&s), holding());
}

#[test]
fn a_fighter_whose_post_failed_beyond_its_distance_closes_in() {
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let mut s = fighter_with_post(&f, 100.0, [10200.0, 10600.0, 0.0], PostKind::Cover);
    let start = s.feet(R_SB_NOTHING);
    fight_until_post_fails(&mut s, &mut NoProgress);
    assert_eq!(post_counts(&s), (1, 0, 1), "{:?}", s.events(R_SB_NOTHING));
    assert!(!has_post(&s));
    // On the update that failed it set off: a walk of its own, not the abandoned one.
    let a = &s.ai.agents[&FormId(R_SB_NOTHING)];
    assert!(a.walker.follower.is_some());
    assert_eq!(a.activity, Activity::Travelling);
    assert!(matches!(search(&s), Some(fnv_ai::alert::Search::Closing { target, .. }) if target == FormId::PLAYER_REF), "{:?}", search(&s));
    let ev = s.events(R_SB_NOTHING);
    let failed = ev.iter().position(|e| matches!(e, AiEvent::PostFailed { .. })).unwrap();
    assert!(matches!(ev.last(), Some(AiEvent::Close { .. })) && failed + 1 == ev.len() - 1, "{ev:?}");
    // Able to move, its first step goes towards the target and it turns that way.
    fight_on(&mut s, 0.5, &mut OnFoot);
    let feet = s.feet(R_SB_NOTHING);
    assert!(dist(feet, POST_TARGET) < dist(start, POST_TARGET), "{feet:?}");
    let (d, to) = (fnv_nav::follow::dir_of(s.ai.agents[&FormId(R_SB_NOTHING)].walker.heading), [POST_TARGET[0] - feet[0], POST_TARGET[1] - feet[1]]);
    assert!((d[0] * to[0] + d[1] * to[1]) / to[0].hypot(to[1]) > 0.9, "{d:?}");
    // It gets there; still no second try at the post.
    fight_on(&mut s, 5.0, &mut OnFoot);
    assert!(dist(s.feet(R_SB_NOTHING), POST_TARGET) < 150.0, "{:?}", s.feet(R_SB_NOTHING));
    assert_eq!(post_counts(&s), (1, 0, 1));
}

/// A fighter stopping within 100 units of the player, who backs away from it at `speed`
/// units/s for 2 s: the share of the updates of the last second it was within `reach`.
fn within_reach_of_a_backing_target(speed: f32, reach: f32) -> f32 {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let start = s.feet(R_SB_NOTHING);
    // The target 120 units off, backing straight away from the fighter towards POST_TARGET.
    let to = [POST_TARGET[0] - start[0], POST_TARGET[1] - start[1]];
    let len = to[0].hypot(to[1]);
    let dir = [to[0] / len, to[1] / len];
    assert!(len > 120.0 + speed * 2.0, "room to back away: {len}");
    let (dt, mut within, mut counted) = (0.05, 0, 0);
    for i in 0..40 {
        let gone = 120.0 + speed * dt * i as f32;
        let at = [start[0] + dir[0] * gone, start[1] + dir[1] * gone, start[2]];
        s.game.time += f64::from(dt);
        fighting(&mut s, R_SB_NOTHING, at);
        s.game.awareness.engage.insert(FormId(R_SB_NOTHING), (100.0, reach));
        s.ai.update_moved(&mut s.game, &s.nav, dt, &mut OnFoot);
        if i >= 20 {
            counted += 1;
            within += usize::from(dist(s.feet(R_SB_NOTHING), at) <= reach);
        }
    }
    within as f32 / counted as f32
}

#[test]
fn a_fighter_keeps_within_reach_of_a_target_backing_away() {
    // #2163: a target that keeps moving off is followed, not left to walk out of reach.
    for speed in [70.0, 150.0] {
        let share = within_reach_of_a_backing_target(speed, 106.0);
        assert!(share >= 0.9, "backing away at {speed}: within reach {share} of the time");
    }
}

#[test]
fn a_fighter_chases_its_detected_target_where_it_is_not_where_it_was_measured() {
    // #2163: detection measures a target a few times a second; the player has moved on
    // 150 units since. The fighter does not stop where the player was measured.
    let f = fixture();
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let start = s.feet(R_SB_NOTHING);
    let to = [POST_TARGET[0] - start[0], POST_TARGET[1] - start[1]];
    let len = to[0].hypot(to[1]);
    let point = |d: f32| [start[0] + to[0] / len * d, start[1] + to[1] / len * d, start[2]];
    let (measured, really) = (point(150.0), point(300.0));
    {
        let p = s.game.world.get(FormId::PLAYER_REF).unwrap();
        (p.cell, p.pos) = (Some(FormId(CELL_C)), really);
    }
    for _ in 0..60 {
        s.game.time += 0.05;
        fighting(&mut s, R_SB_NOTHING, measured);
        s.game.awareness.engage.insert(FormId(R_SB_NOTHING), (100.0, 106.0));
        s.ai.update_moved(&mut s.game, &s.nav, 0.05, &mut OnFoot);
    }
    let feet = s.feet(R_SB_NOTHING);
    assert!(dist(feet, really) <= 100.0, "{} from the player, {} from where it was measured", dist(feet, really), dist(feet, measured));
    assert_eq!(search(&s), holding());
    assert_eq!(s.events(R_SB_NOTHING).iter().filter(|e| matches!(e, AiEvent::Hold { .. })).count(), 1, "it stopped once, at its distance");
}

#[test]
fn a_chase_is_planned_again_sooner_near_its_target_and_leads_one_that_moves() {
    use fnv_ai::alert::{keeps_way, lead, replan_slack, Drift, CHASE_LEAD_MAX, CLOSE_REPLAN_DISTANCE, CLOSE_REPLAN_NEAR};
    // Far off, the target may move 96 units before the way is planned again; with 100
    // units still to come, 25; at its distance, 12.
    assert_eq!(replan_slack(2000.0, 150.0), CLOSE_REPLAN_DISTANCE);
    assert_eq!(replan_slack(250.0, 150.0), 25.0);
    assert_eq!(replan_slack(160.0, 150.0), CLOSE_REPLAN_NEAR);
    assert_eq!(replan_slack(100.0, 150.0), CLOSE_REPLAN_NEAR);
    let to = [0.0, 0.0, 0.0];
    assert!(keeps_way(to, 10.0, [20.0, 0.0, 0.0], 250.0, 150.0, 10.5));
    assert!(!keeps_way(to, 10.0, [30.0, 0.0, 0.0], 250.0, 150.0, 10.5), "the target moved 30 units, 25 allowed");
    assert!(!keeps_way(to, 10.0, [20.0, 0.0, 0.0], 250.0, 150.0, 11.6), "1.5 s have passed");
    // The target's speed: measured over 0.05 s or more; the first measurement as it is
    // (#3256: evened with the "standing" it began with, a walking target read as half
    // its speed and was taken for standing), each later one evened with the one before.
    let d = Drift::begin(to, 10.0);
    assert_eq!((d.speed, d.measured), ([0.0, 0.0], false));
    assert_eq!(d.on([5.0, 0.0, 0.0], 10.02), d, "too soon to measure");
    let d = d.on([30.0, 0.0, 0.0], 10.1);
    assert!((d.speed[0] - 300.0).abs() < 0.01 && d.speed[1] == 0.0 && d.measured, "{d:?}");
    let d = d.on([40.0, 0.0, 0.0], 10.2);
    assert!((d.speed[0] - 200.0).abs() < 0.01, "{d:?}");
    // A player walking backwards (69 units/s) is a moving target from the first measurement.
    let walk = Drift::begin(to, 10.0).on([6.9, 0.0, 0.0], 10.1);
    assert!(walk.speed[0] >= fnv_ai::alert::CHASE_LEAD_MIN_SPEED, "{walk:?}");
    // A moving target is closed in on 0.35 s ahead of it, at most 96 units; a slow one
    // where it is.
    assert_eq!(lead([100.0, 50.0, 7.0], [200.0, 0.0]), [170.0, 50.0, 7.0]);
    assert_eq!(lead([100.0, 50.0, 7.0], [0.0, -1000.0]), [100.0, 50.0 - CHASE_LEAD_MAX, 7.0]);
    assert_eq!(lead([100.0, 50.0, 7.0], [30.0, 0.0]), [100.0, 50.0, 7.0]);
}

#[test]
fn a_door_wait_given_up_with_the_walk_is_not_a_passage() {
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let actor = FormId(R_SB_NOTHING);
    // Failing, far from the post.
    let mut s = fighter_with_post(&f, 2000.0, [10200.0, 10600.0, 0.0], PostKind::Flank);
    fight_on(&mut s, 1.0, &mut AtShutDoor);
    assert_eq!(s.ai.agents[&actor].waiting_at, Some(DOOR_A));
    assert!(door_waits_cancelled(&s).is_empty());
    fight_until_post_fails(&mut s, &mut AtShutDoor);
    assert_eq!(s.ai.agents[&actor].waiting_at, None);
    assert_eq!(door_waits_cancelled(&s), vec![DOOR_A]);
    assert_eq!(search(&s), holding());
    fight_on(&mut s, 5.0, &mut AtShutDoor);
    assert_eq!(door_waits_cancelled(&s), vec![DOOR_A], "told once");
    // Arriving by the deadline, near the post: the walk is over all the same.
    let mut s = fighter_with_post(&f, 2000.0, [10513.0, 10450.0, 0.0], PostKind::Shift);
    fight_on(&mut s, 33.0, &mut AtShutDoor);
    assert_eq!(post_counts(&s), (1, 1, 0));
    assert_eq!(s.ai.agents[&actor].waiting_at, None);
    assert_eq!(door_waits_cancelled(&s), vec![DOOR_A]);
    // The door opening and the walk going on is not one.
    let mut s = fighter_with_post(&f, 2000.0, [10200.0, 10600.0, 0.0], PostKind::Flank);
    fight_on(&mut s, 1.0, &mut AtShutDoor);
    assert_eq!(s.ai.agents[&actor].waiting_at, Some(DOOR_A));
    fight_on(&mut s, 3.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (1, 1, 0));
    assert_eq!(s.ai.agents[&actor].waiting_at, None);
    assert!(door_waits_cancelled(&s).is_empty());
}

#[test]
fn arrival_is_measured_to_the_post_asked_for_now_not_the_one_planned() {
    // The first request snaps 20 units to the navmesh's edge, where the way ends. A request
    // moved within the slack keeps that way, and the feet are measured to the new point:
    // the policy (combat-ai.md §12.1) even when the slack and the snap add up past arrival.
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let first = [11020.0, 10500.0, 0.0];
    let run = |moved_to: [f32; 3]| {
        let mut s = fighter_with_post(&f, 2000.0, first, PostKind::Flank);
        fight_on(&mut s, 0.5, &mut OnFoot);
        ask_post(&mut s, moved_to, PostKind::Flank);
        fight_on(&mut s, 3.0, &mut OnFoot);
        s
    };
    // Not moved, and moved 40: 20 and 60 off.
    for (x, off) in [(11020.0, 20.0), (11060.0, 60.0)] {
        let s = run([x, 10500.0, 0.0]);
        assert_eq!(post_counts(&s), (1, 1, 0), "{:?}", s.events(R_SB_NOTHING));
        assert!((dist(s.feet(R_SB_NOTHING), [x, 10500.0, 0.0]) - off).abs() < 2.0);
    }
    // Moved 47: the way the same, but 67 off. It finished its way and still failed.
    let moved = [11067.0, 10500.0, 0.0];
    let s = run(moved);
    assert_eq!(post_counts(&s), (1, 0, 1), "{:?}", s.events(R_SB_NOTHING));
    let (at, feet, off, _, reason) = the_failure(&s);
    assert_eq!(reason, fnv_ai::alert::PostFailure::StoppedShort);
    assert_eq!(at, moved);
    assert!((64.0..70.0).contains(&off), "{off}");
    assert!(dist(feet, first) < 64.0, "near the first request: {feet:?}");
    assert!(!has_post(&s));
}

#[test]
fn a_way_ending_near_a_post_off_the_navmesh_is_arrival() {
    // The way ends at the navmesh's edge, about 60 units from the point asked for.
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let post = [11060.0, 10500.0, 0.0];
    let mut s = fighter_with_post(&f, 2000.0, post, PostKind::Cover);
    fight_on(&mut s, 3.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (1, 1, 0), "{:?}", s.events(R_SB_NOTHING));
    let off = dist(s.feet(R_SB_NOTHING), post);
    assert!(off > 55.0 && off < 64.0, "{off}");
    assert!(has_post(&s));
    assert!(matches!(search(&s), Some(fnv_ai::alert::Search::AtPost { .. })));
    fight_on(&mut s, 5.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (1, 1, 0), "it stays");
}

#[test]
fn a_way_ending_far_from_a_post_off_the_navmesh_fails_at_once() {
    // The way ends at the navmesh's edge, about 100 units from the point asked for: there
    // is no waiting for the deadline.
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let post = [11100.0, 10500.0, 0.0];
    let mut s = fighter_with_post(&f, 2000.0, post, PostKind::Cover);
    fight_on(&mut s, 3.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (1, 0, 1), "{:?}", s.events(R_SB_NOTHING));
    let (at, feet, off, elapsed, reason) = the_failure(&s);
    assert_eq!(reason, fnv_ai::alert::PostFailure::StoppedShort);
    assert_eq!(at, post);
    assert!(off >= 64.0 && (off - dist(feet, post)).abs() < 0.01, "{off}");
    assert!(elapsed < 3.0, "{elapsed}");
    assert!(!has_post(&s));
    assert_eq!(search(&s), holding());
}

#[test]
fn the_deadline_passing_near_the_post_is_arrival() {
    // No way covered, but feet within the arrival distance count: 63 does, 64 does not.
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let mut s = fighter_with_post(&f, 2000.0, [10513.0, 10450.0, 0.0], PostKind::Shift);
    fight_on(&mut s, 33.0, &mut NoProgress);
    assert_eq!(post_counts(&s), (1, 1, 0), "{:?}", s.events(R_SB_NOTHING));
    assert!(s.events(R_SB_NOTHING).iter().any(|e| matches!(e, AiEvent::AtPost { off, .. } if (*off - 63.0).abs() < 0.01)));
    assert!(has_post(&s));
    assert!(s.ai.agents[&FormId(R_SB_NOTHING)].walker.follower.is_none());

    let mut s = fighter_with_post(&f, 2000.0, [10514.0, 10450.0, 0.0], PostKind::Shift);
    fight_on(&mut s, 33.0, &mut NoProgress);
    assert_eq!(post_counts(&s), (1, 0, 1), "{:?}", s.events(R_SB_NOTHING));
    assert_eq!(the_failure(&s).4, fnv_ai::alert::PostFailure::TimedOut);
    assert!(!has_post(&s));
}

#[test]
fn a_replaced_post_gets_its_own_walk_and_deadline() {
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let first = [10200.0, 10600.0, 0.0];
    let mut s = fighter_with_post(&f, 2000.0, first, PostKind::Flank);
    fight_on(&mut s, 20.0, &mut NoProgress);
    // Moved within the slack it is the same post, on the same way and the same clock.
    let moved = [10240.0, 10600.0, 0.0];
    ask_post(&mut s, moved, PostKind::Flank);
    fight_on(&mut s, 2.0, &mut NoProgress);
    assert_eq!(post_counts(&s), (1, 0, 0));
    // Moved farther it is a new one.
    let second = [10700.0, 10600.0, 0.0];
    ask_post(&mut s, second, PostKind::Flank);
    fight_on(&mut s, 20.0, &mut NoProgress);
    assert_eq!(post_counts(&s), (2, 0, 0), "42 s since the first was planned, 20 since the second");
    assert!(has_post(&s));
    // So is another kind at the same place.
    ask_post(&mut s, second, PostKind::Cover);
    fight_on(&mut s, 20.0, &mut NoProgress);
    assert_eq!(post_counts(&s), (3, 0, 0));
    fight_on(&mut s, 12.0, &mut NoProgress);
    assert_eq!(post_counts(&s), (3, 0, 1), "{:?}", s.events(R_SB_NOTHING));
    let (at, _, _, elapsed, _) = the_failure(&s);
    assert_eq!(at, second);
    assert!(elapsed > 30.0 && elapsed < 31.0, "{elapsed}");
    assert!(!has_post(&s));
    // A failed post asked for again is a new assignment, the combat AI's to make.
    ask_post(&mut s, moved, PostKind::Flank);
    fight_on(&mut s, 3.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (4, 1, 1), "{:?}", s.events(R_SB_NOTHING));
    assert!(dist(s.feet(R_SB_NOTHING), moved) < 40.0);
}

#[test]
fn a_fighter_moved_off_its_post_goes_back_only_past_the_retained_distance() {
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let post = [10200.0, 10600.0, 0.0];
    let mut s = fighter_with_post(&f, 2000.0, post, PostKind::Flank);
    fight_on(&mut s, 3.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (1, 1, 0));
    // Beyond the arrival distance, within the retained one: it stays, or an actor shoved
    // about at the edge would flip between the states and fill the log.
    s.ai.agents.get_mut(&FormId(R_SB_NOTHING)).unwrap().walker.feet = [post[0] + 80.0, post[1], 0.0];
    fight_on(&mut s, 2.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (1, 1, 0));
    assert!(matches!(search(&s), Some(fnv_ai::alert::Search::AtPost { .. })));
    // Past it: not at its post any more, it walks back.
    s.ai.agents.get_mut(&FormId(R_SB_NOTHING)).unwrap().walker.feet = [post[0] + 200.0, post[1], 0.0];
    fight_on(&mut s, 0.5, &mut OnFoot);
    assert_eq!(post_counts(&s).0, 2);
    assert!(matches!(search(&s), Some(fnv_ai::alert::Search::Posted { .. })), "{:?}", search(&s));
    fight_on(&mut s, 3.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (2, 2, 0), "{:?}", s.events(R_SB_NOTHING));
    assert!(dist(s.feet(R_SB_NOTHING), post) < 40.0);
}

#[test]
fn going_back_to_a_post_on_a_way_that_stops_short_ends_in_failure() {
    // At a post off the navmesh, 60 units off, then asked for 40 units farther: still the
    // same post by the slack, but past the retained distance. The way back ends where it
    // stands, too far, so the post goes: this cannot loop.
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let mut s = fighter_with_post(&f, 2000.0, [11060.0, 10500.0, 0.0], PostKind::Cover);
    fight_on(&mut s, 3.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (1, 1, 0));
    ask_post(&mut s, [11100.0, 10500.0, 0.0], PostKind::Cover);
    fight_on(&mut s, 3.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (2, 1, 1), "{:?}", s.events(R_SB_NOTHING));
    assert_eq!(the_failure(&s).4, fnv_ai::alert::PostFailure::StoppedShort);
    assert!(!has_post(&s));
    fight_on(&mut s, 35.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (2, 1, 1));
}

#[test]
fn a_post_with_no_way_to_it_stays_no_post_not_a_failure() {
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    let mut s = fighter_with_post(&f, 2000.0, [20000.0, 20000.0, 0.0], PostKind::Cover);
    fight_on(&mut s, 1.0, &mut NoProgress);
    assert!(s.events(R_SB_NOTHING).iter().any(|e| matches!(e, AiEvent::NoPost { post: PostKind::Cover })));
    assert_eq!(post_counts(&s), (0, 0, 0));
    assert!(!has_post(&s));
    assert_eq!(search(&s), holding());
}

#[test]
fn a_post_the_combat_ai_takes_back_is_no_failure() {
    use fnv_game::combat_group::PostKind;
    let f = fixture();
    // On its way.
    let mut s = fighter_with_post(&f, 2000.0, [10200.0, 10600.0, 0.0], PostKind::Flank);
    fight_on(&mut s, 1.0, &mut NoProgress);
    s.game.awareness.tactics.posts.remove(&FormId(R_SB_NOTHING));
    fight_on(&mut s, 33.0, &mut NoProgress);
    assert_eq!(post_counts(&s), (1, 0, 0));
    assert_eq!(search(&s), holding());
    assert!(s.ai.agents[&FormId(R_SB_NOTHING)].walker.follower.is_none());
    // At its post.
    let mut s = fighter_with_post(&f, 2000.0, [10200.0, 10600.0, 0.0], PostKind::Flank);
    fight_on(&mut s, 3.0, &mut OnFoot);
    s.game.awareness.tactics.posts.remove(&FormId(R_SB_NOTHING));
    fight_on(&mut s, 1.0, &mut OnFoot);
    assert_eq!(post_counts(&s), (1, 1, 0));
    assert_eq!(search(&s), holding());
}

#[test]
fn a_search_party_splits_the_circle() {
    // Combat Search (combat-ai.md §12.5): two sitters placed from one base form, alarmed
    // by the same shot, search together, each its own half of the circle round it.
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS, R_SB_OBJECTS_2]);
    s.run(1.0);
    let t = Timers::default();
    for a in [R_SB_OBJECTS, R_SB_OBJECTS_2] {
        let now = s.game.time;
        let x = s.game.awareness.actors.entry(FormId(a)).or_default();
        x.alarm(FormId::PLAYER_REF, MIDDLE_C, -5.0, Cause::Event(fnv_game::awareness::EventType::Bullet), now, &t);
        x.settle(FormId(a), now, &t, true);
    }
    assert_eq!(s.game.search_party(FormId(R_SB_OBJECTS), FormId::PLAYER_REF), (0, 2));
    assert_eq!(s.game.search_party(FormId(R_SB_OBJECTS_2), FormId::PLAYER_REF), (1, 2));
    s.run(18.0);
    let points = |a: u32| -> Vec<[f32; 3]> {
        s.events(a)
            .into_iter()
            .filter_map(|e| match e {
                AiEvent::Search { to, .. } => Some(*to),
                _ => None,
            })
            .skip(1)
            .collect()
    };
    let (first, second) = (points(R_SB_OBJECTS), points(R_SB_OBJECTS_2));
    assert!(!first.is_empty() && !second.is_empty(), "{first:?} {second:?}");
    // Index 0 searches the half north of the centre, index 1 the half south.
    assert!(first.iter().all(|p| p[1] >= MIDDLE_C[1] - 1.0), "{first:?}");
    assert!(second.iter().all(|p| p[1] <= MIDDLE_C[1] + 1.0), "{second:?}");
}

#[test]
fn a_rooted_actor_stays_where_it_is() {
    // A creature that cannot walk (a raven) keeps its place, whatever its package says.
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let start = s.feet(R_TRAVEL);
    s.ai.root(&s.game, FormId(R_TRAVEL));
    s.run(30.0);
    assert_eq!(s.feet(R_TRAVEL), start);
    assert_eq!(s.ai.agents[&FormId(R_TRAVEL)].activity, Activity::None);
    assert!(s.events(R_TRAVEL).is_empty(), "{:?}", s.events(R_TRAVEL));
    assert!(!s.ai.stands(&s.game, FormId(R_TRAVEL)));
}

/// A rooted actor in combat (an Immobile ceiling turret, #1537) stays where it is but
/// turns in place to its combat target; out of combat it keeps its heading. In dialogue
/// it still does not turn (#1543, `talk.rs` a_rooted_actor_talked_to_does_not_turn).
#[test]
fn a_rooted_actor_in_combat_turns_to_its_target_in_place() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL, R_TO_CHAIR]);
    let start = s.feet(R_TRAVEL);
    s.ai.root(&s.game, FormId(R_TRAVEL));
    let target = s.game.world.placed.get(FormId(R_TO_CHAIR)).unwrap().pos;
    let want = fnv_nav::follow::heading_of([target[0] - start[0], target[1] - start[1]]);
    let off = |h: f32| (h - want + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    // Not in combat: it does not turn.
    s.ai.agents.get_mut(&FormId(R_TRAVEL)).unwrap().walker.heading = want + 2.0;
    s.run(2.0);
    assert!((off(s.ai.agents[&FormId(R_TRAVEL)].walker.heading) - 2.0).abs() < 1e-4);
    // In combat: it turns to its target, where it stands.
    s.game.start_combat(FormId(R_TRAVEL), FormId(R_TO_CHAIR));
    assert!(s.game.awareness.state(FormId(R_TRAVEL)).in_combat());
    s.run(5.0);
    let heading = s.ai.agents[&FormId(R_TRAVEL)].walker.heading;
    assert!(off(heading).abs() < 0.05, "{heading} against {want}");
    assert_eq!(s.feet(R_TRAVEL), start);
    assert_eq!(s.ai.agents[&FormId(R_TRAVEL)].activity, Activity::None);
}

/// A rooted actor given a script's UseWeapon (HELIOS One's ARCHIMEDES turret, #2544)
/// uses it from where it stands: the use is in place at once, and it turns to the use's
/// target, out of combat too, where it stays. Before, a rooted actor's use never got in
/// place, so the engine's shooter never fired.
#[test]
fn a_rooted_actor_uses_a_weapon_from_where_it_stands_and_turns_to_the_target() {
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    let mut s = sim(&f, &[R_LONER]);
    let actor = FormId(R_LONER);
    let start = s.feet(R_LONER);
    s.ai.root(&s.game, actor);
    let target = s.game.world.placed.get(FormId(R_HERE)).unwrap().pos;
    let want = fnv_nav::follow::heading_of([target[0] - start[0], target[1] - start[1]]);
    let off = |h: f32| (h - want + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    s.ai.agents.get_mut(&actor).unwrap().walker.heading = want + 2.5;
    let opcode = s.game.data.table.by_name("UseWeapon").unwrap();
    let ctx = fnv_game::functions::Ctx { unit: "test".into(), opcode, on: Some(actor), self_ref: Some(actor), action_ref: None, quest: None, inst: None, in_condition: false };
    let r = |x: u32| ArgValue::Value(Value::Ref(x));
    let n = |x: f64| ArgValue::Value(Value::Num(x));
    // As ARCHIMEDES's: the weapon, standing on itself, the target, one shot, always hit.
    s.game.call_function(&ctx, &[r(STAT), r(R_LONER), r(R_HERE), n(1.0), n(0.0), n(0.0), n(1.0)]);
    assert!(!s.game.awareness.state(actor).in_combat());
    s.run(0.5);
    assert!(s.game.weapon_use(actor).unwrap().in_place, "in place where it stands");
    s.run(5.0);
    let heading = s.ai.agents[&actor].walker.heading;
    assert!(off(heading).abs() < 0.05, "{heading} against {want}");
    assert_eq!(s.feet(R_LONER), start);
}

/// A rooted actor that never turns (an Immobile creature with a Turning Speed of 0, Mr.
/// House's screen: `Ai::fix_heading`) keeps its heading in combat too, where a turret
/// turns to its target (#2150). An actor that is not rooted is not fixed.
#[test]
fn a_fixed_actor_in_combat_keeps_its_heading() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL, R_TO_CHAIR]);
    let start = s.feet(R_TRAVEL);
    s.ai.fix_heading(FormId(R_TRAVEL));
    assert!(!s.ai.agents[&FormId(R_TRAVEL)].fixed, "only a rooted actor is fixed");
    s.ai.root(&s.game, FormId(R_TRAVEL));
    s.ai.fix_heading(FormId(R_TRAVEL));
    assert!(s.ai.agents[&FormId(R_TRAVEL)].fixed);
    let target = s.game.world.placed.get(FormId(R_TO_CHAIR)).unwrap().pos;
    let want = fnv_nav::follow::heading_of([target[0] - start[0], target[1] - start[1]]);
    let away = want + 2.0;
    s.ai.agents.get_mut(&FormId(R_TRAVEL)).unwrap().walker.heading = away;
    let rot = s.game.world.peek(FormId(R_TRAVEL)).map(|r| r.rot);
    s.game.start_combat(FormId(R_TRAVEL), FormId(R_TO_CHAIR));
    assert!(s.game.awareness.state(FormId(R_TRAVEL)).in_combat());
    s.run(5.0);
    assert_eq!(s.ai.agents[&FormId(R_TRAVEL)].walker.heading, away);
    assert_eq!(s.game.world.peek(FormId(R_TRAVEL)).map(|r| r.rot), rot);
    assert_eq!(s.feet(R_TRAVEL), start);
}

// ---- process levels (fnv_ai::process) -------------------------------------------------

#[test]
fn an_actor_out_of_the_loaded_area_walks_on_in_coarse_updates() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    s.run(1.0);
    let actor = FormId(R_TRAVEL);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(s.ai.set_high(&s.game, actor, false));
    assert!(!s.ai.set_high(&s.game, actor, false), "no change");
    assert_eq!(s.ai.process_counts(), (0, 1));
    // It stands between its updates and covers the time owed in one: one move, which
    // takes it all the way (the walk takes about 8 s, an update owes up to 18).
    let mut moves = 0;
    let mut at = s.feet(R_TRAVEL);
    for _ in 0..(2.0 * (fnv_ai::LOW_PROCESS_SECONDS + 1.0)) as usize {
        s.run(0.5);
        if dist(s.feet(R_TRAVEL), at) > 1e-3 {
            moves += 1;
            at = s.feet(R_TRAVEL);
        }
    }
    assert_eq!(moves, 1);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
    assert!(matches!(s.events(R_TRAVEL).last(), Some(AiEvent::Arrived { .. })), "{:?}", s.events(R_TRAVEL));
    assert_eq!(s.game.world.peek(actor).unwrap().cell, Some(FormId(CELL_B)));
    assert!(s.ai.process_stats.low_updates >= 1);
    // Back in the loaded area: every update again.
    assert!(s.ai.set_high(&s.game, actor, true));
    assert_eq!(s.ai.process_counts(), (1, 0));
    assert_eq!((s.ai.process_stats.left, s.ai.process_stats.came_back), (1, 1));
}

#[test]
fn an_actor_out_of_the_loaded_area_searches_no_more_and_takes_up_its_package() {
    let f = fixture();
    let mut s = sim(&f, &[R_RADIUS]);
    s.run(30.0);
    alert(&mut s, R_RADIUS, [150.0, 100.0, 0.0]);
    s.run(1.0);
    assert!(s.ai.agents[&FormId(R_RADIUS)].search.is_some());
    s.ai.set_high(&s.game, FormId(R_RADIUS), false);
    assert!(s.ai.agents[&FormId(R_RADIUS)].search.is_none());
    assert_eq!(s.ai.process_stats.dropped, 1);
    // Its updates keep to its package: no search starts again while it is out.
    s.run(40.0);
    let ev = s.events(R_RADIUS);
    let searches = ev.iter().filter(|e| matches!(e, AiEvent::Search { .. })).count();
    s.run(40.0);
    assert_eq!(s.events(R_RADIUS).iter().filter(|e| matches!(e, AiEvent::Search { .. })).count(), searches);
    assert!(dist(s.feet(R_RADIUS), [350.0, 350.0, 0.0]) <= 150.0);
}

#[test]
fn a_random_conversation_ends_when_one_of_the_two_leaves_the_loaded_area() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL, R_RADIUS]);
    let (a, b) = (FormId(R_TRAVEL), FormId(R_RADIUS));
    s.ai.agents.get_mut(&a).unwrap().chat = Some(fnv_ai::Chat::Going { partner: b, since: 0.0 });
    s.ai.agents.get_mut(&b).unwrap().chat = Some(fnv_ai::Chat::Waiting { starter: a });
    s.ai.set_high(&s.game, a, false);
    assert_eq!(s.ai.agents[&a].chat, None);
    assert_eq!(s.ai.agents[&b].chat, None, "the partner stops waiting");
    // The waiting one leaves instead: the starter finds nobody waiting and gives up.
    s.ai.set_high(&s.game, a, true);
    s.ai.agents.get_mut(&a).unwrap().chat = Some(fnv_ai::Chat::Going { partner: b, since: 0.0 });
    s.ai.agents.get_mut(&b).unwrap().chat = Some(fnv_ai::Chat::Waiting { starter: a });
    s.ai.set_high(&s.game, b, false);
    assert_eq!(s.ai.agents[&b].chat, None);
    assert_eq!(s.ai.process_stats.dropped, 2);
}

#[test]
fn an_actor_with_no_low_level_processing_stands_still_out_of_the_loaded_area() {
    let f = fixture();
    let mut s = sim(&f, &[R_STEADY]);
    s.run(1.0);
    let actor = FormId(R_STEADY);
    assert!(s.game.no_low_level_processing(actor));
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    s.ai.set_high(&s.game, actor, false);
    let at = s.feet(R_STEADY);
    s.run(60.0);
    assert_eq!(s.feet(R_STEADY), at, "no update out of the loaded area");
    // Nor in hours passed in the Wait menu.
    s.game.open_menu(1012);
    assert_eq!(s.ai.pass_time(&mut s.game, &s.nav, 120.0, 0.5), 0);
    assert_eq!(s.feet(R_STEADY), at);
    assert_eq!((s.ai.process_stats.frozen, s.ai.process_stats.low_updates), (1, 0));
    // Back in the loaded area it walks on.
    s.game.close_menu();
    s.ai.set_high(&s.game, actor, true);
    s.run(30.0);
    assert!(dist(s.feet(R_STEADY), [5250.0, 5250.0, 50.0]) < 5.0);
}

#[test]
fn an_actor_out_of_the_cell_buffer_is_updated_every_two_minutes() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    s.run(1.0);
    let actor = FormId(R_TRAVEL);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(s.ai.set_level(&s.game, actor, fnv_ai::ProcessLevel::Low));
    assert!(!s.ai.set_level(&s.game, actor, fnv_ai::ProcessLevel::Low), "no change");
    assert_eq!(s.ai.level(actor), fnv_ai::ProcessLevel::Low);
    assert_eq!((s.ai.process_counts(), s.ai.far_count()), ((0, 1), 1));
    // Its first update comes within the interval (spread by its share of it), and walks
    // it all the way; the next one an interval later.
    let mut waited = 0;
    while s.ai.process_stats.far_updates == 0 {
        s.run(1.0);
        waited += 1;
        assert!(waited <= 121, "an update within {} s", fnv_ai::FAR_PROCESS_SECONDS);
    }
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0, "one update took it there");
    s.run(f64::from(fnv_ai::FAR_PROCESS_SECONDS) - 2.0);
    assert_eq!(s.ai.process_stats.far_updates, 1, "not before {} s", fnv_ai::FAR_PROCESS_SECONDS);
    s.run(3.0);
    assert_eq!((s.ai.process_stats.fell_far, s.ai.process_stats.far_updates), (1, 2));
    assert_eq!(s.ai.process_stats.low_updates, 2, "no Mid-High update in between");
    // Back up to Mid-High, then into the loaded area.
    assert!(s.ai.set_level(&s.game, actor, fnv_ai::ProcessLevel::MidHigh));
    assert_eq!((s.ai.level(actor), s.ai.far_count()), (fnv_ai::ProcessLevel::MidHigh, 0));
    assert!(s.ai.set_level(&s.game, actor, fnv_ai::ProcessLevel::High));
    assert_eq!(s.ai.process_counts(), (1, 0));
}

#[test]
fn a_follower_is_processed_at_least_as_often_as_its_leader() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL, R_DOG]);
    s.run(1.0);
    let (leader, dog) = (FormId(R_TRAVEL), FormId(R_DOG));
    // The leader is in the loaded area: the dog beyond the buffer is kept at Mid-High.
    assert!(s.ai.set_level(&s.game, dog, fnv_ai::ProcessLevel::Low));
    assert_eq!(s.ai.level(dog), fnv_ai::ProcessLevel::MidHigh);
    // The leader beyond the buffer too: so is the dog.
    assert!(s.ai.set_level(&s.game, leader, fnv_ai::ProcessLevel::Low));
    assert!(s.ai.set_level(&s.game, dog, fnv_ai::ProcessLevel::Low));
    assert_eq!(s.ai.level(dog), fnv_ai::ProcessLevel::Low);
}

/// A follower of one in the loaded area (or coming into it) comes in frame by frame from
/// beyond the cell buffer too (`Ai::follows_near_leader`; smoke sunny-wells: Cheyenne out
/// of the saloon after Sunny Smiles); one whose leader is far does not.
#[test]
fn a_follower_of_a_near_leader_comes_in_frame_by_frame() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL, R_DOG]);
    s.run(1.0);
    let (leader, dog) = (FormId(R_TRAVEL), FormId(R_DOG));
    assert!(s.ai.follows_near_leader(dog));
    assert!(!s.ai.follows_near_leader(leader), "the leader follows nobody");
    assert!(s.ai.set_level(&s.game, leader, fnv_ai::ProcessLevel::Coming));
    assert!(s.ai.follows_near_leader(dog));
    assert!(s.ai.set_level(&s.game, leader, fnv_ai::ProcessLevel::Low));
    assert!(!s.ai.follows_near_leader(dog));
}

#[test]
fn an_actor_walking_into_the_loaded_area_is_moved_every_frame() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    s.run(1.0);
    let actor = FormId(R_TRAVEL);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(s.ai.set_level(&s.game, actor, fnv_ai::ProcessLevel::Coming));
    assert_eq!(s.ai.level(actor), fnv_ai::ProcessLevel::Coming);
    assert_eq!(s.ai.process_counts(), (0, 1));
    // Owner's playtest list 4 item 13: it walks on in real time, not in 18 s jumps.
    for _ in 0..5 {
        let at = s.feet(R_TRAVEL);
        s.run(0.5);
        assert!(dist(s.feet(R_TRAVEL), at) > 1.0, "it walked in this half second");
    }
    assert_eq!(s.ai.process_stats.coming, 1);
    assert!(s.ai.process_stats.coming_updates >= 5);
    // Into the loaded area: the high process.
    assert!(s.ai.set_level(&s.game, actor, fnv_ai::ProcessLevel::High));
    assert_eq!((s.ai.level(actor), s.ai.process_counts()), (fnv_ai::ProcessLevel::High, (1, 0)));
}

#[test]
fn a_far_walk_stops_where_it_comes_into_the_loaded_area() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    s.run(1.0);
    let actor = FormId(R_TRAVEL);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    // Its destination's cell is the loaded area; it walks from the other one.
    s.ai.set_loaded(Some(fnv_ai::LoadedArea { space: B, cells: Default::default() }));
    assert!(s.ai.set_level(&s.game, actor, fnv_ai::ProcessLevel::Low));
    let mut waited = 0;
    while s.ai.process_stats.far_updates == 0 {
        s.run(1.0);
        waited += 1;
        assert!(waited <= 121);
    }
    // The original's Malcolm Holmes: the jump that reached the loaded area stopped at its
    // edge [observed 2026-10-24]. The engine takes it on from there in the high process.
    assert_eq!(s.ai.process_stats.reached_loaded, 1);
    assert_eq!(s.ai.agents[&actor].walker.space, B);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling, "not there yet");
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) > 50.0);
    assert!(s.ai.set_level(&s.game, actor, fnv_ai::ProcessLevel::High));
    s.run(30.0);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
}

/// A Use Weapon package (`fnv_game::use_weapon`): the actor walks to the package's
/// location and turns to its target; there its shooting begins, and the package's goal
/// is met only once its shots are spent.
#[test]
fn a_use_weapon_package_stands_at_its_place_and_meets_its_goal_when_its_shots_are_spent() {
    use fnv_game::use_weapon::{After, Source};
    let f = fixture();
    assert!(f.data.issues.is_empty(), "{:?}", f.data.issues.to_json());
    let mut s = sim(&f, &[R_SHOOTER]);
    s.run(15.0);
    let actor = FormId(R_SHOOTER);
    let ev = s.events(R_SHOOTER);
    assert!(matches!(ev[1], AiEvent::Trip { .. }), "{ev:?}");
    assert!(matches!(ev[2], AiEvent::Arrived { .. }), "{ev:?}");
    assert_eq!(s.ai.agents[&actor].activity, Activity::There);
    assert!(dist(s.feet(R_SHOOTER), [350.0, 350.0, 0.0]) < 40.0);
    let w = s.game.weapon_use(actor).expect("a weapon use").clone();
    assert_eq!(w.source, Source::Package(FormId(P_SHOOT)));
    assert_eq!((w.target, w.weapon, w.volleys, w.shots_per_volley, w.pause), (FormId(R_LONER), None, Some(2), [1, 1], Some([0.5, 0.5])));
    assert!(w.always_hit && w.no_damage && !w.crouch_to_reload && !w.hold_fire_when_blocked);
    assert!(w.in_place);
    // Not met on arriving: the shots are still to come.
    assert!(!s.game.extra[&actor].package_goal_met);
    // It faces its target, the loner south of it.
    let feet = s.feet(R_SHOOTER);
    let want = fnv_nav::follow::heading_of([350.0 - feet[0], 50.0 - feet[1]]);
    let have = s.ai.agents[&actor].walker.heading;
    assert!((want - have).sin().abs() < 0.05 && (want - have).cos() > 0.0, "heading {have}, want {want}");
    // Two volleys of a shot each; spent, the goal is met and the use stays, done.
    let u = s.game.weapon_uses.get_mut(&actor).unwrap();
    assert_eq!(u.shot(0.3), After::Pause(0.5));
    assert_eq!(u.shot(0.3), After::Done);
    s.game.end_weapon_use(actor);
    assert!(s.game.extra[&actor].package_goal_met);
    assert!(s.game.weapon_use(actor).is_some_and(|w| w.done));
    assert!(s.game.active_weapon_use(actor).is_none());
}

/// [design, #2652] Authored fixture cells represent two exterior worldspaces; the
/// engine shooter tests separately exercise WNAM decoding into world_parents.
fn weapon_worlds(f: &mut Fixture, sibling: bool) -> Space {
    let (a, b, parent) = (FormId(0xA00), FormId(0xA01), FormId(0xA02));
    f.refs.cell_world.insert(FormId(CELL_A), a);
    f.refs.cell_world.insert(FormId(CELL_B), b);
    f.refs.world_parents.insert(a, parent);
    f.refs.world_parents.insert(b, if sibling { parent } else { FormId(0xA03) });
    Space::World(a.0)
}

/// (#2652; see crates/fnv-ai/src/goal.rs) Only non-actor targets with a shared immediate parent project;
/// [design] ordinary place_of_ref and actor/interior space checks stay unchanged.
#[test]
fn weapon_target_place_projects_only_sibling_objects() {
    let mut f = fixture();
    let space = weapon_worlds(&mut f, true);
    let mut s = sim(&f, &[]);
    let target = FormId(MARK_B);
    let actual = fnv_ai::goal::place_of_ref(&s.game, target).unwrap();
    assert_ne!(actual.space, space);
    assert_eq!(fnv_ai::goal::weapon_target_place(&s.game, target, space), Some(Place { space, pos: actual.pos }));
    assert_eq!(fnv_ai::goal::place_of_ref(&s.game, target), Some(actual));
    assert!(fnv_ai::goal::weapon_target_place(&s.game, target, A).is_none(), "interior shooter");
    assert!(fnv_ai::goal::weapon_target_place(&s.game, FormId(MARK_C), space).is_none(), "interior target");
    s.game.world.get(FormId(R_LONER)).unwrap().cell = Some(FormId(CELL_B));
    assert!(fnv_ai::goal::weapon_target_place(&s.game, FormId(R_LONER), space).is_none(), "actors do not project");
    assert!(fnv_ai::goal::weapon_target_place(&s.game, FormId(R_SHOOTER), space).is_some(), "same-space actor");
}

/// [design, #2652] Standing and rooted UseWeapon users turn to sibling objects, and
/// keep their heading for unrelated-world objects and sibling-world actors.
#[test]
fn weapon_users_face_sibling_objects_but_not_unrelated_objects_or_actors() {
    use fnv_game::use_weapon::WeaponUse;
    for rooted in [false, true] {
        for (sibling, actor_target) in [(true, false), (false, false), (true, true)] {
            let mut f = fixture();
            weapon_worlds(&mut f, sibling);
            let mut s = sim(&f, &[R_LONER]);
            let actor = FormId(R_LONER);
            let target = if actor_target {
                let state = s.game.world.get(FormId(R_SHOOTER)).unwrap();
                state.cell = Some(FormId(CELL_B));
                state.pos = [5250.0, 5250.0, 50.0];
                FormId(R_SHOOTER)
            } else {
                FormId(MARK_B)
            };
            if rooted {
                s.ai.root(&s.game, actor);
            }
            let feet = s.feet(R_LONER);
            let want = fnv_nav::follow::heading_of([5250.0 - feet[0], 5250.0 - feet[1]]);
            let initial = want + 2.5;
            s.ai.agents.get_mut(&actor).unwrap().walker.heading = initial;
            s.game.give_weapon_use(actor, WeaponUse::of_script(None, None, target, 1, false, false, true, true, None));
            s.run(5.0);
            assert!(s.game.weapon_use(actor).unwrap().in_place);
            let expected = if sibling && !actor_target { want } else { initial };
            let heading = s.ai.agents[&actor].walker.heading;
            let off = fnv_nav::follow::heading_delta(heading, expected);
            assert!(off.abs() < 0.05, "rooted {rooted}, sibling {sibling}, actor target {actor_target}: {heading} vs {expected}");
            assert_eq!(s.feet(R_LONER), feet);
        }
    }
}

/// [design, #2652] EvaluatePackage selects immediately after its conditions change;
/// the next high-process AI step starts the use already at its stand, then faces it.
#[test]
fn evaluate_package_starts_sibling_object_weapon_use_when_conditions_pass() {
    use fnv_game::cond::{Cmp, Condition, Operand, RunOn};
    use fnv_game::package::TargetKind;
    use fnv_game::use_weapon::Source;
    use fnv_script::vm::{ArgValue, Value};
    let mut f = fixture();
    weapon_worlds(&mut f, true);
    let global = f.data.table.by_name("GetGlobalValue").unwrap() - 0x1000;
    let dead = f.data.table.by_name("GetDead").unwrap() - 0x1000;
    let def = f.data.package_defs.get_mut(&FormId(P_SHOOT)).unwrap();
    def.conditions = vec![
        Condition { cmp: Cmp::Eq, or: false, run_on: RunOn::Subject, function: global, params: vec![ArgValue::Value(Value::Ref(0x807))], rhs: Operand::Value(1.0) },
        Condition { cmp: Cmp::Eq, or: false, run_on: RunOn::Reference(Some(R_LONER)), function: dead, params: vec![], rhs: Operand::Value(0.0) },
    ];
    def.target2.as_mut().unwrap().kind = TargetKind::Reference(Some(FormId(MARK_B)));
    let mut s = sim(&f, &[R_SHOOTER]);
    let actor = FormId(R_SHOOTER);
    s.game.world.get(FormId(MARK_A)).unwrap().pos = START;
    assert_eq!(s.game.evaluate_package(actor), None, "the authored global starts at zero");
    s.game.world.set_global(FormId(0x807), 1.0).unwrap();
    assert_eq!(s.game.evaluate_package(actor), Some(FormId(P_SHOOT)));
    assert_eq!(s.game.package_def(&FormId(P_SHOOT)).unwrap().ty.runtime_code(), Some(33));
    s.run(5.0);
    let w = s.game.active_weapon_use(actor).expect("high-process AI starts the selected use");
    assert_eq!(w.source, Source::Package(FormId(P_SHOOT)));
    assert_eq!(w.target, FormId(MARK_B));
    assert!(w.in_place);
    let feet = s.feet(R_SHOOTER);
    let want = fnv_nav::follow::heading_of([5250.0 - feet[0], 5250.0 - feet[1]]);
    assert!(fnv_nav::follow::heading_delta(s.ai.agents[&actor].walker.heading, want).abs() < 0.05);
    s.game.extra.entry(FormId(R_LONER)).or_default().dead = true;
    assert_eq!(s.game.evaluate_package(actor), None, "the not-dead condition still applies");
}

/// #2120: a Use Weapon package's fire waits while the actor is in a combat state (Alert,
/// Combat, Lost), unless the package has Continue During Combat: then it goes on, at the
/// package's own target, from its stand. A script's use goes on in every state; a spent
/// use in none.
#[test]
fn a_use_weapon_package_fires_on_in_combat_only_with_continue_during_combat() {
    use fnv_game::package::flags::CONTINUE_DURING_COMBAT;
    use fnv_game::use_weapon::{After, Source, WeaponUse};
    let actor = FormId(R_SHOOTER);
    let stand = [350.0, 350.0, 0.0];
    for continues in [false, true] {
        let mut f = fixture();
        if continues {
            f.data.package_defs.get_mut(&FormId(P_SHOOT)).unwrap().flags |= CONTINUE_DURING_COMBAT;
        }
        let mut s = sim(&f, &[R_SHOOTER]);
        s.run(15.0);
        assert!(dist(s.feet(R_SHOOTER), stand) < 40.0);
        // The first of its two volleys fired: one left.
        assert_eq!(s.game.weapon_uses.get_mut(&actor).unwrap().shot(0.3), After::Pause(0.5));
        for state in [State::Normal, State::Alert, State::Combat, State::Lost] {
            s.game.awareness.actors.entry(actor).or_default().state = state;
            let active = s.game.active_weapon_use(actor).map(|w| (w.source, w.target, w.volleys.unwrap() - w.volleys_done, w.in_place));
            if !state.in_combat() || continues {
                assert_eq!(active, Some((Source::Package(FormId(P_SHOOT)), FormId(R_LONER), 1, true)), "{state:?}, continues {continues}");
            } else {
                assert_eq!(active, None, "{state:?}: the package's fire waits for the fight to end");
                // Held, not lost: the same target and volleys wait.
                let w = s.game.weapon_use(actor).unwrap();
                assert_eq!((w.target, w.volleys.unwrap() - w.volleys_done, w.done), (FormId(R_LONER), 1, false));
            }
        }
        s.game.awareness.actors.remove(&actor);
        if continues {
            // In a real fight with the player (not its package's target), it keeps its
            // stand and its package's target, its fire still on.
            fighting(&mut s, R_SHOOTER, [stand[0] + 300.0, stand[1], 0.0]);
            s.run(2.0);
            assert_eq!(s.game.awareness.state(actor), State::Combat);
            assert!(dist(s.feet(R_SHOOTER), stand) < 40.0, "{:?}", s.events(R_SHOOTER));
            let w = s.game.active_weapon_use(actor).expect("its fire goes on in combat");
            assert_eq!((w.target, w.volleys.unwrap() - w.volleys_done), (FormId(R_LONER), 1));
            // Spent: the goal is met and the use stays, done; the fight takes over.
            assert_eq!(s.game.weapon_uses.get_mut(&actor).unwrap().shot(0.3), After::Done);
            s.game.end_weapon_use(actor);
            assert!(s.game.weapon_use(actor).is_some_and(|w| w.done));
            for state in [State::Normal, State::Alert, State::Combat, State::Lost] {
                s.game.awareness.actors.entry(actor).or_default().state = state;
                assert!(s.game.active_weapon_use(actor).is_none(), "{state:?}: a spent use is over");
            }
        }
        // A script's use goes on in every state, the package's flag or not.
        s.game.give_weapon_use(actor, WeaponUse::of_script(None, None, FormId(R_LONER), 2, true, true, false, false, None));
        for state in [State::Normal, State::Alert, State::Combat, State::Lost] {
            s.game.awareness.actors.entry(actor).or_default().state = state;
            assert!(s.game.active_weapon_use(actor).is_some_and(|w| w.source == Source::Script && w.target == FormId(R_LONER)), "{state:?}");
        }
    }
}

/// #2145 review: a Continue During Combat package's use in Combat faces the package's
/// target while the engine's shooter acts on it; once the fight takes it over
/// (`WeaponUse::taken`: a marker target, one not drawn) the actor turns to its enemy, as a
/// standing fighter does, and back to the package's target when the shooter has it again.
/// It keeps its stand throughout.
#[test]
fn a_use_weapon_package_the_fight_took_over_turns_to_its_enemy() {
    use fnv_game::package::flags::CONTINUE_DURING_COMBAT;
    let actor = FormId(R_SHOOTER);
    let stand = [350.0, 350.0, 0.0];
    let mut f = fixture();
    f.data.package_defs.get_mut(&FormId(P_SHOOT)).unwrap().flags |= CONTINUE_DURING_COMBAT;
    let mut s = sim(&f, &[R_SHOOTER]);
    s.run(15.0);
    assert!(dist(s.feet(R_SHOOTER), stand) < 40.0);
    let enemy = [stand[0] + 300.0, stand[1], 0.0];
    fighting(&mut s, R_SHOOTER, enemy);
    let faces = |s: &Sim, to: [f32; 2]| {
        let feet = s.feet(R_SHOOTER);
        let want = fnv_nav::follow::heading_of([to[0] - feet[0], to[1] - feet[1]]);
        let have = s.ai.agents[&actor].walker.heading;
        (want - have).sin().abs() < 0.1 && (want - have).cos() > 0.0
    };
    // The shooter has it: the package's target, the loner south of it.
    s.run(2.0);
    assert_eq!(s.game.awareness.state(actor), State::Combat);
    assert!(s.game.awareness.get(actor).and_then(|a| a.combat_target()).is_some());
    assert!(faces(&s, [350.0, 50.0]), "the package's target, heading {}", s.ai.agents[&actor].walker.heading);
    // The fight took it over (set each frame by the engine): its enemy, to the east.
    for _ in 0..4 {
        s.game.weapon_use_taken(actor);
        s.run(0.5);
    }
    assert!(faces(&s, [enemy[0], enemy[1]]), "its enemy, heading {}", s.ai.agents[&actor].walker.heading);
    assert!(dist(s.feet(R_SHOOTER), stand) < 40.0, "it keeps its stand");
    // The shooter has it again: back to the package's target.
    s.game.weapon_uses_untaken();
    s.run(2.0);
    assert!(faces(&s, [350.0, 50.0]), "the package's target again, heading {}", s.ai.agents[&actor].walker.heading);
}

/// The UseWeapon function (`fnv_game::use_weapon`): the actor walks to where it is to
/// stand, and once the use is over goes back to its package.
#[test]
fn use_weapon_sends_the_actor_to_its_place_and_back_to_its_package_after() {
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    let mut s = sim(&f, &[R_LONER]);
    s.run(1.0);
    let actor = FormId(R_LONER);
    let opcode = s.game.data.table.by_name("UseWeapon").unwrap();
    let ctx = fnv_game::functions::Ctx { unit: "test".into(), opcode, on: Some(actor), self_ref: Some(actor), action_ref: None, quest: None, inst: None, in_condition: false };
    let r = |x: u32| ArgValue::Value(Value::Ref(x));
    let n = |x: f64| ArgValue::Value(Value::Num(x));
    s.game.call_function(&ctx, &[r(STAT), r(MARK_A), r(R_HERE), n(4.0), n(1.0), n(0.0), n(1.0)]);
    let w = s.game.weapon_use(actor).expect("a weapon use").clone();
    assert_eq!((w.stand, w.target, w.volleys, w.shots_per_volley), (Some(FormId(MARK_A)), FormId(R_HERE), Some(1), [4, 4]));
    // The defaults of the arguments left out: crouch to reload and hold fire on unless
    // given, no damage off [documented: GECK wiki UseWeapon].
    assert!(w.crouch_to_reload && !w.hold_fire_when_blocked && w.always_hit && !w.no_damage && w.target_here.is_none());
    s.run(10.0);
    let ev = s.events(R_LONER);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::WeaponUse { target, length } if target.0 == R_HERE && *length > 250.0)), "{ev:?}");
    assert!(dist(s.feet(R_LONER), [350.0, 350.0, 0.0]) < 40.0);
    assert!(s.game.weapon_use(actor).unwrap().in_place);
    // Over: it goes back to where it was placed (it has no package).
    s.game.end_weapon_use(actor);
    assert!(s.game.weapon_use(actor).is_none());
    s.run(10.0);
    let ev = s.events(R_LONER);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::WeaponUseOver)), "{ev:?}");
}

/// A melee use of UseWeapon (docs/design/use-weapon.md §3.1): standing on its own
/// reference, the actor closes in on its target within the reach the engine set, stops
/// there, and gives the closing in up when the use is over.
#[test]
fn a_melee_use_closes_in_on_its_target_within_its_reach() {
    use fnv_script::vm::{ArgValue, Value};
    let f = fixture();
    let mut s = sim(&f, &[R_LONER]);
    s.run(1.0);
    let actor = FormId(R_LONER);
    let opcode = s.game.data.table.by_name("UseWeapon").unwrap();
    let ctx = fnv_game::functions::Ctx { unit: "test".into(), opcode, on: Some(actor), self_ref: Some(actor), action_ref: None, quest: None, inst: None, in_condition: false };
    let r = |x: u32| ArgValue::Value(Value::Ref(x));
    let n = |x: f64| ArgValue::Value(Value::Num(x));
    // Standing on its own reference: in place at once, 300 units from its target.
    s.game.call_function(&ctx, &[r(STAT), r(R_LONER), r(R_HERE), n(2.0)]);
    s.run(0.5);
    assert!(s.game.weapon_use(actor).unwrap().in_place);
    assert_eq!(s.feet(R_LONER), [350.0, 50.0, 0.0], "a gun's use stays where it is");
    // The engine finds a melee weapon: a reach to close within.
    s.game.weapon_uses.get_mut(&actor).unwrap().reach = Some([70.0, 100.0]);
    s.run(10.0);
    let away = dist(s.feet(R_LONER), START);
    assert!(away <= 100.0 && away > 20.0, "{away}");
    let ev = s.events(R_LONER);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Close { target, .. } if target.0 == R_HERE)), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Hold { target, .. } if target.0 == R_HERE)), "{ev:?}");
    // Over: the closing in ends with it.
    s.game.end_weapon_use(actor);
    s.run(1.0);
    assert!(s.ai.agents[&actor].search.is_none());
    assert!(s.events(R_LONER).iter().any(|e| matches!(e, AiEvent::WeaponUseOver)));
}

/// DisableNavmesh while an actor walks a way over that navmesh: it plans again, finds no
/// way and sets the package aside; enabled again, the way is there (docs/design/
/// script-stubs.md §3.5). The switches are applied once per change.
#[test]
fn a_navmesh_disabled_under_a_walk_sends_the_actor_to_plan_again() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    s.run(1.0);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert_eq!(s.game.extra[&actor].ai_phase, Some(AiPhase::Travelling));
    assert_eq!(s.ai.apply_navmesh_switches(&mut s.game, &mut s.nav), None);
    // Cell B's navmesh goes: the way through the door ends on it.
    s.game.nav_switches.navmeshes.insert(FormId(2), false);
    assert_eq!(s.ai.apply_navmesh_switches(&mut s.game, &mut s.nav), Some(vec![actor]));
    assert_eq!(s.ai.apply_navmesh_switches(&mut s.game, &mut s.nav), None, "applied once");
    s.run(5.0);
    assert!(s.events(R_TRAVEL).iter().any(|e| matches!(e, AiEvent::Unreachable { package, .. } if package.0 == P_TRAVEL_B)), "{:?}", s.events(R_TRAVEL));
    s.run(30.0);
    assert_ne!(s.game.world.peek(actor).unwrap().cell, Some(FormId(CELL_B)));
    // Enabled again: chosen afresh, it gets there.
    s.game.nav_switches.navmeshes.insert(FormId(2), true);
    s.ai.apply_navmesh_switches(&mut s.game, &mut s.nav);
    s.game.extra.get_mut(&actor).unwrap().package_blocked.clear();
    s.game.choose_packages_now();
    s.run(40.0);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
    assert!(s.game.extra[&actor].package_goal_met);
    assert_eq!(s.game.extra[&actor].ai_phase, Some(AiPhase::There));
}

/// What a sandboxer does is told to the game for GetCurrentAIProcedure
/// (`fnv_game::ai::SandboxPhase`; docs/spec-notes/packages.md §4, procedure codes): walking to the
/// chair or marker, sitting, at the marker, and up from the chair until the next thing.
#[test]
fn a_sandboxer_tells_the_game_what_it_does() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    let actor = FormId(R_SB_OBJECTS);
    let mut seen = Vec::new();
    for _ in 0..1200 {
        s.run(0.5);
        let Some(AiPhase::Sandbox(p)) = s.game.extra.get(&actor).and_then(|e| e.ai_phase) else { continue };
        if seen.last() != Some(&p) {
            seen.push(p);
        }
        let a = &s.ai.agents[&actor];
        match p {
            SandboxPhase::Sitting => assert!(matches!(a.sandbox.as_ref().unwrap().doing, Doing::Sitting(_))),
            SandboxPhase::ToObject => assert_eq!(a.activity, Activity::Travelling),
            _ => {}
        }
    }
    for p in [SandboxPhase::ToObject, SandboxPhase::Sitting, SandboxPhase::AtIdleMarker, SandboxPhase::GotUp] {
        assert!(seen.contains(&p), "{p:?} not in {seen:?}");
    }
    // Up from the chair, then (after walking to it) the idle marker: never the chair again
    // straight from getting up.
    for w in seen.windows(2) {
        if w[0] == SandboxPhase::GotUp {
            assert_ne!(w[1], SandboxPhase::Sitting, "{seen:?}");
        }
    }
}

// ---- the drawn walk (#2124, `fnv_ai::carry::drawn_walk_share`) --------------------

/// Body armour of the medium class (upper body), for the drawn walk's armour rule.
const MEDIUM_ARMOUR: u32 = 0xA05;

/// [`with_arms`] with [`MEDIUM_ARMOUR`].
fn with_arms_and_armour() -> Fixture {
    use fnv_sim::forms::{ArmorClass, BaseData, BaseForm, ItemKind};
    let mut f = with_arms(fixture());
    let BaseData::Item(mut it) = f.forms.bases[&FormId(PISTOL)].data.clone() else { unreachable!("the pistol is an item") };
    (it.kind, it.slots, it.armor_class, it.weapon_traits, it.drawable, it.projectile) = (ItemKind::Armor, Some(fnv_game::detection::UPPER_BODY), ArmorClass::Medium, None, false, None);
    let id = FormId(MEDIUM_ARMOUR);
    f.forms.sigs.insert(id, *b"ARMO");
    f.forms.bases.insert(id, BaseForm { id, sig: *b"ARMO", edid: None, name: None, script: None, prompt: None, voice_filter: false, data: BaseData::Item(it) });
    f
}

/// `actor` holds `weapon` (outfitted, as a drawn actor is) and has it out as SetWeaponOut
/// 1 holds it (`Game::wants_weapon_out`).
fn draw(s: &mut Sim, actor: u32, weapon: u32) {
    let me = FormId(actor);
    s.game.world.add_item(me, FormId(weapon), 1).unwrap();
    s.game.world.outfit(me).unwrap();
    s.game.world.equip(me, FormId(weapon)).unwrap();
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(weapon)));
    s.game.hud.forced_out.insert(me);
}

fn walk_speed(s: &Sim, actor: u32) -> f32 {
    s.ai.agents[&FormId(actor)].walker.speed
}

#[test]
fn a_person_with_a_pistol_drawn_walks_its_package_at_its_walk_over_fmovenoweaponmult() {
    let f = with_arms(fixture());
    let mut drawn = sim(&f, &[R_RADIUS]);
    draw(&mut drawn, R_RADIUS, PISTOL);
    let mut holstered = sim(&f, &[R_RADIUS]);
    drawn.run(2.0);
    holstered.run(2.0);
    // 100 (the test's walk) / 1.1 (fMoveNoWeaponMult's default).
    assert!((walk_speed(&drawn, R_RADIUS) - 100.0 / 1.1).abs() < 1e-3, "{}", walk_speed(&drawn, R_RADIUS));
    assert_eq!(walk_speed(&holstered, R_RADIUS), 100.0);
    // Both still on their way, the holstered one 1.1 times as far.
    let gone = |s: &Sim| {
        assert_eq!(s.ai.agents[&FormId(R_RADIUS)].activity, Activity::Travelling);
        s.ai.agents[&FormId(R_RADIUS)].walker.travelled
    };
    let ratio = gone(&holstered) / gone(&drawn);
    assert!((1.07..=1.13).contains(&ratio), "{ratio}");
}

#[test]
fn a_walk_under_way_follows_the_weapon_drawn_and_put_away() {
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_RADIUS]);
    let me = FormId(R_RADIUS);
    s.game.world.add_item(me, FormId(PISTOL), 1).unwrap();
    s.game.world.outfit(me).unwrap();
    s.game.world.equip(me, FormId(PISTOL)).unwrap();
    s.run(1.0);
    assert_eq!(s.ai.agents[&me].activity, Activity::Travelling);
    assert_eq!(walk_speed(&s, R_RADIUS), 100.0);
    // The animating side tells the weapon out once it is drawn, after the walk was planned.
    let frame = |s: &mut Sim, out: bool| {
        s.game.set_weapon_out(me, out);
        s.game.tick();
        s.ai.update(&mut s.game, &s.nav, 0.5);
    };
    frame(&mut s, true);
    assert!((walk_speed(&s, R_RADIUS) - 100.0 / 1.1).abs() < 1e-3, "{}", walk_speed(&s, R_RADIUS));
    frame(&mut s, false);
    assert_eq!(walk_speed(&s, R_RADIUS), 100.0);
}

#[test]
fn the_drawn_walk_keeps_speed_mult_sneaking_and_the_setting() {
    // Speed Mult 120 (the engine's people's speeds, `creature_pace::paced_speeds`: the
    // walk × Speed Mult × the armour's share).
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_RADIUS]);
    let own = fnv_ai::Speeds { walk: 120.0, run: 480.0 };
    s.ai.set_speeds(FormId(R_RADIUS), fnv_ai::ActorSpeeds { calm: own, alert: own, turn_rate: None, spot_turn_start: None });
    draw(&mut s, R_RADIUS, PISTOL);
    s.run(1.0);
    assert!((walk_speed(&s, R_RADIUS) - 120.0 / 1.1).abs() < 1e-3, "{}", walk_speed(&s, R_RADIUS));
    // Sneaking: × fMoveSneakMult (0.57) on top.
    let mut s = sim(&f, &[R_RADIUS]);
    draw(&mut s, R_RADIUS, PISTOL);
    s.game.extra.entry(FormId(R_RADIUS)).or_default().force_sneak = true;
    s.run(1.0);
    assert!((walk_speed(&s, R_RADIUS) - 100.0 / 1.1 * 0.57).abs() < 1e-3, "{}", walk_speed(&s, R_RADIUS));
    // The load order's fMoveNoWeaponMult.
    let mut f = with_arms(fixture());
    f.forms.settings.insert("fmovenoweaponmult".into(), fnv_sim::settings::Setting::Float(1.25));
    let mut s = sim(&f, &[R_RADIUS]);
    draw(&mut s, R_RADIUS, PISTOL);
    s.run(1.0);
    assert!((walk_speed(&s, R_RADIUS) - 80.0).abs() < 1e-3, "{}", walk_speed(&s, R_RADIUS));
}

#[test]
fn a_pistol_taken_off_on_the_way_gives_the_unarmed_walk_back() {
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_RADIUS]);
    let me = FormId(R_RADIUS);
    draw(&mut s, R_RADIUS, PISTOL);
    s.run(1.0);
    assert!((walk_speed(&s, R_RADIUS) - 100.0 / 1.1).abs() < 1e-3, "{}", walk_speed(&s, R_RADIUS));
    // Back to its fists, SetWeaponOut 1 still on; the animating side's last drawn weapon
    // (not cleared on unequipping) does not count against its inventory.
    s.game.set_held_weapon(me, Some(FormId(PISTOL)));
    s.game.world.unequip(me, FormId(PISTOL)).unwrap();
    assert_eq!(s.game.held_weapon(me), None);
    s.run(0.5);
    assert_eq!(s.ai.agents[&me].activity, Activity::Travelling);
    assert_eq!(walk_speed(&s, R_RADIUS), 100.0);
}

#[test]
fn a_walk_whose_base_speed_changed_on_the_way_still_follows_the_weapon() {
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_RADIUS]);
    let me = FormId(R_RADIUS);
    s.game.world.add_item(me, FormId(PISTOL), 1).unwrap();
    s.game.world.outfit(me).unwrap();
    s.game.world.equip(me, FormId(PISTOL)).unwrap();
    s.run(1.0);
    assert_eq!(walk_speed(&s, R_RADIUS), 100.0);
    let planned = plans(&s, R_RADIUS);
    // New speeds of its own on the way (armour, Speed Mult): the walk takes them at once
    // (`Ai::set_speeds`, issue #2118) and stays the package's own.
    s.ai.set_speeds(me, speeds(120.0, 480.0));
    assert_eq!(walk_speed(&s, R_RADIUS), 120.0);
    s.run(0.5);
    assert_eq!(walk_speed(&s, R_RADIUS), 120.0);
    // Drawn, then put away: the walk follows from its new speed.
    s.game.hud.forced_out.insert(me);
    s.run(0.5);
    assert_eq!(s.ai.agents[&me].activity, Activity::Travelling);
    assert!((walk_speed(&s, R_RADIUS) - 120.0 / 1.1).abs() < 1e-3, "{}", walk_speed(&s, R_RADIUS));
    s.game.hud.forced_out.remove(&me);
    s.run(0.5);
    assert!((walk_speed(&s, R_RADIUS) - 120.0).abs() < 1e-3, "{}", walk_speed(&s, R_RADIUS));
    assert_eq!(plans(&s, R_RADIUS), planned, "{:?}", s.events(R_RADIUS));
}

/// A Speed Mult refresh during a drawn walk (`Ai::set_speeds` rewriting the walk under
/// way, #2118) keeps the drawn share and the walk's ownership: the walk goes on at the new
/// walk / fMoveNoWeaponMult and still follows the weapon, also through a stop at Speed
/// Mult 0 in which the weapon is put away.
#[test]
fn a_speed_mult_refresh_during_a_drawn_walk_keeps_the_share() {
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_RADIUS]);
    let me = FormId(R_RADIUS);
    draw(&mut s, R_RADIUS, PISTOL);
    s.run(1.0);
    let near = |s: &Sim, want: f32| assert!((walk_speed(s, R_RADIUS) - want).abs() < 1e-3, "{} against {want}", walk_speed(s, R_RADIUS));
    near(&s, 100.0 / 1.1);
    let planned = plans(&s, R_RADIUS);
    // Speed Mult 120: at once, and kept on the next steps (not given up as another's).
    s.ai.set_speeds(me, speeds(120.0, 480.0));
    near(&s, 120.0 / 1.1);
    s.run(0.5);
    near(&s, 120.0 / 1.1);
    // Put away and drawn again: the walk is still the package's own.
    s.game.hud.forced_out.remove(&me);
    s.run(0.5);
    near(&s, 120.0);
    s.game.hud.forced_out.insert(me);
    s.run(0.5);
    near(&s, 120.0 / 1.1);
    // Speed Mult 0: it stands; back to 120, the drawn walk again.
    s.ai.set_speeds(me, speeds(0.0, 0.0));
    s.run(0.5);
    near(&s, 0.0);
    s.ai.set_speeds(me, speeds(120.0, 480.0));
    near(&s, 120.0 / 1.1);
    // Standing again, put away while it stands: it goes on at the holstered walk, and
    // drawn after at the drawn one.
    s.ai.set_speeds(me, speeds(0.0, 0.0));
    s.game.hud.forced_out.remove(&me);
    s.run(0.5);
    near(&s, 0.0);
    s.ai.set_speeds(me, speeds(120.0, 480.0));
    near(&s, 120.0);
    s.game.hud.forced_out.insert(me);
    s.run(0.5);
    near(&s, 120.0 / 1.1);
    assert_eq!(s.ai.agents[&me].activity, Activity::Travelling);
    // All on the one walk: no new plan gave it its speeds.
    assert_eq!(plans(&s, R_RADIUS), planned, "{:?}", s.events(R_RADIUS));
}

/// A package's walk planned with the pistol drawn while the speeds are 0 goes on at the
/// drawn walk when they come back (`Ai::plan` keeps the share with the stop's gait).
#[test]
fn a_drawn_walk_planned_at_speed_0_goes_on_drawn() {
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_RADIUS]);
    let me = FormId(R_RADIUS);
    s.ai.set_speeds(me, speeds(0.0, 0.0));
    draw(&mut s, R_RADIUS, PISTOL);
    s.run(1.0);
    assert_eq!(s.ai.agents[&me].activity, Activity::Travelling);
    assert_eq!(walk_speed(&s, R_RADIUS), 0.0);
    s.ai.set_speeds(me, speeds(120.0, 480.0));
    assert!((walk_speed(&s, R_RADIUS) - 120.0 / 1.1).abs() < 1e-3, "{}", walk_speed(&s, R_RADIUS));
    s.game.hud.forced_out.remove(&me);
    s.run(0.5);
    assert!((walk_speed(&s, R_RADIUS) - 120.0).abs() < 1e-3, "{}", walk_speed(&s, R_RADIUS));
}

#[test]
fn a_walk_another_owner_takes_over_with_the_same_speed_is_left_alone() {
    // A wanderer with its pistol out: its sandbox's walk is the drawn walk.
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_SB_WANDER, R_SB_NOTHING]);
    let me = FormId(R_SB_WANDER);
    draw(&mut s, R_SB_WANDER, PISTOL);
    let drawn = 100.0 / 1.1;
    let mut owned = false;
    for _ in 0..120 {
        s.run(0.5);
        let a = &s.ai.agents[&me];
        if a.activity == Activity::Travelling && (a.walker.speed - drawn).abs() < 1e-3 {
            owned = true;
            break;
        }
    }
    assert!(owned, "the wanderer never walked with its pistol drawn: {:?}", doings(&s, R_SB_WANDER));
    // In the same frame a random conversation takes the walk over at the speed it has
    // (as `social`'s set-off would, the walk's end and the next walk in one frame).
    let agent = s.ai.agents.get_mut(&me).unwrap();
    agent.walker.go_to(&s.nav, fnv_nav::path::Place { space: C, pos: [10750.0, 10450.0, 0.0] }, &fnv_nav::path::PathParams::default()).unwrap();
    agent.activity = Activity::Travelling;
    agent.chat = Some(fnv_ai::Chat::Going { partner: FormId(R_SB_NOTHING), since: s.game.time });
    // Put away: the conversation's walk keeps its speed.
    s.game.hud.forced_out.remove(&me);
    s.run(0.5);
    assert_eq!(s.ai.agents[&me].activity, Activity::Travelling);
    assert!((walk_speed(&s, R_SB_WANDER) - drawn).abs() < 1e-3, "{}", walk_speed(&s, R_SB_WANDER));
}

#[test]
fn the_drawn_walk_leaves_runs_other_stances_armour_creatures_and_fights_alone() {
    let f = with_arms_and_armour();
    // A run with the pistol drawn keeps the run.
    let mut s = sim(&f, &[R_RUN]);
    draw(&mut s, R_RUN, PISTOL);
    s.run(0.5);
    assert_eq!(walk_speed(&s, R_RUN), 400.0);
    // A knife (Melee 1 Hand, not measured) keeps the holstered walk.
    let mut s = sim(&f, &[R_RADIUS]);
    draw(&mut s, R_RADIUS, KNIFE);
    s.run(1.0);
    assert_eq!(walk_speed(&s, R_RADIUS), 100.0);
    // So does the pistol in medium armour (not measured drawn).
    let mut s = sim(&f, &[R_RADIUS]);
    s.game.world.add_item(FormId(R_RADIUS), FormId(MEDIUM_ARMOUR), 1).unwrap();
    draw(&mut s, R_RADIUS, PISTOL);
    s.game.world.equip(FormId(R_RADIUS), FormId(MEDIUM_ARMOUR)).unwrap();
    s.run(1.0);
    assert_eq!(walk_speed(&s, R_RADIUS), 100.0);
    // A creature given a pistol to hold, and a person fighting.
    let mut s = sim(&f, &[R_HOUND, R_SB_NOTHING]);
    s.game.set_held_weapon(FormId(R_HOUND), Some(FormId(PISTOL)));
    s.game.hud.forced_out.insert(FormId(R_HOUND));
    assert!(s.game.weapon_is_out(FormId(R_HOUND)));
    assert_eq!(fnv_ai::carry::drawn_walk_share(&s.game, FormId(R_HOUND)), 1.0);
    draw(&mut s, R_SB_NOTHING, PISTOL);
    assert!(fnv_ai::carry::drawn_walk_share(&s.game, FormId(R_SB_NOTHING)) < 1.0);
    fighting(&mut s, R_SB_NOTHING, [10450.0, 10950.0, 0.0]);
    assert_eq!(fnv_ai::carry::drawn_walk_share(&s.game, FormId(R_SB_NOTHING)), 1.0);
}

// ---- re-arming (`fnv_ai::rearm`, docs/design/combat-ai.md §22) ----------------------

const PISTOL: u32 = 0xA00;
const ROUNDS: u32 = 0xA01;
const KNIFE: u32 = 0xA02;
const FLAMER: u32 = 0xA03;
const INCINERATOR: u32 = 0xA04;

/// A pistol (Guns, 10 a shot, 2 a second, firing `ROUNDS`), its rounds and a knife (Melee,
/// 60 a blow): the knife does more damage a second than the pistol.
fn with_arms(mut f: Fixture) -> Fixture {
    use fnv_sim::forms::{BaseData, BaseForm, Item, ItemKind, WeaponTraits};
    let item = |kind: ItemKind, damage: Option<f32>, traits: Option<WeaponTraits>, projectile: Option<FormId>| Item {
        kind,
        value: Some(1),
        money: None,
        weight: Some(1.0),
        slots: None,
        projectile,
        health: Some(100),
        non_playable: false,
        cant_drop: false,
        armor_class: Default::default(),
        power_armor: false,
        armor_dr: None,
        armor_dt: None,
        weapon_damage: damage,
        weight_mods: [0.0; 3],
        drawable: kind == ItemKind::Weapon,
        not_for_npcs: false,
        thrown: false,
        quest_item: false,
        icon: None,
        placed_count: None,
        teaches: None,
        cant_be_taken: false,
        modulates_voice: false,
        weapon_traits: traits,
    };
    let pistol = item(ItemKind::Weapon, Some(10.0), Some(WeaponTraits { skill: Some(41), shots_per_sec: 2.0, ammo: Some(FormId(ROUNDS)), ..Default::default() }), Some(FormId(0xA0F)));
    let knife = item(ItemKind::Weapon, Some(60.0), Some(WeaponTraits { skill: Some(38), attack_mult: 1.0, ..Default::default() }), None);
    let rounds = item(ItemKind::Ammo, None, None, None);
    // A flamer (a flame reaching 640 units, 15 a shot at 10 a second) and an incinerator
    // (globs exploding 150 units round, 21 a shot once a second), both on `ROUNDS`.
    let flamer = item(ItemKind::Weapon, Some(15.0), Some(WeaponTraits { skill: Some(33), automatic: true, fire_rate: 10.0, ammo: Some(FormId(ROUNDS)), max_range: 640.0, flame: true, ..Default::default() }), Some(FormId(0xA0F)));
    let incinerator = item(ItemKind::Weapon, Some(21.0), Some(WeaponTraits { skill: Some(33), shots_per_sec: 1.0, ammo: Some(FormId(ROUNDS)), max_range: 3000.0, blast: 150.0, ..Default::default() }), Some(FormId(0xA0F)));
    // Animation types Pistol - Ballistic (1 Hand) 3, Melee (1 Hand) 1 and Rifle - Automatic
    // (2 Hand) 6 [documented: xEdit].
    f.data.weapon_anim_types.extend([(FormId(PISTOL), 3), (FormId(KNIFE), 1), (FormId(FLAMER), 6), (FormId(INCINERATOR), 6)]);
    for (id, sig, it) in [(PISTOL, *b"WEAP", pistol), (KNIFE, *b"WEAP", knife), (ROUNDS, *b"AMMO", rounds), (FLAMER, *b"WEAP", flamer), (INCINERATOR, *b"WEAP", incinerator)] {
        let id = FormId(id);
        f.forms.sigs.insert(id, sig);
        f.forms.bases.insert(id, BaseForm { id, sig, edid: None, name: None, script: None, prompt: None, voice_filter: false, data: BaseData::Item(it) });
    }
    f
}

/// Lay `base` on the ground of cell C at `at`.
fn lay(s: &mut Sim, base: u32, at: [f32; 3]) -> FormId {
    s.game.world.place(FormId(base), Some(FormId(CELL_C)), at, [0.0; 3], 1).unwrap()
}

#[test]
fn an_unarmed_fighter_fetches_a_gun_and_a_copy_of_its_ammunition() {
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let feet = s.feet(R_SB_NOTHING);
    let gun = lay(&mut s, PISTOL, [feet[0] + 250.0, feet[1], 0.0]);
    let box_of_rounds = lay(&mut s, ROUNDS, [feet[0] + 120.0, feet[1] + 60.0, 0.0]);
    fighting(&mut s, R_SB_NOTHING, [10450.0, 10950.0, 0.0]);
    let me = FormId(R_SB_NOTHING);
    assert!(s.game.held_weapon(me).is_none(), "it starts with its fists");
    s.run(8.0);
    let ev = s.events(R_SB_NOTHING);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Fetch { weapon, stops: 2, .. } if *weapon == FormId(PISTOL))), "{ev:?}");
    // The ammunition first (nearer), copied; then the gun, picked up.
    let got: Vec<FormId> = ev.iter().filter_map(|e| if let AiEvent::FetchGot { item, got: true, .. } = e { Some(*item) } else { None }).collect();
    assert_eq!(got, vec![FormId(ROUNDS), FormId(PISTOL)], "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Rearmed { weapon, .. } if *weapon == FormId(PISTOL))), "{ev:?}");
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(PISTOL)));
    assert!(!s.game.present(gun), "the gun left the ground");
    assert!(s.game.present(box_of_rounds), "the source of the copied rounds stays");
    assert!(s.game.carries_any(me, &[FormId(ROUNDS)]));
    assert!(!s.game.rearms.fetches.contains_key(&me));
}

/// A placed gun with the No AI Acquire flag (#962) is left where it lies: the fighter
/// goes past it for one without the flag.
#[test]
fn a_fighter_leaves_a_gun_flagged_no_ai_acquire() {
    let mut f = with_arms(fixture());
    let me = FormId(R_SB_NOTHING);
    let home = f.refs.refs[&me].pos;
    let template = f.refs.by_cell[&FormId(CELL_C)].iter().map(|r| &f.refs.refs[r]).find(|p| &p.sig == b"REFR").expect("a REFR in cell C").clone();
    let flagged = FormId(0xA10);
    let mut gun = template;
    gun.id = flagged;
    gun.base = FormId(PISTOL);
    gun.edid = None;
    gun.pos = [home[0] + 150.0, home[1], home[2]];
    gun.flags |= fnv_sim::refs::FLAG_NO_AI_ACQUIRE;
    gun.lock = None;
    gun.owner = None;
    gun.health = None;
    gun.count = None;
    gun.enable_parent = None;
    f.refs.refs.insert(flagged, gun);
    f.refs.by_cell.get_mut(&FormId(CELL_C)).unwrap().push(flagged);
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let feet = s.feet(R_SB_NOTHING);
    // Lay it beside the fighter, nearer than the plain gun, and touch it so the search
    // sees it as a weapon on the ground.
    s.game.world.get(flagged).unwrap().pos = [feet[0] + 150.0, feet[1], 0.0];
    let plain = lay(&mut s, PISTOL, [feet[0] - 300.0, feet[1], 0.0]);
    lay(&mut s, ROUNDS, [feet[0], feet[1] + 40.0, 0.0]);
    fighting(&mut s, R_SB_NOTHING, [10450.0, 10950.0, 0.0]);
    s.run(10.0);
    let ev = s.events(R_SB_NOTHING);
    let from: Vec<FormId> = ev.iter().filter_map(|e| if let AiEvent::FetchGot { from, .. } = e { Some(*from) } else { None }).collect();
    assert!(!from.contains(&flagged), "{ev:?}");
    assert!(s.game.present(flagged), "the flagged gun stays on the ground");
    assert!(!s.game.present(plain), "the plain gun is taken instead: {ev:?}");
}

#[test]
fn a_gunman_takes_a_weak_gun_over_a_strong_knife_and_the_knife_only_without_ammunition() {
    let f = with_arms(fixture());
    let me = FormId(R_SB_NOTHING);
    // With rounds for the pistol on the ground: the pistol (its own class first).
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let feet = s.feet(R_SB_NOTHING);
    lay(&mut s, KNIFE, [feet[0] + 100.0, feet[1], 0.0]);
    lay(&mut s, PISTOL, [feet[0] + 200.0, feet[1], 0.0]);
    lay(&mut s, ROUNDS, [feet[0] + 200.0, feet[1] + 80.0, 0.0]);
    fighting(&mut s, R_SB_NOTHING, [10450.0, 10950.0, 0.0]);
    s.run(8.0);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(PISTOL)), "{:?}", s.events(R_SB_NOTHING));
    // No rounds anywhere: the pistol is no use, so the knife, before the fists.
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let feet = s.feet(R_SB_NOTHING);
    lay(&mut s, KNIFE, [feet[0] + 100.0, feet[1], 0.0]);
    let gun = lay(&mut s, PISTOL, [feet[0] + 200.0, feet[1], 0.0]);
    fighting(&mut s, R_SB_NOTHING, [10450.0, 10950.0, 0.0]);
    s.run(8.0);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(KNIFE)), "{:?} {:?}", s.events(R_SB_NOTHING), s.game.world.peek(me).and_then(|r| r.inventory.clone()));
    assert!(s.game.present(gun), "a gun without ammunition is left lying");
}

#[test]
fn a_gun_without_ammunition_anywhere_is_not_fetched() {
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let feet = s.feet(R_SB_NOTHING);
    lay(&mut s, PISTOL, [feet[0] + 200.0, feet[1], 0.0]);
    fighting(&mut s, R_SB_NOTHING, [10450.0, 10950.0, 0.0]);
    s.run(4.0);
    let ev = s.events(R_SB_NOTHING);
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::Fetch { .. })), "{ev:?}");
    assert!(s.game.held_weapon(FormId(R_SB_NOTHING)).is_none());
}

#[test]
fn a_fighter_keeps_the_weapon_it_holds_over_its_other_carried_ones_until_it_loses_it() {
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let me = FormId(R_SB_NOTHING);
    // The outfitting holds the knife (the higher damage); a gunman would rank the pistol
    // above it, but the outfitting's choice stands (Cook-Cook's Incinerator).
    for (item, n) in [(KNIFE, 1), (PISTOL, 1), (ROUNDS, 5)] {
        s.game.world.add_item(me, FormId(item), n).unwrap();
    }
    // Outfitted, as a drawn actor is: what it holds is what is equipped.
    assert!(s.game.world.outfit(me).unwrap());
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(KNIFE)));
    fighting(&mut s, R_SB_NOTHING, [10450.0, 10950.0, 0.0]);
    s.run(3.0);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(KNIFE)), "{:?} {:?}", s.events(R_SB_NOTHING), s.game.world.peek(me).and_then(|r| r.inventory.clone()));
    // The knife is lost: the pistol it carries, rounds and all, by the same rank whether
    // the fight's empty-hand rule (`Game::arm_by_style`) or re-arming takes it up first.
    s.game.world.remove_item(me, FormId(KNIFE), 1).unwrap();
    s.run(2.0);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(PISTOL)), "{:?}", s.events(R_SB_NOTHING));
}

#[test]
fn a_melee_only_fighter_holding_a_gun_takes_up_its_knife_by_the_style_rule() {
    // #1721: a held weapon its combat style rules out is the style rule's to swap
    // (`fnv_game::style_weapon`), not re-arming's, though re-arming runs first.
    const MELEE_ONLY: u32 = 0xC51;
    let mut f = with_arms(fixture());
    f.data.combat_styles.insert(FormId(MELEE_ONLY), fnv_game::data::CombatStyleDef { weapon_restrictions: 1, ..Default::default() });
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let me = FormId(R_SB_NOTHING);
    for (item, n) in [(KNIFE, 1), (PISTOL, 1), (ROUNDS, 5)] {
        s.game.world.add_item(me, FormId(item), n).unwrap();
    }
    assert!(s.game.world.outfit(me).unwrap());
    s.game.world.equip(me, FormId(PISTOL)).unwrap();
    s.game.extra.entry(me).or_default().combat_style = Some(FormId(MELEE_ONLY));
    fighting(&mut s, R_SB_NOTHING, [10450.0, 10950.0, 0.0]);
    // The AI's update before the game's frame, as in the engine: re-arming sees the fight
    // before `Game::arm_by_style` (only for actors already in `Game::fighting`) does.
    s.ai.update(&mut s.game, &s.nav, 0.5);
    assert!(!s.game.fighting.contains(&me));
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(KNIFE)), "{:?}", s.events(R_SB_NOTHING));
    s.run(1.5);
    let ev = s.events(R_SB_NOTHING);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(KNIFE)), "{ev:?}");
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::Rearmed { .. })), "re-arming did not take it: {ev:?}");
}

#[test]
fn a_fighter_takes_the_incinerator_for_a_far_target_and_the_flamer_for_a_near_one() {
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let me = FormId(R_SB_NOTHING);
    for (item, n) in [(FLAMER, 1), (INCINERATOR, 1), (ROUNDS, 5)] {
        s.game.world.add_item(me, FormId(item), n).unwrap();
    }
    assert!(s.game.world.outfit(me).unwrap());
    s.game.world.equip(me, FormId(FLAMER)).unwrap();
    // A target 1,500 units off: beyond the flame, so the incinerator.
    let feet = s.feet(R_SB_NOTHING);
    fighting(&mut s, R_SB_NOTHING, [feet[0], feet[1] + 1500.0, 0.0]);
    s.run(1.5);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(INCINERATOR)), "{:?}", s.events(R_SB_NOTHING));
    assert!(s.events(R_SB_NOTHING).iter().any(|e| matches!(e, AiEvent::RangeSwap { weapon, .. } if *weapon == FormId(INCINERATOR))));
    // The target 100 units off: inside the incinerator's blast, so the flamer again,
    // once the swap's 3 s have passed.
    let feet = s.feet(R_SB_NOTHING);
    fighting(&mut s, R_SB_NOTHING, [feet[0] + 100.0, feet[1], 0.0]);
    s.run(1.0);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(INCINERATOR)), "not before 3 s");
    s.run(3.0);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(FLAMER)), "{:?}", s.events(R_SB_NOTHING));
}

#[test]
fn a_cornered_fighter_with_an_explosive_weapon_takes_up_its_knife() {
    // #2745: an incinerator (blast 150) with its target 100 units off and no gun that
    // fires there: it steps back first (the engine's rule), then, still too close after
    // `CORNERED_SECONDS`, holds the knife it carries though a knife is not its class.
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let me = FormId(R_SB_NOTHING);
    for (item, n) in [(INCINERATOR, 1), (KNIFE, 1), (ROUNDS, 5)] {
        s.game.world.add_item(me, FormId(item), n).unwrap();
    }
    assert!(s.game.world.outfit(me).unwrap());
    s.game.world.equip(me, FormId(INCINERATOR)).unwrap();
    let feet = s.feet(R_SB_NOTHING);
    fighting(&mut s, R_SB_NOTHING, [feet[0] + 100.0, feet[1], 0.0]);
    s.run(1.0);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(INCINERATOR)), "not before it had time to step back");
    s.run(3.0);
    let ev = s.events(R_SB_NOTHING);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(KNIFE)), "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, AiEvent::RangeSwap { weapon, .. } if *weapon == FormId(KNIFE))), "{ev:?}");
}

#[test]
fn a_cornered_fighter_with_only_an_explosive_weapon_fights_with_its_fists_until_clear() {
    // #2745 (the Boomers in the Nellis barracks): nothing else carried, so it puts the
    // incinerator away for its fists; no empty-hand rule takes it up again while the
    // target is too close; with the target clear of the blast it holds it again, and the
    // same when the fight ends.
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_SB_NOTHING]);
    s.run(1.0);
    let me = FormId(R_SB_NOTHING);
    for (item, n) in [(INCINERATOR, 1), (ROUNDS, 5)] {
        s.game.world.add_item(me, FormId(item), n).unwrap();
    }
    assert!(s.game.world.outfit(me).unwrap());
    s.game.world.equip(me, FormId(INCINERATOR)).unwrap();
    let feet = s.feet(R_SB_NOTHING);
    fighting(&mut s, R_SB_NOTHING, [feet[0] + 100.0, feet[1], 0.0]);
    s.run(4.0);
    let ev = s.events(R_SB_NOTHING);
    assert_eq!(s.game.held_weapon(me), None, "{ev:?}");
    assert_eq!(s.game.rearms.put_away.get(&me), Some(&FormId(INCINERATOR)));
    assert!(ev.iter().any(|e| matches!(e, AiEvent::RangeFists { weapon, .. } if *weapon == FormId(INCINERATOR))), "{ev:?}");
    // The empty-hand rules (re-arming's and the style's) leave its fists alone.
    s.game.arm_by_style();
    s.run(2.0);
    assert_eq!(s.game.held_weapon(me), None, "{:?}", s.events(R_SB_NOTHING));
    // The target 1,500 units off: clear of the blast, so the incinerator again.
    let feet = s.feet(R_SB_NOTHING);
    fighting(&mut s, R_SB_NOTHING, [feet[0], feet[1] + 1500.0, 0.0]);
    s.run(1.5);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(INCINERATOR)), "{:?}", s.events(R_SB_NOTHING));
    assert!(s.game.rearms.put_away.is_empty());
    // Cornered again, then the fight is over: it holds the incinerator once more.
    let feet = s.feet(R_SB_NOTHING);
    fighting(&mut s, R_SB_NOTHING, [feet[0] + 100.0, feet[1], 0.0]);
    s.run(6.0);
    assert_eq!(s.game.held_weapon(me), None, "{:?}", s.events(R_SB_NOTHING));
    s.game.calm_rearm(me);
    assert_eq!(s.game.held_weapon(me).map(|w| w.0), Some(FormId(INCINERATOR)));
    assert!(s.game.rearms.put_away.is_empty());
}

/// Stay-at locations given by an object, and a Guard that remains near what it guards
/// (#961, `goal::stay_at`, see docs/spec-notes/packages.md §7.7): Near Object ID walks
/// to the nearest reference of the base (the
/// idle marker, 500 units off); Remain Near Reference to Guard walks to the guarded
/// reference, within the target's distance (120), though the location is "near current".
#[test]
fn object_locations_and_a_guard_remaining_near_its_reference_are_carried_out() {
    let f = fixture();
    let mut s = sim(&f, &[R_SEEKER, R_KEEPER]);
    s.run(30.0);
    let idle = [10500.0, 10250.0, 0.0];
    assert!(dist(s.feet(R_SEEKER), idle) <= 101.0, "seeker at {:?}", s.feet(R_SEEKER));
    assert!(!s.events(R_SEEKER).iter().any(|e| matches!(e, AiEvent::NotCarriedOut { .. })));
    assert!(dist(s.feet(R_KEEPER), MIDDLE_C) <= 121.0, "keeper at {:?}", s.feet(R_KEEPER));
    assert!(dist(s.feet(R_KEEPER), [10950.0, 10050.0, 0.0]) > 400.0);
}

// ---- arriving behind a loading screen (`fnv_ai::arrive`, issue #1208) ----------------

#[test]
fn an_arriving_actor_is_put_at_its_packages_place_without_walking() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    let arr = s.ai.arrive(&mut s.game, &s.nav, actor, &|_| true).expect("put at its place");
    assert_eq!(arr.package, FormId(P_TRAVEL_B));
    assert_eq!((arr.from.space, arr.to.space, arr.walks, arr.furniture), (A, B, 1, None));
    // A walk through a door into another space is not measured.
    assert_eq!(arr.skipped, 0.0);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
    let a = &s.ai.agents[&actor];
    assert_eq!(a.activity, Activity::There);
    assert!(a.walker.follower.is_none());
    assert_eq!(a.walker.travelled, 0.0, "not walked");
    // The game knows where it is, and that the package met its goal.
    assert_eq!(s.game.world.peek(actor).unwrap().cell, Some(FormId(CELL_B)));
    assert!(s.game.extra[&actor].package_goal_met);
    assert!(matches!(s.events(R_TRAVEL).last(), Some(AiEvent::Arrived { .. })));
    // There already: nothing to do again, and the updates after leave it there.
    assert_eq!(s.ai.arrive(&mut s.game, &s.nav, actor, &|_| true), None);
    s.run(10.0);
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
    assert_eq!(s.ai.agents[&actor].walker.travelled, 0.0);
}

#[test]
fn an_arriving_actor_whose_package_gives_it_a_chair_is_on_it() {
    let f = fixture();
    let mut s = sim(&f, &[R_TO_CHAIR]);
    let actor = FormId(R_TO_CHAIR);
    let arr = s.ai.arrive(&mut s.game, &s.nav, actor, &|_| true).expect("put at its place");
    assert_eq!(arr.furniture, Some(FormId(CHAIR_C)));
    let a = &s.ai.agents[&actor];
    assert_eq!((a.activity, a.using), (Activity::There, Some(FormId(CHAIR_C))));
    assert_eq!(a.walker.travelled, 0.0);
    // Still on it after the next updates.
    s.run(5.0);
    assert_eq!(s.ai.agents[&actor].using, Some(FormId(CHAIR_C)));
}

#[test]
fn an_arriving_sandboxer_is_already_at_what_it_chose() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    let actor = FormId(R_SB_OBJECTS);
    let arr = s.ai.arrive(&mut s.game, &s.nav, actor, &|_| true).expect("put at what it chose");
    assert!(arr.walks >= 1, "{arr:?}");
    let did = doings(&s, R_SB_OBJECTS);
    assert_eq!(did.len(), 1, "{did:?}");
    assert!(matches!(did[0], Doing::Sitting(c) if c.0 == CHAIR_C) || matches!(did[0], Doing::AtIdleMarker(m) if m.0 == IDLE_C), "{did:?}");
    let a = &s.ai.agents[&actor];
    assert_eq!(a.activity, Activity::There);
    assert_eq!(a.walker.travelled, 0.0);
    assert_eq!(a.using, arr.furniture);
    assert_eq!(a.using.is_some(), matches!(did[0], Doing::Sitting(_)));
}

#[test]
fn an_arriving_patrol_that_goes_once_is_at_its_last_point() {
    let f = fixture();
    let mut s = sim(&f, &[R_PATROL]);
    let actor = FormId(R_PATROL);
    let arr = s.ai.arrive(&mut s.game, &s.nav, actor, &|_| true).expect("put along its patrol");
    assert_eq!(arr.walks, 3);
    assert!(dist(arr.to.pos, [50.0, 350.0, 0.0]) < 5.0, "{arr:?}");
    // Three straight stretches between the points, in one space.
    assert!(arr.skipped > 800.0, "{arr:?}");
    let points: Vec<u32> = s.events(R_PATROL).iter().filter_map(|e| match e {
        AiEvent::Arrived { patrol_point: Some(p), .. } => Some(p.0),
        _ => None,
    }).collect();
    assert_eq!(points, vec![M1, M2, M3]);
    assert_eq!(s.ai.agents[&actor].activity, Activity::There);
    assert_eq!(s.ai.agents[&actor].walker.travelled, 0.0);
    assert!(s.game.extra[&actor].package_goal_met);
}

#[test]
fn an_arriving_patrol_that_goes_round_is_at_the_point_it_sets_off_for() {
    let f = fixture();
    let mut s = sim(&f, &[R_LOOP]);
    let actor = FormId(R_LOOP);
    // The sentry stands next to L3, its nearest point: there, then it walks on in sight.
    let arr = s.ai.arrive(&mut s.game, &s.nav, actor, &|_| true).expect("put at its first point");
    assert_eq!(arr.walks, 1);
    assert!(dist(arr.to.pos, [50.0, 250.0, 0.0]) < 5.0, "{arr:?}");
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(!s.game.extra[&actor].package_goal_met);
}

#[test]
fn an_actor_whose_walk_starts_away_from_the_player_is_left_to_the_low_process() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL]);
    let actor = FormId(R_TRAVEL);
    // In A, walking to B: with only B near the player (the loaded area), it is not put
    // there, though its walk ends there (the low process brings it to the area's edge).
    assert_eq!(s.ai.arrive(&mut s.game, &s.nav, actor, &|p| p.space == B), None);
    assert!(dist(s.feet(R_TRAVEL), START) < 1.0);
    assert_eq!(s.ai.agents[&actor].walker.travelled, 0.0);
    // A near too (a room behind a door of the loaded cells): put there.
    assert!(s.ai.arrive(&mut s.game, &s.nav, actor, &|p| p.space == A || p.space == B).is_some());
    assert!(dist(s.feet(R_TRAVEL), [5250.0, 5250.0, 50.0]) < 5.0);
}

/// An actor whose package's place is the player (a hunter, a hit squad) is not put
/// beside the player on arrival (coordinator review of #1269).
#[test]
fn an_actor_sent_after_the_player_is_not_put_beside_the_player() {
    let f = fixture();
    let mut s = sim(&f, &[R_HUNTER]);
    let actor = FormId(R_HUNTER);
    let def = &s.game.data.package_defs[&FormId(P_HUNT)];
    assert!(matches!(def.location.map(|l| l.kind), Some(LocationKind::NearReference(Some(p))) if p == FormId::PLAYER_REF));
    {
        let p = s.game.world.get(FormId::PLAYER_REF).unwrap();
        (p.cell, p.pos, p.rot) = (Some(FormId(CELL_B)), [5250.0, 5250.0, 50.0], [0.0; 3]);
    }
    assert_eq!(s.ai.arrive(&mut s.game, &s.nav, actor, &|_| true), None);
    assert!(dist(s.feet(R_HUNTER), [150.0, 150.0, 0.0]) < 1.0, "left where it was");
    // It does set off after the player, walking.
    s.run(2.0);
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(dist(s.feet(R_HUNTER), [5250.0, 5250.0, 50.0]) > 1000.0);
}

/// An actor in a combat state, or one a script just put somewhere, stays where it is
/// on arrival (coordinator review of #1269).
#[test]
fn an_actor_in_combat_or_just_moved_by_a_script_is_not_put_anywhere() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL, R_TO_CHAIR]);
    s.game.start_combat(FormId(R_TRAVEL), FormId(R_TO_CHAIR));
    assert!(s.game.awareness.state(FormId(R_TRAVEL)).in_combat());
    assert_eq!(s.ai.arrive(&mut s.game, &s.nav, FormId(R_TRAVEL), &|_| true), None);
    assert!(dist(s.feet(R_TRAVEL), START) < 1.0);
    // A script's MoveTo just before: where the script put it wins.
    s.ai.relocate(&mut s.game, FormId(R_TO_CHAIR));
    assert_eq!(s.ai.arrive(&mut s.game, &s.nav, FormId(R_TO_CHAIR), &|_| true), None);
    assert_eq!(s.ai.agents[&FormId(R_TO_CHAIR)].using, None);
    // Some time on, its package has it again.
    s.game.time += fnv_ai::arrive::SCRIPT_MOVE_HOLD + 1.0;
    assert_eq!(s.ai.arrive(&mut s.game, &s.nav, FormId(R_TO_CHAIR), &|_| true).and_then(|a| a.furniture), Some(FormId(CHAIR_C)));
}

#[test]
fn rooted_dead_or_restrained_actors_are_not_put_anywhere() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL, R_TO_CHAIR]);
    s.ai.root(&s.game, FormId(R_TRAVEL));
    assert_eq!(s.ai.arrive(&mut s.game, &s.nav, FormId(R_TRAVEL), &|_| true), None);
    s.game.extra.entry(FormId(R_TO_CHAIR)).or_default().restrained = true;
    assert_eq!(s.ai.arrive(&mut s.game, &s.nav, FormId(R_TO_CHAIR), &|_| true), None);
    s.game.extra.get_mut(&FormId(R_TO_CHAIR)).unwrap().restrained = false;
    s.game.extra.get_mut(&FormId(R_TO_CHAIR)).unwrap().dead = true;
    assert_eq!(s.ai.arrive(&mut s.game, &s.nav, FormId(R_TO_CHAIR), &|_| true), None);
    for r in [R_TRAVEL, R_TO_CHAIR] {
        assert_eq!(s.ai.agents[&FormId(r)].walker.travelled, 0.0);
        assert!(dist(s.feet(r), s.game.world.placed.get(FormId(r)).unwrap().pos) < 1.0);
    }
}

// ---- interrupted sandbox activities (#1497) ----------------------------------------

/// Reach a real choice through production AI, not by filling in its private pending
/// state. A travelling choice is identified by the last choice event and its follower;
/// the previous activity can still be in `doing` until the destination is reached.
fn sandbox_until(s: &mut Sim, actor: u32, doing: Doing, travelling: bool) {
    for _ in 0..1200 {
        s.run(0.5);
        let a = &s.ai.agents[&FormId(actor)];
        let chosen = doings(s, actor).last().copied() == Some(doing);
        if chosen && if travelling {
            a.activity == Activity::Travelling && a.walker.follower.is_some()
        } else {
            a.activity == Activity::There && a.walker.follower.is_none()
                && a.sandbox.as_ref().is_some_and(|sb| sb.doing == doing)
        } {
            return;
        }
    }
    panic!("{actor:08X} never reached {doing:?}, travelling={travelling}; {:?}", s.events(actor));
}

fn sandbox_cancelled(s: &Sim, actor: u32) {
    let a = &s.ai.agents[&FormId(actor)];
    let sb = a.sandbox.as_ref().expect("the sandbox day is retained");
    assert_eq!(sb.phase(), SandboxPhase::Standing, "no abandoned object/exit phase");
    assert_eq!(sb.doing, Doing::Standing, "no active furniture or marker action in combat");
    assert!(sb.due(&s.game), "resumption must not wait out the abandoned action's timer");
    assert_eq!(a.using, None);
    assert_eq!(a.face_to, None, "the old idle marker cannot turn the fighter");
}

fn sandbox_resumes(s: &mut Sim, actor: u32) {
    let before = doings(s, actor).len();
    let packages = s.events(actor).iter().filter(|e| matches!(e, AiEvent::Package { .. })).count();
    assert_eq!(s.game.awareness.state(FormId(actor)), State::Normal);
    s.run(1.0);
    assert!(doings(s, actor).len() > before, "a new choice after recovery: {:?}", s.events(actor));
    assert!(s.ai.agents[&FormId(actor)].search.is_none());
    assert_eq!(s.events(actor).iter().filter(|e| matches!(e, AiEvent::Package { .. })).count(), packages,
        "recovery must not rely on a package replacement");
    let chosen = doings(s, actor)[before];
    for _ in 0..40 {
        let a = &s.ai.agents[&FormId(actor)];
        let sb = a.sandbox.as_ref().unwrap();
        if a.activity == Activity::There && a.walker.follower.is_none() && sb.doing == chosen
            && !matches!(sb.phase(), SandboxPhase::ToObject | SandboxPhase::GotUp) {
            assert_eq!(a.using, chosen.furniture(), "the chosen furniture is actually in use");
            let object = match chosen {
                Doing::AtIdleMarker(marker) => Some(marker),
                _ => chosen.furniture(),
            };
            if let Some(object) = object {
                let place = s.game.world.placed.get(object).expect("fixture object");
                assert!(dist(a.walker.feet, place.pos) < 40.0,
                    "the new activity begins at {object}, not at the combat destination: {:?}", a.walker.feet);
            }
            return;
        }
        s.run(0.5);
    }
    panic!("the recovered choice {chosen:?} never began at its destination: {:?}", s.events(actor));
}

#[test]
fn sandbox_interrupt_pending_chair_holding_resumes_after_stop_combat() {
    let f = fixture();
    let mut control = sim(&f, &[R_SB_OBJECTS]);
    sandbox_until(&mut control, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), true);
    let initial = control.feet(R_SB_OBJECTS);
    let before = doings(&control, R_SB_OBJECTS).len();
    control.run(300.0);
    assert!(doings(&control, R_SB_OBJECTS).len() > before, "the uninterrupted control progresses");

    let mut s = sim(&f, &[R_SB_OBJECTS]);
    sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), true);
    assert_eq!(s.feet(R_SB_OBJECTS), initial);
    let actor = FormId(R_SB_OBJECTS);
    let saved = s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule();
    fighting(&mut s, R_SB_OBJECTS, [initial[0], initial[1] + 100.0, initial[2]]);
    s.run(0.5);
    assert!(s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Hold { .. })));
    sandbox_cancelled(&s, R_SB_OBJECTS);
    s.run(5.0);
    assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule(), saved,
        "combat updates must preserve the day, energy and random sequence");
    s.game.stop_combat(actor);
    sandbox_resumes(&mut s, R_SB_OBJECTS);
    let before = doings(&s, R_SB_OBJECTS).len();
    s.run(300.0);
    assert!(doings(&s, R_SB_OBJECTS).len() > before, "recovery continues beyond its first choice");
}

#[test]
fn sandbox_interrupt_pending_marker_closing_resumes_after_target_death() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    sandbox_until(&mut s, R_SB_OBJECTS, Doing::AtIdleMarker(FormId(IDLE_C)), true);
    let actor = FormId(R_SB_OBJECTS);
    let saved = s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule();
    let at = s.feet(R_SB_OBJECTS);
    fighting(&mut s, R_SB_OBJECTS, [10100.0, 10900.0, 0.0]);
    s.game.awareness.engage.insert(actor, (50.0, 100.0));
    s.run(0.5);
    assert!(s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Close { .. })));
    sandbox_cancelled(&s, R_SB_OBJECTS);
    assert!(s.ai.agents[&actor].walker.follower.is_some(), "the combat route replaces the sandbox route");
    s.run(0.5);
    assert!(dist(s.feet(R_SB_OBJECTS), at) > 20.0, "repeated combat updates keep the combat walk alive");
    assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule(), saved);
    // The real target-death path, not a hand-assigned awareness state or StopCombat.
    assert!(s.game.kill(FormId::PLAYER_REF, Some(actor)));
    s.game.settle_awareness();
    sandbox_resumes(&mut s, R_SB_OBJECTS);
}

#[test]
fn sandbox_interrupt_combat_without_a_remaining_target_stops_the_old_walk() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), true);
    let actor = FormId(R_SB_OBJECTS);
    let at = s.feet(R_SB_OBJECTS);
    fighting(&mut s, R_SB_OBJECTS, [at[0], at[1] + 100.0, at[2]]);
    // A target can disappear between measurement and the next awareness settlement.
    s.game.awareness.actors.get_mut(&actor).unwrap().forget(FormId::PLAYER_REF);
    assert_eq!(s.game.awareness.state(actor), State::Combat);
    s.run(0.5);
    sandbox_cancelled(&s, R_SB_OBJECTS);
    assert_eq!(s.feet(R_SB_OBJECTS), at);
    assert!(s.ai.agents[&actor].walker.follower.is_none());
    s.game.settle_awareness();
    sandbox_resumes(&mut s, R_SB_OBJECTS);
}

#[test]
fn sandbox_interrupt_active_chair_marker_sleep_and_meal_are_cancelled() {
    let f = fixture();
    for (actor, doing) in [
        (R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C))),
        (R_SB_OBJECTS, Doing::AtIdleMarker(FormId(IDLE_C))),
        (R_SB_ALL, Doing::Sleeping(FormId(BED_C))),
        (R_SB_ALL, Doing::Eating(Some(FormId(CHAIR_C)))),
    ] {
        let mut s = sim(&f, &[actor]);
        if matches!(doing, Doing::Sleeping(_)) {
            s.game.world.set_global(FormId(G_HOUR), 2.0).unwrap();
        } else if matches!(doing, Doing::Eating(_)) {
            s.run(0.5);
            let lunch = s.ai.agents[&FormId(actor)].sandbox.as_ref().unwrap().times().1[1].0 + 0.1;
            s.game.world.set_global(FormId(G_HOUR), lunch).unwrap();
        }
        sandbox_until(&mut s, actor, doing, false);
        let me = FormId(actor);
        assert!(!s.ai.agents[&me].sandbox.as_ref().unwrap().due(&s.game), "interrupt an active timer: {doing:?}");
        let saved = s.ai.agents[&me].sandbox.as_ref().unwrap().schedule();
        let start = s.ai.events.len();
        let at = s.feet(actor);
        fighting(&mut s, actor, [at[0], at[1] + 100.0, at[2]]);
        s.run(0.5);
        sandbox_cancelled(&s, actor);
        s.run(2.0);
        assert_eq!(s.ai.agents[&me].sandbox.as_ref().unwrap().schedule(), saved, "{doing:?}");
        let releases = s.ai.events[start..].iter().filter(|e| e.actor == me && matches!(e.event, AiEvent::Furniture { furniture: None })).count();
        assert_eq!(releases, usize::from(doing.furniture().is_some()), "release an occupied seat once: {doing:?}");
        s.game.stop_combat(me);
        sandbox_resumes(&mut s, actor);
    }
}

#[test]
fn sandbox_interrupt_releases_pending_and_active_chair_reservations() {
    let f = fixture();
    for travelling in [true, false] {
        let mut s = sim(&f, &[R_SB_OBJECTS]);
        sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), travelling);
        let at = s.feet(R_SB_OBJECTS);
        fighting(&mut s, R_SB_OBJECTS, [at[0], at[1] + 100.0, at[2]]);
        s.run(0.5);
        // Furniture availability is tested by another real sandbox choosing it.
        s.ai.add(&s.game, FormId(R_SB_OBJECTS_2)).unwrap();
        sandbox_until(&mut s, R_SB_OBJECTS_2, Doing::Sitting(FormId(CHAIR_C)), false);
        assert_eq!(s.ai.agents[&FormId(R_SB_OBJECTS_2)].using, Some(FormId(CHAIR_C)));
        assert_eq!(s.ai.agents[&FormId(R_SB_OBJECTS)].using, None);
    }
}

#[test]
fn sandbox_interrupt_releases_a_chair_held_by_a_script_look() {
    use fnv_game::functions::senses::Look;
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    let actor = FormId(R_SB_OBJECTS);
    s.game.senses.looks.insert(actor, Look { target: FormId(R_SLEEP_C), rotate_body: true });
    sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), false);
    assert_eq!(s.ai.agents[&actor].using, None, "Look holds the seat without actual furniture use");
    let at = s.feet(R_SB_OBJECTS);
    fighting(&mut s, R_SB_OBJECTS, [at[0], at[1] + 100.0, at[2]]);
    s.run(0.5);
    sandbox_cancelled(&s, R_SB_OBJECTS);
    s.game.senses.looks.remove(&actor);
    s.ai.social(&mut s.game, &s.nav, 0.5);
    assert_eq!(s.ai.agents[&actor].using, None, "StopLook must not resurrect the combat-cancelled chair");
    s.ai.add(&s.game, FormId(R_SB_OBJECTS_2)).unwrap();
    sandbox_until(&mut s, R_SB_OBJECTS_2, Doing::Sitting(FormId(CHAIR_C)), false);
}

#[test]
fn sandbox_interrupt_post_flee_and_search_do_not_complete_the_old_choice() {
    use fnv_game::combat_group::{Flight, GroupSettings, Post, PostKind};
    let f = fixture();
    for mode in ["post", "flee", "search"] {
        let mut s = sim(&f, &[R_SB_OBJECTS]);
        sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), true);
        let actor = FormId(R_SB_OBJECTS);
        let saved = s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule();
        let threat = [10500.0, 10100.0, 0.0];
        if mode == "search" {
            alert(&mut s, R_SB_OBJECTS, threat);
        } else {
            fighting(&mut s, R_SB_OBJECTS, threat);
            if mode == "post" {
                s.game.awareness.engage.insert(actor, (2000.0, 2500.0));
                s.game.awareness.tactics.posts.insert(actor, Post { at: [10200.0, 10600.0, 0.0], kind: PostKind::Flank, run: true });
            } else {
                s.game.awareness.tactics.settings = Some(GroupSettings { flee_normal: 300.0, ..GroupSettings::default() });
                s.game.awareness.tactics.flights.insert(actor, Flight { from: FormId::PLAYER_REF, from_at: threat, since: s.game.time, spot: None, arrived: None });
            }
        }
        let before = doings(&s, R_SB_OBJECTS).len();
        s.run(0.5);
        sandbox_cancelled(&s, R_SB_OBJECTS);
        s.run(8.0);
        assert_eq!(doings(&s, R_SB_OBJECTS).len(), before, "{mode}: movement belongs to the interruption");
        assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule(), saved);
        assert_eq!(s.ai.agents[&actor].using, None, "{mode}: cannot sit at the combat destination");
        assert!(s.events(R_SB_OBJECTS).iter().any(|e| match mode {
            "post" => matches!(e, AiEvent::AtPost { .. }),
            "flee" => matches!(e, AiEvent::FleeArrived { .. }),
            _ => matches!(e, AiEvent::SearchLook { .. }),
        }), "{mode}: the replacement route completes: {:?}", s.events(R_SB_OBJECTS));
        s.game.stop_combat(actor);
        sandbox_resumes(&mut s, R_SB_OBJECTS);
    }
}

#[test]
fn sandbox_interrupt_rearming_keeps_its_fetch_and_then_resumes() {
    let f = with_arms(fixture());
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), true);
    let actor = FormId(R_SB_OBJECTS);
    let at = s.feet(R_SB_OBJECTS);
    let saved = s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule();
    lay(&mut s, PISTOL, [at[0] - 250.0, at[1], 0.0]);
    lay(&mut s, ROUNDS, [at[0] - 120.0, at[1] + 60.0, 0.0]);
    fighting(&mut s, R_SB_OBJECTS, [at[0], at[1] + 300.0, 0.0]);
    s.run(0.5);
    assert!(s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Fetch { .. })));
    sandbox_cancelled(&s, R_SB_OBJECTS);
    s.run(10.0);
    assert_eq!(s.game.held_weapon(actor).map(|w| w.0), Some(FormId(PISTOL)), "combat updates do not cancel the fetch");
    assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule(), saved);
    s.game.stop_combat(actor);
    sandbox_resumes(&mut s, R_SB_OBJECTS);
}

#[test]
fn sandbox_interrupt_warning_cancels_a_walk_but_preserves_an_active_seat() {
    let f = fixture();
    for travelling in [true, false] {
        let mut s = sim(&f, &[R_SB_OBJECTS]);
        sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), travelling);
        let actor = FormId(R_SB_OBJECTS);
        let at = s.feet(R_SB_OBJECTS);
        let saved = s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule();
        let player = s.game.world.get(FormId::PLAYER_REF).unwrap();
        (player.cell, player.pos) = (Some(FormId(CELL_C)), [at[0] + 100.0, at[1], at[2]]);
        s.game.provocations.aggro_inside.insert(actor);
        s.run(1.0);
        assert_eq!(s.game.awareness.state(actor), State::Normal);
        if travelling {
            assert!(s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Warn { .. })));
            sandbox_cancelled(&s, R_SB_OBJECTS);
            s.game.provocations.aggro_inside.clear();
            sandbox_resumes(&mut s, R_SB_OBJECTS);
        } else {
            assert_eq!(s.ai.agents[&actor].using, Some(FormId(CHAIR_C)));
            assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule(), saved);
            assert!(!s.ai.agents[&actor].sandbox.as_ref().unwrap().due(&s.game));
            assert!(!s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Warn { .. })));
        }
    }
}

/// Continue During Combat does not hold a Sandbox package's actor ("the flags do not
/// work" for Sandbox, GECK wiki "Combat Package Flags"; alert.md §5.2, #2155): it leaves
/// its seat and fights as any other actor.
#[test]
fn sandbox_continue_during_combat_does_not_hold() {
    let mut f = fixture();
    f.data.package_defs.get_mut(&FormId(P_SB_OBJECTS)).unwrap().flags |= flags::CONTINUE_DURING_COMBAT;
    for travelling in [true, false] {
        let mut s = sim(&f, &[R_SB_OBJECTS]);
        sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), travelling);
        let actor = FormId(R_SB_OBJECTS);
        let at = s.feet(R_SB_OBJECTS);
        fighting(&mut s, R_SB_OBJECTS, [at[0], at[1] + 100.0, at[2]]);
        s.run(10.0);
        assert_ne!(s.ai.agents[&actor].using, Some(FormId(CHAIR_C)), "travelling {travelling}");
        assert!(s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Hold { .. } | AiEvent::Close { .. })), "travelling {travelling}");
    }
}

#[test]
fn sandbox_interrupt_does_not_reset_valid_standing_between_choices() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_NOTHING]);
    sandbox_until(&mut s, R_SB_NOTHING, Doing::Standing, false);
    let actor = FormId(R_SB_NOTHING);
    let saved = s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule();
    let before = doings(&s, R_SB_NOTHING).len();
    assert!(!s.ai.agents[&actor].sandbox.as_ref().unwrap().due(&s.game));
    s.run(1.0);
    assert_eq!(doings(&s, R_SB_NOTHING).len(), before, "no follower is valid while standing");
    assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule(), saved);
    assert_eq!(s.ai.agents[&actor].walker.travelled, 0.0);
}

#[test]
fn sandbox_interrupt_normal_spectator_flee_cancels_the_active_seat() {
    use fnv_game::combat_group::GroupSettings;
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), false);
    let actor = FormId(R_SB_OBJECTS);
    let at = s.feet(R_SB_OBJECTS);
    let player_at = [at[0] - 100.0, at[1], at[2]];
    let player = s.game.world.get(FormId::PLAYER_REF).unwrap();
    (player.cell, player.pos) = (Some(FormId(CELL_C)), player_at);
    s.game.awareness.tactics.settings = Some(GroupSettings { flee_normal: 200.0, ..GroupSettings::default() });
    // The non-resident loner fights the player beside it; the sandboxer stays a neutral
    // observer. The loner is put near: a foe with no place near the player is near no one
    // (#3223), and its placed spot is in another cell.
    let loner = s.game.world.get(FormId(R_LONER)).unwrap();
    (loner.cell, loner.pos) = (Some(FormId(CELL_C)), [player_at[0] - 150.0, player_at[1], player_at[2]]);
    fighting(&mut s, R_LONER, player_at);
    let saved = s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule();
    s.ai.social(&mut s.game, &s.nav, 0.5);
    assert!(s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::SpectatorFlee { .. })), "{:?}", s.events(R_SB_OBJECTS));
    assert_eq!(s.game.awareness.state(actor), State::Normal, "this takeover bypasses combat alert_step");
    sandbox_cancelled(&s, R_SB_OBJECTS);
    // End the fight after the actual flee arrival. Waiting a fixed five seconds hits
    // the deliberate re-flee timer while the player is still close to this small room.
    for _ in 0..8 {
        s.run(0.5);
        if matches!(s.ai.agents[&actor].search, Some(fnv_ai::alert::Search::Fled { .. })) {
            break;
        }
    }
    assert!(matches!(s.ai.agents[&actor].search, Some(fnv_ai::alert::Search::Fled { .. })), "the replacement flee route arrives");
    assert!(s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::FleeArrived { .. })));
    assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule(), saved);
    s.game.stop_combat(FormId(R_LONER));
    sandbox_resumes(&mut s, R_SB_OBJECTS);
}

#[test]
fn sandbox_interrupt_streaming_out_and_back_does_not_restore_the_abandoned_choice() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), true);
    let actor = FormId(R_SB_OBJECTS);
    let at = s.feet(R_SB_OBJECTS);
    fighting(&mut s, R_SB_OBJECTS, [at[0], at[1] + 100.0, at[2]]);
    s.run(0.5);
    sandbox_cancelled(&s, R_SB_OBJECTS);
    let saved = s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule();
    assert!(s.ai.set_high(&s.game, actor, false));
    s.run(1.0);
    s.game.stop_combat(actor);
    assert!(s.ai.set_high(&s.game, actor, true));
    assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule(), saved);
    sandbox_resumes(&mut s, R_SB_OBJECTS);
}

#[test]
fn sandbox_interrupt_a_package_changed_during_combat_resumes_its_new_activity() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), true);
    let actor = FormId(R_SB_OBJECTS);
    let at = s.feet(R_SB_OBJECTS);
    fighting(&mut s, R_SB_OBJECTS, [at[0], at[1] + 100.0, at[2]]);
    s.run(0.5);
    sandbox_cancelled(&s, R_SB_OBJECTS);
    s.game.extra.entry(actor).or_default().script_package = Some(FormId(P_SB_WANDER));
    assert_eq!(s.game.evaluate_package(actor), Some(FormId(P_SB_WANDER)));
    s.run(0.5);
    assert!(s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Package { package: Some(p) } if *p == FormId(P_SB_WANDER))));
    s.game.stop_combat(actor);
    let before = doings(&s, R_SB_OBJECTS).len();
    s.run(1.0);
    let choices = doings(&s, R_SB_OBJECTS);
    assert!(choices.len() > before);
    assert!(choices[before..].iter().all(|d| *d == Doing::Wandering), "new package, not the abandoned chair: {choices:?}");
    assert_eq!(s.ai.agents[&actor].using, None);
    s.ai.add(&s.game, FormId(R_SB_OBJECTS_2)).unwrap();
    sandbox_until(&mut s, R_SB_OBJECTS_2, Doing::Sitting(FormId(CHAIR_C)), false);
}

#[test]
fn sandbox_interrupt_recovery_can_be_interrupted_and_recovered_again() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    sandbox_until(&mut s, R_SB_OBJECTS, Doing::Sitting(FormId(CHAIR_C)), true);
    let actor = FormId(R_SB_OBJECTS);
    for round in 0..2 {
        let at = s.feet(R_SB_OBJECTS);
        let saved = s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule();
        fighting(&mut s, R_SB_OBJECTS, [at[0], at[1] + 100.0, at[2]]);
        s.run(0.5);
        sandbox_cancelled(&s, R_SB_OBJECTS);
        assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule(), saved, "round {round}");
        s.game.stop_combat(actor);
        sandbox_resumes(&mut s, R_SB_OBJECTS);
        assert!(matches!(s.ai.agents[&actor].sandbox.as_ref().unwrap().doing,
            Doing::Sitting(_) | Doing::AtIdleMarker(_)), "round {round} completed a real new object choice");
    }
    assert_eq!(s.events(R_SB_OBJECTS).iter().filter(|e| matches!(e, AiEvent::Package { .. })).count(), 1,
        "both interruptions recover within the same sandbox instance");
}

#[test]
fn sandbox_interrupt_preserves_a_valid_chat_approach_and_wait_until_combat() {
    use fnv_ai::Chat;
    let mut f = fixture();
    f.data.package_defs.get_mut(&FormId(P_SB_NOTHING)).unwrap().type_flags &= !type_flags::NO_CONVERSATION;
    for interrupt_starter in [false, true] {
        let mut s = sim(&f, &[R_SB_OBJECTS, R_SB_NOTHING]);
        sandbox_until(&mut s, R_SB_OBJECTS, Doing::AtIdleMarker(FormId(IDLE_C)), false);
        let (starter, partner) = (FormId(R_SB_NOTHING), FormId(R_SB_OBJECTS));
        let waiting_at = s.feet(R_SB_OBJECTS);
        // The same public conversation-boundary setup as the existing walk-up test,
        // now with both sides present. Speech selection belongs to the dialogue tests.
        let a = s.ai.agents.get_mut(&starter).unwrap();
        a.walker.go_to(&s.nav, fnv_nav::path::Place { space: C, pos: waiting_at }, &fnv_nav::path::PathParams::default()).unwrap();
        a.activity = Activity::Travelling;
        a.chat = Some(Chat::Going { partner, since: s.game.time });
        s.ai.agents.get_mut(&partner).unwrap().chat = Some(Chat::Waiting { starter });
        let saved = s.ai.progress();
        let before = s.feet(R_SB_NOTHING);
        s.run(0.5);
        s.ai.social(&mut s.game, &s.nav, 0.5);
        assert!(matches!(s.ai.agents[&starter].chat, Some(Chat::Going { partner: p, .. }) if p == partner));
        assert_eq!(s.ai.agents[&partner].chat, Some(Chat::Waiting { starter }));
        assert!(s.ai.agents[&starter].walker.follower.is_some(), "a legitimate approach keeps its follower");
        assert!(dist(s.feet(R_SB_NOTHING), waiting_at) <= dist(before, waiting_at));
        assert!(s.ai.holds_for_chat(partner));
        assert_eq!(s.feet(R_SB_OBJECTS), waiting_at, "the partner waits at its current activity");
        assert_eq!(s.ai.progress(), saved, "neither legitimate chat state restarts its sandbox");

        let who = if interrupt_starter { R_SB_NOTHING } else { R_SB_OBJECTS };
        let actor = FormId(who);
        let at = s.feet(who);
        fighting(&mut s, who, [at[0], at[1] + 100.0, at[2]]);
        s.run(0.5);
        sandbox_cancelled(&s, who);
        assert!(s.events(who).iter().any(|e| matches!(e, AiEvent::Hold { .. })));
        // Normal social updates resolve the abandoned approach; the test never clears
        // either actor's chat field as part of recovery.
        for _ in 0..8 {
            s.ai.social(&mut s.game, &s.nav, 0.5);
            s.run(0.5);
        }
        assert!(s.ai.agents[&actor].chat.is_none(), "the interrupted chat has ended");
        s.game.stop_combat(actor);
        sandbox_resumes(&mut s, who);
    }
}

// ---- furniture eligibility (issue #1522): cell D ------------------------------------

/// The facts of a test: its own answer for fit, and marker IDs by furniture base. The
/// cross has marker 10, the seats 11 and the bed 1; all made up for the test.
fn facts(fit: fn(ActorKind, &str, FormId) -> Fit) -> Arc<dyn FurnitureFacts> {
    struct Facts(fn(ActorKind, &str, FormId) -> Fit, BTreeMap<FormId, Vec<u8>>);
    impl FurnitureFacts for Facts {
        fn marker_ids(&self, base: FormId) -> Option<&[u8]> {
            self.1.get(&base).map(Vec::as_slice)
        }
        fn fit(&self, kind: ActorKind, skeleton: &str, base: FormId) -> Fit {
            (self.0)(kind, skeleton, base)
        }
    }
    Arc::new(Facts(fit, BTreeMap::from([(FormId(CROSS), vec![10]), (FormId(CHAIR), vec![11]), (FormId(CHAIR_2), vec![11]), (FormId(BED), vec![1])])))
}

/// What the eligibility events of `actor` say: (furniture, decision, reason).
fn eligibility(s: &Sim, actor: u32) -> Vec<(u32, &'static str, &'static str)> {
    s.events(actor).iter().filter_map(|e| match e {
        AiEvent::FurnitureEligibility { furniture, decision, reason } => Some((furniture.0, *decision, *reason)),
        _ => None,
    }).collect()
}

fn set_clock(s: &mut Sim, hour: f32, days: f32) {
    s.game.world.set_global(FormId(G_HOUR), hour).unwrap();
    s.game.world.set_global(FormId(G_DAYS), days).unwrap();
}

/// The hour of lunch this sandbox rolled.
fn lunch(s: &Sim, actor: u32) -> f32 {
    s.ai.agents[&FormId(actor)].sandbox.as_ref().unwrap().times().1[1].0 + 0.1
}

#[test]
fn unsupported_creature_skips_seats_beds_and_meal_seats() {
    let f = fixture();
    let mut s = sim(&f, &[R_HOUND]);
    // The skeleton the record names reaches the facts.
    s.ai.set_furniture_facts(facts(|kind, skeleton, _| if kind == ActorKind::Creature && skeleton == HOUND_MODEL { Fit::Unsupported } else { panic!("{kind:?} {skeleton}") }));
    let hound = FormId(R_HOUND);
    let watch = |s: &mut Sim, seconds: usize| {
        for _ in 0..seconds {
            s.run(1.0);
            assert_eq!(s.ai.agents[&hound].using, None);
            assert_eq!(s.ai.furniture_of(hound), None, "nothing is kept for it");
        }
    };
    // Night, with a bed in the area.
    set_clock(&mut s, 2.0, 5.0);
    watch(&mut s, 60);
    // The afternoon, with seats.
    set_clock(&mut s, 16.5, 5.7);
    watch(&mut s, 300);
    // A meal with no seat it can use: no meal, the day's choices go on (#2011).
    let at = lunch(&s, R_HOUND);
    set_clock(&mut s, at, 5.7);
    let before = doings(&s, R_HOUND).len();
    watch(&mut s, 240);
    assert!(!matches!(s.ai.agents[&hound].sandbox.as_ref().unwrap().doing, Doing::Eating(_)));
    let did = doings(&s, R_HOUND);
    assert!(did.len() > before + 1, "choices went on through the meal: {:?}", &did[before..]);
    assert!(did.iter().all(|d| matches!(d, Doing::Wandering | Doing::Standing)), "{did:?}");
    // Told once for each piece, however many choices were made.
    let mut told = eligibility(&s, R_HOUND);
    told.sort();
    let (no_clips, marker) = ("no-clips-for-skeleton", "casual-marker-10");
    assert_eq!(told, [(CHAIR_NEAR, "excluded", no_clips), (CHAIR_FAR, "excluded", no_clips), (BED_D, "excluded", no_clips), (CROSS_D, "excluded", marker)]);
}

#[test]
fn supported_creature_can_sit_and_sleep() {
    let f = fixture();
    let mut s = sim(&f, &[R_HOUND]);
    s.ai.set_furniture_facts(facts(|_, _, _| Fit::Supported));
    let hound = FormId(R_HOUND);
    set_clock(&mut s, 2.0, 5.0);
    s.run(30.0);
    assert_eq!(s.ai.agents[&hound].sandbox.as_ref().unwrap().doing, Doing::Sleeping(FormId(BED_D)));
    assert_eq!(s.ai.agents[&hound].using, Some(FormId(BED_D)));
    assert_eq!(s.ai.furniture_of(hound), Some(FormId(BED_D)));
    set_clock(&mut s, 16.5, 5.7);
    s.run(600.0);
    assert!(doings(&s, R_HOUND).iter().any(|d| matches!(d, Doing::Sitting(c) if [CHAIR_NEAR, CHAIR_FAR].contains(&c.0))));
}

/// What nothing is known of stays as it was: the same choices as a run without facts
/// (the same random numbers drawn), the cross among what it may choose, and one line.
#[test]
fn unknown_creature_preserves_eligibility() {
    let f = fixture();
    let run = |given: Option<Arc<dyn FurnitureFacts>>| {
        let mut s = sim(&f, &[R_HOUND]);
        if let Some(given) = given {
            s.ai.set_furniture_facts(given);
        }
        s.run(1500.0);
        s
    };
    let (without, unknown) = (run(None), run(Some(Arc::new(NoFacts))));
    assert!(doings(&without, R_HOUND).iter().any(|d| d.furniture().is_some()));
    assert_eq!(doings(&without, R_HOUND), doings(&unknown, R_HOUND));
    assert_eq!(without.feet(R_HOUND), unknown.feet(R_HOUND));
    assert!(eligibility(&without, R_HOUND).is_empty());
    let told = eligibility(&unknown, R_HOUND);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!((told[0].1, told[0].2), ("unknown", "no-evidence"));
}

/// A person with facts for it, no cross near: nothing changes, down to the random numbers.
#[test]
fn a_person_with_no_cross_around_chooses_as_without_facts() {
    let f = fixture();
    let run = |given: bool| {
        let mut s = sim(&f, &[R_SB_ALL]);
        if given {
            s.ai.set_furniture_facts(facts(|_, _, _| Fit::Supported));
        }
        s.run(600.0);
        (doings(&s, R_SB_ALL), s.feet(R_SB_ALL), eligibility(&s, R_SB_ALL))
    };
    let (without, with) = (run(false), run(true));
    assert!(without.0.iter().any(|d| d.furniture().is_some()));
    assert_eq!(without, with);
    assert!(with.2.is_empty());
}

#[test]
fn sandbox_excludes_marker_10_for_people_and_creatures() {
    let f = fixture();
    let on_cross = |s: &Sim, actor: u32| doings(s, actor).iter().any(|d| d.furniture() == Some(FormId(CROSS_D)));
    let run = |given: bool| {
        let mut s = sim(&f, &[R_HOUND, R_PERSON_D]);
        // The afternoon: neither a meal nor the night.
        set_clock(&mut s, 16.5, 5.7);
        if given {
            s.ai.set_furniture_facts(facts(|_, _, _| Fit::Supported));
        }
        s.run(3000.0);
        s
    };
    // Without the rule the cross is one seat among the others.
    let free = run(false);
    assert!(on_cross(&free, R_HOUND) || on_cross(&free, R_PERSON_D));
    let s = run(true);
    for actor in [R_HOUND, R_PERSON_D] {
        assert!(!on_cross(&s, actor), "{:?}", doings(&s, actor));
        assert!(doings(&s, actor).iter().any(|d| matches!(d, Doing::Sitting(_))), "the seats stay");
        assert_eq!(eligibility(&s, actor), [(CROSS_D, "excluded", "casual-marker-10")], "told once");
    }
}

#[test]
fn a_linked_cross_stays_assigned_where_the_actor_fits() {
    let f = fixture();
    // A person, with the cross known for what it is: on it.
    let mut s = sim(&f, &[R_VICTIM]);
    s.ai.set_furniture_facts(facts(|_, _, _| Fit::Supported));
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_VICTIM)].using, Some(FormId(CROSS_LINK)));
    assert!(eligibility(&s, R_VICTIM).is_empty());
    // Nothing known of the creature: as before.
    let mut s = sim(&f, &[R_HOUND_LINKED]);
    s.ai.set_furniture_facts(Arc::new(NoFacts));
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_HOUND_LINKED)].using, Some(FormId(CROSS_LINK)));
    // No clips for it: the link does not put it there, and nothing is kept for it.
    let mut s = sim(&f, &[R_HOUND_LINKED]);
    s.ai.set_furniture_facts(facts(|_, _, _| Fit::Unsupported));
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_HOUND_LINKED)].using, None);
    assert_eq!(s.ai.furniture_of(FormId(R_HOUND_LINKED)), None);
    assert_eq!(eligibility(&s, R_HOUND_LINKED), [(CROSS_LINK, "excluded", "no-clips-for-skeleton")]);
}

#[test]
fn an_unsupported_nearest_piece_does_not_hide_a_supported_farther_one() {
    let f = fixture();
    let fit: fn(ActorKind, &str, FormId) -> Fit = |_, _, base| if base.0 == CHAIR_2 { Fit::Supported } else { Fit::Unsupported };
    let mut s = sim(&f, &[R_HOUND]);
    s.ai.set_furniture_facts(facts(fit));
    s.run(1.0);
    let at = lunch(&s, R_HOUND);
    set_clock(&mut s, at, 5.0);
    s.run(240.0);
    assert_eq!(s.ai.agents[&FormId(R_HOUND)].sandbox.as_ref().unwrap().doing, Doing::Eating(Some(FormId(CHAIR_FAR))));
    assert_eq!(s.ai.furniture_of(FormId(R_HOUND)), Some(FormId(CHAIR_FAR)));
    // An Eat package's search: the seat beyond the nearest.
    let mut s = sim(&f, &[R_EATER_CHAIRS]);
    s.ai.set_furniture_facts(facts(fit));
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_EATER_CHAIRS)].using, Some(FormId(CHAIR_FAR)));
    // Without facts, the nearest.
    let mut s = sim(&f, &[R_EATER_CHAIRS]);
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_EATER_CHAIRS)].using, Some(FormId(CHAIR_NEAR)));
}

#[test]
fn a_package_searching_for_a_seat_does_not_take_a_cross() {
    let f = fixture();
    let mut s = sim(&f, &[R_EATER_CROSS]);
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_EATER_CROSS)].using, Some(FormId(CROSS_D)));
    let mut s = sim(&f, &[R_EATER_CROSS]);
    s.ai.set_furniture_facts(facts(|_, _, _| Fit::Supported));
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_EATER_CROSS)].using, None);
    assert_eq!(s.ai.furniture_of(FormId(R_EATER_CROSS)), None);
    assert_eq!(eligibility(&s, R_EATER_CROSS), [(CROSS_D, "excluded", "casual-marker-10")]);
}

/// A run whose assets could not be read.
struct NoFacts;

impl FurnitureFacts for NoFacts {
    fn marker_ids(&self, _: FormId) -> Option<&[u8]> {
        None
    }
    fn fit(&self, _: ActorKind, _: &str, _: FormId) -> Fit {
        Fit::Unknown
    }
}

/// The rule's answers by kind: a creature out, a person not known. The person sits and the
/// creature does not, side by side, and the kinds are what the records make them: a person
/// on a creature's template stays a person, a creature on a template stays a creature.
#[test]
fn people_are_never_ruled_out_by_what_a_folder_lacks() {
    let f = fixture();
    let fit: fn(ActorKind, &str, FormId) -> Fit = |kind, skeleton, _| {
        if kind != ActorKind::Creature {
            return Fit::Unknown;
        }
        assert_eq!(skeleton, HOUND_MODEL, "a template's skeleton");
        Fit::Unsupported
    };
    let mut s = sim(&f, &[R_HOUND, R_PERSON_D, R_HOUND_MAN, R_HOUND_COPY]);
    s.ai.set_furniture_facts(facts(fit));
    set_clock(&mut s, 2.0, 5.0);
    s.run(30.0);
    // One bed, and a person gets it.
    let person_abed = [R_PERSON_D, R_HOUND_MAN].iter().filter(|a| s.ai.agents[&FormId(**a)].using == Some(FormId(BED_D))).count();
    assert_eq!(person_abed, 1);
    for creature in [R_HOUND, R_HOUND_COPY] {
        assert_eq!(s.ai.agents[&FormId(creature)].using, None);
        assert_eq!(s.ai.furniture_of(FormId(creature)), None);
        assert!(eligibility(&s, creature).iter().all(|e| e.1 == "excluded"), "{:?}", eligibility(&s, creature));
    }
    for person in [R_PERSON_D, R_HOUND_MAN] {
        let told = eligibility(&s, person);
        assert!(told.iter().all(|e| e.2 != "no-clips-for-skeleton"), "{told:?}");
        assert_eq!(told.iter().filter(|e| e.1 == "unknown").count(), 1, "{told:?}");
    }
}

/// What an actor is comes from its own record, a leveled base's pick included; only its
/// skeleton comes from the template. Both ways round: the leveled person on a creature's
/// model must not be ruled out as a creature, the creature on a person's model must not
/// pass as a person.
#[test]
fn what_an_actor_is_comes_from_its_own_record_and_its_skeleton_from_its_template() {
    use fnv_game::nvse_nx::{is_creature, skeleton_model};
    let f = fixture();
    let s = sim(&f, &[]);
    for (actor, creature, skeleton) in [
        (R_LEVELED_MAN, false, HOUND_MODEL),
        (R_HOUND_MAN, false, HOUND_MODEL),
        (R_MAN_HOUND, true, PERSON_MODEL),
        (R_HOUND_COPY, true, HOUND_MODEL),
        (R_HOUND, true, HOUND_MODEL),
    ] {
        assert_eq!(is_creature(&s.game, FormId(actor)), Some(creature), "{actor:X}");
        assert_eq!(skeleton_model(&s.game, FormId(actor)), skeleton, "{actor:X}");
    }
    // And by the rule: the person gets the bed, the creature gets nothing.
    let mut s = sim(&f, &[R_LEVELED_MAN, R_MAN_HOUND]);
    s.ai.set_furniture_facts(facts(|kind, skeleton, _| match kind {
        ActorKind::Person if skeleton == HOUND_MODEL => Fit::Unknown,
        ActorKind::Creature if skeleton == PERSON_MODEL => Fit::Unsupported,
        _ => panic!("{kind:?} {skeleton}"),
    }));
    set_clock(&mut s, 2.0, 5.0);
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_LEVELED_MAN)].using, Some(FormId(BED_D)));
    assert_eq!(s.ai.agents[&FormId(R_MAN_HOUND)].using, None);
    assert_eq!(s.ai.furniture_of(FormId(R_MAN_HOUND)), None);
}

// ---- feet that do not fit the goal's height (#1500; docs/design/navmesh.md §4.1) ----------

/// A strip of quads through `sections` (a left and a right point each, walking along it):
/// one navmesh whatever lies under what.
fn strip(form_id: u32, sections: &[([f32; 3], [f32; 3])]) -> NavMeshRecord {
    let vertices = sections.iter().flat_map(|(l, r)| [*l, *r]).collect();
    let n = sections.len() - 1;
    let mut triangles = Vec::new();
    for k in 0..n {
        let (a, b, c, d) = ((2 * k) as u16, (2 * k + 1) as u16, (2 * k + 3) as u16, (2 * k + 2) as u16);
        let tri = |t: usize| t as i16;
        triangles.push(Triangle { v: [a, b, c], edges: [if k > 0 { tri(2 * k - 1) } else { -1 }, -1, tri(2 * k + 1)], flags: 0, cover: 0 });
        triangles.push(Triangle { v: [a, c, d], edges: [tri(2 * k), if k + 1 < n { tri(2 * k + 2) } else { -1 }, -1], flags: 0, cover: 0 });
    }
    NavMeshRecord { form_id, vertices, triangles, ..Default::default() }
}

/// Cell A with MarkA (350, 350, 0) on a deck, a floor 200 under the deck, and (`joined`) a
/// ramp and a landing between them.
fn deck_over_a_floor(joined: bool) -> NavGraph {
    let deck = [([800.0, 300.0, 0.0], [800.0, 400.0, 0.0]), ([0.0, 300.0, 0.0], [0.0, 400.0, 0.0])];
    let floor = [([0.0, 400.0, -200.0], [0.0, 0.0, -200.0]), ([400.0, 400.0, -200.0], [400.0, 0.0, -200.0])];
    let m = if joined {
        // Along the floor to the east, up the ramp, round the landing and back west.
        let mut s = floor.to_vec();
        s.extend([([800.0, 100.0, 0.0], [800.0, 0.0, 0.0]), ([800.0, 200.0, 0.0], [900.0, 100.0, 0.0]), ([800.0, 300.0, 0.0], [900.0, 400.0, 0.0])]);
        s.extend([([700.0, 300.0, 0.0], [700.0, 400.0, 0.0]), deck[1]]);
        vec![strip(1, &s)]
    } else {
        vec![strip(1, &floor), strip(2, &deck)]
    };
    let input: Vec<_> = m.iter().map(|m| (m, Some(CELL_A), Some(A))).collect();
    NavGraph::assemble(&input)
}

/// A caller's character movement that fell: at its first walk the actor's feet are 200
/// under the end of its path, and the walker takes it from there (as `PhysMover` hands an
/// actor over).
struct FallsUnderGoal(bool);

impl fnv_ai::Mover for FallsUnderGoal {
    fn walk(&mut self, _actor: FormId, walker: &mut fnv_nav::Walker, _nav: &NavGraph, _dt: f32) -> Option<fnv_nav::WalkEvent> {
        if !self.0 {
            let goal = walker.follower.as_ref().and_then(|f| f.destination()).expect("on a walk").pos;
            walker.feet = [goal[0], goal[1], goal[2] - 200.0];
            walker.tri = fnv_nav::graph::NONE;
            self.0 = true;
        }
        None
    }
}

const UNDER_MARK_A: [f32; 3] = [350.0, 350.0, -200.0];

/// `R_RUN` on its way along the deck to MarkA, over the floor; its package a script's if
/// `script`.
fn on_the_deck<'a>(f: &'a Fixture, joined: bool, script: bool) -> Sim<'a> {
    let mut s = sim(f, &[R_RUN]);
    s.nav = deck_over_a_floor(joined);
    if script {
        s.game.extra.entry(FormId(R_RUN)).or_default().script_package = Some(FormId(P_RUN));
    }
    s.ai.agents.get_mut(&FormId(R_RUN)).unwrap().walker.feet = [650.0, 350.0, 0.0];
    s.run(0.5);
    assert_eq!(s.ai.agents[&FormId(R_RUN)].activity, Activity::Travelling);
    s
}

#[test]
fn an_actor_fallen_under_its_goal_walks_back_by_the_ramp_from_where_it_stands() {
    let f = fixture();
    let mut s = on_the_deck(&f, true, false);
    let actor = FormId(R_RUN);
    s.game.tick();
    s.ai.update_moved(&mut s.game, &s.nav, 0.5, &mut FallsUnderGoal(false));
    // Not there; on a new way, from the floor.
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(!s.events(R_RUN).iter().any(|e| matches!(e, AiEvent::Arrived { .. })), "{:?}", s.events(R_RUN));
    assert_eq!(s.ai.agents[&actor].walker.trip.as_ref().unwrap().replans, 1);
    assert!(s.ai.agents[&actor].walker.follower.as_ref().unwrap().path.length > 1000.0);
    assert!((s.feet(R_RUN)[2] + 200.0).abs() < 1.0, "{:?}", s.feet(R_RUN));
    let (mut furthest, mut before) = (0.0f32, s.feet(R_RUN));
    for _ in 0..200 {
        s.run(0.5);
        let now = s.feet(R_RUN);
        furthest = furthest.max(now[0]);
        // No teleport: no faster than it runs, no steeper than the ramp.
        assert!(dist(now, before) <= 200.5, "{before:?} to {now:?}");
        assert!((now[2] - before[2]).abs() <= 0.5 * dist(now, before) + 1.0, "{before:?} to {now:?}");
        before = now;
        if s.ai.agents[&actor].activity != Activity::Travelling {
            break;
        }
    }
    assert!(furthest > 700.0, "not by the ramp: {furthest}");
    let ev = s.events(R_RUN);
    assert!(matches!(ev.last().unwrap(), AiEvent::Arrived { .. }), "{ev:?}");
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::HeightWaived { .. })), "{ev:?}");
    assert!(s.feet(R_RUN)[2].abs() < 1.0, "{:?}", s.feet(R_RUN));
    assert_eq!(s.ai.agents[&actor].walker.height_searches, 1);
}

#[test]
fn an_actor_whose_way_back_needs_a_door_arrives_without_asking_it() {
    let f = fixture();
    let mut s = on_the_deck(&f, true, false);
    let actor = FormId(R_RUN);
    // A door across the ramp, and it would refuse the actor; the deck is beyond its reach.
    let door = fnv_nav::InPlaceDoor {
        door: DOOR_A,
        space: A,
        position: [600.0, 100.0, -100.0],
        rotation: [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        scale: 1.0,
        bounds: ([-160.0, -4.0, 0.0], [160.0, 4.0, 200.0]),
    };
    s.nav.set_in_place_doors(vec![door], |_| None);
    let way = s.nav.find_path(fnv_nav::Place { space: A, pos: UNDER_MARK_A }, fnv_nav::Place { space: A, pos: [350.0, 350.0, 0.0] }, &fnv_nav::PathParams::default()).unwrap();
    assert_eq!(way.legs[0].door_stops.len(), 1, "that way stops at the door");
    let until = s.game.time + 600.0;
    s.game.door_gates.insert((actor, FormId(DOOR_A)), fnv_game::npc_doors::Gate { answer: fnv_game::npc_doors::DoorAnswer::Refused, until });
    s.game.tick();
    s.ai.update_moved(&mut s.game, &s.nav, 0.5, &mut FallsUnderGoal(false));
    s.run(0.5);
    let ev = s.events(R_RUN);
    assert_eq!(ev.iter().filter(|e| matches!(e, AiEvent::HeightWaived { at } if *at == UNDER_MARK_A)).count(), 1, "{ev:?}");
    assert!(matches!(ev.last().unwrap(), AiEvent::Arrived { package, .. } if package.0 == P_RUN), "{ev:?}");
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::DoorRefused { .. } | AiEvent::Unreachable { .. })), "{ev:?}");
    assert_eq!(s.feet(R_RUN), UNDER_MARK_A, "no teleport");
    assert_eq!(s.ai.agents[&actor].walker.height_searches, 1);
}

#[test]
fn a_script_travel_package_whose_new_way_fails_still_arrives() {
    let f = fixture();
    for script in [false, true] {
        let mut s = on_the_deck(&f, false, script);
        let actor = FormId(R_RUN);
        s.game.tick();
        s.ai.update_moved(&mut s.game, &s.nav, 0.5, &mut FallsUnderGoal(false));
        s.run(0.5);
        let ev = s.events(R_RUN);
        assert_eq!(ev.iter().filter(|e| matches!(e, AiEvent::HeightWaived { at } if *at == UNDER_MARK_A)).count(), 1, "{ev:?}");
        assert!(matches!(ev.last().unwrap(), AiEvent::Arrived { package, .. } if package.0 == P_RUN), "{ev:?}");
        assert!(!ev.iter().any(|e| matches!(e, AiEvent::Unreachable { .. })), "{ev:?}");
        assert_eq!(s.ai.agents[&actor].activity, Activity::There);
        assert_eq!(s.feet(R_RUN), UNDER_MARK_A, "no teleport");
        assert!(s.game.extra[&actor].package_goal_met);
    }
}

#[test]
fn a_sandbox_choice_whose_new_way_fails_still_completes() {
    let f = fixture();
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    let actor = FormId(R_SB_OBJECTS);
    for _ in 0..40 {
        s.run(0.5);
        if s.ai.agents[&actor].sandbox.is_some() && s.ai.agents[&actor].activity == Activity::Travelling {
            break;
        }
    }
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling, "on a sandbox's walk");
    s.game.tick();
    s.ai.update_moved(&mut s.game, &s.nav, 0.5, &mut FallsUnderGoal(false));
    let a = &s.ai.agents[&actor];
    assert_eq!(a.activity, Activity::There);
    assert!(a.sandbox.is_some(), "still in its sandbox");
    let ev = s.events(R_SB_OBJECTS);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::HeightWaived { .. })), "{ev:?}");
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::Unreachable { .. })), "{ev:?}");
    // What it chose has begun; had the walk failed it would stand and choose again.
    assert_ne!(a.sandbox.as_ref().unwrap().phase(), SandboxPhase::Standing);
}

#[test]
fn walks_on_the_navmesh_plan_no_new_path_and_waive_no_height() {
    let f = fixture();
    let mut s = sim(&f, &[R_TRAVEL, R_RUN, R_PATROL, R_LOOP, R_RADIUS, R_SB_ALL, R_SB_OBJECTS, R_TO_CHAIR, R_SEEKER]);
    for _ in 0..120 {
        s.run(1.0);
    }
    // Over all of it: the counts outlive a trip, which an arrival ends.
    let arrived = s.ai.events.iter().filter(|e| matches!(e.event, AiEvent::Arrived { .. })).count();
    assert!(arrived > 3, "{arrived}");
    for (id, a) in &s.ai.agents {
        assert_eq!((a.walker.height_searches, a.walker.height_waivers), (0, 0), "{id:?}");
    }
    let waived: Vec<_> = s.ai.events.iter().filter(|e| matches!(e.event, AiEvent::HeightWaived { .. })).map(|e| &e.event).collect();
    assert!(waived.is_empty(), "{waived:?}");
}

// ---- an object with no way to it comes back (#1504) ---------------------------------

const SECOND_CHAIR: u32 = 0x8A2;
const SECOND_CHAIR_AT: [f32; 3] = [10300.0, 10500.0, 0.0];
const SANDBOXER_AT: [f32; 3] = [10450.0, 10450.0, 0.0];
const CHAIR_AT: [f32; 3] = [10700.0, 10500.0, 0.0];

/// One sandboxer whose package leaves on furniture alone, in a cell with a marker and
/// nothing to use but one chair (or two).
fn lone_sandboxer(second_chair: bool) -> Fixture {
    let globs = [
        glob(0x801, "GameYear", 2281.0),
        glob(0x802, "GameMonth", 9.0),
        glob(0x803, "GameDay", 19.0),
        glob(0x804, "GameHour", 12.0),
        glob(G_DAYS, "GameDaysPassed", 5.0),
        glob(0x806, "TimeScale", 30.0),
    ]
    .concat();
    let furniture_only = type_flags::NO_WANDERING | type_flags::NO_EATING | type_flags::NO_SLEEPING | type_flags::NO_IDLE_MARKERS | type_flags::NO_CONVERSATION;
    let mut refs = [
        placed(b"REFR", MARK_C, "MarkC", STAT, MIDDLE_C, None),
        placed(b"REFR", CHAIR_C, "ChairC", CHAIR, CHAIR_AT, None),
        placed(b"ACHR", R_SB_OBJECTS, "SitterRef", N_SB_OBJECTS, SANDBOXER_AT, None),
    ]
    .concat();
    if second_chair {
        refs.extend(placed(b"REFR", SECOND_CHAIR, "ChairD", CHAIR, SECOND_CHAIR_AT, None));
    }
    let cell = [record(b"CELL", CELL_C, &[sub(b"EDID", z("CellC"))]), group(CELL_C.to_le_bytes(), 6, &group(CELL_C.to_le_bytes(), 9, &refs))].concat();
    let top = |sig: &[u8; 4], body: &[u8]| group(*sig, 0, body);
    fixture_of(
        [
            tes4(),
            top(b"GLOB", &globs),
            top(b"STAT", &record(b"STAT", STAT, &[sub(b"EDID", z("Marker"))])),
            top(b"FURN", &record(b"FURN", CHAIR, &[sub(b"EDID", z("Chair")), sub(b"MNAM", 0x4000_0001u32.to_le_bytes().to_vec())])),
            top(b"NPC_", &npc(N_SB_OBJECTS, "Sitter", &[P_SB_OBJECTS])),
            top(b"PACK", &pack_with(P_SB_OBJECTS, "SandboxFurniture", 0, SANDBOX, furniture_only, (0, MARK_C, 450), None)),
            top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
        ]
        .concat(),
    )
}

/// Cell C as squares of 100 with nothing between them.
fn islands(origins: &[[f32; 2]]) -> NavGraph {
    let meshes: Vec<NavMeshRecord> = origins.iter().enumerate().map(|(i, o)| grid(i as u32 + 1, *o, 1, 0.0)).collect();
    let input: Vec<_> = meshes.iter().map(|m| (m, Some(CELL_C), Some(C))).collect();
    NavGraph::assemble(&input)
}

/// The sandboxer's island and the chair's.
fn chair_cut_off() -> NavGraph {
    islands(&[[10400.0, 10400.0], [10650.0, 10450.0]])
}

/// The same cell walkable all over.
fn way_restored() -> NavGraph {
    NavGraph::assemble(&[(&grid(3, [10000.0, 10000.0], 10, 0.0), Some(CELL_C), Some(C))])
}

fn way_to(s: &Sim, to: [f32; 3]) -> bool {
    let at = |pos| Place { space: C, pos };
    s.nav.find_path(at(s.feet(R_SB_OBJECTS)), at(to), &PathParams::default()).is_ok()
}

/// Run `seconds` and say when the sandbox's random numbers moved. In these fixtures the
/// day never changes and a choice with nothing open draws none, so each time is one object
/// chosen: a failed search for a way to it unless a walk or a use follows. This needs no
/// count from the process-wide path probe.
fn run_noting_choices(s: &mut Sim, seconds: f64) -> Vec<f64> {
    let actor = FormId(R_SB_OBJECTS);
    let numbers = |s: &Sim| s.ai.agents[&actor].sandbox.as_ref().map(|sb| sb.schedule().rng);
    let mut times = Vec::new();
    for _ in 0..(seconds * 2.0) as usize {
        let before = numbers(s);
        s.run(0.5);
        if numbers(s) != before {
            times.push(s.game.time);
        }
    }
    times
}

/// The times the sandboxer was seen to choose to stand.
fn standing_times(s: &Sim) -> Vec<f64> {
    s.ai.events.iter().filter(|e| e.actor == FormId(R_SB_OBJECTS) && matches!(e.event, AiEvent::Sandbox { doing: Doing::Standing, .. })).map(|e| e.time).collect()
}

fn package_events(s: &Sim) -> usize {
    s.events(R_SB_OBJECTS).iter().filter(|e| matches!(e, AiEvent::Package { .. })).count()
}

/// The first failed search and its five seconds of standing, through the real updates.
fn fail_at_the_chair(s: &mut Sim) {
    s.nav = chair_cut_off();
    assert!(!way_to(s, CHAIR_AT), "the islands have no way between them");
    assert_eq!(run_noting_choices(s, 0.5), [0.5], "the first choice is the chair");
    assert!(s.ai.agents[&FormId(R_SB_OBJECTS)].walker.follower.is_none());
    assert!(doings(s, R_SB_OBJECTS).is_empty(), "no behaviour began: {:?}", s.events(R_SB_OBJECTS));
    assert_eq!(run_noting_choices(s, 5.0), [] as [f64; 0], "nothing else is open");
    assert_eq!(standing_times(s), [5.5], "the failed choice's five seconds, not the ten of nothing open");
}

/// A meal whose only seat has no way to it: the seat waits out its delay and the meal is
/// not held standing meanwhile; once there is a way, the meal on the seat [design, #2011].
#[test]
fn a_meal_whose_seat_has_no_way_to_it_waits_for_the_way() {
    let mut f = lone_sandboxer(false);
    f.data.package_defs.get_mut(&FormId(P_SB_OBJECTS)).unwrap().type_flags &= !type_flags::NO_EATING;
    // The day's lunch, as this sandbox rolls it (the same numbers every run).
    let mut probe = sim(&f, &[R_SB_OBJECTS]);
    probe.run(0.5);
    let at = lunch(&probe, R_SB_OBJECTS);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    set_clock(&mut s, at, 5.0);
    s.nav = chair_cut_off();
    // The first choice is the meal on the chair; there is no way to it, so nothing began.
    assert_eq!(run_noting_choices(&mut s, 0.5), [0.5]);
    assert!(doings(&s, R_SB_OBJECTS).is_empty(), "{:?}", s.events(R_SB_OBJECTS));
    // While the chair waits out its 60 seconds: no meal, choices go on (nothing else is
    // open in this package, so standing, ten seconds at a time).
    let did = through_a_seatless_meal(&mut s, R_SB_OBJECTS, 50);
    assert!(did.len() >= 3 && did.iter().all(|d| *d == Doing::Standing), "{did:?}");
    // The way is back: after the wait, the meal on the chair.
    s.nav = way_restored();
    let actor = FormId(R_SB_OBJECTS);
    for _ in 0..240 {
        if s.ai.agents[&actor].using.is_some() {
            break;
        }
        s.run(0.5);
    }
    assert_eq!(s.ai.agents[&actor].using, Some(FormId(CHAIR_C)), "{:?}", s.events(R_SB_OBJECTS));
    assert_eq!(s.ai.agents[&actor].sandbox.as_ref().unwrap().doing, Doing::Eating(Some(FormId(CHAIR_C))));
}

#[test]
fn a_sandbox_goes_back_to_a_failed_object_once_there_is_a_way() {
    let f = lone_sandboxer(false);
    assert!(f.data.issues.is_empty(), "{:?}", f.data.issues.to_json());
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    let actor = FormId(R_SB_OBJECTS);
    let chair = FormId(CHAIR_C);
    fail_at_the_chair(&mut s);
    let packages = package_events(&s);
    assert_eq!(packages, 1);

    // Only the graph changes: the sandbox and its choices are the same ones.
    s.nav = way_restored();
    assert!(way_to(&s, CHAIR_AT));
    // Not yet: the first failure's 60 seconds, and the choice after them, at 65.5.
    assert_eq!(run_noting_choices(&mut s, 59.5), [] as [f64; 0], "no retry before its time");
    assert_eq!(s.game.time, 65.0);
    assert!(doings(&s, R_SB_OBJECTS).iter().all(|d| *d == Doing::Standing), "{:?}", s.events(R_SB_OBJECTS));
    assert_eq!(run_noting_choices(&mut s, 0.5), [65.5], "tried again at the first choice after the wait");
    assert_eq!(doings(&s, R_SB_OBJECTS).last(), Some(&Doing::Sitting(chair)));
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);

    // It walks there and really uses the chair.
    for _ in 0..120 {
        if s.ai.agents[&actor].using.is_some() {
            break;
        }
        s.run(0.5);
    }
    let a = &s.ai.agents[&actor];
    assert_eq!(a.using, Some(chair), "{:?}", s.events(R_SB_OBJECTS));
    assert_eq!(a.activity, Activity::There);
    assert_eq!(a.sandbox.as_ref().unwrap().doing, Doing::Sitting(chair));
    assert!(dist(a.walker.feet, CHAIR_AT) < 40.0, "{:?}", a.walker.feet);
    let sat_down = s.game.time;

    // And sits out its time.
    for _ in 0..600 {
        if s.ai.agents[&actor].using.is_none() {
            break;
        }
        s.run(0.5);
    }
    assert_eq!(s.ai.agents[&actor].using, None, "the sitting ended");
    assert!(s.game.time - sat_down > 30.0, "a whole sit: {} to {}", sat_down, s.game.time);
    let furniture: Vec<Option<FormId>> = s.events(R_SB_OBJECTS).iter().filter_map(|e| match e {
        AiEvent::Furniture { furniture } => Some(*furniture),
        _ => None,
    }).collect();
    assert_eq!(furniture, [Some(chair), None]);
    assert_eq!(package_events(&s), packages, "recovery needs no new package");
}

#[test]
fn a_sandbox_tries_a_cut_off_object_ever_less_often_and_stands_between() {
    let f = lone_sandboxer(false);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    s.nav = chair_cut_off();
    let tries = run_noting_choices(&mut s, 3600.0);
    assert!(!way_to(&s, CHAIR_AT));
    // Each wait (60, 120, 240, 480, then 600) ends at the next look for something to do:
    // five seconds after the failure, then every ten.
    assert_eq!(tries, [0.5, 65.5, 190.5, 435.5, 920.5, 1525.5, 2130.5, 2735.5, 3340.5]);
    let gaps: Vec<f64> = tries.windows(2).map(|w| w[1] - w[0]).collect();
    assert_eq!(gaps, [65.0, 125.0, 245.0, 485.0, 605.0, 605.0, 605.0, 605.0]);
    // Between tries it stands, at those same times.
    let mut standing = Vec::new();
    for (i, t) in tries.iter().enumerate() {
        let end = tries.get(i + 1).copied().unwrap_or(s.game.time + 0.5);
        standing.extend(std::iter::successors(Some(t + 5.0), |at| Some(at + 10.0)).take_while(|at| *at < end));
    }
    assert_eq!(standing_times(&s), standing);
    // Nothing else happened to it.
    assert!(doings(&s, R_SB_OBJECTS).iter().all(|d| *d == Doing::Standing), "{:?}", s.events(R_SB_OBJECTS));
    assert!(!s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Furniture { .. })));
    assert_eq!(s.ai.agents[&FormId(R_SB_OBJECTS)].using, None);
    assert_eq!(s.feet(R_SB_OBJECTS), SANDBOXER_AT);
    assert_eq!(package_events(&s), 1);
}

#[test]
fn a_failed_objects_waits_start_over_once_it_has_been_used() {
    let f = lone_sandboxer(true);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    let actor = FormId(R_SB_OBJECTS);
    let chairs = [Doing::Sitting(FormId(CHAIR_C)), Doing::Sitting(FormId(SECOND_CHAIR))];
    let cut_off = || islands(&[[10400.0, 10400.0], [10650.0, 10450.0], [10250.0, 10450.0]]);
    // Long enough that both chairs have failed several times in a row.
    s.nav = cut_off();
    let tries = run_noting_choices(&mut s, 450.0);
    assert!(tries.len() >= 6, "{tries:?}");
    assert!(doings(&s, R_SB_OBJECTS).iter().all(|d| *d == Doing::Standing), "{:?}", s.events(R_SB_OBJECTS));
    // With a way, each is used in turn; stop on the second.
    s.nav = way_restored();
    for _ in 0..4000 {
        s.run(0.5);
        let used = doings(&s, R_SB_OBJECTS);
        if chairs.iter().all(|c| used.contains(c)) && s.ai.agents[&actor].using.is_some() {
            break;
        }
    }
    let first_used = doings(&s, R_SB_OBJECTS).into_iter().find(|d| chairs.contains(d)).unwrap();
    let sitting_on = s.ai.agents[&actor].sandbox.as_ref().unwrap().doing;
    assert!(chairs.contains(&sitting_on) && sitting_on != first_used, "{:?}", s.events(R_SB_OBJECTS));
    assert!(s.ai.agents[&actor].using.is_some());
    // Cut off again while sitting on the second. The first chair has just been used, so
    // its first failure from here waits the shortest time, not the stage it had reached.
    s.nav = cut_off();
    let tries = run_noting_choices(&mut s, 300.0);
    assert!(tries.len() >= 2, "{tries:?}");
    assert_eq!(tries[1] - tries[0], 65.0, "{tries:?}");
}

#[test]
fn failed_objects_are_retried_the_same_way_every_time() {
    let f = lone_sandboxer(false);
    let outcome = |s: &mut Sim, frames: usize| {
        let mut feet = Vec::new();
        for _ in 0..frames {
            s.run(0.5);
            feet.push(s.feet(R_SB_OBJECTS));
        }
        (serde_json::to_string(&s.ai.events).unwrap(), feet, s.ai.agents[&FormId(R_SB_OBJECTS)].sandbox.as_ref().unwrap().schedule())
    };
    let recovering = || {
        let mut s = sim(&f, &[R_SB_OBJECTS]);
        fail_at_the_chair(&mut s);
        s.nav = way_restored();
        let run = outcome(&mut s, 800);
        assert!(doings(&s, R_SB_OBJECTS).contains(&Doing::Sitting(FormId(CHAIR_C))));
        run
    };
    let cut_off = || {
        let mut s = sim(&f, &[R_SB_OBJECTS]);
        s.nav = chair_cut_off();
        let run = outcome(&mut s, 2400);
        assert!(!doings(&s, R_SB_OBJECTS).is_empty());
        run
    };
    assert_eq!(recovering(), recovering());
    assert_eq!(cut_off(), cut_off());
}

/// Combat takes the sandbox for one update and ends: its next choice comes at the next
/// update, whatever its timer said.
fn interrupt_for_one_update(s: &mut Sim) {
    let at = s.feet(R_SB_OBJECTS);
    fighting(s, R_SB_OBJECTS, [at[0], at[1] + 100.0, at[2]]);
    s.run(0.5);
    sandbox_cancelled(s, R_SB_OBJECTS);
    s.game.stop_combat(FormId(R_SB_OBJECTS));
}

/// A way found is not a behaviour begun: a walk given up before it arrives keeps the
/// object's record, so the next failure goes on from the stage it had reached.
#[test]
fn an_approach_given_up_before_arriving_keeps_the_objects_stage() {
    let f = lone_sandboxer(false);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    let actor = FormId(R_SB_OBJECTS);
    // Two failures: the second waits 120 seconds.
    s.nav = chair_cut_off();
    assert_eq!(run_noting_choices(&mut s, 130.0), [0.5, 65.5]);
    // The next try finds a way and sets off.
    s.nav = way_restored();
    assert_eq!(run_noting_choices(&mut s, 60.5), [190.5]);
    assert_eq!(doings(&s, R_SB_OBJECTS).last(), Some(&Doing::Sitting(FormId(CHAIR_C))));
    assert_eq!(s.ai.agents[&actor].activity, Activity::Travelling);
    assert!(s.ai.agents[&actor].walker.follower.is_some());
    // Combat stops it before it arrives, and the way is gone again.
    interrupt_for_one_update(&mut s);
    s.nav = chair_cut_off();
    assert!(!way_to(&s, CHAIR_AT));
    // The third failure waits the third delay: 240 seconds, and five to the next look.
    let tries = run_noting_choices(&mut s, 300.0);
    assert_eq!(tries, [191.5, 436.5], "after the first delay it would be 256.5");
    assert!(!s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Furniture { .. })), "the chair was never used");
}

/// `Game::time == retry_at` is the first moment the object can be chosen. The looks of a
/// standing actor pass the deadline by (55.5, then 65.5, for 60.5), so an interruption
/// brings a choice onto the updates either side of it.
#[test]
fn a_failed_object_can_be_chosen_from_the_moment_its_wait_ends() {
    let f = lone_sandboxer(false);
    let choosing_at = |time: f64| {
        let mut s = sim(&f, &[R_SB_OBJECTS]);
        fail_at_the_chair(&mut s);
        s.nav = way_restored();
        let until_interrupted = time - 1.0 - s.game.time;
        assert_eq!(run_noting_choices(&mut s, until_interrupted), [] as [f64; 0]);
        interrupt_for_one_update(&mut s);
        let tries = run_noting_choices(&mut s, 0.5);
        let last = s.ai.events.iter().rev().find_map(|e| match e.event {
            AiEvent::Sandbox { doing, .. } => Some((e.time, doing)),
            _ => None,
        });
        (tries, last)
    };
    assert_eq!(choosing_at(60.0), (Vec::new(), Some((60.0, Doing::Standing))), "half a second before the deadline");
    assert_eq!(choosing_at(60.5), (vec![60.5], Some((60.5, Doing::Sitting(FormId(CHAIR_C))))), "at the deadline");
}

// ---- a sandbox's only object is used again after a while (#1505) --------------------

use fnv_ai::sandbox::SOLE_OBJECT_AGAIN_AFTER;

const SOLE_MARKER: u32 = 0x8A3;
const SOLE_MARKER_AT: [f32; 3] = [10500.0, 10250.0, 0.0];

/// The sandboxer's choices with their times, in order.
fn timed_doings(s: &Sim) -> Vec<(f64, Doing)> {
    s.ai.events.iter().filter(|e| e.actor == FormId(R_SB_OBJECTS)).filter_map(|e| match e.event {
        AiEvent::Sandbox { doing, .. } => Some((e.time, doing)),
        _ => None,
    }).collect()
}

/// The times the sandboxer got off furniture.
fn got_up_times(s: &Sim) -> Vec<f64> {
    s.ai.events.iter().filter(|e| e.actor == FormId(R_SB_OBJECTS) && matches!(e.event, AiEvent::Furniture { furniture: None })).map(|e| e.time).collect()
}

/// For each use of `object` after the first: the first standing choice after the use before
/// it ended (`ended`) and the use itself. Asserts that only standing came between.
fn sole_object_gaps(did: &[(f64, Doing)], object: Doing, ended: &[f64]) -> Vec<(f64, f64)> {
    let uses: Vec<f64> = did.iter().filter(|d| d.1 == object).map(|d| d.0).collect();
    assert!(ended.len() + 1 >= uses.len(), "{ended:?} {uses:?}");
    uses.iter().enumerate().skip(1).map(|(i, again)| {
        let between: Vec<&(f64, Doing)> = did.iter().filter(|d| d.0 >= ended[i - 1] && d.0 < *again).collect();
        assert!(!between.is_empty() && between.iter().all(|d| d.1 == Doing::Standing), "{between:?}");
        (between[0].0, *again)
    }).collect()
}

/// A sandboxer in cell C with a chair, an idle marker or both, its package leaving on only
/// what is there (no wandering, eating, sleeping or conversation).
fn sole_object_sandboxer(chair: bool, marker: bool) -> Fixture {
    let globs = [
        glob(0x801, "GameYear", 2281.0),
        glob(0x802, "GameMonth", 9.0),
        glob(0x803, "GameDay", 19.0),
        glob(0x804, "GameHour", 12.0),
        glob(G_DAYS, "GameDaysPassed", 5.0),
        glob(0x806, "TimeScale", 30.0),
    ]
    .concat();
    let mut off = type_flags::NO_WANDERING | type_flags::NO_EATING | type_flags::NO_SLEEPING | type_flags::NO_CONVERSATION;
    if !chair {
        off |= type_flags::NO_FURNITURE;
    }
    if !marker {
        off |= type_flags::NO_IDLE_MARKERS;
    }
    let mut refs = [placed(b"REFR", MARK_C, "MarkC", STAT, MIDDLE_C, None), placed(b"ACHR", R_SB_OBJECTS, "SitterRef", N_SB_OBJECTS, SANDBOXER_AT, None)].concat();
    if chair {
        refs.extend(placed(b"REFR", CHAIR_C, "ChairC", CHAIR, CHAIR_AT, None));
    }
    if marker {
        refs.extend(placed_with(b"REFR", SOLE_MARKER, "SoleMarker", IDLE_MARKER, SOLE_MARKER_AT, 1.25, None, None));
    }
    let cell = [record(b"CELL", CELL_C, &[sub(b"EDID", z("CellC"))]), group(CELL_C.to_le_bytes(), 6, &group(CELL_C.to_le_bytes(), 9, &refs))].concat();
    let top = |sig: &[u8; 4], body: &[u8]| group(*sig, 0, body);
    // A marker timing two idles ten seconds apart (no IDLE records: any skeleton fits).
    let idlm = record(b"IDLM", IDLE_MARKER, &[sub(b"EDID", z("LeanMarker")), sub(b"IDLF", vec![0]), sub(b"IDLC", vec![2]), sub(b"IDLT", 10f32.to_le_bytes().to_vec()), sub(b"IDLA", [0x8F0u32.to_le_bytes(), 0x8F1u32.to_le_bytes()].concat())]);
    fixture_of(
        [
            tes4(),
            top(b"GLOB", &globs),
            top(b"STAT", &record(b"STAT", STAT, &[sub(b"EDID", z("Marker"))])),
            top(b"FURN", &record(b"FURN", CHAIR, &[sub(b"EDID", z("Chair")), sub(b"MNAM", 0x4000_0001u32.to_le_bytes().to_vec())])),
            top(b"IDLM", &idlm),
            top(b"NPC_", &npc(N_SB_OBJECTS, "Sitter", &[P_SB_OBJECTS])),
            top(b"PACK", &pack_with(P_SB_OBJECTS, "SandboxSole", 0, SANDBOX, off, (0, MARK_C, 450), None)),
            top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
        ]
        .concat(),
    )
}

/// The issue's case: one chair and nothing else on. Once it has been sat on, it is not
/// chosen at the next look, but after a minute of standing it is again, and so on, in the
/// same sandbox (no new package).
#[test]
fn a_sandboxs_only_chair_is_sat_on_again_after_a_minute_of_something_else() {
    let f = lone_sandboxer(false);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    s.nav = way_restored();
    assert!(way_to(&s, CHAIR_AT));
    let actor = FormId(R_SB_OBJECTS);
    let chair = Doing::Sitting(FormId(CHAIR_C));
    s.run(900.0);
    let allowed = s.ai.agents[&actor].sandbox.as_ref().unwrap().allowed;
    assert!(allowed.furniture && !allowed.wandering && !allowed.idle_markers && !allowed.eating && !allowed.sleeping && !allowed.conversation);
    let did = timed_doings(&s);
    // Before #1505: one sit, then only standing (50 times in 600 seconds).
    assert!(did.iter().filter(|d| d.1 == chair).count() >= 3, "{did:?}");
    assert!(did.iter().all(|d| d.1 == chair || d.1 == Doing::Standing), "{did:?}");
    let ups = got_up_times(&s);
    for (first_stand, again) in sole_object_gaps(&did, chair, &ups) {
        // Never at once: the look on getting up stands, and so does every look for the
        // next minute; the first look a minute after that first stand takes the chair.
        assert!(ups.contains(&first_stand), "the look on getting up stands: {first_stand} {ups:?}");
        assert!(again - first_stand >= SOLE_OBJECT_AGAIN_AFTER, "{first_stand} to {again}");
        assert!(again - first_stand < SOLE_OBJECT_AGAIN_AFTER + 10.0, "the first look after the minute: {first_stand} to {again}");
    }
    let furniture: Vec<Option<FormId>> = s.events(R_SB_OBJECTS).iter().filter_map(|e| match e {
        AiEvent::Furniture { furniture } => Some(*furniture),
        _ => None,
    }).collect();
    assert!(furniture.iter().filter(|f| **f == Some(FormId(CHAIR_C))).count() >= 3, "really sat on: {furniture:?}");
    assert_eq!(package_events(&s), 1, "no new package or sandbox was needed");
}

/// The rule against choosing the object just used again at once still holds for the only
/// object: an interruption right after getting up does not let it through early.
#[test]
fn a_sandboxs_only_chair_is_not_chosen_again_at_once() {
    let f = lone_sandboxer(false);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    s.nav = way_restored();
    let chair = Doing::Sitting(FormId(CHAIR_C));
    // Until it gets up from the first sit.
    for _ in 0..1200 {
        s.run(0.5);
        if !got_up_times(&s).is_empty() {
            break;
        }
    }
    let up = got_up_times(&s)[0];
    assert_eq!(timed_doings(&s).last(), Some(&(up, Doing::Standing)));
    // Combat for an update brings an early look: still standing.
    s.run(20.0);
    interrupt_for_one_update(&mut s);
    s.run(0.5);
    let did = timed_doings(&s);
    assert!(did.iter().filter(|d| d.0 > up).all(|d| d.1 == Doing::Standing), "{did:?}");
    assert_eq!(did.iter().filter(|d| d.1 == chair).count(), 1, "{did:?}");
    // Up to the minute's end, no sit.
    let until = up + SOLE_OBJECT_AGAIN_AFTER - 0.5 - s.game.time;
    s.run(until);
    assert_eq!(timed_doings(&s).iter().filter(|d| d.1 == chair).count(), 1, "{:?}", timed_doings(&s));
    s.run(10.5);
    assert_eq!(timed_doings(&s).iter().filter(|d| d.1 == chair).count(), 2, "{:?}", timed_doings(&s));
    assert_eq!(package_events(&s), 1);
}

/// With two chairs the sandboxer goes from one to the other, never standing between and
/// never on the same one twice running, as before #1505.
#[test]
fn two_chairs_still_take_turns() {
    let f = lone_sandboxer(true);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    s.nav = way_restored();
    s.run(900.0);
    let did = doings(&s, R_SB_OBJECTS);
    assert!(did.len() >= 4, "{did:?}");
    let chairs = [Doing::Sitting(FormId(CHAIR_C)), Doing::Sitting(FormId(SECOND_CHAIR))];
    assert!(did.iter().all(|d| chairs.contains(d)), "{did:?}");
    assert!(did.windows(2).all(|w| w[0] != w[1]), "{did:?}");
}

/// A second chair that is disabled is no other object: the first is the only one open,
/// and comes back after the minute as a lone chair does.
#[test]
fn a_disabled_second_chair_leaves_the_first_as_the_only_one() {
    let f = lone_sandboxer(true);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    s.nav = way_restored();
    s.game.world.set_enabled(FormId(SECOND_CHAIR), false).unwrap();
    let chair = Doing::Sitting(FormId(CHAIR_C));
    s.run(900.0);
    let did = timed_doings(&s);
    assert!(did.iter().all(|d| d.1 == chair || d.1 == Doing::Standing), "{did:?}");
    assert!(did.iter().filter(|d| d.1 == chair).count() >= 3, "{did:?}");
    for (first_stand, again) in sole_object_gaps(&did, chair, &got_up_times(&s)) {
        assert!(again - first_stand >= SOLE_OBJECT_AGAIN_AFTER, "{first_stand} to {again}");
    }
}

/// [design, #2715] Any other allowed behaviour keeps the #1505 default even when
/// it has no available objects or wander route (`Sandbox::next`).
#[test]
fn a_sole_idle_marker_with_other_behaviours_keeps_the_minute_of_something_else() {
    for enabled in [type_flags::NO_WANDERING, type_flags::NO_EATING, type_flags::NO_SLEEPING, type_flags::NO_CONVERSATION, type_flags::NO_FURNITURE] {
        let mut f = sole_object_sandboxer(false, true);
        f.data.package_defs.get_mut(&FormId(P_SB_OBJECTS)).unwrap().type_flags &= !enabled;
        f.refs.refs.get_mut(&FormId(R_SB_OBJECTS)).unwrap().pos = SOLE_MARKER_AT;
        assert!(f.data.issues.is_empty(), "{:?}", f.data.issues.to_json());
        let mut s = sim(&f, &[R_SB_OBJECTS]);
        // [design: synthetic fixture] No navmesh: wandering is permitted but unavailable.
        s.nav = NavGraph::assemble(&[]);
        let marker = Doing::AtIdleMarker(FormId(SOLE_MARKER));
        s.run(900.0);
        let did = timed_doings(&s);
        assert!(did.iter().all(|d| d.1 == marker || d.1 == Doing::Standing), "{enabled:#x}: {did:?}");
        assert!(did.iter().filter(|d| d.1 == marker).count() >= 3, "{enabled:#x}: {did:?}");
        let ended: Vec<f64> = did.windows(2).filter(|w| w[0].1 == marker).map(|w| w[1].0).collect();
        for (first_stand, again) in sole_object_gaps(&did, marker, &ended) {
            assert!(again - first_stand >= SOLE_OBJECT_AGAIN_AFTER, "{first_stand} to {again}");
            assert!(again - first_stand < SOLE_OBJECT_AGAIN_AFTER + 10.0, "{first_stand} to {again}");
        }
        assert_eq!(package_events(&s), 1);
    }
}

/// [design, #2715] Idle-only packages reuse their sole marker at each expiry,
/// including a zero timer's default duration (`Sandbox::next`).
#[test]
fn an_idle_marker_only_sandbox_keeps_its_sole_marker_without_standing() {
    for timer in [0.0, 10.0] {
        let mut f = sole_object_sandboxer(false, true);
        f.data.idle_markers.get_mut(&FormId(IDLE_MARKER)).unwrap().timer = timer;
        f.refs.refs.get_mut(&FormId(R_SB_OBJECTS)).unwrap().pos = SOLE_MARKER_AT;
        let mut s = sim(&f, &[R_SB_OBJECTS]);
        let marker = Doing::AtIdleMarker(FormId(SOLE_MARKER));
        s.run(1.0);
        for _ in 0..360 {
            s.run(0.5);
            assert_eq!(s.ai.agents[&FormId(R_SB_OBJECTS)].sandbox.as_ref().unwrap().doing, marker);
        }
        let did = timed_doings(&s);
        assert!(did.len() >= 5, "timers expired repeatedly: {did:?}");
        assert!(did.iter().all(|d| d.1 == marker), "{did:?}");
        assert_eq!(package_events(&s), 1);
    }
}

/// [design, #2715] Restoring daily rolls does not introduce an initial marker wait
/// (`Ai::settle`, `Sandbox::start`, `Sandbox::set_schedule`).
#[test]
fn a_loaded_idle_marker_only_sandbox_picks_its_marker_on_the_first_update() {
    let mut f = sole_object_sandboxer(false, true);
    f.refs.refs.get_mut(&FormId(R_SB_OBJECTS)).unwrap().pos = SOLE_MARKER_AT;
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    let actor = FormId(R_SB_OBJECTS);
    s.run(40.0);
    let schedule = s.ai.agents[&actor].sandbox.as_ref().unwrap().schedule();
    let json = serde_json::to_string(&s.ai.progress()).unwrap();
    let mut ai = Ai::new(AiConfig::default());
    ai.add(&s.game, actor).unwrap();
    ai.restore(serde_json::from_str(&json).unwrap());
    s.ai = ai;
    let loaded_at = s.game.time;
    s.run(0.5);
    let sb = s.ai.agents[&actor].sandbox.as_ref().unwrap();
    assert_eq!(sb.doing, Doing::AtIdleMarker(FormId(SOLE_MARKER)));
    assert_eq!(sb.times(), (schedule.sleep, schedule.meals));
    assert_eq!(timed_doings(&s), vec![(loaded_at + 0.5, Doing::AtIdleMarker(FormId(SOLE_MARKER)))]);
    assert!(s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Resumed { sandbox: true, .. })));
}

/// [design, #2715] Another eligible idle marker still takes priority over the last;
/// disabled markers remain excluded (`Sandbox::objects`, `Sandbox::next`).
#[test]
fn an_idle_marker_only_sandbox_alternates_markers_and_excludes_disabled_ones() {
    let mut f = sole_object_sandboxer(false, true);
    let second = FormId(SECOND_CHAIR);
    let mut placed = f.refs.refs[&FormId(SOLE_MARKER)].clone();
    placed.id = second;
    placed.pos[0] += 100.0;
    f.refs.refs.insert(second, placed);
    f.refs.by_cell.get_mut(&FormId(CELL_C)).unwrap().push(second);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    s.run(180.0);
    let did = doings(&s, R_SB_OBJECTS);
    let markers = [Doing::AtIdleMarker(FormId(SOLE_MARKER)), Doing::AtIdleMarker(second)];
    assert!(did.len() >= 5, "{did:?}");
    assert!(did.iter().all(|d| markers.contains(d)), "{did:?}");
    assert!(did.windows(2).all(|w| w[0] != w[1]), "{did:?}");
    s.game.world.set_enabled(FormId(SOLE_MARKER), false).unwrap();
    s.game.world.set_enabled(second, false).unwrap();
    s.ai.events.clear();
    s.run(90.0);
    let did = doings(&s, R_SB_OBJECTS);
    assert!(!did.is_empty() && did.iter().all(|d| *d == Doing::Standing), "{did:?}");
}

/// A chair and an idle marker: each is the other's "something else", so the actor goes
/// from one to the other without standing between, as before #1505.
#[test]
fn a_chair_and_an_idle_marker_still_take_turns_without_standing() {
    let f = sole_object_sandboxer(true, true);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    s.nav = way_restored();
    s.run(900.0);
    let did = doings(&s, R_SB_OBJECTS);
    assert!(did.len() >= 4, "{did:?}");
    let both = [Doing::Sitting(FormId(CHAIR_C)), Doing::AtIdleMarker(FormId(SOLE_MARKER))];
    assert!(did.iter().all(|d| both.contains(d)), "{did:?}");
    assert!(did.windows(2).all(|w| w[0] != w[1]), "{did:?}");
}

/// A second chair with no way to it is out of the choices while it waits out its delay
/// (#1504); in those waits the first chair is the only one open and comes back after the
/// minute. The cut-off chair is never used and the backoff goes on.
#[test]
fn a_cut_off_second_chair_leaves_the_first_as_the_only_one_between_its_tries() {
    let f = lone_sandboxer(true);
    let mut s = sim(&f, &[R_SB_OBJECTS]);
    // The sandboxer and the first chair on one walkable area, the second chair on its own.
    let main = grid(1, [10400.0, 10400.0], 4, 0.0);
    let apart = grid(2, [10250.0, 10450.0], 1, 0.0);
    s.nav = NavGraph::assemble(&[(&main, Some(CELL_C), Some(C)), (&apart, Some(CELL_C), Some(C))]);
    assert!(way_to(&s, CHAIR_AT));
    assert!(!way_to(&s, SECOND_CHAIR_AT));
    let chair = Doing::Sitting(FormId(CHAIR_C));
    let tries = run_noting_choices(&mut s, 1800.0);
    let did = timed_doings(&s);
    assert!(did.iter().all(|d| d.1 == chair || d.1 == Doing::Standing), "the second chair never begun: {did:?}");
    assert!(did.iter().filter(|d| d.1 == chair).count() >= 4, "{did:?}");
    for (first_stand, again) in sole_object_gaps(&did, chair, &got_up_times(&s)) {
        assert!(again - first_stand >= SOLE_OBJECT_AGAIN_AFTER, "{first_stand} to {again}");
    }
    // The choices that were neither a stand nor the first chair begun are the second
    // chair's failed tries; their gaps grow with its delays (60, 120, 240, ...).
    let begun: Vec<f64> = did.iter().map(|d| d.0).collect();
    let failed: Vec<f64> = tries.iter().copied().filter(|t| !begun.contains(t)).collect();
    assert!(failed.len() >= 3, "{failed:?} of {tries:?}");
    for (i, w) in failed.windows(2).enumerate() {
        let delay = [60.0, 120.0, 240.0, 480.0, 600.0][i.min(4)];
        assert!(w[1] - w[0] >= delay, "try {} after {}: {failed:?}", i + 1, w[0]);
    }
    assert!(!s.events(R_SB_OBJECTS).iter().any(|e| matches!(e, AiEvent::Furniture { furniture: Some(c) } if c.0 == SECOND_CHAIR)));
    assert_eq!(package_events(&s), 1);
}

// ---- #2122: how near a follower comes once its leader stops ---------------------------

/// Where a follower on its way stops (`carry::follow_stop_at`, [design, #2122]):
/// `FOLLOW_STOP_INSET` (150) inside its distance, not nearer than `COLUMN_GAP` (128), in
/// step or not; short distances and a column's gaps as before.
#[test]
fn a_follower_stops_well_inside_its_distance() {
    use fnv_ai::carry::follow_stop_at;
    let arrive = AiConfig::default().arrive;
    // Boone's 330, the pack brahmin's 450, DefaultFollowPlayerFar's 512: about the
    // original's arrivals (177 and about 300 for the first two).
    for (keep, at) in [(330.0, 180.0), (450.0, 300.0), (512.0, 362.0), (256.0, 128.0), (200.0, 128.0)] {
        assert_eq!(follow_stop_at(keep, arrive, false, false), at, "{keep}");
        assert_eq!(follow_stop_at(keep, arrive, true, false), at, "{keep} in step");
    }
    // At 128 and less: within the distance, or 64 inside it in step, as before.
    assert_eq!(follow_stop_at(128.0, arrive, false, false), 128.0);
    assert_eq!(follow_stop_at(128.0, arrive, true, false), 64.0);
    assert_eq!(follow_stop_at(80.0, arrive, false, false), 80.0);
    assert_eq!(follow_stop_at(80.0, arrive, true, false), arrive);
    // In a column the gap to the one ahead is kept as before.
    assert_eq!(follow_stop_at(128.0, arrive, false, true), 128.0);
    assert_eq!(follow_stop_at(128.0, arrive, true, true), 64.0);
}

/// The fixture with the companion's Accompany package (after the player) at `distance`.
fn companion_at(distance: i32) -> Fixture {
    let mut f = fixture();
    f.data.package_defs.get_mut(&FormId(P_WITH_PLAYER)).unwrap().target.as_mut().unwrap().count_distance = distance;
    f
}

/// Cell C's navmesh made 40 × 40 squares (4,000 units a side) from (10000, 10000), for
/// walks longer than the cell's own 1,000.
fn wide_c() -> NavGraph {
    let c = grid_of(3, [10000.0, 10000.0], 40, 100.0, 0.0);
    NavGraph::assemble(&[(&c, Some(CELL_C), Some(C))])
}

/// The companion 100 units behind the player in a wide cell C; the player walks `walk`
/// units north at 100 units/s, then stands for 12 s; an update every `dt` seconds.
/// Returns the sim and the gap at each update while the player stands.
fn walk_then_stand(f: &Fixture, walk: f32, dt: f64) -> (Sim<'_>, Vec<f32>) {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let game = Game::new(&f.data, world, GameConfig { strict: true, frame_seconds: dt, package_interval: Some(10.0), ..GameConfig::default() });
    let mut s = Sim { game, ai: Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() }), nav: wide_c() };
    let (mate, player) = (FormId(R_COMPANION), FormId::PLAYER_REF);
    for (r, y) in [(player, 10300.0), (mate, 10200.0)] {
        let p = s.game.world.get(r).unwrap();
        (p.cell, p.pos, p.rot) = (Some(FormId(CELL_C)), [10500.0, y, 0.0], [0.0; 3]);
    }
    s.ai.add(&s.game, mate).unwrap();
    let own = fnv_ai::Speeds { walk: 70.0, run: 300.0 };
    s.ai.set_speeds(mate, fnv_ai::ActorSpeeds { calm: own, alert: own, turn_rate: None, spot_turn_start: None });
    let step = |s: &mut Sim, moved: f32| {
        s.game.world.get(player).unwrap().pos[1] += moved;
        s.game.tick();
        s.ai.update(&mut s.game, &s.nav, dt as f32);
        dist(s.feet(R_COMPANION), s.game.world.peek(player).unwrap().pos)
    };
    step(&mut s, 0.0);
    for _ in 0..(walk as f64 / (100.0 * dt)).round() as usize {
        step(&mut s, (100.0 * dt) as f32);
    }
    let gaps = (0..(12.0 / dt) as usize).map(|_| step(&mut s, 0.0)).collect();
    (s, gaps)
}

/// A follower walking after the player comes in well inside its package's distance once
/// the player stops, and stands there (issue #2122: Boone came to 177 of his 330 in the
/// original; before, ours stood at 404 of 512 in a run of main). In step on the way: it
/// sets off once and does not stop and start.
#[test]
fn a_follower_comes_in_when_its_leader_stops() {
    let inset = |keep: f32| fnv_ai::carry::follow_stop_at(keep, AiConfig::default().arrive, false, false);
    for distance in [330, 450, 512, 256] {
        let f = companion_at(distance);
        for dt in [0.5, 0.1] {
            let (s, gaps) = walk_then_stand(&f, 2000.0, dt);
            let mate = FormId(R_COMPANION);
            let ev = s.events(R_COMPANION);
            let at = inset(distance as f32);
            // One Follow, one CaughtUp after the player stopped: no stop and start.
            assert_eq!(ev.iter().filter(|e| matches!(e, AiEvent::Follow { .. })).count(), 1, "{distance} {dt}: {ev:?}");
            let caught: Vec<_> = ev.iter().filter_map(|e| match e {
                AiEvent::CaughtUp { away, stop_at, .. } => Some((*away, *stop_at)),
                _ => None,
            }).collect();
            assert_eq!(caught.len(), 1, "{distance} {dt}: {ev:?}");
            assert_eq!(caught[0].1, at, "{distance} {dt}");
            assert_eq!(s.ai.agents[&mate].activity, Activity::There, "{distance} {dt}");
            // It stands at most one step (its pace, at most 160 units/s, over an update)
            // inside where it was to stop.
            let last = *gaps.last().unwrap();
            assert!(last <= at + 1.0 && last >= at - 160.0 * dt as f32, "{distance} {dt}: stands {last} from the player, to stop at {at}");
            assert!((last - caught[0].0).abs() < 1.0, "{distance} {dt}: it moved after it caught up");
            // And well inside the distance: about the original's arrivals for 330 and 450.
            if distance >= 330 {
                assert!(last < distance as f32 - 100.0, "{distance} {dt}: {last}");
            }
        }
    }
}

/// A short distance is kept as before: within it once the leader stops (in step, 64
/// units inside it).
#[test]
fn a_follower_with_a_short_distance_stops_as_before() {
    let f = companion_at(128);
    let (s, gaps) = walk_then_stand(&f, 1000.0, 0.5);
    let ev = s.events(R_COMPANION);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::CaughtUp { stop_at, .. } if *stop_at == 64.0 || *stop_at == 128.0)), "{ev:?}");
    let last = *gaps.last().unwrap();
    assert!((64.0 - 80.0..=128.0 + 1.0).contains(&last), "{last}");
}

/// A follower already standing outside its distance but within the slack does not set
/// off (in the original Boone stood 383 units off, his distance 330, after a door, and
/// did not set off: docs/spec-notes/npc-behaviour-compare.md); nor does it close in.
#[test]
fn a_follower_standing_within_the_slack_stays() {
    let f = companion_at(330);
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let game = Game::new(&f.data, world, GameConfig { strict: true, frame_seconds: 0.5, package_interval: Some(10.0), ..GameConfig::default() });
    let mut s = Sim { game, ai: Ai::new(AiConfig { walk_speed: 100.0, run_speed: 400.0, ..AiConfig::default() }), nav: wide_c() };
    let (mate, player) = (FormId(R_COMPANION), FormId::PLAYER_REF);
    for (r, y) in [(player, 10683.0), (mate, 10300.0)] {
        let p = s.game.world.get(r).unwrap();
        (p.cell, p.pos, p.rot) = (Some(FormId(CELL_C)), [10500.0, y, 0.0], [0.0; 3]);
    }
    s.ai.add(&s.game, mate).unwrap();
    s.run(15.0);
    let ev = s.events(R_COMPANION);
    assert!(!ev.iter().any(|e| matches!(e, AiEvent::Follow { .. })), "{ev:?}");
    assert_eq!(s.ai.agents[&mate].activity, Activity::There);
    assert!((dist(s.feet(R_COMPANION), s.game.world.peek(player).unwrap().pos) - 383.0).abs() < 1.0);
}

/// Puts the player at `at` in `actor`'s cell, detected by it in Combat, with a distance
/// to close to of 100 units (alert.md §5.2 tests).
fn player_fights(s: &mut Sim, actor: u32, at: [f32; 3]) {
    let cell = s.game.world.peek(FormId(actor)).and_then(|m| m.cell);
    {
        let p = s.game.world.get(FormId::PLAYER_REF).unwrap();
        (p.cell, p.pos) = (cell, at);
    }
    fighting(s, actor, at);
    s.game.awareness.engage.insert(FormId(actor), (100.0, 150.0));
}

/// Continue During Combat on a Travel package the actor has already arrived at: it fights
/// as any other actor, and goes back to its place after the fight [observed: Lanius left
/// his marker and fought, then stood on it again; a knife trooper on a stand-in-place
/// Travel chased 2,000 units; checks 105 to 108, alert.md §5.2, #2155].
#[test]
fn continue_during_combat_frees_an_actor_that_has_arrived() {
    let mut f = fixture();
    f.data.package_defs.get_mut(&FormId(P_RADIUS)).unwrap().flags |= flags::CONTINUE_DURING_COMBAT;
    let mut s = sim(&f, &[R_RADIUS]);
    s.run(30.0);
    assert_eq!(s.ai.agents[&FormId(R_RADIUS)].activity, Activity::There);
    let stood = s.feet(R_RADIUS);
    let target = [stood[0] + 500.0, stood[1], stood[2]];
    player_fights(&mut s, R_RADIUS, target);
    s.run(4.0);
    let ev = s.events(R_RADIUS);
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Close { target, .. } if *target == FormId::PLAYER_REF)), "{ev:?}");
    assert!(dist(s.feet(R_RADIUS), target) < 400.0, "it went for the player: {:?}", s.feet(R_RADIUS));
    // The fight over, it goes back to its package's place.
    s.game.awareness.actors.remove(&FormId(R_RADIUS));
    s.run(30.0);
    assert!(dist(s.feet(R_RADIUS), [350.0, 350.0, 0.0]) <= 150.0, "{:?}", s.feet(R_RADIUS));
}

/// Continue During Combat on a Guard: a target inside the guard radius of the post (the
/// package radius × 1.5, at least 384) is fought; one outside is not pursued, the actor
/// staying on its package [documented: GECK wiki "Combat Package Flags"; observed: check
/// 106, alert.md §5.2, #2155].
#[test]
fn continue_during_combat_guard_pursues_only_inside_its_radius() {
    let mut f = fixture();
    {
        let d = f.data.package_defs.get_mut(&FormId(P_RADIUS)).unwrap();
        d.ty = fnv_game::package::PackageType::Guard;
        d.flags |= flags::CONTINUE_DURING_COMBAT;
    }
    for (off, pursues) in [(250.0, true), (700.0, false)] {
        let mut s = sim(&f, &[R_RADIUS]);
        s.run(30.0);
        let post = [350.0, 350.0, 0.0];
        let target = [post[0] + off, post[1], post[2]];
        player_fights(&mut s, R_RADIUS, target);
        s.run(4.0);
        let ev = s.events(R_RADIUS);
        let closed = ev.iter().any(|e| matches!(e, AiEvent::Close { target, .. } if *target == FormId::PLAYER_REF));
        assert_eq!(closed, pursues, "player {off} from the post: {ev:?}");
        if !pursues {
            assert!(dist(s.feet(R_RADIUS), post) <= 200.0, "it stayed at its post: {:?}", s.feet(R_RADIUS));
        }
    }
}


/// [design, #2538] A cold load door inherits the authored lock across its XTEL.
#[test]
fn load_door_lock_reaches_both_nav_directions_and_resyncs() {
    use fnv_sim::refs::{Lock, Teleport};
    let mut f = fixture();
    let mut a = f.refs.get(FormId(DOOR_A)).unwrap().clone();
    a.base = FormId(SCRIPT_DOOR_BASE);
    a.teleport = Some(Teleport { door: FormId(DOOR_B), pos: [5050.0, 5030.0, 50.0], rot: [0.0; 3], no_alarm: false });
    let mut b = a.clone();
    b.id = FormId(DOOR_B);
    b.cell = Some(FormId(CELL_B));
    b.teleport.as_mut().unwrap().door = a.id;
    b.lock = Some(Lock { level: 255, key: None, leveled: false });
    f.refs.refs.insert(a.id, a);
    f.refs.refs.insert(b.id, b);
    let mut s = sim(&f, &[R_TRAVEL]);
    assert!(s.game.world.peek(FormId(DOOR_A)).is_none());
    assert!(s.ai.apply_door_locks(&mut s.game, &mut s.nav).is_some());
    for d in [DOOR_A, DOOR_B] {
        assert_eq!(s.nav.door_locks[&d], DoorLock { level: 255, key: None });
    }
    s.run(30.0);
    assert_eq!(s.feet(R_TRAVEL), START);
    s.game.world.unlock(FormId(DOOR_A)).unwrap();
    assert!(s.ai.apply_door_locks(&mut s.game, &mut s.nav).is_some());
    for d in [DOOR_A, DOOR_B] {
        assert!(!s.nav.door_locks.contains_key(&d));
    }
    s.run(40.0);
    assert_eq!(s.ai.agents[&FormId(R_TRAVEL)].walker.space, B);
}

#[test]
fn set_package_flag_location_and_travel_to_ref_reach_ai_carry() {
    let f = fixture();
    let trip_running = |s: &Sim| -> Option<bool> {
        s.events(R_TRAVEL).iter().find_map(|e| match e {
            AiEvent::Trip { running, .. } => Some(*running),
            _ => None,
        })
    };

    // Baseline: TravelToB does not have ALWAYS_RUN, so R_TRAVEL walks (running == false).
    let mut baseline = sim(&f, &[R_TRAVEL]);
    baseline.run(1.0);
    assert_eq!(trip_running(&baseline), Some(false));

    // SetPackageFlag bit 13 (ALWAYS_RUN) + SetPackageLocationReference to MarkA reach Ai::begin and Ai::plan.
    let mut edited = sim(&f, &[R_TRAVEL]);
    edited
        .game
        .run_query("Begin Function { }\nSetPackageFlag TravelToB 13 1\nSetPackageLocationReference TravelToB MarkA\nEnd\n", Default::default())
        .unwrap();
    edited.run(10.0);
    assert_eq!(trip_running(&edited), Some(true));
    assert_eq!(edited.ai.agents[&FormId(R_TRAVEL)].walker.space, A);
    assert!(dist(edited.feet(R_TRAVEL), [350.0, 350.0, 0.0]) < 20.0, "moved to MarkA: {:?}", edited.feet(R_TRAVEL));

    // Synthetic TravelToRef package lives only in Game::package_overrides (not GameData::package_defs)
    // and now reaches Ai::begin and Ai::plan with ALWAYS_RUN (flag 1).
    let mut travel = sim(&f, &[R_LONER]);
    let ctx = fnv_game::game::RunCtx { self_ref: Some(FormId(R_LONER)), ..Default::default() };
    travel
        .game
        .run_query("Begin Function { }\nTravelToRef MarkA 1\nEnd\n", ctx)
        .unwrap();
    travel.run(10.0);
    let loner_running = travel.events(R_LONER).iter().find_map(|e| match e {
        AiEvent::Trip { running, .. } => Some(*running),
        _ => None,
    });
    assert_eq!(loner_running, Some(true));
    assert!(dist(travel.feet(R_LONER), [350.0, 350.0, 0.0]) < 80.0, "TravelToRef moved R_LONER to MarkA: {:?}", travel.feet(R_LONER));
}
