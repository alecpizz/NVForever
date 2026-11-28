// Include inside touch::tests after the real parent VATS adapter is composed.
// Fresh App: real plugins/Startup/raw TouchInput/weapon discovery, no VATS state seed.
mod vats_parent_tests {
use super::*;
use bevy::{camera::{CameraProjection, RenderTarget, RenderTargetInfo},
    ecs::system::RunSystemOnce, input::gamepad::{Gamepad, GamepadButton, RawGamepadButtonChangedEvent, RawGamepadEvent}};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, forms::{ActorBase, BaseData, BaseForm, Item, ItemKind}};

const VATS_GUN: FormId = FormId(0xF7800);
const VATS_AMMO: FormId = FormId(0xF7801);
const VATS_PROJECTILE: FormId = FormId(0xF7802);
const VATS_ACTOR_BASE: FormId = FormId(0xF7810);

#[derive(Resource, Default)]
struct VatsParentObserved { uses: usize, executions: usize, executing: bool }
fn observe_parent_use(input: nvf_play::bindings::ControlInput, mut seen: ResMut<VatsParentObserved>) {
    seen.uses += usize::from(input.just_pressed(Control::Use));
}
fn observe_parent_execution(vats: Res<nvf_play::vats_mode::VatsMode>, mut seen: ResMut<VatsParentObserved>) {
    let executing = vats.playing_back();
    seen.executions += usize::from(executing && !seen.executing);
    seen.executing = executing;
}
fn vats_base(id: FormId, sig: [u8; 4], data: BaseData) -> BaseForm {
    BaseForm { id, sig, edid: Some(format!("Authored{id}")), name: Some(format!("Authored{id}")),
        script: None, prompt: None, voice_filter: false, data }
}
fn vats_item(kind: ItemKind) -> BaseData {
    BaseData::Item(Item { kind, value: Some(1), money: None, weight: Some(0.5), slots: None, projectile: None,
        health: Some(100), non_playable: false, cant_drop: false, armor_class: default(), power_armor: false,
        armor_dr: None, armor_dt: None, weapon_damage: None, weight_mods: [0.0; 3], drawable: true,
        not_for_npcs: false, thrown: false, quest_item: false, icon: None, placed_count: None,
        teaches: None, cant_be_taken: false, modulates_voice: false, weapon_traits: None })
}
fn vats_pick(app: &mut App, key: &str, name: &str) {
    let settings = app.world().resource::<nvf_play::settings::Settings>();
    let row = settings.index(key).unwrap();
    if (settings.choices(row).unwrap().chosen)(app.world()) != name {
        assert!(nvf_play::settings::pick(app.world_mut(), row, name));
    }
}
fn vats_action_point(app: &App, control: Control) -> Vec2 {
    let index = BUTTONS.iter().position(|b| b.control == Some(control)).unwrap();
    app.world().resource::<Layout>().config.center(Target::Button(index), Vec2::splat(1000.0))
}
fn vats_edge(app: &mut App, w: Entity, id: u64, phase: TouchPhase, at: Vec2) {
    touch(app, w, id, phase, at.x, at.y);
}
fn vats_raw_action(app: &mut App, w: Entity, id: u64, control: Control) {
    let at = vats_action_point(app, control);
    vats_edge(app, w, id, TouchPhase::Started, at);
    vats_edge(app, w, id, TouchPhase::Ended, at);
    app.update();
}

