//! What every Pip-Boy page shares, drawn into the screen's image
//! (docs/spec-notes/pipboy.md §2): the heading (the page's title and its cards of figures)
//! and the tab line along the foot, and the tools the pages draw with. The screen around
//! them (background, scanlines) is `device_screen.rs`'s.
//! Lengths are the canvas's units (1024 × 768), from its top left corner.
//!
//! The numbers are the menu files' [documented: `menus\main\inventory_menu.xml` and the
//! pieces it includes: `card_info.xml`, `tabline.xml`, `tabline_template.xml`,
//! `super_text_box.xml`, `text_box.xml`, `box.xml`, `line.xml`,
//! `vertical_fade_line.xml`, `menus\globals.xml`] unless marked otherwise.

use super::{Clickable, Frame, Page, PageBody, PipBoy, PipBoyInput, PipBoyRoot, PipBoyScreen, CANVAS};
use crate::interface::{pipboy_amber as amber, at, face, plain_font, set_line, Face, Picture, Pictures};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use fnv_font::{Font, Line};
use nvf_assets::GameData;

/// The menus' rectangle on the canvas: 50 in and down, 855 × 650.
pub const MAIN: Rect = Rect { min: Vec2::new(50.0, 50.0), max: Vec2::new(905.0, 700.0) };
/// Every line is 2 thick on a PC.
pub const THICK: f32 = 2.0;
/// Lines are drawn at brightness 200, text at 255, a dimmed text at 128; a box's fill at
/// brightness 128 and alpha 32. The screen menu keeps these; on the device the bright text
/// is [`DEVICE_TEXT`] ([`Kit::text`]).
pub const LINE: f32 = 200.0;
pub const TEXT: f32 = 255.0;
/// On the device (`Frame::Device`) the bright text is drawn at 213, not the file's 255: on
/// the arm the original's text peaks at 1.31 times its lines' brightness where 255 gave
/// NVForever's 1.57, so its letters read heavier [observed: S.P.E.C.I.A.L. at 1280 x 720,
/// the original's heading line 128 and names 168 red (private job pipboy_fx_still),
/// NVForever's 129 and 202 (scenario pipboy-screen-text); hypothesis: why the original's
/// text comes out dimmer; the dimmed text is not measured and keeps 128]. The same value
/// tints the hand-built pages' pictures (the Vault Boy, icons, bars, map markers), and the
/// dimming holds for them: at 213 the S.P.E.C.I.A.L. Vault Boy reads 190 red at its 99th
/// percentile and 85.5 on average against the original's 191 and 80.7 (212 and 96.2 at
/// 255), the Status page's bright parts 144 at the 95th percentile against 114 (169 at
/// 255), and the bars stay at the screen's red ceiling, about 205 to 212, in all three
/// [observed: private job pipboy_fx_still; scenario pipboy-screen-text]. So it is the
/// device's drawn picture as a whole that the original shows dimmer. Only the device was
/// measured: the screen menu (Use Pip-Boy off) keeps the file's 255. The flat image a
/// Pip-Boy model without a screen falls back to shows the device's drawing, so it takes 213
/// with it [design: the canvas is drawn once per frame kind]. The pages drawn from their
/// menu files take it on the device too, for their pieces at the file's full bright level
/// (`crate::tiles::draw::device_brightness`, docs/spec-notes/pipboy.md §3).
pub const DEVICE_TEXT: f32 = 213.0;
pub const DIMMED: f32 = 128.0;
pub const FILL: (f32, f32) = (128.0, 32.0);
/// A vertical fade line is 60 long.
pub const FADE: f32 = 60.0;
/// The heading: the title (font 4) 70 in, a line of 50 before it and one from 20 after
/// it up to the first card.
const TITLE_X: f32 = 70.0;
const TITLE_GAP: f32 = 20.0;
const LEAD_LINE: f32 = 50.0;
/// A card of figures: its title 5 in, its value right-aligned 10 from its right end,
/// both 20 down, in font 2.
const CARD_TEXT_X: f32 = 5.0;
const CARD_TEXT_Y: f32 = 20.0;
const CARD_VALUE_IN: f32 = 10.0;
/// The least room kept between a card's title and its value when the value is broken
/// into lines ([`card_value_lines`]) [design, #2537].
const CARD_GAP: f32 = 10.0;
/// A button's text under the button's own y: `text_box.xml` centres the text in a box
/// `_verbuf` (15 in the menus' buttons) taller than it, then 7 + 3 lower (not glowing)
/// [documented: `text_box.xml` `button_text` y; the original's "Maintain R)" sits there
/// on the owner's ITEMS screenshot].
pub const BUTTON_TEXT_Y: f32 = 15.0 / 2.0 + 10.0;

/// One button hint of a page: its words, the key's part after them ("[R]", with a
/// controller "[Y]"), and whether it is drawn dim (it does nothing now) (#1896, #1897).
#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    pub word: &'static str,
    pub key: String,
    pub dim: bool,
}

impl Hint {
    pub fn new(word: &'static str, key: String, dim: bool) -> Hint {
        Hint { word, key, dim }
    }

    /// As drawn: "Repair [R]"; the word alone when it has no key (a button only clicked).
    pub fn text(&self) -> String {
        if self.key.is_empty() {
            return self.word.to_string();
        }
        format!("{} {}", self.word, self.key)
    }

    /// The colour it is drawn in: bright (at `text`, [`Kit::text`]), or dimmed.
    pub fn color(&self, text: f32) -> Color {
        amber(if self.dim { DIMMED } else { text })
    }
}

/// A page's hints for the log, in drawing order: "Repair [R] (dim), Equip [Enter]".
pub fn hints_line<'a>(hints: impl IntoIterator<Item = &'a Hint>) -> String {
    hints.into_iter().map(|h| if h.dim { format!("{} (dim)", h.text()) } else { h.text() }).collect::<Vec<_>>().join(", ")
}

