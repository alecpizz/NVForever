//! The Graphics tab's two rows over other rows (#1966; docs/spec-notes/quality-presets.md):
//!
//! - **Quality preset**: Low, Medium, High, Very High from the install's own preset files,
//!   `low.ini`, `medium.ini`, `high.ini` and `VeryHigh.ini` beside `Fallout_default.ini`
//!   [documented: the install's files; docs/spec-notes/quality-presets.md]. Taking one sets
//!   the rows its keys map to ([`MAPPED`]) to the file's values, each through the row's own
//!   setter, as a change on the page would. The row reads the first preset whose values
//!   all the mapped rows hold, else Custom [design: #1966]: a change to one of those rows
//!   shows Custom at once, and moving it back shows the preset again. Custom is what the
//!   row reads, not a choice to take; the list offers it only while the rows match no
//!   preset (as the colour lists' Custom, `nvf_play::ui_colour`, it is then the one chosen)
//!   [design].
//! - The **Enhanced effects** row that set every effect switch at once (#1966) is gone [design:
//!   owner 2026-11-15, the menu overhaul: "no need to have it twice"]; the effects keep
//!   their own rows. The quality rows and the effects never overlap (a quality row is the
//!   same in both looks, `enhanced_keys`), so a preset never touches an effect.
//!
//! **Read at run time, never written.** The values come from the player's install each run
//! (`fnv_install::GameInstall::open(--data)`'s root folder, matched without case); nothing
//! from the files is in NVForever. A file that is missing or unreadable leaves its preset
//! out of the list, said once in the log; with none of the four the Quality row is not
//! registered (the issue: "Without the files the row is not shown"). Test aid
//! [`PRESETS_ENV`] reads them from another folder (the smoke fixtures' made-up files).
//!
//! **Kept in `NVForever.ini`** like the other lists, as the name the row reads
//! (`quality=Custom`). The rows are kept on their own and are the truth: the stored name
//! sets nothing at the start, so a test run's command line still wins over the INI row by
//! row (#1382) [design: #1966 review]. The row names no command-line switch, so the INI read
//! never says one was overridden. Rows that are lists now (Sun Shadows, Shadow Filtering,
//! Shadow Distance) take a preset's value as the choice nearest it.
//!
//! **Defaults unchanged**: nothing here changes a row until the player takes a choice; the
//! game starts with today's values and the maintainer's vanilla look
//! (`graphics::PLAY_DEFAULT`), and the Quality row reads whichever preset they match
//! [design: #1966 point 4].

use crate::graphics;
#[cfg(test)]
use crate::graphics::{Graphics, Preset as Look};
use crate::Args;
use bevy::prelude::*;
use nvf_play::settings::{self, add_choice_list, choice_index, choice_part, ChoiceList, Setting, Settings};
use std::path::{Path, PathBuf};

/// The settings key of the Quality row.
pub const QUALITY_KEY: &str = "quality";
/// What the row reads when the rows match no preset.
pub const CUSTOM: &str = "Custom";

/// Test aid: the folder the preset files are read from instead of the game folder (smoke
/// scenarios use made-up files; a folder without them shows the row missing).
pub const PRESETS_ENV: &str = "NVF_PLAY_TEST_QUALITY_PRESETS";

/// The presets: the name the row shows (our own words: the load order has no game setting
/// for them [documented: FalloutNV.esm holds no "Very High" string, searched 2026-11-09])
/// and the file beside `Fallout_default.ini` [documented: the install's files].
pub const PRESETS: [(&str, &str); 4] = [("Low", "low.ini"), ("Medium", "medium.ini"), ("High", "high.ini"), ("Very High", "VeryHigh.ini")];

/// How a preset key's value becomes a row's value [design: #1966].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Map {
    /// A switch row: on when the number is not 0.
    Switch,
    /// A slider row's part: the number against the largest number the preset files found
    /// give that key (the fullest preset draws at 100%), brought onto the slider's range
    /// and steps by the row.
    Part,
    /// The Shadow filtering row: 0 is Fast, any other number Smooth (the vanilla filter);
    /// Temporal is NVForever's own and no preset takes it [design; what the original does
    /// with each number is not known: hypothesis].
    ShadowFilter,
}

/// A preset key and the row it sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapped {
    pub section: &'static str,
    pub key: &'static str,
    pub row: &'static str,
    pub map: Map,
}

const fn mapped(section: &'static str, key: &'static str, row: &'static str, map: Map) -> Mapped {
    Mapped { section, key, row, map }
}

/// The preset keys that set a row, in the files' order [design: #1966; which key means
/// what is read from its name, docs/spec-notes/quality-presets.md]. Anti-aliasing (the
/// files' multisampling) and FSR are left to their own rows until the maintainer says
/// otherwise (#1966's question), and the anisotropy to the enhanced effects.
pub const MAPPED: [Mapped; 9] = [
    mapped("Display", "fShadowLODStartFade", "shadow-distance", Map::Part),
    mapped("Display", "bDrawShadows", "sun-shadows", Map::Switch),
    mapped("Display", "iShadowFilter", "shadow-filter", Map::ShadowFilter),
    mapped("LOD", "fLODFadeOutMultActors", "actor-distance", Map::Part),
    mapped("LOD", "fLODFadeOutMultObjects", "object-distance", Map::Part),
    mapped("LOD", "fLODFadeOutMultItems", "item-distance", Map::Part),
    mapped("Grass", "fGrassStartFadeDistance", "grass-distance", Map::Part),
    mapped("Water", "bUseWaterReflections", "water-mirror", Map::Switch),
    mapped("BlurShaderHDR", "bDoHighDynamicRange", "glow", Map::Switch),
];

