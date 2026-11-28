//! The developer console (docs/design/console.md): the key left of 1 (`` ` ``, `~`;
//! [`KeyCode::Backquote`], the key's place whatever the keyboard layout prints on it)
//! opens a command line over the world, as the original's console does [hypothesis:
//! from playing it; nothing of its look is copied]. It is a `PlayState::Menu`: the world
//! and the game clock stand still, the cursor is free (`crate::menu`).
//!
//! - **A line is a script function call** as script source spells it:
//!   `[reference.]Function argument …` (`player.additem 0000000F 100`, `GetAV Health`,
//!   `SunnyRef.MoveTo player`). The function is looked up in the function table by name or
//!   alias; each argument is read by the parameter's encoding (an actor value or another
//!   enumeration by name or number, an axis, a string, else a number or a form: an Editor
//!   ID, a FormID with or without `0x`, `player`). Without `reference.` the call runs on
//!   the selected reference. The call goes through `Game::call_function` inside the
//!   script frame (`crate::script`), so what it changes reaches the world as a script's
//!   changes do; its return value is printed (`>> 100.00`).
//! - **Console commands of its own**: `help [text]` (every record and function whose name
//!   or Editor ID contains the text, not case sensitive; `"two words"` is one phrase), `prid <reference>` (select it), `coc <cell>` (travel to the cell's
//!   COC spot, `crate::player::safe::cell_spot`), `tgm` (god mode, `crate::combat`),
//!   `cls` / `clear` (empty the scrollback), `advlevel` / `AdvancePCLevel` (the player's
//!   level up by one and the level-up menu open, `crate::levelup`; the catalogue gives
//!   it no console number), `poker [casino]` (NVForever's poker table, `crate::poker`),
//!   `tcg [radius]` / ToggleCollisionGeometry (the collision shapes drawn in the world,
//!   `crate::collision_view`; not in the catalogue).
//! - **Console-only functions** (no opcode: `sq`, `sqt`, `movetoqt`, `sqs`, `sqv`, `sv`,
//!   `caqs`, `inv`, `getavinfo`, `setgs`, `cam`, `qqq` …) run from
//!   `fnv_game::functions::console`; what they ask of the engine (travel, quit, saving)
//!   comes back as [`ConsoleRequest`]s. `set <var> to <value>` sets a variable or global.
//! - **The world and render toggles** (`tcl`, `tai`, `tdetect`, `tgm`, `tscr`, `tg`, `ts`,
//!   `tws`, `tt`, `ToggleLODLand`, `ToggleNavmesh`) are functions of the table
//!   (`fnv_game::functions::toggles`): the game switches its own and asks the engine for
//!   the rest, which [`replies`] switches (collision: `crate::player::NoClip`; god mode)
//!   or hands to the renderer ([`RenderToggles`]); each prints its new state.
//! - **Click a thing in the world** while it is open: the reference under the pointer
//!   (the collision ray, else the render bounds, as the crosshair picks) is selected and
//!   its details printed and kept over the scrollback, and a box drawn around it: FormID,
//!   Editor ID, base form, cell, position, enabled, script, health of an actor. The
//!   mouse wheel then steps the selection along that click's ray: up to the next
//!   reference further away (behind walls too), down to the next nearer, round to the
//!   other end past either end ([`along_ray`]). Clicking the selected reference again
//!   unselects it.
//! - Keys: Enter runs the line, Up/Down recall earlier lines, Page Up/Down scroll, `` ` ``
//!   or Esc close it.
//! - `editor` closes the console and switches editor mode on or off (`crate::editor`).
//! - **Several statements on a line** (2026-10-19) [design; docs/design/console.md]: `;`
//!   outside quotes ends a statement, so `player.additem f 100; tgm; coc Goodsprings` runs
//!   three, in order, each echoed and answered as a line of its own ([`statements`]); the
//!   whole line is one entry of the history. `close` among them closes the console and
//!   drops the rest.
//!
//! Log: `console: > <line>` and `console: <reply line>` for each line. Test aid
//! `NVF_PLAY_TEST_CONSOLE=<seconds>:<line>[;<seconds>:<line>…]` opens the console and runs
//! the line at that time (a `;` not followed by `<seconds>:` stays in the line, so a line
//! can hold several statements; one line a frame, and none while the script frame has not
//! yet answered the last, so a line never runs ahead of the one before it, #3274) (on the world aids' clock, `crate::pause::WorldAidClock`: with
//! `NVF_PLAY_AIDS_FROM_SCRIPTS=1` the lines wait for the scripts to run); the line `click <x> <y>` clicks at that
//! fraction of the window (0..1 from the top left), `wheel <steps>` turns the wheel (up
//! positive), `close` closes the console.

use crate::activate::pick::{self, RayHit};
use crate::phys::PhysState;
use crate::script::{Runtime, ScriptState};
use crate::sim::SimState;
use crate::{space, PlayData, PlaySet, PlayState, PlayWorld, PlayerCamera, Travel};
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use crate::ui_pointer::PointerState;
use bevy::window::PrimaryWindow;
use fnv_game::functions::console::{ConsoleCtx, ConsoleRequest};
use fnv_phys::{layers, Filter, Owner};
use fnv_script::vm::Value;
use fnv_sim::FormId;

/// Lines kept in the scrollback.
const SCROLLBACK: usize = 400;
/// Lines shown at once.
const SHOWN: usize = 18;
/// How far a click picks, game units [design: about a large interior or a town block].
const CLICK_REACH: f32 = 20_000.0;
/// Pixels of a touchpad scroll that make one wheel step [design].
const WHEEL_PX: f32 = 40.0;
/// Most functions `help` lists.
const HELP_FUNCTIONS: usize = 25;
/// Most records `help` lists; the rest are counted.
const HELP_RECORDS: usize = 200;

/// A line for the script frame to run (`crate::script::run_scripts`).
#[derive(Debug, Clone)]
pub struct ConsoleCall {
    pub line: String,
    /// The selected reference: what the call runs on without `reference.`.
    pub selected: Option<u32>,
    /// The Pip-Boy's active quest (`sqt`, `movetoqt`).
    pub active_quest: Option<u32>,
}

/// What the script frame answers to a [`ConsoleCall`].
#[derive(Debug, Clone, Default)]
pub struct ConsoleReply {
    pub lines: Vec<String>,
    /// `prid`: the reference to select.
    pub select: Option<u32>,
    /// What console-only commands asked of the engine (`fnv_game::functions::console`).
    pub requests: Vec<ConsoleRequest>,
}

/// The console's systems, for systems that must see a frame's lines first (`crate::editor`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConsoleSet;

/// The console's state.
#[derive(Resource, Default)]
pub struct Console {
    pub open: bool,
    /// The pause menu's Console entry asked for it (the player's Replace Help With Console
    /// tweak): it opens as soon as the world is played again.
    pub open_asked: bool,
    input: String,
    history: Vec<String>,
    /// The history line being shown (Up/Down), counted from the newest.
    recall: Option<usize>,
    lines: Vec<String>,
    /// Lines scrolled back from the newest.
    scroll: usize,
    pub selected: Option<u32>,
    /// The selected reference's details, for the top of the screen.
    selected_text: String,
    /// The references the last click's ray passed through, nearest first: the mouse
    /// wheel steps the selection along them ([`step_along`]).
    along: Vec<u32>,
    /// `rbat`: the statements recorded so far, while it records.
    recording: Option<Vec<String>>,
    /// `SetConsoleOuputFile`: the file every console line is added to.
    out_file: Option<std::path::PathBuf>,
    /// The player's tweaks (`crate::tweaks`): a line said again right after itself
    /// becomes `line -#n`; every line starts with the time since the game started.
    pub compress_repeats: bool,
    pub timestamps: bool,
    /// The last line printed and how many times running, for `compress_repeats`.
    repeat: Option<(String, u32)>,
    /// The player's Remember Console History tweak: the history is read once from
    /// [`HISTORY_FILE`] in NVForever's folder and written back as lines are entered [design: docs/design/tweaks.md].
    pub remember_history: bool,
    history_loaded: bool,
    /// The player's Console Output File tweak has set the file this session.
    pub auto_output_done: bool,
    /// The player's Set Home tweak: `sethome`, `home`, `back` and `delhome` work, the
    /// homes kept for this session [design: docs/design/tweaks.md].
    pub homes_on: bool,
    homes: std::collections::BTreeMap<String, Travel>,
    /// Where the last `home` or `back` left from.
    back: Option<Travel>,
}

impl Console {
    /// [design, #2403 / file refs] Local-only editing/navigation fingerprint. Console
    /// text stays in the hash; no command or output is attached to a report.
    pub fn progress(&self) -> Option<impl std::fmt::Debug + '_> {
        self.open.then_some((&self.input, self.recall, self.scroll, self.selected, &self.lines))
    }
}

/// When the console's clock starts: the first line printed (the game's start, near
/// enough) [design: docs/design/tweaks.md].
static CLOCK: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// `[mm:ss]` since [`CLOCK`] started, `[h:mm:ss]` past an hour.
fn stamp(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 { format!("[{h}:{m:02}:{s:02}] ") } else { format!("[{m:02}:{s:02}] ") }
}

/// The text a line said `n` times running shows (`n` from 2) [design: docs/design/tweaks.md].
fn repeated(line: &str, n: u32) -> String {
    format!("{line} -#{n}")
}

/// Render toggles the console asked for, for the renderer (nvforever) to switch and
/// report with [`Console::say`].
#[derive(Resource, Default)]
pub struct RenderToggles(pub Vec<fnv_game::functions::toggles::Toggle>);

/// `Show1stPerson` (`s1st`): the first-person model is shown in third person too, where
/// the player's eye is (nvforever's `player_body` reads it).
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowFirstPerson(pub bool);

/// How long a `SetVel` along the ground carries an actor, seconds: it is a knockback
/// (`crate::combat::Knockback`) starting at the speed asked and slowing evenly to rest,
/// since an actor's walking sets its ground speed every step [design].
pub const SET_VEL_SECS: f32 = 0.5;

/// `SetVel`'s answer: the player's or an actor's character goes up or down at `speed`,
/// or is pushed along the ground ([`SET_VEL_SECS`]); a loose object's physics bodies take
/// the speed on that axis.
pub fn set_vel(id: u32, axis: char, speed: f32, phys: Option<&mut crate::phys::PhysState>, knocks: Option<&mut Messages<crate::combat::Knockback>>) -> String {
    let who = FormId(id);
    let Some(p) = phys else { return "SetVel: no physics in this run".into() };
    let i = match axis {
        'X' => 0,
        'Y' => 1,
        _ => 2,
    };
    let ch = if id == FormId::PLAYER_REF.0 { p.player.as_mut() } else { p.npcs.get_mut(&id).map(|n| &mut n.ch) };
    match ch {
        Some(ch) if i == 2 => {
            ch.vertical_speed = speed;
            ch.grounded = false;
            format!("SetVel: {who} {} at {speed:.0} units/s", if speed >= 0.0 { "up" } else { "down" })
        }
        Some(_) => {
            let mut dir = [0.0f32; 2];
            dir[i] = speed.signum();
            match (speed != 0.0, knocks) {
                (false, _) => "SetVel: 0 along the ground pushes nothing".into(),
                (true, None) => "SetVel: no pushes in this run".into(),
                (true, Some(k)) => {
                    k.write(crate::combat::Knockback { form_id: id, dir, speed: speed.abs(), secs: SET_VEL_SECS, attacker: 0, blow: 0, power: false });
                    format!("SetVel: {who} pushed along {axis} at {speed:.0} units/s, slowing to rest over {SET_VEL_SECS} s")
                }
            }
        }
        None => match p.world.set_ref_velocity(id, i, speed) {
            0 => format!("SetVel: {who} has no physics body that moves"),
            n => format!("SetVel: {who} at {speed:.0} units/s along {axis} ({n} bodies)"),
        },
    }
}

