//! Opt-in player reports (docs/design/player-reports.md; #2055): the choice, the session
//! summary and sending it. The server is LootJunkieAzza/NVForever-Data.
//!
//! - **The choice** ([`decide`], at the start, before the window): `[Reports]` in the home's
//!   `NVForever.ini` (`fnv_install::reports`). A player's start that has no answer for the
//!   current [`CONSENT_VERSION`] shows the popup once; the answer is stored at once. Off,
//!   whatever the INI says, in a developer or smoke run (`--data` / `NVFOREVER_DATA`), a
//!   run a test aid drives, and with `NVF_REPORTS=0`. A run a load was handed over to
//!   (handoff.rs: loading a save starts a new process) follows the stored answer and never
//!   asks; it is where the player plays after a load, so it collects and sends like any. `NVF_REPORTS=1` turns it on without asking (tests; such a run sends
//!   only to `NVF_REPORTS_ENDPOINT`, never to the project's server).
//!   Android never collects or sends player reports, regardless of saved or test settings.
//! - **Official builds only** [owner 2026-11-11]: a build is official when the release
//!   workflow made it (`NVF_OFFICIAL_BUILD=1` at build time). A local build never asks and
//!   never sends to the project's server, as development sessions would skew what reports
//!   show of players' games; it sends only to a server it is pointed at
//!   (`NVF_REPORTS_ENDPOINT`, or `Endpoint=` in `[Reports]`). `NVF_REPORTS_PRODUCTION=1` lets
//!   a local build send to the project's server (the owner's own testing).
//! - **The summary** (summary.rs, collected here): written to
//!   `<home>\reports\pending\session-<UTC time>-<pid>.json` every [`WRITE_EVERY`] and at
//!   exit, so a crash leaves the latest copy. Only while reports are on.
//!   A run that never reached the world (the main menu only, then a load handed over) writes
//!   no summary.
//! - **Sending** (upload.rs): at the next start, on a thread of its own, each earlier
//!   session is finished (how it ended, from the run log and crash report crash.rs left;
//!   its run log gzipped beside it) and sent.
//!
//! Nothing is sent from the game folder or `Documents\My Games\FalloutNV`: every file is
//! in the home and passes `fnv_install::guard_write`.

pub mod anomaly;
pub mod asset_sources;
pub mod snapshot;
mod controls;
mod delete;
mod form;
pub mod summary;
pub mod upload;
mod warnings;

use bevy::prelude::*;
use fnv_install::reports::Reports;
use fnv_install::EngineHome;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use summary::{End, Place, Summary};

pub use warnings::layer as warnings_layer;

/// The version of what is sent (player-reports.md §3) the player is asked about; raised
/// only when what is sent grows in kind, which asks again.
pub const CONSENT_VERSION: u32 = 1;
/// How often the running session's summary is rewritten.
pub const WRITE_EVERY: Duration = Duration::from_secs(60);
/// A run log larger than this is cut to its first and last lines before it is sent (§3.1).
const LOG_CAP_BYTES: usize = 2 * 1024 * 1024;
const LOG_HEAD_LINES: usize = 500;
const LOG_TAIL_LINES: usize = 5000;

/// Reports are on for this run: who to send as and where.
#[derive(Debug, Clone, Resource)]
pub struct Active {
    pub install_id: String,
    pub delete_token: String,
    pub endpoint: String,
    /// Sending is allowed (off in a test run without its own endpoint).
    pub may_send: bool,
}

static ACTIVE: OnceLock<Option<Active>> = OnceLock::new();
/// `--data` / `NVFOREVER_DATA` was given (a developer or smoke run; [`decide`]).
static DEVELOPER_DATA: OnceLock<bool> = OnceLock::new();

/// Whether this run supplied `--data` / `NVFOREVER_DATA` ([`decide`]).
pub(crate) fn developer_data() -> bool {
    DEVELOPER_DATA.get().copied().unwrap_or(false)
}

/// Folders hidden in everything a report sends, besides the home folder that
/// `crash::redact` hides: the Data folder, the game folder and NVForever's own folder (a
/// game folder outside the home can still carry a name). Set by [`ReportsPlugin`] before
/// the upload thread starts.
static HIDDEN_FOLDERS: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();

/// `text` as a report sends it: the home folder and [`HIDDEN_FOLDERS`] written as names.
pub fn redact(text: &str) -> String {
    redact_with(text, HIDDEN_FOLDERS.get().map_or(&[], Vec::as_slice))
}