/// A column of hints at the top right (the ITEMS page's, the weapon mod menu's, the DATA
/// page's): one row under the other 46 apart, each row's word right-aligned on one line
/// with its key just after it [observed: the original's screenshots on issue #1896, 1920 ×
/// 1080: rows 42 to 43 px apart, 0.90 of the item list's row step there (47 to 48 px); the
/// words end at the heading's right end, the button glyph beside them; the mod menu's two
/// rows 43 px apart too; the DATA page's on issue #1897 the same: rows 43 px apart, the
/// words ending within 3 px of the heading's right end] [design: 0.90 of our 51-high list
/// rows is 46].
pub const HINT_PITCH: f32 = 46.0;
/// How far right of the menus' rectangle a hint's key may reach on the canvas: the lit
/// screen ends about 49 past the heading's right end at the first hint's row and further
/// out lower down [design: measured on NVForever's own 1280 × 720 ITEMS frame].
pub const HINT_KEY_ROOM: f32 = 45.0;

/// Where a hint column's words end: at `words_right`, unless the widest key part
/// (`key_widths`: each row's drawn width less its word's) would then run past `limit`;
/// then the whole column moves left until that key ends at `limit`, the words still
/// right-aligned on one line [design: some keyboard key names (the mod menu's "Enter)")
/// are longer than a controller's buttons, which fit].
pub fn hint_words_right(words_right: f32, limit: f32, key_widths: impl IntoIterator<Item = f32>) -> f32 {
    let widest = key_widths.into_iter().fold(0.0, f32::max);
    words_right.min(limit - widest)
}
/// The tab line: 650 down, as wide as the menus; a tab is its text and 35 across, 15 up
/// and down; the lines between the tabs are as long as each other [hypothesis: the files
/// leave their length to code; it fills the line evenly, as on the owner's screenshots].
const TABLINE_Y: f32 = 650.0;
const TAB_HORBUF: f32 = 35.0;
const TAB_VERBUF: f32 = 15.0;
/// A text sits 3 under the middle of its box.
const BOX_TEXT_DROP: f32 = 3.0;

/// The fonts' files by the number the menu files give them [documented:
/// `Fallout_default.ini`, fonts.md §4].
pub const FONTS: [&str; 8] = [
    "textures\\fonts\\glow_monofonto_large.fnt",
    "textures\\fonts\\monofonto_large.fnt",
    "textures\\fonts\\glow_monofonto_medium.fnt",
    "textures\\fonts\\monofonto_verylarge02_dialogs2.fnt",
    "textures\\fonts\\fixedsys_comp_uniform_width.fnt",
    "textures\\fonts\\glow_monofonto_vl_dialogs.fnt",
    "textures\\fonts\\baked-in_monofonto_large.fnt",
    "textures\\fonts\\glow_futura_caps_large.fnt",
];

/// How far under a text's top its letters stand: half the font's size less a little, as
/// measured for font 2 (15 of 31; message-menu.md §3) [hypothesis for the other fonts].
pub fn base(font: &Font) -> f32 {
    font.size * 15.0 / 31.0
}

/// What the pages are drawn with, read when the Pip-Boy first comes up.
#[derive(Resource, Default)]
pub struct Kit {
    faces: Vec<Face>,
    pictures: Option<Pictures>,
    /// The menus are the screen menu: the pages leave out their title, the header of page
    /// names stands in its place ([`heading_title`], [`header`]).
    pub screen: bool,
}

impl Kit {
    fn load(&mut self, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) {
        if self.faces.is_empty() {
            self.faces = FONTS.iter().map(|f| face(f, plain_font(31.0, 13.0), data, images, bc)).collect();
            self.pictures = Some(Pictures::load(data));
        }
    }

    /// Font `n` (1 to 8) as the menu files number them.
    pub fn face(&self, n: usize) -> &Face {
        &self.faces[n.clamp(1, FONTS.len()) - 1]
    }

    /// The bright text's brightness: the file's 255 in the screen menu, [`DEVICE_TEXT`] on
    /// the device.
    pub fn text(&self) -> f32 {
        if self.screen { TEXT } else { DEVICE_TEXT }
    }
}

/// Where a text's letters start for a given anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Centre,
}

/// The tools of a page's drawing.
pub struct Ctx<'a> {
    pub kit: &'a mut Kit,
    pub data: Option<&'a GameData>,
    pub images: &'a mut Assets<Image>,
    pub bc: bool,
    pub glyphs: &'a crate::pad::PadGlyphs,
}

