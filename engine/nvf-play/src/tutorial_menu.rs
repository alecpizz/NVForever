//! The tutorial menu (ShowTutorialMenu, docs/spec-notes/tutorial-menu.md): a message as a
//! page of the "VDSG Manual", laid out by the game's `menus\tutorial_menu.xml`
//! [documented, §2] and drawn with the menus' fonts and lines (`crate::chargen::draw`).
//!
//! - **What it shows**: the runtime's tutorial pages (`fnv_game::Game::tutorial`, opened
//!   by `Game::show_tutorial`): the message's title (MESG FULL) at the top left, its text
//!   (DESC, its `<div>` and `<p>` marks made into paragraphs, [`plain_text`]) in a
//!   scrolling column, "n/N" at the top right when there is more than one page, and
//!   Next, Previous and Close at the right.
//! - **Keys**: A or Right the next page, S or Left the previous one, E, Esc or Enter
//!   close it [documented: the file's `_PCButton_A`, `_S`, `_E`; design: the arrows,
//!   Esc and Enter]; Up, Down and the wheel scroll the text; a click on a button does as
//!   its key. Closing goes through `ScriptState::answer` (`Game::close_menu`: the menu
//!   under it, the Pip-Boy in CGTutorial's case, is the open one again). While it is open
//!   over the Pip-Boy, the Pip-Boy's keys are held (`crate::pipboy`).
//! - It is a script menu (`script::is_script_menu`): the player is held and game time
//!   stops; an unattended run closes it as any script menu (`NVF_PLAY_TEST_SCRIPT_MENUS`).
//!
//! Log lines: `tutorial menu: page <n>/<N> "<title>" (<message>)` when a page shows,
//! `tutorial menu: scroll <value> of <range>` when the hand-built drawing's scroll changes,
//! `tutorial menu: closed (<how>)`.
//!
//! With `[UI] TileMenus=TutorialMenu` the same rules work the menu drawn from its file
//! (`crate::tiles::tutorial_menu`): this module keeps the pages, the keys, the wheel and
//! [`TutorialView`]; clicks on the tiles come as [`TutorialInput`], and how far the text
//! scrolls comes from the file ([`TutorialView::set_scroll_bound`]) [design].

use crate::chargen::draw::{Align, Ctx, Kit, Which, BODY_BASE, LINE_PITCH, THICK};
use crate::interface::{amber, at, set_line, u};
use crate::bindings::Bindings;
use crate::script::ScriptState;
use crate::PlaySet;
use bevy::image::CompressedImageFormatSupport;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use crate::ui_pointer::{Button, PointerState};
use bevy::window::PrimaryWindow;
use fnv_game::game::menus;
use fnv_sim::FormId;
use nvf_assets::GameData;

// ---- the menu file's numbers [documented: menus\tutorial_menu.xml, §2] ------------------

/// The background is this part of the screen, centred.
const SCALE: f32 = 0.8;
/// The corner lines: 48 in from the sides, 96 down from the top (and up from the bottom).
const EDGE_X: f32 = 48.0;
const EDGE_Y: f32 = 96.0;
/// The short lines along the top and bottom from each corner, and the lines fading
/// down (up) from the corners (vertical_fade_line.xml).
const STUB: f32 = 50.0;
const FADE: f32 = 60.0;
/// The room between the pieces (`_buffer`).
const BUF: f32 = 10.0;
/// The title stands 5 lower than centred on the top line.
const TITLE_DROP: f32 = 5.0;
/// The buttons' right side, in from the background's right edge, and the room round
/// their text (text_box.xml `_horbuf`, `_verbuf`).
const BUTTON_RIGHT: f32 = 81.0;
const HOR_BUF: f32 = 20.0;
const VER_BUF: f32 = 20.0;
/// The text column ends this far before the buttons; its text starts 5 in and 21 down
/// (the file's "font adjustment"), wrapping 5 short of the column.
const TEXT_GAP: f32 = 64.0;
const TEXT_X: f32 = 5.0;
const TEXT_Y: f32 = 21.0;
/// A scroll step (`_step_size`).
const STEP: f32 = 30.0;
/// The VDSG symbol: 64 high, its picture's width less 36 (`_dead_space`), which it also
/// starts that much before its place.
const SYMBOL: &str = "interface\\loading\\glow_loading_vdsgsymbol.dds";
const SYMBOL_H: f32 = 64.0;
pub(crate) const SYMBOL_DEAD: f32 = 36.0;
/// The text after the symbol stands 7 lower than centred on the bottom line.
const VDSG_DROP: f32 = 7.0;
/// The PC key hint's place: 5 right of its button (text_box.xml `PCShortcutLabel`).
const HINT_GAP: f32 = 5.0;

// ---- NVForever's own -----------------------------------------------------------------------

