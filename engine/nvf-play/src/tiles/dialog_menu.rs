//! The dialogue menu drawn from `menus\dialog\dialog_menu.xml`, behind
//! `[UI] TileMenus=DialogMenu` [design: ui-tile-tree.md §6.2 step 3; the contract:
//! docs/spec-notes/dialogue-menu.md §6].
//!
//! - **The switch** (`super::vanilla::TileMenus::dialog`): on, with the game's menu files
//!   read, `DialogMenu` is in the tree while a conversation runs (`Game::dialogue`; a
//!   conversation never sets `Game::menu`) and `super::draw` draws its whole tree at the
//!   hand-built menu's stacking level, the hand-built menu (`crate::dialogue_menu`) not
//!   spawned. Off (the default), none of this runs.
//! - **The rules stay in `crate::dialogue`**: it says what the menu shows (`dialogue::view`)
//!   and which choice is selected. This module only writes that into the tree ([`write`]).
//! - **When**: so scripts read what the model says and keep what they write, the model's view
//!   is written at each transition of the conversation, as it happens, inside the runtime's
//!   own work: before a result script of the line runs, when a response moves on and when
//!   choices are offered (`Game::ui_dialogue_hook`, [`boundary`]), which covers a conversation
//!   started or a choice made anywhere, a script's included. A frame's [`publish_system`] does
//!   the same before its scripts, for what the model changes outside a transition. Each trait is
//!   written only when the model's value for it changed: a script's change to a label, an alpha
//!   or a flag stands until the model changes that very thing, and a new selection changes none
//!   of them. [`write_system`], after the scripts, measures, places and decides readiness, and
//!   `super::update` works the tree out to be drawn. A conversation that starts this frame
//!   builds the menu there at the latest, so its first frame is the tiles' too. Placement
//!   ([`place`]) reads the file's evaluated numbers, so they are part of what makes it run
//!   again ([`Placement`]).
//! - **The scroll** ([`place`]) goes by whole rows, as the hand-built menu's [owner choice
//!   2026-11-05: whole rows, as the hand-built menu; the original's 56-unit steps and mid-glyph
//!   clipping, observed in the capture, read as a jump; `dialogue-menu.md` §6.3, §6.6]: the
//!   scroll is the first row shown whole at the window's top, moved a row by the bar's arrows,
//!   by the rows that fit by its page areas and, for a choice the keys move to, by as few rows as
//!   show it whole. Only the window's foot can cut a row. The file still moves the rows by
//!   `_current_value × _scroll_delta`: code writes a delta of 1 and the first row's place as the
//!   value. The prefab's own value counts clicks from 0 in this tree, as the wait and tutorial
//!   menus' do, so the model's is written on every frame.
//! - **Readiness** ([`DialogTiles`], `dialogue-menu.md` §6.5): one decision for the tile
//!   drawing, the hand-built menu and the tile input. A file missing, cut down or restructured,
//!   a template that makes no row with its text, or a text that does not measure gives the
//!   whole hand-built menu: the rows made are unloaded, the reason warned once, and the
//!   preparation tried again next frame, as a font or a file may come.
//! - **The input** ([`follow`]): the tile input feeds the menu while it is drawn, nothing over
//!   it and no pause menu ([`DialogTiles::taking`]), and a pointer's or a script's click comes
//!   back as `crate::dialogue::ViewInput`: a row picks its choice, the pointer coming onto a
//!   row selects it, the click area and the line skip; the scroll bar's arrows and page areas
//!   scroll, which is the view's own. The keys and the wheel stay `crate::dialogue`'s. A click is held to
//!   the screen it was made on, a script's to the one it was queued on (`Click::stamp`), and a
//!   real press while the actor speaks skips once whichever way it arrives.
//! - **NVForever's own tiles**: `NVF_GoodbyeHint` and `NVF_SelectionFill`, from templates
//!   written here (`dialogue-menu.md` §6.4). The hint reads its wording and whether it shows
//!   from the same model and the same "Goodbye hint" switch as the hand-built one.

use super::menus::{self, SizesWritten, Tiled};
use crate::dialogue::{DialogueScreen, ListId, ViewInput};
use crate::dialogue_menu::{goodbye_hint, hint_shown, GoodbyeHint, MenuView};
#[cfg(test)]
use crate::dialogue_menu::GOODBYE_HINT;
use bevy::prelude::*;
use fnv_ui::input::{By, HitCandidate, Report};
use fnv_ui::{MemSource, Origin, TileId, Ui, Value};

/// The menu's name [observed: the vanilla files, by fnvtool ui tree].
pub const DIALOG_MENU: &str = "DialogMenu";

// The tiles the model writes [observed: the vanilla files, by fnvtool ui tree].
const ROOT: &str = "DialogMenu/NOGLOW_BRANCH";
const BAND: &str = "DialogMenu/NOGLOW_BRANCH/DM_TextBackground";
const CLICK_AREA: &str = "DialogMenu/NOGLOW_BRANCH/DM_ClickRect";
const NAME: &str = "DialogMenu/NOGLOW_BRANCH/DM_SpeakerNameLabel";
const LINE: &str = "DialogMenu/NOGLOW_BRANCH/DM_SpeakerText";
const LIST: &str = "DialogMenu/NOGLOW_BRANCH/DM_TopicList";
const HIGHLIGHT: &str = "DialogMenu/NOGLOW_BRANCH/DM_TopicList/lb_highlight_box";
const BAR: &str = "DialogMenu/NOGLOW_BRANCH/DM_TopicList/lb_scrollbar";
const TOP_BRACKET: &str = "DialogMenu/NOGLOW_BRANCH/DM_TopBracket";
const BOTTOM_BRACKET: &str = "DialogMenu/NOGLOW_BRANCH/DM_BottomBracket";
const FRAME_LINES: [&str; 4] = ["top", "left", "bottom", "right"];
const BAR_CONTROLS: [&str; 5] = ["scrollbar_vert_up", "scrollbar_vert_down", "scrollbar_vert_page_up", "scrollbar_vert_page_down", "scrollbar_vert_marker"];
const ROW_TEMPLATE: &str = "DM_TopicTemplate";
const ROW: &str = "DM_Topic";
const ROW_TEXT: &str = "ListItemText";

// NVForever's own tiles, not the file's (`dialogue-menu.md` §6.4).
const FILL_TEMPLATE: &str = "NVF_SelectionFillTemplate";
const HINT_TEMPLATE: &str = "NVF_GoodbyeHintTemplate";
const FILL: &str = "DialogMenu/NOGLOW_BRANCH/DM_TopicList/lb_highlight_box/NVF_SelectionFill";
const HINT: &str = "DialogMenu/NOGLOW_BRANCH/NVF_GoodbyeHint";
const HINT_BOX: &str = "DialogMenu/NOGLOW_BRANCH/NVF_GoodbyeHint/NVF_GoodbyeHintBox";
const HINT_TEXT: &str = "DialogMenu/NOGLOW_BRANCH/NVF_GoodbyeHint/NVF_GoodbyeHintText";
const OWN_TEMPLATES: &str = r#"<template name="NVF_SelectionFillTemplate"><rect name="NVF_SelectionFill">
  <red>0</red><green>0</green><blue>0</blue></rect></template>
<template name="NVF_GoodbyeHintTemplate"><rect name="NVF_GoodbyeHint"><depth>3</depth><visible>0</visible>
  <image name="NVF_GoodbyeHintBox"><filename>Interface\Shared\Background\solid_black.dds</filename><zoom>-1</zoom></image>
  <text name="NVF_GoodbyeHintText"><font>6</font><depth>1</depth></text></rect></template>"#;

/// The hint's right end stands this far left of the band and its dark box reaches this far
/// round the text [design: owner list 5 item 62, as the hand-built hint stood before #1525].
const HINT_GAP: f64 = 20.0;
const HINT_PAD: f64 = 10.0;

/// A said-before row's `_line_alpha`, and so its text's alpha, the text's brightness staying
/// 255 [observed: capture of the original, private job dialogue, 1280 x 720, one row].
const DIM_ALPHA: f64 = 128.0;
/// The band's alpha, read while drawn; the file writes less [observed: capture of the original, private job dialogue, 1280 x 720].
const BAND_ALPHA: f64 = 193.8;
/// `screen()`'s `cropx` and `cropy` [observed: capture of the original, private job dialogue, 1280 x 720: 15 by arithmetic from the
/// name's place at one aspect, not a read of `screen()`; hypothesis: the same at other aspects].
const CROP: f64 = 15.0;
/// The `_scroll_delta` code writes at open: 1, so the bar's value is the rows' offset in
/// interface units and the scroll can stop on any row's top [design: owner choice 2026-11-05,
/// whole rows]. The original wrote 56 on a list of 12 rows of 58 and one of 96 [observed:
/// capture of the original, private job dialogue, 1280 x 720, `dialogue-menu.md` §6.6]. A
/// script's own value is read: the value and the bar's counts are written in its units.
const SCROLL_DELTA: f64 = 1.0;
/// Slack in comparing interface units, which measured heights carry as fractions [design].
const EPS: f64 = 1e-6;
/// The rows the list shows, whose heights make its height: three, or the file's least height
/// if more [observed: capture of the original, private job dialogue, 1280 x 720: 174 = 58 + 58 + 58 although a fourth of 96 would make 270;
/// hypothesis: the owner's Trudy picture agrees, 96 + 96 + 58 = 250 (dialogue-menu.md §3), and a
/// greatest height of 250 would agree with both too: a list of many short rows would tell].
const SHOWN_ROWS: usize = 3;
/// The frame's look on a check the player falls short of [hypothesis: fitted to one picture,
/// dialogue-menu.md §3; design: as the hand-built frame; the capture holds the lines of plain
/// rows only, (200, 143, 52) on its shots]: on a short check its line reads (100, 18, 16) after the file's line
/// brightness of 200 of 255; its fill.
const SHORT_LINE: [f64; 3] = [127.5, 22.95, 20.4];
const SHORT_FILL: [f64; 3] = [62.0, 0.0, 0.0];
const SHORT_FILL_ALPHA: f64 = 82.0;
const FILL_BRIGHTNESS: f64 = 128.0;
const FILL_ALPHA: f64 = 25.5;

fn set(ui: &mut Ui, tile: TileId, name: &str, v: Value) {
    ui.set_trait(tile, name, v);
}

fn set_at(ui: &mut Ui, path: &str, name: &str, v: Value) {
    if let Some(t) = ui.find_tile(path) {
        ui.set_trait(t, name, v);
    }
}

fn num(ui: &Ui, tile: TileId, name: &str) -> f64 {
    ui.tile(tile).get(name).map_or(0.0, Value::num)
}

fn child(ui: &Ui, tile: TileId, name: &str) -> Option<TileId> {
    ui.tile(tile).children.iter().copied().find(|&c| ui.is_live(c) && ui.tile(c).name.eq_ignore_ascii_case(name))
}

/// The rows the code made under the list, in order.
pub fn rows(ui: &Ui) -> Vec<TileId> {
    let Some(list) = ui.find_tile(LIST) else { return Vec::new() };
    ui.tile(list).children.iter().copied().filter(|&c| ui.is_live(c) && ui.tile(c).origin == Origin::Code && ui.tile(c).name.eq_ignore_ascii_case(ROW)).collect()
}

/// [design] Every fixed path the model writes or the input reads, and the pieces the view is
/// not whole without (the frame's four lines, the scroll bar's controls), is part of a
/// successful build; a file a mod restructured keeps the hand-built menu.
pub fn built(ui: &Ui) -> Tiled {
    for path in [DIALOG_MENU, ROOT, BAND, CLICK_AREA, NAME, LINE, LIST, HIGHLIGHT, BAR, TOP_BRACKET, BOTTOM_BRACKET] {
        if ui.find_tile(path).is_none() {
            return Tiled::No(format!("{path} missing"));
        }
    }
    for (under, names) in [(HIGHLIGHT, &FRAME_LINES[..]), (BAR, &BAR_CONTROLS[..])] {
        for name in names {
            if ui.find_tile(&format!("{under}/{name}")).is_none() {
                return Tiled::No(format!("{under}/{name} missing"));
            }
        }
    }
    Tiled::Yes
}

/// [design] What a preparation has to have worked out for the view to be usable and its
/// tiles hit: the file evaluated to boxes with a size, and what is to be clicked a target.
/// The file's values, read after the model's writes, so a mod's change to them counts.
fn check_geometry(ui: &Ui, view: &MenuView) -> Result<(), String> {
    for (path, name) in [(BAND, "width"), (LIST, "width"), (HIGHLIGHT, "width"), (CLICK_AREA, "width"), (CLICK_AREA, "height")] {
        if ui.get_float(&format!("{path}/{name}")).is_none_or(|v| v <= 0.0) {
            return Err(format!("{path} has no {name}"));
        }
    }
    let target = |t: TileId| ui.tile(t).get("target").is_some_and(Value::truthy);
    if view.choices.is_empty() {
        if !ui.find_tile(CLICK_AREA).is_some_and(target) {
            return Err(format!("{CLICK_AREA} takes no click while the line is up"));
        }
        return Ok(());
    }
    for row in rows(ui) {
        if num(ui, row, "width") <= 0.0 || !target(row) {
            return Err(format!("a {ROW} cannot be clicked"));
        }
        if child(ui, row, ROW_TEXT).is_none_or(|t| num(ui, t, "wrapwidth") <= 0.0) {
            return Err(format!("a {ROW} has no room for its text"));
        }
    }
    Ok(())
}

/// [design] What the file's template makes: a row with its text, under the list. Made once
/// and unloaded again, when the tile list has none yet, as a speaking phase has no rows and
/// the first choices should find no surprise.
fn template_makes_a_row(ui: &mut Ui) -> Tiled {
    if !rows(ui).is_empty() {
        return Tiled::Yes;
    }
    let Some(row) = ui.add_code_tile_from_template(&MemSource::default(), LIST, ROW_TEMPLATE, None) else {
        return Tiled::No(format!("no row from {ROW_TEMPLATE}"));
    };
    let has_text = child(ui, row, ROW_TEXT).is_some();
    let name_fits = ui.tile(row).name.eq_ignore_ascii_case(ROW);
    ui.unload_tile(row);
    if !name_fits {
        Tiled::No(format!("{ROW_TEMPLATE} makes no {ROW}"))
    } else if !has_text {
        Tiled::No(format!("{ROW_TEMPLATE} makes no {ROW_TEXT}"))
    } else {
        Tiled::Yes
    }
}

/// Unload the rows made, after a preparation that could not finish.
fn clean(ui: &mut Ui) {
    for row in rows(ui) {
        ui.unload_tile(row);
    }
}

/// [design] A preparation is finished only with every text it has to place measured.
fn check_measured(ui: &Ui, view: &MenuView) -> Result<(), String> {
    let line = view.line.as_deref().map_or("", str::trim);
    if view.choices.is_empty() && !line.is_empty() && ui.find_tile(LINE).is_none_or(|t| num(ui, t, "height") <= 0.0) {
        return Err(format!("{LINE} has no measure"));
    }
    let made = rows(ui);
    if made.len() != view.choices.len() {
        return Err(format!("{} rows for {} choices", made.len(), view.choices.len()));
    }
    for (&row, choice) in made.iter().zip(&view.choices) {
        let text = child(ui, row, ROW_TEXT).ok_or_else(|| format!("a {ROW} has no {ROW_TEXT}"))?;
        if !choice.label.trim().is_empty() && num(ui, text, "height") <= 0.0 {
            return Err(format!("{ROW_TEXT} has no measure"));
        }
    }
    Ok(())
}

