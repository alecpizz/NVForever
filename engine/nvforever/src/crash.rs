//! Crash reports and the run log (owner's playtest 2026-10-11: the game crashed outside
//! Crimson Caravan and nothing was kept to tell why).
//!
//! **The run log.** Everything the game logs (the console's lines, without colours) is
//! also written to `<home>\logs\nvforever-<UTC time>-<pid>.log`, NVForever's own home
//! (`fnv_install::EngineHome`: `Documents\My Games\NVForever` or `NVFOREVER_HOME`), never the
//! game's folder nor `Documents\My Games\FalloutNV` (docs/design/install-layout.md;
//! every path is checked by `fnv_install::guard_write`). Lines are written as they come,
//! so the log holds everything up to a crash. The last [`KEEP_LOGS`] run logs are kept.
//! A run that ends normally writes [`CLEAN_EXIT`] as it exits; at the start the
//! newest earlier log is checked, and one without that line and without a crash report
//! is named in the log: the process ended in a way no Rust code saw (an access violation
//! in a driver, a stack overflow, the process killed), and its log shows what it was
//! doing last.
//!
//! **Crash reports.** A panic on any thread writes `<home>\logs\crash-<UTC time>-<pid>.txt`:
//! the thread, the message and where it was raised, the backtrace, the command line,
//! where the camera was (worldspace or interior, cell, position; [`note_place`], kept
//! every frame) and the name of the run log; the panic is logged too, and the console gets
//! the words Rust's default hook prints, redacted (the default hook is not called: it would
//! print the home folder). If several panics share a second and process id, the first
//! report and its optional sidecars keep that name; later panics are still logged.
//!
//! **Structured events** (events_log.rs): the same events at `info` and up, as JSON lines
//! with where and when they happened, in `events-<UTC time>-<pid>.jsonl` beside the run log.
//!
//! **Native crashes** skip Rust's panic hook; on Windows they leave a minidump
//! (native_crash.rs) and a line in the run log, and the run log check above still names them.
//!
//! **A run that just stops** (no panic, no dump: a stack overflow, an abort, the process
//! ended from outside): a watcher process adds the process's exit code to the end of the run
//! log (exit_watch.rs, #2755), and an allocation that fails says so in the log first
//! ([`SaysWhenEmpty`]).
//!
//! **A lost graphics device** (#2934: the driver stopped answering, or the card was
//! reset). wgpu says so once, and Bevy asks the game what to do at the next frame
//! ([`render_error`]); but the frame in flight goes on drawing with the dead device and
//! usually panics first (a buffer made on it "is invalid"), so the run used to end as that
//! panic. Bevy logs the loss the moment wgpu reports it; the console writer notes that
//! line ([`note_device_lost`]), and from then on a panic is the loss's consequence: its
//! crash report says `graphics device lost` (with the panic under it), the player is told
//! the driver stopped ([`DEVICE_LOST_TEXT`]) and the run ends. Without a panic,
//! [`render_error`] writes the same report and says the same.
//!
//! **Privacy.** Players send these files to us, so the player's home folder (which holds
//! their account name: `C:\Users\<name>`) is written as `%USERPROFILE%` (`~` off Windows)
//! in the run log, the console and crash reports ([`redact`]; owner's request 2026-11-06,
//! #1710).

use bevy::log::tracing_subscriber;
use bevy::log::BoxedLayer;
use bevy::prelude::*;
use std::fs::File;
use std::io::{LineWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Run logs kept in the logs folder, the newest ones [our choice].
pub const KEEP_LOGS: usize = 10;

/// Logged when a run ends normally.
pub const CLEAN_EXIT: &str = "run log: the game exited normally";
/// Written when the game stops before it starts (a bad argument, data that cannot be read).
const ERROR_EXIT: &str = "run log: the game stopped at the start:";

/// The run log, once opened: its path and the writer the log layer takes.
struct RunLog {
    path: PathBuf,
    writer: Mutex<Option<LineWriter<File>>>,
}

static RUN_LOG: OnceLock<RunLog> = OnceLock::new();
/// Where crash reports go.
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
/// The command line, for crash reports.
static COMMAND: OnceLock<String> = OnceLock::new();
/// Where the camera is (updated every frame by [`note_place`]).
static PLACE: Mutex<String> = Mutex::new(String::new());

/// The folders that carry the player's account name, from the home folder: the home
/// folder itself, and with `mounts` (off Windows) where the desktop mounts removable
/// drives under that name, `/run/media/<user>` and `/media/<user>` (a game on such a
/// drive names the player in every path). The one list behind [`redact`], the reports'
/// redaction and the signal handler's (`native_crash_unix`), so the three hide the same
/// (#3152). Empty for a home that is a root or a drive (`/`, `C:`): it would hide every
/// path. The user name is the home folder's last part.
pub(crate) fn name_folders(home: &str, mounts: bool) -> Vec<String> {
    let home = home.trim_end_matches(['\\', '/']);
    if home.len() <= 3 {
        return Vec::new();
    }
    let mut v = vec![home.to_string()];
    let user = home.rsplit('/').next().unwrap_or("");
    if mounts && !user.is_empty() {
        v.push(format!("/run/media/{user}"));
        v.push(format!("/media/{user}"));
    }
    v
}

/// [`name_folders`] of this run's home (`USERPROFILE` on Windows, `HOME` elsewhere) and
/// what they are written as in logs. Read once.
fn hidden_folders() -> &'static (Vec<String>, &'static str) {
    static HIDDEN: OnceLock<(Vec<String>, &'static str)> = OnceLock::new();
    HIDDEN.get_or_init(|| {
        let (var, shown) = if cfg!(windows) { ("USERPROFILE", "%USERPROFILE%") } else { ("HOME", "~") };
        (std::env::var(var).map(|home| name_folders(&home, !cfg!(windows))).unwrap_or_default(), shown)
    })
}

/// `text` with the player's home folder replaced (see the module's Privacy note), and off
/// Windows the removable-drive mounts under the player's name.
pub fn redact(text: &str) -> std::borrow::Cow<'_, str> {
    let (folders, shown) = hidden_folders();
    redact_folders(text, folders, shown)
}

fn redact_folders<'a>(text: &'a str, folders: &[String], shown: &str) -> std::borrow::Cow<'a, str> {
    let mut out = std::borrow::Cow::Borrowed(text);
    for folder in folders {
        if let std::borrow::Cow::Owned(changed) = redact_folder(&out, folder, shown) {
            out = std::borrow::Cow::Owned(changed);
        }
    }
    out
}

