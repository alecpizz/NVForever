//! The Pip-Boy device's screen treatment (docs/spec-notes/pipboy.md §3), one owner around
//! the shared image ([`PipBoyScreen`]), so that whatever paints the pages into it gets the
//! same screen: the undercoat (the screen camera's clear colour), the background picture
//! under the pages, the scanlines over them, and the coefficients the body's material
//! (nvforever `pipboy_arm.rs`) puts the image and the glare on the model with.
//!
//! A static fit, all its numbers in this file. Its shape comes from a capture of the original
//! [observed: private job pipboy_special_device_base: 1280 × 720, noon in Goodsprings, level
//! view, the STATS page, 11 × 11 pixel means at eight empty places of the screen and on a filled
//! bar]: the falloff to the edges is the background picture's, and the glow across the top and
//! the patch on the right are the glare's, so the undercoat is left with next to nothing. Its
//! levels and tone are fitted to playtest shots of the original at 1920 × 1080, brighter than
//! that capture [observed: 2026-11-05, pipboy.md §3]. `ScreenLit:8` has no part in it:
//! `screen.dds` is black, `Screen_n.dds` flat and its specular colour black [observed: the
//! files; `fnvtool nif decode`], so it stays hidden. None of this is lit by the world [design:
//! both shapes are unlit]. Other hours and custom colours are not measured. Flicker is not
//! built; the page change's wash, shudder and vertical hold are `transition.rs`'s.
//!
//! One moving layer: the beam ([`ScreenFxBeam`], [`sweep`]), a wave of light that travels down
//! the screen every [`BEAM_PERIOD`] seconds: a hard leading edge at its foot and a long fade
//! behind it [observed: private jobs pipboy_special_device_motion and pipboy_fx_still; playtest
//! 2026-11-05]. It lies over the pages and under the scanlines, so the lines cross it. Between
//! sweeps it is hidden. Its picture carries a dither, so its slow rise does not quantise into
//! bands in the 8-bit image ([`beam_pixels`]).
//!
//! Two parts are the screen material's, on the model only (nvf_assets `nif_material.wesl`
//! `pipboy_picture`, through `transition::ScreenLook`): the text's edges softened
//! ([`TEXT_SOFTEN`]) and a fine noise new every frame ([`SCREEN_NOISE`]).
//!
//! The path from the image to the frame [design: `pipboy_arm.rs`, `nvf_assets::nif_spawn`
//! `unlit_vertex_colors`, `imagespace.wesl`]: image × [`SCREEN_TINT`] ×
//! [`SHAPE_VERTEX_COLOUR`], plus the glare's texel × [`GLARE_LINEAR_SCALE`] ×
//! [`SHAPE_VERTEX_COLOUR`], through the world camera's tone mapping. The scene's image-space
//! grade leaves the screen's pixels alone (`imagespace.wesl`, `SCREEN_MARK`).
//!
//! The Pip-Boy's colour reaches the picture here ([`apply_colour`]), not through
//! `ui_colour::recolour`, which leaves every [`ScreenFxNode`] alone: the picture's tint is
//! raised and divided by its alpha, so it is no brightness of the colour for that to find,
//! and the undercoat is the glass, which keeps its colour.
//!
//! The screen menu (`Frame::Screen`) has none of this: no undercoat, no background, no
//! scanlines, no glass (docs/design/screen-menu.md §2).
//!
//! Tuning on a STATS shot at 1920 × 1080, indoors, the per-pixel median of a few shots so the
//! beam drops out, remeasuring after each change, in this order:
//! - [`SCREEN_TINT`], on a filled bar and on mid-tone ink: scale a channel by the light of the
//!   target over the light of the reading (undo the tone map; near linear in the middle tones).
//! - [`BACKGROUND_BRIGHTNESS`] and [`BACKGROUND_RED`], on the empty screen beside the figure.
//! - [`GLARE_LINEAR_SCALE`], at the top middle, above the figure.
//! - [`UNDERCOAT_LIGHT`], last, only if the far edges stay wrong.

use super::frame::{scanline_column, Ctx, Kit};
use super::{PipBoy, PipBoyScreen, CANVAS};
use crate::interface::{at, pipboy_rgb, tint, AMBER_RGB};
use crate::terminal::screen::{lifted, scanline_column as baked_column, scanline_pixels};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use nvf_assets::GameData;

/// What the screen multiplies the menus' picture by (linear), the view's tone map after it, with
/// red stopped at a ceiling and the scene's grade left off (`nif_material.wesl`,
/// `PIPBOY_SCREEN`; `imagespace.wesl`, `SCREEN_MARK`). Fitted so a
/// filled bar comes out near the original's about (206, 163, 57), with green and blue dipping
/// on the scanlines and red almost flat, 201 to 211, and the ink's mid tones at its yellower
/// hue, green about 0.74 of red and blue about 0.3; the original reads the same indoors and at
/// dusk [observed: playtest shots of the original at 1920 x 1080, 2026-11-05]. The earlier
/// (0.51, 0.648, 1.389) was fitted to the dimmer private job pipboy_special_device_base.
pub const SCREEN_TINT: [f32; 3] = [1.8, 1.9, 2.1];

/// The vertex colour both the screen's and the glare's shapes carry, in linear light: 0.81
/// in `pipboyarm.nif` [observed: `fnvtool nif decode`], taken as a display value on an
/// unlit shape (`nif_spawn::unlit_vertex_colors`).
pub const SHAPE_VERTEX_COLOUR: f32 = 0.624;

/// What the undercoat adds to the view (linear, before tone map and grade), next to nothing:
/// the far edges of the screen are almost black in the original, (7.4, 5.7, 2.0) on the left
/// [observed: private job pipboy_special_device_base; fitted with the two below].
pub const UNDERCOAT_LIGHT: [f32; 3] = [0.00047, 0.00055, 0.00054];

