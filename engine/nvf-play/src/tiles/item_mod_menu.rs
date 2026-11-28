//! The Pip-Boy's weapon mod menu drawn from `menus\item_mod_menu.xml`, behind
//! `[UI] TileMenus=ItemModMenu` [design: weapon-mods.md §5.1; the way of `super::repair_menu`;
//! issue #1513]. Off (the default), none of this runs and `crate::pipboy::item_mod` draws the
//! menu in the ITEMS page's body.
//!
//! - **The one decision** ([`ItemModTiles::ready`]): the switch on, the mod menu open on ITEMS,
//!   the menu's code 1061 up and its tree built, the file's tiles there ([`built`]) and a usable
//!   measured layout (`super::repair_menu::usable_of`). Painting (`super::draw`), hand-built
//!   suppression (`crate::pipboy::frame::draw`) and input read it and nothing else, and
//!   [`settle`] publishes it each frame before any of them. A pass that fails leaves the whole
//!   hand-built menu, says why once, and is tried again the next frame. Each change of the
//!   decision is logged with the passes it held (`tiles: ItemModMenu ready: drawn from tiles`,
//!   `tiles: ItemModMenu no longer drawn from tiles after <n> passes: <why>`), so a fallback
//!   that lasts one frame shows in the log too.
//! - **The rules stay `crate::pipboy::item_mod`'s**: what is listed, what a mod does, the
//!   figures and their words (`crate::pipboy::item_mod::compare`), when the menu closes, its
//!   sounds and log lines. This module writes the [`Session`] as the file's "set in code"
//!   traits and sends [`ModInput`], as the hand-built rows do. The keys stay the model's
//!   ([`HAND_BUILT_KEYS`]). Only the list's scroll is its own.
//! - **The rows** (the maintainer's design, 2026-10-27, weapon-mods.md §5): the weapon first,
//!   framed by the file's brackets ([`super::repair_menu::write_brackets_of`]), then its attached
//!   and carried mods, the chosen row under the file's highlight box. A mod's effect stands under
//!   its name as a second line of the row's own words (`ListItemText`): the file's row has one
//!   text, so the effect wraps at the name's width, the file's 270, not the hand-built 338
//!   [design; a known difference, weapon-mods.md §5.1].
//! - **The headline and the description**: the line between the title and the weapon's name is
//!   written (`write_headline_line`), the file's own coming to 0; the description is cut at
//!   the hand-built menu's three lines (`cut_description`), so it stays off the cards below.
//! - **Page content only**, as `super::repair_menu`: the page paints into the Pip-Boy's image,
//!   whose background and scanlines are the shell's, so `WMM_Background` stays hidden. The screen
//!   menu has no title and its heading line runs on (`super::stats_menu::write_frame_of`).
//! - **Input** ([`feed`], before the scripts): the pointer is the Pip-Boy's own, in canvas units
//!   (`crate::pipboy::PipBoyScreen::pointer`), acting on the page painted last frame. A row the
//!   pointer comes onto (once it moved) is chosen, a click on a row asks the model's `Click`
//!   (choose, or on the chosen mod attach or remove it), the Mod button (shown on a controller
//!   by the file) its `Use`, Cancel its `Cancel`; the scrollbar's pieces move the list.
//! - **The words** (weapon-mods.md §5.1, each with its source): the file's settings (the title,
//!   the buttons, the condition cards' titles), a record's name and description, and the
//!   figures' words, the model's.

use super::draw::{FontKit, Piece};
use super::input::{frame_keys, log_report, Devices};
use super::item_menu::{on, set};
use super::list::{self, Drag, ListView, RowTile, Wanted};
use super::menus::{self, SizesWritten, Tiled};
use super::repair_cards::{self, CardTiles, CardValues, SideShown};
use super::repair_menu::{follow_hover, usable_of, write_brackets_of, write_file_sizes};
use super::stats_menu::{modal_over, write_frame_of, FlatChanges, PageFonts};
use super::vanilla::TileMenus;
use crate::pipboy::item_mod::{self as model, Compare, ModInput, PipItemMod, Session};
use crate::pipboy::stats::read::Forms;
use crate::pipboy::{Frame, Page, PipBoy, PipBoyAid, PipBoyScreen};
use crate::script::ScriptState;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use fnv_game::game::menus as codes;
use fnv_sim::FormId;
use fnv_ui::{TileId, Ui, Value};
use std::collections::{HashMap, HashSet};

/// [documented: item_mod_menu.xml] The menu and the paths written by this adapter.
pub const ITEM_MOD_MENU: &str = "ItemModMenu";
const BACKGROUND: &str = "ItemModMenu/GLOW_BRANCH/WMM_Background";
const MAIN: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect";
const TITLE: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_HeadlineRect/IMM_Headline_Title";
const LINE: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_HeadlineRect/IMM_Headline_h2";
const NAME: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_HeadlineRect/IMM_ItemName";
const LIST: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_ItemModList";
const LEFT_BRACKET: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_ItemModList/IMM_LeftBracket";
const RIGHT_BRACKET: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_ItemModList/IMM_RightBracket";
const ICON: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_ItemIcon";
const DESC: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_ModDesc";
const COMPARE: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_CompareRect";
const CHOOSE: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_CompareRect/IMM_ChooseItemText";
const MOD: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_ButtonRect/IMM_ItemModButton";
const CANCEL: &str = "ItemModMenu/GLOW_BRANCH/IMM_MainRect/IMM_ButtonRect/IMM_CancelButton";
const LIST_NAME: &str = "IMM_ItemModList";
const TEMPLATE: &str = "IMM_ItemModListTemplate";
const ROW: &str = "IMM_ItemModListTemplateRect";
const ROW_TEXT: &str = "ListItemText";
const ROW_MARK: &str = "IMM_Template_ItemMarker";
/// The square of a worn row where the file names no picture [design: the hand-built list's,
/// `crate::pipboy::item_mod`].
const MARK_PICTURE: &str = "interface\\shared\\marker\\square_filled.dds";
/// Final passes the page waits for its tree before the hand-built menu has the pointer
/// [design: as `super::repair_menu`].
const TREE_WAIT: u8 = 2;

/// The cards [documented: item_mod_menu.xml, names only]: the weapon as it is, and after the
/// chosen mod (the file's own `IM_` on the last stat card).
fn cards() -> CardTiles {
    CardTiles {
        now_health: format!("{COMPARE}/IMM_ItemHealth"),
        now_stat: format!("{COMPARE}/IMM_ItemStat"),
        after_health: format!("{COMPARE}/IMM_ModItemHealth"),
        after_stat: format!("{COMPARE}/IM_ModItemStat"),
        arrow: format!("{COMPARE}/IMM_ArrowIcon"),
        health_gain: format!("{COMPARE}/IMM_HealthImprovementText"),
        stat_gain: format!("{COMPARE}/IMM_StatImprovementText"),
    }
}

/// [design: `crate::pipboy::item_mod::keys`, `crate::pipboy::keys`, `crate::pad::menu_buttons`]
/// What the model takes itself, by the tile input's names: W, S, E, the Pip-Boy's A and D, the
/// arrows, and the controller's A (Enter), B (Esc) and triggers. A script's MenuTapKey still
/// follows the file's `_PCButton_E`.
pub const HAND_BUILT_KEYS: [&str; 13] = ["W", "S", "E", "A", "D", "xup", "xdown", "xleft", "xright", "xbuttona", "xbuttonb", "xbuttonlt", "xbuttonrt"];

/// What a row stands for: the weapon (`None`), or the mod in a slot.
pub type RowKey = (FormId, Option<u8>);

