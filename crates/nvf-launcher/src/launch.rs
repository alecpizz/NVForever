//! Starting the game: which program, with which arguments. The launcher passes no `--data`:
//! the game reads its folder from `[Game] Path` of the home's INI like a start without the
//! launcher, so a player's run through the launcher is a player's run in every way.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

pub(crate) fn game_from_ini(text: &str) -> Option<fnv_install::GameInstall> {
    fnv_install::Config::from_ini(text).game_dir.and_then(|path| fnv_install::GameInstall::open(path).ok())
}

/// Apply an edit to the current INI, not an older UI snapshot.
pub(crate) fn write_ini(path: &Path, edit: impl FnOnce(String) -> String) -> Result<(), String> {
    let now = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.to_string()),
    };
    fnv_install::guard_write(path, game_from_ini(&now).as_ref()).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fnv_install::write_whole(path, edit(now).as_bytes()).map_err(|e| e.to_string())
}

#[cfg(windows)]
pub const GAME_EXE: &str = "nvforever.exe";
#[cfg(not(windows))]
pub const GAME_EXE: &str = "nvforever";

/// How the game is started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// The main menu (the same as starting the program with no arguments).
    MainMenu,
    /// The newest save, or the one given.
    Load(PathBuf),
    /// Straight into the opening at Doc Mitchell's.
    NewGame,
    /// The free-camera explorer.
    Explorer,
}

impl Start {
    pub fn args(&self) -> Vec<String> {
        match self {
            Start::MainMenu => vec!["--play".into(), "--menu".into()],
            Start::Load(p) => vec!["--play".into(), "--load".into(), p.display().to_string()],
            Start::NewGame => vec!["--play".into(), "--new-game".into()],
            Start::Explorer => vec!["--explore".into()],
        }
    }
}

/// The game program: `[Launcher] Exe` of the INI when it is a file, else the one beside the
/// launcher (a release archive has both), else a build of this checkout (developers).
pub fn find_game(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = configured.filter(|p| p.is_file()) {
        return Some(p.to_path_buf());
    }
    let here = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let beside = here.join(GAME_EXE);
    if beside.is_file() {
        return Some(beside);
    }
    // target/release/nvforever-launcher.exe beside engine/target/release/nvforever.exe.
    let root = here.ancestors().find(|a| a.join("engine").join("Cargo.toml").is_file())?;
    ["release", "release-quick"].iter().map(|p| root.join("engine").join("target").join(p).join(GAME_EXE)).find(|p| p.is_file())
}

/// Start `exe` with `start`'s arguments, in its own folder (it finds `schemas/` beside itself).
/// `mod_layers` are the enabled mod folders, lowest priority first. The game does not
/// read `mod-order.txt`; these arguments are that list.
pub fn spawn(exe: &Path, start: &Start, mod_layers: &[PathBuf]) -> std::io::Result<Child> {
    let mut c = Command::new(exe);
    c.args(start.args());
    for layer in mod_layers {
        c.arg("--mod-layer");
        c.arg(layer);
    }
    if let Some(dir) = exe.parent() {
        c.current_dir(dir);
    }
    c.spawn()
}

/// The game's folder inside a Steam library.
const STEAM_GAME: [&str; 3] = ["steamapps", "common", "Fallout New Vegas"];

/// Where the game usually is, most likely first, to fill the Game folder box with when
/// none is set [our choice, #2938: Steam's default library, and GOG's default folder on
/// Windows as the game's own folder dialog starts there (engine `setup.rs`)]. `home` is
/// the user's home folder (Linux: Steam keeps its library under it); `windows` picks the
/// platform's list, so both are tested everywhere.
pub fn usual_game_folders(home: Option<&Path>, windows: bool) -> Vec<PathBuf> {
    if windows {
        return vec![PathBuf::from(r"C:\Program Files (x86)\Steam").join(STEAM_GAME.iter().collect::<PathBuf>()), PathBuf::from(r"C:\GOG Games\Fallout New Vegas")];
    }
    let Some(home) = home else { return Vec::new() };
    [".local/share/Steam", ".steam/steam"].iter().map(|s| home.join(s).join(STEAM_GAME.iter().collect::<PathBuf>())).collect()
}

/// The first of `folders` that `usable` accepts (a folder holding the game).
pub fn first_usable(folders: &[PathBuf], usable: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    folders.iter().find(|f| usable(f)).cloned()
}

/// The user's home folder, for [`usual_game_folders`].
pub fn user_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Whether the game would find its folder when started now: the INI names a usable one,
/// Mod Organizer 2 started the launcher (the game then takes MO2's, `fnv_install::mo2`),
/// or the development variable `NVFOREVER_DATA` is set (docs/development.md).
pub fn folder_known(ini_folder_usable: bool, mo2: bool, data_var: Option<&std::ffi::OsStr>) -> bool {
    ini_folder_usable || mo2 || data_var.is_some_and(|v| !v.is_empty())
}

/// What to say when the game has ended: nothing for a clean exit, else the exit code and
/// where its logs are (#2938: a start that fails at once was left as "The game is
/// starting…"). `code` is `None` when the system gave none (ended by a signal).
pub fn exit_message(code: Option<i32>, success: bool, logs: Option<&Path>) -> Option<String> {
    if success {
        return None;
    }
    let how = match code {
        // Windows reports a crash as a large unsigned code (0xC0000005): shown in hex.
        Some(c) if !(-255..=255).contains(&c) => format!("exit code 0x{:08X}", c as u32),
        Some(c) => format!("exit code {c}"),
        None => "no exit code".to_string(),
    };
    Some(match logs {
        Some(l) => format!("The game stopped with an error ({how}). Its logs are in {}.", l.display()),
        None => format!("The game stopped with an error ({how})."),
    })
}

