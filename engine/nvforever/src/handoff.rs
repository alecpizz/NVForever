//! Loading a save, or going back to the main menu, while playing, without the window
//! closing and another opening (docs/design/save-load.md, "Loading while playing").
//! NVForever still starts again, so a save always lands in a world built fresh from the
//! plugins and nothing of the run before carries over; the change of program is hidden
//! behind a loading screen [design, owner 2026-10-16]. The two runs overlap only for the
//! new run's first moments, not for its whole load [owner 2026-11-01, #1357: the old run
//! held its whole game, about 3.7 GB, while the new one loaded]:
//!
//! 1. This run brings a loading screen up (`nvf_play::loading::CoverRequest`, its picture
//!    one for the save's place), which holds the game. Once it is drawn, the new run
//!    starts with `--handoff <file>` and this window's place and size (`--window-at`,
//!    `--size`, `--window-maximized`); this run then updates a few times a second only.
//! 2. The new run opens its window hidden at that place ([`Hidden`]). A save it cannot
//!    load (made with plugins that cannot be loaded now, damaged, of another format) opens
//!    the main menu instead, and once the window is up a message box says why
//!    ([`LoadFailed`]; owner 2026-11-01), as for `--load` given by hand. As soon as its own
//!    loading screen (or the main menu) is up and drawn for [`SETTLE_SECS`], it shows its
//!    window over the old one, takes the focus and writes the file; it loads behind its
//!    screen from then on, alone. (Holding its script data build until then took the
//!    swap's peak from 7.6 to 6.1 GB but the load from 6-12 s to 16 s: not done.) Only
//!    the last step, making its game and putting the save back
//!    (`nvf_play::script::ScriptState::held`, set from the first frame: #2210), waits until the old run has taken the file
//!    away as it ends, or [`ACK_WAIT_SECS`] after the file was written: on a busy machine
//!    the screen's first frames were slow enough for the save to be put back before the
//!    window was shown, while the old run still held its game (#1651).
//! 3. This run sees the file, takes it away and ends: the frame's exit logs are written
//!    (`Last`), then the process ends at once ([`EndNow`]) instead of taking its world
//!    apart piece by piece, which held its memory for about 2 s more. Streamline (DLSS,
//!    DLSS Frame Generation) is shut down first, as at any other exit (`crate::dlss::end_now`,
//!    #2733: `main`'s shutdown after `App::run` never runs on this path).
//!
//! If the new run ends before it is shown (it stops at the start: a damaged archive, say),
//! the screen comes down and this game goes on; when the new run's log says why it
//! stopped, the player is told in a message box (not in unattended runs) and the log line
//! carries it. A new run that fails later, behind its own screen, is on its own: its crash
//! report is the player's. Closing this window while it waits ends the new run too. A new
//! run still hidden after [`SHOW_AFTER_SECS`] shows itself anyway. The new run writes a
//! marker beside the file as it starts ([`started_marker`]); a new run still running
//! without one after [`JOIN_WAIT_SECS`] is not taking part (a Linux report, #1903: it
//! loaded with its window shown and never wrote the file, and the old run waited until
//! its window was closed, two games playing), and neither is one that has not written the
//! file [`GIVE_UP_MARGIN_SECS`] past its own time limit: this run then ends, leaving the
//! new one to play. A run whose restore fails
//! after the save was read (`nvf_play::save::LoadRefused`) has no game to go on with: it
//! hands over like the return to the main menu, and gives the new run the reason
//! (`--load-refused`) for its message box.
//!
//! The new run gets this run's arguments but none of its test aids (`NVF_PLAY_TEST_*`:
//! a console line that loads would load again in every new run), except test aid
//! `NVF_PLAY_TEST_HANDOFF_EXIT=1`: the new run ends once it has taken over and its world
//! is in (the save put back) or its main menu is open, so a smoke run does not leave it
//! running; and the aids that
//! only measure (`nvf_play::MEASURING_AIDS`), so a session played by hand logs its hitches
//! after a load.
//!
//! Log: `load: handing over to a new run with --load <path>` (`with --menu`), `load: the
//! new run's command line: <exe> <args>`, `handoff: started as the new run (ready file
//! <file>)` (the new run's), `load: the new run (process <pid>) is running but <why> after
//! <s> s; ending this one so one game runs`, `load:
//! <why>; opening the main menu` (the new run's, a save it could not load), `handoff: window shown after <s>
//! s` (`, before its screen was up` past the time limit), `load: the new run is up after
//! <s> s; ending this one`, `load: the new run ended (<status>) before it was ready[:
//! <why>]; staying`.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WINIT_WINDOWS;
#[cfg(not(target_os = "android"))]
use bevy::winit::{UpdateMode, WinitSettings};
use nvf_play::loading::{CoverRequest, Loading, Uncover};
use nvf_play::save::{LoadRequest, Restoring, SaveWrites};
use std::path::{Path, PathBuf};
#[cfg(not(target_os = "android"))]
use std::process::Child;
use std::time::Instant;
#[cfg(any(not(target_os = "android"), test))]
use std::time::Duration;

