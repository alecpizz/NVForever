//! Authored projection/posed-volume -> Bridge -> actual VATS update controls.
//! No game data, renderer/GPU or direct effect calls. All execution pending.
use super::*;
use bevy::{camera::{CameraProjection, RenderTargetInfo}, ecs::system::RunSystemOnce,
    window::{WindowRef, WindowResolution}};
use crate::{bindings::{Bindings, InputDevice}, hitvol::{PartSegment, PosedParts}, sim::test_world};
use fnv_phys::{HitVolume, PartCapsule};

const GUN: FormId = FormId(0xFE800);
const A: u32 = 0xFE811;
const B: u32 = 0xFE812;

fn scene(mode: Mode) -> (App, Entity, Frame) { scene_with_style(mode,VatsStyle::Classic) }
fn scene_with_style(mode: Mode, style: VatsStyle) -> (App, Entity, Frame) {
    let (mut app, pad) = crate::pad::device::tests::playing_app(InputDevice::KeyboardMouse);
    let mut weapon = test_world::item_base(GUN.0);
    if let fnv_sim::forms::BaseData::Item(item) = &mut weapon.data { item.kind = fnv_sim::forms::ItemKind::Weapon; item.health = Some(100); }
    let mut actor = test_world::item_base(0xFE810);
    actor.sig = *b"NPC_";
    actor.data = fnv_sim::forms::BaseData::Actor(fnv_sim::forms::ActorBase { entries: Vec::new(), attributes: None });
    let refs = [A, B].map(|id| { let mut r = test_world::placed(id, actor.id.0); r.sig = *b"ACHR"; r });
    let mut sim = test_world::sim(vec![weapon, actor], refs.to_vec());
    sim.world.add_item(FormId::PLAYER_REF, GUN, 1).unwrap();
    sim.world.equip(FormId::PLAYER_REF, GUN).unwrap();
    let mut script = ScriptState::over_no_plugins();
    with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
        fnv_game::functions::set_actor_value_current(&mut rt.game, FormId::PLAYER_REF, ACTION_POINTS, 100.0).unwrap();
    });
    app.insert_resource(sim).insert_resource(script)
        .insert_resource(crate::PlayWorld(crate::PlayData {
            lo: Box::leak(Box::new(fnv_esm::LoadOrder::new(&[]).unwrap())), schema: Box::leak(Box::default()), index: Box::leak(Box::default()),
            world: 1, interior: None, eye: [0.0, 0.0, 100.0], heading_deg: 90.0, pitch_deg: 0.0,
            auto_walk: 0.0, start_at_cell_spot: false, packages: false,
        }))
        .insert_resource(Player { heading: std::f32::consts::FRAC_PI_2, pitch: 0.0, eye_height: 100.0,
            feet: [0.0; 3], body_eye: None, body_view: None, placed: None })
        .init_resource::<PhysState>().init_resource::<ActorParts>().init_resource::<VatsMode>()
        .init_resource::<WeaponAnim>().init_resource::<ControlLocks>().init_resource::<MoveIntent>()
        .init_resource::<HudMessage>().init_resource::<VatsStyle>().init_resource::<VatsSlowdown>()
        .add_message::<PlaySound>().add_message::<LoopSound>().add_message::<ActorFire>()
        .add_systems(Startup, crate::combat::init_combat)
        .add_systems(Update, (read_keys, update).chain().after(crate::control::read_controls));
    // Use the real install/settings registration. Feedback needs no GPU to exist.
    install(&mut app);
    let window = app.world_mut().spawn((Window { resolution: WindowResolution::new(1000, 700), focused: true, ..default() }, PrimaryWindow)).id();
    let projection = PerspectiveProjection { aspect_ratio: 1000.0 / 700.0, ..default() };
    let mut camera = Camera::default();
    camera.computed.clip_from_view = projection.get_clip_from_view();
    camera.computed.target_info = Some(RenderTargetInfo { physical_size: UVec2::new(1000, 700), scale_factor: 1.0 });
    let camera = app.world_mut().spawn((camera, RenderTarget::Window(WindowRef::Entity(window)), PlayerCamera,
        GlobalTransform::from(Transform::from_translation(space::to_bevy([0.0, 0.0, 100.0])).looking_to(space::to_bevy([1.0, 0.0, 0.0]), Vec3::Y)))).id();
    app.update();
    app.world_mut().resource_mut::<Combat>().forms.weapons.insert(GUN, fnv_combat::data::WeaponStats { health: 100, clip_size: 6, base_damage: 20, ..default() });
    app.world_mut().run_system_once(crate::combat::equip_sync).unwrap();
    app.world_mut().resource_mut::<PlayerControl>().weapon_out = true;
    for (actor, y) in [(A, -30.0), (B, 35.0)] {
        let volume = HitVolume { feet: [120.0, y, 0.0], radius: 20.0, height: 160.0, dead: false };
        let capsules = [
            PartCapsule { part: 0, a: [120.0, y, 130.0], b: [120.0, y, 143.0], radius: 8.0 },
            PartCapsule { part: 1, a: [120.0, y, 75.0], b: [120.0, y, 108.0], radius: 12.0 },
        ];
        app.world_mut().resource_mut::<PhysState>().world.set_actor_parts(actor, volume, &capsules);
        app.world_mut().resource_mut::<ActorParts>().by_ref.insert(actor, PosedParts { data: FormId(0), weapon: None,
            segments: capsules.iter().map(|p| PartSegment { part: p.part, limb: if p.part == 0 { fnv_combat::data::Limb::Head } else { fnv_combat::data::Limb::Torso }, a: p.a, b: p.b, radius: None }).collect() });
    }
    // Standalone capsules publish their queued local poses at query refresh.
    // Project touches from those actual poses, not their initial body origins.
    app.world_mut().resource_mut::<PhysState>().world.update_queries();
    for (actor, y) in [(A, -30.0), (B, 35.0)] {
        for (part, z) in [(0, 136.5), (1, 91.5)] {
            let center = app.world().resource::<PhysState>().world.actor_part_center(actor, part).unwrap();
            assert!(Vec3::from_array(center).abs_diff_eq(Vec3::new(120.0, y, z), 0.001),
                "published authored capsule {actor:08X}/{part}: {center:?}");
        }
    }
    app.world_mut().insert_resource(VatsMode { phase: Phase::Marking, style, entered_at: Some(0.0),
        queue: MarkQueue::new(20.0, 1, 2), weapon: Some(InHand { id: GUN, reach: 1000.0, style: AttackStyle::Single,
            melee_reach: 0.0, sight_fov: 0.0, skill: 60.0, hit: HitTuning::default(), special: None }),
        classic: if style==VatsStyle::Classic {ClassicState { selected: Some(A), targets: [A, B].map(|actor| Candidate { actor, bearing: 0.0, off: 0.0 }).to_vec(), scanned_at: f64::MAX, ..default() }}else{default()}, ..default() });
    assert!(pick(app.world_mut(), mode.name()));
    let f = Frame { window, camera, logical: Vec2::new(1000.0, 700.0), physical: UVec2::new(1000, 700), density: 1.0, admitted: true, fresh: false, neutral: true };
    prepare(&mut app, f, true);
    app.update();
    for actor in [A, B] {
        for part in [0, 1] {
            let at = point(&app, f, actor, part);
            let ray = app.world().get::<Camera>(camera).unwrap()
                .viewport_to_world(app.world().get::<GlobalTransform>(camera).unwrap(), at).unwrap();
            let mut phys = app.world_mut().resource_mut::<PhysState>();
            let filter = shot_filter(&phys);
            let hit = phys.ray(space::to_game(ray.origin), space::to_game(*ray.direction), 1000.0, filter);
            assert!(matches!(hit.map(|h| h.owner), Some(Owner::Actor { form_id, part: hit_part, .. })
                if form_id == actor && hit_part == part), "projected fixture capsule {actor:08X}/{part}: {hit:?}");
        }
    }
    (app, pad, f)
}
fn prepare(app: &mut App, mut f: Frame, neutral: bool) {
    f.neutral = neutral;
    app.world_mut().run_system_once(move |mut b: Bridge| b.prepare(Some(f))).unwrap();
}
fn point(app: &App, f: Frame, actor: u32, part: u8) -> Vec2 {
    let at = app.world().resource::<PhysState>().world.actor_part_center(actor, part).unwrap();
    app.world().get::<Camera>(f.camera).unwrap().world_to_viewport(app.world().get::<GlobalTransform>(f.camera).unwrap(), space::to_bevy(at)).unwrap()
}
fn raw(app: &mut App, f: Frame, id: u64, phase: TouchPhase, at: Vec2, fire: bool) -> bool {
    let t = TouchInput { window: f.window, id, phase, position: at, force: None };
    app.world_mut().run_system_once(move |mut b: Bridge| if fire { b.fire(&t) } else { b.event(&t) }).unwrap()
}
fn tap(app: &mut App, f: Frame, id: u64, actor: u32, part: u8) {
    prepare(app, f, true);
    let p = point(app, f, actor, part);
    assert!(raw(app, f, id, TouchPhase::Started, p, false));
    assert!(raw(app, f, id, TouchPhase::Ended, p, false));
    app.update();
}
fn ap(app: &mut App) -> (f32, f32, f32) {
    app.world_mut().resource_scope(|world, mut sim: Mut<SimState>| {
        player_ap(Some(&mut world.resource_mut::<ScriptState>()), &mut sim).unwrap()
    })
}

