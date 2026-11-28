// Actual registered action through the sole raw reader and existing editor.
fn layout_settings(app: &mut App) -> usize {
    use nvf_play::pause::{Page, Paused};
    app.insert_resource(Paused(true)).insert_resource(Page::Settings { selected: None });
    app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Menu);
    app.update(); app.update();
    app.world().resource::<nvf_play::settings::Settings>().index(entrypoints::KEY).unwrap()
}
fn customize(app: &mut App, row: usize) {
    assert!(nvf_play::settings::pick(app.world_mut(), row, "Customize layout"));
    app.update(); app.update();
    assert!(app.world().resource::<Layout>().editor.is_some());
    assert!(app.world().resource::<nvf_play::pause::FormOver>().0);
}
fn move_editor_stick(app: &mut App, window: Entity) {
    let from = app.world().resource::<Layout>().config.center(Target::Stick, Vec2::splat(1000.));
    touch(app, window, 830, TouchPhase::Started, from.x, from.y); app.update();
    touch(app, window, 830, TouchPhase::Moved, from.x + 60., from.y + 40.); app.update();
    touch(app, window, 830, TouchPhase::Ended, from.x + 60., from.y + 40.); app.update();
}
fn editor_action(app: &mut App, window: Entity, index: usize) {
    let point = layout::editor_rect(index, Vec2::splat(1000.)).center();
    touch(app, window, 831, TouchPhase::Started, point.x, point.y);
    touch(app, window, 831, TouchPhase::Ended, point.x, point.y);
    // A later edge in the same closing batch must not reach Settings or gameplay.
    touch(app, window, 832, TouchPhase::Started, 920., 900.);
    app.update();
}
#[test]
fn mobile_settings_editor_cancels_actual_drag_and_drains_the_closing_batch() {
    let (mut app, window) = controls_scene();
    let row = layout_settings(&mut app);
    let original = app.world().resource::<Layout>().config.clone();
    assert!(nvf_play::settings::pick(app.world_mut(), row, "Customize layout"));
    touch(&mut app, window, 829, TouchPhase::Started, 920., 900.);
    app.update(); app.update();
    assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    move_editor_stick(&mut app, window);
    assert_ne!(app.world().resource::<Layout>().config, original);
    editor_action(&mut app, window, 10);
    assert!(app.world().resource::<Layout>().editor.is_none());
    assert_eq!(app.world().resource::<Layout>().config, original);
    assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(832));
    assert!(app.world().resource::<nvf_play::pause::FormOver>().0);
    touch(&mut app, window, 832, TouchPhase::Ended, 920., 900.);
    app.update();
    assert!(app.world().resource::<nvf_play::pause::FormOver>().0);
    for _ in 0..3 { app.update(); }
    assert!(!app.world().resource::<nvf_play::pause::FormOver>().0);
    assert!(app.world().resource::<nvf_play::pause::Paused>().0);
    assert!(matches!(app.world().resource::<nvf_play::pause::Page>(), nvf_play::pause::Page::Settings { .. }));
    assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    customize(&mut app, row);
}
#[test]
fn mobile_settings_editor_save_uses_the_existing_layout_file_and_returns_paused() {
    let (mut app, window) = controls_scene();
    let row = layout_settings(&mut app);
    let path = std::env::temp_dir().join(format!("nvf-layout-entry-{}.json", uuid::Uuid::new_v4()));
    app.world_mut().resource_mut::<Layout>().path = Some(path.clone());
    customize(&mut app, row);
    move_editor_stick(&mut app, window);
    let expected = app.world().resource::<Layout>().config.encode();
    editor_action(&mut app, window, 9);
    let actual: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(layout::Config::decode(&actual).unwrap().encode(), expected);
    assert!(app.world().resource::<Layout>().editor.is_none());
    for _ in 0..4 { app.update(); }
    assert!(app.world().resource::<nvf_play::pause::Paused>().0);
    assert!(matches!(app.world().resource::<nvf_play::pause::Page>(), nvf_play::pause::Page::Settings { .. }));
    std::fs::remove_file(path).unwrap();
}
#[test]
fn mobile_settings_editor_rejects_gameplay_and_small_windows_without_taking_the_form() {
    let (mut app, window) = controls_scene();
    let row = app.world().resource::<nvf_play::settings::Settings>().index(entrypoints::KEY).unwrap();
    assert!(!nvf_play::settings::pick(app.world_mut(), row, "Customize layout"));
    assert!(app.world().resource::<Layout>().editor.is_none());
    assert!(!app.world().resource::<nvf_play::pause::FormOver>().0);
    let row = layout_settings(&mut app);
    app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(320., 240.);
    app.update();
    assert!(!nvf_play::settings::pick(app.world_mut(), row, "Customize layout"));
    assert!(app.world().resource::<Layout>().editor.is_none());
    assert!(!app.world().resource::<nvf_play::pause::FormOver>().0);
}
#[test]
fn mobile_settings_editor_escape_and_lost_focus_restore_the_draft() {
    for escape in [true, false] {
        let (mut app, window) = controls_scene();
        let row = layout_settings(&mut app);
        let original = app.world().resource::<Layout>().config.clone();
        customize(&mut app, row);
        move_editor_stick(&mut app, window);
        if escape { raw_key(&mut app, window, KeyCode::Escape, ButtonState::Pressed); }
        else { app.world_mut().write_message(WindowFocused { window, focused: false }); }
        app.update();
        assert!(app.world().resource::<Layout>().editor.is_none());
        assert_eq!(app.world().resource::<Layout>().config, original);
        assert!(app.world().resource::<nvf_play::pause::FormOver>().0);
        assert!(app.world().resource::<nvf_play::pause::Paused>().0);
    }
}
#[test]
fn customize_touch_layout_suppresses_android_soft_keyboard_even_on_taps() {
    let (mut app, window) = controls_scene();
    app.insert_resource(nvf_play::save::SaveConfig::with_dir(None))
        .init_resource::<nvf_play::pad::PadGlyphs>()
        .add_message::<nvf_play::save::SaveRequest>()
        .add_message::<nvf_play::save::SaveDone>()
        .add_message::<nvf_play::save::LoadRequest>()
        .add_message::<nvf_play::ui_sounds::UiSound>()
        .add_plugins((nvf_play::pause::PausePlugin, nvf_play::soft_keyboard::SoftKeyboardPlugin));
    app.insert_resource(nvf_play::soft_keyboard::SoftKeyboard::for_platform(true));
    let row = layout_settings(&mut app);
    customize(&mut app, row);
    // Tap inside the layout editor: soft keyboard must stay suppressed.
    move_editor_stick(&mut app, window);
    app.update();
    assert!(!app.world().get::<Window>(window).unwrap().ime_enabled,
        "Customize Touch Layout must not open Android soft keyboard");
}
#[test]
fn removed_hud_shortcuts_never_reappear_on_startup_restore_or_controller_return() {
    use nvf_play::bindings::InputDevice;
    let (mut app, window) = controls_scene();
    for device in [InputDevice::KeyboardMouse, InputDevice::Controller, InputDevice::KeyboardMouse] {
        nvf_play::bindings::set_device(app.world_mut(), device);
        app.update();
        let hints = app.world().resource::<Layout>().config.hints;
        for help in [false, true] {
            let point = layout::utility_rect(help, Vec2::splat(1000.)).center();
            touch(&mut app, window, 840, TouchPhase::Started, point.x, point.y);
            touch(&mut app, window, 840, TouchPhase::Ended, point.x, point.y); app.update();
            assert!(app.world().resource::<Layout>().editor.is_none());
            assert_eq!(app.world().resource::<Layout>().config.hints, hints);
        }
        let world = app.world_mut();
        let mut icons = world.query::<&icons::TouchIcon>();
        assert!(icons.iter(world).all(|icon| Some(icon.0) != super::icons::semantic_index("settings")
            && Some(icon.0) != super::icons::semantic_index("help")));
    }
}