struct VatsParentScene { app: App, window: Entity, camera: Entity, actor: FormId, part_points: [Vec2; 2], pad: Entity }
fn vats_parent_scene(mode: &str) -> VatsParentScene { vats_parent_scene_with_style(mode,"Classic") }
fn vats_parent_scene_with_style(mode: &str, style: &str) -> VatsParentScene {
    use nvf_play::{control::{ControlLocks, LockStats, PlayerControl}, hitvol::{ActorParts, PartSegment, PosedParts},
        player::{MoveIntent, Player}, vats_mode::VatsModePlugin};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, InputPlugin, StatesPlugin)).init_state::<nvf_play::PlayState>()
        .insert_resource(Bindings::default()).insert_resource(nvf_play::settings::SettingsFile(None))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(40)))
        .insert_resource(Player { heading: std::f32::consts::FRAC_PI_2, pitch: 0.0, eye_height: 100.0,
            feet: [0.0; 3], body_eye: None, body_view: None, placed: None })
        .insert_resource(PlayerControl { weapon_out: true, ..default() })
        .init_resource::<ControlLocks>().init_resource::<LockStats>().init_resource::<MoveIntent>()
        .init_resource::<nvf_play::furniture::PlayerSeat>().init_resource::<nvf_play::activate::HudMessage>()
        .init_resource::<nvf_play::combat::WeaponAnim>().init_resource::<ActorParts>()
        .add_message::<nvf_play::sfx::PlaySound>().add_message::<nvf_play::sfx::LoopSound>()
        .add_message::<nvf_play::combat::ActorFire>()
        .insert_resource(nvf_play::PlayWorld(nvf_play::PlayData {
            lo: Box::leak(Box::new(fnv_esm::LoadOrder::new(&[]).unwrap())), schema: Box::leak(Box::default()), index: Box::leak(Box::default()),
            world: 1, interior: None, eye: [0.0, 0.0, 100.0], heading_deg: 90.0, pitch_deg: 0.0,
            auto_walk: 0.0, start_at_cell_spot: false, packages: false,
        }))
        .add_plugins((TouchControlsPlugin, nvf_play::pad::PadPlugin, VatsModePlugin))
        .configure_sets(Update, (nvf_play::PlaySet::Input, nvf_play::PlaySet::Physics, nvf_play::PlaySet::Activation,
            nvf_play::PlaySet::Ui, nvf_play::PlaySet::Sync).chain())
        .add_systems(Startup, nvf_play::combat::init_combat)
        .add_systems(Update, (nvf_play::combat::equip_sync, nvf_play::control::read_controls, observe_parent_use).chain().in_set(nvf_play::PlaySet::Input))
        .init_resource::<VatsParentObserved>()
        .add_systems(Update, observe_parent_execution.after(nvf_play::vats_mode::VatsUpdate));
    nvf_play::vats_mode::touch::register_settings(&mut app);
    let window = app.world_mut().spawn((Window { resolution: bevy::window::WindowResolution::new(1000, 1000), focused: true, ..default() }, PrimaryWindow)).id();
    let pad = app.world_mut().spawn(Gamepad::default()).id();
    let mut camera_value = Camera::default();
    camera_value.computed.clip_from_view = PerspectiveProjection { aspect_ratio: 1.0, ..default() }.get_clip_from_view();
    camera_value.computed.target_info = Some(RenderTargetInfo { physical_size: UVec2::splat(1000), scale_factor: 1.0 });
    let transform = GlobalTransform::from(Transform::from_translation(nvf_play::space::to_bevy([0.0, 0.0, 100.0]))
        .looking_to(nvf_play::space::to_bevy([1.0, 0.0, 0.0]), Vec3::Y));
    let camera = app.world_mut().spawn((camera_value.clone(), transform, RenderTarget::Window(bevy::window::WindowRef::Primary), nvf_play::PlayerCamera)).id();
    let mut forms = FormStore::default();
    for b in [vats_base(FormId::PLAYER_BASE, *b"NPC_", BaseData::Actor(ActorBase { entries: Vec::new(), attributes: None })),
        vats_base(VATS_ACTOR_BASE, *b"NPC_", BaseData::Actor(ActorBase { entries: Vec::new(), attributes: None })),
        vats_base(VATS_GUN, *b"WEAP", vats_item(ItemKind::Weapon)), vats_base(VATS_AMMO, *b"AMMO", vats_item(ItemKind::Ammo))] {
        forms.sigs.insert(b.id, b.sig); forms.bases.insert(b.id, b);
    }
    let forms = Box::leak(Box::new(forms)); let refs = Box::leak(Box::new(RefTable::default()));
    let mut sim = nvf_play::sim::SimState { forms, refs, world: fnv_sim::World::new(forms, refs, SimConfig::default()) };
    sim.world.add_item(FormId::PLAYER_REF, VATS_GUN, 1).unwrap();
    sim.world.add_item(FormId::PLAYER_REF, VATS_AMMO, 3).unwrap();
    sim.world.equip(FormId::PLAYER_REF, VATS_GUN).unwrap();
    let actor = sim.world.place(VATS_ACTOR_BASE, None, [150.0, 0.0, 0.0], [0.0; 3], 1).unwrap();
    app.insert_resource(sim).init_resource::<nvf_play::phys::PhysState>();
    app.update(); // Real Startup from all plugins, including VATS and wrapper labels.
    app.world_mut().resource_mut::<Layout>().config = layout::Config::preset(false);
    app.world_mut().resource_mut::<Layout>().path = None;
    {
        let mut combat = app.world_mut().resource_mut::<nvf_play::combat::Combat>();
        combat.forms.weapons.insert(VATS_GUN, fnv_combat::data::WeaponStats {
            health: 100, clip_size: 2, ammo_use: 1, anim_type: Some("Pistol - Ballistic (1 Hand)".into()),
            ammo: Some(VATS_AMMO), projectile: Some(VATS_PROJECTILE), projectile_count: 1,
            anim_mult: 1.0, attack_mult: 1.0, fire_rate: 2.0, base_damage: 20, action_points: Some(20.0), ..default()
        });
        combat.forms.ammo.insert(VATS_AMMO, fnv_combat::data::AmmoStats { clip_rounds: 1, ..default() });
        combat.forms.projectiles.insert(VATS_PROJECTILE, fnv_combat::data::ProjectileStats { kind: Some("Missile".into()), speed: 1000.0, range: 2000.0, ..default() });
    }
    app.world_mut().run_system_once(nvf_play::combat::equip_sync).unwrap();
    assert_eq!(app.world().resource::<nvf_play::combat::Combat>().player_weapon(), Some(VATS_GUN));
    assert_eq!(app.world().resource::<nvf_play::combat::Combat>().vats_weapon(app.world().resource::<nvf_play::sim::SimState>(), -981.0).unwrap().rounds, 2,
        "real inventory/equip/cycle discovery loads two rounds, no seeded MarkQueue");
    // Choose actual free body points, then author the posed capsules on their world rays.
    let candidates: Vec<_> = (560..800).step_by(30).flat_map(|x| (240..560).step_by(40).map(move |y| Vec2::new(x as f32, y as f32)))
        .filter(|p| app.world().resource::<Layout>().config.hit_filtered(*p, Vec2::splat(1000.0), true, false, |_| true).is_none())
        .filter(|p| !layout::utility_rect(false, Vec2::splat(1000.0)).contains(*p) && !layout::utility_rect(true, Vec2::splat(1000.0)).contains(*p)).collect();
    let one = candidates[0]; let two = *candidates.iter().find(|p| p.distance(one) > 160.0).unwrap();
    let part_points = [one, two];
    let centers = part_points.map(|point| { let ray = camera_value.viewport_to_world(&transform, point).unwrap(); nvf_play::space::to_game(ray.origin + ray.direction.as_vec3() * 150.0) });
    let volume = fnv_phys::HitVolume { feet: [150.0, 0.0, 30.0], radius: 30.0, height: 160.0, dead: false };
    let caps: Vec<_> = centers.iter().enumerate().map(|(i, p)| fnv_phys::PartCapsule { part: i as u8, a: [p[0], p[1], p[2] - 3.0], b: [p[0], p[1], p[2] + 3.0], radius: 6.0 }).collect();
    app.world_mut().resource_mut::<nvf_play::phys::PhysState>().world.set_actor_parts(actor.0, volume, &caps);
    app.world_mut().resource_mut::<ActorParts>().by_ref.insert(actor.0, PosedParts { data: FormId(0), weapon: None,
        segments: caps.iter().map(|p| PartSegment { part: p.part, limb: if p.part == 0 { fnv_combat::data::Limb::Head } else { fnv_combat::data::Limb::Torso }, a: p.a, b: p.b, radius: None }).collect() });
    for (part, point) in part_points.iter().enumerate() {
        let projected = camera_value.world_to_viewport(&transform, nvf_play::space::to_bevy(centers[part])).unwrap();
        assert!(projected.distance(*point) < 0.01, "actual projection matches the authored posed body");
    }
    vats_pick(&mut app, "vats-style", style);
    vats_pick(&mut app, nvf_play::vats_mode::touch::SETTING_KEY, mode);
    app.update();
    vats_raw_action(&mut app, window, 901, Control::Vats);
    assert!(app.world().resource::<nvf_play::vats_mode::VatsMode>().active(), "actual VATS touch entry must succeed");
    assert!(!app.world().resource::<nvf_play::vats_mode::VatsMode>().playing_back());
    for _ in 0..2 { app.update(); }
    assert_eq!(app.world().resource::<nvf_play::vats_mode::VatsMode>().classic_view().is_some(),style=="Classic");
    VatsParentScene { app, window, camera, actor, part_points, pad }
}

