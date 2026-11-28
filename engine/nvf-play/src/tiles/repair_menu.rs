//! The Pip-Boy's repair page drawn from `menus\main\repair_menu.xml`, behind
//! `[UI] TileMenus=RepairMenu` [design: repair.md §5.1; the way of `super::stats_menu`; issue
//! #1468]. Off (the default), none of this runs and `crate::pipboy::repair` draws its list in the
//! ITEMS page's body.
//!
//! - **The one decision** ([`RepairTiles::ready`]): the switch on, the repair list open on ITEMS,
//!   the menu's code 1035 up and its tree built, the file's tiles there ([`built`]) and a usable
//!   measured layout ([`usable`]). Painting (`super::draw`), hand-built suppression
//!   (`crate::pipboy::frame::draw`) and input read it and nothing else, and [`settle`] publishes
//!   it each frame before any of them. A pass that fails leaves the hand-built list, says why
//!   once, and is tried again the next frame. The tree is built the frame after the code is set
//!   (`fnv_game::ui_loading::sync`), so the hand-built list stands for the frame the page opens in.
//! - **The rules stay `crate::pipboy::repair`'s**: what is listed, what a part does, when the list
//!   closes, its sounds and log lines. This module writes the [`crate::pipboy::repair::Session`] as
//!   the file's "set in code" traits, and sends [`RepairInput`], as the hand-built rows do. The
//!   keys stay the model's ([`HAND_BUILT_KEYS`]). Only the list's scroll is its own.
//! - **Off any row** [observed: private job run07c_repair_menu, 1280x720]: the original opens with
//!   no row hovered, the highlight hidden and only the item's own cards. The model always has a
//!   row chosen, so that state is the item's row chosen with the pointer on none
//!   ([`RepairTiles::on_rows`]); a pointer leaving the rows chooses the item again, so that Enter
//!   then uses nothing, as on the item's row [observed: the same job, row 0 does not act].
//! - **The three states** the same job read: off any row the broken cards alone; on the item's
//!   row those, the arrow and the choose text; on a part those, the arrow, the fixed cards and the
//!   gains. The brackets stand on the item's row in all three ([`write_brackets`]). What follows a
//!   repair on a part was not seen [hypothesis]: the model's rule stands.
//! - **Page content only**, as `super::stats_menu`: the page paints into the Pip-Boy's image, whose
//!   background and scanlines are the shell's, so `RM_Background` stays hidden in both frames. The
//!   screen menu has no title and its heading line runs on (`super::stats_menu::write_frame_of`).
//! - **Input** ([`feed`], before the scripts): the pointer is the Pip-Boy's own, in canvas units
//!   (`crate::pipboy::PipBoyScreen::pointer`), acting on the page painted last frame.
//! - **The words** (repair.md §5.1, each with its source): the file's settings, a record's name, a
//!   number, and the one line of NVForever's own, [`CHOOSE_WORDS`].

use super::draw::{FontKit, Look, Piece};
use super::input::{frame_keys, log_report, Devices};
use super::item_menu::{on, set};
use super::list::{self, Drag, ListView, RowTile, Wanted};
use super::menus::{self, SizesWritten, Tiled};
use super::repair_cards::{self, Card, Figures};
use super::stats_menu::{modal_over, shows_whole, write_frame_of, FlatChanges, PageFonts};
use super::vanilla::TileMenus;
use crate::pipboy::repair::{PipRepair, RepairInput, Session};
use crate::pipboy::{Frame, Page, PipBoy, PipBoyAid, PipBoyScreen};
use crate::script::ScriptState;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use fnv_game::game::menus as codes;
use fnv_game::repair::{gain_percent, merged_condition, Gear};
use fnv_sim::inventory::Extra;
use fnv_sim::FormId;
use fnv_ui::{TileId, Ui, Value};
use std::collections::{HashMap, HashSet};

/// [documented: repair_menu.xml] The menu and the paths written by this adapter.
pub const REPAIR_MENU: &str = "RepairMenu";
const BACKGROUND: &str = "RepairMenu/GLOW_BRANCH/RM_Background";
const TITLE: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_HeadlineRect/RM_Headline_Title";
const LINE: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_HeadlineRect/RM_Headline_h2";
const SKILL: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_HeadlineRect/RM_Headline_PlayerSkillInfo";
const LIST: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_RepairList";
const LEFT_BRACKET: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_RepairList/RM_LeftBracket";
const RIGHT_BRACKET: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_RepairList/RM_RightBracket";
const ICON: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_ItemIcon";
const COMPARE: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_CompareRect";
const CHOOSE: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_CompareRect/RM_ChooseItemText";
const REPAIR: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_ButtonRect/RM_RepairButton";
const CANCEL: &str = "RepairMenu/GLOW_BRANCH/RM_MainRect/RM_ButtonRect/RM_CancelButton";
/// The cards' names under [`COMPARE`], for `super::repair_cards`.
const PREFIX: &str = "RM_";
const LIST_NAME: &str = "RM_RepairList";
const TEMPLATE: &str = "RM_RepairListTemplate";
const ROW: &str = "RM_RepairListTemplateRect";
const ROW_TEXT: &str = "ListItemText";
const ROW_METER_BACK: &str = "RM_Template_MeterBackground";
const ROW_METER: &str = "RM_Template_Meter";
const ROW_MARK: &str = "RM_Template_ItemMarker";
/// The square of an equipped row where the file names no picture [design: the hand-built list's,
/// `crate::pipboy::repair`].
const MARK_PICTURE: &str = "interface\\shared\\marker\\square_filled.dds";

/// The line while the item's own row is hovered [design: LEGAL.md; the original's are in its code,
/// so these are NVForever's].
pub const CHOOSE_WORDS: &str = "Choose a part to use.";
/// The choose text's `alpha` pulses from this to full [observed: private job run07c_repair_menu,
/// 1280x720, 64 to 252 at different reads; the file gives no such thing].
const PULSE_LOW: f64 = 64.0;
/// Its period in seconds [hypothesis: the reads do not give it].
const PULSE_SECONDS: f64 = 2.0;
/// Final passes the page waits for its tree before the hand-built list has the pointer: it is
/// built the frame after the code is set, unless the file cannot be built [design].
const TREE_WAIT: u8 = 2;

/// [design: `crate::pipboy::repair::keys`, `crate::pipboy::keys`, `crate::pad::menu_buttons`] What
/// the model takes itself, by the tile input's names: R, W, S, E, the Pip-Boy's A and D, the
/// arrows, and the controller's A (Enter), B (Esc) and triggers. A script's MenuTapKey still follows
/// the file's `_PCButton_E`.
pub const HAND_BUILT_KEYS: [&str; 14] = ["R", "W", "S", "E", "A", "D", "xup", "xdown", "xleft", "xright", "xbuttona", "xbuttonb", "xbuttonlt", "xbuttonrt"];

/// What a row stands for: an item, its extra, and whether it is the item being repaired (a part
/// of the same kind at the same condition is another row).
pub type RowKey = (FormId, Extra, bool);

/// [design] A short or restructured file keeps the hand-built list.
pub fn built(ui: &Ui) -> Tiled {
    let mut paths: Vec<String> = [REPAIR_MENU, BACKGROUND, TITLE, LINE, SKILL, LIST, LEFT_BRACKET, RIGHT_BRACKET, ICON, COMPARE, CHOOSE, REPAIR, CANCEL].into_iter().map(str::to_owned).collect();
    for button in [REPAIR, CANCEL] {
        paths.extend(["button_text", "PCShortcutLabel"].map(|t| format!("{button}/{t}")));
    }
    paths.extend(super::item_card::CARD_TEXTS.map(|t| format!("{SKILL}/{t}")));
    paths.push(format!("{LIST}/{}", list::HIGHLIGHT));
    for piece in [list::MARKER, list::UP, list::DOWN, list::PAGE_UP, list::PAGE_DOWN] {
        paths.push(format!("{LIST}/{}/{piece}", list::SCROLLBAR));
    }
    paths.extend(repair_cards::paths(COMPARE, PREFIX));
    for path in &paths {
        if ui.find_tile(path).is_none() {
            return Tiled::No(format!("{path} missing"));
        }
    }
    list::template_has(ui, REPAIR_MENU, TEMPLATE, ROW, &[("text", ROW_TEXT), ("image", ROW_METER_BACK), ("image", ROW_METER), ("image", ROW_MARK)])
}

