//! Scoped unit tests for ySI JSON category/subfolder parsing and priority resolution
//! (`Book::build_categories`, `YsiCategoryDef`, `YsiCategoryIndex`).

use super::*;
use serde_json::json;

fn icon_rule(priority: i64, tag: &str, obj: Json) -> Rule {
    let mut fields = obj.as_object().cloned().unwrap();
    fields.insert("priority".into(), Json::from(priority));
    fields.insert("tag".into(), Json::from(tag));
    Rule {
        priority,
        tag: tag.to_string(),
        fields,
    }
}

#[test]
fn parses_flat_categories_priority_overrides_and_game_string_names() {
    let book = Book {
        tags: Vec::new(),
        icons_by_tag: std::collections::HashMap::new(),
        icons: vec![
            // Base ySI.json Misc categories (priority 100)
            icon_rule(
                100,
                "(jdd0)",
                json!({
                    "template": "ySIDefault",
                    "filename": "interface\\icons\\Misc_Keychain.dds",
                    "name": "&-sKeyring;",
                    "tab": 3,
                    "count": 2,
                    "icon": "Interface\\Icons\\PipboyImages\\Items\\item_keyring.dds"
                }),
            ),
            icon_rule(
                100,
                "(jdd1)",
                json!({
                    "template": "ySIDefault",
                    "filename": "interface\\icons\\Misc_Key.dds",
                    "category": "(jdd0)"
                }),
            ),
            // Base Weapon category (priority 200)
            icon_rule(
                200,
                "(kw00)",
                json!({
                    "template": "ySIDefault",
                    "filename": "interface\\icons\\WeapA_Rifle_FullAuto.dds",
                    "name": "Automatic Rifles",
                    "tab": 0,
                    "count": 1,
                    "icon": "interface\\icons\\WeapA_Rifle_FullAuto.dds"
                }),
            ),
            // Higher-priority preview override (priority 900) replaces preview icon cleanly
            icon_rule(
                900,
                "(kw00)",
                json!({
                    "template": "ySIDefault",
                    "filename": "interface\\icons\\WeapA_Rifle_FullAuto.dds",
                    "name": "Automatic Rifles",
                    "tab": 0,
                    "count": 1,
                    "icon": "interface\\custom\\kw00_preview.dds"
                }),
            ),
            icon_rule(
                200,
                "(fbb3)",
                json!({
                    "template": "ySIDefault",
                    "filename": "interface\\icons\\WeapA_Rifle_FullAuto.dds",
                    "category": "(kw00)"
                }),
            ),
        ],
    };

    let idx = book.build_categories(|raw| match raw {
        "&-sKeyring;" => "Keyring".into(),
        other => other.into(),
    });

    assert_eq!(idx.categories.len(), 2);
    // Ordered deterministically by (tab, tag): tab 0 (kw00) before tab 3 (jdd0).
    let weap = &idx.categories[0];
    assert_eq!(weap.tag, "(kw00)");
    assert_eq!(weap.name, "Automatic Rifles");
    assert_eq!(weap.tab, 0);
    assert_eq!(weap.parent, None);
    assert_eq!(weap.min_count, 1);
    assert_eq!(weap.preview_icon.as_deref(), Some("interface\\custom\\kw00_preview.dds"));

    let misc = &idx.categories[1];
    assert_eq!(misc.tag, "(jdd0)");
    assert_eq!(misc.name, "Keyring");
    assert_eq!(misc.tab, 3);
    assert_eq!(misc.min_count, 2);

    assert_eq!(idx.tag_to_category.get("(fbb3)"), Some(&0));
    assert_eq!(idx.tag_to_category.get("(jdd1)"), Some(&1));
}

#[test]
fn resolves_nested_subfolders_inheriting_tab_and_rejects_cycles() {
    let book = Book {
        tags: Vec::new(),
        icons_by_tag: std::collections::HashMap::new(),
        icons: vec![
            // Top-level Weapons parent category
            icon_rule(
                100,
                "(cat_ballistic)",
                json!({
                    "name": "Ballistic",
                    "tab": 0,
                    "count": 1,
                    "filename": "interface\\icons\\WeapA_Rifle.dds"
                }),
            ),
            // Nested child subfolder (no explicit tab; inherits tab 0 from parent)
            icon_rule(
                100,
                "(cat_pistols)",
                json!({
                    "name": "Pistols",
                    "category": "(cat_ballistic)",
                    "count": 1,
                    "filename": "interface\\icons\\WeapA_Pistol.dds"
                }),
            ),
            // Item tag inside nested subfolder
            icon_rule(
                100,
                "(fba1)",
                json!({
                    "filename": "interface\\icons\\WeapA_Pistol.dds",
                    "category": "(cat_pistols)"
                }),
            ),
            // Cyclic pair must be discarded cleanly without hanging
            icon_rule(
                100,
                "(cyc_a)",
                json!({ "name": "Cycle A", "tab": 0, "category": "(cyc_b)" }),
            ),
            icon_rule(
                100,
                "(cyc_b)",
                json!({ "name": "Cycle B", "category": "(cyc_a)" }),
            ),
            // Orphan subfolder without root tab must be ignored
            icon_rule(
                100,
                "(orphan)",
                json!({ "name": "Orphan", "category": "(missing_parent)" }),
            ),
        ],
    };

    let idx = book.build_categories(|s| s.to_string());
    assert_eq!(idx.categories.len(), 2);
    assert_eq!(idx.categories[0].tag, "(cat_ballistic)");
    assert_eq!(idx.categories[0].parent, None);
    assert_eq!(idx.categories[0].tab, 0);
    assert_eq!(idx.categories[1].tag, "(cat_pistols)");
    assert_eq!(idx.categories[1].parent, Some(0));
    assert_eq!(idx.categories[1].tab, 0);
    assert_eq!(idx.tag_to_category.get("(fba1)"), Some(&1));
}
