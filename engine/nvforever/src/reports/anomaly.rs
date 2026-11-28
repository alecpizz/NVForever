//! Anomaly detectors and the flight recorder (player-reports.md §4.2, §4.4).
//!
//! **Detectors.** Problems that do not crash the game, each sent as an event of the session
//! summary (`events`) with where it happened (place, cell, position, game hour and state),
//! at most [`MAX_PER_KIND`] of a kind a session. Many are already noticed by the engine,
//! which logs them; those are read from the log lines (`classify`) by the reports' log
//! layer (warnings.rs), so nothing in the game code changes:
//!
//! | kind | when |
//! |---|---|
//! | `fell_out_of_world` | the engine put the player back after falling through the land or out of an interior (nvf-play player.rs) |
//! | `object_fell_through` | the engine caught a dropped item (nvf-play phys.rs) or a corpse (ragdoll.rs) that sank through the land (#2753) |
//! | `gpu_error` | a GPU error stopped the game: out of GPU memory, a lost device (crash.rs `render_error`) |
//! | `path_failure` | an actor cannot reach the place of its package (residents.rs) |
//! | `script_loop` | one script has errored [`SCRIPT_ERRORS`] times |
//! | `missing_asset` | a model, texture, actor file or particles failed to load (nvf-assets guard.rs); a model or actor file a plugin's record names says the `plugin`, and one the base game's own data names and no archive has is not reported (asset_sources.rs, #3045) |
//! | `save_trouble` | a save failed to write, or a load fell back to the main menu |
//! | `audio` | no audio output device; a spoken line without its voice file (not a line with no words, such as `...`) |
//! | `hitch` | a frame over [`HITCH_MS`] outside a load and the [`HITCH_SETTLE_SECS`] after one, and not with the game away (the window unfocused, covered or minimised, the app suspended, the second after each, or a frame of 30 s or more: `mod.rs` `away_frame`, #3043) |
//! | `slow_load` | a loading screen up longer than [`SLOW_LOAD_MS`] |
//! | `player_stuck` | movement held [`STUCK_SECS`] with under [`STUCK_UNITS`] moved, while playing |
//! | `controls_locked` | [design, #2403] an unchanged dialogue, menu or script control lock for [`CONTROLS_LOCKED_SECS`] with [`CONTROLS_LOCKED_PRESSES`] recent press edges |
//! | `memory` | RAM or VRAM in use over [`MEMORY_SHARE`] of the budget (memory_budget.rs), once a game is loaded (not in the `viewer` state at the start, when the RAM in use is other programs') |
//! | `softlock` | the player is held and nothing in the game moves on to let them go: controls off or restrained with no story event, a conversation with no line and no choice, a line that never ends, a menu that never opens (nvf-play softlock.rs, `fnv_game::softlock`) |
//!
//! [design / file refs] Actor travel progress and main-quest stall detection are excluded;
//! see player-reports.md §4.4 for the missing AI progress contract and quest selection.
//!
//! **The flight recorder.** Every log event that reaches the log layer (info and up, and
//! what `[Log]` turns up), with the time and where the camera was, kept for the last
//! [`RECORDER_SECS`] in memory. A panic writes it beside the crash report
//! (`recorder-<time>-<pid>.jsonl`, crash.rs), and it is sent with that crash.

use super::summary::Summary;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub const MAX_PER_KIND: u32 = 3;
pub const HITCH_MS: f32 = 250.0;
/// Seconds after a loading screen when hitches are not counted: the world is still streaming in.
pub const HITCH_SETTLE_SECS: f64 = 10.0;
pub const SLOW_LOAD_MS: f32 = 20_000.0;
pub const STUCK_SECS: f64 = 5.0;
pub const STUCK_UNITS: f32 = 16.0;
pub const MEMORY_SHARE: f64 = 0.9;
pub const SCRIPT_ERRORS: u32 = 100;
pub const RECORDER_SECS: f64 = 60.0;
const RECORDER_MAX_BYTES: usize = 8 * 1024 * 1024;

/// [design, #2403] Real seconds without screen/control progress, also the input window.
pub const CONTROLS_LOCKED_SECS: f64 = 60.0;
/// [design, #2403] Press edges required in that window; holding a button is one press.
pub const CONTROLS_LOCKED_PRESSES: usize = 5;
/// [design, #2403] Payload limits, independent of the world's number of quests/plugins.
pub const CONTROLS_LOCKED_QUESTS: usize = 10;
pub const CONTROLS_LOCKED_SOURCES: usize = 10;
pub const CONTROLS_LOCKED_TOPICS: usize = 10;
pub const CONTROLS_LOCKED_LABEL_CHARS: usize = 128;

