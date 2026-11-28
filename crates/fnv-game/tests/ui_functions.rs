//! The UI script functions over the tile tree (fnv_game::ui_functions), on an empty
//! plugin and a menu file written for the test (no game data):
//! docs/design/ui-tile-tree.md §5.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig};
use fnv_game::ui_functions::{call, NOT_FOUND};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormStore, RefTable, SimConfig, World};
use fnv_ui::MemSource;
use serde_json::Value as Json;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&tes4(), "Empty.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

fn files() -> MemSource {
    MemSource::default()
        .with(
            "menus\\hud.xml",
            r#"<menu name="HUDMenu"> <rect name="Bars"> <width> 300 </width>
               <image name="Fill"> <width> <copy src="parent()" trait="width"/> <mul src="io()" trait="_Health"/> </width> </image>
               <template name="pip"> <image name="Pip"> <alpha> 255 </alpha> </image> </template>
               </rect> <text name="Label"> <string> HP </string> </text> <_Health> 1 </_Health> </menu>"#,
        )
        .with("menus\\prefabs\\mod\\gauge.xml", "<text name=\"ModText\"> <string> hello </string> </text>")
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig::default());
    let src = files();
    g.ui_tree.load_menu(&src, "menus\\hud.xml").unwrap();
    g.ui_tree.update();
    g.ui_files = Some(Box::new(src));
    g
}

fn s(v: &str) -> XVal {
    XVal::Str(v.to_string())
}

fn run(g: &mut Game, name: &str, args: &[XVal]) -> XVal {
    call(g, name, args).expect("a UI function").expect("no error")
}

fn n(v: XVal) -> f64 {
    match v {
        XVal::Num(x) => x,
        other => panic!("not a number: {other:?}"),
    }
}

#[test]
fn get_set_and_mod_by_path() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("HUDMenu/Bars/Fill/width")])), 300.0);
    assert_eq!(n(run(&mut g, "GetUIFloatAlt", &[s("HUDMenu/Bars/Nope/width")])), NOT_FOUND);
    assert_eq!(n(run(&mut g, "SetUIFloat", &[s("HUDMenu/_Health"), XVal::Num(0.5)])), 1.0);
    g.ui_tree.update();
    assert_eq!(n(run(&mut g, "getuifloat", &[s("HUDMenu\\Bars\\Fill\\width")])), 150.0);
    assert_eq!(n(run(&mut g, "ModUIFloat", &[s("HUDMenu/Bars/width"), XVal::Num(-100.0)])), 1.0);
    g.ui_tree.update();
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("HUDMenu/*/*:0/width")])), 100.0);
    assert_eq!(n(run(&mut g, "SetUIStringAlt", &[s("HUDMenu/Bars/Fill/string"), s("x")])), 0.0);
    assert!(call(&mut g, "GetPlayerName", &[]).is_none());
}

#[test]
fn injection_and_loading() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(run(&mut g, "IsComponentLoaded", &[s("HUDMenu/Bars/Fill")])), 1.0);
    assert_eq!(n(run(&mut g, "InjectUIXML", &[s("HUDMenu/Bars"), s("data\\menus\\prefabs\\mod\\gauge.xml")])), 1.0);
    assert_eq!(run(&mut g, "GetUIString", &[s("HUDMenu/Bars/ModText/string")]), s("hello"));
    let comp = "<rect name=\"Added\"> <x> 4 </x> </rect>";
    assert_eq!(n(run(&mut g, "InjectUIComponent", &[s("HUDMenu"), s(comp)])), 1.0);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("HUDMenu/Added/x")])), 4.0);
    assert_eq!(n(run(&mut g, "AddTileFromTemplate", &[s("HUDMenu/Bars|pip|Pip0")])), 1.0);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("HUDMenu/Bars/Pip0/alpha")])), 255.0);
    assert_eq!(n(run(&mut g, "UnloadUIComponent", &[s("HUDMenu/Added")])), 1.0);
    assert_eq!(n(run(&mut g, "IsComponentLoaded", &[s("HUDMenu/Added")])), 0.0);
    run(&mut g, "MenuTapKey", &[XVal::Num(28.0)]);
    assert_eq!(g.ui_taps, [28]);
    // Headless: no files, so InjectUIXML fails.
    g.ui_files = None;
    assert_eq!(n(run(&mut g, "InjectUIXML", &[s("HUDMenu"), s("data\\menus\\prefabs\\mod\\gauge.xml")])), 0.0);
}