impl Console {
    /// Print a reply line (and log it).
    pub fn say(&mut self, line: impl Into<String>) {
        self.print(line);
    }

    fn print(&mut self, line: impl Into<String>) {
        let mut line = line.into();
        let since = CLOCK.get_or_init(std::time::Instant::now).elapsed().as_secs();
        info!("console: {line}");
        // The Compress Repeats tweak: a single line said again replaces itself.
        if self.compress_repeats && !line.contains('\n') {
            if let Some((last, n)) = self.repeat.as_mut().filter(|(last, _)| *last == line) {
                *n += 1;
                let shown = repeated(last, *n);
                if let Some(l) = self.lines.last_mut() {
                    *l = if self.timestamps { format!("{}{shown}", stamp(since)) } else { shown };
                }
                self.scroll = 0;
                return;
            }
            self.repeat = Some((line.clone(), 1));
        } else {
            self.repeat = None;
        }
        if self.timestamps {
            line = format!("{}{line}", stamp(since));
        }
        if let Some(path) = &self.out_file {
            use std::io::Write;
            let added = std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut f| writeln!(f, "{line}"));
            if let Err(e) = added {
                warn!("console: {}: {e}; no longer written", path.display());
                self.out_file = None;
            }
        }
        self.lines.push(line);
        if self.lines.len() > SCROLLBACK {
            let extra = self.lines.len() - SCROLLBACK;
            self.lines.drain(..extra);
        }
        self.scroll = 0;
    }
}

/// `NVF_PLAY_TEST_CONSOLE`: (seconds, line), soonest first.
#[derive(Resource, Default)]
struct ConsoleAid(Vec<(f32, String)>);

/// The statements of a console line: split at `;` outside `"…"`, trimmed, empty ones
/// left out.
pub fn statements(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut quoted) = (String::new(), false);
    for ch in line.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                cur.push(ch);
            }
            ';' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Batch files one console line may run, those they run included (a file running itself
/// stops there) [design].
pub const MAX_BATCHES: usize = 16;

/// Where `bat` looks for its file: NVForever's own folder (`Documents\My Games\NVForever`),
/// then the game's folder, which holds the original's batch files ("a text file in game
/// folder" [documented: GECK wiki Bat]); both are only read.
fn batch_roots(data: Option<&nvf_assets::GameData>) -> Vec<std::path::PathBuf> {
    let home = fnv_install::EngineHome::default_root();
    let game = data.and_then(|d| d.vfs.data_dir().parent().map(std::path::Path::to_path_buf));
    home.into_iter().chain(game).collect()
}

/// `bat <file>` (docs/design/console.md): the console lines of a text file, found in the
/// first of `roots` that has it (or at the path given, when it is absolute), `.txt` added
/// to a name without an extension [hypothesis: `bat mybatfile` reads mybatfile.txt, as
/// players use it]. Blank lines are left out.
pub fn batch_lines(name: &str, roots: &[std::path::PathBuf]) -> Result<(std::path::PathBuf, Vec<String>), String> {
    use std::path::{Path, PathBuf};
    let name = name.trim().trim_matches('"');
    if name.is_empty() {
        return Err("bat <file>: runs the console lines of a text file in NVForever's folder or the game's (.txt when no extension is given)".into());
    }
    let file = if Path::new(name).extension().is_some() { name.to_string() } else { format!("{name}.txt") };
    let path = if Path::new(&file).is_absolute() { Some(PathBuf::from(&file)).filter(|p| p.is_file()) } else { roots.iter().map(|r| r.join(&file)).find(|p| p.is_file()) };
    let shown: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
    let path = path.ok_or_else(|| format!("No batch file {file} (looked in {})", shown.join(", ")))?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let lines = String::from_utf8_lossy(&bytes).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    Ok((path, lines))
}

/// Most statements `rbat` records [documented: GECK wiki Rbat, "up to 20 console
/// commands"]; the ones after are not recorded [hypothesis].
pub const RBAT_MAX: usize = 20;

/// The console history's file in NVForever's folder, and the most lines it keeps
/// (ours) [design: docs/design/tweaks.md].
pub const HISTORY_FILE: &str = "ConsoleHistory.txt";
const HISTORY_KEPT: usize = 100;
/// The file the Console Output File tweak adds console lines to (ours).
pub const AUTO_OUTPUT_FILE: &str = "ConsoleOutput.txt";

impl Console {
    /// The Remember Console History tweak on or off; on, the saved history comes back
    /// the first time.
    pub fn remember(&mut self, on: bool) {
        self.remember_history = on;
        if !on || self.history_loaded {
            return;
        }
        self.history_loaded = true;
        let Ok(path) = own_file(HISTORY_FILE, fnv_install::EngineHome::default_root().as_deref()) else { return };
        if let Ok(text) = std::fs::read_to_string(&path) {
            let saved: Vec<String> = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
            info!("console: {} history lines read from {}", saved.len(), path.display());
            let mut both = saved;
            both.append(&mut self.history);
            self.history = both;
        }
    }

    /// Write the history's last lines to [`HISTORY_FILE`].
    fn save_history(&self) {
        let Ok(path) = own_file(HISTORY_FILE, fnv_install::EngineHome::default_root().as_deref()) else { return };
        let from = self.history.len().saturating_sub(HISTORY_KEPT);
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
        if let Err(e) = std::fs::write(&path, self.history[from..].join("
")) {
            warn!("console: history not written to {}: {e}", path.display());
        }
    }

    /// The Console Output File tweak: the output file is set once a session, unless one
    /// is set already.
    pub fn auto_output(&mut self) {
        self.auto_output_done = true;
        if self.out_file.is_some() {
            return;
        }
        if let Ok(path) = own_file(AUTO_OUTPUT_FILE, fnv_install::EngineHome::default_root().as_deref()) {
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
            info!("console: lines are added to {} (tweak)", path.display());
            self.out_file = Some(path);
        }
    }
}

/// A file `rbat` or `SetConsoleOuputFile` writes: a plain name in NVForever's own folder
/// (`Documents\My Games\NVForever`), never a path, so nothing is written elsewhere (the
/// game's folder least of all; the original writes into its own [documented: GECK wiki
/// SetConsoleOuputFile]); `.txt` added to a name without an extension, as `bat` reads it.
pub fn own_file(name: &str, home: Option<&std::path::Path>) -> Result<std::path::PathBuf, String> {
    let name = name.trim().trim_matches('"');
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.starts_with('.') {
        return Err(format!("{name:?}: a plain file name, written in NVForever's folder"));
    }
    let home = home.ok_or("No NVForever folder to write in")?;
    let file = if std::path::Path::new(name).extension().is_some() { name.to_string() } else { format!("{name}.txt") };
    Ok(home.join(file))
}

/// Whether an aid part starts with `<seconds>:` (else it belongs to the line before).
fn timed_part(p: &str) -> bool {
    p.split_once(':').is_some_and(|(t, _)| t.trim().parse::<f32>().is_ok())
}

fn parse_aid(v: &str) -> Result<Vec<(f32, String)>, String> {
    let mut out = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    for part in v.split(';') {
        match parts.last_mut() {
            Some(last) if !timed_part(part) => {
                last.push(';');
                last.push_str(part);
            }
            _ => parts.push(part.to_string()),
        }
    }
    for part in parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let (t, line) = part.split_once(':').ok_or_else(|| format!("{part:?}: expected <seconds>:<line>"))?;
        let t: f32 = t.trim().parse().map_err(|_| format!("{part:?}: bad seconds"))?;
        out.push((t, line.trim().to_string()));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out)
}

#[derive(Component)]
struct ConsoleRoot;
/// A text of the console: the scrollback, the line being typed, the selected reference.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Part {
    Scroll,
    Input,
    Selected,
}

pub struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        let aid = match std::env::var("NVF_PLAY_TEST_CONSOLE") {
            Ok(v) => parse_aid(&v).unwrap_or_else(|e| {
                warn!("NVF_PLAY_TEST_CONSOLE: {e}");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        app.init_resource::<Console>()
            .insert_resource(ConsoleAid(aid))
            .init_resource::<SearchIndex>()
            .init_resource::<RenderToggles>()
            .init_resource::<ShowFirstPerson>()
            .add_systems(Update, (toggle, typing, search_ready, replies, script_lines, click, draw, outline).chain().in_set(PlaySet::Ui).in_set(ConsoleSet))
            .add_systems(OnExit(PlayState::Menu), close_on_exit);
    }
}

/// Run condition: the console is open.
pub fn console_open(c: Res<Console>) -> bool {
    c.open
}

/// The console takes the keys the world would: whether it is open (for systems that
/// read keys in `PlayState::Menu`).
pub fn no_console(c: Option<Res<Console>>) -> bool {
    !c.is_some_and(|c| c.open)
}

#[allow(clippy::too_many_arguments)]
fn toggle(
    keys: crate::pad::ActionButtons<KeyCode>,
    time: crate::pause::AidTime,
    aid: Res<ConsoleAid>,
    mut c: ResMut<Console>,
    state: Res<State<PlayState>>,
    active: Res<crate::menu::ActiveMenu>,
    loading: Res<crate::loading::Loading>,
    mut next: ResMut<NextState<PlayState>>,
    mut commands: Commands,
    roots: Query<Entity, With<ConsoleRoot>>,
    free_cam: Res<crate::free_cam::FreeCam>,
    (tweaks, bindings, mouse): (Option<Res<crate::tweaks::TweakSettings>>, Option<Res<crate::bindings::Bindings>>, crate::pad::ActionButtons<MouseButton>),
) {
    let keys = keys.snapshot();
    let mouse = mouse.snapshot();
    // The Console binding (Controls > Additional Features), ` unless the player rebinds it.
    let pressed = crate::bindings::binding_just_pressed(bindings.as_deref(), crate::bindings::Control::Console, |k| keys.just_pressed(k), |m| mouse.just_pressed(m));
    let by_aid = aid.0.first().is_some_and(|(t, _)| time.world_secs().is_some_and(|now| *t <= now));
    if !c.open {
        // The free camera's mode 1 holds the world in `Menu` with no menu open (`crate::free_cam`).
        let world = *state.get() == PlayState::Playing || (free_cam.holds_world() && *state.get() == PlayState::Menu);
        let free = world && active.0.is_none() && !loading.active();
        if free && (pressed || by_aid || c.open_asked) {
            c.open = true;
            c.open_asked = false;
            // Already `Menu` under the free camera: no change of state, which would close it.
            NextState::set_if_different(&mut *next, PlayState::Menu);
            spawn(&mut commands, tweaks.as_ref().is_some_and(|t| t.console_text_shadow));
            info!("console: open");
        }
        return;
    }
    if pressed || keys.just_pressed(KeyCode::Escape) {
        close(&mut c, &mut next, &mut commands, &roots);
    }
}

fn close(c: &mut Console, next: &mut NextState<PlayState>, commands: &mut Commands, roots: &Query<Entity, With<ConsoleRoot>>) {
    c.open = false;
    c.input.clear();
    c.recall = None;
    next.set(PlayState::Playing);
    for e in roots {
        commands.entity(e).despawn();
    }
    info!("console: close");
}

/// Something else left the menu state (a travel, a dialogue): the console goes too.
/// What scripts printed and the script errors of this frame, on the console as in the
/// original (PrintC writes there) [documented: PrintToConsole].
fn script_lines(mut c: ResMut<Console>, mut script: Option<ResMut<crate::script::ScriptState>>) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    for line in std::mem::take(&mut rt.game.console_out) {
        c.print(line);
    }
}