/// [design, #2403] A screen fingerprint and effective groups; any change rearms the watch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockProgress {
    pub screen: u64,
    pub groups: [bool; 7],
}

/// [design, #2403] Bounded input history; only the newest required press edges are needed.
#[derive(Default)]
pub struct ControlsLocked {
    progress: Option<LockProgress>,
    since: f64,
    presses: VecDeque<f64>,
    fired: bool,
}

impl ControlsLocked {
    /// [design, #2403] `None` releases the lock (including loading). Returns its duration
    /// once, after enough recent presses and enough time without progress.
    pub fn step(&mut self, now: f64, progress: Option<LockProgress>, presses: usize) -> Option<f64> {
        if self.progress != progress {
            self.progress = progress;
            self.since = now;
            self.presses.clear();
            self.fired = false;
        }
        progress?;
        while self.presses.front().is_some_and(|t| now - t > CONTROLS_LOCKED_SECS) {
            self.presses.pop_front();
        }
        for _ in 0..presses.min(CONTROLS_LOCKED_PRESSES) {
            if self.presses.len() == CONTROLS_LOCKED_PRESSES {
                self.presses.pop_front();
            }
            self.presses.push_back(now);
        }
        let held = now - self.since;
        if self.fired || held < CONTROLS_LOCKED_SECS || self.presses.len() < CONTROLS_LOCKED_PRESSES {
            return None;
        }
        self.fired = true;
        Some(held)
    }
}

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

#[derive(Default)]
struct Recorder {
    lines: VecDeque<(f64, String)>,
    bytes: usize,
}

static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);
/// Anomalies read from log lines, waiting for the next frame to add where they happened.
static FROM_LOG: Mutex<Vec<(&'static str, Value)>> = Mutex::new(Vec::new());
static SCRIPT_ERROR_COUNTS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);

/// Whose record names each model file (asset_sources.rs), once its thread has listed them.
static MODEL_SOURCES: std::sync::OnceLock<super::asset_sources::Sources> = std::sync::OnceLock::new();
static MODEL_SOURCES_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// How long a `missing_asset` event waits for that list before it goes out without a plugin.
pub const SOURCES_WAIT_SECS: u64 = 20;

/// List the load order's model files and their plugins, once a session, on a thread of
/// its own (it reads every base record; nothing waits for it but the events it names).
pub fn list_model_sources(lo: &'static fnv_esm::LoadOrder<'static>) {
    if MODEL_SOURCES_ASKED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let spawned = std::thread::Builder::new().name("nvf-report-sources".into()).spawn(move || {
        let t = Instant::now();
        let sources = super::asset_sources::Sources::of(lo);
        bevy::log::info!("reports: {} model files named by the load order's records, listed in {:.0} ms", sources.len(), t.elapsed().as_secs_f64() * 1000.0);
        let _ = MODEL_SOURCES.set(sources);
    });
    if spawned.is_err() {
        // No list: events go out without a plugin, none is held back.
        let _ = MODEL_SOURCES.set(Default::default());
    }
}

/// What the model sources say of an event found in the log.
#[derive(Debug, PartialEq)]
pub enum Sourced {
    /// Report it as it is.
    Plain,
    /// Report it with the plugin whose record names the file.
    Plugin(String),
    /// The base game's own data names a file no archive has: no fault to report.
    GamesOwn(String),
    /// The list is not made yet: ask again next frame.
    Wait,
}

/// Judge one event: only a `missing_asset` for a model or an actor file is looked up.
/// `sources`: the list when it is made; `listing`: it was asked for; `waited`: seconds
/// this event has waited for it.
pub fn sourced(kind: &str, detail: &Value, sources: Option<&super::asset_sources::Sources>, listing: bool, waited: u64) -> Sourced {
    if kind != "missing_asset" || !matches!(detail["what"].as_str(), Some("model" | "actor file")) {
        return Sourced::Plain;
    }
    let Some(file) = detail["file"].as_str() else { return Sourced::Plain };
    let Some(sources) = sources else {
        return if listing && waited < SOURCES_WAIT_SECS { Sourced::Wait } else { Sourced::Plain };
    };
    // "<path>: <why>" (nvf-assets guard.rs).
    let (path, why) = file.split_once(": ").unwrap_or((file, ""));
    match sources.plugin_of(path) {
        Some(plugin) if super::asset_sources::official(plugin) && why == "not in any archive" => Sourced::GamesOwn(plugin.to_string()),
        Some(plugin) => Sourced::Plugin(plugin.to_string()),
        None => Sourced::Plain,
    }
}

