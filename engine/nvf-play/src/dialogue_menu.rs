//! The dialogue menu, drawn as the original draws it (docs/spec-notes/dialogue-menu.md).
//!
//! `crate::dialogue` says what the menu holds ([`DialogueMenu`]: the actor's name, the
//! line being said or the choices offered, the selected choice); this module sets it out
//! by the game's own interface file and draws it with the game's fonts and pictures, read
//! from the owner's archives at run time. Lengths are in the interface's units, 960 to
//! the height of the view.
//!
//! - [`layout`] places everything (pure, tested without the game's files).
//! - While the actor speaks: the line, without a frame, on a dark band. While the player
//!   chooses: the choices between two brackets on the band, the selected one framed, a
//!   scroll bar at the left when not all of them fit.
//! - The actor's name stands at the top right throughout.
//! - While a Goodbye choice is offered, "[Tab] Goodbye" stands at the left of the band,
//!   level with the bottom bracket, in the band's own text on a backing in its shade (list 5
//!   item 62 asked for "TAB) Goodbye" at the left of the dialogue box; the words are now in the
//!   bracket style of the tile menus' key hints, proposed to the maintainer in #1517; the
//!   interface file has no such hint [observed: dialog_menu.xml]): Tab picks that choice
//!   (`crate::dialogue`). The settings page's "Goodbye hint" switch ([`GoodbyeHint`])
//!   hides the words and what is made for them (docs/spec-notes/dialogue-menu.md §4);
//!   Tab works as before.
//! - The menu is rebuilt when what it shows changes; the pointer chooses the choice it
//!   is on (`crate::dialogue` reads the clicks).

use crate::dialogue::{ChoiceButton, ListId};
use crate::interface::{amber, at, bc_supported, face, plain_font, set_line, Face, Picture, Pictures};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use crate::ui_pointer::Button;
use bevy::window::PrimaryWindow;
use fnv_font::{Font, Line};
use nvf_assets::GameData;

// ---- the interface file's numbers [documented: menus\dialog\dialog_menu.xml, §2] ------

/// The dark band's width, centred on the screen.
const BAND_WIDTH: f32 = 1080.0;
/// The safe zone at each edge [documented: Fallout_default.ini `iSafeZoneY`, hud.md §3].
const CROP_Y: f32 = 15.0;
/// The line and the list are centred on this part of the height inside the safe zone,
/// and kept this far above its bottom.
const CENTRE: f32 = 0.8;
const BOTTOM_ROOM: f32 = 40.0;
/// The line starts this far into the band and wraps this much narrower than it.
const TEXT_IN: f32 = 60.0;
/// The band reaches this far above the line's top (less 5) and is this much taller.
const TEXT_BAND_UP: f32 = 45.0;
const TEXT_BAND_MORE: f32 = 80.0;
/// The list: this far into the band and this much narrower; at least this high.
const LIST_IN: f32 = 20.0;
const LIST_LESS: f32 = 70.0;
const MIN_LIST: f32 = 110.0;
/// The brackets: this far right of the list, the top one this far above it, the bottom
/// one this far below it; their upright ends are this long.
const BRACKET_IN: f32 = 15.0;
const BRACKET_ABOVE: f32 = 20.0;
const BRACKET_BELOW: f32 = 10.0;
const BRACKET_ENDS: f32 = 40.0;
/// The band reaches this far above the top bracket's line and below the bottom one's.
const LIST_BAND_UP: f32 = 30.0;
const LIST_BAND_DOWN: f32 = 30.0;
/// The scroll bar's width at the list's left; the choices start right of it and end
/// this far short of the list's right.
const SCROLL_WIDTH: f32 = 32.0;
const CHOICE_SHORT: f32 = 5.0;
/// A choice's text: this far into its row and down from its top; wrapped this much
/// narrower than the row. The row is the text's height and this much more.
const CHOICE_TEXT: Vec2 = Vec2::new(25.0, 15.0);
const CHOICE_WRAP_LESS: f32 = 40.0;
const CHOICE_PAD: f32 = 20.0;
/// The arrows of the scroll bar.
const ARROW: Vec2 = Vec2::new(32.0, 16.0);
const MARKER_WIDTH: f32 = 6.0;
const MARKER_X: f32 = 12.0;
/// The name: this far left of the safe zone's right edge, and its top this far under the
/// safe zone's top (with the safe zone counted twice, as the file does).
const NAME_RIGHT: f32 = 50.0;
const NAME_TOP: f32 = 170.0;
/// The thickness of every line [documented: menus\globals.xml, on a PC].
const THICK: f32 = 2.0;

// ---- measured on the owner's screenshots [observed, §3] ------------------------------

/// The height of a line of font 6, and how far its letters stand under its top.
const LINE: f32 = 38.0;
const TEXT_BASE: f32 = 23.3;
/// How far font 7's letters stand under its top.
const NAME_BASE: f32 = 19.5;
/// The choices stand this much higher in the list than the file puts them.
const LIFT: f32 = 7.2;
/// The tallest the list grows before it scrolls [hypothesis, §3].
const MAX_LIST: f32 = 290.0;
/// Where the arrows' middles stand from the list's top and bottom, and the length of the
/// marker between them.
const UP_MIDDLE: f32 = 31.0;
const DOWN_MIDDLE: f32 = 35.0;
const TRACK_TOP: f32 = 60.0;
const TRACK_BOTTOM: f32 = 52.0;
const MARKER: f32 = 40.0;
/// How dark the band makes what is behind it.
const SHADE: f32 = 0.87;
/// Between a check's tag and the line.
pub const TAG_GAP: &str = "  ";

// ---- NVForever's own [design; the place asked for in owner list 5 item 62] -------------------------------------

/// The hint shown while a Goodbye choice is offered. The request (list 5 item 62) was
/// "TAB) Goodbye"; these words are the bracket style of the tile menus' key hints, proposed
/// to the maintainer in #1517.
pub const GOODBYE_HINT: &str = "[Tab] Goodbye";

/// [`GOODBYE_HINT`] for the device in use: with a controller its B, which leaves as Tab
/// does ("[B] Goodbye", `crate::pad::menu_buttons`) [design, #1662].
pub fn goodbye_hint() -> String {
    let at = crate::pad::at_menu(fnv_game::game::menus::DIALOGUE);
    format!("{} Goodbye", crate::pad::hint_bracketed(&[KeyCode::Tab], at))
}
/// The hint's backing (the band's shade, flush against the band's left edge) reaches this
/// far left of the words and this far above and below their ink (the pictures of their
/// glyphs); the words' right end stands as far from the band [design].
const HINT_PAD_X: f32 = 10.0;
const HINT_PAD_Y: f32 = 8.0;
/// Where the screen has no room at the side, the words stand under the bottom bracket in a
/// strip of the band: this much clear room between their ink and the bracket's line, and
/// between their ink and the band's lower edge, the band growing under the bracket as far
/// as that takes; their left end is this far in from the band's edge (as the line of text
/// starts) [design].
const HINT_CLEAR: f32 = 15.0;
const HINT_IN: f32 = TEXT_IN;
/// How far the words' ink reaches above and below their baseline when the font has no
/// pictures (the stand-in text): a capital's height in font 6, and nothing below
/// [design].
const STAND_IN_INK: (f32, f32) = (17.0, 0.0);