/// The settings a frame's words read, through the strings table.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Words {
    pub skill: String,
    pub condition: String,
    pub damage: String,
    pub threshold: String,
    /// `sPCMenuHintE` as the device names it.
    pub cancel_hint: String,
}

impl Words {
    fn read(game: &fnv_game::Game<'_>) -> Self {
        let word = |key| fnv_game::ui_loading::game_string(game, key);
        Words {
            skill: word("sRepairSkill"),
            condition: word("sInventoryCondition"),
            damage: word("sInventoryDamage"),
            threshold: word("sInventoryDamageThreshold"),
            cancel_hint: crate::pad::game_hint(&word("sPCMenuHintE"), &[KeyCode::KeyE, KeyCode::Escape], crate::pad::at_menu(codes::REPAIR)),
        }
    }
}

/// A row as shown.
#[derive(Clone, Debug, PartialEq)]
pub struct RowShown {
    pub key: RowKey,
    pub text: String,
    pub equipped: bool,
    pub condition: f32,
    /// The item's condition after using this row [observed: private job run07c_repair_menu,
    /// 1280x720, `user0` read, each value `merged_condition`'s; the item's own row as if merged
    /// with itself].
    pub after: f32,
    /// Resolved ySI tag/icon/template for this row when ySI is active (`None` otherwise).
    pub ysi: Option<fnv_game::ui_ysi::YsiItemDisplay>,
}

/// One whole state of the page: a frame with the same writes nothing but sizes and scroll. A
/// plain value, so tests need no world.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub rows: Vec<RowShown>,
    /// The row with the highlight: none with the pointer off the rows on the item's row.
    pub highlight: Option<usize>,
    /// The selection was last moved by the pointer: the list stays where it is.
    pub pointer_pick: bool,
    /// A count of the keys' moves.
    pub navigated: u32,
    /// The skill's figure on its card, and the setting for a card whose file gives no title.
    pub skill: String,
    pub skill_title: String,
    pub icon: Option<String>,
    pub figures: Figures,
    /// The item's own row is hovered.
    pub choose: bool,
    pub cancel_hint: String,
    /// Whether `apply_row_icon` should instantiate `ySIImage` row tiles (`yUI.ini`
    /// `[Sorting and Icons] bAddInventoryIcons`, gated by `bSortingIcons`).
    pub ysi_icons: bool,
}

impl Shown {
    /// `stat_at`: the figure at a condition, as the hand-built list's (`crate::pipboy::repair::stat`).
    pub(crate) fn of(s: &Session, on_rows: bool, pointer_pick: bool, navigated: u32, stat_at: &dyn Fn(f32) -> Option<(&'static str, i32)>, w: &Words) -> Self {
        let o = &s.offer;
        let merged = |with: f32| merged_condition(&s.settings, o.condition, with, s.skill);
        let mut rows = vec![RowShown { key: (o.item, o.extra, true), text: o.name.clone(), equipped: o.extra.equipped, condition: o.condition, after: merged(o.condition), ysi: None }];
        rows.extend(o.parts.iter().map(|q| RowShown { key: (q.item, q.extra, false), text: q.name.clone(), equipped: q.extra.equipped, condition: q.condition, after: s.after(q), ysi: None }));
        let part = s.selected.checked_sub(1).and_then(|i| o.parts.get(i));
        let to = part.map(|q| s.after(q));
        let now = stat_at(o.condition);
        // The card's title by the item's kind, not the figure's label: since #1902 that is
        // the load order's (localized) setting, no longer "DAM" [design].
        let stat_title = match (now, o.gear) {
            (None, _) => String::new(),
            (Some(_), Gear::Weapon) => w.damage.clone(),
            (Some(_), Gear::Armor) => w.threshold.clone(),
        };
        let figures = Figures {
            condition_title: w.condition.clone(),
            stat_title,
            now: Card { condition: o.condition, stat: now.map(|s| s.1) },
            after: to.map(|to| Card { condition: to, stat: stat_at(to).map(|s| s.1) }),
            gain_percent: to.map_or(0, |to| gain_percent(o.condition, to)),
            gain_stat: to.and_then(|to| stat_at(to).zip(now).map(|(a, b)| a.1 - b.1)),
        };
        let on_item = s.selected == 0 && on_rows;
        Shown {
            rows,
            highlight: (s.selected != 0 || on_item).then_some(s.selected),
            pointer_pick,
            navigated,
            skill: format!("{:.0}", s.skill.max(0.0).floor()),
            skill_title: w.skill.clone(),
            icon: o.icon.clone(),
            figures,
            choose: on_item,
            cancel_hint: w.cancel_hint.clone(),
            ysi_icons: false,
        }
    }
}

/// What a click on the page's tiles stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairTile {
    Row(usize),
    Repair,
    Cancel,
    Bar(list::BarPiece),
    Other,
}

const TARGETS: &[(&str, RepairTile)] = &[("RM_RepairButton", RepairTile::Repair), ("RM_CancelButton", RepairTile::Cancel)];

/// What was written, and what the page keeps between frames. A new menu tile (the tree built
/// again) starts over.
#[derive(Default)]
struct Written {
    menu: Option<TileId>,
    rows: Vec<RowTile<RowKey>>,
    view: ListView<RowKey>,
    shown: Option<Shown>,
    sizes: SizesWritten,
    pictures: HashMap<String, Option<(f64, f64)>>,
    flat: Option<FlatChanges>,
    /// What is shown changed since the log last said so.
    tell: bool,
    /// The scroll last logged as drawn.
    drawn: Option<f64>,
}

/// The decision every reader shares, and what the page keeps.
#[derive(Resource, Default)]
pub struct RepairTiles {
    /// The menu, while the page is ready to display this frame ([`settle`]).
    pub ready: Option<TileId>,
    /// The menu painted by the last drawing (`super::draw`): what the player sees.
    pub shown: Option<TileId>,
    /// The pieces [`usable`] checked this frame, for the drawing to paint.
    pub(crate) pieces: Vec<Piece>,
    /// A menu over the page when this frame began: set before the scripts, so it holds through
    /// the frame that menu closes in (as `super::stats_menu::StatsTiles::modal`).
    pub modal: bool,
    /// Another menu took this frame's tile input (`super::input::feed`).
    pub others: bool,
    /// The list is open and the page has not decided yet: the hand-built list that stands
    /// meanwhile takes no hover, so a pointer at rest cannot choose a part before the page opens
    /// (`crate::pipboy::repair`).
    pub waiting: bool,
    /// Final passes with no tree yet.
    waited: u8,
    /// The pointer is on a row, or the keys have left the item's row (module notes).
    on_rows: bool,
    /// The row the pointer last asked for, to know the keys' moves from its.
    asked: Option<usize>,
    /// The model's selection last seen, the last move's being the pointer's, and the keys' count.
    selected: Option<usize>,
    pointer_pick: bool,
    navigated: u32,
    /// A press on the scroll marker, held.
    drag: Option<Drag>,
    written: Written,
    fallbacks: HashSet<String>,
}

impl RepairTiles {
    /// The list closed: what the next opening starts from. The tree goes with it.
    fn closed(&mut self) {
        self.waiting = false;
        self.waited = 0;
        self.on_rows = false;
        self.asked = None;
        self.selected = None;
        self.pointer_pick = false;
        self.drag = None;
    }

    /// A final pass that found no tree for the open list.
    fn no_tree_yet(&mut self) {
        self.waited = self.waited.saturating_add(1);
        self.waiting = self.waited <= TREE_WAIT;
    }

    /// The model's selection now: who moved it. A change onto a part puts the page on the rows;
    /// an unchanged one says nothing, as the pointer may have just left them and its `Hover(0)`
    /// not been applied yet.
    fn follow_model(&mut self, selected: usize) {
        if self.selected != Some(selected) {
            if self.asked == Some(selected) {
                self.pointer_pick = true;
            } else {
                self.pointer_pick = false;
                self.navigated = self.navigated.wrapping_add(1);
            }
            self.selected = Some(selected);
            if selected != 0 {
                self.on_rows = true;
            }
        }
    }

    /// The tile a click on `tile` stands for, on the page as written.
    pub fn clicked(&self, ui: &Ui, tile: TileId) -> Option<RepairTile> {
        let menu = self.written.menu?;
        if !ui.is_live(tile) || ui.root_of(tile) != menu {
            return None;
        }
        Some(match list::hit(ui, tile, &[(LIST_NAME, self.written.rows.as_slice())], TARGETS) {
            list::Hit::Row(_, index) => RepairTile::Row(index),
            list::Hit::Bar(_, piece) => RepairTile::Bar(piece),
            list::Hit::Target(what) => what,
            list::Hit::Other => RepairTile::Other,
        })
    }

