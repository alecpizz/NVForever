//! Android launcher requests. SAF and HTTPS use Android APIs; the launcher owns decisions and files.

use crate::{catalog, display, launch, news, start, theme};
use fnv_install::{ini_get, ini_pairs, ini_set, plugins, EngineHome, GameInstall};
use nvf_settings_layout::menu;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nvf_settings_layout::MOBILE_TAB;
const TABS: [&str; 5] = ["Gameplay", "Display", MOBILE_TAB, "Graphics", nvf_settings_layout::TWEAKS_TAB];
const IMPORT_FIRST: &str = "Import a complete Fallout: New Vegas folder before starting.";

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Request {
    State,
    ValidateImport { path: PathBuf },
    Start { resume: bool },
    Settings { edits: BTreeMap<String, String> },
    SaveSettings { edits: BTreeMap<String, String> },
    Mods,
    Plugin { name: String, change: PluginChange },
    SaveStart { fields: start::Fields },
    SetNews { enabled: bool },
    FetchNews,
    CheckUpdate { current: String },
    Countdown { version: String, at: i64, now: i64 },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PluginChange { Toggle, Up, Down }

/// Called by the Activity's JNI adapter. Replies contain either `data` or a visible `error`.
pub fn request(home: &Path, text: &str, mut get: impl FnMut(&str) -> Result<String, String>) -> String {
    let home = EngineHome::new(home);
    let result = serde_json::from_str::<Request>(text).map_err(|e| e.to_string())
        .and_then(|r| handle(&home, r, &mut get));
    match result {
        Ok(data) => json!({"data": data}),
        Err(error) => json!({"error": error}),
    }.to_string()
}

fn handle(home: &EngineHome, req: Request, get: &mut impl FnMut(&str) -> Result<String, String>) -> Result<Value, String> {
    let path = home.config_file();
    let ini = if matches!(&req, Request::Countdown { .. } | Request::ValidateImport { .. }) {
        String::new()
    } else {
        match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.to_string()),
        }
    };
    let ready = || launch::game_from_ini(&ini).filter(|game| game.missing_files().is_empty());
    let news_enabled = || ini_get(&ini, "Launcher", "News").is_none_or(|v| v != "0");
    match req {
        Request::State => {
            let game = ready();
            let preview = game.as_ref().and_then(|_| newest_save_preview(home));
            let cont = preview.as_ref().and_then(|p| p.get("name").and_then(Value::as_str).map(str::to_owned));
            Ok(json!({
                "theme": theme::android(), "folder": game.map(|g| g.root),
                "continue": cont,
                "savePreview": preview,
                "newsEnabled": news_enabled(),
            }))
        }
        Request::ValidateImport { path } => {
            let game = GameInstall::open(&path).map_err(|e| e.to_string())?;
            if game.root.canonicalize().map_err(|e| e.to_string())? != path.canonicalize().map_err(|e| e.to_string())? {
                return Err("The copy must contain its own Data directory.".into());
            }
            let missing = game.missing_files();
            if !missing.is_empty() {
                return Err(format!("The copied game is incomplete: {}", missing.join(", ")));
            }
            Ok(Value::Null)
        }
        Request::Start { resume } => {
            let game = ready();
            if !launch::folder_known(game.is_some(), false, None) {
                return Err(IMPORT_FIRST.into());
            }
            let start = if resume {
                let (path, _) = newest_save(home).ok_or("No NVForever save is available to continue.")?;
                launch::Start::Load(path)
            } else {
                launch::Start::MainMenu
            };
            // This selects the platform defaults without recording them as explicit row overrides.
            let mut args = start.args();
            args.push("--android-player-start".into());
            Ok(json!(args))
        }
        Request::Settings { edits } => Ok(settings(&path, &ini, &edits)),
        Request::SaveSettings { edits } => {
            let catalog = catalog::Catalog::load_mobile(Some(&path), &TABS);
            for (key, v) in &edits {
                let row = catalog.rows.iter().find(|row| row.key == *key && allowed_row(key)).ok_or("That setting is not available on Android.")?;
                if !safe_value(v) || desktop_choice(key, v) || display::read_only(&row.key) {
                    return Err(format!("{} cannot be written from the Android launcher.", row.label));
                }
            }
            if !edits.is_empty() {
                launch::write_ini(&path, |text| catalog::with_edits(text, &edits))?;
            }
            Ok(Value::Null)
        }
        Request::Mods => {
            if let Some(game) = ready() {
                plugins::sync(home, &game).map_err(|e| e.to_string())?;
            }
            let list = plugin_list(home)?;
            let ttw = list.entries.iter().find(|p| p.name.eq_ignore_ascii_case(start::TTW_PLUGIN)).map(|p| p.active);
            let entries: Vec<_> = list.entries.iter().map(|p| json!({"name": p.name, "active": p.active})).collect();
            Ok(json!({"fields": start::Fields::read(&ini), "ttw": ttw, "ttwDefaults": start::Fields::ttw(), "entries": entries}))
        }
        Request::Plugin { name, change } => {
            let game = ready().ok_or(IMPORT_FIRST)?;
            let mut list = plugin_list(home)?;
            let i = list.entries.iter().position(|p| p.name == name).ok_or("The plugin list has changed.")?;
            if name.eq_ignore_ascii_case("FalloutNV.esm") {
                return Err("FalloutNV.esm always loads first.".into());
            }
            match change {
                PluginChange::Toggle => list.entries[i].active = !list.entries[i].active,
                PluginChange::Up if i > 0 && !list.entries[i - 1].name.eq_ignore_ascii_case("FalloutNV.esm") => list.entries.swap(i - 1, i),
                PluginChange::Down if i + 1 < list.entries.len() && !list.entries[i + 1].name.eq_ignore_ascii_case("FalloutNV.esm") => list.entries.swap(i, i + 1),
                _ => return Ok(Value::Null),
            }
            let path = home.plugins_file();
            fnv_install::guard_write(&path, Some(&game)).map_err(|e| e.to_string())?;
            fnv_install::write_whole(&path, list.to_text().as_bytes()).map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        Request::SaveStart { fields } => {
            if !safe_value(&fields.quest) || !safe_value(&fields.film) {
                return Err("Use a single line per start setting.".into());
            }
            launch::write_ini(&path, |text| fields.write(&text))?;
            Ok(Value::Null)
        }
        Request::SetNews { enabled } => {
            launch::write_ini(&path, |text| ini_set(&text, "Launcher", "News", if enabled { "1" } else { "0" }))?;
            Ok(Value::Null)
        }
        Request::FetchNews => {
            if !news_enabled() {
                return Ok(json!({"posts": [], "source": "News fetching is off"}));
            }
            let cache = home.cache();
            let (posts, source) = match news::select_posts(get(news::SITE_NEWS), || get(news::RELEASES)) {
                Ok((posts, source)) => {
                    news::save_cache(&cache, &posts);
                    (posts, source)
                }
                Err(_) => (news::load_cache(&cache), news::Source::Cache),
            };
            let next = get(news::SITE_CONFIG).ok().and_then(|js| news::parse_next_release(&js));
            let source = match source {
                news::Source::Site => "From nv-forever.com",
                news::Source::Releases => "Release notes from GitHub",
                news::Source::Cache => "Offline · saved posts",
            };
            let posts: Vec<_> = posts.iter().map(|p| json!({"title": p.title, "body": p.body, "date": p.shown_date(), "author": p.author, "url": p.url})).collect();
            Ok(json!({"posts": posts, "source": source, "next": next.and_then(|n| n.at.map(|at| json!({"version": n.version, "at": at})))}))
        }
        Request::CheckUpdate { current } => check_update(&current, get),
        Request::Countdown { version, at, now } => Ok(json!(if at > now {
            format!("{version} · {}", news::countdown_text(at.saturating_sub(now)))
        } else { String::new() })),
    }
}

