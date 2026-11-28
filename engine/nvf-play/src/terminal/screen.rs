//! The terminal's screen drawn as the original sets it out (docs/spec-notes/hacking.md
//! §2, §7): the hacking game by `menus\hacking_menu.xml`, the terminal's own menu by
//! `menus\computers_menu.xml`, both in the game's font 5 (`fixedsys_comp_uniform_width`,
//! 17 units a character). [design] A derived dark undercoat and tinted Pip-Boy image
//! retain the glass's texture/vignette beneath the measured foreground colours (§2); the
//! text has the original's soft edges ([`soften`]) and scanlines cross the whole canvas
//! ([`scanline_image`]). The monitor keeps its glare sheet at reduced strength
//! (`terminal_view.rs`, §7).
//!
//! The original draws this into a texture shown on a monitor model
//! (`meshes\terminals\terminalinterface01.nif`, `[RenderedTerminal]` in
//! `Fallout_default.ini`), and so does NVForever: the canvas is drawn into
//! `super::TerminalImage` and the engine puts that on the model's screen (nvforever
//! `terminal_view.rs`), where the texture spans [`TEXTURE_UNITS`] of the canvas. Without
//! the model the picture is drawn flat over the darkened world [design, hacking.md §7].
//! Lengths are the interface's units (960 to the view's height): the owner's screenshots
//! show the screen at almost that scale (0.99).

use super::{Hacking, MenuView, Page, Session};
use crate::interface::{at, face, line_anchor, plain_font, set_line, set_marked, texture, u, Face};
use bevy::prelude::*;
use fnv_font::{Font, Line};
use fnv_sim::FormId;
use fnv_game::data::NoteDef;
use fnv_game::hacking::{Puzzle, Target, ROWS, ROW_CHARS};
use nvf_assets::GameData;

// ---- the canvas ------------------------------------------------------------------------

/// The screen: four by three [documented: computers_menu.xml, "screen height × 0.75" and
/// four thirds of that wide].
pub const CANVAS: Vec2 = Vec2::new(960.0, 720.0);
/// Its top, down from the view's, when drawn flat [observed: the owner's screenshots put the first row of
/// the dump 204 units under the screen's top at 0.74 pixels a unit, so the screen starts
/// 62 of 720 pixels down; `fRenderedTerminalVPos` 0.38 moves it up from the middle].
pub const CANVAS_TOP: f32 = 83.0;

/// The text, the blocks and the lit cells. The menus' colour is the terminal colour
/// [documented: Fallout_default.ini `iSystemColorTerminal*` 33, 231, 121; `systemcolor`
/// `&terminal;`]. [observed: hacking.md §2, the original-game JPEG, 2026-11-01] The
/// attempt blocks show (61, 253, 152) (picked on screen; their brightest rows (61-62,
/// 255, 149-151)), the monitor's glare included. [observed: in game, 1280 × 720,
/// 2026-11-01] Our glare adds about (6, 0, 2) display units there: drawn (63, 255, 152),
/// shown (69, 255, 154). [design] So the canvas draws the shown colour less the glare's
/// add in linear light (red 0.0098, blue 0.0092, green taken as red's × 1.14 / 1.19 by
/// `GLARE_LINEAR_SCALE`, terminal_view.rs): (54, 252, 150). Flat, without the glare, the
/// blocks show it as is.
pub fn bright() -> Color {
    compose_screen(Color::srgb_u8(54, 252, 150), None)
}

/// An entry already read, under the Grey Read Notes tweak: the text's green at about half
/// its strength [design: docs/design/tweaks.md].
pub fn read_green() -> Color {
    compose_screen(Color::srgb_u8(27, 126, 75), None)
}

/// [design] The solid fills (blocks, lit cells, the cursor, the separator, the chosen
/// row's box) lie above the scanlines, and the text on them above the fills: on the
/// original the fills keep their green through the scanlines' troughs (253 to 255 while
/// red and blue dim by about 15%) and read solid [observed: hacking.md section 2].
/// [hypothesis] Green is clipped there. [design] Drawing above the lines approximates
/// that solid look: our fills also lose the observed red/blue dip; all three channels
/// stay constant.
const OVER_LINES: GlobalZIndex = GlobalZIndex(1);
const ON_FILLS: GlobalZIndex = GlobalZIndex(2);

/// [design] The middle of the glass beneath the monitor's glare: the original's middle
/// (21, 44, 26) [observed: §2, the 2026-10-31 reference] less what the glare's dark
/// texels add there in linear light, a few units (`terminal_view.rs`, §7); in game the
/// glass alone read (13, 35, 17) in the middle and (4, 8, 2) in the corners [observed:
/// 2026-10-31, the glare switched off]. Tuning on hacking.png away from ink and
/// streak: add (target - measured) sRGB units to the channel; the derived undercoat
/// follows, with the texture intact.
const GLASS_MIDDLE: Color = Color::srgb_u8(12, 38, 20);
/// [observed: private background DDS, centre texel (256,256), 2026-10-31]
/// RGB (76,77,76), alpha 255. A representative sample, not the whole image's colour.
const BACKGROUND_MIDDLE: Color = Color::srgb_u8(76, 77, 76);
/// [design] Main's 0.4 again: the image carries the vignette over a nearly black undercoat.
/// Tuning with the middle held fixed: +0.0056 per excess green unit in a dark corner
/// (near 7) darkens that corner by about one unit; reverse for a corner too dark.
const BACKGROUND_ALPHA: f32 = 0.4;
/// [design] Fit so that the image carries the vignette: with this tint the derived
/// undercoat is nearly black (sRGB 4, 10, 2) and the image's corner texel composes to
/// (3, 8, 2), the original's corner (8, 11, 5) less the glare's few units there.
/// Independent of the foreground's saturated green tint. Keep fixed when tuning alpha;
/// per-channel tint tuning is only needed for the corner's hue.
const BACKGROUND_TINT: Color = Color::srgb_u8(90, 202, 131);

/// [design] The ImageNode's texture × tint, including both alpha factors, in linear light.
fn background_layer(texel: Color) -> Color {
    let p = texel.to_linear();
    let t = BACKGROUND_TINT.to_linear();
    Color::linear_rgba(p.red * t.red, p.green * t.green, p.blue * t.blue, p.alpha * BACKGROUND_ALPHA)
}

/// [design] Solve middle = background × alpha + undercoat × (1-alpha).
/// Using the measured middle as the undercoat would brighten the glass twice (§2/§7).
pub fn screen_color() -> Color {
    let middle = GLASS_MIDDLE.to_linear();
    let layer = background_layer(BACKGROUND_MIDDLE).to_linear();
    let behind = 1.0 - layer.alpha;
    Color::linear_rgb((middle.red - layer.red * layer.alpha) / behind, (middle.green - layer.green * layer.alpha) / behind, (middle.blue - layer.blue * layer.alpha) / behind)
}

/// [design] Pure mirror of the UI layers: undercoat, texture × tint, then foreground
/// coverage, in linear light on the sRGB render target. No texture: middle-colour fallback.
/// The result is opaque in both display modes. The monitor glare's screen-space effect
/// still needs a running-game measurement; it is not predicted here (terminal_view.rs).
pub fn compose_screen(foreground: Color, background_texel: Option<Color>) -> Color {
    let glass = background_texel.map_or(GLASS_MIDDLE, |texel| over(background_layer(texel), screen_color()));
    over(foreground, glass)
}

/// [design] Source-over onto an opaque destination, without a second colour transform.
fn over(foreground: Color, background: Color) -> Color {
    let f = foreground.to_linear();
    let b = background.to_linear();
    let behind = 1.0 - f.alpha;
    Color::linear_rgba(f.red * f.alpha + b.red * behind, f.green * f.alpha + b.green * behind, f.blue * f.alpha + b.blue * behind, 1.0)
}

/// Text on a lit-up cell [documented: the files' "local colour" 0, 5, 0 for it].
pub fn lit_text() -> Color {
    Color::srgb(0.0, 5.0 / 255.0, 0.0)
}

/// How much of the canvas the screen's texture spans, 0 to 1 each way, interface units
/// [observed: the screen shape's texture coordinates run to 0.767 across and 0.755 down
/// (`screen:0`); fitted to the owner's screenshot, where the dump's rows are 0.76 pixels
/// a unit apart at 720 high and its columns 0.73, the header's top 0.08 of the glass
/// down and the dump's last row's foot 0.95; the case hides the glass's edge]. The
/// canvas's right edge falls inside the glass, its bottom (where nothing is drawn) about
/// at the glass's foot.
pub const TEXTURE_UNITS: Vec2 = Vec2::new(1350.0, 960.0);

/// How dark the world is behind the monitor [design: the owner's screenshot shows the
/// room dim and blurred; the blur is not drawn], and behind the flat screen.
const SHADE: f32 = 0.4;
const SHADE_FLAT: f32 = 0.88;

// ---- the hacking menu's numbers [documented: menus\hacking_menu.xml] --------------------

/// The area everything is set in (`hacking_depth_rect`): its corner on the screen.
pub const DEPTH: Vec2 = Vec2::new(32.0, 48.0);
pub const DEPTH_SIZE: Vec2 = Vec2::new(920.0, 630.0);
/// The header above the dump.
pub const HEADER_HEIGHT: f32 = 150.0;
/// Rows of the dump and the log are this high; a character this wide.
pub const ROW: f32 = 28.0;
pub const CHAR: f32 = 17.0;
/// The second column starts here; a column is 19 characters (address, space, 12).
pub const COLUMN_2: f32 = 342.0;
pub const COLUMN_CHARS: f32 = 19.0;
/// Where a row's text starts in its row.
pub const ROW_TEXT_Y: f32 = 6.0;
/// The log starts right of the dump; its prompt 17 in.
pub const LOG_X: f32 = COLUMN_2 + COLUMN_CHARS * CHAR;
pub const PROMPT_X: f32 = 17.0;
/// The entry after the prompt's `>`.
pub const ENTRY_X: f32 = PROMPT_X + 21.0;
pub const FILE_HEIGHT: f32 = ROWS as f32 * ROW;
/// The attempt blocks and the cursor.
pub const BLOCK: f32 = 17.0;
/// The cursor blinks this often [documented: `_blink_interval` 400 ms].
pub const BLINK_SECONDS: f32 = 0.4;