#[test]
fn projected_tap_selects_actual_other_enemy_part_and_reserves_without_attacking_then_touch_fire_accepts() {
    let (mut app, _, f) = scene(Mode::Tap);
    let before = ap(&mut app);
    let p = point(&app, f, B, 0);
    assert!((p.x - f.logical.x / 2.0).abs() > 40.0, "this is not a reticle selection");
    assert!(raw(&mut app, f, 1, TouchPhase::Started, p, false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().highlighted(), Some((B, Some(0))));
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 0, "press only highlights in Tap");
    assert!(raw(&mut app, f, 1, TouchPhase::Ended, p, false)); app.update();
    let v = app.world().resource::<VatsMode>();
    assert_eq!(v.queue.marks().iter().map(|m| (m.target, m.part)).collect::<Vec<_>>(), [(FormId(B), Some(0))]);
    assert_eq!(v.reserved(), 20.0); assert_eq!(v.phase, Phase::Marking);
    assert_eq!(v.stats.shots, 0); assert!(v.shot.is_none()); assert_eq!(ap(&mut app), before);
    assert!(app.world().resource::<State>().highlighted().is_none());
    // Repeated release cannot queue again, even before neutral.
    assert!(raw(&mut app, f, 1, TouchPhase::Ended, p, false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 1);
    prepare(&mut app, f, true);
    assert!(raw(&mut app, f, 2, TouchPhase::Started, Vec2::new(900.0, 500.0), true)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().phase, Phase::Firing, "only separate Fire reaches existing accept");
}

#[test]
fn hold_has_real_progress_queues_once_without_release_or_attack_and_cancelled_short_hold_does_not_queue() {
    let (mut app, _, f) = scene(Mode::Hold);
    let p = point(&app, f, A, 1);
    raw(&mut app, f, 10, TouchPhase::Started, p, false); app.update();
    for _ in 0..3 { prepare(&mut app, f, false); app.update(); }
    assert!(app.world().resource::<State>().progress() > 0.0 && app.world().resource::<State>().progress() < 1.0);
    let mut captions = app.world_mut().query_filtered::<&Text, With<Feedback>>();
    assert!(captions.iter(app.world()).any(|text| text.0.starts_with("Hold to queue · ")));
    raw(&mut app, f, 10, TouchPhase::Canceled, p, false); app.update();
    assert_eq!(app.world().resource::<State>().progress(), 0.0);
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 0);
    prepare(&mut app, f, true); raw(&mut app, f, 11, TouchPhase::Started, p, false); app.update();
    for _ in 0..20 { prepare(&mut app, f, false); app.update(); }
    let v = app.world().resource::<VatsMode>();
    assert_eq!(v.queue.len(), 1); assert_eq!(v.phase, Phase::Marking); assert_eq!(v.stats.shots, 0);
    assert_eq!(v.highlighted(), Some((A, Some(1)))); assert_eq!(app.world().resource::<State>().progress(), 1.0);
    assert!(captions.iter(app.world()).any(|text| text.0 == "Queued · Fire confirms"));
    raw(&mut app, f, 11, TouchPhase::Ended, p, false); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 1);
    assert_eq!(app.world().resource::<State>().progress(), 0.0);
}

