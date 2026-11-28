//! Synthetic XML and sentinel values [design]; expected HUD values [observed: hud.md §14].

use fnv_game::ui_hud::{self, Shown, Texts};
use fnv_ui::{MemSource, Ui, Value};

use crate::common;

// Columns are 4:3, 16:10 and 16:9. The half-unit reticle offset follows the
// truncated middle (see docs/spec-notes/hud.md §14, its child's 607.5/735.5/820.5).
const PLACES: &[(&str, [f64; 3], f64)] = &[
    ("HitPoints", [40.0, 40.0, 40.0], 803.0),
    ("ActionPoints", [894.0, 1150.0, 1320.0], 803.0),
    ("HardcoreMode", [190.0, 190.0, 190.0], 803.0),
    ("SneakMeter", [640.0, 768.0, 853.0], 30.0),
    ("ReticleCenter", [605.5, 733.5, 818.5], 439.5),
    ("EnemyHealth", [465.0, 593.0, 678.0], 843.0),
    ("QuestReminder", [45.0, 45.0, 45.0], 200.0),
    ("Messages", [25.0, 25.0, 25.0], 55.0),
    ("Subtitles", [240.0, 368.0, 453.0], 660.0),
    ("Info", [480.0, 608.0, 693.0], 820.0),
    ("Hokeys", [909.0, 1165.0, 1335.0], 483.0),
    ("Region_Location", [45.0, 45.0, 45.0], 728.0),
    ("XPMeter", [860.0, 1116.0, 1286.0], 650.0),
    ("RadiationMeter", [863.0, 1119.0, 1289.0], 30.0),
    ("BreathMeter", [640.0, 768.0, 853.0], 110.0),
    ("Explosive_positioning_rect", [640.0, 768.0, 853.0], 480.0),
    ("crippled_limb_indicator", [1050.0, 1306.0, 1476.0], 130.0),
    ("DDTIcon", [414.0, 414.0, 414.0], 852.0),
    ("DDTIconEnemy", [433.0, 561.0, 646.0], 846.0),
    ("DDTIconEnemyAP", [433.0, 561.0, 646.0], 846.0),
    ("AmmoTypeLabel", [1044.0, 1300.0, 1470.0], 883.0),
    ("CNDArrows", [1027.0, 1283.0, 1453.0], 886.0),
];

const VISIBLE: &[(&str, f64)] = &[
    ("ActionPoints", 1.0),
    ("HitPoints", 2.0),
    ("RadiationMeter", 4.0),
    ("QuestReminder", 16.0),
    ("Region_Location", 32.0),
    ("ReticleCenter", 64.0),
    ("SneakMeter", 128.0),
    ("Messages", 256.0),
    ("Info", 512.0),
    ("Subtitles", 1024.0),
    ("XPMeter", 4096.0),
    ("BreathMeter", 8192.0),
    ("crippled_limb_indicator", 32768.0),
    ("HardcoreMode", 65536.0),
    ("EnemyHealth", 1.0),
    ("AmmoTypeLabel", 1.0),
    ("CNDArrows", 1.0),
];

/// The children whose alpha shows their piece [observed: hud.md §14.1].
const CHILDREN: &[(&str, &str)] = &[
    ("SneakMeter", "sneak_nif"),
    ("RadiationMeter", "radiation_text"),
    ("RadiationMeter", "radiation_text_value"),
    ("RadiationMeter", "radiation_pointer"),
    ("RadiationMeter", "radiation_bracket"),
];

fn all_shown() -> Shown {
    Shown {
        root: true,
        aiming: false,
        corners: true,
        sneak: true,
        reticle: true,
        enemy: true,
        quests: true,
        messages: true,
        subtitles: true,
        info: true,
        location: true,
        xp: true,
        radiation: true,
        breath: true,
        limbs: true,
        needs: true,
        ammo_label: true,
        condition: true,
    }
}

// The strings observed in the original [observed: hud.md §14.2].
fn texts() -> Texts {
    Texts {
        radiation_word: "RAD/SEC".into(),
        radiation_value: "+0".into(),
        xp_label: "XP".into(),
        xp_level_up: "LEVEL UP".into(),
        needs: ["H20", "FOD", "SLP", "RAD", "LMB"].map(String::from),
        sneak: None,
        prompt_key: None,
        prompt_verb: None,
        frame: Default::default(),
    }
}