/// Real seconds (and at least [`SETTLE_FRAMES`] frames) the new run stays hidden after
/// its loading screen (or the main menu) is up, so the first frame shown is the screen
/// [engineering choice, as `nvf_play::loading::SETTLE_SECS`].
pub const SETTLE_SECS: f64 = 0.25;
pub const SETTLE_FRAMES: u32 = 3;

/// Seconds the new run waits, after telling the old run, for it to take the file away
/// before putting the save back anyway (an old run that died) [engineering choice: the
/// old run looks a few times a second, `WAIT_TICK`].
pub const ACK_WAIT_SECS: f64 = 2.0;

/// A new run shows its window after this many seconds whatever is still loading
/// [engineering choice: as long as a loading screen waits, `nvf_play::loading`].
pub const SHOW_AFTER_SECS: f64 = 120.0;

/// Frames the old run's loading screen is up before the new run starts
/// [engineering choice: the screen is on screen before the long frame of starting it].
const COVER_FRAMES: u32 = 3;

/// Seconds the old run waits for the new run's start marker ([`started_marker`]) before
/// taking it for a run that is not taking part in the handoff, and ending itself so one
/// game runs (#1903) [engineering choice: the marker is written before the data loads,
/// within the first second or two seen in logs; the rest is room for a slow disk].
pub const JOIN_WAIT_SECS: f64 = 30.0;

/// Seconds past [`SHOW_AFTER_SECS`] the old run still waits for a new run that has
/// started but not said it is up, before ending itself all the same (#1903)
/// [engineering choice].
#[cfg(any(not(target_os = "android"), test))]
pub const GIVE_UP_MARGIN_SECS: f64 = 15.0;

/// The one test aid the new run keeps (module notes).
const EXIT_AID: &str = "NVF_PLAY_TEST_HANDOFF_EXIT";

/// How often the old run updates while it waits [engineering choice: often enough to see
/// the new run's file soon, rarely enough to leave the computer to the new run].
#[cfg(not(target_os = "android"))]
const WAIT_TICK: Duration = Duration::from_millis(50);

/// Where the old window is, for the new run's arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// The window's outer top-left corner on the desktop.
    pub at: Option<IVec2>,
    /// Its drawing area in pixels.
    pub size: UVec2,
    pub maximized: bool,
}

impl Placement {
    /// The arguments that open the new run's window the same way.
    pub fn args(&self) -> Vec<String> {
        let mut out = vec!["--size".to_string(), format!("{}x{}", self.size.x, self.size.y)];
        if let Some(p) = self.at {
            // One word: a value starting with '-' is not taken for an option.
            out.push(format!("--window-at={},{}", p.x, p.y));
        }
        if self.maximized {
            out.push("--window-maximized".into());
        }
        out
    }
}