impl<'a> Ctx<'a> {
    pub fn new(kit: &'a mut Kit, data: Option<&'a GameData>, images: &'a mut Assets<Image>, formats: Option<&CompressedImageFormatSupport>, glyphs: &'a crate::pad::PadGlyphs) -> Ctx<'a> {
        let bc = crate::interface::bc_supported(formats);
        kit.load(data, images, bc);
        Ctx { kit, data, images, bc, glyphs }
    }

    /// The bright text's colour on this frame ([`Kit::text`]).
    pub fn bright(&self) -> Color {
        amber(self.kit.text())
    }

    pub fn picture(&mut self, path: &str) -> Option<Picture> {
        self.kit.pictures.as_mut()?.picture(path, self.data, self.images, self.bc)
    }

    /// A picture tinted `color`, stretched over `node`.
    pub fn image(&mut self, p: &mut ChildSpawnerCommands, path: &str, node: Node, color: Color) -> Option<Entity> {
        let (image, rect) = self.picture(path)?;
        Some(p.spawn((node, ImageNode { rect, color, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) })).id())
    }

    /// A straight line from the atlas's solid piece (or a plain fill).
    pub fn line(&mut self, p: &mut ChildSpawnerCommands, x: f32, y: f32, w: f32, h: f32, color: Color) {
        if self.image(p, "solid.dds", at(x, y, w, h), color).is_none() {
            p.spawn((at(x, y, w, h), BackgroundColor(color)));
        }
    }

    /// A line `len` long down from `y` that fades out downwards, or (`up`) up from `y`
    /// that fades out upwards.
    pub fn fade(&mut self, p: &mut ChildSpawnerCommands, x: f32, y: f32, len: f32, up: bool, color: Color) {
        let (name, top) = if up { ("fade_to_top.dds", y - len) } else { ("fade_to_bottom.dds", y) };
        if self.image(p, name, at(x, top, THICK, len), color).is_none() {
            p.spawn((at(x, top, THICK, len), BackgroundColor(color.with_alpha(0.5))));
        }
    }

    /// A box's four sides (and its fill, if any).
    pub fn boxed(&mut self, p: &mut ChildSpawnerCommands, r: Rect, line: Color, fill: Option<Color>) {
        if let Some(f) = fill {
            p.spawn((at(r.min.x, r.min.y, r.width(), r.height()), BackgroundColor(f)));
        }
        let (w, h) = (r.width(), r.height());
        self.line(p, r.min.x, r.min.y, w, THICK, line);
        self.line(p, r.min.x, r.max.y - THICK, w, THICK, line);
        self.line(p, r.min.x, r.min.y + THICK, THICK, h - 2.0 * THICK, line);
        self.line(p, r.max.x - THICK, r.min.y + THICK, THICK, h - 2.0 * THICK, line);
    }

    /// The first line of `s` in font `font`.
    pub fn first_line(&self, font: usize, s: &str) -> Line {
        self.kit.face(font).font.lines(s, None).into_iter().next().unwrap_or_default()
    }

    /// `s` in font `font`, broken at `wrap`.
    pub fn lines(&self, font: usize, s: &str, wrap: Option<f32>) -> Vec<Line> {
        self.kit.face(font).font.lines(s, wrap)
    }

    /// Width of `s` in font `font`.
    pub fn width(&self, font: usize, s: &str) -> f32 {
        self.first_line(font, s).width
    }

    /// The height of a text's line in font `font`.
    pub fn line_height(&self, font: usize) -> f32 {
        self.kit.face(font).font.size
    }

    /// Draw one line of `s` whose top is at `top`; returns its width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, p: &mut ChildSpawnerCommands, font: usize, s: &str, x: f32, top: f32, align: Align, color: Color) -> f32 {
        let face = self.kit.face(font);
        let width = crate::pad::hint_width(face, s, self.glyphs);
        let x = match align {
            Align::Left => x,
            Align::Right => x - width,
            Align::Centre => x - width / 2.0,
        };
        crate::pad::set_hint(p, face, s, x, top + base(&face.font), color, self.glyphs);
        width
    }

    /// Draw `hints` as a column ([`HINT_PITCH`], [`hint_words_right`]) in font 2: the
    /// words end at `words_right` (canvas units), the first row's text top at `top`, the
    /// keys reaching at most [`HINT_KEY_ROOM`] past the menus' rectangle. Returns each
    /// row's drawn rectangle (word and key), for its click area.
    pub fn hint_column(&mut self, p: &mut ChildSpawnerCommands, hints: &[Hint], words_right: f32, top: f32) -> Vec<Rect> {
        let rects = self.hint_column_rects(hints, words_right, top);
        for (h, r) in hints.iter().zip(&rects) {
            self.text(p, 2, &h.text(), r.min.x, r.min.y, Align::Left, h.color(self.kit.text()));
        }
        rects
    }

    /// Where [`Ctx::hint_column`] draws each row (word and key), without drawing: for a
    /// page that needs its rows' places before it draws (the DATA page's click areas).
    pub fn hint_column_rects(&self, hints: &[Hint], words_right: f32, top: f32) -> Vec<Rect> {
        let face = self.kit.face(2);
        let height = face.font.size;
        // The drawn widths, not the text's: a hint that names a button draws its picture.
        let widths: Vec<(f32, f32)> =
            hints.iter().map(|h| (crate::pad::hint_width(face, h.word, self.glyphs), crate::pad::hint_width(face, &h.text(), self.glyphs))).collect();
        let right = hint_words_right(words_right, MAIN.max.x + HINT_KEY_ROOM, widths.iter().map(|(word, all)| all - word));
        widths
            .into_iter()
            .enumerate()
            .map(|(k, (word, all))| {
                let (x, y) = (right - word, top + HINT_PITCH * k as f32);
                Rect::new(x, y, x + all, y + height)
            })
            .collect()
    }

    /// Draw a line already set, its top at `top`.
    #[allow(clippy::too_many_arguments)]
    pub fn set(&mut self, p: &mut ChildSpawnerCommands, font: usize, line: &Line, x: f32, top: f32, align: Align, color: Color) {
        let x = match align {
            Align::Left => x,
            Align::Right => x - line.width,
            Align::Centre => x - line.width / 2.0,
        };
        let face = self.kit.face(font);
        set_line(p, face, line, x, top + base(&face.font), color);
    }

    /// Draw `s` broken at `wrap`, lines `pitch` apart from `top`; returns the lines drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn paragraph(&mut self, p: &mut ChildSpawnerCommands, font: usize, s: &str, x: f32, top: f32, wrap: f32, pitch: f32, color: Color) -> usize {
        let lines = self.lines(font, s, Some(wrap));
        for (i, l) in lines.iter().enumerate() {
            self.set(p, font, l, x, top + pitch * i as f32, Align::Left, color);
        }
        lines.len()
    }
}

/// A card of figures in a page's heading: where it starts, how wide, and its title and
/// value.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub x: f32,
    pub width: f32,
    pub title: String,
    pub value: String,
}

impl Card {
    pub fn new(x: f32, width: f32, title: impl Into<String>, value: impl Into<String>) -> Card {
        Card { x, width, title: title.into(), value: value.into() }
    }
}