/// Every log event the layer sees: into the recorder, and classified.
pub fn on_log(level: &str, target: &str, msg: &str) {
    let t = start().elapsed().as_secs_f64();
    let line = json!({ "t_ms": (t * 1000.0) as u64, "level": level, "target": target, "msg": msg, "where": crate::crash::place_text() }).to_string();
    {
        let mut r = RECORDER.lock().unwrap_or_else(|e| e.into_inner());
        let r = r.get_or_insert_with(Recorder::default);
        r.bytes += line.len();
        r.lines.push_back((t, line));
        while r.lines.front().is_some_and(|(t0, _)| t - t0 > RECORDER_SECS) || r.bytes > RECORDER_MAX_BYTES {
            let Some((_, l)) = r.lines.pop_front() else { break };
            r.bytes -= l.len();
        }
    }
    let found = match classify(level, msg) {
        Some(Found::Now(kind, detail)) => Some((kind, detail)),
        Some(Found::ScriptError(unit)) => {
            let mut m = SCRIPT_ERROR_COUNTS.lock().unwrap_or_else(|e| e.into_inner());
            let n = m.get_or_insert_with(HashMap::new).entry(unit.clone()).or_default();
            *n += 1;
            (*n == SCRIPT_ERRORS).then(|| ("script_loop", json!({ "script": unit, "errors": SCRIPT_ERRORS })))
        }
        None => None,
    };
    if let Some(f) = found {
        FROM_LOG.lock().unwrap_or_else(|e| e.into_inner()).push(f);
    }
}

#[derive(Debug, PartialEq)]
pub enum Found {
    Now(&'static str, Value),
    ScriptError(String),
}

/// What a log line says went wrong, if it is one the detectors know (module table).
pub fn classify(level: &str, msg: &str) -> Option<Found> {
    let warn = level == "WARN" || level == "ERROR";
    let cut = |s: &str| s.chars().take(300).collect::<String>();
    if warn && msg.starts_with("player fell through at") {
        return Some(Found::Now("fell_out_of_world", json!({ "space": "exterior", "line": cut(msg) })));
    }
    if warn && msg.starts_with("player fell out of interior") {
        return Some(Found::Now("fell_out_of_world", json!({ "space": "interior", "line": cut(msg) })));
    }
    if warn && msg.starts_with("dropped 0x") && msg.contains(" fell through the world") {
        return Some(Found::Now("object_fell_through", json!({ "what": "dropped item", "line": cut(msg) })));
    }
    if warn && msg.starts_with("ragdoll: actor ") && msg.contains(" fell through at ") {
        return Some(Found::Now("object_fell_through", json!({ "what": "corpse", "line": cut(msg) })));
    }
    if let Some(rest) = msg.strip_prefix("render: ").filter(|_| level == "ERROR") {
        if let Some((ty, _)) = rest.split_once(" GPU error, the game stops") {
            return Some(Found::Now("gpu_error", json!({ "error": ty, "line": cut(msg) })));
        }
    }
    if let Some((what, rest)) = msg.strip_prefix("softlock: ").filter(|_| warn).and_then(|r| r.split_once(' ')) {
        return Some(Found::Now("softlock", json!({ "what": what, "line": cut(rest) })));
    }
    if msg.contains(": cannot reach the place of ") {
        return Some(Found::Now("path_failure", json!({ "line": cut(msg) })));
    }
    if warn {
        if let Some(rest) = msg.strip_prefix("script error in ") {
            let unit = rest.split(':').next().unwrap_or(rest).trim().to_string();
            return Some(Found::ScriptError(unit));
        }
        if let Some((what, file)) = msg.split_once(" not loaded: ").filter(|(w, _)| matches!(*w, "model" | "texture" | "actor file" | "particles")) {
            return Some(Found::Now("missing_asset", json!({ "what": what, "file": cut(file) })));
        }
        if let Some(e) = msg.strip_prefix("save: failed: ") {
            return Some(Found::Now("save_trouble", json!({ "what": "save failed", "error": cut(e) })));
        }
        if msg.starts_with("load: ") && msg.ends_with("starting again on the main menu") {
            return Some(Found::Now("save_trouble", json!({ "what": "load fell back to the main menu", "line": cut(msg) })));
        }
        if msg.starts_with("audio output: no device") {
            return Some(Found::Now("audio", json!({ "what": "no audio output device" })));
        }
    }
    None
}

/// The recorder's lines, oldest first.
pub fn recorder_lines() -> Vec<String> {
    RECORDER.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|r| r.lines.iter().map(|(_, l)| l.clone()).collect()).unwrap_or_default()
}

