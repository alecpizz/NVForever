fn optional_key(app: &mut App, window: Entity, key_code: KeyCode, down: bool) {
    app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
        key_code, logical_key: bevy::input::keyboard::Key::Character("optional menu fixture".into()),
        state: if down { bevy::input::ButtonState::Pressed } else { bevy::input::ButtonState::Released },
        text: None, repeat: false, window,
    });
    app.update();
}
fn optional_flip(app: &mut App) -> bool {
    let i = app.world().resource::<crate::settings::Settings>().index("touch-terminal-v1").unwrap();
    crate::settings::flip(app.world_mut(), i).unwrap()
}

#[test]
fn registered_terminal_toggle_retires_only_its_owned_contact_and_keeps_preferences() {
    let (mut app, w) = scene();
    register(&mut app);
    app.init_resource::<crate::companion_wheel::touch::Settings>();
    let other = app.world().resource::<crate::companion_wheel::touch::Settings>().enabled;
    app.world_mut().resource_mut::<Settings>().zoom = 2;
    let p = at(&app, 14);
    event(&mut app, w, 71, TouchPhase::Started, p);
    app.update();
    assert!(app.world().resource::<TouchState>().gesture.is_some());
    optional_key(&mut app, w, KeyCode::KeyW, true);
    assert!(!optional_flip(&mut app));
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
    let retired = app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    assert!(retired.changed(crate::touch_ui_options::Feature::Terminal));
    assert_eq!(retired.contacts().collect::<Vec<_>>(), [(w, 71)]);
    assert!(!retired.overflowed());
    assert!(app.world().resource::<TouchState>().gesture.is_none());
    assert!(!app.world().resource::<TouchState>().options);
    assert_eq!(app.world().resource::<Settings>().zoom, 2);
    assert!(!app.world().resource::<Settings>().allow_write, "toggle retains JSON write protection");
    assert_eq!(app.world().resource::<crate::companion_wheel::touch::Settings>().enabled, other);
    assert!(optional_flip(&mut app));
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
    event(&mut app, w, 71, TouchPhase::Ended, p);
    app.update();
    assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
    app.update();
    assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
    app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    tap(&mut app, w, p);
    let guess = Geometry::new(Vec2::new(792.0, 363.0), 2).action(4).center();
    tap(&mut app, w, guess);
    assert_eq!(hacking(&app).puzzle.state, GameState::Granted);
    assert_eq!(app.world().resource::<TerminalStats>().hacked, 1);
}

#[test]
fn terminal_off_uses_real_standard_key_navigation_guess_and_cancel() {
    let (mut app, w) = scene();
    register(&mut app);
    app.world_mut().resource_mut::<TouchState>().options = true;
    assert!(!optional_flip(&mut app));
    app.update(); // Retire only the old enhanced contact/configuration.
    assert!(!captures(app.world().resource::<Settings>(), app.world().resource::<TerminalScreen>()));
    let p = at(&app, 14);
    tap(&mut app, w, p);
    assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
    optional_key(&mut app, w, KeyCode::ArrowRight, true);
    optional_key(&mut app, w, KeyCode::ArrowRight, false);
    assert!(!hacking(&app).hidden);
    optional_key(&mut app, w, KeyCode::Enter, true);
    assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS - 1);
    assert_eq!(app.world().resource::<TerminalStats>().guesses, 1);
    optional_key(&mut app, w, KeyCode::Enter, false);
    optional_key(&mut app, w, KeyCode::Escape, true);
    assert!(matches!(app.world().resource::<NextState<PlayState>>(), NextState::Pending(PlayState::Playing)));
}

