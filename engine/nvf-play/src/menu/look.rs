//! The container and barter menus drawn as the original draws them
//! (docs/spec-notes/barter-menu.md): `menus\container_menu.xml` and
//! `menus\barter_menu.xml` set out, with the game's fonts and pictures read from the
//! owner's archives at run time. Lengths are in the interface's units, 960 to the height
//! of the view, measured from the top left corner of the menus' 1024 × 800 rectangle,
//! which is centred on the screen.
//!
//! - [`layout`] places what moves with the menu's content (the titles' arrows, the caps
//!   arrow, the rows); it is pure and tested without the game's files.
//! - Without the game's files the same layout is drawn with Bevy's font and plain lines.

use super::barter::{self, Flow};
use crate::pipboy::items::{tiles_in, Info};
use super::model::{Category, Command, MenuModel, Mode, Quantity, Row, Side};
use super::{AmountMeter, CategoryTitle, CmdButton, RowButton};
use crate::interface::{amber, at, face, plain_font, texture, Face, Picture, Pictures};
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use bevy::prelude::*;
use crate::ui_pointer::{Button, PointerState};
use fnv_font::{Font, Line};
use fnv_sim::forms::ItemKind;
use nvf_assets::GameData;

// ---- the interface files' numbers [documented: §2] -------------------------------------

/// The menus' rectangle, centred on the screen.
pub const MAIN: Vec2 = Vec2::new(1024.0, 800.0);
/// The two halves: the player's at the left, the container's or merchant's 560 further.
const LEFT_X: f32 = 20.0;
const RIGHT_X: f32 = 580.0;
const TOP: f32 = 20.0;
const HALF_W: f32 = 464.0;
/// The lists: 15 left of their half, 60 down, 365 high; the player's is 462 wide, the
/// other 464.
const LIST_DX: f32 = -15.0;
const LIST_Y: f32 = 60.0;
const LIST_H: f32 = 365.0;
/// The scroll bar at a list's left, 32 wide, with 16-high arrows at its ends; its marker
/// 6 wide, 12 in.
const BAR_W: f32 = 32.0;
const BAR_ARROW: f32 = 16.0;
/// A list scrolls by half a row [documented: `_scroll_delta` 26].
const SCROLL_STEP: f32 = 26.0;
/// In a row: the mark at 15, 15 (25 square), the name at 40, 20 down, the price's right
/// end at 403.
const MARK_AT: f32 = 15.0;
const MARK: f32 = 25.0;
const NAME_X: f32 = 40.0;
const TEXT_Y: f32 = 20.0;
const PRICE_RIGHT: f32 = 403.0;
/// Names wrap at 300 in the barter lists and 370 in the container's.
const WRAP_BARTER: f32 = 300.0;
const WRAP_CONTAINER: f32 = 370.0;
/// Width reserved for a ySI row icon when ySI is active (0 in vanilla).
const YSI_ICON_ADVANCE: f32 = 30.0;
/// The heading: a line at the top with the title 50 in (the barter file 10 above the
/// line, the container file 12.5), the arrows 17.5 × 35 at 30 and after the title, the
/// caps or weight figure right-aligned at 449, 20 down.
const TITLE_X: f32 = 50.0;
const ARROW_X: f32 = 30.0;
const ARROW: Vec2 = Vec2::new(17.5, 35.0);
const FIGURE_RIGHT: f32 = 449.0;
const CORNER: f32 = 40.0;
const FOOTER_Y: f32 = 480.0;
/// The controller's hint under each list, the button that picks it: the files'
/// `*_Items_TitleButton` (LT) 15 in and `*_Container_TitleButton` (RT) 364 in from the
/// half's left, 440 under its top, 85 square, shown only with a controller (`&xbox;`)
/// [documented: container_menu.xml, barter_menu.xml; the hint drawn as text centred in
/// the square in place of the button's picture: design, #1827].
const PICK_HINT_X: [f32; 2] = [15.0, 364.0];
const PICK_HINT_Y: f32 = 440.0;
const PICK_HINT: f32 = 85.0;
/// The bottom bracket, 820 down the screen.
const BRACKET_Y: f32 = 740.0;
/// Accept / Take All and Exit: right-aligned at 1034 less the box's 10, 670 down the
/// screen; each box is its text and 15 high.
const BUTTONS_RIGHT: f32 = 1024.0;
const BUTTONS_Y: f32 = 590.0;
const BUTTON_PAD: f32 = 15.0;
/// The item's picture (256 square) and the item's figures (530 × 120 in tiles a third
/// as wide and high).
const ICON_AT: Vec2 = Vec2::new(42.0, 492.0);
const ICON: f32 = 256.0;
const DATA_Y: f32 = 555.0;
const DATA_X_BARTER: f32 = 287.0;
const DATA_X_CONTAINER: f32 = 327.0;
const TILE: Vec2 = Vec2::new(530.0 / 3.0, 40.0);
/// The caps arrow: its rectangle at 449 (20 more when the player is paid), 180 down;
/// the picture 120 × 360 at 10 (less 25), -15; the figure centred at 32.5 (plus 7.5),
/// 50 down; "Caps" 2.5 right of it, 93.5 down.
const FLOW_X: f32 = LEFT_X + 449.0;
const FLOW_Y: f32 = 180.0;
/// The container's item arrow: 64 × 128 at 474 from the player's half, 170 down (6 more
/// pointing left).
const ITEM_ARROW_X: f32 = LEFT_X + 474.0;
const ITEM_ARROW_Y: f32 = 170.0;
/// The note under the container menu, centred, 800 down the screen and half its box.
const NOTE_Y: f32 = 720.0;
/// Every line is 2 thick [documented: menus\globals.xml, on a PC].
const THICK: f32 = 2.0;
/// The highlight box: a line at brightness 200 and a fill of 40 at half brightness.
const HIGHLIGHT_LINE: f32 = 200.0;
const HIGHLIGHT_FILL: f32 = 40.0;
/// A button the player cannot use is drawn at 128 of 255.
const DIM: f32 = 128.0 / 255.0;

// ---- measured on the owner's screenshots [observed, §3] ----------------------------------

/// A row is the body font's 31 and 20 [observed: rows 51 apart at 1024 lines; the
/// message box's button rows, message-menu.md §3].
pub const ROW_H: f32 = 51.0;
/// How far under a text's top its letters stand: the body font and the title font
/// (message-menu.md §3).
const BODY_BASE: f32 = 15.0;
const TITLE_BASE: f32 = 21.5;
/// Lines of a text are 35 apart (message-menu.md §3).
const LINE_PITCH: f32 = 35.0;
/// The key hint stands this far right of its button's text ("Accept A)").
const HINT_GAP: f32 = 16.0;
/// How dark the original's picture behind the menu makes the world: about a third at its edge on
/// the barter screenshot (the world behind is also blurred there; not built).
/// Ours is as dark as a message box's, 0.93 black [design: the owner's wish after the
/// playtest of 2026-10-17, the other menus' 0.75 (`crate::interface::MENU_BACKING`) still
/// let the world show through too much].
const SHADE: f32 = 0.93;