#[test]
fn deferred_hud_writes_are_made_before_a_script_reads_the_tree() {
    use fnv_game::ui_hud::{self, Shown, Texts};
    let f = fixture();
    let mut g = game(&f);
    let hud = MemSource::default().with("menus\\hud_main_menu.xml", r#"<menu name="HUDMainMenu"> <rect name="RadiationMeter"> <text name="radiation_text"> <string> file </string> </text> </rect> </menu>"#);
    g.ui_tree.load_menu(&hud, "menus\\hud_main_menu.xml").unwrap();
    g.ui_tree.update();
    let texts = Texts {
        radiation_word: "RAD/SEC".into(),
        radiation_value: "+0".into(),
        xp_label: "XP".into(),
        xp_level_up: "LEVEL UP".into(),
        needs: ["H20", "FOD", "SLP", "RAD", "LMB"].map(String::from),
        sneak: None,
        prompt_key: None,
        prompt_verb: None,
        frame: Default::default(),
    };
    // Nothing read the tree: the frame's writes wait, the tree keeps the file's text.
    ui_hud::defer(&mut g, Shown { root: true, ..Default::default() }, texts, None);
    assert_eq!(g.ui_tree.get_string("HUDMainMenu/RadiationMeter/radiation_text/string").as_deref(), Some("file"));
    assert!(!g.ui_used);
    // MenuTapKey reads no tree: still waiting.
    run(&mut g, "MenuTapKey", &[XVal::Num(28.0)]);
    assert!(g.ui_hud_pending.is_some());
    // A script's read makes them first, and the engine writes every frame from then on.
    assert_eq!(run(&mut g, "GetUIString", &[s("HUDMainMenu/RadiationMeter/radiation_text/string")]), s("RAD/SEC"));
    assert!(g.ui_used && g.ui_hud_pending.is_none());
}

#[test]
fn deferred_hud_writes_carry_the_measured_text_widths() {
    use fnv_game::ui_hud::{self, Measure, Shown, Texts};
    let f = fixture();
    let mut g = game(&f);
    // The prompt's verb tile as the file has it: "Button Text", 156 wide [observed:
    // hud.md §14.1].
    let hud = MemSource::default().with(
        "menus\\hud_main_menu.xml",
        r#"<menu name="HUDMainMenu"> <rect name="Info"> <rect name="justify_center_hotrect"> <text name="button_text"> <string> Button Text </string> <font> 7 </font> <width> 156 </width> </text> </rect> </rect> </menu>"#,
    );
    g.ui_tree.load_menu(&hud, "menus\\hud_main_menu.xml").unwrap();
    g.ui_tree.update();
    let texts = Texts {
        radiation_word: "RAD/SEC".into(),
        radiation_value: "+0".into(),
        xp_label: "XP".into(),
        xp_level_up: "LEVEL UP".into(),
        needs: ["H20", "FOD", "SLP", "RAD", "LMB"].map(String::from),
        sneak: None,
        prompt_key: None,
        prompt_verb: Some("Talk".into()),
        frame: Default::default(),
    };
    // A stand-in for the engine's fonts: 13.5 units a letter in font 7, so "Talk" is 54
    // as the original measured it [observed: hud.md §14.1].
    let measure: Measure = std::sync::Arc::new(|font: f64, text: &str| if font == 7.0 { 13.5 * text.chars().count() as f64 } else { 0.0 });
    ui_hud::defer(&mut g, Shown { root: true, ..Default::default() }, texts, Some(measure));
    let path = "HUDMainMenu/Info/justify_center_hotrect/button_text/width";
    assert_eq!(g.ui_tree.get_float(path), Some(156.0));
    // The script's first read makes the strings and then the widths measured from them,
    // as a frame writes them: the measured width, not the file's.
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s(path)])), 54.0);
    assert_eq!(g.ui_tree.get_string("HUDMainMenu/Info/justify_center_hotrect/button_text/string").as_deref(), Some("Talk"));
}

/// A game with a small HUD and start menu, and a frame's HUD writes kept back.
fn deferred(f: &Fixture) -> Game<'_> {
    use fnv_game::ui_hud::{self, Shown, Texts};
    let mut g = game(f);
    let menus = MemSource::default()
        .with("menus\\hud_main_menu.xml", r#"<menu name="HUDMainMenu"> <rect name="RadiationMeter"> <text name="radiation_text"> <string> file </string> </text> </rect> </menu>"#)
        .with("menus\\start_menu.xml", r#"<menu name="StartMenu"> <rect name="Panel"> <x> 5 </x> <template name="row"> <rect name="Row"> <x> 1 </x> </rect> </template> </rect> </menu>"#);
    g.ui_tree.load_menu(&menus, "menus\\hud_main_menu.xml").unwrap();
    g.ui_tree.load_menu(&menus, "menus\\start_menu.xml").unwrap();
    g.ui_tree.update();
    let texts = Texts {
        radiation_word: "RAD/SEC".into(),
        radiation_value: "+0".into(),
        xp_label: "XP".into(),
        xp_level_up: "LEVEL UP".into(),
        needs: ["H20", "FOD", "SLP", "RAD", "LMB"].map(String::from),
        sneak: None,
        prompt_key: None,
        prompt_verb: None,
        frame: Default::default(),
    };
    ui_hud::defer(&mut g, Shown { root: true, ..Default::default() }, texts, None);
    g
}

#[test]
fn only_hud_paths_make_the_deferred_hud_writes() {
    let f = fixture();
    let mut g = deferred(&f);
    // Calls on another menu read and change it, and leave the HUD's writes waiting.
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("StartMenu/Panel/x")])), 5.0);
    assert_eq!(n(run(&mut g, "SetUIFloat", &[s("StartMenu\\Panel\\x"), XVal::Num(6.0)])), 1.0);
    assert_eq!(n(run(&mut g, "AddTileFromTemplate", &[s("StartMenu/Panel|row|Row0")])), 1.0);
    assert_eq!(n(run(&mut g, "InjectUIComponent", &[s("StartMenu/Panel"), s("<rect name=\"Added\"/>")])), 1.0);
    assert_eq!(n(run(&mut g, "UnloadUIComponent", &[s("StartMenu/Panel/Added")])), 1.0);
    assert_eq!(n(run(&mut g, "IsComponentLoaded", &[s("StartMenu/Panel")])), 1.0);
    assert!(!g.ui_used && g.ui_hud_pending.is_some());
    // A read of the HUD (any case, either slash) makes them first.
    assert_eq!(run(&mut g, "GetUIString", &[s(" hudmainmenu\\RadiationMeter/radiation_text/string")]), s("RAD/SEC"));
    assert!(g.ui_used && g.ui_hud_pending.is_none());
}

#[test]
fn a_change_to_the_hud_makes_the_deferred_hud_writes() {
    let f = fixture();
    let mut g = deferred(&f);
    assert_eq!(n(run(&mut g, "InjectUIComponent", &[s("HUDMainMenu"), s("<rect name=\"ModGauge\"> <x> 3 </x> </rect>")])), 1.0);
    assert!(g.ui_used && g.ui_hud_pending.is_none());
    assert_eq!(g.ui_tree.get_string("HUDMainMenu/RadiationMeter/radiation_text/string").as_deref(), Some("RAD/SEC"));
}

/// A query script compiled from source and run by the VM, as a mod's script runs: the
/// value it sets with SetFunctionValue.
fn query(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}
{body}
End
"), Default::default()).expect("the query runs")
}

