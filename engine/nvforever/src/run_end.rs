//! Why a run ended, when something outside the game ended it (#2755).
//!
//! `nvforever.exe` is a console program, so Windows gives it a console window of its own
//! when it is started from Explorer or the launcher. Closing that window, Ctrl+C or
//! Ctrl+Break in it, and Windows shutting down, restarting or signing the user out all end
//! the game with no panic and no dump: its run log just stopped [observed: player reports of
//! 0.7.0-0.8.1; one install's runs ended so six times, one of them with the exit watcher's
//! `0xC000013A`, "ended by Ctrl+C or its console was closed"].
//!
//! Two listeners write one line into the run log as it happens, `run log: ended from
//! outside: <why>`:
//! - a console control handler (`SetConsoleCtrlHandler`): the console window closed, and
//!   sign-out and shutdown when Windows sends those to the console [documented: Microsoft,
//!   "HandlerRoutine callback function": a process that loads user32.dll gets no
//!   CTRL_LOGOFF_EVENT or CTRL_SHUTDOWN_EVENT, which is why the window below is there].
//!   Windows calls the handlers last added first [documented: same page], and Bevy adds
//!   its own (the `ctrlc` crate's) while the app is built, so this one is added in a
//!   Startup system, after it ([`console_handler`]). It returns FALSE: the next handler,
//!   Bevy's, and in the end Windows end the process as before. Ctrl+C and Ctrl+Break are
//!   not noted: Bevy's handler closes the game normally on them (its run log says the game
//!   exited normally, the exit watcher code 130 [observed, 2026-11-27]);
//! - a hidden top-level window on a thread of its own, which Windows asks before it ends
//!   the session (`WM_QUERYENDSESSION`, answered yes) and tells when it does
//!   (`WM_ENDSESSION`) [documented: Microsoft, "WM_ENDSESSION message": its lParam says
//!   sign-out (`ENDSESSION_LOGOFF`), or an installer or the Restart Manager closing the
//!   program (`ENDSESSION_CLOSEAPP`); otherwise a shutdown or restart].
//!
//! The next start says it beside its "did not end normally" warning, and the reports'
//! server can split runs that stopped with no dump by the line. The run is still reported
//! as not ended normally: the game did not save or write its summary.
//! `NVF_RUN_END=0` installs neither.

// The listeners are Windows only; elsewhere only `said` (reading an old run log) is used.
#![cfg_attr(not(windows), allow(dead_code))]

/// What the line says before the reason.
pub const LINE: &str = "run log: ended from outside: ";

/// What ended the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outside {
    CtrlC,
    CtrlBreak,
    ConsoleClosed,
    SignOut,
    Shutdown,
    /// An installer or the Restart Manager closing the program.
    CloseApp,
}

impl Outside {
    /// The line's reason: a short key first (what the server splits by), then words.
    pub fn words(self) -> &'static str {
        match self {
            Outside::CtrlC => "ctrl-c (Ctrl+C was pressed in the game's console window)",
            Outside::CtrlBreak => "ctrl-break (Ctrl+Break was pressed in the game's console window)",
            Outside::ConsoleClosed => "console-closed (the game's console window was closed)",
            Outside::SignOut => "sign-out (Windows is signing the user out)",
            Outside::Shutdown => "shutdown (Windows is shutting down or restarting)",
            Outside::CloseApp => "close-app (an installer or Windows' Restart Manager is closing the game)",
        }
    }

    /// A console control event's kind [documented: Microsoft, "HandlerRoutine callback
    /// function": CTRL_C_EVENT 0, CTRL_BREAK_EVENT 1, CTRL_CLOSE_EVENT 2,
    /// CTRL_LOGOFF_EVENT 5, CTRL_SHUTDOWN_EVENT 6].
    pub fn of_console(kind: u32) -> Option<Outside> {
        match kind {
            0 => Some(Outside::CtrlC),
            1 => Some(Outside::CtrlBreak),
            2 => Some(Outside::ConsoleClosed),
            5 => Some(Outside::SignOut),
            6 => Some(Outside::Shutdown),
            _ => None,
        }
    }

    /// `WM_ENDSESSION`'s lParam [documented: Microsoft, "WM_ENDSESSION message":
    /// ENDSESSION_CLOSEAPP 0x1, ENDSESSION_LOGOFF 0x80000000].
    pub fn of_end_session(lparam: isize) -> Outside {
        let l = lparam as usize as u64 & 0xFFFF_FFFF;
        if l & 0x8000_0000 != 0 {
            Outside::SignOut
        } else if l & 0x1 != 0 {
            Outside::CloseApp
        } else {
            Outside::Shutdown
        }
    }
}

/// The line, without the time stamp.
pub fn line(o: Outside) -> String {
    format!("{LINE}{}", o.words())
}

