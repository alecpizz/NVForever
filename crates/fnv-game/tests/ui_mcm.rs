//! An MCM mod's menu script against the start menu's tile tree (task T7 of
//! docs/handoff/ui-tile-tree-tasks.md, issue #976), headless: the start menu is built
//! from files with MCM's include chain while the start menu is open, and a quest
//! script's `Begin MenuMode 1013` block, compiled from its source as an MCM mod's is,
//! reads and writes MCM's traits through GetMCMFloat, SetMCMFloat and SetMCMString.
//!
//! The menu files and the script are written for the test in MCM's shape (menu-xml.md
//! §10); no game or mod data is used.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{menus, Game, GameConfig, InstKey};
use fnv_game::{ui_loading, GameData};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use fnv_ui::MemSource;
use serde_json::Value as Json;

/// The mod's quest and its script, in the second plugin of the load order (index 1).
const QUEST: u32 = 0x0100_0800;
const SCRIPT: u32 = 0x0100_0900;

/// The quest script, in the way MCM mods write theirs (FOV Slider's `MenuMode 1013`
/// block, menu-xml.md §10): do nothing unless MCM's `_ActiveMod` is this plugin; fill in
/// the first option; take a new value when MCM says one was entered.
const SOURCE: &str = r#"scn McmTestMenuScript
short iShown
float fValue

Begin MenuMode 1013
  if GetMCMFloat 0 0 "_ActiveMod" != GetSelfModIndex
    return
  endif
  SetMCMString 1 1 "_title" "Test option"
  SetMCMFloat 1 1 "_enable" 1
  set iShown to iShown + 1
  if GetMCMFloat 0 0 "_NewValue" == 1
    set fValue to GetMCMFloat 0 0 "_Value"
    SetMCMFloat 0 0 "_NewValue" 0
    SetMCMFloat 0 0 "_Reset" 1
  endif
End
"#;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    // The bytes call an opcode no function has, as bytes compiled with the script
    // extender loaded may: the unit runs from its source (fnv_game::source_compile).
    let bytes = script(&[(0, None, st(0x7FFF, &le16(0)))]);
    let mut subs = unit(1, &bytes, &[(1, "iShown", 1), (2, "fValue", 0)], &[]);
    subs.push(sub(b"SCTX", SOURCE.as_bytes().to_vec()));
    let scpt = record(b"SCPT", 0, SCRIPT & 0xFF_FFFF, &[vec![sub(b"EDID", z("McmTestMenuScript"))], subs].concat());
    // Start game enabled; a short quest delay, so the script runs every frame.
    let data = sub(b"DATA", [vec![1u8, 0, 0, 0], 0.01f32.to_le_bytes().to_vec()].concat());
    let qust = record(b"QUST", 0, QUEST & 0xFF_FFFF, &[sub(b"EDID", z("McmTestQuest")), sub(b"SCRI", (SCRIPT & 0xFF_FFFF).to_le_bytes().to_vec()), data]);
    let plugin = [tes4(), top(b"SCPT", &scpt), top(b"QUST", &qust)].concat();
    let plugins = vec![Plugin::parse(&tes4(), "Base.esm".into()).unwrap(), Plugin::parse(&plugin, "McmTest.esp".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);
    Fixture { data, forms, refs }
}

/// A start menu whose include list adds MCM's file, as MCM's replacement `start_menu.xml`
/// and `includes_StartMenu.xml` do, and an MCM rect with its options list.
fn files() -> MemSource {
    MemSource::default()
        .with("menus\\options\\start_menu.xml", r#"<menu name="StartMenu"> <class> &StartMenu; </class> <include src="includes_StartMenu.xml"/> </menu>"#)
        .with("menus\\prefabs\\includes_StartMenu.xml", r#"<include src="MCM\MCM.xml"/>"#)
        .with(
            "menus\\prefabs\\MCM\\MCM.xml",
            r#"<rect name="MCM"> <_ActiveMod> 0 </_ActiveMod> <_NewValue> 0 </_NewValue> <_Value> 0 </_Value> <_Reset> 0 </_Reset>
               <include src="MCM\Options.xml"/> <rect name="MCM_Back"/> </rect>"#,
        )
        .with(
            "menus\\prefabs\\MCM\\Options.xml",
            r#"<rect name="MCM_Options">
               <hotrect name="Option1"> <_optionID> 1 </_optionID> <_title> </_title> <_enable> 0 </_enable> </hotrect>
               <hotrect name="Option2"> <_optionID> 2 </_optionID> <_title> </_title> <_enable> 0 </_enable> </hotrect>
               </rect>"#,
        )
}

fn game(f: &Fixture) -> Game<'_> {
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    g.new_game();
    let src = files();
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g, Box::new(src), paths);
    g
}

