// Joint integration control: include beside normal_tests.rs only after the
// Root-owned menu_readability module and real setter epoch callback are composed.
// Source-only / UNCOMPILED / UNRUN. This does not prove non-default row reflow.
#[test]
fn normal_barter_registered_size_roundtrip_cannot_revive_a_held_contact() {
    for native in [false,true] {
        let (mut app,_)=normal_scene(native);
        app.insert_resource(crate::settings::SettingsFile(None));
        crate::menu_readability::register(&mut app,crate::pause::GROUP_MENUS);
        let row=app.world().resource::<crate::settings::Settings>().index("barter-menu-size-v1").unwrap();
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        edge(&mut app,209,TouchPhase::Started,p);app.update();
        let p_160 = crate::settings::choice_part(22, 31);
        let p_100 = crate::settings::choice_part(10, 31);
        assert_eq!(crate::settings::set_slider(app.world_mut(),row,p_160),Some(p_160));
        assert_eq!(crate::settings::set_slider(app.world_mut(),row,p_100),Some(p_100));
        assert_eq!(app.world().resource::<crate::menu_readability::MenuReadability>()
            .size(crate::menu_readability::MenuKind::Barter),1.0);
        edge(&mut app,209,TouchPhase::Ended,p);app.update();
        assert!(model(&app).quantity.is_none(),"actual setting callbacks cancel, even when final size returns to its old value");
        assert_eq!(offered(&app),0);
        app.update();app.update();
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        normal_tap_at(&mut app,p);
        assert!(model(&app).quantity.is_some(),"a fresh real normal-menu contact remains usable after actual neutral");
        normal_command(&mut app,Command::AmountCancel);
        assert!(model(&app).quantity.is_none());
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        edge(&mut app,212,TouchPhase::Started,p);app.update();
        assert_eq!(crate::settings::set_slider(app.world_mut(),row,p_100),Some(p_100));
        edge(&mut app,212,TouchPhase::Ended,p);app.update();app.update();
        assert!(model(&app).quantity.is_some(),"setting the same quantized size must preserve the held proposal; Settings may skip its unchanged callback");
        assert_eq!(offered(&app),0);
        normal_command(&mut app,Command::AmountCancel);
        assert!(model(&app).quantity.is_none(),"one normal prompt was opened and can be canceled normally");
    }
}
