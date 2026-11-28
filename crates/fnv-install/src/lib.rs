//! Keeps NVForever and the original game apart.
//!
//! Policy (docs/design/install-layout.md):
//! - The original game install is **read-only input**. NVForever never writes into it,
//!   never runs from inside it, and never touches the original game's per-user folder
//!   (`Documents\My Games\FalloutNV`: its INIs and saves) or its plugin list
//!   (`%LOCALAPPDATA%\FalloutNV\plugins.txt`).
//! - Everything NVForever writes (settings, saves, logs, its own mods) lives under its own
//!   **home** directory, chosen by `NVFOREVER_HOME` or a platform default (Windows:
//!   `Documents\My Games\NVForever`); caches go to the local data folder.
//! - The game's location is kept in the home's `NVForever.ini`; before a run,
//!   [`GameInstall::missing_files`] names any base game file NVForever needs that is absent.
//! - Which plugins load, and in what order, is NVForever's own `plugins.txt` in the home
//!   ([`plugins`]), never the original game's list; a start from Mod Organizer 2 takes the
//!   list of MO2's profile instead ([`mo2`]).
//!
//! Path containment is checked on canonicalized paths, case-insensitively on Windows.

pub mod mo2;
pub mod mod_order;
pub mod plugins;
pub mod reports;
pub mod steam;

use serde::Deserialize;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// The settings file in the home: the game's location (owner request 2026-10-16: an INI in
/// `My Games\NVForever`).
pub const CONFIG_FILE: &str = "NVForever.ini";
/// The settings file of earlier builds (home `%LOCALAPPDATA%\NVForever`), read when there
/// is no INI yet.
pub const LEGACY_CONFIG_FILE: &str = "nvforever.json";
pub const HOME_ENV: &str = "NVFOREVER_HOME";

/// Files in the game's `Data` folder NVForever cannot run without: the base master, the
/// archives of the game's `SArchiveList` (`fnv_vfs::DEFAULT_ARCHIVE_LIST`, [observed]
/// identical in `Fallout_default.ini` and the owner's `Fallout.ini`) and `Update.bsa`
/// [observed in the owner's patched install, 1.4.0.525; it overrides the base archives'
/// meshes and textures]. The add-on masters are optional ([`ADDON_MASTERS`]).
pub const REQUIRED_DATA_FILES: [&str; 8] = [
    "FalloutNV.esm",
    "Fallout - Textures.bsa",
    "Fallout - Textures2.bsa",
    "Fallout - Meshes.bsa",
    "Fallout - Voices1.bsa",
    "Fallout - Sound.bsa",
    "Fallout - Misc.bsa",
    "Update.bsa",
];

/// The add-on masters NVForever loads when they are present (`fnv_vfs::DEFAULT_PLUGINS`
/// after `FalloutNV.esm`).
pub const ADDON_MASTERS: [&str; 9] = [
    "DeadMoney.esm",
    "HonestHearts.esm",
    "OldWorldBlues.esm",
    "LonesomeRoad.esm",
    "GunRunnersArsenal.esm",
    "ClassicPack.esm",
    "MercenaryPack.esm",
    "TribalPack.esm",
    "CaravanPack.esm",
];

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// The message embeds the io::Error, so it is not declared as #[source].
    #[error("{}: {}", .0.display(), .1)]
    Io(PathBuf, std::io::Error),
    #[error("{} is not a Fallout: New Vegas install (expected Data\\FalloutNV.esm inside it)", .0.display())]
    NotAGameInstall(PathBuf),
    #[error("NVForever and the original game overlap:{}", .0.iter().map(|x| format!("\n  - {x}")).collect::<String>())]
    Overlap(Vec<Violation>),
    #[error("refusing to write {}: {reason}", path.display())]
    WriteDenied { path: PathBuf, reason: String },
    #[error("config error: {0}")]
    Config(String),
    #[error("{} is missing game files NVForever needs:{}", .0.display(), .1.iter().map(|x| format!("\n  - Data\\{x}")).collect::<String>())]
    MissingFiles(PathBuf, Vec<String>),
}

/// The user's original game install. Opened read-only; NVForever never writes here.
#[derive(Debug, Clone)]
pub struct GameInstall {
    pub root: PathBuf,
    pub data: PathBuf,
}

impl GameInstall {
    /// Accepts either the install root or its `Data` folder.
    /// `Data` and `FalloutNV.esm` are found whatever their case, as a copy on a
    /// case-sensitive file system may name them (`data/falloutnv.esm`; #995).
    pub fn open(path: impl AsRef<Path>) -> Result<GameInstall, InstallError> {
        let p = canonical(path.as_ref())?;
        let has_master = |d: &Path| child_ci(d, "FalloutNV.esm").is_some_and(|f| f.is_file());
        let (root, data) = match child_ci(&p, "Data").filter(|d| d.is_dir() && has_master(d)) {
            Some(d) => (p.clone(), d),
            None if has_master(&p) => (p.parent().map(Path::to_path_buf).unwrap_or_else(|| p.clone()), p.clone()),
            None => return Err(InstallError::NotAGameInstall(p)),
        };
        Ok(GameInstall { root, data })
    }

