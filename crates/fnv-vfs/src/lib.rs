//! Virtual file system over the game's `Data` folder: BSA archives in precedence order
//! plus loose files, with case-insensitive, slash-agnostic paths (docs/design/layer2.md
//! §2.5). Read-only: nothing here writes to the game install.
//!
//! Precedence, lowest first (docs/spec-notes/bsa.md, "Precedence"): the INI
//! `SArchiveList` archives in list order, then `Update.bsa`, then each active plugin's
//! archives (`<plugin stem> - *.bsa`, `<plugin stem>.bsa`) in load order, then loose files.
//! A later source replaces an earlier one's file with the same path. Checked against
//! in-game screenshots [observed]: `Update.bsa` overrides `Fallout - Meshes.bsa`, and a DLC
//! archive overrides the base archives. Still [hypothesis]: `Update.bsa` below the DLC
//! archives, DLC against DLC by load order, and loose files above every archive.

use fnv_bsa::Archive;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// What every [`Vfs`] of the process has read so far (issue #2407): files, stored bytes,
/// time spent in the reads themselves (the archive's seek and read, a loose file's read;
/// not inflating) and time spent waiting for another reader's turn on the same archive.
/// A loading screen takes the difference over its time: many seconds of reading for few
/// bytes is a slow or cold disk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadStats {
    pub files: u64,
    pub bytes: u64,
    pub read_ns: u64,
    pub queued_ns: u64,
}

impl ReadStats {
    /// What was read after `before`.
    pub fn since(&self, before: &ReadStats) -> ReadStats {
        ReadStats {
            files: self.files.saturating_sub(before.files),
            bytes: self.bytes.saturating_sub(before.bytes),
            read_ns: self.read_ns.saturating_sub(before.read_ns),
            queued_ns: self.queued_ns.saturating_sub(before.queued_ns),
        }
    }
}

static FILES: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static READ_NS: AtomicU64 = AtomicU64::new(0);
static QUEUED_NS: AtomicU64 = AtomicU64::new(0);

/// The process's [`ReadStats`] so far.
pub fn read_stats() -> ReadStats {
    ReadStats { files: FILES.load(Ordering::Relaxed), bytes: BYTES.load(Ordering::Relaxed), read_ns: READ_NS.load(Ordering::Relaxed), queued_ns: QUEUED_NS.load(Ordering::Relaxed) }
}

fn count_read(bytes: usize, queued: std::time::Duration, read: std::time::Duration) {
    FILES.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    QUEUED_NS.fetch_add(queued.as_nanos() as u64, Ordering::Relaxed);
    READ_NS.fetch_add(read.as_nanos() as u64, Ordering::Relaxed);
}

/// `SArchiveList` of the shipped `Fallout_default.ini` and of the owner's `Fallout.ini`
/// [observed: both identical on FNV 1.4.0.525].
pub const DEFAULT_ARCHIVE_LIST: [&str; 6] = [
    "Fallout - Textures.bsa",
    "Fallout - Textures2.bsa",
    "Fallout - Meshes.bsa",
    "Fallout - Voices1.bsa",
    "Fallout - Sound.bsa",
    "Fallout - Misc.bsa",
];

/// The base game and DLC masters in their usual load order.
pub const DEFAULT_PLUGINS: [&str; 10] = [
    "FalloutNV.esm",
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
pub enum VfsError {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0}: {1}")]
    Bsa(String, fnv_bsa::BsaError),
}

/// Environment variable that replaces the archive order of [`Vfs::open_default`]
/// (diagnostics only; see there).
pub const ARCHIVE_ORDER_ENV: &str = "NVF_ARCHIVE_ORDER";

static ARCHIVE_LIST: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Set the run's base archive list (an `SArchiveList` from NVForever's own INI, or
/// `fnvtool vfs order --archive-list`) in place of [`DEFAULT_ARCHIVE_LIST`], before any
/// archive is mounted (issue #1142: a mod setup that adds base archives). Once per run: a
/// second call is refused (`Err` with the list kept). An empty list is refused too.
pub fn set_archive_list(list: Vec<String>) -> Result<(), Vec<String>> {
    if list.is_empty() {
        return Err(list);
    }
    ARCHIVE_LIST.set(list)
}

/// The run's base archive list, lowest precedence first: the one set with
/// [`set_archive_list`], else [`DEFAULT_ARCHIVE_LIST`]. `Update.bsa` and the plugins'
/// own archives come after it ([`Vfs::archive_order`]).
pub fn archive_list() -> Vec<String> {
    match ARCHIVE_LIST.get() {
        Some(l) => l.clone(),
        None => DEFAULT_ARCHIVE_LIST.iter().map(|s| s.to_string()).collect(),
    }
}

/// Whether the run's base archive list was set ([`set_archive_list`]).
pub fn archive_list_overridden() -> bool {
    ARCHIVE_LIST.get().is_some()
}