/// The game settings the file takes its words from [documented: tutorial_menu.xml]: the
/// buttons Next, Previous and Close, their PC key hints, and the manual's words after the
/// symbol. None is in the masters; they are read by name from the game strings table
/// ([`Words`]).
const BUTTONS: [&str; 3] = ["sNext", "sPrevious", "sCloseButton"];
const MANUAL: &str = "sVDSGManual";
/// The keys behind Next, Previous and Close (the file's `_PCButton_A`, `_S`, `_E`; Esc
/// closes it too).
const HINT_KEYS: [&[KeyCode]; 3] = [&[KeyCode::KeyA], &[KeyCode::KeyS], &[KeyCode::KeyE, KeyCode::Escape]];

/// The key hints after Next, Previous and Close, in the menus' usual button style and
/// following the input device: "A)", "S)", "E)" with the keyboard, the controller's
/// buttons ("RT)", "LT)", "B)") with a controller [observed, issue #1904: the original
/// shows "Close E)" with the keyboard and "Close B)" with a controller; Next and Previous
/// in the same style: design]. The file's `&-sPCMenuHint*;` strings are in no plugin and
/// were NVForever's own "[A]" labels (§2 of docs/spec-notes/tutorial-menu.md).
pub fn hints(l: crate::pad::Labels) -> [String; 3] {
    HINT_KEYS.map(|keys| crate::pad::hint_as(keys, crate::pad::at_menu(fnv_game::game::menus::TUTORIAL), l))
}

/// The menu's words: the load order's string settings over NVForever's own labels
/// (`fnv_game::ui_loading::game_string`, `fnv_ui::strings::BUILT_IN`; menu-xml.md §2.4)
/// [design].
#[derive(Clone, Debug, PartialEq)]
pub struct Words {
    /// Next, Previous, Close.
    pub buttons: [String; 3],
    /// Their key hints ([`hints`]).
    pub hints: [String; 3],
    pub manual: String,
}

impl Words {
    /// From the running game's strings, or NVForever's labels alone without one.
    pub fn read(game: Option<&fnv_game::Game<'_>>) -> Words {
        let word = |name: &str| match game {
            Some(g) => fnv_game::ui_loading::game_string(g, name),
            None => fnv_ui::strings::built_in(name).unwrap_or_default().to_owned(),
        };
        Words { buttons: BUTTONS.map(word), hints: hints(crate::pad::labels()), manual: word(MANUAL) }
    }
}
/// The background's darkness: the menus' (`crate::interface::MENU_BACKING`) [design: the
/// file's fill alpha of 32 of 255 leaves the text hard to read over the Pip-Boy].
const SHADE: f32 = crate::interface::MENU_BACKING;
/// The picture's width when its file cannot be read.
pub(crate) const SYMBOL_W: f32 = 64.0;

/// A button of the menu.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmButton {
    Next,
    Previous,
    Close,
}

/// A page: the message's title and its text as shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub message: FormId,
    pub title: String,
    pub text: String,
}