    /// `index` when the row the tree shows there is still the row shown there.
    pub fn row_at(&self, index: usize) -> Option<usize> {
        let tile_key = self.written.rows.get(index)?.key;
        (self.written.shown.as_ref()?.rows.get(index)?.key == tile_key).then_some(index)
    }
}

/// [design] File places and textures stay in the tree; what the file leaves to code is written.
/// Nothing writes the headline's title or the buttons' words: those are the file's settings.
fn write(ui: &mut Ui, s: &Shown, w: &mut Written) {
    let names: Vec<String> = s.rows.iter().map(|r| r.text.clone()).collect();
    let wanted: Vec<Wanted<'_, RowKey>> = s.rows.iter().zip(&names).map(|(r, name)| Wanted { key: &r.key, name }).collect();
    list::reconcile(ui, LIST, TEMPLATE, &mut w.rows, &wanted);
    for (row, shown) in w.rows.iter().zip(&s.rows) {
        // [documented: the file names these for code; observed: private job run07c_repair_menu,
        // 1280x720, `user1` 0 with the skill over 100, its use the file's comment only].
        let is_item = shown.key.2;
        ui.set_trait(row.tile, "_IsRepairItem", on(is_item));
        ui.set_trait(row.tile, "user0", Value::Num(f64::from(shown.after)));
        ui.set_trait(row.tile, "user1", Value::Num(0.0));
        if let Some(t) = list::child(ui, row.tile, ROW_TEXT) {
            ui.set_trait(t, "string", Value::Str(shown.text.clone()));
            // The file leaves it to code, and the fill's and the square's copy it [documented:
            // the file]. A row the skill cannot use was not seen [hypothesis: none dims].
            ui.set_trait(t, "alpha", Value::Num(255.0));
        }
        if let Some(t) = list::child(ui, row.tile, ROW_MARK) {
            ui.set_trait(t, "visible", on(shown.equipped));
            if ui.tile(t).get("filename").is_none_or(|v| v.text().is_empty()) {
                ui.set_trait(t, "filename", Value::Str(MARK_PICTURE.into()));
            }
        }
        if s.ysi_icons {
            fnv_game::ui_ysi::apply_row_icon(ui, &fnv_ui::MemSource::default(), row.tile, shown.ysi.as_ref());
        } else {
            fnv_game::ui_ysi::remove_row_icon(ui, row.tile);
        }
    }
    set(ui, LIST, "_num_filtered", Value::Num(0.0));
    set(ui, LIST, "_enabled", on(true));
    // The page's backing is the Pip-Boy's own (module notes).
    set(ui, BACKGROUND, "visible", on(false));
    repair_cards::write_title_if_none(ui, SKILL, &s.skill_title);
    set(ui, SKILL, "_Value", Value::Str(s.skill.clone()));
    set(ui, ICON, "visible", on(s.icon.is_some()));
    set(ui, ICON, "filename", Value::Str(s.icon.clone().unwrap_or_default()));
    repair_cards::write(ui, COMPARE, PREFIX, Some(&s.figures));
    set(ui, CHOOSE, "visible", on(s.choose));
    set(ui, CHOOSE, "string", Value::Str(if s.choose { CHOOSE_WORDS.into() } else { String::new() }));
    // Repair's `visible` is the file's, for the Xbox: on a PC it stays hidden. Its `target` reads
    // 1 opening and on a part, its line dim on the item's row [documented: the file; observed:
    // private job run07c_repair_menu, 1280x720].
    set(ui, REPAIR, "target", on(!s.choose));
    set(ui, CANCEL, "visible", on(true));
    set(ui, CANCEL, "target", on(true));
    set(ui, CANCEL, "_PCButtonText", Value::Str(s.cancel_hint.clone()));
}

/// The rows' condition bars, as `super::repair_services_menu`'s [documented: the file; observed: the
/// same job, condition x the back's width].
fn write_meters(ui: &mut Ui, rows: &[RowTile<RowKey>], shown: &[RowShown]) {
    for (row, s) in rows.iter().zip(shown) {
        let back = list::child(ui, row.tile, ROW_METER_BACK).and_then(|t| ui.tile(t).get("width")).map_or(0.0, Value::num);
        if let Some(t) = list::child(ui, row.tile, ROW_METER) {
            ui.set_trait(t, "width", Value::Num(f64::from(s.condition.clamp(0.0, 1.0)) * back));
        }
    }
}

/// [design: `pipboy-stats-tabs-1513:engine/nvf-play/src/tiles/stats_menu.rs::write_file_sizes`;
/// `draw::FontKit::picture_size`]
/// Supply the picture dimensions the file can read, including hidden button pictures. Walk the
/// whole page so its icon, brackets, cards and rows get the same writes. Atlas pieces have no own
/// file size. Keep file texels cached, applying the current zoom on every pass.
pub(super) fn write_file_sizes(ui: &mut Ui, menu: TileId, pictures: &mut HashMap<String, Option<(f64, f64)>>, fonts: &mut dyn PageFonts) {
    let mut stack = vec![menu];
    while let Some(t) = stack.pop() {
        let tile = ui.tile(t);
        if !tile.live {
            continue;
        }
        stack.extend(tile.children.iter().copied());
        if tile.kind != "image" || tile.get("texatlas").is_some() {
            continue;
        }
        let name = tile.get("filename").map(|v| v.text().trim().to_string()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let zoom = tile.get("zoom").filter(|v| !matches!(v, Value::Sym(_))).map_or(100.0, Value::num);
        let zoom = if zoom > 0.0 { zoom / 100.0 } else { 1.0 };
        if let Some((width, height)) = *pictures.entry(name.clone()).or_insert_with(|| fonts.picture_texels(&name)) {
            ui.write_changed(t, "filewidth", Value::Num(width * zoom));
            ui.write_changed(t, "fileheight", Value::Num(height * zoom));
        }
    }
}

/// The brackets frame the item's row, whatever is hovered: `y` and `height` are code's, and the
/// height is the row's less one [documented: the file; observed: private job run07c_repair_menu,
/// 1280x720, 51 on a row of 52, y 0; hypothesis: they move with the scroll, no long list was seen].
fn write_brackets(ui: &mut Ui, rows: &[RowTile<RowKey>], value: f64) {
    write_brackets_of(ui, LIST, [LEFT_BRACKET, RIGHT_BRACKET], rows, value);
}

/// [`write_brackets`] for the list `list` and its brackets: on the first row, scrolled with it.
pub(super) fn write_brackets_of<K>(ui: &mut Ui, list: &str, brackets: [&str; 2], rows: &[RowTile<K>], value: f64) {
    let Some(item) = rows.first() else { return };
    let height = ui.tile(item.tile).get("height").map_or(0.0, Value::num);
    let delta = ui.find_tile(list).and_then(|l| ui.tile(l).get("_scroll_delta")).map_or(0.0, Value::num);
    for bracket in brackets {
        set(ui, bracket, "y", Value::Num(-value * delta));
        set(ui, bracket, "height", Value::Num((height - 1.0).max(0.0)));
    }
}

/// The choose text's `alpha` at `secs` (see [`PULSE_LOW`]).
pub fn pulse(secs: f64) -> f64 {
    let wave = 0.5 - 0.5 * (secs * std::f64::consts::TAU / PULSE_SECONDS).cos();
    PULSE_LOW + (255.0 - PULSE_LOW) * wave
}

/// Whether what is about to be painted can be used, on every final pass: a row's words whole in
/// the window and taking the pointer at their middle, for every row that shows whole, and Cancel's.
/// The reason for the hand-built list otherwise.
pub fn usable(ui: &Ui, menu: TileId, tiles: &RepairTiles, pieces: &[Piece]) -> Result<(), String> {
    usable_of(ui, menu, (&tiles.written.rows, ROW_TEXT), CANCEL, pieces, (RepairTile::Row, RepairTile::Cancel), |t| tiles.clicked(ui, t))
}

/// [`usable`] for a page of one list's `rows` (their words the child `row_text`) and a Cancel
/// button `cancel`: `what` names a row's and Cancel's meaning, `clicked` what a tile stands for on
/// the page as written (the weapon mod menu's too, `super::item_mod_menu`).
pub(super) fn usable_of<K, T: Copy + PartialEq + std::fmt::Debug>(ui: &Ui, menu: TileId, (rows, row_text): (&[RowTile<K>], &str), cancel: &str, pieces: &[Piece], what: (fn(usize) -> T, T), clicked: impl Fn(TileId) -> Option<T>) -> Result<(), String> {
    let is_text = |x: &&Piece| matches!(x.look, Look::Text { .. });
    let mut wanted: Vec<(T, (f64, f64))> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let text = list::child(ui, row.tile, row_text);
        if let Some(middle) = pieces.iter().filter(is_text).find(|x| Some(x.tile) == text).and_then(shows_whole) {
            wanted.push(((what.0)(i), middle));
        }
    }
    if wanted.is_empty() {
        return Err("no row shows whole".into());
    }
    // [design: fnv_ui::input::hit_candidates; documented: repair.md §5.1] Cancel is an image,
    // so its children do not inherit its target. Probe its button words explicitly: the shortcut
    // label stands outside the button and may sort before them by depth.
    let cancel_text = ui.find_tile(&format!("{cancel}/button_text"));
    let words = pieces.iter().filter(is_text).find(|x| Some(x.tile) == cancel_text).ok_or("Cancel is not shown")?;
    wanted.push((what.1, shows_whole(words).ok_or("Cancel does not fit")?));
    for (what, at) in wanted {
        let hit = fnv_ui::input::hit(ui, &[menu], at, &fnv_ui::input::no_measure);
        if hit.and_then(|h| clicked(h.tile)) != Some(what) {
            let took = hit.map_or_else(|| "nothing does".into(), |h| format!("{} takes it under {}", ui.path_of(h.tile), ui.path_of(h.under)));
            return Err(format!("{what:?} does not take the pointer: {took}"));
        }
    }
    Ok(())
}