/// Archive names of a comma-separated list (`SArchiveList` style), trimmed, empty items
/// dropped.
pub fn parse_archive_list(list: &str) -> Vec<String> {
    list.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Normalised form of a game path: lower case, backslashes, no leading separator, and a
/// run of separators taken as one. [observed: the Classic Fallout Weapons mod's
/// `Colt6520.nif` names `textures\\weapons\1handpistol\colt6520_anim.dds`, the file lies at
/// `textures\weapons\1handpistol\`, and the owner sees the gun textured in the original
/// game; hypothesis: the game reads the path through Windows' file calls, which take a
/// doubled separator as one (#1705)]
pub fn normalize(path: &str) -> String {
    let mut s = String::with_capacity(path.len());
    for c in path.chars() {
        let c = if c == '/' { '\\' } else { c.to_ascii_lowercase() };
        if c == '\\' && (s.is_empty() || s.ends_with('\\')) {
            continue;
        }
        s.push(c);
    }
    s
}

/// Map a texture path as written in a NIF to a VFS path: NIFs mostly store
/// `textures\...`, but some carry a leading `data\` or an absolute path that contains
/// `\textures\` [hypothesis: the engine strips everything before `textures\`].
pub fn texture_path(raw: &str) -> String {
    let p = normalize(raw);
    if p.starts_with("textures\\") {
        return p;
    }
    if let Some(i) = p.find("\\textures\\") {
        return p[i + 1..].to_string();
    }
    format!("textures\\{p}")
}

/// The path of the normal map that lies beside a texture by name: the same path with
/// `_n` before the extension (`…\ghoul\upperbodymale.dds` → `…\ghoul\upperbodymale_n.dds`);
/// the rest of the path is kept as written. `None` for a path without a file name and
/// extension. Whether that file exists is the caller's question ([`Vfs::exists`]).
/// [observed: every skin shape of the actor models names in texture slot 1 the file
/// named this way after its slot 0; hypothesis: the game finds the normal map of a skin
/// texture it puts in by this name, docs/spec-notes/actors.md §3.1]
pub fn normal_map_beside(texture: &str) -> Option<String> {
    let file_start = texture.rfind(['\\', '/']).map_or(0, |i| i + 1);
    let dot = file_start + texture[file_start..].rfind('.')?;
    if dot == file_start || dot + 1 == texture.len() {
        return None;
    }
    Some(format!("{}_n{}", &texture[..dot], &texture[dot..]))
}

#[derive(Debug, Clone, Copy)]
enum Location {
    Archive { archive: usize, folder: usize, file: usize },
    Loose,
}

pub struct Vfs {
    data_dir: PathBuf,
    archives: Vec<(String, Mutex<Archive>)>,
    /// The file each `archives` entry was opened from (`Data` or a mod folder, #1526).
    archive_files: Vec<PathBuf>,
    index: HashMap<String, Location>,
    /// Lower-cased loose path → winning physical path (preserving filesystem casing).
    loose: HashMap<String, PathBuf>,
    /// [`Vfs::loose`]'s keys sorted, made at the first folder listing and dropped when a
    /// root is indexed: a listing is then a range of it, not a pass over every key
    /// [observed: Nuclear Sunset's GetFilesInFolder ~200 ms a call, 2026-11-25].
    loose_sorted: Mutex<Option<Vec<String>>>,
    /// The folders loose files were indexed from, lowest precedence first: `Data`, then
    /// the mods folders (#1526).
    loose_roots: Vec<PathBuf>,
    /// The highest archive copy of each loose path that also sits in an archive, which
    /// the loose file hides from [`Vfs::read`] ([`Vfs::read_archived`], #975).
    under_loose: HashMap<String, Location>,
    /// Archives and folders that could not be read, with why: skipped, not fatal (a
    /// broken mod must not stop the game). Kept so a caller whose log is not up yet can
    /// report them later.
    skipped: Vec<(String, String)>,
    /// Archives that cannot be opened or whose tables are damaged ([`fnv_bsa::check`]),
    /// with why. The game will not start with one (engine `main.rs`); tools go on with
    /// what reads.
    broken: Vec<(String, String)>,
}

impl Vfs {
    /// Archive file names in precedence order for `plugins` (lowest first).
    pub fn archive_order(data_dir: &Path, archive_list: &[&str], plugins: &[&str]) -> Vec<String> {
        Vfs::archive_order_in(data_dir, None, archive_list, plugins)
    }

    /// [`Vfs::archive_order`], with the plugins' archives also looked for in NVForever's
    /// mods folder `mods`, beside the plugins it holds (docs/design/editor.md §2.2 item 9).
    /// One found there is named by its full path ([`Vfs::open`] joins a relative name to
    /// `data_dir`); a name in both folders is the mods folder's, as a plugin is
    /// (`fnv_install::plugins::plugin_path`). The base list and `Update.bsa` come from
    /// `data_dir` only.
    pub fn archive_order_in(data_dir: &Path, mods: Option<&Path>, archive_list: &[&str], plugins: &[&str]) -> Vec<String> {
        let present = top_archives(data_dir);
        let in_mods: Vec<String> = mods.map(top_archives).unwrap_or_default();
        // Each plugin archive's name, and what `Vfs::open` is given for it.
        let mut owned: Vec<(String, String)> = present.iter().filter(|n| !in_mods.iter().any(|m| m.eq_ignore_ascii_case(n))).map(|n| (n.clone(), n.clone())).collect();
        if let Some(m) = mods {
            owned.extend(in_mods.iter().map(|n| (n.clone(), m.join(n).to_string_lossy().into_owned())));
        }
        Vfs::order_from_owned(&owned, &present, archive_list, plugins)
    }

    /// [`Vfs::archive_order_in`] for any number of mod roots layered over `Data`, lowest
    /// precedence first (#1526 phase 2): an archive name is opened from the highest layer
    /// that has it, the overwrite folder first, so a mod's copy of a base archive replaces
    /// the game's, as usvfs' merged `Data` would show it. The order is the base list's,
    /// then `Update.bsa`, then each plugin's archives as [`Vfs::archive_order_in`].
    pub fn archive_files_layered(data_dir: &Path, roots: &[PathBuf], archive_list: &[&str], plugins: &[&str]) -> Vec<(String, PathBuf)> {
        // Top of each root only. `--mo2` uses this. A nested archive is the launcher's
        // own path (`archive_files_nested`), not this one.
        let mut layers: Vec<&Path> = vec![data_dir];
        layers.extend(roots.iter().filter(|r| r.is_dir()).map(PathBuf::as_path));
        let mut found: Vec<(String, PathBuf)> = Vec::new();
        for dir in layers {
            let mut names = top_archives(dir);
            names.sort();
            for n in names {
                let entry = (n.clone(), dir.join(&n));
                match found.iter_mut().find(|(o, _)| o.eq_ignore_ascii_case(&n)) {
                    Some(slot) => *slot = entry,
                    None => found.push(entry),
                }
            }
        }
        Self::order_found(&found, archive_list, plugins)
    }

    /// [`Vfs::archive_files_layered`] for the launcher's mod folders. Data is still only
    /// the archives sitting in it. A mod folder is read recursively, so an archive in a
    /// nested folder is not missed. `--mo2` does not call this.
    ///
    /// `top_only` (the launcher's mods folder) is read at its top only, as the layer just
    /// above Data: an archive left there loose, outside any mod folder.
    fn archive_files_nested(data_dir: &Path, top_only: Option<&Path>, roots: &[PathBuf], archive_list: &[&str], plugins: &[&str]) -> Vec<(String, PathBuf)> {
        let mut layers: Vec<(&Path, bool)> = vec![(data_dir, true)];
        layers.extend(top_only.filter(|d| d.is_dir()).map(|d| (d, true)));
        layers.extend(roots.iter().filter(|r| r.is_dir()).map(|r| (r.as_path(), false)));
        let mut found: Vec<(String, PathBuf)> = Vec::new();
        for (dir, top) in layers {
            let files = if top {
                top_archives(dir).into_iter().map(|n| (n.clone(), dir.join(&n))).collect::<Vec<_>>()
            } else {
                archives_recursive(dir)
            };
            for (n, path) in files {
                let entry = (n.clone(), path);
                match found.iter_mut().find(|(o, _)| o.eq_ignore_ascii_case(&n)) {
                    Some(slot) => *slot = entry,
                    None => found.push(entry),
                }
            }
        }
        Self::order_found(&found, archive_list, plugins)
    }

    fn order_found(found: &[(String, PathBuf)], archive_list: &[&str], plugins: &[&str]) -> Vec<(String, PathBuf)> {
        let owned: Vec<(String, String)> = found.iter().map(|(n, p)| (n.clone(), p.to_string_lossy().into_owned())).collect();
        let base: Vec<String> = found.iter().map(|(n, _)| n.clone()).collect();
        let names = Vfs::order_from_owned(&owned, &base, archive_list, plugins);
        names
            .into_iter()
            .filter_map(|n| {
                let (o, p) = owned.iter().find(|(o, p)| o.eq_ignore_ascii_case(&n) || p.eq_ignore_ascii_case(&n))?;
                Some((o.clone(), PathBuf::from(p)))
            })
            .collect()
    }

    /// The precedence order of the archive files `owned` (display name, what to open),
    /// lowest precedence first: the base list, `Update.bsa`, then each active plugin's own
    /// archives. Shared by [`Vfs::archive_order_in`] and [`Vfs::archive_files_layered`].
    fn order_from_owned(owned: &[(String, String)], base: &[String], archive_list: &[&str], plugins: &[&str]) -> Vec<String> {
        let find = |name: &str| base.iter().find(|n| n.eq_ignore_ascii_case(name)).cloned();
        let mut order: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut push = |display: String, open_as: String| {
            if !seen.iter().any(|o| o.eq_ignore_ascii_case(&display)) {
                seen.push(display);
                order.push(open_as);
            }
        };
        for a in archive_list {
            if let Some(n) = find(a) {
                push(n.clone(), n);
            }
        }
        if let Some(n) = find("Update.bsa") {
            push(n.clone(), n);
        }
        let stems = plugin_stems(plugins);
        for stem in &stems {
            let mut own: Vec<(u64, &(String, String))> = owned
                .iter()
                .filter(|(n, _)| archive_owner(&stems, n) == Some(stem.as_str()))
                .filter_map(|e| plugin_archive_piece(stem, &e.0).map(|k| (k, e)))
                .collect();
            // Unnumbered archives by name, then the numbered pieces in number order.
            own.sort_by_key(|(k, (n, _))| (*k, n.to_ascii_lowercase()));
            let own = own.into_iter().map(|(_, e)| e);
            for (display, open_as) in own {
                push(display.clone(), open_as.clone());
            }
        }
        order
    }

    /// Open the archives (lowest precedence first) and index loose files under `data_dir`.
    pub fn open(data_dir: &Path, archives: &[String]) -> Result<Vfs, VfsError> {
        let files: Vec<(String, PathBuf)> = archives.iter().map(|n| (n.clone(), data_dir.join(n))).collect();
        Vfs::open_files(data_dir, &files)
    }

    /// [`Vfs::open`] with each archive's file given (a mod's archive does not sit in
    /// `Data`, #1526 phase 2); `archives` keeps the display names.
    pub fn open_files(data_dir: &Path, archives: &[(String, PathBuf)]) -> Result<Vfs, VfsError> {
        let mut vfs = Vfs {
            data_dir: data_dir.to_path_buf(),
            archives: Vec::new(),
            archive_files: Vec::new(),
            index: HashMap::new(),
            loose: HashMap::new(),
            loose_sorted: Mutex::new(None),
            loose_roots: Vec::new(),
            under_loose: HashMap::new(),
            skipped: Vec::new(),
            broken: Vec::new(),
        };
        for (name, file) in archives {
            // A bad or wrong-version archive is skipped; the others still load.
            let a = match Archive::open(file) {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!(archive = %name, path = %file.display(), error = %e, "archive skipped: cannot be read");
                    vfs.skipped.push((name.clone(), e.to_string()));
                    vfs.broken.push((name.clone(), e.to_string()));
                    continue;
                }
            };
            if let Err(e) = a.check_tables() {
                tracing::warn!(archive = %name, error = %e, "archive damaged: its tables point past the end");
                vfs.broken.push((name.clone(), e.to_string()));
            }
            let ai = vfs.archives.len();
            for e in a.entries() {
                vfs.index.insert(
                    normalize(&e.path()),
                    Location::Archive { archive: ai, folder: e.folder_index, file: e.file_index },
                );
            }
            vfs.archives.push((name.clone(), Mutex::new(a)));
            vfs.archive_files.push(file.clone());
        }
        vfs.index_loose(data_dir);
        Ok(vfs)
    }

    fn index_loose(&mut self, root: &Path) {
        self.index_loose_skipping(root, &[]);
    }

    fn index_loose_skipping(&mut self, root: &Path, skip_top: &[String]) {
        if !self.loose_roots.iter().any(|r| r == root) {
            self.loose_roots.push(root.to_path_buf());
        }
        let mut files = Vec::new();
        let mut open: Vec<PathBuf> = std::fs::canonicalize(root).into_iter().collect();
        walk(root, root, &mut open, &mut files, &mut self.skipped, skip_top);
        for rel in files {
            let key = normalize(&rel.to_string_lossy());
            // Plugins and archives at the top level are not part of the asset namespace.
            if !key.contains('\\') {
                continue;
            }
            if let Some(under @ Location::Archive { .. }) = self.index.insert(key.clone(), Location::Loose) {
                self.under_loose.insert(key.clone(), under);
            }
            self.loose.insert(key, root.join(rel));
        }
        *self.loose_sorted.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Default setup: the INI archive list, `Update.bsa`, and the base + DLC plugins.
    ///
    /// Diagnostic override: when the environment variable [`ARCHIVE_ORDER_ENV`] is set, its
    /// comma-separated archive names (lowest precedence first, like `SArchiveList`) replace
    /// that order. It exists to render the same scene under competing precedence rules and
    /// compare them with the original game (docs/spec-notes/bsa.md, "Precedence").
    pub fn open_default(data_dir: &Path) -> Result<Vfs, VfsError> {
        Vfs::open_for_plugins(data_dir, &DEFAULT_PLUGINS)
    }

    /// [`Vfs::open_default`] for the plugins actually loaded, in load order: a mod's
    /// archives (`<mod> - *.bsa`, `<mod>.bsa`) come above the base game's and DLC's when
    /// its plugin loads after them, and loose files stay above every archive. The
    /// [`ARCHIVE_ORDER_ENV`] override applies as there.
    pub fn open_for_plugins<S: AsRef<str>>(data_dir: &Path, plugins: &[S]) -> Result<Vfs, VfsError> {
        Vfs::open_for_plugins_in(data_dir, None, plugins)
    }

    /// [`Vfs::open_for_plugins`] with the archives beside the plugins in NVForever's mods
    /// folder `mods` too ([`Vfs::archive_order_in`]), and its loose files above Data's.
    pub fn open_for_plugins_in<S: AsRef<str>>(data_dir: &Path, mods: Option<&Path>, plugins: &[S]) -> Result<Vfs, VfsError> {
        let mut vfs = Vfs::open(data_dir, &Vfs::order_for_plugins_in(data_dir, mods, plugins).0)?;
        if let Some(root) = mods {
            // An absent optional layer is ordinary; other errors retain walk's diagnostics.
            if !matches!(root.try_exists(), Ok(false)) {
                vfs.index_loose(root);
            }
        }
        Ok(vfs)
    }

    /// [`Vfs::open_for_plugins`] with loose files from the mods folders `roots` layered
    /// over `Data`, lowest precedence first (issue #1526: Mod Organizer 2's enabled mods
    /// from the lowest priority up, then its overwrite folder). The archives mounted are
    /// the base list's (the profile's, under `--mo2`), `Update.bsa` and the plugins'
    /// archives, each taken from the highest layer that holds it.
    pub fn open_for_plugins_layered<S: AsRef<str>>(data_dir: &Path, roots: &[PathBuf], plugins: &[S]) -> Result<Vfs, VfsError> {
        let files = Vfs::order_for_plugins_layered(data_dir, roots, plugins).0;
        let mut vfs = Vfs::open_files(data_dir, &files)?;
        for root in roots {
            // An absent optional layer is ordinary; other errors retain walk's diagnostics.
            if root.as_path() == data_dir || matches!(root.try_exists(), Ok(false)) {
                continue;
            }
            vfs.index_loose(root);
        }
        Ok(vfs)
    }

    /// The launcher's own mods folder. `layers` are the enabled mod folders, lowest
    /// priority first (a `Data` directory inside a mod when it has one). The last layer
    /// wins a loose file. Loose files and archives are read from those folders.
    /// Files in `data_dir` are still read: a plugin, archive, or loose file installed
    /// in Data stays visible. The same relative path in a layer replaces it.
    ///
    /// Archives and loose files that sit directly in `mods_dir`, rather than inside a mod
    /// folder, are included: such an archive is mounted only as Data's would be (the base
    /// list, `Update.bsa`, or an active plugin's own archive), in the same order. Subfolders that are themselves mods are not walked from
    /// `mods_dir`; an enabled one is a layer, a disabled one is left out.
    /// The game does not read `mod-order.txt`. Without layers, folder on/off and the
    /// launcher's order are not applied.
    pub fn open_launcher_mods<S: AsRef<str>>(data_dir: &Path, mods_dir: &Path, layers: &[PathBuf], plugins: &[S]) -> Result<Vfs, VfsError> {
        // An archive loose at the top of the mods folder is one more layer, just above Data,
        // under the same rule as every other archive: the base list, `Update.bsa`, and the
        // archives an active plugin owns, in load order (`order_from_owned`).
        let archive_layers = bsa_scan_roots(layers);
        let files = Vfs::order_for_launcher_layers(data_dir, Some(mods_dir), &archive_layers, plugins).0;
        let mut vfs = Vfs::open_files(data_dir, &files)?;
        let skip = mod_folder_names(mods_dir);
        vfs.index_loose_skipping(mods_dir, &skip);
        for root in layers {
            if root.as_path() == data_dir || matches!(root.try_exists(), Ok(false)) {
                continue;
            }
            vfs.index_loose(root);
        }
        Ok(vfs)
    }

    /// Index `root`'s loose files over every layer already indexed (the highest precedence),
    /// for this mount only: nothing is written anywhere. For a test aid that mounts a fixture
    /// folder it created for the run (nvf-assets `pbr_field/reflect.rs` `seed_mount_pack`)
    /// instead of writing into a mods folder.
    pub fn add_loose_root(&mut self, root: &Path) {
        self.index_loose(root);
    }

    /// The folders loose files were read from, lowest precedence first (`Data` first).
    pub fn loose_roots(&self) -> &[PathBuf] {
        &self.loose_roots
    }

    /// The archives [`Vfs::open_for_plugins`] mounts, lowest precedence first, and
    /// whether they came from the [`ARCHIVE_ORDER_ENV`] override (`fnvtool vfs order`).
    /// The base list is [`archive_list`]'s: an `SArchiveList` set for the run, else the
    /// shipped default.
    pub fn order_for_plugins<S: AsRef<str>>(data_dir: &Path, plugins: &[S]) -> (Vec<String>, bool) {
        Vfs::order_for_plugins_in(data_dir, None, plugins)
    }

    /// [`Vfs::order_for_plugins`] with the mods folder's archives ([`Vfs::archive_order_in`]).
    pub fn order_for_plugins_in<S: AsRef<str>>(data_dir: &Path, mods: Option<&Path>, plugins: &[S]) -> (Vec<String>, bool) {
        match std::env::var(ARCHIVE_ORDER_ENV) {
            Ok(list) if !list.trim().is_empty() => (parse_archive_list(&list), true),
            _ => {
                let plugins: Vec<&str> = plugins.iter().map(AsRef::as_ref).collect();
                let list = archive_list();
                let list: Vec<&str> = list.iter().map(String::as_str).collect();
                (Vfs::archive_order_in(data_dir, mods, &list, &plugins), false)
            }
        }
    }

    /// [`Vfs::order_for_plugins`] for layered roots: each archive is opened from the highest
    /// layer that holds it (#1526 phase 2). The [`ARCHIVE_ORDER_ENV`] override applies as
    /// there, its names resolved through the layers too.
    pub fn order_for_plugins_layered<S: AsRef<str>>(data_dir: &Path, roots: &[PathBuf], plugins: &[S]) -> (Vec<(String, PathBuf)>, bool) {
        match std::env::var(ARCHIVE_ORDER_ENV) {
            Ok(list) if !list.trim().is_empty() => {
                let files = parse_archive_list(&list)
                    .into_iter()
                    .map(|n| {
                        let p = Vfs::archive_file_in_layers(data_dir, roots, &n).unwrap_or_else(|| data_dir.join(&n));
                        (n, p)
                    })
                    .collect();
                (files, true)
            }
            _ => {
                let plugins: Vec<&str> = plugins.iter().map(AsRef::as_ref).collect();
                let list = archive_list();
                let list: Vec<&str> = list.iter().map(String::as_str).collect();
                (Vfs::archive_files_layered(data_dir, roots, &list, &plugins), false)
            }
        }
    }

    /// [`Vfs::order_for_plugins_layered`] for the launcher. Nested archives in a mod
    /// folder are included. `--mo2` keeps [`Vfs::order_for_plugins_layered`], which
    /// reads only the top of each root.
    fn order_for_launcher_layers<S: AsRef<str>>(data_dir: &Path, top_only: Option<&Path>, roots: &[PathBuf], plugins: &[S]) -> (Vec<(String, PathBuf)>, bool) {
        match std::env::var(ARCHIVE_ORDER_ENV) {
            Ok(list) if !list.trim().is_empty() => {
                let lookup: Vec<PathBuf> = top_only.map(Path::to_path_buf).into_iter().chain(roots.iter().cloned()).collect();
                let files = parse_archive_list(&list)
                    .into_iter()
                    .map(|n| {
                        let p = Vfs::archive_file_in_layers(data_dir, &lookup, &n).unwrap_or_else(|| data_dir.join(&n));
                        (n, p)
                    })
                    .collect();
                (files, true)
            }
            _ => {
                let plugins: Vec<&str> = plugins.iter().map(AsRef::as_ref).collect();
                let list = archive_list();
                let list: Vec<&str> = list.iter().map(String::as_str).collect();
                (Vfs::archive_files_nested(data_dir, top_only, roots, &list, &plugins), false)
            }
        }
    }

    /// The highest layer's file `name`, matched without case: `roots` highest priority
    /// first, then `data_dir`.
    pub fn archive_file_in_layers(data_dir: &Path, roots: &[PathBuf], name: &str) -> Option<PathBuf> {
        for dir in roots.iter().rev().map(PathBuf::as_path).chain(std::iter::once(data_dir)) {
            let Ok(rd) = std::fs::read_dir(dir) else { continue };
            if let Some(p) = rd.flatten().map(|e| e.path()).find(|p| p.is_file() && p.file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(name))) {
                return Some(p);
            }
        }
        None
    }

    pub fn archives(&self) -> impl Iterator<Item = &str> {
        self.archives.iter().map(|(n, _)| n.as_str())
    }

    /// The file the archive `name` was opened from: `Data`'s folder, or a mod's (#1526).
    pub fn archive_path(&self, name: &str) -> Option<&Path> {
        let i = self.archives.iter().position(|(n, _)| n.eq_ignore_ascii_case(name))?;
        self.archive_files.get(i).map(PathBuf::as_path)
    }

    /// The game's `Data` folder (its top-level files, `Credits.txt`, are not in the asset
    /// namespace).
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Archives and loose folders skipped because they could not be read: (name, why).
    pub fn skipped(&self) -> &[(String, String)] {
        &self.skipped
    }

    /// Archives that cannot be opened or whose header or tables are damaged: (name, why).
    pub fn broken_archives(&self) -> &[(String, String)] {
        &self.broken
    }

    pub fn exists(&self, path: &str) -> bool {
        self.index.contains_key(&normalize(path))
    }

    /// Which source provides `path`: an archive name, or `"loose"`.
    pub fn source_of(&self, path: &str) -> Option<&str> {
        match self.index.get(&normalize(path))? {
            Location::Archive { archive, .. } => Some(&self.archives[*archive].0),
            Location::Loose => Some("loose"),
        }
    }

    /// The file on disk of a loose `path` (for reading a large file as a stream); `None`
    /// when no source has it or an archive does.
    pub fn loose_path(&self, path: &str) -> Option<PathBuf> {
        let key = normalize(path);
        match self.index.get(&key)? {
            Location::Loose => Some(self.loose[&key].clone()),
            Location::Archive { .. } => None,
        }
    }

    /// Decoded contents of `path`, or `None` when no source has it.
    pub fn read(&self, path: &str) -> Option<std::io::Result<Vec<u8>>> {
        let key = normalize(path);
        Some(self.read_at(&key, *self.index.get(&key)?))
    }

    /// Decoded contents of the highest archive copy of `path`, also when a loose file
    /// replaces it; `None` when no archive has it. Menu files are compared with the
    /// copy a loose replacement hides (docs/design/ui-tile-tree.md, #975).
    pub fn read_archived(&self, path: &str) -> Option<std::io::Result<Vec<u8>>> {
        let key = normalize(path);
        let at = match *self.index.get(&key)? {
            Location::Loose => *self.under_loose.get(&key)?,
            at => at,
        };
        Some(self.read_at(&key, at))
    }

    fn read_at(&self, key: &str, at: Location) -> std::io::Result<Vec<u8>> {
        match at {
            Location::Loose => {
                let t = std::time::Instant::now();
                let bytes = std::fs::read(&self.loose[key]);
                count_read(bytes.as_ref().map_or(0, |b| b.len()), std::time::Duration::ZERO, t.elapsed());
                bytes
            }
            Location::Archive { archive, folder, file } => {
                // The lock covers only the seek and read of the stored bytes; inflating
                // happens after it is let go, so workers reading the same archive do not
                // queue behind each other's decompression (#997).
                let stored = {
                    let asked = std::time::Instant::now();
                    let mut a = self.archives[archive].1.lock().unwrap_or_else(|p| p.into_inner());
                    let got = std::time::Instant::now();
                    let r = a.folders[folder].files[file].clone();
                    let stored = a.read_stored(&r);
                    count_read(r.stored_size() as usize, got - asked, got.elapsed());
                    stored
                };
                stored.and_then(|b| b.decode()).map_err(std::io::Error::other)
            }
        }
    }

    /// [`Self::read`] of a file of at most `stored` bytes where it lies and `contents` bytes
    /// decoded. For an archived file the stored size is looked at before its buffer is made and
    /// the size its contents declare before they are inflated (what inflates to more is an error
    /// already). A loose file, which is both, is refused by the opened file's length before anything
    /// is read, and when it gives more than that. A larger file is an error of kind
    /// `FileTooLarge` that names the path and the size. In [`read_stats`] an archived file
    /// counts once its stored bytes are read, a loose one when it is read whole.
    pub fn read_at_most(&self, path: &str, stored: usize, contents: usize) -> Option<std::io::Result<Vec<u8>>> {
        let key = normalize(path);
        Some(match *self.index.get(&key)? {
            Location::Loose => self.read_loose_at_most(&key, stored.min(contents)),
            Location::Archive { archive, folder, file } => self.read_archived_at_most(&key, (archive, folder, file), stored, contents),
        })
    }

    fn read_loose_at_most(&self, key: &str, most: usize) -> std::io::Result<Vec<u8>> {
        let t = std::time::Instant::now();
        let file = std::fs::File::open(&self.loose[key]).and_then(|f| Ok((f.metadata()?.len(), f)));
        let bytes = file.and_then(|(len, f)| take_at_most(f, len, most)).map_err(|e| std::io::Error::new(e.kind(), format!("{key}: {e}")))?;
        count_read(bytes.len(), std::time::Duration::ZERO, t.elapsed());
        Ok(bytes)
    }

    fn read_archived_at_most(&self, key: &str, (archive, folder, file): (usize, usize, usize), stored: usize, contents: usize) -> std::io::Result<Vec<u8>> {
        let broken = |e| std::io::Error::other(format!("{key}: {e}"));
        // The lock as in `read_at`: for the seek and the read of the stored bytes alone.
        let block = {
            let asked = std::time::Instant::now();
            let mut a = self.archives[archive].1.lock().unwrap_or_else(|p| p.into_inner());
            let got = std::time::Instant::now();
            let r = a.folders[folder].files[file].clone();
            let size = r.stored_size() as usize;
            if size > stored {
                return Err(too_large(key, format_args!("{size} bytes stored, more than the {stored} allowed")));
            }
            let block = a.read_stored(&r);
            count_read(size, got - asked, got.elapsed());
            block.map_err(broken)?
        };
        if let Some(size) = block.contents_len().filter(|n| *n > contents) {
            return Err(too_large(key, format_args!("{size} bytes decoded, more than the {contents} allowed")));
        }
        block.decode().map_err(broken)
    }

    /// The loose files under a folder (lower case, `\` between parts, no leading `\`), as
    /// their whole normalized paths, sorted: every layer's (`Data` and the mods folders), from
    /// the index, so a folder made of several mods' files lists them all without walking the
    /// folders on disk.
    pub fn loose_paths_under(&self, folder: &str) -> Vec<String> {
        let mut pre = normalize(folder);
        if !pre.is_empty() && !pre.ends_with('\\') {
            pre.push('\\');
        }
        let mut sorted = self.loose_sorted.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let keys = sorted.get_or_insert_with(|| {
            let mut k: Vec<String> = self.loose.keys().cloned().collect();
            k.sort();
            k
        });
        let start = keys.partition_point(|k| k.as_str() < pre.as_str());
        keys[start..].iter().take_while(|k| k.starts_with(&pre)).cloned().collect()
    }

    /// Every path (normalised) for which `pred` holds, sorted.
    pub fn paths(&self, pred: impl Fn(&str) -> bool) -> Vec<String> {
        let mut v: Vec<String> = self.index.keys().filter(|k| pred(k)).cloned().collect();
        v.sort();
        v
    }
}