#[test]
fn actual_parent_vats_body_tap_queues_once_then_actual_fire_confirms_without_virtual_use() {
    let mut f = vats_parent_scene("Tap");
    let before = f.app.world().resource::<nvf_play::vats_mode::VatsMode>().reserved();
    let p = f.part_points[1];
    vats_edge(&mut f.app, f.window, 910, TouchPhase::Started, p); f.app.update();
    assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked().is_empty());
    vats_edge(&mut f.app, f.window, 910, TouchPhase::Ended, p); f.app.update();
    let v = f.app.world().resource::<nvf_play::vats_mode::VatsMode>();
    assert_eq!(v.marked(), vec![f.actor.0]); assert_eq!(v.reserved() - before, 20.0);
    let view = v.classic_view().unwrap();
    assert!(view.parts.iter().any(|part| part.selected && part.name == "part 1"));
    assert_eq!(view.queue.len(), 1); assert!(!v.playing_back()); assert_eq!(v.stats.shots, 0); assert_eq!(v.stats.ap_spent, 0.0);
    assert_eq!(f.app.world().resource::<VatsParentObserved>().uses, 0);
    // A duplicate terminal edge remains quarantined at the actual parent reader.
    vats_edge(&mut f.app, f.window, 910, TouchPhase::Ended, p); f.app.update();
    assert_eq!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().classic_view().unwrap().queue.len(), 1);
    vats_raw_action(&mut f.app, f.window, 911, Control::Use);
    assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().playing_back());
    assert_eq!(f.app.world().resource::<VatsParentObserved>().uses, 0, "enhanced touch Fire must not synthesize a generic Use edge");
    assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 1);
    f.app.update(); assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 1);
}

