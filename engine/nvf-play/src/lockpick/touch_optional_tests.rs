#[test]
fn saved_off_choice_cancels_actual_torque_and_retains_size_before_fresh_arc_input() {
    let (mut app, window) = app();
    register(&mut app);
    app.update(); app.update();
    let row = app.world().resource::<settings::Settings>().index(SCHEME_KEY).unwrap();
    let turn = geometry().turn.center();
    touch(&mut app, window, 801, TouchPhase::Started, turn);
    app.update();
    assert!(app.world().resource::<HeldTorque>().touch);
    assert!(settings::pick(app.world_mut(), row, "Off"));
    let retired = app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    assert!(retired.changed(crate::touch_ui_options::Feature::Lockpick));
    assert_eq!(retired.contacts().collect::<Vec<_>>(), [(window, 801)]);
    touch(&mut app, window, 801, TouchPhase::Ended, turn);
    app.update();
    assert!(!app.world().resource::<HeldTorque>().touch);
    assert!(!active(app.world().resource::<LockpickScreen>(), app.world().resource::<Options>()));
    let size = app.world().resource::<Options>().size;
    for _ in 0..30 { app.update(); }
    assert_eq!(game(&app).turn, 0.0, "let existing torque return settle before off-input comparison");
    let before = game(&app).clone();
    touch(&mut app, window, 802, TouchPhase::Started, turn);
    app.update();
    touch(&mut app, window, 802, TouchPhase::Ended, turn);
    app.update();
    assert_eq!(game(&app), &before, "disabled enhanced touch cannot turn the lock");
    assert!(settings::pick(app.world_mut(), row, "Arc + turn"));
    app.update();
    app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    assert_eq!(app.world().resource::<Options>().size, size);
    touch(&mut app, window, 803, TouchPhase::Started, turn);
    app.update();
    assert!(app.world().resource::<HeldTorque>().touch);
    assert_ne!(game(&app), &before, "fresh enabled contact reaches real puzzle progression");
}

#[test]
fn late_lockpick_off_on_refuses_queued_touch_while_actual_held_pad_keeps_torque() {
    #[derive(Resource)] struct Change(bool);
    fn late(world: &mut World) {
        if !std::mem::take(&mut world.resource_mut::<Change>().0) { return; }
        assert!(world.resource::<bevy::input::touch::Touches>().get_pressed(993).is_some());
        let row = world.resource::<settings::Settings>().index(SCHEME_KEY).unwrap();
        assert!(settings::pick(world, row, "Off"));
        assert!(settings::pick(world, row, "Arc + turn"));
    }
    for controller in [false, true] {
        let (mut app, window) = app();
        app.add_plugins(bevy::input::InputPlugin);
        register(&mut app);
        if controller { app.world_mut().resource_mut::<crate::bindings::Bindings>().device = crate::bindings::InputDevice::Controller; }
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.update(); app.update(); app.update();
        if controller {
            app.world_mut().write_message(bevy::input::gamepad::RawGamepadEvent::Button(
                bevy::input::gamepad::RawGamepadButtonChangedEvent::new(pad, GamepadButton::RightTrigger2, 1.0)));
            app.update();
            assert!(app.world().resource::<HeldTorque>().pad);
        }
        app.insert_resource(Change(true)).add_systems(Update, late.before(input));
        let turn = geometry().turn.center();
        touch(&mut app, window, 993, TouchPhase::Started, turn);
        app.update();
        assert!(!app.world().resource::<HeldTorque>().touch);
        assert_eq!(app.world().resource::<HeldTorque>().pad, controller);
        assert_eq!(game(&app).torque, controller, "mode retirement is touch-only, including same-final-scheme off/on");
        app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
        touch(&mut app, window, 993, TouchPhase::Ended, turn);
        app.update();
        if !controller {
            touch(&mut app, window, 994, TouchPhase::Started, turn);
            app.update();
            assert!(game(&app).torque, "fresh touch works after owner takes mailbox");
        } else {
            assert!(app.world().resource::<HeldTorque>().pad);
        }
    }
}

#[test]
fn standalone_lockpick_off_on_keeps_pending_id_without_latching_touch_off() {
    let (mut app, window) = app();
    register(&mut app);
    app.update(); app.update();
    let row = app.world().resource::<settings::Settings>().index(SCHEME_KEY).unwrap();
    let turn = geometry().turn.center();
    touch(&mut app, window, 1103, TouchPhase::Started, turn);
    app.update();
    assert!(app.world().resource::<HeldTorque>().touch);
    assert!(settings::pick(app.world_mut(), row, "Off"));
    assert!(settings::pick(app.world_mut(), row, "Arc + turn"));
    touch(&mut app, window, 1103, TouchPhase::Ended, turn);
    app.update();
    assert!(!app.world().resource::<HeldTorque>().touch);
    let pending = app.world().resource::<crate::touch_ui_options::Retirements>();
    assert!(pending.changed(crate::touch_ui_options::Feature::Lockpick));
    assert!(!pending.barrier(crate::touch_ui_options::Feature::Lockpick));
    assert_eq!(pending.contacts().collect::<Vec<_>>(), [(window, 1103)]);
    touch(&mut app, window, 1104, TouchPhase::Started, turn);
    app.update();
    assert!(app.world().resource::<HeldTorque>().touch);
    assert!(game(&app).torque);
}
