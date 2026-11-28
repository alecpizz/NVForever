//! The updater (#2713, docs/design/launcher.md "Updater"): fetch the newest release's archive
//! for this system, check it against the release's `SHA256SUMS.txt`, unpack it in the home's
//! cache folder and swap the program files of the folder the launcher runs from, all or
//! nothing, keeping the replaced files to roll back. The home (saves, `NVForever.ini`,
//! `plugins.txt`, mods) is never touched: it is not in the archive and not in the install.
//!
//! The steps that decide (which release, which archive, which checksum, where to stage, what
//! to move and copy) are pure functions with tests; the ones that act (download, unpack,
//! [`apply`]) are small and tested on fixture folders.

use crate::news::{self, Latest};
use fnv_install::{guard_write, GameInstall};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The checksum list every release carries (`.github/workflows/release.yml`, `sha256sum *`).
pub const CHECKSUMS: &str = "SHA256SUMS.txt";
/// The folder of the install the updater keeps its own files in: `installed.json` and
/// `previous/v<version>/` (the files the last update replaced).
pub const UPDATE_DIR: &str = "update";
const MANIFEST: &str = "installed.json";
const PREVIOUS: &str = "previous";
/// The suffix of a file being copied in, renamed over its place once whole.
const NEW_SUFFIX: &str = ".nvf-new";

pub fn launcher_exe() -> String {
    format!("nvforever-launcher{}", std::env::consts::EXE_SUFFIX)
}

/// The archive's programs that must not be running while they are replaced (the launcher
/// itself is renamed aside while it runs, which every system allows).
pub fn programs() -> [String; 3] {
    let s = std::env::consts::EXE_SUFFIX;
    [format!("nvforever{s}"), format!("nvf-viewer{s}"), format!("fnvtool{s}")]
}

/// A release's file, as GitHub's release list gives it.
pub use crate::news::Asset;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
}

/// This build's release target, as the release workflow names its archives; `None` on a
/// system the releases have no archive for (the update button opens the page there).
pub fn target() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("x86_64-pc-windows-msvc")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-gnu")
    } else {
        None
    }
}

/// `NVForever-<tag>-<target>.zip` on Windows, `.tar.gz` elsewhere (release.yml's Package step).
pub fn archive_name(tag: &str, target: &str) -> (String, ArchiveKind) {
    if target.contains("windows") {
        (format!("NVForever-{tag}-{target}.zip"), ArchiveKind::Zip)
    } else {
        (format!("NVForever-{tag}-{target}.tar.gz"), ArchiveKind::TarGz)
    }
}

/// The release's archive for `target`, by its exact name.
pub fn choose_asset<'a>(assets: &'a [Asset], tag: &str, target: &str) -> Option<(&'a Asset, ArchiveKind)> {
    let (name, kind) = archive_name(tag, target);
    assets.iter().find(|a| a.name == name).map(|a| (a, kind))
}

/// Where this repository's releases keep their files on GitHub.
pub const REPO_DOWNLOADS: &str = "https://github.com/LootJunkieAzza/NVForever/releases/download/";

/// Whether the launcher's test aids work: in tests and debug builds only, never in a release's
/// launcher (a local release list, a version taken for an older one, plain `http` to this
/// machine).
pub fn test_aids() -> bool {
    cfg!(any(test, debug_assertions))
}

/// Whether the updater may fetch `url` for release `tag`: a file of that very release of this
/// repository (`releases/download/<tag>/`). `SHA256SUMS.txt` comes from the same release, so
/// it only catches damage; the address is what ties the archive to this project. Plain `http`
/// to this machine only with the test aids.
pub fn fetchable(url: &str, tag: &str, name: &str) -> bool {
    let ours = url.strip_prefix(REPO_DOWNLOADS).and_then(|r| r.strip_prefix(tag)).and_then(|r| r.strip_prefix('/')).is_some_and(|file| file == name);
    ours || (test_aids() && (url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:")))
}

pub fn checksums_asset(assets: &[Asset]) -> Option<&Asset> {
    assets.iter().find(|a| a.name == CHECKSUMS)
}

/// The SHA-256 of `name` in a `sha256sum` list (`<64 hex>  <name>`, or `*<name>` for binary
/// mode); `None` when the list does not name it or its hash is not 64 hex digits.
pub fn checksum_for(sums: &str, name: &str) -> Option<[u8; 32]> {
    sums.lines().find_map(|line| {
        let (hash, rest) = line.trim().split_once(char::is_whitespace)?;
        let file = rest.trim_start();
        let file = file.strip_prefix('*').unwrap_or(file);
        (file == name).then(|| parse_hex32(hash)).flatten()
    })
}

fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `s` (a release tag, a version) may name a folder: letters, digits, `.`, `_`, `-`;
/// not empty and not `.`/`..`. A tag comes from the network, so it is checked before any
/// path is made from it.
pub fn safe_component(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// `p` as a path below a folder: only plain names (`.` dropped); `None` for an absolute
/// path, a drive, `..` or nothing at all (an archive entry that would land outside).
pub fn safe_rel(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// The newest release's download folder: `<cache>/updates/<tag>/`, holding the archive
/// (`<name>.part` while it arrives) and `unpacked/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staging {
    pub dir: PathBuf,
}

pub fn updates_dir(cache: &Path) -> PathBuf {
    cache.join("updates")
}

impl Staging {
    pub fn new(cache: &Path, tag: &str) -> Option<Staging> {
        safe_component(tag).then(|| Staging { dir: updates_dir(cache).join(tag) })
    }
    pub fn part(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.part"))
    }
    pub fn archive(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
    pub fn unpacked(&self) -> PathBuf {
        self.dir.join("unpacked")
    }
}

/// Staging folders of releases no newer than `current` (left by an update that has been
/// installed, or a download given up on), to remove at start.
pub fn stale_staging(cache: &Path, current: &str) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(updates_dir(cache)) else { return Vec::new() };
    rd.flatten().filter(|e| !news::is_newer(&e.file_name().to_string_lossy(), current)).map(|e| e.path()).collect()
}

/// Whether the launcher in `dir` runs from a release folder the updater may change: not a
/// build of a source checkout (that one updates with git), the game program beside it,
/// and not inside the game's folder (`game`, the one the INI names) or the original game's
/// user folder ([`guard_write`]).
pub fn check_install(dir: &Path, game: Option<&GameInstall>) -> Result<(), String> {
    if game.is_some_and(|g| g.contains(dir)) {
        return Err("NVForever is unpacked inside the game folder, which it never writes to: move it to a folder of its own to update it.".into());
    }
    if dir.ancestors().any(|a| a.join("engine").join("Cargo.toml").is_file()) {
        return Err("This launcher is a build of the source code: update it with git and cargo.".into());
    }
    if !dir.join(&programs()[0]).is_file() {
        return Err(format!("{} is not beside the launcher, so this is not a release folder the updater can change.", programs()[0]));
    }
    guard_write(&dir.join(launcher_exe()), game).map_err(|e| e.to_string())
}

// ---- Downloading ----

pub fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(h.finalize().into());
        }
        h.update(&buf[..n]);
    }
}

/// Copy `from` into `dest`, hashing as it goes, at most `limit` bytes; `on_bytes` hears the
/// count so far. Stops with an error when `cancel` is set.
pub fn copy_hashed(mut from: impl Read, dest: &Path, limit: u64, mut on_bytes: impl FnMut(u64), cancel: &AtomicBool) -> Result<[u8; 32], String> {
    let mut out = fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let n = from.read(&mut buf).map_err(|e| format!("download: {e}"))?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > limit {
            return Err("the download is larger than the release says".into());
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", dest.display()))?;
        on_bytes(done);
    }
    out.sync_all().map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(h.finalize().into())
}

// ---- Unpacking ----

