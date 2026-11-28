//! Terminals (docs/spec-notes/hacking.md): activating one opens its screen. A locked
//! terminal first needs hacking: the word game of `fnv_game::hacking`, drawn as the
//! original's (`screen`). The right word opens the terminal's menu; four wrong words
//! lock it for good; leaving the game before then and coming back gives a new game
//! with the attempts back [the owner, 2026-10-11]. A terminal that is unlocked, hacked
//! before, or whose password note the player holds opens at its menu
//! (`fnv_game::terminal::opening`). Winning the hacking game is the lock picking's
//! crime when someone else owns the terminal (`crate::lockpick::LockOpened`). The menu
//! lists the items whose conditions hold; choosing one runs its result script, shows its
//! result text at the bottom, shows its note or opens its sub menu.
//!
//! The screen is drawn into a picture ([`TerminalImage`]) that the engine puts on the
//! screen of the game's monitor model (nvforever `terminal_view.rs`), as the original
//! does; the monitor is drawn over the darkened world, and a click on its power button
//! leaves the terminal. Where the model is not drawn the picture is shown flat in the
//! middle of the window instead (hacking.md §7).
//!
//! It is a `PlayState::Menu`: the world and the game clock stand still and the cursor is
//! free. To scripts it is menu 1055 while hacking and 1057 in the menu [documented: GECK
//! wiki GetMenuTargetRef, "the terminal being hacked", "the terminal being used"].
//!
//! Keys (hacking.md §7): in the hacking game the arrows (or W A S D) move the cursor a
//! character or a row, over a word at once; Enter or E enters what is under it; the
//! pointer moves it and a click enters. The cursor follows the pointer at once, but its
//! hover sound (`UiEvent::Focus`) is asked for at most once every [`HOVER_SOUND_GAP`] of
//! real time (a sound whose file is still being read can start later); the moves in
//! between are silent and their sounds are dropped, not played later (#2216) [design]. A key's move always sounds. In the menu Up / Down choose, Enter or E
//! (or a click) takes the item. Tab or Backspace go back: from a note to the list, from a
//! sub menu to its parent, else out of the terminal; Esc leaves at once. [hypothesis: the
//! original's PC keys beyond Tab are not captured]
//!
//! A terminal whose model has "Open" and "Close" sequences (the wall terminal
//! `Terminals\Terminal01.NIF`: the keyboard slides out, then the screen; its "Open" is
//! 0.5 s with the key `Sound: OBJComputerTerminalForward` [observed: `fnvtool nif
//! decode`]) is drawn and timed as an animated container (`crate::container_anim`): a
//! request to open it first plays "Open", the screen comes up when it has ended, and
//! "Close" plays when the screen is left or the terminal refuses [observed: the owner,
//! #1532, the original plays the opening before its menu; hypothesis: the close as the
//! open]. Log: `terminal 0x… plays Open before its screen`, `terminal 0x… closes`.
//!
//! Test aid `NVF_PLAY_TEST_TERMINAL=<seconds>:<step>[,…]`: `open:<reference>` opens a
//! terminal as if the player had activated it, `science:<n>` makes the player's Science
//! that for terminals, `wrong` enters a word that is not the password, `right` the
//! password, `brackets` the first bracket pair left, `symbol` a lone symbol,
//! `item:<n>` takes the n-th listed item (from 0), `back` and `exit` as Tab and Esc,
//! `power` clicks the monitor's power button, `point:<index>` puts the hacking cursor on
//! dump character `index` as the pointer does (`TerminalInput::Point`).
//! At exit, when a terminal was opened: "terminal summary: …"; when the pointer moved the
//! hacking cursor: "terminal hover: P changed points, F focus sounds, S suppressed" (the
//! pointer's moves onto another character, or showing it, and how many of them sounded).

pub mod screen;
pub mod touch;

use crate::interface::bc_supported;
use crate::pause::{AidTime, Paused};
use crate::script::{with_world, ScriptState};
use crate::sim::SimState;
use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
use crate::{PlaySet, PlayState};
use bevy::asset::RenderAssetUsages;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::PrimaryWindow;
use fnv_game::hacking::{Dictionary, Outcome, Puzzle, State as GameState, Target, ROWS, ROW_CHARS};
use fnv_game::terminal::{self as rules, Opening};
use fnv_sim::FormId;
use nvf_assets::GameData;
use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;
use std::time::Duration;

/// The menu codes scripts see [documented: GECK wiki GetMenuTargetRef].
pub const MENU_HACKING: u16 = 1055;
pub const MENU_TERMINAL: u16 = 1057;

/// After the password, the wait message (`hacking::words::WAIT`) stays this long before
/// the menu [design]; after the last wrong word, the log this long before the lockout
/// screen [design].
pub const GRANTED_SECONDS: f32 = 2.0;
pub const LOCKED_SECONDS: f32 = 1.5;

/// The pointer's hover sound in the hacking game is asked for at most this often (real time):
/// the hover record's file is a click 66.6 ms long (issue #2216), so this is that rounded
/// up, whatever the frame rate [design]. Keys and the menu's rows are not held to it.
pub const HOVER_SOUND_GAP: Duration = Duration::from_millis(70);

/// The game's word list [observed: `Fallout - Misc.bsa`].
pub const DICTIONARY: &str = "menus\\falloutdict.txt";

/// The longest the screen waits for a terminal's "Open" sequence (seconds) [design:
/// the wall terminal's is 0.5 s].
const OPEN_WAIT: f32 = 2.0;

/// The animated terminal playing "Open" before its screen.
#[derive(Resource, Default, Debug)]
pub struct TerminalOpening {
    /// Playing "Open": (reference, seconds waited).
    pending: Option<(u32, f32)>,
    /// Its "Open" ended (or was waited for long enough): the next request for it opens
    /// the screen without holding again.
    ready: Option<u32>,
}

impl TerminalOpening {
    /// Called with a request to open `reference`: true when it is held while its model
    /// plays "Open" (a terminal not drawn, or without the sequences, is not held).
    fn hold(&mut self, doors: Option<&mut crate::doors::Doors>, reference: u32) -> bool {
        if self.ready.take() == Some(reference) {
            return false;
        }
        let Some(d) = doors.filter(|d| d.is_open(reference) == Some(false)) else { return false };
        d.set_open(reference, true);
        if self.pending.is_none_or(|(r, _)| r != reference) {
            info!("terminal 0x{reference:08X} plays Open before its screen");
            self.pending = Some((reference, 0.0));
        }
        true
    }

    /// Advance by `dt`: the reference whose screen is now to be asked for again.
    fn advance(&mut self, doors: Option<&crate::doors::Doors>, dt: f32) -> Option<u32> {
        let (r, waited) = self.pending.as_mut()?;
        *waited += dt;
        let r = *r;
        let opened = doors.is_none_or(|d| d.is_open(r) != Some(false));
        if !opened && *waited <= OPEN_WAIT {
            return None;
        }
        self.pending = None;
        self.ready = Some(r);
        Some(r)
    }
}

/// Open the terminal reference (a placed TERM) for the player.
#[derive(Message, Clone, Copy, Debug)]
pub struct OpenTerminal {
    pub reference: u32,
}

/// The picture the screen is drawn into, one pixel to a unit of the interface: the
/// canvas is its top left [`screen::CANVAS`] (as the Pip-Boy's, `crate::pipboy`).
#[derive(Resource)]
pub struct TerminalImage {
    pub image: Handle<Image>,
    pub camera: Entity,
}

/// The picture's size, pixels.
pub const TARGET: UVec2 = UVec2::new(1280, 960);

/// The monitor as the engine draws it (nvforever `terminal_view.rs`): a picture the size
/// of the window, clear where the model is not.
#[derive(Resource)]
pub struct MonitorView {
    pub image: Handle<Image>,
}

/// Where the window's pointer lies on the screen's canvas (interface units from its top
/// left), if on it: set by the engine from the monitor's screen, or here from the flat
/// picture.
#[derive(Resource, Default)]
pub struct TerminalPointer {
    pub canvas: Option<Vec2>,
}

/// Test aid `power`: the engine clicks the monitor's power button.
#[derive(Resource, Default)]
pub struct PowerAid(pub bool);

/// The menu's rows on the canvas as last drawn, for the pointer, and an open note's window
/// (for paging it).
#[derive(Resource, Default)]
struct MenuRows(screen::Drawn);

/// The game's word list, read once.
#[derive(Resource, Default)]
pub struct Words(pub Dictionary);

/// The open terminal, if any.
#[derive(Resource, Default)]
pub struct TerminalScreen(pub Option<Session>);

/// [design, #2403] Synthetic hacking states for reports integration tests only.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> TerminalScreen {
    let dict = Dictionary::parse("AAAA\nAAAB\nAABA\nABAA\nBAAA\n");
    let mut rng = ChaCha8Rng::seed_from_u64(42);
    let mut h = Hacking::new(Puzzle::new(&dict, fnv_game::hacking::Difficulty::VeryEasy, 2, &mut rng).unwrap(), 0);
    if step >= 1 {
        h.cursor = 1;
    }
    if step >= 2 {
        h.puzzle.attempts -= 1;
        h.puzzle.log.push("Synthetic attempt".into());
    }
    if step >= 3 {
        h.ended = Some(step as f32);
        h.last_hover_focus = Some(Duration::from_secs(step as u64));
    }
    TerminalScreen(Some(Session { reference: FormId(41), base: FormId(42), page: Page::Hacking(h) }))
}