#[test]
fn compiled_scripts_reach_the_tree() {
    let f = fixture();
    let mut g = game(&f);
    // Set and read back by path through the VM and the extender's name lookup.
    let v = query(&mut g, "SetUIFloat \"HUDMenu/Bars/width\" 120
SetFunctionValue (GetUIFloat \"HUDMenu/Bars/width\")");
    assert_eq!(n(v), 120.0);
    assert_eq!(n(query(&mut g, "SetFunctionValue (GetUIFloatAlt \"HUDMenu/Bars/Nope\")")), NOT_FOUND);
    // The format string's values are filled in before the tree gets the text.
    let v = query(&mut g, "SetUIStringEx \"HUDMenu/Label/string\" \"HP %g\" 50
SetFunctionValue (GetUIString \"HUDMenu/Label/string\")");
    assert_eq!(v, s("HP 50"));
    assert_eq!(n(query(&mut g, "SetFunctionValue (AddTileFromTemplate \"HUDMenu/Bars|pip|Pip%g\" 3)")), 1.0);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("HUDMenu/Bars/Pip3/alpha")])), 255.0);
    // `%q` is a quote [documented: InjectUIComponent's example; String Formatting].
    let v = query(&mut g, "SetFunctionValue (InjectUIComponent \"HUDMenu\" \"<rect name=%qAdded%g%q> <x> %g </x> </rect>\" 2 9)");
    assert_eq!(n(v), 1.0);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("HUDMenu/Added2/x")])), 9.0);
}

#[test]
fn mcm_functions_stand_for_start_menu_traits() {
    let f = fixture();
    let mut g = game(&f);
    // MCM's child and grandchild count from 1 in file order, 0 stopping at the level
    // above (see docs/spec-notes/menu-xml.md §10): `1 1` is the first child's
    // first child, `0 0` the MCM rect.
    let src = MemSource::default().with(
        "menus\\start.xml",
        r#"<menu name="StartMenu"> <rect name="MCM"> <_ActiveMod> 7 </_ActiveMod>
           <rect name="Mod"> <_count> 2 </_count> <rect name="Option"> <_enable> 1 </_enable> <string> off </string> </rect> <rect name="Second"> <_enable> 2 </_enable> </rect> </rect>
           <rect name="Mods"/> </rect> </menu>"#,
    );
    g.ui_tree.load_menu(&src, "menus\\start.xml").unwrap();
    g.ui_tree.update();
    assert_eq!(n(run(&mut g, "GetMCMFloat", &[XVal::Num(1.0), XVal::Num(1.0), s("_enable")])), 1.0);
    run(&mut g, "SetMCMFloat", &[XVal::Num(1.0), XVal::Num(1.0), s("_enable"), XVal::Num(0.0)]);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("StartMenu/MCM/Mod/Option/_enable")])), 0.0);
    run(&mut g, "SetMCMString", &[XVal::Num(1.0), XVal::Num(1.0), s("string"), s("on")]);
    assert_eq!(run(&mut g, "GetUIString", &[s("StartMenu/MCM/Mod/Option/string")]), s("on"));
    assert_eq!(n(run(&mut g, "GetMCMFloat", &[XVal::Num(1.0), XVal::Num(2.0), s("_enable")])), 2.0);
    // 0 stops at the level above: the MCM rect's own traits, the first child's own.
    assert_eq!(n(run(&mut g, "GetMCMFloat", &[XVal::Num(0.0), XVal::Num(0.0), s("_ActiveMod")])), 7.0);
    assert_eq!(n(run(&mut g, "GetMCMFloat", &[XVal::Num(1.0), XVal::Num(0.0), s("_count")])), 2.0);
    run(&mut g, "SetMCMFloat", &[XVal::Num(0.0), XVal::Num(0.0), s("_ActiveMod"), XVal::Num(3.0)]);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("StartMenu/MCM/_ActiveMod")])), 3.0);
    // A trait or place that is not there reads 0, where GetUIFloat reads NOT_FOUND, and
    // the setters make nothing.
    assert_eq!(n(run(&mut g, "GetMCMFloat", &[XVal::Num(5.0), XVal::Num(5.0), s("_enable")])), 0.0);
    run(&mut g, "SetMCMFloat", &[XVal::Num(0.0), XVal::Num(0.0), s("_Nope"), XVal::Num(1.0)]);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("StartMenu/MCM/_Nope")])), NOT_FOUND);
}

// ---- the pointer on tiles (task T8, fnv_game::ui_input, fnv_ui::input) --------------

/// A synthetic menu with two buttons, a hidden target image, a plain image with an id,
/// and the traits the event handlers below write.
fn pointer_menu(g: &mut Game) -> fnv_ui::TileId {
    let src = MemSource::default().with(
        "menus\\pointer.xml",
        r#"<menu name="TestMenu"> <class> &QuantityMenu; </class>
           <_clickMenu> 0 </_clickMenu> <_clickId> 0 </_clickId> <_nameLen> 0 </_nameLen> <_cancel> 0 </_cancel>
           <_prefix> 0 </_prefix> <_overs> 0 </_overs> <_overMenu> 0 </_overMenu>
           <_PCButton_E> CancelButton </_PCButton_E>
           <rect name="Main"> <x> 100 </x> <y> 100 </y>
             <hotrect name="OkButton"> <id> 1 </id> <target> &true; </target> <x> 10 </x> <y> 10 </y> <width> 200 </width> <height> 50 </height> </hotrect>
             <hotrect name="CancelButton"> <id> 2 </id> <target> &true; </target> <x> 10 </x> <y> 100 </y> <width> 200 </width> <height> 50 </height> </hotrect>
             <image name="Arrow"> <target> &true; </target> <visible> &false; </visible> <x> 300 </x> <y> 10 </y> <width> 20 </width> <height> 20 </height> </image>
             <image name="Plain"> <id> 9 </id> <x> 400 </x> <y> 10 </y> <width> 20 </width> <height> 20 </height> </image>
           </rect> </menu>"#,
    );
    let menu = g.ui_tree.load_menu(&src, "menus\\pointer.xml").unwrap();
    g.ui_tree.update();
    menu
}

fn pointer_at(x: f64, y: f64) -> fnv_ui::input::Input {
    fnv_ui::input::Input { at: Some((x, y)), ..Default::default() }
}

