//! The lockpicking menu's text and buttons as `menus\lockpick_menu.xml` sets them out
//! [documented], over the lock's picture ([`super::LockView`], drawn by the engine).
//! Lengths are the interface's units (960 to the view's height).
//!
//! - The main rectangle is the view less four times the safe zone, centred; the safe zone
//!   is 15 [hypothesis: the file's `cropx`/`cropy` are the safe zone, `iSafeZoneX/Y` 15,
//!   docs/spec-notes/dialogue-menu.md §3], so everything is 30 in from the view's edges.
//! - The information box: 400 × 175, 10 in from the main rectangle's left and bottom, a
//!   bracket of lines (a line fading upwards at the top left, the left side, the bottom
//!   and a line fading to the right), and three rows in font 7 at 20 in and 35, 80 and
//!   125 down: the player's skill, the bobby pins and the lock's level, each title on
//!   the left and its value right-aligned 80 short of the box's width.
//! - The buttons: right-aligned 50 from the main rectangle's right, the first 72 under
//!   the information box's top: Force Lock (F) and Exit (E) under it.
//! - The status line: font 7, right-aligned with the buttons, 450 down the main rectangle.
//! - All in the HUD's colour, without glow (`&hudmain;`, `NOGLOW_BRANCH`).
//! - The detect area, where the pointer is hidden ([`in_detect_area`]).
//! - With the debug switch, the debug displays ([`draw_debug`]).
//! - The help box over all of it the first time (`super::help`).
//! - As the menu leaves, all of it fades out with the lock's picture ([`fade`]).

use super::{LockView, LockpickDebug, LockpickInput, LockpickScreen, Session};
use crate::interface::{amber, at, face, plain_font, set_line, u, Face, Pictures};
use crate::pause::AidTime;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use crate::ui_pointer::Button;
use bevy::window::PrimaryWindow;
use fnv_game::lockpick::{Phase, FULL_TURN, PIN_RANGE};
use nvf_assets::GameData;

/// Font 7 [documented: the menu file's `<font> 7 </font>`; Fallout_default.ini
/// `sFontFile_7`].
const FONT: &str = "textures\\fonts\\baked-in_monofonto_large.fnt";
/// The help box's fonts: its title's font 6 and the default font 1 of its text
/// [documented: tutorial_menu.xml; the fonts' files as `pipboy::frame` lists them].
const TITLE_FONT: &str = "textures\\fonts\\glow_monofonto_vl_dialogs.fnt";
const BODY_FONT: &str = "textures\\fonts\\glow_monofonto_large.fnt";
/// How far font 7's letters stand under a text's top, and its line (dialogue-menu.md).
const BASE: f32 = 19.5;
const LINE: f32 = 36.0;
/// The same for the help box's fonts [hypothesis: as message-menu.md §3 measured the
/// title font, and the body font as the message box's text].
const TITLE_BASE: f32 = 21.5;
const BODY_BASE: f32 = 15.0;

/// From the view's edges to the main rectangle: twice the safe zone [hypothesis].
const EDGE: f32 = 30.0;
const INFO: Vec2 = Vec2::new(400.0, 175.0);
const INFO_IN: f32 = 10.0;
const ROW_X: f32 = 20.0;
const ROWS_Y: [f32; 3] = [35.0, 80.0, 125.0];
const VALUE_X: f32 = INFO.x - 80.0 + ROW_X;
/// Lines are 2 thick [documented: menus\globals.xml, on a PC]; the fading ones 30 long.
const THICK: f32 = 2.0;
const FADE: f32 = 30.0;
const BUTTONS_RIGHT: f32 = 50.0;
const BUTTONS_DOWN: f32 = 72.0;
/// A button is its text and 9 above and below [documented: `_verbuf` 9 of text_box.xml].
const BUTTON_H: f32 = LINE + 18.0;
/// The key hint after a button's text ("Exit E)") [design: as the barter menu's].
const HINT_GAP: f32 = 16.0;
const STATUS_Y: f32 = 450.0;
/// A button that cannot be used is drawn at half brightness [design].
const DIM: f32 = 128.0;
/// A button not chosen, as the help box's Close [observed: dimmer than the text on the
/// owner's screenshot; the value is a guess].
pub const BUTTON_DIM: f32 = 160.0;
/// How dark the world is behind the menu: with the engine's lock drawn over it, and
/// without. The owner's screenshots show the room dim behind the lock [observed], but a
/// see-through shade lets the interface drawn under the menu (corner and quest messages,
/// the explorer's lines) show through, so the lock stands on black for now [design].
const SHADE_BEHIND_LOCK: f32 = 1.0;
/// How bright the lock is drawn while the help box is up [observed: about half as bright
/// on the owner's screenshot of the help, all over the screen].
const UNDER_HELP: f32 = 0.47;
const SHADE: f32 = 0.88;

