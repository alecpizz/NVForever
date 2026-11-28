use super::*;
use crate::settings;

#[test]
fn expanded_hud_choices_keep_five_percent_steps_and_finite_default() {
    assert_eq!((hud_scale_of_part(0.0), hud_scale_of_part(1.0)), (0.25, 2.5));
    assert_eq!(hud_scale_of_part(f32::NAN), 1.0);
    for i in 0..HUD_SCALE_CHOICES {
        let part = settings::choice_part(i, HUD_SCALE_CHOICES);
        let expected = 0.25 + i as f32 * 0.05;
        assert!((hud_scale_of_part(part) - expected).abs() < 0.001);
        assert_eq!(hud_scale_part(hud_scale_of_part(part)), part);
    }
}

#[test]
fn desktop_register_hud_scale_keeps_ui_scale_visible_and_omits_hud_scale_v2() {
    let mut app = App::new();
    register_hud_scale(&mut app, "Display");
    let settings = app.world().resource::<settings::Settings>();
    let legacy = settings.index("ui-scale").expect("desktop keeps ui-scale");
    assert_eq!(settings.index(HUD_SCALE_KEY), None, "desktop must not register hud-scale-v2");
    assert!(settings.list()[legacy].slider.is_none());
    assert_eq!((settings.choices(legacy).unwrap().names)(app.world()).len(), 6);
    assert!(settings.hidden(app.world()).is_empty(), "desktop ui-scale must remain visible");
}

#[test]
fn actual_registered_hud_slider_migrates_legacy_sizes_without_changing_menu_values() {
    let mut app = App::new();
    register_android_hud_scale(&mut app, "Display");
    crate::menu_scale::register(&mut app, "Display");
    let settings = app.world().resource::<settings::Settings>();
    let row = settings.list().iter().find(|row| row.key == HUD_SCALE_KEY).unwrap();
    let slider = row.slider.unwrap();
    for (old, size) in [("0", 0.5), ("0.25", 0.75), ("0.5", 1.0), ("1", 1.5)] {
        let input = format!("[Settings]\nui-scale={old}\nmenu-scale=0.3\n[Other]\nkeep=yes\n");
        let migrated = settings.split_older(&input);
        let part: f32 = fnv_install::ini_get(&migrated, "Settings", HUD_SCALE_KEY).unwrap().parse().unwrap();
        assert!((hud_scale_of_part(slider.fit(part)) - size).abs() < 0.001);
        assert_eq!(fnv_install::ini_get(&migrated, "Settings", "menu-scale").as_deref(), Some("0.3"));
        assert_eq!(fnv_install::ini_get(&migrated, "Other", "keep").as_deref(), Some("yes"));
    }
    let explicit = "[Settings]\nui-scale=0\nhud-scale-v2=1\nmenu-scale=0.3\n";
    assert_eq!(settings.split_older(explicit), explicit);
    assert_eq!(slider.text(1.0), "250%");
}

