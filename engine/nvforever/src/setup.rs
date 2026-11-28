//! Where the original game is, and whether it has the files NVForever needs
//! (docs/design/install-layout.md, "First start").
//!
//! The game folder comes from, in order: `--data` / `NVFOREVER_DATA` (developers and the
//! smoke tests; never stored, and never a dialog, so a scripted run fails instead of
//! waiting), the game Mod Organizer 2's instance manages when MO2 started NVForever
//! (`fnv_install::mo2::game_dir`; never stored), the home's `NVForever.ini`, else the player chooses the folder in a dialog:
//! the game's own folder, the one holding `Data` (e.g.
//! `F:\SteamLibrary\steamapps\common\Fallout New Vegas`; its `Data` folder is accepted
//! too). A chosen folder is stored in the INI for later starts. Whatever the source, the
//! folder must hold every file of `fnv_install::REQUIRED_DATA_FILES` and be apart from
//! NVForever's home and executable (`fnv_install::check_separation`); otherwise the game
//! does not start and says what is wrong.
//!
//! The home (`Documents\My Games\NVForever` on Windows) is made here, and on its first
//! start the settings and saves of an earlier build's home (`%LOCALAPPDATA%\NVForever`)
//! are copied into it (`fnv_install::EngineHome::adopt_legacy`).
//!
//! A player's start loads the plugins turned on in the home's own `plugins.txt`
//! (`fnv_install::plugins`), made at the first start with the game's masters on and every
//! other plugin in `Data` off. A start from Mod Organizer 2 loads the plugins of MO2's
//! profile instead (`fnv_install::mo2`; docs/mod-organizer-2.md).

use fnv_install::{check_separation, Config, EngineHome, GameInstall, InstallError};
use std::path::{Path, PathBuf};

/// The game at `dir`, or why it cannot be used, in words for the player.
fn validate(dir: &Path, home: &EngineHome) -> Result<GameInstall, String> {
    let game = GameInstall::open(dir).map_err(|e| match e {
        InstallError::NotAGameInstall(p) => format!(
            "{} is not a Fallout: New Vegas folder: there is no Data\\FalloutNV.esm in it.\n\
             Choose the folder that holds FalloutNV.exe and the Data folder.",
            p.display()
        ),
        InstallError::Io(p, e) => format!("{}: {e}", p.display()),
        e => e.to_string(),
    })?;
    game.check_files().map_err(|e| format!("{e}\n\nVerify the game's files (Steam: Properties, Installed Files) or reinstall the game."))?;
    let exe = std::env::current_exe().ok();
    let overlap = check_separation(home, &game, exe.as_deref());
    if !overlap.is_empty() {
        return Err(InstallError::Overlap(overlap).to_string());
    }
    Ok(game)
}

/// The game install to run with (module notes). `cli` is `--data` / `NVFOREVER_DATA`.
pub fn game_install(cli: Option<&Path>) -> Result<GameInstall, String> {
    let home = EngineHome::user().ok_or("no folder for NVForever's settings and saves could be found (set NVFOREVER_HOME)")?;
    home.create().map_err(|e| e.to_string())?;
    if let Some(old) = EngineHome::legacy() {
        match home.adopt_legacy(&old) {
            Ok(saves) if !saves.is_empty() => {
                eprintln!("setup: copied {} save(s) from {} to {}", saves.len(), old.saves().display(), home.saves().display())
            }
            Ok(_) => {}
            Err(e) => eprintln!("setup: the earlier settings and saves were not brought over: {e}"),
        }
    }
    if let Some(dir) = cli {
        return validate(dir, &home);
    }
    // Started from Mod Organizer 2: the game its instance manages, where it shows the mods
    // (never stored; the INI's folder is for other starts).
    if let Some(dir) = fnv_install::mo2::game_dir() {
        eprintln!("setup: Mod Organizer 2 start: game folder from its instance ({})", dir.display());
        return validate(&dir, &home).map_err(|e| {
            format!("The game folder Mod Organizer 2 manages ({}) cannot be used:\n\n{e}", dir.display())
        });
    }
    let cfg = Config::load(&home).map_err(|e| e.to_string())?;
    let mut problem = None;
    if let Some(dir) = &cfg.game_dir {
        match validate(dir, &home) {
            Ok(g) => return Ok(g),
            Err(e) => {
                problem = Some(format!(
                    "The Fallout: New Vegas folder set in {}\n({}) cannot be used:\n\n{e}",
                    home.config_file().display(),
                    dir.display()
                ))
            }
        }
    }
    let game = ask(&home, problem)?;
    Config { game_dir: Some(game.root.clone()), ..cfg }.save(&home, Some(&game)).map_err(|e| e.to_string())?;
    eprintln!("setup: game folder {} stored in {}", game.root.display(), home.config_file().display());
    Ok(game)
}