/// The header's lines, down from the header's top [observed: the screenshots, 1, 32 and
/// 93 units: rows 0, 1 and 3 at 31 apart].
pub const HEADER_LINES: [f32; 3] = [0.0, 31.0, 93.0];

/// The baseline under a text's top [design: font 5's capitals stand 18 above it, its
/// tallest characters 21, in a 25-unit line].
pub const BASE: f32 = 20.0;

// ---- the terminal menu's numbers [documented: menus\computers_menu.xml] -----------------

pub const MENU_HEADER_Y: f32 = 60.0;
/// A line of font 5 [documented: the font's own line height, 25].
pub const LINE: f32 = 25.0;
pub const MENU_X: f32 = 100.0;
pub const WELCOME_WRAP: f32 = 600.0;
pub const LIST_WIDTH: f32 = 700.0;
pub const LIST_ROWS: usize = 12;
/// A row of the list [hypothesis: the shared list template's height is not in the file;
/// the screenshot shows one item only].
pub const LIST_ROW: f32 = 30.0;
pub const NOTE_LINES: usize = 16;

/// What the screen drew that the input reads back: the menu's rows on the canvas (for the
/// pointer) and an open note's window (for paging it, #3409).
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Drawn {
    pub rows: Vec<(usize, Rect)>,
    pub note: Option<NoteWindow>,
}

/// The lines of an open note on the screen: `shown` of its `total` from line `first`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteWindow {
    pub note: FormId,
    pub first: usize,
    pub shown: usize,
    pub total: usize,
}

impl NoteWindow {
    /// The scroll that shows the note's last lines (the most it scrolls).
    pub fn last_first(&self) -> usize {
        self.total.saturating_sub(self.shown)
    }

    /// The scroll of the next page: the line after the last one shown, kept to
    /// [`NoteWindow::last_first`]; `None` on the last page.
    pub fn next_page(&self) -> Option<usize> {
        (self.first + self.shown < self.total).then(|| (self.first + self.shown).min(self.last_first()))
    }
}
/// The result's prompt: 30 above the bottom; its text 34 right of the prompt.
pub const RESULT_GAP: f32 = 30.0;
pub const RESULT_X: f32 = 34.0;

/// The words on the screens that are not in the plugins or the menu files (hacking.md
/// §8). All are NVForever's own (LEGAL.md); only where each line stands follows the
/// original [observed: the owner's screenshots]. The lockout screen's lines are not
/// here: they come from the game strings table (`sHackingLockout3`/`sHackingLockout4`).
pub mod words {
    pub const HACK_TITLE: &str = "TERMINAL ACCESS PROTOCOL";
    /// The header's second line.
    pub const HACK_PROMPT: &str = "PASSWORD REQUIRED";
    /// [hypothesis] In place of the prompt on the last attempt.
    pub const HACK_WARNING: &str = "LAST TRY BEFORE LOCKOUT";
    pub const MENU_TITLE: &str = "TERMINAL OPERATING SYSTEM";
    pub const MENU_COPYRIGHT: &str = "VERSION 1.0";

    pub fn attempts(n: u8) -> String {
        format!("{n} TRIES REMAINING")
    }
}

/// The left edge and the top of a dump character, in the depth area.
pub fn cell_at(index: usize) -> Vec2 {
    let column = index / (ROWS * ROW_CHARS);
    let row = (index / ROW_CHARS) % ROWS;
    let col = index % ROW_CHARS;
    Vec2::new(column as f32 * COLUMN_2 + (7 + col) as f32 * CHAR, HEADER_HEIGHT + row as f32 * ROW)
}

/// The dump character under a point of the canvas.
pub fn canvas_index(p: Vec2) -> Option<usize> {
    index_at(p - DEPTH - Vec2::new(0.0, HEADER_HEIGHT))
}

/// The dump character under a point of the dump area (`(0, 0)` its top left, which is
/// the depth area's left edge and the header's bottom).
pub fn index_at(p: Vec2) -> Option<usize> {
    if p.y < 0.0 || p.y >= FILE_HEIGHT {
        return None;
    }
    let row = (p.y / ROW) as usize;
    for column in 0..2 {
        let x = p.x - column as f32 * COLUMN_2 - 7.0 * CHAR;
        if (0.0..ROW_CHARS as f32 * CHAR).contains(&x) {
            return Some(Puzzle::index(column, row, (x / CHAR) as usize));
        }
    }
    None
}

/// The log's lines shown, newest last: as many as stand above the prompt, a blank row
/// between [documented: entries stand 3 + n rows up from the bottom and are hidden once
/// above the top; observed: one blank row on the screenshots].
pub fn shown_log(log: &[String]) -> &[String] {
    let room = ROWS - 2;
    &log[log.len().saturating_sub(room)..]
}

// ---- drawing -----------------------------------------------------------------------------

/// What the screens are drawn with.
pub struct Faces {
    /// Font 5, with soft edges when its texture could be read ([`soften`]).
    pub text: Face,
    /// The same glyphs for the dark text on the lit cells, their edges bent for dark over
    /// the fill rather than ink over the glass.
    pub lit: Face,
    pub background: Option<Handle<Image>>,
    /// The scanlines over the whole canvas ([`scanline_image`]).
    pub scanlines: Option<Handle<Image>>,
    /// How much of the glass's light the scanlines keep on average, in linear light (1
    /// without them); the undercoat is raised by it so that the glass still averages
    /// [`GLASS_MIDDLE`].
    pub scan_kept: f32,
}

const FONT: &str = "textures\\fonts\\fixedsys_comp_uniform_width.fnt";
// [documented: hacking_menu.xml] The hacking menu's own background, not the model's
// pipboy3000 stand-in; use the same loaded image for the terminal's other pages.
const BACKGROUND: &str = "textures\\interface\\shared\\background\\pipboy.dds";

pub fn load_faces(data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Faces {
    let soft = data.and_then(|d| soft_faces(FONT, d, images));
    if data.is_some() && soft.is_none() {
        warn!("terminal: {FONT} drawn with hard edges (its texture could not be softened)");
    }
    let (text, lit) = soft.unwrap_or_else(|| {
        let hard = face(FONT, plain_font(25.0, CHAR), data, images, bc);
        let lit = Face { font: hard.font.clone(), image: hard.image.clone(), vector: hard.vector.clone() };
        (hard, lit)
    });
    let (scanlines, scan_kept) = data.and_then(|d| scanline_image(d, images)).map_or((None, 1.0), |(image, kept)| (Some(image), kept));
    Faces { text, lit, background: data.and_then(|d| texture(BACKGROUND, d, images, bc)).map(|t| t.0), scanlines, scan_kept }
}

// ---- the original's soft strokes and scanlines (hacking.md §2) ---------------------------

/// [documented: Fallout_default.ini `bDoRenderedTerminalScanlines=1`] The only scanline
/// picture in the archives [observed]: one soft dark band, white at its top to about 0.8
/// (pipboy/frame.rs).
const SCANLINES: &str = "textures\\pipboy3000\\pipboyscanlines.dds";
/// [hypothesis: `fRenderedTerminalScanlineScale=130` lines down the screen's texture,
/// [`TEXTURE_UNITS`] high] The original-game JPEG shows them 7.33 units apart, 131 down
/// the texture [observed: §2].
pub const SCANLINE_PITCH: f32 = TEXTURE_UNITS.y / 130.0;
/// [design] Room round each glyph in the soft copy of the font, at least the blur's reach.
const SOFT_PAD: usize = 3;
/// [design] The blur's spread in texels, which are canvas units (fnv-font): with it and
/// the text in the terminal colour, the original's header lines give off 1.01 and 1.00
/// of our light in display values; 0.6 and 0.7 left the strokes harder than the
/// original's (§2).
const SOFT_SIGMA: f32 = 0.8;

/// sRGB display value (0 to 1) to linear light.
fn linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// [design] The coverage that, blended in linear light, gives what blending the display
/// values `ink` over `glass` (green, 0 to 1) at `a` would: the original's soft edges
/// are dark shoulders, not a glow (§2).
pub fn display_blend(a: f32, ink: f32, glass: f32) -> f32 {
    let (li, lg) = (linear(ink), linear(glass));
    if (li - lg).abs() < 1e-6 {
        return a;
    }
    ((linear(a * ink + (1.0 - a) * glass) - lg) / (li - lg)).clamp(0.0, 1.0)
}

/// A font's glyphs in a texture of their own, with soft edges.
pub struct Soft {
    pub font: Font,
    /// White, the coverage in alpha; `width` × `height`.
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

/// [design] Each drawn glyph of `font` (its texture's coverage `alpha`, `w` × `h`) in a
/// cell of its own with [`SOFT_PAD`] texels round it, the coverage blurred by a Gaussian
/// of `sigma` texels and passed through `bend`. The glyphs grow by the room on every side
/// and keep their advances, so the letters stand where they did and lines keep their
/// widths. `None` when a glyph lies outside the texture or will not fit.
pub fn soften(font: &Font, alpha: &[f32], w: usize, h: usize, sigma: f32, bend: impl Fn(f32) -> f32) -> Option<Soft> {
    const ACROSS: usize = 512;
    let pad = SOFT_PAD;
    if alpha.len() != w * h {
        return None;
    }
    let mut cells = Vec::new();
    let (mut x, mut y, mut row) = (0, 0, 0);
    for (code, g) in font.glyphs.iter().enumerate() {
        if !g.drawn() {
            continue;
        }
        // [design] Refuse malformed replacement-font UVs before rounding can hide
        // an out-of-range edge (or turn NaN into an integer). Keep the hard fallback.
        if !g.uv.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) || g.uv[0] >= g.uv[2] || g.uv[1] >= g.uv[3] {
            return None;
        }
        // [design] Fractional UV boundaries use the nearest source texel edge.
        let at = |v: f32, n: usize| (v * n as f32).round() as usize;
        let (x0, y0, x1, y1) = (at(g.uv[0], w), at(g.uv[1], h), at(g.uv[2], w), at(g.uv[3], h));
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let (cw, ch) = (x1 - x0 + 2 * pad, y1 - y0 + 2 * pad);
        if cw > ACROSS {
            return None;
        }
        if x + cw > ACROSS {
            (x, y, row) = (0, y + row, 0);
        }
        cells.push((code, x0, y0, x1 - x0, y1 - y0, x, y));
        x += cw;
        row = row.max(ch);
    }
    let (width, height) = (ACROSS, (y + row).max(1));
    let mut cover = vec![0.0f32; width * height];
    for &(_, x0, y0, gw, gh, dx, dy) in &cells {
        for yy in 0..gh {
            let from = (y0 + yy) * w + x0;
            let to = (dy + pad + yy) * width + dx + pad;
            cover[to..to + gw].copy_from_slice(&alpha[from..from + gw]);
        }
    }
    blur(&mut cover, width, height, sigma, pad);
    let rgba = cover.iter().flat_map(|&a| [255, 255, 255, (bend(a.clamp(0.0, 1.0)).clamp(0.0, 1.0) * 255.0).round() as u8]).collect();
    let mut out = font.clone();
    let (wf, hf) = (width as f32, height as f32);
    for &(code, _, _, gw, gh, dx, dy) in &cells {
        let g = &mut out.glyphs[code];
        let (px, py) = (pad as f32 * g.width / gw as f32, pad as f32 * g.height / gh as f32);
        g.uv = [dx as f32 / wf, dy as f32 / hf, (dx + gw + 2 * pad) as f32 / wf, (dy + gh + 2 * pad) as f32 / hf];
        g.width += 2.0 * px;
        g.height += 2.0 * py;
        g.before -= px;
        g.after -= px;
        g.ascent += py;
        // [design] The rect includes the padding: before/ascent move its origin back
        // by the same amount. No extra UV offset is needed for line_anchor's shared
        // sampling phase (fonts.md section 3.2); the original ink stays inside this box.
    }
    Some(Soft { font: out, rgba, width, height })
}

/// A Gaussian blur of `sigma`, reaching at most `reach` texels; outside is empty.
fn blur(a: &mut [f32], w: usize, h: usize, sigma: f32, reach: usize) {
    if sigma <= 0.0 {
        return;
    }
    let r = ((3.0 * sigma).ceil() as usize).min(reach) as isize;
    let mut k: Vec<f32> = (-r..=r).map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= sum);
    let tap = |n: usize, at: usize, i: isize| usize::try_from(at as isize + i).ok().filter(|&v| v < n);
    let mut tmp = vec![0.0f32; a.len()];
    for y in 0..h {
        for x in 0..w {
            tmp[y * w + x] = (-r..=r).filter_map(|i| tap(w, x, i).map(|xx| a[y * w + xx] * k[(i + r) as usize])).sum();
        }
    }
    for y in 0..h {
        for x in 0..w {
            a[y * w + x] = (-r..=r).filter_map(|i| tap(h, y, i).map(|yy| tmp[yy * w + x] * k[(i + r) as usize])).sum();
        }
    }
}

