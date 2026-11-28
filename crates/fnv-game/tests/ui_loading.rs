//! Startup on files written for these tests, without Bevy or game data [design].

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{
    ui_loading::{self, UiFiles, ALWAYS_LOADED},
    Game, GameConfig, GameData,
};
use fnv_script::FunctionTable;
use fnv_sim::{settings::Setting, FormStore, RefTable, SimConfig, World};
use fnv_ui::{MemSource, Source, Value};
use fnv_vfs::Vfs;
use std::{path::Path, sync::Arc};

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let gmst = record(b"GMST", 0, 0x800, &[sub(b"EDID", z("sProbe")), sub(b"DATA", z("plugin text"))]);
    let plugins = vec![Plugin::parse(&[tes4(), top(b"GMST", &gmst)].concat(), "Empty.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &json, &refs);
    Fixture { data, forms, refs }
}

fn files() -> MemSource {
    let mut src = MemSource::default()
        .with("menus/globals.xml", r#"<rect name="Strings"><_Probe>17</_Probe></rect>"#)
        .with("menus/prefabs/probe.xml", r#"<text name="Label"><string>&-sProbe;</string></text></wrong>"#);
    src.strings.insert("sProbe".into(), "fixture text".into());
    for (i, name) in ALWAYS_LOADED.iter().enumerate() {
        let include = if i % 2 == 0 { "probe.xml" } else { "PROBE.XML" };
        src = src.with(
            &format!("menus/added/{i}.xml"),
            &format!(
                r#"<menu name="{name}">
            <class>&{name};</class><depth><copy src="globals()" trait="_Probe"/></depth><include src="{include}"/>
            </menu>"#
            ),
        );
    }
    src
}

// These menus and their trait values are written for the tests [design]. Class codes
// come from fnv_ui::value::MENU_CLASSES [documented: Hardcoded UI Shortcuts, revision 39903].
fn lifetime_files() -> MemSource {
    files()
        .with(
            "menus/added/trade.xml",
            r#"<menu name="Trade"><class>&BarterMenu;</class><depth>42</depth><rect name="Row"><depth><copy src="parent()" trait="depth"/></depth></rect></menu>"#,
        )
        .with("menus/added/box.xml", r#"<menu name="MessageMenu"><class>&MessageMenu;</class><depth>7</depth></menu>"#)
        .with("menus/added/start.xml", r#"<menu name="StartMenu"><class>&StartMenu;</class><depth>8</depth></menu>"#)
        .with("menus/added/dialog.xml", r#"<menu name="DialogMenu"><class>&DialogMenu;</class></menu>"#)
}

fn load_lifetime(g: &mut Game<'_>) {
    let src = lifetime_files();
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(g, Box::new(src), paths);
}

#[test]
fn uio_uses_active_plugins_and_survives_menu_lifetime() {
    // [design] Synthetic lists exercise the real load/sync route, without installed mods.
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src = lifetime_files()
        .with(
            "uio/public/fixture.txt",
            "added.xml::HUDMainMenu\n?EMPTY.ESM && #added.xml\nadded.xml::StartMenu\n?Empty.esm\nadded.xml::InventoryMenu\n?Disabled.esp",
        )
        .with_loose("menus/prefabs/added.xml", "<rect name=\"Added\"/>");
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    let report = ui_loading::load(&mut g, Box::new(src), paths);
    let hud = g.ui_tree.find_tile("HUDMainMenu/Added").unwrap();
    assert!(g.ui_tree.mod_tile_drawn(hud, None));
    assert!(g.ui_tree.find_tile("InventoryMenu/Added").is_none());
    assert!(report.warnings.values().flatten().any(|w| w.contains("condition false")));
    let report = ui_loading::sync(&mut g, true, false);
    assert!(g.ui_tree.find_tile("StartMenu/Added").is_some());
    assert!(report.changes.iter().any(|w| w.contains("UIO") && w.contains("applied")));
    ui_loading::sync(&mut g, false, false);
    assert!(g.ui_tree.find_tile("StartMenu/Added").is_none());
    ui_loading::sync(&mut g, true, false);
    assert!(g.ui_tree.find_tile("StartMenu/Added").is_some());
    assert_eq!(g.ui_tree.uio.decisions[1].applied, 2);
}

#[test]
fn uio_builtin_conditions_and_three_line_entries_follow_load_order_and_prefabs() {
    // [design] Vanilla control: with no UIO list files, loading is completely unchanged.
    let f_vanilla = fixture();
    let mut g_vanilla = Game::new(&f_vanilla.data, World::new(&f_vanilla.forms, &f_vanilla.refs, SimConfig::default()), GameConfig::default());
    let vanilla_src = lifetime_files();
    let vanilla_paths = vanilla_src.files.keys().cloned().collect::<Vec<_>>();
    let vanilla_report = ui_loading::load(&mut g_vanilla, Box::new(vanilla_src), vanilla_paths);
    assert_eq!((vanilla_report.menus, vanilla_report.tiles), (6, 12));
    assert!(g_vanilla.ui_tree.uio.decisions.is_empty());
    assert!(g_vanilla.ui_tree.uio.warnings.is_empty());

    // [design] Synthetic load order with `Vanilla UI Plus.esp` + `DarN_Fonts` exercises
    // `vui+`, `darn`, `darnui`, and 3-line public list entries through real `ui_loading::load`.
    let gmst = record(b"GMST", 0, 0x800, &[sub(b"EDID", z("sProbe")), sub(b"DATA", z("plugin text"))]);
    let plugins = vec![
        Plugin::parse(&[tes4(), top(b"GMST", &gmst)].concat(), "Empty.esm".into()).unwrap(),
        Plugin::parse(&tes4(), "Vanilla UI Plus.esp".into()).unwrap(),
    ];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &json, &refs);
    let mut g = Game::new(&data, World::new(&forms, &refs, SimConfig::default()), GameConfig::default());

    let src = lifetime_files()
        .with("menus/prefabs/DarN_Fonts/DarN_Fonts.xml", "<rect name=\"Fonts\"/>")
        .with_loose("menus/prefabs/vui_patch.xml", "<rect name=\"VuiPatch\"/>")
        .with_loose("menus/prefabs/darn_patch.xml", "<rect name=\"DarnPatch\"/>")
        .with_loose("menus/prefabs/vanilla_patch.xml", "<rect name=\"VanillaPatch\"/>")
        .with_loose("menus/prefabs/three_line.xml", "<rect name=\"ThreeLine\"/>")
        .with(
            "uio/public/builtin_and_three_line.txt",
            "vui_patch.xml::HUDMainMenu\nvui+\n\
             darn_patch.xml::HUDMainMenu\ndarn || darnui\n\
             vanilla_patch.xml::HUDMainMenu\n!vui+ && !darn\n\
             three_line.xml::InventoryMenu\n#vui_patch.xml\ntrue\n",
        );
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    let _report = ui_loading::load(&mut g, Box::new(src), paths);
    assert!(g.ui_tree.uio.warnings.is_empty(), "{:?}", g.ui_tree.uio.warnings);
    assert!(g.ui_tree.find_tile("HUDMainMenu/VuiPatch").is_some());
    assert!(g.ui_tree.find_tile("HUDMainMenu/DarnPatch").is_none(), "DarN_Fonts must not activate darn/darnui");
    assert!(g.ui_tree.find_tile("HUDMainMenu/VanillaPatch").is_none());
    assert!(g.ui_tree.find_tile("InventoryMenu/ThreeLine").is_some());
}

#[test]
fn open_close_and_inject_again() {
    use fnv_game::game::menus;
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    load_lifetime(&mut g);
    assert!(g.ui_tree.menu("Trade").is_none());
    g.menu = Some(menus::BARTER);
    let report = ui_loading::sync(&mut g, false, false);
    assert_eq!(report.changes, ["built BarterMenu (menus\\added\\trade.xml, 2 tiles)"]);
    assert!(report.warnings.is_empty());
    let root = g.ui_tree.menu("Trade").unwrap();
    assert_eq!(g.ui_tree.tile(root).get("depth"), Some(&Value::Num(42.0)));
    let row = g.ui_tree.find_tile("Trade/Row").unwrap();
    assert_eq!(g.ui_tree.tile(row).get("depth"), Some(&Value::Num(42.0)));
    assert!(ui_loading::sync(&mut g, false, false).changes.is_empty());
    g.ui_tree.inject_text(g.ui_files.as_deref().unwrap(), "Trade", r#"<rect name="Injected"><depth>9</depth></rect>"#);
    let injected = g.ui_tree.find_tile("Trade/Injected").unwrap();
    g.ui_tree.set_trait(root, "depth", Value::Num(99.0));
    g.menu = None;
    assert_eq!(ui_loading::sync(&mut g, false, false).changes, ["dropped BarterMenu"]);
    assert!(!g.ui_tree.tile(root).live);
    assert!(!g.ui_tree.tile(injected).live);
    assert!(g.ui_tree.menu("Trade").is_none());
    for name in ALWAYS_LOADED {
        assert!(g.ui_tree.menu(name).is_some());
    }
    g.menu = Some(menus::BARTER);
    assert_eq!(ui_loading::sync(&mut g, false, false).changes.len(), 1);
    let rebuilt = g.ui_tree.menu("Trade").unwrap();
    assert_ne!(rebuilt, root);
    assert_eq!(g.ui_tree.tile(rebuilt).get("depth"), Some(&Value::Num(42.0)));
    assert!(g.ui_tree.find_tile("Trade/Injected").is_none());
    // Persistent menus keep their injected content on close [design: §3.3].
    let hud = g.ui_tree.menu("HUDMainMenu").unwrap();
    g.ui_tree.inject_text(g.ui_files.as_deref().unwrap(), "InventoryMenu", r#"<rect name="Kept"/>"#);
    let kept = g.ui_tree.find_tile("InventoryMenu/Kept").unwrap();
    g.menu = Some(menus::INVENTORY);
    ui_loading::sync(&mut g, false, false);
    g.menu = None;
    ui_loading::sync(&mut g, false, false);
    assert_eq!(g.ui_tree.menu("HUDMainMenu"), Some(hud));
    assert!(g.ui_tree.menu("InventoryMenu").is_some());
    assert_eq!(g.ui_tree.find_tile("InventoryMenu/Kept"), Some(kept));
}

#[test]
fn start_menu_with_pause_or_main_menu() {
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    load_lifetime(&mut g);
    assert!(ui_loading::sync(&mut g, false, false).changes.is_empty());
    assert_eq!(ui_loading::sync(&mut g, true, false).changes, ["built StartMenu (menus\\added\\start.xml, 1 tiles)"]);
    let root = g.ui_tree.menu("StartMenu").unwrap();
    assert!(ui_loading::sync(&mut g, true, true).changes.is_empty());
    assert!(ui_loading::sync(&mut g, false, true).changes.is_empty());
    assert_eq!(g.ui_tree.menu("StartMenu"), Some(root));
    assert_eq!(ui_loading::sync(&mut g, false, false).changes, ["dropped StartMenu"]);
    assert_eq!(ui_loading::sync(&mut g, false, true).changes, ["built StartMenu (menus\\added\\start.xml, 1 tiles)"]);
    assert_ne!(g.ui_tree.menu("StartMenu"), Some(root));
    assert_eq!(ui_loading::sync(&mut g, false, false).changes, ["dropped StartMenu"]);
}

#[test]
fn underlying_menu_and_dialogue_stay_open() {
    use fnv_game::{dialogue::Conversation, game::menus};
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    load_lifetime(&mut g);
    g.menu = Some(menus::MESSAGE);
    g.menu_under = Some(menus::BARTER);
    g.dialogue = Some(Conversation { actor: fnv_sim::FormId::PLAYER_REF, through: None, line: None, choices: Vec::new(), topic_list: false, lines: 0, then: None, ending: false });
    assert_eq!(ui_loading::sync(&mut g, false, false).changes.len(), 3);
    g.menu = None;
    assert_eq!(ui_loading::sync(&mut g, false, false).changes, ["dropped MessageMenu"]);
    assert!(g.ui_tree.menu("Trade").is_some());
    assert!(g.ui_tree.menu("DialogMenu").is_some());
    g.menu_under = None;
    g.dialogue = None;
    assert_eq!(ui_loading::sync(&mut g, false, false).changes.len(), 2);
}

#[test]
fn the_engines_prompt_builds_over_the_open_menu_and_drops_with_it() {
    use fnv_game::game::menus;
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src = lifetime_files().with("menus/added/quantity.xml", r#"<menu name="QuantityMenu"><class>&QuantityMenu;</class></menu>"#);
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g, Box::new(src), paths);
    // T15a: the prompt is no `Game::menu`, yet its file builds while `ui_prompt` holds it.
    g.menu = Some(menus::BARTER);
    g.ui_prompt = Some(menus::QUANTITY);
    let changes = ui_loading::sync(&mut g, false, false).changes;
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert!(g.ui_tree.menu("QuantityMenu").is_some() && g.ui_tree.menu("Trade").is_some());
    assert!(!g.menu_open(menus::QUANTITY), "MenuMode does not report the prompt");
    g.ui_prompt = None;
    assert_eq!(ui_loading::sync(&mut g, false, false).changes, ["dropped QuantityMenu"]);
    assert!(g.ui_tree.menu("Trade").is_some());
}

#[test]
fn the_engines_vats_screen_builds_while_it_is_up() {
    use fnv_game::game::menus;
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src = lifetime_files().with("menus/added/vats.xml", r#"<menu name="VATSMenu"><class>&VATSMenu;</class></menu>"#);
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g, Box::new(src), paths);
    // VATS is no `Game::menu`: its file builds only while the engine's screen reads it.
    assert!(ui_loading::sync(&mut g, false, false).changes.is_empty());
    g.ui_vats = true;
    assert_eq!(ui_loading::sync(&mut g, false, false).changes, ["built VATSMenu (menus\\added\\vats.xml, 1 tiles)"]);
    assert!(g.ui_tree.menu("VATSMenu").is_some());
    assert!(!g.menu_open(menus::VATS), "the screen alone does not make MenuMode 1056 hold");
    assert!(ui_loading::sync(&mut g, false, false).changes.is_empty());
    g.ui_vats = false;
    assert_eq!(ui_loading::sync(&mut g, false, false).changes, ["dropped VATSMenu"]);
}

#[test]
fn missing_class_warns_once_and_side_codes_are_ignored() {
    use fnv_game::game::menus;
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    // A matching filename and menu name alone do not match a class [design].
    let src = lifetime_files().with("menus/quantity_menu.xml", r#"<menu name="QuantityMenu"/>"#);
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g, Box::new(src), paths);
    let quantity = fnv_ui::value::menu_class_number("QuantityMenu").unwrap();
    g.menu = Some(quantity);
    assert_eq!(ui_loading::sync(&mut g, false, false).warnings, ["QuantityMenu: no file with matching menu class"]);
    assert!(ui_loading::sync(&mut g, false, false).warnings.is_empty());
    g.menu = None;
    ui_loading::sync(&mut g, false, false);
    g.menu = Some(quantity);
    assert!(ui_loading::sync(&mut g, false, false).warnings.is_empty());
    assert!(g.ui_tree.menu("QuantityMenu").is_none());
    g.menu = Some(menus::LOCKPICK);
    assert_eq!(ui_loading::sync(&mut g, false, false).warnings.len(), 1);
    assert!(ui_loading::sync(&mut g, false, false).warnings.is_empty());
    for code in [menus::PIPBOY, menus::LOADING, menus::VATS] {
        g.menu = Some(code);
        let report = ui_loading::sync(&mut g, false, false);
        assert!(report.changes.is_empty());
        assert!(report.warnings.is_empty());
    }
    g.menu = None;
    g.loading = true;
    assert!(ui_loading::sync(&mut g, false, false).changes.is_empty());
    load_lifetime(&mut g);
    g.menu = Some(quantity);
    assert_eq!(ui_loading::sync(&mut g, false, false).warnings.len(), 1);
}

#[test]
fn reopened_menu_does_not_repeat_mends() {
    use fnv_game::game::menus;
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src = lifetime_files().with(
        "menus/added/trade.xml",
        r#"<menu name="Trade"><class>&BarterMenu;</class><include src="PROBE.XML"/><depth><copy>1</copy><bogus/></depth></menu>"#,
    );
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g, Box::new(src), paths);
    g.menu = Some(menus::BARTER);
    let first = ui_loading::sync(&mut g, false, false);
    assert_eq!(first.warnings.len(), 1);
    assert!(first.warnings[0].contains("<bogus> is no operator"));
    g.menu = None;
    ui_loading::sync(&mut g, false, false);
    g.menu = Some(menus::BARTER);
    let second = ui_loading::sync(&mut g, false, false);
    assert_eq!(second.changes.len(), 1);
    assert!(second.warnings.is_empty());
}

#[test]
fn startup_and_rebuild() {
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src = files();
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    let report = ui_loading::load(&mut g, Box::new(src), paths.clone());
    assert_eq!((report.menus, report.tiles), (6, 12));
    assert_eq!((report.game_strings, report.game_strings_unresolved), (1, 0));
    assert_eq!(report.warning_count(), 1);
    assert_eq!(report.warnings["menus\\prefabs\\probe.xml"].len(), 1);
    assert_eq!(g.ui_menus.path("hudmainmenu"), Some("menus\\added\\0.xml"));
    assert_eq!(g.ui_tree.tile(g.ui_tree.globals()).get("_Probe"), Some(&Value::Num(17.0)));
    let hud = g.ui_tree.menu("HUDMainMenu").unwrap();
    assert_eq!(g.ui_tree.tile(hud).get("depth"), Some(&Value::Num(17.0)));
    let label = g.ui_tree.find_tile("HUDMainMenu/Label").unwrap();
    assert_eq!(g.ui_tree.tile(label).get("string"), Some(&Value::Str("fixture text".into())));
    g.ui_tree.inject_text(g.ui_files.as_deref().unwrap(), "HUDMainMenu", r#"<rect name="Injected"/>"#);
    g.ui_taps.push(28);
    ui_loading::load(&mut g, Box::new(files()), paths);
    assert!(g.ui_tree.find_tile("HUDMainMenu/Injected").is_none());
    assert_eq!(g.ui_taps, [28]);
}

#[test]
fn engine_menus_read_game_strings_through_the_tree_source() {
    // T10 (menu-xml.md §2.4): the engine's own menus ask `ui_loading::game_string`: the
    // tree's source once loaded (the load order over NVForever's labels), the labels
    // alone before. Synthetic text for the source.
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    assert_eq!(ui_loading::game_string(&g, "sCloseButton"), fnv_ui::strings::built_in("sCloseButton").unwrap());
    assert_eq!(ui_loading::game_string(&g, "sNvfNowhere"), "");
    let mut src = files();
    src.strings.insert("sCloseButton".into(), "fixture close".into());
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g, Box::new(src), paths);
    assert_eq!(ui_loading::game_string(&g, "SCLOSEBUTTON"), "fixture close");
    assert_eq!(ui_loading::game_string(&g, "sProbe"), "fixture text");
    // A name the source lacks still reads the shipped label.
    assert_eq!(ui_loading::game_string(&g, "sNext"), fnv_ui::strings::built_in("sNext").unwrap());
    assert_eq!(ui_loading::game_string(&g, "sNvfNowhere"), "");
}

#[test]
fn missing_menu_and_globals_warn() {
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src = MemSource::default().with("menus/renamed.xml", r#"<menu name="HUDMainMenu"><depth>4</depth></menu>"#);
    let report = ui_loading::load(&mut g, Box::new(src), ["menus/renamed.xml".into()]);
    assert_eq!(report.menus, 1);
    assert_eq!(report.warning_count(), 6);
    assert_eq!((report.game_strings, report.game_strings_unresolved), (0, 0));
    assert_eq!(report.warnings["InventoryMenu"], ["menu not found"]);
    assert_eq!(report.warnings["menus\\globals.xml"], ["not found"]);
    assert!(g.ui_tree.menu("HUDMainMenu").is_some());
    assert!(g.ui_files.is_some());
}

#[test]
fn vfs_source_owns_strings_and_reads_files() {
    fn is_static_source<T: Source + Send + Sync + 'static>(_: &T) {}
    let f = fixture();
    let mut world = World::new(&f.forms, &f.refs, SimConfig::default());
    // Only these synthetic files under the repository's target directory [design].
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/ui-loading-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("Menus")).unwrap();
    let xml = r#"<menu name="HUDMainMenu"><text name="Label"><string>&-sProbe;</string></text><text name="Gone"><string>&-sAbsent;</string></text></menu>"#;
    std::fs::write(dir.join("Menus/renamed.xml"), xml).unwrap();
    let vfs = Arc::new(Vfs::open(&dir, &[]).unwrap());
    let src = UiFiles::new(vfs.clone(), &world);
    is_static_source(&src);
    assert_eq!(src.menu_paths(), ["menus\\renamed.xml"]);
    assert_eq!(src.list("MENUS"), ["menus\\renamed.xml"]);
    assert_eq!(src.read("MENUS/RENAMED.XML").unwrap(), xml.as_bytes());
    assert!(src.is_loose("MENUS/RENAMED.XML"));
    assert!(!src.is_loose("menus/missing.xml"));
    assert_eq!(src.game_string("SPROBE").as_deref(), Some("plugin text"));
    world.change_setting("sProbe", Setting::Str("changed".into())).unwrap();
    assert_eq!(src.game_string("sProbe").as_deref(), Some("plugin text"));
    let next = UiFiles::new(vfs, &world);
    drop(world);
    assert_eq!(next.game_string("sProbe").as_deref(), Some("changed"));
    assert_eq!(src.game_string("sAbsent"), None);
    let mut ui = fnv_ui::Ui::new();
    let root = ui.load_menu(&src, "menus/renamed.xml").unwrap();
    assert_eq!(ui.mod_added_tiles(root), [root]);
    let label = ui.find_tile("HUDMainMenu/Label").unwrap();
    assert_eq!(ui.tile(label).get("string"), Some(&Value::Str("plugin text".into())));
    // T10: the load order's record answers; a name with no record and no built-in label
    // reads empty (menu-xml.md §2.4).
    assert_eq!(src.game_strings().lookup("sprobe"), Some(("plugin text", fnv_ui::strings::Origin::LoadOrder)));
    assert_eq!(ui.get_string("HUDMainMenu/Gone/string").as_deref(), Some(""));
    assert_eq!(ui.game_strings_read.get("sabsent"), Some(&false));
    assert_eq!(ui.game_strings_read.get("sprobe"), Some(&true));
    std::fs::remove_file(dir.join("Menus/renamed.xml")).unwrap();
    std::fs::remove_dir(dir.join("Menus")).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn menu_search_bar_tiles_are_gated_by_b_menu_search_tweak() {
    use fnv_game::{
        game::{menus, RunCtx},
        tweaks::Tweaks,
        ui_search_bar,
    };
    use fnv_script::vm::XVal;

    let f = fixture();
    let src_with_container = || {
        lifetime_files().with(
            "menus/added/container.xml",
            r#"<menu name="ContainerMenu"><class>&ContainerMenu;</class></menu>"#,
        )
    };

    // Default / bMenuSearch=0: zero search-bar tiles injected on load or sync; vanilla tile counts stay unchanged.
    let mut g_off = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    assert!(!ui_search_bar::menu_search_enabled(&g_off));
    assert!(g_off.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", b"[Tweaks]\nbMenuSearch=0\n"));
    assert!(!ui_search_bar::menu_search_enabled(&g_off));
    let src = src_with_container();
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    let report_off = ui_loading::load(&mut g_off, Box::new(src), paths);
    assert_eq!((report_off.menus, report_off.tiles), (6, 12));
    for path in ["InventoryMenu\\IM_SearchBar", "StatsMenu\\SM_SearchBar", "MapMenu\\MM_SearchBar"] {
        let loaded = g_off
            .run_query(&format!("Begin Function {{ }}\nSetFunctionValue (IsComponentLoaded \"{path}\")\nEnd\n"), RunCtx::default())
            .unwrap();
        assert_eq!(loaded, XVal::Num(0.0), "{path} must not be injected when bMenuSearch=0");
    }
    g_off.menu = Some(menus::BARTER);
    let barter_off = ui_loading::sync(&mut g_off, false, false);
    assert_eq!(barter_off.changes, ["built BarterMenu (menus\\added\\trade.xml, 2 tiles)"]);
    assert!(g_off.ui_tree.find_tile("Trade/BM_SearchBar").is_none());
    g_off.menu = Some(1008);
    ui_loading::sync(&mut g_off, false, false);
    assert!(g_off.ui_tree.find_tile("ContainerMenu/CM_SearchBar").is_none());

    // Unbound INI bMenuSearch=1 enables search-bar tile publication on both load and sync.
    let mut g_ini = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    assert!(g_ini.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", b"[Tweaks]\nbMenuSearch=1\n"));
    assert!(ui_search_bar::menu_search_enabled(&g_ini));
    let src = src_with_container();
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    let report_ini = ui_loading::load(&mut g_ini, Box::new(src), paths);
    assert_eq!((report_ini.menus, report_ini.tiles), (6, 15));
    assert!(g_ini.ui_tree.find_tile("InventoryMenu/IM_SearchBar").is_some());
    assert!(g_ini.ui_tree.find_tile("StatsMenu/SM_SearchBar").is_some());
    assert!(g_ini.ui_tree.find_tile("MapMenu/MM_SearchBar").is_some());
    g_ini.menu = Some(menus::BARTER);
    let barter_on = ui_loading::sync(&mut g_ini, false, false);
    assert_eq!(barter_on.changes, ["built BarterMenu (menus\\added\\trade.xml, 3 tiles)"]);
    assert!(g_ini.ui_tree.find_tile("Trade/BM_SearchBar").is_some());

    // Bound engine settings take authority over a stale INI file: menu_search=false suppresses injection,
    // and menu_search=true enables it without needing an INI file.
    let mut g_bound = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    assert!(g_bound.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", b"[Tweaks]\nbMenuSearch=1\n"));
    g_bound.bind_stewie_stat_settings(&Tweaks { menu_search: false, ..Default::default() });
    assert!(!ui_search_bar::menu_search_enabled(&g_bound));
    let src = src_with_container();
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    let report_bound_off = ui_loading::load(&mut g_bound, Box::new(src), paths);
    assert_eq!((report_bound_off.menus, report_bound_off.tiles), (6, 12));
    assert!(g_bound.ui_tree.find_tile("InventoryMenu/IM_SearchBar").is_none());

    g_bound.bind_stewie_stat_settings(&Tweaks { menu_search: true, ..Default::default() });
    assert!(ui_search_bar::menu_search_enabled(&g_bound));
    let src = src_with_container();
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    let report_bound_on = ui_loading::load(&mut g_bound, Box::new(src), paths);
    assert_eq!((report_bound_on.menus, report_bound_on.tiles), (6, 15));
    assert!(g_bound.ui_tree.find_tile("InventoryMenu/IM_SearchBar").is_some());
}

#[test]
fn menu_search_bar_tiles_publish_builtin_and_script_offset_copy() {
    use fnv_game::{
        game::{menus, RunCtx},
        ui_search_bar,
    };
    use fnv_script::vm::XVal;

    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    g.tweaks.menu_search = true;
    let src = lifetime_files().with(
        "menus/added/container.xml",
        r#"<menu name="ContainerMenu"><class>&ContainerMenu;</class></menu>"#,
    );
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g, Box::new(src), paths);

    // Stewie plugin gate promoted to 960 (#3183) with search-bar tile publication live.
    let ver = g
        .run_query("Begin Function { }\nSetFunctionValue (GetPluginVersion \"lStewieAl's Tweaks\")\nEnd\n", RunCtx::default())
        .unwrap();
    assert_eq!(ver, XVal::Num(960.0));

    // Persistent Pip-Boy search-bar tiles are published at load and script-addressable.
    for path in ["InventoryMenu\\IM_SearchBar", "StatsMenu\\SM_SearchBar", "MapMenu\\MM_SearchBar"] {
        let loaded = g
            .run_query(&format!("Begin Function {{ }}\nSetFunctionValue (IsComponentLoaded \"{path}\")\nEnd\n"), RunCtx::default())
            .unwrap();
        assert_eq!(loaded, XVal::Num(1.0), "{path} should be loaded");
    }

    // Exercise the Pip-Boy UI Tweaks offset propagation pattern across IM/SM/MM_SearchBar.
    g.run_query(
        r#"Begin Function { }
SetUIFloatAlt "InventoryMenu\IM_SearchBar\_OffsetX" 24
SetUIFloatAlt "InventoryMenu\IM_SearchBar\_OffsetY" -8
SetUIFloatAlt "StatsMenu\SM_SearchBar\_OffsetX" (GetUIFloatAlt "InventoryMenu\IM_SearchBar\_OffsetX")
SetUIFloatAlt "StatsMenu\SM_SearchBar\_OffsetY" (GetUIFloatAlt "InventoryMenu\IM_SearchBar\_OffsetY")
SetUIFloatAlt "MapMenu\MM_SearchBar\_OffsetX" (GetUIFloatAlt "InventoryMenu\IM_SearchBar\_OffsetX")
SetUIFloatAlt "MapMenu\MM_SearchBar\_OffsetY" (GetUIFloatAlt "InventoryMenu\IM_SearchBar\_OffsetY")
End"#,
        RunCtx::default(),
    )
    .unwrap();
    assert_eq!(ui_search_bar::read_offsets(&g.ui_tree, menus::INVENTORY), Some((24.0, -8.0)));
    assert_eq!(ui_search_bar::read_offsets(&g.ui_tree, menus::STATS), Some((24.0, -8.0)));
    assert_eq!(ui_search_bar::read_offsets(&g.ui_tree, menus::MAP), Some((24.0, -8.0)));

    // Query text and visibility publish cleanly via write_query and read back via GetUIString/GetUIFloatAlt.
    assert!(ui_search_bar::write_query(&mut g.ui_tree, menus::INVENTORY, true, "med-x"));
    let text = g
        .run_query("Begin Function { }\nstring_var s\nlet s := GetUIString \"InventoryMenu\\IM_SearchBar\\string\"\nSetFunctionValue s\nEnd\n", RunCtx::default())
        .unwrap();
    assert_eq!(text, XVal::Str("med-x".into()));

    // Transient menus publish their search-bar tile on sync open and clean up on close.
    g.menu = Some(1008);
    ui_loading::sync(&mut g, false, false);
    assert!(g.ui_tree.find_tile("ContainerMenu/CM_SearchBar").is_some());
    assert!(ui_search_bar::write_query(&mut g.ui_tree, 1008, true, "scrap"));
    assert_eq!(g.ui_tree.get_string("ContainerMenu/CM_SearchBar/string").as_deref(), Some("scrap"));
    g.menu = None;
    ui_loading::sync(&mut g, false, false);
    assert!(g.ui_tree.find_tile("ContainerMenu/CM_SearchBar").is_none());
}

#[test]
fn menu_search_bar_vfs_prefab_takes_precedence_and_preserves_siblings() {
    use fnv_game::{
        game::{menus, RunCtx},
        ui_search_bar,
    };

    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    g.tweaks.menu_search = true;
    let src = files().with(
        "menus/prefabs/lStewieAl/MenuSearch/Inventory.xml",
        r#"<text name="IM_SearchBar">
            <_OffsetX><copy>12</copy><add>6</add></_OffsetX>
            <_OffsetY>-4</_OffsetY>
            <x>100</x>
            <y>20</y>
            <visible>0</visible>
        </text>
        <image name="SearchIcon">
            <visible><copy src="sibling(IM_SearchBar)" trait="visible"/></visible>
            <x><copy src="sibling(IM_SearchBar)" trait="x"/><sub src="me()" trait="width"/></x>
            <width>16</width>
        </image>"#,
    );
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g, Box::new(src), paths);

    // Evaluated expression from the VFS prefab propagates through scripts to StatsMenu and MapMenu.
    g.run_query(
        r#"Begin Function { }
SetUIFloatAlt "StatsMenu\SM_SearchBar\_OffsetX" (GetUIFloatAlt "InventoryMenu\IM_SearchBar\_OffsetX")
SetUIFloatAlt "StatsMenu\SM_SearchBar\_OffsetY" (GetUIFloatAlt "InventoryMenu\IM_SearchBar\_OffsetY")
SetUIFloatAlt "MapMenu\MM_SearchBar\_OffsetX" (GetUIFloatAlt "InventoryMenu\IM_SearchBar\_OffsetX")
SetUIFloatAlt "MapMenu\MM_SearchBar\_OffsetY" (GetUIFloatAlt "InventoryMenu\IM_SearchBar\_OffsetY")
End"#,
        RunCtx::default(),
    )
    .unwrap();
    assert_eq!(ui_search_bar::read_offsets(&g.ui_tree, menus::INVENTORY), Some((18.0, -4.0)));
    assert_eq!(ui_search_bar::read_offsets(&g.ui_tree, menus::STATS), Some((18.0, -4.0)));
    assert_eq!(ui_search_bar::read_offsets(&g.ui_tree, menus::MAP), Some((18.0, -4.0)));

    // Sibling icon tile from the VFS prefab reacts to write_query visibility updates.
    assert_eq!(g.ui_tree.get_float("InventoryMenu/SearchIcon/visible"), Some(0.0));
    assert_eq!(g.ui_tree.get_float("InventoryMenu/SearchIcon/x"), Some(84.0));
    assert!(ui_search_bar::write_query(&mut g.ui_tree, menus::INVENTORY, true, "nuka"));
    g.ui_tree.update();
    assert_eq!(g.ui_tree.get_float("InventoryMenu/SearchIcon/visible"), Some(1.0));
}

#[test]
fn book_menu_variants_keep_canonical_xml_and_open_cleanly() {
    use fnv_game::game::{menus, RunCtx};
    use fnv_script::vm::XVal;

    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src = files()
        .with(
            "menus/book_menu.xml",
            r#"<menu name="BookMenu"><class>&BookMenu;</class><user0>1</user0><text name="BookBody"><string>canonical</string></text></menu>"#,
        )
        .with(
            "menus/alt_book.xml",
            r#"<menu name="BookMenu"><class>&BookMenu;</class><user0>9</user0><text name="BookBody"><string>alt</string></text></menu>"#,
        )
        .with(
            "menus/BookBackground1.xml",
            r#"<menu name="BookMenu"><class>&BookMenu;</class><user0>2</user0><text name="BookBody"><string>bg1</string></text></menu>"#,
        )
        .with(
            "menus/BookBackground4.xml",
            r#"<menu name="BookMenu"><class>&BookMenu;</class><user0>5</user0><text name="BookBody"><string>bg4</string></text></menu>"#,
        );
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    let report = ui_loading::load(&mut g, Box::new(src), paths);

    // Zero duplicate-menu or duplicate-class warnings (only the fixture's intentional probe.xml mend).
    assert_eq!(report.warning_count(), 1);
    assert!(report.warnings.values().flatten().all(|w| !w.contains("duplicate")));
    assert_eq!(g.ui_menus.path("BookMenu"), Some(fnv_ui::index::CANONICAL_BOOK_MENU));
    assert_eq!(g.ui_menus.code_path(menus::BOOK), Some(fnv_ui::index::CANONICAL_BOOK_MENU));
    assert_eq!(
        g.ui_menus.variants("BookMenu"),
        ["menus\\alt_book.xml", "menus\\bookbackground1.xml", "menus\\bookbackground4.xml"]
    );

    // Opening BookMenu (1026) loads canonical `menus\book_menu.xml` and drops cleanly on close.
    g.open_menu(menus::BOOK);
    let open = ui_loading::sync(&mut g, false, false);
    assert_eq!(open.changes, ["built BookMenu (menus\\book_menu.xml, 2 tiles)"]);
    assert!(open.warnings.is_empty());
    let mode = g
        .run_query("Begin Function { }\nSetFunctionValue (MenuMode 1026)\nEnd\n", RunCtx::default())
        .unwrap();
    assert_eq!(mode, XVal::Num(1.0));
    let body = g
        .run_query("Begin Function { }\nSetFunctionValue (GetUIString \"BookMenu\\BookBody\\string\")\nEnd\n", RunCtx::default())
        .unwrap();
    assert_eq!(body, XVal::Str("canonical".into()));
    let user0 = g
        .run_query("Begin Function { }\nSetFunctionValue (GetUIFloat \"BookMenu\\user0\")\nEnd\n", RunCtx::default())
        .unwrap();
    assert_eq!(user0, XVal::Num(1.0));

    g.close_menu();
    let close = ui_loading::sync(&mut g, false, false);
    assert_eq!(close.changes, ["dropped BookMenu"]);
    assert!(g.ui_tree.menu("BookMenu").is_none());
}

#[test]
fn text_input_menu_loads_plugin_xml_and_syncs_traits_without_affecting_vanilla() {
    use fnv_game::game::{menus, RunCtx};
    use fnv_script::vm::XVal;

    const VANILLA_TEXT_EDIT: &str = r#"<menu name="TextEditMenu">
        <class>&TextEditMenu;</class>
        <rect name="TEM_MainRect">
            <text name="textedit_prompt"><string>Enter Name</string></text>
            <text name="textedit_text"><string></string></text>
            <hotrect name="textedit_button_ok"/>
        </rect>
    </menu>"#;

    const JIP_TEXT_INPUT: &str = r#"<menu name="TextEditMenu">
        <class>&TextEditMenu;</class>
        <user0>400</user0>
        <user1>200</user1>
        <user2>0</user2>
        <user3>0</user3>
        <rect name="NOGLOW_BRANCH">
            <width><copy src="parent()" trait="user0"/></width>
            <height><copy src="parent()" trait="user1"/></height>
            <x><copy src="parent()" trait="user2"/></x>
            <y><copy src="parent()" trait="user3"/></y>
            <text name="TEM_TitleText"><string></string></text>
            <hotrect name="TEM_TextInputBox">
                <text name="TEM_InputText"><string></string></text>
            </hotrect>
            <hotrect name="TEM_OKButton"/>
        </rect>
    </menu>"#;

    let f = fixture();

    // 1. Vanilla GetPlayerName/ShowNameMenu with plugin XML present still loads vanilla
    //    `menus\dialog\texteditmenu.xml` with zero extra traits.
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src = files()
        .with("menus/dialog/texteditmenu.xml", VANILLA_TEXT_EDIT)
        .with("NVSE/Plugins/textinput/texteditmenu.xml", JIP_TEXT_INPUT);
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    let load_report = ui_loading::load(&mut g, Box::new(src), paths);
    assert_eq!(load_report.warning_count(), 1);
    assert_eq!(g.ui_menus.code_path(menus::NAME), Some("menus\\dialog\\texteditmenu.xml"));

    g.open_menu(menus::NAME);
    let vanilla_open = ui_loading::sync(&mut g, false, false);
    assert_eq!(vanilla_open.changes, ["built TextEditMenu (menus\\dialog\\texteditmenu.xml, 5 tiles)"]);
    assert!(g.ui_tree.find_tile("TextEditMenu/TEM_MainRect/textedit_prompt").is_some());
    assert!(g.ui_tree.find_tile("TextEditMenu/NOGLOW_BRANCH/TEM_TitleText").is_none());
    assert_eq!(g.ui_tree.get_string("TextEditMenu/TEM_MainRect/textedit_prompt/string").as_deref(), Some("Enter Name"));
    assert_eq!(g.ui_tree.get_float("TextEditMenu/user0"), None);
    g.close_menu();
    assert_eq!(ui_loading::sync(&mut g, false, false).changes, ["dropped TextEditMenu"]);

    // 2. JIP ShowTextInputMenu prefers `nvse\plugins\textinput\texteditmenu.xml` and syncs
    //    size (user0/user1), offsets (user2/user3), title, and input text.
    let opened = g
        .run_query(
            "Begin Function { }\nSetFunctionValue (ShowTextInputMenu (Begin Function {string_var s}\nEnd) 640 320 \"Filter %g\" 2)\nEnd\n",
            RunCtx::default(),
        )
        .unwrap();
    assert_eq!(opened, XVal::Num(1.0));
    assert_eq!(g.menu, Some(menus::NAME));
    let jip_open = ui_loading::sync(&mut g, false, false);
    assert_eq!(jip_open.changes, ["built TextEditMenu (nvse\\plugins\\textinput\\texteditmenu.xml, 6 tiles)"]);
    assert!(jip_open.warnings.is_empty());

    assert_eq!(
        g.run_query("Begin Function { }\nSetFunctionValue (MenuMode 1051)\nEnd\n", RunCtx::default()).unwrap(),
        XVal::Num(1.0)
    );
    assert_eq!(
        g.run_query("Begin Function { }\nSetFunctionValue (IsComponentLoaded \"TextEditMenu\\NOGLOW_BRANCH\\TEM_OKButton\")\nEnd\n", RunCtx::default()).unwrap(),
        XVal::Num(1.0)
    );
    assert_eq!(
        g.run_query("Begin Function { }\nSetFunctionValue (GetUIFloat \"TextEditMenu\\NOGLOW_BRANCH\\width\")\nEnd\n", RunCtx::default()).unwrap(),
        XVal::Num(640.0)
    );
    assert_eq!(
        g.run_query("Begin Function { }\nSetFunctionValue (GetUIFloat \"TextEditMenu\\NOGLOW_BRANCH\\height\")\nEnd\n", RunCtx::default()).unwrap(),
        XVal::Num(320.0)
    );
    assert_eq!(
        g.run_query("Begin Function { }\nSetFunctionValue (GetUIString \"TextEditMenu\\NOGLOW_BRANCH\\TEM_TitleText\\string\")\nEnd\n", RunCtx::default()).unwrap(),
        XVal::Str("Filter 2".into())
    );

    // SetTextInputExtendedProps and SetTextInputString immediately update the loaded tree.
    g.run_query("Begin Function { }\nSetTextInputExtendedProps 120 80 1 8 2\nSetTextInputString \"HelloWorld\"\nEnd\n", RunCtx::default())
        .unwrap();
    assert_eq!(g.ui_tree.get_float("TextEditMenu/NOGLOW_BRANCH/x"), Some(120.0));
    assert_eq!(g.ui_tree.get_float("TextEditMenu/NOGLOW_BRANCH/y"), Some(80.0));
    assert_eq!(
        g.run_query("Begin Function { }\nSetFunctionValue (GetUIString \"TextEditMenu\\NOGLOW_BRANCH\\TEM_TextInputBox\\TEM_InputText\\string\")\nEnd\n", RunCtx::default()).unwrap(),
        XVal::Str("HelloWor".into())
    );

    // Live typing from the engine updates TEM_InputText without clobbering script SetUIFloat edits.
    g.run_query("Begin Function { }\nSetUIFloat \"TextEditMenu\\user0\" 777\nEnd\n", RunCtx::default()).unwrap();
    g.ui_tree.update();
    g.set_text_input_live_text("Typed");
    assert_eq!(g.ui_tree.get_float("TextEditMenu/NOGLOW_BRANCH/width"), Some(777.0));
    assert_eq!(
        g.ui_tree.get_string("TextEditMenu/NOGLOW_BRANCH/TEM_TextInputBox/TEM_InputText/string").as_deref(),
        Some("Typed")
    );

    // Closing drops TextEditMenu cleanly.
    g.finish_text_input("Typed".into());
    assert_eq!(ui_loading::sync(&mut g, false, false).changes, ["dropped TextEditMenu"]);
    assert!(g.ui_tree.menu("TextEditMenu").is_none());

    // 3. Fallback when `NVSE/Plugins/textinput/texteditmenu.xml` is absent: uses vanilla
    //    `menus\dialog\texteditmenu.xml` and syncs prompt/text without inventing user0..user3.
    let mut g_fb = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src_fb = files().with("menus/dialog/texteditmenu.xml", VANILLA_TEXT_EDIT);
    let paths_fb = src_fb.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g_fb, Box::new(src_fb), paths_fb);
    g_fb.run_query(
        "Begin Function { }\nShowTextInputMenu (Begin Function {string_var s}\nEnd) 500 250 \"Fallback\"\nSetTextInputString \"Seed\"\nEnd\n",
        RunCtx::default(),
    )
    .unwrap();
    let fb_open = ui_loading::sync(&mut g_fb, false, false);
    assert_eq!(fb_open.changes, ["built TextEditMenu (menus\\dialog\\texteditmenu.xml, 5 tiles)"]);
    assert_eq!(g_fb.ui_tree.get_string("TextEditMenu/TEM_MainRect/textedit_prompt/string").as_deref(), Some("Fallback"));
    assert_eq!(g_fb.ui_tree.get_string("TextEditMenu/TEM_MainRect/textedit_text/string").as_deref(), Some("Seed"));
    assert_eq!(g_fb.ui_tree.get_float("TextEditMenu/user0"), None);

    // 4. Independent unavailable tracking: missing vanilla `texteditmenu.xml` does not block
    //    a later `ShowTextInputMenu` when `NVSE/Plugins/textinput/texteditmenu.xml` exists.
    let mut g_only_jip = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src_only_jip = files().with("NVSE/Plugins/textinput/texteditmenu.xml", JIP_TEXT_INPUT);
    let paths_only_jip = src_only_jip.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g_only_jip, Box::new(src_only_jip), paths_only_jip);
    g_only_jip.open_menu(menus::NAME);
    assert_eq!(ui_loading::sync(&mut g_only_jip, false, false).warnings, ["TextEditMenu: no file with matching menu class"]);
    g_only_jip.close_menu();
    ui_loading::sync(&mut g_only_jip, false, false);
    g_only_jip
        .run_query(
            "Begin Function { }\nShowTextInputMenu (Begin Function {string_var s}\nEnd) 0 0 \"PluginOnly\"\nEnd\n",
            RunCtx::default(),
        )
        .unwrap();
    let plugin_only = ui_loading::sync(&mut g_only_jip, false, false);
    assert_eq!(plugin_only.changes, ["built TextEditMenu (nvse\\plugins\\textinput\\texteditmenu.xml, 6 tiles)"]);
    assert!(plugin_only.warnings.is_empty());
    // Zero dimensions preserve XML defaults (400x200).
    assert_eq!(g_only_jip.ui_tree.get_float("TextEditMenu/NOGLOW_BRANCH/width"), Some(400.0));
    assert_eq!(g_only_jip.ui_tree.get_float("TextEditMenu/NOGLOW_BRANCH/height"), Some(200.0));

    // 5. Direct switch between vanilla TextEditMenu and ShowTextInputMenu without an
    //    intervening closed frame replaces the stale root cleanly.
    let mut g_sw = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    let src_sw = files()
        .with("menus/dialog/texteditmenu.xml", VANILLA_TEXT_EDIT)
        .with("NVSE/Plugins/textinput/texteditmenu.xml", JIP_TEXT_INPUT);
    let paths_sw = src_sw.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g_sw, Box::new(src_sw), paths_sw);
    g_sw.open_menu(menus::NAME);
    ui_loading::sync(&mut g_sw, false, false);
    assert!(g_sw.ui_tree.find_tile("TextEditMenu/TEM_MainRect").is_some());
    // Simulate ShowTextInputMenu replacing the active TextEditMenu while menu 1051 stays open.
    g_sw.other.jip12.text_input = Some(fnv_game::nvse_jip12::TextInput {
        callback: XVal::Num(0.0),
        title: "Switched".into(),
        size: (520.0, 260.0),
        offset: (10.0, 20.0),
        min: 1,
        max: 32,
        digits_only: false,
        enter_closes: false,
        text: "Now".into(),
        set_text: None,
    });
    let sw_report = ui_loading::sync(&mut g_sw, false, false);
    assert_eq!(
        sw_report.changes,
        ["dropped TextEditMenu", "built TextEditMenu (nvse\\plugins\\textinput\\texteditmenu.xml, 6 tiles)"]
    );
    assert!(g_sw.ui_tree.find_tile("TextEditMenu/TEM_MainRect").is_none());
    assert_eq!(g_sw.ui_tree.get_float("TextEditMenu/NOGLOW_BRANCH/width"), Some(520.0));
    assert_eq!(
        g_sw.ui_tree.get_string("TextEditMenu/NOGLOW_BRANCH/TEM_TitleText/string").as_deref(),
        Some("Switched")
    );
    assert_eq!(
        g_sw.ui_tree.get_string("TextEditMenu/NOGLOW_BRANCH/TEM_TextInputBox/TEM_InputText/string").as_deref(),
        Some("Now")
    );
}