/// A line for the log when what the page shows changes (smoke reads these).
fn describe(ui: &Ui, s: &Shown) -> String {
    let item = s.rows.first();
    format!(
        "tiles: {REPAIR_MENU} drawn from tiles: {} rows; item {:?}; highlight {:?}; choose {}; skill {}; figures: {}",
        s.rows.len(),
        item.map_or("none", |r| r.text.as_str()),
        s.highlight.and_then(|i| s.rows.get(i)).map_or("none", |r| r.text.as_str()),
        s.choose,
        s.skill,
        repair_cards::describe(ui, COMPARE, PREFIX),
    )
}

/// One pass over the page; the menu when all of it went through. `late`: the pass before the
/// drawing; the other, before the scripts, does the sizes only for a model that changed.
pub(crate) fn page(ui: &mut Ui, s: Shown, device: bool, late: bool, secs: f64, tiles: &mut RepairTiles, fonts: &mut dyn PageFonts) -> Result<TileId, String> {
    if let Tiled::No(reason) = built(ui) {
        return Err(reason);
    }
    let menu = ui.menu(REPAIR_MENU).ok_or_else(|| format!("{REPAIR_MENU} is not in the tree"))?;
    let w = &mut tiles.written;
    if w.menu != Some(menu) {
        *w = Written { menu: Some(menu), ..Default::default() };
    }
    let changed = w.shown.as_ref() != Some(&s);
    if changed {
        write(ui, &s, w);
        w.shown = Some(s);
        w.tell = true;
    }
    if changed || late {
        let Some(s) = w.shown.take() else { return Err("nothing written".into()) };
        ui.update();
        write_file_sizes(ui, menu, &mut w.pictures, fonts);
        ui.update();
        write_meters(ui, &w.rows, &s.rows);
        let texts = menus::texts(ui, menu);
        // Three rounds: a size leads to a place and a place to a wrap width [design: as
        // `super::stats_menu::page`].
        for _ in 0..3 {
            if fonts.write_sizes(ui, &texts, &mut w.sizes) == 0 {
                break;
            }
            ui.update();
        }
        repair_cards::fit_meters(ui, COMPARE, PREFIX);
        if let (Some(title), Some(line)) = (ui.find_tile(TITLE), ui.find_tile(LINE)) {
            write_frame_of(ui, title, line, device, &mut w.flat);
        }
        if let Some(list_tile) = ui.find_tile(LIST) {
            list::settle_list(ui, list_tile, &w.rows, ROW_TEXT, &mut w.view, s.highlight, 0, s.pointer_pick, s.navigated);
            w.view.rewrite(ui, list_tile);
        }
        write_brackets(ui, &w.rows, w.view.value);
        ui.update();
        super::item_menu::mark_menu(ui, REPAIR_MENU, &[LIST]);
        w.shown = Some(s);
    } else if let Some(list_tile) = ui.find_tile(LIST) {
        w.view.rewrite(ui, list_tile);
    }
    if w.shown.as_ref().is_some_and(|s| s.choose) {
        set(ui, CHOOSE, "alpha", Value::Num(pulse(secs)));
    }
    if late {
        ui.update();
        let pieces = fonts.set_out(ui, menu);
        usable(ui, menu, tiles, &pieces)?;
        tiles.pieces = pieces;
        let w = &mut tiles.written;
        if std::mem::take(&mut w.tell) {
            if let Some(s) = w.shown.as_ref() {
                info!("{}", describe(ui, s));
            }
        }
        let now = ui.find_tile(LIST).and_then(|l| list::value_shown(ui, l));
        if w.drawn != now {
            if let Some(v) = now {
                let (value, most) = (Value::Num(v).text(), Value::Num(w.view.counts.most()).text());
                info!("tiles: {REPAIR_MENU} list scrolled to {value} of {most}");
            }
            w.drawn = now;
        }
    }
    Ok(menu)
}

/// What a pass over the page reads.
#[derive(SystemParam)]
pub(crate) struct RepairPage<'w> {
    script: Option<ResMut<'w, ScriptState>>,
    switch: Option<Res<'w, TileMenus>>,
    pip: Option<Res<'w, PipBoy>>,
    repair: Option<Res<'w, PipRepair>>,
    combat: Option<Res<'w, crate::combat::Combat>>,
    tiles: ResMut<'w, RepairTiles>,
    fonts: FontKit<'w>,
    time: Res<'w, Time<Real>>,
}

/// Whether the page is eligible to prepare: the switch on and the list up on ITEMS.
pub fn eligible(switch: TileMenus, pip: &PipBoy, open: bool) -> bool {
    switch.repair_menu && open && pip.shown() && pip.page == Page::Items
}

/// A pass; `None` when the page is not eligible, or its tree is not there yet: the code is set
/// late in a frame and the tree built in the next (nothing is touched then, and a file that cannot
/// be built is told by `fnv_game::ui_loading::sync`).
fn pass(p: &mut RepairPage<'_>, late: bool) -> Option<Result<TileId, String>> {
    let pip = p.pip.as_deref()?;
    let session = p.repair.as_deref().and_then(|r| r.0.as_ref());
    if !eligible(p.switch.as_deref().copied().unwrap_or_default(), pip, session.is_some()) {
        p.tiles.closed();
        return None;
    }
    let session = session?;
    let device = pip.frame == Frame::Device;
    let secs = f64::from(p.time.elapsed_secs());
    let combat = p.combat.as_deref();
    let game = &mut p.script.as_mut()?.rt.as_mut()?.game;
    game.ui_files.as_ref()?;
    if !game.menu_open(codes::REPAIR) || game.ui_tree.menu(REPAIR_MENU).is_none() {
        if late {
            p.tiles.no_tree_yet();
        }
        return None;
    }
    p.tiles.waiting = false;
    p.tiles.follow_model(session.selected);
    let stat_at = |c: f32| crate::pipboy::repair::stat(combat, &session.offer, c);
    let mut shown = Shown::of(session, p.tiles.on_rows, p.tiles.pointer_pick, p.tiles.navigated, &stat_at, &Words::read(game));
    let add_icons = game.ysi_add_inventory_icons();
    shown.ysi_icons = add_icons;
    if add_icons {
        for r in &mut shown.rows {
            r.ysi = game.ysi_item_display(r.key.0);
        }
    }
    Some(page(&mut game.ui_tree, shown, device, late, secs, &mut p.tiles, &mut p.fonts))
}

/// Before the scripts, after the tile input: this frame's model in the tree, and the scroll.
pub(crate) fn write_system(mut p: RepairPage) {
    // A failure here is told by `settle`, which decides.
    let _ = pass(&mut p, false);
}

