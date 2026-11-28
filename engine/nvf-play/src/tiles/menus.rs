//! T15a: the message box and the "How many?" prompt drawn from their files, behind
//! `[UI] TileMenus=MessageMenu,QuantityMenu` (the prompt also over a container menu drawn from
//! its tiles, `[UI] TileMenus=ContainerMenu` or `BarterMenu`, [`quantity_tiled`]) [design: ui-tile-tree.md
//! §6.2 step 3; plan: docs/handoff/ui-tile-tree-tasks.md, T15a].
//!
//! - **The switch** (`super::vanilla::TileMenus`): with a menu on and the game's menu files
//!   read, its hand-built drawing is not spawned ([`message_tiled`], [`quantity_tiled`];
//!   `crate::message_menu`, `crate::menu::look::spawn_quantity`) and `super::draw` draws
//!   the menu's whole tree. Off (the default), none of this runs.
//! - **What the model writes** (before scripts, only when what the menu shows changed):
//!   [`write_message`] the box's strings, its icon, one `MM_ButtonTemplate` row per button
//!   and the list's and the box's sizes the file leaves "set in code"; [`write_quantity`]
//!   the meter's stack and amount, from which the file works out the fill and the amount
//!   text. The rules the box follows (what it holds, which button is chosen, the keys) stay
//!   the hand-built menus' (`crate::message_menu::MessagePanel`, `crate::script_hud`,
//!   `crate::menu::model::Quantity`) [design].
//! - **Text sizes**: every text tile of a drawn menu gets its measured `width` and `height`
//!   ([`write_text_sizes`]), as the HUD's measured texts get their width (hud.md §14.1):
//!   the files place the buttons and the list by them [hypothesis: the original's code
//!   writes them; menu-xml.md §3.1.2].
//! - None of the hand-built drawings' copied numbers (`message_menu.rs`'s layout constants,
//!   `look.rs`'s `Q_*`) is read here: the places come from the files. The few numbers this
//!   module keeps are each tagged where they stand.

use crate::message_menu::PanelView;
use fnv_ui::{Origin, TileId, Ui, Value};
use std::collections::HashMap;

/// The menus' names [documented: message_menu.xml, quantity_menu.xml].
pub const MESSAGE_MENU: &str = "MessageMenu";
pub const QUANTITY_MENU: &str = "QuantityMenu";

/// The tiles the model writes, under their menus [documented: the menu files].
const MAIN: &str = "MessageMenu/NOGLOW_BRANCH/MM_MainRect";
const LIST: &str = "MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_ButtonList";
const ROW_TEMPLATE: &str = "MM_ButtonTemplate";
const ROW: &str = "MM_Button";
const METER: &str = "QuantityMenu/NOGLOW_BRANCH/QM_MainRect/QM_AmountMeter";

/// [design] A whole menu draws from tiles or falls back with one diagnostic reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tiled {
    Yes,
    No(String),
}

impl Tiled {
    pub fn yes(&self) -> bool {
        matches!(self, Self::Yes)
    }
}

/// [design] Every fixed path the message model writes is part of a successful build.
/// Look for the template along the same ancestors as `Ui::add_code_tile_from_template`.
pub fn message_built(ui: &Ui) -> Tiled {
    for path in [
        MESSAGE_MENU,
        MAIN,
        LIST,
        "MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_Title",
        "MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_MessageText",
        "MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_MessageIcon",
        "MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_ButtonList/lb_scrollbar",
        "MessageMenu/NOGLOW_BRANCH/MM_Background",
    ] {
        if ui.find_tile(path).is_none() {
            return Tiled::No(format!("{path} missing"));
        }
    }
    let mut at = ui.find_tile(LIST);
    while let Some(t) = at {
        if let Some(tp) = ui.tile(t).templates.iter().find(|tp| tp.name.eq_ignore_ascii_case(ROW_TEMPLATE)) {
            let row = tp.body.iter().find_map(|n| match n {
                fnv_ui::xml::Node::Element(e) if e.attr("name").is_some_and(|n| n.eq_ignore_ascii_case(ROW)) => Some(e),
                _ => None,
            });
            let Some(row) = row else { return Tiled::No("MM_ButtonTemplate/MM_Button missing".into()) };
            if !row.elements().any(|e| e.is("text") && e.attr("name").is_some_and(|n| n.eq_ignore_ascii_case("ListItemText"))) {
                return Tiled::No("MM_ButtonTemplate/MM_Button/ListItemText missing".into());
            }
            return Tiled::Yes;
        }
        at = ui.tile(t).parent;
    }
    Tiled::No(format!("{ROW_TEMPLATE} missing"))
}

/// [design] Every fixed path the quantity model writes is part of a successful build.
pub fn quantity_built(ui: &Ui) -> Tiled {
    for path in [QUANTITY_MENU, METER, "QuantityMenu/NOGLOW_BRANCH/QM_Background"] {
        if ui.find_tile(path).is_none() {
            return Tiled::No(format!("{path} missing"));
        }
    }
    Tiled::Yes
}

/// [design] The model has measured these rows before input and drawing. Until scrolling
/// is built, a list taller than `LIST_MOST` keeps the whole hand-built box. Scrolling is
/// a follow-up (docs/handoff/ui-tile-tree-tasks.md, T15a).
pub fn message_status(ui: &Ui) -> Tiled {
    let built = message_built(ui);
    if !built.yes() {
        return built;
    }
    let rows = rows(ui);
    if rows.iter().any(|&r| child(ui, r, "ListItemText").is_none()) {
        return Tiled::No("MM_ButtonTemplate/MM_Button/ListItemText missing".into());
    }
    if rows.iter().map(|&r| num(ui, r, "height")).sum::<f64>() > LIST_MOST {
        return Tiled::No("button rows exceed LIST_MOST (420); scrolling is not built".into());
    }
    Tiled::Yes
}

/// How far the message box's button list grows before it would scroll [documented:
/// message_menu.xml, the list's `height` "set by code" under a `min` of 420; the list's
/// scrolling is not built, as in the hand-built box, message-menu.md §4].
pub const LIST_MOST: f64 = 420.0;

/// The backgrounds' opacity, which both files leave "set in code" (their `alpha` copies
/// `globals()`'s `_background_fill_alpha`, 32): the hand-built box's (`message_menu::SHADE`)
/// [hypothesis: on the owner's screenshot the room behind the box can hardly be made out,
/// message-menu.md §3; wait-menu.md §2].
pub fn background_alpha() -> f64 {
    (f64::from(crate::message_menu::SHADE) * 255.0).round()
}

/// A text's width and height in interface units: font number, string, wrap width.
pub type Size<'a> = dyn Fn(f64, &str, Option<f64>) -> (f64, f64) + 'a;

/// Which menus `super::draw` drew from their tiles this frame, for the blur of the world
/// behind them (nvforever's `imagespace::tile_menu_blur`) [design]. Empty by default: with
/// the menu switches off it stays empty and nothing reads differently. Written whole every
/// drawing frame from the decision the drawing itself made, so it cannot outlive a closed
/// menu or a fallback to the hand-built drawing.
#[derive(bevy::prelude::Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileMenuBlur {
    /// `TutorialMenu` is drawn from its tiles.
    pub tutorial: bool,
    /// `ContainerMenu` is drawn from its tiles.
    pub container: bool,
    /// `BarterMenu` is drawn from its tiles [design: barter-menu.md section 9].
    pub barter: bool,
    /// `RecipeMenu` is drawn from its tiles [design: crafting.md section 4].
    pub recipe: bool,
    /// `TraitMenu` is drawn from its tiles.
    pub traits: bool,
    /// `RepairServicesMenu` is drawn from its tiles [design: repair.md §3.1].
    pub repair_services: bool,
    /// [design: chargen.md §3.1] CharGenMenu is drawn from its tiles.
    pub tags: bool,
}

impl TileMenuBlur {
    /// Any menu drawn from its tiles wants the world blurred.
    pub fn any(self) -> bool {
        if self.repair_services {
            return true;
        }
        self.tutorial || self.container || self.barter || self.recipe || self.traits || self.tags
    }
}

