// Mixed-device control through the real input/selector/gesture/control graph.
// No test writes a latch, controller hold, synthetic pointer, or resulting effect.
fn mixed_source_child(name: &str) -> bool {
    const CHILD: &str = "NVF_TEST_MIXED_CONTROLLER_SOURCE_CHILD";
    if std::env::var_os(CHILD).is_some() { return false; }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--test-threads=1"]).env(CHILD, "1").spawn().unwrap();
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => { assert!(status.success(), "isolated mixed-source control failed"); return true; }
            Ok(None) if start.elapsed() < std::time::Duration::from_secs(30) => std::thread::sleep(std::time::Duration::from_millis(10)),
            result => { let kill = child.kill(); let reap = child.wait(); panic!("mixed-source child deadline/wait: {result:?}; kill={kill:?}; reap={reap:?}"); }
        }
    }
}
fn physical_aim(app: &mut App, window: Entity, pressed: bool) {
    app.world_mut().write_message(bevy::input::mouse::MouseButtonInput {
        window, button: MouseButton::Right,
        state: if pressed { bevy::input::ButtonState::Pressed } else { bevy::input::ButtonState::Released },
    });
    app.update();
}
fn physical_sneak(app: &mut App, window: Entity, pressed: bool) {
    let key_code = app.world().resource::<Bindings>().get(Control::CrouchSneak).key.unwrap();
    app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
        window, key_code, logical_key: bevy::input::keyboard::Key::Character("x".into()),
        state: if pressed { bevy::input::ButtonState::Pressed } else { bevy::input::ButtonState::Released },
        repeat: false, text: None,
    });
    app.update();
}
#[test]
fn keys_with_pad_keeps_physical_aim_and_sneak_separate_from_real_controller_latches_through_coexistence_and_cancel() {
    let name = concat!(module_path!(), "::keys_with_pad_keeps_physical_aim_and_sneak_separate_from_real_controller_latches_through_coexistence_and_cancel");
    if mixed_source_child(name.split_once("::").unwrap().1) { return; }
    crate::bindings::set_keys_with_pad(true);
    let (mut app, pad, window) = scene();
    app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
    app.insert_resource(crate::tweaks::TweakSettings { hold_to_sneak: true, ..default() });
    options(&mut app, true, true);
    assert_eq!(app.world().resource::<Bindings>().get(Control::Block).mouse, Some(MouseButton::Right));
    physical_aim(&mut app, window, true);
    assert!(aiming(&app), "genuine physical aim remains an admitted native hold");
    assert!(!app.world().resource::<Gestures>().latched, "a mouse press cannot create a controller latch");
    physical_aim(&mut app, window, false);
    assert!(!aiming(&app), "real mouse release releases its independent hold");
    assert!(!app.world().resource::<Gestures>().latched);
    button(&mut app, pad, Control::Block, true);
    assert!(aiming(&app) && app.world().resource::<Gestures>().latched, "actual pad positive still latches");
    button(&mut app, pad, Control::Block, false);
    assert!(aiming(&app));
    physical_aim(&mut app, window, true);
    physical_aim(&mut app, window, false);
    assert!(aiming(&app) && app.world().resource::<Gestures>().latched, "physical release must not clear the pad latch");
    physical_aim(&mut app, window, true);
    button(&mut app, pad, Control::Block, true);
    assert!(aiming(&app) && !app.world().resource::<Gestures>().latched, "pad can toggle off while the physical hold remains");
    button(&mut app, pad, Control::Block, false);
    assert!(aiming(&app));
    physical_aim(&mut app, window, false);
    assert!(!aiming(&app));
    physical_sneak(&mut app, window, true);
    assert!(sneak(&app));
    assert!(!app.world().resource::<HeldSneakInput>().controller_active, "physical sneak is never owned by the controller");
    physical_sneak(&mut app, window, false);
    assert!(!sneak(&app));
    button(&mut app, pad, Control::CrouchSneak, true);
    assert!(sneak(&app) && app.world().resource::<HeldSneakInput>().controller_active);
    physical_sneak(&mut app, window, true);
    button(&mut app, pad, Control::CrouchSneak, false);
    assert!(sneak(&app) && !app.world().resource::<HeldSneakInput>().controller_active, "native hold survives the pad source release");
    physical_sneak(&mut app, window, false);
    assert!(!sneak(&app));
    button(&mut app, pad, Control::CrouchSneak, true);
    physical_sneak(&mut app, window, true);
    physical_sneak(&mut app, window, false);
    assert!(sneak(&app) && app.world().resource::<HeldSneakInput>().controller_active, "pad survives the native source release");
    button(&mut app, pad, Control::CrouchSneak, false);
    assert!(!sneak(&app));
    button(&mut app, pad, Control::Block, true); button(&mut app, pad, Control::Block, false);
    physical_aim(&mut app, window, true);
    crate::bindings::cancel_input_holds(app.world_mut()); app.update();
    assert!(!app.world().resource::<Gestures>().latched, "pad invalidation clears its latch");
    assert!(aiming(&app), "pad invalidation must not destroy an independent physical aim hold");
    physical_aim(&mut app, window, false);
    assert!(!aiming(&app));
    // A global cover quarantines both real sources through neutral, without raw resets.
    app.init_resource::<crate::pad::PlatformInputGate>();
    button(&mut app, pad, Control::Block, true); button(&mut app, pad, Control::Block, false);
    physical_aim(&mut app, window, true);
    physical_sneak(&mut app, window, true);
    button(&mut app, pad, Control::CrouchSneak, true);
    assert!(aiming(&app) && sneak(&app));
    app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true); app.update();
    assert!(!aiming(&app) && !sneak(&app));
    assert!(!app.world().resource::<Gestures>().latched);
    assert!(!app.world().resource::<HeldSneakInput>().controller_active);
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Right));
    let key = app.world().resource::<Bindings>().get(Control::CrouchSneak).key.unwrap();
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(key));
    let Some(PadInput::Button(sneak_button)) = app.world().resource::<Bindings>().pad(Control::CrouchSneak) else { panic!() };
    assert!(app.world().get::<Gamepad>(pad).unwrap().pressed(sneak_button));
    app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(false); app.update();
    assert!(!aiming(&app) && !sneak(&app), "held sources cannot resume through the closing cover");
    physical_aim(&mut app, window, false); physical_sneak(&mut app, window, false);
    button(&mut app, pad, Control::CrouchSneak, false); app.update();
    assert!(!aiming(&app) && !sneak(&app));
    physical_aim(&mut app, window, true); physical_sneak(&mut app, window, true);
    assert!(aiming(&app) && sneak(&app), "fresh physical contacts work after actual neutral");
    assert!(!app.world().resource::<Gestures>().latched && !app.world().resource::<HeldSneakInput>().controller_active);
    physical_aim(&mut app, window, false); physical_sneak(&mut app, window, false);
    assert!(!aiming(&app) && !sneak(&app));
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
}