/// [design] A short or restructured file keeps the whole hand-built menu.
pub fn built(ui: &Ui) -> Tiled {
    let mut paths: Vec<String> = [ITEM_MOD_MENU, BACKGROUND, MAIN, TITLE, LINE, NAME, LIST, LEFT_BRACKET, RIGHT_BRACKET, ICON, DESC, COMPARE, MOD, CANCEL].into_iter().map(str::to_owned).collect();
    for button in [MOD, CANCEL] {
        paths.extend(["button_text", "PCShortcutLabel"].map(|t| format!("{button}/{t}")));
    }
    paths.push(format!("{LIST}/{}", list::HIGHLIGHT));
    for piece in [list::MARKER, list::UP, list::DOWN, list::PAGE_UP, list::PAGE_DOWN] {
        paths.push(format!("{LIST}/{}/{piece}", list::SCROLLBAR));
    }
    paths.extend(cards().paths());
    for path in &paths {
        if ui.find_tile(path).is_none() {
            return Tiled::No(format!("{path} missing"));
        }
    }
    list::template_has(ui, ITEM_MOD_MENU, TEMPLATE, ROW, &[("text", ROW_TEXT), ("image", ROW_MARK)])
}

/// The settings a frame's words read, through the strings table.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Words {
    /// For a condition card whose file gives no title.
    pub condition: String,
    /// `sPCMenuHintE` as the device names it.
    pub cancel_hint: String,
}

impl Words {
    fn read(game: &fnv_game::Game<'_>) -> Self {
        let word = |key| fnv_game::ui_loading::game_string(game, key);
        Words {
            condition: word("sInventoryCondition"),
            // The model's hints are the ITEMS page's (`crate::pipboy::item_mod::menu_hints`).
            cancel_hint: crate::pad::game_hint(&word("sPCMenuHintE"), &[KeyCode::KeyE, KeyCode::Escape], crate::pad::at_menu(codes::INVENTORY)),
        }
    }
}

/// A row as shown.
#[derive(Clone, Debug, PartialEq)]
pub struct RowShown {
    pub key: RowKey,
    /// Its name, and under a mod's its effect on a line of its own.
    pub text: String,
    /// The weapon worn, or the mod on it: the row's square.
    pub worn: bool,
    /// Resolved ySI tag/icon/template for this row when ySI is active (`None` otherwise).
    pub ysi: Option<fnv_game::ui_ysi::YsiItemDisplay>,
}

/// One whole state of the page: a frame with the same writes nothing but sizes and scroll. A
/// plain value, so tests need no world.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub rows: Vec<RowShown>,
    /// The model's chosen row: the highlight's.
    pub highlight: usize,
    /// The selection was last moved by the pointer: the list stays where it is.
    pub pointer_pick: bool,
    /// A count of the keys' moves.
    pub navigated: u32,
    /// The weapon's name, in the headline.
    pub name: String,
    pub icon: Option<String>,
    /// The chosen mod's description; empty on the weapon's row.
    pub desc: String,
    /// The figures; `None` (no weapon record) hides every card.
    pub cards: Option<CardValues>,
    /// A mod's row is chosen: the Mod button acts.
    pub chosen: bool,
    pub cancel_hint: String,
    /// Whether `apply_row_icon` should instantiate `ySIImage` row tiles (`yUI.ini`
    /// `[Sorting and Icons] bAddInventoryIcons`, gated by `bSortingIcons`).
    pub ysi_icons: bool,
}

impl Shown {
    /// `cmp`: the model's figures for the chosen row, `desc` the chosen mod's description.
    pub fn of(s: &Session, cmp: Option<&Compare>, desc: Option<&str>, pointer_pick: bool, navigated: u32, w: &Words) -> Self {
        let o = &s.offer;
        let mut rows = vec![RowShown { key: (o.item, None), text: o.name.clone(), worn: o.extra.equipped, ysi: None }];
        rows.extend(o.choices.iter().map(|ch| {
            let name = model::row_name(ch);
            let text = match ch.effect.as_deref() {
                Some(e) => format!("{name}\n{e}"),
                None => name,
            };
            RowShown { key: (ch.item, Some(ch.slot)), text, worn: ch.attached, ysi: None }
        }));
        let chosen = model::chosen(s);
        let cards = cmp.map(|c| {
            let title = c.now.as_ref().or(c.after.as_ref().and_then(Option::as_ref)).map_or("", |f| f.0);
            let side = |stat: &Option<(&'static str, String)>| SideShown { condition: c.condition, stat: stat.as_ref().map(|f| f.1.clone()) };
            CardValues {
                condition_title: w.condition.clone(),
                stat_title: title.to_string(),
                now: side(&c.now),
                after: c.after.as_ref().map(side),
                // The model has no condition gain: a mod leaves the condition as it is.
                health_gain: None,
                stat_gain: c.gain.clone(),
            }
        });
        Shown {
            rows,
            highlight: s.selected.min(o.choices.len()),
            pointer_pick,
            navigated,
            name: o.name.clone(),
            icon: chosen.map_or(o.icon.clone(), |ch| ch.icon.clone()),
            desc: desc.filter(|_| chosen.is_some()).unwrap_or_default().to_string(),
            cards,
            chosen: chosen.is_some(),
            cancel_hint: w.cancel_hint.clone(),
            ysi_icons: false,
        }
    }
}

/// What a click on the page's tiles stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModTile {
    Row(usize),
    Mod,
    Cancel,
    Bar(list::BarPiece),
    Other,
}

const TARGETS: &[(&str, ModTile)] = &[("IMM_ItemModButton", ModTile::Mod), ("IMM_CancelButton", ModTile::Cancel)];

/// What was written, and what the page keeps between frames. A new menu tile (the tree built
/// again) starts over: no tile id of the tree before is kept.
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
pub struct ItemModTiles {
    /// The menu, while the page is ready to display this frame ([`settle`]).
    pub ready: Option<TileId>,
    /// The menu painted by the last drawing (`super::draw`): what the player sees.
    pub shown: Option<TileId>,
    /// The pieces the decision checked this frame, for the drawing to paint.
    pub(crate) pieces: Vec<Piece>,
    /// A menu over the page when this frame began (as `super::repair_menu::RepairTiles::modal`).
    pub modal: bool,
    /// Another menu took this frame's tile input (`super::input::feed`).
    pub others: bool,
    /// The menu is open and the page has not decided yet: the hand-built rows that stand
    /// meanwhile take no hover (`crate::pipboy::item_mod`).
    pub waiting: bool,
    /// Final passes with no tree yet.
    waited: u8,
    /// The page was eligible on the last pass (the switch on and the menu open on ITEMS).
    open: bool,
    /// Passes in a row the page was ready.
    passes: u32,
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

impl ItemModTiles {
    /// The menu closed: what the next opening starts from.
    fn closed(&mut self) {
        self.waiting = false;
        self.waited = 0;
        self.open = false;
        self.asked = None;
        self.selected = None;
        self.pointer_pick = false;
        self.drag = None;
    }

    /// A final pass that found no tree for the open menu.
    fn no_tree_yet(&mut self) {
        self.waited = self.waited.saturating_add(1);
        self.waiting = self.waited <= TREE_WAIT;
    }

    /// The model's selection now: who moved it. `late`: the pass after the model's keys, which
    /// have applied this frame's requests by then, so the pointer's request is used up there: a
    /// key's later move back onto a row the pointer once asked for is the keys' (and brings the
    /// row into view). In a frame with both a hover and a key's move the model applies the hover
    /// first and the key after (`crate::pipboy::item_mod::keys`), so the key decides the row; the
    /// move counts as the keys' only when the chosen row changes, as anywhere else here.
    fn follow_model(&mut self, selected: usize, late: bool) {
        if self.selected != Some(selected) {
            if self.asked == Some(selected) {
                self.pointer_pick = true;
            } else {
                self.pointer_pick = false;
                self.navigated = self.navigated.wrapping_add(1);
            }
            self.selected = Some(selected);
        }
        if late {
            self.asked = None;
        }
    }

