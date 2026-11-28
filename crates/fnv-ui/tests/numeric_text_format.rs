use fnv_ui::{MemSource, Ui, Value};

#[test]
fn numeric_text_drops_f64_rounding_tail_from_f32_fractions() {
    // Mirrors item_card::write setting ConditionInfo/user5 from f32 via f64::from,
    // followed by XML multiplying by _VUI+CNDmax (e.g. 800).
    let product = f64::from(0.62_f32) * 800.0;
    assert_ne!(product.fract(), 0.0);
    assert_eq!(Value::Num(product).text(), "496");

    let one_third = f64::from(1.0_f32 / 3.0_f32) * 300.0;
    assert_eq!(Value::Num(one_third).text(), "100");

    // Ordinary finite fractions stay compact and accurate to f32 precision.
    assert_eq!(Value::Num(0.5).text(), "0.5");
    assert_eq!(Value::Num(f64::from(12.25_f32)).text(), "12.25");

    // Whole numbers and zero keep integer formatting without a decimal point.
    assert_eq!(Value::Num(0.0).text(), "0");
    assert_eq!(Value::Num(-0.0).text(), "0");
    assert_eq!(Value::Num(800.0).text(), "800");
    assert_eq!(Value::Num(-42.0).text(), "-42");

    // Values outside finite non-zero f32 range fall back to f64 text rather than
    // collapsing to "0" or "inf".
    assert_eq!(Value::Num(1e-45_f64).text(), format!("{}", 1e-45_f64));
    assert_eq!(Value::Num(1e40_f64).text(), format!("{}", 1e40_f64));
}

#[test]
fn condition_label_expression_formats_cleanly_without_spilling_into_adjacent_cards() {
    let xml = r#"
<menu name="InventoryMenu">
  <rect name="IM_ItemInfoRect">
    <rect name="ConditionInfo">
      <visible>1</visible>
      <user5>0</user5>
      <_VUI+CNDmax>800</_VUI+CNDmax>
      <rect name="ConditionMeter">
        <x>70</x>
        <y>10</y>
        <width>100</width>
        <height>18</height>
      </rect>
      <text name="ConditionLabel">
        <string>
          <copy src="parent" trait="user5" />
          <mul><copy src="parent" trait="_VUI+CNDmax" /></mul>
        </string>
        <x><copy src="sibling(ConditionMeter)" trait="x" /></x>
        <y><copy src="sibling(ConditionMeter)" trait="y" /></y>
        <visible><copy src="parent" trait="visible" /></visible>
      </text>
      <text name="ConditionLabelDiv">
        <string> / </string>
        <x>
          <copy src="sibling(ConditionLabel)" trait="x" />
          <add><copy src="sibling(ConditionLabel)" trait="width" /></add>
        </x>
        <y><copy src="sibling(ConditionLabel)" trait="y" /></y>
        <visible><copy src="sibling(ConditionLabel)" trait="visible" /></visible>
      </text>
      <text name="ConditionLabelMax">
        <string><copy src="parent" trait="_VUI+CNDmax" /></string>
        <x>
          <copy src="sibling(ConditionLabel)" trait="x" />
          <add><copy src="sibling(ConditionLabel)" trait="width" /></add>
          <add><copy src="sibling(ConditionLabelDiv)" trait="width" /></add>
        </x>
        <y><copy src="sibling(ConditionLabel)" trait="y" /></y>
        <visible><copy src="sibling(ConditionLabel)" trait="visible" /></visible>
      </text>
    </rect>
  </rect>
</menu>
"#;
    let src = MemSource::default().with("menus\\main\\inventory_menu.xml", xml);
    let mut ui = Ui::new();
    ui.load_menu(&src, "menus\\main\\inventory_menu.xml")
        .expect("menu loads");

    let cnd = ui
        .find_tile("InventoryMenu/IM_ItemInfoRect/ConditionInfo")
        .expect("ConditionInfo");
    ui.set_trait(cnd, "user5", Value::Num(f64::from(0.62_f32)));
    ui.update();

    let label = ui
        .get_string("InventoryMenu/IM_ItemInfoRect/ConditionInfo/ConditionLabel/string")
        .expect("ConditionLabel string");
    let max_label = ui
        .get_string("InventoryMenu/IM_ItemInfoRect/ConditionInfo/ConditionLabelMax/string")
        .expect("ConditionLabelMax string");

    assert_eq!(label, "496");
    assert_eq!(max_label, "800");

    // Simulate measured widths (e.g., 8 units per character) and confirm sibling x
    // layout places the three labels sequentially without overlap.
    let label_tile = ui
        .find_tile("InventoryMenu/IM_ItemInfoRect/ConditionInfo/ConditionLabel")
        .unwrap();
    let div_tile = ui
        .find_tile("InventoryMenu/IM_ItemInfoRect/ConditionInfo/ConditionLabelDiv")
        .unwrap();
    ui.set_trait(label_tile, "width", Value::Num((label.len() as f64) * 8.0));
    ui.set_trait(div_tile, "width", Value::Num(12.0));
    ui.update();

    let x_label = ui
        .get_float("InventoryMenu/IM_ItemInfoRect/ConditionInfo/ConditionLabel/x")
        .unwrap();
    let x_div = ui
        .get_float("InventoryMenu/IM_ItemInfoRect/ConditionInfo/ConditionLabelDiv/x")
        .unwrap();
    let x_max = ui
        .get_float("InventoryMenu/IM_ItemInfoRect/ConditionInfo/ConditionLabelMax/x")
        .unwrap();
    assert_eq!(x_label, 70.0);
    assert_eq!(x_div, 94.0);
    assert_eq!(x_max, 106.0);
    // Whole "496 / 800" readout fits well inside the 100-unit meter (70..170).
    assert!(x_max + 24.0 < 170.0);
}

#[test]
fn vanilla_string_traits_remain_unchanged() {
    let xml = r#"
<menu name="InventoryMenu">
  <rect name="AmmoInfo">
    <_Title>10mm (12/250)</_Title>
    <_Value></_Value>
    <text name="CI_TitleText">
      <string><copy src="parent" trait="_Title" /></string>
    </text>
    <text name="CI_ValueText">
      <string><copy src="parent" trait="_Value" /></string>
    </text>
  </rect>
</menu>
"#;
    let src = MemSource::default().with("menus\\main\\inventory_menu.xml", xml);
    let mut ui = Ui::new();
    ui.load_menu(&src, "menus\\main\\inventory_menu.xml")
        .expect("menu loads");
    ui.update();

    assert_eq!(
        ui.get_string("InventoryMenu/AmmoInfo/CI_TitleText/string").as_deref(),
        Some("10mm (12/250)")
    );
    assert_eq!(
        ui.get_string("InventoryMenu/AmmoInfo/CI_ValueText/string").as_deref(),
        Some("")
    );
}