#[test]
fn raw_path_cancel_second_finger_window_death_and_late_gate_cannot_queue_or_replay() {
    for case in 0..10 {
        let (mut app, _, f) = scene(Mode::Tap);
        let p = point(&app, f, B, 1);
        raw(&mut app, f, 20, TouchPhase::Started, p, false); app.update();
        match case {
            0 => { raw(&mut app, f, 20, TouchPhase::Moved, Vec2::new(2.0, 2.0), false); raw(&mut app, f, 20, TouchPhase::Ended, p, false); }
            1 => { raw(&mut app, f, 20, TouchPhase::Canceled, p, false); raw(&mut app, f, 20, TouchPhase::Ended, p, false); }
            2 => { raw(&mut app, f, 21, TouchPhase::Started, p, false); raw(&mut app, f, 20, TouchPhase::Ended, p, false); }
            3 => { let other = app.world_mut().spawn(Window::default()).id(); raw(&mut app, Frame { window: other, ..f }, 20, TouchPhase::Ended, p, false); }
            4 => {
                {
                    let mut phys = app.world_mut().resource_mut::<PhysState>();
                    let mut volume = phys.world.actor_volume(B).unwrap();
                    volume.dead = true;
                    phys.world.set_actor_volume(B, volume);
                }
                raw(&mut app, f, 20, TouchPhase::Ended, p, false);
            }
            5 => { raw(&mut app, f, 20, TouchPhase::Ended, p, false); app.world_mut().resource_mut::<crate::pause::Paused>().0 = true; }
            6 => { raw(&mut app, f, 20, TouchPhase::Ended, p, false); app.world_mut().get_mut::<Camera>(f.camera).unwrap().computed.target_info = None; }
            7 => { raw(&mut app, f, 20, TouchPhase::Ended, p, false); app.world_mut().resource_mut::<VatsMode>().entered_at = Some(99.0); }
            8 => { raw(&mut app, f, 20, TouchPhase::Ended, p, false); app.world_mut().resource_mut::<Bindings>().device = InputDevice::Controller; }
            _ => {
                raw(&mut app, f, 20, TouchPhase::Ended, p, false);
                app.world_mut().resource_mut::<SimState>().world.set_enabled(FormId(B), false).unwrap();
                assert!(app.world().resource::<PhysState>().world.actor_volume(B).is_some(), "exercise late removal before renderer volume retirement");
            }
        }
        app.update();
        assert_eq!(app.world().resource::<VatsMode>().queue.len(), 0, "case{case}");
        assert_eq!(app.world().resource::<VatsMode>().stats.shots, 0, "case{case}");
        // An unrelated unknown release never takes a newer owner's gesture.
        app.world_mut().run_system_once(|mut b: Bridge| b.cancel()).unwrap();
        assert!(raw(&mut app, f, 20, TouchPhase::Ended, p, false), "old owner stays quarantined {case}");
    }
}