pub struct Session {
    pub reference: FormId,
    pub base: FormId,
    pub page: Page,
}

pub enum Page {
    Hacking(Hacking),
    /// The lockout screen's two lines, read from the game strings table when it opens.
    LockedOut([String; 2]),
    Menu(MenuView),
}

pub struct Hacking {
    pub puzzle: Puzzle,
    /// The dump character the cursor is on.
    pub cursor: usize,
    /// Nothing is lit: at the start and while the pointer is off the dump [observed: the
    /// owner's first screenshot, the pointer beside the screen and only the prompt].
    pub hidden: bool,
    /// When the game ended (aid clock seconds): won or locked out.
    pub ended: Option<f32>,
    /// When the pointer's last hover sound started (real time, `Time<Real>`): the next
    /// waits [`HOVER_SOUND_GAP`]. The pointer leaving the dump does not clear it, so going
    /// off and on again does not skip the wait; a new game starts without one.
    pub last_hover_focus: Option<Duration>,
}

impl Hacking {
    pub fn new(puzzle: Puzzle, cursor: usize) -> Self {
        Hacking { puzzle, cursor, hidden: true, ended: None, last_hover_focus: None }
    }
}

/// What an input did to the hacking game's cursor ([`cursor_step`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CursorStep {
    /// Enter the dump character at this index.
    Pick(usize),
    /// Nothing entered; the cursor's focus sound.
    Moved(Focus),
}

/// The focus sound of a cursor input that entered nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    /// The cursor stayed where it was, or went out of sight: no sound.
    Silent,
    /// It moved onto another character, or showed again: the sound.
    Sound,
    /// The pointer moved it within [`HOVER_SOUND_GAP`] of the last hover sound: no sound,
    /// and none later.
    Suppressed,
}

/// Apply a cursor input to the hacking game at real time `now`: where the cursor goes,
/// what is to be entered, and whether its move sounds. A pointer's move (`Point`) sounds
/// at most once every [`HOVER_SOUND_GAP`] (#2216) [design]; a key's (`Step`) always.
fn cursor_step(h: &mut Hacking, cmd: TerminalInput, now: Duration) -> CursorStep {
    let (cursor_was, hidden_was) = (h.cursor, h.hidden);
    let pick = match cmd {
        TerminalInput::Point(i) => {
            (h.cursor, h.hidden) = (i, false);
            None
        }
        TerminalInput::Unpoint => {
            h.hidden = true;
            None
        }
        // The first key shows the cursor where it is.
        TerminalInput::Step(dx, dy) => {
            if !h.hidden {
                h.cursor = step_cursor(&h.puzzle, h.cursor, dx, dy);
            }
            h.hidden = false;
            None
        }
        TerminalInput::Select if h.hidden => None,
        TerminalInput::Select => Some(h.cursor),
        TerminalInput::Aid(a) => aid_pick(&h.puzzle, a),
        _ => None,
    };
    if let Some(at) = pick {
        (h.cursor, h.hidden) = (at, false);
        return CursorStep::Pick(at);
    }
    // Sound only when the cursor moved onto another character (or showed again).
    if h.hidden || (h.cursor == cursor_was && !hidden_was) {
        return CursorStep::Moved(Focus::Silent);
    }
    if !matches!(cmd, TerminalInput::Point(_)) {
        return CursorStep::Moved(Focus::Sound);
    }
    if h.last_hover_focus.is_some_and(|last| now.saturating_sub(last) < HOVER_SOUND_GAP) {
        return CursorStep::Moved(Focus::Suppressed);
    }
    h.last_hover_focus = Some(now);
    CursorStep::Moved(Focus::Sound)
}

pub struct MenuView {
    /// The terminal whose items are listed (the placed one's base, or a sub menu).
    pub terminal: FormId,
    /// The terminals above, with the row chosen in each.
    pub parents: Vec<(FormId, usize)>,
    /// Indices of the items listed.
    pub items: Vec<usize>,
    /// Their lines (ITXT, with a script's changes, `fnv_game::terminal::item`), in the
    /// same order.
    pub labels: Vec<String>,
    pub selected: usize,
    /// No row is lit until the pointer or a key comes to the list [observed: the owner's
    /// screenshot of a terminal's menu, the pointer beside the screen].
    pub hidden: bool,
    /// The last chosen item's result text.
    pub result: String,
    /// The note shown instead of the list, and its first line shown.
    pub note: Option<FormId>,
    pub scroll: usize,
    /// The top terminal's server and welcome text (a sub menu keeps them).
    pub server: u8,
    pub welcome: String,
}

/// The terminal entries whose note the player has read, by terminal (FormID) and item
/// index, under the Grey Read Notes tweak (an entry read with it off is not counted);
/// the menu draws them dimmer while the tweak is on. Kept with the save
/// ([`READ_ENTRIES_SAVE_HOOK`]) [design: docs/design/tweaks.md].
#[derive(Resource, Default, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReadEntries(pub std::collections::BTreeSet<(u32, u32)>);

impl ReadEntries {
    pub fn has(&self, terminal: FormId, item: usize) -> bool {
        self.0.contains(&(terminal.0, item as u32))
    }
}

