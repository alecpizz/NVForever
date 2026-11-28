use super::{tests::{activate, delta, scene}, *};

#[test]
fn independent_axis_signs_apply_once_after_actual_ads_gain() {
    for x in [false, true] {
        for y in [false, true] {
            for aimed in [false, true] {
                let (mut app, window, point) = scene(true);
                {
                    let mut options = app.world_mut().resource_mut::<Options>();
                    options.invert_x = x;
                    options.invert_y = y;
                }
                app.world_mut().resource_mut::<nvf_play::aim_zoom::AimZoom>().aimed = aimed;
                app.update(); // A preference change cancels the prior sensor reference.
                activate(&mut app, window, point, true);
                let gain = if aimed { 0.5 } else { 1.0 };
                assert_eq!(delta(&app), Vec2::new(if x { -0.02 } else { 0.02 }, if y { 0.01 } else { -0.01 }) * gain);
                assert_eq!(app.world().resource::<bevy::input::mouse::AccumulatedMouseMotion>().delta, Vec2::ZERO);
                assert!(app.world().resource::<ButtonInput<MouseButton>>().get_pressed().next().is_none());
                assert!(app.world().resource::<ButtonInput<KeyCode>>().get_pressed().next().is_none());
            }
        }
    }
}

#[test]
fn changing_sign_or_recentering_cancels_pending_motion_and_requires_fresh_activation() {
    let (mut app, window, point) = scene(true);
    activate(&mut app, window, point, true);
    assert_eq!(delta(&app), Vec2::new(0.02, -0.01));
    app.world_mut().resource_mut::<Options>().invert_y = true;
    app.update();
    assert_eq!(delta(&app), Vec2::ZERO);
    app.update();
    assert_eq!(delta(&app), Vec2::ZERO);
    activate(&mut app, window, point, true);
    assert_eq!(delta(&app), Vec2::new(0.02, 0.01));
    let index = app.world().resource::<settings::Settings>().index(KEYS[5]).unwrap();
    assert!(settings::pick(app.world_mut(), index, "Recenter now"));
    assert_eq!(delta(&app), Vec2::ZERO);
    app.update();
    app.update();
    assert_eq!(delta(&app), Vec2::ZERO);
    activate(&mut app, window, point, true);
    assert_eq!(delta(&app), Vec2::new(0.02, 0.01));
    assert!(app.world().resource::<Options>().invert_y);
}

#[test]
fn both_inversion_preferences_default_off_and_round_trip_independently_through_ini() {
    for x in [false, true] {
        for y in [false, true] {
            let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("nvf-gyro-signs-{}-{nonce}.ini", std::process::id()));
            let mut app = App::new();
            install(&mut app);
            assert!(!app.world().resource::<Options>().invert_x);
            assert!(!app.world().resource::<Options>().invert_y);
            app.insert_resource(settings::SettingsFile(Some(path.clone())));
            for key in [KEYS[6], KEYS[7]] {
                let index = app.world().resource::<settings::Settings>().index(key).unwrap();
                assert_eq!(settings::flip(app.world_mut(), index), None);
            }
            // Enabling Gyro admits its child rows and writes the INI even for false/false.
            let index = app.world().resource::<settings::Settings>().index(KEYS[0]).unwrap();
            assert_eq!(settings::flip(app.world_mut(), index), Some(true));
            for (key, value) in [(KEYS[6], x), (KEYS[7], y)] {
                let index = app.world().resource::<settings::Settings>().index(key).unwrap();
                if value { assert_eq!(settings::flip(app.world_mut(), index), Some(true)); }
            }
            let stored = std::fs::read_to_string(&path).unwrap();
            assert!(stored.contains(&format!("{}={}", KEYS[6], if x { 1 } else { 0 })));
            assert!(stored.contains(&format!("{}={}", KEYS[7], if y { 1 } else { 0 })));
            let mut loaded = App::new();
            install(&mut loaded);
            loaded.insert_resource(settings::SettingsFile(Some(path.clone())));
            settings::apply_stored(loaded.world_mut());
            let options = loaded.world().resource::<Options>();
            assert_eq!((options.invert_x, options.invert_y), (x, y));
            assert_eq!((options.sensitivity, options.ads, options.recenter), (1.0, 0.5, 0));
            std::fs::remove_file(path).unwrap();
        }
    }
}
