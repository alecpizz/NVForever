//! The settings page's menu as the launcher shows it: the game's own [design: owner
//! 2026-11-16: the launcher follows the game's menus, so it never names a tab or category
//! the game does not have]. Two sources, both the game's:
//!
//! - **The game's menu file** (`settings-menu.json` beside `NVForever.ini`,
//!   `nvf_settings_layout::menu`): the page as the game laid it out at its last start, every
//!   tab, category and row with its label, note and kind (switch, slider and its steps' words,
//!   list and its choices). Used while its version and table stamp are the launcher's
//!   ([`Catalog::from_menu`]; dev builds between releases share a version, the stamp tells
//!   their tables apart). A file from another build still lends its labels and notes onto
//!   this table, so a Tweaks row is not left as a raw key.
//! - **Built in** ([`Catalog::built_in`]), before the game has written one (a first start, a
//!   game older than the file, an update not started yet): the tabs, categories and row order
//!   from the game's own table (`nvf_settings_layout::SECTIONS`, the crate the game's pause
//!   menu draws from), the labels and notes from `docs/settings-reference.md` (a test in the
//!   game keeps it the same as the table: `settings_layout.rs`,
//!   `the_reference_has_every_row_where_the_page_shows_it`). What kind of row a key is the
//!   launcher then goes by the value the INI holds ([`Kind::of_value`]).

use std::collections::HashMap;
use std::path::Path;

use nvf_settings_layout::menu::{self, Menu};
use nvf_settings_layout::{CONTROL_PREFIX, SECTIONS, TAB_ORDER, TWEAKS_TAB};

/// The game's tabs the launcher shows, by their names in the game's table (`TAB_ORDER`), in
/// the game's order [design: owner 2026-11-16: Audio, Controls, NVF Features and Debug are
/// set in the game only]. A test fails when the game no longer has one of them.
pub const LAUNCHER_TABS: [&str; 4] = ["Gameplay", "Display", "Graphics", TWEAKS_TAB];

const REFERENCE: &str = include_str!("../../../docs/settings-reference.md");

/// The heading after which the reference lists the Controls tab's own rows (kept in
/// `[Controls]`, not `[Settings]`): the launcher stops there.
const OWN_ROWS: &str = "the tab's own rows";

/// Whether `rule` holds for its row's INI value `value` (its choice's name `name`), the row
/// of kind `kind` (`None`: built in, by the value).
pub(crate) fn rule_holds(rule: &menu::Rule, kind: Option<&menu::Kind>, value: &str, name: &str) -> bool {
    if !rule.choices.is_empty() {
        return rule.choices.iter().any(|c| c.eq_ignore_ascii_case(name));
    }
    match kind {
        Some(menu::Kind::List { choices }) => choices.first().is_none_or(|c| c.name != name),
        Some(menu::Kind::Slider { .. }) => value.parse::<f32>().is_ok_and(|v| v >= 0.5),
        // A switch, or a row with no kind (built in): off as `0`, a list's Off, or no value.
        _ => !(value.is_empty() || value == "0" || value.eq_ignore_ascii_case("off")),
    }
}

pub(crate) fn with_edits(mut text: String, changes: &std::collections::BTreeMap<String, String>) -> String {
    for (key, value) in changes {
        text = fnv_install::ini_set(&text, "Settings", key, value);
    }
    text
}

/// One row of the settings page.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub label: String,
    /// Its tab's name in the game's table (`TAB_ORDER`).
    pub tab: String,
    /// Its category's heading as shown (the Tweaks tab's: "Group › Heading").
    pub heading: Option<String>,
    pub note: String,
    /// How it changes, from the game's menu file; `None` (built in): by the INI's value.
    pub kind: Option<menu::Kind>,
    /// The value the game takes while the INI has none (the game's menu file); `None`: not
    /// known (built in, or a file written before it had them).
    pub default: Option<String>,
    /// Built in, with no words in the reference (a Tweaks row): shown only while the INI
    /// holds it. The label is words, not the key. The game's menu replaces it.
    pub unlisted: bool,
    /// Shown only while this holds, greyed while one of `needs` does not: the game's rules
    /// (`nvf_settings_layout::shown_with`, `needs`), the same in both sources.
    pub shown_with: Option<menu::Rule>,
    pub needs: Vec<menu::Rule>,
}