/// The settings page's "Goodbye hint" switch (key `dialogue-goodbye-hint`): on by
/// default [owner, list 5 item 62]; off hides the words, their backing and the narrow screens' strip; a tall list moves back down, Tab still says goodbye.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoodbyeHint(pub bool);

impl Default for GoodbyeHint {
    fn default() -> Self {
        GoodbyeHint(true)
    }
}

/// Whether the Tab hint stands on `view`, for both views: `setting` is the player's switch for
/// it (#1517), and a Goodbye choice has to be offered [owner, list 5 item 62].
pub fn hint_shown(view: &MenuView, setting: bool) -> bool {
    setting && view.goodbye && !view.choices.is_empty()
}

/// A choice as the menu shows it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChoiceRow {
    /// The tag of a skill or perk check and the line.
    pub label: String,
    /// Said before: drawn dim.
    pub darkened: bool,
    /// A check the player falls short of: its frame is red.
    pub short: bool,
}

/// What the menu shows: the actor's name, and the line being said or the choices.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MenuView {
    pub speaker: String,
    pub line: Option<String>,
    pub choices: Vec<ChoiceRow>,
    /// One of the choices is the Goodbye one: Tab says it.
    pub goodbye: bool,
}

/// The menu to draw (set by `crate::dialogue`), its selected choice and the first choice
/// shown ([`window`]).
#[derive(Resource, Default)]
pub struct DialogueMenu {
    pub view: Option<MenuView>,
    pub selected: usize,
    first: usize,
    /// The screen of the conversation this stands for, which its buttons carry.
    pub list: Option<ListId>,
}

// ---- layout --------------------------------------------------------------------------

/// A line of text: where it starts and the line its letters stand on.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub line: Line,
    pub x: f32,
    pub base: f32,
}

/// A rectangle: left, top, width, height.
pub type Area = [f32; 4];

