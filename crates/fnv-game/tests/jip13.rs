//! JIP LN NVSE batch 13 (#1006; docs/spec-notes/jip-physics-13.md) on a synthetic plugin,
//! the engine's physics stood in for by `Game::set_phys_view`: velocities, layers and
//! properties, contacts, the ground, the requests to the engine, script-forced travel,
//! moves, PlaceAtCell, rotations, turning and the reticle.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nvse_jip13::{material_id, PhysBody, PhysRequest, PhysView};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const WRLD: u32 = 0x930;
const EXT: u32 = 0x932;
const STAT: u32 = 0x937;
const XMARKER: u32 = 0x938;
const BOX_REF: u32 = 0x93A;
const OTHER_REF: u32 = 0x93B;
const NPC: u32 = 0x93F;
const NPC_REF: u32 = 0x940;
const FLST: u32 = 0x941;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn edid(sig: &[u8; 4], fid: u32, name: &str, rest: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    record(sig, 0, fid, &[vec![sub(b"EDID", z(name))], rest.to_vec()].concat())
}

fn placed(sig: &[u8; 4], fid: u32, name: &str, base: u32, pos: [f32; 3], rot: [f32; 3]) -> Vec<u8> {
    let data = [f32b(pos[0]), f32b(pos[1]), f32b(pos[2]), f32b(rot[0]), f32b(rot[1]), f32b(rot[2])].concat();
    record(sig, 0, fid, &[sub(b"EDID", z(name)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn plugin_bytes() -> Vec<u8> {
    let npcs = [record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]), edid(b"NPC_", NPC, "TestNpc", &[])].concat();
    let stat = [edid(b"STAT", STAT, "TestStatic", &[]), edid(b"STAT", XMARKER, "XMarker", &[])].concat();
    let flst = edid(b"FLST", FLST, "TestList", &[sub(b"LNAM", u32s(&[OTHER_REF]))]);
    let wrld = edid(b"WRLD", WRLD, "TestWorld", &[]);
    let ext = edid(b"CELL", EXT, "TestExterior", &[sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[0, 0]))]);
    let refs = [
        placed(b"REFR", BOX_REF, "BoxRef", STAT, [100.0, 0.0, 0.0], [0.0; 3]),
        placed(b"REFR", OTHER_REF, "OtherRef", STAT, [0.0, 100.0, 0.0], [0.0; 3]),
        placed(b"ACHR", NPC_REF, "NpcRef", NPC, [0.0, 0.0, 0.0], [0.0; 3]),
    ]
    .concat();
    let ext_children = group(EXT.to_le_bytes(), 6, &group(EXT.to_le_bytes(), 9, &refs));
    let world_children = group(WRLD.to_le_bytes(), 1, &group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &[ext, ext_children].concat())));
    [tes4(), top(b"NPC_", &npcs), top(b"STAT", &stat), top(b"FLST", &flst), top(b"WRLD", &[wrld, world_children].concat())].concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jip13.esm".into()).unwrap()];
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
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(EXT));
    g.player_entered(Some(FormId(EXT)));
    g
}

