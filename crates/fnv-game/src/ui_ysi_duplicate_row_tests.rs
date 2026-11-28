//! Scoped regression tests for `ui_ysi::apply_row_icon` and `ui_ysi::sync_row_icon_x`
//! covering duplicate-named list rows, delimiter-bearing item names, multi-`ySIImage`
//! deduplication, and first-frame expression-driven `ListItemText/x` alignment.

use crate::game::menus;
use crate::ui_ysi::{
    apply_row_icon, publish_for_menu, sync_row_icon_x, YsiItemDisplay, YSI_DEFAULT_TEMPLATE,
    YSI_IMAGE_TILE, YSI_XML_PATH,
};
use fnv_ui::{MemSource, TileId, Ui, Value};

const YSI_XML: &str = r#"
<rect name="ySI">
  <_ySI> 1 </_ySI>
  <_ySIBlanks> 1 </_ySIBlanks>
  <_ySIWidth> 32 </_ySIWidth>
  <_ySIHeight> 32 </_ySIHeight>
  <_ySISpaceUp> 6 </_ySISpaceUp>
  <_ySISpaceLeft> -5 </_ySISpaceLeft>
  <_ySISpaceRight> 5 </_ySISpaceRight>
</rect>
<template name="ySIDefault">
  <image name="ySIImage">
    <filename> interface\icons\Misc_Unknown.dds </filename>
    <width> <copy src="ySI" trait="_ySIWidth"/> </width>
    <height> <copy src="ySI" trait="_ySIHeight"/> </height>
    <user0> <copy src="ySI" trait="_ySISpaceLeft"/> </user0>
    <user1> <copy src="ySI" trait="_ySISpaceRight"/> </user1>
    <x> 0 </x>
    <y> <copy src="ySI" trait="_ySISpaceUp"/> </y>
    <visible> 1 </visible>
  </image>
</template>
<template name="ySINoTag">
  <image name="ySIImage">
    <filename> interface\icons\Misc_Unknown.dds </filename>
    <width> <copy src="ySI" trait="_ySIWidth"/> <onlyif src="ySI" trait="_ySIBlanks"/> </width>
    <height> <copy src="ySI" trait="_ySIHeight"/> </height>
    <user0> <copy src="ySI" trait="_ySISpaceLeft"/> </user0>
    <user1> <copy src="ySI" trait="_ySISpaceRight"/> </user1>
    <x> 0 </x>
    <y> <copy src="ySI" trait="_ySISpaceUp"/> </y>
    <visible> 1 </visible>
  </image>
</template>
"#;

const MENU_XML: &str = r#"
<menu name="InventoryMenu">
  <class> &InventoryMenu; </class>
  <hotrect name="IM_InventoryList"/>
  <template name="IM_InventoryListTemplate">
    <hotrect name="IM_InventoryListTemplateRect">
      <text name="ListItemText">
        <x> <copy> 36 </copy> </x>
        <string> Item </string>
      </text>
    </hotrect>
  </template>
</menu>
"#;

fn live_ysi_images(ui: &Ui, row: TileId) -> Vec<TileId> {
    ui.tile(row)
        .children
        .iter()
        .copied()
        .filter(|&c| ui.is_live(c) && ui.tile(c).name.eq_ignore_ascii_case(YSI_IMAGE_TILE))
        .collect()
}

#[test]
fn duplicate_named_sibling_rows_each_get_one_icon_and_never_pile_up_on_first_row() {
    let src = MemSource::default()
        .with("menus/inv.xml", MENU_XML)
        .with(YSI_XML_PATH, YSI_XML);
    let mut ui = Ui::new();
    let inv = ui.load_menu(&src, "menus/inv.xml").unwrap();
    assert!(publish_for_menu(&mut ui, &src, inv, Some(menus::INVENTORY)));
    ui.update();

    // Three sibling rows sharing the exact same display name (e.g. three Varmint Rifles
    // at different conditions in Pip-Boy Weapons).
    let list = "InventoryMenu/IM_InventoryList";
    let r0 = ui
        .add_code_tile_from_template(&src, list, "IM_InventoryListTemplate", Some("Varmint Rifle"))
        .unwrap();
    let r1 = ui
        .add_code_tile_from_template(&src, list, "IM_InventoryListTemplate", Some("Varmint Rifle"))
        .unwrap();
    let r2 = ui
        .add_code_tile_from_template(&src, list, "IM_InventoryListTemplate", Some("Varmint Rifle"))
        .unwrap();

    let d0 = YsiItemDisplay {
        tag: Some("(fbb1)".into()),
        icon: Some("interface\\icons\\WeapA_Rifle.dds".into()),
        template: YSI_DEFAULT_TEMPLATE.into(),
    };
    let d1 = YsiItemDisplay {
        tag: Some("(fbb2)".into()),
        icon: Some("interface\\icons\\WeapA_Rifle_SemiAuto.dds".into()),
        template: YSI_DEFAULT_TEMPLATE.into(),
    };
    let d2 = YsiItemDisplay {
        tag: Some("(fbb5)".into()),
        icon: Some("interface\\icons\\WeapA_Rifle_Sniper.dds".into()),
        template: YSI_DEFAULT_TEMPLATE.into(),
    };

    // Apply across multiple refresh passes to prove repeated updates never stack icons on r0.
    for _ in 0..3 {
        assert!(apply_row_icon(&mut ui, &src, r0, Some(&d0)));
        assert!(apply_row_icon(&mut ui, &src, r1, Some(&d1)));
        assert!(apply_row_icon(&mut ui, &src, r2, Some(&d2)));
        ui.update();
    }

    for (row, expected_icon) in [
        (r0, "interface\\icons\\WeapA_Rifle.dds"),
        (r1, "interface\\icons\\WeapA_Rifle_SemiAuto.dds"),
        (r2, "interface\\icons\\WeapA_Rifle_Sniper.dds"),
    ] {
        let imgs = live_ysi_images(&ui, row);
        assert_eq!(imgs.len(), 1, "each duplicate-named row must own exactly one ySIImage");
        assert_eq!(
            ui.tile(imgs[0]).get("filename").map(Value::text).as_deref(),
            Some(expected_icon)
        );
    }
}