/// [design] What the view is not whole without, once laid out: what the renderer would paint
/// and what the pointer could reach, each read from the renderer's and the input's own
/// selection, so a tile of a kind or with content they do not take counts as missing. The name,
/// and the band and the line while there is a line; the band, brackets, frame, a text and a
/// reachable region for each row shown, and the scroll bar's pieces when it is called for.
fn check_drawn(ui: &Ui, view: &MenuView, menu: TileId) -> Result<(), String> {
    use super::draw::{pieces, Select};
    let drawn = pieces(ui, menu, &|_: f64, _: &str, _: Option<f64>| (0.0, 0.0), Select::Whole { kept: &[], strip: None }, true).0;
    let paints = |path: &str| {
        let Some(wanted) = ui.find_tile(path) else { return false };
        drawn.iter().any(|p| {
            let mut at = Some(p.tile);
            while let Some(t) = at {
                if t == wanted {
                    return true;
                }
                at = ui.tile(t).parent;
            }
            false
        })
    };
    let need = |path: &str| if paints(path) { Ok(()) } else { Err(format!("{path} draws nothing")) };
    let hits = fnv_ui::input::hit_candidates(ui, menu, &fnv_ui::input::no_measure);
    let region = |t: TileId| hits.iter().find(|c| c.under == t && c.region().width > 0.0 && c.region().height > 0.0).map(|c| c.region());
    let reached = |t: TileId| region(t).is_some();
    // The middle of its region goes to it, or to what stands for the same: no other target
    // over it takes the click.
    let wins = |t: TileId| {
        region(t).is_some_and(|r| {
            let winner = fnv_ui::input::top_hit(ui, &hits, (r.left + r.width / 2.0, r.top + r.height / 2.0));
            winner.is_some_and(|h| dialog_tile(ui, h.tile).is_some() && dialog_tile(ui, h.tile) == dialog_tile(ui, t))
        })
    };
    if !view.speaker.trim().is_empty() {
        need(NAME)?;
    }
    if view.choices.is_empty() {
        if !view.line.as_deref().map_or("", str::trim).is_empty() {
            need(BAND)?;
            need(LINE)?;
        }
        if !ui.find_tile(CLICK_AREA).is_some_and(reached) {
            return Err(format!("{CLICK_AREA} has no region to click"));
        }
        if !ui.find_tile(CLICK_AREA).is_some_and(wins) {
            return Err(format!("{CLICK_AREA} is covered by another target"));
        }
        return Ok(());
    }
    need(BAND)?;
    need(TOP_BRACKET)?;
    need(BOTTOM_BRACKET)?;
    let framed = ui.find_tile(LIST).map_or((-1.0, 0.0), |l| (num(ui, l, "_highlight_y"), num(ui, l, "_selected_height")));
    let list_height = ui.get_float(&format!("{LIST}/height")).unwrap_or(0.0);
    if framed.1 > 0.0 && framed.0 + framed.1 > 0.0 && framed.0 < list_height {
        for line in FRAME_LINES {
            need(&format!("{HIGHLIGHT}/{line}"))?;
        }
    }
    for (row, choice) in rows(ui).into_iter().zip(&view.choices) {
        // Rows out of the list's window are not on screen to be drawn or reached.
        let at = num(ui, row, "y");
        if at + num(ui, row, "height") <= 0.0 || at >= list_height {
            continue;
        }
        if !reached(row) {
            return Err(format!("a {ROW} has no region to click"));
        }
        if !wins(row) {
            return Err(format!("a {ROW} is covered by another target"));
        }
        if !choice.label.trim().is_empty() && !child(ui, row, ROW_TEXT).is_some_and(|t| drawn.iter().any(|p| p.tile == t)) {
            return Err(format!("a {ROW_TEXT} draws nothing"));
        }
    }
    let bar = ui.find_tile(BAR).map_or((0.0, 0.0), |b| (num(ui, b, "_number_of_items"), num(ui, b, "_number_of_visible_items")));
    if bar.0 > bar.1 {
        for control in ["scrollbar_vert_up", "scrollbar_vert_down", "scrollbar_vert_marker"] {
            need(&format!("{BAR}/{control}"))?;
        }
        for control in ["scrollbar_vert_up", "scrollbar_vert_down"] {
            if !ui.find_tile(&format!("{BAR}/{control}")).is_some_and(reached) {
                return Err(format!("{BAR}/{control} has no region to click"));
            }
            if !ui.find_tile(&format!("{BAR}/{control}")).is_some_and(wins) {
                return Err(format!("{BAR}/{control} is covered by another target"));
            }
        }
    }
    Ok(())
}

/// Make NVForever's own tiles once the menu is built: their templates, then one of each. And
/// the scroll bar's pictures their own alpha: the bar is a picture tile with an alpha of 0,
/// which would pass its 0 down to them, though the owner's pictures show them (§3)
/// [hypothesis: how the original keeps them from it is not known; design: the mark
/// `super::layout::OWN_ALPHA`, as the tutorial menu's].
fn own_tiles(ui: &mut Ui) -> bool {
    let src = MemSource::default();
    let Some(menu) = ui.menu(DIALOG_MENU) else { return false };
    for control in ["scrollbar_vert_up", "scrollbar_vert_down", "scrollbar_vert_marker"] {
        set_at(ui, &format!("{BAR}/{control}"), super::layout::OWN_ALPHA, Value::Num(1.0));
    }
    if !ui.tile(menu).templates.iter().any(|t| t.name.eq_ignore_ascii_case(HINT_TEMPLATE)) && !ui.inject_text(&src, DIALOG_MENU, OWN_TEMPLATES) {
        return false;
    }
    (ui.find_tile(FILL).is_some() || ui.add_code_tile_from_template(&src, HIGHLIGHT, FILL_TEMPLATE, None).is_some())
        && (ui.find_tile(HINT).is_some() || ui.add_code_tile_from_template(&src, ROOT, HINT_TEMPLATE, None).is_some())
}

/// What the menu is to show this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    /// The menu's tile: a new one when it is built again.
    pub menu: TileId,
    pub list: ListId,
    pub view: MenuView,
    pub selected: usize,
    /// `DialogueScreen::framed` and `DialogueScreen::keyed`.
    pub framed: bool,
    pub keyed: u64,
    /// `screen()`'s width and height.
    pub screen: (f64, f64),
    /// The player's setting for the Tab hint ([`hint_shown`]): changing it places the hint at
    /// once, as it is part of what [`Placement`] compares.
    pub hint: bool,
}

/// What the model last wrote into the tree, to write only what changes of it.
struct Published {
    menu: TileId,
    list: ListId,
    view: MenuView,
    visible: bool,
}

/// `screen()`'s `cropx` and `cropy` while this menu holds them: what they were (absent too) and
/// what it wrote, so they go back on every way out unless a script has changed them since.
struct CropKept {
    /// Whose: a tree built again after a load has its own.
    screen: TileId,
    before: [Option<Value>; 2],
    wrote: [Option<f64>; 2],
}

const CROPS: [&str; 2] = ["cropx", "cropy"];

/// What the last placement was worked out from: the model and every evaluated number of the
/// file's it reads. Another one places the rows again [design].
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    shown: Shown,
    numbers: Vec<f64>,
}

impl Placement {
    fn read(ui: &Ui, shown: &Shown) -> Placement {
        let at = |path: &str, name: &str| ui.get_float(&format!("{path}/{name}")).unwrap_or(0.0);
        let mut numbers = vec![at(DIALOG_MENU, "_MinListHeight"), at(LIST, "_scroll_delta"), at(BAND, "x"), at(BOTTOM_BRACKET, "y"), at(HIGHLIGHT, "width"), at(HINT_TEXT, "width"), at(HINT_TEXT, "height")];
        for r in rows(ui) {
            numbers.push(num(ui, r, "_VerticalSpacing"));
            // What `place` reads of a row's text: an expression may change it with no measuring.
            if let Some(t) = child(ui, r, ROW_TEXT) {
                numbers.extend([num(ui, t, "width"), num(ui, t, "height")]);
            }
        }
        Placement { shown: shown.clone(), numbers }
    }
}

/// [`Written`], shared with the transition hook the runtime holds.
#[derive(Resource, Clone, Default)]
pub struct SharedWritten(std::sync::Arc<std::sync::Mutex<Written>>);

impl SharedWritten {
    /// Rows the bar asks to scroll by, placed after the frame's scripts.
    pub fn scroll(&self, steps: i32) {
        if steps != 0 {
            if let Ok(mut written) = self.0.lock() {
                written.scroll_asked = written.scroll_asked.saturating_add(steps);
            }
        }
    }
}

/// What the steps of a frame keep between them and between frames.
#[derive(Default)]
pub struct Written {
    /// The speaker's name as last worked out with the world at hand, for a transition with
    /// none ([`boundary`]).
    speaker: Option<(fnv_sim::FormId, String)>,
    published: Option<Published>,
    placed: Option<Placement>,
    /// [`Written::placed`] changed in the last [`write`]: worth a line in the log.
    moved: bool,
    sizes: SizesWritten,
    /// The scroll by rows, written again every frame.
    scroll: RowScroll,
    /// Rows the bar's arrows and page areas asked to scroll by since the last placing
    /// ([`follow`]).
    scroll_asked: i32,
    /// `DialogueScreen::keyed` at the last placing.
    keyed_seen: u64,
    crop: Option<CropKept>,
    /// The menu whose row template was tried.
    probed: Option<TileId>,
    /// The reasons warned already: once each, as long as the run lasts.
    warned: std::collections::HashSet<String>,
    /// Tests stand in for the real file's expressions ([`evaluate`]).
    #[cfg(test)]
    stand_in: Option<fn(&mut Ui)>,
    /// This conversation asked for the menu at once; the frames' own lifetime pass goes on
    /// asking (`ui_loading::sync`, which remembers a file it could not read).
    build_asked: bool,
}

impl Written {
    /// Warn of a reason the menu stays hand-built, once in the run ([`Written::warned`]).
    fn warn(&mut self, status: &Tiled) {
        if let Tiled::No(reason) = status {
            if self.warned.insert(reason.clone()) {
                warn!("{DIALOG_MENU} drawn by hand: {reason}");
            }
        }
    }

    /// The menu stays hand-built for now: nothing of the preparation is left behind, and the
    /// next frame prepares it afresh.
    fn fall_back(&mut self, ui: &mut Ui) {
        clean(ui);
        release_crop(ui, self);
        self.published = None;
        self.placed = None;
        self.sizes.clear();
        self.scroll = RowScroll::default();
    }

    /// No conversation any more (no tree at all when `ui` is `None`).
    fn forget(&mut self, ui: Option<&mut Ui>) {
        match ui {
            Some(ui) => self.fall_back(ui),
            None => {
                self.published = None;
                self.placed = None;
                self.crop = None;
            }
        }
        self.build_asked = false;
    }
}

/// Hold `screen()`'s `cropx` and `cropy` at the safe zone for this menu, which reads them
/// (`dialogue-menu.md` §6.2). The first time keeps what they were; a script's own value since
/// stands.
fn hold_crop(ui: &mut Ui, written: &mut Written) {
    let screen = ui.screen();
    let want = [CROP, CROP];
    if written.crop.as_ref().is_some_and(|k| k.screen != screen) {
        written.crop = None;
    }
    let kept = written.crop.get_or_insert_with(|| CropKept { screen, before: CROPS.map(|n| ui.tile(screen).get(n).cloned()), wrote: [None, None] });
    for (i, name) in CROPS.into_iter().enumerate() {
        if kept.wrote[i].is_none_or(|w| num(ui, screen, name) == w) {
            ui.set_trait(screen, name, Value::Num(want[i]));
            kept.wrote[i] = Some(want[i]);
        }
    }
}

/// Give `screen()`'s `cropx` and `cropy` back as they were, absent if they were, unless a script
/// has changed them since this menu wrote them.
fn release_crop(ui: &mut Ui, written: &mut Written) {
    let Some(kept) = written.crop.take() else { return };
    let screen = ui.screen();
    if kept.screen != screen {
        return;
    }
    for (i, name) in CROPS.into_iter().enumerate() {
        if kept.wrote[i].is_some_and(|w| num(ui, screen, name) == w) {
            match &kept.before[i] {
                Some(v) => ui.set_trait(screen, name, v.clone()),
                None => {
                    ui.remove_trait(screen, name);
                }
            }
        }
    }
}

/// Write what the file leaves to code of the model's view (`dialogue-menu.md` §6.2): the
/// speaker, the line, the phase, one row per choice. Each only when the model's value for it
/// changed since it was last written, so what a script set stands until then ([`Published`]).
/// Another list than the last's has its rows made again, so a click still queued on the last
/// list's rows finds its tile gone.
pub fn publish(ui: &mut Ui, view: &MenuView, list: &ListId, written: &mut Written) -> Result<(), String> {
    let menu = ui.menu(DIALOG_MENU).ok_or_else(|| format!("{DIALOG_MENU} missing"))?;
    let was = written.published.take().filter(|p| p.menu == menu);
    let fresh = was.as_ref().is_none_or(|p| p.list != *list);
    let before = was.as_ref().map(|p| &p.view);
    let flag = |on: bool| Value::Num(f64::from(u8::from(on)));
    // No subtitles setting yet: on, but for a line with no text, which leaves the name alone.
    let phase = |v: &MenuView| (v.choices.is_empty(), !v.choices.is_empty() || !v.line.as_deref().map_or("", str::trim).is_empty());
    let (speaking, subtitles) = phase(view);
    let was_phase = before.map(phase);
    if !was.as_ref().is_some_and(|p| p.visible) {
        set(ui, menu, "_DialogVisible", flag(true));
    }
    if was_phase.map(|p| p.0) != Some(speaking) {
        set(ui, menu, "_ShowingText", flag(speaking));
    }
    if was_phase.map(|p| p.1) != Some(subtitles) {
        set(ui, menu, "_ShowSubtitles", flag(subtitles));
    }
    if before.is_none_or(|v| v.speaker != view.speaker) {
        set_at(ui, NAME, "string", Value::Str(view.speaker.trim().to_string()));
    }
    if before.is_none_or(|v| v.line != view.line) {
        set_at(ui, LINE, "string", Value::Str(view.line.as_deref().map_or("", str::trim).to_string()));
    }
    if before.is_none() {
        set_at(ui, BAND, "alpha", Value::Num(BAND_ALPHA));
        set_at(ui, LIST, "_scroll_delta", Value::Num(SCROLL_DELTA));
    }
    // The hint's words follow the device (a controller's B, #1662).
    let words = goodbye_hint();
    if ui.get_string(&format!("{HINT_TEXT}/string")).as_deref() != Some(words.as_str()) {
        set_at(ui, HINT_TEXT, "string", Value::Str(words));
    }
    hold_crop(ui, written);
    let mut made = rows(ui);
    if fresh {
        made.drain(..).for_each(|t| ui.unload_tile(t));
    }
    while made.len() > view.choices.len() {
        if let Some(t) = made.pop() {
            ui.unload_tile(t);
        }
    }
    while made.len() < view.choices.len() {
        // The template's includes were spliced in when the file was read (menu-xml.md §2.2).
        match ui.add_code_tile_from_template(&MemSource::default(), LIST, ROW_TEMPLATE, None) {
            Some(t) => made.push(t),
            None => return Err(format!("no row from {ROW_TEMPLATE}")),
        }
    }
    for (i, (&row, choice)) in made.iter().zip(&view.choices).enumerate() {
        let old = before.filter(|_| !fresh).and_then(|v| v.choices.get(i));
        if old.is_none() {
            // After this the window's ([`place`]).
            set(ui, row, "listindex", Value::Num(i as f64));
        }
        if old.is_none_or(|c| c.label != choice.label) {
            set(ui, row, "string", Value::Str(choice.label.trim().to_string()));
        }
        if old.is_none_or(|c| c.darkened != choice.darkened) {
            set(ui, row, "_line_alpha", Value::Num(if choice.darkened { DIM_ALPHA } else { 255.0 }));
        }
    }
    written.published = Some(Published { menu, list: list.clone(), view: view.clone(), visible: true });
    ui.update();
    Ok(())
}

/// The menu is held hidden: a menu over the conversation, or the character creator.
fn hide(ui: &mut Ui, written: &mut Written) {
    set_at(ui, DIALOG_MENU, "_DialogVisible", Value::Num(0.0));
    if let Some(p) = written.published.as_mut() {
        p.visible = false;
    }
}

/// The frame's colours: red on a check the player falls short of, the file's colour otherwise.
fn write_frame_colour(ui: &mut Ui, short: bool) {
    for line in FRAME_LINES {
        let Some(t) = ui.find_tile(&format!("{HIGHLIGHT}/{line}")) else { continue };
        set(ui, t, "systemcolor", Value::Num(if short { 0.0 } else { 1.0 }));
        if short {
            for (name, v) in ["red", "green", "blue"].into_iter().zip(SHORT_LINE) {
                set(ui, t, name, Value::Num(v));
            }
        }
    }
    let Some(fill) = ui.find_tile(FILL) else { return };
    set(ui, fill, "systemcolor", Value::Num(if short { 0.0 } else { 1.0 }));
    for (name, v) in ["red", "green", "blue"].into_iter().zip(SHORT_FILL) {
        set(ui, fill, name, Value::Num(v));
    }
    set(ui, fill, "brightness", Value::Num(if short { 255.0 } else { FILL_BRIGHTNESS }));
    set(ui, fill, "alpha", Value::Num(if short { SHORT_FILL_ALPHA } else { FILL_ALPHA }));
}