/// `--window-at X,Y`.
pub fn parse_at(s: &str) -> Option<IVec2> {
    let (x, y) = s.split_once(',')?;
    Some(IVec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// The new run, while this one waits for it. Dropped while still waiting (this run ending
/// some other way, as by its window being closed), it ends the new run too.
#[cfg(not(target_os = "android"))]
struct Successor {
    child: Child,
    file: PathBuf,
    started: Instant,
    /// The update mode before the wait.
    settings: WinitSettings,
    /// The new run is up: it goes on alone.
    handed_over: bool,
}

#[cfg(not(target_os = "android"))]
impl Drop for Successor {
    fn drop(&mut self) {
        if !self.handed_over && matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_file(started_marker(&self.file));
    }
}

/// The file beside the ready file the new run writes as soon as it starts with
/// `--handoff` (before its data loads), so the old run knows it is taking part (#1903).
pub fn started_marker(ready: &Path) -> PathBuf {
    let mut name = ready.as_os_str().to_owned();
    name.push(".started");
    PathBuf::from(name)
}

/// The new run's first step: say it started as the handoff's new run ([`started_marker`]).
/// Called before the log is up; the failure is reported again once it is.
pub fn mark_started(ready: &Path) -> std::io::Result<()> {
    std::fs::write(started_marker(ready), b"")
}

/// Whether the old run, still waiting for a new run that is running, stops waiting and
/// ends (#1903): `Some(why)` when the new run never said it started as a handoff run, or
/// has not said it is up well past the time it shows itself anyway.
#[cfg(any(not(target_os = "android"), test))]
fn give_up(waited_secs: f64, joined: bool) -> Option<&'static str> {
    if !joined && waited_secs >= JOIN_WAIT_SECS {
        Some("it did not start as a handoff run (no start marker)")
    } else if waited_secs >= SHOW_AFTER_SECS + GIVE_UP_MARGIN_SECS {
        Some("it never said it was up")
    } else {
        None
    }
}

/// What the old run is doing about a load or a return to the main menu.
#[derive(Default)]
enum Leaving {
    #[default]
    No,
    /// Asked; saves in flight are still being written. `None`: the main menu.
    Asked(Option<LoadRequest>),
    /// The screen is up since this many frames.
    Covered(Option<LoadRequest>, u32),
    #[cfg(not(target_os = "android"))]
    Waiting(Successor),
    /// The new run is up: this one ends after this frame's `Last` ([`EndNow`]).
    HandedOver,
}

/// What this run is doing, and why the load it was started for was refused
/// (`nvf_play::save::LoadRefused`), for the new run.
#[derive(Resource, Default)]
pub struct Handoff(Leaving, Option<String>);

impl Handoff {
    /// The screen is covered for a load or the main menu, and a new run takes over, or is
    /// about to start: what this run holds for others (the co-op host's port, `crate::coop`)
    /// goes now. False again when the new run failed and this one stays.
    pub fn leaving(&self) -> bool {
        match &self.0 {
            Leaving::Covered(..) | Leaving::HandedOver => true,
            #[cfg(not(target_os = "android"))]
            Leaving::Waiting(_) => true,
            _ => false,
        }
    }

    /// Whether what it is leaving for is a load (not the main menu).
    pub fn loading(&self) -> bool {
        matches!(&self.0, Leaving::Covered(Some(_), _) | Leaving::Asked(Some(_)))
    }
}

/// The new run's arguments: this run's without those the save or the window decide
/// (`keep`), and what to open.
pub fn new_run_args(keep: &[String], what: Option<&LoadRequest>, placement: Option<Placement>, file: &Path) -> Vec<String> {
    let mut args = keep.to_vec();
    match what {
        Some(r) => args.extend(["--load".to_string(), r.path.display().to_string()]),
        None => args.push("--menu".into()),
    }
    args.extend(placement.map(|p| p.args()).unwrap_or_default());
    args.push("--handoff".into());
    args.push(file.display().to_string());
    args
}

/// A successor keeps its profile and measuring aids, but does not repeat the
/// script clock or driving aids from the run that requested the handoff.
#[cfg(any(not(target_os = "android"), test))]
fn remove_driving_aids(command: &mut std::process::Command, keys: impl IntoIterator<Item = std::ffi::OsString>) {
    for key in keys {
        if key.to_str().is_some_and(|key| key == "NVF_PLAY_AIDS_FROM_SCRIPTS" || (nvf_play::drives_run(key) && key != EXIT_AID)) {
            command.env_remove(&key);
        }
    }
}

/// Where the save starts, for the loading screen's picture.
fn destination_of(r: &LoadRequest, here: nvf_play::Location) -> Option<fnv_world::loadscreen::Destination> {
    let f = fnv_save::read_header(&r.path).ok()?;
    let pl = &f.header.place;
    Some(fnv_world::loadscreen::Destination {
        interior: pl.interior,
        exterior: pl.interior.is_none().then(|| (pl.world.unwrap_or(here.world), fnv_world::cell_of(pl.feet[0], pl.feet[1]))),
    })
}

fn placement(window: &Window, entity: Entity) -> Placement {
    let size = UVec2::new(window.resolution.physical_width(), window.resolution.physical_height());
    WINIT_WINDOWS.with_borrow(|windows| match windows.get_window(entity) {
        Some(w) => Placement { at: w.outer_position().ok().map(|p| IVec2::new(p.x, p.y)), size, maximized: w.is_maximized() },
        None => Placement { at: None, size, maximized: false },
    })
}

/// A message box saying why a load did not happen, on its own thread so the game goes on
/// drawing behind it; none in unattended runs, which would wait on it.
fn tell_player(why: &str) {
    // Unattended: a test aid drives this run (as nvf_play's test_aid::Unattended).
    if std::env::vars_os().any(|(k, _)| k.to_str().is_some_and(nvf_play::drives_run)) {
        info!("load: message box not shown (unattended run): the save could not be loaded: {why}");
        return;
    }
    info!("load: message box: the save could not be loaded: {why}");
    // rfd is a Windows-only dependency; elsewhere the log line carries the reason.
    #[cfg(windows)]
    {
        let text = format!("The save could not be loaded.

{why}");
        std::thread::spawn(move || {
            rfd::MessageDialog::new().set_title("NVForever").set_level(rfd::MessageLevel::Warning).set_description(text).show();
        });
    }
    #[cfg(not(windows))]
    let _ = why;
}

/// A file name no other handoff uses.
fn ready_file() -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("nvforever-handoff-{}-{nanos}.ready", std::process::id()))
}