/// The glare's linear scale on `ScreenGlare.dds`, its colour warmer than the texture's: fitted
/// to the top middle above the figure, about (86, 76, 50) [observed: playtest shots of the
/// original at 1920 x 1080, 2026-11-05]. The terminal's `GLARE_LINEAR_SCALE` is measured for
/// its own glass and is not this.
pub const GLARE_LINEAR_SCALE: [f32; 3] = [3.2, 1.4, 0.75];

/// The background picture over the whole canvas. Its vignette is the screen's falloff. The
/// menu files give it brightness 90 [documented: as `frame.rs`]; it takes 120 to match beside
/// the figure, about (35, 27, 15) [observed: playtest shots of the original at 1920 x 1080,
/// 2026-11-05; fitted].
const BACKGROUND: &str = "Interface\\Shared\\Background\\pipboy.dds";
pub const BACKGROUND_BRIGHTNESS: f32 = 120.0;
/// The picture's red, in linear light: the original's empty screen is less red than amber at
/// this brightness, about (21, 17, 7) against (26, 18, 8) [observed: playtest shots of the
/// original at 1920 x 1080, 2026-11-05; fitted].
const BACKGROUND_RED: f32 = 0.7;
/// How much of the undercoat the picture covers. What it adds stays what it would over
/// black, its vignette whole; the undercoat shows through [design: the terminal's way,
/// `terminal::screen::BACKGROUND_ALPHA`; the value is its].
const BACKGROUND_ALPHA: f32 = 0.4;

/// The scanlines: the band of `pipboyscanlines.dds`, again every 9.5 of the canvas, dimming
/// the display values by about 15% at its dark end [observed: pipboy.md §3, the solid
/// bar's rows on the owner's ITEMS screenshot].
const SCANLINES: &str = "textures\\pipboy3000\\pipboyscanlines.dds";
pub const SCANLINE_PITCH: f32 = 9.5;
/// The dip at the dark end: 16 to 22% between the brightest and the darkest rows of an
/// empty stretch of the original's screen at 1920 × 1080, indoors and at dusk alike, over a
/// 9-pixel period [observed: playtest shots of the original, 2026-11-05]. It gives about 22% on
/// the empty screen here, and about 18% in green on a filled bar, where the original has 21%;
/// the empty screen is the larger part of the view, and 24% here measured 27% on it against
/// the original's 19% [observed: this engine's shots, 2026-11-05; the choice is design]. The
/// earlier 15% was the ITEMS screenshot's reading.
pub const SCANLINE_DEPTH: f32 = 0.20;

/// The beam (module docs). Seconds between sweeps: its leading edge passed the same row 5.12 s
/// apart by the wall clock [observed: private job pipboy_fx_still, 2026-11-10; the earlier 5
/// was read on a whole-second clock]. Seconds from just above the top edge to off the foot:
/// its edge went 231 and 239 rows a second down the 1280 x 720 shots in two jobs (156 on one
/// sweep) [observed: the same job], about 415 canvas units a second
/// (`transition::CANVAS_PER_ROW`), so the canvas and the wave in 2.3 s. The earlier 1.3 s was a
/// [hypothesis] from a whole-second clock, nearly twice as fast.
pub const BEAM_PERIOD: f32 = 5.12;
pub const BEAM_TRAVEL: f32 = 2.3;
/// Its rise and its leading edge in canvas units, and its height, the two together. On the
/// mean of frames aligned at the edge the light rises about straight over 72 rows of the 720
/// (55 from 10 to 90 %) to a peak of +14 in display red, then falls to nothing over 24 rows
/// (14 from 90 to 10 %) [observed: private job pipboy_fx_still, 3 frames holding the whole
/// wave]. The rows are the measured numbers; at 1.8 canvas units a row
/// (`transition::CANVAS_PER_ROW` [hypothesis]) they are 129.6 and 43.2, so 130 and 43, 173 in
/// all. The earlier 224 and 24 were read on single frames with the measure's own smoothing in
/// them.
pub const BEAM_RISE: f32 = 130.0;
pub const BEAM_EDGE: f32 = 43.0;
pub const BEAM_HEIGHT: f32 = BEAM_RISE + BEAM_EDGE;
/// How strongly it covers the image at its peak, fitted to that +14 [hypothesis: fitted on the
/// playtest shots of the original at 1920 x 1080, where the same step reads 13 to 14; the
/// private job's peak is 13.6].
pub const BEAM_ALPHA: f32 = 0.016;
/// The beam's peak in display levels, for its dither: one display level is about this share
/// of its picture's alpha range (the private job's 13.6 [observed]).
const BEAM_LEVELS: f32 = 13.6;
/// The beam's picture is this many columns wide, twice the canvas, and a canvas's width of it
/// is shown, from a new place every frame, so its dither changes from frame to frame [design].
const BEAM_COLUMNS: usize = 2 * CANVAS.x as usize;

/// How far apart the screen's taps of the menus' picture are, in the view's pixels, on the
/// model (`nif_material.wesl` `pipboy_picture`: a 3 x 3 tent). The original's text edges rise
/// from 10 to 90 % over about 1.6 pixels at 1280 x 720 on most of its text, 1.8 to 2.1 on the
/// titles, where a hard edge measures 0.8 [observed: private jobs pipboy_fx_still and
/// pipboy_fx_text, every STATS tab, ITEMS and DATA; the same with fBlurRadiusPipboy at 0]. A
/// tent of this reach brings NVForever's to about that [observed: the same measure on this
/// engine's 1280 x 720 shots, scenario pipboy-screen-text: with none, 1.48 to 1.60 px; at 0.75,
/// 1.69 to 1.96, past the original; fitted between].
pub const TEXT_SOFTEN: f32 = 0.4;

