//! ySI row icons beside expression-driven list text (`fnv_game::ui_ysi`): UI overhauls such
//! as VUI+ place `ListItemText` with an expression (the font's offset plus the equip
//! square's padding). The icon goes at the text's base place and the text moves past it
//! once, however often the row is written, while the file's expression keeps working.
use fnv_game::game::menus;
use fnv_game::ui_ysi::{self, YsiItemDisplay};
use fnv_ui::{MemSource, Ui};

/// A row shaped like VUI+'s `IM_InventoryListTemplate`: the text's `x` is worked out from
/// a sibling's padding, and its wrap width from its own `x`.
const MENU: &str = r#"<menu name="InventoryMenu"><class>&InventoryMenu;</class>
  <hotrect name="Row">
    <width>400</width>
    <text name="ListItemText">
      <string>10mm Pistol</string>
      <wrapwidth><copy src="parent()" trait="width"/><sub src="me()" trait="x"/></wrapwidth>
      <x><copy>10</copy><add src="sibling(IM_Template_ItemMarker)" trait="_SquarePadding"/></x>
    </text>
    <rect name="IM_Template_ItemMarker"><_SquarePadding>20</_SquarePadding></rect>
  </hotrect>
  <hotrect name="RowLiteral">
    <text name="ListItemText"><x>40</x><string>Item</string></text>
  </hotrect>
</menu>"#;

/// `menus/ySI/ySI.xml` reduced to the traits the spacing reads, with VUI+'s numbers
/// (width 32, space left -5, space right 5).
const YSI: &str = r#"<rect name="ySI">
  <_ySI> 1 </_ySI>
  <_ySIBlanks> 1 </_ySIBlanks>
  <_ySIWidth> 32 </_ySIWidth>
  <_ySIHeight> 32 </_ySIHeight>
  <_ySISpaceLeft> -5 </_ySISpaceLeft>
  <_ySISpaceRight> 5 </_ySISpaceRight>
</rect>
<template name="ySIDefault">
  <image name="ySIImage">
    <filename>interface\icons\Misc_Unknown.dds</filename>
    <width><copy src="ySI" trait="_ySIWidth"/></width>
    <height><copy src="ySI" trait="_ySIHeight"/></height>
    <user0><copy src="ySI" trait="_ySISpaceLeft"/></user0>
    <user1><copy src="ySI" trait="_ySISpaceRight"/></user1>
    <x> 0 </x>
  </image>
</template>
<template name="ySINoTag">
  <image name="ySIImage">
    <filename>interface\icons\Misc_Unknown.dds</filename>
    <width><copy src="ySI" trait="_ySIWidth"/><onlyif src="ySI" trait="_ySIBlanks"/></width>
    <x> 0 </x>
  </image>
</template>"#;

fn tagged() -> YsiItemDisplay {
    YsiItemDisplay { tag: Some("(a01)".into()), icon: Some("interface\\icons\\pistol.dds".into()), template: ui_ysi::YSI_DEFAULT_TEMPLATE.into() }
}

fn menu(with_ysi: bool) -> (Ui, MemSource) {
    let mut src = MemSource::default().with("menus/inv.xml", MENU);
    if with_ysi {
        src = src.with(ui_ysi::YSI_XML_PATH, YSI);
    }
    let mut ui = Ui::new();
    let root = ui.load_menu(&src, "menus/inv.xml").unwrap();
    assert_eq!(ui_ysi::publish_for_menu(&mut ui, &src, root, Some(menus::INVENTORY)), with_ysi);
    ui.update();
    (ui, src)
}

/// Writes the row as the tile adapter does each frame, then works the tree out.
fn frame(ui: &mut Ui, src: &MemSource, row: &str, display: Option<&YsiItemDisplay>) {
    let row = ui.find_tile(row).unwrap();
    ui_ysi::apply_row_icon(ui, src, row, display);
    ui.update();
}

fn num(ui: &Ui, path: &str) -> f64 {
    ui.get_float(path).unwrap_or_else(|| panic!("{path}"))
}

#[test]
fn expression_text_moves_past_the_icon_once() {
    let (mut ui, src) = menu(true);
    assert_eq!(num(&ui, "InventoryMenu/Row/ListItemText/x"), 30.0, "the file's place");
    for _ in 0..5 {
        frame(&mut ui, &src, "InventoryMenu/Row", Some(&tagged()));
        // Text: 30 + (-5) + 32 + 5; the icon: 30 + (-5). Never shifted twice.
        assert_eq!(num(&ui, "InventoryMenu/Row/ListItemText/x"), 62.0);
        assert_eq!(num(&ui, "InventoryMenu/Row/ySIImage/x"), 25.0);
        // What reads the text's place sees the moved one: the wrap is narrower.
        assert_eq!(num(&ui, "InventoryMenu/Row/ListItemText/wrapwidth"), 400.0 - 62.0);
    }
}