/// [design] Font 5 with soft edges: its texture's top level read here, the glyphs softened
/// ([`soften`]) and bent ([`display_blend`]) for the ink over the glass, and again for the
/// dark text over a lit cell; the two share one layout.
fn soft_faces(path: &str, data: &GameData, images: &mut Assets<Image>) -> Option<(Face, Face)> {
    let font = Font::parse(&data.read(&crate::interface::font_file(path))?).ok()?;
    // The `.tex` before the `.dds`, as every font is drawn (interface::font_tex).
    let (w, h, rgba) = match crate::interface::font_tex(&font, data) {
        Some(t) => (t.width, t.height, t.rgba),
        None => {
            let bytes = data.read(&font.texture_path())?;
            let dds = fnv_dds::Dds::parse(&bytes).ok()?;
            let (w, h) = (dds.header.width, dds.header.height);
            let top = dds.levels.first()?.first()?;
            (w, h, fnv_dds::to_rgba8(dds.format, top).or_else(|| fnv_dds::decode_blocks(dds.format, w, h, top))?)
        }
    };
    let alpha: Vec<f32> = rgba.iter().skip(3).step_by(4).map(|&a| a as f32 / 255.0).collect();
    let mut bent = |ink: Color, under: Color| {
        let (ink, under) = (ink.to_srgba().green, under.to_srgba().green);
        let soft = soften(&font, &alpha, w as usize, h as usize, SOFT_SIGMA, |a| display_blend(a, ink, under))?;
        let format = bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;
        let mut image = nvf_assets::texture::checked_image(soft.width as u32, soft.height as u32, 1, format, soft.rgba).map_err(|e| warn!("terminal: {path}: {e}")).ok()?;
        image.asset_usage = bevy::asset::RenderAssetUsages::default();
        image.sampler = bevy::image::ImageSampler::linear();
        let size = Vec2::new(soft.width as f32, soft.height as f32);
        Some(Face { font: soft.font, image: Some((images.add(image), size)), vector: None })
    };
    let text = bent(bright(), GLASS_MIDDLE)?;
    let lit = bent(lit_text(), bright())?;
    Some((text, lit))
}

/// How much light a scanline texel `l` (0 to 1) keeps, in linear light: [design] it
/// approximately scales the display values. [observed: hacking.md section 2] The fit
/// was calibrated on the block dip (blue about 0.82 at 720 high; ours 0.81 in the
/// calibration run, before the fills moved above the lines; scaling
/// the light instead gave 0.94) and the glass banding. [design] Multiplying linear(C)
/// by linear(l) is darker than C * l in display values for dark tones: glass (12,38,20)
/// at 0.8 gives about (7,28,14), not (10,30,16).
fn scan_kept(l: f32) -> f32 {
    linear(l)
}

/// [design] The scanlines as one column, a value per canvas row: the light the band
/// keeps there at `pitch`, read with linear filtering. One stretched picture, not a tile
/// per line.
pub fn scanline_column(band: &[f32], rows: usize, pitch: f32) -> Vec<f32> {
    let n = band.len() as isize;
    (0..rows)
        .map(|y| {
            let t = ((y as f32 + 0.5) / pitch).fract() * n as f32 - 0.5;
            let i = t.floor();
            let f = t - i;
            let at = |k: f32| band[(k as isize).rem_euclid(n) as usize];
            scan_kept(at(i) * (1.0 - f) + at(i + 1.0) * f)
        })
        .collect()
}

/// The scanlines' picture: black, its alpha the light taken away per canvas row; with the
/// light it keeps on average.
fn scanline_image(data: &GameData, images: &mut Assets<Image>) -> Option<(Handle<Image>, f32)> {
    let band = crate::pipboy::frame::scanline_column(&data.read(SCANLINES)?)?;
    if band.is_empty() {
        return None;
    }
    let column = scanline_column(&band, CANVAS.y as usize, SCANLINE_PITCH);
    let (rgba, kept) = scanline_pixels(&column);
    let format = bevy::render::render_resource::TextureFormat::Rgba8Unorm;
    let mut image = nvf_assets::texture::checked_image(1, column.len() as u32, 1, format, rgba).map_err(|e| warn!("terminal: {SCANLINES}: {e}")).ok()?;
    image.asset_usage = bevy::asset::RenderAssetUsages::default();
    image.sampler = bevy::image::ImageSampler::linear();
    Some((images.add(image), kept))
}

/// [design] Quantize the overlay first, then measure the light those actual bytes keep.
pub(crate) fn scanline_pixels(column: &[f32]) -> (Vec<u8>, f32) {
    let alpha: Vec<u8> = column.iter().map(|&k| ((1.0 - k).clamp(0.0, 1.0) * 255.0).round() as u8).collect();
    let kept = alpha.iter().map(|&a| 1.0 - a as f32 / 255.0).sum::<f32>() / alpha.len() as f32;
    let rgba = alpha.iter().flat_map(|&a| [0, 0, 0, a]).collect();
    (rgba, kept)
}

#[derive(Component)]
pub struct ScreenRoot;

/// What lies under the screen: the darkened world and the monitor, or the flat picture.
#[derive(Component)]
pub struct Backdrop {
    pub flat: bool,
}

/// The flat picture of the screen, for the pointer.
#[derive(Component)]
pub struct FlatScreen;

/// What [`spawn_backdrop`] draws over the darkened world.
pub enum Under {
    /// The screen's picture (`super::TerminalImage`), flat where the original's screen is.
    Flat(Handle<Image>),
    /// The monitor's picture (`super::MonitorView`), over the whole window.
    Monitor(Handle<Image>),
}

pub fn spawn_backdrop(commands: &mut Commands, under: Under) {
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    let (flat, shade) = match under {
        Under::Flat(_) => (true, SHADE_FLAT),
        Under::Monitor(_) => (false, SHADE),
    };
    // The flat picture's top is [`CANVAS_TOP`] down; the monitor's picture fills the window.
    // At the UI scale the flat picture keeps to the view's middle (#1631).
    let top = if flat { crate::interface::from_top(CANVAS_TOP, 0.5) } else { Val::ZERO };
    let root = Node { justify_content: JustifyContent::Center, align_items: AlignItems::FlexStart, padding: UiRect::top(top), ..full.clone() };
    commands.spawn((Backdrop { flat }, Name::new("Terminal backdrop"), root, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, shade)), GlobalZIndex(106))).with_children(|r| match under {
        Under::Flat(image) => {
            let crop = Rect::new(0.0, 0.0, CANVAS.x, CANVAS.y);
            r.spawn((FlatScreen, Node { width: u(CANVAS.x), height: u(CANVAS.y), ..default() }, ImageNode { rect: Some(crop), ..ImageNode::new(image) }));
        }
        Under::Monitor(image) => {
            r.spawn((full, ImageNode::new(image)));
        }
    });
}

/// The blinking cursor block.
#[derive(Component)]
pub struct Blink;

/// The characters a second the screen's text types out at, in reading order as the page draws
/// its lines [hypothesis: the original types a page's text out as it comes up (#3327, the
/// player's report and video); its rate is not measured, observer check 254; this is about a
/// second for a menu page].
pub const TYPE_RATE: f32 = 600.0;
/// How far a typing line's clip reaches left of the line and past its end, canvas units: a
/// glyph's ink may stand a little out of its advance [design].
const TYPE_MARGIN: f32 = 4.0;

/// A line of text that types out (#3327): its clip widens from nothing to the whole line,
/// `chars` characters at [`TYPE_RATE`], from `start` seconds after its page came up.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Typed {
    pub start: f32,
    pub chars: f32,
    pub width: f32,
}

impl Typed {
    /// The clip's width `elapsed` seconds after the page came up.
    pub fn shown(&self, elapsed: f32) -> f32 {
        ((elapsed - self.start) * TYPE_RATE / self.chars.max(1.0)).clamp(0.0, 1.0) * self.width
    }

