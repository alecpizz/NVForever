//! The settings page written beside `NVForever.ini` for the launcher
//! (`nvf_settings_layout::menu`, [`write_menu`]): every tab, category and row as this run's
//! page lays them out (the registry through the table, `crate::settings_layout`), with each
//! row's label and note as the page shows them and how it changes (switch, slider and the
//! words of its steps, list and its choices) [design: owner 2026-11-16: the launcher's
//! settings must always be the game's, so it never names a menu the game does not have].
//!
//! Written once every row is registered (`PostStartup`, after `crate::settings::check_notes`),
//! only when it differs from the file there; the log says
//! `settings: menu for the launcher written: <path> (<tabs> tabs, <rows> rows)` (`unchanged`
//! when the file there is the same). A run with no
//! settings file (`SettingsFile(None)`) writes none.
//!
//! The file is the page of a player's start, so a run that is not one leaves the file there
//! as it is (`settings: menu for the launcher not written: …`) [design: #2830 review]: a run
//! given a switch a row names (`--no-voice`, `--graphics-only`: the row's state, or its
//! module, is the command line's then, `GivenSwitches`), and a run with fewer rows than the
//! file there from the same build (version and table stamp: a module missing this run).
//!
//! Each row carries its default (the state before the INI was read, `StartStates`,
//! `StartChoices`: no switch a row names was given, so the defaults), which the launcher
//! goes by while the INI does not hold that row.

use bevy::prelude::*;
use fnv_util::words::tr;
use nvf_settings_layout::menu::{self, Menu};

use crate::pause::on_page;
use crate::settings::{GivenSwitches, Settings, SettingsFile, Slider, StartChoices, StartStates};
use crate::settings_layout::resolve_layout;

/// The most steps a slider's words are listed for (the field of view's are some 100).
const MOST_STEPS: usize = 1000;

/// The page as `settings` lays it out in `world` (the Controls tab's own rows left out: they
/// are not `[Settings]` rows).
pub fn menu_of(world: &World, settings: &Settings) -> Menu {
    let layout = resolve_layout(settings.list());
    let mut tabs = Vec::new();
    for t in &layout.tabs {
        let categories: Vec<menu::Category> = t
            .sections
            .iter()
            .filter(|s| !s.rows.is_empty())
            .map(|s| menu::Category {
                group: s.placement.subgroup.map(on_page),
                heading: s.placement.heading.map(on_page),
                rows: s.rows.iter().map(|&i| row(world, settings, i)).collect(),
            })
            .collect();
        if !categories.is_empty() {
            tabs.push(menu::Tab { name: t.name.to_string(), label: on_page(t.name), categories });
        }
    }
    Menu { version: env!("CARGO_PKG_VERSION").to_string(), layout: layout_stamp(), tabs }
}

fn layout_stamp() -> String {
    if cfg!(target_os = "android") {
        nvf_settings_layout::mobile_layout_stamp()
    } else {
        nvf_settings_layout::layout_stamp()
    }
}

fn row(world: &World, settings: &Settings, i: usize) -> menu::Row {
    let s = &settings.list()[i];
    let kind = if let Some(list) = settings.choices(i) {
        // A choice that does something instead of being kept (Download DLSS… on the
        // Upscaler row) is not a value: the INI never holds it.
        let action = settings.choice_looks(i).and_then(|l| l.action);
        let choices = (list.names)(world)
            .into_iter()
            .filter(|name| !action.is_some_and(|is_action| is_action(name)))
            .map(|name| {
                let label = on_page(&settings.choice_label(world, i, &name));
                menu::Choice { name, label }
            })
            .collect();
        menu::Kind::List { choices }
    } else if let Some(slider) = s.slider {
        menu::Kind::Slider { min: slider.min, step: slider.step, steps: steps(&slider) }
    } else {
        menu::Kind::Switch
    };
    // What it needs of other rows (the shared tables the page goes by), its reason translated.
    let words = |w: &'static str| tr(w).to_string();
    let shown_with = crate::settings_layout::shown_with(s.key).map(|r| menu::Rule::of(r, words));
    let needs = nvf_settings_layout::needs(s.key).into_iter().map(|r| menu::Rule::of(r, words)).collect();
    menu::Row { key: s.key.to_string(), label: on_page(s.label), note: tr(s.note).to_string(), kind, default: default(world, settings, i), shown_with, needs }
}