#[test]
fn the_file_expression_keeps_working_after_the_shift() {
    let (mut ui, src) = menu(true);
    frame(&mut ui, &src, "InventoryMenu/Row", Some(&tagged()));
    // The equip square's padding changes: the base follows it, and so do both places.
    ui.set_float("InventoryMenu/Row/IM_Template_ItemMarker/_SquarePadding", 40.0);
    ui.update();
    frame(&mut ui, &src, "InventoryMenu/Row", Some(&tagged()));
    assert_eq!(num(&ui, "InventoryMenu/Row/ListItemText/x"), 82.0);
    assert_eq!(num(&ui, "InventoryMenu/Row/ySIImage/x"), 45.0);
}

#[test]
fn a_replaced_expression_is_shifted_again_not_twice() {
    let (mut ui, src) = menu(true);
    frame(&mut ui, &src, "InventoryMenu/Row", Some(&tagged()));
    // A later write gives `x` new operators (a script's or a mod's replacement): 70.
    let text = ui.find_tile("InventoryMenu/Row/ListItemText").unwrap();
    let ops = vec![fnv_ui::expr::Op { kind: fnv_ui::expr::OpKind::Copy, arg: fnv_ui::expr::Arg::Const(fnv_ui::Value::Num(70.0)) }];
    ui.set_trait_expr(text, "x", ops);
    ui.update();
    assert_eq!(num(&ui, "InventoryMenu/Row/ListItemText/x"), 70.0);
    frame(&mut ui, &src, "InventoryMenu/Row", Some(&tagged()));
    frame(&mut ui, &src, "InventoryMenu/Row", Some(&tagged()));
    assert_eq!(num(&ui, "InventoryMenu/Row/ListItemText/x"), 102.0);
    assert_eq!(num(&ui, "InventoryMenu/Row/ySIImage/x"), 65.0);
}

#[test]
fn no_blank_space_puts_untagged_text_back() {
    let (mut ui, src) = menu(true);
    frame(&mut ui, &src, "InventoryMenu/Row", Some(&tagged()));
    ui.set_float("InventoryMenu/ySI/_ySIBlanks", 0.0);
    ui.update();
    frame(&mut ui, &src, "InventoryMenu/Row", None);
    assert_eq!(num(&ui, "InventoryMenu/Row/ListItemText/x"), 30.0);
    frame(&mut ui, &src, "InventoryMenu/Row", Some(&tagged()));
    assert_eq!(num(&ui, "InventoryMenu/Row/ListItemText/x"), 62.0, "a tagged row is still spaced");
}

#[test]
fn literal_text_still_shifts_once() {
    let (mut ui, src) = menu(true);
    for _ in 0..3 {
        frame(&mut ui, &src, "InventoryMenu/RowLiteral", Some(&tagged()));
    }
    assert_eq!(num(&ui, "InventoryMenu/RowLiteral/ListItemText/x"), 72.0);
    assert_eq!(num(&ui, "InventoryMenu/RowLiteral/ySIImage/x"), 35.0);
}

#[test]
fn removing_the_icon_puts_the_text_back() {
    let (mut ui, src) = menu(true);
    frame(&mut ui, &src, "InventoryMenu/Row", Some(&tagged()));
    frame(&mut ui, &src, "InventoryMenu/RowLiteral", Some(&tagged()));
    for row in ["InventoryMenu/Row", "InventoryMenu/RowLiteral"] {
        let t = ui.find_tile(row).unwrap();
        ui_ysi::remove_row_icon(&mut ui, t);
    }
    ui.update();
    assert!(ui.find_tile("InventoryMenu/Row/ySIImage").is_none());
    assert!(ui.find_tile("InventoryMenu/RowLiteral/ySIImage").is_none());
    assert_eq!(num(&ui, "InventoryMenu/Row/ListItemText/x"), 30.0);
    assert_eq!(num(&ui, "InventoryMenu/RowLiteral/ListItemText/x"), 40.0);
}

#[test]
fn without_ysi_the_row_is_untouched() {
    let (mut ui, src) = menu(false);
    let text = ui.find_tile("InventoryMenu/Row/ListItemText").unwrap();
    let before = ui.tile(text).traits.iter().map(|t| (t.name.clone(), t.expr.clone())).collect::<Vec<_>>();
    frame(&mut ui, &src, "InventoryMenu/Row", Some(&tagged()));
    let row = ui.find_tile("InventoryMenu/Row").unwrap();
    ui_ysi::remove_row_icon(&mut ui, row);
    ui.update();
    let after = ui.tile(text).traits.iter().map(|t| (t.name.clone(), t.expr.clone())).collect::<Vec<_>>();
    assert_eq!(before, after, "no private traits, the file's operators as they were");
    assert_eq!(num(&ui, "InventoryMenu/Row/ListItemText/x"), 30.0);
    assert!(ui.find_tile("InventoryMenu/Row/ySIImage").is_none());
}