#[test]
fn delimiter_bearing_row_names_attach_directly_without_path_splitting() {
    let src = MemSource::default()
        .with("menus/inv.xml", MENU_XML)
        .with(YSI_XML_PATH, YSI_XML);
    let mut ui = Ui::new();
    let inv = ui.load_menu(&src, "menus/inv.xml").unwrap();
    assert!(publish_for_menu(&mut ui, &src, inv, Some(menus::INVENTORY)));
    ui.update();

    let list = "InventoryMenu/IM_InventoryList";
    let slash_row = ui
        .add_code_tile_from_template(
            &src,
            list,
            "IM_InventoryListTemplate",
            Some("T-51b Armor (BOS/Outcast)"),
        )
        .unwrap();
    let colon_row = ui
        .add_code_tile_from_template(
            &src,
            list,
            "IM_InventoryListTemplate",
            Some("Holotape:0"),
        )
        .unwrap();

    let disp = YsiItemDisplay {
        tag: Some("(hbd1)".into()),
        icon: Some("interface\\icons\\Apparel_ArmorD_Power.dds".into()),
        template: YSI_DEFAULT_TEMPLATE.into(),
    };

    assert!(apply_row_icon(&mut ui, &src, slash_row, Some(&disp)));
    assert!(apply_row_icon(&mut ui, &src, colon_row, Some(&disp)));
    assert_eq!(live_ysi_images(&ui, slash_row).len(), 1);
    assert_eq!(live_ysi_images(&ui, colon_row).len(), 1);
}

#[test]
fn pre_existing_duplicate_ysi_image_children_are_deduplicated_to_one() {
    let dup_menu = r#"
<menu name="InventoryMenu">
  <class> &InventoryMenu; </class>
  <rect name="Row">
    <text name="ListItemText"><x>40</x><string>Recipe</string></text>
    <image name="ySIImage"><filename>stale_a.dds</filename></image>
    <image name="ySIImage"><filename>stale_b.dds</filename></image>
  </rect>
</menu>
"#;
    let src = MemSource::default()
        .with("menus/inv.xml", dup_menu)
        .with(YSI_XML_PATH, YSI_XML);
    let mut ui = Ui::new();
    let inv = ui.load_menu(&src, "menus/inv.xml").unwrap();
    assert!(publish_for_menu(&mut ui, &src, inv, Some(menus::INVENTORY)));
    ui.update();

    let row = ui.find_tile("InventoryMenu/Row").unwrap();
    assert_eq!(live_ysi_images(&ui, row).len(), 2);

    let disp = YsiItemDisplay {
        tag: Some("(fba1)".into()),
        icon: Some("interface\\icons\\WeapA_Pistol.dds".into()),
        template: YSI_DEFAULT_TEMPLATE.into(),
    };
    assert!(apply_row_icon(&mut ui, &src, row, Some(&disp)));
    let imgs = live_ysi_images(&ui, row);
    assert_eq!(imgs.len(), 1);
    assert_eq!(
        ui.tile(imgs[0]).get("filename").map(Value::text).as_deref(),
        Some("interface\\icons\\WeapA_Pistol.dds")
    );
}

#[test]
fn sync_row_icon_x_aligns_fresh_expression_row_after_update_and_is_idempotent() {
    let src = MemSource::default()
        .with("menus/inv.xml", MENU_XML)
        .with(YSI_XML_PATH, YSI_XML);
    let mut ui = Ui::new();
    let inv = ui.load_menu(&src, "menus/inv.xml").unwrap();
    assert!(publish_for_menu(&mut ui, &src, inv, Some(menus::INVENTORY)));
    ui.update();

    let row = ui
        .add_code_tile_from_template(
            &src,
            "InventoryMenu/IM_InventoryList",
            "IM_InventoryListTemplate",
            Some("9mm Pistol"),
        )
        .unwrap();
    let disp = YsiItemDisplay {
        tag: Some("(fba1)".into()),
        icon: Some("interface\\icons\\WeapA_Pistol.dds".into()),
        template: YSI_DEFAULT_TEMPLATE.into(),
    };

    // Before ui.update(), expression ListItemText/x is still unevaluated (0.0), so the
    // initial apply_row_icon places ySIImage/x at 0.0 + (-5.0) = -5.0.
    assert!(apply_row_icon(&mut ui, &src, row, Some(&disp)));
    let img = live_ysi_images(&ui, row)[0];
    assert_eq!(ui.tile(img).get("x").map(Value::num), Some(-5.0));

    // After ui.update() evaluates ListItemText/x to 36.0, sync_row_icon_x moves ySIImage/x
    // to 36.0 + (-5.0) = 31.0 and becomes a no-op on subsequent calls.
    ui.update();
    assert!(sync_row_icon_x(&mut ui, row));
    assert_eq!(ui.tile(img).get("x").map(Value::num), Some(31.0));
    assert!(!sync_row_icon_x(&mut ui, row));
}