/// The detect area: from the main rectangle's left plus 30 (the file centres a box as wide
/// as the main rectangle on the screen's width, inside the main rectangle) over the main
/// rectangle's width, from 80 above its top for 720 [documented: `LPM_DetectArea`].
const DETECT_X: f32 = 30.0;
const DETECT_Y: f32 = -80.0;
const DETECT_H: f32 = 720.0;

/// The debug displays [documented: lockpick_menu.xml, shown in debug mode]: the meter 50
/// down the main rectangle, as wide, 10 high, at alpha 64; the arrow (38 × 24, 20 down)
/// over the pin's place; the pin's health right-aligned with the buttons, 240 down; the
/// tuning box (350 wide) at (10, 100) with "Sweet Spot" and "Concentric" 50 apart.
const METER_Y: f32 = 50.0;
const METER_H: f32 = 10.0;
const METER_ALPHA: f32 = 64.0 / 255.0;
const ARROW: &str = "interface\\hud\\glow_xp_bar_pointer.dds";
const ARROW_SIZE: Vec2 = Vec2::new(38.0, 24.0);
const ARROW_Y: f32 = 20.0;
const HEALTH_Y: f32 = 240.0;
const TWEAK: Vec2 = Vec2::new(10.0, 100.0);
const TWEAK_W: f32 = 350.0;
const TWEAK_ROW: f32 = 50.0;

/// The menu's root node.
#[derive(Component)]
pub struct LockpickRoot;

/// The debug displays' root node.
#[derive(Component)]
pub struct LockpickDebugRoot;

/// A line of a paragraph, by its number.
#[derive(Component, Clone, Copy)]
pub struct ParagraphLine(pub usize);

/// A button and what it does.
#[derive(Component, Clone, Copy)]
pub struct LockButton(pub LockpickInput);

/// The alpha a node was drawn with, before the menu fades out.
#[derive(Component, Clone, Copy)]
pub struct DrawnAlpha(f32);

/// The fonts and the interface's pictures, read when the menu first opens.
pub struct Kit {
    font: Face,
    title: Face,
    body: Face,
    pictures: Pictures,
}

impl Kit {
    pub(crate) fn load(data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Kit {
        Kit {
            font: face(FONT, plain_font(36.0, 15.0), data, images, bc),
            title: face(TITLE_FONT, plain_font(38.0, 16.0), data, images, bc),
            body: face(BODY_FONT, plain_font(31.0, 12.0), data, images, bc),
            pictures: Pictures::load(data),
        }
    }
}

/// Which of the kit's fonts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontKind {
    /// Font 7, the menu's own.
    Menu,
    /// Font 6, the help box's title.
    Title,
    /// Font 1, the help box's text.
    Body,
}

/// What the screen shows; drawn again when it changes.
fn view_key(s: &Session, has_view: bool, view_w: f32) -> String {
    format!(
        "{}:{}:{}:{:?}:{:?}:{}:{}:{}:{}:{view_w:.0}:{:?}",
        s.game.skill,
        s.game.pins,
        s.game.level.name(),
        s.status,
        s.game.phase == Phase::Picking && s.ended.is_none(),
        s.force_label(),
        has_view,
        s.help.is_some(),
        s.leaving.is_some(),
        crate::pad::labels(),
    )
}

/// The view's width in interface units.
fn view_width(window: Option<&Window>) -> f32 {
    window.map_or(960.0 * 16.0 / 9.0, |w| 960.0 * w.width() / w.height().max(1.0))
}