/// The reach of the screen's noise in display values (triangular, new every frame), on the
/// model (`nif_material.wesl` `pipboy_picture`): an empty stretch of the original's screen
/// varies by 0.61 levels (standard deviation) from frame to frame, and the beam's rise steps
/// up and down a row at a time, not in bands [observed: private job pipboy_fx_still, 109 frames
/// without the beam; the rise's runs a row long at the median]. This reach gives about 0.55
/// levels, and about 0.6 once rounded to whole levels [design].
pub const SCREEN_NOISE: f32 = 1.35 / 255.0;
/// How far its colour is lightened from the Pip-Boy's towards white: what it adds on the
/// original's screen is about (1, 0.58, 0.17) in linear light [observed: the same frames].
const BEAM_LIGHTEN: f32 = 0.2;

/// Where the beam's top edge is at `phase` seconds into its period: from a band's height
/// above the canvas to the canvas's foot, in [`BEAM_TRAVEL`]; `None` while it is hidden.
pub fn beam_top(phase: f32) -> Option<f32> {
    (0.0..BEAM_TRAVEL).contains(&phase).then(|| -BEAM_HEIGHT + (CANVAS.y + BEAM_HEIGHT) * phase / BEAM_TRAVEL)
}

/// The wave's shape at row `y` of the beam from its top, 0 to 1: a straight rise to the peak,
/// then the leading edge.
fn beam_weight(y: usize) -> f32 {
    let rows = BEAM_HEIGHT as usize;
    let edge = BEAM_EDGE as usize;
    if y + edge < rows {
        (y as f32 + 0.5) / (rows - edge) as f32
    } else {
        0.5 + 0.5 * (std::f32::consts::PI * ((y + edge - rows) as f32 + 0.5) / edge as f32).cos()
    }
}

