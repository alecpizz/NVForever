//! Scoped regression tests for ancestor `<systemcolor>` inheritance vs local
//! `<red>`/`<green>`/`<blue>` copy expressions (such as outline/shadow text prefabs
//! under `<menu>` tiles with `<systemcolor>&pipboy;</systemcolor>`).

use super::{pieces, Select};
use crate::interface;
use fnv_ui::{MemSource, Ui};

fn plain(_font: f64, s: &str, _wrap: Option<f64>) -> (f64, f64) {
    (s.len() as f64 * 12.0, 24.0)
}

#[test]
fn ancestor_systemcolor_beats_copied_zero_rgb_while_nosystemcolor_keeps_local_rgb() {
    let src = MemSource::default().with(
        "map_headline.xml",
        r#"<menu name="MapMenu">
          <systemcolor>&pipboy;</systemcolor>
          <rect name="Headline">
            <text name="Original">
              <string>DATA</string>
              <brightness>255</brightness>
              <red><copy src="parent" trait="red"/></red>
              <green><copy src="parent" trait="green"/></green>
              <blue><copy src="parent" trait="blue"/></blue>
            </text>
            <text name="Shadow">
              <string>DATA</string>
              <brightness>0</brightness>
              <red>0</red><green>0</green><blue>0</blue>
            </text>
          </rect>
          <rect name="LocalPanel">
            <systemcolor>&nosystemcolor;</systemcolor>
            <text name="CustomTint">
              <string>LOCAL</string>
              <red>51</red><green>102</green><blue>204</blue>
            </text>
          </rect>
          <text name="DirectLocal">
            <string>OPT_OUT</string>
            <systemcolor>0</systemcolor>
            <red>255</red><green>0</green><blue>128</blue>
          </text>
        </menu>"#,
    );
    let mut ui = Ui::new();
    let menu = ui.load_menu(&src, "map_headline.xml").unwrap();
    ui.update();
    let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &[], strip: None }, false);
    assert_eq!(drawn.len(), 4);

    let pipboy = interface::pipboy_rgb().map(|c| f32::from(c) / 255.0);
    // Original inherits MapMenu's &pipboy; instead of turning black from copied 0/0/0 RGB.
    assert_eq!(drawn[0].colour[..3], pipboy);
    // Explicit brightness=0 shadow still renders black.
    assert_eq!(drawn[1].colour[..3], [0.0, 0.0, 0.0]);
    // Ancestor &nosystemcolor; lets child use its own local RGB.
    assert_eq!(drawn[2].colour[..3], [51.0 / 255.0, 102.0 / 255.0, 204.0 / 255.0]);
    // Tile-level systemcolor=0 lets tile use its own local RGB.
    assert_eq!(drawn[3].colour[..3], [1.0, 0.0, 128.0 / 255.0]);
}

#[test]
fn black_rgb_without_ancestor_systemcolor_stays_black() {
    let src = MemSource::default().with(
        "book.xml",
        r#"<menu name="BookMenu">
          <text name="PageNum">
            <string>1</string>
            <red>0</red><green>0</green><blue>0</blue>
          </text>
        </menu>"#,
    );
    let mut ui = Ui::new();
    let menu = ui.load_menu(&src, "book.xml").unwrap();
    ui.update();
    let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &[], strip: None }, false);
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].colour[..3], [0.0, 0.0, 0.0]);
}