#[derive(Debug, Clone, PartialEq)]
pub struct ChoicePlace {
    pub index: usize,
    pub area: Area,
    pub lines: Vec<Placed>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScrollBar {
    pub up: Area,
    pub down: Area,
    pub marker: Area,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListPlace {
    pub area: Area,
    /// The left end, width and height of the brackets' lines.
    pub top_line: f32,
    pub bottom_line: f32,
    pub bracket_x: f32,
    pub bracket_width: f32,
    pub choices: Vec<ChoicePlace>,
    pub scroll: Option<ScrollBar>,
}

/// The menu set out on a screen `width` units wide (960 high).
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub name: Option<Placed>,
    pub band: Option<Area>,
    pub text: Vec<Placed>,
    pub list: Option<ListPlace>,
    /// "[Tab] Goodbye", while a Goodbye choice is offered and the switch is on.
    pub hint: Option<Placed>,
    /// Its backing in the band's shade, when it stands at the band's side.
    pub hint_back: Option<Area>,
    /// The first choice shown.
    pub first: usize,
}

/// The height of a text of `lines` lines in font 6.
fn text_height(lines: usize) -> f32 {
    LINE * lines as f32
}

/// Where a block `height` high starts: centred on the centring line, but not lower than
/// the room the safe zone leaves under it.
fn centred(height: f32, extra: f32) -> f32 {
    let inside = 960.0 - 2.0 * CROP_Y;
    (inside * CENTRE - height / 2.0).min(inside - height - BOTTOM_ROOM - extra)
}

/// One past the last of the whole choices from `from` that fit in `room`, at least one.
fn fitting(from: usize, heights: &[f32], room: f32) -> usize {
    let mut sum = 0.0;
    let mut end = from;
    while end < heights.len() && (end == from || sum + heights[end] <= room) {
        sum += heights[end];
        end += 1;
    }
    end
}

/// The room the list gives its choices: the whole choices that fit from the top in
/// [`MAX_LIST`], at least one. The same at every scroll of the list, so the band keeps its
/// height and place as the selection goes down it; the room a later window leaves under
/// its last whole choice stays empty [owner's playtest 2026-11-05: the band grew and moved
/// when the selection scrolled onto a wrapped choice, as the choices from the new first one
/// packed differently; design: whole choices only, no choice cut at the foot].
pub fn list_room(heights: &[f32]) -> f32 {
    heights[..fitting(0, heights, MAX_LIST)].iter().sum()
}

/// The first choice shown so that `selected` is among the shown ones, moving as little
/// as possible: as many whole choices as fit the list's room ([`list_room`]), at least one.
pub fn window(first: usize, selected: usize, heights: &[f32]) -> (usize, usize) {
    let n = heights.len();
    if n == 0 {
        return (0, 0);
    }
    let selected = selected.min(n - 1);
    let room = list_room(heights);
    let fits = |from: usize| fitting(from, heights, room);
    let mut first = first.min(selected);
    while fits(first) <= selected {
        first += 1;
    }
    // Show as much as fits: no room left empty under the last choice when earlier ones
    // could be shown.
    while first > 0 && fits(first - 1) == n && fits(first) == n {
        first -= 1;
    }
    (first, fits(first))
}

const LIST_WIDTH: f32 = BAND_WIDTH - LIST_LESS;
const CHOICE_WIDTH: f32 = LIST_WIDTH - SCROLL_WIDTH - CHOICE_SHORT;

/// Each choice's lines.
fn choice_lines(view: &MenuView, body: &Font) -> Vec<Vec<Line>> {
    view.choices.iter().map(|c| body.lines(c.label.trim(), Some(CHOICE_WIDTH - CHOICE_WRAP_LESS))).collect()
}

/// Each choice's row height: its text's and the room under it.
fn choice_heights(lines: &[Vec<Line>]) -> Vec<f32> {
    lines.iter().map(|l| text_height(l.len().max(1)) + CHOICE_PAD).collect()
}

/// The safe zone at the sides: as the height's in a 4:3 screen, narrower in units on a
/// wider one [hypothesis, §3].
fn crop_x(width: f32) -> f32 {
    CROP_Y * 1280.0 / width
}

/// How far the pictures of `line`'s glyphs reach above and below the baseline they stand
/// on: the greatest ascent, and the greatest of height less ascent (the glyph's own
/// numbers, `fnv_font::Glyph`); [`STAND_IN_INK`] where none is drawn.
fn glyph_ink(line: &Line, font: &Font) -> (f32, f32) {
    let drawn: Vec<_> = line.glyphs.iter().map(|p| font.bitmap_glyph(p)).filter(|g| g.drawn()).collect();
    if drawn.is_empty() {
        return STAND_IN_INK;
    }
    let up = drawn.iter().map(|g| g.ascent).fold(f32::MIN, f32::max);
    let down = drawn.iter().map(|g| g.height - g.ascent).fold(f32::MIN, f32::max);
    (up.max(0.0), down.max(0.0))
}

/// How much the band grows under its bottom bracket for words whose ink is `ink`: what the
/// ink and [`HINT_CLEAR`] above and below it need beyond the band's own strip there.
fn hint_room(ink: (f32, f32)) -> f32 {
    (ink.0 + ink.1 + 2.0 * HINT_CLEAR - (LIST_BAND_DOWN - THICK)).max(0.0)
}

/// Whether "[Tab] Goodbye", `words` wide, stands left of the band with its backing inside
/// the safe zone.
fn hint_at_side(words: f32, width: f32, band_x: f32) -> bool {
    band_x - HINT_PAD_X - words - HINT_PAD_X >= crop_x(width)
}

/// Where "[Tab] Goodbye" stands, its letters on `base`, and its backing; `ink` is the words'
/// [`glyph_ink`]. At the band's side: the words' right end [`HINT_PAD_X`] from the band's
/// edge, their ink centred on the bottom bracket's line, on a backing flush against the
/// band. Else (the screen is too narrow for that: it depends on the screen's width and the
/// words', not on one aspect) inside the band, in the strip under the bottom bracket that
/// [`hint_room`] makes tall enough, [`HINT_IN`] from the band's left edge (kept inside the
/// safe zone), the ink centred in the strip: [`HINT_CLEAR`] clear of the bracket's line and
/// corner, under the choices and the scroll bar [design].
fn hint_place(line: Line, words: f32, ink: (f32, f32), width: f32, band_x: f32, bottom_line: f32) -> (Placed, Option<Area>) {
    let (up, down) = ink;
    if hint_at_side(words, width, band_x) {
        let x = band_x - HINT_PAD_X - words;
        let base = bottom_line + THICK / 2.0 + (up - down) / 2.0;
        let back = [x - HINT_PAD_X, base - up - HINT_PAD_Y, words + 2.0 * HINT_PAD_X, up + down + 2.0 * HINT_PAD_Y];
        return (Placed { x, base, line }, Some(back));
    }
    let right = band_x + BAND_WIDTH - HINT_IN - words;
    let x = (band_x + HINT_IN).min(right).max(crop_x(width));
    let strip = LIST_BAND_DOWN - THICK + hint_room(ink);
    let base = bottom_line + THICK + (strip - up - down) / 2.0 + up;
    (Placed { x, base, line }, None)
}

/// `hint`: whether the "Goodbye hint" switch is on.
#[allow(clippy::too_many_arguments)]
pub fn layout(view: &MenuView, first: usize, selected: usize, width: f32, hint: bool, body: &Font, name_font: &Font, glyphs: &crate::pad::PadGlyphs) -> Layout {
    let band_x = (width - BAND_WIDTH) / 2.0;
    let name = Some(view.speaker.trim()).filter(|s| !s.is_empty()).map(|s| {
        let line = name_font.lines(s, None).remove(0);
        let right = width - 2.0 * crop_x(width) - NAME_RIGHT;
        Placed { x: right - line.width, base: 2.0 * CROP_Y + NAME_TOP + NAME_BASE, line }
    });
    let mut out = Layout { name, band: None, text: Vec::new(), list: None, hint: None, hint_back: None, first: 0 };
    if view.choices.is_empty() {
        let Some(text) = view.line.as_deref().map(str::trim).filter(|t| !t.is_empty()) else { return out };
        let lines = body.lines(text, Some(BAND_WIDTH - 2.0 * TEXT_IN));
        let height = text_height(lines.len());
        let top = centred(height, 0.0);
        out.band = Some([band_x, top - TEXT_BAND_UP, BAND_WIDTH, height + TEXT_BAND_MORE]);
        out.text = lines.into_iter().enumerate().map(|(i, line)| Placed { line, x: band_x + TEXT_IN, base: top + TEXT_BASE + LINE * i as f32 }).collect();
        return out;
    }

    let list_x = band_x + LIST_IN;
    let choice_x = list_x + SCROLL_WIDTH;
    let lines = choice_lines(view, body);
    let heights = choice_heights(&lines);
    let (first, end) = window(first, selected, &heights);
    let height = list_room(&heights).max(MIN_LIST);
    // The hint's own words, and the room under the band they need where they stand in it.
    let hint_text = hint_shown(view, hint).then(goodbye_hint);
    let hint_line = hint_text.as_deref().map(|t| body.lines(t, None).remove(0));
    let ink = hint_line.as_ref().map_or(STAND_IN_INK, |l| glyph_ink(l, body));
    let words = hint_text.as_deref().map_or(0.0, |t| crate::pad::hint_width_font(body, t, glyphs));
    let room = hint_text.as_deref().map_or(0.0, |_| if hint_at_side(words, width, band_x) { 0.0 } else { hint_room(ink) });
    let top = centred(height, room);
    let mut y = top - LIFT;
    let mut choices = Vec::new();
    for (i, ls) in lines.iter().enumerate().take(end).skip(first) {
        let placed = ls
            .iter()
            .enumerate()
            .map(|(j, l)| Placed { line: l.clone(), x: choice_x + CHOICE_TEXT.x, base: y + CHOICE_TEXT.y + TEXT_BASE + LINE * j as f32 })
            .collect();
        choices.push(ChoicePlace { index: i, area: [choice_x, y, CHOICE_WIDTH, heights[i]], lines: placed });
        y += heights[i];
    }
    let bottom = top + height;
    let scroll = (end - first < view.choices.len()).then(|| {
        let (track_top, track_bottom) = (top + TRACK_TOP, bottom - TRACK_BOTTOM);
        let hidden = view.choices.len() - (end - first);
        let along = (track_bottom - track_top - MARKER).max(0.0) * first as f32 / hidden as f32;
        ScrollBar {
            up: [list_x, top + UP_MIDDLE - ARROW.y / 2.0, ARROW.x, ARROW.y],
            down: [list_x, bottom - DOWN_MIDDLE - ARROW.y / 2.0, ARROW.x, ARROW.y],
            marker: [list_x + MARKER_X, track_top + along, MARKER_WIDTH, MARKER],
        }
    });
    let top_line = top - BRACKET_ABOVE;
    let bottom_line = bottom + BRACKET_BELOW;
    out.band = Some([band_x, top_line - LIST_BAND_UP, BAND_WIDTH, bottom_line - top_line + LIST_BAND_UP + LIST_BAND_DOWN + room]);
    out.list = Some(ListPlace { area: [list_x, top, LIST_WIDTH, height], top_line, bottom_line, bracket_x: list_x + BRACKET_IN, bracket_width: LIST_WIDTH, choices, scroll });
    out.first = first;
    if let Some(line) = hint_line {
        let (placed, back) = hint_place(line, words, ink, width, band_x, bottom_line);
        (out.hint, out.hint_back) = (Some(placed), back);
    }
    out
}

// ---- the game's fonts and pictures ---------------------------------------------------

/// Font 6 (the line and the choices) and font 7 (the name) [documented: the interface
/// file names them by number; Fallout_default.ini `sFontFile_6`, `_7`].
const BODY_FONT: &str = "textures\\fonts\\glow_monofonto_vl_dialogs.fnt";
const NAME_FONT: &str = "textures\\fonts\\baked-in_monofonto_large.fnt";

#[derive(Clone)]
pub(crate) struct Kit {
    pub(crate) body: Face,
    pub(crate) name: Face,
    band: Option<Picture>,
    fade_down: Option<Picture>,
    fade_up: Option<Picture>,
    arrow_up: Option<Picture>,
    arrow_down: Option<Picture>,
    marker: Option<Picture>,
}

pub(crate) fn load_kit(data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Kit {
    let mut pictures = Pictures::load(data);
    let mut picture = |path: &str| pictures.picture(path, data, images, bc);
    let band = picture("interface\\shared\\background\\solid_black.dds");
    let fade_down = picture("interface\\shared\\line\\fade_to_bottom.dds");
    let fade_up = picture("interface\\shared\\line\\fade_to_top.dds");
    let arrow_up = picture("interface\\shared\\scrollbar\\arrow_up.dds");
    let arrow_down = picture("interface\\shared\\scrollbar\\arrow_down.dds");
    let marker = picture("interface\\shared\\scrollbar\\vert_marker.dds");
    Kit {
        body: face(BODY_FONT, plain_font(34.0, 17.0), data, images, bc),
        name: face(NAME_FONT, plain_font(36.0, 16.0), data, images, bc),
        band,
        fade_down,
        fade_up,
        arrow_up,
        arrow_down,
        marker,
    }
}

// ---- drawing ---------------------------------------------------------------------------

pub struct DialogueMenuPlugin;

impl Plugin for DialogueMenuPlugin {
    fn build(&self, app: &mut App) {
        crate::dialogue_touch::install(app);
        app.init_resource::<DialogueMenu>().init_resource::<GoodbyeHint>().add_systems(Update, draw.in_set(crate::PlaySet::Ui).in_set(DrawMenu));
        crate::settings::add_setting(
            app,
            crate::settings::Setting {
                key: "dialogue-goodbye-hint",
                group: crate::pause::GROUP_MENUS,
                label: "Goodbye hint",
                note: "On: \"[Tab] Goodbye\" stands at the left of the dialogue box while a Goodbye choice is offered. Off: hidden; Tab still says goodbye.",
                switches: "",
                get: |w| w.resource::<GoodbyeHint>().0,
                set: |w, on| w.resource_mut::<GoodbyeHint>().0 = on,
                slider: None,
            },
        );
    }
}

/// The drawing of the menu, for what fills [`DialogueMenu`] to run before.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DrawMenu;

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
pub(crate) struct TouchNativeRoot;

/// The frame of a choice, shown while it is the selected one.
#[derive(Component)]
struct Frame(usize);

/// The line of the red frame of a check the player falls short of [observed: §3].
fn short_line() -> Color {
    Color::srgb_u8(100, 18, 16)
}

fn image(p: Option<&Picture>, color: Color) -> Option<ImageNode> {
    p.map(|(image, rect)| ImageNode { rect: *rect, color, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image.clone()) })
}

fn spawn_menu(commands: &mut Commands, view: &MenuView, l: &Layout, kit: &Kit, selected: usize, on_screen: Option<&ListId>, glyphs: &crate::pad::PadGlyphs) {
    spawn_menu_on(commands, view, l, kit, selected, on_screen, glyphs, false, None, None);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_touch_menu(commands: &mut Commands, view: &MenuView, l: &Layout, kit: &Kit, selected: usize, on_screen: Option<&ListId>, glyphs: &crate::pad::PadGlyphs, confirm: Option<(&str,f32)>, camera: Entity) {
    spawn_menu_on(commands, view, l, kit, selected, on_screen, glyphs, true, confirm, Some(camera));
}

#[allow(clippy::too_many_arguments)]
fn spawn_menu_on(commands: &mut Commands, view: &MenuView, l: &Layout, kit: &Kit, selected: usize, on_screen: Option<&ListId>, glyphs: &crate::pad::PadGlyphs, touch: bool, confirm: Option<(&str,f32)>, camera: Option<Entity>) {
    let screen = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    // At the UI scale the menu keeps to the view's bottom, in its middle (#1631).
    let screen = if touch { screen } else { crate::interface::framed_root(screen, crate::interface::BOTTOM_MIDDLE) };
    let bright = amber(255.0);
    let mut root = commands.spawn((screen, GlobalZIndex(if touch {2400} else {90})));
    if touch {root.insert((TouchNativeRoot,crate::dialogue_touch::PaletteRoot));} else {root.insert(MenuRoot);}
    if let Some(camera)=camera {root.insert(UiTargetCamera(camera));}
    root.with_children(|s| {
        if let Some([x, y, w, h]) = l.band {
            let shade = Color::srgba(0.0, 0.0, 0.0, SHADE);
            match image(kit.band.as_ref(), shade) {
                Some(i) => s.spawn((at(x, y, w, h), i)),
                None => s.spawn((at(x, y, w, h), BackgroundColor(shade))),
            };
        }
        if let Some(n) = &l.name {
            set_line(s, &kit.name, &n.line, n.x, n.base, bright);
        }
        if let Some([x, y, w, h]) = l.hint_back {
            let shade = Color::srgba(0.0, 0.0, 0.0, SHADE);
            match image(kit.band.as_ref(), shade) {
                Some(i) => s.spawn((at(x, y, w, h), i)),
                None => s.spawn((at(x, y, w, h), BackgroundColor(shade))),
            };
        }
        if let Some(hint) = &l.hint {
            crate::pad::set_hint(s, &kit.body, &goodbye_hint(), hint.x, hint.base, bright, glyphs);
        }
        for t in &l.text {
            set_line(s, &kit.body, &t.line, t.x, t.base, bright);
        }
        if touch && view.choices.is_empty() {
            if let Some([x,y,w,h]) = l.band {
                s.spawn((Button, crate::dialogue_touch::native_skip(on_screen.cloned()), at(x,y,w,h)));
            }
        }
        let Some(list) = &l.list else { return };
        // The brackets: a line with an upright at each end that fades out, down from the
        // top one and up from the bottom one.
        let (bx, bw) = (list.bracket_x, list.bracket_width);
        for (y, fade, ends_y) in [(list.top_line, &kit.fade_down, list.top_line), (list.bottom_line, &kit.fade_up, list.bottom_line + THICK - BRACKET_ENDS)] {
            s.spawn((at(bx, y, bw, THICK), BackgroundColor(bright)));
            for x in [bx, bx + bw - THICK] {
                if let Some(i) = image(fade.as_ref(), bright) {
                    s.spawn((at(x, ends_y, THICK, BRACKET_ENDS), i));
                }
            }
        }
        if let Some(bar) = &list.scroll {
            for (area, p) in [(bar.up, &kit.arrow_up), (bar.down, &kit.arrow_down), (bar.marker, &kit.marker)] {
                let [x, y, w, h] = area;
                match image(p.as_ref(), bright) {
                    Some(i) => s.spawn((at(x, y, w, h), i)),
                    None => s.spawn((at(x + w / 2.0 - THICK, y, 2.0 * THICK, h), BackgroundColor(bright))),
                };
            }
        }
        if let Some((caption,minimum)) = confirm {
            let [x,_,w,_] = list.area;
            let y = list.bottom_line + 15.0;
            let height = minimum.max(kit.body.font.size * 1.6);
            s.spawn((Button,crate::dialogue_touch::native_confirm(on_screen.cloned()),
                at(x,y,w,height),BackgroundColor(amber(128.0).with_alpha(0.1))))
                .with_children(|p| {
                    for (index,line) in kit.body.font.lines(caption,Some(w-40.0)).iter().enumerate() {
                        set_line(p,&kit.body,line,20.0,crate::interface::line_base(&kit.body)
                            + index as f32 * kit.body.font.size * 1.2,bright);
                    }
                });
        }
        // The choices, cut at the brackets' lines.
        let clip_top = list.top_line + THICK;
        let mut window = at(0.0, clip_top, 0.0, list.bottom_line - clip_top);
        (window.width, window.overflow) = (Val::Percent(100.0),
            if touch { Overflow::scroll_y() } else { Overflow::clip() });
        let mut clip = s.spawn(window);
        if touch {clip.insert(crate::dialogue_touch::native_viewport());}
        clip.with_children(|w| {
            for c in &list.choices {
                let [x, y, cw, ch] = c.area;
                let row = &view.choices[c.index];
                let mut choice = w.spawn((Button, ChoiceButton(c.index, on_screen.cloned()), at(x, y - clip_top, cw, ch)));
                if touch {choice.insert(crate::dialogue_touch::native_choice(c.index,on_screen.cloned(),row.short));}
                choice.with_children(|r| {
                    // [documented: the list's highlight box] One unit short of the row; a
                    // faint fill and a line around it, red for a check the player falls
                    // short of [observed].
                    let h = ch - 1.0;
                    let (line, fill) = if row.short { (short_line(), Color::srgba_u8(62, 0, 0, 82)) } else { (amber(200.0), amber(128.0).with_alpha(0.1)) };
                    let shown = if c.index == selected { Visibility::Inherited } else { Visibility::Hidden };
                    r.spawn((Frame(c.index), at(0.0, 0.0, cw, h), shown, BackgroundColor(fill))).with_children(|f| {
                        for fy in [0.0, h - THICK] {
                            f.spawn((at(0.0, fy, cw, THICK), BackgroundColor(line)));
                        }
                        for fx in [0.0, cw - THICK] {
                            f.spawn((at(fx, THICK, THICK, h - 2.0 * THICK), BackgroundColor(line)));
                        }
                    });
                    // Said before: as bright as the interface's disabled text (128 of 255).
                    let color = if row.darkened { amber(128.0) } else { bright };
                    for t in &c.lines {
                        set_line(r, &kit.body, &t.line, t.x - x, t.base - y, color);
                    }
                });
            }
        });
    });
}


/// What the drawn menu was made from: the menu's view, the first choice shown, the width
/// in units, the "Goodbye hint" switch, the button labels and the current font revision.
type MenuKey = (MenuView, usize, u32, bool, crate::pad::Labels, u32);

fn menu_key(view: Option<&MenuView>, creating: bool, first: usize, width: f32, hint: bool) -> Option<MenuKey> {
    view.filter(|_| !creating).map(|v| (v.clone(), first, width.round() as u32, hint, crate::pad::labels(), crate::cjk_font::revision()))
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    mut menu: ResMut<DialogueMenu>,
    data: Option<Res<GameData>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    roots: Query<Entity, With<MenuRoot>>,
    mut frames: Query<(&Frame, &mut Visibility)>,
    mut kit: Local<Option<Kit>>,
    mut shown: Local<Option<MenuKey>>,
    creator: Option<Res<crate::creator::CreatorMenu>>,
    tiles: Option<Res<crate::tiles::dialog_menu::DialogTiles>>,
    mut buttons: Query<&mut ChoiceButton>,
    hint: Res<GoodbyeHint>,
    glyphs: Res<crate::pad::PadGlyphs>,
    touch: Res<crate::dialogue_touch::TouchDialogue>,
) {
    if touch.enabled {
        for e in &roots { commands.entity(e).despawn(); }
        *shown = None;
        return;
    }
    // Drawn from its file: all of the menu, none of it here (`crate::tiles::dialog_menu`).
    if tiles.is_some_and(|t| t.by_tiles()) {
        for e in &roots {
            commands.entity(e).despawn();
        }
        *shown = None;
        return;
    }
    let width = windows.single().map_or(960.0 * 16.0 / 9.0, |w| 960.0 * w.width() / w.height().max(1.0));
    let selected = menu.selected;
    let kit = kit.get_or_insert_with(|| load_kit(data.as_deref(), &mut images, bc_supported(formats.as_deref())));
    // Where the list starts follows the selection: the menu is rebuilt when it scrolls.
    let first = match &menu.view {
        Some(v) if !v.choices.is_empty() => window(menu.first, selected, &choice_heights(&choice_lines(v, &kit.body.font))).0,
        _ => 0,
    };
    if menu.first != first {
        menu.first = first;
    }
    // Nothing over the character creator: the opening opens it at the end of a line of
    // Doc Mitchell's, still in the conversation [owner 2026-10-17].
    let creating = creator.is_some_and(|c| c.is_open());
    let key = menu_key(menu.view.as_ref(), creating, first, width, hint.0);
    if key != *shown {
        for e in &roots {
            commands.entity(e).despawn();
        }
        if let Some((view, first, _, hint_on, _, _)) = &key {
            let l = layout(view, *first, selected, width, *hint_on, &kit.body.font, &kit.name.font, &glyphs);
            // One line when the hint appears, or when the switch holds it back.
            let was = shown.as_ref().and_then(|s| s.0.goodbye.then_some(s.3));
            match (view.goodbye.then_some(*hint_on), was) {
                (Some(true), was) if was != Some(true) => info!("dialogue menu: {} shown at the left of the choices", goodbye_hint()),
                (Some(false), was) if was != Some(false) => info!("dialogue menu: {} hidden by the Goodbye hint setting", goodbye_hint()),
                _ => {}
            }
            spawn_menu(&mut commands, view, &l, kit, selected, menu.list.as_ref(), &glyphs);
        }
        *shown = key;
        return;
    }
    for (frame, mut v) in &mut frames {
        v.set_if_neq(if frame.0 == selected { Visibility::Inherited } else { Visibility::Hidden });
    }
    // The same words for the next screen (no rebuild): what stands is that screen's now.
    for mut b in &mut buttons {
        if b.1 != menu.list {
            b.1.clone_from(&menu.list);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 17 on, spaces 14; sizes as the game's fonts 6 and 7.
    fn font(size: f32) -> Font {
        let mut f = plain_font(size, 17.0);
        f.glyphs[b' ' as usize].after = 14.0;
        f
    }

    fn choices(labels: &[&str]) -> MenuView {
        MenuView { speaker: "Trudy".into(), line: None, choices: labels.iter().map(|l| ChoiceRow { label: l.to_string(), ..default() }).collect(), goodbye: false }
    }

    fn lay(v: &MenuView, width: f32, hint: bool) -> Layout {
        layout(v, 0, 0, width, hint, &font(34.0), &font(36.0), &no_glyphs())
    }

    /// No glyph pictures: the test's hints name no PlayStation button, so the width is the
    /// text's.
    fn no_glyphs() -> crate::pad::PadGlyphs {
        crate::pad::PadGlyphs::default()
    }

    /// A font of the test's own numbers: the letters of the hint are pictures 10 wide (as
    /// 17 on with the 7 after them, so the words are as wide as in `font`) whose top stands
    /// 24 above their baseline and which are 24 high, except the brackets (top 40 above, 52 high:
    /// they reach 12 below) and "y" (top 17 above, 29 high: it reaches 12 below).
    fn pictured() -> Font {
        let mut f = font(34.0);
        let glyph = |ascent: f32, height: f32| fnv_font::Glyph { uv: [0.0, 0.0, 1.0, 1.0], width: 10.0, height, before: 0.0, after: 7.0, ascent };
        for c in "TabGodye".bytes() {
            f.glyphs[c as usize] = glyph(24.0, 24.0);
        }
        f.glyphs[b'[' as usize] = glyph(40.0, 52.0);
        f.glyphs[b']' as usize] = glyph(40.0, 52.0);
        f.glyphs[b'y' as usize] = glyph(17.0, 29.0);
        f
    }

    fn lay_with(v: &MenuView, width: f32, hint: bool, body: &Font) -> Layout {
        layout(v, 0, 0, width, hint, body, &font(36.0), &no_glyphs())
    }

    /// The hint's ink: left, top, right, bottom, from the numbers `up` above and `down`
    /// below its baseline.
    fn ink_box(h: &Placed, up: f32, down: f32) -> [f32; 4] {
        [h.x, h.base - up, h.x + h.line.width, h.base + down]
    }

    fn meets(a: [f32; 4], b: Area) -> bool {
        a[0] < b[0] + b[2] && b[0] < a[2] && a[1] < b[1] + b[3] && b[1] < a[3]
    }

    fn goodbye(labels: &[&str]) -> MenuView {
        MenuView { goodbye: true, ..choices(labels) }
    }

    #[test]
    fn the_hint_ink_is_read_from_the_glyphs() {
        let line = pictured().lines(GOODBYE_HINT, None).remove(0);
        // "[" and "]" reach 40 above and 12 below; "y" 12 below.
        assert_eq!(glyph_ink(&line, &pictured()), (40.0, 12.0));
        // A font with no pictures: the stand-in's numbers.
        assert_eq!(glyph_ink(&font(34.0).lines(GOODBYE_HINT, None).remove(0), &font(34.0)), STAND_IN_INK);
        // 52 of ink and 15 clear above and below it need 82 under the line, 54 more than the
        // band has there; the stand-in's 17 need 19 more.
        assert_eq!(hint_room((40.0, 12.0)), 54.0);
        assert_eq!(hint_room(STAND_IN_INK), 19.0);
        assert_eq!(hint_room((5.0, 0.0)), 7.0);
        assert_eq!(hint_room((0.0, 0.0)), 2.0);
    }

    #[test]
    fn the_goodbye_hint_stands_left_of_the_band_on_a_backing() {
        let mut v = choices(&["What happened here?", "Goodbye."]);
        assert!(lay(&v, 1707.0, true).hint.is_none(), "no hint without a Goodbye choice");
        v.goodbye = true;
        // With the tall, descending glyphs: ink 40 above the baseline, 12 below.
        let body = pictured();
        let l = lay_with(&v, 1707.0, true, &body);
        let hint = l.hint.expect("the hint");
        let (band, list, back) = (l.band.unwrap(), l.list.unwrap(), l.hint_back.expect("its backing"));
        // The words end HINT_PAD_X from the band; their ink (28 high) is centred on the
        // bottom bracket's line, 14 above the baseline.
        assert!((hint.x + hint.line.width - (band[0] - HINT_PAD_X)).abs() < 0.01);
        assert!((hint.base - 14.0 - (list.bottom_line + THICK / 2.0)).abs() < 0.01);
        // The backing is flush against the band and reaches 10 and 8 round the ink.
        let ink = ink_box(&hint, 40.0, 12.0);
        assert!((back[0] + back[2] - band[0]).abs() < 0.01);
        assert!((ink[0] - back[0] - 10.0).abs() < 0.01 && (ink[1] - back[1] - 8.0).abs() < 0.01);
        assert!((back[1] + back[3] - ink[3] - 8.0).abs() < 0.01);
        assert!(back[0] >= crop_x(1707.0));
        // The band is the height it is without the hint.
        assert_eq!(l.band, lay_with(&v, 1707.0, false, &body).band);
        // A font with no pictures: the stand-in's 17 above, none below, centred the same way.
        let l = lay(&v, 1707.0, true);
        let hint = l.hint.expect("the hint");
        assert!((hint.base - 8.5 - (l.list.unwrap().bottom_line + THICK / 2.0)).abs() < 0.01);
    }

    #[test]
    fn the_goodbye_hint_moves_into_the_band_where_the_side_is_too_narrow() {
        let v = goodbye(&["One", "Two", "Goodbye."]);
        // 1280 units wide is 4:3 (1024x768 too); 1200 is 5:4; 1100 narrower. The band's left
        // edge is 100, 60 and 10 from the screen's, the words 218 wide: no room at the side.
        for width in [1280.0, 1200.0, 1100.0] {
            for (body, up, down, room) in [(pictured(), 40.0, 12.0, 54.0), (font(34.0), 17.0, 0.0, 19.0)] {
                let l = lay_with(&v, width, true, &body);
                let hint = l.hint.expect("the hint");
                let (band, list) = (l.band.unwrap(), l.list.unwrap());
                assert!(l.hint_back.is_none(), "width {width}: no backing inside the band");
                let [left, top, right, bottom] = ink_box(&hint, up, down);
                // The strip under the bracket's line is 28 + room high, the ink centred in it:
                // 15 clear above and below.
                assert!((band[3] - lay_with(&v, width, false, &body).band.unwrap()[3] - room).abs() < 0.01, "width {width}: the band grew by {room}");
                assert!((top - (list.bottom_line + THICK) - HINT_CLEAR).abs() < 0.01, "width {width}: clear of the bracket");
                assert!((band[1] + band[3] - bottom - HINT_CLEAR).abs() < 0.01, "width {width}: clear of the band's bottom");
                // HINT_IN from the band's left edge (or the safe zone's), inside the safe zone.
                assert!((left - (band[0] + HINT_IN).max(crop_x(width))).abs() < 0.01, "width {width}");
                assert!(left >= crop_x(width) && right <= width - crop_x(width), "width {width}: inside the safe zone");
                assert!(band[1] + band[3] <= 960.0 - CROP_Y, "width {width}: the band ends inside the safe zone");
            }
        }
    }

    #[test]
    fn the_goodbye_hint_never_covers_a_choice_or_the_scroll_bar() {
        // Six long choices scroll (the list at its tallest); 5:4, 4:3, wider screens, the
        // selection at both ends, the tall glyphs and the stand-in.
        let long = "Tell me everything you know about the people who attacked the town last night";
        let v = goodbye(&[long, long, long, long, long, "Goodbye."]);
        for (body, up, down) in [(pictured(), 40.0, 12.0), (font(34.0), 17.0, 0.0)] {
            for width in [1200.0, 1280.0, 1440.0, 1707.0, 2560.0] {
                for selected in [0, 5] {
                    let l = layout(&v, 0, selected, width, true, &body, &font(36.0), &no_glyphs());
                    let hint = l.hint.expect("the hint");
                    let ink = ink_box(&hint, up, down);
                    let (band, list) = (l.band.unwrap(), l.list.unwrap());
                    assert!(list.scroll.is_some());
                    for c in &list.choices {
                        assert!(!meets(ink, c.area), "width {width}: over choice {}", c.index);
                    }
                    let bar = list.scroll.as_ref().unwrap();
                    for a in [bar.up, bar.down, bar.marker] {
                        assert!(!meets(ink, a), "width {width}: over the scroll bar");
                    }
                    if let Some(back) = l.hint_back {
                        assert!(!meets(ink, band) && back[0] >= crop_x(width), "width {width}: the side's backing is outside the band");
                    }
                    assert!(ink[0] >= crop_x(width) && ink[2] <= width - crop_x(width), "width {width}: inside the safe zone");
                    assert!(band[1] + band[3] <= 960.0 - CROP_Y, "width {width}: the band ends inside the safe zone");
                }
            }
        }
    }

    #[test]
    fn the_goodbye_hint_switch_changes_the_words_and_where_they_need_room() {
        let v = goodbye(&["What happened here?", "Goodbye."]);
        // At the side the words and their backing are all that change.
        let (on, off) = (lay(&v, 1707.0, true), lay(&v, 1707.0, false));
        assert!(on.hint.is_some() && on.hint_back.is_some());
        assert!(off.hint.is_none() && off.hint_back.is_none());
        assert_eq!((on.band, on.list, on.name), (off.band, off.list, off.name));
        // Where they stand in the band, a short list stays and the band shrinks to its own.
        let (on, off) = (lay(&v, 1280.0, true), lay(&v, 1280.0, false));
        assert!(on.hint.is_some() && off.hint.is_none());
        assert_eq!((on.list.as_ref(), on.name.as_ref()), (off.list.as_ref(), off.name.as_ref()));
        assert!(on.band.unwrap()[3] > off.band.unwrap()[3]);
        // But a list at its tallest moves up for the room: turning the hint on moves it up
        // by up to the room (19 for the stand-in's ink).
        let long = "Tell me everything you know about the people who attacked the town last night";
        let v = goodbye(&[long, long, long, long, long, "Goodbye."]);
        let (on, off) = (lay(&v, 1280.0, true), lay(&v, 1280.0, false));
        let shift = off.list.unwrap().area[1] - on.list.unwrap().area[1];
        assert!(shift > 0.0 && shift <= hint_room(STAND_IN_INK), "moved up by {shift}");
    }

    #[test]
    fn a_flip_of_the_switch_rebuilds_the_menu() {
        let mut v = choices(&["Hello.", "Goodbye."]);
        v.goodbye = true;
        let on = menu_key(Some(&v), false, 0, 1707.0, true);
        assert_eq!(on, menu_key(Some(&v), false, 0, 1707.2, true));
        assert_ne!(on, menu_key(Some(&v), false, 0, 1707.0, false), "the drawing's cache sees the switch");
        assert_eq!(menu_key(Some(&v), true, 0, 1707.0, true), None, "nothing over the character creator");
    }

    #[test]
    fn the_goodbye_hint_is_a_setting_on_by_default() {
        let mut app = App::new();
        app.add_plugins(DialogueMenuPlugin);
        let world = app.world_mut();
        assert!(world.resource::<GoodbyeHint>().0);
        let settings = world.resource::<crate::settings::Settings>();
        let s = settings.list()[settings.index("dialogue-goodbye-hint").expect("listed")];
        assert_eq!((s.group, s.label), (crate::pause::GROUP_MENUS, "Goodbye hint"));
        assert!((s.get)(world));
        (s.set)(world, false);
        assert!(!world.resource::<GoodbyeHint>().0 && !(s.get)(world));
    }

    #[test]
    fn the_line_is_centred_on_its_band() {
        // Two lines on a 16:9 screen, as the owner's screenshot of Bruce Isaac.
        let text = format!("{} {}", "a".repeat(55), "b".repeat(5));
        let v = MenuView { speaker: "Bruce Isaac".into(), line: Some(text), choices: Vec::new(), goodbye: false };
        let width = 960.0 * 16.0 / 9.0;
        let l = layout(&v, 0, 0, width, true, &font(34.0), &font(36.0), &no_glyphs());
        assert_eq!(l.text.len(), 2);
        // The text's top at 706, its first line standing 23.3 under it, the next 38 lower.
        assert!((l.text[0].base - 729.3).abs() < 1e-3);
        assert!((l.text[1].base - 767.3).abs() < 1e-3);
        // Starts 60 into the band, which is 1080 wide and centred.
        let band = l.band.unwrap();
        assert!((band[0] - (width - 1080.0) / 2.0).abs() < 1e-3);
        assert!((l.text[0].x - band[0] - 60.0).abs() < 1e-3);
        assert_eq!((band[1], band[3]), (661.0, 156.0));
        assert!(l.list.is_none());
        // The name stands right of the middle at the top, ending short of the edge.
        let n = l.name.unwrap();
        assert!((n.x + n.line.width - (width - 2.0 * crop_x(width) - 50.0)).abs() < 1e-3);
        assert_eq!(n.base, 219.5);
    }

    #[test]
    fn the_choices_stand_between_the_brackets() {
        // Three choices on a 4:3 screen, two of two lines and one of one, as the owner's
        // screenshot of Trudy; a fourth that does not fit.
        let long = "abcde ".repeat(12);
        let v = choices(&[&long, &long, "Do you know the robot?", &long]);
        let l = layout(&v, 0, 1, 1280.0, true, &font(34.0), &font(36.0), &no_glyphs());
        let list = l.list.unwrap();
        assert_eq!(list.choices.len(), 3);
        // The list is 250 high, its top at 619 and the brackets' lines 20 above and 10
        // below it.
        assert_eq!(list.area, [120.0, 619.0, 1010.0, 250.0]);
        assert_eq!((list.top_line, list.bottom_line, list.bracket_x), (599.0, 879.0, 135.0));
        // The rows: 96, 96 and 58 high, starting 7.2 above the list's top, from the right
        // of the scroll bar.
        let tops: Vec<f32> = list.choices.iter().map(|c| c.area[1]).collect();
        assert!((tops[0] - 611.8).abs() < 1e-3 && (tops[1] - 707.8).abs() < 1e-3 && (tops[2] - 803.8).abs() < 1e-3);
        assert_eq!(list.choices[0].area[0], 152.0);
        assert_eq!(list.choices[0].area[2], 973.0);
        let first = &list.choices[0].lines;
        assert!((first[0].base - (611.8 + 15.0 + 23.3)).abs() < 1e-3);
        assert_eq!(first[0].x, 177.0);
        // The scroll bar shows because the fourth choice does not fit.
        let bar = list.scroll.unwrap();
        assert_eq!(bar.up[0], 120.0);
        assert!((bar.up[1] + 8.0 - 650.0).abs() < 1e-3);
        // The band: 30 above and below the brackets.
        assert_eq!(l.band.unwrap(), [100.0, 569.0, 1080.0, 340.0]);
    }

    #[test]
    fn a_short_list_keeps_its_least_height() {
        let l = layout(&choices(&["Goodbye."]), 0, 0, 1280.0, true, &font(34.0), &font(36.0), &no_glyphs());
        let list = l.list.unwrap();
        assert_eq!(list.area[3], 110.0);
        assert!(list.scroll.is_none());
    }

    #[test]
    fn the_window_follows_the_selection() {
        let h = [58.0; 8];
        // Five rows of 58 fit in 290.
        assert_eq!(window(0, 0, &h), (0, 5));
        assert_eq!(window(0, 4, &h), (0, 5));
        assert_eq!(window(0, 5, &h), (1, 6));
        assert_eq!(window(3, 7, &h), (3, 8));
        // Going up moves the window only when the selection leaves it.
        assert_eq!(window(3, 4, &h), (3, 8));
        assert_eq!(window(3, 2, &h), (2, 7));
        // A window that could show more above when the list got shorter.
        assert_eq!(window(6, 7, &h[..7]), (2, 7));
        // One very long choice is shown alone.
        assert_eq!(window(0, 0, &[400.0, 58.0]), (0, 1));
        assert_eq!(window(0, 0, &[]), (0, 0));
    }

    #[test]
    fn the_list_keeps_its_height_as_the_selection_scrolls() {
        // Sunny Smiles's six choices at 1280 x 720: two of two lines among them. The room
        // is the first three (250); from the second choice only three fit it again, not the
        // four that 290 would take, so the band neither grows nor moves.
        let h = [96.0, 58.0, 96.0, 58.0, 58.0, 58.0];
        assert_eq!(list_room(&h), 250.0);
        assert_eq!(window(0, 2, &h), (0, 3));
        assert_eq!(window(0, 3, &h), (1, 4));
        assert_eq!(window(1, 4, &h), (2, 5));
        assert_eq!(window(2, 5, &h), (3, 6));
        // Back up: the window moves only when the selection leaves it.
        assert_eq!(window(3, 3, &h), (3, 6));
        assert_eq!(window(3, 2, &h), (2, 5));
        // In the layout the list and the band are the same at both scrolls.
        let two = "abcde ".repeat(12);
        let v = choices(&[&two, "Do you know the robot?", &two, "Short.", "Short.", "Goodbye."]);
        let a = layout(&v, 0, 1, 1280.0, true, &font(34.0), &font(36.0), &no_glyphs());
        let b = layout(&v, 0, 3, 1280.0, true, &font(34.0), &font(36.0), &no_glyphs());
        let (la, lb) = (a.list.unwrap(), b.list.unwrap());
        assert_eq!(la.area, lb.area);
        assert_eq!(a.band, b.band);
        assert_eq!((la.choices.len(), lb.choices.len()), (3, 3));
        assert_eq!((la.choices[0].index, lb.choices[0].index), (0, 1));
        // The room under the last whole choice stays empty: the third choice shown from the
        // second ends 38 above the list's foot.
        let last = &lb.choices[2];
        assert!((la.area[1] + la.area[3]) - (last.area[1] + last.area[3]) > 30.0);
    }
}
