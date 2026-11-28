//! Why a run ended, when nothing in the run saw it end (#2755; docs/design/player-reports.md
//! §3.2). Thirteen 0.7.0 sessions stopped mid-play with no panic, no minidump and no line:
//! a stack overflow (native_crash.rs's filter returns at once for one), a fail-fast or an
//! abort (no handler runs for those), a filter some driver replaced, or the process ended
//! from outside. The process's **exit code** tells these apart, and only another process
//! can read it.
//!
//! So the game starts a second, tiny process at its start: this same executable with
//! [`ARG`], which does nothing but wait for the game's process to end, read its exit code
//! and, when the code is not 0, add one line to the end of that run's log:
//!
//! ```text
//! 20261116-101500  WARN nvforever::exit_watch: run log: the process ended with exit code 0xC00000FD (stack overflow)
//! ```
//!
//! The line travels with the run log (a player's hand-sent file, and the copy sent with a
//! crash report), and the next start repeats it beside its "previous run did not end
//! normally" warning (crash.rs). A clean exit (code 0) writes nothing: the log is not
//! opened again. Windows only: elsewhere a process cannot wait for one that is not its
//! child, and the Linux signal handler already names the signal (native_crash_unix.rs).
//!
//! The watcher opens the game's process by its id a moment after the game started it. Were
//! the game to end and its id be given to another process within that moment, the watcher
//! would report that process instead [our note: not guarded against].
//!
//! `NVF_EXIT_WATCH=0` starts no watcher.

use std::path::Path;

/// The watcher's first argument; then the process id and the run log's path.
pub const ARG: &str = "--watch-exit";

/// What the watcher's line says before the code.
pub const LINE: &str = "run log: the process ended with exit code ";

/// What an exit code means, in a few words. The `0xC…` values are NTSTATUS codes
/// [documented: Microsoft, ntstatus.h / "NTSTATUS Values"]; a process that dies of an
/// unhandled exception exits with the exception's code.
pub fn words(code: u32) -> &'static str {
    match code {
        0 => "a normal exit",
        // [hypothesis] Task Manager's "End task" and most launchers end a process with 1;
        // so does a program's own error exit.
        1 => "an error exit, or the process was ended from outside (Task Manager ends with 1)",
        // [documented: Rust's std, a panic that unwinds out of `main` exits with 101]
        101 => "a Rust panic in the main thread",
        // [observed, 2026-11-27: Ctrl+Break in the game's console] Bevy's Ctrl+C handler
        // closes the game normally and it exits with 130.
        130 => "Ctrl+C or Ctrl+Break, and the game closed normally",
        0xC000_0005 => "access violation",
        0xC000_0017 => "no memory",
        0xC000_001D => "illegal instruction",
        0xC000_0094 => "integer division by zero",
        0xC000_00FD => "stack overflow",
        0xC000_0135 => "a DLL was not found",
        0xC000_013A => "ended by Ctrl+C or its console was closed",
        0xC000_0142 => "a DLL failed to start",
        0xC000_0374 => "heap corruption",
        // STATUS_STACK_BUFFER_OVERRUN is what `__fastfail` ends a process with, whatever the
        // reason [documented: Microsoft, "__fastfail"]. Rust's `abort` is a fail-fast on
        // Windows, and Rust aborts when an allocation fails or a panic cannot unwind.
        0xC000_0409 => "fail-fast: an abort (out of memory, a panic that could not unwind) or a failed check in a library or driver",
        0xC000_041D => "an exception in a window callback",
        0xE06D_7363 => "an uncaught C++ exception",
        0x4001_0004 => "ended by a debugger",
        _ => "not a code we name",
    }
}

/// The watcher's line, without the time stamp.
pub fn line(code: u32) -> String {
    if code < 0x1_0000 {
        format!("{LINE}{code} ({})", words(code))
    } else {
        format!("{LINE}0x{code:08X} ({})", words(code))
    }
}

/// What a run log says of its process's exit: the text after [`LINE`] (the code and its
/// words), `None` when the watcher wrote nothing.
pub fn said(log_text: &str) -> Option<&str> {
    log_text.lines().rev().find_map(|l| l.split_once(LINE)).map(|(_, rest)| rest.trim())
}

