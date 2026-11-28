//! The rest of the tile functions (fnv_game::ui_more, ui_cache, ui_hudbar, ui_gradients;
//! task T9), on a synthetic plugin and menu files written for the test (no game data,
//! made-up text): docs/spec-notes/menu-xml.md §9.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig};
use fnv_game::ui_more::call;
use fnv_game::GameData;
use fnv_script::vm::{XKey, XVal};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use fnv_ui::{MemSource, Value};
use serde_json::Value as Json;

const BASE: u32 = 0x800;
const CELL: u32 = 0x801;
const REFERENCE: u32 = 0x802;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let base = top(b"ACTI", &record(b"ACTI", 0, BASE, &[]));
    let reference = record(b"REFR", 0, REFERENCE, &[sub(b"NAME", u32s(&[BASE])), sub(b"DATA", vec![0; 24])]);
    let cell = top(b"CELL", &[record(b"CELL", 0, CELL, &[sub(b"DATA", vec![1, 0])]), group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &reference))].concat());
    // The player's base, for the HUD bars' equipment condition (the player's inventory).
    let npcs = top(b"NPC_", &record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]));
    let plugins = vec![Plugin::parse(&[tes4(), base, npcs, cell].concat(), "Empty.esm".into()).unwrap()];
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
            "menus\\test.xml",
            r#"<menu name="TestMenu"> <rect name="Box"> <width> 200 </width> <alpha> 255 </alpha>
                 <image name="Pic"> <alpha> 10 </alpha> </image>
                 <text name="Words"> <alpha> 10 </alpha> <string> abc </string> </text>
                 <rect name="Inner"> <image name="Pic2"> <alpha> 10 </alpha> <width> <copy src="grandparent()" trait="width"/> </width> </image> </rect>
               </rect>
               <rect name="List">
                 <rect name="item"> <_y> 0 </_y> <height> 10 </height> <string> Pear (3) </string> <_value> 5 </_value> <_sel> 0 </_sel> </rect>
                 <rect name="item"> <_y> 10 </_y> <height> 20 </height> <string> apple </string> <_value> 9 </_value> <_sel> 1 </_sel> </rect>
                 <rect name="item"> <_y> 30 </_y> <height> 10 </height> <string> Melon+ </string> <_value> 1 </_value> <_sel> 0 </_sel> </rect>
                 <image name="scroll"> <height> 99 </height> </image>
               </rect> </menu>"#,
        )
        .with("menus\\hud.xml", r#"<menu name="HUDMainMenu"> <rect name="Corner"> <x> 4 </x> </rect> </menu>"#)
        .with("menus\\prefabs\\mod\\attach.xml", r#"<rect name="Tag"> <text name="Label"> <string> hi </string> </text> </rect>"#)
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig::default());
    let src = files();
    g.ui_tree.load_menu(&src, "menus\\test.xml").unwrap();
    g.ui_tree.load_menu(&src, "menus\\hud.xml").unwrap();
    g.ui_tree.update();
    g.ui_files = Some(Box::new(src));
    g
}

fn s(v: &str) -> XVal {
    XVal::Str(v.to_string())
}

fn x(v: f64) -> XVal {
    XVal::Num(v)
}

fn run(g: &mut Game, name: &str, args: &[XVal]) -> XVal {
    run_on(g, name, None, args)
}

fn run_on(g: &mut Game, name: &str, on: Option<FormId>, args: &[XVal]) -> XVal {
    call(g, name, on, args).expect("a UI function").expect("no error")
}

fn n(v: XVal) -> f64 {
    match v {
        XVal::Num(x) => x,
        other => panic!("not a number: {other:?}"),
    }
}

fn float(g: &Game, path: &str) -> f64 {
    g.ui_tree.get_float(path).unwrap_or(-999.0)
}

fn string(g: &Game, path: &str) -> String {
    g.ui_tree.get_string(path).unwrap_or_default()
}

/// The engine's frame as `tiles::update` has it: the gradients stepped, the tree worked
/// out, the attachments pruned.
fn frame(g: &mut Game, dt: f64) {
    g.ui_gradients.step(&mut g.ui_tree, dt);
    g.ui_tree.update();
    fnv_game::ui_more::prune_attachments(g);
}

fn array(g: &Game, v: &XVal) -> Vec<(XKey, XVal)> {
    let XVal::Array(id) = v else { panic!("not an array: {v:?}") };
    g.ext.array(*id).expect("the array").entries.clone()
}

#[test]
fn gradual_changes_step_on_real_time_and_the_readers_see_them() {
    let f = fixture();
    let mut g = game(&f);
    let w = "TestMenu/Box/width";
    run(&mut g, "SetUIFloatGradual", &[s(w), x(200.0), x(50.0), x(4.0)]);
    assert_eq!(float(&g, w), 200.0, "the start value at once");
    frame(&mut g, 1.0);
    assert_eq!(float(&g, w), 162.5);
    // The tree works out what reads it.
    assert_eq!(float(&g, "TestMenu/Box/Inner/Pic2/width"), 162.5);
    assert_eq!(n(run(&mut g, "GetUIFloatGradualActiveForTile", &[s("TestMenu/Box")])), 1.0);
    assert_eq!(n(run(&mut g, "GetUIFloatGradualActiveForTileTrait", &[s(w)])), 1.0);
    assert_eq!(n(run(&mut g, "GetUIFloatGradualMode", &[s(w)])), 0.0);
    assert_eq!(n(run(&mut g, "GetUIFloatGradualTimer", &[s(w)])), 1.0);
    assert_eq!(n(run(&mut g, "GetUIFloatGradualTimer", &[s(w), x(1.0)])), 3.0);
    frame(&mut g, 3.5);
    assert_eq!(float(&g, w), 50.0, "ends at the end value");
    assert_eq!(n(run(&mut g, "GetUIFloatGradualActiveForTileTrait", &[s(w)])), 0.0);
    assert_eq!(n(run(&mut g, "GetUIFloatGradualMode", &[s(w)])), -1.0);
    assert!(g.ui_gradients.is_empty());

    // A repeating change runs until stopped; the stop with a start sets it.
    let a = "TestMenu/Box/alpha";
    run(&mut g, "SetUIFloatGradual", &[s(a), x(0.0), x(100.0), x(4.0), x(2.0)]);
    frame(&mut g, 2.0);
    assert_eq!(float(&g, a), 100.0);
    frame(&mut g, 3.0);
    assert_eq!(float(&g, a), 50.0);
    assert_eq!(n(run(&mut g, "GetUIFloatGradualMode", &[s(a)])), 2.0);
    run(&mut g, "SetUIFloatGradual", &[s(a), x(7.0)]);
    assert_eq!(float(&g, a), 7.0);
    frame(&mut g, 1.0);
    assert_eq!(float(&g, a), 7.0, "stopped");
    // A trait not there starts nothing.
    assert_eq!(n(run(&mut g, "SetUIFloatGradual", &[s("TestMenu/Box/nope"), x(0.0), x(1.0), x(1.0)])), 0.0);
    assert!(g.ui_gradients.is_empty());
}

#[test]
fn a_change_on_an_unloaded_tile_is_dropped_and_a_rebuilt_tree_forgets_them() {
    let f = fixture();
    let mut g = game(&f);
    run(&mut g, "SetUIFloatGradual", &[s("TestMenu/Box/Pic/alpha"), x(0.0), x(255.0), x(2.0), x(3.0)]);
    let pic = g.ui_tree.find_tile("TestMenu/Box/Pic").unwrap();
    g.ui_tree.unload_tile(pic);
    frame(&mut g, 0.5);
    assert!(g.ui_gradients.is_empty());
    run(&mut g, "SetUIFloatGradual", &[s("TestMenu/Box/alpha"), x(0.0), x(255.0), x(2.0), x(3.0)]);
    fnv_game::ui_more::tree_rebuilt(&mut g);
    assert!(g.ui_gradients.is_empty());
}

#[test]
fn string_reveal_letter_by_letter() {
    let f = fixture();
    let mut g = game(&f);
    let p = "TestMenu/Box/Words/string";
    run(&mut g, "SetUIStringReveal", &[s(p), s("Hello"), x(5.0)]);
    assert_eq!(string(&g, p), "");
    frame(&mut g, 2.0);
    assert_eq!(string(&g, p), "He");
    frame(&mut g, 3.0);
    assert_eq!(string(&g, p), "Hello");
    assert!(g.ui_gradients.is_empty());
    // Inverted, then stopped with a text.
    run(&mut g, "SetUIStringReveal", &[s(p), s("Hello"), x(5.0), x(0.0), x(1.0)]);
    frame(&mut g, 1.0);
    assert_eq!(string(&g, p), "ello");
    run(&mut g, "SetUIStringReveal", &[s(p), s("Done")]);
    frame(&mut g, 1.0);
    assert_eq!(string(&g, p), "Done");
}

#[test]
fn float_multiple_sets_the_tiles_inside_of_a_type() {
    let f = fixture();
    let mut g = game(&f);
    // Images only (type 2): Pic and Pic2, not the text or the rect.
    assert_eq!(n(run(&mut g, "SetUIFloatMultiple", &[s("TestMenu/Box"), s("alpha"), x(33.0), x(2.0)])), 2.0);
    assert_eq!(float(&g, "TestMenu/Box/Pic/alpha"), 33.0);
    assert_eq!(float(&g, "TestMenu/Box/Inner/Pic2/alpha"), 33.0);
    assert_eq!(float(&g, "TestMenu/Box/Words/alpha"), 10.0);
    assert_eq!(float(&g, "TestMenu/Box/alpha"), 255.0, "not the named tile itself");
    // No type: every tile inside that has the trait.
    assert_eq!(n(run(&mut g, "SetUIFloatMultiple", &[s("TestMenu/Box"), s("alpha"), x(1.0)])), 3.0);
    assert_eq!(float(&g, "TestMenu/Box/Words/alpha"), 1.0);
    assert_eq!(n(run(&mut g, "SetUIFloatMultiple", &[s("TestMenu/Nope"), s("alpha"), x(1.0)])), 0.0);
}