    /// The tile a click on `tile` stands for, on the page as written.
    pub fn clicked(&self, ui: &Ui, tile: TileId) -> Option<ModTile> {
        let menu = self.written.menu?;
        if !ui.is_live(tile) || ui.root_of(tile) != menu {
            return None;
        }
        Some(match list::hit(ui, tile, &[(LIST_NAME, self.written.rows.as_slice())], TARGETS) {
            list::Hit::Row(_, index) => ModTile::Row(index),
            list::Hit::Bar(_, piece) => ModTile::Bar(piece),
            list::Hit::Target(what) => what,
            list::Hit::Other => ModTile::Other,
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
    let names: Vec<String> = s.rows.iter().map(|r| r.text.lines().next().unwrap_or_default().to_string()).collect();
    let wanted: Vec<Wanted<'_, RowKey>> = s.rows.iter().zip(&names).map(|(r, name)| Wanted { key: &r.key, name }).collect();
    list::reconcile(ui, LIST, TEMPLATE, &mut w.rows, &wanted);
    for (row, shown) in w.rows.iter().zip(&s.rows) {
        // [documented: the file names it on the template, its comment the repair menu's; design:
        // the weapon's row, as `_IsRepairItem` the item's]. `user0` and `user1` are left: what
        // the original writes there for a mod was not seen.
        ui.set_trait(row.tile, "_IsItemModItem", on(shown.key.1.is_none()));
        if let Some(t) = list::child(ui, row.tile, ROW_TEXT) {
            ui.set_trait(t, "string", Value::Str(shown.text.clone()));
            // The file leaves it to code and the square copies it [documented: the file;
            // hypothesis: no row dims].
            ui.set_trait(t, "alpha", Value::Num(255.0));
        }
        if let Some(t) = list::child(ui, row.tile, ROW_MARK) {
            ui.set_trait(t, "visible", on(shown.worn));
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
    set(ui, NAME, "string", Value::Str(s.name.clone()));
    set(ui, ICON, "visible", on(s.icon.is_some()));
    set(ui, ICON, "filename", Value::Str(s.icon.clone().unwrap_or_default()));
    set(ui, DESC, "string", Value::Str(s.desc.clone()));
    repair_cards::write_values(ui, &cards(), s.cards.as_ref());
    // The hand-built menu has no such line, and what the original shows there was not seen: it
    // stays hidden [design], so the arrow follows the after cards alone (the file's rule).
    set(ui, CHOOSE, "visible", on(false));
    set(ui, CHOOSE, "string", Value::Str(String::new()));
    // Mod's `visible` is the file's, for the Xbox: on a PC it stays hidden [documented: the
    // file]. It acts on a mod's row only, as the model's Use does.
    set(ui, MOD, "target", on(s.chosen));
    set(ui, CANCEL, "visible", on(true));
    set(ui, CANCEL, "target", on(true));
    set(ui, CANCEL, "_PCButtonText", Value::Str(s.cancel_hint.clone()));
}

/// Between the headline's line and the weapon's name, as between the title and the line
/// [documented: the file starts the line 20 past the title; design: the same 20 before the name].
const NAME_GAP: f64 = 20.0;

/// The headline's line from where it starts (after the title on the device, the lead line's end in
/// the screen menu, `super::stats_menu::write_frame_of`) to [`NAME_GAP`] before the name's words,
/// as the hand-built heading draws it (`crate::pipboy::frame::heading_named`). The file's
/// `_length` reads a tile it does not have (`IMM_Headline_PlayerSkillInfo`, the repair file's), so
/// it comes to 0 and the line would not show [documented: item_mod_menu.xml; design: written
/// here]. Called with the texts measured and the tree worked out; nothing for a name that leaves
/// no room.
fn write_headline_line(ui: &mut Ui) {
    let (Some(line), Some(name), Some(words)) = (ui.find_tile(LINE), ui.find_tile(NAME), ui.find_tile(&format!("{NAME}/button_text"))) else { return };
    let read = |t, n| ui.tile(t).get(n).map_or(0.0, Value::num);
    let right_justified = matches!(ui.tile(words).get("justify"), Some(Value::Sym(s)) if s.eq_ignore_ascii_case("right"));
    // A right-justified text's `x` is its right end (`super::layout`).
    let left = read(name, "x") + read(words, "x") - if right_justified { read(words, "width") } else { 0.0 };
    let length = (left - NAME_GAP - read(line, "x")).max(0.0);
    // The last word on `_length` for this page: `write_frame_of` writes it too, as it moves the
    // line's start between the device and the screen menu, so the value now is compared, not a
    // cache of this writer's own last write.
    if ui.tile(line).get("_length").map(Value::num) != Some(length) {
        ui.set_trait(line, "_length", Value::Num(length));
    }
}

/// The longest start of `text`, whole words, that `height_of` gives no more than the height of
/// `lines` lines: the first `lines` lines of its wrapping, as a start of the words wraps as the
/// start of the whole. `height_of` measures a text in the tile's font and wrap.
pub(super) fn first_lines(text: &str, lines: usize, height_of: &mut dyn FnMut(&str) -> f64) -> String {
    let limit = height_of(&vec!["X"; lines.max(1)].join("\n"));
    if height_of(text) <= limit {
        return text.to_string();
    }
    // The ends of the words (before a space or a line break), the whole text last.
    let ends: Vec<usize> = text.char_indices().filter(|&(_, c)| c == ' ' || c == '\n').map(|(i, _)| i).chain([text.len()]).collect();
    let start = |k: usize| if k == 0 { "" } else { &text[..ends[k - 1]] };
    // `lo` words fit, `hi` do not.
    let (mut lo, mut hi) = (0, ends.len());
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if height_of(start(mid)) <= limit {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    start(lo).to_string()
}

/// The description at most the hand-built menu's lines ([`model::DESC_LINES`]): a text tile is
/// measured to its words and not clipped by the file's `height`, so a long one would run over the
/// cards below it [design: weapon-mods.md §5.1]. Left whole when the page cannot measure.
fn cut_description(ui: &mut Ui, desc: &str, fonts: &mut dyn PageFonts) {
    let Some(t) = ui.find_tile(DESC).filter(|_| !desc.is_empty()) else { return };
    let font = ui.tile(t).get("font").map_or(0.0, Value::num);
    let wrap = ui.tile(t).get("wrapwidth").map(Value::num).filter(|w| *w > 0.0);
    if fonts.text_size(font, "X", wrap).is_none() {
        return;
    }
    // Line breaks as the font sets them (`fnv_font::Font::lines`), so a cut before a break leaves
    // no bare carriage return.
    let whole = desc.replace("\r\n", "\n");
    let shown = first_lines(&whole, model::DESC_LINES, &mut |text| fonts.text_size(font, text, wrap).map_or(0.0, |s| s.1));
    if shown != desc {
        ui.set_trait(t, "string", Value::Str(shown));
    }
}

/// A line for the log when what the page shows changes (smoke reads these).
fn describe(ui: &Ui, s: &Shown) -> String {
    let first_line = |i: usize| s.rows.get(i).and_then(|r| r.text.lines().next()).unwrap_or("none");
    let effects = s.rows.iter().filter(|r| r.text.contains('\n')).count();
    format!(
        "tiles: {ITEM_MOD_MENU} drawn from tiles: {} rows ({effects} with an effect line); weapon {:?}; highlight {:?}; figures: {}",
        s.rows.len(),
        first_line(0),
        first_line(s.highlight),
        repair_cards::describe_of(ui, &cards()),
    )
}

/// One pass over the page; the menu when all of it went through. `late`: the pass before the
/// drawing; the other, before the scripts, does the sizes only for a model that changed.
pub(crate) fn page(ui: &mut Ui, s: Shown, device: bool, late: bool, tiles: &mut ItemModTiles, fonts: &mut dyn PageFonts) -> Result<TileId, String> {
    if let Tiled::No(reason) = built(ui) {
        return Err(reason);
    }
    let menu = ui.menu(ITEM_MOD_MENU).ok_or_else(|| format!("{ITEM_MOD_MENU} is not in the tree"))?;
    if tiles.written.menu != Some(menu) {
        tiles.written = Written { menu: Some(menu), ..Default::default() };
        // A held press on the old tree's marker goes with it.
        tiles.drag = None;
    }
    let w = &mut tiles.written;
    let changed = w.shown.as_ref() != Some(&s);
    if changed {
        write(ui, &s, w);
        cut_description(ui, &s.desc, fonts);
        w.shown = Some(s);
        w.tell = true;
    }
    if changed || late {
        let Some(s) = w.shown.take() else { return Err("nothing written".into()) };
        ui.update();
        write_file_sizes(ui, menu, &mut w.pictures, fonts);
        ui.update();
        let texts = menus::texts(ui, menu);
        // Three rounds: a size leads to a place and a place to a wrap width [design: as
        // `super::repair_menu::page`].
        for _ in 0..3 {
            if fonts.write_sizes(ui, &texts, &mut w.sizes) == 0 {
                break;
            }
            ui.update();
        }
        repair_cards::fit_meters_of(ui, &cards());
        if let (Some(title), Some(line)) = (ui.find_tile(TITLE), ui.find_tile(LINE)) {
            write_frame_of(ui, title, line, device, &mut w.flat);
        }
        write_headline_line(ui);
        if let Some(list_tile) = ui.find_tile(LIST) {
            list::settle_list(ui, list_tile, &w.rows, ROW_TEXT, &mut w.view, Some(s.highlight), 0, s.pointer_pick, s.navigated);
            w.view.rewrite(ui, list_tile);
        }
        write_brackets_of(ui, LIST, [LEFT_BRACKET, RIGHT_BRACKET], &w.rows, w.view.value);
        ui.update();
        super::item_menu::mark_menu(ui, ITEM_MOD_MENU, &[LIST]);
        w.shown = Some(s);
    } else if let Some(list_tile) = ui.find_tile(LIST) {
        w.view.rewrite(ui, list_tile);
    }
    if late {
        ui.update();
        let pieces = fonts.set_out(ui, menu);
        usable_of(ui, menu, (&tiles.written.rows, ROW_TEXT), CANCEL, &pieces, (ModTile::Row, ModTile::Cancel), |t| tiles.clicked(ui, t))?;
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
                info!("tiles: {ITEM_MOD_MENU} list scrolled to {value} of {most}");
            }
            w.drawn = now;
        }
    }
    Ok(menu)
}

/// What a pass over the page reads.
#[derive(SystemParam)]
pub(crate) struct ModPage<'w, 's> {
    script: Option<ResMut<'w, ScriptState>>,
    switch: Option<Res<'w, TileMenus>>,
    pip: Option<Res<'w, PipBoy>>,
    modding: Option<Res<'w, PipItemMod>>,
    combat: Option<Res<'w, crate::combat::Combat>>,
    play: Option<Res<'w, crate::PlayWorld>>,
    forms: Local<'s, Option<Forms>>,
    tiles: ResMut<'w, ItemModTiles>,
    fonts: FontKit<'w>,
}

/// Whether the page is eligible to prepare: the switch on and the mod menu up on ITEMS.
pub fn eligible(switch: TileMenus, pip: &PipBoy, open: bool) -> bool {
    switch.item_mod && open && pip.shown() && pip.page == Page::Items
}

/// A pass; `None` when the page is not eligible, or its tree is not there yet (nothing is
/// touched then).
fn pass(p: &mut ModPage<'_, '_>, late: bool) -> Option<Result<TileId, String>> {
    let pip = p.pip.as_deref()?;
    let session = p.modding.as_deref().and_then(|m| m.0.as_ref());
    if !eligible(p.switch.as_deref().copied().unwrap_or_default(), pip, session.is_some()) {
        p.tiles.closed();
        return None;
    }
    p.tiles.open = true;
    let session = session?;
    let device = pip.frame == Frame::Device;
    let cmp = p.combat.as_deref().and_then(|c| c.forms.weapons.get(&session.offer.item)).map(|w| model::compare(session, w));
    if p.forms.is_none() {
        *p.forms = p.play.as_ref().map(|w| Forms::new(w.lo, w.schema));
    }
    let desc = model::chosen(session).zip(p.forms.as_mut()).map(|(ch, f)| f.text(ch.item.0).description.clone());
    let game = &mut p.script.as_mut()?.rt.as_mut()?.game;
    game.ui_files.as_ref()?;
    if !game.menu_open(codes::ITEM_MOD) || game.ui_tree.menu(ITEM_MOD_MENU).is_none() {
        if late {
            p.tiles.no_tree_yet();
        }
        return None;
    }
    p.tiles.waiting = false;
    p.tiles.follow_model(session.selected, late);
    let mut shown = Shown::of(session, cmp.as_ref(), desc.as_deref(), p.tiles.pointer_pick, p.tiles.navigated, &Words::read(game));
    let add_icons = game.ysi_add_inventory_icons();
    shown.ysi_icons = add_icons;
    if add_icons {
        for r in &mut shown.rows {
            r.ysi = game.ysi_item_display(r.key.0);
        }
    }
    Some(page(&mut game.ui_tree, shown, device, late, &mut p.tiles, &mut p.fonts))
}

/// Before the scripts, after the tile input: this frame's model in the tree, and the scroll.
pub(crate) fn write_system(mut p: ModPage) {
    // A failure here is told by `settle`, which decides.
    let _ = pass(&mut p, false);
}

/// Before the drawing, after the model's keys: what they changed, the tree worked out, and the
/// one decision every reader of this frame shares.
pub(crate) fn settle(mut p: ModPage) {
    let mut why = None;
    let ready = match pass(&mut p, true) {
        Some(Ok(menu)) => Some(menu),
        Some(Err(reason)) => {
            let line = format!("{ITEM_MOD_MENU} drawn by hand: {reason}");
            if p.tiles.fallbacks.insert(line.clone()) {
                warn!("{line}");
            }
            why = Some(reason);
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
    let tiles = &mut *p.tiles;
    match (tiles.ready, ready) {
        (None, Some(_)) => {
            info!("tiles: {ITEM_MOD_MENU} ready: drawn from tiles");
            tiles.passes = 1;
        }
        (Some(_), Some(_)) => tiles.passes = tiles.passes.saturating_add(1),
        (Some(_), None) => {
            let why = why.unwrap_or_else(|| if tiles.open { "its tree is not there".into() } else { "closed".into() });
            info!("tiles: {ITEM_MOD_MENU} no longer drawn from tiles after {} passes: {why}", tiles.passes);
            tiles.passes = 0;
        }
        (None, None) => {}
    }
    if tiles.ready != ready {
        tiles.ready = ready;
    }
}

/// The menu, when the page takes this frame's tile input: painted last frame, and the page up,
/// with nothing over it.
pub fn takes(tiles: &ItemModTiles, switch: TileMenus, pip: &PipBoy, open: bool, game: &fnv_game::Game<'_>, paused: bool) -> Option<TileId> {
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

/// What the page's tile input reads of the Pip-Boy.
#[derive(SystemParam)]
pub(crate) struct FeedPip<'w> {
    pip: Res<'w, PipBoy>,
    screen: Option<Res<'w, PipBoyScreen>>,
    aid: Res<'w, PipBoyAid>,
    paused: Option<Res<'w, crate::pause::Paused>>,
    modding: Res<'w, PipItemMod>,
}

/// The page's tile input, on a frame it takes it ([`takes`]): see the module notes.
pub(crate) fn feed(mut script: ResMut<ScriptState>, switch: Res<TileMenus>, from: FeedPip, mut tiles: ResMut<ItemModTiles>, devices: Devices, mut out: MessageWriter<ModInput>, mut state: Local<Feed>) {
    if !switch.item_mod {
        return;
    }
    let FeedPip { pip, screen, aid, paused, modding } = from;
    let Some(rt) = script.rt.as_mut() else { return };
    let game = &mut rt.game;
    let open = modding.0.as_ref();
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
    let physical = frame_keys(&mut input, &devices, game, crate::pad::at_menu(codes::INVENTORY));
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
            if !menus::key_click_acts(ITEM_MOD_MENU, k, &physical) {
                continue;
            }
        }
        let Some(what) = tiles.clicked(ui, c.tile) else { continue };
        let ask = match what {
            ModTile::Row(index) => {
                let at_row = if row_acted { None } else { tiles.row_at(index) };
                let Some(index) = at_row else {
                    info!("tiles: {ITEM_MOD_MENU} a click on a row no longer there is ignored");
                    continue;
                };
                row_acted = true;
                tiles.asked = Some(index);
                ModInput::Click(index)
            }
            ModTile::Mod => ModInput::Use,
            ModTile::Cancel => ModInput::Cancel,
            ModTile::Bar(list::BarPiece::Marker) => {
                pressed_marker = true;
                let list_tile = ui.find_tile(LIST);
                tiles.drag = at.filter(|_| held && matches!(c.by, fnv_ui::input::By::Pointer)).zip(list_tile).and_then(|((_, y), l)| list::marker_grab(ui, menu, l, y));
                continue;
            }
            ModTile::Bar(piece) => {
                let value = tiles.written.view.value;
                if let Some(v) = ui.find_tile(LIST).and_then(|l| list::after_piece(ui, l, piece, value)) {
                    info!("tiles: {ITEM_MOD_MENU} {piece:?} on its tiles");
                    tiles.written.view.scroll_to(v);
                }
                continue;
            }
            ModTile::Other => continue,
        };
        info!("tiles: {ITEM_MOD_MENU} {ask:?} on its tiles");
        out.write(ask);
    }
    // A drag: the scroll under the pointer while the press holds.
    if let (false, Some(drag), Some((_, y))) = (pressed_marker, tiles.drag, at) {
        if let Some(v) = list::value_under(ui, drag, y) {
            tiles.written.view.scroll_to(v);
        }
    }
    // The tile under the pointer itself chooses its row, as the hand-built rows do; leaving the
    // rows keeps the choice, as there.
    let under = at.and_then(|at| fnv_ui::input::hit(ui, &[menu], at, &fnv_ui::input::no_measure)).and_then(|h| tiles.clicked(ui, h.tile));
    let under_row = if let Some(ModTile::Row(i)) = under { tiles.row_at(i) } else { None };
    if let Some(Some(i)) = follow_hover(&mut state.acted, under_row, settled) {
        tiles.asked = Some(i);
        if !row_acted && session.selected != i {
            info!("tiles: {ITEM_MOD_MENU} Hover({i}) on its tiles");
            out.write(ModInput::Hover(i));
        }
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
    use crate::pipboy::item_mod::{Choice, Offer};
    use fnv_combat::data::{WeaponMod, WeaponStats};
    use fnv_sim::inventory::Extra;
    use fnv_ui::MemSource;

    /// [design] A fixture written for these tests, with its own dimensions and simple dependencies
    /// (#1512). It keeps the tile and trait names this module looks up, which are the engine's
    /// interface; no value or expression comes from the game's file, and it says nothing about it.
    fn fixture(change: impl FnOnce(String) -> String) -> Ui {
        let mut ui = Ui::new();
        ui.load_menu(&MemSource::default().with("fixture.xml", &change(fixture_xml())), "fixture.xml").unwrap();
        ui.update();
        ui
    }

    fn fixture_xml() -> String {
        let card = |name: &str, inner: &str, x: u32, y: u32| format!(r#"<rect name="{name}"><x>{x}</x><y>{y}</y><text name="CI_TitleText"/><text name="CI_ValueText"/>{inner}</rect>"#);
        let meter = |name: &str| format!(r#"<rect name="{name}"><x>90</x><width>40</width><image name="MeterImage"/></rect>"#);
        let button = |name: &str, words: &str, y: u32, hide: &str| {
            format!(
                r#"<image name="{name}"><y>{y}</y>
            <width><copy src="child(button_text)" trait="width"/><add>12</add></width>
            <height><copy src="child(TestPicture)" trait="fileheight"/><add>6</add></height>
            <target>1</target>{hide}<text name="button_text"><y>2</y><string>{words}</string></text>
            <image name="TestPicture"><visible>0</visible><filename>pics\button.dds</filename></image>
            <text name="PCShortcutLabel"><depth>-3</depth><x><copy src="parent()" trait="width"/><add>8</add></x>
              <string><copy src="parent()" trait="_PCButtonText"/></string></text></image>"#
            )
        };
        format!(
            r#"<menu name="ItemModMenu"><rect name="NOGLOW_BRANCH"/><rect name="GLOW_BRANCH"><image name="WMM_Background"/><rect name="IMM_MainRect"><x>20</x><y>30</y>
              <rect name="IMM_HeadlineRect"><text name="IMM_Headline_Title"><string>Own heading</string></text><image name="IMM_Headline_h2"><x>150</x><_length>200</_length></image>
                <rect name="IMM_ItemName"><x>300</x><text name="button_text"><x>200</x><justify>&right;</justify><string><copy src="parent()" trait="string"/></string></text></rect></rect>
              <hotrect name="IMM_ItemModList"><y>60</y><width>260</width><height>180</height><clipwindow>1</clipwindow><_scroll_delta>11</_scroll_delta>
                <_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>
                <hotrect name="lb_highlight_box"/>
                <image name="lb_scrollbar"><_current_value>0</_current_value>
                  <image name="scrollbar_vert_marker"/><image name="scrollbar_vert_up"/><image name="scrollbar_vert_down"/>
                  <hotrect name="scrollbar_vert_page_up"/><hotrect name="scrollbar_vert_page_down"/>
                </image>
                <rect name="IMM_LeftBracket"/><rect name="IMM_RightBracket"/>
              </hotrect>
              <image name="IMM_ItemIcon"><x>320</x></image>
              <text name="IMM_ModDesc"><x>320</x><y>120</y></text>
              <rect name="IMM_CompareRect"><x>300</x><y>250</y>{}{}<image name="IMM_ArrowIcon"/><text name="IMM_HealthImprovementText"/><text name="IMM_StatImprovementText"/><text name="IMM_ChooseItemText"/>{}{}</rect>
              <rect name="IMM_ButtonRect"><x>620</x><y>60</y>{}{}</rect>
              </rect></rect>
              <template name="IMM_ItemModListTemplate"><hotrect name="IMM_ItemModListTemplateRect"><width>240</width><target>1</target>
                <_y>0</_y><_VerticalSpacing>6</_VerticalSpacing>
                <y><copy src="me()" trait="_y"/><sub src="parent()" trait="_offset"/></y>
                <text name="ListItemText"><x>30</x></text><image name="IMM_Template_ItemMarker"/>
              </hotrect></template></menu>"#,
            card("IMM_ItemHealth", &meter("IMM_ItemHealthMeter"), 0, 0),
            card("IMM_ItemStat", "", 150, 0),
            card("IMM_ModItemHealth", &meter("IMM_ModItemHealthMeter"), 0, 100),
            card("IM_ModItemStat", "", 150, 100),
            button("IMM_ItemModButton", "Own mod", 0, "<visible>0</visible>"),
            button("IMM_CancelButton", "Own leave", 30, ""),
        )
    }

    const GUN: FormId = FormId(0x10);
    const MAGS: FormId = FormId(0x21);
    const SIGHT: FormId = FormId(0x22);
    const QUIET: FormId = FormId(0x23);

    fn weapon() -> WeaponStats {
        let m = |slot, item, effect: &str, a| WeaponMod { slot, item: Some(item), effect: Some(effect.into()), value_a: a, value_b: 0.0 };
        WeaponStats { base_damage: 15, clip_size: 8, min_spread: 0.9, mods: vec![m(1, MAGS, "Increase Clip Capacity", 3.0), m(2, SIGHT, "Decrease Spread", 0.5), m(3, QUIET, "Silence", 0.0)], ..Default::default() }
    }

    fn choice(item: FormId, slot: u8, name: &str, count: i32, attached: bool, effect: Option<&str>) -> Choice {
        Choice { item, slot, name: name.into(), count, icon: Some(format!("icons\\{slot}.dds")), attached, effect: effect.map(str::to_string) }
    }

    /// The weapon, worn, with its magazines on and two carried mods: a spread one and a quiet one.
    fn session(selected: usize) -> Session {
        let extra = Extra { equipped: true, mods: 1, condition: Some(0.5), ..Extra::default() };
        Session {
            offer: Offer {
                item: GUN,
                extra,
                name: "Own gun +".into(),
                icon: Some("icons\\gun.dds".into()),
                choices: vec![choice(MAGS, 1, "Own mags", 0, true, Some("Capacity up")), choice(QUIET, 3, "Own quiet", 1, false, None), choice(SIGHT, 2, "Own sight", 3, false, Some("Spread down"))],
            },
            first: 0,
            selected,
        }
    }

    fn words() -> Words {
        Words { condition: "State".into(), cancel_hint: "[E]".into() }
    }

    fn shown(selected: usize) -> Shown {
        let s = session(selected);
        let cmp = model::compare(&s, &weapon());
        Shown::of(&s, Some(&cmp), Some("Own words."), false, 0, &words())
    }

    struct TestFonts;

    /// Eight units a letter, 20 a line, lines by the line breaks and the wrap.
    fn measure(_: f64, text: &str, wrap: Option<f64>) -> (f64, f64) {
        let lines: Vec<f64> = text.split('\n').map(|l| l.chars().count() as f64 * 8.0).collect();
        let width = lines.iter().copied().fold(0.0, f64::max);
        (wrap.filter(|w| *w > 0.0).unwrap_or(1000.0).min(width), 20.0 * lines.len() as f64)
    }

    impl PageFonts for TestFonts {
        fn write_sizes(&mut self, ui: &mut Ui, texts: &[TileId], written: &mut SizesWritten) -> usize {
            menus::write_text_sizes_of(ui, texts, written, &measure)
        }
        fn set_out(&mut self, ui: &Ui, menu: TileId) -> Vec<Piece> {
            super::super::draw::pieces(ui, menu, &measure, super::super::draw::Select::Whole { kept: &[], strip: None }, true).0
        }
        fn picture_texels(&mut self, name: &str) -> Option<(f64, f64)> {
            (name == "pics\\button.dds").then_some((32.0, 24.0))
        }
        fn text_size(&mut self, font: f64, text: &str, wrap: Option<f64>) -> Option<(f64, f64)> {
            Some(measure(font, text, wrap))
        }
    }

    fn run(ui: &mut Ui, s: Shown, tiles: &mut ItemModTiles) -> Result<TileId, String> {
        page(ui, s, true, true, tiles, &mut TestFonts)
    }

    fn number(ui: &Ui, path: &str, name: &str) -> f64 {
        ui.get_float(&format!("{path}/{name}")).unwrap()
    }

    fn string(ui: &Ui, path: &str, name: &str) -> String {
        ui.get_string(&format!("{path}/{name}")).unwrap()
    }

    fn card(name: &str) -> String {
        format!("{COMPARE}/{name}")
    }

    #[test]
    fn the_switch_is_off_by_default_and_only_the_open_menu_on_items_is_eligible() {
        let up_on = |page| {
            let mut pip = PipBoy::default();
            pip.phase = crate::pipboy::Phase::Up;
            pip.page = page;
            pip
        };
        let pip = up_on(Page::Items);
        let on = TileMenus { item_mod: true, ..TileMenus::default() };
        assert!(!eligible(TileMenus::default(), &pip, true), "off: the hand-built menu");
        assert!(!eligible(TileMenus { inventory: true, repair_menu: true, ..TileMenus::default() }, &pip, true), "other pages' switches are not this one");
        assert!(!eligible(on, &pip, false), "the menu closed");
        assert!(eligible(on, &pip, true));
        assert!(!eligible(on, &up_on(Page::Data), true));
        assert!(!eligible(on, &PipBoy::default(), true), "the Pip-Boy down");
    }

    #[test]
    fn a_complete_file_builds_and_a_cut_down_one_falls_back_whole() {
        assert_eq!(built(&fixture(|s| s)), Tiled::Yes);
        for missing in ["WMM_Background", "IMM_Headline_h2", "IMM_ItemName", "IMM_LeftBracket", "IMM_ModDesc", "IMM_ModItemHealthMeter", "IM_ModItemStat", "CI_ValueText", "PCShortcutLabel", "scrollbar_vert_marker", "IMM_ItemModListTemplateRect", "IMM_Template_ItemMarker"] {
            let ui = fixture(|s| s.replace(missing, "Absent"));
            assert!(!built(&ui).yes(), "{missing}");
        }
        assert!(!built(&Ui::new()).yes());
        let mut ui = fixture(|s| s.replace("IMM_ModDesc", "Absent"));
        let reason = run(&mut ui, shown(0), &mut ItemModTiles::default()).unwrap_err();
        assert_eq!(reason, format!("{DESC} missing"), "the reason names the path");
    }

    #[test]
    fn the_weapon_stands_first_framed_and_each_mod_says_what_it_does_under_its_name() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        run(&mut ui, shown(0), &mut tiles).unwrap();
        let rows = &tiles.written.rows;
        assert_eq!(rows.len(), 4);
        let child = |i: usize, name| list::child(&ui, rows[i].tile, name).unwrap();
        let text = |i: usize| ui.tile(child(i, ROW_TEXT)).get("string").unwrap().text();
        assert_eq!(text(0), "Own gun +");
        assert_eq!(text(1), "Own mags\nCapacity up", "an attached mod: no count, its effect under it");
        assert_eq!(text(2), "Own quiet", "no effect in words: one line");
        assert_eq!(text(3), "Own sight (3)\nSpread down", "a carried mod's count");
        assert_eq!(ui.tile(rows[0].tile).name, "Own gun +", "a row is named after what it stands for");
        assert_eq!(ui.tile(rows[3].tile).name, "Own sight (3)");
        let flag = |i: usize, name: &str| ui.tile(rows[i].tile).get(name).unwrap().num();
        assert_eq!((flag(0, "_IsItemModItem"), flag(1, "_IsItemModItem")), (1.0, 0.0));
        let height = |i: usize| flag(i, "height");
        assert_eq!(height(1), height(0) + 20.0, "the effect's line makes its row a line taller");
        assert_eq!(height(2), height(0));
        let mark = |i: usize| ui.tile(child(i, ROW_MARK)).get("visible").unwrap().num();
        assert_eq!((mark(0), mark(1), mark(2), mark(3)), (1.0, 1.0, 0.0, 0.0), "the worn weapon and the mod on it");
        for bracket in [LEFT_BRACKET, RIGHT_BRACKET] {
            assert_eq!(number(&ui, bracket, "y"), 0.0, "{bracket}");
            assert_eq!(number(&ui, bracket, "height"), height(0) - 1.0, "{bracket}");
        }
        assert_eq!(number(&ui, LIST, "_highlight_y"), 0.0, "the weapon's row chosen");
        assert_eq!(string(&ui, NAME, "string"), "Own gun +");
        assert_eq!(string(&ui, ICON, "filename"), "icons\\gun.dds");
        assert_eq!(string(&ui, DESC, "string"), "", "no mod chosen, no description");
        assert_eq!(number(&ui, BACKGROUND, "visible"), 0.0, "the Pip-Boy's own backing");
        assert_eq!(string(&ui, TITLE, "string"), "Own heading", "the file's title");
        assert_eq!(number(&ui, MOD, "visible"), 0.0, "the file's, hidden on a PC");
        assert_eq!(number(&ui, MOD, "target"), 0.0, "nothing to attach on the weapon's row");
        assert_eq!(string(&ui, CANCEL, "_PCButtonText"), "[E]");
    }

    #[test]
    fn the_weapons_row_shows_its_cards_alone() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        run(&mut ui, shown(0), &mut tiles).unwrap();
        assert_eq!(number(&ui, &card("IMM_ItemHealth"), "user0"), 0.5);
        assert_eq!(string(&ui, &card("IMM_ItemHealth"), "_Title"), "State", "the setting where the file gives none");
        assert_eq!(string(&ui, &card("IMM_ItemStat"), "_Value"), "15", "the damage");
        for hidden in ["IMM_ModItemHealth", "IM_ModItemStat", "IMM_HealthImprovementText", "IMM_StatImprovementText", "IMM_ChooseItemText"] {
            assert_eq!(number(&ui, &card(hidden), "visible"), 0.0, "{hidden}");
        }
    }

    #[test]
    fn decimal_negative_and_worded_figures_show_as_the_model_words_them() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        // The spread mod: decimals and a fall.
        run(&mut ui, shown(3), &mut tiles).unwrap();
        assert_eq!(string(&ui, &card("IMM_ItemStat"), "_Title"), "Spread");
        assert_eq!(string(&ui, &card("IMM_ItemStat"), "_Value"), "0.90");
        assert_eq!(string(&ui, &card("IM_ModItemStat"), "_Value"), "0.40");
        assert_eq!(number(&ui, &card("IM_ModItemStat"), "visible"), 1.0);
        assert_eq!(string(&ui, &card("IMM_StatImprovementText"), "string"), "-0.50", "negative, not \"+-\"");
        assert_eq!(number(&ui, &card("IMM_HealthImprovementText"), "visible"), 0.0, "a mod leaves the condition");
        assert_eq!(number(&ui, &card("IMM_ModItemHealth"), "user0"), 0.5);
        assert_eq!(string(&ui, DESC, "string"), "Own words.");
        assert_eq!(string(&ui, ICON, "filename"), "icons\\2.dds", "the chosen mod's picture");
        assert_eq!(number(&ui, MOD, "target"), 1.0);
        // The quiet mod: words, no gain.
        run(&mut ui, shown(2), &mut tiles).unwrap();
        assert_eq!(string(&ui, &card("IMM_ItemStat"), "_Title"), "Noise");
        assert_eq!(string(&ui, &card("IM_ModItemStat"), "_Value"), "Silent");
        assert_eq!(number(&ui, &card("IMM_StatImprovementText"), "visible"), 0.0, "words have no gain");
        assert_eq!(string(&ui, &card("IMM_StatImprovementText"), "string"), "", "nothing stays of the row before");
        // The mags, attached: taking them off is a whole fall.
        run(&mut ui, shown(1), &mut tiles).unwrap();
        assert_eq!((string(&ui, &card("IMM_ItemStat"), "_Value"), string(&ui, &card("IM_ModItemStat"), "_Value")), ("11".to_string(), "8".to_string()));
        assert_eq!(string(&ui, &card("IMM_StatImprovementText"), "string"), "-3");
        let line = describe(&ui, tiles.written.shown.as_ref().unwrap());
        assert_eq!(line, "tiles: ItemModMenu drawn from tiles: 4 rows (2 with an effect line); weapon \"Own gun +\"; highlight \"Own mags\"; figures: State 50%, Clip 11, State 50%, Clip 8, -3");
    }

    #[test]
    fn no_weapon_record_hides_every_card() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        run(&mut ui, Shown::of(&session(3), None, None, false, 0, &words()), &mut tiles).unwrap();
        for hidden in ["IMM_ItemHealth", "IMM_ItemStat", "IMM_ModItemHealth", "IM_ModItemStat"] {
            assert_eq!(number(&ui, &card(hidden), "visible"), 0.0, "{hidden}");
        }
    }

    #[test]
    fn a_tree_built_again_starts_over_and_keeps_no_tile_of_the_old_one() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        let first = run(&mut ui, shown(1), &mut tiles).unwrap();
        let old_rows: Vec<TileId> = tiles.written.rows.iter().map(|r| r.tile).collect();
        ui.unload_tile(first);
        ui.load_menu(&MemSource::default().with("fixture.xml", &fixture_xml()), "fixture.xml").unwrap();
        ui.update();
        let again = run(&mut ui, shown(1), &mut tiles).unwrap();
        assert_ne!(first, again);
        assert_eq!(tiles.written.menu, Some(again));
        assert_eq!(tiles.written.rows.len(), 4);
        for r in &tiles.written.rows {
            assert!(ui.is_live(r.tile) && ui.root_of(r.tile) == again, "rows of the new tree");
            assert!(!old_rows.contains(&r.tile));
        }
        assert_eq!(string(&ui, &card("IM_ModItemStat"), "_Value"), "8", "written whole again");
        assert!(old_rows.iter().all(|&t| tiles.clicked(&ui, t).is_none()), "an old row's click acts on nothing");
    }

    #[test]
    fn clicks_stand_for_their_tiles_and_a_row_gone_acts_on_nothing() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        run(&mut ui, shown(3), &mut tiles).unwrap();
        let third = tiles.written.rows[3].tile;
        assert_eq!(tiles.clicked(&ui, third), Some(ModTile::Row(3)));
        assert_eq!(tiles.row_at(3), Some(3));
        for (path, what) in [(format!("{CANCEL}/button_text"), ModTile::Cancel), (MOD.to_string(), ModTile::Mod), (format!("{LIST}/lb_scrollbar/scrollbar_vert_down"), ModTile::Bar(list::BarPiece::Down)), (ICON.to_string(), ModTile::Other)] {
            assert_eq!(tiles.clicked(&ui, ui.find_tile(&path).unwrap()), Some(what), "{path}");
        }
        let mut less = session(0);
        less.offer.choices.pop();
        run(&mut ui, Shown::of(&less, None, None, false, 0, &words()), &mut tiles).unwrap();
        assert!(!ui.is_live(third));
        assert_eq!(tiles.clicked(&ui, third), None);
        assert_eq!(tiles.row_at(3), None);
    }

    #[test]
    fn every_row_and_cancel_take_the_pointer_and_a_covered_cancel_falls_back() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        let menu = run(&mut ui, shown(0), &mut tiles).unwrap();
        assert!(!tiles.pieces.is_empty());
        let words = ui.find_tile(&format!("{CANCEL}/button_text")).unwrap();
        let piece = tiles.pieces.iter().find(|p| p.tile == words).unwrap();
        let hit = fnv_ui::input::hit(&ui, &[menu], super::super::stats_menu::shows_whole(piece).unwrap(), &fnv_ui::input::no_measure).unwrap();
        assert_eq!(tiles.clicked(&ui, hit.tile), Some(ModTile::Cancel));
        let cover = r#"<rect name="IMM_ButtonRect"><x>620</x><y>60</y><hotrect name="TestCover"><target>1</target><depth>8</depth><y>30</y><width>100</width><height>30</height></hotrect>"#;
        let (mut ui, mut tiles) = (fixture(|s| s.replace(r#"<rect name="IMM_ButtonRect"><x>620</x><y>60</y>"#, cover)), ItemModTiles::default());
        let reason = run(&mut ui, shown(0), &mut tiles).unwrap_err();
        assert!(reason.starts_with("Cancel does not take the pointer:"), "{reason}");
    }

    #[test]
    fn the_screen_menu_hides_the_title_and_gives_it_back_on_the_device() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        page(&mut ui, shown(0), false, true, &mut tiles, &mut TestFonts).unwrap();
        assert_eq!(number(&ui, TITLE, "visible"), 0.0);
        page(&mut ui, shown(0), true, true, &mut tiles, &mut TestFonts).unwrap();
        assert_ne!(ui.get_float(&format!("{TITLE}/visible")), Some(0.0));
        assert_eq!(number(&ui, LINE, "x"), 150.0);
    }

    /// The fixture's own places: the line from 150, the name's words right-justified at 300 + 200,
    /// eight units a letter.
    #[test]
    fn the_headline_line_runs_from_the_title_to_before_the_name() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        run(&mut ui, shown(0), &mut tiles).unwrap();
        let name_left = 300.0 + 200.0 - 8.0 * "Own gun +".len() as f64;
        assert_eq!(number(&ui, LINE, "_length"), name_left - NAME_GAP - 150.0, "the file's own reads a tile it lacks");
        // A longer name moves the end; the screen menu starts the line at the lead line's end.
        let mut s = session(0);
        s.offer.name = "Own much longer gun +".into();
        let longer = Shown::of(&s, None, None, false, 0, &words());
        run(&mut ui, longer.clone(), &mut tiles).unwrap();
        let name_left = 300.0 + 200.0 - 8.0 * "Own much longer gun +".len() as f64;
        assert_eq!(number(&ui, LINE, "_length"), name_left - NAME_GAP - 150.0);
        page(&mut ui, longer, false, true, &mut tiles, &mut TestFonts).unwrap();
        let from = f64::from(crate::pipboy::frame::lead_line_end());
        assert_eq!(number(&ui, LINE, "x"), from);
        assert_eq!(number(&ui, LINE, "_length"), (name_left - NAME_GAP - from).max(0.0));
        // A name reaching the title leaves no line.
        s.offer.name = "W".repeat(60);
        run(&mut ui, Shown::of(&s, None, None, false, 0, &words()), &mut tiles).unwrap();
        assert_eq!(number(&ui, LINE, "_length"), 0.0);
    }

    /// No room for the line, through device, screen menu and device again: the screen menu's own
    /// write of `_length` (`write_frame_of`, as it moves the line's start) does not stand.
    #[test]
    fn a_name_leaving_no_room_keeps_no_line_across_the_frames() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        let mut s = session(0);
        s.offer.name = "W".repeat(60);
        let long = Shown::of(&s, None, None, false, 0, &words());
        for device in [true, false, true, false, true] {
            page(&mut ui, long.clone(), device, true, &mut tiles, &mut TestFonts).unwrap();
            assert_eq!(number(&ui, LINE, "_length"), 0.0, "device {device}");
        }
        // And with room again after it, on the screen menu.
        let short = shown(0);
        page(&mut ui, short, false, true, &mut tiles, &mut TestFonts).unwrap();
        let from = f64::from(crate::pipboy::frame::lead_line_end());
        let name_left = 300.0 + 200.0 - 8.0 * "Own gun +".len() as f64;
        assert_eq!(number(&ui, LINE, "_length"), (name_left - NAME_GAP - from).max(0.0));
    }