/// The page: its tabs (the table's name, the words shown) and rows, in the page's order.
#[derive(Clone, Debug, PartialEq)]
pub struct Catalog {
    pub tabs: Vec<(String, String)>,
    pub rows: Vec<Row>,
    /// From the game's menu file (not built in).
    pub from_game: bool,
}

impl Catalog {
    /// The game's menu beside `ini` when it is this launcher's version and table. Another
    /// build's file still supplies the words. No file: the built-in table.
    pub fn load(ini: Option<&Path>) -> Catalog {
        Self::load_tabs(ini, &LAUNCHER_TABS, nvf_settings_layout::layout_stamp)
    }

    #[cfg(any(feature = "mobile", test))]
    pub(crate) fn load_mobile(ini: Option<&Path>, tabs: &[&str]) -> Catalog {
        Self::load_tabs(ini, tabs, nvf_settings_layout::mobile_layout_stamp)
    }

    /// [`Catalog::load`] for `tabs` and its table stamp: the game's menu when it is this
    /// build's; another build's still lends its words to the built-in table
    /// ([`Catalog::with_game_words`]); no file, the built-in table.
    fn load_tabs(ini: Option<&Path>, tabs: &[&str], stamp: fn() -> String) -> Catalog {
        let parsed = ini.and_then(|p| std::fs::read_to_string(p.with_file_name(menu::MENU_FILE)).ok()).and_then(|t| Menu::parse(&t));
        let Some(menu) = parsed else {
            return Catalog::built_in();
        };
        Catalog::from_menu_tabs(&menu, env!("CARGO_PKG_VERSION"), tabs, stamp).unwrap_or_else(|| Catalog::built_in().with_game_words(&menu))
    }

    /// The game's menu `m`, when it is version `version`'s and of this launcher's table
    /// (`nvf_settings_layout::layout_stamp`).
    pub fn from_menu(m: &Menu, version: &str) -> Option<Catalog> {
        Self::from_menu_tabs(m, version, &LAUNCHER_TABS, nvf_settings_layout::layout_stamp)
    }

    fn from_menu_tabs(m: &Menu, version: &str, tabs: &[&str], stamp: fn() -> String) -> Option<Catalog> {
        // Not this version's, another build's table (or a file from before the stamp), or
        // with no row (a broken write): the built-in copy.
        if m.version != version || m.layout != stamp() || m.rows().next().is_none() {
            return None;
        }
        let mut rows = Vec::new();
        let tabs: Vec<&menu::Tab> = m.tabs.iter().filter(|t| tabs.contains(&t.name.as_str())).collect();
        for t in &tabs {
            for c in &t.categories {
                let heading = match (&c.group, &c.heading) {
                    (Some(g), Some(h)) => Some(format!("{g} › {h}")),
                    (g, h) => g.clone().or_else(|| h.clone()),
                };
                for r in &c.rows {
                    // The file's rules (their reasons in the game's language); a file written
                    // before it had them: the shared table's, the same game version's.
                    let english = |w: &'static str| w.to_string();
                    let shown_with = r.shown_with.clone().or_else(|| nvf_settings_layout::shown_with(&r.key).map(|x| menu::Rule::of(x, english)));
                    let needs = if r.needs.is_empty() { nvf_settings_layout::needs(&r.key).into_iter().map(|x| menu::Rule::of(x, english)).collect() } else { r.needs.clone() };
                    rows.push(Row {
                        key: r.key.clone(),
                        label: r.label.clone(),
                        tab: t.name.clone(),
                        heading: heading.clone(),
                        note: r.note.clone(),
                        kind: Some(r.kind.clone()),
                        default: (!r.default.is_empty()).then(|| r.default.clone()),
                        unlisted: false,
                        shown_with,
                        needs,
                    });
                }
            }
        }
        if rows.is_empty() {
            return None;
        }
        let tabs = tabs.iter().map(|t| (t.name.clone(), t.label.clone())).collect();
        Some(Catalog { tabs, rows, from_game: true })
    }

