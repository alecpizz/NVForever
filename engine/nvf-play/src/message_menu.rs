//! The message box, drawn as the original draws it (docs/spec-notes/message-menu.md).
//!
//! `script_hud` says what the box holds ([`MessagePanel`]: a title, a text, buttons);
//! this module sets it out by the game's own interface file and draws it with the game's
//! fonts and line textures, read from the owner's archives at run time (§2, §3). Lengths
//! are in the interface's units, 960 to the height of the view.
//!
//! - [`layout`] places everything (pure, tested without the game's files).
//! - The box is rebuilt when its content changes; after that only the frame around the
//!   chosen button moves. The pointer chooses the button it is on.
//! - Without the game's fonts (no archives, or a font that cannot be read) the same
//!   layout is drawn with Bevy's font and plain lines.
//! - Fonts, textures and units are `crate::interface`'s, shared with the other parts of
//!   the interface.
//! - With `[UI] TileMenus=MessageMenu` (T15a) a message box is drawn from the game's
//!   `MessageMenu` tiles instead (`crate::tiles::menus`) and this module spawns nothing for
//!   it; the panels of the other script menus it shows stay drawn here.

use crate::interface::{amber, at, bc_supported, face, plain_font, set_text, texture, u, Face, Set};
use crate::script_hud::BoxButton;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use crate::ui_pointer::{Button, PointerState};
use fnv_font::{Font, Line};
use nvf_assets::GameData;

// ---- the interface file's numbers [documented: menus\message_menu.xml, §2] -----------

const MIN_WIDTH: f32 = 200.0;
const MAX_WIDTH: f32 = 700.0;
/// Room between the outline and the text, sideways and upwards.
const HOR_BUF: f32 = 40.0;
const VER_BUF: f32 = 20.0;
/// The title starts this far in and stands this far above the top line.
const TITLE_X: f32 = 60.0;
const TITLE_Y: f32 = -10.0;
/// The top line stops this far before the title and goes on this far after it.
const TITLE_GAP: f32 = 10.0;
/// The short lines down from the top corners [documented] and up from the bottom
/// corners [observed: screenshot].
const CORNER: f32 = 40.0;
/// A button's row is its text's height and this much more, the text at the row's
/// bottom [documented: the list's template].
const BUTTON_PAD: f32 = 20.0;
/// Button texts wrap this much narrower than the message [documented].
const BUTTON_WRAP_LESS: f32 = 60.0;
/// The thickness of every line [documented: menus\globals.xml, on a PC].
const THICK: f32 = 2.0;
/// The icon left of the text, when the code turns it on: its size, and how much further
/// right the text starts (and how much narrower it wraps) [documented].
const ICON: f32 = 100.0;
const ICON_ROOM: f32 = 120.0;

// ---- measured on the owner's screenshot [observed, §3] ------------------------------

/// The height the title takes, whatever it says.
const TITLE_HEIGHT: f32 = 38.0;
/// How far under a text's top its first line stands: the title's font and the other.
const TITLE_BASE: f32 = 21.5;
const TEXT_BASE: f32 = 15.0;
/// Lines of a text are the font's size and this much apart.
const LEADING: f32 = 4.0;
/// The frame around the chosen button is this much wider than the widest button text,
/// and its middle this far right of the box's.
const FRAME_PAD: f32 = 42.0;
const FRAME_SHIFT: f32 = 2.5;
/// How dark the box makes what is behind it (as the Wait menu's, wait-menu.md §2).
pub(crate) const SHADE: f32 = 0.93;

/// What a box holds. `buttons` are the labels as drawn ("OK" for a box without any).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PanelView {
    pub title: Option<String>,
    pub text: String,
    pub buttons: Vec<String>,
    /// A picture left of the text (a path under the archives' `textures\`, whole), as
    /// the reputation boxes have (`crate::reputation`).
    pub icon: Option<String>,
}

