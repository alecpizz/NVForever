// Included inside the existing adapter tests: reuse its actual raw-input/rules App.
fn with_hint_setting(app: &mut App) -> usize {
    if app.world().get_resource::<settings::Settings>().is_none_or(|s| s.index(HINTS_KEY).is_none()) { register(app); }
    app.world().resource::<settings::Settings>().index(HINTS_KEY).unwrap()
}

#[test]
fn hints_off_keeps_enhanced_angle_and_torque_consumers_and_action_feedback() {
    let (mut app, window) = app();
    let hint = with_hint_setting(&mut app);
    assert_eq!(settings::flip(app.world_mut(), hint), Some(false));
    app.update();
    assert_eq!(app.world().resource::<Options>().scheme, Scheme::Arc);
    let original_turn = geometry().turn;
    touch(&mut app, window, 201, TouchPhase::Started, geometry().point(80.0)); app.update();
    assert!((game(&app).pin - 80.0).abs() < 0.01);
    touch(&mut app, window, 202, TouchPhase::Started, original_turn.center()); app.update();
    assert!(game(&app).torque && game(&app).turn > 0.0);
    let mut nodes = app.world_mut().query::<(&Visual, &Visibility, &Node, Option<&Text>)>();
    for (kind, visible, node, text) in nodes.iter(app.world()) {
        match kind {
            Visual::Guide => assert_eq!(*visible, Visibility::Hidden),
            Visual::Turn => {
                assert_eq!(*visible, Visibility::Inherited);
                assert_eq!((node.left, node.top, node.width, node.height),
                    (px(original_turn.min.x), px(original_turn.min.y), px(original_turn.width()), px(original_turn.height())));
            }
            Visual::TurnLabel => assert_eq!(text.unwrap().0, "Turning"),
            Visual::Value => assert!(text.unwrap().0.starts_with("Angle ")),
            _ => {}
        }
    }
    assert_eq!(settings::flip(app.world_mut(), hint), Some(true)); app.update();
    assert!(game(&app).torque, "changing explanatory visibility does not release an owned Turn");
    assert!(nodes.iter(app.world()).any(|(kind, visible, _, _)| matches!(kind, Visual::Guide) && *visible == Visibility::Inherited));
    touch(&mut app, window, 202, TouchPhase::Ended, original_turn.center()); app.update();
    assert!(!game(&app).torque);
}

#[test]
fn turning_hints_off_closes_existing_tutorial_through_actual_close_consumer_without_losing_status() {
    use bevy::ecs::system::RunSystemOnce;
    let (mut app, _) = app();
    let hint = with_hint_setting(&mut app);
    app.insert_resource(crate::sim::test_world::sim(Vec::new(), Vec::new()))
        .init_resource::<super::super::LockpickStats>()
        .add_message::<crate::ui_sounds::UiSound>().add_message::<super::super::LockOpened>();
    {
        let mut screen = app.world_mut().resource_mut::<LockpickScreen>();
        let s = screen.0.as_mut().unwrap();
        s.status = Some("Authored pin state feedback".into());
        s.help = Some(super::super::Help { title: "Authored explanation".into(), text: "Authored tutorial paragraph".into(),
            words: super::super::help::BoxWords { manual: "Guide".into(), close: "Close".into(), close_hint: "E".into() } });
    }
    let before = game(&app).clone();
    assert_eq!(settings::flip(app.world_mut(), hint), Some(false));
    assert!(app.world().resource::<LockpickScreen>().0.as_ref().unwrap().help.is_some(), "the setting queues the normal command, not a private effect");
    app.world_mut().run_system_once(super::super::apply).unwrap();
    let s = app.world().resource::<LockpickScreen>().0.as_ref().unwrap();
    assert!(s.help.is_none()); assert_eq!(s.game, before);
    assert_eq!(s.status.as_deref(), Some("Authored pin state feedback"));
    assert_eq!(app.world().resource::<super::super::LockpickStats>().picked, 0);
}