/// Open one of the launcher's own folders with the system's file browser.
pub fn open_folder(dir: &Path) {
    open(&dir.display().to_string());
}

/// Whether `url` may be handed to the system: an `https://` web page and nothing else. URLs
/// come from the network (`news.json`, GitHub), and the system opener runs whatever it is
/// given: a local program, a `\\host\share` path, a `file:` or other protocol handler.
/// Only URL characters are let through (RFC 3986's, no spaces, controls, quotes, backslashes
/// or commas, which explorer reads as its own switches); a page whose address needs others
/// is refused rather than guessed at.
pub fn is_web_url(url: &str) -> bool {
    let u = url.trim();
    let url_char = |c: char| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+;=%".contains(c);
    u.len() > 8 && u.get(..8).is_some_and(|s| s.eq_ignore_ascii_case("https://")) && u.chars().all(url_char)
}

/// Open a web page in the browser; anything but an `https://` URL is refused.
pub fn open_url(url: &str) {
    if is_web_url(url) {
        open(url.trim());
    } else {
        eprintln!("launcher: not opening {url:?}: only https:// pages are opened");
    }
}

fn open(target: &str) {
    #[cfg(windows)]
    let r = Command::new("explorer").arg(target).spawn();
    #[cfg(target_os = "macos")]
    let r = Command::new("open").arg(target).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let r = Command::new("xdg-open").arg(target).spawn();
    if let Err(e) = r {
        eprintln!("launcher: could not open {target}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments() {
        assert_eq!(Start::MainMenu.args(), ["--play", "--menu"]);
        assert_eq!(Start::NewGame.args(), ["--play", "--new-game"]);
        assert_eq!(Start::Explorer.args(), ["--explore"]);
        let a = Start::Load(PathBuf::from("s").join("q.nvfsave")).args();
        assert_eq!(&a[..2], ["--play", "--load"]);
        assert!(a[2].ends_with("q.nvfsave"));
    }

    /// #2938: the Game folder box's first guess.
    #[test]
    fn usual_folders() {
        let home = Path::new("/home/courier");
        let linux = usual_game_folders(Some(home), false);
        assert_eq!(
            linux,
            [home.join(".local/share/Steam/steamapps/common/Fallout New Vegas"), home.join(".steam/steam/steamapps/common/Fallout New Vegas")]
        );
        assert!(usual_game_folders(None, false).is_empty());
        let windows = usual_game_folders(None, true);
        assert_eq!(windows[0], PathBuf::from(r"C:\Program Files (x86)\Steam").join("steamapps").join("common").join("Fallout New Vegas"));
        assert_eq!(windows.len(), 2);
        // The first that holds the game; none when the game is in neither.
        assert_eq!(first_usable(&linux, |p| p.starts_with(home.join(".steam"))), Some(linux[1].clone()));
        assert_eq!(first_usable(&linux, |_| true), Some(linux[0].clone()));
        assert_eq!(first_usable(&linux, |_| false), None);
    }

    /// #2938: when the game would find its folder.
    #[test]
    fn folder_known_cases() {
        use std::ffi::OsStr;
        assert!(!folder_known(false, false, None));
        assert!(!folder_known(false, false, Some(OsStr::new(""))));
        assert!(folder_known(true, false, None));
        assert!(folder_known(false, true, None));
        assert!(folder_known(false, false, Some(OsStr::new("/games/fnv/Data"))));
    }

    /// #2938: a game that ends with an error is reported.
    #[test]
    fn exit_messages() {
        let logs = Path::new("home").join("logs");
        assert_eq!(exit_message(Some(0), true, Some(&logs)), None);
        let m = exit_message(Some(1), false, Some(&logs)).unwrap();
        assert!(m.starts_with("The game stopped with an error (exit code 1). Its logs are in ") && m.contains("logs"), "{m}");
        assert_eq!(exit_message(Some(-1073741819), false, None).unwrap(), "The game stopped with an error (exit code 0xC0000005).");
        assert_eq!(exit_message(None, false, None).unwrap(), "The game stopped with an error (no exit code).");
    }

    #[test]
    fn only_https_pages_are_opened() {
        assert!(is_web_url("https://nv-forever.com/news/070"));
        assert!(is_web_url("HTTPS://github.com/LootJunkieAzza/NVForever/releases/tag/v0.7.0"));
        for bad in [
            "",
            "https://",
            "http://nv-forever.com",
            r"C:\Windows\System32\calc.exe",
            r"\\host\share\x.exe",
            "file:///C:/x.exe",
            "ms-settings:",
            "javascript:alert(1)",
            "https://x.com\" a.exe",
            "https://x.com/a b",
            "https://x.com/a\tb",
            "https://x.com/a,/select,C:\\x.exe",
            "https://x.com/\\x",
            "https://x.com/é",
            "aéééé…",
            "httpsé://x.com",
        ] {
            assert!(!is_web_url(bad), "{bad}");
        }
    }
}