/// Row `i`'s state before the INI was read, as the INI keeps it; empty when not known (no
/// `crate::settings::apply_stored` this run).
fn default(world: &World, settings: &Settings, i: usize) -> String {
    if settings.choices(i).is_some() {
        let start = world.get_resource::<StartChoices>();
        start.and_then(|c| c.0.iter().find(|(r, _)| *r == i)).map(|(_, t)| t.clone()).unwrap_or_default()
    } else {
        let start = world.get_resource::<StartStates>();
        start.and_then(|s| s.0.get(i)).map(|&v| settings.list()[i].stored_text(v)).unwrap_or_default()
    }
}

/// Why this run's page is not a player's start, so not one for the launcher (module notes);
/// `None`: it is.
fn not_a_players_start(world: &World, settings: &Settings, m: &Menu, file: &std::path::Path) -> Option<String> {
    if let Some(given) = world.get_resource::<GivenSwitches>() {
        if let Some(first) = settings.list().iter().find_map(|s| given.named_in(s.switches)) {
            return Some(format!("{first} on the command line sets a row"));
        }
    }
    let old = std::fs::read_to_string(file).ok().and_then(|t| Menu::parse(&t))?;
    let (had, has) = (old.rows().count(), m.rows().count());
    (old.version == m.version && old.layout == m.layout && has < had).then(|| format!("{has} rows, the same build's file there has {had} (a module missing this run)"))
}

/// The values `slider` reaches (`Slider::fit`) and the words the page shows for each, when
/// it shows words (none for a percentage, or past [`MOST_STEPS`]).
fn steps(slider: &Slider) -> Vec<menu::Step> {
    if slider.shown.is_none() {
        return Vec::new();
    }
    let step = slider.step.max(1e-3);
    let (first, last) = ((slider.min / step - 1e-4).ceil() as i64, (1.0 / step).round() as i64);
    if last - first + 1 > MOST_STEPS as i64 {
        return Vec::new();
    }
    let mut out: Vec<menu::Step> = Vec::new();
    for k in first..=last {
        let value = slider.fit(k as f32 * step);
        if out.last().is_none_or(|s| s.value != value) {
            out.push(menu::Step { value, label: on_page(&slider.text(value)) });
        }
    }
    out
}