fn close_on_exit(mut c: ResMut<Console>, mut commands: Commands, roots: Query<Entity, With<ConsoleRoot>>) {
    if c.open {
        c.open = false;
        info!("console: closed by a change of state");
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn typing(
    (mut typed, switching): (MessageReader<KeyboardInput>, Option<Res<crate::pad::SwitchingInput>>),
    time: crate::pause::AidTime,
    mut aid: ResMut<ConsoleAid>,
    mut c: ResMut<Console>,
    mut script: Option<ResMut<ScriptState>>,
    sim: Res<SimState>,
    mut travel: MessageWriter<crate::loading::TravelRequest>,
    mut next: ResMut<NextState<PlayState>>,
    mut commands: Commands,
    roots: Query<Entity, With<ConsoleRoot>>,
    mut clicks: Local<Vec<Vec2>>,
    (mut index, play, mut poker, location): (ResMut<SearchIndex>, Res<PlayWorld>, MessageWriter<crate::poker::OpenPoker>, Option<Res<crate::Location>>),
    (mut saves, mut loads, save_cfg, mut save_log): (MessageWriter<crate::save::SaveRequest>, MessageWriter<crate::save::LoadRequest>, Res<crate::save::SaveConfig>, ResMut<crate::save::SaveLog>),
    (pipboy, data, player): (Option<Res<crate::pipboy::data::DataState>>, Option<Res<nvf_assets::GameData>>, Option<Res<crate::player::Player>>),
    (mut view, mut editor): (ResMut<crate::collision_view::CollisionView>, ResMut<crate::editor::Editor>),
) {
    let typed: Vec<KeyboardInput> = typed.read().filter(|e| switching.as_ref().is_none_or(|s| s.allows(e))).cloned().collect();
    if !c.open {
        return;
    }
    index.start(&play.0, sim.refs);
    for l in std::mem::take(&mut save_log.0) {
        c.print(l);
    }
    let mut submit: Vec<String> = Vec::new();
    let mut wheel_steps = 0i32;
    // A world aid (`crate::pause::WorldAidClock`): with NVF_PLAY_AIDS_FROM_SCRIPTS it
    // waits for the scripts, which most lines need.
    // One line a frame, and only once the script frame has answered the last one sent
    // (#3274: `prid` then `damageobject` 0.2 s later ran the second before the first had
    // selected its reference when that frame was slow).
    let now = time.world_secs();
    let answering = script.as_deref().is_some_and(|st| !st.console.is_empty() || !st.console_out.is_empty());
    if !answering && aid.0.first().is_some_and(|(t, _)| now.is_some_and(|now| *t <= now)) {
        let (t, line) = aid.0.remove(0);
        let late = now.map_or(0.0, |now| now - t);
        if late > 0.1 {
            info!("test aid NVF_PLAY_TEST_CONSOLE: {line} ({late:.2} s late: the last line was still being answered)");
        } else {
            info!("test aid NVF_PLAY_TEST_CONSOLE: {line}");
        }
        submit.push(line);
    }
    for k in typed.iter().filter(|k| k.state.is_pressed()) {
        // The key that opens and closes the console types nothing.
        if k.key_code == KeyCode::Backquote {
            continue;
        }
        match &k.logical_key {
            Key::Enter => submit.push(std::mem::take(&mut c.input)),
            Key::Backspace => {
                c.input.pop();
            }
            Key::ArrowUp | Key::ArrowDown => {
                let n = c.history.len();
                if n == 0 {
                    continue;
                }
                let up = k.logical_key == Key::ArrowUp;
                c.recall = match (c.recall, up) {
                    (None, true) => Some(0),
                    (None, false) => None,
                    (Some(i), true) => Some((i + 1).min(n - 1)),
                    (Some(0), false) => None,
                    (Some(i), false) => Some(i - 1),
                };
                c.input = c.recall.map(|i| c.history[n - 1 - i].clone()).unwrap_or_default();
            }
            Key::PageUp => c.scroll = (c.scroll + SHOWN / 2).min(c.lines.len().saturating_sub(SHOWN)),
            Key::PageDown => c.scroll = c.scroll.saturating_sub(SHOWN / 2),
            _ => {
                if let Some(t) = k.text.as_deref().filter(|t| !t.is_empty() && t.chars().all(|ch| !ch.is_control() && ch != '`') && fnv_util::text::encodable(t)) {
                    c.input.push_str(t);
                }
            }
        }
    }
    // Each line once in the history, then its statements one by one.
    let mut runs: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    for line in submit {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        c.recall = None;
        if c.history.last() != Some(&line) {
            c.history.push(line.clone());
            if c.remember_history {
                c.save_history();
            }
        }
        let parts = statements(&line);
        if parts.len() > 1 {
            info!("console: {} statements on the line", parts.len());
        }
        runs.extend(parts);
    }
    let mut batches = 0;
    while let Some(line) = runs.pop_front() {
        c.print(format!("> {line}"));
        let (head, rest) = line.split_once(char::is_whitespace).map_or((line.as_str(), ""), |(h, r)| (h, r.trim()));
        let lower = head.to_ascii_lowercase();
        if lower != "rbat" {
            if let Some(rec) = c.recording.as_mut().filter(|r| r.len() < RBAT_MAX) {
                rec.push(line.clone());
            }
        }
        // Where the player stands now, as a travel back to it (the Set Home tweak).
        let here = match (location.as_deref(), player.as_deref()) {
            (Some(l), Some(p)) => Some(Travel { world: l.interior.is_none().then_some(l.world), interior: l.interior, feet: p.feet, heading_deg: p.heading.to_degrees(), pitch_deg: None }),
            _ => None,
        };
        let name = if rest.is_empty() { "home".to_string() } else { rest.to_ascii_lowercase() };
        match lower.as_str() {
            "cls" | "clear" | "clr" => c.lines.clear(),
            // Joint progression's debug words (`fnv_game::coop_joint`, coop-shared-progress.md §3.2).
            "coop.presence" | "coop.ledger" => match script.as_deref().and_then(|s| s.rt.as_ref()) {
                None => c.print(format!("{head}: scripts are not running")),
                Some(rt) if lower == "coop.ledger" => rt.game.coop_ledger_lines().into_iter().for_each(|l| c.print(l)),
                Some(rt) => match fnv_game::functions::console::form(&rt.game, rest) {
                    Ok(q) => rt.game.coop_presence_lines(q).into_iter().for_each(|l| c.print(l)),
                    Err(e) => c.print(format!("coop.presence <quest>: {e}")),
                },
            },
            "sethome" if c.homes_on => match here {
                Some(t) => {
                    c.print(format!("Home \"{name}\" set here"));
                    c.homes.insert(name, t);
                }
                None => c.print("sethome: the player is not in the world"),
            },
            "delhome" if c.homes_on => match c.homes.remove(&name) {
                Some(_) => c.print(format!("Home \"{name}\" removed")),
                None => c.print(format!("No home \"{name}\"")),
            },
            "home" if c.homes_on => match c.homes.get(&name).copied() {
                Some(t) => {
                    c.back = here;
                    c.print(format!("Travelling to home \"{name}\""));
                    travel.write(crate::loading::TravelRequest(t));
                }
                None if c.homes.is_empty() => c.print("No homes yet: sethome [name] sets one here"),
                None => {
                    let names: Vec<String> = c.homes.keys().cloned().collect();
                    c.print(format!("No home \"{name}\"; homes: {}", names.join(", ")));
                }
            },
            "back" if c.homes_on => match c.back.take() {
                Some(t) => {
                    c.back = here;
                    c.print("Travelling back");
                    travel.write(crate::loading::TravelRequest(t));
                }
                None => c.print("back: no home or back travel yet"),
            },
            // `rbat`: start or cancel recording; `rbat <file>`: the recording written for
            // `bat` (docs/design/console.md).
            "rbat" => match (rest.is_empty(), c.recording.take()) {
                (true, None) => {
                    c.recording = Some(Vec::new());
                    c.print(format!("Recording console lines (up to {RBAT_MAX}); rbat <file> saves them, rbat cancels"));
                }
                (true, Some(_)) => c.print("Recording cancelled"),
                (false, None) => c.print("rbat first starts a recording"),
                (false, Some(rec)) => match own_file(rest, fnv_install::EngineHome::default_root().as_deref()) {
                    Ok(path) => {
                        let text: String = rec.iter().map(|l| format!("{l}\n")).collect();
                        match std::fs::create_dir_all(path.parent().unwrap_or(&path)).and_then(|_| std::fs::write(&path, text)) {
                            Ok(()) => c.print(format!("{} lines saved to {}", rec.len(), path.display())),
                            Err(e) => c.print(format!("{}: {e}", path.display())),
                        }
                    }
                    Err(e) => {
                        c.print(e);
                        c.recording = Some(rec);
                    }
                },
            },
            // `SetConsoleOuputFile <file>` (`scof`, the catalogue's spelling): every console
            // line from now on is added to the file; without a name, no longer.
            "setconsoleouputfile" | "scof" => {
                if rest.is_empty() {
                    c.out_file = None;
                    c.print("Console lines no longer written to a file");
                } else {
                    match own_file(rest, fnv_install::EngineHome::default_root().as_deref()) {
                        Ok(path) => {
                            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
                            c.print(format!("Console lines are added to {}", path.display()));
                            c.out_file = Some(path);
                        }
                        Err(e) => c.print(e),
                    }
                }
            }
            "help" => {
                let terms = query_terms(rest);
                if terms.is_empty() {
                    c.print("A line is a script function, declaration, or expression: [reference.]Function argument ..., int x = 5, player.GetAV Health + 10");
                    c.print("eval <script expression>: print a number, form, quoted string, or array preview (bare expressions print results directly too)");
                    c.print("help <text>: records and functions whose name or Editor ID contains it (not case sensitive); \"two words\" for a phrase, several words must all match");
                    c.print("prid <ref>, coc <cell>, cow <world> <x> <y>, coe <x> <y>, tgm, cls, save <name>, load [name], saves, set <var> to <value>, editor (editor mode on/off), editor select|move|turn|place|delete|undo, editor save <Name.esp>, editor why [topic] (why the selected actor says what it says). Click something to select it; calls without a reference run on it.");
                    c.print("Quests and info: sq, sqt, movetoqt [n], sqs <quest>, sqv <quest>, sv, caqs, inv, getavinfo <av>, setgs <setting> <value>, cam, qqq, advlevel (level up), poker [casino] (a poker table)");
                    c.print("Toggles: tcl (collision), tai (actors' AI), tdetect, tgm, tscr (scripts), tg (grass), ts (sky), tws (water), tt (trees), tlv (leaves), ToggleLODLand, ToggleNavmesh, tlik (head tracking), temo (emotions), tcg [radius] (collision shapes)");
                    c.print("bat <file> (console lines from a text file), rbat [file] (record lines for bat), scof [file] (write the console to a file), slpm [1-5] (a practice lock), [ref.]mfg expression|phoneme|modifier <id> <0-100>, mfg reset");
                    c.print("plm <x> <y> <z> [name] [0-9] (a marker seen through walls), clm (clear them), s1st (first-person model in third person), [ref.]setvel x|y|z <speed>, tst <global>, prc, tlv (tree leaves), ClearScreenBlood");
                } else {
                    let funcs = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| matching_functions(&rt.game.data.table, &terms)).unwrap_or_default();
                    if !funcs.is_empty() {
                        let more = funcs.len().saturating_sub(HELP_FUNCTIONS);
                        c.print(format!("Functions: {}{}", funcs[..funcs.len().min(HELP_FUNCTIONS)].join(", "), if more > 0 { format!(" (+{more})") } else { String::new() }));
                    }
                    match index.ready.as_ref() {
                        Some(ix) => print_records(&mut c, ix, &terms, rest),
                        None => {
                            c.print("Reading every record for the search (the first time only)...");
                            index.waiting.push(rest.to_string());
                        }
                    }
                }
            }
            "close" => {
                close(&mut c, &mut next, &mut commands, &roots);
                return;
            }
            // `editor save [<name>]`: the moved references into a plugin (the target when
            // no name is given); `editor open [<name>]`: list the plugins or pick the
            // target (`crate::editor`).
            "editor" if crate::editor::verb_is(rest, "save") => editor.save_asked = Some(rest[4..].trim().to_string()),
            "editor" if crate::editor::verb_is(rest, "open") => editor.open_asked = Some(rest[4..].trim().to_string()),
            // `editor why [<topic>]`: the dialogue debugger, read-only (`crate::editor::why`).
            "editor" if crate::editor::verb_is(rest, "why") => match script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
                _ if !editor.on => c.print("Editor mode is off: type editor first"),
                Some(rt) => match crate::editor::why(&mut rt.game, editor.selected.or(c.selected), &rest[3..]) {
                    Ok(lines) => lines.into_iter().for_each(|l| c.print(l)),
                    Err(e) => c.print(e),
                },
                None => c.print("editor why: scripts are not running"),
            },
            "editor" if crate::editor::verb_is(rest, "find") => match rest[4..].trim() {
                "" => c.print("editor find <words>: the placeable records named like that"),
                q => editor.find_asked = Some(q.to_string()),
            },
            "editor" if crate::editor::Op::parse(rest).is_some() => match crate::editor::Op::parse(rest) {
                Some(Ok(op)) if editor.on => editor.ops.push(op),
                Some(Ok(_)) => c.print("Editor mode is off: type editor first"),
                Some(Err(e)) => c.print(e),
                None => {}
            },
            // `editor place <#n | Editor ID>` (a FormID is an `Op::Place`).
            "editor" if crate::editor::verb_is(rest, "place") && editor.on => editor.place_named = Some(rest[5..].trim().to_string()),
            // Editor mode (`crate::editor`) takes the screen: the console closes.
            "editor" => {
                editor.asked = true;
                c.print(if editor.on { "Leaving editor mode" } else { "Editor mode: type editor in the console again to leave" });
                close(&mut c, &mut next, &mut commands, &roots);
                return;
            }
            // `bat <file>`: the file's lines run next, in order, as if typed.
            "bat" => {
                batches += 1;
                let found = if batches > MAX_BATCHES { Err(format!("bat: more than {MAX_BATCHES} batch files on one line")) } else { batch_lines(rest, &batch_roots(data.as_deref())) };
                match found {
                    Ok((path, lines)) => {
                        c.print(format!("Running {} ({} lines)", path.display(), lines.len()));
                        for l in lines.iter().rev() {
                            for st in statements(l).into_iter().rev() {
                                runs.push_front(st);
                            }
                        }
                    }
                    Err(e) => c.print(e),
                }
            }
            "save" => match fnv_save::check_manual_name(rest) {
                Ok(n) => {
                    saves.write(crate::save::SaveRequest { name: n.to_string(), kind: fnv_save::SaveKind::Manual });
                }
                Err(e) => c.print(e.to_string()),
            },
            "load" => match save_cfg.path_of(if rest.is_empty() { fnv_save::QUICK_NAME } else { rest }) {
                Ok(p) if p.exists() => {
                    c.print(format!("Loading {}", p.display()));
                    loads.write(crate::save::LoadRequest { path: p });
                }
                Ok(p) => c.print(format!("No save {}", p.display())),
                Err(e) => c.print(e),
            },
            "saves" => {
                let list = save_cfg.dir.as_deref().map(fnv_save::list).unwrap_or_default();
                if list.is_empty() {
                    c.print("No saves yet");
                }
                for (p, h) in list.iter().take(30) {
                    let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    c.print(format!("{name}: {} level {}, {}, {} ({:?})", h.player_name, h.level, h.location, crate::save::game_date(h), h.kind));
                }
            }
            // With scripts running, `tgm` is ToggleGodMode of the table (below).
            "tgm" if !script.as_deref().is_some_and(|s| s.rt.is_some()) => {
                let on = !crate::combat::god_mode();
                crate::combat::set_god_mode(on);
                c.print(format!("God mode {}", if on { "on" } else { "off" }));
            }
            // ToggleCollisionGeometry: the collision view (`crate::collision_view`); with a
            // radius, on within that many units of the camera.
            "tcg" | "togglecollisiongeometry" => match rest.split_whitespace().next().map(str::parse::<f32>) {
                None => {
                    let on = !view.on;
                    view.set(on, None);
                    c.print(view.state());
                }
                Some(Ok(r)) if r.is_finite() && r > 0.0 => {
                    view.set(true, Some(r));
                    c.print(view.state());
                }
                Some(_) => c.print("tcg [radius]: the collision shapes within radius game units of the camera"),
            },
            "click" => {
                let xy: Vec<f32> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                match xy[..] {
                    [x, y] => clicks.push(Vec2::new(x, y)),
                    _ => c.print("click <x> <y>: fractions of the window"),
                }
            }
            "wheel" => match rest.trim().parse::<i32>() {
                Ok(n) => wheel_steps += n,
                Err(_) => c.print("wheel <steps>: up (further) is positive"),
            },
            // NVForever's own poker table (docs/design/poker.md §6); the console closes so
            // the table opens on the world.
            "poker" => match crate::poker::Casino::from_letter(rest) {
                Some(casino) => {
                    c.print(format!("Opening the poker table at {}", casino.title()));
                    poker.write(crate::poker::OpenPoker { casino, table: None });
                    close(&mut c, &mut next, &mut commands, &roots);
                    return;
                }
                None => c.print("poker [t|g|u|l|b]: The Tops, Gomorrah, Ultra-Luxe, Lucky 38 or a plain deck"),
            },
            "coc" => match coc(&sim, rest) {
                Ok((t, name)) => {
                    c.print(format!("Travelling to {name}"));
                    travel.write(crate::loading::TravelRequest(t));
                }
                Err(e) => c.print(e),
            },
            "cow" | "centeronworld" | "coe" | "centeronexterior" => {
                let words: Vec<&str> = rest.split_whitespace().collect();
                let here = location.as_deref().map_or(play.0.world, |l| l.world);
                let asked = match (matches!(head.to_ascii_lowercase().as_str(), "cow" | "centeronworld"), &words[..]) {
                    (true, [w, x, y]) => world_word(&play.0, w).map(|w| (w, *x, *y)),
                    (true, _) => Err("cow <worldspace> <x> <y>: an exterior cell by its grid square".to_string()),
                    (false, [x, y]) => Ok((here, *x, *y)),
                    (false, _) => Err("coe <x> <y>: an exterior cell of the current worldspace by its grid square".to_string()),
                };
                match asked.and_then(|(w, x, y)| center_on(&play.0, w, x, y)) {
                    Ok((t, name)) => {
                        c.print(format!("Travelling to {name}"));
                        travel.write(crate::loading::TravelRequest(t));
                    }
                    Err(e) => c.print(e),
                }
            }
            _ => match script.as_deref_mut() {
                Some(st) if st.rt.is_some() => {
                    let active_quest = pipboy.as_ref().and_then(|p| p.active_quest(st.rt.as_ref().map(|rt| &rt.game)));
                    st.console.push(ConsoleCall { line, selected: c.selected, active_quest });
                }
                _ => c.print("Scripts are not running: only cls, tgm and coc work"),
            },
        }
    }
    // Clicks asked by the aid go to [`click`] through the console's queue.
    if !clicks.is_empty() || wheel_steps != 0 {
        commands.insert_resource(AidClicks(std::mem::take(&mut *clicks), wheel_steps));
    }
}

