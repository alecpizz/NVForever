//! [design, #3662] Authored XTRI 23 (Projectile Zone) activator primitives crossed by
//! real hitscan and multi-frame flying shots through `script_trigger::update` and
//! `combat::fly`. Verifies:
//! - An enabled loaded XTRI 23 box activator receives `OnHit` once per shot (hitscan or
//!   multi-frame missile) without stopping the shot from hitting a bottle behind it.
//! - A shot passing beside the box does not trigger it.
//! - A disabled XTRI 23 activator receives no hit.
//! - Primitives on other layers (XTRI 22 ActorZone, or no XTRI) are unaffected.
//! - If an earlier crossed zone disables a second zone behind it in `OnHit`, the second
//!   zone does not fire on that same shot.

use super::*;
use fnv_collide::{CollisionObject, Iso, Motion as CollideMotion, Shape};
use fnv_game::game::event;
use fnv_phys::RefPose;
use fnv_script::vm::Value;

const HITSCAN_GUN: FormId = FormId(0x940);
const HITSCAN_PROJ: FormId = FormId(0x941);
const MISSILE_GUN: FormId = FormId(0x942);
const MISSILE_PROJ: FormId = FormId(0x943);

const ACTI_ZONE: FormId = FormId(0x950);
const ACTI_DISABLE_NEXT: FormId = FormId(0x951);
const ACTI_BOTTLE: FormId = FormId(0x952);
const SCPT_COUNT: FormId = FormId(0x955);
const SCPT_DISABLE_NEXT: FormId = FormId(0x956);

const ZONE_A: FormId = FormId(0x960);
const ZONE_DISABLED: FormId = FormId(0x961);
const ZONE_ACTOR_LAYER: FormId = FormId(0x962);
const ZONE_NO_XTRI: FormId = FormId(0x963);
const ZONE_FIRST: FormId = FormId(0x964);
const ZONE_SECOND: FormId = FormId(0x965);
const BOTTLE: FormId = FormId(0x966);

fn le16(v: u16) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn st(op: u16, body: &[u8]) -> Vec<u8> {
    [le16(op), le16(body.len() as u16), body.to_vec()].concat()
}
fn ex(tokens: &[Vec<u8>]) -> Vec<u8> {
    tokens.iter().flat_map(|t| [vec![b' '], t.clone()].concat()).collect()
}
fn set_short(idx: u16, e: Vec<u8>) -> Vec<u8> {
    st(0x15, &[[vec![b's'], le16(idx)].concat(), le16(e.len() as u16), e].concat())
}
fn script_unit(scda: &[u8], vars: &[(u32, &str, u8)], refs: &[u32]) -> Vec<Subrecord> {
    let mut schr = vec![0u8; 4];
    schr.extend((refs.len() as u32).to_le_bytes());
    schr.extend((scda.len() as u32).to_le_bytes());
    schr.extend((vars.len() as u32).to_le_bytes());
    schr.extend(0u16.to_le_bytes()); // object script
    schr.extend(1u16.to_le_bytes()); // enabled
    let mut out = vec![sub(b"SCHR", schr), sub(b"SCDA", scda.to_vec())];
    for (i, n, t) in vars {
        let mut slsd = i.to_le_bytes().to_vec();
        slsd.extend([0u8; 12]);
        slsd.push(*t);
        slsd.extend([0u8; 7]);
        out.push(sub(b"SLSD", slsd));
        out.push(sub(b"SCVR", z(n)));
    }
    for r in refs {
        out.push(sub(b"SCRO", r.to_le_bytes().to_vec()));
    }
    out
}
fn on_hit_script(body: Vec<u8>) -> Vec<u8> {
    let end = st(0x11, &[]);
    let len = (body.len() + end.len()) as u32;
    let mut header = [le16(event::ON_HIT), len.to_le_bytes().to_vec()].concat();
    let mut out = st(0x1D, &[]);
    out.extend(st(0x10, &header));
    out.extend(body);
    out.extend(end);
    header.clear();
    out
}
fn proj_rec(id: FormId, name: &str, flags: u16, speed: f32) -> Vec<u8> {
    let mut data = vec![0; 84];
    data[0..2].copy_from_slice(&flags.to_le_bytes());
    data[2..4].copy_from_slice(&1u16.to_le_bytes()); // Missile
    data[8..12].copy_from_slice(&speed.to_le_bytes());
    data[12..16].copy_from_slice(&5000f32.to_le_bytes());
    record(b"PROJ", 0, id.0, &[sub(b"EDID", z(name)), sub(b"DATA", data)])
}
fn xprm_box(half: [f32; 3]) -> Subrecord {
    let mut d: Vec<u8> = half.into_iter().flat_map(f32::to_le_bytes).collect();
    d.extend([0.0f32, 1.0, 0.0, 0.35].into_iter().flat_map(f32::to_le_bytes));
    d.extend(1u32.to_le_bytes()); // Box
    sub(b"XPRM", d)
}
fn placed_prim(id: FormId, edid: &str, base: FormId, pos: [f32; 3], half: Option<[f32; 3]>, xtri: Option<u32>, flags: u32) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"NAME", base.0.to_le_bytes().to_vec())];
    if let Some(h) = half {
        subs.push(xprm_box(h));
    }
    if let Some(layer) = xtri {
        subs.push(sub(b"XTRI", layer.to_le_bytes().to_vec()));
    }
    let data: Vec<u8> = pos.into_iter().chain([0.0; 3]).flat_map(f32::to_le_bytes).collect();
    subs.push(sub(b"DATA", data));
    record(b"REFR", flags, id.0, &subs)
}