#[test]
fn hints_off_future_open_does_not_consume_first_tutorial_but_enabled_fresh_open_shows_it() {
    use bevy::ecs::system::RunSystemOnce;
    use fnv_esm::raw::{write_subrecords, Subrecord};
    fn record(sig: &[u8; 4], id: u32, subs: &[Subrecord]) -> Vec<u8> {
        let body = write_subrecords(subs);
        [&sig[..], &(body.len() as u32).to_le_bytes(), &0u32.to_le_bytes(), &id.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &body].concat()
    }
    let sub = |sig, data| Subrecord { sig, data, extended: false };
    let head = record(b"TES4", 0, &[sub(*b"HEDR", [1.34f32.to_le_bytes(), 1u32.to_le_bytes(), 0x800u32.to_le_bytes()].concat())]);
    let message = record(b"MESG", fnv_game::lockpick::HELP_MESSAGE.0, &[
        sub(*b"FULL", b"Authored lock tutorial\0".to_vec()), sub(*b"DESC", b"Authored tutorial paragraph\0".to_vec()),
    ]);
    let group = [&b"GRUP"[..], &((24 + message.len()) as u32).to_le_bytes(), &b"MESG"[..], &0u32.to_le_bytes(), &[0; 8], &message].concat();
    let plugin = fnv_esm::Plugin::parse(&[head, group].concat(), "AuthoredLock.esm".into()).unwrap();
    let script = crate::script::ScriptState::over_plugins(&[plugin]);
    let mut sim = crate::sim::test_world::sim(vec![crate::sim::test_world::item_base(fnv_game::lockpick::BOBBY_PIN.0)], Vec::new());
    sim.world.add_item(fnv_sim::FormId::PLAYER_REF, fnv_game::lockpick::BOBBY_PIN, 2).unwrap();
    let (mut app, _) = app();
    let hint = with_hint_setting(&mut app);
    app.add_plugins(bevy::state::app::StatesPlugin).insert_state(crate::PlayState::Playing)
        .insert_resource(crate::pause::Paused(false)).insert_resource(script).insert_resource(sim)
        .insert_resource(super::super::PracticeLock(Some(Level::VeryEasy)))
        .insert_resource(super::super::LockpickAid { steps: Vec::new(), skill: Some(100.0) })
        .init_resource::<super::super::LockpickStats>()
        .add_message::<super::super::OpenLockpick>().add_message::<crate::ui_sounds::UiSound>();
    app.world_mut().resource_mut::<LockpickScreen>().0 = None;
    assert_eq!(settings::flip(app.world_mut(), hint), Some(false));
    app.world_mut().run_system_once(super::super::open).unwrap();
    let s = app.world().resource::<LockpickScreen>().0.as_ref().unwrap();
    assert!(s.help.is_none()); assert!(active(app.world().resource::<LockpickScreen>(), app.world().resource::<Options>()));
    assert!(!app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.locks.help_shown);
    // A later normal opening still owns the first-use tutorial when re-enabled.
    app.world_mut().resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.close_menu();
    app.world_mut().resource_mut::<LockpickScreen>().0 = None;
    app.world_mut().resource_mut::<super::super::PracticeLock>().0 = Some(Level::VeryEasy);
    assert_eq!(settings::flip(app.world_mut(), hint), Some(true));
    app.world_mut().run_system_once(super::super::open).unwrap();
    assert_eq!(app.world().resource::<LockpickScreen>().0.as_ref().unwrap().help.as_ref().unwrap().text, "Authored tutorial paragraph");
    assert!(app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.locks.help_shown);
}