const REPO_DOWNLOADS: &str = "https://github.com/LootJunkieAzza/NVForever/releases/download/";
const CHECKSUMS_NAME: &str = "SHA256SUMS.txt";

fn safe_component(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn repo_asset_url(url: &str, tag: &str, name: &str) -> bool {
    safe_component(tag)
        && safe_component(name)
        && url.strip_prefix(REPO_DOWNLOADS)
            .and_then(|rest| rest.strip_prefix(tag))
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some_and(|file| file == name)
}

fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, rest) = line.trim().split_once(char::is_whitespace)?;
        let file = rest.trim_start();
        let file = file.strip_prefix('*').unwrap_or(file);
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

/// Compare Android launcher version strings, treating `-android-r<N>` as a
/// post-release Android revision above the base release (`0.7.0-android-r10` > `v0.7.0`,
/// and `0.7.0-android-r10` > `0.7.0-android-r9`), while preserving SemVer ordering for
/// ordinary pre-releases (`0.9.0-rc.1` < `0.9.0`) and core version numbers (`0.7.1` > `0.7.0-android-r10`).
pub(crate) fn cmp_versions(a: &str, b: &str) -> std::cmp::Ordering {
    fn split_android_rev(v: &str) -> (&str, Option<u64>) {
        let trimmed = v.trim();
        let no_build = trimmed.split('+').next().unwrap_or(trimmed);
        if let Some((base, rev_str)) = no_build.rsplit_once("-android-r") {
            if let Ok(rev) = rev_str.parse::<u64>() {
                return (base, Some(rev));
            }
        }
        (no_build, None)
    }
    let (a_base, a_rev) = split_android_rev(a);
    let (b_base, b_rev) = split_android_rev(b);
    let base_ord = news::cmp_versions(a_base, b_base);
    if base_ord.is_ne() {
        return base_ord;
    }
    match (a_rev, b_rev) {
        (None, None) => std::cmp::Ordering::Equal,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(x), Some(y)) => x.cmp(&y),
    }
}