fn redact_with(text: &str, folders: &[(String, &'static str)]) -> String {
    let mut out = crate::crash::redact(text).into_owned();
    for (folder, shown) in folders {
        out = crate::crash::redact_folder(&out, folder, shown).into_owned();
    }
    out
}

/// The folders [`redact`] hides for this run, longest (innermost) first; a root or a drive
/// is left out, as it would hide every path. `mo2`: the Mod Organizer 2 folders of a
/// `--mo2` run ([`mo2_folders`]).
fn folders_to_hide(data: &Path, exe_dir: Option<&Path>, mo2: &[(PathBuf, &'static str)]) -> Vec<(String, &'static str)> {
    let data = std::path::absolute(data).unwrap_or_else(|_| data.to_path_buf());
    let mut v: Vec<(String, &'static str)> = Vec::new();
    let mut add = |p: &Path, shown: &'static str| {
        let s = p.to_string_lossy();
        let s = s.trim_end_matches(['\\', '/']);
        if s.len() > 3 && !v.iter().any(|(f, _)| f.eq_ignore_ascii_case(s)) {
            v.push((s.to_string(), shown));
        }
    };
    add(&data, "<Data>");
    if let Some(game) = data.parent() {
        add(game, "<game>");
    }
    if let Some(exe) = exe_dir {
        add(exe, "<NVForever>");
    }
    for (folder, shown) in mo2 {
        add(&std::path::absolute(folder).unwrap_or_else(|_| folder.clone()), shown);
    }
    v.sort_by_key(|(f, _)| std::cmp::Reverse(f.len()));
    v
}

/// The folders of a `--mo2` run for [`ReportsPlugin::mo2_folders`]: the instance, and its
/// mods and overwrite folders, which the instance's INI may put elsewhere.
pub fn mo2_folders(instance: &Path, layout: Option<&fnv_install::mo2::Layout>) -> Vec<(PathBuf, &'static str)> {
    let mut v = vec![(instance.to_path_buf(), "<MO2>")];
    if let Some(l) = layout {
        v.push((l.mods.clone(), "<MO2 mods>"));
        v.push((l.overwrite.clone(), "<MO2 overwrite>"));
    }
    v
}

/// A new install ID and delete token made while this run plays ("Delete my reports",
/// delete.rs): this run's summary is rewritten under the ID, and anything this run still
/// sends goes with them.
static RENEWED: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

pub(crate) fn renew(install_id: String, delete_token: String) {
    if let Ok(mut r) = RENEWED.lock() {
        *r = Some((install_id, delete_token));
    }
}

/// The ID and token to send as now: [`renew`]'s when there was one, else the run's own.
pub(crate) fn identity(active: &Active) -> (String, String) {
    RENEWED
        .lock()
        .ok()
        .and_then(|r| r.clone())
        .unwrap_or_else(|| (active.install_id.clone(), active.delete_token.clone()))
}

/// This run's choice, once [`decide`] made it.
pub fn active() -> Option<&'static Active> {
    ACTIVE.get().and_then(Option::as_ref)
}

/// A random UUID v4, as text.
pub(crate) fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// A random token for deleting this install's reports (§5.1).
pub(crate) fn new_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Turn reports on or off in the INI (the popup's answer, or the settings switch). On makes
/// a new install id and delete token when there are none; off clears them (§2.2: turning
/// it on again makes new ones). Also keeps the settings page's own `send-reports` row in step.
pub fn store_choice(home: &EngineHome, on: bool) -> Result<Reports, String> {
    let mut r = Reports::load(home);
    r.send = Some(on);
    r.consent_version = Some(CONSENT_VERSION);
    if on {
        r.install_id = r.install_id.or_else(|| Some(new_uuid()));
        r.delete_token = r.delete_token.or_else(|| Some(new_token()));
    } else {
        // Kept, so "Delete my reports" can still reach what this ID sent (delete.rs).
        if let (Some(id), Some(token)) = (r.install_id.take(), r.delete_token.take()) {
            if !r.past.iter().any(|(i, _)| *i == id) {
                r.past.push((id, token));
            }
        }
    }
    r.save(home).map_err(|e| e.to_string())?;
    // The settings page stores its rows in [Settings]; its row for this must agree, or the
    // page would put the old value back at the next start.
    let file = home.config_file();
    if let Ok(text) = std::fs::read_to_string(&file) {
        let text = fnv_install::ini_set(&text, "Settings", SETTING_KEY, if on { "1" } else { "0" });
        if fnv_install::guard_write(&file, None).is_ok() {
            let _ = std::fs::write(&file, text);
        }
    }
    Ok(r)
}

/// Why reports are off for a run that may not ask or send, if one of the reasons holds.
fn forced_off(developer_data: bool) -> Option<&'static str> {
    if std::env::var("NVF_REPORTS").is_ok_and(|v| v.trim() == "0") {
        return Some("NVF_REPORTS=0");
    }
    if std::env::vars_os().any(|(k, _)| k.to_str().is_some_and(nvf_play::drives_run)) {
        return Some("a test aid drives this run");
    }
    developer_data.then_some("a developer or smoke run (--data)")
}

/// Settle this run's choice (module notes); logs it with `eprintln!` (before the log
/// plugin). `developer_data`: `--data` / `NVFOREVER_DATA` was given.
pub fn decide(developer_data: bool, handoff: bool) {
    let _ = DEVELOPER_DATA.set(developer_data);
    let _ = ACTIVE.set(choose(developer_data, handoff));
}

fn choose(developer_data: bool, handoff: bool) -> Option<Active> {
    if cfg!(target_os = "android") {
        return None;
    }
    let home = EngineHome::user()?;
    let test_on = std::env::var("NVF_REPORTS").is_ok_and(|v| v.trim() == "1");
    let endpoint_env = std::env::var("NVF_REPORTS_ENDPOINT").ok().filter(|v| !v.trim().is_empty());
    if !test_on {
        if let Some(why) = forced_off(developer_data) {
            eprintln!("reports: off ({why})");
            return None;
        }
    }
    let mut r = Reports::load(&home);
    // A local build only reports to a server it is pointed at (module notes).
    if !test_on && !OFFICIAL_BUILD && endpoint_env.is_none() && r.endpoint.is_none() && !production_allowed() {
        eprintln!("reports: off (a local build: only official releases send to the project's server; NVF_REPORTS_ENDPOINT for a test server)");
        return None;
    }
    if test_on {
        if r.send != Some(true) || r.install_id.is_none() {
            r = store_choice(&home, true).map_err(|e| eprintln!("reports: off, the choice could not be stored: {e}")).ok()?;
        }
    } else if (r.send.is_none() || r.consent_version.unwrap_or(0) < CONSENT_VERSION) && handoff {
        // The run that handed the load over asked (or could not); never ask mid-load.
        eprintln!("reports: off (not asked in a run a load was handed over to)");
        return None;
    } else if r.send.is_none() || r.consent_version.unwrap_or(0) < CONSENT_VERSION {
        let yes = ask();
        r = match store_choice(&home, yes) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("reports: off, the answer could not be stored: {e}");
                return None;
            }
        };
        eprintln!("reports: the player chose {}", if yes { "to send reports" } else { "not to send reports" });
    }
    if r.send != Some(true) {
        eprintln!("reports: off (the player's choice; Settings > Gameplay > Bug Reports)");
        return None;
    }
    let (Some(install_id), Some(delete_token)) = (r.install_id, r.delete_token) else {
        eprintln!("reports: off ([Reports] has no InstallId or DeleteToken)");
        return None;
    };
    let test_run = test_on && forced_off(developer_data).is_some();
    let ini_endpoint = r.endpoint.is_some();
    let endpoint = endpoint_env.clone().or(r.endpoint).unwrap_or_else(|| upload::DEFAULT_ENDPOINT.to_string());
    let may_send = may_send(SendCase {
        test_on,
        test_run,
        endpoint_env: endpoint_env.is_some(),
        ini_endpoint,
    });
    eprintln!("reports: on, sending to {endpoint}{}", if may_send { "" } else { " (not in this test run: no NVF_REPORTS_ENDPOINT)" });
    Some(Active { install_id, delete_token, endpoint: endpoint.trim_end_matches('/').to_string(), may_send })
}

/// What decides whether a run that has reports on may send (module notes).
#[derive(Clone, Copy, Default)]
struct SendCase {
    /// `NVF_REPORTS=1`.
    test_on: bool,
    /// `NVF_REPORTS=1` in a run that would otherwise be off (`--data`, a test aid, `NVF_REPORTS=0`).
    test_run: bool,
    /// `NVF_REPORTS_ENDPOINT` is set.
    endpoint_env: bool,
    /// `[Reports] Endpoint` is set (a self-hosted server).
    ini_endpoint: bool,
}

/// `NVF_REPORTS=1` sends only to a server it is pointed at, never to the project's server,
/// whatever the build (§8): `NVF_REPORTS_ENDPOINT`, or, outside a test run, the INI's own
/// `Endpoint`. Without `NVF_REPORTS=1`, `choose` has already turned a local build with no
/// server of its own off (`OFFICIAL_BUILD` and `NVF_REPORTS_PRODUCTION` decide there); they
/// do not open the project's server to `NVF_REPORTS=1`, which skips the popup.
fn may_send(c: SendCase) -> bool {
    if !c.test_on {
        return true;
    }
    c.endpoint_env || (!c.test_run && c.ini_endpoint)
}

/// Made by the release workflow (`.github/workflows/release.yml` sets it at build time).
pub const OFFICIAL_BUILD: bool = option_env!("NVF_OFFICIAL_BUILD").is_some();

/// `NVF_REPORTS_PRODUCTION=1`: a local build may send to the project's server.
fn production_allowed() -> bool {
    std::env::var("NVF_REPORTS_PRODUCTION").is_ok_and(|v| v.trim() == "1")
}

const ASK_TITLE: &str = "Help fix NVForever?";
/// The popup's words (player-reports.md §2.1; our own words).
const ASK_TEXT: &str = "NVForever can send a report after each session: the build, your PC's hardware and settings, \
    your mod list, how the game ran, the game's log and any problems it noticed. No personal details and no saves.\n\n\
    The reports are kept private and used only to find and fix bugs. You can change this at any time in \
    Settings > Gameplay > Bug Reports.\n\nWhat is sent: https://github.com/LootJunkieAzza/NVForever/blob/main/docs/design/player-reports.md";

/// The popup; true: Send reports. Closing it or Esc is Not now.
#[cfg(windows)]
fn ask() -> bool {
    use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
    let r = MessageDialog::new()
        .set_title(ASK_TITLE)
        .set_level(MessageLevel::Info)
        .set_description(ASK_TEXT)
        .set_buttons(MessageButtons::OkCancelCustom("Send reports".into(), "Not now".into()))
        .show();
    matches!(r, MessageDialogResult::Ok) || matches!(&r, MessageDialogResult::Custom(s) if s == "Send reports")
}

/// No dialogs off Windows yet: not asked, so off (§1.1: any run where the popup could not be shown).
#[cfg(not(windows))]
fn ask() -> bool {
    let _ = (ASK_TITLE, ASK_TEXT);
    false
}

/// The reports folders, created and checked; `None` (logged) when they cannot be used.
pub fn folders() -> Option<(PathBuf, PathBuf)> {
    let home = EngineHome::user()?;
    let root = home.reports();
    let (pending, sent) = (root.join("pending"), root.join("sent"));
    for d in [&pending, &sent] {
        if let Err(e) = fnv_install::guard_write(d, None).map_err(|e| e.to_string()).and_then(|()| std::fs::create_dir_all(d).map_err(|e| e.to_string())) {
            warn!("reports: {}: {e}", d.display());
            return None;
        }
    }
    Some((pending, sent))
}

// ------------------------------------------------------------------ the collector

/// Per-subsystem log levels from `[Log]` in NVForever.ini (player-reports.md §4.1), e.g.
/// `script=debug`, so a player asked to reproduce a bug can turn one part up. A name is one
/// of [`LOG_SUBSYSTEMS`] or a module path (`nvf_play::doors=trace`); levels are trace,
/// debug, info, warn, error, off. The result is the log plugin's filter: Bevy's default with
/// these added. `RUST_LOG` still goes on top [observed: bevy_log 0.20.1
/// `LogPlugin::build_filter_layer` adds its directives to this filter; its doc comment says
/// they replace it, the code does not].
pub fn log_filter() -> String {
    let ini = EngineHome::user().and_then(|h| std::fs::read_to_string(h.config_file()).ok()).unwrap_or_default();
    let extra = log_directives(&ini);
    if extra.is_empty() {
        return bevy::log::DEFAULT_FILTER.to_string();
    }
    eprintln!("setup: log levels from [Log]: {extra}");
    format!("{},{extra}", bevy::log::DEFAULT_FILTER)
}

/// `[Log]` names and the modules they turn up.
pub const LOG_SUBSYSTEMS: &[(&str, &[&str])] = &[
    ("script", &["nvf_play::script", "fnv_game", "fnv_script"]),
    ("ai", &["fnv_ai", "nvforever::residents"]),
    ("nav", &["fnv_nav"]),
    ("physics", &["fnv_phys", "nvf_play::phys"]),
    ("assets", &["nvf_assets"]),
    ("save", &["nvf_play::save", "fnv_save"]),
    ("quest", &["nvf_play::script", "fnv_game::quest"]),
    ("dialogue", &["nvf_play::dialogue"]),
    ("render", &["nvforever"]),
    ("audio", &["nvf_play::audio_out", "nvf_play::voice", "nvf_play::music"]),
    ("ui", &["nvf_play::pipboy", "nvf_play::interface", "nvf_play::settings_view"]),
    ("reports", &["nvforever::reports"]),
];

fn log_directives(ini: &str) -> String {
    let mut out = Vec::new();
    let mut in_log = false;
    for line in ini.lines().map(str::trim) {
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_log = s.trim().eq_ignore_ascii_case("Log");
            continue;
        }
        if !in_log || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim().to_ascii_lowercase());
        if !matches!(v.as_str(), "trace" | "debug" | "info" | "warn" | "error" | "off") {
            continue;
        }
        match LOG_SUBSYSTEMS.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)) {
            Some((_, modules)) => out.extend(modules.iter().map(|m| format!("{m}={v}"))),
            None if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':') && !k.is_empty() => out.push(format!("{k}={v}")),
            None => {}
        }
    }
    out.join(",")
}