/// Draw a page's heading inside the menus' rectangle (whose top left is `MAIN.min`): a
/// vertical line fading down at its left, a line of 50 to the title, the title in font 4,
/// a line from 20 after the title to the first card, and the cards.
pub fn heading(p: &mut ChildSpawnerCommands, c: &mut Ctx, title: &str, cards: &[Card]) {
    let (x0, y0) = (MAIN.min.x, MAIN.min.y);
    let line = amber(LINE);
    c.fade(p, x0, y0, FADE, false, line);
    c.line(p, x0, y0, LEAD_LINE, THICK, line);
    let after = heading_title(p, c, title);
    let first = cards.first().map_or(MAIN.width(), |k| k.x);
    if x0 + first > after {
        c.line(p, after, y0, x0 + first - after, THICK, line);
    }
    for k in cards {
        card(p, c, x0 + k.x, y0, k.width, &k.title, &k.value);
    }
}

/// Where the heading's lead line ends: where the line runs on from in the screen menu, which
/// has no title ([`heading_title`]).
pub fn lead_line_end() -> f32 {
    MAIN.min.x + LEAD_LINE
}

/// A page's title in its heading (font 4, 70 in, on the heading's line); where the line
/// goes on after it. In the screen menu the title is left out, the header of page names
/// standing on the line in its place ([`header`]), and the line runs on from the lead line
/// [design: the owner, 2026-10-31, not the page's name twice].
pub fn heading_title(p: &mut ChildSpawnerCommands, c: &mut Ctx, title: &str) -> f32 {
    let (x0, y0) = (MAIN.min.x, MAIN.min.y);
    if c.kit.screen {
        return x0 + LEAD_LINE;
    }
    let top=y0-base(&c.kit.face(4).font)+THICK;
    let w = c.text(p, 4, title, x0 + TITLE_X, top, Align::Left, c.bright());
    p.spawn((at(x0+TITLE_X,top,w,c.line_height(4)),super::tab_swipe::Strip(super::tab_swipe::Kind::Sections),Pickable::IGNORE));
    x0 + TITLE_X + w + TITLE_GAP
}

/// The heading with a name (font 4) right-aligned `right_in` short of the menus' right
/// edge in place of cards, the line running from the title up to 20 before it [documented:
/// item_mod_menu.xml `IMM_ItemName`; hypothesis: the line stops at the name].
pub fn heading_named(p: &mut ChildSpawnerCommands, c: &mut Ctx, title: &str, name: &str, right_in: f32) {
    let (x0, y0) = (MAIN.min.x, MAIN.min.y);
    let line = amber(LINE);
    let bright = c.bright();
    c.fade(p, x0, y0, FADE, false, line);
    c.line(p, x0, y0, LEAD_LINE, THICK, line);
    let top = y0 - base(&c.kit.face(4).font) + THICK;
    let after = heading_title(p, c, title);
    let right = MAIN.max.x - right_in;
    let name_w = c.text(p, 4, name, right, top, Align::Right, bright);
    let stop = right - name_w - TITLE_GAP;
    if stop > after {
        c.line(p, after, y0, stop - after, THICK, line);
    }
}

/// A card's value as the lines it is drawn in, each right-aligned under the one before:
/// one line while it fits beside the title, else broken between its comma-separated parts
/// (an item's effects, "HP +5, Rads -3, FOD +41, STR -1") so that no line reaches the
/// title; a part is never cut, so one wider than the room stands on a line of its own.
/// `width` is the card's, `font` the face both are set in (issue #2537: a long effects
/// line ran over its "EFFECTS" title) [design: where it breaks; the original's
/// `card_info.xml` gives `CI_ValueText` a `wrapwidth` (441 of the effects card's 466)
/// [documented], so its value is broken into lines too; how its lines look is not
/// captured].
pub fn card_value_lines(font: &Font, title: &str, width: f32, value: &str) -> Vec<String> {
    let room = width - CARD_VALUE_IN - CARD_TEXT_X - font.width(title) - CARD_GAP;
    if value.is_empty() {
        return Vec::new();
    }
    if font.width(value) <= room {
        return vec![value.to_string()];
    }
    let parts: Vec<&str> = value.split(", ").collect();
    let mut lines: Vec<Vec<&str>> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        // A line that another follows ends with the comma between the parts.
        let comma = if i + 1 < parts.len() { "," } else { "" };
        match lines.last_mut() {
            Some(last) if font.width(&format!("{}, {part}{comma}", last.join(", "))) <= room => last.push(part),
            _ => lines.push(vec![part]),
        }
    }
    let n = lines.len();
    lines.iter().enumerate().map(|(i, l)| format!("{}{}", l.join(", "), if i + 1 < n { "," } else { "" })).collect()
}

/// A card: a line along its top, one fading down its right side, the title at the left
/// and the value right-aligned, both in font 2; a value too long for the room beside the
/// title goes on in further lines under it ([`card_value_lines`]).
pub fn card(p: &mut ChildSpawnerCommands, c: &mut Ctx, x: f32, y: f32, width: f32, title: &str, value: &str) {
    let line = amber(LINE);
    let bright = c.bright();
    c.line(p, x, y, width, THICK, line);
    c.fade(p, x + width - THICK, y, FADE, false, line);
    c.text(p, 2, title, x + CARD_TEXT_X, y + CARD_TEXT_Y, Align::Left, bright);
    let pitch = c.line_height(2);
    for (i, l) in card_value_lines(&c.kit.face(2).font, title, width, value).iter().enumerate() {
        c.text(p, 2, l, x + width - CARD_VALUE_IN, y + CARD_TEXT_Y + pitch * i as f32, Align::Right, bright);
    }
}

