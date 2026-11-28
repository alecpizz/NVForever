//! The window's mode: an ordinary window (`--window windowed`, the default) or borderless
//! full screen on the display chosen in the Display group's "Monitor" row (the first
//! display until one is chosen; `--window borderless`). The settings page's Display group
//! switches it while the game runs (`settings.rs`), kept in `NVForever.ini` like every
//! switch. Alt+Enter flips the same row from anywhere in the game (#2209,
//! `nvf_play::full_screen_key`). Each mode the primary window settles on is logged with the size
//! its surface really has, and the size it was asked for when the two differ (#4381); the
//! surface's physical and logical size and its scale factor print on a line of their own (#3884).
//!
//! **The resolution** is the Display group's first row, a dropdown of the sizes the
//! window's monitor can show (its video modes, smallest first) [owner 2026-10-18: a
//! resolution dropdown at the top of Display, a 20 s countdown back to the one before].
//! In a window it is the window's size (`--size`); in full screen the monitor's own size
//! is borderless full screen as before, and a smaller one switches the monitor to it
//! (exclusive full screen at that video mode, its highest refresh rate) [design]. The two
//! are kept apart, so leaving full screen gives back the window's size: kept in the INI
//! as `resolution=<window>;<full screen>` (`desktop`: the monitor's own), e.g.
//! `resolution=1600x900;desktop`. A resolution chosen on the page goes back to the one
//! before unless it is kept within 20 seconds (`nvf_play::settings::PendingKeep`).
//!
//! [`apply_display`] makes the window what `Args` and [`FullScreenSize`] ask whenever they
//! change, so the settings only write those (a window resized by its frame is left as it
//! is until they change).

use crate::Args;
use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use std::time::{Duration, Instant};
use bevy::window::{Monitor, MonitorSelection, OnMonitor, PrimaryMonitor, PrimaryWindow, VideoMode, VideoModeSelection, WindowMode};

/// `--window`: how the game's window is shown.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowChoice {
    /// A window of --size with a frame.
    #[default]
    Windowed,
    /// Borderless full screen on the display chosen in the Display group's "Monitor" row
    /// (its own resolution; --size is not used then, screenshots too are the display's size).
    Borderless,
}

impl WindowChoice {
    pub fn mode(self) -> WindowMode {
        match self {
            WindowChoice::Windowed => WindowMode::Windowed,
            // `Current` cannot be resolved while the window is being created (it has no
            // monitor yet) [documented: bevy_window MonitorSelection]; on Wayland there is no
            // "primary" monitor either [documented: winit Wayland backend], so the compositor
            // places the window [observed: #1641]. This is only the window's initial mode:
            // `apply_display` moves it to the chosen display (the first one until a choice is
            // kept) once the monitors are known.
            WindowChoice::Borderless => WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
        }
    }
}

/// Full screen's resolution: `None` the monitor's own (borderless), else that size
/// (exclusive full screen).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FullScreenSize(pub Option<UVec2>);

/// What the window's monitor can show: its own size (as the desktop has it, noted while
/// not in exclusive full screen) and its video modes.
#[derive(Resource, Clone, Debug, Default)]
pub struct MonitorModes {
    pub desktop: Option<UVec2>,
    pub modes: Vec<VideoMode>,
    /// Every monitor, for the Display "Monitor" list and for selecting one (its entity)
    /// once it is known.
    pub monitors: Vec<MonitorInfo>,
    /// The monitor the window is on now (its entity), for comparing against a chosen one.
    pub current: Option<Entity>,
}

/// A monitor: its connector name (stable — kept in the INI), its entity, its native size
/// (for the list's label), its position in the desktop (for a stable left-to-right,
/// top-to-bottom order) and whether it is the system's primary display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorInfo {
    pub name: String,
    pub entity: Entity,
    pub size: UVec2,
    pub position: IVec2,
    pub primary: bool,
}

impl MonitorModes {
    /// The sizes of the video modes, smallest first, none under 640x480.
    pub fn sizes(&self) -> Vec<UVec2> {
        let mut s: Vec<UVec2> = self.modes.iter().map(|m| m.physical_size).filter(|s| s.x >= 640 && s.y >= 480).collect();
        s.extend(self.desktop);
        s.sort_by_key(|s| (u64::from(s.x) * u64::from(s.y), s.x));
        s.dedup();
        s
    }

    /// The video mode of size `size` with the highest refresh rate.
    fn mode_of(&self, size: UVec2) -> Option<VideoMode> {
        self.modes.iter().filter(|m| m.physical_size == size).max_by_key(|m| (m.refresh_rate_millihertz, m.bit_depth)).copied()
    }
}

/// The display full screen opens on: the Display "Monitor" choice (kept in the INI by
/// connector name), or the system's primary display until one is chosen (or if the chosen
/// one is not attached now), else the first.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct MonitorChoice(pub Option<String>);

/// The "Monitor" list's label for a display: "Display N (WxH)".
fn monitor_label(i: usize, m: &MonitorInfo) -> String {
    format!("Display {} ({}x{})", i + 1, m.size.x, m.size.y)
}

/// A stable display order, independent of the discovery order: left-to-right, then
/// top-to-bottom (the discovery order can put the window's current display first).
fn ordered(mut monitors: Vec<MonitorInfo>) -> Vec<MonitorInfo> {
    monitors.sort_by_key(|m| (m.position.x, m.position.y));
    monitors
}

/// The index in `modes.monitors` of the display full screen goes to: the chosen monitor by
/// its name, else the system's primary display, else the first. The list is ordered left
/// to right, so the first is not the primary on a desk whose primary display is not the
/// leftmost (#3334: a portrait display left of the main one took the game). `None` while
/// no monitor is known.
fn chosen_index(modes: &MonitorModes, monitor: &MonitorChoice) -> Option<usize> {
    let chosen = monitor.0.as_ref().and_then(|name| modes.monitors.iter().position(|m| &m.name == name));
    chosen.or_else(|| modes.monitors.iter().position(|m| m.primary)).or_else(|| (!modes.monitors.is_empty()).then_some(0))
}