#[test]
fn the_active_component_is_the_pointers_tile() {
    use fnv_game::ui_input;
    use fnv_ui::input::no_measure;
    let f = fixture();
    let mut g = game(&f);
    let menu = pointer_menu(&mut g);
    // Nothing highlighted [hypothesis: empty and -1].
    assert_eq!(run(&mut g, "GetActiveUIComponentName", &[]), s(""));
    assert_eq!(run(&mut g, "GetActiveUIComponentFullName", &[]), s(""));
    assert_eq!(n(run(&mut g, "GetActiveUIComponentID", &[])), -1.0);
    ui_input::step(&mut g, &[menu], &pointer_at(150.0, 130.0), &no_measure);
    assert_eq!(run(&mut g, "GetActiveUIComponentName", &[]), s("OkButton"));
    assert_eq!(run(&mut g, "GetActiveUIComponentFullName", &[]), s("TestMenu/Main/OkButton"));
    assert_eq!(n(run(&mut g, "GetActiveUIComponentID", &[])), 1.0);
    // The full name is a path GetUIFloat takes [documented: GetActiveUIComponentFullName].
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("TestMenu/Main/OkButton/mouseover")])), 1.0);
    // Through the VM as a script calls them.
    assert_eq!(n(query(&mut g, "SetFunctionValue (GetActiveUIComponentID)")), 1.0);
    // PrintActiveTile prints its name [documented: PrintActiveTile].
    g.console_out.clear();
    run(&mut g, "PrintActiveTile", &[]);
    assert_eq!(g.console_out, ["OkButton"]);
    // The pointer leaves: nothing highlighted again, mouseover back to 0.
    ui_input::step(&mut g, &[menu], &pointer_at(0.0, 0.0), &no_measure);
    assert_eq!(run(&mut g, "GetActiveUIComponentName", &[]), s(""));
    assert_eq!(n(run(&mut g, "GetActiveUIComponentID", &[])), -1.0);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("TestMenu/Main/OkButton/mouseover")])), 0.0);
}

#[test]
fn click_functions_click_as_a_press_does() {
    use fnv_game::ui_input;
    use fnv_ui::input::{no_measure, By};
    let f = fixture();
    let mut g = game(&f);
    let menu = pointer_menu(&mut g);
    // ClickMenuButton by `Menu#id` and by path [documented: ClickMenuButton]. The traits
    // are read from the tree, not through GetUIFloat, so only the click functions could
    // start the HUD's writes.
    assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("TestMenu#1")])), 1.0);
    assert_eq!(g.ui_tree.get_float("TestMenu/Main/OkButton/clicked").unwrap(), 1.0);
    assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("TestMenu\\Main\\CancelButton"), XVal::Num(3.0)])), 1.0);
    assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("TestMenu#2"), XVal::Num(0.0)])), 0.0);
    // It needs a target with an id: not the hidden image (no id), not the plain one (no
    // target), not a menu that is not there.
    assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("TestMenu/Main/Arrow")])), 0.0);
    assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("TestMenu#9")])), 0.0);
    assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("NoMenu#1")])), 0.0);
    // ClickMenuTile takes any target, hidden too [documented: ClickMenuTile; Tutorial].
    assert_eq!(n(run(&mut g, "ClickMenuTile", &[s("TestMenu/Main/Arrow")])), 1.0);
    assert_eq!(n(run(&mut g, "ClickMenuTile", &[s("TestMenu/Main/Plain")])), 0.0);
    assert!(!g.ui_used, "clicks on another menu leave the HUD's writes waiting");
    // The next step reports them all and ends the pulses.
    let report = ui_input::step(&mut g, &[menu], &Default::default(), &no_measure);
    let names: Vec<String> = report.clicks.iter().map(|c| g.ui_tree.tile(c.tile).name.clone()).collect();
    assert_eq!(names, ["OkButton", "CancelButton", "CancelButton", "CancelButton", "Arrow"]);
    assert!(report.clicks.iter().all(|c| c.by == By::Script));
    assert_eq!(g.ui_tree.get_float("TestMenu/Main/OkButton/clicked").unwrap(), 0.0);
    assert_eq!(g.ui_tree.get_float("TestMenu/Main/CancelButton/clicked").unwrap(), 0.0);
}

#[test]
fn a_click_on_the_hud_makes_its_deferred_writes() {
    use fnv_game::ui_hud::{self, Shown, Texts};
    let f = fixture();
    let mut g = game(&f);
    let hud = MemSource::default().with(
        "menus\\hud_main_menu.xml",
        r#"<menu name="HUDMainMenu"> <hotrect name="Probe"> <id> 4 </id> <target> 1 </target> </hotrect> </menu>"#,
    );
    g.ui_tree.load_menu(&hud, "menus\\hud_main_menu.xml").unwrap();
    g.ui_tree.update();
    let texts = Texts {
        radiation_word: String::new(),
        radiation_value: String::new(),
        xp_label: String::new(),
        xp_level_up: String::new(),
        needs: Default::default(),
        sneak: None,
        prompt_key: None,
        prompt_verb: None,
        frame: Default::default(),
    };
    ui_hud::defer(&mut g, Shown::default(), texts, None);
    assert_eq!(n(run(&mut g, "ClickMenuTile", &[s("HUDMainMenu#4")])), 1.0);
    assert!(g.ui_used && g.ui_hud_pending.is_none());
}