/// `main`'s first step: when this process was started as a watcher, watch and say `true`
/// (the caller ends at once: no game starts).
pub fn run_if_watcher() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(ARG)) {
        return false;
    }
    let pid = args.next().and_then(|p| p.to_str().and_then(|p| p.parse::<u32>().ok()));
    let log = args.next().map(std::path::PathBuf::from);
    if let (Some(pid), Some(log)) = (pid, log) {
        watch(pid, &log);
    }
    true
}

/// Whether `log` is a run log's name in a folder NVForever may write to (the game started
/// us with its own checked path; this is the watcher's own check of what it was handed).
fn may_write(log: &Path) -> bool {
    let named = log.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("nvforever-") && n.ends_with(".log"));
    named && log.is_file() && fnv_install::guard_write(log, None).is_ok()
}

fn watch(pid: u32, log: &Path) {
    if !may_write(log) {
        return;
    }
    #[cfg(windows)]
    if let Some(code) = win::exit_code(pid).filter(|c| *c != 0) {
        append(log, code);
    }
    #[cfg(not(windows))]
    let _ = pid;
}

/// Add the line to the end of the run log (its process is gone: nothing else writes there).
#[cfg_attr(not(windows), allow(dead_code))]
fn append(log: &Path, code: u32) {
    use std::io::Write;
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(log) {
        // A run cut off mid-line: start on a line of our own.
        let _ = writeln!(f, "\n{}  WARN nvforever::exit_watch: {}", crate::crash::stamp(secs), line(code));
    }
}

/// Start the watcher for this process and its run log. Failing costs only the line.
pub fn start(log: &Path) {
    if std::env::var("NVF_EXIT_WATCH").is_ok_and(|v| v.trim() == "0") {
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        /// CREATE_NO_WINDOW [documented: Microsoft, "Process Creation Flags"]: no console.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let Ok(exe) = std::env::current_exe() else { return };
        let _ = Command::new(exe)
            .arg(ARG)
            .arg(std::process::id().to_string())
            .arg(log)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(not(windows))]
    let _ = log;
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, WaitForSingleObject, INFINITE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    /// Wait for process `pid` to end; its exit code. `None` when it cannot be opened.
    pub fn exit_code(pid: u32) -> Option<u32> {
        // SAFETY: plain Win32 calls on a handle opened and closed here.
        unsafe {
            let h = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            WaitForSingleObject(h, INFINITE);
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code) != 0;
            CloseHandle(h);
            ok.then_some(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_the_code_and_reads_back() {
        assert_eq!(line(0xC000_00FD), "run log: the process ended with exit code 0xC00000FD (stack overflow)");
        assert!(line(1).starts_with("run log: the process ended with exit code 1 (an error exit"));
        let log = format!("20261116-101500  INFO nvforever: sky\n\n20261116-101501  WARN nvforever::exit_watch: {}\n", line(0xC000_0409));
        assert!(said(&log).is_some_and(|s| s.starts_with("0xC0000409 (fail-fast")));
        assert_eq!(said("20261116-101500  INFO nvforever: sky\n"), None);
        assert_eq!(words(0xDEAD_BEEF), "not a code we name");
    }

    #[test]
    fn only_a_run_log_is_written_to() {
        let dir = std::env::temp_dir().join(format!("nvf-exit-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (log, other) = (dir.join("nvforever-20261116-101500-7.log"), dir.join("settings.ini"));
        std::fs::write(&log, "first line, cut of").unwrap();
        std::fs::write(&other, "x").unwrap();
        assert!(may_write(&log) && !may_write(&other) && !may_write(&dir.join("nvforever-none.log")));
        append(&log, 0xC000_0005);
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.starts_with("first line, cut of\n"), "{text}");
        assert!(said(&text).is_some_and(|s| s == "0xC0000005 (access violation)"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The watcher's wait reads another process's exit code.
    #[cfg(windows)]
    #[test]
    fn another_processs_exit_code_is_read() {
        let mut child = std::process::Command::new("cmd").args(["/c", "exit 7"]).spawn().unwrap();
        assert_eq!(win::exit_code(child.id()), Some(7));
        let _ = child.wait();
    }
}