/// The monitor selection for [`MonitorChoice`] ([`chosen_index`]); no monitor known yet →
/// `Primary`.
fn borderless_selection(modes: &MonitorModes, monitor: &MonitorChoice) -> MonitorSelection {
    chosen_index(modes, monitor).map_or(MonitorSelection::Primary, |i| MonitorSelection::Entity(modes.monitors[i].entity))
}

pub struct WindowModePlugin;

/// Apply the chosen window settings before telemetry snapshots their physical size.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApplyWindowMode;

impl Plugin for WindowModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FullScreenSize>().init_resource::<MonitorModes>().init_resource::<MonitorChoice>().add_systems(Update, (watch_monitor, apply_display, log_mode).chain().in_set(ApplyWindowMode));
    }
}

/// The settings page's switch: full screen on or off (`apply_display` does it).
pub fn set_borderless(world: &mut World, on: bool) {
    world.resource_mut::<Args>().window = if on { WindowChoice::Borderless } else { WindowChoice::Windowed };
}

/// "WxH".
pub fn size_text(s: UVec2) -> String {
    format!("{}x{}", s.x, s.y)
}

/// A size from "WxH" (at least 320x240, at most 16384 a side).
pub fn parse_size(t: &str) -> Option<UVec2> {
    let (w, h) = t.trim().split_once(['x', 'X'])?;
    let s = UVec2::new(w.trim().parse().ok()?, h.trim().parse().ok()?);
    (s.x >= 320 && s.y >= 240 && s.x <= 16384 && s.y <= 16384).then_some(s)
}

/// The resolution row's choices: the monitor's sizes (in a window none larger than the
/// monitor), the chosen one among them.
pub fn resolution_names(world: &World) -> Vec<String> {
    let m = world.resource::<MonitorModes>();
    let windowed = world.resource::<Args>().window == WindowChoice::Windowed;
    let mut sizes: Vec<UVec2> = m.sizes().into_iter().filter(|s| !windowed || m.desktop.is_none_or(|d| s.x <= d.x && s.y <= d.y)).collect();
    if let Some(c) = parse_size(&resolution_chosen(world)) {
        if !sizes.contains(&c) {
            sizes.push(c);
            sizes.sort_by_key(|s| (u64::from(s.x) * u64::from(s.y), s.x));
        }
    }
    sizes.into_iter().map(size_text).collect()
}

/// The resolution in use: the window's, or full screen's (the monitor's own when it is
/// borderless).
pub fn resolution_chosen(world: &World) -> String {
    let args = world.resource::<Args>();
    match (args.window, world.resource::<FullScreenSize>().0, world.resource::<MonitorModes>().desktop) {
        (WindowChoice::Windowed, ..) => parse_size(&args.size).map_or(args.size.clone(), size_text),
        (WindowChoice::Borderless, Some(s), _) | (WindowChoice::Borderless, None, Some(s)) => size_text(s),
        (WindowChoice::Borderless, None, None) => "desktop".to_string(),
    }
}

/// Take a resolution from the row: the window's size, or full screen's.
pub fn resolution_pick(world: &mut World, name: &str) -> bool {
    let Some(s) = parse_size(name) else { return false };
    if world.resource::<Args>().window == WindowChoice::Windowed {
        world.resource_mut::<Args>().size = size_text(s);
    } else {
        let desktop = world.resource::<MonitorModes>().desktop;
        world.resource_mut::<FullScreenSize>().0 = (desktop != Some(s)).then_some(s);
    }
    true
}

/// Both resolutions as the INI keeps them: `<window>;<full screen>`.
pub fn resolution_save(world: &World) -> String {
    let full = world.resource::<FullScreenSize>().0.map_or("desktop".to_string(), size_text);
    format!("{};{full}", world.resource::<Args>().size)
}

/// Both resolutions from the INI's text (a window's size alone also does).
pub fn resolution_load(world: &mut World, text: &str) -> bool {
    let (window, full) = text.split_once(';').unwrap_or((text, ""));
    let Some(window) = parse_size(window) else { return false };
    let full = match full.trim() {
        "" => world.resource::<FullScreenSize>().0,
        f if f.eq_ignore_ascii_case("desktop") => None,
        f => match parse_size(f) {
            Some(s) => Some(s),
            None => return false,
        },
    };
    world.resource_mut::<Args>().size = size_text(window);
    world.resource_mut::<FullScreenSize>().0 = full;
    true
}

/// The Display "Monitor" list's choices: each display as "Display N (WxH)".
pub fn monitor_names(world: &World) -> Vec<String> {
    world.resource::<MonitorModes>().monitors.iter().enumerate().map(|(i, m)| monitor_label(i, m)).collect()
}

/// The chosen display: the saved choice while it is still attached, else the primary
/// display ([`chosen_index`]).
pub fn monitor_chosen(world: &World) -> String {
    let m = world.resource::<MonitorModes>();
    let c = world.resource::<MonitorChoice>();
    chosen_index(m, c).map(|i| monitor_label(i, &m.monitors[i])).unwrap_or_default()
}

/// Choose a display by its label; false when it is not attached now.
pub fn monitor_pick(world: &mut World, label: &str) -> bool {
    let found = world.resource::<MonitorModes>().monitors.iter().enumerate().find(|(i, m)| monitor_label(*i, m) == label).map(|(_, m)| m.name.clone());
    let Some(name) = found else { return false };
    world.resource_mut::<MonitorChoice>().0 = Some(name);
    true
}

/// The monitor choice as the INI keeps it: its connector name, or "" for none (the first
/// display).
pub fn monitor_save(world: &World) -> String {
    world.resource::<MonitorChoice>().0.clone().unwrap_or_default()
}

/// The monitor choice from its INI text; "" is none (the primary display).
pub fn monitor_load(world: &mut World, text: &str) -> bool {
    let name = text.trim();
    world.resource_mut::<MonitorChoice>().0 = (!name.is_empty()).then(|| name.to_string());
    true
}

