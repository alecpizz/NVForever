//! How fast frames are shown: the display sync (vsync off, vsync, or vsync with variable
//! refresh) and an optional cap on the frame rate (docs/design/graphics-enhanced.md §3.10).
//! Both are settings-page rows (`settings.rs`); vsync starts on, as Bevy's default, except
//! while `--log-frame-times` measures (then off and not on the page), and there is no cap
//! unless one is chosen (while measuring, the cap is off the page too, and the test aid
//! `--frame-cap <fps>` sets one for the run). Neither changes the picture.
//!
//! **Variable refresh** (G-Sync, FreeSync; #1082) keeps vsync on and caps the frame rate a
//! few frames below the monitor's refresh rate ([`vrr_cap`]), so a frame never comes in
//! faster than the screen can refresh and never waits on vsync: a VRR screen then shows
//! each frame as it is done. The refresh rate is the window's monitor's current video mode
//! as winit reports it (`Monitor::refresh_rate_millihertz`), or, in exclusive full screen,
//! the video mode the window took (winit's docs say to use that one then) [documented:
//! winit 0.30 `MonitorHandle::refresh_rate_millihertz`]. Whether the screen really runs
//! variable refresh is the driver's (its control panel's G-SYNC / FreeSync switch and, in
//! borderless full screen, whether it allows it for windowed games): docs/spec-notes/
//! display-sync.md. The game cannot see it; the log names the cap it set.
//! With DLSS Frame Generation (which needs vsync off on Vulkan) variable refresh presents with
//! vsync off and keeps its cap, handed to Reflex ([`FramePace::presents_vsync`], [`wait_for_cap`]):
//! frames still come in under the refresh rate, so the screen shows each as it is done.
//!
//! **One limiter at a time** (2026-11-23): the cap counts frames shown, and exactly one thing
//! holds the frames to it. While DLSS-G runs that is Reflex's frame limit, the cap whole (Reflex
//! holds the frames shown, generated ones included), and the limiter here does not wait.
//! Otherwise it is the limiter here ([`own_cap`]): with FSR frame generation at half the cap
//! (two frames shown for each real one) or FSR's own cap under half the refresh rate, the lower;
//! with Vertical sync a cap at or above the refresh rate is vsync's alone. Latency reduction
//! (Reflex's or Anti-Lag's sleep) sets no limit of its own outside DLSS-G (minimum interval and
//! `maxFPS` 0). Variable refresh keeps vsync as a backstop only: the cap stays under the refresh
//! rate, so vsync never holds a frame that came on time.
//!
//! The cap waits once a frame until a frame's time (1/cap s) has passed since the last one,
//! sleeping and then spinning the last millisecond (a sleep alone can wake a millisecond
//! late). Bevy's renderer runs a frame behind the main world, so it is held to the same rate.
//!
//! **Where it waits** (2026-11-10): right after the frame just simulated is handed to the
//! render thread, before the next frame's input is read ([`after_hand_off`]; `anti_lag.rs`
//! wraps that extract). Until then it waited at the end of the main schedule (`Last`): after
//! the frame's input was read and the frame simulated, and before it was handed over, so the
//! whole wait lay between the player's input and the picture [observed 2026-11-10,
//! `NVF_PACE_LOG`: docs/design/frame-generation.md "Measurements"]. Without pipelined
//! rendering there is no such hand-off and it waits in `Last` as before.
//!
//! **A late frame** is made up for, a little: the frames keep a schedule a frame's time
//! apart, and after a late one the next frames may each come [`REPAY`] of a frame's time
//! early until the schedule is met again, so the mean rate is the cap's and no frame comes
//! much faster than the cap allows. More than a frame's time behind is forgiven.
//!
//! **Frame generation's presenter** (`fg_presenter.rs`) shows each real frame as two paced
//! presents that take nearly the whole of a real frame's time, so once it is behind (a late
//! frame, then one on time) it stays behind: frames wait in it, done, for up to a frame's
//! time, all of it latency [observed 2026-11-10, 4K, 76 real fps: the first present 9-12 ms
//! after the hand-over, 3-4 of them the GPU's]. It reports how long each frame waited
//! ([`PRESENTER_LAG_NANOS`]) and the limiter holds the next frame back by a part of that
//! ([`catch_up`]): the wait moves from after the frame is drawn to before its input is read,
//! where it costs no latency, and the presenter is caught up within a few frames.
//! `NVF_FG_LAG_FEEDBACK=0` turns it off, for comparisons.
//!
//! The present modes the window's surface supports are logged once, with the one vsync
//! takes there ([`log_present_modes`]).

use bevy::prelude::*;
use bevy::render::renderer::{RenderAdapter, RenderInstance};
use bevy::render::view::ExtractedWindow;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::{Monitor, OnMonitor, PresentMode, PrimaryMonitor, PrimaryWindow, RawHandleWrapper, VideoModeSelection, WindowMode};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The frame-rate caps the settings page offers (0: none).
pub const CAPS: [u32; 6] = [0, 30, 60, 90, 120, 144];

/// How frames meet the display's refresh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DisplaySync {
    /// Shown as soon as drawn (may tear).
    Off,
    /// Each frame waits for the refresh.
    #[default]
    Vsync,
    /// Vsync, and the frame rate capped below the refresh rate for a G-Sync / FreeSync
    /// screen ([`vrr_cap`]).
    Variable,
}