/// The settings page's row (nvf-play's settings_layout names it).
pub const SETTING_KEY: &str = "send-reports";

/// Collects this session's summary while reports are on; adds the settings row always.
pub struct ReportsPlugin {
    /// The plugins loaded, in order.
    pub plugins: Vec<String>,
    pub data: PathBuf,
    pub mo2: bool,
    /// The Mod Organizer 2 folders of a `--mo2` run and what each is written as in a
    /// report (the instance, its mods and overwrite folders): they are often outside the
    /// home folder and can carry a name (#3152).
    pub mo2_folders: Vec<(PathBuf, &'static str)>,
    /// A load handed over from an earlier run (kept for the log; it collects and sends as any run).
    pub handoff: bool,
    /// What this run's start loads: `save_load` (`--load`) or `new_game`; it names the start's
    /// loading screen in the summary's `loads`.
    pub start_kind: &'static str,
}

/// The official plugins whose quests make the quest catalogue (the base game and its add-ons).
pub const OFFICIAL_PLUGINS: [&str; 10] = [
    "FalloutNV.esm",
    "DeadMoney.esm",
    "HonestHearts.esm",
    "OldWorldBlues.esm",
    "LonesomeRoad.esm",
    "GunRunnersArsenal.esm",
    "CaravanPack.esm",
    "ClassicPack.esm",
    "MercenaryPack.esm",
    "TribalPack.esm",
];

#[derive(Resource)]
struct Collector {
    summary: Summary,
    file: PathBuf,
    started: Instant,
    last_write: Instant,
    places: std::collections::HashMap<String, usize>,
    cells_seen: std::collections::HashSet<u32>,
    last_space: Option<(String, Instant)>,
    space_time: std::collections::HashMap<String, f32>,
    start_kind: &'static str,
    /// A loading screen up now: (real seconds when it went up, kind).
    load: Option<(f64, &'static str)>,
    /// Where the player is this frame, for anomaly events.
    here: anomaly::Here,
    /// When the last loading screen went down (real seconds).
    load_ended: f64,
    /// Until when (real seconds) the frames are not play: the window lost or got back its
    /// focus, was covered, shown again or minimised, or the app was suspended or resumed
    /// (`track_away`).
    away_until: f64,
    detectors: anomaly::Detectors,
}

impl Plugin for ReportsPlugin {
    fn build(&self, app: &mut App) {
        nvf_play::settings::add_setting(
            app,
            nvf_play::settings::Setting {
                key: SETTING_KEY,
                group: "Reports",
                label: "Send reports to help fix NVForever",
                note: "After each session: build, hardware, settings, mod list, how it ran and the log; no personal details, no saves. From the next start",
                switches: "",
                get: |_| EngineHome::user().is_some_and(|h| Reports::load(&h).send == Some(true)),
                set: |_, on| {
                    if let Some(h) = EngineHome::user() {
                        if Reports::load(&h).send != Some(on) {
                            match store_choice(&h, on) {
                                Ok(_) => info!("reports: turned {} in the settings (from the next start)", if on { "on" } else { "off" }),
                                Err(e) => warn!("reports: the choice could not be stored: {e}"),
                            }
                        }
                    }
                },
                slider: None,
            },
        );
        delete::add_row(app);
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        let _ = HIDDEN_FOLDERS.set(folders_to_hide(&self.data, exe_dir.as_deref(), &self.mo2_folders));
        // A hand report needs no opt-in (§3.3): the form and the snapshot it sends are
        // there whatever the choice.
        form::add(app);
        app.add_systems(Update, refresh_snapshot.run_if(|| active().is_some() || form::wants_snapshot()));
        let Some(active) = active().cloned() else { return };
        let Some((pending, sent)) = folders() else { return };
        if active.may_send {
            if self.handoff {
                info!("reports: a load handed over to this run; earlier sessions are sent from here");
            }
            upload::spawn(active.clone(), pending.clone(), sent, std::process::id());
        } else if !active.may_send {
            info!("reports: sending skipped in this test run (no NVF_REPORTS_ENDPOINT)");
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let file = pending.join(format!("session-{}-{}.json", crate::crash::stamp(now), std::process::id()));
        let summary = base_summary(&active, &self.plugins, &self.data, self.mo2, now);
        spawn_plugin_crcs(&self.plugins, &self.data);
        app.insert_resource(active)
            .insert_resource(Collector {
                summary,
                file,
                started: Instant::now(),
                last_write: Instant::now(),
                places: Default::default(),
                cells_seen: Default::default(),
                last_space: None,
                space_time: Default::default(),
                start_kind: self.start_kind,
                load: None,
                here: Default::default(),
                load_ended: 0.0,
                away_until: 0.0,
                detectors: Default::default(),
            })
            .add_systems(Startup, (read_gpu, read_window))
            .add_systems(Update, (track_away, collect, track_loads, list_model_sources, detect.after(nvf_play::control::enforce_locks)).chain())
            // After Bevy's window-closed exit (AppExit in Last): else a window closed by the
            // player missed the clean end and the run was sent as a native crash (#2203).
            .add_systems(Last, (controls::detect, write_summary.after(bevy::window::ExitSystems)).chain());
    }
}

/// `YYYY-MM-DDTHH:MM:00Z` of a Unix time, rounded down to the minute (§3.1).
fn iso_minute(unix: u64) -> String {
    let (y, m, d) = crate::crash::civil_from_days((unix / 86_400) as i64);
    let s = unix % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:00Z", s / 3600, s / 60 % 60)
}

fn base_summary(active: &Active, plugins: &[String], data: &Path, mo2: bool, now: u64) -> Summary {
    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
    let sys = System::new_with_specifics(RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing()).with_memory(MemoryRefreshKind::nothing().with_ram()));
    let settings = EngineHome::user()
        .and_then(|h| std::fs::read_to_string(h.config_file()).ok())
        .map(|t| settings_rows(&t))
        .unwrap_or_default();
    Summary {
        install_id: active.install_id.clone(),
        session_id: new_uuid(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        commit: option_env!("NVF_GIT_COMMIT").map(str::to_string),
        profile: if cfg!(debug_assertions) { "dev" } else { "release" },
        dirty: None,
        start_utc: iso_minute(now),
        os: System::name(),
        os_build: System::os_version(),
        cpu: sys.cpus().first().map(|c| c.brand().trim().to_string()).filter(|s| !s.is_empty()),
        cores: Some(sys.cpus().len()).filter(|&n| n > 0),
        ram_gib: Some((sys.total_memory() as f64 / (1u64 << 30) as f64).round() as u64).filter(|&g| g > 0),
        settings,
        plugins: plugins.iter().map(|p| (p.clone(), fnv_install::child_ci(data, p).and_then(|f| std::fs::metadata(f).ok()).map(|m| m.len()), None)).collect(),
        mo2,
        ..Default::default()
    }
}

/// The plugins' CRC32s (§3.1 `load_order`), in load order, once [`spawn_plugin_crcs`] has
/// read them all.
static PLUGIN_CRCS: OnceLock<Vec<Option<String>>> = OnceLock::new();

/// Reads every plugin on a thread of its own (the official masters are hundreds of MB; the
/// game never waits on it) for its CRC32, as 8 hex digits; the summary gets them at its
/// next write. Sizes and CRCs tell versions of a plugin apart.
fn spawn_plugin_crcs(plugins: &[String], data: &Path) {
    let files: Vec<Option<PathBuf>> = plugins.iter().map(|p| fnv_install::child_ci(data, p)).collect();
    let _ = std::thread::Builder::new().name("nvf-reports-crc".into()).spawn(move || {
        let crcs = files.iter().map(|f| f.as_deref().and_then(file_crc32)).collect();
        let _ = PLUGIN_CRCS.set(crcs);
    });
}

/// A file's CRC32 (the zlib/gzip one), as 8 upper-case hex digits.
fn file_crc32(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut crc = flate2::Crc::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => crc.update(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(format!("{:08X}", crc.sum()))
}

/// The settings the player changed: the `[Settings]` rows of the INI (the page writes only
/// what was changed) and `[Text] CodePage`; never `[Game] Path` or `[Reports]` (§3.1).
fn settings_rows(ini: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut section = String::new();
    for line in ini.lines().map(str::trim) {
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = s.trim().to_string();
        } else if let Some((k, v)) = line.split_once('=').filter(|_| !line.starts_with(';') && !line.starts_with('#')) {
            let keep = section.eq_ignore_ascii_case("Settings") || (section.eq_ignore_ascii_case("Text") && k.trim().eq_ignore_ascii_case("CodePage"));
            if keep && !k.trim().eq_ignore_ascii_case(SETTING_KEY) {
                out.push((format!("{section}.{}", k.trim()), v.trim().to_string()));
            }
        }
    }
    out
}

fn gpu_vendor(id: u32) -> &'static str {
    match id {
        0x10DE => "nvidia",
        0x1002 | 0x1022 => "amd",
        0x8086 => "intel",
        0x5143 => "qualcomm",
        0x106B => "apple",
        _ => "other",
    }
}

fn read_gpu(adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>, mut c: ResMut<Collector>) {
    if let Some(a) = adapter {
        c.summary.gpu_name = Some(a.name.clone());
        c.summary.gpu_vendor = Some(gpu_vendor(a.vendor).to_string());
        c.summary.gpu_driver = Some(format!("{} {}", a.driver, a.driver_info).trim().to_string()).filter(|s| !s.is_empty());
        c.summary.graphics_api = Some(format!("{:?}", a.backend));
    }
}

fn read_window(windows: Query<&Window, With<bevy::window::PrimaryWindow>>, mut c: ResMut<Collector>) {
    if let Ok(w) = windows.single() {
        c.summary.display = Some((w.resolution.physical_width(), w.resolution.physical_height(), None, format!("{:?}", w.mode)));
    }
}

/// Every frame: its time in the camera's place, cells visited, time in each worldspace.
fn collect(
    time: Res<Time>,
    real: Res<Time<Real>>,
    player: Option<Res<nvf_play::player::Player>>,
    cam: Query<&GlobalTransform, With<crate::fly::FlyCam>>,
    streamer: Option<Res<crate::world::Streamer>>,
    world: Option<Res<crate::world::WorldData>>,
    mut c: ResMut<Collector>,
) {
    let (Some(streamer), Some(world), Ok(cam)) = (streamer, world, cam.single()) else { return };
    let g = crate::world::bevy_to_game(cam.translation());
    let (place, interior, cell) = match streamer.interior {
        Some(id) => {
            let c = world.index.cells.get(&id);
            (c.and_then(|c| c.edid.clone()).unwrap_or_else(|| format!("{id:08X}")), true, c)
        }
        None => {
            let ws = world.index.worldspaces.get(&world.world).and_then(|w| w.edid.clone()).unwrap_or_else(|| format!("{:08X}", world.world));
            (ws, false, world.grid_cell(fnv_world::cell_of(g.x, g.y)))
        }
    };
    let c = &mut *c;
    let t_s = c.started.elapsed().as_secs();
    if let Some(cell) = cell {
        if c.summary.cells.len() < summary::MAX_CELLS && c.cells_seen.insert(cell.form_id) {
            c.summary.cells.push((format!("{:08X}", cell.form_id), cell.edid.clone()));
        }
    }
    let cell_name = cell.and_then(|c| c.edid.clone()).unwrap_or_default();
    c.here.place.clone_from(&place);
    c.here.cell = cell.map(|cl| cl.edid.clone().unwrap_or_else(|| format!("{:08X}", cl.form_id)));
    c.here.pos = player.as_ref().map_or([g.x, g.y, g.z], |p| p.feet);
    let i = *c.places.entry(place.clone()).or_insert_with(|| {
        c.summary.places.push(Place { name: place.clone(), interior, ..Default::default() });
        c.summary.places.len() - 1
    });
    // The first frames after a load are the load, not play; nor are frames with the game
    // away (`track_away`).
    let ms = time.delta_secs() * 1000.0;
    if ms > 0.0 && t_s > 2 && !c.away(real.elapsed_secs_f64(), real.delta_secs_f64()) {
        c.summary.places[i].add_frame(ms, t_s, &cell_name);
    }
    if !interior {
        *c.space_time.entry(place.clone()).or_default() += time.delta_secs();
    }
    if c.last_space.as_ref().is_none_or(|(p, _)| *p != place) {
        c.last_space = Some((place, Instant::now()));
    }
}

/// Loading screens, timed on the real clock from when one goes up to when it goes down
/// (nvf_play::loading: the screen pauses the game clock; `finish` lowers it in `Last`, so
/// this sees it the next frame, a frame's time late at most). The kind: the start's
/// (`new_game` or `save_load`, from the command line; a save loaded in play hands over to a
/// new run, which starts with it), `fast_travel` (FastTravelLanding), `door` (a load door's
/// arrival) or `other` (a script or the console moving the player). A run's own cover before
/// it hands a load over is never timed: the run ends under it.
fn track_loads(
    real: Res<Time<Real>>,
    loading: Option<Res<nvf_play::loading::Loading>>,
    fast: Option<Res<nvf_play::save::FastTravelLanding>>,
    mut c: ResMut<Collector>,
) {
    let Some(loading) = loading else { return };
    let now = real.elapsed_secs_f64();
    let fast = fast.is_some_and(|f| f.0);
    match (loading.active(), c.load) {
        (true, None) => {
            let kind = if loading.starting() {
                c.start_kind
            } else if !loading.travelling() {
                return;
            } else if fast {
                "fast_travel"
            } else if loading.arriving {
                "door"
            } else {
                "other"
            };
            c.load = Some((now, kind));
        }
        (true, Some((t, kind))) => {
            // Fast travel's flag may be set after the screen went up.
            if fast && matches!(kind, "door" | "other") {
                c.load = Some((t, "fast_travel"));
            }
        }
        (false, Some((t, kind))) => {
            c.load = None;
            c.load_ended = now;
            let ms = ((now - t) * 1000.0) as f32;
            if ms > anomaly::SLOW_LOAD_MS {
                let c = &mut *c;
                let t_s = c.started.elapsed().as_secs();
                // Where the time went (issue #2407): each wait's end and the longest frame.
                let phases = loading.last_phases.as_ref().map(|p| {
                    let until: serde_json::Map<String, serde_json::Value> = p.until.iter().map(|(n, t)| (n.to_string(), serde_json::json!((t * 1000.0).round()))).collect();
                    let pl = &p.pipelines;
                    // Where the pipelines' time went (#2407), ms summed over the compile threads.
                    let pipelines = serde_json::json!({ "compiled": pl.pipelines, "queued_ms": pl.queued_ms.round(), "lock_ms": pl.lock_ms.round(), "compose_ms": pl.compose_ms.round(), "modules_ms": pl.naga_ms.round(), "create_ms": pl.create_ms.round() });
                    serde_json::json!({ "until_ms": until, "longest_frame_ms": (p.longest_frame * 1000.0).round(), "frames": p.frames, "read_files": p.reads.files, "read_mb": (p.reads.bytes as f64 / 1e5).round() / 10.0, "read_ms": (p.reads.read_ns as f64 / 1e6).round(), "queued_ms": (p.reads.queued_ns as f64 / 1e6).round(), "pipelines": pipelines })
                });
                c.detectors.fire(&mut c.summary, &c.here, t_s, "slow_load", serde_json::json!({ "load": kind, "ms": ms.round(), "phases": phases }));
            }
            match c.summary.loads.iter_mut().find(|(k, _)| k == kind) {
                Some((_, l)) => l.times_ms.push(ms),
                None => c.summary.loads.push((kind.to_string(), summary::Loads { times_ms: vec![ms] })),
            }
        }
        (false, None) => {}
    }
}

/// Rebuild the world snapshot every [`snapshot::EVERY_SECS`] (snapshot.rs).
fn refresh_snapshot(
    real: Res<Time<Real>>,
    mut last: Local<f64>,
    script: Option<ResMut<nvf_play::script::ScriptState>>,
    sim: Option<ResMut<nvf_play::sim::SimState>>,
    streamer: Option<Res<crate::world::Streamer>>,
    memory: Option<Res<crate::memory_budget::MemoryBudget>>,
) {
    let now = real.elapsed_secs_f64();
    if now - *last < snapshot::EVERY_SECS && !form::snapshot_now() {
        return;
    }
    *last = now;
    let (Some(mut script), Some(mut sim)) = (script, sim) else { return };
    let mut defaults: Vec<(fnv_sim::FormId, f32)> = sim.forms.globals.iter().map(|(k, v)| (*k, *v)).collect();
    defaults.sort_by_key(|(k, _)| k.0);
    let cells = serde_json::json!({
        "interior": streamer.as_ref().and_then(|s| s.interior).map(|c| format!("{c:08X}")),
        "exterior_loaded": streamer.as_ref().map(|s| s.loaded.len()),
    });
    let memory = serde_json::json!({
        "vram_use_mb": memory.as_ref().and_then(|m| m.vram_use_mb),
        "ram_share": memory.as_ref().and_then(|m| m.ram_share),
    });
    let Some(rt) = script.rt.as_mut() else { return };
    let v = nvf_play::script::with_world(rt, &mut sim, |rt| snapshot::build(&mut rt.game, &defaults, cells, memory));
    snapshot::keep(v);
}

/// Whether the player asks to move and could: playing, a movement key held, and neither a
/// script's movement lock nor furniture holding them (`held_still`; #3046).
fn asks_to_move(state: &str, intent: Option<&nvf_play::player::MoveIntent>, held_still: bool) -> bool {
    state == "playing" && !held_still && intent.is_some_and(|i| i.forward.abs() + i.right.abs() > 0.1)
}

#[cfg(test)]
mod stuck_tests {
    use super::{asks_to_move, away_frame};
    use nvf_play::player::MoveIntent;