/// The box to draw (set by `script_hud`) and its chosen button.
#[derive(Resource, Default)]
pub struct MessagePanel {
    pub view: Option<PanelView>,
    pub selected: usize,
    /// A row was chosen since the box opened (the pointer onto it, or a key). The box drawn
    /// from its tiles frames `selected` only then: the original chooses no row when a box
    /// opens [observed: hud_menus capture 2026-10-31, `_highlight_y` -1 and the highlight
    /// hidden at open]. The hand-built box frames `selected` from the start (§4).
    pub chosen: bool,
    /// Which showing of a box `view` is (`fnv_game::Game::front_box`'s serial), so a box
    /// replaced by one alike in every word is a new box too.
    pub showing: Option<u64>,
}

impl MessagePanel {
    /// A box (or none) to show, the same showing as before.
    pub fn set(&mut self, view: Option<PanelView>) {
        self.show(view, self.showing);
    }

    /// A box (or none) to show, and which showing it is: another view or another showing
    /// starts at the first button with no row chosen (§4; T15a).
    pub fn show(&mut self, view: Option<PanelView>, showing: Option<u64>) {
        if self.view != view || self.showing != showing {
            self.view = view;
            self.showing = showing;
            self.selected = 0;
            self.chosen = false;
        }
    }

    /// The pointer (or a ref) chooses row `i`.
    pub fn choose(&mut self, i: usize) {
        self.selected = i;
        self.chosen = true;
    }

    /// Up (-1) or Down (1). With `first_picks` (the box drawn from tiles) and no row chosen
    /// yet, the key chooses the first row without moving [observed: hud_menus capture
    /// 2026-10-31, a tapped Down chose row 0 of a box with none chosen; design: Up the
    /// same]; otherwise it moves the choice ([`MessagePanel::step`]).
    pub fn key(&mut self, by: i32, first_picks: bool) {
        if !(first_picks && !self.chosen) {
            self.step(by);
        }
        self.chosen = true;
    }

    /// The row the box frames when drawn from its tiles: `selected` once one was chosen.
    pub fn framed(&self) -> Option<usize> {
        self.chosen.then_some(self.selected)
    }

    /// Move the choice up (-1) or down (1), stopping at the ends.
    pub fn step(&mut self, by: i32) {
        let n = self.view.as_ref().map_or(0, |v| v.buttons.len());
        if n > 0 {
            self.selected = (self.selected as i32 + by).clamp(0, n as i32 - 1) as usize;
        }
    }
}

// ---- layout --------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonRow {
    pub y: f32,
    pub height: f32,
    /// Relative to the box, as everything else.
    pub lines: Vec<Set>,
}

/// The box set out, from its top left corner (the left end of the top line).
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub width: f32,
    /// Down to the bottom line.
    pub height: f32,
    pub title: Option<Set>,
    /// The top line's pieces: start and length.
    pub top: Vec<(f32, f32)>,
    pub text: Vec<Set>,
    pub buttons: Vec<ButtonRow>,
    /// The frame's left edge and width, the same for every button.
    pub frame: (f32, f32),
    /// The icon's top left corner ([`ICON`] square).
    pub icon: Option<Vec2>,
}

/// The height of a text of `lines` lines.
fn text_height(font: &Font, lines: usize) -> f32 {
    match lines {
        0 => 0.0,
        n => font.size + (font.size + LEADING) * (n - 1) as f32,
    }
}

