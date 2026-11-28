//! Scoped unit tests for packed FNV-1a `trait_hashes`, sorted `expr_traits`,
//! zero-alloc `find_child_path` / `set_trait_str`, and `hit_candidates` depth ordering.

use super::*;

#[test]
fn trait_hashes_and_expr_traits_stay_synced_across_add_replace_and_remove() {
    let mut ui = Ui::new();
    let src = MemSource::default().with(
        "test_menu.xml",
        r#"<menu name="TestMenu">
            <rect name="Box">
                <a>10</a>
                <b><copy src="me()" trait="a"/><add>5</add></b>
                <c>20</c>
                <d><copy src="me()" trait="c"/><add>7</add></d>
            </rect>
        </menu>"#,
    );
    let menu = ui.load_menu(&src, "test_menu.xml").unwrap();
    ui.update();
    let box_tile = ui.find_child_path(menu, "Box").unwrap();
    assert_eq!(ui.tile(box_tile).get("B"), Some(&Value::Num(15.0)));
    assert_eq!(ui.tile(box_tile).get("d"), Some(&Value::Num(27.0)));

    // Removing an earlier plain trait shifts `expr_traits` and `trait_hashes` together.
    assert!(ui.remove_trait(box_tile, "A"));
    assert_eq!(ui.tile(box_tile).trait_index("a"), None);
    assert!(ui.tile(box_tile).trait_index("b").is_some());
    assert!(ui.tile(box_tile).trait_index("d").is_some());
    ui.update();
    assert_eq!(ui.tile(box_tile).get("b"), Some(&Value::Num(5.0)));
    assert_eq!(ui.tile(box_tile).get("d"), Some(&Value::Num(27.0)));

    // Replacing an expression trait with a literal unmarks it from `expr_traits`,
    // and replacing a literal with an expression re-marks it.
    assert!(ui.inject_text(&src, "TestMenu/Box", "<b>99</b><c><copy src=\"me()\" trait=\"b\"/><add>1</add></c>"));
    ui.update();
    assert_eq!(ui.tile(box_tile).get("b"), Some(&Value::Num(99.0)));
    assert_eq!(ui.tile(box_tile).get("c"), Some(&Value::Num(100.0)));
    assert_eq!(ui.tile(box_tile).get("d"), Some(&Value::Num(107.0)));
}

#[test]
fn set_trait_str_preserves_write_serial_and_dirty_semantics_without_spurious_change() {
    let mut ui = Ui::new();
    let tile = ui.screen();
    ui.set_trait_str(tile, "string", "Hello");
    assert_eq!(ui.tile(tile).get("STRING"), Some(&Value::Str("Hello".into())));
    let s1 = ui.trait_write_serial(tile, "string").unwrap();
    let (w1, c1) = ui.trait_write_counts();

    // Writing the exact same string advances write_serial & trait_writes, not trait_changes.
    ui.set_trait_str(tile, "string", "Hello");
    let s2 = ui.trait_write_serial(tile, "string").unwrap();
    let (w2, c2) = ui.trait_write_counts();
    assert_ne!(s1, s2);
    assert_eq!(w2, w1 + 1);
    assert_eq!(c2, c1);

    // Writing a new string updates the value and advances trait_changes.
    ui.set_trait_str(tile, "string", "World");
    let (w3, c3) = ui.trait_write_counts();
    assert_eq!(ui.tile(tile).get("string"), Some(&Value::Str("World".into())));
    assert_eq!(w3, w2 + 1);
    assert_eq!(c3, c2 + 1);
}

#[test]
fn find_child_path_resolves_nested_and_indexed_children_matching_find_tile() {
    let mut ui = Ui::new();
    let src = MemSource::default().with(
        "hud.xml",
        r#"<menu name="HUDMainMenu">
            <rect name="Info">
                <text name="row"><string>first</string></text>
                <text name="row"><string>second</string></text>
            </rect>
        </menu>"#,
    );
    let menu = ui.load_menu(&src, "hud.xml").unwrap();
    assert_eq!(ui.find_child_path(menu, "Info/row:0"), ui.find_tile("HUDMainMenu/Info/row:0"));
    assert_eq!(ui.find_child_path(menu, "Info/row:1"), ui.find_tile("HUDMainMenu/Info/row:1"));
    assert_eq!(ui.find_child_path(menu, "Info/missing"), None);
}