impl DisplaySync {
    /// The settings page's choices, in order.
    pub const ALL: [DisplaySync; 3] = [DisplaySync::Off, DisplaySync::Vsync, DisplaySync::Variable];

    /// The name the page shows.
    pub fn name(self) -> &'static str {
        match self {
            DisplaySync::Off => "Off",
            DisplaySync::Vsync => "Vertical sync",
            DisplaySync::Variable => "Variable refresh",
        }
    }

    /// As kept in `NVForever.ini` (`display-sync=`).
    pub fn stored(self) -> &'static str {
        match self {
            DisplaySync::Off => "off",
            DisplaySync::Vsync => "vsync",
            DisplaySync::Variable => "variable",
        }
    }

    /// From the page's name or the INI's text, any case; also the old `vsync=` switch's
    /// values (1/0, on/off, true/false, yes/no) so an INI written before keeps working.
    pub fn parse(t: &str) -> Option<DisplaySync> {
        let t = t.trim().to_ascii_lowercase();
        DisplaySync::ALL.into_iter().find(|d| d.stored() == t || d.name().to_ascii_lowercase() == t).or(match t.as_str() {
            "1" | "on" | "true" | "yes" => Some(DisplaySync::Vsync),
            "0" | "false" | "no" => Some(DisplaySync::Off),
            "vrr" | "g-sync" | "gsync" | "freesync" => Some(DisplaySync::Variable),
            _ => None,
        })
    }

    /// Whether frames wait for the refresh.
    pub fn vsync(self) -> bool {
        self != DisplaySync::Off
    }
}

/// The display sync and the frame-rate cap.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FramePace {
    pub sync: DisplaySync,
    /// Frames per second at most, as chosen on the page; 0: no cap.
    pub cap: u32,
}

impl FramePace {
    /// Whether the window presents with vsync, `dlssg` whether DLSS Frame Generation is asked
    /// for (`frame_gen::DlssgRequest`): as the display sync says, except that Variable refresh
    /// presents with vsync off while DLSS-G is asked for. DLSS-G needs vsync off on Vulkan
    /// (`frame_gen::dlss_unavailable_reason`); its frames are held under the refresh rate by
    /// variable refresh's cap, handed to Reflex ([`wait_for_cap`]), so a G-Sync screen still
    /// shows each one as it comes, without tearing [design: the vsync-off-and-cap setup VRR
    /// screens are run with; docs/spec-notes/display-sync.md].
    pub fn presents_vsync(&self, dlssg: bool) -> bool {
        match self.sync {
            DisplaySync::Off => false,
            DisplaySync::Vsync => true,
            DisplaySync::Variable => !dlssg,
        }
    }
}

/// The window's monitor's refresh rate in millihertz (`None`: not known yet), and the cap
/// variable refresh adds (0: none).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refresh {
    pub millihertz: Option<u32>,
    pub vrr_cap: u32,
}

pub struct FramePacePlugin(pub FramePace);

/// `NVF_PACE_LOG=1`: every ten seconds one line with what the limiter costs in latency: the
/// mean time from the start of a frame of the main world (its input is read just before) to
/// the frame's hand-off to the render thread, the limiter's wait a frame and where it fell,
/// and the CPU time the render thread spent recording FSR's and frame generation's passes.
/// A measuring aid.
static PACE_LOG: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var("NVF_PACE_LOG").is_ok_and(|v| v.trim() == "1"));

/// CPU time recording FSR's ([`Recorded::Fsr`]) and frame generation's passes, and the frames
/// counted, since the last line (`NVF_PACE_LOG`).
static RECORDED: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// Whose recording [`recorded`] times.
#[derive(Clone, Copy)]
pub(crate) enum Recorded {
    Fsr = 0,
    FrameGeneration = 1,
}

/// When a pass's recording begins, with `NVF_PACE_LOG` (else `None`, and nothing is timed).
pub(crate) fn recording() -> Option<Instant> {
    PACE_LOG.then(Instant::now)
}