fn is_newer(a: &str, b: &str) -> bool {
    cmp_versions(a, b) == std::cmp::Ordering::Greater
}

fn newest(list: Vec<news::Latest>) -> Option<news::Latest> {
    list.into_iter().max_by(|a, b| cmp_versions(&a.version, &b.version))
}

/// Select the ARM64 Android `.apk` candidate from a release's assets by filename and
/// repository URL, refusing names containing `unsigned`, `x86`, or `armeabi` and
/// off-repository download URLs. Cryptographic signing-certificate verification runs
/// in Android (`MobileLauncher` + `UpdatePolicy.verifyApkMetadata`) before installer handoff.
fn select_android_apk<'a>(assets: &'a [news::Asset], tag: &str) -> Option<&'a news::Asset> {
    let candidates: Vec<&news::Asset> = assets.iter().filter(|a| {
        let lower = a.name.to_ascii_lowercase();
        lower.ends_with(".apk")
            && !lower.contains("unsigned")
            && !lower.contains("x86")
            && !lower.contains("armeabi")
            && repo_asset_url(&a.browser_download_url, tag, &a.name)
    }).collect();
    let is_arm64 = |name: &str| {
        let lower = name.to_ascii_lowercase();
        lower.contains("arm64") || lower.contains("aarch64")
    };
    candidates.iter().copied().find(|a| is_arm64(&a.name))
        .or_else(|| (candidates.len() == 1).then(|| candidates[0]))
}

fn check_update(current: &str, get: &mut impl FnMut(&str) -> Result<String, String>) -> Result<Value, String> {
    let text = get(news::RELEASES)?;
    let releases = news::parse_release_list(&text)?;
    let Some(latest) = newest(releases) else {
        return Ok(json!({"available": false, "current": current}));
    };
    if !is_newer(&latest.version, current) {
        return Ok(json!({
            "available": false,
            "current": current,
            "version": latest.version,
            "tag": latest.tag,
        }));
    }
    let asset = select_android_apk(&latest.assets, &latest.tag).and_then(|apk| {
        // Always fail closed (matching desktop `update.rs`): direct in-app APK download
        // requires a published `SHA256SUMS.txt` on the same repository release with a valid
        // 64-hex-char entry for the selected APK. Otherwise omit `asset` so the launcher
        // offers only "Open release page" in the browser instead of an unverified download.
        let sums = latest.assets.iter().find(|a| a.name == CHECKSUMS_NAME)?;
        if !repo_asset_url(&sums.browser_download_url, &latest.tag, CHECKSUMS_NAME) {
            return None;
        }
        let body = get(&sums.browser_download_url).ok()?;
        let sha256 = checksum_for(&body, &apk.name)?;
        Some(json!({
            "name": apk.name,
            "url": apk.browser_download_url,
            "size": apk.size,
            "sha256": sha256,
        }))
    });
    Ok(json!({
        "available": true,
        "current": current,
        "version": latest.version,
        "tag": latest.tag,
        "url": latest.url,
        "notes": latest.notes,
        "asset": asset,
    }))
}

