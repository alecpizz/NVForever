//! T5 part C: the tiles mods added to `HUDMainMenu`, drawn over NVForever's own HUD
//! [design: ui-tile-tree.md §6.1; what the drawing traits are taken to mean:
//! docs/spec-notes/menu-xml.md §3.1.2].
//!
//! - Which tiles: [`fnv_ui::Ui::mod_added_tiles`] of the HUD, laid out by
//!   [`super::layout`], each item kept only when [`fnv_ui::Ui::mod_tile_drawn`] says it is
//!   the mod's own. Code-made copies (`fnv_game::ui_hud::copies`) never draw. A loose
//!   replacement of the HUD file (or of a prefab it includes) draws the file tiles the
//!   archived copy under it lacks (`fnv_vfs::Vfs::read_archived`,
//!   [`fnv_ui::Ui::archived_name_paths`], read once) [design: #975]; `fnvtool ui
//!   mod-tiles` shows the same choice.
//! - Units: the tree's places are the HUD's interface units, 960 to the view's height
//!   at every size, so `screen()` is 960 × height/width wide [documented: hud.md §14;
//!   `fnv_game::ui_hud::write`]; [`crate::interface::u`] turns them into the window's
//!   pixels (height / 960 a unit), as the current HUD does.
//! - With `[UI] TileMenus=HUDMainMenu` (T12a, `super::vanilla`) the whole menu draws
//!   instead ([`Select::Whole`]), with `zoom`, `tile`, any `texatlas` and the compass
//!   strip ([`Fit`], [`Source`]; menu-xml.md §3.1.3).
//! - The pieces are worked out every frame (a pure function of the tree, [`pieces`]) and
//!   compared with the last frame's, piece by piece, each in its own slot node [design:
//!   the tree's change count moves on every write, so it cannot tell]. A piece only moved
//!   or recoloured (the compass marks while the player turns, the fades and flashes) has
//!   its nodes moved and recoloured in place (T12d); one of another look or size is
//!   spawned again in its slot alone; a piece that comes or goes (a fade reaching 0)
//!   spawns or removes its slot ([`change`]; maintainer's review of #902) [design].
//! - A `tile` picture is one image of its repeats baked at its texels' resolution and
//!   stretched over its box ([`crate::repeat`]), so the bars' ticks stand evenly at any
//!   window size (T12d) [design].

use super::layout::{self, Alignment, Kind, Rect as TileRect};
use super::model;
use super::vanilla::{drawn_line, kept_pieces, TileMenus, KEPT_PIECES, STRIP_TILE};
use crate::compass_strip::{StripNode, StripTexels, StripWindow};
use crate::hud::{strip_left, HudView, STRIP};
use crate::interface::{self, at, bc_supported, face, plain_font, set_marked, u, Face, Pictures, SizedPicture};
use crate::repeat::{self, Repeats};
use crate::script::ScriptState;
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use fnv_font::Font;
use fnv_ui::{TileId, Ui, Value};
use nvf_assets::GameData;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The menu drawn from: the HUD's file names it so [documented: hud_main_menu.xml].
pub const HUD_MENU: &str = "HUDMainMenu";

/// Over NVForever's HUD (its root has no stacking level, 0), the sneak gauges (5) and the
/// corner and quest messages (50); under the dialogue menu and the Pip-Boy (90), which
/// hide the HUD anyway [design].
pub const TILES_Z: i32 = 60;

/// A tile's `font` as the INI's font number: 1 to 9 as written; none, 0 or any other
/// number font 1 [hypothesis: the sneak word's tile has no `font`, and its observed widths,
/// 106 for "[HIDDEN]", 136 (the next word) and 14 (a space), are font 1's advances
/// summed, hud.md §14.1; observed 2026-10-29 on the owner's font files].
pub fn font_number(font: f64) -> u8 {
    let n = font.round();
    if (1.0..=9.0).contains(&n) {
        n as u8
    } else {
        1
    }
}

/// Resolve a tile's `font` number against loaded `xfonts` extended slots (`10..=255`).
///
/// Vanilla slots `1..=9` stay as written (preserving unloaded font `9`); registered
/// extended slots `10..=255` resolve to their slot number (e.g. MAPMO `21`–`22`); and `0`,
/// negative/non-finite values, or unregistered extended IDs (e.g. `<font>42</font>` when no
/// `xfonts` file defines slot `42`) fall back to font `1` without panicking or routing to
/// unloaded font `9` [design; hypothesis: original-game fallback for unregistered extended
/// IDs is not directly captured; fonts.md §4.1].
pub fn resolve_font_number(font: f64, extra: &BTreeMap<u8, String>) -> u8 {
    let n = font.round();
    if !n.is_finite() {
        return 1;
    }
    if (1.0..=9.0).contains(&n) {
        n as u8
    } else if (10.0..=255.0).contains(&n) {
        let slot = n as u8;
        if extra.contains_key(&slot) {
            slot
        } else {
            1
        }
    } else {
        1
    }
}

/// A text's width and height in interface units in `font`, the face of font number
/// `number` ([`font_number`]), set as `wrap` allows. A `wrapwidth` of 0 or less does not
/// wrap [observed: the prompt's `button_text` has `wrapwidth` 0 and the original measured
/// "Button Text" 156 wide, one line of font 7, hud.md §14.1]. The height is the font's
/// tallest glyph picture ([`tallest_glyph`]) and one line pitch ([`line_pitch`]) for each
/// line after the first [observed: hud_menus capture 2026-10-31, the text tiles' `height`
/// in fonts 1-8 at one to three lines, fonts.md §3.1]; an empty string is one line high.
pub fn measure(font: &Font, number: u8, text: &str, wrap: Option<f64>) -> (f64, f64) {
    let (width, height, _) = dimensions(font, number, text, wrap);
    (width, height)
}

/// The renderer's text layout including its actual line count [design: shared by
/// GetStringUIDimensions; height includes glyph extents and is not a line counter].
pub fn dimensions(font: &Font, number: u8, text: &str, wrap: Option<f64>) -> (f64, f64, f64) {
    let lines = font.lines(text, wrap.filter(|w| *w > 0.0).map(|w| w as f32));
    let width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
    let height = tallest_glyph(font) + lines.len().saturating_sub(1) as f32 * line_pitch(number, font);
    (f64::from(width), f64::from(height), lines.len() as f64)
}

/// Owned metrics, independent of font atlas uploads. Failed reads are retried at most
/// once a second when queried; successful fonts are parsed once [design].
#[derive(Default)]
struct StringMetrics {
    fonts: HashMap<u8, Font>,
    retry_after: HashMap<u8, std::time::Duration>,
}

impl StringMetrics {
    fn measure(&mut self, number: u8, text: &str, wrap: f64, now: std::time::Duration, load: &impl Fn(u8) -> Option<Font>) -> (f64, f64, f64) {
        if !self.fonts.contains_key(&number) && self.retry_after.get(&number).is_none_or(|&after| now >= after) {
            if let Some(font) = load(number) {
                self.fonts.insert(number, font);
                self.retry_after.remove(&number);
            } else {
                self.retry_after.insert(number, now + std::time::Duration::from_secs(1));
            }
        }
        self.fonts.get(&number).map_or((0.0, 0.0, 0.0), |f| dimensions(f, number, text, Some(wrap)))
    }
}

/// A session's owned font source and metrics for script queries. No Bevy resources or
/// texture handles enter the callback [design]. All font IDs use the renderer's selector.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn string_measure(load: impl Fn(u8) -> Option<Font> + Send + Sync + 'static) -> fnv_game::ui_more::Measure {
    string_measure_with_fonts(BTreeMap::new(), load)
}

/// [`string_measure`] with an `xfonts` extended slot map (`interface::load_xfonts`) so
/// script `GetStringUIDimensions` calls resolve registered extended slots and fall back
/// cleanly for `<font>0` and unregistered extended IDs [design].
pub(crate) fn string_measure_with_fonts(
    extra: BTreeMap<u8, String>,
    load: impl Fn(u8) -> Option<Font> + Send + Sync + 'static,
) -> fnv_game::ui_more::Measure {
    let cache = std::sync::Mutex::new(StringMetrics::default());
    let start = std::time::Instant::now();
    Box::new(move |font, text, wrap| {
        let number = resolve_font_number(font, &extra);
        cache.lock().map_or((0.0, 0.0, 0.0), |mut cache| cache.measure(number, text, wrap, start.elapsed(), &load))
    })
}

/// A text tile's size as the code writes it into a tile-drawn menu (T15a,
/// `super::menus::write_text_sizes`): [`measure`], an empty string one space wide [observed:
/// hud_menus capture 2026-10-31, the empty `MM_Title` (font 6) 14 x 38 at three sizes, font
/// 6's space advancing 14 and its tallest glyph 38; hypothesis: the rule]. Only the message
/// box's title is measured empty (`write_text_sizes` gives other empty texts 0 x 0).
pub fn measure_written(font: &Font, number: u8, text: &str, wrap: Option<f64>) -> (f64, f64) {
    let (width, height) = measure(font, number, text, wrap);
    if text.is_empty() {
        (f64::from(font.width(" ")), height)
    } else {
        (width, height)
    }
}

/// The tallest glyph picture of a font (glyph number 10, fonts.md §2): what a one-line
/// text tile measures high in the original [observed: hud_textbase and hud_menus captures
/// 2026-10-31: 36, 32, 31, 37, 28, 38, 42, 64 for fonts 1-8, each its file's tallest]. A
/// font with no pictures (the stand-in, `interface::plain_font`) its size [design].
pub fn tallest_glyph(font: &Font) -> f32 {
    let tallest = font.glyphs.iter().map(|g| g.height).fold(0.0, f32::max);
    if tallest > 0.0 {
        tallest
    } else {
        font.size
    }
}

/// The game's fonts by number, read when first asked for (the renderer and the text widths
/// written back share them).
#[derive(Resource, Default)]
pub struct TileFonts {
    faces: HashMap<u8, Face>,
    /// Extended `NVSE\Plugins\xfonts\*.txt` slots discovered once when `GameData` is
    /// available [design: fonts.md §4.1].
    extra: Option<BTreeMap<u8, String>>,
    /// The faces' fonts, owned, for the text widths a frame keeps back
    /// ([`fnv_game::ui_hud::defer`]); made again when a face is read.
    measure: Option<fnv_game::ui_hud::Measure>,
    /// What each notice text copy's width was last measured from
    /// ([`super::notices::write_widths`]).
    notice_widths: super::notices::WidthsWritten,
    /// What each mod-added HUD text's width was last measured from: its string and wrap
    /// ([`FontKit::write_mod_widths`]).
    mod_widths: HashMap<TileId, (String, f64)>,
}

#[derive(SystemParam)]
pub(crate) struct FontKit<'w> {
    fonts: ResMut<'w, TileFonts>,
    data: Option<Res<'w, GameData>>,
    images: ResMut<'w, Assets<Image>>,
    formats: Option<Res<'w, CompressedImageFormatSupport>>,
    /// The pictures the code made (`super::runtime_images`).
    runtime: Option<Res<'w, super::runtime_images::RuntimeImages>>,
}

/// Path passed to [`interface::face`] for resolved slot `n`: vanilla slots `1..=9` pass the
/// raw [`interface::FONT_FILES`] entry so `face()` runs `is_unloaded_font` first (keeping
/// Font 9 unloaded even if an INI sets `sFontFile_9`) and resolves `font_file()` exactly
/// once (preserving INI font swaps without double indirection); extended slots `>= 10` look
/// up `extra` directly [design].
fn face_slot_path(slot: u8, extra: Option<&BTreeMap<u8, String>>) -> &str {
    if (1..=9).contains(&slot) {
        interface::FONT_FILES[usize::from(slot) - 1]
    } else {
        extra
            .and_then(|map| map.get(&slot))
            .map(String::as_str)
            .unwrap_or(interface::FONT_FILES[0])
    }
}

impl FontKit<'_> {
    fn ensure_xfonts(&mut self) {
        if self.fonts.extra.is_none() {
            if let Some(data) = self.data.as_deref() {
                self.fonts.extra = Some(interface::load_xfonts(&data.vfs));
            }
        }
    }

    pub(crate) fn font_number(&self, font: f64) -> u8 {
        match self.fonts.extra.as_ref() {
            Some(extra) => resolve_font_number(font, extra),
            None => font_number(font),
        }
    }

    pub(crate) fn load(&mut self, font: f64) {
        self.ensure_xfonts();
        let n = self.font_number(font);
        if !self.fonts.faces.contains_key(&n) {
            // Only when a face is made: entries in a `profile hitch` line are the faces made in
            // that frame, whether read or the stand-in (`interface::face`; a tile-drawn menu's
            // own first use in `draw` counts here too, outside `tiles::write`).
            let _span = info_span!("tiles::write::font_load").entered();
            let bc = bc_supported(self.formats.as_deref());
            let path = face_slot_path(n, self.fonts.extra.as_ref());
            let f = face(path, plain_font(31.0, 15.0), self.data.as_deref(), &mut self.images, bc);
            self.fonts.faces.insert(n, f);
            self.fonts.measure = None;
        }
    }

    pub(crate) fn face(&self, font: f64) -> Option<&Face> {
        self.fonts.faces.get(&self.font_number(font))
    }

    pub(crate) fn images_mut(&mut self) -> &mut Assets<Image> {
        &mut self.images
    }

    /// The faces made so far, the game's fonts and stand-ins alike
    /// (`NVF_PLAY_TEST_TILE_COUNTS`).
    pub(crate) fn faces_made(&self) -> usize {
        self.fonts.faces.len()
    }

    /// The hand-built HUD's corner label places (`crate::hud::label_places`) in font 7, the
    /// HUD's [documented: hudtemplates.xml's text templates], and how far a text tile's
    /// first line stands below its top in font 7 ([`text_base`]): what a text tile's `y`
    /// is above the line.
    pub(crate) fn hud_labels(&mut self) -> ([(f32, f32); 4], f32) {
        self.load(7.0);
        match self.face(7.0) {
            Some(face) => (crate::hud::label_places(&face.font), text_base(7)),
            None => (crate::hud::label_places(&plain_font(31.0, 15.0)), text_base(7)),
        }
    }

    /// A text's width in font 7, as the tile layout measures it ([`measure`]).
    pub(crate) fn hud_width(&mut self, text: &str) -> f32 {
        self.load(7.0);
        self.face(7.0).map_or(0.0, |face| measure(&face.font, 7, text, None).0 as f32)
    }

    /// Font 7's lines for a text at a wrap width, as [`measure`] sets them, and the font's
    /// size (the HUD's texts, `super::notices`, which stack the quest objectives a size and
    /// the hand-built HUD's leading apart: 36 + 2, font 7's observed line pitch 38,
    /// [`line_pitch`]).
    pub(crate) fn hud_lines(&mut self) -> impl Fn(&str, Option<f64>) -> (usize, f64) + '_ {
        self.load(7.0);
        let font = self.face(7.0).map(|f| &f.font);
        move |text: &str, wrap: Option<f64>| match font {
            Some(f) => (f.lines(text, wrap.filter(|w| *w > 0.0).map(|w| w as f32)).len(), f64::from(f.size)),
            None => (1, 0.0),
        }
    }

    /// The measure of the HUD text tiles' widths ([`fnv_game::ui_hud::write_text_widths`]);
    /// only with the game's own fonts, a stand-in font would write made-up widths, so
    /// `None` when a text tile's font is not read [design]. The text tiles' fonts are the
    /// same before and after [`fnv_game::ui_hud::write`], which sets no fonts.
    pub(crate) fn widths(&mut self, ui: &Ui) -> Option<fnv_game::ui_hud::Measure> {
        if !self.game_fonts(ui, &fnv_game::ui_hud::measured_texts(ui)) {
            return None;
        }
        if self.fonts.measure.is_none() {
            let owned: HashMap<u8, Font> = self.fonts.faces.iter().map(|(&n, face)| (n, face.font.clone())).collect();
            let extra = self.fonts.extra.clone().unwrap_or_default();
            self.fonts.measure = Some(std::sync::Arc::new(move |font: f64, text: &str| {
                let n = resolve_font_number(font, &extra);
                owned.get(&n).map_or(0.0, |f| measure(f, n, text, None).0)
            }));
        }
        self.fonts.measure.clone()
    }

    /// Write the HUD text tiles' measured widths into the tree ([`FontKit::widths`]).
    pub(crate) fn write_widths(&mut self, ui: &mut Ui) {
        if let Some(measure) = self.widths(ui) {
            fnv_game::ui_hud::write_text_widths(ui, &*measure);
        }
        self.write_mod_widths(ui);
    }

    /// The widths of the text tiles mods add to the HUD (`fnv_game::ui_hud::unmeasured_texts`),
    /// each measured in its own font when that font is read, and only when its string or
    /// wrap changed. The original sets every text tile's `width` to its measured size; a mod
    /// that centres a text by its own width read 0 here, so MAPMO's HUD clock (x = (dial
    /// width − text width) / 2 from its place) ran off the right edge of the view
    /// [observed 2026-11-21 in FadedLight's Nuclear Sunset fork, #4493].
    fn write_mod_widths(&mut self, ui: &mut Ui) {
        // The notices' copies have their own measure (`write_notice_widths`).
        let notices: std::collections::HashSet<TileId> = super::notices::texts(ui).into_iter().collect();
        let tiles: Vec<TileId> = fnv_game::ui_hud::unmeasured_texts(ui).into_iter().filter(|t| !notices.contains(t)).collect();
        let present: std::collections::HashSet<TileId> = tiles.iter().copied().collect();
        self.fonts.mod_widths.retain(|id, _| present.contains(id));
        for tile in tiles {
            let t = ui.tile(tile);
            let Some(string) = t.get("string").map(Value::text) else { continue };
            let font = t.get("font").map_or(0.0, Value::num);
            let wrap = t.get("wrapwidth").map_or(0.0, Value::num);
            if self.fonts.mod_widths.get(&tile).is_some_and(|(s, w)| *s == string && *w == wrap) {
                continue;
            }
            self.load(font);
            let Some(face) = self.face(font).filter(|f| f.image.is_some()) else { continue };
            let (width, _) = measure(&face.font, self.font_number(font), &string, Some(wrap).filter(|w| *w > 0.0));
            ui.set_trait(tile, "width", Value::Num(width));
            ui.set_trait(tile, fnv_game::ui_hud::MEASURED_MARK, Value::Num(1.0));
            self.fonts.mod_widths.insert(tile, (string, wrap));
        }
    }

    /// Write the tile HUD's notice texts' widths (`super::notices::write_widths`), after
    /// `super::notices::write` has set their strings; only with the game's own fonts, as
    /// [`FontKit::write_widths`].
    pub(crate) fn write_notice_widths(&mut self, ui: &mut Ui) {
        if !self.game_fonts(ui, &super::notices::texts(ui)) {
            return;
        }
        let mut last = std::mem::take(&mut self.fonts.notice_widths);
        super::notices::write_widths(ui, &mut last, &|font, text, wrap| self.face(font).map_or(0.0, |f| measure(&f.font, self.font_number(font), text, wrap).0));
        self.fonts.notice_widths = last;
    }

    /// The measure of a menu's texts drawn from tiles (T15a, `super::menus`), its text
    /// tiles' `fonts` (`super::menus::fonts`) loaded first: a text's width and height as
    /// the renderer sets it ([`measure`]). A font not read measures by its stand-in, as it is drawn [design:
    /// the menu's places follow from these sizes, so a box is still laid out].
    pub(crate) fn menu_measure(&mut self, fonts: &[f64]) -> impl Fn(f64, &str, Option<f64>) -> (f64, f64) + '_ {
        self.ensure_xfonts();
        for &font in fonts {
            self.load(font);
        }
        let faces = &self.fonts.faces;
        let extra = self.fonts.extra.as_ref();
        move |font: f64, text: &str, wrap: Option<f64>| {
            let n = extra.map_or_else(|| font_number(font), |e| resolve_font_number(font, e));
            faces.get(&n).map_or((0.0, 0.0), |face| measure_written(&face.font, n, text, wrap))
        }
    }

    /// A picture's own file's size in texels, read once by the one who asks (the tutorial
    /// menu's symbol, `super::tutorial_menu::write_symbol`).
    pub(crate) fn picture_size(&mut self, path: &str) -> Option<Vec2> {
        let bc = bc_supported(self.formats.as_deref());
        let data = self.data.as_deref();
        Pictures::load(data).own_sized(path, data, &mut self.images, bc).map(|(_, _, size)| size)
    }

    /// Whether the fonts of `tiles` are all the game's own (read, not stand-ins), loading
    /// them; false without game data or without tiles.
    fn game_fonts(&mut self, ui: &Ui, tiles: &[TileId]) -> bool {
        if self.data.is_none() || tiles.is_empty() {
            return false;
        }
        let fonts: Vec<f64> = tiles.iter().map(|&t| ui.tile(t).get("font").map_or(0.0, Value::num)).collect();
        for &f in &fonts {
            self.load(f);
        }
        fonts.iter().all(|&f| self.face(f).is_some_and(|face| face.image.is_some()))
    }
}

/// How an image fills its box [what the traits are taken to mean: menu-xml.md §3.1.3].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// Stretched over the box: `zoom` -1 [documented: GECK wiki Zoom, "free scaling …
    /// regardless its source size"] or a constant such as `&scale;` [hypothesis: its
    /// pictures, the compass ticks and the condition arrows, are the size of their boxes];
    /// every picture of the mods' tiles with the switch off (T5).
    Stretch,
    /// No `zoom` (T12c, [`placed`]): an atlas piece stretched over its box, a picture read
    /// from its own file at its texels' size, zoom's default 100 [documented: GECK wiki
    /// Zoom, "The default value is 100"; hypothesis: the corner brackets' 255 x 127 atlas
    /// pieces fill their 512 x 256 boxes and the radiation pointer's 63 x 31 its 46 x 24
    /// box, the radiation bracket's own 512 x 128 file stands unstretched in its 567 x 120
    /// box, on the original's screenshots, menu-xml.md §3.1.3].
    Natural,
    /// A number: the picture at its texels' size times `zoom` / 100 (one texel one
    /// interface unit at 100), from the box's top left, cut to the box [documented: GECK
    /// wiki Zoom, a multiplier of the picture's size that "cannot exceed the specified
    /// height and width"; hypothesis: one texel a unit, as the 1024-texel compass strip
    /// turns 1024 units, hud.md §14.5, and the top-left anchor].
    Zoom(f64),
    /// `tile` true: the picture repeated over the box from its top left at its texels'
    /// size times this `zoom` / 100 (100 without a numeric zoom) [hypothesis: by the
    /// trait's name; the bars' 8-texel tick picture in boxes the code sizes in steps of 8,
    /// hud.md §14.3; zoom as the multiplier of the picture's size, GECK wiki Zoom].
    Tiled(f64),
    /// `tile` `&scale;` (the map's dotted line, `PathLineTemplate`, the one vanilla use): the
    /// picture scaled to this height (the box's), its shape kept, and repeated along the box
    /// [observed: #4685, the original's World Map line is a row of whole dots about the line's
    /// height apart; `&scale;` is the number 0, so it is not a true `tile`, and read as one
    /// the 64 x 16 picture stood once at its texels' size, its dot cut to a sliver].
    TiledToHeight(f64),
    /// The compass strip: twice side by side, moved across its window by the heading
    /// (nothing in the tree moves it, hud.md §14.4) at 1024 units a turn with its N at
    /// x 154 of the window at heading 0, and fading out at the window's right end
    /// [observed: hud.md §14.5, T12e; design: `hud::strip_left`, `crate::compass_strip`,
    /// the hand-built HUD's own].
    Strip,
}

/// Where an image's picture is read from (`interface::Pictures`).
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Its piece of the shared atlas when the shared index has its file name, else its
    /// own file: every picture of the mods' tiles with the switch off, as T5 reads them.
    Shared,
    /// The atlas its `texatlas` names (`Pictures::locate`'s order).
    Index(String),
    /// No `texatlas`, with the whole menu drawn: its own file [observed: the radiation
    /// bracket, whose `texatlas` line the file comments out (radiation.md §6), shows its
    /// own file's slanted ends and single line on the original's screenshots, not the
    /// shared atlas piece's square ends and doubled line; menu-xml.md §3.1.3].
    Own,
    /// A `filename` with the runtime prefix (`super::runtime_images::PREFIX`): a texture
    /// the code made, looked up before any atlas or file, stamped with its entry's revision
    /// and size as the pieces are painted (`super::runtime_images::mark`; none from
    /// [`pieces`], none while no entry is set), so a new picture under the same name
    /// repaints [design: menu-xml.md §3.1.5].
    Runtime(Option<super::runtime_images::Stamp>),
}