/// Add the time since `started` ([`recording`]) to `which`'s total; `frame` counts a frame.
pub(crate) fn recorded(which: Recorded, started: Option<Instant>, frame: bool) {
    if let Some(started) = started {
        RECORDED[which as usize].fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if frame {
            RECORDED[2 + which as usize].fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// `NVF_PACE_LOG`'s sums since its last line.
#[derive(Resource, Default)]
pub(crate) struct PaceLog {
    frame_start: Option<Instant>,
    since: Option<Instant>,
    frames: u32,
    to_hand_off: Duration,
    waited: Duration,
    /// Of `waited`, for frame generation's presenter to catch up ([`catch_up`]).
    held: Duration,
}

/// How long the last frame handed to frame generation's presenter waited there before its first
/// present, beyond the GPU's work on it (`fg_presenter.rs` `run`); taken by the limiter
/// ([`wait_for_cap`]), which holds the next frame back for it ([`catch_up`]).
pub(crate) static PRESENTER_LAG_NANOS: AtomicU64 = AtomicU64::new(0);

/// What the limiter held frames back for the presenter since the presenter last took it
/// (`fg_presenter.rs` `Presenter::send`): left out of the frame times its pacing follows.
/// Counted in, the presents' spacing grew with the held frames and nothing was caught up
/// [observed 2026-11-10: half a millisecond held on every frame, 74 real fps where 76 were
/// reached before, the wait in the presenter still 3 ms over the GPU's].
pub(crate) static HELD_FOR_PRESENTER_NANOS: AtomicU64 = AtomicU64::new(0);

/// `NVF_FG_LAG_FEEDBACK=0`: the limiter ignores the presenter's lag, for comparisons.
static LAG_FEEDBACK: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| !std::env::var("NVF_FG_LAG_FEEDBACK").is_ok_and(|v| v.trim() == "0"));

/// How long the limiter holds the next frame back when the last one waited `lag` in frame
/// generation's presenter: half of what is over a millisecond (the presenter's own
/// polling and pacing leave about that), at most a millisecond and a half a frame [design: a
/// frame's wait is known a frame late, so the whole of it at once would overshoot. The frames
/// it costs are what the presenter falls behind by, whatever the share: a larger share only
/// leaves a shorter wait; a quarter left frames waiting 4.4 ms beyond the GPU's work, half
/// 3.3 ms, observed 2026-11-10, docs/design/frame-generation.md "Measurements"].
fn catch_up(lag: Duration) -> Duration {
    lag.saturating_sub(Duration::from_millis(1)).mul_f64(0.5).min(Duration::from_micros(1500))
}

/// The limiter's clock: where the frames' schedule stands and when the last capped frame went
/// on ([`next_frame`]); `None` while there is no cap.
#[derive(Resource, Default)]
pub(crate) struct CapClock(Option<(Instant, Instant)>);

/// The limiter waits after the hand-off to the render thread ([`after_hand_off`]), not in
/// `Last` ([`cap_rate`]): set by `anti_lag.rs` when it wraps pipelined rendering's extract.
#[derive(Resource)]
pub(crate) struct WaitsAfterHandOff;

fn pace_log_frame_start(mut log: ResMut<PaceLog>) {
    log.frame_start = Some(Instant::now());
}

/// Right after the frame just simulated is handed to the render thread (pipelined rendering;
/// `anti_lag.rs` wraps that extract and calls this before it announces the next frame's
/// input).
pub(crate) fn after_hand_off(world: &mut World) {
    pace_log(world);
    if world.contains_resource::<WaitsAfterHandOff>() {
        wait_for_cap(world);
    }
}

/// `NVF_PACE_LOG`'s line, at the hand-off (before the limiter's wait there).
fn pace_log(world: &mut World) {
    let after = world.contains_resource::<WaitsAfterHandOff>();
    let Some(mut log) = world.get_resource_mut::<PaceLog>() else { return };
    let now = Instant::now();
    if let Some(start) = log.frame_start.take() {
        log.to_hand_off += now - start;
        log.frames += 1;
    }
    let since = *log.since.get_or_insert(now);
    if now - since < Duration::from_secs(10) || log.frames == 0 {
        return;
    }
    let frames = f64::from(log.frames);
    let [fsr, fg, fsr_frames, fg_frames] = [0, 1, 2, 3].map(|i| RECORDED[i].swap(0, Ordering::Relaxed));
    let micros = |nanos: u64, n: u64| nanos as f64 / 1e3 / n.max(1) as f64;
    info!(
        "frame pace: {:.1} fps; input to hand-off {:.2} ms a frame; the limiter waited {:.2} ms a frame ({:.2} of it for the presenter to catch up), {} the hand-off; recording FSR {:.0} us, frame generation {:.0} us a frame",
        frames / (now - since).as_secs_f64(),
        log.to_hand_off.as_secs_f64() * 1e3 / frames,
        log.waited.as_secs_f64() * 1e3 / frames,
        log.held.as_secs_f64() * 1e3 / frames,
        if after { "after" } else { "before" },
        micros(fsr, fsr_frames),
        micros(fg, fg_frames),
    );
    *log = PaceLog { since: Some(now), ..default() };
}

/// Apply presentation settings and observe the monitor before telemetry snapshots them.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApplyFramePace;

impl Plugin for FramePacePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0)
            .init_resource::<Refresh>()
            .init_resource::<CapClock>()
            .add_systems(Update, (apply_vsync, watch_refresh, tell_physics).chain().in_set(ApplyFramePace))
            .add_systems(Last, cap_rate);
        if *PACE_LOG {
            app.init_resource::<PaceLog>().add_systems(First, pace_log_frame_start.before(bevy::time::TimeSystems));
        }
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(Render, log_present_modes.in_set(RenderSystems::PrepareViews).before(bevy::render::view::prepare_windows));
        }
    }
}

/// The frames-a-second cap that keeps a screen refreshing at `millihertz` inside its
/// variable-refresh range: a few frames below the refresh rate, more on faster screens
/// (the refresh rate less its square over 3600, at least 3 below: 60 Hz 57, 120 Hz 116,
/// 144 Hz 138, 165 Hz 157, 240 Hz 224), never under 30. `None` for a rate under 40 Hz,
/// which no variable-refresh screen runs at as its top [design: a margin for the limiter's
/// error and a frame that comes in early; the r - r²/3600 rule is one widely used for VRR
/// caps, not a vendor figure; docs/spec-notes/display-sync.md].
pub fn vrr_cap(millihertz: u32) -> Option<u32> {
    let hz = f64::from(millihertz) / 1000.0;
    if hz < 40.0 {
        return None;
    }
    let below = (hz * hz / 3600.0).max(3.0);
    Some(((hz - below).floor() as u32).max(30))
}

