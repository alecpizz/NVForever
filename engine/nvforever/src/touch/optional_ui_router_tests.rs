// Root claim-before-leaf and actual InputPlugin neutral recovery controls.
#[test]
fn feature_switch_retires_the_sole_readers_claim_before_any_leaf_replay() {
    let (mut app, window) = optional_terminal_scene();
    let row = optional_terminal_row(&app);
    assert_eq!(nvf_play::settings::flip(app.world_mut(), row), Some(true));
    app.update(); app.update();
    let point = optional_menu_point(&app, KeyCode::Enter);
    touch(&mut app, window, 920, TouchPhase::Started, point.x, point.y); app.update();
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Enter));
    // This fixture has the real Settings setter but no terminal leaf reader:
    // only the parent's actual raw admission can know this contact.
    assert_eq!(nvf_play::settings::flip(app.world_mut(), row), Some(false));
    let foreign = app.world_mut().spawn(Window::default()).id();
    touch(&mut app, foreign, 921, TouchPhase::Started, point.x, point.y);
    raw_key(&mut app, window, KeyCode::KeyW, ButtonState::Pressed);
    app.update();
    let mask = app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>();
    assert!(!mask.allows(920));
    assert!(mask.allows(921), "foreign-window input is not a feature-owned contact");
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
    touch(&mut app, window, 920, TouchPhase::Ended, point.x, point.y); app.update();
    assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(920), "release remains masked through UI focus");
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Enter));
    touch(&mut app, foreign, 921, TouchPhase::Canceled, point.x, point.y);
    app.update(); app.update();
    touch(&mut app, window, 922, TouchPhase::Started, point.x, point.y); app.update();
    assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Enter));
}
#[test]
fn feature_switch_bounds_parent_claims_and_recovers_on_real_input_neutral() {
    let (mut app, window) = optional_terminal_scene();
    let row = optional_terminal_row(&app);
    assert_eq!(nvf_play::settings::flip(app.world_mut(), row), Some(true)); app.update();
    let point = optional_menu_point(&app, KeyCode::Enter);
    for id in 950..983 { touch(&mut app, window, id, TouchPhase::Started, point.x, point.y); }
    app.update();
    assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(999), "claim overflow fails closed");
    assert_eq!(nvf_play::settings::flip(app.world_mut(), row), Some(false));
    for id in 950..983 { touch(&mut app, window, id, TouchPhase::Canceled, point.x, point.y); }
    app.update();
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Enter));
    app.update(); app.update();
    assert!(app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(999));
    touch(&mut app, window, 999, TouchPhase::Started, point.x, point.y); app.update();
    assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Enter));
}