/// Whether the open message box is drawn from `MessageMenu`'s tiles: the switch on, the
/// menu files read, a message box the open menu and its file built into the tree (the
/// panels other script menus show in the same box, and a box whose file is missing or did
/// not build every model path, or whose measured rows exceed `LIST_MOST`, keep the
/// hand-built one). Every drawing and input reader uses this same decision [design].
pub fn message_tiled(on: bool, game: &fnv_game::Game<'_>) -> bool {
    on && game.ui_files.is_some() && game.menu == Some(fnv_game::game::menus::MESSAGE) && message_status(&game.ui_tree).yes()
}

/// Whether the "How many?" prompt is drawn from `QuantityMenu`'s tiles: its switch,
/// or the container or barter menu or the ITEMS page beneath it actually drawing from its tiles, with
/// files read and the prompt's required paths built [design: barter-menu.md sections 7-9].
/// A missing or short prompt file, and the frame before its tree builds, keep the
/// hand-built prompt. The crafting menu's prompt stays hand-built.
pub fn quantity_tiled(on: bool, game: &fnv_game::Game<'_>, open: bool, item_tiled: bool) -> bool {
    prompt_tiled(on, item_tiled, open, game.ui_files.is_some(), &game.ui_tree)
}

/// [`quantity_tiled`]'s decision, `files` being whether the menu files were read.
fn prompt_tiled(on: bool, item_tiled: bool, open: bool, files: bool, ui: &Ui) -> bool {
    (on || item_tiled) && open && files && quantity_built(ui).yes()
}

/// What a prompt or box puts under the pointer: nothing (not shown), its hand-built
/// drawing (whose own sheet and buttons take the pointer), or, drawn from tiles, a
/// [`modal_sheet`] alone (the tile renderer's nodes take no pointer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modal {
    None,
    HandBuilt,
    Sheet,
}

/// [`Modal`] for a prompt or box `shown`, drawn from its tiles when `tiled`.
pub fn modal(shown: bool, tiled: bool) -> Modal {
    match (shown, tiled) {
        (false, _) => Modal::None,
        (true, false) => Modal::HandBuilt,
        (true, true) => Modal::Sheet,
    }
}

/// Marks [`modal_sheet`]'s node.
#[derive(bevy::prelude::Component)]
pub struct ModalSheet;

/// A node over the whole view that takes the pointer while a prompt or box drawn from its
/// tiles is up, so nothing under it is clicked (the container menu's Exit, the dialogue's
/// topics): both files say no click goes past them [documented: message_menu.xml and
/// quantity_menu.xml, `stackingtype` `&no_click_past;`], as the hand-built prompt's own
/// sheet does (`crate::menu::look::spawn_quantity`). The tile pointer
/// (`super::input`) reads the cursor itself, so the sheet does not stop it.
pub fn modal_sheet() -> impl bevy::prelude::Bundle {
    use bevy::prelude::{Node, PositionType, Val};
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..Default::default() };
    (ModalSheet, full, bevy::ui::FocusPolicy::Block, crate::ui_pointer::PointerState::default())
}

/// The keys a menu's hand-built handler acts on itself, by the names the tile input gives
/// them (`super::input`): the prompt's A, E, Left, Right (`crate::menu`'s prompt keys) and
/// the controller buttons that stand in for its Enter, Esc, Page Up and Page Down
/// (`crate::pad::menu_key`); the box's Up, Down and the controller's A for its Enter
/// (`crate::script_hud::menu_keys`); the Wait, tutorial and container menus' their own lists.
/// Any other key a file binds is left to the file.
pub fn hand_built_takes(menu: &str, key: &str) -> bool {
    let keys: &[&str] = if menu.eq_ignore_ascii_case(QUANTITY_MENU) {
        &["A", "E", "xleft", "xright", "xbuttona", "xbuttonb", "xbuttonlb", "xbuttonrb"]
    } else if menu.eq_ignore_ascii_case(MESSAGE_MENU) {
        &["xup", "xdown", "xbuttona"]
    } else if menu.eq_ignore_ascii_case(super::wait_menu::SLEEP_WAIT_MENU) {
        &super::wait_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::tutorial_menu::TUTORIAL_MENU) {
        &super::tutorial_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::stats_menu::STATS_MENU) {
        &super::stats_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::inventory_menu::INVENTORY_MENU) {
        &super::inventory_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::map_menu::MAP_MENU) {
        &super::map_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::container_menu::CONTAINER_MENU) {
        &super::container_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::barter_menu::BARTER_MENU) {
        &super::barter_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::recipe_menu::RECIPE_MENU) {
        &super::recipe_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::trait_menu::TRAIT_MENU) || menu.eq_ignore_ascii_case(super::tag_menu::TAG_MENU) {
        &super::trait_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::dialog_menu::DIALOG_MENU) {
        &super::dialog_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::repair_services_menu::REPAIR_SERVICES_MENU) {
        &super::repair_services_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::repair_menu::REPAIR_MENU) {
        &super::repair_menu::HAND_BUILT_KEYS
    } else if menu.eq_ignore_ascii_case(super::item_mod_menu::ITEM_MOD_MENU) {
        &super::item_mod_menu::HAND_BUILT_KEYS
    } else {
        &[]
    };
    keys.iter().any(|k| k.eq_ignore_ascii_case(key))
}

/// Whether a click a key made on `menu`'s tiles acts: unless the key came from the keyboard
/// (or a controller button standing in for a key) this frame, `physical`, and the menu's
/// hand-built handler took it then ([`hand_built_takes`]), so one press acts once. A
/// script's MenuTapKey and the files' bindings the hand-built handler lacks act [design].
pub fn key_click_acts(menu: &str, key: &str, physical: &[&str]) -> bool {
    !(physical.iter().any(|p| p.eq_ignore_ascii_case(key)) && hand_built_takes(menu, key))
}

/// Whether the hand-built box moved its choice for a key this frame (Up, Down): the
/// highlight a file's `mouseover` ref moved for the same key is then not taken as the
/// choice, so a press moves it once ([`chosen_row`]) [design].
pub fn nav_by_hand(physical: &[&str]) -> bool {
    physical.iter().any(|k| ["xup", "xdown"].iter().any(|n| n.eq_ignore_ascii_case(k)) && hand_built_takes(MESSAGE_MENU, k))
}

/// The box's row the frame's highlight chooses: the row the highlight moved onto, unless
/// the hand-built handler moved the choice for a key this frame (`by_hand`,
/// [`nav_by_hand`]): the pointer chooses, the hand-built keys step.
pub fn chosen_row(ui: &Ui, report: &fnv_ui::input::Report, by_hand: bool) -> Option<usize> {
    match report.over {
        Some((_, Some(to))) if !by_hand && ui.is_live(to) => row_button(ui, to),
        _ => None,
    }
}

/// The box's button a click picks: a live row's (a row of a box since replaced was
/// unloaded, `write_message`'s `fresh`), by the pointer, a script, or a key the
/// hand-built handler did not take ([`key_click_acts`]).
pub fn message_pick(ui: &Ui, click: &fnv_ui::input::Click, physical: &[&str]) -> Option<usize> {
    if !ui.is_live(click.tile) {
        return None;
    }
    if let fnv_ui::input::By::Key(k) = &click.by {
        if !key_click_acts(MESSAGE_MENU, k, physical) {
            return None;
        }
    }
    row_button(ui, click.tile)
}

fn set(ui: &mut Ui, tile: TileId, name: &str, v: Value) {
    ui.set_trait(tile, name, v);
}

fn num(ui: &Ui, tile: TileId, name: &str) -> f64 {
    ui.tile(tile).get(name).map_or(0.0, Value::num)
}

fn child(ui: &Ui, tile: TileId, name: &str) -> Option<TileId> {
    ui.tile(tile).children.iter().copied().find(|&c| ui.is_live(c) && ui.tile(c).name.eq_ignore_ascii_case(name))
}

/// The button rows the code made under the list, in the order made (the first button
/// first).
pub fn rows(ui: &Ui) -> Vec<TileId> {
    let Some(list) = ui.find_tile(LIST) else { return Vec::new() };
    ui.tile(list).children.iter().copied().filter(|&c| ui.is_live(c) && ui.tile(c).origin == Origin::Code && ui.tile(c).name.eq_ignore_ascii_case(ROW)).collect()
}