#[test]
fn actual_parent_vats_hold_progress_cancel_and_same_batch_move_off_never_auto_execute() {
    let mut f = vats_parent_scene("Hold"); let p = f.part_points[0];
    vats_edge(&mut f.app, f.window, 920, TouchPhase::Started, p); f.app.update();
    for _ in 0..3 { f.app.update(); }
    assert!(f.app.world().resource::<nvf_play::vats_mode::touch::State>().progress() > 0.0);
    vats_edge(&mut f.app, f.window, 920, TouchPhase::Canceled, p); f.app.update();
    assert_eq!(f.app.world().resource::<nvf_play::vats_mode::touch::State>().progress(), 0.0);
    assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked().is_empty());
    f.app.update(); vats_edge(&mut f.app, f.window, 922, TouchPhase::Started, p); f.app.update();
    vats_edge(&mut f.app, f.window, 922, TouchPhase::Moved, Vec2::splat(2.0));
    vats_edge(&mut f.app, f.window, 922, TouchPhase::Ended, p); f.app.update();
    assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked().is_empty(), "out-and-back path cannot become a held selection");
    f.app.update(); vats_edge(&mut f.app, f.window, 921, TouchPhase::Started, p); f.app.update();
    for _ in 0..20 { f.app.update(); }
    let v = f.app.world().resource::<nvf_play::vats_mode::VatsMode>();
    assert_eq!(v.marked(), vec![f.actor.0]); assert_eq!(v.reserved(), 20.0);
    assert!(!v.playing_back()); assert_eq!(v.stats.shots, 0);
    vats_edge(&mut f.app, f.window, 921, TouchPhase::Ended, p); f.app.update();
    assert_eq!(f.app.world().resource::<VatsParentObserved>().uses, 0);
    assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 0);
}