/// Write the recorder to `path` (a panic, crash.rs). Redacted like everything sent.
pub fn dump_recorder(path: &Path) -> std::io::Result<()> {
    let text: String = recorder_lines().into_iter().map(|l| super::redact(&l) + "\n").collect();
    std::fs::write(path, text)
}

/// Where and when, for an event (set by the collector each frame).
#[derive(Debug, Clone, Default)]
pub struct Here {
    pub place: String,
    pub cell: Option<String>,
    pub pos: [f32; 3],
    pub hour: Option<f32>,
    pub state: &'static str,
}

/// The detectors' state, kept in the collector.
#[derive(Default)]
pub struct Detectors {
    counts: HashMap<&'static str, u32>,
    stuck_from: Option<(f64, [f32; 3])>,
    memory_high: bool,
    voice_missing: Option<u64>,
    pub controls_locked: ControlsLocked,
    /// `missing_asset` events waiting for the model sources ([`sourced`]): since when (s).
    waiting: Vec<(u64, &'static str, Value)>,
}

impl Detectors {
    /// Add an event of `kind` (at most [`MAX_PER_KIND`] a session). Returns whether it was kept.
    pub fn fire(&mut self, s: &mut Summary, here: &Here, t_s: u64, kind: &'static str, detail: Value) -> bool {
        let n = self.counts.entry(kind).or_default();
        if *n >= MAX_PER_KIND || s.events.len() >= 500 {
            return false;
        }
        *n += 1;
        let mut e = json!({
            "kind": kind, "t_s": t_s, "place": here.place, "cell": here.cell,
            "pos": [here.pos[0].round(), here.pos[1].round(), here.pos[2].round()],
            "hour": here.hour.map(|h| (h * 10.0).round() / 10.0), "state": here.state,
        });
        if let (Some(o), Value::Object(d)) = (e.as_object_mut(), detail) {
            o.extend(d);
        }
        bevy::log::info!("reports: anomaly {kind} at {}{}", here.place, here.cell.as_deref().map(|c| format!(" ({c})")).unwrap_or_default());
        s.events.push(e);
        true
    }

    /// The anomalies the log lines found since the last frame.
    pub fn drain_log(&mut self, s: &mut Summary, here: &Here, t_s: u64) {
        let found = std::mem::take(&mut *FROM_LOG.lock().unwrap_or_else(|e| e.into_inner()));
        let waiting = std::mem::take(&mut self.waiting);
        let listing = MODEL_SOURCES_ASKED.load(std::sync::atomic::Ordering::Relaxed);
        for (since, kind, mut detail) in waiting.into_iter().chain(found.into_iter().map(|(kind, detail)| (t_s, kind, detail))) {
            match sourced(kind, &detail, MODEL_SOURCES.get(), listing, t_s.saturating_sub(since)) {
                Sourced::Wait => self.waiting.push((since, kind, detail)),
                Sourced::GamesOwn(plugin) => bevy::log::debug!("reports: {} is named by {plugin} and in no archive: the game's own, not reported", detail["file"]),
                Sourced::Plugin(plugin) => {
                    detail["plugin"] = json!(plugin);
                    self.fire(s, here, t_s, kind, detail);
                }
                Sourced::Plain => {
                    self.fire(s, here, t_s, kind, detail);
                }
            }
        }
    }