/// The message's text without its marks: `<p>` and `<br>` start a new paragraph, other
/// tags (`<div>`) go, a paragraph's lines are joined into one [hypothesis: how the menu
/// reads HelpHealingLimbs' `<div>` and `<p>`, the only marks the tutorial messages
/// carry, observed in the masters].
pub fn plain_text(s: &str) -> String {
    let out = marks_to_breaks(s);
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in out.replace('\r', "").split('\n') {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs.join("\n\n")
}

/// The message's text with its tags gone: `<p>` and `<br>` as an empty line, the others as
/// nothing; a `<` that opens no tag stays. Its line breaks are left as they are.
pub(crate) fn marks_to_breaks(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('>') else {
            out.push_str(&rest[i..]);
            rest = "";
            break;
        };
        let tag = rest[i + 1..i + j].trim().trim_start_matches('/').to_ascii_lowercase();
        if tag == "p" || tag.starts_with("p ") || tag.starts_with("br") {
            out.push_str("\n\n");
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}

/// The pages of the open tutorial menu, none when it is not the open menu; with `keys`
/// the key strings name the keys bound now ([`pages_with`]).
pub fn pages(script: &ScriptState, keys: Option<&Bindings>) -> Vec<Page> {
    let Some(rt) = script.rt.as_ref().filter(|rt| rt.game.menu == Some(menus::TUTORIAL)) else { return Vec::new() };
    pages_of(&rt.game, keys)
}

/// The pages of the tutorial menu of `g`, as the hand-built drawing shows them.
pub(crate) fn pages_of(g: &fnv_game::Game<'_>, keys: Option<&Bindings>) -> Vec<Page> {
    pages_with(g, plain_text, keys)
}

/// The pages of the tutorial menu of `g`, each message's text read through `text`. With
/// `keys` its key strings (`&-sUActnUse;`, `&sDeviceMouse;`) name the keys or buttons
/// bound now, as the lockpick help and the notes do (`crate::lockpick::help::expand_keys`)
/// [observed: #1909, the Movement and Combat page printed the codes as they are].
pub(crate) fn pages_with(g: &fnv_game::Game<'_>, text: fn(&str) -> String, keys: Option<&Bindings>) -> Vec<Page> {
    g.tutorial
        .iter()
        .map(|&m| {
            let def = g.data.messages.get(&m);
            let body = text(def.and_then(|d| d.text.as_deref()).unwrap_or_default());
            Page {
                message: m,
                title: def.and_then(|d| d.title.clone()).unwrap_or_default(),
                text: match keys {
                    Some(k) => crate::lockpick::help::expand_keys(&body, k),
                    None => body,
                },
            }
        })
        .collect()
}

// ---- layout --------------------------------------------------------------------------------

/// Where the pieces go, from the background's top left, for a background `w` × `h`.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub w: f32,
    pub h: f32,
    /// The buttons' rectangles (left, top, width, height): Next, Previous, Close.
    pub buttons: [[f32; 4]; 3],
    /// The text column: left, top, width, height.
    pub column: [f32; 4],
}

/// The buttons' widths are their words' and the room round them; `button_h` a text
/// line's height and the room.
pub fn layout(w: f32, h: f32, widths: [f32; 3], text_h: f32) -> Layout {
    let bh = text_h + VER_BUF;
    let right = w - BUTTON_RIGHT;
    let top = EDGE_Y + FADE;
    let bottom = h - EDGE_Y - FADE;
    let next = [right - widths[0] - HOR_BUF, top, widths[0] + HOR_BUF, bh];
    let prev = [right - widths[1] - HOR_BUF, top + bh, widths[1] + HOR_BUF, bh];
    let close = [right - widths[2] - HOR_BUF, bottom - bh, widths[2] + HOR_BUF, bh];
    let x = EDGE_X + STUB + BUF;
    let column = [x, top, (prev[0] - x - TEXT_GAP).max(0.0), (bottom - top).max(0.0)];
    Layout { w, h, buttons: [next, prev, close], column }
}

/// How far the text can scroll: its height less the column's, in whole steps.
pub fn max_scroll(text_height: f32, column_h: f32) -> f32 {
    ((text_height + STEP - column_h) / STEP).ceil().max(0.0) * STEP
}

// ---- state ---------------------------------------------------------------------------------

/// The page shown, the text's scroll and the button under the pointer.
#[derive(Resource, Default)]
pub struct TutorialView {
    pub page: usize,
    pub scroll: f32,
    pub hover: Option<TmButton>,
    /// How far the shown page can scroll (set when drawn).
    max_scroll: f32,
    kit: Option<Kit>,
}

/// What the menu is asked to do, by a key, a click or a tile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Next,
    Previous,
    Close,
    /// Move the text by so many units (down the text when positive).
    Scroll(f32),
    /// Put the scroll at a value.
    ScrollTo(f32),
}

/// A command from the menu's tiles (`crate::tiles::input`), read by [`keys`] the same frame.
#[derive(Message, Clone, Copy, Debug)]
pub struct TutorialInput(pub Command);

impl TutorialView {
    /// Apply a command for `n` pages (at least one): pages turn within their bounds and a
    /// turn starts the page at its top; the scroll stays in its range. True when it asks the
    /// menu to close, which goes through `ScriptState::answer`.
    pub fn apply(&mut self, n: usize, command: Command) -> bool {
        let page = self.page.min(n.saturating_sub(1));
        let mut turn = |to: usize| {
            if to != self.page {
                self.page = to;
                self.scroll = 0.0;
            }
        };
        match command {
            Command::Next => turn((page + 1).min(n.saturating_sub(1))),
            Command::Previous => turn(page.saturating_sub(1)),
            Command::Close => return true,
            Command::Scroll(by) => self.scroll = (self.scroll + by).clamp(0.0, self.max_scroll),
            Command::ScrollTo(to) => self.scroll = to.clamp(0.0, self.max_scroll),
        }
        false
    }

    /// How far the shown page scrolls, from the menu's file when it is drawn from tiles; the
    /// scroll is held in the range (a new page, another window size).
    pub fn set_scroll_bound(&mut self, max: f32) {
        self.max_scroll = max.max(0.0);
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);
    }
}

#[derive(Component)]
struct TutorialRoot;

/// What was drawn last: the pages, the view, the screen's size in units, whether the
/// menu is drawn from its tiles (the scroll and the hover are then not this drawing's) and
/// the key hints (a change of input device draws it again).
#[derive(Default, PartialEq)]
struct Shown(Vec<Page>, usize, i32, Option<TmButton>, (i32, i32), bool, [String; 3]);

pub struct TutorialMenuPlugin;

impl Plugin for TutorialMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TutorialView>().add_message::<TutorialInput>().add_systems(Update, draw.in_set(PlaySet::Ui));
    }
}