/// The old run's side (module notes, steps 1 and 3): loads asked for while playing
/// (`LoadRequest`) and the pause menu's return to the main menu (`MainMenuRequest`).
/// A save not yet on disk (its thumbnail, its writer thread) is waited for.
#[allow(clippy::too_many_arguments)]
pub fn leave(
    (mut asked, mut to_menu, mut refused): (MessageReader<LoadRequest>, MessageReader<nvf_play::pause::MainMenuRequest>, MessageReader<nvf_play::save::LoadRefused>),
    (writes, hold): (Res<SaveWrites>, Option<Res<crate::coop::HoldLeave>>),
    mut handoff: ResMut<Handoff>,
    (mut cover, mut uncover): (MessageWriter<CoverRequest>, MessageWriter<Uncover>),
    #[cfg(not(target_os = "android"))]
    mut exit: MessageWriter<AppExit>,
    location: Res<nvf_play::Location>,
    window: Query<(Entity, &Window), With<PrimaryWindow>>,
    #[cfg(not(target_os = "android"))]
    mut winit: ResMut<WinitSettings>,
    _main_thread: NonSendMarker,
) {
    let handoff = &mut *handoff;
    let why = refused.read().last().map(|r| r.0.clone());
    let request = asked.read().last().cloned().map(Some).or(to_menu.read().last().map(|_| None)).or(why.is_some().then_some(None));
    handoff.1 = why.or(handoff.1.take());
    let h = &mut handoff.0;
    if let Some(r) = request {
        match h {
            Leaving::No => *h = Leaving::Asked(r),
            _ => info!("load: already going; {} ignored", r.map_or("the main menu".to_string(), |r| r.path.display().to_string())),
        }
    }
    match std::mem::take(h) {
        Leaving::No => {}
        Leaving::HandedOver => *h = Leaving::HandedOver,
        Leaving::Asked(r) if writes.busy() => *h = Leaving::Asked(r),
        // A co-op guest going to the main menu saves its own game first (`crate::coop`;
        // only a guest in a host's world holds).
        Leaving::Asked(None) if hold.as_deref().is_some_and(|h| h.holds()) => *h = Leaving::Asked(None),
        Leaving::Asked(r) => {
            cover.write(CoverRequest { to: r.as_ref().and_then(|r| destination_of(r, *location)) });
            *h = Leaving::Covered(r, 0);
        }
        Leaving::Covered(r, n) if n < COVER_FRAMES => *h = Leaving::Covered(r, n + 1),
        Leaving::Covered(r, _) => {
            let file = ready_file();
            let _ = std::fs::remove_file(&file);
            let _ = std::fs::remove_file(started_marker(&file));
            let keep = crate::restart_args(&crate::command_line());
            #[cfg(target_os = "android")]
            {
                // Android has no executable to spawn. The private launcher kills only
                // this app's :game process before starting a fresh NativeActivity.
                // The successor is shown normally, without the desktop overlap protocol.
                let mut args = new_run_args(&keep, r.as_ref(), None, &file);
                args.truncate(args.len() - 2);
                args.extend(handoff.1.iter().map(|why| format!("--load-refused={why}")));
                match crate::android::restart(&args) {
                    Ok(()) => { *h = Leaving::HandedOver; }
                    Err(error) => { warn!("load: {error}; staying"); uncover.write(Uncover); }
                }
            }
            #[cfg(not(target_os = "android"))]
            let started = {
                let mut args = new_run_args(&keep, r.as_ref(), window.single().ok().map(|(e, w)| placement(w, e)), &file);
                args.extend(handoff.1.iter().map(|why| format!("--load-refused={why}")));
                std::env::current_exe()
                    .map_err(|e| format!("cannot find this program to start it again: {e}"))
                    .and_then(|exe| {
                        let mut cmd = std::process::Command::new(&exe);
                        cmd.args(&args);
                        // The whole line, so a report shows what the new run was given (#1903).
                        info!("load: the new run's command line: {} {}", exe.display(), args.join(" "));
                        remove_driving_aids(&mut cmd, std::env::vars_os().map(|(key, _)| key));
                        cmd.spawn().map_err(|e| format!("could not start {}: {e}", exe.display()))
                    })
            };
            #[cfg(not(target_os = "android"))]
            match started {
                Ok(child) => {
                    info!("load: handing over to a new run with {}", r.as_ref().map_or("--menu".to_string(), |r| format!("--load {}", r.path.display())));
                    let settings = winit.clone();
                    *winit = WinitSettings { focused_mode: UpdateMode::reactive_low_power(WAIT_TICK), unfocused_mode: UpdateMode::reactive_low_power(WAIT_TICK) };
                    *h = Leaving::Waiting(Successor { child, file, started: Instant::now(), settings, handed_over: false });
                }
                Err(e) => {
                    warn!("load: {e}; staying");
                    uncover.write(Uncover);
                }
            }
        }
        #[cfg(not(target_os = "android"))]
        Leaving::Waiting(mut s) => {
            if s.file.exists() {
                info!("load: the new run is up after {:.2} s; ending this one", s.started.elapsed().as_secs_f64());
                s.handed_over = true;
                exit.write(AppExit::Success);
                *h = Leaving::HandedOver;
                return;
            }
            let ended = match s.child.try_wait() {
                Ok(None) => {
                    let waited = s.started.elapsed().as_secs_f64();
                    if let Some(why) = give_up(waited, started_marker(&s.file).exists()) {
                        // The new run plays on with its own window: one game is left (#1903).
                        warn!("load: the new run (process {}) is running but {why} after {waited:.0} s; ending this one so one game runs", s.child.id());
                        s.handed_over = true;
                        exit.write(AppExit::Success);
                        *h = Leaving::HandedOver;
                        return;
                    }
                    *h = Leaving::Waiting(s);
                    return;
                }
                Ok(Some(status)) => match crate::crash::stopped_at_start(s.child.id()) {
                    Some(why) => {
                        tell_player(&why);
                        format!("the new run ended ({status}) before it was ready: {why}")
                    }
                    None => format!("the new run ended ({status}) before it was ready"),
                },
                Err(e) => format!("cannot tell whether the new run is still going ({e})"),
            };
            warn!("load: {ended}; staying");
            *winit = s.settings.clone();
            uncover.write(Uncover);
        }
    }
}