/// The cap the limiter keeps: the page's and variable refresh's, the lower when both.
pub fn effective_cap(cap: u32, vrr: u32) -> u32 {
    match (cap, vrr) {
        (0, v) => v,
        (c, 0) => c,
        (c, v) => c.min(v),
    }
}

/// The real frames a second frame pacing's own limiter keeps ([`wait_for_cap`]; 0: none), so
/// that one limiter holds the frames and no two hold them at the same rate:
/// - the page's cap and variable refresh's, the lower, both counting frames shown;
/// - with Vertical sync a page cap at or above the refresh rate is dropped: vsync already holds
///   the frames there, and a limiter at about the same rate beside it only queues frames
///   behind vsync (latency) when the two drift apart;
/// - while FSR frame generation runs (`fsr_fg_cap`, `frame_gen::FrameGenCap`, its cap under
///   half the refresh rate) each real frame is shown twice, so the frames-shown cap is halved
///   for real frames, as Reflex's limit does it for DLSS-G, and the lower of that and FSR's
///   own cap holds.
pub fn own_cap(pace: &FramePace, refresh: &Refresh, fsr_fg_cap: u32) -> u32 {
    let refresh_hz = refresh.millihertz.map(|m| (m + 500) / 1000);
    let page = match (pace.sync, refresh_hz) {
        (DisplaySync::Vsync, Some(hz)) if pace.cap >= hz => 0,
        _ => pace.cap,
    };
    let shown = effective_cap(page, refresh.vrr_cap);
    if fsr_fg_cap == 0 {
        return shown;
    }
    effective_cap(shown / 2, fsr_fg_cap)
}

/// The physics rate's Match FPS ceiling (`nvf_play::phys_rate::display_hz`):
/// the cap the limiter keeps ([`own_cap`]), with vsync and the window's refresh rate. Frame
/// generation's cap is the one its switch set last frame.
fn tell_physics(pace: Res<FramePace>, refresh: Res<Refresh>, fg: Option<Res<crate::frame_gen::FrameGenCap>>, dlssg: Option<Res<crate::frame_gen::DlssgRequest>>, physics: Option<ResMut<nvf_play::phys_rate::DisplayPace>>) {
    let Some(mut physics) = physics else { return };
    let cap = own_cap(&pace, &refresh, fg.map_or(0, |c| c.0));
    let hz = nvf_play::phys_rate::display_hz(cap, pace.presents_vsync(dlssg.is_some_and(|r| r.on)), refresh.millihertz);
    physics.set_if_neq(nvf_play::phys_rate::DisplayPace(hz));
}

/// The window's present mode as the display sync asks ([`FramePace::presents_vsync`]), when
/// that changes. DLSS Frame Generation needs vsync off on Vulkan: with Vertical sync its row's
/// DLSS choice is dimmed (`frame_gen::dlss_unavailable_reason`); with Variable refresh the
/// window presents with vsync off while DLSS-G is asked for. The request is made before this
/// (`frame_gen::request_dlssg`), so the swapchain DLSS-G's load makes in the same frame's
/// extraction already has the mode it needs, and the one made when DLSS-G is unloaded has
/// vsync again.
fn apply_vsync(pace: Res<FramePace>, dlssg: Option<Res<crate::frame_gen::DlssgRequest>>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    let dlssg = dlssg.is_some_and(|r| r.on);
    let vsync = pace.presents_vsync(dlssg);
    let mode = if vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
    for mut w in &mut windows {
        if w.present_mode != mode {
            w.present_mode = mode;
            let why = if pace.sync == DisplaySync::Variable && dlssg { " (variable refresh with DLSS Frame Generation: its frames held under the refresh rate by the cap)" } else { "" };
            info!("frame pace: vsync {}{why}", if vsync { "on" } else { "off" });
        }
    }
}

