//! The launcher's window: Home (news), Settings, Mods and Game folder pages beside a bar
//! that starts the game.

use crate::catalog::{self, Catalog, Kind, Row};
use crate::catalog::rule_holds;
use crate::launch::{self, Start};
use crate::news::{self, Post, Source};
use crate::theme::{self, BAD, CARD, GOLD, GOOD, LINE, WEAK};
use crate::update::{self, Msg, Phase};
use nvf_settings_layout::menu;
use eframe::egui::{self, load::Bytes, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use fnv_install::{guard_write, ini_get, ini_pairs, ini_set, plugins, write_whole, Config, EngineHome, GameInstall};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

const LOGO: &[u8] = include_bytes!("../assets/logo-256.png");
/// The launcher's own keys in the home's INI.
const LAUNCHER: &str = "Launcher";
/// How much of a long post shows before "Read more".
const PREVIEW_CHARS: usize = 700;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Settings,
    Mods,
    Game,
    /// Co-op: what it is and Play Co-Op (the game's main menu, where co-op is turned on).
    Coop,
}

enum NewsState {
    Off,
    Loading,
    Shown(Source),
    Failed(String),
}

/// The Game folder page's NVForever.ini keys (#3483): the keys this launcher reads that are not
/// rows of the game's menu, the new game's start among them (#4313). `[Game] Path` has its own
/// box and Save on that page.
#[derive(Debug, Clone, Default, PartialEq)]
struct IniForm {
    /// `[General] sCharGenQuest` and `sIntroMovie` (src/start.rs).
    start: crate::start::Fields,
    /// `[Launcher] Exe`; empty starts the program beside the launcher.
    exe: String,
    /// `[Launcher] News`.
    news: bool,
    /// `[Launcher] CloseOnPlay`.
    close: bool,
}

impl IniForm {
    /// The form as the INI `text` has it, with the launcher's defaults for keys it lacks.
    fn read(text: &str) -> IniForm {
        let flag = |key, default| ini_get(text, LAUNCHER, key).map_or(default, |v| v != "0");
        IniForm {
            start: crate::start::Fields::read(text),
            exe: ini_get(text, LAUNCHER, "Exe").unwrap_or_default(),
            news: flag("News", true),
            close: flag("CloseOnPlay", false),
        }
    }

    /// The form as it would be saved: the typed program without surrounding spaces.
    fn trimmed(&self) -> IniForm {
        IniForm { exe: self.exe.trim().to_string(), ..self.clone() }
    }

    /// Why this form cannot be saved over `was` (the INI as read), if it cannot. Only a
    /// changed program is checked, so a saved one that went missing (an unplugged drive) does
    /// not stop the other keys being saved. An empty program clears `Exe`.
    fn check(&self, was: &IniForm, is_file: impl Fn(&str) -> bool) -> Result<(), String> {
        let now = self.trimmed();
        if now.exe != was.exe && !now.exe.is_empty() && !is_file(&now.exe) {
            return Err(format!("{} is not a file.", now.exe));
        }
        Ok(())
    }

    /// `text` with the keys this form changed from `was` set, as the game tabs' Save writes
    /// only the rows that changed; every other line is kept.
    fn apply(&self, was: &IniForm, text: &str) -> String {
        let now = self.trimmed();
        let flag = |on: bool| if on { "1" } else { "0" };
        let mut t = text.to_string();
        if now.start != was.start {
            t = now.start.write(&t);
        }
        if now.exe != was.exe {
            t = ini_set(&t, LAUNCHER, "Exe", &now.exe);
        }
        if now.news != was.news {
            t = ini_set(&t, LAUNCHER, "News", flag(now.news));
        }
        if now.close != was.close {
            t = ini_set(&t, LAUNCHER, "CloseOnPlay", flag(now.close));
        }
        t
    }
}

type NewsResult = Result<(Vec<Post>, Source), String>;
type NextResult = Result<Option<news::NextRelease>, String>;
type LatestResult = Result<Option<news::Latest>, String>;

/// Where the updater is (docs/design/launcher.md "Updater").
enum Upd {
    Idle,
    Working { phase: Phase, done: u64, total: u64 },
    /// Downloaded and checked; installs as soon as the game is not running.
    Ready(update::Ready),
    Installing,
    /// The swap is done and the new launcher was started: this window is closing (#3117).
    Closing,
    Failed(String),
}

impl Upd {
    /// Whether a close of the window is refused: only during the swap itself. The launcher's
    /// own close after the swap arrives as a close request one frame later, so the state has
    /// to have left `Installing` by then or the launcher cancels its own close (#3117).
    fn refuses_close(&self) -> bool {
        matches!(self, Upd::Installing)
    }

    /// The state once the swap went in, by whether the new launcher could be started.
    fn installed(restarted: &Result<(), String>) -> Upd {
        if restarted.is_ok() { Upd::Closing } else { Upd::Idle }
    }
}

/// The launcher's version: its build's, or `NVF_LAUNCHER_VERSION` (a test aid: a launcher
/// that takes itself for an older version offers the newest release as an update, so the
/// updater can be tried against a real release; docs/design/launcher.md).
fn version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        std::env::var("NVF_LAUNCHER_VERSION").ok().filter(|v| update::test_aids() && update::safe_component(v)).unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
    })
}

/// The message line while the game starts, cleared when it ends without an error.
const STARTING: &str = "The game is starting…";
/// On a start with no game folder set (#2938): at the launcher's start, and on Play.
const NEEDS_FOLDER: &str = "Set your Fallout: New Vegas folder here, then Save: the game needs it to start.";
const NEEDS_FOLDER_FIRST: &str = "Set your Fallout: New Vegas folder first (here, then Save): the game cannot start without it.";

#[derive(Clone)]
struct NewestSave {
    path: PathBuf,
    name: String,
    detail: String,
    shot: Option<Arc<[u8]>>,
}

pub struct App {
    page: Page,
    home: Option<EngineHome>,
    ini_text: String,
    message: Option<(String, bool)>,
    // Settings.
    /// The game's settings menu (src/catalog.rs): its rows, its tabs' names and the words shown.
    rows: Vec<Row>,
    tabs: Vec<String>,
    tab_labels: Vec<String>,
    tab: usize,
    /// The Game folder page's NVForever.ini keys as edited ([`IniForm`]).
    ini: IniForm,
    words: HashMap<&'static str, &'static [&'static str]>,
    lists: HashMap<&'static str, &'static [&'static str]>,
    stored: BTreeMap<String, String>,
    edits: BTreeMap<String, String>,
    // Game folder.
    game_edit: String,
    /// The start card's Custom… window, open while `Some`: the quest and film as typed there,
    /// put into the NVForever.ini card's start by its OK (#4288, #4313).
    start_custom: Option<crate::start::Fields>,
    // Mods.
    mods: Option<plugins::PluginList>,
    mods_dirty: bool,
    /// The row being dragged in the load order.
    mods_drag: Option<usize>,
    /// How many load-order backups to keep. The cap, not how many exist now.
    backup_max: u32,
    /// A lower cap waiting on a yes before the oldest backups are deleted.
    backup_cut: Option<u32>,
    /// The backup Load backup writes into a profile (`plugins-<this>.txt`).
    backup_pick: Option<String>,
    /// The mod list. A lower ticked mod wins a loose file. Apply does not link into Data.
    mod_rows: Vec<ModRow>,
    /// The mod row being dragged. A locked row stays put.
    mod_drag: Option<usize>,
    /// Plugin names found when the mod list was opened or refreshed.
    mods_present: HashSet<String>,
    /// Plugin name, then the ticked mods that contain it, top of the mod list first.
    plugin_folders: HashMap<String, Vec<String>>,
    /// Plugin name, then the masters named in the plugin that wins.
    plugin_masters: HashMap<String, Vec<String>>,
    /// Plugin name, then the mod it belongs to. A lower mod wins.
    plugin_home: HashMap<String, String>,
    /// How many plugins each mod contains.
    folder_counts: HashMap<String, usize>,
    /// Loose files and plugins in each mod, relative to its Data folder when it has one.
    folder_files: HashMap<String, Vec<String>>,
    /// Loose-file clashes. A lower ticked mod wins.
    clashes: HashMap<String, Vec<Clash>>,
    /// The find box on the load order.
    plugin_find: String,
    /// The find box on the mod list.
    mod_find: String,
    /// The plugin last clicked, so its mod is marked in the mod list.
    picked_plugin: Option<String>,
    /// Scroll the mod list to that mod once.
    mod_jump: bool,
    /// Show overwrites is on. Green: this mod's loose file wins. Red: a lower mod's copy wins.
    show_overwrites: bool,
    /// Apply is asking before it writes the lists.
    apply_ask: bool,
    /// The name typed for a profile. It is saved under the launcher's profiles folder, next to mods.
    profile_name: String,
    /// Profile name, then the folder that holds its mod list and load order.
    profile_choices: Vec<(String, PathBuf)>,
    /// The profile chosen in the list.
    profile_pick: Option<String>,
    /// Delete is asking before it removes that profile. Mod folders stay.
    profile_delete_ask: bool,
    /// A mod waiting for a new folder name.
    mod_rename: Option<usize>,
    mod_rename_text: String,
    /// A mod waiting to be moved into the launcher's recycle folder.
    mod_recycle: Option<usize>,
    /// The load order shows this mod's plugins only.
    plugin_only_folder: Option<String>,
    /// The note on the open profile.
    profile_note: String,
    /// Archive names stored with the profile. The mod list does not show them.
    profile_archives: Vec<String>,
    /// Mod list and load order last saved or loaded, so the page can show unsaved changes.
    saved_lists: String,
    /// Plugins that were ticked before a mod was unticked, so ticking it again can put them back.
    plugin_ticks: HashMap<String, HashSet<String>>,
    /// The mod list before the last drag.
    drag_undo: Option<Vec<ModRow>>,
    /// Mods in the multi-select. Ctrl-click adds one.
    mod_selected: HashSet<String>,
    /// Plugins in the multi-select. Ctrl-click adds one.
    plugin_selected: HashSet<String>,
    /// 0 all, 1 ticked, 2 unticked, 3 has an overwrite, 4 has a plugin.
    mod_filter: u8,
    /// Play is asking because the mod list or the load order is unsaved.
    play_dirty_ask: bool,
    /// Set when Add profile would replace a profile that already has a mod list.
    profile_replace: Option<String>,
    /// The mod a new separator sits under.
    separator_at: Option<usize>,
    /// True: the new separator is inserted above that mod. False: below it.
    separator_above: bool,
    /// A separator being renamed. The window uses `separator_name`.
    separator_edit: Option<usize>,
    separator_name: String,
    /// Focus the separator name once when the window opens.
    separator_focus: bool,
    /// Loose files a ticked mod already has. The install waits until the player decides.
    install_warn: Vec<String>,
    install_ready: Option<PathBuf>,
    /// Which mod's note is open.
    mod_note_for: Option<String>,
    mod_note: String,
    /// An install held until the loose-file clash list has been read.
    held: Option<HeldInstall>,
    /// An archive with more than one folder that could be the mod.
    root_ask: Option<RootAsk>,
    /// The archive the open FOMOD was staged from.
    install_source: Option<PathBuf>,
    /// The remembered profile was already loaded this run.
    profile_restored: bool,
    /// Used once by the next install, when a dropped archive supplies a mod name.
    install_name: String,
    /// Quick Install is open. Nothing is copied yet.
    quick: Option<QuickAsk>,
    /// Manual install is open. Nothing is copied until OK.
    manual: Option<ManualAsk>,
    /// The profile bar, so a drop there is not installed as a mod.
    profile_rect: egui::Rect,
    /// An open FOMOD. Nothing else is added until it is finished.
    wizard: Option<crate::fomod::Wizard>,
    /// Bottom bar height. Saved when the user lets go of the grip.
    bar_h: f32,
    /// The displays, for the Resolution and Monitor lists (asked once at start).
    monitors: Vec<crate::display::Monitor>,
    /// The Language list (the language files found at start).
    languages: Vec<crate::display::Choice>,
    /// "Delete them now": asking first, then the request's answer.
    reports_confirm: bool,
    /// DLSS: what this PC has (read at start and after a download), and the licence open.
    dlss: crate::dlss::Status,
    dlss_licence: bool,
    reports_rx: Option<Receiver<Result<crate::reports::Deleted, String>>>,
    // News.
    news: Vec<(Post, Option<Arc<[u8]>>)>,
    news_state: NewsState,
    news_rx: Option<Receiver<NewsResult>>,
    next_rx: Option<Receiver<NextResult>>,
    next: Option<news::NextRelease>,
    latest_rx: Option<Receiver<LatestResult>>,
    /// A release newer than this launcher, when GitHub has one.
    latest: Option<news::Latest>,
    expanded: HashSet<usize>,
    md: CommonMarkCache,
    // The updater.
    upd: Upd,
    upd_rx: Option<Receiver<Msg>>,
    upd_cancel: Arc<AtomicBool>,
    upd_open: bool,
    // Starting the game.
    newest: Option<NewestSave>,
    exe: Option<PathBuf>,
    child: Option<Child>,
    /// Test aid: `NVF_LAUNCHER_SHOT=<file.png>[;page]` saves the window once the news has
    /// settled and closes (`NVF_LAUNCHER_PAGE`: home, settings, mods, game, coop).
    shot: Option<(PathBuf, u32)>,
}

/// A row of the mod list (`fnv_install::mod_order`): a mod folder or a separator. A locked
/// mod stays put when another row is dragged.
type ModRow = fnv_install::mod_order::ModLine;

/// The side file of a profile's locked mods, beside its `modlist.txt` (MO2 has no lock mark).
const PROFILE_LOCKS: &str = "locks.txt";
/// The side file of the home list's locked mods, beside `mod-order.txt`.
const HOME_LOCKS: &str = "mod-locks.txt";

/// What a right-click on a mod row asked for. Applied after the row is drawn.
enum ModAct {
    Open(usize),
    Top(usize),
    Bottom(usize),
    Overwrites,
    Copied(usize),
    Rename(usize),
    Recycle(usize),
    Lock(usize),
    Note(usize),
    /// Name a separator and put it above (true) or below this row.
    Separator(usize, bool),
    /// Rename the separator on this row.
    RenameSeparator(usize),
    /// Turn Overwrite's loose files into a mod under this row.
    MakeBelow(usize),
    /// Put the mod list back to how it was before the last drag.
    Undo,
    /// Enable or disable every mod in the multi-select.
    SetSelected(bool),
    /// A dated copy of the mod list and the load order. Not a copy of the mod files.
    Backup,
}

/// An install held back until the player has seen which loose files would lose.
enum HeldInstall {
    Tree { name: String, from: PathBuf, temp: Option<PathBuf>, source: PathBuf },
    Plan { plan: crate::fomod::Plan, source: PathBuf },
}

struct RootAsk {
    name: String,
    roots: Vec<PathBuf>,
    temp: Option<PathBuf>,
    source: PathBuf,
}

/// Quick Install. Nothing is written until OK or Manual.
struct QuickAsk {
    source: PathBuf,
    name: String,
    suggestions: Vec<String>,
}

/// Manual install. `<data>` is the folder that becomes the mod.
struct ManualAsk {
    source: PathBuf,
    temp: Option<PathBuf>,
    top: PathBuf,
    data_rel: PathBuf,
    tree: crate::fomod::ManFile,
    name: String,
}

/// The plugins only mod folders that are off have, so the game opens none of them on a start
/// through the launcher (`plugins::layered_plugin_files`, then Data): `home` (the page's
/// `plugin_home`) names a folder in `off`, no folder that is on has one (`on_folders`, the
/// page's `plugin_folders`), and `elsewhere` (loose at the top of the mods folder, or in Data)
/// is false. `plugin_home` names one folder a plugin, the highest; a lower folder that is on, or
/// a loose copy, still serves it.
fn only_in_off_folders(home: &HashMap<String, String>, off: &HashSet<String>, on_folders: &HashMap<String, Vec<String>>, elsewhere: impl Fn(&str) -> bool) -> HashSet<String> {
    home.iter()
        .filter(|(plugin, folder)| off.contains(*folder) && !on_folders.contains_key(*plugin) && !elsewhere(plugin))
        .map(|(plugin, _)| plugin.clone())
        .collect()
}

/// What a right-click on a plugin row asked for.
enum PluginAct {
    /// The owning mod, so the row can open that folder.
    Show(usize),
    Copy(String),
    /// Enable or disable the plugins in the multi-select. FalloutNV.esm stays.
    Selected(bool),
    /// Enable or disable every plugin except FalloutNV.esm.
    All(bool),
}

/// What a right-click on a profile asked for.
enum ProfileAct {
    Open,
    Duplicate,
    Rename,
    DeleteAsk,
}

struct Clash {
    other: String,
    files: Vec<String>,
    wins: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        crate::fonts::install(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        // Test aid: NVF_LAUNCHER_ZOOM=0.6 draws everything smaller (a whole page in one shot).
        if let Some(z) = std::env::var("NVF_LAUNCHER_ZOOM").ok().and_then(|z| z.parse::<f32>().ok()).filter(|z| (0.3..=3.0).contains(z)) {
            cc.egui_ctx.set_zoom_factor(z);
        }
        let home = EngineHome::user();
        let menu = Catalog::load(home.as_ref().map(EngineHome::config_file).as_deref());
        let backup_max = read_backup_max(home.as_ref());
        let mut app = App {
            page: Page::Home,
            home,
            ini_text: String::new(),
            message: None,
            tabs: menu.tab_names(),
            tab_labels: menu.tabs.iter().map(|(_, l)| l.clone()).collect(),
            rows: menu.rows,
            tab: 0,
            ini: IniForm::default(),
            words: catalog::step_words(),
            lists: catalog::list_choices(),
            stored: BTreeMap::new(),
            edits: BTreeMap::new(),
            game_edit: String::new(),
            start_custom: None,
            mods: None,
            mods_dirty: false,
            mods_drag: None,
            backup_max,
            backup_cut: None,
            backup_pick: None,
            mod_rows: Vec::new(),
            mod_drag: None,
            mods_present: HashSet::new(),
            plugin_folders: HashMap::new(),
            plugin_masters: HashMap::new(),
            plugin_home: HashMap::new(),
            folder_counts: HashMap::new(),
            folder_files: HashMap::new(),
            clashes: HashMap::new(),
            plugin_find: String::new(),
            mod_find: String::new(),
            picked_plugin: None,
            mod_jump: false,
            show_overwrites: false,
            apply_ask: false,
            profile_name: String::new(),
            profile_choices: Vec::new(),
            profile_pick: None,
            profile_delete_ask: false,
            mod_rename: None,
            mod_rename_text: String::new(),
            mod_recycle: None,
            plugin_only_folder: None,
            profile_note: String::new(),
            profile_archives: Vec::new(),
            saved_lists: String::new(),
            plugin_ticks: HashMap::new(),
            drag_undo: None,
            mod_selected: HashSet::new(),
            plugin_selected: HashSet::new(),
            mod_filter: 0,
            play_dirty_ask: false,
            profile_replace: None,
            separator_at: None,
            separator_above: false,
            separator_edit: None,
            separator_name: String::new(),
            separator_focus: false,
            install_warn: Vec::new(),
            install_ready: None,
            mod_note_for: None,
            mod_note: String::new(),
            held: None,
            root_ask: None,
            install_source: None,
            profile_restored: false,
            install_name: String::new(),
            quick: None,
            manual: None,
            profile_rect: egui::Rect::NOTHING,
            wizard: None,
            bar_h: 56.0,
            monitors: crate::display::monitors(),
            languages: crate::display::language_choices(&fnv_util::words::dirs(
                std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())).as_deref(),
                EngineHome::user().map(|h| h.root).as_deref(),
            )),
            reports_confirm: false,
            dlss: crate::dlss::Status::read(None),
            dlss_licence: false,
            reports_rx: None,
            news: Vec::new(),
            news_state: NewsState::Off,
            news_rx: None,
            next_rx: None,
            next: None,
            latest_rx: None,
            latest: None,
            expanded: HashSet::new(),
            md: CommonMarkCache::default(),
            upd: Upd::Idle,
            upd_rx: None,
            upd_cancel: Arc::new(AtomicBool::new(false)),
            upd_open: false,
            newest: None,
            exe: None,
            child: None,
            shot: std::env::var_os("NVF_LAUNCHER_SHOT").map(|p| (PathBuf::from(p), 0)),
        };
        app.page = match std::env::var("NVF_LAUNCHER_PAGE").as_deref() {
            Ok("settings") => Page::Settings,
            Ok("mods") => Page::Mods,
            Ok("game") => Page::Game,
            Ok("coop") => Page::Coop,
            Ok("update") => {
                app.upd_open = true;
                Page::Home
            }
            // The "Delete them now" question, open (nothing is sent without its button).
            Ok("delete-reports") => {
                app.reports_confirm = true;
                Page::Settings
            }
            // NVIDIA's licence before Download DLSS, open (nothing is fetched without I accept).
            Ok("dlss-licence") => {
                app.dlss_licence = true;
                app.tab = app.tabs.iter().position(|t| t == "Graphics").unwrap_or(0);
                Page::Settings
            }
            // `settings:<tab>`: that tab of the Settings page.
            Ok(p) if p.starts_with("settings:") => {
                let tab = &p["settings:".len()..];
                app.tab = app.tabs.iter().position(|t| t.eq_ignore_ascii_case(tab)).unwrap_or(0);
                Page::Settings
            }
            _ => Page::Home,
        };
        // The INI first: the mod list's sync needs its game folder.
        app.reload_ini();
        app.bar_h = ini_get(&app.ini_text, LAUNCHER, "BottomBar").and_then(|s| s.parse::<f32>().ok()).unwrap_or(56.0).clamp(48.0, 160.0);
        if app.page == Page::Mods {
            app.enter_mods();
        }
        // No game folder yet (a first start): open on its page (#2938), unless a test aid
        // chose the page.
        if !app.folder_known() && std::env::var_os("NVF_LAUNCHER_PAGE").is_none() {
            app.page = Page::Game;
            app.message = Some((NEEDS_FOLDER.into(), false));
        }
        app.find_newest_save();
        if let Some(h) = &app.home {
            app.news = news::load_cache(&h.cache()).into_iter().map(|p| (p, None)).collect();
            if !app.news.is_empty() {
                app.news_state = NewsState::Shown(Source::Cache);
            }
        }
        if app.launcher_flag("News", true) {
            app.fetch_news(&cc.egui_ctx);
        }
        // Every write of the launcher's (the INI, the updater's swap) is refused inside the game
        // folder the INI names (fnv_install::guard_write), even where no GameInstall is passed.
        if let Some(g) = app.game() {
            fnv_install::remember_game(&g);
        }
        // A swap cut short in its renames (the window closed, the power lost) is finished
        // first; a swap cut short before them leaves only files to tidy away.
        // The folder the launcher runs from, whether or not nvforever is still beside it.
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) {
            match update::resume_interrupted(&dir, app.game().as_ref()) {
                Some(Ok(v)) => app.message = Some((format!("An update to v{v} was interrupted; it has been finished."), false)),
                Some(Err(e)) => app.message = Some((format!("An interrupted update could not be finished: {e}. Download the release from its page."), true)),
                None => {}
            }
        }
        // Started by the updater (update::restart): say what changed.
        for a in std::env::args().skip(1) {
            match a.as_str() {
                "--updated" | "--rolled-back" => {
                    // The version the swap wrote (an archive without a launcher leaves this one).
                    let installed = install_dir(app.game().as_ref()).ok().and_then(|d| update::read_manifest(&d)).map_or_else(|| version().to_string(), |m| m.version);
                    app.message = Some(if a == "--updated" {
                        (format!("Updated to v{installed}. Your saves, settings and mods are as they were."), false)
                    } else {
                        (format!("Rolled back to v{installed}."), false)
                    });
                }
                _ => {}
            }
        }
        // Downloads of releases no newer than this one are done with.
        if let Some(h) = &app.home {
            let cache = h.cache();
            std::thread::spawn(move || {
                for d in update::stale_staging(&cache, version()) {
                    let _ = std::fs::remove_dir_all(d);
                }
            });
        }
        // Test aid: NVF_LAUNCHER_PAGE=mods opens the mod page for a layout shot.
        if std::env::var("NVF_LAUNCHER_PAGE").ok().as_deref() == Some("mods") {
            app.page = Page::Mods;
            app.enter_mods();
        }
        app
    }

    // ---- The INI ----

    fn ini_path(&self) -> Option<PathBuf> {
        self.home.as_ref().map(EngineHome::config_file)
    }

    fn reload_ini(&mut self) {
        self.ini_text = self.ini_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        self.stored = ini_pairs(&self.ini_text, "Settings").into_iter().collect();
        self.edits.clear();
        // The game writes its menu at each start (src/catalog.rs): the page follows it, on the
        // same tab.
        let menu = Catalog::load(self.ini_path().as_deref());
        let on = self.tabs.get(self.tab).cloned();
        self.tabs = menu.tab_names();
        self.tab_labels = menu.tabs.iter().map(|(_, l)| l.clone()).collect();
        self.rows = menu.rows;
        self.tab = on.and_then(|on| self.tabs.iter().position(|t| *t == on)).unwrap_or(0);
        self.game_edit = Config::from_ini(&self.ini_text).game_dir.map(|p| p.display().to_string()).unwrap_or_default();
        self.ini = IniForm::read(&self.ini_text);
        // None set: the box starts on Steam's usual folder when the game is there; it is
        // stored only by Save (#2938).
        if self.game_edit.is_empty() {
            let usual = launch::usual_game_folders(launch::user_home().as_deref(), cfg!(windows));
            if let Some(f) = launch::first_usable(&usual, |p| GameInstall::open(p).is_ok()) {
                self.game_edit = f.display().to_string();
            }
        }
        let configured = ini_get(&self.ini_text, LAUNCHER, "Exe").filter(|s| !s.is_empty()).map(PathBuf::from);
        self.exe = launch::find_game(configured.as_deref());
        // The DLSS folder is beside the game this INI starts (crate::dlss::folder_for).
        self.dlss = crate::dlss::Status::read(self.exe.as_deref());
    }

    fn launcher_flag(&self, key: &str, default: bool) -> bool {
        ini_get(&self.ini_text, LAUNCHER, key).map_or(default, |v| v != "0")
    }

    fn game(&self) -> Option<GameInstall> {
        launch::game_from_ini(&self.ini_text)
    }

    /// Whether the game would find its folder when started now ([`launch::folder_known`]).
    fn folder_known(&self) -> bool {
        launch::folder_known(self.game().is_some(), fnv_install::mo2::usvfs_loaded(), std::env::var_os("NVFOREVER_DATA").as_deref())
    }

    /// Read the latest file first, keeping every other line.
    fn write_ini(&self, edit: impl FnOnce(String) -> String) -> Result<(), String> {
        let path = self.ini_path().ok_or("no home folder (Documents) found")?;
        let now = std::fs::read_to_string(&path).unwrap_or_default();
        let text = edit(now.clone());
        if text == now {
            return Ok(());
        }
        if path.is_file() {
            let bak = path.with_file_name("NVForever.ini.bak");
            guard_write(&bak, self.game().as_ref()).map_err(|e| e.to_string())?;
            std::fs::copy(&path, &bak).map_err(|e| format!("{}: {e}", bak.display()))?;
        }
        launch::write_ini(&path, |_| text)
    }

    fn set_launcher_key(&mut self, key: &str, value: &str) {
        let r = self.write_ini(|t| ini_set(&t, LAUNCHER, key, value));
        self.after_write(r, None);
    }

    fn after_write(&mut self, r: Result<(), String>, done: Option<&str>) {
        let edits = std::mem::take(&mut self.edits);
        match r {
            Ok(()) => {
                self.reload_ini();
                if let Some(d) = done {
                    self.message = Some((d.to_string(), false));
                }
            }
            Err(e) => {
                self.edits = edits;
                self.message = Some((e, true));
            }
        }
    }

    // ---- Saves and news ----

    fn find_newest_save(&mut self) {
        let Some(h) = &self.home else { return };
        self.newest = fnv_save::list(&h.saves()).into_iter().next().map(|(path, hd)| {
            let shot = fnv_save::read_header(&path).ok().map(|s| s.shot).filter(|s| !s.is_empty()).map(Arc::from);
            let name = if hd.player_name.is_empty() { path.file_stem().unwrap_or_default().to_string_lossy().into_owned() } else { hd.player_name.clone() };
            let hours = hd.play_seconds / 3600.0;
            let mut parts = vec![format!("Level {}", hd.level)];
            parts.extend(shown_place(&hd.location).map(str::to_string));
            parts.push(format!("{hours:.0} h played"));
            let detail = parts.join(" · ");
            NewestSave { path, name, detail, shot }
        });
    }

    fn fetch_news(&mut self, ctx: &egui::Context) {
        let (tx, rx) = channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(news::fetch());
            c.request_repaint();
        });
        self.news_rx = Some(rx);
        let (tx, rx) = channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(news::fetch_next_release());
            c.request_repaint();
        });
        self.next_rx = Some(rx);
        let (tx, rx) = channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(news::fetch_latest());
            c.request_repaint();
        });
        self.latest_rx = Some(rx);
        self.news_state = NewsState::Loading;
    }

    fn poll_news(&mut self) {
        if let Some(r) = self.next_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.next_rx = None;
            match r {
                Ok(n) => self.next = n,
                Err(e) => eprintln!("launcher: next release: {e}"),
            }
        }
        if let Some(r) = self.latest_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.latest_rx = None;
            match r {
                Ok(l) => self.latest = l.filter(|l| news::is_newer(&l.version, version())),
                Err(e) => eprintln!("launcher: latest release: {e}"),
            }
        }
        let Some(rx) = &self.news_rx else { return };
        let Ok(r) = rx.try_recv() else { return };
        self.news_rx = None;
        match r {
            Ok((posts, source)) => {
                if let Some(h) = &self.home {
                    news::save_cache(&h.cache(), &posts);
                }
                self.news = posts
                    .into_iter()
                    .map(|mut p| {
                        let img = p.image_bytes.take().map(Arc::from);
                        (p, img)
                    })
                    .collect();
                self.expanded.clear();
                self.news_state = NewsState::Shown(source);
            }
            Err(e) => self.news_state = NewsState::Failed(e),
        }
    }

    // ---- Starting ----

    fn running(&mut self) -> bool {
        match &mut self.child {
            Some(c) => match c.try_wait() {
                Ok(None) => true,
                ended => {
                    self.child = None;
                    self.reload_ini();
                    self.find_newest_save();
                    // An error is said, with where the logs are; a clean exit only clears
                    // "The game is starting…" (#2938).
                    let said = ended.ok().flatten().and_then(|s| launch::exit_message(s.code(), s.success(), self.home.as_ref().map(EngineHome::logs).as_deref()));
                    match said {
                        Some(m) => self.message = Some((m, true)),
                        None if self.message.as_ref().is_some_and(|(m, _)| m == STARTING) => self.message = None,
                        None => {}
                    }
                    false
                }
            },
            None => false,
        }
    }

    fn start(&mut self, ctx: &egui::Context, start: Start) {
        if matches!(self.upd, Upd::Installing | Upd::Closing) {
            self.message = Some(("The update is being installed: play once it is done.".into(), true));
            return;
        }
        if self.play_dirty_ask {
            self.play_dirty_ask = false;
        } else if self.page_unsaved() {
            self.remember_lists();
            if self.page_unsaved() {
                self.message = Some(("The mod list could not be stored, so the game was not started.".into(), true));
                return;
            }
        }
        if !self.edits.is_empty() {
            let r = self.save_settings();
            if r.is_err() {
                self.after_write(r, None);
                return;
            }
            self.after_write(r, None);
        }
        // No game folder: the game would only ask for one (Windows) or quit (elsewhere);
        // the launcher's own page asks instead (#2938).
        if !self.folder_known() {
            self.page = Page::Game;
            self.message = Some((NEEDS_FOLDER_FIRST.into(), true));
            return;
        }
        let Some(exe) = self.exe.clone() else {
            self.message = Some((format!("{} was not found beside the launcher: pick it on the Game folder page.", launch::GAME_EXE), true));
            return;
        };
        let layers = self.home.as_ref().map(|h| plugins::mod_content_layers(&h.mods())).unwrap_or_default();
        match launch::spawn(&exe, &start, &layers) {
            Ok(c) => {
                self.child = Some(c);
                self.message = Some((STARTING.into(), false));
                if self.launcher_flag("CloseOnPlay", false) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Err(e) => self.message = Some((format!("{}: {e}", exe.display()), true)),
        }
    }

    /// Play Co-Op: the plain game at its main menu, as Play's main menu start. Co-op is part
    /// of the game itself (no separate co-op program), run in the game's own home with its
    /// saves, settings and mods; the game's Co-op switch (off by default) turns it on.
    fn start_coop(&mut self, ctx: &egui::Context) {
        self.start(ctx, COOP_START);
    }

    /// The Co-op page: what co-op is, how it is turned on, and Play Co-Op.
    fn coop_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Co-op");
        ui.label(RichText::new(COOP_ABOUT).color(WEAK));
        ui.add_space(8.0);
        let running = self.child.is_some();
        ui.add_enabled_ui(self.exe.is_some() && !running, |ui| {
            if big_button(ui, "Play Co-Op", "the main menu: then host or join", true, 58.0).clicked() {
                self.start_coop(ui.ctx());
            }
        });
        if self.exe.is_none() {
            ui.label(RichText::new(format!("{} was not found beside the launcher: pick it on the Game folder page.", launch::GAME_EXE)).color(BAD));
        }
        if let Some(home) = self.home.as_ref() {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("Co-op uses the game's own saves, settings, mods and logs, in {}", home.root.display())).small().color(WEAK));
        }
    }

    fn save_settings(&mut self) -> Result<(), String> {
        self.write_ini(|text| catalog::with_edits(text, &self.edits))
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let title = match &self.profile_pick {
            Some(name) if self.page == Page::Mods => format!("NVForever Launcher — {name}"),
            _ => "NVForever Launcher".into(),
        };
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(title));
        let ctx = &ui.ctx().clone();
        self.poll_news();
        let running = self.running();
        self.poll_update(ctx, running);
        if self.upd.refuses_close() && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.message = Some(("Installing the update: the launcher closes by itself when it is done.".into(), false));
        }
        if running {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        self.top_bar(ui);
        self.bottom_bar(ui, ctx, running);
        self.nav(ui);
        self.test_shot(ctx);
        self.update_window(ctx, running);
        self.reports_window(ctx, running);
        self.dlss_licence_window(ctx);
        self.installer_window(ctx);
        self.take_dropped(ctx);
        egui::CentralPanel::default().frame(Frame::new().fill(theme::BG).inner_margin(Margin::symmetric(16, 8))).show(ui, |ui| match self.page {
            Page::Home => self.home_page(ui),
            Page::Settings => self.settings_page(ui),
            Page::Mods => self.mods_page(ui),
            Page::Game => {
                let w = ui.available_width();
                egui::ScrollArea::vertical().id_salt("game-page").auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_width(w);
                    self.game_page(ui);
                });
            }
            Page::Coop => self.coop_page(ui),
        });
        self.separator_window(ctx);
        self.quick_window(ctx);
        self.manual_window(ctx);
        self.backup_cut_window(ctx);
        // Last, after everything that can draw a link (a post's Markdown is in the central
        // panel): egui's own link opening would skip the https rule.
        for url in take_links(ctx) {
            launch::open_url(&url);
        }
        self.remember_lists();
    }
}