/// What one drawn tile looks like.
#[derive(Clone, Debug, PartialEq)]
pub enum Look {
    /// A `rect` with its own `red`, `green` or `blue`: a filled box.
    Box,
    /// An `image`'s `filename`, read as `source` says, filling its box as `fit` says.
    Picture { name: String, source: Source, fit: Fit },
    /// At the font's own size, whatever the tile's `zoom` ([`layout::Text::zoom`]).
    Text { string: String, font: u8, wrap: Option<f64>, alignment: Alignment },
    /// A `nif` tile's model, playing its `animation` (`super::model`), with the whole menu
    /// drawn only. Where it stands in its sequence is read by `super::model::animate`, so
    /// the piece stays the same as it plays.
    Model { filename: String, animation: Option<String> },
}

/// Where a picture's texture stands in its box, in interface units from the box's top
/// left ([`placed`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placed {
    /// Stretched over the box.
    Fill,
    /// This size, cut to the box.
    At { width: f64, height: f64 },
    /// Repeated over the box at this size.
    Repeat { width: f64, height: f64 },
    /// The compass strip ([`Fit::Strip`]).
    Strip,
}

/// How a picture of `texels` (its atlas piece's or its own file's size) stands in its
/// box: `piece` says it was found in an atlas [what the traits are taken to mean: [`Fit`]].
pub fn placed(fit: Fit, piece: bool, texels: (f64, f64)) -> Placed {
    let at = |z: f64| (texels.0 * z / 100.0, texels.1 * z / 100.0);
    match fit {
        Fit::Stretch => Placed::Fill,
        Fit::Natural if piece => Placed::Fill,
        Fit::Natural => Placed::At { width: texels.0, height: texels.1 },
        Fit::Zoom(z) => {
            let (width, height) = at(z);
            Placed::At { width, height }
        }
        Fit::Tiled(z) => {
            let (width, height) = at(z);
            Placed::Repeat { width, height }
        }
        Fit::TiledToHeight(h) if texels.1 > 0.0 => {
            let (width, height) = at(h / texels.1 * 100.0);
            Placed::Repeat { width, height }
        }
        Fit::TiledToHeight(_) => Placed::Fill,
        Fit::Strip => Placed::Strip,
    }
}

/// How far a text tile's first line stands under its top in font number `font`
/// ([`font_number`]), the line a glyph's `ascent` is measured from
/// (`interface::set_marked`); the next lines one [`line_pitch`] apart. One number per
/// font [observed: the capitals' ink under the tile's `y` in the original, hud_textbase
/// capture 2026-10-31 (fonts 1, 7, 8: hud.md §14.10) and hud_menus capture 2026-10-31
/// (fonts 1-8 at three window sizes: fonts.md §3.1); "+9" and "RAD/SEC", both at
/// y 65, are not on one line]. The fonts' glows are as deep under the letters as over
/// them, so the middle of a text's ink is where it is whatever edge the eye takes; the
/// glyph pictures put that middle a known height over this line (fonts.md §3), so each
/// capture gives each font's line under the top: font 7 24.5-24.9, font 8 30.5, font 1
/// 20.3-21.0, fonts 2-5 about 16.5, font 6 22.4 (±0.5-0.7).
///
/// [design: NVForever's calibration] Font 7 keeps 24, the earlier captures' 23.7-25.5
/// (menu-xml.md §3.1.2), and every other font stands as far from it as the hud_menus
/// capture shows (one capture, one font-7 reading for all): font 1 19.8 → 20, fonts 2-5
/// 16.0 → 16, font 6 21.9 → 22, font 8 30.0. Font 8 was 29.5 from hud_textbase (its 30.5
/// against that capture's font 7 at 24.9, 5.6 lower); hud_menus measures it 6.0 lower
/// (font 7 at 24.5). Both captures read font 8 at 30.5 itself; their mean distance, 5.8,
/// is nearer 30 than 29.5, and 30 keeps the table on one capture's font 7. No font
/// measure gives the lines (the size less the deepest glyph's reach under the line: 21,
/// 24, 37 for fonts 1, 7, 8; fonts.md §3).
pub const fn text_base(font: u8) -> f32 {
    match font {
        1 => 20.0,
        2..=5 => 16.0,
        6 => 22.0,
        8 => 30.0,
        // Font 7; and font 9, which draws nothing in the original (its texture is not in
        // the archives [observed: hud_menus capture 2026-10-31, fonts.md §3.1]) and here is
        // drawn in the stand-in font (`interface::face`), on font 7's line [design].
        _ => 24.0,
    }
}

/// First-line baseline under a text tile's top for slot `number` and parsed face `font`.
///
/// Vanilla slots `1..=9` keep [`text_base`] bit-for-bit unchanged. Extended `xfonts` slots
/// (`>= 10`) use the tallest ASCII glyph ascent (at least `0.75 * font.size`, matching
/// `interface::line_base`) so large modded fonts (such as MAPMO's 76.7-unit HUD fonts) do
/// not clip above the tile top [design: fonts.md §4.1].
pub fn text_base_for(number: u8, font: &Font) -> f32 {
    if (1..=9).contains(&number) {
        text_base(number)
    } else {
        font.glyphs
            .iter()
            .skip(0x20)
            .take(0x5F)
            .filter(|g| g.drawn())
            .map(|g| g.ascent)
            .fold(0.0, f32::max)
            .max(font.size * 0.75)
    }
}

/// How far apart a text's lines stand in font number `number` ([`font_number`]), whose face
/// is `font`. One number per font [observed: hud_menus capture 2026-10-31: a two-line text
/// tile's `height` less a one-line one's, the same at three window sizes, and three lines
/// in fonts 4, 7 and 8; the ink's line-to-line distance agrees, fonts.md §3.1]: 31, 35, 32,
/// 36, 25, 38, 38, 52 for fonts 1-8. For fonts 2, 3, 6 and 7 that is not the file's size
/// (31, 26, 34, 36) and no number of the files gives all eight [observed]; why is not
/// known. Any other number (font 9, no texture) its file's size [hypothesis].
pub fn line_pitch(number: u8, font: &Font) -> f32 {
    match number {
        1 => 31.0,
        2 => 35.0,
        3 => 32.0,
        4 => 36.0,
        5 => 25.0,
        6 | 7 => 38.0,
        8 => 52.0,
        _ => font.size,
    }
}

/// The lines of a text tile as they are set in its box (`width` wide before the text's
/// zoom): each line, its x by the tile's justification [hypothesis] and its baseline in
/// font number `number` ([`text_base`]), the lines one [`line_pitch`] apart.
pub fn text_lines(font: &Font, number: u8, string: &str, wrap: Option<f64>, alignment: Alignment, width: f64) -> Vec<(fnv_font::Line, f32, f32)> {
    let (base, pitch) = (text_base_for(number, font), line_pitch(number, font));
    let width = width as f32;
    font.lines(string, wrap.map(|w| w as f32))
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let x = match alignment {
                Alignment::Left => 0.0,
                Alignment::Center => (width - line.width) / 2.0,
                Alignment::Right => width - line.width,
            };
            (line, x, base + i as f32 * pitch)
        })
        .collect()
}

/// A text piece's lines as the renderer sets them in its box ([`text_lines`]), in its
/// font `font` (the face of the piece's font number) and on that number's line
/// ([`text_base`]); none for a piece that is not a text.
pub fn piece_lines(piece: &Piece, font: &Font) -> Vec<(fnv_font::Line, f32, f32)> {
    match &piece.look {
        Look::Text { string, font: number, wrap, alignment } => text_lines(font, *number, string, *wrap, *alignment, piece.rect.width),
        _ => Vec::new(),
    }
}

/// One tile to draw, in interface units from the view's top left.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub tile: TileId,
    pub look: Look,
    pub rect: TileRect,
    pub clip: Option<TileRect>,
    /// sRGB and alpha, 0 to 1.
    pub colour: [f32; 4],
    pub depth: f64,
    /// A picture's SetUITexOffset offset and SetUIAlphaMap mask, put on by
    /// [`super::texmod::mark`] as the pieces are painted (#1833); none from [`pieces`].
    pub tex: Option<super::texmod::TexMod>,
    /// Its tile's turn ([`layout::Turn`]): a box's or a picture's (not the compass strip's);
    /// texts and models are not turned, as no file turns one [design: menu-xml.md §3.1.4].
    pub turn: Option<layout::Turn>,
    /// A picture's `cropx` and `cropy`: the point of its picture, after its `zoom`, put at its
    /// box's top left [documented: GECK wiki "Tutorial: Understanding XML", "`cropx`, `cropy`
    /// specifies the point of the texture that will be placed at the upper left-hand corner";
    /// "Cropx", "applied after the zoom trait"], interface units; read for a picture drawn at
    /// its texels' size times its zoom ([`Placed::At`]), none otherwise or without either trait
    /// [design: `map_menu.xml`'s map markers crop their icons so, 14 texels times the zoom].
    pub crop: Option<(f64, f64)>,
}

/// Which tiles of the menu draw.
#[derive(Clone, Copy, Debug)]
pub enum Select<'a> {
    /// T5: only the tiles mods added ([`fnv_ui::Ui::mod_added_tiles`], each kept when
    /// [`fnv_ui::Ui::mod_tile_drawn`] accepts it against these vanilla names), every
    /// picture stretched over its box from the shared atlas or its own file, as T5 drew
    /// them.
    /// `by_locus`: laid out by the `locus` rule (`layout::layout_by_locus`; the start
    /// menu's Mods page).
    Mods { vanilla: Option<&'a HashSet<String>>, by_locus: bool },
    /// T12a, `[UI] TileMenus=HUDMainMenu`: every tile of the menu (the file's, the
    /// code-made copies and the mods'), but the file's and the code's under `kept` (the
    /// pieces NVForever still draws by hand, `super::vanilla::KEPT_PIECES`); `zoom`,
    /// `tile` and `texatlas` applied, `strip` scrolled by the heading [design].
    Whole { kept: &'a [TileId], strip: Option<TileId> },
}

/// A system colour by its constant, as the base colour: `None` for the tile's own red,
/// green and blue.
fn system_colour(v: &Value) -> Option<[f32; 3]> {
    let rgb = |c: [u8; 3]| c.map(|x| f32::from(x) / 255.0);
    let alt = || {
        let c = crate::reticle::crime_colour().to_srgba();
        [c.red, c.green, c.blue]
    };
    match v {
        Value::Sym(name) => match name.to_ascii_lowercase().as_str() {
            // [hypothesis: by its name, the HUD's main colour; NVForever draws its HUD in
            // `interface::hud_rgb`, the settings' HUD colour].
            "hudmain" => Some(rgb(interface::hud_rgb())),
            // [observed: Fallout_default.ini `iSystemColorHUDAlt*` 255, 67, 42, as the
            // radiation meter (radiation.md §6) and `reticle::crime_colour`].
            "hudalt" => Some(alt()),
            // [hypothesis: by its name, the Pip-Boy's screen colour].
            "pipboy" => Some(rgb(interface::pipboy_rgb())),
            // [hypothesis: by their names, the tile's own colour].
            "nosystemcolor" | "uselocalcolor" => None,
            // `&terminal;`, `&mainmenu;` and unknown names: the HUD's colour [hypothesis].
            _ => Some(rgb(interface::hud_rgb())),
        },
        // [observed: private jobs hud_jip file_0032, hud_hardcore2 hc4-hc5]
        Value::Num(n) if *n == 1.0 => Some(rgb(interface::hud_rgb())),
        Value::Num(n) if *n == 2.0 => Some(alt()),
        // Other nonzero IDs retain the HUD colour [hypothesis: menu-xml.md §3.1.2].
        Value::Num(n) if *n != 0.0 => Some(rgb(interface::hud_rgb())),
        _ => None,
    }
}

/// A tile's base colour [hypothesis: the order is NVForever's, menu-xml.md §3.1.2]: its own
/// `systemcolor`; else its own non-black `red`/`green`/`blue` when it sets any; else the
/// nearest tile above's `systemcolor` (so child prefabs that copy `red`/`green`/`blue` from
/// a parent without RGB traits — evaluating to 0/0/0 — still inherit an enclosing menu's
/// `systemcolor`, while `&nosystemcolor;` / `&uselocalcolor;` / `0` on the tile or ancestor
/// yields local RGB); else its own `red`/`green`/`blue` when no ancestor sets `systemcolor`;
/// else system colour 1, the HUD's [documented: GECK wiki Color UI directives, "Systemcolor
/// is by default 1 and always assign"; observed: the ammo type's label, a menu child with no
/// `systemcolor` above it, drew white before T12c where the hand-built HUD and the original
/// draw the HUD's texts amber].
pub(super) fn base_colour(ui: &Ui, id: TileId, own: [f32; 3]) -> [f32; 3] {
    let tile = ui.tile(id);
    if let Some(v) = tile.get("systemcolor") {
        return system_colour(v).unwrap_or(own);
    }
    let has_rgb = ["red", "green", "blue"].iter().any(|n| tile.get(n).is_some());
    if has_rgb && own != [0.0, 0.0, 0.0] {
        return own;
    }
    let mut at = tile.parent;
    while let Some(p) = at {
        if let Some(v) = ui.tile(p).get("systemcolor") {
            return system_colour(v).unwrap_or(own);
        }
        at = ui.tile(p).parent;
    }
    if has_rgb {
        return own;
    }
    system_colour(&Value::Num(1.0)).unwrap_or(own)
}

#[cfg(test)]
mod systemcolor_inheritance_tests;

/// How an image fills its box, by its `tile` and `zoom` ([`Fit`]).
pub fn fit(tile: &fnv_ui::Tile) -> Fit {
    let zoom = tile.get("zoom");
    if tile.get("tile").is_some_and(|t| *t == Value::Sym("scale".into())) {
        let h = tile.get("height").map_or(0.0, Value::num);
        if h > 0.0 {
            return Fit::TiledToHeight(h);
        }
    }
    if tile.get("tile").is_some_and(Value::truthy) {
        let z = zoom.filter(|v| !matches!(v, Value::Sym(_))).map(Value::num).filter(|z| *z > 0.0).unwrap_or(100.0);
        return Fit::Tiled(z);
    }
    match zoom {
        None => Fit::Natural,
        Some(Value::Sym(_)) => Fit::Stretch,
        Some(v) if v.num() < 0.0 => Fit::Stretch,
        Some(v) => Fit::Zoom(v.num()),
    }
}

/// Whether `id` is one of `of` or under one.
fn under(ui: &Ui, id: TileId, of: &[TileId]) -> bool {
    let mut at = Some(id);
    while let Some(t) = at {
        if of.contains(&t) {
            return true;
        }
        at = ui.tile(t).parent;
    }
    false
}

fn piece_from_item(ui: &Ui, item: &layout::DrawItem, whole: bool, strip: bool, resolve_font: &dyn Fn(f64) -> u8) -> Option<Piece> {
    if !item.visible || item.alpha <= 0.0 {
        return None;
    }
    let tile = ui.tile(item.tile);
    let look = match &item.kind {
        Kind::Rect if ["red", "green", "blue"].iter().any(|n| tile.get(n).is_some()) => Look::Box,
        // A hotrect a model marked (`layout::HOTRECT_FILL`): its box filled.
        Kind::Hotrect => Look::Box,
        Kind::Image(image) if !image.filename.trim().is_empty() => {
            let fit = match () {
                _ if strip => Fit::Strip,
                _ if whole => fit(tile),
                _ => Fit::Stretch,
            };
            let source = match image.texatlas.as_ref().map(|a| a.text().trim().to_string()).filter(|a| !a.is_empty()) {
                _ if super::runtime_images::is_runtime(&image.filename) => Source::Runtime(None),
                _ if !whole => Source::Shared,
                Some(atlas) => Source::Index(atlas),
                None => Source::Own,
            };
            Look::Picture { name: image.filename.trim().to_string(), source, fit }
        }
        Kind::Text(text) if !text.string.is_empty() => {
            Look::Text { string: text.string.clone(), font: resolve_font(text.font), wrap: text.wrapwidth.filter(|w| *w > 0.0), alignment: text.alignment }
        }
        // A model with the whole menu drawn (T12 leftovers, `super::model`); the mods'
        // nif tiles with the switch off are not drawn, as in T5 [design].
        Kind::Nif(nif) if whole && !nif.filename.trim().is_empty() => Look::Model { filename: nif.filename.trim().to_string(), animation: nif.animation.clone() },
        // Empty rects only place their children; radial and window tiles are not drawn
        // [design].
        _ => return None,
    };
    if item.clip.is_some_and(|c| c.width <= 0.0 || c.height <= 0.0) {
        return None;
    }
    let c = &item.colour;
    let own = [c.red, c.green, c.blue].map(|x| x as f32 / 255.0);
    // `brightness` scales the colour, 255 the colour itself [hypothesis: as the
    // interface's `amber(brightness)`]; clamped to 0..=255 [design].
    let b = (c.brightness.clamp(0.0, 255.0) / 255.0) as f32;
    let [r, g, bl] = base_colour(ui, item.tile, own).map(|x| x * b);
    let mut alpha = (item.alpha / 255.0).clamp(0.0, 1.0) as f32;
    // A faint fill reads as the hand-built highlights' does, whose alpha is put through
    // the same curve (`crate::menu::look`, `crate::message_menu`) [design: not compared
    // with a shot of the original, none shows a highlight yet].
    if matches!(item.kind, Kind::Hotrect) {
        alpha = alpha.powf(2.2);
    }
    let turn = match &look {
        Look::Box => item.turn,
        Look::Picture { fit, .. } if *fit != Fit::Strip => item.turn,
        _ => None,
    };
    let crop = match &item.kind {
        Kind::Image(image) if image.cropx.is_some() || image.cropy.is_some() => Some((image.cropx.unwrap_or(0.0), image.cropy.unwrap_or(0.0))),
        _ => None,
    };
    Some(Piece { tile: item.tile, look, rect: item.rect, clip: item.clip, colour: [r, g, bl, alpha], depth: item.depth, tex: None, turn, crop })
}

/// The pieces to draw of `menu`, in drawing order (stable by summed `depth`
/// [hypothesis: depth is the stacking order, ui-tile-tree.md §6.1]), and how many of the
/// mod roots ([`fnv_ui::Ui::mod_added_tiles`]) draw at least one. `select` says which
/// tiles ([`Select`]); `t15` is `tiles::interface_screen`'s hotrect-alpha gate [design].
pub fn pieces(ui: &Ui, menu: TileId, measure: &layout::Measure<'_>, select: Select<'_>, t15: bool) -> (Vec<Piece>, usize) {
    pieces_with_fonts(ui, menu, measure, &font_number, select, t15)
}

/// [`pieces`] with an explicit font-ID resolver (`resolve_font`), used when `xfonts`
/// extended slots may be loaded [design: fonts.md §4.1].
pub fn pieces_with_fonts(
    ui: &Ui,
    menu: TileId,
    measure: &layout::Measure<'_>,
    resolve_font: &dyn Fn(f64) -> u8,
    select: Select<'_>,
    t15: bool,
) -> (Vec<Piece>, usize) {
    let roots = ui.mod_added_tiles(menu);
    let items: Vec<(Option<TileId>, layout::DrawItem)> = match select {
        Select::Mods { by_locus, .. } => roots
            .iter()
            .flat_map(|&r| if by_locus { layout::layout_by_locus(ui, r, measure, t15) } else { layout::layout(ui, r, measure, t15) }.into_iter().map(move |i| (Some(r), i)))
            .collect(),
        Select::Whole { .. } => layout::layout(ui, menu, measure, t15).into_iter().map(|i| (roots.iter().copied().find(|&r| under(ui, i.tile, &[r])), i)).collect(),
    };
    let whole = matches!(select, Select::Whole { .. });
    let mut out = Vec::new();
    let mut drawn_roots = HashSet::new();
    for (root, item) in items {
        let wanted = match select {
            Select::Mods { vanilla, .. } => ui.mod_tile_drawn(item.tile, vanilla),
            // A mod's own tiles draw under the kept pieces too [design].
            Select::Whole { kept, .. } => !under(ui, item.tile, kept) || (root.is_some() && ui.mod_tile_drawn(item.tile, None)),
        };
        if !wanted {
            continue;
        }
        let strip = matches!(select, Select::Whole { strip: Some(s), .. } if s == item.tile);
        let Some(piece) = piece_from_item(ui, &item, whole, strip, resolve_font) else { continue };
        out.push(piece);
        if let Some(r) = root {
            drawn_roots.insert(r);
        }
    }
    out.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    (out, drawn_roots.len())
}

/// [design] Pieces for an isolated projected-UI subtree rooted at `root`, shifted by
/// `(-viewport.x, -viewport.y)` with finite coordinate validation and clip intersection
/// against the `[0..width, 0..height]` target canvas. Reuses the normal piece builder so
/// runtime images, rotation, colour and fit behave identically to normal menu drawing.
pub(crate) fn projected_pieces(
    ui: &Ui,
    root: TileId,
    viewport: fnv_game::ui_projection::Viewport,
    measure: &layout::Measure<'_>,
    t15: bool,
) -> Vec<Piece> {
    let vw = f64::from(viewport.width);
    let vh = f64::from(viewport.height);
    if !viewport.x.is_finite() || !viewport.y.is_finite() || vw <= 0.0 || vh <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for item in layout::layout_isolated(ui, root, measure, t15) {
        let Some(mut piece) = piece_from_item(ui, &item, true, false, &font_number) else { continue };
        piece.rect.left -= viewport.x;
        piece.rect.top -= viewport.y;
        if ![piece.rect.left, piece.rect.top, piece.rect.width, piece.rect.height].into_iter().all(f64::is_finite) {
            continue;
        }
        if let Some(mut clip) = piece.clip {
            clip.left -= viewport.x;
            clip.top -= viewport.y;
            if ![clip.left, clip.top, clip.width, clip.height].into_iter().all(f64::is_finite) {
                continue;
            }
            let left = clip.left.max(0.0);
            let top = clip.top.max(0.0);
            let right = (clip.left + clip.width).min(vw);
            let bottom = (clip.top + clip.height).min(vh);
            let width = (right - left).max(0.0);
            let height = (bottom - top).max(0.0);
            if width <= 0.0 || height <= 0.0 {
                continue;
            }
            piece.clip = Some(TileRect { left, top, width, height });
        }
        out.push(piece);
    }
    out.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    out
}

/// The renderer's node over the HUD.
#[derive(Component)]
struct TileRoot;

/// The compass strip of a [`Fit::Strip`] picture: its window baked into one image, or the
/// doubled strip moved by the heading without its texels (`crate::compass_strip`),
/// following the heading by [`scroll`].
#[derive(Component)]
pub(crate) struct TileStrip(StripNode);

/// A node that takes its piece's colour (recoloured in place, [`Change::InPlace`]), and
/// where: a box's fill, or what a picture or a glyph draws. Every UI node carries a
/// background too, transparent unless it is a box's, so only [`Ink::Fill`] colours one.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ink {
    Fill,
    Drawn,
}

/// Where a menu's pieces are painted: the window (every menu until the Pip-Boy's pages), or
/// an image a camera renders ([`Target::canvas`]). A root node and the pieces' slots belong
/// to one target, so its fields are part of what a drawing is compared with ([`paint`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    /// The camera the pieces are drawn by; none: the window's.
    pub camera: Option<Entity>,
    /// The pieces' area in the view's units, from its top left and cut off there; none: the
    /// whole view.
    pub canvas: Option<Vec2>,
    /// Logical pixels to an interface unit (the tiled pictures').
    pub scale: f32,
    /// The camera's image's pixels to a logical pixel: 1 in the window. Layout does not
    /// read it (the units are logical: the camera's target already scales them).
    pub image_scale: f32,
    /// Typography only, measured and painted inside the unchanged menu viewport.
    pub text_scale: f32,
    /// The stacking level of the root.
    pub z: i32,
    /// Where a menu in the window keeps its place at the UI scale (its root a
    /// `crate::interface::menu_frame` kept there); none: the root is the whole view (the
    /// HUD, whose places are written for the scaled view, and a camera's image, which keeps
    /// its size: its picture is what scales) [design: #1631].
    pub frame: Option<Vec2>,
}

impl Target {
    /// In the window, at `scale` logical pixels to an interface unit.
    pub fn window(scale: f32, z: i32) -> Target {
        Target { camera: None, canvas: None, scale, image_scale: 1.0, text_scale: 1.0, z, frame: None }
    }

    /// An image a camera renders: `canvas` is the part of it the pieces lie on. The image
    /// is as many logical pixels as interface units (`crate::pipboy::TARGET`), so one unit
    /// is one logical pixel whatever `image_scale` is [design: pipboy.rs, its module notes].
    pub fn image(camera: Entity, canvas: Vec2, image_scale: f32, z: i32) -> Target {
        Target { camera: Some(camera), canvas: Some(canvas), scale: 1.0, image_scale, text_scale: 1.0, z, frame: None }
    }

    /// A menu in the window drawn at the UI scale in a frame kept at `anchor`
    /// ([`Target::frame`]).
    pub fn in_frame(self, anchor: Vec2) -> Target {
        Target { frame: Some(anchor), ..self }
    }

    /// The same fixed canvas, with glyphs measured and painted at this size.
    pub(crate) fn text_sized(self, scale: f32) -> Self {
        Self { text_scale: scale, ..self }
    }

