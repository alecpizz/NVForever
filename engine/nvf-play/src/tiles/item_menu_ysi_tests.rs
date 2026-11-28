//! Scoped regression tests for `tiles::item_menu` ySI row icon placement across
//! duplicate-named rows, delimiter-bearing names, and first-frame expression `ListItemText/x`.

use super::*;
use crate::menu::model::{Category, RowKey, Side};
use fnv_sim::FormId;
use fnv_ui::{MemSource, TileId, Ui, Value};

const MENU_XML: &str = r#"
<menu name="InventoryMenu">
  <class> &InventoryMenu; </class>
  <hotrect name="IM_InventoryList">
    <width> 400 </width>
    <height> 468 </height>
    <_scroll_delta> 26 </_scroll_delta>
    <image name="lb_scrollbar">
      <_number_of_items> 1 </_number_of_items>
      <_number_of_visible_items> 1 </_number_of_visible_items>
      <_current_value> 0 </_current_value>
    </image>
    <hotrect name="lb_highlight_box"/>
  </hotrect>
  <template name="IM_InventoryListTemplate">
    <hotrect name="IM_InventoryListTemplateRect">
      <_y> 0 </_y>
      <_VerticalSpacing> 10 </_VerticalSpacing>
      <text name="ListItemText">
        <x> <copy> 36 </copy> </x>
        <string> Item </string>
      </text>
      <rect name="IM_Template_ItemMarker">
        <visible> 0 </visible>
      </rect>
    </hotrect>
  </template>
</menu>
"#;

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

const PATHS: Paths = Paths {
    menu: "InventoryMenu",
    lists: ["", "InventoryMenu/IM_InventoryList"],
    titles: ["", ""],
    template: "IM_InventoryListTemplate",
    row: "IM_InventoryListTemplateRect",
    row_text: "ListItemText",
    row_mark: "IM_Template_ItemMarker",
    targets: &[],
};

fn ysi_images(ui: &Ui, row: TileId) -> Vec<TileId> {
    ui.tile(row)
        .children
        .iter()
        .copied()
        .filter(|&c| ui.is_live(c) && ui.tile(c).name.eq_ignore_ascii_case(fnv_game::ui_ysi::YSI_IMAGE_TILE))
        .collect()
}

fn snapshot(rows: Vec<RowShown>, ysi_icons: bool) -> Snapshot {
    Snapshot {
        lists: [
            ListShown::default(),
            ListShown { rows, category: Category::Weapons, selected: Some(0), filtered: 0 },
        ],
        titles: Default::default(),
        weight: String::new(),
        focus: Side::Player,
        pointer_pick: false,
        asking: false,
        navigated: 0,
        card: None,
        icon: None,
        take_all: false,
        take_all_hint: String::new(),
        note: None,
        ysi_icons,
    }
}