const BODY_FONT: &str = "textures\\fonts\\monofonto_large.fnt";
const TITLE_FONT: &str = "textures\\fonts\\glow_monofonto_vl_dialogs.fnt";
const TICK_TEXTURE: &str = "textures\\interface\\hud\\hud_tick_mark.dds";

/// What the menus are drawn with, read when the first one opens.
pub struct Kit {
    body: Face,
    title: Face,
    pictures: Pictures,
    /// The meters' tick mark (`hud_tick_mark.dds`, not in the atlas).
    tick: Option<Handle<Image>>,
    text_size:f32,
}

impl Kit {
    pub(crate) fn set_text_size(&mut self,size:f32) {self.text_size=size;}

    pub fn load(data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Kit {
        Kit {
            body: face(BODY_FONT, plain_font(31.0, 12.0), data, images, bc),
            title: face(TITLE_FONT, plain_font(34.0, 14.0), data, images, bc),
            pictures: Pictures::load(data),
            text_size:1.0,
            tick: data.and_then(|d| texture(TICK_TEXTURE, d, images, bc)).map(|t| t.0),
        }
    }
}

/// Where a text's letters start for a given anchor.
#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
    Centre,
}

fn first_line(font: &Font, s: &str) -> Line {
    font.lines(s, None).into_iter().next().unwrap_or_default()
}

/// The title of a half: the player's is the category's; the other half's is the
/// container's or merchant's name while it shows everything [hypothesis: its title for
/// other categories is not observed].
pub fn half_title(m: &MenuModel, side: Side) -> String {
    let c = m.category[side as usize];
    // The player's tweak: "Weapons", not "WEAPONS" [design: docs/design/tweaks.md].
    let title = |t: &str| if m.title_case_categories { title_case(t) } else { t.to_string() };
    match side {
        Side::Player => title(c.title()),
        Side::Container if c == Category::All => m.title.clone(),
        Side::Container => title(c.title()),
    }
}

/// Whether taking the container's selected row would put the player over the carry
/// limit (the Weight Warning tweak; only the player's half of a container).
fn over_if_taken(m: &MenuModel, side: Side) -> bool {
    if !m.warn_encumbrance || side != Side::Player || !matches!(m.mode, Mode::Container(_)) {
        return false;
    }
    let Ok(e) = m.encumbrance.as_ref() else { return false };
    let row = m.list(Side::Container).rows.get(m.selected[Side::Container as usize]);
    row.and_then(|r| r.weight.map(|w| w * r.count.max(1) as f32)).is_some_and(|w| e.weight + f64::from(w) > e.capacity)
}

/// `WEAPONS` as `Weapons`: each word's first letter kept, the rest lower case.
fn title_case(t: &str) -> String {
    t.split(' ').map(|w| {
        let mut c = w.chars();
        c.next().map_or_else(String::new, |f| f.to_string() + &c.as_str().to_lowercase())
    }).collect::<Vec<_>>().join(" ")
}

/// A row's text: the name, and the count when there is more than one [observed].
pub fn row_text(r: &Row) -> String {
    if r.count > 1 {
        format!("{} ({})", r.name, r.count)
    } else {
        r.name.clone()
    }
}

/// The caps' word of a half's figure ([`half_figure`]).
pub fn caps_word() -> &'static str {
    crate::pipboy::labels::built_in_label("sInventoryCaps")
}

/// The weight's word of a half's figure ([`half_figure`]).
pub fn weight_word() -> &'static str {
    crate::pipboy::labels::built_in_label("sInventoryWeight")
}

/// The figure at the top right of a half: the caps on either side of a barter, the
/// player's weight in a container's. The words are the settings the menu files' headline
/// cards show (`sInventoryCaps`, `sInventoryWeight`): the load order's text, else
/// NVForever's own label [design, #4495: in place of the words read from the original's
/// screen].
pub fn half_figure(m: &MenuModel, side: Side) -> Option<String> {
    match (m.mode, side, &m.trade) {
        // The player's tweak: the caps after the trade beside those now (our words) [design: docs/design/tweaks.md].
        (Mode::Barter(_), _, Some(t)) if m.barter_caps_change && t.balance != 0 => {
            let now = i64::from(t.purse[side as usize]);
            let after = if side == Side::Player { now + t.paid() } else { now - t.paid() };
            Some(format!("{}  {now} (to {after})", caps_word()))
        }
        (Mode::Barter(_), _, Some(t)) => Some(format!("{}  {}", caps_word(), t.purse[side as usize])),
        (Mode::Container(_), Side::Player, _) => {
            m.encumbrance.as_ref().ok().map(|e| format!("{}  {:.0}/{:.0}", weight_word(), e.weight, e.capacity))
        }
        _ => None,
    }
}

/// Where the parts of a heading go, from its half's left end: the left arrow, the title,
/// the right arrow, and the top line after it (start, length).
pub fn heading(title_width: f32) -> (f32, f32, f32, (f32, f32)) {
    let right_arrow = TITLE_X + 10.0 + title_width;
    let line = right_arrow + 20.0;
    (ARROW_X, TITLE_X, right_arrow, (line, (HALF_W - line).max(0.0)))
}

/// The heading's part that takes the wheel for the category, from its half's left end
/// (start, width): from the left arrow to the right arrow's end [design: #1192].
pub fn title_zone(title_width: f32) -> (f32, f32) {
    let (left, _, right, _) = heading(title_width);
    (left, right + ARROW.x - left)
}

/// Where the caps figure is centred, from the menu's left: the file's "somewhat centred"
/// rule, which moves a figure wider than 70 by its excess (left when the player pays,
/// right when paid).
pub fn flow_figure_x(flow: Flow, figure_width: f32) -> f32 {
    let sell = flow == Flow::Sell;
    let rect = FLOW_X + if sell { 20.0 } else { 0.0 };
    let excess = if figure_width > 70.0 { figure_width - 70.0 } else { 0.0 };
    rect + 32.5 + if sell { 7.5 } else { 0.0 } - excess * if sell { -1.0 } else { 1.0 }
}

/// A row's height for a name of `lines` lines: the text and 20 [documented: the template's
/// `_VerticalSpacing`], a text of n lines being 31 + 35 (n − 1) high (message-menu.md §3).
pub fn row_height(lines: usize) -> f32 {
    ROW_H + LINE_PITCH * lines.saturating_sub(1) as f32
}

