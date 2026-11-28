//! End-to-end `GetReticleTargetLimb` through `find_target`, physics actor body-part
//! capsules (`ActorParts` + `CombatForms::body_parts`), `Senses::reticle`, and the
//! compiled script query.
use super::*;
use crate::combat::Combat;
use crate::hitvol::{ActorParts, PartSegment, PosedParts, WEAPON_PART};
use crate::script::ScriptState;
use fnv_combat::data::{BodyPart, BodyPartData, Limb};
use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_game::game::RunCtx;
use fnv_phys::gore::GoreStart;
use fnv_phys::{HitVolume, PartCapsule};
use fnv_script::vm::XVal;

const BPTD_HUMAN: FormId = FormId(0x0000_0801);

fn header() -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0u32.to_le_bytes(), 0x1000u32.to_le_bytes()].concat();
    let body = write_subrecords(&[Subrecord { sig: *b"HEDR", data: hedr, extended: false }]);
    [
        b"TES4".to_vec(),
        (body.len() as u32).to_le_bytes().to_vec(),
        vec![0; 4],
        0u32.to_le_bytes().to_vec(),
        vec![0; 4],
        15u16.to_le_bytes().to_vec(),
        vec![0; 2],
        body,
    ]
    .concat()
}

fn body_part(part_type: &str) -> BodyPart {
    BodyPart { part_type: Some(part_type.into()), ..BodyPart::default() }
}

fn query_limb(app: &mut App, expr: &str) -> f64 {
    let mut script = app.world_mut().resource_mut::<ScriptState>();
    let rt = script.rt.as_mut().unwrap();
    let XVal::Num(n) = rt
        .game
        .run_query(&format!("Begin Function {{ }}\nSetFunctionValue ({expr})\nEnd\n"), RunCtx::default())
        .unwrap()
    else {
        panic!("expected numeric return for {expr}");
    };
    n
}

fn limb_scene() -> App {
    let bytes = header();
    let plugins = [fnv_esm::Plugin::parse(&bytes, "ReticleLimb.esm".into()).unwrap()];
    let script = ScriptState::over_plugins(&plugins);
    let mut app = scene(&[]);
    app.insert_resource(script);
    app.add_systems(Startup, crate::combat::init_combat);
    // Run startup once so `Combat` resource exists, then install our BPTD parts.
    app.update();
    app.world_mut().resource_mut::<Combat>().forms.body_parts.insert(
        BPTD_HUMAN,
        BodyPartData {
            edid: Some("HumanBody".into()),
            model: None,
            ragdoll: None,
            parts: vec![body_part("Torso"), body_part("Head 1"), body_part("Left Arm 1")],
        },
    );

    // Settler standing east at x = 80: torso around z = 75..100, head around z = 118..126,
    // weapon segment around z = 56..64. Player eye is at (0, 0, 100) looking east (+X).
    let mut parts = ActorParts::default();
    parts.by_ref.insert(
        SETTLER_REF,
        PosedParts {
            data: BPTD_HUMAN,
            segments: vec![
                PartSegment { part: 0, limb: Limb::Torso, a: [80.0, 0.0, 75.0], b: [80.0, 0.0, 102.0], radius: Some(12.0) },
                PartSegment { part: 1, limb: Limb::Head, a: [80.0, 0.0, 118.0], b: [80.0, 0.0, 126.0], radius: Some(9.0) },
            ],
            weapon: None,
        },
    );
    app.insert_resource(parts);

    {
        let mut phys = app.world_mut().resource_mut::<PhysState>();
        let vol = HitVolume { feet: [80.0, 0.0, 0.0], radius: 16.0, height: 128.0, dead: false };
        let caps = [
            PartCapsule { part: 0, a: [80.0, 0.0, 75.0], b: [80.0, 0.0, 102.0], radius: 12.0 },
            PartCapsule { part: 1, a: [80.0, 0.0, 118.0], b: [80.0, 0.0, 126.0], radius: 9.0 },
            PartCapsule { part: WEAPON_PART, a: [80.0, 0.0, 56.0], b: [80.0, 0.0, 64.0], radius: 6.0 },
        ];
        phys.world.set_actor_parts(SETTLER_REF, vol, &caps);
        // A second actor with a whole-body capsule only (no posed parts) to the north (+Y).
        phys.world.set_actor_volume(
            NPC_REF,
            HitVolume { feet: [0.0, 80.0, 0.0], radius: 16.0, height: 128.0, dead: false },
        );
        // A non-actor chest to the south (-Y).
        let pose = RefPose {
            form_id: CHEST_REF,
            position: [0.0, -80.0, 100.0],
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            scale: 1.0,
        };
        phys.world.add_ref(&pose, &[boxed(15.0)]);
        // A detached gore piece (`GORE_PART`) west and below eye level at (-80, 0, 50)
        // so level west (-X, z=100) still misses into open sky while pitching down hits gore.
        phys.world.add_gore(&GoreStart {
            id: 1,
            actor: NPC_REF,
            translation: [-80.0, 0.0, 50.0],
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            half_extents: [15.0, 15.0, 15.0],
            center: [0.0; 3],
            linvel: [0.0; 3],
            angvel: [0.0; 3],
            mass: 1.0,
        });
        phys.world.update_queries();
    }
    app
}