/// `text` with every `folder` in it written as `shown`: letters in any case (Windows paths
/// are not case sensitive) and `\` and `/` alike, a run of them counting as one separator
/// (a path printed with `{:?}` doubles its backslashes: `C:\\Users\\Alex`); only where the
/// folder name ends (at a separator or a non-name character), so `C:\Users\Al` leaves
/// `C:\Users\Alex` alone.
pub(crate) fn redact_folder<'a>(text: &'a str, folder: &str, shown: &str) -> std::borrow::Cow<'a, str> {
    let sep = |c: u8| matches!(c, b'\\' | b'/');
    let (t, f) = (text.as_bytes(), folder.as_bytes());
    if f.is_empty() {
        return std::borrow::Cow::Borrowed(text);
    }
    // Where a match of `folder` that starts at `i` ends in `text`, if there is one.
    let match_at = |i: usize| -> Option<usize> {
        let (mut ti, mut fi) = (i, 0);
        while fi < f.len() {
            if sep(f[fi]) {
                if !t.get(ti).is_some_and(|&c| sep(c)) {
                    return None;
                }
                while fi < f.len() && sep(f[fi]) {
                    fi += 1;
                }
                while ti < t.len() && sep(t[ti]) {
                    ti += 1;
                }
            } else {
                if !t.get(ti).is_some_and(|c| c.eq_ignore_ascii_case(&f[fi])) {
                    return None;
                }
                ti += 1;
                fi += 1;
            }
        }
        Some(ti)
    };
    let mut out: Option<String> = None;
    let (mut i, mut copied) = (0, 0);
    while i < t.len() {
        let found = match_at(i).filter(|&end| {
            let ends = t.get(end).is_none_or(|&c| !(c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-') || c >= 0x80));
            ends && text.is_char_boundary(i) && text.is_char_boundary(end)
        });
        match found {
            Some(end) => {
                let o = out.get_or_insert_with(|| String::with_capacity(text.len()));
                o.push_str(&text[copied..i]);
                o.push_str(shown);
                i = end;
                copied = i;
            }
            None => i += 1,
        }
    }
    match out {
        Some(mut o) => {
            o.push_str(&text[copied..]);
            std::borrow::Cow::Owned(o)
        }
        None => std::borrow::Cow::Borrowed(text),
    }
}

/// The logs folder in NVForever's home, created; `None` (with the reason) when there is
/// no home or it may not be written there.
pub(crate) fn logs_dir(data: &Path) -> Result<PathBuf, String> {
    let root = fnv_install::EngineHome::default_root().ok_or("no NVForever home folder (set NVFOREVER_HOME)")?;
    let dir = fnv_install::EngineHome::new(root).logs();
    let game = fnv_install::GameInstall::open(data).ok();
    fnv_install::guard_write(&dir, game.as_ref()).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (H. Hinnant's
/// `civil_from_days`).
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// A UTC time stamp for file names, `YYYYMMDD-HHMMSS`.
pub(crate) fn stamp(unix_secs: u64) -> String {
    let (days, secs) = ((unix_secs / 86_400) as i64, unix_secs % 86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Crash reports kept in the logs folder, the newest ones [our choice].
pub const KEEP_REPORTS: usize = 20;

/// The files named `<prefix>…<ext>` to delete so that `keep` remain, given the names in
/// the folder: they sort by their time stamp, oldest first.
pub(crate) fn old_files(names: &[String], prefix: &str, ext: &str, keep: usize) -> Vec<String> {
    let mut logs: Vec<&String> = names.iter().filter(|n| n.starts_with(prefix) && n.ends_with(ext)).collect();
    logs.sort();
    let n = logs.len().saturating_sub(keep);
    logs.into_iter().take(n).cloned().collect()
}

/// Why the run with process id `pid` stopped at the start, from its run log beside this
/// run's (handoff.rs: a new run that ended before it was ready).
#[cfg(not(target_os = "android"))]
pub fn stopped_at_start(pid: u32) -> Option<String> {
    let dir = LOG_DIR.get()?;
    let tail = format!("-{pid}.log");
    let log = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("nvforever-") && n.ends_with(&tail))
    })?;
    stop_reason(&std::fs::read_to_string(log).ok()?)
}

#[cfg(any(not(target_os = "android"), test))]
fn stop_reason(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.strip_prefix(ERROR_EXIT)).map(|r| r.trim().to_string())
}

/// Whether a run log's text shows a run that did not end normally.
fn ended_abnormally(text: &str) -> bool {
    !text.contains(CLEAN_EXIT) && !text.contains(ERROR_EXIT) && !text.contains("crash report written to")
}

/// Open the run log and set the panic hook. Call first thing in `main`; afterwards the
/// log plugin takes [`file_layer`]. Failing to open the log only costs the file.
/// `previous_running`: the run before this one is still going (it handed a load over to
/// this one, handoff.rs), so its unfinished log is no sign of a crash.
pub fn install(data: &Path, previous_running: bool) {
    let _ = COMMAND.set(std::env::args().collect::<Vec<_>>().join(" "));
    let dir = match logs_dir(data) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("run log: not written ({})", redact(&e));
            set_hook();
            return;
        }
    };
    let _ = LOG_DIR.set(dir.clone());
    // The newest earlier run log, before this one is opened and old ones go.
    let mut names: Vec<String> = std::fs::read_dir(&dir).map(|r| r.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    names.sort();
    let previous = names.iter().rev().find(|n| n.starts_with("nvforever-") && n.ends_with(".log")).map(|n| dir.join(n));
    let run_stamp = stamp(now_secs());
    let path = dir.join(format!("nvforever-{run_stamp}-{}.log", std::process::id()));
    names.push(path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string());
    for old in old_files(&names, "nvforever-", ".log", KEEP_LOGS)
        .into_iter()
        .chain(old_files(&names, "crash-", ".txt", KEEP_REPORTS))
        .chain(old_files(&names, "recorder-", ".jsonl", KEEP_REPORTS))
        .chain(old_files(&names, "snapshot-", ".json", KEEP_REPORTS))
        // The events file of this run is made after this: one fewer old one kept.
        .chain(old_files(&names, "events-", ".jsonl", KEEP_LOGS - 1))
        .chain(old_files(&names, "crash-", ".dmp", crate::native_crash::KEEP_DUMPS))
    {
        let _ = std::fs::remove_file(dir.join(old));
    }
    match File::create(&path) {
        Ok(f) => {
            let _ = RUN_LOG.set(RunLog { path: path.clone(), writer: Mutex::new(Some(LineWriter::new(f))) });
            // How this process ends, when nothing in it sees that (exit_watch.rs), and
            // when something outside ends it (run_end.rs).
            crate::exit_watch::start(&path);
            crate::run_end::install();
        }
        Err(e) => eprintln!("run log: {}: {e}", redact(&path.display().to_string())),
    }
    // A native crash on Linux, where no dump can be written (#2167): a line in the same
    // log. The signal handler is armed from `.init_array` (native_crash_unix.rs, before
    // Rust's std takes SIGSEGV for its own stack-overflow check); this opens the log and
    // the buffers it writes into.
    #[cfg(target_os = "linux")]
    crate::native_crash_unix::install();
    set_hook();
    test_panic();
    crate::native_crash::install(&dir, &run_stamp);
    crate::events_log::install(&dir, &run_stamp);
    // Said once the log plugin is up (`report_start`).
    let unclean = previous.filter(|_| !previous_running).and_then(|p| {
        let text = std::fs::read_to_string(&p).ok().filter(|t| ended_abnormally(t))?;
        let exit = crate::exit_watch::said(&text).map(str::to_string);
        let _ = OUTSIDE.set(crate::run_end::said(&text).map(str::to_string));
        Some((p, exit))
    });
    let _ = UNCLEAN.set(unclean);
}