/// A whole-number hash (PCG), for the beam's dither.
fn hash(v: u32) -> u32 {
    let state = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// A triangular value in -1..1 for texel (`x`, `y`).
fn dither(x: usize, y: usize) -> f32 {
    let unit = |k: u32| (hash(x as u32 ^ hash(y as u32 ^ hash(k))) >> 8) as f32 / (1u32 << 24) as f32;
    unit(0) + unit(1) - 1.0
}

/// The beam's picture, [`BEAM_COLUMNS`] wide, white, with the wave's shape in its alpha from
/// the top down ([`beam_weight`]) and a triangular dither of about a display level on it, less
/// where the alpha is near either end so it never clips: the 8-bit image under it then holds
/// the rise as a mix of neighbouring levels, not as bands about five rows wide (module docs).
/// The strength is on the node's colour, so a faint beam keeps a full range of alpha here.
fn beam_pixels() -> Vec<u8> {
    let level = 255.0 / BEAM_LEVELS;
    let rows = BEAM_HEIGHT as usize;
    let mut out = Vec::with_capacity(rows * BEAM_COLUMNS * 4);
    for y in 0..rows {
        let v = beam_weight(y) * 255.0;
        let reach = level.min(v).min(255.0 - v);
        for x in 0..BEAM_COLUMNS {
            let a = (v + reach * dither(x, y)).round().clamp(0.0, 255.0) as u8;
            out.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    out
}

/// Where the beam's picture is read from on frame `frame`: a canvas's width of it from a
/// column that changes every frame ([`BEAM_COLUMNS`]).
fn beam_window(frame: u32) -> Rect {
    let x = (hash(frame) % (BEAM_COLUMNS as u32 - CANVAS.x as u32)) as f32;
    Rect::new(x, 0.0, x + CANVAS.x, BEAM_HEIGHT)
}

/// The beam's colour for the Pip-Boy colour `rgb`, its strength in the alpha.
fn beam_tint(rgb: [u8; 3]) -> Color {
    let lighter = rgb.map(|c| f32::from(c) / 255.0).map(|c| c + (1.0 - c) * BEAM_LIGHTEN);
    Color::srgba(lighter[0], lighter[1], lighter[2], BEAM_ALPHA)
}

fn linear(v: f32) -> f32 {
    Color::srgb(v, 0.0, 0.0).to_linear().red
}

/// The background picture's colour over black: amber at its brightness, in linear light.
fn background_light(rgb: [u8; 3]) -> Vec3 {
    Vec3::from_array(rgb.map(|c| linear(f32::from(c) / 255.0 * BACKGROUND_BRIGHTNESS / 255.0))) * Vec3::new(BACKGROUND_RED, 1.0, 1.0)
}

/// The undercoat before the scanlines' lift: [`UNDERCOAT_LIGHT`], back through the material's
/// product and from under the picture. The glass, which does not follow the interface's
/// colour [hypothesis].
fn glass_undercoat() -> Vec3 {
    Vec3::from_array(UNDERCOAT_LIGHT) / (Vec3::from_array(SCREEN_TINT) * SHAPE_VERTEX_COLOUR) / (1.0 - BACKGROUND_ALPHA)
}

/// The undercoat, the screen camera's clear colour on the device: raised by what the
/// scanlines take away on average (`kept`), as the terminal's is.
pub fn undercoat(kept: f32) -> Color {
    let u = glass_undercoat();
    lifted(Color::linear_rgb(u.x, u.y, u.z), kept)
}

/// The picture's colour and alpha over the undercoat: what it adds stays the amber's at its
/// brightness, raised as the undercoat is. Follows the interface's colour.
fn picture_tint(rgb: [u8; 3], kept: f32) -> Color {
    let p = background_light(rgb) / BACKGROUND_ALPHA;
    lifted(Color::linear_rgba(p.x, p.y, p.z, BACKGROUND_ALPHA), kept)
}

/// The band's darkening rescaled to `depth` at its dark end, in display values. A band with
/// no dark end stays as it is.
pub fn band_at_depth(band: &[f32], depth: f32) -> Vec<f32> {
    let dip = 1.0 - band.iter().copied().fold(1.0, f32::min);
    if dip <= 0.0 {
        return band.to_vec();
    }
    band.iter().map(|&l| 1.0 - depth * (1.0 - l) / dip).collect()
}

/// The treatment's parts made once.
#[derive(Resource)]
pub struct ScreenFx {
    scanlines: Option<Handle<Image>>,
    /// The beam's column ([`beam_pixels`]).
    beam: Option<Handle<Image>>,
    /// How much of the light the scanlines keep on average, in linear light (1 without).
    pub kept: f32,
    baked: bool,
    /// The Pip-Boy colour the picture is tinted for.
    rgb: [u8; 3],
    /// The beam's first sweep since the treatment came on was logged.
    swept: bool,
    announced: bool,
}

impl Default for ScreenFx {
    fn default() -> Self {
        ScreenFx { scanlines: None, beam: None, kept: 1.0, baked: false, rgb: AMBER_RGB, swept: false, announced: false }
    }
}

/// A one-column picture of `rgba` rows, read with linear filtering, for a layer stretched
/// over the canvas.
fn column_image(rgba: Vec<u8>, what: &str) -> Option<Image> {
    let format = bevy::render::render_resource::TextureFormat::Rgba8Unorm;
    let mut image = nvf_assets::texture::checked_image(1, (rgba.len() / 4) as u32, 1, format, rgba).map_err(|e| warn!("pipboy: {what}: {e}")).ok()?;
    image.asset_usage = bevy::asset::RenderAssetUsages::default();
    image.sampler = bevy::image::ImageSampler::linear();
    Some(image)
}

/// The beam's picture, read texel for texel (its node is drawn one texel to a canvas unit on
/// the device), so the dither stays a texel's.
fn beam_image() -> Option<Image> {
    let format = bevy::render::render_resource::TextureFormat::Rgba8Unorm;
    let mut image = nvf_assets::texture::checked_image(BEAM_COLUMNS as u32, BEAM_HEIGHT as u32, 1, format, beam_pixels()).map_err(|e| warn!("pipboy: beam: {e}")).ok()?;
    image.asset_usage = bevy::asset::RenderAssetUsages::default();
    image.sampler = bevy::image::ImageSampler::nearest();
    Some(image)
}

impl ScreenFx {
    /// One baked column over the whole canvas, a value per row, as the terminal's: no tile
    /// per line, whose fractional pitch rounded to whole rows. A broken file gives no
    /// scanlines, not a GPU error. The beam's column needs no file.
    fn bake(&mut self, data: Option<&GameData>, images: &mut Assets<Image>) {
        if self.baked {
            return;
        }
        self.baked = true;
        self.beam = beam_image().map(|i| images.add(i));
        let Some((image, kept)) = scanline_image(data) else { return };
        self.scanlines = Some(images.add(image));
        self.kept = kept;
    }
}

/// The same decoded column used by the device, before any owner adds its image.
fn scanline_image(data: Option<&GameData>) -> Option<(Image, f32)> {
    let band = data.and_then(|d| d.read(SCANLINES)).and_then(|b| scanline_column(&b))
        .filter(|b| !b.is_empty())?;
    let column = baked_column(&band_at_depth(&band, SCANLINE_DEPTH), CANVAS.y as usize, SCANLINE_PITCH);
    let (rgba, kept) = scanline_pixels(&column);
    Some((column_image(rgba, SCANLINES)?, kept))
}

/// Independent dialogue art from the actual Pip-Boy picture/scanline pipeline.
/// These handles are newly loaded here, never the gameplay ScreenFx resources.
pub struct DialogueScreenArt {
    background: Handle<Image>,
    rect: Option<Rect>,
    scanlines: Option<Handle<Image>>,
    kept: f32,
}

impl DialogueScreenArt {
    pub fn load(data: &GameData, images: &mut Assets<Image>, bc: bool) -> Option<Self> {
        let mut pictures = crate::interface::Pictures::load(Some(data));
        let (background, rect) = pictures.picture(BACKGROUND, Some(data), images, bc)?;
        let (scanlines, kept) = scanline_image(Some(data))
            .map_or((None, 1.0), |(image, kept)| (Some(images.add(image)), kept));
        Some(Self { background, rect, scanlines, kept })
    }

    /// Parent supplies the actual dialogue content camera. Art never claims input.
    pub fn spawn(&self, parent: &mut ChildSpawnerCommands, canvas: Rect) {
        let node = || Node {
            position_type: PositionType::Absolute,
            left: Val::Px(canvas.min.x), top: Val::Px(canvas.min.y),
            width: Val::Px(canvas.width()), height: Val::Px(canvas.height()),
            ..default()
        };
        parent.spawn((ScreenFxNode, node(), ImageNode { rect: self.rect,
            color: picture_tint(pipboy_rgb(), self.kept), image_mode: NodeImageMode::Stretch,
            ..ImageNode::new(self.background.clone()) }, ZIndex(-1),
            bevy::picking::Pickable::IGNORE, bevy::ui::FocusPolicy::Pass,
            Name::new("Dialogue Pip-Boy actual background")));
        if let Some(image) = &self.scanlines {
            parent.spawn((ScreenFxNode, node(), ImageNode { image_mode: NodeImageMode::Stretch,
                ..ImageNode::new(image.clone()) }, ZIndex(10),
                bevy::picking::Pickable::IGNORE, bevy::ui::FocusPolicy::Pass,
                Name::new("Dialogue Pip-Boy actual scanlines")));
        }
    }

    /// Call after retiring this owner's nodes. Neither image came from a shared cache.
    pub fn remove_images(self, images: &mut Assets<Image>) {
        images.remove(self.background.id());
        if let Some(image) = self.scanlines { images.remove(image.id()); }
    }
}

/// A layer of the treatment in the image; they outlive the pages' redraws.
#[derive(Component)]
pub struct ScreenFxLayer;

/// Any node of the treatment: `ui_colour::recolour` leaves it alone (module docs).
#[derive(Component, Default)]
pub struct ScreenFxNode;

/// The node under the pages that shows the background: the picture, or a faint fill
/// without it.
#[derive(Component)]
pub struct ScreenFxPicture;

/// The beam's node (module docs), moved by [`sweep`].
#[derive(Component)]
pub struct ScreenFxBeam;

/// The faint fill where the picture is missing.
fn missing_picture_fill(rgb: [u8; 3]) -> Color {
    tint(rgb, BACKGROUND_BRIGHTNESS).with_alpha(0.1)
}

type Pictures<'w, 's> = Query<'w, 's, (Option<&'static mut ImageNode>, Option<&'static mut BackgroundColor>), With<ScreenFxPicture>>;
type Beams<'w, 's> = Query<'w, 's, &'static mut ImageNode, (With<ScreenFxBeam>, Without<ScreenFxPicture>)>;

/// The picture's and the beam's nodes recoloured for `rgb`, when that is not the colour they have.
fn apply_colour(fx: &mut ScreenFx, rgb: [u8; 3], pictures: &mut Pictures, beams: &mut Beams) {
    if fx.rgb == rgb {
        return;
    }
    fx.rgb = rgb;
    for (image, fill) in pictures {
        if let Some(mut image) = image {
            image.color = picture_tint(rgb, fx.kept);
        }
        if let Some(mut fill) = fill {
            fill.0 = missing_picture_fill(rgb);
        }
    }
    for mut beam in beams {
        beam.color = beam_tint(rgb);
    }
    info!("pipboy: the screen's picture recoloured to {rgb:?}");
}

/// Under every page and over every page, in the image's own stack; the beam between the
/// pages and the scanlines.
const UNDER_PAGES: GlobalZIndex = GlobalZIndex(-1);
const OVER_PAGES: GlobalZIndex = GlobalZIndex(10);
const BEAM: GlobalZIndex = GlobalZIndex(9);

/// The treatment shows with the device's frame, not in the screen menu.
pub fn wanted(pip: &PipBoy) -> bool {
    pip.on_arm()
}

fn announce(fx: &mut ScreenFx, pip: &PipBoy) {
    if pip.warming.is_none() && !fx.announced {
        fx.announced = true;
        let kept = fx.kept;
        let shown = undercoat(kept).to_srgba().to_u8_array();
        info!("pipboy: screen treatment on (scanlines {}, keeping {kept:.2} of the light; undercoat ({}, {}, {}))", if fx.scanlines.is_some() { "baked" } else { "none" }, shown[0], shown[1], shown[2]);
    }
}

/// Keep the layers: the picture under the pages and the scanlines over them, while the
/// device's frame shows. The pages are drawn between, by whichever painter draws them.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    pip: Res<PipBoy>,
    screen: Option<Res<PipBoyScreen>>,
    layers: Query<Entity, With<ScreenFxLayer>>,
    mut pictures: Pictures,
    mut beams: Beams,
    mut fx: ResMut<ScreenFx>,
    mut kit: ResMut<Kit>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    let Some(screen) = screen else { return };
    let want = wanted(&pip);
    if want == !layers.is_empty() {
        if want {
            apply_colour(&mut fx, pipboy_rgb(), &mut pictures, &mut beams);
            announce(&mut fx, &pip);
        }
        return;
    }
    if !want {
        for e in &layers {
            commands.entity(e).despawn();
        }
        if fx.announced { info!("pipboy: screen treatment off"); }
        fx.announced = false;
        return;
    }
    let mut c = Ctx::new(&mut kit, data.as_deref(), &mut images, formats.as_deref(), &glyphs);
    fx.bake(c.data, &mut *c.images);
    let (rgb, kept) = (pipboy_rgb(), fx.kept);
    fx.rgb = rgb;
    let whole = || at(0.0, 0.0, CANVAS.x, CANVAS.y);
    let picture = c.picture(BACKGROUND);
    commands.spawn((ScreenFxLayer, ScreenFxNode, whole(), UiTargetCamera(screen.camera), UNDER_PAGES, Pickable::IGNORE, Name::new("Pip-Boy screen background"))).with_children(|r| {
        let node = (ScreenFxPicture, ScreenFxNode, whole(), Pickable::IGNORE);
        match picture {
            Some((image, rect)) => r.spawn((node, ImageNode { rect, color: picture_tint(rgb, kept), image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) })),
            None => r.spawn((node, BackgroundColor(missing_picture_fill(rgb)))),
        };
    });
    if let Some(lines) = fx.scanlines.clone() {
        commands.spawn((
            ScreenFxLayer,
            ScreenFxNode,
            whole(),
            ImageNode { image_mode: NodeImageMode::Stretch, ..ImageNode::new(lines) },
            UiTargetCamera(screen.camera),
            OVER_PAGES,
            Pickable::IGNORE,
            Name::new("Pip-Boy screen scanlines"),
        ));
    }
    if let Some(beam) = fx.beam.clone() {
        commands.spawn((
            ScreenFxLayer,
            ScreenFxNode,
            ScreenFxBeam,
            at(0.0, -BEAM_HEIGHT, CANVAS.x, BEAM_HEIGHT),
            Visibility::Hidden,
            ImageNode { color: beam_tint(rgb), rect: Some(beam_window(0)), image_mode: NodeImageMode::Stretch, ..ImageNode::new(beam) },
            UiTargetCamera(screen.camera),
            BEAM,
            Pickable::IGNORE,
            Name::new("Pip-Boy screen beam"),
        ));
    }
    fx.swept = false;
    announce(&mut fx, &pip);
}

/// Move the beam: shown and travelling for [`BEAM_TRAVEL`] of every [`BEAM_PERIOD`] on the
/// real clock, hidden the rest. Under the pause menu the band stands where it is; the clock
/// runs on, so on resume it jumps to where the clock says (or hides): a wall-clock effect
/// held in the picture, not a phase kept across the pause [design]. Logged once each time
/// the treatment comes on, on the first sweep. Its picture is read from a new place every
/// frame it shows ([`beam_window`]).
#[allow(clippy::type_complexity)]
pub fn sweep(time: Res<Time<Real>>, paused: Res<crate::pause::Paused>, pip: Res<PipBoy>, mut fx: ResMut<ScreenFx>, mut frame: Local<u32>, mut beams: Query<(&mut Node, &mut Visibility, &mut ImageNode), With<ScreenFxBeam>>) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    if paused.0 {
        return;
    }
    *frame = frame.wrapping_add(1);
    let phase = time.elapsed_secs() % BEAM_PERIOD;
    for (mut node, mut visibility, mut image) in &mut beams {
        match beam_top(phase) {
            Some(top) => {
                node.top = crate::interface::u(top);
                image.rect = Some(beam_window(*frame));
                *visibility = Visibility::Inherited;
                if !fx.swept && pip.warming.is_none() {
                    fx.swept = true;
                    info!("pipboy: the beam sweeps the screen ({BEAM_TRAVEL} s, every {BEAM_PERIOD} s)");
                }
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Frame, Page, PageBody, PipBoyRoot, Phase};
    use super::*;

    /// The band of the file as pipboy.md §3 describes it: white at the top, 0.8 at the foot.
    fn band() -> Vec<f32> {
        (0..64).map(|i| 1.0 - 0.2 * i as f32 / 63.0).collect()
    }

    fn over(top: Color, under: LinearRgba) -> LinearRgba {
        let t = top.to_linear();
        LinearRgba::rgb(t.red * t.alpha + under.red * (1.0 - t.alpha), t.green * t.alpha + under.green * (1.0 - t.alpha), t.blue * t.alpha + under.blue * (1.0 - t.alpha))
    }

    /// The layers in order at one row of the column: undercoat, picture × tint, what is on
    /// top of it (a page's opaque ink, if any), the scanline's bytes.
    fn image_row(texel: f32, ink: Option<Color>, alpha_byte: u8, kept: f32) -> LinearRgba {
        let tint = picture_tint(AMBER_RGB, kept).to_linear();
        let t = linear(texel);
        let picture = Color::linear_rgba(tint.red * t, tint.green * t, tint.blue * t, tint.alpha);
        let mut c = over(picture, undercoat(kept).to_linear());
        if let Some(ink) = ink {
            c = over(ink, c);
        }
        over(Color::srgba_u8(0, 0, 0, alpha_byte), c)
    }

    #[test]
    fn the_column_has_a_row_per_canvas_row_the_pitch_and_the_depth() {
        let column = baked_column(&band_at_depth(&band(), SCANLINE_DEPTH), CANVAS.y as usize, SCANLINE_PITCH);
        assert_eq!(column.len(), 768);
        let (lo, hi) = column.iter().fold((1.0f32, 0.0f32), |(lo, hi), &k| (lo.min(k), hi.max(k)));
        // In linear light; `linear(0.85)` is a dip of 15% in the display values.
        assert!((hi - 1.0).abs() < 0.03, "{hi}");
        assert!((lo - linear(1.0 - SCANLINE_DEPTH)).abs() < 0.03, "{lo}");
        // Two lines on is 19 rows, a whole number: the same place in the band.
        for y in 0..700 {
            assert!((column[y] - column[y + 19]).abs() < 1e-3, "row {y}");
        }
    }

    #[test]
    fn a_band_is_rescaled_to_the_depth_and_a_white_one_stays() {
        let b = band_at_depth(&[1.0, 0.9, 0.8], 0.15);
        assert!((b[0] - 1.0).abs() < 1e-6 && (b[1] - 0.925).abs() < 1e-5 && (b[2] - 0.85).abs() < 1e-5, "{b:?}");
        assert_eq!(band_at_depth(&[1.0; 4], 0.15), vec![1.0; 4]);
    }

    #[test]
    fn the_empty_screen_averages_the_fit_through_every_layer() {
        // The same bytes and mean as `bake`, then the layers as `draw` and `camera_on` lay
        // them, and the material's product (`pipboy_arm.rs`): the picture's light, its
        // vignette whole, and the undercoat's, with the scanlines' mean given back.
        let column = baked_column(&band_at_depth(&band(), SCANLINE_DEPTH), CANVAS.y as usize, SCANLINE_PITCH);
        let (rgba, kept) = scanline_pixels(&column);
        assert!(kept > 0.7 && kept < 1.0, "{kept}");
        let tint_shape = Vec3::from_array(SCREEN_TINT) * SHAPE_VERTEX_COLOUR;
        for texel in [0.05, 0.3, 0.39] {
            let mut sum = Vec3::ZERO;
            for p in rgba.as_chunks::<4>().0 {
                let c = image_row(texel, None, p[3], kept);
                sum += Vec3::new(c.red, c.green, c.blue);
            }
            let scene = sum / column.len() as f32 * tint_shape;
            let want = background_light(AMBER_RGB) * linear(texel) * tint_shape + Vec3::from_array(UNDERCOAT_LIGHT);
            assert!((scene - want).abs().max_element() < 2e-5, "{scene} {want}");
        }
        // The picture carries the falloff: at its dark edge the undercoat is no more than
        // comparable, in its bright middle it is a small part.
        assert!(glass_undercoat().min_element() > 0.0, "{}", glass_undercoat());
        let middle = background_light(AMBER_RGB) * linear(76.0 / 255.0) * tint_shape;
        assert!(Vec3::from_array(UNDERCOAT_LIGHT).x < 0.2 * middle.x, "{middle}");
    }

    #[test]
    fn opaque_amber_keeps_its_value_on_the_bright_rows() {
        // The band's white rows take nothing from the ink, and the undercoat is behind it.
        let amber = crate::interface::tint(AMBER_RGB, 255.0);
        let c = image_row(0.3, Some(amber), 0, 0.8);
        let a = amber.to_linear();
        assert!((c.red - a.red).abs() < 1e-5 && (c.green - a.green).abs() < 1e-5 && (c.blue - a.blue).abs() < 1e-5);
    }

    /// Whichever colour the test says, not the global one other tests may be setting.
    #[derive(Resource)]
    struct Rgb([u8; 3]);

    #[test]
    fn a_new_pipboy_colour_retints_the_picture_and_leaves_the_undercoat() {
        let green = [26, 255, 128];
        let kept = 0.8;
        let mut app = App::new();
        app.insert_resource(Rgb(AMBER_RGB)).insert_resource(ScreenFx { kept, ..default() }).add_systems(Update, |rgb: Res<Rgb>, mut fx: ResMut<ScreenFx>, mut pictures: Pictures, mut beams: Beams| {
            apply_colour(&mut fx, rgb.0, &mut pictures, &mut beams);
        });
        let image = app.world_mut().spawn((ScreenFxPicture, ImageNode { color: picture_tint(AMBER_RGB, kept), ..default() })).id();
        let fill = app.world_mut().spawn((ScreenFxPicture, BackgroundColor(missing_picture_fill(AMBER_RGB)))).id();
        let beam = app.world_mut().spawn((ScreenFxBeam, ImageNode { color: beam_tint(AMBER_RGB), ..default() })).id();
        let under = undercoat(kept);
        // The same colour touches nothing.
        app.update();
        assert_eq!(app.world().get::<ImageNode>(image).unwrap().color, picture_tint(AMBER_RGB, kept));
        // Another: the lifted tint for it, with its alpha, the faint fill, and the beam's lighter colour.
        app.world_mut().insert_resource(Rgb(green));
        app.update();
        let c = app.world().get::<ImageNode>(image).unwrap().color;
        assert_eq!(c, picture_tint(green, kept));
        assert_ne!(c, picture_tint(AMBER_RGB, kept));
        assert_eq!(c.alpha(), BACKGROUND_ALPHA);
        assert_eq!(app.world().get::<BackgroundColor>(fill).unwrap().0, missing_picture_fill(green));
        assert_eq!(app.world().get::<ImageNode>(beam).unwrap().color, beam_tint(green));
        assert_eq!(undercoat(kept), under, "the glass keeps its colour");
    }

    #[test]
    fn the_beam_sweeps_the_canvas_in_its_time_and_hides_between() {
        assert_eq!(beam_top(0.0), Some(-BEAM_HEIGHT), "starts above the top edge");
        let mid = beam_top(BEAM_TRAVEL / 2.0).unwrap();
        assert!((mid - (CANVAS.y - BEAM_HEIGHT) / 2.0).abs() < 1e-3, "{mid}: its middle at the canvas's middle half way");
        let last = beam_top(BEAM_TRAVEL - 1e-4).unwrap();
        assert!(last > CANVAS.y - 1.0 && last <= CANVAS.y, "{last}: off the foot at the end");
        assert_eq!(beam_top(BEAM_TRAVEL), None);
        assert_eq!(beam_top(BEAM_PERIOD - 0.1), None);
        // Its shape: nothing at the top, a straight rise, a full range at the peak, and the
        // leading edge at the foot, much steeper.
        let n = BEAM_HEIGHT as usize;
        let edge = BEAM_EDGE as usize;
        let w: Vec<f32> = (0..n).map(beam_weight).collect();
        assert!(w[0] < 0.01 && w[n - 1] < 0.01, "{} {}", w[0], w[n - 1]);
        assert!(w.iter().copied().fold(0.0, f32::max) > 0.99);
        let half = (n - edge) / 2;
        assert!((w[half] - 0.5).abs() < 0.01, "half way up the rise: {}", w[half]);
        assert!(w[..n - edge].windows(2).all(|p| p[0] <= p[1]), "the rise only rises");
        assert!(w[n - edge..].windows(2).all(|p| p[0] >= p[1]), "the edge only falls");
        let rise = 1.0 / (n - edge) as f32;
        assert!(1.0 / edge as f32 > 2.5 * rise, "the leading edge is the hard side");
        // Its picture: that shape in the alpha, on average along each row, with a dither of
        // up to about a display level on it that never clips, different from texel to texel.
        let px = beam_pixels();
        assert_eq!(px.len(), n * BEAM_COLUMNS * 4);
        assert!(px.chunks(4).all(|p| p[..3] == [255, 255, 255]), "white, the tint on the node");
        let level = 255.0 / BEAM_LEVELS;
        for y in 0..n {
            let row: Vec<f32> = px[y * BEAM_COLUMNS * 4..(y + 1) * BEAM_COLUMNS * 4].chunks(4).map(|p| f32::from(p[3])).collect();
            let mean = row.iter().sum::<f32>() / row.len() as f32;
            assert!((mean - w[y] * 255.0).abs() < 1.0, "row {y}: {mean} against {}", w[y] * 255.0);
            let v = w[y] * 255.0;
            let reach = level.min(v).min(255.0 - v);
            assert!(row.iter().all(|a| (a - v).abs() <= reach + 0.5), "row {y}");
        }
        let mid = half * BEAM_COLUMNS * 4;
        let alphas: std::collections::HashSet<u8> = px[mid..mid + BEAM_COLUMNS * 4].chunks(4).map(|p| p[3]).collect();
        assert!(alphas.len() > 20, "a mid row holds many levels: {}", alphas.len());
        // A canvas's width of it, from a new place on most frames, inside the picture.
        let windows: Vec<Rect> = (0..20).map(beam_window).collect();
        assert!(windows.iter().all(|r| r.min.x >= 0.0 && r.max.x <= BEAM_COLUMNS as f32 && r.width() == CANVAS.x && r.height() == BEAM_HEIGHT));
        let starts: std::collections::HashSet<u32> = windows.iter().map(|r| r.min.x as u32).collect();
        assert!(starts.len() > 15, "{starts:?}");
        // Its colour: amber lightened a little, its strength in the alpha.
        let t = beam_tint(AMBER_RGB).to_srgba();
        assert!(t.red > 0.99 && t.green > f32::from(AMBER_RGB[1]) / 255.0 && t.blue > f32::from(AMBER_RGB[2]) / 255.0);
        assert!(t.green < 0.85 && t.blue < 0.55, "not near white: {t:?}");
        assert_eq!(t.alpha, BEAM_ALPHA);
    }

    fn app(frame: Frame) -> App {
        let mut app = App::new();
        app.init_resource::<PipBoy>()
            .init_resource::<Kit>()
            .init_resource::<ScreenFx>()
            .init_resource::<Assets<Image>>()
            .init_resource::<crate::pad::PadGlyphs>()
            .insert_resource(PipBoyScreen { image: Handle::default(), camera: Entity::PLACEHOLDER, on_model: true, pointer: None, scale: 1.0 })
            .add_systems(Update, (super::super::frame::draw, draw).chain());
        let mut pip = app.world_mut().resource_mut::<PipBoy>();
        pip.phase = Phase::Up;
        pip.frame = frame;
        app
    }

    fn layers(app: &mut App) -> Vec<Entity> {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<ScreenFxLayer>>().iter(world).collect()
    }

    #[test]
    fn the_layers_are_made_once_and_outlive_the_pages() {
        let mut app = app(Frame::Device);
        app.update();
        let first = layers(&mut app);
        assert_eq!(first.len(), 2, "the background and the beam; no scanlines without the game's data");
        let world = app.world_mut();
        let beams: Vec<(Visibility, GlobalZIndex)> = world.query_filtered::<(&Visibility, &GlobalZIndex), With<ScreenFxBeam>>().iter(world).map(|(v, z)| (*v, *z)).collect();
        assert_eq!(beams, vec![(Visibility::Hidden, BEAM)], "the beam waits hidden");
        let z = beams[0].1 .0;
        assert!(0 < z && z < OVER_PAGES.0, "over the pages (0), under the scanlines: {z}");
        // A change of page redraws the page, not the treatment, and only one page root is left.
        app.world_mut().resource_mut::<PipBoy>().page = Page::Stats;
        app.update();
        app.update();
        assert_eq!(layers(&mut app), first);
        let world = app.world_mut();
        assert_eq!(world.query_filtered::<Entity, With<PipBoyRoot>>().iter(world).count(), 1);
        assert_eq!(world.query::<&PageBody>().iter(world).count(), 1);
        // Down: gone, and back when it comes up again.
        app.world_mut().resource_mut::<PipBoy>().phase = Phase::Down;
        app.update();
        app.update();
        assert!(layers(&mut app).is_empty());
        app.world_mut().resource_mut::<PipBoy>().phase = Phase::Up;
        app.update();
        app.update();
        assert_eq!(layers(&mut app).len(), 2);
    }

    #[test]
    fn the_screen_menu_has_no_treatment() {
        let mut app = app(Frame::Screen);
        app.update();
        app.update();
        assert!(layers(&mut app).is_empty(), "flat: no background, no scanlines");
        assert!(!wanted(&PipBoy { frame: Frame::Screen, phase: Phase::Up, ..default() }));
        assert!(!wanted(&PipBoy { frame: Frame::Device, phase: Phase::Down, ..default() }));
        assert!(wanted(&PipBoy { frame: Frame::Device, phase: Phase::Raising, ..default() }));
    }

    #[test]
    fn hidden_layers_are_announced_on_the_real_open_and_a_screen_only_warm_has_none() {
        let mut app = app(Frame::Device);
        app.world_mut().resource_mut::<PipBoy>().warming = Some(super::super::Warming {
            since: std::time::Instant::now(), frames: 0, asked_at: 0,
            covered: false, device: true, saved_frame: Frame::Device, saved_page: None, drawn: false,
        });
        app.update();
        let hidden = layers(&mut app);
        assert!(!hidden.is_empty());
        assert!(!app.world().resource::<ScreenFx>().announced);
        app.world_mut().resource_mut::<PipBoy>().warming = None;
        app.update();
        assert_eq!(layers(&mut app), hidden);
        assert!(app.world().resource::<ScreenFx>().announced);
        let mut pip = app.world_mut().resource_mut::<PipBoy>();
        pip.warming = Some(super::super::Warming {
            since: std::time::Instant::now(), frames: 0, asked_at: 0,
            covered: true, device: false, saved_frame: Frame::Device, saved_page: None, drawn: false,
        });
        app.update();
        assert!(layers(&mut app).is_empty());
        assert!(!app.world().resource::<ScreenFx>().announced);
    }
}