    pub fn contains(&self, path: &Path) -> bool {
        is_within(path, &self.root)
    }

    /// The file in `Data` whose name matches `name` regardless of case (the game's own
    /// names are mixed case; a copy on a case-sensitive file system may differ).
    pub fn data_file(&self, name: &str) -> Option<PathBuf> {
        child_ci(&self.data, name).filter(|p| p.is_file())
    }

    /// The [`REQUIRED_DATA_FILES`] that are absent or empty, in that list's order.
    pub fn missing_files(&self) -> Vec<String> {
        REQUIRED_DATA_FILES
            .iter()
            .filter(|n| self.data_file(n).and_then(|p| p.metadata().ok()).is_none_or(|m| m.len() == 0))
            .map(|n| n.to_string())
            .collect()
    }

    /// `Ok` when every required file is there, else [`InstallError::MissingFiles`].
    pub fn check_files(&self) -> Result<(), InstallError> {
        let missing = self.missing_files();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(InstallError::MissingFiles(self.root.clone(), missing))
        }
    }

    /// The [`ADDON_MASTERS`] present in `Data`.
    pub fn addon_masters(&self) -> Vec<&'static str> {
        ADDON_MASTERS.into_iter().filter(|n| self.data_file(n).is_some()).collect()
    }
}

/// NVForever's own writable directory tree.
#[derive(Debug, Clone)]
pub struct EngineHome {
    pub root: PathBuf,
    /// Generated data that can be made again (generated LOD): `<root>\cache`, except for
    /// the per-user default on Windows, where it stays in the local data folder so that
    /// gigabytes of cache never land in (possibly synced) Documents.
    pub cache_root: PathBuf,
}

/// The per-user local data directory (`directories` crate): Windows
/// `%LOCALAPPDATA%\NVForever` (the Local AppData known folder), macOS
/// `~/Library/Application Support/NVForever`, other Unix `$XDG_DATA_HOME/nvforever` or
/// `~/.local/share/nvforever`. The home of earlier builds on Windows.
fn local_data_dir() -> Option<PathBuf> {
    let name = if cfg!(any(windows, target_os = "macos")) { "NVForever" } else { "nvforever" };
    directories::BaseDirs::new().map(|d| d.data_local_dir().join(name))
}

fn home_env_set() -> bool {
    std::env::var_os(HOME_ENV).is_some_and(|s| !s.is_empty())
}

impl EngineHome {
    /// `NVFOREVER_HOME` if set; else on Windows `Documents\My Games\NVForever` (the
    /// Documents known folder, so a redirected Documents is followed; beside the original
    /// game's `My Games\FalloutNV`, never inside it), elsewhere the local data directory.
    pub fn default_root() -> Option<PathBuf> {
        if let Some(h) = std::env::var_os(HOME_ENV).filter(|s| !s.is_empty()) {
            return Some(PathBuf::from(h));
        }
        if cfg!(windows) {
            directories::UserDirs::new().and_then(|u| u.document_dir().map(|d| d.join("My Games").join("NVForever")))
        } else {
            local_data_dir()
        }
    }

    /// The home this user runs with: [`EngineHome::default_root`], with the cache in the
    /// local data folder on Windows unless `NVFOREVER_HOME` chose the home.
    pub fn user() -> Option<EngineHome> {
        let root = EngineHome::default_root()?;
        let cache_root = match local_data_dir() {
            Some(l) if cfg!(windows) && !home_env_set() => l.join("cache"),
            _ => root.join("cache"),
        };
        Some(EngineHome { root, cache_root })
    }

    /// The home of earlier builds (`%LOCALAPPDATA%\NVForever`) on Windows when
    /// `NVFOREVER_HOME` is not set: its settings and saves are brought over once
    /// ([`EngineHome::adopt_legacy`]).
    pub fn legacy() -> Option<EngineHome> {
        if home_env_set() || !cfg!(windows) {
            return None;
        }
        local_data_dir().map(EngineHome::new)
    }

    /// A home with everything under `root` (tests, `--home`, `NVFOREVER_HOME`).
    pub fn new(root: impl Into<PathBuf>) -> EngineHome {
        let root = root.into();
        EngineHome { cache_root: root.join("cache"), root }
    }

