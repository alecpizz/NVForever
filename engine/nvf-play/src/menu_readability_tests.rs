use super::*;

#[test]
fn every_menu_size_is_independent_finite_and_reaches_both_ends() {
    let mut sizes = MenuReadability::default();
    assert_eq!(sizes.size(MenuKind::Barter), 1.0);
    assert_eq!(MenuReadability::for_platform(true), MenuReadability { barter: 1.50, repair: 1.50, workbench: 1.50 });
    assert_eq!(MenuReadability::for_platform(false), MenuReadability { barter: 1.0, repair: 1.0, workbench: 1.0 });
    sizes.barter = 2.0;
    assert_eq!(sizes.size(MenuKind::Barter), 2.0);
    assert_eq!(sizes.size(MenuKind::Repair), 1.0);
    assert_eq!(sizes.size(MenuKind::Workbench), 1.0);
    assert_eq!((fit(-1.0), fit(9.0), fit(f32::NAN)), (0.5, 2.0, 1.0));
    for i in 0..31 {
        let value = settings::choice_part(i, 31);
        assert!((of_part(value) - (0.5 + i as f32 * 0.05)).abs() < 0.001);
        assert!((part(of_part(value)) - value).abs() < 0.001);
    }
}

#[test]
fn independent_sizes_use_actual_settings_write_reload_and_keep_other_settings() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("nvf-menu-readability-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    struct Files(std::path::PathBuf);
    impl Drop for Files {
        fn drop(&mut self) {
            for file in ["NVForever.ini", "NVForever.ini.bak", "NVForever.ini.tmp"] {
                let path = self.0.join(file);
                if path.is_file() { std::fs::remove_file(path).unwrap(); }
            }
            std::fs::remove_dir(&self.0).unwrap();
        }
    }
    let _files = Files(dir.clone());
    let path = dir.join("NVForever.ini");
    std::fs::write(&path, "[Settings]\nmenu-scale=0.3\n[Other]\nkeep=yes\n").unwrap();
    let start = || {
        let mut app = App::new();
        app.insert_resource(settings::SettingsFile(Some(path.clone())));
        register(&mut app, "Display");
        app.add_systems(PostStartup, settings::apply_stored);
        app.update();
        app
    };
    let mut app = start();
    for (key, value) in [("barter-menu-size-v1", 1.5), ("repair-menu-size-v1", 0.5), ("workbench-menu-size-v1", 1.25)] {
        let index = app.world().resource::<settings::Settings>().index(key).unwrap();
        settings::set_slider(app.world_mut(), index, part(value));
    }
    let restored = start();
    assert_eq!(*restored.world().resource::<MenuReadability>(), MenuReadability { barter: 1.5, repair: 0.5, workbench: 1.25 });
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_eq!(fnv_install::ini_get(&saved, "Settings", "menu-scale").as_deref(), Some("0.3"));
    assert_eq!(fnv_install::ini_get(&saved, "Other", "keep").as_deref(), Some("yes"));
}