    /// The game's table with the reference's words (module notes).
    pub fn built_in() -> Catalog {
        let words: HashMap<String, Row> = parse(REFERENCE).into_iter().map(|r| (r.key.clone(), r)).collect();
        let mut rows = Vec::new();
        for s in SECTIONS.iter().filter(|s| LAUNCHER_TABS.contains(&s.placement.tab)) {
            let p = s.placement;
            let heading = match (p.subgroup, p.heading) {
                (Some(g), Some(h)) => Some(format!("{g} › {h}")),
                (g, h) => g.or(h).map(str::to_string),
            };
            for k in s.keys.iter().filter(|k| !k.starts_with(CONTROL_PREFIX)) {
                let (label, note, unlisted) = match words.get(*k) {
                    Some(w) => (w.label.clone(), w.note.clone(), false),
                    None => (readable_tweak(k), String::new(), true),
                };
                let english = |w: &'static str| w.to_string();
                let shown_with = nvf_settings_layout::shown_with(k).map(|r| menu::Rule::of(r, english));
                let needs = nvf_settings_layout::needs(k).into_iter().map(|r| menu::Rule::of(r, english)).collect();
                rows.push(Row { key: k.to_string(), label, tab: p.tab.to_string(), heading: heading.clone(), note, kind: None, default: None, unlisted, shown_with, needs });
            }
        }
        let tabs = TAB_ORDER.iter().filter(|t| LAUNCHER_TABS.contains(t) && rows.iter().any(|r| r.tab == **t)).map(|t| (t.to_string(), t.to_string())).collect();
        Catalog { tabs, rows, from_game: false }
    }

    /// Keeps this launcher's rows. Copies a label, note, and control from `menu` when the
    /// key is the same and the label is a real name, not the key.
    fn with_game_words(mut self, menu: &Menu) -> Self {
        let words: HashMap<&str, &menu::Row> = menu.rows().map(|r| (r.key.as_str(), r)).collect();
        for row in &mut self.rows {
            let Some(game) = words.get(row.key.as_str()) else { continue };
            let label = game.label.trim();
            let bare = row.key.strip_prefix("tweak-").unwrap_or(row.key.as_str());
            if label.is_empty() || label.eq_ignore_ascii_case(&row.key) || label.eq_ignore_ascii_case(bare) {
                continue;
            }
            row.label = game.label.clone();
            if !game.note.trim().is_empty() {
                row.note = game.note.clone();
            }
            row.kind = Some(game.kind.clone());
            if !game.default.is_empty() {
                row.default = Some(game.default.clone());
            }
            row.unlisted = false;
        }
        for (name, label) in &mut self.tabs {
            if let Some(tab) = menu.tabs.iter().find(|t| &t.name == name) {
                if !tab.label.trim().is_empty() {
                    *label = tab.label.clone();
                }
            }
        }
        self
    }

    /// The tabs' names, in order.
    pub fn tab_names(&self) -> Vec<String> {
        self.tabs.iter().map(|(n, _)| n.clone()).collect()
    }
}