fn newest_save(home: &EngineHome) -> Option<(PathBuf, String)> {
    fnv_save::list(&home.saves()).into_iter().next().map(|(path, header)| (path, header.player_name))
}

fn format_playtime(play_seconds: f64) -> String {
    if !play_seconds.is_finite() || play_seconds <= 0.0 {
        return "< 1m".into();
    }
    let total_minutes = (play_seconds / 60.0).floor() as u64;
    if total_minutes == 0 {
        return "< 1m".into();
    }
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((triple >> 6) & 0x3f) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(triple & 0x3f) as usize] as char } else { '=' });
    }
    out
}

/// Metadata and base64-encoded PNG thumbnail for the newest save, used by the
/// Android launcher's Continue preview card.
fn newest_save_preview(home: &EngineHome) -> Option<Value> {
    let (path, hd) = fnv_save::list(&home.saves()).into_iter().next()?;
    let name = if hd.player_name.trim().is_empty() {
        path.file_stem().unwrap_or_default().to_string_lossy().into_owned()
    } else {
        hd.player_name.clone()
    };
    let location = hd.location.trim();
    let shown_location = if location.is_empty() || location == "Unnamed" { "" } else { location };
    let playtime = format_playtime(hd.play_seconds);
    let mut parts = vec![format!("Level {}", hd.level)];
    if !shown_location.is_empty() {
        parts.push(shown_location.to_string());
    }
    parts.push(playtime.clone());
    let detail = parts.join(" · ");
    let shot = fnv_save::read_header(&path)
        .ok()
        .map(|s| s.shot)
        .filter(|s| !s.is_empty())
        .map(|bytes| base64_encode(&bytes));
    Some(json!({
        "name": name,
        "level": hd.level,
        "location": shown_location,
        "playSeconds": hd.play_seconds,
        "playtime": playtime,
        "detail": detail,
        "shotBase64": shot,
    }))
}