#[test]
fn reticle_target_limb_reads_exact_part_range_and_clears_on_non_actor_or_sky() {
    let mut app = limb_scene();

    // Looking level east (+X, z=100) hits Settler's torso (part 0 -> location 0).
    app.update();
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb"), 0.0);
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb 100"), 0.0);
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb 40"), -1.0, "beyond maxRange");

    // Pitch up slightly toward the head capsule at (80, 0, 122).
    app.world_mut().resource_mut::<Player>().pitch = (22.0f32).atan2(80.0);
    app.update();
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb"), 1.0, "Head 1");

    // Pitch down toward the held weapon capsule at (80, 0, 60).
    app.world_mut().resource_mut::<Player>().pitch = (-40.0f32).atan2(80.0);
    app.update();
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb"), 14.0, "Weapon part");

    // Turn north (+Y) toward NPC_REF (single whole-body capsule, no posed parts) -> 0 (Torso)
    // within maxRange, and -1 beyond maxRange [design].
    {
        let mut p = app.world_mut().resource_mut::<Player>();
        p.heading = 0.0;
        p.pitch = 0.0;
    }
    app.update();
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb"), 0.0, "WHOLE fallback reads Torso (0)");
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb 100"), 0.0, "WHOLE within maxRange");
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb 40"), -1.0, "WHOLE beyond maxRange");

    // Turn south (-Y) toward the non-actor chest -> -1.
    app.world_mut().resource_mut::<Player>().heading = std::f32::consts::PI;
    app.update();
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb"), -1.0);

    // Turn west (-X) level toward open sky (no hit) -> -1.
    app.world_mut().resource_mut::<Player>().heading = -std::f32::consts::FRAC_PI_2;
    app.update();
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb"), -1.0);

    // Pitch down west toward the detached gore piece (`GORE_PART`) at (-80, 0, 50) -> hit is
    // recorded on NPC_REF's gore collider, but GetReticleTargetLimb still returns -1.
    app.world_mut().resource_mut::<Player>().pitch = (-50.0f32).atan2(80.0);
    app.update();
    {
        let script = app.world().resource::<ScriptState>();
        let rt = script.rt.as_ref().unwrap();
        assert!(rt.game.senses.reticle.and_then(|r| r.hit).is_some(), "ray must actually hit the gore collider");
        assert_eq!(rt.game.senses.crosshair_far, Some(FormId(NPC_REF)));
    }
    assert_eq!(query_limb(&mut app, "GetReticleTargetLimb"), -1.0, "GORE_PART returns -1");
}