/// All of `from`, an opened file of `len` bytes, when that is at most `most`: nothing is read
/// otherwise, and room is made for `len` and no more. One byte is asked for past them, so one
/// that grew since is an error and is not read on.
fn take_at_most(mut from: impl std::io::Read, len: u64, most: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Error, ErrorKind, Read as _};
    let over = |size: u64| Error::new(ErrorKind::FileTooLarge, format!("{size} bytes, more than the {most} allowed"));
    let room = usize::try_from(len).ok().filter(|n| *n <= most).ok_or_else(|| over(len))?;
    let mut bytes = Vec::with_capacity(room);
    from.by_ref().take(len).read_to_end(&mut bytes)?;
    match from.take(1).read_to_end(&mut Vec::new())? {
        0 => Ok(bytes),
        _ if room == most => Err(over(len + 1)),
        _ => Err(Error::other(format!("it grew past its {len} bytes while it was read"))),
    }
}

fn too_large(key: &str, how: std::fmt::Arguments) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::FileTooLarge, format!("{key}: {how}"))
}

/// The `*.bsa` files directly in `dir` (a mod's archive lies at its root, not nested).
fn top_archives(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.to_ascii_lowercase().ends_with(".bsa"))
                .collect()
        })
        .unwrap_or_default()
}

/// Every loose file under `dir`, following directory symlinks and Windows NTFS directory
/// junctions while skipping cycles (links back into an active ancestor in `open`) and
/// broken links. A folder or entry that cannot be read is skipped (and noted in `skipped`);
/// the rest of the walk goes on. A folder directly under `root` named in `skip_top` (a mod
/// folder, seen from the top of the mods folder) is not entered, linked or not.
fn walk(root: &Path, dir: &Path, open: &mut Vec<PathBuf>, out: &mut Vec<PathBuf>, skipped: &mut Vec<(String, String)>, skip_top: &[String]) {
    fn skip(skipped: &mut Vec<(String, String)>, p: &Path, e: std::io::Error) {
        tracing::warn!(path = %p.display(), error = %e, "loose files skipped: cannot be read");
        skipped.push((p.display().to_string(), e.to_string()));
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => return skip(skipped, dir, e),
    };
    let mut subdirs: Vec<(PathBuf, bool)> = Vec::new();
    for e in rd {
        let e = match e {
            Ok(e) => e,
            Err(err) => {
                skip(skipped, dir, err);
                continue;
            }
        };
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => subdirs.push((p, false)),
            Ok(t) if t.is_file() => {
                if let Ok(rel) = p.strip_prefix(root) {
                    out.push(rel.to_path_buf());
                }
            }
            // Symlinks and Windows NTFS directory junctions report neither `is_dir` nor
            // `is_file` on `DirEntry::file_type()`: follow via `metadata` and guard cycles.
            Ok(_) => match std::fs::metadata(&p) {
                Ok(m) if m.is_dir() => subdirs.push((p, true)),
                Ok(m) if m.is_file() => {
                    if let Ok(rel) = p.strip_prefix(root) {
                        out.push(rel.to_path_buf());
                    }
                }
                Ok(_) => {}
                Err(err) => skip(skipped, &p, err),
            },
            Err(err) => skip(skipped, &p, err),
        }
    }
    for (p, linked) in subdirs {
        // A mod folder seen from the top of the mods folder is a layer of its own, or off.
        if dir == root && p.file_name().is_some_and(|n| skip_top.iter().any(|s| n.eq_ignore_ascii_case(s))) {
            continue;
        }
        let real = match open.last() {
            Some(parent) if !linked => p.file_name().map(|name| parent.join(name)),
            _ => match std::fs::canonicalize(&p) {
                Ok(r) => Some(r),
                Err(err) => {
                    skip(skipped, &p, err);
                    continue;
                }
            },
        };
        if let Some(real) = real {
            if open.contains(&real) {
                continue;
            }
            open.push(real);
            walk(root, &p, open, out, skipped, skip_top);
            open.pop();
        } else {
            walk(root, &p, open, out, skipped, skip_top);
        }
    }
}