/// The ticked rows of a manual install. A folder can be set as `<data>`.
fn manual_nodes(ui: &mut egui::Ui, node: &mut crate::fomod::ManFile, prefix: &str, pick: &mut Option<String>, depth: u8, shown: &mut usize) {
    for child in &mut node.children {
        if *shown > 400 {
            ui.label(RichText::new("The rest of the archive stays ticked.").small().color(WEAK));
            return;
        }
        *shown += 1;
        let rel = if prefix.is_empty() { child.name.clone() } else { format!("{prefix}/{}", child.name) };
        let dir = child.dir;
        let row = ui.horizontal(|ui| {
            if ui.checkbox(&mut child.checked, "").changed() {
                let on = child.checked;
                crate::fomod::set_branch(child, on);
            }
            if dir {
                ui.label(RichText::new(&child.name).strong());
            } else {
                ui.label(&child.name);
            }
        });
        if dir {
            let rel_pick = rel.clone();
            row.response.context_menu(|ui| {
                if ui.button("Set as data folder").clicked() {
                    *pick = Some(rel_pick);
                    ui.close();
                }
            });
            if depth > 0 {
                ui.indent(rel.clone(), |ui| {
                    manual_nodes(ui, child, &rel, pick, depth - 1, shown);
                });
            }
        }
    }
}

fn fit_to_screen(ctx: &egui::Context) -> Vec2 {
    let size = ctx.viewport_rect().size();
    Vec2::new((size.x - 32.0).max(280.0), (size.y - 32.0).max(200.0))
}

/// The pages egui was asked to open this frame (links in a post's text), taken out of its
/// output so that only [`launch::open_url`] opens them.
fn take_links(ctx: &egui::Context) -> Vec<String> {
    ctx.output_mut(|o| {
        let mut urls = Vec::new();
        o.commands.retain(|c| match c {
            egui::OutputCommand::OpenUrl(u) => {
                urls.push(u.url.clone());
                false
            }
            _ => true,
        });
        urls
    })
}

impl App {
    fn test_shot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.shot else { return };
        if self.news_rx.is_none() {
            *frames += 1;
        }
        if *frames == 20 {
            let (path, ctx2) = (path.clone(), ctx.clone());
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                let [w, h] = img.size;
                let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                match image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8) {
                    Ok(()) => println!("launcher: screenshot saved to {}", path.display()),
                    Err(e) => eprintln!("launcher: screenshot {}: {e}", path.display()),
                }
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
                ctx2.request_repaint();
            })));
        }
        ctx.request_repaint();
    }
}

// ---- The frame around the pages ----

impl App {
    fn top_bar(&mut self, root: &mut egui::Ui) {
        egui::Panel::top("top").frame(Frame::new().fill(theme::PANEL).inner_margin(Margin::symmetric(16, 8)).stroke(Stroke::new(1.0, LINE))).show(root, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::from_bytes("bytes://nvf-logo.png", LOGO).fit_to_exact_size(Vec2::splat(44.0)));
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.label(RichText::new("NVForever").font(crate::fonts::title(22.0)).color(GOLD));
                    ui.label(RichText::new("Launcher").small().color(WEAK));
                });
                self.countdown(ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(h) = &self.home {
                        if ui.small_button("Open folder").on_hover_text("Open NVForever's folder (settings, saves, logs)").clicked() {
                            launch::open_folder(&h.root);
                        }
                    }
                    if let Some(l) = &self.latest {
                        let b = egui::Button::new(RichText::new(format!("Update to v{}", l.version)).strong().color(theme::ON_GOLD)).fill(GOLD);
                        if ui.add(b).on_hover_text(format!("v{} is out (this is v{})", l.version, version())).clicked() {
                            self.upd_open = true;
                        }
                    }
                });
            });
        });
    }

    /// The site's next-release countdown in the middle of the top bar, as the site's own bar
    /// shows it; nothing when the site announces none or the moment has passed.
    fn countdown(&self, ui: &mut egui::Ui) {
        let Some(n) = &self.next else { return };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        let (lead, time) = match n.at {
            Some(at) if at > now => (format!("NVForever {} releases in", n.version), Some(news::countdown_text(at - now))),
            Some(_) => return,
            None => (format!("NVForever {} is coming soon", n.version), None),
        };
        // The middle of the whole bar, not of what the row has left after the logo.
        let rect = ui.clip_rect();
        let p = ui.painter();
        let lead_g = p.layout_no_wrap(lead, egui::FontId::proportional(15.0), WEAK);
        let time_g = time.map(|t| p.layout_no_wrap(t, egui::FontId::proportional(18.0), GOLD));
        let w = lead_g.size().x + time_g.as_ref().map_or(0.0, |g| g.size().x + 12.0) + 40.0;
        let pill = egui::Rect::from_center_size(rect.center(), Vec2::new(w, 34.0));
        p.rect(pill, CornerRadius::same(17), theme::SELECTED, Stroke::new(1.0, theme::GOLD_DIM), egui::StrokeKind::Inside);
        let lead_w = lead_g.size().x;
        p.galley(egui::pos2(pill.left() + 16.0, pill.center().y - lead_g.size().y / 2.0), lead_g, WEAK);
        if let Some(g) = time_g {
            p.galley(egui::pos2(pill.left() + 28.0 + lead_w, pill.center().y - g.size().y / 2.0), g, GOLD);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(500));
    }

    fn nav(&mut self, root: &mut egui::Ui) {
        egui::Panel::left("nav").exact_size(210.0).resizable(false).frame(Frame::new().fill(theme::PANEL).inner_margin(Margin::same(12))).show(root, |ui| {
            egui::ScrollArea::vertical().id_salt("nav").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(4.0);
            for (page, name) in [(Page::Home, "HOME"), (Page::Settings, "SETTINGS"), (Page::Mods, "MODS"), (Page::Game, "GAME FOLDER"), (Page::Coop, "CO-OP")] {
                if nav_button(ui, name, self.page == page).clicked() {
                    self.page = page;
                    self.message = None;
                    if page == Page::Mods {
                        self.enter_mods();
                    }
                }
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            if nav_button(ui, "WEBSITE", false).on_hover_text(news::SITE).clicked() {
                launch::open_url(news::SITE);
            }
            if nav_button(ui, "GITHUB", false).on_hover_text(REPO).clicked() {
                launch::open_url(REPO);
            }
            });
        });
    }

    fn bottom_bar(&mut self, root: &mut egui::Ui, ctx: &egui::Context, running: bool) {
        let pad_y: i8 = 4;
        let h = self.bar_h.clamp(48.0, 160.0);
        egui::Panel::bottom("bottom").exact_size(h).resizable(false).frame(Frame::new().fill(theme::PANEL).inner_margin(Margin::symmetric(12, pad_y)).stroke(Stroke::new(1.0, LINE))).show(root, |ui| {
            // The top edge resizes the bar. There is no drawn grip; the buttons fill the rest.
            let (grip, grip_resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 4.0), Sense::drag());
            if grip_resp.hovered() || grip_resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            let _ = grip;
            if grip_resp.dragged() {
                self.bar_h = (self.bar_h - grip_resp.drag_delta().y).clamp(48.0, 160.0);
            }
            if grip_resp.double_clicked() {
                self.bar_h = 56.0;
                self.set_launcher_key("BottomBar", "56");
            } else if grip_resp.drag_stopped() {
                self.set_launcher_key("BottomBar", &format!("{:.0}", self.bar_h));
            }
            let bar_h = ui.available_height().clamp(36.0, 148.0);
            let status = self.message.clone();
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("v{}", version())).size((bar_h * 0.42).clamp(10.0, 22.0)).strong().color(WEAK));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled_ui(!running, |ui| {
                        ui.menu_button(RichText::new("More").size((bar_h * 0.26).clamp(10.0, 28.0)), |ui| {
                            if ui.button("New game").clicked() {
                                self.start(ctx, Start::NewGame);
                                ui.close();
                            }
                            if ui.button("Main menu").clicked() {
                                self.start(ctx, Start::MainMenu);
                                ui.close();
                            }
                            if ui.button("Explorer (free camera)").clicked() {
                                self.start(ctx, Start::Explorer);
                                ui.close();
                            }
                            ui.separator();
                            let mut close = self.launcher_flag("CloseOnPlay", false);
                            if ui.checkbox(&mut close, "Close the launcher when the game starts").changed() {
                                self.set_launcher_key("CloseOnPlay", if close { "1" } else { "0" });
                            }
                        });
                        let (title, sub, start) = match &self.newest {
                            _ if running => ("Running…".to_string(), "the game is open".to_string(), None),
                            Some(s) => ("Continue".to_string(), format!("{} · {}", s.name, s.detail), Some(Start::Load(s.path.clone()))),
                            None => ("Play".to_string(), "new game from the main menu".to_string(), Some(Start::MainMenu)),
                        };
                        if big_button(ui, &title, &sub, true, bar_h).clicked() {
                            if let Some(s) = start {
                                self.start(ctx, s);
                            }
                        }
                        if self.exe.is_some() && !running && big_button(ui, "Play Co-Op", "the main menu: then host or join", false, bar_h).clicked() {
                            self.start_coop(ctx);
                        }
                        if self.newest.is_some() && !running && big_button(ui, "Main menu", "load or start a game", false, bar_h).clicked() {
                            self.start(ctx, Start::MainMenu);
                        }
                        if let Some(s) = self.newest.clone() {
                            if let Some(shot) = s.shot.clone() {
                                let uri = format!("bytes://{}", s.path.file_name().and_then(|n| n.to_str()).unwrap_or("save"));
                                let resp = ui.add(egui::Image::from_bytes(uri, Bytes::Shared(shot)).fit_to_exact_size(Vec2::new(bar_h * 1.7, bar_h * 0.95)).corner_radius(3).sense(Sense::click())).on_hover_text("Load this save");
                                if resp.clicked() {
                                    self.start(ctx, Start::Load(s.path));
                                }
                            }
                        }
                    });
                    if let Some((m, bad)) = status {
                        let width = ui.available_width().max(32.0);
                        let color = if bad { BAD } else { GOOD };
                        ui.allocate_ui_with_layout(Vec2::new(width, ui.available_height().max(18.0)), Layout::left_to_right(Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(&m).small().color(color)).wrap()).on_hover_text(m);
                        });
                    }
                });
            });
        });
    }
}

const REPO: &str = "https://github.com/LootJunkieAzza/NVForever";

/// A drop that did not come with a real path (the name only). The bytes are written
/// aside and installed from there, so a name alone never becomes an empty mod folder.
/// The folder is a new private one (`fomod::make_temp`), so no other user can have put a
/// link or another file there first.
fn write_dropped_bytes(name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let dir = crate::fomod::make_temp()?;
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// How Play Co-Op starts the game: the same program and main menu start as Play, nothing
/// co-op on the command line, so co-op stays as the game's own Co-op switch has it [design].
const COOP_START: Start = Start::MainMenu;

/// The Co-op page's text: the game's own menu names (nvf-play `lobby.rs`, `main_menu.rs`, `pause.rs`).
const COOP_ABOUT: &str = "Play the Mojave with friends: one player hosts a lobby and the others join it with its code. Co-op is off until you turn it on in the game's Settings, under Gameplay, with the Co-op row. The host then loads a game, opens the pause menu's Co-op and picks Create Lobby; friends (they turn Co-op on too) pick Join Lobby on their main menu and type the code.";

/// A page button of the left bar; the open page's has a gold edge and mark.
fn load_mod_rows(home: &EngineHome) -> Vec<ModRow> {
    let mods = home.mods();
    let mut found = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&mods) {
        for entry in rd.flatten() {
            let path = entry.path();
            // A directory link is a mod folder. A file is not.
            if !path.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else { continue };
            if name.starts_with('.') || crate::modlist::is_tool_dir(&name) {
                continue;
            }
            found.push(name);
        }
    }
    found.sort_by_key(|a| a.to_ascii_lowercase());
    let text = std::fs::read_to_string(home.root.join("mod-order.txt")).unwrap_or_default();
    let mut rows = fnv_install::mod_order::read_mod_order(&text);
    let locks = std::fs::read_to_string(home.root.join(HOME_LOCKS)).unwrap_or_default();
    fnv_install::mod_order::apply_locks(&mut rows, &fnv_install::mod_order::read_locks(&locks));
    let mut order: Vec<ModRow> = Vec::new();
    for row in rows {
        if row.separator {
            order.push(row);
            continue;
        }
        if let Some(real) = found.iter().find(|n| n.eq_ignore_ascii_case(&row.name)) {
            if !order.iter().any(|m| !m.separator && m.name.eq_ignore_ascii_case(real)) {
                order.push(ModRow { name: real.clone(), ..row });
            }
        }
    }
    for name in found {
        if !order.iter().any(|m| m.name.eq_ignore_ascii_case(&name)) {
            order.push(ModRow::module(name, true));
        }
    }
    order
}

fn mod_file_keys(folder: &Path) -> Vec<String> {
    let root = if folder.join("Data").is_dir() { folder.join("Data") } else { folder.to_path_buf() };
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_symlink() {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(rel) = path.strip_prefix(&root) else { continue };
            let key = rel.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
            if !key.is_empty() {
                out.push(key);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn note_files(files: &HashMap<String, Vec<String>>, folder: &str, plugin: &str) -> String {
    let key = plugin.to_ascii_lowercase();
    files.get(folder).and_then(|list| list.iter().find(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).eq_ignore_ascii_case(&key)).cloned()).unwrap_or_else(|| plugin.to_string())
}

fn clash_notes(rows: &[(String, bool)], files: &HashMap<String, Vec<String>>) -> HashMap<String, Vec<Clash>> {
    let mut owners: HashMap<String, Vec<String>> = HashMap::new();
    for (name, on) in rows {
        if !on {
            continue;
        }
        let Some(list) = files.get(name) else { continue };
        for file in list {
            let who = owners.entry(file.clone()).or_default();
            if who.last().map(String::as_str) != Some(name.as_str()) {
                who.push(name.clone());
            }
        }
    }
    let mut grouped: HashMap<(String, String, bool), Vec<String>> = HashMap::new();
    for (file, who) in owners {
        if who.len() < 2 {
            continue;
        }
        let winner = who.last().cloned().unwrap_or_default();
        for loser in &who[..who.len() - 1] {
            grouped.entry((winner.clone(), loser.clone(), true)).or_default().push(file.clone());
            grouped.entry((loser.clone(), winner.clone(), false)).or_default().push(file.clone());
        }
    }
    let mut out: HashMap<String, Vec<Clash>> = HashMap::new();
    for ((folder, other, wins), mut found) in grouped {
        found.sort();
        out.entry(folder).or_default().push(Clash { other, files: found, wins });
    }
    for notes in out.values_mut() {
        notes.sort_by(|a, b| b.wins.cmp(&a.wins).then_with(|| a.other.cmp(&b.other)));
    }
    out
}

fn nav_button(ui: &mut egui::Ui, name: &str, on: bool) -> egui::Response {
    let text = RichText::new(name).font(crate::fonts::title(15.0)).color(if on { GOLD } else { theme::TEXT });
    let b = egui::Button::new(text)
        .min_size(Vec2::new(ui.available_width(), 42.0))
        .fill(if on { theme::SELECTED } else { theme::BUTTON })
        .stroke(Stroke::new(1.0, if on { theme::GOLD_DIM } else { LINE }));
    let r = ui.add(b);
    if on {
        let bar = egui::Rect::from_min_size(r.rect.left_top() + Vec2::new(0.0, 8.0), Vec2::new(3.0, r.rect.height() - 16.0));
        ui.painter().rect_filled(bar, CornerRadius::same(1), GOLD);
    }
    r
}

/// A button in the site's primary style when `on` (gold, dark words, 9:1), else a plain one.
fn gold_button(text: &str, on: bool) -> egui::Button<'static> {
    let words = RichText::new(text.to_string()).strong();
    if on {
        egui::Button::new(words.color(theme::ON_GOLD)).fill(GOLD)
    } else {
        egui::Button::new(words)
    }
}

/// A large two-line button (title and a small line under it), gold when `primary`.
fn big_button(ui: &mut egui::Ui, title: &str, sub: &str, primary: bool, height: f32) -> egui::Response {
    let height = height.clamp(12.0, 180.0);
    let size = Vec2::new(if primary { height * 4.8 } else { height * 3.3 }, height);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let fill = match (primary, resp.hovered() && enabled) {
        (true, true) => theme::GOLD_HOVER,
        (true, false) => GOLD,
        (false, true) => theme::BUTTON_HOVER,
        (false, false) => theme::BUTTON,
    };
    let fill = if enabled { fill } else { fill.gamma_multiply(0.5) };
    let ink = if primary { theme::ON_GOLD } else { theme::TEXT };
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(4), fill, Stroke::new(1.0, if primary { GOLD } else { LINE }), egui::StrokeKind::Inside);
    let pad_x = (height * 0.14).clamp(6.0, 18.0);
    let icon = (height * 0.15).clamp(5.0, 13.0);
    let left = rect.left() + pad_x;
    let c = egui::pos2(left + icon, rect.center().y);
    p.add(egui::Shape::convex_polygon(
        vec![c + Vec2::new(-icon * 0.78, -icon), c + Vec2::new(icon, 0.0), c + Vec2::new(-icon * 0.78, icon)],
        ink,
        Stroke::NONE,
    ));
    let text_x = left + icon * 2.1 + (height * 0.06).clamp(3.0, 8.0);
    let max_w = (rect.right() - pad_x - text_x).max(8.0);
    let title_px = (height * 0.32).clamp(10.0, 30.0);
    let sub_px = (height * 0.18).clamp(8.0, 18.0);
    let (title_draw, title_font) = fit_button_line(p, title, crate::fonts::title(title_px), ink, max_w);
    let (sub_draw, sub_font) = fit_button_line(p, sub, egui::FontId::proportional(sub_px), ink, max_w);
    p.text(egui::pos2(text_x, rect.top() + height * 0.10), egui::Align2::LEFT_TOP, title_draw, title_font, ink);
    p.text(
        egui::pos2(text_x, rect.bottom() - height * 0.08),
        egui::Align2::LEFT_BOTTOM,
        sub_draw,
        sub_font,
        ink.gamma_multiply(0.8),
    );
    resp
}

/// Shrink `text` to `max_w`, then cut it only if the smallest size still does not fit.
fn fit_button_line(p: &egui::Painter, text: &str, mut font: egui::FontId, color: egui::Color32, max_w: f32) -> (String, egui::FontId) {
    let min = 8.0;
    while font.size > min && p.layout_no_wrap(text.to_owned(), font.clone(), color).size().x > max_w {
        font.size = (font.size - 0.5).max(min);
    }
    let mut chars: Vec<char> = text.chars().collect();
    let mut shown = text.to_owned();
    while !chars.is_empty() && p.layout_no_wrap(shown.clone(), font.clone(), color).size().x > max_w {
        chars.pop();
        let stem: String = chars.iter().collect();
        shown = format!("{}…", stem.trim_end());
        if chars.is_empty() {
            break;
        }
    }
    (shown, font)
}

/// An on/off switch.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = Vec2::new(44.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let bg = theme::BUTTON_ACTIVE.lerp_to_gamma(GOLD, t);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(11), bg);
    let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
    p.circle_filled(egui::pos2(x, rect.center().y), 8.0, theme::TEXT);
    resp
}

/// The part of 0..1 that `text`, typed into a slider of the evenly spread `words`, stands
/// for: one of the words (any case), else a number; `None`: neither.
fn word_part(words: &[String], text: &str) -> Option<f64> {
    let t = text.trim();
    let n = words.len();
    match words.iter().position(|w| w.trim().eq_ignore_ascii_case(t)) {
        Some(i) if n > 1 => Some(i as f64 / (n - 1) as f64),
        Some(_) => Some(0.0),
        None => t.parse::<f64>().ok().filter(|v| v.is_finite()),
    }
}

/// Whether the INI's value `v` is one the game's row kind `kind` takes (else the page goes by
/// the value, as for a row with no kind).
fn menu_fits(kind: &menu::Kind, v: &str) -> bool {
    match kind {
        menu::Kind::Switch => matches!(v.trim(), "0" | "1"),
        menu::Kind::Slider { .. } => v.trim().parse::<f32>().is_ok(),
        menu::Kind::List { choices } => choices.iter().any(|c| c.name == v),
    }
}

/// The control for a row of kind `kind` (the game's menu file, src/catalog.rs), drawn as the
/// page draws the other rows; the new value when it changed.
fn menu_control(ui: &mut egui::Ui, key: &str, kind: &menu::Kind, v: &str, control_w: f32) -> Option<String> {
    match kind {
        menu::Kind::Switch => {
            let mut on = v.trim() == "1";
            toggle(ui, &mut on).changed().then(|| if on { "1" } else { "0" }.to_string())
        }
        menu::Kind::Slider { min, step, steps } => {
            // A part of 0..1 whatever the file says (a hand edit, a broken write).
            let min = &if min.is_finite() { min.clamp(0.0, 1.0) } else { 0.0 };
            let mut f: f32 = v.trim().parse().unwrap_or(*min);
            // No egui steps: egui counts them from the range's start, the game from 0
            // (`Slider::fit`), so a value of the game's would move by being drawn. A value the
            // player moves is fitted as the game fits it; one drawn stays as the INI has it.
            let s = egui::Slider::new(&mut f, *min..=1.0).clamping(egui::SliderClamping::Edits);
            let s = if steps.is_empty() {
                s.custom_formatter(|x, _| format!("{:.0}%", x * 100.0)).custom_parser(|s| s.trim().trim_end_matches('%').parse::<f64>().ok().map(|x| x / 100.0))
            } else {
                let (shown, typed) = (steps.clone(), steps.clone());
                // Typed into it: a step's words (any case), else a number.
                s.custom_formatter(move |x, _| menu::Step::nearest(&shown, x as f32).unwrap_or_default().to_string())
                    .custom_parser(move |t| menu::Step::parse(&typed, t).map(f64::from))
            };
            ui.add_sized([control_w, 20.0], s).changed().then(|| part_text(game_fit(f, *min, *step)))
        }
        menu::Kind::List { choices } => {
            let shown = choices.iter().find(|c| c.name == v).map_or(v.to_string(), |c| c.label.clone());
            let mut picked = None;
            egui::ComboBox::from_id_salt(key).width(control_w - 8.0).selected_text(shown).show_ui(ui, |ui| {
                for c in choices {
                    if ui.selectable_label(c.name == v, &c.label).clicked() && c.name != v {
                        picked = Some(c.name.clone());
                    }
                }
            });
            picked
        }
    }
}

/// `v` brought into `min..=1` and onto a step of `step` from 0, to the thousandth, as the game
/// fits a slider's value (engine/nvf-play/src/settings.rs, `Slider::fit`).
fn game_fit(v: f32, min: f32, step: f32) -> f32 {
    let step = step.max(1e-3);
    let v = (v.clamp(min, 1.0) / step).round() * step;
    ((v * 1000.0).round() / 1000.0).clamp(min, 1.0)
}

/// A slider part (`0.50`) as the INI keeps it: to the thousandth, at least two places.
fn part_text(v: f32) -> String {
    let s = format!("{:.3}", v.clamp(0.0, 1.0));
    if s.ends_with('0') {
        s[..s.len() - 1].to_string()
    } else {
        s
    }
}

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new().fill(CARD).corner_radius(6).stroke(Stroke::new(1.0, LINE)).inner_margin(Margin::same(16)).show(ui, add).inner
}

// ---- The pages ----

