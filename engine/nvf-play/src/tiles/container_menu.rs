//! T15c: the container menu drawn from `menus\container_menu.xml`, behind
//! `[UI] TileMenus=ContainerMenu` [design: ui-tile-tree.md §6.2 step 3; notes: barter-menu.md
//! §8]. Off (the default), none of this runs and the hand-built menu (`crate::menu::look`)
//! draws. While it draws, its "How many?" prompt is drawn from `QuantityMenu`'s tiles too,
//! with no switch of its own (`super::menus::quantity_tiled`).
//!
//! - **The rules stay the hand-built menu's** (`crate::menu::model`): what a row is, its
//!   order, the prompt, Take All and what it closes; every command goes through
//!   `crate::menu::MenuInput` to `apply_input`, with its sounds, theft and events. This
//!   module is the view: it writes the model's state as the file's "set in code" traits and
//!   says what a click on a tile stands for. Only the lists' scroll is its own, which the
//!   model never had in units (it kept a first row for the hand-built drawing).
//! - **A frame** ([`settle`], before `super::input::feed`): the tree worked out first (a new
//!   window size leads to places), what changed written ([`write`]), the texts measured, the
//!   rows placed, the counts, scroll and highlight written, the tree worked out again. So
//!   the tree is one whole state of the model as the frame's input sees it. `apply_input`
//!   runs later in the frame (`PlaySet::Ui`): a command's result reaches the tree on the next
//!   frame, and the frame's drawing shows the state it was clicked on. After the input's step
//!   the scroll and highlight are written again ([`ContainerTiles::rewrite`]): the file's
//!   value counts clicks from 0, and each pulse's end would bring that back.
//! - **Clicks on rows that changed**: a row's tile stands for one key for its whole life
//!   (`super::list::reconcile`), a key gone unloads its tile, and a click is checked against
//!   the model's row at that index ([`ContainerTiles::row_at`]); at most one row click acts a
//!   frame, as the first may change the rows under the next. Commands queued before it may
//!   too (a script's heading click): the row's commands go behind a guard with the menu and
//!   the key ([`ContainerTiles::guard`]), checked as they apply (`crate::menu::apply_input`).
//! - **Both lists are enabled** [observed: `_enabled` 1 and 1 at every read, though the
//!   prefab's comment warns against it]; the list without the focus shows no highlight by
//!   `_highlight_y` -1 and `_selected_height` 0 [observed]. Its rows take the pointer, so a
//!   row of either list can gain the focus.
//! - **Alpha**: every tile of the menu takes only its own alpha ([`mark_tiles`],
//!   `super::layout::OWN_ALPHA`) [observed: barter-menu.md §8]. The highlight's fill is drawn
//!   (`super::layout::HOTRECT_FILL`).
//! - **Keys** ([`HAND_BUILT_KEYS`]): the hand-built keys stay the keys, so physical A focuses
//!   the player's list and R takes all, against the file's `_PCButton_A`; a script's MenuTapKey
//!   still acts through the file. The Take All hint reads `sPCMenuHintR` as Exit's reads the
//!   file's `sPCMenuHintE`, so what shows is what works, both in the words of the strings table
//!   (`fnv_ui::strings`) [design: crate::menu's keys, which no owner decision set; the file
//!   names `sPCMenuHintA`].
//! - **The wheel** stays `crate::menu::menu_mouse`'s: it moves the selection, over a heading
//!   the category; this module only says which heading the pointer is over ([`TileWheelTitle`]).

use super::item_menu::{self, on, set, side_name, Paths};
use super::list;
use super::menus::{SizesWritten, Tiled};
use crate::menu::look;
use crate::menu::model::{MenuModel, Mode, Side};
use crate::pipboy::items::Info;
use fnv_ui::{Ui, Value};

pub use super::item_menu::{MenuTiles as ContainerTiles, MenuTile as ContainerTile, TileWheelTitle,
    RowShown, ListShown, Snapshot, rows_shown, takes_input, hover_selects, row_commands, row_commands_with};

/// The menu's name [documented: container_menu.xml].
pub const CONTAINER_MENU: &str = "ContainerMenu";

const BACKGROUND: &str = "ContainerMenu/NOGLOW_BRANCH/CM_Background";
const SUBTITLE: &str = "ContainerMenu/NOGLOW_BRANCH/CM_Subtitle";
const WEIGHT: &str = "ContainerMenu/NOGLOW_BRANCH/CM_ItemsRect/CM_Items_CapsLabel";
const ARROW: &str = "ContainerMenu/NOGLOW_BRANCH/CM_ArrowIcon";
const ICON: &str = "ContainerMenu/NOGLOW_BRANCH/CM_ItemIcon";
const CARD: &str = "ContainerMenu/NOGLOW_BRANCH/CM_ItemData";
const TAKE_ALL: &str = "ContainerMenu/NOGLOW_BRANCH/CM_ButtonRect/CM_TakeAllButton";
const EXIT: &str = "ContainerMenu/NOGLOW_BRANCH/CM_ButtonRect/CM_ExitButton";
const TEMPLATE: &str = "CM_list_template";
const ROW: &str = "CM_list_template_container";
const ROW_TEXT: &str = "ListItemText";
const ROW_MARK: &str = "CM_list_template_ItemMarker";
/// By [`Side`]: the container's at the right, the player's at the left.
const LISTS: [&str; 2] = ["ContainerMenu/NOGLOW_BRANCH/CM_ContainerRect/CM_Container_InventoryList", "ContainerMenu/NOGLOW_BRANCH/CM_ItemsRect/CM_Items_InventoryList"];
const TITLES: [&str; 2] = ["ContainerMenu/NOGLOW_BRANCH/CM_ContainerRect/CM_ContainerTitle", "ContainerMenu/NOGLOW_BRANCH/CM_ItemsRect/CM_ItemsTitle"];

/// The backing's opacity [observed: `CM_Background`'s `alpha` 193.8 at 1280 x 720, the file's
/// `_background_fill_alpha` of 32 being replaced in the original].
pub const BACKGROUND_ALPHA: f64 = 193.8;

/// The setting of the hint beside Take All: the key that does it here (module notes) [documented:
/// menu-xml.md §2.4 lists the setting; `fnv_ui::strings::BUILT_IN` words it as the other keys'].
pub const TAKE_ALL_HINT: &str = "sPCMenuHintR";