/// The scroll by rows [design: owner choice 2026-11-05, whole rows, as the hand-built menu;
/// `dialogue-menu.md` §6.3]: what [`place`] worked out and [`write_scroll`] writes on every
/// frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowScroll {
    /// The first row shown whole at the window's top.
    pub first: usize,
    /// The bar's `_current_value`: that row's place, in `_scroll_delta`s.
    pub value: f64,
    /// The bar's `_jump_size`: the whole rows that fit in the window from `first`, at least 1.
    pub jump: f64,
}

/// The last first row: the least row from which the rows left fit in a window `height` high,
/// so the list never shows room at its foot it could fill with rows; the last row when even it
/// does not fit [design].
pub fn last_first(tops: &[f64], heights: &[f64], height: f64) -> usize {
    let total = tops.last().zip(heights.last()).map_or(0.0, |(t, h)| t + h);
    tops.iter().position(|&t| total - t <= height + EPS).unwrap_or(tops.len().saturating_sub(1))
}

/// The whole rows that fit in a window `height` high from row `first`, at least 1 [design].
pub fn rows_fitting(first: usize, tops: &[f64], heights: &[f64], height: f64) -> usize {
    let Some(&top) = tops.get(first) else { return 1 };
    (first..tops.len()).take_while(|&i| tops[i] + heights[i] - top <= height + EPS).count().max(1)
}

/// The first row that shows row `s` whole in a window `height` high, moving as few rows as it
/// can from `first`, within `most` [design: owner choice 2026-11-05, whole rows]: a row above
/// the window comes to its top; a row whose foot is below it raises the first row until it
/// fits, or comes to the top itself when it is taller than the window.
pub fn scroll_to_show(first: usize, s: usize, tops: &[f64], heights: &[f64], height: f64, most: usize) -> usize {
    let mut at = first;
    if s < at {
        at = s;
    } else {
        while at < s && tops[s] + heights[s] - tops[at] > height + EPS {
            at += 1;
        }
    }
    at.min(most)
}

/// Set the rows out, one under the other from their measured texts; the list's height from
/// the first [`SHOWN_ROWS`] and the file's least; the scroll, `steps` rows from where it stands
/// and to the selected row when `follow` asks; the frame on the selected row, or none
/// (`selected` `None`). The bar counts in `_scroll_delta`s: its items the last first row's
/// place and 1, visible 1, so the prefab's range (items less visible) is the farthest the
/// value goes and the marker reaches the track's end with the last row [design; the bar's
/// counting as the original's, observed: capture of the original, private job dialogue,
/// 1280 x 720: items 11, visible 1 for 10 steps]. Returns the scroll.
fn place(ui: &mut Ui, view: &MenuView, selected: Option<usize>, first: usize, steps: i32, follow: bool) -> RowScroll {
    let (Some(menu), Some(list), Some(bar)) = (ui.menu(DIALOG_MENU), ui.find_tile(LIST), ui.find_tile(BAR)) else { return RowScroll::default() };
    let made = rows(ui);
    if made.is_empty() {
        set(ui, list, "_highlight_y", Value::Num(-1.0));
        set(ui, list, "_selected_height", Value::Num(0.0));
        set(ui, bar, "_number_of_items", Value::Num(1.0));
        set(ui, bar, "_number_of_visible_items", Value::Num(1.0));
        return RowScroll { jump: 1.0, ..RowScroll::default() };
    }
    let delta = num(ui, list, "_scroll_delta");
    let delta = if delta > 0.0 { delta } else { SCROLL_DELTA };
    let heights: Vec<f64> = made.iter().map(|&r| child(ui, r, ROW_TEXT).map_or(0.0, |t| num(ui, t, "height")) + num(ui, r, "_VerticalSpacing")).collect();
    let mut y = 0.0;
    let mut tops = Vec::with_capacity(made.len());
    for (i, (&row, &height)) in made.iter().zip(&heights).enumerate() {
        tops.push(y);
        set(ui, row, "height", Value::Num(height));
        set(ui, row, "_y", Value::Num(y));
        set(ui, row, "listindex", Value::Num(i as f64));
        y += height;
    }
    let height = heights.iter().take(SHOWN_ROWS).sum::<f64>().max(num(ui, menu, "_MinListHeight"));
    let most = last_first(&tops, &heights, height);
    let selected = selected.map(|s| s.min(made.len() - 1));
    let mut at = (first as i64 + i64::from(steps)).clamp(0, most as i64) as usize;
    if let (true, Some(s)) = (follow, selected) {
        at = scroll_to_show(at, s, &tops, &heights, height, most);
    }
    set(ui, list, "height", Value::Num(height));
    set(ui, list, "_highlight_y", Value::Num(selected.map_or(-1.0, |s| tops[s] - tops[at])));
    set(ui, list, "_selected_height", Value::Num(selected.map_or(0.0, |s| heights[s])));
    set(ui, bar, "_number_of_items", Value::Num(tops[most] / delta + 1.0));
    set(ui, bar, "_number_of_visible_items", Value::Num(1.0));
    write_frame_colour(ui, selected.and_then(|s| view.choices.get(s)).is_some_and(|c| c.short));
    if let (Some(fill), Some(highlight), Some(s)) = (ui.find_tile(FILL), ui.find_tile(HIGHLIGHT), selected) {
        let wide = num(ui, highlight, "width");
        set(ui, fill, "width", Value::Num(wide));
        set(ui, fill, "height", Value::Num(heights[s] - 1.0));
    }
    RowScroll { first: at, value: tops[at] / delta, jump: rows_fitting(at, &tops, &heights, height) as f64 }
}

/// Place the Tab hint from the band and the bottom bracket as the file worked them out, left
/// of the band where the screen has room, else at the safe zone's edge. Shown while a Goodbye
/// choice is offered [design: dialogue-menu.md §6.4].
fn place_hint(ui: &mut Ui, view: &MenuView, setting: bool) {
    let Some(hint) = ui.find_tile(HINT) else { return };
    let show = hint_shown(view, setting);
    set(ui, hint, "visible", Value::Num(f64::from(u8::from(show))));
    let (Some(text), Some(pad), Some(band), Some(bottom), true) = (ui.find_tile(HINT_TEXT), ui.find_tile(HINT_BOX), ui.get_float(&format!("{BAND}/x")), ui.get_float(&format!("{BOTTOM_BRACKET}/y")), show) else { return };
    let (width, height) = (num(ui, text, "width"), num(ui, text, "height"));
    set(ui, hint, "x", Value::Num((band - HINT_GAP - width).max(CROP + HINT_PAD)));
    set(ui, hint, "y", Value::Num(bottom - height / 2.0));
    set(ui, pad, "x", Value::Num(-HINT_PAD));
    set(ui, pad, "y", Value::Num(-HINT_PAD / 2.0));
    set(ui, pad, "width", Value::Num(width + 2.0 * HINT_PAD));
    set(ui, pad, "height", Value::Num(height + HINT_PAD));
    set(ui, pad, "alpha", Value::Num(BAND_ALPHA));
}

/// The scroll, on every frame the menu is tiled: the prefab counts clicks from 0 in this
/// tree, so its value and the rows it moves would drift from the model's otherwise, and pokes
/// `_SetInCode`, as the prefab asks of code that sets the value [observed: the vanilla files,
/// by fnvtool: its comment]. The page, `_jump_size`, is the whole rows that fit from the first
/// row, so it changes as the list scrolls [design].
fn write_scroll(ui: &mut Ui, scroll: &RowScroll) {
    let Some(bar) = ui.find_tile(BAR) else { return };
    set(ui, bar, "_current_value", Value::Num(scroll.value));
    set(ui, bar, "_jump_size", Value::Num(scroll.jump.max(1.0)));
    set(ui, bar, "_SetInCode", Value::Num(1.0));
}

/// Work the tree out, what a test's stand-in does for the real file's expressions too.
fn evaluate(ui: &mut Ui, written: &Written) {
    ui.update();
    #[cfg(test)]
    if let Some(stand_in) = written.stand_in {
        stand_in(ui);
        ui.update();
    }
    #[cfg(not(test))]
    let _ = written;
}

/// The second step of a frame, after the scripts: measure, place and scroll. `round` is one
/// round of measuring: the fonts as the tree stands then, the sizes of the texts it is given
/// written, how many were. The places are worked out again when the model, a text's size or
/// any evaluated number they read moved ([`Placement`]).
fn settle(ui: &mut Ui, now: &Shown, written: &mut Written, round: &mut dyn FnMut(&mut Ui, &[TileId], &mut SizesWritten) -> usize) -> Result<(), String> {
    evaluate(ui, written);
    let texts = menus::texts(ui, now.menu);
    let measured = round(ui, &texts, &mut written.sizes);
    if measured > 0 {
        evaluate(ui, written);
    }
    check_measured(ui, &now.view)?;
    check_geometry(ui, &now.view)?;
    let steps = std::mem::take(&mut written.scroll_asked);
    let follow = written.keyed_seen != now.keyed;
    written.keyed_seen = now.keyed;
    written.moved = measured > 0 || steps != 0 || follow || written.placed.as_ref() != Some(&Placement::read(ui, now));
    if written.moved {
        // A new list starts at its top.
        if written.placed.as_ref().is_none_or(|p| p.shown.list != now.list || p.shown.menu != now.menu) {
            written.scroll = RowScroll::default();
        }
        written.scroll = place(ui, &now.view, now.framed.then_some(now.selected), written.scroll.first, steps, follow);
        evaluate(ui, written);
        place_hint(ui, &now.view, now.hint);
        written.placed = Some(Placement::read(ui, now));
    }
    write_scroll(ui, &written.scroll);
    fnv_ui::input::mark_clip_windows(ui, now.menu);
    if written.moved {
        evaluate(ui, written);
        check_drawn(ui, &now.view, now.menu)?;
    }
    Ok(())
}

/// A frame of the menu shown, with what the first step does (a conversation that started in
/// the scripts has had none), then [`settle`].
pub fn write(ui: &mut Ui, now: &Shown, written: &mut Written, round: &mut dyn FnMut(&mut Ui, &[TileId], &mut SizesWritten) -> usize) -> Result<(), String> {
    if let Tiled::No(reason) = prepare(ui, written) {
        return Err(reason);
    }
    publish(ui, &now.view, &now.list, written)?;
    settle(ui, now, written, round)
}

/// The scroll as the tree holds it, by rows: the first row shown (the last whose place is at or
/// above the bar's value times `_scroll_delta`) and the last first row ([`last_first`]) from the
/// rows' places and heights and the list's height.
fn scroll_rows(ui: &Ui, made: &[TileId]) -> (usize, usize) {
    let tops: Vec<f64> = made.iter().map(|&r| num(ui, r, "_y")).collect();
    let heights: Vec<f64> = made.iter().map(|&r| num(ui, r, "height")).collect();
    let (list, bar) = (ui.find_tile(LIST), ui.find_tile(BAR));
    let offset = bar.map_or(0.0, |b| num(ui, b, "_current_value")) * list.map_or(SCROLL_DELTA, |l| num(ui, l, "_scroll_delta"));
    let first = tops.iter().rposition(|&t| t <= offset + EPS).unwrap_or(0);
    (first, last_first(&tops, &heights, list.map_or(0.0, |l| num(ui, l, "height"))))
}

/// A line for the log when what the menu shows changes (smoke reads these): the view as the
/// tree worked it out, the scroll as `scroll <first row> of <last first row>`. Call after
/// [`write`].
pub fn describe(ui: &Ui, now: &Shown) -> String {
    let n = |x: f64| Value::Num(x).text();
    let f = |path: &str, name: &str| ui.get_float(&format!("{path}/{name}")).unwrap_or(-1.0);
    if now.view.choices.is_empty() {
        let line = now.view.line.as_deref().unwrap_or_default();
        return format!("tiles: {DIALOG_MENU} drawn from tiles: speaker {:?}, line {line:?}, band {}", now.view.speaker, f(BAND, "visible") != 0.0);
    }
    let made = rows(ui);
    let heights: Vec<String> = made.iter().map(|&r| n(num(ui, r, "height"))).collect();
    let (first, most) = scroll_rows(ui, &made);
    let dim = made.iter().filter(|&&r| num(ui, r, "_line_alpha") == DIM_ALPHA).count();
    let short = now.framed && now.view.choices.get(now.selected).is_some_and(|c| c.short);
    let selected = if now.framed { now.selected.to_string() } else { "none".into() };
    format!(
        "tiles: {DIALOG_MENU} drawn from tiles: speaker {:?}, {} choices, selected {selected}, row heights [{}], list y {} height {}, scroll {} of {}, bar {}, dim {dim}, frame {}, hint {}",
        now.view.speaker,
        made.len(),
        heights.join(", "),
        n(f(LIST, "y")),
        n(f(LIST, "height")),
        first,
        most,
        f(BAR, "visible") != 0.0,
        if short { "red" } else { "plain" },
        f(HINT, "visible") != 0.0,
    )
}

/// The keys `crate::dialogue::input` acts on itself, by the tile input's names: W, S and the
/// arrows (a step), E and the controller's A (Activate). Any other key a file binds is the
/// file's (`super::menus::key_click_acts`).
pub const HAND_BUILT_KEYS: [&str; 6] = ["W", "S", "E", "xup", "xdown", "xbuttona"];

/// What the menu's tiles stand for, by name [observed: the vanilla files, by fnvtool ui tree].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogTile {
    /// A row shown, the choice's number.
    Choice(usize),
    /// The click area or the line.
    Skip,
    /// A scroll bar's arrow or page area: rows to scroll by, an arrow one, a page the bar's
    /// `_jump_size` (the whole rows that fit), the list's selection staying [design: owner
    /// choice 2026-11-05, whole rows; the original's arrow moved its value by 1 and a page by 3
    /// of its 56-unit steps, and `_highlight_y` went with it, observed: capture of the
    /// original, private job dialogue, 1280 x 720]. The marker's drag is not built.
    Scroll(i32),
}

/// What `tile` stands for, by its name or a parent's. A row hidden (out of the window) is
/// nothing, and neither is the scroll bar's marker: dragging it is not built [design].
pub fn dialog_tile(ui: &Ui, tile: TileId) -> Option<DialogTile> {
    let page = || ui.find_tile(BAR).map_or(1.0, |b| num(ui, b, "_jump_size")).max(1.0) as i32;
    let mut at = Some(tile);
    while let Some(t) = at {
        let tile = ui.tile(t);
        match tile.name.to_ascii_lowercase().as_str() {
            "dm_topic" if tile.origin == Origin::Code => {
                let i = num(ui, t, "listindex");
                return (i >= 0.0).then_some(DialogTile::Choice(i as usize));
            }
            "dm_clickrect" | "dm_speakertext" => return Some(DialogTile::Skip),
            "scrollbar_vert_up" => return Some(DialogTile::Scroll(-1)),
            "scrollbar_vert_down" => return Some(DialogTile::Scroll(1)),
            "scrollbar_vert_page_up" => return Some(DialogTile::Scroll(-page())),
            "scrollbar_vert_page_down" => return Some(DialogTile::Scroll(page())),
            "scrollbar_vert_marker" => return None,
            _ => {}
        }
        at = tile.parent;
    }
    None
}