/// The button a row stands for: its `listindex`, when `tile` is a row or in one.
pub fn row_button(ui: &Ui, tile: TileId) -> Option<usize> {
    let rows = rows(ui);
    let mut at = Some(tile);
    while let Some(t) = at {
        if rows.contains(&t) {
            let i = num(ui, t, "listindex");
            return (i >= 0.0).then_some(i as usize);
        }
        at = ui.tile(t).parent;
    }
    None
}

/// Write what `MessageMenu` shows of `view` with button `selected` chosen (none: no row
/// framed, `_highlight_y` -1 and `_selected_height` 0, as the original's box at open
/// [observed: hud_menus capture 2026-10-31]) [design: the
/// traits the file says the code sets; its own expressions place the rest]:
/// - `MM_Title`'s `string`, and its `visible` 0 without a title [hypothesis: the file
///   starts the top line's right piece at 0 and hides its left piece by the title's
///   `visible`; a box without a title has a whole top line, message-menu.md §2-§3];
/// - `MM_MessageText`'s `string`; `MM_MessageIcon`'s `filename` and `visible` for a box
///   with a picture (the file makes room for it while it is visible) [documented];
/// - one row from `MM_ButtonTemplate` per button under `MM_ButtonList`, made or unloaded
///   to match, each with its `string`, `listindex` (the file shows a row with an index of 0
///   or more), `height` (its text's and the template's `_VerticalSpacing`, which the
///   template's comment says is added when `height` is 0) and `_y` (the rows one under the
///   other) [documented: list_box_template.xml's comments; observed: the button's row is
///   its text and 20, message-menu.md §3];
/// - the list's `height` (the rows', at most [`LIST_MOST`]), its `width` (the widest
///   button's text and the menu's two `_horbuf`, at most the text's room) [hypothesis:
///   fitted to the screenshot: the frame, 72 outside around "OK" 29 wide, is the list less
///   37 by list_box.xml's highlight box], `_highlight_y` and `_selected_height` (the chosen
///   row's), the scroll bar's `_number_of_items` and `_number_of_visible_items` (the rows'
///   height and the list's in `_scroll_delta` steps, so the bar shows only when the rows
///   overflow) [documented: list_box.xml's comments; design: unrounded];
/// - `MM_MainRect`'s `width` ("set in code"): the widest of the text's place, widest line
///   and `_horbuf` (the file's place moves the text right for the icon), the title's room, and the list's width and two
///   `_horbuf`, within `_MinMenuWidth` and `_MaxMenuWidth` [hypothesis: the file's
///   comments; design: the title's and the list's room, as the hand-built box makes room
///   for its title and its frame];
/// - the background's `alpha` ([`background_alpha`]).
///
/// `fresh`: another box than the last written ([`another_box`]: a new or replaced
/// message, one alike in every word included): its rows are made again, so a click still
/// queued on the last box's rows (a script's ClickMenuButton, reported at the next input
/// step) finds its tile gone and answers nothing ([`message_pick`]) [design].
///
/// `size` measures a text. The tree is worked out once in between, so the rows' texts
/// read their strings before they are measured. A total row height above `LIST_MOST`
/// makes [`message_status`] reject tiling for every input/drawing reader; list scrolling
/// is a follow-up [design: docs/handoff/ui-tile-tree-tasks.md, T15a].
pub fn write_message(ui: &mut Ui, view: &PanelView, selected: Option<usize>, fresh: bool, size: &Size<'_>) {
    if !message_built(ui).yes() {
        return;
    }
    let Some(menu) = ui.menu(MESSAGE_MENU) else { return };
    let (Some(main), Some(list)) = (ui.find_tile(MAIN), ui.find_tile(LIST)) else { return };
    let path = |name: &str| format!("{MAIN}/{name}");
    let title = view.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or_default().to_string();
    if let Some(t) = ui.find_tile(&path("MM_Title")) {
        set(ui, t, "visible", Value::Num(f64::from(!title.is_empty())));
        set(ui, t, "string", Value::Str(title.clone()));
    }
    if let Some(t) = ui.find_tile(&path("MM_MessageText")) {
        set(ui, t, "string", Value::Str(view.text.trim_end().to_string()));
    }
    if let Some(t) = ui.find_tile(&path("MM_MessageIcon")) {
        set(ui, t, "visible", Value::Num(f64::from(view.icon.is_some())));
        set(ui, t, "filename", Value::Str(view.icon.clone().unwrap_or_default()));
    }
    // The rows: as many as the buttons, the made ones kept for the same box.
    let mut made = rows(ui);
    if fresh {
        for t in made.drain(..) {
            ui.unload_tile(t);
        }
    }
    while made.len() > view.buttons.len() {
        if let Some(t) = made.pop() {
            ui.unload_tile(t);
        }
    }
    while made.len() < view.buttons.len() {
        // The template's includes were spliced in when the file was read (menu-xml.md
        // §2.2), so no files are needed to make a row.
        match ui.add_code_tile_from_template(&fnv_ui::MemSource::default(), LIST, ROW_TEMPLATE, None) {
            Some(t) => made.push(t),
            None => break,
        }
    }
    for (i, (&row, label)) in made.iter().zip(&view.buttons).enumerate() {
        set(ui, row, "string", Value::Str(label.trim().to_string()));
        set(ui, row, "listindex", Value::Num(i as f64));
    }
    ui.update();
    // The rows' heights from their texts, one under the other.
    let mut y = 0.0;
    let mut widest: f64 = 0.0;
    let mut chosen = None;
    for (i, &row) in made.iter().enumerate() {
        let text = child(ui, row, "ListItemText");
        let (w, h) = text.map_or((0.0, 0.0), |t| measure_tile(ui, t, size));
        widest = widest.max(w);
        let height = h + num(ui, row, "_VerticalSpacing");
        set(ui, row, "height", Value::Num(height));
        set(ui, row, "_y", Value::Num(y));
        if Some(i) == selected {
            chosen = Some((y, height));
        }
        y += height;
    }
    let total = y;
    let horbuf = num(ui, menu, "_horbuf");
    let room = num(ui, menu, "_MaxMenuWidth") - 2.0 * horbuf;
    let list_width = (widest + 2.0 * horbuf).min(room);
    let list_height = total.min(LIST_MOST);
    set(ui, list, "width", Value::Num(list_width));
    set(ui, list, "height", Value::Num(list_height));
    let (hy, hh) = chosen.unwrap_or((-1.0, 0.0));
    set(ui, list, "_highlight_y", Value::Num(hy));
    set(ui, list, "_selected_height", Value::Num(hh));
    if let Some(bar) = child(ui, list, "lb_scrollbar") {
        let delta = num(ui, list, "_scroll_delta").max(1.0);
        set(ui, bar, "_number_of_items", Value::Num(total / delta));
        set(ui, bar, "_number_of_visible_items", Value::Num(list_height / delta));
    }
    // The box's width from its texts.
    // The text's place (the file moves it right for the icon), its widest line and the
    // buffer on its right.
    let text_need = match ui.find_tile(&path("MM_MessageText")) {
        Some(t) if !view.text.trim().is_empty() => num(ui, t, "x") + measure_tile(ui, t, size).0 + horbuf,
        _ => 0.0,
    };
    let title_need = match ui.find_tile(&path("MM_Title")) {
        Some(t) if !title.is_empty() => {
            let x = num(ui, t, "x");
            // The title's left room again on its right, past the line's gap [design].
            x + measure_tile(ui, t, size).0 + TITLE_GAP + x
        }
        _ => 0.0,
    };
    let least = num(ui, menu, "_MinMenuWidth");
    let most = num(ui, menu, "_MaxMenuWidth").max(least);
    let width = [text_need, title_need, list_width + 2.0 * horbuf].into_iter().fold(least, f64::max).min(most);
    set(ui, main, "width", Value::Num(width));
    if let Some(bg) = ui.find_tile("MessageMenu/NOGLOW_BRANCH/MM_Background") {
        set(ui, bg, "alpha", Value::Num(background_alpha()));
    }
}