    /// `NVForever.ini` in the home. When there is no file of exactly that name but one
    /// whose name differs only in case (`nvforever.ini`, made by hand on a case-sensitive
    /// file system), that file is the one read and written, as the game's own files are
    /// found ([`child_ci`], #995). Every reader and writer of the INI goes through here.
    pub fn config_file(&self) -> PathBuf {
        child_ci(&self.root, CONFIG_FILE).unwrap_or_else(|| self.root.join(CONFIG_FILE))
    }
    pub fn saves(&self) -> PathBuf {
        self.root.join("saves")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    /// Where F12 saves its shots (`nvforever-NNN.png`, [`first_unused`]) [#1987].
    pub fn screenshots(&self) -> PathBuf {
        self.root.join("screenshots")
    }
    pub fn cache(&self) -> PathBuf {
        self.cache_root.clone()
    }
    /// NVForever's own mod folder (plugins/archives/loose files layered over the
    /// read-only game Data). Mods are never installed into the game's Data folder.
    pub fn mods(&self) -> PathBuf {
        self.root.join("mods")
    }

    pub fn subdirs(&self) -> [PathBuf; 4] {
        [self.saves(), self.logs(), self.cache(), self.mods()]
    }

    pub fn create(&self) -> Result<(), InstallError> {
        for d in std::iter::once(self.root.clone()).chain(self.subdirs()) {
            guard_write(&d, None)?;
            std::fs::create_dir_all(&d).map_err(|e| InstallError::Io(d.clone(), e))?;
        }
        Ok(())
    }

    /// First run of a new home: when it has no settings file yet, take the game's
    /// location from `old`'s settings and copy `old`'s saves that are not here already
    /// (copies; the old folder is left as it was). Returns the names of the saves copied.
    pub fn adopt_legacy(&self, old: &EngineHome) -> Result<Vec<String>, InstallError> {
        if is_within(&self.root, &old.root) || is_within(&old.root, &self.root) || self.config_file().exists() {
            return Ok(Vec::new());
        }
        // Written even without a game folder (`Path=` empty, asked for at the start): the
        // INI marks the home as set up, so saves are copied once and a save deleted here
        // later does not come back.
        Config::load(old)?.save(self, None)?;
        let mut copied = Vec::new();
        let Ok(entries) = std::fs::read_dir(old.saves()) else { return Ok(copied) };
        for e in entries.flatten() {
            let from = e.path();
            let to = self.saves().join(e.file_name());
            if !from.is_file() || to.exists() {
                continue;
            }
            guard_write(&to, None)?;
            std::fs::create_dir_all(self.saves()).map_err(|err| InstallError::Io(self.saves(), err))?;
            std::fs::copy(&from, &to).map_err(|err| InstallError::Io(to.clone(), err))?;
            copied.push(e.file_name().to_string_lossy().into_owned());
        }
        copied.sort();
        Ok(copied)
    }
}

/// The original game's per-user folder (INIs, saves; its plugins.txt is in
/// `%LOCALAPPDATA%\FalloutNV` [observed]). NVForever must not
/// read or write it. Windows: `Documents\My Games\FalloutNV` [documented: community/xEdit
/// docs; verify on the user's PC], where Documents is the Documents known folder, so a
/// Documents folder redirected (e.g. to OneDrive or another drive) is followed.
pub fn original_user_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        directories::UserDirs::new()
            .and_then(|u| u.document_dir().map(|d| d.join("My Games").join("FalloutNV")))
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation(pub String);

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The original game's load-order folder, `%LOCALAPPDATA%\FalloutNV` (its `plugins.txt`
/// [observed]). NVForever's own list has the same file name, so a home pointed here
/// would write over the original's [#1987]. NVForever reads MO2's copy there
/// (`mo2::plugins_file`) and never writes it.
pub fn original_load_order_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        directories::BaseDirs::new().map(|d| d.data_local_dir().join("FalloutNV"))
    } else {
        None
    }
}

/// The original game's folders outside its install: its user folder and its load order's.
fn original_dirs() -> impl Iterator<Item = PathBuf> {
    [original_user_dir(), original_load_order_dir()].into_iter().flatten()
}

/// The first `<dir>\<stem>NNN.<ext>` (from 001) that does not exist yet: a screenshot that
/// keeps the earlier runs' [#1987].
pub fn first_unused(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    (1u32..)
        .map(|n| dir.join(format!("{stem}{n:03}.{ext}")))
        .find(|p| !p.exists())
        .expect("a free number")
}

/// All the ways the two installs could interact. Empty = cleanly separated.
pub fn check_separation(home: &EngineHome, game: &GameInstall, exe: Option<&Path>) -> Vec<Violation> {
    let mut v = Vec::new();
    if is_within(&home.root, &game.root) {
        v.push(Violation(format!(
            "NVForever home {} is inside the game install {}",
            home.root.display(),
            game.root.display()
        )));
    }
    if is_within(&game.root, &home.root) {
        v.push(Violation(format!(
            "the game install {} is inside the NVForever home {}",
            game.root.display(),
            home.root.display()
        )));
    }
    if let Some(e) = exe {
        if is_within(e, &game.root) {
            v.push(Violation(format!(
                "the NVForever executable {} is inside the game install; install it in its own folder",
                e.display()
            )));
        }
    }
    for u in original_dirs() {
        if is_within(&home.root, &u) || is_within(&u, &home.root) {
            v.push(Violation(format!(
                "NVForever home {} overlaps the original game's user folder {}",
                home.root.display(),
                u.display()
            )));
        }
    }
    v
}

/// The game install this process reads, set once at start-up ([`remember_game`]).
static GAME_IN_USE: std::sync::OnceLock<GameInstall> = std::sync::OnceLock::new();