fn title(g: &Game) -> String {
    g.ui_tree.get_string("StartMenu/MCM/MCM_Options/Option1/_title").unwrap_or_default()
}

#[test]
fn an_mcm_menu_script_runs_against_the_start_menu() {
    let f = fixture();
    assert!(f.data.script(FormId(SCRIPT)).is_some(), "the script loads from its source");
    let mut g = game(&f);
    let quest = InstKey::Quest { quest: FormId(QUEST) };
    let shown = |g: &Game| g.var(quest, 1).num();

    // In play the block does not run, and there is no start menu.
    g.frames(5);
    assert_eq!(shown(&g), 0.0);
    assert!(g.ui_tree.menu("StartMenu").is_none());

    // The start menu opens: its file is built with MCM's include, and the block runs,
    // but MCM shows another mod (0).
    g.menu = Some(menus::START);
    let report = ui_loading::sync(&mut g, true, false);
    assert!(report.changes.iter().any(|c| c.starts_with("built StartMenu")), "{:?}", report.changes);
    assert!(g.ui_tree.find_tile("StartMenu/MCM/MCM_Options/Option2").is_some());
    g.frames(5);
    assert_eq!((shown(&g), title(&g)), (0.0, String::new()));

    // MCM picks this mod (its load-order index, 1): the script fills in its option.
    assert!(g.ui_tree.set_float("StartMenu/MCM/_ActiveMod", 1.0));
    g.frames(3);
    assert!(shown(&g) >= 1.0);
    assert_eq!(title(&g), "Test option");
    assert_eq!(g.ui_tree.get_float("StartMenu/MCM/MCM_Options/Option1/_enable"), Some(1.0));
    assert_eq!(g.ui_tree.get_float("StartMenu/MCM/MCM_Options/Option2/_enable"), Some(0.0));

    // A value entered in MCM's scale: the script takes it and answers.
    g.ui_tree.set_float("StartMenu/MCM/_Value", 42.0);
    g.ui_tree.set_float("StartMenu/MCM/_NewValue", 1.0);
    g.frames(2);
    assert_eq!(g.var(quest, 2).num(), 42.0);
    assert_eq!(g.ui_tree.get_float("StartMenu/MCM/_NewValue"), Some(0.0));
    assert_eq!(g.ui_tree.get_float("StartMenu/MCM/_Reset"), Some(1.0));

    // The menu closes: the block stops, and the start menu is dropped.
    g.menu = None;
    ui_loading::sync(&mut g, false, false);
    let before = shown(&g);
    g.frames(5);
    assert_eq!(shown(&g), before);
    assert!(g.ui_tree.menu("StartMenu").is_none());
}