/// Keys, the wheel and clicks of the open tutorial menu (before the frame's scripts run,
/// `crate::script`): pages, scrolling, Close through `ScriptState::answer`. A press on a
/// hand-built button always counts: they stand only while they are what is on screen, and
/// the tile input takes nothing until they have gone (`crate::tiles::input`) [design]. A key
/// or a hand-built button that asks for a turn wins the frame's turn, even at an end of the
/// pages: a tile's Next or Previous that frame is dropped, its Close and scroll still act
/// [design].
pub fn keys(
    keys: crate::pad::ActionButtons<KeyCode>,
    (mut wheel, mut tiles): (MessageReader<MouseWheel>, MessageReader<TutorialInput>),
    clicks: Query<(&PointerState, &TmButton), Changed<PointerState>>,
    mut view: ResMut<TutorialView>,
    mut script: ResMut<ScriptState>,
    (sticks, bindings, pads): (Option<Res<crate::pad::StickButtons>>, Option<Res<Bindings>>, crate::pad::ActionPads),
) {
    let keys = keys.snapshot();
    use KeyCode::*;
    let n = pages(&script, None).len();
    if n == 0 {
        wheel.clear();
        tiles.clear();
        if view.page != 0 || view.scroll != 0.0 {
            view.page = 0;
            view.scroll = 0.0;
        }
        return;
    }
    let mut hover = None;
    let mut clicked = None;
    for (i, b) in &clicks {
        match i {
            PointerState::Pressed => clicked = Some(*b),
            PointerState::Hovered => hover = Some(Some(*b)),
            PointerState::None if view.hover == Some(*b) => hover = hover.or(Some(None)),
            PointerState::None => {}
        }
    }
    if let Some(h) = hover.filter(|h| *h != view.hover) {
        view.hover = h;
    }
    let mut turned = false;
    if keys.any_just_pressed([KeyA, ArrowRight]) || clicked == Some(TmButton::Next) {
        view.apply(n, Command::Next);
        turned = true;
    } else if keys.any_just_pressed([KeyS, ArrowLeft]) || clicked == Some(TmButton::Previous) {
        view.apply(n, Command::Previous);
        turned = true;
    } else if keys.any_just_pressed([KeyE, Escape, Enter, NumpadEnter]) || clicked == Some(TmButton::Close) {
        let how = if clicked.is_some() { "click" } else { "key" };
        if script.answer(None, how) {
            info!("tutorial menu: closed ({how})");
        }
        tiles.clear();
        return;
    }
    // What the tiles asked this frame (`crate::tiles::input`), each once.
    for TutorialInput(command) in tiles.read().copied() {
        if turned && matches!(command, Command::Next | Command::Previous) {
            continue;
        }
        if view.apply(n, command) {
            if script.answer(None, "click") {
                info!("tutorial menu: closed (click)");
            }
            return;
        }
    }
    let controller = bindings.is_some_and(|b| b.device == crate::bindings::InputDevice::Controller);
    let filtered_sticks = sticks.as_ref().map(|s| pads.filter_sticks(s));
    let mut by = stick_scroll(filtered_sticks.as_ref().filter(|_| controller));
    if keys.just_pressed(ArrowDown) {
        by += STEP;
    }
    if keys.just_pressed(ArrowUp) {
        by -= STEP;
    }
    for w in wheel.read() {
        let lines = match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 40.0,
        };
        by -= lines.signum() * STEP;
    }
    if by != 0.0 {
        view.apply(n, Command::Scroll(by));
    }
}

/// How far the left stick scrolls the text this frame, with a controller working the
/// controls: a step down or up for each push past [`crate::pad::STICK_PRESS`], as the
/// D-pad's (issue #1905: in the original the left stick's Up and Down scroll the text as
/// the D-pad's do, owner's play on a controller). A held push does not repeat [design:
/// as the D-pad here].
fn stick_scroll(sticks: Option<&crate::pad::StickButtons>) -> f32 {
    use crate::pad::StickDir;
    let Some(s) = sticks else { return 0.0 };
    let mut by = 0.0;
    if s.just_pressed(StickDir::Down) {
        by += STEP;
    }
    if s.just_pressed(StickDir::Up) {
        by -= STEP;
    }
    by
}