/// Remember the game install this process reads, so that [`guard_write`] refuses writes
/// into it even where the caller has no [`GameInstall`] at hand (a save, a test aid's
/// output). The first call wins; later ones are ignored.
pub fn remember_game(game: &GameInstall) {
    let _ = GAME_IN_USE.set(game.clone());
}

/// [`remember_game`] for the install whose `Data` folder (or root) is `data`, when it
/// opens; a folder that is not a game install is ignored.
pub fn remember_game_at(data: &Path) {
    if let Ok(g) = GameInstall::open(data) {
        remember_game(&g);
    }
}

/// The entry `name` of the folder `dir`, matched regardless of case: the exact name when
/// it exists, else the first entry whose name equals it ignoring ASCII case. `None` when
/// neither exists. The game's own names are mixed case; a copy on a case-sensitive file
/// system may differ (#995).
pub fn child_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name)).map(|e| e.path())
}

/// Gatekeeper for every file NVForever writes: never into `game`, the remembered game
/// install ([`remember_game`]) or the original game's user folder.
pub fn guard_write(path: &Path, game: Option<&GameInstall>) -> Result<(), InstallError> {
    for g in [game, GAME_IN_USE.get()].into_iter().flatten() {
        if g.contains(path) {
            return Err(InstallError::WriteDenied {
                path: path.to_path_buf(),
                reason: format!("it is inside the original game install {}", g.root.display()),
            });
        }
    }
    for u in original_dirs() {
        if is_within(path, &u) {
            return Err(InstallError::WriteDenied {
                path: path.to_path_buf(),
                reason: format!("it is inside the original game's user folder {}", u.display()),
            });
        }
    }
    Ok(())
}

/// The home's settings, `NVForever.ini`:
///
/// ```ini
/// [Game]
/// Path=F:\SteamLibrary\steamapps\common\Fallout New Vegas
///
/// [Text]
/// CodePage=1251
/// ```
///
/// `Path` is the original game's install folder (the one holding `Data`), read-only input.
/// `CodePage` is the encoding of the game's text (`fnv_util::text`, #1439): 1252
/// (the default, English installs), 1251 (Russian), 1250 (Central European), or
/// 936 (GB18030/GBK for simplified Chinese). [`Config::save`] preserves this setting.
/// Saving changes only the keys it owns and keeps every other line, so the file can carry
/// comments and settings of later builds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    /// The original game's install folder (read-only input).
    pub game_dir: Option<PathBuf>,
    /// `[Text] CodePage`, as written (checked where it is used); None: not set.
    pub code_page: Option<String>,
    /// `[Interface] CjkFontPath`, absolute or relative to the INI.
    pub cjk_font_path: Option<PathBuf>,
}

/// `nvforever.json` of earlier builds; unknown keys are ignored.
#[derive(Deserialize)]
struct LegacyConfig {
    #[serde(default)]
    game_dir: Option<PathBuf>,
}

const INI_HEADER: &str = "; NVForever settings. Path is your Fallout: New Vegas folder (the one holding Data);\n; change it here, or delete this file to be asked again at the next start.\n; [design, #2215] Pip-Boy pages use their menu files by default.\n; To use hand-built Pip-Boy pages, add a [UI] section with an empty TileMenus= line.\n";

impl Config {
    /// The INI, else the `nvforever.json` of an earlier build in the same folder, else
    /// nothing set.
    pub fn load(home: &EngineHome) -> Result<Config, InstallError> {
        let p = home.config_file();
        match std::fs::read_to_string(&p) {
            Ok(t) => return Ok(Config::from_ini(&t)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(InstallError::Io(p, e)),
        }
        let legacy = home.root.join(LEGACY_CONFIG_FILE);
        let text = match std::fs::read_to_string(&legacy) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(InstallError::Io(legacy, e)),
        };
        let c: LegacyConfig =
            serde_json::from_str(&text).map_err(|e| InstallError::Config(format!("{}: {e}", legacy.display())))?;
        Ok(Config {
            game_dir: c.game_dir,
            ..Config::default()
        })
    }

    pub fn from_ini(text: &str) -> Config {
        Config {
            game_dir: ini_get(text, "Game", "Path").filter(|s| !s.is_empty()).map(PathBuf::from),
            code_page: ini_get(text, "Text", "CodePage").filter(|s| !s.is_empty()),
            cjk_font_path: ini_get(text, "Interface", "CjkFontPath")
                .map(|path| path.trim_matches('"').trim().to_owned())
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
        }
    }

    /// `text` (an existing INI, or empty) with this config's keys set.
    pub fn to_ini(&self, text: &str) -> String {
        let base = if text.trim().is_empty() { INI_HEADER.to_string() } else { text.to_string() };
        let path = self.game_dir.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        ini_set(&base, "Game", "Path", &path)
    }