/// Whether a point of the window (logical pixels from the top left; the window `size`) is
/// in the detect area, where the pointer is hidden.
pub fn in_detect_area(p: Vec2, size: Vec2) -> bool {
    if size.y <= 0.0 {
        return false;
    }
    // The area is the interface's: where it is drawn at the UI scale (#1631).
    let p = crate::interface::from_menu_frame(p, size, crate::interface::MIDDLE);
    let k = 960.0 / size.y;
    let (x, y, view_w) = (p.x * k, p.y * k, size.x * k);
    let left = EDGE + DETECT_X;
    let top = EDGE + DETECT_Y;
    (left..=left + view_w - 2.0 * EDGE).contains(&x) && (top..=top + DETECT_H).contains(&y)
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    screen: Res<LockpickScreen>,
    view: Option<Res<LockView>>,
    data: Option<Res<GameData>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    roots: Query<Entity, With<LockpickRoot>>,
    (mut kit, mut shown): (Local<Option<Kit>>, Local<Option<String>>),
) {
    let view_w = view_width(window.single().ok());
    let key = screen.0.as_ref().map(|s| view_key(s, view.is_some(), view_w));
    if key == *shown {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    *shown = key;
    let Some(s) = screen.0.as_ref() else { return };
    let bc = crate::interface::bc_supported(formats.as_deref());
    let kit = kit.get_or_insert_with(|| Kit::load(data.as_deref(), &mut images, bc));
    let mut c = Ctx { kit, data: data.as_deref(), images: &mut images, bc, glyphs: &glyphs };
    spawn(&mut commands, &mut c, s, view.as_deref(), view_w);
}

pub struct Ctx<'a> {
    pub(crate) kit: &'a mut Kit,
    pub(crate) data: Option<&'a GameData>,
    pub(crate) images: &'a mut Assets<Image>,
    pub(crate) bc: bool,
    pub(crate) glyphs: &'a crate::pad::PadGlyphs,
}

impl Ctx<'_> {
    /// A picture of the interface stretched over `node` (a plain fill without it).
    pub fn image(&mut self, p: &mut ChildSpawnerCommands, path: &str, node: Node, color: Color) {
        match self.kit.pictures.picture(path, self.data, self.images, self.bc) {
            Some((image, rect)) => {
                p.spawn((node, ImageNode { rect, color, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) }));
            }
            None => {
                p.spawn((node, BackgroundColor(color)));
            }
        }
    }

    fn face(&self, kind: FontKind) -> (&Face, f32) {
        match kind {
            FontKind::Menu => (&self.kit.font, BASE),
            FontKind::Title => (&self.kit.title, TITLE_BASE),
            FontKind::Body => (&self.kit.body, BODY_BASE),
        }
    }

    /// The height of one line of the font.
    pub fn font_size(&self, kind: FontKind) -> f32 {
        self.face(kind).0.font.size
    }

    /// Where the font's printable ASCII glyphs reach, from a line's top: how far the
    /// tallest rises above it (none when every glyph stays under it) and how far down the
    /// deepest goes.
    pub fn ink(&self, kind: FontKind) -> (f32, f32) {
        let (face, base) = self.face(kind);
        let drawn = || face.font.glyphs.iter().skip(0x20).take(0x5F).filter(|g| g.drawn());
        let rise = drawn().map(|g| g.ascent - base).fold(0.0, f32::max);
        let fall = drawn().map(|g| base - g.ascent + g.height).fold(0.0, f32::max);
        (rise, fall)
    }

    /// How wide one line of text is in a font.
    pub fn text_width(&self, kind: FontKind, s: &str) -> f32 {
        crate::pad::hint_width(self.face(kind).0, s, self.glyphs)
    }

    /// Text in font 7 with its top at `top`; `right`: `x` is its right end. Its width.
    fn text(&mut self, p: &mut ChildSpawnerCommands, s: &str, x: f32, top: f32, right: bool, color: Color) -> f32 {
        self.text_in(p, FontKind::Menu, s, x, top, right, color)
    }

    /// One line of text in a font, its top at `top`; `right`: `x` is its right end. Its
    /// width.
    #[allow(clippy::too_many_arguments)]
    pub fn text_in(&mut self, p: &mut ChildSpawnerCommands, kind: FontKind, s: &str, x: f32, top: f32, right: bool, color: Color) -> f32 {
        let (face, base) = self.face(kind);
        let width = crate::pad::hint_width(face, s, self.glyphs);
        let x = if right { x - width } else { x };
        crate::pad::set_hint(p, face, s, x, top + base, color, self.glyphs);
        width
    }

    /// Text wrapped at `wrap`, its top at `top`, its lines the font's size and `leading`
    /// apart, each in its own [`ParagraphLine`]. The number of lines.
    #[allow(clippy::too_many_arguments)]
    pub fn paragraph(&mut self, p: &mut ChildSpawnerCommands, kind: FontKind, s: &str, x: f32, top: f32, wrap: f32, leading: f32, color: Color) -> usize {
        let (face, base) = self.face(kind);
        let pitch = face.font.size + leading;
        let lines = face.font.lines(s.trim_end(), Some(wrap));
        for (i, line) in lines.iter().enumerate() {
            p.spawn((ParagraphLine(i), at(x, top + pitch * i as f32, 0.0, 0.0))).with_children(|l| set_line(l, face, line, 0.0, base, color));
        }
        lines.len()
    }
}

