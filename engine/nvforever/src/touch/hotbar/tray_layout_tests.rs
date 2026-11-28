mod tray_layout_regressions {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    #[test]
    fn legacy_anchor_and_every_legacy_size_migrate_without_moving_fitted_slots_or_writing_disk() {
        for scale in [0.6,0.7,0.8,0.9,1.0,1.1,1.2,1.3,1.4,1.5,1.6,1.7,1.8] {
            let size=Vec2::new(1000.,600.);let mut world=World::new();
            world.insert_resource(super::super::super::Layout::default());
            let prefs=Preferences {scale,..default()};
            let legacy=Bar {placement:Placement {center:Vec2::new(0.3,0.7),..default()},safe_bottom:24.,size,..default()};
            let previous=geometry(size,&prefs,&legacy).unwrap();
            world.insert_resource(legacy);world.insert_resource(prefs);world.insert_resource(super::super::super::drawer::SafeBottom(24.));
            world.spawn((Window {resolution:bevy::window::WindowResolution::new(1000,600),..default()},bevy::window::PrimaryWindow));
            world.run_system_once(migrate_tray).unwrap();
            let config=&world.resource::<super::super::super::Layout>().config;
            assert!(!config.tray_explicit,"migration stays in memory until explicit layout Save");
            assert_eq!(config.tray.scale,scale);
            assert_eq!(config.tray.center,Vec2::new(0.3,0.7*(600.-24.-64.)/600.));
            let active=config.tray.clone();world.resource_mut::<Bar>().active_tray=Some(active);
            let migrated=geometry(size,world.resource::<Preferences>(),world.resource::<Bar>()).unwrap();
            assert_eq!(previous,migrated,"legacy preferred{scale} must keep exact fitted rectangles");
        }
    }
    #[test]
    fn stored_legacy_size_cannot_overwrite_explicit_profile_size_during_settings_application() {
        let dir=std::env::temp_dir().join(format!("nvf-tray-size-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&dir).unwrap();
        let ini=dir.join("settings.ini");let path=dir.join("touch-controls.json");
        std::fs::write(&ini,"[Settings]\ntouch-hotbar-size=1\n").unwrap();
        let mut app=App::new();install(&mut app);app.insert_resource(settings::SettingsFile(Some(ini.clone())));
        let mut layout=super::super::super::Layout {path:Some(path.clone()),..default()};layout.config.tray_explicit=true;layout.config.tray.scale=1.0;
        app.insert_resource(layout).insert_resource(super::super::super::tray_editor::Ready(false));
        settings::apply_stored(app.world_mut());
        assert_eq!(app.world().resource::<Preferences>().scale,1.8);
        assert_eq!(app.world().resource::<super::super::super::Layout>().config.tray.scale,1.0,"INI applies before authoritative profile scale getter");
        app.world_mut().resource_mut::<super::super::super::tray_editor::Ready>().0=true;
        let row=app.world().resource::<settings::Settings>().index("touch-hotbar-size").unwrap();
        let slider=app.world().resource::<settings::Settings>().list()[row].slider.unwrap();
        assert_eq!(slider.text((slider.get)(app.world())),"100%");
        let size = settings::choice_part(7, 13);
        assert_eq!(settings::set_slider(app.world_mut(), row, size), None);
        let hotbar = app.world().resource::<settings::Settings>().index("touch-hotbar").unwrap();
        assert_eq!(settings::flip(app.world_mut(), hotbar), Some(true));
        assert!(settings::set_slider(app.world_mut(), row, size).is_some());
        assert_eq!(app.world().resource::<super::super::super::Layout>().config.tray.scale,1.3);
        let saved:serde_json::Value=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["tray"]["scale"],serde_json::json!(1.3_f32));
        std::fs::remove_file(ini).unwrap();std::fs::remove_file(path).unwrap();std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn legacy_pin_writes_preserve_unknowns_and_refuse_future_schema_without_touching_reserved_slot() {
        let dir=std::env::temp_dir().join(format!("nvf-tray-pins-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&dir).unwrap();let path=dir.join("touch-hotbar.json");
        let original=serde_json::json!({"version":1,"center":[0.3,0.7],"pins":[false,true,false,false,false,false,false,false],"new_binding_metadata":{"retain":[1,2]}});
        std::fs::write(&path,serde_json::to_vec(&original).unwrap()).unwrap();let mut pins=Placement::decode(&original).unwrap();
        assert!(!pins.pins[AMMO_SWAP]);pins.pins[3]=true;pins.save_to(&path).unwrap();
        let saved:serde_json::Value=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["new_binding_metadata"],original["new_binding_metadata"]);assert_eq!(saved["pins"][AMMO_SWAP],false);assert_eq!(saved["pins"][3],true);
        let future=b"{\"version\":9,\"opaque\":true}";std::fs::write(&path,future).unwrap();assert!(pins.save_to(&path).is_err());assert_eq!(std::fs::read(&path).unwrap(),future);
        std::fs::remove_file(path).unwrap();std::fs::remove_dir(dir).unwrap();
    }
}