/// The run before this one, when it did not end normally: its log, and what the log says
/// of its process's exit code (exit_watch.rs).
static UNCLEAN: OnceLock<Option<(PathBuf, Option<String>)>> = OnceLock::new();
/// What the run before this one says ended it from outside (run_end.rs), when it did not
/// end normally.
static OUTSIDE: OnceLock<Option<String>> = OnceLock::new();

/// Lines an allocation failure may write (a loop of failing allocations must not fill the log).
const ALLOC_FAIL_LINES: u32 = 4;
/// Allocations that failed in this run.
static ALLOC_FAILS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The system allocator, which says in the run log when it has no memory to give (#2755).
/// Rust aborts when an allocation fails and its caller cannot go on: on Windows that is a
/// fail-fast, which no exception filter sees (native_crash.rs), so the run log just stopped.
/// Rust's own "memory allocation of N bytes failed" goes to the console only.
pub struct SaysWhenEmpty;

// SAFETY: every call goes to the system allocator unchanged; a null answer is only noted.
unsafe impl std::alloc::GlobalAlloc for SaysWhenEmpty {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: the caller's contract is System's.
        let p = unsafe { std::alloc::System.alloc(layout) };
        if p.is_null() {
            alloc_failed(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: the caller's contract is System's.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: the caller's contract is System's.
        let p = unsafe { std::alloc::System.alloc_zeroed(layout) };
        if p.is_null() {
            alloc_failed(layout.size());
        }
        p
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller's contract is System's.
        let p = unsafe { std::alloc::System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            alloc_failed(new_size);
        }
        p
    }
}

/// An allocation of `size` bytes failed: a line in the run log, written straight to the
/// file. Nothing here allocates (there is no memory), and the log's lock is never waited on
/// (the failing allocation can be the log's own).
fn alloc_failed(size: usize) {
    if ALLOC_FAILS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= ALLOC_FAIL_LINES {
        return;
    }
    let Some(log) = RUN_LOG.get() else { return };
    let Ok(mut w) = log.writer.try_lock() else { return };
    if let Some(f) = w.as_mut() {
        let t = now_secs();
        let (y, m, d) = civil_from_days((t / 86_400) as i64);
        let s = t % 86_400;
        let _ = writeln!(
            f,
            "{y:04}{m:02}{d:02}-{:02}{:02}{:02} ERROR nvforever::crash: out of memory: an allocation of {size} bytes failed (the process aborts when its caller cannot go on without it)",
            s / 3600,
            s / 60 % 60,
            s % 60
        );
        let _ = f.flush();
    }
}

/// A line in the run log at once, written straight to the file and flushed, from any
/// thread, and to the console: for a run that is about to be ended (run_end.rs). The log's lock is waited on
/// for at most 200 ms (Windows gives a closing console about 5 s).
#[cfg_attr(not(windows), allow(dead_code))] // run_end.rs's listeners are Windows only
pub(crate) fn write_now(level: &str, target: &str, text: &str) {
    let line = format!("{}  {level} {target}: {text}", stamp(now_secs()));
    if let Some(log) = RUN_LOG.get() {
        for _ in 0..40 {
            if let Ok(mut w) = log.writer.try_lock() {
                if let Some(f) = w.as_mut() {
                    let _ = writeln!(f, "{line}");
                    let _ = f.flush();
                }
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    // And the console, as the log's other lines (a closed console only fails the write).
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// The words Bevy logs when wgpu reports the device lost, before the reason and wgpu's
/// text [observed: bevy_render 0.19.1 `error_handler.rs`, `Caught DeviceLost error:
/// {reason:?} {str}`; the player report of #2934].
const LOST_LINE: &str = "Caught DeviceLost error:";

/// What the player is told when the graphics device is lost (#2934), as for running out
/// of GPU memory (#2754).
pub const DEVICE_LOST_TEXT: &str = "The graphics driver stopped responding, so NVForever has to close. Update the graphics driver; if it happens again, lower the graphics settings (resolution, shadows, draw distances) and close other programs that use the graphics card.";

/// The lost device's reason and text, once Bevy has logged it.
static DEVICE_LOST: Mutex<Option<String>> = Mutex::new(None);
/// Set by the first thread to end the run for a lost device: it alone tells the player.
static LOST_ENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The reason and text of a lost device in a log line, when the line is Bevy's.
fn lost_in_line(line: &str) -> Option<&str> {
    line.split_once(LOST_LINE).map(|(_, rest)| rest.trim())
}

/// Note a lost device from a log line (the console writer sees every line).
fn note_device_lost(line: &str) {
    if let Some(what) = lost_in_line(line) {
        // Never waits: a line logged while the lock is held is not the first.
        if let Ok(mut lost) = DEVICE_LOST.try_lock() {
            lost.get_or_insert_with(|| if what.is_empty() { "no reason given".to_string() } else { what.to_string() });
        }
    }
}

/// The lost device's reason and text, when the device is lost.
pub fn device_lost() -> Option<String> {
    DEVICE_LOST.try_lock().ok().and_then(|l| l.clone())
}

/// A crash report's message for a lost device: the same words whatever panic followed, so
/// the reports group under the loss.
fn lost_message(what: &str) -> String {
    format!("graphics device lost: {what}")
}

/// A crash report's backtrace part for a lost device: the panic that followed, then its
/// backtrace.
fn lost_backtrace(thread: &str, location: &str, message: &str, backtrace: &str) -> String {
    format!("the panic that followed the loss, on thread '{thread}' at {location}:\n{message}\n\n{backtrace}")
}

/// The flight recorder's last 60 s and the world snapshot beside a crash report, when
/// reports are on (reports/anomaly.rs, reports/snapshot.rs).
fn write_sidecars(dir: &Path, when: &str) {
    if crate::reports::active().is_some() {
        let rec = crate::reports::anomaly::dump_recorder(&dir.join(format!("recorder-{when}-{}.jsonl", std::process::id())));
        let snap = crate::reports::snapshot::dump(&dir.join(format!("snapshot-{when}-{}.json", std::process::id())));
        match (rec.is_ok(), snap.is_ok()) {
            (true, true) => info!("reports: the flight recorder and world snapshot written beside the crash report"),
            (true, false) => info!("reports: the flight recorder written beside the crash report (no world snapshot yet)"),
            (false, true) => info!("reports: the world snapshot written beside the crash report (no flight recorder)"),
            (false, false) => {}
        }
    }
}

/// Test aid `NVF_PLAY_TEST_DEVICE_LOST=<seconds>`: the graphics device is destroyed that
/// long after the start, so what the game does about a lost device can be checked. The
/// frames after it fail as they do after a real loss (the same panic as the player report
/// of #2934) [observed, this aid, GTX 1050]; wgpu 30 does not call the loss callback for a
/// device destroyed this way, so the aid logs the loss in Bevy's words itself.
pub fn test_device_lost(time: Res<Time>, device: Option<Res<bevy::render::renderer::RenderDevice>>, mut at: Local<Option<Option<f32>>>) {
    let wanted = *at.get_or_insert_with(|| std::env::var("NVF_PLAY_TEST_DEVICE_LOST").ok().and_then(|v| v.trim().parse::<f32>().ok()));
    let (Some(secs), Some(device)) = (wanted, device) else { return };
    if time.elapsed_secs() >= secs {
        *at = Some(None);
        warn!("test aid: destroying the graphics device (NVF_PLAY_TEST_DEVICE_LOST)");
        error!("{LOST_LINE} Destroyed by the test aid NVF_PLAY_TEST_DEVICE_LOST");
        device.wgpu_device().destroy();
    }
}

/// Test aid `NVF_PLAY_TEST_PANIC=<seconds>`: a thread of its own ("nvf-test-panic")
/// panics that long after the first frame (when the camera's place is known), so the
/// crash report can be checked; the game goes on (a panic on a thread of its own does
/// not stop it).
fn test_panic() {
    let Some(secs) = std::env::var("NVF_PLAY_TEST_PANIC").ok().and_then(|v| v.trim().parse::<f32>().ok()) else { return };
    let _ = std::thread::Builder::new().name("nvf-test-panic".into()).spawn(move || {
        while PLACE.lock().map(|p| p.is_empty()).unwrap_or(true) {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        std::thread::sleep(std::time::Duration::from_secs_f32(secs.max(0.0)));
        panic!("test panic (NVF_PLAY_TEST_PANIC)");
    });
}

/// The log plugin's extra layers: the same lines as the console, into the run log, and
/// the span profile when it is asked for (profile.rs, `NVF_PLAY_TEST_PROFILE`).
pub fn file_layer(_app: &mut App) -> Option<BoxedLayer> {
    let file = RUN_LOG.get().map(|log| {
        let writer = move || RunLogWriter(log);
        Box::new(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(writer)) as BoxedLayer
    });
    let layers: Vec<BoxedLayer> = file.into_iter().chain(crate::events_log::layer()).chain(crate::profile::layer()).chain(crate::reports::warnings_layer()).collect();
    (!layers.is_empty()).then(|| Box::new(layers) as BoxedLayer)
}

/// The console's lines (the log plugin's own layer, the same but for the home folder,
/// [`redact`]).
pub fn console_layer(_app: &mut App) -> Option<bevy::log::BoxedFmtLayer> {
    Some(Box::new(tracing_subscriber::fmt::Layer::default().with_writer(|| ConsoleWriter)))
}

/// A line to the console, redacted (this crate's `eprintln!`, main.rs).
pub fn eprint_line(args: std::fmt::Arguments<'_>) {
    let _ = writeln!(std::io::stderr(), "{}", redact(&args.to_string()));
}

/// One write to the console, redacted (the layer writes each line whole).
struct ConsoleWriter;

impl Write for ConsoleWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        note_device_lost(&text);
        std::io::stderr().write_all(redact(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}

/// One write to the run log, redacted (the layer makes one per line).
struct RunLogWriter(&'static RunLog);

/// The last line written to the run log without its time stamp, and how many times it came
/// again since (player-reports.md §4.1: a spammed line folded so it cannot push the useful
/// lines out).
static LAST_LINE: Mutex<(String, u64)> = Mutex::new((String::new(), 0));

/// A log line's text without its leading time stamp (`2026-11-10T23:44:31.588916Z  INFO …`).
fn without_stamp(line: &str) -> &str {
    match line.split_once(' ') {
        Some((first, rest)) if first.len() > 10 && first.as_bytes()[0].is_ascii_digit() && first.contains('T') => rest.trim_start(),
        _ => line,
    }
}

/// What to write for `line` given the last one: nothing (a repeat, counted), or the line,
/// after a note of how many times the one before it came again.
fn fold(last: &mut (String, u64), line: &str) -> Option<String> {
    let body = without_stamp(line);
    if !body.trim().is_empty() && body == last.0 {
        last.1 += 1;
        return None;
    }
    let note = (last.1 > 0).then(|| format!("… the line above repeated {} more time{}
", last.1, if last.1 == 1 { "" } else { "s" }));
    last.0 = body.to_string();
    last.1 = 0;
    Some(note.unwrap_or_default() + line)
}

impl Write for RunLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = redact(&String::from_utf8_lossy(buf)).into_owned();
        let Some(out) = fold(&mut LAST_LINE.lock().unwrap_or_else(|e| e.into_inner()), &text) else {
            return Ok(buf.len());
        };
        // A panic while the lock was held must not stop the log.
        let mut w = self.0.writer.lock().unwrap_or_else(|e| e.into_inner());
        match w.as_mut() {
            Some(f) => f.write_all(out.as_bytes()).map(|()| buf.len()),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut w = self.0.writer.lock().unwrap_or_else(|e| e.into_inner());
        w.as_mut().map_or(Ok(()), |f| f.flush())
    }
}

/// A native crash (native_crash.rs): a line in the run log naming the exception and the
/// dump, written straight to the file (the log plugin may be where it crashed). Never waits
/// on the log's lock: a crash while it was held writes nothing rather than hang.
// Called only by native_crash.rs's Windows filter.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn native_crash_line(code: u32, dump: Option<&Path>) {
    let dump = dump.map_or_else(|| "no dump written".to_string(), |p| format!("dump written to {}", redact(&p.display().to_string())));
    let line = format!("native crash, exception 0x{code:08X}; {dump}");
    // The console too (stderr is not locked by the log plugin): the smoke runner reads it.
    let _ = writeln!(std::io::stderr(), "{line}");
    let Some(log) = RUN_LOG.get() else { return };
    let Ok(mut w) = log.writer.try_lock() else { return };
    if let Some(f) = w.as_mut() {
        // A line still being repeated when the crash came: its count, which `fold` writes
        // only when a different line arrives. (A run killed from outside still loses it.)
        if let Ok(mut last) = LAST_LINE.try_lock() {
            if last.1 > 0 {
                let _ = writeln!(f, "… the line above repeated {} more time{}", last.1, if last.1 == 1 { "" } else { "s" });
                last.1 = 0;
            }
        }
        let _ = writeln!(f, "{} ERROR nvforever::crash: {line}", stamp(now_secs()));
        crate::events_log::flush();
        let _ = f.flush();
    }
}

/// The game stops before the log plugin is up: the reason goes into the run log.
pub fn early_exit(why: &str) {
    let Some(log) = RUN_LOG.get() else { return };
    let mut w = log.writer.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(f) = w.as_mut() {
        let _ = writeln!(f, "{ERROR_EXIT} {}", redact(why));
    }
}

/// A reason the game stops at the start that the player must see (a damaged archive): a
/// message box before the game ends, as there is no window yet. None in unattended runs
/// (a test aid drives them; they would wait on it) or in a handoff's new run, whose old
/// run shows the reason it reads from this run's log (handoff.rs).
pub fn tell_player_at_start(text: &str, handoff: bool) {
    if handoff || std::env::vars_os().any(|(k, _)| k.to_str().is_some_and(nvf_play::drives_run)) {
        return;
    }
    // rfd is a Windows-only dependency; elsewhere the error line carries the reason.
    #[cfg(windows)]
    rfd::MessageDialog::new().set_title("NVForever").set_level(rfd::MessageLevel::Error).set_description(text).show();
    #[cfg(not(windows))]
    let _ = text;
}

/// The run log's path, when there is one.
pub fn run_log_path() -> Option<&'static Path> {
    RUN_LOG.get().map(|l| l.path.as_path())
}

/// The text of a crash report.
fn report_text(thread: &str, message: &str, location: &str, place: &str, command: &str, run_log: Option<&Path>, backtrace: &str) -> String {
    format!(
        "NVForever {} crashed (panic)\ntime (UTC): {}\nthread: {thread}\nmessage: {message}\nat: {location}\nwhere: {}\ncommand line: {command}\nrun log: {}\n\nbacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        stamp(now_secs()),
        if place.is_empty() { "not known yet (before the first frame)" } else { place },
        run_log.map_or_else(|| "none".to_string(), |p| p.display().to_string()),
    )
}

/// Write one panic report and its optional sidecars. `false` means an earlier report
/// already owns this name; its evidence must stay together.
fn write_panic_report(path: &Path, text: &str, sidecars: impl FnOnce()) -> std::io::Result<bool> {
    let result = match File::options().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(redact(text).as_bytes()),
        // Reserve the report name before collecting sidecars: concurrent or reentrant
        // panics must not replace the report or the context belonging to it.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.is_file() => return Ok(false),
        Err(e) => Err(e),
    };
    // As before, try the sidecars even if the text could not be written.
    sidecars();
    result?;
    Ok(true)
}

fn set_hook() {
    std::panic::set_hook(Box::new(move |info| {
        // A broken model, texture or animation: caught, and that file counts as failed
        // (nvf_assets::guard). Logged, not a crash.
        if nvf_assets::guard::in_guard() {
            let message = nvf_assets::guard::message(info.payload());
            let location = info.location().map_or_else(|| "unknown".to_string(), |l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
            warn!("a broken file panicked while loading at {location}: {message}; that file is skipped");
            return;
        }
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed").to_string();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(not text)".to_string());
        let location = info.location().map_or_else(|| "unknown".to_string(), |l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let place = PLACE.try_lock().map(|p| p.clone()).unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        // After a lost graphics device a panic is its consequence (module notes): the
        // report is the loss's, with the panic under it.
        let lost = device_lost();
        if let Some(what) = &lost {
            // Another thread is already ending the run for it (the player may be reading
            // its message): this one waits for the process to end.
            if LOST_ENDING.load(std::sync::atomic::Ordering::SeqCst) {
                loop {
                    std::thread::park();
                }
            }
            error!("render: the graphics device was lost ({what}); the panic below is its consequence and the game stops");
        }
        let text = match &lost {
            Some(what) => {
                report_text("the graphics driver", &lost_message(what), "reported by wgpu", &place, COMMAND.get().map_or("", |s| s.as_str()), run_log_path(), &lost_backtrace(&thread, &location, &message, &backtrace))
            }
            None => report_text(&thread, &message, &location, &place, COMMAND.get().map_or("", |s| s.as_str()), run_log_path(), &backtrace),
        };
        let written = LOG_DIR.get().map(|dir| {
            let when = stamp(now_secs());
            let path = dir.join(format!("crash-{when}-{}.txt", std::process::id()));
            let result = write_panic_report(&path, &text, || write_sidecars(dir, &when));
            (result, path)
        });
        let place = if place.is_empty() { "not known yet" } else { place.as_str() };
        match &written {
            Some((Ok(true), p)) => error!("panic on thread '{thread}' at {location}: {message}; where: {place}; crash report written to {}", p.display()),
            Some((Ok(false), p)) => error!("panic on thread '{thread}' at {location}: {message}; where: {place}; earlier crash report preserved at {}", p.display()),
            Some((Err(e), p)) => error!("panic on thread '{thread}' at {location}: {message}; the crash report could not be written to {}: {e}", p.display()),
            None => error!("panic on thread '{thread}' at {location}: {message} (no logs folder for a crash report)"),
        }
        crate::events_log::flush();
        if let Some(l) = RUN_LOG.get() {
            if let Some(f) = l.writer.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                let _ = f.flush();
            }
        }
        // What Rust's default hook prints to the console, redacted: the default hook itself
        // would print the message and backtrace with the home folder in them. Kept (rather
        // than only the `error!` line) because it says the panic before the log plugin is
        // up, and the smoke runner looks for "panicked" in the console.
        eprint_line(format_args!("thread '{thread}' panicked at {location}:\n{message}"));
        if std::env::var_os("RUST_BACKTRACE").is_some_and(|v| v != "0") {
            eprint_line(format_args!("stack backtrace:\n{backtrace}"));
        } else {
            eprint_line(format_args!("note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace"));
        }
        // A lost device: the player is told why the game closes, and it closes here (the
        // frame cannot be finished, and the threads still drawing would only panic too).
        if lost.is_some() && !LOST_ENDING.swap(true, std::sync::atomic::Ordering::SeqCst) {
            tell_player_at_start(DEVICE_LOST_TEXT, false);
            std::process::exit(1);
        }
    }));
}

/// The crash report of a lost device that no panic followed ([`render_error`]): the same
/// message as the panic hook's, so both group together.
fn write_lost_report(what: &str) {
    let place = PLACE.try_lock().map(|p| p.clone()).unwrap_or_default();
    let text = report_text("the graphics driver", &lost_message(what), "reported by wgpu", &place, COMMAND.get().map_or("", |s| s.as_str()), run_log_path(), "none: no panic followed the loss");
    let Some(dir) = LOG_DIR.get() else { return };
    let when = stamp(now_secs());
    let path = dir.join(format!("crash-{when}-{}.txt", std::process::id()));
    match write_panic_report(&path, &text, || write_sidecars(dir, &when)) {
        Ok(true) => error!("render: crash report written to {}", path.display()),
        Ok(false) => error!("render: earlier crash report preserved at {}", path.display()),
        Err(e) => error!("render: the crash report could not be written to {}: {e}", path.display()),
    }
}

/// Say where the run log is, and whether the run before this one ended abnormally.
pub fn report_start(mut commands: Commands) {
    // After Bevy's own console handler, so that it is called first (run_end.rs).
    crate::run_end::console_handler();
    match run_log_path() {
        Some(p) => info!("run log: {}", p.display()),
        None => info!("run log: none"),
    }
    if let Some(p) = crate::events_log::path() {
        info!("events log: {}", p.display());
    }
    if let Some(Some((p, exit))) = UNCLEAN.get() {
        warn!("run log: the previous run did not end normally and left no crash report (a native crash, or the process was ended): {}", p.display());
        match exit {
            Some(e) => warn!("run log: the previous run's process ended with exit code {e}"),
            None => warn!("run log: the previous run's exit code was not recorded"),
        }
        if let Some(Some(why)) = OUTSIDE.get() {
            warn!("run log: the previous run was ended from outside: {why}");
        }
    }
    commands.insert_resource(bevy::render::error_handler::RenderErrorHandler(render_error));
}

/// GPU validation errors let through before the game stops after all.
const VALIDATION_ERRORS_LET_THROUGH: u32 = 20;

/// What the game does on a GPU error. Bevy's own choice is to close the game on any;
/// the images made from files are checked before they reach the GPU
/// (nvf_assets::texture::checked_image), so a validation error left is most likely one
/// broken file's: it is logged and the game goes on. One that keeps coming back stops
/// the game as before, rather than flash the screen frame after frame; so do running out
/// of GPU memory and a lost device.
fn render_error(error: &bevy::render::error_handler::RenderError, main: &mut World, _: &mut World) -> bevy::render::error_handler::RenderErrorPolicy {
    use bevy::render::error_handler::{ErrorType, RenderErrorPolicy};
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEEN: AtomicU32 = AtomicU32::new(0);
    if matches!(error.ty, ErrorType::Validation) {
        let n = SEEN.fetch_add(1, Ordering::Relaxed) + 1;
        if n <= VALIDATION_ERRORS_LET_THROUGH {
            warn!("render: GPU validation error {n} of {VALIDATION_ERRORS_LET_THROUGH} let through (most likely a broken file): {}", error.description);
            return RenderErrorPolicy::Ignore;
        }
    }
    error!("render: {:?} GPU error, the game stops: {}", error.ty, error.description);
    if matches!(error.ty, ErrorType::OutOfMemory) {
        // The player must see why the game closes (#2754); none in unattended runs.
        tell_player_at_start("The graphics card ran out of memory, so NVForever has to close. Lower the graphics settings (resolution, shadows, draw distances) and close other programs that use the graphics card.", false);
    }
    if matches!(error.ty, ErrorType::DeviceLost) && !LOST_ENDING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        // No panic came first (#2934): the loss gets its crash report here, and the
        // player is told. Bevy asks again every frame until the game has stopped; once.
        let what = device_lost().unwrap_or_else(|| error.description.clone());
        write_lost_report(&what);
        tell_player_at_start(DEVICE_LOST_TEXT, false);
    }
    main.write_message(AppExit::error());
    RenderErrorPolicy::StopRendering
}

/// Keep where the camera is for a crash report: worldspace or interior, cell, position.
pub fn note_place(cam: Query<&GlobalTransform, With<crate::fly::FlyCam>>, streamer: Res<crate::world::Streamer>, world: Res<crate::world::WorldData>, mut last: Local<String>) {
    let Ok(cam) = cam.single() else { return };
    let g = crate::world::bevy_to_game(cam.translation());
    let space = match streamer.interior {
        Some(c) => format!("interior {}", world.index.cells.get(&c).and_then(|c| c.edid.clone()).unwrap_or_else(|| format!("{c:08X}"))),
        None => {
            let (x, y) = fnv_world::cell_of(g.x, g.y);
            let name = world.grid_cell((x, y)).and_then(|c| c.edid.clone()).map(|n| format!(" {n}")).unwrap_or_default();
            format!("{} cell {x},{y}{name}", world.index.worldspaces.get(&world.world).and_then(|w| w.edid.clone()).unwrap_or_default())
        }
    };
    let text = format!("{space}, position {:.0}, {:.0}, {:.0}", g.x, g.y, g.z);
    if *last != text {
        if let Ok(mut p) = PLACE.try_lock() {
            p.clone_from(&text);
        }
        *last = text;
    }
}

/// Where the camera is, as a crash report would say (the stress tests' samples).
pub fn place_text() -> String {
    PLACE.try_lock().map(|p| p.clone()).unwrap_or_default()
}

/// `; where: <place>` for a log line, or nothing before the camera has a place.
pub fn place_text_suffix() -> String {
    let p = place_text();
    if p.is_empty() {
        p
    } else {
        format!("; where: {p}")
    }
}

/// Log [`CLEAN_EXIT`] when the run ends normally.
pub fn report_exit(mut exit: MessageReader<AppExit>) {
    if exit.read().next().is_some() {
        if let Some(n) = crate::events_log::flush() {
            info!("events log: {n} events written");
        }
        info!("{CLEAN_EXIT}");
        crate::events_log::flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ReportDir(PathBuf);

    impl ReportDir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!("nvf-panic-report-test-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for ReportDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// #2934: Bevy's line for a lost device is recognised, and its report names the loss
    /// whatever panic followed.
    #[test]
    fn a_lost_device_is_read_from_bevys_line_and_named_in_the_report() {
        let line = "2026-11-17T19:42:37.625Z ERROR bevy_render::error_handler: Caught DeviceLost error: Unknown Unexpected error variant (driver implementation is at fault)\n";
        assert_eq!(lost_in_line(line), Some("Unknown Unexpected error variant (driver implementation is at fault)"));
        assert_eq!(lost_in_line("ERROR bevy_render::error_handler: Caught rendering error: Validation Error"), None);
        assert_eq!(lost_in_line("ERROR bevy_render::view::window: Couldn't get swap chain texture: Validation"), None);
        let what = lost_in_line(line).unwrap();
        let panic_a = lost_backtrace("Compute Task Pool (7)", "device.rs:72:18", "Failed to get mapped range for buffer", "frames");
        let panic_b = lost_backtrace("main", "other.rs:1:1", "another consequence", "frames");
        let a = report_text("the graphics driver", &lost_message(what), "reported by wgpu", "cell", "command", None, &panic_a);
        let b = report_text("the graphics driver", &lost_message(what), "reported by wgpu", "cell", "command", None, &panic_b);
        // The lines a report is grouped by are the loss's, the same for both panics.
        let head = |t: &str| t.lines().filter(|l| l.starts_with("thread: ") || l.starts_with("message: ") || l.starts_with("at: ")).map(str::to_string).collect::<Vec<_>>();
        assert_eq!(head(&a), ["thread: the graphics driver", "message: graphics device lost: Unknown Unexpected error variant (driver implementation is at fault)", "at: reported by wgpu"]);
        assert_eq!(head(&a), head(&b));
        // The panic is kept under it.
        assert!(a.contains("the panic that followed the loss, on thread 'Compute Task Pool (7)' at device.rs:72:18:\nFailed to get mapped range for buffer\n\nframes"));
        assert!(DEVICE_LOST_TEXT.contains("graphics driver stopped responding") && DEVICE_LOST_TEXT.contains("Update the graphics driver"));
    }

    #[test]
    fn same_second_panics_preserve_the_first_report_and_sidecars() {
        let dir = ReportDir::new();
        let path = dir.0.join("crash-20261116-120000-42.txt");
        let recorder = dir.0.join("recorder-20261116-120000-42.jsonl");
        let snapshot = dir.0.join("snapshot-20261116-120000-42.json");
        let first = report_text("render worker", "first panic", "first.rs:1:1", "cell", "command", None, "first backtrace");
        let later = report_text("readback worker", "later panic", "later.rs:2:2", "cell", "command", None, "later backtrace");
        let write_sidecars = |text: &str| {
            std::fs::write(&recorder, text).unwrap();
            std::fs::write(&snapshot, text).unwrap();
        };
        assert!(write_panic_report(&path, &first, || write_sidecars("first context")).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), redact(&first));
        assert!(!write_panic_report(&path, &later, || write_sidecars("later context")).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), redact(&first));
        assert_eq!(std::fs::read_to_string(&recorder).unwrap(), "first context");
        assert_eq!(std::fs::read_to_string(&snapshot).unwrap(), "first context");
        // A later second still gets its own report with the established name.
        let next = dir.0.join("crash-20261116-120001-42.txt");
        assert!(write_panic_report(&next, &later, || {}).unwrap());
        assert_eq!(std::fs::read_to_string(next).unwrap(), redact(&later));
    }

    #[test]
    fn concurrent_panics_keep_one_complete_report_with_its_sidecars() {
        let dir = ReportDir::new();
        let path = dir.0.join("crash-20261116-120000-42.txt");
        let sidecar = dir.0.join("snapshot-20261116-120000-42.json");
        let barrier = std::sync::Barrier::new(4);
        let winners = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4).map(|i| {
                let (path, sidecar, barrier) = (&path, &sidecar, &barrier);
                scope.spawn(move || {
                    let text = format!("panic {i}\n{}", "backtrace\n".repeat(1000));
                    barrier.wait();
                    write_panic_report(path, &text, || std::fs::write(sidecar, &text).unwrap()).unwrap()
                })
            }).collect();
            handles.into_iter().map(|h| usize::from(h.join().unwrap())).sum::<usize>()
        });
        assert_eq!(winners, 1);
        let report = std::fs::read_to_string(&path).unwrap();
        assert_eq!(report, std::fs::read_to_string(sidecar).unwrap());
        assert!((0..4).any(|i| report == format!("panic {i}\n{}", "backtrace\n".repeat(1000))));
    }

    #[test]
    fn a_reentrant_report_attempt_preserves_the_reserved_report() {
        let dir = ReportDir::new();
        let path = dir.0.join("crash-20261116-120000-42.txt");
        assert!(write_panic_report(&path, "first", || {
            assert!(!write_panic_report(&path, "nested", || panic!("a collision must not collect sidecars again")).unwrap());
        }).unwrap());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "first");
    }

    #[test]
    fn a_report_io_error_is_not_a_preserved_report() {
        let dir = ReportDir::new();
        let absent = dir.0.join("missing").join("crash-20261116-120000-42.txt");
        let mut collected = false;
        assert_eq!(write_panic_report(&absent, "panic", || collected = true).unwrap_err().kind(), std::io::ErrorKind::NotFound);
        assert!(collected, "failure to write the text must still try to collect the sidecars");
        // A directory occupying the name is an error, not an earlier report.
        let sidecar = dir.0.join("snapshot-20261116-120000-42.json");
        assert!(write_panic_report(&dir.0, "panic", || std::fs::write(&sidecar, "context").unwrap()).is_err());
        assert_eq!(std::fs::read_to_string(sidecar).unwrap(), "context");
    }

    /// #2755: an allocation the system cannot give is noted (and, with a run log, written).
    #[test]
    fn a_failed_allocation_is_noted() {
        let before = ALLOC_FAILS.load(std::sync::atomic::Ordering::Relaxed);
        let layout = std::alloc::Layout::from_size_align(isize::MAX as usize - 4095, 4096).unwrap();
        // SAFETY: a non-zero size; the null answer is not used.
        let p = unsafe { std::alloc::GlobalAlloc::alloc(&SaysWhenEmpty, std::hint::black_box(layout)) };
        assert!(p.is_null(), "no system gives half the address space");
        assert!(ALLOC_FAILS.load(std::sync::atomic::Ordering::Relaxed) > before);
    }

    /// #2203: a system reading AppExit in Last after `ExitSystems` sees the exit Bevy writes
    /// when the last window closes (here: no window at all) in that same, final frame.
    #[test]
    fn exit_readers_see_the_window_closed_exit() {
        #[derive(Resource, Default)]
        struct Seen(bool);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::window::WindowPlugin { primary_window: None, ..default() }))
            .init_resource::<Seen>()
            .add_systems(Last, (|mut exit: MessageReader<AppExit>, mut seen: ResMut<Seen>| seen.0 |= exit.read().next().is_some()).after(bevy::window::ExitSystems));
        app.update();
        assert!(app.world().resource::<Seen>().0, "the exit written by exit_on_all_closed was missed");
    }

    #[test]
    fn repeated_lines_are_folded() {
        let mut last = (String::new(), 0);
        assert_eq!(fold(&mut last, "2026-11-10T23:44:31.1Z  WARN x: spam
").as_deref(), Some("2026-11-10T23:44:31.1Z  WARN x: spam
"));
        assert_eq!(fold(&mut last, "2026-11-10T23:44:31.2Z  WARN x: spam
"), None);
        assert_eq!(fold(&mut last, "2026-11-10T23:44:31.3Z  WARN x: spam
"), None);
        assert_eq!(
            fold(&mut last, "2026-11-10T23:44:32.0Z  INFO y: other
").as_deref(),
            Some("… the line above repeated 2 more times
2026-11-10T23:44:32.0Z  INFO y: other
")
        );
    }

    #[test]
    fn stamps_are_utc_calendar_times() {
        assert_eq!(stamp(0), "19700101-000000");
        // 2026-10-12 08:30:05 UTC.
        assert_eq!(stamp(1_791_793_805), "20261012-083005");
        // A leap day.
        assert_eq!(stamp(951_782_400), "20000229-000000");
    }

    #[test]
    fn the_oldest_run_logs_go_and_nothing_else() {
        let names: Vec<String> =
            ["nvforever-20261012-080000-1.log", "crash-20261001-000000-5.txt", "nvforever-20261010-120000-9.log", "notes.txt", "nvforever-20261011-090000-3.log"].map(String::from).to_vec();
        assert_eq!(old_files(&names, "nvforever-", ".log", 2), vec!["nvforever-20261010-120000-9.log".to_string()]);
        assert!(old_files(&names, "nvforever-", ".log", 10).is_empty());
        assert_eq!(old_files(&names, "crash-", ".txt", 0), vec!["crash-20261001-000000-5.txt".to_string()]);
    }

    #[test]
    fn a_log_without_the_exit_line_or_a_crash_report_ended_abnormally() {
        assert!(!ended_abnormally(&format!("line\n{CLEAN_EXIT}\n")));
        assert!(!ended_abnormally("line\nERROR panic on thread 'main' at x: boom; crash report written to C:\\x\n"));
        assert!(ended_abnormally("line\nINFO streaming cell -2,26\n"));
        assert_eq!(stop_reason(&format!("a\n{ERROR_EXIT} --load: x was made with other plugins\n")).as_deref(), Some("--load: x was made with other plugins"));
        assert_eq!(stop_reason("a\nb\n"), None);
    }

    /// The removable-drive mounts under the player's name are hidden with the home
    /// folder, from one list (#3152).
    #[test]
    fn the_mounts_under_the_players_name_are_hidden() {
        let folders = name_folders("/home/alex/", true);
        assert_eq!(folders, ["/home/alex", "/run/media/alex", "/media/alex"]);
        let r = |t: &'static str| redact_folders(t, &folders, "~").into_owned();
        assert_eq!(r("panicked at /run/media/alex/USB/nvf/src/main.rs:3"), "panicked at ~/USB/nvf/src/main.rs:3");
        assert_eq!(r("data /media/alex/Games/Data and /home/alex/x"), "data ~/Games/Data and ~/x");
        // Another account, and a name that only starts the same, are left alone.
        assert_eq!(r("/media/bob/x /run/media/alexandra/x"), "/media/bob/x /run/media/alexandra/x");
        assert!(matches!(redact_folders("no paths here", &folders, "~"), std::borrow::Cow::Borrowed(_)));
        // On Windows only the home folder; a root or a drive hides nothing.
        assert_eq!(name_folders(r"C:\Users\Alex", false), [r"C:\Users\Alex"]);
        assert!(name_folders("/", true).is_empty() && name_folders("C:", false).is_empty());
    }

    #[test]
    fn the_home_folder_is_hidden_in_any_case_and_slash() {
        let r = |t: &'static str| redact_folder(t, r"C:\Users\Alex", "%USERPROFILE%").into_owned();
        assert_eq!(r(r"run log: C:\Users\Alex\Documents\My Games\NVForever\logs\a.log"), r"run log: %USERPROFILE%\Documents\My Games\NVForever\logs\a.log");
        assert_eq!(r("data c:/users/ALEX/Games and C:\\Users\\Alex"), r"data %USERPROFILE%/Games and %USERPROFILE%");
        // Another account whose name starts the same is left alone.
        assert_eq!(r(r"C:\Users\Alexandra\x"), r"C:\Users\Alexandra\x");
        assert!(matches!(redact_folder("no paths here", r"C:\Users\Alex", "~"), std::borrow::Cow::Borrowed(_)));
        // A path printed with `{:?}` (doubled backslashes), and one with a stray double slash.
        let debug = format!("{:?}", std::path::Path::new(r"C:\Users\Alex\Documents\a.ini"));
        assert_eq!(redact_folder(&debug, r"C:\Users\Alex", "%USERPROFILE%"), r#""%USERPROFILE%\\Documents\\a.ini""#);
        assert_eq!(r("C:\\\\Users\\\\Alexandra"), "C:\\\\Users\\\\Alexandra");
        assert_eq!(r("c://users//alex//x"), "%USERPROFILE%//x");
        // Not ASCII: matched byte for byte, and the text around it kept whole.
        assert_eq!(redact_folder("é /home/Zoë/x é", "/home/Zoë", "~"), "é ~/x é");
    }

    #[test]
    fn a_report_has_thread_message_place_and_backtrace() {
        let t = report_text("Compute Task Pool (3)", "index out of bounds", "src/x.rs:1:2", "WastelandNV cell -2,26 CrimsonCaravan, position 1, 2, 3", "nvforever --play", Some(Path::new("C:/h/logs/run.log")), "0: main");
        for want in ["thread: Compute Task Pool (3)", "message: index out of bounds", "at: src/x.rs:1:2", "where: WastelandNV cell -2,26 CrimsonCaravan", "run log: C:/h/logs/run.log", "backtrace:\n0: main"] {
            assert!(t.contains(want), "{want} in {t}");
        }
        assert!(report_text("main", "m", "l", "", "", None, "").contains("where: not known yet"));
    }
}