    /// Whether a root made for `self` serves `other`: the same camera, area, level and frame.
    fn same_root(&self, other: &Target) -> bool {
        self.camera == other.camera && self.canvas == other.canvas && self.z == other.z && self.frame == other.frame
    }
}

/// The root node of a menu's pieces on `target`.
fn spawn_root(commands: &mut Commands, target: &Target) -> Entity {
    let node = match target.canvas {
        Some(c) => Node { overflow: Overflow::clip(), ..at(0.0, 0.0, c.x, c.y) },
        None => Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
    };
    let node = target.frame.map_or(node.clone(), |anchor| interface::framed_root(node, anchor));
    let mut root = commands.spawn((TileRoot, node, GlobalZIndex(target.z), Pickable::IGNORE));
    if let Some(camera) = target.camera {
        root.insert(UiTargetCamera(camera));
    }
    root.id()
}

/// A piece's nodes: its slot (over the whole view, stacked by the piece's place in the
/// list), the clip wrapper in it and the piece's own outer node.
struct Slot {
    node: Entity,
    order: usize,
    wrapper: Option<Entity>,
    outer: Option<Entity>,
    hud_size: Option<crate::hud_readability::Part>,
}

/// A piece by its tile and its count among that tile's pieces, as slots are kept.
type Key = (TileId, usize);

fn keys(pieces: &[Piece]) -> Vec<Key> {
    let mut seen: HashMap<TileId, usize> = HashMap::new();
    pieces
        .iter()
        .map(|p| {
            let n = seen.entry(p.tile).or_default();
            *n += 1;
            (p.tile, *n - 1)
        })
        .collect()
}

#[derive(Default)]
pub(crate) struct Drawn {
    root: Option<Entity>,
    pieces: Vec<Piece>,
    /// Where the pieces were set out for: the camera, the area and the scale (the tiled
    /// pictures').
    target: Option<Target>,
    counts: (usize, usize),
    font_revision: u32,
    /// The whole menu's start line is written (T12a).
    told: bool,
    pictures: Option<Pictures>,
    /// Each piece's nodes, as spawned ([`change`]).
    slots: HashMap<Key, Slot>,
    repeats: Repeats,
    /// The compass strip's first window is told (`crate::compass_strip`).
    strip_told: bool,
    /// The `nif` tiles' models read so far (`super::model`).
    models: model::Models,
    /// The menu tile whose start line is written (T15a's menus, told at each build).
    told_menu: Option<TileId>,
    /// The archived HUD file's name paths, by the menu file's path, read once: what a
    /// loose replacement's own tiles are told from (`fnv_ui::Ui::archived_name_paths`,
    /// #975).
    vanilla: Option<(String, Option<HashSet<String>>)>,
    /// The texture offsets' and alpha maps' textures, baked images, budget and told lines
    /// (`super::texmod`).
    tex: super::texmod::State,
    /// What was said once about the runtime pictures (`super::runtime_images`).
    runtime_told: super::runtime_images::Told,
}

/// A `tile` picture baked into one image ([`crate::repeat`]): the image and its node's
/// width and height in interface units (the box's, or a fraction of a texel more, cut to
/// the box).
type Baked = (Handle<Image>, f32, f32);

/// How a piece differs from its tile's piece of the last frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Same,
    /// Moved or recoloured only: the same look, box and clip sizes. Its nodes are moved
    /// and recoloured in place ([`recolour`]) [design: the compass marks move every frame
    /// while the player turns or the actors walk (T12d); the fades and flashes change
    /// alpha].
    InPlace,
    /// Another look or size, or a model's colour (its parts take no [`Ink`]): its nodes
    /// are spawned again, in its slot alone.
    Spawn,
}

pub fn change(old: &Piece, new: &Piece) -> Change {
    let size = |r: &TileRect| (r.width, r.height);
    if old == new {
        Change::Same
    } else if old.look == new.look && old.tex == new.tex && old.crop == new.crop && size(&old.rect) == size(&new.rect) && old.clip.map(|c| size(&c)) == new.clip.map(|c| size(&c)) {
        // A model's parts take no Ink: spawned again in its new colour.
        if old.colour != new.colour && matches!(new.look, Look::Model { .. }) {
            Change::Spawn
        } else {
            Change::InPlace
        }
    } else {
        Change::Spawn
    }
}

/// Recolour one node of a piece changed in place: only the colour its [`Ink`] names, the
/// box's fill or what a picture or a glyph draws; a drawn node's background stays
/// transparent [design: the review of #902, where every node's background took the
/// colour and drew a box behind each glyph].
pub(crate) fn recolour<I, B, T>(ink: Ink, image: Option<I>, fill: Option<B>, text: Option<T>, colour: Color)
where
    I: std::ops::DerefMut<Target = ImageNode>,
    B: std::ops::DerefMut<Target = BackgroundColor>,
    T: std::ops::DerefMut<Target = TextColor>,
{
    match ink {
        Ink::Fill => {
            if let Some(mut fill) = fill {
                fill.0 = colour;
            }
        }
        Ink::Drawn => {
            if let Some(mut image) = image {
                image.color = colour;
            }
            if let Some(mut text) = text {
                text.0 = colour;
            }
        }
    }
}

fn f(v: f64) -> f32 {
    v as f32
}

/// The transform that turns a piece's outer node (its box's size) by its turn about its
/// pivot: Bevy turns a node about its middle, so it is moved by [`layout::Turn::shift`]
/// too; the identity for an unturned piece [design: menu-xml.md §3.1.4].
///
/// The shift is worked out from the box's unrounded size, so a turned node is laid out
/// unrounded ([`UNROUNDED`]): Bevy turns about the middle of the size it lays out, and a
/// rounded size (a 20 x 10 box is 15 x 8 pixels at 0.75 a unit, not 15 x 7.5) would put
/// the pivot up to half a pixel off, changing as the box moves across pixel edges. The
/// shift is known in the frame it is written, where the rounded size is known only after
/// Bevy's layout, a frame late; and a turned quad stands on no pixel grid anyway [design].
pub fn turned(piece: &Piece) -> UiTransform {
    match piece.turn {
        Some(turn) => {
            let (x, y) = turn.shift(piece.rect.width, piece.rect.height);
            UiTransform { translation: Val2::new(u(f(x)), u(f(y))), rotation: Rot2::radians(f(turn.angle)), ..default() }
        }
        None => UiTransform::IDENTITY,
    }
}

/// Where a picture drawn at its texels' size times its zoom stands in its box: moved up and left
/// by its crop ([`Piece::crop`]), so that point of the picture is at the box's top left.
pub fn piece_crop(piece: &Piece) -> (f64, f64) {
    piece.crop.unwrap_or((0.0, 0.0))
}

/// What setting a piece out reads besides the piece.
struct Ctx<'a> {
    fonts: &'a TileFonts,
    /// The PlayStation button pictures, drawn in a hint's place [design, #1829].
    glyphs: &'a crate::pad::PadGlyphs,
    /// Logical pixels to an interface unit.
    scale: f32,
    heading: f32,
    text_scale: f32,
}

/// How a turned piece's clip wrapper and outer node (and what they hold) are laid out:
/// unrounded ([`turned`]). The wrapper too: the outer node stands at its place less the
/// clip's unrounded origin, so a wrapper rounded to the pixel (a clip at x 10.3 units is 8
/// pixels at 0.75 a unit, not 7.725) would move the pivot by the rounding, changing as the
/// clip moves. Unturned pieces' nodes carry no `LayoutConfig` and are rounded as before.
pub const UNROUNDED: LayoutConfig = LayoutConfig { use_rounding: false };

/// Turn (or turn back) a piece's spawned nodes as its piece says: the outer node `outer`
/// takes the transform, it and the clip `wrapper` the unrounded layout, while turned; the
/// identity and the inherited rounding when not.
fn set_turn(commands: &mut Commands, wrapper: Option<Entity>, outer: Entity, piece: &Piece) {
    let turned_now = piece.turn.is_some();
    let mut node = commands.entity(outer);
    if turned_now {
        node.insert((turned(piece), UNROUNDED));
    } else {
        node.insert(UiTransform::IDENTITY).remove::<LayoutConfig>();
    }
    if let Some(w) = wrapper {
        if turned_now {
            commands.entity(w).insert(UNROUNDED);
        } else {
            commands.entity(w).remove::<LayoutConfig>();
        }
    }
}

/// A piece's outer node, turned when its tile is ([`turned`]; inside its clip wrapper, so
/// the unturned clip cuts the turned quad). An unturned piece's nodes are spawned as before.
fn spawn_piece(p: &mut ChildSpawnerCommands, piece: &Piece, ctx: &Ctx<'_>, picture: Option<SizedPicture>, baked: Option<Baked>, strip: Option<StripWindow>, model: Option<model::Prepared>) -> Option<Entity> {
    let outer = spawn_unturned(p, piece, ctx, picture, baked, strip, model)?;
    if piece.turn.is_some() {
        set_turn(&mut p.commands(), None, outer, piece);
    }
    Some(outer)
}

fn spawn_unturned(p: &mut ChildSpawnerCommands, piece: &Piece, ctx: &Ctx<'_>, picture: Option<SizedPicture>, baked: Option<Baked>, strip: Option<StripWindow>, model: Option<model::Prepared>) -> Option<Entity> {
    let r = piece.rect;
    let (dx, dy) = piece.clip.map_or((0.0, 0.0), |c| (c.left, c.top));
    let [cr, cg, cb, ca] = piece.colour;
    let colour = Color::srgba(cr, cg, cb, ca);
    let node = at(f(r.left - dx), f(r.top - dy), f(r.width), f(r.height));
    match &piece.look {
        Look::Box => Some(p.spawn((node, BackgroundColor(colour), Ink::Fill, Pickable::IGNORE)).id()),
        Look::Picture { fit, .. } => {
            let (image, rect, size) = picture?;
            let picture = |image_mode| ImageNode { color: colour, rect, image_mode, ..ImageNode::new(image.clone()) };
            let mut cut = node.clone();
            cut.overflow = Overflow::clip();
            Some(match placed(*fit, rect.is_some(), (f64::from(size.x), f64::from(size.y))) {
                Placed::Fill => p.spawn((node, picture(NodeImageMode::Stretch), Ink::Drawn, Pickable::IGNORE)).id(),
                Placed::Repeat { width, .. } => match baked {
                    // One image of the repeats, stretched [design: `crate::repeat`].
                    Some((strip, w, h)) => p
                        .spawn((cut, Pickable::IGNORE))
                        .with_children(|c| {
                            c.spawn((at(0.0, 0.0, w, h), ImageNode { color: colour, image_mode: NodeImageMode::Stretch, ..ImageNode::new(strip) }, Ink::Drawn, Pickable::IGNORE));
                        })
                        .id(),
                    // Without the picture's texels or for a box too large to bake: Bevy's
                    // tiles, one texel `width / size.x` units, `ctx.scale` logical pixels a
                    // unit.
                    None => {
                        let per_texel = if size.x > 0.0 { f(width) / size.x } else { 1.0 };
                        let mode = NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: ctx.scale * per_texel };
                        p.spawn((node, picture(mode), Ink::Drawn, Pickable::IGNORE)).id()
                    }
                },
                Placed::At { width, height } => {
                    let (cx, cy) = piece_crop(piece);
                    p.spawn((cut, Pickable::IGNORE))
                        .with_children(|c| {
                            c.spawn((at(f(-cx), f(-cy), f(width), f(height)), picture(NodeImageMode::Stretch), Ink::Drawn, Pickable::IGNORE));
                        })
                        .id()
                }
                // One image of the window's strip, its right end faded as the original's
                // (`crate::compass_strip`, T12e); without the strip's texels the doubled
                // strip moved by the heading, unfaded.
                Placed::Strip => p
                    .spawn((cut, Pickable::IGNORE))
                    .with_children(|c| match strip {
                        Some(window) => {
                            let size = window.size();
                            let image = ImageNode { color: colour, image_mode: NodeImageMode::Stretch, ..ImageNode::new(window.image()) };
                            c.spawn((TileStrip(StripNode::Baked(window)), at(0.0, 0.0, size.x, size.y), image, Ink::Drawn, Pickable::IGNORE));
                        }
                        None => {
                            c.spawn((TileStrip(StripNode::Moving), at(strip_left(ctx.heading), 0.0, 2.0 * STRIP.x, STRIP.y), Pickable::IGNORE)).with_children(|s| {
                                for i in 0..2 {
                                    s.spawn((at(i as f32 * STRIP.x, 0.0, STRIP.x, STRIP.y), picture(NodeImageMode::Stretch), Ink::Drawn, Pickable::IGNORE));
                                }
                            });
                        }
                    })
                    .id(),
            })
        }
        Look::Model { .. } => Some(model::spawn(p, node, piece.tile, model?, colour)),
        Look::Text { string, font, wrap, alignment } => {
            let face = ctx.fonts.faces.get(font)?;
            let id = p
                .spawn((node, Pickable::IGNORE))
                .with_children(|t| {
                    let factor=ctx.text_scale;
                    let _units=interface::units_at(interface::unit_scale()*factor);
                    let wrap=wrap.map(|w|w/f64::from(factor));
                    let width=piece.rect.width/f64::from(factor);
                    // A hint on one line names a button: draw its glyph in the name's place
                    // (`set_hint_marked`, so the glyph recolours with its piece).
                    if crate::pad::has_glyph(string) && face.font.lines(string, wrap.map(|w| w as f32)).len() == 1 {
                        let w = crate::pad::hint_width(face, string, ctx.glyphs);
                        let x = match alignment {
                            Alignment::Left => 0.0,
                            Alignment::Center => (width as f32 - w) / 2.0,
                            Alignment::Right => width as f32 - w,
                        };
                        crate::pad::set_hint_marked(t, face, string, x, text_base_for(*font, &face.font), colour, ctx.glyphs, Ink::Drawn);
                    } else {
                        for (line, x, base) in text_lines(&face.font, *font, string, wrap, *alignment, width) {
                            set_marked(t, face, &line, x, base, colour, Ink::Drawn);
                        }
                    }
                })
                .id();
            Some(id)
        }
    }
}

/// How many of the menu's live tiles come from its files and how many from the code.
fn origins(ui: &Ui, menu: TileId) -> (usize, usize) {
    let (mut file, mut code) = (0, 0);
    let mut stack = vec![menu];
    while let Some(t) = stack.pop() {
        let tile = ui.tile(t);
        if !tile.live {
            continue;
        }
        match tile.origin {
            fnv_ui::Origin::File { .. } => file += 1,
            fnv_ui::Origin::Code => code += 1,
            _ => {}
        }
        stack.extend(tile.children.iter().copied());
    }
    (file, code)
}

#[derive(SystemParam)]
pub(crate) struct View<'w, 's> {
    menus: Option<Res<'w, TileMenus>>,
    readability: Option<Res<'w, crate::menu_readability::MenuReadability>>,
    hud: Option<Res<'w, HudView>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    /// The container, barter or inventory menu, for whether its prompt is open (T15a).
    active: Option<Res<'w, crate::menu::ActiveMenu>>,
    /// The dialogue menu's one decision (`super::dialog_menu`).
    dialog: Option<Res<'w, super::dialog_menu::DialogTiles>>,
    /// The pause menu and its page, for the Mods page (T11, `super::mods`).
    paused: Option<Res<'w, crate::pause::Paused>>,
    page: Option<Res<'w, crate::pause::Page>>,
    /// The Pip-Boy's image and the STATS tiles' one decision (`super::stats_menu`).
    screen: Option<Res<'w, crate::pipboy::PipBoyScreen>>,
    /// The Pip-Boy's frame, for its pages' brightness on the device ([`pipboy_brightness`]).
    pip: Option<Res<'w, crate::pipboy::PipBoy>>,
    stats: Option<ResMut<'w, super::stats_menu::StatsTiles>>,
    /// The ITEMS page's one decision (`super::inventory_menu`).
    inventory: Option<ResMut<'w, super::inventory_menu::InventoryTiles>>,
    /// The Pip-Boy's repair page's one decision (`super::repair_menu`).
    repair_page: Option<ResMut<'w, super::repair_menu::RepairTiles>>,
    /// The Pip-Boy's weapon mod menu's one decision (`super::item_mod_menu`).
    item_mod: Option<ResMut<'w, super::item_mod_menu::ItemModTiles>>,
    /// The DATA page's one decision (`super::map_menu`).
    map: Option<ResMut<'w, super::map_menu::MapTiles>>,
    /// T15c: the container menu, when this frame's writer has it drawn from its tiles.
    container: Option<Res<'w, super::container_menu::ContainerTiles>>,
    barter: Option<Res<'w, super::barter_menu::BarterTiles>>,
    recipe: Option<Res<'w, super::recipe_menu::RecipeTiles>>,
    /// The vendor's repair menu, when this frame's writer has it drawn from its tiles.
    repair_services: Option<Res<'w, super::repair_services_menu::RepairServicesTiles>>,
    /// The trait menu, when this frame's writer has it drawn from its tiles.
    traits: Option<Res<'w, super::trait_menu::TraitTiles>>,
    tags: Option<Res<'w, super::tag_menu::TagTiles>>,
    /// The classic VATS screen, when this frame's writer has it drawn from its file.
    vats: Option<Res<'w, super::vats_menu::VatsTiles>>,
    /// The open character menu, for the same decision as the hand-built trait menu's.
    chargen: Option<Res<'w, crate::chargen::ChargenMenus>>,
}