#[test]
fn actual_ap_and_loaded_round_limits_are_reserved_not_spent_and_fire_with_empty_queue_never_accepts() {
    let (mut app, _, f) = scene(Mode::Tap);
    app.world_mut().resource_scope(|world, mut sim: Mut<SimState>| {
        let mut script = world.resource_mut::<ScriptState>();
        with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| fnv_game::functions::set_actor_value_current(&mut rt.game, FormId::PLAYER_REF, ACTION_POINTS, 30.0).unwrap());
    });
    tap(&mut app, f, 30, A, 0); tap(&mut app, f, 31, B, 1);
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 1);
    assert_eq!(app.world().resource::<VatsMode>().reserved(), 20.0);
    assert_eq!(ap(&mut app).0, 30.0); assert_eq!(app.world().resource::<VatsMode>().phase, Phase::Marking);
    let (mut app, _, f) = scene(Mode::Tap);
    prepare(&mut app, f, true); raw(&mut app, f, 39, TouchPhase::Started, Vec2::new(900.0, 500.0), true); app.update();
    assert_eq!(app.world().resource::<VatsMode>().phase, Phase::Marking);
    // The same real mark consumer refuses the third shot: this queue has two rounds.
    raw(&mut app, f, 39, TouchPhase::Ended, Vec2::new(900.0, 500.0), true);
    for id in 40..43 { tap(&mut app, f, id, B, 0); }
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 2);
    assert_eq!(app.world().resource::<VatsMode>().reserved(), 40.0);
    assert_eq!(app.world().resource::<VatsMode>().stats.shots, 0);
}