impl App {
    fn home_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("News");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let on = self.launcher_flag("News", true);
                if on && !matches!(self.news_state, NewsState::Loading) && ui.small_button("Refresh").clicked() {
                    self.fetch_news(ui.ctx());
                }
                let mut fetch = on;
                if ui.checkbox(&mut fetch, RichText::new("Fetch news").small()).on_hover_text("Ask nv-forever.com (and GitHub) for the news when the launcher opens").changed() {
                    self.set_launcher_key("News", if fetch { "1" } else { "0" });
                    if fetch {
                        self.fetch_news(ui.ctx());
                    } else {
                        self.news_state = NewsState::Off;
                    }
                }
                let note = match &self.news_state {
                    NewsState::Off => "news fetching is off".to_string(),
                    NewsState::Loading => "fetching…".to_string(),
                    NewsState::Shown(Source::Site) => format!("{} posts from nv-forever.com", self.news.len()),
                    NewsState::Shown(Source::Releases) => format!("{} releases from GitHub", self.news.len()),
                    NewsState::Shown(Source::Cache) => "saved posts".to_string(),
                    NewsState::Failed(_) => "offline: showing saved posts".to_string(),
                };
                let l = ui.label(RichText::new(note).small().color(WEAK));
                if let NewsState::Failed(e) = &self.news_state {
                    l.on_hover_text(e);
                }
            });
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if self.news.is_empty() {
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("No news yet").size(20.0).color(GOLD));
                    ui.label("Posts from nv-forever.com and the release notes show here once the launcher has fetched them.");
                });
            }
            for i in 0..self.news.len() {
                let (post, img) = &self.news[i];
                let (post, img) = (post.clone(), img.clone());
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(bytes) = img {
                        let w = ui.available_width();
                        ui.add(egui::Image::from_bytes(format!("bytes://news/{i}/{}", post.title), Bytes::Shared(bytes)).max_width(w).max_height(360.0).corner_radius(4));
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&post.title).font(crate::fonts::title(24.0)).color(GOLD));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let by = [post.shown_date(), post.author.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                            ui.label(RichText::new(by).small().color(WEAK));
                        });
                    });
                    let open = self.expanded.contains(&i);
                    let (text, cut) = preview(&post.body, open);
                    // Room between lines and paragraphs: a bigger body size (the font's own
                    // line height grows with it) and more space between blocks.
                    ui.scope(|ui| {
                        let s = ui.style_mut();
                        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
                        s.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(14.0));
                        s.spacing.item_spacing.y = 10.0;
                        CommonMarkViewer::new().show(ui, &mut self.md, text);
                    });
                    ui.horizontal(|ui| {
                        if cut && ui.button(if open { "Show less" } else { "Read more" }).clicked() {
                            if open {
                                self.expanded.remove(&i);
                            } else {
                                self.expanded.insert(i);
                            }
                        }
                        if let Some(url) = &post.url {
                            if ui.link("Open in the browser").clicked() {
                                launch::open_url(url);
                            }
                        }
                    });
                });
                ui.add_space(12.0);
            }
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Settings");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let dirty = !self.edits.is_empty();
                if ui.add_enabled(dirty, gold_button("Save", dirty)).clicked() {
                    let r = self.save_settings();
                    self.after_write(r, Some("Settings saved: the game uses them at its next start."));
                }
                if ui.add_enabled(dirty, egui::Button::new("Undo changes")).clicked() {
                    self.edits.clear();
                }
                if dirty {
                    ui.label(RichText::new(format!("{} changed", self.edits.len())).small().color(GOLD));
                }
            });
        });
        ui.label(
            RichText::new("The game's own settings page, kept in NVForever.ini. A row the game has not written yet shows its default once you change it in the game.")
                .small()
                .color(WEAK),
        );
        ui.horizontal_wrapped(|ui| {
            for (i, t) in self.tab_labels.clone().iter().enumerate() {
                let on = self.tab == i;
                let b = egui::Button::new(RichText::new(t).strong().color(if on { theme::ON_GOLD } else { theme::TEXT })).fill(if on { GOLD } else { Color32::TRANSPARENT }).stroke(Stroke::NONE);
                if ui.add(b).clicked() {
                    self.tab = i;
                }
            }
        });
        ui.separator();
        let tab = self.tabs.get(self.tab).cloned();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(tab) = tab {
                if tab == "Graphics" {
                    self.dlss_card(ui);
                }
                // A row with no words built in (a Tweaks row before the game wrote its menu) only
                // while the INI holds it.
                let rows: Vec<Row> = self.rows.iter().filter(|r| r.tab == tab && (!r.unlisted || self.stored.contains_key(&r.key))).cloned().collect();
                let mut heading: Option<Option<String>> = None;
                for r in rows {
                    // The game's rules: hidden while the row it goes with is not on its
                    // choice, greyed (with the game's reason) while a row it needs is off.
                    // No tab the launcher shows has only hidden rows (only the game's own
                    // Controls tab can), so a tab is never left empty by them.
                    if r.shown_with.as_ref().is_some_and(|rule| !self.holds(rule)) {
                        continue;
                    }
                    let greyed = r.needs.iter().find(|rule| !self.holds(rule)).map(|rule| rule.why.clone());
                    if let Some(colour) = crate::display::colour_of_part(&r.key) {
                        if !self.current(colour).is_some_and(|c| c.eq_ignore_ascii_case(crate::display::CUSTOM)) {
                            continue;
                        }
                    }
                    if heading.as_ref() != Some(&r.heading) {
                        if let Some(h) = &r.heading {
                            ui.add_space(8.0);
                            ui.label(RichText::new(h.to_uppercase()).small().strong().color(theme::GOLD_TEXT));
                        }
                        heading = Some(r.heading.clone());
                    }
                    self.setting_row(ui, &r.key, &r.label, &r.note, r.kind.as_ref(), greyed.as_deref());
                }
            }
        });
    }

    fn backup_max_picker(&mut self, ui: &mut egui::Ui) {
        let files = self.home.as_ref().map(plugin_backups).unwrap_or_default();
        egui::ComboBox::from_id_salt("plugin-backup-max").selected_text(self.backup_max.to_string()).show_ui(ui, |ui| {
            for n in BACKUP_MAX {
                if ui.selectable_label(self.backup_max == *n, n.to_string()).clicked() && self.backup_max != *n {
                    let extra = files.len().saturating_sub(*n as usize);
                    if *n < self.backup_max && extra > 0 {
                        self.backup_cut = Some(*n);
                    } else {
                        self.set_backup_max(*n);
                    }
                }
            }
        });
    }


    /// The NVForever.ini card's `[Launcher] Exe` box.
    fn exe_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Game program").small()).on_hover_text("[Launcher] Exe. Empty uses the program beside the launcher.");
            ui.add(egui::TextEdit::singleline(&mut self.ini.exe).hint_text("beside the launcher").desired_width((ui.available_width() - 160.0).max(120.0)));
            if cfg!(windows) && ui.button("Choose…").clicked() {
                if let Some(f) = pick_file("nvforever") {
                    self.ini.exe = f.display().to_string();
                }
            }
        });
    }

    /// The Game folder page's NVForever.ini card: the new game's start (#4313), the `[Launcher]`
    /// keys this launcher reads, the game-written input device, the file's place, and the
    /// card's Save.
    fn ini_card(&mut self, ui: &mut egui::Ui) {
        if self.home.is_none() {
            ui.label("No home folder was found, so NVForever.ini cannot be written.");
            return;
        }
        let ttw = self.ttw_active();
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            self.start_row(ui, ttw);
            ui.checkbox(&mut self.ini.news, "Fetch news").on_hover_text("[Launcher] News. Ask nv-forever.com (and GitHub) for the news when the launcher opens.");
            ui.checkbox(&mut self.ini.close, "Close the launcher when the game starts").on_hover_text("[Launcher] CloseOnPlay.");
            self.exe_row(ui);
            let device = ini_get(&self.ini_text, "Controls", "Input Device").filter(|s| !s.is_empty()).unwrap_or_else(|| "not set".into());
            ui.label(RichText::new(format!("Input device: {device}")).small().color(WEAK)).on_hover_text("[Controls] Input Device. The game writes this. It is not changed here.");
            if let Some(path) = self.ini_path() {
                ui.label(RichText::new(path.display().to_string()).small().color(WEAK)).on_hover_text("The file these values are saved in. Its folder is NVFOREVER_HOME when that is set, else Documents\\My Games\\NVForever on Windows, ~/Library/Application Support/NVForever on macOS, and ~/.local/share/nvforever (or $XDG_DATA_HOME/nvforever) on Linux.");
            }
            let dirty = self.ini_form_dirty();
            ui.horizontal(|ui| {
                if ui.add_enabled(dirty, gold_button("Save", dirty)).on_hover_text("Writes the values you changed into NVForever.ini.").clicked() {
                    let ctx = ui.ctx().clone();
                    self.save_ini_form(&ctx);
                }
                ui.label(RichText::new("Other lines in the file are left as they are.").small().color(WEAK));
            });
        });
    }

    /// The card's first row, the new game's start (src/start.rs, #2945, moved here from the
    /// Mods page by #4313): the TTW on/off switch (on is TTW's start, off the base game's,
    /// quest `00102037` and `FNVIntro.bik`), a "custom" tag for another mod's start, and
    /// Custom…; #3229's warning under it while it applies. The card's Save writes it.
    fn start_row(&mut self, ui: &mut egui::Ui, ttw: Option<bool>) {
        use crate::start;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("New game start").strong()).on_hover_text("For mods that start somewhere else (their install guide gives the values). TTW off is the New Vegas opening and its film. Saved to NVForever.ini's [General].");
            let mut ttw_on = self.ini.start.is_ttw();
            let label = if ttw_on { "TTW on" } else { "TTW off" };
            if ui.checkbox(&mut ttw_on, label).on_hover_text("On: quest 001FFFF8 and no intro film. Off: the base game, quest 00102037 and FNVIntro.bik. Save to keep it.").changed() {
                self.ini.start = if ttw_on { start::Fields::ttw() } else { start::Fields::original() };
            }
            if self.ini.start.is_custom() {
                ui.label(RichText::new("custom").small().color(GOLD)).on_hover_text("Another mod's start quest or intro film, set with Custom…");
            }
            if ui.small_button("Custom…").on_hover_text("Another mod's start quest or intro film (.bik)").clicked() {
                self.start_custom = Some(self.ini.start.clone());
            }
        });
        if let Some(why) = start::ttw_missing(&self.ini.start, ttw) {
            ui.label(RichText::new(why).small().color(BAD));
        }
        self.start_custom_window(ui.ctx());
    }

    /// Whether TTW's master is on in the load order (`None`: not in it). The Mods page's list
    /// when it has been read; else `plugins.txt` as it is on disk, read without the Mods page's
    /// sync, so opening the Game folder page writes nothing (#4313).
    fn ttw_active(&mut self) -> Option<bool> {
        if self.mods.is_none() {
            if let Some(home) = &self.home {
                // Read once: no file is an empty list (TTW not in it), not a read every frame.
                let text = std::fs::read_to_string(home.plugins_file()).unwrap_or_default();
                self.mods = Some(plugins::PluginList::parse(&text));
            }
        }
        let list = self.mods.as_ref()?;
        list.entries.iter().find(|e| e.name.eq_ignore_ascii_case(crate::start::TTW_PLUGIN)).map(|e| e.active)
    }

    fn ini_form_dirty(&self) -> bool {
        self.ini.trimmed() != IniForm::read(&self.ini_text)
    }

    /// The card's Save: the changed keys over the file as it is on disk ([`IniForm::apply`]).
    fn save_ini_form(&mut self, ctx: &egui::Context) {
        let was = IniForm::read(&self.ini_text);
        let form = self.ini.trimmed();
        if let Err(e) = form.check(&was, |f| PathBuf::from(f).is_file()) {
            self.message = Some((e, true));
            return;
        }
        let r = self.write_ini(|t| form.apply(&was, &t));
        let ok = r.is_ok();
        // Changes not saved yet stay: the Settings page's, and the game folder box above on this
        // page, which the reload after a write would otherwise drop unseen.
        let pending = self.edits.clone();
        let folder = self.game_edit.clone();
        self.after_write(r, Some("NVForever.ini saved."));
        if ok {
            self.edits = pending;
            self.game_edit = folder;
        }
        // As the News card's box does: on fetches the news now, off stops showing it.
        if ok && form.news != was.news {
            if form.news {
                self.fetch_news(ctx);
            } else {
                self.news_state = NewsState::Off;
            }
        }
    }

    fn setting_row(&mut self, ui: &mut egui::Ui, key: &str, label: &str, note: &str, kind: Option<&menu::Kind>, greyed: Option<&str>) {
        let stored = self.stored.get(key).cloned();
        let value = self.edits.get(key).cloned().or_else(|| stored.clone());
        Frame::new().fill(CARD).corner_radius(4).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // Greyed as the game greys it: shown, its value kept, not changed here.
                if greyed.is_some() {
                    ui.disable();
                }
                let control_w = 260.0;
                // Resolution's two lists carry their names to the left: room for them.
                let column_w = if key == "resolution" { control_w + 90.0 } else { control_w };
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - column_w - 16.0).max(200.0));
                    let changed = self.edits.contains_key(key);
                    ui.label(RichText::new(if changed { format!("{label}  •") } else { label.to_string() }).strong().color(if changed { GOLD } else { theme::TEXT }));
                    if !note.is_empty() {
                        ui.label(RichText::new(note).small().color(WEAK));
                    }
                    if let Some(why) = greyed {
                        ui.label(RichText::new(format!("Greyed out: {why}")).small().color(WEAK));
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if key == "reports-delete" {
                        self.reports_control(ui);
                        return;
                    }
                    let Some(v) = value else {
                        ui.label(RichText::new("game default").small().color(WEAK)).on_hover_text("Not in NVForever.ini yet: change it once on the game's settings page");
                        return;
                    };
                    // Not a value: the game shows what was sent and offers "Delete them now"; the
                    // INI only ever holds "kept". The launcher shows the state (the same file).
                    if key == "reports-delete" {
                        self.reports_control(ui);
                        return;
                    }
                    if crate::display::read_only(key) {
                        ui.label(RichText::new("set in the game").small().color(WEAK))
                            .on_hover_text("The game works this out from the rows below; choosing one there sets them all (a preset from the game folder's files).");
                        ui.label(RichText::new(&v).strong());
                        return;
                    }
                    let new = if key == "resolution" {
                        self.resolution_control(ui, &v, control_w)
                    } else if let Some(kind) = kind.filter(|k| menu_fits(k, &v)) {
                        // The game's own row (its menu file) whenever the INI's value is one it
                        // takes: before the launcher's lists below, which are copies that can
                        // drift (Frame Rate Limit became a list of names in the overhaul).
                        menu_control(ui, key, kind, &v, control_w)
                    } else if let Some(choices) = self.display_choices(key) {
                        let at = crate::display::chosen(key, &choices, &v);
                        // Monitor with nothing kept: the display the game takes then (#3334).
                        let at = at.or_else(|| {
                            let m = (key == "monitor" && v.trim().is_empty()).then(|| crate::display::chosen_monitor(&self.monitors, None)).flatten()?;
                            choices.iter().position(|c| c.stored == m.name)
                        });
                        let shown = at.map_or(v.clone(), |i| choices[i].label.clone());
                        let mut picked = None;
                        egui::ComboBox::from_id_salt(key).width(control_w - 8.0).selected_text(shown).show_ui(ui, |ui| {
                            for (i, c) in choices.iter().enumerate() {
                                if ui.selectable_label(at == Some(i), &c.label).clicked() {
                                    picked = Some(c.stored.clone());
                                }
                            }
                        });
                        picked.filter(|p| at.is_none_or(|i| &choices[i].stored != p))
                    } else if let Some(&list) = self.lists.get(key) {
                        // A list row before its value's kind: an older 1/0 it took over from
                        // shows as the choice it stands for (catalog::list_value).
                        let v = catalog::list_value(key, &v);
                        let mut t = v.clone();
                        egui::ComboBox::from_id_salt(key).width(control_w - 8.0).selected_text(t.clone()).show_ui(ui, |ui| {
                            for c in list.iter() {
                                ui.selectable_value(&mut t, c.to_string(), *c);
                            }
                        });
                        (t != v).then_some(t)
                    } else {
                        match Kind::of_value(&v) {
                        Kind::Switch => {
                            let mut on = v.trim() == "1";
                            toggle(ui, &mut on).changed().then(|| if on { "1" } else { "0" }.to_string())
                        }
                        Kind::Slider => {
                            let mut f: f32 = v.trim().parse().unwrap_or(0.0);
                            let words: Option<Vec<String>> = self.words.get(key).map(|w| w.iter().map(|s| s.to_string()).collect()).or_else(|| crate::display::steps(key));
                            let s = match words {
                                Some(words) => {
                                    let n = words.len();
                                    let typed = words.clone();
                                    egui::Slider::new(&mut f, 0.0..=1.0)
                                        .step_by(1.0 / (n - 1) as f64)
                                        .custom_formatter(move |x, _| words[((x * (n - 1) as f64).round() as usize).min(n - 1)].clone())
                                        .custom_parser(move |t| word_part(&typed, t))
                                }
                                None => egui::Slider::new(&mut f, 0.0..=1.0).custom_formatter(|x, _| format!("{:.0}%", x * 100.0)).custom_parser(|s| {
                                    s.trim().trim_end_matches('%').parse::<f64>().ok().map(|x| x / 100.0)
                                }),
                            };
                            ui.add_sized([control_w, 20.0], s).changed().then(|| part_text(f))
                        }
                        Kind::Text => {
                            let mut t = v.clone();
                            ui.add_sized([control_w, 22.0], egui::TextEdit::singleline(&mut t)).changed().then_some(t)
                        }
                        }
                    };
                    if let Some(n) = new {
                        // A named colour writes its three parts too: the game reads the parts
                        // after the name, and a part read makes the colour Custom. A part moved
                        // makes it Custom here as in the game.
                        if let (Some(parts), Some(rgb)) = (crate::display::colour_parts(key), crate::display::preset_rgb(&n)) {
                            for (k, c) in parts.into_iter().zip(rgb) {
                                self.set_edit(k, part_text(f32::from(c) / 255.0));
                            }
                        }
                        if let Some(colour) = crate::display::colour_of_part(key) {
                            self.set_edit(colour, crate::display::CUSTOM.to_string());
                        }
                        self.set_edit(key, n);
                    }
                });
            });
        });
        ui.add_space(2.0);
    }

    /// Change `key` to `value` on the page (no change when it is the INI's value).
    fn set_edit(&mut self, key: &str, value: String) {
        if self.stored.get(key) == Some(&value) {
            self.edits.remove(key);
        } else {
            self.edits.insert(key.to_string(), value);
        }
    }

    /// "Reports already sent": what this PC sent, and "Delete them now" (asks first).
    fn reports_control(&mut self, ui: &mut egui::Ui) {
        let sent = self.home.as_ref().is_some_and(crate::reports::any_sent);
        if self.reports_rx.is_some() {
            ui.add(egui::Spinner::new());
            ui.label(RichText::new("Deleting…").small().color(WEAK));
        } else if sent {
            if ui.button("Delete them now").clicked() {
                self.reports_confirm = true;
            }
            ui.label(RichText::new("Kept on the server").small().color(WEAK));
        } else {
            ui.label(RichText::new("None sent from this PC").small().color(WEAK));
        }
    }

    /// The question before deleting, and the request in the background.
    fn reports_window(&mut self, ctx: &egui::Context, running: bool) {
        if let Some(r) = self.reports_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.reports_rx = None;
            self.message = Some(match r {
                Ok(d) => {
                    let mut m = format!("The server was asked to delete the reports of {} install ID(s) from this PC (it removes them within 7 days). Later reports get a new ID.", d.ids);
                    // A report not sent yet that could not be removed would still be sent (#4591).
                    if d.left > 0 {
                        let dir = self.home.as_ref().map(|h| h.reports().join("pending").display().to_string()).unwrap_or_default();
                        m.push_str(&format!(" {} report(s) not sent yet could not be deleted. Delete them by hand from {dir}.", d.left));
                    }
                    (m, d.left > 0)
                }
                Err(e) => (format!("Could not delete the reports: {e}"), true),
            });
            self.reload_ini();
        }
        if !self.reports_confirm {
            return;
        }
        let modal = egui::Modal::new(egui::Id::unique("reports-delete")).frame(Frame::window(&ctx.global_style()).inner_margin(Margin::symmetric(24, 20))).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(RichText::new("Delete your reports?").size(20.0).strong().color(GOLD));
            ui.add_space(8.0);
            ui.label("NVForever's report server is asked to delete every report this PC sent; it removes them within 7 days. Reports not sent yet are deleted here. Later reports get a new ID, not linked to these.");
            ui.add_space(4.0);
            ui.label(RichText::new("This cannot be undone.").color(BAD));
            if running {
                ui.label(RichText::new("Close the game first: it holds this PC's report ID while it runs.").small().color(WEAK));
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let delete = egui::Button::new(RichText::new("Delete them").strong().color(theme::ON_GOLD)).fill(BAD);
                if ui.add_enabled(!running, delete).clicked() {
                    if let Some(home) = self.home.clone() {
                        let (tx, rx) = channel();
                        let c = ctx.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(crate::reports::delete_all(&home));
                            c.request_repaint();
                        });
                        self.reports_rx = Some(rx);
                    }
                    self.reports_confirm = false;
                }
                if ui.button("Cancel").clicked() {
                    self.reports_confirm = false;
                }
            });
        });
        if modal.should_close() {
            self.reports_confirm = false;
        }
    }

    /// The DLSS card (src/dlss.rs): what DLSS needs, what this PC has, and the download.
    fn dlss_card(&mut self, ui: &mut egui::Ui) {
        use nvf_dlss_files::download;
        let running = download::current().map(|h| h.progress()).filter(|p| !p.phase.finished());
        let last = download::current().map(|h| h.progress()).filter(|p| p.phase.finished());
        if matches!(last.as_ref().map(|p| &p.phase), Some(download::Phase::Done(_))) && !self.dlss.complete() {
            self.dlss = crate::dlss::Status::read(self.exe.as_deref());
        }
        let s = self.dlss.clone();
        let need = |ui: &mut egui::Ui, ok: bool, words: String| {
            ui.horizontal(|ui| {
                // A dot, green or red (the system font has no check marks).
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.5, if ok { GOOD } else { BAD });
                ui.label(RichText::new(words).small());
            });
        };
        ui.add_space(8.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("NVIDIA DLSS").strong().size(17.0));
            ui.label(RichText::new("An optional upscaler beside FSR 3.1 (and DLSS frame generation on RTX 40 or newer). NVForever ships no NVIDIA file: the four upscaling and four frame generation files come from NVIDIA's own Streamline release, under NVIDIA's licence.").small().color(WEAK));
            ui.add_space(6.0);
            let os_words = if s.windows { "Windows".to_string() } else { "Windows. The DLSS files are the Windows build, so this stays off here.".to_string() };
            need(ui, s.windows, os_words);
            let gpu = if s.gpus.is_empty() {
                if s.windows { "no graphics card named by Windows".to_string() } else { "no card named in the NVIDIA driver".to_string() }
            } else {
                s.gpus.join(", ")
            };
            need(ui, s.rtx(), format!("An NVIDIA RTX graphics card with a current driver ({gpu})"));
            need(ui, s.upscaling == nvf_dlss_files::FILES.len(), format!("Upscaling files: {} of {}", s.upscaling, nvf_dlss_files::FILES.len()));
            need(ui, s.frame_generation == nvf_dlss_files::FG_FILES.len(), format!("Frame generation files: {} of {}", s.frame_generation, nvf_dlss_files::FG_FILES.len()));
            match &s.folder {
                Ok(f) => ui.label(RichText::new(format!("In {}", f.display())).small().color(WEAK)),
                Err(e) => ui.label(RichText::new(e.clone()).small().color(BAD)),
            };
            ui.add_space(6.0);
            if let Some(p) = &running {
                let (words, part) = crate::dlss::progress_words(p);
                let bar = match part {
                    Some(f) => egui::ProgressBar::new(f).text(words),
                    None => egui::ProgressBar::new(0.0).animate(true).text(words),
                };
                ui.add(bar);
                if ui.button("Cancel download").clicked() {
                    if let Some(h) = download::current() {
                        h.cancel();
                    }
                }
                ui.ctx().request_repaint_after(Duration::from_millis(200));
                return;
            }
            if let Some(p) = &last {
                let (words, _) = crate::dlss::progress_words(p);
                let ok = matches!(p.phase, download::Phase::Done(_));
                ui.label(RichText::new(words).small().color(if ok { GOOD } else if matches!(p.phase, download::Phase::Cancelled) { WEAK } else { BAD }));
                if ok {
                    ui.label(RichText::new("Choose DLSS as the Upscaler below (or in the game's settings).").small().color(WEAK));
                }
            }
            ui.horizontal_wrapped(|ui| {
                if s.windows && s.folder.is_ok() && !s.complete() && ui.add(gold_button("Download DLSS…", true)).on_hover_text("Shows NVIDIA's licence first; nothing is downloaded before you accept it.").clicked() {
                    self.dlss_licence = true;
                }
                if let Ok(f) = &s.folder {
                    if ui.button("Open dlss folder").on_hover_text("To copy the files in by hand (docs/dlss.md).").clicked() {
                        match nvf_dlss_files::create_dlss_folder(f) {
                            Ok(()) => launch::open_folder(f),
                            Err(e) => self.message = Some((format!("{}: {e}", f.display()), true)),
                        }
                    }
                }
                if ui.button("NVIDIA's release page").clicked() {
                    launch::open_url(download::RELEASE_PAGE);
                }
                if ui.button("Check again").clicked() {
                    self.dlss = crate::dlss::Status::read(self.exe.as_deref());
                }
            });
        });
        ui.add_space(4.0);
    }

    /// NVIDIA's licence before the download: I accept takes a mouse click only (as the game's
    /// dialog: no key pressed by habit accepts a licence), Cancel closes with nothing done.
    fn dlss_licence_window(&mut self, ctx: &egui::Context) {
        if !self.dlss_licence {
            return;
        }
        let modal = egui::Modal::new(egui::Id::unique("dlss-licence")).frame(Frame::window(&ctx.global_style()).inner_margin(Margin::symmetric(24, 20))).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.label(RichText::new("NVIDIA's licence").size(20.0).strong().color(GOLD));
            ui.add_space(6.0);
            ui.label("Download DLSS fetches eight files from NVIDIA's Streamline 2.14.16 release on GitHub: the six sl.* files (MIT licence) and nvngx_dlss.dll and nvngx_dlssg.dll, under the licence below. Each file is checked against its pinned SHA-256 and NVIDIA's signatures, and nothing is written unless all pass; a file already in the dlss folder is kept.");
            ui.add_space(6.0);
            Frame::new().fill(theme::BG).corner_radius(4).inner_margin(Margin::same(10)).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, false]).show(ui, |ui| {
                    ui.label(RichText::new(nvf_dlss_files::LICENCE).small().monospace());
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let accept = ui.add(gold_button("I accept", true)).on_hover_text("A mouse click only.");
                if accept.clicked_by(egui::PointerButton::Primary) {
                    nvf_dlss_files::download::start_into(nvf_dlss_files::download::LicenceAccepted::by_player(), None, crate::dlss::folder_for(self.exe.as_deref()));
                    self.dlss_licence = false;
                }
                if ui.button("Cancel").clicked() {
                    self.dlss_licence = false;
                }
                if ui.button("Open on GitHub").clicked() {
                    launch::open_url(nvf_dlss_files::download::RELEASE_PAGE);
                }
            });
        });
        if modal.should_close() {
            self.dlss_licence = false;
        }
    }

    /// The value of `key` as the page shows it now: an unsaved change, else the INI's.
    fn current(&self, key: &str) -> Option<String> {
        self.edits.get(key).or_else(|| self.stored.get(key)).cloned()
    }

    /// Whether the game's rule `rule` holds for the values on the page now (the INI's, or the
    /// changes not saved yet): its row on, or on one of its choices. As the game: a switch on,
    /// a list past its first choice; the input device from `[Controls]`. A parent the INI does
    /// not hold yet is the game's default for it (its menu file); with none known, the rule
    /// holds: the launcher neither greys nor hides a row on a guess the game may not share.
    fn holds(&self, rule: &menu::Rule) -> bool {
        let parent = self.rows.iter().find(|r| r.key == rule.parent);
        let value = if rule.parent == nvf_settings_layout::DEVICE_ROW {
            ini_get(&self.ini_text, "Controls", "Input Device").unwrap_or_default()
        } else {
            match self.current(&rule.parent).or_else(|| parent.and_then(|r| r.default.clone())) {
                Some(v) => v,
                None => return true,
            }
        };
        let kind = parent.and_then(|r| r.kind.as_ref());
        // The choice's name: the INI keeps some lists by another word (frame generation
        // `dlss`), which the launcher's own list names.
        let name = match kind {
            Some(menu::Kind::List { choices }) if choices.iter().any(|c| c.name == value) => value.clone(),
            _ => self.display_choices(&rule.parent).and_then(|l| l.into_iter().find(|c| c.stored == value)).map_or(value.clone(), |c| c.label),
        };
        rule_holds(rule, kind, value.trim(), name.trim())
    }

    /// The list for a Display row the game keeps as a choice (src/display.rs); `None` for
    /// the other rows, and for Monitor when the displays could not be asked.
    fn display_choices(&self, key: &str) -> Option<Vec<crate::display::Choice>> {
        use crate::display;
        match key {
            "monitor" if !self.monitors.is_empty() => Some(display::monitor_choices(&self.monitors)),
            "language" => Some(self.languages.clone()),
            k => display::common_choices(k),
        }
    }

    /// Resolution: the window's size and full screen's, as two lists (the INI keeps both in
    /// one value). The sizes are those of the display chosen on the Monitor row.
    fn resolution_control(&mut self, ui: &mut egui::Ui, v: &str, width: f32) -> Option<String> {
        use crate::display;
        let (window, full) = display::split_resolution(v);
        let monitor_value = self.current("monitor");
        let m = display::chosen_monitor(&self.monitors, monitor_value.as_deref());
        let mut out = None;
        ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Max), |ui| {
            for (name, now, choices, is_window) in [("Window", &window, display::window_choices(m, &window), true), ("Full screen", &full, display::full_choices(m, &full), false)] {
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let at = display::chosen("resolution", &choices, now);
                        let shown = at.map_or(now.clone(), |i| choices[i].label.clone());
                        egui::ComboBox::from_id_salt(("resolution", name)).width(width - 8.0).selected_text(shown).show_ui(ui, |ui| {
                            for (i, c) in choices.iter().enumerate() {
                                if ui.selectable_label(at == Some(i), &c.label).clicked() && at != Some(i) {
                                    out = Some(if is_window { display::join_resolution(&c.stored, &full) } else { display::join_resolution(&window, &c.stored) });
                                }
                            }
                        });
                        ui.label(RichText::new(name).small().color(WEAK));
                    });
                });
            }
        });
        out
    }

    fn load_mods(&mut self) {
        self.mods_dirty = false;
        self.mod_drag = None;
        let Some(home) = self.home.clone() else { return };
        if let Some(game) = self.game() {
            match plugins::sync(&home, &game) {
                Ok(synced) => {
                    self.mods_present = synced.present.into_iter().map(|s| s.to_ascii_lowercase()).collect();
                }
                Err(e) => {
                    self.message = Some((e.to_string(), true));
                    self.mods_present.clear();
                }
            }
        } else {
            self.mods_present.clear();
        }
        self.mods = std::fs::read_to_string(home.plugins_file()).ok().map(|t| plugins::PluginList::parse(&t));
        self.drop_gone_plugins();
        self.mod_rows = load_mod_rows(&home);
        self.refresh_plugin_notes();
        self.keep_data_plugins();
        self.drop_inactive_mod_plugins();
        self.saved_lists = self.lists_text();
    }

    fn enter_mods(&mut self) {
        self.load_mods();
        self.refresh_owned_profiles();
        if !self.profile_restored {
            self.profile_restored = true;
            self.restore_remembered_profile();
        }
    }

    fn page_unsaved(&self) -> bool {
        self.mods_dirty || (!self.saved_lists.is_empty() && self.lists_text() != self.saved_lists)
    }

    /// Writes the mod list and the load order without a profile and without the Save button.
    /// The home's `plugins.txt` and `mod-order.txt` are the default. An open profile is updated
    /// too, and still overrides that default the next time it is opened.
    fn remember_lists(&mut self) {
        if !self.page_unsaved() || self.mods.is_none() {
            return;
        }
        if let Err(e) = self.save_mods() {
            if self.message.as_ref().map(|(m, _)| m != &e).unwrap_or(true) {
                self.message = Some((e, true));
            }
            return;
        }
        if let Some(dir) = self.picked_profile_dir() {
            if self.profile_owned(&dir) {
                let _ = Self::write_profile_lists(&dir, &self.mod_rows, self.mods.as_ref());
            }
        }
        self.mods_dirty = false;
        self.saved_lists = self.lists_text();
    }

    fn restore_remembered_profile(&mut self) {
        let Some(home) = self.home.clone() else { return };
        let Some(name) = ini_get(&self.ini_text, LAUNCHER, "Profile").filter(|s| !s.is_empty()) else { return };
        let parent = home.root.join("profiles");
        self.profile_choices = Self::profile_folders(&parent);
        if !self.profile_choices.iter().any(|(n, _)| n == &name) {
            return;
        }
        self.profile_pick = Some(name.clone());
        self.profile_name = name;
        self.load_chosen_profile();
    }

    /// Plugins that belong only to mods that are off stay in the load order, unticked
    /// ([`only_in_off_folders`]). Runs right after `refresh_plugin_notes`, so `plugin_folders`
    /// holds the folders that are on now.
    fn drop_inactive_mod_plugins(&mut self) {
        let off: HashSet<String> = self.mod_rows.iter().filter(|r| !r.on && !r.separator).map(|r| r.name.clone()).collect();
        if off.is_empty() {
            return;
        }
        let mods = self.home.as_ref().map(|h| h.mods());
        let game = self.game();
        let elsewhere = |plugin: &str| {
            mods.as_ref().and_then(|m| fnv_install::child_ci(m, plugin)).is_some_and(|p| p.is_file()) || game.as_ref().is_some_and(|g| g.data_file(plugin).is_some())
        };
        let owned = only_in_off_folders(&self.plugin_home, &off, &self.plugin_folders, elsewhere);
        let Some(list) = self.mods.as_mut() else { return };
        for entry in &mut list.entries {
            if entry.name.eq_ignore_ascii_case("FalloutNV.esm") {
                continue;
            }
            if owned.contains(&entry.name.to_ascii_lowercase()) {
                entry.active = false;
            }
        }
    }

    /// A folder that is off does not turn off a plugin already in the game Data folder.
    fn keep_data_plugins(&mut self) {
        let Some(game) = self.game() else { return };
        let off: HashSet<String> = self.mod_rows.iter().filter(|r| !r.on).map(|r| r.name.clone()).collect();
        if off.is_empty() {
            return;
        }
        let owned: Vec<String> = self.plugin_home.iter().filter(|(_, folder)| off.contains(*folder)).map(|(plugin, _)| plugin.clone()).collect();
        let Some(list) = self.mods.as_mut() else { return };
        let mut changed = false;
        for entry in &mut list.entries {
            let key = entry.name.to_ascii_lowercase();
            if entry.active || !owned.iter().any(|n| n == &key) {
                continue;
            }
            if game.data_file(&entry.name).is_some() {
                entry.active = true;
                changed = true;
            }
        }
        if changed {
            self.mods_dirty = true;
            self.message = Some(("Plugins already in the game Data folder were left on. Save to keep that.".into(), false));
        }
    }

    /// A plugin whose file is not in Data and not under the mods folder is taken off
    /// the list. FalloutNV.esm stays. An empty scan removes nothing.
    fn drop_gone_plugins(&mut self) {
        if self.mods_present.is_empty() {
            return;
        }
        let Some(list) = self.mods.as_mut() else { return };
        let before = list.entries.len();
        list.entries.retain(|e| {
            let key = e.name.to_ascii_lowercase();
            key == "falloutnv.esm" || self.mods_present.contains(&key)
        });
        let gone = before - list.entries.len();
        if gone > 0 {
            self.mods_dirty = true;
            self.message = Some((format!("Removed {gone} plugins whose files are gone. Save to keep that."), false));
        }
    }

    fn refresh_plugin_notes(&mut self) {
        self.plugin_folders.clear();
        self.plugin_masters.clear();
        self.plugin_home.clear();
        self.folder_counts.clear();
        self.folder_files.clear();
        let Some(home) = self.home.clone() else { return };
        let mods = home.mods();
        let mut files: HashMap<String, PathBuf> = HashMap::new();
        for row in &self.mod_rows {
            if row.separator {
                continue;
            }
            let listed = plugins::plugins_in_tree(&mods.join(&row.name));
            self.folder_counts.insert(row.name.clone(), listed.len());
            self.folder_files.insert(row.name.clone(), mod_file_keys(&mods.join(&row.name)));
            for (name, path) in listed {
                let key = name.to_ascii_lowercase();
                self.plugin_home.insert(key.clone(), row.name.clone());
                if !row.on {
                    continue;
                }
                self.plugin_folders.entry(key.clone()).or_default().push(row.name.clone());
                files.insert(key, path);
            }
        }
        if let Some(game) = self.game() {
            if let Ok(rd) = std::fs::read_dir(&game.data) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.is_symlink() || !path.is_file() {
                        continue;
                    }
                    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
                    if !plugins::is_plugin(name) {
                        continue;
                    }
                    files.entry(name.to_ascii_lowercase()).or_insert(path);
                }
            }
        }
        for (key, path) in files {
            let names = plugins::master_names(&path);
            if !names.is_empty() {
                self.plugin_masters.insert(key, names);
            }
        }
        self.recompute_clashes();
    }

    fn recompute_clashes(&mut self) {
        let rows: Vec<(String, bool)> = self.mod_rows.iter().map(|r| (r.name.clone(), r.on)).collect();
        self.clashes = clash_notes(&rows, &self.folder_files);
    }

    fn mods_page(&mut self, ui: &mut egui::Ui) {
        let help = "Left: the profile and the mod list. Right: plugins. Drag a row to change priority. Right-click a mod or a plugin. Drop a folder or an archive on the list to install it. Apply saves the lists. Nothing is copied into Data.";
        ui.horizontal(|ui| {
            let unsaved = self.page_unsaved();
            let heading = match &self.profile_pick {
                Some(name) if unsaved => format!("Mods — {name} *"),
                Some(name) => format!("Mods — {name}"),
                None if unsaved => "Mods *".into(),
                None => "Mods".into(),
            };
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(h) = &self.home {
                    if ui.button("Open mods folder").clicked() {
                        let _ = std::fs::create_dir_all(h.mods());
                        launch::open_folder(&h.mods());
                    }
                }
                if ui.button("Refresh").on_hover_text("Picks up new mod folders. The load order is left as it is.").clicked() {
                    self.rescan_mods();
                }
                if ui.add_enabled(unsaved, gold_button("Save", unsaved)).on_hover_text("Writes the open profile's mod list and load order. With no profile open, writes the launcher's own lists. Nothing is copied into Data.").clicked() {
                    self.save_open_profile();
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(heading).heading()).truncate()).on_hover_text(help);
                });
            });
        });
        self.install_warn_row(ui);
        self.root_ask_row(ui);
        self.mod_note_row(ui);
        ui.separator();
        let apply_h = 40.0;
        let list_h = (ui.available_height() - apply_h - ui.spacing().item_spacing.y).max(48.0);
        let full_w = ui.available_width();
        ui.allocate_ui_with_layout(Vec2::new(full_w, list_h), Layout::left_to_right(Align::Min), |ui| {
            let gap = ui.spacing().item_spacing.x;
            let total = (full_w - gap).max(1.0);
            let right_floor = 240.0_f32.min(total * 0.5);
            let left_w = (total * 0.62).clamp(right_floor, (total - right_floor).max(right_floor));
            ui.allocate_ui_with_layout(Vec2::new(left_w, list_h), Layout::top_down(Align::Min), |ui| {
                ui.set_width(left_w);
                self.mod_column(ui);
            });
            let right_w = ui.available_width().max(1.0);
            ui.allocate_ui_with_layout(Vec2::new(right_w, list_h), Layout::top_down(Align::Min), |ui| {
                ui.set_width(right_w);
                self.plugin_column(ui);
            });
        });
        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), apply_h), Layout::right_to_left(Align::Center), |ui| {
            let running = self.running();
            if self.apply_ask {
                if ui.add_enabled(!running, egui::Button::new("Apply")).clicked() {
                    self.apply_ask = false;
                    match self.apply_mods() {
                        Ok(()) => self.message = Some(("Mod list and load order saved. Nothing was copied into Data.".into(), false)),
                        Err(e) => self.message = Some((e, true)),
                    }
                }
                if ui.button("Cancel").clicked() {
                    self.apply_ask = false;
                }
                ui.label(RichText::new("Nothing is linked into Data.").small().color(WEAK));
            } else if ui.add_enabled(!running, egui::Button::new("Apply")).on_hover_text("Saves the mod list and the load order. Does not copy or link files into Data.").clicked() {
                self.apply_ask = true;
            }
        });
    }

    fn mod_column(&mut self, ui: &mut egui::Ui) {
        self.profile_row(ui);
        let mods_on = self.mod_rows.iter().filter(|r| r.on).count();
        let mods_all = self.mod_rows.len();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Mod list").small().color(GOLD)).on_hover_text("Right-click a mod. Double-click a separator to rename it.");
            ui.label(RichText::new(format!("{mods_on} on / {mods_all}")).small().color(WEAK)).on_hover_text("Enabled mods, and every mod in the list.");
            if ui.button(if self.show_overwrites { "Hide conflicts" } else { "Conflicts" }).on_hover_text("Green: this mod's loose file wins. Red: a lower mod overwrites it.").clicked() {
                self.show_overwrites = !self.show_overwrites;
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.mod_find).hint_text("Find a mod").desired_width(ui.available_width()));
        self.mod_rename_line(ui);
        self.mod_recycle_line(ui);
        let mod_find = self.mod_find.trim().to_ascii_lowercase();
        let marked = self.picked_plugin.as_ref().and_then(|key| self.plugin_home.get(key)).cloned();
        if self.show_overwrites {
            ui.label(RichText::new("Higher priority overwrites the same loose file. Green wins. Red loses. Hover a line for the paths.").small().color(WEAK));
        }
        egui::ScrollArea::both().id_salt("mod-folders").auto_shrink([false, false]).show(ui, |ui| {
            if self.mod_rows.is_empty() {
                ui.label(RichText::new("No mods in the mods folder.").small().color(WEAK));
                return;
            }
            let n = self.mod_rows.len();
            let mut moved: Option<(usize, usize)> = None;
            let mut menu: Option<ModAct> = None;
            let mut rects = Vec::with_capacity(n);
            for i in 0..n {
                let name = self.mod_rows[i].name.clone();
                let separator = self.mod_rows[i].separator;
                let locked = self.mod_rows[i].locked;
                if !self.mod_row_visible(i, &mod_find) {
                    rects.push(egui::Rect::NOTHING);
                    continue;
                }
                let mut on = self.mod_rows[i].on;
                let dragged = self.mod_drag == Some(i);
                let shown = marked.as_deref() == Some(name.as_str());
                let selected = self.mod_selected.contains(&name);
                let fill = if dragged || shown || selected { theme::SELECTED } else { CARD };
                let stroke = if dragged || shown || selected { Stroke::new(1.0, if (shown || selected) && !dragged { GOLD } else { theme::GOLD_DIM }) } else { Stroke::NONE };
                let mut box_changed = false;
                let row = ui.scope_builder(egui::UiBuilder::new().id_salt(("mod-folder", i)).sense(Sense::click_and_drag()), |ui| {
                    Frame::new().fill(fill).stroke(stroke).corner_radius(4).inner_margin(Margin::symmetric(10, 4)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (grip, _) = ui.allocate_exact_size(Vec2::new(14.0, 18.0), Sense::hover());
                            let c = if dragged { GOLD } else { WEAK };
                            for k in [-4.0, 0.0, 4.0] {
                                let y = grip.center().y + k;
                                ui.painter().line_segment([egui::pos2(grip.left() + 1.0, y), egui::pos2(grip.right() - 1.0, y)], Stroke::new(1.5, c));
                            }
                            if separator {
                                ui.label(RichText::new(format!("— {name}")).color(GOLD)).on_hover_text("Separator. Not a mod. Double-click to rename it.");
                            } else {
                            if ui.checkbox(&mut on, "").changed() {
                                box_changed = true;
                            }
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let real_at = self.mod_rows.iter().take(i + 1).filter(|r| !r.separator).count();
                                    let real_n = self.mod_rows.iter().filter(|r| !r.separator).count();
                                    let prio = crate::modlist::priority(real_at.saturating_sub(1), real_n);
                                    ui.label(RichText::new(prio.to_string()).small().monospace().color(WEAK)).on_hover_text("Priority. A higher number sits lower and wins a loose-file conflict.");
                                    ui.label(&name);
                                    if locked {
                                        ui.label(RichText::new("locked").small().color(WEAK));
                                    }
                                    let count = self.folder_counts.get(&name).copied().unwrap_or(0);
                                    if count > 0 {
                                        ui.label(RichText::new(count.to_string()).small().color(WEAK)).on_hover_text("Plugins in this folder.");
                                    }
                                    if shown {
                                        ui.label(RichText::new("clicked plugin").small().color(GOLD));
                                    }
                                });
                                let files = self.folder_files.get(&name).map(Vec::as_slice).unwrap_or(&[]);
                                let kind = crate::modlist::folder_kind(files);
                                if !kind.is_empty() {
                                    ui.label(RichText::new(kind).small().color(BAD));
                                }
                                if let Some(from) = self.mod_source_line(&name) {
                                    ui.label(RichText::new(from).small().color(WEAK));
                                }
                                if self.show_overwrites {
                                    if let Some(notes) = self.clashes.get(&name) {
                                        let order: Vec<&str> = self.mod_rows.iter().map(|r| r.name.as_str()).collect();
                                        let place = |mod_name: &str| order.iter().position(|n| *n == mod_name).unwrap_or(usize::MAX);
                                        let mut used_mods: Vec<&str> = notes.iter().filter(|n| n.wins).map(|n| n.other.as_str()).collect();
                                        let mut skipped_mods: Vec<&str> = notes.iter().filter(|n| !n.wins).map(|n| n.other.as_str()).collect();
                                        used_mods.sort_by_key(|m| place(m));
                                        skipped_mods.sort_by_key(|m| place(m));
                                        let used_files: Vec<String> = notes.iter().filter(|n| n.wins).flat_map(|n| n.files.iter().map(|f| format!("{f}  (also in {})", n.other))).collect();
                                        let skipped_files: Vec<String> = notes.iter().filter(|n| !n.wins).flat_map(|n| n.files.iter().map(|f| format!("{f}  (used from {})", n.other))).collect();
                                        let file_word = |n: usize| if n == 1 { "file" } else { "files" };
                                        let be = |n: usize| if n == 1 { "is" } else { "are" };
                                        if !used_mods.is_empty() {
                                            let nfiles = notes.iter().filter(|n| n.wins).flat_map(|n| n.files.iter()).collect::<HashSet<_>>().len();
                                            let who = if used_mods.len() == 1 {
                                                format!("{} is higher in the list, so the copy in that mod is skipped", used_mods[0])
                                            } else {
                                                format!("These mods are higher in the list, so the copy in each of them is skipped: {}", used_mods.join(", "))
                                            };
                                            let line = format!("{nfiles} shared {} in this mod {} used. {who}.", file_word(nfiles), be(nfiles));
                                            ui.label(RichText::new(line).small().color(GOOD)).on_hover_text(used_files.join("\n"));
                                        }
                                        if !skipped_mods.is_empty() {
                                            let nfiles = notes.iter().filter(|n| !n.wins).flat_map(|n| n.files.iter()).collect::<HashSet<_>>().len();
                                            let who = if skipped_mods.len() == 1 {
                                                format!("{} is lower in the list, so the copy in that mod is used", skipped_mods[0])
                                            } else {
                                                format!("These mods are lower in the list, so the copy in each of them is used: {}", skipped_mods.join(", "))
                                            };
                                            let line = format!("{nfiles} shared {} in this mod {} skipped. {who}.", file_word(nfiles), be(nfiles));
                                            ui.label(RichText::new(line).small().color(BAD)).on_hover_text(skipped_files.join("\n"));
                                        }
                                        for (listed, line) in used_files.iter().chain(skipped_files.iter()).enumerate() {
                                            if listed == 8 {
                                                ui.label(RichText::new("…").small().color(WEAK));
                                                break;
                                            }
                                            ui.label(RichText::new(line).small().color(WEAK));
                                        }
                                    }
                                }
                            });
                            }
                        });
                    });
                });
                if box_changed {
                    self.set_mod_on(i, on);
                }
                let resp = row.response;
                let shared = self.shared_paths(&name);
                resp.container_context_menu(|ui| {
                    Self::mod_row_menu(ui, on, i, n, !shared.is_empty(), locked, separator, &shared, self.drag_undo.is_some(), &mut menu);
                });
                if resp.secondary_clicked() && !separator && !self.mod_selected.contains(&name) {
                    self.mod_selected.clear();
                    self.mod_selected.insert(name.clone());
                }
                if separator && resp.double_clicked() {
                    menu = Some(ModAct::RenameSeparator(i));
                }
                if resp.clicked() && !separator {
                    let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                    if ctrl {
                        if !self.mod_selected.insert(name.clone()) {
                            self.mod_selected.remove(&name);
                        }
                    } else if self.plugin_only_folder.as_deref() == Some(name.as_str()) {
                        self.plugin_only_folder = None;
                    } else {
                        self.plugin_only_folder = Some(name.clone());
                    }
                }
                if resp.drag_started_by(egui::PointerButton::Primary) && mod_find.is_empty() && self.mod_filter == 0 && !locked {
                    self.mod_drag = Some(i);
                }
                if self.mod_jump && shown {
                    ui.scroll_to_rect(resp.rect, Some(Align::Center));
                    self.mod_jump = false;
                }
                let _ = resp.clone().on_hover_cursor(egui::CursorIcon::Grab);
                rects.push(resp.rect);
            }
            if mod_find.is_empty() && self.mod_filter == 0 { if let Some(from) = self.mod_drag {
                let ctx = ui.ctx().clone();
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                if let Some(pos) = ctx.pointer_latest_pos() {
                    let centers: Vec<f32> = rects.iter().map(|r| r.center().y).collect();
                    let gap = insertion_index(&centers, pos.y);
                    if let (Some(first), Some(last)) = (rects.first(), rects.last()) {
                        let y = if gap < rects.len() { rects[gap].top() - ui.spacing().item_spacing.y / 2.0 } else { last.bottom() + ui.spacing().item_spacing.y / 2.0 };
                        ui.painter().line_segment([egui::pos2(first.left(), y), egui::pos2(first.right(), y)], Stroke::new(2.0, GOLD));
                    }
                    let clip = ui.clip_rect();
                    let edge = 36.0;
                    if pos.y < clip.top() + edge {
                        ui.scroll_with_delta(Vec2::new(0.0, 8.0));
                    } else if pos.y > clip.bottom() - edge {
                        ui.scroll_with_delta(Vec2::new(0.0, -8.0));
                    }
                    ctx.request_repaint();
                    if !ctx.input(|i| i.pointer.any_down()) {
                        self.mod_drag = None;
                        let to = move_target(from, gap);
                        if to != from {
                            moved = Some((from, to));
                        }
                    }
                }
            }
            }
            if let Some((from, to)) = moved {
                let prior = self.mod_rows.clone();
                let next = reorder_mods(&prior, from, to);
                if next.iter().map(|r| &r.name).ne(prior.iter().map(|r| &r.name)) || next.iter().zip(prior.iter()).any(|(a, b)| a.separator != b.separator) {
                    self.drag_undo = Some(prior);
                    self.mod_rows = next;
                    self.mods_dirty = true;
                    self.recompute_clashes();
                }
            }
            if let Some(act) = menu {
                self.apply_mod_act(act);
            }
        });
    }

    fn plugin_column(&mut self, ui: &mut egui::Ui) {
        self.backup_row(ui);
        let folders = self.plugin_folders.clone();
        let masters = self.plugin_masters.clone();
        let active = self.mods.as_ref().map(|l| l.entries.iter().filter(|e| e.active).count()).unwrap_or(0);
        // The number is how many plugins are on. There is no slot cap.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Load order").small().color(GOLD)).on_hover_text("Right-click a plugin. An ESM is a master and should sit above the plugins that need it.");
            ui.label(RichText::new(active.to_string()).small().color(WEAK)).on_hover_text("Plugins that are enabled.");
        });
        let mut only_folder = self.plugin_only_folder.clone();
        if let Some(folder) = only_folder.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Plugins in {folder}")).small().color(WEAK));
                if ui.button("Show every plugin").clicked() {
                    only_folder = None;
                    self.plugin_only_folder = None;
                }
            });
        }
        let in_data: HashSet<String> = {
            let names: Vec<String> = self.mods.as_ref().map(|l| l.entries.iter().map(|e| e.name.clone()).collect()).unwrap_or_default();
            self.game().map(|g| names.into_iter().filter(|name| g.data_file(name).is_some()).map(|name| name.to_ascii_lowercase()).collect()).unwrap_or_default()
        };
        let Some(list) = &mut self.mods else {
            ui.label("No plugin list yet: set the game folder first, or start the game once.");
            return;
        };
        ui.add(egui::TextEdit::singleline(&mut self.plugin_find).hint_text("Find a plugin").desired_width(ui.available_width()));
        let find = self.plugin_find.trim().to_ascii_lowercase();
        let active_names: HashSet<String> = list.entries.iter().filter(|e| e.active).map(|e| e.name.to_ascii_lowercase()).collect();
        let present = self.mods_present.clone();
        let is_esm: Vec<bool> = list.entries.iter().map(|e| e.name.to_ascii_lowercase().ends_with(".esm")).collect();
        let n = list.entries.len();
        let is_base: Vec<bool> = list.entries.iter().map(|e| e.name.eq_ignore_ascii_case("FalloutNV.esm")).collect();
        let fixed = is_base.iter().take_while(|b| **b).count();
        let order_at: HashMap<String, usize> = list.entries.iter().enumerate().map(|(i, e)| (e.name.to_ascii_lowercase(), i)).collect();
        let folder_on: HashMap<String, bool> = self.mod_rows.iter().filter(|r| !r.separator).map(|r| (r.name.clone(), r.on)).collect();
        let mut moved: Option<(usize, usize)> = None;
        let mut menu: Option<PluginAct> = None;
        egui::ScrollArea::both().id_salt("plugin-rows").auto_shrink([false, false]).show(ui, |ui| {
            let mut rects = Vec::with_capacity(n);
            let mut last_esm = false;
            let mut toggled = false;
            for (i, e) in list.entries.iter_mut().enumerate() {
                if !find.is_empty() && !e.name.to_ascii_lowercase().contains(&find) {
                    continue;
                }
                if let Some(only) = &only_folder {
                    let key = e.name.to_ascii_lowercase();
                    let in_folder = self.folder_files.get(only).is_some_and(|files| {
                        files.iter().any(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).eq_ignore_ascii_case(&key))
                    });
                    if !in_folder {
                        continue;
                    }
                }
                if last_esm && !is_esm[i] {
                    ui.separator();
                }
                last_esm = is_esm[i];
                let base = is_base[i];
                let dragged = self.mods_drag == Some(i);
                let picked = self.plugin_selected.contains(&e.name.to_ascii_lowercase());
                let fill = if dragged || picked { theme::SELECTED } else { CARD };
                let stroke = if dragged { Stroke::new(1.0, theme::GOLD_DIM) } else { Stroke::NONE };
                let sense = if base { Sense::click() } else { Sense::click_and_drag() };
                let mut name_rect = egui::Rect::NOTHING;
                let row = ui.scope_builder(egui::UiBuilder::new().id_salt(("mod-row", i)).sense(sense), |ui| {
                    let hot = !base && (dragged || ui.response().hovered());
                    Frame::new().fill(fill).stroke(stroke).corner_radius(4).inner_margin(Margin::symmetric(10, 4)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (grip, _) = ui.allocate_exact_size(Vec2::new(14.0, 18.0), Sense::hover());
                            if !base {
                                let c = if hot { GOLD } else { WEAK };
                                for k in [-4.0, 0.0, 4.0] {
                                    let y = grip.center().y + k;
                                    ui.painter().line_segment([egui::pos2(grip.left() + 1.0, y), egui::pos2(grip.right() - 1.0, y)], Stroke::new(1.5, c));
                                }
                            }
                            ui.label(RichText::new(format!("{i:02}")).small().monospace().color(WEAK)).on_hover_text("Priority. A plugin lower in this list loads later.");
                            ui.label(RichText::new(format!("{:02X}", i)).monospace().color(WEAK)).on_hover_text("Mod index.");
                            if ui.add_enabled(!base, egui::Checkbox::without_text(&mut e.active)).changed() {
                                self.mods_dirty = true;
                                toggled = true;
                                self.picked_plugin = Some(e.name.to_ascii_lowercase());
                                self.mod_find.clear();
                                self.mod_jump = true;
                            }
                            name_rect = {
                                let key = e.name.to_ascii_lowercase();
                                let ticked = self.plugin_home.get(&key).and_then(|folder| folder_on.get(folder)).copied().unwrap_or(false);
                                let grey = !base && !ticked && !in_data.contains(&key);
                                let text = if grey { RichText::new(&e.name).color(WEAK) } else { RichText::new(&e.name) };
                                let rest = ui.available_width().max(48.0);
                                let folder = self.plugin_home.get(&key).cloned();
                                ui.allocate_ui_with_layout(Vec2::new(rest, 18.0), Layout::top_down(Align::Min), |ui| {
                                    ui.set_width(rest);
                                    let lab = ui.add(egui::Label::new(text).wrap());
                                    if grey {
                                        lab.clone().on_hover_text("Not in a ticked mod, and not in Data.");
                                    }
                                    if let Some(folder) = folder {
                                        ui.label(RichText::new(format!("in {folder}")).small().color(WEAK));
                                    }
                                    lab.rect
                                }).inner
                            };
                            let key = e.name.to_ascii_lowercase();
                            if !present.is_empty() && !present.contains(&key) {
                                ui.label(RichText::new("missing").small().color(BAD));
                            }
                            if let Some(have) = folders.get(&key).filter(|v| v.len() > 1) {
                                if let Some(win) = have.last() {
                                    let lost: Vec<&str> = have[..have.len() - 1].iter().map(String::as_str).collect();
                                    let skipped = if lost.len() == 1 {
                                        format!("The copy in {} is skipped.", lost[0])
                                    } else {
                                        format!("The copies in these mods are skipped: {}.", lost.join(", "))
                                    };
                                    ui.label(RichText::new(format!("The copy in {win} is used.")).small().color(GOOD)).on_hover_text(note_files(&self.folder_files, win, &e.name));
                                    ui.label(RichText::new(skipped).small().color(BAD)).on_hover_text(lost.iter().map(|loser| note_files(&self.folder_files, loser, &e.name)).collect::<Vec<_>>().join("\n"));
                                }
                            }
                            if e.active {
                                if let Some(need) = masters.get(&key) {
                                    let off: Vec<&str> = need.iter().filter(|m| !active_names.contains(&m.to_ascii_lowercase())).map(String::as_str).collect();
                                    if !off.is_empty() {
                                        ui.label(RichText::new(format!("needs {}", off.join(", "))).small().color(BAD));
                                    }
                                    let lower: Vec<&str> = need.iter().filter(|m| {
                                        let mk = m.to_ascii_lowercase();
                                        active_names.contains(&mk) && crate::modlist::master_is_lower(i, order_at.get(&mk).copied())
                                    }).map(String::as_str).collect();
                                    if !lower.is_empty() {
                                        ui.label(RichText::new(format!("master is lower: {}", lower.join(", "))).small().color(BAD));
                                    }
                                }
                            }
                        });
                    });
                });
                let resp = row.response;
                let plugin_name = e.name.clone();
                let plugin_on = e.active;
                let plugin_folder = self.plugin_home.get(&plugin_name.to_ascii_lowercase()).cloned();
                resp.container_context_menu(|ui| {
                    Self::plugin_row_menu(ui, &plugin_name, plugin_on, base, i, plugin_folder.clone(), &mut menu);
                });
                if resp.secondary_clicked() && !base {
                    let key = plugin_name.to_ascii_lowercase();
                    if !self.plugin_selected.contains(&key) {
                        self.plugin_selected.clear();
                        self.plugin_selected.insert(key);
                    }
                }
                if !base {
                    if resp.drag_started_by(egui::PointerButton::Primary) {
                        self.mods_drag = Some(i);
                    }
                    if resp.clicked() {
                        let key = e.name.to_ascii_lowercase();
                        let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                        if ctrl {
                            if !self.plugin_selected.insert(key.clone()) {
                                self.plugin_selected.remove(&key);
                            }
                        } else if !resp.interact_pointer_pos().is_some_and(|p| name_rect.contains(p)) {
                            self.plugin_selected.clear();
                            self.plugin_selected.insert(key.clone());
                            self.picked_plugin = Some(key);
                            self.mod_find.clear();
                            self.mod_jump = true;
                        }
                    }
                    if resp.clicked() && resp.interact_pointer_pos().is_some_and(|p| name_rect.contains(p)) {
                        e.active = !e.active;
                        self.mods_dirty = true;
                        toggled = true;
                    }
                    let _ = resp.clone().on_hover_cursor(egui::CursorIcon::Grab);
                }
                rects.push(resp.rect);
            }
            if let Some(act) = menu.take() {
                match act {
                    PluginAct::Show(index) => {
                        if let Some(name) = list.entries.get(index).map(|e| e.name.to_ascii_lowercase()) {
                            self.picked_plugin = Some(name);
                            self.mod_find.clear();
                            self.mod_jump = true;
                        }
                    }
                    PluginAct::Copy(name) => self.message = Some((format!("Copied {name}."), false)),
                    PluginAct::Selected(on) => {
                        let names = self.plugin_selected.clone();
                        for entry in &mut list.entries {
                            if names.contains(&entry.name.to_ascii_lowercase()) && !entry.name.eq_ignore_ascii_case("FalloutNV.esm") && entry.active != on {
                                entry.active = on;
                                self.mods_dirty = true;
                                toggled = true;
                            }
                        }
                    }
                    PluginAct::All(on) => {
                        for entry in &mut list.entries {
                            if !entry.name.eq_ignore_ascii_case("FalloutNV.esm") && entry.active != on {
                                entry.active = on;
                                self.mods_dirty = true;
                                toggled = true;
                            }
                        }
                    }
                }
            }
            if toggled {
                let homes = self.plugin_home.clone();
                let active_now: HashSet<String> = list.entries.iter().filter(|e| e.active).map(|e| e.name.to_ascii_lowercase()).collect();
                for row in &mut self.mod_rows {
                    let has = homes.values().any(|folder| folder == &row.name);
                    if !has {
                        continue;
                    }
                    row.on = homes.iter().any(|(plugin, folder)| folder == &row.name && active_now.contains(plugin));
                }
                let rows: Vec<(String, bool)> = self.mod_rows.iter().map(|r| (r.name.clone(), r.on)).collect();
                self.clashes = clash_notes(&rows, &self.folder_files);
            }
            if !find.is_empty() || only_folder.is_some() {
                return;
            }
            let Some(from) = self.mods_drag else { return };
            let ctx = ui.ctx().clone();
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            let Some(pos) = ctx.pointer_latest_pos() else { return };
            let centers: Vec<f32> = rects.iter().map(|r| r.center().y).collect();
            let gap = insertion_index(&centers, pos.y).max(fixed);
            if let (Some(first), Some(last)) = (rects.first(), rects.last()) {
                let y = if gap < rects.len() { rects[gap].top() - ui.spacing().item_spacing.y / 2.0 } else { last.bottom() + ui.spacing().item_spacing.y / 2.0 };
                ui.painter().line_segment([egui::pos2(first.left(), y), egui::pos2(first.right(), y)], Stroke::new(2.0, GOLD));
            }
            let clip = ui.clip_rect();
            let edge = 36.0;
            if pos.y < clip.top() + edge {
                ui.scroll_with_delta(Vec2::new(0.0, 8.0));
            } else if pos.y > clip.bottom() - edge {
                ui.scroll_with_delta(Vec2::new(0.0, -8.0));
            }
            ctx.request_repaint();
            if !ctx.input(|i| i.pointer.any_down()) {
                self.mods_drag = None;
                let to = move_target(from, gap);
                if to != from {
                    moved = Some((from, to));
                }
            }
        });
        if let Some((from, to)) = moved {
            let e = list.entries.remove(from);
            list.entries.insert(to, e);
            self.mods_dirty = true;
        }
    }

    /// Add a folder, a loose plugin, or an archive. An installer that needs a choice opens a window.
    fn installer_window(&mut self, ctx: &egui::Context) {
        let decision = {
            let Some(w) = self.wizard.as_mut() else { return };
            crate::fomod::draw(ctx, w)
        };
        match decision {
            Some(crate::fomod::Decision::Cancel) => {
                if let Some(w) = self.wizard.take() {
                    w.forget_images(ctx);
                    w.discard();
                }
            }
            Some(crate::fomod::Decision::Install) => {
                let Some(home) = self.home.clone() else { return };
                let mods = home.mods().to_path_buf();
                let recycle = self.recycle_dir().ok();
                let result = self.wizard.as_mut().map(|w| w.install_over(&mods, recycle.as_deref()));
                match result {
                    Some(Ok(name)) => {
                        if let Some(w) = self.wizard.take() {
                            self.store_choices(&w);
                            if let Some(src) = self.install_source.take() {
                                self.note_install(&name, &src);
                            }
                            self.log_mod(&format!("installed {name}"));
                            w.forget_images(ctx);
                            w.discard();
                        }
                        self.finish_import(Ok(name));
                    }
                    Some(Err(e)) => {
                        if let Some(w) = self.wizard.as_mut() {
                            w.error = Some(e);
                        }
                    }
                    None => {}
                }
            }
            None => {}
        }
    }

    fn ask_new_profile(&mut self) {
        self.save_as_profile();
    }

    /// Names a separator in the mod list. A separator is not a mod and it is not installed.
    fn separator_window(&mut self, ctx: &egui::Context) {
        if self.separator_at.is_none() && self.separator_edit.is_none() {
            return;
        }
        let renaming = self.separator_edit.is_some();
        let mut open = true;
        egui::Window::new(if renaming { "Rename separator" } else { "Add separator" })
            .collapsible(false)
            .resizable(false)
            .constrain(true)
            .max_size(fit_to_screen(ctx))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label(RichText::new("A separator. Not a mod, and it holds no plugins or loose files.").small().color(WEAK));
                let edit = ui.add(egui::TextEdit::singleline(&mut self.separator_name).hint_text("Separator name").desired_width(300.0));
                if self.separator_focus {
                    edit.request_focus();
                    self.separator_focus = false;
                }
                ui.horizontal(|ui| {
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button(if renaming { "Rename" } else { "Add" }).clicked() || (enter && (edit.lost_focus() || edit.has_focus())) {
                        self.commit_separator();
                    }
                    if ui.button("Cancel").clicked() {
                        self.separator_at = None;
                        self.separator_edit = None;
                        self.separator_name.clear();
                    }
                });
            });
        if !open {
            self.separator_at = None;
            self.separator_edit = None;
            self.separator_name.clear();
        }
    }


    fn install_facts(&self) -> crate::fomod::FactsInput {
        let active = self.mods.as_ref().map(|l| l.entries.iter().filter(|e| e.active).map(|e| e.name.clone()).collect()).unwrap_or_default();
        let mut paths = HashSet::new();
        for files in self.folder_files.values() {
            for file in files {
                paths.insert(file.clone());
            }
        }
        crate::fomod::FactsInput { active, present: self.mods_present.clone(), paths }
    }

    /// Plugins of a folder just added get a line. A new folder is on, so those plugins are on.
    /// Without this the folder shows in Mod Order and the plugin list does not change.
    fn adopt_new_plugins(&mut self) {
        let Some(home) = self.home.clone() else { return };
        self.merge_found_mods(&home);
        let mods = home.mods();
        let mut found = Vec::new();
        for row in &self.mod_rows {
            if row.separator {
                continue;
            }
            for (plugin, _) in plugins::plugins_in_tree(&mods.join(&row.name)) {
                found.push((plugin, row.on));
            }
        }
        if self.mods.is_none() {
            let text = std::fs::read_to_string(home.plugins_file()).unwrap_or_default();
            self.mods = Some(plugins::PluginList::parse(&text));
        }
        if let Some(list) = &mut self.mods {
            let names: Vec<String> = found.iter().map(|(n, _)| n.clone()).collect();
            let added = list.add_new(&names);
            for (plugin, on) in &found {
                if *on && added.iter().any(|n| n.eq_ignore_ascii_case(plugin)) {
                    if let Some(e) = list.entries.iter_mut().find(|e| e.name.eq_ignore_ascii_case(plugin)) {
                        e.active = true;
                    }
                }
            }
        }
        for (plugin, _) in &found {
            self.mods_present.insert(plugin.to_ascii_lowercase());
        }
        self.refresh_plugin_notes();
        self.mods_dirty = true;
    }

    fn finish_import(&mut self, result: Result<String, String>) {
        match result {
            Ok(name) => {
                self.page = Page::Mods;
                self.adopt_new_plugins();
                self.message = Some((format!("Added {name}. Its plugins are in the list. Save to keep that."), false));
            }
            Err(e) => self.message = Some((e, true)),
        }
    }

    fn take_dropped(&mut self, ctx: &egui::Context) {
        let n = ctx.input(|i| i.raw.dropped_files.len());
        if n == 0 {
            return;
        }
        // A folder dropped on the Profiles box is a profile folder, not a mod to install.
        let over_profile = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| self.profile_rect.contains(p));
        if over_profile {
            let path = ctx.input(|i| {
                i.raw.dropped_files.iter().find_map(|f| {
                    let path = f.path();
                    path.exists().then(|| path.to_path_buf())
                })
            });
            match path {
                Some(_) => {
                    self.message = Some(("Profiles are saved next to the mods folder. That drop was not used as a save location.".into(), false));
                }
                None => self.message = Some(("That drop had no file path.".into(), true)),
            }
            return;
        }
        if self.wizard.is_some() {
            self.message = Some(("Finish the open installer first.".into(), true));
            return;
        }
        let mut sources = Vec::new();
        for i in 0..n {
            let path = ctx.input(|input| input.raw.dropped_files.get(i).map(|f| f.path().to_path_buf()));
            let Some(path) = path else { continue };
            if path.exists() {
                sources.push(Ok(path));
                continue;
            }
            let loaded = ctx.input(|input| input.raw.dropped_files.get(i).and_then(|f| f.bytes().ok()));
            match loaded {
                Some(bytes) if !bytes.is_empty() => {
                    let name = path.file_name().and_then(|n| n.to_str()).filter(|s| !s.is_empty() && !s.contains("..") && !s.contains('/') && !s.contains('\\')).unwrap_or("dropped.mod");
                    sources.push(write_dropped_bytes(name, &bytes));
                }
                _ => sources.push(Err("That drop had no file path.".into())),
            }
        }
        let mut first = None;
        let mut extra = 0usize;
        let mut failed = None;
        for source in sources {
            match source {
                Ok(path) => {
                    if first.is_none() {
                        first = Some(path);
                    } else {
                        extra += 1;
                    }
                }
                Err(e) => failed = Some(e),
            }
        }
        if let Some(path) = first {
            self.page = Page::Mods;
            self.begin_install(path);
        }
        if extra > 0 {
            self.message = Some(("Install this one first. Drop the next after it.".into(), false));
        } else if let Some(e) = failed {
            self.message = Some((e, true));
        }
    }

    /// Opens Quick Install. OK and Manual are the only ways anything is copied.
    fn begin_install(&mut self, path: PathBuf) {
        if self.wizard.is_some() || self.quick.is_some() || self.manual.is_some() {
            self.message = Some(("Finish the open installer first.".into(), true));
            return;
        }
        if path.as_os_str().is_empty() || !path.exists() {
            self.message = Some(("That path was not found.".into(), true));
            return;
        }
        let typed = std::mem::take(&mut self.install_name);
        let stem = path.file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).unwrap_or("mod").to_string();
        let name = if typed.trim().is_empty() { stem.clone() } else { typed.trim().to_string() };
        let mut suggestions = Vec::new();
        if !name.eq_ignore_ascii_case(&stem) {
            suggestions.push(name.clone());
        }
        suggestions.push(stem);
        self.quick = Some(QuickAsk { source: path, name, suggestions });
    }

    fn quick_window(&mut self, ctx: &egui::Context) {
        if self.quick.is_none() {
            return;
        }
        let mut open = true;
        let mut manual = false;
        let mut ok = false;
        let mut cancel = false;
        egui::Window::new("Quick Install")
            .collapsible(false)
            .resizable(false)
            .constrain(true)
            .max_size(fit_to_screen(ctx))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                let quick = self.quick.as_mut().unwrap();
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut quick.name).desired_width(300.0));
                });
                let suggestions = quick.suggestions.clone();
                let current = quick.name.clone();
                if suggestions.len() > 1 {
                    egui::ComboBox::from_id_salt("quick-name").width(300.0).selected_text(&current).show_ui(ui, |ui| {
                        for name in suggestions {
                            if ui.selectable_label(current == name, &name).clicked() {
                                self.quick.as_mut().unwrap().name = name;
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("Manual").on_hover_text("Shows the archive. You choose the data folder. Nothing is copied until OK.").clicked() {
                        manual = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                        if ui.button("OK").clicked() {
                            ok = true;
                        }
                    });
                });
            });
        if manual {
            self.open_manual_from_quick();
        } else if ok {
            self.accept_quick();
        } else if cancel || !open {
            self.quick = None;
        }
    }

    fn accept_quick(&mut self) {
        let Some(quick) = self.quick.take() else { return };
        self.install_name = quick.name.clone();
        match self.stage_one(&quick.source) {
            Ok(Some(name)) => self.finish_import(Ok(name)),
            Ok(None) => {
                if self.install_warn.is_empty() && self.root_ask.is_none() && self.wizard.is_some() {
                    self.message = Some(("Choose what to install.".into(), false));
                }
            }
            Err(e) => {
                self.quick = Some(quick);
                self.message = Some((e, true));
            }
        }
    }

    fn open_manual_from_quick(&mut self) {
        let Some(quick) = self.quick.take() else { return };
        match crate::fomod::open_manual(&quick.source) {
            Ok((temp, top, tree)) => {
                self.manual = Some(ManualAsk { source: quick.source, temp, top, data_rel: PathBuf::new(), tree, name: quick.name });
            }
            Err(e) => {
                self.quick = Some(quick);
                self.message = Some((e, true));
            }
        }
    }

    fn manual_window(&mut self, ctx: &egui::Context) {
        if self.manual.is_none() {
            return;
        }
        let mut open = true;
        let mut ok = false;
        let mut cancel = false;
        let mut pick: Option<String> = None;
        let mut up = false;
        let valid = self.manual.as_ref().is_some_and(|m| crate::fomod::data_valid(&m.tree));
        let can_up = self.manual.as_ref().is_some_and(|m| !m.data_rel.as_os_str().is_empty());
        let list_h = (ctx.viewport_rect().height() * 0.45).clamp(140.0, 420.0);
        egui::Window::new("Install Mods")
            .collapsible(false)
            .resizable(true)
            .constrain(true)
            .default_width(520.0)
            .max_size(fit_to_screen(ctx))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(460.0);
                ui.set_min_height(280.0);
                let manual = self.manual.as_mut().unwrap();
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut manual.name).desired_width(340.0));
                });
                ui.label(RichText::new("Content").small().color(WEAK));
                egui::ScrollArea::vertical().id_salt("manual-files").max_height(list_h).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut manual.tree.checked, "").changed() {
                            let on = manual.tree.checked;
                            crate::fomod::set_branch(&mut manual.tree, on);
                        }
                        ui.label(RichText::new("<data>").strong());
                        if can_up && ui.small_button("Folder above").on_hover_text("Move <data> up one folder.").clicked() {
                            up = true;
                        }
                    });
                    let mut shown = 0usize;
                    manual_nodes(ui, &mut manual.tree, "", &mut pick, 4, &mut shown);
                });
                if valid {
                    ui.label(RichText::new("The content of <data> looks valid.").small().color(GOOD));
                } else {
                    ui.label(RichText::new("The content of <data> does not look valid.").small().color(BAD));
                }
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                        if ui.button("OK").on_hover_text("Copies the ticked files into the mods folder. The game Data folder is not used.").clicked() {
                            ok = true;
                        }
                    });
                });
            });
        if let Some(rel) = pick {
            self.set_manual_data(PathBuf::from(rel));
        } else if up {
            if let Some(manual) = self.manual.as_ref() {
                let parent = manual.data_rel.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                self.set_manual_data(parent);
            }
        } else if ok {
            self.accept_manual();
        } else if cancel || !open {
            self.discard_manual();
        }
    }

    fn set_manual_data(&mut self, rel: PathBuf) {
        let Some(manual) = self.manual.as_mut() else { return };
        let dir = manual.top.join(&rel);
        if !dir.is_dir() {
            return;
        }
        manual.tree = crate::fomod::scan_manual(&dir);
        manual.data_rel = rel;
    }

    fn discard_manual(&mut self) {
        if let Some(manual) = self.manual.take() {
            crate::fomod::discard(&manual.temp);
        }
    }

    fn accept_manual(&mut self) {
        let Some(manual) = self.manual.take() else { return };
        if !manual.tree.children.iter().any(|c| c.checked) {
            self.message = Some(("Nothing was ticked, so nothing was copied.".into(), true));
            self.manual = Some(manual);
            return;
        }
        let name = match Self::one_name(&manual.name) {
            Ok(name) => name,
            Err(e) => {
                self.message = Some((e, true));
                self.manual = Some(manual);
                return;
            }
        };
        let Some(home) = self.home.clone() else {
            self.message = Some(("No launcher home, so nothing was copied.".into(), true));
            self.manual = Some(manual);
            return;
        };
        let dest = home.mods().join(&name);
        if dest.exists() {
            self.message = Some(("That folder is already in the mods folder.".into(), true));
            self.manual = Some(manual);
            return;
        }
        if let Err(e) = guard_write(&dest, None) {
            self.message = Some((e.to_string(), true));
            self.manual = Some(manual);
            return;
        }
        let data = manual.top.join(&manual.data_rel);
        if let Err(e) = crate::fomod::copy_checked(&data, &manual.tree, &dest) {
            let _ = std::fs::remove_dir_all(&dest);
            self.message = Some((e, true));
            self.manual = Some(manual);
            return;
        }
        let source_note = manual.source.clone();
        crate::fomod::discard(&manual.temp);
        self.note_install(&name, &source_note);
        self.log_mod(&format!("installed {name}"));
        self.finish_import(Ok(name));
    }

    fn stage_one(&mut self, src: &Path) -> Result<Option<String>, String> {
        if self.wizard.is_some() || self.manual.is_some() {
            return Err("Finish the open installer first.".into());
        }
        let preferred = match std::mem::take(&mut self.install_name) {
            name if name.trim().is_empty() => String::new(),
            name => Self::one_name(&name)?,
        };
        self.clear_held();
        if let Some(ask) = self.root_ask.take() {
            crate::fomod::discard(&ask.temp);
        }
        if src.as_os_str().is_empty() || !src.exists() {
            return Err("That path was not found.".into());
        }
        let home = self.home.clone().ok_or("no home folder")?;
        let mods = home.mods().to_path_buf();
        std::fs::create_dir_all(&mods).map_err(|e| format!("{}: {e}", mods.display()))?;
        if let Some(game) = self.game() {
            if let (Ok(s), Ok(g)) = (src.canonicalize(), game.root.canonicalize()) {
                if s == g || s.starts_with(&g) {
                    return Err("That path is in the game folder. It is not copied.".into());
                }
            }
        }
        if let Ok(mods_c) = mods.canonicalize() {
            if let Ok(s) = src.canonicalize() {
                if s == mods_c || s.starts_with(&mods_c) {
                    return Err("That folder is already in the mods folder.".into());
                }
            }
        }
        let facts = self.install_facts();
        let source = src.to_path_buf();
        match crate::fomod::stage(src, facts)? {
            crate::fomod::Staged::Tree { name, from, temp } => {
                let name = if preferred.is_empty() { name } else { preferred };
                self.offer_tree(name, from, temp, source)
            }
            crate::fomod::Staged::Plan(mut plan) => {
                if !preferred.is_empty() {
                    plan.name = preferred;
                }
                self.offer_plan(plan, source)
            }
            crate::fomod::Staged::Wizard(mut w) => {
                if !preferred.is_empty() {
                    w.folder_name = preferred;
                }
                self.prime_wizard(&mut w);
                self.install_source = Some(source);
                self.wizard = Some(w);
                Ok(None)
            }
            crate::fomod::Staged::ChooseRoot { name, roots, temp } => {
                let name = if preferred.is_empty() { name } else { preferred };
                self.root_ask = Some(RootAsk { name, roots, temp, source });
                self.message = Some(("That archive has more than one folder that could be the mod. Pick one.".into(), false));
                Ok(None)
            }
        }
    }

    fn write_mod_order(&self) -> Result<(), String> {
        let home = self.home.as_ref().ok_or("no home folder")?;
        let game = self.game();
        std::fs::create_dir_all(&home.root).map_err(|e| format!("{}: {e}", home.root.display()))?;
        for (file, body) in [("mod-order.txt", Self::modlist_text(&self.mod_rows)), (HOME_LOCKS, Self::locks_text(&self.mod_rows))] {
            let path = home.root.join(file);
            guard_write(&path, game.as_ref()).map_err(|e| e.to_string())?;
            std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(())
    }

    fn apply_mods(&mut self) -> Result<(), String> {
        self.save_mods()?;
        self.mods_dirty = false;
        Ok(())
    }

    fn set_mod_on(&mut self, index: usize, on: bool) {
        let Some(row) = self.mod_rows.get(index) else { return };
        if row.separator || row.on == on {
            return;
        }
        let folder = row.name.clone();
        self.mod_rows[index].on = on;
        let keys: HashSet<String> = self.plugin_home.iter().filter(|(_, owned)| *owned == &folder).map(|(key, _)| key.clone()).collect();
        // A plugin already in the game Data folder stays on when this folder is turned off.
        let in_data: HashSet<String> = self.game().map(|g| keys.iter().filter(|name| g.data_file(name).is_some()).cloned().collect()).unwrap_or_default();
        if on {
            let remembered = self.plugin_ticks.get(&folder).cloned();
            if let Some(home) = self.home.clone() {
                let names: Vec<String> = plugins::plugins_in_tree(&home.mods().join(&folder)).into_iter().map(|(n, _)| n).collect();
                if let Some(list) = &mut self.mods {
                    list.add_new(&names);
                    for entry in &mut list.entries {
                        let key = entry.name.to_ascii_lowercase();
                        if !names.iter().any(|n| n.eq_ignore_ascii_case(&key)) {
                            continue;
                        }
                        let restore = remembered.as_ref().is_none_or(|set| set.is_empty() || set.iter().any(|n| n.eq_ignore_ascii_case(&entry.name)));
                        entry.active = restore;
                    }
                }
            }
        } else if let Some(list) = &mut self.mods {
            // Unticking a mod unticks its plugins and leaves them in the load order.
            let mut remembered = HashSet::new();
            for entry in &mut list.entries {
                if entry.name.eq_ignore_ascii_case("FalloutNV.esm") {
                    continue;
                }
                let key = entry.name.to_ascii_lowercase();
                if keys.contains(&key) && !in_data.contains(&key) {
                    if entry.active {
                        remembered.insert(entry.name.clone());
                    }
                    entry.active = false;
                }
            }
            self.plugin_ticks.insert(folder.clone(), remembered);
        } else {
            self.plugin_ticks.insert(folder.clone(), HashSet::new());
        }
        self.mods_dirty = true;
        self.recompute_clashes();
    }

    fn shared_paths(&self, name: &str) -> String {
        let Some(notes) = self.clashes.get(name) else { return String::new() };
        let mut lines = Vec::new();
        for note in notes {
            let which = if note.wins { "used" } else { "skipped" };
            for file in &note.files {
                lines.push(format!("{file}\t{which}\t{}", note.other));
            }
        }
        lines.sort();
        lines.dedup();
        lines.join("\n")
    }

    #[allow(clippy::too_many_arguments)]
    fn mod_row_menu(ui: &mut egui::Ui, _on: bool, index: usize, count: usize, shared: bool, locked: bool, separator: bool, paths: &str, can_undo: bool, act: &mut Option<ModAct>) {
        if can_undo && ui.button("Undo last drag").clicked() {
            *act = Some(ModAct::Undo);
            ui.close();
        }
        if separator {
            if ui.button("Separator above").on_hover_text("Puts a named separator above this row.").clicked() {
                *act = Some(ModAct::Separator(index, true));
                ui.close();
            }
            if ui.button("Separator below").on_hover_text("Puts a named separator below this row.").clicked() {
                *act = Some(ModAct::Separator(index, false));
                ui.close();
            }
            if ui.button("Make mod below").on_hover_text("Turns loose files in Overwrite into a mod under this row. The game folder is not used.").clicked() {
                *act = Some(ModAct::MakeBelow(index));
                ui.close();
            }
            if ui.button("Rename…").on_hover_text("Or double-click the separator.").clicked() {
                *act = Some(ModAct::RenameSeparator(index));
                ui.close();
            }
            if ui.button("Remove…").clicked() {
                *act = Some(ModAct::Recycle(index));
                ui.close();
            }
            return;
        }
        if ui.button("Enable selected").clicked() {
            *act = Some(ModAct::SetSelected(true));
            ui.close();
        }
        if ui.button("Disable selected").clicked() {
            *act = Some(ModAct::SetSelected(false));
            ui.close();
        }
        if ui.button("Rename mod…").clicked() {
            *act = Some(ModAct::Rename(index));
            ui.close();
        }
        if ui.button("Remove mod…").clicked() {
            *act = Some(ModAct::Recycle(index));
            ui.close();
        }
        if ui.button("Create backup").on_hover_text("A dated copy of the mod list and the load order. The mod files stay.").clicked() {
            *act = Some(ModAct::Backup);
            ui.close();
        }
        if ui.button("Open in Explorer").clicked() {
            *act = Some(ModAct::Open(index));
            ui.close();
        }
        ui.separator();
        if ui.add_enabled(index > 0, egui::Button::new("Move to top")).clicked() {
            *act = Some(ModAct::Top(index));
            ui.close();
        }
        if ui.add_enabled(index + 1 < count, egui::Button::new("Move to bottom")).clicked() {
            *act = Some(ModAct::Bottom(index));
            ui.close();
        }
        if ui.button("Show conflicts").clicked() {
            *act = Some(ModAct::Overwrites);
            ui.close();
        }
        if ui.add_enabled(shared, egui::Button::new("Copy conflict paths")).clicked() {
            ui.ctx().copy_text(paths.to_string());
            *act = Some(ModAct::Copied(paths.lines().count()));
            ui.close();
        }
        if ui.button(if locked { "Unlock" } else { "Lock" }).on_hover_text("A locked row stays where it is when another row is dragged.").clicked() {
            *act = Some(ModAct::Lock(index));
            ui.close();
        }
        if ui.button("Note…").clicked() {
            *act = Some(ModAct::Note(index));
            ui.close();
        }
        if ui.button("Make mod below").on_hover_text("Turns loose files in Overwrite into a mod under this row. The game folder is not used.").clicked() {
            *act = Some(ModAct::MakeBelow(index));
            ui.close();
        }
        if ui.button("Separator above").on_hover_text("A named line in the mod list. Not a mod.").clicked() {
            *act = Some(ModAct::Separator(index, true));
            ui.close();
        }
        if ui.button("Separator below").on_hover_text("A named line in the mod list. Not a mod.").clicked() {
            *act = Some(ModAct::Separator(index, false));
            ui.close();
        }
        if ui.button("Rename…").clicked() {
            *act = Some(ModAct::Rename(index));
            ui.close();
        }
        if ui.button("Remove…").clicked() {
            *act = Some(ModAct::Recycle(index));
            ui.close();
        }
    }

    fn apply_mod_act(&mut self, act: ModAct) {
        match act {
            ModAct::Open(index) => self.open_mod_folder(index),
            ModAct::Top(index) => self.move_mod_row(index, 0),
            ModAct::Bottom(index) => {
                let last = self.mod_rows.len().saturating_sub(1);
                self.move_mod_row(index, last);
            }
            ModAct::Overwrites => self.show_overwrites = true,
            ModAct::Copied(n) => self.message = Some((format!("Copied {n} conflict paths."), false)),
            ModAct::Rename(index) => {
                if let Some(name) = self.mod_rows.get(index).map(|r| r.name.clone()) {
                    self.mod_rename_text = name;
                    self.mod_rename = Some(index);
                    self.mod_recycle = None;
                }
            }
            ModAct::Recycle(index) => {
                self.mod_recycle = Some(index);
                self.mod_rename = None;
            }
            ModAct::Lock(index) => {
                if let Some(row) = self.mod_rows.get_mut(index) {
                    if !row.separator {
                        row.locked = !row.locked;
                        self.mods_dirty = true;
                    }
                }
            }
            ModAct::Note(index) => {
                if let Some(name) = self.mod_rows.get(index).filter(|r| !r.separator).map(|r| r.name.clone()) {
                    self.mod_note = self.mod_note_path(&name).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
                    self.mod_note_for = Some(name);
                }
            }
            ModAct::Separator(index, above) => {
                self.separator_edit = None;
                self.separator_at = Some(index);
                self.separator_above = above;
                self.separator_name.clear();
                self.separator_focus = true;
            }
            ModAct::RenameSeparator(index) => {
                if let Some(name) = self.mod_rows.get(index).filter(|r| r.separator).map(|r| r.name.clone()) {
                    self.separator_at = None;
                    self.separator_edit = Some(index);
                    self.separator_name = name;
                    self.separator_focus = true;
                }
            }
            ModAct::MakeBelow(index) => self.make_mod_below(index),
            ModAct::Undo => self.undo_last_drag(),
            ModAct::SetSelected(on) => self.set_selected_mods(on),
            ModAct::Backup => self.save_dated_backup(),
        }
    }

    fn move_mod_row(&mut self, from: usize, to: usize) {
        if from >= self.mod_rows.len() || to >= self.mod_rows.len() {
            return;
        }
        let prior = self.mod_rows.clone();
        let next = reorder_mods(&prior, from, to);
        if next.iter().map(|r| &r.name).eq(prior.iter().map(|r| &r.name)) {
            return;
        }
        self.drag_undo = Some(prior);
        self.mod_rows = next;
        self.mod_drag = None;
        self.mods_dirty = true;
        self.recompute_clashes();
    }

    fn open_mod_folder(&mut self, index: usize) {
        let Some(name) = self.mod_rows.get(index).map(|r| r.name.clone()) else { return };
        let Some(home) = &self.home else { return };
        if Self::one_name(&name).is_err() {
            self.message = Some(("That folder was not opened.".into(), true));
            return;
        }
        let dir = home.mods().join(&name);
        if dir.is_dir() {
            launch::open_folder(&dir);
        } else {
            self.message = Some((format!("{name} is not in the mods folder."), true));
        }
    }

    fn mod_rename_line(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.mod_rename else { return };
        let Some(current) = self.mod_rows.get(index).map(|r| r.name.clone()) else {
            self.mod_rename = None;
            return;
        };
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.mod_rename_text).hint_text(&current).desired_width(ui.available_width() - 150.0));
            if ui.button("Rename").clicked() {
                self.rename_mod_at(index);
            }
            if ui.button("Cancel").clicked() {
                self.mod_rename = None;
            }
        });
    }

    fn mod_recycle_line(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.mod_recycle else { return };
        let Some(name) = self.mod_rows.get(index).map(|r| r.name.clone()) else {
            self.mod_recycle = None;
            return;
        };
        ui.horizontal(|ui| {
            let many = self.mod_selected.len() > 1 && self.mod_selected.contains(&name);
            if many {
                ui.label(RichText::new(format!("Move {} selected mods into recycle?", self.mod_selected.len())).small().color(BAD));
                if ui.button("Move").clicked() {
                    self.remove_selected_mods();
                }
            } else {
                ui.label(RichText::new(format!("Move {name} into recycle?")).small().color(BAD));
                if ui.button("Move").clicked() {
                    self.recycle_mod_at(index);
                }
            }
            if ui.button("Cancel").clicked() {
                self.mod_recycle = None;
            }
        });
    }

    fn rename_mod_at(&mut self, index: usize) {
        let Some(from) = self.mod_rows.get(index).map(|r| r.name.clone()) else { return };
        let to = match Self::one_name(&self.mod_rename_text) {
            Ok(name) => name,
            Err(e) => {
                self.message = Some((e, true));
                return;
            }
        };
        let Some(home) = self.home.clone() else { return };
        match Self::rename_mod_dir(&home.mods(), &from, &to) {
            Ok(()) => {
                self.mod_rows[index].name = to.clone();
                if self.plugin_only_folder.as_deref() == Some(from.as_str()) {
                    self.plugin_only_folder = Some(to.clone());
                }
                self.mod_rename = None;
                self.mods_dirty = true;
                self.refresh_plugin_notes();
                self.log_mod(&format!("renamed {from} to {to}"));
                self.message = Some((format!("Renamed {from} to {to}."), false));
            }
            Err(e) => self.message = Some((e, true)),
        }
    }

    fn recycle_mod_at(&mut self, index: usize) {
        let Some(row) = self.mod_rows.get(index) else { return };
        if row.separator {
            let name = row.name.clone();
            self.mod_rows.remove(index);
            self.mod_selected.remove(&name);
            self.mod_recycle = None;
            self.mods_dirty = true;
            return;
        }
        let name = row.name.clone();
        let Some(home) = self.home.clone() else { return };
        let recycle = home.root.join("recycle");
        if self.game().as_ref().is_some_and(|g| self.dir_is_under(&home.root, &g.root))
            || Self::under_original_documents(&home.root)
            || self.dir_is_under(&home.root, &home.mods())
        {
            self.message = Some(("Recycle was not used. The home is in the game folder, the original game's documents, or the mods folder.".into(), true));
            return;
        }
        match Self::move_mod_aside(&home.mods(), &recycle, &name) {
            Ok(dest) => {
                self.set_mod_on(index, false);
                if self.mod_rows.get(index).is_some_and(|r| r.name == name) {
                    self.mod_rows.remove(index);
                }
                if self.plugin_only_folder.as_deref() == Some(name.as_str()) {
                    self.plugin_only_folder = None;
                }
                self.mod_recycle = None;
                self.mod_rename = None;
                self.refresh_plugin_notes();
                self.mod_selected.remove(&name);
                self.log_mod(&format!("removed {name}"));
                self.message = Some((format!("Moved {name} to {}.", dest.display()), false));
            }
            Err(e) => self.message = Some((e, true)),
        }
    }

    fn plugin_row_menu(ui: &mut egui::Ui, name: &str, on: bool, base: bool, index: usize, folder: Option<String>, act: &mut Option<PluginAct>) {
        let _ = on;
        if ui.add_enabled(!base, egui::Button::new("Enable selected")).clicked() {
            *act = Some(PluginAct::Selected(true));
            ui.close();
        }
        if ui.add_enabled(!base, egui::Button::new("Disable selected")).clicked() {
            *act = Some(PluginAct::Selected(false));
            ui.close();
        }
        if ui.add_enabled(!base, egui::Button::new("Enable all")).on_hover_text("Turns on every plugin except FalloutNV.esm.").clicked() {
            *act = Some(PluginAct::All(true));
            ui.close();
        }
        if ui.add_enabled(!base, egui::Button::new("Disable all")).on_hover_text("Turns off every plugin except FalloutNV.esm.").clicked() {
            *act = Some(PluginAct::All(false));
            ui.close();
        }
        if ui.add_enabled(folder.is_some(), egui::Button::new("Open Origin in Explorer")).clicked() {
            *act = Some(PluginAct::Show(index));
            ui.close();
        }
        ui.separator();
        if ui.button("Copy name").clicked() {
            ui.ctx().copy_text(name.to_string());
            *act = Some(PluginAct::Copy(name.to_string()));
            ui.close();
        }
    }

    fn profile_menu(ui: &mut egui::Ui, owned: bool, act: &mut Option<ProfileAct>) {
        if ui.button("Open folder").clicked() {
            *act = Some(ProfileAct::Open);
            ui.close();
        }
        if ui.add_enabled(owned, egui::Button::new("Duplicate")).on_hover_text("Copies the four list files. A dropped folder is left as it was.").clicked() {
            *act = Some(ProfileAct::Duplicate);
            ui.close();
        }
        if ui.add_enabled(owned, egui::Button::new("Rename to the typed name")).on_hover_text("Uses the name in the box. A dropped folder is left as it was.").clicked() {
            *act = Some(ProfileAct::Rename);
            ui.close();
        }
        if ui.add_enabled(owned, egui::Button::new("Delete…")).on_hover_text("Removes this profile folder. Mod folders stay.").clicked() {
            *act = Some(ProfileAct::DeleteAsk);
            ui.close();
        }
    }

    fn profile_owned(&self, dir: &Path) -> bool {
        let Some(home) = &self.home else { return false };
        Self::profile_is_under(dir, &home.root.join("profiles"))
    }

    fn run_profile_act(&mut self, act: ProfileAct) {
        match act {
            ProfileAct::Open => self.open_picked_profile(),
            ProfileAct::Duplicate => self.duplicate_picked_profile(),
            ProfileAct::Rename => self.rename_picked_profile(),
            ProfileAct::DeleteAsk => {
                if self.profile_pick.is_some() {
                    self.profile_delete_ask = true;
                }
            }
        }
    }

    fn picked_profile_dir(&self) -> Option<PathBuf> {
        let name = self.profile_pick.as_ref()?;
        self.profile_choices.iter().find(|(n, _)| n == name).map(|(_, p)| p.clone())
    }

    fn open_picked_profile(&mut self) {
        let Some(dir) = self.picked_profile_dir() else {
            self.message = Some(("Choose a profile.".into(), true));
            return;
        };
        if dir.is_dir() {
            launch::open_folder(&dir);
        } else {
            self.message = Some(("That profile folder was not found.".into(), true));
        }
    }

    fn duplicate_picked_profile(&mut self) {
        let Some(from) = self.picked_profile_dir() else {
            self.message = Some(("Choose a profile.".into(), true));
            return;
        };
        let Some(home) = self.home.clone() else { return };
        if !self.profile_owned(&from) {
            self.message = Some(("That profile is only read. It was not copied.".into(), true));
            return;
        }
        let current = self.profile_pick.clone().unwrap_or_default();
        let typed = Self::profile_folder_name(&self.profile_name).ok();
        let name = match typed.filter(|n| !n.eq_ignore_ascii_case(&current)) {
            Some(name) => name,
            None => match Self::profile_folder_name(&format!("{current} copy")) {
                Ok(name) => name,
                Err(_) => {
                    self.message = Some(("Type a new name for the copy.".into(), true));
                    return;
                }
            },
        };
        let dest = home.root.join("profiles").join(&name);
        if self.profile_dir_refused(&dest, &home) {
            return;
        }
        match Self::copy_profile_lists(&from, &dest) {
            Ok(()) => {
                let parent = home.root.join("profiles");
               self.profile_choices = Self::profile_folders(&parent);
                self.profile_pick = Some(name.clone());
                self.profile_name = name.clone();
                self.message = Some((format!("Copied the lists to {name}."), false));
            }
            Err(e) => self.message = Some((e, true)),
        }
    }

    fn rename_picked_profile(&mut self) {
        let Some(from) = self.picked_profile_dir() else {
            self.message = Some(("Choose a profile.".into(), true));
            return;
        };
        let Some(home) = self.home.clone() else { return };
        if !self.profile_owned(&from) {
            self.message = Some(("That profile is only read. It was not renamed.".into(), true));
            return;
        }
        let name = match Self::profile_folder_name(&self.profile_name) {
            Ok(name) => name,
            Err(e) => {
                self.message = Some((e, true));
                return;
            }
        };
        let current = self.profile_pick.clone().unwrap_or_default();
        if name.eq_ignore_ascii_case(&current) {
            self.message = Some(("Type a new name first.".into(), true));
            return;
        }
        let dest = home.root.join("profiles").join(&name);
        if self.profile_dir_refused(&dest, &home) {
            return;
        }
        if dest.exists() {
            self.message = Some(("That profile is already there.".into(), true));
            return;
        }
        match std::fs::rename(&from, &dest) {
            Ok(()) => {
                let parent = home.root.join("profiles");
               self.profile_choices = Self::profile_folders(&parent);
                self.profile_pick = Some(name.clone());
                self.profile_name = name.clone();
                self.message = Some((format!("Renamed the profile to {name}."), false));
            }
            Err(e) => self.message = Some((format!("{}: {e}", from.display()), true)),
        }
    }

    fn delete_picked_profile(&mut self) {
        let Some(dir) = self.picked_profile_dir() else {
            self.profile_delete_ask = false;
            self.message = Some(("Choose a profile.".into(), true));
            return;
        };
        let Some(home) = self.home.clone() else { return };
        self.profile_delete_ask = false;
        if !self.profile_owned(&dir) {
            self.message = Some(("That profile is only read. It was not deleted.".into(), true));
            return;
        }
        match Self::remove_profile_dir(&dir) {
            Ok(()) => {
                let parent = home.root.join("profiles");
               self.profile_choices = Self::profile_folders(&parent);
                self.profile_pick = None;
                self.message = Some(("Deleted that profile. Mod folders were left.".into(), false));
            }
            Err(e) => self.message = Some((e, true)),
        }
    }

    fn profile_row(&mut self, ui: &mut egui::Ui) {
        let rows: Vec<(String, bool)> = self.profile_choices.iter().map(|(n, p)| (n.clone(), self.profile_owned(p))).collect();
        let pick_owned = self.profile_pick.as_ref().and_then(|n| rows.iter().find(|(name, _)| name == n).map(|(_, owned)| *owned)).unwrap_or(false);
        let mut act: Option<ProfileAct> = None;
        let mut target: Option<String> = None;
        let block = ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            let heading = ui.add(egui::Label::new(RichText::new("Profiles").small().color(GOLD)).sense(Sense::click()));
            heading.on_hover_text("Right-click the chosen profile.").context_menu(|ui| Self::profile_menu(ui, pick_owned, &mut act));
            ui.horizontal_wrapped(|ui| {
                let name_w = (ui.available_width() - 120.0).clamp(80.0, 420.0);
                let name_edit = ui.add(egui::TextEdit::singleline(&mut self.profile_name).hint_text("Name this profile").desired_width(name_w));
                if name_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.ask_new_profile();
                }
                if ui.button("Add profile").on_hover_text("Saves this mod list and load order under that name, in the profiles folder next to mods. Nothing is copied into Data.").clicked() {
                    self.ask_new_profile();
                }
            });
            if let Some(name) = self.profile_replace.clone() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{name} already has a mod list.")).small().color(BAD));
                    if ui.button("Replace").clicked() {
                        self.write_profile_named(&name);
                    }
                    if ui.button("Cancel").clicked() {
                        self.profile_replace = None;
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                let shown = self.profile_pick.clone().unwrap_or_else(|| {
                    if self.profile_choices.is_empty() { "no profiles".into() } else { "choose a profile".into() }
                });
                let combo_w = (ui.available_width() - 70.0).clamp(100.0, 240.0);
                let combo = egui::ComboBox::from_id_salt("profile-which").width(combo_w).selected_text(shown).show_ui(ui, |ui| {
                    for (name, owned) in &rows {
                        let resp = ui.selectable_label(self.profile_pick.as_deref() == Some(name.as_str()), name);
                        if resp.clicked() {
                            self.profile_pick = Some(name.clone());
                            self.profile_name = name.clone();
                            self.profile_delete_ask = false;
                        }
                        let item_name = name.clone();
                        let item_owned = *owned;
                        resp.context_menu(|ui| {
                            target = Some(item_name);
                            Self::profile_menu(ui, item_owned, &mut act);
                        });
                    }
                });
                combo.response.on_hover_text("Profiles live next to the mods folder. Right-click one.").context_menu(|ui| Self::profile_menu(ui, pick_owned, &mut act));
                if ui.add_enabled(self.profile_pick.is_some(), egui::Button::new("Load")).on_hover_text("Puts that profile's mod list and load order on the page. Files stay in the mods folder.").clicked() {
                    self.load_chosen_profile();
                }
            });
            if let Some(name) = target.take() {
                self.profile_pick = Some(name);
            }
            if let Some(act) = act.take() {
                self.run_profile_act(act);
            }
            if self.profile_delete_ask {
                let name = self.profile_pick.clone().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Delete profile {name}?")).small().color(BAD));
                    if ui.button("Delete").clicked() {
                        self.delete_picked_profile();
                    }
                    if ui.button("Cancel").clicked() {
                        self.profile_delete_ask = false;
                    }
                });
            }
        });
        self.profile_rect = block.response.rect;
    }

    /// Lists profiles in the launcher's profiles folder, next to mods.
    fn refresh_owned_profiles(&mut self) {
        let Some(home) = self.home.clone() else { return };
        let parent = home.root.join("profiles");
        let _ = std::fs::create_dir_all(&parent);
        self.profile_choices = Self::profile_folders(&parent);
    }

    /// Writes the open profile when one is open. Otherwise only the launcher home lists. Data is not linked.
    fn save_open_profile(&mut self) {
        if let Some(dir) = self.picked_profile_dir() {
            if !self.profile_owned(&dir) {
                match self.save_mods() {
                    Ok(()) => {
                        self.mods_dirty = false;
                        self.saved_lists = self.lists_text();
                        self.message = Some(("That profile is only read. The home lists were saved. Nothing was linked into Data.".into(), false));
                    }
                    Err(e) => self.message = Some((e, true)),
                }
                return;
            }
        }
        let Some(name) = self.profile_pick.clone() else {
            match self.save_mods() {
                Ok(()) => {
                    self.mods_dirty = false;
                    self.saved_lists = self.lists_text();
                    self.message = Some(("Lists saved in the launcher home. No profile is open. Nothing was linked into Data.".into(), false));
                }
                Err(e) => self.message = Some((e, true)),
            }
            return;
        };
        self.write_profile_named(&name);
    }

    fn save_as_profile(&mut self) {
        let name = match Self::profile_folder_name(&self.profile_name) {
            Ok(name) => name,
            Err(e) => {
                self.message = Some((e, true));
                return;
            }
        };
        let Some(home) = self.home.clone() else {
            self.message = Some(("No home folder yet.".into(), true));
            return;
        };
        let dir = home.root.join("profiles").join(&name);
        if dir.join("modlist.txt").is_file() && self.profile_replace.as_deref() != Some(name.as_str()) {
            self.profile_replace = Some(name);
            return;
        }
        self.write_profile_named(&name);
    }

    fn write_profile_named(&mut self, name: &str) {
        let Some(home) = self.home.clone() else {
            self.message = Some(("No home folder yet.".into(), true));
            return;
        };
        if self.mod_rows.is_empty() && self.mods.as_ref().is_none_or(|list| list.entries.is_empty()) {
            self.message = Some(("Nothing to save.".into(), true));
            return;
        }
        let parent = home.root.join("profiles");
        let dir = parent.join(name);
        if self.profile_dir_refused(&dir, &home) {
            return;
        }
        if dir.exists() && !self.profile_owned(&dir) {
            self.message = Some(("That profile is only read. It was not written.".into(), true));
            return;
        }
        if self.mods.is_some() {
            if let Err(e) = self.save_mods() {
                self.message = Some((e, true));
                return;
            }
        }
        if dir.join("modlist.txt").is_file() {
            if let Err(e) = crate::modlist::keep_history(&dir, 5) {
                self.message = Some((e, true));
                return;
            }
        }
        if let Err(e) = Self::write_profile_lists(&dir, &self.mod_rows, self.mods.as_ref()) {
            self.message = Some((e, true));
            return;
        }
        if let Err(e) = self.write_profile_side(&dir) {
            self.message = Some((e, true));
            return;
        }
        self.profile_name = name.to_string();
        self.profile_choices = Self::profile_folders(&parent);
        self.profile_pick = Some(name.to_string());
        self.profile_replace = None;
        self.saved_lists = self.lists_text();
        self.mods_dirty = false;
        self.remember_profile(name);
        self.log_mod(&format!("saved profile {name}"));
        self.message = Some((format!("Saved profile {name} next to the mods folder. Nothing was copied into Data."), false));
    }

    fn write_profile_side(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(dir.join("note.txt"), &self.profile_note).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut archives = self.profile_archives.clone();
        archives.sort();
        archives.dedup();
        let body = if archives.is_empty() { String::new() } else { format!("{}\n", archives.join("\n")) };
        std::fs::write(dir.join("archives.txt"), body).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(())
    }

    fn profile_dir_refused(&mut self, dir: &Path, home: &EngineHome) -> bool {
        let game_root = self.game().map(|g| g.root);
        if game_root.as_ref().is_some_and(|root| self.dir_is_under(dir, root)) || Self::under_original_documents(dir) || self.dir_is_under(dir, &home.mods()) {
            self.message = Some(("That profile would be written in the game folder, the original game's documents, or the mods folder. It was not written.".into(), true));
            return true;
        }
        false
    }

    fn dir_is_under(&self, path: &Path, root: &Path) -> bool {
        if root.as_os_str().is_empty() {
            return false;
        }
        match (path.canonicalize(), root.canonicalize()) {
            (Ok(path), Ok(root)) => path == root || path.starts_with(&root),
            _ => false,
        }
    }

    /// True when `path` is the original game's documents folder, or inside it.
    fn under_original_documents(path: &Path) -> bool {
        let parts: Vec<String> = path.components().map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase()).collect();
        parts.windows(2).any(|pair| pair[0] == "my games" && pair[1] == "falloutnv")
    }

    fn load_chosen_profile(&mut self) {
        let Some(name) = self.profile_pick.clone() else {
            self.message = Some(("Choose a profile.".into(), true));
            return;
        };
        let Some(dir) = self.profile_choices.iter().find(|(n, _)| n == &name).map(|(_, p)| p.clone()) else {
            self.message = Some(("Choose a profile.".into(), true));
            return;
        };
        let mods_text = std::fs::read_to_string(dir.join("modlist.txt")).unwrap_or_default();
        let plugins_text = std::fs::read_to_string(dir.join("plugins.txt")).unwrap_or_default();
        let order_text = std::fs::read_to_string(dir.join("loadorder.txt")).unwrap_or_default();
        if mods_text.trim().is_empty() && plugins_text.trim().is_empty() && order_text.trim().is_empty() {
            self.message = Some(("That profile has no mod list and no load order.".into(), true));
            return;
        }
        // A profile load only puts the lists on the page. It does not link or copy mod files.
        let locks_text = std::fs::read_to_string(dir.join(PROFILE_LOCKS)).unwrap_or_default();
        let profile_mods = Self::profile_page_rows(&mods_text, &locks_text);
        let mut placed = Vec::new();
        let mut missing = 0usize;
        let home_mods = self.home.as_ref().map(|h| h.mods());
        for row in &profile_mods {
            if row.separator {
                placed.push(row.clone());
                continue;
            }
            if Self::find_mod_dir(&dir, &row.name, home_mods.as_deref()).is_none() && home_mods.as_ref().is_none_or(|mods| !mods.join(&row.name).is_dir()) {
                missing += 1;
                continue;
            }
            placed.push(row.clone());
        }
        let folders = placed.iter().filter(|r| !r.separator).count();
        if !placed.is_empty() {
            let mut rows = Vec::new();
            for row in &self.mod_rows {
                if !placed.iter().any(|p| p.name.eq_ignore_ascii_case(&row.name)) {
                    rows.push(ModRow { name: row.name.clone(), on: row.on, separator: row.separator, locked: row.locked });
                }
            }
            rows.extend(placed);
            self.mod_rows = rows;
        }
        let active = Self::profile_plugin_names(&plugins_text);
        let mut order = Self::profile_plugin_names(&order_text);
        if order.is_empty() {
            order = active.clone();
        }
        let active_keys: HashSet<String> = active.iter().map(|n| n.to_ascii_lowercase()).collect();
        if !order.is_empty() || !active.is_empty() {
            let mut entries = Vec::new();
            let mut seen = HashSet::new();
            if !order.iter().any(|n| n.eq_ignore_ascii_case("FalloutNV.esm")) {
                entries.push(plugins::PluginEntry { name: "FalloutNV.esm".into(), active: true });
                seen.insert("falloutnv.esm".to_string());
            }
            for plugin in &order {
                let key = plugin.to_ascii_lowercase();
                if !seen.insert(key.clone()) {
                    continue;
                }
                let on = plugin.eq_ignore_ascii_case("FalloutNV.esm") || active_keys.contains(&key);
                entries.push(plugins::PluginEntry { name: plugin.clone(), active: on });
            }
            if let Some(list) = &self.mods {
                for entry in &list.entries {
                    if seen.insert(entry.name.to_ascii_lowercase()) {
                        entries.push(plugins::PluginEntry { name: entry.name.clone(), active: false });
                    }
                }
            }
            self.mods = Some(plugins::PluginList { entries });
        }
        self.mods_dirty = true;
        self.refresh_plugin_notes();
        self.recompute_clashes();
        self.profile_note = std::fs::read_to_string(dir.join("note.txt")).unwrap_or_default();
        self.profile_archives = std::fs::read_to_string(dir.join("archives.txt")).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        self.saved_lists = self.lists_text();
        self.mods_dirty = false;
        self.remember_profile(&name);
        self.log_mod(&format!("loaded profile {name}"));
        self.message = Some((format!("Loaded {name}: {folders} mods in the list, {missing} not in the mods folder. Nothing was linked or copied."), false));
    }

    /// The mod folder named `name`: beside the profile, in the mods folder, or inside a folder there.
    fn find_mod_dir(profile_dir: &Path, name: &str, home_mods: Option<&Path>) -> Option<PathBuf> {
        let mut found = Vec::new();
        if let Some(mods) = home_mods {
            found.push(mods.join(name));
            if let Ok(rd) = std::fs::read_dir(mods) {
                for entry in rd.flatten() {
                    found.push(entry.path().join(name));
                }
            }
        }
        if let Some(instance) = profile_dir.parent().and_then(|p| p.parent()) {
            if let Some(dir) = Self::instance_subdir(instance, "mod_directory", "mods") {
                found.push(dir.join(name));
            }
            found.push(instance.join("mods").join(name));
        }
        found.into_iter().find(|p| p.is_dir())
    }

    fn instance_subdir(instance: &Path, key: &str, default: &str) -> Option<PathBuf> {
        let rd = std::fs::read_dir(instance).ok()?;
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ini")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Some(raw) = Self::ini_setting(&text, "Settings", key) else { continue };
            let raw = raw.replace("%BASE_DIR%", &instance.to_string_lossy());
            let dir = PathBuf::from(&raw);
            let dir = if dir.is_absolute() { dir } else { instance.join(dir) };
            if dir.is_dir() {
                return Some(dir);
            }
        }
        let fallback = instance.join(default);
        fallback.is_dir().then_some(fallback)
    }

    fn profile_folders(root: &Path) -> Vec<(String, PathBuf)> {
        let mut dirs = vec![root.to_path_buf(), root.join("profiles")];
        if let Some(extra) = Self::profiles_dir_from_settings(root) {
            dirs.push(extra);
        }
        let mut found = Vec::new();
        let mut seen = HashSet::new();
        for dir in dirs {
            if dir.join("modlist.txt").is_file() && seen.insert(dir.clone()) {
                let label = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "profile".into());
                found.push((label, dir));
                continue;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir() && path.join("modlist.txt").is_file() && seen.insert(path.clone()) {
                    found.push((entry.file_name().to_string_lossy().into_owned(), path));
                }
            }
        }
        found.sort_by_key(|row| row.0.to_ascii_lowercase());
        found
    }

    fn profiles_dir_from_settings(root: &Path) -> Option<PathBuf> {
        let rd = std::fs::read_dir(root).ok()?;
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ini")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Some(raw) = Self::ini_setting(&text, "Settings", "profiles_directory") else { continue };
            let raw = raw.replace("%BASE_DIR%", &root.to_string_lossy());
            let dir = PathBuf::from(&raw);
            let dir = if dir.is_absolute() { dir } else { root.join(dir) };
            if dir.is_dir() {
                return Some(dir);
            }
        }
        None
    }

    fn ini_setting(text: &str, section: &str, key: &str) -> Option<String> {
        let mut here = false;
        for line in text.lines() {
            let t = line.trim();
            if let Some(name) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                here = name.eq_ignore_ascii_case(section);
                continue;
            }
            if !here {
                continue;
            }
            let Some((k, v)) = t.split_once('=') else { continue };
            if !k.trim().eq_ignore_ascii_case(key) {
                continue;
            }
            let v = v.trim();
            let v = v.strip_prefix("@ByteArray(").and_then(|s| s.strip_suffix(')')).unwrap_or(v);
            return Some(v.replace("\\\\", "\\"));
        }
        None
    }

    /// A profile's rows, separators included, the winning folder last on the page. `locks`
    /// is its lock side file's text.
    fn profile_page_rows(text: &str, locks: &str) -> Vec<ModRow> {
        let mut rows = fnv_install::mod_order::read_modlist(text);
        fnv_install::mod_order::apply_locks(&mut rows, &fnv_install::mod_order::read_locks(locks));
        rows
    }

    fn lists_text(&self) -> String {
        let mods = format!("{}{}", Self::modlist_text(&self.mod_rows), Self::locks_text(&self.mod_rows));
        let (plugins_text, order) = Self::plugin_list_text(self.mods.as_ref());
        let archives = self.profile_archives.join("\n");
        format!("{mods}\n{plugins_text}\n{order}\n{}\n{archives}", self.profile_note)
    }

    fn remember_profile(&mut self, name: &str) {
        self.set_launcher_key("Profile", name);
    }

    fn log_mod(&self, line: &str) {
        let Some(home) = &self.home else { return };
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = crate::modlist::append_log(&home.logs().join("mods.log"), &format!("{stamp} {line}"));
    }

    /// Picks up new mod folders without rewriting the load order or dropping unsaved drags.
    fn rescan_mods(&mut self) {
        let Some(home) = self.home.clone() else { return };
        self.merge_found_mods(&home);
        self.message = Some(("Mod list refreshed. The load order was not rewritten.".into(), false));
    }

    fn merge_found_mods(&mut self, home: &EngineHome) {
        let mods = home.mods();
        let mut found = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&mods) {
            for entry in rd.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else { continue };
                if name.starts_with('.') || crate::modlist::is_tool_dir(&name) {
                    continue;
                }
                found.push(name);
            }
        }
        found.sort_by_key(|a| a.to_ascii_lowercase());
        let mut next = Vec::new();
        for row in &self.mod_rows {
            if row.separator || found.iter().any(|n| n.eq_ignore_ascii_case(&row.name)) {
                next.push(row.clone());
            }
        }
        for name in found {
            if !next.iter().any(|r| !r.separator && r.name.eq_ignore_ascii_case(&name)) {
                next.push(ModRow::module(name, true));
            }
        }
        self.mod_rows = next;
        self.refresh_plugin_notes();
    }

    fn mod_row_visible(&self, index: usize, find: &str) -> bool {
        let Some(row) = self.mod_rows.get(index) else { return false };
        if row.separator {
            return find.is_empty() && self.mod_filter == 0;
        }
        if !find.is_empty() && !row.name.to_ascii_lowercase().contains(find) {
            return false;
        }
        match self.mod_filter {
            1 => row.on,
            2 => !row.on,
            3 => self.clashes.contains_key(&row.name),
            4 => self.folder_counts.get(&row.name).copied().unwrap_or(0) > 0,
            _ => true,
        }
    }

    fn commit_separator(&mut self) {
        let name = self.separator_name.trim();
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains('%') {
            self.message = Some(("Type a separator name. No slash.".into(), true));
            return;
        }
        let name = name.to_string();
        if let Some(index) = self.separator_edit {
            if self.mod_rows.get(index).is_some_and(|r| r.separator) {
                self.mod_rows[index].name = name;
                self.mods_dirty = true;
            }
            self.separator_edit = None;
            self.separator_name.clear();
            return;
        }
        let Some(after) = self.separator_at else { return };
        let at = if self.separator_above { after } else { after.saturating_add(1) };
        let at = at.min(self.mod_rows.len());
        self.mod_rows.insert(at, ModRow { name, on: false, separator: true, locked: false });
        self.mods_dirty = true;
        self.separator_at = None;
        self.separator_name.clear();
    }

    fn undo_last_drag(&mut self) {
        if let Some(rows) = self.drag_undo.take() {
            self.mod_rows = rows;
            self.mods_dirty = true;
            self.recompute_clashes();
        }
    }

    fn set_selected_mods(&mut self, on: bool) {
        let names: Vec<String> = self.mod_selected.iter().cloned().collect();
        if names.is_empty() {
            return;
        }
        for name in names {
            if let Some(index) = self.mod_rows.iter().position(|r| !r.separator && r.name == name) {
                self.set_mod_on(index, on);
            }
        }
    }

    fn remove_selected_mods(&mut self) {
        let names = self.mod_selected.clone();
        let indexes: Vec<usize> = self.mod_rows.iter().enumerate().filter(|(_, r)| names.contains(&r.name)).map(|(i, _)| i).collect();
        for index in indexes.into_iter().rev() {
            self.recycle_mod_at(index);
        }
        self.mod_selected.clear();
        self.mod_recycle = None;
    }

    fn install_warn_row(&mut self, ui: &mut egui::Ui) {
        if self.install_warn.is_empty() {
            return;
        }
        ui.label(RichText::new("These loose files are already in a ticked mod. The lower mod would win.").small().color(BAD));
        for line in self.install_warn.iter().take(8) {
            ui.label(RichText::new(line).small().color(WEAK));
        }
        ui.horizontal(|ui| {
            if ui.button("Install anyway").clicked() {
                self.confirm_held();
            }
            if ui.button("Cancel install").clicked() {
                self.clear_held();
                self.message = Some(("Install cancelled. Nothing was copied.".into(), false));
            }
        });
    }

    fn root_ask_row(&mut self, ui: &mut egui::Ui) {
        let Some(ask) = &self.root_ask else { return };
        let labels: Vec<String> = ask.roots.iter().map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("folder").to_string()).collect();
        ui.label(RichText::new("Which folder holds the plugin or the meshes?").small().color(WEAK));
        ui.horizontal_wrapped(|ui| {
            for (i, label) in labels.iter().enumerate() {
                if ui.button(label).clicked() {
                    self.pick_root(i);
                }
            }
            if ui.button("Cancel").clicked() {
                self.cancel_root();
            }
        });
    }

    fn mod_note_row(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.mod_note_for.clone() else { return };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Note for {name}")).small().color(WEAK));
            ui.add(egui::TextEdit::singleline(&mut self.mod_note).desired_width((ui.available_width() - 140.0).max(80.0)));
            if ui.button("Save note").clicked() {
                self.save_mod_note();
            }
            if ui.button("Close").clicked() {
                self.mod_note_for = None;
            }
        });
    }

    fn clear_held(&mut self) {
        if let Some(held) = self.held.take() {
            match held {
                HeldInstall::Tree { temp, .. } => crate::fomod::discard(&temp),
                HeldInstall::Plan { plan, .. } => crate::fomod::discard(&plan.temp),
            }
        }
        self.install_warn.clear();
        self.install_ready = None;
    }

    fn loose_clashes(&self, incoming: &[String]) -> Vec<String> {
        let enabled: Vec<(String, Vec<String>)> = self.mod_rows.iter().filter(|r| r.on && !r.separator).filter_map(|r| self.folder_files.get(&r.name).map(|files| (r.name.clone(), files.clone()))).collect();
        let borrowed: Vec<(&str, &[String])> = enabled.iter().map(|(n, f)| (n.as_str(), f.as_slice())).collect();
        crate::modlist::clashes_with(incoming, &borrowed)
    }

    fn offer_tree(&mut self, name: String, from: PathBuf, temp: Option<PathBuf>, source: PathBuf) -> Result<Option<String>, String> {
        let warn = self.loose_clashes(&mod_file_keys(&from));
        if warn.is_empty() {
            self.commit_tree(&name, &from, &temp, &source).map(Some)
        } else {
            self.install_warn = warn;
            self.install_ready = Some(source.clone());
            self.held = Some(HeldInstall::Tree { name, from, temp, source });
            self.message = Some(("Those loose files are already in a ticked mod. Install anyway to copy them, or cancel.".into(), true));
            Ok(None)
        }
    }

    fn offer_plan(&mut self, plan: crate::fomod::Plan, source: PathBuf) -> Result<Option<String>, String> {
        let warn = self.loose_clashes(&crate::fomod::plan_loose_files(&plan));
        if warn.is_empty() {
            self.commit_plan(&plan, &source).map(Some)
        } else {
            self.install_warn = warn;
            self.install_ready = Some(source.clone());
            self.held = Some(HeldInstall::Plan { plan, source });
            self.message = Some(("Those loose files are already in a ticked mod. Install anyway to copy them, or cancel.".into(), true));
            Ok(None)
        }
    }

    fn confirm_held(&mut self) {
        let Some(held) = self.held.take() else { return };
        self.install_warn.clear();
        let ready = self.install_ready.take();
        let result = match held {
            HeldInstall::Tree { name, from, temp, source } => self.commit_tree(&name, &from, &temp, &source),
            HeldInstall::Plan { plan, source } => self.commit_plan(&plan, &source),
        };
        let _ = ready;
        match result {
            Ok(name) => self.finish_import(Ok(name)),
            Err(e) => self.message = Some((e, true)),
        }
    }

    fn commit_tree(&mut self, name: &str, from: &Path, temp: &Option<PathBuf>, source: &Path) -> Result<String, String> {
        let home = self.home.clone().ok_or_else(|| "no home folder".to_string())?;
        let mods = home.mods();
        let dest = mods.join(name);
        if dest.exists() {
            Self::mod_dir_is_inside(&dest, &mods)?;
        }
        let recycle = self.recycle_dir()?;
        let placed = match crate::fomod::replace_tree(&mods, name, from, &recycle) {
            Ok(name) => name,
            Err(e) => {
                crate::fomod::discard(temp);
                return Err(e);
            }
        };
        crate::fomod::discard(temp);
        self.note_install(&placed, source);
        self.log_mod(&format!("installed {placed}"));
        Ok(placed)
    }

    fn commit_plan(&mut self, plan: &crate::fomod::Plan, source: &Path) -> Result<String, String> {
        let home = self.home.clone().ok_or_else(|| "no home folder".to_string())?;
        let mods = home.mods();
        let dest = mods.join(&plan.name);
        if dest.exists() {
            Self::mod_dir_is_inside(&dest, &mods)?;
        }
        let recycle = self.recycle_dir()?;
        let temp = plan.temp.clone();
        let placed = match crate::fomod::replace_plan(&mods, plan, &recycle) {
            Ok(name) => name,
            Err(e) => {
                crate::fomod::discard(&temp);
                return Err(e);
            }
        };
        crate::fomod::discard(&temp);
        self.note_install(&placed, source);
        self.log_mod(&format!("installed {placed}"));
        Ok(placed)
    }

    fn recycle_dir(&self) -> Result<PathBuf, String> {
        let home = self.home.as_ref().ok_or_else(|| "no home folder".to_string())?;
        let recycle = home.root.join("recycle");
        if self.game().as_ref().is_some_and(|g| self.dir_is_under(&home.root, &g.root)) || Self::under_original_documents(&home.root) || self.dir_is_under(&home.root, &home.mods()) {
            return Err("Recycle was not used. The home is in the game folder, the original game's documents, or the mods folder.".into());
        }
        Ok(recycle)
    }

    fn note_install(&self, name: &str, source: &Path) {
        if Self::one_name(name).is_err() {
            return;
        }
        let Some(home) = &self.home else { return };
        let dir = home.root.join("mod-meta").join(name);
        let _ = std::fs::create_dir_all(&dir);
        let when = backup_stamp();
        let _ = std::fs::write(dir.join("source.txt"), format!("{}\n{when}\n", source.display()));
    }

    fn prime_wizard(&self, wizard: &mut crate::fomod::Wizard) {
        let Some(home) = &self.home else { return };
        if Self::one_name(&wizard.folder_name).is_err() {
            return;
        }
        let path = home.root.join("mod-meta").join(&wizard.folder_name).join("choices.txt");
        if let Ok(text) = std::fs::read_to_string(path) {
            wizard.apply_choices(&text);
        }
    }

    fn store_choices(&self, wizard: &crate::fomod::Wizard) {
        if Self::one_name(&wizard.folder_name).is_err() {
            return;
        }
        let Some(home) = &self.home else { return };
        let dir = home.root.join("mod-meta").join(&wizard.folder_name);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("choices.txt"), wizard.choices_text());
    }

    fn mod_note_path(&self, name: &str) -> Option<PathBuf> {
        if Self::one_name(name).is_err() {
            return None;
        }
        Some(self.home.as_ref()?.root.join("mod-meta").join(name).join("note.txt"))
    }

    fn mod_source_line(&self, name: &str) -> Option<String> {
        if Self::one_name(name).is_err() {
            return None;
        }
        let path = self.home.as_ref()?.root.join("mod-meta").join(name).join("source.txt");
        let text = std::fs::read_to_string(path).ok()?;
        let mut lines = text.lines();
        let file = lines.next()?;
        let when = lines.next().unwrap_or("");
        let file = Path::new(file).file_name().and_then(|n| n.to_str()).unwrap_or(file);
        Some(format!("from {file} · {when}"))
    }

    fn save_mod_note(&mut self) {
        let Some(name) = self.mod_note_for.clone() else { return };
        let Some(path) = self.mod_note_path(&name) else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&path, &self.mod_note) {
            Ok(()) => self.message = Some((format!("Note saved for {name}. It is kept by the launcher, not inside the mod."), false)),
            Err(e) => self.message = Some((format!("{}: {e}", path.display()), true)),
        }
    }



    fn pick_root(&mut self, index: usize) {
        let Some(ask) = self.root_ask.take() else { return };
        let Some(root) = ask.roots.get(index).cloned() else {
            crate::fomod::discard(&ask.temp);
            return;
        };
        match self.offer_tree(ask.name, root, ask.temp, ask.source) {
            Ok(Some(name)) => self.finish_import(Ok(name)),
            Ok(None) => {}
            Err(e) => self.message = Some((e, true)),
        }
    }

    fn cancel_root(&mut self) {
        if let Some(ask) = self.root_ask.take() {
            crate::fomod::discard(&ask.temp);
        }
        self.message = Some(("Install cancelled. Nothing was copied.".into(), false));
    }

    /// Unused from the page. The load-order button was removed. The rule stays for a later pass.
    #[allow(dead_code)]
    fn masters_first_plugins(&mut self) {
        let Some(list) = self.mods.clone() else { return };
        let names: Vec<String> = list.entries.iter().map(|e| e.name.clone()).collect();
        let locked: Vec<bool> = names.iter().map(|n| n.eq_ignore_ascii_case("FalloutNV.esm")).collect();
        let esm: Vec<bool> = names.iter().map(|n| n.to_ascii_lowercase().ends_with(".esm")).collect();
        let readable = names.iter().all(|name| self.plugin_file(name).is_some_and(|path| header_ok(&path)));
        match crate::modlist::masters_first(&names, &locked, &esm, readable) {
            Ok(order) => {
                let Some(list) = &mut self.mods else { return };
                let mut entries = std::mem::take(&mut list.entries);
                let mut next = Vec::new();
                for name in &order {
                    if let Some(i) = entries.iter().position(|e| &e.name == name) {
                        next.push(entries.remove(i));
                    }
                }
                next.append(&mut entries);
                list.entries = next;
                self.mods_dirty = true;
                self.message = Some(("Masters moved above the other unlocked plugins.".into(), false));
            }
            Err(e) => self.message = Some((e, true)),
        }
    }

    fn plugin_file(&self, name: &str) -> Option<PathBuf> {
        if let Some(folder) = self.plugin_home.get(&name.to_ascii_lowercase()) {
            if let Some(home) = &self.home {
                let root = home.mods().join(folder);
                let direct = root.join(name);
                if direct.is_file() {
                    return Some(direct);
                }
                if let Some((_, path)) = plugins::plugins_in_tree(&root).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                    return Some(path);
                }
            }
        }
        self.game().and_then(|g| g.data_file(name))
    }

    fn overwrite_dir(&self) -> Option<PathBuf> {
        let home = self.home.as_ref()?;
        let dir = home.root.join("overwrite");
        if Self::under_original_documents(&dir) {
            return None;
        }
        if self.game().as_ref().is_some_and(|g| dir.starts_with(&g.root)) {
            return None;
        }
        Some(dir)
    }

    /// Loose files in Overwrite become a mod under the row that was right-clicked.
    fn make_mod_below(&mut self, index: usize) {
        if index >= self.mod_rows.len() {
            return;
        }
        let Some(dir) = self.overwrite_dir() else {
            self.message = Some(("Overwrite is not available.".into(), true));
            return;
        };
        if !dir.is_dir() || mod_file_keys(&dir).is_empty() {
            self.message = Some(("Overwrite is empty.".into(), true));
            return;
        }
        let Some(home) = self.home.clone() else { return };
        let name = self.free_mod_name(&home, "Overwrite");
        let dest = home.mods().join(&name);
        if let Err(e) = guard_write(&dest, self.game().as_ref()) {
            self.message = Some((e.to_string(), true));
            return;
        }
        if dest.exists() {
            self.message = Some(("That mod is already there.".into(), true));
            return;
        }
        let _ = std::fs::create_dir_all(home.mods());
        match std::fs::rename(&dir, &dest) {
            Ok(()) => {
                let _ = std::fs::create_dir_all(&dir);
                let at = index.saturating_add(1).min(self.mod_rows.len());
                self.mod_rows.insert(at, ModRow::module(name.clone(), true));
                self.adopt_new_plugins();
                self.log_mod(&format!("installed {name} from overwrite"));
                self.message = Some((format!("Made {name} under this row. The game folder was not used. Rename it from the menu if you want another name."), false));
            }
            Err(e) => self.message = Some((format!("{}: {e}", dir.display()), true)),
        }
    }

    fn free_mod_name(&self, home: &EngineHome, base: &str) -> String {
        let mods = home.mods();
        for n in 1..1000 {
            let name = if n == 1 { base.to_string() } else { format!("{base} {n}") };
            let taken = mods.join(&name).exists() || self.mod_rows.iter().any(|r| r.name.eq_ignore_ascii_case(&name));
            if !taken && Self::one_name(&name).is_ok() {
                return name;
            }
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        format!("{base} {stamp}")
    }

    fn save_dated_backup(&mut self) {
        let Some(home) = self.home.clone() else { return };
        if self.mods.is_none() && self.mod_rows.is_empty() {
            self.message = Some(("Nothing to save.".into(), true));
            return;
        }
        let stamp = backup_stamp();
        let dir = backup_dir(&home).join(format!("lists-{stamp}"));
        if self.profile_dir_refused(&dir, &home) {
            return;
        }
        if let Err(e) = Self::write_profile_lists(&dir, &self.mod_rows, self.mods.as_ref()) {
            self.message = Some((e, true));
            return;
        }
        let (plugins_text, _) = Self::plugin_list_text(self.mods.as_ref());
        let path = backup_dir(&home).join(format!("plugins-{stamp}.txt"));
        if let Err(e) = std::fs::write(&path, plugins_text) {
            self.message = Some((format!("{}: {e}", path.display()), true));
            return;
        }
        prune_plugin_backups(&home, self.backup_max);
        self.backup_pick = Some(stamp.clone());
        self.message = Some((format!("Saved a backup of the mod list and the load order ({stamp})."), false));
    }

    fn load_dated_backup(&mut self) {
        let Some(label) = self.backup_pick.clone() else { return };
        let Some(home) = self.home.clone() else { return };
        let plugins_path = backup_dir(&home).join(format!("plugins-{label}.txt"));
        let plugins_text = match std::fs::read_to_string(&plugins_path) {
            Ok(text) => text,
            Err(e) => {
                self.message = Some((format!("{}: {e}", plugins_path.display()), true));
                return;
            }
        };
        let lists = backup_dir(&home).join(format!("lists-{label}"));
        let mods_text = if lists.join("modlist.txt").is_file() {
            std::fs::read_to_string(lists.join("modlist.txt")).unwrap_or_default()
        } else {
            Self::modlist_text(&self.mod_rows)
        };
        let locks_text = if lists.join("modlist.txt").is_file() {
            std::fs::read_to_string(lists.join(PROFILE_LOCKS)).unwrap_or_default()
        } else {
            Self::locks_text(&self.mod_rows)
        };
        let order_text = if lists.join("loadorder.txt").is_file() {
            std::fs::read_to_string(lists.join("loadorder.txt")).unwrap_or_default()
        } else {
            let parsed = plugins::PluginList::parse(&plugins_text);
            Self::plugin_list_text(Some(&parsed)).1
        };
        let name = Self::profile_folder_name(&format!("backup {label}")).unwrap_or_else(|_| "backup".into());
        let dir = home.root.join("profiles").join(&name);
        if self.profile_dir_refused(&dir, &home) {
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.message = Some((format!("{}: {e}", dir.display()), true));
            return;
        }
        for (file, body) in [("modlist.txt", mods_text), ("plugins.txt", plugins_text), ("loadorder.txt", order_text), (PROFILE_LOCKS, locks_text)] {
            if let Err(e) = std::fs::write(dir.join(file), body) {
                self.message = Some((format!("{}: {e}", dir.display()), true));
                return;
            }
        }
        let parent = home.root.join("profiles");
        self.profile_choices = Self::profile_folders(&parent);
        self.profile_pick = Some(name.clone());
        self.profile_name = name.clone();
        self.load_chosen_profile();
        self.message = Some((format!("Restored {label} into profile {name}. Nothing was linked."), false));
    }

    /// A mod list as on and off folders. Separators are skipped. The test reads a saved profile this way.
    #[allow(dead_code)]
    fn profile_mod_rows(text: &str) -> Vec<(String, bool)> {
        fnv_install::mod_order::read_modlist(text).into_iter().filter(|r| !r.separator).map(|r| (r.name, r.on)).collect()
    }

    fn profile_plugin_names(text: &str) -> Vec<String> {
        let mut names = Vec::new();
        for line in text.lines() {
            let t = line.trim().trim_start_matches('\u{feff}');
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let name = t.strip_prefix('*').unwrap_or(t).trim();
            if name.is_empty() || names.iter().any(|n: &String| n.eq_ignore_ascii_case(name)) {
                continue;
            }
            names.push(name.to_string());
        }
        names
    }

    /// A profile folder name: one plain name, not a path.
    fn profile_folder_name(raw: &str) -> Result<String, String> {
        let name = raw.trim();
        let bad = name.is_empty()
            || name.len() > 80
            || name.starts_with('.')
            || name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'));
        if bad {
            return Err(if name.is_empty() { "Name the profile first.".into() } else { "That profile name cannot be used.".into() });
        }
        Ok(name.to_string())
    }

    /// True when `dir` is a real folder under `profiles_root`, not the root itself.
    fn profile_is_under(dir: &Path, profiles_root: &Path) -> bool {
        match (dir.canonicalize(), profiles_root.canonicalize()) {
            (Ok(dir), Ok(root)) => dir != root && dir.starts_with(&root),
            _ => false,
        }
    }

    /// One folder name. Not a path.
    fn one_name(raw: &str) -> Result<String, String> {
        let name = raw.trim();
        let bad = name.is_empty()
            || name.len() > 120
            || name.starts_with('.')
            || name.ends_with('.')
            || name == ".."
            || name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'));
        if bad {
            Err("That name cannot be used.".into())
        } else {
            Ok(name.to_string())
        }
    }

    /// Copies the four list files (the three lists and the locks). Any other file stays where it was.
    fn copy_profile_lists(from: &Path, to: &Path) -> Result<(), String> {
        if to.exists() {
            return Err("That profile is already there.".into());
        }
        std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
        let mut any = false;
        for file_name in ["modlist.txt", "plugins.txt", "loadorder.txt", PROFILE_LOCKS] {
            let src = from.join(file_name);
            if !src.is_file() {
                continue;
            }
            std::fs::copy(&src, to.join(file_name)).map_err(|e| format!("{}: {e}", src.display()))?;
            any |= file_name != PROFILE_LOCKS;
        }
        if !any {
            let _ = std::fs::remove_dir_all(to);
            return Err("That profile has no lists.".into());
        }
        Ok(())
    }

    /// Removes a profile folder. A link is not followed.
    fn remove_profile_dir(dir: &Path) -> Result<(), String> {
        let meta = std::fs::symlink_metadata(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        if meta.file_type().is_symlink() {
            return Err("That profile is a link. It was not deleted.".into());
        }
        std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
    }

    /// Renames a mod folder inside `mods`. A link outside that folder is left alone.
    fn rename_mod_dir(mods: &Path, from: &str, to: &str) -> Result<(), String> {
        let from = Self::one_name(from)?;
        let to = Self::one_name(to)?;
        if from.eq_ignore_ascii_case(&to) {
            return Err("That is already the name.".into());
        }
        let src = mods.join(&from);
        let dest = mods.join(&to);
        if dest.exists() {
            return Err("That folder is already in the mods folder.".into());
        }
        Self::mod_dir_is_inside(&src, mods)?;
        std::fs::rename(&src, &dest).map_err(|e| format!("{}: {e}", src.display()))
    }

    /// Moves a mod folder into `recycle`. A real file in the game folder is not touched.
    fn move_mod_aside(mods: &Path, recycle: &Path, name: &str) -> Result<PathBuf, String> {
        let name = Self::one_name(name)?;
        let src = mods.join(&name);
        Self::mod_dir_is_inside(&src, mods)?;
        std::fs::create_dir_all(recycle).map_err(|e| format!("{}: {e}", recycle.display()))?;
        let mut dest = recycle.join(&name);
        if dest.exists() {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            dest = recycle.join(format!("{name} {stamp}"));
        }
        if dest.exists() {
            return Err("That folder is already in recycle.".into());
        }
        std::fs::rename(&src, &dest).map_err(|e| format!("{}: {e}", src.display()))?;
        Ok(dest)
    }

    /// The mod folder is a direct child of `mods`. A link that points outside is refused.
    fn mod_dir_is_inside(src: &Path, mods: &Path) -> Result<(), String> {
        let meta = std::fs::symlink_metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
        if meta.file_type().is_symlink() {
            let target = std::fs::canonicalize(src).map_err(|e| format!("{}: {e}", src.display()))?;
            let mods = mods.canonicalize().map_err(|e| format!("{}: {e}", mods.display()))?;
            if target.starts_with(&mods) {
                return Ok(());
            }
            return Err("That folder is a link outside the mods folder. It was not moved.".into());
        }
        let src = src.canonicalize().map_err(|e| format!("{}: {e}", src.display()))?;
        let mods = mods.canonicalize().map_err(|e| format!("{}: {e}", mods.display()))?;
        if src.parent() == Some(mods.as_path()) {
            Ok(())
        } else {
            Err("That folder is not in the mods folder.".into())
        }
    }

    /// Writes the three list files and the lock side file. An existing profile keeps every other file. Returns whether the lists replaced older ones.
    fn write_profile_lists(dir: &Path, rows: &[ModRow], list: Option<&plugins::PluginList>) -> Result<bool, String> {
        let replaced = dir.join("modlist.txt").is_file();
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let modlist = Self::modlist_text(rows);
        let (plugins_text, order_text) = Self::plugin_list_text(list);
        for (file_name, body) in [("modlist.txt", modlist), ("plugins.txt", plugins_text), ("loadorder.txt", order_text), (PROFILE_LOCKS, Self::locks_text(rows))] {
            let path = dir.join(file_name);
            std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(replaced)
    }

    /// The rows in MO2's `modlist.txt` form (`fnv_install::mod_order`).
    fn modlist_text(rows: &[ModRow]) -> String {
        fnv_install::mod_order::modlist_text(rows)
    }

    /// The locked mods, for the side file beside a list.
    fn locks_text(rows: &[ModRow]) -> String {
        fnv_install::mod_order::locks_text(rows)
    }

    fn plugin_list_text(list: Option<&plugins::PluginList>) -> (String, String) {
        let mut active = String::new();
        let mut order = String::new();
        let Some(list) = list else { return (active, order) };
        for entry in &list.entries {
            if entry.name.is_empty() || entry.name.contains('/') || entry.name.contains('\\') {
                continue;
            }
            order.push_str(&entry.name);
            order.push('\n');
            if entry.active {
                active.push('*');
                active.push_str(&entry.name);
                active.push('\n');
            }
        }
        (active, order)
    }

    fn backup_row(&mut self, ui: &mut egui::Ui) {
        let Some(home) = self.home.clone() else { return };
        let files = plugin_backups(&home);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Backups").small().color(WEAK)).on_hover_text("The most load-order copies to keep. Not how many exist now.");
            self.backup_max_picker(ui);
            let shown = self.backup_pick.clone().unwrap_or_else(|| if files.is_empty() { "no backups".into() } else { "choose a backup".into() });
            egui::ComboBox::from_id_salt("plugin-backup-which").selected_text(shown).show_ui(ui, |ui| {
                for path in &files {
                    let label = backup_label(path);
                    if ui.selectable_label(self.backup_pick.as_deref() == Some(label.as_str()), &label).clicked() {
                        self.backup_pick = Some(label);
                    }
                }
            });
            if ui.add_enabled(self.backup_pick.is_some(), egui::Button::new("Load backup")).on_hover_text("Writes a profile from this backup. Nothing is linked into Data.").clicked() {
                self.load_dated_backup();
            }
            let can_backup = self.mods.is_some() || !self.mod_rows.is_empty();
            if ui.add_enabled(can_backup, egui::Button::new("Save backup")).on_hover_text("Keeps a dated copy of the mod list and the load order.").clicked() {
                self.save_dated_backup();
            }
        });
    }

    fn set_backup_max(&mut self, n: u32) {
        let Some(home) = self.home.clone() else { return };
        match write_backup_max(&home, n) {
            Ok(()) => {
                self.backup_max = n;
                self.set_launcher_key("PluginBackups", &n.to_string());
                self.backup_pick = self.backup_pick.take().filter(|pick| plugin_backups(&home).iter().any(|p| backup_label(p) == *pick));
                self.message = Some((format!("At most {n} load-order backups are kept."), false));
            }
            Err(e) => self.message = Some((e, true)),
        }
    }

    /// Lowering the cap asks first. Yes deletes the oldest backups. No leaves the cap alone.
    fn backup_cut_window(&mut self, ctx: &egui::Context) {
        let Some(n) = self.backup_cut else { return };
        let extra = self.home.as_ref().map(|h| plugin_backups(h).len().saturating_sub(n as usize)).unwrap_or(0);
        let mut open = true;
        let mut answer = None;
        egui::Window::new("Fewer backups")
            .collapsible(false)
            .resizable(false)
            .constrain(true)
            .max_size(fit_to_screen(ctx))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(340.0);
                ui.label(format!("Keep at most {n} backups?"));
                ui.label(RichText::new(format!("Delete the {extra} oldest backups? A backup is a copy of the mod list and the load order. The mods in the mods folder stay.")).small().color(WEAK));
                ui.horizontal(|ui| {
                    if ui.button("Delete oldest").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Don't delete").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if let Some(delete) = answer {
            self.answer_backup_cut(delete);
        } else if !open {
            self.backup_cut = None;
            self.message = Some(("The backup cap was left as it is. Nothing was deleted.".into(), false));
        }
    }

    fn answer_backup_cut(&mut self, delete: bool) {
        let Some(n) = self.backup_cut.take() else { return };
        if !delete {
            self.message = Some(("The backup cap was left as it is. Nothing was deleted.".into(), false));
            return;
        }
        let Some(home) = self.home.clone() else { return };
        let extra = plugin_backups(&home).len().saturating_sub(n as usize);
        match write_backup_max(&home, n) {
            Ok(()) => {
                self.backup_max = n;
                self.set_launcher_key("PluginBackups", &n.to_string());
                prune_plugin_backups(&home, n);
                self.backup_pick = self.backup_pick.take().filter(|pick| plugin_backups(&home).iter().any(|p| backup_label(p) == *pick));
                self.message = Some((format!("Deleted {extra} oldest backups. At most {n} are kept. The mods were not removed."), false));
            }
            Err(e) => self.message = Some((e, true)),
        }
    }


    /// The start's Custom… window (#4288): a mod's own start quest and intro film, for a start
    /// that is neither TTW's nor the base game's. OK puts them on the NVForever.ini card, whose
    /// Save writes them (#4313); Cancel, Escape or a click outside leaves the card as it was.
    fn start_custom_window(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.start_custom.as_mut() else { return };
        let mut done = None;
        let modal = egui::Modal::new(egui::Id::unique("start-custom")).frame(Frame::window(&ctx.global_style()).inner_margin(Margin::symmetric(24, 20))).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new("Custom start").size(20.0).strong().color(GOLD));
            ui.label(RichText::new("For a mod whose install guide gives its own start quest or intro film.").small().color(WEAK));
            ui.add_space(8.0);
            egui::Grid::new("start_custom_fields").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Start quest");
                ui.add(egui::TextEdit::singleline(&mut draft.quest).hint_text("FormID or editor ID; 00102037 is New Vegas").desired_width(300.0));
                ui.end_row();
                ui.label("Intro film");
                ui.add_enabled(!draft.no_film, egui::TextEdit::singleline(&mut draft.film).hint_text(r"a .bik file in Data\Video").desired_width(300.0));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut draft.no_film, "No intro film");
                ui.end_row();
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.add(gold_button("OK", true)).clicked() {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
            });
        });
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        match done {
            Some(true) => self.ini.start = self.start_custom.take().unwrap_or_default(),
            Some(false) => self.start_custom = None,
            None => {}
        }
    }

    fn save_mods(&mut self) -> Result<(), String> {
        // The live plugins.txt is written here. A dated backup is only made when the
        // player presses Save backup. Turning a mod on or off does not make one.
        let (Some(home), Some(list)) = (&self.home, &self.mods) else { return Err("no plugin list".into()) };
        let path = home.plugins_file();
        let body = list.to_text();
        guard_write(&path, self.game().as_ref()).map_err(|e| e.to_string())?;
        write_whole(&path, body.as_bytes()).map_err(|e| e.to_string())?;
        self.write_mod_order()
    }

    fn game_folders() -> Vec<(PathBuf, &'static str)> {
        fn add_game(path: &Path, store: &'static str, found: &mut Vec<(PathBuf, &'static str)>, seen: &mut HashSet<PathBuf>) {
            if GameInstall::open(path).is_err() {
                return;
            }
            let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            if seen.insert(key) {
                found.push((path.to_path_buf(), store));
            }
        }
        fn consider(path: &Path, store: &'static str, found: &mut Vec<(PathBuf, &'static str)>, seen: &mut HashSet<PathBuf>) {
            if !path.is_dir() {
                return;
            }
            if GameInstall::open(path).is_ok() {
                add_game(path, store, found, seen);
                return;
            }
            let Ok(rd) = std::fs::read_dir(path) else { return };
            for entry in rd.flatten() {
                let child = entry.path();
                if child.is_dir() {
                    add_game(&child, store, found, seen);
                }
            }
        }
        fn paths_in_json(text: &str) -> Vec<PathBuf> {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
            fn walk(value: &serde_json::Value, out: &mut Vec<PathBuf>) {
                match value {
                    serde_json::Value::Object(map) => {
                        for key in ["install_path", "installPath", "install_dir"] {
                            if let Some(serde_json::Value::String(path)) = map.get(key) {
                                if !path.is_empty() {
                                    out.push(PathBuf::from(path));
                                }
                            }
                        }
                        for child in map.values() {
                            walk(child, out);
                        }
                    }
                    serde_json::Value::Array(items) => {
                        for child in items {
                            walk(child, out);
                        }
                    }
                    _ => {}
                }
            }
            let mut out = Vec::new();
            walk(&value, &mut out);
            out
        }
        let mut found = Vec::new();
        let mut seen = HashSet::new();
        let Some(home_os) = std::env::var_os("HOME") else { return found };
        let home = PathBuf::from(home_os);
        let mut parents: Vec<(PathBuf, &'static str)> = Vec::new();
        for rel in [".steam/steam", ".local/share/Steam", ".var/app/com.valvesoftware.Steam/data/Steam"] {
            let steam = home.join(rel);
            if !steam.is_dir() {
                continue;
            }
            parents.push((steam.join("steamapps/common"), "Steam"));
            if let Ok(text) = std::fs::read_to_string(steam.join("steamapps/libraryfolders.vdf")) {
                for line in text.lines() {
                    let Some(rest) = line.trim().strip_prefix("\"path\"") else { continue };
                    let rest = rest.trim().trim_matches('"');
                    if !rest.is_empty() {
                        parents.push((PathBuf::from(rest).join("steamapps/common"), "Steam"));
                    }
                }
            }
        }
        for rel in ["GOG Games", ".wine/drive_c/GOG Games", ".wine/drive_c/Program Files (x86)/GOG Galaxy/Games"] {
            parents.push((home.join(rel), "GOG"));
        }
        for rel in [".wine/drive_c/Program Files/Epic Games", ".wine/drive_c/Program Files (x86)/Epic Games"] {
            parents.push((home.join(rel), "Epic"));
        }
        for rel in ["Games", "Games/Heroic"] {
            let root = home.join(rel);
            let Ok(rd) = std::fs::read_dir(&root) else { continue };
            for entry in rd.flatten() {
                let child = entry.path();
                if !child.is_dir() {
                    continue;
                }
                let store = match child.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase() {
                    n if n.contains("epic") => "Epic",
                    n if n.contains("gog") => "GOG",
                    n if n.contains("steam") => "Steam",
                    _ => "Heroic",
                };
                parents.push((child, store));
            }
        }
        for (rel, store) in [
            (".config/legendary/installed.json", "Epic"),
            (".config/heroic/legendaryConfig/legendary/installed.json", "Epic"),
            (".var/app/com.heroicgameslauncher.hgl/config/heroic/legendaryConfig/legendary/installed.json", "Epic"),
            (".config/heroic/gog_store/installed.json", "GOG"),
            (".var/app/com.heroicgameslauncher.hgl/config/heroic/gog_store/installed.json", "GOG"),
            (".config/minigalaxy/config.json", "GOG"),
        ] {
            if let Ok(text) = std::fs::read_to_string(home.join(rel)) {
                for folder in paths_in_json(&text) {
                    parents.push((folder, store));
                }
            }
        }
        for (parent, store) in parents {
            consider(&parent, store, &mut found, &mut seen);
        }
        found
    }

    fn game_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Game folder");
        ui.label(RichText::new("Your Fallout: New Vegas folder, the one holding Data. NVForever only reads it.").small().color(WEAK));
        ui.add_space(8.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let reserve = if cfg!(windows) { 190.0 } else { 360.0 };
                ui.add(egui::TextEdit::singleline(&mut self.game_edit).desired_width(ui.available_width() - reserve));
                if cfg!(windows) && ui.button("Browse…").clicked() {
                    if let Some(d) = pick_folder("Your Fallout: New Vegas folder") {
                        self.game_edit = d.display().to_string();
                    }
                }
                if !cfg!(windows) && ui.button("Find game folder").clicked() {
                    match Self::game_folders().as_slice() {
                        [] => self.message = Some(("No Fallout: New Vegas folder in Steam, GOG, or Epic.".into(), true)),
                        [one] => {
                            self.game_edit = one.0.display().to_string();
                            self.message = Some((format!("Filled in {} from {}.", one.0.display(), one.1), false));
                        }
                        many => {
                            let pick = many.iter().find(|p| {
                                p.0.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                                    let n = n.to_ascii_lowercase();
                                    n.contains("vegas") || n.contains("fallout")
                                })
                            }).unwrap_or(&many[0]);
                            self.game_edit = pick.0.display().to_string();
                            self.message = Some((format!("Found {}. Filled in {} from {}.", many.len(), pick.0.display(), pick.1), false));
                        }
                    }
                }
                if ui.button(RichText::new("Save").strong()).clicked() {
                    self.save_game_folder();
                }
            });
            match GameInstall::open(self.game_edit.trim()) {
                Ok(g) => {
                    let missing = g.missing_files();
                    if missing.is_empty() {
                        ui.label(RichText::new("Every game file NVForever needs is there.").color(GOOD));
                    } else {
                        ui.label(RichText::new(format!("Missing from Data: {}", missing.join(", "))).color(BAD));
                    }
                    let addons = g.addon_masters();
                    ui.label(RichText::new(format!("Add-ons found: {}", if addons.is_empty() { "none".to_string() } else { addons.join(", ") })).small().color(WEAK));
                }
                Err(e) if !self.game_edit.trim().is_empty() => {
                    ui.label(RichText::new(e.to_string()).color(BAD));
                }
                Err(_) => {}
            }
        });
        ui.add_space(12.0);
        ui.label(RichText::new("PROGRAM").small().strong().color(theme::GOLD_TEXT));
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            match &self.exe {
                Some(p) => ui.label(format!("Starts {}", p.display())),
                None => ui.label(RichText::new(format!("{} was not found beside the launcher.", launch::GAME_EXE)).color(BAD)),
            };
            if cfg!(windows) && ui.button("Choose the game program…").clicked() {
                if let Some(f) = pick_file("nvforever") {
                    self.set_launcher_key("Exe", &f.display().to_string());
                }
            }
        });
        ui.add_space(12.0);
        ui.label(RichText::new("NVFOREVER.INI").small().strong().color(theme::GOLD_TEXT));
        self.ini_card(ui);
        ui.add_space(12.0);
        ui.label(RichText::new("VERSION").small().strong().color(theme::GOLD_TEXT));
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(format!("NVForever v{}", version()));
            let install = install_dir(self.game().as_ref());
            let kept = install.as_ref().ok().and_then(|d| update::previous_version(d));
            match (&install, kept) {
                (Ok(dir), Some((v, _))) => {
                    let busy = !matches!(self.upd, Upd::Idle | Upd::Failed(_));
                    let b = ui.add_enabled(!busy, egui::Button::new(format!("Roll back to v{v}")));
                    if b.on_hover_text("Put the version the last update replaced back (this one is kept in its place). Saves, settings and mods are not touched.").clicked() {
                        self.roll_back(dir.clone(), ui.ctx());
                    }
                }
                (Ok(_), None) => {
                    ui.label(RichText::new("No earlier version is kept: an update keeps the one it replaces, to roll back to.").small().color(WEAK));
                }
                (Err(e), _) => {
                    ui.label(RichText::new(format!("Updates: {e}")).small().color(WEAK));
                }
            }
        });
        ui.add_space(12.0);
        ui.label(RichText::new("FOLDERS").small().strong().color(theme::GOLD_TEXT));
        if let Some(h) = self.home.clone() {
            ui.horizontal_wrapped(|ui| {
                for (name, dir) in [("Saves", h.saves()), ("Logs", h.logs()), ("Screenshots", h.screenshots()), ("Settings", h.root.clone())] {
                    if ui.button(name).clicked() {
                        let _ = std::fs::create_dir_all(&dir);
                        launch::open_folder(&dir);
                    }
                }
            });
        }
    }

    fn save_game_folder(&mut self) {
        let dir = PathBuf::from(self.game_edit.trim());
        let r = match GameInstall::open(&dir) {
            Ok(_) => self.write_ini(|t| {
                Config {
                    game_dir: Some(dir.clone()),
                    ..Config::default()
                }
                .to_ini(&t)
            }),
            Err(e) => Err(e.to_string()),
        };
        // The NVForever.ini card's changes not saved yet stay, as its Save keeps this box's.
        let pending = self.ini.clone();
        let ok = r.is_ok();
        self.after_write(r, Some("Game folder saved."));
        if ok {
            self.ini = pending;
        }
    }
}