    /// Whether the line is typed out by `elapsed`.
    pub fn done(&self, elapsed: f32) -> bool {
        elapsed >= self.start + self.chars / TYPE_RATE
    }
}

thread_local! {
    /// While [`spawn`] draws a page: the seconds since the page came up and the characters
    /// drawn before the line being drawn (the lines type out one after another).
    static TYPING: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((f32::INFINITY, 0.0)) };
}

/// The [`Typed`] of the next line of `chars` characters `width` wide, in the order drawn.
fn next_typed(chars: usize, width: f32) -> (Typed, f32) {
    TYPING.with(|t| {
        let (elapsed, before) = t.get();
        t.set((elapsed, before + chars as f32));
        (Typed { start: before / TYPE_RATE, chars: chars as f32, width: width + 2.0 * TYPE_MARGIN }, elapsed)
    })
}

/// Draw a line that types out: `draw` puts it at `x` within a clip that starts
/// [`TYPE_MARGIN`] left of it (its own left edge), as wide as [`Typed::shown`] says.
fn typed_line(p: &mut ChildSpawnerCommands, line: &Line, x: f32, draw: impl FnOnce(&mut ChildSpawnerCommands, f32)) {
    typed_span(p, line.glyphs.len(), line.width, x, draw);
}

/// [`typed_line`] for `chars` characters over `width` (what is drawn may be wider than the
/// text, as the chosen row's box).
fn typed_span(p: &mut ChildSpawnerCommands, chars: usize, width: f32, x: f32, draw: impl FnOnce(&mut ChildSpawnerCommands, f32)) {
    let (typed, elapsed) = next_typed(chars, width);
    let clip = Node { overflow: Overflow::clip(), ..at(x - TYPE_MARGIN, 0.0, typed.shown(elapsed), CANVAS.y) };
    p.spawn((clip, typed)).with_children(|c| draw(c, TYPE_MARGIN));
}

/// [`set_line`], typing out ([`typed_line`]).
fn typed_set_line(p: &mut ChildSpawnerCommands, face: &Face, line: &Line, x: f32, base: f32, color: Color) {
    typed_line(p, line, x, |c, x| set_line(c, face, line, x, base, color));
}

/// One line of text from its top.
fn text(p: &mut ChildSpawnerCommands, face: &Face, s: &str, x: f32, top: f32, color: Color) -> f32 {
    let Some(line) = face.font.lines(s, None).into_iter().next() else { return 0.0 };
    typed_set_line(p, face, &line, x, top + BASE, color);
    line.width
}

/// A line of text whose characters have colours of their own, by the column each stands
/// in (every character of font 5 is [`CHAR`] wide): the lit cells of the dump.
/// The addresses, puzzle words and filler are ASCII (`fnv_game::hacking`).
/// `is_lit` explicitly selects the dark colour, `lit`'s picture (the same layout as
/// `face`'s), and the ordering above fills.
fn text_colored(p: &mut ChildSpawnerCommands, face: &Face, lit: &Face, line: &Line, x: f32, top: f32, is_lit: impl Fn(usize) -> bool) {
    typed_line(p, line, x, |p, x| text_colored_now(p, face, lit, line, x, top, is_lit));
}

/// [`text_colored`]'s line, drawn at once.
fn text_colored_now(p: &mut ChildSpawnerCommands, face: &Face, lit: &Face, line: &Line, x: f32, top: f32, is_lit: impl Fn(usize) -> bool) {
    let base = top + BASE;
    let bright = bright();
    let Some((image, size)) = &face.image else {
        set_line(p, face, line, x, base, bright);
        return;
    };
    p.spawn(line_anchor(base)).with_children(|p| {
        for placed in &line.glyphs {
            let k = (placed.x / CHAR).round() as usize;
            let g = face.font.glyph(placed.code);
            if !g.drawn() {
                continue;
            }
            let rect = Rect::new(g.uv[0] * size.x, g.uv[1] * size.y, g.uv[2] * size.x, g.uv[3] * size.y);
            let on_fill = is_lit(k);
            let c = if on_fill { lit_text() } else { bright };
            let picture = if on_fill { lit.image.as_ref().map_or(image, |(i, _)| i) } else { image };
            let glyph = (
                at(x + placed.x + g.before, -g.ascent, g.width, g.height),
                ImageNode { rect: Some(rect), color: c, image_mode: NodeImageMode::Stretch, ..ImageNode::new(picture.clone()) },
                LayoutConfig { use_rounding: false },
            );
            if on_fill {
                p.spawn((glyph, ON_FILLS));
            } else {
                p.spawn(glyph);
            }
        }
    });
}

/// Text centred across the screen.
fn centred(p: &mut ChildSpawnerCommands, face: &Face, s: &str, width: f32, top: f32, color: Color) {
    let w = face.font.width(s);
    text(p, face, s, (width - w) / 2.0, top, color);
}

/// Draw the existing terminal glass beneath caller-owned content. Coordinates use
/// the caller's original terminal canvas scope; no terminal session is opened.
pub fn spawn_background(c: &mut ChildSpawnerCommands, faces: &Faces, canvas: Rect) {
    // [design] Keep the texture/vignette, accounting for its contribution in the
    // undercoat. Both layers stay beneath ink; compose_screen mirrors this order.
    // Both are raised by what the scanlines take away on average (`lifted`).
    let undercoat = if faces.background.is_some() { screen_color() } else { compose_screen(Color::NONE, None) };
    c.spawn((at(canvas.min.x, canvas.min.y, canvas.width(), canvas.height()), BackgroundColor(lifted(undercoat, faces.scan_kept))));
    if let Some(image) = &faces.background {
        c.spawn((at(canvas.min.x, canvas.min.y, canvas.width(), canvas.height()), ImageNode { color: lifted(BACKGROUND_TINT, faces.scan_kept).with_alpha(BACKGROUND_ALPHA), image_mode: NodeImageMode::Stretch, ..ImageNode::new(image.clone()) }));
    }
}

/// Draw the existing terminal scanline treatment above caller-owned content.
pub fn spawn_scanlines(c: &mut ChildSpawnerCommands, faces: &Faces, canvas: Rect) {
    // [design] Over everything on the canvas, under the monitor's glare (§2, §7).
    if let Some(lines) = &faces.scanlines {
        c.spawn((at(canvas.min.x, canvas.min.y, canvas.width(), canvas.height()), ImageNode { image_mode: NodeImageMode::Stretch, ..ImageNode::new(lines.clone()) }));
    }
}

/// Draw the screen into the picture `camera` renders; returns the menu's rows on the
/// canvas, for the pointer, and an open note's window ([`Drawn`]).
/// `read`: the entries drawn dimmer (the Grey Read Notes tweak on).
/// `typed_for`: the seconds since the page came up, how far its text has typed out
/// ([`Typed`]).
#[allow(clippy::too_many_arguments)]
pub fn spawn(commands: &mut Commands, camera: Entity, faces: &Faces, session: &Session, game: &fnv_game::Game, cursor_on: bool, read: Option<&super::ReadEntries>, typed_for: f32) -> Drawn {
    TYPING.with(|t| t.set((typed_for, 0.0)));
    let mut rows = Drawn::default();
    let root = Node { overflow: Overflow::clip(), ..at(0.0, 0.0, CANVAS.x, CANVAS.y) };
    commands.spawn((ScreenRoot, root, UiTargetCamera(camera), Name::new("Terminal screen"))).with_children(|c| {
        spawn_background(c, faces, Rect::from_corners(Vec2::ZERO, CANVAS));
        match &session.page {
            Page::Hacking(h) => hacking_page(c, &faces.text, &faces.lit, h, cursor_on),
            Page::LockedOut(lines) => lockout_page(c, &faces.text, lines),
            Page::Menu(m) => menu_page(c, &faces.text, &faces.lit, m, game, cursor_on, &mut rows, read),
        }
        spawn_scanlines(c, faces, Rect::from_corners(Vec2::ZERO, CANVAS));
    });
    rows
}

/// [design] `glass` raised so that, with the scanlines keeping `kept` of its light on
/// average, it still averages what it would without them.
pub fn lifted(glass: Color, kept: f32) -> Color {
    let g = glass.to_linear();
    let k = kept.max(0.05);
    Color::linear_rgba(g.red / k, g.green / k, g.blue / k, g.alpha)
}

fn hacking_page(c: &mut ChildSpawnerCommands, face: &Face, lit_face: &Face, h: &Hacking, cursor_on: bool) {
    let p = &h.puzzle;
    let bright = bright();
    c.spawn(at(DEPTH.x, DEPTH.y, DEPTH_SIZE.x, DEPTH_SIZE.y)).with_children(|d| {
        // The header.
        text(d, face, words::HACK_TITLE, 0.0, HEADER_LINES[0], bright);
        let prompt = if p.attempts == 1 { words::HACK_WARNING } else { words::HACK_PROMPT };
        text(d, face, prompt, 0.0, HEADER_LINES[1], bright);
        let w = text(d, face, &words::attempts(p.attempts), 0.0, HEADER_LINES[2], bright);
        // [documented] Blocks one apart, starting one block after the text.
        for i in 0..p.attempts {
            let x = w + (2 * i as usize + 1) as f32 * BLOCK;
            d.spawn((at(x, HEADER_LINES[2] + BASE - 18.0, BLOCK, BLOCK), BackgroundColor(bright), OVER_LINES));
        }
        // The dump: the lit span under the cursor, while the game goes on.
        let lit = (h.ended.is_none() && !h.hidden).then(|| p.span(p.target(h.cursor)));
        let is_lit = |i: usize| lit.as_ref().is_some_and(|r| r.contains(&i));
        for column in 0..2 {
            for row in 0..ROWS {
                let x = column as f32 * COLUMN_2;
                let top = HEADER_HEIGHT + row as f32 * ROW + ROW_TEXT_Y;
                let first = Puzzle::index(column, row, 0);
                let s = format!("0x{:04X} {}", p.row_address(column, row), p.row_text(column, row));
                for k in 0..ROW_CHARS {
                    if is_lit(first + k) {
                        let cell = cell_at(first + k);
                        d.spawn((at(cell.x, cell.y + ROW_TEXT_Y - 2.0, CHAR, ROW - 4.0), BackgroundColor(bright), OVER_LINES));
                    }
                }
                if let Some(line) = face.font.lines(&s, None).into_iter().next() {
                    text_colored(d, face, lit_face, &line, x, top, |k| k >= 7 && is_lit(first + k - 7));
                }
            }
        }
        // The log, the prompt and what the cursor would enter.
        d.spawn(at(LOG_X, HEADER_HEIGHT, DEPTH_SIZE.x - LOG_X, FILE_HEIGHT)).with_children(|l| {
            let shown = shown_log(&p.log);
            for (i, entry) in shown.iter().rev().enumerate() {
                let top = FILE_HEIGHT - ROW * (3 + i) as f32 + ROW_TEXT_Y + 1.0;
                text(l, face, entry, PROMPT_X, top, bright);
            }
            let top = FILE_HEIGHT - ROW + ROW_TEXT_Y + 1.0;
            text(l, face, ">", PROMPT_X, top, bright);
            let entry = if lit.is_some() { p.text(p.target(h.cursor)) } else { String::new() };
            let w = text(l, face, &entry, ENTRY_X, top, bright);
            if entry.is_empty() {
                l.spawn((Blink, at(ENTRY_X + w, top + BASE - 18.0, BLOCK, BLOCK), BackgroundColor(bright), OVER_LINES, if cursor_on { Visibility::Inherited } else { Visibility::Hidden }));
            }
        });
    });
}

