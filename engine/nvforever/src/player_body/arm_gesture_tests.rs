//! Authored tracks and actual Animator/Transform consumption, without game files.
use super::*;
use bevy::ecs::system::RunSystemOnce;
use fnv_nif::anim::{Channel, Key, KeyKind, Keys, TransformTrack};

#[test]
fn direct_arm_times_really_pose_forward_reverse_and_settled_model_variants() {
    for (female, bone) in [(false, "OwnedMaleArm"), (true, "OwnedFemaleArm")] {
        let candidates = ClipKey::PipBoy(female).candidates();
        assert_eq!(
            candidates.len(),
            if female { 2 } else { 1 },
            "female retains native male fallback"
        );
        let mut world = World::new();
        let root = world.spawn(Transform::IDENTITY).id();
        let node = world.spawn(Transform::IDENTITY).id();
        let skel = Arc::new(PreparedSkeleton {
            names: vec![Some("OwnedRoot".into()), Some(bone.into())],
            parents: vec![None, Some(0)],
            locals: vec![Transform::IDENTITY; 2],
            bind_world: vec![bevy::math::Affine3A::IDENTITY; 2],
            by_name: [("OwnedRoot".into(), 0), (bone.into(), 1)].into(),
            bbx_half_width: None,
        });
        let clip = Arc::new(Clip {
            name: "Owned arm pose".into(),
            start: 0.0,
            stop: nvf_play::pipboy::CLIP_SECONDS,
            frequency: 1.0,
            cycle_type: 2,
            tracks: vec![(
                bone.into(),
                Arc::new(TransformTrack {
                    translation: Channel::Keys(Keys {
                        kind: KeyKind::Linear,
                        keys: [(0.0, 0.0), (nvf_play::pipboy::HOLD_SECONDS, 40.0)]
                            .into_iter()
                            .map(|(time, x)| Key {
                                time,
                                value: [x, 0.0, 0.0],
                                forward: [0.0; 3],
                                backward: [0.0; 3],
                                tbc: [0.0; 3],
                            })
                            .collect(),
                    }),
                    rotation: Channel::Static(None),
                    euler: None,
                    scale: Channel::Static(None),
                    per_cycle: None,
                }),
            )],
            priorities: vec![98],
            skipped: 0,
            accum_root: None,
            text_keys: vec![],
            visibility: vec![],
            emitters: vec![],
            materials: vec![],
            free: vec![],
            free_visibility: vec![],
            lights: vec![],
        });
        for fraction in [0.25, 0.75, 0.5, 1.0, 1.0, 0.0] {
            let t = nvf_play::pipboy::HOLD_SECONDS * fraction;
            let mut animator = Animator::new(skel.clone(), vec![root, node], None);
            animator.begin();
            set_pipboy_layer(&mut animator, &clip, t);
            animator.end();
            animator.advance(0.5, 1.0);
            ease_pipboy_layer(&mut animator, t);
            assert_eq!(
                animator
                    .layers
                    .iter()
                    .find(|l| l.tag == TAG_PIPBOY)
                    .unwrap()
                    .time,
                t,
                "the real layer must remain at the finger/settle time"
            );
            animator.pose();
            world
                .run_system_once(move |mut transforms: Query<&mut Transform>| {
                    animator.apply(&mut transforms)
                })
                .unwrap();
            let actual = world.get::<Transform>(node).unwrap();
            let expected = 40.0 * fraction * pipboy_ease(t);
            assert!(
                (actual.translation.x - expected).abs() < 0.0001,
                "{female}/{fraction}: actual={actual:?}"
            );
            assert_eq!(actual.translation.y, 0.0);
            assert_eq!(actual.translation.z, 0.0);
        }
    }
}

#[path = "arm_gesture_coupled_tests.rs"]
mod coupled;