/// `.bsa` files under `dir`, including nested folders. Installer folders are not entered.
fn archives_recursive(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0u8)];
    let mut seen = std::collections::HashSet::new();
    while let Some((d, depth)) = stack.pop() {
        if depth > 32 {
            continue;
        }
        let key = std::fs::canonicalize(&d).unwrap_or_else(|_| d.clone());
        if !seen.insert(key) {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let lower = name.to_ascii_lowercase();
            if p.is_dir() {
                if lower.starts_with('.') || matches!(lower.as_str(), "fomod" | "meta" | "__macosx" | "docs" | "source") {
                    continue;
                }
                stack.push((p, depth + 1));
            } else if lower.ends_with(".bsa") && !out.iter().any(|(n, _): &(String, PathBuf)| n.eq_ignore_ascii_case(&name)) {
                out.push((name, p));
            }
        }
    }
    out.sort_by_key(|a| a.0.to_ascii_lowercase());
    out
}

/// Immediate subfolders of the mods folder that are mods, not asset folders.
fn mod_folder_names(mods: &Path) -> Vec<String> {
    std::fs::read_dir(mods)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let lower = name.to_ascii_lowercase();
            if !e.path().is_dir() || lower.starts_with('.') || is_asset_dir(&lower) {
                None
            } else {
                Some(name)
            }
        })
        .collect()
}