pub fn layout(view: &PanelView, body: &Font, title_font: &Font) -> Layout {
    let room = if view.icon.is_some() { ICON_ROOM } else { 0.0 };
    let wrap = MAX_WIDTH - 2.0 * HOR_BUF - room;
    let text = view.text.trim_end();
    let lines = if text.is_empty() { Vec::new() } else { body.lines(text, Some(wrap)) };
    let buttons: Vec<Vec<Line>> = view.buttons.iter().map(|b| body.lines(b.trim(), Some(wrap - BUTTON_WRAP_LESS))).collect();
    let title = view.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(|t| title_font.lines(t, None).remove(0));
    let widest = |ls: &[Line]| ls.iter().map(|l| l.width).fold(0.0, f32::max);
    let widest_button = buttons.iter().map(|b| widest(b)).fold(0.0, f32::max);
    // The file leaves the width to the game's code: the text's and the buffers, within
    // the file's limits [hypothesis: the screenshot's full-width text gives 700], and
    // never less than the title or a button's frame needs [design].
    let need = [
        widest(&lines) + 2.0 * HOR_BUF + room,
        title.as_ref().map_or(0.0, |t| TITLE_X + t.width + TITLE_GAP + TITLE_X),
        widest_button + FRAME_PAD + 2.0 * HOR_BUF,
    ];
    let width = need.into_iter().fold(MIN_WIDTH, f32::max).min(MAX_WIDTH);

    let top = match &title {
        Some(t) => vec![(0.0, TITLE_X - TITLE_GAP), (TITLE_X + t.width + TITLE_GAP, width - TITLE_X - t.width - TITLE_GAP)],
        None => vec![(0.0, width)],
    };
    let title = title.map(|line| Set { line, x: TITLE_X, base: TITLE_Y + TITLE_BASE });
    // [documented] The text starts under the title's room; the buttons under the text.
    let text_y = TITLE_Y + TITLE_HEIGHT + VER_BUF - 5.0;
    let pitch = body.size + LEADING;
    let text_sets = lines.iter().enumerate().map(|(i, l)| Set { line: l.clone(), x: HOR_BUF + room, base: text_y + TEXT_BASE + pitch * i as f32 }).collect();
    let mut y = text_y - 10.0 + if lines.is_empty() { 0.0 } else { text_height(body, lines.len()) + 30.0 };
    // The icon at the text's top, at the free room's end [hypothesis: the file gives
    // its size and the text's shift; the owner's screenshot of a reputation box shows its
    // middle about 90 in and 90 down]; the buttons stay under it.
    let icon = view.icon.as_ref().map(|_| Vec2::new(HOR_BUF, text_y));
    if icon.is_some() {
        y = y.max(text_y - 10.0 + ICON + 30.0);
    }
    let middle = width / 2.0 + FRAME_SHIFT;
    let mut rows = Vec::new();
    for b in &buttons {
        let height = text_height(body, b.len()) + BUTTON_PAD;
        let sets = b.iter().enumerate().map(|(i, l)| Set { line: l.clone(), x: middle - l.width / 2.0, base: y + BUTTON_PAD + TEXT_BASE + pitch * i as f32 }).collect();
        rows.push(ButtonRow { y, height, lines: sets });
        y += height;
    }
    let frame_width = widest_button + FRAME_PAD;
    Layout { width, height: y + VER_BUF, title, top, text: text_sets, buttons: rows, frame: (middle - frame_width / 2.0, frame_width), icon }
}

// ---- the game's fonts and textures ----------------------------------------------------

/// What the box is drawn with. The font files are the game's defaults for font 2 (text
/// and buttons) and font 6 (title) [documented: Fallout_default.ini `sFontFile_2`, `_6`;
/// the interface file names the fonts by number].
struct Faces {
    body: Face,
    title: Face,
    fade_down: Option<Handle<Image>>,
    fade_up: Option<Handle<Image>>,
    shade: Option<Handle<Image>>,
}

const BODY_FONT: &str = "textures\\fonts\\monofonto_large.fnt";
const TITLE_FONT: &str = "textures\\fonts\\glow_monofonto_vl_dialogs.fnt";

fn load_faces(data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Faces {
    let mut line = |name: &str| data.and_then(|d| texture(&format!("textures\\interface\\shared\\{name}.dds"), d, images, bc)).map(|t| t.0);
    let (fade_down, fade_up, shade) = (line("line\\fade_to_bottom"), line("line\\fade_to_top"), line("background\\solid_black"));
    Faces {
        body: face(BODY_FONT, plain_font(31.0, 12.0), data, images, bc),
        title: face(TITLE_FONT, plain_font(34.0, 14.0), data, images, bc),
        fade_down,
        fade_up,
        shade,
    }
}

// ---- drawing ---------------------------------------------------------------------------

pub struct MessageMenuPlugin;

impl Plugin for MessageMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MessagePanel>().add_systems(Update, (point, draw).chain().in_set(crate::PlaySet::Ui).in_set(DrawBox));
    }
}

