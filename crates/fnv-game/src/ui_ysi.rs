//! Clean-room `menus/ySI/ySI.xml` menu injection and icon-template binding contract
//! (`docs/spec-notes/ysi-ui-injection.md`).
//!
//! When `menus\ySI\ySI.xml` is absent from the VFS / [`Source`], this module does nothing
//! (zero overhead and zero tile-tree changes). When present, it injects `menus\ySI\ySI.xml`
//! into `HUDMainMenu` and item-list menus so scripts and UI expressions can read `ySI`
//! sizing/spacing traits and instantiate `ySIDefault` / `ySINoTag` icon templates, while
//! deduplicating against menus that already received a `ySI` tile or template (for example
//! via a UIO manifest or loose menu override).
//!
//! This module does not claim `GetPluginVersion "yUI" >= 140`, does not touch
//! `ysi_filename.rs` or `nvse_small.rs`, and does not implement inventory sorting or
//! renderer-side icon drawing.

use crate::game::menus;
use fnv_ui::expr::{Arg, Op, OpKind, Src};
use fnv_ui::{xml::Node, Source, TileId, Ui};

/// Canonical VFS path of ySI's XML fragment.
pub const YSI_XML_PATH: &str = "menus\\ySI\\ySI.xml";
/// Root configuration rect declared by `menus/ySI/ySI.xml`.
pub const YSI_ROOT_TILE: &str = "ySI";
/// Default icon template declared alongside `ySI`.
pub const YSI_DEFAULT_TEMPLATE: &str = "ySIDefault";
/// Untyped/no-tag icon template declared alongside `ySI`.
pub const YSI_NO_TAG_TEMPLATE: &str = "ySINoTag";
/// Default instantiated icon child tile name.
pub const YSI_IMAGE_TILE: &str = "ySIImage";
/// Private trait tracking which template built an instantiated [`YSI_IMAGE_TILE`] child.
const YSI_TEMPLATE_TRAIT: &str = "_ysi_template";
/// Private trait recording a row's `ListItemText` base `x` prior to ySI icon spacing: the
/// file's literal value, or the file's own operators when `x` is an expression.
const YSI_BASE_X_TRAIT: &str = "_ysi_base_x";
/// Private trait holding how far an expression-driven `ListItemText/x` is moved right.
const YSI_X_SHIFT_TRAIT: &str = "_ysi_x_shift";

/// Resolved ySI sorting tag, icon texture filename, and XML template name for an item form
/// [design: `docs/spec-notes/ysi-filename.md`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YsiItemDisplay {
    /// Sorting tag (session `ySISetTrait "tag"` override or JSON rule match).
    pub tag: Option<String>,
    /// Icon DDS path under `textures\` (session `ySISetTrait "icon"` override or JSON icon rule).
    pub icon: Option<String>,
    /// Icon template name (`ySIDefault`, `ySINoTag`, or a custom template named by the icon rule).
    pub template: String,
}

/// Picks the icon template name: explicit non-empty rule template, else `ySIDefault` when
/// tagged/icon-bearing or `ySINoTag` otherwise [documented: ySI - Sorting Ycons v1.4b
/// (Nexus Mods 74358), shipped `menus/ySI/ySI.xml` and `menus/ySI/ySI.json` declare the
/// `ySIDefault` and `ySINoTag` template names (no yUI/ySI DLL or source consulted);
/// hypothesis: unprobed row-rendering fallback from an omitted rule template to
/// `ySIDefault`/`ySINoTag` (note `ySIGetTrait "template"` returns `""` in yUI v1.4d per
/// `w4e_02_ysi_metadata_traits`)].
pub fn row_template_or_default(template: Option<&str>, has_icon_or_tag: bool) -> String {
    if let Some(t) = template.map(str::trim).filter(|s| !s.is_empty()) {
        return t.to_string();
    }
    if has_icon_or_tag {
        YSI_DEFAULT_TEMPLATE.to_string()
    } else {
        YSI_NO_TAG_TEMPLATE.to_string()
    }
}

/// Whether the menu root owning `tile` has `ySI` injected (a direct `ySI` tile or either
/// core `ySIDefault`/`ySINoTag` template). Returns `false` when `menus/ySI/ySI.xml` is absent.
pub fn menu_has_ysi(ui: &Ui, tile: TileId) -> bool {
    if !ui.is_live(tile) {
        return false;
    }
    let root = ui.root_of(tile);
    ui.is_live(root)
        && (has_direct_child(ui, root, YSI_ROOT_TILE).is_some()
            || has_template(ui, root, YSI_DEFAULT_TEMPLATE)
            || has_template(ui, root, YSI_NO_TAG_TEMPLATE))
}