/// The schedule after `Last` in which a run that handed over ends ([`end_now`]).
#[derive(bevy::ecs::schedule::ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EndNow;

/// Run [`end_now`] after every `Last` system of each frame.
pub fn add_end_now(app: &mut App) {
    app.init_schedule(EndNow);
    app.world_mut().resource_mut::<bevy::app::MainScheduleOrder>().insert_after(Last, EndNow);
    app.add_systems(EndNow, end_now);
}

/// The old run, once the new one is up: its exit logs are written, its saves on disk
/// (waited for before the screen came up), a frame capture finished (`--frame-telemetry`:
/// its writer otherwise finishes in the teardown this skips), Streamline shut down
/// (`crate::dlss::end_now`: `main`'s shutdown after `App::run` is skipped too; a player's
/// runs that left DLSS Frame Generation to the DLLs' detach crashed on this exit [observed],
/// the detach being the cause is [hypothesis], #2733); the process ends
/// here, the system freeing its memory at once.
fn end_now(world: &mut World) {
    if matches!(world.resource::<Handoff>().0, Leaving::HandedOver) {
        crate::frame_telemetry::finish_for_exit(world);
        crate::dlss::end_now(world, "the run hands over to a new one");
        // Android's private restart activity owns termination and relaunch ordering.
        #[cfg(not(target_os = "android"))]
        std::process::exit(0);
    }
}

