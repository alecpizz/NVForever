//! Tonemapping presets (#4690, stage 1; docs/design/tonemapping-presets.md): a few values laid
//! on the main view's own tonemapper curve (lights.rs `ToneCurve`) through Bevy's colour
//! grading on the world camera, which the tonemapping pass runs before the curve. An optional
//! improvement over the original, not the original game's behaviour [design].
//!
//! - **Default** is built in and is Bevy's default grading, which every 3D camera already
//!   carries (bevy_render camera.rs: `Camera3d` requires `ColorGrading`), so the shipped look of
//!   each rendering mode is unchanged.
//! - The other presets are files, one per preset, in `<NVForever home>\Tonemapping\` (read at
//!   the start; [`TonePreset::from_toml`]). The settings row `tonemap-preset` picks the active
//!   one and keeps its name in `NVForever.ini`; a name not on disk falls back to Default.
//! - The eye adaptation still applies: Bevy's auto exposure adds its exposure to the grading's
//!   [documented: engine/vendor/bevy_post_process/src/auto_exposure/auto_exposure.wesl,
//!   `view.color_grading.exposure += exposure`].

use crate::fly::FlyCam;
use bevy::prelude::*;
use bevy::render::view::ColorGrading;
use nvf_play::settings::{add_choice_list, ChoiceList, Setting};
use std::path::{Path, PathBuf};

/// The built-in preset's name; a file of this name is left out.
pub const DEFAULT_NAME: &str = "Default";
/// The file format this build reads and writes.
pub const FORMAT: i64 = 1;
/// The ranges of the values (docs/design/tonemapping-presets.md, the values) [design].
pub const EXPOSURE_RANGE: (f32, f32) = (-3.0, 3.0);
pub const CONTRAST_RANGE: (f32, f32) = (0.5, 1.5);
pub const HIGHLIGHTS_RANGE: (f32, f32) = (0.0, 1.0);
/// How far the highlight roll-off lowers the highlights' gain at its most [design].
pub const HIGHLIGHTS_GAIN_DROP: f32 = 0.5;

/// One preset: its name and its values.
#[derive(Clone, Debug, PartialEq)]
pub struct TonePreset {
    pub name: String,
    /// Exposure compensation in EV, before the curve.
    pub exposure: f32,
    /// Contrast of shadows, midtones and highlights alike (1: none).
    pub contrast: f32,
    /// Highlight roll-off, 0 (none) to 1.
    pub highlights: f32,
}

impl TonePreset {
    /// The built-in preset: no change.
    pub fn default_preset() -> Self {
        Self { name: DEFAULT_NAME.into(), exposure: 0.0, contrast: 1.0, highlights: 0.0 }
    }

    /// The grading the preset lays on the world camera. Default's is Bevy's default.
    pub fn grading(&self) -> ColorGrading {
        let mut g = ColorGrading::default();
        g.global.exposure = self.exposure;
        for section in [&mut g.shadows, &mut g.midtones, &mut g.highlights] {
            section.contrast = self.contrast;
        }
        g.highlights.gain = 1.0 - HIGHLIGHTS_GAIN_DROP * self.highlights;
        g
    }

    /// A preset file's text: `format`, `name` and the values (module notes). A value out of its
    /// range is held to it, a missing one takes Default's; the file's name is the fallback name.
    pub fn from_toml(text: &str, file_stem: &str) -> Result<Self, String> {
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.message().to_string())?;
        match table.get("format") {
            Some(toml::Value::Integer(FORMAT)) => {}
            Some(toml::Value::Integer(n)) => return Err(format!("format {n}, this build reads format {FORMAT}")),
            Some(_) => return Err("format is not a whole number".into()),
            None => return Err("no format line".into()),
        }
        let name = match table.get("name") {
            Some(toml::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            Some(toml::Value::String(_)) | None => file_stem.trim().to_string(),
            Some(_) => return Err("name is not text".into()),
        };
        if name.eq_ignore_ascii_case(DEFAULT_NAME) {
            return Err(format!("the name {DEFAULT_NAME} is the built-in preset's"));
        }
        let d = Self::default_preset();
        let value = |key: &str, default: f32, (lo, hi): (f32, f32)| -> Result<f32, String> {
            match table.get(key) {
                None => Ok(default),
                Some(toml::Value::Float(f)) if f.is_finite() => Ok((*f as f32).clamp(lo, hi)),
                Some(toml::Value::Integer(i)) => Ok((*i as f32).clamp(lo, hi)),
                Some(_) => Err(format!("{key} is not a number")),
            }
        };
        Ok(Self {
            name,
            exposure: value("exposure", d.exposure, EXPOSURE_RANGE)?,
            contrast: value("contrast", d.contrast, CONTRAST_RANGE)?,
            highlights: value("highlights", d.highlights, HIGHLIGHTS_RANGE)?,
        })
    }

    /// The preset as a file's text (the format [`Self::from_toml`] reads).
    pub fn to_toml(&self) -> String {
        let name = toml::Value::String(self.name.clone());
        format!(
            "# NVForever tonemapping preset (docs/design/tonemapping-presets.md)\nformat = {FORMAT}\nname = {name}\nexposure = {:?}\ncontrast = {:?}\nhighlights = {:?}\n",
            self.exposure, self.contrast, self.highlights
        )
    }
}