fn zone_app() -> App {
    let header = record(b"TES4", 1, 0, &[sub(b"HEDR", [1.34f32.to_le_bytes(), 0u32.to_le_bytes(), 0x1000u32.to_le_bytes()].concat())]);
    let player = record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player")), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())]);
    let weapons = [
        weapon(HITSCAN_GUN, "ZoneHitscanGun", None, HITSCAN_PROJ),
        weapon(MISSILE_GUN, "ZoneMissileGun", None, MISSILE_PROJ),
    ]
    .concat();
    let projs = [
        // Hitscan flag = bit 0 (1), fast bullet so pending hit delivers immediately.
        proj_rec(HITSCAN_PROJ, "ZoneHitscanProj", 1, 20000.0),
        // Non-hitscan missile: speed 400 units/s so crossing a 200-unit box takes multiple frames.
        proj_rec(MISSILE_PROJ, "ZoneMissileProj", 0, 400.0),
    ]
    .concat();

    // Script 1: `Begin OnHit` -> `set hits to hits + 1`.
    let bump = set_short(1, ex(&[[vec![b's'], le16(1)].concat(), b"1".to_vec(), b"+".to_vec()]));
    let scpt_count = record(b"SCPT", 0, SCPT_COUNT.0, &[vec![sub(b"EDID", z("ZoneCountScript"))], script_unit(&on_hit_script(bump.clone()), &[(1, "hits", 1)], &[])].concat());
    // Script 2: `Begin OnHit` -> `set hits to hits + 1; ZoneSecondRef.Disable`.
    let disable_second = [le16(0x1C), le16(1), st(0x1022, &[])].concat();
    let scpt_disable = record(
        b"SCPT",
        0,
        SCPT_DISABLE_NEXT.0,
        &[vec![sub(b"EDID", z("ZoneDisableNextScript"))], script_unit(&on_hit_script([bump, disable_second].concat()), &[(1, "hits", 1)], &[ZONE_SECOND.0])].concat(),
    );

    let actis = [
        record(b"ACTI", 0, ACTI_ZONE.0, &[sub(b"EDID", z("TestProjZoneActi")), sub(b"SCRI", SCPT_COUNT.0.to_le_bytes().to_vec())]),
        record(b"ACTI", 0, ACTI_DISABLE_NEXT.0, &[sub(b"EDID", z("TestDisableNextActi")), sub(b"SCRI", SCPT_DISABLE_NEXT.0.to_le_bytes().to_vec())]),
        record(b"ACTI", 0, ACTI_BOTTLE.0, &[sub(b"EDID", z("TestBottleActi")), sub(b"SCRI", SCPT_COUNT.0.to_le_bytes().to_vec())]),
    ]
    .concat();

    // Lane 1 (X = 0): ZONE_A at Y = 200 (XTRI 23), BOTTLE behind it at Y = 450.
    // Also along Lane 1: ZONE_DISABLED (Initially Disabled 0x0800, XTRI 23),
    // ZONE_ACTOR_LAYER (XTRI 22), and ZONE_NO_XTRI (no XTRI).
    // Lane 2 (X = 500): ZONE_FIRST at Y = 200 disables ZONE_SECOND at Y = 350.
    let refs = [
        placed_prim(ZONE_A, "ZoneARef", ACTI_ZONE, [0.0, 200.0, 64.0], Some([50.0, 100.0, 50.0]), Some(23), 0),
        placed_prim(ZONE_DISABLED, "ZoneDisabledRef", ACTI_ZONE, [0.0, 200.0, 64.0], Some([50.0, 100.0, 50.0]), Some(23), 0x0800),
        placed_prim(ZONE_ACTOR_LAYER, "ZoneActorRef", ACTI_ZONE, [0.0, 200.0, 64.0], Some([50.0, 100.0, 50.0]), Some(22), 0),
        placed_prim(ZONE_NO_XTRI, "ZoneNoXtriRef", ACTI_ZONE, [0.0, 200.0, 64.0], Some([50.0, 100.0, 50.0]), None, 0),
        placed_prim(ZONE_FIRST, "ZoneFirstRef", ACTI_DISABLE_NEXT, [500.0, 200.0, 64.0], Some([50.0, 40.0, 50.0]), Some(23), 0),
        placed_prim(ZONE_SECOND, "ZoneSecondRef", ACTI_ZONE, [500.0, 350.0, 64.0], Some([50.0, 40.0, 50.0]), Some(23), 0),
        placed_prim(BOTTLE, "BottleRef", ACTI_BOTTLE, [0.0, 450.0, 64.0], None, None, 0),
    ]
    .concat();
    let cell_body = [
        record(b"CELL", 0, CELL.0, &[sub(b"EDID", z("ZoneTestCell")), sub(b"DATA", vec![1])]),
        [&b"GRUP"[..], &((24 + 24 + refs.len()) as u32).to_le_bytes(), &CELL.0.to_le_bytes(), &6i32.to_le_bytes(), &[0; 8]].concat(),
        [&b"GRUP"[..], &((24 + refs.len()) as u32).to_le_bytes(), &CELL.0.to_le_bytes(), &9i32.to_le_bytes(), &[0; 8]].concat(),
        refs,
    ]
    .concat();
    let cell_top = [
        &b"GRUP"[..],
        &((24 + 24 + 24 + cell_body.len()) as u32).to_le_bytes(),
        b"CELL",
        &0i32.to_le_bytes(),
        &[0; 8],
        &b"GRUP"[..],
        &((24 + 24 + cell_body.len()) as u32).to_le_bytes(),
        &0i32.to_le_bytes(),
        &2i32.to_le_bytes(),
        &[0; 8],
        &b"GRUP"[..],
        &((24 + cell_body.len()) as u32).to_le_bytes(),
        &0i32.to_le_bytes(),
        &3i32.to_le_bytes(),
        &[0; 8],
    ]
    .concat();
    let raw = [
        header,
        group(b"NPC_", &player),
        group(b"WEAP", &weapons),
        group(b"PROJ", &projs),
        group(b"SCPT", &[scpt_count, scpt_disable].concat()),
        group(b"ACTI", &actis),
        [cell_top, cell_body].concat(),
    ]
    .concat();
    let plugin = fnv_esm::Plugin::parse(&raw, "ProjectileZoneTest.esm".into()).unwrap();
    let plugins = Box::leak(vec![plugin].into_boxed_slice());
    let state = ScriptState::over_plugins(plugins);
    let game = &state.rt.as_ref().unwrap().game;
    let (forms, refs) = (game.world.forms, game.world.placed);
    let lo = Box::leak(Box::new(fnv_esm::LoadOrder::new(plugins).unwrap()));
    let schema = Box::leak(Box::new(fnv_esm::Schema::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv")).unwrap()));

    let mut phys = PhysState::default();
    let bottle_col = CollisionObject {
        node: None,
        node_block: 0,
        node_transform: Iso::IDENTITY,
        body_transform: Iso::IDENTITY,
        motion: CollideMotion::Static,
        layer: fnv_phys::layers::CLUTTER,
        mass: 0.0,
        friction: 0.5,
        restitution: 0.0,
        shape: Shape::Box { half_extents: [25.0, 25.0, 25.0] }.into(),
        shell: 0.0,
        material: None,
        wind: false,
    };
    phys.world.add_ref(&RefPose { form_id: BOTTLE.0, position: [0.0, 450.0, 64.0], rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0 }, &[bottle_col]);
    phys.world.update_queries();

    let mut a = App::new();
    a.insert_resource(state)
        .insert_resource(SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) })
        .insert_resource(PlayWorld(crate::PlayData { lo, schema, index: Box::leak(Box::default()), world: 0, interior: Some(CELL.0), eye: [0.0, 0.0, 64.0], heading_deg: 0.0, pitch_deg: 0.0, auto_walk: 0.0, start_at_cell_spot: false, packages: false }))
        .insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 64.0, feet: [0.0, 0.0, 0.0], body_eye: None, body_view: None, placed: None })
        .init_resource::<PlayerControl>()
        .init_resource::<PlayerLimbs>()
        .init_resource::<Time>()
        .insert_resource(phys)
        .init_resource::<CombatStats>()
        .init_resource::<HitFeedback>()
        .init_resource::<ActorParts>()
        .init_resource::<WeaponAnim>()
        .init_resource::<ShotVisuals>()
        .init_resource::<crate::dismember::Dismemberment>()
        .init_resource::<held_grenade_probe::Probe>()
        .add_message::<ActorFire>()
        .add_message::<ActorStrike>()
        .add_message::<ShotFired>()
        .add_message::<PlaySound>()
        .add_message::<crate::sfx::ScaledSound>()
        .add_message::<ActorHit>()
        .add_message::<ActorPush>()
        .add_message::<ImpactFx>()
        .add_message::<WeaponBroken>()
        .add_message::<Knockback>()
        .add_message::<ExplosionFx>()
        .add_message::<crate::world_sounds::ImpactPicked>()
        .add_message::<crate::RefChanged>()
        .add_message::<crate::Travel>()
        .add_message::<CasingFx>()
        .add_message::<LoopSound>()
        .add_systems(Startup, init_combat)
        .add_systems(Update, (super::super::fire, actor_attacks, fly, leave_space_shots).chain());
    a.update();
    a.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        world.resource_scope(|world, mut sim: Mut<SimState>| {
            let play = world.resource::<PlayWorld>();
            let phys = world.resource::<PhysState>();
            with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
                rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(CELL);
                rt.game.player_entered(Some(CELL));
                crate::script_trigger::update(rt, play, phys, None, false);
            });
        });
    });
    a
}