/// MCM's tiles in its file order (menu-xml.md §10): Options, Scale, List, Trigger, ModList;
/// written for the test.
fn list_files() -> MemSource {
    let options = (1..=2).map(|i| format!(r#"<hotrect name="Option{i}"> <depth> 13 </depth> <image name="image"> <depth> 12 </depth> </image> </hotrect>"#)).collect::<String>();
    let items = (1..=3).map(|i| format!(r#"<hotrect name="Item{i}"> <alpha> 0 </alpha> <text name="text"> <string> </string> </text> </hotrect>"#)).collect::<String>();
    let mods = (1..=23).map(|i| format!(r#"<hotrect name="Mod{i}"> <_enable> 0 </_enable> <_modIndex> 0 </_modIndex> <_modIndex2> 0 </_modIndex2> <_subID> 0 </_subID> <text name="text"> <string> </string> <width> {} </width> </text> </hotrect>"#, 10 + i)).collect::<String>();
    let subs = (1..=10).map(|i| format!(r#"<hotrect name="SubMenu{i}"> <_enable> 0 </_enable> <text name="text"> <string> </string> <width> {} </width> </text> </hotrect>"#, 100 + i)).collect::<String>();
    MemSource::default()
        .with(r"menus\options\start_menu.xml", r#"<menu name="StartMenu"> <class> &StartMenu; </class> <include src="includes_StartMenu.xml"/> </menu>"#)
        .with(r"menus\prefabs\includes_StartMenu.xml", r#"<include src="MCM\MCM.xml"/>"#)
        .with(
            r"menus\prefabs\MCM\MCM.xml",
            &format!(
                r#"<rect name="MCM"> <rect name="MCM_Options"> {options} <hotrect name="ArrowUp"> <depth> 13 </depth> </hotrect> </rect>
                   <rect name="MCM_Scale"/> <rect name="MCM_List"> <alpha> 255 </alpha> {items} </rect> <hotrect name="MCM_Trigger"/>
                   <rect name="MCM_ModList"> {mods} {subs} <hotrect name="ArrowUp"> <y> 5 </y> </hotrect> </rect> </rect>"#
            ),
        )
}

#[test]
fn mcms_list_functions_set_its_rows_as_the_original_did() {
    use fnv_game::ui_functions::call;
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    g.new_game();
    let src = list_files();
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g, Box::new(src), paths);
    g.menu = Some(menus::START);
    ui_loading::sync(&mut g, true, false);
    let n = |v: f64| XVal::Num(v);
    let s = |v: &str| XVal::Str(v.to_string());
    let get = |g: &Game, p: &str| g.ui_tree.get_float(&format!("StartMenu/MCM/{p}"));
    let text = |g: &Game, p: &str| g.ui_tree.get_string(&format!("StartMenu/MCM/{p}")).unwrap_or_default();

    // [observed: job mcm976e, menu-xml.md §10.2] `3 1 2 "alpha" 77`: Item1 and Item2, not
    // Item3, not MCM_List; `3 3 2`: nothing; `3 0 1`: Item1 only; a missing trait stays
    // missing; a path below the row reaches its child.
    call(&mut g, "SetMCMFloatMass", &[n(3.0), n(1.0), n(2.0), s("alpha"), n(77.0)]).unwrap().unwrap();
    assert_eq!([get(&g, "MCM_List/Item1/alpha"), get(&g, "MCM_List/Item2/alpha"), get(&g, "MCM_List/Item3/alpha"), get(&g, "MCM_List/alpha")], [Some(77.0), Some(77.0), Some(0.0), Some(255.0)]);
    call(&mut g, "SetMCMFloatMass", &[n(3.0), n(3.0), n(2.0), s("alpha"), n(55.0)]);
    assert_eq!([get(&g, "MCM_List/Item2/alpha"), get(&g, "MCM_List/Item3/alpha")], [Some(77.0), Some(0.0)]);
    call(&mut g, "SetMCMFloatMass", &[n(3.0), n(0.0), n(1.0), s("alpha"), n(66.0)]);
    assert_eq!([get(&g, "MCM_List/Item1/alpha"), get(&g, "MCM_List/Item2/alpha"), get(&g, "MCM_List/alpha")], [Some(66.0), Some(77.0), Some(255.0)]);
    call(&mut g, "SetMCMFloatMass", &[n(3.0), n(1.0), n(2.0), s("_nvfm"), n(7.0)]);
    assert_eq!(get(&g, "MCM_List/Item1/_nvfm"), None);
    call(&mut g, "SetMCMFloatMass", &[n(1.0), n(1.0), n(2.0), s("image/depth"), n(21.0)]);
    assert_eq!([get(&g, "MCM_Options/Option1/image/depth"), get(&g, "MCM_Options/Option2/image/depth"), get(&g, "MCM_Options/Option1/depth")], [Some(21.0), Some(21.0), Some(13.0)]);

    // SetMCMStringMass: the same rows, the text formatted by the VM before (here plain).
    call(&mut g, "SetMCMStringMass", &[n(3.0), n(1.0), n(2.0), s("text/string"), s("nvf")]);
    assert_eq!([text(&g, "MCM_List/Item1/text/string"), text(&g, "MCM_List/Item2/text/string"), text(&g, "MCM_List/Item3/text/string")], ["nvf", "nvf", ""]);

    // SetMCMModList row enable modIndex value form: Mod22 gets the name and the three
    // numbers. The form here has no name: the string is emptied as GetName's would be.
    call(&mut g, "SetMCMModList", &[n(22.0), n(1.0), n(6.0), n(5.0), XVal::Ref(0)]);
    assert_eq!([get(&g, "MCM_ModList/Mod22/_enable"), get(&g, "MCM_ModList/Mod22/_modIndex"), get(&g, "MCM_ModList/Mod22/_modIndex2"), get(&g, "MCM_ModList/Mod22/_subID")], [Some(1.0), Some(6.0), Some(5.0), Some(0.0)]);
    call(&mut g, "SetMCMModList", &[n(22.0), n(2.0), n(7.0), n(9.0), XVal::Ref(0)]);
    assert_eq!([get(&g, "MCM_ModList/Mod22/_enable"), get(&g, "MCM_ModList/Mod22/_modIndex2")], [Some(2.0), Some(9.0)]);
    // Past Mod23 nothing changes: SubMenu1, the next child, keeps its own (MCM's loop calls
    // it for every mod; Begin Again's 27 wrote mod names into SubMenu1-4).
    let sub = get(&g, "MCM_ModList/SubMenu1/_enable");
    call(&mut g, "SetMCMModList", &[n(24.0), n(1.0), n(7.0), n(9.0), XVal::Ref(0)]);
    assert_eq!([get(&g, "MCM_ModList/SubMenu1/_enable"), get(&g, "MCM_ModList/SubMenu1/_modIndex")], [sub, None]);

    // GetMCMListWidth: list 1 a mod row's text width, list 2 a sub-menu row's, else 0.
    let width = |g: &mut Game, list: f64, row: f64| match call(g, "GetMCMListWidth", &[n(list), n(row)]) {
        Some(Ok(XVal::Num(w))) => w,
        other => panic!("{other:?}"),
    };
    assert_eq!([width(&mut g, 1.0, 1.0), width(&mut g, 1.0, 23.0), width(&mut g, 2.0, 1.0), width(&mut g, 2.0, 10.0)], [11.0, 33.0, 101.0, 110.0]);
    assert_eq!([width(&mut g, 1.0, 0.0), width(&mut g, 3.0, 1.0), width(&mut g, 1.0, 24.0)], [0.0, 0.0, 0.0]);

    // The shorthands take a path below the tile, as MCM's scripts pass (`"ArrowUp/y"`).
    match call(&mut g, "GetMCMFloat", &[n(5.0), n(0.0), s("ArrowUp/y")]) {
        Some(Ok(XVal::Num(y))) => assert_eq!(y, 5.0),
        other => panic!("{other:?}"),
    }
}

/// A page filled in the guide's way, by place (`*:N` from the first child, from 1)
/// [documented: MCM Guide v6, The Options Panel, Using Lists; docs/spec-notes/mcm.md §2.1]:
/// the extender's four reach MCM's children in file order, JIP's `…Alt` count from the
/// last (hud.md §14.8).
#[test]
fn a_guide_style_page_by_place_reaches_its_rows() {
    use fnv_game::ui_functions::call;
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    g.new_game();
    let src = list_files();
    let paths = src.files.keys().cloned().collect::<Vec<_>>();
    ui_loading::load(&mut g, Box::new(src), paths);
    g.menu = Some(menus::START);
    ui_loading::sync(&mut g, true, false);
    let n = |v: f64| XVal::Num(v);
    let s = |v: &str| XVal::Str(v.to_string());
    let num = |r: Option<Result<XVal, String>>| match r {
        Some(Ok(XVal::Num(v))) => v,
        other => panic!("{other:?}"),
    };
    // `*:1/*:2` is Option2 of MCM_Options, MCM's first child.
    assert_eq!(num(call(&mut g, "SetUIFloat", &[s("StartMenu/MCM/*:1/*:2/depth"), n(20.0)])), 1.0);
    assert_eq!(g.ui_tree.get_float("StartMenu/MCM/MCM_Options/Option2/depth"), Some(20.0));
    assert_eq!(g.ui_tree.get_float("StartMenu/MCM/MCM_Options/Option1/depth"), Some(13.0));
    // `*:3/*:2/text/string` is Item2's text in MCM_List, the third child.
    call(&mut g, "SetUIString", &[s("StartMenu/MCM/*:3/*:2/text/string"), s("Setting 2")]).unwrap().unwrap();
    assert_eq!(g.ui_tree.get_string("StartMenu/MCM/MCM_List/Item2/text/string").as_deref(), Some("Setting 2"));
    call(&mut g, "SetUIStringEx", &[s("StartMenu/MCM/*:3/*:1/text/string"), s("Setting 1")]).unwrap().unwrap();
    assert_eq!(g.ui_tree.get_string("StartMenu/MCM/MCM_List/Item1/text/string").as_deref(), Some("Setting 1"));
    // GetUIFloat reads the same places. Of these five children MCM_ModList, the last, is
    // `*:0`, and `*:5` is none, as `*:8` of the pause column's eight was [observed:
    // menu-xml.md §10.1] (in MCM's own nineteen it is `*:5`).
    assert_eq!(num(call(&mut g, "GetUIFloat", &[s("StartMenu/MCM/*:1/*:2/depth")])), 20.0);
    assert_eq!(num(call(&mut g, "GetUIFloat", &[s("StartMenu/MCM/*:0/ArrowUp/y")])), 5.0);
    assert_eq!(num(call(&mut g, "GetUIFloat", &[s("StartMenu/MCM/*:5/ArrowUp/y")])), -999.0);
    // JIP's from the last, from 0: `*:4` of five is the first, MCM_Options.
    assert_eq!(num(call(&mut g, "GetUIFloatAlt", &[s("StartMenu/MCM/*:4/*:1/depth")])), 20.0);
    // A place past the one before the last is none: GetUIFloat's -999, SetUIFloat 0.
    assert_eq!(num(call(&mut g, "GetUIFloat", &[s("StartMenu/MCM/*:5/*:99/depth")])), -999.0);
    assert_eq!(num(call(&mut g, "SetUIFloat", &[s("StartMenu/MCM/*:9/depth"), n(1.0)])), 0.0);
}

/// JIP Companions Command & Control's `iJIPCCCModIndex` is its own load index once a game
/// is made (`fnv_game::jip_ccc`, docs/spec-notes/mcm.md §4); a load order without CCC, or
/// CCC without the global, is left alone.
#[test]
fn ccc_mod_index_global_is_its_load_index() {
    let glob = |edid: &str| record(b"GLOB", 0, 0x0800, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b's']), sub(b"FLTV", 0f32.to_le_bytes().to_vec())]);
    let build = |name: &str, edid: &str| {
        let plugin = [tes4(), top(b"GLOB", &glob(edid))].concat();
        let plugins = vec![Plugin::parse(&tes4(), "Base.esm".into()).unwrap(), Plugin::parse(&tes4(), "Other.esp".into()).unwrap(), Plugin::parse(&plugin, name.into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
        let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let forms = FormStore::build(&lo, &schema);
        let refs = RefTable::build(&lo, &schema);
        let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);
        (data, forms, refs)
    };
    let value = |name: &str, edid: &str| {
        let (data, forms, refs) = build(name, edid);
        let g = Game::new(&data, World::new(&forms, &refs, SimConfig::default()), GameConfig::default());
        let (id, _) = g.world.forms.global_by_edid(edid).unwrap();
        g.world.global(id).unwrap()
    };
    assert_eq!(value(fnv_game::jip_ccc::PLUGIN, fnv_game::jip_ccc::MOD_INDEX_GLOBAL), 2.0);
    assert_eq!(value("Someone Else.esp", fnv_game::jip_ccc::MOD_INDEX_GLOBAL), 0.0);
    assert_eq!(value(fnv_game::jip_ccc::PLUGIN, "iSomethingElse"), 0.0);
}

/// Clean-room regression for #2270: a mod script filling MCM option rows via chained `*:N`
/// wildcard paths (`SetUIFloat "StartMenu/MCM/*:1/*:1/_enable" 1`,
/// `SetUIStringEx "StartMenu/MCM/*:1/*:3/value/*:1/string" "%k" 18`, `*:9/string`) and
/// MCM's slider popup resolving unprefixed `&sAccept;` / `&sCancel;` via built-in labels.
#[test]
fn positional_wildcard_script_and_unprefixed_scale_entities_resolve() {
    struct StringsSource {
        mem: MemSource,
        strings: fnv_ui::GameStrings,
    }
    impl fnv_ui::Source for StringsSource {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.mem.read(path)
        }
        fn game_string(&self, name: &str) -> Option<String> {
            self.strings.get(name)
        }
        fn game_string_is_built_in(&self, name: &str) -> bool {
            matches!(self.strings.lookup(name), Some((_, fnv_ui::strings::Origin::BuiltIn)))
        }
    }

    const POSITIONAL_SCRIPT: &str = r#"scn McmPositionalTestScript
Begin MenuMode 1013
  if GetUIFloat "StartMenu/MCM/_ActiveMod" != GetSelfModIndex
    return
  endif
  if GetUIFloat "StartMenu/MCM/_Reset"
    SetUIFloat "StartMenu/MCM/_Reset" 0
    SetUIFloat "StartMenu/MCM/*:1/*:1/_enable" 1
    SetUIString "StartMenu/MCM/*:1/*:1/_title" "Row One"
    SetUIFloat "StartMenu/MCM/*:1/*:3/_enable" 1
    SetUIStringEx "StartMenu/MCM/*:1/*:3/value/*:1/string" "%k" 18
    SetUIString "StartMenu/MCM/*:9/string" "Info text"
  endif
End
"#;

    let bytes = script(&[(0, None, st(0x7FFF, &le16(0)))]);
    let mut subs = unit(1, &bytes, &[], &[]);
    subs.push(sub(b"SCTX", POSITIONAL_SCRIPT.as_bytes().to_vec()));
    let scpt = record(b"SCPT", 0, SCRIPT & 0xFF_FFFF, &[vec![sub(b"EDID", z("McmPositionalTestScript"))], subs].concat());
    let data = sub(b"DATA", [vec![1u8, 0, 0, 0], 0.01f32.to_le_bytes().to_vec()].concat());
    let qust = record(b"QUST", 0, QUEST & 0xFF_FFFF, &[sub(b"EDID", z("McmPositionalQuest")), sub(b"SCRI", (SCRIPT & 0xFF_FFFF).to_le_bytes().to_vec()), data]);
    let plugin = [tes4(), top(b"SCPT", &scpt), top(b"QUST", &qust)].concat();
    let plugins = vec![Plugin::parse(&tes4(), "Base.esm".into()).unwrap(), Plugin::parse(&plugin, "PosTest.esp".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let gdata = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);

    let options = (1..=4)
        .map(|i| {
            format!(
                r#"<hotrect name="Option{i}"> <_enable> 0 </_enable> <_title> </_title>
                   <rect name="value"> <text name="text"> <string> </string> </text> <image name="check"/> </rect> </hotrect>"#
            )
        })
        .collect::<String>();
    let mem = MemSource::default()
        .with(r"menus\options\start_menu.xml", r#"<menu name="StartMenu"> <class> &StartMenu; </class> <include src="MCM\MCM.xml"/> </menu>"#)
        .with(
            r"menus\prefabs\MCM\MCM.xml",
            &format!(
                r#"<rect name="MCM"> <_ActiveMod> 0 </_ActiveMod> <_Reset> 0 </_Reset>
                   <rect name="MCM_Options"> {options} </rect>
                   <rect name="MCM_Scale">
                     <hotrect name="Scale_Accept"> <text name="text"> <string> &sAccept; </string> </text> </hotrect>
                     <hotrect name="Scale_Cancel"> <text name="text"> <string> &sCancel; </string> </text> </hotrect>
                   </rect>
                   <rect name="MCM_List"/> <hotrect name="MCM_Trigger"/> <rect name="MCM_ModList"/>
                   <rect name="MCM_BG"/> <rect name="MCM_Bracket"/> <rect name="MCM_ModTitle"/>
                   <text name="MCM_Info"> <string> </string> </text>
                   <text name="MCM_Version"> <string> 1 </string> </text>
                 </rect>"#
            ),
        );
    let paths = mem.files.keys().cloned().collect::<Vec<_>>();
    let src = StringsSource { mem, strings: fnv_ui::GameStrings::from_settings(Vec::<(&str, &str)>::new()) };

    let mut g = Game::new(&gdata, World::new(&forms, &refs, SimConfig::default()), GameConfig::default());
    g.new_game();
    ui_loading::load(&mut g, Box::new(src), paths);
    g.menu = Some(menus::START);
    ui_loading::sync(&mut g, true, false);

    // Unprefixed `&sAccept;` and `&sCancel;` in MCM_Scale expand to built-in labels instead of raw entities.
    assert_eq!(g.ui_tree.get_string("StartMenu/MCM/MCM_Scale/Scale_Accept/text/string").as_deref(), Some("Accept"));
    assert_eq!(g.ui_tree.get_string("StartMenu/MCM/MCM_Scale/Scale_Cancel/text/string").as_deref(), Some("Cancel"));

    // Selecting this mod with `_Reset = 1` populates options via chained `*:N` wildcard paths and `%k`.
    assert!(g.ui_tree.set_float("StartMenu/MCM/_ActiveMod", 1.0));
    assert!(g.ui_tree.set_float("StartMenu/MCM/_Reset", 1.0));
    g.frames(3);
    assert_eq!(g.ui_tree.get_float("StartMenu/MCM/_Reset"), Some(0.0));
    assert_eq!(g.ui_tree.get_float("StartMenu/MCM/MCM_Options/Option1/_enable"), Some(1.0));
    assert_eq!(g.ui_tree.get_string("StartMenu/MCM/MCM_Options/Option1/_title").as_deref(), Some("Row One"));
    assert_eq!(g.ui_tree.get_float("StartMenu/MCM/MCM_Options/Option3/_enable"), Some(1.0));
    assert_eq!(g.ui_tree.get_string("StartMenu/MCM/MCM_Options/Option3/value/text/string").as_deref(), Some("E"));
    assert_eq!(g.ui_tree.get_string("StartMenu/MCM/MCM_Info/string").as_deref(), Some("Info text"));
}

/// Regression for #3916 (Mods page reopen path): unloading `StartMenu` before
/// `OnMenuClose 1013` lets `StartMenu\visible == -999` fire, resets `MCMModList`,
/// and lets UIO rebuild a fresh single `MCM` subtree on each subsequent Pause visit.
#[test]
fn mcm_reopens_cleanly_across_multiple_pause_visits_with_uio() {
    const MCM_QUEST: u32 = 0x0100_0810;
    const MCM_SCRIPT: u32 = 0x0100_0811;
    const MCM_LIST_QUEST: u32 = 0x0100_0812;
    const OPENS_GLOB: u32 = 0x0100_0813;

    const MCM_SOURCE: &str = r#"scn McmRepeatTestScript
short iMenu

Begin MenuMode 1013
  if iMenu == 0 && GetUIFloat "StartMenu/MCM/MCM_Trigger/_clicked" == 1
    set iMenu to 1
    SetUIFloat "StartMenu/MCM/_Menu" 1
    StartQuest MCMModList
  elseif iMenu == 1 && GetUIFloat "StartMenu/MCM/MCM_Trigger/_clicked" == 1
    set iMenu to 0
    SetUIFloat "StartMenu/MCM/_Menu" 0
    ResetQuest MCMModList
  endif
End
"#;

    let bytes = script(&[(0, None, st(0x7FFF, &le16(0)))]);
    let mut subs = unit(1, &bytes, &[(1, "iMenu", 1)], &[]);
    subs.push(sub(b"SCTX", MCM_SOURCE.as_bytes().to_vec()));
    let scpt = record(b"SCPT", 0, MCM_SCRIPT & 0xFF_FFFF, &[vec![sub(b"EDID", z("McmRepeatTestScript"))], subs].concat());
    let mcm_q = record(
        b"QUST",
        0,
        MCM_QUEST & 0xFF_FFFF,
        &[
            sub(b"EDID", z("MCM")),
            sub(b"SCRI", (MCM_SCRIPT & 0xFF_FFFF).to_le_bytes().to_vec()),
            sub(b"DATA", [vec![0x19u8, 0, 0, 0], 0.01f32.to_le_bytes().to_vec()].concat()),
        ],
    );
    let list_q = record(
        b"QUST",
        0,
        MCM_LIST_QUEST & 0xFF_FFFF,
        &[
            sub(b"EDID", z("MCMModList")),
            sub(b"DATA", [vec![0x08u8, 0, 0, 0], 0.01f32.to_le_bytes().to_vec()].concat()),
        ],
    );
    let glob = record(
        b"GLOB",
        0,
        OPENS_GLOB & 0xFF_FFFF,
        &[sub(b"EDID", z("TestMcmOpens")), sub(b"FNAM", vec![b's']), sub(b"FLTV", 0f32.to_le_bytes().to_vec())],
    );
    let plugin = [tes4(), top(b"GLOB", &glob), top(b"SCPT", &scpt), top(b"QUST", &[mcm_q, list_q].concat())].concat();
    let plugins = vec![Plugin::parse(&tes4(), "Base.esm".into()).unwrap(), Plugin::parse(&plugin, "MCMRepeat.esp".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let gdata = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);

    let src = MemSource::default()
        .with(r"menus\options\start_menu.xml", r#"<menu name="StartMenu"><class>&StartMenu;</class><_MCMExt+Init>0</_MCMExt+Init></menu>"#)
        .with_loose(
            r"menus\prefabs\MCM\MCM.xml",
            r#"<rect name="MCM"><_Menu>0</_Menu><hotrect name="MCM_Trigger"><_clicked>0</_clicked></hotrect></rect>"#,
        )
        .with("uio/public/mcm_repeat.txt", "MCM\\MCM.xml::StartMenu\n?MCMRepeat.esp");
    let paths = src.files.keys().cloned().collect::<Vec<_>>();

    let mut g = Game::new(&gdata, World::new(&forms, &refs, SimConfig::default()), GameConfig::default());
    g.new_game();
    ui_loading::load(&mut g, Box::new(src), paths);

    g.run_query(
        r#"Begin Function { }
SetOnMenuOpenEventHandler (begin function {int iMenu}
  if eval GetUIFloat "StartMenu/_MCMExt+Init" == 0
    SetUIFloat "StartMenu/_MCMExt+Init" 1
    set TestMcmOpens to TestMcmOpens + 1
  endif
end) 1 1013
SetOnMenuCloseEventHandler (begin function {int iMenu}
  if eval GetUIFloatAlt "StartMenu\visible" != -999
    return
  endif
  ResetQuest MCMModList
end) 1 1013
End
"#,
        Default::default(),
    )
    .unwrap();

    let count_start_tiles = |g: &Game| -> (usize, usize) {
        let root = g.ui_tree.menu("StartMenu").expect("StartMenu loaded");
        let mcm_children = g
            .ui_tree
            .tile(root)
            .children
            .iter()
            .filter(|&&c| g.ui_tree.is_live(c) && g.ui_tree.tile(c).name.eq_ignore_ascii_case("MCM"))
            .count();
        let mut total = 0;
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if !g.ui_tree.is_live(id) {
                continue;
            }
            total += 1;
            stack.extend(g.ui_tree.tile(id).children.iter().copied());
        }
        (total, mcm_children)
    };

    let mcm_id = FormId(MCM_QUEST);
    let list_id = FormId(MCM_LIST_QUEST);
    let opens_id = FormId(OPENS_GLOB);
    let mut expected_tiles = None;

    for visit in 1..=3 {
        ui_loading::sync(&mut g, true, false);
        let (total_tiles, mcm_children) = count_start_tiles(&g);
        assert_eq!(mcm_children, 1, "visit {visit}: UIO must inject MCM exactly once (no duplicate tiles)");
        if let Some(prev) = expected_tiles {
            assert_eq!(total_tiles, prev, "visit {visit}: StartMenu tile count must stay constant");
        } else {
            expected_tiles = Some(total_tiles);
        }
        assert_eq!(g.ui_tree.get_float("StartMenu/_MCMExt+Init"), Some(0.0), "visit {visit}: fresh StartMenu tree");

        // Enter Mods page: OnMenuOpen 1013 fires, click trigger, MCM opens.
        assert_eq!(g.page_menu_event(menus::START, true), 1);
        assert_eq!(g.world.global(opens_id).unwrap(), visit as f32, "visit {visit}: OnMenuOpen 1013 ran");
        assert!(g.ui_tree.set_float("StartMenu/MCM/MCM_Trigger/_clicked", 1.0));
        g.page_tick(menus::START);
        assert_eq!(g.ui_tree.get_float("StartMenu/MCM/_Menu"), Some(1.0), "visit {visit}: MCM opened");
        assert!(g.quest_state(mcm_id).running, "visit {visit}: MCM quest running");
        assert!(g.quest_state(list_id).running, "visit {visit}: MCMModList started");

        // Second click closes MCM via its script; unload StartMenu, then fire OnMenuClose 1013.
        g.page_tick(menus::START);
        assert_eq!(g.ui_tree.get_float("StartMenu/MCM/_Menu"), Some(0.0), "visit {visit}: MCM closed");
        assert!(g.ui_tree.set_float("StartMenu/MCM/MCM_Trigger/_clicked", 0.0));
        let root = g.ui_tree.menu("StartMenu").unwrap();
        g.ui_tree.unload_tile(root);
        assert_eq!(g.page_menu_event(menus::START, false), 1);
        assert!(!g.quest_state(list_id).running, "visit {visit}: MCMModList stopped by ResetQuest");

        if visit == 2 {
            ui_loading::sync(&mut g, false, false);
            assert!(g.ui_tree.menu("StartMenu").is_none());
            g.frames(5);
        }
    }
}