/// Menu class code of `HUDMainMenu` (`fnv_ui::value::MENU_CLASSES`).
const HUD_MENU_CODE: u16 = 1004;

/// Menus that host `menus/ySI/ySI.xml` when present in the VFS [documented: ySI - Sorting
/// Ycons v1.4b (Nexus Mods 74358), shipped `menus/ySI/ySI.xml` (no yUI/ySI DLL or source
/// consulted); observed: `BarterMenu` injection (`barter(mm=1053,ysi=1,pip=0)`, i.e.
/// `_ySI=1`, `_ySIPipBoy=0`) in `w4e_05_ysi_ini_precedence_and_gating`
/// (`tools/capture/nvse/catalogs/wave4e_mods.json`, #3803); hypothesis: remaining target
/// menu list (`docs/spec-notes/ysi-ui-injection.md`)].
pub const TARGET_MENUS: &[(u16, &str)] = &[
    (HUD_MENU_CODE, "HUDMainMenu"),
    (menus::INVENTORY, "InventoryMenu"),
    (menus::BARTER, "BarterMenu"),
    (1008, "ContainerMenu"),
    (menus::REPAIR, "RepairMenu"),
    (1058, "RepairServicesMenu"),
    (1061, "ItemModMenu"),
    (1077, "RecipeMenu"),
];

/// Whether `menus\ySI\ySI.xml` exists in `files`.
pub fn is_present(files: &dyn Source) -> bool {
    files.read(YSI_XML_PATH).is_some()
}

/// Whether `class_code` or `menu_name` identifies a menu targeted for `ySI.xml` injection.
pub fn is_target_menu(class_code: Option<u16>, menu_name: &str) -> bool {
    if let Some(code) = class_code {
        if TARGET_MENUS.iter().any(|&(c, _)| c == code) {
            return true;
        }
    }
    TARGET_MENUS.iter().any(|&(_, name)| name.eq_ignore_ascii_case(menu_name))
}

fn has_direct_child(ui: &Ui, parent: TileId, name: &str) -> Option<TileId> {
    ui.tile(parent)
        .children
        .iter()
        .copied()
        .find(|&c| ui.is_live(c) && ui.tile(c).name.eq_ignore_ascii_case(name))
}

fn direct_children(ui: &Ui, parent: TileId, name: &str) -> Vec<TileId> {
    ui.tile(parent)
        .children
        .iter()
        .copied()
        .filter(|&c| ui.is_live(c) && ui.tile(c).name.eq_ignore_ascii_case(name))
        .collect()
}

fn has_template(ui: &Ui, tile: TileId, name: &str) -> bool {
    ui.tile(tile).templates.iter().any(|t| t.name.eq_ignore_ascii_case(name))
}

fn is_tile_tag(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "menu" | "rect" | "image" | "text" | "nif" | "radial" | "hotrect" | "window"
    )
}

fn find_valid_template_body<'a>(ui: &'a Ui, start: TileId, template_name: &str) -> Option<&'a [Node]> {
    let mut look = Some(start);
    while let Some(t) = look {
        if let Some(tp) = ui.tile(t).templates.iter().find(|tp| tp.name.eq_ignore_ascii_case(template_name)) {
            let has_tile = tp.body.iter().any(|n| matches!(n, Node::Element(e) if is_tile_tag(&e.name)));
            return has_tile.then_some(tp.body.as_slice());
        }
        look = ui.tile(t).parent;
    }
    None
}

/// Inject `menus\ySI\ySI.xml` into a freshly loaded target menu `root` when present in
/// `files` and not already injected on `root` (deduplicating against UIO manifests or
/// loose menu overrides).
///
/// Returns `true` if `menus\ySI\ySI.xml` was injected into `root`, or `false` if skipped,
/// absent, empty, or already present.
pub fn publish_for_menu(ui: &mut Ui, files: &dyn Source, root: TileId, class_code: Option<u16>) -> bool {
    if !ui.is_live(root) {
        return false;
    }
    let menu_name = ui.tile(root).name.clone();
    if menu_name.is_empty() || !is_target_menu(class_code, &menu_name) {
        return false;
    }
    // Deduplicate: skip if UIO, a loose menu replacement, or an earlier call already
    // installed the `ySI` rect or either core ySI template on this menu root.
    if has_direct_child(ui, root, YSI_ROOT_TILE).is_some()
        || has_template(ui, root, YSI_DEFAULT_TEMPLATE)
        || has_template(ui, root, YSI_NO_TAG_TEMPLATE)
    {
        return false;
    }
    let Some(bytes) = files.read(YSI_XML_PATH) else {
        return false;
    };
    let text = fnv_ui::xml::decode(&bytes);
    if text.trim().is_empty() {
        return false;
    }
    ui.inject_text(files, &menu_name, &text)
}