/// Clicks the test aid asked for, as window fractions, and wheel steps (up positive).
#[derive(Resource)]
struct AidClicks(Vec<Vec2>, i32);

/// The script frame's answers, and what console-only commands asked of the engine.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn replies(
    mut c: ResMut<Console>,
    script: Option<ResMut<ScriptState>>,
    sim: Res<SimState>,
    play: Res<PlayWorld>,
    (mut travel, mut saves, mut loads, save_cfg, mut exit): (
        MessageWriter<crate::loading::TravelRequest>,
        MessageWriter<crate::save::SaveRequest>,
        MessageWriter<crate::save::LoadRequest>,
        Res<crate::save::SaveConfig>,
        MessageWriter<AppExit>,
    ),
    (mut next, mut commands, roots): (ResMut<NextState<PlayState>>, Commands, Query<Entity, With<ConsoleRoot>>),
    (mut noclip, mut render, mut practice): (ResMut<crate::player::NoClip>, ResMut<RenderToggles>, Option<ResMut<crate::lockpick::PracticeLock>>),
    (mut markers, location, mut phys, mut knocks, mut first): (
        ResMut<crate::location_markers::LocationMarkers>,
        Option<Res<crate::Location>>,
        Option<ResMut<crate::phys::PhysState>>,
        Option<ResMut<Messages<crate::combat::Knockback>>>,
        ResMut<ShowFirstPerson>,
    ),
) {
    let Some(mut st) = script else { return };
    if let Some(rt) = st.rt.as_mut() {
        use fnv_game::functions::toggles::Toggle;
        let state = |off: bool| if off { "off" } else { "on" };
        for t in std::mem::take(&mut rt.game.toggles.asked) {
            match t {
                Toggle::Collision => {
                    noclip.0 = !noclip.0;
                    c.print(format!("Collision {}", state(noclip.0)));
                }
                Toggle::GodMode => {
                    let on = !crate::combat::god_mode();
                    crate::combat::set_god_mode(on);
                    c.print(format!("God mode {}", state(!on)));
                }
                Toggle::ActorsAI => c.print(format!("Actors' AI {}", state(rt.game.toggles.actors_ai_off))),
                Toggle::Detection => c.print(format!("Detection of the player {}", state(rt.game.toggles.detection_off))),
                Toggle::Scripts => c.print(format!("Scripts {}", state(rt.game.toggles.scripts_off))),
                t => render.0.push(t),
            }
        }
        // The game sees the engine's god mode (tgm, the settings page, `--god`): effects
        // that would damage the player do nothing in it (aid-items.md §3).
        rt.game.toggles.god_mode = crate::combat::god_mode();
    }
    let mut out = std::mem::take(&mut st.console_out);
    // What scripts asked of the engine through console commands (xNVSE's con_QuitGame,
    // con_Save, … in `fnv_game::nvse_console`), taken as a console line's are.
    if let Some(rt) = st.rt.as_mut().filter(|rt| !rt.game.console_requests.is_empty()) {
        out.push(ConsoleReply { requests: std::mem::take(&mut rt.game.console_requests), ..Default::default() });
    }
    for r in out {
        for l in r.lines {
            c.print(l);
        }
        if let Some(id) = r.select {
            select(&mut c, id, &play.0, &sim, st.rt.as_mut());
        }
        for q in r.requests {
            match q {
                ConsoleRequest::Select(id) => select(&mut c, id.0, &play.0, &sim, st.rt.as_mut()),
                ConsoleRequest::Travel { world, interior, feet, heading_deg, label } => {
                    info!("console: travelling to {label}");
                    travel.write(crate::loading::TravelRequest(Travel { world: world.map(|w| w.0), interior: interior.map(|i| i.0), feet, heading_deg, pitch_deg: None }));
                }
                ConsoleRequest::Quit => {
                    info!("console: quit");
                    exit.write(AppExit::Success);
                }
                // The console opens only when no other menu is open, so closing every
                // menu is closing the console [hypothesis: as the original's CloseAllMenus
                // typed into its console].
                ConsoleRequest::CloseMenus => {
                    if c.open {
                        close(&mut c, &mut next, &mut commands, &roots);
                    }
                }
                ConsoleRequest::Save(name) => match fnv_save::check_manual_name(&name) {
                    Ok(n) => {
                        saves.write(crate::save::SaveRequest { name: n.to_string(), kind: fnv_save::SaveKind::Manual });
                    }
                    Err(e) => c.print(e.to_string()),
                },
                ConsoleRequest::Load(name) => match save_cfg.path_of(if name.is_empty() { fnv_save::QUICK_NAME } else { &name }) {
                    Ok(p) if p.exists() => {
                        c.print(format!("Loading {}", p.display()));
                        loads.write(crate::save::LoadRequest { path: p });
                    }
                    Ok(p) => c.print(format!("No save {}", p.display())),
                    Err(e) => c.print(e),
                },
                // ShowLockPickMenu: the console closes, then the menu opens on the
                // practice lock (`crate::lockpick::PracticeLock`).
                ConsoleRequest::Lockpick(level) => match practice.as_deref_mut() {
                    Some(p) => {
                        p.0 = Some(level);
                        if c.open {
                            close(&mut c, &mut next, &mut commands, &roots);
                        }
                    }
                    None => c.print("No lockpicking in this run"),
                },
                ConsoleRequest::PlaceMarker { pos, name, color } => {
                    let (world, interior) = location.as_deref().map_or((play.0.world, None), |l| (l.world, l.interior));
                    markers.0.push(crate::location_markers::LocationMarker { pos, name, color, world, interior });
                }
                ConsoleRequest::ClearMarkers => {
                    let n = markers.clear_here(location.as_deref());
                    c.print(format!("{n} location markers cleared"));
                }
                ConsoleRequest::Show1stPerson => {
                    first.0 = !first.0;
                    c.print(format!("First-person model in third person {}", if first.0 { "shown" } else { "hidden" }));
                }
                ConsoleRequest::SetVel { on, axis, speed } => {
                    let line = set_vel(on.0, axis, speed, phys.as_deref_mut(), knocks.as_deref_mut());
                    c.print(line);
                }
            }
        }
    }
    // Keep the details at the top current (health, position change).
    if let (true, Some(id)) = (c.open, c.selected) {
        let text = describe(id, &play.0, &sim, st.rt.as_mut()).join("   ");
        if c.selected_text != text {
            c.selected_text = text;
        }
    }
}