#[test]
fn menu_events_fire_from_the_pointer_and_keys() {
    use fnv_game::ui_input;
    use fnv_ui::input::{no_measure, Input};
    let f = fixture();
    let mut g = game(&f);
    let menu = pointer_menu(&mut g);
    // [documented: SetOnMenuClickEventHandler: menu ID, tile id, lower-case name; a
    // `Menu#id`, full or partial path filter. SetOnMouseoverChangeEventHandler: the menu
    // ID, any menu without a filter.]
    query(
        &mut g,
        r#"SetOnMenuClickEventHandler (Begin Function {int m, int t, string_var sN}
SetUIFloat "TestMenu/_clickMenu" m
SetUIFloat "TestMenu/_clickId" t
SetUIFloat "TestMenu/_nameLen" (Sv_Length sN)
End) 1 "TestMenu#1"
SetOnMenuClickEventHandler (Begin Function {int m, int t, string_var sN}
ModUIFloat "TestMenu/_cancel" 1
End) 1 "TestMenu/Main/CancelButton"
SetOnMenuClickEventHandler (Begin Function {int m, int t, string_var sN}
ModUIFloat "TestMenu/_prefix" 1
End) 1 "testmenu/MAIN"
SetOnMouseoverChangeEventHandler (Begin Function {int m}
ModUIFloat "TestMenu/_overs" 1
SetUIFloat "TestMenu/_overMenu" m
End) 1"#,
    );
    let get = |g: &mut Game, t: &str| n(run(g, "GetUIFloat", &[s(&format!("TestMenu/{t}"))]));
    // The pointer onto OkButton: one highlight change, in QuantityMenu (1016).
    ui_input::step(&mut g, &[menu], &pointer_at(150.0, 130.0), &no_measure);
    assert_eq!(get(&mut g, "_overs"), 1.0);
    assert_eq!(get(&mut g, "_overMenu"), 1016.0);
    // A click on it: the `#1` and the partial-path handlers run, the Cancel one does not.
    ui_input::step(&mut g, &[menu], &Input { press: true, ..pointer_at(150.0, 130.0) }, &no_measure);
    assert_eq!(get(&mut g, "_clickMenu"), 1016.0);
    assert_eq!(get(&mut g, "_clickId"), 1.0);
    assert_eq!(get(&mut g, "_nameLen"), 8.0, "\"okbutton\"");
    assert_eq!(get(&mut g, "_prefix"), 1.0);
    assert_eq!(get(&mut g, "_cancel"), 0.0);
    assert_eq!(get(&mut g, "_overs"), 1.0, "a still pointer moves no highlight");
    // The E key clicks CancelButton through the menu's _PCButton_E.
    ui_input::step(&mut g, &[menu], &Input { keys: vec!["E".into()], ..pointer_at(150.0, 130.0) }, &no_measure);
    assert_eq!(get(&mut g, "_cancel"), 1.0);
    assert_eq!(get(&mut g, "_prefix"), 2.0);
    // A script's click fires its event at the next step.
    run(&mut g, "ClickMenuButton", &[s("TestMenu#2")]);
    assert_eq!(get(&mut g, "_cancel"), 1.0);
    ui_input::step(&mut g, &[menu], &pointer_at(150.0, 130.0), &no_measure);
    assert_eq!(get(&mut g, "_cancel"), 2.0);
}

#[test]
fn a_script_click_ends_with_no_menu_taking_input() {
    use fnv_game::ui_input;
    let f = fixture();
    let mut g = game(&f);
    pointer_menu(&mut g);
    query(
        &mut g,
        r#"SetOnMenuClickEventHandler (Begin Function {int m, int t, string_var sN}
ModUIFloat "TestMenu/_cancel" 1
End) 1 "TestMenu#2""#,
    );
    let clicked = |g: &Game| g.ui_tree.get_float("TestMenu/Main/CancelButton/clicked");
    let fired = |g: &Game| g.ui_tree.get_float("TestMenu/_cancel").unwrap();
    // Nothing highlighted and nothing left over: no step, no tree work.
    assert!(ui_input::idle(&mut g).is_none());
    // A mod clicks every frame and no menu takes tile input (play without the test aid):
    // each click lasts its frame, fires its event once, and leaves nothing queued
    // [design: the maintainer's review of #957].
    for frame in 1..=3 {
        assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("TestMenu#2")])), 1.0);
        assert_eq!(clicked(&g), Some(1.0));
        assert!(g.ui_pointer.pending());
        let report = ui_input::idle(&mut g).expect("the script's click is stepped");
        assert_eq!(report.clicks.len(), 1, "frame {frame}");
        assert_eq!(clicked(&g), Some(0.0), "frame {frame}");
        assert_eq!(fired(&g), f64::from(frame));
        assert!(!g.ui_pointer.pending(), "frame {frame}");
    }
    // The frame after: nothing left, nothing fired again.
    assert!(ui_input::idle(&mut g).is_none());
    assert_eq!(fired(&g), 3.0);
}

#[test]
fn a_huge_click_count_is_capped() {
    use fnv_game::ui_functions::MAX_CLICK_TIMES;
    use fnv_game::ui_input;
    let f = fixture();
    let mut g = game(&f);
    pointer_menu(&mut g);
    // `1e9` clicks would hang the frame; it makes MAX_CLICK_TIMES [design: the
    // maintainer's review of #957]. An infinite count too; a count that is no number, none.
    assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("TestMenu#2"), XVal::Num(1e9)])), 1.0);
    assert_eq!(ui_input::idle(&mut g).expect("clicks to report").clicks.len(), MAX_CLICK_TIMES);
    assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("TestMenu#2"), XVal::Num(f64::INFINITY)])), 1.0);
    assert_eq!(ui_input::idle(&mut g).expect("clicks to report").clicks.len(), MAX_CLICK_TIMES);
    assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("TestMenu#2"), XVal::Num(f64::NAN)])), 0.0);
    assert!(!g.ui_pointer.pending());
    // Under the cap the count is kept.
    assert_eq!(n(run(&mut g, "ClickMenuButton", &[s("TestMenu#2"), XVal::Num(5.0)])), 1.0);
    assert_eq!(ui_input::idle(&mut g).expect("clicks to report").clicks.len(), 5);
}