/// Re-align a row's existing `ySIImage/x` after `ui.update()` has evaluated expression-driven
/// `ListItemText/x` / `_ysi_base_x` and `ySI` spacing traits on freshly created rows [design].
///
/// Returns `true` if `ySIImage/x` was changed.
pub fn sync_row_icon_x(ui: &mut Ui, row_tile: TileId) -> bool {
    if !ui.is_live(row_tile) {
        return false;
    }
    let Some(icon_tile) = has_direct_child(ui, row_tile, YSI_IMAGE_TILE) else {
        return false;
    };
    let Some(text_tile) = has_direct_child(ui, row_tile, "ListItemText") else {
        return false;
    };
    let root = ui.root_of(row_tile);
    let ysi_rect = has_direct_child(ui, root, YSI_ROOT_TILE);
    let ysi_num = |trait_name: &str| {
        ysi_rect
            .and_then(|r| ui.tile(r).get(trait_name))
            .map(fnv_ui::Value::num)
    };
    let icon_num = |trait_name: &str| ui.tile(icon_tile).get(trait_name).map(fnv_ui::Value::num);

    let has_icon = !ui
        .tile(icon_tile)
        .get(YSI_TEMPLATE_TRAIT)
        .is_some_and(|v| v.text().eq_ignore_ascii_case(YSI_NO_TAG_TEMPLATE));
    let blanks_on = ysi_num("_ySIBlanks").is_none_or(|v| v != 0.0);
    let width = if has_icon || blanks_on {
        ysi_num("_ySIWidth")
            .or_else(|| icon_num("width"))
            .unwrap_or(0.0)
            .max(0.0)
    } else {
        0.0
    };
    let space_left = ysi_num("_ySISpaceLeft")
        .or_else(|| icon_num("user0"))
        .unwrap_or(0.0);

    let base_x = ui
        .tile(text_tile)
        .get(YSI_BASE_X_TRAIT)
        .or_else(|| ui.tile(text_tile).get("x"))
        .map_or(0.0, fnv_ui::Value::num);
    let want_x = if width > 0.0 {
        base_x + space_left
    } else {
        base_x
    };
    if ui.tile(icon_tile).get("x").map(fnv_ui::Value::num) == Some(want_x) {
        return false;
    }
    ui.set_trait(icon_tile, "x", fnv_ui::Value::Num(want_x));
    true
}

/// Safely instantiate a ySI icon template (such as `ySIDefault` or `ySINoTag`) under
/// `parent_path`, naming the created tile `tile_name` (defaulting to [`YSI_IMAGE_TILE`]).
///
/// Refuses blank paths/template names, missing parent tiles, missing templates, and
/// malformed/empty templates (containing no tile element) without panicking or mutating
/// `parent_path`'s traits. If `parent_path` already has a live child with `tile_name`,
/// returns that existing tile without duplicating it.
pub fn instantiate_icon(
    ui: &mut Ui,
    files: &dyn Source,
    parent_path: &str,
    template_name: &str,
    tile_name: Option<&str>,
) -> Option<TileId> {
    let parent_path = parent_path.trim();
    let template_name = template_name.trim();
    if parent_path.is_empty() || template_name.is_empty() {
        return None;
    }
    let parent = ui.find_tile(parent_path)?;
    let target_name = tile_name.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(YSI_IMAGE_TILE);
    if let Some(existing) = has_direct_child(ui, parent, target_name) {
        return Some(existing);
    }
    // Verify the resolved template body actually constructs a tile before calling
    // `add_from_template`, so malformed trait-only templates leave `parent` untouched.
    find_valid_template_body(ui, parent, template_name)?;
    ui.add_from_template(files, parent_path, template_name, Some(target_name))
}

