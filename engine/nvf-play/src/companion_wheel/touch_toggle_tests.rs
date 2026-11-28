fn optional_key(app: &mut App, window: Entity, key_code: KeyCode, down: bool) {
    app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
        key_code, logical_key: bevy::input::keyboard::Key::Character("optional menu fixture".into()),
        state: if down { bevy::input::ButtonState::Pressed } else { bevy::input::ButtonState::Released },
        text: None, repeat: false, window,
    });
    app.update();
}
fn optional_flip(app: &mut App) -> bool {
    let i = app.world().resource::<crate::settings::Settings>().index("touch-companion-v1").unwrap();
    crate::settings::flip(app.world_mut(), i).unwrap()
}

#[test]
fn registered_wheel_toggle_retires_old_release_without_changing_drag_mode_or_game() {
    let (mut app, w) = scene();
    register(&mut app);
    app.world_mut().resource_mut::<Settings>().drag_release = true;
    event(&mut app, w, 72, TouchPhase::Started, at(Command::BackUp));
    app.update();
    optional_key(&mut app, w, KeyCode::KeyW, true);
    assert!(!optional_flip(&mut app));
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
    let retired = app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    assert_eq!(retired.contacts().collect::<Vec<_>>(), [(w, 72)]);
    assert!(retired.changed(crate::touch_ui_options::Feature::Companion));
    assert!(app.world().resource::<Settings>().drag_release);
    assert!(!app.world().resource::<Settings>().allow_write, "toggle retains JSON write protection");
    assert!(optional_flip(&mut app));
    event(&mut app, w, 72, TouchPhase::Ended, at(Command::BackUp));
    app.update(); app.update();
    assert!(app.world().resource::<WheelStats>().commands.is_empty());
    app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    tap(&mut app, w, at(Command::BackUp));
    assert_eq!(app.world().resource::<WheelStats>().commands, [Command::BackUp.name()]);
    let mut script = app.world_mut().resource_mut::<ScriptState>();
    let game = &mut script.rt.as_mut().unwrap().game;
    assert!(game.take_back_up(FormId(0x99)).is_some());
    assert!(game.take_back_up(FormId(0x99)).is_none());
}

#[test]
fn wheel_off_standard_keys_reach_real_game_command_and_exit() {
    let (mut app, w) = scene();
    register(&mut app);
    app.add_systems(Update, super::super::keys.after(input).before(super::super::apply));
    assert!(!optional_flip(&mut app));
    tap(&mut app, w, at(Command::BackUp));
    assert!(app.world().resource::<WheelStats>().commands.is_empty());
    for _ in 0..Command::ALL.len() {
        optional_key(&mut app, w, KeyCode::ArrowRight, true);
        optional_key(&mut app, w, KeyCode::ArrowRight, false);
        let s = app.world().resource::<WheelScreen>().0.as_ref().unwrap();
        if s.selected.is_some_and(|i| s.buttons[i].command == Command::BackUp) { break; }
    }
    optional_key(&mut app, w, KeyCode::Enter, true);
    assert_eq!(app.world().resource::<WheelStats>().commands, [Command::BackUp.name()]);
    {
        let mut script = app.world_mut().resource_mut::<ScriptState>();
        assert!(script.rt.as_mut().unwrap().game.take_back_up(FormId(0x99)).is_some());
    }
    optional_key(&mut app, w, KeyCode::Enter, false);
    optional_key(&mut app, w, KeyCode::Escape, true);
    assert!(app.world().resource::<WheelScreen>().0.is_none());
}

#[test]
fn registered_wheel_switch_restores_independently_of_legacy_drag_preference() {
    let dir = std::env::temp_dir().join(format!("nvf-wheel-toggle-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("settings.ini");
    let (mut app, _) = scene();
    register(&mut app);
    app.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
    assert!(!optional_flip(&mut app));
    let (mut restored, w) = scene();
    register(&mut restored);
    restored.world_mut().resource_mut::<Settings>().drag_release = true;
    restored.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
    crate::settings::apply_stored(restored.world_mut());
    assert!(!restored.world().resource::<Settings>().enabled);
    assert!(restored.world().resource::<Settings>().drag_release);
    tap(&mut restored, w, at(Command::BackUp));
    assert!(restored.world().resource::<WheelStats>().commands.is_empty());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn late_wheel_off_on_drops_only_its_old_tap_and_preserves_other_feature_input() {
    #[derive(Resource)] struct Change(bool);
    fn late(world: &mut World) {
        if !std::mem::take(&mut world.resource_mut::<Change>().0) { return; }
        assert!(world.resource::<bevy::input::touch::Touches>().any_just_released());
        let row = world.resource::<crate::settings::Settings>().index("touch-companion-v1").unwrap();
        assert_eq!(crate::settings::flip(world, row), Some(false));
        assert_eq!(crate::settings::flip(world, row), Some(true));
    }
    let (mut app, w) = scene();
    register(&mut app);
    app.insert_resource(Change(true)).add_systems(Update, late.before(input));
    event(&mut app, w, 992, TouchPhase::Started, at(Command::BackUp));
    event(&mut app, w, 992, TouchPhase::Ended, at(Command::BackUp));
    app.update();
    assert!(app.world().resource::<WheelStats>().commands.is_empty());
    assert!(app.world().resource::<TouchState>().gesture.is_none());
    app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    app.update();
    assert!(app.world().resource::<WheelStats>().commands.is_empty());
    crate::terminal::touch::register(&mut app);
    let row = app.world().resource::<crate::settings::Settings>().index("touch-terminal-v1").unwrap();
    crate::settings::flip(app.world_mut(), row);
    assert!(app.world().resource::<crate::touch_ui_options::Retirements>().changed(crate::touch_ui_options::Feature::Terminal));
    tap(&mut app, w, at(Command::BackUp));
    assert_eq!(app.world().resource::<WheelStats>().commands, [Command::BackUp.name()]);
    let mut script = app.world_mut().resource_mut::<ScriptState>();
    assert!(script.rt.as_mut().unwrap().game.take_back_up(FormId(0x99)).is_some());
}

#[test]
fn standalone_wheel_off_on_accepts_fresh_command_without_losing_pending_retirement() {
    let (mut app, w) = scene();
    register(&mut app);
    event(&mut app, w, 1102, TouchPhase::Started, at(Command::BackUp));
    app.update();
    assert!(!optional_flip(&mut app));
    assert!(optional_flip(&mut app));
    event(&mut app, w, 1102, TouchPhase::Ended, at(Command::BackUp));
    app.update();
    assert!(app.world().resource::<WheelStats>().commands.is_empty());
    let pending = app.world().resource::<crate::touch_ui_options::Retirements>();
    assert!(pending.changed(crate::touch_ui_options::Feature::Companion));
    assert!(!pending.barrier(crate::touch_ui_options::Feature::Companion));
    assert_eq!(pending.contacts().collect::<Vec<_>>(), [(w, 1102)]);
    tap(&mut app, w, at(Command::BackUp));
    assert_eq!(app.world().resource::<WheelStats>().commands, [Command::BackUp.name()]);
    let mut script = app.world_mut().resource_mut::<ScriptState>();
    assert!(script.rt.as_mut().unwrap().game.take_back_up(FormId(0x99)).is_some());
    assert!(script.rt.as_mut().unwrap().game.take_back_up(FormId(0x99)).is_none());
}