    #[test]
    fn a_description_with_carriage_returns_is_cut_with_none_left() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        let long = Shown::of(&session(3), None, Some("one\r\ntwo\r\nthree\r\nfour"), false, 0, &words());
        run(&mut ui, long, &mut tiles).unwrap();
        assert_eq!(string(&ui, DESC, "string"), "one\ntwo\nthree");
        let short = Shown::of(&session(3), None, Some("one\r\ntwo"), false, 0, &words());
        run(&mut ui, short, &mut tiles).unwrap();
        assert_eq!(string(&ui, DESC, "string"), "one\ntwo", "a short one loses its carriage returns too");
    }

    #[test]
    fn who_moved_the_selection_decides_the_scroll() {
        let mut tiles = ItemModTiles::default();
        tiles.follow_model(0, true);
        assert!(!tiles.pointer_pick);
        // The pointer asks for row 2 (`feed`); the early pass sees no change yet, the late one
        // the model's.
        tiles.asked = Some(2);
        tiles.follow_model(0, false);
        assert_eq!(tiles.asked, Some(2), "kept until the model has applied it");
        tiles.follow_model(2, true);
        assert!(tiles.pointer_pick, "the pointer's pick leaves the list");
        assert_eq!(tiles.asked, None, "used up");
        let before = tiles.navigated;
        tiles.follow_model(3, true);
        assert!(!tiles.pointer_pick);
        assert_eq!(tiles.navigated, before + 1, "a key's move brings its row into view");
        tiles.follow_model(2, true);
        assert!(!tiles.pointer_pick, "a key back onto the row the pointer once chose is the keys'");
        assert_eq!(tiles.navigated, before + 2);
        // A hover on the row already chosen changes nothing; a later key's return is the keys'.
        tiles.asked = Some(2);
        tiles.follow_model(2, true);
        tiles.follow_model(1, true);
        tiles.follow_model(2, true);
        assert!(!tiles.pointer_pick);
        tiles.closed();
        assert_eq!(tiles.selected, None);
    }