/// The box a [`write_message`] was for: the menu's tile (a new one when the menu was built
/// again), the box's showing (`fnv_game::Game::front_box`'s serial, counted up for every
/// box shown; `None` for a panel with no box queued, the perks' choice list) and what it
/// shows.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxShown {
    pub menu: TileId,
    pub showing: Option<u64>,
    pub view: PanelView,
}

/// Whether `now` is another box than `was` (the rows are made again, `write_message`'s
/// `fresh`): another menu tile, another showing (a box replaced by one alike in every word
/// and button is another showing) or other words or buttons.
pub fn another_box(was: Option<&BoxShown>, now: &BoxShown) -> bool {
    was != Some(now)
}

/// The gap between the title and the top line's right piece [documented: message_menu.xml,
/// `MM_headline_h2` starts 10 past the title].
const TITLE_GAP: f64 = 10.0;

/// A text tile's size by its string, font and wrap width as the tree holds them.
fn measure_tile(ui: &Ui, t: TileId, size: &Size<'_>) -> (f64, f64) {
    let tile = ui.tile(t);
    let string = tile.get("string").map(Value::text).unwrap_or_default();
    if string.is_empty() {
        return (0.0, 0.0);
    }
    let wrap = tile.get("wrapwidth").map(Value::num).filter(|w| *w > 0.0);
    size(tile.get("font").map_or(0.0, Value::num), &string, wrap)
}

/// What the prompt shows: the amount chosen of the stack (`crate::menu::model::Quantity`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Amount {
    pub amount: i32,
    pub most: i32,
}

/// The step sizes the file leaves to the code (`user3`, `user4`): NVForever's arrows and
/// Page Up/Down [design: `crate::menu::model::QUANTITY_JUMP`; the original's are not read].
const STEPS: (f64, f64) = (1.0, crate::menu::model::QUANTITY_JUMP as f64);

/// Write what `QuantityMenu` shows [documented: quantity_menu.xml's comments on the
/// meter's traits]: `user0` the stack ("Max amount"), `user2` the amount ("Code value …
/// set in code"), `user3` and `user4` the step sizes ([`STEPS`]); the file works the
/// meter's `_Value` (the amount over the stack), its tiled fill and the amount's text out
/// of them. The background's `alpha` ([`background_alpha`]).
pub fn write_quantity(ui: &mut Ui, q: Amount) {
    if !quantity_built(ui).yes() {
        return;
    }
    let Some(meter) = ui.find_tile(METER) else { return };
    set(ui, meter, "user0", Value::Num(f64::from(q.most.max(1))));
    set(ui, meter, "user2", Value::Num(f64::from(q.amount)));
    set(ui, meter, "user3", Value::Num(STEPS.0));
    set(ui, meter, "user4", Value::Num(STEPS.1));
    if let Some(bg) = ui.find_tile("QuantityMenu/NOGLOW_BRANCH/QM_Background") {
        set(ui, bg, "alpha", Value::Num(background_alpha()));
    }
}

/// What the prompt's tiles stand for, by name [documented: quantity_menu.xml].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuantityTile {
    Ok,
    Cancel,
    Decrease,
    Increase,
    /// The meter or a piece of it.
    Meter,
}

/// The prompt's tile `tile` stands for, by its name or a parent's.
pub fn quantity_tile(ui: &Ui, tile: TileId) -> Option<QuantityTile> {
    let mut at = Some(tile);
    while let Some(t) = at {
        let name = ui.tile(t).name.to_ascii_lowercase();
        match name.as_str() {
            "qm_okbutton" => return Some(QuantityTile::Ok),
            "qm_cancelbutton" => return Some(QuantityTile::Cancel),
            "qm_decreasearrow" => return Some(QuantityTile::Decrease),
            "qm_increasearrow" => return Some(QuantityTile::Increase),
            "qm_amountmeter" => return Some(QuantityTile::Meter),
            _ => {}
        }
        at = ui.tile(t).parent;
    }
    None
}

/// The meter's box in interface units, for a press or a drag on it.
pub fn meter_box(ui: &Ui) -> Option<super::layout::Rect> {
    let meter = ui.find_tile(METER)?;
    super::layout::layout(ui, meter, &|_, _, _| (0.0, 0.0), true).into_iter().find(|i| i.tile == meter).map(|i| i.rect)
}

/// Whether the tiled `QuantityMenu` is built and currently visible.
pub(crate) fn normal_touch_quantity_shown(ui: &Ui) -> bool {
    quantity_built(ui).yes() && ui.menu(QUANTITY_MENU).is_some_and(|menu| fnv_ui::input::shown(ui, menu))
}

/// [design: docs/design/android.md] Touch hit-testing for the tiled `QuantityMenu`,
/// independent of whether the menu beneath it (Barter, Container, Inventory) is tiled
/// or hand-built. Exact tile hits return `Source::Tile` (routing through the native
/// tile pointer), while padded arrow/button/meter targets and backdrop taps outside
/// `QM_MainRect` return `Source::Direct` so touch never soft-locks the prompt.
pub(crate) fn normal_touch_quantity_hit(
    ui: &Ui,
    point: bevy::prelude::Vec2,
    window: bevy::prelude::Vec2,
) -> Option<crate::menu::normal_touch::Hit> {
    use crate::menu::{model::Command, normal_touch::{Hit, Source, Target}};
    use fnv_ui::input::{hit_candidates, no_measure, top_hit};
    let menu = ui.menu(QUANTITY_MENU)?;
    if !fnv_ui::input::shown(ui, menu) {
        return None;
    }
    let at = super::input::to_menu_units(point, window, bevy::prelude::Vec2::splat(0.5));
    let candidates = hit_candidates(ui, menu, &no_measure);
    if let Some(picked) = top_hit(ui, &candidates, at) {
        if let Some(kind) = quantity_tile(ui, picked.tile) {
            let target = match kind {
                QuantityTile::Ok => Target::Command(Command::AmountOk),
                QuantityTile::Cancel => Target::Command(Command::AmountCancel),
                QuantityTile::Decrease => Target::Command(Command::AmountStep(-1)),
                QuantityTile::Increase => Target::Command(Command::AmountStep(1)),
                QuantityTile::Meter => {
                    let b = meter_box(ui)?;
                    if !b.width.is_finite() || b.width <= 0.0 {
                        return None;
                    }
                    Target::Meter(((at.0 - b.left) / b.width).clamp(0.0, 1.0) as f32)
                }
            };
            return Some(Hit { source: Source::Tile(menu, picked.tile), target, pane: None, row_step: 0.0 });
        }
    }
    // Padded finger targets for the prompt's arrows, buttons, and meter.
    const PAD: f64 = 16.0;
    for c in candidates.iter().rev() {
        let Some(kind) = quantity_tile(ui, c.under) else { continue };
        let r = c.region();
        if r.width <= 0.0 || r.height <= 0.0 {
            continue;
        }
        let (px, py) = match kind {
            QuantityTile::Meter => (4.0, 12.0),
            _ => (PAD, PAD),
        };
        if at.0 >= r.left - px && at.0 < r.left + r.width + px && at.1 >= r.top - py && at.1 < r.top + r.height + py {
            let target = match kind {
                QuantityTile::Ok => Target::Command(Command::AmountOk),
                QuantityTile::Cancel => Target::Command(Command::AmountCancel),
                QuantityTile::Decrease => Target::Command(Command::AmountStep(-1)),
                QuantityTile::Increase => Target::Command(Command::AmountStep(1)),
                QuantityTile::Meter => {
                    let b = meter_box(ui)?;
                    if !b.width.is_finite() || b.width <= 0.0 {
                        return None;
                    }
                    Target::Meter(((at.0 - b.left) / b.width).clamp(0.0, 1.0) as f32)
                }
            };
            return Some(Hit { source: Source::Direct(menu, c.under), target, pane: None, row_step: 0.0 });
        }
    }
    // Tapping outside the modal dialog card cancels the prompt cleanly.
    if let Some(main) = ui.find_tile("QuantityMenu/NOGLOW_BRANCH/QM_MainRect") {
        let inside = super::layout::layout(ui, main, &|_, _, _| (0.0, 0.0), true)
            .into_iter()
            .find(|i| i.tile == main)
            .is_some_and(|i| {
                let r = i.rect;
                at.0 >= r.left && at.0 < r.left + r.width && at.1 >= r.top && at.1 < r.top + r.height
            });
        if !inside {
            return Some(Hit {
                source: Source::Direct(menu, menu),
                target: Target::Command(Command::AmountCancel),
                pane: None,
                row_step: 0.0,
            });
        }
    }
    None
}

