// Actual merged input consumers; authored records/events only, no renderer/game files.
fn main_input_child(name: &str) -> bool {
    const CHILD: &str = "NVF_TEST_MAIN_INPUT_UNION_CHILD";
    if std::env::var_os(CHILD).is_some() { return false; }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--test-threads=1"]).env(CHILD, "1").spawn().unwrap();
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => { assert!(status.success(), "isolated input control failed"); return true; }
            Ok(None) if start.elapsed() < std::time::Duration::from_secs(30) => std::thread::sleep(std::time::Duration::from_millis(10)),
            result => { let kill = child.kill(); let reap = child.wait(); panic!("input child deadline/wait: {result:?}; kill={kill:?}; reap={reap:?}"); }
        }
    }
}
fn main_input_key(app: &mut App, key_code: KeyCode, pressed: bool) {
    app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
        window: Entity::PLACEHOLDER, key_code,
        logical_key: bevy::input::keyboard::Key::Character("x".into()),
        state: if pressed { bevy::input::ButtonState::Pressed } else { bevy::input::ButtonState::Released },
        repeat: false, text: None,
    });
}
fn main_input_mouse(app: &mut App, pressed: bool) {
    app.world_mut().write_message(bevy::input::mouse::MouseButtonInput {
        window: Entity::PLACEHOLDER, button: MouseButton::Left,
        state: if pressed { bevy::input::ButtonState::Pressed } else { bevy::input::ButtonState::Released },
    });
}
#[test]
fn explicit_main_stick_preferences_restore_the_saved_android_zone_without_rewriting_it() {
    let name = concat!(module_path!(), "::explicit_main_stick_preferences_restore_the_saved_android_zone_without_rewriting_it");
    if main_input_child(name.split_once("::").unwrap().1) { return; }
    use bevy::{ecs::system::SystemState, input::gamepad::{GamepadAxis, RawGamepadAxisChangedEvent, RawGamepadEvent}};
    let (mut app, pad) = crate::pad::device::tests::playing_app(crate::bindings::InputDevice::Controller);
    app.insert_resource(crate::controller_aim::Options { dead_zone: 0.4, ..default() })
        .insert_resource(crate::tweaks::TweakSettings { left_dead_zone: 0.9, right_dead_zone: 0.0, ..default() })
        .add_systems(Update, crate::tweaks::sync.before(crate::control::read_controls));
    for axis in [GamepadAxis::LeftStickY, GamepadAxis::RightStickY] {
        app.world_mut().write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(pad, axis, 0.3)));
    }
    app.update();
    let raw = app.world().get::<Gamepad>(pad).unwrap().right_stick();
    assert!(raw.length() > 0.0 && raw.length() < 0.4, "real Bevy axis processing precedes this boundary");
    let mut state = SystemState::<crate::pad::ActionPads>::new(app.world_mut());
    { let pads = state.get(app.world()).unwrap();
      assert_eq!(crate::pad::stick(&pads, true), Vec2::ZERO);
      assert_eq!(crate::pad::stick(&pads, false), Vec2::ZERO); }
    app.world_mut().resource_mut::<crate::tweaks::TweakSettings>().custom_dead_zones = true; app.update();
    { let pads = state.get(app.world()).unwrap();
      assert_eq!(crate::pad::stick(&pads, true), Vec2::ZERO);
      assert_eq!(crate::pad::stick(&pads, false), raw);
      assert_eq!(pads.physical_stick(false), raw); }
    app.world_mut().resource_mut::<crate::tweaks::TweakSettings>().custom_dead_zones = false; app.update();
    { let pads = state.get(app.world()).unwrap(); assert_eq!(crate::pad::stick(&pads, false), Vec2::ZERO); }
    assert_eq!(app.world().resource::<crate::controller_aim::Options>().dead_zone, 0.4);
    assert_eq!(app.world().resource::<crate::tweaks::TweakSettings>().right_dead_zone, 0.0);
}
#[test]
fn tweak_device_edges_use_the_real_selector_cancel_holds_and_never_accept_synthetic_mouse_or_covered_edges() {
    let name = concat!(module_path!(), "::tweak_device_edges_use_the_real_selector_cancel_holds_and_never_accept_synthetic_mouse_or_covered_edges");
    if main_input_child(name.split_once("::").unwrap().1) { return; }
    use crate::bindings::{Control, InputDevice};
    let (mut app, pad) = crate::pad::device::tests::playing_app(InputDevice::KeyboardMouse);
    app.insert_resource(crate::tweaks::TweakSettings { toggle_device: true, ..default() })
        .init_resource::<crate::pad::PlatformInputGate>();
    app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
    app.world_mut().resource_mut::<PlayerControl>().ready_held = Some(0.1);
    app.world_mut().resource_mut::<PlayerControl>().reload_wait = Some(0.1);
    crate::pad::device::tests::button(&mut app, pad, true); app.update();
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
    assert!(app.world().resource::<PlayerControl>().ready_held.is_none());
    assert!(app.world().resource::<PlayerControl>().reload_wait.is_none());
    let mut input = bevy::ecs::system::SystemState::<ControlInput>::new(app.world_mut());
    assert!(!input.get(app.world()).unwrap().just_pressed(Control::Activate), "switching A is consumed");
    crate::pad::device::tests::button(&mut app, pad, false); app.update(); app.update();
    #[derive(Resource)] struct Virtual(bool);
    fn virtual_left(mut once: ResMut<Virtual>, mut mouse: ResMut<ButtonInput<MouseButton>>) {
        if std::mem::take(&mut once.0) { mouse.press(MouseButton::Left); }
    }
    app.insert_resource(Virtual(true)).add_systems(PreUpdate, virtual_left
        .after(crate::pad::InputSwitchSystems::ReadActivity).before(crate::pad::InputSwitchSystems::Switch));
    app.update();
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller, "aggregate touch-like Left is not hardware selection");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left); app.update();
    app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true);
    main_input_mouse(&mut app, true); app.update();
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
    app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(false); app.update();
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller, "covered held edge never becomes fresh");
    main_input_mouse(&mut app, false); app.update(); app.update();
    main_input_mouse(&mut app, true); app.update();
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    assert!(!input.get(app.world()).unwrap().just_pressed(Control::Use), "hardware return edge is consumed without deleting hardware");
}
#[test]
fn admitted_physical_keys_with_pad_and_device_only_look_tweaks_preserve_independent_gyro() {
    let name = concat!(module_path!(), "::admitted_physical_keys_with_pad_and_device_only_look_tweaks_preserve_independent_gyro");
    if main_input_child(name.split_once("::").unwrap().1) { return; }
    use crate::bindings::{Control, InputDevice};
    let (mut app, _) = crate::pad::device::tests::playing_app(InputDevice::Controller);
    app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
    crate::bindings::set_keys_with_pad(true);
    main_input_key(&mut app, KeyCode::KeyW, true); app.update();
    let mut input = bevy::ecs::system::SystemState::<ControlInput>::new(app.world_mut());
    assert!(input.get(app.world()).unwrap().pressed(Control::Forward));
    assert!(input.get(app.world()).unwrap().physical_pressed(Control::Forward));
    app.init_resource::<MoveIntent>().init_resource::<crate::furniture::PlayerSeat>()
        .add_systems(Update, read_input.before(crate::control::read_controls));
    app.update();
    assert_eq!(app.world().resource::<MoveIntent>().forward, 1.0, "idle pad cannot replace the admitted physical key");
    assert_eq!(app.world().resource::<MoveIntent>().analog_scale, 1.0);
    app.init_resource::<crate::script_input::ScriptBlocked>();
    app.world_mut().resource_mut::<crate::script_input::ScriptBlocked>().keys.push(KeyCode::KeyW);
    assert!(!input.get(app.world()).unwrap().pressed(Control::Forward));
    assert!(!input.get(app.world()).unwrap().physical_pressed(Control::Forward));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left); app.update();
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    assert!(!input.get(app.world()).unwrap().pressed(Control::Use), "virtual aggregate Left is not opt-in physical input");
    assert!(!input.get(app.world()).unwrap().physical_mouse_pressed(MouseButton::Left));
    main_input_mouse(&mut app, true); app.update();
    assert!(input.get(app.world()).unwrap().pressed(Control::Use));
    assert!(input.get(app.world()).unwrap().physical_mouse_pressed(MouseButton::Left));
    app.init_resource::<crate::pad::PlatformInputGate>();
    app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true); app.update();
    assert!(!input.get(app.world()).unwrap().physical_mouse_pressed(MouseButton::Left));
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    crate::bindings::set_keys_with_pad(false);
    let mut look = gyro_look_app();
    look.insert_resource(crate::tweaks::TweakSettings { invert_x: true, horizontal_look: 2.0, aim_turn_x: 0.5, aim_turn_y: 0.5, ..default() });
    { let mut b = look.world_mut().resource_mut::<Bindings>(); b.invert_y = true; b.mouse_sensitivity = 0.01; }
    { let mut c = look.world_mut().resource_mut::<PlayerControl>(); c.aiming = true; c.weapon_out = true; }
    look.world_mut().resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(10.0, 5.0);
    look.update();
    let p = look.world().resource::<Player>();
    assert!((p.heading - 0.92).abs() < 1e-6, "device -0.1 plus independent gyro +0.02");
    assert!((p.pitch - 0.035).abs() < 1e-6, "device inverted +0.025 plus independent gyro contribution +0.01");
    assert_eq!(look.world().resource::<GyroLook>().radians, Vec2::new(0.02, -0.01));
}
#[test]
fn platform_cancellation_retires_deferred_reload_and_publishes_neutral_jump_before_any_return() {
    let name = concat!(module_path!(), "::platform_cancellation_retires_deferred_reload_and_publishes_neutral_jump_before_any_return");
    if main_input_child(name.split_once("::").unwrap().1) { return; }
    use crate::bindings::{Control, InputDevice};
    for device in [InputDevice::KeyboardMouse, InputDevice::Controller] {
        let (mut app, pad) = crate::pad::device::tests::playing_app(device);
        app.insert_resource(crate::tweaks::TweakSettings { double_tap_ammo: true, ..default() })
            .init_resource::<crate::pad::PlatformInputGate>();
        app.world_mut().resource_mut::<PlayerControl>().weapon_out = true;
        crate::pad::device::tests::control_button(&mut app, pad, device, Control::ReadyItem, true); app.update();
        crate::pad::device::tests::control_button(&mut app, pad, device, Control::ReadyItem, false); app.update();
        assert!(app.world().resource::<PlayerControl>().reload_wait.is_some());
        assert!(!app.world().resource::<PlayerControl>().reload);
        app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true); app.update();
        assert!(app.world().resource::<PlayerControl>().reload_wait.is_none());
        for _ in 0..10 { app.update(); }
        assert!(!app.world().resource::<PlayerControl>().reload);
        app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(false); app.update();
        crate::pad::device::tests::control_button(&mut app, pad, device, Control::ReadyItem, true); app.update();
        crate::pad::device::tests::control_button(&mut app, pad, device, Control::ReadyItem, false); app.update();
        for _ in 0..10 { app.update(); }
        assert!(app.world().resource::<PlayerControl>().reload, "fresh admitted native timing still reloads");
    }
    let (mut app, pad) = crate::pad::device::tests::playing_app(InputDevice::KeyboardMouse);
    app.init_resource::<MoveIntent>().init_resource::<crate::furniture::PlayerSeat>()
        .init_resource::<crate::pad::PlatformInputGate>()
        .add_systems(Update, read_input.before(crate::control::read_controls));
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::KeyboardMouse, Control::Jump, true); app.update();
    assert!(JUMP_HELD.load(std::sync::atomic::Ordering::Relaxed));
    app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true); app.update();
    assert!(!JUMP_HELD.load(std::sync::atomic::Ordering::Relaxed));
    assert!(!app.world().resource::<MoveIntent>().jump);
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(app.world().resource::<Bindings>().get(Control::Jump).key.unwrap()));
}
#[test]
fn native_hold_sneak_and_controller_hold_preserve_the_independent_base_and_real_keyboard_release() {
    let name = concat!(module_path!(), "::native_hold_sneak_and_controller_hold_preserve_the_independent_base_and_real_keyboard_release");
    if main_input_child(name.split_once("::").unwrap().1) { return; }
    use crate::bindings::{Control, InputDevice};
    crate::bindings::set_keys_with_pad(true);
    let (mut app, pad) = crate::pad::device::tests::playing_app(InputDevice::Controller);
    app.add_plugins(crate::controller_aim::ControllerAimPlugin)
        .insert_resource(crate::tweaks::TweakSettings { hold_to_sneak: true, ..default() });
    app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
    app.world_mut().resource_mut::<crate::controller_aim::Options>().hold_sneak = true;
    app.update(); app.update();
    app.world_mut().resource_mut::<PlayerControl>().sneak = true;
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::Controller, Control::CrouchSneak, true); app.update();
    assert!(app.world().resource::<crate::control::HeldSneakInput>().controller_active);
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::Controller, Control::CrouchSneak, false); app.update();
    assert!(app.world().resource::<PlayerControl>().sneak, "pad hold restores independently true native base");
    app.world_mut().resource_mut::<PlayerControl>().sneak = false;
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::Controller, Control::CrouchSneak, true); app.update();
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::KeyboardMouse, Control::CrouchSneak, true); app.update();
    assert!(app.world().resource::<PlayerControl>().sneak);
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::KeyboardMouse, Control::CrouchSneak, false); app.update();
    assert!(app.world().resource::<PlayerControl>().sneak, "independent pad hold remains");
    crate::pad::device::tests::control_button(&mut app, pad, InputDevice::Controller, Control::CrouchSneak, false); app.update();
    assert!(!app.world().resource::<PlayerControl>().sneak, "last source release restores false native base");
    let (mut native, native_pad) = crate::pad::device::tests::playing_app(InputDevice::KeyboardMouse);
    native.insert_resource(crate::tweaks::TweakSettings { hold_to_sneak: true, ..default() })
        .init_resource::<crate::pad::PlatformInputGate>();
    crate::pad::device::tests::control_button(&mut native, native_pad, InputDevice::KeyboardMouse, Control::CrouchSneak, true); native.update();
    assert!(native.world().resource::<PlayerControl>().sneak);
    native.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true); native.update();
    assert!(!native.world().resource::<PlayerControl>().sneak, "only the actual native momentary source is canceled by the cover");
    native.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(false); native.update();
    assert!(!native.world().resource::<PlayerControl>().sneak, "held covered key cannot revive the canceled source");
    crate::pad::device::tests::control_button(&mut native, native_pad, InputDevice::KeyboardMouse, Control::CrouchSneak, false); native.update();
    crate::pad::device::tests::control_button(&mut native, native_pad, InputDevice::KeyboardMouse, Control::CrouchSneak, true); native.update();
    assert!(native.world().resource::<PlayerControl>().sneak, "fresh native hold works after neutral");
}