/// Attach or update the [`YSI_IMAGE_TILE`] child on a live list `row_tile` when its owning
/// menu has `ySI` injected (`menus/ySI/ySI.xml`).
///
/// - When `ySI` is not installed on the menu, this is a complete no-op (`false`).
/// - When `display` provides a non-empty `icon`, instantiates (or reuses) `ySIImage` from
///   `display.template` (falling back to `ySIDefault` then `ySINoTag` if a custom template
///   is absent), writes `filename`, and makes the tile visible.
/// - When `display` is `None` or has no icon, instantiates `ySINoTag` when available (for
///   `_ySIBlanks` alignment) or hides any previously instantiated `ySIImage` tile cleanly.
pub fn apply_row_icon(
    ui: &mut Ui,
    files: &dyn Source,
    row_tile: TileId,
    display: Option<&YsiItemDisplay>,
) -> bool {
    if !menu_has_ysi(ui, row_tile) {
        return false;
    }
    let icon_path = display
        .and_then(|d| d.icon.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let chosen_template: Option<String> = if let Some(d) = display.filter(|_| icon_path.is_some()) {
        let mut candidates = [d.template.as_str(), YSI_DEFAULT_TEMPLATE, YSI_NO_TAG_TEMPLATE]
            .into_iter()
            .filter(|s| !s.trim().is_empty());
        candidates
            .find(|tpl| find_valid_template_body(ui, row_tile, tpl).is_some())
            .map(str::to_string)
    } else if find_valid_template_body(ui, row_tile, YSI_NO_TAG_TEMPLATE).is_some() {
        Some(YSI_NO_TAG_TEMPLATE.to_string())
    } else {
        None
    };

    let existing_images = direct_children(ui, row_tile, YSI_IMAGE_TILE);
    let Some(template_name) = chosen_template else {
        for (idx, existing) in existing_images.into_iter().enumerate() {
            if idx == 0 {
                ui.set_trait(existing, "visible", fnv_ui::Value::Num(0.0));
            } else {
                ui.unload_tile(existing);
            }
        }
        return false;
    };

    let mut kept = None;
    for existing in existing_images {
        let same_tpl = kept.is_none()
            && ui
                .tile(existing)
                .get(YSI_TEMPLATE_TRAIT)
                .is_some_and(|v| v.text().eq_ignore_ascii_case(&template_name));
        if same_tpl {
            kept = Some(existing);
        } else {
            ui.unload_tile(existing);
        }
    }

    let tile = if let Some(existing) = kept {
        existing
    } else {
        let Some(created) =
            ui.add_code_tile_from_template_under(files, row_tile, &template_name, Some(YSI_IMAGE_TILE))
        else {
            return false;
        };
        ui.set_trait(created, YSI_TEMPLATE_TRAIT, fnv_ui::Value::Str(template_name));
        created
    };

    if let Some(filename) = icon_path {
        ui.set_trait(tile, "filename", fnv_ui::Value::Str(filename.to_string()));
        ui.set_trait(tile, "visible", fnv_ui::Value::Num(1.0));
    }
    position_row_icon(ui, row_tile, tile, icon_path.is_some());
    true
}

/// Position `ySIImage`'s `x` and shift `ListItemText`'s `x` according to the
/// `ySI` rect's `_ySIWidth` / `_ySISpaceLeft` (`user0`) / `_ySISpaceRight` (`user1`) /
/// `_ySIBlanks` traits [documented: Sorting Icons 1.4b package, `menus/ySI/ySI.xml`: the
/// icon's `x` is "set in code", `user0` is the "additional space left between equip square
/// and icon" and `user1` the "additional space left between icon and text"; no ySI/yUI DLL
/// or source code was consulted]. A literal `ListItemText/x` is shifted idempotently via
/// the private `_ysi_base_x` [design]. An expression-driven `ListItemText/x` (UI overhauls
/// that place the text from the font and the equip square, such as VUI+) keeps the file's
/// operators: they move to `_ysi_base_x` and `x` becomes that plus `_ysi_x_shift`, so the
/// text moves past the icon once however often the row is written [design].
fn position_row_icon(ui: &mut Ui, row_tile: TileId, icon_tile: TileId, has_icon: bool) {
    let Some(text_tile) = has_direct_child(ui, row_tile, "ListItemText") else {
        return;
    };
    let root = ui.root_of(row_tile);
    let ysi_rect = has_direct_child(ui, root, YSI_ROOT_TILE);
    let ysi_num = |trait_name: &str| {
        ysi_rect
            .and_then(|r| ui.tile(r).get(trait_name))
            .map(fnv_ui::Value::num)
    };
    let icon_num = |trait_name: &str| ui.tile(icon_tile).get(trait_name).map(fnv_ui::Value::num);

    let blanks_on = ysi_num("_ySIBlanks").is_none_or(|v| v != 0.0);
    let width = if has_icon || blanks_on {
        ysi_num("_ySIWidth")
            .or_else(|| icon_num("width"))
            .unwrap_or(0.0)
            .max(0.0)
    } else {
        0.0
    };
    let space_left = ysi_num("_ySISpaceLeft")
        .or_else(|| icon_num("user0"))
        .unwrap_or(0.0);
    let space_right = ysi_num("_ySISpaceRight")
        .or_else(|| icon_num("user1"))
        .unwrap_or(0.0);

    let (icon_x, text_shift) = if width > 0.0 {
        (space_left, space_left + width + space_right)
    } else {
        (0.0, 0.0)
    };
    let base_x = shift_list_text(ui, text_tile, text_shift);
    ui.set_trait(icon_tile, "x", fnv_ui::Value::Num(base_x + icon_x));
}

/// The operators of a shifted expression-driven `ListItemText/x`: its own expression
/// (kept in `_ysi_base_x`) plus `_ysi_x_shift`.
fn shifted_x_ops() -> Vec<Op> {
    let me = |name: &str| Arg::Ref { src: Src::Me, name: name.to_string() };
    vec![
        Op { kind: OpKind::Copy, arg: me(YSI_BASE_X_TRAIT) },
        Op { kind: OpKind::Add, arg: me(YSI_X_SHIFT_TRAIT) },
    ]
}

/// Moves `ListItemText` (`text_tile`) `shift` units right of its base place, and answers
/// the base place: the file's literal `x` (kept in `_ysi_base_x`), or for an expression the
/// value the file's operators work out. Writing the same shift again changes nothing.
fn shift_list_text(ui: &mut Ui, text_tile: TileId, shift: f64) -> f64 {
    let x_expr = ui
        .tile(text_tile)
        .trait_index("x")
        .and_then(|idx| ui.tile(text_tile).traits[idx].expr.clone());
    match x_expr {
        Some(ops) => {
            if ops != shifted_x_ops() {
                // Seed `_ysi_base_x` with the already-evaluated `x` so `ySIImage/x` lands
                // at the right spot on the first frame (before the next `ui.update()`
                // evaluates `_ysi_base_x`), then move the file's operators there and wrap
                // `x` to add `_ysi_x_shift`.
                let initial = ui.tile(text_tile).get("x").cloned().unwrap_or(fnv_ui::Value::Num(0.0));
                ui.set_trait(text_tile, YSI_BASE_X_TRAIT, initial);
                ui.set_trait_expr(text_tile, YSI_BASE_X_TRAIT, ops);
                ui.set_trait_expr(text_tile, "x", shifted_x_ops());
            }
            ui.set_trait(text_tile, YSI_X_SHIFT_TRAIT, fnv_ui::Value::Num(shift));
            ui.tile(text_tile).get(YSI_BASE_X_TRAIT).map_or(0.0, fnv_ui::Value::num)
        }
        None => {
            let base_x = match ui.tile(text_tile).get(YSI_BASE_X_TRAIT) {
                Some(saved) => saved.num(),
                None => {
                    let initial = ui.tile(text_tile).get("x").map_or(0.0, fnv_ui::Value::num);
                    ui.set_trait(text_tile, YSI_BASE_X_TRAIT, fnv_ui::Value::Num(initial));
                    initial
                }
            };
            ui.set_trait(text_tile, "x", fnv_ui::Value::Num(base_x + shift));
            base_x
        }
    }
}

/// Takes a row's `ySIImage` off (icons switched off, `yUI.ini` `bAddInventoryIcons=0`) and
/// puts its `ListItemText` back at its base place. Nothing for a row without one.
pub fn remove_row_icon(ui: &mut Ui, row_tile: TileId) {
    let Some(icon) = has_direct_child(ui, row_tile, YSI_IMAGE_TILE) else {
        return;
    };
    ui.unload_tile(icon);
    if let Some(text_tile) = has_direct_child(ui, row_tile, "ListItemText") {
        if ui.tile(text_tile).get(YSI_BASE_X_TRAIT).is_some() {
            shift_list_text(ui, text_tile, 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_ui::MemSource;

    const SYNTHETIC_YSI_XML: &str = r#"
<rect name="ySI">
  <_ySI> 1 </_ySI>
  <_ySIBlanks> 1 </_ySIBlanks>
  <_ySIPipBoy> <copy src="parent()" trait="class"/> <eq> &InventoryMenu; </eq> </_ySIPipBoy>
  <_ySIWidth> <copy> 24 </copy> <add> <copy> 8 </copy> <onlyif src="me()" trait="_ySIPipBoy"/> </add> </_ySIWidth>
  <_ySIHeight> 24 </_ySIHeight>
  <_ySISpaceUp> 4 </_ySISpaceUp>
</rect>
<template name="ySIDefault">
  <image name="ySIImage">
    <filename> icons\default_item.dds </filename>
    <width> <copy src="ySI" trait="_ySIWidth"/> </width>
    <height> <copy src="ySI" trait="_ySIHeight"/> </height>
    <y> <copy src="ySI" trait="_ySISpaceUp"/> </y>
    <visible> 1 </visible>
  </image>
</template>
<template name="ySINoTag">
  <image name="ySIImage">
    <filename> icons\untagged_item.dds </filename>
    <width> <copy src="ySI" trait="_ySIWidth"/> <onlyif src="ySI" trait="_ySIBlanks"/> </width>
    <height> <copy src="ySI" trait="_ySIHeight"/> </height>
    <visible> 1 </visible>
  </image>
</template>
"#;

    #[test]
    fn absent_ysi_xml_is_a_no_op() {
        let src = MemSource::default().with("menus/hud.xml", r#"<menu name="HUDMainMenu"><class>&HUDMainMenu;</class></menu>"#);
        let mut ui = Ui::new();
        let root = ui.load_menu(&src, "menus/hud.xml").unwrap();
        assert!(!is_present(&src));
        assert!(!publish_for_menu(&mut ui, &src, root, Some(HUD_MENU_CODE)));
        assert!(ui.find_tile("HUDMainMenu/ySI").is_none());
        assert!(ui.tile(root).templates.is_empty());
    }

    #[test]
    fn injects_ysi_xml_evaluates_traits_and_deduplicates() {
        let src = MemSource::default()
            .with("menus/hud.xml", r#"<menu name="HUDMainMenu"><class>&HUDMainMenu;</class><rect name="Row"/></menu>"#)
            .with("menus/inv.xml", r#"<menu name="InventoryMenu"><class>&InventoryMenu;</class><rect name="Row"/></menu>"#)
            .with("menus/start.xml", r#"<menu name="StartMenu"><class>&StartMenu;</class></menu>"#)
            .with(YSI_XML_PATH, SYNTHETIC_YSI_XML);
        let mut ui = Ui::new();
        let hud = ui.load_menu(&src, "menus/hud.xml").unwrap();
        let inv = ui.load_menu(&src, "menus/inv.xml").unwrap();
        let start = ui.load_menu(&src, "menus/start.xml").unwrap();

        assert!(is_present(&src));
        assert!(publish_for_menu(&mut ui, &src, hud, Some(HUD_MENU_CODE)));
        assert!(publish_for_menu(&mut ui, &src, inv, Some(menus::INVENTORY)));
        // Non-target menus are left untouched.
        assert!(!publish_for_menu(&mut ui, &src, start, Some(menus::START)));
        // Second call deduplicates cleanly without creating a second `ySI` tile.
        assert!(!publish_for_menu(&mut ui, &src, hud, Some(HUD_MENU_CODE)));
        ui.update();

        assert_eq!(ui.get_float("HUDMainMenu/ySI/_ySI"), Some(1.0));
        assert_eq!(ui.get_float("HUDMainMenu/ySI/_ySIPipBoy"), Some(0.0));
        assert_eq!(ui.get_float("HUDMainMenu/ySI/_ySIWidth"), Some(24.0));
        assert_eq!(ui.get_float("InventoryMenu/ySI/_ySIPipBoy"), Some(1.0));
        assert_eq!(ui.get_float("InventoryMenu/ySI/_ySIWidth"), Some(32.0));
        assert!(ui.find_tile("StartMenu/ySI").is_none());

        let ysi_count = ui.tile(hud).children.iter().filter(|&&c| ui.tile(c).name.eq_ignore_ascii_case("ySI")).count();
        assert_eq!(ysi_count, 1);

        // Template instantiation resolves `src="ySI"` expressions and deduplicates.
        let icon = instantiate_icon(&mut ui, &src, "InventoryMenu/Row", YSI_DEFAULT_TEMPLATE, None).unwrap();
        assert_eq!(instantiate_icon(&mut ui, &src, "InventoryMenu/Row", YSI_DEFAULT_TEMPLATE, None), Some(icon));
        ui.update();
        assert_eq!(ui.get_float("InventoryMenu/Row/ySIImage/width"), Some(32.0));
        assert_eq!(ui.get_string("InventoryMenu/Row/ySIImage/filename").as_deref(), Some("icons\\default_item.dds"));
    }

    #[test]
    fn malformed_empty_and_missing_templates_refuse_without_mutating_parent() {
        let src = MemSource::default()
            .with(
                "menus/hud.xml",
                r#"<menu name="HUDMainMenu"><class>&HUDMainMenu;</class>
                     <rect name="Row"><width>100</width></rect>
                     <template name="EmptyTpl"></template>
                     <template name="TraitOnlyTpl"><width>999</width></template>
                   </menu>"#,
            )
            .with(YSI_XML_PATH, "   \r\n  ");
        let mut ui = Ui::new();
        let hud = ui.load_menu(&src, "menus/hud.xml").unwrap();
        // Empty ySI.xml is refused cleanly.
        assert!(!publish_for_menu(&mut ui, &src, hud, Some(HUD_MENU_CODE)));
        assert!(ui.find_tile("HUDMainMenu/ySI").is_none());
        // Missing, blank, empty, and trait-only templates refuse without panicking or mutating parent.
        assert!(instantiate_icon(&mut ui, &src, "HUDMainMenu/Row", "NoSuchTemplate", None).is_none());
        assert!(instantiate_icon(&mut ui, &src, "HUDMainMenu/Row", "", None).is_none());
        assert!(instantiate_icon(&mut ui, &src, "HUDMainMenu/Missing", "EmptyTpl", None).is_none());
        assert!(instantiate_icon(&mut ui, &src, "HUDMainMenu/Row", "EmptyTpl", None).is_none());
        assert!(instantiate_icon(&mut ui, &src, "HUDMainMenu/Row", "TraitOnlyTpl", None).is_none());
        assert_eq!(ui.get_float("HUDMainMenu/Row/width"), Some(100.0));
        assert!(ui.tile(ui.find_tile("HUDMainMenu/Row").unwrap()).children.is_empty());
    }

    #[test]
    fn apply_row_icon_populates_updates_switches_templates_and_stays_noop_without_ysi() {
        let base_xml = r#"<menu name="InventoryMenu"><class>&InventoryMenu;</class><rect name="Row"><text name="ListItemText"><string>Item</string></text></rect></menu>"#;
        // Without ySI.xml: complete no-op even when display data is passed.
        let plain_src = MemSource::default().with("menus/inv.xml", base_xml);
        let mut plain_ui = Ui::new();
        let _ = plain_ui.load_menu(&plain_src, "menus/inv.xml").unwrap();
        let plain_row = plain_ui.find_tile("InventoryMenu/Row").unwrap();
        let tagged = YsiItemDisplay {
            tag: Some("(a01)".into()),
            icon: Some("icons\\pistol.dds".into()),
            template: YSI_DEFAULT_TEMPLATE.into(),
        };
        assert!(!apply_row_icon(&mut plain_ui, &plain_src, plain_row, Some(&tagged)));
        assert!(plain_ui.find_tile("InventoryMenu/Row/ySIImage").is_none());

        // With ySI.xml injected: tagged rows get ySIDefault + filename, updates reuse the tile,
        // unknown custom templates fall back to ySIDefault, and untagged rows switch to ySINoTag.
        let src = MemSource::default()
            .with("menus/inv.xml", base_xml)
            .with(YSI_XML_PATH, SYNTHETIC_YSI_XML);
        let mut ui = Ui::new();
        let inv = ui.load_menu(&src, "menus/inv.xml").unwrap();
        assert!(publish_for_menu(&mut ui, &src, inv, Some(menus::INVENTORY)));
        let row = ui.find_tile("InventoryMenu/Row").unwrap();

        assert!(apply_row_icon(&mut ui, &src, row, Some(&tagged)));
        ui.update();
        let first_id = ui.find_tile("InventoryMenu/Row/ySIImage").unwrap();
        assert_eq!(ui.get_string("InventoryMenu/Row/ySIImage/filename").as_deref(), Some("icons\\pistol.dds"));
        assert_eq!(ui.get_float("InventoryMenu/Row/ySIImage/width"), Some(32.0));

        let updated = YsiItemDisplay {
            tag: Some("(a02)".into()),
            icon: Some("icons\\rifle.dds".into()),
            template: "MissingCustomTemplate".into(),
        };
        assert!(apply_row_icon(&mut ui, &src, row, Some(&updated)));
        ui.update();
        assert_eq!(ui.find_tile("InventoryMenu/Row/ySIImage"), Some(first_id));
        assert_eq!(ui.get_string("InventoryMenu/Row/ySIImage/filename").as_deref(), Some("icons\\rifle.dds"));

        // Switching to untagged swaps cleanly to ySINoTag without duplicating children.
        assert!(apply_row_icon(&mut ui, &src, row, None));
        ui.update();
        let images = ui
            .tile(row)
            .children
            .iter()
            .filter(|&&c| ui.is_live(c) && ui.tile(c).name.eq_ignore_ascii_case(YSI_IMAGE_TILE))
            .count();
        assert_eq!(images, 1);
        assert_eq!(ui.get_string("InventoryMenu/Row/ySIImage/filename").as_deref(), Some("icons\\untagged_item.dds"));
    }

    #[test]
    fn apply_row_icon_positions_icon_and_shifts_literal_list_text_idempotently() {
        let menu_xml = r#"<menu name="InventoryMenu"><class>&InventoryMenu;</class>
          <rect name="RowLiteral"><text name="ListItemText"><x>40</x><string>Item</string></text></rect>
          <rect name="RowExpr"><text name="ListItemText"><x><copy>55</copy></x><string>ExprItem</string></text></rect>
        </menu>"#;
        let ysi_xml = r#"
<rect name="ySI">
  <_ySI> 1 </_ySI>
  <_ySIBlanks> 1 </_ySIBlanks>
  <_ySIWidth> 32 </_ySIWidth>
  <_ySIHeight> 32 </_ySIHeight>
  <_ySISpaceLeft> -5 </_ySISpaceLeft>
  <_ySISpaceRight> 5 </_ySISpaceRight>
</rect>
<template name="ySIDefault">
  <image name="ySIImage">
    <filename> icons\default.dds </filename>
    <width> <copy src="ySI" trait="_ySIWidth"/> </width>
    <height> <copy src="ySI" trait="_ySIHeight"/> </height>
    <user0> <copy src="ySI" trait="_ySISpaceLeft"/> </user0>
    <user1> <copy src="ySI" trait="_ySISpaceRight"/> </user1>
    <x> 0 </x>
    <visible> 1 </visible>
  </image>
</template>
<template name="ySINoTag">
  <image name="ySIImage">
    <filename> icons\blank.dds </filename>
    <width> <copy src="ySI" trait="_ySIWidth"/> <onlyif src="ySI" trait="_ySIBlanks"/> </width>
    <height> <copy src="ySI" trait="_ySIHeight"/> </height>
    <user0> <copy src="ySI" trait="_ySISpaceLeft"/> </user0>
    <user1> <copy src="ySI" trait="_ySISpaceRight"/> </user1>
    <x> 0 </x>
    <visible> 1 </visible>
  </image>
</template>
"#;
        let src = MemSource::default()
            .with("menus/inv.xml", menu_xml)
            .with(YSI_XML_PATH, ysi_xml);
        let mut ui = Ui::new();
        let inv = ui.load_menu(&src, "menus/inv.xml").unwrap();
        assert!(publish_for_menu(&mut ui, &src, inv, Some(menus::INVENTORY)));

        let tagged = YsiItemDisplay {
            tag: Some("(a01)".into()),
            icon: Some("icons\\pistol.dds".into()),
            template: YSI_DEFAULT_TEMPLATE.into(),
        };
        let row_lit = ui.find_tile("InventoryMenu/RowLiteral").unwrap();
        let row_expr = ui.find_tile("InventoryMenu/RowExpr").unwrap();

        // An expression-driven row: the file's operators keep working out the base place
        // (55) and the text moves past the icon: 55 + (-5) + 32 + 5 = 87, the icon at
        // 55 + (-5) = 50. (Before, the text stayed at 55 and the icon drew over it.)
        // Calling apply_row_icon before ui.update() must not freeze the base at the
        // unevaluated 0.0.
        assert_eq!(ui.get_float("InventoryMenu/RowExpr/ListItemText/x"), Some(0.0));
        assert!(apply_row_icon(&mut ui, &src, row_expr, Some(&tagged)));
        ui.update();
        assert_eq!(ui.get_float("InventoryMenu/RowExpr/ListItemText/_ysi_base_x"), Some(55.0));
        assert_eq!(ui.get_float("InventoryMenu/RowExpr/ListItemText/x"), Some(87.0));
        assert!(apply_row_icon(&mut ui, &src, row_expr, Some(&tagged)));
        ui.update();
        assert_eq!(ui.get_float("InventoryMenu/RowExpr/ySIImage/x"), Some(50.0));
        assert_eq!(ui.get_float("InventoryMenu/RowExpr/ListItemText/x"), Some(87.0));

        // Repeated calls are idempotent: icon x = 40 + (-5) = 35, text x = 40 - 5 + 32 + 5 = 72.
        assert!(apply_row_icon(&mut ui, &src, row_lit, Some(&tagged)));
        assert!(apply_row_icon(&mut ui, &src, row_lit, Some(&tagged)));
        ui.update();
        assert_eq!(ui.get_float("InventoryMenu/RowLiteral/ySIImage/x"), Some(35.0));
        assert_eq!(ui.get_float("InventoryMenu/RowLiteral/ListItemText/x"), Some(72.0));

        // Disabling _ySIBlanks restores literal ListItemText/x to its original 40 on untagged rows.
        ui.set_float("InventoryMenu/ySI/_ySIBlanks", 0.0);
        ui.update();
        assert!(apply_row_icon(&mut ui, &src, row_lit, None));
        ui.update();
        assert_eq!(ui.get_float("InventoryMenu/RowLiteral/ListItemText/x"), Some(40.0));
    }
}