/// Note the window's monitor's refresh rate and, with variable refresh, the cap it gives;
/// one line each time that cap changes.
fn watch_refresh(pace: Res<FramePace>, dlssg: Option<Res<crate::frame_gen::DlssgRequest>>, windows: Query<(&Window, Option<&OnMonitor>), With<PrimaryWindow>>, monitors: Query<(&Monitor, Has<PrimaryMonitor>)>, mut refresh: ResMut<Refresh>, mut said: Local<Option<(DisplaySync, Option<u32>, bool)>>) {
    let Ok((w, on)) = windows.single() else { return };
    let monitor = on.and_then(|o| monitors.get(o.0).ok()).or_else(|| monitors.iter().find(|m| m.1)).or_else(|| monitors.iter().next()).map(|m| m.0);
    let millihertz = match w.mode {
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(m)) => Some(m.refresh_rate_millihertz),
        _ => monitor.and_then(|m| m.refresh_rate_millihertz),
    }
    .filter(|&m| m > 0);
    let vrr = if pace.sync == DisplaySync::Variable { millihertz.and_then(vrr_cap).unwrap_or(0) } else { 0 };
    let now = Refresh { millihertz, vrr_cap: vrr };
    if *refresh != now {
        *refresh = now;
    }
    let vsync = pace.presents_vsync(dlssg.is_some_and(|r| r.on));
    let key = (pace.sync, millihertz, vsync);
    if *said == Some(key) || (pace.sync == DisplaySync::Variable && millihertz.is_none() && monitors.is_empty()) {
        return;
    }
    *said = Some(key);
    if pace.sync != DisplaySync::Variable {
        return;
    }
    let hz = |m: u32| format!("{:.0}", f64::from(m) / 1000.0);
    match (millihertz, vrr) {
        (Some(m), c) if c > 0 => info!("frame pace: variable refresh, at most {c} fps (the monitor's {} Hz), vsync {}", hz(m), if vsync { "on" } else { "off for DLSS Frame Generation, the cap through Reflex" }),
        (Some(m), _) => warn!("frame pace: variable refresh, the monitor's {} Hz is too slow to cap under; {}", hz(m), if vsync { "vsync only" } else { "vsync off for DLSS Frame Generation, no cap" }),
        (None, _) => warn!("frame pace: variable refresh, the monitor's refresh rate is not known; {}, no cap", if vsync { "vsync only" } else { "vsync off for DLSS Frame Generation" }),
    }
}

/// How much of a frame's time a frame may come early while the limiter makes up for a late
/// one [design: under the margins the caps keep below the screen's rate, the least of them
/// frame generation's real-frame cap at 144 Hz, 71 of 72].
const REPAY: f64 = 0.01;

/// When the next capped frame may go on, and the clock after it. `clock` is where the
/// schedule stands (frames a frame's time apart) and when the last frame went on. On time,
/// the frame goes on at the schedule's next step. Late, it goes on at once, but no sooner
/// than a frame's time less [`REPAY`] after the last one, and the schedule is kept, so the
/// frames after it catch up that much each; a schedule more than a frame's time behind is
/// moved up to a frame's time behind.
fn next_frame(clock: Option<(Instant, Instant)>, cap: u32, now: Instant) -> (Instant, (Instant, Instant)) {
    let Some((schedule, _)) = clock else { return (now, (now, now)) };
    let period = Duration::from_secs_f64(1.0 / f64::from(cap.max(1)));
    let due = schedule + period;
    let start = earliest(clock, cap).map_or(now, |earliest| earliest.max(now));
    let schedule = if start.duration_since(due) > period { start - period } else { due };
    (start, (schedule, start))
}

/// The earliest the next capped frame may go on, whenever it is done: the schedule's next
/// step, or a frame's time less [`REPAY`] after the last frame if that is later. `None`: the
/// clock has not started.
fn earliest(clock: Option<(Instant, Instant)>, cap: u32) -> Option<Instant> {
    let (schedule, last) = clock?;
    let period = Duration::from_secs_f64(1.0 / f64::from(cap.max(1)));
    Some((schedule + period).max(last + period.mul_f64(1.0 - REPAY)))
}

/// When a frame held back `hold` for frame generation's presenter goes on, and what the
/// hold adds to its wait. The hold counts from `earliest`, the time the frame could go on at
/// the soonest ([`earliest`]): a frame that is late by more than the hold has given the
/// presenter that time already and is not held at all, and one late by less waits only the
/// rest.
fn held_back(earliest: Instant, now: Instant, hold: Duration) -> (Instant, Duration) {
    let start = (earliest + hold).max(now);
    (start, start.saturating_duration_since(earliest.max(now)))
}

/// Reflex's frame limit for a cap of `cap` frames shown a second: microseconds per frame shown,
/// 0 for no cap. While DLSS-G generates, Reflex's limit holds the frames shown, generated ones
/// included, not the real frames: 13158 us (76) held a 2x run at 76 shown and 38 real
/// [observed: a run on an RTX 5090 at 160 Hz, 2026-11-23, 355 real frames in 10 s, the overlay at 76 (38)];
/// so the cap goes to it whole, never divided by the frames DLSS-G presents per real frame.
pub fn reflex_frame_limit_us(cap: u32) -> u32 {
    if cap == 0 {
        return 0;
    }
    (1_000_000.0 / f64::from(cap)).round() as u32
}

