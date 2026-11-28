#[test]
fn registered_dialogue_off_retires_old_answer_and_preserves_independent_text_preferences() {
    let (mut app, window, _) = scene(false);
    let index = app.world().resource::<crate::settings::Settings>().index("touch-dialogue").unwrap();
    let before = *app.world().resource::<TouchDialogue>();
    event(&mut app, window, 811, TouchPhase::Started, 200.0, 100.0);
    app.update();
    assert_eq!(crate::settings::flip(app.world_mut(), index), Some(false));
    let retired = app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    assert!(retired.changed(crate::touch_ui_options::Feature::Dialogue));
    assert_eq!(retired.contacts().collect::<Vec<_>>(), [(window, 811)]);
    event(&mut app, window, 811, TouchPhase::Ended, 200.0, 100.0);
    app.update();
    assert_eq!(asked(&app).pick, None);
    assert!(!app.world().resource::<TouchFrame>().active);
    assert_eq!(app.world().resource::<TouchDialogue>().choice_scale, before.choice_scale);
    assert_eq!(app.world().resource::<TouchDialogue>().subtitle_scale, before.subtitle_scale);
    assert_eq!(app.world().resource::<TouchDialogue>().confirm, before.confirm);
    tap(&mut app, window, 812, 100.0);
    assert_eq!(asked(&app).pick, None, "disabled enhanced overlay cannot answer");
    assert_eq!(crate::settings::flip(app.world_mut(), index), Some(true));
    app.update();
    app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    app.update(); // Consume the reopened overlay identity before starting a contact.
    tap(&mut app, window, 813, 100.0);
    assert_eq!(asked(&app).pick, Some(2), "fresh returned contact reaches real offered-index consumer");
}

#[test]
fn late_dialogue_off_on_cannot_answer_from_an_admitted_old_raw_batch() {
    #[derive(Resource)] struct Change(bool);
    fn late(world: &mut World) {
        if !std::mem::take(&mut world.resource_mut::<Change>().0) { return; }
        assert!(world.resource::<bevy::input::touch::Touches>().any_just_released());
        let row = world.resource::<crate::settings::Settings>().index("touch-dialogue").unwrap();
        assert_eq!(crate::settings::flip(world, row), Some(false));
        assert_eq!(crate::settings::flip(world, row), Some(true));
    }
    let (mut app, window, _) = scene(false);
    app.insert_resource(Change(true)).add_systems(Update, late.before(read));
    event(&mut app, window, 995, TouchPhase::Started, 200., 100.);
    event(&mut app, window, 995, TouchPhase::Ended, 200., 100.);
    app.update();
    assert_eq!(asked(&app).pick, None);
    assert!(app.world().resource::<Gestures>().gesture.is_none());
    app.world_mut().resource_mut::<crate::touch_ui_options::Retirements>().take();
    app.update();
    assert_eq!(asked(&app).pick, None);
    tap(&mut app, window, 996, 100.);
    assert_eq!(asked(&app).pick, Some(2));
}

#[test]
fn standalone_dialogue_off_on_expires_only_leaf_barrier_before_a_fresh_answer() {
    let (mut app, window, _) = scene(false);
    let row = app.world().resource::<crate::settings::Settings>().index("touch-dialogue").unwrap();
    event(&mut app, window, 1105, TouchPhase::Started, 200., 100.);
    app.update();
    assert_eq!(crate::settings::flip(app.world_mut(), row), Some(false));
    assert_eq!(crate::settings::flip(app.world_mut(), row), Some(true));
    event(&mut app, window, 1105, TouchPhase::Ended, 200., 100.);
    app.update();
    assert_eq!(asked(&app).pick, None);
    let pending = app.world().resource::<crate::touch_ui_options::Retirements>();
    assert!(pending.changed(crate::touch_ui_options::Feature::Dialogue));
    assert!(!pending.barrier(crate::touch_ui_options::Feature::Dialogue));
    assert_eq!(pending.contacts().collect::<Vec<_>>(), [(window, 1105)]);
    tap(&mut app, window, 1106, 100.);
    assert_eq!(asked(&app).pick, Some(2));
}

#[test]
fn standalone_restored_dialogue_on_needs_no_wrapper_drain_to_answer() {
    let path = std::env::temp_dir().join(format!("nvf-dialogue-on-{}-{}.ini", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let (mut saved, _, _) = scene(false);
    saved.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
    let row = saved.world().resource::<crate::settings::Settings>().index("touch-dialogue").unwrap();
    assert_eq!(crate::settings::flip(saved.world_mut(), row), Some(false));
    assert_eq!(crate::settings::flip(saved.world_mut(), row), Some(true));
    let (mut app, window, _) = scene(false);
    let row = app.world().resource::<crate::settings::Settings>().index("touch-dialogue").unwrap();
    assert_eq!(crate::settings::flip(app.world_mut(), row), Some(false));
    app.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
    crate::settings::apply_stored(app.world_mut());
    assert!(app.world().resource::<TouchDialogue>().enabled);
    app.update();
    assert!(app.world().resource::<crate::touch_ui_options::Retirements>().changed(crate::touch_ui_options::Feature::Dialogue));
    assert!(!app.world().resource::<crate::touch_ui_options::Retirements>().barrier(crate::touch_ui_options::Feature::Dialogue));
    tap(&mut app, window, 1107, 100.);
    assert_eq!(asked(&app).pick, Some(2));
    std::fs::remove_file(path).unwrap();
}