/// Before the drawing, after the model's keys: what they changed, the tree worked out, and the
/// one decision every reader of this frame shares.
pub(crate) fn settle(mut p: RepairPage) {
    let ready = match pass(&mut p, true) {
        Some(Ok(menu)) => Some(menu),
        Some(Err(reason)) => {
            let line = format!("{REPAIR_MENU} drawn by hand: {reason}");
            if p.tiles.fallbacks.insert(line.clone()) {
                warn!("{line}");
            }
            None
        }
        None => None,
    };
    if ready.is_none() {
        // The next time it is written whole; the rows stay.
        p.tiles.written.shown = None;
        p.tiles.written.drawn = None;
        p.tiles.pieces.clear();
    }
    if p.tiles.ready != ready {
        p.tiles.ready = ready;
    }
}

/// The menu, when the page takes this frame's tile input: painted last frame, and the page up,
/// with nothing over it.
pub fn takes(tiles: &RepairTiles, switch: TileMenus, pip: &PipBoy, open: bool, game: &fnv_game::Game<'_>, paused: bool) -> Option<TileId> {
    if !pip.open() || !eligible(switch, pip, open) || paused || modal_over(game.menu) {
        return None;
    }
    tiles.shown.filter(|&m| game.ui_files.is_some() && game.ui_tree.is_live(m))
}

/// What the page's input keeps between frames.
#[derive(Default)]
pub(crate) struct Feed {
    /// The frame it took input in, and the pointer's place then, until it moved: a row drawn under
    /// a pointer that has not moved chooses nothing.
    taking: Option<Frame>,
    drawn_at: Option<Vec2>,
    /// The row the pointer last came onto.
    acted: Option<usize>,
}

/// What the row under the pointer asks, `under`, against the one it last came onto (`acted`).
/// `settled`: the pointer has moved since the page came up. Returns the row, or `Some(None)` for
/// the pointer leaving the rows; `None` for nothing new.
pub(super) fn follow_hover(acted: &mut Option<usize>, under: Option<usize>, settled: bool) -> Option<Option<usize>> {
    match under {
        Some(i) if settled && *acted != Some(i) => {
            *acted = Some(i);
            Some(Some(i))
        }
        Some(_) => None,
        None => acted.take().map(|_| None),
    }
}

/// What the page's tile input reads of the Pip-Boy.
#[derive(SystemParam)]
pub(crate) struct FeedPip<'w> {
    pip: Res<'w, PipBoy>,
    screen: Option<Res<'w, PipBoyScreen>>,
    aid: Res<'w, PipBoyAid>,
    paused: Option<Res<'w, crate::pause::Paused>>,
    repair: Res<'w, PipRepair>,
}

