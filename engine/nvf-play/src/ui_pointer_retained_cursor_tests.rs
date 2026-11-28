// Same fixture bytes apply to H571 and the candidate. Outcomes remain UNRUN.
fn cursor_touch(app: &mut App, window: Entity, id: u64, phase: bevy::input::touch::TouchPhase, position: Vec2) {
    app.world_mut().write_message(bevy::input::touch::TouchInput {
        window, id, phase, position, force: std::option::Option::None,
    });
}
fn cursor_mouse(app: &mut App, window: Entity, state: bevy::input::ButtonState) {
    app.world_mut().write_message(bevy::input::mouse::MouseButtonInput {
        window, button: MouseButton::Left, state,
    });
}

#[test]
fn retained_cursor_mouse_hover_hold_and_release_keep_existing_priority_with_touch() {
    use bevy::input::{ButtonState, touch::TouchPhase};
    let (mut app, window, button) = touch_scene();
    app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(Vec2::splat(40.0)));
    app.update();
    assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Hovered);
    cursor_mouse(&mut app, window, ButtonState::Pressed);
    cursor_touch(&mut app, window, 51, TouchPhase::Started, Vec2::splat(300.0));
    app.update();
    assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Pressed,
        "an admitted physical mouse press wins over a simultaneous touch elsewhere");
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    app.update();
    assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Pressed,
        "a held mouse remains the pointer owner");
    cursor_mouse(&mut app, window, ButtonState::Released);
    app.update();
    assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Hovered,
        "mouse release uses the mouse position, not the held touch elsewhere");
    assert!(app.world().resource::<ButtonInput<MouseButton>>().just_released(MouseButton::Left));
    cursor_touch(&mut app, window, 51, TouchPhase::Ended, Vec2::splat(300.0));
    app.update();
    app.update();
    assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Hovered,
        "ordinary mouse hover resumes after the touch terminal frame");
}

#[test]
fn retained_cursor_after_focus_and_cover_cannot_redirect_fresh_real_taffy_touch() {
    use bevy::input::touch::TouchPhase;
    use bevy::window::{AppLifecycle, WindowFocused};
    let (mut app, window, button) = touch_scene();
    app.add_message::<WindowFocused>().add_message::<AppLifecycle>()
        .init_resource::<crate::pad::PlatformInputGate>();
    let retained = Vec2::splat(300.0);
    app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(retained));
    app.update();
    assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::None);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.world_mut().write_message(WindowFocused { window, focused: false });
    app.world_mut().write_message(AppLifecycle::Suspended);
    app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true);
    cursor_touch(&mut app, window, 61, TouchPhase::Started, Vec2::splat(40.0));
    app.update();
    assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::None,
        "the actual pointer reader remains blocked by the platform cover");
    assert!(app.world().resource::<Touches>().get_pressed(61).is_some(), "hardware contact is intact");
    cursor_touch(&mut app, window, 61, TouchPhase::Canceled, Vec2::splat(40.0));
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.world_mut().write_message(WindowFocused { window, focused: true });
    app.world_mut().write_message(AppLifecycle::Running);
    app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(false);
    app.update();
    assert!(app.world().resource::<Touches>().iter().next().is_none());
    assert!(app.world().get::<Window>(window).unwrap().cursor_position().unwrap().abs_diff_eq(retained, 0.01));
    cursor_touch(&mut app, window, 62, TouchPhase::Started, Vec2::splat(40.0));
    app.update();
    assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Pressed,
        "fresh touch hits the measured button despite the retained mouse cursor");
    cursor_touch(&mut app, window, 62, TouchPhase::Ended, Vec2::splat(40.0));
    app.update();
    assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Hovered,
        "ordinary release stays at the touch position for its terminal frame");
    app.update();
    assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::None);
}

#[test]
fn retained_cursor_touch_terminal_and_same_batch_edges_keep_admission_and_timing() {
    use bevy::input::touch::TouchPhase;
    for same_batch in [false, true] {
        let (mut app, window, button) = touch_scene();
        app.init_resource::<TouchUiCapture>();
        app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(Vec2::splat(300.0)));
        app.world_mut().resource_mut::<TouchUiCapture>().block(71);
        cursor_touch(&mut app, window, 71, TouchPhase::Started, Vec2::splat(40.0));
        app.update();
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::None,
            "a sheet-owned contact cannot become the selected touch position");
        cursor_touch(&mut app, window, 71, TouchPhase::Ended, Vec2::splat(40.0));
        app.update();
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::None);
        app.world_mut().resource_mut::<TouchUiCapture>().begin_frame();
        cursor_touch(&mut app, window, 72, TouchPhase::Started, Vec2::splat(40.0));
        if same_batch { cursor_touch(&mut app, window, 72, TouchPhase::Ended, Vec2::splat(40.0)); }
        app.update();
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Pressed,
            "same-batch and ordinary touches still act on press");
        if same_batch {
            assert!(app.world().resource::<Touches>().just_released(72));
        } else {
            cursor_touch(&mut app, window, 72, TouchPhase::Ended, Vec2::splat(40.0));
            app.update();
            assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Hovered);
        }
        app.update();
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::None,
            "terminal touch does not leave a pressed node behind");
        assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    }
}