/// The brightness (0 to 1) of each row of a DXT1 texture's top level, read down its
/// first column; `None` for any other file.
pub fn scanline_column(dds: &[u8]) -> Option<Vec<f32>> {
    let d = fnv_dds::Dds::parse(dds).ok()?;
    if !matches!(d.format, fnv_dds::Format::Dxt1) {
        return None;
    }
    let (w, h) = (d.header.width as usize, d.header.height as usize);
    let top = d.levels.first()?.first()?;
    let blocks_across = w.div_ceil(4);
    let rgb = |c: u16| [((c >> 11) & 31) as f32 / 31.0, ((c >> 5) & 63) as f32 / 63.0, (c & 31) as f32 / 31.0];
    let mut out = Vec::with_capacity(h);
    for by in 0..h.div_ceil(4) {
        let b = top.get(by * blocks_across * 8..by * blocks_across * 8 + 8)?;
        let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
        let (p0, p1) = (rgb(c0), rgb(c1));
        let mix = |a: f32, x: f32, y: f32| x + (y - x) * a;
        let palette: [[f32; 3]; 4] = if c0 > c1 {
            [p0, p1, std::array::from_fn(|i| mix(1.0 / 3.0, p0[i], p1[i])), std::array::from_fn(|i| mix(2.0 / 3.0, p0[i], p1[i]))]
        } else {
            [p0, p1, std::array::from_fn(|i| mix(0.5, p0[i], p1[i])), [0.0; 3]]
        };
        for row in 0..4 {
            let index = (b[4 + row] & 3) as usize;
            let [r, g, bl] = palette[index];
            out.push((r + g + bl) / 3.0);
        }
    }
    out.truncate(h);
    Some(out)
}

/// Where the tab line puts its tabs: each tab's box, left to right, and the length of
/// the lines between them (and at the two ends).
pub fn tab_boxes(widths: &[f32]) -> (Vec<Rect>, f32) {
    let boxes: Vec<f32> = widths.iter().map(|w| w + TAB_HORBUF).collect();
    let gap = ((MAIN.width() - boxes.iter().sum::<f32>()) / (boxes.len() + 1) as f32).max(0.0);
    let mut x = MAIN.min.x + gap;
    let rects = boxes
        .iter()
        .map(|&w| {
            let r = Rect::new(x, TABLINE_Y - TAB_VERBUF, x + w, TABLINE_Y + TAB_VERBUF);
            x += w + gap;
            r
        })
        .collect();
    (rects, gap)
}

/// The tab line along the foot: a vertical line rising at each end, the tabs' names in
/// font 2 joined by lines at their middle, the tab shown boxed and filled.
fn tab_line(p: &mut ChildSpawnerCommands, c: &mut Ctx, page: Page, current: usize) {
    let line = amber(LINE);
    let bright = c.bright();
    let names = page.tabs();
    let text_h = c.line_height(2);
    let widths: Vec<f32> = names.iter().map(|n| c.width(2, n)).collect();
    let (boxes, gap) = tab_boxes(&widths);
    let half = (text_h + TAB_VERBUF) / 2.0;
    c.fade(p, MAIN.min.x, TABLINE_Y + THICK / 2.0, FADE, true, line);
    c.fade(p, MAIN.max.x - THICK, TABLINE_Y + THICK / 2.0, FADE, true, line);
    let mut x = MAIN.min.x;
    for (i, (name, r)) in names.iter().zip(&boxes).enumerate() {
        c.line(p, x, TABLINE_Y - THICK / 2.0, gap, THICK, line);
        x = r.max.x;
        let r = Rect::new(r.min.x, TABLINE_Y - half, r.max.x, TABLINE_Y + half);
        let selected = i == current;
        if selected {
            c.boxed(p, r, line, Some(amber(FILL.0).with_alpha(FILL.1 / 255.0)));
        } else {
            // Under the pointer another tab is boxed, unfilled [hypothesis: the
            // `text_box.xml` focus box; the original's look is not captured].
            p.spawn((at(0.0, 0.0, CANVAS.x, CANVAS.y), TabHoverBox(i), Visibility::Hidden, Pickable::IGNORE)).with_children(|b| c.boxed(b, r, line, None));
        }
        let top = r.min.y + (r.height() - text_h) / 2.0 + BOX_TEXT_DROP;
        c.text(p, 2, name, r.min.x + TAB_HORBUF / 2.0, top, Align::Left, bright);
        // The tab's box takes the pointer.
        p.spawn((at(r.min.x, r.min.y, r.width(), r.height()), Clickable(PipBoyInput::Tab(i)), super::tab_swipe::Strip(super::tab_swipe::Kind::Subtabs), Pickable::default()))
            .observe(super::on_click)
            .observe(on_tab_over)
            .observe(on_tab_out);
    }
    c.line(p, x, TABLINE_Y - THICK / 2.0, MAIN.max.x - x, THICK, line);
}

/// The box shown round tab `.0` while the pointer is on it.
#[derive(Component, Clone, Copy)]
pub struct TabHoverBox(pub usize);

fn on_tab_over(over: On<PointerOver>, nodes: Query<&Clickable>, mut pip: ResMut<PipBoy>, mut out: MessageWriter<super::sounds::TabHovered>) {
    if let Ok(Clickable(PipBoyInput::Tab(i))) = nodes.get(over.entity) {
        pip.hover_tab = Some(*i);
        out.write(super::sounds::TabHovered(*i));
    }
}

fn on_tab_out(out: On<PointerOut>, nodes: Query<&Clickable>, mut pip: ResMut<PipBoy>) {
    if let Ok(Clickable(PipBoyInput::Tab(i))) = nodes.get(out.entity) {
        if pip.hover_tab == Some(*i) {
            pip.hover_tab = None;
        }
    }
}

