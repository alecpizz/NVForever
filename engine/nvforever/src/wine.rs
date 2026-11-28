//! Whether the Windows build runs under Wine or Proton (#3493): such a run reports itself
//! as Windows, and its crashes (winit's re-entrant handler under RADV) may be the
//! translation layer's, so the log says so at the start.
//!
//! Wine's `ntdll.dll` exports `wine_get_version`, which Windows' does not [documented:
//! WineHQ Developer FAQ, "How can I detect Wine?"]; it returns Wine's version as a C
//! string. Proton is Wine underneath, so it answers the same way.

/// Wine's version when this Windows build runs under Wine or Proton; `None` on Windows
/// itself and on every other build.
#[cfg(windows)]
pub fn version() -> Option<String> {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    let name: Vec<u16> = "ntdll.dll".encode_utf16().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated wide string; ntdll is loaded in every Windows process, and
    // the handle is not freed (GetModuleHandleW takes no reference).
    let ntdll = unsafe { GetModuleHandleW(name.as_ptr()) };
    if ntdll.is_null() {
        return None;
    }
    // SAFETY: a valid module handle and a NUL-terminated ANSI name.
    let f = unsafe { GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()) }?;
    // SAFETY: Wine's `const char *wine_get_version(void)` [documented: the FAQ above]; the
    // export exists only under Wine, which defines it so.
    let get = unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, unsafe extern "C" fn() -> *const std::ffi::c_char>(f) };
    let p = unsafe { get() };
    if p.is_null() {
        return Some(String::new());
    }
    // SAFETY: a NUL-terminated string Wine keeps for the life of the process.
    Some(unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

#[cfg(not(windows))]
pub fn version() -> Option<String> {
    None
}

/// The log line for a run under Wine: what it is, and that a native Linux build exists.
pub fn describe(version: &str) -> String {
    let v = if version.is_empty() { String::new() } else { format!(" {version}") };
    format!("system: the Windows build is running under Wine{v} (Wine or Proton); the system and driver names reported are the translation layer's, and NVForever has a native Linux build")
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_line_names_wine_and_its_version() {
        assert!(super::describe("9.0").contains("under Wine 9.0 (Wine or Proton)"));
        assert!(super::describe("").contains("under Wine (Wine or Proton)"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_itself_is_not_wine() {
        // The test machines run Windows itself, whose ntdll has no Wine export.
        assert_eq!(super::version(), None);
    }
}
