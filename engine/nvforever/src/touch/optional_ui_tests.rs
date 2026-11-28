// Include inside touch.rs's existing tests module. This tests the real wrapper
// raw reader and generic-key output; core fixtures separately test actual effects.
fn optional_terminal_scene() -> (App, Entity) {
    let (mut app, window) = scene();
    app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Menu);
    app.init_resource::<nvf_play::terminal::touch::Settings>();
    app.world_mut().resource_mut::<nvf_play::terminal::touch::Settings>().enabled = false;
    app.insert_resource(nvf_play::terminal::TerminalScreen(Some(nvf_play::terminal::Session {
        reference: fnv_sim::FormId(1), base: fnv_sim::FormId(2),
        page: nvf_play::terminal::Page::LockedOut(["Authored fixture".into(), "No game files".into()]),
    })));
    nvf_play::terminal::touch::register(&mut app);
    app.update(); app.update();
    (app, window)
}
fn optional_terminal_row(app: &App) -> usize {
    app.world().resource::<nvf_play::settings::Settings>().index("touch-terminal-v1").unwrap()
}
fn optional_menu_point(app: &App, key: KeyCode) -> Vec2 {
    let i = BUTTONS.iter().position(|b| !b.playing && b.key == Some(key)).unwrap();
    app.world().resource::<layout::Layout>().config.center(layout::Target::Button(i), Vec2::splat(1000.0))
}

#[test]
fn registered_enhanced_terminal_off_restores_real_generic_touch_without_old_release() {
    let (mut app, window) = optional_terminal_scene();
    let row = optional_terminal_row(&app);
    let p = optional_menu_point(&app, KeyCode::Enter);
    touch(&mut app, window, 901, TouchPhase::Started, p.x, p.y);
    app.update();
    assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Enter));
    assert_eq!(nvf_play::settings::flip(app.world_mut(), row), Some(true));
    app.update();
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Enter));
    assert_eq!(nvf_play::settings::flip(app.world_mut(), row), Some(false));
    touch(&mut app, window, 901, TouchPhase::Ended, p.x, p.y);
    app.update();
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Enter));
    app.update();
    for (id, key) in [(902, KeyCode::ArrowRight), (903, KeyCode::Enter), (904, KeyCode::Tab), (905, KeyCode::Escape)] {
        let p = optional_menu_point(&app, key);
        touch(&mut app, window, id, TouchPhase::Started, p.x, p.y);
        app.update();
        assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(key), "fresh standard {key:?}");
        touch(&mut app, window, id, TouchPhase::Ended, p.x, p.y);
        app.update();
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(key));
    }
    raw_key(&mut app, window, KeyCode::KeyW, ButtonState::Pressed);
    app.update();
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW), "physical owner is unchanged");
}

#[test]
fn restored_terminal_off_still_accepts_real_generic_touch_cancel() {
    let dir = std::env::temp_dir().join(format!("nvf-wrapper-terminal-toggle-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("settings.ini");
    let (mut app, _) = optional_terminal_scene();
    app.insert_resource(nvf_play::settings::SettingsFile(Some(path.clone())));
    let row = optional_terminal_row(&app);
    assert_eq!(nvf_play::settings::flip(app.world_mut(), row), Some(true));
    assert_eq!(nvf_play::settings::flip(app.world_mut(), row), Some(false));
    let (mut restored, window) = optional_terminal_scene();
    restored.world_mut().resource_mut::<nvf_play::terminal::touch::Settings>().enabled = true;
    restored.insert_resource(nvf_play::settings::SettingsFile(Some(path.clone())));
    nvf_play::settings::apply_stored(restored.world_mut());
    restored.update(); restored.update();
    assert!(!restored.world().resource::<nvf_play::terminal::touch::Settings>().enabled);
    let p = optional_menu_point(&restored, KeyCode::Escape);
    touch(&mut restored, window, 910, TouchPhase::Started, p.x, p.y);
    restored.update();
    assert!(restored.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape));
    touch(&mut restored, window, 910, TouchPhase::Canceled, p.x, p.y);
    restored.update();
    assert!(!restored.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Escape));
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