#[test]
fn duplicate_condition_rows_get_one_aligned_icon_each_on_first_settle() {
    let src = MemSource::default()
        .with("menus/inv.xml", MENU_XML)
        .with(fnv_game::ui_ysi::YSI_XML_PATH, YSI_XML);
    let mut ui = Ui::new();
    let inv = ui.load_menu(&src, "menus/inv.xml").unwrap();
    assert!(fnv_game::ui_ysi::publish_for_menu(
        &mut ui,
        &src,
        inv,
        Some(fnv_game::game::menus::INVENTORY),
    ));
    ui.update();

    let rows = vec![
        RowShown {
            key: RowKey { units: (FormId(0x100), Some(1.0f32.to_bits()), 0), reference: None, nth: 0 },
            name: "Varmint Rifle".into(),
            text: "Varmint Rifle".into(),
            equipped: true,
            dim: false,
            ysi: Some(fnv_game::ui_ysi::YsiItemDisplay {
                tag: Some("(fbb1)".into()),
                icon: Some("interface\\icons\\WeapA_Rifle.dds".into()),
                template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
            }),
        },
        RowShown {
            key: RowKey { units: (FormId(0x100), Some(0.6f32.to_bits()), 0), reference: None, nth: 0 },
            name: "Varmint Rifle".into(),
            text: "Varmint Rifle (CND 60%)".into(),
            equipped: false,
            dim: false,
            ysi: Some(fnv_game::ui_ysi::YsiItemDisplay {
                tag: Some("(fbb1)".into()),
                icon: Some("interface\\icons\\WeapA_Rifle.dds".into()),
                template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
            }),
        },
        RowShown {
            key: RowKey { units: (FormId(0x200), Some(1.0f32.to_bits()), 0), reference: None, nth: 0 },
            name: "T-51b Armor (BOS/Outcast)".into(),
            text: "T-51b Armor (BOS/Outcast)".into(),
            equipped: false,
            dim: false,
            ysi: Some(fnv_game::ui_ysi::YsiItemDisplay {
                tag: Some("(hbd1)".into()),
                icon: Some("interface\\icons\\Apparel_ArmorD_Power.dds".into()),
                template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
            }),
        },
    ];

    let snap = snapshot(rows, true);
    let mut tiles = MenuTiles::new(&PATHS);
    let mut no_sizes = |_: &mut Ui, _: &mut SizesWritten| 0usize;

    // Frame 1: reconcile + apply_row_icon + settle (with sync_row_icon_x).
    write_lists(&mut ui, &snap, &mut tiles);
    settle(&mut ui, &snap, &mut tiles, &mut no_sizes);

    let player_rows = &tiles.rows[Side::Player as usize];
    assert_eq!(player_rows.len(), 3);
    for (idx, row) in player_rows.iter().enumerate() {
        let imgs = ysi_images(&ui, row.tile);
        assert_eq!(imgs.len(), 1, "row {idx} must have exactly one ySIImage on frame 1");
        assert_eq!(
            ui.tile(imgs[0]).get("x").map(Value::num),
            Some(31.0),
            "row {idx} ySIImage/x must align to evaluated ListItemText base_x + space_left (36 - 5 = 31) on frame 1",
        );
    }

    // Subsequent refreshes keep exactly 1 ySIImage per row (no duplicate piling).
    write_lists(&mut ui, &snap, &mut tiles);
    settle(&mut ui, &snap, &mut tiles, &mut no_sizes);
    for (idx, row) in tiles.rows[Side::Player as usize].iter().enumerate() {
        assert_eq!(ysi_images(&ui, row.tile).len(), 1, "row {idx} must keep exactly one ySIImage after refresh");
    }
}

#[test]
fn vanilla_menu_without_ysi_xml_leaves_duplicate_rows_untouched() {
    let src = MemSource::default().with("menus/inv.xml", MENU_XML);
    let mut ui = Ui::new();
    let _ = ui.load_menu(&src, "menus/inv.xml").unwrap();
    ui.update();

    let rows = vec![
        RowShown {
            key: RowKey { units: (FormId(0x100), Some(1.0f32.to_bits()), 0), reference: None, nth: 0 },
            name: "Varmint Rifle".into(),
            text: "Varmint Rifle".into(),
            equipped: false,
            dim: false,
            ysi: None,
        },
        RowShown {
            key: RowKey { units: (FormId(0x100), Some(0.5f32.to_bits()), 0), reference: None, nth: 0 },
            name: "Varmint Rifle".into(),
            text: "Varmint Rifle (CND 50%)".into(),
            equipped: false,
            dim: false,
            ysi: None,
        },
    ];
    let snap = snapshot(rows, true);
    let mut tiles = MenuTiles::new(&PATHS);
    let mut no_sizes = |_: &mut Ui, _: &mut SizesWritten| 0usize;

    write_lists(&mut ui, &snap, &mut tiles);
    settle(&mut ui, &snap, &mut tiles, &mut no_sizes);

    for row in &tiles.rows[Side::Player as usize] {
        assert!(ysi_images(&ui, row.tile).is_empty());
    }
}