/// A save given with `--load` that could not be loaded: the run opened the main menu
/// instead and tells the player why once its window is up ([`tell_load_failed`]).
#[derive(Resource)]
pub struct LoadFailed(pub String);

/// The message box for [`LoadFailed`], once the window is shown (a handoff's new run
/// shows it over the old run's window first).
pub fn tell_load_failed(mut commands: Commands, failed: Option<Res<LoadFailed>>, hidden: Option<Res<Hidden>>, windows: Query<&Window, With<PrimaryWindow>>) {
    let Some(failed) = failed else { return };
    if hidden.as_ref().is_some_and(|h| !h.told) || !windows.single().is_ok_and(|w| w.visible) {
        return;
    }
    tell_player(&failed.0);
    #[cfg(target_os = "android")]
    if crate::android::activity_string("loadRefused", Some(&failed.0)).is_err() {
        warn!("load: the Android load refusal view could not be opened");
    }
    commands.remove_resource::<LoadFailed>();
}

/// The new run while its window is hidden (`--handoff <file>`; module notes, step 2).
#[derive(Resource)]
pub struct Hidden {
    file: PathBuf,
    maximize: bool,
    started: Instant,
    /// Since when (and for how many frames) the screen has been up.
    ready: Option<(Instant, u32)>,
    /// Frames since the window was shown: the focus follows a frame later (a hidden window
    /// takes none), then the file.
    shown: Option<u32>,
    /// Test aid `NVF_PLAY_TEST_HANDOFF_EXIT`.
    exit_after: bool,
    /// The file is written (and when): with the test aid, the run waits for its world,
    /// then ends.
    told: bool,
    told_at: Option<Instant>,
    /// The save may be put back: the old run has ended ([`ACK_WAIT_SECS`]).
    released: bool,
}