#[test]
fn registered_terminal_switch_restores_from_ini_without_replacing_legacy_preferences() {
    let dir = std::env::temp_dir().join(format!("nvf-terminal-toggle-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("settings.ini");
    let (mut app, _) = scene();
    register(&mut app);
    app.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
    assert!(!optional_flip(&mut app));
    assert!(std::fs::read_to_string(&path).unwrap().contains("touch-terminal-v1"));
    let (mut restored, w) = scene();
    register(&mut restored);
    restored.world_mut().resource_mut::<Settings>().zoom = 2;
    restored.world_mut().resource_mut::<Settings>().scheme = Scheme::Cursor;
    restored.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
    crate::settings::apply_stored(restored.world_mut());
    assert!(!restored.world().resource::<Settings>().enabled);
    assert_eq!(restored.world().resource::<Settings>().zoom, 2);
    assert_eq!(restored.world().resource::<Settings>().scheme, Scheme::Cursor);
    tap(&mut restored, w, at(&app, 14));
    assert_eq!(restored.world().resource::<TerminalStats>().guesses, 0);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn late_terminal_off_on_after_input_admission_cannot_replay_the_old_raw_tap() {
    #[derive(Resource)] struct Change(bool);
    fn late(world: &mut World) {
        if !std::mem::take(&mut world.resource_mut::<Change>().0) { return; }
        assert!(world.resource::<bevy::input::touch::Touches>().any_just_released());
        assert!(world.resource::<Messages<TouchInput>>().len() >= 2);
        let row = world.resource::<crate::settings::Settings>().index("touch-terminal-v1").unwrap();
        assert_eq!(crate::settings::flip(world, row), Some(false));
        assert_eq!(crate::settings::flip(world, row), Some(true));
    }
    let (mut app, w) = scene();
    register(&mut app);
    app.world_mut().resource_mut::<Settings>().separate_confirm = false;
    app.insert_resource(Change(true)).add_systems(Update, late.before(input));
    let word = at(&app, 14);
    event(&mut app, w, 991, TouchPhase::Started, word);
    event(&mut app, w, 991, TouchPhase::Ended, word);
    app.update();
    assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
    assert_eq!(hacking(&app).puzzle.state, GameState::Guessing);
    assert!(app.world().resource::<TouchState>().gesture.is_none());
    app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    app.update();
    assert_eq!(app.world().resource::<TerminalStats>().guesses, 0, "discarded reader batch cannot replay after publication");
    tap(&mut app, w, word);
    assert_eq!(hacking(&app).puzzle.state, GameState::Granted);
    assert_eq!(app.world().resource::<TerminalStats>().hacked, 1);
}

#[test]
fn standalone_terminal_off_on_expires_leaf_barrier_but_keeps_root_contacts() {
    let (mut app, w) = scene();
    register(&mut app);
    app.world_mut().resource_mut::<Settings>().separate_confirm = false;
    let word = at(&app, 14);
    event(&mut app, w, 1101, TouchPhase::Started, word);
    app.update();
    assert!(app.world().resource::<TouchState>().gesture.is_some());
    assert!(!optional_flip(&mut app));
    assert!(optional_flip(&mut app));
    assert!(app.world().resource::<crate::touch_ui_options::Retirements>().barrier(crate::touch_ui_options::Feature::Terminal));
    event(&mut app, w, 1101, TouchPhase::Ended, word);
    app.update();
    assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
    let pending = app.world().resource::<crate::touch_ui_options::Retirements>();
    assert!(pending.changed(crate::touch_ui_options::Feature::Terminal));
    assert!(!pending.barrier(crate::touch_ui_options::Feature::Terminal));
    assert_eq!(pending.contacts().collect::<Vec<_>>(), [(w, 1101)]);
    tap(&mut app, w, word);
    assert_eq!(hacking(&app).puzzle.state, GameState::Granted);
    assert_eq!(app.world().resource::<TerminalStats>().hacked, 1);
    assert!(app.world().resource::<crate::touch_ui_options::Retirements>().changed(crate::touch_ui_options::Feature::Terminal));
}

#[test]
fn standalone_restored_terminal_on_accepts_fresh_contact_without_a_wrapper_take() {
    let dir = std::env::temp_dir().join(format!("nvf-terminal-on-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("settings.ini");
    let (mut saved, _) = scene();
    register(&mut saved);
    saved.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
    assert!(!optional_flip(&mut saved));
    assert!(optional_flip(&mut saved));
    let (mut app, w) = scene();
    register(&mut app);
    assert!(!optional_flip(&mut app));
    app.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
    crate::settings::apply_stored(app.world_mut());
    assert!(app.world().resource::<Settings>().enabled);
    app.world_mut().resource_mut::<Settings>().separate_confirm = false;
    app.update();
    assert!(app.world().resource::<crate::touch_ui_options::Retirements>().changed(crate::touch_ui_options::Feature::Terminal));
    assert!(!app.world().resource::<crate::touch_ui_options::Retirements>().barrier(crate::touch_ui_options::Feature::Terminal));
    tap(&mut app, w, at(&saved, 14));
    assert_eq!(hacking(&app).puzzle.state, GameState::Granted);
    assert_eq!(app.world().resource::<TerminalStats>().hacked, 1);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