// These production setters use process-wide HUD atomics; isolate their real startup
// boundary in the same test binary so other interface fixtures keep their defaults.
#[test]
fn actual_hud_ini_migration_edit_restart_and_wide_drawing_are_process_isolated() {
    const CHILD: &str = "NVF_HUD_SCALE_EXTENDED_TEST_CHILD";
    if std::env::var(CHILD).as_deref() != Ok("1") {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "interface::hud_scale_extended_tests::actual_hud_ini_migration_edit_restart_and_wide_drawing_are_process_isolated", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1").stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(10)),
                status => {
                    let killed = child.kill();
                    let waited = child.wait();
                    panic!("isolated HUD deadline/wait failure: {status:?}; kill={killed:?}; wait={waited:?}");
                }
            }
        }
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "isolated HUD consumer failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return;
    }
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("nvf-wide-hud-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    struct Files(std::path::PathBuf);
    impl Drop for Files {
        fn drop(&mut self) {
            for file in ["NVForever.ini", "NVForever.ini.bak", "NVForever.ini.tmp"] {
                let file = self.0.join(file);
                if file.is_file() { std::fs::remove_file(file).unwrap(); }
            }
            std::fs::remove_dir(&self.0).unwrap();
        }
    }
    let _files = Files(dir.clone());
    let path = dir.join("NVForever.ini");
    std::fs::write(&path, "[Settings]\nui-scale=0.25\nmenu-scale=0.3\n[Other]\nkeep=yes\n").unwrap();
    let start = || {
        set_ui_scale_setting(1.0);
        assert_eq!(ui_scale_setting(), 1.0, "cold startup default cannot retain the prior process setting");
        let mut app = App::new();
        app.insert_resource(settings::SettingsFile(Some(path.clone())));
        register_android_hud_scale(&mut app, "Display");
        crate::menu_scale::register(&mut app, "Display");
        app.add_systems(PostStartup, settings::apply_stored);
        app.add_systems(PostStartup, settings::fix_ui_scale.after(settings::apply_stored));
        app.update();
        app
    };
    let mut app = start();
    assert_eq!(ui_scale_setting(), 0.75, "legacy saved size is decoded before startup");
    assert_eq!(ui_scale(), 0.75, "actual HUD drawing receives migrated value");
    assert_eq!(app.world().resource::<crate::menu_scale::MenuScale>().scale(), 0.6, "Main list loads the old 65% menu value at its nearest 60% choice");
    let row = app.world().resource::<settings::Settings>().index(HUD_SCALE_KEY).unwrap();
    settings::set_slider(app.world_mut(), row, 1.0);
    assert_eq!(ui_scale_setting(), 2.5);
    assert_eq!(ui_scale(), 0.75, "editing preserves documented next-start behavior");
    let _restarted = start();
    assert_eq!(ui_scale(), 2.5, "actual new startup draws beyond the former 150% cap");
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_eq!(fnv_install::ini_get(&saved, "Settings", HUD_SCALE_KEY).unwrap().parse::<f32>().unwrap(), 1.0);
    assert_eq!(fnv_install::ini_get(&saved, "Settings", "menu-scale").as_deref(), Some("60%"), "HUD edits preserve the independent menu value using Main's canonical list codec");
    assert_eq!(fnv_install::ini_get(&saved, "Other", "keep").as_deref(), Some("yes"));
}

#[test]
fn actual_registered_hud_retains_main_legacy_list_and_migrates_its_named_values() {
    let mut app = App::new();
    register_android_hud_scale(&mut app, "Display");
    crate::menu_scale::register(&mut app, "Display");
    let settings = app.world().resource::<settings::Settings>();
    let legacy = settings.index("ui-scale").unwrap();
    let wide = settings.index(HUD_SCALE_KEY).unwrap();
    assert!(settings.list()[legacy].slider.is_none());
    assert_eq!((settings.choices(legacy).unwrap().names)(app.world()).len(), 6);
    assert!(settings.hidden(app.world()).contains(&(legacy, wide)), "the wider row represents its retained legacy alias");
    let slider = settings.list()[wide].slider.unwrap();
    for (name, size, menu) in [("50%", 0.5, "0.000"), ("80%", 0.8, "0.600"), ("100%", 1.0, "1.000")] {
        let input = format!("[Settings]\nui-scale={name}\n[Other]\nkeep=yes\n");
        let migrated = settings.split_older(&input);
        let part: f32 = fnv_install::ini_get(&migrated, "Settings", HUD_SCALE_KEY).unwrap().parse().unwrap();
        assert!((hud_scale_of_part(slider.fit(part)) - size).abs() < 0.001);
        assert_eq!(fnv_install::ini_get(&migrated, "Settings", crate::menu_scale::KEY).as_deref(), Some(menu));
        assert_eq!(fnv_install::ini_get(&migrated, "Settings", "ui-scale").as_deref(), Some(name));
        assert_eq!(fnv_install::ini_get(&migrated, "Other", "keep").as_deref(), Some("yes"));
        let explicit = format!("[Settings]\nui-scale={name}\nhud-scale-v2=1\nmenu-scale=70%\n");
        assert_eq!(settings.split_older(&explicit), explicit, "explicit independent values win over the old named key");
    }
    assert!(crate::menu_scale::legacy_menu("75%").is_empty(), "an unregistered legacy name is not guessed");
}