/// Note the monitor full screen goes to ([`chosen_index`]) for the resolution list (its
/// video modes and, while not in exclusive full screen, its size), and the window's actual
/// monitor (from `OnMonitor`, for comparing against a chosen display). The modes are those
/// of the display the window is sent to, not of the one it is on: #3334 asked the portrait
/// display for a 1920x1080 at 160 Hz mode read from the other one.
fn watch_monitor(windows: Query<(&Window, Option<&OnMonitor>), With<PrimaryWindow>>, monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>, choice: Res<MonitorChoice>, mut modes: ResMut<MonitorModes>) {
    let Ok((w, on)) = windows.single() else { return };
    let all = ordered(monitors.iter().map(|(e, m, primary)| MonitorInfo { name: m.name.clone().unwrap_or_default(), entity: e, size: m.physical_size(), position: m.physical_position, primary }).collect());
    let target = chosen_index(&MonitorModes { monitors: all.clone(), ..default() }, &choice).map(|i| all[i].entity);
    let monitor = target.and_then(|e| monitors.get(e).ok()).or_else(|| on.and_then(|o| monitors.get(o.0).ok())).or_else(|| monitors.iter().find(|m| m.2)).or_else(|| monitors.iter().next());
    let Some((_, m, _)) = monitor else { return };
    let desktop = if matches!(w.mode, WindowMode::Fullscreen(..)) { modes.desktop } else { Some(m.physical_size()) };
    let current = on.map(|o| o.0);
    if modes.desktop != desktop || modes.modes != m.video_modes || modes.monitors != all || modes.current != current {
        info!("window: monitor {} {}, {} video modes", m.name.as_deref().unwrap_or("?"), desktop.map_or("?".to_string(), size_text), m.video_modes.len());
        modes.desktop = desktop;
        modes.modes = m.video_modes.clone();
        modes.monitors = all;
        modes.current = current;
    }
}

/// What [`apply_display`] last made the window: the mode, the window size, the full-screen
/// size and the chosen monitor.
type DisplayWant = (WindowChoice, String, Option<UVec2>, Option<String>);

/// [`apply_display`]'s frame-to-frame state.
#[derive(Default)]
struct DisplayState {
    /// The frame after a monitor move bounced through windowed: the target mode to enter.
    bounce: Option<WindowMode>,
    /// Windows refused to put the desktop's display mode back on leaving exclusive full
    /// screen: the window stays exclusive and the restore is asked again at this time (#3332).
    restore_retry: Option<Instant>,
}

/// How long after a refused restore of the desktop's display mode it is asked again.
const RESTORE_RETRY: Duration = Duration::from_secs(2);

/// What a change of the window's mode asks of the display before winit is given it: a video
/// mode to switch the monitor to (entering exclusive full screen, or another exclusive
/// mode), the desktop's own mode back (leaving exclusive full screen), or nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DisplayStep {
    Enter(VideoMode),
    Restore,
    Nothing,
}

/// The display step of a change from `from` to `to`. winit 0.30 makes these two changes of
/// the display's mode itself and panics on any answer but success from Windows
/// [documented: winit 0.30.13 `platform_impl/windows/window.rs` `set_fullscreen`, the
/// `assert_eq!(res, DISP_CHANGE_SUCCESSFUL)` after each `ChangeDisplaySettingsExW`;
/// observed: #3332, -1 on the restore after the 20 s revert], so [`apply_display`] asks
/// Windows first and does not hand winit a change Windows refuses.
fn display_step(from: WindowMode, to: WindowMode) -> DisplayStep {
    match (from, to) {
        _ if from == to => DisplayStep::Nothing,
        (_, WindowMode::Fullscreen(_, VideoModeSelection::Specific(m))) => DisplayStep::Enter(m),
        // Exclusive to exclusive (the current video mode): winit only enters, no restore.
        (WindowMode::Fullscreen(..), WindowMode::Fullscreen(..)) => DisplayStep::Nothing,
        (WindowMode::Fullscreen(..), _) => DisplayStep::Restore,
        _ => DisplayStep::Nothing,
    }
}

/// The name of an answer of `ChangeDisplaySettingsExW` other than success, for the log
/// [documented: Microsoft Learn, ChangeDisplaySettingsExW, return values].
fn refusal_text(code: i32) -> String {
    let name = match code {
        1 => "DISP_CHANGE_RESTART",
        -1 => "DISP_CHANGE_FAILED",
        -2 => "DISP_CHANGE_BADMODE",
        -3 => "DISP_CHANGE_NOTUPDATED",
        -4 => "DISP_CHANGE_BADFLAGS",
        -5 => "DISP_CHANGE_BADPARAM",
        -6 => "DISP_CHANGE_BADDUALVIEW",
        _ => "an unknown answer",
    };
    format!("{name} ({code})")
}

/// The display device of a monitor selection (winit names a Windows monitor by its device,
/// e.g. `\\.\DISPLAY1` [documented: winit 0.30.13 `platform_impl/windows/monitor.rs`
/// `name`]); `None`: the primary display.
fn device_of(modes: &MonitorModes, selection: MonitorSelection) -> Option<String> {
    match selection {
        MonitorSelection::Entity(e) => modes.monitors.iter().find(|m| m.entity == e).map(|m| m.name.clone()),
        _ => None,
    }
}