impl Hidden {
    pub fn new(file: PathBuf, maximize: bool) -> Self {
        let exit_after = std::env::var(EXIT_AID).is_ok_and(|v| v == "1");
        Hidden { file, maximize, started: Instant::now(), ready: None, shown: None, exit_after, told: false, told_at: None, released: false }
    }
}

/// Whether a hidden new run's screen is up, and for how long; `Some(late)` once the window
/// is to be shown (`late`: the time limit was reached first).
fn settle(h: &mut Hidden, up: bool, now: Instant) -> Option<bool> {
    if up {
        let (since, frames) = h.ready.get_or_insert((now, 0));
        *frames += 1;
        if *frames >= SETTLE_FRAMES && now.duration_since(*since).as_secs_f64() >= SETTLE_SECS {
            return Some(false);
        }
    } else {
        h.ready = None;
    }
    (now.duration_since(h.started).as_secs_f64() >= SHOW_AFTER_SECS).then_some(true)
}

/// The new run's save waits while [`Hidden`] says the old run has not ended
/// (`ScriptState::held`; module notes, step 2).
pub fn hold_restore(hidden: Option<Res<Hidden>>, script: Option<ResMut<nvf_play::script::ScriptState>>) {
    let held = hidden.is_some_and(|h| !h.released);
    if let Some(mut s) = script.filter(|s| s.held != held) {
        s.held = held;
    }
}

/// The new run's side: once its loading screen (or the main menu) is up, show the window
/// over the old run's, take the focus and tell the old run.
#[allow(clippy::too_many_arguments)]
pub fn reveal(
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    hidden: Option<ResMut<Hidden>>,
    mut loading: ResMut<Loading>,
    main_menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    streamer: Res<crate::world::Streamer>,
    phys: Option<Res<nvf_play::phys::PhysState>>,
    restoring: Restoring,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Some(mut h) = hidden else { return };
    // The save waits for the old run to end (module notes, step 2).
    if !h.released {
        if let Some(t) = h.told_at.filter(|t| !h.file.exists() || t.elapsed().as_secs_f64() >= ACK_WAIT_SECS) {
            h.released = true;
            let _ = std::fs::remove_file(started_marker(&h.file));
            let gone = !h.file.exists();
            info!("handoff: {} after {:.2} s; the save goes back now", if gone { "the old run has ended" } else { "no word from the old run" }, t.elapsed().as_secs_f64());
        }
    }
    let Ok(mut window) = windows.single_mut() else { return };
    match h.shown {
        None => {
            if !loading.unseen {
                loading.unseen = true;
            }
            let up = loading.active() || main_menu.is_some_and(|m| m.open);
            let Some(late) = settle(&mut h, up, Instant::now()) else { return };
            info!("handoff: window shown after {:.2} s{}", h.started.elapsed().as_secs_f64(), if late { ", before its screen was up" } else { "" });
            window.visible = true;
            if h.maximize {
                window.set_maximized(true);
            }
            loading.unseen = false;
            h.shown = Some(0);
        }
        Some(0) => {
            window.focused = true;
            h.shown = Some(1);
        }
        Some(_) if !h.told => {
            if let Err(e) = std::fs::write(&h.file, b"") {
                warn!("handoff: cannot tell the old run ({}: {e}); it ends when its own window closes", h.file.display());
            }
            h.told = true;
            h.told_at = Some(Instant::now());
        }
        Some(_) if !h.exit_after => {
            if h.released {
                commands.remove_resource::<Hidden>();
            }
        }
        Some(_) => {
            // Test aid: the world in, as a player would first see it, or the main menu.
            let menu = main_menu.is_some_and(|m| m.open);
            let world_in = streamer.settled && phys.is_none_or(|p| p.loading_done()) && !restoring.busy() && !loading.starting();
            if menu || world_in {
                info!("test aid {EXIT_AID}: the new run took over and its {} is in; ending it", if menu { "main menu" } else { "world" });
                commands.remove_resource::<Hidden>();
                exit.write(AppExit::Success);
            }
        }
    }
}