#[test]
fn attached_components_go_into_the_hud_root() {
    let f = fixture();
    let mut g = game(&f);
    let actor = Some(FormId(0x14));
    let xml = r#"<rect name="Badge"> <text name="T"> <string> x </string> </text> </rect>"#;
    assert_eq!(n(run_on(&mut g, "AttachUIComponent", actor, &[s("Bip01 Head"), s(xml)])), 1.0);
    let tile = g.ui_tree.find_tile("HUDMainMenu/Badge").expect("in the HUD's root");
    assert_eq!(g.ui_extra.attached.len(), 1);
    assert_eq!(g.ui_extra.attached[0].reference, FormId(0x14));
    assert_eq!(g.ui_extra.attached[0].node, "Bip01 Head");
    assert_eq!(g.ui_extra.attached[0].tile, tile);
    assert_eq!(string(&g, "HUDMainMenu/Badge/T/string"), "x");
    // A file under data\menus.
    assert_eq!(n(run_on(&mut g, "AttachUIXML", actor, &[s("Node"), s("data\\menus\\prefabs\\mod\\attach.xml")])), 1.0);
    assert_eq!(string(&g, "HUDMainMenu/Tag/Label/string"), "hi");
    // Not a single rect, no reference, a missing file: 0.
    assert_eq!(n(run_on(&mut g, "AttachUIComponent", actor, &[s("N"), s("<rect name=\"A\"/><rect name=\"B\"/>")])), 0.0);
    assert_eq!(n(run_on(&mut g, "AttachUIComponent", actor, &[s("N"), s("<text name=\"A\"/>")])), 0.0);
    assert_eq!(n(run(&mut g, "AttachUIComponent", &[s("N"), s(xml)])), 0.0);
    assert_eq!(n(run_on(&mut g, "AttachUIXML", actor, &[s("N"), s("data\\menus\\nope.xml")])), 0.0);
    assert_eq!(g.ui_extra.attached.len(), 2);
}

#[test]
fn attachments_replace_by_reference_node_and_rect_and_are_pruned_each_frame() {
    let f = fixture();
    let mut g = game(&f);
    let reference = FormId(REFERENCE);
    let cell = FormId(CELL);
    g.loaded_cells.insert(cell);
    let other = g.world.place(FormId(BASE), Some(cell), [0.0; 3], [0.0; 3], 1).unwrap();
    let xml = r#"<rect name="Tag"><visible><copy src="parent()" trait="visible"/></visible><x>99</x></rect>"#;
    run_on(&mut g, "AttachUIXML", Some(reference), &[s("Head"), s("menus/prefabs/mod/attach.xml")]);
    let old = g.ui_extra.attached[0].tile;
    run_on(&mut g, "AttachUIComponent", Some(reference), &[s("Head"), s(xml)]);
    assert_eq!(g.ui_extra.attached.len(), 1);
    assert!(!g.ui_tree.tile(old).live);
    let replaced = g.ui_extra.attached[0].tile;
    assert_eq!(g.ui_tree.tile(replaced).get("visible").unwrap().num(), 0.0);
    assert_eq!(g.ui_tree.tile(replaced).get("x").unwrap().num(), 99.0, "the XML's constant x stays until placed");
    run_on(&mut g, "AttachUIComponent", Some(reference), &[s("Arm"), s(xml)]);
    run_on(&mut g, "AttachUIComponent", Some(other), &[s("Head"), s(xml)]);
    run_on(&mut g, "AttachUIComponent", Some(reference), &[s("Head"), s("<rect name=\"Other\"/>")]);
    assert_eq!(g.ui_extra.attached.len(), 4);
    let hud = g.ui_tree.menu("HUDMainMenu").unwrap();
    g.ui_tree.set_trait(hud, "visible", Value::Num(1.0));
    frame(&mut g, 0.0);
    assert_eq!(g.ui_extra.attached.len(), 4);
    for a in &g.ui_extra.attached {
        assert_eq!(g.ui_tree.tile(a.tile).get("visible").unwrap().num(), 0.0);
    }
    let gone: Vec<_> = g.ui_extra.attached.iter().filter(|a| a.reference == reference).map(|a| a.tile).collect();
    g.world.set_deleted(reference, true).unwrap();
    frame(&mut g, 0.0);
    assert_eq!(g.ui_extra.attached.len(), 1);
    assert!(gone.iter().all(|&t| !g.ui_tree.tile(t).live));
    g.world.set_deleted(reference, false).unwrap();
    g.world.set_enabled(reference, false).unwrap();
    run_on(&mut g, "AttachUIComponent", Some(reference), &[s("Head"), s(xml)]);
    frame(&mut g, 0.0);
    assert_eq!(g.ui_extra.attached.len(), 1);
    assert_eq!(g.ui_extra.attached[0].reference, other);
    let remaining = g.ui_extra.attached[0].tile;
    g.loaded_cells.clear();
    frame(&mut g, 0.0);
    assert!(g.ui_extra.attached.is_empty());
    assert!(!g.ui_tree.tile(remaining).live);
    run_on(&mut g, "AttachUIComponent", Some(FormId(0xdead)), &[s("Head"), s(xml)]);
    frame(&mut g, 0.0);
    assert!(g.ui_extra.attached.is_empty());
}

#[test]
fn attachments_are_placed_at_their_node_and_hidden_without_it() {
    use fnv_game::ui_more::{place_attachments, Placement};
    let f = fixture();
    let mut g = game(&f);
    let reference = FormId(REFERENCE);
    g.loaded_cells.insert(FormId(CELL));
    let hud = g.ui_tree.menu("HUDMainMenu").unwrap();
    g.ui_tree.set_trait(hud, "visible", Value::Num(1.0));
    // The XML's own `x` and an expression in `visible` (made-up component).
    let xml = r#"<rect name="Tag"><visible><copy src="parent()" trait="visible"/></visible><x>99</x><text name="T"><x>-10</x><string>t</string></text></rect>"#;
    run_on(&mut g, "AttachUIComponent", Some(reference), &[s("Bip01 Head"), s(xml)]);
    let tile = g.ui_extra.attached[0].tile;
    let num = |g: &Game, t: &str| g.ui_tree.tile(tile).get(t).unwrap().num();
    // Right after attaching, before any placement: x and y are there for scripts (the XML's
    // constant x, a y it did not write made 0).
    assert_eq!(float(&g, "HUDMainMenu/Tag/x"), 99.0);
    assert_eq!(float(&g, "HUDMainMenu/Tag/y"), 0.0);
    frame(&mut g, 0.0);
    assert_eq!(num(&g, "visible"), 0.0, "hidden until placed");

    // The node on screen: the rect's top left at its point, shown, one change reported.
    let mut asked = Vec::new();
    let changed = place_attachments(&mut g, |r, node| {
        asked.push((r, node.to_string()));
        Some((400.0, 300.0))
    });
    assert_eq!(asked, vec![(reference, "Bip01 Head".to_string())], "the node by its case-sensitive name");
    assert_eq!(changed, vec![Placement { reference, node: "Bip01 Head".into(), rect: "Tag".into(), at: Some((400.0, 300.0)) }]);
    assert_eq!((num(&g, "x"), num(&g, "y"), num(&g, "visible")), (400.0, 300.0, 1.0));
    frame(&mut g, 0.0);
    assert_eq!(num(&g, "visible"), 1.0, "the prune leaves a placed rect shown");
    assert_eq!(float(&g, "HUDMainMenu/Tag/T/x"), -10.0, "the children keep their places");

    // Following the node (an animated bone): moved, no state change, no write when still.
    assert!(place_attachments(&mut g, |_, _| Some((410.5, 290.0))).is_empty());
    assert_eq!((num(&g, "x"), num(&g, "y")), (410.5, 290.0));
    let writes = g.ui_tree.trait_write_counts().0;
    place_attachments(&mut g, |_, _| Some((410.5, 290.0)));
    assert_eq!(g.ui_tree.trait_write_counts().0, writes, "a still node writes nothing");

    // A script hides it while it stays on screen: it stays hidden.
    g.ui_tree.set_trait(tile, "visible", Value::Num(0.0));
    place_attachments(&mut g, |_, _| Some((410.5, 290.0)));
    assert_eq!(num(&g, "visible"), 0.0);

    // Off screen, behind the camera or the node not loaded: hidden, reported once, and held
    // hidden through the expression when the HUD's `visible` changes.
    let changed = place_attachments(&mut g, |_, _| None);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].at, None);
    assert!(place_attachments(&mut g, |_, _| None).is_empty());
    g.ui_tree.set_trait(hud, "visible", Value::Num(0.0));
    frame(&mut g, 0.0);
    g.ui_tree.set_trait(hud, "visible", Value::Num(1.0));
    frame(&mut g, 0.0);
    assert_eq!(num(&g, "visible"), 0.0);
    // Back on screen: shown again.
    assert_eq!(place_attachments(&mut g, |_, _| Some((1.0, 2.0))).len(), 1);
    assert_eq!(num(&g, "visible"), 1.0);

    // The tiles above count: a HUD root moved by (5, 7) leaves the rect on the point.
    g.ui_tree.set_trait(hud, "x", Value::Num(5.0));
    g.ui_tree.set_trait(hud, "y", Value::Num(7.0));
    place_attachments(&mut g, |_, _| Some((100.0, 100.0)));
    assert_eq!((num(&g, "x"), num(&g, "y")), (95.0, 93.0));

    // Expressions in the rect's own x and y, whose source changes before the tree is worked
    // out (as before each drawing): the placement holds.
    let moving = r#"<rect name="Moving"><x><copy src="parent()" trait="width"/></x><y><copy src="parent()" trait="height"/></y></rect>"#;
    run_on(&mut g, "AttachUIComponent", Some(reference), &[s("Bip01 Head"), s(moving)]);
    assert!(g.ui_extra.attached.last().is_some(), "the moving rect is attached");
    // Before the first placement, and while the node is not found, the expressions are gone
    // but x and y stay numbers scripts read and write.
    for unresolved in [false, true] {
        if unresolved {
            place_attachments(&mut g, |_, _| None);
        }
        g.ui_tree.set_trait(hud, "width", Value::Num(50.0));
        frame(&mut g, 0.0);
        let x = float(&g, "HUDMainMenu/Moving/x");
        assert_ne!(x, -999.0, "x is still there");
        assert_ne!(x, 50.0, "its expression no longer follows the source");
        assert_ne!(float(&g, "HUDMainMenu/Moving/y"), -999.0, "y is still there");
        assert!(g.ui_tree.set_float("HUDMainMenu/Moving/x", 12.0), "a script's SetUIFloat finds it");
        assert_eq!(float(&g, "HUDMainMenu/Moving/x"), 12.0);
        g.ui_tree.set_trait(hud, "width", Value::Num(60.0));
        frame(&mut g, 0.0);
        assert_eq!(float(&g, "HUDMainMenu/Moving/x"), 12.0, "the script's value holds");
    }
    place_attachments(&mut g, |_, _| Some((300.0, 200.0)));
    for size in [111.0, 222.0] {
        g.ui_tree.set_trait(hud, "width", Value::Num(size));
        g.ui_tree.set_trait(hud, "height", Value::Num(size));
        frame(&mut g, 0.0);
        assert_eq!(float(&g, "HUDMainMenu/Moving/x"), 300.0 - 5.0, "the source changing does not move it");
        assert_eq!(float(&g, "HUDMainMenu/Moving/y"), 200.0 - 7.0);
    }

    // A reference that unloads: the attachment goes, nothing more is asked.
    g.loaded_cells.clear();
    frame(&mut g, 0.0);
    assert!(!g.ui_tree.tile(tile).live);
    assert!(place_attachments(&mut g, |_, _| panic!("no attachment is left")).is_empty());
}