/// Unpack `archive` into `dest` (emptied first) and give the folder holding the programs:
/// the archive's one top folder (`NVForever-<tag>-<target>/`), else `dest`. Entries that
/// would land outside `dest`, links and devices are refused; the result must hold the game
/// program (the launcher too from v0.8.0 on; an archive without one leaves the running
/// launcher in place, and it restarts as itself).
pub fn unpack(archive: &Path, kind: ArchiveKind, dest: &Path) -> Result<PathBuf, String> {
    if dest.exists() {
        fs::remove_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    match kind {
        ArchiveKind::Zip => unzip(archive, dest),
        ArchiveKind::TarGz => untar(archive, dest),
    }?;
    let root = payload_root(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let need = &programs()[0];
    if !root.join(need).is_file() {
        return Err(format!("the archive has no {need}: not an NVForever release for this system"));
    }
    Ok(root)
}

fn unzip(archive: &Path, dest: &Path) -> Result<(), String> {
    let bad = |e: &dyn std::fmt::Display| format!("{}: {e}", archive.display());
    let f = fs::File::open(archive).map_err(|e| bad(&e))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| bad(&e))?;
    for i in 0..z.len() {
        let mut entry = z.by_index(i).map_err(|e| bad(&e))?;
        let name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
        let rel = entry.enclosed_name().and_then(|p| safe_rel(&p)).ok_or_else(|| bad(&format!("entry {name} would land outside the folder")))?;
        if entry.is_symlink() {
            return Err(bad(&format!("entry {name} is a link")));
        }
        let to = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&to).map_err(|e| format!("{}: {e}", to.display()))?;
            continue;
        }
        if let Some(p) = to.parent() {
            fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        let mut out = fs::File::create(&to).map_err(|e| format!("{}: {e}", to.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| bad(&e))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&to, fs::Permissions::from_mode(mode & 0o777));
        }
    }
    Ok(())
}

fn untar(archive: &Path, dest: &Path) -> Result<(), String> {
    let bad = |e: &dyn std::fmt::Display| format!("{}: {e}", archive.display());
    let f = fs::File::open(archive).map_err(|e| bad(&e))?;
    let mut t = tar::Archive::new(flate2::read::GzDecoder::new(f));
    for entry in t.entries().map_err(|e| bad(&e))? {
        let mut entry = entry.map_err(|e| bad(&e))?;
        let path = entry.path().map_err(|e| bad(&e))?.into_owned();
        let rel = safe_rel(&path).ok_or_else(|| bad(&format!("entry {} would land outside the folder", path.display())))?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(bad(&format!("entry {} is not a file or folder", path.display())));
        }
        let to = dest.join(&rel);
        if let Some(p) = to.parent() {
            fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        entry.unpack(&to).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(())
}

/// The archive's one top folder, else `dest` itself.
pub fn payload_root(dest: &Path) -> std::io::Result<PathBuf> {
    let entries: Vec<_> = fs::read_dir(dest)?.flatten().collect();
    match entries.as_slice() {
        [one] if one.file_type()?.is_dir() => Ok(one.path()),
        _ => Ok(dest.to_path_buf()),
    }
}

/// Every file below `root`, as relative paths, sorted.
pub fn list_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for e in fs::read_dir(dir)? {
            let e = e?;
            let t = e.file_type()?;
            if t.is_dir() {
                walk(root, &e.path(), out)?;
            } else if t.is_file() {
                out.push(e.path().strip_prefix(root).map_err(std::io::Error::other)?.to_path_buf());
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

// ---- The swap ----

/// `<install>/update/installed.json`: the version an update installed and its files, so the
/// next update also sets aside the files a release no longer has (never a file the player
/// put in the folder: only the ones listed).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    /// `/`-separated, relative to the install.
    pub files: Vec<String>,
}

pub fn manifest_path(install: &Path) -> PathBuf {
    install.join(UPDATE_DIR).join(MANIFEST)
}

pub fn read_manifest(install: &Path) -> Option<Manifest> {
    fs::read_to_string(manifest_path(install)).ok().and_then(|t| serde_json::from_str(&t).ok())
}

pub fn previous_dir(install: &Path) -> PathBuf {
    install.join(UPDATE_DIR).join(PREVIOUS)
}

fn slash(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// The kept previous version a roll back would put back: its version and folder, when it
/// holds the game program (an empty folder is no version to go back to).
pub fn previous_version(install: &Path) -> Option<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = fs::read_dir(previous_dir(install))
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let v = name.strip_prefix('v')?.to_string();
            let p = e.path();
            (safe_component(&v) && p.join(&programs()[0]).is_file()).then_some((v, p))
        })
        .collect();
    found.sort_by(|a, b| news::cmp_versions(&b.0, &a.0));
    found.into_iter().next()
}

/// What a swap does, decided before anything moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The version the swap installs (written to the manifest).
    pub version: String,
    /// `<install>/update/previous/v<version being replaced>`: where the files set aside go.
    pub backup: PathBuf,
    /// Moved into `backup` first: the install's files the payload replaces, and the files of
    /// the old manifest the payload no longer has.
    pub aside: Vec<PathBuf>,
    /// Then copied from the payload into the install.
    pub copy: Vec<PathBuf>,
}

/// The swap of `install` (version `old_version`, its manifest `old`) for a payload with
/// `payload_files` (version `new_version`); `exists` says whether an install path is there.
/// Refused when a payload path would land outside the install or in the updater's own
/// `update/` folder.
pub fn plan(install: &Path, payload_files: &[PathBuf], exists: impl Fn(&Path) -> bool, old: Option<&Manifest>, old_version: &str, new_version: &str) -> Result<Plan, String> {
    if !safe_component(old_version) || !safe_component(new_version) {
        return Err(format!("unusable version {old_version:?} or {new_version:?}"));
    }
    let ours = |rel: &Path| rel.components().next().is_some_and(|c| c.as_os_str().eq_ignore_ascii_case(UPDATE_DIR));
    let mut copy = BTreeSet::new();
    for f in payload_files {
        let rel = safe_rel(f).filter(|r| !ours(r)).ok_or_else(|| format!("the update holds an unusable path {}", f.display()))?;
        copy.insert(rel);
    }
    let mut aside: BTreeSet<PathBuf> = copy.iter().filter(|r| exists(&install.join(r))).cloned().collect();
    for f in old.map_or(&[][..], |m| &m.files[..]) {
        if let Some(rel) = safe_rel(Path::new(f)).filter(|r| !ours(r) && !copy.contains(r)) {
            if exists(&install.join(&rel)) {
                aside.insert(rel);
            }
        }
    }
    Ok(Plan { version: new_version.to_string(), backup: previous_dir(install).join(format!("v{old_version}")), aside: aside.into_iter().collect(), copy: copy.into_iter().collect() })
}

/// The first of the archive's programs in `install` that is running, if any. Windows will
/// not open a running program for writing (sharing violation), Linux neither (ETXTBSY).
pub fn busy_program(install: &Path) -> Option<String> {
    let busy_code = if cfg!(windows) { 32 } else { 26 };
    programs().into_iter().find(|p| {
        let path = install.join(p);
        path.is_file() && matches!(fs::OpenOptions::new().write(true).open(&path), Err(e) if e.raw_os_error() == Some(busy_code))
    })
}

/// The journal of a swap under way (`update/swap.json`): written once every new file is
/// staged, removed once the swap is done; a launcher that finds one at start finishes the
/// swap ([`resume_interrupted`]).
const JOURNAL: &str = "swap.json";
/// The suffix of a running program moved out of the way so its new copy can take its name
/// (Windows lets a running program be renamed, not replaced); removed at the next start.
const OLD_SUFFIX: &str = ".nvf-old";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Journal {
    /// The install folder the journal was written for: a journal found in another folder (a
    /// copied install) is not followed.
    #[serde(default)]
    install: String,
    version: String,
    /// The files staged beside their place as `<file>.nvf-new`, `/`-separated.
    copy: Vec<String>,
    /// The files of the old release the new one lacks, removed (their copies are in the backup).
    retire: Vec<String>,
}