/// The first row to draw: the list's own first row, or a later one when rows of two
/// lines above the selection would push it out of the list.
pub fn first_shown(heights: &[f32], first: usize, selected: usize) -> usize {
    let mut first = first.min(heights.len());
    while first < selected && heights[first..=selected.min(heights.len().saturating_sub(1))].iter().sum::<f32>() > LIST_H {
        first += 1;
    }
    first
}

/// The lines of each row of a list as drawn, and the rows' heights.
fn row_lines(m: &MenuModel, side: Side, kit: &Kit) -> (Vec<Vec<Line>>, Vec<f32>) {
    let rows = &m.list(side).rows;
    let icon_advance = if rows.iter().any(|r| r.ysi.is_some()) { YSI_ICON_ADVANCE } else { 0.0 };
    let wrap = (if matches!(m.mode, Mode::Barter(_)) { WRAP_BARTER } else { WRAP_CONTAINER }) - icon_advance;
    let texts: Vec<Vec<Line>> = rows.iter().map(|r| kit.body.font.lines(&row_text(r), Some(wrap/kit.text_size))).collect();
    let heights = texts.iter().map(|t| (row_height(t.len())*kit.text_size).max(ROW_H)).collect();
    (texts, heights)
}

/// The first row drawn from the list's own first row: a selection the pointer made stays
/// where it is (only the wheel and the keys scroll; owner's playtest list 7, item 11), any
/// other is brought into the list ([`first_shown`]).
fn shown_from(m: &MenuModel, side: Side, heights: &[f32], first: usize) -> usize {
    if m.pointer_pick {
        first.min(heights.len())
    } else {
        first_shown(heights, first, m.selected[side as usize])
    }
}

/// The first row each list will draw, for the menu to keep as its scroll position (so a
/// row the pointer picks later does not move the list back to where the model last
/// scrolled it).
pub fn drawn_first(m: &MenuModel, side: Side, kit: &Kit) -> usize {
    let (_, heights) = row_lines(m, side, kit);
    shown_from(m, side, &heights, m.scroll[side as usize])
}

/// The scroll bar's marker: its top and height inside the bar's track, or `None` when the
/// list fits. Counted in steps of half a row [hypothesis: the steps not shown are the
/// marker's positions].
pub fn marker(rows: usize, first: usize) -> Option<(f32, f32)> {
    let track = LIST_H - 2.0 * BAR_ARROW;
    let shown = (LIST_H / SCROLL_STEP).floor();
    let total = (rows as f32 * ROW_H / SCROLL_STEP).ceil();
    if total <= shown {
        return None;
    }
    let positions = total - shown + 1.0;
    let h = (track / positions).max(32.0).min(track);
    let at = (first as f32 * ROW_H / SCROLL_STEP).min(positions - 1.0);
    Some((at / (positions - 1.0) * (track - h), h))
}

// ---- drawing -------------------------------------------------------------------------------

/// A list of whole rows' scroll bar marker: its top and height inside a track `track`
/// high, or `None` when the `rows` fit `page` at once; one position per first row.
pub fn rows_marker(rows: usize, first: usize, page: usize, track: f32) -> Option<(f32, f32)> {
    if rows <= page || page == 0 {
        return None;
    }
    let positions = (rows - page + 1) as f32;
    let h = (track / positions).max(32.0).min(track);
    let at = (first.min(rows - page) as f32) / (positions - 1.0);
    Some((at * (track - h), h))
}

/// The container menu's scroll bar (arrows at the ends, the marker between) for another
/// menu's list of whole rows: `BAR_W` wide at `x`, `y`, `height` high. Nothing when the
/// list fits.
#[allow(clippy::too_many_arguments)]
pub fn spawn_rows_bar(p: &mut ChildSpawnerCommands, kit: &mut Kit, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool, glyphs: &crate::pad::PadGlyphs, (x, y, height): (f32, f32, f32), rows: usize, first: usize, page: usize) {
    let Some((my, mh)) = rows_marker(rows, first, page, height - 2.0 * BAR_ARROW) else { return };
    let mut c = Ctx { kit, data, images, bc, glyphs };
    let bright = amber(255.0);
    c.image(p, "arrow_up.dds", at(x, y, BAR_W, BAR_ARROW), bright);
    c.image(p, "arrow_down.dds", at(x, y + height - BAR_ARROW, BAR_W, BAR_ARROW), bright);
    if c.image(p, "vert_marker.dds", at(x + 12.0, y + BAR_ARROW + my, 6.0, mh), bright).is_none() {
        p.spawn((at(x + 12.0, y + BAR_ARROW + my, 6.0, mh), BackgroundColor(bright)));
    }
}

/// The scroll bar's width.
pub const ROWS_BAR_W: f32 = BAR_W;

struct Ctx<'a> {
    kit: &'a mut Kit,
    data: Option<&'a GameData>,
    images: &'a mut Assets<Image>,
    bc: bool,
    glyphs: &'a crate::pad::PadGlyphs,
}

impl Ctx<'_> {
    fn picture(&mut self, path: &str) -> Option<Picture> {
        self.kit.pictures.picture(path, self.data, self.images, self.bc)
    }

    /// A picture tinted the interface's colour (at `alpha`), stretched over `node`.
    fn image(&mut self, p: &mut ChildSpawnerCommands, path: &str, node: Node, color: Color) -> Option<Entity> {
        self.image_with(p, path, node, color, ())
    }

    /// The same, with more components on the picture (a button).
    fn image_with(&mut self, p: &mut ChildSpawnerCommands, path: &str, node: Node, color: Color, extra: impl Bundle) -> Option<Entity> {
        let (image, rect) = self.picture(path)?;
        Some(p.spawn((node, ImageNode { rect, color, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) }, extra)).id())
    }

    /// A straight line, from the atlas's solid piece (or a plain fill).
    fn line(&mut self, p: &mut ChildSpawnerCommands, x: f32, y: f32, w: f32, h: f32, color: Color) {
        if self.image(p, "solid.dds", at(x, y, w, h), color).is_none() {
            p.spawn((at(x, y, w, h), BackgroundColor(color)));
        }
    }

    /// A line 40 long that fades out downwards (or upwards, `up`), standing at `y`.
    fn fade(&mut self, p: &mut ChildSpawnerCommands, x: f32, y: f32, up: bool, color: Color) {
        let name = if up { "fade_to_top.dds" } else { "fade_to_bottom.dds" };
        self.image(p, name, at(x, y, THICK, CORNER), color);
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, p: &mut ChildSpawnerCommands, title: bool, s: &str, x: f32, top: f32, align: Align, color: Color) -> f32 {
        let face = if title { &self.kit.title } else { &self.kit.body };
        let scale=self.kit.text_size;
        let width = crate::pad::hint_width(face, s, self.glyphs)*scale;
        let x = match align {
            Align::Left => x,
            Align::Right => x - width,
            Align::Centre => x - width / 2.0,
        };
        let base = top + if title { TITLE_BASE } else { BODY_BASE }*scale;
        let _units=crate::interface::units_at(crate::interface::unit_scale()*scale);
        crate::pad::set_hint(p, face, s, x/scale, base/scale, color, self.glyphs);
        width
    }
}