fn is_asset_dir(lower: &str) -> bool {
    matches!(lower, "meshes" | "textures" | "sound" | "music" | "video" | "menus" | "interface" | "scripts" | "facegen" | "shaders" | "lodsettings" | "trees" | "materials" | "distantlod" | "lsdata" | "nvse" | "data")
}

/// A layer that points at a mod's inner `Data` folder is scanned from the mod folder,
/// so an archive beside that `Data` folder is still found.
/// The plugins' names without their extension, lowercase, in load order.
fn plugin_stems(plugins: &[&str]) -> Vec<String> {
    plugins.iter().map(|p| p.rsplit_once('.').map_or(*p, |(s, _)| s).to_ascii_lowercase()).collect()
}

/// A plugin owns `<stem>.bsa`, space-suffixed archives `<stem> *.bsa` (including
/// `<stem> - *.bsa` and modlist suites such as `NS A Content T 00.bsa`, #3756), and the
/// numbered pieces a packer splits a big archive into, `<stem>N.bsa` / `<stem>N *.bsa`
/// (#2521; docs/design/mod-folders.md). The piece's number (0 for an unnumbered archive)
/// when `name` is `stem`'s. `stem` is lowercase, as [`plugin_stems`] gives it.
fn plugin_archive_piece(stem: &str, name: &str) -> Option<u64> {
    if stem.is_empty() {
        return None;
    }
    let l = name.to_ascii_lowercase();
    let rest = l.strip_prefix(stem)?;
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let (num, tail) = rest.split_at(digits);
    (tail == ".bsa" || (tail.starts_with(' ') && tail.ends_with(".bsa"))).then(|| num.parse().unwrap_or(0))
}

/// The plugin that owns archive `name`: of the stems that claim it, the longest, so
/// `Foo - Bar.bsa` is `Foo - Bar.esp`'s, not `Foo.esp`'s (#996).
fn archive_owner<'a>(stems: &'a [String], name: &str) -> Option<&'a str> {
    stems.iter().filter(|s| plugin_archive_piece(s, name).is_some()).max_by_key(|s| s.len()).map(String::as_str)
}