/// The drawing of the box, for what fills [`MessagePanel`] to run before.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DrawBox;

#[derive(Component)]
struct BoxRoot;

/// The frame of a button (0-based), shown while the button is the chosen one.
#[derive(Component)]
struct Frame(usize);

type BoxFrame = (Vec2, crate::touch_safe_area::SafeInsets, f32);

fn spawn_box(
    commands: &mut Commands,
    view: &PanelView,
    faces: &Faces,
    selected: usize,
    icon: Option<Handle<Image>>,
    (view_px, safe, menu_scale): BoxFrame,
) {
    let l = layout(view, &faces.body.font, &faces.title.font);
    let fit = crate::interface::fit_centered_box(menu_scale, view_px, safe, l.width + 48.0, l.height + 48.0);
    let mut screen = Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    if let Some((_, pad)) = fit {
        screen.padding = pad;
    }
    let _units = fit.map(|(s, _)| crate::interface::units_at(s));
    let bright = amber(255.0);
    commands.spawn((BoxRoot, screen, GlobalZIndex(108))).with_children(|root| {
        root.spawn(Node { width: u(l.width), height: u(l.height), ..default() }).with_children(|b| {
            // [documented] The dark picture behind: a tenth wider and three tenths taller
            // than the box, a little up and left of it.
            let (w, h) = (l.width * 1.1, l.height * 1.3);
            let behind = at(-0.04 * w, -0.1 * h, w, h);
            match &faces.shade {
                Some(image) => b.spawn((behind, ImageNode { color: Color::srgba(0.0, 0.0, 0.0, SHADE), image_mode: NodeImageMode::Stretch, ..ImageNode::new(image.clone()) })),
                None => b.spawn((behind, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, SHADE)))),
            };
            // The outline: the top line around the title, the bottom line, and at each
            // corner a short line that fades out.
            for &(x, len) in &l.top {
                b.spawn((at(x, 0.0, len, THICK), BackgroundColor(bright)));
            }
            b.spawn((at(0.0, l.height, l.width, THICK), BackgroundColor(bright)));
            for x in [0.0, l.width - THICK] {
                for (image, y) in [(&faces.fade_down, 0.0), (&faces.fade_up, l.height + THICK - CORNER)] {
                    if let Some(image) = image {
                        b.spawn((at(x, y, THICK, CORNER), ImageNode { color: bright, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image.clone()) }));
                    }
                }
            }
            if let Some(t) = &l.title {
                set_text(b, &faces.title, t, Vec2::ZERO, bright, ());
            }
            for t in &l.text {
                set_text(b, &faces.body, t, Vec2::ZERO, bright, ());
            }
            if let (Some(at_), Some(image)) = (l.icon, icon) {
                b.spawn((at(at_.x, at_.y, ICON, ICON), ImageNode { color: bright, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) }));
            }
            let (frame_x, frame_w) = l.frame;
            for (i, row) in l.buttons.iter().enumerate() {
                b.spawn((Button, BoxButton(i), at(frame_x, row.y, frame_w, row.height))).with_children(|r| {
                    // [documented: the list's highlight box] One unit short of the row;
                    // a faint fill and a line around it.
                    let h = row.height - 1.0;
                    let shown = if i == selected { Visibility::Inherited } else { Visibility::Hidden };
                    r.spawn((Frame(i), at(0.0, 0.0, frame_w, h), shown)).with_children(|f| {
                        // 40 of 255, as dark as it reads where the game mixes it in.
                        f.spawn((at(0.0, 0.0, frame_w, h), BackgroundColor(amber(128.0).with_alpha((40.0f32 / 255.0).powf(2.2)))));
                        let line = amber(200.0);
                        for y in [0.0, h - THICK] {
                            f.spawn((at(0.0, y, frame_w, THICK), BackgroundColor(line)));
                        }
                        for x in [0.0, frame_w - THICK] {
                            f.spawn((at(x, THICK, THICK, h - 2.0 * THICK), BackgroundColor(line)));
                        }
                    });
                    for t in &row.lines {
                        set_text(r, &faces.body, t, Vec2::new(frame_x, row.y), bright, ());
                    }
                });
            }
        });
    });
}