    /// Movement held with the player not moving: `moving` this frame, at `now` (real s).
    pub fn check_stuck(&mut self, s: &mut Summary, here: &Here, t_s: u64, now: f64, moving: bool) {
        if !moving {
            self.stuck_from = None;
            return;
        }
        match self.stuck_from {
            None => self.stuck_from = Some((now, here.pos)),
            Some((t0, p0)) => {
                let d = ((here.pos[0] - p0[0]).powi(2) + (here.pos[1] - p0[1]).powi(2) + (here.pos[2] - p0[2]).powi(2)).sqrt();
                if d >= STUCK_UNITS {
                    self.stuck_from = Some((now, here.pos));
                } else if now - t0 >= STUCK_SECS {
                    self.fire(s, here, t_s, "player_stuck", json!({ "held_s": (now - t0).round(), "moved": d.round() }));
                    self.stuck_from = None;
                }
            }
        }
    }

    /// RAM or VRAM share of the budget; fires on crossing [`MEMORY_SHARE`] upward.
    pub fn check_memory(&mut self, s: &mut Summary, here: &Here, t_s: u64, vram: Option<(u64, u64)>, ram_share: Option<f64>) {
        let vram_share = vram.filter(|(_, b)| *b > 0).map(|(u, b)| u as f64 / b as f64);
        // Before a game is loaded the RAM in use is other programs' (38 of 85 events from
        // players in 2026-11-12..15 fired there, #2758): not counted.
        if here.state == "viewer" {
            return;
        }
        let high = vram_share.is_some_and(|v| v > MEMORY_SHARE) || ram_share.is_some_and(|r| r > MEMORY_SHARE);
        if high && !self.memory_high {
            self.fire(s, here, t_s, "memory", json!({ "vram_share": vram_share.map(|v| (v * 100.0).round() / 100.0), "ram_share": ram_share.map(|v| (v * 100.0).round() / 100.0) }));
        }
        self.memory_high = high;
    }