#[test]
fn traits_added_and_value_and_element_types() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(run(&mut g, "AddTraitToTile", &[s("TestMenu/Box"), s("_NewTrait"), x(5.0)])), 1.0);
    assert_eq!(float(&g, "TestMenu/Box/_NewTrait"), 5.0);
    assert_eq!(n(run(&mut g, "AddTraitToTile", &[s("TestMenu/Nope"), s("_t"), x(5.0)])), 0.0);
    assert_eq!(n(run(&mut g, "AddTraitToTile", &[s("TestMenu/Box"), s(""), x(5.0)])), 0.0);
    let tile = g.ui_tree.find_tile("TestMenu/Box").unwrap();
    assert!(g.ui_tree.tile(tile).trait_index("").is_none());
    // 1 a number, 2 a string, 3 an action, -1 none.
    assert_eq!(n(run(&mut g, "GetUIValueType", &[s("TestMenu/Box/width")])), 1.0);
    assert_eq!(n(run(&mut g, "GetUIValueType", &[s("TestMenu/Box/Words/string")])), 2.0);
    assert_eq!(n(run(&mut g, "GetUIValueType", &[s("TestMenu/Box/Inner/Pic2/width")])), 3.0);
    assert_eq!(n(run(&mut g, "GetUIValueType", &[s("TestMenu/Box/nope")])), -1.0);
    // 1 rect, 2 image, 3 text, 5 menu, -1 none.
    assert_eq!(n(run(&mut g, "GetUIElementType", &[s("TestMenu/Box")])), 1.0);
    assert_eq!(n(run(&mut g, "GetUIElementType", &[s("TestMenu/Box/Pic")])), 2.0);
    assert_eq!(n(run(&mut g, "GetUIElementType", &[s("TestMenu/Box/Words")])), 3.0);
    assert_eq!(n(run(&mut g, "GetUIElementType", &[s("TestMenu")])), 5.0);
    assert_eq!(n(run(&mut g, "GetUIElementType", &[s("")])), -1.0);
}

#[test]
fn cursor_inspection_resolves_missing_and_invalid_paths_and_rejects_unloaded_tiles() {
    let f = fixture();
    let mut g = game(&f);
    for args in [vec![], vec![s("Missing/Tile")]] {
        assert_eq!(run(&mut g, "GetUIElementType", &args), x(-1.0));
    }
    run(&mut g, "DumpTileInfo", &[]);
    run(&mut g, "DumpTileInfo", &[x(0.0), s("Missing/Tile")]);
    assert!(g.console_out.is_empty());
    g.ui_pointer.over = g.ui_tree.find_tile("TestMenu/Box/Words");
    assert_eq!(run(&mut g, "GetUIElementType", &[s("TestMenu/Box/Pic")]), x(2.0));
    for path in [None, Some("Missing/Tile")] {
        let args = path.map(|p| vec![s(p)]).unwrap_or_default();
        assert_eq!(run(&mut g, "GetUIElementType", &args), x(3.0));
        let args = path.map(|p| vec![x(0.0), s(p)]).unwrap_or_default();
        run(&mut g, "DumpTileInfo", &args);
        assert!(g.console_out.iter().any(|l| l == "[ Tilename ] : Words"));
        g.console_out.clear();
    }
    run(&mut g, "DumpTileInfo", &[x(0.0), s("TestMenu/Box/Pic")]);
    assert!(g.console_out.iter().any(|l| l == "[ Tilename ] : Pic"));
    g.console_out.clear();
    let tile = g.ui_pointer.over.unwrap();
    g.ui_tree.unload_tile(tile);
    assert_eq!(run(&mut g, "GetUIElementType", &[]), x(-1.0));
    assert_eq!(run(&mut g, "GetUIElementType", &[s("TestMenu/Box/Words")]), x(-1.0));
    run(&mut g, "DumpTileInfo", &[]);
    assert!(g.console_out.is_empty());
}

#[test]
fn sort_list_box_places_the_items_by_their_keys() {
    let f = fixture();
    let mut g = game(&f);
    let ys = |g: &Game| -> Vec<f64> { (0..3).map(|k| float(g, &format!("TestMenu/List/item:{k}/_y"))).collect() };
    // `item:N` counts from the last: item:0 Melon+ (h 10), item:1 apple (h 20), item:2 Pear (3) (h 10).
    // Alphabetic by item name: apple, Melon, Pear.
    assert_eq!(n(run(&mut g, "SortUIListBox", &[s("TestMenu/List"), s("@string")])), 1.0);
    assert_eq!(ys(&g), vec![20.0, 0.0, 30.0]);
    // Selected first, then by value, highest first.
    run(&mut g, "SortUIListBox", &[s("TestMenu/List"), s("-_sel,-#_value")]);
    assert_eq!(ys(&g), vec![30.0, 0.0, 20.0]);
    // Into another trait, by another increment.
    run(&mut g, "SortUIListBox", &[s("TestMenu/List"), s("#_value$_value$height")]);
    assert_eq!(float(&g, "TestMenu/List/item:0/_value"), 1.0);
    // The scroll bar has no `_y` and is left alone.
    assert_eq!(g.ui_tree.get("TestMenu/List/scroll/_y").map(Value::num), None);
    assert_eq!(n(run(&mut g, "SortUIListBox", &[s("TestMenu/Nope"), s("string")])), 0.0);
}

#[test]
fn string_dimensions_by_the_engines_measure() {
    let f = fixture();
    let mut g = game(&f);
    // [design] Headless outputs are deterministic, including through compiled locals.
    let r = run(&mut g, "GetStringUIDimensions", &[s("some words"), x(3.0), x(80.0)]);
    assert_eq!(array(&g, &r).into_iter().map(|(_, v)| v).collect::<Vec<_>>(), vec![x(0.0); 4]);
    let script = "float w\nfloat h\nfloat lines\nlet w := -11\nlet h := -22\nlet lines := -33\nGetStringUIDimensions \"some words\" 3 80 w h lines\nSetFunctionValue (w * 1000000 + h * 1000 + lines)";
    // The outputs packed in one number: a function's own array is let go when it ends.
    assert_eq!(query(&mut g, script), x(0.0), "the sentinels are overwritten with zeros");
    // A stub: 10 units a letter, wrapped at the width, 20 a line in font 3.
    g.ui_extra.measure = Some(Box::new(|font, text, wrap| {
        let w = text.chars().count() as f64 * 10.0;
        let lines = if wrap > 0.0 { (w / wrap).ceil().max(1.0) } else { 1.0 };
        (w.min(if wrap > 0.0 { wrap } else { w }), lines * if font == 3.0 { 20.0 } else { 10.0 }, lines)
    }));
    let r = run(&mut g, "GetStringUIDimensions", &[s("some words"), x(3.0), x(80.0)]);
    let vals: Vec<XVal> = array(&g, &r).into_iter().map(|(_, v)| v).collect();
    assert_eq!(vals, vec![x(0.0), x(80.0), x(40.0), x(2.0)]);
    assert_eq!(query(&mut g, script), x(80_040_002.0));
}

#[test]
fn dump_tile_info_to_the_console_and_an_array() {
    let f = fixture();
    let mut g = game(&f);
    run(&mut g, "DumpTileInfo", &[x(0.0), s("TestMenu/Box/Words")]);
    assert_eq!(g.console_out, vec!["[ Tilename ] : Words".to_string(), "[ alpha ] : 10.000000".into(), "[ string ] : abc".into()]);
    g.console_out.clear();
    run(&mut g, "DumpTileInfo", &[x(1.0), s("TestMenu/Box/Words")]);
    assert!(g.console_out.is_empty(), "to the log, not the console");
    run(&mut g, "DumpTileInfoALL", &[]);
    assert!(g.console_out.iter().any(|l| l == "TestMenu/List/scroll:"));
    assert!(g.console_out.iter().any(|l| l == "HUDMainMenu/Corner:"));
    let r = run(&mut g, "DumpTileInfoToArray", &[s("TestMenu/Box/Words")]);
    let entries = array(&g, &r);
    assert!(entries.contains(&(XKey::Str("Tilename".into()), s("Words"))));
    assert!(entries.contains(&(XKey::Str("alpha".into()), x(10.0))));
    assert!(entries.contains(&(XKey::Str("string".into()), s("abc"))));
    let none = run(&mut g, "DumpTileInfoToArray", &[s("TestMenu/Nope")]);
    assert!(array(&g, &none).is_empty());
}

#[test]
fn inspection_flushes_only_for_the_resolved_hud_tile() {
    use fnv_game::ui_hud::{self, Shown, Texts};
    let f = fixture();
    let mut g = game(&f);
    let texts = Texts {
        radiation_word: String::new(), radiation_value: String::new(),
        xp_label: String::new(), xp_level_up: String::new(),
        needs: Default::default(), sneak: None, prompt_key: None, prompt_verb: None,
        frame: Default::default(),
    };
    ui_hud::defer(&mut g, Shown { root: true, ..Default::default() }, texts, None);
    g.ui_pointer.over = g.ui_tree.menu("HUDMainMenu");
    run(&mut g, "GetUIElementType", &[s("TestMenu/Box")]);
    run(&mut g, "DumpTileInfo", &[x(0.0), s("TestMenu/Box")]);
    assert!(g.ui_hud_pending.is_some(), "explicit other-menu path wins over the HUD cursor");
    g.ui_pointer.over = g.ui_tree.find_tile("TestMenu/Box");
    run(&mut g, "GetUIElementType", &[]);
    run(&mut g, "GetUIElementType", &[s("HUDMainMenu/Missing")]);
    run(&mut g, "DumpTileInfo", &[x(0.0), s("HUDMainMenu/Missing")]);
    assert!(g.ui_hud_pending.is_some());
    assert!(!g.ui_used);
    g.console_out.clear();
    run(&mut g, "DumpTileInfo", &[x(0.0), s("HUDMainMenu")]);
    assert!(g.ui_hud_pending.is_none());
    assert!(g.ui_used);
    assert!(g.console_out.iter().any(|line| line == "[ visible ] : 1.000000"));
    for command in ["GetUIElementType", "DumpTileInfo"] {
        let texts = Texts {
            radiation_word: String::new(), radiation_value: String::new(),
            xp_label: String::new(), xp_level_up: String::new(),
            needs: Default::default(), sneak: None, prompt_key: None, prompt_verb: None,
            frame: Default::default(),
        };
        ui_hud::defer(&mut g, Shown { root: true, ..Default::default() }, texts, None);
        g.ui_used = false;
        g.ui_pointer.over = g.ui_tree.menu("HUDMainMenu");
        let args = if command == "DumpTileInfo" { vec![x(0.0), s("Missing/Tile")] } else { vec![] };
        run(&mut g, command, &args);
        assert!(g.ui_hud_pending.is_none(), "{command} resolved the HUD cursor");
        assert!(g.ui_used);
    }
}