/// The preset files' other keys: no NVForever row counts for them, so a preset leaves them
/// unused (docs/spec-notes/quality-presets.md says why for each) [design: #1966].
pub const UNUSED: [(&str, &str); 32] = [
    ("Display", "iMultiSample"),
    ("Display", "iWaterMultisamples"),
    ("Display", "bTransparencyMultisampling"),
    ("Display", "iMaxDecalsPerFrame"),
    ("Display", "iMaxAnisotropy"),
    ("Display", "iTexMipMapSkip"),
    ("Display", "iTexMipMapMinimum"),
    ("Display", "fDecalLOD1"),
    ("Display", "fDecalLOD2"),
    ("Display", "iActorShadowCountInt"),
    ("Display", "iActorShadowCountExt"),
    ("Display", "fSpecularLODStartFade"),
    ("Display", "fLightLODStartFade"),
    ("Display", "iShadowMapResolution"),
    ("TerrainManager", "fTreeLoadDistance"),
    ("TerrainManager", "fBlockLoadDistanceLow"),
    ("TerrainManager", "fBlockLoadDistance"),
    ("TerrainManager", "fSplitDistanceMult"),
    ("TerrainManager", "fHighBlockLoadDistanceLow"),
    ("Grass", "b30GrassVS"),
    ("Imagespace", "bDoDepthOfField"),
    ("Imagespace", "iRadialBlurLevel"),
    ("Water", "bUseWaterRefractions"),
    ("Water", "bUseWaterReflectionBlur"),
    ("Water", "bUseWaterDisplacements"),
    ("Water", "bUseWaterDepth"),
    ("Water", "iWaterReflectWidth"),
    ("Water", "iWaterReflectHeight"),
    ("Water", "bReflectExplosions"),
    ("Water", "iWaterBlurAmount"),
    ("Water", "bAutoWaterSilhouetteReflections"),
    ("Water", "bForceHighDetailReflections"),
];

/// NVForever's own rows a preset sets beside the files' keys: (preset, row, the choice it
/// takes) [design, #2864]. Smooth terrain is not in the original, so no preset file names
/// it; on an integrated GPU (Intel N150, 1920x1080, the Low preset's rows, Goodsprings at
/// noon) 4 x 4 costs 9 ms of a 73 ms frame and 2 x 2 3 ms [observed: #2864], so the two
/// lower presets turn it off and the two upper ones keep the default. The row applies at
/// the next start (`nvf_play::smooth_terrain`), and so does a preset's change to it.
pub const OWN: [(&str, &str, &str); 4] = [
    ("Low", nvf_play::smooth_terrain::KEY, "Off"),
    ("Medium", nvf_play::smooth_terrain::KEY, "Off"),
    ("High", nvf_play::smooth_terrain::KEY, "4 x 4"),
    ("Very High", nvf_play::smooth_terrain::KEY, "4 x 4"),
];