/// Spawn the menu under `root`. `note`: the line under the menu.
#[allow(clippy::too_many_arguments)]
pub fn spawn(root: &mut ChildSpawnerCommands, m: &MenuModel, note: Option<&str>, info: Option<&Info>, kit: &mut Kit, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool, glyphs: &crate::pad::PadGlyphs) {
    let mut c = Ctx { kit, data, images, bc, glyphs };
    let bright = amber(255.0);
    let main = Node {
        width: crate::interface::u(MAIN.x),
        height: crate::interface::u(MAIN.y),
        ..default()
    };
    root.spawn(main).with_children(|p| {
        // [documented] The dark picture behind, 40 left of and 105 above the menu.
        let behind = at(-40.0, -105.0, 1144.0, 970.0);
        let shade = Color::srgba(0.0, 0.0, 0.0, SHADE);
        if c.image(p, "interface\\shared\\background\\solid_black.dds", behind.clone(), shade).is_none() {
            p.spawn((behind, BackgroundColor(shade)));
        }
        for side in [Side::Player, Side::Container] {
            half(p, &mut c, m, side);
        }
        // The bottom bracket: a line with a short line fading up at each end.
        c.line(p, LEFT_X, BRACKET_Y, MAIN.x, THICK, bright);
        for x in [LEFT_X, LEFT_X + MAIN.x - THICK] {
            c.fade(p, x, BRACKET_Y - CORNER + 1.0, true, bright);
        }
        match (m.mode, &m.trade) {
            (Mode::Barter(_), Some(t)) => {
                if let Some(flow) = barter::flow(t.balance) {
                    // The player's tweak: the whole total, past the merchant's caps [design: docs/design/tweaks.md].
                    let shown = if m.full_barter_total { t.balance } else { t.paid() };
                    caps_flow(p, &mut c, flow, shown.unsigned_abs(), t.affordable());
                }
            }
            (Mode::Container(_), _) if m.selected_row(m.focus).is_some() => {
                // [documented: the arrow points the way the item would go]
                let (name, dy) = if m.focus == Side::Player { ("right.dds", 0.0) } else { ("left.dds", 6.0) };
                c.image(p, &format!("interface\\shared\\arrow\\{name}"), at(ITEM_ARROW_X, ITEM_ARROW_Y + dy, 64.0, 128.0), bright);
            }
            _ => {}
        }
        buttons(p, &mut c, m);
        if let Some(r) = m.selected_row(m.focus) {
            item_data(p, &mut c, m, r, info);
        }
        if let Some(n) = note.filter(|n| !n.is_empty()) {
            c.text(p, false, n, MAIN.x / 2.0, NOTE_Y + (31.0 + 20.0) / 2.0 + 20.0, Align::Centre, bright);
        }
    });
}

/// [design: barter-menu.md section 9] Keep the model's own refusal note over the tiled
/// barter view, whose file has no subtitle tile, at the existing hand-built place.
pub fn spawn_note(root: &mut ChildSpawnerCommands, note: &str, kit: &mut Kit, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool, glyphs: &crate::pad::PadGlyphs) {
    let mut c = Ctx { kit, data, images, bc, glyphs };
    root.spawn(Node { width: crate::interface::u(MAIN.x), height: crate::interface::u(MAIN.y), ..default() }).with_children(|p| {
        c.text(p, false, note, MAIN.x / 2.0, NOTE_Y + (31.0 + 20.0) / 2.0 + 20.0, Align::Centre, amber(255.0));
    });
}

fn half(p: &mut ChildSpawnerCommands, c: &mut Ctx, m: &MenuModel, side: Side) {
    let bright = amber(255.0);
    let x0 = if side == Side::Player { LEFT_X } else { RIGHT_X };
    let title_top = TOP + if matches!(m.mode, Mode::Barter(_)) { -10.0 } else { -12.5 };
    let title = half_title(m, side);
    let title_w = first_line(&c.kit.title.font, &title).width*c.kit.text_size;
    let (left_arrow, title_x, right_arrow, (line_x, line_len)) = heading(title_w);
    // The heading.
    c.fade(p, x0, TOP, false, bright);
    c.line(p, x0, TOP, 25.0, THICK, bright);
    c.text(p, true, &title, x0 + title_x, title_top, Align::Left, bright);
    // Over the arrows and the title, where the wheel and horizontal touch swipe step the category.
    // Spawned before the arrows so arrows sit above it in UiStack for direct taps.
    let (zone_x, zone_w) = title_zone(title_w);
    p.spawn((at(x0 + zone_x, TOP - 20.0, zone_w, 56.0), FocusPolicy::Pass, PointerState::default(), CategoryTitle(side)));
    for (x, name, by) in [(left_arrow, "main_arrow_left.dds", -1), (right_arrow, "main_arrow_right.dds", 1)] {
        // Generous 52x52 hit container centered on the exact 17.5x35 arrow artwork.
        let hit_w = 52.0;
        let hit_h = 52.0;
        let dx = (hit_w - ARROW.x) * 0.5;
        let dy = (hit_h - ARROW.y) * 0.5;
        p.spawn((at(x0 + x - dx, TOP - 10.0 - dy, hit_w, hit_h), Button, CmdButton(Command::Category(side, by)))).with_children(|btn| {
            c.image(btn, &format!("interface\\main\\{name}"), at(dx, dy, ARROW.x, ARROW.y), bright);
        });
    }
    c.line(p, x0 + line_x, TOP, line_len, THICK, bright);
    if let Some(f) = half_figure(m, side) {
        // The player's tweak: the weight in red when taking the container's selected row
        // would put the player over [design: docs/design/tweaks.md].
        let colour = if over_if_taken(m, side) { crate::reticle::crime_colour() } else { bright };
        c.text(p, false, &f, x0 + FIGURE_RIGHT, TOP + 20.0, Align::Right, colour);
    }
    c.fade(p, x0 + HALF_W - THICK, TOP, false, bright);
    // The footer.
    c.fade(p, x0, TOP + FOOTER_Y - CORNER, true, bright);
    c.line(p, x0, TOP + FOOTER_Y, HALF_W, THICK, bright);
    c.fade(p, x0 + HALF_W - THICK, TOP + FOOTER_Y - CORNER, true, bright);
    if let Some(hint) = pick_hint(m.mode, side, crate::pad::labels()) {
        let x = x0 + PICK_HINT_X[side as usize] + PICK_HINT / 2.0;
        c.text(p, false, &hint, x, TOP + PICK_HINT_Y + PICK_HINT / 2.0 - BODY_BASE, Align::Centre, bright);
    }
    list(p, c, m, side, x0);
}