/// Hold the frame until its time at the cap ([`next_frame`]). While DLSS Frame Generation runs
/// (the spike's `on`, #2317, Stage 3; `dlss::frame_generation_running`) the cap is Reflex's
/// instead (`ReflexOptions::frameLimitUs`, the limiter NVIDIA documents for frame generation
/// [documented: `ProgrammingGuideDLSS_G.md` §6.3, §12.1]): the page's or variable refresh's cap
/// on frames shown, handed to Reflex whole ([`reflex_frame_limit_us`],
/// `dlss::set_reflex_frame_limit`), and nothing waits here, so the driver and DLSS-G's pacer see
/// one limiter [design: fork-notes `fg-facts.md` §3 item 5]. Reflex holds the frames shown, so
/// the real frames follow what DLSS-G presents by themselves: the cap divided by it while it
/// generates, the whole cap in menus (DLSS-G's mode off) and while it presents one frame per real
/// frame (the window not in front). Until 2026-11-23 the cap was divided here as well (#2731's
/// divisor), which held the real frames at half the intended rate at 2x.
fn wait_for_cap(world: &mut World) {
    let Some(pace) = world.get_resource::<FramePace>() else { return };
    if crate::dlss::frame_generation_running() {
        let cap = effective_cap(crate::mobile_performance::runtime_cap(world, pace.cap), world.get_resource::<Refresh>().map_or(0, |r| r.vrr_cap));
        crate::dlss::set_reflex_frame_limit(reflex_frame_limit_us(cap));
        if let Some(mut clock) = world.get_resource_mut::<CapClock>() {
            clock.0 = None;
        }
        return;
    }
    if crate::dlss::frame_generation_on() {
        // Frame generation ended for the session: the limit is frame pacing's again.
        crate::dlss::set_reflex_frame_limit(0);
    }
    // FSR frame generation shows each real frame twice: at most half the refresh rate of them.
    // The thermal policy's cap (`mobile_performance::runtime_cap`) stands in for the page's.
    let runtime = FramePace { cap: crate::mobile_performance::runtime_cap(world, pace.cap), ..*pace };
    let cap = own_cap(&runtime, world.get_resource::<Refresh>().unwrap_or(&Refresh::default()), world.get_resource::<crate::frame_gen::FrameGenCap>().map_or(0, |c| c.0));
    let lag = Duration::from_nanos(PRESENTER_LAG_NANOS.swap(0, Ordering::Relaxed));
    let Some(mut clock) = world.get_resource_mut::<CapClock>() else { return };
    if cap == 0 {
        clock.0 = None;
        return;
    }
    let now = Instant::now();
    let (mut start, mut next) = next_frame(clock.0, cap, now);
    // Frame generation's presenter is behind: this frame comes that much later, and the
    // schedule goes on from there.
    let mut held = if *LAG_FEEDBACK { catch_up(lag) } else { Duration::ZERO };
    if !held.is_zero() {
        // What it really adds: a frame already later than that is not held at all.
        (start, held) = held_back(earliest(clock.0, cap).unwrap_or(now), now, held);
    }
    if !held.is_zero() {
        next = (start, start);
        HELD_FOR_PRESENTER_NANOS.fetch_add(held.as_nanos() as u64, Ordering::Relaxed);
    }
    clock.0 = Some(next);
    let Some(left) = start.checked_duration_since(now).filter(|d| !d.is_zero()) else { return };
    if let Some(sleep) = left.checked_sub(Duration::from_millis(1)) {
        std::thread::sleep(sleep);
    }
    while Instant::now() < start {
        std::hint::spin_loop();
    }
    if let Some(mut log) = world.get_resource_mut::<PaceLog>() {
        log.waited += left;
        log.held += held;
    }
}

/// The limiter in `Last`, where the frame is not handed to a render thread (no pipelined
/// rendering): with one it waits after the hand-off instead ([`after_hand_off`]).
pub(crate) fn cap_rate(world: &mut World) {
    if !world.contains_resource::<WaitsAfterHandOff>() {
        wait_for_cap(world);
    }
}

/// The present mode Bevy takes for vsync on a surface offering `modes`: FIFO relaxed when
/// there, else FIFO (bevy_render 0.20 `view::window::present_mode`, its AutoVsync
/// fallbacks) [documented: wgpu-types 30 `PresentMode::AutoVsync`].
fn vsync_mode(modes: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    if modes.contains(&wgpu::PresentMode::FifoRelaxed) {
        wgpu::PresentMode::FifoRelaxed
    } else {
        wgpu::PresentMode::Fifo
    }
}

