//! What the parts of the original's interface are drawn with, shared by the modules that
//! draw one (`message_menu`, `corner_messages`, `hud`, `dialogue_menu`): lengths in the interface's units (960
//! to the height of the view), the interface's colour, the game's fonts and textures
//! read from the owner's archives at run time, a line of text set as glyphs of the
//! font's texture, and the index of the interface's texture atlas.
//!
//! Without the game's files a font is stood in for by one whose characters are all as
//! wide as each other, and its text is drawn with Bevy's font.

use bevy::image::{CompressedImageFormatSupport, CompressedImageFormats, ImageSampler};
use bevy::prelude::*;
use fnv_font::{Font, Glyph, Line};
use nvf_assets::GameData;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};

mod tga;

/// The interface's colour [observed: the brightest text of the owner's screenshot,
/// message-menu.md §3].
pub const AMBER: [f32; 3] = [1.0, 182.0 / 255.0, 66.0 / 255.0];

/// How dark the sheet behind the menus drawn over the world in play makes it (the
/// container and barter menus, the tag-skill and trait lists): 0.75 black [design: the
/// owner's wish after the playtest of 2026-10-11, amber text was hard to read over an
/// orange world; the original blurs the world behind these menus instead (not built),
/// its sheet is lighter: about 0.33 to 0.4 measured on the screenshots].
pub const MENU_BACKING: f32 = 0.75;

/// The stacking level of the character and workbench menus (tag skills, traits, name,
/// level-up, recipes, repair services): over the HUD, the corner and quest messages
/// (50), the dialogue menu and the Pip-Boy (90), the terminal and the casino games (106);
/// under the message boxes (108) these menus may ask, the tutorial (109), the pause menu
/// (110), the console (120) and the loading screen [design: the owner's playtest list 7,
/// item 2, "on top level with nothing in front"].
pub const TOP_MENU_Z: i32 = 107;

/// How dark the sheet behind those menus makes the world: the container and barter
/// menus' 0.93 (`crate::menu::look`) [design: list 7 item 2, "the backgrounds need to be
/// darker", the 0.75 of [`MENU_BACKING`] and the files' 32 of 255 let the world show
/// through the text].
pub const TOP_MENU_SHADE: f32 = 0.93;

/// The sheet those menus spread over the whole screen, on their root node: nothing of the
/// world or of the HUD's messages shows beside them either.
pub fn top_menu_backing() -> BackgroundColor {
    BackgroundColor(Color::BLACK.with_alpha(TOP_MENU_SHADE))
}

/// The menu files' shared fill: `_background_fill_alpha`, 32 of 255 [documented:
/// globals.xml]. The sheets the recipe and repair-services menus spread behind themselves
/// (`RM_Background`, `RSM_Background`) take it; at this strength their edge cannot be seen
/// over the world [observed: the owner's screenshots of both menus, 2026-10-19,
/// docs/spec-notes/repair.md §3].
pub const BACKGROUND_FILL_ALPHA: f32 = 32.0 / 255.0;

/// [`AMBER`] as 0 to 255 per channel: the default of both interface colours.
pub const AMBER_RGB: [u8; 3] = [255, 182, 66];

/// The colour the HUD and the menus are drawn in, and the one of the Pip-Boy's screens
/// (`crate::ui_colour` sets them from the settings page), packed as `0xRRGGBB`. Global,
/// not a resource: [`amber`] is called from drawing code that has no world at hand.
static HUD_RGB: AtomicU32 = AtomicU32::new(pack(AMBER_RGB));
static PIPBOY_RGB: AtomicU32 = AtomicU32::new(pack(AMBER_RGB));

#[cfg(test)]
std::thread_local! {
    static TEST_COLOURS: std::cell::Cell<Option<([u8; 3], [u8; 3])>> =
        const { std::cell::Cell::new(None) };
}

/// Isolate authored palette changes from other test apps.
/// Systems that consume the override must run on this thread.
#[cfg(test)]
pub(crate) fn colours_at(hud: [u8; 3], pipboy: [u8; 3]) -> impl Drop {
    struct Restore(Option<([u8; 3], [u8; 3])>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_COLOURS.set(self.0);
        }
    }
    Restore(TEST_COLOURS.replace(Some((hud, pipboy))))
}