// ---- drawing -------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    script: Res<ScriptState>,
    view: ResMut<TutorialView>,
    roots: Query<Entity, With<TutorialRoot>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    (formats, switch): (Option<Res<CompressedImageFormatSupport>>, Option<Res<crate::tiles::vanilla::TileMenus>>),
    bindings: Option<Res<Bindings>>,
    mut shown: Local<Shown>,
) {
    let pages = pages(&script, bindings.as_deref());
    // T15b: with `[UI] TileMenus=TutorialMenu` the menu is drawn from its file
    // (`crate::tiles::tutorial_menu`); only the pages' lines and a sheet are left here.
    let tiled = script.rt.as_ref().is_some_and(|rt| crate::tiles::tutorial_menu::tiled(switch.as_ref().is_some_and(|s| s.tutorial), &rt.game));
    let words = Words::read(script.rt.as_ref().map(|rt| &rt.game));
    // The screen in the interface's units: 960 high.
    let screen = windows.single().map_or(Vec2::new(1280.0, 960.0), |w| Vec2::new(960.0 * w.width() / w.height().max(1.0), 960.0));
    let page = view.page.min(pages.len().saturating_sub(1));
    let (scroll, hover) = if tiled { (0, None) } else { (view.scroll.round() as i32, view.hover) };
    let now = Shown(pages.clone(), page, scroll, hover, (screen.x.round() as i32, screen.y.round() as i32), tiled, words.hints.clone());
    if now == *shown {
        return;
    }
    if now.0 != shown.0 || now.1 != shown.1 {
        if let Some(p) = pages.get(page) {
            info!("tutorial menu: page {}/{} {:?} ({})", page + 1, pages.len(), p.title, p.message);
        }
    }
    if !tiled && !pages.is_empty() && (now.6 != shown.6 || shown.0.is_empty()) {
        let [next, previous, close] = &words.hints;
        let [n, pr, cl] = &words.buttons;
        info!("tutorial menu: hints {n} {next}, {pr} {previous}, {cl} {close}");
    }
    let scroll_moved = now.2 != shown.2;
    *shown = now;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(p) = pages.get(page) else { return };
    // Drawn from tiles, the menu keeps the pointer from what is under it with a sheet alone:
    // no click goes past it [documented: tutorial_menu.xml, `stackingtype` `&no_click_past;`].
    if crate::tiles::menus::modal(true, tiled) == crate::tiles::menus::Modal::Sheet {
        let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
        commands.spawn((full, GlobalZIndex(crate::tiles::draw::TUTORIAL_Z), TutorialRoot)).with_children(|r| {
            r.spawn(crate::tiles::menus::modal_sheet());
        });
        return;
    }
    let bc = crate::interface::bc_supported(formats.as_deref());
    let view = view.into_inner();
    let kit = view.kit.get_or_insert_with(|| Kit::load(data.as_deref(), &mut images, bc));
    let mut c = Ctx { kit, data: data.as_deref(), images: &mut images, bc, glyphs: &glyphs };
    let (w, h) = (screen.x * SCALE, screen.y * SCALE);
    let widths = [0, 1, 2].map(|k| c.width(Which::Body, &words.buttons[k]));
    let l = layout(w, h, widths, crate::chargen::draw::BODY_H);
    let lines = c.face(Which::Glow).font.lines(&p.text, Some(l.column[2] - TEXT_X)).len();
    let text_h = if lines == 0 { 0.0 } else { crate::chargen::draw::BODY_H + LINE_PITCH * (lines - 1) as f32 };
    view.max_scroll = max_scroll(text_h + TEXT_Y, l.column[3]);
    view.scroll = view.scroll.min(view.max_scroll);
    if scroll_moved {
        info!("tutorial menu: scroll {} of {}", view.scroll, view.max_scroll);
    }
    let root = Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    let (count, scroll, hover, max) = ((page + 1, pages.len()), view.scroll, view.hover, view.max_scroll);
    commands.spawn((root, GlobalZIndex(crate::tiles::draw::TUTORIAL_Z), TutorialRoot)).with_children(|r| {
        r.spawn(Node { width: u(w), height: u(h), ..default() }).with_children(|b| spawn_page(b, &mut c, &l, p, &words, count, scroll, max, hover));
    });
}