/// A Tweaks key the reference does not name, as words in sentence case, as the reference
/// writes its labels (the game's page shows them in Title Case; the launcher shows them as
/// written): `tweak-aiming-draws-weapon` is "Aiming draws weapon". The game's menu file
/// replaces this.
fn readable_tweak(key: &str) -> String {
    let bare = key.strip_prefix("tweak-").unwrap_or(key);
    bare.split('-')
        .filter(|w| !w.is_empty())
        .enumerate()
        .map(|(i, w)| match w {
            // "V.A.T.S." as the data spells it (MESG `HelpVATS`), as nvf-settings-layout's
            // table does. The rest are plain abbreviations, not the original's screen text.
            "vats" => "V.A.T.S.".to_string(),
            "npc" => "NPC".to_string(),
            "hud" => "HUD".to_string(),
            "dlss" => "DLSS".to_string(),
            "ai" => "AI".to_string(),
            "xp" => "XP".to_string(),
            other if i == 0 => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The rows of `text` (the reference's Markdown): key, label, tab, heading and note.
pub fn parse(text: &str) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    let (mut tab, mut heading) = (String::new(), None::<String>);
    let mut in_note = false;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            if h.contains(OWN_ROWS) {
                break;
            }
            let mut parts = h.split('›').map(str::trim);
            tab = parts.next().unwrap_or_default().to_string();
            heading = parts.next().map(str::to_string);
            in_note = false;
        } else if let Some(h) = line.strip_prefix("### `") {
            in_note = false;
            if let Some((key, rest)) = h.split_once('`') {
                let label = rest.trim_start_matches(':').trim().to_string();
                out.push(Row { key: key.to_string(), label, tab: tab.clone(), heading: heading.clone(), note: String::new(), kind: None, default: None, unlisted: false, shown_with: None, needs: Vec::new() });
            }
        } else if let Some(n) = line.strip_prefix("- On the page:") {
            if let Some(r) = out.last_mut() {
                r.note = n.trim().to_string();
                in_note = true;
            }
        } else if in_note && line.starts_with("  ") && !line.trim_start().starts_with("- ") {
            if let Some(r) = out.last_mut() {
                r.note.push(' ');
                r.note.push_str(line.trim());
            }
        } else {
            in_note = false;
        }
    }
    out
}

/// How a row is changed, from the text its INI line holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `key=1` / `key=0`.
    Switch,
    /// A part of a full amount, `key=0.50`.
    Slider,
    /// Anything else: a list's choice by its own text (`resolution=1600x900;desktop`).
    Text,
}

impl Kind {
    pub fn of_value(v: &str) -> Kind {
        match v.trim() {
            "0" | "1" => Kind::Switch,
            t if t.contains('.') && t.parse::<f32>().is_ok() => Kind::Slider,
            _ => Kind::Text,
        }
    }
}

/// Sliders whose steps the page shows as words, not a percentage: the words for each step,
/// first to last (the step `i` of `n` is kept as `i / (n - 1)`, `Slider::choice` in
/// engine/nvf-play/src/settings.rs). Words are NVForever's own (the page's).
pub fn step_words() -> HashMap<&'static str, &'static [&'static str]> {
    // Difficulty was the last one; it is a list since the menu overhaul ([`list_choices`]).
    HashMap::new()
}

/// List rows whose choices are fixed, by their names as the INI keeps them (each list's
/// `name()` in the engine; a test checks the engine still has every name). Other lists
/// (the resolution, the monitor) depend on the machine and stay text.
pub fn list_choices() -> HashMap<&'static str, &'static [&'static str]> {
    HashMap::from([
        // fnv-game difficulty.rs `CHOICES` (the five levels, then Custom, 2026-11-22).
        ("difficulty", &["Very Easy", "Easy", "Normal", "Hard", "Very Hard", "Custom"][..]),
        ("kill-camera", &["Last enemy", "Every kill", "Off"][..]),
        ("vats-style", &["Classic", "Mark and Fire"][..]),
        // nvf_coop::prefs::Connection::name.
        ("coop-connection", &["Automatic", "Direct only", "LAN only"][..]),
        // As the INI keeps them (`Smoothing::stored`, #2676); the engine also reads its older 1/0.
        ("smooth-terrain", &["off", "2x2", "4x4", "4x4-near"][..]),
        // ssao.rs `AO_CHOICES` (2026-11-19); the engine also reads its older 1/0.
        ("ambient-occlusion", &["Off", "Performance", "Quality"][..]),
        // volumetric_clouds.rs `CLOUD_CHOICES` (2026-11-23); the engine also reads its older 1/0.
        ("volumetric-clouds", &["Off", "Performance", "Normal", "High"][..]),
    ])
}