/// The hint under a half with a controller in use (labels `l`): the button that picks
/// that list, "LT)" under the player's and "RT)" under the other ([`PICK_HINT_X`]).
/// None with the keyboard, as the original shows none.
pub fn pick_hint(mode: Mode, side: Side, l: crate::pad::Labels) -> Option<String> {
    let at = match mode {
        Mode::Barter(_) => crate::pad::at_menu(fnv_game::game::menus::BARTER),
        Mode::Container(_) => crate::pad::at_menu(crate::script::MENU_CONTAINER),
        Mode::Inventory => return None,
    };
    if l == crate::pad::Labels::Keys {
        return None;
    }
    let key = if side == Side::Player { KeyCode::ArrowLeft } else { KeyCode::ArrowRight };
    Some(crate::pad::hint_as(&[key], at, l))
}

fn list(p: &mut ChildSpawnerCommands, c: &mut Ctx, m: &MenuModel, side: Side, x0: f32) {
    let bright = amber(255.0);
    let width = if side == Side::Player { 462.0 } else { 464.0 };
    let rows = &m.list(side).rows;
    let first = m.scroll[side as usize];
    let barter = matches!(m.mode, Mode::Barter(_));
    let node = Node { overflow: Overflow::clip(), ..at(x0 + LIST_DX, TOP + LIST_Y, width, LIST_H) };
    p.spawn(node).with_children(|l| {
        if let Some((y, h)) = marker(rows.len(), first) {
            c.image(l, "arrow_up.dds", at(0.0, 0.0, BAR_W, BAR_ARROW), bright);
            c.image(l, "arrow_down.dds", at(0.0, LIST_H - BAR_ARROW, BAR_W, BAR_ARROW), bright);
            if c.image(l, "vert_marker.dds", at(12.0, BAR_ARROW + y, 6.0, h), bright).is_none() {
                l.spawn((at(12.0, BAR_ARROW + y, 6.0, h), BackgroundColor(bright)));
            }
        }
        let row_w = width - BAR_W - 5.0;
        let name_x = NAME_X + if rows.iter().any(|r| r.ysi.is_some()) { YSI_ICON_ADVANCE } else { 0.0 };
        let (texts, heights) = row_lines(m, side, c.kit);
        let first = shown_from(m, side, &heights, first);
        let mut y = 0.0;
        for i in first..rows.len() {
            if y >= LIST_H {
                break;
            }
            let r = &rows[i];
            let row_h = heights[i];
            let row_y = y;
            y += row_h;
            let mut entity=l.spawn((Button, RowButton(side, i), at(BAR_W, row_y, row_w, row_h)));
            if let Some(identity)=super::normal_touch::RenderedRow::of(m,side,i) {entity.insert(identity);}
            entity.with_children(|row| {
                // [documented] The highlight box of the list that has the focus, one
                // unit short of the row.
                if m.focus == side && m.selected[side as usize] == i {
                    let h = row_h - 1.0;
                    let fill = amber(128.0).with_alpha((HIGHLIGHT_FILL / 255.0).powf(2.2));
                    row.spawn((at(0.0, 0.0, row_w, h), BackgroundColor(fill)));
                    let line = amber(HIGHLIGHT_LINE);
                    for yy in [0.0, h - THICK] {
                        c.line(row, 0.0, yy, row_w, THICK, line);
                    }
                    for xx in [0.0, row_w - THICK] {
                        c.line(row, xx, THICK, THICK, h - 2.0 * THICK, line);
                    }
                }
                // [documented] A filled square for an equipped item, an open one for an
                // item put up for the trade.
                let mark = if r.bartered { Some("square.dds") } else if r.equipped || r.loaded { Some("square_filled.dds") } else { None };
                if let Some(mark) = mark {
                    c.image(row, mark, at(MARK_AT, MARK_AT, MARK, MARK), bright);
                }
                if let Some(icon) = r.ysi.as_ref().and_then(|d| d.icon.as_deref()) {
                    c.image(row, icon, at(NAME_X, MARK_AT, 24.0, 24.0), bright);
                }
                for (k, line) in texts[i].iter().enumerate() {
                    super::readability::line(row,&c.kit.body,line,
                        Vec2::new(name_x,(TEXT_Y+BODY_BASE+LINE_PITCH*k as f32)*c.kit.text_size),c.kit.text_size,bright);
                }
                if barter {
                    let price = r.price.map_or("--".to_string(), barter::price_text);
                    c.text(row, false, &price, PRICE_RIGHT, TEXT_Y*c.kit.text_size, Align::Right, bright);
                }
            });
        }
    });
}

/// The arrow and the caps figure between the lists, dimmed when the side that pays has
/// too few caps [design: the owner's request; Accept is refused then].
fn caps_flow(p: &mut ChildSpawnerCommands, c: &mut Ctx, flow: Flow, caps: u64, affordable: bool) {
    let bright = amber(255.0).with_alpha(if affordable { 1.0 } else { DIM });
    let sell = flow == Flow::Sell;
    let rect_x = FLOW_X + if sell { 20.0 } else { 0.0 };
    let name = if sell { "barter_arrow_sell.dds" } else { "barter_arrow_buy.dds" };
    c.image(p, &format!("interface\\barter\\{name}"), at(rect_x + 10.0 - if sell { 25.0 } else { 0.0 }, FLOW_Y - 15.0, 120.0, 360.0), bright);
    let figure = caps.to_string();
    let w = first_line(&c.kit.title.font, &figure).width;
    let x = flow_figure_x(flow, w);
    c.text(p, true, &figure, x, FLOW_Y + 50.0, Align::Centre, bright);
    c.text(p, false, "Caps", x + 2.5, FLOW_Y + 93.5, Align::Centre, bright);
}