#[test]
fn actual_parent_vats_late_camera_secondary_finger_and_physical_mouse_preserve_authority() {
    for case in 0..3 {
        let mut f = vats_parent_scene("Tap"); let p = f.part_points[0];
        vats_edge(&mut f.app, f.window, 930, TouchPhase::Started, p); f.app.update();
        match case {
            0 => { f.app.world_mut().get_mut::<Camera>(f.camera).unwrap().computed.target_info = None; }
            1 => { vats_edge(&mut f.app, f.window, 931, TouchPhase::Started, p); }
            _ => {
                f.app.world_mut().write_message(MouseButtonInput { window: f.window, button: MouseButton::Left, state: ButtonState::Pressed });
            }
        }
        vats_edge(&mut f.app, f.window, 930, TouchPhase::Ended, p); f.app.update();
        let v = f.app.world().resource::<nvf_play::vats_mode::VatsMode>();
        assert_eq!(v.marked().len(), usize::from(case == 2), "case{case}: only actual hardware Use can queue");
        assert!(!v.playing_back()); assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 0);
        assert_eq!(f.app.world().resource::<VatsParentObserved>().uses, usize::from(case == 2));
        if case == 2 {
            assert!(f.app.world().resource::<TouchState>().physical_mouse.contains(&MouseButton::Left));
            assert!(f.app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        }
    }
}

#[test]
fn actual_parent_vats_off_physical_queue_accept_and_controller_trigger_south_remain_baseline() {
    for controller in [false, true] {
        let mut f = vats_parent_scene("Off");
        if controller {
            nvf_play::bindings::set_device(f.app.world_mut(), nvf_play::bindings::InputDevice::Controller);
            f.app.update(); f.app.update();
            f.app.world_mut().write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(f.pad, GamepadButton::RightTrigger2, 1.0)));
        } else {
            f.app.world_mut().write_message(MouseButtonInput { window: f.window, button: MouseButton::Left, state: ButtonState::Pressed });
        }
        f.app.update();
        let v = f.app.world().resource::<nvf_play::vats_mode::VatsMode>();
        assert_eq!(v.marked().len(), 1); assert_eq!(v.reserved(), 20.0); assert!(!v.playing_back());
        if controller {
            f.app.world_mut().write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(f.pad, GamepadButton::RightTrigger2, 0.0)));
            f.app.update();
            f.app.world_mut().write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(f.pad, GamepadButton::South, 1.0)));
        } else {
            f.app.world_mut().write_message(MouseButtonInput { window: f.window, button: MouseButton::Left, state: ButtonState::Released });
            f.app.update(); raw_key(&mut f.app, f.window, KeyCode::KeyE, ButtonState::Pressed);
        }
        f.app.update();
        assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().playing_back());
        assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 1);
    }
}

#[test]
fn actual_parent_vats_physical_return_retires_first_batch_and_next_fresh_touch_queues() {
    let mut f = vats_parent_scene("Tap"); let body = f.part_points[0];
    f.app.world_mut().write_message(CursorMoved { window: f.window, position: Vec2::splat(20.0), delta: Some(Vec2::ONE) });
    f.app.update();
    assert_eq!(f.app.world().resource::<Bindings>().device, nvf_play::bindings::InputDevice::KeyboardMouse);
    assert!(f.app.world().resource::<TouchState>().hardware_intent);
    let fire = vats_action_point(&f.app, Control::Use);
    for (id, at) in [(950, fire), (951, body)] {
        vats_edge(&mut f.app, f.window, id, TouchPhase::Started, at);
        vats_edge(&mut f.app, f.window, id, TouchPhase::Ended, at);
    }
    f.app.update();
    assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked().is_empty());
    assert!(!f.app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(950));
    assert!(!f.app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(951));
    assert_eq!(f.app.world().resource::<VatsParentObserved>().uses, 0);
    assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 0);
    vats_edge(&mut f.app, f.window, 952, TouchPhase::Started, body);
    vats_edge(&mut f.app, f.window, 952, TouchPhase::Ended, body);
    f.app.update();
    assert_eq!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked(), vec![f.actor.0]);
    assert_eq!(f.app.world().resource::<VatsParentObserved>().uses, 0);
}