/// [design] The container adapter's paths and click meanings.
pub(super) const PATHS: Paths = Paths {
    menu: CONTAINER_MENU, lists: LISTS, titles: TITLES, template: TEMPLATE,
    row: ROW, row_text: ROW_TEXT, row_mark: ROW_MARK,
    targets: &[
        ("CM_TakeAllButton", ContainerTile::TakeAll), ("CM_ExitButton", ContainerTile::Exit),
        ("CM_Items_LeftFilterArrow", ContainerTile::Arrow(Side::Player, -1)),
        ("CM_Items_RightFilterArrow", ContainerTile::Arrow(Side::Player, 1)),
        ("CM_Container_LeftFilterArrow", ContainerTile::Arrow(Side::Container, -1)),
        ("CM_Container_RightFilterArrow", ContainerTile::Arrow(Side::Container, 1)),
        ("CM_ItemsTitle", ContainerTile::Title(Side::Player)),
        ("CM_ContainerTitle", ContainerTile::Title(Side::Container)),
    ],
};

/// [design] Every path the model writes or input reads is part of a successful build, the
/// buttons' words, which give them their sizes, the condition meter's drawable image, and the
/// rows' template with the two children written; a file a mod restructured keeps the hand-built menu whole. Worked out once a frame
/// ([`ContainerTiles::menu`]).
pub fn built(ui: &Ui) -> Tiled {
    let mut paths: Vec<String> = [CONTAINER_MENU, BACKGROUND, SUBTITLE, WEIGHT, ARROW, ICON, CARD, TAKE_ALL, EXIT].iter().map(|s| (*s).to_string()).collect();
    paths.extend([TAKE_ALL, EXIT].map(|b| format!("{b}/button_text")));
    paths.extend(TITLES.iter().map(|s| (*s).to_string()));
    for side in ["CM_ContainerRect/CM_Container", "CM_ItemsRect/CM_Items"] {
        for arrow in ["LeftFilterArrow", "RightFilterArrow"] {
            paths.push(format!("ContainerMenu/NOGLOW_BRANCH/{side}_{arrow}"));
        }
    }
    for list_path in LISTS {
        paths.push(format!("{list_path}/{}", list::HIGHLIGHT));
        for piece in [list::MARKER, list::UP, list::DOWN, list::PAGE_UP, list::PAGE_DOWN] {
            paths.push(format!("{list_path}/{}/{piece}", list::SCROLLBAR));
        }
    }
    paths.extend(super::item_card::CARDS.iter().chain(&[super::item_card::EFFECTS, super::item_card::CONDITION_ARROWS, super::item_card::CONDITION_METER, super::item_card::CONDITION_METER_IMAGE]).map(|c| format!("{CARD}/{c}")));
    for card in super::item_card::CARDS.iter().chain(&[super::item_card::EFFECTS]) {
        paths.extend(super::item_card::CARD_TEXTS.map(|t| format!("{CARD}/{card}/{t}")));
    }
    item_menu::built(ui, &PATHS, paths, &[("text", ROW_TEXT), ("image", ROW_MARK)])
}

/// Whether the open container menu is drawn from its tiles: the switch on, the menu files read,
/// a container's menu open (`open`: `crate::menu::ActiveMenu`'s mode; the barter menu keeps its
/// drawing), the top menu 1008 (a box a script shows over it builds its own file) and the file
/// built with every path. Every drawing and input reader uses this decision [design], as this
/// frame's writer left it ([`ContainerTiles::menu`]).
pub fn tiled(on: bool, game: &fnv_game::Game<'_>, open: bool) -> bool {
    on && open && game.ui_files.is_some() && game.menu == Some(crate::script::MENU_CONTAINER) && built(&game.ui_tree).yes()
}

/// Whether `m` is a menu this file draws.
pub fn is_container(m: Option<&MenuModel>) -> bool {
    m.is_some_and(|m| matches!(m.mode, Mode::Container(_)))
}

/// The weight label [observed: three spaces; 289.50 carried reads 289, 49.50 reads 49, so
/// whole units cut off]. The word is the hand-built menu's (`look::weight_word`, the
/// setting `sInventoryWeight`) [design, #4495].
fn weight_text(weight: f64, capacity: f64) -> String {
    format!("{}   {:.0}/{capacity:.0}", crate::menu::look::weight_word(), weight.floor())
}

fn weight_label(m: &MenuModel) -> String {
    m.encumbrance.as_ref().map_or(String::new(), |e| weight_text(e.weight, e.capacity))
}

impl Snapshot {
    /// What `m` shows; `info` the selected row's figures (`crate::pipboy::items::info_of`),
    /// `note` the line under the menu (`crate::menu::note`) and `take_all_hint`
    /// [`TAKE_ALL_HINT`]'s text.
    pub fn of(m: &MenuModel, info: Option<&Info>, note: Option<String>, take_all_hint: String) -> Snapshot {
        let list = |side: Side| ListShown {
            rows: rows_shown(&m.list(side).rows),
            category: m.category[side as usize],
            selected: (m.focus == side && m.selected_row(side).is_some()).then_some(m.selected[side as usize]),
            filtered: m.filtered[side as usize],
        };
        let selected = m.selected_row(m.focus);
        Snapshot {
            lists: [list(Side::Container), list(Side::Player)],
            titles: [look::half_title(m, Side::Container), look::half_title(m, Side::Player)],
            weight: weight_label(m),
            focus: m.focus,
            pointer_pick: m.pointer_pick,
            asking: m.quantity.is_some(),
            navigated: m.navigated,
            card: selected.map(|r| look::item_figures(m, r, info)),
            icon: selected.and_then(|r| r.icon.clone()),
            take_all: !m.list(Side::Container).rows.is_empty(),
            take_all_hint,
            note,
            ysi_icons: m.ysi_icons,
        }
    }
}

/// Write what `ContainerMenu` shows of `s`: the rows, the headings, the weight, the item's
/// picture and figures, the arrow, the line under the menu, Take All's state and hint, and the
/// backing. A list's places are [`settle`]'s, once the texts are measured.
pub fn write(ui: &mut Ui, s: &Snapshot, st: &mut ContainerTiles) {
    item_menu::write_lists(ui, s, st);
    set(ui, WEIGHT, "string", Value::Str(s.weight.clone()));
    // The arrow points the way the item would go [observed: `user0` 1 with the left list's
    // row selected, 2 with the right's, not shown with none].
    set(ui, ARROW, "visible", on(s.card.is_some()));
    set(ui, ARROW, "user0", Value::Num(if s.focus == Side::Player { 1.0 } else { 2.0 }));
    set(ui, ICON, "visible", on(s.card.is_some() && s.icon.is_some()));
    set(ui, ICON, "filename", Value::Str(s.icon.clone().unwrap_or_default()));
    super::item_card::write(ui, CARD, s.card.as_ref());
    // The file's own one blank without a note [documented].
    set(ui, SUBTITLE, "string", Value::Str(s.note.clone().unwrap_or_else(|| " ".into())));
    set(ui, TAKE_ALL, "target", on(s.take_all));
    set(ui, TAKE_ALL, "_PCButtonText", Value::Str(s.take_all_hint.clone()));
    set(ui, BACKGROUND, "alpha", Value::Num(BACKGROUND_ALPHA));
}