fn buttons(p: &mut ChildSpawnerCommands, c: &mut Ctx, m: &MenuModel) {
    use crate::pad::{at_menu, hint};
    use KeyCode::*;
    let (barter, container) = (at_menu(fnv_game::game::menus::BARTER), at_menu(crate::script::MENU_CONTAINER));
    let list: Vec<(&str, String, Command, bool)> = match (m.mode, &m.trade) {
        (Mode::Barter(_), Some(t)) => vec![("Accept", hint(&[KeyA], barter), Command::Accept, !t.is_empty()), ("Exit", hint(&[KeyX, Escape], barter), Command::Close, true)],
        _ => vec![("Take All", hint(&[KeyR], container), Command::TakeAll, !m.list(Side::Container).rows.is_empty()), ("Exit", hint(&[KeyE, Escape], container), Command::Close, true)],
    };
    let h = (31.0 + BUTTON_PAD)*c.kit.text_size;
    for (i, (label, hint, cmd, on)) in list.into_iter().enumerate() {
        let color = amber(255.0).with_alpha(if on { 1.0 } else { DIM });
        let y = BUTTONS_Y + i as f32*h - 2.0*(h-(31.0+BUTTON_PAD));
        let w = c.text(p, false, label, BUTTONS_RIGHT, y + BUTTON_PAD, Align::Right, color);
        c.text(p, false, &hint, BUTTONS_RIGHT + HINT_GAP, y + BUTTON_PAD, Align::Left, color);
        p.spawn((Button, CmdButton(cmd), at(BUTTONS_RIGHT - w - 10.0, y, w + 20.0 + HINT_GAP + 30.0, h)));
    }
}

/// The selected item's picture and figures: weight and value, and the condition of a
/// weapon or armour as a bar [documented: the tiles of `item_stats_display.xml`; the
/// damage, resistance, strength, ammunition, effects and mods tiles are not built].
/// The selected item's picture and figures: the Pip-Boy's ITEMS page's ([`Info`],
/// `crate::pipboy::items::tiles_in`: damage or DT, DPS, STR, condition, ammunition or
/// armour class, effects) in this menu's figures' rectangle, with the weight and value of
/// one unit, its value at its condition and before the trade's multiplier [owner's
/// playtest list 7 item 25; documented: the same `item_stats_display.xml`, §2].
fn item_data(p: &mut ChildSpawnerCommands, c: &mut Ctx, m: &MenuModel, r: &Row, info: Option<&Info>) {
    let bright = amber(255.0);
    if let Some(icon) = r.icon.as_deref() {
        c.image(p, icon, at(ICON_AT.x, ICON_AT.y, ICON, ICON), bright);
    }
    let x0 = if matches!(m.mode, Mode::Barter(_)) { DATA_X_BARTER } else { DATA_X_CONTAINER };
    let i = item_figures(m, r, info);
    let tiles = tiles_in(&i, Vec2::new(3.0 * TILE.x, 3.0 * TILE.y));
    let shown: Vec<String> = tiles.iter().map(|t| if t.value.is_empty() { t.title.clone() } else { format!("{} {}", t.title, t.value) }).collect();
    info!("menu: figures of {}: {}", r.name, shown.join(", "));
    for t in tiles {
        let (x, y, w) = (x0 + t.rect.min.x, DATA_Y + t.rect.min.y, t.rect.width());
        card(p, c, x, y, w, &t.title, &t.value);
        if let Some(fill) = t.meter {
            // [documented: meter.xml placed 62 in, 15 down, 20 high, 72 short of the tile]
            let (mx, my, mw) = (x + 62.0, y + 15.0, w - 72.0);
            p.spawn((at(mx, my, mw, 20.0), BackgroundColor(amber(255.0).with_alpha(0.15))));
            p.spawn((at(mx, my, mw * fill, 20.0), BackgroundColor(bright)));
        }
    }
}

/// The figures of the selected row in these menus: the Pip-Boy's ([`Info`]), with the weight
/// and value of one unit, the value at its condition and before the trade's multiplier
/// [owner's playtest list 7 item 25]. The hand-built drawing and the tiles
/// (`crate::tiles::item_card`) both read these.
pub fn item_figures(m: &MenuModel, r: &Row, info: Option<&Info>) -> Info {
    let prices = m.trade.as_ref().map(|t| t.prices).unwrap_or_default();
    let value = r.worth(&prices).map(|v| v.round() as i64);
    let condition = r.condition.filter(|_| matches!(r.kind, Some(ItemKind::Weapon | ItemKind::Armor)));
    let mut i = info.cloned().unwrap_or_else(|| Info { condition, ..Info::default() });
    i.weight = r.weight.filter(|w| *w > 0.0).map(|w| format!("{w:.2}"));
    i.value = value.filter(|v| *v > 0).map(|v| v.to_string());
    i
}

/// A tile of figures `w` wide: a line along its top and one fading down its right side,
/// the title at the left and the value at the right.
fn card(p: &mut ChildSpawnerCommands, c: &mut Ctx, x: f32, y: f32, w: f32, title: &str, value: &str) {
    let bright = amber(255.0);
    c.line(p, x, y, w, THICK, bright);
    c.fade(p, x + w - THICK, y, false, bright);
    c.text(p, false, title, x + 5.0, y + 20.0, Align::Left, bright);
    // A value too long for the room beside the title goes on in lines under it, as on the
    // Pip-Boy's cards (#2537).
    let pitch = c.kit.body.font.size;
    for (i, l) in crate::pipboy::frame::card_value_lines(&c.kit.body.font, title, w, value).iter().enumerate() {
        c.text(p, false, l, x + w - 10.0, y + 20.0 + pitch * i as f32, Align::Right, bright);
    }
}

// ---- the "How many?" prompt -----------------------------------------------------------
//
// `menus\quantity_menu.xml` [documented; docs/spec-notes/barter-menu.md §7]: a 740 × 370
// box centred on the screen; inside it, 22 in and 42 down, a 690-wide frame of a top
// bracket and a bottom bracket 290 lower (lines with 50-long fading ends); "How many?" in
// font 2 centred at 345, 50 down; the meter 149.5 in, 115 down, 391 × 24, with its
// arrows (17.5 × 35) at 130 and 549; the amount in font 6 centred at 345, 170 down; Ok
// and Cancel right-aligned at 665, 155 and 200 down. The meter is the interface's meter
// (`meter.xml`): a ground at a quarter of the colour and over it tick marks as far as the
// amount is a part of the stack. The owner's screenshots agree [observed: the frame 775 ×
// 327 pixels at 1080 lines, 690 × 291 units; ticks 8 units apart].

const Q_BOX: Vec2 = Vec2::new(740.0, 370.0);
const Q_IN: Vec2 = Vec2::new(22.0, 42.0);
const Q_W: f32 = 690.0;
const Q_BOTTOM: f32 = 290.0;
const Q_FADE: f32 = 50.0;
const Q_CENTRE: f32 = 345.0;
const Q_PROMPT_Y: f32 = 50.0;
const Q_METER: Rect = Rect { min: Vec2::new(149.5, 115.0), max: Vec2::new(540.5, 139.0) };
const Q_ARROWS: [f32; 2] = [130.0, 549.0];
const Q_AMOUNT_Y: f32 = 170.0;
/// The tweaks' weight or price line, under the amount [design: docs/design/tweaks.md].
const Q_EXTRA_Y: f32 = 215.0;