/// Turn a frame's report on the menu's tiles into what the view asks of the conversation:
/// see the module notes. `physical`: the keys from the keyboard (or a controller standing in
/// for them) this frame; `list`: what the rows were written for; `scroll_asked`: the view's own,
/// the rows the bar asks for ([`Written::scroll_asked`]).
pub fn follow(ui: &Ui, report: &Report, physical: &[&str], menu: TileId, list: Option<&ListId>, asked: &mut ViewInput, scroll_asked: &mut i32) {
    asked.list = list.cloned();
    let mine = |t: TileId| ui.is_live(t) && ui.root_of(t) == menu;
    // A highlight a file's ref moved for a hand-built Up or Down is not the pointer's: the
    // selection moves once, by the key.
    let by_key = physical.iter().any(|k| k.eq_ignore_ascii_case("xup") || k.eq_ignore_ascii_case("xdown"));
    if let (false, Some((_, Some(to)))) = (by_key, report.over) {
        if let Some(DialogTile::Choice(i)) = Some(to).filter(|&t| mine(t)).and_then(|t| dialog_tile(ui, t)) {
            asked.hover = Some(i);
        }
    }
    for c in report.clicks.iter().filter(|c| mine(c.tile)) {
        // Stamped when it was made: a click queued on a screen that has gone acts on nothing.
        if list.is_none_or(|l| l.stamp() != c.stamp) {
            info!("tiles: {DIALOG_MENU} click from an earlier screen ignored: {}", ui.path_of(c.tile));
            continue;
        }
        if let By::Key(k) = &c.by {
            if !menus::key_click_acts(DIALOG_MENU, k, physical) {
                continue;
            }
        }
        match dialog_tile(ui, c.tile) {
            // The first in a frame, as one act a frame is what `crate::dialogue` takes.
            Some(DialogTile::Choice(i)) if asked.pick.is_none() => {
                info!("tiles: {DIALOG_MENU} choice {i} picked on its tiles");
                asked.pick = Some(i);
            }
            Some(DialogTile::Skip) if !asked.skip => {
                info!("tiles: {DIALOG_MENU} line skipped on its tiles");
                asked.skip = true;
            }
            Some(DialogTile::Scroll(n)) => {
                info!("tiles: {DIALOG_MENU} list scrolled {n} on its tiles");
                *scroll_asked = scroll_asked.saturating_add(n);
            }
            _ => {}
        }
    }
}

/// A line for the log when what the tile renderer was given changes (smoke reads these): what
/// it paints, from its own pieces, not from the tree's or the model's values; a row's text only
/// when it stands in the list's window.
pub fn painted(ui: &Ui, pieces: &[super::draw::Piece]) -> String {
    let in_view = |p: &super::draw::Piece| p.clip.is_none_or(|c| p.rect.top < c.top + c.height && p.rect.top + p.rect.height > c.top);
    let named = |n: &str| pieces.iter().filter(|p| ui.tile(p.tile).name.eq_ignore_ascii_case(n)).count();
    let any = |names: &[&str]| names.iter().map(|n| named(n)).sum::<usize>();
    let row_text = |p: &&super::draw::Piece| ui.tile(p.tile).name.eq_ignore_ascii_case(ROW_TEXT) && in_view(p);
    let dim = pieces.iter().filter(row_text).filter(|p| p.colour[3] < 0.6).count();
    let red = pieces.iter().any(|p| FRAME_LINES.iter().any(|n| ui.tile(p.tile).name.eq_ignore_ascii_case(n)) && ui.tile(p.tile).get("systemcolor").is_some_and(|c| c.num() == 0.0));
    format!(
        "tiles: {DIALOG_MENU} painted: band {}, name {}, line {}, rows {}, dim {dim}, frame {} {}, fill {}, bar {}, brackets {}, hint {}",
        named("DM_TextBackground"),
        named("DM_SpeakerNameLabel"),
        named("DM_SpeakerText"),
        pieces.iter().filter(row_text).count(),
        any(&FRAME_LINES),
        if red { "red" } else { "plain" },
        named("NVF_SelectionFill"),
        any(&BAR_CONTROLS),
        any(&["MainLine", "LeftVert", "RightVert"]),
        named("NVF_GoodbyeHintText"),
    )
}

/// What decides how the menu is drawn this frame, worked out once by [`write_system`] after
/// the scripts, for the tile drawing, the hand-built menu and the next frame's tile input
/// alike: they cannot disagree, and what takes a click is what was on screen.
#[derive(Resource, Debug, Default, PartialEq)]
pub struct DialogTiles {
    /// The menu, while it is drawn from its tiles.
    menu: Option<TileId>,
    /// Nothing over it: a menu over the conversation, or the character creator, hides it as
    /// they hide the hand-built one.
    shown: bool,
    /// The list its rows were written for.
    list: Option<ListId>,
    /// What can be hit as it was drawn, and `screen()`'s size then ([`snapshot_system`]).
    hits: Vec<HitCandidate>,
    hit_screen: (f64, f64),
}

impl DialogTiles {
    /// The hand-built menu gives way.
    pub fn by_tiles(&self) -> bool {
        self.menu.is_some()
    }

    /// The tile renderer draws the menu.
    pub fn drawn(&self) -> bool {
        self.menu.is_some() && self.shown
    }

    /// The menu, when it takes the tile input this frame: drawn last frame, the conversation
    /// still running with nothing over it, and the pause menu not up. A menu over the
    /// conversation keeps the input until it is gone, its closing frame included: `Game::menu`
    /// goes in that frame's tick, after this. On a frame whose window has a new size it takes
    /// nothing: the pointer's units are the new size's and [`DialogTiles::hits`] the old one's,
    /// and one press there is better lost [design]. Anything else that moves in the tree
    /// (an animation, a script) changes nothing: the click is on the candidates as drawn.
    pub fn taking(&self, game: &fnv_game::Game<'_>, paused: bool) -> Option<TileId> {
        let screen = game.ui_tree.screen();
        let same_size = (num(&game.ui_tree, screen, "width"), num(&game.ui_tree, screen, "height")) == self.hit_screen;
        self.menu.filter(|_| self.shown && !paused && game.dialogue.is_some() && !crate::dialogue::held(game) && same_size)
    }

    /// What can be hit, as it was drawn.
    pub fn hits(&self) -> &[HitCandidate] {
        &self.hits
    }

    fn take_hits(&mut self, ui: &Ui) {
        let Some(menu) = self.menu else { return };
        self.hits = fnv_ui::input::hit_candidates(ui, menu, &fnv_ui::input::no_measure);
        let screen = ui.screen();
        self.hit_screen = (num(ui, screen, "width"), num(ui, screen, "height"));
    }

    /// The list the rows were written for.
    pub fn list(&self) -> Option<&ListId> {
        self.list.as_ref()
    }
}

/// The tree as it is to be drawn: still whole ([`check_drawn`], whatever changed it since it was
/// placed: a script, a gradient), and then what can be hit, for the next frame's input. Otherwise
/// the whole hand-built menu, from this very frame's drawing.
fn finish_frame(ui: &mut Ui, state: &mut DialogTiles, written: &mut Written) {
    if !state.drawn() {
        return;
    }
    let checked = match (state.menu, written.published.as_ref()) {
        (Some(menu), Some(p)) => check_drawn(ui, &p.view, menu),
        _ => Ok(()),
    };
    match checked {
        Ok(()) => state.take_hits(ui),
        Err(reason) => {
            written.warn(&Tiled::No(reason));
            written.fall_back(ui);
            *state = DialogTiles::default();
        }
    }
}

/// After `super::update`, the frame's last working out of the tree, animations stepped, and
/// before the drawing: [`finish_frame`].
pub(crate) fn snapshot_system(mut script: ResMut<crate::script::ScriptState>, switch: Res<super::vanilla::TileMenus>, mut state: ResMut<DialogTiles>, shared: Res<SharedWritten>) {
    if !switch.dialog || !state.drawn() {
        return;
    }
    if let (Some(rt), Ok(mut written)) = (script.rt.as_mut(), shared.0.lock()) {
        finish_frame(&mut rt.game.ui_tree, &mut state, &mut written);
    }
}

/// The menu in the tree and ready to be written: its paths, NVForever's own tiles, its row
/// template.
fn prepare(ui: &mut Ui, written: &mut Written) -> Tiled {
    let status = built(ui);
    if !status.yes() {
        return status;
    }
    if !own_tiles(ui) {
        return Tiled::No("NVForever's own tiles were not made".into());
    }
    let Some(menu) = ui.menu(DIALOG_MENU) else { return Tiled::No(format!("{DIALOG_MENU} missing")) };
    if written.probed != Some(menu) {
        let status = template_makes_a_row(ui);
        if !status.yes() {
            return status;
        }
        written.probed = Some(menu);
    }
    Tiled::Yes
}

/// A conversation that started this frame: its menu now, so no frame is drawn by hand.
fn ask_for_menu(game: &mut fnv_game::Game<'_>, files: &super::MenuFiles, written: &mut Written) {
    if game.ui_tree.menu(DIALOG_MENU).is_none() && !written.build_asked {
        written.build_asked = true;
        files.write(game);
        game.ui_tree.update();
    }
}

/// What the hook does at a transition of the conversation (see the module notes), with no
/// world at hand: the name comes from the last frame's for the same speaker, else from the
/// runtime's, which the frame's own steps put right before anything is drawn. The menu is built
/// here for a conversation just started. Nothing is decided here.
pub(crate) fn boundary(game: &mut fnv_game::Game<'_>, written: &mut Written) {
    let Some(who) = game.dialogue.as_ref().map(crate::dialogue::speaker_form) else { return };
    if game.ui_files.is_none() {
        return;
    }
    if game.ui_tree.menu(DIALOG_MENU).is_none() && !written.build_asked {
        written.build_asked = true;
        // Scripts run: the pause and main menus are not up.
        let report = fnv_game::ui_loading::sync(game, false, false);
        report.changes.iter().for_each(|line| info!("ui: {line}"));
        report.warnings.iter().for_each(|line| warn!("ui: {line}"));
        game.ui_tree.update();
    }
    if !prepare(&mut game.ui_tree, written).yes() {
        return;
    }
    let speaker = written.speaker.as_ref().filter(|(form, _)| *form == who).map_or_else(|| game.data.name(who), |(_, name)| name.clone());
    match (crate::dialogue::view_named(game, speaker), crate::dialogue::list_id(game)) {
        (Some(view), Some(list)) => {
            if publish(&mut game.ui_tree, &view, &list, written).is_err() {
                written.published = None;
            }
        }
        _ => hide(&mut game.ui_tree, written),
    }
}

/// With the switch on, the runtime tells [`boundary`] of its transitions. Before anything of the
/// frame can start a conversation.
pub(crate) fn install_hook(mut script: ResMut<crate::script::ScriptState>, switch: Res<super::vanilla::TileMenus>, shared: Res<SharedWritten>) {
    if !switch.dialog {
        return;
    }
    let Some(rt) = script.rt.as_mut().filter(|rt| rt.game.ui_dialogue_hook.is_none()) else { return };
    let shared = shared.0.clone();
    rt.game.ui_dialogue_hook = Some(Box::new(move |g| {
        if let Ok(mut written) = shared.lock() {
            boundary(g, &mut written);
        }
    }));
}

/// Remember the speaker's name, worked out with the world at hand, for [`boundary`].
fn keep_speaker(game: &fnv_game::Game<'_>, view: Option<&MenuView>, written: &mut Written) {
    if let (Some(view), Some(c)) = (view, game.dialogue.as_ref()) {
        let who = crate::dialogue::speaker_form(c);
        if written.speaker.as_ref().is_none_or(|(form, name)| *form != who || *name != view.speaker) {
            written.speaker = Some((who, view.speaker.clone()));
        }
    }
}

/// A frame's own first step, before its scripts: see the module notes. Nothing is decided here;
/// [`write_system`] says what came of it.
pub(crate) fn publish_system(mut script: ResMut<crate::script::ScriptState>, switch: Res<super::vanilla::TileMenus>, files: super::MenuFiles, sim: Res<crate::sim::SimState>, creator: Option<Res<crate::creator::CreatorMenu>>, shared: Res<SharedWritten>) {
    if !switch.dialog {
        return;
    }
    let Some(game) = script.rt.as_mut().map(|rt| &mut rt.game).filter(|g| g.dialogue.is_some() && g.ui_files.is_some()) else { return };
    let Ok(mut written) = shared.0.lock() else { return };
    ask_for_menu(game, &files, &mut written);
    if !prepare(&mut game.ui_tree, &mut written).yes() {
        return;
    }
    let creating = creator.is_some_and(|c| c.is_open());
    let view = crate::dialogue::view(game, &sim).filter(|_| !creating);
    keep_speaker(game, view.as_ref(), &mut written);
    match (view, crate::dialogue::list_id(game)) {
        (Some(view), Some(list)) => {
            if publish(&mut game.ui_tree, &view, &list, &mut written).is_err() {
                written.published = None;
            }
        }
        _ => hide(&mut game.ui_tree, &mut written),
    }
}