#[test]
fn saved_hint_switch_is_independent_of_enhanced_scheme_size_and_other_ini_sections() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!("nvf-lock-hints-{}-{}.ini", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&path, "[Other]\nkeep=yes\n").unwrap();
    let make = || {
        let mut app = App::new(); app.init_resource::<Options>(); register(&mut app);
        app.insert_resource(settings::SettingsFile(Some(path.clone()))); app
    };
    let mut app = make();
    let row = app.world().resource::<settings::Settings>().index(HINTS_KEY).unwrap();
    let scheme = app.world().resource::<settings::Settings>().index(SCHEME_KEY).unwrap();
    assert!(settings::pick(app.world_mut(), scheme, "Precision slider + turn"));
    assert_eq!(settings::flip(app.world_mut(), row), Some(false));
    let mut loaded = make(); settings::apply_stored(loaded.world_mut());
    assert!(!loaded.world().resource::<Hints>().0);
    assert_eq!(loaded.world().resource::<Options>().scheme, Scheme::Slider);
    assert!(std::fs::read_to_string(&path).unwrap().contains("keep=yes"));
    assert_eq!(settings::flip(app.world_mut(), row), Some(true));
    let mut loaded = make(); settings::apply_stored(loaded.world_mut());
    assert!(loaded.world().resource::<Hints>().0);
    assert_eq!(loaded.world().resource::<Options>().scheme, Scheme::Slider);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn live_user_palette_recolours_actual_control_text_strokes_and_pressed_surface_without_changing_hits() {
    // The production palette is process-wide. Re-execute this exact shared-binary
    // selector alone, so concurrent tests never observe our chosen colours.
    const CHILD: &str = "NVF_TEST_LOCKPICK_PALETTE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "lockpick::touch::tests::live_user_palette_recolours_actual_control_text_strokes_and_pressed_surface_without_changing_hits", "--nocapture"])
            .env(CHILD, "1").spawn().unwrap();
        let started = std::time::Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < std::time::Duration::from_secs(30) =>
                    std::thread::sleep(std::time::Duration::from_millis(10)),
                Ok(None) => {
                    let killed = child.kill();
                    let reaped = child.wait();
                    panic!("isolated palette control exceeded 30s; own-child kill={killed:?}, reap={reaped:?}");
                }
                Err(error) => {
                    let killed = child.kill();
                    let reaped = child.wait();
                    panic!("isolated palette wait failed: {error}; own-child kill={killed:?}, reap={reaped:?}");
                }
            }
        };
        assert!(status.success(), "isolated actual palette control failed"); return;
    }
    let (mut app, window) = app();
    app.add_plugins(crate::ui_colour::UiColourPlugin);
    let row = app.world().resource::<settings::Settings>().index("hud-colour").unwrap();
    app.update();
    for name in ["Green", "Blue"] {
        assert!(settings::pick(app.world_mut(), row, name)); app.update();
        let ink = crate::interface::amber(255.0);
        let mut nodes = app.world_mut().query::<(&Visual, &Node, Option<&TextColor>, Option<&BorderColor>, Option<&BackgroundColor>)>();
        for (kind, node, text, border, background) in nodes.iter(app.world()) {
            if let Some(text) = text { assert_eq!(text.0, ink); }
            if matches!(kind, Visual::Turn | Visual::Angle) {
                assert_eq!(*border.unwrap(), BorderColor::all(ink.with_alpha(0.65)));
                assert_eq!(node.border, UiRect::all(px(geometry().scale)));
            }
            if matches!(kind, Visual::Dot(_) | Visual::Needle) { assert_eq!(background.unwrap().0, ink.with_alpha(0.95)); }
        }
        touch(&mut app, window, 301, TouchPhase::Started, geometry().turn.center()); app.update();
        assert!(game(&app).torque);
        let world = app.world_mut(); let mut visuals = world.query::<(&Visual, &BackgroundColor)>();
        assert!(visuals.iter(world).any(|(kind, background)| matches!(kind, Visual::Turn) && background.0 == ink.with_alpha(0.22)));
        touch(&mut app, window, 301, TouchPhase::Ended, geometry().turn.center()); app.update();
        assert!(!game(&app).torque);
    }
    // Freshly painted HUD greys are also a brightness of the old grey. A generic
    // recolour after the authoritative draw would apply that change twice.
    for part in [0.8, 0.4] {
        for key in ["hud-red", "hud-green", "hud-blue"] {
            let row=app.world().resource::<settings::Settings>().index(key).unwrap();
            assert!(settings::set_slider(app.world_mut(),row,part).is_some());
        }
        let g=geometry();
        let mut visuals=app.world_mut().query_filtered::<Entity,With<Visual>>();
        let ids=visuals.iter(app.world()).collect::<Vec<_>>();
        app.update();
        let ink=crate::interface::amber(255.0);
        let mut visuals=app.world_mut().query_filtered::<Entity,With<Visual>>();
        assert_eq!(visuals.iter(app.world()).collect::<Vec<_>>(),ids,"palette change never rebuilds touch targets");
        let mut visuals=app.world_mut().query::<(&Visual,&Node,Option<&TextColor>,Option<&BorderColor>)>();
        let mut painted=0;
        for (kind,node,text,border) in visuals.iter(app.world()) {
            if let Some(text)=text {assert_eq!(text.0,ink,"actual final-frame text uses current palette exactly once");painted+=1;}
            if matches!(kind,Visual::Turn|Visual::Angle) {
                assert_eq!(*border.unwrap(),BorderColor::all(ink.with_alpha(0.65)));
                assert_eq!(node.border,UiRect::all(px(g.scale)));
            }
        }
        assert!(painted>0);
        touch(&mut app,window,302,TouchPhase::Started,g.turn.center());app.update();
        assert!(game(&app).torque,"palette does not change actual held-turn input");
        touch(&mut app,window,302,TouchPhase::Ended,g.turn.center());app.update();
        assert!(!game(&app).torque);
    }
}
