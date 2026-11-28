//! Authored prepared-sample CPU consumer fixture; validation is recorded separately.
//! The explicit seam is an already-decoded authored Clip/Sample, not ActorCtx
//! asynchronous KF loading or script_sequences::prepare / drive_body scheduling.
use super::*;
use bevy::ecs::system::SystemState;
use fnv_game::game::{Game, RunCtx};
use fnv_nif::anim::{Channel, Key, KeyKind, Keys, TransformTrack};
use fnv_script::vm::XVal;

const OWNED_PATH: &str = "qa\\sequence2395\\owned_pose.kf";

// Availability-only interface seam for the real ActivateAnim handler. This does
// not pretend that a KF decoded successfully: the decoded CPU Clip is separately
// authored below and no ActorParts clip cache is touched. Preserve every other
// path's existing lookup, roots, reader and write policy, then restore them.
struct Availability(fnv_game::mod_files::Roots);
impl Availability {
    fn install() -> Self {
        let mut saved = None;
        fnv_game::mod_files::update_roots(|roots| {
            saved = Some(roots.clone());
            let previous = roots.lookup.clone();
            roots.lookup = Some(Arc::new(move |path| {
                if path
                    .replace('/', "\\")
                    .eq_ignore_ascii_case("meshes\\qa\\sequence2395\\owned_pose.kf")
                {
                    1
                } else {
                    previous.as_ref().map_or(0, |lookup| lookup(path))
                }
            }));
        });
        Self(saved.expect("capture existing public file roots"))
    }
}
impl Drop for Availability {
    fn drop(&mut self) {
        fnv_game::mod_files::set_roots(self.0.clone());
    }
}

fn authored_clip(cycle: i64, animated: bool) -> Arc<Clip> {
    let translation = if animated {
        Channel::Keys(Keys {
            kind: KeyKind::Linear,
            keys: [(0.0, 20.0), (1.0, 40.0)]
                .into_iter()
                .map(|(time, x)| Key {
                    time,
                    value: [x, 0.0, 0.0],
                    forward: [0.0; 3],
                    backward: [0.0; 3],
                    tbc: [0.0; 3],
                })
                .collect(),
        })
    } else {
        Channel::Static(Some([10.0, 0.0, 0.0]))
    };
    Arc::new(Clip {
        name: "Owned sequence consumer".into(),
        start: 0.0,
        stop: 1.0,
        frequency: 1.0,
        cycle_type: cycle,
        tracks: vec![(
            "OwnedBone".into(),
            Arc::new(TransformTrack {
                translation,
                rotation: Channel::Static(None),
                euler: None,
                scale: Channel::Static(None),
                per_cycle: None,
            }),
        )],
        priorities: vec![if animated { 1 } else { 200 }],
        skipped: 0,
        accum_root: None,
        text_keys: vec![],
        visibility: vec![],
        emitters: vec![],
        materials: vec![],
        free: vec![],
        free_visibility: vec![],
        lights: vec![],
    })
}

struct OwnedRig {
    rig: Rig,
    driven: Entity,
    unbound: Entity,
    root_x: f32,
}