/// The colours an [`Ink`] node may carry.
type InkNodes = (Option<&'static mut ImageNode>, Option<&'static mut BackgroundColor>, Option<&'static mut TextColor>);

/// The drawn pieces' nodes: their places, and the nodes under a slot to recolour.
#[derive(SystemParam)]
pub(crate) struct Nodes<'w, 's> {
    place: Query<'w, 's, &'static mut Node>,
    children: Query<'w, 's, &'static Children>,
    ink: Query<'w, 's, (&'static Ink, InkNodes)>,
    /// The game, for the pictures' texture offsets and alpha maps (`super::texmod`, #1833).
    script: Res<'w, ScriptState>,
    /// The compass strips' windows, moved on in place by a new offset (#1833).
    strips: Query<'w, 's, &'static mut TileStrip>,
}

/// The menus drawn from tiles besides the HUD (T15a, `super::menus`), each in its own
/// [`Drawn`].
#[derive(Default)]
pub(crate) struct MenusDrawn {
    message: Drawn,
    quantity: Drawn,
    /// The start menu's mod tiles on the pause menu's Mods page (T11).
    start: Drawn,
    sleep_wait: Drawn,
    tutorial: Drawn,
    /// The Pip-Boy's STATS page, in the Pip-Boy's image.
    stats: Drawn,
    /// The Pip-Boy's ITEMS page, in the Pip-Boy's image.
    inventory: Drawn,
    /// The Pip-Boy's repair page, in the Pip-Boy's image.
    repair_page: Drawn,
    /// The Pip-Boy's weapon mod menu, in the Pip-Boy's image.
    item_mod: Drawn,
    /// The Pip-Boy's DATA page, in the Pip-Boy's image.
    map: Drawn,
    container: Drawn,
    barter: Drawn,
    recipe: Drawn,
    repair_services: Drawn,
    traits: Drawn,
    tags: Drawn,
    dialog: Drawn,
    /// The classic VATS target screen (`super::vats_menu`).
    vats: Drawn,
}

/// The stacking levels of the menus drawn from tiles besides the HUD: their hand-built
/// drawings' (the message box's 108, `crate::message_menu`; the prompt's 101 over the
/// container menu's 100, `crate::menu`) [design].
pub const MESSAGE_Z: i32 = 108;
pub const QUANTITY_Z: i32 = 101;
/// T15b: the Wait / Sleep menu's, its hand-built drawing's 105 (`crate::wait`) [design].
pub const SLEEP_WAIT_Z: i32 = 105;
/// The dialogue menu's, its hand-built drawing's 90 (`crate::dialogue_menu`) [design].
pub const DIALOG_Z: i32 = 90;
/// T15b: the tutorial menu's, over the other menus as its hand-built drawing is
/// (`crate::tutorial_menu`) [design].
pub const TUTORIAL_Z: i32 = 109;
/// The Pip-Boy's page drawn from tiles, in the Pip-Boy's image: over the shell's root (no
/// level, 0), whose header it leaves the pointer to, and under the beam and the scanlines
/// (`crate::pipboy::device_screen`) [design].
pub const PIPBOY_TILES_Z: i32 = 1;
/// T15c: the container menu's, its hand-built drawing's 100 (`crate::menu`), under its prompt
/// [design].
pub const CONTAINER_Z: i32 = 100;
/// The trait menu's, its hand-built drawing's (`crate::chargen`) [design].
pub const TRAIT_Z: i32 = crate::interface::TOP_MENU_Z;

/// Where a tile menu keeps its place at the UI scale (`crate::interface::menu_frame`): the
/// dialogue menu at the view's bottom, as its hand-built drawing does
/// (`crate::dialogue_menu`), the others in its middle [design: #1631]. The pointer reads
/// the same (`super::input`).
pub fn menu_anchor(dialog: bool) -> Vec2 {
    if dialog {
        interface::BOTTOM_MIDDLE
    } else {
        interface::MIDDLE
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(mut commands: Commands, script: Res<ScriptState>, mut view: View, mut kit: FontKit, (mut drawn, mut others): (Local<Drawn>, Local<MenusDrawn>), mut nodes: Nodes, mut blur: ResMut<super::menus::TileMenuBlur>, glyphs: Res<crate::pad::PadGlyphs>) {
    let scale = view.windows.single().map_or(1.0, |w| w.height() / 960.0);
    let heading = view.hud.as_ref().map_or(0.0, |h| h.heading);
    // T15a: the message box and the prompt, when switched on and in the tree (their
    // hand-built drawings are then not spawned, `super::menus`).
    let switch = view.menus.as_deref().copied().unwrap_or_default();
    let game = script.rt.as_ref().map(|rt| &rt.game);
    let others = &mut *others;
    // The prompt drawn only while open: its tree drops at the next frame's sync.
    let prompt_open = view.active.as_ref().and_then(|a| a.0.as_ref()).is_some_and(|m| m.quantity.is_some());
    let container_on = view.container.as_ref().is_some_and(|c| c.menu.is_some());
    let barter_on = view.barter.as_ref().is_some_and(|b| b.menu().is_some());
    let inventory_on = view.inventory.as_ref().is_some_and(|i| i.ready.is_some());
    let mut blur_now = super::menus::TileMenuBlur::default();
    for (name, on, z, drawn) in [
        (super::menus::MESSAGE_MENU, game.is_some_and(|g| super::menus::message_tiled(switch.message, g)), MESSAGE_Z, &mut others.message),
        (super::menus::QUANTITY_MENU, game.is_some_and(|g| super::menus::quantity_tiled(switch.quantity, g, prompt_open, container_on || barter_on || inventory_on)), QUANTITY_Z, &mut others.quantity),
        (super::wait_menu::SLEEP_WAIT_MENU, game.is_some_and(|g| super::wait_menu::tiled(switch.sleep_wait, g)), SLEEP_WAIT_Z, &mut others.sleep_wait),
        (super::tutorial_menu::TUTORIAL_MENU, game.is_some_and(|g| super::tutorial_menu::tiled(switch.tutorial, g)), TUTORIAL_Z, &mut others.tutorial),
        (super::container_menu::CONTAINER_MENU, container_on, CONTAINER_Z, &mut others.container),
        (super::barter_menu::BARTER_MENU, barter_on, CONTAINER_Z, &mut others.barter),
        (super::recipe_menu::RECIPE_MENU, view.recipe.as_ref().is_some_and(|r| r.menu.is_some()), CONTAINER_Z, &mut others.recipe),
        (super::repair_services_menu::REPAIR_SERVICES_MENU, view.repair_services.as_ref().is_some_and(|r| r.menu.is_some()), CONTAINER_Z, &mut others.repair_services),
        // One decision with the hand-built menu's and, below, the blur's (`trait_menu::drawn`).
        (super::trait_menu::TRAIT_MENU, view.traits.as_ref().is_some_and(|t| super::trait_menu::drawn(t, view.chargen.as_ref().and_then(|c| c.traits_open()), game).is_some()), TRAIT_Z, &mut others.traits),
        (super::tag_menu::TAG_MENU, view.tags.as_ref().is_some_and(|t| super::tag_menu::drawn(t, view.chargen.as_ref().and_then(|c| c.tags_open()), game).is_some()), TRAIT_Z, &mut others.tags),
        (super::dialog_menu::DIALOG_MENU, view.dialog.as_ref().is_some_and(|d| d.drawn()), DIALOG_Z, &mut others.dialog),
        (super::vats_menu::VATS_MENU, view.vats.as_ref().is_some_and(|v| v.drawn), super::vats_menu::VATS_Z, &mut others.vats),
    ] {
        let ui = game.map(|g| &g.ui_tree);
        let menu = ui.filter(|_| on).and_then(|ui| ui.menu(name).map(|m| (ui, m)));
        // The blur behind the menu follows this very decision, not the switch or the menu's id.
        blur_now.tutorial |= name == super::tutorial_menu::TUTORIAL_MENU && menu.is_some();
        blur_now.container |= name == super::container_menu::CONTAINER_MENU && menu.is_some();
        blur_now.barter |= name == super::barter_menu::BARTER_MENU && menu.is_some();
        blur_now.recipe |= name == super::recipe_menu::RECIPE_MENU && menu.is_some();
        blur_now.repair_services |= name == super::repair_services_menu::REPAIR_SERVICES_MENU && menu.is_some();
        blur_now.traits |= name == super::trait_menu::TRAIT_MENU && menu.is_some();
        blur_now.tags |= name == super::tag_menu::TAG_MENU && menu.is_some();
        let kind=match name {
            super::barter_menu::BARTER_MENU=>Some(crate::menu_readability::MenuKind::Barter),
            super::repair_services_menu::REPAIR_SERVICES_MENU=>Some(crate::menu_readability::MenuKind::Repair),
            super::recipe_menu::RECIPE_MENU=>Some(crate::menu_readability::MenuKind::Workbench),
            super::menus::QUANTITY_MENU if view.active.as_ref().and_then(|a|a.0.as_ref()).is_some_and(|m|matches!(m.mode,crate::menu::model::Mode::Barter(_)))=>Some(crate::menu_readability::MenuKind::Barter),
            _=>None,
        };
        let text_scale=kind.map_or(1.0,|kind|crate::menu::readability::size(view.readability.as_deref(),kind));
        let pieces = match menu {
            Some((ui, menu)) => {
                if drawn.told_menu != Some(menu) {
                    let (file, code) = origins(ui, menu);
                    info!("tiles: {name} drawn from tiles ({file} file tiles, {code} code tiles)");
                    drawn.told_menu = Some(menu);
                }
                // The VATS screen writes `screen()` in interface units itself while it is up.
                let mut pieces = whole_pieces_sized(ui, menu, &mut kit, super::interface_screen(switch) || name == super::vats_menu::VATS_MENU, text_scale);
                // Marked as `paint` marks them, so a modified picture is not told as a change
                // every frame (#1833).
                super::texmod::mark(&mut pieces, game.map(|g| &g.other.itr));
                super::runtime_images::mark(&mut pieces, kit.runtime.as_deref(), &mut drawn.runtime_told);
                {
                    let _units = interface::units_at(interface::menu_scale());
                    super::runtime_images::present(&mut pieces, kit.runtime.as_deref(), Some(ui));
                }
                if name == super::dialog_menu::DIALOG_MENU && pieces != drawn.pieces {
                    info!("{}", super::dialog_menu::painted(ui, &pieces));
                }
                pieces
            }
            None => Vec::new(),
        };
        // At the menus' scale (#1631, the UI scale at most 100%): the units `scale × it` logical pixels, in a frame
        // kept at the menu's anchor.
        let anchor = menu_anchor(name == super::dialog_menu::DIALOG_MENU);
        if name == super::vats_menu::VATS_MENU {
            // The VATS target screen is a piece of the HUD: its arrows, buttons and queue
            // keep to the view's edges and its boxes stand on the target, so it takes the
            // UI scale in the view's own frame, as the HUD does, with `screen()` written
            // for the scaled view (`super::write_screen_scaled`). Above 100 % it stays at
            // the original's size, as the menus do: the file sets its buttons and its
            // queue out for a view 960 units high, and in a lower one they run into each
            // other (seen at 150 %).
            let _units = interface::units_at(interface::menu_scale());
            paint(&mut commands, drawn, pieces, Target::window(scale * interface::menu_scale(), z), Brightness::File, heading, ui, &mut kit, &mut nodes, &glyphs);
            continue;
        }
        paint(&mut commands, drawn, pieces, Target::window(scale * interface::menu_scale(), z).in_frame(anchor).text_sized(text_scale), Brightness::File, heading, ui, &mut kit, &mut nodes, &glyphs);
    }
    *blur = blur_now;
    pipboy_pages(&mut commands, game, &mut view, &mut kit, (&mut others.stats, &mut others.inventory, &mut others.map), &mut nodes, heading, &glyphs);
    repair_page(&mut commands, game, &mut view, &mut kit, &mut others.repair_page, &mut nodes, heading, &glyphs);
    item_mod_page(&mut commands, game, &mut view, &mut kit, &mut others.item_mod, &mut nodes, heading, &glyphs);
    // T11: the Mods page, the start menu's mod tiles over the pause menu.
    let mods_on = switch.start && view.paused.as_ref().zip(view.page.as_ref()).is_some_and(|(p, page)| crate::pause::mods_open(p, page));
    mods_page(&mut commands, &script, mods_on, &mut kit, &mut others.start, &mut nodes, scale * interface::menu_scale(), heading, &glyphs);
    // The HUD at the UI scale (#1631): its places are written for a view 960 / scale units
    // high (`fnv_game::ui_hud::write`), a unit `scale` times the size it is in the menus.
    let _hud = crate::interface::hud_units();
    hud(&mut commands, &script, &view, &mut kit, &mut drawn, &mut nodes, scale * interface::ui_scale(), heading, &glyphs);
}

/// How bright the Pip-Boy's pages drawn from their files are painted: the hand-built pages'
/// device brightness while the Pip-Boy is on the device (`Frame::Device`, the flat image a model
/// without a screen falls back to included: it shows the device's drawing), the file's in the
/// screen menu ([`device_brightness`]). Not the image target: the screen menu draws into one too.
fn pipboy_brightness(view: &View) -> Brightness {
    Brightness::pipboy(view.pip.as_ref().is_some_and(|p| p.frame == crate::pipboy::Frame::Device))
}

/// The Pip-Boy's pages drawn from their files, in the Pip-Boy's image, each while it is ready
/// (`super::stats_menu::StatsTiles::ready`, `super::inventory_menu::InventoryTiles::ready`,
/// decided before this drawing). What was painted is published for the next frame's tile input,
/// which acts on what the player saw.
#[allow(clippy::too_many_arguments)]
fn pipboy_pages(commands: &mut Commands, game: Option<&fnv_game::Game<'_>>, view: &mut View, kit: &mut FontKit, (stats_drawn, inventory_drawn, map_drawn): (&mut Drawn, &mut Drawn, &mut Drawn), nodes: &mut Nodes, heading: f32, glyphs: &crate::pad::PadGlyphs) {
    let Some(target) = view.screen.as_ref().map(|s| Target::image(s.camera, crate::pipboy::CANVAS, s.scale, PIPBOY_TILES_Z)) else { return };
    let target = (target, pipboy_brightness(view));
    let ui = game.map(|g| &g.ui_tree);
    let live = |ready: Option<TileId>| ready.filter(|&m| ui.is_some_and(|ui| ui.is_live(m)));
    // The one measured layout each decision checked; worked out again next frame.
    let menu = live(view.stats.as_ref().and_then(|s| s.ready));
    let pieces = menu.and(view.stats.as_mut()).map_or_else(Vec::new, |s| std::mem::take(&mut s.pieces));
    pipboy_page(commands, ui, super::stats_menu::STATS_MENU, menu, pieces, target, kit, stats_drawn, nodes, heading, glyphs);
    if let Some(stats) = view.stats.as_mut().filter(|s| s.shown != menu) {
        stats.shown = menu;
    }
    let menu = live(view.inventory.as_ref().and_then(|s| s.ready));
    let pieces = menu.and(view.inventory.as_mut()).map_or_else(Vec::new, |s| std::mem::take(&mut s.pieces));
    pipboy_page(commands, ui, super::inventory_menu::INVENTORY_MENU, menu, pieces, target, kit, inventory_drawn, nodes, heading, glyphs);
    if let Some(inventory) = view.inventory.as_mut().filter(|s| s.shown != menu) {
        inventory.shown = menu;
    }
    let menu = live(view.map.as_ref().and_then(|s| s.ready));
    let pieces = menu.and(view.map.as_mut()).map_or_else(Vec::new, |s| std::mem::take(&mut s.pieces));
    pipboy_page(commands, ui, super::map_menu::MAP_MENU, menu, pieces, target, kit, map_drawn, nodes, heading, glyphs);
    if let Some(map) = view.map.as_mut().filter(|s| s.shown != menu) {
        map.shown = menu;
    }
}

/// One page of [`pipboy_pages`]; `menu` while it is ready, `pieces` what its decision checked.
/// Logged when it changes: how many pieces go to the painter, and how many of them are the rows'
/// names. That is structure, before painting: it says nothing of the pixels, which only shots can.
#[allow(clippy::too_many_arguments)]
fn pipboy_page(commands: &mut Commands, ui: Option<&Ui>, name: &str, menu: Option<TileId>, pieces: Vec<Piece>, (target, brightness): (Target, Brightness), kit: &mut FontKit, drawn: &mut Drawn, nodes: &mut Nodes, heading: f32, glyphs: &crate::pad::PadGlyphs) {
    match ui.zip(menu) {
        Some((ui, menu)) => {
            // The STATS page is told once its rows are there, on whichever tab first has some:
            // Status with no effects has no code tile to count (#1944); the others at once.
            if drawn.told_menu != Some(menu) {
                let (file, code) = origins(ui, menu);
                if code > 0 || name != super::stats_menu::STATS_MENU {
                    info!("tiles: {name} drawn from tiles ({file} file tiles, {code} code tiles)");
                    drawn.told_menu = Some(menu);
                }
            }
            let names = pieces.iter().filter(|p| matches!(p.look, Look::Text { .. }) && ui.tile(p.tile).name.eq_ignore_ascii_case("ListItemText")).count();
            let counts = (pieces.len(), names);
            if counts != drawn.counts {
                info!("tiles: {name} set out for the Pip-Boy's image: {} pieces, {} row names", counts.0, counts.1);
                drawn.counts = counts;
            }
        }
        None => drawn.counts = (0, 0),
    }
    paint(commands, drawn, pieces, target, brightness, heading, ui, kit, nodes, glyphs);
}

/// The Pip-Boy's repair page, in the Pip-Boy's image, while it is ready
/// (`super::repair_menu::RepairTiles::ready`, decided before this drawing), as [`pipboy_page`].
#[allow(clippy::too_many_arguments)]
fn repair_page(commands: &mut Commands, game: Option<&fnv_game::Game<'_>>, view: &mut View, kit: &mut FontKit, drawn: &mut Drawn, nodes: &mut Nodes, heading: f32, glyphs: &crate::pad::PadGlyphs) {
    let Some(target) = view.screen.as_ref().map(|s| Target::image(s.camera, crate::pipboy::CANVAS, s.scale, PIPBOY_TILES_Z)) else { return };
    let brightness = pipboy_brightness(view);
    let name = super::repair_menu::REPAIR_MENU;
    let ui = game.map(|g| &g.ui_tree);
    let ready = view.repair_page.as_ref().and_then(|s| s.ready);
    let menu = ui.zip(ready).filter(|(ui, m)| ui.is_live(*m));
    let pieces = match (menu, view.repair_page.as_mut()) {
        (Some((ui, menu)), Some(page)) => {
            if drawn.told_menu != Some(menu) {
                let (file, code) = origins(ui, menu);
                info!("tiles: {name} drawn from tiles ({file} file tiles, {code} code tiles)");
                drawn.told_menu = Some(menu);
            }
            let pieces = std::mem::take(&mut page.pieces);
            let names = pieces.iter().filter(|p| matches!(p.look, Look::Text { .. }) && ui.tile(p.tile).name.eq_ignore_ascii_case("ListItemText")).count();
            let counts = (pieces.len(), names);
            if counts != drawn.counts {
                info!("tiles: {name} set out for the Pip-Boy's image: {} pieces, {} row names", counts.0, counts.1);
                drawn.counts = counts;
            }
            pieces
        }
        _ => {
            drawn.counts = (0, 0);
            Vec::new()
        }
    };
    paint(commands, drawn, pieces, target, brightness, heading, ui, kit, nodes, glyphs);
    let shown = menu.map(|(_, m)| m);
    if let Some(page) = view.repair_page.as_mut().filter(|s| s.shown != shown) {
        page.shown = shown;
    }
}

/// The Pip-Boy's weapon mod menu, in the Pip-Boy's image, while it is ready
/// (`super::item_mod_menu::ItemModTiles::ready`, decided before this drawing), by [`pipboy_page`].
#[allow(clippy::too_many_arguments)]
fn item_mod_page(commands: &mut Commands, game: Option<&fnv_game::Game<'_>>, view: &mut View, kit: &mut FontKit, drawn: &mut Drawn, nodes: &mut Nodes, heading: f32, glyphs: &crate::pad::PadGlyphs) {
    let Some(target) = view.screen.as_ref().map(|s| Target::image(s.camera, crate::pipboy::CANVAS, s.scale, PIPBOY_TILES_Z)) else { return };
    let target = (target, pipboy_brightness(view));
    let ui = game.map(|g| &g.ui_tree);
    let menu = view.item_mod.as_ref().and_then(|s| s.ready).filter(|&m| ui.is_some_and(|ui| ui.is_live(m)));
    let pieces = menu.and(view.item_mod.as_mut()).map_or_else(Vec::new, |s| std::mem::take(&mut s.pieces));
    pipboy_page(commands, ui, super::item_mod_menu::ITEM_MOD_MENU, menu, pieces, target, kit, drawn, nodes, heading, glyphs);
    if let Some(page) = view.item_mod.as_mut().filter(|s| s.shown != menu) {
        page.shown = menu;
    }
}

/// The pause menu's Mods page (T11, `super::mods`): the tiles mods added to `StartMenu`,
/// drawn as T5 draws the HUD's ([`Select::Mods`]: a loose replacement's own tiles, not the
/// archived file's), in interface units, at [`super::mods::MODS_Z`]; nothing while `on` is
/// false.
#[allow(clippy::too_many_arguments)]
fn mods_page(commands: &mut Commands, script: &ScriptState, on: bool, kit: &mut FontKit, drawn: &mut Drawn, nodes: &mut Nodes, scale: f32, heading: f32, glyphs: &crate::pad::PadGlyphs) {
    let files = script.rt.as_ref().and_then(|rt| rt.game.ui_files.as_deref());
    let ui = script.rt.as_ref().filter(|rt| rt.game.ui_files.is_some()).map(|rt| &rt.game.ui_tree);
    let menu = ui.filter(|_| on).and_then(|ui| ui.menu(super::mods::START_MENU).map(|m| (ui, m)));
    let (pieces, roots) = match menu {
        Some((ui, menu)) => {
            kit.ensure_xfonts();
            let mut stack = ui.mod_added_tiles(menu);
            while let Some(t) = stack.pop() {
                let tile = ui.tile(t);
                if tile.kind == "text" {
                    kit.load(tile.get("font").map_or(0.0, Value::num));
                }
                stack.extend(tile.children.iter().copied());
            }
            let measured = |font: f64, text: &str, wrap: Option<f64>| kit.face(font).map_or((0.0, 0.0), |face| measure(&face.font, kit.font_number(font), text, wrap));
            let resolve = |font: f64| kit.font_number(font);
            let vanilla = files.and_then(|files| archived_names(ui, menu, files, &mut drawn.vanilla));
            let (mut drawn, roots) = pieces_with_fonts(ui, menu, &measured, &resolve, Select::Mods { vanilla, by_locus: true }, true);
            // MCM's trigger is its row in the original's pause column, which NVForever's
            // column stands for (its Mods entry): not drawn over it [design: owner-review.md #34].
            if let Some(trigger) = ui.find_tile(super::mods::MCM_TRIGGER) {
                drawn.retain(|p| !under(ui, p.tile, &[trigger]));
            }
            (drawn, roots)
        }
        None => (Vec::new(), 0),
    };
    let counts = (roots, pieces.len());
    if counts != drawn.counts {
        info!("tiles: {}: Mods page, {} mod tiles drawn ({} items)", super::mods::START_MENU, counts.0, counts.1);
        drawn.counts = counts;
    }
    // In the middle of the view, as the pause menu's page under it (`crate::pause`).
    paint(commands, drawn, pieces, Target::window(scale, super::mods::MODS_Z).in_frame(interface::MIDDLE), Brightness::File, heading, ui, kit, nodes, glyphs);
}

/// The fonts of `pieces`' texts that are not loaded (`loaded` says which are), each once:
/// what [`draw`] loads before painting a menu, so a text is never spawned without its face
/// (a piece spawned without it would stay empty: an unchanged piece is not spawned again).
pub fn unloaded_fonts(pieces: &[Piece], loaded: impl Fn(u8) -> bool) -> Vec<u8> {
    let mut out = Vec::new();
    for p in pieces {
        if let Look::Text { font, .. } = &p.look {
            if !loaded(*font) && !out.contains(font) {
                out.push(*font);
            }
        }
    }
    out
}

/// Fonts for [`whole_pieces`]: whether one is loaded, loading it, and a text's size in it
/// (none, (0, 0), while it is not loaded).
pub trait MenuFonts {
    fn prepare_fonts(&mut self) {}
    fn resolve_font(&self, font: f64) -> u8 {
        font_number(font)
    }
    fn loaded(&self, font: u8) -> bool;
    fn load_font(&mut self, font: u8);
    fn size(&self, font: f64, text: &str, wrap: Option<f64>) -> (f64, f64);
}

impl MenuFonts for FontKit<'_> {
    fn prepare_fonts(&mut self) {
        self.ensure_xfonts();
    }
    fn resolve_font(&self, font: f64) -> u8 {
        self.font_number(font)
    }
    fn loaded(&self, font: u8) -> bool {
        self.fonts.faces.contains_key(&font)
    }
    fn load_font(&mut self, font: u8) {
        self.load(f64::from(font));
    }
    fn size(&self, font: f64, text: &str, wrap: Option<f64>) -> (f64, f64) {
        self.face(font).map_or((0.0, 0.0), |face| measure(&face.font, self.font_number(font), text, wrap))
    }
}

/// The pieces of a menu drawn whole (T15a's menus). Its fonts were loaded by this frame's
/// writes (`super::menus`, before scripts); a text piece in a font not loaded yet (a script
/// brought it in meanwhile) has it loaded here and the pieces worked out again, so no text
/// is set out unmeasured or spawned without its face ([`unloaded_fonts`]).
pub fn whole_pieces(ui: &Ui, menu: TileId, fonts: &mut impl MenuFonts, t15: bool) -> Vec<Piece> {
    whole_pieces_sized(ui, menu, fonts, t15, 1.0)
}

pub(crate) fn whole_pieces_sized(ui: &Ui, menu: TileId, fonts: &mut impl MenuFonts, t15: bool, scale: f32) -> Vec<Piece> {
    fonts.prepare_fonts();
    let select = Select::Whole { kept: &[], strip: None };
    let set_out = |fonts: &dyn MenuFonts| {
        pieces_with_fonts(
            ui,
            menu,
            &|font: f64, text: &str, wrap: Option<f64>| {
                crate::menu::readability::measured(&|font, text, wrap| fonts.size(font, text, wrap), scale, font, text, wrap)
            },
            &|font: f64| fonts.resolve_font(font),
            select,
            t15,
        )
        .0
    };
    let first = set_out(fonts);
    let missing = unloaded_fonts(&first, |f| fonts.loaded(f));
    if missing.is_empty() {
        return first;
    }
    for f in missing {
        fonts.load_font(f);
    }
    set_out(fonts)
}

/// The archived copy's name paths of the menu file `menu` was read from, when a root of
/// its mod tiles is a file's (a loose replacement, or a loose prefab it includes): read
/// once per file path into `kept` [design: #975, as `fnvtool ui mod-tiles`].
pub(super) fn archived_names<'a>(ui: &Ui, menu: TileId, files: &dyn fnv_ui::Source, kept: &'a mut Option<(String, Option<HashSet<String>>)>) -> Option<&'a HashSet<String>> {
    let fnv_ui::Origin::File { path, .. } = &ui.tile(menu).origin else { return None };
    if !ui.mod_added_tiles(menu).iter().any(|&r| matches!(ui.tile(r).origin, fnv_ui::Origin::File { .. })) {
        return None;
    }
    if kept.as_ref().is_none_or(|(p, _)| p != path) {
        *kept = Some((path.clone(), Ui::archived_name_paths(files, path)));
    }
    kept.as_ref().and_then(|(_, names)| names.as_ref())
}

/// The HUD's pieces (T5's mods' tiles, or with `[UI] TileMenus=HUDMainMenu` the whole menu)
/// drawn ([`paint`]).
#[allow(clippy::too_many_arguments)]
fn hud(commands: &mut Commands, script: &ScriptState, view: &View, kit: &mut FontKit, drawn: &mut Drawn, nodes: &mut Nodes, scale: f32, heading: f32, glyphs: &crate::pad::PadGlyphs) {
    let whole = view.menus.as_ref().is_some_and(|m| m.hud);
    let t15 = view.menus.as_deref().copied().is_some_and(super::interface_screen);
    let files = script.rt.as_ref().and_then(|rt| rt.game.ui_files.as_deref());
    let ui = script.rt.as_ref().filter(|rt| rt.game.ui_files.is_some()).map(|rt| &rt.game.ui_tree);
    let menu = ui.and_then(|ui| ui.menu(HUD_MENU).map(|m| (ui, m)));
    // With the tile HUD off and no mod tiles in the HUD (the default game), nothing to
    // walk or lay out (`fnv_ui::Ui::has_mod_added_tiles`, a kept count) [design: #938].
    let (pieces, roots) = match menu {
        Some((ui, menu)) if whole || ui.has_mod_added_tiles(menu) => {
            kit.ensure_xfonts();
            // The fonts of the text tiles drawn, read before measuring.
            let mut stack = if whole { vec![menu] } else { ui.mod_added_tiles(menu) };
            while let Some(t) = stack.pop() {
                let tile = ui.tile(t);
                if tile.kind == "text" {
                    kit.load(tile.get("font").map_or(0.0, Value::num));
                }
                stack.extend(tile.children.iter().copied());
            }
            let measured = |font: f64, text: &str, wrap: Option<f64>| kit.face(font).map_or((0.0, 0.0), |face| measure(&face.font, kit.font_number(font), text, wrap));
            let resolve = |font: f64| kit.font_number(font);
            if whole {
                if !drawn.told {
                    let (file, code) = origins(ui, menu);
                    info!("{}", drawn_line(file, code, &KEPT_PIECES));
                    drawn.told = true;
                }
                let kept = kept_pieces(ui);
                pieces_with_fonts(ui, menu, &measured, &resolve, Select::Whole { kept: &kept, strip: ui.find_tile(STRIP_TILE) }, t15)
            } else {
                let vanilla = files.and_then(|files| archived_names(ui, menu, files, &mut drawn.vanilla));
                pieces_with_fonts(ui, menu, &measured, &resolve, Select::Mods { vanilla, by_locus: false }, t15)
            }
        }
        _ => (Vec::new(), 0),
    };
    // The whole menu's pieces come and go with the HUD's states; only the mods' count is
    // told then [design].
    let counts = (roots, if whole { 0 } else { pieces.len() });
    if counts != drawn.counts {
        if whole {
            info!("tiles: {HUD_MENU}: {} mod tiles drawn (whole menu)", counts.0);
        } else {
            info!("tiles: {HUD_MENU}: {} mod tiles drawn ({} items)", counts.0, counts.1);
        }
        drawn.counts = counts;
    }
    // The HUD keeps the view's own frame: its places are written for the scaled view.
    paint(commands, drawn, pieces, Target::window(scale, TILES_Z), Brightness::File, heading, ui, kit, nodes, glyphs);
    if let Some(ui) = ui {
        for ((tile, _), slot) in &mut drawn.slots {
            let wanted = crate::hud_readability::tile_part(&ui.path_of(*tile));
            if wanted != slot.hud_size {
                if let Some(part) = wanted {
                    commands.entity(slot.node).insert(crate::hud_readability::HudSize(part));
                } else {
                    commands.entity(slot.node).remove::<crate::hud_readability::HudSize>()
                        .insert(UiTransform::IDENTITY);
                }
                slot.hud_size = wanted;
            }
        }
    }
}

/// How bright a menu's pieces are painted ([`paint`]), chosen by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Brightness {
    /// As the file resolves them: every menu in the window, and the Pip-Boy's pages in the
    /// screen menu (Use Pip-Boy off), where the hand-built pages keep the file's 255 too.
    File,
    /// A Pip-Boy page on the device (`crate::pipboy::Frame::Device`, its flat fallback image
    /// too), whose page colour is `bright` (`crate::interface::pipboy_rgb`): the pieces at the
    /// file's full bright level take the hand-built pages' device brightness
    /// ([`device_brightness`]).
    PipBoyDevice { bright: [u8; 3] },
}

impl Brightness {
    /// A Pip-Boy page's: [`Brightness::PipBoyDevice`] while the Pip-Boy is on the device
    /// (`device`), else the file's.
    pub(crate) fn pipboy(device: bool) -> Brightness {
        if device {
            Brightness::PipBoyDevice { bright: interface::pipboy_rgb() }
        } else {
            Brightness::File
        }
    }
}