    /// Spoken lines without their file, counted by the voice player.
    pub fn check_voice(&mut self, s: &mut Summary, here: &Here, t_s: u64, missing: u64) {
        let before = self.voice_missing.replace(missing);
        if before.is_some_and(|b| missing > b) {
            self.fire(s, here, t_s, "audio", json!({ "what": "a spoken line without its voice file", "missing_so_far": missing }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(screen: u64) -> Option<LockProgress> {
        Some(LockProgress { screen, groups: [false; 7] })
    }

    #[test]
    fn controls_locked_fires_at_sixty_seconds_once_per_lock() {
        let mut watch = ControlsLocked::default();
        assert_eq!(watch.step(0.0, lock(1), 0), None);
        assert_eq!(watch.step(59.0, lock(1), CONTROLS_LOCKED_PRESSES), None);
        assert_eq!(watch.step(60.0, lock(1), 0), Some(60.0));
        assert_eq!(watch.step(120.0, lock(1), CONTROLS_LOCKED_PRESSES), None);
        assert_eq!(watch.step(121.0, None, 0), None);
        assert_eq!(watch.step(122.0, lock(1), 0), None);
        assert_eq!(watch.step(182.0, lock(1), CONTROLS_LOCKED_PRESSES), Some(60.0));
    }

    #[test]
    fn controls_locked_ignores_idle_held_and_old_input() {
        let mut watch = ControlsLocked::default();
        watch.step(0.0, lock(1), 1);
        assert_eq!(watch.step(60.0, lock(1), 0), None, "a held key gives no new edges");
        assert_eq!(watch.step(61.0, lock(1), CONTROLS_LOCKED_PRESSES - 1), None, "old press expired");
        assert_eq!(watch.step(122.0, lock(1), 0), None, "idle does not fire");
        assert_eq!(watch.step(123.0, lock(1), 1), None);
        assert_eq!(watch.step(124.0, lock(1), CONTROLS_LOCKED_PRESSES - 1), Some(124.0));
    }

    #[test]
    fn controls_locked_progress_and_reenabled_groups_restart_the_window() {
        let mut watch = ControlsLocked::default();
        let locked = Some(LockProgress { screen: 1, groups: [true; 7] });
        watch.step(0.0, locked, CONTROLS_LOCKED_PRESSES);
        assert_eq!(watch.step(59.0, lock(2), 0), None, "a new line or page is progress");
        assert_eq!(watch.step(60.0, lock(2), CONTROLS_LOCKED_PRESSES), None);
        assert_eq!(watch.step(119.0, lock(2), 0), Some(60.0));
        watch.step(120.0, locked, 0);
        let released = Some(LockProgress { screen: 1, groups: [true, false, true, true, true, true, true] });
        assert_eq!(watch.step(179.0, released, 0), None);
        assert_eq!(watch.step(180.0, released, CONTROLS_LOCKED_PRESSES), None);
        assert_eq!(watch.step(239.0, released, 0), Some(60.0));
    }

    #[test]
    fn controls_locked_loading_clears_input_and_session_cap_still_applies() {
        let mut watch = ControlsLocked::default();
        watch.step(0.0, lock(1), CONTROLS_LOCKED_PRESSES);
        watch.step(59.0, None, CONTROLS_LOCKED_PRESSES);
        watch.step(60.0, lock(1), 0);
        assert_eq!(watch.step(120.0, lock(1), 0), None);
        let (mut d, mut s) = (Detectors::default(), Summary::default());
        for i in 0u32..5 {
            let now = f64::from(i * 61);
            d.controls_locked.step(now, lock(i.into()), 0);
            if let Some(held) = d.controls_locked.step(now + 60.0, lock(i.into()), CONTROLS_LOCKED_PRESSES) {
                d.fire(&mut s, &Here::default(), now as u64, "controls_locked", json!({ "held_s": held }));
            }
        }
        assert_eq!(s.events.len(), MAX_PER_KIND as usize);
    }

    fn kind(level: &str, msg: &str) -> Option<&'static str> {
        match classify(level, msg) {
            Some(Found::Now(k, _)) => Some(k),
            Some(Found::ScriptError(_)) => Some("script_error"),
            None => None,
        }
    }

    #[test]
    fn the_engines_own_lines_become_anomalies() {
        assert_eq!(kind("WARN", "player fell through at (1, 2, -900), terrain z 100: back on the ground"), Some("fell_out_of_world"));
        assert_eq!(kind("WARN", "player fell out of interior 00106185 at (1, 2, 3), lowest collision z -40: back to where they last stood"), Some("fell_out_of_world"));
        assert_eq!(kind("INFO", "ai [12.0] Sunny: cannot reach the place of 0012AB34: no path"), Some("path_failure"));
        assert_eq!(kind("WARN", "model not loaded: meshes\\x.nif: broken"), Some("missing_asset"));
        // #3045: whose file a missing model is, and the base game's own dangling names.
        let mut sources = crate::reports::asset_sources::Sources::default();
        sources.note("meshes\\creatures\\ghoul\\mmm_ghoulvariant_v1.nif", "FalloutNV.esm");
        sources.note("meshes\\mod\\gun.nif", "Mod.esp");
        let event = |what: &str, file: &str| json!({ "what": what, "file": file });
        let ghoul = event("actor file", "meshes\\creatures\\ghoul\\mmm_ghoulvariant_v1.nif: not in any archive");
        assert_eq!(sourced("missing_asset", &ghoul, Some(&sources), true, 0), Sourced::GamesOwn("FalloutNV.esm".into()));
        // The game's own file that is there and does not read is a fault of ours: reported.
        let broken = event("model", "meshes\\creatures\\ghoul\\mmm_ghoulvariant_v1.nif: footer claims 65670 roots");
        assert_eq!(sourced("missing_asset", &broken, Some(&sources), true, 0), Sourced::Plugin("FalloutNV.esm".into()));
        let gun = event("model", "Meshes\\Mod\\Gun.nif: not in any archive");
        assert_eq!(sourced("missing_asset", &gun, Some(&sources), true, 0), Sourced::Plugin("Mod.esp".into()));
        assert_eq!(sourced("missing_asset", &event("model", "meshes\\other.nif: not in any archive"), Some(&sources), true, 0), Sourced::Plain);
        // Textures have no record of their own; other kinds are not looked up.
        assert_eq!(sourced("missing_asset", &event("texture", "meshes\\mod\\gun.nif: not in any archive"), Some(&sources), true, 0), Sourced::Plain);
        assert_eq!(sourced("softlock", &gun, Some(&sources), true, 0), Sourced::Plain);
        // Until the list is made an event waits, for a while; with no list asked for, not at all.
        assert_eq!(sourced("missing_asset", &gun, None, true, SOURCES_WAIT_SECS - 1), Sourced::Wait);
        assert_eq!(sourced("missing_asset", &gun, None, true, SOURCES_WAIT_SECS), Sourced::Plain);
        assert_eq!(sourced("missing_asset", &gun, None, false, 0), Sourced::Plain);
        assert_eq!(sourced("missing_asset", &event("texture", "x.dds: gone"), None, true, 0), Sourced::Plain);
        assert_eq!(kind("WARN", "save: failed: disk full"), Some("save_trouble"));
        assert_eq!(classify("WARN", "softlock: controls_held for 30 s: scripts hold the controls (movement)"), Some(Found::Now("softlock", json!({ "what": "controls_held", "line": "for 30 s: scripts hold the controls (movement)" }))));
        assert_eq!(kind("WARN", "load: C:\\s.nvfsave: bad header; starting again on the main menu"), Some("save_trouble"));
        assert_eq!(kind("WARN", "audio output: no device (gone); sound runs silent until one appears"), Some("audio"));
        assert_eq!(classify("WARN", "script error in GSScript: bad ref"), Some(Found::ScriptError("GSScript".into())));
        assert_eq!(kind("WARN", "dropped 0xFF000179 fell through the world: collision removed, put back where it was dropped"), Some("object_fell_through"));
        assert_eq!(classify("WARN", "ragdoll: actor 001710BB fell through at (-9617, -96521, 10863), terrain z 11015: back above the ground").map(|f| matches!(f, Found::Now(_, ref d) if d["what"] == "corpse")), Some(true));
        assert_eq!(classify("ERROR", "render: OutOfMemory GPU error, the game stops: "), Some(Found::Now("gpu_error", json!({ "error": "OutOfMemory", "line": "render: OutOfMemory GPU error, the game stops: " }))));
        assert_eq!(kind("WARN", "render: GPU validation error 1 of 20 let through (most likely a broken file): x"), None, "a validation error let through is no stop");
        assert_eq!(kind("INFO", "model not loaded: x"), None, "only warnings count");
        assert_eq!(kind("WARN", "something else"), None);
    }

    #[test]
    fn events_have_where_and_are_capped_per_kind() {
        let mut d = Detectors::default();
        let mut s = Summary::default();
        let here = Here { place: "WastelandNV".into(), cell: Some("Goodsprings".into()), pos: [1.4, 2.6, 3.0], hour: Some(13.25), state: "playing" };
        for _ in 0..5 {
            d.fire(&mut s, &here, 7, "hitch", json!({ "ms": 400 }));
        }
        assert_eq!(s.events.len(), 3);
        assert_eq!(s.events[0]["kind"], "hitch");
        assert_eq!(s.events[0]["ms"], 400);
        assert_eq!(s.events[0]["cell"], "Goodsprings");
        assert_eq!(s.events[0]["pos"], json!([1.0, 3.0, 3.0]));
    }

    #[test]
    fn stuck_needs_five_seconds_without_moving() {
        let mut d = Detectors::default();
        let mut s = Summary::default();
        let mut here = Here { pos: [0.0, 0.0, 0.0], ..Default::default() };
        d.check_stuck(&mut s, &here, 0, 0.0, true);
        d.check_stuck(&mut s, &here, 3, 3.0, true);
        assert!(s.events.is_empty());
        here.pos = [5.0, 0.0, 0.0];
        d.check_stuck(&mut s, &here, 6, 6.0, true);
        assert_eq!(s.events.len(), 1, "5 s held, 5 units moved");
        // Walking resets it.
        d.check_stuck(&mut s, &here, 7, 7.0, true);
        here.pos = [100.0, 0.0, 0.0];
        d.check_stuck(&mut s, &here, 13, 13.0, true);
        assert_eq!(s.events.len(), 1);
    }

    #[test]
    fn memory_fires_on_crossing_up_only() {
        let mut d = Detectors::default();
        let mut s = Summary::default();
        let h = Here::default();
        d.check_memory(&mut s, &h, 0, Some((950, 1000)), None);
        d.check_memory(&mut s, &h, 1, Some((960, 1000)), None);
        assert_eq!(s.events.len(), 1);
        d.check_memory(&mut s, &h, 2, Some((500, 1000)), None);
        d.check_memory(&mut s, &h, 3, None, Some(0.95));
        assert_eq!(s.events.len(), 2);
    }

    #[test]
    fn memory_is_not_counted_before_a_game_is_loaded() {
        let mut d = Detectors::default();
        let mut s = Summary::default();
        let mut h = Here { state: "viewer", ..Default::default() };
        d.check_memory(&mut s, &h, 0, Some((30, 8000)), Some(0.96));
        assert!(s.events.is_empty(), "other programs' RAM at the start");
        h.state = "playing";
        d.check_memory(&mut s, &h, 60, Some((30, 8000)), Some(0.96));
        assert_eq!(s.events.len(), 1);
    }
}