/// The mode to give the window for a change from `from` to `to`, once Windows has been
/// asked: a video mode Windows says it cannot set gives borderless full screen on the same
/// display instead, and a refused restore of the desktop's mode keeps the window as it is
/// (exclusive) until a later ask succeeds. Each refusal is logged.
fn checked(from: WindowMode, to: WindowMode, selection: MonitorSelection, modes: &MonitorModes, state: &mut DisplayState) -> WindowMode {
    let retrying = state.restore_retry.take();
    match display_step(from, to) {
        DisplayStep::Enter(m) => match display_check::can_enter(device_of(modes, selection).as_deref(), &m) {
            Ok(()) => to,
            Err(code) => {
                warn!("window: Windows cannot set the video mode {} at {:.0} Hz ({}): borderless full screen instead", size_text(m.physical_size), m.refresh_rate_millihertz as f32 / 1000.0, refusal_text(code));
                WindowMode::BorderlessFullscreen(selection)
            }
        },
        DisplayStep::Restore => match display_check::restore() {
            Ok(()) => {
                if retrying.is_some() {
                    info!("window: the display's own mode is back");
                }
                to
            }
            Err(code) => {
                if retrying.is_none() {
                    warn!("window: Windows refused to put the display's own mode back ({}): exclusive full screen stays, asked again every {} s", refusal_text(code), RESTORE_RETRY.as_secs());
                }
                state.restore_retry = Some(Instant::now() + RESTORE_RETRY);
                from
            }
        },
        DisplayStep::Nothing => to,
    }
}

/// Asking Windows about a display-mode change before winit makes it (#3332).
#[cfg(windows)]
mod display_check {
    use bevy::window::VideoMode;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Graphics::Gdi::{ChangeDisplaySettingsExW, CDS_FULLSCREEN, CDS_TEST, DEVMODEW, DISP_CHANGE_SUCCESSFUL, DM_BITSPERPEL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH};

    fn answer(res: i32) -> Result<(), i32> {
        if res == DISP_CHANGE_SUCCESSFUL {
            Ok(())
        } else {
            Err(res)
        }
    }

    /// Whether Windows can switch `device` (`None`: the primary display) to `m`: the mode
    /// is only tested (`CDS_TEST`), nothing changes [documented: Microsoft Learn,
    /// ChangeDisplaySettingsExW, CDS_TEST].
    pub fn can_enter(device: Option<&str>, m: &VideoMode) -> Result<(), i32> {
        // SAFETY: DEVMODEW is a plain C struct; all zeros is a valid value, and only the
        // fields named in dmFields are read.
        let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_BITSPERPEL | DM_DISPLAYFREQUENCY;
        dm.dmPelsWidth = m.physical_size.x;
        dm.dmPelsHeight = m.physical_size.y;
        dm.dmBitsPerPel = u32::from(m.bit_depth);
        // winit reports dmDisplayFrequency * 1000 [documented: winit 0.30.13 monitor.rs].
        dm.dmDisplayFrequency = m.refresh_rate_millihertz / 1000;
        let name: Option<Vec<u16>> = device.map(|d| d.encode_utf16().chain([0]).collect());
        let name_ptr = name.as_ref().map_or(null(), |n| n.as_ptr());
        // SAFETY: name_ptr is null or a NUL-terminated UTF-16 string that outlives the call;
        // dm is a valid DEVMODEW with dmSize set.
        answer(unsafe { ChangeDisplaySettingsExW(name_ptr, &dm, null_mut(), CDS_TEST, null()) })
    }

    /// Put the desktop's own display mode back, as winit does on leaving exclusive full
    /// screen (the same call: no device, no mode, `CDS_FULLSCREEN`) [documented: winit
    /// 0.30.13 window.rs `set_fullscreen`]. Done first here so a refusal is an error and not
    /// winit's panic; winit's own call after it then has nothing to change [hypothesis].
    pub fn restore() -> Result<(), i32> {
        // SAFETY: null device and mode are the documented "back to the registry's mode" call.
        answer(unsafe { ChangeDisplaySettingsExW(null(), null(), null_mut(), CDS_FULLSCREEN, null()) })
    }
}

/// Elsewhere winit does not change the display's mode with a panic on refusal: nothing to ask.
#[cfg(not(windows))]
mod display_check {
    use bevy::window::VideoMode;

    pub fn can_enter(_device: Option<&str>, _m: &VideoMode) -> Result<(), i32> {
        Ok(())
    }

    pub fn restore() -> Result<(), i32> {
        Ok(())
    }
}

/// Make the window what `Args` (`window`, `size`) and [`FullScreenSize`] ask, when they
/// change (and again once the monitor's modes are known, for exclusive full screen). Runs
/// on the main thread, where winit changes the display's mode too.
#[allow(clippy::too_many_arguments)]
fn apply_display(args: Res<Args>, full: Res<FullScreenSize>, modes: Res<MonitorModes>, monitor: Res<MonitorChoice>, mut windows: Query<&mut Window, With<PrimaryWindow>>, mut last: Local<Option<DisplayWant>>, mut state: Local<DisplayState>, _main_thread: NonSendMarker) {
    // The frame after a monitor move bounced through windowed: enter the target mode.
    if let Some(mode) = state.bounce {
        if let Ok(mut w) = windows.single_mut() {
            let selection = borderless_selection(&modes, &monitor);
            let mode = checked(w.mode, mode, selection, &modes, &mut state);
            if w.mode != mode {
                w.mode = mode;
            }
            state.bounce = None;
        }
        return;
    }
    let want = (args.window, args.size.clone(), full.0, monitor.0.clone());
    let retry_due = state.restore_retry.is_some_and(|t| Instant::now() >= t);
    // Re-applied when the choice or the monitors change: a chosen monitor's entity is only
    // known after the monitors are discovered, a frame or so after the start. Also when a
    // refused restore of the desktop's mode is due to be asked again.
    if last.as_ref() == Some(&want) && !modes.is_changed() && !retry_due {
        return;
    }
    let prev = last.replace(want);
    let Ok(mut w) = windows.single_mut() else { return };
    let prev_mode = w.mode;
    let selection = borderless_selection(&modes, &monitor);
    let mode = match (args.window, full.0) {
        (WindowChoice::Windowed, _) => WindowMode::Windowed,
        (WindowChoice::Borderless, None) => WindowMode::BorderlessFullscreen(selection),
        (WindowChoice::Borderless, Some(s)) => match modes.mode_of(s) {
            Some(m) => WindowMode::Fullscreen(selection, VideoModeSelection::Specific(m)),
            None => {
                // Not known yet (or not the monitor's): borderless until it is.
                if !modes.modes.is_empty() {
                    warn!("window: the monitor has no video mode {}; borderless full screen instead", size_text(s));
                }
                WindowMode::BorderlessFullscreen(selection)
            }
        },
    };
    if w.mode == mode {
        // Back to the mode the window has (a refused restore no longer wanted).
        state.restore_retry = None;
    } else {
        // Moving full screen to a different display: the compositor ignores a full-screen
        // request to another output while already full screen [observed: #1641], so bounce
        // through windowed (one frame) and re-enter on the next. Only when the chosen
        // display is a specific monitor other than the one the window is already on.
        let target = match selection {
            MonitorSelection::Entity(e) => Some(e),
            _ => None,
        };
        let moving = target.is_some() && modes.current != target
            && matches!(w.mode, WindowMode::BorderlessFullscreen(_) | WindowMode::Fullscreen(..))
            && matches!(mode, WindowMode::BorderlessFullscreen(_) | WindowMode::Fullscreen(..));
        let next = if moving { WindowMode::Windowed } else { mode };
        let next = checked(w.mode, next, selection, &modes, &mut state);
        if next != w.mode {
            if moving {
                state.bounce = Some(mode);
            }
            w.mode = next;
        }
    }
    // A window of --size (leaving full screen keeps no size of its own [observed:
    // 800x600]). main checked --size at the start. Only apply --size on initial apply,
    // when --size itself changed, or when returning to windowed mode from full screen;
    // a window resized or maximized by the user must not be snapped back on monitor
    // refreshes (module doc).
    if should_apply_windowed_size(prev.as_ref().map(|p| p.1.as_str()), &args.size, prev_mode) {
        if let (WindowMode::Windowed, WindowMode::Windowed, Some(s)) = (mode, w.mode, parse_size(&args.size)) {
            if w.resolution.physical_size() != s {
                w.resolution.set_physical_resolution(s.x, s.y);
            }
        }
    }
}