/// The hand-built pages' device brightness on the Pip-Boy's pages drawn from their files
/// [design: #2212]. On the device the hand-built pages draw only their bright role at
/// `crate::pipboy::frame::DEVICE_TEXT` (213) instead of the file's 255: the text, the Vault
/// Boy, icons, bars and the map's picture (`Ctx::bright`); their lines stay at 200, the dimmed
/// text and the fills at 128 and the map's location icons at 220 (255 under the highlight box)
/// (`pipboy::frame`, `pipboy::data`, docs/spec-notes/pipboy.md §3). So no flat multiplier:
/// a piece painted at the page's colour at its full level (the file's resolved `brightness`
/// 255 over `bright`, what [`pieces`] gives as `bright / 255`) takes 213 of 255; a piece the
/// file already dims (its lines' 200, its dimmed texts and fills, its background's 90), one
/// in another colour, and the map's location icons (`super::map_menu::MARKER_ROW`, whose
/// 220/255 the hand-built page keeps too) keep the file's colour; the alpha is never touched.
/// The file's heading lines resolve at 255 (`line.xml`'s brightness picked by the heading),
/// so they take 213 where the hand-built heading's lines are 200 [design: the rule kept
/// whole rather than special-cased by picture; docs/spec-notes/pipboy.md §3].
pub(crate) fn device_brightness(pieces: &mut [Piece], ui: Option<&Ui>, brightness: Brightness) {
    let Brightness::PipBoyDevice { bright } = brightness else { return };
    let factor = crate::pipboy::frame::DEVICE_TEXT / 255.0;
    let full = bright.map(|c| f32::from(c) / 255.0);
    for p in pieces.iter_mut() {
        // A float tolerance, not half a level: the pieces' colour is `bright * brightness / 255`
        // in floats, so a brightness of 254 over a dark colour must not pass for 255.
        let at_full = p.colour[..3].iter().zip(full).all(|(c, f)| (c - f).abs() <= 1e-4);
        if !at_full || ui.is_some_and(|ui| super::map_menu::keeps_file_brightness(ui, p.tile)) {
            continue;
        }
        for c in &mut p.colour[..3] {
            *c *= factor;
        }
    }
}

/// Despawn a [`Drawn`] root and clear its cached piece slots.
pub(crate) fn clear(commands: &mut Commands, drawn: &mut Drawn) {
    if let Some(root) = drawn.root.take() {
        commands.entity(root).despawn();
    }
    drawn.slots.clear();
    drawn.pieces.clear();
    drawn.target = None;
}

/// Bring one menu's drawn nodes in line with its `pieces` on `target`: nothing when they and
/// the target are as last drawn; otherwise each piece's slot kept, moved and recoloured in
/// place, spawned again or removed ([`change`]). `brightness` is applied to the pieces before
/// they are compared ([`device_brightness`]), so a page whose frame switches between the
/// device and the screen menu is recoloured.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint(commands: &mut Commands, drawn: &mut Drawn, mut pieces: Vec<Piece>, target: Target, brightness: Brightness, heading: f32, ui: Option<&Ui>, kit: &mut FontKit, nodes: &mut Nodes, glyphs: &crate::pad::PadGlyphs) {
    device_brightness(&mut pieces, ui, brightness);
    let scale = target.scale;
    // ITR's texture offsets and alpha maps on the pictures, before they are compared (#1833).
    super::texmod::mark(&mut pieces, nodes.script.rt.as_ref().map(|rt| &rt.game.other.itr));
    // The runtime pictures' revisions and sizes, so a new one under the same name repaints.
    super::runtime_images::mark(&mut pieces, kit.runtime.as_deref(), &mut drawn.runtime_told);
    // Apply recipient metadata in the actual target's units, before final comparison.
    // A camera's image is laid out at its own size whatever the UI scale (#1631).
    let _canvas = target.camera.map(|_| interface::original_units());
    super::runtime_images::present(&mut pieces, kit.runtime.as_deref(), ui);
    drawn.tex.frame();
    drawn.tex.prune(&pieces, ui);
    let font_revision = crate::cjk_font::revision();
    let font_changed = drawn.font_revision != font_revision;
    drawn.font_revision = font_revision;
    if !font_changed && pieces == drawn.pieces && drawn.target == Some(target) {
        return;
    }
    // Nothing drawn yet and nothing to draw (a menu switched off): no root node either.
    if pieces.is_empty() && drawn.root.is_none() {
        drawn.target = Some(target);
        return;
    }
    // Another camera, area or level: the root is made again for it.
    if drawn.target.is_some_and(|was| !was.same_root(&target)) {
        if let Some(root) = drawn.root.take() {
            commands.entity(root).despawn();
        }
    }
    let root = *drawn.root.get_or_insert_with(|| spawn_root(commands, &target));
    // A new size: the tiled pictures are set for the old one, so every piece again.
    if drawn.target != Some(target) {
        commands.entity(root).despawn_related::<Children>();
        drawn.slots.clear();
        drawn.pieces.clear();
        drawn.target = Some(target);
    }
    let keys = keys(&pieces);
    let old: HashMap<Key, &Piece> = self::keys(&drawn.pieces).into_iter().zip(&drawn.pieces).collect();
    // The pieces gone take their slots with them.
    let wanted: HashSet<Key> = keys.iter().copied().collect();
    drawn.slots.retain(|key, slot| {
        let keep = wanted.contains(key);
        if !keep {
            commands.entity(slot.node).despawn();
        }
        keep
    });
    let mut spawn = Vec::new();
    // Pieces whose new offset waits for the bake budget: kept as last shown, so the next
    // frame tries again (`super::texmod::State::shift_in_place`).
    let mut waiting = Vec::new();
    for (i, (key, piece)) in keys.iter().zip(&pieces).enumerate() {
        let (Some(slot), Some(was)) = (drawn.slots.get_mut(key), old.get(key)) else {
            spawn.push(i);
            continue;
        };
        if slot.order != i {
            commands.entity(slot.node).insert(ZIndex(i as i32));
            slot.order = i;
        }
        let font_changed = font_changed && matches!(piece.look, Look::Text { .. });
        let mut how = if font_changed { Change::Spawn } else { change(was, piece) };
        // Only the offset changed: its image baked again in place, its nodes kept (#1833).
        if how == Change::Spawn && super::texmod::only_offset_differs(was, piece) {
            let strip = nodes.children.iter_descendants(slot.node).find(|e| nodes.strips.contains(*e));
            let mut strip = strip.and_then(|e| nodes.strips.get_mut(e).ok());
            let window = strip.as_mut().and_then(|s| match &mut s.0 {
                StripNode::Baked(w) => Some(w),
                StripNode::Moving => None,
            });
            let data = kit.data.as_deref();
            let read = |p: &str| data.and_then(|d| d.read(p));
            if let Some(shown) = drawn.tex.shift_in_place(piece, &mut kit.images, Some(&read), window) {
                if !shown {
                    waiting.push((i, was.tex.clone()));
                }
                how = change(&Piece { tex: piece.tex.clone(), ..(*was).clone() }, piece);
            }
        }
        match how {
            Change::Same => {}
            Change::Spawn => spawn.push(i),
            Change::InPlace => {
                if was.rect != piece.rect || was.clip != piece.clip {
                    if let (Some(e), Some(c)) = (slot.wrapper, piece.clip) {
                        if let Ok(mut node) = nodes.place.get_mut(e) {
                            node.left = u(f(c.left));
                            node.top = u(f(c.top));
                        }
                    }
                    let (dx, dy) = piece.clip.map_or((0.0, 0.0), |c| (c.left, c.top));
                    if let Some(e) = slot.outer {
                        if let Ok(mut node) = nodes.place.get_mut(e) {
                            node.left = u(f(piece.rect.left - dx));
                            node.top = u(f(piece.rect.top - dy));
                        }
                    }
                }
                // Turned further (the map's arrow as the player turns): turned in place.
                if was.turn != piece.turn {
                    if let Some(e) = slot.outer {
                        set_turn(commands, slot.wrapper, e, piece);
                    }
                }
                if was.colour != piece.colour {
                    let [r, g, b, a] = piece.colour;
                    let colour = Color::srgba(r, g, b, a);
                    for e in nodes.children.iter_descendants(slot.node) {
                        let Ok((ink, (image, fill, text))) = nodes.ink.get_mut(e) else { continue };
                        recolour(*ink, image, fill, text, colour);
                    }
                }
            }
        }
    }
    if !spawn.is_empty() {
        for &i in &spawn {
            if let Look::Text { font, .. } = &pieces[i].look {
                kit.load(f64::from(*font));
            }
        }
        let bc = bc_supported(kit.formats.as_deref());
        let data = kit.data.as_deref();
        let pictures = drawn.pictures.get_or_insert_with(|| Pictures::load(data));
        let mut made = Vec::with_capacity(spawn.len());
        for &i in &spawn {
            let piece = &pieces[i];
            let picture = match &piece.look {
                Look::Picture { name, source, .. } => match source {
                    Source::Shared => pictures.picture_sized(name, None, data, &mut kit.images, bc),
                    Source::Index(atlas) => pictures.picture_sized(name, Some(atlas), data, &mut kit.images, bc),
                    Source::Own => pictures.own_sized(name, data, &mut kit.images, bc),
                    Source::Runtime(_) => super::runtime_images::picture(kit.runtime.as_deref(), name, &mut drawn.runtime_told),
                },
                _ => None,
            };
            // A `tile` picture's repeats, baked into one image.
            let baked = match (&piece.look, &picture, data) {
                // A runtime picture's texels are not read back: Bevy's tiles repeat it.
                (Look::Picture { name, source: Source::Runtime(_), fit }, Some((_, rect, size)), _) => {
                    if matches!(placed(*fit, rect.is_some(), (f64::from(size.x), f64::from(size.y))), Placed::Repeat { .. }) {
                        super::runtime_images::unbaked(name, &mut drawn.runtime_told);
                    }
                    None
                }
                (Look::Picture { name, source, fit }, Some((_, rect, size)), Some(data)) => {
                    let texels = (f64::from(size.x), f64::from(size.y));
                    match placed(*fit, rect.is_some(), texels) {
                        Placed::Repeat { width, height } => repeat::texels_for((piece.rect.width, piece.rect.height), (width, height), texels).and_then(|(out, node)| {
                            let atlas = match source {
                                Source::Index(a) => Some(a.as_str()),
                                _ => None,
                            };
                            let path = pictures.texture_path(name, atlas, rect.is_some(), data);
                            let cut = rect.map(repeat::piece_texels);
                            drawn.repeats.baked(&path, cut, out, data, &mut kit.images).map(|image| (image, f(node.0), f(node.1)))
                        }),
                        _ => None,
                    }
                }
                _ => None,
            };
            // SetUITexOffset's offset and SetUIAlphaMap's mask, baked into the picture's
            // place; the compass strip's below (`super::texmod`, #1833).
            let (picture, baked) = match (&piece.tex, &piece.look, &picture, data) {
                (Some(tex), Look::Picture { name, source, fit }, Some((_, rect, size)), Some(data)) if *fit != Fit::Strip => {
                    let atlas = match source {
                        Source::Index(a) => Some(a.as_str()),
                        _ => None,
                    };
                    let path = pictures.texture_path(name, atlas, rect.is_some(), data);
                    let texels = (f64::from(size.x), f64::from(size.y));
                    let repeat = match placed(*fit, rect.is_some(), texels) {
                        Placed::Repeat { width, height } => repeat::texels_for((piece.rect.width, piece.rect.height), (width, height), texels),
                        _ => None,
                    };
                    let bake = super::texmod::Bake { state: &mut drawn.tex, data, images: &mut kit.images, ui };
                    let drawing = super::texmod::Drawing { path: &path, placed: placed(*fit, rect.is_some(), texels), repeat };
                    super::texmod::apply(tex, piece, drawing, picture.clone(), baked, bake)
                }
                _ => (picture, baked),
            };
            // The compass strip's window, baked into one image (`crate::compass_strip`).
            let strip = match (&piece.look, &picture, data) {
                (Look::Picture { name, source, fit: Fit::Strip }, Some((_, rect, _)), Some(data)) if !matches!(source, Source::Runtime(_)) => {
                    let atlas = match source {
                        Source::Index(a) => Some(a.as_str()),
                        _ => None,
                    };
                    // With an offset or a mask: the strip moved on and its window masked
                    // (`super::texmod::strip`, #1833); as it was when their texels are not read.
                    let modded = piece.tex.as_ref().and_then(|tex| {
                        let path = pictures.texture_path(name, atlas, rect.is_some(), data);
                        let bake = super::texmod::Bake { state: &mut drawn.tex, data, images: &mut kit.images, ui };
                        let (texels, mask) = super::texmod::strip(tex, piece, &path, *rect, bake)?;
                        StripWindow::new(texels, f(piece.rect.width), heading, &mut kit.images).map(|w| w.masked(mask, &mut kit.images))
                    });
                    let window = modded.or_else(|| StripTexels::read(pictures, name, atlas, *rect, &mut drawn.repeats, data).and_then(|t| StripWindow::new(t, f(piece.rect.width), heading, &mut kit.images)));
                    if !drawn.strip_told {
                        match &window {
                            Some(w) => info!("tiles: the compass strip baked into one window image of {} x {} texels", w.texels().0, w.texels().1),
                            None => info!("tiles: the compass strip drawn unfaded (its texels not read)"),
                        }
                        drawn.strip_told = true;
                    }
                    window
                }
                _ => None,
            };
            // A `nif` tile's model and its pieces' textures (`super::model`).
            let model = match (&piece.look, data, ui) {
                (Look::Model { filename, animation }, Some(data), Some(ui)) => drawn.models.get(filename, animation.as_deref(), data).map(|m| {
                    let textures = m.pieces.iter().map(|p| (p.texture.clone(), pictures.file(&p.texture, data, &mut kit.images, bc))).collect();
                    model::Prepared { fraction: model::fraction(ui, piece.tile), textures, model: m }
                }),
                _ => None,
            };
            made.push((i, picture, baked, strip, model));
        }
        let ctx = Ctx { fonts: &kit.fonts, glyphs, scale, heading, text_scale:target.text_scale };
        for (i, picture, baked, strip, model) in made {
            let piece = &pieces[i];
            let key = keys[i];
            match drawn.slots.get_mut(&key) {
                Some(slot) => {
                    let mut nodes = (None, None);
                    commands.entity(slot.node).despawn_related::<Children>().with_children(|s| nodes = fill(s, piece, &ctx, picture, baked, strip, model));
                    (slot.wrapper, slot.outer) = nodes;
                }
                None => {
                    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
                    let mut slot = None;
                    commands.entity(root).with_children(|r| {
                        let mut nodes = (None, None);
                        let node = r.spawn((full, ZIndex(i as i32), Pickable::IGNORE)).with_children(|s| nodes = fill(s, piece, &ctx, picture, baked, strip, model)).id();
                        slot = Some(Slot { node, order: i, wrapper: nodes.0, outer: nodes.1, hud_size: None });
                    });
                    drawn.slots.extend(slot.map(|s| (key, s)));
                }
            }
        }
    }
    for (i, shown) in waiting {
        pieces[i].tex = shown;
    }
    drawn.pieces = pieces;
}

/// A piece in its slot, in a clip wrapper when it has a clip: the wrapper and the piece's
/// own outer node.
fn fill(s: &mut ChildSpawnerCommands, piece: &Piece, ctx: &Ctx<'_>, picture: Option<SizedPicture>, baked: Option<Baked>, strip: Option<StripWindow>, model: Option<model::Prepared>) -> (Option<Entity>, Option<Entity>) {
    match piece.clip {
        Some(c) => {
            let mut wrapper = at(f(c.left), f(c.top), f(c.width), f(c.height));
            wrapper.overflow = Overflow::clip();
            let mut outer = None;
            let id = s.spawn((wrapper, Pickable::IGNORE)).with_children(|w| outer = spawn_piece(w, piece, ctx, picture, baked, strip, model)).id();
            // A turned piece's wrapper unrounded with it ([`UNROUNDED`]); others as before.
            if piece.turn.is_some() {
                s.commands().entity(id).insert(UNROUNDED);
            }
            (Some(id), outer)
        }
        None => (None, spawn_piece(s, piece, ctx, picture, baked, strip, model)),
    }
}

/// Show the tile-drawn compass strip at the heading [design: as the hand-built HUD's
/// `hud::draw` shows its own, `crate::compass_strip`; hud.md §14.4-§14.5].
pub(crate) fn scroll(hud: Option<Res<HudView>>, mut images: ResMut<Assets<Image>>, mut strips: Query<(&mut TileStrip, &mut Node)>) {
    let Some(hud) = hud else { return };
    for (mut strip, node) in &mut strips {
        strip.0.follow(hud.heading, node, &mut images);
    }
}

#[cfg(test)]
#[path = "draw_layout_tests.rs"]
mod layout_tests;

// Install only the production native painter in the authored CPU typography scene.
#[cfg(test)]
pub(crate) fn install_typography_fixture_painter(app: &mut App) {
    app.init_resource::<TileFonts>().init_resource::<super::menus::TileMenuBlur>()
        .add_systems(Update, draw.after(super::barter_menu::write_system).after(crate::menu::MenuSystems));
}

#[cfg(test)]
#[derive(Resource)]
pub(super) struct TestPainted(pub Ui, pub TileId);