/// Compiles and runs a query through the VM to the host (`Game::run_query`, `nvse::call`).
fn query(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).expect("the query runs")
}

#[test]
fn compiled_scripts_dispatch_each_t9_family_to_the_host() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(query(&mut g, "SetUIFloatGradual \"TestMenu/Box/width\" 0 100 2\nSetFunctionValue (GetUIFloatGradualActiveForTileTrait \"TestMenu/Box/width\")"), x(1.0));
    frame(&mut g, 1.0);
    assert_eq!(float(&g, "TestMenu/Box/width"), 50.0);
    assert_eq!(query(&mut g, "CacheUI 0 \"TestMenu/Box\"\nCacheUIFloat 0 \"alpha\" 42\nSetFunctionValue (CacheUIGetFloat 0 \"alpha\")"), x(42.0));
    assert_eq!(float(&g, "TestMenu/Box/alpha"), 42.0);
    assert_eq!(query(&mut g, "HudBarCreate \"VMBar\" 12 34 0\nHudBarSetAlpha \"VMBar\" 0 77\nSetFunctionValue (HudBarExists \"VMBar\")"), x(1.0));
    let path = g.ui_extra.bars.list[0].path();
    assert_eq!(float(&g, &format!("{path}/Meter/alpha")), 77.0);
    // [observed: fnv_script::table, vm::xcall] SortUIListBox has no opcode and sits in the
    // wiki's "Console Functions" category. On the numbered path its string argument
    // failed to convert before the host was reached. [design] The table now leaves NVSE
    // extender functions unnumbered so compiled calls reach the extender's dispatch.
    assert_eq!(query(&mut g, "SetFunctionValue (SortUIListBox \"TestMenu/List\" \"#_value\")"), x(1.0));
    assert_eq!(float(&g, "TestMenu/List/item:0/_y"), 0.0);
    assert_eq!(float(&g, "TestMenu/List/item:1/_y"), 20.0);
    assert_eq!(float(&g, "TestMenu/List/item:2/_y"), 10.0);
}

#[test]
fn numeric_trait_writes_sanitize_non_finite_values() {
    let f = fixture();
    let mut g = game(&f);
    run(&mut g, "CacheUI", &[x(0.0), s("TestMenu/Box")]);
    for bad in [x(f64::NAN), x(f64::INFINITY), x(f64::NEG_INFINITY), s("nan")] {
        run(&mut g, "SetUIFloatMultiple", &[s("TestMenu/Box"), s("alpha"), bad.clone()]);
        assert_eq!(float(&g, "TestMenu/Box/Pic/alpha"), 0.0);
        run(&mut g, "AddTraitToTile", &[s("TestMenu/Box"), s("_finite"), bad.clone()]);
        assert_eq!(float(&g, "TestMenu/Box/_finite"), 0.0);
        run(&mut g, "CacheUIFloat", &[x(0.0), s("width"), bad.clone()]);
        assert_eq!(float(&g, "TestMenu/Box/width"), 0.0);
        run(&mut g, "CacheUIFloat", &[x(0.0), s("width"), x(10.0)]);
        run(&mut g, "CacheUIModFloat", &[x(0.0), s("width"), bad.clone()]);
        assert_eq!(float(&g, "TestMenu/Box/width"), 10.0);
        run(&mut g, "CacheUIRange", &[s("width"), x(0.0), x(0.0), bad.clone()]);
        assert_eq!(float(&g, "TestMenu/Box/width"), 0.0);
        run(&mut g, "CacheUIRangeMod", &[x(0.0), x(0.0), s("width"), bad]);
        assert_eq!(float(&g, "TestMenu/Box/width"), 0.0);
    }
    let tile = g.ui_tree.find_tile("TestMenu/Box").unwrap();
    g.ui_tree.set_trait(tile, "width", Value::Str("nan".into()));
    run(&mut g, "CacheUIModFloat", &[x(0.0), s("width"), x(1.0)]);
    assert_eq!(float(&g, "TestMenu/Box/width"), 0.0);
    run(&mut g, "CacheUIFloat", &[x(0.0), s("width"), x(f64::MAX)]);
    run(&mut g, "CacheUIRangeMod", &[s("width"), x(0.0), x(0.0), x(f64::MAX)]);
    assert_eq!(float(&g, "TestMenu/Box/width"), 0.0);
}

#[test]
fn hud_bar_element_setters_sanitize_non_finite_values() {
    let f = fixture();
    let mut g = game(&f);
    run(&mut g, "HudBarCreate", &[s("Finite"), x(f64::NAN), x(f64::INFINITY), x(0.0)]);
    let path = g.ui_extra.bars.list[0].path();
    assert_eq!(float(&g, &format!("{path}/x")), 0.0);
    assert_eq!(float(&g, &format!("{path}/y")), 0.0);
    for bad in [x(f64::NAN), x(f64::INFINITY), x(f64::NEG_INFINITY), s("nan")] {
        for name in ["HudBarSetAlpha", "HudBarSetDepth", "HudBarSetIndent", "HudBarSetZoom", "HudBarSetSystemColor", "HudBarSetMeterTrait", "HudBarSetBarTrait"] {
            assert_eq!(n(run(&mut g, name, &[s("Finite"), x(0.0), bad.clone()])), 1.0, "{name}");
        }
        for name in ["HudBarSetSize", "HudBarSetSizeAlt", "HudBarSetPos", "HudBarSetColor"] {
            assert_eq!(n(run(&mut g, name, &[s("Finite"), x(0.0), bad.clone(), bad.clone(), bad.clone()])), 1.0, "{name}");
        }
        run(&mut g, "HudBarSetPos", &[s("Finite"), x(10.0), bad.clone(), bad.clone()]);
        for k in 0..3 {
            run(&mut g, "HudBarSetRotation", &[s("Finite"), x(0.0), x(f64::from(k)), x(15.0)]);
            assert_eq!(n(run(&mut g, "HudBarSetRotation", &[s("Finite"), x(0.0), x(f64::from(k)), bad.clone()])), 1.0);
        }
        for k in 0..2 {
            assert_eq!(n(run(&mut g, "HudBarSetTextTrait", &[s("Finite"), x(0.0), x(f64::from(k)), bad.clone()])), 1.0);
        }
        assert_eq!(g.ui_extra.bars.list[0].elements[0].rotate, [0.0; 3]);
        assert_eq!(g.ui_extra.bars.list[0].elements[2].justify, Some(0.0));
        for name in ["HudBarSetValueFloat", "HudBarSetValueMax", "HudBarSetValuePercentage"] {
            assert_eq!(n(run(&mut g, name, &[s("Finite"), bad.clone()])), -1.0, "{name}");
        }
        assert_eq!(float(&g, &format!("{path}/Text/font")), 0.0);
        for name in ["x", "y", "width", "height", "alpha", "depth", "zoom", "systemcolor", "red", "green", "blue"] {
            assert_eq!(float(&g, &format!("{path}/Meter/{name}")), 0.0, "{name}");
        }
        assert_eq!(g.ui_extra.bars.list[0].bar_trait(0), 0.0);
        assert_eq!(g.ui_extra.bars.list[0].meter[0], 0.0);
    }
}

#[test]
fn cache_ui_by_index_per_mod_and_by_key() {
    let f = fixture();
    let mut g = game(&f);
    g.other.plugin = "first.esp".into();
    assert_eq!(n(run(&mut g, "CacheUI", &[x(20.0), s("TestMenu/Box")])), 1.0);
    run(&mut g, "CacheUIFloat", &[x(20.0), s("width"), x(120.0)]);
    assert_eq!(float(&g, "TestMenu/Box/width"), 120.0);
    run(&mut g, "CacheUIModFloat", &[x(20.0), s("width"), x(5.0)]);
    assert_eq!(n(run(&mut g, "CacheUIGetFloat", &[x(20.0), s("width")])), 125.0);
    // A path under the cached tile, and a trait not there.
    run(&mut g, "CacheUIString", &[x(20.0), s("Words/string"), s("new")]);
    assert_eq!(string(&g, "TestMenu/Box/Words/string"), "new");
    assert_eq!(n(run(&mut g, "CacheUIGetFloat", &[x(20.0), s("nope")])), -999.0);
    // Another mod's cache is its own.
    g.other.plugin = "second.esp".into();
    assert_eq!(n(run(&mut g, "CacheUIGetFloat", &[x(20.0), s("width")])), -999.0);
    // The Alt forms by key, from any mod.
    run(&mut g, "CacheUIAlt", &[s("Combat"), x(0.0), s("TestMenu/Box/Pic")]);
    run(&mut g, "CacheUIAlt", &[s("Combat"), x(1.0), s("TestMenu/Box/Words")]);
    g.other.plugin = "first.esp".into();
    assert_eq!(run(&mut g, "CacheUIGetStringAlt", &[s("combat"), x(1.0), s("string")]), s("new"));
    // Ranges, either argument order for RangeMod (its page and example differ).
    assert_eq!(n(run(&mut g, "CacheUIRangeAlt", &[s("Combat"), s("alpha"), x(0.0), x(1.0), x(50.0)])), 2.0);
    assert_eq!(float(&g, "TestMenu/Box/Pic/alpha"), 50.0);
    run(&mut g, "CacheUIRangeModAlt", &[s("Combat"), s("alpha"), x(0.0), x(1.0), x(2.0)]);
    run(&mut g, "CacheUIRangeModAlt", &[s("Combat"), x(0.0), x(1.0), s("alpha"), x(3.0)]);
    assert_eq!(float(&g, "TestMenu/Box/Words/alpha"), 55.0);
    // Dump to the console, erase.
    run(&mut g, "CacheUIDumpAlt", &[s("Combat"), x(-1.0)]);
    assert_eq!(g.console_out, vec!["CacheUI [0] TestMenu/Box/Pic".to_string(), "CacheUI [1] TestMenu/Box/Words".into()]);
    run(&mut g, "CacheUIEraseAlt", &[s("Combat"), x(0.0)]);
    assert_eq!(n(run(&mut g, "CacheUIGetFloatAlt", &[s("Combat"), x(0.0), s("alpha")])), -999.0);
    assert_eq!(n(run(&mut g, "CacheUIGetFloatAlt", &[s("Combat"), x(1.0), s("alpha")])), 55.0);
    run(&mut g, "CacheUIEraseAlt", &[s("Combat"), x(-1.0)]);
    assert_eq!(n(run(&mut g, "CacheUIGetFloatAlt", &[s("Combat"), x(1.0), s("alpha")])), -999.0);
    // An unloaded tile's entry goes.
    let pic = g.ui_tree.find_tile("TestMenu/Box").unwrap();
    g.ui_tree.unload_tile(pic);
    assert_eq!(n(run(&mut g, "CacheUIGetFloat", &[x(20.0), s("width")])), -999.0);
}