/// A preset as rows: its name, each mapped row's value, as `Setting::value` gives it
/// (a switch 1 or 0, a slider its part), and the choices of NVForever's own rows ([`OWN`]).
#[derive(Clone, Debug, PartialEq)]
pub struct QualityPreset {
    pub label: &'static str,
    pub rows: Vec<(&'static str, f32)>,
    pub choices: Vec<(&'static str, &'static str)>,
}

/// [`OWN`]'s rows for the preset named `label`.
pub fn own_choices(label: &str) -> Vec<(&'static str, &'static str)> {
    OWN.iter().filter(|(p, _, _)| *p == label).map(|(_, row, choice)| (*row, *choice)).collect()
}

/// The presets offered this run (those whose file was read), in [`PRESETS`]' order, and
/// the folder they came from.
#[derive(Resource, Clone, Debug, Default)]
pub struct QualityPresets {
    pub dir: PathBuf,
    pub presets: Vec<QualityPreset>,
}

/// Preset names, each with a text: a file's contents, or why it was not read.
pub type Named = Vec<(&'static str, String)>;

/// The preset files of `dir`: (name, text) for each read, (name, why) for each not.
pub fn read_files(dir: &Path) -> (Named, Named) {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for (label, file) in PRESETS {
        let Some(path) = fnv_install::child_ci(dir, file).filter(|p| p.is_file()) else {
            missing.push((label, format!("{file} not found")));
            continue;
        };
        match std::fs::read(&path) {
            Ok(bytes) => found.push((label, decode(&bytes))),
            Err(e) => missing.push((label, format!("{file} not read: {e}"))),
        }
    }
    (found, missing)
}

/// A preset file's text [design: #1966 review]. The install's files are plain 8-bit text,
/// keys and numbers ASCII [documented: the four files]; a file saved again by an editor may
/// carry a byte order mark, which would hide the first `[Display]` heading from the INI
/// reader. So: a UTF-8 mark is dropped; a UTF-16 mark (FF FE little end first, FE FF big
/// end first) decodes the rest as UTF-16; anything else is read as UTF-8, a byte that is
/// not (the code page's letters, only ever in comments or values) becoming U+FFFD, which no
/// key or number holds. A file in UTF-16 without its mark reads as no keys at all, and
/// [`build`] then says its preset sets no row.
pub fn decode(bytes: &[u8]) -> String {
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&pair| if le { u16::from_le_bytes(pair) } else { u16::from_be_bytes(pair) }).collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// `[section] key` of `text` as a finite number.
fn number(text: &str, section: &str, key: &str) -> Option<f32> {
    fnv_install::ini_get(text, section, key)?.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

/// The Shadow filtering row's part for a preset's number ([`Map::ShadowFilter`]).
fn shadow_filter_part(v: f32) -> f32 {
    use bevy::light::ShadowFilteringMethod as M;
    let method = if v.round() == 0.0 { M::Hardware2x2 } else { M::Gaussian };
    let i = graphics::SHADOW_FILTERS.iter().position(|&(_, m)| m == method).unwrap_or(1);
    choice_part(i, graphics::SHADOW_FILTERS.len())
}

/// The presets of the files read (`files`: name and text, in [`PRESETS`]' order), and what
/// in them is not taken: a mapped key whose value is not a number, a scaled key no file
/// gives a number above 0, a key that is neither mapped nor known as unused. A key a file
/// lacks leaves its row out of that preset.
pub fn build(files: &[(&'static str, String)]) -> (Vec<QualityPreset>, Vec<String>) {
    let mut problems = Vec::new();
    // The largest number of each scaled key over the files.
    let most: Vec<Option<f32>> = MAPPED
        .iter()
        .map(|m| {
            (m.map == Map::Part).then(|| files.iter().filter_map(|(_, t)| number(t, m.section, m.key)).fold(f32::MIN, f32::max)).filter(|v| *v > 0.0)
        })
        .collect();
    for (m, top) in MAPPED.iter().zip(&most) {
        let given = files.iter().any(|(_, t)| fnv_install::ini_get(t, m.section, m.key).is_some());
        if m.map == Map::Part && top.is_none() && given {
            problems.push(format!("[{}] {} is above 0 in no preset file, so {} is not set by any", m.section, m.key, m.row));
        }
    }
    let presets = files
        .iter()
        .map(|(label, text)| {
            let mut rows = Vec::new();
            for (m, top) in MAPPED.iter().zip(&most) {
                let Some(raw) = fnv_install::ini_get(text, m.section, m.key) else { continue };
                let Some(v) = raw.trim().parse::<f32>().ok().filter(|v| v.is_finite()) else {
                    problems.push(format!("{label}: [{}] {}={} is not a number; {} is left out", m.section, m.key, raw.trim(), m.row));
                    continue;
                };
                let value = match m.map {
                    Map::Switch => f32::from(u8::from(v != 0.0)),
                    Map::ShadowFilter => shadow_filter_part(v),
                    Map::Part => match top {
                        Some(top) => (v / top).clamp(0.0, 1.0),
                        None => continue,
                    },
                };
                rows.push((m.row, value));
            }
            for (section, key) in keys(text) {
                let known = MAPPED.iter().any(|m| m.section.eq_ignore_ascii_case(&section) && m.key.eq_ignore_ascii_case(&key))
                    || UNUSED.iter().any(|(s, k)| s.eq_ignore_ascii_case(&section) && k.eq_ignore_ascii_case(&key));
                if !known {
                    problems.push(format!("{label}: [{section}] {key} is not a key the presets are known to set; not used"));
                }
            }
            if rows.is_empty() {
                problems.push(format!("{label}: none of the keys that set a row is in it (not an INI of the presets, or an encoding not read); it sets no row"));
            }
            QualityPreset { label, rows, choices: own_choices(label) }
        })
        .collect();
    (presets, problems)
}

/// Every `[section] key` of an INI text, in order.
fn keys(text: &str) -> Vec<(String, String)> {
    let mut section = String::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with(';') || t.starts_with('#') {
            continue;
        }
        if let Some(s) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = s.trim().to_string();
        } else if let Some((k, _)) = t.split_once('=') {
            out.push((section.clone(), k.trim().to_string()));
        }
    }
    out
}

/// Whether row `key` holds `target` now: a slider by its value on its steps, a switch by
/// on or off; `None` when the row is not registered this run.
fn holds(world: &World, settings: &Settings, key: &str, target: f32) -> Option<bool> {
    let i = settings.index(key)?;
    let s = settings.list()[i];
    // A list (a row that was a slider, the menu overhaul): its choice the value's.
    if let Some(c) = settings.choices(i) {
        let names = (c.names)(world);
        let now = (c.chosen)(world);
        return Some(names.get(choice_index(target, names.len())).is_some_and(|n| *n == now));
    }
    Some(match s.slider {
        Some(sl) => sl.fit((sl.get)(world)) == sl.fit(target),
        None => (s.get)(world) == (target > 0.5),
    })
}

/// Whether list row `key` shows the choice `name` now; `None` when the row is not
/// registered this run.
fn holds_choice(world: &World, settings: &Settings, key: &str, name: &str) -> Option<bool> {
    let c = settings.choices(settings.index(key)?)?;
    Some((c.chosen)(world) == name)
}

/// Whether `preset` leaves row `key` greyed: one of the rows it needs to be on
/// (`nvf_play::settings_layout::NEEDS`: Shadow Filtering and Shadow Distance need Sun
/// Shadows) is a row of the preset that the preset turns off. A row counts as on as the
/// page counts it (`Settings::disabled_status`): a switch on, a list past its first choice.
fn greyed_by(world: &World, settings: &Settings, preset: &QualityPreset, key: &str) -> bool {
    nvf_play::settings_layout::NEEDS.iter().filter(|(child, _, _)| *child == key).any(|(_, parent, _)| {
        preset.rows.iter().filter(|(k, _)| k == parent).any(|(_, v)| {
            let Some(i) = settings.index(parent) else { return false };
            match settings.choices(i) {
                Some(c) => choice_index(*v, (c.names)(world).len()) == 0,
                None => *v < 0.5,
            }
        })
    })
}

/// What the Quality row reads: the first preset whose rows (those registered) all hold its
/// values, its own rows' choices ([`OWN`]) included, else [`CUSTOM`]. A preset none of
/// whose rows is registered matches nothing.
///
/// A row the preset itself leaves greyed is not asked ([`greyed_by`]) [design, #3357]: with
/// Sun Shadows already off, taking a preset that keeps them off cannot set Shadow Filtering
/// or Shadow Distance (a greyed row takes no edit), so whatever they held before stayed and
/// the row read Custom straight after Low was taken [observed: smoke
/// `quality-main-menu-measure`, which passed once and failed on every run after, as its
/// first run leaves Sun Shadows off in its home]. What a greyed row holds draws nothing;
/// the next preset that turns its parent on sets it ([`pick_quality`]'s second pass).
pub fn quality_reading(world: &World) -> &'static str {
    let (Some(settings), Some(p)) = (world.get_resource::<Settings>(), world.get_resource::<QualityPresets>()) else { return CUSTOM };
    p.presets
        .iter()
        .find(|preset| {
            let mut held: Vec<bool> = preset.rows.iter().filter(|(k, _)| !greyed_by(world, settings, preset, k)).filter_map(|(k, v)| holds(world, settings, k, *v)).collect();
            held.extend(preset.choices.iter().filter_map(|(k, name)| holds_choice(world, settings, k, name)));
            !held.is_empty() && held.iter().all(|h| *h)
        })
        .map_or(CUSTOM, |preset| preset.label)
}

fn quality_names(world: &World) -> Vec<String> {
    let mut names: Vec<String> = world.get_resource::<QualityPresets>().map_or(Vec::new(), |p| p.presets.iter().map(|p| p.label.to_string()).collect());
    if quality_reading(world) == CUSTOM {
        names.push(CUSTOM.to_string());
    }
    names
}

fn quality_chosen(world: &World) -> String {
    quality_reading(world).to_string()
}

/// Take preset `name`: each of its rows set through `nvf_play::settings` as a change on
/// the page would (logged `settings: <key> <value>`, kept in the INI, a renderer row's
/// pipelines waited for on resume). False: not a preset offered (Custom is not taken).
pub fn pick_quality(world: &mut World, name: &str) -> bool {
    let Some(p) = world.get_resource::<QualityPresets>() else { return false };
    let Some(preset) = p.presets.iter().find(|p| p.label.eq_ignore_ascii_case(name.trim())).cloned() else { return false };
    let dir = p.dir.clone();
    let mut set = 0;
    // A row that needs another (`nvf_play::settings_layout::NEEDS`: Shadow Filtering needs
    // Sun Shadows) before it, so it takes its value while its parent is still on; and twice
    // over, so one whose parent was off takes it once the parent is on.
    let mut rows = preset.rows.clone();
    rows.sort_by_key(|(k, _)| !nvf_play::settings_layout::NEEDS.iter().any(|(child, _, _)| child == k));
    for pass in 0..2 {
        for (key, v) in &rows {
            let Some(i) = world.resource::<Settings>().index(key) else { continue };
            let s = world.resource::<Settings>().list()[i];
            if let Some(c) = world.resource::<Settings>().choices(i) {
                let names = (c.names)(world);
                if let Some(name) = names.get(choice_index(*v, names.len())) {
                    settings::pick(world, i, name);
                }
            } else if s.slider.is_some() {
                settings::set_slider(world, i, *v);
            } else if (s.get)(world) != (*v > 0.5) {
                settings::flip(world, i);
            }
            if pass == 0 {
                set += 1;
            }
        }
    }
    // NVForever's own rows ([`OWN`]): each takes its choice as a pick on the page would.
    for (key, name) in &preset.choices {
        let Some(i) = world.resource::<Settings>().index(key) else { continue };
        set += 1;
        if settings::pick(world, i, name) && *key == nvf_play::smooth_terrain::KEY {
            info!("quality presets: {}: Smooth terrain {name}, from the next start", preset.label);
        }
    }
    info!("quality presets: {} from {}: {set} row(s) set", preset.label, dir.display());
    true
}

/// A stored name: one the row can read is taken as it is and sets nothing (the rows are
/// kept on their own; module notes).
fn load_quality(_world: &mut World, text: &str) -> bool {
    let t = text.trim();
    PRESETS.iter().any(|(label, _)| label.eq_ignore_ascii_case(t)) || t.eq_ignore_ascii_case(CUSTOM)
}

/// The switches `--graphics enhanced` turns on and `vanilla` leaves off, by settings key
/// (`graphics::KEYS` that are switches and differ between the two looks).
#[cfg(test)]
pub fn enhanced_keys() -> Vec<&'static str> {
    let (mut on, mut off) = (Graphics::preset(Look::Enhanced), Graphics::preset(Look::Vanilla));
    graphics::KEYS
        .into_iter()
        .filter(|k| match (on.switch(k).map(|b| *b), off.switch(k).map(|b| *b)) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        })
        .collect()
}

