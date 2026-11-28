//! Regression tests verifying `fnv-ai` goal/furniture/arrival resolution honors runtime
//! `SetLinkedReference` (`Game::linked_ref`) across patrol chains, follow targets, and
//! `PlaceAtMe`-created actors.

use fnv_ai::goal::{resolve, Goal};
use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_nav::NavGraph;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL: u32 = 0x200;
const STAT_MARKER: u32 = 0x500;
const M1: u32 = 0x501;
const M2: u32 = 0x502;
const M3: u32 = 0x503;
const PKG_PATROL: u32 = 0x600;
const PKG_FOLLOW: u32 = 0x601;
const NPC_PATROL: u32 = 0x800;
const NPC_TARGET: u32 = 0x801;
const PATROL_REF: u32 = 0x300;
const TARGET_REF: u32 = 0x301;

const PATROL: u8 = 13;
const FOLLOW: u8 = 1;

fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
    Subrecord { sig: *sig, data, extended: false }
}

fn z(s: &str) -> Vec<u8> {
    [s.as_bytes(), &[0]].concat()
}

fn record(sig: &[u8; 4], fid: u32, subs: &[Subrecord]) -> Vec<u8> {
    let payload = write_subrecords(subs);
    [
        &sig[..],
        &(payload.len() as u32).to_le_bytes(),
        &0u32.to_le_bytes(),
        &fid.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        &payload,
    ]
    .concat()
}

fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
    [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
}

fn tes4() -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x900u32.to_le_bytes()].concat();
    let payload = write_subrecords(&[sub(b"HEDR", hedr)]);
    [&b"TES4"[..], &(payload.len() as u32).to_le_bytes(), &1u32.to_le_bytes(), &[0; 4], &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
}

fn placed(sig: &[u8; 4], fid: u32, edid: &str, base: u32, pos: [f32; 3]) -> Vec<u8> {
    let mut data = Vec::with_capacity(24);
    for v in pos {
        data.extend_from_slice(&v.to_le_bytes());
    }
    data.extend_from_slice(&[0u8; 12]);
    record(sig, fid, &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn plugin_bytes() -> Vec<u8> {
    let stat = record(b"STAT", STAT_MARKER, &[sub(b"EDID", z("PatrolMarkerBase"))]);
    let patrol_pkg = record(
        b"PACK",
        PKG_PATROL,
        &[
            sub(b"EDID", z("RuntimeLinkedPatrol")),
            sub(b"PKDT", [&0u32.to_le_bytes()[..], &[PATROL, 0, 0, 0], &[0; 4]].concat()),
            // LocationKind::NearLinked (type 6).
            sub(b"PLDT", [6i32.to_le_bytes(), 0u32.to_le_bytes(), 64i32.to_le_bytes()].concat()),
            sub(b"PKPT", 1u16.to_le_bytes().to_vec()),
        ],
    );
    let follow_pkg = record(
        b"PACK",
        PKG_FOLLOW,
        &[
            sub(b"EDID", z("RuntimeLinkedFollow")),
            sub(b"PKDT", [&0u32.to_le_bytes()[..], &[FOLLOW, 0, 0, 0], &[0; 4]].concat()),
            // TargetKind::LinkedReference (type 3), distance 128.
            sub(b"PTDT", [3i32.to_le_bytes(), 0u32.to_le_bytes(), 128i32.to_le_bytes(), 0f32.to_le_bytes()].concat()),
        ],
    );
    let npc_p = record(
        b"NPC_",
        NPC_PATROL,
        &[
            sub(b"EDID", z("PatrolNpc")),
            sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat()),
            sub(b"PKID", PKG_PATROL.to_le_bytes().to_vec()),
        ],
    );
    let npc_t = record(
        b"NPC_",
        NPC_TARGET,
        &[
            sub(b"EDID", z("TargetNpc")),
            sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat()),
        ],
    );
    let cell = record(b"CELL", CELL, &[sub(b"EDID", z("PatrolCell"))]);
    let refs = [
        placed(b"REFR", M1, "Marker1", STAT_MARKER, [100.0, 0.0, 0.0]),
        placed(b"REFR", M2, "Marker2", STAT_MARKER, [300.0, 0.0, 0.0]),
        placed(b"REFR", M3, "Marker3", STAT_MARKER, [300.0, 200.0, 0.0]),
        placed(b"ACHR", PATROL_REF, "PatrolRef", NPC_PATROL, [0.0, 0.0, 0.0]),
        placed(b"ACHR", TARGET_REF, "TargetRef", NPC_TARGET, [50.0, 50.0, 0.0]),
    ]
    .concat();
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs));
    [
        tes4(),
        group(*b"STAT", 0, &stat),
        group(*b"PACK", 0, &[patrol_pkg, follow_pkg].concat()),
        group(*b"NPC_", 0, &[npc_p, npc_t].concat()),
        group(*b"CELL", 0, &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat()))),
    ]
    .concat()
}