fn spawn(commands: &mut Commands, c: &mut Ctx, s: &Session, view: Option<&LockView>, view_w: f32) {
    let bright = amber(255.0);
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    let mut root = commands.spawn((LockpickRoot, full.clone(), GlobalZIndex(106)));
    match view {
        // The lock as the engine draws it fills the view, the room dim behind it.
        Some(v) => {
            root.insert(BackgroundColor(Color::srgba(0.0, 0.0, 0.0, SHADE_BEHIND_LOCK)));
            // Under the help box the lock is drawn darker.
            let light = if s.help.is_some() { UNDER_HELP } else { 1.0 };
            root.with_children(|r| {
                r.spawn((full.clone(), ImageNode { color: Color::srgb(light, light, light), ..ImageNode::new(v.image.clone()) }));
            });
        }
        None => {
            root.insert(BackgroundColor(Color::srgba(0.0, 0.0, 0.0, SHADE)));
        }
    }
    // The rectangle and the help at the UI scale, in the middle of the view
    // (`crate::interface::menu_frame`, #1631); the lock's picture fills the view.
    root.with_children(|r| crate::interface::in_menu_frame(r, crate::interface::MIDDLE, |r| {
        // The main rectangle.
        let main = Node { position_type: PositionType::Absolute, left: u(EDGE), right: u(EDGE), top: u(EDGE), bottom: u(EDGE), ..default() };
        r.spawn(main).with_children(|m| {
            // The information box.
            let info = Node { position_type: PositionType::Absolute, left: u(INFO_IN), bottom: u(INFO_IN), width: u(INFO.x), height: u(INFO.y), ..default() };
            m.spawn(info).with_children(|b| {
                c.image(b, "interface\\shared\\line\\fade_to_top.dds", at(0.0, 0.0, THICK, FADE), bright);
                c.image(b, "interface\\shared\\line\\solid.dds", at(0.0, FADE - THICK, THICK, INFO.y - FADE), bright);
                c.image(b, "interface\\shared\\line\\solid.dds", at(0.0, INFO.y - THICK, INFO.x - FADE, THICK), bright);
                c.image(b, "interface\\shared\\line\\fade_to_right.dds", at(INFO.x - FADE - THICK, INFO.y - THICK, FADE, THICK), bright);
                // The rows' titles are the file's game settings (`Words`, from the game
                // strings table) [documented: lockpick_menu.xml].
                let words = &s.words;
                let rows = [(&words.skill, format!("{:.0}", s.game.skill)), (&words.picks, s.game.pins.to_string()), (&words.level, s.game.level.name().to_string())];
                for ((title, value), y) in rows.iter().zip(ROWS_Y) {
                    c.text(b, title, ROW_X, y, false, bright);
                    c.text(b, value, VALUE_X, y, true, bright);
                }
            });
            // The buttons, right-aligned 50 from the right, from 72 under the box's top;
            // not usable under the help box (the tutorial menu lets no click past,
            // documented: tutorial_menu.xml `no_click_past`).
            let usable = s.help.is_none() && s.leaving.is_none();
            let force_on = s.game.phase == Phase::Picking && s.ended.is_none();
            // The game's PC hints, or the buttons that press their keys here: X (Force,
            // `crate::pad::menu_buttons`) and B (Esc) [#1662].
            let hint_at = crate::pad::at_menu(fnv_game::game::menus::LOCKPICK);
            let force_hint = crate::pad::game_hint(&s.words.force_hint, &[KeyCode::KeyF], hint_at);
            let exit_hint = crate::pad::game_hint(&s.words.exit_hint, &[KeyCode::KeyE, KeyCode::Escape], hint_at);
            let buttons = [(s.force_label(), &force_hint, LockpickInput::Force(None), force_on), (s.words.exit.clone(), &exit_hint, LockpickInput::Exit, true)];
            let first = Node { position_type: PositionType::Absolute, right: u(BUTTONS_RIGHT), bottom: u(INFO_IN + INFO.y - BUTTONS_DOWN - 2.0 * BUTTON_H), width: u(0.0), height: u(2.0 * BUTTON_H), ..default() };
            m.spawn(first).with_children(|b| {
                for (i, (label, hint, cmd, on)) in buttons.into_iter().enumerate() {
                    let color = amber(if on { 255.0 } else { DIM });
                    let y = i as f32 * BUTTON_H;
                    let w = c.text(b, &label, 0.0, y + 9.0, true, color);
                    c.text(b, hint, HINT_GAP, y + 9.0, false, color);
                    if on && usable {
                        b.spawn((Button, LockButton(cmd), at(-w - 10.0, y, w + 20.0 + HINT_GAP + 30.0, BUTTON_H)));
                    }
                }
            });
            // The status line.
            if let Some(status) = &s.status {
                let line = Node { position_type: PositionType::Absolute, right: u(BUTTONS_RIGHT), top: u(STATUS_Y), width: u(0.0), height: u(LINE), ..default() };
                m.spawn(line).with_children(|b| {
                    c.text(b, status, 0.0, 0.0, true, bright);
                });
            }
        });
        if let Some(h) = &s.help {
            super::help::spawn(r, c, h, view_w);
        }
    }));
}