/// What a text tile's size was last measured from, and the size written.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    string: String,
    font: f64,
    wrap: Option<f64>,
    size: (f64, f64),
}

pub type SizesWritten = HashMap<TileId, Measured>;

/// The live text tiles under `menu`.
pub fn texts(ui: &Ui, menu: TileId) -> Vec<TileId> {
    let mut out = Vec::new();
    let mut stack = vec![menu];
    while let Some(t) = stack.pop() {
        let tile = ui.tile(t);
        if !tile.live {
            continue;
        }
        if tile.kind == "text" {
            out.push(t);
        }
        stack.extend(tile.children.iter().copied());
    }
    out
}

/// The fonts of the text tiles under `menu`, each once (the renderer reads them before
/// measuring, `super::draw::FontKit::menu_measure`).
pub fn fonts(ui: &Ui, menu: TileId) -> Vec<f64> {
    fonts_of(ui, &texts(ui, menu))
}

/// The fonts of `texts` ([`texts`]), each once.
pub fn fonts_of(ui: &Ui, texts: &[TileId]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for &t in texts {
        let f = ui.tile(t).get("font").map_or(0.0, Value::num);
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// Write each text tile's `width` and `height` under `menu` from its string, font and
/// wrap width, a wrapped text as wide as its widest line [hypothesis: as the HUD's text
/// widths, hud.md §14.1; the original's code is taken to write the heights too, which the
/// message box's list and text places read]. An axis the file (or a mod) works out with an
/// expression is left to it and the other axis still written; a tile measured from the
/// same string, font and wrap is not measured again (`last`, as
/// `super::notices::write_widths`). Every measure is kept, 0 x 0 too: `size` from
/// `super::draw::FontKit::menu_measure` has a face, the game's or a stand-in, for each font it
/// was given, so each caller gets it with the fonts as the tree stands just before measuring
/// [design]. Returns how many tiles were written.
pub fn write_text_sizes(ui: &mut Ui, menu: TileId, last: &mut SizesWritten, size: &Size<'_>) -> usize {
    let tiles = texts(ui, menu);
    write_text_sizes_of(ui, &tiles, last, size)
}

/// [`write_text_sizes`] of the text tiles `tiles` ([`texts`] of a menu, walked once a frame
/// for the fonts and the sizes both).
pub fn write_text_sizes_of(ui: &mut Ui, tiles: &[TileId], last: &mut SizesWritten, size: &Size<'_>) -> usize {
    last.retain(|t, _| tiles.contains(t));
    let title = ui.find_tile(&format!("{MAIN}/MM_Title"));
    let mut written = 0;
    for &t in tiles {
        let tile = ui.tile(t);
        let computed = |name: &str| tile.trait_index(name).is_some_and(|k| tile.traits[k].expr.is_some());
        let (own_width, own_height) = (!computed("width"), !computed("height"));
        if !own_width && !own_height {
            continue;
        }
        let string = tile.get("string").map(Value::text).unwrap_or_default();
        let font = tile.get("font").map_or(0.0, Value::num);
        let wrap = tile.get("wrapwidth").map(Value::num).filter(|w| *w > 0.0);
        let now = (tile.get("width").map(Value::num), tile.get("height").map(Value::num));
        if let Some(m) = last.get(&t) {
            let kept = (!own_width || now.0 == Some(m.size.0)) && (!own_height || now.1 == Some(m.size.1));
            if m.string == string && m.font == font && m.wrap == wrap && kept {
                continue;
            }
        }
        // The box's empty title is measured (one space wide, one line high: it keeps its
        // room) [observed: hud_menus capture 2026-10-31, 14 x 38]; another empty text
        // (the files', a mod's) measures nothing, as before [design: not captured].
        let measured = if string.is_empty() && Some(t) != title { (0.0, 0.0) } else { size(font, &string, wrap) };
        if own_width {
            ui.set_trait(t, "width", Value::Num(measured.0));
        }
        if own_height {
            ui.set_trait(t, "height", Value::Num(measured.1));
        }
        last.insert(t, Measured { string, font, wrap, size: measured });
        written += 1;
    }
    written
}

/// A line for the log when what the box shows changes (smoke reads these).
pub fn describe_message(ui: &Ui, view: &PanelView) -> String {
    let width = ui.find_tile(MAIN).map_or(0.0, |m| num(ui, m, "width"));
    let rows = rows(ui).len();
    format!("tiles: {MESSAGE_MENU} drawn from tiles: \"{}\", {rows} button row(s), width {}", view.text.trim().replace('\n', " / "), Value::Num(width).text())
}

/// A line for the log when the prompt's amount changes.
pub fn describe_quantity(ui: &Ui) -> String {
    let shown = ui.get_string(&format!("{}/QM_AmountChosen/string", METER.rsplit_once('/').map_or("", |(p, _)| p))).unwrap_or_default();
    let most = ui.get_float(&format!("{METER}/user0")).unwrap_or(0.0);
    format!("tiles: {QUANTITY_MENU} drawn from tiles: amount {shown} of {}", Value::Num(most).text())
}

/// Module-owned authored prompt fixture, no production API or game file access.
#[cfg(test)]
pub(crate) fn load_quantity_fixture_for_touch_tests(ui:&mut Ui,change:impl FnOnce(&str)->String) {
    tests::load_whole_prompt(ui,change);
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_ui::MemSource;

    /// Every character 10 wide, lines 30 high, wrapped by whole characters [design].
    fn size(_: f64, text: &str, wrap: Option<f64>) -> (f64, f64) {
        let n = text.chars().count() as f64;
        let per = wrap.map_or(n, |w| (w / 10.0).floor().max(1.0));
        let lines = (n / per).ceil().max(1.0);
        (10.0 * n.min(per), 30.0 * lines)
    }

    /// [design] A small layout written for these tests, with its own structure, numbers and
    /// expressions; it does not come from the game's files and says nothing about them. It keeps
    /// the tile and trait names the model looks up, which are the engine's interface
    /// ([`message_built`], the menu's widths and buffer, the rows' spacing, the list's scroll
    /// step), and only the dependencies the tests exercise: the text moves right while the
    /// icon shows. Fonts 6 (the title) and 2 (the rows) are the numbers the measure and
    /// baseline tests below are about. The real layout's numbers are checked by the smoke
    /// scenario `tiles-message-box`.
    fn message() -> Ui {
        message_changed(str::to_string)
    }

    fn message_changed(change: impl FnOnce(&str) -> String) -> Ui {
        let src = MemSource::default().with(
            "message.xml",
            &change(r#"<menu name="MessageMenu"><_MinMenuWidth>180</_MinMenuWidth><_MaxMenuWidth>600</_MaxMenuWidth><_horbuf>30</_horbuf>
  <rect name="NOGLOW_BRANCH">
    <image name="MM_Background"><alpha>10</alpha></image>
    <rect name="MM_MainRect">
      <text name="MM_Title"><font>6</font><x>50</x></text>
      <image name="MM_MessageIcon"><visible>&false;</visible></image>
      <text name="MM_MessageText"><font>2</font><wrapwidth>580</wrapwidth>
        <x><copy src="sibling(MM_MessageIcon)" trait="visible"/><mul>90</mul><add>30</add></x></text>
      <rect name="MM_ButtonList"><_scroll_delta>25</_scroll_delta>
        <image name="lb_scrollbar"/>
      </rect>
    </rect>
  </rect>
  <template name="MM_ButtonTemplate"><rect name="MM_Button"><_VerticalSpacing>16</_VerticalSpacing>
    <text name="ListItemText"><font>2</font><wrapwidth>450</wrapwidth><string><copy src="parent()" trait="string"/></string></text>
  </rect></template>
</menu>"#),
        );
        let mut ui = Ui::new();
        ui.load_menu(&src, "message.xml").unwrap();
        ui.update();
        ui
    }

    fn view(title: Option<&str>, text: &str, buttons: &[&str]) -> PanelView {
        PanelView { title: title.map(str::to_string), text: text.into(), buttons: buttons.iter().map(|b| b.to_string()).collect(), icon: None }
    }

    fn f(ui: &Ui, path: &str) -> f64 {
        ui.get_float(&format!("MessageMenu/NOGLOW_BRANCH/MM_MainRect/{path}")).unwrap()
    }

    #[test]
    fn the_box_writes_its_strings_rows_and_sizes() {
        let mut ui = message();
        write_message(&mut ui, &view(Some("Title"), "Hello there", &["Yes", "No thanks"]), Some(1), true, &size);
        let main = "MessageMenu/NOGLOW_BRANCH/MM_MainRect";
        assert_eq!(ui.get_string(&format!("{main}/MM_Title/string")).as_deref(), Some("Title"));
        assert_eq!(f(&ui, "MM_Title/visible"), 1.0);
        assert_eq!(ui.get_string(&format!("{main}/MM_MessageText/string")).as_deref(), Some("Hello there"));
        // Two rows in the order of the buttons, each its text's height and the template's 16.
        let r = rows(&ui);
        assert_eq!(r.len(), 2);
        let row = |i: usize, t: &str| ui.tile(r[i]).get(t).map(Value::num).unwrap();
        assert_eq!((row(0, "listindex"), row(0, "_y"), row(0, "height")), (0.0, 0.0, 46.0));
        assert_eq!((row(1, "listindex"), row(1, "_y"), row(1, "height")), (1.0, 46.0, 46.0));
        assert_eq!(ui.tile(r[1]).get("string").map(Value::text).as_deref(), Some("No thanks"));
        // The list: the widest button ("No thanks", 90) and two _horbuf; the rows' height;
        // the chosen (second) row framed.
        assert_eq!((f(&ui, "MM_ButtonList/width"), f(&ui, "MM_ButtonList/height")), (150.0, 92.0));
        assert_eq!((f(&ui, "MM_ButtonList/_highlight_y"), f(&ui, "MM_ButtonList/_selected_height")), (46.0, 46.0));
        // No overflow: the bar's counts equal (a file hides the bar then).
        assert_eq!(f(&ui, "MM_ButtonList/lb_scrollbar/_number_of_items"), f(&ui, "MM_ButtonList/lb_scrollbar/_number_of_visible_items"));
        // No row chosen (a box at open): no row framed.
        write_message(&mut ui, &view(Some("Title"), "Hello there", &["Yes", "No thanks"]), None, false, &size);
        assert_eq!((f(&ui, "MM_ButtonList/_highlight_y"), f(&ui, "MM_ButtonList/_selected_height")), (-1.0, 0.0));
        // The box: the list's 150 and 60 is 210, over the text's 30 + 110 + 30, the
        // title's 50 + 50 + 10 + 50 and the least width, 180.
        assert_eq!(ui.get_float(&format!("{main}/width")), Some(210.0));
        assert_eq!(ui.get_float("MessageMenu/NOGLOW_BRANCH/MM_Background/alpha"), Some(background_alpha()));
        assert_eq!(row_button(&ui, child(&ui, r[1], "ListItemText").unwrap()), Some(1));
    }

    #[test]
    fn rows_follow_the_buttons_and_the_box_stays_within_its_widths() {
        let mut ui = message();
        write_message(&mut ui, &view(None, "x", &["a", "b", "c"]), Some(0), true, &size);
        assert_eq!(rows(&ui).len(), 3);
        // No title: hidden, so the file draws the top line whole; the least width.
        assert_eq!(f(&ui, "MM_Title/visible"), 0.0);
        assert_eq!(ui.get_float("MessageMenu/NOGLOW_BRANCH/MM_MainRect/width"), Some(180.0));
        // Fewer buttons unload the extra rows; the kept ones are the same tiles.
        let before = rows(&ui);
        write_message(&mut ui, &view(None, "x", &["only"]), Some(0), false, &size);
        assert_eq!(rows(&ui), before[..1].to_vec());
        // A long text (30 + 580 + 30 at its wrap): as wide as the box allows.
        write_message(&mut ui, &view(None, &"w".repeat(200), &["OK"]), Some(0), true, &size);
        assert_eq!(ui.get_float("MessageMenu/NOGLOW_BRANCH/MM_MainRect/width"), Some(600.0));
    }

    #[test]
    fn oversized_boxes_fall_back_and_a_later_short_box_can_be_tiled() {
        // [design] Twelve 46-unit rows exceed the 420-unit cap. Every reader of
        // message_tiled takes this same status after the model has measured the rows.
        let mut ui = message();
        let many: Vec<String> = (0..12).map(|i| format!("button {i}")).collect();
        let v = PanelView { buttons: many, ..view(None, "x", &[]) };
        write_message(&mut ui, &v, Some(11), true, &size);
        assert_eq!(message_status(&ui), Tiled::No("button rows exceed LIST_MOST (420); scrolling is not built".into()));
        assert!(!message_status(&ui).yes());
        // [design] The cap includes measured wrapping and spacing, not a button count.
        write_message(&mut ui, &v, None, false, &|_, _, _| (10.0, 19.0));
        assert_eq!(f(&ui, "MM_ButtonList/height"), LIST_MOST);
        assert_eq!(message_status(&ui), Tiled::Yes, "exactly 420 fits");
        write_message(&mut ui, &view(None, "x", &["a"]), None, true, &size);
        assert_eq!(message_status(&ui), Tiled::Yes);
        write_message(&mut ui, &view(None, "x", &[&"x".repeat(800)]), None, true, &size);
        assert!(!message_status(&ui).yes(), "one wrapped row can also exceed the cap");
    }

    #[test]
    fn cut_down_message_files_fall_back_for_every_model_path_and_template() {
        // [design] Each file still builds MessageMenu, but renames one required tile or
        // template so its vanilla path is absent, as a restructuring mod might do.
        for (name, missing) in [
            ("MM_MainRect", MAIN),
            ("MM_ButtonList", LIST),
            ("MM_Title", "MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_Title"),
            ("MM_MessageText", "MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_MessageText"),
            ("MM_MessageIcon", "MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_MessageIcon"),
            ("lb_scrollbar", "MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_ButtonList/lb_scrollbar"),
            ("MM_Background", "MessageMenu/NOGLOW_BRANCH/MM_Background"),
            (ROW_TEMPLATE, ROW_TEMPLATE),
            (ROW, "MM_ButtonTemplate/MM_Button"),
            ("ListItemText", "MM_ButtonTemplate/MM_Button/ListItemText"),
        ] {
            let mut ui = message_changed(|xml| xml.replace(&format!("name=\"{name}\""), "name=\"Modded\""));
            assert!(ui.menu(MESSAGE_MENU).is_some());
            let expected = Tiled::No(format!("{missing} missing"));
            assert_eq!(message_built(&ui), expected);
            assert_eq!(message_status(&ui), expected);
            write_message(&mut ui, &view(None, "x", &["OK"]), None, true, &size);
            assert!(rows(&ui).is_empty(), "a partial model is not written");
        }
    }

    #[test]
    fn cut_down_quantity_files_fall_back_for_every_model_path() {
        // [design] The shared eligibility helper rejects a loaded but partial prompt.
        for (body, missing) in [
            (r#"<rect name="QM_MainRect"><rect name="QM_AmountMeter"/></rect>"#, "QuantityMenu/NOGLOW_BRANCH/QM_Background"),
            (r#"<image name="QM_Background"/>"#, METER),
        ] {
            let src = MemSource::default().with("q.xml", &format!(r#"<menu name="QuantityMenu"><rect name="NOGLOW_BRANCH">{body}</rect></menu>"#));
            let mut ui = Ui::new();
            ui.load_menu(&src, "q.xml").unwrap();
            assert!(ui.menu(QUANTITY_MENU).is_some());
            assert_eq!(quantity_built(&ui), Tiled::No(format!("{missing} missing")));
            write_quantity(&mut ui, Amount { amount: 5, most: 20 });
            assert_eq!(ui.get_float(&format!("{METER}/user0")), None);
        }
    }

    /// A prompt's file with every path the model writes.
    fn whole_prompt() -> Ui {
        let mut ui=Ui::new();load_whole_prompt(&mut ui,str::to_owned);ui
    }
    pub(super) fn load_whole_prompt(ui:&mut Ui,change:impl FnOnce(&str)->String) {
        let xml=r#"<menu name="QuantityMenu"><rect name="NOGLOW_BRANCH"><image name="QM_Background"/><rect name="QM_MainRect"><rect name="QM_AmountMeter"/></rect></rect></menu>"#;
        let src=MemSource::default().with("q.xml",&change(xml));
        ui.load_menu(&src,"q.xml").unwrap();
    }

    #[test]
    fn the_prompt_follows_a_container_drawn_from_tiles_or_its_own_switch() {
        // [design] (switch, container drawn from tiles, prompt open, files read), a built file.
        let ui = whole_prompt();
        assert!(prompt_tiled(false, true, true, true, &ui), "the container's prompt, its switch off");
        assert!(prompt_tiled(true, false, true, true, &ui), "the switch, with no container: barter, inventory");
        assert!(prompt_tiled(true, true, true, true, &ui), "both");
        assert!(!prompt_tiled(false, false, true, true, &ui), "neither, as a container that fell back: by hand");
        assert!(!prompt_tiled(false, true, false, true, &ui), "no prompt open");
        assert!(!prompt_tiled(true, false, false, true, &ui), "no prompt open, the switch on");
        assert!(!prompt_tiled(false, true, true, false, &ui), "the menu files not read");
    }

    #[test]
    fn a_container_drawn_from_tiles_keeps_the_hand_built_prompt_without_its_file() {
        // [design] Never built (a missing or failed file: `Game::ui_prompt` leaves no menu).
        assert!(!prompt_tiled(false, true, true, true, &Ui::new()));
        assert!(!prompt_tiled(true, true, true, true, &Ui::new()));
        // Built, but short of a path the model writes.
        let src = MemSource::default().with("q.xml", r#"<menu name="QuantityMenu"><rect name="NOGLOW_BRANCH"><image name="QM_Background"/></rect></menu>"#);
        let mut ui = Ui::new();
        ui.load_menu(&src, "q.xml").unwrap();
        assert_eq!(quantity_built(&ui), Tiled::No(format!("{METER} missing")));
        assert!(!prompt_tiled(false, true, true, true, &ui));
    }

    #[test]
    fn an_icon_shows_and_makes_room() {
        let mut ui = message();
        let mut v = view(None, "Hello", &["OK"]);
        v.icon = Some(r"textures\interface\icons\message icons\glow_message_vaultboy_very_happy.dds".into());
        write_message(&mut ui, &v, Some(0), true, &size);
        assert_eq!(f(&ui, "MM_MessageIcon/visible"), 1.0);
        assert_eq!(ui.get_string("MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_MessageIcon/filename").as_deref(), v.icon.as_deref());
        // The layout moves the text 90 right while the icon shows; the box keeps that room
        // (the text's place, its 50 and the buffer).
        assert_eq!(f(&ui, "MM_MessageText/x"), 120.0);
        assert_eq!(ui.get_float("MessageMenu/NOGLOW_BRANCH/MM_MainRect/width"), Some(30.0 + 90.0 + 50.0 + 30.0));
    }

    #[test]
    fn text_sizes_are_written_once_per_change() {
        let mut ui = message();
        write_message(&mut ui, &view(Some("T"), "Hi", &["OK"]), Some(0), true, &size);
        ui.update();
        let menu = ui.menu(MESSAGE_MENU).unwrap();
        let mut last = SizesWritten::new();
        // The title, the text and the row's text.
        assert_eq!(write_text_sizes(&mut ui, menu, &mut last, &size), 3);
        assert_eq!((f(&ui, "MM_MessageText/width"), f(&ui, "MM_MessageText/height")), (20.0, 30.0));
        assert_eq!(write_text_sizes(&mut ui, menu, &mut last, &size), 0, "nothing changed");
        ui.set_string("MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_MessageText/string", "Hello");
        assert_eq!(write_text_sizes(&mut ui, menu, &mut last, &size), 1);
        assert_eq!(f(&ui, "MM_MessageText/width"), 50.0);
    }


    #[test]
    fn a_key_moves_the_choice_once() {
        // Finding of the T15a review: a file's `mouseover` ref and the hand-built Down both
        // moved the choice, so one press skipped a button.
        let mut ui = message();
        let v = view(None, "Choose", &["a", "b", "c"]);
        write_message(&mut ui, &v, Some(0), true, &size);
        let r = rows(&ui);
        let mut panel = crate::message_menu::MessagePanel::default();
        panel.set(Some(v));
        // The pointer over the first row chooses it.
        let hover = fnv_ui::input::Report { over: Some((None, Some(r[0]))), ..Default::default() };
        if let Some(i) = chosen_row(&ui, &hover, nav_by_hand(&[])) {
            panel.choose(i);
        }
        assert_eq!(panel.framed(), Some(0));
        // Down, from the keyboard: a ref moves the highlight to the second row and the
        // hand-built handler steps the choice; only the step counts.
        let keyed = fnv_ui::input::Report { over: Some((Some(r[0]), Some(r[1]))), ..Default::default() };
        if let Some(i) = chosen_row(&ui, &keyed, nav_by_hand(&["xdown"])) {
            panel.choose(i);
        }
        panel.key(1, true);
        // Enter answers the chosen button: the second.
        assert_eq!(panel.selected, 1);
        // A script's tapped Down is no hand-built press: the highlight's row is taken.
        assert_eq!(chosen_row(&ui, &keyed, nav_by_hand(&[])), Some(1));
    }

    #[test]
    fn a_tiled_prompt_or_box_keeps_a_sheet_under_the_pointer_while_shown() {
        use bevy::prelude::*;
        // Shown and drawn from tiles: the sheet; by hand: the hand-built drawing's own;
        // gone with it.
        assert_eq!(modal(true, true), Modal::Sheet);
        assert_eq!(modal(true, false), Modal::HandBuilt);
        assert_eq!((modal(false, true), modal(false, false)), (Modal::None, Modal::None));
        // The sheet covers the view and takes the pointer.
        let mut world = World::new();
        let e = world.spawn(modal_sheet()).id();
        let node = world.get::<Node>(e).unwrap();
        assert_eq!((node.width, node.height, node.position_type), (Val::Percent(100.0), Val::Percent(100.0), PositionType::Absolute));
        assert_eq!(world.get::<bevy::ui::FocusPolicy>(e), Some(&bevy::ui::FocusPolicy::Block));
        assert!(world.get::<crate::ui_pointer::PointerState>(e).is_some() && world.get::<ModalSheet>(e).is_some());
    }

    #[test]
    fn a_box_replaced_by_one_alike_is_another_box() {
        let mut ui = message();
        let menu = ui.menu(MESSAGE_MENU).unwrap();
        let v = view(None, "Again?", &["Yes", "No"]);
        let shown = |showing| BoxShown { menu, showing, view: v.clone() };
        // The first box, then the same box on the next frame: written once.
        assert!(another_box(None, &shown(Some(1))));
        assert!(!another_box(Some(&shown(Some(1))), &shown(Some(1))));
        // Replaced by a box alike in every word and button (the next showing): another box,
        // so its rows are made again and a click queued on the first's answers nothing.
        let first = shown(Some(1));
        write_message(&mut ui, &v, Some(0), another_box(None, &first), &size);
        let row = rows(&ui)[0];
        let mut pointer = fnv_ui::input::Pointer::default();
        pointer.script_click(&mut ui, row);
        let second = shown(Some(2));
        write_message(&mut ui, &v, Some(0), another_box(Some(&first), &second), &size);
        let report = pointer.step(&mut ui, &[menu], &fnv_ui::input::Input::default(), &fnv_ui::input::no_measure);
        assert_eq!(message_pick(&ui, &report.clicks[0], &[]), None);
        // Another menu tile (the menu built again), or other words: another box too.
        let mut other = shown(Some(2));
        other.view.text = "Other".into();
        assert!(another_box(Some(&second), &other));
    }

    #[test]
    fn a_click_queued_on_a_replaced_box_answers_nothing() {
        use fnv_ui::input::{no_measure, By, Input, Pointer};
        let mut ui = message();
        write_message(&mut ui, &view(None, "A", &["Yes", "No"]), Some(0), true, &size);
        let menu = ui.menu(MESSAGE_MENU).unwrap();
        let first = rows(&ui)[0];
        // A script clicks the first row of box A; A closes and B opens before the next step.
        let mut pointer = Pointer::default();
        pointer.script_click(&mut ui, first);
        write_message(&mut ui, &view(None, "B", &["Fight", "Flee"]), Some(0), true, &size);
        let report = pointer.step(&mut ui, &[menu], &Input::default(), &no_measure);
        assert_eq!(report.clicks.len(), 1);
        assert_eq!(report.clicks[0].by, By::Script);
        assert_eq!(message_pick(&ui, &report.clicks[0], &[]), None, "B is not answered with A's click");
        // On the same box, a script's click answers.
        let row = rows(&ui)[1];
        pointer.script_click(&mut ui, row);
        write_message(&mut ui, &view(None, "B", &["Fight", "Flee"]), Some(0), false, &size);
        let report = pointer.step(&mut ui, &[menu], &Input::default(), &no_measure);
        assert_eq!(message_pick(&ui, &report.clicks[0], &[]), Some(1));
    }

    #[test]
    fn a_key_acts_once_from_the_keyboard_and_through_its_binding_otherwise() {
        // The keyboard's A on the prompt: the hand-built handler took it (AmountOk).
        assert!(!key_click_acts(QUANTITY_MENU, "A", &["A"]));
        // A script's MenuTapKey A reaches only the tiles: its binding acts.
        assert!(key_click_acts(QUANTITY_MENU, "A", &[]));
        // The controller's A: acts through the file's xbuttona ref unless it stood in for
        // Enter this frame (the controller working the controls).
        assert!(key_click_acts(QUANTITY_MENU, "xbuttona", &[]));
        assert!(!key_click_acts(QUANTITY_MENU, "xbuttona", &["xbuttona"]));
        // A key the hand-built handler has no use for, bound by a file, acts.
        assert!(key_click_acts(MESSAGE_MENU, "Y", &["Y"]));
        assert!(!key_click_acts(MESSAGE_MENU, "xdown", &["xdown"]));
        assert!(nav_by_hand(&["xup"]) && !nav_by_hand(&["A"]));
    }

    #[test]
    fn a_computed_axis_is_left_and_the_other_written() {
        let src = MemSource::default().with(
            "m.xml",
            r#"<menu name="M"><text name="W"><string>abc</string><width><copy>77</copy></width></text>
              <text name="H"><string>abcd</string><height><copy>66</copy></height></text></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "m.xml").unwrap();
        ui.update();
        let mut last = SizesWritten::new();
        assert_eq!(write_text_sizes(&mut ui, menu, &mut last, &size), 2);
        let g = |p: &str| ui.get_float(&format!("M/{p}")).unwrap();
        // The width worked out by the file stays; the height is measured, and vice versa.
        assert_eq!((g("W/width"), g("W/height")), (77.0, 30.0));
        assert_eq!((g("H/width"), g("H/height")), (40.0, 66.0));
        assert_eq!(write_text_sizes(&mut ui, menu, &mut last, &size), 0, "nothing changed");
    }

    #[test]
    fn an_empty_title_keeps_its_room_and_the_row_text_stands_on_font_2s_line() {
        use super::super::draw::{piece_lines, pieces, text_base, Look, Select};
        // The measure as the renderer writes it for the menus (`draw::measure_written`):
        // an empty string one space wide and one line high; here font 6's space 14 and
        // tallest glyph 38, the original's empty `MM_Title` [observed: hud_menus capture
        // 2026-10-31, 14 x 38].
        fn sized(font: f64, text: &str, wrap: Option<f64>) -> (f64, f64) {
            match (font as u8, text.is_empty()) {
                (6, true) => (14.0, 38.0),
                _ => size(font, text, wrap),
            }
        }
        let mut ui = message();
        write_message(&mut ui, &view(None, "Hi", &["Ok"]), None, true, &sized);
        ui.update();
        let menu = ui.menu(MESSAGE_MENU).unwrap();
        write_text_sizes(&mut ui, menu, &mut SizesWritten::new(), &sized);
        ui.update();
        assert_eq!((f(&ui, "MM_Title/width"), f(&ui, "MM_Title/height"), f(&ui, "MM_Title/visible")), (14.0, 38.0, 0.0));
        // Another empty text (here the message's own, emptied) measures nothing.
        ui.set_string("MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_MessageText/string", "");
        write_text_sizes(&mut ui, menu, &mut SizesWritten::new(), &sized);
        assert_eq!((f(&ui, "MM_MessageText/width"), f(&ui, "MM_MessageText/height")), (0.0, 0.0));
        // The row's text, font 2: set out on font 2's line, 16 under its tile's top
        // [observed: hud_menus capture 2026-10-31, "Ok" 16.5 under ListItemText's y].
        let (set, _) = pieces(&ui, menu, &sized, Select::Whole { kept: &[], strip: None }, true);
        let ok = set.iter().find(|p| matches!(&p.look, Look::Text { string, .. } if string == "Ok")).unwrap();
        let lines = piece_lines(ok, &crate::interface::plain_font(31.0, 10.0));
        assert!(matches!(&ok.look, Look::Text { font: 2, .. }));
        assert_eq!(lines[0].2, text_base(2));
        assert_eq!(text_base(2), 16.0);
    }

    #[test]
    fn the_prompt_writes_the_stack_and_the_amount() {
        // [design] A small layout written for this test, with its own structure, numbers and
        // expressions; it does not come from the game's files and says nothing about them. It
        // keeps the paths and the traits the model writes (the stack `user0`, the amount
        // `user2`), which are the engine's interface, and works a fill and the amount's text out
        // of them, as a layout does. The real layout's numbers are checked by the smoke
        // scenario `tiles-quantity`.
        let src = MemSource::default().with(
            "quantity.xml",
            r#"<menu name="QuantityMenu"><rect name="NOGLOW_BRANCH"><image name="QM_Background"><alpha>10</alpha></image>
  <rect name="QM_MainRect"><x>40</x><y>30</y>
    <rect name="QM_AmountMeter"><x>100</x><y>80</y><width>300</width><height>20</height>
      <image name="Fill"><width><copy src="parent()" trait="user2"/><div src="parent()" trait="user0"/><mul src="parent()" trait="width"/></width></image></rect>
    <text name="QM_AmountChosen"><string><copy src="sibling(QM_AmountMeter)" trait="user2"/></string></text>
    <rect name="QM_OKButton"><text name="Label"/></rect>
    <image name="QM_DecreaseArrow"/>
  </rect></rect></menu>"#,
        );
        let mut ui = Ui::new();
        ui.load_menu(&src, "quantity.xml").unwrap();
        write_quantity(&mut ui, Amount { amount: 5, most: 20 });
        ui.update();
        assert_eq!(ui.get_float(&format!("{METER}/Fill/width")), Some(300.0 * 0.25));
        assert_eq!(ui.get_string("QuantityMenu/NOGLOW_BRANCH/QM_MainRect/QM_AmountChosen/string").as_deref(), Some("5"));
        assert_eq!(describe_quantity(&ui), "tiles: QuantityMenu drawn from tiles: amount 5 of 20");
        assert_eq!(ui.get_float("QuantityMenu/NOGLOW_BRANCH/QM_Background/alpha"), Some(background_alpha()));
        // The meter's box from the places down the tree; the tiles by what they stand for.
        let b = meter_box(&ui).unwrap();
        assert_eq!((b.left, b.top, b.width, b.height), (140.0, 110.0, 300.0, 20.0));
        let tile = |p: &str| ui.find_tile(&format!("QuantityMenu/NOGLOW_BRANCH/QM_MainRect/{p}")).unwrap();
        assert_eq!(quantity_tile(&ui, tile("QM_OKButton/Label")), Some(QuantityTile::Ok));
        assert_eq!(quantity_tile(&ui, tile("QM_AmountMeter/Fill")), Some(QuantityTile::Meter));
        assert_eq!(quantity_tile(&ui, tile("QM_DecreaseArrow")), Some(QuantityTile::Decrease));
        assert_eq!(quantity_tile(&ui, tile("QM_AmountChosen")), None);
    }
}