#[test]
fn off_and_controller_keep_existing_queue_and_accept_and_hardware_takeover_retires_only_touch() {
    let (mut app, pad, f) = scene(Mode::Off);
    let p = point(&app, f, A, 0);
    assert!(!raw(&mut app, f, 50, TouchPhase::Started, p, false));
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::KeyboardMouse, Control::Use, true); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 1, "fallback physical Fire still queues");
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::KeyboardMouse, Control::Use, false); app.update();
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::KeyboardMouse, Control::Activate, true); app.update();
    assert_eq!(app.world().resource::<VatsMode>().phase, Phase::Firing);
    let (mut app, pad, f) = scene(Mode::Hold);
    let p = point(&app, f, B, 0);
    raw(&mut app, f, 51, TouchPhase::Started, p, false); app.update();
    app.world_mut().run_system_once(|mut b: Bridge| b.hardware_intent()).unwrap();
    crate::bindings::set_device(app.world_mut(), InputDevice::Controller);
    app.update(); // consume the switching edge before an ordinary controller action
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::Controller, Control::Use, true); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 1, "actual pad RightTrigger still queues");
    assert!(raw(&mut app, f, 51, TouchPhase::Ended, p, false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 1, "old finger cannot queue after controller");
}

#[test]
fn saved_mode_is_independent_of_entry_gesture_and_mode_changes_cancel_pending_contact() {
    let (mut app, _, f) = scene(Mode::Tap);
    let i = app.world().resource::<crate::settings::Settings>().index(SETTING_KEY).unwrap();
    let choices = app.world().resource::<crate::settings::Settings>().choices(i).unwrap();
    assert_eq!((choices.save)(app.world()), "Tap");
    let p = point(&app, f, A, 1); raw(&mut app, f, 60, TouchPhase::Started, p, false); app.update();
    assert!((choices.load)(app.world_mut(), "Hold"));
    assert!(raw(&mut app, f, 60, TouchPhase::Ended, p, false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 0);
    assert_eq!((choices.save)(app.world()), "Hold"); assert!(!(choices.load)(app.world_mut(), "invalid"));
    assert_eq!((choices.save)(app.world()), "Hold");
}

#[test]
fn actual_settings_pick_store_reload_preserves_each_mode_and_unrelated_entry_preference() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!("nvf-vats-touch-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("authored.ini");
    std::fs::write(&file, "[Settings]\ntouch-vats-double-tap-v1=1\n[Unrelated]\nkeep=unchanged\n").unwrap();
    let mut app = App::new(); install(&mut app);
    app.insert_resource(crate::settings::SettingsFile(Some(file.clone())));
    let i = app.world().resource::<crate::settings::Settings>().index(SETTING_KEY).unwrap();
    assert_eq!(app.world().resource::<State>().mode(), Mode::Off);
    for name in ["Tap", "Hold", "Off"] {
        assert!(crate::settings::pick(app.world_mut(), i, name));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains(&format!("{SETTING_KEY}={name}")));
        assert!(text.contains("touch-vats-double-tap-v1=1")); assert!(text.contains("keep=unchanged"));
        let mut reload = App::new(); install(&mut reload);
        reload.insert_resource(crate::settings::SettingsFile(Some(file.clone())));
        crate::settings::apply_stored(reload.world_mut());
        assert_eq!(reload.world().resource::<State>().mode().name(), name);
    }
    std::fs::remove_file(file).unwrap(); std::fs::remove_dir(directory).unwrap();
}

