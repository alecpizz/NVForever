// Actual registrations, shared page action APIs and the ordinary INI consumer.
// Rendering/input behavior stays covered by the unchanged owning raw/Taffy cases.
struct AvailabilityFile(std::path::PathBuf);
impl AvailabilityFile {
    fn new(text: &str) -> Self {
        let path = std::env::temp_dir().join(format!("nvf-dialogue-availability-{}-{}.ini",
            std::process::id(), std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&path, text).unwrap();
        Self(path)
    }
}
impl Drop for AvailabilityFile {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}
fn availability_app(file: &AvailabilityFile) -> App {
    let mut app = App::new();
    install(&mut app);
    app.insert_resource(crate::settings::SettingsFile(Some(file.0.clone())));
    app
}
fn availability_row(app: &App, key: &str) -> usize {
    app.world().resource::<crate::settings::Settings>().index(key).unwrap()
}
fn availability_enabled(app: &mut App, on: bool) {
    if app.world().resource::<TouchDialogue>().enabled != on {
        let row = availability_row(app, "touch-dialogue");
        assert_eq!(crate::settings::flip(app.world_mut(), row), Some(on));
    }
    assert_eq!(app.world().resource::<TouchDialogue>().enabled, on);
}
fn availability_children() -> [&'static str; 10] {
    ["dialogue-choice-text-size", "dialogue-subtitle-text-size", "dialogue-touch-choice",
        "dialogue-speaker-side", "dialogue-device-side-v1", "dialogue-theme-v1", "dialogue-background-opacity-v1",
        "dialogue-background-tint-v1", "dialogue-frame-opacity-v1", "dialogue-hints-v1"]
}
#[test]
fn enhanced_dialogue_registered_positive_then_off_greys_all_children_without_mutating_them() {
    let file = AvailabilityFile::new("[Settings]\n[Unrelated]\nkeep=yes\n");
    let mut app = availability_app(&file);
    availability_enabled(&mut app, true);
    let theme_row=availability_row(&app, "dialogue-theme-v1");
    assert_eq!(app.world().resource::<appearance::Appearance>().theme,appearance::Theme::Minimal);
    for name in ["Normal","Classic","Retro Console","Wrist Terminal"] {
        assert_eq!(crate::settings::pick(app.world_mut(),theme_row,name),name!="Normal");
        for key in ["dialogue-background-opacity-v1","dialogue-background-tint-v1","dialogue-frame-opacity-v1","dialogue-hints-v1"] {
            let settings=app.world().resource::<crate::settings::Settings>();
            assert_eq!(settings.disabled_status(app.world(),settings.index(key).unwrap()),Some("Available with the Field Notes theme."),"{name}: {key}");
        }
        // The device models are placed by the speaker side too.
        let settings=app.world().resource::<crate::settings::Settings>();
        let side=settings.disabled_status(app.world(),settings.index("dialogue-speaker-side").unwrap());
        assert_eq!(side,(!matches!(name,"Retro Console"|"Wrist Terminal"))
            .then_some("Available with Field Notes, Retro Console and Wrist Terminal."),"{name}: speaker side");
        let dev_side=settings.disabled_status(app.world(),settings.index("dialogue-device-side-v1").unwrap());
        assert_eq!(dev_side,(!matches!(name,"Retro Console"|"Wrist Terminal"))
            .then_some("Available with Retro Console and Wrist Terminal."),"{name}: device side");
    }
    assert!(crate::settings::pick(app.world_mut(),theme_row,"Field Notes"));
    for key in availability_children() {
        let settings = app.world().resource::<crate::settings::Settings>();
        let expected = (key == "dialogue-device-side-v1").then_some("Available with Retro Console and Wrist Terminal.");
        assert_eq!(settings.disabled_status(app.world(), settings.index(key).unwrap()), expected, "{key}");
    }
    let row = availability_row(&app, "dialogue-theme-v1");
    assert!(crate::settings::pick(app.world_mut(), row, "Wrist Terminal"), "real registered callback positive first");
    let row = availability_row(&app, "dialogue-choice-text-size");
    assert!(crate::settings::set_slider(app.world_mut(), row, 0.6).is_some());
    availability_enabled(&mut app, false);
    let appearance = *app.world().resource::<appearance::Appearance>();
    let dialogue = *app.world().resource::<TouchDialogue>();
    let side = *app.world().resource::<crate::dialogue_split::Options>();
    let bytes = std::fs::read(&file.0).unwrap();
    for key in availability_children() {
        let settings = app.world().resource::<crate::settings::Settings>();
        assert_eq!(settings.disabled_status(app.world(), settings.index(key).unwrap()), Some("Needs Touch dialogue."), "{key}");
    }
    for (key, name) in [("dialogue-theme-v1", "Classic"),
        ("dialogue-background-tint-v1", "Warm amber"), ("dialogue-touch-choice", "Direct tap"),
        ("dialogue-speaker-side", "Speaker right")] {
        let row = availability_row(&app, key);
        assert!(!crate::settings::pick(app.world_mut(), row, name), "unavailable {key} cannot be edited through the page");
    }
    for key in ["dialogue-choice-text-size", "dialogue-subtitle-text-size",
        "dialogue-background-opacity-v1", "dialogue-frame-opacity-v1"] {
        let row = availability_row(&app, key);
        assert_eq!(crate::settings::set_slider(app.world_mut(), row, 0.4), None, "{key}");
    }
    let hints = availability_row(&app, "dialogue-hints-v1");
    assert_eq!(crate::settings::flip(app.world_mut(), hints), None);
    assert_eq!(*app.world().resource::<appearance::Appearance>(), appearance);
    assert_eq!(*app.world().resource::<TouchDialogue>(), dialogue);
    assert_eq!(*app.world().resource::<crate::dialogue_split::Options>(), side);
    assert_eq!(std::fs::read(&file.0).unwrap(), bytes, "refused edits never save a new value");
    availability_enabled(&mut app, true);
    let row = availability_row(&app, "dialogue-theme-v1");
    assert!(crate::settings::pick(app.world_mut(), row, "Classic"));
    assert_eq!(app.world().resource::<appearance::Appearance>().theme, appearance::Theme::ClassicGlass);
}

#[test]
fn enhanced_dialogue_disabled_children_load_and_roundtrip_then_on_edits_and_off_preserves() {
    let input = "[Settings]\ntouch-dialogue=0\ndialogue-theme-v1=Wrist Terminal\ndialogue-choice-text-size=0.8\ndialogue-subtitle-text-size=0.2\ndialogue-touch-choice=Direct tap\ndialogue-speaker-side=Speaker left\ndialogue-background-opacity-v1=0.3\ndialogue-frame-opacity-v1=0.4\ndialogue-background-tint-v1=Neutral\ndialogue-hints-v1=1\n[Unrelated]\nkeep=yes\n";
    let file = AvailabilityFile::new(input);
    let mut app = availability_app(&file);
    crate::settings::apply_stored(app.world_mut());
    assert!(!app.world().resource::<TouchDialogue>().enabled);
    assert!(!app.world().resource::<TouchDialogue>().confirm);
    assert_eq!(app.world().resource::<TouchDialogue>().choice_scale, from_part(0.8));
    assert_eq!(app.world().resource::<TouchDialogue>().subtitle_scale, from_part(0.2));
    assert_eq!(app.world().resource::<appearance::Appearance>().theme, appearance::Theme::WristTerminal);
    assert_eq!(app.world().resource::<appearance::Appearance>().tint, appearance::Tint::Neutral);
    assert!(app.world().resource::<appearance::Appearance>().hints);
    assert_eq!(app.world().resource::<crate::dialogue_split::Options>().side, crate::dialogue_split::Side::Left);
    assert_eq!(std::fs::read_to_string(&file.0).unwrap(), input, "INI restoration remains nonwriting while rows are grey");
    availability_enabled(&mut app, true);
    let row = availability_row(&app, "dialogue-touch-choice");
    assert!(crate::settings::pick(app.world_mut(), row, "Select then confirm"));
    let theme_row = availability_row(&app,"dialogue-theme-v1");
    assert!(crate::settings::pick(app.world_mut(),theme_row,"Field Notes"));
    let row = availability_row(&app, "dialogue-background-opacity-v1");
    assert!(crate::settings::set_slider(app.world_mut(), row, 0.7).is_some());
    let row = availability_row(&app, "dialogue-speaker-side");
    assert!(crate::settings::pick(app.world_mut(), row, "Speaker right"));
    let row = availability_row(&app, "dialogue-hints-v1");
    assert_eq!(crate::settings::flip(app.world_mut(), row), Some(false));
    assert!(crate::settings::pick(app.world_mut(),theme_row,"Wrist Terminal"));
    availability_enabled(&mut app, false);
    let appearance = *app.world().resource::<appearance::Appearance>();
    let dialogue = *app.world().resource::<TouchDialogue>();
    let side = *app.world().resource::<crate::dialogue_split::Options>();
    let saved = std::fs::read_to_string(&file.0).unwrap();
    assert_eq!(fnv_install::ini_get(&saved, "Settings", "touch-dialogue").as_deref(), Some("0"));
    assert_eq!(fnv_install::ini_get(&saved, "Unrelated", "keep").as_deref(), Some("yes"));
    let mut reloaded = availability_app(&file);
    crate::settings::apply_stored(reloaded.world_mut());
    assert_eq!(*reloaded.world().resource::<appearance::Appearance>(), appearance);
    assert_eq!(*reloaded.world().resource::<TouchDialogue>(), dialogue);
    assert_eq!(*reloaded.world().resource::<crate::dialogue_split::Options>(), side);
    for key in availability_children() {
        let settings = reloaded.world().resource::<crate::settings::Settings>();
        assert!(settings.disabled_status(reloaded.world(), settings.index(key).unwrap()).is_some(), "{key}");
    }
}