/// [design] Use the shared measuring, row placement and scrolling path.
pub fn settle(ui: &mut Ui, s: &Snapshot, st: &mut ContainerTiles, write_sizes: &mut dyn FnMut(&mut Ui, &mut SizesWritten) -> usize) {
    item_menu::settle(ui, s, st, write_sizes);
}

/// [design] Apply the shared marks to this menu only.
pub fn mark_tiles(ui: &mut Ui) { item_menu::mark_tiles(ui, &PATHS); }

/// The keys the hand-built menu acts on itself (`crate::menu::menu_keys`), by the tile input's
/// names: W, S, A and D, E, R and the arrows, and the controller buttons standing in there:
/// A, B, the shoulders and triggers, the D-pad's as the arrows, Start, X for R
/// (`crate::pad::menu_buttons`). The file binds A to Take All and E to Exit [documented:
/// `_PCButton_A`, `_PCButton_E`]: both stay the hand-built keys (module notes), a script's
/// MenuTapKey acting through the file.
pub const HAND_BUILT_KEYS: [&str; 18] = ["A", "D", "W", "S", "E", "R", "xup", "xdown", "xleft", "xright", "xbuttona", "xbuttonb", "xbuttonx", "xbuttonlb", "xbuttonrb", "xbuttonlt", "xbuttonrt", "xbuttonstart"];