    /// #3046: keys held while a script locks movement (Doc Mitchell's opening) or furniture
    /// holds the player are not a player stuck; the same keys free are.
    #[test]
    fn frames_after_the_game_was_away_are_not_play() {
        assert!(away_frame(10.5, 0.4, 11.0), "within a second of a focus change");
        assert!(!away_frame(11.5, 0.4, 11.0), "after it");
        assert!(away_frame(500.0, 45.0, 0.0), "the machine slept");
        assert!(!away_frame(500.0, 2.0, 0.0), "a long frame in play is a hitch");
    }

    #[test]
    fn held_keys_count_only_when_the_player_could_move() {
        let forward = MoveIntent { forward: 1.0, ..MoveIntent::default() };
        assert!(asks_to_move("playing", Some(&forward), false));
        assert!(!asks_to_move("playing", Some(&forward), true), "locked or seated");
        assert!(!asks_to_move("menu", Some(&forward), false));
        assert!(!asks_to_move("playing", Some(&MoveIntent::default()), false));
        assert!(!asks_to_move("playing", None, false));
    }
}

impl Collector {
    /// Whether a frame that ends `now` (real seconds) after `dt` seconds is the game away
    /// rather than play ([`away_frame`]).
    fn away(&self, now: f64, dt: f64) -> bool {
        away_frame(now, dt, self.away_until)
    }
}

/// Seconds after the window or the app comes back during which frames are not counted as
/// play [design, #3043: the first frames after a focus change or a resume carry the time
/// away, and pipelines or textures the driver dropped while hidden are made again].
pub const AWAY_SETTLE_SECS: f64 = 1.0;
/// A frame this long or longer is taken as the machine having slept (no window message
/// comes for a sleep) [design, #3043: no hitch a report has shown runs this long; a real
/// stall that long is a hang, which the crash handler's watchdog reports].
pub const AWAY_FRAME_SECS: f64 = 30.0;

/// Whether a frame that ends at `now` (real seconds) and lasted `dt` is the game away, not
/// play: it ends before `away_until` (a focus change, a cover or a suspend in the
/// [`AWAY_SETTLE_SECS`] before), or it lasted [`AWAY_FRAME_SECS`] or more. Such frames
/// count neither as hitches nor in the places' frame times [design, #3043: "decide whether
/// a frame after a suspend counts as a hitch": it does not].
pub fn away_frame(now: f64, dt: f64, away_until: f64) -> bool {
    now <= away_until || dt >= AWAY_FRAME_SECS
}

/// Frames are not play for [`AWAY_SETTLE_SECS`] after the primary window loses or gets
/// back its focus, is covered or shown again (minimised included), or the app is
/// suspended or resumed, and while the window does not have the focus at all.
fn track_away(
    real: Res<Time<Real>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    (mut focus, mut occluded, mut life): (MessageReader<bevy::window::WindowFocused>, MessageReader<bevy::window::WindowOccluded>, MessageReader<bevy::window::AppLifecycle>),
    mut c: ResMut<Collector>,
) {
    let changed = focus.read().count() + occluded.read().count() + life.read().count() > 0;
    let unfocused = windows.single().is_ok_and(|w| !w.focused);
    if changed || unfocused {
        c.away_until = real.elapsed_secs_f64() + AWAY_SETTLE_SECS;
    }
}

/// Whose record names each model file, listed once on a thread of its own as soon as the
/// load order is there (#3045, asset_sources.rs).
fn list_model_sources(world: Option<Res<crate::world::WorldData>>) {
    if let Some(world) = world {
        anomaly::list_model_sources(world.lo);
    }
}

/// The anomaly detectors that read the game's state each frame (anomaly.rs), and the ones
/// the log lines found since the last frame, given where the player is.
fn detect(
    real: Res<Time<Real>>,
    loading: Option<Res<nvf_play::loading::Loading>>,
    intent: Option<Res<nvf_play::player::MoveIntent>>,
    (locks, seat): (Option<Res<nvf_play::control::ControlLocks>>, Option<Res<nvf_play::furniture::PlayerSeat>>),
    voice: Option<Res<nvf_play::voice::VoiceState>>,
    memory: Option<Res<crate::memory_budget::MemoryBudget>>,
    mut c: ResMut<Collector>,
) {
    let c = &mut *c;
    let t_s = c.started.elapsed().as_secs();
    let loading_now = loading.as_ref().is_some_and(|l| l.active());
    // The game hour and state, as the structured events say them (events_log.rs, last frame's).
    let (hour, state) = crate::events_log::context();
    c.here.hour = hour;
    c.here.state = if loading_now {
        "loading"
    } else if state.is_empty() {
        "viewer"
    } else {
        state
    };
    c.detectors.drain_log(&mut c.summary, &c.here, t_s);
    // A hitch: a frame over the limit while the world runs (not behind a loading screen).
    let ms = real.delta_secs() * 1000.0;
    let settled = real.elapsed_secs_f64() - c.load_ended > anomaly::HITCH_SETTLE_SECS && t_s > 20;
    let away = c.away(real.elapsed_secs_f64(), real.delta_secs_f64());
    if ms > anomaly::HITCH_MS && !loading_now && settled && !away && matches!(c.here.state, "playing" | "viewer") {
        c.detectors.fire(&mut c.summary, &c.here, t_s, "hitch", serde_json::json!({ "ms": ms.round() }));
    }
    // Movement keys held count only when the player could move: not while scripts lock
    // movement (the opening in Doc Mitchell's house: 3 of 49 installs' only player_stuck,
    // #3046; controls_locked covers a lock) nor while furniture holds the player. Read after
    // `enforce_locks`, which strikes a locked movement from the frame's intent.
    let held_still = locks.as_ref().is_some_and(|l| l.movement) || seat.as_ref().is_some_and(|s| s.hold.is_some());
    let moving = asks_to_move(c.here.state, intent.as_deref(), held_still);
    c.detectors.check_stuck(&mut c.summary, &c.here, t_s, real.elapsed_secs_f64(), moving);
    if let Some(m) = memory {
        c.detectors.check_memory(&mut c.summary, &c.here, t_s, m.vram_use_mb, m.ram_share);
    }
    if let Some(v) = voice {
        c.detectors.check_voice(&mut c.summary, &c.here, t_s, v.missing);
    }
}

/// A quest's plugin and its form ID within that plugin (top byte 00), when the plugin is one
/// of [`OFFICIAL_PLUGINS`].
fn official_quest(load_order: &[String], form: u32) -> Option<(String, String)> {
    let plugin = load_order.get((form >> 24) as usize)?;
    let official = OFFICIAL_PLUGINS.iter().find(|p| p.eq_ignore_ascii_case(plugin))?;
    Some((format!("00{:06X}", form & 0x00FF_FFFF), official.to_string()))
}

/// The quest catalogue and the completed quests, from the running game (module notes; the
/// Pip-Boy lists a quest with a name and objectives, nvf-play pipboy/data.rs `listed`).
fn read_quests(g: &fnv_game::game::Game, s: &mut Summary) {
    let order = &g.data.load_order;
    if s.quest_catalog.is_empty() {
        s.quest_catalog = g
            .data
            .quests
            .iter()
            .filter(|(_, d)| d.name.as_deref().is_some_and(|n| !n.trim().is_empty()) && !d.objectives.is_empty())
            .filter_map(|(id, d)| official_quest(order, id.0).map(|(f, p)| (f, p, d.edid.clone(), d.name.clone())))
            .collect();
    }
    let listed: std::collections::HashSet<(&str, &str)> = s.quest_catalog.iter().map(|(f, p, _, _)| (f.as_str(), p.as_str())).collect();
    let completed: Vec<(String, String)> = g
        .quests
        .iter()
        .filter(|(_, st)| st.completed && !st.failed)
        .filter_map(|(id, _)| official_quest(order, id.0))
        .filter(|(f, p)| listed.contains(&(f.as_str(), p.as_str())))
        .collect();
    s.quests_completed = completed;
}

/// Rewrite the summary every [`WRITE_EVERY`], and at exit as a clean end.
fn write_summary(mut exit: MessageReader<AppExit>, script: Option<Res<nvf_play::script::ScriptState>>, mut c: ResMut<Collector>) {
    let exiting = exit.read().next().is_some();
    // The player deleted their reports: this session goes on under the new ID, written now
    // so a crash cannot leave it under the deleted one.
    let renewed = RENEWED.lock().ok().and_then(|r| r.as_ref().map(|(id, _)| id.clone())).filter(|id| *id != c.summary.install_id);
    if let Some(id) = &renewed {
        c.summary.install_id = id.clone();
    }
    if !exiting && renewed.is_none() && c.last_write.elapsed() < WRITE_EVERY {
        return;
    }
    let c = &mut *c;
    c.last_write = Instant::now();
    // Never in the world (the main menu, then a load handed to a new run): nothing to say.
    if c.summary.places.iter().all(|p| p.histogram.iter().sum::<u64>() == 0) {
        if exiting {
            info!("reports: no summary for this run (it never reached the world)");
        }
        return;
    }
    // What the last frame's log lines found: a GPU error that stops the game (`gpu_error`)
    // is logged in the frame the game exits in, after `detect` ran (#2758).
    if exiting {
        let t_s = c.started.elapsed().as_secs();
        c.detectors.drain_log(&mut c.summary, &c.here, t_s);
    }
    bring_up_to_date(c, script.as_deref(), exiting);
    let bytes = c.summary.to_bytes(redact);
    match fnv_install::guard_write(&c.file, None).map_err(|e| e.to_string()).and_then(|()| std::fs::write(&c.file, &bytes).map_err(|e| e.to_string())) {
        Ok(()) if exiting => info!("reports: session summary written to {}", c.file.display()),
        Ok(()) => debug!("reports: session summary written to {}", c.file.display()),
        Err(e) => warn!("reports: the session summary could not be written to {}: {e}", c.file.display()),
    }
}

/// Bring the summary up to now (written every minute and at exit, and sent with a hand
/// report, form.rs); `exiting`: the run is ending cleanly.
fn bring_up_to_date(c: &mut Collector, script: Option<&nvf_play::script::ScriptState>, exiting: bool) {
    c.summary.duration_s = c.started.elapsed().as_secs() / 60 * 60;
    c.summary.end = if exiting { End::Clean } else { End::Unknown };
    c.summary.worldspace_time_s = c.space_time.iter().map(|(k, v)| (k.clone(), *v as u64)).collect();
    c.summary.warnings = warnings::groups();
    if let Some(crcs) = PLUGIN_CRCS.get() {
        for ((_, _, crc), new) in c.summary.plugins.iter_mut().zip(crcs) {
            crc.clone_from(new);
        }
    }
    if let Some(rt) = script.and_then(|s| s.rt.as_ref()) {
        let g = &rt.game;
        let mut stubs: std::collections::BTreeMap<String, u64> = Default::default();
        for (op, n) in g.stubbed.iter().chain(g.unhandled.iter()) {
            let name = g.data.table.get(*op).map(|f| f.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("0x{op:04X}"));
            *stubs.entry(name).or_default() += n;
        }
        c.summary.script_stubs = stubs.into_iter().collect();
        read_quests(g, &mut c.summary);
    }
}

// ------------------------------------------------------------------ earlier sessions

/// Finish an earlier session's upload before it is sent: how it ended (a summary still
/// saying `unknown` was cut short: `panic` when crash.rs wrote a crash report for its
/// process, else `native`), its crash report and its run log beside it. Returns the
/// upload, or `None` when the summary cannot be read.
pub fn finish_earlier(summary: &Path, logs: &Path) -> Option<upload::Pending> {
    let name = summary.file_name()?.to_str()?;
    let stem = name.strip_suffix(".json")?;
    let pid = stem.rsplit('-').next()?;
    let dir = summary.parent()?;
    let names: Vec<String> = std::fs::read_dir(logs).map(|r| r.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    let find = |prefix: &str, ext: &str| names.iter().filter(|n| n.starts_with(prefix) && n.ends_with(&format!("-{pid}{ext}"))).max().map(|n| logs.join(n));
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(summary).ok()?).ok()?;
    let crash = find("crash-", ".txt");
    // A crash report makes it a panic even when the game went on and exited normally (a
    // panic on a thread of its own does not stop the game, crash.rs).
    let end = match (&crash, v["end"].as_str()) {
        (Some(_), _) => Some("panic"),
        (None, Some("unknown")) => Some("native"),
        _ => None,
    };
    if let Some(end) = end.filter(|e| v["end"] != *e) {
        v["end"] = serde_json::Value::from(end);
        std::fs::write(summary, v.to_string()).ok()?;
    }
    let panic_part = dir.join(format!("{stem}.panic.txt"));
    if let Some(c) = crash.filter(|_| v["end"] == "panic" && !panic_part.exists()) {
        if let Ok(text) = std::fs::read_to_string(c) {
            let _ = std::fs::write(&panic_part, redact(&text));
        }
    }
    // A native crash's minidump (native_crash.rs), copied as it is: binary, it cannot be
    // redacted here. The server scrubs the user name from its module paths; stack memory can
    // still hold other text, so dumps stay private (player-reports.md §3.2).
    let dump_part = dir.join(format!("{stem}.dmp"));
    if v["end"] == "native" && !dump_part.exists() {
        if let Some(d) = find("crash-", ".dmp") {
            let _ = std::fs::copy(d, &dump_part);
        }
    }
    let snap_part = dir.join(format!("{stem}.snapshot.json"));
    if v["end"] == "panic" && !snap_part.exists() {
        if let Some(s) = find("snapshot-", ".json") {
            let _ = std::fs::copy(s, &snap_part);
        }
    }
    let rec_part = dir.join(format!("{stem}.recorder.jsonl.gz"));
    if !rec_part.exists() {
        if let Some(text) = find("recorder-", ".jsonl").and_then(|r| std::fs::read_to_string(r).ok()) {
            let _ = std::fs::write(&rec_part, upload::gzip(text.as_bytes()));
        }
    }
    let log_part = dir.join(format!("{stem}.log.gz"));
    if !log_part.exists() {
        if let Some(text) = find("nvforever-", ".log").and_then(|l| std::fs::read_to_string(l).ok()) {
            let _ = std::fs::write(&log_part, upload::gzip(cut_log(&redact(&text)).as_bytes()));
        }
    }
    let names: Vec<String> = std::fs::read_dir(dir).map(|r| r.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    upload::pending_in(dir, &names).into_iter().find(|p| p.summary == summary)
}

/// A run log within the cap (§3.1): whole when small, else its first and last lines.
fn cut_log(text: &str) -> std::borrow::Cow<'_, str> {
    // Logs compress about 10:1; the cap is on the compressed size.
    if text.len() <= LOG_CAP_BYTES * 8 {
        return std::borrow::Cow::Borrowed(text);
    }
    let lines: Vec<&str> = text.lines().collect();
    let head = &lines[..LOG_HEAD_LINES.min(lines.len())];
    let tail = &lines[lines.len().saturating_sub(LOG_TAIL_LINES).max(head.len())..];
    std::borrow::Cow::Owned(format!("{}\n[nvforever: {} lines left out]\n{}\n", head.join("\n"), lines.len() - head.len() - tail.len(), tail.join("\n")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_plugins_crc_is_the_zlib_one() {
        let p = std::env::temp_dir().join(format!("nvf-crc-test-{}.esp", std::process::id()));
        std::fs::write(&p, b"123456789").unwrap();
        // The CRC-32 check value of "123456789".
        assert_eq!(super::file_crc32(&p).as_deref(), Some("CBF43926"));
        let _ = std::fs::remove_file(&p);
        assert_eq!(super::file_crc32(&p), None);
    }

    #[test]
    fn nvf_reports_1_never_sends_to_the_project_server_from_a_local_build() {
        use super::{may_send, SendCase};
        let on = SendCase { test_on: true, ..SendCase::default() };
        // The review's case: a local build, NVF_REPORTS=1, no endpoint anywhere.
        assert!(!may_send(on));
        assert!(may_send(SendCase { endpoint_env: true, ..on }));
        assert!(may_send(SendCase { ini_endpoint: true, ..on }));
        // A test run (--data, a test aid) only ever sends to NVF_REPORTS_ENDPOINT.
        let test_run = SendCase { test_run: true, ..on };
        assert!(!may_send(SendCase { ini_endpoint: true, ..test_run }));
        assert!(may_send(SendCase { endpoint_env: true, ..test_run }));
        // Without NVF_REPORTS=1 the local-build gate in `choose` has already decided.
        assert!(may_send(SendCase::default()));
    }

    #[test]
    fn reports_hide_the_game_data_and_exe_folders() {
        // Paths as this platform writes them (a `\` is no separator off Windows).
        let (root, s) = if cfg!(windows) { (r"D:\", r"\") } else { ("/", "/") };
        let p = |parts: &[&str]| format!("{root}{}", parts.join(s));
        let data = p(&["Games", "Alex Smith", "Fallout New Vegas", "Data"]);
        let f = super::folders_to_hide(std::path::Path::new(&data), Some(std::path::Path::new(&p(&["Tools", "NVForever"]))), &[]);
        let r = |t: &str| super::redact_with(t, &f);
        assert_eq!(r(&format!("loaded {data}{s}FalloutNV.esm")), format!("loaded <Data>{s}FalloutNV.esm"));
        // Any case, either separator.
        let game_lower = p(&["games", "alex smith", "fallout new vegas"]).replace('\\', "/");
        assert_eq!(r(&format!("ini at {game_lower}/Fallout.ini")), "ini at <game>/Fallout.ini");
        assert_eq!(r(&format!("exe {}{s}nvforever.exe", p(&["Tools", "NVForever"]))), format!("exe <NVForever>{s}nvforever.exe"));
        assert_eq!(r("nothing here"), "nothing here");
        // A root or a drive is never hidden whole.
        assert!(super::folders_to_hide(std::path::Path::new(&p(&["Data"])), None, &[]).iter().all(|(p, _)| p.len() > 3));
    }

    /// A `--mo2` run hides the instance and its mods and overwrite folders, wherever the
    /// instance's INI puts them (#3152).
    #[test]
    fn reports_hide_the_mo2_folders() {
        let (root, s) = if cfg!(windows) { (r"E:\", r"\") } else { ("/", "/") };
        let p = |parts: &[&str]| format!("{root}{}", parts.join(s));
        let path = |parts: &[&str]| std::path::PathBuf::from(p(parts));
        let instance = path(&["Alex Smith", "MO2"]);
        // The mods folder under the instance, the overwrite folder somewhere else.
        let folders = vec![(instance.clone(), "<MO2>"), (instance.join("mods"), "<MO2 mods>"), (path(&["Alex Smith", "ow"]), "<MO2 overwrite>")];
        let f = super::folders_to_hide(std::path::Path::new(&p(&["Games", "FNV", "Data"])), None, &folders);
        let r = |t: &str| super::redact_with(t, &f);
        assert_eq!(r(&format!("read {}{s}mods{s}A Mod{s}a.esp", instance.display())), format!("read <MO2 mods>{s}A Mod{s}a.esp"));
        assert_eq!(r(&format!("ini {}{s}ModOrganizer.ini", instance.display())), format!("ini <MO2>{s}ModOrganizer.ini"));
        assert_eq!(r(&format!("wrote {}{s}x.txt", p(&["Alex Smith", "ow"]))), format!("wrote <MO2 overwrite>{s}x.txt"));
        assert!(!r(&format!("{} and {}", instance.display(), p(&["Alex Smith", "ow", "y"]))).contains("Alex"));
        // Without a layout only the instance is named.
        assert_eq!(super::mo2_folders(&instance, None), [(instance, "<MO2>")]);
    }

    use super::*;

    #[test]
    fn only_changed_settings_are_reported_never_paths() {
        let ini = "[Game]\nPath=C:\\Games\\FNV\n[Settings]\nmenu-pause=1\nsend-reports=1\n; x=1\n[Text]\nCodePage=1251\n[Reports]\nInstallId=abc\nEndpoint=https://x\n";
        assert_eq!(settings_rows(ini), vec![("Settings.menu-pause".to_string(), "1".to_string()), ("Text.CodePage".to_string(), "1251".to_string())]);
    }

    #[test]
    fn quests_are_named_by_plugin_whatever_the_load_order() {
        let order: Vec<String> = ["FalloutNV.esm", "SomeMod.esp", "deadmoney.esm"].map(String::from).to_vec();
        assert_eq!(official_quest(&order, 0x0010_4C1C), Some(("00104C1C".to_string(), "FalloutNV.esm".to_string())));
        // DeadMoney.esm at index 2 here: the ID is within the plugin, the name as the list spells it.
        assert_eq!(official_quest(&order, 0x0200_0ABC), Some(("00000ABC".to_string(), "DeadMoney.esm".to_string())));
        assert_eq!(official_quest(&order, 0x0100_0001), None, "a mod's quest");
        assert_eq!(official_quest(&order, 0x0700_0001), None, "past the load order");
    }

    #[test]
    fn log_levels_come_from_the_log_section() {
        let ini = "[Settings]
script=debug
[Log]
script=debug
nvf_play::doors=TRACE
ai=loud
; nav=debug
";
        assert_eq!(log_directives(ini), "nvf_play::script=debug,fnv_game=debug,fnv_script=debug,nvf_play::doors=trace");
        assert_eq!(log_directives("[Game]
Path=x
"), "");
    }

    #[test]
    fn minute_times_and_vendors() {
        assert_eq!(iso_minute(1_791_793_805), "2026-10-12T08:30:00Z");
        assert_eq!(gpu_vendor(0x10DE), "nvidia");
        assert_eq!(gpu_vendor(0x1234), "other");
    }

    #[test]
    fn a_long_log_keeps_its_head_and_tail() {
        let text: String = (0..200_000).map(|i| format!("line {i} {}\n", "x".repeat(100))).collect();
        let cut = cut_log(&text);
        assert!(cut.starts_with("line 0 ") && cut.contains("line 199999 ") && cut.contains("lines left out"));
        assert_eq!(cut_log("short\n"), "short\n");
    }

    #[test]
    fn an_earlier_session_cut_short_is_marked_and_given_its_files() {
        let root = std::env::temp_dir().join(format!("nvf-reports-test-{}", std::process::id()));
        let (pending, logs) = (root.join("pending"), root.join("logs"));
        std::fs::create_dir_all(&pending).unwrap();
        std::fs::create_dir_all(&logs).unwrap();
        let s = pending.join("session-20261110-080000-4242.json");
        std::fs::write(&s, r#"{"schema":"nvf-session/1","end":"unknown"}"#).unwrap();
        std::fs::write(logs.join("nvforever-20261110-080000-4242.log"), "INFO a\n").unwrap();
        std::fs::write(logs.join("crash-20261110-081500-4242.txt"), "message: boom\n").unwrap();
        let p = finish_earlier(&s, &logs).unwrap();
        assert_eq!(upload::end_of(&std::fs::read(&s).unwrap()).as_deref(), Some("panic"));
        assert!(p.log.is_some() && p.panic.is_some());
        // Without a crash report: a native crash.
        let s2 = pending.join("session-20261110-090000-77.json");
        std::fs::write(&s2, r#"{"end":"unknown"}"#).unwrap();
        let p2 = finish_earlier(&s2, &logs).unwrap();
        assert_eq!(upload::end_of(&std::fs::read(&s2).unwrap()).as_deref(), Some("native"));
        assert!(p2.panic.is_none() && p2.log.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