const fn pack([r, g, b]: [u8; 3]) -> u32 {
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

fn unpack(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// The HUD's and the menus' colour.
pub fn hud_rgb() -> [u8; 3] {
    #[cfg(test)]
    if let Some((hud, _)) = TEST_COLOURS.get() {
        return hud;
    }
    unpack(HUD_RGB.load(Ordering::Relaxed))
}

pub fn set_hud_rgb(rgb: [u8; 3]) {
    #[cfg(test)]
    if let Some((_, pipboy)) = TEST_COLOURS.get() {
        TEST_COLOURS.set(Some((rgb, pipboy)));
        return;
    }
    HUD_RGB.store(pack(rgb), Ordering::Relaxed);
}

/// The Pip-Boy screens' colour.
pub fn pipboy_rgb() -> [u8; 3] {
    #[cfg(test)]
    if let Some((_, pipboy)) = TEST_COLOURS.get() {
        return pipboy;
    }
    unpack(PIPBOY_RGB.load(Ordering::Relaxed))
}

pub fn set_pipboy_rgb(rgb: [u8; 3]) {
    #[cfg(test)]
    if let Some((hud, _)) = TEST_COLOURS.get() {
        TEST_COLOURS.set(Some((hud, rgb)));
        return;
    }
    PIPBOY_RGB.store(pack(rgb), Ordering::Relaxed);
}

/// `rgb` at a brightness (0 to 255).
pub fn tint(rgb: [u8; 3], brightness: f32) -> Color {
    let b = brightness / 255.0;
    Color::srgb(f32::from(rgb[0]) / 255.0 * b, f32::from(rgb[1]) / 255.0 * b, f32::from(rgb[2]) / 255.0 * b)
}

/// The interface's colour (the HUD's, [`hud_rgb`]) at a brightness (0 to 255, as the
/// interface files give it).
pub fn amber(brightness: f32) -> Color {
    tint(hud_rgb(), brightness)
}

/// The Pip-Boy screens' colour ([`pipboy_rgb`]) at a brightness, as [`amber`].
pub fn pipboy_amber(brightness: f32) -> Color {
    tint(pipboy_rgb(), brightness)
}

/// The HUD scale the settings page shows and `NVForever.ini` keeps (`hud-scale-v2`),
/// as a factor (1 the original's size). Legacy `ui-scale` is migrated separately.
/// It takes effect at the next start ([`ui_scale`]).
/// Stored as the bits of an `f32`.
static UI_SCALE_SETTING: AtomicU32 = AtomicU32::new(0x3f80_0000);
/// The UI scale this run draws with, fixed once the INI is read ([`fix_ui_scale`]).
static UI_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);

/// The least and greatest UI scale [design: #1631, 50% to 150%].
pub const UI_SCALE_RANGE: (f32, f32) = (0.5, 1.5);

/// `s` brought into [`UI_SCALE_RANGE`] (1 when not a number), to the hundredth.
pub fn fit_ui_scale(s: f32) -> f32 {
    if !s.is_finite() {
        return 1.0;
    }
    (s.clamp(UI_SCALE_RANGE.0, UI_SCALE_RANGE.1) * 100.0).round() / 100.0
}

/// The settings page's UI scale choices: [`UI_SCALE_RANGE`] in steps of 5% [design].
pub const UI_SCALE_CHOICES: usize = 21;

/// The HUD Scale row's choices: 50% to 100% in steps of 10% [design: the menu overhaul,
/// owner 2026-11-15; #1631's slider went to 150% in 5% steps].
pub const UI_SCALE_LIST: [f32; 6] = [0.5, 0.6, 0.7, 0.8, 0.9, 1.0];

/// The settings page's UI scale row, `ui-scale` in `NVForever.ini` (its choice's name,
/// "80%"; an older slider's part, [`ui_scale_part`], still loads as the choice nearest it),
/// under `group` (the renderer lists it with the Display rows) [design: #1631, the menu
/// overhaul].
pub fn ui_scale_row(group: &'static str) -> (crate::settings::Setting, crate::settings::ChoiceList) {
    (
        crate::settings::Setting { key: "ui-scale", group, label: "HUD scale", note: "Size of the in-game HUD, independent of menu scale. Takes effect at the next start.", switches: "", get: |_| false, set: |_, _| {}, slider: None },
        crate::settings::ChoiceList {
            names: |_| ui_scale_names(),
            chosen: |_| ui_scale_names()[crate::settings::nearest(&UI_SCALE_LIST, ui_scale_setting())].clone(),
            pick: |_, n| take_ui_scale(n),
            save: |_| ui_scale_names()[crate::settings::nearest(&UI_SCALE_LIST, ui_scale_setting())].clone(),
            load: |_, t| take_ui_scale(t),
            ask_keep: false,
        },
    )
}

fn ui_scale_names() -> Vec<String> {
    UI_SCALE_LIST.iter().map(|s| format!("{:.0}%", s * 100.0)).collect()
}

/// The HUD scale `text` names (or an older slider's part stands for: past 100%, 100%).
fn take_ui_scale(text: &str) -> bool {
    match crate::settings::choice_of(text, &ui_scale_names(), |v| crate::settings::nearest(&UI_SCALE_LIST, ui_scale_of_part(v))) {
        Some(i) => {
            set_ui_scale_setting(UI_SCALE_LIST[i]);
            true
        }
        None => false,
    }
}

/// A UI scale as the settings slider's part (0 at 50%, 0.5 at 100%, 1 at 150%), to the
/// thousandth as `crate::settings::Slider::fit` keeps it.
pub fn ui_scale_part(s: f32) -> f32 {
    let (lo, hi) = UI_SCALE_RANGE;
    (((fit_ui_scale(s) - lo) / (hi - lo)) * 1000.0).round() / 1000.0
}

/// Expanded HUD slider has its own key so earlier 50..150% saved parts keep their size.
pub const HUD_SCALE_KEY: &str = "hud-scale-v2";
#[cfg(any(target_os = "android", test))]
const HUD_SCALE_CHOICES: usize = 46;

fn fit_hud_scale(scale: f32) -> f32 {
    if !scale.is_finite() { return 1.0; }
    (scale.clamp(0.25, 2.5) * 100.0).round() / 100.0
}
fn hud_scale_part(scale: f32) -> f32 {
    (((fit_hud_scale(scale) - 0.25) / 2.25) * 1000.0).round() / 1000.0
}
#[cfg(any(target_os = "android", test))]
fn hud_scale_of_part(part: f32) -> f32 {
    if !part.is_finite() { return 1.0; }
    fit_hud_scale(0.25 + part.clamp(0.0, 1.0) * 2.25)
}

pub fn register_hud_scale(app: &mut App, group: &'static str) {
    #[cfg(target_os = "android")]
    register_android_hud_scale(app, group);
    #[cfg(not(target_os = "android"))]
    {
        let (legacy, choices) = ui_scale_row(group);
        crate::settings::add_choice_list(app, legacy, choices);
        crate::settings::set_older_split(app, "ui-scale", crate::menu_scale::legacy_menu);
    }
}

#[cfg(any(target_os = "android", test))]
pub(crate) fn register_android_hud_scale(app: &mut App, group: &'static str) {
    use crate::settings::{self, Setting, Slider};
    let (legacy, choices) = ui_scale_row(group);
    settings::add_choice_list(app, legacy, choices);
    // The wider HUD control represents the retained legacy key on the page.
    settings::set_shown_when(app, "ui-scale", HUD_SCALE_KEY, |_| false);
    settings::add_setting(app, Setting::slider(HUD_SCALE_KEY, group, "HUD scale",
        "Size of the in-game HUD, from 25% to 250%. Menus have separate sizes. Takes effect at the next start.",
        Slider::choice(HUD_SCALE_CHOICES, |_| hud_scale_part(ui_scale_setting()),
            |_, part| UI_SCALE_SETTING.store(hud_scale_of_part(part).to_bits(), Ordering::Relaxed),
            |part| format!("{:.0}%", hud_scale_of_part(part) * 100.0))));
    settings::set_older_key(app, HUD_SCALE_KEY, "ui-scale");
    settings::set_older_split(app, "ui-scale", crate::menu_scale::legacy_menu);
}

pub(crate) fn legacy_hud_scale(value: f32) -> String {
    format!("{:.3}", hud_scale_part(ui_scale_of_part(value)))
}

/// The UI scale a slider's part stands for ([`ui_scale_part`]'s inverse).
pub fn ui_scale_of_part(part: f32) -> f32 {
    let (lo, hi) = UI_SCALE_RANGE;
    let part = if part.is_finite() { part.clamp(0.0, 1.0) } else { 0.5 };
    fit_ui_scale(lo + part * (hi - lo))
}

/// The UI scale the settings page shows (what the next start draws with).
pub fn ui_scale_setting() -> f32 {
    f32::from_bits(UI_SCALE_SETTING.load(Ordering::Relaxed))
}

pub fn set_ui_scale_setting(s: f32) {
    UI_SCALE_SETTING.store(fit_ui_scale(s).to_bits(), Ordering::Relaxed);
}

/// The UI scale this run's HUD is drawn at [design: #1631]: an NVForever setting, not the
/// original's (its view is 960 units high at every size [observed: hud.md §14]).
pub fn ui_scale() -> f32 {
    f32::from_bits(UI_SCALE.load(Ordering::Relaxed))
}

/// The independent menu scale this run draws at (#2519), at most the original's size.
/// A menu is laid out for the whole view (its buttons and columns reach close to the
/// view's edges, the pause menu's Back row 882 of 960 units down), so drawn larger in a
/// frame kept to the middle it would run off the screen; it may shrink, not grow
/// [design: #1631, retained by #2519]. HUD scale is independent.
pub fn menu_scale() -> f32 {
    crate::menu_scale::drawing()
}

/// The legacy shared-setting menu scale, used to migrate a missing menu key (#2519).
pub fn menu_scale_of(ui: f32) -> f32 {
    ui.min(1.0)
}

/// Fix the run's UI scale from the setting, once the INI is read (`PostStartup`, after
/// `crate::settings::apply_stored`): what is drawn keeps one size until the next start,
/// rather than mixing pieces drawn before and after a change [design].
pub fn fix_ui_scale() {
    let s = ui_scale_setting();
    UI_SCALE.store(s.to_bits(), Ordering::Relaxed);
    info!("interface: UI scale {:.0}% (the HUD drawn at {:.0}%, the menus at {:.0}% of the original's size)", s * 100.0, s * 100.0, menu_scale() * 100.0);
}

std::thread_local! {
    /// The scale [`u`] applies on this thread when a scope set one ([`units_at`]); `None`
    /// is the menus' scale ([`menu_scale`]).
    static UNIT_SCALE: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// While alive, [`u`] (and so [`at`], the text and picture helpers) on this thread draw
/// at the scale it was made with; dropped, they go back to what they drew at before.
/// Outside every scope they draw at the independent menus' scale ([`menu_scale`], at
/// most 100%) [design: #2519]; the HUD's systems take [`hud_units`]. The screens drawn into a texture (the Pip-Boy's pages,
/// the terminal's) take [`original_units`]: their texture keeps its size, the picture of
/// it on the window is what scales.
#[must_use]
pub struct HudUnits(Option<f32>);

impl Drop for HudUnits {
    fn drop(&mut self) {
        UNIT_SCALE.with(|c| c.set(self.0));
    }
}

/// Draw at the UI scale until the returned guard is dropped ([`HudUnits`]). The HUD's
/// systems take one (#1766); outside any scope the menus draw at [`menu_scale`].
pub fn hud_units() -> HudUnits {
    units_at(ui_scale())
}

/// Draw at the original's size until the returned guard is dropped: for nodes laid out
/// in a texture of their own (a canvas camera's target), whose `Vh` is the texture's
/// height, not the window's [design: #1631, the picture of the canvas scales instead],
/// and for what fills the view rather than being a menu (the credits' roll).
pub fn original_units() -> HudUnits {
    units_at(1.0)
}

/// [`hud_units`] at a given scale.
pub fn units_at(scale: f32) -> HudUnits {
    HudUnits(UNIT_SCALE.with(|c| c.replace(Some(scale))))
}

/// The scale [`u`] applies here and now: a scope's ([`units_at`]), else the menus'
/// ([`menu_scale`]).
pub fn unit_scale() -> f32 {
    UNIT_SCALE.with(|c| c.get()).unwrap_or_else(menu_scale)
}

/// A length of the interface (960 units high) as a part of the view's height, times the
/// UI scale ([`unit_scale`]; at 100% `units / 9.6` exactly).
pub fn u(units: f32) -> Val {
    Val::Vh(units * unit_scale() / 9.6)
}

/// A font size in interface units, as [`u`].
pub fn font_units(units: f32) -> FontSize {
    FontSize::Vh(units * unit_scale() / 9.6)
}

/// Where a frame 960 units high (the original's view) stands from the view's top for its
/// pieces to keep their distance from the view's top (`anchor` 0), middle (0.5) or bottom
/// (1) at the scale [`u`] applies: `anchor × (100% − 960 units)`, 0 at the original's
/// size [design: #1631, the pieces keep to their edges].
pub fn frame_top(anchor: f32) -> Val {
    Val::Vh(anchor * 100.0 * (1.0 - unit_scale()))
}

/// Where the middle of the view is kept by a menu drawn at the UI scale ([`menu_frame`]).
pub const MIDDLE: Vec2 = Vec2::new(0.5, 0.5);

/// A node's top `units` down from the top of a view-sized [`menu_frame`] kept at
/// `anchor_y` (0 the view's top, 0.5 its middle, 1 its bottom), for a menu placed in its
/// root rather than in a frame: `u(units)` at the original's size, else the frame's top
/// ([`frame_top`]) and the scaled length together [design: #1631].
pub fn from_top(units: f32, anchor_y: f32) -> Val {
    let s = unit_scale();
    if s == 1.0 {
        return u(units);
    }
    Val::Vh(anchor_y * 100.0 * (1.0 - s) + units * s / 9.6)
}

/// Where a menu that stands at the view's bottom keeps its place at the UI scale: the
/// middle of the bottom edge (the dialogue menu) [design: #1631].
pub const BOTTOM_MIDDLE: Vec2 = Vec2::new(0.5, 1.0);

/// The node a hand-built menu's pieces are spawned in at the UI scale, or `None` at the
/// original's size (the pieces then go straight into the menu's root, as before).
///
/// A menu places its pieces from its root's top left in units (`at`, `top:
/// u((960 - h) / 2)`) and from its root's middle or edges in parts of it
/// (`Val::Percent`). Scaled, the root's pieces go into this node instead: the view's
/// size times the scale, standing so that the point `anchor` (0 to 1 of the view's width
/// and height; [`MIDDLE`] for the menus) keeps its place. Inside it, a length in units
/// is the scale times its original's, and a part of the frame the same part of a view
/// that size, so the whole menu is drawn as on a view the scale times the window's,
/// centred on it [design: #1631]. The menu's own sheet over the world and any picture
/// filling the view stay on the root, at the window's size.
pub fn menu_frame(anchor: Vec2) -> Option<Node> {
    let s = unit_scale();
    (s != 1.0).then(|| Node {
        position_type: PositionType::Absolute,
        left: Val::Percent(anchor.x * 100.0 * (1.0 - s)),
        top: Val::Percent(anchor.y * 100.0 * (1.0 - s)),
        width: Val::Percent(100.0 * s),
        height: Val::Percent(100.0 * s),
        ..default()
    })
}

/// How much of the room a smaller menu scale leaves the pause menu takes back, each way:
/// its frame is `s × (1 + MENU_SPREAD × (1 − s))` of the view at menu scale `s` (25% more
/// room at 50%, none at 100%) [design: owner 2026-11-15, the menu overhaul: "when the hud is
/// scaled down at 50% ... extend the vertical/horizontal space by 25%"]. Only the pause
/// menu's pages lay themselves out for it ([`menu_room`]); the other menus keep
/// [`menu_frame`].
pub const MENU_SPREAD: f32 = 0.5;

std::thread_local! {
    /// A scope asked for the whole view ([`whole_view`]).
    static WHOLE_VIEW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// While alive, a spread frame on this thread takes the whole view whatever the scale
/// ([`spread_share`] 1), so a page drawn smaller to fit a narrow view gets all of its room
/// (the settings page in a portrait view, #3334); dropped, it goes back.
#[must_use]
pub struct WholeView(bool);

impl Drop for WholeView {
    fn drop(&mut self) {
        WHOLE_VIEW.with(|c| c.set(self.0));
    }
}

/// [`WholeView`] from now until it is dropped.
pub fn whole_view() -> WholeView {
    WholeView(WHOLE_VIEW.with(|c| c.replace(true)))
}

/// The share of the view a spread frame takes (module notes of [`MENU_SPREAD`]); all of it
/// inside a [`whole_view`] scope.
fn spread_share() -> f32 {
    if WHOLE_VIEW.with(|c| c.get()) {
        return 1.0;
    }
    let s = unit_scale();
    (s * (1.0 + MENU_SPREAD * (1.0 - s))).min(1.0)
}

/// How many times the original's 960 units each way a spread frame holds at the scale
/// [`u`] applies (1 at 100%, 1.25 at 50%).
pub fn menu_room() -> f32 {
    spread_share() / unit_scale()
}

/// [`menu_frame`] for a page that spreads into the room a smaller scale leaves
/// ([`MENU_SPREAD`]).
pub fn spread_frame(anchor: Vec2) -> Option<Node> {
    let f = spread_share();
    (f != 1.0).then(|| Node {
        position_type: PositionType::Absolute,
        left: Val::Percent(anchor.x * 100.0 * (1.0 - f)),
        top: Val::Percent(anchor.y * 100.0 * (1.0 - f)),
        width: Val::Percent(100.0 * f),
        height: Val::Percent(100.0 * f),
        ..default()
    })
}

/// [`in_menu_frame`] in a [`spread_frame`].
pub fn in_spread_frame(p: &mut ChildSpawnerCommands, anchor: Vec2, content: impl FnOnce(&mut ChildSpawnerCommands)) {
    match spread_frame(anchor) {
        Some(frame) => {
            p.spawn((frame, Pickable::IGNORE)).with_children(content);
        }
        None => content(p),
    }
}

/// [`over_view`] for a piece of a [`spread_frame`].
pub fn over_spread_view(node: Node, anchor: Vec2, across: bool) -> Node {
    let f = spread_share();
    if f == 1.0 {
        return node;
    }
    let mut node = Node { top: Val::Percent(-anchor.y * 100.0 * (1.0 - f) / f), height: Val::Percent(100.0 / f), bottom: Val::Auto, ..node };
    if across {
        node = Node { left: Val::Percent(-anchor.x * 100.0 * (1.0 - f) / f), width: Val::Percent(100.0 / f), right: Val::Auto, margin: UiRect::default(), ..node };
    }
    node
}

/// Spawn a menu's pieces (`content`) under `p` in its [`menu_frame`] kept at `anchor`; at
/// the original's size straight under `p`.
pub fn in_menu_frame(p: &mut ChildSpawnerCommands, anchor: Vec2, content: impl FnOnce(&mut ChildSpawnerCommands)) {
    match menu_frame(anchor) {
        Some(frame) => {
            p.spawn((frame, Pickable::IGNORE)).with_children(content);
        }
        None => content(p),
    }
}

/// A menu's root node made its own [`menu_frame`] (for a root that has no sheet or
/// picture over the whole view to keep): its place and size replaced, the rest kept; the
/// node itself at the original's size.
pub fn framed_root(node: Node, anchor: Vec2) -> Node {
    match menu_frame(anchor) {
        Some(frame) => Node { left: frame.left, top: frame.top, width: frame.width, height: frame.height, right: Val::Auto, bottom: Val::Auto, ..node },
        None => node,
    }
}

/// Inside a [`menu_frame`] kept at `anchor`: `node` stretched over the whole view's height
/// (and, with `across`, its width too), not only the frame's: a sheet or band of the menu
/// that reaches the view's edges at the original's size. `node` itself at the original's
/// size.
pub fn over_view(node: Node, anchor: Vec2, across: bool) -> Node {
    let s = unit_scale();
    if s == 1.0 {
        return node;
    }
    let mut node = Node { top: Val::Percent(-anchor.y * 100.0 * (1.0 - s) / s), height: Val::Percent(100.0 / s), bottom: Val::Auto, ..node };
    if across {
        node = Node { left: Val::Percent(-anchor.x * 100.0 * (1.0 - s) / s), width: Val::Percent(100.0 / s), right: Val::Auto, margin: UiRect::default(), ..node };
    }
    node
}

/// A point of the window (from its top left, in pixels of any kind, `view` the window's
/// size in the same pixels) where a menu drawn at the UI scale in a [`menu_frame`] kept at
/// `anchor` would have it at the original's size: what a menu's own pointer code reads,
/// so it hits what is drawn. The point itself at the original's size.
pub fn from_menu_frame(point: Vec2, view: Vec2, anchor: Vec2) -> Vec2 {
    let s = unit_scale();
    if s == 1.0 {
        return point;
    }
    (point - anchor * view * (1.0 - s)) / s
}

/// [`from_menu_frame`]'s inverse: where a point of a menu at the original's size is drawn
/// at the UI scale.
pub fn to_menu_frame(point: Vec2, view: Vec2, anchor: Vec2) -> Vec2 {
    let s = unit_scale();
    if s == 1.0 {
        return point;
    }
    anchor * view * (1.0 - s) + point * s
}

/// [design: docs/design/android.md] Effective draw scale and safe-area padding for a
/// centred fixed-dimension menu box (`content_w × content_h` interface units) when
/// `menu_scale > 1.0` (Android up to 200%) or platform safe-area insets are non-zero.
/// At `menu_scale <= 1.0` with zero safe insets (all desktop runs), returns `None` so
/// callers retain their exact desktop layout untouched.
pub fn fit_centered_box(
    menu_scale: f32,
    view_px: Vec2,
    safe: crate::touch_safe_area::SafeInsets,
    content_w: f32,
    content_h: f32,
) -> Option<(f32, UiRect)> {
    let scale = if menu_scale.is_finite() && menu_scale > 0.0 { menu_scale } else { 1.0 };
    let view_w = if view_px.x.is_finite() && view_px.x > 1.0 { view_px.x } else { 1280.0 };
    let view_h = if view_px.y.is_finite() && view_px.y > 1.0 { view_px.y } else { 720.0 };
    let clean = |v: f32, cap: f32| if v.is_finite() { v.clamp(0.0, cap) } else { 0.0 };
    let (l, r) = (clean(safe.left, view_w * 0.45), clean(safe.right, view_w * 0.45));
    let (t, b) = (clean(safe.top, view_h * 0.45), clean(safe.bottom, view_h * 0.45));
    let has_safe = l > 0.5 || t > 0.5 || r > 0.5 || b > 0.5;
    if scale <= 1.0 && !has_safe {
        return None;
    }
    let px_per_u1 = view_h / 960.0;
    let safe_w_u1 = (view_w - l - r).max(1.0) / px_per_u1;
    let safe_h_u1 = (view_h - t - b).max(1.0) / px_per_u1;
    let max_scale = (safe_w_u1 / content_w.max(1.0)).min(safe_h_u1 / content_h.max(1.0));
    let draw_scale = scale.min(max_scale).max(0.1);
    let pad = UiRect { left: Val::Px(l), right: Val::Px(r), top: Val::Px(t), bottom: Val::Px(b) };
    Some((draw_scale, pad))
}

pub fn at(left: f32, top: f32, width: f32, height: f32) -> Node {
    Node { position_type: PositionType::Absolute, left: u(left), top: u(top), width: u(width), height: u(height), ..default() }
}

/// [design: fonts.md §3.2, #1221] A zero-size anchor at a text line's baseline. Its
/// layout rounds to a physical pixel, then its 0.25-pixel sampling phase places the
/// fractional glyph children consistently.
pub fn line_anchor(base: f32) -> (Node, LayoutConfig, UiTransform) {
    (
        at(0.0, base, 0.0, 0.0),
        LayoutConfig { use_rounding: true },
        UiTransform::from_translation(Val2::px(0.0, 0.25)),
    )
}

/// Whether the device can sample the block-compressed textures the game ships.
pub fn bc_supported(formats: Option<&CompressedImageFormatSupport>) -> bool {
    formats.is_none_or(|f| f.0.contains(CompressedImageFormats::BC))
}

/// A line of text: where it starts and the line its letters stand on.
#[derive(Debug, Clone, PartialEq)]
pub struct Set {
    pub line: Line,
    pub x: f32,
    pub base: f32,
}

/// A font and its texture (the picture and its size in texels); without the texture the
/// text is drawn with Bevy's font; with `vector`, with a vector font at the screen's own
/// resolution (`crate::vector_font`), measured by `font` as ever.
#[derive(Clone)]
pub struct Face {
    pub font: Font,
    pub image: Option<(Handle<Image>, Vec2)>,
    pub vector: Option<VectorFace>,
}

/// A vector font for a [`Face`] (`crate::vector_font`): the font, its size (the em), its
/// weight and where its baseline lies below the top of the text Bevy lays out, in interface
/// units.
#[derive(Clone, Debug)]
pub struct VectorFace {
    pub font: Handle<bevy::text::Font>,
    pub em: f32,
    pub weight: u16,
    pub base: f32,
}

/// **The vector letters' dark glow** [design: owner 2026-11-15, second test: "the vanilla font
/// has a slight dark glow effect with a drop shadow effect behind it so it is more visible on
/// the background"; the game's fonts have it in their pictures, observed]: under each line two
/// rings of eight dark copies each ([`GLOW_RINGS`]: so far off it as a part of the em, so dark;
/// four copies a ring, each with a shadow on its other side), the outer one fainter so the
/// glow fades out, and a drop shadow down and right ([`DROP`]), their darkness a part of the
/// words' alpha. Third test: "blur the text drop shadow glow a little bit more and decrease
/// its opacity by 10%": two rings for the one (0.07 of the em, 0.24 a copy), about a tenth less
/// dark where they meet the letters, and the drop shadow a tenth lighter.
pub const GLOW_RINGS: [(f32, f32); 2] = [(0.045, 0.13), (0.095, 0.08)];
pub const DROP: Vec2 = Vec2::new(0.045, 0.065);
pub const DROP_ALPHA: f32 = 0.5;

/// A dark copy of a vector line under it (its glow); recolouring passes it over.
#[derive(Component)]
pub struct Halo;

/// A vector line's [`TextShadow`] offset as a part of its em; [`glow_offsets`] turns it into
/// the logical pixels the shadow takes.
#[derive(Component, Clone, Copy)]
pub struct TextGlow(pub Vec2);

/// Set each glowing line's shadow offset from its laid-out height (one em, its line height
/// being the font's size). After the UI's layout.
pub fn glow_offsets(mut lines: Query<(&TextGlow, &bevy::ui::ComputedNode, &mut bevy::ui::widget::TextShadow), Changed<bevy::ui::ComputedNode>>) {
    for (g, node, mut shadow) in &mut lines {
        let offset = g.0 * node.size().y * node.inverse_scale_factor();
        if shadow.offset != offset {
            shadow.offset = offset;
        }
    }
}

/// A game texture for the interface: not repeated, so that a stretched picture's two
/// ends do not run into each other.
pub fn texture(path: &str, data: &GameData, images: &mut Assets<Image>, bc: bool) -> Option<(Handle<Image>, Vec2)> {
    texture_read(path, data, images, bc, nvf_assets::texture::dds_image)
}

/// A font's texture: [`texture`] with its largest level only, so a glyph cut out of it
/// never takes in its neighbours from a smaller level ([`nvf_assets::texture::dds_image_top`]).
pub fn font_texture(path: &str, data: &GameData, images: &mut Assets<Image>, bc: bool) -> Option<(Handle<Image>, Vec2)> {
    texture_read(path, data, images, bc, nvf_assets::texture::dds_image_top)
}

fn texture_read(
    path: &str,
    data: &GameData,
    images: &mut Assets<Image>,
    bc: bool,
    decode: fn(&[u8], bool, bool) -> Result<Image, String>,
) -> Option<(Handle<Image>, Vec2)> {
    let bytes = data.read(path)?;
    match decode_texture(&bytes, bc, decode) {
        Ok(mut image) => {
            image.sampler = ImageSampler::linear();
            let size = image.size().as_vec2();
            Some((images.add(image), size))
        }
        Err(e) => {
            warn!("interface: {path}: {e}");
            None
        }
    }
}

fn decode_texture(bytes: &[u8], bc: bool, decode_dds: fn(&[u8], bool, bool) -> Result<Image, String>) -> Result<Image, String> {
    if bytes.starts_with(b"DDS ") || !tga::recognizable(bytes) {
        return decode_dds(bytes, true, bc);
    }
    let decoded = tga::decode(bytes)?;
    nvf_assets::texture::checked_image(decoded.width, decoded.height, 1, bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb, decoded.rgba)
}

/// A font's picture from its `.tex` file, read before the `.dds` (docs/spec-notes/fonts.md
/// §2.2): a Russian install's fonts describe the letters of their `.tex`, while the `.dds`
/// beside it is the English one [observed: #1439]; in an English install the two are the
/// same picture; modded bitmap fonts often ship `.fnt` + `.tex`/`.Tex` without a sibling
/// `.dds` [design]. None, with a warning for a broken file, when there is none.
pub fn font_tex(font: &Font, data: &GameData) -> Option<fnv_font::Tex> {
    let path = font.tex_path();
    match fnv_font::Tex::parse(&data.read(&path)?) {
        Ok(t) => Some(t),
        Err(e) => {
            if data.vfs.exists(&font.texture_path()) {
                warn!("interface: {path}: {e}; the .dds is drawn instead");
            } else {
                warn!("interface: {path}: {e}");
            }
            None
        }
    }
}

/// A `.tex` picture as an interface image, made as [`font_texture`] makes a `.dds` one.
fn tex_image(path: &str, tex: fnv_font::Tex, images: &mut Assets<Image>) -> Option<(Handle<Image>, Vec2)> {
    let format = bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;
    match nvf_assets::texture::checked_image(tex.width, tex.height, 1, format, tex.rgba) {
        Ok(mut image) => {
            image.sampler = ImageSampler::linear();
            let size = image.size().as_vec2();
            Some((images.add(image), size))
        }
        Err(e) => {
            warn!("interface: {path}: {e}");
            None
        }
    }
}

/// A stand-in font for measuring when the game's cannot be read: every character as wide
/// as the next.
pub fn plain_font(size: f32, advance: f32) -> Font {
    let glyph = Glyph { after: advance, ..default() };
    Font { size, texture: String::new(), glyphs: vec![glyph; 256], extra: Default::default() }
}

/// The install's own font files, by number (`None`: the default, [`FONT_FILES`]); set once
/// at start ([`set_font_files`]).
static INSTALL_FONTS: std::sync::OnceLock<[Option<String>; 9]> = std::sync::OnceLock::new();

/// The font files an INI's `[Fonts]` section names: `sFontFile_1` … `_9`, each as a game
/// path in small letters with backslashes; `None` where the INI names none
/// [documented: Fallout_default.ini `[Fonts]`; hypothesis: a localized install points
/// these at its own files, #1439].
pub fn fonts_in_ini(text: &str) -> [Option<String>; 9] {
    std::array::from_fn(|i| {
        let v = fnv_install::ini_get(text, "Fonts", &format!("sFontFile_{}", i + 1))?;
        let v = v.trim().trim_matches('"').replace('/', "\\").to_ascii_lowercase();
        (!v.is_empty()).then_some(v)
    })
}

/// Draw font n with the file `files[n - 1]` names where it names one, from now on (once
/// a run; a second call is ignored).
pub fn set_font_files(files: [Option<String>; 9]) {
    let _ = INSTALL_FONTS.set(files);
}

/// Configure lazy fallback for a missing CJK character.
pub fn set_cjk_font_path(path: Option<std::path::PathBuf>) {
    crate::cjk_font::configure(path);
}

/// The file a font is read from: a default font file ([`FONT_FILES`]) is the install's
/// own one for that number when its INI names one ([`set_font_files`]); any other path
/// is itself.
pub fn font_file(path: &str) -> String {
    font_file_in(path, INSTALL_FONTS.get())
}

pub(crate) fn font_file_in(path: &str, install: Option<&[Option<String>; 9]>) -> String {
    let own = install.and_then(|fonts| {
        let n = FONT_FILES.iter().position(|f| f.eq_ignore_ascii_case(path))?;
        fonts[n].clone()
    });
    own.unwrap_or_else(|| path.to_string())
}

/// Font 9 (`FONT_FILES[8]`, `NVFont_Test`) is not loaded in the original game (fonts 1-8
/// only [observed: hud_menus capture 2026-10-31, docs/spec-notes/fonts.md §3.1; sup6
/// `LOADED_FONTS = 8`; `GetFontFile 9` returning `""`]). Keep it unloaded in the shared
/// [`face`] loader so every caller (`tiles::draw::FontKit`, `credits::start`,
/// `loading_look::fit_tip`, `wait`, etc.) keeps the vanilla stand-in consistently [design].
pub(crate) fn is_unloaded_font(path: &str) -> bool {
    path.len() == FONT_FILES[8].len()
        && path.bytes().zip(FONT_FILES[8].bytes()).all(|(a, b)| {
            let a = if a == b'/' { b'\\' } else { a.to_ascii_lowercase() };
            a == b
        })
}

/// The game's font at `path` (the install's own file for it: [`font_file`]), else `stand_in`.
pub fn face(path: &str, stand_in: Font, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Face {
    if is_unloaded_font(path) {
        return Face { font: stand_in, image: None, vector: None };
    }
    let path = &font_file(path);
    if is_unloaded_font(path) {
        return Face { font: stand_in, image: None, vector: None };
    }
    let read = data.and_then(|d| {
        let font = match Font::parse(&d.read(path)?) {
            Ok(f) => f,
            Err(e) => {
                warn!("interface: {path}: {e}");
                return None;
            }
        };
        let image = font_tex(&font, d).and_then(|t| tex_image(&font.tex_path(), t, images)).or_else(|| font_texture(&font.texture_path(), d, images, bc))?;
        Some(Face { font, image: Some(image), vector: None })
    });
    read.unwrap_or_else(|| {
        warn!("interface: drawn without the game's font {path}");
        Face { font: stand_in, image: None, vector: None }
    })
}

/// The INI's fonts by number, font n at `n - 1` [documented: Fallout_default.ini
/// `sFontFile_1` … `_9`].
pub const FONT_FILES: [&str; 9] = [
    "textures\\fonts\\glow_monofonto_large.fnt",
    "textures\\fonts\\monofonto_large.fnt",
    "textures\\fonts\\glow_monofonto_medium.fnt",
    "textures\\fonts\\monofonto_verylarge02_dialogs2.fnt",
    "textures\\fonts\\fixedsys_comp_uniform_width.fnt",
    "textures\\fonts\\glow_monofonto_vl_dialogs.fnt",
    "textures\\fonts\\baked-in_monofonto_large.fnt",
    "textures\\fonts\\glow_futura_caps_large.fnt",
    "textures\\fonts\\nvfont_test.fnt",
];

/// VFS folder where mods ship extended font slot definitions (`*.txt`) for JIP LN NVSE
/// [documented: JIP LN NVSE Plugin (`https://www.nexusmods.com/newvegas/mods/58277`,
/// 50.80+, current 57.30) Nexus article "Installing extra UI fonts using JIP LN NVSE";
/// observed: MAPMO v6.4 (`https://www.nexusmods.com/newvegas/mods/74365`) public
/// `Data\NVSE\Plugins\xfonts\MAPMO.txt` asset; no third-party DLL or source code consulted].
pub const XFONTS_DIR: &str = "nvse\\plugins\\xfonts\\";

/// Parse `<slot>=<path>` entries from an `xfonts` text file into `out`.
///
/// Only extended slots `10..=255` are accepted so vanilla INI slots `1..=9` (and the
/// unloaded vanilla font `9` rule) are never overwritten by mod `xfonts` files
/// [documented: JIP LN NVSE 50.80+ `xfonts` format (`10..=31` in the 50.80 article,
/// `10..=89` on GECK Wiki `SetFontFile`); observed: MAPMO v6.4 `MAPMO.txt` maps slots
/// `21` and `22` to `textures\fonts\mapmo_hud_caps_font.fnt` and
/// `textures\fonts\mapmo_hud_caps_font2.fnt`; design: comment/quote/slash/`data\`
/// normalization and `10..=255` upper slot guard].
pub fn parse_xfonts(text: &str, out: &mut BTreeMap<u8, String>) {
    for raw in text.lines() {
        let line = raw.trim_start_matches('\u{feff}').trim();
        if line.is_empty()
            || line.starts_with(';')
            || line.starts_with('#')
            || line.starts_with("//")
        {
            continue;
        }
        let Some((lhs, rhs)) = line.split_once('=') else {
            continue;
        };
        let Ok(slot) = lhs.trim().parse::<u8>() else {
            continue;
        };
        if slot < 10 {
            continue;
        }
        let mut path = rhs
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .replace('/', "\\")
            .to_ascii_lowercase();
        while let Some(rest) = path.strip_prefix('\\') {
            path = rest.to_string();
        }
        if let Some(rest) = path.strip_prefix("data\\") {
            path = rest.to_string();
        }
        if !path.is_empty() {
            out.insert(slot, path);
        }
    }
}

/// Discover and parse all `NVSE\Plugins\xfonts\*.txt` files visible through `vfs`.
///
/// Files are processed in case-insensitive sorted path order so overrides are
/// deterministic [design]. Returns an empty map when no `xfonts` definitions exist,
/// leaving unmodded vanilla font loading completely unchanged.
pub fn load_xfonts(vfs: &fnv_vfs::Vfs) -> BTreeMap<u8, String> {
    let mut paths = vfs.paths(|p| {
        let lower = p.to_ascii_lowercase();
        lower.starts_with(XFONTS_DIR) && lower.ends_with(".txt")
    });
    paths.sort_by_key(|a| a.to_ascii_lowercase());
    let mut out = BTreeMap::new();
    for path in paths {
        if let Some(Ok(bytes)) = vfs.read(&path) {
            parse_xfonts(&String::from_utf8_lossy(&bytes), &mut out);
        }
    }
    out
}

/// Resolve a font slot number (`1..=9` vanilla or `10..=255` extended `xfonts`) to its
/// normalized VFS `.fnt` path [design]. Slot `0` and unregistered extended slots return
/// `None`.
pub fn slot_font_path(slot: u8, extra: &BTreeMap<u8, String>) -> Option<String> {
    if (1..=9).contains(&slot) {
        Some(font_file(FONT_FILES[usize::from(slot) - 1]))
    } else if slot >= 10 {
        extra.get(&slot).cloned()
    } else {
        None
    }
}

/// Where a font's letters stand from a line's top: the tallest glyph's rise [design].
pub fn line_base(face: &Face) -> f32 {
    face.font.glyphs.iter().skip(0x20).take(0x5F).filter(|g| g.drawn()).map(|g| g.ascent).fold(0.0, f32::max).max(face.font.size * 0.75)
}

/// The font's x-height: the rise of its typical lowercase letter, the band the eye centres
/// on for mixed-case text (so a button glyph next to "Yes" sits on it, not on the cap
/// height) [design]. The median of the lowercase letters, so the ascenders in the range
/// (`b`, `d`, `f`, `h`, `k`, `l`, `t`) do not pull it up to their own or the capitals'
/// rise. A font with no lowercase letters at all — its text is all capitals — centres on
/// its capitals instead ([`line_base`]).
pub fn x_height(face: &Face) -> f32 {
    let mut rises = [0.0f32; 0x1A];
    let mut n = 0;
    for g in face.font.glyphs.iter().skip(0x61).take(0x1A).filter(|g| g.drawn()) {
        rises[n] = g.ascent;
        n += 1;
    }
    if n == 0 {
        return line_base(face);
    }
    rises[..n].sort_by(f32::total_cmp);
    rises[n / 2]
}

/// Draw a line of text at `origin` (what its `x` and `base` are measured from). Every
/// node that carries the colour also gets `mark`.
pub fn set_text(p: &mut ChildSpawnerCommands, face: &Face, set: &Set, origin: Vec2, color: Color, mark: impl Bundle + Clone) {
    set_marked(p, face, &set.line, set.x - origin.x, set.base - origin.y, color, mark);
}

/// Draw a line of text that starts at `x` and stands on `base`, both from the parent's
/// top left corner.
pub fn set_line(p: &mut ChildSpawnerCommands, face: &Face, line: &Line, x: f32, base: f32, color: Color) {
    set_marked(p, face, line, x, base, color, ());
}

/// [`set_line`] with `mark` on every coloured node (each glyph, or the stand-in text).
pub fn set_marked(p: &mut ChildSpawnerCommands, face: &Face, line: &Line, x: f32, base: f32, color: Color, mark: impl Bundle + Clone) {
    set_scaled_bitmap_line(p, face, line, x, base, 1.0, color, mark);
}

type RedrawLine = Box<dyn Fn(&mut ChildSpawnerCommands) + Send + Sync>;

#[derive(Component)]
struct CjkLine(RedrawLine);

/// A CJK line retains its advances and gaps so even a static screen can replace its
/// question marks and move the following letters when the font is ready.
pub(crate) fn redraw_cjk_lines(world: &mut World) {
    use bevy::ecs::world::CommandQueue;
    let mut lines = world.query::<(Entity, &CjkLine)>();
    let mut queue = CommandQueue::default();
    let mut commands = Commands::new(&mut queue, world);
    for (entity, redraw) in lines.iter(world) {
        commands.entity(entity).despawn_related::<Children>().with_children(|p| redraw.0(p));
        if !crate::cjk_font::enabled() {
            commands.entity(entity).remove::<CjkLine>();
        }
    }
    queue.apply(world);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn set_scaled_bitmap_line(
    p: &mut ChildSpawnerCommands,
    face: &Face,
    line: &Line,
    x: f32,
    base: f32,
    scale: f32,
    color: Color,
    mark: impl Bundle + Clone,
) {
    if !needs_cjk_line(face, line) {
        draw_marked(p, face, line, x, base, scale, color, mark);
        return;
    }
    let advance: Vec<_> = line.glyphs.iter().map(|g| face.font.glyph_of(g.ch).advance()).collect();
    let unit_scale = unit_scale();
    let mut anchor = p.spawn(at(0.0, 0.0, 0.0, 0.0));
    anchor.with_children(|p| draw_marked(p, face, line, x, base, scale, color, mark.clone()));
    let face = face.clone();
    let line = line.clone();
    anchor.insert(CjkLine(Box::new(move |p| {
        let _units = units_at(unit_scale);
        let mut line = line.clone();
        let mut delta = 0.0;
        for (glyph, &old_advance) in line.glyphs.iter_mut().zip(&advance) {
            glyph.x += delta;
            delta += face.font.glyph_of(glyph.ch).advance() - old_advance;
        }
        line.width += delta;
        draw_marked(p, &face, &line, x, base, scale, color, mark.clone());
    })));
}

pub(crate) fn needs_cjk_line(face: &Face, line: &Line) -> bool {
    crate::cjk_font::enabled() && line.glyphs.iter().any(|g| face.font.needs_cjk_fallback(g.ch))
}

#[allow(clippy::too_many_arguments)]
fn draw_marked(
    p: &mut ChildSpawnerCommands,
    face: &Face,
    line: &Line,
    x: f32,
    base: f32,
    scale: f32,
    color: Color,
    mark: impl Bundle + Clone,
) {
    if face.image.is_some() && face.vector.is_none() {
        // [observed: #1221] Snap the shared baseline once, keeping each glyph's
        // fractional texture box.
        p.spawn(line_anchor(base)).with_children(|p| {
            for placed in &line.glyphs {
                set_glyph(p, face, placed, x + placed.x * scale, 0.0, scale, color, mark.clone());
            }
        });
        return;
    }
    if !needs_cjk_line(face, line) {
        set_bevy_line(p, face, line.text_spaced(&face.font), x, base, color, mark);
        return;
    }
    // Keep Bevy's original shaping for consecutive primary glyphs. The fallback
    // images start at the same measured offsets and baseline as those text runs.
    let mut run = Line::default();
    let mut start = 0.0;
    let flush = |p: &mut ChildSpawnerCommands, run: &mut Line, start: f32| {
        if !run.glyphs.is_empty() {
            set_bevy_line(
                p,
                face,
                run.text_spaced(&face.font),
                x + start,
                base,
                color,
                mark.clone(),
            );
            run.glyphs.clear();
        }
    };
    for &placed in &line.glyphs {
        let mut native = placed;
        if face.font.needs_cjk_fallback(placed.ch) {
            if let Some((key, glyph)) = crate::cjk_font::lookup(placed.ch, face.font.size) {
                flush(p, &mut run, start);
                set_cjk_glyph(p, key, glyph, x + placed.x, base, 1.0, color, mark.clone());
                continue;
            }
            native.ch = '?';
        }
        if run.glyphs.is_empty() {
            start = placed.x;
        }
        native.x -= start;
        run.glyphs.push(native);
    }
    flush(p, &mut run, start);
}

fn set_bevy_line(
    p: &mut ChildSpawnerCommands,
    face: &Face,
    text: String,
    x: f32,
    base: f32,
    color: Color,
    mark: impl Bundle,
) {
    if let Some(v) = &face.vector {
        // The line's text in the vector font, its baseline on `base`: Bevy lays it out at the
        // screen's resolution, without kerning or ligatures so it is as wide as `face.font`
        // measured it (`crate::vector_font`), in one line.
        let font = TextFont { font_size: font_units(v.em), weight: bevy::text::FontWeight(v.weight), font_features: crate::vector_font::features(), ..TextFont::default().with_font(v.font.clone()) };
        let layout = (bevy::text::LineHeight::RelativeToFont(1.0), TextLayout::new(Justify::Left, LineBreak::NoWrap));
        let shadow = |part: Vec2, alpha: f32| (TextGlow(part), bevy::ui::widget::TextShadow { offset: Vec2::ZERO, color: Color::BLACK.with_alpha(alpha) });
        let ink = color.alpha();
        p.spawn((Node { position_type: PositionType::Absolute, left: u(x), top: u(base - v.base), ..default() }, Pickable::IGNORE)).with_children(|n| {
            // The glow (module notes of `GLOW_RINGS`): each copy off one way, its shadow the
            // other way; the outer ring first, under the inner.
            for (r, alpha) in GLOW_RINGS.into_iter().rev() {
                let d = r * std::f32::consts::FRAC_1_SQRT_2;
                let dark = Color::BLACK.with_alpha(alpha * ink);
                for at in [Vec2::new(-r, 0.0), Vec2::new(0.0, -r), Vec2::new(-d, -d), Vec2::new(d, -d)] {
                    n.spawn((
                        Node { position_type: PositionType::Absolute, left: u(at.x * v.em), top: u(at.y * v.em), ..default() },
                        Text::new(text.clone()),
                        font.clone(),
                        layout,
                        TextColor(dark),
                        shadow(-2.0 * at, alpha * ink),
                        Halo,
                        Pickable::IGNORE,
                    ));
                }
            }
            n.spawn((Text::new(text), font, layout, TextColor(color), shadow(DROP, DROP_ALPHA * ink), mark));
        });
    } else {
        let size = face.font.size * 0.75;
        p.spawn(Node { position_type: PositionType::Absolute, left: u(x), top: u(base - size), ..default() }).with_children(|n| {
            n.spawn((Text::new(text), TextFont { font_size: font_units(size), ..default() }, TextColor(color), mark));
        });
    }
}

/// Draw one bitmap glyph starting at `x` and standing on `base`. Its offsets and ink
/// are in the face's units multiplied by `scale`. A missing CJK glyph can use an outline
/// atlas entry with precisely the metrics used in layout.
#[allow(clippy::too_many_arguments)]
pub fn set_glyph(
    p: &mut ChildSpawnerCommands,
    face: &Face,
    placed: &fnv_font::Placed,
    x: f32,
    base: f32,
    scale: f32,
    color: Color,
    mark: impl Bundle,
) {
    if face.font.needs_cjk_fallback(placed.ch) {
        if let Some((key, glyph)) = crate::cjk_font::lookup(placed.ch, face.font.size) {
            set_cjk_glyph(p, key, glyph, x, base, scale, color, mark);
            return;
        }
    }
    let Some((image, size)) = &face.image else {
        return;
    };
    let glyph = face.font.glyph(placed.code);
    if !glyph.drawn() {
        return;
    }
    let rect = Rect::new(
        glyph.uv[0] * size.x,
        glyph.uv[1] * size.y,
        glyph.uv[2] * size.x,
        glyph.uv[3] * size.y,
    );
    p.spawn((
        at(
            x + glyph.before * scale,
            base - glyph.ascent * scale,
            glyph.width * scale,
            glyph.height * scale,
        ),
        ImageNode {
            rect: Some(rect),
            color,
            image_mode: NodeImageMode::Stretch,
            ..ImageNode::new(image.clone())
        },
        LayoutConfig {
            use_rounding: false,
        },
        mark,
    ));
}

#[allow(clippy::too_many_arguments)]
fn set_cjk_glyph(
    p: &mut ChildSpawnerCommands,
    key: crate::cjk_font::GlyphKey,
    raster: std::sync::Arc<crate::cjk_font::Raster>,
    x: f32,
    base: f32,
    scale: f32,
    color: Color,
    mark: impl Bundle,
) {
    let g = raster.metrics;
    if !g.drawn() {
        return;
    }
    let entity = p
        .spawn((
            at(
                x + g.before * scale,
                base - g.ascent * scale,
                g.width * scale,
                g.height * scale,
            ),
            LayoutConfig {
                use_rounding: false,
            },
            mark,
        ))
        .id();
    p.commands().queue(move |world: &mut World| {
        if let Some(image) = crate::cjk_font::image_node(world, key, &raster, color) {
            if let Ok(mut entity) = world.get_entity_mut(entity) {
                entity.insert(image);
            }
        }
    });
}

/// Where a picture lies in an atlas texture: the atlas's file name and the picture's
/// left, top, width and height as parts of the atlas's width and height.
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasPiece {
    pub atlas: String,
    pub rect: [f32; 4],
}

/// Read an atlas index (`textures\interface\interfaceshared.tai`), by picture file name
/// in small letters. A line is the picture's file name, then the atlas's file name, a
/// number, a type, and after them the left and top offsets, a depth offset, and the
/// width and height, all as parts of the atlas; `#` starts a comment [documented: the
/// file's own heading; corner-messages.md §2.2]. Lines that do not read so are left out.
pub fn atlas_index(text: &str) -> HashMap<String, AtlasPiece> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, rest)) = line.split_once(char::is_whitespace) else { continue };
        let fields: Vec<&str> = rest.split(',').map(str::trim).collect();
        let num = |i: usize| fields.get(i).and_then(|f| f.parse::<f32>().ok());
        if let (Some(&atlas), Some(x), Some(y), Some(w), Some(h)) = (fields.first(), num(3), num(4), num(6), num(7)) {
            out.insert(name.to_ascii_lowercase(), AtlasPiece { atlas: atlas.to_ascii_lowercase(), rect: [x, y, w, h] });
        }
    }
    out
}

/// The interface's texture atlas index [documented: corner-messages.md §2.2].
pub const ATLAS_INDEX: &str = "textures\\interface\\interfaceshared.tai";

/// The file name an atlas index knows a picture by.
pub fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase()
}