#[cfg(test)]
pub(super) fn paint_test_menu(
    mut commands: Commands,
    painted: Res<TestPainted>,
    mut kit: FontKit,
    mut nodes: Nodes,
    glyphs: Res<crate::pad::PadGlyphs>,
    mut drawn: Local<Drawn>,
) {
    let measure = |_: f64, _: &str, _: Option<f64>| (0.0, 0.0);
    let (set, _) = pieces(&painted.0, painted.1, &measure, Select::Whole { kept: &[], strip: None }, true);
    paint(&mut commands, &mut drawn, set, Target::window(1.0, 1), Brightness::File, 0.0, Some(&painted.0), &mut kit, &mut nodes, &glyphs);
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_ui::MemSource;

    /// Every character 10 wide, lines 20 high [design].
    fn plain(_: f64, text: &str, _: Option<f64>) -> (f64, f64) {
        (10.0 * text.chars().count() as f64, 20.0)
    }

    /// [design] A window's target is told by its scale and level alone; an image's by its
    /// camera, area and scale too, so a drawing kept for one is never kept for another.
    #[test]
    fn a_target_is_part_of_what_a_drawing_is_kept_for() {
        let mut world = World::new();
        let (a, b) = (world.spawn_empty().id(), world.spawn_empty().id());
        let canvas = Vec2::new(400.0, 300.0);
        assert_eq!(Target::window(1.5, 7), Target::window(1.5, 7));
        assert_ne!(Target::window(1.5, 7), Target::window(2.0, 7));
        assert!(Target::window(1.5, 7).same_root(&Target::window(2.0, 7)), "a new window size keeps its root");
        assert!(!Target::window(1.5, 7).same_root(&Target::window(1.5, 8)));
        let image = Target::image(a, canvas, 1.0, 1);
        assert!(!image.same_root(&Target::window(1.0, 1)), "the window's root cannot serve a camera");
        assert!(!image.same_root(&Target::image(b, canvas, 1.0, 1)), "nor another camera's");
        assert!(!image.same_root(&Target::image(a, Vec2::new(500.0, 300.0), 1.0, 1)), "nor another area's");
        // A sharper image keeps the root and its units, and is another drawing.
        let sharper = Target::image(a, canvas, 1.5, 1);
        assert!(image.same_root(&sharper));
        assert_ne!(image, sharper);
        assert_eq!(sharper.scale, 1.0, "layout is not scaled twice: the camera's target does it");
    }

    /// [design] The real drawing system over a runtime with the menu files read, the tutorial
    /// menu's switch `on`, nothing open.
    fn tutorial_drawing(on: bool) -> App {
        let mut state = ScriptState::over_no_plugins();
        state.rt.as_mut().unwrap().game.ui_files = Some(Box::new(MemSource::default()));
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<TileFonts>()
            .init_resource::<crate::pad::PadGlyphs>()
            .init_resource::<super::super::menus::TileMenuBlur>()
            .insert_resource(TileMenus { tutorial: on, ..Default::default() })
            .insert_resource(state)
            .add_systems(Update, draw);
        app
    }

    fn with_game(app: &mut App, change: impl FnOnce(&mut fnv_game::Game<'_>)) {
        change(&mut app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game);
    }

    /// The frame's drawing: what it published for the blur of this menu; the other flag is
    /// not this menu's to set.
    fn tutorial_blur_after_frame(app: &mut App) -> bool {
        // A stale value must not survive into the frame: the drawing writes it whole.
        app.world_mut().resource_mut::<super::super::menus::TileMenuBlur>().container = true;
        app.update();
        let blur = *app.world().resource::<super::super::menus::TileMenuBlur>();
        assert!(!blur.container);
        blur.tutorial
    }

    fn open_tutorial(game: &mut fnv_game::Game<'_>) {
        game.menu = Some(fnv_game::game::menus::TUTORIAL);
    }

    fn built_tutorial() -> Ui {
        super::super::tutorial_menu::tests::menu_changed(str::to_string)
    }

    /// The first frame of an opened menu draws by hand and the next from the file (the tree
    /// is built at the next frame's sync): the blur starts with the tiles, stops in the frame
    /// the menu closes, and comes back with a reopening.
    #[test]
    fn the_tile_menu_blur_follows_the_tile_drawing_through_open_close_and_reopen() {
        let mut app = tutorial_drawing(true);
        assert!(!tutorial_blur_after_frame(&mut app), "nothing open");
        with_game(&mut app, open_tutorial);
        assert!(!tutorial_blur_after_frame(&mut app), "open, not built yet: drawn by hand");
        with_game(&mut app, |g| g.ui_tree = built_tutorial());
        assert!(tutorial_blur_after_frame(&mut app), "the first frame drawn from tiles");
        assert!(tutorial_blur_after_frame(&mut app), "and every one after");
        with_game(&mut app, |g| g.menu = None);
        assert!(!tutorial_blur_after_frame(&mut app), "closed, its tree not dropped yet");
        with_game(&mut app, |g| g.ui_tree = Ui::new());
        assert!(!tutorial_blur_after_frame(&mut app), "the tree dropped");
        with_game(&mut app, open_tutorial);
        assert!(!tutorial_blur_after_frame(&mut app), "open again, drawn by hand");
        with_game(&mut app, |g| g.ui_tree = built_tutorial());
        assert!(tutorial_blur_after_frame(&mut app), "reopened, drawn from tiles");
    }

    /// Whatever keeps the hand-built drawing keeps the world sharp: a file without a path the
    /// model needs, no menu files, no runtime, the switch off.
    #[test]
    fn the_tile_menu_blur_stays_off_while_the_menu_is_drawn_by_hand() {
        let mut app = tutorial_drawing(true);
        with_game(&mut app, open_tutorial);
        with_game(&mut app, |g| g.ui_tree = super::super::tutorial_menu::tests::menu_changed(|xml| xml.replace(r#"name="TM_scrollbar""#, r#"name="Modded""#)));
        assert!(!tutorial_blur_after_frame(&mut app), "a file a mod restructured");

        with_game(&mut app, |g| g.ui_tree = built_tutorial());
        assert!(tutorial_blur_after_frame(&mut app), "the whole file");
        with_game(&mut app, |g| g.ui_files = None);
        assert!(!tutorial_blur_after_frame(&mut app), "the menu files gone");

        with_game(&mut app, |g| g.ui_files = Some(Box::new(MemSource::default())));
        assert!(tutorial_blur_after_frame(&mut app), "the menu files back");
        app.world_mut().resource_mut::<ScriptState>().rt = None;
        assert!(!tutorial_blur_after_frame(&mut app), "the runtime gone");

        let mut off = tutorial_drawing(false);
        with_game(&mut off, open_tutorial);
        with_game(&mut off, |g| g.ui_tree = built_tutorial());
        assert!(!tutorial_blur_after_frame(&mut off), "the switch off, the file there");
    }

    fn hud() -> (Ui, MemSource) {
        // [design] A synthetic HUD: one piece in the HUD's colour with a file text, and a
        // template the code copies from.
        let src = MemSource::default().with(
            "hud.xml",
            r#"<menu name="HUDMainMenu"><rect name="Piece"><x>100</x><y>50</y><systemcolor>&hudmain;</systemcolor>
              <text name="Label"><string>HP</string></text>
              <template name="row"><text name="Row"><string>copy</string></text></template></rect></menu>"#,
        );
        let mut ui = Ui::new();
        ui.load_menu(&src, "hud.xml").unwrap();
        ui.update();
        (ui, src)
    }

    #[test]
    fn a_text_whose_font_comes_late_is_set_out_once_its_face_is_there() {
        // Review of T15a's font dedupe: a script's text in a font the writes did not load.
        let src = MemSource::default().with(
            "m.xml",
            r#"<menu name="M"><text name="A"><font>2</font><string>Hi</string></text><text name="B"><font>8</font><string>Late</string></text></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "m.xml").unwrap();
        ui.update();
        let (set, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &[], strip: None }, false);
        // Font 2 loaded, font 8 not: font 8 is to load before painting.
        let mut loaded = vec![2u8];
        assert_eq!(unloaded_fonts(&set, |f| loaded.contains(&f)), vec![8]);
        // Once loaded, nothing is missing: the text piece spawns with its face.
        loaded.push(8);
        assert!(unloaded_fonts(&set, |f| loaded.contains(&f)).is_empty());
        assert!(set.iter().any(|p| matches!(&p.look, Look::Text { font: 8, string, .. } if string == "Late")));
        // Through the drawing's own path: font 8 not loaded on the first layout, loaded by
        // it, and the text set out again at its measured size.
        struct Fonts(Vec<u8>);
        impl MenuFonts for Fonts {
            fn loaded(&self, font: u8) -> bool {
                self.0.contains(&font)
            }
            fn load_font(&mut self, font: u8) {
                self.0.push(font);
            }
            fn size(&self, font: f64, text: &str, wrap: Option<f64>) -> (f64, f64) {
                if self.loaded(font_number(font)) { plain(font, text, wrap) } else { (0.0, 0.0) }
            }
        }
        let mut fonts = Fonts(vec![2]);
        let drawn = whole_pieces(&ui, menu, &mut fonts, true);
        assert!(fonts.loaded(8), "the late font is loaded");
        let late = drawn.iter().find(|p| matches!(&p.look, Look::Text { string, .. } if string == "Late")).unwrap();
        assert_eq!((late.rect.width, late.rect.height), (40.0, 20.0), "set out with its face, not at (0, 0)");
        // With every font there, one layout and nothing loaded.
        let before = fonts.0.len();
        assert_eq!(whole_pieces(&ui, menu, &mut fonts, true), drawn);
        assert_eq!(fonts.0.len(), before);
    }

    #[test]
    fn highlight_fill_is_drawn() {
        let src = MemSource::default().with(
            "h.xml",
            r#"<menu name="M"><rect name="R"><systemcolor>1</systemcolor>
              <hotrect name="H"><alpha>40</alpha><brightness>128</brightness><width>100</width><height>50</height></hotrect></rect></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "h.xml").unwrap();
        ui.update();
        let select = || Select::Whole { kept: &[], strip: None };
        assert!(pieces(&ui, menu, &plain, select(), true).0.is_empty(), "unmarked, a hotrect draws nothing");
        let h = ui.find_tile("M/R/H").unwrap();
        ui.set_trait(h, layout::HOTRECT_FILL, Value::Num(1.0));
        ui.set_trait(h, layout::OWN_ALPHA, Value::Num(1.0));
        let (drawn, _) = pieces(&ui, menu, &plain, select(), true);
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].look, Look::Box);
        assert!((drawn[0].colour[3] - (40.0f32 / 255.0).powf(2.2)).abs() < 1e-6, "the hand-built fill's curve");
    }

    #[test]
    fn only_the_mods_tiles_draw_and_code_copies_never() {
        let (mut ui, src) = hud();
        let menu = ui.menu(HUD_MENU).unwrap();
        assert_eq!(pieces(&ui, menu, &plain, Select::Mods { vanilla: None, by_locus: false }, false), (Vec::new(), 0));
        ui.add_code_tile_from_template(&src, "HUDMainMenu/Piece", "row", None).unwrap();
        assert!(ui.inject_text(
            &src,
            "HUDMainMenu/Piece",
            r#"<rect name="Mod"><x>5</x><y>6</y><width>50</width><height>20</height><red>0</red><green>128</green><blue>255</blue><alpha>128</alpha>
                <image name="Pic"><filename>solid.dds</filename><width>8</width><height>8</height></image>
                <text name="Words"><string>abc</string><justify>&right;</justify><brightness>0</brightness></text>
                <rect name="Empty"/><nif name="Model"/></rect>"#
        ));
        ui.update();
        let (drawn, roots) = pieces(&ui, menu, &plain, Select::Mods { vanilla: None, by_locus: false }, false);
        assert_eq!(roots, 1);
        let names: Vec<_> = drawn.iter().map(|p| ui.tile(p.tile).name.as_str()).collect();
        assert_eq!(names, ["Mod", "Pic", "Words"]);
        assert_eq!(drawn[0].look, Look::Box);
        assert_eq!(drawn[0].rect, TileRect { left: 105.0, top: 56.0, width: 50.0, height: 20.0 });
        // Its own colour, not the piece's system colour; alpha 128 of 255.
        assert_eq!(drawn[0].colour, [0.0, 128.0 / 255.0, 1.0, 128.0 / 255.0]);
        // The picture takes the piece's colour (the HUD's); the text's brightness 0 is black.
        let hud = interface::hud_rgb().map(|c| f32::from(c) / 255.0);
        assert_eq!(drawn[1].look, Look::Picture { name: "solid.dds".into(), source: Source::Shared, fit: Fit::Stretch });
        assert_eq!(drawn[1].colour[..3], hud);
        assert_eq!(drawn[2].look, Look::Text { string: "abc".into(), font: 1, wrap: None, alignment: Alignment::Right });
        assert_eq!(drawn[2].rect, TileRect { left: 75.0, top: 56.0, width: 30.0, height: 20.0 });
        assert_eq!(drawn[2].colour, [0.0, 0.0, 0.0, 128.0 / 255.0]);
        // The HUD hidden hides them; a zero alpha too.
        let hidden = ui.find_tile("HUDMainMenu").unwrap();
        ui.set_trait(hidden, "visible", Value::Num(0.0));
        assert_eq!(pieces(&ui, menu, &plain, Select::Mods { vanilla: None, by_locus: false }, false).1, 0);
        ui.set_trait(hidden, "visible", Value::Num(1.0));
        ui.set_float("HUDMainMenu/Piece/Mod/alpha", 0.0);
        assert_eq!(pieces(&ui, menu, &plain, Select::Mods { vanilla: None, by_locus: false }, false).1, 0);
    }

    #[test]
    fn depth_orders_the_drawing_and_clips_carry_to_the_pieces() {
        let (mut ui, src) = hud();
        let menu = ui.menu(HUD_MENU).unwrap();
        assert!(ui.inject_text(
            &src,
            "HUDMainMenu",
            r#"<rect name="Clip"><width>40</width><height>40</height><clips>&true;</clips><red>1</red>
                <image name="Front"><filename>a.dds</filename><depth>5</depth><width>80</width><height>80</height></image>
                <image name="Back"><filename>b.dds</filename><depth>-5</depth></image></rect>
              <text name="Zero"><string>x</string><font>0</font><wrapwidth>0</wrapwidth></text>"#
        ));
        ui.update();
        let (drawn, roots) = pieces(&ui, menu, &plain, Select::Mods { vanilla: None, by_locus: false }, false);
        assert_eq!(roots, 2);
        let names: Vec<_> = drawn.iter().map(|p| ui.tile(p.tile).name.as_str()).collect();
        assert_eq!(names, ["Back", "Clip", "Zero", "Front"]);
        assert_eq!(drawn[3].clip, Some(TileRect { left: 0.0, top: 0.0, width: 40.0, height: 40.0 }));
        assert_eq!(drawn[1].clip, None);
        assert_eq!(drawn[2].look, Look::Text { string: "x".into(), font: 1, wrap: None, alignment: Alignment::Left });
    }

    #[test]
    fn a_clip_window_carries_to_the_pieces_only_once_turned_on() {
        let mut ui = Ui::new();
        let src = MemSource::default().with(
            "m.xml",
            r#"<menu name="M"><hotrect name="W"><x>5</x><y>6</y><width>40</width><height>30</height><clipwindow>&true;</clipwindow>
                <text name="T"><string>x</string><y>-9</y><width>30</width><height>90</height></text></hotrect></menu>"#,
        );
        ui.load_menu(&src, "m.xml").unwrap();
        ui.update();
        let menu = ui.menu("M").unwrap();
        let clip = |ui: &Ui| pieces(ui, menu, &plain, Select::Whole { kept: &[], strip: None }, true).0[0].clip;
        assert_eq!(clip(&ui), None);
        fnv_ui::input::mark_clip_windows(&mut ui, menu);
        assert_eq!(clip(&ui), Some(TileRect { left: 5.0, top: 6.0, width: 40.0, height: 30.0 }));
    }

    #[test]
    fn a_loose_hud_draws_nothing_of_its_file_without_the_archived_names() {
        let src = MemSource::default().with_loose("hud.xml", r#"<menu name="HUDMainMenu"><rect name="New"><red>9</red><width>4</width><height>4</height></rect></menu>"#);
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "hud.xml").unwrap();
        ui.update();
        assert_eq!(pieces(&ui, menu, &plain, Select::Mods { vanilla: None, by_locus: false }, false).1, 0);
        let vanilla: HashSet<String> = ["hudmainmenu".to_string()].into();
        assert_eq!(pieces(&ui, menu, &plain, Select::Mods { vanilla: Some(&vanilla), by_locus: false }, false).1, 1);
        // No archived copy under it: nothing to compare with, nothing drawn (#975).
        let mut kept = None;
        assert_eq!(archived_names(&ui, menu, &src, &mut kept), None);
    }

    /// [design] A loose HUD over an archived one draws its own tile, read through the
    /// archived copy's names, and keeps the archived tile hidden (#975).
    #[test]
    fn a_loose_hud_over_an_archived_one_draws_its_own_tiles() {
        let src = MemSource::default().with_loose_over(
            "menus/main/hud_main_menu.xml",
            r#"<menu name="HUDMainMenu"><rect name="Old"><red>9</red><width>4</width><height>4</height></rect><rect name="New"><red>9</red><width>4</width><height>4</height></rect></menu>"#,
            r#"<menu name="HUDMainMenu"><rect name="Old"/></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "menus/main/hud_main_menu.xml").unwrap();
        ui.update();
        let mut kept = None;
        let vanilla = archived_names(&ui, menu, &src, &mut kept).cloned();
        assert_eq!(vanilla, Some(["hudmainmenu".to_string(), "hudmainmenu/old".to_string()].into()));
        let (drawn, roots) = pieces(&ui, menu, &plain, Select::Mods { vanilla: vanilla.as_ref(), by_locus: false }, false);
        assert_eq!((drawn.len(), roots), (1, 1));
        assert_eq!(drawn[0].tile, ui.find_tile("HUDMainMenu/New").unwrap());
    }

    #[test]
    fn font_numbers_and_measuring() {
        assert_eq!([0.0, 1.0, 7.0, 9.0, 10.0, -3.0, 7.4].map(font_number), [1, 1, 7, 9, 1, 1, 7]);
        // The stand-in font has no pictures: one line its size, the next lines a pitch.
        let font = plain_font(31.0, 15.0);
        assert_eq!(measure(&font, 7, "ab cd", None), (75.0, 31.0));
        assert_eq!(measure(&font, 7, "ab cd", Some(0.0)), (75.0, 31.0));
        assert_eq!(measure(&font, 7, "ab cd", Some(40.0)), (30.0, 31.0 + 38.0));
        assert_eq!(measure(&font, 9, "ab cd", Some(40.0)), (30.0, 62.0));
        assert_eq!(measure(&font, 7, "", None), (0.0, 31.0));
    }

    #[test]
    fn string_dimensions_share_layout_and_count_lines_directly() {
        let font = tall_font(36.0, 42.0);
        assert_eq!(dimensions(&font, 7, "ab cd", Some(40.0)), (30.0, 80.0, 2.0));
        assert_eq!(dimensions(&font, 7, "ab\n\ncd", None), (30.0, 118.0, 3.0));
        assert_eq!(dimensions(&font, 7, "", None), (0.0, 42.0, 1.0));
        let other = tall_font(52.0, 64.0);
        assert_eq!(dimensions(&other, 8, "ab\ncd", None), (30.0, 116.0, 2.0));
        let measure = string_measure(move |n| Some(if n == 8 { other.clone() } else { font.clone() }));
        assert_eq!(measure(8.0, "ab\ncd", 0.0), (30.0, 116.0, 2.0));
        assert_eq!(measure(0.0, "ab", -1.0), (30.0, 42.0, 1.0));
    }

    #[test]
    fn unavailable_metrics_retry_without_reparsing_every_frame() {
        use std::cell::Cell;
        use std::time::Duration;
        let mut cache = StringMetrics::default();
        let available = Cell::new(false);
        let reads = Cell::new(0);
        let load = |_: u8| {
            reads.set(reads.get() + 1);
            available.get().then(|| plain_font(29.0, 10.0))
        };
        assert_eq!(cache.measure(9, "ab", 0.0, Duration::ZERO, &load), (0.0, 0.0, 0.0));
        available.set(true);
        assert_eq!(cache.measure(9, "ab", 0.0, Duration::from_millis(500), &load), (0.0, 0.0, 0.0));
        assert_eq!(reads.get(), 1);
        assert_eq!(cache.measure(9, "ab", 0.0, Duration::from_secs(1), &load), (20.0, 29.0, 1.0));
        assert_eq!(cache.measure(9, "ab cd", 25.0, Duration::from_secs(2), &load), (20.0, 58.0, 2.0));
        assert_eq!(reads.get(), 2);
    }

    /// A stand-in font whose tallest glyph picture is `tallest` high.
    fn tall_font(size: f32, tallest: f32) -> Font {
        let mut font = plain_font(size, 15.0);
        font.glyphs[usize::from(b'A')].width = 10.0;
        font.glyphs[usize::from(b'A')].height = tallest;
        font
    }

    /// The hud_menus capture (2026-10-31, fonts.md §3): a text tile's `height` is its
    /// font's tallest glyph and one pitch for each further line, as the original read it
    /// for every drawn font at one, two and (fonts 4, 7, 8) three lines.
    #[test]
    fn a_texts_height_is_its_tallest_glyph_and_a_pitch_a_line() {
        // Font, its file's size, its tallest glyph; the original's heights at 1-3 lines.
        let fonts: [(u8, f32, f32, [Option<f64>; 3]); 8] = [
            (1, 31.0, 36.0, [Some(36.0), Some(67.0), None]),
            (2, 31.0, 32.0, [Some(32.0), Some(67.0), None]),
            (3, 26.0, 31.0, [Some(31.0), Some(63.0), None]),
            (4, 36.0, 37.0, [Some(37.0), Some(73.0), Some(109.0)]),
            (5, 25.0, 28.0, [Some(28.0), Some(53.0), None]),
            (6, 34.0, 38.0, [Some(38.0), Some(76.0), None]),
            (7, 36.0, 42.0, [Some(42.0), Some(80.0), Some(118.0)]),
            (8, 52.0, 64.0, [Some(64.0), Some(116.0), Some(168.0)]),
        ];
        for (n, size, tallest, heights) in fonts {
            let font = tall_font(size, tallest);
            assert_eq!(tallest_glyph(&font), tallest);
            for (text, height) in ["A", "A\nA", "A\nA\nA"].into_iter().zip(heights) {
                if let Some(h) = height {
                    assert_eq!(measure(&font, n, text, None).1, h, "font {n}, {text:?}");
                }
            }
        }
    }

    #[test]
    fn tiles_numeric_colours_match_symbols_and_flow_to_enemy_children() {
        let src = MemSource::default().with("c.xml", r#"<menu name="HUDMainMenu"><rect name="EnemyHealth"><systemcolor>2</systemcolor>
            <image name="meter"><filename>tick.dds</filename><brightness>175</brightness><width>32</width><height>20</height></image>
            </rect><text name="Word"><systemcolor>1</systemcolor><string>H20</string><zoom>75</zoom><justify>&right;</justify></text></menu>"#);
        let mut ui = Ui::new();
        let root = ui.load_menu(&src, "c.xml").unwrap();
        ui.update();
        let (drawn, _) = pieces(&ui, root, &plain, Select::Whole { kept: &kept_pieces(&ui), strip: None }, false);
        assert_eq!(drawn.len(), 2);
        for (number, name) in [(1.0, "hudmain"), (2.0, "hudalt")] {
            assert_eq!(system_colour(&Value::Num(number)), system_colour(&Value::Sym(name.into())));
        }
        let red = system_colour(&Value::Num(2.0)).unwrap().map(|c| c * (175.0 / 255.0));
        assert_eq!(drawn[0].colour[..3], red);
        // T12e: the text's zoom 75 neither shrinks its box nor its glyphs (the original's
        // Hardcore words, hud.md §13); right-justified, its x is the box's right end.
        assert_eq!(drawn[1].rect, TileRect { left: -30.0, top: 0.0, width: 30.0, height: 20.0 });
        assert!(matches!(drawn[1].look, Look::Text { .. }));
        for name in ["EnemyHealth", "Info", "HardcoreMode", "XPMeter", "crippled_limb_indicator", "Messages", "QuestReminder", "Region_Location", "Subtitles"] {
            assert!(!KEPT_PIECES.contains(&name));
        }
    }

    /// [design] A synthetic HUD after the vanilla file's shapes: a corner piece in the
    /// HUD's colour with the compass window and a meter the code copies from templates, a
    /// kept piece (the prompt) with a file text, and the radiation meter's alt colour.
    fn vanilla_hud() -> (Ui, MemSource, TileId) {
        let src = MemSource::default().with(
            "hud.xml",
            r#"<menu name="HUDMainMenu"><visible>&false;</visible>
              <rect name="HitPoints"><x>40</x><y>803</y><systemcolor>&hudmain;</systemcolor></rect>
              <rect name="Info"><systemcolor>&hudmain;</systemcolor><text name="button_text"><string>Talk</string></text></rect>
              <rect name="RadiationMeter"><systemcolor>&hudalt;</systemcolor>
                <image name="radiation_bracket"><filename>Interface\HUD\bracket.dds</filename><width>567</width><height>120</height></image></rect>
              <template name="template_compass_window"><image name="compass_window"><filename> Interface\HUD\strip.dds </filename>
                <texatlas> Interface\InterfaceShared.tai </texatlas><width>345</width><height>64</height><zoom>100</zoom><x>20</x><y>65</y></image></template>
              <template name="template_meter"><image name="meter"><filename>Interface\HUD\hud_tick_mark.dds</filename><tile>&true;</tile><width>144</width><height>20</height></image></template>
              <template name="template_justify_left_text"><text name="justify_left_text"><string>HP</string><zoom>&scale;</zoom></text></template>
            </menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "hud.xml").unwrap();
        for template in ["template_compass_window", "template_meter", "template_justify_left_text"] {
            ui.add_code_tile_from_template(&src, "HUDMainMenu/HitPoints", template, None).unwrap();
        }
        ui.add_code_tile_from_template(&src, "HUDMainMenu/Info", "template_justify_left_text", None).unwrap();
        ui.set_float("HUDMainMenu/visible", 1.0);
        ui.update();
        (ui, src, menu)
    }

    fn names(ui: &Ui, drawn: &[Piece]) -> Vec<String> {
        drawn.iter().map(|p| ui.path_of(p.tile)).collect()
    }

    #[test]
    fn with_the_switch_on_the_files_and_the_codes_tiles_draw_but_the_kept_pieces() {
        let (mut ui, src, menu) = vanilla_hud();
        // Off (T5): none of them.
        assert_eq!(pieces(&ui, menu, &plain, Select::Mods { vanilla: None, by_locus: false }, false).0, Vec::new());
        let kept = [ui.find_tile("HUDMainMenu/Info").unwrap()];
        let strip = ui.find_tile("HUDMainMenu/HitPoints/compass_window");
        let whole = |ui: &Ui| pieces(ui, menu, &plain, Select::Whole { kept: &kept, strip }, false);
        let (drawn, roots) = whole(&ui);
        assert_eq!(roots, 0);
        assert_eq!(
            names(&ui, &drawn),
            ["HUDMainMenu/HitPoints/compass_window", "HUDMainMenu/HitPoints/meter", "HUDMainMenu/HitPoints/justify_left_text", "HUDMainMenu/RadiationMeter/radiation_bracket"]
        );
        // The strip scrolls; the tiled meter repeats; `&scale;` stretches, no zoom is
        // zoom's default; the texatlas goes to the picture lookup, trimmed; no texatlas
        // reads the picture's own file (T12c).
        assert_eq!(drawn[0].look, Look::Picture { name: "Interface\\HUD\\strip.dds".into(), source: Source::Index("Interface\\InterfaceShared.tai".into()), fit: Fit::Strip });
        assert_eq!(drawn[0].rect, TileRect { left: 60.0, top: 868.0, width: 345.0, height: 64.0 });
        assert_eq!(drawn[1].look, Look::Picture { name: "Interface\\HUD\\hud_tick_mark.dds".into(), source: Source::Own, fit: Fit::Tiled(100.0) });
        assert_eq!(drawn[3].look, Look::Picture { name: "Interface\\HUD\\bracket.dds".into(), source: Source::Own, fit: Fit::Natural });
        // Colours as the hand-built HUD's: the HUD's at full brightness and alpha, the
        // radiation meter's the alt colour.
        let hud = interface::hud_rgb().map(|c| f32::from(c) / 255.0);
        assert_eq!(drawn[2].colour, [hud[0], hud[1], hud[2], 1.0]);
        let alt = crate::reticle::crime_colour().to_srgba();
        assert_eq!(drawn[3].colour, [alt.red, alt.green, alt.blue, 1.0]);
        // Without the strip named, the compass window is its zoom's.
        let (plain_strip, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &kept, strip: None }, false);
        assert_eq!(plain_strip[0].look, Look::Picture { name: "Interface\\HUD\\strip.dds".into(), source: Source::Index("Interface\\InterfaceShared.tai".into()), fit: Fit::Zoom(100.0) });
        // A mod's tile under a kept piece still draws, and counts as a mod root.
        assert!(ui.inject_text(&src, "HUDMainMenu/Info", r#"<rect name="Outline"><red>1</red><width>4</width><height>4</height></rect>"#));
        ui.update();
        let (drawn, roots) = whole(&ui);
        assert_eq!(roots, 1);
        assert!(names(&ui, &drawn).contains(&"HUDMainMenu/Info/Outline".to_string()));
        assert!(!names(&ui, &drawn).iter().any(|n| n.contains("button_text") || n.contains("Info/justify_left_text")));
        // The HUD hidden hides all of them.
        ui.set_float("HUDMainMenu/visible", 0.0);
        assert!(whole(&ui).0.is_empty());
    }

    #[test]
    fn zoom_and_tile_say_how_a_picture_fills_its_box() {
        let src = MemSource::default().with(
            "z.xml",
            r#"<menu name="Z"><image name="none"/><image name="free"><zoom>-1</zoom></image><image name="scale"><zoom>&scale;</zoom></image>
              <image name="hundred"><zoom>100</zoom></image><image name="more"><zoom> 125 </zoom></image><image name="tiled"><tile>&true;</tile><zoom>50</zoom></image>
              <image name="untiled"><tile>&false;</tile></image><image name="tiled_scale"><tile>&true;</tile><zoom>&scale;</zoom></image>
              <image name="tiled_free"><tile>&true;</tile><zoom>-1</zoom></image>
              <image name="path"><tile>&scale;</tile><height>5.8</height></image><image name="path_flat"><tile>&scale;</tile></image></menu>"#,
        );
        let mut ui = Ui::new();
        ui.load_menu(&src, "z.xml").unwrap();
        ui.update();
        let fit_of = |name: &str| fit(ui.tile(ui.find_tile(&format!("Z/{name}")).unwrap()));
        assert_eq!(
            ["none", "free", "scale", "hundred", "more", "tiled", "untiled", "tiled_scale", "tiled_free", "path", "path_flat"].map(fit_of),
            [Fit::Natural, Fit::Stretch, Fit::Stretch, Fit::Zoom(100.0), Fit::Zoom(125.0), Fit::Tiled(50.0), Fit::Natural, Fit::Tiled(100.0), Fit::Tiled(100.0), Fit::TiledToHeight(5.8), Fit::Natural]
        );
    }

    /// A picture's `cropx` and `cropy` go with its piece, the point of the zoomed picture put at
    /// the box's top left (a map marker's icon, 14 texels in, times its zoom); a crop changed
    /// spawns the piece again, as the picture inside it moves; none without either trait.
    #[test]
    fn a_pictures_crop_puts_that_point_at_its_boxs_top_left() {
        let src = MemSource::default().with(
            "c.xml",
            r#"<menu name="C"><image name="icon"><filename>i.dds</filename><width>40</width><height>40</height><zoom>120</zoom><cropx>16.8</cropx><cropy>16.8</cropy></image>
              <image name="half"><filename>i.dds</filename><width>40</width><height>40</height><zoom>120</zoom><cropy>5</cropy></image>
              <image name="plain"><filename>i.dds</filename><width>40</width><height>40</height><zoom>120</zoom></image></menu>"#,
        );
        let mut ui = Ui::new();
        ui.load_menu(&src, "c.xml").unwrap();
        ui.update();
        let menu = ui.menu("C").unwrap();
        let set = pieces(&ui, menu, &|_, _, _| (0.0, 0.0), Select::Whole { kept: &[], strip: None }, true).0;
        let of = |name: &str| set.iter().find(|p| ui.tile(p.tile).name == name).unwrap().clone();
        assert_eq!((of("icon").crop, of("half").crop, of("plain").crop), (Some((16.8, 16.8)), Some((0.0, 5.0)), None));
        assert_eq!((piece_crop(&of("icon")), piece_crop(&of("plain"))), ((16.8, 16.8), (0.0, 0.0)));
        let mut moved = of("icon");
        moved.crop = Some((0.0, 0.0));
        assert_eq!(change(&of("icon"), &moved), Change::Spawn, "the picture moved inside its box");
    }

    /// T12c: no zoom stretches an atlas piece over its box and stands a picture of its own
    /// file at its texels' size; a number scales the picture, tiled or not.
    #[test]
    fn where_a_picture_stands_in_its_box() {
        // The corner bracket's half-size atlas piece fills its box; the radiation
        // bracket's own 512 x 128 file stands at its size in its 567 x 120 box.
        assert_eq!(placed(Fit::Natural, true, (255.0, 127.0)), Placed::Fill);
        assert_eq!(placed(Fit::Natural, false, (512.0, 128.0)), Placed::At { width: 512.0, height: 128.0 });
        assert_eq!(placed(Fit::Stretch, false, (512.0, 128.0)), Placed::Fill);
        assert_eq!(placed(Fit::Zoom(75.0), false, (128.0, 64.0)), Placed::At { width: 96.0, height: 48.0 });
        assert_eq!(placed(Fit::Tiled(100.0), false, (8.0, 32.0)), Placed::Repeat { width: 8.0, height: 32.0 });
        // #4685: the dotted line's 64 x 16 picture at its 5.8-high box's height, repeated.
        let Placed::Repeat { width, height } = placed(Fit::TiledToHeight(5.8), false, (64.0, 16.0)) else { panic!("repeated") };
        assert!((width - 23.2).abs() < 1e-9 && (height - 5.8).abs() < 1e-9);
        assert_eq!(placed(Fit::Strip, true, (1023.0, 63.0)), Placed::Strip);
        // The condition fill's zoom stands its picture's 11 drawn rows over its 20-unit
        // box: one repeat covers it.
        let Placed::Repeat { width, height } = placed(Fit::Tiled(crate::tiles::vanilla::CONDITION_FILL_ZOOM), false, (128.0, 16.0)) else { panic!("tiled") };
        assert!((11.0 * height / 16.0 - 20.0).abs() < 1e-9 && width > 60.0);
    }

    /// T12c: a tile with no `systemcolor` on it or above it takes system colour 1, the
    /// HUD's; its own `red`/`green`/`blue` and `&nosystemcolor;` still give its own.
    #[test]
    fn a_tile_with_no_system_colour_above_it_is_the_huds() {
        let src = MemSource::default().with(
            "c.xml",
            r#"<menu name="HUDMainMenu"><text name="AmmoTypeLabel"><string>HP</string><justify>&right;</justify></text>
              <text name="Own"><string>x</string><red>10</red></text>
              <text name="None"><string>x</string><systemcolor>&nosystemcolor;</systemcolor></text></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "c.xml").unwrap();
        ui.update();
        let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &[], strip: None }, false);
        let hud = interface::hud_rgb().map(|c| f32::from(c) / 255.0);
        assert_eq!(drawn[0].colour[..3], hud);
        assert_eq!(drawn[1].colour[..3], [10.0 / 255.0, 1.0, 1.0]);
        assert_eq!(drawn[2].colour[..3], [1.0, 1.0, 1.0]);
    }

    /// #979: texts of fonts 7 and 8 set at one `y` do not stand on one line: "RAD/SEC"
    /// (font 7) 24 under it, "+9" (font 8) 6 lower, as the original's ink (hud.md
    /// §14.10, fonts.md §3); the next lines a font's pitch apart; justification sets each
    /// line in the box.
    #[test]
    fn fonts_7_and_8_at_one_y_stand_on_their_own_lines() {
        let (small, large) = (plain_font(36.0, 15.0), plain_font(52.0, 20.0));
        let a = text_lines(&small, 7, "RAD/SEC", None, Alignment::Left, 105.0);
        let b = text_lines(&large, 8, "+9", None, Alignment::Right, 100.0);
        assert_eq!((a[0].2, b[0].2), (24.0, 30.0));
        assert_eq!((a[0].1, b[0].1), (0.0, 60.0));
        // Font 7's second line 38 under its first, not its file's size 36.
        let two = text_lines(&small, 7, "ab cd", Some(40.0), Alignment::Center, 40.0);
        assert_eq!(two.iter().map(|(_, x, base)| (*x, *base)).collect::<Vec<_>>(), [(5.0, 24.0), (5.0, 24.0 + 38.0)]);
        // A font-8 text's second line one font-8 pitch under its first.
        let lines = text_lines(&large, 8, "ab cd", Some(40.0), Alignment::Left, 40.0);
        assert_eq!(lines.iter().map(|(_, _, base)| *base).collect::<Vec<_>>(), [30.0, 30.0 + 52.0]);
    }

    #[test]
    fn measured_and_drawn_cjk_tile_lines_use_the_same_advances() {
        let mut font = plain_font(31.0, 15.0);
        let glyph = fnv_font::Glyph {
            width: 20.0,
            ..default()
        };
        for c in "甲乙丙丁".chars() {
            font.extra.insert(c, glyph);
        }
        crate::cjk_font::with_enabled_for_test(|| {
            let text = "甲乙丙丁";
            let wrap = Some(40.0);
            assert_eq!(dimensions(&font, 1, text, wrap), (40.0, 62.0, 2.0));
            let lines = text_lines(&font, 1, text, wrap, Alignment::Right, 50.0);
            assert_eq!(
                lines
                    .iter()
                    .map(|(line, x, base)| (line.text(), line.width, *x, *base))
                    .collect::<Vec<_>>(),
                [
                    ("甲乙".to_string(), 40.0, 10.0, 20.0),
                    ("丙丁".to_string(), 40.0, 10.0, 51.0)
                ]
            );
        });
    }

    /// #979: a text with no `font` is font 1 ([`font_number`]), whose line stands 20 under
    /// its top (the level digits' ink, hud.md §14.10); font 9, which draws nothing in the
    /// original (no texture), takes font 7's 24 and its file's size as its pitch [design].
    #[test]
    fn the_default_font_stands_higher_and_font_9_takes_font_7s_line() {
        assert_eq!(text_base(font_number(0.0)), 20.0);
        let digit = text_lines(&plain_font(31.0, 15.0), font_number(0.0), "2", None, Alignment::Left, 15.0);
        assert_eq!(digit[0].2, 20.0);
        assert_eq!(text_base(9), text_base(7));
        assert_eq!(line_pitch(9, &plain_font(29.0, 15.0)), 29.0);
    }

    /// The hud_menus capture (2026-10-31, fonts.md §3): each font's first line under a text
    /// tile's top on font 7's 24, and its line pitch, which for fonts 2, 3, 6 and 7 is not
    /// the file's size; a two-line text's second line one pitch under the first.
    #[test]
    fn every_font_has_its_own_line_and_pitch() {
        // Font, its file's size; the line under the top, the pitch.
        let fonts: [(u8, f32, f32, f32); 8] =
            [(1, 31.0, 20.0, 31.0), (2, 31.0, 16.0, 35.0), (3, 26.0, 16.0, 32.0), (4, 36.0, 16.0, 36.0), (5, 25.0, 16.0, 25.0), (6, 34.0, 22.0, 38.0), (7, 36.0, 24.0, 38.0), (8, 52.0, 30.0, 52.0)];
        for (n, size, base, pitch) in fonts {
            let font = plain_font(size, 15.0);
            assert_eq!((text_base(n), line_pitch(n, &font)), (base, pitch), "font {n}");
            let lines = text_lines(&font, n, "ab cd", Some(40.0), Alignment::Left, 40.0);
            assert_eq!(lines.iter().map(|(_, _, b)| *b).collect::<Vec<_>>(), [base, base + pitch], "font {n}");
        }
    }

    /// #979: from a tile's `font` trait through the drawn pieces to the lines the renderer
    /// sets the text on: no `font` is font 1 (20), fonts 2-5 16, font 6 22, font 7 24,
    /// font 8 30; a wrapped text's second line one pitch lower (font 2: 35).
    #[test]
    fn a_text_tiles_font_trait_sets_the_line_it_is_drawn_on() {
        let (mut ui, src) = hud();
        let menu = ui.menu(HUD_MENU).unwrap();
        assert!(ui.inject_text(
            &src,
            "HUDMainMenu",
            r#"<text name="Plain"><string>2</string></text>
              <text name="Two"><string>ab cd</string><font>2</font><wrapwidth>40</wrapwidth></text>
              <text name="Three"><string>Ok</string><font>3</font></text>
              <text name="Four"><string>Ok</string><font>4</font></text>
              <text name="Five"><string>Ok</string><font>5</font></text>
              <text name="Six"><string>Ok</string><font>6</font></text>
              <text name="Seven"><string>XP</string><font>7</font></text>
              <text name="Eight"><string>LEVEL UP</string><font>8</font></text>"#
        ));
        ui.update();
        let (drawn, _) = pieces(&ui, menu, &plain, Select::Mods { vanilla: None, by_locus: false }, false);
        let font = plain_font(36.0, 15.0);
        let bases = |name: &str| {
            let piece = drawn.iter().find(|p| ui.tile(p.tile).name == name).expect("a text piece");
            piece_lines(piece, &font).into_iter().map(|(_, _, base)| base).collect::<Vec<_>>()
        };
        let firsts = ["Plain", "Three", "Four", "Five", "Six", "Seven", "Eight"].map(|n| bases(n)[0]);
        assert_eq!(firsts, [20.0, 16.0, 16.0, 16.0, 22.0, 24.0, 30.0]);
        assert_eq!(bases("Two"), [16.0, 16.0 + 35.0]);
    }

    /// T12d: a `tile` picture is one image of its repeats at its texels' resolution,
    /// stretched over its box, so the bars' ticks stand a true period apart at any window
    /// size (each within half a pixel), not 6 and 7 pixels by turns at 1280 x 800.
    #[test]
    fn a_tiled_meter_is_one_image_and_its_ticks_stand_evenly() {
        let (ui, _, menu) = vanilla_hud();
        let kept = [ui.find_tile("HUDMainMenu/Info").unwrap()];
        let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &kept, strip: None }, false);
        let meter = drawn.iter().find(|p| ui.path_of(p.tile) == "HUDMainMenu/HitPoints/meter").unwrap();
        let Look::Picture { fit, .. } = &meter.look else { panic!("a picture") };
        let Placed::Repeat { width, height } = placed(*fit, false, (8.0, 32.0)) else { panic!("repeated") };
        let ((tw, th), (nw, nh)) = repeat::texels_for((meter.rect.width, meter.rect.height), (width, height), (8.0, 32.0)).unwrap();
        // The template's 144 x 20 box: 18 repeats of 8 texels, the picture's top 20 rows.
        assert_eq!(((tw, th), (nw, nh)), ((144, 20), (144.0, 20.0)));
        for (w, h) in [(1280.0, 800.0), (1920.0, 1080.0), (1280.0, 720.0)] {
            let scale = h / 960.0;
            let period = 8.0 * scale;
            // A tick's left edge: texel 8k of the image, its node from the box's left.
            let edges: Vec<f64> = (0..tw / 8).map(|k| (meter.rect.left + f64::from(k * 8) * nw / f64::from(tw)) * scale).collect();
            for pair in edges.windows(2) {
                assert!((pair[1] - pair[0] - period).abs() < 0.5, "{w}x{h}: {pair:?}");
            }
        }
    }

    /// T12d and the review of #902: a piece only moved (the compass marks while turning)
    /// or recoloured (the fades and flashes) is changed in place; a change of size or
    /// look spawns that piece's nodes again, and only its own.
    #[test]
    fn moved_and_recoloured_pieces_change_in_place() {
        let (ui, _, menu) = vanilla_hud();
        let kept = [ui.find_tile("HUDMainMenu/Info").unwrap()];
        let (old, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &kept, strip: None }, false);
        assert_eq!(change(&old[0], &old[0]), Change::Same);
        let mut moved = old[0].clone();
        moved.rect.left += 0.25;
        if let Some(c) = moved.clip.as_mut() {
            c.top += 3.0;
        }
        assert_eq!(change(&old[0], &moved), Change::InPlace);
        let mut dimmer = old[2].clone();
        dimmer.colour[3] = 0.5;
        assert_eq!(change(&old[2], &dimmer), Change::InPlace);
        let mut wider = old[1].clone();
        wider.rect.width += 8.0;
        assert_eq!(change(&old[1], &wider), Change::Spawn);
        let text = old.iter().find(|p| matches!(p.look, Look::Text { .. })).expect("a text piece");
        let mut other = text.clone();
        if let Look::Text { string, .. } = &mut other.look {
            string.push('!');
        }
        assert_eq!(change(text, &other), Change::Spawn);
    }

    /// The review of #952: #902's bug was every node's background taking the piece's
    /// colour, a box behind each glyph. A recolour writes only what the node's Ink names.
    #[test]
    fn a_recolour_writes_only_the_colour_its_ink_names() {
        let (was, now) = (Color::srgba(1.0, 0.7, 0.3, 1.0), Color::srgba(0.2, 0.9, 0.4, 0.5));
        // A picture or glyph (Drawn), its background transparent as spawned.
        let (mut image, mut text, mut back) = (ImageNode { color: was, ..default() }, TextColor(was), BackgroundColor(Color::NONE));
        recolour(Ink::Drawn, Some(&mut image), Some(&mut back), Some(&mut text), now);
        assert_eq!((image.color, text.0), (now, now));
        assert_eq!(back.0, Color::NONE, "a drawn node's background stays transparent");
        // A box (Fill): its background takes the colour; nothing else it carries does.
        let (mut image, mut text, mut fill) = (ImageNode { color: was, ..default() }, TextColor(was), BackgroundColor(was));
        recolour(Ink::Fill, Some(&mut image), Some(&mut fill), Some(&mut text), now);
        assert_eq!(fill.0, now);
        assert_eq!((image.color, text.0), (was, was));
        // A node with none of the colours a recolour writes is left alone.
        recolour::<&mut ImageNode, &mut BackgroundColor, &mut TextColor>(Ink::Drawn, None, None, None, now);
    }

    /// A model's parts take no Ink, so its colour changing spawns it again; only moved, it
    /// is changed in place.
    #[test]
    fn a_models_colour_change_is_answered_by_a_respawn() {
        let model = Piece {
            tile: Ui::new().screen(),
            look: Look::Model { filename: r"Interface\HUD\AirTimer01.NIF".into(), animation: Some("Forward".into()) },
            rect: TileRect { left: 10.0, top: 20.0, width: 64.0, height: 64.0 },
            clip: None,
            colour: [1.0, 0.7, 0.3, 1.0],
            depth: 0.0,
            tex: None,
            turn: None,
            crop: None,
        };
        let mut dimmer = model.clone();
        dimmer.colour[3] = 0.5;
        assert_eq!(change(&model, &dimmer), Change::Spawn);
        let mut moved = model.clone();
        moved.rect.left += 4.0;
        assert_eq!(change(&model, &moved), Change::InPlace);
    }

    #[test]
    fn a_tiles_pieces_are_kept_apart_by_their_count() {
        let (ui, _, menu) = vanilla_hud();
        let kept = [ui.find_tile("HUDMainMenu/Info").unwrap()];
        let (mut all, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &kept, strip: None }, false);
        all.push(all[0].clone());
        let keys = keys(&all);
        assert_eq!(keys[0], (all[0].tile, 0));
        assert_eq!(*keys.last().unwrap(), (all[0].tile, 1));
        assert_eq!(keys.iter().collect::<HashSet<_>>().len(), keys.len());
    }

    /// [design] Turns reach a picture's and a box's piece, not a text's; a piece only turned
    /// further changes in place; an unturned piece's node keeps the identity transform.
    #[test]
    fn a_turned_picture_carries_its_turn_and_turns_about_its_pivot() {
        let src = MemSource::default().with(
            "r.xml",
            r#"<menu name="R"><rect name="Clip"><width>50</width><height>50</height><clips>&true;</clips>
              <image name="Arrow"><filename>arrow.dds</filename><x>10</x><y>10</y><width>20</width><height>30</height><zoom>-1</zoom>
                <rotateangle>3.141592653589793</rotateangle><rotateaxisx>10</rotateaxisx><rotateaxisy>15</rotateaxisy></image>
              <rect name="Bar"><width>40</width><height>4</height><red>255</red><rotateangle>0.5</rotateangle></rect>
              <text name="Word"><string>N</string><rotateangle>1</rotateangle></text>
              <image name="Still"><filename>still.dds</filename><width>8</width><height>8</height></image></rect></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "r.xml").unwrap();
        ui.update();
        let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &[], strip: None }, true);
        let of = |name: &str| drawn.iter().find(|p| ui.tile(p.tile).name == name).unwrap();
        let arrow = of("Arrow");
        assert_eq!(arrow.turn, Some(layout::Turn { angle: std::f64::consts::PI, pivot: (10.0, 15.0) }));
        assert_eq!(arrow.clip, Some(TileRect { left: 0.0, top: 0.0, width: 50.0, height: 50.0 }), "the clip stays the unturned window");
        // About its middle: Bevy's own pivot, the node only turned.
        let t = turned(arrow);
        assert_eq!(t.translation, Val2::new(u(0.0), u(0.0)));
        // A half turn: by its sine and cosine (Rot2 may hold it as -pi).
        assert!((t.rotation.cos + 1.0).abs() < 1e-6 && t.rotation.sin.abs() < 1e-6, "{:?}", t.rotation);
        assert_eq!(of("Bar").turn.map(|t| t.pivot), Some((0.0, 0.0)));
        assert_eq!(of("Word").turn, None, "texts are not turned");
        assert_eq!(of("Still").turn, None);
        assert_eq!(turned(of("Still")), UiTransform::IDENTITY);
        // About its top left a quarter: moved by the shift the layout works out.
        let mut corner = arrow.clone();
        corner.turn = Some(layout::Turn { angle: std::f64::consts::FRAC_PI_2, pivot: (0.0, 0.0) });
        let (x, y) = corner.turn.unwrap().shift(20.0, 30.0);
        assert!((x - -25.0).abs() < 1e-9 && (y - -5.0).abs() < 1e-9, "({x}, {y})");
        assert_eq!(turned(&corner).translation, Val2::new(u(f(x)), u(f(y))));
        // Turned further, same look and size: in place.
        assert_eq!(change(arrow, &corner), Change::InPlace);
    }

    /// [design] The tree and menu a test paints through [`paint`] itself.
    #[derive(Resource)]
    struct Painted(Ui, TileId);

    fn paint_painted(mut commands: Commands, painted: Res<Painted>, mut kit: FontKit, mut nodes: Nodes, glyphs: Res<crate::pad::PadGlyphs>, mut drawn: Local<Drawn>) {
        let (set, _) = pieces(&painted.0, painted.1, &plain, Select::Whole { kept: &[], strip: None }, true);
        paint(&mut commands, &mut drawn, set, Target::window(1.0, 1), Brightness::File, 0.0, Some(&painted.0), &mut kit, &mut nodes, &glyphs);
    }

    /// A small page of its own (#1512) in one colour, 200 100 50, at the brightness levels a
    /// Pip-Boy page's file resolves: a bright text, a dimmed text, a line at 200, a faint fill
    /// at full brightness, a location icon at 255, and a text in another colour.
    const DEVICE_PAGE: &str = r#"<menu name="D"><rect name="Page"><width>400</width><height>300</height>
      <text name="Bright"><string>A</string><red>200</red><green>100</green><blue>50</blue></text>
      <text name="Dimmed"><string>B</string><red>200</red><green>100</green><blue>50</blue><brightness>128</brightness><y>30</y></text>
      <image name="Line"><filename>solid.dds</filename><width>100</width><height>2</height><red>200</red><green>100</green><blue>50</blue><brightness>200</brightness><y>60</y></image>
      <rect name="Fill"><width>50</width><height>20</height><red>200</red><green>100</green><blue>50</blue><alpha>32</alpha><y>90</y></rect>
      <image name="MapMarker"><filename>icon.dds</filename><width>20</width><height>20</height><red>200</red><green>100</green><blue>50</blue><y>120</y></image>
      <text name="Other"><string>C</string><red>10</red><green>20</green><blue>30</blue><y>150</y></text>
    </rect></menu>"#;
    const DEVICE_BRIGHT: [u8; 3] = [200, 100, 50];

    fn device_page() -> (Ui, TileId) {
        let src = MemSource::default().with("d.xml", DEVICE_PAGE);
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "d.xml").unwrap();
        ui.update();
        (ui, menu)
    }

    /// [design: #2212] On the device only the pieces at the page colour's full level take the
    /// hand-built pages' device brightness (213 of 255); the dimmed text, the line at 200, the
    /// location icon and a piece in another colour keep the file's; the alpha is never touched.
    /// The file's brightness (the screen menu, every window menu) changes nothing.
    #[test]
    fn a_pipboy_page_on_the_device_dims_only_its_full_bright_pieces() {
        let (ui, menu) = device_page();
        let (set, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &[], strip: None }, true);
        let of = |set: &[Piece], name: &str| set.iter().find(|p| ui.tile(p.tile).name == name).unwrap().colour;
        let full = DEVICE_BRIGHT.map(|c| f32::from(c) / 255.0);
        assert_eq!(of(&set, "Bright"), [full[0], full[1], full[2], 1.0], "the fixture's bright text is at its full colour");
        let mut file = set.clone();
        device_brightness(&mut file, Some(&ui), Brightness::File);
        assert_eq!(file, set, "the file's brightness: nothing changes");
        assert_eq!(Brightness::pipboy(false), Brightness::File, "the screen menu keeps the file's");
        let mut device = set.clone();
        device_brightness(&mut device, Some(&ui), Brightness::PipBoyDevice { bright: DEVICE_BRIGHT });
        let k = crate::pipboy::frame::DEVICE_TEXT / 255.0;
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        assert!(near(of(&device, "Bright"), [full[0] * k, full[1] * k, full[2] * k, 1.0]), "{:?}", of(&device, "Bright"));
        let fill = of(&set, "Fill");
        assert!((fill[3] - 32.0 / 255.0).abs() < 1e-6, "the fill is faint: {fill:?}");
        assert!(near(of(&device, "Fill"), [full[0] * k, full[1] * k, full[2] * k, fill[3]]), "a full-bright fill dimmed, its alpha kept: {:?}", of(&device, "Fill"));
        for kept in ["Dimmed", "Line", "MapMarker", "Other"] {
            assert_eq!(of(&device, kept), of(&set, kept), "{kept} keeps the file's colour");
        }
        // Without the tree the location icon is not known, and is at full colour: dimmed.
        let mut blind = set.clone();
        device_brightness(&mut blind, None, Brightness::PipBoyDevice { bright: DEVICE_BRIGHT });
        assert_ne!(of(&blind, "MapMarker"), of(&set, "MapMarker"));
        // Every Pip-Boy switch off: no pieces, nothing to change.
        let mut none: Vec<Piece> = Vec::new();
        device_brightness(&mut none, Some(&ui), Brightness::PipBoyDevice { bright: DEVICE_BRIGHT });
        assert!(none.is_empty());
    }

    /// [design] The brightness a test paints [`DEVICE_PAGE`] with.
    #[derive(Resource)]
    struct PaintedBrightness(Brightness);

    fn paint_device_page(mut commands: Commands, painted: Res<Painted>, brightness: Res<PaintedBrightness>, mut kit: FontKit, mut nodes: Nodes, glyphs: Res<crate::pad::PadGlyphs>, mut drawn: Local<Drawn>) {
        let (set, _) = pieces(&painted.0, painted.1, &plain, Select::Whole { kept: &[], strip: None }, true);
        paint(&mut commands, &mut drawn, set, Target::window(1.0, 1), brightness.0, 0.0, Some(&painted.0), &mut kit, &mut nodes, &glyphs);
    }

    /// [design: #2212] The brightness is part of what the painter compares: the frame switched
    /// from the device to the screen menu and back recolours the full-bright fill in place.
    #[test]
    fn a_pipboy_page_is_recoloured_when_its_frame_switches() {
        let (ui, menu) = device_page();
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<TileFonts>()
            .init_resource::<crate::pad::PadGlyphs>()
            .init_resource::<super::super::runtime_images::RuntimeImages>()
            .insert_resource(ScriptState::over_no_plugins())
            .insert_resource(Painted(ui, menu))
            .insert_resource(PaintedBrightness(Brightness::PipBoyDevice { bright: DEVICE_BRIGHT }))
            .add_systems(Update, paint_device_page);
        let fills = |app: &mut App| -> Vec<(Entity, Srgba)> {
            let world = app.world_mut();
            world.query::<(Entity, &Ink, &BackgroundColor)>().iter(world).filter(|(_, ink, _)| **ink == Ink::Fill).map(|(e, _, c)| (e, c.0.to_srgba())).collect()
        };
        let k = crate::pipboy::frame::DEVICE_TEXT / 255.0;
        let red = f32::from(DEVICE_BRIGHT[0]) / 255.0;
        app.update();
        let device = fills(&mut app);
        assert_eq!(device.len(), 1, "one fill: {device:?}");
        assert!((device[0].1.red - red * k).abs() < 1e-5 && (device[0].1.alpha - 32.0 / 255.0).abs() < 1e-5, "{device:?}");
        app.world_mut().resource_mut::<PaintedBrightness>().0 = Brightness::File;
        app.update();
        let screen = fills(&mut app);
        assert_eq!(screen[0].0, device[0].0, "recoloured in place");
        assert!((screen[0].1.red - red).abs() < 1e-5, "the screen menu: the file's 255: {screen:?}");
        app.world_mut().resource_mut::<PaintedBrightness>().0 = Brightness::PipBoyDevice { bright: DEVICE_BRIGHT };
        app.update();
        assert!((fills(&mut app)[0].1.red - red * k).abs() < 1e-5, "back on the device: dimmed again");
    }

    /// The pictures drawn: each node's entity and texture.
    fn drawn_images(app: &mut App) -> Vec<(Entity, Handle<Image>)> {
        let world = app.world_mut();
        world.query::<(Entity, &ImageNode)>().iter(world).map(|(e, i)| (e, i.image.clone())).collect()
    }

    /// [design] A runtime picture through the real painting: nothing while no entry is set,
    /// its texture once one is, repainted by a new handle, by the same handle set again and
    /// by a new size under the one name, kept while nothing changes, gone when removed.
    #[test]
    fn a_runtime_picture_repaints_when_its_entry_changes_under_the_same_name() {
        let src = MemSource::default().with(
            "rt.xml",
            r#"<menu name="RT"><image name="Map"><filename> nvf:test_map </filename><width>64</width><height>32</height><zoom>-1</zoom></image></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "rt.xml").unwrap();
        ui.update();
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<TileFonts>()
            .init_resource::<crate::pad::PadGlyphs>()
            .init_resource::<super::super::runtime_images::RuntimeImages>()
            .insert_resource(ScriptState::over_no_plugins())
            .insert_resource(Painted(ui, menu))
            .add_systems(Update, paint_painted);
        let image = |app: &mut App| app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
        let set = |app: &mut App, h: &Handle<Image>, size: Vec2| {
            let name = app.world_mut().resource_mut::<super::super::runtime_images::RuntimeImages>().set("test_map", h.clone(), size);
            assert_eq!(name, "nvf:test_map");
        };
        app.update();
        assert!(drawn_images(&mut app).is_empty(), "no entry: nothing drawn");
        let (a, b) = (image(&mut app), image(&mut app));
        set(&mut app, &a, Vec2::new(64.0, 32.0));
        app.update();
        let first = drawn_images(&mut app);
        assert_eq!(first.iter().map(|(_, h)| h.clone()).collect::<Vec<_>>(), std::slice::from_ref(&a), "set late: drawn");
        app.update();
        assert_eq!(drawn_images(&mut app), first, "nothing changed: the same node");
        set(&mut app, &b, Vec2::new(64.0, 32.0));
        app.update();
        let second = drawn_images(&mut app);
        assert_eq!(second.iter().map(|(_, h)| h.clone()).collect::<Vec<_>>(), std::slice::from_ref(&b), "a new handle, the same name: repainted");
        set(&mut app, &b, Vec2::new(64.0, 32.0));
        app.update();
        let third = drawn_images(&mut app);
        assert_eq!(third.len(), 1);
        assert_ne!(third[0].0, second[0].0, "the same handle set again (its texels changed): repainted");
        set(&mut app, &b, Vec2::new(128.0, 32.0));
        app.update();
        assert_ne!(drawn_images(&mut app)[0].0, third[0].0, "a new size: repainted");
        assert!(app.world_mut().resource_mut::<super::super::runtime_images::RuntimeImages>().remove("nvf:test_map"));
        app.update();
        assert!(drawn_images(&mut app).is_empty(), "removed: nothing drawn");
    }

    /// [design] A runtime name is looked up as a runtime picture whatever the tile's atlas,
    /// and its piece carries no stamp until painted.
    #[test]
    fn a_runtime_name_is_its_own_source() {
        let src = MemSource::default().with(
            "rn.xml",
            r#"<menu name="RN"><image name="A"><filename>NVF:Outline</filename><texatlas>Interface\InterfaceShared.tai</texatlas><width>9</width><height>9</height></image>
              <image name="B"><filename>Interface\nvf\b.dds</filename><width>9</width><height>9</height></image></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "rn.xml").unwrap();
        ui.update();
        let (set, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &[], strip: None }, true);
        assert!(matches!(&set[0].look, Look::Picture { source: Source::Runtime(None), .. }));
        assert!(matches!(&set[1].look, Look::Picture { source: Source::Own, .. }));
        // Stamped by its entry; a texture offset is dropped from it, said once.
        let mut set = set;
        set[0].tex = Some(super::super::texmod::TexMod { offset: (0.5, 0.0), mask: None });
        let mut images = super::super::runtime_images::RuntimeImages::default();
        images.set("outline", Handle::default(), Vec2::new(9.0, 9.0));
        let mut told = super::super::runtime_images::Told::default();
        super::super::runtime_images::mark(&mut set, Some(&images), &mut told);
        assert!(matches!(&set[0].look, Look::Picture { source: Source::Runtime(Some(s)), .. } if s.size == Vec2::new(9.0, 9.0)));
        assert!(set[0].tex.is_none() && told.texmod("nvf:outline"));
    }

    #[test]
    fn a_texatlas_names_the_index_its_picture_is_found_in() {
        // A fake shared index and a fake second one under textures\interface\mine\.
        let shared = interface::atlas_index("glow_a.dds\tInterfaceShared0.dds, 0, 2D, 0.5, 0.25, 0.0, 0.25, 0.125\n");
        let mut pictures = Pictures::with_atlas(shared);
        let reads = std::cell::RefCell::new(Vec::new());
        let read = |p: &str| -> Option<Vec<u8>> {
            reads.borrow_mut().push(p.to_string());
            (p == "textures\\interface\\mine\\mine.tai").then(|| b"# mine\nGlow_B.dds\tMine0.dds, 0, 2D, 0.0, 0.5, 0.0, 0.5, 0.5\n".to_vec())
        };
        let mine = Some(" Interface\\Mine\\Mine.tai ");
        assert_eq!(pictures.locate("Interface\\HUD\\glow_b.dds", mine, &read), interface::Located { texture: "textures\\interface\\mine\\mine0.dds".into(), part: Some([0.0, 0.5, 0.5, 0.5]) });
        // Not in the named index: the shared one's piece, else its own file.
        assert_eq!(pictures.locate("Interface\\HUD\\Glow_A.dds", mine, &read), interface::Located { texture: "textures\\interface\\interfaceshared0.dds".into(), part: Some([0.5, 0.25, 0.25, 0.125]) });
        assert_eq!(pictures.locate("\\Interface\\HUD\\c.dds", mine, &read), interface::Located { texture: "textures\\interface\\hud\\c.dds".into(), part: None });
        // The shared index by name is the one already read; a named index is read once.
        assert_eq!(pictures.locate("glow_a.dds", Some("Interface\\InterfaceShared.tai"), &read).part, Some([0.5, 0.25, 0.25, 0.125]));
        assert_eq!(*reads.borrow(), ["textures\\interface\\mine\\mine.tai"]);
        assert_eq!(interface::atlas_index_path("Interface/InterfaceShared.tai").as_deref(), Some(interface::ATLAS_INDEX));
        assert_eq!(interface::atlas_index_path("  "), None);
    }

    /// [design] A generated recipient's metadata is consumed by real paint, changes only
    /// its in-place transform, and does not overwrite XML or the texture revision.
    #[test]
    fn generated_recipient_presentation_reaches_actual_image_node_without_replacing_pixels() {
        let src = MemSource::default().with("recipient.xml", r#"<menu name="Recipient"><image name="Map"><filename>nvf:recipient</filename><width>80</width><height>40</height><zoom>-1</zoom><rotateangle>1</rotateangle><rotateaxisx>20</rotateaxisx><rotateaxisy>10</rotateaxisy></image></menu>"#);
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "recipient.xml").unwrap();
        ui.update();
        let recipient = ui.find_tile("Recipient/Map").unwrap();
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<TileFonts>()
            .init_resource::<crate::pad::PadGlyphs>()
            .init_resource::<super::super::runtime_images::RuntimeImages>()
            .insert_resource(ScriptState::over_no_plugins())
            .insert_resource(Painted(ui, menu))
            .add_systems(Update, paint_painted);
        let h = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
        app.world_mut().resource_mut::<super::super::runtime_images::RuntimeImages>().set("recipient", h.clone(), Vec2::new(80., 40.));
        let stamp = app.world().resource::<super::super::runtime_images::RuntimeImages>().stamp("recipient");
        let state = |app: &mut App| {
            let world = app.world_mut();
            world.query::<(Entity, &ImageNode, &UiTransform)>().iter(world)
                .map(|(e, i, t)| (e, i.image.clone(), *t)).collect::<Vec<_>>()
        };
        assert!(app.world_mut().resource_mut::<super::super::runtime_images::RuntimeImages>().set_recipient_axes("recipient", recipient, 1., 0.));
        app.update();
        let before = state(&mut app);
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].1, h);
        assert_eq!(before[0].2, UiTransform::IDENTITY);
        assert!(app.world_mut().resource_mut::<super::super::runtime_images::RuntimeImages>().set_recipient_axes("recipient", recipient, 0., 1.));
        app.update();
        let after = state(&mut app);
        assert_eq!(after.len(), 1);
        assert_eq!((after[0].0, &after[0].1), (before[0].0, &h));
        assert!((after[0].2.rotation.sin + 1.0).abs() < 1e-6 && after[0].2.rotation.cos.abs() < 1e-6);
        assert_eq!(app.world().resource::<super::super::runtime_images::RuntimeImages>().stamp("recipient"), stamp);
        assert_eq!(app.world().resource::<Painted>().0.get_float("Recipient/Map/rotateangle"), Some(1.));
    }

    #[test]
    fn font_kit_loads_tex_only_slot_1_and_keeps_font_9_undrawn() {
        use crate::interface::font_tex_tests::{synthetic_fnt, synthetic_tex, OwnedFontFiles};
        let files = OwnedFontFiles::new(&[
            ("glow_monofonto_large.fnt", synthetic_fnt("Glow_Monofonto_Large")),
            ("Glow_Monofonto_Large.Tex", synthetic_tex(4, 2, [255, 255, 255, 255])),
            ("nvfont_test.fnt", synthetic_fnt("NVFont_Test_0_Lod_A")),
            ("nvfont_test_0_lod_a.tex", synthetic_tex(4, 4, [255, 255, 255, 255])),
        ]);
        let src = MemSource::default().with(
            "fonts.xml",
            r#"<menu name="Fonts">
                <text name="Slot1"><string>A</string><font>1</font></text>
                <text name="Slot9"><string>A</string><font>9</font></text>
            </menu>"#,
        );
        let mut ui = Ui::new();
        ui.load_menu(&src, "fonts.xml").unwrap();
        ui.update();
        let t1 = ui.find_tile("Fonts/Slot1").unwrap();
        let t9 = ui.find_tile("Fonts/Slot9").unwrap();

        #[derive(Resource, Default)]
        struct Seen {
            f1_image: bool,
            f9_image: bool,
            f1_game: bool,
            f9_game: bool,
        }
        #[derive(Resource)]
        struct Probe {
            ui: Ui,
            t1: TileId,
            t9: TileId,
        }
        fn check(mut kit: FontKit, probe: Res<Probe>, mut seen: ResMut<Seen>) {
            seen.f1_game = kit.game_fonts(&probe.ui, &[probe.t1]);
            seen.f9_game = kit.game_fonts(&probe.ui, &[probe.t9]);
            seen.f1_image = kit.face(1.0).is_some_and(|f| f.image.is_some());
            seen.f9_image = kit.face(9.0).is_some_and(|f| f.image.is_some());
        }

        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<TileFonts>()
            .init_resource::<Seen>()
            .insert_resource(files.data())
            .insert_resource(Probe { ui, t1, t9 })
            .add_systems(Update, check);
        app.update();
        let seen = app.world().resource::<Seen>();
        assert!(seen.f1_image && seen.f1_game, "tex-only slot 1 should load");
        assert!(!seen.f9_image && !seen.f9_game, "vanilla font 9 should remain stand-in");
    }

    #[test]
    fn resolve_font_number_handles_vanilla_xfonts_zero_and_unregistered_ids() {
        let empty = BTreeMap::new();
        for (raw, expected) in [
            (0.0, 1),
            (-5.0, 1),
            (f64::NAN, 1),
            (f64::INFINITY, 1),
            (1.0, 1),
            (7.0, 7),
            (9.0, 9),
            (21.0, 1),
            (22.0, 1),
            (42.0, 1),
            (300.0, 1),
        ] {
            assert_eq!(resolve_font_number(raw, &empty), expected, "vanilla raw={raw}");
        }

        let mut extra = BTreeMap::new();
        extra.insert(21, "textures\\fonts\\mapmo_hud_caps_font.fnt".to_string());
        extra.insert(22, "textures\\fonts\\mapmo_hud_caps_font2.fnt".to_string());
        assert_eq!(resolve_font_number(0.0, &extra), 1);
        assert_eq!(resolve_font_number(1.0, &extra), 1);
        assert_eq!(resolve_font_number(9.0, &extra), 9);
        assert_eq!(resolve_font_number(21.0, &extra), 21);
        assert_eq!(resolve_font_number(22.0, &extra), 22);
        assert_eq!(resolve_font_number(42.0, &extra), 1, "unregistered 42 must fall back to font 1, not font 9");

        extra.insert(42, "textures\\fonts\\custom_42.fnt".to_string());
        assert_eq!(resolve_font_number(42.0, &extra), 42);
    }

    #[test]
    fn text_base_for_preserves_vanilla_and_computes_extended_ascent_baseline() {
        use crate::interface::font_tex_tests::synthetic_fnt_sized;
        let big = Font::parse(&synthetic_fnt_sized("mapmo_hud_caps_font", 76.7, 24.0, 58.0)).unwrap();
        for slot in 1..=9u8 {
            assert_eq!(text_base_for(slot, &big), text_base(slot), "vanilla slot {slot} unchanged");
        }
        // Extended slot 21 uses max ASCII ascent (58.0 >= 76.7 * 0.75 = 57.525).
        assert_eq!(text_base_for(21, &big), 58.0);
        // Extended slot with no drawn ASCII glyphs falls back to 0.75 * font.size.
        let stand_in = plain_font(80.0, 20.0);
        assert_eq!(text_base_for(21, &stand_in), 60.0);
        // text_lines uses text_base_for + font.size line_pitch for extended slots.
        let lines = text_lines(&big, 21, "A\nA", None, Alignment::Left, 100.0);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].2, 58.0);
        assert!((lines[1].2 - (58.0 + 76.7)).abs() < 1e-4);
    }

    #[test]
    fn font_kit_and_whole_pieces_resolve_mapmo_xfonts_21_22_and_fallback_0_and_42() {
        use crate::interface::font_tex_tests::{synthetic_fnt, synthetic_fnt_sized, synthetic_tex, OwnedFontFiles};

        let xml = r#"<menu name="XFontMenu">
            <text name="T0"><string>A</string><font>0</font></text>
            <text name="T1"><string>A</string><font>1</font></text>
            <text name="T9"><string>A</string><font>9</font></text>
            <text name="T21"><string>A</string><font>21</font></text>
            <text name="T22"><string>A</string><font>22</font></text>
            <text name="T42"><string>A</string><font>42</font></text>
        </menu>"#;

        #[derive(Resource, Default)]
        struct Seen {
            fonts_by_name: BTreeMap<String, u8>,
            widths_by_name: BTreeMap<String, f64>,
            f1_loaded: bool,
            f9_loaded: bool,
            f21_loaded: bool,
            f22_loaded: bool,
            game_fonts_21_22: bool,
            game_fonts_0_42: bool,
        }
        #[derive(Resource)]
        struct Probe {
            ui: Ui,
            menu: TileId,
        }
        fn run(mut kit: FontKit, probe: Res<Probe>, mut seen: ResMut<Seen>) {
            let pieces = whole_pieces(&probe.ui, probe.menu, &mut kit, true);
            for p in &pieces {
                let name = probe.ui.tile(p.tile).name.clone();
                if let Look::Text { font, .. } = p.look {
                    seen.fonts_by_name.insert(name.clone(), font);
                    seen.widths_by_name.insert(name, p.rect.width);
                }
            }
            seen.f1_loaded = kit.face(1.0).is_some_and(|f| f.image.is_some());
            seen.f9_loaded = kit.face(9.0).is_some_and(|f| f.image.is_some());
            seen.f21_loaded = kit.face(21.0).is_some_and(|f| f.image.is_some());
            seen.f22_loaded = kit.face(22.0).is_some_and(|f| f.image.is_some());
            let t0 = probe.ui.find_tile("XFontMenu/T0").unwrap();
            let t21 = probe.ui.find_tile("XFontMenu/T21").unwrap();
            let t22 = probe.ui.find_tile("XFontMenu/T22").unwrap();
            let t42 = probe.ui.find_tile("XFontMenu/T42").unwrap();
            seen.game_fonts_21_22 = kit.game_fonts(&probe.ui, &[t21, t22]);
            seen.game_fonts_0_42 = kit.game_fonts(&probe.ui, &[t0, t42]);
        }

        // Case 1: With MAPMO.txt registering slots 21 and 22 (.fnt + .Tex, no .dds).
        let mapmo_txt = b"21=textures\\fonts\\mapmo_hud_caps_font.fnt\n\
                          22=textures\\fonts\\mapmo_hud_caps_font2.fnt\n";
        let modded_files = OwnedFontFiles::with_extra_files(
            &[
                ("glow_monofonto_large.fnt", synthetic_fnt("Glow_Monofonto_Large")),
                ("Glow_Monofonto_Large.Tex", synthetic_tex(4, 2, [255, 255, 255, 255])),
                ("nvfont_test.fnt", synthetic_fnt("NVFont_Test_0_Lod_A")),
                ("nvfont_test_0_lod_a.tex", synthetic_tex(4, 4, [255, 255, 255, 255])),
                ("mapmo_hud_caps_font.fnt", synthetic_fnt_sized("mapmo_hud_caps_font", 76.7, 24.0, 58.0)),
                ("mapmo_hud_caps_font.Tex", synthetic_tex(8, 4, [20, 120, 220, 255])),
                ("mapmo_hud_caps_font2.fnt", synthetic_fnt_sized("mapmo_hud_caps_font2", 76.7, 32.0, 60.0)),
                ("mapmo_hud_caps_font2.Tex", synthetic_tex(8, 4, [40, 140, 240, 255])),
            ],
            &[("NVSE/Plugins/xfonts/MAPMO.txt", mapmo_txt.to_vec())],
        );
        let src = MemSource::default().with("xfonts.xml", xml);
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "xfonts.xml").unwrap();
        ui.update();

        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<TileFonts>()
            .init_resource::<Seen>()
            .insert_resource(modded_files.data())
            .insert_resource(Probe { ui, menu })
            .add_systems(Update, run);
        app.update();
        let seen = app.world().resource::<Seen>();
        assert_eq!(seen.fonts_by_name.get("T0"), Some(&1));
        assert_eq!(seen.fonts_by_name.get("T1"), Some(&1));
        assert_eq!(seen.fonts_by_name.get("T9"), Some(&9));
        assert_eq!(seen.fonts_by_name.get("T21"), Some(&21));
        assert_eq!(seen.fonts_by_name.get("T22"), Some(&22));
        assert_eq!(seen.fonts_by_name.get("T42"), Some(&1));
        assert!(seen.f1_loaded && seen.f21_loaded && seen.f22_loaded);
        assert!(!seen.f9_loaded, "vanilla font 9 must stay unloaded");
        assert!(seen.game_fonts_21_22 && seen.game_fonts_0_42);
        // Distinct glyph widths prove slots 1, 21, and 22 measured with their own faces.
        assert_eq!(seen.widths_by_name.get("T1"), Some(&11.0));
        assert_eq!(seen.widths_by_name.get("T0"), Some(&11.0));
        assert_eq!(seen.widths_by_name.get("T42"), Some(&11.0));
        assert_eq!(seen.widths_by_name.get("T21"), Some(&25.0));
        assert_eq!(seen.widths_by_name.get("T22"), Some(&33.0));

        // Case 2: Unmodded vanilla (no xfonts directory) resolves 0, 21, 22, 42 all to slot 1.
        let vanilla_files = OwnedFontFiles::new(&[
            ("glow_monofonto_large.fnt", synthetic_fnt("Glow_Monofonto_Large")),
            ("Glow_Monofonto_Large.Tex", synthetic_tex(4, 2, [255, 255, 255, 255])),
        ]);
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "xfonts.xml").unwrap();
        ui.update();
        let mut vanilla_app = App::new();
        vanilla_app
            .init_resource::<Assets<Image>>()
            .init_resource::<TileFonts>()
            .init_resource::<Seen>()
            .insert_resource(vanilla_files.data())
            .insert_resource(Probe { ui, menu })
            .add_systems(Update, run);
        vanilla_app.update();
        let vseen = vanilla_app.world().resource::<Seen>();
        for tile in ["T0", "T1", "T21", "T22", "T42"] {
            assert_eq!(vseen.fonts_by_name.get(tile), Some(&1), "vanilla {tile} -> slot 1");
            assert_eq!(vseen.widths_by_name.get(tile), Some(&11.0), "vanilla {tile} width");
        }
        assert_eq!(vseen.fonts_by_name.get("T9"), Some(&9));
        assert!(!vseen.f9_loaded);
    }

    #[test]
    fn string_measure_with_fonts_measures_xfonts_and_falls_back_for_0_and_42() {
        let mut extra = BTreeMap::new();
        extra.insert(21, "textures\\fonts\\mapmo_hud_caps_font.fnt".to_string());
        let f1 = plain_font(31.0, 10.0);
        let f21 = plain_font(76.7, 25.0);
        let measure = string_measure_with_fonts(extra, move |n| match n {
            1 => Some(f1.clone()),
            21 => Some(f21.clone()),
            _ => None,
        });
        assert_eq!(measure(1.0, "AB", 0.0), (20.0, 31.0, 1.0));
        assert_eq!(measure(0.0, "AB", 0.0), (20.0, 31.0, 1.0));
        assert_eq!(measure(42.0, "AB", 0.0), (20.0, 31.0, 1.0));
        let (w, h, lines) = measure(21.0, "AB", 0.0);
        assert_eq!((w, lines), (50.0, 1.0));
        assert!((h - 76.7f32 as f64).abs() < 1e-4);
    }

    #[test]
    fn face_slot_path_preserves_single_pass_ini_swaps_and_font_9_unloaded_guard() {
        let mut extra = BTreeMap::new();
        extra.insert(21, "textures\\fonts\\mapmo_hud_caps_font.fnt".to_string());
        // Simulate an install INI that swaps sFontFile_1 -> FONT_FILES[1],
        // sFontFile_2 -> FONT_FILES[2], and attempts to remap sFontFile_9.
        let ini = format!(
            "[Fonts]\nsFontFile_1={}\nsFontFile_2={}\nsFontFile_9=textures\\fonts\\custom_9.fnt\n",
            interface::FONT_FILES[1],
            interface::FONT_FILES[2],
        );
        let install_fonts = interface::fonts_in_ini(&ini);

        // Vanilla slots 1..=9 pass the raw FONT_FILES entry into interface::face(),
        // so face() resolves font_file_in() exactly once (no double swap on slot 1 -> 2 -> 3)
        // and catches Font 9 via is_unloaded_font() BEFORE font_file_in() runs.
        for slot in 1..=9u8 {
            assert_eq!(
                face_slot_path(slot, Some(&extra)),
                interface::FONT_FILES[usize::from(slot) - 1],
                "slot {slot} must pass raw FONT_FILES entry to interface::face()"
            );
        }
        let raw_slot_1 = face_slot_path(1, Some(&extra));
        assert!(!interface::is_unloaded_font(raw_slot_1));
        assert_eq!(
            interface::font_file_in(raw_slot_1, Some(&install_fonts)),
            interface::FONT_FILES[1],
            "single-pass font_file_in keeps sFontFile_1 swap at FONT_FILES[1]"
        );
        let raw_slot_9 = face_slot_path(9, Some(&extra));
        assert!(
            interface::is_unloaded_font(raw_slot_9),
            "pre-font_file unloaded guard must catch Font 9 even when sFontFile_9 is overridden"
        );
        // Extended slots >= 10 resolve from extra directly; unregistered slots fall back to slot 1.
        assert_eq!(face_slot_path(21, Some(&extra)), "textures\\fonts\\mapmo_hud_caps_font.fnt");
        assert_eq!(face_slot_path(42, Some(&extra)), interface::FONT_FILES[0]);
        assert_eq!(face_slot_path(21, None), interface::FONT_FILES[0]);
    }

}