/// The presets' folder in a home.
pub fn presets_dir(home: &Path) -> PathBuf {
    home.join("Tonemapping")
}

/// The presets in `dir`, by name (case kept, sorted without regard to case), the unreadable
/// ones with why. Two files of one name: the first in file-name order is kept.
pub fn read_dir(dir: &Path) -> (Vec<TonePreset>, Vec<(PathBuf, String)>) {
    let mut found = Vec::new();
    let mut bad = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return (found, bad) };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("toml"))).collect();
    paths.sort();
    for path in paths {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        let parsed = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| TonePreset::from_toml(&t, &stem));
        match parsed {
            Ok(p) if found.iter().any(|q: &TonePreset| q.name.eq_ignore_ascii_case(&p.name)) => bad.push((path, format!("the name {} is taken by an earlier file", p.name))),
            Ok(p) => found.push(p),
            Err(e) => bad.push((path, e)),
        }
    }
    found.sort_by_key(|p| p.name.to_lowercase());
    (found, bad)
}

/// The presets this run knows and the active one.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct TonePresets {
    /// The file presets (Default is not among them).
    pub presets: Vec<TonePreset>,
    /// The active preset's name.
    pub active: String,
}

impl Default for TonePresets {
    fn default() -> Self {
        Self { presets: Vec::new(), active: DEFAULT_NAME.into() }
    }
}

impl TonePresets {
    /// Default, then the files' presets.
    pub fn names(&self) -> Vec<String> {
        std::iter::once(DEFAULT_NAME.to_string()).chain(self.presets.iter().map(|p| p.name.clone())).collect()
    }

    /// The preset of this name (any case), Default among them.
    pub fn find(&self, name: &str) -> Option<TonePreset> {
        if name.eq_ignore_ascii_case(DEFAULT_NAME) {
            return Some(TonePreset::default_preset());
        }
        self.presets.iter().find(|p| p.name.eq_ignore_ascii_case(name)).cloned()
    }

    /// The active preset (Default when its name is gone).
    pub fn active_preset(&self) -> TonePreset {
        self.find(&self.active).unwrap_or_else(TonePreset::default_preset)
    }

    /// Make the preset of this name active; false: no such preset.
    pub fn pick(&mut self, name: &str) -> bool {
        match self.find(name) {
            Some(p) => {
                self.active = p.name;
                true
            }
            None => false,
        }
    }
}

pub struct TonePresetsPlugin;

impl Plugin for TonePresetsPlugin {
    fn build(&self, app: &mut App) {
        let mut state = TonePresets::default();
        if let Some(home) = fnv_install::EngineHome::default_root() {
            let dir = presets_dir(&home);
            seed_test_preset(&dir);
            let (presets, bad) = read_dir(&dir);
            for (path, why) in &bad {
                warn!("tonemapping presets: {} left out: {why}", path.display());
            }
            if !presets.is_empty() {
                info!("tonemapping presets: {} in {}", presets.len(), dir.display());
            }
            state.presets = presets;
        }
        app.insert_resource(state).add_systems(PostUpdate, apply_preset);
        add_choice_list(
            app,
            Setting {
                key: "tonemap-preset",
                group: "Graphics",
                label: "Tonemapping preset",
                note: "A look laid on the tonemapper: exposure, contrast and highlight roll-off. Default keeps the shipped look. Presets are files in the Tonemapping folder of NVForever's home.",
                switches: "",
                get: |_| false,
                set: |_, _| {},
                slider: None,
            },
            ChoiceList {
                names: |w| w.get_resource::<TonePresets>().map_or_else(|| vec![DEFAULT_NAME.to_string()], TonePresets::names),
                chosen: |w| w.get_resource::<TonePresets>().map_or_else(|| DEFAULT_NAME.to_string(), |s| s.active_preset().name),
                pick: |w, name| w.get_resource_mut::<TonePresets>().is_some_and(|mut s| s.pick(name)),
                save: |w| w.get_resource::<TonePresets>().map_or_else(|| DEFAULT_NAME.to_string(), |s| s.active_preset().name),
                load: |w, text| {
                    let Some(mut s) = w.get_resource_mut::<TonePresets>() else { return false };
                    if !s.pick(text.trim()) {
                        info!("tonemapping presets: {} is not in the folder; Default", text.trim());
                        s.active = DEFAULT_NAME.into();
                    }
                    true
                },
                ask_keep: false,
            },
        );
    }
}

/// The smoke tests' preset (test aid `NVF_PLAY_TEST_TONE_PRESET=1`, only under an explicit
/// `NVFOREVER_HOME`, as the ground pack's aid: never into a player's home).
pub const TEST_PRESET_NAME: &str = "Smoke Dusk";