fn authored_rig(world: &mut World, view: View, root_x: f32) -> OwnedRig {
    let root = world
        .spawn((
            Transform::from_xyz(root_x, 0.0, 0.0),
            GlobalTransform::default(),
        ))
        .id();
    let driven = world
        .spawn((
            Transform::from_xyz(5.0, 0.0, 0.0),
            GlobalTransform::default(),
            ChildOf(root),
        ))
        .id();
    let unbound = world
        .spawn((
            Transform::from_xyz(0.0, 3.0, 0.0),
            GlobalTransform::default(),
            ChildOf(root),
        ))
        .id();
    let skel = Arc::new(PreparedSkeleton {
        names: vec![
            Some("OwnedRoot".into()),
            Some("OwnedBone".into()),
            Some("OwnedUnbound".into()),
        ],
        parents: vec![None, Some(0), Some(0)],
        locals: vec![
            Transform::IDENTITY,
            Transform::from_xyz(5.0, 0.0, 0.0),
            Transform::from_xyz(0.0, 3.0, 0.0),
        ],
        bind_world: vec![
            Affine3A::IDENTITY,
            Affine3A::from_translation(Vec3::new(5.0, 0.0, 0.0)),
            Affine3A::from_translation(Vec3::new(0.0, 3.0, 0.0)),
        ],
        by_name: [
            ("OwnedRoot".into(), 0),
            ("OwnedBone".into(), 1),
            ("OwnedUnbound".into(), 2),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    });
    let base = Arc::new(ClipInfo::new(
        "owned-underlying".into(),
        authored_clip(0, false),
        None,
    ));
    // Neutral locomotion inputs are explicit authored fixture state. None is a
    // known absent weapon choice; it does not fabricate a successful asset read.
    let clips = [
        (ClipKey::Idle(Stance::Stand, false, false), Some(base)),
        (ClipKey::Equip(Stance::Stand, UNARMED, true), None),
        (ClipKey::Equip(Stance::Stand, UNARMED, false), None),
        (ClipKey::Holster(Stance::Stand, UNARMED), None),
    ]
    .into_iter()
    .collect();
    let rig = Rig {
        view,
        root,
        lens: None,
        folder: String::new(),
        skeleton: "owned".into(),
        animator: Animator::new(skel, vec![root, driven, unbound], None),
        clips,
        variants: HashMap::new(),
        overrides: Arc::default(),
        active_clip: None,
        head: None,
        eye_local: Vec3::ZERO,
        looking: None,
        camera: None,
        script_turn: None,
        posed_root: None,
        script_used: None,
        translate: None,
        weapon_node: None,
        weapon_parent: None,
        holstered_weapon: None,
        furniture_logged: None,
        attached: None,
        weapon_model: None,
        weapon_emitter: None,
        weapon_parts: vec![],
        idle_logged: false,
        sights_turned: None,
        muzzle: None,
        muzzle_entity: None,
        casing: None,
        casing_entity: None,
        sighting: None,
        scope_eye: None,
        use_lens: false,
        sight_relief: None,
        drawn: vec![],
        spawned_forms: vec![],
        shown: None,
        seen: None,
        furniture: None,
        since_attack: f32::INFINITY,
        attack_loop: None,
        wound_up: None,
        wound_hit: None,
        sighting2: None,
        attack_eject: None,
        ejected: false,
        carry: None,
    };
    OwnedRig {
        rig,
        driven,
        unbound,
        root_x,
    }
}

fn neutral_frame(dt: f32) -> Frame {
    Frame {
        inertia: false,
        pipboy_manual: false,
        crouch_jump: false,
        aim_fade: None,
        stance_fade: None,
        move_fade: None,
        land_stretch: None,
        hands_pace: None,
        equip_at: None,
        dt,
        stance: Stance::Stand,
        dirs: None,
        fast: false,
        female: false,
        limping: false,
        toddler: false,
        prefix: UNARMED,
        grip: None,
        drawn: None,
        hands: WeaponHands::default(),
        airborne: 0.0,
        landing: false,
        turn_rate: 0.0,
        pitch: 0.0,
        upright: false,
        aiming: false,
        speed: 0.0,
        attack: None,
        reload: None,
        jam: None,
        jam_now: false,
        attack_now: false,
        reload_now: false,
        reload_round_now: false,
        reload_rate: 1.0,
        attack_rate: 1.0,
        equip_rate: 1.0,
        spin_now: false,
        wind_now: false,
        throw_held: false,
    }
}

fn pose(
    world: &mut World,
    target: &mut OwnedRig,
    samples: &[crate::script_sequences::Sample],
    dt: f32,
) {
    pose_frame(world, target, samples, &neutral_frame(dt), dt, None);
}

fn pose_frame(
    world: &mut World,
    target: &mut OwnedRig,
    samples: &[crate::script_sequences::Sample],
    frame: &Frame,
    sequence_dt: f32,
    pipboy: Option<f32>,
) {
    pose_frame_with_pipboy_dt(world, target, samples, frame, sequence_dt, 0.0, pipboy);
}

fn pose_frame_with_pipboy_dt(
    world: &mut World,
    target: &mut OwnedRig,
    samples: &[crate::script_sequences::Sample],
    frame: &Frame,
    sequence_dt: f32,
    pipboy_dt: f32,
    pipboy: Option<f32>,
) {
    let mut state =
        SystemState::<(ResMut<ActorParts>, ModelCtx, Query<&mut Transform>, Query<&ChildOf>)>::new(world);
    {
        let (mut parts, mut models, mut transforms, parents) = state.get_mut(world).expect("authored fixture resources are installed");
        let mut ctx = ActorCtx {
            cache: &mut parts,
            models: &mut models,
        };
        let special = IdleSlot::<Arc<Clip>>::default();
        // No camera/head is installed in this authored rig. The None eye result
        // is not an admission failure: actual Transform application precedes it.
        let _ = pose_rig(
            &mut target.rig,
            frame,
            &[],
            Posed {
                root: Transform::from_xyz(target.root_x, 0.0, 0.0),
                look_pitch: 0.0,
                special: &special,
                sequences: samples,
                sequence_dt,
                pipboy_dt,
                pipboy,
                sway: SwayPose::NONE,
                sights: None,
            },
            &mut ctx,
            (&mut transforms, &parents),
        );
    }
    state.apply(world);
}

fn scalar_query(game: &mut Game<'_>, source: &str) -> XVal {
    game.run_query(
        &format!("Begin Function {{}}\n{source}\nEnd"),
        RunCtx::default(),
    )
    .unwrap()
}

// This is deliberately a prepared-sample seam. It reads the real public request
// and clock, but does not call prepare, publish loader metadata or fill a clip cache.
fn prepared(game: &Game<'_>, clip: &Arc<Clip>) -> Vec<crate::script_sequences::Sample> {
    let key = (FormId::PLAYER_REF, true, OWNED_PATH.to_string());
    game.anim_paths
        .sequences
        .get(&key)
        .map(|request| crate::script_sequences::Sample {
            request: request.clone(),
            clip: clip.clone(),
            time: (game.script_sequence_time_for(FormId::PLAYER_REF, OWNED_PATH, true)
                * f64::from(clip.frequency)) as f32,
            weight: request.weight,
            fade_in: request.ease_in.expect("fixture supplies explicit ease-in"),
            additive: None,
        })
        .into_iter()
        .collect()
}

fn assert_pose(app: &App, target: &OwnedRig, x: f32) {
    let world = app.world();
    let local = world.get::<Transform>(target.driven).unwrap();
    let measured_global = world
        .get::<GlobalTransform>(target.driven)
        .unwrap()
        .translation();
    println!(
        "sequence-consumer: view={:?} root={:?} bone={:?} local={:?} world={:?} target_x={x}",
        target.rig.view, target.rig.root, target.driven, local, measured_global
    );
    assert!(
        (local.translation.x - x).abs() < 1e-5,
        "actual applied local pose {:?}, expected {x}",
        local.translation
    );
    let global = world
        .get::<GlobalTransform>(target.driven)
        .unwrap()
        .translation();
    assert!(
        (global.x - (target.root_x + x)).abs() < 1e-5,
        "actual propagated world pose {global:?}"
    );
    assert_eq!(
        *world.get::<Transform>(target.unbound).unwrap(),
        Transform::from_xyz(0.0, 3.0, 0.0)
    );
    assert_eq!(
        world
            .get::<GlobalTransform>(target.unbound)
            .unwrap()
            .translation(),
        Vec3::new(target.root_x, 3.0, 0.0)
    );
}

#[test]
fn prepared_public_sequence_reaches_real_pose_and_releases_to_the_underlying_clip() {
    // Canonical engine tests run in parallel. Keep the process-global Roots seam
    // in a fresh process executing only this exact fixture, never in that parent.
    const SELF_TEST: &str = "player_body::sequence_consumer_tests::prepared_public_sequence_reaches_real_pose_and_releases_to_the_underlying_clip";
    const CHILD: &str = "NVF_QA_SEQUENCE_CONSUMER_CHILD";
    if std::env::var(CHILD).as_deref() != Ok("1") {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([SELF_TEST, "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .expect("start the isolated exact consumer fixture");
        assert!(
            output.status.success(),
            "isolated consumer fixture failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.iter().any(|arg| arg == "--exact") && args.iter().any(|arg| arg == SELF_TEST),
        "Roots seam requires this exact fixture alone in its owned process"
    );
    let _availability = Availability::install();
    for (cycle, end_sample) in [(0, 25.0), (1, 35.0), (2, 40.0)] {
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        app.add_plugins(bevy::transform::TransformPlugin);
        let forms: &'static fnv_sim::FormStore =
            Box::leak(Box::new(crate::outfits::tests::actor_forms(vec![])));
        let refs: &'static fnv_sim::RefTable = Box::leak(Box::default());
        let mut game =
            crate::outfits::tests::game(fnv_sim::World::new(forms, refs, Default::default()));
        game.cfg.frame_seconds = 0.125;
        game.senses.first_person = true;
        let mut first = authored_rig(app.world_mut(), View::First, 100.0);
        let mut third = authored_rig(app.world_mut(), View::Third, 200.0);
        let clip = authored_clip(cycle, true);
        clip.check().unwrap();
        pose(app.world_mut(), &mut first, &[], 0.25);
        pose(app.world_mut(), &mut third, &[], 0.25);
        app.update();
        assert_pose(&app, &first, 10.0);
        assert_pose(&app, &third, 10.0);
        assert_eq!(
            scalar_query(
                &mut game,
                &format!("SetFunctionValue (PlayerREF.ActivateAnim \"{OWNED_PATH}\" 1 3 1 1 0)")
            ),
            XVal::Num(1.0)
        );
        let key = (FormId::PLAYER_REF, true, OWNED_PATH.to_string());
        let request = game.anim_paths.sequences[&key].clone();
        assert!(request.first && request.stopped.is_none());
        assert_eq!(
            (request.priority, request.weight, request.ease_in),
            (3, 1.0, Some(0.0))
        );
        let tag = 0x4000_0000 | request.serial;
        for (ticks, expected) in [(2, 25.0), (4, 35.0), (4, end_sample)] {
            for _ in 0..ticks {
                game.tick();
            }
            let samples = prepared(&game, &clip);
            println!(
                "sequence-consumer: phase=active cycle={cycle} core_time={} sample_time={}",
                game.time, samples[0].time
            );
            pose(app.world_mut(), &mut first, &samples, 0.0);
            pose(app.world_mut(), &mut third, &[], 0.0);
            app.update();
            assert_pose(&app, &first, expected);
            assert_pose(&app, &third, 10.0);
            let layer = first
                .rig
                .animator
                .layers
                .iter()
                .find(|layer| layer.tag == tag)
                .expect("real consumer added the script layer");
            assert_eq!(layer.sequence_priority, 3);
            assert_eq!(layer.mode, if cycle == 2 { Mode::Once } else { Mode::Loop });
            assert!(!third
                .rig
                .animator
                .layers
                .iter()
                .any(|layer| layer.tag == tag));
        }
        assert_eq!(
            scalar_query(
                &mut game,
                &format!("SetFunctionValue (PlayerREF.DeactivateAnim \"{OWNED_PATH}\" 0.5)")
            ),
            XVal::Num(1.0)
        );
        assert!(game.anim_paths.sequences[&key].stopped.is_some());
        for share in [0.75, 0.5, 0.25, 0.0] {
            game.tick();
            println!(
                "sequence-consumer: phase=fade cycle={cycle} core_time={} expected_share={share}",
                game.time
            );
            let samples = prepared(&game, &clip);
            pose(app.world_mut(), &mut first, &samples, 0.125);
            pose(app.world_mut(), &mut third, &[], 0.125);
            app.update();
            assert_pose(&app, &first, 10.0 + (end_sample - 10.0) * share);
            assert_pose(&app, &third, 10.0);
        }
        // [documented: Game::tick -> knvse_tick] Request cleanup precedes
        // advancing Game.time. The fourth render fade step reaches zero after
        // that frame's cleanup has already read the preceding core time.
        assert!(!first
            .rig
            .animator
            .layers
            .iter()
            .any(|layer| layer.tag == tag));
        println!(
            "sequence-consumer: phase=core-retirement cycle={cycle} before_time={} request_present={}",
            game.time, game.anim_paths.sequences.contains_key(&key)
        );
        game.tick(); // Exactly one ordinary cleanup boundary; no direct mutation.
        assert!(!game.anim_paths.sequences.contains_key(&key));
        assert!(first
            .rig
            .animator
            .layers
            .iter()
            .any(|layer| layer.tag == TAG_IDLE));
        pose(app.world_mut(), &mut first, &[], 0.125);
        app.update();
        assert_pose(&app, &first, 10.0); // Underlying clip, not bind x=5.
    }
}

#[test]
fn device_menu_sequence_moves_and_blends_with_holstered_or_drawn_hands_and_a_held_world() {
    const SELF_TEST: &str = "player_body::sequence_consumer_tests::device_menu_sequence_moves_and_blends_with_holstered_or_drawn_hands_and_a_held_world";
    const CHILD: &str = "NVF_QA_DEVICE_MENU_SEQUENCE_CHILD";
    if std::env::var(CHILD).as_deref() != Ok("1") {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([SELF_TEST, "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output().unwrap();
        assert!(output.status.success(), "isolated device menu consumer failed:\n{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return;
    }
    let args: Vec<_> = std::env::args().collect();
    assert!(args.iter().any(|arg| arg == "--exact") && args.iter().any(|arg| arg == SELF_TEST));
    let _availability = Availability::install();
    // The prepared-sample seam exercises the real tiered pose and fade consumer.
    // It does not load a weapon model, project a screen or execute installed mods.
    for (smooth, phase) in [false, true].into_iter().flat_map(|smooth| [WeaponPhase::Holstered, WeaponPhase::Drawn].map(|phase| (smooth, phase))) {
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        app.add_plugins(bevy::transform::TransformPlugin);
        let forms = Box::leak(Box::new(crate::outfits::tests::actor_forms(vec![])));
        let refs = Box::leak(Box::default());
        let mut game = crate::outfits::tests::game(fnv_sim::World::new(forms, refs, Default::default()));
        game.cfg.frame_seconds = 0.125;
        game.senses.first_person = true;
        let mut first = authored_rig(app.world_mut(), View::First, 100.0);
        let mut third = authored_rig(app.world_mut(), View::Third, 200.0);
        first.rig.animator.smooth_seams(smooth.then_some(0.1));
        third.rig.animator.smooth_seams(smooth.then_some(0.1));
        let ordinary = Arc::new(ClipInfo::new("authored-device-hold".into(), authored_clip(2, false), None));
        first.rig.clips.insert(ClipKey::PipBoy(false), Some(ordinary));
        let clip = authored_clip(2, true);
        clip.check().unwrap();
        let mut frame = neutral_frame(0.0);
        frame.hands.phase = phase;
        frame.hands.in_hand = phase == WeaponPhase::Drawn;
        frame.drawn = (phase == WeaponPhase::Drawn).then_some(UNARMED);
        // A Pip-Boy hold and the idle have already reached their ordinary pose.
        pose(app.world_mut(), &mut first, &[], 0.25);
        pose(app.world_mut(), &mut third, &[], 0.25);
        app.update();
        assert_pose(&app, &first, 10.0);
        assert_eq!(scalar_query(&mut game, &format!("SetFunctionValue (PlayerREF.ActivateAnim \"{OWNED_PATH}\" 1 1 1 1 0.25)")), XVal::Num(1.0));
        game.menu = Some(fnv_game::game::menus::INVENTORY);
        let held = game.time;
        for _ in 0..4 {
            game.tick();
            pose_frame(app.world_mut(), &mut first, &prepared(&game, &clip), &frame, 0.125, Some(nvf_play::pipboy::HOLD_SECONDS));
            pose_frame(app.world_mut(), &mut third, &[], &frame, 0.0, None);
            app.update();
        }
        assert_eq!(game.time, held, "device menu must keep the world held");
        assert_pose(&app, &first, 30.0);
        assert_pose(&app, &third, 10.0);
        let layer = first.rig.animator.layers.iter().find(|layer| layer.tag & 0x4000_0000 != 0).unwrap();
        assert_eq!((layer.sequence_priority, layer.ease), (1, 1.0));
        assert!(first.rig.animator.layers.iter().filter(|layer| layer.tag == TAG_IDLE).all(|layer| layer.time == 0.25), "ordinary clip time must stay held");

        assert_eq!(scalar_query(&mut game, &format!("SetFunctionValue (PlayerREF.DeactivateAnim \"{OWNED_PATH}\" 0.25)")), XVal::Num(1.0));
        for _ in 0..3 {
            game.tick();
            pose_frame(app.world_mut(), &mut first, &prepared(&game, &clip), &frame, 0.125, Some(nvf_play::pipboy::HOLD_SECONDS));
            app.update();
        }
        assert_pose(&app, &first, 10.0);
        assert!(!first.rig.animator.layers.iter().any(|layer| layer.tag & 0x4000_0000 != 0), "deactivation releases the visible menu pose while world time is held");
        assert!(game.anim_paths.sequences.is_empty());
    }
}

/// #2862: Under `--menu-pause original` (`dt == 0.0`) with Animation Smoothing on and
/// no script ticks (`sequence_dt == 0.0`, e.g. `--no-scripts` or render frames between
/// script ticks), opening the Pip-Boy with empty hands or a holstered weapon must still
/// raise the first-person arm pose into view via `pipboy_dt` and restore the prior
/// arms-down pose on close, without ticking ordinary world clips or breaking drawn hands.
#[test]
fn holstered_and_empty_hands_pipboy_raises_and_lowers_with_held_world_and_seam_smoothing() {
    let make_static_clip = |name: &str, x: f32, priority: i32| {
        Arc::new(ClipInfo::new(
            name.into(),
            Arc::new(Clip {
                name: name.into(),
                start: 0.0,
                stop: 1.0,
                frequency: 1.0,
                cycle_type: 2,
                tracks: vec![(
                    "OwnedBone".into(),
                    Arc::new(TransformTrack {
                        translation: Channel::Static(Some([x, 0.0, 0.0])),
                        rotation: Channel::Static(None),
                        euler: None,
                        scale: Channel::Static(None),
                        per_cycle: None,
                    }),
                )],
                priorities: vec![priority],
                skipped: 0,
                accum_root: None,
                text_keys: vec![],
                visibility: vec![],
                emitters: vec![],
                materials: vec![],
                free: vec![],
                free_visibility: vec![],
                lights: vec![],
            }),
            None,
        ))
    };

    for (label, prefix, phase) in [
        ("empty-hands", UNARMED, WeaponPhase::Holstered),
        ("holstered-pistol", "1hp", WeaponPhase::Holstered),
        ("drawn-pistol", "1hp", WeaponPhase::Drawn),
    ] {
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        app.add_plugins(bevy::transform::TransformPlugin);
        let mut first = authored_rig(app.world_mut(), View::First, 100.0);
        first.rig.animator.smooth_seams(Some(SEAM_SECONDS));
        first.rig.clips.insert(ClipKey::Idle(Stance::Stand, false, false), Some(make_static_clip("idle", 10.0, 10)));
        first.rig.clips.insert(ClipKey::Holster(Stance::Stand, prefix), Some(make_static_clip("holster", 10.0, 55)));
        first.rig.clips.insert(ClipKey::Equip(Stance::Stand, prefix, true), None);
        first.rig.clips.insert(ClipKey::Equip(Stance::Stand, prefix, false), None);
        first.rig.clips.insert(ClipKey::Aim(Stance::Stand, prefix, Aim::Level, false), Some(make_static_clip("aim", 18.0, 50)));
        first.rig.clips.insert(ClipKey::PipBoy(false), Some(make_static_clip("pipboy", 30.0, 98)));

        let mut base = neutral_frame(0.25);
        base.inertia = true;
        base.prefix = prefix;
        base.hands.phase = phase;
        base.hands.in_hand = phase == WeaponPhase::Drawn;
        base.drawn = (phase == WeaponPhase::Drawn).then_some(prefix);
        let expected_down = if phase == WeaponPhase::Drawn { 18.0 } else { 10.0 };

        // Establish pre-Pip-Boy pose (arms down for empty/holstered, held for drawn).
        pose_frame_with_pipboy_dt(app.world_mut(), &mut first, &[], &base, 0.25, 0.0, None);
        app.update();
        assert_pose(&app, &first, expected_down);

        // Open Pip-Boy under held world (dt == 0.0) and zero script ticks (sequence_dt == 0.0).
        let mut held = base;
        held.dt = 0.0;
        let mut pip = nvf_play::pipboy::PipBoy::default();
        pip.raise();
        for _ in 0..10 {
            pip.advance(0.05);
            pose_frame_with_pipboy_dt(app.world_mut(), &mut first, &[], &held, 0.0, 0.05, Some(pip.clip_time));
            app.update();
        }
        assert_eq!(pip.phase, nvf_play::pipboy::Phase::Up, "{label}");
        assert_pose(&app, &first, 30.0);
        assert!(
            first.rig.animator.layers.iter().filter(|l| l.tag == TAG_IDLE).all(|l| (l.time - 0.25).abs() < 1e-6),
            "{label}: ordinary idle clip time must stay held while world is paused"
        );

        // Close Pip-Boy: lowers cleanly back to the prior pose (arms down for empty/holstered, held for drawn).
        pip.lower();
        while pip.phase != nvf_play::pipboy::Phase::Down {
            pip.advance(0.05);
            let clip_t = pip.on_arm().then_some(pip.clip_time);
            let pip_dt = if clip_t.is_some() { 0.05 } else { 0.0 };
            let world_dt = if clip_t.is_some() { 0.0 } else { 0.05 };
            held.dt = world_dt;
            pose_frame_with_pipboy_dt(app.world_mut(), &mut first, &[], &held, world_dt, pip_dt, clip_t);
            app.update();
        }
        // Let the residual leaving fade finish once normal world time resumes.
        held.dt = 0.25;
        pose_frame_with_pipboy_dt(app.world_mut(), &mut first, &[], &held, 0.25, 0.0, None);
        app.update();
        assert!(!first.rig.animator.layers.iter().any(|l| l.tag == TAG_PIPBOY), "{label}: Pip-Boy layer retired after lowering");
        assert_pose(&app, &first, expected_down);
    }
}

#[test]
fn script_sequence_weight_and_additive_pose_reach_real_rig_transform() {
    let (mut app, _files) = crate::outfits::tests::asset_fixture();
    app.add_plugins(bevy::transform::TransformPlugin);
    let mut first = authored_rig(app.world_mut(), View::First, 100.0);
    // Underlying idle puts OwnedBone at x=10 (bind is x=5).
    pose(app.world_mut(), &mut first, &[], 0.25);
    app.update();
    assert_pose(&app, &first, 10.0);
    assert_eq!(first.rig.active_clip.as_ref().map(|(p, _)| p.as_str()), Some("owned-underlying"));

    // 1. Non-additive script sequence at x=30 with SetAnimWeight 0.25 -> 10 + 0.25 * (30 - 10) = 15.
    let overlay = authored_clip(2, true); // at t=0.5, x=30
    let make_sample = |serial: u32, weight: f32, time: f32, additive: Option<nvf_assets::anim_blend::AdditivePose>| crate::script_sequences::Sample {
        request: fnv_game::nvse_anim::ScriptSequence {
            path: OWNED_PATH.into(),
            first: true,
            serial,
            priority: 3,
            weight,
            ease_in: Some(0.0),
            ease_out: 0.2,
            stopped: None,
            auto_finish: false,
            clock_offset: 0.0,
        },
        clip: overlay.clone(),
        time,
        weight,
        fade_in: 0.0,
        additive,
    };
    pose(app.world_mut(), &mut first, &[make_sample(1, 0.25, 0.5, None)], 0.0);
    app.update();
    assert_pose(&app, &first, 15.0);

    // Weight 0.0 yields 100% back to the underlying clip (x=10).
    pose(app.world_mut(), &mut first, &[make_sample(1, 0.0, 0.5, None)], 0.0);
    app.update();
    assert_pose(&app, &first, 10.0);

    // 2. Additive script sequence with reference pose at t=0.0 (x=20) sampled at t=0.5 (x=30):
    //    delta = +10; at weight 0.5 on top of base x=10 -> x=15.
    let add = Some(nvf_assets::anim_blend::AdditivePose { reference: Some(overlay.clone()), ref_time: 0.0 });
    pose(app.world_mut(), &mut first, &[make_sample(2, 0.5, 0.5, add)], 0.0);
    app.update();
    assert_pose(&app, &first, 15.0);
}