/// The folder the launcher runs from, when the updater may change it ([`update::check_install`]).
fn install_dir(game: Option<&GameInstall>) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("the launcher's folder is unknown")?.to_path_buf();
    update::check_install(&dir, game)?;
    Ok(dir)
}

fn megabytes(b: u64) -> String {
    format!("{:.1} MB", b as f64 / 1_048_576.0)
}

// ---- The updater ----

impl App {
    /// Download, check and unpack the newest release in the background.
    fn start_update(&mut self, ctx: &egui::Context) {
        let (Some(latest), Some(home)) = (self.latest.clone(), self.home.clone()) else { return };
        let game = self.game();
        let (tx, rx) = channel();
        self.upd_cancel = Arc::new(AtomicBool::new(false));
        let (cancel, c) = (self.upd_cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let send = |m: Msg| {
                let _ = tx.send(m);
                c.request_repaint();
            };
            let r = update::prepare(&latest, &home.cache(), game.as_ref(), &send, &cancel);
            send(Msg::Ready(r));
        });
        self.upd_rx = Some(rx);
        self.upd = Upd::Working { phase: Phase::Checking, done: 0, total: 0 };
    }

    fn start_install(&mut self, ctx: &egui::Context, ready: update::Ready) {
        let game = self.game();
        let dir = match install_dir(game.as_ref()) {
            Ok(d) => d,
            Err(e) => {
                self.upd = Upd::Failed(e);
                return;
            }
        };
        let (tx, rx) = channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Installed(update::install(&dir, &ready, version(), game.as_ref())));
            c.request_repaint();
        });
        self.upd_rx = Some(rx);
        self.upd = Upd::Installing;
    }

    fn roll_back(&mut self, dir: PathBuf, ctx: &egui::Context) {
        if self.running() {
            self.message = Some(("Close the game first, then roll back.".into(), true));
            return;
        }
        match update::roll_back(&dir, version(), self.game().as_ref()) {
            Ok(v) => match update::restart(&dir, "--rolled-back") {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => self.message = Some((format!("Rolled back to v{v}; start the launcher again ({e})."), false)),
            },
            Err(e) => self.message = Some((format!("Roll back failed, nothing was changed: {e}"), true)),
        }
    }

    fn poll_update(&mut self, ctx: &egui::Context, running: bool) {
        while let Some(m) = self.upd_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match m {
                Msg::Phase(p) => {
                    if let Upd::Working { phase, .. } = &mut self.upd {
                        *phase = p;
                    }
                }
                Msg::Bytes(n, t) => {
                    if let Upd::Working { done, total, .. } = &mut self.upd {
                        (*done, *total) = (n, t);
                    }
                }
                Msg::Ready(Ok(r)) => self.upd = Upd::Ready(r),
                Msg::Ready(Err(e)) => self.upd = Upd::Failed(e),
                Msg::Installed(Ok(v)) => {
                    self.upd_rx = None;
                    let restarted = install_dir(self.game().as_ref()).and_then(|d| update::restart(&d, "--updated").map_err(|e| e.to_string()));
                    self.upd = Upd::installed(&restarted);
                    match restarted {
                        Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => {
                            self.latest = None;
                            self.message = Some((format!("Updated to v{v}; start the launcher again ({e})."), false));
                        }
                    }
                    return;
                }
                Msg::Installed(Err(e)) => self.upd = Upd::Failed(e),
            }
        }
        if matches!(self.upd, Upd::Ready(_)) && !running {
            if let Upd::Ready(r) = std::mem::replace(&mut self.upd, Upd::Idle) {
                self.start_install(ctx, r);
            }
        }
    }

    /// The update dialog: what is new, Update now / Later, then the progress.
    fn update_window(&mut self, ctx: &egui::Context, running: bool) {
        if !self.upd_open {
            return;
        }
        let Some(l) = self.latest.clone() else { return };
        let size = update::target().and_then(|t| update::choose_asset(&l.assets, &l.tag, t)).map(|(a, _)| a.size);
        let mut open = true;
        // A set width (most of the launcher's, at most 760): a window left to size itself
        // shrinks to its text and wraps the notes into a narrow column.
        let width = (ctx.content_rect().width() - 160.0).clamp(320.0, 760.0);
        let notes_h = (ctx.viewport_rect().height() * 0.4).clamp(80.0, 320.0);
        egui::Window::new(format!("NVForever v{}", l.version))
            .collapsible(false)
            .resizable(false)
            .constrain(true)
            .min_width(width)
            .max_width(width)
            .max_height((ctx.viewport_rect().height() - 48.0).max(200.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            // Room around the text and between the rows: the default window margin is tight.
            .frame(Frame::window(&ctx.global_style()).inner_margin(Margin::symmetric(24, 20)))
            // Our own heading and close button: egui's title bar does not reach across the
            // wider margin.
            .title_bar(false)
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("NVForever v{}", l.version)).size(22.0).strong().color(GOLD));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("×").size(24.0).color(WEAK)).frame(false)).on_hover_text("Close").clicked() {
                            open = false;
                        }
                    });
                });
                ui.separator();
                ui.add_space(4.0);
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
                ui.spacing_mut().button_padding = Vec2::new(14.0, 6.0);
                ui.label(format!("v{} is out; this is v{}.{}", l.version, version(), size.map_or(String::new(), |s| format!(" Download: {}.", megabytes(s)))));
                if !l.notes.trim().is_empty() {
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(notes_h).auto_shrink([false, true]).show(ui, |ui| {
                        CommonMarkViewer::new().show(ui, &mut self.md, &l.notes);
                    });
                }
                ui.add_space(14.0);
                let install = install_dir(self.game().as_ref());
                match &self.upd {
                    Upd::Idle => {
                        let note = match &install {
                            Ok(_) => "Your saves, settings and mods are not touched. The version you have now is kept, to roll back to from the Game folder page.".to_string(),
                            Err(e) => e.clone(),
                        };
                        ui.label(RichText::new(note).small().color(WEAK));
                        ui.horizontal(|ui| {
                            if install.is_ok() && ui.button(RichText::new("Update now").strong()).clicked() {
                                self.start_update(ui.ctx());
                            }
                            if ui.button("Later").clicked() {
                                self.upd_open = false;
                            }
                            if ui.button("Release page").clicked() {
                                launch::open_url(&l.url);
                            }
                        });
                    }
                    Upd::Working { phase, done, total } => {
                        let (phase, done, total) = (*phase, *done, *total);
                        let bar = if phase == Phase::Downloading && total > 0 {
                            egui::ProgressBar::new(done as f32 / total as f32).text(format!("{} {} of {}", phase.words(), megabytes(done), megabytes(total)))
                        } else {
                            egui::ProgressBar::new(0.0).animate(true).text(phase.words())
                        };
                        ui.add(bar);
                        if phase == Phase::Downloading && ui.button("Cancel").clicked() {
                            self.upd_cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    Upd::Ready(_) => {
                        ui.label(RichText::new(if running { "Downloaded. It installs as soon as the game is closed." } else { "Downloaded." }).color(GOOD));
                    }
                    Upd::Installing | Upd::Closing => {
                        ui.add(egui::ProgressBar::new(1.0).animate(true).text(Phase::Installing.words()));
                    }
                    Upd::Failed(e) => {
                        ui.label(RichText::new(format!("The update did not go in: {e}")).color(BAD));
                        ui.label(RichText::new("Try again, or download it from the release page.").small().color(WEAK));
                        ui.horizontal(|ui| {
                            if install.is_ok() && ui.button("Try again").clicked() {
                                self.start_update(ui.ctx());
                            }
                            if ui.button("Release page").clicked() {
                                launch::open_url(&l.url);
                            }
                        });
                    }
                }
            });
        // Closing the window while it works lets the download carry on; the Update button
        // shows it again.
        if !open {
            self.upd_open = false;
            if matches!(self.upd, Upd::Failed(_)) {
                self.upd = Upd::Idle;
            }
        }
    }
}