#[test]
fn actual_parent_vats_controller_return_masks_real_batch_then_next_tap_needs_no_idle_frame() {
    let mut f = vats_parent_scene("Tap"); let body = f.part_points[0];
    nvf_play::bindings::set_device(f.app.world_mut(), nvf_play::bindings::InputDevice::Controller);
    f.app.update();
    assert_eq!(f.app.world().resource::<Bindings>().device, nvf_play::bindings::InputDevice::Controller);
    let fire = vats_action_point(&f.app, Control::Use);
    for (id, at) in [(960, body), (961, fire)] {
        vats_edge(&mut f.app, f.window, id, TouchPhase::Started, at);
        vats_edge(&mut f.app, f.window, id, TouchPhase::Ended, at);
    }
    f.app.update();
    assert_eq!(f.app.world().resource::<Bindings>().device, nvf_play::bindings::InputDevice::KeyboardMouse);
    assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked().is_empty());
    assert!(!f.app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(960));
    assert!(!f.app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(961));
    assert!(!f.app.world().resource::<nvf_play::pad::TouchDeviceSwitch>().allows_actions());
    // No intervening update: this exposes stale pre-selector Switch admission.
    vats_edge(&mut f.app, f.window, 962, TouchPhase::Started, body);
    vats_edge(&mut f.app, f.window, 962, TouchPhase::Ended, body);
    f.app.update();
    assert_eq!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked(), vec![f.actor.0]);
    assert_eq!(f.app.world().resource::<VatsParentObserved>().uses, 0);
    assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 0);
}

#[test]
fn actual_parent_vats_fire_ownership_continues_outside_button_and_never_emits_use() {
    let mut f = vats_parent_scene("Tap"); let body = f.part_points[0];
    vats_edge(&mut f.app, f.window, 970, TouchPhase::Started, body);
    vats_edge(&mut f.app, f.window, 970, TouchPhase::Ended, body); f.app.update();
    assert_eq!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked(), vec![f.actor.0]);
    let fire = vats_action_point(&f.app, Control::Use);
    vats_edge(&mut f.app, f.window, 971, TouchPhase::Started, fire); f.app.update();
    assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 1);
    vats_edge(&mut f.app, f.window, 971, TouchPhase::Moved, body);
    vats_edge(&mut f.app, f.window, 971, TouchPhase::Ended, Vec2::splat(2.0)); f.app.update();
    assert_eq!(f.app.world().resource::<VatsParentObserved>().uses, 0);
    assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 1);
    assert!(!f.app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(971));
}

#[test]
fn actual_parent_vats_unread_terminal_batch_is_not_neutral_and_later_neutral_recovers() {
    let mut f = vats_parent_scene("Tap"); let body = f.part_points[0];
    vats_edge(&mut f.app, f.window, 980, TouchPhase::Started, body);
    vats_edge(&mut f.app, f.window, 980, TouchPhase::Canceled, body); f.app.update();
    // Reuse without any real neutral frame must not revive a canceled contact.
    vats_edge(&mut f.app, f.window, 980, TouchPhase::Started, body);
    vats_edge(&mut f.app, f.window, 980, TouchPhase::Ended, body); f.app.update();
    assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked().is_empty());
    // One update still sees the terminal Touches. The next one is genuinely idle.
    f.app.update(); f.app.update();
    vats_edge(&mut f.app, f.window, 980, TouchPhase::Started, body);
    vats_edge(&mut f.app, f.window, 980, TouchPhase::Ended, body); f.app.update();
    assert_eq!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked(), vec![f.actor.0]);
    assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 0);
}

#[test]
fn actual_parent_vats_secondary_action_finger_cancels_hold_without_generic_fire() {
    let mut f = vats_parent_scene("Hold"); let body = f.part_points[0];
    vats_edge(&mut f.app, f.window, 990, TouchPhase::Started, body); f.app.update();
    for _ in 0..3 { f.app.update(); }
    assert!(f.app.world().resource::<nvf_play::vats_mode::touch::State>().progress() > 0.0);
    vats_edge(&mut f.app, f.window, 998, TouchPhase::Ended, body); f.app.update();
    assert!(f.app.world().resource::<nvf_play::vats_mode::touch::State>().progress() > 0.0,
        "an unrelated terminal edge does not steal the actual held owner");
    let fire = vats_action_point(&f.app, Control::Use);
    vats_edge(&mut f.app, f.window, 991, TouchPhase::Started, fire);
    vats_edge(&mut f.app, f.window, 990, TouchPhase::Ended, body);
    vats_edge(&mut f.app, f.window, 991, TouchPhase::Ended, fire); f.app.update();
    assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked().is_empty());
    assert_eq!(f.app.world().resource::<VatsParentObserved>().uses, 0);
    assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 0);
}