fn plugin_list(home: &EngineHome) -> Result<plugins::PluginList, String> {
    match std::fs::read_to_string(home.plugins_file()) {
        Ok(text) => Ok(plugins::PluginList::parse(&text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(plugins::PluginList::default()),
        Err(e) => Err(e.to_string()),
    }
}

fn safe_value(value: &str) -> bool {
    !value.contains(['\n', '\r', '\0'])
}

fn allowed_row(key: &str) -> bool {
    // Android has no player-report controls. Desktop controls and in-game
    // actions do not represent editable Android INI settings.
    !matches!(key, "monitor" | "resolution" | "borderless" | "dlss-model" | "frame-generation-frames"
        | "send-reports" | "reports-delete" | "touch-layout-editor-v1" | "touch-practice-v1"
        | "touch-tray-editor-v1" | "mobile-thermal-status")
}

fn desktop_choice(key: &str, value: &str) -> bool {
    matches!(key, "upscaler" | "frame-generation") && value.to_ascii_lowercase().contains("dlss")
}

fn display_choices(key: &str, home: &Path) -> Option<Vec<display::Choice>> {
    match key {
        "language" => Some(display::language_choices(&[home.join("lang")])),
        k => display::common_choices(k),
    }
}

fn settings(path: &Path, ini: &str, edits: &BTreeMap<String, String>) -> Value {
    let catalog = catalog::Catalog::load_mobile(Some(path), &TABS);
    let home = path.parent().unwrap_or(path);
    let stored: BTreeMap<_, _> = ini_pairs(ini, "Settings").into_iter().collect();
    let current = |key: &str| edits.get(key).or_else(|| stored.get(key)).map(String::as_str);
    let holds = |rule: &menu::Rule| {
        let parent = catalog.rows.iter().find(|row| row.key == rule.parent);
        let kind = parent.and_then(|row| row.kind.as_ref());
        let device;
        let value = if rule.parent == nvf_settings_layout::DEVICE_ROW {
            device = ini_get(ini, "Controls", "Input Device").unwrap_or_default();
            &device
        } else if let Some(v) = current(&rule.parent).or_else(|| parent.and_then(|r| r.default.as_deref())) {
            v
        } else {
            return true;
        };
        let name = display_choices(&rule.parent, home).and_then(|l| l.into_iter().find(|c| c.stored == value));
        catalog::rule_holds(rule, kind, value.trim(), name.as_ref().map_or(value, |c| &c.label).trim())
    };
    let rows: Vec<_> = catalog.rows.iter().filter(|r| allowed_row(&r.key)
        && (!r.unlisted || stored.contains_key(&r.key))
        && r.shown_with.as_ref().is_none_or(holds)
        && display::colour_of_part(&r.key).is_none_or(|key| current(key).is_some_and(|v| v.eq_ignore_ascii_case(display::CUSTOM))))
        .map(|r| {
            let value = current(&r.key).or(r.default.as_deref()).unwrap_or_default();
            let kind = control(r, value, home);
            let reason = r.needs.iter().find(|rule| !holds(rule)).map(|rule| rule.why.as_str());
            json!({"key": r.key, "label": r.label, "tab": r.tab, "heading": r.heading, "note": r.note,
                "value": catalog::list_value(&r.key, value), "control": kind, "reason": reason,
                "readOnly": display::read_only(&r.key)})
        }).collect();
    let mut tabs = catalog.tabs;
    if !tabs.iter().any(|(name, _)| name == MOBILE_TAB) {
        let index = tabs.iter().position(|(name, _)| name == "Graphics").unwrap_or(tabs.len());
        tabs.insert(index, (MOBILE_TAB.into(), MOBILE_TAB.into()));
    }
    json!({"rows": rows, "tabs": tabs})
}

fn control(row: &catalog::Row, value: &str, home: &Path) -> Value {
    let key = &row.key;
    match &row.kind {
        Some(k @ menu::Kind::Slider { .. }) if key != "frame-cap" && (value.is_empty() || value.parse::<f32>().is_ok()) => return json!(k),
        Some(k @ menu::Kind::Switch) => return json!(k),
        Some(menu::Kind::List { choices }) if !choices.is_empty() && !matches!(key.as_str(), "display-sync" | "language" | "frame-cap" | "frame-generation" | "physics-rate" | "fps-counter-low") => {
            let choices: Vec<_> = choices.iter().filter(|c| !desktop_choice(key, &c.name)).collect();
            return json!({"type": "list", "choices": choices});
        }
        _ => {}
    }
    if let Some(choices) = display_choices(key, home) {
        let choices: Vec<_> = choices.into_iter().filter(|c| !desktop_choice(key, &c.stored))
            .map(|c| json!({"name": c.stored, "label": c.label})).collect();
        return json!({"type": "list", "choices": choices});
    }
    if let Some(names) = catalog::list_choices().get(key.as_str()) {
        let choices: Vec<_> = names.iter().map(|name| json!({"name": name, "label": name})).collect();
        return json!({"type": "list", "choices": choices});
    }
    let words = match key.as_str() {
        // Android expands menu scale and barter/repair/workbench readability up to 200%.
        "menu-scale" => Some((0..16).map(|i| format!("{}%", 50 + 10 * i)).collect()),
        "barter-menu-size-v1" | "repair-menu-size-v1" | "workbench-menu-size-v1" => {
            Some((0..31).map(|i| format!("{}%", 50 + 5 * i)).collect())
        }
        _ => display::steps(key)
            .or_else(|| catalog::step_words().get(key.as_str()).map(|words| words.iter().map(|s| s.to_string()).collect())),
    };
    if let Some(words) = words {
        let step = 1.0 / (words.len() - 1) as f32;
        let steps: Vec<_> = words.iter().enumerate().map(|(i, label)| json!({"value": i as f32 * step, "label": label})).collect();
        return json!({"type": "slider", "min": 0, "step": step, "steps": steps});
    }
    match catalog::Kind::of_value(value) {
        catalog::Kind::Switch => json!({"type": "switch"}),
        catalog::Kind::Slider => json!({"type": "slider", "min": 0, "step": 0.01}),
        catalog::Kind::Text => json!({"type": "text"}),
    }
}

#[cfg(test)]
mod tests;