    #[test]
    fn a_long_description_keeps_the_hand_built_lines_and_stays_off_the_cards() {
        // Ten units a letter at a wrap of 100: as many words a line as fit in ten letters.
        let mut wrapped = |text: &str| {
            let mut lines = 0u32;
            for para in text.split('\n') {
                let mut len = 0usize;
                lines += 1;
                for w in para.split(' ') {
                    let add = if len == 0 { w.len() } else { w.len() + 1 };
                    if len > 0 && len + add > 10 {
                        lines += 1;
                        len = w.len();
                    } else {
                        len += add;
                    }
                }
            }
            20.0 * f64::from(lines)
        };
        assert_eq!(first_lines("short words", 3, &mut wrapped), "short words");
        let text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh";
        assert_eq!(first_lines(text, 3, &mut wrapped), "aaaa bbbb cccc dddd eeee ffff", "two words a line, three lines");
        assert_eq!(first_lines("", 3, &mut wrapped), "");
        // On the page: the fixture's description has no wrap, so its lines are its line breaks.
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        let s = session(3);
        let long = Shown::of(&s, None, Some("one\ntwo\nthree\nfour\nfive"), false, 0, &words());
        run(&mut ui, long, &mut tiles).unwrap();
        assert_eq!(string(&ui, DESC, "string"), "one\ntwo\nthree", "the hand-built three lines");
        assert_eq!(number(&ui, DESC, "height"), 60.0);
    }