/// The page's tile input, on a frame it takes it ([`takes`]): see the module notes.
pub(crate) fn feed(mut script: ResMut<ScriptState>, switch: Res<TileMenus>, from: FeedPip, mut tiles: ResMut<RepairTiles>, devices: Devices, mut out: MessageWriter<RepairInput>, mut state: Local<Feed>) {
    if !switch.repair_menu {
        return;
    }
    let FeedPip { pip, screen, aid, paused, repair } = from;
    let Some(rt) = script.rt.as_mut() else { return };
    let game = &mut rt.game;
    let open = repair.0.as_ref();
    let modal = eligible(*switch, &pip, open.is_some()) && pip.open() && modal_over(game.menu);
    if tiles.modal != modal {
        tiles.modal = modal;
    }
    let is_paused = paused.as_ref().is_some_and(|p| p.0);
    let menu = takes(&tiles, *switch, &pip, open.is_some(), game, is_paused).filter(|_| !tiles.others);
    let (Some(menu), Some(session)) = (menu, open) else {
        *state = Feed::default();
        return;
    };
    let pointer = screen.as_ref().and_then(|s| s.pointer);
    if state.taking != Some(pip.frame) {
        *state = Feed { taking: Some(pip.frame), drawn_at: pointer, ..Default::default() };
    }
    if pointer != state.drawn_at {
        state.drawn_at = None;
    }
    let settled = state.drawn_at.is_none() || pointer.is_none();
    // The test aid's click (`crate::pipboy::drive_pointer`): 1 is its press's frame.
    let aid_press = aid.click == 1 && !aid.right;
    let held = aid_press || devices.mouse.pressed(MouseButton::Left);
    let at = pointer.map(|p| (f64::from(p.x), f64::from(p.y)));
    let mut input = fnv_ui::input::Input {
        at,
        press: aid_press || devices.mouse.just_pressed(MouseButton::Left),
        shift: devices.keys.pressed(KeyCode::ShiftLeft) || devices.keys.pressed(KeyCode::ShiftRight),
        ..Default::default()
    };
    let physical = frame_keys(&mut input, &devices, game, crate::pad::at_menu(codes::REPAIR));
    let report = fnv_game::ui_input::step(game, &[menu], &input, &fnv_ui::input::no_measure);
    let ui = &game.ui_tree;
    log_report(ui, &report);
    let tiles = &mut *tiles;
    if !held {
        tiles.drag = None;
    }
    let mut pressed_marker = false;
    let mut row_acted = false;
    for c in report.clicks.iter().filter(|c| ui.is_live(c.tile)) {
        if let fnv_ui::input::By::Key(k) = &c.by {
            if !menus::key_click_acts(REPAIR_MENU, k, &physical) {
                continue;
            }
        }
        let Some(what) = tiles.clicked(ui, c.tile) else { continue };
        let ask = match what {
            RepairTile::Row(index) => {
                let at_row = if row_acted { None } else { tiles.row_at(index) };
                let Some(index) = at_row else {
                    info!("tiles: {REPAIR_MENU} a click on a row no longer there is ignored");
                    continue;
                };
                row_acted = true;
                tiles.on_rows = true;
                tiles.asked = Some(index);
                RepairInput::Click(index)
            }
            RepairTile::Repair => RepairInput::Use,
            RepairTile::Cancel => RepairInput::Cancel,
            RepairTile::Bar(list::BarPiece::Marker) => {
                pressed_marker = true;
                let list_tile = ui.find_tile(LIST);
                tiles.drag = at.filter(|_| held && matches!(c.by, fnv_ui::input::By::Pointer)).zip(list_tile).and_then(|((_, y), l)| list::marker_grab(ui, menu, l, y));
                continue;
            }
            RepairTile::Bar(piece) => {
                let value = tiles.written.view.value;
                if let Some(v) = ui.find_tile(LIST).and_then(|l| list::after_piece(ui, l, piece, value)) {
                    info!("tiles: {REPAIR_MENU} {piece:?} on its tiles");
                    tiles.written.view.scroll_to(v);
                }
                continue;
            }
            RepairTile::Other => continue,
        };
        info!("tiles: {REPAIR_MENU} {ask:?} on its tiles");
        out.write(ask);
    }
    // A drag: the scroll under the pointer while the press holds.
    if let (false, Some(drag), Some((_, y))) = (pressed_marker, tiles.drag, at) {
        if let Some(v) = list::value_under(ui, drag, y) {
            tiles.written.view.scroll_to(v);
        }
    }
    // The tile under the pointer itself, whatever a key moved the highlight to (as
    // `super::stats_menu::feed`).
    let under = at.and_then(|at| fnv_ui::input::hit(ui, &[menu], at, &fnv_ui::input::no_measure)).and_then(|h| tiles.clicked(ui, h.tile));
    let under_row = if let Some(RepairTile::Row(i)) = under { tiles.row_at(i) } else { None };
    match follow_hover(&mut state.acted, under_row, settled) {
        Some(Some(i)) => {
            tiles.on_rows = true;
            tiles.asked = Some(i);
            if !row_acted && session.selected != i {
                info!("tiles: {REPAIR_MENU} Hover({i}) on its tiles");
                out.write(RepairInput::Hover(i));
            }
        }
        Some(None) => {
            tiles.on_rows = false;
            tiles.asked = Some(0);
            if !row_acted && session.selected != 0 {
                info!("tiles: {REPAIR_MENU} Hover(0) on its tiles");
                out.write(RepairInput::Hover(0));
            }
        }
        None => {}
    }
    // The scroll and the highlight are the view's again, over the file's own count of the clicks
    // and their pulses' ends.
    if let Some(l) = game.ui_tree.find_tile(LIST) {
        tiles.written.view.rewrite(&mut game.ui_tree, l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipboy::repair::{Offer, Part};
    use fnv_game::repair::MergeSettings;
    use fnv_ui::MemSource;

    /// [design] A fixture written for these tests, with its own dimensions and simple dependencies
    /// (#1512). It keeps the tile and trait names this module looks up, which are the engine's
    /// interface; no value or expression comes from the game's file, and it says nothing about it.
    fn fixture(change: impl FnOnce(String) -> String) -> Ui {
        let card = |name: &str, inner: &str| format!(r#"<rect name="{name}"><text name="CI_TitleText"/><text name="CI_ValueText"/>{inner}</rect>"#);
        let meter = |name: &str| format!(r#"<rect name="{name}"><image name="MeterImage"/></rect>"#);
        let button = |name: &str, words: &str, y: u32, hide: &str| format!(r#"<image name="{name}"><y>{y}</y>
            <width><copy src="child(button_text)" trait="width"/><add>12</add></width>
            <height><copy src="child(TestPicture)" trait="fileheight"/><add>6</add></height>
            <target>1</target>{hide}<text name="button_text"><y>2</y><string>{words}</string></text>
            <image name="TestPicture"><visible>0</visible><filename>pics\button.dds</filename></image>
            <text name="PCShortcutLabel"><depth>-3</depth><x><copy src="parent()" trait="width"/><add>8</add></x>
              <string><copy src="parent()" trait="_PCButtonText"/></string></text></image>"#);
        let xml = format!(
            r#"<menu name="RepairMenu"><rect name="NOGLOW_BRANCH"/><rect name="GLOW_BRANCH"><image name="RM_Background"/><rect name="RM_MainRect"><x>30</x><y>40</y>
              <rect name="RM_HeadlineRect"><text name="RM_Headline_Title"><string>Own title</string></text><image name="RM_Headline_h2"><x>200</x><_length>300</_length></image>{}</rect>
              <hotrect name="RM_RepairList"><y>70</y><width>240</width><height>200</height><clipwindow>1</clipwindow><_scroll_delta>13</_scroll_delta>
                <_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>
                <hotrect name="lb_highlight_box"/>
                <image name="lb_scrollbar"><_current_value>0</_current_value>
                  <image name="scrollbar_vert_marker"/><image name="scrollbar_vert_up"/><image name="scrollbar_vert_down"/>
                  <hotrect name="scrollbar_vert_page_up"/><hotrect name="scrollbar_vert_page_down"/>
                </image>
                <image name="RM_LeftBracket"/><image name="RM_RightBracket"/>
              </hotrect>
              <image name="RM_ItemIcon"/>
              <rect name="RM_CompareRect"><x>300</x><y>300</y>{}{}<image name="RM_ArrowIcon"/><text name="RM_HealthImprovementText"/><text name="RM_StatImprovementText"/><text name="RM_ChooseItemText"/>{}{}</rect>
              <rect name="RM_ButtonRect"><x>500</x><y>70</y>{}{}</rect>
              </rect></rect>
              <template name="RM_RepairListTemplate"><hotrect name="RM_RepairListTemplateRect"><width>220</width><target>1</target>
                <_y>0</_y><_VerticalSpacing>8</_VerticalSpacing>
                <y><copy src="me()" trait="_y"/><sub src="parent()" trait="_offset"/></y>
                <text name="ListItemText"/><image name="RM_Template_MeterBackground"><width>50</width></image><image name="RM_Template_Meter"/><image name="RM_Template_ItemMarker"/>
              </hotrect></template></menu>"#,
            card("RM_Headline_PlayerSkillInfo", ""),
            card("RM_BrokenItemHealth", &meter("RM_BrokenItemHealthMeter")),
            card("RM_BrokenItemStat", ""),
            card("RM_FixedItemHealth", &meter("RM_FixedItemHealthMeter")),
            card("RM_FixedItemStat", ""),
            button("RM_RepairButton", "Own mend", 0, "<visible>0</visible>"),
            button("RM_CancelButton", "Own back", 40, ""),
        );
        let mut ui = Ui::new();
        ui.load_menu(&MemSource::default().with("fixture.xml", &change(xml)), "fixture.xml").unwrap();
        ui.update();
        ui
    }

    fn at(condition: f32) -> Extra {
        Extra { condition: Some(condition), ..Extra::default() }
    }

    fn session(selected: usize) -> Session {
        let part = |id, c: f32| Part { item: FormId(id), extra: at(c), name: format!("Part {id}"), condition: c, kit: false };
        Session {
            offer: Offer { item: FormId(1), extra: at(0.25), name: "Own item".into(), gear: Gear::Weapon, condition: 0.25, icon: Some("interface\\icons\\own.dds".into()), parts: vec![part(2, 0.5), part(1, 0.25)] },
            skill: 40.0,
            settings: MergeSettings::default(),
            first: 0,
            selected,
            preview: false,
        }
    }

    fn words() -> Words {
        Words { skill: "Own skill".into(), condition: "State".into(), damage: "Force".into(), threshold: "Guard".into(), cancel_hint: "[E]".into() }
    }

    fn stat(c: f32) -> Option<(&'static str, i32)> {
        Some(("DAM", (c * 20.0) as i32))
    }

    fn shown(selected: usize, on_rows: bool) -> Shown {
        Shown::of(&session(selected), on_rows, false, 0, &stat, &words())
    }

    #[test]
    fn the_stat_card_title_follows_the_items_kind_not_a_localized_label() {
        // A load order whose settings name damage and DT in other words (written for the test).
        let localized = |c: f32| Some(("SCH", (c * 20.0) as i32));
        let weapon = Shown::of(&session(1), true, false, 0, &localized, &words());
        assert_eq!(weapon.figures.stat_title, "Force", "a weapon's card is damage whatever its label");
        let mut armor = session(1);
        armor.offer.gear = Gear::Armor;
        let localized_dt = |c: f32| Some(("SRS", (c * 10.0) as i32));
        let armor = Shown::of(&armor, true, false, 0, &localized_dt, &words());
        assert_eq!(armor.figures.stat_title, "Guard", "an armour's card is DT");
        let none = Shown::of(&session(1), true, false, 0, &|_| None, &words());
        assert_eq!(none.figures.stat_title, "", "no figure, no title");
    }

    struct TestFonts;

    fn measure(_: f64, text: &str, wrap: Option<f64>) -> (f64, f64) {
        (wrap.unwrap_or(1000.0).min(text.chars().count() as f64 * 8.0), 20.0)
    }

    impl PageFonts for TestFonts {
        fn write_sizes(&mut self, ui: &mut Ui, texts: &[TileId], written: &mut SizesWritten) -> usize {
            menus::write_text_sizes_of(ui, texts, written, &measure)
        }
        fn set_out(&mut self, ui: &Ui, menu: TileId) -> Vec<Piece> {
            super::super::draw::pieces(ui, menu, &measure, super::super::draw::Select::Whole { kept: &[], strip: None }, true).0
        }
        fn picture_texels(&mut self, name: &str) -> Option<(f64, f64)> {
            match name {
                "pics\\button.dds" => Some((32.0, 24.0)),
                "pics\\other.dds" => Some((48.0, 36.0)),
                _ => None,
            }
        }
    }

    fn run(ui: &mut Ui, s: Shown, tiles: &mut RepairTiles) -> Result<TileId, String> {
        page(ui, s, true, true, 0.0, tiles, &mut TestFonts)
    }

    fn number(ui: &Ui, path: &str, name: &str) -> f64 {
        ui.get_float(&format!("{path}/{name}")).unwrap()
    }

    fn string(ui: &Ui, path: &str, name: &str) -> String {
        ui.get_string(&format!("{path}/{name}")).unwrap()
    }

    fn cards(name: &str) -> String {
        format!("{COMPARE}/{name}")
    }

    #[test]
    fn cancel_checks_its_words_and_sizes_hidden_button_pictures() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        let menu = run(&mut ui, shown(0, false), &mut tiles).unwrap();
        assert_eq!(number(&ui, CANCEL, "width"), 76.0);
        assert_eq!(number(&ui, CANCEL, "height"), 30.0);
        assert_eq!(number(&ui, &format!("{REPAIR}/TestPicture"), "fileheight"), 24.0, "hidden ancestors still need sizes");
        let cancel = ui.find_tile(CANCEL).unwrap();
        let texts: Vec<_> = tiles.pieces.iter().filter(|p| matches!(p.look, Look::Text { .. }) && super::super::stats_menu::inside(&ui, p.tile, cancel)).collect();
        assert_eq!(ui.tile(texts[0].tile).name, "PCShortcutLabel", "the first descendant is the external hint");
        assert!(fnv_ui::input::hit(&ui, &[menu], shows_whole(texts[0]).unwrap(), &fnv_ui::input::no_measure).is_none(), "the external hint has no pointer target");
        let words = texts.iter().find(|p| ui.tile(p.tile).name == "button_text").unwrap();
        let hit = fnv_ui::input::hit(&ui, &[menu], shows_whole(words).unwrap(), &fnv_ui::input::no_measure).unwrap();
        assert_eq!(tiles.clicked(&ui, hit.tile), Some(RepairTile::Cancel));
        assert_eq!(number(&ui, &format!("{REPAIR}/button_text"), "width"), 64.0, "hidden button text is measured too");
    }

    #[test]
    fn picture_sizes_cover_the_page_and_follow_file_and_zoom_changes() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        run(&mut ui, shown(1, true), &mut tiles).unwrap();
        let row_picture = list::child(&ui, tiles.written.rows[0].tile, ROW_MARK).unwrap();
        let paths = [ICON.to_owned(), LEFT_BRACKET.to_owned(), cards("RM_ArrowIcon"), format!("{CANCEL}/TestPicture")];
        let pictures: Vec<_> = paths.iter().map(|p| ui.find_tile(p).unwrap()).chain([row_picture]).collect();
        for &t in &pictures {
            assert_eq!(ui.tile(t).kind, "image", "{} must be a picture", ui.path_of(t));
            ui.set_trait(t, "filename", Value::Str("pics\\button.dds".into()));
            ui.set_trait(t, "zoom", Value::Num(125.0));
        }
        // [design: `page`] A late pass sees changes even when the model did not change.
        run(&mut ui, shown(1, true), &mut tiles).unwrap();
        for &t in &pictures {
            assert_eq!(ui.tile(t).get("filewidth").map(Value::num), Some(40.0), "{} filewidth", ui.path_of(t));
            assert_eq!(ui.tile(t).get("fileheight").map(Value::num), Some(30.0), "{} fileheight", ui.path_of(t));
        }
        let icon = pictures[0];
        ui.set_trait(icon, "zoom", Value::Num(50.0));
        ui.set_trait(icon, "filename", Value::Str("pics\\other.dds".into()));
        let atlas = pictures[1];
        ui.set_trait(atlas, "texatlas", Value::Str("test.tai".into()));
        ui.set_trait(atlas, "filewidth", Value::Num(7.0));
        run(&mut ui, shown(1, true), &mut tiles).unwrap();
        assert_eq!(ui.tile(icon).get("filewidth").unwrap().num(), 24.0);
        assert_eq!(ui.tile(icon).get("fileheight").unwrap().num(), 18.0);
        assert_eq!(ui.tile(atlas).get("filewidth").unwrap().num(), 7.0);
    }

    #[test]
    fn a_covered_cancel_still_falls_back_and_names_what_took_the_pointer() {
        let (mut ui, mut tiles) = (fixture(|s| s.replace("<rect name=\"RM_ButtonRect\"><x>500</x><y>70</y>", r#"<rect name="RM_ButtonRect"><x>500</x><y>70</y><hotrect name="TestCover"><target>1</target><depth>8</depth><y>40</y><width>100</width><height>30</height></hotrect>"#)), RepairTiles::default());
        let reason = run(&mut ui, shown(0, false), &mut tiles).unwrap_err();
        assert!(reason.starts_with("Cancel does not take the pointer:"), "{reason}");
        assert!(reason.contains("RM_ButtonRect/TestCover takes it"), "{reason}");
    }

    #[test]
    fn an_incomplete_file_falls_back_whole() {
        assert_eq!(built(&fixture(|s| s)), Tiled::Yes);
        for missing in ["RM_Background", "RM_Headline_h2", "RM_LeftBracket", "RM_ChooseItemText", "RM_FixedItemHealthMeter", "CI_ValueText", "PCShortcutLabel", "scrollbar_vert_marker", "RM_RepairListTemplateRect", "RM_Template_MeterBackground", "RM_Template_ItemMarker"] {
            let ui = fixture(|s| s.replace(missing, "Absent"));
            assert!(!built(&ui).yes(), "{missing}");
        }
        assert!(!built(&Ui::new()).yes());
        let mut ui = fixture(|s| s.replace("RM_ArrowIcon", "Absent"));
        assert!(run(&mut ui, shown(0, false), &mut RepairTiles::default()).is_err(), "a short file is no page");
    }

    #[test]
    fn the_item_stands_first_marked_and_each_part_says_what_it_gives() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        run(&mut ui, shown(0, false), &mut tiles).unwrap();
        let rows = &tiles.written.rows;
        assert_eq!(rows.len(), 3);
        let s = session(0);
        let trait_of = |i: usize, name: &str| ui.tile(rows[i].tile).get(name).unwrap().num();
        assert_eq!(trait_of(0, "_IsRepairItem"), 1.0);
        assert_eq!(trait_of(1, "_IsRepairItem"), 0.0);
        assert_eq!(trait_of(2, "user1"), 0.0);
        assert_eq!(trait_of(1, "user0") as f32, merged_condition(&s.settings, 0.25, 0.5, 40.0));
        assert_eq!(trait_of(0, "user0") as f32, merged_condition(&s.settings, 0.25, 0.25, 40.0), "the item as if merged with itself");
        assert_eq!(trait_of(2, "user0"), trait_of(0, "user0"), "a part alike gives the same");
        assert_ne!(rows[0].tile, rows[2].tile, "a part like the item is a row of its own");
        let child = |i: usize, name| list::child(&ui, rows[i].tile, name).unwrap();
        assert_eq!(ui.tile(child(0, ROW_TEXT)).get("string").unwrap().text(), "Own item");
        assert_eq!(ui.tile(child(1, ROW_TEXT)).get("alpha").unwrap().num(), 255.0);
        assert_eq!(ui.tile(child(1, ROW_METER)).get("width").unwrap().num(), 25.0, "the part's own condition of the back's 50");
        assert_eq!(ui.tile(child(0, ROW_METER)).get("width").unwrap().num(), 12.5);
        assert_eq!(ui.tile(child(0, ROW_MARK)).get("visible").unwrap().num(), 0.0);
    }

    #[test]
    fn off_any_row_only_the_items_own_cards_stand() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        run(&mut ui, shown(0, false), &mut tiles).unwrap();
        assert_eq!(number(&ui, &cards("RM_BrokenItemHealth"), "visible"), 1.0);
        assert_eq!(number(&ui, &cards("RM_BrokenItemHealth"), "user0"), 0.25);
        assert_eq!(number(&ui, &cards("RM_BrokenItemStat"), "visible"), 1.0);
        assert_eq!(number(&ui, &cards("RM_FixedItemHealth"), "visible"), 0.0);
        assert_eq!(number(&ui, &cards("RM_HealthImprovementText"), "visible"), 0.0);
        assert_eq!(number(&ui, CHOOSE, "visible"), 0.0);
        assert_eq!(number(&ui, LIST, "_highlight_y"), -1.0);
        assert_eq!(number(&ui, ICON, "visible"), 1.0);
        assert_eq!(string(&ui, ICON, "filename"), "interface\\icons\\own.dds");
    }

    #[test]
    fn on_the_item_the_choose_text_shows_and_pulses() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        run(&mut ui, shown(0, true), &mut tiles).unwrap();
        assert_eq!(number(&ui, CHOOSE, "visible"), 1.0);
        assert_eq!(string(&ui, CHOOSE, "string"), CHOOSE_WORDS);
        assert_eq!(number(&ui, CHOOSE, "alpha"), PULSE_LOW);
        assert_eq!(number(&ui, &cards("RM_FixedItemHealth"), "visible"), 0.0);
        assert_eq!(number(&ui, REPAIR, "target"), 0.0, "the item is not used on itself");
        assert_eq!(number(&ui, LIST, "_highlight_y"), 0.0);
        page(&mut ui, shown(0, true), true, true, PULSE_SECONDS / 2.0, &mut tiles, &mut TestFonts).unwrap();
        assert_eq!(number(&ui, CHOOSE, "alpha"), 255.0);
        for i in 0..40 {
            let a = pulse(f64::from(i) * 0.17);
            assert!((PULSE_LOW..=255.0).contains(&a), "{a}");
        }
    }

    #[test]
    fn on_a_part_the_fixed_cards_and_the_gains_show() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        run(&mut ui, shown(1, true), &mut tiles).unwrap();
        let s = session(1);
        let to = merged_condition(&s.settings, 0.25, 0.5, 40.0);
        assert_eq!(number(&ui, CHOOSE, "visible"), 0.0);
        assert_eq!(number(&ui, &cards("RM_FixedItemHealth"), "visible"), 1.0);
        assert_eq!(number(&ui, &cards("RM_FixedItemHealth"), "user0") as f32, to.clamp(0.0, 1.0));
        assert_eq!(number(&ui, &cards("RM_FixedItemStat"), "visible"), 1.0);
        assert_eq!(string(&ui, &cards("RM_FixedItemStat"), "_Value"), ((to * 20.0) as i32).to_string());
        assert_eq!(string(&ui, &cards("RM_BrokenItemStat"), "_Title"), "Force", "the setting's words");
        assert_eq!(string(&ui, &cards("RM_HealthImprovementText"), "string"), format!("+{}%", gain_percent(0.25, to)));
        assert_eq!(number(&ui, REPAIR, "target"), 1.0);
        assert!(number(&ui, LIST, "_highlight_y") > 0.0);
    }

    #[test]
    fn the_brackets_stay_on_the_items_row_whatever_is_hovered() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        for s in [shown(0, false), shown(0, true), shown(2, true)] {
            run(&mut ui, s, &mut tiles).unwrap();
            let height = ui.tile(tiles.written.rows[0].tile).get("height").unwrap().num();
            for bracket in [LEFT_BRACKET, RIGHT_BRACKET] {
                assert_eq!(number(&ui, bracket, "y"), 0.0, "{bracket}");
                assert_eq!(number(&ui, bracket, "height"), height - 1.0, "{bracket}");
            }
        }
    }

    #[test]
    fn the_words_are_settings_and_the_files_stay() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        run(&mut ui, shown(1, true), &mut tiles).unwrap();
        assert_eq!(string(&ui, SKILL, "_Title"), "Own skill");
        assert_eq!(string(&ui, SKILL, "_Value"), "40");
        assert_eq!(string(&ui, CANCEL, "_PCButtonText"), "[E]");
        assert_eq!(string(&ui, &format!("{CANCEL}/button_text"), "string"), "Own back");
        assert_eq!(string(&ui, TITLE, "string"), "Own title");
        assert_eq!(number(&ui, BACKGROUND, "visible"), 0.0, "the Pip-Boy's own backing");
        assert_eq!(number(&ui, REPAIR, "visible"), 0.0, "the file's, hidden on a PC");
    }

    #[test]
    fn the_screen_menu_hides_the_title_and_gives_it_back_on_the_device() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        page(&mut ui, shown(0, false), false, true, 0.0, &mut tiles, &mut TestFonts).unwrap();
        assert_eq!(number(&ui, TITLE, "visible"), 0.0);
        page(&mut ui, shown(0, false), true, true, 0.0, &mut tiles, &mut TestFonts).unwrap();
        assert_ne!(ui.get_float(&format!("{TITLE}/visible")), Some(0.0));
        assert_eq!(number(&ui, LINE, "x"), 200.0);
    }

    #[test]
    fn clicks_stand_for_their_tiles_and_a_row_gone_acts_on_nothing() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        run(&mut ui, shown(1, true), &mut tiles).unwrap();
        let first = tiles.written.rows[1].tile;
        assert_eq!(tiles.clicked(&ui, first), Some(RepairTile::Row(1)));
        assert_eq!(tiles.row_at(1), Some(1));
        for (path, what) in [(format!("{CANCEL}/button_text"), RepairTile::Cancel), (REPAIR.to_string(), RepairTile::Repair), (format!("{LIST}/lb_scrollbar/scrollbar_vert_up"), RepairTile::Bar(list::BarPiece::Up)), (ICON.to_string(), RepairTile::Other)] {
            assert_eq!(tiles.clicked(&ui, ui.find_tile(&path).unwrap()), Some(what), "{path}");
        }
        let mut less = session(0);
        less.offer.parts.remove(0);
        let s = Shown::of(&less, true, false, 0, &stat, &words());
        run(&mut ui, s, &mut tiles).unwrap();
        assert!(!ui.is_live(first));
        assert_eq!(tiles.clicked(&ui, first), None);
        assert_eq!(tiles.row_at(2), None);
        let other = ui.load_menu(&MemSource::default().with("o.xml", r#"<menu name="Other"><rect name="RM_CancelButton"/></menu>"#), "o.xml").unwrap();
        assert_eq!(tiles.clicked(&ui, other), None);
    }

    #[test]
    fn who_moved_the_selection_decides_the_scroll_and_the_highlight() {
        let mut tiles = RepairTiles::default();
        tiles.follow_model(0);
        assert!(!tiles.on_rows, "the page opens with no row hovered");
        assert!(!tiles.pointer_pick);
        tiles.asked = Some(2);
        tiles.follow_model(2);
        assert!(tiles.pointer_pick, "the pointer's pick leaves the list");
        let before = tiles.navigated;
        tiles.follow_model(1);
        assert!(!tiles.pointer_pick);
        assert_eq!(tiles.navigated, before + 1, "a key's move brings its row into view");
        assert!(tiles.on_rows, "a key on a part shows its figures");
        tiles.follow_model(0);
        assert!(tiles.on_rows, "and a key back on the item shows the item's row hovered");
        tiles.closed();
        assert!(!tiles.on_rows);
        assert_eq!(tiles.selected, None);
    }

    #[test]
    fn leaving_the_rows_returns_the_page_off_any_row() {
        let mut tiles = RepairTiles::default();
        tiles.follow_model(0);
        tiles.follow_model(2);
        assert!(tiles.on_rows);
        // What `feed` does as the pointer leaves, then the pass before the model's keys apply its
        // `Hover(0)`, then the one after.
        tiles.on_rows = false;
        tiles.asked = Some(0);
        tiles.follow_model(2);
        assert!(!tiles.on_rows, "an unchanged selection says nothing");
        tiles.follow_model(0);
        assert!(!tiles.on_rows);
        let s = Shown::of(&session(0), tiles.on_rows, tiles.pointer_pick, tiles.navigated, &stat, &words());
        assert_eq!((s.highlight, s.choose), (None, false));
    }

    #[test]
    fn the_hand_built_list_waits_for_the_tree_only_so_long() {
        let mut tiles = RepairTiles::default();
        assert!(!tiles.waiting, "nothing to wait for with the switch off or the list shut");
        tiles.no_tree_yet();
        assert!(tiles.waiting);
        tiles.no_tree_yet();
        assert!(tiles.waiting);
        tiles.no_tree_yet();
        assert!(!tiles.waiting, "a file that cannot be built leaves the hand-built list the pointer");
        tiles.closed();
        tiles.no_tree_yet();
        assert!(tiles.waiting, "the next opening waits again");
    }

    #[test]
    fn a_pointer_chooses_only_once_it_moved_and_leaving_says_so() {
        let mut acted = None;
        assert_eq!(follow_hover(&mut acted, Some(1), false), None, "drawn under a resting pointer");
        assert_eq!(follow_hover(&mut acted, None, false), None, "nothing to leave");
        assert_eq!(follow_hover(&mut acted, Some(1), true), Some(Some(1)));
        assert_eq!(follow_hover(&mut acted, Some(1), true), None, "once");
        assert_eq!(follow_hover(&mut acted, Some(2), true), Some(Some(2)));
        assert_eq!(follow_hover(&mut acted, None, true), Some(None));
        assert_eq!(follow_hover(&mut acted, None, true), None, "once");
    }

    #[test]
    fn physical_keys_act_once_and_scripts_keep_the_file_bindings() {
        for key in ["E", "R", "W", "xdown", "xbuttonb"] {
            assert!(!menus::key_click_acts(REPAIR_MENU, key, &[key]), "{key}");
            assert!(menus::key_click_acts(REPAIR_MENU, key, &[]), "{key}");
        }
    }

    #[test]
    fn the_same_state_writes_nothing_new_and_the_log_says_what_smoke_reads() {
        let (mut ui, mut tiles) = (fixture(|s| s), RepairTiles::default());
        run(&mut ui, shown(1, true), &mut tiles).unwrap();
        let rows: Vec<TileId> = tiles.written.rows.iter().map(|r| r.tile).collect();
        run(&mut ui, shown(1, true), &mut tiles).unwrap();
        assert_eq!(tiles.written.rows.iter().map(|r| r.tile).collect::<Vec<_>>(), rows);
        assert!(!tiles.pieces.is_empty());
        let line = describe(&ui, tiles.written.shown.as_ref().unwrap());
        assert!(line.starts_with("tiles: RepairMenu drawn from tiles: 3 rows; item \"Own item\"; highlight \"Part 2\"; choose false; skill 40;"), "{line}");
    }
}