/// The line the quantity tweaks add under the amount: in a barter what `amount` units
/// fetch, in a container what they weigh (our words) [design: docs/design/tweaks.md].
pub fn quantity_line(price: Option<f32>, weight: Option<f32>, amount: i32) -> Option<String> {
    match (price, weight) {
        (Some(p), _) => Some(format!("Price: {:.0} caps", p * amount as f32)),
        (None, Some(w)) => Some(format!("Weight: {}", format!("{:.1}", w * amount as f32).trim_end_matches(".0"))),
        _ => None,
    }
}
const Q_BUTTONS_RIGHT: f32 = 665.0;
const Q_BUTTONS_Y: [f32; 2] = [155.0, 200.0];
/// A tick of the meter: 8 apart [observed], the picture's top 8 × 20 texels stretched to
/// the meter's height (as the HUD's bars, `crate::hud`).
const Q_TICK: f32 = 8.0;

/// How many ticks of the meter show for a part of the stack (a tick partly in shows,
/// cut).
pub fn quantity_ticks(part: f32) -> usize {
    let w = Q_METER.width() * part.clamp(0.0, 1.0);
    (w / Q_TICK).ceil() as usize
}

/// Spawn the prompt under `root` (a node centring its children on the screen). A sheet
/// behind it takes the pointer, so nothing under the prompt does.
/// The settings naming the "How many?" prompt's two buttons (quantity_menu.xml's sOK and
/// sCancel), read through `fnv_game::ui_loading::game_string`: the load order's GMST,
/// else NVForever's own label (`fnv_ui::strings::BUILT_IN`; LEGAL.md) [design, #1183].
pub const QUANTITY_BUTTONS: [&str; 2] = ["sOK", "sCancel"];