/// The code page of the game's text for this run (`fnv_util::text::chosen`, #1439):
/// `NVF_CODE_PAGE`, else `[Text] CodePage` of the home's NVForever.ini, else
/// Windows-1252. Set before any of the game's text is read. The flag says whether one of
/// the two named it; when neither did, [`guess_code_page`] may change it once the
/// plugins are open. Read the optional CJK font path from the same INI.
pub fn text_code_page() -> Result<(fnv_util::text::CodePage, bool, Option<PathBuf>), String> {
    let (ini, cjk_font_path) = EngineHome::user().and_then(|home| {
        let cfg = Config::load(&home).ok()?;
        let cjk_font_path = cfg.cjk_font_path.map(|path| home.root.join(path));
        Some((cfg.code_page, cjk_font_path))
    }).unwrap_or_default();
    fnv_util::text::configured(ini.as_deref())
        .map(|page| (page.unwrap_or_default(), page.is_some(), cjk_font_path))
}

/// The code page read from the plugins' own text when no setting names one (#1443,
/// `fnv_esm::text_guess`): set and said in the log when it is not the default. Runs after
/// the plugins are opened and before their record text is decoded. False: the text tells
/// none.
pub fn guess_code_page(plugins: &[fnv_esm::Plugin]) -> bool {
    let g = fnv_esm::text_guess::guess(plugins);
    let Some(c) = g.code_page else { return false };
    eprintln!(
        "setup: game text read as {c}, found in the plugins' text ({}); set [Text] CodePage in NVForever.ini to choose another",
        g.supporting_plugins().join(", ")
    );
    fnv_util::text::set_code_page(c);
    true
}

/// The language Steam installed the game in (`fnv_install::steam`, #1443): its code page
/// and the language of NVForever's own words, said in the log when it is not English.
/// `data` is the game's `Data` folder; `None` outside a Steam library or for a language
/// this build does not know.
pub fn steam_language(data: &std::path::Path) -> Option<(fnv_util::text::CodePage, Option<&'static str>)> {
    let name = fnv_install::steam::manifest_language(data.parent()?)?;
    let found = fnv_util::words::for_steam_language(&name);
    match found {
        Some((page, _)) if name != "english" => eprintln!("setup: Steam installed the game in {name} ({page}), used when no setting and no plugin text names a code page"),
        None => eprintln!("setup: Steam installed the game in {name}, a language this build does not know; the code page comes from the plugins' text"),
        _ => {}
    }
    found
}

/// The code page Steam's language names (`steam_language`), set when no setting names one.
pub fn use_code_page(page: fnv_util::text::CodePage) {
    fnv_util::text::set_code_page(page);
}