/// The player's Grey Read Notes tweak, set by `crate::tweaks` each frame.
static GREY_READ: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Switch the Grey Read Notes tweak on or off.
pub fn set_grey_read(on: bool) {
    GREY_READ.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn grey_read() -> bool {
    GREY_READ.load(std::sync::atomic::Ordering::Relaxed)
}

/// [`ReadEntries`] in the save; the terminals moved to the current load order, those of
/// a plugin no longer loaded dropped.
pub const READ_ENTRIES_SAVE_HOOK: crate::save::SaveHook = crate::save::SaveHook {
    key: "terminal_read_entries",
    save: |w| w.get_resource::<ReadEntries>().filter(|r| !r.0.is_empty()).and_then(|r| serde_json::to_value(r).ok()),
    load: |w, v| {
        if let (Ok(r), Some(mut read)) = (serde_json::from_value::<ReadEntries>(v.clone()), w.get_resource_mut::<ReadEntries>()) {
            *read = r;
        }
    },
    remap: Some(|v, r| {
        if let Some(list) = v.as_array_mut() {
            list.retain_mut(|e| match e.get(0).and_then(serde_json::Value::as_u64).and_then(|n| r.map(n as u32)) {
                Some(n) => {
                    e[0] = serde_json::Value::from(n);
                    true
                }
                None => false,
            });
        }
    }),
};

/// A command for the open terminal.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum TerminalInput {
    /// Put the cursor on a dump character (the pointer).
    Point(usize),
    /// The pointer left the dump.
    Unpoint,
    /// Move the cursor: characters across, rows down.
    Step(i32, i32),
    /// Enter / take what the cursor is on.
    Select,
    /// Choose a menu row (the pointer).
    Row(usize),
    Back,
    Exit,
    /// Test aid: enter a word or a pair of this kind.
    Aid(AidPick),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AidPick {
    Wrong,
    Right,
    Brackets,
    Symbol,
    Item(usize),
}

#[derive(Resource, Default, Debug)]
struct TerminalStats {
    opened: u32,
    hacks: u32,
    hacked: u32,
    locked_out: u32,
    refused: u32,
    guesses: u32,
    pairs: u32,
    duds: u32,
    refills: u32,
    items: u32,
    /// Result scripts that called ForceTerminalBack.
    forced_back: u32,
    /// The pointer's moves of the hacking cursor onto another character (or showing it),
    /// and of those, the ones that sounded and the ones held back by [`HOVER_SOUND_GAP`].
    hover_points: u32,
    hover_sounds: u32,
    hover_suppressed: u32,
}

impl TerminalStats {
    /// Counts a cursor input's hover sound for the exit line: only the pointer's moves
    /// onto another character (or showing it) count, each as sounded or held back.
    fn count_hover(&mut self, cmd: TerminalInput, focus: Focus) {
        if !matches!(cmd, TerminalInput::Point(_)) || focus == Focus::Silent {
            return;
        }
        self.hover_points += 1;
        if focus == Focus::Sound {
            self.hover_sounds += 1;
        } else {
            self.hover_suppressed += 1;
        }
    }
}

/// `NVF_PLAY_TEST_TERMINAL`.
#[derive(Resource, Default)]
struct TerminalAid {
    steps: Vec<(f32, AidStep)>,
    science: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AidStep {
    Open(u32),
    Science(f32),
    Input(TerminalInput),
    Power,
}

fn parse_aid(v: &str) -> Result<Vec<(f32, AidStep)>, String> {
    crate::test_aid::timed(v)?
        .into_iter()
        .map(|(t, what)| {
            let (op, arg) = what.split_once(':').unwrap_or((what, ""));
            let step = match op {
                "open" => AidStep::Open(u32::from_str_radix(arg.trim().trim_start_matches("0x"), 16).map_err(|_| format!("{what:?}: expected open:<reference hex>"))?),
                "science" => AidStep::Science(arg.trim().parse().map_err(|_| format!("{what:?}: expected science:<n>"))?),
                "wrong" => AidStep::Input(TerminalInput::Aid(AidPick::Wrong)),
                "right" => AidStep::Input(TerminalInput::Aid(AidPick::Right)),
                "brackets" => AidStep::Input(TerminalInput::Aid(AidPick::Brackets)),
                "symbol" => AidStep::Input(TerminalInput::Aid(AidPick::Symbol)),
                "item" => AidStep::Input(TerminalInput::Aid(AidPick::Item(arg.trim().parse().map_err(|_| format!("{what:?}: expected item:<n>"))?))),
                "back" => AidStep::Input(TerminalInput::Back),
                "select" => AidStep::Input(TerminalInput::Select),
                "exit" => AidStep::Input(TerminalInput::Exit),
                "power" => AidStep::Power,
                "point" => AidStep::Input(TerminalInput::Point(
                    arg.trim()
                        .parse()
                        .ok()
                        .filter(|&i| i < fnv_game::hacking::DUMP_CHARS)
                        .ok_or_else(|| format!("{what:?}: expected point:<index below {}>", fnv_game::hacking::DUMP_CHARS))?,
                )),
                _ => return Err(format!("{what:?}: expected open:, science:, wrong, right, brackets, symbol, item:, back, exit, power or point:")),
            };
            Ok((t, step))
        })
        .collect()
}

pub struct TerminalPlugin;

impl Plugin for TerminalPlugin {
    fn build(&self, app: &mut App) {
        let steps = match std::env::var("NVF_PLAY_TEST_TERMINAL").map(|v| parse_aid(&v)) {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                warn!("NVF_PLAY_TEST_TERMINAL: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        crate::save::add_save_hook(app, READ_ENTRIES_SAVE_HOOK);
        app.init_resource::<TerminalScreen>()
            .init_resource::<TerminalStats>()
            .init_resource::<Words>()
            .init_resource::<TerminalPointer>()
            .init_resource::<PowerAid>()
            .init_resource::<MenuRows>()
            .init_resource::<TerminalOpening>()
            .init_resource::<touch::Settings>()
            .init_resource::<touch::TouchState>()
            .init_resource::<ReadEntries>()
            .insert_resource(TerminalAid { steps, science: None })
            .add_message::<OpenTerminal>()
            .add_message::<TerminalInput>()
            .add_systems(Startup, (read_words, setup_image, touch::load))
            .add_systems(
                Update,
                (run_aid, follow_open, open, backdrop, touch::input, (keys, mouse, apply, timers).chain().run_if(in_state(PlayState::Menu)), draw)
                    .chain()
                    .after(crate::menu::open_menus)
                    .in_set(PlaySet::Ui),
            )
            .add_systems(Update, touch::draw.after(draw).in_set(PlaySet::Ui))
            .add_systems(OnExit(PlayState::Menu), close)
            .add_systems(Update, fade_out.in_set(PlaySet::Ui))
            .add_systems(Last, report);
        #[cfg(any(target_os = "android", test))]
        touch::register(app);
    }
}

fn read_words(data: Option<Res<GameData>>, mut words: ResMut<Words>) {
    let text = data.as_deref().and_then(|d| d.read(DICTIONARY));
    match text {
        Some(bytes) => {
            // UTF-8 when it is, else 8-bit text in this run's code page (#1439).
            let text = std::str::from_utf8(&bytes).map_or_else(|_| fnv_util::text::decode(&bytes), str::to_owned);
            words.0 = Dictionary::parse(&text);
            info!("terminal: {} words in {DICTIONARY}", words.0.len());
        }
        None => warn!("terminal: {DICTIONARY} not found; terminals cannot be hacked"),
    }
}

fn setup_image(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let size = Extent3d { width: TARGET.x, height: TARGET.y, ..default() };
    let [r, g, b, _] = screen::screen_color().to_srgba().to_u8_array();
    let mut image = Image::new_fill(size, TextureDimension::D2, &[b, g, r, 255], TextureFormat::Bgra8UnormSrgb, RenderAssetUsages::default());
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = images.add(image);
    let camera = commands
        .spawn((
            Name::new("Terminal screen camera"),
            Camera2d,
            // Off the canvas the picture is the screen's colour.
            Camera { order: -2, is_active: false, clear_color: ClearColorConfig::Custom(screen::screen_color()), ..default() },
            RenderTarget::Image(ImageRenderTarget::from(image.clone())),
        ))
        .id();
    commands.insert_resource(TerminalImage { image, camera });
}

/// What lies under the screen while a terminal is open: the world darkened, and over it
/// the monitor the engine draws ([`MonitorView`]), or else the screen's picture flat in
/// the middle of the window (whose pointer is found here). The screen's camera draws only
/// while a terminal is open.
#[allow(clippy::too_many_arguments)]
fn backdrop(
    mut commands: Commands,
    screen: Res<TerminalScreen>,
    image: Option<Res<TerminalImage>>,
    monitor: Option<Res<MonitorView>>,
    touch: Res<touch::Settings>,
    roots: Query<(Entity, &screen::Backdrop)>,
    flat: Query<(&ComputedNode, &UiGlobalTransform), With<screen::FlatScreen>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cams: Query<&mut Camera>,
    mut pointer: ResMut<TerminalPointer>,
) {
    let Some(image) = image else { return };
    let open = screen.0.is_some();
    if let Ok(mut c) = cams.get_mut(image.camera) {
        if c.is_active != open {
            c.is_active = open;
        }
    }
    // The flat picture until the monitor is drawn (the engine draws it a frame or more
    // after the terminal opens), then the monitor; `None` once closed.
    let want = open.then_some(monitor.is_none() || touch.enabled);
    let have = roots.iter().next().map(|(_, b)| b.flat);
    if want != have {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        match (want, monitor.as_ref()) {
            (Some(false), Some(m)) => screen::spawn_backdrop(&mut commands, screen::Under::Monitor(m.image.clone())),
            (Some(_), _) => screen::spawn_backdrop(&mut commands, screen::Under::Flat(image.image.clone())),
            (None, _) => {}
        }
    }
    if want != Some(true) {
        if !open {
            pointer.canvas = None;
        }
        return;
    }
    let Some((computed, at)) = flat.iter().next() else { return };
    let cursor = windows.single().ok().and_then(|w| w.physical_cursor_position());
    let size = computed.size();
    pointer.canvas = cursor.and_then(|c| {
        let p = (c - (at.translation - size / 2.0)) / size * screen::CANVAS;
        (p.x >= 0.0 && p.y >= 0.0 && p.x < screen::CANVAS.x && p.y < screen::CANVAS.y).then_some(p)
    });
}

fn run_aid(
    time: AidTime,
    mut aid: ResMut<TerminalAid>,
    mut open: MessageWriter<OpenTerminal>,
    mut input: MessageWriter<TerminalInput>,
    mut power: ResMut<PowerAid>,
) {
    if aid.steps.is_empty() {
        return;
    }
    let Some(now) = time.world_secs() else { return };
    let mut due: Vec<(f32, AidStep)> = aid.steps.iter().filter(|(t, _)| *t <= now).copied().collect();
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    aid.steps.retain(|(t, _)| *t > now);
    for (_, step) in due {
        info!("test aid NVF_PLAY_TEST_TERMINAL: {step:?}");
        match step {
            AidStep::Open(r) => {
                open.write(OpenTerminal { reference: r });
            }
            AidStep::Science(s) => aid.science = Some(s),
            AidStep::Input(i) => {
                input.write(i);
            }
            AidStep::Power => power.0 = true,
        }
    }
}

/// A seed for a new game: different every time [design: the original's dump is new on
/// every visit, the owner 2026-10-11].
fn seed(reference: FormId, now: f32) -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    t ^ ((reference.0 as u64) << 20) ^ now.to_bits() as u64
}

/// The listed items of a terminal and the view over them.
fn menu_view(rt: &mut crate::script::Runtime, reference: FormId, terminal: FormId, top: FormId) -> MenuView {
    let items = rules::items(&mut rt.game, reference, terminal);
    let labels = labels(rt, terminal, &items);
    let (server, welcome) = rt.game.data.terminals.get(&top).map(|t| (t.server, rt.game.expand_codes(&t.welcome))).unwrap_or_default();
    MenuView { terminal, parents: Vec::new(), items, labels, selected: 0, hidden: true, result: String::new(), note: None, scroll: 0, server, welcome }
}

/// The lines of terminal `terminal`'s items `items`.
fn labels(rt: &crate::script::Runtime, terminal: FormId, items: &[usize]) -> Vec<String> {
    items.iter().map(|&i| rules::item(&rt.game, terminal, i).map(|it| rt.game.expand_codes(&it.text)).unwrap_or_default()).collect()
}

/// The menu lists `items` of `terminal`, with their lines: every change of the list
/// goes through here, so the lines drawn are always the list's (#1328 review: Back from
/// a sub menu left the sub menu's lines).
fn list(rt: &crate::script::Runtime, m: &mut MenuView, terminal: FormId, items: Vec<usize>) {
    m.labels = labels(rt, terminal, &items);
    (m.terminal, m.items) = (terminal, items);
}

fn log_menu(rt: &crate::script::Runtime, m: &MenuView) {
    let data = rt.game.data;
    if !data.terminals.contains_key(&m.terminal) {
        return;
    }
    let names: Vec<String> = m.labels.iter().map(|l| format!("{l:?}")).collect();
    info!("terminal: menu of {} ({} of {} items listed): {}", data.name(m.terminal), m.items.len(), rules::item_count(&rt.game, m.terminal), names.join(", "));
}

#[allow(clippy::too_many_arguments)]
fn open(
    mut requests: MessageReader<OpenTerminal>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut screen: ResMut<TerminalScreen>,
    paused: Res<Paused>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    words: Res<Words>,
    aid: Res<TerminalAid>,
    time: AidTime,
    mut stats: ResMut<TerminalStats>,
    mut corner: Option<ResMut<crate::corner_messages::CornerMessages>>,
    (mut doors, mut opening_anim): (Option<ResMut<crate::doors::Doors>>, ResMut<TerminalOpening>),
) {
    let Some(asked) = requests.read().last().copied() else { return };
    if *state.get() != PlayState::Playing || screen.0.is_some() || paused.0 {
        info!("terminal: request for {:08X} ignored, the player is not in the world", asked.reference);
        return;
    }
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else {
        warn!("terminal: scripts are not running; terminals need the game's rules");
        return;
    };
    let reference = FormId(asked.reference);
    let Some(base) = sim.base_of(asked.reference) else {
        warn!("terminal: {reference} is not a placed reference");
        return;
    };
    if !rt.game.data.terminals.contains_key(&base) {
        warn!("terminal: {reference} (base {base}) is not a terminal");
        return;
    }
    // An animated terminal plays "Open" first; `follow_open` asks again once it has.
    if opening_anim.hold(doors.as_deref_mut(), asked.reference) {
        return;
    }
    let now = time.elapsed_secs();
    let name = rt.game.data.name(base);
    let opening = with_world(rt, &mut sim, |rt| {
        let sci = aid.science.unwrap_or_else(|| rules::science(&mut rt.game));
        rules::opening(&mut rt.game, reference, base, sci).map(|o| (o, sci))
    });
    let Some((opening, sci)) = opening else { return };
    stats.opened += 1;
    let page = match opening {
        Opening::Menu => {
            let m = with_world(rt, &mut sim, |rt| menu_view(rt, reference, base, base));
            info!("terminal: open {reference} {name}: its menu (unlocked, hacked before or the password is held)");
            log_menu(rt, &m);
            rt.game.open_menu(MENU_TERMINAL);
            Page::Menu(m)
        }
        Opening::LockedOut => {
            info!("terminal: open {reference} {name}: locked out for good");
            rt.game.open_menu(MENU_HACKING);
            Page::LockedOut(lockout_lines(&rt.game))
        }
        Opening::Refused(r) => {
            stats.refused += 1;
            // [hypothesis, hacking.md §8] The wording is NVForever's.
            let line = match r {
                fnv_game::hacking::Refusal::RequiresKey => fnv_util::words::tr("This terminal requires a password.").to_string(),
                fnv_game::hacking::Refusal::Science { needed } => {
                    fnv_util::words::trf("You need a Science skill of {needed} to hack this terminal.", &[("needed", &format!("{needed:.0}"))])
                }
            };
            // The player's tweak says the player's own Science [design: docs/design/tweaks.md].
            let line = if matches!(r, fnv_game::hacking::Refusal::Science { .. }) && rt.game.tweaks.show_current_skill { format!("{} (yours: {sci:.0})", line.trim_end_matches('.')) } else { line };
            info!("terminal: {reference} {name} refused (Science {sci:.0}): on screen: {line:?}");
            // A top-left corner message (owner, list 5 item 45).
            if let Some(c) = corner.as_deref_mut() {
                c.push(line.into());
            }
            close_model(doors.as_deref_mut(), asked.reference);
            return;
        }
        Opening::Hack { difficulty, words: n } => {
            let mut rng = ChaCha8Rng::seed_from_u64(seed(reference, now));
            let Some(mut puzzle) = Puzzle::new(&words.0, difficulty, n, &mut rng) else {
                warn!("terminal: {reference} {name}: the word list has too few words for {}", difficulty.name());
                return;
            };
            // The player's hacking tweaks [design: docs/design/tweaks.md].
            let t = &rt.game.tweaks;
            puzzle.rules = fnv_game::hacking::HackRules::from_tweaks(t);
            stats.hacks += 1;
            info!(
                "terminal: open {reference} {name}: hacking, {} (Science {sci:.0}): {} words of {} letters, {} bracket pairs",
                difficulty.name(),
                puzzle.words.len(),
                puzzle.password().len(),
                puzzle.pairs().len()
            );
            rt.game.open_menu(MENU_HACKING);
            let cursor = first_target(&puzzle);
            Page::Hacking(Hacking::new(puzzle, cursor))
        }
    };
    screen.0 = Some(Session { reference, base, page });
    next.set(PlayState::Menu);
}

/// Where the cursor starts: the top left character [design].
fn first_target(_p: &Puzzle) -> usize {
    0
}

/// The cursor moved by a key: across a whole word at once, from the end of a row of the
/// first column into the second and back [hypothesis, hacking.md §7].
pub fn step_cursor(p: &Puzzle, cursor: usize, dx: i32, dy: i32) -> usize {
    let per_column = ROWS * ROW_CHARS;
    let (mut column, mut row, mut col) = ((cursor / per_column) as i32, ((cursor / ROW_CHARS) % ROWS) as i32, (cursor % ROW_CHARS) as i32);
    if dy != 0 {
        row = (row + dy).clamp(0, ROWS as i32 - 1);
        return Puzzle::index(column as usize, row as usize, col as usize);
    }
    if dx == 0 {
        return cursor;
    }
    // Leave the word the cursor is on first.
    let span = p.span(p.target(cursor));
    let mut at = if dx > 0 { span.end as i32 - 1 } else { span.start as i32 };
    column = at / per_column as i32;
    row = (at / ROW_CHARS as i32) % ROWS as i32;
    col = at % ROW_CHARS as i32 + dx.signum();
    if col >= ROW_CHARS as i32 {
        if column == 0 {
            (column, col) = (1, 0);
        } else {
            return cursor;
        }
    } else if col < 0 {
        if column == 1 {
            (column, col) = (0, ROW_CHARS as i32 - 1);
        } else {
            return cursor;
        }
    }
    at = Puzzle::index(column as usize, row as usize, col as usize) as i32;
    // A word that runs on from the row before or into the next: its first character.
    match p.target(at as usize) {
        Target::Word(w) if dx > 0 => p.words[w].start.max(at as usize),
        _ => at as usize,
    }
}

/// How long the granted screen stays with the Faster Hacking tweak [design: docs/design/tweaks.md].
const FAST_GRANTED_SECONDS: f32 = 0.5;

fn keys(keys: crate::pad::ActionButtons<KeyCode>, screen: Res<TerminalScreen>, mut out: MessageWriter<TerminalInput>, mut touch: Option<ResMut<touch::TouchState>>, tweaks: Option<Res<crate::tweaks::TweakSettings>>) {
    let keys = keys.snapshot();
    use KeyCode::*;
    let Some(s) = screen.0.as_ref() else { return };
    if let Some(touch) = touch.as_deref_mut().filter(|t| t.configuring()) {
        if keys.just_pressed(Escape) { touch.close_options(); }
        return;
    }
    let mut send = |i| {
        out.write(i);
    };
    // The player's tweak: Tab closes the terminal from any page (Backspace still goes
    // back) [design: docs/design/tweaks.md].
    let tab_closes = tweaks.as_ref().is_some_and(|t| t.tab_closes_terminals);
    if keys.just_pressed(Escape) || (tab_closes && keys.just_pressed(Tab)) {
        send(TerminalInput::Exit);
        return;
    }
    if keys.any_just_pressed([Tab, Backspace]) {
        send(TerminalInput::Back);
        return;
    }
    if keys.any_just_pressed([Enter, NumpadEnter, KeyE]) {
        send(TerminalInput::Select);
    }
    let hacking = matches!(s.page, Page::Hacking(_));
    // The player's tweak: 1 to 9 and 0 (the tenth) choose a row [design: docs/design/tweaks.md].
    if !hacking && tweaks.as_ref().is_some_and(|t| t.numbered_terminal_keys) {
        let digits = [Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0];
        let pads = [Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9, Numpad0];
        if let Some(row) = (0..10).find(|&i| keys.just_pressed(digits[i]) || keys.just_pressed(pads[i])) {
            send(TerminalInput::Row(row));
            send(TerminalInput::Select);
            return;
        }
    }
    if keys.any_just_pressed([ArrowUp, KeyW]) {
        send(TerminalInput::Step(0, -1));
    }
    if keys.any_just_pressed([ArrowDown, KeyS]) {
        send(TerminalInput::Step(0, 1));
    }
    if hacking && keys.any_just_pressed([ArrowLeft, KeyA]) {
        send(TerminalInput::Step(-1, 0));
    }
    if hacking && keys.any_just_pressed([ArrowRight, KeyD]) {
        send(TerminalInput::Step(1, 0));
    }
}

/// The pointer on the screen's canvas ([`TerminalPointer`]) puts the cursor on the dump
/// character or the menu row under it, and a click enters that.
fn mouse(
    (screen, touch): (Res<TerminalScreen>, Res<touch::Settings>),
    buttons: crate::pad::ActionButtons<MouseButton>,
    pointer: Res<TerminalPointer>,
    rows: Res<MenuRows>,
    mut last: Local<Option<usize>>,
    mut last_row: Local<Option<usize>>,
    mut out: MessageWriter<TerminalInput>,
) {
    let buttons = buttons.snapshot();
    let Some(s) = screen.0.as_ref() else { return };
    if touch.enabled { return; } // The accessible flat adapter owns its own hit targets.
    match &s.page {
        Page::Hacking(h) => {
            let under = pointer.canvas.and_then(screen::canvas_index);
            if under != *last {
                *last = under;
                match under {
                    Some(i) if i != h.cursor || h.hidden => {
                        out.write(TerminalInput::Point(i));
                    }
                    None => {
                        out.write(TerminalInput::Unpoint);
                    }
                    _ => {}
                }
            }
            if under.is_some() && buttons.just_pressed(MouseButton::Left) {
                out.write(TerminalInput::Select);
            }
        }
        // A click on an open note turns its page (#3409) [hypothesis: as E].
        Page::Menu(m) if m.note.is_some() => {
            if pointer.canvas.is_some() && buttons.just_pressed(MouseButton::Left) {
                out.write(TerminalInput::Select);
            }
        }
        Page::Menu(_) => {
            let under = pointer.canvas.and_then(|p| rows.0.rows.iter().find(|(_, r)| r.contains(p)).map(|(i, _)| *i));
            if under != *last_row {
                *last_row = under;
                if let Some(i) = under {
                    out.write(TerminalInput::Row(i));
                }
            }
            if let (Some(i), true) = (under, buttons.just_pressed(MouseButton::Left)) {
                out.write(TerminalInput::Row(i));
                out.write(TerminalInput::Select);
            }
        }
        Page::LockedOut(_) => {
            if buttons.just_pressed(MouseButton::Left) {
                out.write(TerminalInput::Back);
            }
        }
    }
}

/// Close the terminal: back to the world.
fn leave(next: &mut NextState<PlayState>, why: &str) {
    info!("terminal: closed ({why})");
    next.set(PlayState::Playing);
}

#[allow(clippy::too_many_arguments)]
fn apply(
    mut input: MessageReader<TerminalInput>,
    mut screen: ResMut<TerminalScreen>,
    mut next: ResMut<NextState<PlayState>>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    time: AidTime,
    real: Res<Time<Real>>,
    mut stats: ResMut<TerminalStats>,
    mut rng: Local<Option<ChaCha8Rng>>,
    touch: Option<Res<touch::TouchState>>,
    (mut sounds, mut opened, mut read, drawn): (MessageWriter<UiSound>, MessageWriter<crate::lockpick::LockOpened>, ResMut<ReadEntries>, Res<MenuRows>),
) {
    let now = time.elapsed_secs();
    // The hover sound's gap is real time: the menu's pause and the game's speed do not
    // stretch it.
    let real_now = real.elapsed();
    // The hacking menu's sounds (list 5 item 47; `crate::ui_sounds`).
    let mut sound = |event| {
        sounds.write(UiSound { menu: UiMenu::Terminal, event });
    };
    // RefreshTerminalMenu (`fnv_game::nvse_jg9`): the open menu's list is read again; with
    // no terminal open the request is dropped.
    if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
        if std::mem::take(&mut rt.game.other.jg9.refresh_terminal) {
            if let Some(s) = screen.0.as_mut() {
                if let Page::Menu(m) = &mut s.page {
                    let (reference, t) = (s.reference, m.terminal);
                    let items = with_world(rt, &mut sim, |rt| rules::items(&mut rt.game, reference, t));
                    list(rt, m, t, items);
                    m.selected = m.selected.min(m.items.len().saturating_sub(1));
                    info!("terminal: a script redraws the menu");
                    log_menu(rt, m);
                }
            }
        }
    }
    let mut entered = false;
    for &cmd in input.read() {
        let committing = matches!(cmd, TerminalInput::Select | TerminalInput::Aid(_));
        if committing && (entered || touch.as_deref().is_some_and(touch::TouchState::configuring)) { continue; }
        if committing { entered = true; }
        let Some(s) = screen.0.as_mut() else { return };
        let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
        let (reference, base) = (s.reference, s.base);
        match &mut s.page {
            Page::Hacking(h) => {
                // The player's tweak: no leaving once a guess is made, until the game ends [design: docs/design/tweaks.md].
                if matches!(cmd, TerminalInput::Back | TerminalInput::Exit) && h.ended.is_none() && !h.puzzle.log.is_empty() && rt.game.tweaks.no_exit_hacking {
                    info!("terminal: leaving refused, a guess was made (tweak)");
                    continue;
                }
                if matches!(cmd, TerminalInput::Back | TerminalInput::Exit) {
                    // Leaving keeps nothing: the next visit is a new game with every
                    // attempt [the owner, 2026-10-11]; a lockout stays.
                    leave(&mut next, if h.ended.is_some() { "after the game" } else { "the hacking game was left; attempts come back on the next visit" });
                    return;
                }
                if h.ended.is_some() {
                    continue;
                }
                let at = match cursor_step(h, cmd, real_now) {
                    CursorStep::Pick(at) => at,
                    CursorStep::Moved(focus) => {
                        stats.count_hover(cmd, focus);
                        if focus == Focus::Sound {
                            sound(UiEvent::Focus);
                        }
                        continue;
                    }
                };
                let p = &mut h.puzzle;
                sound(UiEvent::Picked);
                let entered = p.text(p.target(at));
                let rng = rng.get_or_insert_with(|| ChaCha8Rng::seed_from_u64(seed(reference, now)));
                let o = p.choose(at, rng);
                match o {
                    Outcome::Denied { likeness } => {
                        sound(UiEvent::Refused);
                        stats.guesses += 1;
                        info!("terminal: >{entered}: wrong word, {likeness} of {} right, {} attempts left", p.password().len(), p.attempts);
                    }
                    Outcome::Granted => {
                        sound(UiEvent::Granted);
                        stats.guesses += 1;
                        stats.hacked += 1;
                        info!("terminal: >{entered}: the password, access granted");
                        rules::hacked(&mut rt.game, reference, true);
                        // Hacking an owned terminal is the crime of picking an owned lock:
                        // karma, and the owner's side that sees it turns hostile
                        // (`crate::crime`; the owner, 2026-10-22).
                        opened.write(crate::lockpick::LockOpened { reference: reference.0, owner: None, how: crate::lockpick::OpenedBy::Hacked });
                        h.ended = Some(now);
                    }
                    Outcome::LockedOut => {
                        sound(UiEvent::Refused);
                        stats.guesses += 1;
                        stats.locked_out += 1;
                        info!("terminal: >{entered}: wrong word, no attempts left: the terminal is locked for good");
                        rules::hacked(&mut rt.game, reference, false);
                        h.ended = Some(now);
                    }
                    Outcome::DudRemoved => {
                        stats.pairs += 1;
                        stats.duds += 1;
                        info!("terminal: >{entered}: Dud removed ({} words left)", p.words.iter().filter(|w| !w.removed).count());
                    }
                    Outcome::Replenished => {
                        stats.pairs += 1;
                        stats.refills += 1;
                        info!("terminal: >{entered}: attempts restored ({} attempts)", p.attempts);
                    }
                    Outcome::Error => info!("terminal: >{entered}: Error"),
                    Outcome::Ignored => {}
                }
            }
            Page::LockedOut(_) => {
                if matches!(cmd, TerminalInput::Back | TerminalInput::Exit | TerminalInput::Select) {
                    leave(&mut next, "locked out");
                    return;
                }
            }
            Page::Menu(m) => {
                let (selected_was, hidden_was) = (m.selected, m.hidden);
                let take = match cmd {
                    TerminalInput::Exit => {
                        leave(&mut next, "Esc");
                        return;
                    }
                    TerminalInput::Back => {
                        if m.note.take().is_some() {
                            info!("terminal: back from the note to the list");
                        } else if let Some((parent, row)) = m.parents.pop() {
                            let items = with_world(rt, &mut sim, |rt| rules::items(&mut rt.game, reference, parent));
                            list(rt, m, parent, items);
                            m.selected = row;
                            log_menu(rt, m);
                        } else {
                            leave(&mut next, "back from the top menu");
                            return;
                        }
                        None
                    }
                    // Up / Down scroll a note a line [design], no further than its last lines
                    // show (as the screen last drew it); E, Enter or a click turns to its next
                    // page, and nothing on the last (#3409) [hypothesis: the player's memory
                    // of the original; observer check 255].
                    TerminalInput::Step(_, dy) if m.note.is_some() => {
                        let window = drawn.0.note.filter(|w| Some(w.note) == m.note);
                        m.scroll = note_scroll(m.scroll, window, dy);
                        None
                    }
                    TerminalInput::Select if m.note.is_some() => {
                        if let Some(next) = drawn.0.note.filter(|w| Some(w.note) == m.note).and_then(|w| w.next_page()) {
                            info!("terminal: note turned to line {next}");
                            m.scroll = next;
                            sound(UiEvent::Focus);
                        }
                        None
                    }
                    TerminalInput::Step(_, dy) if m.note.is_none() && !m.items.is_empty() => {
                        if !m.hidden {
                            m.selected = (m.selected as i32 + dy).clamp(0, m.items.len() as i32 - 1) as usize;
                        }
                        m.hidden = false;
                        None
                    }
                    TerminalInput::Row(r) if r < m.items.len() => {
                        (m.selected, m.hidden) = (r, false);
                        None
                    }
                    TerminalInput::Select if m.note.is_none() && m.hidden => {
                        m.hidden = false;
                        None
                    }
                    TerminalInput::Select if m.note.is_none() => m.items.get(m.selected).copied(),
                    TerminalInput::Aid(AidPick::Item(n)) if m.note.is_none() => {
                        (m.selected, m.hidden) = (n.min(m.items.len().saturating_sub(1)), false);
                        m.items.get(n).copied()
                    }
                    _ => None,
                };
                let Some(item) = take else {
                    let moves = matches!(cmd, TerminalInput::Step(..) | TerminalInput::Row(_) | TerminalInput::Select);
                    if moves && m.note.is_none() && !m.hidden && (m.selected != selected_was || hidden_was) {
                        sound(UiEvent::Focus);
                    }
                    continue;
                };
                sound(UiEvent::Picked);
                let terminal = m.terminal;
                let from = rt.game.events.len();
                let chosen = with_world(rt, &mut sim, |rt| rules::choose(&mut rt.game, reference, terminal, item));
                let Some(c) = chosen else { continue };
                if c.note.is_some() && grey_read() && read.0.insert((terminal.0, item as u32)) {
                    debug!("terminal: item {item} of {} read (Grey Read Notes)", rt.game.data.name(terminal));
                }
                let ran: Vec<String> = rt.game.events[from..].iter().take(8).map(|e| format!("{:?}", e.kind)).collect();
                if !ran.is_empty() {
                    info!("terminal: its result script: {}", ran.join("; "));
                }
                stats.items += 1;
                let text = rules::item(&rt.game, terminal, item).map(|i| i.text).unwrap_or_default();
                info!(
                    "terminal: chose {text:?} of {}: result {:?}{}{}",
                    rt.game.data.name(terminal),
                    c.result,
                    c.submenu.map(|s| format!(", sub menu {}", rt.game.data.name(s))).unwrap_or_default(),
                    c.note.map(|n| format!(", note {}", rt.game.data.name(n))).unwrap_or_default()
                );
                m.result = rt.game.expand_codes(&c.result);
                // The result script called ForceTerminalBack: back one level, or out of
                // the terminal from its top level [documented: GECK wiki ForceTerminalBack].
                if c.back {
                    stats.forced_back += 1;
                    (m.note, m.scroll) = (None, 0);
                    if let Some((parent, row)) = m.parents.pop() {
                        info!("terminal: ForceTerminalBack: back to {}", rt.game.data.name(parent));
                        let items = with_world(rt, &mut sim, |rt| rules::items(&mut rt.game, reference, parent));
                        list(rt, m, parent, items);
                        m.selected = row;
                        log_menu(rt, m);
                        continue;
                    }
                    leave(&mut next, "ForceTerminalBack from the top menu");
                    return;
                }
                if let Some(sub) = c.submenu {
                    m.parents.push((terminal, m.selected));
                    m.terminal = sub;
                    m.selected = 0;
                }
                // [design] The list is read again after every choice: a script may have
                // changed what its conditions see (the "Force Redraw" flag says so too).
                let t = m.terminal;
                let items = with_world(rt, &mut sim, |rt| rules::items(&mut rt.game, reference, t));
                list(rt, m, t, items);
                m.selected = m.selected.min(m.items.len().saturating_sub(1));
                (m.note, m.scroll) = (c.note, 0);
                if c.submenu.is_some() {
                    log_menu(rt, m);
                }
            }
        }
        let _ = base;
    }
}

/// The test aid's pick of a kind of word or pair.
fn aid_pick(p: &Puzzle, a: AidPick) -> Option<usize> {
    match a {
        AidPick::Right => Some(p.words[p.password].start),
        AidPick::Wrong => p.words.iter().enumerate().find(|(i, w)| *i != p.password && !w.removed).map(|(_, w)| w.start),
        AidPick::Brackets => p.pairs().first().map(|&(s, _)| s),
        AidPick::Symbol => (0..p.chars.len()).find(|&i| matches!(p.target(i), Target::Char(_)) && !p.chars[i].is_alphabetic()),
        AidPick::Item(_) => None,
    }
}

/// The lockout screen's lines: `sHackingLockout3` and `sHackingLockout4`, the names
/// `menus\hacking_menu.xml` gives them, from the game strings table (the load order's
/// string settings over NVForever's labels; menu-xml.md §2.4).
fn lockout_lines(game: &fnv_game::Game<'_>) -> [String; 2] {
    [fnv_game::ui_loading::game_string(game, "sHackingLockout3"), fnv_game::ui_loading::game_string(game, "sHackingLockout4")]
}

/// The ended game turns into the menu or the lockout screen.
fn timers(time: AidTime, mut screen: ResMut<TerminalScreen>, mut sim: ResMut<SimState>, mut script: Option<ResMut<ScriptState>>) {
    let now = time.elapsed_secs();
    let Some(s) = screen.0.as_mut() else { return };
    let Page::Hacking(h) = &s.page else { return };
    let Some(ended) = h.ended else { return };
    let won = h.puzzle.state == GameState::Granted;
    // The player's tweak keeps the granted screen up briefly only [design: docs/design/tweaks.md].
    let fast = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.faster_hacking_transition);
    let granted = if fast { FAST_GRANTED_SECONDS } else { GRANTED_SECONDS };
    if now - ended < if won { granted } else { LOCKED_SECONDS } {
        return;
    }
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
    if rt.game.menu == Some(MENU_HACKING) {
        rt.game.close_menu();
    }
    if won {
        let (reference, base) = (s.reference, s.base);
        let m = with_world(rt, &mut sim, |rt| menu_view(rt, reference, base, base));
        log_menu(rt, &m);
        rt.game.open_menu(MENU_TERMINAL);
        s.page = Page::Menu(m);
    } else {
        let lines = lockout_lines(&rt.game);
        info!("terminal: on screen: {} / {}", lines[0], lines[1]);
        rt.game.open_menu(MENU_HACKING);
        s.page = Page::LockedOut(lines);
    }
}

/// A note's scroll after a step of `dy` lines from `scroll`: never above its first line, nor
/// past its last lines' `window` (when the screen has drawn it).
fn note_scroll(scroll: usize, window: Option<screen::NoteWindow>, dy: i32) -> usize {
    let s = (scroll.min(window.map_or(usize::MAX, |w| w.last_first())) as i64 + dy as i64).max(0) as usize;
    window.map_or(s, |w| s.min(w.last_first()))
}

/// A key for the page the screen shows, whatever is chosen, scrolled or guessed on it: its
/// text types out again when this changes (`screen::Typed`, #3327) [hypothesis: the
/// original types out a page as it comes up, not when the selection moves].
fn page_key(s: &Session) -> String {
    match &s.page {
        Page::Hacking(_) => "h".into(),
        Page::LockedOut(_) => "lockout".into(),
        Page::Menu(m) => format!("m{}:{}:{:?}", m.terminal.0, m.parents.len(), m.note),
    }
}

/// A key for what the screen shows: it is drawn again when this changes.
fn view_key(s: &Session) -> String {
    match &s.page {
        Page::Hacking(h) => format!("h{}:{}:{}:{}:{}:{:?}:{:?}", h.cursor, h.hidden, h.puzzle.log.len(), h.puzzle.attempts, h.puzzle.used.len(), h.ended.is_some(), h.puzzle.words.iter().filter(|w| w.removed).count()),
        Page::LockedOut(_) => "lockout".into(),
        Page::Menu(m) => format!("m{}:{:?}:{}:{}:{}:{}:{:?}:{}", m.terminal.0, m.items, m.selected, m.hidden, m.scroll, m.result, m.note, m.parents.len()),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    screen: Res<TerminalScreen>,
    data: Option<Res<GameData>>,
    script: Option<Res<ScriptState>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    roots: Query<Entity, With<screen::ScreenRoot>>,
    fading: Query<Entity, With<FadeOut>>,
    mut blinks: Query<&mut Visibility, With<screen::Blink>>,
    mut typed: Query<(&screen::Typed, &mut Node)>,
    time: Res<Time<Real>>,
    image: Option<Res<TerminalImage>>,
    (mut rows, read): (ResMut<MenuRows>, Res<ReadEntries>),
    mut faces: Local<Option<screen::Faces>>,
    mut shown: Local<Option<(String, u32)>>,
    mut page: Local<Option<(String, f32)>>,
) {
    // The screen is laid out in its own texture, at its size whatever the UI scale; the
    // picture of it on the window scales (`screen::spawn_backdrop`, #1631).
    let _canvas = crate::interface::original_units();
    let grey = grey_read();
    let key = screen.0.as_ref().map(|s| (format!("{}:{grey}", view_key(s)), crate::cjk_font::revision()));
    // A closing screen keeps its picture while it fades (`fade_out`); one opened meanwhile
    // takes its place at once.
    if !fading.is_empty() {
        if key.is_none() {
            return;
        }
        for e in &fading {
            commands.entity(e).try_despawn();
        }
        *shown = None;
    }
    let on = ((time.elapsed_secs() / screen::BLINK_SECONDS) as u32).is_multiple_of(2);
    // The page's text types out from when it came up (a closed screen starts again).
    let now = time.elapsed_secs();
    let this_page = screen.0.as_ref().map(page_key);
    if page.as_ref().map(|p| &p.0) != this_page.as_ref() {
        *page = this_page.map(|k| (k, now));
    }
    let typed_for = page.as_ref().map_or(f32::INFINITY, |p| now - p.1);
    if key == *shown {
        for mut v in &mut blinks {
            v.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
        }
        for (t, mut node) in &mut typed {
            let w = crate::interface::u(t.shown(typed_for));
            if node.width != w {
                node.width = w;
            }
        }
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    *shown = key;
    rows.0 = screen::Drawn::default();
    let (Some(s), Some(rt), Some(image)) = (screen.0.as_ref(), script.as_ref().and_then(|s| s.rt.as_ref()), image) else { return };
    let faces = faces.get_or_insert_with(|| screen::load_faces(data.as_deref(), &mut images, bc_supported(formats.as_deref())));
    rows.0 = screen::spawn(&mut commands, image.camera, faces, s, &rt.game, on, grey.then_some(&*read), typed_for);
}

/// How long the screen fades out as it closes, seconds [documented:
/// `menus\computers_menu.xml` `menufade` 0.25; hypothesis: that the trait is the close's
/// fade].
pub const CLOSE_FADE: f32 = 0.25;

/// A closed terminal's backdrop and screen, fading out: seconds left of how many.
#[derive(Component)]
struct FadeOut {
    left: f32,
    total: f32,
}

/// The closing screen fades: its backdrop's shade and pictures lose their strength, and
/// all of it goes once the fade is over.
fn fade_out(mut commands: Commands, time: Res<Time<Real>>, mut fading: Query<(Entity, &mut FadeOut, Option<&mut BackgroundColor>, Option<&Children>)>, mut pictures: Query<&mut ImageNode>) {
    for (e, mut f, bg, children) in &mut fading {
        let start = f.left / f.total.max(1e-3);
        f.left -= time.delta_secs();
        if f.left <= 0.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        let k = (f.left / f.total.max(1e-3)) / start.max(1e-3);
        if let Some(mut bg) = bg {
            let a = bg.0.alpha();
            bg.0.set_alpha(a * k);
        }
        for c in children.into_iter().flatten() {
            if let Ok(mut img) = pictures.get_mut(*c) {
                let a = img.color.alpha();
                img.color.set_alpha(a * k);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn close(
    mut commands: Commands,
    mut screen: ResMut<TerminalScreen>,
    roots: Query<Entity, With<screen::ScreenRoot>>,
    backdrops: Query<Entity, With<screen::Backdrop>>,
    script: Option<ResMut<ScriptState>>,
    doors: Option<ResMut<crate::doors::Doors>>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    // The player's Terminal Fade Time tweak sets the fade's length [design: docs/design/tweaks.md].
    let fade = tweaks.as_ref().filter(|t| t.terminal_fade).map_or(CLOSE_FADE, |t| t.terminal_fade_seconds);
    let had = screen.0.is_some();
    if let Some(s) = screen.0.take() {
        close_model(doors.map(|d| d.into_inner()), s.reference.0);
        if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
            if matches!(rt.game.menu, Some(MENU_HACKING | MENU_TERMINAL)) {
                rt.game.close_menu();
            }
        }
    }
    for e in roots.iter().chain(&backdrops) {
        if had && fade > 0.0 {
            commands.entity(e).insert(FadeOut { left: fade, total: fade });
        } else {
            commands.entity(e).despawn();
        }
    }
}

/// Ask for the screen again once an animated terminal's "Open" has ended (or waited too
/// long).
fn follow_open(time: Res<Time>, mut opening: ResMut<TerminalOpening>, doors: Option<Res<crate::doors::Doors>>, mut open: MessageWriter<OpenTerminal>) {
    if let Some(reference) = opening.advance(doors.as_deref(), time.delta_secs()) {
        open.write(OpenTerminal { reference });
    }
}

/// Play an animated terminal's "Close" (nothing for one without the sequences).
fn close_model(doors: Option<&mut crate::doors::Doors>, reference: u32) {
    if doors.is_some_and(|d| d.set_open(reference, false)) {
        info!("terminal 0x{reference:08X} closes");
    }
}

#[cfg(test)]
mod opening_tests {
    use super::*;
    use crate::doors::{DoorTiming, Doors};

    #[test]
    fn an_animated_terminal_is_held_until_its_open_has_played() {
        let mut doors = Doors::default();
        let keys = vec![(0.0, "start".to_string()), (0.00001, "Sound: OBJComputerTerminalForward".to_string()), (0.5, "end".to_string())];
        doors.register(5, false, DoorTiming::new((0.0, 0.5, &keys), (0.0, 0.67, &keys)));
        let mut op = TerminalOpening::default();
        // Not drawn: opens at once.
        assert!(!op.hold(Some(&mut doors), 6));
        assert!(!op.hold(None, 5));
        assert!(op.hold(Some(&mut doors), 5));
        assert!(op.hold(Some(&mut doors), 5), "asked again while it opens: still held");
        assert_eq!(op.advance(Some(&doors), 0.1), None);
        doors.advance(0.55);
        assert_eq!(op.advance(Some(&doors), 0.45), Some(5));
        assert_eq!(op.advance(Some(&doors), 0.1), None, "asked for once");
        assert!(!op.hold(Some(&mut doors), 5), "open: the screen comes up");
    }

    #[test]
    fn a_terminal_that_never_opens_is_not_waited_for_forever() {
        let mut doors = Doors::default();
        let keys = vec![(0.0, "start".to_string()), (60.0, "end".to_string())];
        doors.register(5, false, DoorTiming::new((0.0, 60.0, &keys), (0.0, 1.0, &keys)));
        let mut op = TerminalOpening::default();
        assert!(op.hold(Some(&mut doors), 5));
        assert_eq!(op.advance(Some(&doors), OPEN_WAIT + 0.1), Some(5));
        assert!(!op.hold(Some(&mut doors), 5), "the request after the wait is not held again");
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<TerminalStats>) {
    if exit.read().next().is_none() {
        return;
    }
    if stats.opened != 0 || stats.refused != 0 {
        info!(
            "terminal summary: {} opened ({} refused), {} hacking games ({} hacked, {} locked out), {} words entered, {} bracket pairs ({} duds removed, {} allowances replenished), {} menu items chosen ({} sent back by ForceTerminalBack)",
            stats.opened, stats.refused, stats.hacks, stats.hacked, stats.locked_out, stats.guesses, stats.pairs, stats.duds, stats.refills, stats.items, stats.forced_back
        );
    }
    if stats.hover_points != 0 {
        info!("terminal hover: {} changed points, {} focus sounds, {} suppressed", stats.hover_points, stats.hover_sounds, stats.hover_suppressed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_game::hacking::{Word, ATTEMPTS, DUMP_CHARS};

    #[test]
    fn read_entries_are_kept_by_terminal_and_item() {
        let mut r = ReadEntries::default();
        r.0.insert((0x1234, 2));
        assert!(r.has(FormId(0x1234), 2));
        assert!(!r.has(FormId(0x1234), 1) && !r.has(FormId(0x1235), 2));
        // Saved as pairs, the terminal first (the save hook's remap moves it).
        let v = serde_json::to_value(&r).expect("saved");
        assert_eq!(v, serde_json::json!([[0x1234, 2]]));
        assert_eq!(serde_json::from_value::<ReadEntries>(v).expect("loaded"), r);
    }

    fn puzzle() -> Puzzle {
        let mut chars = vec!['.'; DUMP_CHARS];
        // Row 0 of column 1 ends in "LOO", row 1 starts with "TING".
        for (at, text) in [(9, "LOOTING"), (204, "MEETING")] {
            for (k, c) in text.chars().enumerate() {
                chars[at + k] = c;
            }
        }
        let words = vec![Word { text: "LOOTING".into(), len: 7, start: 9, removed: false }, Word { text: "MEETING".into(), len: 7, start: 204, removed: false }];
        Puzzle { chars, address: 0xF650, words, password: 1, attempts: ATTEMPTS, used: Vec::new(), log: Vec::new(), state: GameState::Guessing, rules: Default::default(), guessed: Vec::new() }
    }

    #[test]
    fn the_cursor_crosses_a_word_at_once() {
        let p = puzzle();
        // Right from just before the word: onto its start; right again: past its end,
        // which is in the next row.
        assert_eq!(step_cursor(&p, 8, 1, 0), 9);
        assert_eq!(step_cursor(&p, 9, 1, 0), 16);
        // Left from just after it: onto it, then before it.
        assert_eq!(step_cursor(&p, 16, -1, 0), 15);
        assert_eq!(step_cursor(&p, 15, -1, 0), 8);
    }

    #[test]
    fn the_cursor_goes_between_the_columns() {
        let p = puzzle();
        // The end of row 5 of column 1 goes on at the start of row 5 of column 2.
        let end = Puzzle::index(0, 5, 11);
        assert_eq!(step_cursor(&p, end, 1, 0), Puzzle::index(1, 5, 0));
        assert_eq!(step_cursor(&p, Puzzle::index(1, 5, 0), -1, 0), end);
        // Off the left of column 1 or the right of column 2: it stays.
        assert_eq!(step_cursor(&p, Puzzle::index(0, 3, 0), -1, 0), Puzzle::index(0, 3, 0));
        assert_eq!(step_cursor(&p, Puzzle::index(1, 3, 11), 1, 0), Puzzle::index(1, 3, 11));
        // Rows: the same column, stopping at the ends.
        assert_eq!(step_cursor(&p, Puzzle::index(0, 0, 4), 0, -1), Puzzle::index(0, 0, 4));
        assert_eq!(step_cursor(&p, Puzzle::index(0, 0, 4), 0, 1), Puzzle::index(0, 1, 4));
        assert_eq!(step_cursor(&p, Puzzle::index(1, 16, 4), 0, 1), Puzzle::index(1, 16, 4));
    }

    #[test]
    fn the_aid_finds_each_kind() {
        let p = puzzle();
        assert_eq!(aid_pick(&p, AidPick::Right), Some(204));
        assert_eq!(aid_pick(&p, AidPick::Wrong), Some(9));
        assert_eq!(aid_pick(&p, AidPick::Brackets), None);
        assert_eq!(aid_pick(&p, AidPick::Symbol), Some(0));
    }

    #[test]
    /// #3409: E turns a long note a page on, the last page keeping its last lines in view;
    /// on the last page it does nothing. Up / Down stop at the ends; a note that fits has
    /// no next page.
    fn a_long_note_turns_by_pages() {
        let w = |first| screen::NoteWindow { note: FormId(7), first, shown: 16, total: 40 };
        assert_eq!(w(0).next_page(), Some(16));
        assert_eq!(w(16).next_page(), Some(24), "the last page shows the last 16 lines");
        assert_eq!(w(24).next_page(), None, "nothing after the last page");
        assert_eq!(screen::NoteWindow { note: FormId(7), first: 0, shown: 9, total: 9 }.next_page(), None, "a note that fits");
        assert_eq!(note_scroll(24, Some(w(24)), 1), 24, "Down stops at the last lines");
        assert_eq!(note_scroll(24, Some(w(24)), -1), 23);
        assert_eq!(note_scroll(0, Some(w(0)), -1), 0);
        assert_eq!(note_scroll(90, Some(w(24)), -1), 23, "a scroll past the end comes back from the end");
        assert_eq!(note_scroll(3, None, 1), 4, "not drawn yet: a line on");
        assert_eq!(parse_aid("5:select").unwrap(), vec![(5.0, AidStep::Input(TerminalInput::Select))]);
    }

    #[test]
    fn the_aid_reads_its_steps() {
        let s = parse_aid("1:open:0010ABCD,2:science:100,3:wrong,4:item:2,5:back").unwrap();
        assert_eq!(s[0], (1.0, AidStep::Open(0x0010ABCD)));
        assert_eq!(s[1], (2.0, AidStep::Science(100.0)));
        assert_eq!(s[2], (3.0, AidStep::Input(TerminalInput::Aid(AidPick::Wrong))));
        assert_eq!(s[3], (4.0, AidStep::Input(TerminalInput::Aid(AidPick::Item(2)))));
        assert_eq!(s[4], (5.0, AidStep::Input(TerminalInput::Back)));
        assert!(parse_aid("1:dance").is_err());
        let s = parse_aid("6:point:12").unwrap();
        assert_eq!(s[0], (6.0, AidStep::Input(TerminalInput::Point(12))));
        assert!(parse_aid(&format!("6:point:{}", DUMP_CHARS - 1)).is_ok());
        assert!(parse_aid(&format!("6:point:{DUMP_CHARS}")).is_err(), "past the dump");
        assert!(parse_aid("6:point:x").is_err());
        assert!(parse_aid("6:point").is_err());
    }

    fn game() -> Hacking {
        Hacking::new(puzzle(), first_target(&puzzle()))
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_mouse_sweep_limits_focus_sounds() {
        for hz in [60u64, 144] {
            let mut h = game();
            let inputs = 120;
            let mut sounded: Vec<Duration> = Vec::new();
            for k in 0..inputs {
                let now = Duration::from_nanos(k as u64 * 1_000_000_000 / hz);
                let step = cursor_step(&mut h, TerminalInput::Point(k), now);
                assert_eq!(h.cursor, k, "{hz} Hz: the cursor follows every point");
                assert!(!h.hidden);
                match step {
                    CursorStep::Moved(Focus::Sound) => sounded.push(now),
                    CursorStep::Moved(Focus::Suppressed) => {}
                    other => panic!("{hz} Hz: a changed point is a sound or held back, not {other:?}"),
                }
            }
            assert!(!sounded.is_empty() && sounded.len() < inputs, "{hz} Hz: {} sounds for {inputs} points", sounded.len());
            for w in sounded.windows(2) {
                assert!(w[1] - w[0] >= HOVER_SOUND_GAP, "{hz} Hz: sounds {:?} apart", w[1] - w[0]);
            }
        }
    }

    #[test]
    fn hover_sounds_resume_at_the_gap() {
        let mut h = game();
        let t = Duration::from_secs(1);
        assert_eq!(cursor_step(&mut h, TerminalInput::Point(1), t), CursorStep::Moved(Focus::Sound));
        assert_eq!(cursor_step(&mut h, TerminalInput::Point(2), t + HOVER_SOUND_GAP - Duration::from_micros(1)), CursorStep::Moved(Focus::Suppressed));
        assert_eq!(h.cursor, 2, "held back, the cursor still moves");
        assert_eq!(cursor_step(&mut h, TerminalInput::Point(3), t + HOVER_SOUND_GAP), CursorStep::Moved(Focus::Sound));
    }

    #[test]
    fn leaving_the_dump_does_not_reset_the_sound_gap() {
        let mut h = game();
        let steps = [(TerminalInput::Point(1), ms(0)), (TerminalInput::Unpoint, ms(10)), (TerminalInput::Point(2), ms(20))];
        let got: Vec<CursorStep> = steps.iter().map(|&(i, t)| cursor_step(&mut h, i, t)).collect();
        assert_eq!(got, [CursorStep::Moved(Focus::Sound), CursorStep::Moved(Focus::Silent), CursorStep::Moved(Focus::Suppressed)]);
        assert_eq!((h.cursor, h.hidden), (2, false), "shown again where the pointer is");
    }

    #[test]
    fn keyboard_focus_is_not_throttled() {
        let mut h = game();
        assert_eq!(cursor_step(&mut h, TerminalInput::Point(5), ms(0)), CursorStep::Moved(Focus::Sound));
        // Keys right after the pointer's sound, and after each other: every move sounds.
        for (k, t) in [(6, 1), (7, 2), (8, 3)] {
            assert_eq!(cursor_step(&mut h, TerminalInput::Step(1, 0), ms(t)), CursorStep::Moved(Focus::Sound));
            assert_eq!(h.cursor, k);
        }
        // A point that does not move the cursor neither sounds nor starts the wait again.
        assert_eq!(cursor_step(&mut h, TerminalInput::Point(20), ms(100)), CursorStep::Moved(Focus::Sound));
        assert_eq!(cursor_step(&mut h, TerminalInput::Point(20), ms(150)), CursorStep::Moved(Focus::Silent));
        assert_eq!(cursor_step(&mut h, TerminalInput::Point(21), ms(170)), CursorStep::Moved(Focus::Sound), "70 ms after the last sound");
        // A new game: its first point sounds at once.
        let mut next = game();
        assert_eq!(cursor_step(&mut next, TerminalInput::Point(3), ms(171)), CursorStep::Moved(Focus::Sound));
    }

    #[test]
    fn the_hover_line_counts_the_pointers_moves() {
        // A sweep at 144 Hz, with keys, unchanged points and Unpoint between: only the
        // pointer's moves count, each as sounded or held back (P = F + S).
        let mut h = game();
        let mut stats = TerminalStats::default();
        let mut sounds = 0;
        let mut inputs: Vec<TerminalInput> = (0..60).map(TerminalInput::Point).collect();
        inputs.extend([TerminalInput::Point(59), TerminalInput::Step(1, 0), TerminalInput::Unpoint, TerminalInput::Step(-1, 0)]);
        for (k, cmd) in inputs.into_iter().enumerate() {
            let CursorStep::Moved(focus) = cursor_step(&mut h, cmd, Duration::from_nanos(k as u64 * 1_000_000_000 / 144)) else { panic!("nothing is entered") };
            stats.count_hover(cmd, focus);
            sounds += u32::from(focus == Focus::Sound && matches!(cmd, TerminalInput::Point(_)));
        }
        assert_eq!(stats.hover_points, 60, "the 60 moves; the unchanged point, the keys and Unpoint do not count");
        assert_eq!(stats.hover_sounds, sounds);
        assert_eq!(stats.hover_points, stats.hover_sounds + stats.hover_suppressed);
        assert!(stats.hover_sounds >= 1 && stats.hover_suppressed >= 1);
    }
}