fn select(c: &mut Console, id: u32, d: &PlayData, sim: &SimState, rt: Option<&mut Runtime>) {
    c.selected = Some(id);
    let lines = describe(id, d, sim, rt);
    c.selected_text = lines.join("   ");
    for l in lines {
        c.print(l);
    }
}

/// A record's signature, Editor ID and name (FULL) from the load order's winning copy.
fn record_names(d: &PlayData, id: FormId) -> (String, Option<String>, Option<String>) {
    let Some((_, rec)) = d.lo.winner(id.0) else { return (String::new(), None, None) };
    let sig = String::from_utf8_lossy(&rec.header.sig).to_string();
    let text = fnv_util::text::decode_z;
    let subs = rec.subrecords().unwrap_or_default();
    let get = |want: &[u8; 4]| subs.iter().find(|s| &s.sig == want).map(|s| text(&s.data)).filter(|t| !t.is_empty());
    (sig, get(b"EDID"), get(b"FULL"))
}

/// A cell's name for a save's header: its Editor ID, else its worldspace and grid.
pub fn place_name(d: &PlayData, cell: u32) -> String {
    let (_, e, n) = record_names(d, FormId(cell));
    n.or(e).unwrap_or_else(|| {
        d.index.cells.get(&cell).and_then(|i| Some((i.world?, i.grid?))).map_or_else(
            || FormId(cell).to_string(),
            |(w, (x, y))| format!("{} ({x}, {y})", record_names(d, FormId(w)).1.unwrap_or_else(|| FormId(w).to_string())),
        )
    })
}

/// The player's Extra Console Details tweak: a selected reference's details add its
/// base's model [design: docs/design/tweaks.md].
static MODEL_DETAILS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set by the Tweaks tab.
pub fn set_model_details(on: bool) {
    MODEL_DETAILS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// A record's model path (MODL), from the load order's winning copy.
fn model_path(d: &PlayData, id: FormId) -> Option<String> {
    let (_, rec) = d.lo.winner(id.0)?;
    let subs = rec.subrecords().unwrap_or_default();
    subs.iter().find(|s| &s.sig == b"MODL").map(|s| fnv_util::text::decode_z(&s.data)).filter(|t| !t.is_empty())
}

/// A reference's details, a line each.
pub fn describe(id: u32, d: &PlayData, sim: &SimState, rt: Option<&mut Runtime>) -> Vec<String> {
    let r = FormId(id);
    if r == FormId::PLAYER_REF {
        return vec![format!("{r} player")];
    }
    let placed = sim.refs.get(r);
    let state = sim.world.peek(r);
    let (rsig, redid, _) = record_names(d, r);
    let edid = redid.or_else(|| placed.and_then(|p| p.edid.clone()));
    let mut out = vec![format!("{r} {rsig}{}", edid.map(|e| format!(" {e}")).unwrap_or_default())];
    let base = state.map(|s| s.base).or(placed.map(|p| p.base));
    if let Some(b) = base {
        let (sig, bedid, name) = record_names(d, b);
        let name = name.map(|n| format!(" \"{n}\"")).unwrap_or_default();
        out.push(format!("base {b} {sig} {}{name}", bedid.unwrap_or_default()));
        if let Some(m) = model_path(d, b).filter(|_| MODEL_DETAILS.load(std::sync::atomic::Ordering::Relaxed)) {
            out.push(format!("model {m}"));
        }
    }
    let cell = state.and_then(|s| s.cell).or(placed.and_then(|p| p.cell));
    let pos = state.map(|s| s.pos).or(placed.map(|p| p.pos));
    if let Some(p) = pos {
        let cell = cell.map(|c| {
            let (_, e, n) = record_names(d, c);
            // An exterior cell rarely has an Editor ID: its worldspace and grid square.
            let grid = d.index.cells.get(&c.0).and_then(|i| Some((i.world?, i.grid?))).map(|(w, (x, y))| {
                let (_, we, _) = record_names(d, FormId(w));
                format!(" {} ({x}, {y})", we.unwrap_or_else(|| FormId(w).to_string()))
            });
            let named = e.map(|e| format!(" {e}")).or(grid).unwrap_or_default();
            format!("{c}{named}{}", n.map(|n| format!(" \"{n}\"")).unwrap_or_default())
        });
        out.push(format!("cell {}  pos {:.0} {:.0} {:.0}", cell.unwrap_or_else(|| "-".into()), p[0], p[1], p[2]));
    }
    if let Some(s) = state {
        out.push(format!("{}{}{}", if s.enabled() { "enabled" } else { "disabled" }, if s.deleted() { ", removed" } else { "" }, if s.count != 1 { format!(", count {}", s.count) } else { String::new() }));
    } else if let Some(p) = placed.filter(|p| sim.forms.item(p.base).is_some()) {
        // Not touched yet: the stack it was placed with.
        let n = sim.forms.placed_count(p.base, p.count);
        if n != 1 {
            out.push(format!("count {n} (as placed)"));
        }
    }
    if let Some(rt) = rt {
        let g = &mut rt.game;
        if let Some(sc) = base.and_then(|b| g.data.script_of_base(b)) {
            out.push(format!("script {sc} {}", g.data.name(sc)));
        }
        let actor = placed.is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE")) || r == FormId::PLAYER_REF;
        if actor {
            let health = fnv_game::functions::actor_value(g, r, fnv_game::actor_values::HEALTH).unwrap_or(0.0);
            let dead = if g.is_dead(r) { ", dead" } else { "" };
            out.push(format!("health {health:.0}{dead}"));
        }
    }
    out
}

/// Where `coc <cell>` goes: the cell's COC spot (interiors and exterior cells alike).
fn coc(sim: &SimState, name: &str) -> Result<(Travel, String), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("coc <cell>".into());
    }
    let refs = sim.refs;
    let cell = refs.cell_edids.get(&name.to_ascii_lowercase()).copied().or_else(|| parse_form_id(name).filter(|c| refs.by_cell.contains_key(c))).ok_or_else(|| format!("No cell named {name}"))?;
    let spot = crate::player::safe::cell_spot(refs, cell.0).ok_or_else(|| format!("{name} has no marker or door to arrive at"))?;
    let world = refs.cell_world.get(&cell).map(|w| w.0);
    let t = Travel { world, interior: if world.is_some() { None } else { Some(cell.0) }, feet: spot.feet, heading_deg: spot.heading.to_degrees(), pitch_deg: None };
    Ok((t, name.to_string()))
}

/// A worldspace by Editor ID (not case sensitive) or FormID.
fn world_word(d: &PlayData, word: &str) -> Result<u32, String> {
    let worlds = &d.index.worldspaces;
    worlds
        .iter()
        .find(|(_, w)| w.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(word)))
        .map(|(&id, _)| id)
        .or_else(|| parse_form_id(word).map(|f| f.0).filter(|f| worlds.contains_key(f)))
        .ok_or_else(|| format!("No worldspace named {word}"))
}

/// `cow <world> <x> <y>` (CenterOnWorld) and `coe <x> <y>` (CenterOnExterior, the current
/// worldspace, the one last walked in from an interior): the player goes to the exterior
/// cell at grid square (x, y) ("moves the player to given exterior grid coordinates"
/// [documented: GECK wiki CenterOnWorld, CenterOnExterior]). Where in the cell is not
/// documented: its centre, on the landscape there [hypothesis] (the run's surface,
/// `crate::smooth_terrain::spec`, as the collision has it), which the arrival rule
/// (physics.md §6.2) then settles as for any travel.
fn center_on(d: &PlayData, world: u32, x: &str, y: &str) -> Result<(Travel, String), String> {
    let grid = |w: &str| w.parse::<i32>().map_err(|_| format!("{w:?} is not a grid number"));
    let g = (grid(x)?, grid(y)?);
    let ws = d.index.worldspaces.get(&world).ok_or_else(|| format!("No worldspace {}", FormId(world)))?;
    let name = format!("{} ({}, {})", ws.edid.clone().unwrap_or_else(|| FormId(world).to_string()), g.0, g.1);
    // Only a cell the worldspace itself has: physics stands on a worldspace's own land,
    // not its parent's (`PhysState::set_world`).
    let cell = ws.grid.get(&g).and_then(|c| d.index.cells.get(c)).ok_or_else(|| format!("No exterior cell at {name}"))?;
    let land = cell.land.and_then(|_| {
        let ids = fnv_world::land_surface::land_ids(g, |n| ws.grid.get(&n).and_then(|c| d.index.cells.get(c)).and_then(|c| c.land));
        fnv_world::land_surface::load_surface(d.lo, d.schema, &ids, crate::smooth_terrain::spec())?.height_at(CELL_CENTRE, CELL_CENTRE)
    });
    // A cell without landscape: the worldspace's default land height.
    let land = land.or(ws.default_land_height).unwrap_or(0.0);
    // Water over the land there: on its surface, not on the bed [hypothesis: where the
    // original puts the player is undocumented].
    let water = cell.has_water().then(|| cell.water_height.or(ws.default_water_height)).flatten().filter(|w| w.abs() < 1.0e6);
    let z = water.map_or(land, |w| land.max(w));
    Ok((Travel { world: Some(world), interior: None, feet: grid_centre(g, z), heading_deg: 0.0, pitch_deg: None }, name))
}

/// Half a cell's width: an exterior cell's centre from its corner.
const CELL_CENTRE: f32 = fnv_world::CELL_SIZE / 2.0;

/// The centre of exterior grid square `g` at height `z`, game space.
fn grid_centre(g: (i32, i32), z: f32) -> [f32; 3] {
    [g.0 as f32 * fnv_world::CELL_SIZE + CELL_CENTRE, g.1 as f32 * fnv_world::CELL_SIZE + CELL_CENTRE, z]
}

/// A FormID as the console takes it: hex with or without `0x`, up to eight digits.
pub fn parse_form_id(s: &str) -> Option<FormId> {
    let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    (!h.is_empty() && h.len() <= 8 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| u32::from_str_radix(h, 16).ok().map(FormId)).flatten()
}