fn bsa_scan_roots(layers: &[PathBuf]) -> Vec<PathBuf> {
    layers
        .iter()
        .map(|p| {
            if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Data")) {
                p.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| p.clone())
            } else {
                p.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The install check (fnv-install) asks for every archive of the default list and
    /// knows the same add-on masters.
    #[test]
    fn install_check_covers_default_archives() {
        for a in DEFAULT_ARCHIVE_LIST {
            assert!(fnv_install::REQUIRED_DATA_FILES.contains(&a), "{a}");
        }
        assert_eq!(fnv_install::REQUIRED_DATA_FILES[0], DEFAULT_PLUGINS[0]);
        assert_eq!(fnv_install::ADDON_MASTERS[..], DEFAULT_PLUGINS[1..]);
    }

    #[test]
    fn paths_normalise() {
        assert_eq!(normalize("/Meshes/Clutter/X.NIF"), "meshes\\clutter\\x.nif");
        assert_eq!(texture_path("Textures/Clutter/a.dds"), "textures\\clutter\\a.dds");
        assert_eq!(texture_path("data\\textures\\a.dds"), "textures\\a.dds");
        assert_eq!(texture_path("c:\\proj\\data\\textures\\b\\a.dds"), "textures\\b\\a.dds");
        assert_eq!(texture_path("clutter\\a.dds"), "textures\\clutter\\a.dds");
        // A doubled separator is one (#1705): mod meshes name such paths.
        assert_eq!(texture_path("textures\\\\weapons\\1HandPistol\\a.dds"), "textures\\weapons\\1handpistol\\a.dds");
        assert_eq!(normalize("\\\\Meshes//Clutter\\/X.NIF"), "meshes\\clutter\\x.nif");
        assert_eq!(texture_path("data\\\\textures\\\\b\\a.dds"), "textures\\b\\a.dds");
    }

    #[test]
    fn the_normal_map_beside_a_texture_has_n_before_the_extension() {
        assert_eq!(
            normal_map_beside("textures\\characters\\ghoul\\upperbodymale.dds").as_deref(),
            Some("textures\\characters\\ghoul\\upperbodymale_n.dds")
        );
        // The path is kept as written (case, separators, no `textures\` in front).
        assert_eq!(normal_map_beside("Characters/RaiderFemale/HandFemale.DDS").as_deref(), Some("Characters/RaiderFemale/HandFemale_n.DDS"));
        assert_eq!(normal_map_beside("headhuman.dds").as_deref(), Some("headhuman_n.dds"));
        // Only the last dot of the file name counts, and dots in folder names do not.
        assert_eq!(normal_map_beside("a.b\\head.old.dds").as_deref(), Some("a.b\\head.old_n.dds"));
        assert_eq!(normal_map_beside("textures\\characters\\facemods\\falloutnv.esm\\x"), None);
        // No file name or no extension: no sibling.
        assert_eq!(normal_map_beside(""), None);
        assert_eq!(normal_map_beside("textures\\characters\\male\\"), None);
        assert_eq!(normal_map_beside("textures\\characters\\male\\.dds"), None);
        assert_eq!(normal_map_beside("textures\\characters\\male\\headhuman."), None);
        assert_eq!(normal_map_beside("textures\\characters\\male\\headhuman"), None);
    }

    /// A folder added after the mount opened lies over Data's loose files and is listed last.
    #[test]
    fn an_added_loose_root_lies_over_every_other() {
        let base = std::env::temp_dir().join(format!("nvf-vfs-added-root-{}", std::process::id()));
        let (data, extra) = (base.join("Data"), base.join("extra"));
        std::fs::create_dir_all(data.join("textures/a")).unwrap();
        std::fs::create_dir_all(extra.join("textures/a")).unwrap();
        std::fs::write(data.join("textures/a/both.dds"), b"data").unwrap();
        std::fs::write(data.join("textures/a/data_only.dds"), b"data").unwrap();
        std::fs::write(extra.join("textures/a/both.dds"), b"extra").unwrap();
        std::fs::write(extra.join("textures/a/extra_only.dds"), b"extra").unwrap();
        let mut vfs = Vfs::open(&data, &[]).unwrap();
        assert!(!vfs.exists("textures\\a\\extra_only.dds"));
        vfs.add_loose_root(&extra);
        assert_eq!(vfs.read("textures\\a\\both.dds").unwrap().unwrap(), b"extra");
        assert_eq!(vfs.read("textures\\a\\data_only.dds").unwrap().unwrap(), b"data");
        assert_eq!(vfs.read("textures\\a\\extra_only.dds").unwrap().unwrap(), b"extra");
        assert_eq!(vfs.loose_roots().last(), Some(&extra));
        // A folder listing takes every layer's files from the index, and an added root
        // drops the sorted keys made for an earlier listing (#4490).
        let listed = vfs.loose_paths_under("Textures/A");
        assert_eq!(listed, ["textures\\a\\both.dds", "textures\\a\\data_only.dds", "textures\\a\\extra_only.dds"]);
        let later = base.join("later");
        std::fs::create_dir_all(later.join("textures/a")).unwrap();
        std::fs::write(later.join("textures/a/later.dds"), b"later").unwrap();
        vfs.add_loose_root(&later);
        assert_eq!(vfs.loose_paths_under("textures\\a\\").len(), 4);
        assert!(vfs.loose_paths_under("textures\\b").is_empty());
        // Nothing was written into either folder.
        assert_eq!(std::fs::read(data.join("textures/a/both.dds")).unwrap(), b"data");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn loose_files_and_order() {
        let dir = std::env::temp_dir().join(format!("nvf-vfs-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Meshes/Sub")).unwrap();
        std::fs::write(dir.join("Meshes/Sub/A.nif"), b"loose").unwrap();
        for n in ["Fallout - Meshes.bsa", "Update.bsa", "DeadMoney - Main.bsa", "DeadMoney - Sounds.bsa", "Other.bsa"] {
            std::fs::write(dir.join(n), b"").unwrap();
        }
        let order = Vfs::archive_order(&dir, &DEFAULT_ARCHIVE_LIST, &["FalloutNV.esm", "DeadMoney.esm"]);
        assert_eq!(order, ["Fallout - Meshes.bsa", "Update.bsa", "DeadMoney - Main.bsa", "DeadMoney - Sounds.bsa"]);
        let vfs = Vfs::open(&dir, &[]).unwrap();
        assert!(vfs.exists("meshes/sub/a.NIF"));
        assert_eq!(vfs.source_of("MESHES\\SUB\\A.nif"), Some("loose"));
        assert_eq!(vfs.read("meshes\\sub\\a.nif").unwrap().unwrap(), b"loose");
        assert!(!vfs.exists("fallout - meshes.bsa"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A broken archive (here an empty file) is skipped, not fatal here; loose files still load.
    #[test]
    fn a_bad_archive_is_skipped() {
        let dir = std::env::temp_dir().join(format!("nvf-vfs-bad-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Meshes")).unwrap();
        std::fs::write(dir.join("Meshes/A.nif"), b"loose").unwrap();
        std::fs::write(dir.join("Broken.bsa"), b"").unwrap();
        let vfs = Vfs::open(&dir, &["Broken.bsa".to_string(), "Missing.bsa".to_string()]).unwrap();
        assert_eq!(vfs.archives().count(), 0);
        assert_eq!(vfs.skipped().len(), 2);
        // Both are named for the start-up check, with why.
        let broken: Vec<&str> = vfs.broken_archives().iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(broken, ["Broken.bsa", "Missing.bsa"]);
        assert!(vfs.broken_archives()[0].1.contains("0 bytes"), "{:?}", vfs.broken_archives());
        assert!(vfs.exists("meshes/a.nif"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Files read through the VFS from an archive (compressed or not, names embedded or
    /// not) decode to the bytes stored, also when many threads read one archive at once.
    #[test]
    fn archive_reads_decode_the_same_bytes_from_many_threads() {
        use fnv_bsa::{encode_block, flags, write_archive, WriteFile, WriteFolder, SIZE_COMPRESSION_TOGGLE};
        let dir = std::env::temp_dir().join(format!("nvf-vfs-bsa-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let contents: Vec<Vec<u8>> = (0..24u32).map(|i| (0..(i * 997 + 3)).map(|j| (j * 31 + i) as u8).collect()).collect();
        for (n, embed) in [("Plain.bsa", false), ("Named.bsa", true)] {
            let files = contents
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let name = format!("f{i}.nif").into_bytes();
                    let path = format!("meshes\\t\\f{i}.nif").into_bytes();
                    // Every other file compressed: the toggle against an uncompressed default.
                    let compress = i % 2 == 0;
                    WriteFile {
                        name,
                        size_flag_bits: if compress { SIZE_COMPRESSION_TOGGLE } else { 0 },
                        data: encode_block(c, compress, embed.then_some(&path[..])),
                    }
                })
                .collect();
            let mut bytes = Vec::new();
            let archive_flags = flags::DIR_NAMES | flags::FILE_NAMES | if embed { flags::EMBED_FILE_NAMES } else { 0 };
            write_archive(&mut bytes, archive_flags, 0x1, 0, &[WriteFolder { name: b"meshes\\t".to_vec(), files }]).unwrap();
            std::fs::write(dir.join(n), &bytes).unwrap();
        }
        for n in ["Plain.bsa", "Named.bsa"] {
            let vfs = Vfs::open(&dir, &[n.to_string()]).unwrap();
            assert_eq!(vfs.source_of("meshes\\t\\f0.nif"), Some(n));
            std::thread::scope(|s| {
                for t in 0..8 {
                    let (vfs, contents) = (&vfs, &contents);
                    s.spawn(move || {
                        for k in 0..contents.len() * 4 {
                            let i = (k * 7 + t) % contents.len();
                            assert_eq!(vfs.read(&format!("meshes/t/f{i}.nif")).unwrap().unwrap(), contents[i], "{n} f{i}");
                        }
                    });
                }
            });
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A loose file hides an archive's copy from `read` but not from `read_archived`,
    /// which reads the highest archive's (#975); a path no archive has reads nothing there.
    #[test]
    fn the_archived_copy_under_a_loose_file_still_reads() {
        use fnv_bsa::{encode_block, flags, write_archive, WriteFile, WriteFolder};
        let dir = std::env::temp_dir().join(format!("nvf-vfs-under-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Menus/Main")).unwrap();
        std::fs::write(dir.join("Menus/Main/HUD_Main_Menu.xml"), b"loose").unwrap();
        std::fs::write(dir.join("Menus/Main/new.xml"), b"new").unwrap();
        for (n, text) in [("Low.bsa", b"low"), ("High.bsa", b"top")] {
            let file = |name: &str, data: &[u8]| WriteFile { name: name.as_bytes().to_vec(), size_flag_bits: 0, data: encode_block(data, false, None) };
            let files = vec![file("hud_main_menu.xml", text), file("only.xml", text)];
            let mut bytes = Vec::new();
            write_archive(&mut bytes, flags::DIR_NAMES | flags::FILE_NAMES, 0x1, 0, &[WriteFolder { name: b"menus\\main".to_vec(), files }]).unwrap();
            std::fs::write(dir.join(n), &bytes).unwrap();
        }
        let vfs = Vfs::open(&dir, &["Low.bsa".to_string(), "High.bsa".to_string()]).unwrap();
        assert_eq!(vfs.source_of("menus\\main\\hud_main_menu.xml"), Some("loose"));
        assert_eq!(vfs.read("menus/main/hud_main_menu.xml").unwrap().unwrap(), b"loose");
        assert_eq!(vfs.read_archived("Menus/Main/HUD_Main_Menu.xml").unwrap().unwrap(), b"top");
        // Not loose: the archived copy is the one that wins.
        assert_eq!(vfs.read_archived("menus\\main\\only.xml").unwrap().unwrap(), b"top");
        assert!(vfs.read_archived("menus\\main\\new.xml").is_none());
        assert!(vfs.read_archived("menus\\main\\absent.xml").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mod_archives_follow_their_plugins() {
        let dir = std::env::temp_dir().join(format!("nvf-vfs-mods-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["Fallout - Meshes.bsa", "Update.bsa", "DeadMoney - Main.bsa", "MyMod.bsa", "MyMod - Textures.bsa", "Unused.bsa"] {
            std::fs::write(dir.join(n), b"").unwrap();
        }
        // Base archives, then each plugin's own archives in load order; an archive no
        // loaded plugin names stays out.
        let order = Vfs::archive_order(&dir, &DEFAULT_ARCHIVE_LIST, &["FalloutNV.esm", "DeadMoney.esm", "MyMod.esp"]);
        assert_eq!(order, ["Fallout - Meshes.bsa", "Update.bsa", "DeadMoney - Main.bsa", "MyMod - Textures.bsa", "MyMod.bsa"]);
        // Without the plugin its archives are not read.
        let order = Vfs::archive_order(&dir, &DEFAULT_ARCHIVE_LIST, &DEFAULT_PLUGINS);
        assert!(!order.iter().any(|n| n.starts_with("MyMod")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A plugin in NVForever's mods folder brings the archives beside it, named by their
    /// full path; one in both folders is the mods folder's copy.
    #[test]
    fn mods_folder_archives_follow_their_plugins() {
        let root = std::env::temp_dir().join(format!("nvf-vfs-modsdir-{}", std::process::id()));
        let (data, mods) = (root.join("Data"), root.join("mods"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&mods).unwrap();
        for n in ["Fallout - Meshes.bsa", "Update.bsa", "Both.bsa"] {
            std::fs::write(data.join(n), b"").unwrap();
        }
        for n in ["Edits.bsa", "Edits - Textures.bsa", "Both.bsa", "Update.bsa", "Stray.bsa"] {
            std::fs::write(mods.join(n), b"").unwrap();
        }
        let full = |n: &str| mods.join(n).to_string_lossy().into_owned();
        let order = Vfs::archive_order_in(&data, Some(&mods), &DEFAULT_ARCHIVE_LIST, &["FalloutNV.esm", "Both.esp", "Edits.esp"]);
        // The base list and Update.bsa only from Data; no plugin claims Stray.bsa.
        assert_eq!(order, ["Fallout - Meshes.bsa".to_string(), "Update.bsa".into(), full("Both.bsa"), full("Edits - Textures.bsa"), full("Edits.bsa")]);
        // Without a mods folder, as before.
        assert_eq!(Vfs::archive_order_in(&data, None, &DEFAULT_ARCHIVE_LIST, &["FalloutNV.esm", "Both.esp", "Edits.esp"]), ["Fallout - Meshes.bsa", "Update.bsa", "Both.bsa"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A mod's loose files are read from the mod folder, including a nested folder.
    /// The base game's loose file is still read from Data. A mod does not replace it
    /// unless the mod folder has that path.
    #[test]
    fn launcher_mod_loose_files_come_from_the_mod_folder() {
        let root = std::env::temp_dir().join(format!("nvf-vfs-launcher-mods-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (data, mods) = (root.join("Data"), root.join("mods"));
        std::fs::create_dir_all(data.join("meshes")).unwrap();
        std::fs::create_dir_all(mods.join("Cool").join("meshes")).unwrap();
        std::fs::create_dir_all(mods.join("Cool").join("deep").join("meshes")).unwrap();
        std::fs::write(data.join("meshes").join("a.nif"), b"base").unwrap();
        std::fs::write(data.join("meshes").join("only-data.nif"), b"base").unwrap();
        std::fs::write(mods.join("Cool").join("meshes").join("a.nif"), b"mod").unwrap();
        std::fs::write(mods.join("Cool").join("deep").join("meshes").join("b.nif"), b"deep").unwrap();
        let layers = vec![mods.join("Cool")];
        let vfs = Vfs::open_launcher_mods(&data, &mods, &layers, &[] as &[&str]).unwrap();
        assert_eq!(vfs.read("meshes/a.nif").unwrap().unwrap(), b"mod");
        assert_eq!(vfs.read("meshes/only-data.nif").unwrap().unwrap(), b"base");
        assert_eq!(vfs.read("deep/meshes/b.nif").unwrap().unwrap(), b"deep");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// An archive loose at the top of the launcher's mods folder is claimed as the base game
    /// claims one in Data: `<stem>.bsa`, `<stem> - *.bsa`, the numbered pieces, the longest
    /// stem winning, in plugin load order. `A.esp` does not claim `Armor.bsa`.
    #[test]
    fn a_loose_archive_in_the_mods_folder_follows_the_base_game_claim() {
        let root = std::env::temp_dir().join(format!("nvf-vfs-loose-claim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (data, mods) = (root.join("Data"), root.join("mods"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(mods.join("ModB")).unwrap();
        for name in ["Armor.bsa", "A.bsa", "A - Textures.bsa", "A2.bsa", "A - Extra.bsa", "Stray.bsa"] {
            std::fs::write(mods.join(name), b"").unwrap();
        }
        std::fs::write(mods.join("ModB").join("B.bsa"), b"").unwrap();
        let layers = vec![mods.join("ModB")];
        let names = |plugins: &[&str]| -> Vec<String> {
            Vfs::order_for_launcher_layers(&data, Some(&mods), &layers, plugins).0.into_iter().map(|(n, _)| n).collect()
        };
        // B loads before A, so B's archive comes first. `A - Extra.bsa` is `A - Extra.esp`'s.
        // Armor.bsa and Stray.bsa belong to no active plugin, so they are not mounted.
        assert_eq!(names(&["FalloutNV.esm", "B.esp", "A.esp", "A - Extra.esp"]), ["B.bsa", "A - Textures.bsa", "A.bsa", "A2.bsa", "A - Extra.bsa"]);
        // A.esp alone: still not Armor.bsa.
        assert_eq!(names(&["A.esp"]), ["A - Extra.bsa", "A - Textures.bsa", "A.bsa", "A2.bsa"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// `Foo - Bar.bsa` matches both `Foo.esp` ("Foo - *") and `Foo - Bar.esp`: it takes
    /// the longer stem's place, after `Foo - Bar.esp`'s other archives load order puts
    /// later (#996).
    #[test]
    fn an_archive_belongs_to_the_longest_plugin_stem() {
        let dir = std::env::temp_dir().join(format!("nvf-vfs-stems-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["Foo.bsa", "Foo - Main.bsa", "Foo - Bar.bsa", "Middle.bsa"] {
            std::fs::write(dir.join(n), b"").unwrap();
        }
        let order = Vfs::archive_order(&dir, &[], &["Foo.esp", "Middle.esp", "Foo - Bar.esp"]);
        assert_eq!(order, ["Foo - Main.bsa", "Foo.bsa", "Middle.bsa", "Foo - Bar.bsa"]);
        // Without the longer plugin, Foo.esp keeps claiming it.
        let order = Vfs::archive_order(&dir, &[], &["Foo.esp", "Middle.esp"]);
        assert_eq!(order, ["Foo - Bar.bsa", "Foo - Main.bsa", "Foo.bsa", "Middle.bsa"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A packer splits a big archive into `<stem>.bsa`, `<stem>1.bsa`, `<stem>2.bsa` …
    /// (the Begin Again list's `Begin Again Worldspace Merge1.bsa` … `6.bsa` hold its rock
    /// textures): every piece is the plugin's, in number order (#2521). A name that only
    /// starts with the stem (`MergeExtra.bsa`) is not, and a numbered piece another plugin
    /// is named for exactly goes with that plugin.
    #[test]
    fn numbered_archive_pieces_follow_their_plugin() {
        let dir = std::env::temp_dir().join(format!("nvf-vfs-numbered-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["Merge.bsa", "Merge10.bsa", "Merge2.bsa", "Merge1.bsa", "Merge1 - Textures.bsa", "MergeExtra.bsa", "Merge3.bsa", "Merge3.esp.bsa"] {
            std::fs::write(dir.join(n), b"").unwrap();
        }
        let order = Vfs::archive_order(&dir, &[], &["Merge.esp"]);
        assert_eq!(order, ["Merge.bsa", "Merge1 - Textures.bsa", "Merge1.bsa", "Merge2.bsa", "Merge3.bsa", "Merge10.bsa"]);
        let order = Vfs::archive_order(&dir, &[], &["Merge.esp", "Merge3.esp"]);
        assert_eq!(order, ["Merge.bsa", "Merge1 - Textures.bsa", "Merge1.bsa", "Merge2.bsa", "Merge10.bsa", "Merge3.bsa"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Space-suffixed plugin archives such as `NS A Content T 00.bsa` follow their plugin
    /// (#3756), while unrelated files sharing a short prefix without a space boundary stay
    /// out, a longer plugin stem wins ownership, and explicit archive-list entries are not
    /// duplicated in either flat or layered discovery.
    #[test]
    fn space_suffixed_archives_follow_their_plugin() {
        let base = std::env::temp_dir().join(format!("nvf-vfs-space-suffix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (data, mod_dir) = (base.join("Data"), base.join("mods").join("NS"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&mod_dir).unwrap();
        let names = [
            "NS.bsa",
            "NS - Textures.bsa",
            "NS A Content T 00.bsa",
            "NS A Content T 01.bsa",
            "NS A Content.bsa",
            "NSExtra.bsa",
            "NS_Other.bsa",
            "NS-Other.bsa",
            "NS.esp.bsa",
            "Middle.bsa",
        ];
        for n in names {
            std::fs::write(data.join(n), b"").unwrap();
        }
        let order = Vfs::archive_order(&data, &[], &["NS.esp", "Middle.esp"]);
        assert_eq!(
            order,
            ["NS - Textures.bsa", "NS A Content T 00.bsa", "NS A Content T 01.bsa", "NS A Content.bsa", "NS.bsa", "Middle.bsa"]
        );
        // Longer plugin stem takes its own archives and space-suffixed children.
        let order = Vfs::archive_order(&data, &[], &["NS.esp", "Middle.esp", "NS A Content.esp"]);
        assert_eq!(
            order,
            ["NS - Textures.bsa", "NS.bsa", "Middle.bsa", "NS A Content T 00.bsa", "NS A Content T 01.bsa", "NS A Content.bsa"]
        );
        // Explicit archive-list entry keeps its list position and is not mounted twice.
        let order = Vfs::archive_order(&data, &["NS A Content T 00.bsa"], &["NS.esp", "Middle.esp"]);
        assert_eq!(
            order,
            ["NS A Content T 00.bsa", "NS - Textures.bsa", "NS A Content T 01.bsa", "NS A Content.bsa", "NS.bsa", "Middle.bsa"]
        );
        // Same non-duplication holds in layered MO2 discovery when the archive lives in a mod root.
        for n in ["NS A Content T 00.bsa", "NS A Content T 01.bsa", "NSExtra.bsa"] {
            std::fs::write(mod_dir.join(n), b"").unwrap();
        }
        let layered_empty_data = base.join("DataEmpty");
        std::fs::create_dir_all(&layered_empty_data).unwrap();
        let layered = Vfs::archive_files_layered(&layered_empty_data, std::slice::from_ref(&mod_dir), &["NS A Content T 00.bsa"], &["NS.esp"]);
        let layered_names: Vec<&str> = layered.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(layered_names, ["NS A Content T 00.bsa", "NS A Content T 01.bsa"]);
        assert_eq!(layered[0].1, mod_dir.join("NS A Content T 00.bsa"));
        // Without NS.esp loaded, none of the NS* archives are auto-mounted.
        let order = Vfs::archive_order(&data, &[], &["Middle.esp"]);
        assert_eq!(order, ["Middle.bsa"]);
        std::fs::remove_dir_all(&base).unwrap();
    }

    fn link_dir(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, link).is_ok();
        #[cfg(windows)]
        return std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success());
        #[cfg(not(any(unix, windows)))]
        return false;
    }

    /// Directory junctions (Windows) and directory symlinks (Unix) inside `Data` or a mods
    /// overlay are followed so loose files inside linked folders (e.g. `textures\pbr`) are
    /// indexed and readable, while cyclic links back to an ancestor terminate without looping.
    #[test]
    fn loose_walk_follows_directory_links_and_stops_on_cycles() {
        let base = std::env::temp_dir().join(format!("nvf-vfs-junctions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let data = base.join("Data");
        let mods = base.join("mods");
        let overlay = base.join("overlay_pbr");
        std::fs::create_dir_all(data.join("textures")).unwrap();
        std::fs::create_dir_all(mods.join("textures")).unwrap();
        std::fs::create_dir_all(overlay.join("landscape")).unwrap();
        std::fs::write(overlay.join("landscape").join("dirt_p.dds"), b"height-bytes").unwrap();

        let pbr_link = mods.join("textures").join("pbr");
        let cycle_link = overlay.join("loop_back");
        let made = link_dir(&overlay, &pbr_link) && link_dir(&mods, &cycle_link);
        #[cfg(any(unix, windows))]
        assert!(made, "directory link creation should succeed on unix and windows");
        if made {
            let vfs = Vfs::open_for_plugins_in(&data, Some(&mods), &[] as &[&str]).unwrap();
            let pbr_paths = vfs.paths(|k| k.starts_with("textures\\pbr\\"));
            assert_eq!(pbr_paths, ["textures\\pbr\\landscape\\dirt_p.dds"]);
            assert!(!vfs.exists("textures\\pbr"), "linked directory must not be indexed as a file");
            assert_eq!(vfs.read("textures\\pbr\\landscape\\dirt_p.dds").unwrap().unwrap(), b"height-bytes");
        }
        let _ = std::fs::remove_dir(&cycle_link);
        let _ = std::fs::remove_dir(&pbr_link);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A dangling directory link is skipped cleanly without registering a phantom file.
    #[test]
    fn loose_walk_skips_dangling_directory_link() {
        let base = std::env::temp_dir().join(format!("nvf-vfs-dangling-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let data = base.join("Data");
        let target = base.join("vanished");
        std::fs::create_dir_all(data.join("textures")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        let link = data.join("textures").join("broken_dir");
        let made = link_dir(&target, &link);
        #[cfg(any(unix, windows))]
        assert!(made, "directory link creation should succeed on unix and windows");
        if made {
            std::fs::remove_dir(&target).unwrap();
            let vfs = Vfs::open(&data, &[]).unwrap();
            assert!(!vfs.exists("textures\\broken_dir"));
            assert!(!vfs.skipped().is_empty(), "dangling link should be recorded in skipped");
        }
        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod at_most_tests;
#[cfg(test)]
mod own_loose_tests;