/// What the debug displays show, rounded so they are not drawn again every frame.
fn debug_key(s: &Session) -> String {
    let g = &s.game;
    format!("{:.1}:{:.1}:{:.1}:{:.0}:{:.2}:{:?}", g.pin, g.sweet, g.half, g.settings.concentric, g.health, g.phase)
}

/// The debug displays (debug switch `NVF_PLAY_DEBUG_LOCKPICK`): the meter with the sweet
/// spot (bright) and the ring around it (dim) along the half circle, the arrow at the
/// pin's angle, the pin's health and the two tuning values [documented: the displays and
/// their places; design: the colours (the file leaves them to the code), the words, and
/// that the tuning values are shown, not changed].
#[allow(clippy::too_many_arguments)]
pub fn draw_debug(
    mut commands: Commands,
    screen: Res<LockpickScreen>,
    debug: Res<LockpickDebug>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    roots: Query<Entity, With<LockpickDebugRoot>>,
    (mut kit, mut shown): (Local<Option<Kit>>, Local<Option<String>>),
) {
    let key = screen.0.as_ref().filter(|_| debug.0).map(debug_key);
    if key == *shown {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    *shown = key;
    let Some(s) = screen.0.as_ref().filter(|_| debug.0) else { return };
    let bc = crate::interface::bc_supported(formats.as_deref());
    let kit = kit.get_or_insert_with(|| Kit::load(data.as_deref(), &mut images, bc));
    let mut c = Ctx { kit, data: data.as_deref(), images: &mut images, bc, glyphs: &glyphs };
    let g = &s.game;
    let bright = amber(255.0);
    let pct = |deg: f32| Val::Percent((deg / PIN_RANGE * 100.0).clamp(0.0, 100.0));
    let span = |from: f32, to: f32| {
        let (a, b) = (from.clamp(0.0, PIN_RANGE), to.clamp(0.0, PIN_RANGE));
        Node { position_type: PositionType::Absolute, left: pct(a), width: pct(b - a), top: u(0.0), height: u(METER_H), ..default() }
    };
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands.spawn((LockpickDebugRoot, crate::interface::framed_root(full, crate::interface::MIDDLE), GlobalZIndex(107))).with_children(|r| {
        let main = Node { position_type: PositionType::Absolute, left: u(EDGE), right: u(EDGE), top: u(EDGE), bottom: u(EDGE), ..default() };
        r.spawn(main).with_children(|m| {
            let meter = Node { position_type: PositionType::Absolute, left: u(0.0), right: u(0.0), top: u(METER_Y), height: u(METER_H), ..default() };
            m.spawn((meter, BackgroundColor(amber(255.0).with_alpha(METER_ALPHA)))).with_children(|b| {
                let ring = g.half + g.settings.concentric;
                b.spawn((span(g.sweet - ring, g.sweet + ring), BackgroundColor(amber(255.0).with_alpha(0.35))));
                b.spawn((span(g.sweet - g.half, g.sweet + g.half), BackgroundColor(amber(255.0).with_alpha(0.9))));
            });
            // The arrow over the pin's angle.
            let arrow_row = Node { position_type: PositionType::Absolute, left: u(0.0), right: u(0.0), top: u(ARROW_Y), height: u(ARROW_SIZE.y), ..default() };
            m.spawn(arrow_row).with_children(|b| {
                let node = Node {
                    position_type: PositionType::Absolute,
                    left: pct(g.pin),
                    margin: UiRect::left(u(-ARROW_SIZE.x / 2.0)),
                    width: u(ARROW_SIZE.x),
                    height: u(ARROW_SIZE.y),
                    ..default()
                };
                c.image(b, ARROW, node, bright);
            });
            // The pin's health, right-aligned with the buttons.
            let health = Node { position_type: PositionType::Absolute, right: u(BUTTONS_RIGHT), top: u(HEALTH_Y), width: u(0.0), height: u(LINE), ..default() };
            m.spawn(health).with_children(|b| {
                let lock = format!("Pin health {:.0}%  Lock {:.0}° of {:.0}°", g.health * 100.0, g.max_turn(), FULL_TURN);
                c.text(b, &lock, 0.0, 0.0, true, bright);
            });
            // The tuning values.
            let tweak = Node { position_type: PositionType::Absolute, left: u(TWEAK.x), top: u(TWEAK.y), width: u(TWEAK_W), height: u(2.0 * TWEAK_ROW), ..default() };
            m.spawn(tweak).with_children(|b| {
                for (i, (title, value)) in [("Sweet Spot", 2.0 * g.half), ("Concentric", g.settings.concentric)].into_iter().enumerate() {
                    let y = i as f32 * TWEAK_ROW;
                    c.text(b, title, 0.0, y, false, bright);
                    c.text(b, &format!("{value:.1}"), TWEAK_W, y, true, bright);
                }
            });
        });
    });
}

type MenuRoots = Or<(With<LockpickRoot>, With<LockpickDebugRoot>)>;
type Faded = (Option<&'static mut ImageNode>, Option<&'static mut BackgroundColor>, Option<&'static mut TextColor>, Option<&'static DrawnAlpha>);

/// As the menu leaves, everything it draws fades out (the lock's picture with it).
pub fn fade(
    mut commands: Commands,
    screen: Res<LockpickScreen>,
    time: AidTime,
    roots: Query<Entity, MenuRoots>,
    children: Query<&Children>,
    mut nodes: Query<Faded>,
) {
    let Some(s) = screen.0.as_ref() else { return };
    if s.leaving.is_none() {
        return;
    }
    let f = s.fade(time.elapsed_secs());
    for root in &roots {
        for e in std::iter::once(root).chain(children.iter_descendants(root)) {
            let Ok((image, background, text, drawn)) = nodes.get_mut(e) else { continue };
            let current = image.as_ref().map(|i| i.color.alpha()).or(background.as_ref().map(|b| b.0.alpha())).or(text.as_ref().map(|t| t.0.alpha()));
            let Some(current) = current else { continue };
            let base = match drawn {
                Some(d) => d.0,
                None => {
                    commands.entity(e).insert(DrawnAlpha(current));
                    current
                }
            };
            let a = base * f;
            if let Some(mut i) = image {
                i.color.set_alpha(a);
            }
            if let Some(mut b) = background {
                b.0.set_alpha(a);
            }
            if let Some(mut t) = text {
                t.0.set_alpha(a);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_hides_over_the_lock_only() {
        // A 1280 × 720 window: 960 units high, 1706.7 wide; the area runs from 60 units in
        // to the right edge, from the top to 670 units down.
        let size = Vec2::new(1280.0, 720.0);
        assert!(in_detect_area(Vec2::new(640.0, 360.0), size));
        assert!(in_detect_area(Vec2::new(640.0, 0.0), size));
        // The left edge (the owner's screenshot shows the pointer there) and the
        // information box's rows below the area.
        assert!(!in_detect_area(Vec2::new(5.0, 310.0), size));
        assert!(!in_detect_area(Vec2::new(200.0, 600.0), size));
        assert!(!in_detect_area(Vec2::new(640.0, 360.0), Vec2::ZERO));
    }
}