#[test]
fn due_hold_move_off_in_same_batch_is_rejected_at_actual_consumer_and_bounded_quarantine_recovers() {
    let (mut app, _, f) = scene(Mode::Hold);
    let p = point(&app, f, B, 0);
    raw(&mut app, f, 70, TouchPhase::Started, p, false); app.update();
    // Do not prepare until the real hold duration has elapsed. The next prepare
    // queues the due hold, but the subsequent raw move must still invalidate it.
    for _ in 0..11 { app.update(); }
    prepare(&mut app, f, false);
    raw(&mut app, f, 70, TouchPhase::Moved, Vec2::splat(2.0), false);
    raw(&mut app, f, 70, TouchPhase::Ended, p, false); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 0);
    assert_eq!(app.world().resource::<State>().progress(), 0.0);
    prepare(&mut app, f, true);
    // Too many raw positions in one batch refuse the complete gesture; they are
    // never truncated into a valid-looking end point.
    raw(&mut app, f, 71, TouchPhase::Started, p, false);
    for _ in 0..MAX_POINTS { raw(&mut app, f, 71, TouchPhase::Moved, p, false); }
    raw(&mut app, f, 71, TouchPhase::Ended, p, false); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 0);
    for id in 100..(100 + MAX_RETIRED as u64 + 1) {
        raw(&mut app, f, id, TouchPhase::Started, p, false);
        raw(&mut app, f, id, TouchPhase::Canceled, p, false);
    }
    assert!(app.world().resource::<State>().overflow);
    assert!(raw(&mut app, f, 999, TouchPhase::Started, p, false));
    prepare(&mut app, f, true);
    assert!(!app.world().resource::<State>().overflow);
    assert!(pick(app.world_mut(), "Tap")); prepare(&mut app, f, true);
    raw(&mut app, f, 1000, TouchPhase::Started, p, false);
    // A different unknown terminal edge must not steal the actual owner.
    assert!(!raw(&mut app, f, 1001, TouchPhase::Ended, p, false));
    raw(&mut app, f, 1000, TouchPhase::Ended, p, false); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 1);
    assert_eq!(app.world().resource::<VatsMode>().phase, Phase::Marking);
}