/// A list row's value as one of its choices ([`list_choices`]): the older values a list took
/// over from map to the choice they stand for (the Smooth terrain row's on/off of #2149:
/// `1`, true, on, yes are `4x4`; `0`, false, off, no are `off`; the Ambient occlusion row's
/// switch of before 2026-11-19: `Quality` and `Off`, the Volumetric clouds row's of before
/// 2026-11-23: `Normal` and `Off`, as the engine reads them); anything else as it is (trimmed).
pub fn list_value(key: &str, value: &str) -> String {
    let v = value.trim();
    // The Difficulty slider's part of before the menu overhaul: the nearest of the five
    // levels, as the engine reads it (nvf-play difficulty.rs `take_level`).
    if key == "difficulty" {
        if let Some(p) = v.parse::<f32>().ok().filter(|p| p.is_finite()) {
            let i = (p.clamp(0.0, 1.0) * 4.0).round() as usize;
            return list_choices()["difficulty"][i].to_string();
        }
        return v.to_string();
    }
    let (on, off) = match key {
        "smooth-terrain" => ("4x4", "off"),
        "ambient-occlusion" => ("Quality", "Off"),
        "volumetric-clouds" => ("Normal", "Off"),
        _ => return v.to_string(),
    };
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => on.to_string(),
        "0" | "false" | "off" | "no" => off.to_string(),
        // A choice written by hand in another case, which the engine takes too.
        _ => list_choices()[key].iter().find(|c| c.eq_ignore_ascii_case(v)).unwrap_or(&v).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rows_tabs_headings_and_notes() {
        let t = "# S\n\n## Gameplay › Game\n\n### `difficulty`: Difficulty\n\n- On the page: How hard the\n  fights are.\n- In full: more\n  words.\n- Registered in x\n\n## Audio\n\n### `music`: Music\n\n- On the page: Plays it.\n\n## Controls › the tab's own rows\n\n### `x`: X\n";
        let r = parse(t);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], Row { key: "difficulty".into(), label: "Difficulty".into(), tab: "Gameplay".into(), heading: Some("Game".into()), note: "How hard the fights are.".into(), kind: None, default: None, unlisted: false, shown_with: None, needs: vec![] });
        assert_eq!((r[1].tab.as_str(), r[1].heading.as_deref(), r[1].note.as_str()), ("Audio", None, "Plays it."));
    }

    #[test]
    fn built_in_the_games_tabs_categories_and_order_with_the_references_words() {
        let c = Catalog::built_in();
        assert!(!c.from_game);
        assert_eq!(c.tab_names(), LAUNCHER_TABS, "the launcher's tabs, in the game's order");
        // Every row of those tabs where the game's table places it, in its order.
        let placed: Vec<(&str, &str)> = SECTIONS
            .iter()
            .filter(|s| LAUNCHER_TABS.contains(&s.placement.tab))
            .flat_map(|s| s.keys.iter().filter(|k| !k.starts_with(CONTROL_PREFIX)).map(move |k| (*k, s.placement.tab)))
            .collect();
        assert_eq!(c.rows.iter().map(|r| (r.key.as_str(), r.tab.as_str())).collect::<Vec<_>>(), placed);
        let at = |k: &str| c.rows.iter().find(|r| r.key == k).unwrap_or_else(|| panic!("{k} missing"));
        assert_eq!((at("menu-pause").tab.as_str(), at("menu-pause").heading.as_deref()), ("Gameplay", Some("Dialogue")));
        for key in ["difficulty", "menu-pause", "resolution", "borderless"] {
            let row = at(key);
            assert!(!row.unlisted && !row.note.is_empty() && !row.label.is_empty(), "{row:?}");
        }
        // A Tweaks row stays unlisted until the game's menu names it. The label is words, not the key.
        assert_eq!(at("dlss-model").shown_with.as_ref().map(|r| r.parent.as_str()), Some("upscaler"), "the game's rules built in too");
        for gone in ["volume-master", "aim-lock", "grass-fire", "god-mode"] {
            assert!(c.rows.iter().all(|r| r.key != gone), "{gone}: its tab is the game's only");
        }
        if let Some(t) = c.rows.iter().find(|r| r.tab == TWEAKS_TAB) {
            assert!(t.unlisted && t.heading.as_deref().is_some_and(|h| h.contains(" › ")), "{t:?}");
            assert!(!t.label.contains("tweak-") && t.label != t.key, "{t:?}");
        }
    }

    /// The launcher's tabs are the game's: each one still in its table, in its order.
    #[test]
    fn the_launchers_tabs_are_tabs_of_the_game() {
        let at: Vec<usize> = LAUNCHER_TABS.iter().map(|t| TAB_ORDER.iter().position(|o| o == t).unwrap_or_else(|| panic!("{t}: no longer a tab of the game"))).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "in the game's order");
        // Each has a row no rule hides, so the page never leaves one of them empty (only the
        // game's Controls tab can be, and the launcher does not show it).
        let c = Catalog::built_in();
        for t in LAUNCHER_TABS {
            assert!(c.rows.iter().any(|r| r.tab == t && r.shown_with.is_none()), "{t}: every row can be hidden");
        }
    }

    /// The launcher never shows a row the game's table does not place, nor in another tab or
    /// category than the game's [design: owner 2026-11-16].
    #[test]
    fn every_reference_row_is_where_the_game_places_it() {
        for r in parse(REFERENCE) {
            let s = SECTIONS.iter().find(|s| s.keys.contains(&r.key.as_str())).unwrap_or_else(|| panic!("{}: in the reference, not in the game's menu", r.key));
            assert_eq!((s.placement.tab, s.placement.heading), (r.tab.as_str(), r.heading.as_deref()), "{}: the reference's place is not the game's", r.key);
        }
    }

    fn game_menu(version: &str) -> Menu {
        use menu::{Category, Kind as K, Row as R, Tab};
        let row = |key: &str, kind: K| R { key: key.into(), label: key.to_uppercase(), note: "n".into(), kind, default: "1".into(), shown_with: None, needs: vec![] };
        Menu {
            version: version.into(),
            layout: nvf_settings_layout::layout_stamp(),
            tabs: vec![
                Tab { name: "Gameplay".into(), label: "Gameplay".into(), categories: vec![Category { group: None, heading: Some("Game".into()), rows: vec![row("hardcore", K::Switch)] }] },
                Tab {
                    name: TWEAKS_TAB.into(),
                    label: "Tweaks".into(),
                    categories: vec![Category { group: Some("Combat".into()), heading: Some("In Combat".into()), rows: vec![row("tweak-no-combat-music", K::Switch)] }],
                },
            ],
        }
    }

    #[test]
    fn the_games_menu_file_when_it_is_this_version() {
        let m = game_menu(env!("CARGO_PKG_VERSION"));
        let c = Catalog::from_menu(&m, env!("CARGO_PKG_VERSION")).unwrap();
        assert!(c.from_game);
        assert_eq!(c.tab_names(), ["Gameplay", TWEAKS_TAB]);
        // A tab the game keeps to itself is left out, with its rows.
        let mut more = game_menu(env!("CARGO_PKG_VERSION"));
        let mut audio = more.tabs[0].clone();
        audio.name = "Audio".into();
        audio.categories[0].rows[0].key = "volume-master".into();
        more.tabs.insert(1, audio);
        let c2 = Catalog::from_menu(&more, env!("CARGO_PKG_VERSION")).unwrap();
        assert_eq!((c2.tab_names(), c2.rows.len()), (vec!["Gameplay".to_string(), TWEAKS_TAB.to_string()], 2));
        let only_audio = Menu { tabs: vec![more.tabs[1].clone()], ..more.clone() };
        assert_eq!(Catalog::from_menu(&only_audio, env!("CARGO_PKG_VERSION")), None, "nothing the launcher shows: built in");
        assert_eq!(c.rows[1].heading.as_deref(), Some("Combat › In Combat"));
        assert_eq!((c.rows[1].label.as_str(), c.rows[1].kind.clone(), c.rows[1].default.as_deref()), ("TWEAK-NO-COMBAT-MUSIC", Some(menu::Kind::Switch), Some("1")));
        // The game's rules: from the file, else (a file without them) the shared table.
        let mut with_rules = game_menu(env!("CARGO_PKG_VERSION"));
        with_rules.tabs[0].categories[0].rows.push(menu::Row { key: "shadow-distance".into(), label: "S".into(), note: "n".into(), kind: menu::Kind::Switch, default: String::new(), shown_with: None, needs: vec![] });
        with_rules.tabs[0].categories[0].rows.push(menu::Row {
            key: "vats-slowdown".into(),
            label: "V".into(),
            note: "n".into(),
            kind: menu::Kind::Switch,
            default: String::new(),
            shown_with: None,
            needs: vec![menu::Rule { parent: "vats-style".into(), choices: vec!["Classic".into()], why: "Nur klassisch.".into() }],
        });
        let w = Catalog::from_menu(&with_rules, env!("CARGO_PKG_VERSION")).unwrap();
        let at = |k: &str| w.rows.iter().find(|r| r.key == k).unwrap().clone();
        assert_eq!(at("shadow-distance").needs.iter().map(|r| (r.parent.as_str(), r.why.as_str())).collect::<Vec<_>>(), [("sun-shadows", "Needs Sun Shadows.")]);
        assert_eq!(at("vats-slowdown").needs[0].why, "Nur klassisch.", "the file's words win");
        assert_eq!(at("hardcore").needs, vec![]);
        assert_eq!(at("shadow-distance").default, None, "no default in the file: not known");
        assert_eq!(Catalog::from_menu(&m, "0.0.1"), None, "another version's menu: built in");
        // The same version from a build of another table (dev builds share a version), or a
        // file from before the stamp: built in.
        for layout in ["0123456789abcdef", ""] {
            assert_eq!(Catalog::from_menu(&Menu { layout: layout.into(), ..m.clone() }, env!("CARGO_PKG_VERSION")), None, "{layout:?}");
        }
        let empty = Menu { tabs: vec![], ..m.clone() };
        assert_eq!(Catalog::from_menu(&empty, env!("CARGO_PKG_VERSION")), None, "no row: built in, not an empty page");
        let dir = std::env::temp_dir().join(format!("nvf-launcher-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ini = dir.join("NVForever.ini");
        assert!(!Catalog::load(Some(&ini)).from_game, "no file: built in");
        std::fs::write(dir.join(menu::MENU_FILE), m.to_text()).unwrap();
        assert_eq!(Catalog::load(Some(&ini)), c);
        std::fs::write(dir.join(menu::MENU_FILE), game_menu("0.0.1").to_text()).unwrap();
        assert!(!Catalog::load(Some(&ini)).from_game, "an older game's: built in");
        std::fs::write(dir.join(menu::MENU_FILE), "{").unwrap();
        assert!(!Catalog::load(Some(&ini)).from_game, "broken: built in");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_menu_from_another_build_keeps_this_table_and_uses_the_games_words() {
        let mut m = game_menu("0.0.1");
        m.tabs[0].categories[0].rows[0].label = "Hardcore Mode".into();
        m.tabs[0].categories[0].rows[0].note = "Needs food and sleep.".into();
        let dir = std::env::temp_dir().join(format!("nvf-launcher-menu-words-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ini = dir.join("NVForever.ini");
        std::fs::write(dir.join(menu::MENU_FILE), m.to_text()).unwrap();
        let c = Catalog::load(Some(&ini));
        assert!(!c.from_game, "another build: this launcher's rows");
        assert_eq!(c.tab_names(), LAUNCHER_TABS);
        let row = c.rows.iter().find(|r| r.key == "hardcore").unwrap();
        assert_eq!((row.label.as_str(), row.note.as_str()), ("Hardcore Mode", "Needs food and sleep."));
        assert!(row.kind.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unnamed_tweak_is_labelled_in_sentence_case_from_its_key() {
        assert_eq!(super::readable_tweak("tweak-aiming-draws-weapon"), "Aiming draws weapon");
        // Abbreviations keep their capitals wherever they are; "V.A.T.S." as the data spells it.
        assert_eq!(super::readable_tweak("tweak-npc-uses-vats-in-hud"), "NPC uses V.A.T.S. in HUD");
        assert_eq!(super::readable_tweak("tweak-ai--xp"), "AI XP");
    }

    #[test]
    fn list_choices_match_the_engines_names() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../engine/nvf-play/src/");
        for (key, file) in [("kill-camera", "kill_cam.rs"), ("vats-style", "vats_classic.rs"), ("smooth-terrain", "smooth_terrain.rs")] {
            let src = std::fs::read_to_string(format!("{root}{file}")).unwrap();
            for name in list_choices()[key] {
                assert!(src.contains(&format!("=> \"{name}\"")), "{key}: {name} is no longer a choice in {file}");
            }
        }
        // The Difficulty row's: the engine's level names (fnv-game difficulty.rs `name`).
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../fnv-game/src/difficulty.rs")).unwrap();
        for name in list_choices()["difficulty"] {
            assert!(src.contains(&format!("=> \"{name}\"")), "difficulty: {name} is no longer a level in fnv-game's difficulty.rs");
        }
        // The Ambient occlusion row's, as the engine lists them (ssao.rs `AO_CHOICES`).
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../engine/nvforever/src/ssao.rs")).unwrap();
        let listed = list_choices()["ambient-occlusion"].iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ");
        assert!(src.contains(&format!("pub const AO_CHOICES: [&str; 3] = [{listed}];")), "ambient-occlusion: the engine's AO_CHOICES are no longer {listed}");
        // The Volumetric clouds row's (volumetric_clouds.rs `CLOUD_CHOICES`).
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../engine/nvforever/src/volumetric_clouds.rs")).unwrap();
        let listed = list_choices()["volumetric-clouds"].iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ");
        assert!(src.contains(&format!("pub const CLOUD_CHOICES: [&str; 4] = [{listed}];")), "volumetric-clouds: the engine's CLOUD_CHOICES are no longer {listed}");
    }

    /// The Smooth terrain row's older on/off reads as its choices (4x4 and off), the Ambient
    /// occlusion row's as Quality and Off, the Volumetric clouds row's as Normal and Off, so a launcher with an old INI offers the list, not
    /// a switch; other values stay as they are.
    #[test]
    fn older_values_map_to_list_choices() {
        let choices = list_choices();
        for (v, want) in [("1", "4x4"), ("0", "off"), (" TRUE ", "4x4"), ("no", "off"), ("2x2", "2x2"), ("4x4-near", "4x4-near")] {
            let got = list_value("smooth-terrain", v);
            assert_eq!(got, want, "{v}");
            assert!(choices["smooth-terrain"].contains(&got.as_str()));
        }
        for (v, want) in [("1", "Quality"), ("0", "Off"), ("on", "Quality"), ("false", "Off"), ("Performance", "Performance"), (" Quality ", "Quality")] {
            let got = list_value("ambient-occlusion", v);
            assert_eq!(got, want, "{v}");
            assert!(choices["ambient-occlusion"].contains(&got.as_str()));
        }
        for (v, want) in [("1", "Normal"), ("0", "Off"), ("on", "Normal"), ("false", "Off"), ("Performance", "Performance"), (" High ", "High"), ("high", "High")] {
            let got = list_value("volumetric-clouds", v);
            assert_eq!(got, want, "{v}");
            assert!(choices["volumetric-clouds"].contains(&got.as_str()));
        }
        assert_eq!(list_value("volumetric-clouds", "ultra"), "ultra");
        assert_eq!(list_value("kill-camera", " 1 "), "1");
        for (v, want) in [("0.50", "Normal"), ("0", "Very Easy"), ("1.0", "Very Hard"), ("0.75", "Hard"), ("Custom", "Custom")] {
            assert_eq!(list_value("difficulty", v), want, "{v}");
        }
    }

    #[test]
    fn kinds_from_values() {
        assert_eq!(Kind::of_value("1"), Kind::Switch);
        assert_eq!(Kind::of_value("0.50"), Kind::Slider);
        assert_eq!(Kind::of_value("1920x1080;desktop"), Kind::Text);
        assert_eq!(Kind::of_value("2"), Kind::Text);
    }
}