#[test]
fn keyboard_controller_selection_event_fires_on_key_navigation_not_pointer() {
    use fnv_game::ui_input;
    use fnv_ui::input::{no_measure, Input};
    let f = fixture();
    let mut g = game(&f);
    let src = MemSource::default().with(
        "menus\\kbnav.xml",
        r#"<menu name="InventoryMenu"> <class> &InventoryMenu; </class>
           <_overs> 0 </_overs> <_kbMatch> 0 </_kbMatch> <_kbMenu> 0 </_kbMenu>
           <_kbAny> 0 </_kbAny> <_kbOther> 0 </_kbOther>
           <rect name="Main"> <x> 100 </x> <y> 100 </y>
             <hotrect name="RowA"> <id> 29 </id> <target> &true; </target>
               <x> 10 </x> <y> 10 </y> <width> 200 </width> <height> 40 </height>
               <xdown> <ref src="sibling(RowB)" trait="mouseover"/> </xdown>
             </hotrect>
             <hotrect name="RowB"> <id> 29 </id> <target> &true; </target>
               <x> 10 </x> <y> 60 </y> <width> 200 </width> <height> 40 </height>
               <xup> <ref src="sibling(RowA)" trait="mouseover"/> </xup>
               <xdown> <ref src="me()" trait="mouseover"/> </xdown>
             </hotrect>
           </rect> </menu>"#,
    );
    let menu = g.ui_tree.load_menu(&src, "menus\\kbnav.xml").unwrap();
    g.ui_tree.update();
    query(
        &mut g,
        r#"SetOnMouseoverChangeEventHandler (Begin Function {int m}
ModUIFloat "InventoryMenu/_overs" 1
End) 1 1002
SetOnKBCTrlUIDeltaHandler 1 (Begin Function {int m}
ModUIFloat "InventoryMenu/_kbMatch" 1
SetUIFloat "InventoryMenu/_kbMenu" m
End) 0 1002
SetOnKBCTrlUIDeltaHandler 1 (Begin Function {int m}
ModUIFloat "InventoryMenu/_kbAny" 1
End) 0 0
SetJohnnyOnKeyboardControllerSelectionChangeEventHandler 1 (Begin Function {int m}
ModUIFloat "InventoryMenu/_kbOther" 1
End) 0 1053"#,
    );
    let get = |g: &mut Game, t: &str| n(run(g, "GetUIFloat", &[s(&format!("InventoryMenu/{t}"))]));

    // Pointer hover onto RowA fires SetOnMouseoverChangeEventHandler only.
    ui_input::step(&mut g, &[menu], &pointer_at(150.0, 120.0), &no_measure);
    assert_eq!(get(&mut g, "_overs"), 1.0);
    assert_eq!(get(&mut g, "_kbMatch"), 0.0);
    assert_eq!(get(&mut g, "_kbAny"), 0.0);
    assert_eq!(get(&mut g, "_kbOther"), 0.0);

    // Keyboard/controller xdown moves RowA -> RowB and fires matching 1002 + wildcard 0.
    ui_input::step(&mut g, &[menu], &Input { pads: vec!["xdown".into()], ..pointer_at(150.0, 120.0) }, &no_measure);
    assert_eq!(get(&mut g, "_overs"), 2.0);
    assert_eq!(get(&mut g, "_kbMatch"), 1.0);
    assert_eq!(get(&mut g, "_kbMenu"), 1002.0);
    assert_eq!(get(&mut g, "_kbAny"), 1.0);
    assert_eq!(get(&mut g, "_kbOther"), 0.0);

    // Self-referencing xdown on RowB does not change highlight and does not fire again.
    ui_input::step(&mut g, &[menu], &Input { pads: vec!["xdown".into()], ..pointer_at(150.0, 120.0) }, &no_measure);
    assert_eq!(get(&mut g, "_overs"), 2.0);
    assert_eq!(get(&mut g, "_kbMatch"), 1.0);
    assert_eq!(get(&mut g, "_kbAny"), 1.0);

    // Keyboard/controller xup moves RowB -> RowA and fires again.
    ui_input::step(&mut g, &[menu], &Input { pads: vec!["xup".into()], ..pointer_at(150.0, 120.0) }, &no_measure);
    assert_eq!(get(&mut g, "_overs"), 3.0);
    assert_eq!(get(&mut g, "_kbMatch"), 2.0);
    assert_eq!(get(&mut g, "_kbAny"), 2.0);

    // Pure pointer move onto RowB with a self-referencing xdown in the same frame must not
    // falsely claim keyboard navigation.
    ui_input::step(&mut g, &[menu], &Input { pads: vec!["xdown".into()], ..pointer_at(150.0, 175.0) }, &no_measure);
    assert_eq!(get(&mut g, "_overs"), 4.0);
    assert_eq!(get(&mut g, "_kbMatch"), 2.0);
    assert_eq!(get(&mut g, "_kbAny"), 2.0);

    // Game restore flushes session-only JohnnyGuitar handlers.
    let saved = g.snapshot();
    let restored = Game::restore(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default(), &saved).unwrap();
    assert!(!restored.jg8_listening(fnv_game::nvse_jg8::Ev::KeyboardSelection));
}