#[cfg(windows)]
fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

#[cfg(windows)]
fn pick_file(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_file()
}

/// No picker off Windows (Cargo.toml): the path is typed.
#[cfg(not(windows))]
fn pick_folder(_: &str) -> Option<PathBuf> {
    None
}

#[cfg(not(windows))]
fn pick_file(_: &str) -> Option<PathBuf> {
    None
}

/// A save's place as the Continue button shows it: the cell's name when the save holds one
/// ("The Tops Presidential Suite"), else nothing. The game writes the cell's FULL name, else
/// its Editor ID (`StripFSGate`), else its worldspace and grid (`WastelandNV (-15, -13)`)
/// (`place_name` in engine/nvf-play/src/console.rs); the launcher has no data files to look
/// the last two up in, and an Editor ID is no name to show a player.
fn shown_place(location: &str) -> Option<&str> {
    let l = location.trim();
    let grid = l.ends_with(')') && l.rfind(" (").is_some_and(|i| l[i + 2..l.len() - 1].split(',').all(|n| n.trim().parse::<i32>().is_ok()));
    let editor_id = !l.contains(' ');
    (!l.is_empty() && !grid && !editor_id).then_some(l)
}

/// The gap a dragged row would drop into: the number of rows whose middle is above `y`
/// (0 = before the first row, `centers.len()` = after the last).
fn header_ok(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let mut head = [0u8; 4];
    std::io::Read::read_exact(&mut file, &mut head).is_ok() && &head == b"TES4"
}