/// Whether `apply_display` should enforce `args.size` on a windowed surface: true on
/// first apply, when the requested size changed, or when leaving a full-screen mode;
/// false when re-running for monitor-list changes while already windowed with the same
/// requested size [design].
fn should_apply_windowed_size(prev_size: Option<&str>, current_size: &str, prev_mode: WindowMode) -> bool {
    prev_size != Some(current_size) || prev_mode != WindowMode::Windowed
}

#[cfg(test)]
mod windowed_resize_tests;

/// [`log_mode`]'s frame-to-frame state: the mode and physical size it last printed, and the one
/// the window had the frame before.
#[derive(Default)]
struct LogState {
    printed: Option<(WindowMode, UVec2)>,
    before: Option<(WindowMode, UVec2)>,
}

impl LogState {
    /// This frame's `(mode, physical size)` when it is to be printed: at once while nothing has
    /// been printed yet (the start's), and otherwise once the size has held for a frame — a
    /// frame's border dragged across the screen changes the size every frame, and only the size
    /// it comes to rest at is worth a line [observed: the log before #4381 printed a window's
    /// size only when it equaled `--size`, so the surface 1280x721 of `--size 1280x720` (#3884)
    /// was never printed].
    fn holds(&mut self, now: (WindowMode, UVec2)) -> Option<(WindowMode, UVec2)> {
        let before = self.before;
        self.before = Some(now);
        let first = self.printed.is_none();
        if self.printed == Some(now) || !(first || before == Some(now)) {
            return None;
        }
        self.printed = Some(now);
        Some(now)
    }
}

/// The size the window was asked to have in `mode`: `--size` for a window, the display's own
/// size for borderless full screen ([`MonitorModes::desktop`], as the desktop has it), the video
/// mode's own for exclusive full screen. `None` while it is not known: the monitors are
/// discovered a frame or so after the start.
fn asked_size(mode: WindowMode, args: &Args, modes: &MonitorModes) -> Option<UVec2> {
    match mode {
        WindowMode::Windowed => parse_size(&args.size),
        WindowMode::BorderlessFullscreen(_) => modes.desktop,
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(m)) => Some(m.physical_size),
        WindowMode::Fullscreen(..) => None,
    }
}

/// The line one mode and surface size is logged as: the mode and the size the window's surface
/// really has — [`Window::physical_size`], which bevy_winit keeps from winit's own `PhysicalSize`
/// on each resize [documented: engine/vendor/bevy_winit/src/state.rs `react_to_resize`] — and,
/// when that is not the size the mode asked for ([`asked_size`]), the size asked for as well
/// (#4381).
fn mode_line(mode: WindowMode, asked: Option<UVec2>, surface: UVec2) -> String {
    let line = |base: &str| match asked {
        Some(a) if a != surface => format!("{base}, asked {}, surface {}", size_text(a), size_text(surface)),
        _ => format!("{base}, {}", size_text(surface)),
    };
    match mode {
        WindowMode::Windowed => line("window: windowed"),
        WindowMode::BorderlessFullscreen(_) => line("window: borderless full screen"),
        // The video mode names the size asked for; the surface's own is printed only when it
        // differs, so the line reads "…, 1280x720 at 60 Hz, surface 1280x721".
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(m)) => {
            let base = format!("window: exclusive full screen, {} at {:.0} Hz", size_text(m.physical_size), m.refresh_rate_millihertz as f32 / 1000.0);
            if m.physical_size == surface {
                base
            } else {
                format!("{base}, surface {}", size_text(surface))
            }
        }
        WindowMode::Fullscreen(..) => line("window: exclusive full screen"),
    }
}

/// Last physical surface reported by [`log_mode`], to log the actual physical size and
/// scale factor on startup, mode switches, scale changes, full-screen compositor resizes,
/// and windowed mismatches against `--size` without spamming every pixel of an interactive
/// border drag [design, #3884].
#[derive(Default)]
struct SurfaceLog {
    last: Option<(WindowMode, UVec2, u32)>,
    last_asked: Option<String>,
}