/// Show the hover box of the tab under the pointer (none while the Pip-Boy is down).
pub fn hover_boxes(mut pip: ResMut<PipBoy>, mut boxes: Query<(&TabHoverBox, &mut Visibility)>) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    // A page drawn from its tiles has no hand-built tabs for this to be left over from.
    if (!pip.open() || boxes.is_empty()) && pip.hover_tab.is_some() {
        pip.hover_tab = None;
    }
    for (b, mut v) in &mut boxes {
        let want = if pip.hover_tab == Some(b.0) { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

type ShownFrame = (Page, usize, Frame, bool, u32);

/// Rebuild the frame (and with it every page's body) when the Pip-Boy's page or tab
/// changed, or it came up, or a page went over to its tiles or back. A page drawn from its
/// file (`crate::tiles::stats_menu::StatsTiles::ready` or
/// `crate::tiles::inventory_menu::InventoryTiles::ready`, each the one decision) keeps the shell and
/// the screen menu's header, and has none of the hand-built page: its heading and tab line are
/// the tiles', once. The device's background and scanlines are `device_screen.rs`'s either way.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    pip: Res<PipBoy>,
    screen: Option<Res<PipBoyScreen>>,
    roots: Query<Entity, With<PipBoyRoot>>,
    mut kit: ResMut<Kit>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    tiles: Option<Res<crate::tiles::stats_menu::StatsTiles>>,
    inventory: Option<Res<crate::tiles::inventory_menu::InventoryTiles>>,
    repair_tiles: Option<Res<crate::tiles::repair_menu::RepairTiles>>,
    map_tiles: Option<Res<crate::tiles::map_menu::MapTiles>>,
    item_mod_tiles: Option<Res<crate::tiles::item_mod_menu::ItemModTiles>>,
    mut shown: Local<Option<ShownFrame>>,
) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    let Some(screen) = screen else { return };
    let tiled = tiles.is_some_and(|t| t.ready.is_some()) || inventory.is_some_and(|t| t.ready.is_some());
    // The repair page likewise (`crate::tiles::repair_menu::RepairTiles::ready`).
    let tiled = tiled || repair_tiles.is_some_and(|t| t.ready.is_some());
    // The weapon mod menu likewise (`crate::tiles::item_mod_menu::ItemModTiles::ready`).
    let tiled = tiled || item_mod_tiles.is_some_and(|t| t.ready.is_some());
    // The DATA page likewise, on the tabs it draws (`crate::tiles::map_menu::MapTiles::ready`).
    let tiled = tiled || map_tiles.is_some_and(|t| t.ready.is_some());
    let want = pip.shown().then(|| (pip.page, pip.tab(), pip.frame, tiled, crate::cjk_font::revision()));
    if want == *shown && (want.is_none() || !roots.is_empty()) {
        return;
    }
    *shown = want;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some((page, tab, frame, tiled, _)) = want else { return };
    // The screen menu is plain, as the barter menu is: the device's own screen around the
    // pages is `device_screen.rs`'s, on the device only (docs/design/screen-menu.md §2).
    let device = frame == Frame::Device;
    let mut c = Ctx::new(&mut kit, data.as_deref(), &mut images, formats.as_deref(), &glyphs);
    c.kit.screen = !device;
    let root = Node { overflow: Overflow::clip(), ..at(0.0, 0.0, CANVAS.x, CANVAS.y) };
    commands.spawn((PipBoyRoot, root, UiTargetCamera(screen.camera), Name::new("Pip-Boy menus"))).with_children(|r| {
        // The shell every view of a page keeps: the screen menu's header.
        if !device {
            header(r, &mut c, page);
        }
        if !tiled {
            hand_built_page(r, &mut c, page, tab);
        }
    });
}

/// What a hand-built page has inside the shell: the tab line and the body its module fills.
fn hand_built_page(r: &mut ChildSpawnerCommands, c: &mut Ctx, page: Page, tab: usize) {
    tab_line(r, c, page, tab);
    r.spawn((at(0.0, 0.0, CANVAS.x, CANVAS.y), PageBody { page, tab }, Pickable::IGNORE));
}

// ---- The screen menu's header (docs/design/screen-menu.md §3.1) ---------------------

/// The box shown round a page's name in the header while the pointer is on it.
#[derive(Component)]
struct HeaderHover;

/// The names are in the title's font, their boxes spaced as far apart as the title is
/// from its line [design].
const HEADER_FONT: usize = 4;
const HEADER_GAP: f32 = TITLE_GAP * 2.0;

/// Where the header puts the three names' boxes on the canvas: from the title's place,
/// standing on the heading's line (their foot on it), each as wide as its name and a
/// tab's padding and as high as a tab.
pub fn header_boxes(widths: [f32; 3], text_h: f32) -> [Rect; 3] {
    let h = text_h + TAB_VERBUF;
    let foot = MAIN.min.y + THICK;
    let mut x = MAIN.min.x + TITLE_X - TAB_HORBUF / 2.0;
    widths.map(|w| {
        let r = Rect::new(x, foot - h, x + w + TAB_HORBUF, foot);
        x = r.max.x + HEADER_GAP;
        r
    })
}