/// Write the page beside the settings file when it differs from the one there.
pub fn write_menu(world: &World) {
    let Some(path) = world.get_resource::<SettingsFile>().and_then(|f| f.0.clone()) else { return };
    let Some(settings) = world.get_resource::<Settings>() else { return };
    let m = menu_of(world, settings);
    let text = m.to_text();
    let file = path.with_file_name(menu::MENU_FILE);
    let (tabs, rows) = (m.tabs.len(), m.rows().count());
    if std::fs::read_to_string(&file).is_ok_and(|t| t == text) {
        info!("settings: menu for the launcher unchanged: {} ({tabs} tabs, {rows} rows)", file.display());
        return;
    }
    if let Some(why) = not_a_players_start(world, settings, &m, &file) {
        info!("settings: menu for the launcher not written: {why}; {} left as it is", file.display());
        return;
    }
    // Through the same gatekeeper as the INI beside it (`settings::write`): never into the
    // game folder or the original's user folder, whatever the home points at.
    if let Err(e) = fnv_install::guard_write(&file, None) {
        warn!("settings: menu for the launcher not written: {e}");
        return;
    }
    match std::fs::write(&file, &text) {
        Ok(()) => info!("settings: menu for the launcher written: {} ({tabs} tabs, {rows} rows)", file.display()),
        Err(e) => warn!("settings: menu for the launcher not written: {}: {e}", file.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{add_choice_list, add_setting, set_choice_action, ChoiceList, Setting};

    #[derive(Resource, Default)]
    struct Part(f32);

    fn setting(key: &'static str, group: &'static str, label: &'static str) -> Setting {
        Setting { key, group, label, note: "What it does.", switches: "", get: |_| false, set: |_, _| {}, slider: None }
    }

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Part>();
        add_setting(&mut app, setting("hardcore", "Gameplay", "Hardcore mode"));
        let words = |v: f32| ["Very easy", "Easy", "Normal", "Hard", "Very hard"][(v * 4.0).round() as usize].to_string();
        add_setting(&mut app, Setting { slider: Some(Slider::choice(5, |w| w.resource::<Part>().0, |w, v| w.resource_mut::<Part>().0 = v, words)), ..setting("difficulty", "Gameplay", "Difficulty") });
        add_setting(&mut app, Setting { slider: Some(Slider { get: |_| 1.0, set: |_, _| {}, min: 0.1, step: 0.05, shown: None }), ..setting("volume-master", "Audio", "Master volume") });
        let list = ChoiceList { names: |_| vec!["Classic".into(), "Mark and Fire".into()], chosen: |_| "Classic".into(), pick: |_, _| true, save: |_| "Classic".into(), load: |_, _| true, ask_keep: false };
        add_choice_list(&mut app, setting("vats-style", "Gameplay", "V.A.T.S. style"), list);
        let upscaler = ChoiceList { names: |_| vec!["Off".into(), "DLSS".into(), "Download DLSS...".into()], chosen: |_| "Off".into(), pick: |_, _| true, save: |_| "Off".into(), load: |_, _| true, ask_keep: false };
        add_choice_list(&mut app, setting("upscaler", "Display", "Upscaler"), upscaler);
        set_choice_action(&mut app, "upscaler", |name| name.starts_with("Download"));
        add_setting(&mut app, setting("a-later-row", "Gameplay", "A later row"));
        add_setting(&mut app, setting("vats-slowdown", "Gameplay", "Time slowdown"));
        app
    }

    #[test]
    fn the_menu_is_the_pages_tabs_categories_and_rows() {
        let app = app();
        let w = app.world();
        let m = menu_of(w, w.resource::<Settings>());
        assert_eq!((m.version.as_str(), m.layout.clone()), (env!("CARGO_PKG_VERSION"), layout_stamp()));
        let tabs: Vec<&str> = m.tabs.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(tabs, ["Gameplay", "Display", "Audio"], "in the table's order, a tab with no rows left out");
        let g = &m.tabs[0];
        let heads: Vec<Option<&str>> = g.categories.iter().map(|c| c.heading.as_deref()).collect();
        assert_eq!(heads, [Some("Game"), Some("V.A.T.S."), None], "the table's categories, then the row it does not place");
        assert_eq!(g.categories[0].rows[1].label, "Hardcore Mode", "as the page shows it");
        assert_eq!(g.categories[0].rows[1].kind, menu::Kind::Switch);
        let diff = &g.categories[0].rows[0];
        assert_eq!(diff.key, "difficulty");
        let menu::Kind::Slider { min, step, steps } = &diff.kind else { panic!("{diff:?}") };
        assert_eq!((*min, *step), (0.0, 0.25));
        assert_eq!(steps.iter().map(|s| (s.value, s.label.as_str())).collect::<Vec<_>>(), [(0.0, "Very Easy"), (0.25, "Easy"), (0.5, "Normal"), (0.75, "Hard"), (1.0, "Very Hard")]);
        let menu::Kind::List { choices } = &g.categories[1].rows[0].kind else { panic!() };
        // The rules the page goes by, for the launcher (the shared tables).
        let slowdown = &g.categories[1].rows[1];
        assert_eq!(slowdown.key, "vats-slowdown");
        assert_eq!(slowdown.needs, vec![menu::Rule { parent: "vats-style".into(), choices: vec!["Classic".into()], why: "Classic V.A.T.S. only.".into() }]);
        assert!(g.categories[1].rows[0].needs.is_empty() && g.categories[1].rows[0].shown_with.is_none());
        let upscaler = &m.tabs[1].categories[0].rows[0];
        assert_eq!(upscaler.key, "upscaler");
        assert_eq!(choices.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Classic", "Mark and Fire"]);
        let menu::Kind::List { choices } = &m.tabs[1].categories[0].rows[0].kind else { panic!() };
        assert_eq!(choices.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Off", "DLSS"], "an action is not a value the INI keeps");
        let volume = &m.tabs[2].categories[0].rows[0];
        assert_eq!(volume.kind, menu::Kind::Slider { min: 0.1, step: 0.05, steps: Vec::new() }, "a percentage: no words");
    }

    #[test]
    fn written_beside_the_ini_only_when_it_changed() {
        let dir = std::env::temp_dir().join(format!("nvf-settings-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = app();
        app.insert_resource(SettingsFile(Some(dir.join("NVForever.ini"))));
        let file = dir.join(menu::MENU_FILE);
        write_menu(app.world());
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(Menu::parse(&text).map(|m| m.rows().count()), Some(7));
        std::fs::write(&file, "old").unwrap();
        write_menu(app.world());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text, "an old file is replaced");
        app.insert_resource(SettingsFile(None));
        std::fs::remove_file(&file).unwrap();
        write_menu(app.world());
        assert!(!file.exists(), "no settings file: no menu");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exported_android_controller_rules_keep_shared_rules_and_actual_registry_rows() {
        let mut app = app();
        for key in crate::settings_layout::ANDROID_CONTROLLER_KEYS {
            add_setting(&mut app, setting(key, "Controller", key));
        }
        add_setting(&mut app, setting("button-style", "Controller", "Button style"));
        add_setting(&mut app, setting("upscaler-quality", "Graphics", "Upscaler quality"));
        let before: Vec<_> = app.world().resource::<Settings>().list().iter()
            .map(|s| (s.key, s.group, s.label, s.note)).collect();
        let menu = menu_of(app.world(), app.world().resource::<Settings>());
        let row = |key: &str| menu.rows().find(|r| r.key == key).unwrap();
        let controller = menu::Rule { parent: crate::settings_layout::DEVICE_ROW.into(),
            choices: vec![crate::settings_layout::CONTROLLER_DEVICE.into()], why: String::new() };
        for key in crate::settings_layout::ANDROID_CONTROLLER_KEYS {
            assert_eq!(row(key).shown_with, Some(controller.clone()), "{key}");
            assert!(row(key).needs.is_empty());
        }
        assert_eq!(row("button-style").shown_with, Some(controller));
        assert_eq!(row("upscaler-quality").shown_with, Some(menu::Rule {
            parent: "upscaler".into(), choices: vec!["FSR 3.1".into(), "DLSS".into()], why: String::new() }));
        assert!(row("a-later-row").shown_with.is_none());
        assert!(row("a-later-row").needs.is_empty());
        assert!(menu.rows().all(|r| r.key != "controller-not-registered"));
        assert_eq!(crate::settings_layout::shown_with("controller-not-registered"), None);
        assert_eq!(menu.rows().count(), before.len(), "export every actual registered row once");
        assert_eq!(before, app.world().resource::<Settings>().list().iter()
            .map(|s| (s.key, s.group, s.label, s.note)).collect::<Vec<_>>());
        assert_eq!(Menu::parse(&menu.to_text()), Some(menu), "same public shared schema");
    }

    #[test]
    fn each_row_carries_its_default() {
        let mut app = app();
        app.world_mut().resource_mut::<Part>().0 = 0.75;
        crate::settings::apply_stored(app.world_mut());
        app.world_mut().resource_mut::<Part>().0 = 0.25;
        let w = app.world();
        let m = menu_of(w, w.resource::<Settings>());
        let at = |k: &str| m.rows().find(|r| r.key == k).unwrap().default.clone();
        assert_eq!((at("difficulty"), at("hardcore"), at("vats-style"), at("volume-master")), ("0.75".into(), "0".into(), "Classic".into(), "1.00".into()), "the states before the INI");
        let none = self::app();
        assert_eq!(menu_of(none.world(), none.world().resource::<Settings>()).rows().next().map(|r| r.default.clone()), Some(String::new()), "not known: empty");
    }

    #[test]
    fn only_a_players_start_writes_it() {
        let dir = std::env::temp_dir().join(format!("nvf-settings-menu-start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(menu::MENU_FILE);
        // A switch a row names: the row's state is the command line's, the file stays.
        let mut app = app();
        add_setting(&mut app, setting("voice", "Audio", "Dialogue voice").with_switches("--no-voice"));
        app.insert_resource(SettingsFile(Some(dir.join("NVForever.ini"))));
        app.insert_resource(GivenSwitches { given: vec!["--no-voice".into()], win: false });
        write_menu(app.world());
        assert!(!file.exists(), "a switch a row names: not written");
        // Other switches (the world, the data folder), a test run's too: written.
        app.insert_resource(GivenSwitches { given: vec!["--play".into(), "--data".into()], win: true });
        write_menu(app.world());
        let full = Menu::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(full.rows().count(), 8);
        // A run of the same build with a module missing: fewer rows, the file stays.
        let mut less = self::app();
        less.insert_resource(SettingsFile(Some(dir.join("NVForever.ini"))));
        write_menu(less.world());
        assert_eq!(Menu::parse(&std::fs::read_to_string(&file).unwrap()), Some(full.clone()), "fewer rows of the same build: kept");
        // Another build's file (an older version, or another table): replaced.
        let older = Menu { layout: "0000000000000000".into(), ..full };
        std::fs::write(&file, older.to_text()).unwrap();
        write_menu(less.world());
        assert_eq!(Menu::parse(&std::fs::read_to_string(&file).unwrap()).map(|m| m.rows().count()), Some(7), "another build's: replaced");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