/// Format the `window: surface ...` diagnostic line when the window's physical surface
/// state should be logged; `None` when unchanged or suppressed mid-drag.
fn surface_line(state: &mut SurfaceLog, mode: WindowMode, physical: UVec2, logical: Vec2, scale: f32, asked: &str) -> Option<String> {
    let scale_bits = scale.to_bits();
    let now = (mode, physical, scale_bits);
    let asked_size = parse_size(asked);
    let should_log = match state.last {
        None => true,
        Some(prev) if prev == now => false,
        Some((prev_mode, prev_size, prev_scale)) => {
            prev_mode != mode
                || prev_scale != scale_bits
                || mode != WindowMode::Windowed
                || state.last_asked.as_deref() != Some(asked)
                || asked_size == Some(physical)
                || asked_size == Some(prev_size)
        }
    };
    state.last_asked = Some(asked.to_string());
    if !should_log {
        return None;
    }
    state.last = Some(now);
    let mismatch = if mode == WindowMode::Windowed && asked_size.is_some_and(|a| a != physical) {
        format!(" (asked {asked})")
    } else {
        String::new()
    };
    Some(format!(
        "window: surface {} physical, {:.0}x{:.0} logical, scale {scale:.3}{mismatch}",
        size_text(physical),
        logical.x,
        logical.y
    ))
}