/// The screen menu's header, in place of the page's title: STATS, ITEMS and DATA on the
/// heading's line, the page shown boxed and filled as the tab shown is, the others
/// dimmed; a name under the pointer boxed; a click shows its page.
fn header(r: &mut ChildSpawnerCommands, c: &mut Ctx, page: Page) {
    let line = amber(LINE);
    let text_h = c.line_height(HEADER_FONT);
    let widths = Page::ALL.map(|p| c.width(HEADER_FONT, p.title()));
    for (p, b) in Page::ALL.into_iter().zip(header_boxes(widths, text_h)) {
        let selected = p == page;
        let local = Rect::new(0.0, 0.0, b.width(), b.height());
        // Centred in its box by the letters themselves: across by the line's width, down
        // by its tallest letter's rise (capitals, no descenders) [design: the owner,
        // 2026-10-31; the font's own baseline left the names high].
        let name = c.first_line(HEADER_FONT, p.title());
        let face = c.kit.face(HEADER_FONT);
        let rise = name.glyphs.iter().map(|g| face.font.bitmap_glyph(g).ascent).fold(0.0, f32::max);
        let (x, base) = ((b.width() - name.width) / 2.0, (b.height() + rise) / 2.0);
        r.spawn((at(b.min.x, b.min.y, b.width(), b.height()), Clickable(PipBoyInput::Page(p)), super::tab_swipe::Strip(super::tab_swipe::Kind::Sections), Pickable::default()))
            .with_children(|n| {
                if selected {
                    c.boxed(n, local, line, Some(amber(FILL.0).with_alpha(FILL.1 / 255.0)));
                } else {
                    n.spawn((at(0.0, 0.0, b.width(), b.height()), HeaderHover, Visibility::Hidden, Pickable::IGNORE)).with_children(|h| c.boxed(h, local, line, None));
                }
                set_line(n, c.kit.face(HEADER_FONT), &name, x, base, amber(if selected { c.kit.text() } else { DIMMED }));
            })
            .observe(super::on_click)
            .observe(on_header_over)
            .observe(on_header_out);
    }
}