/// What a run log says ended it from outside, `None` when nothing did (the first such line:
/// a console closed during a shutdown is the console).
pub fn said(log_text: &str) -> Option<&str> {
    log_text.lines().find_map(|l| l.split_once(LINE)).map(|(_, rest)| rest.trim())
}

/// Whether a line was written: once a run.
static NOTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Write the line for `o`, once.
fn note(o: Outside) {
    if NOTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    crate::crash::write_now("WARN", "nvforever::run_end", &line(o));
}

fn off() -> bool {
    std::env::var("NVF_RUN_END").is_ok_and(|v| v.trim() == "0")
}

/// Start the session-end window (Windows only). Call after the run log is open.
pub fn install() {
    #[cfg(windows)]
    if !off() {
        win::install();
    }
}

/// Add the console control handler (Windows only): a Startup system, so that it is added
/// after Bevy's own and is called before it.
pub fn console_handler() {
    #[cfg(windows)]
    if !off() {
        win::console_handler();
    }
}

#[cfg(windows)]
mod win {
    use super::{note, Outside};
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, RegisterClassW, TranslateMessage, MSG, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW, WS_OVERLAPPED,
    };

    unsafe extern "system" fn on_console(kind: u32) -> BOOL {
        // Ctrl+C and Ctrl+Break: Bevy's handler, next, closes the game normally.
        if let Some(o) = Outside::of_console(kind).filter(|o| !matches!(o, Outside::CtrlC | Outside::CtrlBreak)) {
            note(o);
        }
        // FALSE: the next handler, in the end Windows' own, ends the process as before.
        0
    }

    unsafe extern "system" fn on_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            // Yes, the session may end: the game holds nothing it could save here.
            WM_QUERYENDSESSION => 1,
            WM_ENDSESSION => {
                if wparam != 0 {
                    note(Outside::of_end_session(lparam));
                }
                0
            }
            // SAFETY: the window's own messages, passed on unchanged.
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    pub fn console_handler() {
        // SAFETY: a handler with the signature Windows calls; it only writes the run log.
        unsafe {
            SetConsoleCtrlHandler(Some(on_console), 1);
        }
    }

    pub fn install() {
        let _ = std::thread::Builder::new().name("nvf-run-end".into()).spawn(|| {
            let class: Vec<u16> = "NVForeverRunEnd\0".encode_utf16().collect();
            // SAFETY: a window class and a never-shown window of this thread, and its
            // message loop; every pointer outlives the calls (`class` lives to the end).
            unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let wc = WNDCLASSW { lpfnWndProc: Some(on_message), hInstance: instance, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
                if RegisterClassW(&wc) == 0 {
                    return;
                }
                let hwnd = CreateWindowExW(0, class.as_ptr(), class.as_ptr(), WS_OVERLAPPED, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null());
                if hwnd.is_null() {
                    return;
                }
                // Test aid: `NVF_PLAY_TEST_END_SESSION=<seconds>` sends this window the
                // WM_ENDSESSION Windows sends at a shutdown (the game goes on: only the
                // line is written), for smoke `run-end-session`.
                if let Some(secs) = std::env::var("NVF_PLAY_TEST_END_SESSION").ok().and_then(|v| v.trim().parse::<f32>().ok()) {
                    let to = hwnd as usize;
                    let _ = std::thread::Builder::new().name("nvf-run-end-test".into()).spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs_f32(secs.max(0.0)));
                        // A message to this process's own window (inside the unsafe block
                        // above); a gone window only makes the call fail.
                        PostMessageW(to as HWND, WM_ENDSESSION, 1, 0);
                    });
                }
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_what_ended_the_run_and_reads_back() {
        assert_eq!(line(Outside::ConsoleClosed), "run log: ended from outside: console-closed (the game's console window was closed)");
        let log = format!("INFO a\n20261127-101500  WARN nvforever::run_end: {}\nINFO b\n", line(Outside::Shutdown));
        assert_eq!(said(&log), Some("shutdown (Windows is shutting down or restarting)"));
        assert_eq!(said("INFO a\n"), None);
    }

    #[test]
    fn console_events_and_end_session_flags_map_to_reasons() {
        assert_eq!(Outside::of_console(2), Some(Outside::ConsoleClosed));
        assert_eq!(Outside::of_console(0), Some(Outside::CtrlC));
        assert_eq!(Outside::of_console(6), Some(Outside::Shutdown));
        assert_eq!(Outside::of_console(3), None);
        assert_eq!(Outside::of_end_session(0), Outside::Shutdown);
        assert_eq!(Outside::of_end_session(0x8000_0000u32 as i32 as isize), Outside::SignOut);
        assert_eq!(Outside::of_end_session(1), Outside::CloseApp);
    }
}