fn lockout_page(c: &mut ChildSpawnerCommands, face: &Face, lines: &[String; 2]) {
    let bright = bright();
    c.spawn(at(DEPTH.x, DEPTH.y, DEPTH_SIZE.x, DEPTH_SIZE.y)).with_children(|d| {
        // [documented] One line above the middle, one below.
        centred(d, face, &lines[0], DEPTH_SIZE.x, DEPTH_SIZE.y / 2.0 - LINE, bright);
        centred(d, face, &lines[1], DEPTH_SIZE.x, DEPTH_SIZE.y / 2.0 + LINE, bright);
    });
}

/// The text a note shows on the screen: a text note's text, else its name.
pub fn note_text(n: Option<&NoteDef>) -> String {
    match n {
        Some(NoteDef { text: Some(t), .. }) => t.replace('\r', ""),
        Some(n) => n.name.clone().unwrap_or_default(),
        None => String::new(),
    }
}

/// Menu choices and the result prompt have a single row. Keep long CJK text within it.
fn fit_cjk_line<'a>(font: &Font, text: &'a str, room: f32) -> std::borrow::Cow<'a, str> {
    if crate::cjk_font::enabled() && text.chars().any(fnv_font::is_cjk) {
        crate::pause::fit_text(|s| font.width(s), text, room).into()
    } else {
        text.into()
    }
}

/// Set a terminal welcome or note text in wrapped lines at [`WELCOME_WRAP`].
///
/// Each `\n`-separated paragraph produces at least one line (an empty or whitespace-only
/// paragraph produces one empty [`Line`] from [`Font::lines`], counting as one row — #3478
/// [hypothesis: original-game screenshots in #3478 show roughly half NVForever's previous
/// double-counted gap; observer check pending]).
pub(crate) fn wrapped_lines(font: &Font, text: &str) -> Vec<Line> {
    font.lines(&text.replace('\r', ""), Some(WELCOME_WRAP))
}

/// Vertical placement of the terminal menu's welcome block and separator on the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WelcomeLayout {
    pub top: f32,
    pub rows: usize,
    pub widest: f32,
    pub separator_y: f32,
    pub body_y: f32,
}

/// Lay out the welcome text under the three-line terminal header.
pub(crate) fn welcome_layout(font: &Font, welcome: &str) -> (Vec<Line>, WelcomeLayout) {
    let top = MENU_HEADER_Y + 3.0 * LINE + 20.0;
    let lines = wrapped_lines(font, welcome);
    let widest = lines.iter().fold(0.0f32, |w, l| w.max(l.width));
    let rows = lines.len();
    // [documented] The separator 10 under the welcome text, as long as it is wide.
    let separator_y = top + rows as f32 * LINE + 10.0;
    let body_y = separator_y + 12.0;
    (lines, WelcomeLayout { top, rows, widest, separator_y, body_y })
}

/// How many note lines fit below `body_y` above the result line, capped at [`NOTE_LINES`].
pub(crate) fn note_page_room(body_y: f32) -> usize {
    // [documented] At most 16 lines at a time; here no more than stand above the
    // result line [design], from the line scrolled to.
    let room = (((CANVAS.y - LINE - RESULT_GAP) - (body_y + 10.0)) / LINE).floor().max(1.0) as usize;
    NOTE_LINES.min(room)
}

/// Lay out a terminal note below `body_y` and compute its visible [`NoteWindow`].
pub(crate) fn note_layout(font: &Font, note: FormId, text: &str, scroll: usize, body_y: f32) -> (Vec<Line>, NoteWindow) {
    let lines = wrapped_lines(font, text);
    let page = note_page_room(body_y);
    let first = scroll.min(lines.len().saturating_sub(page));
    let window = NoteWindow { note, first, shown: page.min(lines.len()), total: lines.len() };
    (lines, window)
}

#[allow(clippy::too_many_arguments)]
fn menu_page(c: &mut ChildSpawnerCommands, face: &Face, lit_face: &Face, m: &MenuView, game: &fnv_game::Game, cursor_on: bool, drawn: &mut Drawn, read: Option<&super::ReadEntries>) {
    let rows = &mut drawn.rows;
    let bright = bright();
    let data = game.data;
    let Some(t) = data.terminals.get(&m.terminal) else { return };
    let mut y = MENU_HEADER_Y;
    for line in [words::MENU_TITLE, words::MENU_COPYRIGHT, &fnv_game::terminal::server_text(m.server)] {
        centred(c, face, line, CANVAS.x, y, bright);
        y += LINE;
    }
    // The welcome text (the top terminal's, or the sub menu's own when it has one).
    // Its player string codes expanded (`fnv_game::text_codes`; the top's is already).
    let own = game.expand_codes(&t.welcome);
    let welcome = if t.welcome.trim().is_empty() { &m.welcome } else { &own };
    let (welcome_lines, layout) = welcome_layout(&face.font, welcome);
    for (i, line) in welcome_lines.iter().enumerate() {
        if !line.glyphs.is_empty() {
            typed_set_line(c, face, line, MENU_X, layout.top + i as f32 * LINE + BASE, bright);
        }
    }
    // [documented] The separator 10 under the welcome text, as long as it is wide.
    c.spawn((at(MENU_X, layout.separator_y, layout.widest.max(1.0), 2.0), BackgroundColor(bright), OVER_LINES));
    let y = layout.body_y;
    match m.note {
        Some(n) => {
            let text = game.expand_codes(&note_text(data.notes.get(&n)));
            let (lines, window) = note_layout(&face.font, n, &text, m.scroll, y);
            drawn.note = Some(window);
            for (i, line) in lines.into_iter().skip(window.first).take(window.shown).enumerate() {
                if !line.glyphs.is_empty() {
                    typed_set_line(c, face, &line, MENU_X, y + 10.0 + i as f32 * LINE + BASE, bright);
                }
            }
        }
        None => {
            let first = m.selected.saturating_sub(LIST_ROWS - 1);
            for (row, label) in m.labels.iter().enumerate().skip(first).take(LIST_ROWS) {
                let top = y + (row - first) as f32 * LIST_ROW;
                let lit = row == m.selected && !m.hidden;
                rows.push((row, Rect::new(MENU_X, top, MENU_X + LIST_WIDTH, top + LIST_ROW)));
                c.spawn(at(MENU_X, top, LIST_WIDTH, LIST_ROW)).with_children(|b| {
                    let s = format!("> {label}");
                    let s = fit_cjk_line(&face.font, &s, LIST_WIDTH - CHAR);
                    let w = face.font.width(&s);
                    if lit {
                        // [documented] A filled box (224 of 255) behind the chosen row; it types
                        // out with its text [design].
                        if let Some(line) = face.font.lines(&s, None).into_iter().next() {
                            typed_span(b, line.glyphs.len(), w + CHAR, 0.0, |b, x| {
                                b.spawn((at(x, 2.0, w + CHAR, LIST_ROW - 4.0), BackgroundColor(bright.with_alpha(224.0 / 255.0)), OVER_LINES));
                                set_marked(b, lit_face, &line, x, (LIST_ROW - LINE) / 2.0 + BASE, lit_text(), ON_FILLS);
                            });
                        }
                    } else {
                        let was_read = read.zip(m.items.get(row)).is_some_and(|(r, &item)| r.has(m.terminal, item));
                        text(b, face, &s, 0.0, (LIST_ROW - LINE) / 2.0, if was_read { read_green() } else { bright });
                    }
                });
            }
        }
    }
    // The result line at the bottom: the prompt, the last result, the cursor.
    let top = CANVAS.y - LINE - RESULT_GAP;
    text(c, face, ">", MENU_X, top, bright);
    let result_room = CANVAS.x - 2.0 * MENU_X - RESULT_X - CHAR - BLOCK;
    let result = fit_cjk_line(&face.font, &m.result, result_room);
    let w = text(c, face, &result, MENU_X + RESULT_X, top, bright);
    let x = MENU_X + RESULT_X + if m.result.is_empty() { 0.0 } else { w + CHAR };
    c.spawn((Blink, at(x, top + BASE - 18.0, BLOCK, BLOCK), BackgroundColor(bright), OVER_LINES, if cursor_on { Visibility::Inherited } else { Visibility::Hidden }));
}