fn header_hover(entity: Entity, on: bool, children: &Query<&Children>, boxes: &mut Query<&mut Visibility, With<HeaderHover>>) {
    for c in children.get(entity).into_iter().flatten() {
        if let Ok(mut v) = boxes.get_mut(*c) {
            *v = if on { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

fn on_header_over(over: On<PointerOver>, nodes: Query<&Clickable>, children: Query<&Children>, mut boxes: Query<&mut Visibility, With<HeaderHover>>, mut out: MessageWriter<super::sounds::TabHovered>) {
    if let Ok(Clickable(PipBoyInput::Page(p))) = nodes.get(over.entity) {
        header_hover(over.entity, true, &children, &mut boxes);
        out.write(super::sounds::TabHovered(p.index()));
    }
}

fn on_header_out(out: On<PointerOut>, children: Query<&Children>, mut boxes: Query<&mut Visibility, With<HeaderHover>>) {
    header_hover(out.entity, false, &children, &mut boxes);
}

// ---- Added for the STATS page ------------------------------------------------------

impl Ctx<'_> {
    /// Adjust only an actually selected own image. Atlas pieces retain the existing
    /// texel layout; a loose file shadowed by an atlas must not set their density.
    pub fn picture_zoom(&mut self, path: &str, logical_height: f32) -> f32 {
        let Some((image, None)) = self.picture(path) else { return 1.0 };
        self.images.get(&image).map_or(1.0, |image| {
            let height = image.size().y;
            if height == 0 { 1.0 } else { logical_height / height as f32 }
        })
    }

    /// A picture's size in its file's pixels: its atlas piece's, or the whole file's.
    pub fn picture_size(&mut self, path: &str) -> Option<Vec2> {
        let (image, rect) = self.picture(path)?;
        match rect {
            Some(r) => Some(r.size()),
            None => self.images.get(&image).map(|i| i.size().as_vec2()),
        }
    }

    /// A picture as the menu files draw an image tile: the file at `zoom` (1 = one canvas
    /// unit a pixel), shown from its top left over `w` × `h` and cut off beyond
    /// [hypothesis: an image's width and height show a part of the file and do not
    /// stretch it; `stats_menu.xml` sizes its pictures as "filewidth/fileheight already
    /// adjusted for zoom", and the Vault Boy's pieces fill exactly the parts of their
    /// files that the file gives as their sizes]. Returns the size drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn image_cut(&mut self, p: &mut ChildSpawnerCommands, path: &str, x: f32, y: f32, w: f32, h: f32, zoom: f32, color: Color) -> Option<Vec2> {
        let (image, rect) = self.picture(path)?;
        let full = match rect {
            Some(r) => r,
            None => Rect::from_corners(Vec2::ZERO, self.images.get(&image)?.size().as_vec2()),
        };
        let (cut, size) = cut_picture(full, w, h, zoom);
        p.spawn((at(x, y, size.x, size.y), ImageNode { rect: Some(cut), color, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) }));
        Some(size)
    }
}

fn cut_picture(full: Rect, w: f32, h: f32, zoom: f32) -> (Rect, Vec2) {
    let cut = Rect::new(full.min.x, full.min.y, (full.min.x + w / zoom).min(full.max.x), (full.min.y + h / zoom).min(full.max.y));
    (cut, cut.size() * zoom)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #2537: an aid item's Hardcore effects ran over the card's "EFFECTS" title.
    /// Every character 13 wide: the effects card (466) has room for 26 beside its title.
    #[test]
    fn a_long_card_value_breaks_between_its_parts() {
        let font = plain_font(31.0, 13.0);
        let lines = |width: f32, value: &str| card_value_lines(&font, "EFFECTS", width, value);
        assert_eq!(lines(466.0, "HP +40"), ["HP +40"]);
        assert_eq!(lines(466.0, "HP +5, Rads -3, FOD +41, STR -1"), ["HP +5, Rads -3, FOD +41,", "STR -1"]);
        // Just fitting stays one line; one character more breaks before the last part.
        assert_eq!(lines(466.0, "HP +5, Rads -3, FOD +4, S1"), ["HP +5, Rads -3, FOD +4, S1"]);
        assert_eq!(lines(466.0, "HP +5, Rads -3, FOD +41, S1"), ["HP +5, Rads -3, FOD +41,", "S1"]);
        // A part wider than the room is not cut: a line of its own.
        assert_eq!(lines(150.0, "Speech +2, Melee Weap. +2"), ["Speech +2,", "Melee Weap. +2"]);
        assert!(lines(466.0, "").is_empty());
        // No line reaches the title.
        let room = 466.0 - CARD_VALUE_IN - CARD_TEXT_X - font.width("EFFECTS") - CARD_GAP;
        for l in lines(466.0, "HP +5, Rads -3, FOD +41, STR -1, H2O +10, AGL -2, END +1") {
            assert!(font.width(&l) <= room, "{l}");
        }
    }

    #[test]
    fn paperdoll_hd_cut_keeps_logical_size_and_samples_the_same_art() {
        // Authored atlas rectangles with the same normalized art at 1x and 4x.
        let native = Rect::new(128.0, 256.0, 255.0, 383.0);
        let hd = Rect::new(512.0, 1024.0, 1020.0, 1532.0);
        let (a, sa) = cut_picture(native, 85.0 * 1.45, 92.0 * 1.45, 1.45);
        let (b, sb) = cut_picture(hd, 85.0 * 1.45, 92.0 * 1.45, 1.45 / 4.0);
        assert!((sa - sb).length() < 0.001);
        assert!((b.min - a.min * 4.0).length() < 0.001);
        assert!((b.max - a.max * 4.0).length() < 0.001);
        // The old 1px/unit zoom on HD samples only a quarter of each intended edge.
        let (wrong, _) = cut_picture(hd, 85.0 * 1.45, 92.0 * 1.45, 1.45);
        assert!((wrong.width() * 4.0 - b.width()).abs() < 0.001);
    }

    #[test]
    fn paperdoll_density_adjustment_retains_partial_piece_clipping_and_native_rectangles() {
        let native = Rect::new(9.0, 7.0, 29.0, 17.0);
        let hd = Rect::new(36.0, 28.0, 116.0, 68.0);
        let (a, sa) = cut_picture(native, 50.0, 50.0, 1.0);
        let (b, sb) = cut_picture(hd, 50.0, 50.0, 0.25);
        assert_eq!(a, native);
        assert_eq!(b, hd);
        assert_eq!(sa, Vec2::new(20.0, 10.0));
        assert_eq!(sb, sa);
    }

    /// A hint column's words end at the heading's right end while every key fits in the
    /// room past it (a controller's short keys); a longer key (the keyboard's "Enter)")
    /// moves the whole column left until it ends at the limit (#1896).
    #[test]
    fn a_hint_column_moves_left_only_for_a_key_that_does_not_fit() {
        let (right, limit) = (MAIN.max.x, MAIN.max.x + HINT_KEY_ROOM);
        assert_eq!(hint_words_right(right, limit, [30.0, 40.0, HINT_KEY_ROOM]), right);
        assert_eq!(hint_words_right(right, limit, [40.0, 160.0, 60.0]), limit - 160.0);
        assert_eq!(hint_words_right(right, limit, []), right);
        // The row pitch: 0.90 of the item list's 51-high rows, as the original's.
        assert!((HINT_PITCH / super::super::items::ROW_H - 0.9).abs() < 0.01);
    }

    #[test]
    fn a_dxt1_column_is_read_row_by_row() {
        // A 4 × 8 DXT1 file: two blocks, white over black, and black over white.
        let mut f = vec![0u8; 128];
        f[0..4].copy_from_slice(b"DDS ");
        f[4..8].copy_from_slice(&124u32.to_le_bytes());
        f[8..12].copy_from_slice(&0x1007u32.to_le_bytes());
        f[12..16].copy_from_slice(&8u32.to_le_bytes());
        f[16..20].copy_from_slice(&4u32.to_le_bytes());
        f[76..80].copy_from_slice(&32u32.to_le_bytes());
        f[80..84].copy_from_slice(&4u32.to_le_bytes());
        f[84..88].copy_from_slice(b"DXT1");
        // Block 1: c0 white, c1 black; rows 0-1 index 0 (white), rows 2-3 index 1.
        f.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x55, 0x55]);
        // Block 2: every texel index 2 (a third of the way to black).
        f.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00, 0xAA, 0xAA, 0xAA, 0xAA]);
        let col = scanline_column(&f).expect("read");
        assert_eq!(col.len(), 8);
        assert_eq!(&col[..4], &[1.0, 1.0, 0.0, 0.0]);
        assert!(col[4..].iter().all(|&l| (l - 2.0 / 3.0).abs() < 1e-5));
        assert_eq!(scanline_column(b"not a dds"), None);
    }

    #[test]
    fn the_header_names_stand_on_the_heading_line() {
        // Widths about the title font's for the three names.
        let boxes = header_boxes([110.0, 110.0, 90.0], 34.0);
        for w in boxes.windows(2) {
            assert!(w[1].min.x - w[0].max.x >= HEADER_GAP - 1e-3, "{boxes:?}");
        }
        for r in boxes {
            // On the line, inside the canvas's top margin (shown by the screen menu).
            assert!((r.max.y - (MAIN.min.y + THICK)).abs() < 1e-3 && r.min.y >= 0.0, "{r:?}");
        }
        // From where the title stands, ending before the right half of the menus.
        assert!((boxes[0].min.x - (MAIN.min.x + TITLE_X - TAB_HORBUF / 2.0)).abs() < 1e-3);
        assert!(boxes[2].max.x < MAIN.min.x + MAIN.width() * 0.75);
    }

    #[test]
    fn the_tabs_share_the_line_evenly() {
        let (boxes, gap) = tab_boxes(&[100.0, 100.0, 100.0, 100.0, 100.0]);
        // 855 less five tabs of 135, over six gaps.
        assert!((gap - (855.0 - 675.0) / 6.0).abs() < 1e-4);
        assert!((boxes[0].min.x - (MAIN.min.x + gap)).abs() < 1e-4);
        assert!((MAIN.max.x - boxes[4].max.x - gap).abs() < 1e-3);
        for w in boxes.windows(2) {
            assert!((w[1].min.x - w[0].max.x - gap).abs() < 1e-4);
        }
        // Tabs too wide for the line touch.
        let (_, gap) = tab_boxes(&[300.0; 5]);
        assert_eq!(gap, 0.0);
    }
}