#[test]
fn set_ui_string_alt_xml_entity_strings_hide_tiles_and_match_expressions() {
    use fnv_ui::input::{hit, no_measure};
    use fnv_ui::Value;

    let f = fixture();
    let mut g = game(&f);
    let src = MemSource::default().with(
        "menus\\barter_test.xml",
        r#"<menu name="BarterTest">
  <rect name="Panel">
    <text name="Title">
      <id>1</id>
      <target>&true;</target>
      <x>10</x>
      <y>10</y>
      <width>100</width>
      <height>20</height>
      <visible>&true;</visible>
      <justify>&left;</justify>
      <_flag>&true;</_flag>
      <_pic_0>off.dds</_pic_0>
      <_pic_1>on.dds</_pic_1>
      <filename><copy src="me()" trait="_flag"/><copy src="me()" trait="_pic_"/></filename>
      <_is_centered><copy>1</copy><onlyif><copy src="me()" trait="justify"/><eq>&center;</eq></onlyif></_is_centered>
      <string>ALL ITEMS</string>
    </text>
  </rect>
</menu>"#,
    );
    let menu = g.ui_tree.load_menu(&src, "menus\\barter_test.xml").unwrap();
    g.ui_tree.update();
    let tile = g.ui_tree.find_tile("BarterTest/Panel/Title").unwrap();

    // Initially visible (x=10..110, y=10..30), left-justified, _flag = &true; -> on.dds.
    // Static XML `<visible>&true;</visible>` parses as Value::Sym("true") and reads 1.0 via GetUIFloat.
    assert!(g.ui_tree.tile(tile).get("visible").is_none_or(Value::truthy));
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("BarterTest/Panel/Title/visible")])), 1.0);
    assert!(hit(&g.ui_tree, &[menu], (20.0, 20.0), &no_measure).is_some());
    assert_eq!(g.ui_tree.get_string("BarterTest/Panel/Title/filename").as_deref(), Some("on.dds"));
    assert_eq!(g.ui_tree.get_float("BarterTest/Panel/Title/_is_centered"), Some(0.0));

    // SetUIString / SetUIStringAlt store entity tokens as verbatim strings whose numeric
    // GetUIFloat / GetUIFloatAlt readback is 0.0 for "&false", "&true", and "&center"
    // [observed: w4d_43_nif_selector_and_ui_entities].
    assert_eq!(n(run(&mut g, "SetUIString", &[s("BarterTest/Panel/Title/visible"), s("&false")])), 1.0);
    assert!(!g.ui_tree.tile(tile).get("visible").is_none_or(Value::truthy), "&false string must hide tile");
    assert!(hit(&g.ui_tree, &[menu], (20.0, 20.0), &no_measure).is_none());
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("BarterTest/Panel/Title/visible")])), 0.0);
    assert_eq!(n(run(&mut g, "GetUIFloatAlt", &[s("BarterTest/Panel/Title/visible")])), 0.0);
    assert_eq!(run(&mut g, "GetUIString", &[s("BarterTest/Panel/Title/visible")]), s("&false"));

    // SetUIString / SetUIStringAlt ".../visible" "&true" keeps GetUIFloat == 0.0 (not 1.0)
    // and preserves "&true" verbatim for GetUIString.
    assert_eq!(n(run(&mut g, "SetUIString", &[s("BarterTest/Panel/Title/visible"), s("&true")])), 1.0);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("BarterTest/Panel/Title/visible")])), 0.0);
    assert_eq!(n(run(&mut g, "GetUIFloatAlt", &[s("BarterTest/Panel/Title/visible")])), 0.0);
    assert_eq!(run(&mut g, "GetUIString", &[s("BarterTest/Panel/Title/visible")]), s("&true"));
    assert!(g.ui_tree.tile(tile).get("visible").is_none_or(Value::truthy), "&true string must show tile");
    assert!(hit(&g.ui_tree, &[menu], (20.0, 20.0), &no_measure).is_some());

    // Entity strings also work in XML <eq>, suffix-<copy>, and text justification (x=10, w=100 -> -40..60),
    // while reading 0.0 via GetUIFloat.
    assert_eq!(n(run(&mut g, "SetUIStringAlt", &[s("BarterTest/Panel/Title/justify"), s("&center")])), 1.0);
    assert_eq!(n(run(&mut g, "GetUIFloat", &[s("BarterTest/Panel/Title/justify")])), 0.0);
    assert_eq!(n(run(&mut g, "SetUIStringAlt", &[s("BarterTest/Panel/Title/_flag"), s("&false")])), 1.0);
    g.ui_tree.update();
    assert_eq!(g.ui_tree.get_float("BarterTest/Panel/Title/_is_centered"), Some(1.0));
    assert_eq!(g.ui_tree.get_string("BarterTest/Panel/Title/filename").as_deref(), Some("off.dds"));
    assert!(hit(&g.ui_tree, &[menu], (-20.0, 20.0), &no_measure).is_some());
    assert!(hit(&g.ui_tree, &[menu], (80.0, 20.0), &no_measure).is_none());
}