/// One line each time the primary window settles on a mode and a surface size (the first: the
/// start's), in every mode including borderless full screen, so a size that is not the one asked
/// for is diagnosable from the log alone (#4381, the pixels #3884 cost a code trace over). The
/// physical size, logical size and scale factor print apart ([`surface_line`], #3884).
fn log_mode(args: Res<Args>, modes: Res<MonitorModes>, windows: Query<&Window, With<PrimaryWindow>>, mut log: Local<LogState>, mut surface: Local<SurfaceLog>) {
    let Ok(w) = windows.single() else { return };
    if let Some((mode, size)) = log.holds((w.mode, w.physical_size())) {
        info!("{}", mode_line(mode, asked_size(mode, &args, &modes), size));
    }
    if let Some(line) = surface_line(&mut surface, w.mode, w.physical_size(), Vec2::new(w.resolution.width(), w.resolution.height()), w.resolution.scale_factor(), &args.size) {
        info!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_the_surface_size_and_the_size_asked_when_they_differ() {
        let (w, tall) = (UVec2::new(1280, 720), UVec2::new(1280, 721));
        // The size asked, met: the line reads as it always has.
        assert_eq!(mode_line(WindowMode::Windowed, Some(w), w), "window: windowed, 1280x720");
        // #4381: a surface one row off the size asked read as the size asked (the line printed
        // the logical size, and only when it equaled --size); now both are named.
        assert_eq!(mode_line(WindowMode::Windowed, Some(w), tall), "window: windowed, asked 1280x720, surface 1280x721");
        // Borderless prints the monitor's surface in every case, not no size at all.
        let borderless = WindowMode::BorderlessFullscreen(MonitorSelection::Primary);
        let (desk, one_more) = (UVec2::new(3840, 2160), UVec2::new(3840, 2161));
        assert_eq!(mode_line(borderless, Some(desk), desk), "window: borderless full screen, 3840x2160");
        assert_eq!(mode_line(borderless, Some(desk), one_more), "window: borderless full screen, asked 3840x2160, surface 3840x2161");
        // The size is printed even when what was asked is not known yet (no monitor discovered).
        assert_eq!(mode_line(borderless, None, one_more), "window: borderless full screen, 3840x2161");
        // Exclusive full screen names the video mode it asked for; the surface only when it differs.
        let mode = VideoMode { physical_size: w, bit_depth: 32, refresh_rate_millihertz: 60_000 };
        let full = WindowMode::Fullscreen(MonitorSelection::Primary, VideoModeSelection::Specific(mode));
        assert_eq!(mode_line(full, Some(w), w), "window: exclusive full screen, 1280x720 at 60 Hz");
        assert_eq!(mode_line(full, Some(w), tall), "window: exclusive full screen, 1280x720 at 60 Hz, surface 1280x721");
    }

    #[test]
    fn a_size_is_printed_once_it_holds_and_each_state_once() {
        let (a, b, c) = ((WindowMode::Windowed, UVec2::new(1280, 720)), (WindowMode::Windowed, UVec2::new(1280, 721)), (WindowMode::Windowed, UVec2::new(1290, 730)));
        let mut log = LogState::default();
        // The start's state is printed at once, and once.
        assert_eq!(log.holds(a), Some(a));
        assert_eq!(log.holds(a), None);
        // A size that changes every frame (a border dragged) is passed over...
        assert_eq!(log.holds(b), None);
        // ...and printed when it comes to rest.
        assert_eq!(log.holds(b), Some(b));
        assert_eq!(log.holds(c), None);
        assert_eq!(log.holds(c), Some(c));
        // A change of mode is printed once its size holds, and the way back too, at the same size.
        let borderless = (WindowMode::BorderlessFullscreen(MonitorSelection::Primary), UVec2::new(3840, 2160));
        assert_eq!(log.holds(borderless), None, "the frame the mode changed is still settling");
        assert_eq!(log.holds(borderless), Some(borderless));
        let back = (WindowMode::Windowed, UVec2::new(3840, 2160));
        assert_eq!(log.holds(back), None);
        assert_eq!(log.holds(back), Some(back));
        // The mode bounce through windowed while moving displays is one frame: it is not printed.
        let bounce = (WindowMode::Windowed, UVec2::new(1280, 800));
        assert_eq!(log.holds(bounce), None);
        let target = (WindowMode::BorderlessFullscreen(MonitorSelection::Primary), UVec2::new(3840, 2160));
        assert_eq!(log.holds(target), None);
        assert_eq!(log.holds(target), Some(target));
    }

    #[test]
    fn the_size_asked_is_size_for_a_window_and_the_display_or_the_mode_in_full_screen() {
        use clap::Parser;
        let args = Args::parse_from(["nvforever", "--size", "1280x720"]);
        let modes = MonitorModes { desktop: Some(UVec2::new(1920, 1080)), ..default() };
        assert_eq!(asked_size(WindowMode::Windowed, &args, &modes), Some(UVec2::new(1280, 720)));
        assert_eq!(asked_size(WindowMode::BorderlessFullscreen(MonitorSelection::Primary), &args, &modes), Some(UVec2::new(1920, 1080)), "borderless asks for the display's own size, not --size");
        let mode = VideoMode { physical_size: UVec2::new(1600, 900), bit_depth: 32, refresh_rate_millihertz: 144_000 };
        assert_eq!(asked_size(WindowMode::Fullscreen(MonitorSelection::Primary, VideoModeSelection::Specific(mode)), &args, &modes), Some(UVec2::new(1600, 900)));
        // No display discovered yet: nothing is asked of the window.
        assert_eq!(asked_size(WindowMode::BorderlessFullscreen(MonitorSelection::Primary), &args, &MonitorModes::default()), None);
    }

    #[test]
    fn sizes_read_and_write_as_wxh() {
        assert_eq!(parse_size(" 1600x900 "), Some(UVec2::new(1600, 900)));
        assert_eq!(parse_size("1600X900"), Some(UVec2::new(1600, 900)));
        assert_eq!(parse_size("100x100"), None);
        assert_eq!(parse_size("wide"), None);
        assert_eq!(size_text(UVec2::new(1280, 800)), "1280x800");
    }

    #[test]
    fn the_monitor_sizes_are_listed_smallest_first_once_each() {
        let mode = |x, y, hz| VideoMode { physical_size: UVec2::new(x, y), bit_depth: 32, refresh_rate_millihertz: hz };
        let m = MonitorModes { desktop: Some(UVec2::new(1920, 1080)), modes: vec![mode(1280, 720, 60000), mode(1920, 1080, 60000), mode(1920, 1080, 144000), mode(640, 400, 60000), mode(1600, 900, 60000)], ..default() };
        assert_eq!(m.sizes(), [UVec2::new(1280, 720), UVec2::new(1600, 900), UVec2::new(1920, 1080)]);
        assert_eq!(m.mode_of(UVec2::new(1920, 1080)).map(|m| m.refresh_rate_millihertz), Some(144000), "the highest refresh rate");
        assert_eq!(m.mode_of(UVec2::new(1024, 768)), None);
    }

    #[test]
    fn borderless_opens_on_the_chosen_display_else_the_first() {
        let e1 = Entity::from_raw_u32(1).unwrap();
        let e2 = Entity::from_raw_u32(2).unwrap();
        let info = |name: &str, e| MonitorInfo { name: name.to_string(), entity: e, size: UVec2::new(1920, 1080), position: IVec2::ZERO, primary: false };
        let modes = MonitorModes { monitors: vec![info("DP-1", e1), info("HDMI-A-1", e2)], ..default() };
        // No choice and no display marked primary: the first display.
        assert_eq!(borderless_selection(&modes, &MonitorChoice(None)), MonitorSelection::Entity(e1));
        // A chosen display that is attached: its entity.
        assert_eq!(borderless_selection(&modes, &MonitorChoice(Some("HDMI-A-1".into()))), MonitorSelection::Entity(e2));
        // A chosen display that is not attached now (unplugged): the first display (none is
        // marked primary here).
        assert_eq!(borderless_selection(&modes, &MonitorChoice(Some("DP-2".into()))), MonitorSelection::Entity(e1));
        // No monitors known yet: the initial (primary) selection.
        assert_eq!(borderless_selection(&MonitorModes::default(), &MonitorChoice(None)), MonitorSelection::Primary);
    }

    /// #3334: a portrait display left of the primary one. With no choice kept the game goes
    /// to the primary display, not the leftmost, and the Monitor row says so.
    #[test]
    fn with_no_choice_full_screen_goes_to_the_primary_display() {
        let (portrait, main) = (Entity::from_raw_u32(1).unwrap(), Entity::from_raw_u32(2).unwrap());
        let monitors = ordered(vec![
            MonitorInfo { name: "DISPLAY1".into(), entity: main, size: UVec2::new(3840, 2160), position: IVec2::ZERO, primary: true },
            MonitorInfo { name: "DISPLAY2".into(), entity: portrait, size: UVec2::new(1080, 1920), position: IVec2::new(-1080, 0), primary: false },
        ]);
        let modes = MonitorModes { monitors, ..default() };
        assert_eq!(modes.monitors[0].entity, portrait, "the list stays left to right");
        assert_eq!(borderless_selection(&modes, &MonitorChoice(None)), MonitorSelection::Entity(main));
        assert_eq!(borderless_selection(&modes, &MonitorChoice(Some("DISPLAY2".into()))), MonitorSelection::Entity(portrait));
        // A kept choice that is unplugged: the primary again.
        assert_eq!(borderless_selection(&modes, &MonitorChoice(Some("DISPLAY3".into()))), MonitorSelection::Entity(main));
        let mut world = World::new();
        world.insert_resource(modes);
        world.init_resource::<MonitorChoice>();
        assert_eq!(monitor_chosen(&world), "Display 2 (3840x2160)");
    }

    #[test]
    fn the_monitor_list_picks_a_display_by_label() {
        let mut world = World::new();
        world.init_resource::<MonitorModes>();
        world.init_resource::<MonitorChoice>();
        world.resource_mut::<MonitorModes>().monitors = vec![MonitorInfo { name: "HDMI-A-1".to_string(), entity: Entity::from_raw_u32(1).unwrap(), size: UVec2::new(1920, 1080), position: IVec2::ZERO, primary: true }];
        // The list shows each display's label, and the default (nothing saved) is the first.
        assert_eq!(monitor_names(&world), vec!["Display 1 (1920x1080)".to_string()]);
        assert_eq!(monitor_chosen(&world), "Display 1 (1920x1080)");
        // Pick it by label; a label that is not attached is refused.
        assert!(monitor_pick(&mut world, "Display 1 (1920x1080)"));
        assert_eq!(monitor_chosen(&world), "Display 1 (1920x1080)");
        assert!(!monitor_pick(&mut world, "Display 2 (2560x1440)"));
        // A saved display that is then unplugged reads as the first (the effective selection).
        world.resource_mut::<MonitorModes>().monitors.clear();
        assert_eq!(monitor_chosen(&world), "");
    }

    #[test]
    fn displays_are_ordered_left_to_right_then_top_to_bottom() {
        let m = |x, y| MonitorInfo { name: format!("({x},{y})"), entity: Entity::PLACEHOLDER, size: UVec2::ZERO, position: IVec2::new(x, y), primary: false };
        let ordered = ordered(vec![m(0, 100), m(-200, 0), m(0, 0), m(200, 0)]);
        let pos: Vec<IVec2> = ordered.iter().map(|m| m.position).collect();
        assert_eq!(pos, vec![IVec2::new(-200, 0), IVec2::new(0, 0), IVec2::new(0, 100), IVec2::new(200, 0)]);
    }

    #[test]
    fn only_leaving_or_entering_exclusive_full_screen_asks_the_display() {
        let mode = |x, y, hz| VideoMode { physical_size: UVec2::new(x, y), bit_depth: 32, refresh_rate_millihertz: hz };
        let sel = MonitorSelection::Primary;
        let small = WindowMode::Fullscreen(sel, VideoModeSelection::Specific(mode(640, 480, 144_000)));
        let other = WindowMode::Fullscreen(sel, VideoModeSelection::Specific(mode(800, 600, 60_000)));
        let borderless = WindowMode::BorderlessFullscreen(sel);
        // #3332: the 20 s revert from 640x480 exclusive to borderless is a restore.
        assert_eq!(display_step(small, borderless), DisplayStep::Restore);
        assert_eq!(display_step(small, WindowMode::Windowed), DisplayStep::Restore);
        assert_eq!(display_step(borderless, small), DisplayStep::Enter(mode(640, 480, 144_000)));
        assert_eq!(display_step(WindowMode::Windowed, small), DisplayStep::Enter(mode(640, 480, 144_000)));
        // One exclusive mode to another: winit only enters the new one.
        assert_eq!(display_step(small, other), DisplayStep::Enter(mode(800, 600, 60_000)));
        assert_eq!(display_step(small, small), DisplayStep::Nothing);
        assert_eq!(display_step(borderless, WindowMode::Windowed), DisplayStep::Nothing);
        assert_eq!(display_step(WindowMode::Windowed, borderless), DisplayStep::Nothing);
    }

    #[test]
    fn a_refused_display_change_is_named_in_the_log() {
        assert_eq!(refusal_text(-1), "DISP_CHANGE_FAILED (-1)");
        assert_eq!(refusal_text(-2), "DISP_CHANGE_BADMODE (-2)");
        assert_eq!(refusal_text(7), "an unknown answer (7)");
    }

    #[test]
    fn the_display_device_is_the_chosen_monitor_s_name() {
        let mut modes = MonitorModes::default();
        let e = Entity::from_raw_u32(3).unwrap();
        modes.monitors = vec![MonitorInfo { name: r"\.\DISPLAY2".to_string(), entity: e, size: UVec2::new(1920, 1080), position: IVec2::ZERO, primary: false }];
        assert_eq!(device_of(&modes, MonitorSelection::Entity(e)).as_deref(), Some(r"\.\DISPLAY2"));
        assert_eq!(device_of(&modes, MonitorSelection::Primary), None);
    }

    /// #3884: the physical surface size and scale factor are logged on startup (including
    /// when the OS hands back a different height than `--size`), on scale or full-screen
    /// surface changes, and on transitions away from `--size`, without spamming mid-drag.
    #[test]
    fn surface_log_reports_mismatches_and_full_screen_resizes_without_drag_spam() {
        let mut s = SurfaceLog::default();
        let first = surface_line(&mut s, WindowMode::Windowed, UVec2::new(1280, 720), Vec2::new(1280.0, 720.0), 1.0, "1280x720");
        assert_eq!(first.as_deref(), Some("window: surface 1280x720 physical, 1280x720 logical, scale 1.000"));
        assert_eq!(surface_line(&mut s, WindowMode::Windowed, UVec2::new(1280, 720), Vec2::new(1280.0, 720.0), 1.0, "1280x720"), None);
        // OS configure hands back 1280x721 while --size asked for 1280x720: logged once with "(asked 1280x720)".
        let off = surface_line(&mut s, WindowMode::Windowed, UVec2::new(1280, 721), Vec2::new(1280.0, 721.0), 1.0, "1280x720");
        assert_eq!(off.as_deref(), Some("window: surface 1280x721 physical, 1280x721 logical, scale 1.000 (asked 1280x720)"));
        // Subsequent drag frames away from --size stay quiet.
        assert_eq!(surface_line(&mut s, WindowMode::Windowed, UVec2::new(1284, 725), Vec2::new(1284.0, 725.0), 1.0, "1280x720"), None);
        // Scale factor change logs immediately.
        let scaled = surface_line(&mut s, WindowMode::Windowed, UVec2::new(1284, 725), Vec2::new(642.0, 362.5), 2.0, "1280x720");
        assert_eq!(scaled.as_deref(), Some("window: surface 1284x725 physical, 642x362 logical, scale 2.000 (asked 1280x720)"));
        // Borderless full screen logs both the mode switch and the compositor's subsequent resize.
        let fs = WindowMode::BorderlessFullscreen(MonitorSelection::Primary);
        let enter = surface_line(&mut s, fs, UVec2::new(1280, 721), Vec2::new(1280.0, 721.0), 1.0, "1280x720");
        assert_eq!(enter.as_deref(), Some("window: surface 1280x721 physical, 1280x721 logical, scale 1.000"));
        let full = surface_line(&mut s, fs, UVec2::new(3840, 2161), Vec2::new(3840.0, 2161.0), 1.0, "1280x720");
        assert_eq!(full.as_deref(), Some("window: surface 3840x2161 physical, 3840x2161 logical, scale 1.000"));
    }
}