/// NVForever's own words in the language Steam installed the game in, when its file was
/// read and the run reads the game's text in that language's code page; else in the
/// language of the game's text (#1948,
/// `fnv_util::words::follow_code_page`): chosen and said in the log when the run's code
/// page names a language and its file was read. Runs after the code page is settled and
/// before NVForever.ini's rows are applied, so a `language=` there wins.
pub fn follow_text_language(steam: Option<(fnv_util::text::CodePage, Option<&'static str>)>) {
    let page = fnv_util::text::code_page();
    if let Some((steam_page, Some(id))) = steam.filter(|s| s.0 == page) {
        if let Some(id) = fnv_util::words::follow_language(id) {
            eprintln!("setup: NVForever's own words in {id}, the language Steam installed the game in ({steam_page}); set language in NVForever.ini or on the settings page to choose another");
            return;
        }
    }
    if let Some(id) = fnv_util::words::follow_code_page(page) {
        eprintln!("setup: NVForever's own words in {id}, the language of the game's text ({page}); set language in NVForever.ini or on the settings page to choose another");
    }
}

/// Draw the fonts the install's `Fallout_default.ini` names in `[Fonts]` (`sFontFile_N`),
/// not always the English install's files (#1439). The file is in the game's own folder,
/// read-only input; the per-user `My Games\FalloutNV` copy is never read
/// (docs/design/install-layout.md). A localized install pointing these at its own files is
/// [hypothesis: no localized install looked at, observer check 77].
pub fn install_fonts(game: &GameInstall, cjk_font_path: Option<PathBuf>) {
    nvf_play::interface::set_cjk_font_path(cjk_font_path);
    let Some(text) = fnv_install::child_ci(&game.root, "Fallout_default.ini").and_then(|p| std::fs::read(p).ok()) else {
        return;
    };
    let fonts = nvf_play::interface::fonts_in_ini(&fnv_util::text::code_page().decode(&text));
    for (n, (own, default)) in fonts.iter().zip(nvf_play::interface::FONT_FILES).enumerate() {
        if let Some(own) = own.as_deref().filter(|own| !own.eq_ignore_ascii_case(default)) {
            eprintln!("setup: font {} read from {own} (the install's Fallout_default.ini)", n + 1);
        }
    }
    nvf_play::interface::set_font_files(fonts);
}

#[cfg(windows)]
const INTRO: &str = "Welcome to NVForever.\n\n\
    NVForever runs on your own copy of Fallout: New Vegas and only reads its files; it never \
    changes the game's folder.\n\n\
    Choose the game's folder next: the one that holds FalloutNV.exe and the Data folder, \
    for example\nF:\\SteamLibrary\\steamapps\\common\\Fallout New Vegas\n\n\
    It is remembered for later starts.";

/// Usual places of the game, to open the folder dialog at [our choice: Steam's default
/// library and GOG's default folder].
#[cfg(windows)]
fn likely_start() -> Option<std::path::PathBuf> {
    [
        r"C:\Program Files (x86)\Steam\steamapps\common\Fallout New Vegas",
        r"C:\GOG Games\Fallout New Vegas",
        r"C:\Program Files (x86)\Steam\steamapps\common",
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .find(|p| p.is_dir())
}

/// Ask for the game's folder in the system's dialogs until it can be used or the player
/// gives up. `problem` says why the stored folder was refused.
#[cfg(windows)]
fn ask(home: &EngineHome, problem: Option<String>) -> Result<GameInstall, String> {
    use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
    let confirm = |level: MessageLevel, text: &str| {
        let r = MessageDialog::new().set_title("NVForever").set_level(level).set_description(text).set_buttons(MessageButtons::OkCancel).show();
        matches!(r, MessageDialogResult::Ok)
    };
    let cancelled = || format!("no Fallout: New Vegas folder was chosen; NVForever needs one (it is stored in {})", home.config_file().display());
    let (level, text) = match &problem {
        Some(p) => (MessageLevel::Warning, format!("{p}\n\nChoose the game's folder again.")),
        None => (MessageLevel::Info, INTRO.to_string()),
    };
    if !confirm(level, &text) {
        return Err(problem.unwrap_or_else(cancelled));
    }
    loop {
        let mut dialog = FileDialog::new().set_title("Choose your Fallout: New Vegas folder");
        if let Some(start) = likely_start() {
            dialog = dialog.set_directory(start);
        }
        let Some(dir) = dialog.pick_folder() else { return Err(cancelled()) };
        match validate(&dir, home) {
            Ok(g) => return Ok(g),
            Err(e) => {
                if !confirm(MessageLevel::Error, &format!("{e}\n\nChoose another folder?")) {
                    return Err(e);
                }
            }
        }
    }
}

/// No dialogs here yet: say where the folder goes.
#[cfg(not(windows))]
fn ask(home: &EngineHome, problem: Option<String>) -> Result<GameInstall, String> {
    let how = format!(
        "pass --data <Fallout New Vegas folder>, or set Path= under [Game] in {}",
        home.config_file().display()
    );
    Err(match problem {
        Some(p) => format!("{p}\n{how}"),
        None => format!("no Fallout: New Vegas folder is set: {how}"),
    })
}

/// The plugins to load, in order, from the home's `plugins.txt` (made or given new lines
/// here); says what the player may want to know about it.
pub fn plugin_list(game: &GameInstall) -> Result<Vec<String>, String> {
    // Started from Mod Organizer 2: its profile's plugins, and the home's list is left alone.
    if let Some((file, s)) = fnv_install::mo2::sync(game).map_err(|e| e.to_string())? {
        eprintln!("setup: Mod Organizer 2 start: plugins from its profile ({})", file.display());
        if !s.missing.is_empty() {
            eprintln!("setup: plugins on in Mod Organizer 2 but not in Data, skipped: {}", s.missing.join(", "));
        }
        if s.base_forced {
            eprintln!("setup: FalloutNV.esm loads first whatever Mod Organizer 2's order says");
        }
        eprintln!("setup: plugins: {}", s.load.join(", "));
        return Ok(s.load);
    }
    let home = EngineHome::user().ok_or("no folder for NVForever's settings and saves could be found (set NVFOREVER_HOME)")?;
    let file = home.plugins_file();
    let s = fnv_install::plugins::sync(&home, game).map_err(|e| e.to_string())?;
    if s.written {
        eprintln!("setup: plugin list written: {}", file.display());
    }
    let off: Vec<&String> = s.added.iter().filter(|a| !s.load.contains(a)).collect();
    if !off.is_empty() {
        eprintln!(
            "setup: plugins found in Data and left off (turn them on with a * in {}): {}",
            file.display(),
            off.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        );
    }
    if !s.missing.is_empty() {
        eprintln!("setup: plugins turned on in {} but in neither Data nor {}, skipped: {}", file.display(), home.mods().display(), s.missing.join(", "));
    }
    if s.base_forced {
        eprintln!("setup: FalloutNV.esm loads first whatever {} says", file.display());
    }
    if !s.from_mods.is_empty() {
        eprintln!("setup: plugins from {}: {}", home.mods().display(), s.from_mods.join(", "));
    }
    eprintln!("setup: plugins: {}", s.load.join(", "));
    Ok(s.load)
}