/// A line for the log when what the menu shows changes (smoke reads these).
pub fn describe(ui: &Ui, s: &Snapshot) -> String {
    let [container, player] = &s.lists;
    let selected = s.lists[s.focus as usize].selected.and_then(|k| s.lists[s.focus as usize].rows.get(k)).map_or("none", |r| r.text.as_str());
    let figures = super::item_card::describe(ui, CARD);
    format!(
        "tiles: {CONTAINER_MENU} drawn from tiles: \"{}\" {} rows, \"{}\" {} rows, \"{}\", {} selected \"{selected}\", figures: {figures}",
        s.titles[Side::Player as usize],
        player.rows.len(),
        s.titles[Side::Container as usize],
        container.rows.len(),
        s.weight,
        side_name(s.focus),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::model::{Category, RowKey};
    use super::super::list::Drag;
    use fnv_sim::FormId;
    use fnv_ui::MemSource;

    /// A list's height in [`menu`], which holds eight rows of 40 whole.
    const PAGE: f64 = 340.0;
    /// The `_scroll_delta` in [`menu`] until code writes it.
    const OWN_DELTA: f64 = 15.0;

    /// [design] A small layout written for these tests, with its own structure, numbers and
    /// expressions; it does not come from the game's files and says nothing about them. It keeps
    /// the tile names and paths the model and the input look up, which are the engine's interface
    /// ([`built`]), the trait holders the card writes to, and only the dependencies the tests
    /// exercise: a row stands at its `_y` less its list's `_offset`, and the highlight at what the
    /// list was told. With [`list::lines_size`] a row is 40 high, 30 more for each further line.
    /// The real layout's numbers are checked by the smoke scenarios `tiles-container` and
    /// `tiles-container-quantity`.
    fn menu_changed(change: impl FnOnce(&str) -> String) -> Ui {
        let cards: String = ["DamageResistInfo", "DPSInfo", "WeightInfo", "ValueInfo", "ConditionInfo", "AmmoInfo", "EffectsInfo", "StrengthReqInfo", "DAMInfo", "DamageThresholdInfo"]
            .iter()
            .map(|c| {
                let inner = if *c == "ConditionInfo" { r#"<rect name="ConditionMeter"><image name="MeterImage"></image></rect><image name="CNDArrows"></image>"# } else { "" };
                format!(r#"<rect name="{c}"><_Title>{c}</_Title><_Value></_Value><visible>&false;</visible><text name="CI_TitleText"></text><text name="CI_ValueText"></text>{inner}</rect>"#)
            })
            .collect();
        // One side of the menu, its names all from `prefix`: at x, 50 down; its list 10 in and 20
        // down from there. A list's clip is what a row is cut by; the pieces of its bar are names.
        let side = |prefix: &str, x: u32, extra: &str| {
            format!(
                r#"<rect name="{prefix}Rect"><x>{x}</x><y>50</y>
                <image name="{prefix}_LeftFilterArrow"/><text name="{prefix}Title"><string></string></text><image name="{prefix}_RightFilterArrow"/>{extra}
                <hotrect name="{prefix}_InventoryList"><x>10</x><y>20</y><width>240</width><height>340</height><clipwindow>&true;</clipwindow>
                  <_scroll_delta>15</_scroll_delta><_highlight_y>-1</_highlight_y><_selected_height>0</_selected_height>
                  <_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>
                  <hotrect name="lb_highlight_box"><alpha>60</alpha><width>200</width>
                    <y><copy src="parent()" trait="_highlight_y"/></y><height><copy src="parent()" trait="_selected_height"/></height></hotrect>
                  <image name="lb_scrollbar"><alpha>0</alpha><_current_value>0</_current_value>
                    <image name="scrollbar_vert_marker"/><image name="scrollbar_vert_up"/><image name="scrollbar_vert_down"/>
                    <hotrect name="scrollbar_vert_page_up"/><hotrect name="scrollbar_vert_page_down"/>
                  </image></hotrect></rect>"#
            )
        };
        let items = side("CM_Items", 100, r#"<text name="CM_Items_CapsLabel"></text>"#);
        let container = side("CM_Container", 500, "");
        let xml = format!(
            r#"<menu name="ContainerMenu"><rect name="NOGLOW_BRANCH">
            <image name="CM_Background"><alpha>10</alpha></image><rect name="CM_Subtitle"/>
            {items}{container}
            <image name="CM_ArrowIcon"/><image name="CM_ItemIcon"/>
            <rect name="CM_ButtonRect">
              <image name="CM_TakeAllButton"><alpha>0</alpha><text name="button_text"><string>Go</string></text></image>
              <image name="CM_ExitButton"><alpha>0</alpha><text name="button_text"><string>Stop</string></text></image>
            </rect>
            <rect name="CM_ItemData"><user1>30</user1>{cards}</rect>
            </rect>
            <template name="CM_list_template"><hotrect name="CM_list_template_container"><x>30</x><width>200</width><target>&true;</target>
              <_y>0</_y><_VerticalSpacing>10</_VerticalSpacing>
              <y><copy src="me()" trait="_y"/><sub src="parent()" trait="_offset"/></y>
              <text name="ListItemText"></text><image name="CM_list_template_ItemMarker"/>
            </hotrect></template></menu>"#
        );
        let mut ui = Ui::new();
        ui.load_menu(&MemSource::default().with("cm.xml", &change(&xml)), "cm.xml").unwrap();
        ui.update();
        ui
    }

    fn menu() -> Ui {
        menu_changed(str::to_string)
    }

    fn row(item: u32, text: &str) -> RowShown {
        RowShown { key: RowKey { units: (FormId(item), None, 0), reference: None, nth: 0 }, name: format!("Item {item}"), text: text.into(), equipped: false, dim: false, ysi: None }
    }

    fn snapshot(left: &[(u32, &str)], right: &[(u32, &str)], focus: Side, selected: usize) -> Snapshot {
        let list = |rows: &[(u32, &str)], side: Side| ListShown {
            rows: rows.iter().map(|&(i, t)| row(i, t)).collect(),
            category: Category::All,
            selected: (focus == side && selected < rows.len()).then_some(selected),
            filtered: 0,
        };
        Snapshot {
            lists: [list(right, Side::Container), list(left, Side::Player)],
            titles: ["Footlocker".into(), "ITEMS".into()],
            weight: "Weight   52/200".into(),
            focus,
            pointer_pick: false,
            asking: false,
            navigated: 0,
            card: Some(Info { value: Some("30".into()), weight: Some("5.00".into()), ..Info::default() }),
            icon: Some("interface\\icons\\x.dds".into()),
            take_all: !right.is_empty(),
            take_all_hint: "[R]".into(),
            note: None,
            ysi_icons: true,
        }
    }

    #[test]
    fn the_weight_cuts_off_to_whole_units() {
        assert_eq!(weight_text(52.0, 200.0), "Weight   52/200");
        assert_eq!(weight_text(49.5, 200.0), "Weight   49/200");
        assert_eq!(weight_text(289.5, 200.0), "Weight   289/200");
    }

    /// A frame as the writer does it: what changed written, then [`settle`].
    fn frame(ui: &mut Ui, s: &Snapshot, st: &mut ContainerTiles) {
        frame_sized(ui, s, st, &list::lines_size);
    }

    /// [`frame`] with the texts measured by `size`.
    fn frame_sized(ui: &mut Ui, s: &Snapshot, st: &mut ContainerTiles, size: &super::super::menus::Size<'_>) {
        let menu = ui.menu(CONTAINER_MENU).unwrap();
        st.menu = Some(menu);
        if st.is_new(menu, s) {
            write(ui, s, st);
        }
        let texts = super::super::menus::texts(ui, menu);
        settle(ui, s, st, &mut |ui: &mut Ui, sizes: &mut SizesWritten| super::super::menus::write_text_sizes_of(ui, &texts, sizes, size));
        mark_tiles(ui);
    }

    fn at(ui: &Ui, path: &str) -> f64 {
        ui.get_float(path).unwrap_or(-999.0)
    }

    /// The highlighted row stands whole in its list, by what the tree holds.
    #[track_caller]
    fn assert_selected_shown(ui: &Ui, side: Side) {
        let list = LISTS[side as usize];
        let (y, height, page) = (at(ui, &format!("{list}/_highlight_y")), at(ui, &format!("{list}/_selected_height")), at(ui, &format!("{list}/height")));
        assert!(height > 0.0 && y >= -1e-6 && y + height <= page + 1e-6, "the row at y {y}, {height} high, in a list {page} high");
    }

    fn twenty() -> Vec<(u32, String)> {
        (1..=20).map(|i| (i, format!("Row {i}"))).collect()
    }

    fn refs(v: &[(u32, String)]) -> Vec<(u32, &str)> {
        v.iter().map(|(i, s)| (*i, s.as_str())).collect()
    }

    #[test]
    fn writes_both_lists_and_unit_figures() {
        let (mut ui, mut st) = (menu(), ContainerTiles::default());
        let s = snapshot(&[(1, "9mm Pistol"), (2, "Long|name")], &[(3, "Bottle Cap (25)")], Side::Player, 1);
        frame(&mut ui, &s, &mut st);
        let left = LISTS[Side::Player as usize];
        let right = LISTS[Side::Container as usize];
        // The rows, named after their items, the second two lines high; both places the model's.
        let rows = &st.rows[Side::Player as usize];
        assert_eq!((rows.len(), st.rows[Side::Container as usize].len()), (2, 1));
        assert_eq!(ui.tile(rows[1].tile).name, "Item 2");
        assert_eq!(ui.tile(rows[1].tile).get("height").map(Value::num), Some(70.0));
        assert_eq!(ui.tile(rows[1].tile).get("_y").map(Value::num), Some(40.0));
        assert_eq!(ui.get_string(&format!("{}/{ROW_TEXT}/string", ui.path_of(rows[0].tile))).as_deref(), Some("9mm Pistol"));
        // The highlight only in the list with the focus [observed].
        assert_eq!((at(&ui, &format!("{left}/_highlight_y")), at(&ui, &format!("{left}/_selected_height"))), (40.0, 70.0));
        assert_eq!((at(&ui, &format!("{right}/_highlight_y")), at(&ui, &format!("{right}/_selected_height"))), (-1.0, 0.0));
        // Fits: one item, the delta the layout holds.
        assert_eq!(at(&ui, &format!("{left}/lb_scrollbar/_number_of_items")), 1.0);
        assert_eq!(at(&ui, &format!("{left}/_scroll_delta")), OWN_DELTA);
        // The strings, the arrow, the picture, the figures.
        assert_eq!(ui.get_string(&format!("{}/string", TITLES[0])).as_deref(), Some("Footlocker"));
        assert_eq!(ui.get_string(&format!("{WEIGHT}/string")).as_deref(), Some("Weight   52/200"));
        assert_eq!((at(&ui, &format!("{ARROW}/visible")), at(&ui, &format!("{ARROW}/user0"))), (1.0, 1.0));
        assert_eq!(at(&ui, &format!("{ICON}/visible")), 1.0);
        assert_eq!(ui.get_string(&format!("{CARD}/WeightInfo/_Value")).as_deref(), Some("5.00"));
        assert_eq!(ui.get_string(&format!("{TAKE_ALL}/_PCButtonText")).as_deref(), Some("[R]"));
        assert_eq!(at(&ui, &format!("{BACKGROUND}/alpha")), BACKGROUND_ALPHA);
        assert!(describe(&ui, &s).contains("\"ITEMS\" 2 rows, \"Footlocker\" 1 rows"), "{}", describe(&ui, &s));
    }

    #[test]
    fn alpha_is_each_tiles_own_and_the_highlight_fills() {
        let (mut ui, mut st) = (menu(), ContainerTiles::default());
        frame(&mut ui, &snapshot(&[(1, "One")], &[(2, "Two")], Side::Player, 0), &mut st);
        let menu = ui.menu(CONTAINER_MENU).unwrap();
        let items = super::super::layout::layout(&ui, menu, &list::lines_size, true);
        let alpha = |path: &str| items.iter().find(|i| i.tile == ui.find_tile(path).unwrap()).unwrap().alpha;
        assert_eq!(alpha(&format!("{TAKE_ALL}/button_text")), 255.0, "under an image at alpha 0");
        assert_eq!(alpha(&format!("{}/lb_scrollbar/scrollbar_vert_marker", LISTS[1])), 255.0);
        assert_eq!(alpha(&format!("{}/lb_highlight_box", LISTS[1])), 60.0, "the fill's own");
        assert_eq!(alpha(BACKGROUND), BACKGROUND_ALPHA, "0-255 here; `draw::pieces` divides by 255");
    }

    #[test]
    fn the_keys_pick_scrolls_the_pointers_does_not() {
        let (mut ui, mut st) = (menu(), ContainerTiles::default());
        let rows = twenty();
        let rows = refs(&rows);
        let left = LISTS[1];
        frame(&mut ui, &snapshot(&rows, &[], Side::Player, 0), &mut st);
        // 20 rows of 40 in 340: 8 whole, 12 positions after the first.
        assert_eq!(at(&ui, &format!("{left}/lb_scrollbar/_number_of_items")), 13.0);
        let delta = at(&ui, &format!("{left}/_scroll_delta"));
        assert!((delta - (800.0 - PAGE) / 12.0).abs() < 1e-9, "{delta}");
        // A key's pick of the row under the fold.
        frame(&mut ui, &snapshot(&rows, &[], Side::Player, 8), &mut st);
        assert_eq!(st.value(Side::Player), 1.0, "moved as little as shows it whole");
        // The pointer's pick of the partial row stays put, with the highlight on its place.
        let mut pointed = snapshot(&rows, &[], Side::Player, 9);
        pointed.pointer_pick = true;
        frame(&mut ui, &pointed, &mut st);
        assert_eq!(st.value(Side::Player), 1.0);
        assert!((at(&ui, &format!("{left}/_highlight_y")) - (9.0 * 40.0 - delta)).abs() < 1e-9);
        // The bar moves the list; a frame with the same pick does not bring it back.
        assert!(st.scroll_to(Side::Player, 5.0));
        frame(&mut ui, &pointed, &mut st);
        assert_eq!(st.value(Side::Player), 5.0);
        let keyed = snapshot(&rows, &[], Side::Player, 9);
        frame(&mut ui, &keyed, &mut st);
        frame(&mut ui, &keyed, &mut st);
        assert_eq!(st.value(Side::Player), 5.0, "the row was seen with the pointer's pick");
        assert!(st.scroll_to(Side::Player, 12.0));
        assert!(!st.scroll_to(Side::Player, 99.0), "held at the end");
        // A shorter list, the same view: held in its range.
        frame(&mut ui, &snapshot(&rows[..3], &[], Side::Player, 0), &mut st);
        assert_eq!(st.value(Side::Player), 0.0);
        assert_eq!(list::value_shown(&ui, ui.find_tile(left).unwrap()), Some(0.0));
    }

    #[test]
    fn another_category_starts_at_the_top() {
        let (mut ui, mut st) = (menu(), ContainerTiles::default());
        let rows = twenty();
        let rows = refs(&rows);
        let mut s = snapshot(&rows, &[], Side::Player, 0);
        s.pointer_pick = true;
        frame(&mut ui, &s, &mut st);
        st.scroll_to(Side::Player, 6.0);
        s.lists[Side::Player as usize].category = Category::Aid;
        frame(&mut ui, &s, &mut st);
        assert_eq!(st.value(Side::Player), 0.0);
    }

    #[test]
    fn inactive_column_can_gain_focus() {
        let (mut ui, mut st) = (menu(), ContainerTiles::default());
        let rows = twenty();
        let rows = refs(&rows);
        let s = snapshot(&[(1, "One")], &rows, Side::Player, 0);
        frame(&mut ui, &s, &mut st);
        let menu = ui.menu(CONTAINER_MENU).unwrap();
        let under = |ui: &Ui, st: &ContainerTiles, at: (f64, f64)| fnv_ui::input::hit(ui, &[menu], at, &fnv_ui::input::no_measure).and_then(|h| st.clicked(ui, h.tile));
        // The right list's first row, its list without the focus: x 500 + 10 + 30, y 50 + 20.
        assert_eq!(under(&ui, &st, (600.0, 90.0)), Some(ContainerTile::Row(Side::Container, 0)));
        assert_eq!(row_commands(Side::Container, 0, false, false), vec![crate::menu::model::Command::Hover(Side::Container, 0), crate::menu::model::Command::One]);
        // The list moved: its rows follow, 2 x (800 - 340) / 12 = 76.67 up; the third is there now.
        assert!(st.scroll_to(Side::Container, 2.0));
        frame(&mut ui, &s, &mut st);
        assert_eq!(under(&ui, &st, (600.0, 90.0)), Some(ContainerTile::Row(Side::Container, 2)));
        // Past the list's clip (its bottom at y 410) nothing takes the pointer, though rows stand
        // there: twenty rows reach far below it [observed: check 80].
        let past = under(&ui, &st, (600.0, 420.0));
        assert!(past.is_none_or(|t| t == ContainerTile::Other), "{past:?}");
    }

    #[test]
    fn clicks_by_tile() {
        let (mut ui, mut st) = (menu(), ContainerTiles::default());
        frame(&mut ui, &snapshot(&[(1, "One")], &[(2, "Two")], Side::Player, 0), &mut st);
        let of = |ui: &Ui, st: &ContainerTiles, path: &str| st.clicked(ui, ui.find_tile(path).unwrap());
        assert_eq!(of(&ui, &st, &format!("{TAKE_ALL}/button_text")), Some(ContainerTile::TakeAll));
        assert_eq!(of(&ui, &st, EXIT), Some(ContainerTile::Exit));
        assert_eq!(of(&ui, &st, "ContainerMenu/NOGLOW_BRANCH/CM_ItemsRect/CM_Items_RightFilterArrow"), Some(ContainerTile::Arrow(Side::Player, 1)));
        assert_eq!(of(&ui, &st, TITLES[0]), Some(ContainerTile::Title(Side::Container)));
        assert_eq!(of(&ui, &st, &format!("{}/lb_scrollbar/scrollbar_vert_page_down", LISTS[1])), Some(ContainerTile::Bar(Side::Player, list::BarPiece::PageDown)));
        assert_eq!(of(&ui, &st, &format!("{}/lb_scrollbar", LISTS[1])), Some(ContainerTile::Other));
        // Not tiled: a queued click acts on nothing.
        st.menu = None;
        assert_eq!(of(&ui, &st, EXIT), None);
    }

    #[test]
    fn missing_template_child_falls_back() {
        assert_eq!(built(&menu()), Tiled::Yes);
        for (name, missing) in [
            ("CM_ExitButton", EXIT),
            ("CM_Items_CapsLabel", WEIGHT),
            ("CM_ArrowIcon", ARROW),
            ("CM_ItemsTitle", TITLES[1]),
        ] {
            let ui = menu_changed(|xml| xml.replace(&format!("name=\"{name}\""), "name=\"Modded\""));
            assert_eq!(built(&ui), Tiled::No(format!("{missing} missing")));
        }
        let ui = menu_changed(|xml| xml.replace("name=\"scrollbar_vert_page_up\"", "name=\"Modded\""));
        assert!(matches!(built(&ui), Tiled::No(reason) if reason.contains("scrollbar_vert_page_up")));
        let ui = menu_changed(|xml| xml.replace("name=\"WeightInfo\"", "name=\"Modded\""));
        assert!(matches!(built(&ui), Tiled::No(reason) if reason.contains("WeightInfo")));
        // A card's words and the condition's bar.
        for piece in ["ConditionMeter", "MeterImage", "CI_TitleText", "CI_ValueText"] {
            let ui = menu_changed(|xml| xml.replace(&format!("name=\"{piece}\""), "name=\"Modded\""));
            assert!(matches!(built(&ui), Tiled::No(reason) if reason.contains(piece)), "{piece}");
        }
        // Each card's, not only the first's.
        let ui = menu_changed(|xml| xml.replace(r#"<rect name="AmmoInfo"><_Title>AmmoInfo</_Title><_Value></_Value><visible>&false;</visible><text name="CI_TitleText">"#, r#"<rect name="AmmoInfo"><_Title>AmmoInfo</_Title><_Value></_Value><visible>&false;</visible><text name="Modded">"#));
        assert!(matches!(built(&ui), Tiled::No(reason) if reason.contains("AmmoInfo/CI_TitleText")));
        let ui = menu_changed(|xml| xml.replace("name=\"button_text\"", "name=\"Modded\""));
        assert!(matches!(built(&ui), Tiled::No(reason) if reason.contains("button_text")));
        let ui = menu_changed(|xml| xml.replace("name=\"ListItemText\"", "name=\"Modded\""));
        assert_eq!(built(&ui), Tiled::No("CM_list_template/CM_list_template_container/ListItemText missing".into()));
        let ui = menu_changed(|xml| xml.replace("name=\"CM_list_template_ItemMarker\"", "name=\"Modded\""));
        assert!(matches!(built(&ui), Tiled::No(reason) if reason.contains("CM_list_template_ItemMarker")));
        let ui = menu_changed(|xml| xml.replace("name=\"CM_list_template\"", "name=\"Modded\""));
        assert_eq!(built(&ui), Tiled::No("CM_list_template missing".into()));
    }

    /// `rows` one-line rows with `above` others before them, a list 340 high at 40 a row (eight
    /// fit); the last row selected, by keys, the same key whatever comes above it.
    fn long_list(above: u32, rows: u32) -> Snapshot {
        let names: Vec<(u32, String)> = (0..above).map(|i| 1000 + i).chain(1..=rows).map(|i| (i, format!("Row {i}"))).collect();
        let left: Vec<(u32, &str)> = names.iter().map(|(i, n)| (*i, n.as_str())).collect();
        snapshot(&left, &[(100, "Mug")], Side::Player, left.len() - 1)
    }

    #[test]
    fn the_selected_row_stays_shown_as_the_rows_move_under_it() {
        let mut ui = menu();
        let mut st = ContainerTiles::default();
        let s = long_list(0, 20);
        frame(&mut ui, &s, &mut st);
        assert_selected_shown(&ui, Side::Player);
        // The bar moves the list freely while nothing else changes.
        st.scroll_to(Side::Player, 0.0);
        frame(&mut ui, &s, &mut st);
        assert_eq!(st.value(Side::Player), 0.0);
        // The same row, moved by rows coming above it: back in view, by keys.
        frame(&mut ui, &long_list(4, 20), &mut st);
        assert_selected_shown(&ui, Side::Player);
        // With the pointer's pick the list stands still, whatever moves.
        st.scroll_to(Side::Player, 0.0);
        let mut pointer = long_list(8, 20);
        pointer.pointer_pick = true;
        frame(&mut ui, &pointer, &mut st);
        assert_eq!(st.value(Side::Player), 0.0, "hover does not scroll");
    }

    #[test]
    fn a_key_on_the_same_row_brings_it_back() {
        let mut ui = menu();
        let mut st = ContainerTiles::default();
        let mut s = long_list(0, 20);
        frame(&mut ui, &s, &mut st);
        st.scroll_to(Side::Player, 0.0);
        frame(&mut ui, &s, &mut st);
        assert_eq!(st.value(Side::Player), 0.0);
        s.navigated += 1;
        frame(&mut ui, &s, &mut st);
        assert_selected_shown(&ui, Side::Player);
    }

    #[test]
    fn a_wrap_below_the_selected_row_keeps_it_shown() {
        // Twenty rows 10 high in a list 70 high, the last 70 high and then 10: the row and the
        // list stand still, what a value moves does not.
        let mut ui = menu_changed(|xml| xml.replace("<height>340</height>", "<height>70</height>").replace("<_VerticalSpacing>10</_VerticalSpacing>", "<_VerticalSpacing>0</_VerticalSpacing>"));
        let mut st = ContainerTiles::default();
        let size = |_: f64, text: &str, _: Option<f64>| (100.0, if text == "Tall" { 70.0 } else { 10.0 });
        let rows = |last: &'static str| -> Vec<(u32, &'static str)> { (1..=20).map(|i| (i, if i == 20 { last } else { "Short" })).collect() };
        let left = LISTS[Side::Player as usize];
        frame_sized(&mut ui, &snapshot(&rows("Tall"), &[], Side::Player, 11), &mut st, &size);
        assert!((at(&ui, &format!("{left}/_scroll_delta")) - 190.0 / 15.0).abs() < 1e-9);
        assert_eq!(st.value(Side::Player), 4.0);
        assert_selected_shown(&ui, Side::Player);
        frame_sized(&mut ui, &snapshot(&rows("Short"), &[], Side::Player, 11), &mut st, &size);
        assert_eq!(at(&ui, &format!("{left}/_scroll_delta")), 10.0);
        assert_eq!(st.value(Side::Player), 5.0, "the scale fell: value 4 would leave the row at y 70");
        assert_selected_shown(&ui, Side::Player);
    }

    #[test]
    fn a_list_that_comes_to_fit_keeps_its_delta() {
        let mut ui = menu();
        let mut st = ContainerTiles::default();
        let left = "ContainerMenu/NOGLOW_BRANCH/CM_ItemsRect/CM_Items_InventoryList";
        frame(&mut ui, &snapshot(&[(1, "A")], &[], Side::Player, 0), &mut st);
        assert_eq!(at(&ui, &format!("{left}/_scroll_delta")), OWN_DELTA, "the layout's, before any overflow");
        frame(&mut ui, &long_list(0, 20), &mut st);
        let long = at(&ui, &format!("{left}/_scroll_delta"));
        assert!(long != OWN_DELTA);
        // The filter leaves one row: nothing to scroll, the delta as it was, filtered counted.
        let mut one = snapshot(&[(1, "A")], &[], Side::Player, 0);
        one.lists[Side::Player as usize].filtered = 19;
        frame(&mut ui, &one, &mut st);
        assert_eq!(at(&ui, &format!("{left}/_scroll_delta")), long);
        assert_eq!(at(&ui, &format!("{left}/lb_scrollbar/_number_of_items")), 1.0);
        assert_eq!(st.value(Side::Player), 0.0);
        assert_eq!(at(&ui, &format!("{left}/_num_filtered")), 19.0);
    }

    #[test]
    fn each_measuring_round_sees_the_tree_worked_out_and_a_zero_measure_is_kept() {
        // A computed trait as a mod's computed font would read what the model just wrote.
        let mut ui = menu_changed(|xml| xml.replace(r#"<text name="CM_ItemsTitle">"#, r#"<text name="CM_ItemsTitle"><_echo><copy src="me()" trait="string"/></_echo>"#));
        let mut st = ContainerTiles::default();
        let s = snapshot(&[(1, "A")], &[], Side::Player, 0);
        let menu = ui.menu(CONTAINER_MENU).unwrap();
        st.menu = Some(menu);
        assert!(st.is_new(menu, &s));
        write(&mut ui, &s, &mut st);
        let texts = super::super::menus::texts(&ui, menu);
        let echo = format!("{}/_echo", TITLES[1]);
        let mut first_round = None;
        // Everything measures 0 x 0: written and kept, as any other measure.
        let asked = std::cell::Cell::new(0);
        let nothing = |_: f64, _: &str, _: Option<f64>| {
            asked.set(asked.get() + 1);
            (0.0, 0.0)
        };
        settle(&mut ui, &s, &mut st, &mut |ui: &mut Ui, sizes: &mut SizesWritten| {
            first_round.get_or_insert_with(|| ui.get_string(&echo));
            super::super::menus::write_text_sizes_of(ui, &texts, sizes, &nothing)
        });
        assert_eq!(first_round.flatten().as_deref(), Some("ITEMS"));
        let first = asked.get();
        assert!(first > 0);
        let row = list::child(&ui, st.rows[Side::Player as usize][0].tile, ROW_TEXT).unwrap();
        assert_eq!(ui.tile(row).get("height").map(Value::num), Some(0.0));
        assert!(st.sizes.contains_key(&row));
        // The same string, font and wrap are not measured again.
        settle(&mut ui, &s, &mut st, &mut |ui: &mut Ui, sizes: &mut SizesWritten| super::super::menus::write_text_sizes_of(ui, &texts, sizes, &nothing));
        assert_eq!(asked.get(), first);
        settle(&mut ui, &s, &mut st, &mut |ui: &mut Ui, sizes: &mut SizesWritten| super::super::menus::write_text_sizes_of(ui, &texts, sizes, &list::lines_size));
        assert_eq!(ui.tile(row).get("height").map(Value::num), Some(0.0), "not taken from another measure");
        // Another string is.
        let s = snapshot(&[(1, "B")], &[], Side::Player, 0);
        assert!(st.is_new(menu, &s));
        write(&mut ui, &s, &mut st);
        settle(&mut ui, &s, &mut st, &mut |ui: &mut Ui, sizes: &mut SizesWritten| super::super::menus::write_text_sizes_of(ui, &texts, sizes, &list::lines_size));
        assert_eq!(ui.tile(row).get("height").map(Value::num), Some(30.0));
    }

    #[test]
    fn reopen_discards_old_rows() {
        let (mut ui, mut st) = (menu(), ContainerTiles::default());
        let s = snapshot(&[(1, "One")], &[(2, "Two")], Side::Player, 0);
        frame(&mut ui, &s, &mut st);
        let old = st.rows[1][0].tile;
        // The menu built again (a box over it dropped it): the same snapshot is written anew.
        let menu_tile = ui.menu(CONTAINER_MENU).unwrap();
        ui.unload_tile(menu_tile);
        let mut ui = menu();
        frame(&mut ui, &s, &mut st);
        assert_eq!(st.rows[1].len(), 1);
        assert_ne!(st.rows[1][0].tile, old);
        assert!(ui.is_live(st.rows[1][0].tile));
        // Closed: nothing stays.
        st.drag = Some(Drag { menu: menu_tile, list: menu_tile, bar: menu_tile, top: 0.0, grab: 0.0 });
        st.scroll_to(Side::Player, 1.0);
        st.warned = true;
        st.close();
        assert!(st.menu.is_none() && st.drag.is_none() && st.shown().is_none() && st.rows[1].is_empty());
        assert!(st.warned, "the warning is once a session");
    }

    #[test]
    fn stationary_pointer_does_not_undo_wheel() {
        // A list moved under a resting pointer: its new row does not take the selection.
        assert!(!hover_selects(false, false, false));
        // A held wheel, the pointer jittering.
        assert!(!hover_selects(true, true, false));
        // The pointer travels onto another row.
        assert!(hover_selects(true, false, false));
        assert!(!hover_selects(true, false, true), "already selected");
    }

    #[test]
    fn quantity_blocks_container_input() {
        let (ui, _) = (menu(), ());
        let m = ui.menu(CONTAINER_MENU);
        assert_eq!(takes_input(m, false), m);
        assert_eq!(takes_input(m, true), None);
        assert_eq!(takes_input(None, false), None);
    }

    #[test]
    fn physical_a_focuses_without_taking_all() {
        use super::super::menus::key_click_acts;
        // The file binds A and E: from the keyboard they are the hand-built keys, a script's
        // MenuTapKey (not physical) acts through the file.
        assert!(!key_click_acts(CONTAINER_MENU, "A", &["A"]));
        assert!(key_click_acts(CONTAINER_MENU, "A", &[]));
        assert!(!key_click_acts(CONTAINER_MENU, "E", &["E"]));
        assert!(key_click_acts(CONTAINER_MENU, "E", &[]));
    }

    #[test]
    fn a_row_moved_in_the_model_acts_on_nothing() {
        let (mut ui, mut st) = (menu(), ContainerTiles::default());
        frame(&mut ui, &snapshot(&[(1, "One"), (2, "Two")], &[], Side::Player, 0), &mut st);
        let now = |items: &[u32]| -> Vec<RowShown> { items.iter().map(|&i| row(i, "x")).collect() };
        assert_eq!(st.row_in(Side::Player, 1, &now(&[1, 2])), Some(1), "as the tree shows it");
        assert_eq!(st.row_in(Side::Player, 1, &now(&[1, 3])), None, "another item there");
        assert_eq!(st.row_in(Side::Player, 1, &now(&[1])), None, "the list shorter");
        assert_eq!(st.row_in(Side::Container, 0, &now(&[1])), None, "no such tile");
    }

    #[test]
    fn ysi_row_icons_attach_when_ysi_xml_is_present_and_noop_when_absent() {
        let tagged = fnv_game::ui_ysi::YsiItemDisplay {
            tag: Some("(a01)".into()),
            icon: Some("icons\\stimpak.dds".into()),
            template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
        };
        let mut s = snapshot(&[(1, "Stimpak"), (2, "Tin Can")], &[], Side::Player, 0);
        s.lists[Side::Player as usize].rows[0].ysi = Some(tagged);

        // Without ySI.xml: safe fallback, zero ySIImage tiles created.
        let (mut plain_ui, mut plain_st) = (menu(), ContainerTiles::default());
        frame(&mut plain_ui, &s, &mut plain_st);
        let row0 = plain_st.rows[Side::Player as usize][0].tile;
        assert!(list::child(&plain_ui, row0, fnv_game::ui_ysi::YSI_IMAGE_TILE).is_none());

        // With ySI.xml injected into ContainerMenu: tagged row gets ySIDefault + filename,
        // untagged row gets ySINoTag, and clicking ySIImage still maps to Row(0).
        let ysi_xml = r#"
<rect name="ySI">
  <_ySI> 1 </_ySI>
  <_ySIBlanks> 1 </_ySIBlanks>
  <_ySIWidth> 24 </_ySIWidth>
  <_ySIHeight> 24 </_ySIHeight>
</rect>
<template name="ySIDefault">
  <image name="ySIImage">
    <filename> icons\default.dds </filename>
    <width> <copy src="ySI" trait="_ySIWidth"/> </width>
    <height> <copy src="ySI" trait="_ySIHeight"/> </height>
    <visible> 1 </visible>
  </image>
</template>
<template name="ySINoTag">
  <image name="ySIImage">
    <filename> icons\blank.dds </filename>
    <width> <copy src="ySI" trait="_ySIWidth"/> <onlyif src="ySI" trait="_ySIBlanks"/> </width>
    <height> <copy src="ySI" trait="_ySIHeight"/> </height>
    <visible> 1 </visible>
  </image>
</template>
"#;
        let (mut ui, mut st) = (menu(), ContainerTiles::default());
        let src = MemSource::default().with(fnv_game::ui_ysi::YSI_XML_PATH, ysi_xml);
        let root = ui.menu(CONTAINER_MENU).unwrap();
        assert!(fnv_game::ui_ysi::publish_for_menu(&mut ui, &src, root, Some(1008)));
        frame(&mut ui, &s, &mut st);

        let r0_tile = st.rows[Side::Player as usize][0].tile;
        let r1_tile = st.rows[Side::Player as usize][1].tile;
        let img0 = list::child(&ui, r0_tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).expect("tagged row icon");
        let img1 = list::child(&ui, r1_tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).expect("untagged row icon");
        assert_eq!(ui.tile(img0).get("filename").map(Value::text).as_deref(), Some("icons\\stimpak.dds"));
        assert_eq!(ui.tile(img0).get("width").map(Value::num), Some(24.0));
        assert_eq!(ui.tile(img1).get("filename").map(Value::text).as_deref(), Some("icons\\blank.dds"));
        assert_eq!(st.clicked(&ui, img0), Some(ContainerTile::Row(Side::Player, 0)));

        // Disabling bAddInventoryIcons (ysi_icons = false) unloads existing ySIImage tiles
        // and never instantiates ySINoTag even while ContainerMenu/ySI remains present.
        let mut s_off = s.clone();
        s_off.ysi_icons = false;
        s_off.lists[Side::Player as usize].rows[0].ysi = None;
        frame(&mut ui, &s_off, &mut st);
        assert!(ui.find_tile("ContainerMenu/ySI").is_some(), "ySI tile stays readable by scripts");
        assert!(list::child(&ui, st.rows[Side::Player as usize][0].tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).is_none());
        assert!(list::child(&ui, st.rows[Side::Player as usize][1].tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).is_none());

        // Fresh menu with ySI.xml present and ysi_icons = false creates zero ySIImage tiles.
        let (mut ui_off, mut st_off) = (menu(), ContainerTiles::default());
        let root_off = ui_off.menu(CONTAINER_MENU).unwrap();
        assert!(fnv_game::ui_ysi::publish_for_menu(&mut ui_off, &src, root_off, Some(1008)));
        frame(&mut ui_off, &s_off, &mut st_off);
        assert!(ui_off.find_tile("ContainerMenu/ySI").is_some());
        for row in &st_off.rows[Side::Player as usize] {
            assert!(
                list::child(&ui_off, row.tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).is_none(),
                "bAddInventoryIcons=0 must not instantiate ySIImage/ySINoTag tiles"
            );
        }
    }
}