/// A picture: its texture and the part of it that is the picture (all of it for `None`).
pub type Picture = (Handle<Image>, Option<Rect>);

/// The atlas index a tile's `texatlas` names, as a game path in small letters: the
/// files name it under the textures folder (`Interface\InterfaceShared.tai` is
/// [`ATLAS_INDEX`]) [documented: GECK wiki Texatlas, its example beside a `filename`
/// under `textures\`; the HUD's `texatlas` traits]. `None` for an empty name.
pub fn atlas_index_path(texatlas: &str) -> Option<String> {
    let t = texatlas.trim().trim_start_matches(['\\', '/']).replace('/', "\\").to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    Some(if t.starts_with("textures\\") { t } else { format!("textures\\{t}") })
}

/// Where a picture is read from: the texture's game path and, for a piece of an atlas,
/// the piece as parts of the texture's width and height (left, top, width, height).
#[derive(Debug, Clone, PartialEq)]
pub struct Located {
    pub texture: String,
    pub part: Option<[f32; 4]>,
}

/// A picture with its size in texels (the atlas piece's, or the whole texture's).
pub type SizedPicture = (Handle<Image>, Option<Rect>, Vec2);

/// The interface's pictures as its files name them, read when first asked for.
#[derive(Default)]
pub struct Pictures {
    atlas: HashMap<String, AtlasPiece>,
    /// Other atlas indexes a tile's `texatlas` named, by game path; empty for one that
    /// could not be read.
    indexes: HashMap<String, HashMap<String, AtlasPiece>>,
    /// Textures read so far, by path; `None` for one that could not be read.
    textures: HashMap<String, Option<(Handle<Image>, Vec2)>>,
}