const VARS: &str = "float fX\nfloat fY\nfloat fZ\nref rItem\nstring_var sOut\narray_var aList\n";

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("{VARS}Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

fn s(g: &mut Game, body: &str) -> String {
    match q(g, body) {
        XVal::Str(s) => s,
        v => panic!("{body}: {v:?}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// A box with one moving body on node "Box", turned a quarter about Z (its local x is
/// the world's y).
fn box_view() -> PhysView {
    let body = PhysBody {
        node: Some("Box".into()),
        layer: 4,
        dynamic: true,
        linvel: [70.0, 0.0, -14.0],
        angvel: [0.0, 0.0, std::f32::consts::TAU],
        rotation: [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        mass: 12.0,
        friction: 0.5,
        restitution: 0.25,
        linear_damping: 0.1,
        angular_damping: 0.05,
        inertia: [1.0, 2.0, 3.0],
    };
    PhysView { bodies: vec![body], contacts: vec![FormId(OTHER_REF)], grabbable: true, ground: None, phantom: false }
}

#[test]
fn bodies_through_the_view() {
    let f = fixture();
    let mut g = game(&f);
    // The first ask watches the reference; the engine tells it from the next frame.
    assert_eq!(n(&mut g, "BoxRef.GetCollisionObjLayerType \"Box\""), 0.0);
    assert!(g.phys_watched().any(|r| r == FormId(BOX_REF)));
    g.set_phys_view(FormId(BOX_REF), Some(box_view()));
    assert_eq!(n(&mut g, "BoxRef.GetCollisionObjLayerType \"box\""), 4.0);
    assert_eq!(n(&mut g, "BoxRef.GetCollisionObjProperty \"Box\" 0"), 0.5);
    assert_eq!(n(&mut g, "BoxRef.GetCollisionObjProperty \"Box\" 5"), 2.0);
    assert_eq!(n(&mut g, "BoxRef.GetCollisionObjProperty \"Box\" 7"), 12.0);
    assert_eq!(n(&mut g, "BoxRef.GetRigidBodyMass"), 12.0);
    assert_eq!(n(&mut g, "NpcRef.GetRigidBodyMass"), 0.0, "an actor's is 0 [documented]");
    // Static (non-dynamic) rigid body reports 0.0, not 100.0 [observed: w4c_b4_06_jip13_physics_queries].
    let mut static_view = box_view();
    static_view.bodies[0].dynamic = false;
    g.set_phys_view(FormId(OTHER_REF), Some(static_view));
    assert_eq!(n(&mut g, "OtherRef.GetRigidBodyMass"), 0.0, "static rigid body returns 0.0");
    // Revolutions a second; 7 game units a second the linear unit.
    assert!(close(n(&mut g, "BoxRef.GetAngularVelocity \"Box\" Z"), 1.0));
    q(&mut g, "let fX := 0\nlet aList := 0");
    let lin = s(&mut g, "BoxRef.GetLinearVelocityAlt \"Box\" fX fY fZ\nSetFunctionValue (sv_Construct \"%g %g %g\" fX fY fZ)");
    assert_eq!(lin, "10 0 -2");
    // Local: the body's x is the world's y, so the world's x speed is along its -y.
    let local = s(&mut g, "BoxRef.GetLinearVelocityAlt \"Box\" fX fY fZ 1\nSetFunctionValue (sv_Construct \"%g %g %g\" fX fY fZ)");
    assert_eq!(local, "0 -10 -2");
    assert_eq!(n(&mut g, "BoxRef.GetLinearVelocityAlt \"NoNode\" fX fY fZ"), 0.0);
    assert!(close(n(&mut g, "BoxRef.GetObjectVelocity X"), 70.0));
    // Contacts, by reference, list and type.
    assert_eq!(n(&mut g, "BoxRef.GetHasContact OtherRef"), 1.0);
    assert_eq!(n(&mut g, "BoxRef.GetHasContact NpcRef"), 0.0);
    assert_eq!(n(&mut g, "BoxRef.GetHasContact TestList"), 1.0);
    assert_eq!(n(&mut g, "BoxRef.GetHasContactBase TestStatic"), 1.0);
    assert_eq!(n(&mut g, "ar_Size (BoxRef.GetContactRefs)"), 1.0);
    assert_eq!(n(&mut g, "BoxRef.IsGrabbable"), 1.0);
    assert_eq!(n(&mut g, "BoxRef.GetHasPhantom"), 0.0);
    assert_eq!(n(&mut g, "NpcRef.GetHasPhantom"), 0.0, "an actor's is 0 [observed: Doc Mitchell]");

    // The setters go to the engine.
    let _ = g.take_phys_requests();
    q(&mut g, "BoxRef.SetCollisionObjLayerType \"*\" 15\nBoxRef.SetCollisionObjProperty \"Box\" 2 0.15\nBoxRef.SetAngularVelocity \"Box\" X 2\nBoxRef.SetLinearVelocity \"Box\" 1 0 0 1\nBoxRef.ToggleObjectCollision 0");
    let r = FormId(BOX_REF);
    let reqs = g.take_phys_requests();
    assert_eq!(reqs[0], PhysRequest::Layer { r, body: 0, layer: 15 });
    assert_eq!(reqs[1], PhysRequest::Property { r, body: 0, property: 2, value: 0.15 });
    match &reqs[2] {
        PhysRequest::Velocity { angvel: Some(w), linvel: None, .. } => assert!((w[0] - 2.0 * std::f32::consts::TAU).abs() < 1e-4 && (w[2] - std::f32::consts::TAU).abs() < 1e-4),
        other => panic!("{other:?}"),
    }
    // Local x (1 unit = 7 game units a second) is the world's y.
    match &reqs[3] {
        PhysRequest::Velocity { linvel: Some(v), angvel: None, .. } => assert!(v[0].abs() < 1e-4 && (v[1] - 7.0).abs() < 1e-4),
        other => panic!("{other:?}"),
    }
    assert_eq!(reqs[4], PhysRequest::Collision { r, on: false });
    assert!(g.collision_off(r));
}

#[test]
fn ground_materials_and_gravity() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(material_id(None), 1, "terrain: dirt");
    assert_eq!((material_id(Some(0)), material_id(Some(9)), material_id(Some(16)), material_id(Some(73))), (0, 5, 9, 5));
    assert_eq!(n(&mut g, "BoxRef.GetGroundMaterial"), -1.0, "not an actor");
    n(&mut g, "NpcRef.GetGroundMaterial");
    g.set_phys_view(FormId(NPC_REF), Some(PhysView { ground: Some((Some(FormId(BOX_REF)), Some(4))), ..PhysView::default() }));
    assert_eq!(n(&mut g, "NpcRef.GetGroundMaterial"), 2.0, "grass");
    assert_eq!(n(&mut g, "NpcRef.GetObjectUnderFeet"), f64::from(BOX_REF));
    g.set_phys_view(FormId(NPC_REF), Some(PhysView::default()));
    assert_eq!(n(&mut g, "NpcRef.GetGroundMaterial"), -1.0, "in the air");
    let _ = g.take_phys_requests();
    q(&mut g, "SetLocalGravityVector 0 0 -45");
    assert_eq!(g.take_phys_requests(), vec![PhysRequest::Gravity(Some([0.0, 0.0, -45.0]))]);
}

#[test]
fn pushes_turns_and_travel() {
    let f = fixture();
    let mut g = game(&f);
    let _ = g.take_phys_requests();
    q(&mut g, "NpcRef.PushActorNoRagdoll 10 90\nNpcRef.Turn 90\nNpcRef.FaceObject OtherRef");
    let reqs = g.take_phys_requests();
    let turns = g.take_turns();
    match &reqs[0] {
        PhysRequest::Shove { dir, speed, .. } => assert!((dir[0] - 1.0).abs() < 1e-5 && dir[1].abs() < 1e-5 && *speed == 300.0, "east, 10 × 30"),
        other => panic!("{other:?}"),
    }
    assert_eq!(turns.len(), 2);
    assert!((turns[0].1 - std::f32::consts::FRAC_PI_2).abs() < 1e-5 && turns[1].1.abs() < 1e-5, "{turns:?}");
    // TravelToRef gives the actor a Travel package to the reference.
    q(&mut g, "NpcRef.TravelToRef OtherRef 1");
    let p = g.extra.get(&FormId(NPC_REF)).and_then(|e| e.script_package).expect("a script package");
    let def = g.package_def(&p).expect("its definition");
    assert_eq!(def.ty, fnv_game::package::PackageType::Travel);
    assert_eq!(def.location.map(|l| l.kind), Some(fnv_game::package::LocationKind::NearReference(Some(FormId(OTHER_REF)))));
    assert_ne!(def.flags & fnv_game::package::flags::ALWAYS_RUN, 0);
    // MoveAwayFrom: to a marker made straight away from the other reference.
    q(&mut g, "NpcRef.MoveAwayFrom OtherRef 500");
    let p2 = g.extra.get(&FormId(NPC_REF)).and_then(|e| e.script_package).unwrap();
    assert_ne!(p, p2);
    let Some(fnv_game::package::LocationKind::NearReference(Some(m))) = g.package_def(&p2).and_then(|d| d.location).map(|l| l.kind) else { panic!("a marker") };
    let pos = g.world.get(m).unwrap().pos;
    assert!((pos[1] + 500.0).abs() < 1e-3 && pos[0].abs() < 1e-3, "{pos:?}");
}

#[test]
fn moves_and_rotations() {
    let f = fixture();
    let mut g = game(&f);
    // SetAngleEx: global angles, then face a point.
    q(&mut g, "BoxRef.SetAngleEx 0 0 90");
    let rot = g.world.get(FormId(BOX_REF)).unwrap().rot;
    assert!((rot[2] - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    q(&mut g, "BoxRef.SetAngleEx 100 100 0 4");
    let rot = g.world.get(FormId(BOX_REF)).unwrap().rot;
    assert!(rot[2].abs() < 1e-5 && rot[0].abs() < 1e-5, "faces north (+y): {rot:?}");
    // RotateAroundPoint: a quarter about Z at the origin moves (100, 0) to (0, 100).
    q(&mut g, "BoxRef.SetAngleEx 0 0 0\nBoxRef.RotateAroundPoint 0 0 0 0 0 90");
    let p = g.world.get(FormId(BOX_REF)).unwrap().pos;
    assert!(p[0].abs() < 1e-3 && (p[1] - 100.0).abs() < 1e-3, "{p:?}");
    // PlaceAtCell in a worldspace: at the position, in the cell there.
    let made = n(&mut g, "PlaceAtCell TestStatic 1 TestWorld 10 20 30");
    let s = g.world.get(FormId(made as u32)).unwrap();
    assert_eq!((s.cell, s.pos), (Some(FormId(EXT)), [10.0, 20.0, 30.0]));
    // MoveToFadeDelay: nothing until the delay is over.
    q(&mut g, "BoxRef.MoveToFadeDelay OtherRef 5");
    g.tick();
    assert!(g.world.get(FormId(BOX_REF)).unwrap().pos[1] > 50.0, "not yet moved to OtherRef's place");
    // The reticle: no hit, nothing.
    assert_eq!(n(&mut g, "GetReticlePosAlt fX fY fZ"), 0.0);
    g.senses.reticle = Some(fnv_game::functions::senses::Reticle { start: [0.0, 0.0, 100.0], dir: [0.0, 1.0, 0.0], hit: Some(50.0), limb: None });
    assert_eq!(s_of(&mut g, "GetReticlePosAlt fX fY fZ\nSetFunctionValue (sv_Construct \"%g %g %g\" fX fY fZ)"), "0 50 100");
    q(&mut g, "BoxRef.MoveToReticle 1000 0 0 10");
    assert_eq!(g.world.get(FormId(BOX_REF)).unwrap().pos, [0.0, 50.0, 110.0]);
    assert_eq!(n(&mut g, "BoxRef.MoveToReticle 10"), 0.0, "beyond the range");
}

fn s_of(g: &mut Game, body: &str) -> String {
    s(g, body)
}