/// A click in the world picks what is under the pointer.
#[allow(clippy::too_many_arguments)]
fn click(
    mouse: crate::pad::ActionButtons<MouseButton>,
    aid: Option<Res<AidClicks>>,
    mut commands: Commands,
    mut c: ResMut<Console>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    phys: Option<ResMut<PhysState>>,
    visuals: crate::PickVisuals,
    sim: Res<SimState>,
    play: Res<PlayWorld>,
    mut script: Option<ResMut<ScriptState>>,
    panel: Query<&PointerState, With<ConsoleRoot>>,
    free_cam: Res<crate::free_cam::FreeCam>,
    (mut wheel, mut wheel_px): (MessageReader<MouseWheel>, Local<f32>),
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    let mouse = mouse.snapshot();
    // The player's tweak: a click never picks the player [design: docs/design/tweaks.md].
    let skip_player = tweaks.is_some_and(|t| t.console_skips_player);
    let pickable = move |id: u32| !(skip_player && id == FormId::PLAYER_REF.0);
    // Read every frame so a turn made while the console is shut does not count later.
    let mut steps = 0i32;
    for w in wheel.read() {
        match w.unit {
            MouseScrollUnit::Line => steps += w.y.round() as i32,
            MouseScrollUnit::Pixel => {
                *wheel_px += w.y;
                let n = (*wheel_px / WHEEL_PX).trunc();
                *wheel_px -= n * WHEEL_PX;
                steps += n as i32;
            }
        }
    }
    if !c.open {
        return;
    }
    let Ok(window) = windows.single() else { return };
    // A click on the console's own panel is not a pick, nor is the wheel turned over it.
    let over_panel = panel.iter().any(|i| *i != PointerState::None);
    if over_panel {
        steps = 0;
    }
    let mut at: Vec<Vec2> = Vec::new();
    if let Some(a) = aid {
        at.extend(a.0.iter().map(|f| Vec2::new(f.x * window.width(), f.y * window.height())));
        steps += a.1;
        commands.remove_resource::<AidClicks>();
    }
    if mouse.just_pressed(MouseButton::Left) && !over_panel {
        at.extend(window.cursor_position());
    }
    let Some(at) = at.last().copied() else {
        // The wheel: up selects the next reference further along the last click's ray,
        // down the next nearer, round to the other end past the last.
        if let Some(id) = c.selected.filter(|_| steps != 0).and_then(|cur| step_along(&c.along, cur, steps)) {
            let n = c.along.len();
            let i = c.along.iter().position(|&a| a == id).unwrap_or(0);
            info!("console: wheel {steps:+} selects {} ({} of {n} along the click)", FormId(id), i + 1);
            select(&mut c, id, &play.0, &sim, script.as_deref_mut().and_then(|s| s.rt.as_mut()));
            c.print(format!("{} of {n} along the click (wheel up: further, down: nearer)", i + 1));
        }
        return;
    };
    let Ok((cam, cam_t)) = camera.single() else { return };
    let Ok(ray) = cam.viewport_to_world(cam_t, at) else { return };
    let (o, d) = (ray.origin, *ray.direction);
    if free_cam.on {
        // The camera is the free camera (`crate::free_cam`): the ray starts there.
        let g = space::to_game(o);
        info!("console: the ray from the free camera at ({:.0}, {:.0}, {:.0})", g[0], g[1], g[2]);
    }
    let Under { hits, bounds, choice, .. } = pick_under(o, d, CLICK_REACH, phys.map(|p| p.into_inner()), &visuals, pickable);
    match choice {
        // Clicking the selected reference again unselects it.
        Some(p) if c.selected == Some(p.form_id) => {
            info!("console: unselected {}", FormId(p.form_id));
            c.selected = None;
            c.selected_text.clear();
            c.along.clear();
            c.print(format!("Unselected {}", FormId(p.form_id)));
        }
        Some(p) => {
            let refs = hits.iter().filter_map(|&(id, d)| Some((id?, d)));
            c.along = along_ray(p.form_id, refs, bounds.iter().copied().filter(|&(_, d)| d <= CLICK_REACH));
            info!("console: picked {} at {:.0} by {:?}; {} along the ray", FormId(p.form_id), p.distance, p.source, c.along.len());
            select(&mut c, p.form_id, &play.0, &sim, script.as_deref_mut().and_then(|s| s.rt.as_mut()));
            let n = c.along.len();
            if n > 1 {
                c.print(format!("{n} references along the click: the mouse wheel selects the next further (up) or nearer (down)"));
            }
        }
        None => {
            c.along.clear();
            c.print("Nothing there to select");
        }
    }
}

/// What a click's ray met ([`pick_under`]).
pub struct Under {
    /// The reference picked: the crosshair's rule (`activate::pick::choose`).
    pub picked: Option<u32>,
    choice: Option<pick::Pick>,
    /// Every collision hit along the ray, walls too (reference, distance), nearest first.
    hits: Vec<(Option<u32>, f32)>,
    /// The drawn references whose render bounds the ray enters (reference, distance).
    bounds: Vec<(u32, f32)>,
}

/// The reference a ray from `o` along `d` (Bevy space) picks within `reach` game units:
/// the first collision hit, else the render bounds it enters, as the console's click and
/// the crosshair pick.
pub fn pick_under(o: Vec3, d: Vec3, reach: f32, phys: Option<&mut PhysState>, visuals: &crate::PickVisuals, pickable: impl Fn(u32) -> bool + Copy) -> Under {
    let (eye, dir) = (space::to_game(o), {
        let p = space::to_game(o + d);
        let e = space::to_game(o);
        [p[0] - e[0], p[1] - e[1], p[2] - e[2]]
    });
    // Every collision hit along the ray, walls too: the first is the crosshair's hit, the
    // rest what the wheel steps to behind it.
    let hits: Vec<(Option<u32>, f32)> = phys
        .map(|p| {
            let mut filter = Filter::default();
            filter.exclude_layers.insert(layers::TRIGGER);
            p.ray_all(eye, dir, reach, filter)
        })
        .unwrap_or_default()
        .into_iter()
        .map(|h| {
            let form_id = match h.owner {
                Owner::Ref { form_id, .. } | Owner::Actor { form_id, .. } => Some(form_id),
                _ => None,
            };
            (form_id, h.distance)
        })
        .collect();
    let hit = hits.iter().find(|(f, _)| f.is_none_or(pickable)).map(|&(form_id, distance)| RayHit { form_id, distance });
    let bounds: Vec<(u32, f32)> = visuals
        .iter()
        .filter(|(_, _, _, v)| v.get())
        .filter_map(|(r, aabb, gt, _)| {
            let t = gt.affine();
            let (ce, he) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
            Some((r.0, pick::ray_box(o, d, &t, ce, he)?))
        })
        .collect();
    let choice = pick::choose(hit, bounds.iter().copied(), reach, pickable);
    Under { picked: choice.as_ref().map(|p| p.form_id), choice, hits, bounds }
}

/// The references a click's ray passes through, nearest first: those its collision
/// hits (at the hit) and those whose render bounds it enters (at the entry, for one with
/// no collision hit; a box is looser than the shape in it). `picked`, the clicked one,
/// is always among them.
fn along_ray(picked: u32, collision: impl IntoIterator<Item = (u32, f32)>, bounds: impl IntoIterator<Item = (u32, f32)>) -> Vec<u32> {
    let mut at: Vec<(u32, f32)> = Vec::new();
    let put = |list: &mut Vec<(u32, f32)>, id: u32, d: f32, replace: bool| match list.iter_mut().find(|e| e.0 == id) {
        Some(e) if replace => e.1 = e.1.min(d),
        Some(_) => {}
        None => list.push((id, d)),
    };
    for (id, d) in collision {
        put(&mut at, id, d, true);
    }
    let hit_by_collision = at.len();
    for (id, d) in bounds {
        let replace = !at[..hit_by_collision].iter().any(|e| e.0 == id);
        put(&mut at, id, d, replace);
    }
    at.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut ids: Vec<u32> = at.into_iter().map(|e| e.0).collect();
    if !ids.contains(&picked) {
        ids.insert(0, picked);
    }
    ids
}

/// The reference `by` steps from `current` along `along` (positive: further), round to
/// the other end past either end; `None` when `current` is not on it.
fn step_along(along: &[u32], current: u32, by: i32) -> Option<u32> {
    let i = along.iter().position(|&id| id == current)?;
    let n = along.len() as i64;
    Some(along[(i as i64 + by as i64).rem_euclid(n) as usize])
}

fn console_root_node(soft_keyboard: bool) -> Node {
    let mut node = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(Val::Px(8.0)),
        row_gap: Val::Px(4.0),
        ..default()
    };
    if soft_keyboard {
        // Keep the console input clear of the bottom ~50% on-screen keyboard on Android.
        node.top = Val::Percent(4.0);
        node.max_height = Val::Percent(42.0);
        node.overflow = Overflow::clip();
    } else {
        node.bottom = Val::Px(0.0);
    }
    node
}

fn spawn(commands: &mut Commands, shadow: bool) {
    let font = |size: f32| TextFont { font_size: FontSize::Px(size), ..default() };
    // The player's tweak: the scrollback a darker green over a faint shadow [design: docs/design/tweaks.md].
    let green = if shadow { Color::srgb(0.6, 0.85, 0.6) } else { Color::srgb(0.75, 1.0, 0.75) };
    let shade = TextShadow { offset: Vec2::splat(1.5), color: Color::srgba(0.0, 0.0, 0.0, if shadow { 0.8 } else { 0.0 }) };
    commands.spawn((
        ConsoleRoot,
        PointerState::None,
        console_root_node(cfg!(target_os = "android")),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
        GlobalZIndex(120),
        children![
            // The details of the selected reference, over the scrollback.
            (Part::Selected, Text::new(""), font(15.0), TextColor(Color::srgb(1.0, 0.95, 0.6))),
            (Part::Scroll, Text::new(""), font(14.0), TextColor(green), shade),
            (Part::Input, Text::new("> "), font(16.0), TextColor(Color::WHITE)),
        ],
    ));
}

fn draw(c: Res<Console>, mut texts: Query<(&Part, &mut Text)>, time: Res<Time<Real>>) {
    if !c.open {
        return;
    }
    let end = c.lines.len().saturating_sub(c.scroll);
    let start = end.saturating_sub(SHOWN);
    let caret = if time.elapsed_secs().fract() < 0.5 { "_" } else { " " };
    for (part, mut t) in &mut texts {
        let want = match part {
            Part::Scroll => c.lines[start..end].join("
"),
            Part::Input => format!("> {}{caret}", c.input),
            Part::Selected if c.selected.is_some() => c.selected_text.clone(),
            Part::Selected => "Click something to select it".to_string(),
        };
        if t.0 != want {
            t.0 = want;
        }
    }
}

/// A box around the selected reference's drawn parts while the console is open.
fn outline(c: Res<Console>, visuals: crate::PickVisuals, mut gizmos: Gizmos) {
    let (true, Some(id)) = (c.open, c.selected) else { return };
    for (r, aabb, gt, v) in &visuals {
        if r.0 != id || !v.get() {
            continue;
        }
        let local = Transform::from_translation(Vec3::from(aabb.center)).with_scale(Vec3::from(aabb.half_extents) * 2.0);
        gizmos.cube(gt.mul_transform(local), Color::srgb(1.0, 0.85, 0.2));
    }
}

// ---- `help <text>`: every record by Editor ID and name ----

/// One record for the search.
#[derive(Debug, Clone)]
pub struct Found {
    pub id: FormId,
    pub sig: [u8; 4],
    pub edid: Option<String>,
    pub name: Option<String>,
}

/// Placed records: searched only when they have an Editor ID (from the reference table),
/// so the search does not read a million references.
const PLACED: [&[u8; 4]; 14] = [b"REFR", b"ACHR", b"ACRE", b"PGRE", b"PMIS", b"PBEA", b"PFLA", b"PCBE", b"PARW", b"PBAR", b"PHZD", b"NAVM", b"NAVI", b"LAND"];

/// Every record's Editor ID and name, read once on a thread when the console first opens.
#[derive(Resource, Default)]
pub struct SearchIndex {
    building: Option<std::sync::Mutex<std::sync::mpsc::Receiver<Vec<Found>>>>,
    pub ready: Option<Vec<Found>>,
    /// Searches asked for before it was ready.
    waiting: Vec<String>,
}

impl SearchIndex {
    fn start(&mut self, d: &PlayData, refs: &'static fnv_sim::RefTable) {
        if self.ready.is_some() || self.building.is_some() {
            return;
        }
        let lo = d.lo;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let t0 = std::time::Instant::now();
            let all = read_records(lo, refs);
            info!("console: search index of {} records in {:.2} s", all.len(), t0.elapsed().as_secs_f64());
            let _ = tx.send(all);
        });
        self.building = Some(std::sync::Mutex::new(rx));
    }
}