fn seed_test_preset(dir: &Path) {
    if !std::env::var("NVF_PLAY_TEST_TONE_PRESET").is_ok_and(|v| v.trim() == "1") {
        return;
    }
    if std::env::var_os("NVFOREVER_HOME").is_none() {
        warn!("NVF_PLAY_TEST_TONE_PRESET: needs an explicit NVFOREVER_HOME; nothing written");
        return;
    }
    let preset = TonePreset { name: TEST_PRESET_NAME.into(), exposure: 0.5, contrast: 1.1, highlights: 0.3 };
    let path = dir.join("smoke-dusk.toml");
    match std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, preset.to_toml())) {
        Ok(()) => info!("NVF_PLAY_TEST_TONE_PRESET: wrote {}", path.display()),
        Err(e) => warn!("NVF_PLAY_TEST_TONE_PRESET: {}: {e}", path.display()),
    }
}

/// Lay the active preset's grading on the world camera when it changes or a new world camera
/// appears; logged once a change.
fn apply_preset(state: Res<TonePresets>, mut cams: Query<(Ref<FlyCam>, &mut ColorGrading)>) {
    let mut shown = false;
    for (cam, mut grading) in &mut cams {
        if !state.is_changed() && !cam.is_added() {
            continue;
        }
        let preset = state.active_preset();
        let want = preset.grading();
        if *grading != want {
            *grading = want;
        }
        if !shown {
            info!("tonemapping presets: {} (exposure {:+.2} EV, contrast {:.2}, highlights {:.2})", preset.name, preset.exposure, preset.contrast, preset.highlights);
            shown = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_bevys_default_grading() {
        assert_eq!(TonePreset::default_preset().grading(), ColorGrading::default());
    }

    #[test]
    fn a_preset_sets_exposure_contrast_and_highlight_gain() {
        let p = TonePreset { name: "Dusk".into(), exposure: 0.5, contrast: 1.2, highlights: 0.4 };
        let g = p.grading();
        assert_eq!(g.global.exposure, 0.5);
        assert_eq!((g.shadows.contrast, g.midtones.contrast, g.highlights.contrast), (1.2, 1.2, 1.2));
        assert!((g.highlights.gain - 0.8).abs() < 1e-6);
        assert_eq!((g.shadows.gain, g.midtones.gain), (1.0, 1.0));
    }

    #[test]
    fn the_file_round_trips_and_holds_values_to_their_ranges() {
        let p = TonePreset { name: "Warm \"dusk\"".into(), exposure: 0.3, contrast: 1.1, highlights: 0.2 };
        assert_eq!(TonePreset::from_toml(&p.to_toml(), "x").unwrap(), p);
        let wild = TonePreset::from_toml("format = 1\nexposure = 9\ncontrast = 0\nhighlights = -1\n", "Wild").unwrap();
        assert_eq!((wild.name.as_str(), wild.exposure, wild.contrast, wild.highlights), ("Wild", 3.0, 0.5, 0.0));
        let sparse = TonePreset::from_toml("format = 1\nname = \"Only exposure\"\nexposure = -1\n", "f").unwrap();
        assert_eq!((sparse.contrast, sparse.highlights), (1.0, 0.0), "missing values take Default's");
    }

    #[test]
    fn broken_or_foreign_files_are_refused_with_a_reason() {
        assert!(TonePreset::from_toml("exposure = 1\n", "a").unwrap_err().contains("format"));
        assert!(TonePreset::from_toml("format = 2\n", "a").unwrap_err().contains("format 2"));
        assert!(TonePreset::from_toml("format = 1\nexposure = \"bright\"\n", "a").unwrap_err().contains("exposure"));
        assert!(TonePreset::from_toml("format = 1\nname = \"default\"\n", "a").unwrap_err().contains("built-in"));
        assert!(TonePreset::from_toml("format = [", "a").is_err());
    }

    #[test]
    fn the_folder_lists_good_files_by_name_and_names_the_bad_ones() {
        let dir = std::env::temp_dir().join(format!("nvf-tone-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.toml"), "format = 1\nname = \"bright\"\nexposure = 1\n").unwrap();
        std::fs::write(dir.join("a.toml"), "format = 1\nname = \"Amber\"\n").unwrap();
        std::fs::write(dir.join("c.toml"), "format = 1\nname = \"BRIGHT\"\n").unwrap();
        std::fs::write(dir.join("d.toml"), "not toml [").unwrap();
        std::fs::write(dir.join("e.txt"), "format = 1\n").unwrap();
        let (found, bad) = read_dir(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Amber", "bright"]);
        assert_eq!(bad.len(), 2, "{bad:?}");
        assert!(bad.iter().any(|(p, why)| p.ends_with("c.toml") && why.contains("taken")));
    }

    #[test]
    fn picking_falls_back_to_default_and_finds_any_case() {
        let mut s = TonePresets { presets: vec![TonePreset { name: "Dusk".into(), exposure: 1.0, contrast: 1.0, highlights: 0.0 }], ..Default::default() };
        assert_eq!(s.names(), ["Default", "Dusk"]);
        assert!(s.pick("dusk"));
        assert_eq!(s.active, "Dusk");
        assert!(!s.pick("Noon"));
        assert_eq!(s.active, "Dusk", "an unknown name changes nothing");
        s.active = "Gone".into();
        assert_eq!(s.active_preset(), TonePreset::default_preset());
    }
}