#[allow(clippy::too_many_arguments)]
fn spawn_page(b: &mut ChildSpawnerCommands, c: &mut Ctx, l: &Layout, p: &Page, words: &Words, (n, of): (usize, usize), scroll: f32, max: f32, hover: Option<TmButton>) {
    let bright = amber(255.0);
    let shade = Color::srgba(0.0, 0.0, 0.0, SHADE);
    if c.image(b, "interface\\shared\\background\\solid_black.dds", at(0.0, 0.0, l.w, l.h), shade, ()).is_none() {
        b.spawn((at(0.0, 0.0, l.w, l.h), BackgroundColor(shade)));
    }
    let (w, h) = (l.w, l.h);
    let right = w - EDGE_X;
    // The top: a corner fading down at each end, a stub, the title, a line on to the
    // page count (or to the stub at the right with one page), the stub.
    c.fade(b, EDGE_X, EDGE_Y, FADE, false, bright);
    c.fade(b, right, EDGE_Y, FADE, false, bright);
    c.line(b, EDGE_X, EDGE_Y, STUB, THICK, bright);
    c.line(b, right - STUB, EDGE_Y, STUB, THICK, bright);
    let title_top = EDGE_Y + (THICK - 38.0) / 2.0 + TITLE_DROP;
    let title_x = EDGE_X + STUB + BUF;
    let tw = c.text(b, Which::Title, &p.title, title_x, title_top, Align::Left, bright);
    let mut line_end = right - STUB - BUF;
    if of > 1 {
        let pw = c.text(b, Which::Title, &format!("{n}/{of}"), right - STUB - BUF, title_top, Align::Right, bright);
        line_end -= pw + BUF;
    }
    let line_from = title_x + tw + BUF;
    if line_end > line_from {
        c.line(b, line_from, EDGE_Y, line_end - line_from, THICK, bright);
    }
    // The bottom: the corners fading up, a stub, the VDSG symbol and words, a line on
    // to the right corner.
    let low = h - EDGE_Y;
    c.fade(b, EDGE_X, low - FADE, FADE, true, bright);
    c.fade(b, right, low - FADE, FADE, true, bright);
    c.line(b, EDGE_X, low - THICK, STUB, THICK, bright);
    let sx = EDGE_X + STUB + BUF - SYMBOL_DEAD;
    let sw = c.picture(SYMBOL).and_then(|(_, rect)| rect.map(|r| r.width())).unwrap_or(SYMBOL_W + SYMBOL_DEAD) - SYMBOL_DEAD;
    let sy = low - THICK + (THICK - SYMBOL_H) / 2.0;
    c.image(b, SYMBOL, at(sx, sy, sw.max(1.0), SYMBOL_H), bright, ());
    let vx = sx + sw + BUF;
    let vw = c.text(b, Which::Body, &words.manual, vx, low - THICK + (THICK - crate::chargen::draw::BODY_H) / 2.0 + VDSG_DROP, Align::Left, bright);
    let from = vx + vw + BUF;
    if right > from {
        c.line(b, from, low - THICK, right - from, THICK, bright);
    }
    // The text, clipped to its column and moved up by the scroll.
    let [cx, cy, cw, ch] = l.column;
    b.spawn(Node { overflow: Overflow::clip(), ..at(cx, cy, cw, ch) }).with_children(|col| {
        let face = c.face(Which::Glow);
        for (k, line) in face.font.lines(&p.text, Some(cw - TEXT_X)).iter().enumerate() {
            set_line(col, face, line, TEXT_X, TEXT_Y - scroll + BODY_BASE + LINE_PITCH * k as f32, bright);
        }
    });
    // A scroll mark beside the column while the text is longer than it [design: the
    // file's scroll bar is a prefab not drawn here].
    if max > 0.0 {
        let track = ch - THICK;
        let thumb = (track * ch / (ch + max)).max(20.0);
        let y = cy + (track - thumb) * scroll / max;
        c.line(b, EDGE_X, cy, THICK, ch, amber(128.0));
        c.line(b, EDGE_X - 2.0, y, THICK + 4.0, thumb, bright);
    }
    // The buttons: their words right-aligned in the box, the key hint after it; the box
    // shows under the pointer (text_box.xml).
    let buttons = [TmButton::Next, TmButton::Previous, TmButton::Close];
    for (k, which) in buttons.into_iter().enumerate() {
        let [x, y, bw, bh] = l.buttons[k];
        let usable = match which {
            TmButton::Next => n < of,
            TmButton::Previous => n > 1,
            TmButton::Close => true,
        };
        let color = amber(255.0).with_alpha(if usable { 1.0 } else { 0.5 });
        if hover == Some(which) && usable {
            c.highlight(b, x, y, bw, bh, 1.0);
        }
        let top = y + VER_BUF / 2.0;
        c.text(b, Which::Body, &words.buttons[k], x + bw - HOR_BUF / 2.0, top, Align::Right, color);
        c.text(b, Which::Body, &words.hints[k], x + bw + HINT_GAP, top, Align::Left, color);
        b.spawn((Button, which, at(x, y, bw, bh)));
    }
}