/// Whether a row's character `index` is drawn lit (tests).
pub fn lit(p: &Puzzle, cursor: usize, index: usize) -> bool {
    let t: Target = p.target(cursor);
    p.span(t).contains(&index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #3327: a page's lines type out one after another in the order drawn, each widening
    /// to its whole width; a page drawn again (the selection moved) types on from where it was.
    #[test]
    fn lines_type_out_one_after_another() {
        TYPING.with(|t| t.set((0.5, 0.0)));
        let (a, elapsed) = next_typed(300, 600.0);
        let (b, _) = next_typed(60, 120.0);
        assert_eq!(elapsed, 0.5);
        assert_eq!((a.start, b.start), (0.0, 300.0 / TYPE_RATE));
        assert_eq!(a.shown(0.0), 0.0);
        assert!((a.shown(150.0 / TYPE_RATE) - a.width / 2.0).abs() < 1e-3, "half the line by half its time");
        assert_eq!(a.shown(1.0), a.width);
        assert!(a.done(300.0 / TYPE_RATE) && !b.done(300.0 / TYPE_RATE));
        assert_eq!(b.shown(300.0 / TYPE_RATE), 0.0, "the next line waits for the one before");
        assert_eq!(b.width, 120.0 + 2.0 * TYPE_MARGIN);
        // A page drawn again later starts counting from its first line again.
        TYPING.with(|t| t.set((9.0, 0.0)));
        assert_eq!(next_typed(300, 600.0), (a, 9.0));
    }

    #[test]
    fn cjk_terminal_labels_fit_on_one_row() {
        let mut font = plain_font(25.0, 10.0);
        let glyph = fnv_font::Glyph {
            width: 18.0,
            ..default()
        };
        for c in "甲乙丙丁".chars() {
            font.extra.insert(c, glyph);
        }
        assert_eq!(fit_cjk_line(&font, "甲乙丙丁甲", 66.0), "甲乙丙丁甲");
        crate::cjk_font::with_enabled_for_test(|| {
            assert_eq!(fit_cjk_line(&font, "甲乙", 66.0), "甲乙");
            assert_eq!(fit_cjk_line(&font, "甲乙丙丁甲", 66.0), "甲乙...");
            assert_eq!(fit_cjk_line(&font, "abcdefghi", 66.0), "abcdefghi");
        });
    }

    #[test]
    fn composed_layers_match_the_measured_screen_in_both_modes() {
        // [observed: hacking.md §2] Final displayed glass ranges and block colour; the
        // glass alone sits under the ranges by the glare's floor (GLASS_MIDDLE).
        let ranges = [(16..=23), (38..=52), (15..=32)];
        let glass = compose_screen(Color::NONE, Some(BACKGROUND_MIDDLE));
        let block = bright();
        assert_eq!(glass.to_srgba().to_u8_array(), [12, 38, 20, 255]);
        assert_eq!(block.to_srgba().to_u8_array(), [54, 252, 150, 255]);
        // [design] Verify the actual undercoat + ImageNode source-over, not just the
        // target constant. Samples cover a dark corner, dim texture and middle;
        // the brightest texture can exceed the middle's range, as can the glare (§2).
        assert_eq!(screen_color().to_srgba().to_u8_array(), [4, 10, 2, 255]);
        let tint = BACKGROUND_TINT.to_linear();
        for (sample, expected) in [([5, 6, 5], [3, 8, 2, 255]), ([49, 49, 49], [7, 24, 11, 255]), ([76, 77, 76], [12, 38, 20, 255]), ([99, 99, 99], [17, 50, 28, 255])] {
            let texel = Color::srgb_u8(sample[0], sample[1], sample[2]);
            let p = texel.to_linear();
            let layer = Color::linear_rgba(p.red * tint.red, p.green * tint.green, p.blue * tint.blue, BACKGROUND_ALPHA);
            let ui_glass = over(layer, screen_color());
            assert_eq!(ui_glass.to_srgba().to_u8_array(), expected);
            assert_eq!(compose_screen(Color::NONE, Some(texel)), ui_glass);
            assert_eq!(compose_screen(block, Some(texel)).to_srgba().to_u8_array(), [54, 252, 150, 255]);
        }
        // A transparent texture sample leaves the undercoat, and a missing image
        // uses the middle target; neither may turn the screen transparent [design].
        assert_eq!(compose_screen(Color::NONE, Some(Color::NONE)), screen_color());
        assert_eq!(compose_screen(Color::NONE, None).to_srgba().to_u8_array(), [12, 38, 20, 255]);
        // [design] Exercise source-over at partial glyph coverage as well as the
        // empty glass and solid blocks; blending must happen in linear light.
        assert_eq!(compose_screen(block.with_alpha(0.5), Some(BACKGROUND_MIDDLE)).to_srgba().to_u8_array(), [39, 187, 110, 255]);
        for (shade, glare_floor) in [(SHADE, Some([8u8, 6, 8])), (SHADE_FLAT, None)] {
            for world in [Color::BLACK, Color::WHITE] {
                let backdrop = over(Color::BLACK.with_alpha(shade), world);
                let shown_glass = over(glass, backdrop).to_srgba().to_u8_array();
                let shown_block = over(block, backdrop).to_srgba().to_u8_array();
                // [design] Monitor only: what the glare's dark texels add over the
                // middle in linear light, about (8, 6, 8) display units on this glass
                // (terminal_view.rs, §7), puts the glass into the §2 ranges.
                if let Some(floor) = glare_floor {
                    for channel in 0..3 {
                        assert!(ranges[channel].contains(&(shown_glass[channel] + floor[channel])), "glass {shown_glass:?}, shade {shade}");
                    }
                } else {
                    // Flat mode has no glare: the opaque canvas must show its own
                    // middle, independently of the world behind its darker shade.
                    assert_eq!(shown_glass, [12, 38, 20, 255]);
                }
                for channel in 0..3 {
                    assert!(shown_block[channel].abs_diff([54, 252, 150][channel]) <= 3, "block {shown_block:?}, shade {shade}");
                }
                assert_eq!(shown_glass[3], 255);
                assert_eq!(shown_block[3], 255);
            }
        }
    }

    #[test]
    fn soft_edges_blend_as_display_values_do() {
        // [design] Ends stay put; a half-covered texel of ink over the glass gives what
        // mixing the display values would, much less light than a linear mix (§2).
        let (ink, glass) = (1.0, 38.0 / 255.0);
        assert_eq!(display_blend(0.0, ink, glass), 0.0);
        assert!((display_blend(1.0, ink, glass) - 1.0).abs() < 1e-5);
        let half = display_blend(0.5, ink, glass);
        assert!((0.25..0.35).contains(&half), "{half}");
        assert_eq!(display_blend(0.3, 0.5, 0.5), 0.3);
    }

    #[test]
    fn soft_dark_edges_blend_as_display_values_over_bright_fills() {
        // [design] Exercise the lit atlas's dark-over-bright bend independently of
        // bright text on glass; its half coverage needs more than half linear alpha.
        let (ink, fill) = (5.0 / 255.0, 252.0 / 255.0);
        for coverage in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let alpha = display_blend(coverage, ink, fill);
            let shown = over(lit_text().with_alpha(alpha), bright()).to_srgba();
            let expected = coverage * ink + (1.0 - coverage) * fill;
            assert!((shown.green - expected).abs() < 1e-5, "coverage {coverage}, alpha {alpha}");
        }
        assert_eq!(display_blend(0.0, ink, fill), 0.0);
        assert!((display_blend(1.0, ink, fill) - 1.0).abs() < 1e-5);
        assert!((0.7..0.85).contains(&display_blend(0.5, ink, fill)));
    }

    #[test]
    fn softened_glyphs_keep_their_place_and_do_not_bleed() {
        use fnv_font::Glyph;
        // Two 2 x 2 glyphs touching corner to corner on a 4 x 4 texture.
        let mut glyphs = vec![Glyph::default(); 256];
        glyphs[65] = Glyph { uv: [0.0, 0.0, 0.5, 0.5], width: 2.0, height: 2.0, before: 1.0, after: 1.0, ascent: 2.0 };
        glyphs[66] = Glyph { uv: [0.5, 0.5, 1.0, 1.0], width: 2.0, height: 2.0, before: 0.0, after: 3.0, ascent: 1.0 };
        let font = Font { size: 4.0, texture: String::new(), glyphs, extra: Default::default() };
        let mut alpha = vec![0.0; 16];
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1), (2, 2), (3, 2), (2, 3), (3, 3)] {
            alpha[y * 4 + x] = 1.0;
        }
        let soft = soften(&font, &alpha, 4, 4, SOFT_SIGMA, |a| a).expect("softened");
        let pad = SOFT_PAD as f32;
        for code in [65, 66] {
            let (a, b) = (font.glyphs[code], soft.font.glyphs[code]);
            assert_eq!(a.advance(), b.advance());
            assert_eq!((b.width, b.height, b.before, b.ascent), (a.width + 2.0 * pad, a.height + 2.0 * pad, a.before - pad, a.ascent + pad));
        }
        // Read a softened glyph's cell back: the middle stays the strongest, the blur
        // falls off within the room, and the cell's border is empty (no neighbour in it).
        let cell = |code: usize| {
            let g = soft.font.glyphs[code];
            let (x0, y0) = ((g.uv[0] * soft.width as f32).round() as usize, (g.uv[1] * soft.height as f32).round() as usize);
            let side = 2 + 2 * SOFT_PAD;
            (0..side).map(|y| (0..side).map(|x| soft.rgba[((y0 + y) * soft.width + x0 + x) * 4 + 3]).collect::<Vec<_>>()).collect::<Vec<_>>()
        };
        for code in [65, 66] {
            let c = cell(code);
            let mid = c[SOFT_PAD][SOFT_PAD];
            assert!(mid > 128 && mid < 255, "{mid}");
            assert!(c[SOFT_PAD][SOFT_PAD - 1] > 0 && c[SOFT_PAD][SOFT_PAD - 1] < mid);
            let last = c.len() - 1;
            assert!(c[0].iter().chain(&c[last]).all(|&a| a == 0), "{c:?}");
        }
    }

    #[test]
    fn softened_glyphs_refuse_invalid_uvs_before_rounding() {
        use fnv_font::Glyph;
        let mut font = Font { size: 4.0, texture: String::new(), glyphs: vec![Glyph::default(); 256], extra: Default::default() };
        // [design] Include slight excursions that rounding would otherwise hide,
        // non-finite edges, reversed ranges, and ranges collapsed by rounding.
        for uv in [
            [-0.01, 0.0, 0.5, 1.0], [0.0, 0.0, 1.01, 1.0],
            [0.0, -0.01, 1.0, 1.0], [0.0, 0.0, 1.0, 1.01],
            [f32::NAN, 0.0, 1.0, 1.0], [0.0, 0.0, f32::INFINITY, 1.0],
            [0.75, 0.0, 0.25, 1.0], [0.0, 0.75, 1.0, 0.25],
            [0.5, 0.0, 0.5, 1.0], [0.01, 0.0, 0.02, 1.0],
        ] {
            font.glyphs[65] = Glyph { uv, width: 1.0, height: 1.0, ..Glyph::default() };
            assert!(soften(&font, &[1.0; 16], 4, 4, SOFT_SIGMA, |a| a).is_none(), "{uv:?}");
        }
    }

    #[test]
    fn softened_fractional_uvs_use_nearest_edges_and_preserve_ink_placement() {
        use fnv_font::Glyph;
        let mut glyphs = vec![Glyph::default(); 256];
        // [design] UVs span source edges (1.25, 2.25) to (3.25, 5.25), rounded to
        // a 2 x 3 cut-out. Non-unit metric/texel ratios must also pad proportionally.
        let original = Glyph { uv: [1.25 / 8.0, 2.25 / 8.0, 3.25 / 8.0, 5.25 / 8.0], width: 3.0, height: 6.0, before: 1.0, after: 2.0, ascent: 5.0 };
        glyphs[65] = original;
        let font = Font { size: 8.0, texture: String::new(), glyphs, extra: Default::default() };
        let mut alpha = vec![0.0; 64];
        for y in 2..5 {
            alpha[y * 8 + 1] = 0.25;
            alpha[y * 8 + 2] = 0.75;
        }
        let soft = soften(&font, &alpha, 8, 8, 0.0, |a| a).expect("fractional glyph");
        let g = soft.font.glyphs[65];
        assert_eq!(g.advance(), original.advance());
        let px = g.width / (2 + 2 * SOFT_PAD) as f32;
        let py = g.height / (3 + 2 * SOFT_PAD) as f32;
        assert_eq!(g.before + SOFT_PAD as f32 * px, original.before);
        assert_eq!(-g.ascent + SOFT_PAD as f32 * py, -original.ascent);
        // No rect offset: padding moves the cut-out back to the original ink origin.
        for y in 0..3 {
            let from = ((SOFT_PAD + y) * soft.width + SOFT_PAD) * 4 + 3;
            assert_eq!([soft.rgba[from], soft.rgba[from + 4]], [64, 191]);
        }
    }

    #[test]
    fn softened_shelves_wrap_below_the_tallest_glyph_without_coverage_bleed() {
        use fnv_font::Glyph;
        let mut glyphs = vec![Glyph::default(); 256];
        let mut alpha = vec![0.0; 750 * 4];
        for (i, (height, coverage)) in [(1, 1.0), (4, 0.25), (2, 0.75)].into_iter().enumerate() {
            glyphs[65 + i] = Glyph { uv: [i as f32 / 3.0, 0.0, (i + 1) as f32 / 3.0, height as f32 / 4.0], width: 250.0, height: height as f32, ..Glyph::default() };
            for y in 0..height {
                alpha[y * 750 + i * 250..y * 750 + (i + 1) * 250].fill(coverage);
            }
        }
        let font = Font { size: 4.0, texture: String::new(), glyphs, extra: Default::default() };
        let soft = soften(&font, &alpha, 750, 4, SOFT_SIGMA, |a| a).expect("wrapped shelf");
        let origins: Vec<_> = (65..68).map(|code| {
            let g = soft.font.glyphs[code];
            ((g.uv[0] * soft.width as f32).round() as usize, (g.uv[1] * soft.height as f32).round() as usize)
        }).collect();
        assert_eq!(origins, [(0, 0), (256, 0), (0, 4 + 2 * SOFT_PAD)]);
        // Independently blur each glyph: packing beside different coverage or on the
        // next shelf must produce exactly the same cell, including its empty border.
        for (i, &(x0, y0)) in origins.iter().enumerate() {
            let mut solo = font.clone();
            for (k, g) in solo.glyphs[65..68].iter_mut().enumerate() {
                if k != i { *g = Glyph::default(); }
            }
            let single = soften(&solo, &alpha, 750, 4, SOFT_SIGMA, |a| a).unwrap();
            let height = [1, 4, 2][i] + 2 * SOFT_PAD;
            for y in 0..height {
                let packed = ((y0 + y) * soft.width + x0) * 4;
                let alone = y * single.width * 4;
                assert_eq!(&soft.rgba[packed..packed + 256 * 4], &single.rgba[alone..alone + 256 * 4]);
            }
        }
    }

    #[test]
    fn scanlines_darken_display_values_and_the_glass_is_lifted_back() {
        // [design] A white band keeps everything; a band dipping to 0.8 keeps linear(0.8)
        // at its trough, about 0.6 of the light (0.8 of the display value) (§2).
        assert!(scanline_column(&[1.0; 64], 720, SCANLINE_PITCH).iter().all(|&k| (k - 1.0).abs() < 1e-6));
        let band: Vec<f32> = (0..64).map(|i| if (32..48).contains(&i) { 0.8 } else { 1.0 }).collect();
        let column = scanline_column(&band, 720, SCANLINE_PITCH);
        let (lo, hi) = column.iter().fold((1.0f32, 0.0f32), |(lo, hi), &k| (lo.min(k), hi.max(k)));
        assert!((lo - linear(0.8)).abs() < 0.01 && (hi - 1.0).abs() < 1e-6, "{lo} {hi}");
        let rows = (SCANLINE_PITCH * 10.0).round() as usize;
        assert!((column[3] - column[3 + rows]).abs() < 0.1, "ten lines on, the same place in the band");
        let glass = Color::srgb_u8(12, 38, 20);
        assert_eq!(lifted(glass, 1.0).to_srgba().to_u8_array(), glass.to_srgba().to_u8_array());
        let lift = lifted(glass, 0.5).to_linear();
        assert!((lift.green - 2.0 * glass.to_linear().green).abs() < 1e-6);
    }

    #[test]
    fn quantized_scanlines_preserve_the_glass_mean_through_all_layers() {
        // [design] Use the same baked bytes and kept mean as scanline_image, then
        // mirror spawn's lifted undercoat -> texture * lifted tint -> overlay. Check
        // corners, middle and bright glass plus the missing-background fallback.
        let band: Vec<f32> = (0..64).map(|i| if (32..48).contains(&i) { 0.8 } else { 1.0 }).collect();
        let column = scanline_column(&band, CANVAS.y as usize, SCANLINE_PITCH);
        let (rgba, kept) = scanline_pixels(&column);
        let byte_mean = rgba.as_chunks::<4>().0.iter().map(|p| 1.0 - p[3] as f32 / 255.0).sum::<f32>() / column.len() as f32;
        assert_eq!(kept, byte_mean);
        assert!(kept > 0.5 && kept < 1.0);
        for sample in [Some(Color::srgb_u8(5, 6, 5)), Some(BACKGROUND_MIDDLE), Some(Color::srgb_u8(99, 99, 99)), None] {
            let glass = if let Some(texel) = sample {
                let p = texel.to_linear();
                let t = lifted(BACKGROUND_TINT, kept).to_linear();
                over(Color::linear_rgba(p.red * t.red, p.green * t.green, p.blue * t.blue, p.alpha * BACKGROUND_ALPHA), lifted(screen_color(), kept))
            } else {
                lifted(compose_screen(Color::NONE, None), kept)
            };
            let mut light_sum = [0.0f32; 3];
            let mut display_sum = [0u32; 3];
            let mut min_green = 255;
            let mut max_green = 0;
            for p in rgba.as_chunks::<4>().0 {
                let shown = over(Color::srgba_u8(p[0], p[1], p[2], p[3]), glass);
                let light = shown.to_linear();
                let bytes = shown.to_srgba().to_u8_array();
                for (channel, value) in [light.red, light.green, light.blue].into_iter().enumerate() {
                    light_sum[channel] += value;
                    display_sum[channel] += bytes[channel] as u32;
                }
                min_green = min_green.min(bytes[1]);
                max_green = max_green.max(bytes[1]);
                assert_eq!(bytes[3], 255);
            }
            assert!(max_green > min_green, "scanlines must remain visible");
            let target = compose_screen(Color::NONE, sample);
            let linear_target = target.to_linear();
            let bytes = target.to_srgba().to_u8_array();
            for (channel, value) in [linear_target.red, linear_target.green, linear_target.blue].into_iter().enumerate() {
                assert!((light_sum[channel] / column.len() as f32 - value).abs() < 1e-6);
                assert!((display_sum[channel] as f32 / column.len() as f32 - bytes[channel] as f32).abs() <= 1.0);
            }
        }
    }

    #[test]
    fn the_file_fits_the_depth_area() {
        // Two columns of 19 characters end where the log starts, 665 in.
        assert_eq!(LOG_X, 665.0);
        const { assert!(HEADER_HEIGHT + FILE_HEIGHT <= DEPTH_SIZE.y) };
        const { assert!(DEPTH.x + DEPTH_SIZE.x <= CANVAS.x && DEPTH.y + DEPTH_SIZE.y <= CANVAS.y) };
    }

    #[test]
    fn cells_and_points_agree() {
        for i in [0, 11, 12, 203, 204, 215, 407] {
            let c = cell_at(i);
            let p = Vec2::new(c.x + CHAR / 2.0, c.y - HEADER_HEIGHT + ROW / 2.0);
            assert_eq!(index_at(p), Some(i), "{i}");
        }
        // On an address or between the columns: nothing.
        assert_eq!(index_at(Vec2::new(10.0, 10.0)), None);
        assert_eq!(index_at(Vec2::new(330.0, 10.0)), None);
        assert_eq!(index_at(Vec2::new(500.0, FILE_HEIGHT + 1.0)), None);
    }

    #[test]
    fn the_log_keeps_fifteen_lines() {
        let log: Vec<String> = (0..20).map(|i| format!(">{i}")).collect();
        let shown = shown_log(&log);
        assert_eq!(shown.len(), 15);
        assert_eq!(shown.last().map(String::as_str), Some(">19"));
        assert_eq!(shown_log(&log[..3]).len(), 3);
    }

    #[test]
    fn soft_faces_loads_tex_only_font_without_dds() {
        use crate::interface::font_tex_tests::{synthetic_fnt, synthetic_tex, OwnedFontFiles};
        let files = OwnedFontFiles::new(&[
            ("term_tex.fnt", synthetic_fnt("Term_Tex_Only")),
            ("Term_Tex_Only.Tex", synthetic_tex(32, 64, [255, 255, 255, 220])),
        ]);
        let data = files.data();
        let mut images = Assets::<Image>::default();
        let (text, lit) = soft_faces("textures\\fonts\\term_tex.fnt", &data, &mut images)
            .expect("terminal soft_faces should load .tex-only font without .dds");
        assert!(text.image.is_some() && lit.image.is_some());
    }

    #[test]
    fn welcome_blank_lines_count_as_one_row_each() {
        let font = plain_font(LINE, CHAR);
        let (_, base) = welcome_layout(&font, "Welcome");
        assert_eq!(base.rows, 1);
        assert_eq!(base.separator_y, base.top + LINE + 10.0);
        assert_eq!(base.body_y, base.separator_y + 12.0);
        assert_eq!(base.widest, 7.0 * CHAR);

        // N trailing blank lines (both LF and CRLF) leave N rows between the welcome text
        // and the separator (#3478), without changing widest.
        for (text, expected_rows) in [
            ("Welcome\n", 2),
            ("Welcome\r\n", 2),
            ("Welcome\n\n", 3),
            ("Welcome\r\n\r\n", 3),
            ("Welcome\n\n\n\n", 5),
            ("Welcome\r\n\r\n\r\n\r\n", 5),
            ("\nWelcome", 2),
            ("\r\n\r\nWelcome", 3),
        ] {
            let (_, layout) = welcome_layout(&font, text);
            assert_eq!(layout.rows, expected_rows, "{text:?}");
            assert_eq!(layout.separator_y - base.separator_y, (expected_rows - 1) as f32 * LINE, "{text:?}");
            assert_eq!(layout.widest, base.widest, "{text:?}");
        }

        // Consecutive interior blank lines and whitespace-only lines each take one row.
        for (text, expected_rows) in [
            ("Line 1\n\n\nLine 2", 4),
            ("Line 1\r\n\r\n\r\nLine 2", 4),
            ("Line 1\n   \nLine 2", 3),
            ("Line 1\r\n   \r\nLine 2", 3),
            ("Line 1\n\t\nLine 2", 3),
        ] {
            let (_, layout) = welcome_layout(&font, text);
            assert_eq!(layout.rows, expected_rows, "{text:?}");
        }

        // Wrapped paragraphs followed by blank lines keep their wrapped row count plus one
        // row per blank line. At CHAR = 17 and WELCOME_WRAP = 600, 35 chars fit on one line.
        let long = "alpha beta gamma delta epsilon zeta eta theta";
        assert_eq!(wrapped_lines(&font, long).len(), 2);
        let (_, wrapped_interior) = welcome_layout(&font, &format!("{long}\n\nNext"));
        assert_eq!(wrapped_interior.rows, 4);
        let (_, wrapped_trailing) = welcome_layout(&font, &format!("{long}\r\n\r\n\r\n"));
        assert_eq!(wrapped_trailing.rows, 5);

        // An empty welcome text occupies one row (not two) and widest stays 0.
        let (_, empty) = welcome_layout(&font, "");
        assert_eq!(empty.rows, 1);
        assert_eq!(empty.widest, 0.0);
    }

    #[test]
    fn note_pagination_and_blank_lines_match_welcome_row_budget() {
        let font = plain_font(LINE, CHAR);
        let note_id = FormId(0x0000_1234);

        // CRLF and LF note bodies with consecutive, leading, trailing, and wrapped blank lines
        // produce identical row counts (one row per blank line).
        let long = "alpha beta gamma delta epsilon zeta eta theta";
        let note_lf = format!("Title\n=====\n\n{long}\n\n\nTail\n");
        let note_crlf = note_lf.replace('\n', "\r\n");
        let lines_lf = wrapped_lines(&font, &note_lf);
        let lines_crlf = wrapped_lines(&font, &note_crlf);
        // Title (1) + ===== (1) + blank (1) + long (2) + 2 blanks (2) + Tail (1) + trailing blank (1) = 9
        assert_eq!(lines_lf.len(), 9);
        assert_eq!(lines_lf, lines_crlf);

        // With a 2-line welcome and 0 trailing blank lines, room hits the 16-line cap.
        let (_, no_blanks) = welcome_layout(&font, "Welcome line 1\nWelcome line 2");
        assert_eq!(no_blanks.rows, 2);
        assert_eq!(note_page_room(no_blanks.body_y), NOTE_LINES);

        // With 4 trailing blank lines after a 2-line welcome, the welcome occupies 6 rows
        // instead of 10, leaving 13 note rows on page one instead of 9 (#3478).
        let (_, four_blanks) = welcome_layout(&font, "Welcome line 1\r\nWelcome line 2\r\n\r\n\r\n\r\n");
        assert_eq!(four_blanks.rows, 6);
        assert_eq!(note_page_room(four_blanks.body_y), 13);

        let long_note = (1..=30).map(|i| format!("Line {i}")).collect::<Vec<_>>().join("\n");
        let (_, page1) = note_layout(&font, note_id, &long_note, 0, four_blanks.body_y);
        assert_eq!(page1, NoteWindow { note: note_id, first: 0, shown: 13, total: 30 });
        assert_eq!(page1.next_page(), Some(13));
        let (_, page2) = note_layout(&font, note_id, &long_note, 13, four_blanks.body_y);
        assert_eq!(page2, NoteWindow { note: note_id, first: 13, shown: 13, total: 30 });
        assert_eq!(page2.next_page(), Some(17));
        let (_, page3) = note_layout(&font, note_id, &long_note, 17, four_blanks.body_y);
        assert_eq!(page3, NoteWindow { note: note_id, first: 17, shown: 13, total: 30 });
        assert_eq!(page3.next_page(), None);
    }

    #[test]
    fn spawn_menu_page_counts_welcome_blank_lines_once_for_notes_and_rows() {
        fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(6 + data.len());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(data.len() as u16).to_le_bytes());
            out.extend_from_slice(data);
            out
        }
        fn rec(sig: &[u8; 4], id: u32, subs: &[Vec<u8>]) -> Vec<u8> {
            let body: Vec<u8> = subs.iter().flat_map(|s| s.iter().copied()).collect();
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&[0u8; 8]);
            out.extend_from_slice(&body);
            out
        }
        fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(b"GRUP");
            out.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
            out.extend_from_slice(sig);
            out.extend_from_slice(&[0u8; 12]);
            out.extend_from_slice(body);
            out
        }

        let term_id = FormId(0x0000_0D70);
        let note_id = FormId(0x0000_0D71);
        let welcome_desc = "Welcome line 1\r\nWelcome line 2\r\n\r\n\r\n\r\n";
        let note_body = (1..=20).map(|i| if i % 5 == 0 { String::new() } else { format!("Note row {i}") }).collect::<Vec<_>>().join("\r\n");

        let mut bytes = rec(b"TES4", 0, &[sub(b"HEDR", &[0, 0, 0x80, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0])]);
        bytes.extend(top(
            b"TERM",
            &rec(
                b"TERM",
                term_id.0,
                &[
                    sub(b"EDID", b"BlankWelcomeTerm\0"),
                    sub(b"FULL", b"Test Terminal\0"),
                    sub(b"DESC", [welcome_desc.as_bytes(), &[0]].concat().as_slice()),
                    sub(b"DNAM", &[0, 2, 8, 0]),
                ],
            ),
        ));
        bytes.extend(top(
            b"NOTE",
            &rec(
                b"NOTE",
                note_id.0,
                &[
                    sub(b"EDID", b"LongTerminalNote\0"),
                    sub(b"FULL", b"Long Note\0"),
                    sub(b"DATA", &[1]),
                    sub(b"TNAM", [note_body.as_bytes(), &[0]].concat().as_slice()),
                ],
            ),
        ));

        let plugins = vec![fnv_esm::Plugin::parse(&bytes, "TermLayout.esm".into()).unwrap()];
        let lo = fnv_esm::LoadOrder::new(&plugins).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let schema = fnv_esm::Schema::load_dir(root.join("fnv")).unwrap();
        let functions = root.join("script/functions.json");
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&functions).unwrap()).unwrap();
        let forms = fnv_sim::FormStore::build(&lo, &schema);
        let refs = fnv_sim::RefTable::build(&lo, &schema);
        let data = fnv_game::GameData::build(&lo, &schema, fnv_script::FunctionTable::load(&functions).unwrap(), &json, &refs);
        let game = fnv_game::Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());

        let font = plain_font(LINE, CHAR);
        let faces = Faces {
            text: Face { font: font.clone(), image: None, vector: None },
            lit: Face { font: font.clone(), image: None, vector: None },
            background: None,
            scanlines: None,
            scan_kept: 1.0,
        };
        let (_, expected_welcome) = welcome_layout(&font, welcome_desc);

        // 1. Open note view: drawn.note gets the full 13 lines (not 9).
        let mut world = World::new();
        let camera = world.spawn_empty().id();
        let note_session = Session {
            reference: FormId(0x0000_0D72),
            base: term_id,
            page: Page::Menu(MenuView {
                terminal: term_id,
                parents: Vec::new(),
                items: vec![0],
                labels: vec!["Read Note".into()],
                selected: 0,
                hidden: false,
                result: String::new(),
                note: Some(note_id),
                scroll: 0,
                server: 8,
                welcome: welcome_desc.into(),
            }),
        };
        let drawn_note = {
            let mut commands = world.commands();
            spawn(&mut commands, camera, &faces, &note_session, &game, true, None, f32::INFINITY)
        };
        assert_eq!(drawn_note.note, Some(NoteWindow { note: note_id, first: 0, shown: 13, total: 20 }));

        // 2. Menu list view: the first menu row starts directly at expected_welcome.body_y.
        let list_session = Session {
            reference: FormId(0x0000_0D72),
            base: term_id,
            page: Page::Menu(MenuView {
                terminal: term_id,
                parents: Vec::new(),
                items: vec![0, 1],
                labels: vec!["First Entry".into(), "Second Entry".into()],
                selected: 0,
                hidden: false,
                result: String::new(),
                note: None,
                scroll: 0,
                server: 8,
                welcome: welcome_desc.into(),
            }),
        };
        let drawn_list = {
            let mut commands = world.commands();
            spawn(&mut commands, camera, &faces, &list_session, &game, true, None, f32::INFINITY)
        };
        assert_eq!(drawn_list.rows.len(), 2);
        assert_eq!(drawn_list.rows[0].1.min.y, expected_welcome.body_y);
        assert_eq!(drawn_list.rows[1].1.min.y, expected_welcome.body_y + LIST_ROW);
    }
}