#[test]
fn barter_and_container_menu_click_handlers_support_wildcards_indices_and_removal() {
    use fnv_game::game::RunCtx;
    use fnv_game::ui_input;
    use fnv_ui::input::{no_measure, Input};
    let f = fixture();
    let mut g = game(&f);

    // Keep Stewie plugin gate pinned at 960 while building #3184 menu-click consumers.
    assert_eq!(
        g.run_query("Begin Function { }\nSetFunctionValue (GetPluginVersion \"lStewieAl's Tweaks\")\nEnd\n", RunCtx::default()).unwrap(),
        XVal::Num(960.0),
    );

    let src = MemSource::default()
        .with(
            "menus\\barter_menu.xml",
            r#"<menu name="BarterMenu">
                <class> &BarterMenu; </class>
                <_lastMenu> 0 </_lastMenu>
                <_lastId> 0 </_lastId>
                <_lastName> </_lastName>
                <_wildHits> 0 </_wildHits>
                <_idx1Hits> 0 </_idx1Hits>
                <_prefixHits> 0 </_prefixHits>
                <rect name="BarterCategories">
                    <x> 0 </x> <y> 0 </y> <width> 300 </width> <height> 100 </height>
                    <hotrect name="BC_Category">
                        <id> 10 </id> <target> 1 </target>
                        <x> 0 </x> <y> 0 </y> <width> 80 </width> <height> 40 </height>
                    </hotrect>
                    <hotrect name="BC_Category">
                        <id> 11 </id> <target> 1 </target>
                        <x> 100 </x> <y> 0 </y> <width> 80 </width> <height> 40 </height>
                    </hotrect>
                    <hotrect name="BC_Category">
                        <id> 12 </id> <target> 1 </target>
                        <x> 200 </x> <y> 0 </y> <width> 80 </width> <height> 40 </height>
                    </hotrect>
                </rect>
            </menu>"#,
        )
        .with(
            "menus\\container_menu.xml",
            r#"<menu name="ContainerMenu">
                <class> &ContainerMenu; </class>
                <_hits> 0 </_hits>
                <_menuId> 0 </_menuId>
                <hotrect name="CM_TakeAll">
                    <id> 20 </id> <target> 1 </target>
                    <x> 10 </x> <y> 10 </y> <width> 80 </width> <height> 40 </height>
                </hotrect>
            </menu>"#,
        );
    let barter = g.ui_tree.load_menu(&src, "menus\\barter_menu.xml").unwrap();
    let container = g.ui_tree.load_menu(&src, "menus\\container_menu.xml").unwrap();
    g.ui_tree.update();

    query(
        &mut g,
        r#"SetOnMenuClickEventHandler (Begin Function {int m, int t, string_var sN}
SetUIFloat "BarterMenu/_lastMenu" m
SetUIFloat "BarterMenu/_lastId" t
SetUIString "BarterMenu/_lastName" sN
ModUIFloat "BarterMenu/_wildHits" 1
End) 1 "BarterMenu/*/BC_Category"
SetOnMenuClickEventHandler (Begin Function {int m, int t, string_var sN}
ModUIFloat "BarterMenu/_idx1Hits" 1
End) 1 "BarterMenu/BarterCategories/BC_Category:1"
SetOnMenuClickEventHandler (Begin Function {int m, int t, string_var sN}
ModUIFloat "ContainerMenu/_hits" 1
SetUIFloat "ContainerMenu/_menuId" m
End) 1 "ContainerMenu#20""#,
    );

    // Register a UDF on a prefix path, then test slash/case-insensitive removal below.
    query(
        &mut g,
        r#"ref f
let f := (Begin Function {int m, int t, string_var sN}
ModUIFloat "BarterMenu/_prefixHits" 1
End)
PlayerREF.AuxVarSetRef "bc_prefix_udf" f 0
SetOnMenuClickEventHandler f 1 "BarterMenu/BarterCategories""#,
    );

    // Click sibling 0 (id 10): wildcard and prefix fire, index-1 handler does not; name is "bc_category:0".
    ui_input::step(&mut g, &[barter], &Input { press: true, ..pointer_at(40.0, 20.0) }, &no_measure);
    assert_eq!(g.ui_tree.get_float("BarterMenu/_lastMenu"), Some(1053.0));
    assert_eq!(g.ui_tree.get_float("BarterMenu/_lastId"), Some(10.0));
    assert_eq!(g.ui_tree.get_string("BarterMenu/_lastName").as_deref(), Some("bc_category:0"));
    assert_eq!(g.ui_tree.get_float("BarterMenu/_wildHits"), Some(1.0));
    assert_eq!(g.ui_tree.get_float("BarterMenu/_idx1Hits"), Some(0.0));
    assert_eq!(g.ui_tree.get_float("BarterMenu/_prefixHits"), Some(1.0));

    // Click sibling 1 (id 11): all three handlers fire; name is "bc_category:1".
    ui_input::step(&mut g, &[barter], &Input { press: true, ..pointer_at(140.0, 20.0) }, &no_measure);
    assert_eq!(g.ui_tree.get_float("BarterMenu/_lastId"), Some(11.0));
    assert_eq!(g.ui_tree.get_string("BarterMenu/_lastName").as_deref(), Some("bc_category:1"));
    assert_eq!(g.ui_tree.get_float("BarterMenu/_wildHits"), Some(2.0));
    assert_eq!(g.ui_tree.get_float("BarterMenu/_idx1Hits"), Some(1.0));
    assert_eq!(g.ui_tree.get_float("BarterMenu/_prefixHits"), Some(2.0));

    // Remove the prefix handler using mixed case + backslash: sibling 2 click must NOT increment _prefixHits.
    query(
        &mut g,
        r#"ref f
let f := PlayerREF.AuxVarGetRef "bc_prefix_udf" 0
SetOnMenuClickEventHandler f 0 "bartermenu\BARTERCATEGORIES""#,
    );
    ui_input::step(&mut g, &[barter], &Input { press: true, ..pointer_at(240.0, 20.0) }, &no_measure);
    assert_eq!(g.ui_tree.get_string("BarterMenu/_lastName").as_deref(), Some("bc_category:2"));
    assert_eq!(g.ui_tree.get_float("BarterMenu/_wildHits"), Some(3.0));
    assert_eq!(g.ui_tree.get_float("BarterMenu/_idx1Hits"), Some(1.0));
    assert_eq!(g.ui_tree.get_float("BarterMenu/_prefixHits"), Some(2.0));

    // ContainerMenu (1008) click via Menu#id.
    ui_input::step(&mut g, &[container], &Input { press: true, ..pointer_at(40.0, 25.0) }, &no_measure);
    assert_eq!(g.ui_tree.get_float("ContainerMenu/_hits"), Some(1.0));
    assert_eq!(g.ui_tree.get_float("ContainerMenu/_menuId"), Some(1008.0));
}

#[test]
fn click_menu_tile_resolves_target_hotrect_child_and_fires_menu_click_event() {
    use fnv_game::ui_input;
    let f = fixture();
    let mut g = game(&f);
    let src = MemSource::default().with(
        "menus\\hotchild.xml",
        r#"<menu name="ChildMenu"> <class> &QuantityMenu; </class>
           <_hits> 0 </_hits> <_id> 0 </_id> <_name> "" </_name>
           <rect name="Main">
             <hotrect name="ActionBtn"> <id> 7 </id> <target> &true; </target>
               <text name="Label"> <string> OK </string> </text>
             </hotrect>
             <image name="Plain"> <id> 8 </id> </image>
           </rect> </menu>"#,
    );
    g.ui_tree.load_menu(&src, "menus\\hotchild.xml").unwrap();
    g.ui_tree.update();
    query(
        &mut g,
        r#"SetOnMenuClickEventHandler (Begin Function {int m, int t, string_var sN}
ModUIFloat "ChildMenu/_hits" 1
SetUIFloat "ChildMenu/_id" t
SetUIString "ChildMenu/_name" sN
End) 1 "ChildMenu/Main/ActionBtn""#,
    );
    // Child inside a target <hotrect> resolves to the owning target <hotrect>.
    assert_eq!(n(run(&mut g, "ClickMenuTile", &[s("ChildMenu/Main/ActionBtn/Label")])), 1.0);
    assert_eq!(g.ui_tree.get_float("ChildMenu/Main/ActionBtn/clicked"), Some(1.0));
    // Non-target tile outside any target <hotrect> still returns 0.
    assert_eq!(n(run(&mut g, "ClickMenuTile", &[s("ChildMenu/Main/Plain")])), 0.0);
    let report = ui_input::idle(&mut g).expect("queued script click is reported");
    assert_eq!(report.clicks.len(), 1);
    assert_eq!(g.ui_tree.get_float("ChildMenu/Main/ActionBtn/clicked"), Some(0.0));
    assert_eq!(g.ui_tree.get_float("ChildMenu/_hits"), Some(1.0));
    assert_eq!(g.ui_tree.get_float("ChildMenu/_id"), Some(7.0));
    assert_eq!(g.ui_tree.get_string("ChildMenu/_name").as_deref(), Some("actionbtn"));
}