/// The prompt's button words from the running game's strings, or NVForever's labels
/// alone without one.
pub fn quantity_words(game: Option<&fnv_game::Game<'_>>) -> [String; 2] {
    QUANTITY_BUTTONS.map(|name| match game {
        Some(g) => fnv_game::ui_loading::game_string(g, name),
        None => fnv_ui::strings::built_in(name).unwrap_or_default().to_owned(),
    })
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_quantity(root: &mut ChildSpawnerCommands, q: &Quantity, words: &[String; 2], kit: &mut Kit, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool, glyphs: &crate::pad::PadGlyphs, extra: Option<&str>) {
    spawn_quantity_reusing(root,q,words,(kit,data,images,bc,glyphs,extra),(None,None));
}
/// Marks the full-screen modal backdrop behind the hand-built quantity prompt so
/// touch taps outside the dialog card cancel the prompt cleanly.
#[derive(Component)]
pub(crate) struct QuantityBackdrop;

pub(super) type QuantitySources<'a>=(&'a mut Kit,Option<&'a GameData>,&'a mut Assets<Image>,bool,&'a crate::pad::PadGlyphs,Option<&'a str>);
pub(super) fn spawn_quantity_reusing(root:&mut ChildSpawnerCommands,q:&Quantity,words:&[String;2],
    (kit,data,images,bc,glyphs,extra):QuantitySources,
    (retained,paint):(Option<Entity>,Option<super::PhysicalMeterPaint>)) {
    let mut c = Ctx { kit, data, images, bc, glyphs };
    let bright = amber(255.0);
    let sheet = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    root.spawn((sheet, QuantityBackdrop, FocusPolicy::Block, PointerState::default()));
    let boxed = Node { width: crate::interface::u(Q_BOX.x), height: crate::interface::u(Q_BOX.y), ..default() };
    root.spawn((boxed, FocusPolicy::Block)).with_children(|b| {
        let shade = Color::srgba(0.0, 0.0, 0.0, SHADE);
        if c.image(b, "interface\\shared\\background\\solid_black.dds", at(0.0, 0.0, Q_BOX.x, Q_BOX.y), shade).is_none() {
            b.spawn((at(0.0, 0.0, Q_BOX.x, Q_BOX.y), BackgroundColor(shade)));
        }
        b.spawn(at(Q_IN.x, Q_IN.y, Q_W, Q_BOTTOM + THICK)).with_children(|p| {
            for (y, up) in [(0.0, false), (Q_BOTTOM, true)] {
                c.line(p, 0.0, y, Q_W, THICK, bright);
                let (name, top) = if up { ("fade_to_top.dds", y - Q_FADE + 1.0) } else { ("fade_to_bottom.dds", y) };
                for x in [0.0, Q_W - THICK] {
                    if c.image(p, name, at(x, top, THICK, Q_FADE), bright).is_none() {
                        p.spawn((at(x, top, THICK, Q_FADE), BackgroundColor(bright.with_alpha(0.5))));
                    }
                }
            }
            c.text(p, false, "How many?", Q_CENTRE, Q_PROMPT_Y, Align::Centre, bright);
            // The meter: its ground, the ticks as far as the amount, and over them what
            // takes the pointer.
            let (mx, my, mw, mh) = (Q_METER.min.x, Q_METER.min.y, Q_METER.width(), Q_METER.height());
            c.line(p, mx, my, mw, mh, bright.with_alpha(0.25));
            let tick = c.kit.tick.clone();
            for i in 0..quantity_ticks(q.part()) {
                let x = mx + i as f32 * Q_TICK;
                let w = Q_TICK.min(mx + mw - x);
                match &tick {
                    Some(image) => p.spawn((at(x, my, w, mh), ImageNode { rect: Some(Rect::new(0.0, 0.0, w, 20.0)), color: bright, ..ImageNode::new(image.clone()) })),
                    None => p.spawn((at(x, my, (w - 3.0).max(1.0), mh), BackgroundColor(bright))),
                };
            }
            let node=at(mx,my-6.0,mw,mh+12.0);
            let mut paint=paint;
            if let Some(paint)=paint.as_mut(){paint.style=Some(node.clone());}
            if let Some(e)=retained {
                let parent=p.target_entity();
                let mut commands=p.commands();
                let mut leaf=commands.entity(e);
                leaf.insert((node,ChildOf(parent)));
                if let Some(paint)=paint {leaf.insert(paint);}
            } else {
                let mut leaf=p.spawn((AmountMeter,Button,RelativeCursorPosition::default(),node));
                if let Some(paint)=paint {leaf.insert(paint);}
            }
            for (x, name, step) in [(Q_ARROWS[0], "main_arrow_left.dds", -1), (Q_ARROWS[1], "main_arrow_right.dds", 1)] {
                let hit_w = 48.0;
                let hit_h = 48.0;
                // Expand away from AmountMeter so the generous 48x48 target never overlaps the meter track.
                let pad_x = hit_w - ARROW.x;
                let (box_x, img_x) = if step < 0 { (x - pad_x, pad_x) } else { (x, 0.0) };
                let dy = (hit_h - ARROW.y) * 0.5;
                p.spawn((at(box_x, my - 5.5 - dy, hit_w, hit_h), Button, CmdButton(Command::AmountStep(step)))).with_children(|btn| {
                    let node = at(img_x, dy, ARROW.x, ARROW.y);
                    if c.image(btn, &format!("interface\\main\\{name}"), node.clone(), bright).is_none() {
                        btn.spawn((node, BackgroundColor(bright.with_alpha(0.5))));
                    }
                });
            }
            c.text(p, true, &q.amount.to_string(), Q_CENTRE, Q_AMOUNT_Y, Align::Centre, bright);
            // The player's tweaks: what the amount weighs or costs, under it [design: docs/design/tweaks.md].
            if let Some(line) = extra {
                c.text(p, false, line, Q_CENTRE, Q_EXTRA_Y, Align::Centre, bright);
            }
            let h = (31.0 + BUTTON_PAD)*c.kit.text_size;
            // The prompt's own keys over any menu: A (Enter) and B (Esc) on a controller.
            let any = crate::pad::MenuAt::default();
            let (ok, cancel) = (crate::pad::hint(&[KeyCode::KeyA, KeyCode::Enter], any), crate::pad::hint(&[KeyCode::KeyE, KeyCode::Escape], any));
            for (i,((label,hint,cmd),old_y)) in [(words[0].as_str(),ok,Command::AmountOk),(words[1].as_str(),cancel,Command::AmountCancel)].into_iter().zip(Q_BUTTONS_Y).enumerate() {
                let y=old_y - (2-i) as f32*(h-(31.0+BUTTON_PAD));
                let w = c.text(p, false, label, Q_BUTTONS_RIGHT, y + BUTTON_PAD, Align::Right, bright);
                c.text(p, false, &hint, Q_BUTTONS_RIGHT + HINT_GAP, y + BUTTON_PAD, Align::Left, bright);
                p.spawn((Button, CmdButton(cmd), at(Q_BUTTONS_RIGHT - w - 10.0, y, w + 20.0 + HINT_GAP + 30.0, h)));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_and_the_caps_figure() {
        // The barter screenshot's "ITEMS" (title font) at 50; the line resumes 20 after
        // the right arrow.
        let (l, t, r, (x, len)) = heading(80.0);
        assert_eq!((l, t, r, x, len), (30.0, 50.0, 140.0, 160.0, 304.0));
        assert_eq!(heading(500.0).3, (580.0, 0.0));
        // The wheel's zone runs from the left arrow to the right arrow's end.
        assert_eq!(title_zone(80.0), (30.0, 140.0 + 17.5 - 30.0));
        // "9492" in the title font is about 70 wide: centred 509 from the menu's left
        // when the player is paid, as the screenshot shows.
        assert_eq!(flow_figure_x(Flow::Sell, 70.0), 20.0 + 449.0 + 20.0 + 32.5 + 7.5);
        assert_eq!(flow_figure_x(Flow::Buy, 90.0), 20.0 + 449.0 + 32.5 - 20.0);
        assert_eq!(flow_figure_x(Flow::Sell, 90.0), 20.0 + 449.0 + 20.0 + 40.0 + 20.0);
    }

    /// Issue #1827: with a controller each list names the trigger that picks it, as the
    /// files' title buttons; with the keyboard none, as the original.
    #[test]
    fn the_lists_name_the_trigger_that_picks_them() {
        use crate::pad::Labels;
        use fnv_sim::FormId;
        for mode in [Mode::Container(FormId(0x105658)), Mode::Barter(FormId(0x104C0C))] {
            assert_eq!(pick_hint(mode, Side::Player, Labels::Xbox).as_deref(), Some("[LT]"));
            assert_eq!(pick_hint(mode, Side::Container, Labels::Xbox).as_deref(), Some("[RT]"));
            assert_eq!(pick_hint(mode, Side::Player, Labels::PlayStation).as_deref(), Some("[L2]"));
            assert_eq!(pick_hint(mode, Side::Player, Labels::Keys), None);
        }
        assert_eq!(pick_hint(Mode::Inventory, Side::Player, Labels::Xbox), None);
    }

    #[test]
    fn the_quantity_meter_fills_by_the_amount() {
        // 391 long, ticks 8 apart: a full stack shows 49 ticks (the last cut), half 25.
        assert_eq!(quantity_ticks(1.0), 49);
        assert_eq!(quantity_ticks(0.5), 25);
        assert_eq!(quantity_ticks(0.0), 0);
        assert_eq!(quantity_ticks(2.0), 49);
    }

    #[test]
    fn two_line_names_make_taller_rows() {
        assert_eq!((row_height(1), row_height(2)), (51.0, 86.0));
        // Seven rows of one line fit; two-line rows above the selection push the first
        // drawn row down until the selection fits.
        assert_eq!(first_shown(&[51.0; 10], 0, 6), 0);
        let mut h = vec![51.0; 10];
        h[1] = 86.0;
        h[2] = 86.0;
        assert_eq!(first_shown(&h, 0, 6), 2);
        assert_eq!(first_shown(&h, 0, 2), 0);
    }

    #[test]
    fn the_rows_marker_runs_the_track_by_first_row() {
        assert_eq!(rows_marker(7, 0, 7, 300.0), None);
        let (top, h) = rows_marker(17, 0, 7, 300.0).unwrap();
        assert_eq!((top, h), (0.0, 32.0), "11 positions in 300: the marker's least height");
        let (end, _) = rows_marker(17, 10, 7, 300.0).unwrap();
        assert!((end - (300.0 - h)).abs() < 1e-3);
        // A first row past the end draws as the end.
        assert_eq!(rows_marker(17, 40, 7, 300.0).unwrap().0, end);
    }

    #[test]
    fn the_scroll_marker() {
        // Seven rows fit (14 half-rows of 26 in 365).
        assert_eq!(marker(7, 0), None);
        let (y, h) = marker(20, 0).unwrap();
        assert_eq!(y, 0.0);
        assert!(h >= 32.0);
        // Further down, further down the track, never past its end.
        let (mid, _) = marker(20, 6).unwrap();
        let (end, _) = marker(20, 13).unwrap();
        assert!(0.0 < mid && mid < end && end + h <= LIST_H - 2.0 * BAR_ARROW + 1e-3);
    }
}