#[test]
fn runtime_set_linked_reference_resolves_patrol_route_and_follow_for_placed_and_created_actors() {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "PatrolLink.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    let world = World::new(&forms, &refs, SimConfig::default());
    let mut g = Game::new(&data, world, GameConfig::default());
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(CELL));
    g.player_entered(Some(FormId(CELL)));

    let nav = NavGraph::default();
    let patrol_def = g.data.package_defs.get(&FormId(PKG_PATROL)).unwrap();
    let follow_def = g.data.package_defs.get(&FormId(PKG_FOLLOW)).unwrap();

    // Without links, neither Patrol nor Follow can resolve a linked target.
    assert!(matches!(resolve(&g, &nav, FormId(PATROL_REF), patrol_def), Goal::Missing(_)));
    assert!(matches!(resolve(&g, &nav, FormId(PATROL_REF), follow_def), Goal::Missing(_)));

    // Link PatrolRef -> Marker1 -> Marker2 -> Marker3 -> Marker1 via runtime SetLinkedReference.
    g.run_query(
        "Begin Function {}\nPatrolRef.SetLinkedReference Marker1\nMarker1.SetLinkedReference Marker2\nMarker2.SetLinkedReference Marker3\nMarker3.SetLinkedReference Marker1\nEnd\n",
        RunCtx::default(),
    )
    .unwrap();

    match resolve(&g, &nav, FormId(PATROL_REF), patrol_def) {
        Goal::Route(route) => {
            let ids: Vec<FormId> = route.points.iter().map(|(id, _)| *id).collect();
            assert_eq!(ids, vec![FormId(M1), FormId(M2), FormId(M3)]);
            assert_eq!(route.loops_to, Some(0));
            assert!(route.repeatable);
        }
        other => panic!("expected Goal::Route for runtime-linked placed actor, got {other:?}"),
    }

    // A PlaceAtMe-created actor (not in RefTable) linked at runtime resolves the same full route.
    let XVal::Ref(created_id) = g
        .run_query(
            "Begin Function {}\nref r\nlet r := Marker1.PlaceAtMe PatrolNpc 1\nr.SetLinkedReference Marker1\nSetFunctionValue r\nEnd\n",
            RunCtx::default(),
        )
        .unwrap()
    else {
        panic!("PlaceAtMe must return a reference");
    };
    let created = FormId(created_id);
    assert!(created >= FormId::FIRST_DYNAMIC);
    match resolve(&g, &nav, created, patrol_def) {
        Goal::Route(route) => {
            let ids: Vec<FormId> = route.points.iter().map(|(id, _)| *id).collect();
            assert_eq!(ids, vec![FormId(M1), FormId(M2), FormId(M3)]);
            assert_eq!(route.loops_to, Some(0));
        }
        other => panic!("expected Goal::Route for PlaceAtMe runtime-linked actor, got {other:?}"),
    }

    // Follow package targeting LinkedReference also honors runtime SetLinkedReference.
    g.run_query("Begin Function {}\nPatrolRef.SetLinkedReference TargetRef\nEnd\n", RunCtx::default()).unwrap();
    match resolve(&g, &nav, FormId(PATROL_REF), follow_def) {
        Goal::Follow(f) => {
            assert_eq!(f.target, FormId(TARGET_REF));
            assert_eq!(f.distance, 128.0);
        }
        other => panic!("expected Goal::Follow to TargetRef, got {other:?}"),
    }
}