/// Log once the present modes the primary window's surface supports: a surface of its
/// own on the window, asked and dropped (Bevy's own is private to it).
fn log_present_modes(windows: Query<(&ExtractedWindow, &RawHandleWrapper)>, instance: Res<RenderInstance>, adapter: Res<RenderAdapter>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Some((_, handle)) = windows.iter().find(|(w, _)| w.physical_width > 0) else { return };
    *done = true;
    let target = wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(handle.get_display_handle()), raw_window_handle: handle.get_window_handle() };
    // SAFETY: the window's handles are valid while it has a RawHandleWrapper, as Bevy's
    // create_surfaces relies on; the surface is dropped before this returns.
    let surface = match unsafe { instance.create_surface_unsafe(target) } {
        Ok(s) => s,
        Err(e) => {
            warn!("frame pace: the window's present modes not read: {e}");
            return;
        }
    };
    let caps = surface.get_capabilities(&adapter);
    drop(surface);
    let backend = adapter.get_info().backend;
    info!("frame pace: the surface ({backend:?}) supports present modes {:?}; vsync presents as {:?}", caps.present_modes, vsync_mode(&caps.present_modes));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_waits_out_what_is_left_of_its_time_at_the_cap() {
        let t = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        // The first capped frame starts the clock.
        assert_eq!(next_frame(None, 50, t), (t, (t, t)));
        // On time: at the schedule's next step, 20 ms on.
        assert_eq!(next_frame(Some((t, t)), 50, t + ms(5)), (t + ms(20), (t + ms(20), t + ms(20))));
        assert_eq!(next_frame(Some((t, t)), 50, t + ms(20)).0, t + ms(20), "just in time: no wait");
    }

    #[test]
    fn a_late_frame_is_made_up_for_a_little_at_a_time() {
        let t = Instant::now();
        let us = |n: u64| Duration::from_micros(n);
        // 3 ms late: it goes on at once and the schedule stays where it was.
        let (start, clock) = next_frame(Some((t, t)), 50, t + us(23_000));
        assert_eq!((start, clock), (t + us(23_000), (t + us(20_000), t + us(23_000))));
        // The next frames, done early, each come 1% of a frame's time (0.2 ms) early...
        let (start, clock) = next_frame(Some(clock), 50, start + us(5_000));
        assert_eq!(start, t + us(23_000 + 19_800));
        assert_eq!(clock.0, t + us(40_000));
        // ...until the schedule is met again: 3 ms in 15 frames.
        let mut state = (start, clock);
        for _ in 0..14 {
            state = next_frame(Some(state.1), 50, state.0 + us(5_000));
        }
        assert_eq!(state.0, state.1 .0, "back on the schedule");
        assert_eq!(state.0, t + us(20_000 * 16));
        // More than a frame's time behind: the rest is forgiven.
        let (start, clock) = next_frame(Some((t, t)), 50, t + us(90_000));
        assert_eq!((start, clock.0), (t + us(90_000), t + us(70_000)));
    }

    #[test]
    fn the_next_frame_is_held_back_for_a_presenter_that_is_behind() {
        let ms = |n: f64| Duration::from_secs_f64(n / 1000.0);
        assert_eq!(catch_up(ms(0.0)), Duration::ZERO);
        assert_eq!(catch_up(ms(1.0)), Duration::ZERO, "the presenter's own polling and pacing");
        assert_eq!(catch_up(ms(3.0)), ms(1.0));
        assert_eq!(catch_up(ms(12.0)), ms(1.5), "at most a millisecond and a half a frame");
    }

    #[test]
    fn a_late_frame_is_held_back_only_by_what_its_lateness_did_not_give_the_presenter() {
        let t = Instant::now();
        let us = |n: u64| Duration::from_micros(n);
        // Done early: the whole hold comes after the frame's time at the cap.
        assert_eq!(held_back(t + us(20_000), t + us(5_000), us(1_500)), (t + us(21_500), us(1_500)));
        // Half a millisecond late: the rest of the hold.
        assert_eq!(held_back(t + us(20_000), t + us(20_500), us(1_500)), (t + us(21_500), us(1_000)));
        // Later than the hold: it goes on at once.
        assert_eq!(held_back(t + us(20_000), t + us(23_000), us(1_500)), (t + us(23_000), Duration::ZERO));
        // The earliest a frame may go on is the schedule's next step, or after a late frame
        // 1% of a frame's time less than a frame's time after it.
        assert_eq!(earliest(None, 50), None);
        assert_eq!(earliest(Some((t, t)), 50), Some(t + us(20_000)));
        assert_eq!(earliest(Some((t, t + us(3_000))), 50), Some(t + us(3_000 + 19_800)));
    }

    #[test]
    fn variable_refresh_caps_a_few_frames_under_the_refresh_rate() {
        assert_eq!(vrr_cap(60_000), Some(57));
        assert_eq!(vrr_cap(59_940), Some(56), "59.94 Hz");
        assert_eq!(vrr_cap(75_000), Some(72));
        assert_eq!(vrr_cap(120_000), Some(116));
        assert_eq!(vrr_cap(143_998), Some(138), "a 144 Hz mode as Windows reports it");
        assert_eq!(vrr_cap(144_000), Some(138));
        assert_eq!(vrr_cap(165_000), Some(157));
        assert_eq!(vrr_cap(240_000), Some(224));
        assert_eq!(vrr_cap(360_000), Some(324));
        assert_eq!(vrr_cap(30_000), None, "too slow for variable refresh");
        for hz in 40..=500 {
            let c = vrr_cap(hz * 1000).unwrap();
            assert!(c + 3 <= hz && c >= 30, "{hz} Hz: {c}");
        }
    }

    #[test]
    fn variable_refresh_presents_with_vsync_off_only_for_dlss_frame_generation() {
        let pace = |sync| FramePace { sync, cap: 0 };
        for dlssg in [false, true] {
            assert!(!pace(DisplaySync::Off).presents_vsync(dlssg));
            assert!(pace(DisplaySync::Vsync).presents_vsync(dlssg), "Vertical sync dims DLSS-G rather than giving way");
        }
        assert!(pace(DisplaySync::Variable).presents_vsync(false));
        assert!(!pace(DisplaySync::Variable).presents_vsync(true));
    }

    /// The window's present mode follows DLSS-G's request with Variable refresh, both ways, and
    /// the display sync alone otherwise.
    #[test]
    fn the_window_follows_the_dlssg_request_with_variable_refresh() {
        let mut app = App::new();
        app.insert_resource(FramePace { sync: DisplaySync::Variable, cap: 0 })
            .insert_resource(crate::frame_gen::DlssgRequest { on: false, frames: 1 })
            .add_systems(Update, apply_vsync);
        let window = app.world_mut().spawn((Window::default(), PrimaryWindow)).id();
        let mode = |app: &App| app.world().get::<Window>(window).unwrap().present_mode;
        app.update();
        assert_eq!(mode(&app), PresentMode::AutoVsync);
        app.world_mut().resource_mut::<crate::frame_gen::DlssgRequest>().on = true;
        app.update();
        assert_eq!(mode(&app), PresentMode::AutoNoVsync, "DLSS-G asked for: vsync off");
        app.world_mut().resource_mut::<FramePace>().sync = DisplaySync::Vsync;
        app.update();
        assert_eq!(mode(&app), PresentMode::AutoVsync, "Vertical sync keeps vsync");
        app.world_mut().resource_mut::<FramePace>().sync = DisplaySync::Variable;
        app.update();
        assert_eq!(mode(&app), PresentMode::AutoNoVsync);
        app.world_mut().resource_mut::<crate::frame_gen::DlssgRequest>().on = false;
        app.update();
        assert_eq!(mode(&app), PresentMode::AutoVsync, "DLSS-G no longer asked for: vsync again");
    }

    #[test]
    fn the_lower_of_the_pages_cap_and_variable_refreshs_holds() {
        assert_eq!(effective_cap(0, 0), 0);
        assert_eq!(effective_cap(60, 0), 60);
        assert_eq!(effective_cap(0, 138), 138);
        assert_eq!(effective_cap(60, 138), 60);
        assert_eq!(effective_cap(144, 138), 138);
    }

    #[test]
    fn one_limiter_holds_the_frames() {
        let pace = |sync, cap| FramePace { sync, cap };
        let at = |hz: u32, vrr: u32| Refresh { millihertz: Some(hz), vrr_cap: vrr };
        // Vertical sync: a cap at or above the refresh rate is vsync's to keep.
        assert_eq!(own_cap(&pace(DisplaySync::Vsync, 144), &at(143_998, 0), 0), 0, "144 on a 144 Hz screen: vsync alone");
        assert_eq!(own_cap(&pace(DisplaySync::Vsync, 60), &at(59_940, 0), 0), 0, "60 on 59.94 Hz: vsync alone");
        assert_eq!(own_cap(&pace(DisplaySync::Vsync, 144), &at(160_000, 0), 0), 144, "under the refresh rate the cap holds");
        assert_eq!(own_cap(&pace(DisplaySync::Vsync, 144), &Refresh::default(), 0), 144, "refresh rate not known: the cap holds");
        // Off and variable refresh keep the cap; variable refresh's own is the lower.
        assert_eq!(own_cap(&pace(DisplaySync::Off, 144), &at(144_000, 0), 0), 144);
        assert_eq!(own_cap(&pace(DisplaySync::Variable, 0), &at(160_000, 152), 0), 152);
        assert_eq!(own_cap(&pace(DisplaySync::Variable, 144), &at(160_000, 152), 0), 144);
        // FSR frame generation: the cap counts frames shown, two for each real frame, and its
        // own cap under half the refresh rate holds when lower.
        assert_eq!(own_cap(&pace(DisplaySync::Vsync, 0), &at(160_000, 0), 78), 78);
        assert_eq!(own_cap(&pace(DisplaySync::Vsync, 120), &at(160_000, 0), 78), 60, "120 shown: 60 real");
        assert_eq!(own_cap(&pace(DisplaySync::Variable, 0), &at(160_000, 152), 78), 76, "152 shown at most: 76 real");
        assert_eq!(own_cap(&pace(DisplaySync::Vsync, 144), &at(144_000, 0), 71), 71, "144 on 144 Hz is vsync's: FSR's own cap");
    }

    #[test]
    fn reflex_gets_the_whole_cap_on_frames_shown() {
        assert_eq!(reflex_frame_limit_us(0), 0, "no cap");
        assert_eq!(reflex_frame_limit_us(152), 6_579, "variable refresh at 160 Hz: 152 shown, whatever DLSS-G generates");
        assert_eq!(reflex_frame_limit_us(144), 6_944);
        assert_eq!(reflex_frame_limit_us(60), 16_667);
    }

    #[test]
    fn display_sync_reads_its_own_names_and_the_old_vsync_switch() {
        for d in DisplaySync::ALL {
            assert_eq!(DisplaySync::parse(d.stored()), Some(d));
            assert_eq!(DisplaySync::parse(d.name()), Some(d));
        }
        assert_eq!(DisplaySync::parse(" 1 "), Some(DisplaySync::Vsync), "vsync=1 as written before");
        assert_eq!(DisplaySync::parse("0"), Some(DisplaySync::Off));
        assert_eq!(DisplaySync::parse("On"), Some(DisplaySync::Vsync));
        assert_eq!(DisplaySync::parse("off"), Some(DisplaySync::Off));
        assert_eq!(DisplaySync::parse("VARIABLE REFRESH"), Some(DisplaySync::Variable));
        assert_eq!(DisplaySync::parse("maybe"), None);
    }

    #[test]
    fn vsync_presents_fifo_relaxed_where_there_else_fifo() {
        use wgpu::PresentMode as P;
        assert_eq!(vsync_mode(&[P::Fifo, P::FifoRelaxed, P::Mailbox, P::Immediate]), P::FifoRelaxed);
        assert_eq!(vsync_mode(&[P::Fifo, P::Immediate]), P::Fifo);
    }
}