fn fixture() -> Ui {
    let mut xml = String::from(r#"<menu name="HUDMainMenu"><visible>17</visible>"#);
    for (name, _, _) in PLACES {
        let children = CHILDREN.iter().filter(|(piece, _)| piece == name);
        let mut children: String = children.map(|(_, child)| format!(r#"<image name="{child}"><alpha>17</alpha><string>sentinel</string></image>"#)).collect();
        let texts: &[&str] = match *name {
            "XPMeter" => &["XPLabel", "XPLevelUp"],
            "HardcoreMode" => &["Dehydration", "Hunger", "Sleep", "Rads", "LMBs"],
            _ => &[],
        };
        for text in texts {
            children.push_str(&format!(r#"<text name="{text}"><string>sentinel</string></text>"#));
        }
        if *name == "Info" {
            children.push_str(r#"<rect name="justify_center_hotrect"><string>sentinel</string><text name="PCShortcutLabel"><string>sentinel</string></text><text name="button_text"><string>sentinel</string></text></rect>"#);
        }
        xml.push_str(&format!(r#"<rect name="{name}"><visible>17</visible>{children}</rect>"#));
    }
    xml.push_str(r#"<rect name="DDTIconAP"><visible>17</visible></rect>"#);
    for name in ["_CrosshairVisible", "_SneakVisible", "_SneakLevel", "_RadiationLevel", "_TargetType", "_ActionType", "_SpreadPercentage"] {
        xml.push_str(&format!("<{name}>17</{name}>"));
    }
    xml.push_str("</menu>");
    let source = MemSource::default().with("hud.xml", &xml);
    let mut ui = Ui::new();
    ui.load_menu(&source, "hud.xml").unwrap();
    ui
}

fn screen(ui: &mut Ui, width: f64, height: f64) {
    let tile = ui.screen();
    ui.set_trait(tile, "width", Value::Num(width));
    ui.set_trait(tile, "height", Value::Num(height));
}

#[test]
fn places_at_the_three_observed_aspect_ratios_and_resize() {
    let mut ui = fixture();
    for (column, (width, height)) in [(1024.0, 768.0), (1280.0, 800.0), (1920.0, 1080.0)].into_iter().enumerate() {
        screen(&mut ui, width, height);
        ui_hud::write(&mut ui, all_shown(), &texts());
        ui.update();
        for (name, xs, y) in PLACES {
            assert_eq!(ui.get_float(&format!("HUDMainMenu/{name}/x")), Some(xs[column]), "{name}, {width}x{height}");
            assert_eq!(ui.get_float(&format!("HUDMainMenu/{name}/y")), Some(*y), "{name}");
        }
        assert_eq!(ui.get_float("HUDMainMenu/CNDArrows/height"), Some(20.0));
        assert_eq!(ui.get_float("HUDMainMenu/DDTIconAP/x"), None);
    }
}

/// #1631: at a UI scale of 75% the view is 1280 units high (960 / 0.75) and 2275.55 wide
/// at 16:9: the left pieces keep their x, the right ones and the middle ones follow the
/// wider view, and each piece keeps its distance from its edge (bottom pieces 320 units
/// further down, the middle ones 160, the top ones where they were).
#[test]
fn a_smaller_ui_scale_keeps_each_piece_at_its_edge() {
    let mut ui = fixture();
    screen(&mut ui, 1920.0, 1080.0);
    let mut t = texts();
    t.frame.ui_scale = 0.75;
    ui_hud::write(&mut ui, all_shown(), &t);
    ui.update();
    let (right, middle) = ((1280.0f64 * 1920.0 / 1080.0).floor(), (1280.0f64 * 1920.0 / 1080.0 / 2.0).floor());
    assert_eq!((right, middle), (2275.0, 1137.0));
    for (name, x, y) in [
        ("HitPoints", 40.0, 803.0 + 320.0),
        ("ActionPoints", right - 386.0, 803.0 + 320.0),
        ("XPMeter", right - 420.0, 650.0 + 320.0),
        ("CNDArrows", right - 253.0, 886.0 + 320.0),
        ("EnemyHealth", middle - 175.0, 843.0 + 320.0),
        ("ReticleCenter", middle - 34.5, 439.5 + 160.0),
        ("Info", middle - 160.0, 820.0 + 160.0),
        ("SneakMeter", middle, 30.0),
        ("RadiationMeter", right - 417.0, 30.0),
        ("Messages", 25.0, 55.0),
    ] {
        assert_eq!(ui.get_float(&format!("HUDMainMenu/{name}/x")), Some(x), "{name}");
        assert_eq!(ui.get_float(&format!("HUDMainMenu/{name}/y")), Some(y), "{name}");
    }
    assert_eq!(ui.get_float("HUDMainMenu/Subtitles/x"), Some(middle - 400.0));
    // The subtitles keep to the middle, as the prompt they rise over.
    assert_eq!(ui.get_float("HUDMainMenu/Subtitles/y"), Some(660.0 + 160.0));
    // Back at 100%: the observed places.
    ui_hud::write(&mut ui, all_shown(), &texts());
    ui.update();
    for (name, xs, y) in PLACES {
        assert_eq!(ui.get_float(&format!("HUDMainMenu/{name}/x")), Some(xs[2]), "{name}");
        assert_eq!(ui.get_float(&format!("HUDMainMenu/{name}/y")), Some(*y), "{name}");
    }
}

#[test]
fn visible_keeps_its_value_shown_or_not_and_untouched_file_traits() {
    let mut ui = fixture();
    // Everything shown, nothing shown (the reticle hidden too) and the root hidden.
    for shown in [all_shown(), Shown { root: true, ..Shown::default() }, Shown { root: false, ..all_shown() }] {
        ui_hud::write(&mut ui, shown, &texts());
        ui.update();
        assert_eq!(ui.get_float("HUDMainMenu/visible"), Some(f64::from(shown.root)));
        for (name, value) in VISIBLE {
            let shows = shown.root && (*name != "ReticleCenter" || shown.reticle) && (!CORNER_PIECES.contains(name) || shown.corners);
            let expected = if shows { *value } else { 0.0 };
            assert_eq!(ui.get_float(&format!("HUDMainMenu/{name}/visible")), Some(expected), "{name}");
        }
        for name in ["Hokeys", "Explosive_positioning_rect", "DDTIcon", "DDTIconAP", "DDTIconEnemy", "DDTIconEnemyAP"] {
            assert_eq!(ui.get_float(&format!("HUDMainMenu/{name}/visible")), Some(17.0), "{name}");
        }
        for name in ["_CrosshairVisible", "_SneakVisible", "_SneakLevel", "_RadiationLevel", "_TargetType", "_ActionType", "_SpreadPercentage"] {
            assert_eq!(ui.get_float(&format!("HUDMainMenu/{name}")), Some(17.0), "{name}");
        }
    }
}

/// The pieces the hand-built HUD hides with its corners (#980).
const CORNER_PIECES: [&str; 5] = ["ActionPoints", "HitPoints", "RadiationMeter", "AmmoTypeLabel", "CNDArrows"];

/// A movement lock, portrait, held camera or the weapon wheel (`corners` off) hides the
/// corner pieces and leaves the rest as they were (#980).
#[test]
fn the_corner_pieces_hide_with_the_corners() {
    let mut ui = fixture();
    ui_hud::write(&mut ui, Shown { corners: false, ..all_shown() }, &texts());
    ui.update();
    for (name, value) in VISIBLE {
        let expected = if CORNER_PIECES.contains(name) { 0.0 } else { *value };
        assert_eq!(ui.get_float(&format!("HUDMainMenu/{name}/visible")), Some(expected), "{name}");
    }
    ui_hud::write(&mut ui, all_shown(), &texts());
    ui.update();
    for name in CORNER_PIECES {
        assert_ne!(ui.get_float(&format!("HUDMainMenu/{name}/visible")), Some(0.0), "{name} back");
    }
}

#[test]
fn aiming_hides_the_reticle_and_sets_the_enemy_bar_to_8() {
    let mut ui = fixture();
    ui_hud::write(&mut ui, Shown { aiming: true, ..all_shown() }, &texts());
    assert_eq!(ui.get_float("HUDMainMenu/ReticleCenter/visible"), Some(0.0));
    assert_eq!(ui.get_float("HUDMainMenu/EnemyHealth/visible"), Some(8.0));
    ui_hud::write(&mut ui, all_shown(), &texts());
    assert_eq!(ui.get_float("HUDMainMenu/ReticleCenter/visible"), Some(64.0));
    assert_eq!(ui.get_float("HUDMainMenu/EnemyHealth/visible"), Some(1.0));
}

#[test]
fn sneaking_and_radiation_show_through_the_childrens_alpha() {
    let mut ui = fixture();
    for (sneak, radiation) in [(true, false), (false, true), (false, false)] {
        ui_hud::write(&mut ui, Shown { sneak, radiation, ..all_shown() }, &texts());
        for (piece, child) in CHILDREN {
            let on = if *piece == "SneakMeter" { sneak } else { radiation };
            let expected = if on { 255.0 } else { 0.0 };
            assert_eq!(ui.get_float(&format!("HUDMainMenu/{piece}/{child}/alpha")), Some(expected), "{piece}/{child}");
        }
        assert_eq!(ui.get_float("HUDMainMenu/SneakMeter/visible"), Some(128.0));
        assert_eq!(ui.get_float("HUDMainMenu/RadiationMeter/visible"), Some(4.0));
    }
}

#[test]
fn script_values_hold_until_the_next_write() {
    let mut ui = fixture();
    screen(&mut ui, 1280.0, 800.0);
    ui_hud::write(&mut ui, all_shown(), &texts());
    assert!(ui.set_float("HUDMainMenu/ActionPoints/x", 17.0));
    assert!(ui.set_float("HUDMainMenu/HitPoints/visible", 17.0));
    ui.update();
    assert_eq!(ui.get_float("HUDMainMenu/ActionPoints/x"), Some(17.0));
    assert_eq!(ui.get_float("HUDMainMenu/HitPoints/visible"), Some(17.0));
    ui_hud::write(&mut ui, all_shown(), &texts());
    ui.update();
    assert_eq!(ui.get_float("HUDMainMenu/ActionPoints/x"), Some(1150.0));
    assert_eq!(ui.get_float("HUDMainMenu/HitPoints/visible"), Some(2.0));
}

#[test]
fn no_hud_loaded_means_no_writes() {
    let mut ui = Ui::new();
    screen(&mut ui, 1280.0, 800.0);
    let snapshot = |ui: &Ui| [ui.screen(), ui.strings(), ui.globals()].into_iter().chain(ui.menus()).map(|id| format!("{:?}", ui.tile(id))).collect::<Vec<_>>();
    let before = snapshot(&ui);
    ui_hud::write(&mut ui, all_shown(), &texts());
    assert_eq!(before, snapshot(&ui));
    assert!(ui.find_tile("HUDMainMenu").is_none());
}

#[test]
fn every_observed_file_string_is_written_even_when_hidden() {
    let mut ui = fixture();
    let texts = Texts { radiation_value: "+9".into(), sneak: Some("[HIDDEN]".into()), prompt_key: Some("E)".into()), prompt_verb: Some("Talk".into()), ..texts() };
    ui_hud::write(&mut ui, Shown::default(), &texts);
    ui.update();
    // All file paths and strings [observed: hud.md §14.2].
    for (path, value) in [
        ("RadiationMeter/radiation_text", "RAD/SEC"),
        ("RadiationMeter/radiation_text_value", "+9"),
        ("XPMeter/XPLabel", "XP"),
        ("XPMeter/XPLevelUp", "LEVEL UP"),
        ("HardcoreMode/Dehydration", "H20"),
        ("HardcoreMode/Hunger", "FOD"),
        ("HardcoreMode/Sleep", "SLP"),
        ("HardcoreMode/Rads", "RAD"),
        ("HardcoreMode/LMBs", "LMB"),
        ("SneakMeter/sneak_nif", "[HIDDEN]"),
        ("Info/justify_center_hotrect/PCShortcutLabel", "E)"),
        ("Info/justify_center_hotrect", "Talk"),
        ("Info/justify_center_hotrect/button_text", "Talk"),
    ] {
        assert_eq!(ui.get_string(&format!("HUDMainMenu/{path}/string")).as_deref(), Some(value), "{path}");
    }
}

#[test]
fn a_scripts_need_word_lasts_until_the_huds_word_changes() {
    // [observed: jip-probes gaps1, SetUIString on HardcoreMode/Dehydration read back in
    // later steps; #1026].
    let mut ui = fixture();
    let path = "HUDMainMenu/HardcoreMode/Dehydration/string";
    ui_hud::write(&mut ui, all_shown(), &texts());
    assert!(ui.set(path, fnv_ui::Value::Str("NVF one".into())));
    for _ in 0..3 {
        ui_hud::write(&mut ui, all_shown(), &texts());
        ui.update();
    }
    assert_eq!(ui.get_string(path).as_deref(), Some("NVF one"), "the HUD's word did not change");
    let mut changed = texts();
    changed.needs[0] = "H2O!".into();
    ui_hud::write(&mut ui, all_shown(), &changed);
    ui.update();
    assert_eq!(ui.get_string(path).as_deref(), Some("H2O!"), "a new word is written");
}

#[test]
fn standing_and_losing_the_prompt_keep_the_last_strings() {
    let mut ui = fixture();
    let current = Texts { sneak: Some("[CAUTION]".into()), prompt_key: Some("E)".into()), prompt_verb: Some("Talk".into()), ..texts() };
    ui_hud::write(&mut ui, all_shown(), &current);
    ui.update();
    ui_hud::write(&mut ui, Shown { root: true, ..Shown::default() }, &texts());
    ui.update();
    for (path, value) in [
        ("SneakMeter/sneak_nif", "[CAUTION]"),
        ("Info/justify_center_hotrect/PCShortcutLabel", "E)"),
        ("Info/justify_center_hotrect", "Talk"),
        ("Info/justify_center_hotrect/button_text", "Talk"),
    ] {
        assert_eq!(ui.get_string(&format!("HUDMainMenu/{path}/string")).as_deref(), Some(value), "{path}");
    }
    assert_eq!(ui.get_float("HUDMainMenu/SneakMeter/sneak_nif/alpha"), Some(0.0));
}

#[test]
fn no_radiation_writes_plus_zero_and_needs_accept_plugin_words() {
    let mut ui = fixture();
    let current = Texts { radiation_value: "+9".into(), ..texts() };
    ui_hud::write(&mut ui, all_shown(), &current);
    // Plugin words are synthetic [design].
    let current = Texts { needs: ["Water", "Food", "Rest", "Rads", "Limbs"].map(String::from), ..texts() };
    ui_hud::write(&mut ui, Shown { radiation: false, ..all_shown() }, &current);
    ui.update();
    assert_eq!(ui.get_string("HUDMainMenu/RadiationMeter/radiation_text_value/string").as_deref(), Some("+0"));
    for (tile, word) in ["Dehydration", "Hunger", "Sleep", "Rads", "LMBs"].into_iter().zip(current.needs) {
        assert_eq!(ui.get_string(&format!("HUDMainMenu/HardcoreMode/{tile}/string")), Some(word));
    }
}

#[test]
fn missing_file_tiles_are_skipped() {
    let source = MemSource::default().with("hud.xml", r#"<menu name="HUDMainMenu"><rect name="RadiationMeter"><text name="radiation_text"/></rect><rect name="Info"/></menu>"#);
    let mut ui = Ui::new();
    ui.load_menu(&source, "hud.xml").unwrap();
    let current = Texts { sneak: Some("[HIDDEN]".into()), prompt_key: Some("E)".into()), prompt_verb: Some("Talk".into()), ..texts() };
    ui_hud::write(&mut ui, all_shown(), &current);
    ui.update();
    assert_eq!(ui.get_string("HUDMainMenu/RadiationMeter/radiation_text/string").as_deref(), Some("RAD/SEC"));
    assert!(ui.find_tile("HUDMainMenu/RadiationMeter/radiation_text_value").is_none());
    assert!(ui.find_tile("HUDMainMenu/Info/justify_center_hotrect").is_none());
}

// Hand-written synthetic templates with sentinel values [design]. Identifiers and
// default tile names [documented: menus/prefabs/hudtemplates.xml; hud.md §14.6].
fn copy_files(omit: &[&str]) -> MemSource {
    let mut xml = String::from(r#"<menu name="HUDMainMenu"><class>&hudmainmenu;</class>"#);
    for (template, kind, name, body) in [
        (
            "template_compass_window",
            "image",
            "compass_window",
            "<width>345</width><height>64</height><x>20</x><y>65</y>",
        ),
        ("template_justify_left_text", "text", "justify_left_text", "<visible>0</visible><alpha>0</alpha>"),
        ("template_justify_right_text", "text", "justify_right_text", ""),
        ("template_justify_center_text", "text", "justify_center_text", ""),
        ("template_meter", "image", "meter", "<height>20</height>"),
        ("template_meter_background", "image", "MeterBackground", "<width>60</width><height>20</height>"),
        ("template_left_bracket", "image", "left_bracket", ""),
        ("template_right_bracket", "image", "right_bracket", ""),
        ("template_info_seperator", "image", "info_seperator", "<filename>divider.dds</filename>"),
        ("template_enemy_health_bracket", "image", "enemy_health_bracket", "<filename>enemy.dds</filename>"),
        (
            "template_reticle_center",
            "image",
            "reticle_center",
            "<user0>plain.dds</user0><user1>filled.dds</user1><width>64</width><height>64</height><x>2</x><y>9</y>",
        ),
        ("template_compass_npc_icon_group", "rect", "compass_npc_icon_group", "<y>17</y>"),
        ("template_compass_icon_group", "rect", "compass_icon_group", "<y>17</y>"),
        ("compass_npc_icon", "image", "compass_npc_icon", "<y>-10</y><x><copy src=\"me()\" trait=\"_Heading\"/></x>"),
        ("template_compass_icon_player", "image", "compass_icon", ""),
        ("template_compass_icon_quest", "image", "compass_icon", ""),
        ("template_compass_icon_marker", "image", "compass_icon", ""),
    ] {
        if !omit.contains(&template) {
            xml.push_str(&format!(r#"<template name="{template}"><{kind} name="{name}"><depth>17</depth>{body}</{kind}></template>"#));
        }
    }
    xml.push_str(
        r#"<rect name="HitPoints"/><rect name="ActionPoints"/><rect name="ReticleCenter"/>
        <rect name="Info"><rect name="justify_center_hotrect"/></rect>
        <rect name="EnemyHealth"/><rect name="Subtitles"/><rect name="RadiationMeter"><image name="radiation_pointer"><x>15</x></image></rect></menu>"#,
    );
    MemSource::default().with("menus/hud.xml", &xml)
}

fn copy_tree(omit: &[&str]) -> Ui {
    let files = copy_files(omit);
    let mut ui = Ui::new();
    ui.load_menu(&files, "menus/hud.xml").unwrap();
    ui_hud::build(&mut ui);
    ui.update();
    ui
}

// Ordering [observed: hud.md §14–§14.4]; uncaptured names (see docs/spec-notes/hud.md §14.6).
fn assert_copy_paths(ui: &Ui) {
    for (parent, names) in [
        ("HitPoints", &["compass_window", "justify_left_text", "meter", "left_bracket"][..]),
        (
            "ActionPoints",
            &[
                "MeterBackground",
                "meter:0",
                "justify_right_text:0",
                "justify_right_text:1",
                "justify_right_text:2",
                "meter:1",
                "right_bracket",
            ][..],
        ),
        ("ReticleCenter", &["reticle_center"][..]),
        ("EnemyHealth", &["justify_center_text", "meter", "enemy_health_bracket"][..]),
        (
            "Info",
            &[
                "info_seperator", "justify_left_text:0", "justify_right_text:0",
                "justify_left_text:1", "justify_right_text:1", "justify_center_text:0",
                "justify_center_text:1", "justify_center_text:2", "justify_center_hotrect",
            ][..],
        ),
        ("HitPoints/compass_window", &["compass_npc_icon_group", "compass_icon_group:0", "compass_icon_group:1"][..]),
    ] {
        for (i, name) in names.iter().enumerate() {
            let named = ui.find_tile(&format!("HUDMainMenu/{parent}/{name}"));
            assert!(named.is_some(), "{parent}/{name}");
            assert_eq!(named, ui.find_tile(&format!("HUDMainMenu/{parent}/*:{i}")));
        }
    }
    assert!(ui.find_tile("HUDMainMenu/HitPoints/compass_window/compass_icon_group:0/compass_icon").is_some());
    for (path, expected) in [("HitPoints/compass_window/compass_npc_icon_group/y", 10.0), ("HitPoints/compass_window/compass_icon_group:0/y", 30.0), ("ReticleCenter/reticle_center/depth", 1.0)] {
        assert_eq!(ui.get_float(&format!("HUDMainMenu/{path}")), Some(expected));
    }
    for i in 0..10 {
        assert_eq!(
            ui.find_tile(&format!("HUDMainMenu/HitPoints/compass_window/compass_npc_icon_group/compass_npc_icon:{i}")),
            ui.find_tile(&format!("HUDMainMenu/HitPoints/*:0/*:0/*:{i}"))
        );
    }
}

#[test]
fn copies_exist_by_name_and_index_on_load_and_rebuild() {
    use fnv_esm::{LoadOrder, Plugin, Schema};
    use fnv_game::{ui_loading, Game, GameConfig, GameData};
    use fnv_script::FunctionTable;
    use fnv_sim::{FormStore, RefTable, SimConfig, World};
    let plugins = vec![Plugin::parse(&common::tes4(), "Empty.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let functions = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &functions, &refs);
    let mut game = Game::new(&data, World::new(&forms, &refs, SimConfig::default()), GameConfig::default());
    for _ in 0..2 {
        let files = copy_files(&[]);
        ui_loading::load(&mut game, Box::new(files), ["menus/hud.xml".into()]);
        assert_copy_paths(&game.ui_tree);
        assert_eq!(game.ui_tree.get_string("HUDMainMenu/HitPoints/*:1/string").as_deref(), Some("HP"));
        assert!(game.ui_tree.find_tile("HUDMainMenu/HitPoints/Injected").is_none());
        game.ui_tree.inject_text(&MemSource::default(), "HUDMainMenu/HitPoints", r#"<rect name="Injected"/>"#);
        game.ui_tree.set_string("HUDMainMenu/HitPoints/*:1/string", "changed");
    }
}

#[test]
fn bar_copies_follow_captured_tick_and_condition_widths() {
    let mut ui = copy_tree(&[]);
    let mut input = texts();
    // Health/AP and condition pairs [observed: hud.md §14.3].
    for (health, hp_width) in [(1.0, 296.0), (0.5, 144.0), (0.1, 24.0)] {
        input.frame.health = health;
        for (ap, width) in [(43.68 / 80.0, 160.0), (52.15 / 80.0, 192.0), (69.7 / 80.0, 256.0), (1.0, 296.0)] {
            input.frame.action_points = ap;
            ui_hud::write(&mut ui, all_shown(), &input);
            ui.update();
            assert_eq!(ui.get_float("HUDMainMenu/HitPoints/*:2/width"), Some(hp_width));
            assert_eq!(ui.get_float("HUDMainMenu/HitPoints/meter/x"), Some(20.0));
            assert_eq!(ui.get_float("HUDMainMenu/ActionPoints/*:5/width"), Some(width));
            assert_eq!(ui.get_float("HUDMainMenu/ActionPoints/*:5/x"), Some(326.0 - width));
        }
    }
    for (condition, width) in [(0.999, 59.0), (0.5, 30.0), (0.4987, 29.0), (0.1, 6.0)] {
        input.frame.condition = Some(condition);
        input.frame.ammo = "12/60".into();
        ui_hud::write(&mut ui, all_shown(), &input);
        ui.update();
        assert_eq!(ui.get_float("HUDMainMenu/ActionPoints/*:1/width"), Some(width));
        assert_eq!(ui.get_float("HUDMainMenu/ActionPoints/*:1/x"), Some(90.0));
        assert_eq!(ui.get_float("HUDMainMenu/ActionPoints/*:1/y"), Some(83.0));
        assert_eq!(ui.get_string("HUDMainMenu/ActionPoints/*:3/string").as_deref(), Some("12/60"));
    }
    assert_eq!(ui.get_string("HUDMainMenu/ActionPoints/*:2/string").as_deref(), Some("CND"));
    assert_eq!(ui.get_string("HUDMainMenu/ActionPoints/*:4/string").as_deref(), Some("AP"));
    assert_eq!(ui.get_float("HUDMainMenu/ActionPoints/*:0/width"), Some(60.0));
}

fn prompt(height: f64) -> ui_hud::Prompt {
    ui_hud::Prompt {
        name: "Goodsprings Settler".into(),
        weight: "--".into(),
        value: "--".into(),
        empty: false,
        locked: false,
        lock_text: "Locked".into(),
        item: false,
        height,
    }
}

#[test]
fn prompt_and_reticle_copies_change_and_keep_absent_strings() {
    let mut ui = copy_tree(&[]);
    let mut input = Texts {
        prompt_verb: Some("Talk".into()),
        ..texts()
    };
    input.frame.prompt = Some(prompt(42.0));
    ui_hud::write(&mut ui, all_shown(), &input);
    ui.update();
    for (i, value) in ["VAL", "--", "WG", "--", "[EMPTY]", "Locked", "Goodsprings Settler", "Talk"].iter().enumerate() {
        assert_eq!(ui.get_string(&format!("HUDMainMenu/Info/*:{}/string", i + 1)).as_deref(), Some(*value));
    }
    for i in [5, 6] {
        assert_eq!(ui.get_float(&format!("HUDMainMenu/Info/*:{i}/visible")), Some(1.0));
        assert_eq!(ui.get_float(&format!("HUDMainMenu/Info/*:{i}/alpha")), Some(0.0));
    }
    assert_eq!(ui.get_float("HUDMainMenu/Info/*:7/height"), Some(42.0));
    assert_eq!(ui.get_float("HUDMainMenu/Info/*:7/y"), Some(2.0));
    assert_eq!(ui.get_string("HUDMainMenu/ReticleCenter/reticle_center/filename").as_deref(), Some("filled.dds"));
    assert_eq!(ui.get_float("HUDMainMenu/ReticleCenter/reticle_center/user0"), Some(0.0));
    assert_eq!(ui.get_string("HUDMainMenu/ReticleCenter/reticle_center/user1").as_deref(), Some("filled.dds"));
    assert_eq!(ui.get_float("HUDMainMenu/ReticleCenter/*:0/depth"), Some(1.0));
    ui_hud::write(&mut ui, Shown { info: false, ..all_shown() }, &texts());
    ui.update();
    assert_eq!(ui.get_string("HUDMainMenu/Info/*:7/string").as_deref(), Some("Goodsprings Settler"));
    assert_eq!(ui.get_string("HUDMainMenu/Info/*:8/string").as_deref(), Some("Talk"));
    assert_eq!(ui.get_float("HUDMainMenu/Info/*:7/visible"), Some(0.0));
    assert_eq!(ui.get_string("HUDMainMenu/ReticleCenter/reticle_center/filename").as_deref(), Some("plain.dds"));
}

#[test]
fn subtitles_follow_both_captured_prompt_heights_and_hold_without_prompt() {
    let mut ui = copy_tree(&[]);
    let mut input = texts();
    ui_hud::write(&mut ui, all_shown(), &input);
    assert_eq!(ui.get_float("HUDMainMenu/Subtitles/y"), Some(660.0));
    for (height, y) in [(42.0, 610.0), (80.0, 572.0)] {
        input.frame.prompt = Some(prompt(height));
        ui_hud::write(&mut ui, all_shown(), &input);
        ui.update();
        assert_eq!(ui.get_float("HUDMainMenu/Subtitles/y"), Some(y));
    }
    ui_hud::write(&mut ui, Shown { info: false, ..all_shown() }, &input);
    assert_eq!(ui.get_float("HUDMainMenu/Subtitles/y"), Some(572.0));
    ui_hud::write(&mut ui, all_shown(), &texts());
    assert_eq!(ui.get_float("HUDMainMenu/Subtitles/y"), Some(572.0));
}

#[test]
fn radiation_pointer_follows_the_paired_capture_range_only_while_shown() {
    let mut ui = copy_tree(&[]);
    let mut input = texts();
    // [observed: private job hud_radiation2 r_in_000-r_in_990; hypothesis:
    // rule between sampled points, within 0.7 units; >1000 clamp unobserved].
    input.frame.radiation = 300.0;
    ui_hud::write(&mut ui, Shown { radiation: false, ..all_shown() }, &input);
    assert_eq!(ui.get_float("HUDMainMenu/RadiationMeter/radiation_pointer/x"), Some(15.0));
    for (rads, x) in [(0.0, 15.0), (70.0, 36.0), (90.0, 42.0), (93.0, 42.9), (998.0, 314.4), (1000.0, 315.0), (1500.0, 315.0), (-1.0, 15.0), (f64::NAN, 15.0)] {
        input.frame.radiation = rads;
        ui_hud::write(&mut ui, all_shown(), &input);
        ui.update();
        assert_eq!(ui.get_float("HUDMainMenu/RadiationMeter/radiation_pointer/x"), Some(x));
    }
}

#[test]
fn compass_marks_write_heading_distance_visibility_and_reuse_slots() {
    use ui_hud::{CompassMark, MarkKind};
    let mut ui = copy_tree(&[]);
    let mut input = texts();
    input.frame.marks = vec![
        CompassMark {
            kind: MarkKind::Actor,
            heading: 139.0,
            distance: 99.0,
        },
        CompassMark {
            kind: MarkKind::Hostile,
            heading: 30.1,
            distance: 99.0,
        },
        CompassMark {
            kind: MarkKind::Quest,
            heading: 148.0,
            distance: 12.0,
        },
        CompassMark {
            kind: MarkKind::Place,
            heading: 272.7,
            distance: 13.0,
        },
        CompassMark {
            kind: MarkKind::Found,
            heading: 279.0,
            distance: 14.0,
        },
    ];
    ui_hud::write(&mut ui, all_shown(), &input);
    ui.update();
    for (path, x, distance, visible) in [
        ("compass_npc_icon_group/*:0", 139.0, 1.0, 1.0),
        ("compass_npc_icon_group/*:1", 30.1, 1.0, 1.0),
        ("compass_icon_group:0/*:0", 148.0, 12.0, 1.0),
        ("compass_icon_group:1/*:0", 272.7, 13.0, 1.0),
        ("compass_icon_group:1/*:1", 279.0, 14.0, 0.0),
    ] {
        let path = format!("HUDMainMenu/HitPoints/compass_window/{path}");
        assert_eq!(ui.get_float(&format!("{path}/_Heading")), Some(x));
        assert_eq!(ui.get_float(&format!("{path}/x")), Some(x));
        assert_eq!(ui.get_float(&format!("{path}/_Distance")), Some(distance));
        assert_eq!(ui.get_float(&format!("{path}/visible")), Some(visible));
    }
    let npc = ui.find_tile("HUDMainMenu/HitPoints/compass_window/compass_npc_icon_group").unwrap();
    assert_eq!(ui.tile(npc).children.len(), 10);
    assert_eq!(ui.tile(npc).get("y"), Some(&Value::Num(10.0)));
    let tick = |i| format!("HUDMainMenu/HitPoints/compass_window/compass_npc_icon_group/*:{i}/systemcolor");
    assert_eq!(ui.get_float(&tick(0)), Some(1.0));
    assert_eq!(ui.get_float(&tick(1)), Some(2.0));
    input.frame.marks[0].kind = MarkKind::Hostile;
    input.frame.marks[1].kind = MarkKind::Actor;
    ui_hud::write(&mut ui, all_shown(), &input);
    assert_eq!(ui.get_float(&tick(0)), Some(2.0));
    assert_eq!(ui.get_float(&tick(1)), Some(1.0));
    let quest = ui.find_tile("HUDMainMenu/HitPoints/compass_window/compass_icon_group:0/*:0").unwrap();
    input.frame.marks.clear();
    ui_hud::write(&mut ui, all_shown(), &input);
    assert_eq!(ui.tile(quest).get("visible"), Some(&Value::Num(0.0)));
    input.frame.marks.push(CompassMark {
        kind: MarkKind::Quest,
        heading: 139.0,
        distance: 1.0,
    });
    ui_hud::write(&mut ui, all_shown(), &input);
    assert_eq!(ui.find_tile("HUDMainMenu/HitPoints/compass_window/compass_icon_group:0/*:0"), Some(quest));
    assert_eq!(ui.tile(quest).get("visible"), Some(&Value::Num(1.0)));
}

#[test]
fn missing_templates_and_parents_skip_quietly_without_cross_writes() {
    ui_hud::build(&mut Ui::new());
    let mut ui = fixture();
    ui_hud::build(&mut ui);
    assert!(ui.warnings.is_empty());
    assert!(ui.find_tile("HUDMainMenu/HitPoints/compass_window").is_none());
    // A mod's own same-named tile is not a code copy [design].
    ui.inject_text(&MemSource::default(), "HUDMainMenu/HitPoints", r#"<image name="meter"><width>17</width></image>"#);
    ui_hud::write(&mut ui, all_shown(), &texts());
    assert_eq!(ui.get_float("HUDMainMenu/HitPoints/meter/width"), Some(17.0));
    for missing in [
        "template_meter",
        "template_justify_left_text",
        "template_compass_npc_icon_group",
        "template_compass_icon_player",
        "template_compass_icon_quest",
    ] {
        let mut ui = copy_tree(&[missing]);
        let mut input = texts();
        input.frame.marks.push(ui_hud::CompassMark {
            kind: ui_hud::MarkKind::Quest,
            heading: 139.0,
            distance: 1.0,
        });
        ui_hud::write(&mut ui, all_shown(), &input);
        ui.update();
        assert!(ui.warnings.is_empty(), "{missing}");
        assert_eq!(ui.get_string("HUDMainMenu/ActionPoints/justify_right_text:0/string").as_deref(), Some("CND"));
        if missing == "template_meter" {
            assert!(ui.find_tile("HUDMainMenu/ActionPoints/meter").is_none());
            assert_eq!(ui.get_float("HUDMainMenu/ActionPoints/MeterBackground/width"), Some(60.0));
        }
        if missing == "template_compass_icon_player" {
            assert_eq!(ui.get_float("HUDMainMenu/HitPoints/compass_window/compass_icon_group:0/*:0/visible"), Some(1.0));
        }
    }
}

#[test]
fn copies_are_code_made_and_not_mod_tiles() {
    // The renderer draws mod-added tiles only; the copies are the engine's [design].
    let ui = copy_tree(&[]);
    let hud = ui.menu("HUDMainMenu").unwrap();
    let window = ui.find_tile("HUDMainMenu/HitPoints/compass_window").unwrap();
    assert_eq!(ui.tile(window).origin, fnv_ui::Origin::Code);
    assert!(ui.mod_added_tiles(hud).is_empty());
}

/// Widths of the observed strings in the game's fonts, by font number as the tiles give
/// it (0: none): the fonts' glyph advances summed, read from the owner's font files
/// (font 1 `glow_monofonto_large`, 7 `baked-in_monofonto_large`, 8
/// `glow_futura_caps_large`) [observed, 2026-10-29]. The font files are not available
/// headless, so this stands in for fnv-font [design].
fn observed_width(font: f64, text: &str) -> f64 {
    match (font as u32, text) {
        (0, "[HIDDEN]") => 106.0,
        (0, "[DETECTED]") => 136.0,
        (7, "Button Text") => 156.0,
        (7, "Talk") => 54.0,
        (8, "+0") => 50.0,
        (8, "+9") => 49.0,
        other => panic!("not measured: {other:?}"),
    }
}

#[test]
fn text_widths_follow_the_observed_measures_and_the_hotrect_its_child() {
    // [design] Synthetic file tiles; the hotrect's width as its prefab works it out:
    // the child's width plus `_horbuf` (20) [documented: text_box.xml].
    let xml = r#"<menu name="HUDMainMenu">
      <rect name="SneakMeter"><text name="sneak_nif"><width>128</width></text></rect>
      <rect name="Info"><rect name="justify_center_hotrect"><_horbuf>20</_horbuf>
        <width><copy src="child(button_text)" trait="width"/><add src="me()" trait="_horbuf"/></width>
        <text name="button_text"><font>7</font><string>Button Text</string></text></rect></rect>
      <rect name="RadiationMeter"><text name="radiation_text_value"><font>8</font><string></string></text></rect>
    </menu>"#;
    let mut ui = Ui::new();
    ui.load_menu(&MemSource::default().with("hud.xml", xml), "hud.xml").unwrap();
    ui.update();
    let width = |ui: &Ui, path: &str| ui.get_float(&format!("HUDMainMenu/{path}/width")).unwrap();
    let mut t = texts();
    let mut shown = all_shown();
    shown.sneak = false;
    ui_hud::write(&mut ui, shown, &t);
    ui_hud::write_text_widths(&mut ui, &observed_width);
    ui.update();
    // No sneak word yet: the file's width stays. The file's "Button Text": 156, box 176.
    assert_eq!(width(&ui, "SneakMeter/sneak_nif"), 128.0);
    assert_eq!(width(&ui, "Info/justify_center_hotrect/button_text"), 156.0);
    assert_eq!(width(&ui, "Info/justify_center_hotrect"), 176.0);
    assert_eq!(width(&ui, "RadiationMeter/radiation_text_value"), 50.0);
    shown.sneak = true;
    t.sneak = Some("[HIDDEN]".into());
    t.prompt_verb = Some("Talk".into());
    t.radiation_value = "+9".into();
    ui_hud::write(&mut ui, shown, &t);
    ui_hud::write_text_widths(&mut ui, &observed_width);
    ui.update();
    assert_eq!(width(&ui, "SneakMeter/sneak_nif"), 106.0);
    assert_eq!(width(&ui, "Info/justify_center_hotrect/button_text"), 54.0);
    assert_eq!(width(&ui, "Info/justify_center_hotrect"), 74.0);
    assert_eq!(width(&ui, "RadiationMeter/radiation_text_value"), 49.0);
    t.sneak = Some("[DETECTED]".into());
    ui_hud::write(&mut ui, shown, &t);
    ui_hud::write_text_widths(&mut ui, &observed_width);
    assert_eq!(width(&ui, "SneakMeter/sneak_nif"), 136.0);
    // Standing: the word and its width stay [observed: hud.md §14.1].
    shown.sneak = false;
    t.sneak = None;
    ui_hud::write(&mut ui, shown, &t);
    ui_hud::write_text_widths(&mut ui, &observed_width);
    assert_eq!(width(&ui, "SneakMeter/sneak_nif"), 136.0);
}

#[test]
fn prompt_item_empty_and_lock_copies_match_the_captured_paths() {
    let mut ui = copy_tree(&[]);
    let mut input = texts();
    let mut p = prompt(42.0);
    p.item = true;
    p.name = "9mm Pistol".into();
    p.weight = "1.5".into();
    p.value = "100".into();
    input.frame.prompt = Some(p.clone());
    ui_hud::write(&mut ui, all_shown(), &input);
    for (index, x, y) in [(1, 165.0, 60.0), (2, 290.0, 60.0), (3, 10.0, 60.0), (4, 135.0, 60.0), (5, 160.0, 65.0), (6, 160.0, 65.0), (7, 160.0, 2.0)] {
        assert_eq!(ui.get_float(&format!("HUDMainMenu/Info/*:{index}/x")), Some(x));
        assert_eq!(ui.get_float(&format!("HUDMainMenu/Info/*:{index}/y")), Some(y));
    }
    for i in 0..=4 {
        assert_eq!(ui.get_float(&format!("HUDMainMenu/Info/*:{i}/visible")), Some(1.0));
    }
    assert_eq!(ui.get_string("HUDMainMenu/Info/*:2/string").as_deref(), Some("100"));
    assert_eq!(ui.get_string("HUDMainMenu/Info/*:4/string").as_deref(), Some("1.5"));
    p.item = false;
    p.empty = true;
    p.value = "--".into();
    p.weight = "--".into();
    input.frame.prompt = Some(p.clone());
    ui_hud::write(&mut ui, all_shown(), &input);
    for i in 0..=4 {
        assert_eq!(ui.get_float(&format!("HUDMainMenu/Info/*:{i}/visible")), Some(0.0));
    }
    assert_eq!(ui.get_float("HUDMainMenu/Info/*:5/alpha"), Some(255.0));
    p.locked = true;
    p.lock_text = "[Locked - Average]".into();
    input.frame.prompt = Some(p);
    ui_hud::write(&mut ui, all_shown(), &input);
    assert_eq!(ui.get_float("HUDMainMenu/Info/*:5/visible"), Some(0.0));
    assert_eq!(ui.get_float("HUDMainMenu/Info/*:6/alpha"), Some(255.0));
    assert_eq!(ui.get_string("HUDMainMenu/Info/*:6/string").as_deref(), Some("[Locked - Average]"));
    input.frame.prompt = None;
    ui_hud::write(&mut ui, all_shown(), &input);
    for i in 0..=8 {
        assert_eq!(ui.get_float(&format!("HUDMainMenu/Info/*:{i}/visible")), Some(0.0));
    }
    assert_eq!(ui.get_string("HUDMainMenu/Info/*:6/string").as_deref(), Some("[Locked - Average]"));
}

#[test]
fn enemy_ticks_colour_and_engine_visibility() {
    let mut ui = copy_tree(&[]);
    let mut input = texts();
    for (health, width) in [(7.01 / 45.0, 32.0), (8.01 / 45.0, 32.0), (0.5, 112.0), (1.0, 240.0)] {
        for hostile in [false, true] {
            input.frame.enemy = Some(ui_hud::Enemy { name: "Actor".into(), health, hostile });
            // [design, #2659] The engine suppresses full health outside classic VATS.
            ui_hud::write(&mut ui, Shown { enemy: health < 1.0, ..all_shown() }, &input);
            assert_eq!(ui.get_float("HUDMainMenu/EnemyHealth/*:1/width"), Some(width));
            assert_eq!(ui.get_float("HUDMainMenu/EnemyHealth/*:1/x"), Some(171.0 - width / 2.0));
            assert_eq!(ui.get_float("HUDMainMenu/EnemyHealth/systemcolor"), Some(if hostile { 2.0 } else { 1.0 }));
            for i in 0..3 {
                assert_eq!(ui.get_float(&format!("HUDMainMenu/EnemyHealth/*:{i}/visible")), Some(f64::from(health < 1.0)));
            }
            // [design, #2659] Classic explicitly shows the selected target at full health.
            ui_hud::write(&mut ui, all_shown(), &input);
            for i in 0..3 {
                assert_eq!(ui.get_float(&format!("HUDMainMenu/EnemyHealth/*:{i}/visible")), Some(1.0));
            }
        }
    }
    assert_eq!(ui.get_float("HUDMainMenu/EnemyHealth/*:2/width"), Some(500.0));
    assert_eq!(ui.get_float("HUDMainMenu/EnemyHealth/*:2/brightness"), Some(175.0));
    assert_eq!(ui.get_float("HUDMainMenu/EnemyHealth/*:1/height"), Some(20.0));
    input.frame.aim_hostile = true;
    ui_hud::write(&mut ui, all_shown(), &input);
    assert_eq!(ui.get_float("HUDMainMenu/ReticleCenter/reticle_center/systemcolor"), Some(2.0));
    assert_eq!(ui.get_string("HUDMainMenu/ReticleCenter/reticle_center/filename").as_deref(), Some("plain.dds"));
    ui_hud::write(&mut ui, Shown { aiming: true, ..all_shown() }, &input);
    assert_eq!(ui.get_float("HUDMainMenu/ReticleCenter/reticle_center/systemcolor"), Some(1.0));
}

fn animated_tree() -> Ui {
    let mut ui = fixture();
    let src = MemSource::default();
    for name in ["XPBracket", "XPPointer", "XPAmount", "XPLastLevel", "XPNextLevel"] {
        ui.inject_text(&src, "HUDMainMenu/XPMeter", &format!(r#"<image name="{name}"/>"#));
    }
    let pictures: String = ["Head", "Face", "Torso", "Left_Arm", "Right_Arm", "Left_Leg", "Right_Leg"].iter().map(|n| format!(r#"<image name="{n}"/>"#)).collect();
    ui.inject_text(&src, "HUDMainMenu/crippled_limb_indicator", &pictures);
    ui
}

#[test]
fn need_words_keep_places_and_use_stage_alpha_and_colour_in_both_modes() {
    let mut ui = fixture();
    let mut input = texts();
    input.frame.hardcore = true;
    for (stage, alpha, colour) in [(0, 0.0, 1.0), (1, 124.0, 1.0), (2, 255.0, 1.0), (3, 255.0, 2.0), (4, 255.0, 2.0)] {
        input.frame.need_stages = [stage; 4];
        ui_hud::write(&mut ui, all_shown(), &input);
        for name in ["Dehydration", "Hunger", "Sleep", "Rads"] {
            assert_eq!(ui.get_float(&format!("HUDMainMenu/HardcoreMode/{name}/alpha")), Some(alpha));
            assert_eq!(ui.get_float(&format!("HUDMainMenu/HardcoreMode/{name}/systemcolor")), Some(colour));
        }
    }
    input.frame.animation.seconds = 0.3;
    ui_hud::write(&mut ui, all_shown(), &input);
    assert_eq!(ui.get_float("HUDMainMenu/HardcoreMode/Rads/alpha"), Some(128.0));
    input.frame.need_stages = [2; 4];
    for hardcore in [false, true] {
        input.frame.hardcore = hardcore;
        // T12e: three crippled limbs draw the word red, as the original's.
        for (limbs, alpha, colour) in [(0, 0.0, 1.0), (1, 124.0, 1.0), (2, 255.0, 1.0), (3, 255.0, 2.0), (6, 255.0, 2.0)] {
            input.frame.crippled = std::array::from_fn(|i| i < limbs);
            ui_hud::write(&mut ui, all_shown(), &input);
            assert_eq!(ui.get_float("HUDMainMenu/HardcoreMode/LMBs/alpha"), Some(alpha));
            assert_eq!(ui.get_float("HUDMainMenu/HardcoreMode/LMBs/systemcolor"), Some(colour), "{limbs} limbs");
            assert_eq!(ui.get_float("HUDMainMenu/HardcoreMode/Dehydration/alpha"), Some(if hardcore { 255.0 } else { 0.0 }));
            for (name, x) in [("Dehydration", 0.0), ("Hunger", 55.0), ("Sleep", 109.0), ("Rads", 162.0), ("LMBs", 218.0)] {
                assert_eq!(ui.get_float(&format!("HUDMainMenu/HardcoreMode/{name}/x")), Some(x));
                assert_eq!(ui.get_float(&format!("HUDMainMenu/HardcoreMode/{name}/zoom")), Some(75.0));
            }
        }
    }
}

#[test]
fn xp_timeline_fades_glides_keeps_the_first_gain_and_shows_level_up_separately() {
    use fnv_game::experience::Standing;
    let mut animation = ui_hud::Animation::default();
    let mut standing = Standing { xp: 10.0, level: 1, max: 30, this: 0.0, next: 200.0 };
    let mut ui = animated_tree();
    let mut input = texts();
    let healthy = [false; 6];
    animation.step(0.0, true, standing, healthy);
    standing.xp = 35.0;
    animation.step(0.0, true, standing, healthy);
    standing.xp = 40.0;
    input.frame.animation = animation.step(0.6, true, standing, healthy);
    ui_hud::write(&mut ui, all_shown(), &input);
    assert_eq!(ui.get_string("HUDMainMenu/XPMeter/XPAmount/string").as_deref(), Some("+25"));
    assert_eq!(ui.get_float("HUDMainMenu/XPMeter/XPBracket/alpha"), Some(76.5));
    let paused = animation.step(10.0, false, standing, healthy);
    assert_eq!(paused.xp.as_ref().unwrap().age, 0.6);
    standing.xp = 200.0;
    for (dt, alpha, pointer, level_on) in [(1.4, 255.0, 65.75, false), (1.0, 255.0, 167.875, false), (1.0, 255.0, 270.0, false), (1.0, 127.5, 270.0, false), (1.0, 0.0, 270.0, false), (1.8, 0.0, 270.0, true)] {
        input.frame.animation = animation.step(dt, true, standing, healthy);
        ui_hud::write(&mut ui, all_shown(), &input);
        assert!((ui.get_float("HUDMainMenu/XPMeter/XPBracket/alpha").unwrap() - alpha).abs() < 1e-6);
        assert!((ui.get_float("HUDMainMenu/XPMeter/XPPointer/x").unwrap() - pointer).abs() < 1e-5);
        assert_eq!(ui.get_float("HUDMainMenu/XPMeter/XPLevelUp/visible"), Some(f64::from(level_on)));
        if level_on {
            assert_eq!(ui.get_float("HUDMainMenu/XPMeter/XPLevelUp/alpha"), Some(255.0));
        }
    }
    input.frame.animation = animation.step(2.0, true, standing, healthy);
    ui_hud::write(&mut ui, all_shown(), &input);
    assert!(input.frame.animation.xp.is_none());
    assert_eq!(ui.get_float("HUDMainMenu/XPMeter/XPLevelUp/visible"), Some(0.0));
}

#[test]
fn limb_timeline_restarts_all_broken_limbs_and_healing_starts_nothing() {
    use fnv_game::experience::Standing;
    let mut animation = ui_hud::Animation::default();
    let standing = Standing { xp: 10.0, level: 1, max: 30, this: 0.0, next: 200.0 };
    let mut ui = animated_tree();
    let mut input = texts();
    let mut limbs = [false; 6];
    animation.step(0.0, true, standing, limbs);
    limbs[4] = true;
    animation.step(0.0, true, standing, limbs);
    for (dt, whole_alpha) in [(0.425, 127.5), (0.425, 255.0), (3.3, 255.0), (0.425, 127.5), (0.425, 0.0)] {
        input.frame.animation = animation.step(dt, true, standing, limbs);
        ui_hud::write(&mut ui, all_shown(), &input);
        assert!((ui.get_float("HUDMainMenu/crippled_limb_indicator/Head/alpha").unwrap() - whole_alpha).abs() < 1e-6);
        assert_eq!(ui.get_float("HUDMainMenu/crippled_limb_indicator/Left_Leg/systemcolor"), Some(2.0));
        assert_eq!(ui.get_float("HUDMainMenu/crippled_limb_indicator/Left_Leg/x"), Some(24.75));
    }
    assert_eq!(ui.get_float("HUDMainMenu/crippled_limb_indicator/Left_Leg/alpha"), Some(-16.0));
    limbs[0] = true;
    animation.step(0.0, true, standing, limbs);
    input.frame.animation = animation.step(0.17, true, standing, limbs);
    ui_hud::write(&mut ui, all_shown(), &input);
    assert_eq!(ui.get_float("HUDMainMenu/crippled_limb_indicator/Head/alpha"), ui.get_float("HUDMainMenu/crippled_limb_indicator/Left_Leg/alpha"));
    limbs = [false; 6];
    input.frame.animation = animation.step(0.0, true, standing, limbs);
    ui_hud::write(&mut ui, all_shown(), &input);
    assert!(input.frame.animation.limb_age.is_none());
    assert_eq!(ui.get_string("HUDMainMenu/crippled_limb_indicator/Head/filename").as_deref(), Some("Interface\\Stats\\head_broken.dds"));
    assert!(ui.get_float("HUDMainMenu/crippled_limb_indicator/Head/alpha").unwrap() < 0.0);
}

/// A mod's HUD text with no `width` of its own is left to the engine to measure, and stays
/// so once the engine wrote one; a text whose file gives a width, and the vanilla pieces the
/// engine measures on their own path, are not (#4493).
#[test]
fn mod_texts_without_a_width_are_measured_by_the_engine() {
    let xml = r#"<menu name="HUDMainMenu"><rect name="ModClock"><text name="Clock"><string>12:00</string></text><text name="Sized"><width>50</width><string>x</string></text></rect><rect name="XPMeter"><text name="XPLabel"><string>XP</string></text></rect></menu>"#;
    let mut ui = Ui::new();
    ui.load_menu(&MemSource::default().with("hud.xml", xml), "hud.xml").unwrap();
    let clock = ui.find_tile("HUDMainMenu/ModClock/Clock").unwrap();
    let sized = ui.find_tile("HUDMainMenu/ModClock/Sized").unwrap();
    let known = ui_hud::measured_texts(&ui);
    let got = ui_hud::unmeasured_texts(&ui);
    assert!(got.contains(&clock), "{got:?}");
    assert!(!got.contains(&sized));
    assert!(got.iter().all(|t| !known.contains(t)), "the vanilla pieces keep their own measure");
    // Once the engine wrote its width (and marked it), it is still the engine's to measure.
    ui.set_trait(clock, "width", Value::Num(40.0));
    ui.set_trait(clock, ui_hud::MEASURED_MARK, Value::Num(1.0));
    assert!(ui_hud::unmeasured_texts(&ui).contains(&clock));
}