/// The pointer chooses the button it comes onto.
fn point(buttons: Query<(&PointerState, &BoxButton), Changed<PointerState>>, mut panel: ResMut<MessagePanel>) {
    for (interaction, b) in &buttons {
        if *interaction == PointerState::Hovered && panel.selected != b.0 {
            panel.selected = b.0;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    panel: Res<MessagePanel>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    roots: Query<Entity, With<BoxRoot>>,
    mut frames: Query<(&Frame, &mut Visibility)>,
    mut faces: Local<Option<Faces>>,
    (mut shown, mut was_tiled, mut font_revision): (Local<Option<PanelView>>, Local<bool>, Local<u32>),
    mut last_frame: Local<Option<BoxFrame>>,
    (menus, script): (Option<Res<crate::tiles::vanilla::TileMenus>>, Option<Res<crate::script::ScriptState>>),
    (windows, safe): (Query<&Window, With<bevy::window::PrimaryWindow>>, Option<Res<crate::touch_safe_area::SafeInsets>>),
) {
    // T15a: the box drawn from its tiles spawns nothing here.
    let tiled = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| crate::tiles::menus::message_tiled(menus.as_ref().is_some_and(|m| m.message), &rt.game));
    let revision = crate::cjk_font::revision();
    let view_px = windows.single().map_or(Vec2::new(1280.0, 720.0), |w| Vec2::new(w.width(), w.height()));
    let safe_insets = safe.as_deref().copied().unwrap_or_default();
    let frame_key = (view_px, safe_insets, crate::interface::unit_scale());
    let frame_changed = panel.view.is_some() && !tiled && *last_frame != Some(frame_key);
    if panel.view != *shown || tiled != *was_tiled || *font_revision != revision || frame_changed {
        *was_tiled = tiled;
        *font_revision = revision;
        *last_frame = Some(frame_key);
        for e in &roots {
            commands.entity(e).despawn();
        }
        // Drawn from tiles, the box keeps the pointer from what is under it with a sheet
        // alone (`crate::tiles::menus::modal_sheet`; the hand-built box's rows take it
        // where they stand); it goes with the box.
        if crate::tiles::menus::modal(panel.view.is_some(), tiled) == crate::tiles::menus::Modal::Sheet {
            commands.spawn((BoxRoot, Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, GlobalZIndex(108))).with_children(|root| {
                root.spawn(crate::tiles::menus::modal_sheet());
            });
        }
        if let Some(view) = panel.view.as_ref().filter(|_| !tiled) {
            let faces = faces.get_or_insert_with(|| {
                load_faces(data.as_deref(), &mut images, bc_supported(formats.as_deref()))
            });
            let icon = view.icon.as_deref().and_then(|p| data.as_deref().and_then(|d| texture(p, d, &mut images, bc_supported(formats.as_deref())))).map(|t| t.0);
            spawn_box(&mut commands, view, faces, panel.selected, icon, frame_key);
        }
        shown.clone_from(&panel.view);
        return;
    }
    for (frame, mut v) in &mut frames {
        v.set_if_neq(if frame.0 == panel.selected { Visibility::Inherited } else { Visibility::Hidden });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Letters 10 wide with 1 after them, spaces 5; sizes as the game's two fonts.
    fn font(size: f32) -> Font {
        let mut f = plain_font(size, 11.0);
        f.glyphs[b' ' as usize].after = 5.0;
        f
    }

    fn view(title: Option<&str>, text: &str, buttons: &[&str]) -> PanelView {
        PanelView { title: title.map(str::to_string), text: text.into(), buttons: buttons.iter().map(|b| b.to_string()).collect(), icon: None }
    }

    #[test]
    fn an_icon_moves_the_text_right_and_keeps_the_buttons_under_it() {
        let mut v = view(Some("Goodsprings"), "Liked
People like you.", &["Ok"]);
        let plain = layout(&v, &font(31.0), &font(34.0));
        v.icon = Some(r"textures\interface\icons\message icons\glow_message_vaultboy_very_happy.dds".into());
        let l = layout(&v, &font(31.0), &font(34.0));
        assert_eq!(l.icon, Some(Vec2::new(40.0, 43.0)));
        assert_eq!(l.text[0].x, 160.0);
        assert_eq!(l.text[0].base, plain.text[0].base);
        // Two lines are shorter than the icon: the button moves down below it.
        assert!(l.buttons[0].y >= 43.0 + ICON);
        assert!(plain.icon.is_none());
    }

    #[test]
    fn the_screenshots_box_is_set_out_as_measured() {
        // Thirteen full lines, a title and one button, as on the owner's screenshot:
        // each line is 56 letters, and two of them do not fit the wrap width.
        let line = "a".repeat(56);
        let text = vec![line.as_str(); 13].join(" ");
        let l = layout(&view(Some("Title"), &text, &["OK"]), &font(31.0), &font(34.0));
        assert_eq!(l.width, 696.0);
        assert_eq!(l.text.len(), 13);
        // The first line stands 58 under the top line, the next 35 lower.
        assert_eq!((l.text[0].x, l.text[0].base, l.text[1].base), (40.0, 58.0, 93.0));
        // The button's row starts 514 down and is 51 high; its text stands at 549.
        let b = &l.buttons[0];
        assert_eq!((b.y, b.height, b.lines[0].base), (514.0, 51.0, 549.0));
        assert_eq!(l.height, 585.0);
        // The title, and the top line around it.
        let t = l.title.as_ref().unwrap();
        assert_eq!((t.x, t.base, t.line.width), (60.0, 11.5, 55.0));
        assert_eq!(l.top, vec![(0.0, 50.0), (125.0, 571.0)]);
        // The frame: the button's text and 42, its middle 2.5 right of the box's.
        assert_eq!(l.frame, (350.5 - 32.0, 64.0));
        assert_eq!(b.lines[0].x, 350.5 - 11.0);
    }

    #[test]
    fn a_small_box_keeps_the_least_width() {
        let l = layout(&view(None, "Hi", &["OK"]), &font(31.0), &font(34.0));
        assert_eq!(l.width, 200.0);
        assert_eq!(l.top, vec![(0.0, 200.0)]);
        assert!(l.title.is_none());
        // A box without text: the buttons move up into its place.
        let none = layout(&view(None, "", &["OK"]), &font(31.0), &font(34.0));
        assert_eq!(none.buttons[0].y, 33.0);
        assert!(none.text.is_empty());
    }

    #[test]
    fn buttons_stand_one_under_the_other() {
        let l = layout(&view(None, "Choose", &["Leave him alone.", "Treat with medicine.", "Use medical equipment to treat."]), &font(31.0), &font(34.0));
        let ys: Vec<f32> = l.buttons.iter().map(|b| b.y).collect();
        assert_eq!(ys, vec![94.0, 145.0, 196.0]);
        assert_eq!(l.height, 196.0 + 51.0 + 20.0);
        // Every button's text is centred on the same middle; the frame fits the widest.
        let widest = l.buttons[2].lines[0].line.width;
        assert_eq!(l.frame.1, widest + 42.0);
        for b in &l.buttons {
            let t = &b.lines[0];
            assert!((t.x + t.line.width / 2.0 - (l.width / 2.0 + 2.5)).abs() < 1e-3);
        }
        // The box is wide enough for the frame.
        assert!(l.width >= l.frame.1 + 80.0);
    }

    #[test]
    fn the_choice_stops_at_the_ends() {
        let mut p = MessagePanel::default();
        p.set(Some(view(None, "x", &["a", "b", "c"])));
        p.step(-1);
        assert_eq!(p.selected, 0);
        p.step(1);
        p.step(1);
        p.step(1);
        assert_eq!(p.selected, 2);
        // A new box starts at its first button.
        p.set(Some(view(None, "y", &["a"])));
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn a_box_from_tiles_opens_with_no_row_chosen() {
        let mut p = MessagePanel::default();
        p.set(Some(view(None, "x", &["a", "b", "c"])));
        assert_eq!(p.framed(), None, "no row chosen at open");
        // The first Down chooses the first row; the next moves on.
        p.key(1, true);
        assert_eq!(p.framed(), Some(0));
        p.key(1, true);
        assert_eq!(p.framed(), Some(1));
        // The pointer chooses a row; a new box chooses none again.
        p.choose(2);
        assert_eq!(p.framed(), Some(2));
        p.set(Some(view(None, "y", &["a"])));
        assert_eq!(p.framed(), None);
        // A box replaced by one alike in every word (the next showing): no row chosen, and
        // the first Down picks the first row again.
        let a = view(None, "same", &["a", "b"]);
        p.show(Some(a.clone()), Some(1));
        p.choose(1);
        p.show(Some(a.clone()), Some(1));
        assert_eq!(p.framed(), Some(1), "the same showing keeps its choice");
        p.show(Some(a), Some(2));
        assert_eq!(p.framed(), None);
        p.key(1, true);
        assert_eq!(p.framed(), Some(0));
        // The hand-built box's keys move from the first row at once.
        p.set(Some(view(None, "z", &["a", "b"])));
        p.key(1, false);
        assert_eq!(p.selected, 1);
    }

    #[test]
    fn high_menu_scale_clamps_tall_confirmation_message_box_inside_safe_viewport() {
        use crate::touch_safe_area::SafeInsets;
        let v = view(
            Some("Character Rebuild"),
            "You are about to leave Goodsprings. Would you like to rebuild your character's name, appearance, S.P.E.C.I.A.L., tag skills, and traits before entering the Mojave Wasteland?",
            &["Rebuild Character", "Continue with Current Character", "Cancel"],
        );
        let l = layout(&v, &font(31.0), &font(34.0));
        let safe = SafeInsets { left: 40.0, top: 12.0, right: 40.0, bottom: 28.0 };
        let view_px = Vec2::new(960.0, 540.0);
        for scale in [1.25f32, 1.5, 2.0] {
            let (s, _) = crate::interface::fit_centered_box(scale, view_px, safe, l.width + 48.0, l.height + 48.0).expect("clamped");
            let px_per_u = (view_px.y / 960.0) * s;
            assert!(l.width * px_per_u <= view_px.x - safe.left - safe.right, "width @ {scale}");
            assert!(l.height * px_per_u <= view_px.y - safe.top - safe.bottom, "buttons fit @ {scale}");
        }
    }

    /// `bPopupMenusDontMoveCursor`: opening a popup message box leaves the OS window cursor
    /// position untouched (NVForever never warps the cursor on popup open).
    #[test]
    fn opening_a_popup_leaves_the_window_cursor_position_unchanged() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>().add_plugins(MessageMenuPlugin);
        let mut window = Window::default();
        let initial = Vec2::new(123.0, 456.0);
        window.set_cursor_position(Some(initial));
        let win_entity = app.world_mut().spawn(window).id();

        app.world_mut().resource_mut::<MessagePanel>().set(Some(view(Some("Confirm"), "Proceed?", &["No", "Yes"])));
        app.update();

        let spawned = app.world_mut().query_filtered::<Entity, With<BoxRoot>>().iter(app.world()).count();
        assert_eq!(spawned, 1, "popup box spawned");
        assert_eq!(
            app.world().get::<Window>(win_entity).unwrap().cursor_position(),
            Some(initial),
            "popup open must not warp the cursor"
        );
    }
}