    pub fn save(&self, home: &EngineHome, game: Option<&GameInstall>) -> Result<(), InstallError> {
        let p = home.config_file();
        guard_write(&p, game)?;
        let old = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(InstallError::Io(p, e)),
        };
        std::fs::create_dir_all(&home.root).map_err(|e| InstallError::Io(home.root.clone(), e))?;
        write_whole(&p, self.to_ini(&old).as_bytes())
    }
}

/// Write a small text file whole or not at all: beside its place as `<name>.tmp`,
/// flushed, then renamed over the old one. A crash or a full disk mid-write leaves the
/// old file, not a cut one (a cut plugins.txt would come back with the lost plugins
/// switched off, #1982). The caller has passed `path` through [`guard_write`].
pub fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), InstallError> {
    use std::io::Write;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let io = |e| InstallError::Io(path.to_path_buf(), e);
    {
        let mut f = std::fs::File::create(&tmp).map_err(io)?;
        f.write_all(bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    std::fs::rename(&tmp, path).map_err(io)
}

/// The name of an INI line's `[section]`, if it is one.
fn ini_section(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[').and_then(|r| r.strip_suffix(']')).map(str::trim)
}

/// `key=value` of an INI line that is not a comment, both trimmed.
fn ini_pair(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    if t.starts_with(';') || t.starts_with('#') {
        return None;
    }
    t.split_once('=').map(|(k, v)| (k.trim(), v.trim()))
}

/// Every `key=value` line of `[section]` (the section's name compared without case), in
/// file order, comments left out; a key given twice is listed twice. The launcher lists
/// the `[Settings]` rows with it.
pub fn ini_pairs(text: &str, section: &str) -> Vec<(String, String)> {
    let mut in_section = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(s) = ini_section(line) {
            in_section = s.eq_ignore_ascii_case(section);
        } else if in_section {
            if let Some((k, v)) = ini_pair(line) {
                out.push((k.to_string(), v.to_string()));
            }
        }
    }
    out
}

/// The value of `key` in `[section]` (names compared without case); the first one wins.
pub fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        if let Some(s) = ini_section(line) {
            in_section = s.eq_ignore_ascii_case(section);
        } else if in_section {
            if let Some((_, v)) = ini_pair(line).filter(|(k, _)| k.eq_ignore_ascii_case(key)) {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// `text` with `key=value` in `[section]`: the first such line replaced, else the key added
/// after the section's last line, else the section added at the end. Other lines are kept.
pub fn ini_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let entry = format!("{key}={value}");
    let mut in_section = false;
    let mut last_in_section: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(s) = ini_section(line) {
            in_section = s.eq_ignore_ascii_case(section);
            if in_section {
                last_in_section = Some(i);
            }
        } else if in_section {
            if ini_pair(line).is_some_and(|(k, _)| k.eq_ignore_ascii_case(key)) {
                lines[i] = entry;
                return lines.join("\n") + "\n";
            }
            if !line.trim().is_empty() {
                last_in_section = Some(i);
            }
        }
    }
    match last_in_section {
        Some(i) => lines.insert(i + 1, entry),
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
            lines.push(entry);
        }
    }
    lines.join("\n") + "\n"
}

/// The canonical path, without Windows' verbatim prefix where the plain form means the
/// same (`\\?\C:\x` → `C:\x`, `\\?\UNC\srv\x` → `\\srv\x`): it is shown to the player and
/// stored in the INI.
fn canonical(p: &Path) -> Result<PathBuf, InstallError> {
    std::fs::canonicalize(p).map(|c| plain(&c)).map_err(|e| InstallError::Io(p.to_path_buf(), e))
}

fn plain(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') && rest.len() < 260 => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

/// Best-effort absolute, normalized form even for paths that do not exist yet:
/// canonicalize the longest existing ancestor and append the rest lexically.
pub fn normalize(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map(|c| c.join(p)).unwrap_or_else(|_| p.to_path_buf())
    };
    let mut existing = abs.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(c) = std::fs::canonicalize(&existing) {
            let mut out = c;
            for t in tail.iter().rev() {
                out.push(t);
            }
            return lexical(&out);
        }
        // Walk up by component, not file_name(): file_name() is None for a path that
        // ends in `..`, which used to drop to the uncanonicalized path, so a symlinked
        // base (macOS /var -> /private/var) no longer matched its canonical form.
        let last = match existing.components().next_back() {
            Some(c @ (Component::Normal(_) | Component::ParentDir | Component::CurDir)) => c.as_os_str().to_os_string(),
            _ => return lexical(&abs),
        };
        match existing.parent() {
            Some(parent) => {
                tail.push(last);
                existing = parent.to_path_buf();
            }
            None => return lexical(&abs),
        }
    }
}

fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn key(p: &Path) -> String {
    let s = normalize(p).to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/').to_string();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// True if `inner` is `outer` or below it.
pub fn is_within(inner: &Path, outer: &Path) -> bool {
    let i = key(inner);
    let o = key(outer);
    i == o || i.starts_with(&(o + "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nvf-install-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A write with no game at hand is still refused inside the remembered install
    /// (#994); outside it, it is allowed.
    #[test]
    fn remembered_game_refuses_writes_without_a_game_at_hand() {
        let root = tmp("remembered");
        let g = fake_game(&root.join("game"));
        remember_game_at(&g.data);
        let inside = root.join("game").join("Data").join("out").join("trace.jsonl");
        assert!(matches!(guard_write(&inside, None), Err(InstallError::WriteDenied { .. })));
        assert!(matches!(guard_write(&root.join("game").join("shot.png"), None), Err(InstallError::WriteDenied { .. })));
        assert!(guard_write(&root.join("elsewhere").join("trace.jsonl"), None).is_ok());
    }

    /// The original's load-order folder is guarded as its user folder is, and a home
    /// there is reported (Windows, where the folder is known) [#1987]. Nothing is written.
    #[test]
    fn the_originals_load_order_folder_is_guarded() {
        let Some(lo) = original_load_order_dir() else { return };
        assert!(matches!(guard_write(&lo.join("plugins.txt"), None), Err(InstallError::WriteDenied { .. })));
        let root = tmp("loadorder");
        let g = fake_game(&root.join("game"));
        assert_eq!(check_separation(&EngineHome::new(lo.clone()), &g, None).len(), 1);
        assert_eq!(check_separation(&EngineHome::new(lo.join("NVForever")), &g, None).len(), 1);
        // NVForever's own folder beside it is fine.
        let own = lo.parent().unwrap().join("NVForever-guard-test");
        assert!(guard_write(&own.join("plugins.txt"), None).is_ok());
    }

    /// Screenshots take the first number not used yet, so a run keeps the last run's.
    #[test]
    fn a_screenshot_takes_the_first_free_number() {
        let d = tmp("shots");
        assert_eq!(first_unused(&d, "nvforever-", "png"), d.join("nvforever-001.png"));
        std::fs::write(d.join("nvforever-001.png"), b"").unwrap();
        std::fs::write(d.join("nvforever-002.png"), b"").unwrap();
        assert_eq!(first_unused(&d, "nvforever-", "png"), d.join("nvforever-003.png"));
        assert_eq!(EngineHome::new(&d).screenshots(), d.join("screenshots"));
    }

    /// A copy whose names differ in case from the game's (`data/falloutnv.esm`,
    /// `deadmoney.esm`), as on a case-sensitive file system, still opens, and its files
    /// are found by the game's names (#995).
    #[test]
    fn a_lower_case_copy_opens_and_finds_the_masters() {
        let root = tmp("lowercase");
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("falloutnv.esm"), b"TES4").unwrap();
        std::fs::write(data.join("deadmoney.esm"), b"TES4").unwrap();
        for at in [root.clone(), data.clone()] {
            let g = GameInstall::open(&at).unwrap();
            assert!(g.data.file_name().unwrap().eq_ignore_ascii_case("data"), "{}", g.data.display());
            assert!(g.data_file("FalloutNV.esm").is_some());
            let dm = child_ci(&g.data, "DeadMoney.esm").expect("the add-on master by the game's name");
            assert!(dm.is_file());
        }
        assert!(child_ci(&data, "HonestHearts.esm").is_none());
    }

    fn fake_game(root: &Path) -> GameInstall {
        std::fs::create_dir_all(root.join("Data")).unwrap();
        std::fs::write(root.join("Data").join("FalloutNV.esm"), b"TES4").unwrap();
        GameInstall::open(root).unwrap()
    }

    #[test]
    fn containment() {
        let t = tmp("contain");
        assert!(is_within(&t.join("a/b"), &t));
        assert!(is_within(&t, &t));
        assert!(!is_within(&t.join("ab"), &t.join("a")));
        assert!(is_within(&t.join("a/../a/b"), &t.join("a")));
        std::fs::remove_dir_all(&t).ok();
    }

    /// A base reached through a symlink (macOS: /var -> /private/var) and a path with
    /// `..` below a directory that does not exist yet.
    #[cfg(unix)]
    #[test]
    fn containment_through_symlink_with_dotdot() {
        let t = tmp("symlink");
        std::fs::create_dir_all(t.join("real")).unwrap();
        std::os::unix::fs::symlink(t.join("real"), t.join("link")).unwrap();
        let base = t.join("link");
        assert!(is_within(&base.join("a/../a/b"), &base.join("a")));
        assert!(is_within(&base.join("a/../a/b"), &t.join("real/a")));
        assert!(!is_within(&base.join("a/../c"), &base.join("a")));
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn game_open_accepts_root_or_data() {
        let t = tmp("open");
        let g = fake_game(&t.join("FNV"));
        assert!(g.data.ends_with("Data"));
        let g2 = GameInstall::open(t.join("FNV").join("Data")).unwrap();
        assert_eq!(g.root, g2.root);
        assert!(matches!(GameInstall::open(&t), Err(InstallError::NotAGameInstall(_))));
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn separation_rules() {
        let t = tmp("sep");
        let g = fake_game(&t.join("FNV"));
        let ok = EngineHome::new(t.join("NVForever"));
        assert!(check_separation(&ok, &g, Some(&t.join("NVForever-bin/nvforever.exe"))).is_empty());
        let inside = EngineHome::new(t.join("FNV").join("NVForever"));
        assert_eq!(check_separation(&inside, &g, None).len(), 1);
        let around = EngineHome::new(t.clone());
        assert_eq!(check_separation(&around, &g, None).len(), 1);
        assert_eq!(check_separation(&ok, &g, Some(&t.join("FNV/nvforever.exe"))).len(), 1);
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn writes_into_game_are_denied() {
        let t = tmp("guard");
        let g = fake_game(&t.join("FNV"));
        assert!(guard_write(&t.join("FNV/Data/new.esp"), Some(&g)).is_err());
        assert!(guard_write(&t.join("FNV/../FNV/x.txt"), Some(&g)).is_err());
        assert!(guard_write(&t.join("elsewhere/x.txt"), Some(&g)).is_ok());
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn config_roundtrip() {
        let t = tmp("cfg");
        let home = EngineHome::new(t.join("home"));
        home.create().unwrap();
        assert_eq!(Config::load(&home).unwrap(), Config::default());
        let c = Config { game_dir: Some(t.join("FNV")), ..Config::default() };
        c.save(&home, None).unwrap();
        assert_eq!(Config::load(&home).unwrap(), c);
        let text = std::fs::read_to_string(home.config_file()).unwrap();
        assert!(text.starts_with("; NVForever settings") && text.contains("\n[Game]\nPath="), "{text}");
        std::fs::remove_dir_all(&t).ok();
    }

    /// A hand-made `nvforever.ini` on a case-sensitive file system is the INI read and
    /// written: a save does not put a second `NVForever.ini` beside it. With both names
    /// present the exact one wins, as `child_ci` finds it (#995).
    #[test]
    fn an_existing_ini_is_used_whatever_its_case() {
        let t = tmp("cfg-case");
        let home = EngineHome::new(t.join("home"));
        home.create().unwrap();
        let lower = home.root.join("nvforever.ini");
        std::fs::write(&lower, "[Game]\nPath=F:\\FNV\n").unwrap();
        let exact = home.root.join(CONFIG_FILE);
        if exact.exists() {
            // A case-insensitive file system (Windows, macOS): both names are one file, so
            // there is no other case to find.
            std::fs::remove_dir_all(&t).ok();
            return;
        }
        assert_eq!(home.config_file(), lower);
        assert_eq!(Config::load(&home).unwrap().game_dir, Some(PathBuf::from("F:\\FNV")));
        Config { game_dir: Some(PathBuf::from("D:\\FNV")), ..Config::default() }.save(&home, None).unwrap();
        assert!(!exact.exists(), "the save made a second INI");
        assert_eq!(Config::load(&home).unwrap().game_dir, Some(PathBuf::from("D:\\FNV")));
        std::fs::write(&exact, "[Game]\nPath=G:\\FNV\n").unwrap();
        assert_eq!(home.config_file(), exact);
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn code_page_is_read_and_kept() {
        assert_eq!(Config::from_ini("[Text]\nCodePage=\n").code_page, None);
        for page in ["1251", "936"] {
            let ini = format!("[Game]\nPath=C:\\FNV\n\n[Text]\nCodePage = {page}\n");
            assert_eq!(Config::from_ini(&ini).code_page.as_deref(), Some(page));
            let again = Config { game_dir: Some(PathBuf::from("D:\\FNV")), ..Config::default() }.to_ini(&ini);
            assert_eq!(Config::from_ini(&again).code_page.as_deref(), Some(page));
        }
    }

    #[test]
    fn cjk_font_path_is_optional_and_kept_when_saving() {
        let text = "[Interface]\nCjkFontPath = \"fonts/My CJK font.otf\"\n";
        let path = Path::new("fonts/My CJK font.otf");
        assert_eq!(Config::from_ini(text).cjk_font_path.as_deref(), Some(path));
        assert_eq!(
            Config::from_ini("[Interface]\nCjkFontPath=\n").cjk_font_path,
            None
        );
        assert_eq!(
            Config::from_ini(&Config::default().to_ini(text))
                .cjk_font_path
                .as_deref(),
            Some(path)
        );
    }

    #[test]
    fn verbatim_prefix_dropped() {
        assert_eq!(plain(Path::new(r"\\?\F:\Steam\FNV")), PathBuf::from(r"F:\Steam\FNV"));
        assert_eq!(plain(Path::new(r"\\?\UNC\srv\share\FNV")), PathBuf::from(r"\\srv\share\FNV"));
        assert_eq!(plain(Path::new(r"\\?\Volume{x}\FNV")), PathBuf::from(r"\\?\Volume{x}\FNV"));
        assert_eq!(plain(Path::new("/home/x")), PathBuf::from("/home/x"));
    }

    #[test]
    fn ini_keeps_missing_and_empty_tile_menu_lists_distinct() {
        for (text, expected) in [
            ("", None),
            ("[UI]\n;TileMenus=MapMenu\n", None),
            ("[UI]\nTileMenus=\n", Some("")),
            ("[ui]\ntilemenus =   \n", Some("")),
            ("[UI]\nTileMenus=MapMenu,StatsMenu\n", Some("MapMenu,StatsMenu")),
        ] {
            assert_eq!(ini_get(text, "UI", "TileMenus").as_deref(), expected);
            let saved = Config::default().to_ini(text);
            assert_eq!(ini_get(&saved, "UI", "TileMenus").as_deref(), expected);
        }
        let new = Config::default().to_ini("");
        assert!(new.contains("add a [UI] section with an empty TileMenus= line"));
    }

    #[test]
    fn ini_pairs_lists_one_section() {
        let t = "[Game]
Path=X

[settings]
; c
a=1
 b = 0.50 
[Other]
c=2
";
        assert_eq!(ini_pairs(t, "Settings"), vec![("a".to_string(), "1".to_string()), ("b".to_string(), "0.50".to_string())]);
        assert!(ini_pairs(t, "None").is_empty());
    }

    #[test]
    fn ini_keeps_other_lines() {
        let text = "; mine\n[Display]\nW=1\n\n[game]\n; the folder\npath = C:\\Old\nOther=2\n";
        assert_eq!(ini_get(text, "Game", "Path").as_deref(), Some("C:\\Old"));
        assert_eq!(
            ini_set(text, "Game", "Path", "F:\\FNV"),
            "; mine\n[Display]\nW=1\n\n[game]\n; the folder\nPath=F:\\FNV\nOther=2\n"
        );
        assert_eq!(ini_set("[Display]\nW=1\n", "Game", "Path", "X"), "[Display]\nW=1\n\n[Game]\nPath=X\n");
        assert_eq!(ini_set("[Game]\nOther=2\n\n[Display]\n", "Game", "Path", "X"), "[Game]\nOther=2\nPath=X\n\n[Display]\n");
        assert_eq!(ini_get("[Game]\n;Path=no\n", "Game", "Path"), None);
        assert_eq!(ini_get("Path=top\n[Other]\nPath=no\n", "Game", "Path"), None);
    }

    #[test]
    fn legacy_json_is_read_and_adopted() {
        let t = tmp("legacy");
        let old = EngineHome::new(t.join("old"));
        old.create().unwrap();
        std::fs::write(old.root.join(LEGACY_CONFIG_FILE), r#"{"format":"nvforever.config/1","game_dir":"F:\\FNV"}"#).unwrap();
        std::fs::write(old.saves().join("a.nvfsave"), b"1").unwrap();
        let new = EngineHome::new(t.join("new"));
        new.create().unwrap();
        std::fs::write(new.saves().join("b.nvfsave"), b"2").unwrap();
        std::fs::write(old.saves().join("b.nvfsave"), b"old").unwrap();
        assert_eq!(new.adopt_legacy(&old).unwrap(), ["a.nvfsave"]);
        assert_eq!(Config::load(&new).unwrap().game_dir, Some(PathBuf::from("F:\\FNV")));
        assert!(new.config_file().is_file());
        assert_eq!(std::fs::read(new.saves().join("b.nvfsave")).unwrap(), b"2");
        assert!(old.saves().join("a.nvfsave").is_file());
        // Once the INI is there, nothing is taken again.
        std::fs::write(old.saves().join("c.nvfsave"), b"3").unwrap();
        assert!(new.adopt_legacy(&old).unwrap().is_empty());
        // Without an old game folder the INI is still written, with the path empty.
        let bare = EngineHome::new(t.join("bare"));
        std::fs::remove_file(old.root.join(LEGACY_CONFIG_FILE)).unwrap();
        assert_eq!(bare.adopt_legacy(&old).unwrap().len(), 3);
        assert_eq!(Config::load(&bare).unwrap(), Config::default());
        assert!(bare.adopt_legacy(&old).unwrap().is_empty());
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn required_files() {
        let t = tmp("required");
        let g = fake_game(&t.join("FNV"));
        assert_eq!(g.missing_files(), REQUIRED_DATA_FILES[1..].to_vec());
        assert!(matches!(g.check_files(), Err(InstallError::MissingFiles(_, ref m)) if m.len() == 7));
        for n in &REQUIRED_DATA_FILES[1..7] {
            // Any case of the name will do.
            std::fs::write(g.data.join(n.to_uppercase()), b"BSA").unwrap();
        }
        // An empty file counts as missing.
        std::fs::write(g.data.join("Update.bsa"), b"").unwrap();
        assert_eq!(g.missing_files(), ["Update.bsa"]);
        std::fs::write(g.data.join("Update.bsa"), b"BSA").unwrap();
        assert!(g.missing_files().is_empty());
        assert!(g.check_files().is_ok());
        assert!(g.addon_masters().is_empty());
        std::fs::write(g.data.join("honesthearts.esm"), b"TES4").unwrap();
        assert_eq!(g.addon_masters(), ["HonestHearts.esm"]);
        std::fs::remove_dir_all(&t).ok();
    }
}