#[test]
fn cache_ui_gradual_delay_lock_and_reveal() {
    let f = fixture();
    let mut g = game(&f);
    run(&mut g, "CacheUI", &[x(0.0), s("TestMenu/Box")]);
    run(&mut g, "CacheUI", &[x(1.0), s("TestMenu/Box/Pic")]);
    // A gradual by index, mode 5 (ease in).
    run(&mut g, "CacheUIGradual", &[x(0.0), s("alpha"), x(0.0), x(100.0), x(2.0), x(5.0)]);
    frame(&mut g, 1.0);
    assert_eq!(float(&g, "TestMenu/Box/alpha"), 25.0);
    // Mode 4 is not built: refused, nothing changes.
    assert_eq!(n(run(&mut g, "CacheUIGradual", &[x(1.0), s("alpha"), x(0.0), x(100.0), x(2.0), x(4.0)])), 0.0);
    assert_eq!(float(&g, "TestMenu/Box/Pic/alpha"), 10.0);
    // A range of indices.
    run(&mut g, "CacheUIRangeGradual", &[x(0.0), x(1.0), s("alpha"), x(0.0), x(10.0), x(1.0)]);
    frame(&mut g, 1.0);
    assert_eq!(float(&g, "TestMenu/Box/alpha"), 10.0);
    assert_eq!(float(&g, "TestMenu/Box/Pic/alpha"), 10.0);
    // A delay: nothing for 2 s, then from the value then to 200 in 1 s; locked (flag 2).
    run(&mut g, "CacheUIGradualDelay", &[x(0.0), s("width"), x(2.0), x(200.0), x(1.0), x(2.0)]);
    frame(&mut g, 1.0);
    assert_eq!(float(&g, "TestMenu/Box/width"), 200.0, "the file's 200, untouched in the delay");
    g.ui_tree.set_float("TestMenu/Box/width", 100.0);
    frame(&mut g, 1.5);
    assert_eq!(float(&g, "TestMenu/Box/width"), 150.0, "from 100 at the delay's end, half a second in");
    // Locked: a gradual with an end is refused, a stop is not.
    assert_eq!(n(run(&mut g, "CacheUIGradual", &[x(0.0), s("width"), x(0.0), x(1.0), x(1.0)])), 0.0);
    run(&mut g, "CacheUIGradual", &[x(0.0), s("width"), x(5.0)]);
    assert_eq!(float(&g, "TestMenu/Box/width"), 5.0);
    frame(&mut g, 1.0);
    assert_eq!(float(&g, "TestMenu/Box/width"), 5.0);
    // A delay watching a trait ends when it changes.
    run(&mut g, "CacheUIGradualDelay", &[x(0.0), s("width"), x(2.0), x(300.0), x(1.0), x(0.0), x(0.0), s("alpha")]);
    g.ui_tree.set_float("TestMenu/Box/alpha", 77.0);
    frame(&mut g, 3.0);
    assert_eq!(float(&g, "TestMenu/Box/width"), 5.0);
    assert!(g.ui_gradients.is_empty());
    // JIP's stop leaves CacheUI's change running.
    run(&mut g, "CacheUIGradual", &[x(0.0), s("width"), x(0.0), x(10.0), x(10.0)]);
    run(&mut g, "SetUIFloatGradual", &[s("TestMenu/Box/width")]);
    frame(&mut g, 1.0);
    assert_eq!(float(&g, "TestMenu/Box/width"), 1.0);
    // A reveal by index.
    run(&mut g, "CacheUIStringReveal", &[x(0.0), s("Words/string"), s("abcd"), x(4.0), x(1.0)]);
    frame(&mut g, 1.0);
    assert_eq!(string(&g, "TestMenu/Box/Words/string"), "d");
}

#[test]
fn cache_ui_ranges_visit_only_the_cached_indices() {
    // Review of T9: a script's range is bounded by the cache, not by its numbers.
    let f = fixture();
    let mut g = game(&f);
    run(&mut g, "CacheUI", &[x(0.0), s("TestMenu/Box")]);
    run(&mut g, "CacheUI", &[x(7.0), s("TestMenu/Box/Pic")]);
    assert_eq!(n(run(&mut g, "CacheUIRange", &[s("alpha"), x(0.0), x(1e15), x(50.0)])), 2.0);
    assert_eq!((float(&g, "TestMenu/Box/alpha"), float(&g, "TestMenu/Box/Pic/alpha")), (50.0, 50.0));
    // Only the indices in the range; a reversed range reaches nothing.
    assert_eq!(n(run(&mut g, "CacheUIRange", &[s("alpha"), x(1.0), x(1e15), x(60.0)])), 1.0);
    assert_eq!((float(&g, "TestMenu/Box/alpha"), float(&g, "TestMenu/Box/Pic/alpha")), (50.0, 60.0));
    assert_eq!(n(run(&mut g, "CacheUIRange", &[s("alpha"), x(7.0), x(0.0), x(70.0)])), 0.0);
    assert_eq!(n(run(&mut g, "CacheUIRangeGradual", &[x(-1e15), x(1e15), s("alpha"), x(0.0), x(10.0), x(1.0)])), 2.0);
}

#[test]
fn a_delayed_change_meets_the_running_one_when_its_delay_ends() {
    // Review of T9: the flag is read when the delay ends too; a finished immediate change
    // is not kept; a NaN time is no time.
    let f = fixture();
    let mut g = game(&f);
    run(&mut g, "CacheUI", &[x(0.0), s("TestMenu/Box")]);
    // Flag 1: after 1 s the delayed change replaces the one running then.
    run(&mut g, "CacheUIGradualDelay", &[x(0.0), s("width"), x(1.0), x(300.0), x(1.0), x(1.0)]);
    run(&mut g, "CacheUIGradual", &[x(0.0), s("width"), x(0.0), x(10.0), x(10.0)]);
    frame(&mut g, 1.5);
    // At the delay's end the running change goes and the delayed one starts from the value
    // then (0 or 1, by the order the two are stepped in that frame): half a second later it
    // is half way from there to 300.
    let w = float(&g, "TestMenu/Box/width");
    assert!((150.0..=150.5).contains(&w), "{w}");
    assert_eq!(g.ui_gradients.list.len(), 1);
    frame(&mut g, 1.0);
    assert_eq!(float(&g, "TestMenu/Box/width"), 300.0);
    // Flag 0: the delayed change gives way to the one running when its delay ends.
    run(&mut g, "CacheUIGradual", &[x(0.0), s("width"), x(0.0), x(10.0), x(10.0)]);
    run(&mut g, "CacheUIGradualDelay", &[x(0.0), s("width"), x(1.0), x(300.0), x(1.0), x(0.0)]);
    frame(&mut g, 1.5);
    assert_eq!(float(&g, "TestMenu/Box/width"), 1.5);
    assert_eq!(g.ui_gradients.list.len(), 1);
    run(&mut g, "CacheUIGradual", &[x(0.0), s("width"), x(5.0)]);
    assert!(g.ui_gradients.is_empty());
    // A locked change with no time to run is done as written and locks nothing.
    run(&mut g, "CacheUIGradualDelay", &[x(0.0), s("width"), x(0.0), x(7.0), x(0.0), x(2.0)]);
    assert_eq!(float(&g, "TestMenu/Box/width"), 7.0);
    assert!(g.ui_gradients.is_empty());
    assert_eq!(n(run(&mut g, "CacheUIGradual", &[x(0.0), s("width"), x(0.0), x(1.0), x(1.0)])), 1.0);
    // A NaN duration or value: no time, 0 as a value; the trait never holds a NaN.
    run(&mut g, "CacheUIGradual", &[x(0.0), s("width"), x(0.0), x(9.0), x(f64::NAN)]);
    assert_eq!(float(&g, "TestMenu/Box/width"), 9.0);
    assert!(g.ui_gradients.is_empty());
    run(&mut g, "SetUIFloatGradual", &[s("TestMenu/Box/width"), x(f64::NAN)]);
    assert_eq!(float(&g, "TestMenu/Box/width"), 0.0);
}

#[test]
fn cache_ui_queues() {
    let f = fixture();
    let mut g = game(&f);
    run(&mut g, "CUICreateQueue", &[x(0.0)]);
    for v in [0.0, 25.0, 20.0, 9.0] {
        run(&mut g, "CUIQueueFloat", &[x(0.0), x(v)]);
    }
    assert_eq!(n(run(&mut g, "CUIGetQueueSize", &[x(0.0)])), 4.0);
    run(&mut g, "CUIQueueCycle", &[x(0.0), x(1.0)]);
    let q = |g: &mut Game| -> Vec<f64> { (0..4).map(|k| n(run(g, "CUIGetQueue", &[x(0.0), x(k as f64)]))).collect() };
    assert_eq!(q(&mut g), vec![25.0, 20.0, 9.0, 0.0]);
    run(&mut g, "CUIQueueCycle", &[x(0.0), x(0.0)]);
    assert_eq!(q(&mut g), vec![0.0, 25.0, 20.0, 9.0]);
    // Called again, a value goes; forced add with duplicates refused, without allowed.
    run(&mut g, "CUIQueueFloat", &[x(0.0), x(25.0)]);
    assert_eq!(q(&mut g), vec![0.0, 20.0, 9.0, -1.0]);
    run(&mut g, "CUIQueueFloat", &[x(0.0), x(9.0), x(1.0), x(1.0)]);
    assert_eq!(n(run(&mut g, "CUIGetQueueSize", &[x(0.0)])), 3.0);
    run(&mut g, "CUIQueueFloat", &[x(0.0), x(9.0), x(0.0), x(1.0)]);
    assert_eq!(n(run(&mut g, "CUIGetQueueSize", &[x(0.0)])), 4.0);
    run(&mut g, "CUIQueueFloat", &[x(0.0), x(9.0), x(0.0), x(0.0)]);
    assert_eq!(n(run(&mut g, "CUIGetQueueSize", &[x(0.0)])), 3.0);
    assert_eq!(n(run(&mut g, "CUIGetQueue", &[x(5.0), x(0.0)])), -1.0);
}