fn read_records(lo: &fnv_esm::LoadOrder<'_>, refs: &fnv_sim::RefTable) -> Vec<Found> {
    let text = |b: &[u8]| fnv_util::text::decode_z(b).trim().to_string();
    let mut out: Vec<Found> = lo
        .index
        .keys()
        .filter_map(|&id| {
            let (_, rec) = lo.winner(id)?;
            let sig = rec.header.sig;
            if PLACED.contains(&&sig) {
                return None;
            }
            let subs = rec.subrecords().ok()?;
            let get = |want: &[u8; 4]| subs.iter().find(|s| &s.sig == want).map(|s| text(&s.data)).filter(|t| !t.is_empty());
            Some(Found { id: FormId(id), sig, edid: get(b"EDID"), name: get(b"FULL") })
        })
        .collect();
    out.extend(refs.edids.values().filter_map(|&r| {
        let p = refs.get(r)?;
        Some(Found { id: r, sig: p.sig, edid: p.edid.clone(), name: None })
    }));
    out.sort_by(|a, b| a.sig.cmp(&b.sig).then_with(|| a.name.cmp(&b.name)).then(a.id.cmp(&b.id)));
    out
}

/// The search terms of `help`: a `"quoted phrase"` is one term, other words one each.
/// Lower case: the search is not case sensitive.
pub fn query_terms(text: &str) -> Vec<String> {
    words(text).into_iter().map(|w| w.trim_matches('"').to_lowercase()).filter(|w| !w.is_empty()).collect()
}

/// Whether every term is in the record's name or Editor ID. An Editor ID has no spaces, so
/// a phrase matches it with its spaces left out (`varmint rifle` finds `WeapNVVarmintRifle`).
pub fn matches(f: &Found, terms: &[String]) -> bool {
    let name = f.name.as_deref().map(str::to_lowercase).unwrap_or_default();
    let edid = f.edid.as_deref().map(str::to_lowercase).unwrap_or_default();
    terms.iter().all(|t| name.contains(t.as_str()) || edid.contains(&t.replace(' ', "")))
}

fn matching_functions(table: &fnv_script::FunctionTable, terms: &[String]) -> Vec<String> {
    let has = |n: &str, t: &str| n.to_lowercase().contains(&t.replace(' ', ""));
    let mut v: Vec<String> = table.functions.values().filter(|f| terms.iter().all(|t| has(&f.name, t) || f.aliases.iter().any(|a| has(a, t)))).map(|f| f.name.clone()).collect();
    v.sort_unstable();
    v
}

fn print_records(c: &mut Console, ix: &[Found], terms: &[String], asked: &str) {
    let hits: Vec<&Found> = ix.iter().filter(|f| matches(f, terms)).collect();
    if hits.is_empty() {
        c.print(format!("No record named like {asked}"));
        return;
    }
    c.print(format!("{} record(s) match {asked}:", hits.len()));
    for f in hits.iter().take(HELP_RECORDS) {
        let name = f.name.as_deref().map(|n| format!(" \"{n}\"")).unwrap_or_default();
        c.print(format!("{} {} {}{name}", f.id, String::from_utf8_lossy(&f.sig), f.edid.as_deref().unwrap_or("-")));
    }
    if hits.len() > HELP_RECORDS {
        c.print(format!("... {} more: narrow the search (\"a phrase\", or more words)", hits.len() - HELP_RECORDS));
    }
}

/// The index came in: answer the searches that waited for it.
fn search_ready(mut index: ResMut<SearchIndex>, mut c: ResMut<Console>) {
    let got = index.building.as_ref().and_then(|rx| rx.lock().ok().and_then(|r| r.try_recv().ok()));
    let Some(all) = got else { return };
    index.building = None;
    for q in std::mem::take(&mut index.waiting) {
        print_records(&mut c, &all, &query_terms(&q), &q);
    }
    index.ready = Some(all);
}

// ---- the script side: run in the script frame (`crate::script::run_scripts`) ----

/// Split a line into words; `"…"` keeps spaces, commas separate as spaces do.
pub fn words(line: &str) -> Vec<String> {
    fnv_game::functions::console::words(line)
}

/// A form by name as the console takes it: `player`, an Editor ID (a reference's first),
/// a FormID with or without `0x`.
fn form(rt: &Runtime, s: &str) -> Result<FormId, String> {
    let g = &rt.game;
    fnv_game::runner::resolve(g, s).or_else(|e| parse_form_id(s).ok_or(e))
}

/// Run one console line in the script frame.
pub(crate) fn run_call(rt: &mut Runtime, call: &ConsoleCall) -> ConsoleReply {
    match run_line(rt, call) {
        Ok(r) => r,
        Err(e) => ConsoleReply { lines: vec![e], ..Default::default() },
    }
}

fn run_line(rt: &mut Runtime, call: &ConsoleCall) -> Result<ConsoleReply, String> {
    let line = call.line.trim();
    let end = line.find(char::is_whitespace).unwrap_or(line.len());
    let (word, expression) = line.split_at(end);
    if word.eq_ignore_ascii_case("eval") {
        let expression = expression.trim();
        let ctx = fnv_game::game::RunCtx { self_ref: call.selected.map(FormId), ..Default::default() };
        let value = rt.game.eval_console_expression(expression, ctx)?;
        return Ok(ConsoleReply { lines: vec![format!("{expression} >> {value}")], ..Default::default() });
    }
    let w = words(line);
    let Some(first) = w.first() else { return Ok(ConsoleReply::default()) };
    let (on_name, func) = if first.parse::<f64>().is_ok() {
        (None, first.clone())
    } else {
        match first.rsplit_once('.') {
            Some((r, f)) if !r.is_empty() && !f.is_empty() => (Some(r.to_string()), f.to_string()),
            _ => (None, first.clone()),
        }
    };
    let args = &w[1..];
    // Words of the console's own for functions the catalogue gives no alias
    // [hypothesis: `tai` is the word players type for the AI toggle; not in the catalogue].
    let func = match func.to_ascii_lowercase().as_str() {
        "tai" => "ToggleActorsAI".to_string(),
        _ => func,
    };
    let selected = call.selected.map(FormId);
    // Persistent console declarations, `let` statements, and lines starting with a
    // console local go directly to the script compiler so locals persist across lines
    // and print their results transparently (docs/design/console.md).
    if on_name.is_none()
        && (fnv_script::compile::parse::DeclType::from_word(&func).is_some()
            || func.eq_ignore_ascii_case("let")
            || rt.game.has_console_local(&func))
    {
        return compiled(&mut rt.game, call, selected);
    }
    if let Some(n) = &on_name {
        if rt.game.has_console_local(n) {
            if let Some((name, run)) = fnv_game::functions::console::lookup_word(&rt.game.data.table, &func) {
                let on = rt.game.console_local_ref(n).ok_or_else(|| format!("{name}: console local {n} holds no reference"))?;
                let g = &mut rt.game;
                let console_ctx = ConsoleCtx { on: Some(on), active_quest: call.active_quest.map(FormId) };
                let r = run(g, &console_ctx, args);
                let requests = std::mem::take(&mut g.console_requests);
                let lines = r.map_err(|e| format!("{name}: {e}"))?;
                return Ok(ConsoleReply { lines, requests, ..Default::default() });
            }
            return compiled(&mut rt.game, call, selected);
        }
    }
    let on = match &on_name {
        Some(n) => match form(rt, n) {
            Ok(id) => Some(id),
            Err(_) => return compiled(&mut rt.game, call, selected),
        },
        None => selected,
    };
    let g = &mut rt.game;
    let console_ctx = ConsoleCtx { on, active_quest: call.active_quest.map(FormId) };
    // `AdvancePCLevel` (`AdvLevel`): the catalogue gives it neither an opcode nor the
    // console category, so the console knows it by name; the console closes so the
    // level-up menu it opens is on top [design].
    if on_name.is_none() && (func.eq_ignore_ascii_case("AdvancePCLevel") || func.eq_ignore_ascii_case("AdvLevel")) {
        let level = fnv_game::levelup::advance(g);
        return Ok(ConsoleReply { lines: vec![format!("AdvancePCLevel >> level {level}")], requests: vec![ConsoleRequest::CloseMenus], ..Default::default() });
    }
    // `SetGameDifficulty` / `GetGameDifficulty` (JIP: the catalogue gives them no
    // opcode, so the console knows them by name): the settings page's level
    // (`crate::difficulty`, docs/spec-notes/difficulty.md §4).
    if on_name.is_none() && func.eq_ignore_ascii_case("SetGameDifficulty") {
        let code = args.first().and_then(|a| a.parse::<f64>().ok()).ok_or("SetGameDifficulty <0-4>")?;
        let d = fnv_game::difficulty::Difficulty::from_code(code as i64).ok_or("SetGameDifficulty: 0 (Very Easy) to 4 (Very Hard)")?;
        crate::difficulty::set_level(d);
        let line = g.set_difficulty(d);
        info!("{line}");
        return Ok(ConsoleReply { lines: vec![format!("SetGameDifficulty >> {} ({})", d.name(), d.code())], ..Default::default() });
    }
    if on_name.is_none() && func.eq_ignore_ascii_case("GetGameDifficulty") {
        return Ok(ConsoleReply { lines: vec![format!("GetGameDifficulty >> {:.2}", g.difficulty.code() as f64)], ..Default::default() });
    }
    // The `set <variable> to <value>` statement: keep plain literal assignments to
    // globals or selected-reference script variables on the direct path; route console
    // locals and expressions through the compiler.
    if on_name.is_none() && func.eq_ignore_ascii_case("set") {
        let literal = args.len() == 3
            && args[1].eq_ignore_ascii_case("to")
            && !g.has_console_local(&args[0])
            && !g.has_console_local(&args[2])
            && (args[2].parse::<f64>().is_ok() || !fnv_game::functions::console::has_expression_operator(&args[2]));
        if !literal {
            return compiled(g, call, on);
        }
        let lines = fnv_game::functions::console::set_statement(g, &console_ctx, args)?;
        return Ok(ConsoleReply { lines, ..Default::default() });
    }
    // A console-only command (numbered from `CONSOLE_OPCODE_BASE`) that prints lines,
    // registered by name in `fnv_game::functions::console`; the others go through
    // `Game::call_function` as every function does.
    let console_fn = fnv_game::functions::console::lookup_word(&g.data.table, &func);
    if let Some((name, run)) = console_fn {
        let r = run(g, &console_ctx, args);
        let requests = std::mem::take(&mut g.console_requests);
        let lines = r.map_err(|e| format!("{name}: {e}"))?;
        return Ok(ConsoleReply { lines, requests, ..Default::default() });
    }
    // Decide before calling any host function so a runtime failure never retries a side effect.
    let use_compiler = fnv_game::functions::console::call_has_expression(g, line, &func, args)
        || args.iter().any(|a| g.has_console_local(a))
        || g.data.table.unnumbered.contains_key(&func.to_ascii_lowercase())
        || g.data.table.by_name(&func).is_none();
    if use_compiler {
        return compiled(g, call, on);
    }
    let asked = g.toggles.asked.len();
    let called = fnv_game::functions::console::call_words(g, on, &func, args, "console")?;
    let (fname, v) = (called.name, called.value);
    // A function of a system not built says so instead of its 0.
    if let Some(why) = called.stub {
        return Ok(ConsoleReply { lines: vec![format!("{fname}: not built ({why})")], ..Default::default() });
    }
    // A toggle: [`replies`] prints its new state.
    if g.toggles.asked.len() > asked {
        return Ok(ConsoleReply::default());
    }
    let name = |r: FormId| if r == FormId::PLAYER_REF { "player".to_string() } else { g.data.name(r) };
    let shown = match v {
        Value::Num(n) => format!("{n:.2}"),
        Value::Ref(0) => "none".to_string(),
        Value::Ref(r) => format!("{} {}", FormId(r), name(FormId(r))),
    };
    let on_text = on.map(|r| format!(" on {}", name(r))).unwrap_or_default();
    Ok(ConsoleReply { lines: vec![format!("{fname}{on_text} >> {shown}")], ..Default::default() })
}