#[test]
fn trusted_actual_return_checks_primary_mode_focus_and_retires_without_queueing() {
    let (mut app, _, f) = scene(Mode::Tap);
    let p = point(&app, f, A, 0);
    app.world_mut().run_system_once(|mut b: Bridge| b.hardware_intent()).unwrap();
    let foreign = app.world_mut().spawn(Window::default()).id();
    let edge = |window, phase| TouchInput { window, id: 1200, phase, position: p, force: None };
    let t = edge(foreign, TouchPhase::Started);
    assert!(!app.world_mut().run_system_once(move |mut b: Bridge| b.consume_touch_return(&t, foreign)).unwrap(),
        "a secondary Window cannot impersonate the actual PrimaryWindow");
    let t = edge(f.window, TouchPhase::Started);
    assert!(!app.world_mut().run_system_once(move |mut b: Bridge| b.consume_touch_return(&t, foreign)).unwrap());
    app.world_mut().get_mut::<Window>(f.window).unwrap().focused = false;
    assert!(!app.world_mut().run_system_once(move |mut b: Bridge| b.consume_touch_return(&t, f.window)).unwrap());
    app.world_mut().get_mut::<Window>(f.window).unwrap().focused = true;
    let end = edge(f.window, TouchPhase::Ended);
    assert!(!app.world_mut().run_system_once(move |mut b: Bridge| b.consume_touch_return(&end, f.window)).unwrap());
    assert!(app.world_mut().run_system_once(move |mut b: Bridge| b.consume_touch_return(&t, f.window)).unwrap());
    assert!(app.world_mut().run_system_once(|b: Bridge| b.owns(1200)).unwrap());
    app.update();
    assert!(app.world().resource::<VatsMode>().marked().is_empty());
    assert!(!app.world().resource::<VatsMode>().playing_back());
    prepare(&mut app, f, false);
    assert!(raw(&mut app, f, 1200, TouchPhase::Ended, p, false));
    app.update();
    assert!(app.world().resource::<VatsMode>().marked().is_empty());
    tap(&mut app, f, 1201, A, 0);
    assert_eq!(app.world().resource::<VatsMode>().marked(), vec![A]);
    assert!(pick(app.world_mut(), "Off"));
    app.world_mut().run_system_once(|mut b: Bridge| b.hardware_intent()).unwrap();
    assert!(!app.world_mut().run_system_once(move |mut b: Bridge| b.consume_touch_return(&t, f.window)).unwrap());
}

#[test]
fn existing_feedback_updates_to_live_hud_palette_without_replacing_geometry() {
    const CHILD: &str = "NVF_VATS_LIVE_PALETTE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "vats_mode::touch::tests::existing_feedback_updates_to_live_hud_palette_without_replacing_geometry", "--nocapture"])
            .env(CHILD, "1").spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => { assert!(status.success(), "owned palette child failed: {status}"); return; }
                Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(10)),
                result => { let _ = child.kill(); let _ = child.wait(); panic!("owned palette child deadline/status: {result:?}"); }
            }
        }
    }
    let (mut app, _, f) = scene(Mode::Hold);
    let p = point(&app, f, A, 0);
    raw(&mut app, f, 1210, TouchPhase::Started, p, false); app.update();
    app.world_mut().run_system_once(feedback).unwrap();
    let mut feedback_nodes = app.world_mut().query_filtered::<(Entity, &Node, &TextColor), With<Feedback>>();
    let (entity, node, colour) = feedback_nodes.single(app.world()).unwrap();
    let (left, top) = (node.left, node.top);
    assert_eq!(colour.0, crate::interface::amber(255.0));
    crate::interface::set_hud_rgb([41, 117, 233]);
    app.world_mut().run_system_once(feedback).unwrap();
    let (same, node, colour) = feedback_nodes.single(app.world()).unwrap();
    assert_eq!(same, entity); assert_eq!((node.left, node.top), (left, top));
    assert_eq!(colour.0, crate::interface::amber(255.0));
    assert_eq!(colour.0, Color::srgb(41.0 / 255.0, 117.0 / 255.0, 233.0 / 255.0));
}

#[test]
fn android_defaults_vats_targeting_to_tap_while_desktop_keeps_off() {
    assert_eq!(State::for_platform(true).mode(), Mode::Tap);
    assert_eq!(State::for_platform(false).mode(), Mode::Off);
}

include!("live_camera_tests.rs");

include!("deadeye_tests.rs");