#[test]
fn hud_bars_made_of_tiles_in_the_hud() {
    let f = fixture();
    let mut g = game(&f);
    g.other.plugin = "bars.esp".into();
    assert_eq!(n(run(&mut g, "HudBarCreate", &[s("Health"), x(10.0), x(100.0), x(0.0)])), 1.0);
    assert_eq!(n(run(&mut g, "HudBarCreate", &[s("Health"), x(0.0), x(0.0), x(0.0)])), 0.0, "the mod has one of the name");
    assert_eq!(n(run(&mut g, "HudBarExists", &[s("health")])), 1.0);
    let path = match run(&mut g, "HudBarGetElementUIPath", &[s("Health"), x(10.0)]) {
        XVal::Str(p) => p,
        other => panic!("{other:?}"),
    };
    assert!(g.ui_tree.find_tile(&path).is_some(), "a rect in HUDMainMenu: {path}");
    let meter = format!("{path}/Meter");
    // Hidden until shown; placed where made.
    assert_eq!(float(&g, &format!("{path}/visible")), 0.0);
    assert_eq!(float(&g, &format!("{path}/x")), 10.0);
    run(&mut g, "HudBarShowBar", &[s("Health"), x(1.0)]);
    assert_eq!(float(&g, &format!("{path}/visible")), 1.0);
    assert_eq!(n(run(&mut g, "HudBarGetVisible", &[s("Health"), x(10.0)])), 1.0);
    // The meter's width from the value: indent 30 plus half of the rest of 330.
    run(&mut g, "HudBarSetTexture", &[s("Health"), x(0.0), s("mybars\\line.dds")]);
    run(&mut g, "HudBarSetSize", &[s("Health"), x(0.0), x(330.0), x(20.0)]);
    run(&mut g, "HudBarSetIndent", &[s("Health"), x(0.0), x(30.0)]);
    run(&mut g, "HudBarSetValueMax", &[s("Health"), x(200.0)]);
    assert_eq!(n(run(&mut g, "HudBarSetValueFloat", &[s("Health"), x(100.0)])), 1.0);
    assert_eq!(float(&g, &format!("{meter}/width")), 180.0);
    assert_eq!(string(&g, &format!("{meter}/filename")), "mybars\\line.dds");
    run(&mut g, "HudBarSetValuePercentage", &[s("Health"), x(2.0)]);
    assert_eq!(float(&g, &format!("{meter}/width")), 330.0, "normalized to 100 %");
    // Look: alpha for the images (10 all), colour, depth, position, rotation, text.
    run(&mut g, "HudBarSetAlpha", &[s("Health"), x(10.0), x(120.0)]);
    assert_eq!(float(&g, &format!("{path}/Frame/alpha")), 120.0);
    assert_eq!(float(&g, &format!("{path}/Text/alpha")), 255.0, "the Text is not an image");
    run(&mut g, "HudBarSetColor", &[s("Health"), x(2.0), x(1.0), x(2.0), x(3.0)]);
    assert_eq!(float(&g, &format!("{path}/Text/green")), 2.0);
    assert_eq!(float(&g, &format!("{path}/Meter/depth")), 100.0);
    run(&mut g, "HudBarSetPos", &[s("Health"), x(2.0), x(5.0), x(9.0)]);
    assert_eq!(float(&g, &format!("{path}/Text/y")), 9.0);
    run(&mut g, "HudBarSetRotation", &[s("Health"), x(0.0), x(0.0), x(1.5)]);
    assert_eq!(float(&g, &format!("{meter}/rotateangle")), 1.5);
    run(&mut g, "HudBarSetTextString", &[s("Health"), x(0.0), s("my bar")]);
    assert_eq!(string(&g, &format!("{path}/Text/string")), "my bar");
    run(&mut g, "HudBarSetTextTrait", &[s("Health"), x(0.0), x(1.0), x(2.0)]);
    assert!(matches!(g.ui_tree.get(&format!("{path}/Text/justify")), Some(Value::Sym(c)) if c == "center"));
    // Auto position: the Text right of the Meter.
    run(&mut g, "HudBarSetAutoPos", &[s("Health")]);
    assert_eq!(float(&g, &format!("{path}/Text/x")), 330.0);
    // Bar traits kept and read back; 1 / -1.
    assert_eq!(n(run(&mut g, "HudBarGetBarTrait", &[s("Health"), x(2.0)])), 0.1);
    assert_eq!(n(run(&mut g, "HudBarSetBarTrait", &[s("Health"), x(23.0), x(1.0)])), 1.0);
    assert_eq!(n(run(&mut g, "HudBarGetBarTrait", &[s("Health"), x(23.0)])), 1.0);
    assert_eq!(n(run(&mut g, "HudBarSetBarTrait", &[s("Nope"), x(23.0), x(1.0)])), -1.0);
    // Other bar types: value text, percentage, alpha.
    run(&mut g, "HudBarCreate", &[s("Weight"), x(0.0), x(0.0), x(3.0)]);
    run(&mut g, "HudBarSetTextString", &[s("Weight"), x(1.0), s("Carry:")]);
    run(&mut g, "HudBarSetTextString", &[s("Weight"), x(2.0), s(" wg")]);
    run(&mut g, "HudBarSetMeterTrait", &[s("Weight"), x(2.0), x(1.0)]);
    run(&mut g, "HudBarSetValueMax", &[s("Weight"), x(210.0)]);
    run(&mut g, "HudBarSetValueFloat", &[s("Weight"), x(50.0)]);
    let wpath = match run(&mut g, "HudBarGetElementUIPath", &[s("Weight"), x(2.0)]) {
        XVal::Str(p) => p,
        other => panic!("{other:?}"),
    };
    assert_eq!(string(&g, &format!("{wpath}/string")), "Carry:50/210 wg");
    assert_eq!(n(run(&mut g, "HudBarSetValuePercentage", &[s("Weight"), x(0.5)])), -1.0, "not for type 3");
    run(&mut g, "HudBarCreate", &[s("Pct"), x(0.0), x(0.0), x(4.0)]);
    run(&mut g, "HudBarSetTextString", &[s("Pct"), x(2.0), s("%")]);
    run(&mut g, "HudBarSetValuePercentage", &[s("Pct"), x(0.444)]);
    let ppath = match run(&mut g, "HudBarGetElementUIPath", &[s("Pct"), x(2.0)]) {
        XVal::Str(p) => p,
        other => panic!("{other:?}"),
    };
    assert_eq!(string(&g, &format!("{ppath}/string")), "44%");
    run(&mut g, "HudBarCreate", &[s("Fade"), x(0.0), x(0.0), x(2.0)]);
    run(&mut g, "HudBarSetMeterTrait", &[s("Fade"), x(0.0), x(50.0)]);
    run(&mut g, "HudBarSetValueFloat", &[s("Fade"), x(50.0)]);
    let fpath = match run(&mut g, "HudBarGetElementUIPath", &[s("Fade"), x(0.0)]) {
        XVal::Str(p) => p,
        other => panic!("{other:?}"),
    };
    assert_eq!(float(&g, &format!("{fpath}/alpha")), 152.5);
    // Another mod sees only its own; the console takes the first of the name.
    g.other.plugin = "other.esp".into();
    assert_eq!(n(run(&mut g, "HudBarExists", &[s("Health")])), 0.0);
    let own = run(&mut g, "HudBarGetAllBars", &[]);
    assert!(array(&g, &own).is_empty());
    let all = run(&mut g, "HudBarGetAllBars", &[x(1.0)]);
    let all = array(&g, &all);
    assert_eq!(array(&g, &all[0].1).len(), 4);
    g.other.plugin = String::new();
    assert_eq!(n(run(&mut g, "HudBarExists", &[s("Health")])), 1.0);
    // Destroyed: the tiles go.
    g.other.plugin = "bars.esp".into();
    run(&mut g, "HudBarDestroy", &[s("Health")]);
    assert!(g.ui_tree.find_tile(&path).is_none());
    run(&mut g, "HudBarDestroyAllBars", &[]);
    assert!(g.ui_extra.bars.list.is_empty());
    // Not this file's: the script-variable value (`nvse_sup7`, tests/sup7.rs).
    assert!(call(&mut g, "HudBarSetValueScriptVar", None, &[s("x"), x(1.0)]).is_none());
}

#[test]
fn hud_bars_are_made_again_in_a_rebuilt_tree() {
    let f = fixture();
    let mut g = game(&f);
    run(&mut g, "HudBarCreate", &[s("Kept"), x(1.0), x(2.0), x(0.0)]);
    run(&mut g, "HudBarCreate", &[s("Gone"), x(1.0), x(2.0), x(0.0)]);
    run(&mut g, "HudBarSetBarTrait", &[s("Gone"), x(1.0), x(1.0)]);
    // A new tree, as a load builds.
    let src = files();
    let mut ui = fnv_ui::Ui::new();
    ui.load_menu(&src, "menus\\hud.xml").unwrap();
    g.ui_tree = ui;
    fnv_game::ui_more::tree_rebuilt(&mut g);
    assert_eq!(g.ui_extra.bars.list.len(), 1);
    let path = g.ui_extra.bars.list[0].path();
    assert_eq!(float(&g, &format!("{path}/y")), 2.0);
}

/// The frame's bar step alone (`Game::tick` calls it after the scripts).
fn bar_frame(g: &mut Game, dt: f64) {
    g.cfg.frame_seconds = dt;
    fnv_game::ui_hudbar::tick(g);
}

fn visible(g: &mut Game, bar: &str) -> f64 {
    n(run(g, "HudBarGetVisible", &[s(bar), x(10.0)]))
}