pub fn journal_path(install: &Path) -> PathBuf {
    install.join(UPDATE_DIR).join(JOURNAL)
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn write_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let tmp = with_suffix(path, ".tmp");
    let text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// Every path `plan` writes, through [`guard_write`] with the game folder the INI names: never
/// inside the game folder or the original game's user folder.
fn guard_plan(install: &Path, plan: &Plan, game: Option<&GameInstall>) -> Result<(), String> {
    for p in plan.aside.iter().chain(&plan.copy).map(|r| install.join(r)).chain([plan.backup.clone(), journal_path(install)]) {
        guard_write(&p, game).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Phase 1, which changes nothing the install runs from: the files to be replaced or retired
/// copied into `plan.backup`, each new file copied beside its place as `<file>.nvf-new`, then
/// the journal written. On any failure what was made is removed again; the error says if
/// some of it could not be.
fn stage(install: &Path, payload: &Path, plan: &Plan) -> Result<Journal, String> {
    if plan.backup.exists() {
        fs::remove_dir_all(&plan.backup).map_err(|e| format!("{}: {e}", plan.backup.display()))?;
    }
    let mut staged: Vec<PathBuf> = Vec::new();
    let undo = |staged: &[PathBuf], why: String| -> String {
        let mut left: Vec<String> = staged.iter().filter(|t| fs::remove_file(t).is_err() && t.exists()).map(|t| t.display().to_string()).collect();
        if plan.backup.exists() && fs::remove_dir_all(&plan.backup).is_err() {
            left.push(plan.backup.display().to_string());
        }
        if left.is_empty() {
            format!("{why}. Nothing was changed.")
        } else {
            format!("{why}. The program files were not changed, but these could not be removed: {}.", left.join(", "))
        }
    };
    for rel in &plan.aside {
        let (from, to) = (install.join(rel), plan.backup.join(rel));
        if let Err(e) = to.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::copy(&from, &to)) {
            return Err(undo(&staged, format!("{}: {e}", from.display())));
        }
    }
    for rel in &plan.copy {
        let tmp = with_suffix(&install.join(rel), NEW_SUFFIX);
        let r = tmp.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::copy(payload.join(rel), &tmp));
        if tmp.exists() {
            staged.push(tmp.clone());
        }
        if let Err(e) = r {
            return Err(undo(&staged, format!("{}: {e}", tmp.display())));
        }
    }
    let copy: BTreeSet<&PathBuf> = plan.copy.iter().collect();
    let journal = Journal { install: install.display().to_string(), version: plan.version.clone(), copy: plan.copy.iter().map(|p| slash(p)).collect(), retire: plan.aside.iter().filter(|r| !copy.contains(r)).map(|p| slash(p)).collect() };
    if let Err(e) = write_json(&journal_path(install), &journal) {
        return Err(undo(&staged, format!("{}: {e}", journal_path(install).display())));
    }
    Ok(journal)
}

/// Phase 2, renames only: each staged file takes its place (a running program is first moved to
/// `<file>.nvf-old`), the retired files go, the manifest is written and the journal removed.
/// Every new file is already whole, so a phase 2 cut short (the window closed, the power lost)
/// is finished by the next start ([`resume_interrupted`]) rather than undone.
fn commit(install: &Path, journal: &Journal, game: Option<&GameInstall>) -> Result<(), String> {
    for p in [manifest_path(install), journal_path(install)] {
        guard_write(&p, game).map_err(|e| e.to_string())?;
    }
    for rel in &journal.copy {
        let to = install.join(rel);
        let tmp = with_suffix(&to, NEW_SUFFIX);
        if !tmp.exists() {
            continue; // Already in place (a swap finished at the next start).
        }
        if fs::rename(&tmp, &to).is_err() && to.exists() {
            // A running program cannot be replaced, only renamed: move it aside, then the new
            // copy in. If that second rename fails too (a virus scanner holding the new file),
            // the old one goes back to its name, so the install is never left without it.
            let old = with_suffix(&to, OLD_SUFFIX);
            let _ = fs::remove_file(&old);
            fs::rename(&to, &old).map_err(|e| format!("{}: {e}", to.display()))?;
            if let Err(e) = fs::rename(&tmp, &to) {
                return Err(match fs::rename(&old, &to) {
                    Ok(()) => format!("{}: {e}", to.display()),
                    Err(back) => format!("{}: {e}; the old one could not be put back from {} either: {back}", to.display(), old.display()),
                });
            }
        } else if tmp.exists() {
            fs::rename(&tmp, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    for rel in &journal.retire {
        let p = install.join(rel);
        if p.exists() {
            fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        }
    }
    let manifest = Manifest { version: journal.version.clone(), files: journal.copy.clone() };
    write_json(&manifest_path(install), &manifest).map_err(|e| format!("{}: {e}", manifest_path(install).display()))?;
    fs::remove_file(journal_path(install)).map_err(|e| format!("{}: {e}", journal_path(install).display()))
}

/// Carry out `plan` on `install` from `payload`: [`stage`] (nothing the install runs from
/// changes; a failure leaves it as it was), then [`commit`] (renames only). Afterwards only
/// `plan.backup` is kept under `update/previous/`. Nothing is written inside the game folder
/// (`game`, the one the INI names) or the original game's user folder.
pub fn apply(install: &Path, payload: &Path, plan: &Plan, game: Option<&GameInstall>) -> Result<(), String> {
    guard_plan(install, plan, game)?;
    let journal = stage(install, payload, plan)?;
    commit(install, &journal, game).map_err(|e| format!("the update stopped part-way ({e}); start the launcher again and it finishes it"))?;
    // One previous version is kept: the one just set aside (a roll back's source goes too).
    if let Ok(rd) = fs::read_dir(previous_dir(install)) {
        for e in rd.flatten().filter(|e| e.path() != plan.backup) {
            let _ = fs::remove_dir_all(e.path());
        }
    }
    Ok(())
}

/// The journal of a swap cut short, read and checked: every path a plain one below the
/// install, outside `update/`, and writable by [`guard_write`] with the game folder `game`.
/// `None` without a journal; an error for one that cannot be read or trusted.
fn pending_journal(install: &Path, game: Option<&GameInstall>) -> Option<Result<Journal, String>> {
    let path = journal_path(install);
    let text = fs::read_to_string(&path).ok()?;
    let checked = serde_json::from_str::<Journal>(&text).map_err(|e| format!("{}: {e}", path.display())).and_then(|j| {
        if !safe_component(&j.version) {
            return Err(format!("{}: unusable version {:?}", path.display(), j.version));
        }
        if !same_folder(Path::new(&j.install), install) {
            return Err(format!("{}: written for another folder ({})", path.display(), j.install));
        }
        for (i, f) in j.copy.iter().chain(&j.retire).enumerate() {
            let rel = safe_rel(Path::new(f)).filter(|r| !r.components().next().is_some_and(|c| c.as_os_str().eq_ignore_ascii_case(UPDATE_DIR)));
            let rel = rel.ok_or_else(|| format!("{}: unusable path {f:?}", path.display()))?;
            let place = install.join(&rel);
            guard_write(&place, game).map_err(|e| e.to_string())?;
            // A file to put in place must be staged or already in place.
            if i < j.copy.len() && !place.exists() && !with_suffix(&place, NEW_SUFFIX).exists() {
                return Err(format!("{}: {f} is neither staged nor in place", path.display()));
            }
        }
        Ok(j)
    });
    Some(checked)
}

/// Whether two paths name one folder (the same once made absolute and resolved; else the same
/// text, without case on Windows).
fn same_folder(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => {
            let (x, y) = (a.to_string_lossy(), b.to_string_lossy());
            if cfg!(windows) {
                x.eq_ignore_ascii_case(&y)
            } else {
                x == y
            }
        }
    }
}

/// A swap cut short is finished before anything else changes the install: the start does it,
/// and an install or a roll back does it first, refusing to go on when it cannot (a new swap
/// would stage over the kept previous version with half-swapped files). The version it
/// finished, if it finished one: the install then holds that release, not the running one, so
/// the caller stops and the launcher restarts into it.
fn finish_pending(install: &Path, game: Option<&GameInstall>) -> Result<Option<String>, String> {
    match pending_journal(install, game) {
        None => Ok(None),
        Some(Ok(j)) => commit(install, &j, game)
            .map(|()| Some(j.version.clone()))
            .map_err(|e| format!("an earlier update to v{} is still unfinished and could not be finished ({e}); try again, or download the release from its page", j.version)),
        Some(Err(e)) => Err(format!("an earlier update left a journal that cannot be used ({e}); download the release from its page")),
    }
}

/// At the launcher's start, in the folder it runs from (`install`, whether or not
/// [`check_install`] passes: a swap cut short may have left it without `nvforever`): a swap
/// whose journal is still there was cut short in its renames; it is finished (every new file is
/// whole), and the version it installed is returned. With no journal, in a release folder,
/// the leftovers of a swap cut short before its renames (`*.nvf-new`) and the old running
/// program moved aside (`*.nvf-old`) are removed.
pub fn resume_interrupted(install: &Path, game: Option<&GameInstall>) -> Option<Result<String, String>> {
    match pending_journal(install, game) {
        Some(Ok(j)) => Some(commit(install, &j, game).map(|()| j.version.clone())),
        Some(Err(e)) => Some(Err(e)),
        None if check_install(install, game).is_err() => None,
        None => {
            for f in list_files(install).unwrap_or_default() {
                let name = f.to_string_lossy();
                if f.components().next().is_some_and(|c| c.as_os_str() == UPDATE_DIR) {
                    continue;
                }
                if name.ends_with(NEW_SUFFIX) || name.ends_with(OLD_SUFFIX) {
                    let _ = fs::remove_file(install.join(&f));
                }
            }
            None
        }
    }
}

// ---- The whole update, for the launcher's worker thread ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Downloading,
    Verifying,
    Unpacking,
    Installing,
}

impl Phase {
    pub fn words(self) -> &'static str {
        match self {
            Phase::Checking => "Checking the release…",
            Phase::Downloading => "Downloading…",
            Phase::Verifying => "Checking the download…",
            Phase::Unpacking => "Unpacking…",
            Phase::Installing => "Installing…",
        }
    }
}

pub enum Msg {
    Phase(Phase),
    Bytes(u64, u64),
    Ready(Result<Ready, String>),
    Installed(Result<String, String>),
}

/// A downloaded, checked and unpacked release, waiting to be swapped in.
#[derive(Clone, Debug)]
pub struct Ready {
    pub version: String,
    pub payload: PathBuf,
    pub files: Vec<PathBuf>,
    pub staging: PathBuf,
}

/// Download (unless a checked copy is already staged), check and unpack `latest` into the
/// cache. Nothing in the install changes.
pub fn prepare(latest: &Latest, cache: &Path, game: Option<&GameInstall>, send: &dyn Fn(Msg), cancel: &AtomicBool) -> Result<Ready, String> {
    send(Msg::Phase(Phase::Checking));
    let target = target().ok_or("the releases have no archive for this system")?;
    let (asset, kind) = choose_asset(&latest.assets, &latest.tag, target).ok_or_else(|| format!("v{} has no archive for {target}", latest.version))?;
    let sums_asset = checksums_asset(&latest.assets).ok_or_else(|| format!("v{} publishes no {CHECKSUMS}, so its download cannot be checked", latest.version))?;
    for (url, name) in [(&asset.browser_download_url, &asset.name), (&sums_asset.browser_download_url, &sums_asset.name)] {
        if !fetchable(url, &latest.tag, name) {
            return Err(format!("the release names a download address outside {REPO_DOWNLOADS}{}/: {url}", latest.tag));
        }
    }
    let sums = news::get_text(&news::agent(), &sums_asset.browser_download_url)?;
    let want = checksum_for(&sums, &asset.name).ok_or_else(|| format!("{CHECKSUMS} does not list {}", asset.name))?;
    let staging = Staging::new(cache, &latest.tag).ok_or_else(|| format!("unusable release tag {:?}", latest.tag))?;
    guard_write(&staging.dir, game).map_err(|e| e.to_string())?;
    fs::create_dir_all(&staging.dir).map_err(|e| format!("{}: {e}", staging.dir.display()))?;
    let archive = staging.archive(&asset.name);
    let have = archive.is_file().then(|| sha256_file(&archive).ok()).flatten();
    if have != Some(want) {
        send(Msg::Phase(Phase::Downloading));
        let part = staging.part(&asset.name);
        let total = asset.size;
        // A little over the listed size, for a release whose size GitHub leaves out (0).
        let limit = if total > 0 { total } else { 4 << 30 };
        let mut resp = news::download_agent().get(&asset.browser_download_url).call().map_err(|e| format!("{}: {e}", asset.name))?;
        let reader = resp.body_mut().with_config().limit(limit).reader();
        let got = copy_hashed(reader, &part, limit, |n| send(Msg::Bytes(n, total)), cancel);
        send(Msg::Phase(Phase::Verifying));
        let got = got.inspect_err(|_| {
            let _ = fs::remove_file(&part);
        })?;
        if got != want {
            let _ = fs::remove_file(&part);
            return Err(format!("the download of {} does not match its checksum (got {}, {CHECKSUMS} says {}): nothing was changed", asset.name, hex(&got), hex(&want)));
        }
        fs::rename(&part, &archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    }
    send(Msg::Phase(Phase::Unpacking));
    let payload = unpack(&archive, kind, &staging.unpacked())?;
    let files = list_files(&payload).map_err(|e| format!("{}: {e}", payload.display()))?;
    Ok(Ready { version: latest.version.clone(), payload, files, staging: staging.dir })
}

/// Swap `ready` into `install` (running version `current`); the staged copy is removed after.
/// When an earlier swap was still pending, finishing it is all this does: the install then
/// holds that release (not `current`), so a new swap would keep the wrong one as the previous
/// version; its version is returned, the launcher restarts into it and offers the newer one
/// again.
pub fn install(install: &Path, ready: &Ready, current: &str, game: Option<&GameInstall>) -> Result<String, String> {
    if let Some(finished) = finish_pending(install, game)? {
        return Ok(finished);
    }
    check_install(install, game)?;
    if let Some(p) = busy_program(install) {
        return Err(format!("{p} is running: close it, then install."));
    }
    let exists = |p: &Path| p.exists();
    let plan = plan(install, &ready.files, exists, read_manifest(install).as_ref(), current, &ready.version)?;
    apply(install, &ready.payload, &plan, game)?;
    let _ = fs::remove_dir_all(&ready.staging);
    Ok(ready.version.clone())
}

/// Put the kept previous version back (the current one is kept in its place, so a second
/// roll back goes forward again).
pub fn roll_back(install: &Path, current: &str, game: Option<&GameInstall>) -> Result<String, String> {
    if let Some(finished) = finish_pending(install, game)? {
        return Err(format!("an interrupted update to v{finished} was just finished: start the launcher again, then roll back"));
    }
    check_install(install, game)?;
    if let Some(p) = busy_program(install) {
        return Err(format!("{p} is running: close it, then roll back."));
    }
    let (version, dir) = previous_version(install).ok_or("no previous version is kept")?;
    if version == current {
        return Err(format!("the kept version is this one (v{current})"));
    }
    let files = list_files(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let exists = |p: &Path| p.exists();
    let plan = plan(install, &files, exists, read_manifest(install).as_ref(), current, &version)?;
    apply(install, &dir, &plan, game)?;
    Ok(version)
}

/// Start the launcher now in `install` (the new one, after a swap) and say why.
pub fn restart(install: &Path, why: &str) -> std::io::Result<()> {
    std::process::Command::new(install.join(launcher_exe())).arg(why).current_dir(install).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nvf-launcher-update-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn asset(name: &str) -> Asset {
        Asset { name: name.into(), browser_download_url: format!("https://x/{name}"), size: 10 }
    }

    #[test]
    fn archive_for_the_target() {
        let assets = [asset("NVForever-v0.8.0-x86_64-pc-windows-msvc.zip"), asset("NVForever-v0.8.0-x86_64-unknown-linux-gnu.tar.gz"), asset(CHECKSUMS)];
        let (a, k) = choose_asset(&assets, "v0.8.0", "x86_64-pc-windows-msvc").unwrap();
        assert_eq!((a.name.as_str(), k), ("NVForever-v0.8.0-x86_64-pc-windows-msvc.zip", ArchiveKind::Zip));
        let (a, k) = choose_asset(&assets, "v0.8.0", "x86_64-unknown-linux-gnu").unwrap();
        assert_eq!((a.name.as_str(), k), ("NVForever-v0.8.0-x86_64-unknown-linux-gnu.tar.gz", ArchiveKind::TarGz));
        assert!(choose_asset(&assets, "v0.7.0", "x86_64-pc-windows-msvc").is_none());
        assert!(choose_asset(&assets, "v0.8.0", "aarch64-apple-darwin").is_none());
        assert_eq!(checksums_asset(&assets).unwrap().name, CHECKSUMS);
        let ours = format!("{REPO_DOWNLOADS}v0.8.0/NVForever-v0.8.0-x86_64-pc-windows-msvc.zip");
        assert!(fetchable(&ours, "v0.8.0", "NVForever-v0.8.0-x86_64-pc-windows-msvc.zip"));
        assert!(!fetchable(&ours, "v0.8.0", "SHA256SUMS.txt"), "the address must name the asset itself");
        assert!(fetchable("http://127.0.0.1:8765/y.zip", "v0.8.0", "y.zip"), "the tests' local server");
        for bad in [
            format!("{REPO_DOWNLOADS}v0.7.0/x.zip"),
            format!("{REPO_DOWNLOADS}v0.8.0/../v0.7.0/x.zip"),
            format!("{REPO_DOWNLOADS}v0.8.0/"),
            "https://github.com/someone/else/releases/download/v0.8.0/x.zip".to_string(),
            "http://github.com/y.zip".into(),
            "file:///C:/y.zip".into(),
            "\\\\host\\share\\y.zip".into(),
            String::new(),
        ] {
            assert!(!fetchable(&bad, "v0.8.0", "x.zip"), "{bad}");
        }
        assert!(target().is_some(), "the machines that build releases have a target");
    }

    #[test]
    fn checksums_as_sha256sum_writes_them() {
        let h = "ab".repeat(32);
        let sums = format!("{}  NVForever-v0.8.0-x86_64-unknown-linux-gnu.tar.gz\n{h}  NVForever-v0.8.0-x86_64-pc-windows-msvc.zip\n{} *bin.zip\nnot a line\n", "00".repeat(32), "11".repeat(32));
        assert_eq!(checksum_for(&sums, "NVForever-v0.8.0-x86_64-pc-windows-msvc.zip"), Some([0xab; 32]));
        assert_eq!(checksum_for(&sums, "bin.zip"), Some([0x11; 32]));
        assert_eq!(checksum_for(&sums, "NVForever-v0.8.0-x86_64-pc-windows"), None);
        assert_eq!(checksum_for("abc  short.zip", "short.zip"), None);
        assert_eq!(checksum_for(&format!("{}  x.zip", "zz".repeat(32)), "x.zip"), None);
        assert_eq!(hex(&[0xab; 2]), "abab");
    }

    #[test]
    fn hashing_a_download() {
        let d = tmp("hash");
        let body = b"the archive bytes".repeat(10_000);
        let mut seen = 0;
        let got = copy_hashed(&body[..], &d.join("a.part"), body.len() as u64, |n| seen = n, &AtomicBool::new(false)).unwrap();
        assert_eq!(got, <[u8; 32]>::from(Sha256::digest(&body)));
        assert_eq!(got, sha256_file(&d.join("a.part")).unwrap());
        assert_eq!(seen, body.len() as u64);
        assert!(copy_hashed(&body[..], &d.join("b.part"), 100, |_| {}, &AtomicBool::new(false)).is_err(), "longer than said");
        assert_eq!(copy_hashed(&body[..], &d.join("c.part"), u64::MAX, |_| {}, &AtomicBool::new(true)).unwrap_err(), "cancelled");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn staging_paths() {
        let cache = Path::new("cache");
        let s = Staging::new(cache, "v0.8.0").unwrap();
        assert_eq!(s.dir, cache.join("updates").join("v0.8.0"));
        assert_eq!(s.part("a.zip"), s.dir.join("a.zip.part"));
        assert_eq!(s.archive("a.zip"), s.dir.join("a.zip"));
        assert_eq!(s.unpacked(), s.dir.join("unpacked"));
        for bad in ["", ".", "..", "../x", "v1/../../x", "C:", "v 1"] {
            assert!(Staging::new(cache, bad).is_none(), "{bad:?}");
        }
        let d = tmp("stale");
        for v in ["v0.6.0", "v0.7.0", "v0.8.0"] {
            fs::create_dir_all(updates_dir(&d).join(v)).unwrap();
        }
        let mut stale: Vec<_> = stale_staging(&d, "0.7.0").into_iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        stale.sort();
        assert_eq!(stale, ["v0.6.0", "v0.7.0"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn relative_paths_only() {
        assert_eq!(safe_rel(Path::new("a/./b.txt")), Some(PathBuf::from("a").join("b.txt")));
        for bad in ["", ".", "../a", "a/../../b", "/etc/passwd"] {
            assert_eq!(safe_rel(Path::new(bad)), None, "{bad:?}");
        }
        #[cfg(windows)]
        assert_eq!(safe_rel(Path::new(r"C:\Windows\x")), None);
    }

    #[test]
    fn the_swap_plan() {
        let install = Path::new("inst");
        let payload: Vec<PathBuf> = ["nvforever.exe", "nvforever-launcher.exe", "schemas/a.toml", "new.txt"].iter().map(PathBuf::from).collect();
        let there: BTreeSet<PathBuf> = ["nvforever.exe", "nvforever-launcher.exe", "schemas/a.toml", "schemas/gone.toml", "mine.txt"].iter().map(|p| install.join(p)).collect();
        let exists = |p: &Path| there.contains(p);
        let old = Manifest { version: "0.7.0".into(), files: vec!["nvforever.exe".into(), "schemas/gone.toml".into(), "../outside".into(), "update/installed.json".into(), "vanished.txt".into()] };
        let p = plan(install, &payload, exists, Some(&old), "0.7.0", "0.8.0").unwrap();
        assert_eq!(p.version, "0.8.0");
        assert_eq!(p.backup, install.join("update").join("previous").join("v0.7.0"));
        let rel = |v: &[&str]| v.iter().map(PathBuf::from).collect::<Vec<_>>();
        // Replaced files and the old release's file the new one lacks; never mine.txt (the
        // player's), the updater's own folder, a path outside or a file already gone.
        assert_eq!(p.aside, rel(&["nvforever-launcher.exe", "nvforever.exe", "schemas/a.toml", "schemas/gone.toml"]));
        assert_eq!(p.copy, rel(&["new.txt", "nvforever-launcher.exe", "nvforever.exe", "schemas/a.toml"]));
        // No manifest (an archive unpacked by hand): only what the payload replaces.
        let p = plan(install, &payload, exists, None, "0.7.0", "0.8.0").unwrap();
        assert_eq!(p.aside, rel(&["nvforever-launcher.exe", "nvforever.exe", "schemas/a.toml"]));
        // A payload reaching outside, or into update/, is refused whole.
        assert!(plan(install, &[PathBuf::from("../x")], exists, None, "0.7.0", "0.8.0").is_err());
        assert!(plan(install, &[PathBuf::from("Update/installed.json")], exists, None, "0.7.0", "0.8.0").is_err());
        assert!(plan(install, &payload, exists, None, "0.7.0", "../0.8.0").is_err());
    }

    /// A release folder as a player has it: programs, a schema, a file of their own.
    fn fixture_install(dir: &Path, version: &str) {
        for p in [launcher_exe(), programs()[0].clone(), programs()[1].clone()] {
            put(&dir.join(&p), &format!("{p} {version}"));
        }
        put(&dir.join("schemas").join("a.toml"), &format!("a {version}"));
        put(&dir.join("schemas").join("gone.toml"), "only in the old release");
        put(&dir.join("my notes.txt"), "the player's");
    }

    /// The new release's archive in the release workflow's shape: one top folder.
    fn fixture_zip(path: &Path, top: &str, version: &str, extra: &[(&str, &str)]) {
        let mut z = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).unix_permissions(0o755);
        let mut files: Vec<(String, String)> = [launcher_exe(), programs()[0].clone(), programs()[1].clone()].into_iter().map(|p| (p.clone(), format!("{p} {version}"))).collect();
        files.push(("schemas/a.toml".into(), format!("a {version}")));
        files.push(("schemas/b.toml".into(), format!("b {version}")));
        files.extend(extra.iter().map(|(a, b)| (a.to_string(), b.to_string())));
        z.add_directory(format!("{top}/"), o).unwrap();
        for (name, text) in files {
            z.start_file(format!("{top}/{name}"), o).unwrap();
            z.write_all(text.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn unpacking_refuses_escapes() {
        let d = tmp("escape");
        let zip_path = d.join("bad.zip");
        fixture_zip(&zip_path, "NVForever-v0.8.0-t", "0.8.0", &[("../../escaped.txt", "x")]);
        assert!(unpack(&zip_path, ArchiveKind::Zip, &d.join("out")).is_err());
        assert!(!d.join("escaped.txt").exists());
        // Not NVForever: no programs.
        let mut z = zip::ZipWriter::new(fs::File::create(d.join("other.zip")).unwrap());
        z.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
        z.finish().unwrap();
        assert!(unpack(&d.join("other.zip"), ArchiveKind::Zip, &d.join("out")).unwrap_err().contains("not an NVForever release"));
        // A tar.gz with one top folder, as the Linux archive is.
        let tgz = d.join("a.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(fs::File::create(&tgz).unwrap(), flate2::Compression::fast());
            let mut t = tar::Builder::new(gz);
            for p in [launcher_exe(), programs()[0].clone()] {
                let mut h = tar::Header::new_gnu();
                h.set_size(2);
                h.set_mode(0o755);
                h.set_cksum();
                t.append_data(&mut h, format!("NVForever-v0.8.0-t/{p}"), &b"hi"[..]).unwrap();
            }
            t.into_inner().unwrap().finish().unwrap();
        }
        let root = unpack(&tgz, ArchiveKind::TarGz, &d.join("tout")).unwrap();
        assert_eq!(root, d.join("tout").join("NVForever-v0.8.0-t"));
        assert_eq!(read(&root.join(launcher_exe())), "hi");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn update_and_roll_back_a_fixture_install() {
        let d = tmp("swap");
        let install = d.join("NVForever");
        fixture_install(&install, "0.7.0");
        let zip_path = d.join("NVForever-v0.8.0-x86_64-pc-windows-msvc.zip");
        fixture_zip(&zip_path, "NVForever-v0.8.0-x86_64-pc-windows-msvc", "0.8.0", &[]);
        let want = sha256_file(&zip_path).unwrap();
        let sums = format!("{}  {}\n", hex(&want), zip_path.file_name().unwrap().to_string_lossy());
        assert_eq!(checksum_for(&sums, &zip_path.file_name().unwrap().to_string_lossy()), Some(want));

        let payload = unpack(&zip_path, ArchiveKind::Zip, &d.join("cache").join("unpacked")).unwrap();
        let files = list_files(&payload).unwrap();
        assert_eq!(files.len(), 5);
        let ready = Ready { version: "0.8.0".into(), payload, files, staging: d.join("cache") };
        assert_eq!(install_unchecked(&install, &ready, "0.7.0"), Ok(()));

        assert_eq!(read(&install.join(launcher_exe())), format!("{} 0.8.0", launcher_exe()));
        assert_eq!(read(&install.join("schemas").join("b.toml")), "b 0.8.0");
        // An unpacked-by-hand install has no manifest, so gone.toml stays (it may be anyone's).
        assert_eq!(read(&install.join("schemas").join("gone.toml")), "only in the old release");
        assert_eq!(read(&install.join("my notes.txt")), "the player's");
        let m = read_manifest(&install).unwrap();
        assert_eq!(m.version, "0.8.0");
        assert!(m.files.contains(&"schemas/b.toml".to_string()));
        let (v, kept) = previous_version(&install).unwrap();
        assert_eq!(v, "0.7.0");
        assert_eq!(read(&kept.join(launcher_exe())), format!("{} 0.7.0", launcher_exe()));
        assert!(!kept.join("my notes.txt").exists());

        // Roll back: 0.7.0's files return, b.toml (only in 0.8.0's manifest) is set aside,
        // and 0.8.0 is kept so the roll back can be undone.
        assert_eq!(roll_back_unchecked(&install, "0.8.0"), Ok("0.7.0".to_string()));
        assert_eq!(read(&install.join(launcher_exe())), format!("{} 0.7.0", launcher_exe()));
        assert_eq!(read(&install.join("schemas").join("a.toml")), "a 0.7.0");
        assert!(!install.join("schemas").join("b.toml").exists());
        assert_eq!(read(&install.join("my notes.txt")), "the player's");
        let (v, kept) = previous_version(&install).unwrap();
        assert_eq!(v, "0.8.0");
        assert_eq!(read(&kept.join("schemas").join("b.toml")), "b 0.8.0");
        assert!(!previous_dir(&install).join("v0.7.0").exists(), "one previous version is kept");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_copy_puts_everything_back() {
        let d = tmp("fail");
        let install = d.join("NVForever");
        fixture_install(&install, "0.7.0");
        let payload = d.join("payload");
        put(&payload.join(launcher_exe()), "new launcher");
        put(&payload.join(&programs()[0]), "new game");
        // The plan names a file the payload lacks: its copy fails after others went in.
        let files = vec![PathBuf::from(launcher_exe()), PathBuf::from(&programs()[0]), PathBuf::from("zz-missing.txt")];
        let p = plan(&install, &files, |p| p.exists(), None, "0.7.0", "0.8.0").unwrap();
        assert!(apply(&install, &payload, &p, None).unwrap_err().contains("Nothing was changed"));
        assert_eq!(read(&install.join(launcher_exe())), format!("{} 0.7.0", launcher_exe()));
        assert_eq!(read(&install.join(&programs()[0])), format!("{} 0.7.0", programs()[0]));
        assert!(!install.join("zz-missing.txt").exists());
        assert!(read_manifest(&install).is_none());
        assert!(!p.backup.exists(), "the backup's copies are removed again");
        assert!(!list_files(&install).unwrap().iter().any(|f| f.to_string_lossy().ends_with(NEW_SUFFIX)), "no staged file is left");
        assert!(!journal_path(&install).exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn installs_the_updater_will_not_change() {
        let d = tmp("check");
        assert!(check_install(&d, None).unwrap_err().contains("not a release folder"));
        put(&d.join(&programs()[0]), "x");
        assert_eq!(check_install(&d, None), Ok(()));
        let here = std::env::current_exe().unwrap();
        assert!(check_install(here.parent().unwrap(), None).is_err(), "a cargo build of this checkout");
        assert_eq!(busy_program(&d), None);
        let _ = fs::remove_dir_all(&d);
    }

    /// A local HTTP server answering `files` by path (404 otherwise), one request per
    /// connection; its address.
    fn serve(files: Vec<(String, Vec<u8>)>) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for mut c in l.incoming().flatten() {
                let mut req = Vec::new();
                let mut b = [0u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match c.read(&mut b) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => req.extend_from_slice(&b[..n]),
                    }
                }
                let line = String::from_utf8_lossy(&req).lines().next().unwrap_or_default().to_string();
                let path = line.split(' ').nth(1).unwrap_or_default().to_string();
                let (status, body) = match files.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => ("200 OK", body.clone()),
                    None => ("404 Not Found", Vec::new()),
                };
                let _ = write!(c, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = c.write_all(&body);
            }
        });
        addr
    }

    /// The whole update as the launcher runs it (download, checksum, unpack, swap), from a
    /// fixture release on a local server into a fixture install; then the same with a
    /// checksum list that does not match, which must leave the install alone.
    #[test]
    fn update_from_a_local_release() {
        let Some(target) = target() else { return };
        let d = tmp("served");
        let install = d.join("NVForever");
        fixture_install(&install, "0.7.0");
        let (name, kind) = archive_name("v0.8.0", target);
        let archive = d.join(&name);
        match kind {
            ArchiveKind::Zip => fixture_zip(&archive, &format!("NVForever-v0.8.0-{target}"), "0.8.0", &[]),
            ArchiveKind::TarGz => {
                let gz = flate2::write::GzEncoder::new(fs::File::create(&archive).unwrap(), flate2::Compression::fast());
                let mut t = tar::Builder::new(gz);
                for p in [launcher_exe(), programs()[0].clone(), "schemas/b.toml".to_string()] {
                    let text = format!("{p} 0.8.0");
                    let mut h = tar::Header::new_gnu();
                    h.set_size(text.len() as u64);
                    h.set_mode(0o755);
                    h.set_cksum();
                    t.append_data(&mut h, format!("NVForever-v0.8.0-{target}/{p}"), text.as_bytes()).unwrap();
                }
                t.into_inner().unwrap().finish().unwrap();
            }
        }
        let bytes = fs::read(&archive).unwrap();
        let good = format!("{}  {name}\n", hex(&Sha256::digest(&bytes)));
        let bad = format!("{}  {name}\n", "00".repeat(32));
        let base = serve(vec![(format!("/{name}"), bytes.clone()), ("/good/SHA256SUMS.txt".into(), good.into_bytes()), ("/bad/SHA256SUMS.txt".into(), bad.into_bytes())]);
        let release = |sums: &str| Latest {
            version: "0.8.0".into(),
            tag: "v0.8.0".into(),
            url: format!("{base}/page"),
            notes: String::new(),
            assets: vec![
                Asset { name: name.clone(), browser_download_url: format!("{base}/{name}"), size: bytes.len() as u64 },
                Asset { name: CHECKSUMS.into(), browser_download_url: format!("{base}/{sums}/SHA256SUMS.txt"), size: 0 },
            ],
        };
        let cache = d.join("cache");
        let msgs = std::sync::Mutex::new(Vec::new());
        let send = |m: Msg| {
            if let Msg::Phase(p) = m {
                msgs.lock().unwrap().push(p);
            }
        };

        let e = prepare(&release("bad"), &cache, None, &send, &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("does not match its checksum"), "{e}");
        assert!(!Staging::new(&cache, "v0.8.0").unwrap().part(&name).exists(), "the bad download is deleted");
        assert_eq!(read(&install.join(launcher_exe())), format!("{} 0.7.0", launcher_exe()));

        msgs.lock().unwrap().clear();
        let ready = prepare(&release("good"), &cache, None, &send, &AtomicBool::new(false)).unwrap();
        assert_eq!(*msgs.lock().unwrap(), [Phase::Checking, Phase::Downloading, Phase::Verifying, Phase::Unpacking]);
        assert!(ready.payload.starts_with(updates_dir(&cache)));
        // A second run finds the checked archive staged and downloads nothing.
        msgs.lock().unwrap().clear();
        let ready = prepare(&release("good"), &cache, None, &send, &AtomicBool::new(false)).unwrap();
        assert_eq!(*msgs.lock().unwrap(), [Phase::Checking, Phase::Unpacking]);

        assert_eq!(install_with_check(&install, &ready), Ok("0.8.0".to_string()));
        assert_eq!(read(&install.join(launcher_exe())), format!("{} 0.8.0", launcher_exe()));
        assert_eq!(read(&install.join("schemas").join("b.toml")), if kind == ArchiveKind::Zip { "b 0.8.0".to_string() } else { "schemas/b.toml 0.8.0".to_string() });
        assert_eq!(read(&install.join("my notes.txt")), "the player's");
        assert!(!ready.staging.exists(), "the staged copy is removed after the swap");
        assert_eq!(previous_version(&install).unwrap().0, "0.7.0");
        let _ = fs::remove_dir_all(&d);
    }

    /// [`install`] itself (with [`check_install`]: the fixture install has the game program).
    fn install_with_check(install_dir: &Path, ready: &Ready) -> Result<String, String> {
        install(install_dir, ready, "0.7.0", None)
    }

    /// NVForever unpacked inside the game folder: the updater refuses before writing anything,
    /// and the swap's own guard refuses too.
    #[test]
    fn never_inside_the_game_folder() {
        let d = tmp("in-game");
        put(&d.join("Data").join("FalloutNV.esm"), "TES4");
        let game = GameInstall::open(&d).unwrap();
        let install = d.join("NVForever");
        fixture_install(&install, "0.7.0");
        assert!(check_install(&install, Some(&game)).unwrap_err().contains("inside the game folder"));
        let payload = d.join("payload");
        put(&payload.join(launcher_exe()), "new");
        let p = plan(&install, &[PathBuf::from(launcher_exe())], |p| p.exists(), None, "0.7.0", "0.8.0").unwrap();
        assert!(apply(&install, &payload, &p, Some(&game)).is_err());
        assert_eq!(read(&install.join(launcher_exe())), format!("{} 0.7.0", launcher_exe()));
        assert!(!p.backup.exists() && !journal_path(&install).exists());
        let _ = fs::remove_dir_all(&d);
    }

    /// The launcher killed after staging, before or during the renames: its next start
    /// finishes the swap from the journal; killed before the journal, it tidies the staged
    /// files away and the old release runs as before.
    #[test]
    fn a_swap_cut_short_is_finished_at_the_next_start() {
        let d = tmp("cut-short");
        let install = d.join("NVForever");
        fixture_install(&install, "0.7.0");
        let payload = d.join("payload");
        for f in [launcher_exe(), programs()[0].clone(), "schemas/a.toml".to_string()] {
            put(&payload.join(&f), &format!("{f} 0.8.0"));
        }
        let files = list_files(&payload).unwrap();
        let p = plan(&install, &files, |p| p.exists(), None, "0.7.0", "0.8.0").unwrap();
        let j = stage(&install, &payload, &p).unwrap();
        // The renames cut short after the first file.
        let first = install.join(&j.copy[0]);
        fs::rename(with_suffix(&first, NEW_SUFFIX), &first).unwrap();
        assert_eq!(resume_interrupted(&install, None), Some(Ok("0.8.0".to_string())));
        assert_eq!(read(&install.join(launcher_exe())), format!("{} 0.8.0", launcher_exe()));
        assert_eq!(read(&install.join("schemas").join("a.toml")), "schemas/a.toml 0.8.0");
        assert_eq!(read_manifest(&install).unwrap().version, "0.8.0");
        assert!(!journal_path(&install).exists());
        assert_eq!(read(&p.backup.join(launcher_exe())), format!("{} 0.7.0", launcher_exe()), "the old release is kept to roll back to");
        assert_eq!(resume_interrupted(&install, None), None, "nothing more to do");

        // Cut short before the journal: only staged files, which go; the release is untouched.
        let d2 = tmp("cut-early");
        let install2 = d2.join("NVForever");
        fixture_install(&install2, "0.7.0");
        put(&with_suffix(&install2.join(launcher_exe()), NEW_SUFFIX), "half a file");
        put(&with_suffix(&install2.join("schemas").join("a.toml"), NEW_SUFFIX), "x");
        assert_eq!(resume_interrupted(&install2, None), None);
        assert!(!list_files(&install2).unwrap().iter().any(|f| f.to_string_lossy().ends_with(NEW_SUFFIX)));
        assert_eq!(read(&install2.join(launcher_exe())), format!("{} 0.7.0", launcher_exe()));
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&d2);
    }

    /// The second rename of a running program fails (the new file held, as a virus scanner may):
    /// the old program goes back to its name, the journal stays, and the start that follows,
    /// with no `nvforever` (the first rename moved it), still finishes the swap once the file is
    /// free. An install before that finishes it first instead of staging over the backup.
    #[cfg(windows)]
    #[test]
    fn a_failed_second_rename_puts_the_program_back() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = tmp("second-rename");
        let install = d.join("NVForever");
        fixture_install(&install, "0.7.0");
        let payload = d.join("payload");
        for f in [launcher_exe(), programs()[0].clone()] {
            put(&payload.join(&f), &format!("{f} 0.8.0"));
        }
        let files = list_files(&payload).unwrap();
        let p = plan(&install, &files, |p| p.exists(), None, "0.7.0", "0.8.0").unwrap();
        let j = stage(&install, &payload, &p).unwrap();
        let game = install.join(&programs()[0]);
        let new = with_suffix(&game, NEW_SUFFIX);
        // The staged file held with no sharing: it cannot be renamed, so the first rename fails,
        // the program is moved aside, and the second rename fails too.
        let held_new = fs::OpenOptions::new().read(true).share_mode(0).open(&new).unwrap();
        let err = commit(&install, &j, None).unwrap_err();
        assert!(err.contains(&programs()[0]), "{err}");
        assert_eq!(read(&game), format!("{} 0.7.0", programs()[0]), "the old program is back under its name");
        assert!(!with_suffix(&game, OLD_SUFFIX).exists());
        assert!(journal_path(&install).exists(), "the journal stays to finish the swap later");
        // Try again (an install of a newer release) while the file is still held: it refuses
        // and leaves the kept previous version alone.
        let newer = d.join("payload-0.9");
        for f in [launcher_exe(), programs()[0].clone()] {
            put(&newer.join(&f), &format!("{f} 0.9.0"));
        }
        let ready = Ready { version: "0.9.0".into(), payload: newer.clone(), files: list_files(&newer).unwrap(), staging: d.join("staging") };
        assert!(super::install(&install, &ready, "0.7.0", None).unwrap_err().contains("still unfinished"));
        assert_eq!(read(&p.backup.join(&programs()[0])), format!("{} 0.7.0", programs()[0]));
        drop(held_new);
        // Try again once the file is free: it finishes the 0.8.0 swap and stops there (the
        // launcher restarts into 0.8.0), rather than staging 0.9.0 with 0.8.0's files kept as
        // "v0.7.0".
        assert_eq!(super::install(&install, &ready, "0.7.0", None), Ok("0.8.0".to_string()));
        assert_eq!(read(&game), format!("{} 0.8.0", programs()[0]));
        assert_eq!(read(&install.join(launcher_exe())), format!("{} 0.8.0", launcher_exe()));
        assert!(!journal_path(&install).exists());
        let (kept, dir) = previous_version(&install).unwrap();
        assert_eq!(kept, "0.7.0");
        assert_eq!(read(&dir.join(&programs()[0])), format!("{} 0.7.0", programs()[0]), "roll back still restores 0.7.0");
        let _ = fs::remove_dir_all(&d);
    }

    /// Cut short between moving a program aside and moving its new copy in: the install has no
    /// `nvforever`, and the start still finishes the swap from the journal.
    #[test]
    fn a_start_without_nvforever_finishes_the_swap() {
        let d = tmp("no-nvforever");
        let install = d.join("NVForever");
        fixture_install(&install, "0.7.0");
        let payload = d.join("payload");
        for f in [launcher_exe(), programs()[0].clone()] {
            put(&payload.join(&f), &format!("{f} 0.8.0"));
        }
        let p = plan(&install, &list_files(&payload).unwrap(), |p| p.exists(), None, "0.7.0", "0.8.0").unwrap();
        stage(&install, &payload, &p).unwrap();
        let game = install.join(&programs()[0]);
        fs::rename(&game, with_suffix(&game, OLD_SUFFIX)).unwrap();
        assert!(check_install(&install, None).is_err(), "no nvforever beside the launcher");
        assert_eq!(resume_interrupted(&install, None), Some(Ok("0.8.0".to_string())));
        assert_eq!(read(&game), format!("{} 0.8.0", programs()[0]));
        assert_eq!(read(&install.join(launcher_exe())), format!("{} 0.8.0", launcher_exe()));
        let _ = fs::remove_dir_all(&d);
    }

    /// A journal naming a path outside the install, or inside the game folder, is not followed.
    #[test]
    fn an_untrusted_journal_is_refused() {
        let d = tmp("bad-journal");
        put(&d.join("Data").join("FalloutNV.esm"), "TES4");
        let game = GameInstall::open(&d).unwrap();
        let install = d.join("NVForever");
        fixture_install(&install, "0.7.0");
        let at = install.display().to_string();
        let outside = Journal { install: at.clone(), version: "0.8.0".into(), copy: vec!["../escape.dll".into()], retire: vec![] };
        write_json(&journal_path(&install), &outside).unwrap();
        assert!(resume_interrupted(&install, None).unwrap().unwrap_err().contains("unusable path"));
        let ours = Journal { install: at.clone(), version: "0.8.0".into(), copy: vec![launcher_exe()], retire: vec![] };
        write_json(&journal_path(&install), &ours).unwrap();
        assert!(resume_interrupted(&install, Some(&game)).unwrap().is_err(), "inside the game folder");
        // Written for another folder (a copied install), or naming a file neither staged nor in
        // place: not followed.
        let elsewhere = Journal { install: d.join("Other").display().to_string(), ..ours.clone() };
        write_json(&journal_path(&install), &elsewhere).unwrap();
        assert!(resume_interrupted(&install, None).unwrap().unwrap_err().contains("another folder"));
        let missing = Journal { copy: vec!["gone.dll".into()], ..ours };
        write_json(&journal_path(&install), &missing).unwrap();
        assert!(resume_interrupted(&install, None).unwrap().unwrap_err().contains("neither staged nor in place"));
        assert_eq!(read(&install.join(launcher_exe())), format!("{} 0.7.0", launcher_exe()));
        let _ = fs::remove_dir_all(&d);
    }

    /// [`install`] and [`roll_back`] without [`check_install`] (a test folder under the
    /// system's temp folder has no game program to check against).
    fn install_unchecked(install: &Path, ready: &Ready, current: &str) -> Result<(), String> {
        let p = plan(install, &ready.files, |p| p.exists(), read_manifest(install).as_ref(), current, &ready.version)?;
        apply(install, &ready.payload, &p, None)
    }

    fn roll_back_unchecked(install: &Path, current: &str) -> Result<String, String> {
        let (version, dir) = previous_version(install).ok_or("none kept")?;
        let p = plan(install, &list_files(&dir).unwrap(), |p| p.exists(), read_manifest(install).as_ref(), current, &version)?;
        apply(install, &dir, &p, None)?;
        Ok(version)
    }
}