#[cfg(feature = "report-test-support")]
impl TutorialView {
    /// [design, #2403] Synthetic page/scroll/hover for detector regressions.
    #[doc(hidden)]
    pub fn report_progress_fixture(step: usize) -> Self {
        Self { page: usize::from(step >= 1), scroll: if step >= 2 { 20.0 } else { 0.0 }, hover: (step >= 3).then_some(TmButton::Close), ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #1905: the left stick's push down or up scrolls a step, once per push.
    #[test]
    fn the_left_stick_scrolls_a_step_a_push() {
        use crate::pad::{StickButtons, StickDir};
        let mut s = StickButtons::default();
        assert_eq!(stick_scroll(Some(&s)), 0.0);
        s.now[StickDir::Down.index()] = true;
        assert_eq!(stick_scroll(Some(&s)), STEP);
        s.before = s.now;
        assert_eq!(stick_scroll(Some(&s)), 0.0, "a held push does not repeat");
        s = StickButtons::default();
        s.now[StickDir::Up.index()] = true;
        assert_eq!(stick_scroll(Some(&s)), -STEP);
        assert_eq!(stick_scroll(None), 0.0, "no controller at work");
    }

    /// Issue #1904: the hints are in the usual button style and name the controller's
    /// buttons with a controller (RT Next, LT Previous, B Close).
    #[test]
    fn hints_follow_the_input_device() {
        use crate::pad::Labels;
        assert_eq!(hints(Labels::Keys), ["[A]", "[S]", "[E]"]);
        assert_eq!(hints(Labels::Xbox), ["[RT]", "[LT]", "[B]"]);
        assert_eq!(hints(Labels::PlayStation), ["[R2]", "[L2]", "[Circle]"]);
    }

    #[test]
    fn the_marks_become_paragraphs() {
        let s = "<div>\r\nIn order to heal a crippled limb, you have three choices.\r\n<p>\r\nStimpaks can be used\r\nfor general healing.\r\n<p>\r\nTo heal a limb directly.";
        assert_eq!(plain_text(s), "In order to heal a crippled limb, you have three choices.\n\nStimpaks can be used for general healing.\n\nTo heal a limb directly.");
        assert_eq!(plain_text("No marks"), "No marks");
        assert_eq!(plain_text("a<br>b </div>"), "a\n\nb");
        assert_eq!(plain_text("a < b"), "a < b");
        assert_eq!(plain_text(""), "");
    }

    #[test]
    fn the_layout_follows_the_file() {
        // A 1280 × 960 screen: the background is 1024 × 768.
        let l = layout(1024.0, 768.0, [40.0, 80.0, 50.0], 31.0);
        // Next at the column's top, its right side 81 in; Previous under it.
        assert_eq!(l.buttons[0], [1024.0 - 81.0 - 60.0, 156.0, 60.0, 51.0]);
        assert_eq!(l.buttons[1], [1024.0 - 81.0 - 100.0, 207.0, 100.0, 51.0]);
        // Close stands on the lower corners' fade lines' top.
        assert_eq!(l.buttons[2][1] + l.buttons[2][3], 768.0 - 96.0 - 60.0);
        // The column: after the title's stub, down to the lower corners' lines, 64 short
        // of Previous.
        assert_eq!(l.column, [108.0, 156.0, 1024.0 - 81.0 - 100.0 - 64.0 - 108.0, 768.0 - 312.0]);
    }

    #[test]
    fn the_commands_keep_pages_and_scroll_in_their_bounds() {
        let mut v = TutorialView::default();
        v.set_scroll_bound(100.0);
        assert!(!v.apply(2, Command::Previous));
        assert_eq!(v.page, 0, "no wrapping back");
        assert!(!v.apply(2, Command::Scroll(30.0)));
        assert_eq!(v.scroll, 30.0);
        assert!(!v.apply(2, Command::Scroll(-50.0)));
        assert_eq!(v.scroll, 0.0);
        assert!(!v.apply(2, Command::ScrollTo(70.0)));
        assert!(!v.apply(2, Command::Next));
        assert_eq!((v.page, v.scroll), (1, 0.0), "a turn starts the page at its top");
        assert!(!v.apply(2, Command::ScrollTo(500.0)));
        assert_eq!(v.scroll, 100.0);
        assert!(!v.apply(2, Command::Next));
        assert_eq!((v.page, v.scroll), (1, 100.0), "no wrapping on; the page and its scroll stay");
        v.set_scroll_bound(40.0);
        assert_eq!(v.scroll, 40.0, "a smaller range holds the scroll in it");
        assert!(v.apply(2, Command::Close));
    }

    /// One frame of [`keys`] with three pages open at `from`, a range of 100: what the frame
    /// pressed and what the tiles asked. The page, the scroll and whether the menu was answered.
    fn one_frame(from: usize, key: Option<KeyCode>, button: Option<TmButton>, tiles: &[Command]) -> (usize, f32, bool) {
        let mut state = ScriptState::over_no_plugins();
        let game = &mut state.rt.as_mut().unwrap().game;
        game.menu = Some(menus::TUTORIAL);
        game.tutorial = vec![FormId(0x11), FormId(0x12), FormId(0x13)];
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<TutorialView>()
            .insert_resource(state)
            .add_message::<MouseWheel>()
            .add_message::<TutorialInput>()
            .add_systems(Update, keys);
        let mut view = app.world_mut().resource_mut::<TutorialView>();
        view.page = from;
        view.set_scroll_bound(100.0);
        if let Some(key) = key {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        }
        if let Some(button) = button {
            app.world_mut().spawn((button, PointerState::Pressed));
        }
        for &command in tiles {
            app.world_mut().write_message(TutorialInput(command));
        }
        app.update();
        let view = app.world().resource::<TutorialView>();
        (view.page, view.scroll, app.world().resource::<ScriptState>().answered())
    }

    #[test]
    fn a_turn_by_key_or_hand_built_button_takes_the_frames_tile_turns() {
        use Command::{Next, Previous};
        use KeyCode::{KeyA, KeyS};
        let page = |from, key, button, tiles: &[Command]| one_frame(from, key, button, tiles).0;
        // The same way: one page, not two.
        assert_eq!(page(0, Some(KeyA), None, &[Next]), 1);
        assert_eq!(page(2, Some(KeyS), None, &[Previous]), 1);
        assert_eq!(page(0, None, Some(TmButton::Next), &[Next]), 1);
        assert_eq!(page(2, None, Some(TmButton::Previous), &[Previous]), 1);
        // Opposite ways: the key's, not back where it started.
        assert_eq!(page(1, Some(KeyA), None, &[Previous]), 2);
        assert_eq!(page(1, Some(KeyS), None, &[Next]), 0);
        assert_eq!(page(1, None, Some(TmButton::Next), &[Previous]), 2);
        // At an end the key turns nothing and still wins.
        assert_eq!(page(2, Some(KeyA), None, &[Previous]), 2);
        assert_eq!(page(0, Some(KeyS), None, &[Next]), 0);
        // With no such key or button the tiles' turns act, each once.
        assert_eq!(page(0, None, None, &[Next]), 1);
        assert_eq!(page(2, None, None, &[Previous]), 1);
        assert_eq!(page(0, None, None, &[Next, Next]), 2);
    }

    #[test]
    fn a_turn_by_key_leaves_the_tiles_close_and_scroll() {
        use KeyCode::KeyA;
        assert_eq!(one_frame(0, Some(KeyA), None, &[Command::Scroll(40.0)]), (1, 40.0, false));
        assert_eq!(one_frame(0, Some(KeyA), None, &[Command::Next, Command::ScrollTo(60.0)]), (1, 60.0, false));
        assert_eq!(one_frame(0, Some(KeyA), None, &[Command::Previous, Command::Close]), (1, 0.0, true));
    }

    #[test]
    fn the_text_scrolls_by_whole_steps() {
        assert_eq!(max_scroll(100.0, 456.0), 0.0);
        assert_eq!(max_scroll(456.0, 456.0), 30.0);
        assert_eq!(max_scroll(500.0, 456.0), 90.0);
    }

    /// A tutorial page's key strings name the keys bound now, in the hand-built and the
    /// tiled menu's text alike [#1909]. A plugin with nothing but its header and the
    /// player; the message is put in by hand (synthetic text, charter).
    #[test]
    fn a_tutorial_page_names_the_keys_bound() {
        use fnv_esm::raw::{write_subrecords, Subrecord};
        use fnv_esm::{LoadOrder, Plugin, Schema};
        use fnv_game::{Game, GameConfig, GameData};
        use fnv_sim::{FormStore, RefTable, SimConfig, World};
        use fnv_sim::FormId;

        let record = |sig: &[u8; 4], fid: u32, subs: &[Subrecord]| {
            let payload = write_subrecords(subs);
            [&sig[..], &(payload.len() as u32).to_le_bytes(), &0u32.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
        };
        let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
        let player = record(b"NPC_", 0x7, &[Subrecord { sig: *b"EDID", data: b"Player ".to_vec(), extended: false }]);
        let bytes = [
            record(b"TES4", 0, &[Subrecord { sig: *b"HEDR", data: hedr, extended: false }]),
            [&b"GRUP"[..], &((24 + player.len()) as u32).to_le_bytes(), b"NPC_", &0i32.to_le_bytes(), &[0; 8], &player].concat(),
        ]
        .concat();
        let plugins = vec![Plugin::parse(&bytes, "Tutorial.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let schema = Schema::load_dir(root.join("fnv")).unwrap();
        let functions = root.join("script/functions.json");
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&functions).unwrap()).unwrap();
        let (forms, refs) = (FormStore::build(&lo, &schema), RefTable::build(&lo, &schema));
        let mut data = GameData::build(&lo, &schema, fnv_script::FunctionTable::load(&functions).unwrap(), &json, &refs);
        let page = FormId(0x900);
        let text = "nvf go:

&-sUActnUse;: nvf hit &-sUActnBlock;: nvf hold
&sDeviceMouse;: nvf look";
        data.messages.insert(page, fnv_game::data::MessageDef { title: Some("nvf page".into()), text: Some(text.into()), ..Default::default() });
        let mut g = Game::new(&data, World::new(&forms, &refs, SimConfig::default()), GameConfig::default());
        g.tutorial.push(page);

        let keys = Bindings::game_defaults();
        let shown = pages_of(&g, Some(&keys));
        assert_eq!(shown.len(), 1);
        assert!(shown[0].text.contains("Left Mouse: nvf hit Left Alt: nvf hold"), "{}", shown[0].text);
        assert!(shown[0].text.contains("Mouse: nvf look"), "{}", shown[0].text);
        assert!(!shown[0].text.contains('&'), "{}", shown[0].text);
        let tiled = pages_with(&g, crate::tiles::tutorial_menu::body_text, Some(&keys));
        assert!(tiled[0].text.contains("Left Mouse: nvf hit") && !tiled[0].text.contains('&'), "{}", tiled[0].text);
        // Block names its first key (the defaults: Left Alt, Right Mouse), as the lockpick
        // help does. Without bindings (none loaded yet) the text is as it was.
        assert!(pages_of(&g, None)[0].text.contains("&-sUActnUse;"));
    }
}