#[test]
fn hud_bar_conditions_follow_the_players_state() {
    let f = fixture();
    let mut g = game(&f);
    let p = FormId::PLAYER_REF;
    run(&mut g, "HudBarCreate", &[s("C"), x(10.0), x(20.0), x(0.0)]);
    let path = g.ui_extra.bars.list[0].path();
    // Hidden by HudBarShowBar: 0, whatever the conditions.
    run(&mut g, "HudBarSetBarTrait", &[s("C"), x(27.0), x(2.0)]);
    g.senses.first_person = true;
    assert_eq!(visible(&mut g, "C"), 0.0);
    // Shown, hidden in first person: -1 at once, nothing drawn, no frame of it shown.
    run(&mut g, "HudBarShowBar", &[s("C"), x(1.0)]);
    assert_eq!(visible(&mut g, "C"), -1.0);
    assert_eq!(float(&g, &format!("{path}/visible")), 0.0);
    // Third person: 1, drawn after the frame.
    g.senses.first_person = false;
    assert_eq!(visible(&mut g, "C"), 1.0);
    bar_frame(&mut g, 0.1);
    assert_eq!(float(&g, &format!("{path}/visible")), 1.0);
    run(&mut g, "HudBarSetBarTrait", &[s("C"), x(27.0), x(0.0)]);
    // Each condition read from the game.
    type Set = Box<dyn Fn(&mut Game, bool)>;
    let checks: Vec<(i64, f64, Set)> = vec![
        (21, 1.0, Box::new(|g, on| g.player.controls_disabled[0] = on)),
        (22, 1.0, Box::new(move |g, on| {
            g.senses.motion.insert(p, fnv_game::functions::senses::Motion { sneaking: on, ..Default::default() });
        })),
        (24, 2.0, Box::new(|g, on| g.jip2.told.sights_up = !on)),
        (26, 1.0, Box::new(move |g, on| g.extra.entry(p).or_default().weapon_out = on)),
        (26, 2.0, Box::new(move |g, on| g.extra.entry(p).or_default().weapon_out = !on)),
        (0, 1.0, Box::new(|g, on| g.menu = on.then_some(1002))),
    ];
    for (k, v, set) in &checks {
        run(&mut g, "HudBarSetBarTrait", &[s("C"), x(*k as f64), x(*v)]);
        set(&mut g, true);
        assert_eq!(visible(&mut g, "C"), -1.0, "trait {k} {v} hides");
        bar_frame(&mut g, 0.1);
        assert_eq!(float(&g, &format!("{path}/visible")), 0.0, "trait {k} {v} not drawn");
        set(&mut g, false);
        assert_eq!(visible(&mut g, "C"), 1.0, "trait {k} {v} shows again");
        bar_frame(&mut g, 0.1);
        assert_eq!(float(&g, &format!("{path}/visible")), 1.0, "trait {k} {v} drawn again");
        run(&mut g, "HudBarSetBarTrait", &[s("C"), x(*k as f64), x(0.0)]);
    }
    // Aiming is the sights up as the engine tells it: Block held with a melee weapon, or
    // with a gun while a reload holds the sights down, is Block held (`told.aiming`, as
    // IsAiming reads it) without the sights up, so not aiming.
    run(&mut g, "HudBarSetBarTrait", &[s("C"), x(24.0), x(2.0)]);
    g.jip2.told.aiming = true;
    g.jip2.told.sights_up = false;
    assert_eq!(visible(&mut g, "C"), -1.0, "blocking or reloading: not aiming");
    g.jip2.told.sights_up = true;
    assert_eq!(visible(&mut g, "C"), 1.0, "the sights up");
}

#[test]
fn hud_bar_item_condition_follows_equipment() {
    let f = fixture();
    let mut g = game(&f);
    let (gun, vest, list) = (FormId(0x900), FormId(0x901), FormId(0x950));
    run(&mut g, "HudBarCreate", &[s("Ammo"), x(0.0), x(0.0), x(0.0)]);
    run(&mut g, "HudBarShowBar", &[s("Ammo"), x(1.0)]);
    run(&mut g, "HudBarSetBarTrait", &[s("Ammo"), x(25.0), x(1.0)]);
    // Neither an item nor a list: hidden.
    assert_eq!(visible(&mut g, "Ammo"), -1.0);
    run(&mut g, "HudBarSetItem", &[s("Ammo"), XVal::Ref(gun.0)]);
    assert_eq!(visible(&mut g, "Ammo"), -1.0, "carried, not equipped");
    let inv = g.world.inventory_mut(FormId::PLAYER_REF).unwrap();
    inv.add(gun, 1);
    inv.add(vest, 1);
    inv.set_equipped(gun);
    assert_eq!(visible(&mut g, "Ammo"), 1.0);
    // The item removed (no form): hidden again though the gun is equipped.
    run(&mut g, "HudBarSetItem", &[s("Ammo")]);
    assert_eq!(visible(&mut g, "Ammo"), -1.0);
    // A form list: any member equipped; an empty one hides.
    g.form_lists.insert(list, Vec::new());
    run(&mut g, "HudBarSetFormList", &[s("Ammo"), XVal::Ref(list.0)]);
    assert_eq!(visible(&mut g, "Ammo"), -1.0, "an empty list");
    g.form_lists.insert(list, vec![FormId(0x999), vest]);
    assert_eq!(visible(&mut g, "Ammo"), -1.0, "the vest not equipped");
    g.world.inventory_mut(FormId::PLAYER_REF).unwrap().set_equipped(vest);
    assert_eq!(visible(&mut g, "Ammo"), 1.0);
    // Item and list together: either is enough.
    run(&mut g, "HudBarSetItem", &[s("Ammo"), XVal::Ref(0x998)]);
    assert_eq!(visible(&mut g, "Ammo"), 1.0);
    g.world.inventory_mut(FormId::PLAYER_REF).unwrap().remove(vest, 1);
    assert_eq!(visible(&mut g, "Ammo"), -1.0, "the vest gone, the item not equipped");
    // The trait off: shown, item or not.
    run(&mut g, "HudBarSetBarTrait", &[s("Ammo"), x(25.0), x(0.0)]);
    assert_eq!(visible(&mut g, "Ammo"), 1.0);
}

#[test]
fn hud_bars_vanish_and_appear_gradually() {
    let f = fixture();
    let mut g = game(&f);
    let p = FormId::PLAYER_REF;
    run(&mut g, "HudBarCreate", &[s("F"), x(100.0), x(20.0), x(0.0)]);
    run(&mut g, "HudBarShowBar", &[s("F"), x(1.0)]);
    for (k, v) in [(22.0, 1.0), (41.0, 1.0), (42.0, 1.0), (43.0, 40.0), (44.0, 1.0), (61.0, 1.0), (62.0, 2.0), (63.0, -10.0), (64.0, 0.5)] {
        run(&mut g, "HudBarSetBarTrait", &[s("F"), x(k), x(v)]);
    }
    let path = g.ui_extra.bars.list[0].path();
    let meter = format!("{path}/Meter/alpha");
    bar_frame(&mut g, 0.25);
    assert_eq!(float(&g, &meter), 255.0);
    // Sneaking: -1 at once, the bar fading out over 1 s to the right.
    g.senses.motion.insert(p, fnv_game::functions::senses::Motion { sneaking: true, ..Default::default() });
    assert_eq!(visible(&mut g, "F"), -1.0);
    bar_frame(&mut g, 0.25);
    assert_eq!(float(&g, &meter), 191.25);
    assert_eq!(float(&g, &format!("{path}/x")), 110.0);
    assert_eq!(float(&g, &format!("{path}/visible")), 1.0, "still drawn while it vanishes");
    bar_frame(&mut g, 0.25);
    assert_eq!(float(&g, &format!("{path}/x")), 120.0);
    // Standing up half way: back from half, at the appearing speed, moving on from where
    // it is (no jump to the appearing shift's line).
    g.senses.motion.insert(p, fnv_game::functions::senses::Motion::default());
    assert_eq!(visible(&mut g, "F"), 1.0);
    bar_frame(&mut g, 0.125);
    assert_eq!(float(&g, &meter), 191.25);
    assert_eq!(float(&g, &format!("{path}/x")), 110.0);
    assert_eq!(float(&g, &format!("{path}/y")), 20.0);
    bar_frame(&mut g, 1.0);
    assert_eq!(float(&g, &meter), 255.0);
    assert_eq!(float(&g, &format!("{path}/x")), 100.0);
    assert_eq!(float(&g, &format!("{path}/y")), 20.0);
    // Gone all the way: not drawn.
    g.senses.motion.insert(p, fnv_game::functions::senses::Motion { sneaking: true, ..Default::default() });
    bar_frame(&mut g, 2.0);
    assert_eq!(float(&g, &format!("{path}/visible")), 0.0);
    // Appearing from hidden comes from the appearing shift (63: 10 above).
    g.senses.motion.insert(p, fnv_game::functions::senses::Motion::default());
    bar_frame(&mut g, 0.25);
    assert_eq!(float(&g, &format!("{path}/x")), 100.0);
    assert_eq!(float(&g, &format!("{path}/y")), 15.0);
    assert_eq!(float(&g, &meter), 127.5);
    // Only the fade's traits change during it: a width set behind the bar's back stays.
    let width = format!("{path}/Meter/width");
    let t = g.ui_tree.find_tile(&format!("{path}/Meter")).unwrap();
    g.ui_tree.set_trait(t, "width", Value::Num(77.0));
    bar_frame(&mut g, 0.125);
    assert_eq!(float(&g, &width), 77.0);
    assert_eq!(float(&g, &format!("{path}/y")), 17.5);
    // Hidden by a camera condition (27) and shown again: at once both ways.
    run(&mut g, "HudBarSetBarTrait", &[s("F"), x(27.0), x(2.0)]);
    g.senses.first_person = true;
    bar_frame(&mut g, 0.01);
    assert_eq!(float(&g, &format!("{path}/visible")), 0.0);
    g.senses.first_person = false;
    bar_frame(&mut g, 0.01);
    assert_eq!(float(&g, &meter), 255.0, "no fade in after the camera condition");
    assert_eq!(float(&g, &format!("{path}/visible")), 1.0);
    // The fades go on in menu mode (the engine runs `Game::tick` there; under the pause
    // menu it runs none and they wait).
    g.senses.motion.insert(p, fnv_game::functions::senses::Motion { sneaking: true, ..Default::default() });
    g.menu = Some(1002);
    bar_frame(&mut g, 0.5);
    assert_eq!(float(&g, &meter), 127.5);
    g.menu = None;
    // A tree built again finishes the change at once.
    let src = files();
    let mut ui = fnv_ui::Ui::new();
    ui.load_menu(&src, "menus\\hud.xml").unwrap();
    g.ui_tree = ui;
    fnv_game::ui_more::tree_rebuilt(&mut g);
    assert_eq!(float(&g, &format!("{path}/visible")), 0.0);
}