#[test]
fn actual_parent_without_core_vats_resources_keeps_normal_touch_fire() {
    let (mut app, window) = scene();
    assert!(!app.world().contains_resource::<nvf_play::vats_mode::touch::State>());
    assert!(!app.world().contains_resource::<nvf_play::vats_mode::VatsMode>());
    let fire = vats_action_point(&app, Control::Use);
    vats_edge(&mut app, window, 995, TouchPhase::Started, fire); app.update();
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    vats_edge(&mut app, window, 995, TouchPhase::Ended, fire); app.update();
    assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
}

#[test]
fn actual_parent_vats_secondary_window_interrupts_primary_hold_without_claiming_its_pointer() {
    let mut f = vats_parent_scene("Hold"); let body = f.part_points[0];
    vats_edge(&mut f.app, f.window, 996, TouchPhase::Started, body); f.app.update();
    for _ in 0..3 { f.app.update(); }
    assert!(f.app.world().resource::<nvf_play::vats_mode::touch::State>().progress() > 0.0);
    let other = f.app.world_mut().spawn(Window::default()).id();
    vats_edge(&mut f.app, other, 997, TouchPhase::Started, body);
    vats_edge(&mut f.app, other, 997, TouchPhase::Ended, body); f.app.update();
    assert!(f.app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(997),
        "the primary VATS owner does not claim the secondary window's released pointer");
    for _ in 0..16 { f.app.update(); }
    assert_eq!(f.app.world().resource::<nvf_play::vats_mode::touch::State>().progress(), 0.0);
    assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked().is_empty());
    assert_eq!(f.app.world().resource::<VatsParentObserved>().uses, 0);
    assert_eq!(f.app.world().resource::<VatsParentObserved>().executions, 0);
    assert!(f.app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(997));
    vats_edge(&mut f.app, f.window, 996, TouchPhase::Ended, body); f.app.update();
    assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked().is_empty());
}

#[test]
fn actual_parent_mark_and_fire_tap_and_hold_queue_body_then_only_separate_fire_executes() {
    for mode in ["Tap","Hold"] {
        let mut f=vats_parent_scene_with_style(mode,"Mark and Fire");
        let at=f.part_points[1];
        assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().selected().is_none());
        vats_edge(&mut f.app,f.window,1500,TouchPhase::Started,at);f.app.update();
        assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().marked().is_empty());
        if mode=="Hold" {for _ in 0..20 {f.app.update();}}
        else {vats_edge(&mut f.app,f.window,1500,TouchPhase::Ended,at);f.app.update();}
        let v=f.app.world().resource::<nvf_play::vats_mode::VatsMode>();
        assert_eq!(v.marked(),vec![f.actor.0]);assert_eq!(v.reserved(),20.0);
        assert!(!v.playing_back());assert_eq!(v.stats.shots,0);
        assert_eq!(v.highlighted(),Some((f.actor.0,Some(1))));
        assert_eq!(f.app.world().resource::<VatsParentObserved>().uses,0);
        assert_eq!(f.app.world().resource::<VatsParentObserved>().executions,0);
        if mode=="Hold" {vats_edge(&mut f.app,f.window,1500,TouchPhase::Ended,at);f.app.update();}
        f.app.update();
        vats_raw_action(&mut f.app,f.window,1501,Control::Use);
        assert!(f.app.world().resource::<nvf_play::vats_mode::VatsMode>().playing_back());
        assert_eq!(f.app.world().resource::<VatsParentObserved>().uses,0,"dedicated Fire must not synthesize a physical Use edge");
        assert_eq!(f.app.world().resource::<VatsParentObserved>().executions,1);
    }
}
}