#[cfg(test)]
mod death_smoke_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_new_run_opens_where_the_old_window_is() {
        let keep = vec!["--play".to_string(), "--msaa".into(), "8".into()];
        let r = LoadRequest { path: PathBuf::from(r"C:\saves\quicksave.nvfsave") };
        let p = Placement { at: Some(IVec2::new(-1910, 40)), size: UVec2::new(1600, 900), maximized: true };
        let args = new_run_args(&keep, Some(&r), Some(p), Path::new("ready"));
        assert_eq!(args, ["--play", "--msaa", "8", "--load", r"C:\saves\quicksave.nvfsave", "--size", "1600x900", "--window-at=-1910,40", "--window-maximized", "--handoff", "ready"]);
        let args = new_run_args(&keep, None, None, Path::new("ready"));
        assert_eq!(args, ["--play", "--msaa", "8", "--menu", "--handoff", "ready"]);
        assert_eq!(parse_at("-1910,40"), Some(IVec2::new(-1910, 40)));
        assert_eq!(parse_at("12"), None);
    }

    #[test]
    fn the_window_waits_for_the_world_then_settles() {
        let t0 = Instant::now();
        let mut h = Hidden::new(PathBuf::new(), false);
        h.started = t0;
        assert_eq!(settle(&mut h, false, t0), None);
        let late = |s: f64| t0 + Duration::from_secs_f64(s);
        // In, but not for long enough (frames, then time).
        assert_eq!(settle(&mut h, true, late(1.0)), None);
        assert_eq!(settle(&mut h, true, late(1.1)), None);
        assert_eq!(settle(&mut h, true, late(1.2)), None);
        assert_eq!(settle(&mut h, true, late(1.3)), Some(false));
        // Out again starts the wait over.
        let mut h = Hidden::new(PathBuf::new(), false);
        h.started = t0;
        for s in [1.0, 1.1, 1.2] {
            settle(&mut h, true, late(s));
        }
        assert_eq!(settle(&mut h, false, late(1.3)), None);
        assert_eq!(settle(&mut h, true, late(1.4)), None);
        // Never in: shown at the time limit.
        assert_eq!(settle(&mut h, false, late(SHOW_AFTER_SECS)), Some(true));
    }

    #[test]
    fn the_old_run_stops_waiting_for_a_new_run_that_does_not_take_part() {
        // A new run without its start marker: given time to start, then left alone (#1903).
        assert_eq!(give_up(JOIN_WAIT_SECS - 1.0, false), None);
        assert!(give_up(JOIN_WAIT_SECS, false).is_some());
        // A new run that started as one: waited for until well past its own time limit.
        assert_eq!(give_up(JOIN_WAIT_SECS, true), None);
        assert_eq!(give_up(SHOW_AFTER_SECS, true), None);
        assert!(give_up(SHOW_AFTER_SECS + GIVE_UP_MARGIN_SECS, true).is_some());
        assert_eq!(started_marker(Path::new("/tmp/nvforever-handoff-1-2.ready")), PathBuf::from("/tmp/nvforever-handoff-1-2.ready.started"));
    }
}