/// Run a statement, declaration, or expression through the script compiler with
/// persistent console locals and transparent result printing
/// (`Game::run_console_line`, docs/design/console.md).
fn compiled(g: &mut fnv_game::Game, call: &ConsoleCall, on: Option<FormId>) -> Result<ConsoleReply, String> {
    let asked = g.toggles.asked.len();
    let ctx = fnv_game::game::RunCtx { self_ref: on, ..Default::default() };
    let res = g.run_console_line(&call.line, ctx)?;
    if g.toggles.asked.len() > asked {
        return Ok(ConsoleReply::default());
    }
    let reply = match res.value {
        Some(v) => format!("{} >> {v}", call.line.trim()),
        None => format!("{} >> ran", call.line.trim()),
    };
    let mut lines = vec![reply];
    lines.extend(res.warnings);
    Ok(ConsoleReply { lines, ..Default::default() })
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> Console {
    let mut c = Console { open: true, ..Default::default() };
    if step >= 1 {
        c.input = "synthetic".into();
    }
    if step >= 2 {
        c.recall = Some(0);
    }
    if step >= 3 {
        c.scroll = 1;
    }
    if step >= 4 {
        c.selected = Some(42);
    }
    if step >= 5 {
        c.lines.push("Synthetic result".into());
    }
    c.auto_output_done = step >= 6;
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tweaks_count_repeats_and_stamp_lines() {
        let mut c = Console { compress_repeats: true, ..Default::default() };
        for _ in 0..3 {
            c.print("same");
        }
        c.print("other");
        c.print("same");
        assert_eq!(c.lines, ["same -#3", "other", "same"]);
        assert_eq!(stamp(75), "[01:15] ");
        assert_eq!(stamp(3725), "[1:02:05] ");
        let mut c = Console { timestamps: true, ..Default::default() };
        c.print("x");
        assert!(c.lines[0].starts_with('[') && c.lines[0].ends_with("] x"));
    }

    #[test]
    fn rbat_and_scof_write_plain_names_in_nvforevers_folder_only() {
        let home = std::path::Path::new("home");
        assert_eq!(own_file("mine", Some(home)), Ok(home.join("mine.txt")));
        assert_eq!(own_file("\"log.log\"", Some(home)), Ok(home.join("log.log")));
        for bad in ["", "..\\up", "a/b", "C:x", ".hidden"] {
            assert!(own_file(bad, Some(home)).is_err(), "{bad}");
        }
        assert!(own_file("mine", None).is_err());
    }

    #[test]
    fn set_vel_without_physics_says_so() {
        assert_eq!(set_vel(0x14, 'Z', 300.0, None, None), "SetVel: no physics in this run");
    }

    #[test]
    fn batch_files_are_found_in_the_first_folder_that_has_them() {
        let base = std::env::temp_dir().join(format!("nvf-bat-{}", std::process::id()));
        let (home, game) = (base.join("home"), base.join("game"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("setup.txt"), "tgm\r\n\r\n  player.additem f 100 \nbat more\n").unwrap();
        std::fs::write(home.join("other.bat"), "tcl").unwrap();
        let roots = [home.clone(), game.clone()];
        let (path, lines) = batch_lines("setup", &roots).unwrap();
        assert_eq!(path, game.join("setup.txt"));
        assert_eq!(lines, ["tgm", "player.additem f 100", "bat more"]);
        assert_eq!(batch_lines("\"other.bat\"", &roots).unwrap().1, ["tcl"]);
        assert!(batch_lines("missing", &roots).unwrap_err().starts_with("No batch file missing.txt"));
        assert!(batch_lines("", &roots).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn operators_outside_quotes_send_a_line_to_the_compiler() {
        use fnv_game::functions::console::has_expression_operator;
        assert!(has_expression_operator("set x to GetAV Luck + 1"));
        assert!(has_expression_operator("player.SetAV Luck (5)"));
        assert!(!has_expression_operator("player.additem f 100"));
        assert!(!has_expression_operator("ShowMessage \"a-b\""));
    }

    #[test]
    fn along_ray_orders_by_distance_and_steps_round() {
        // An actor in front (capsule hit at 100) and one behind (at 180); a lamp seen only
        // by its bounds at 140; the front actor's loose box enters at 80 but its hit counts.
        let along = along_ray(1, [(1, 100.0), (1, 105.0), (2, 180.0)], [(1, 80.0), (3, 140.0), (3, 150.0)]);
        assert_eq!(along, vec![1, 3, 2]);
        assert_eq!(step_along(&along, 1, 1), Some(3));
        assert_eq!(step_along(&along, 3, 1), Some(2));
        assert_eq!(step_along(&along, 2, 1), Some(1), "past the last goes round to the first");
        assert_eq!(step_along(&along, 1, -1), Some(2), "before the first goes round to the last");
        assert_eq!(step_along(&along, 1, 4), Some(3));
        assert_eq!(step_along(&along, 9, 1), None);
        // The picked one is always on it.
        assert_eq!(along_ray(7, [], []), vec![7]);
        assert_eq!(step_along(&[7], 7, 1), Some(7));
    }

    #[test]
    fn a_line_holds_several_statements() {
        assert_eq!(statements("player.additem f 100; tgm;coc Goodsprings"), ["player.additem f 100", "tgm", "coc Goodsprings"]);
        assert_eq!(statements(r#"help "a;b"; cls"#), [r#"help "a;b""#, "cls"]);
        assert_eq!(statements(" ; tgm ;; "), ["tgm"]);
        // The aid keeps a line's own statements together.
        let aid = parse_aid("12:player.setav luck 9; tgm;12.5:close").unwrap();
        assert_eq!(aid, vec![(12.0, "player.setav luck 9; tgm".to_string()), (12.5, "close".to_string())]);
    }

    #[test]
    fn words_split_on_spaces_and_commas_and_keep_quotes() {
        assert_eq!(words("player.additem 0000000F, 100"), ["player.additem", "0000000F", "100"]);
        assert_eq!(words("ShowMessage \"two words\" 3"), ["ShowMessage", "\"two words\"", "3"]);
        assert!(words("   ").is_empty());
    }

    #[test]
    fn help_matches_names_and_editor_ids_without_case_and_phrases_narrow() {
        let rifle = Found { id: FormId(1), sig: *b"WEAP", edid: Some("WeapNVVarmintRifle".into()), name: Some("Varmint Rifle".into()) };
        let ammo = Found { id: FormId(2), sig: *b"AMMO", edid: Some("Ammo556mm".into()), name: Some("5.56mm Round (Varmint)".into()) };
        assert_eq!(query_terms("VARMINT"), ["varmint"]);
        assert_eq!(query_terms("\"varmint rifle\""), ["varmint rifle"]);
        assert!(matches(&rifle, &query_terms("varmint")) && matches(&ammo, &query_terms("Varmint")));
        assert!(matches(&rifle, &query_terms("\"varmint rifle\"")));
        assert!(!matches(&ammo, &query_terms("\"varmint rifle\"")));
        // By Editor ID: the phrase without its spaces.
        let by_edid = Found { name: None, ..rifle.clone() };
        assert!(matches(&by_edid, &query_terms("\"Varmint Rifle\"")));
        // Several words: all must be there, in any order.
        assert!(matches(&ammo, &query_terms("round varmint")));
        assert!(!matches(&rifle, &query_terms("round varmint")));
    }

    #[test]
    fn form_ids_are_read_with_or_without_0x() {
        assert_eq!(parse_form_id("0000000F"), Some(FormId(0xF)));
        assert_eq!(parse_form_id("0x00104C0C"), Some(FormId(0x0010_4C0C)));
        assert_eq!(parse_form_id("f"), Some(FormId(0xF)));
        assert_eq!(parse_form_id("player"), None);
        assert_eq!(parse_form_id("123456789"), None);
    }

    #[test]
    fn cow_goes_to_the_middle_of_the_grid_square() {
        assert_eq!(grid_centre((0, 0), 5.0), [2048.0, 2048.0, 5.0]);
        assert_eq!(grid_centre((-17, 1), 0.0), [-17.0 * 4096.0 + 2048.0, 4096.0 + 2048.0, 0.0]);
    }

    #[test]
    fn the_aid_is_read_soonest_first() {
        let a = parse_aid("12:player.additem f 5; 10:help stim").unwrap();
        assert_eq!(a, vec![(10.0, "help stim".to_string()), (12.0, "player.additem f 5".to_string())]);
        assert!(parse_aid("nonsense").is_err());
    }

    #[test]
    fn improved_console_persists_locals_prints_results_and_keeps_vanilla_dispatch() {
        let mut st = ScriptState::over_no_plugins();
        let rt = st.rt.as_mut().unwrap();
        let call = |rt: &mut Runtime, line: &str| {
            run_call(rt, &ConsoleCall { line: line.into(), selected: None, active_quest: None }).lines
        };

        assert_eq!(call(rt, "int x"), ["int x >> ran"]);
        assert_eq!(call(rt, "set x to 10"), ["set x to 10 >> 10"]);
        assert_eq!(call(rt, "x + 2"), ["x + 2 >> 12"]);
        assert_eq!(call(rt, "1.5 + 2.5"), ["1.5 + 2.5 >> 4"]);
        assert_eq!(call(rt, "eval x * 2"), ["x * 2 >> 20"]);

        assert_eq!(call(rt, "string_var s = \"hi\""), ["string_var s = \"hi\" >> \"hi\""]);
        assert_eq!(call(rt, "let s += \"!\""), ["let s += \"!\" >> \"hi!\""]);
        assert_eq!(call(rt, "s"), ["s >> \"hi!\""]);

        assert_eq!(call(rt, "array_var a = ar_List 1, 9"), ["array_var a = ar_List 1, 9 >> [0: 1, 1: 9]"]);
        assert_eq!(call(rt, "a[0]"), ["a[0] >> 1"]);

        assert_eq!(call(rt, "IsPluginInstalled \"Improved Console\""), ["IsPluginInstalled \"Improved Console\" >> 1"]);
        assert_eq!(call(rt, "GetPluginVersion \"Improved Console\""), ["GetPluginVersion \"Improved Console\" >> 3"]);

        assert_eq!(call(rt, "ref r = player"), ["ref r = player >> 0x00000014 0x00000014"]);
        assert_eq!(call(rt, "player.SetAV Health 70"), ["SetActorValue on player >> 0.00"]);
        assert_eq!(call(rt, "player.GetAV Health"), ["GetActorValue on player >> 70.00"]);
        assert_eq!(call(rt, "r.GetAV Health"), ["r.GetAV Health >> 70"]);
        assert_eq!(call(rt, "player.GetAV Health + x"), ["player.GetAV Health + x >> 80"]);
    }
}