impl Pictures {
    pub fn load(data: Option<&GameData>) -> Pictures {
        let atlas = data.and_then(|d| d.read(ATLAS_INDEX)).map(|b| atlas_index(&String::from_utf8_lossy(&b))).unwrap_or_default();
        Pictures { atlas, ..default() }
    }

    /// Pictures with this shared atlas index and no files to read (tests).
    pub fn with_atlas(atlas: HashMap<String, AtlasPiece>) -> Pictures {
        Pictures { atlas, ..default() }
    }

    fn texture(&mut self, path: String, data: &GameData, images: &mut Assets<Image>, bc: bool) -> Option<(Handle<Image>, Vec2)> {
        self.textures.entry(path).or_insert_with_key(|p| texture(p, data, images, bc)).clone()
    }

    /// Where a picture named as the interface names it (under the textures folder) is:
    /// its piece of the atlas `texatlas` names when that index has its file name (the
    /// index read once through `read` and kept), else its piece of the shared atlas, else
    /// its own file. The atlas texture is the index's line's file, in the index's folder
    /// [documented: interfaceshared.tai's heading, "InterfaceShared<idx>.dds"; hypothesis:
    /// another index's atlas beside it, as the shared one's]. Falling back to the shared
    /// atlas and the own file when the named index lacks the picture is [design].
    pub fn locate(&mut self, path: &str, texatlas: Option<&str>, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Located {
        let name = file_name(path);
        if let Some(index) = texatlas.and_then(atlas_index_path).filter(|p| p != ATLAS_INDEX) {
            let pieces = self.indexes.entry(index.clone()).or_insert_with(|| read(&index).map(|b| atlas_index(&String::from_utf8_lossy(&b))).unwrap_or_default());
            if let Some(piece) = pieces.get(&name) {
                let folder = index.rfind('\\').map_or("", |i| &index[..=i]);
                return Located { texture: format!("{folder}{}", piece.atlas), part: Some(piece.rect) };
            }
        }
        if let Some(piece) = self.atlas.get(&name) {
            return Located { texture: format!("textures\\interface\\{}", piece.atlas), part: Some(piece.rect) };
        }
        Located { texture: own_texture(path), part: None }
    }

    /// A picture named as the interface names it (under the textures folder): its
    /// piece of the atlas when the index has its file name, else its own file.
    pub fn picture(&mut self, path: &str, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Option<Picture> {
        self.picture_sized(path, None, data, images, bc).map(|(image, rect, _)| (image, rect))
    }

    /// [`Pictures::picture`] in the atlas `texatlas` names ([`Pictures::locate`]), with
    /// its size in texels.
    pub fn picture_sized(&mut self, path: &str, texatlas: Option<&str>, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Option<SizedPicture> {
        let data = data?;
        let at = self.locate(path, texatlas, &|p| data.read(p));
        if let Some([x, y, w, h]) = at.part {
            if let Some((image, size)) = self.texture(at.texture, data, images, bc) {
                let rect = Rect::new(x * size.x, y * size.y, (x + w) * size.x, (y + h) * size.y);
                return Some((image, Some(rect), rect.size()));
            }
        }
        self.texture(own_texture(path), data, images, bc).map(|(image, size)| (image, None, size))
    }

    /// A picture read from its own file only, with its size in texels: no atlas looked in
    /// (a whole menu's image without `texatlas`, `crate::tiles::draw::Source::Own`).
    pub fn own_sized(&mut self, path: &str, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Option<SizedPicture> {
        self.texture(own_texture(path), data?, images, bc).map(|(image, size)| (image, None, size))
    }

    /// A texture by the full game path a model names it by (`textures\…`), read once and
    /// kept, with its size in texels: the tile HUD's `nif` tiles' pieces
    /// (`crate::tiles::model`).
    pub fn file(&mut self, path: &str, data: &GameData, images: &mut Assets<Image>, bc: bool) -> Option<(Handle<Image>, Vec2)> {
        self.texture(path.trim().to_ascii_lowercase(), data, images, bc)
    }

    /// The game path of the texture a picture was read from: its atlas's when
    /// [`Pictures::picture_sized`] found it a `piece` (`texatlas` as given there), else
    /// its own file's ([`Pictures::own_sized`], or the atlas's fallback).
    pub fn texture_path(&mut self, path: &str, texatlas: Option<&str>, piece: bool, data: &GameData) -> String {
        if piece {
            self.locate(path, texatlas, &|p| data.read(p)).texture
        } else {
            own_texture(path)
        }
    }
}

/// A picture's own file: its path under the textures folder, in small letters.
fn own_texture(path: &str) -> String {
    format!("textures\\{}", path.trim().trim_start_matches(['\\', '/']).to_ascii_lowercase())
}

/// `face` drawn `k` times its size: every glyph's box and spacing scaled (the game's
/// fonts come in few sizes).
pub fn scaled_face(face: &Face, k: f32) -> Face {
    let mut font = face.font.clone();
    font.size *= k;
    for g in &mut font.glyphs {
        (g.width, g.height, g.before, g.after, g.ascent) = (g.width * k, g.height * k, g.before * k, g.after * k, g.ascent * k);
    }
    let vector = face.vector.clone().map(|v| VectorFace { em: v.em * k, base: v.base * k, ..v });
    Face { font, image: face.image.clone(), vector }
}

#[cfg(test)]
#[path = "interface_layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
#[path = "interface/bitmap_text_tests.rs"]
mod bitmap_text_tests;

#[cfg(test)]
#[path = "interface/font_tex_tests.rs"]
pub(crate) mod font_tex_tests;

#[cfg(test)]
#[path = "interface/tga_consumer_tests.rs"]
mod tga_consumer_tests;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn authored_rgba_dds(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0; 128 + width as usize * height as usize * 4];
        bytes[..4].copy_from_slice(b"DDS ");
        for (at, value) in [(4, 124), (8, 0x100f), (12, height), (16, width),
            (76, 32), (80, 0x41), (88, 32), (92, 0xff0000), (96, 0xff00),
            (100, 0xff), (104, 0xff000000), (108, 0x1000)] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn an_atlas_index_is_read() {
        let text = "# Heading\n#\nIcon_A.dds\t\tShared0.dds, 0, 2D, 0.25, 0.5, 0.0, 0.125, 0.0625\n\nnot a line\nb.dds Shared0.dds, 0, 2D, x, 0, 0, 1, 1\n";
        let index = atlas_index(text);
        assert_eq!(index.len(), 1);
        assert_eq!(index["icon_a.dds"], AtlasPiece { atlas: "shared0.dds".into(), rect: [0.25, 0.5, 0.125, 0.0625] });
    }

    #[test]
    fn a_pictures_file_name_is_what_the_atlas_knows_it_by() {
        assert_eq!(file_name("Interface\\Icons\\Message Icons\\Glow_Message_Vaultboy_In_Pain.dds"), "glow_message_vaultboy_in_pain.dds");
        assert_eq!(file_name("a.dds"), "a.dds");
    }

    #[test]
    fn the_installs_ini_names_its_fonts() {
        let ini = "[General]\nsFontFile_1=Nope.fnt\n[Fonts]\nsFontFile_1=Textures\\Fonts\\Glow_Monofonto_Large_RU.fnt\nsFontFile_7 = Textures/Fonts/Baked-in_RU.fnt\nsFontFile_9=\n";
        let fonts = fonts_in_ini(ini);
        assert_eq!(fonts[0].as_deref(), Some("textures\\fonts\\glow_monofonto_large_ru.fnt"));
        assert_eq!(fonts[6].as_deref(), Some("textures\\fonts\\baked-in_ru.fnt"));
        assert!(fonts[1].is_none() && fonts[8].is_none());
        // A default font file is read from the install's own file for its number.
        assert_eq!(font_file_in(FONT_FILES[0], Some(&fonts)), "textures\\fonts\\glow_monofonto_large_ru.fnt");
        assert_eq!(font_file_in("Textures\\Fonts\\Baked-in_Monofonto_Large.fnt", Some(&fonts)), "textures\\fonts\\baked-in_ru.fnt");
        // A font the INI does not name, another path, or no INI: the path as given.
        assert_eq!(font_file_in(FONT_FILES[1], Some(&fonts)), FONT_FILES[1]);
        assert_eq!(font_file_in("textures\\fonts\\other.fnt", Some(&fonts)), "textures\\fonts\\other.fnt");
        assert_eq!(font_file_in(FONT_FILES[0], None), FONT_FILES[0]);
    }

    /// #1631: at 100% every length is the original's `units / 9.6` of the view's height
    /// exactly; inside a HUD scope at 75% it is three quarters of that, and the menus'
    /// lengths outside the scope (or after it) are the original's again.
    #[test]
    fn the_ui_scale_shrinks_the_huds_lengths_only_inside_its_scope() {
        let vh = |v: Val| match v {
            Val::Vh(x) => x,
            other => panic!("{other:?}"),
        };
        for units in [0.0, 1.0, 13.0, 386.0, 803.0, 960.0, -480.0] {
            assert_eq!(vh(u(units)).to_bits(), (units / 9.6f32).to_bits(), "{units}");
            let _same = units_at(1.0);
            assert_eq!(vh(u(units)).to_bits(), (units / 9.6f32).to_bits(), "{units} at 100%");
        }
        assert_eq!(frame_top(1.0), Val::Vh(0.0));
        {
            let _hud = units_at(0.75);
            assert!((vh(u(960.0)) - 75.0).abs() < 1e-4);
            assert_eq!(at(10.0, 20.0, 40.0, 80.0).width, u(40.0));
            assert!((vh(at(10.0, 20.0, 40.0, 80.0).height) - 80.0 * 0.75 / 9.6).abs() < 1e-5);
            // A frame kept to the bottom stands a quarter of the view down; to the middle
            // an eighth; to the top not at all.
            assert!((vh(frame_top(1.0)) - 25.0).abs() < 1e-4);
            assert!((vh(frame_top(0.5)) - 12.5).abs() < 1e-4);
            assert_eq!(frame_top(0.0), Val::Vh(0.0));
            {
                let _inner = units_at(1.0);
                assert_eq!(vh(u(96.0)), 96.0 / 9.6);
            }
            assert!((vh(u(96.0)) - 7.5).abs() < 1e-4, "the inner scope gives the outer one back");
        }
        assert_eq!(vh(u(96.0)), 96.0 / 9.6, "out of the scope: the run's scale (100% in the tests)");
    }

    /// #1631: the slider's 21 steps are 50% to 150% by 5%, 100% in the middle; a value out
    /// of range (or not a number) is brought into it.
    #[test]
    fn the_ui_scale_slider_reaches_50_to_150_percent() {
        assert_eq!(ui_scale_of_part(0.5), 1.0);
        assert_eq!(ui_scale_part(1.0), 0.5);
        assert_eq!((ui_scale_of_part(0.0), ui_scale_of_part(1.0)), (0.5, 1.5));
        assert_eq!(ui_scale_of_part(0.25), 0.75);
        assert_eq!(ui_scale_part(0.75), 0.25);
        for i in 0..UI_SCALE_CHOICES {
            let part = crate::settings::choice_part(i, UI_SCALE_CHOICES);
            let s = ui_scale_of_part(part);
            assert!((s - (0.5 + 0.05 * i as f32)).abs() < 1e-6, "{i}: {s}");
            assert_eq!(ui_scale_part(s), part, "{i}");
        }
        assert_eq!((fit_ui_scale(0.1), fit_ui_scale(9.0), fit_ui_scale(f32::NAN), fit_ui_scale(0.876)), (0.5, 1.5, 1.0, 0.88));
        assert_eq!((ui_scale_of_part(-3.0), ui_scale_of_part(7.0), ui_scale_of_part(f32::NAN)), (0.5, 1.5, 1.0));
        // The row is a list of 50% to 100% (the menu overhaul); an older slider's part loads
        // as the nearest choice, past 100% as 100%.
        let (row, list) = ui_scale_row("Display");
        assert!(row.slider.is_none());
        let w = World::new();
        assert_eq!((list.names)(&w), ["50%", "60%", "70%", "80%", "90%", "100%"]);
        // (The setting itself is set by settings.rs' test only: one global, tests in parallel.)
        let of = |t: &str| crate::settings::choice_of(t, &ui_scale_names(), |v| crate::settings::nearest(&UI_SCALE_LIST, ui_scale_of_part(v))).map(|i| UI_SCALE_LIST[i]);
        assert_eq!(of("0.3"), Some(0.8), "0.3 was 80%");
        assert_eq!(of("1"), Some(1.0), "150%: 100%");
        assert_eq!(of("60%"), Some(0.6));
        assert_eq!(of("big"), None);
    }

    fn pct(v: Val) -> f32 {
        match v {
            Val::Percent(x) => x,
            other => panic!("{other:?}"),
        }
    }

    /// #1631: at 100% a menu is drawn exactly as before: no frame, the root untouched, the
    /// pointer read as it is, and the placing helpers the original's `u` bit for bit.
    #[test]
    fn at_100_percent_the_menus_are_placed_as_before() {
        let _s = units_at(1.0);
        assert!(menu_frame(MIDDLE).is_none() && menu_frame(BOTTOM_MIDDLE).is_none());
        let root = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() };
        assert_eq!(framed_root(root.clone(), MIDDLE), root);
        assert_eq!(over_view(root.clone(), MIDDLE, true), root);
        for units in [0.0, 13.0, 386.0, 960.0, -480.0, 0.1] {
            assert_eq!(from_top(units, 0.5), u(units));
            assert_eq!(from_top(units, 0.5), Val::Vh(units / 9.6));
            assert_eq!(font_units(units), FontSize::Vh(units / 9.6));
        }
        for p in [Vec2::ZERO, Vec2::new(640.0, 360.0), Vec2::new(1279.5, 0.25)] {
            assert_eq!(from_menu_frame(p, Vec2::new(1280.0, 720.0), MIDDLE).to_array().map(f32::to_bits), p.to_array().map(f32::to_bits));
            assert_eq!(to_menu_frame(p, Vec2::new(1280.0, 720.0), BOTTOM_MIDDLE), p);
        }
    }

    /// Where a piece `units` from a menu frame's top left is drawn, in pixels of a
    /// `view`-sized window, as the layout places it: the frame's insets (parts of the
    /// view) and then `u` (parts of the view's height).
    fn drawn_at(anchor: Vec2, units: Vec2, view: Vec2) -> Vec2 {
        let f = menu_frame(anchor).unwrap();
        let vh = |v: Val| match v {
            Val::Vh(x) => x / 100.0 * view.y,
            other => panic!("{other:?}"),
        };
        Vec2::new(pct(f.left) / 100.0 * view.x + vh(u(units.x)), pct(f.top) / 100.0 * view.y + vh(u(units.y)))
    }

    /// #1631: at 50% and 75% a menu's frame is the view's size times the scale, kept at
    /// its anchor; the pointer read through [`from_menu_frame`] lands where the piece under
    /// it would stand at 100%; [`from_top`] and [`over_view`] agree with the frame.
    #[test]
    fn a_scaled_menu_keeps_its_anchor_and_its_clicks_land_on_what_is_drawn() {
        let view = Vec2::new(1280.0, 720.0);
        let per = view.y / 960.0;
        for s in [0.5f32, 0.75] {
            let _s = units_at(s);
            let f = menu_frame(MIDDLE).unwrap();
            assert!((pct(f.width) - 100.0 * s).abs() < 1e-4 && (pct(f.height) - 100.0 * s).abs() < 1e-4, "{s}");
            assert!((pct(f.left) - 50.0 * (1.0 - s)).abs() < 1e-4 && (pct(f.top) - 50.0 * (1.0 - s)).abs() < 1e-4, "{s}");
            // The middle of the view stays the middle; a piece at the view's middle in
            // units (480 down, half the width across) is drawn there.
            let middle_units = Vec2::new(480.0 * view.x / view.y, 480.0);
            assert!((drawn_at(MIDDLE, middle_units, view) - view / 2.0).length() < 1e-3, "{s}");
            // Kept to the bottom: the bottom edge stays, the middle of it too.
            let bottom = drawn_at(BOTTOM_MIDDLE, Vec2::new(480.0 * view.x / view.y, 960.0), view);
            assert!((bottom - Vec2::new(view.x / 2.0, view.y)).length() < 1e-3, "{s}: {bottom}");
            // A button at (200, 150) units, 100 x 40: a click on its drawn middle reads its
            // 100% middle, a click just outside its drawn box reads outside its 100% box.
            for anchor in [MIDDLE, BOTTOM_MIDDLE] {
                let (corner, size) = (Vec2::new(200.0, 150.0), Vec2::new(100.0, 40.0));
                let drawn_middle = drawn_at(anchor, corner + size / 2.0, view);
                let read = from_menu_frame(drawn_middle, view, anchor) / per;
                assert!((read - (corner + size / 2.0)).length() < 1e-3, "{s} {anchor}: {read}");
                let drawn_corner = drawn_at(anchor, corner, view);
                let outside = from_menu_frame(drawn_corner - Vec2::splat(1.0), view, anchor) / per;
                assert!(outside.x < corner.x && outside.y < corner.y, "{s} {anchor}: {outside}");
                assert!((to_menu_frame(from_menu_frame(drawn_middle, view, anchor), view, anchor) - drawn_middle).length() < 1e-3);
            }
            // A root placed by `from_top` stands where the same piece of the frame does.
            let vh = |v: Val| match v {
                Val::Vh(x) => x / 100.0 * view.y,
                other => panic!("{other:?}"),
            };
            assert!((vh(from_top(300.0, 0.5)) - drawn_at(MIDDLE, Vec2::new(0.0, 300.0), view).y).abs() < 1e-3, "{s}");
            // A sheet inside the frame reaches the view's edges again.
            let sheet = over_view(Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, MIDDLE, true);
            let frame_px = view * s;
            let frame_at = view * (1.0 - s) / 2.0;
            assert!((frame_at.y + pct(sheet.top) / 100.0 * frame_px.y).abs() < 1e-3 && (pct(sheet.height) / 100.0 * frame_px.y - view.y).abs() < 1e-3, "{s}");
            assert!((frame_at.x + pct(sheet.left) / 100.0 * frame_px.x).abs() < 1e-3 && (pct(sheet.width) / 100.0 * frame_px.x - view.x).abs() < 1e-3, "{s}");
        }
    }

    /// #1631: above 100% the menus stay at the original's size (only the HUD grows), so
    /// a menu's frame is never larger than the view and the pieces at its edges stay on
    /// it: the pause menu's Back row (882 units down) and its right column (50 units from
    /// the right edge, 375 wide). Below 100% the menus follow the UI scale.
    #[test]
    fn above_100_percent_a_menus_frame_is_no_larger_than_the_view() {
        let view = Vec2::new(1280.0, 720.0);
        let units_px = |units: f32| units * unit_scale() * view.y / 960.0;
        for ui in [1.05f32, 1.25, 1.5] {
            let s = menu_scale_of(ui);
            assert_eq!(s, 1.0, "{ui}");
            let _s = units_at(s);
            if let Some(f) = menu_frame(MIDDLE) {
                assert!(pct(f.width) <= 100.0 && pct(f.height) <= 100.0 && pct(f.left) >= 0.0 && pct(f.top) >= 0.0, "{ui}: {f:?}");
            }
            // The Back row's top and the right column's right edge, in pixels of the view.
            assert!(units_px(882.0) < view.y, "{ui}");
            let width_units = 960.0 * view.x / view.y;
            assert!(units_px(width_units - 50.0) <= view.x, "{ui}");
        }
        for ui in [0.5f32, 0.75, 0.95, 1.0] {
            assert_eq!(menu_scale_of(ui), ui);
            let _s = units_at(menu_scale_of(ui));
            match menu_frame(MIDDLE) {
                Some(f) => assert!(pct(f.width) <= 100.0 && pct(f.height) <= 100.0 && pct(f.left) >= 0.0 && pct(f.top) >= 0.0, "{ui}: {f:?}"),
                None => assert_eq!(ui, 1.0),
            }
        }
    }

    /// A font whose capitals and lowercase ascenders rise `24`, other lowercase letters
    /// `17`, and whose `drawn` letters carry a picture. `lowercase_off`: those letters
    /// have no picture, as in the caps-only title font.
    fn measured_font(lowercase_off: bool) -> Face {
        let mut glyphs = vec![Glyph::default(); 256];
        let mut put = |code: u8, ascent: f32| {
            glyphs[code as usize] = Glyph { width: 10.0, height: 10.0, ascent, ..Glyph::default() };
        };
        for code in 0x20..0x7Fu8 {
            let c = code as char;
            let ascender = matches!(c, 'b' | 'd' | 'f' | 'h' | 'k' | 'l' | 't');
            let ascent = if c.is_ascii_uppercase() || ascender { 24.0 } else { 17.0 };
            put(code, ascent);
        }
        if lowercase_off {
            for code in 0x61..=0x7Au8 {
                glyphs[code as usize] = Glyph::default();
            }
        }
        Face { font: Font { size: 31.0, texture: String::new(), glyphs, extra: Default::default() }, image: None, vector: None }
    }

    /// #1829: the hint glyphs sit on the x-height band, not the cap-height middle. The
    /// lowercase ascenders (`b`, `d`, `f`, `h`, `k`, `l`, `t`) are in the lowercase range,
    /// so taking their tallest rise would return the cap height and change nothing.
    #[test]
    fn the_x_height_is_the_lowercase_band_not_the_cap_height() {
        let face = measured_font(false);
        assert_eq!(line_base(&face), 24.0);
        assert_eq!(x_height(&face), 17.0);
    }

    /// A font with no lowercase picture has no x-height band to centre on: all its text is
    /// capitals, so the glyphs centre on those.
    #[test]
    fn a_caps_only_font_centres_its_glyphs_on_the_cap_height() {
        let face = measured_font(true);
        assert_eq!(x_height(&face), line_base(&face));
    }

    #[test]
    fn fit_centered_box_preserves_desktop_and_clamps_high_scale_within_safe_area() {
        use crate::touch_safe_area::SafeInsets;
        let desktop = Vec2::new(1600.0, 900.0);
        assert_eq!(fit_centered_box(1.0, desktop, SafeInsets::default(), 1088.0, 840.0), None);
        assert_eq!(fit_centered_box(0.8, desktop, SafeInsets::default(), 1088.0, 840.0), None);

        let safe = SafeInsets { left: 48.0, top: 12.0, right: 48.0, bottom: 24.0 };
        for view in [Vec2::new(1280.0, 720.0), Vec2::new(2400.0, 1080.0), Vec2::new(960.0, 540.0), Vec2::new(800.0, 600.0)] {
            for scale in [1.25f32, 1.5, 2.0] {
                let (draw_scale, pad) = fit_centered_box(scale, view, safe, 1088.0, 840.0).expect("clamped");
                assert!(draw_scale <= scale && draw_scale > 0.1);
                let px_per_u = (view.y / 960.0) * draw_scale;
                assert!(1088.0 * px_per_u <= view.x - safe.left - safe.right + 0.5, "{view:?} @ {scale}");
                assert!(840.0 * px_per_u <= view.y - safe.top - safe.bottom + 0.5, "{view:?} @ {scale}");
                assert_eq!(pad.left, Val::Px(safe.left));
                assert_eq!(pad.bottom, Val::Px(safe.bottom));
            }
        }
    }

    #[test]
    fn xfonts_parse_and_slot_paths_preserve_vanilla_slots_and_normalize_paths() {
        let mut extra = BTreeMap::new();
        parse_xfonts(
            "\u{feff}   ; comment after BOM and leading spaces\n# hash\n// slash\n\
             0=textures\\fonts\\bad0.fnt\n\
             1=textures\\fonts\\override_vanilla.fnt\n\
             9=textures\\fonts\\override_nine.fnt\n\
             \u{feff}  21=textures\\fonts\\mapmo_hud_caps_font.fnt\n\
             22 = \"Data/Textures/Fonts/MAPMO_HUD_Caps_Font2.fnt\"\n\
             42=   \n\
             256=textures\\fonts\\too_high.fnt\n\
             not_a_line\n",
            &mut extra,
        );
        assert_eq!(extra.len(), 2);
        assert_eq!(extra.get(&21).map(String::as_str), Some("textures\\fonts\\mapmo_hud_caps_font.fnt"));
        assert_eq!(extra.get(&22).map(String::as_str), Some("textures\\fonts\\mapmo_hud_caps_font2.fnt"));
        assert!(!extra.contains_key(&0) && !extra.contains_key(&1) && !extra.contains_key(&9) && !extra.contains_key(&42));
        // Later definitions for the same extended slot replace earlier ones cleanly.
        parse_xfonts("22=\\textures/fonts/override_22.fnt\n", &mut extra);
        assert_eq!(extra.get(&22).map(String::as_str), Some("textures\\fonts\\override_22.fnt"));
        // Slot path resolution preserves vanilla 1..=9 and returns None for 0 / unregistered.
        assert_eq!(slot_font_path(0, &extra), None);
        assert_eq!(slot_font_path(1, &extra).as_deref(), Some(FONT_FILES[0]));
        assert_eq!(slot_font_path(9, &extra).as_deref(), Some(FONT_FILES[8]));
        assert_eq!(slot_font_path(21, &extra).as_deref(), Some("textures\\fonts\\mapmo_hud_caps_font.fnt"));
        assert_eq!(slot_font_path(22, &extra).as_deref(), Some("textures\\fonts\\override_22.fnt"));
        assert_eq!(slot_font_path(42, &extra), None);
    }
}

#[cfg(test)]
#[path = "interface/hud_scale_extended_tests.rs"]
mod hud_scale_extended_tests;