/// The folder the presets are read from: the test aid's, else the game folder (the
/// install's root, `--data`'s parent).
fn presets_dir(args: &Args) -> PathBuf {
    if let Some(d) = std::env::var_os(PRESETS_ENV).filter(|d| !d.is_empty()) {
        info!("test aid {PRESETS_ENV}: the quality presets from {}", Path::new(&d).display());
        return PathBuf::from(d);
    }
    let data = args.data();
    fnv_install::GameInstall::open(data).map(|g| g.root).unwrap_or_else(|_| data.parent().unwrap_or(data).to_path_buf())
}

/// What the Quality row reads, last logged.
#[derive(Resource, Default)]
struct LastReading(Option<&'static str>);

/// Log what the rows read when it changes (at the start, then while the pause menu is
/// open): `quality presets: the Quality row reads Custom`.
fn log_reading(world: &mut World) {
    let quality = world.contains_resource::<QualityPresets>().then(|| quality_reading(world));
    let mut last = world.get_resource_or_insert_with(LastReading::default);
    if let Some(q) = quality.filter(|q| last.0 != Some(*q)) {
        last.0 = Some(q);
        info!("quality presets: the Quality row reads {q}");
    }
}

fn paused(p: Option<Res<nvf_play::pause::Paused>>) -> bool {
    p.is_some_and(|p| p.0)
}

/// Register, when the install has its preset files, the Quality row (`group`: the Graphics
/// rows' group), after the rows it sets so the start can tell what it reads. The Enhanced
/// effects row that set the effect rows at once is gone: the owner, 2026-11-15, "no need to
/// have it twice" (the menu overhaul); each effect keeps its own row.
pub fn register(app: &mut App, group: &'static str) {
    app.add_systems(PostStartup, log_reading.after(settings::apply_stored));
    app.add_systems(Update, log_reading.run_if(paused));
    let dir = presets_dir(app.world().resource::<Args>());
    let (files, missing) = read_files(&dir);
    if files.is_empty() {
        let names: Vec<&str> = PRESETS.iter().map(|(_, f)| *f).collect();
        info!("quality presets: none of {} in {}: the Quality row is not shown", names.join(", "), dir.display());
        return;
    }
    for (label, why) in &missing {
        info!("quality presets: {label} is not offered: {why} in {}", dir.display());
    }
    let (presets, problems) = build(&files);
    for p in &problems {
        warn!("quality presets: {p}");
    }
    let offered: Vec<&str> = presets.iter().map(|p| p.label).collect();
    info!("quality presets: {} from {}; {} key(s) set rows, {} not used", offered.join(", "), dir.display(), MAPPED.len(), UNUSED.len());
    app.insert_resource(QualityPresets { dir, presets });
    add_choice_list(
        app,
        Setting {
            key: QUALITY_KEY,
            group,
            label: "Quality preset",
            note: "Sets the rows the quality presets cover to the game folder's low.ini to VeryHigh.ini, and Smooth terrain (Off at Low and Medium). Custom: none match.",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList { names: quality_names, chosen: quality_chosen, pick: pick_quality, save: quality_chosen, load: load_quality, ask_keep: false },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvf_play::settings::{add_setting, Slider};

    // Made-up preset files for the tests: not any install's values.
    const LOW: &str = "[Display]\nbDrawShadows=0\niShadowFilter=0\nfShadowLODStartFade=250\niMaxAnisotropy=3\n[LOD]\nfLODFadeOutMultActors=1\nfLODFadeOutMultObjects=2\n[Grass]\nfGrassStartFadeDistance=0\n[Water]\nbUseWaterReflections=0\n[BlurShaderHDR]\nbDoHighDynamicRange=0\n";
    const HIGH: &str = "[Display]\nbDrawShadows=1\niShadowFilter=3\nfShadowLODStartFade=1000\n[LOD]\nfLODFadeOutMultActors=4\nfLODFadeOutMultObjects=8\nfLODFadeOutMultItems=6\n[Grass]\nfGrassStartFadeDistance=2000\n[Water]\nbUseWaterReflections=1\n[BlurShaderHDR]\nbDoHighDynamicRange=1\n";

    #[test]
    fn the_mapped_and_unused_keys_are_41_each_once_and_no_effect_is_a_quality_row() {
        let mut all: Vec<(String, String)> = MAPPED.iter().map(|m| (m.section.to_ascii_lowercase(), m.key.to_ascii_lowercase())).collect();
        all.extend(UNUSED.iter().map(|(s, k)| (s.to_ascii_lowercase(), k.to_ascii_lowercase())));
        let n = all.len();
        all.sort();
        all.dedup();
        assert_eq!((n, all.len()), (41, 41));
        let effects = enhanced_keys();
        assert_eq!(effects, ["ambient-occlusion", "fine-shadows", "bloom", "sharpen", "anisotropic", "light-shafts", "water-reflections", "water-effects"]);
        for m in MAPPED {
            assert!(!effects.contains(&m.row), "{} is an enhanced effect", m.row);
            let (mut a, mut b) = (Graphics::preset(Look::Enhanced), Graphics::preset(Look::Vanilla));
            assert_eq!(a.switch(m.row).map(|x| *x), b.switch(m.row).map(|x| *x), "{} is the same in both looks", m.row);
        }
    }

    #[test]
    fn each_key_maps_to_its_row_and_a_slider_by_the_fullest_preset() {
        let (presets, problems) = build(&[("Low", LOW.into()), ("High", HIGH.into())]);
        let low = &presets[0];
        let get = |p: &QualityPreset, k: &str| p.rows.iter().find(|(r, _)| *r == k).map(|(_, v)| *v);
        assert_eq!(get(low, "sun-shadows"), Some(0.0));
        assert_eq!(get(low, "shadow-filter"), Some(0.0), "0: Fast");
        assert_eq!(get(&presets[1], "shadow-filter"), Some(0.5), "any other number: Smooth");
        assert_eq!(get(low, "shadow-distance"), Some(0.25), "250 of the fullest preset's 1000");
        assert_eq!(get(low, "actor-distance"), Some(0.25));
        assert_eq!(get(low, "object-distance"), Some(0.25));
        assert_eq!(get(low, "item-distance"), None, "a key the file lacks leaves its row alone");
        assert_eq!(get(low, "grass-distance"), Some(0.0), "the row brings it onto its range");
        assert_eq!(get(low, "water-mirror"), Some(0.0));
        assert_eq!(get(low, "glow"), Some(0.0));
        assert!(presets[1].rows.iter().filter(|(r, _)| r.ends_with("-distance")).all(|(_, v)| *v == 1.0), "the fullest is 100%");
        assert!(problems.is_empty(), "iMaxAnisotropy is known, not used: {problems:?}");
        // A key no list knows, and a value that is not a number, are said and left out.
        let odd = format!("{LOW}[Madeup]\nfNothing=1\n").replace("bDrawShadows=0", "bDrawShadows=maybe");
        let (presets, problems) = build(&[("Low", odd)]);
        assert_eq!(get(&presets[0], "sun-shadows"), None);
        assert!(problems.iter().any(|p| p.contains("bDrawShadows=maybe is not a number")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("[Madeup] fNothing is not a key")), "{problems:?}");
        // Alone, the low file is its own fullest; a scaled key at 0 everywhere sets nothing.
        let (presets, problems) = build(&[("Low", LOW.into())]);
        assert_eq!(get(&presets[0], "shadow-distance"), Some(1.0));
        assert_eq!(get(&presets[0], "grass-distance"), None);
        assert!(problems.iter().any(|p| p.contains("fGrassStartFadeDistance is above 0 in no preset file")), "{problems:?}");
    }

    #[test]
    fn a_missing_file_leaves_its_preset_out_and_none_leaves_the_row_out() {
        let dir = std::env::temp_dir().join(format!("nvf-quality-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (found, missing) = read_files(&dir);
        assert!(found.is_empty());
        assert_eq!(missing.iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["Low", "Medium", "High", "Very High"]);
        // Matched without case, as a copy on a case-sensitive file system may name it.
        std::fs::write(dir.join("LOW.INI"), LOW).unwrap();
        std::fs::write(dir.join("VeryHigh.ini"), HIGH).unwrap();
        let (found, missing) = read_files(&dir);
        assert_eq!(found.iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["Low", "Very High"]);
        assert_eq!(missing, [("Medium", "medium.ini not found".to_string()), ("High", "high.ini not found".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1966 review: a byte order mark before the first heading must not hide `[Display]`.
    #[test]
    fn a_byte_order_mark_does_not_hide_the_first_section() {
        let dir = std::env::temp_dir().join(format!("nvf-quality-bom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(LOW.as_bytes());
        std::fs::write(dir.join("low.ini"), bom).unwrap();
        let (found, _) = read_files(&dir);
        assert!(found[0].1.starts_with("[Display]"), "the mark is dropped");
        let (presets, problems) = build(&found);
        let rows: Vec<&str> = presets[0].rows.iter().map(|(r, _)| *r).collect();
        for r in ["shadow-distance", "sun-shadows", "shadow-filter"] {
            assert!(rows.contains(&r), "[Display]'s {r} is read: {rows:?}");
        }
        // Read as the same file without the mark: the same rows and the same remarks (the
        // low file alone has grass at 0, so its grass is said to be set by none either way).
        let (plain, plain_problems) = build(&[("Low", LOW.into())]);
        assert_eq!(presets[0].rows, plain[0].rows);
        assert_eq!(problems, plain_problems);
        assert!(!problems.iter().any(|p| p.contains("[Display]") || p.contains("sets no row")), "{problems:?}");
        let _ = std::fs::remove_dir_all(&dir);
        // UTF-16 with its mark, either end first, reads the same; code-page letters in a
        // comment cost nothing.
        let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain(LOW.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain(LOW.encode_utf16().flat_map(u16::to_be_bytes)).collect();
        assert_eq!((decode(&le), decode(&be)), (LOW.to_string(), LOW.to_string()));
        let mut cp = b"; caf\xE9\n".to_vec();
        cp.extend_from_slice(LOW.as_bytes());
        assert_eq!(build(&[("Low", decode(&cp))]).0[0].rows, build(&[("Low", LOW.into())]).0[0].rows);
        // UTF-16 without a mark reads as no keys, and the log says the preset sets nothing.
        let bare: Vec<u8> = LOW.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let (presets, problems) = build(&[("Low", decode(&bare))]);
        assert!(presets[0].rows.is_empty());
        assert!(problems.iter().any(|p| p.contains("it sets no row")), "{problems:?}");
    }

    #[derive(Resource, Default)]
    struct Rows {
        shadows: bool,
        glow: bool,
        mirror: bool,
        filter: f32,
        parts: [f32; 5],
    }

    fn switch(key: &'static str, get: fn(&World) -> bool, set: fn(&mut World, bool)) -> Setting {
        Setting { key, group: "Graphics", label: key, note: "", switches: "", get, set, slider: None }
    }

    fn part(key: &'static str, n: usize) -> Setting {
        let gets: [fn(&World) -> f32; 5] = [|w| w.resource::<Rows>().parts[0], |w| w.resource::<Rows>().parts[1], |w| w.resource::<Rows>().parts[2], |w| w.resource::<Rows>().parts[3], |w| w.resource::<Rows>().parts[4]];
        let get = gets[n];
        let sets: [fn(&mut World, f32); 5] = [
            |w: &mut World, v| w.resource_mut::<Rows>().parts[0] = v,
            |w: &mut World, v| w.resource_mut::<Rows>().parts[1] = v,
            |w: &mut World, v| w.resource_mut::<Rows>().parts[2] = v,
            |w: &mut World, v| w.resource_mut::<Rows>().parts[3] = v,
            |w: &mut World, v| w.resource_mut::<Rows>().parts[4] = v,
        ];
        let set = sets[n];
        Setting::slider(key, "Graphics", key, "", Slider { get, set, min: 0.1, step: 0.05, shown: None })
    }

    /// The test's Smooth terrain row: a list of the row's own names.
    #[derive(Resource)]
    struct Smooth(&'static str);

    const SMOOTH_NAMES: [&str; 4] = ["Off", "2 x 2", "4 x 4", "4 x 4 near"];

    fn pick_smooth(w: &mut World, name: &str) -> bool {
        let Some(n) = SMOOTH_NAMES.iter().find(|n| **n == name) else { return false };
        w.resource_mut::<Smooth>().0 = n;
        true
    }

    /// The mapped rows at today's defaults (all on, Smooth, every distance 100%, Smooth
    /// terrain 4 x 4) with the Quality row over made-up presets.
    fn app(files: &[(&'static str, &str)]) -> App {
        let mut app = App::new();
        app.insert_resource(Smooth("4 x 4"));
        add_choice_list(
            &mut app,
            Setting { key: nvf_play::smooth_terrain::KEY, group: "Graphics", label: "s", note: "", switches: "", get: |_| false, set: |_, _| {}, slider: None },
            ChoiceList {
                names: |_| SMOOTH_NAMES.iter().map(|n| n.to_string()).collect(),
                chosen: |w| w.resource::<Smooth>().0.to_string(),
                pick: pick_smooth,
                save: |w| w.resource::<Smooth>().0.to_string(),
                load: pick_smooth,
                ask_keep: false,
            },
        );
        app.insert_resource(Rows { shadows: true, glow: true, mirror: true, filter: 0.5, parts: [1.0; 5] });
        add_setting(&mut app, switch("sun-shadows", |w| w.resource::<Rows>().shadows, |w, on| w.resource_mut::<Rows>().shadows = on));
        add_setting(&mut app, switch("glow", |w| w.resource::<Rows>().glow, |w, on| w.resource_mut::<Rows>().glow = on));
        add_setting(&mut app, switch("water-mirror", |w| w.resource::<Rows>().mirror, |w, on| w.resource_mut::<Rows>().mirror = on));
        add_setting(
            &mut app,
            Setting::slider("shadow-filter", "Graphics", "shadow-filter", "", Slider::choice(3, |w| w.resource::<Rows>().filter, |w, v| w.resource_mut::<Rows>().filter = v, |v| format!("{v}"))),
        );
        for (n, key) in ["object-distance", "actor-distance", "item-distance", "shadow-distance", "grass-distance"].into_iter().enumerate() {
            add_setting(&mut app, part(key, n));
        }
        let files: Vec<(&'static str, String)> = files.iter().map(|(l, t)| (*l, t.to_string())).collect();
        let (presets, _) = build(&files);
        app.insert_resource(QualityPresets { dir: PathBuf::from("made-up"), presets });
        add_choice_list(
            &mut app,
            Setting { key: QUALITY_KEY, group: "Graphics", label: "q", note: "", switches: "", get: |_| false, set: |_, _| {}, slider: None },
            ChoiceList { names: quality_names, chosen: quality_chosen, pick: pick_quality, save: quality_chosen, load: load_quality, ask_keep: false },
        );
        app
    }

    #[test]
    fn a_preset_sets_its_rows_and_any_change_to_one_reads_custom() {
        let mut app = app(&[("Low", LOW), ("Very High", HIGH)]);
        let w = app.world_mut();
        assert_eq!(quality_reading(w), "Very High", "today's defaults match the fullest made-up preset");
        assert_eq!(quality_names(w), ["Low", "Very High"], "Custom is offered only while it is read");
        let row = w.resource::<Settings>().index(QUALITY_KEY).unwrap();
        assert!(settings::pick(w, row, "Low"));
        let r = w.resource::<Rows>();
        assert_eq!((r.shadows, r.glow, r.mirror, r.filter), (false, false, false, 0.0));
        assert_eq!(r.parts, [0.25, 0.25, 1.0, 0.25, 0.1], "item-distance is not in the low file; grass is on the slider's least");
        assert_eq!(quality_reading(w), "Low");
        // A covered row changed: Custom, offered as the one chosen; back again: Low.
        let obj = w.resource::<Settings>().index("object-distance").unwrap();
        settings::set_slider(w, obj, 0.5);
        assert_eq!(quality_reading(w), CUSTOM);
        assert_eq!(quality_names(w), ["Low", "Very High", "Custom"]);
        assert!(!settings::pick(w, row, CUSTOM), "Custom is read, not taken");
        settings::set_slider(w, obj, 0.25);
        assert_eq!(quality_reading(w), "Low");
        // #3357: Low leaves Sun Shadows off, which greys Shadow Filtering and Shadow
        // Distance; what those hold then (left from before, as a greyed row takes no edit)
        // does not make the row read Custom, and Low can be taken again over it.
        w.resource_mut::<Rows>().filter = 0.5;
        w.resource_mut::<Rows>().parts[3] = 0.75;
        assert_eq!(quality_reading(w), "Low", "greyed rows are not asked");
        assert!(!settings::pick(w, row, "Low"), "it is the choice already");
        // With Sun Shadows on they are asked again.
        w.resource_mut::<Rows>().shadows = true;
        assert_eq!(quality_reading(w), CUSTOM);
        // A preset that turns Sun Shadows on sets them (the second pass), from that state too.
        w.resource_mut::<Rows>().shadows = false;
        assert!(settings::pick(w, row, "Very High"));
        let r = w.resource::<Rows>();
        assert_eq!((r.shadows, r.filter, r.parts[3]), (true, 0.5, 1.0));
        assert_eq!(quality_reading(w), "Very High");
        assert!(settings::pick(w, row, "Low"));
        assert_eq!(quality_reading(w), "Low");
        // A row no preset covers changes nothing.
        let sun = w.resource::<Settings>().index("sun-shadows").unwrap();
        settings::flip(w, sun);
        assert_eq!(quality_reading(w), CUSTOM, "sun-shadows is covered");
        settings::flip(w, sun);
        assert!(settings::pick(w, row, "very high"), "a name in any case");
        assert_eq!(w.resource::<Rows>().parts, [1.0; 5]);
        assert_eq!(quality_reading(w), "Very High");
        assert!(!settings::pick(w, row, "Medium"), "a preset without its file is not offered");
    }

    /// #2248: a preset sets its nine rows and writes `NVForever.ini` once, not once per row
    /// (each write cost ~27 ms on the main thread in the measured run).
    #[test]
    fn a_preset_writes_the_ini_once() {
        let dir = std::env::temp_dir().join(format!("nvf-quality-writes-{}", std::process::id()));
        let path = dir.join("NVForever.ini");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = app(&[("Low", LOW), ("Very High", HIGH)]);
        app.insert_resource(settings::SettingsFile(Some(path.clone()))).init_resource::<settings::StoreAttempts>();
        let w = app.world_mut();
        let writes = |w: &World| w.resource::<settings::StoreAttempts>().0.load(std::sync::atomic::Ordering::Relaxed);
        let row = w.resource::<Settings>().index(QUALITY_KEY).unwrap();
        assert!(settings::pick(w, row, "Low"));
        assert_eq!(writes(w), 1, "one write for the preset and every row it set");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("quality=Low") && text.contains("sun-shadows=0") && text.contains("object-distance=0.25"), "{text}");
        let obj = w.resource::<Settings>().index("object-distance").unwrap();
        settings::set_slider(w, obj, 0.5);
        assert_eq!(writes(w), 2, "a row alone: one write");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #2864: the presets set Smooth terrain, a row of NVForever's own that no preset file
    /// names: Off at Low and Medium, 4 x 4 at High and Very High; changed by hand, Custom.
    #[test]
    fn the_presets_set_smooth_terrain() {
        // The table: every preset names the row once, with a choice the row offers.
        for (preset, _) in PRESETS {
            let own = own_choices(preset);
            assert_eq!(own.len(), 1, "{preset}");
            assert_eq!(own[0].0, nvf_play::smooth_terrain::KEY);
            assert!(SMOOTH_NAMES.contains(&own[0].1), "{preset}: {}", own[0].1);
        }
        let choice = |p: &str| own_choices(p)[0].1;
        assert_eq!((choice("Low"), choice("Medium"), choice("High"), choice("Very High")), ("Off", "Off", "4 x 4", "4 x 4"));
        for n in SMOOTH_NAMES {
            assert!(nvf_play::smooth_terrain::Smoothing::parse(&n.replace(' ', "").replace("4x4near", "4x4-near").to_ascii_lowercase()).is_some(), "{n} is one of the row's levels");
        }
        // On the page.
        let mut app = app(&[("Low", LOW), ("Very High", HIGH)]);
        let w = app.world_mut();
        let row = w.resource::<Settings>().index(QUALITY_KEY).unwrap();
        assert_eq!(quality_reading(w), "Very High");
        assert!(settings::pick(w, row, "Low"));
        assert_eq!(w.resource::<Smooth>().0, "Off");
        assert_eq!(quality_reading(w), "Low");
        // The row changed by hand: Custom; back: Low; the fullest preset brings 4 x 4 back.
        let smooth = w.resource::<Settings>().index(nvf_play::smooth_terrain::KEY).unwrap();
        assert!(settings::pick(w, smooth, "2 x 2"));
        assert_eq!(quality_reading(w), CUSTOM);
        assert!(settings::pick(w, smooth, "Off"));
        assert_eq!(quality_reading(w), "Low");
        assert!(settings::pick(w, row, "Very High"));
        assert_eq!(w.resource::<Smooth>().0, "4 x 4");
        assert_eq!(quality_reading(w), "Very High");
    }

    #[test]
    fn the_stored_name_is_kept_and_sets_nothing_custom_included() {
        let mut app = app(&[("Low", LOW), ("Very High", HIGH)]);
        let w = app.world_mut();
        for t in ["Low", "Custom", "very high", "Medium"] {
            assert!(load_quality(w, t), "{t}");
        }
        assert!(!load_quality(w, "Ultra"));
        assert_eq!(w.resource::<Rows>().parts, [1.0; 5], "a stored name sets no row: they are kept on their own");
        assert_eq!(quality_chosen(w), "Very High");
    }
}