fn hits_of(a: &App, r: FormId) -> f64 {
    let g = &a.world().resource::<ScriptState>().rt.as_ref().unwrap().game;
    let Some((key, _)) = g.instance_of(r) else { return 0.0 };
    match g.var(key, 1) {
        Value::Num(n) => n,
        _ => 0.0,
    }
}

fn step_app(a: &mut App, seconds: f64) {
    a.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f64(seconds));
    a.update();
}

#[test]
fn xtri23_projectile_zone_receives_onhit_for_hitscan_and_flying_without_stopping_shots() {
    let mut a = zone_app();
    // Only enabled XTRI 23 primitives are registered as Projectile Zones.
    {
        let rt = a.world().resource::<ScriptState>().rt.as_ref().unwrap();
        let found: Vec<FormId> = rt.triggers.projectile_zone_volumes().map(|(r, _)| r).collect();
        assert!(found.contains(&ZONE_A), "enabled XTRI 23 registered: {found:?}");
        assert!(found.contains(&ZONE_FIRST));
        assert!(found.contains(&ZONE_SECOND));
        assert!(!found.contains(&ZONE_DISABLED), "disabled XTRI 23 excluded at discovery");
        assert!(!found.contains(&ZONE_ACTOR_LAYER), "XTRI 22 unaffected");
        assert!(!found.contains(&ZONE_NO_XTRI), "primitive without XTRI unaffected");
    }

    // 1. Hitscan shot straight down Lane 1 (+Y): crosses ZONE_A and hits BOTTLE behind it.
    a.world_mut().write_message(ActorFire {
        shooter: FormId::PLAYER_REF,
        weapon: HITSCAN_GUN,
        origin: [0.0, 0.0, 64.0],
        aim: [0.0, 1.0, 0.0],
        cone: 0.0,
        muzzle: None,
        harmless: false,
        sure: None,
    });
    for _ in 0..4 {
        step_app(&mut a, 0.05);
    }
    assert_eq!(hits_of(&a, ZONE_A), 1.0, "hitscan shot triggers XTRI 23 OnHit once");
    assert_eq!(hits_of(&a, BOTTLE), 1.0, "hitscan shot still reaches the bottle behind the volume");
    assert_eq!(hits_of(&a, ZONE_DISABLED), 0.0, "disabled XTRI 23 does nothing");
    assert_eq!(hits_of(&a, ZONE_ACTOR_LAYER), 0.0, "XTRI 22 primitive is unaffected");
    assert_eq!(hits_of(&a, ZONE_NO_XTRI), 0.0, "primitive without XTRI is unaffected");

    // 2. Multi-frame flying missile down Lane 1 (+Y): spends ~5 frames inside ZONE_A before hitting BOTTLE.
    a.world_mut().write_message(ActorFire {
        shooter: FormId::PLAYER_REF,
        weapon: MISSILE_GUN,
        origin: [0.0, 0.0, 64.0],
        aim: [0.0, 1.0, 0.0],
        cone: 0.0,
        muzzle: None,
        harmless: false,
        sure: None,
    });
    for _ in 0..20 {
        step_app(&mut a, 0.1);
    }
    assert_eq!(hits_of(&a, ZONE_A), 2.0, "multi-frame flying missile triggers XTRI 23 OnHit exactly once");
    assert_eq!(hits_of(&a, BOTTLE), 2.0, "flying missile still reaches the bottle behind the volume");

    // 3. Shot passing beside ZONE_A (X = 200) triggers nothing.
    a.world_mut().write_message(ActorFire {
        shooter: FormId::PLAYER_REF,
        weapon: HITSCAN_GUN,
        origin: [200.0, 0.0, 64.0],
        aim: [0.0, 1.0, 0.0],
        cone: 0.0,
        muzzle: None,
        harmless: false,
        sure: None,
    });
    for _ in 0..4 {
        step_app(&mut a, 0.05);
    }
    assert_eq!(hits_of(&a, ZONE_A), 2.0, "shot beside the box does not trigger it");

    // 4. Disabling ZONE_A stops further shots from triggering it.
    a.world_mut().resource_mut::<SimState>().world.set_enabled(ZONE_A, false).unwrap();
    a.world_mut().write_message(ActorFire {
        shooter: FormId::PLAYER_REF,
        weapon: HITSCAN_GUN,
        origin: [0.0, 0.0, 64.0],
        aim: [0.0, 1.0, 0.0],
        cone: 0.0,
        muzzle: None,
        harmless: false,
        sure: None,
    });
    for _ in 0..4 {
        step_app(&mut a, 0.05);
    }
    assert_eq!(hits_of(&a, ZONE_A), 2.0, "disabled volume does nothing");
    assert_eq!(hits_of(&a, BOTTLE), 3.0, "bottle still hit");

    // 5. Lane 2: a single hitscan shot crosses ZONE_FIRST then ZONE_SECOND; ZONE_FIRST's
    // OnHit disables ZONE_SECOND, so ZONE_SECOND must not fire on the same shot.
    a.world_mut().write_message(ActorFire {
        shooter: FormId::PLAYER_REF,
        weapon: HITSCAN_GUN,
        origin: [500.0, 0.0, 64.0],
        aim: [0.0, 1.0, 0.0],
        cone: 0.0,
        muzzle: None,
        harmless: false,
        sure: None,
    });
    for _ in 0..4 {
        step_app(&mut a, 0.05);
    }
    assert_eq!(hits_of(&a, ZONE_FIRST), 1.0, "first zone fired");
    assert_eq!(hits_of(&a, ZONE_SECOND), 0.0, "second zone disabled by first zone's OnHit must not fire");

    // 6. Re-enable ZONE_A and move it to Lane 3 (X = 800): shooting Lane 1 misses it,
    // shooting Lane 3 fires its OnHit at the new pose.
    {
        let mut sim = a.world_mut().resource_mut::<SimState>();
        sim.world.set_enabled(ZONE_A, true).unwrap();
        sim.world.get(ZONE_A).unwrap().pos = [800.0, 200.0, 64.0];
    }
    a.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        world.resource_scope(|world, mut sim: Mut<SimState>| {
            let play = world.resource::<PlayWorld>();
            let phys = world.resource::<PhysState>();
            with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
                crate::script_trigger::update(rt, play, phys, None, false);
            });
        });
    });
    a.world_mut().write_message(ActorFire {
        shooter: FormId::PLAYER_REF,
        weapon: HITSCAN_GUN,
        origin: [0.0, 0.0, 64.0],
        aim: [0.0, 1.0, 0.0],
        cone: 0.0,
        muzzle: None,
        harmless: false,
        sure: None,
    });
    for _ in 0..4 {
        step_app(&mut a, 0.05);
    }
    assert_eq!(hits_of(&a, ZONE_A), 2.0, "old pose no longer fires OnHit after ZONE_A moved");
    a.world_mut().write_message(ActorFire {
        shooter: FormId::PLAYER_REF,
        weapon: HITSCAN_GUN,
        origin: [800.0, 0.0, 64.0],
        aim: [0.0, 1.0, 0.0],
        cone: 0.0,
        muzzle: None,
        harmless: false,
        sure: None,
    });
    for _ in 0..4 {
        step_app(&mut a, 0.05);
    }
    assert_eq!(hits_of(&a, ZONE_A), 3.0, "moved ZONE_A fires OnHit at its new pose");
}