/// The second step, after the scripts: the switch, a conversation, its menu ready and what the
/// model shows measured and placed ([`write`]); [`DialogTiles`] says the result.
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_system(
    mut script: ResMut<crate::script::ScriptState>,
    switch: Res<super::vanilla::TileMenus>,
    files: super::MenuFiles,
    sim: Res<crate::sim::SimState>,
    mut fonts: super::draw::FontKit,
    screen: Option<Res<DialogueScreen>>,
    creator: Option<Res<crate::creator::CreatorMenu>>,
    mut state: ResMut<DialogTiles>,
    shared: Res<SharedWritten>,
    hint: Res<GoodbyeHint>,
    touch: Option<Res<crate::dialogue_touch::TouchDialogue>>,
) {
    if !switch.dialog {
        return;
    }
    let Ok(mut written) = shared.0.lock() else { return };
    let written = &mut *written;
    let Some(rt) = script.rt.as_mut() else {
        written.forget(None);
        state.set_if_neq(DialogTiles::default());
        return;
    };
    let game = &mut rt.game;
    if touch.is_some_and(|t| t.enabled) {
        written.fall_back(&mut game.ui_tree);
        state.set_if_neq(DialogTiles::default());
        return;
    }
    if game.dialogue.is_none() || game.ui_files.is_none() {
        written.forget(Some(&mut game.ui_tree));
        state.set_if_neq(DialogTiles::default());
        return;
    }
    ask_for_menu(game, &files, written);
    // One reason for the whole menu to stay hand-built, or none.
    let status = prepare(&mut game.ui_tree, written);
    let menu = game.ui_tree.menu(DIALOG_MENU);
    let (Some(menu), Tiled::Yes) = (menu, status.clone()) else {
        written.warn(&status);
        written.fall_back(&mut game.ui_tree);
        state.set_if_neq(DialogTiles::default());
        return;
    };
    let creating = creator.is_some_and(|c| c.is_open());
    let (view, list) = (crate::dialogue::view(game, &sim).filter(|_| !creating), crate::dialogue::list_id(game));
    keep_speaker(game, view.as_ref(), written);
    let (Some(view), Some(list)) = (view, list) else {
        hide(&mut game.ui_tree, written);
        state.set_if_neq(DialogTiles { menu: Some(menu), shown: false, list: None, ..DialogTiles::default() });
        return;
    };
    let ui = &mut game.ui_tree;
    let now = Shown {
        menu,
        list: list.clone(),
        view,
        selected: screen.as_ref().map_or(0, |s| s.selected()),
        framed: screen.as_ref().is_some_and(|s| s.framed()),
        keyed: screen.as_ref().map_or(0, |s| s.keyed()),
        screen: (num(ui, ui.screen(), "width"), num(ui, ui.screen(), "height")),
        hint: hint.0,
    };
    let mut round = |ui: &mut Ui, texts: &[TileId], sizes: &mut SizesWritten| {
        let size = fonts.menu_measure(&menus::fonts_of(ui, texts));
        menus::write_text_sizes_of(ui, texts, sizes, &size)
    };
    match write(ui, &now, written, &mut round) {
        Ok(()) => {
            if written.moved {
                ui.update();
                info!("{}", describe(ui, &now));
            }
            let (hits, hit_screen) = (std::mem::take(&mut state.hits), state.hit_screen);
            state.set_if_neq(DialogTiles { menu: Some(menu), shown: true, list: Some(list), hits, hit_screen });
        }
        Err(reason) => {
            written.warn(&Tiled::No(reason));
            written.fall_back(ui);
            state.set_if_neq(DialogTiles::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_menu::ChoiceRow;
    use fnv_ui::input::{no_measure, Input, Pointer};

    /// [design] The tree these tests stand on, tile by tile from the paths the code itself looks
    /// up ([`built`]'s list), each with only the plain numbers some test reads; no expressions,
    /// and nothing else of any file. What a real file would work out, [`work_out`] does in the
    /// test's own arithmetic. So these tests give up any proof that the real file agrees with
    /// the model (where rows land, the bar's rule, the band's and the brackets' places): the
    /// smoke scenarios `tiles-dialog-*` run against the real file.
    fn tiles() -> Vec<(String, &'static str, &'static str)> {
        let plain = |path: &str, kind: &'static str, traits: &'static str| (path.to_string(), kind, traits);
        let mut out = vec![
            plain(ROOT, "rect", ""),
            plain(BAND, "image", "<filename>p.dds</filename><width>800</width><x>40</x>"),
            plain(CLICK_AREA, "hotrect", "<height>900</height><width>900</width>"),
            plain(NAME, "text", ""),
            plain(LINE, "text", "<wrapwidth>400</wrapwidth>"),
            plain(LIST, "hotrect", "<_scroll_delta>4</_scroll_delta><clipwindow>1</clipwindow><x>100</x><y>300</y><width>700</width>"),
            plain(HIGHLIGHT, "hotrect", "<width>600</width><x>30</x>"),
            plain(BAR, "image", "<alpha>0</alpha><height>40</height><width>30</width>"),
            plain(TOP_BRACKET, "rect", ""),
            plain(BOTTOM_BRACKET, "rect", "<y>640</y>"),
        ];
        for bracket in [TOP_BRACKET, BOTTOM_BRACKET] {
            out.push((format!("{bracket}/End"), "image", "<filename>e.dds</filename>"));
        }
        for line in FRAME_LINES {
            out.push((format!("{HIGHLIGHT}/{line}"), "image", "<filename>l.dds</filename><height>2</height><width>2</width>"));
        }
        for (i, control) in BAR_CONTROLS.iter().enumerate() {
            let traits = ["<filename>u.dds</filename><target>1</target><height>5</height><width>30</width><y>15</y>", "<filename>d.dds</filename><target>1</target><height>5</height><width>30</width><y>20</y>", "<target>1</target><height>5</height><width>30</width>", "<target>1</target><height>5</height><width>30</width><y>5</y>", "<filename>m.dds</filename><target>1</target><height>5</height><width>30</width><y>10</y>"];
            let kind = if control.contains("page") { "hotrect" } else { "image" };
            out.push((format!("{BAR}/{control}"), kind, traits[i]));
        }
        out
    }

    /// A row: its spacing and size, a target hotrect with a text.
    const ROW_XML: &str = r#"<hotrect name="DM_Topic"><_VerticalSpacing>12</_VerticalSpacing><target>1</target><width>600</width><x>30</x><text name="ListItemText"><wrapwidth>200</wrapwidth></text></hotrect>"#;

    /// The tree without the tiles at `skip`, with `row` as its row template, on a 1280 x 720
    /// window in interface units.
    fn tree(skip: &[&str], row: &str) -> Ui {
        let src = MemSource::default().with("d.xml", &format!(r#"<menu name="DialogMenu"><_MinListHeight>70</_MinListHeight><template name="{ROW_TEMPLATE}">{row}</template></menu>"#));
        let mut ui = Ui::new();
        ui.load_menu(&src, "d.xml").unwrap();
        for (path, kind, traits) in tiles() {
            if skip.contains(&path.as_str()) {
                continue;
            }
            let (parent, name) = path.rsplit_once('/').unwrap();
            ui.inject_text(&MemSource::default(), parent, &format!(r#"<{kind} name="{name}">{traits}</{kind}>"#));
        }
        super::super::write_screen(&mut ui, 1280.0, 720.0, true);
        ui.update();
        ui
    }

    /// What the two steps keep, with [`work_out`] standing in for the real file's expressions.
    fn fresh() -> Written {
        Written { stand_in: Some(work_out), ..Written::default() }
    }

    /// What the real file would work out from code's writes, in the test's own arithmetic:
    /// see [`fixture`].
    fn work_out(ui: &mut Ui) {
        let (Some(list), Some(bar)) = (ui.find_tile(LIST), ui.find_tile(BAR)) else { return };
        let scrolled = num(ui, bar, "_current_value") * num(ui, list, "_scroll_delta");
        for r in rows(ui) {
            let shown = num(ui, r, "listindex") >= 0.0;
            let place = num(ui, r, "_y") - scrolled;
            set(ui, r, "y", Value::Num(place));
            set(ui, r, "visible", Value::Num(f64::from(u8::from(shown))));
            if let Some(text) = child(ui, r, ROW_TEXT) {
                let words = ui.tile(r).get("string").map_or_else(String::new, Value::text);
                let strength = num(ui, r, "_line_alpha");
                set(ui, text, "string", Value::Str(words));
                set(ui, text, "alpha", Value::Num(strength));
            }
        }
        if let Some(h) = ui.find_tile(HIGHLIGHT) {
            let (at, tall) = (num(ui, list, "_highlight_y"), num(ui, list, "_selected_height"));
            set(ui, h, "y", Value::Num(at));
            set(ui, h, "height", Value::Num(tall));
            set(ui, h, "visible", Value::Num(f64::from(u8::from(at + tall >= 0.0))));
        }
        // `_jump_size` is code's, written on every frame ([`write_scroll`]).
        let many = num(ui, bar, "_number_of_items") > num(ui, bar, "_number_of_visible_items");
        set(ui, bar, "visible", Value::Num(f64::from(u8::from(many))));
        if let (Some(area), Some(menu)) = (ui.find_tile(CLICK_AREA), ui.menu(DIALOG_MENU)) {
            let line_up = num(ui, menu, "_ShowingText");
            set(ui, area, "target", Value::Num(line_up));
        }
    }

    /// Prepared as `write_system` prepares it.
    fn menu() -> Ui {
        let mut ui = tree(&[], ROW_XML);
        assert_eq!(built(&ui), Tiled::Yes);
        assert!(own_tiles(&mut ui));
        ui
    }

    /// Every character 10 wide, a line 40 high, wrapped at the wrap width.
    fn size(_font: f64, text: &str, wrap: Option<f64>) -> (f64, f64) {
        let width = text.chars().count() as f64 * 10.0;
        match wrap {
            Some(w) if width > w => (w, (width / w).ceil() * 40.0),
            _ => (width, 40.0),
        }
    }

    fn round(ui: &mut Ui, texts: &[TileId], sizes: &mut SizesWritten) -> usize {
        menus::write_text_sizes_of(ui, texts, sizes, &size)
    }

    fn row(label: &str) -> ChoiceRow {
        ChoiceRow { label: label.to_string(), ..Default::default() }
    }

    fn row_of(label: &str) -> ChoiceRow {
        row(label)
    }

    fn choosing(rows: Vec<ChoiceRow>) -> MenuView {
        MenuView { speaker: "Nora".into(), line: None, choices: rows, goodbye: false }
    }

    /// With the selection framed; `keyed` as if the keys moved it there.
    fn shown(ui: &Ui, view: MenuView, lines: u32, selected: usize) -> Shown {
        let topics: Vec<u32> = (0..view.choices.len() as u32).map(|i| 0x100 + i).collect();
        Shown { menu: ui.menu(DIALOG_MENU).unwrap(), list: ListId::for_test(lines, &topics), view, selected, framed: true, keyed: selected as u64, screen: (1706.0, 960.0), hint: true }
    }

    /// One frame: written, the tree worked out as `super::super::update` does.
    fn frame(ui: &mut Ui, written: &mut Written, now: &Shown) {
        write(ui, now, written, &mut round).unwrap();
        evaluate(ui, written);
    }

    fn frame_on(ui: &mut Ui, written: &mut Written, view: MenuView, lines: u32, selected: usize) {
        let now = shown(ui, view, lines, selected);
        frame(ui, written, &now);
    }

    fn f(ui: &Ui, path: &str, name: &str) -> f64 {
        ui.get_float(&format!("{path}/{name}")).unwrap()
    }

    /// The tile renderer's own selection of what it paints.
    fn pieces(ui: &Ui) -> Vec<super::super::draw::Piece> {
        let menu = ui.menu(DIALOG_MENU).unwrap();
        let measure = |font: f64, text: &str, wrap: Option<f64>| size(font, text, wrap);
        super::super::draw::pieces(ui, menu, &measure, super::super::draw::Select::Whole { kept: &[], strip: None }, true).0
    }

    /// The text drawn: its string and what shows of it.
    fn texts_drawn(ui: &Ui) -> Vec<(String, f32)> {
        pieces(ui)
            .into_iter()
            .filter_map(|p| match p.look {
                super::super::draw::Look::Text { string, .. } => Some((string, p.colour[3])),
                _ => None,
            })
            .collect()
    }

    fn pictures_drawn(ui: &Ui, name: &str) -> usize {
        pieces(ui).iter().filter(|p| matches!(&p.look, super::super::draw::Look::Picture { name: n, .. } if n == name)).count()
    }

    fn ui_mut<'u>(game: &'u mut fnv_game::Game<'_>) -> &'u mut Ui {
        &mut game.ui_tree
    }

    /// A pointer on the screen with this generation.
    fn pointer(stamp: u64) -> Pointer {
        let mut p = Pointer::default();
        p.stamp = stamp;
        p
    }

    fn step(ui: &mut Ui, pointer: &mut Pointer, at: (f64, f64), press: bool) -> Report {
        let menu = ui.menu(DIALOG_MENU).unwrap();
        pointer.step(ui, &[menu], &Input { at: Some(at), press, ..Input::default() }, &no_measure)
    }

    fn asked(ui: &Ui, report: &Report, physical: &[&str], lines: u32) -> ViewInput {
        let mut asked = ViewInput::default();
        follow(ui, report, physical, ui.menu(DIALOG_MENU).unwrap(), Some(&ListId::for_test(lines, &[])), &mut asked, &mut 0);
        asked
    }

    #[test]
    fn the_line_is_written_with_its_speaker_and_no_list() {
        let mut ui = menu();
        let mut written = fresh();
        let view = MenuView { speaker: "Nora".into(), line: Some("Stay a while.".into()), choices: Vec::new(), goodbye: false };
        frame_on(&mut ui, &mut written, view, 1, 0);
        assert_eq!(f(&ui, DIALOG_MENU, "_ShowingText"), 1.0);
        assert_eq!(f(&ui, DIALOG_MENU, "_ShowSubtitles"), 1.0);
        assert_eq!(f(&ui, DIALOG_MENU, "_DialogVisible"), 1.0);
        let drawn = texts_drawn(&ui);
        assert!(drawn.iter().any(|(s, _)| s == "Nora"), "{drawn:?}");
        assert!(drawn.iter().any(|(s, _)| s == "Stay a while."), "{drawn:?}");
        assert!(rows(&ui).is_empty());
        assert_eq!(f(&ui, HINT, "visible"), 0.0);
        // The safe zone the file reads.
        assert_eq!(num(&ui, ui.screen(), "cropy"), 15.0);
        assert_eq!(num(&ui, ui.screen(), "cropx"), 15.0);
        // A response with no text: the name alone, no band behind nothing.
        let view = MenuView { speaker: "Nora".into(), line: None, choices: Vec::new(), goodbye: false };
        frame_on(&mut ui, &mut written, view, 2, 0);
        assert_eq!(f(&ui, DIALOG_MENU, "_ShowSubtitles"), 0.0);
        assert_eq!(texts_drawn(&ui).iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["Nora"]);
    }

    #[test]
    fn choices_stand_in_rows_of_their_wrapped_height() {
        let mut ui = menu();
        let mut written = fresh();
        // Wrap at 200: 25 characters take two lines of 40.
        let view = choosing(vec![row("Short one."), row(&"w".repeat(25)), row("Third.")]);
        frame_on(&mut ui, &mut written, view.clone(), 1, 1);
        let made = rows(&ui);
        assert_eq!(made.len(), 3);
        let g = |t: TileId, n: &str| ui.tile(t).get(n).map_or(-1.0, Value::num);
        assert_eq!((g(made[0], "height"), g(made[1], "height"), g(made[2], "height")), (52.0, 92.0, 52.0));
        assert_eq!((g(made[0], "_y"), g(made[1], "_y"), g(made[2], "_y")), (0.0, 52.0, 144.0));
        // They fit: the list holds them, the bar stays away, the frame on the selected row.
        assert_eq!(f(&ui, LIST, "height"), 196.0);
        assert_eq!(f(&ui, BAR, "visible"), 0.0);
        assert_eq!((f(&ui, LIST, "_highlight_y"), f(&ui, LIST, "_selected_height")), (52.0, 92.0));
        assert_eq!(f(&ui, HIGHLIGHT, "y"), 52.0);
        // A list that wraps otherwise after a script's change: the row is measured and moved.
        ui.set_trait(made[0], "string", Value::Str("x".repeat(25)));
        ui.update();
        frame_on(&mut ui, &mut written, view, 1, 1);
        assert_eq!(ui.tile(made[0]).get("height").map(Value::num), Some(92.0));
        assert_eq!(ui.tile(made[1]).get("_y").map(Value::num), Some(92.0));
    }

    #[test]
    fn a_list_opens_with_nothing_framed() {
        let mut ui = menu();
        let mut written = fresh();
        let now = Shown { framed: false, keyed: 0, ..shown(&ui, choosing(vec![row("A."), row("B."), row("C.")]), 1, 0) };
        frame(&mut ui, &mut written, &now);
        assert_eq!((f(&ui, LIST, "_highlight_y"), f(&ui, LIST, "_selected_height")), (-1.0, 0.0));
        assert!(painted(&ui, &pieces(&ui)).contains("frame 0 plain, fill 0"), "{}", painted(&ui, &pieces(&ui)));
        assert!(describe(&ui, &now).contains("selected none"));
        // The first press frames the first choice.
        let keyed = Shown { framed: true, keyed: 1, ..now };
        frame(&mut ui, &mut written, &keyed);
        assert_eq!((f(&ui, LIST, "_highlight_y"), f(&ui, LIST, "_selected_height")), (0.0, 52.0));
        assert!(painted(&ui, &pieces(&ui)).contains("frame 4 plain, fill 1"));
    }

    #[test]
    fn a_short_list_keeps_the_files_least_height() {
        let mut ui = menu();
        frame_on(&mut ui, &mut fresh(), choosing(vec![row("One.")]), 1, 0);
        // 52 of row, the file's least 70.
        assert_eq!(f(&ui, LIST, "height"), 70.0);
        assert_eq!(f(&ui, DIALOG_MENU, "_MinListHeight"), 70.0);
    }

    /// Six rows, the first and third of two lines: 96, 58, 96, 58, 58, 58 high, as
    /// `tiles-dialog-scroll`'s list at 1280 x 720; the list 250 high, its first three rows.
    /// [design: the heights from the smoke scenario's note; the labels and this tree are the
    /// test's own.]
    fn six_rows() -> (Ui, Written, MenuView) {
        let mut ui = tree(&[], &ROW_XML.replace("<_VerticalSpacing>12</_VerticalSpacing>", "<_VerticalSpacing>20</_VerticalSpacing>"));
        assert!(own_tiles(&mut ui));
        let mut labels: Vec<ChoiceRow> = (0..6).map(|i| row(&format!("Choice {i}."))).collect();
        labels[0].label = "a".repeat(25);
        labels[2].label = "b".repeat(25);
        (ui, fresh(), choosing(labels))
    }

    #[test]
    fn the_list_scrolls_by_whole_rows_to_the_framed_row() {
        // [design: owner choice 2026-11-05, whole rows] The first row shown is always whole: the
        // value is its place, the frame stands at the selected row's place less it.
        let (mut ui, mut written, view) = six_rows();
        let mut go = |ui: &mut Ui, selected: usize, keyed: u64| {
            let now = Shown { keyed, ..shown(ui, view.clone(), 1, selected) };
            write(ui, &now, &mut written, &mut round_of_38).unwrap();
            evaluate(ui, &written);
            let line = describe(ui, &now);
            let at = line.find("scroll ").unwrap() + "scroll ".len();
            let first: usize = line[at..].split(' ').next().unwrap().parse().unwrap();
            (first, f(ui, BAR, "_current_value"), f(ui, LIST, "_highlight_y"), f(ui, BAR, "_jump_size"))
        };
        assert_eq!(go(&mut ui, 0, 1), (0, 0.0, 0.0, 3.0));
        let made = rows(&ui);
        let heights: Vec<f64> = made.iter().map(|&r| num(&ui, r, "height")).collect();
        assert_eq!(heights, [96.0, 58.0, 96.0, 58.0, 58.0, 58.0]);
        assert_eq!(f(&ui, LIST, "height"), 250.0);
        assert_eq!(f(&ui, LIST, "_scroll_delta"), 1.0, "code's delta: the value is in units");
        // The last first row is 3: rows 3 to 5 fit (174), rows 2 to 5 do not (270). The bar
        // counts to its place, 250, so the marker's range ends with it.
        assert_eq!((f(&ui, BAR, "_number_of_items"), f(&ui, BAR, "_number_of_visible_items")), (251.0, 1.0));
        assert_eq!(f(&ui, BAR, "visible"), 1.0);
        // Down: the first row rises only as far as shows the framed row whole.
        let down = [(1, (0, 0.0, 96.0, 3.0)), (2, (0, 0.0, 154.0, 3.0)), (3, (1, 96.0, 154.0, 3.0)), (4, (2, 154.0, 154.0, 3.0)), (5, (3, 250.0, 116.0, 3.0))];
        for (k, (selected, want)) in down.into_iter().enumerate() {
            assert_eq!(go(&mut ui, selected, 2 + k as u64), want, "row {selected} going down");
        }
        let now = Shown { keyed: 6, ..shown(&ui, view.clone(), 1, 5) };
        assert!(describe(&ui, &now).contains(", scroll 3 of 3, bar true"), "{}", describe(&ui, &now));
        // Up: the scroll holds while the framed row shows, then the row comes to the top.
        let up = [(4, (3, 250.0, 58.0, 3.0)), (3, (3, 250.0, 0.0, 3.0)), (2, (2, 154.0, 0.0, 3.0)), (1, (1, 96.0, 0.0, 3.0)), (0, (0, 0.0, 0.0, 3.0))];
        for (k, (selected, want)) in up.into_iter().enumerate() {
            assert_eq!(go(&mut ui, selected, 100 + k as u64), want, "row {selected} going up");
        }
        assert_eq!(f(&ui, LIST, "height"), 250.0, "the list's height stays");
    }

    #[test]
    fn a_list_that_fits_does_not_scroll() {
        let (mut ui, mut written, _) = six_rows();
        let view = choosing(vec![row("One."), row(&"c".repeat(25)), row("Three.")]);
        for (k, selected) in [0, 1, 2, 1].into_iter().enumerate() {
            let now = Shown { keyed: k as u64 + 1, ..shown(&ui, view.clone(), 1, selected) };
            write(&mut ui, &now, &mut written, &mut round_of_38).unwrap();
            evaluate(&mut ui, &written);
            assert!(describe(&ui, &now).contains(", scroll 0 of 0, bar false"), "{}", describe(&ui, &now));
        }
        assert_eq!(f(&ui, BAR, "_current_value"), 0.0);
        assert_eq!((f(&ui, BAR, "_number_of_items"), f(&ui, BAR, "_number_of_visible_items")), (1.0, 1.0));
        assert_eq!(f(&ui, BAR, "visible"), 0.0);
        // Asked to scroll, it holds.
        written.scroll_asked += 2;
        let now = Shown { keyed: 4, ..shown(&ui, view, 1, 1) };
        write(&mut ui, &now, &mut written, &mut round_of_38).unwrap();
        assert_eq!(f(&ui, BAR, "_current_value"), 0.0);
    }

    #[test]
    fn the_last_first_row_leaves_the_rest_in_the_window() {
        let (tops, heights) = ([0.0, 96.0, 154.0, 250.0, 308.0, 366.0], [96.0, 58.0, 96.0, 58.0, 58.0, 58.0]);
        assert_eq!(last_first(&tops, &heights, 250.0), 3, "rows 3 to 5 make 174; rows 2 to 5 make 270, over 250");
        assert_eq!(last_first(&tops, &heights, 270.0), 2, "exactly the rest from row 2");
        assert_eq!(last_first(&tops, &heights, 500.0), 0, "the whole list (424) fits: no scroll");
        // A last row taller than the window: it comes to the top, its foot cut.
        assert_eq!(last_first(&[0.0, 50.0], &[50.0, 300.0], 100.0), 1, "no first row fits the rest: the last row");
        assert_eq!(scroll_to_show(0, 1, &[0.0, 50.0], &[50.0, 300.0], 100.0, 1), 1, "the tall row starts at the top");
        assert_eq!(rows_fitting(0, &tops, &heights, 250.0), 3, "96 + 58 + 96 = 250, the window exactly");
        assert_eq!(rows_fitting(1, &tops, &heights, 100.0), 1, "58 fits; 58 + 96 = 154 does not");
        assert_eq!(rows_fitting(0, &tops, &heights, 50.0), 1, "at least one, though none fits whole");
        assert_eq!(scroll_to_show(3, 1, &tops, &heights, 250.0, 3), 1, "a row above comes to the top");
        assert_eq!(scroll_to_show(0, 5, &tops, &heights, 250.0, 3), 3, "row 5 ends at 424: the first row at 174 or below, row 3 at 250");
        assert_eq!(scroll_to_show(0, 2, &tops, &heights, 250.0, 3), 0, "a row ending at the window's foot shows");
    }

    /// One list as the capture had it: 12 rows, the fourth of two lines.
    fn the_captured_list() -> (Ui, Written, MenuView) {
        // [observed: the capture's heights, 58 and 96. The labels and this tree are the test's
        // own.]
        let mut ui = tree(&[], &ROW_XML.replace("<_VerticalSpacing>12</_VerticalSpacing>", "<_VerticalSpacing>20</_VerticalSpacing>"));
        assert!(own_tiles(&mut ui));
        let mut labels: Vec<ChoiceRow> = (0..12).map(|i| row(&format!("Choice {i}."))).collect();
        labels[3].label = "w".repeat(25);
        (ui, fresh(), choosing(labels))
    }

    fn round_of_38(ui: &mut Ui, texts: &[TileId], sizes: &mut SizesWritten) -> usize {
        menus::write_text_sizes_of(ui, texts, sizes, &|_: f64, text: &str, wrap: Option<f64>| {
            let width = text.chars().count() as f64 * 10.0;
            (width.min(wrap.unwrap_or(width)), (width / wrap.unwrap_or(width).max(1.0)).ceil().max(1.0) * 38.0)
        })
    }

    #[test]
    fn the_bars_arrows_and_pages_scroll_by_rows_and_leave_the_frame_on_its_row() {
        // Rows of 58, the fourth 96 (places 0, 58, 116, 174, 270, 328 … 676); the list 174 high.
        let (mut ui, mut written, view) = the_captured_list();
        let now = Shown { keyed: 0, ..shown(&ui, view, 1, 0) };
        let bar_piece = |ui: &Ui, n: &str| match dialog_tile(ui, ui.find_tile(&format!("{BAR}/{n}")).unwrap()) {
            Some(DialogTile::Scroll(steps)) => steps,
            other => panic!("{n}: {other:?}"),
        };
        let go = |ui: &mut Ui, written: &mut Written, steps: i32| {
            written.scroll_asked += steps;
            write(ui, &now, written, &mut round_of_38).unwrap();
            evaluate(ui, written);
            (f(ui, BAR, "_current_value"), f(ui, LIST, "_highlight_y"), f(ui, BAR, "_jump_size"))
        };
        assert_eq!(go(&mut ui, &mut written, 0), (0.0, 0.0, 3.0));
        // The last first row is 9 (560): rows 9 to 11 fill the window.
        assert_eq!(f(&ui, BAR, "_number_of_items"), 561.0);
        // An arrow: a row, the frame on the first row carried off with it; from row 1 two whole
        // rows fit (58 + 58 = 116; the 96 after them would make 212, over 174), so a page is two.
        let down = bar_piece(&ui, "scrollbar_vert_down");
        assert_eq!(down, 1);
        assert_eq!(go(&mut ui, &mut written, down), (58.0, -58.0, 2.0));
        let page = bar_piece(&ui, "scrollbar_vert_page_down");
        assert_eq!(page, 2);
        assert_eq!(go(&mut ui, &mut written, page), (174.0, -174.0, 2.0));
        assert_eq!(f(&ui, HIGHLIGHT, "visible"), 0.0, "off the top, no frame");
        let page = bar_piece(&ui, "scrollbar_vert_page_down");
        assert_eq!(go(&mut ui, &mut written, page), (328.0, -328.0, 3.0));
        let up = bar_piece(&ui, "scrollbar_vert_up");
        assert_eq!(up, -1);
        assert_eq!(go(&mut ui, &mut written, up), (270.0, -270.0, 3.0));
        let page = bar_piece(&ui, "scrollbar_vert_page_up");
        assert_eq!(page, -3);
        assert_eq!(go(&mut ui, &mut written, page), (58.0, -58.0, 2.0));
        // The ends hold: the top, and the last first row.
        assert_eq!(go(&mut ui, &mut written, -9).0, 0.0);
        assert_eq!(go(&mut ui, &mut written, 99), (560.0, -560.0, 3.0));
        assert!(describe(&ui, &now).contains(", scroll 9 of 9, "), "{}", describe(&ui, &now));
        assert_eq!(pictures_drawn(&ui, "m.dds"), 1, "a marker though the bar itself has no alpha");
        // The window shows the last three rows whole and nothing of the row above.
        let drawn = painted(&ui, &pieces(&ui));
        assert!(drawn.contains("rows 3,"), "{drawn}");
    }

    #[test]
    fn the_scroll_is_written_again_over_what_the_file_worked_out() {
        // [design] The real prefab counts clicks from 0 in this tree; here anything else standing
        // in the value will do: the model's goes back every frame.
        let (mut ui, mut written, view) = the_captured_list();
        let now = shown(&ui, view, 1, 5);
        let go = |ui: &mut Ui, written: &mut Written| {
            write(ui, &now, written, &mut round_of_38).unwrap();
            evaluate(ui, written);
        };
        go(&mut ui, &mut written);
        // Row 5 (328 to 386) whole in 174: the first row at 212 or below, row 4 at 270.
        assert_eq!(f(&ui, BAR, "_current_value"), 270.0);
        ui.set_trait(ui.find_tile(BAR).unwrap(), "_current_value", Value::Num(1.0));
        ui.set_trait(ui.find_tile(BAR).unwrap(), "_jump_size", Value::Num(9.0));
        go(&mut ui, &mut written);
        assert_eq!(f(&ui, BAR, "_current_value"), 270.0);
        assert_eq!(f(&ui, BAR, "_jump_size"), 3.0);
        assert_eq!(f(&ui, BAR, "_SetInCode"), 1.0);
    }

    #[test]
    fn one_oversized_choice_makes_its_own_height_and_an_empty_list_has_no_rows() {
        let mut ui = menu();
        let mut written = fresh();
        // Wrap at 200: 150 characters are eight lines, 332 with the spacing.
        let view = choosing(vec![row(&"o".repeat(150)), row("After.")]);
        frame_on(&mut ui, &mut written, view.clone(), 1, 0);
        assert_eq!(f(&ui, LIST, "height"), 384.0);
        // Both rows are among the first three, so the list holds them: no scroll, by rows.
        assert!(describe(&ui, &shown(&ui, view, 1, 0)).contains(", scroll 0 of 0, bar false"));
        assert_eq!(f(&ui, BAR, "_current_value"), 0.0);
        assert_eq!(f(&ui, BAR, "_number_of_items"), 1.0);
        // The list gone: its rows too.
        let view = MenuView { speaker: "Nora".into(), line: Some("Well.".into()), choices: Vec::new(), goodbye: false };
        frame_on(&mut ui, &mut written, view, 2, 0);
        assert!(rows(&ui).is_empty());
        assert_eq!(f(&ui, LIST, "_selected_height"), 0.0);
        assert_eq!((f(&ui, BAR, "_current_value"), f(&ui, BAR, "_number_of_items")), (0.0, 1.0), "no rows, no scroll");
    }

    #[test]
    fn a_dim_row_and_a_short_checks_frame_are_painted_so() {
        let mut ui = menu();
        let mut written = fresh();
        let short = ChoiceRow { label: "[Sneak 17/25]  I could.".into(), short: true, ..Default::default() };
        let dim = ChoiceRow { label: "Said before.".into(), darkened: true, ..Default::default() };
        let view = choosing(vec![short, row("Plain."), dim]);
        frame_on(&mut ui, &mut written, view.clone(), 1, 0);
        let alpha = |ui: &Ui, s: &str| texts_drawn(ui).iter().find(|(t, _)| t == s).unwrap().1;
        assert!((alpha(&ui, "Said before.") - 128.0 / 255.0).abs() < 1e-4);
        assert_eq!(alpha(&ui, "Plain."), 1.0);
        // The frame's four lines red, its fill dark red: the short check is selected.
        let lines = |ui: &Ui| pieces(ui).into_iter().filter(|p| matches!(&p.look, super::super::draw::Look::Picture { name, .. } if name == "l.dds")).collect::<Vec<_>>();
        assert_eq!(lines(&ui).len(), 4);
        let [r, g, b, _] = lines(&ui)[0].colour;
        let bright = 255.0;
        assert!((r * bright - 127.5).abs() < 0.5 && (g * bright - 22.95).abs() < 0.5 && (b * bright - 20.4).abs() < 0.5, "{r} {g} {b}");
        let fill = pieces(&ui).into_iter().find(|p| matches!(p.look, super::super::draw::Look::Box)).expect("the fill");
        assert!((fill.colour[0] * 255.0 - 62.0).abs() < 0.5 && (fill.colour[3] * 255.0 - 82.0).abs() < 0.5, "{:?}", fill.colour);
        // Off the check the frame goes back to the interface's colour.
        frame_on(&mut ui, &mut written, view, 1, 1);
        let [r, g, b, _] = lines(&ui)[0].colour;
        assert!(g > 0.2 && (r, g, b) != (0.5, 0.09, 0.08), "the HUD's colour: {r} {g} {b}");
        let fill = pieces(&ui).into_iter().find(|p| matches!(p.look, super::super::draw::Look::Box)).unwrap();
        assert!((fill.colour[3] * 255.0 - 25.5).abs() < 0.5);
    }

    #[test]
    fn the_hint_shows_while_a_goodbye_is_offered_anchored_to_the_files_geometry() {
        let mut ui = menu();
        let mut written = fresh();
        let mut view = choosing(vec![row("Bye."), row("Stay.")]);
        frame_on(&mut ui, &mut written, view.clone(), 1, 0);
        assert!(!texts_drawn(&ui).iter().any(|(s, _)| s == GOODBYE_HINT));
        assert_eq!(pictures_drawn(&ui, "Interface\\Shared\\Background\\solid_black.dds"), 0);
        view.goodbye = true;
        frame_on(&mut ui, &mut written, view.clone(), 1, 0);
        assert!(texts_drawn(&ui).iter().any(|(s, _)| s == GOODBYE_HINT));
        assert_eq!(pictures_drawn(&ui, "Interface\\Shared\\Background\\solid_black.dds"), 1);
        // Left of the band where it has room? This band is at 40, so the safe zone's edge; and
        // level with the bottom bracket, less half a line.
        assert_eq!(f(&ui, HINT, "x"), CROP + HINT_PAD);
        assert_eq!(f(&ui, HINT, "y"), 620.0);
        // A wider gap: right end of the text 20 left of the band.
        ui.set_trait(ui.find_tile(BAND).unwrap(), "x", Value::Num(400.0));
        let moved = MenuView { speaker: "Nora again".into(), ..view };
        frame_on(&mut ui, &mut written, moved, 1, 0);
        assert_eq!(f(&ui, HINT, "x"), 400.0 - HINT_GAP - size(0.0, GOODBYE_HINT, None).0);
        // Code's own, and not taken for a mod's tile.
        let menu = ui.menu(DIALOG_MENU).unwrap();
        let hint = ui.find_tile(HINT).unwrap();
        assert!(!ui.mod_added_tiles(menu).contains(&hint), "code tiles are not a mod's");
        assert_eq!(ui.tile(hint).origin, Origin::Code);
        // During a line: no hint, whatever the model last said of it.
        let line = MenuView { speaker: "Nora".into(), line: Some("Fine.".into()), choices: Vec::new(), goodbye: false };
        frame_on(&mut ui, &mut written, line, 2, 0);
        assert!(!texts_drawn(&ui).iter().any(|(s, _)| s == GOODBYE_HINT));
    }

    #[test]
    fn the_hint_setting_hides_only_the_hint_at_once() {
        let mut ui = menu();
        let mut written = fresh();
        let view = MenuView { goodbye: true, ..choosing(vec![row("Bye.")]) };
        let now = shown(&ui, view, 1, 0);
        frame(&mut ui, &mut written, &now);
        assert!(texts_drawn(&ui).iter().any(|(s, _)| s == GOODBYE_HINT));
        frame(&mut ui, &mut written, &Shown { hint: false, ..now.clone() });
        assert!(written.moved, "the setting is in the cache's key");
        assert!(!texts_drawn(&ui).iter().any(|(s, _)| s == GOODBYE_HINT));
        assert!(texts_drawn(&ui).iter().any(|(s, _)| s == "Bye."), "the list stays");
        frame(&mut ui, &mut written, &now);
        assert!(texts_drawn(&ui).iter().any(|(s, _)| s == GOODBYE_HINT));
    }

    #[test]
    fn what_is_painted_is_told_from_the_renderers_pieces() {
        let mut ui = menu();
        let short = ChoiceRow { label: "[Sneak 17/25]  I could.".into(), short: true, ..Default::default() };
        let dim = ChoiceRow { label: "Said before.".into(), darkened: true, ..Default::default() };
        let view = MenuView { goodbye: true, ..choosing(vec![short, row("Plain."), dim]) };
        frame_on(&mut ui, &mut fresh(), view, 1, 0);
        assert_eq!(painted(&ui, &pieces(&ui)), "tiles: DialogMenu painted: band 1, name 1, line 0, rows 3, dim 1, frame 4 red, fill 1, bar 0, brackets 0, hint 1");
    }

    #[test]
    fn the_pointer_selects_and_picks_on_the_rows_as_they_are_drawn() {
        let mut ui = menu();
        let labels: Vec<ChoiceRow> = (0..8).map(|i| row(&format!("Choice {i}."))).collect();
        frame_on(&mut ui, &mut fresh(), choosing(labels), 1, 0);
        let mut p = pointer(1);
        // The list at 300: rows of 52, so y 330 is choice 0, y 370 choice 1.
        let r = step(&mut ui, &mut p, (200.0, 330.0), false);
        assert_eq!(asked(&ui, &r, &[], 1).hover, Some(0));
        let r = step(&mut ui, &mut p, (200.0, 370.0), true);
        let a = asked(&ui, &r, &[], 1);
        assert_eq!((a.hover, a.pick), (Some(1), Some(1)));
        // Under the list's clip: a row laid out there takes nothing.
        let r = step(&mut ui, &mut p, (200.0, 460.0), true);
        assert_eq!(asked(&ui, &r, &[], 1).pick, None);
    }

    #[test]
    fn hits_land_on_what_was_drawn_through_an_animation_and_a_press_on_a_new_size_is_lost() {
        let mut state_ui = crate::script::ScriptState::over_no_plugins();
        let game = &mut state_ui.rt.as_mut().unwrap().game;
        game.ui_tree = menu();
        game.dialogue = Some(fnv_game::dialogue::Conversation { actor: fnv_sim::FormId(0x14), through: None, line: None, choices: Vec::new(), topic_list: false, lines: 1, then: None, ending: false });
        let labels: Vec<ChoiceRow> = (0..8).map(|i| row(&format!("Choice {i}."))).collect();
        frame_on(&mut game.ui_tree, &mut fresh(), choosing(labels), 1, 0);
        let mut state = DialogTiles { menu: game.ui_tree.menu(DIALOG_MENU), shown: true, ..DialogTiles::default() };
        state.take_hits(&game.ui_tree);
        // The rows and the bar's controls, resolved.
        let ui = &game.ui_tree;
        let held = |t: TileId| state.hits().iter().any(|c| c.under == t);
        assert!(rows(ui).iter().all(|&r| held(r)), "every row; the clip tells which can be reached");
        assert!(BAR_CONTROLS.iter().all(|n| held(ui.find_tile(&format!("{BAR}/{n}")).unwrap())), "the bar's controls");
        // Something animates a row away between the drawing and the input: input goes on,
        // and on the row where it was drawn.
        let on_row = rows(ui)[0];
        ui_mut(game).set_trait(on_row, "y", Value::Num(500.0));
        game.ui_tree.update();
        assert!(state.taking(game, false).is_some(), "an animation does not stop input");
        let menu_tile = game.ui_tree.menu(DIALOG_MENU).unwrap();
        let mut p = pointer(1);
        let input = Input { at: Some((200.0, 330.0)), press: true, resolved: Some(state.hits().to_vec()), ..Input::default() };
        let r = p.step(&mut game.ui_tree, &[menu_tile], &input, &no_measure);
        assert_eq!(r.clicks.first().map(|c| c.tile), Some(on_row));
        // A new window size before the next input: that frame only, nothing taken.
        assert!(state.taking(game, true).is_none(), "the pause menu up");
        super::super::write_screen(&mut game.ui_tree, 1600.0, 720.0, true);
        game.ui_tree.update();
        assert!(state.taking(game, false).is_none(), "the new size");
        state.take_hits(&game.ui_tree);
        assert!(state.taking(game, false).is_some(), "the next drawing's");
    }

    #[test]
    fn a_key_the_hand_built_input_took_does_not_act_again() {
        let mut ui = menu();
        frame_on(&mut ui, &mut fresh(), choosing(vec![row("A."), row("B.")]), 1, 0);
        let bound = rows(&ui)[1];
        for key in ["xbuttona", "E", "S", "xdown"] {
            let click = fnv_ui::input::Click { tile: bound, by: By::Key(key.into()), id: None, stamp: 1 };
            let r = Report { clicks: vec![click.clone()], ..Report::default() };
            assert_eq!(asked(&ui, &r, &[key], 1).pick, None, "{key} by hand");
            // A script's MenuTapKey is not physical: the file's own binding.
            assert_eq!(asked(&ui, &r, &[], 1).pick, Some(1), "{key} tapped by a script");
        }
        // A highlight moved for a hand-built Down is not the pointer's.
        let r = Report { over: Some((None, Some(bound))), ..Report::default() };
        assert_eq!(asked(&ui, &r, &["xdown"], 1).hover, None);
        assert_eq!(asked(&ui, &r, &[], 1).hover, Some(1));
        // A press on a row and a queued click on another: the first in the frame.
        let first = rows(&ui)[0];
        let click = |tile| fnv_ui::input::Click { tile, by: By::Pointer, id: None, stamp: 1 };
        let r = Report { clicks: vec![click(first), click(bound)], ..Report::default() };
        assert_eq!(asked(&ui, &r, &[], 1).pick, Some(0));
    }

    #[test]
    fn a_script_click_is_followed_and_a_stale_one_finds_its_row_gone() {
        let mut ui = menu();
        let mut written = fresh();
        let mut p = pointer(1);
        frame_on(&mut ui, &mut written, choosing(vec![row("A."), row("B.")]), 1, 0);
        let on_b = rows(&ui)[1];
        p.script_click(&mut ui, on_b);
        let r = step(&mut ui, &mut p, (0.0, 0.0), false);
        assert_eq!(asked(&ui, &r, &[], 1).pick, Some(1), "a script's click, reported at the next step");
        // Queued, then the conversation went on: the rows of the new list are other tiles.
        let stale = rows(&ui)[1];
        p.script_click(&mut ui, stale);
        p.stamp = 2;
        frame_on(&mut ui, &mut written, choosing(vec![row("C."), row("D.")]), 2, 0);
        assert!(!ui.is_live(stale), "the old list's row went with it");
        let r = step(&mut ui, &mut p, (0.0, 0.0), false);
        assert_eq!(asked(&ui, &r, &[], 2).pick, None);
    }

    #[test]
    fn a_skip_queued_on_one_response_does_not_skip_the_next() {
        // The click area lasts from response to response, so nothing else tells them apart.
        let mut ui = menu();
        let mut written = fresh();
        let line = |t: &str| MenuView { speaker: "Nora".into(), line: Some(t.into()), choices: Vec::new(), goodbye: false };
        frame_on(&mut ui, &mut written, line("First."), 1, 0);
        let area = ui.find_tile(CLICK_AREA).unwrap();
        let mut p = pointer(1);
        p.script_click(&mut ui, area);
        // The response ends by itself before the click is reported. The same goes for the same
        // actor's next conversation, whose stamp also differs.
        p.stamp = 2;
        frame_on(&mut ui, &mut written, line("Second."), 2, 0);
        let r = step(&mut ui, &mut p, (1000.0, 1000.0), false);
        assert_eq!(r.clicks.len(), 1);
        assert!(!asked(&ui, &r, &[], 2).skip, "from an earlier screen");
        // On the screen it was made on, it skips.
        p.script_click(&mut ui, area);
        let r = step(&mut ui, &mut p, (1000.0, 1000.0), false);
        assert!(asked(&ui, &r, &[], 2).skip);
    }

    #[test]
    fn the_click_area_skips_and_the_bar_asks_for_steps() {
        let mut ui = menu();
        let mut written = fresh();
        let line = MenuView { speaker: "Nora".into(), line: Some("Well.".into()), choices: Vec::new(), goodbye: false };
        frame_on(&mut ui, &mut written, line, 1, 0);
        let area = ui.find_tile(CLICK_AREA).unwrap();
        assert_eq!(dialog_tile(&ui, area), Some(DialogTile::Skip));
        let click = fnv_ui::input::Click { tile: area, by: By::Pointer, id: None, stamp: 1 };
        let r = Report { clicks: vec![click.clone(), click], ..Report::default() };
        assert!(asked(&ui, &r, &[], 1).skip, "twice in a frame is one skip");
        // An overflowing list: the arrows and pages ask, the marker does nothing yet.
        let labels: Vec<ChoiceRow> = (0..8).map(|i| row(&format!("Choice {i}."))).collect();
        frame_on(&mut ui, &mut written, choosing(labels), 2, 0);
        let piece = |ui: &Ui, n: &str| dialog_tile(ui, ui.find_tile(&format!("{BAR}/{n}")).unwrap());
        assert_eq!(piece(&ui, "scrollbar_vert_up"), Some(DialogTile::Scroll(-1)));
        assert_eq!(piece(&ui, "scrollbar_vert_down"), Some(DialogTile::Scroll(1)));
        // A page is the whole rows that fit: three of 52 in 156.
        assert_eq!(piece(&ui, "scrollbar_vert_page_up"), Some(DialogTile::Scroll(-3)));
        assert_eq!(piece(&ui, "scrollbar_vert_page_down"), Some(DialogTile::Scroll(3)));
        assert_eq!(piece(&ui, "scrollbar_vert_marker"), None);
        assert_eq!(dialog_tile(&ui, rows(&ui)[1]), Some(DialogTile::Choice(1)));
        // A click on the arrow asks once, for this screen.
        let down = ui.find_tile(&format!("{BAR}/scrollbar_vert_down")).unwrap();
        let click = fnv_ui::input::Click { tile: down, by: By::Pointer, id: None, stamp: 2 };
        let r = Report { clicks: vec![click], ..Report::default() };
        let mut steps = 0;
        follow(&ui, &r, &[], ui.menu(DIALOG_MENU).unwrap(), Some(&ListId::for_test(2, &[])), &mut ViewInput::default(), &mut steps);
        assert_eq!(steps, 1);
    }

    #[test]
    fn a_tree_lacking_a_piece_keeps_the_whole_hand_built_menu() {
        let needed: Vec<String> = [BAND, CLICK_AREA, NAME, LINE, LIST, HIGHLIGHT, BAR, TOP_BRACKET, BOTTOM_BRACKET].map(String::from).into_iter().chain(FRAME_LINES.map(|n| format!("{HIGHLIGHT}/{n}"))).chain(BAR_CONTROLS.map(|n| format!("{BAR}/{n}"))).collect();
        for path in needed {
            let ui = tree(&[path.as_str()], ROW_XML);
            assert_eq!(built(&ui), Tiled::No(format!("{path} missing")), "{path}");
        }
        assert_eq!(built(&Ui::new()), Tiled::No(format!("{DIALOG_MENU} missing")));
        // A template that makes no row, or a row with no text: nothing written, nothing left.
        for (row, reason) in [
            (r#"<hotrect name="Other"/>"#, format!("{ROW_TEMPLATE} makes no {ROW}")),
            (r#"<hotrect name="DM_Topic"><target>1</target></hotrect>"#, format!("{ROW_TEMPLATE} makes no {ROW_TEXT}")),
            ("", format!("no row from {ROW_TEMPLATE}")),
        ] {
            let mut ui = tree(&[], row);
            assert!(own_tiles(&mut ui));
            let now = shown(&ui, choosing(vec![row_of("A."), row_of("B.")]), 1, 0);
            assert_eq!(write(&mut ui, &now, &mut fresh(), &mut round), Err(reason), "{row}");
            assert!(rows(&ui).is_empty(), "{row}");
        }
    }

    #[test]
    fn a_tree_with_nothing_to_click_or_no_room_is_not_ready() {
        let view = choosing(vec![row("A.")]);
        // Sizes the tree works out to nothing, or a row that takes no click.
        for (path, name, reason) in [
            (BAND, "width", format!("{BAND} has no width")),
            (LIST, "width", format!("{LIST} has no width")),
            (HIGHLIGHT, "width", format!("{HIGHLIGHT} has no width")),
            (CLICK_AREA, "width", format!("{CLICK_AREA} has no width")),
            (CLICK_AREA, "height", format!("{CLICK_AREA} has no height")),
        ] {
            let mut ui = menu();
            ui.set_trait(ui.find_tile(path).unwrap(), name, Value::Num(0.0));
            let now = shown(&ui, view.clone(), 1, 0);
            assert_eq!(write(&mut ui, &now, &mut fresh(), &mut round), Err(reason.clone()), "{path} {name}");
        }
        let mut ui = tree(&[], &ROW_XML.replace("<target>1</target>", "<target>0</target>"));
        assert!(own_tiles(&mut ui));
        let now = shown(&ui, view.clone(), 1, 0);
        assert_eq!(write(&mut ui, &now, &mut fresh(), &mut round), Err(format!("a {ROW} cannot be clicked")));
        let mut ui = tree(&[], &ROW_XML.replace("<wrapwidth>200</wrapwidth>", ""));
        assert!(own_tiles(&mut ui));
        let now = shown(&ui, view, 1, 0);
        assert_eq!(write(&mut ui, &now, &mut fresh(), &mut round), Err(format!("a {ROW} has no room for its text")));
        // The click area a target only with a line up: a tree that makes it none is not whole.
        let line = MenuView { speaker: "Nora".into(), line: Some("Well.".into()), choices: Vec::new(), goodbye: false };
        let mut ui = menu();
        let now = shown(&ui, line, 1, 0);
        let mut written = Written { stand_in: Some(|ui| { work_out(ui); let area = ui.find_tile(CLICK_AREA).unwrap(); ui.set_trait(area, "target", Value::Num(0.0)); }), ..Written::default() };
        assert_eq!(write(&mut ui, &now, &mut written, &mut round), Err(format!("{CLICK_AREA} takes no click while the line is up")));
    }

    #[test]
    fn a_tree_without_what_it_has_to_draw_or_reach_is_not_ready() {
        let many = choosing((0..8).map(|i| row(&format!("Choice {i}."))).collect());
        let frame_top = format!("{HIGHLIGHT}/top");
        let bracket = format!("{TOP_BRACKET}/End");
        let (marker, up) = (format!("{BAR}/scrollbar_vert_marker"), format!("{BAR}/scrollbar_vert_up"));
        // The tiles named, their pictures gone.
        for (path, reason) in [
            (BAND, format!("{BAND} draws nothing")),
            (frame_top.as_str(), format!("{frame_top} draws nothing")),
            (bracket.as_str(), format!("{TOP_BRACKET} draws nothing")),
            (marker.as_str(), format!("{marker} draws nothing")),
            (up.as_str(), format!("{up} draws nothing")),
        ] {
            let mut ui = menu();
            ui.set_trait(ui.find_tile(path).unwrap(), "filename", Value::Str(String::new()));
            let now = shown(&ui, many.clone(), 1, 0);
            assert_eq!(write(&mut ui, &now, &mut fresh(), &mut round), Err(reason.clone()), "{path}");
        }
        // A control that takes no click.
        let mut ui = menu();
        ui.set_trait(ui.find_tile(&up).unwrap(), "target", Value::Num(0.0));
        let now = shown(&ui, many.clone(), 1, 0);
        assert_eq!(write(&mut ui, &now, &mut fresh(), &mut round), Err(format!("{up} has no region to click")));
        // A row of a kind the pointer does not take, for all its target and size.
        let mut ui = tree(&[], &ROW_XML.replace("hotrect", "rect"));
        assert!(own_tiles(&mut ui));
        let now = shown(&ui, choosing(vec![row("A.")]), 1, 0);
        assert_eq!(write(&mut ui, &now, &mut fresh(), &mut round), Err(format!("a {ROW} has no region to click")));
        // The line a tile that paints no text.
        let mut ui = tree(&[LINE], ROW_XML);
        ui.inject_text(&MemSource::default(), ROOT, r#"<image name="DM_SpeakerText"><height>40</height></image>"#);
        assert!(own_tiles(&mut ui));
        let line = MenuView { speaker: "Nora".into(), line: Some("Well.".into()), choices: Vec::new(), goodbye: false };
        let now = shown(&ui, line, 1, 0);
        assert_eq!(write(&mut ui, &now, &mut fresh(), &mut round), Err(format!("{LINE} draws nothing")));
    }

    #[test]
    fn what_changes_after_placing_is_checked_as_the_frame_is_drawn() {
        let mut ui = menu();
        let mut written = fresh();
        let now = shown(&ui, choosing(vec![row("A."), row("B.")]), 1, 0);
        frame(&mut ui, &mut written, &now);
        let mut state = DialogTiles { menu: ui.menu(DIALOG_MENU), shown: true, list: Some(now.list.clone()), ..DialogTiles::default() };
        finish_frame(&mut ui, &mut state, &mut written);
        assert!(state.drawn() && !state.hits().is_empty(), "whole");
        // A picture goes, nothing placed anew: the frame's check finds it, all of the view goes.
        let bracket = ui.find_tile(&format!("{TOP_BRACKET}/End")).unwrap();
        ui.set_trait(bracket, "filename", Value::Str(String::new()));
        ui.update();
        finish_frame(&mut ui, &mut state, &mut written);
        assert!(!state.by_tiles(), "the hand-built menu from this frame");
        assert!(rows(&ui).is_empty(), "nothing left behind");
        assert_eq!(num(&ui, ui.screen(), "cropy"), 0.0, "the safe zone given back");
        assert!(written.warned.contains(&format!("{TOP_BRACKET} draws nothing")));
    }

    #[test]
    fn a_target_over_a_row_takes_its_clicks_and_the_view_falls_back() {
        let mut ui = menu();
        ui.inject_text(&MemSource::default(), ROOT, r#"<hotrect name="Cover"><target>1</target><depth>9</depth><x>100</x><y>300</y><width>700</width><height>300</height></hotrect>"#);
        let now = shown(&ui, choosing(vec![row("A."), row("B.")]), 1, 0);
        assert_eq!(write(&mut ui, &now, &mut fresh(), &mut round), Err(format!("a {ROW} is covered by another target")));
        // Off to one side it covers nothing that matters.
        let mut ui = menu();
        ui.inject_text(&MemSource::default(), ROOT, r#"<hotrect name="Cover"><target>1</target><depth>9</depth><x>1000</x><y>0</y><width>50</width><height>50</height></hotrect>"#);
        let now = shown(&ui, choosing(vec![row("A."), row("B.")]), 1, 0);
        assert_eq!(write(&mut ui, &now, &mut fresh(), &mut round), Ok(()));
    }

    #[test]
    fn text_that_does_not_measure_is_not_ready_and_is_tried_again() {
        let mut ui = menu();
        let mut written = fresh();
        let now = shown(&ui, choosing(vec![row("A."), row("B.")]), 1, 0);
        let mut nothing = |ui: &mut Ui, texts: &[TileId], sizes: &mut SizesWritten| menus::write_text_sizes_of(ui, texts, sizes, &|_: f64, _: &str, _: Option<f64>| (0.0, 0.0));
        assert_eq!(write(&mut ui, &now, &mut written, &mut nothing), Err(format!("{ROW_TEXT} has no measure")));
        // As `write_system` cleans up.
        written.fall_back(&mut ui);
        assert!(rows(&ui).is_empty());
        // The font came: next frame is whole.
        assert_eq!(write(&mut ui, &now, &mut written, &mut round), Ok(()));
        assert_eq!(rows(&ui).len(), 2);
    }

    #[test]
    fn scripts_read_what_the_model_says_before_they_run_and_keep_what_they_write() {
        let mut ui = menu();
        let mut written = fresh();
        let line = MenuView { speaker: "Nora".into(), line: Some("Well.".into()), choices: Vec::new(), goodbye: false };
        frame_on(&mut ui, &mut written, line, 1, 0);
        // The first step alone, as before a frame's scripts: the new phase, words and rows are
        // there to be read though nothing is measured or placed yet.
        let view = choosing(vec![row("A."), row("B.")]);
        let list = ListId::for_test(2, &[0x100, 0x101]);
        publish(&mut ui, &view, &list, &mut written).unwrap();
        assert_eq!(f(&ui, DIALOG_MENU, "_ShowingText"), 0.0);
        assert_eq!(rows(&ui).len(), 2);
        assert_eq!(ui.tile(rows(&ui)[1]).get("string").map(Value::text).as_deref(), Some("B."));
        assert_eq!(ui.get_string(&format!("{LINE}/string")).as_deref(), Some(""));
        frame_on(&mut ui, &mut written, view.clone(), 2, 0);
        // A script writes a label, an alpha and a flag.
        let r = rows(&ui)[0];
        ui.set_trait(r, "string", Value::Str("Mine.".into()));
        ui.set_trait(r, "_line_alpha", Value::Num(77.0));
        let menu_tile = ui.menu(DIALOG_MENU).unwrap();
        ui.set_trait(menu_tile, "_ShowSubtitles", Value::Num(0.0));
        // The selection moves, in both steps: none of it is written over.
        publish(&mut ui, &view, &list, &mut written).unwrap();
        frame_on(&mut ui, &mut written, view.clone(), 2, 1);
        assert_eq!(ui.tile(r).get("string").map(Value::text).as_deref(), Some("Mine."));
        assert_eq!(num(&ui, r, "_line_alpha"), 77.0);
        assert_eq!(f(&ui, DIALOG_MENU, "_ShowSubtitles"), 0.0);
        assert_eq!(f(&ui, LIST, "_highlight_y"), 52.0, "the selection itself did move");
        // Until the model changes that very thing.
        let mut changed = view.clone();
        changed.choices[0].label = "Theirs.".into();
        frame_on(&mut ui, &mut written, changed, 2, 1);
        assert_eq!(ui.tile(r).get("string").map(Value::text).as_deref(), Some("Theirs."));
        assert_eq!(num(&ui, r, "_line_alpha"), 77.0, "that alpha still");
        // A new screen writes all of it again.
        frame_on(&mut ui, &mut written, choosing(vec![row("C.")]), 3, 0);
        assert_eq!(num(&ui, rows(&ui)[0], "_line_alpha"), 255.0);
    }

    #[test]
    fn a_transition_publishes_through_the_runtimes_hook_and_a_scripts_writes_stand_after_it() {
        use fnv_game::dialogue::Conversation;
        let mut state = crate::script::ScriptState::over_no_plugins();
        let game = &mut state.rt.as_mut().unwrap().game;
        game.ui_files = Some(Box::new(MemSource::default()));
        game.ui_tree = menu();
        let shared = SharedWritten::default();
        shared.0.lock().unwrap().speaker = Some((fnv_sim::FormId(0x14), "Nora".into()));
        let ears = shared.0.clone();
        game.ui_dialogue_hook = Some(Box::new(move |g| boundary(g, &mut ears.lock().unwrap())));
        // The conversation started, a line up with no text, a transition: the tree has it.
        game.dialogue = Some(Conversation { actor: fnv_sim::FormId(0x14), through: None, line: None, choices: Vec::new(), topic_list: false, lines: 1, then: None, ending: false });
        game.dialogue_transition();
        assert_eq!(f(&game.ui_tree, DIALOG_MENU, "_ShowingText"), 1.0);
        assert_eq!(f(&game.ui_tree, DIALOG_MENU, "_DialogVisible"), 1.0);
        assert_eq!(game.ui_tree.get_string(&format!("{NAME}/string")).as_deref(), Some("Nora"));
        assert!(game.ui_dialogue_hook.is_some(), "kept for the next");
        // A script in that very line writes to the new phase: the same screen's next
        // transition, and the frame's own step, leave it.
        let menu_tile = game.ui_tree.menu(DIALOG_MENU).unwrap();
        game.ui_tree.set_trait(menu_tile, "_ShowingText", Value::Num(7.0));
        game.dialogue_transition();
        assert_eq!(f(&game.ui_tree, DIALOG_MENU, "_ShowingText"), 7.0);
        // A menu over the conversation: hidden, not published.
        game.menu = Some(fnv_game::game::menus::MESSAGE);
        game.dialogue_transition();
        assert_eq!(f(&game.ui_tree, DIALOG_MENU, "_DialogVisible"), 0.0);
    }

    #[test]
    fn the_places_follow_the_evaluated_numbers_they_read() {
        let mut ui = menu();
        let mut written = fresh();
        let labels: Vec<ChoiceRow> = (0..8).map(|i| row(&format!("Choice {i}."))).collect();
        let view = MenuView { goodbye: true, ..choosing(labels) };
        frame_on(&mut ui, &mut written, view.clone(), 1, 5);
        // Rows of 52, three shown (156): the row to 312 shows whole from row 3, at 156.
        assert_eq!(f(&ui, BAR, "_current_value"), 156.0);
        // A settled frame places nothing again.
        frame_on(&mut ui, &mut written, view.clone(), 1, 5);
        assert!(!written.moved);
        // The scroll step, the least height, a row's spacing, the bracket and the band change
        // with nothing else: each moves what reads it.
        let (list, menu_tile) = (ui.find_tile(LIST).unwrap(), ui.menu(DIALOG_MENU).unwrap());
        ui.set_trait(list, "_scroll_delta", Value::Num(100.0));
        frame_on(&mut ui, &mut written, view.clone(), 1, 5);
        assert!(written.moved);
        // A script's delta: the value and the bar's counts in its units, the rows where they were
        // (the last first row 5, at 260).
        assert!((f(&ui, BAR, "_number_of_items") - 3.6).abs() < 1e-9, "{}", f(&ui, BAR, "_number_of_items"));
        assert!((f(&ui, BAR, "_current_value") - 1.56).abs() < 1e-9);
        assert!(describe(&ui, &shown(&ui, view.clone(), 1, 5)).contains(", scroll 3 of 5, "));
        ui.set_trait(menu_tile, "_MinListHeight", Value::Num(400.0));
        frame_on(&mut ui, &mut written, view.clone(), 1, 5);
        assert_eq!(f(&ui, LIST, "height"), 400.0);
        for r in rows(&ui) {
            ui.set_trait(r, "_VerticalSpacing", Value::Num(20.0));
        }
        frame_on(&mut ui, &mut written, view.clone(), 1, 5);
        assert_eq!(ui.tile(rows(&ui)[0]).get("height").map(Value::num), Some(60.0));
        ui.set_trait(ui.find_tile(BOTTOM_BRACKET).unwrap(), "y", Value::Num(700.0));
        frame_on(&mut ui, &mut written, view.clone(), 1, 5);
        assert_eq!(f(&ui, HINT, "y"), 680.0);
        ui.set_trait(ui.find_tile(BAND).unwrap(), "x", Value::Num(500.0));
        frame_on(&mut ui, &mut written, view, 1, 5);
        assert_eq!(f(&ui, HINT, "x"), 500.0 - HINT_GAP - size(0.0, GOODBYE_HINT, None).0);
    }

    #[test]
    fn a_row_texts_computed_height_alone_moves_the_places() {
        // [design] Nothing is measured: only the evaluated height differs, as when an expression
        // of a file works it out.
        let mut ui = menu();
        let mut written = fresh();
        let view = choosing(vec![row("A."), row("B."), row("C.")]);
        let now = shown(&ui, view, 1, 1);
        frame(&mut ui, &mut written, &now);
        assert_eq!(f(&ui, LIST, "_highlight_y"), 52.0);
        let text = child(&ui, rows(&ui)[0], ROW_TEXT).unwrap();
        let mut keep = |ui: &mut Ui, _: &[TileId], _: &mut SizesWritten| {
            ui.set_trait(text, "height", Value::Num(100.0));
            0
        };
        write(&mut ui, &now, &mut written, &mut keep).unwrap();
        assert!(written.moved);
        assert_eq!(f(&ui, LIST, "_highlight_y"), 112.0);
        assert_eq!(f(&ui, LIST, "height"), 216.0);
        write(&mut ui, &now, &mut written, &mut keep).unwrap();
        assert!(!written.moved, "settled");
    }

    #[test]
    fn the_safe_zone_goes_back_as_it_was_on_the_way_out() {
        let crop = |ui: &Ui, n: &str| ui.tile(ui.screen()).get(n).map(Value::num);
        // Absent before: absent after, whichever way out.
        let view = choosing(vec![row("A.")]);
        let mut ui = menu();
        let mut written = fresh();
        frame_on(&mut ui, &mut written, view.clone(), 1, 0);
        assert_eq!(crop(&ui, "cropy"), Some(15.0));
        written.forget(Some(&mut ui));
        assert_eq!((crop(&ui, "cropx"), crop(&ui, "cropy")), (None, None), "the conversation over");
        frame_on(&mut ui, &mut written, view.clone(), 2, 0);
        written.fall_back(&mut ui);
        assert_eq!((crop(&ui, "cropx"), crop(&ui, "cropy")), (None, None), "a fallback");
        // Held through a menu over the conversation.
        frame_on(&mut ui, &mut written, view.clone(), 3, 0);
        hide(&mut ui, &mut written);
        assert_eq!(crop(&ui, "cropy"), Some(15.0));
        written.forget(Some(&mut ui));
        // Set before: that value after.
        let screen = ui.screen();
        ui.set_trait(screen, "cropx", Value::Num(7.0));
        frame_on(&mut ui, &mut written, view.clone(), 4, 0);
        assert_ne!(crop(&ui, "cropx"), Some(7.0));
        written.forget(Some(&mut ui));
        assert_eq!((crop(&ui, "cropx"), crop(&ui, "cropy")), (Some(7.0), None));
        // A script's own value since stands, then and on the way out.
        frame_on(&mut ui, &mut written, view.clone(), 5, 0);
        ui.set_trait(screen, "cropy", Value::Num(3.0));
        frame_on(&mut ui, &mut written, view.clone(), 5, 0);
        assert_eq!(crop(&ui, "cropy"), Some(3.0));
        written.forget(Some(&mut ui));
        assert_eq!(crop(&ui, "cropy"), Some(3.0));
        // A tree built again has its own screen, and the zone is held on it.
        let (mut old, mut again) = (menu(), menu());
        frame_on(&mut old, &mut written, view.clone(), 6, 0);
        frame_on(&mut again, &mut written, view, 6, 0);
        assert_eq!(crop(&again, "cropy"), Some(15.0));
        written.forget(Some(&mut again));
        assert_eq!(crop(&again, "cropy"), None);
    }

    #[test]
    fn a_warning_is_given_once_for_each_reason() {
        let mut written = fresh();
        for _ in 0..3 {
            written.warn(&Tiled::No("a".into()));
            written.warn(&Tiled::Yes);
        }
        assert_eq!(written.warned.len(), 1);
        written.warn(&Tiled::No("b".into()));
        assert_eq!(written.warned.len(), 2);
    }

    #[test]
    fn the_decision_is_one() {
        let none = DialogTiles::default();
        assert!(!none.by_tiles() && !none.drawn());
        let mut ui = menu();
        let menu = ui.menu(DIALOG_MENU);
        let over = DialogTiles { menu, shown: false, ..DialogTiles::default() };
        assert!(over.by_tiles() && !over.drawn(), "a menu over it: neither view draws");
        let up = DialogTiles { menu, shown: true, ..DialogTiles::default() };
        assert!(up.by_tiles() && up.drawn());
        // The picture it stands for, as `super::draw` is given it.
        frame_on(&mut ui, &mut fresh(), choosing(vec![row("A.")]), 1, 0);
        assert_eq!(pictures_drawn(&ui, "m.dds"), 0, "no bar for a list that fits");
        assert!(!pieces(&ui).is_empty());
        assert_eq!(describe(&ui, &shown(&ui, choosing(vec![row("A.")]), 1, 0)), "tiles: DialogMenu drawn from tiles: speaker \"Nora\", 1 choices, selected 0, row heights [52], list y 300 height 70, scroll 0 of 0, bar false, dim 0, frame plain, hint false");
    }
}
