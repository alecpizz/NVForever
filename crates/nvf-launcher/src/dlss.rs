//! The DLSS card of Settings, Graphics (#2765): what DLSS needs and whether this PC has it,
//! NVIDIA's licence, and the game's own Download DLSS… (`nvf-dlss-files`: the eight files
//! from NVIDIA's Streamline release, pinned and signature-checked, only after the player
//! accepted the licence). Nothing goes online before I accept is clicked.

use nvf_dlss_files::download::{Phase, Progress};
use nvf_dlss_files::{FG_FILES, FILES};
use std::path::{Path, PathBuf};

/// What DLSS needs, as this PC has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub windows: bool,
    /// The graphics cards Windows names (the adapters of the attached displays).
    pub gpus: Vec<String>,
    /// The DLSS folder the game reads and the download writes, or why there is none.
    pub folder: Result<PathBuf, String>,
    /// How many of the four upscaling files, and of the four frame generation files, it holds.
    pub upscaling: usize,
    pub frame_generation: usize,
}

impl Status {
    /// `game`: the program the launcher starts; the files go beside it, where it reads them.
    pub fn read(game: Option<&Path>) -> Status {
        let folder = folder_for(game);
        let (upscaling, frame_generation) = folder.as_ref().map_or((0, 0), |f| (present(f, &FILES), present(f, &FG_FILES)));
        Status { windows: cfg!(windows), gpus: gpu_names(), folder, upscaling, frame_generation }
    }

    /// An NVIDIA RTX card is among the graphics cards (the game asks the driver itself at start;
    /// this is the name only).
    pub fn rtx(&self) -> bool {
        self.gpus.iter().any(|g| is_rtx(g))
    }

    /// Every one of the eight files is there.
    pub fn complete(&self) -> bool {
        self.upscaling == FILES.len() && self.frame_generation == FG_FILES.len()
    }
}

/// The DLSS folder the game reads: `dlss` beside `game` (`NVF_DLSS_DIR` when set), else beside
/// the launcher (a release's layout: the game beside it). A source checkout's launcher is in
/// `target/release` and its game in `engine/target/release`: beside the launcher, the game
/// would never see the files.
pub fn folder_for(game: Option<&Path>) -> Result<PathBuf, String> {
    match game.and_then(Path::parent) {
        Some(dir) => nvf_dlss_files::download::target_folder_beside(Some(dir)),
        None => nvf_dlss_files::download::target_folder(),
    }
}

/// Whether a graphics card's name is an NVIDIA RTX one.
pub fn is_rtx(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("nvidia") && n.contains("rtx")
}

/// How many of `names` are files in `folder`.
pub fn present(folder: &Path, names: &[&str]) -> usize {
    names.iter().filter(|n| folder.join(n).is_file()).count()
}

/// The download's step in words, and how far it is (0..1) when it knows.
pub fn progress_words(p: &Progress) -> (String, Option<f32>) {
    let mb = |b: u64| b as f32 / 1_048_576.0;
    let part = (p.to_fetch > 0).then(|| (p.fetched as f32 / p.to_fetch as f32).clamp(0.0, 1.0));
    let words = match &p.phase {
        Phase::Connecting => "Asking GitHub for NVIDIA's release…".to_string(),
        Phase::Directory => "Reading the release's list of files…".to_string(),
        Phase::File { name, index, of } => format!("{name} ({index} of {of}): {:.1} of {:.1} MB", mb(p.fetched), mb(p.to_fetch)),
        Phase::Checking => "Checking the files' signatures…".to_string(),
        Phase::Installing => "Moving the files into the dlss folder…".to_string(),
        Phase::Done(folder) => format!("DLSS is ready in {}: the game uses it at its next start.", folder.display()),
        Phase::Failed(f) => format!("The download failed, nothing was written: {f}."),
        Phase::Cancelled => "Cancelled, nothing was written.".to_string(),
    };
    (words, part)
}

/// The graphics cards of the displays attached to the desktop (EnumDisplayDevicesW's
/// DeviceString), each once.
#[cfg(windows)]
// The Win32 calls (#4592): the crate denies unsafe code everywhere else.
#[allow(unsafe_code)]
pub fn gpu_names() -> Vec<String> {
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP};
    let mut out: Vec<String> = Vec::new();
    for i in 0.. {
        // SAFETY: DISPLAY_DEVICEW is plain data; all zeros is valid and cb says its size.
        let mut dd: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        // SAFETY: a null device asks for the adapters' outputs; dd is a valid, sized struct.
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut dd, 0) } == 0 {
            break;
        }
        if dd.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0 {
            continue;
        }
        let w = &dd.DeviceString;
        let name = String::from_utf16_lossy(&w[..w.iter().position(|c| *c == 0).unwrap_or(w.len())]).trim().to_string();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

#[cfg(not(windows))]
pub fn gpu_names() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvf_dlss_files::download::Failure;

    /// The files go beside the game the launcher starts, not beside the launcher: a source
    /// checkout's launcher (target/release) starts engine/target/release's game.
    #[cfg(windows)]
    #[test]
    fn the_folder_is_beside_the_game() {
        if std::env::var_os(nvf_dlss_files::ENV_DIR).is_some_and(|v| !v.is_empty()) {
            return;
        }
        let release = Path::new("C:/").join("nvf").join("engine").join("target").join("release");
        assert_eq!(folder_for(Some(&release.join("nvforever.exe"))).unwrap(), release.join("dlss"));
        assert_eq!(folder_for(None), nvf_dlss_files::download::target_folder());
    }

    #[test]
    fn rtx_cards_by_name() {
        assert!(is_rtx("NVIDIA GeForce RTX 4070"));
        assert!(is_rtx("NVIDIA RTX A2000"));
        assert!(!is_rtx("NVIDIA GeForce GTX 1080"));
        assert!(!is_rtx("AMD Radeon RX 7900 XTX"));
        let s = Status { windows: true, gpus: vec!["Intel(R) UHD Graphics".into(), "NVIDIA GeForce RTX 3060".into()], folder: Err(String::new()), upscaling: 4, frame_generation: 3 };
        assert!(s.rtx() && !s.complete());
    }

    #[test]
    fn files_present_in_a_folder() {
        let d = std::env::temp_dir().join(format!("nvf-launcher-dlss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("sl.interposer.dll"), b"x").unwrap();
        std::fs::write(d.join("nvngx_dlss.dll"), b"x").unwrap();
        assert_eq!(present(&d, &FILES), 2);
        assert_eq!(present(&d, &FG_FILES), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn progress_in_words() {
        let p = |phase, fetched, to_fetch| Progress { phase, fetched, to_fetch };
        let (w, part) = progress_words(&p(Phase::File { name: "nvngx_dlss.dll", index: 4, of: 8 }, 1_048_576 * 5, 1_048_576 * 20));
        assert_eq!(w, "nvngx_dlss.dll (4 of 8): 5.0 of 20.0 MB");
        assert_eq!(part, Some(0.25));
        assert_eq!(progress_words(&p(Phase::Connecting, 0, 0)).1, None);
        assert!(progress_words(&p(Phase::Failed(Failure::Http(404)), 0, 0)).0.contains("nothing was written: the server answered HTTP 404"));
    }
}