    #[test]
    fn the_hand_built_menu_waits_for_the_tree_only_so_long() {
        let mut tiles = ItemModTiles::default();
        assert!(!tiles.waiting);
        for _ in 0..TREE_WAIT {
            tiles.no_tree_yet();
            assert!(tiles.waiting);
        }
        tiles.no_tree_yet();
        assert!(!tiles.waiting, "a file that cannot be built leaves the hand-built menu the pointer");
        tiles.closed();
        tiles.no_tree_yet();
        assert!(tiles.waiting, "the next opening waits again");
    }

    #[test]
    fn the_same_state_writes_nothing_new() {
        let (mut ui, mut tiles) = (fixture(|s| s), ItemModTiles::default());
        run(&mut ui, shown(1), &mut tiles).unwrap();
        let rows: Vec<TileId> = tiles.written.rows.iter().map(|r| r.tile).collect();
        run(&mut ui, shown(1), &mut tiles).unwrap();
        assert_eq!(tiles.written.rows.iter().map(|r| r.tile).collect::<Vec<_>>(), rows);
    }

    #[test]
    fn physical_keys_act_once_and_scripts_keep_the_file_bindings() {
        for key in ["E", "W", "xdown", "xbuttonb"] {
            assert!(!menus::key_click_acts(ITEM_MOD_MENU, key, &[key]), "{key}");
            assert!(menus::key_click_acts(ITEM_MOD_MENU, key, &[]), "{key}");
        }
    }
}