#[test]
fn authored_tile_smoke_scripts_compile_against_the_function_table() {
    let f = fixture();
    let g = game(&f);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/smoke/fixtures");
    for path in [
        "ui-dimensions-home/mods/NVSE/plugins/Scripts/gr_nvf_dimensions.txt",
        "ui-cursor-home/mods/NVSE/plugins/Scripts/gr_nvf_cursor.txt",
        "ui-cursor-home/mods/NVSE/user_defined_functions/nvf_cursor_probe.txt",
        "ui-typewriter-home/mods/NVSE/plugins/Scripts/gr_nvf_typewriter.txt",
        "ui-typewriter-home/mods/NVSE/user_defined_functions/nvf_type_original.txt",
        "ui-typewriter-home/mods/NVSE/user_defined_functions/nvf_type_replacement.txt",
        "ui-typewriter-home/mods/NVSE/user_defined_functions/nvf_type_controller.txt",
        "ui-hudbar-home/mods/NVSE/plugins/Scripts/gr_nvf_hudbar.txt",
        "ui-hudbar-home/mods/NVSE/user_defined_functions/nvf_hudbar_controller.txt",
        "ui-hudbar-home/mods/NVSE/user_defined_functions/nvf_hudbar_value.txt",
    ] {
        let src = std::fs::read_to_string(root.join(path)).unwrap();
        let compiled = fnv_script::compile::compile(&src, &g.data.table, &fnv_game::game::GameEnv { g: &g }, None);
        let errors: Vec<_> = compiled.errors().collect();
        assert!(errors.is_empty(), "{path}: {errors:?}");
    }
}


#[test]
fn compiled_projection_source_command_reaches_typed_state_without_writing_ui_traits() {
    let f=fixture();let mut g=game(&f);let before=g.ui_tree.trait_write_counts();
    let ctx=fnv_game::game::RunCtx {self_ref:Some(FormId(REFERENCE)),..Default::default()};
    let value=g.run_query("ref target\nBegin Function { }\nlet target := GetSelf\ntarget.ProjectUITile \"TestMenu/Box\" \"screen\" -10 -20 80 40\nSetFunctionValue 7\nEnd\n",ctx).unwrap();
    assert_eq!(value,x(7.));assert_eq!(g.ui_tree.trait_write_counts(),before);
    let requests:Vec<_>=g.ui_extra.projections.source_requests(&g.ui_tree).collect();assert_eq!(requests.len(),1);
    assert_eq!(requests[0].0.reference,FormId(REFERENCE));assert_eq!(requests[0].1.source,g.ui_tree.find_tile("TestMenu/Box").unwrap());
    assert_eq!((requests[0].1.viewport.x,requests[0].1.viewport.y,requests[0].1.viewport.width,requests[0].1.viewport.height),(-10.,-20.,80,40));
    drop(requests);
    let value=g.run_query("Begin Function { }\nProjectUITile \"TestMenu/Box\" \"screen\" 0 0 1 1\nSetFunctionValue 9\nEnd\n",fnv_game::game::RunCtx {self_ref:Some(FormId(REFERENCE)),..Default::default()}).unwrap();
    assert_eq!(value,x(9.));assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(),0);
}

#[test]
fn projection_source_route_ignores_missing_reference_and_real_tree_reset_clears_requests() {
    let f=fixture();let mut g=game(&f);let args=[s("TestMenu/Box"),s("screen"),x(0.),x(0.),x(80.),x(40.)];
    assert_eq!(fnv_game::nvse::call(&mut g,&Default::default(),"projectuitile",None,&args).unwrap(),Ok(x(0.)));
    assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(),0);
    assert_eq!(fnv_game::nvse::call(&mut g,&Default::default(),"projectuitile",Some(FormId(REFERENCE)),&args).unwrap(),Ok(x(0.)));
    assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(),1);
    fnv_game::ui_more::tree_rebuilt(&mut g);assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(),0);
}

#[test]
fn project_ui_tile_returns_zero_and_continues_across_all_seven_observed_cases() {
    let f = fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx { self_ref: Some(FormId(REFERENCE)), ..Default::default() };

    // Helper: runs ProjectUITile under strict GameConfig and returns (ret, ok_sentinel).
    // If ProjectUITile aborted the script, run_query would return Err and unwrap would panic.
    let run_case = |g: &mut Game<'_>, call_line: &str, run_ctx: fnv_game::game::RunCtx| -> (XVal, XVal) {
        let script = format!(
            "ref rNull\nfloat fRet\nint ok\nBegin Function {{ }}\nlet rNull := 0\nlet fRet := -9\nlet ok := 0\n{call_line}\nlet ok := 1\nSetFunctionValue (fRet * 10 + ok)\nEnd\n"
        );
        let encoded = g.run_query(&script, run_ctx).unwrap();
        let ret = g.run_query(
            &format!("ref rNull\nfloat fRet\nBegin Function {{ }}\nlet rNull := 0\nlet fRet := -9\n{call_line}\nSetFunctionValue fRet\nEnd\n"),
            run_ctx,
        ).unwrap();
        (ret, encoded)
    };

    // 1. valid (64x64) -> returns 0.0, continues (ok=1), enqueues 64x64 request.
    let (r, enc) = run_case(&mut g, "let fRet := ProjectUITile \"TestMenu/Box\" \"screen\" 0 0 64 64", ctx);
    assert_eq!((r, enc), (x(0.), x(1.)));
    assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(), 1);

    // 2. 1x1 withdrawal -> returns 0.0, continues (ok=1), withdraws active request.
    let (r, enc) = run_case(&mut g, "let fRet := ProjectUITile \"TestMenu/Box\" \"screen\" 0 0 1 1", ctx);
    assert_eq!((r, enc), (x(0.), x(1.)));
    assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(), 0);

    // 3. missing tile path -> returns 0.0, continues (ok=1), enqueues nothing.
    let (r, enc) = run_case(&mut g, "let fRet := ProjectUITile \"NoSuchMenu/NoSuchTile\" \"screen\" 0 0 64 64", ctx);
    assert_eq!((r, enc), (x(0.), x(1.)));
    assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(), 0);

    // 4. 0x0 viewport -> returns 0.0, continues (ok=1), enqueues nothing.
    let (r, enc) = run_case(&mut g, "let fRet := ProjectUITile \"TestMenu/Box\" \"screen\" 0 0 0 0", ctx);
    assert_eq!((r, enc), (x(0.), x(1.)));
    assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(), 0);

    // 5. negative -10x-10 viewport -> returns 0.0, continues (ok=1), enqueues nothing.
    let (r, enc) = run_case(&mut g, "let fRet := ProjectUITile \"TestMenu/Box\" \"screen\" 0 0 -10 -10", ctx);
    assert_eq!((r, enc), (x(0.), x(1.)));
    assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(), 0);

    // 6. fractional 64.7x64.7 viewport -> returns 0.0, continues (ok=1), enqueues rounded 65x65 request.
    let (r, enc) = run_case(&mut g, "let fRet := ProjectUITile \"TestMenu/Box\" \"screen\" 0 0 64.7 64.7", ctx);
    assert_eq!((r, enc), (x(0.), x(1.)));
    {
        let reqs: Vec<_> = g.ui_extra.projections.source_requests(&g.ui_tree).collect();
        assert_eq!(reqs.len(), 1);
        assert_eq!((reqs[0].1.viewport.width, reqs[0].1.viewport.height), (65, 65));
    }

    // 7. null ref (rNull = 0): returns 0.0, continues (ok=1), does NOT enqueue or fall back to self_ref,
    //    and leaves the existing valid projection untouched.
    let (r, enc) = run_case(&mut g, "let fRet := rNull.ProjectUITile \"TestMenu/Box\" \"other\" 0 0 64 64", ctx);
    assert_eq!((r, enc), (x(0.), x(1.)));
    let (r_no_self, enc_no_self) = run_case(&mut g, "let fRet := rNull.ProjectUITile \"TestMenu/Box\" \"other\" 0 0 64 64", fnv_game::game::RunCtx::default());
    assert_eq!((r_no_self, enc_no_self), (x(0.), x(1.)));
    let reqs: Vec<_> = g.ui_extra.projections.source_requests(&g.ui_tree).collect();
    assert_eq!(reqs.len(), 1, "null-ref call must neither enqueue on null nor fall back to self_ref");
    assert_eq!(reqs[0].0.block, "screen");
}

#[test]
fn corrected_projection_catalogue_metadata_drives_the_real_source_vm() {
    let f = fixture();
    let command = f.data.table.unnumbered.get("projectuitile").expect("source command is catalogued");
    assert_eq!(command.params.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(),
        vec!["string", "string", "float", "float", "float", "float"]);
    assert!(command.params.iter().all(|p| !p.optional));
    assert_eq!(f.data.table.by_name("ProjectUITile"), None, "no native opcode is admitted");
    let mut g = game(&f);
    let before = g.ui_tree.trait_write_counts();
    let context = fnv_game::game::RunCtx { self_ref: Some(FormId(REFERENCE)), ..Default::default() };
    let returned = g.run_query("ref target\nfloat returned\nBegin Function { }\nlet target := GetSelf\nlet returned := target.ProjectUITile \"TestMenu/Box\" \"SCREEN\" -7 11 80 40\nSetFunctionValue returned\nEnd\n", context).unwrap();
    assert_eq!(returned, x(0.));
    assert_eq!(g.ui_tree.trait_write_counts(), before);
    {
        let requests: Vec<_> = g.ui_extra.projections.source_requests(&g.ui_tree).collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0.reference, FormId(REFERENCE));
        assert_eq!(requests[0].0.block, "screen");
        assert_eq!(requests[0].1.source, g.ui_tree.find_tile("TestMenu/Box").unwrap());
        let viewport = requests[0].1.viewport;
        assert_eq!((viewport.x, viewport.y, viewport.width, viewport.height), (-7., 11., 80, 40));
    }
    let old_stub_shape = [x(1.), x(2.), x(3.), x(4.), x(5.), x(6.)];
    assert!(fnv_game::nvse::call(&mut g, &context, "projectuitile", Some(FormId(REFERENCE)), &old_stub_shape).unwrap().is_err());
    assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(), 1);
    let withdrawn = g.run_query("float returned\nBegin Function { }\nlet returned := ProjectUITile \"missing\" \"screen\" 0 0 1 1\nSetFunctionValue returned\nEnd\n", context).unwrap();
    assert_eq!(withdrawn, x(0.));
    assert_eq!(g.ui_extra.projections.source_requests(&g.ui_tree).count(), 0);
    assert_eq!(g.ui_tree.trait_write_counts(), before);
}