/// Moves one row. A locked row stays at its index.
fn reorder_mods(rows: &[ModRow], from: usize, to: usize) -> Vec<ModRow> {
    if from >= rows.len() || to >= rows.len() || rows[from].locked {
        return rows.to_vec();
    }
    let mut moved = rows.to_vec();
    let row = moved.remove(from);
    moved.insert(to, row);
    let unlocked: Vec<ModRow> = moved.into_iter().filter(|r| !r.locked).collect();
    let mut unlocked = unlocked.into_iter();
    rows.iter().map(|row| if row.locked { row.clone() } else { unlocked.next().unwrap_or_else(|| row.clone()) }).collect()
}

fn insertion_index(centers: &[f32], y: f32) -> usize {
    centers.iter().filter(|c| **c < y).count()
}

/// The index row `from` ends at when dropped into gap `gap` (taken out first, so a gap below
/// it is one less).
fn move_target(from: usize, gap: usize) -> usize {
    if gap > from {
        gap - 1
    } else {
        gap
    }
}

/// `body` cut after the paragraph that passes [`PREVIEW_CHARS`], unless `open`; whether it
/// was long enough to cut.
fn preview(body: &str, open: bool) -> (&str, bool) {
    if body.len() <= PREVIEW_CHARS {
        return (body, false);
    }
    // A byte count can fall inside a character (`·`, `—`); start at the next whole one.
    let from = (PREVIEW_CHARS..=body.len()).find(|&i| body.is_char_boundary(i)).unwrap_or(body.len());
    let at = body[from..].find("\n\n").map(|i| i + from);
    match at {
        Some(at) if !open => (&body[..at], true),
        Some(_) => (body, true),
        None => (body, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Azza's review of #3723 item 10: Play Co-Op starts the plain game, as Play's main
    /// menu start does, with no co-op program and no co-op arguments.
    #[test]
    fn play_coop_starts_the_plain_game_at_its_main_menu() {
        assert_eq!(COOP_START.args(), Start::MainMenu.args());
        assert!(COOP_START.args().iter().all(|a| !a.contains("coop") && a != "--host" && a != "--join"));
        assert!(launch::GAME_EXE == "nvforever.exe" || launch::GAME_EXE == "nvforever");
    }

    /// The page names the game's own menu entries, so a renamed entry shows up here.
    #[test]
    fn coop_page_names_the_games_menu_entries() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../engine/nvf-play/src/");
        let lobby = std::fs::read_to_string(format!("{root}lobby.rs")).unwrap();
        let menu = std::fs::read_to_string(format!("{root}main_menu.rs")).unwrap();
        for name in ["Create Lobby", "Join Lobby"] {
            assert!(COOP_ABOUT.contains(name) && (lobby.contains(&format!("\"{name}\"")) || menu.contains(&format!("\"{name}\""))), "{name}");
        }
        assert!(lobby.contains("const TITLE: &str = \"Co-op\";") && lobby.contains("key: COOP_KEY") && lobby.contains("label: \"Co-op\"") && lobby.contains("pub const COOP_KEY: &str = \"coop\";"));
        let pause = std::fs::read_to_string(format!("{root}pause.rs")).unwrap();
        assert!(lobby.contains("group: crate::pause::GROUP_MENUS"));
        assert!(pause.contains("pub const GROUP_MENUS: &str = \"Gameplay\";") && COOP_ABOUT.contains("Gameplay") && COOP_ABOUT.contains("Settings"));
    }

    /// #3117: the close guard holds the window only during the swap; once the new launcher
    /// was started the old one's own close goes through.
    #[test]
    fn the_window_closes_once_the_new_launcher_started() {
        assert!(Upd::Installing.refuses_close());
        let after = Upd::installed(&Ok(()));
        assert!(matches!(after, Upd::Closing));
        assert!(!after.refuses_close());
        // The new launcher did not start: the window stays, usable and closable by hand.
        let after = Upd::installed(&Err("no such file".into()));
        assert!(matches!(after, Upd::Idle));
        assert!(!after.refuses_close());
        for u in [Upd::Idle, Upd::Working { phase: Phase::Checking, done: 0, total: 0 }, Upd::Failed(String::new())] {
            assert!(!u.refuses_close());
        }
    }

    /// #3483: the Game folder page's NVForever.ini card writes only the keys it changed, keeps
    /// every other line (`[Game] Path` among them), and checks only a changed program, so a
    /// saved one that went missing does not stop the other keys being saved.
    #[test]
    fn the_ini_card_writes_only_what_changed() {
        let text = "; mine\n[Game]\nPath=F:\\FNV\n\n[Launcher]\nExe=D:\\gone\\nvforever.exe\n\n[Settings]\nhud-colour=Amber\n";
        let was = IniForm::read(text);
        assert_eq!(was, IniForm { start: crate::start::Fields::original(), exe: "D:\\gone\\nvforever.exe".into(), news: true, close: false });
        let no = |_: &str| false;
        let yes = |_: &str| true;
        // Nothing changed: nothing checked, the text as it was.
        assert!(was.check(&was, no).is_ok());
        assert_eq!(was.apply(&was, text), text);
        // CloseOnPlay alone, though the saved program is not there now.
        let close = IniForm { close: true, ..was.clone() };
        assert!(close.check(&was, no).is_ok());
        assert_eq!(
            close.apply(&was, text),
            "; mine\n[Game]\nPath=F:\\FNV\n\n[Launcher]\nExe=D:\\gone\\nvforever.exe\nCloseOnPlay=1\n\n[Settings]\nhud-colour=Amber\n"
        );
        // A changed program must be a file; an empty one clears the key.
        let exe = IniForm { exe: " C:\\NVF\\nvforever.exe ".into(), ..was.clone() };
        assert!(exe.check(&was, no).is_err());
        assert!(exe.check(&was, yes).is_ok());
        assert!(exe.apply(&was, text).contains("\nExe=C:\\NVF\\nvforever.exe\n"));
        let cleared = IniForm { exe: String::new(), ..was.clone() };
        assert!(cleared.check(&was, no).is_ok());
        assert!(cleared.apply(&was, text).contains("\nExe=\n"));
        // A file without a [Launcher] section gets one at the end.
        let bare = "[Game]\nPath=F:\\FNV\n";
        let was = IniForm::read(bare);
        assert_eq!(IniForm { news: false, ..was.clone() }.apply(&was, bare), "[Game]\nPath=F:\\FNV\n\n[Launcher]\nNews=0\n");
    }

    /// #4313: the card's Save writes the new game's start only when it changed, as its other
    /// keys, and keeps every other line; switching TTW on and back off before Save writes nothing.
    #[test]
    fn the_ini_card_writes_the_start_only_when_it_changed() {
        use crate::start::Fields;
        let text = "[General]\nsCharGenQuest=001FFFF8\nsIntroMovie=\n\n[Launcher]\nNews=0\n\n[Settings]\nhud-colour=Amber\n";
        let was = IniForm::read(text);
        assert!(was.start.is_ttw());
        // TTW off: the base game's two lines, nothing else changed.
        let off = IniForm { start: Fields::original(), ..was.clone() };
        assert!(off.check(&was, |_| false).is_ok());
        assert_eq!(off.apply(&was, text), "[General]\nsCharGenQuest=00102037\nsIntroMovie=FNVIntro.bik\n\n[Launcher]\nNews=0\n\n[Settings]\nhud-colour=Amber\n");
        assert_eq!(IniForm::read(&off.apply(&was, text)), off);
        // Another mod's start, from the Custom… window.
        let custom = IniForm { start: Fields { quest: "MyStart".into(), film: "Mine.bik".into(), no_film: false }, ..was.clone() };
        let out = custom.apply(&was, text);
        assert_eq!(ini_get(&out, "General", "sCharGenQuest").as_deref(), Some("MyStart"));
        assert_eq!(ini_get(&out, "General", "sIntroMovie").as_deref(), Some("Mine.bik"));
        // News alone leaves the start's lines as they are.
        let news = IniForm { news: true, ..was.clone() };
        assert_eq!(news.apply(&was, text), "[General]\nsCharGenQuest=001FFFF8\nsIntroMovie=\n\n[Launcher]\nNews=1\n\n[Settings]\nhud-colour=Amber\n");
        // On and back off before Save: the form is as read, nothing to write.
        let back = IniForm { start: Fields::ttw(), ..was.clone() };
        assert_eq!(back, was);
        assert_eq!(back.apply(&was, text), text);
    }

    #[test]
    fn parts_as_the_ini_keeps_them() {
        assert_eq!(part_text(0.5), "0.50");
        assert_eq!(part_text(1.0 / 3.0), "0.333");
        assert_eq!(part_text(1.2), "1.00");
        assert_eq!(part_text(0.25), "0.25");
    }

    /// The launcher's controls drawn with no window (egui headless), the pointer driven by
    /// events as a player's mouse would: what each frame's control returned and where its
    /// words were drawn.
    struct Headless {
        ctx: egui::Context,
        time: f64,
    }

    type Words = Vec<(String, egui::Pos2)>;

    impl Headless {
        fn new() -> Headless {
            Headless { ctx: egui::Context::default(), time: 0.0 }
        }

        /// One frame with `events`: the control's answer and the frame's words with their middles.
        fn frame(&mut self, events: Vec<egui::Event>, kind: &menu::Kind, v: &str) -> (Option<String>, Words) {
            self.time += 0.1;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0))),
                time: Some(self.time),
                focused: true,
                events,
                ..Default::default()
            };
            let mut got = None;
            let out = self.ctx.run_ui(input, |ui| got = menu_control(ui, "row", kind, v, 260.0));
            let mut words = Vec::new();
            for c in &out.shapes {
                collect_words(&c.shape, &mut words);
            }
            out.drop_without_applying_deltas();
            (got, words)
        }

        /// Frames with no input until the layout settles; every answer they gave.
        fn idle(&mut self, kind: &menu::Kind, v: &str) -> Vec<String> {
            (0..3).filter_map(|_| self.frame(Vec::new(), kind, v).0).collect()
        }

        /// A press at `from`, a move to `to`, a release: every answer on the way.
        fn drag(&mut self, kind: &menu::Kind, v: &str, from: egui::Pos2, to: egui::Pos2) -> Vec<String> {
            let press = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            let steps = [
                vec![egui::Event::PointerMoved(from)],
                vec![press(from, true)],
                vec![egui::Event::PointerMoved(from.lerp(to, 0.5))],
                vec![egui::Event::PointerMoved(to)],
                vec![press(to, false)],
                vec![],
            ];
            steps.into_iter().filter_map(|e| self.frame(e, kind, v).0).collect()
        }

        fn click(&mut self, kind: &menu::Kind, v: &str, at: egui::Pos2) -> Vec<String> {
            self.drag(kind, v, at, at)
        }

        /// The middle of the words `w` as drawn now.
        fn find(&mut self, kind: &menu::Kind, v: &str, w: &str) -> egui::Pos2 {
            let (_, words) = self.frame(Vec::new(), kind, v);
            words.iter().filter(|(t, _)| t == w).map(|(_, p)| *p).max_by(|a, b| a.y.total_cmp(&b.y)).unwrap_or_else(|| panic!("{w:?} not drawn: {words:?}"))
        }
    }

    fn collect_words(s: &egui::Shape, out: &mut Words) {
        match s {
            egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos + t.galley.rect.center().to_vec2())),
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect_words(s, out)),
            _ => {}
        }
    }

    fn slider(min: f32, step: f32, words: &[&str]) -> menu::Kind {
        let steps = words.iter().enumerate().map(|(i, w)| menu::Step { value: min + i as f32 * step, label: w.to_string() }).collect();
        menu::Kind::Slider { min, step, steps }
    }

    fn kill_camera() -> menu::Kind {
        let c = |name: &str, label: &str| menu::Choice { name: name.into(), label: label.into() };
        menu::Kind::List { choices: vec![c("Last enemy", "Last Enemy"), c("Every kill", "Every Kill"), c("Off", "Off")] }
    }

    /// Opening a tab changes nothing: no row's control answers without the player.
    #[test]
    fn a_drawn_row_changes_nothing() {
        let cases = [
            (menu::Kind::Switch, "1"),
            (menu::Kind::Switch, "0"),
            // Off the slider's own grid but on the game's (steps from 0): the bug of ee6450871.
            (menu::Kind::Slider { min: 0.1, step: 0.05, steps: vec![] }, "0.35"),
            (menu::Kind::Slider { min: 0.05, step: 1.0 / 30.0, steps: vec![] }, "0.333"),
            // Out of the range (an older build's value): kept, not clamped by drawing it.
            (menu::Kind::Slider { min: 0.2, step: 0.1, steps: vec![] }, "0.10"),
            (menu::Kind::Slider { min: 0.0, step: 0.1, steps: vec![] }, "1.50"),
            (slider(0.0, 0.25, &["Very Easy", "Easy", "Normal", "Hard", "Very Hard"]), "0.75"),
            (kill_camera(), "Last enemy"),
        ];
        for (kind, v) in cases {
            assert_eq!(Headless::new().idle(&kind, v), Vec::<String>::new(), "{kind:?} at {v}");
        }
    }

    #[test]
    fn a_slider_dragged_to_its_ends_is_the_games_first_and_last_value() {
        let kind = menu::Kind::Slider { min: 0.1, step: 0.05, steps: vec![] };
        let rail = egui::Pos2::new(20.0, 10.0);
        let mut h = Headless::new();
        h.idle(&kind, "0.35");
        assert_eq!(h.drag(&kind, "0.35", rail, egui::Pos2::new(2000.0, 10.0)).last().map(String::as_str), Some("1.00"));
        let mut h = Headless::new();
        h.idle(&kind, "0.35");
        assert_eq!(h.drag(&kind, "0.35", rail, egui::Pos2::new(-500.0, 10.0)).last().map(String::as_str), Some("0.10"), "its start, not 0");
        // Moved part of the way: onto a step of the game's (from 0), to the thousandth.
        let mut h = Headless::new();
        h.idle(&kind, "0.35");
        let got = h.drag(&kind, "0.35", rail, egui::Pos2::new(63.0, 10.0));
        let last = got.last().expect("moved");
        let v: f32 = last.parse().unwrap();
        assert!((0.1..=1.0).contains(&v) && ((v / 0.05).round() * 0.05 - v).abs() < 1e-4, "{last} is not on a step of 0.05");
    }

    #[test]
    fn a_slider_shows_the_games_words_for_its_steps() {
        let kind = slider(0.0, 0.25, &["Very Easy", "Easy", "Normal", "Hard", "Very Hard"]);
        let mut h = Headless::new();
        h.idle(&kind, "0.75");
        h.find(&kind, "0.75", "Hard");
        let got = h.drag(&kind, "0.75", egui::Pos2::new(20.0, 10.0), egui::Pos2::new(-500.0, 10.0));
        assert_eq!(got.last().map(String::as_str), Some("0.00"));
        h.find(&kind, "0.00", "Very Easy");
    }

    #[test]
    fn a_list_shows_the_games_words_and_keeps_the_choices_name() {
        let kind = kill_camera();
        let mut h = Headless::new();
        h.idle(&kind, "Last enemy");
        let shown = h.find(&kind, "Last enemy", "Last Enemy");
        assert!(h.click(&kind, "Last enemy", shown).is_empty(), "opening the list changes nothing");
        let every = h.find(&kind, "Last enemy", "Every Kill");
        assert_eq!(h.click(&kind, "Last enemy", every), ["Every kill"], "the INI keeps the name, not the words");
        // The chosen one picked again: no change.
        let mut h = Headless::new();
        h.idle(&kind, "Last enemy");
        let shown = h.find(&kind, "Last enemy", "Last Enemy");
        h.click(&kind, "Last enemy", shown);
        let item = h.find(&kind, "Last enemy", "Last Enemy");
        assert!(item.y > shown.y, "the open list's own line");
        assert!(h.click(&kind, "Last enemy", item).is_empty(), "the chosen one picked again");
    }

    #[test]
    fn a_switch_clicked_flips() {
        for (from, to) in [("0", "1"), ("1", "0")] {
            let mut h = Headless::new();
            h.idle(&menu::Kind::Switch, from);
            assert_eq!(h.click(&menu::Kind::Switch, from, egui::Pos2::new(20.0, 10.0)), [to]);
        }
    }

    /// A menu file the game did not write as it should (a hand edit, a broken write) draws
    /// and answers without a panic, and a moved slider stays a value the game reads.
    #[test]
    fn odd_rows_draw_and_answer_without_a_panic() {
        let c = |n: &str| menu::Choice { name: n.into(), label: n.into() };
        let kinds = [
            menu::Kind::Slider { min: 2.0, step: 0.1, steps: vec![] },
            menu::Kind::Slider { min: -1.0, step: -0.5, steps: vec![] },
            menu::Kind::Slider { min: 0.0, step: 0.0, steps: vec![] },
            menu::Kind::Slider { min: 0.0, step: 0.5, steps: vec![menu::Step { value: 0.0, label: String::new() }] },
            menu::Kind::List { choices: vec![] },
            menu::Kind::List { choices: vec![c("A"), c("A"), c("")] },
        ];
        for kind in &kinds {
            for v in ["0.5", "abc", "", "A", "1e9", "-3"] {
                let mut h = Headless::new();
                h.idle(kind, v);
                let got = h.drag(kind, v, egui::Pos2::new(20.0, 10.0), egui::Pos2::new(2000.0, 10.0));
                if let (menu::Kind::Slider { .. }, Some(n)) = (kind, got.last()) {
                    let x: f32 = n.parse().unwrap_or_else(|_| panic!("{kind:?} at {v}: {n} is not a number"));
                    assert!(x.is_finite() && (0.0..=1.0).contains(&x), "{kind:?} at {v}: {n} out of 0..1");
                }
            }
        }
    }

    #[test]
    fn the_games_rules_hold_as_the_game_has_them() {
        let rule = |parent: &str, choices: &[&str]| menu::Rule { parent: parent.into(), choices: choices.iter().map(|c| c.to_string()).collect(), why: String::new() };
        let c = |n: &str| menu::Choice { name: n.into(), label: n.into() };
        let shadows = menu::Kind::List { choices: vec![c("Off"), c("Performance"), c("Quality")] };
        // A row it needs on: a switch on, a list past its first choice.
        assert!(rule_holds(&rule("glow", &[]), Some(&menu::Kind::Switch), "1", "1"));
        assert!(!rule_holds(&rule("glow", &[]), Some(&menu::Kind::Switch), "0", "0"));
        assert!(!rule_holds(&rule("sun-shadows", &[]), Some(&shadows), "Off", "Off"));
        assert!(rule_holds(&rule("sun-shadows", &[]), Some(&shadows), "Quality", "Quality"));
        // Built in (no kind): by the value.
        assert!(!rule_holds(&rule("sun-shadows", &[]), None, "Off", "Off"));
        assert!(!rule_holds(&rule("glow", &[]), None, "", ""));
        assert!(rule_holds(&rule("glow", &[]), None, "1", "1"));
        // On one of its choices, by name (the frame generation's `dlss` named DLSS).
        assert!(rule_holds(&rule("frame-generation", &["DLSS"]), None, "dlss", "DLSS"));
        assert!(!rule_holds(&rule("upscaler", &["FSR 3.1", "DLSS"]), None, "Off", "Off"));
        assert!(rule_holds(&rule(nvf_settings_layout::DEVICE_ROW, &["Controller"]), None, "controller", "controller"));
        // Typed into a slider of words: the word's part, else a number.
        let words: Vec<String> = ["Low", "Medium", "High"].map(String::from).to_vec();
        assert_eq!((word_part(&words, " high "), word_part(&words, "medium"), word_part(&words, "0.25")), (Some(1.0), Some(0.5), Some(0.25)));
        assert_eq!((word_part(&words, "Ultra"), word_part(&["Only".to_string()], "only")), (None, Some(0.0)));
    }

    #[test]
    fn a_moved_slider_is_fitted_as_the_game_fits_it() {
        // Steps from 0 as the game counts them, not from the slider's start.
        assert_eq!(part_text(game_fit(0.52, 0.1, 0.05)), "0.50");
        assert_eq!(part_text(game_fit(0.01, 0.1, 0.05)), "0.10", "not below the start");
        assert_eq!(part_text(game_fit(0.34, 0.0, 1.0 / 30.0)), "0.333");
        assert_eq!(part_text(game_fit(2.0, 0.0, 0.25)), "1.00");
    }

    #[test]
    fn only_real_place_names_are_shown() {
        assert_eq!(shown_place("The Tops Presidential Suite"), Some("The Tops Presidential Suite"));
        assert_eq!(shown_place("Goodsprings"), None);
        assert_eq!(shown_place("StripFSGate"), None);
        assert_eq!(shown_place("WastelandNV (-15, -13)"), None);
        assert_eq!(shown_place(""), None);
        assert_eq!(shown_place("Prospector Saloon"), Some("Prospector Saloon"));
    }

    #[test]
    fn links_are_taken_from_egui() {
        let ctx = egui::Context::default();
        let mut taken = Vec::new();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.ctx().open_url(egui::OpenUrl::new_tab("https://nv-forever.com"));
            ui.ctx().copy_text("kept".into());
            taken = take_links(ui.ctx());
        });
        assert_eq!(taken, ["https://nv-forever.com"]);
        assert!(!out.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(_))));
        assert!(out.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(_))));
        // The test never draws, so the font atlas is unused. egui panics if it is dropped dirty.
        out.textures_delta.clear();
    }

    #[test]
    fn a_saved_profile_reads_back_in_page_order() {
        let dir = std::env::temp_dir().join(format!("nvforever-profile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let rows = vec![
            super::ModRow::module("A", false),
            super::ModRow::module("B", true),
            super::ModRow::module("C", true),
        ];
        let list = fnv_install::plugins::PluginList {
            entries: vec![
                fnv_install::plugins::PluginEntry { name: "FalloutNV.esm".into(), active: true },
                fnv_install::plugins::PluginEntry { name: "Off.esp".into(), active: false },
                fnv_install::plugins::PluginEntry { name: "On.esp".into(), active: true },
            ],
        };
        let saved = dir.join("Road");
        super::App::write_profile_lists(&saved, &rows, Some(&list)).unwrap();
        std::fs::write(saved.join("notes.txt"), "keep").unwrap();
        let replaced = super::App::write_profile_lists(&saved, &rows, Some(&list)).unwrap();
        assert!(replaced);
        assert_eq!(std::fs::read_to_string(saved.join("notes.txt")).unwrap(), "keep");
        let mods = std::fs::read_to_string(saved.join("modlist.txt")).unwrap();
        let plugins_text = std::fs::read_to_string(saved.join("plugins.txt")).unwrap();
        let order = std::fs::read_to_string(saved.join("loadorder.txt")).unwrap();
        assert_eq!(
            super::App::profile_mod_rows(&mods),
            vec![("A".into(), false), ("B".into(), true), ("C".into(), true)]
        );
        assert_eq!(super::App::profile_plugin_names(&order), vec!["FalloutNV.esm", "Off.esp", "On.esp"]);
        assert_eq!(super::App::profile_plugin_names(&plugins_text), vec!["FalloutNV.esm", "On.esp"]);
        assert!(mods.lines().next().unwrap().starts_with("+C"));
        assert!(super::App::profile_folder_name("").is_err());
        assert!(super::App::profile_folder_name("a/b").is_err());
        assert_eq!(super::App::profile_folder_name(" Road ").unwrap(), "Road");
        assert!(super::App::under_original_documents(std::path::Path::new("/home/me/Documents/My Games/FalloutNV/saves")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_profile_menu_copies_lists_and_a_mod_moves_aside() {
        let root = std::env::temp_dir().join(format!("nvforever-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let profiles = root.join("profiles");
        let saved = profiles.join("Road");
        std::fs::create_dir_all(&saved).unwrap();
        std::fs::write(saved.join("modlist.txt"), "+A\n").unwrap();
        std::fs::write(saved.join("plugins.txt"), "*On.esp\n").unwrap();
        std::fs::write(saved.join("loadorder.txt"), "On.esp\n").unwrap();
        std::fs::write(saved.join("notes.txt"), "keep").unwrap();
        let copy = profiles.join("Road copy");
        super::App::copy_profile_lists(&saved, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(copy.join("modlist.txt")).unwrap(), "+A\n");
        assert!(!copy.join("notes.txt").exists());
        assert!(super::App::copy_profile_lists(&saved, &copy).is_err());
        assert!(super::App::profile_is_under(&saved, &profiles));
        assert!(!super::App::profile_is_under(&profiles, &profiles));
        assert!(super::App::one_name("a/b").is_err());
        let mods = root.join("mods");
        let recycle = root.join("recycle");
        std::fs::create_dir_all(mods.join("Bag")).unwrap();
        std::fs::write(mods.join("Bag").join("a.txt"), "x").unwrap();
        let dest = super::App::move_mod_aside(&mods, &recycle, "Bag").unwrap();
        assert!(dest.join("a.txt").is_file());
        assert!(!mods.join("Bag").exists());
        std::fs::create_dir_all(mods.join("Old")).unwrap();
        super::App::rename_mod_dir(&mods, "Old", "New").unwrap();
        assert!(mods.join("New").is_dir());
        #[cfg(unix)]
        {
            let outside = root.join("outside");
            std::fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, mods.join("Linked")).unwrap();
            assert!(super::App::move_mod_aside(&mods, &recycle, "Linked").is_err());
            assert!(mods.join("Linked").exists());
            std::os::unix::fs::symlink(&saved, profiles.join("LinkedProfile")).unwrap();
            assert!(super::App::remove_profile_dir(&profiles.join("LinkedProfile")).is_err());
        }
        super::App::remove_profile_dir(&copy).unwrap();
        assert!(!copy.exists());
        assert!(saved.join("notes.txt").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dragging_a_row() {
        let centers = [10.0, 30.0, 50.0, 70.0];
        assert_eq!(insertion_index(&centers, 0.0), 0);
        assert_eq!(insertion_index(&centers, 31.0), 2);
        assert_eq!(insertion_index(&centers, 99.0), 4);
        // Row 1 dropped below row 3 (gap 4) ends last; dropped at its own gaps it stays.
        assert_eq!(move_target(1, 4), 3);
        assert_eq!(move_target(1, 1), 1);
        assert_eq!(move_target(1, 2), 1);
        // Row 3 dropped into gap 1 ends second.
        assert_eq!(move_target(3, 1), 1);
        // The same move on a list.
        let mut v = vec!["a", "b", "c", "d"];
        let (from, to) = (0, move_target(0, insertion_index(&centers, 60.0)));
        let x = v.remove(from);
        v.insert(to, x);
        assert_eq!(v, ["b", "c", "a", "d"]);
    }

    #[test]
    fn a_locked_row_stays_put_and_a_separator_round_trips() {
        let rows = vec![
            super::ModRow { name: "A".into(), on: true, separator: false, locked: true },
            super::ModRow::module("B", true),
            super::ModRow { name: "Quests".into(), on: false, separator: true, locked: false },
            super::ModRow::module("C", false),
        ];
        let moved = super::reorder_mods(&rows, 3, 0);
        assert_eq!(moved[0].name, "A");
        assert!(moved[0].locked);
        assert!(moved.iter().any(|r| r.separator && r.name == "Quests"));
        let text = super::App::modlist_text(&rows);
        assert!(text.lines().all(|l| l.starts_with('+') || l.starts_with('-')), "only MO2's prefixes: {text:?}");
        assert!(text.contains("-Quests_separator"));
        let back = super::App::profile_page_rows(&text, &super::App::locks_text(&rows));
        assert!(back.iter().any(|r| r.separator && r.name == "Quests"));
        assert!(back.iter().any(|r| r.locked && r.name == "A"));
        assert_eq!(back.iter().filter(|r| !r.separator).map(|r| r.name.as_str()).collect::<Vec<_>>(), ["A", "B", "C"]);
    }

    #[test]
    fn previews_cut_at_a_paragraph() {
        assert_eq!(preview("short", false), ("short", false));
        let long = format!("{}\n\nrest", "a".repeat(PREVIEW_CHARS + 5));
        assert_eq!(preview(&long, false).0.len(), PREVIEW_CHARS + 5);
        // Byte PREVIEW_CHARS inside a two-byte `·`: no panic, cut at the paragraph after it.
        let wide = format!("{}·b\n\nrest", "a".repeat(PREVIEW_CHARS - 1));
        assert!(!wide.is_char_boundary(PREVIEW_CHARS));
        assert_eq!(preview(&wide, false), (&wide[..wide.len() - 6], true));
        let wide_end = format!("{}—", "a".repeat(PREVIEW_CHARS - 1));
        assert_eq!(preview(&wide_end, false), (wide_end.as_str(), false));
        assert_eq!(preview(&long, true), (long.as_str(), true));
    }

    #[test]
    fn a_plugin_the_game_still_reads_keeps_its_tick() {
        let owned = |pairs: &[(&str, &str)]| pairs.iter().map(|(p, f)| (p.to_string(), f.to_string())).collect::<HashMap<String, String>>();
        // The highest folder holding each plugin (`plugin_home`); "Old Weapons" is off.
        let home = owned(&[("shared.esp", "Old Weapons"), ("oldonly.esp", "Old Weapons"), ("loose.esp", "Old Weapons"), ("indata.esp", "Old Weapons"), ("weapons.esp", "Weapons")]);
        let off: HashSet<String> = HashSet::from(["Old Weapons".to_string()]);
        // Weapons is on and has Shared.esp too, lower than Old Weapons.
        let on: HashMap<String, Vec<String>> = [("shared.esp", "Weapons"), ("weapons.esp", "Weapons")].iter().map(|(p, f)| (p.to_string(), vec![f.to_string()])).collect();
        let elsewhere = |plugin: &str| plugin == "loose.esp" || plugin == "indata.esp";
        assert_eq!(only_in_off_folders(&home, &off, &on, elsewhere), HashSet::from(["oldonly.esp".to_string()]));
        // With nothing else serving them, every plugin of a folder that is off is unticked.
        assert_eq!(only_in_off_folders(&home, &off, &HashMap::new(), |_| false).len(), 4);
    }

    /// TOCTOU-001: dropped bytes go into a new private folder, not one whose name another user
    /// could guess and make first.
    #[test]
    fn dropped_bytes_go_into_a_new_private_folder() {
        let a = write_dropped_bytes("Mod.zip", b"PK").unwrap();
        let b = write_dropped_bytes("Mod.zip", b"PK").unwrap();
        let (da, db) = (a.parent().unwrap().to_path_buf(), b.parent().unwrap().to_path_buf());
        assert_ne!(da, db, "each drop gets its own folder");
        assert!(da.file_name().unwrap().to_string_lossy().starts_with("nvforever-import-"), "{}", da.display());
        assert_eq!(std::fs::read(&a).unwrap(), b"PK");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&da).unwrap().permissions().mode() & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(&da);
        let _ = std::fs::remove_dir_all(&db);
    }
}

#[allow(clippy::items_after_test_module)]
const BACKUP_MAX: &[u32] = &[1, 3, 5, 10, 20];
const BACKUP_MAX_DEFAULT: u32 = 5;

fn backup_dir(home: &EngineHome) -> PathBuf {
    home.root.join("plugin-backups")
}

fn backup_label(path: &std::path::Path) -> String {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("backup").trim_start_matches("plugins-").trim_end_matches(".txt").to_string()
}

fn read_backup_max(home: Option<&EngineHome>) -> u32 {
    let Some(home) = home else { return BACKUP_MAX_DEFAULT };
    std::fs::read_to_string(backup_dir(home).join("max")).ok().and_then(|t| t.trim().parse().ok()).filter(|n| BACKUP_MAX.contains(n)).unwrap_or(BACKUP_MAX_DEFAULT)
}

fn write_backup_max(home: &EngineHome, max: u32) -> Result<(), String> {
    let dir = backup_dir(home);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("max");
    guard_write(&path, None).map_err(|e| e.to_string())?;
    write_whole(&path, max.to_string().as_bytes()).map_err(|e| e.to_string())
}

fn plugin_backups(home: &EngineHome) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(backup_dir(home)).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("plugins-") && n.ends_with(".txt"))).collect();
    files.sort();
    files
}

fn prune_plugin_backups(home: &EngineHome, max: u32) {
    let files = plugin_backups(home);
    let keep = max as usize;
    if files.len() > keep {
        for path in &files[..files.len() - keep] {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn backup_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400) as u32;
    let (h, min, sec) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 { y += 1; }
    format!("{y:04}-{m:02}-{d:02}-{h:02}{min:02}{sec:02}")
}
