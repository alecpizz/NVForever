//! Live tuning of the volumetric clouds' debug aids (#3032 stage 2e;
//! docs/design/graphics-enhanced.md §3.23). A development tool, read in a played run (it
//! does not make a run unattended); an engineering choice, not a fact about the original game.
//!
//! `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_TUNE=<file>` names a text file of `NAME=value` lines; while the
//! row is on the game looks at it at most every [`POLL_EVERY`] of real time ([`poll_tune`], in the
//! main world's `Update` before the volume's own systems) and reads it again when its modification
//! time or length changed. Its values override the aids of those names on top of the values read
//! at the start ([`CloudAids`], the `LazyLock` statics of volumetric_clouds.rs snapshotted once):
//! a key removed from the file goes back to its start value; a line that cannot be read (no `=`,
//! an unknown name, a value out of form, a start-only key) is ignored with one log line, again
//! only when that line changed; a file that cannot be read keeps the last good values. A changed
//! effective value drops the medium's history once ([`CloudTune::edits`], counted with the sky's
//! jumps). Without the variable nothing is read and nothing changes.
//!
//! Live ([`TuneKey::live`]): the relief's and the erosion's strengths (`SHAPE`, `EROSION`), the
//! density (`DENSITY`, the aid `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS`), the march's quality (`STEPS`),
//! a type forced everywhere (`TYPE`, `none` for the weather's mix), the debug view (`VIEW`), the
//! light's ten weights (`OCTAVE`, `BACKSCATTER`, `UPPER`, `LOWER`, `GROUND`, `FORWARD`, `SUN`, `LEVEL`,
//! `OCCLUDE`, `POWDER`; 0 to 4, 1 as designed), the sunlight's share in the direct light's colour
//! (`SUNCOLOUR`, alias `SUNCOLOR`, 0 to 1, 0.5 as designed; volumetric_clouds_light.rs), the night's pull
//! toward the cloud layers' luminance (`NIGHTPULL`, 0 to 2, 0.5 as designed) and the phase's weights by
//! moonlight (`MOONFORWARD`, `MOONBACK`, 0 to 4, the day's unless set; the same file) and the wisps
//! (`WISP`, volumetric_clouds_wisp.rs): all of them reach the march through the uniform each frame. Start-only [design]: the domain's
//! height (`HEIGHT`) and the horizon (`HORIZON`), which the cloud map of stage 2d may key on; the noise (its seeds, sizes and
//! baked levels) is no aid. Names may be the short ones or the variables' full names, in any case;
//! `#` starts a comment; a later line for the same key wins.
//!
//! The module is compiled only with the developer aids (dev_aids.rs): a release build has no tune
//! file, nothing that looks for one, and the values of its start.

use crate::volumetric_clouds::{debug_type, debug_view, CloudAids, CloudType, CloudView, MAX_DEBUG_DENSITY, MAX_STEPS, MIN_STEPS};
use crate::volumetric_clouds_light::{try_moon_cap, try_night_pull, try_share, try_weight, LightWeight, LIGHT_WEIGHTS, MOON_BACK_VAR, MOON_CAP_VAR, MOON_FORWARD_VAR, NIGHT_PULL_VAR, SUN_COLOR_VAR, SUN_COLOUR_VAR};
use crate::volumetric_clouds_wisp::try_wisp;
use bevy::prelude::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The variable that names the tune file.
pub const TUNE_VAR: &str = "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_TUNE";
/// The file is looked at no more often than this, in real time [design: twice a
/// second; a stat of one small file].
pub const POLL_EVERY: Duration = Duration::from_millis(500);

impl CloudAids {
    /// These values with a file's live overrides on top.
    pub fn with(self, o: &TuneOverrides) -> CloudAids {
        CloudAids {
            density: o.density.unwrap_or(self.density),
            steps: o.steps.unwrap_or(self.steps),
            view: o.view.unwrap_or(self.view),
            forced: o.forced.unwrap_or(self.forced),
            shape: o.shape.unwrap_or(self.shape),
            erosion: o.erosion.unwrap_or(self.erosion),
            light: {
                let mut light = self.light;
                light.sun_colour = o.sun_colour.unwrap_or(light.sun_colour);
                light.night_pull = o.night_pull.unwrap_or(light.night_pull);
                light.moon_forward = o.moon_forward.or(light.moon_forward);
                light.moon_back = o.moon_back.or(light.moon_back);
                light.moon_cap = o.moon_cap.or(light.moon_cap);
                for (w, v) in LightWeight::ALL.into_iter().zip(o.weights) {
                    if let Some(v) = v {
                        light.weights[w.index()] = v;
                    }
                }
                light
            },
            wisp: o.wisp.unwrap_or(self.wisp),
            ..self
        }
    }
}

/// A name a tune file may set. Each key's scale and range are as designed (1 the designed strength
/// or weight); a run starts from its start values (volumetric_clouds.rs `START_VALUES`,
/// volumetric_clouds_light.rs `START_LIGHT`, volumetric_clouds_wisp.rs `START_WISP`: the look judged
/// by eye on 2026-11-23), unless a start variable sets the key; a tune line overrides, and a deleted line
/// returns to the start value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuneKey {
    /// `DENSITY`: the density's scale, 0 to 4 (1 as designed); starts at 0.76.
    Density,
    Steps,
    View,
    Type,
    /// `SHAPE`: the base relief's strength, 0 to 4 (1 as designed; 0 and every value under 0.01 draw
    /// as 0.01); starts at 1.
    Shape,
    /// `EROSION`: the erosion's strength, 0 to 4 (1 as designed); starts at 1.26.
    Erosion,
    /// A light weight (`OCTAVE`, `BACKSCATTER`, `UPPER`, `LOWER`, `GROUND`, `FORWARD`, `SUN`, `LEVEL`,
    /// `OCCLUDE`, `POWDER`), 0 to 4 (1 as designed); they start at BACKSCATTER 1.65, SUN 1.3, LEVEL 1.12 and 1
    /// for the rest (volumetric_clouds_light.rs `START_LIGHT`).
    Weight(LightWeight),
    /// `SUNCOLOUR` (alias `SUNCOLOR`): the sunlight's share in the direct light's colour, 0 to 1
    /// (0.5 as designed; volumetric_clouds_light.rs `direct_colour`); starts at 0.
    SunColour,
    /// `NIGHTPULL`: how far the clouds' light is drawn toward the cloud layers' luminance by night, 0 to 2
    /// (0.5 as designed; volumetric_clouds_light.rs `night_gain`); starts at 0.5.
    NightPull,
    /// `MOONFORWARD`, `MOONBACK`: the FORWARD and BACKSCATTER weights by moonlight, 0 to 4 (the day's unless
    /// set; volumetric_clouds_light.rs `LightAids::phase_weight`); start unset.
    MoonForward,
    MoonBack,
    /// `MOONCAP`: the phase's cap by moonlight, 0 to 16 (the day's unless set); starts unset.
    MoonCap,
    /// `WISP`: the wisps' strength, 0 to 4 (#3032 stage 2h, W2); starts at 0.52.
    Wisp,
    Height,
    Horizon,
}

impl TuneKey {
    pub const ALL: [TuneKey; 24] = [
        TuneKey::Shape,
        TuneKey::Erosion,
        TuneKey::Density,
        TuneKey::Steps,
        TuneKey::Type,
        TuneKey::View,
        TuneKey::Weight(LightWeight::Octave),
        TuneKey::Weight(LightWeight::Backscatter),
        TuneKey::Weight(LightWeight::Upper),
        TuneKey::Weight(LightWeight::Lower),
        TuneKey::Weight(LightWeight::Ground),
        TuneKey::Weight(LightWeight::Forward),
        TuneKey::Weight(LightWeight::Sun),
        TuneKey::Weight(LightWeight::Level),
        TuneKey::Weight(LightWeight::Occlude),
        TuneKey::Weight(LightWeight::Powder),
        TuneKey::SunColour,
        TuneKey::NightPull,
        TuneKey::MoonForward,
        TuneKey::MoonBack,
        TuneKey::MoonCap,
        TuneKey::Wisp,
        TuneKey::Height,
        TuneKey::Horizon,
    ];

    /// Its short name in a tune file.
    pub fn short(self) -> &'static str {
        match self {
            TuneKey::Density => "DENSITY",
            TuneKey::Steps => "STEPS",
            TuneKey::View => "VIEW",
            TuneKey::Type => "TYPE",
            TuneKey::Shape => "SHAPE",
            TuneKey::Erosion => "EROSION",
            TuneKey::Weight(w) => w.short(),
            TuneKey::SunColour => "SUNCOLOUR",
            TuneKey::NightPull => "NIGHTPULL",
            TuneKey::MoonForward => "MOONFORWARD",
            TuneKey::MoonBack => "MOONBACK",
            TuneKey::MoonCap => "MOONCAP",
            TuneKey::Wisp => "WISP",
            TuneKey::Height => "HEIGHT",
            TuneKey::Horizon => "HORIZON",
        }
    }

    /// The start variable of the same aid.
    pub fn var(self) -> &'static str {
        match self {
            TuneKey::Density => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS",
            TuneKey::Steps => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_STEPS",
            TuneKey::View => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW",
            TuneKey::Type => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_TYPE",
            TuneKey::Shape => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SHAPE",
            TuneKey::Erosion => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_EROSION",
            TuneKey::Weight(w) => w.var(),
            TuneKey::SunColour => SUN_COLOUR_VAR,
            TuneKey::NightPull => NIGHT_PULL_VAR,
            TuneKey::MoonForward => MOON_FORWARD_VAR,
            TuneKey::MoonBack => MOON_BACK_VAR,
            TuneKey::MoonCap => MOON_CAP_VAR,
            TuneKey::Wisp => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_WISP",
            TuneKey::Height => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HEIGHT",
            TuneKey::Horizon => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HORIZON",
        }
    }

    /// Whether a tune file may change it while the game runs (the module's documentation).
    pub fn live(self) -> bool {
        !matches!(self, TuneKey::Height | TuneKey::Horizon)
    }

    /// The key of a short or full name, in any case (`SUNCOLOR` and its variable's name too).
    pub fn named(name: &str) -> Option<TuneKey> {
        let n = name.trim().to_ascii_uppercase();
        if n == "SUNCOLOR" || n == SUN_COLOR_VAR {
            return Some(TuneKey::SunColour);
        }
        TuneKey::ALL.into_iter().find(|k| n == k.short() || n == k.var())
    }
}

/// `DENSITY`: a finite number from 0, held to [`MAX_DEBUG_DENSITY`] (volumetric_clouds.rs
/// `debug_density`'s form, but `None` for anything else instead of the default).
pub fn try_density(v: &str) -> Option<f32> {
    v.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0).map(|v| v.min(MAX_DEBUG_DENSITY))
}

/// `STEPS`: a whole number held to [`MIN_STEPS`]..[`MAX_STEPS`] (`debug_steps`'s form).
pub fn try_steps(v: &str) -> Option<u32> {
    v.trim().parse::<u32>().ok().map(|n| n.clamp(MIN_STEPS, MAX_STEPS))
}

/// `SHAPE`, `EROSION`: a finite number held to 0..4 (`debug_strength`'s form, `MAX_STRENGTH`).
pub fn try_strength(v: &str) -> Option<f32> {
    v.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(0.0, crate::volumetric_clouds::MAX_STRENGTH))
}

/// `VIEW`: one of `debug_view`'s names, or `none` (the clouds).
pub fn try_view(v: &str) -> Option<CloudView> {
    let n = v.trim().to_ascii_lowercase();
    if n == "none" {
        return Some(CloudView::None);
    }
    Some(debug_view(Some(&n))).filter(|view| *view != CloudView::None)
}

/// `TYPE`: one of `debug_type`'s names, or `none` (the weather's mix, no type forced).
pub fn try_type(v: &str) -> Option<Option<CloudType>> {
    let n = v.trim().to_ascii_lowercase();
    if n == "none" {
        return Some(None);
    }
    debug_type(Some(&n)).map(Some)
}

/// A file's live overrides (`None`: not set by the file).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TuneOverrides {
    pub density: Option<f32>,
    pub steps: Option<u32>,
    pub view: Option<CloudView>,
    pub forced: Option<Option<CloudType>>,
    pub shape: Option<f32>,
    pub erosion: Option<f32>,
    /// By [`LightWeight::ALL`]'s order.
    pub weights: [Option<f32>; LIGHT_WEIGHTS],
    pub sun_colour: Option<f32>,
    pub night_pull: Option<f32>,
    pub moon_forward: Option<f32>,
    pub moon_back: Option<f32>,
    pub moon_cap: Option<f32>,
    pub wisp: Option<f32>,
}

/// A file read: its overrides and the lines it ignored, each with why.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedTune {
    pub overrides: TuneOverrides,
    pub bad: Vec<(String, String)>,
}

/// The overrides of a tune file's text (the module's documentation).
pub fn parse_tune(text: &str) -> ParsedTune {
    let mut p = ParsedTune::default();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut bad = |why: String| p.bad.push((line.to_string(), why));
        let Some((name, value)) = line.split_once('=') else {
            bad("not NAME=value".into());
            continue;
        };
        let Some(key) = TuneKey::named(name) else {
            bad(format!("an unknown name; live: {}", live_names()));
            continue;
        };
        if !key.live() {
            bad(format!("{} is start-only: set {} before the run", key.short(), key.var()));
            continue;
        }
        let o = &mut p.overrides;
        let ok = match key {
            TuneKey::Density => set(&mut o.density, try_density(value)),
            TuneKey::Steps => set(&mut o.steps, try_steps(value)),
            TuneKey::View => set(&mut o.view, try_view(value)),
            TuneKey::Type => set(&mut o.forced, try_type(value)),
            TuneKey::Shape => set(&mut o.shape, try_strength(value)),
            TuneKey::Erosion => set(&mut o.erosion, try_strength(value)),
            TuneKey::Weight(w) => set(&mut o.weights[w.index()], try_weight(value)),
            TuneKey::SunColour => set(&mut o.sun_colour, try_share(value)),
            TuneKey::NightPull => set(&mut o.night_pull, try_night_pull(value)),
            TuneKey::MoonForward => set(&mut o.moon_forward, try_weight(value)),
            TuneKey::MoonBack => set(&mut o.moon_back, try_weight(value)),
            TuneKey::MoonCap => set(&mut o.moon_cap, try_moon_cap(value)),
            TuneKey::Wisp => set(&mut o.wisp, try_wisp(value)),
            TuneKey::Height | TuneKey::Horizon => false,
        };
        if !ok {
            let form = match key {
                TuneKey::Density => "a number from 0 (held to 4)",
                TuneKey::Steps => "a whole number (held to 4..128)",
                TuneKey::Shape | TuneKey::Erosion => "a number (held to 0..4)",
                TuneKey::View => "none, cover, vigour, count, noise, sun, ambient, depth, opacity or body-height",
                TuneKey::Type => "none, sheet, cumulus or tower",
                TuneKey::Weight(_) | TuneKey::MoonForward | TuneKey::MoonBack | TuneKey::Wisp => "a number (held to 0..4)",
                TuneKey::SunColour => "a number (held to 0..1)",
                TuneKey::NightPull => "a number (held to 0..2)",
                TuneKey::MoonCap => "a number (held to 0..16)",
                TuneKey::Height | TuneKey::Horizon => "start-only",
            };
            p.bad.push((line.to_string(), format!("{} wants {form}", key.short())));
        }
    }
    p
}

/// `slot` set to `v` when it is a value; whether it was.
fn set<T>(slot: &mut Option<T>, v: Option<T>) -> bool {
    let ok = v.is_some();
    if ok {
        *slot = v;
    }
    ok
}

/// The live keys' short names, for the log.
fn live_names() -> String {
    TuneKey::ALL.into_iter().filter(|k| k.live()).map(TuneKey::short).collect::<Vec<_>>().join(", ")
}

/// The effective live values in words, the changed ones named.
fn describe(start: &CloudAids, now: &CloudAids) -> String {
    let changed: Vec<&str> = [
        (TuneKey::Shape, start.shape != now.shape),
        (TuneKey::Erosion, start.erosion != now.erosion),
        (TuneKey::Density, start.density != now.density),
        (TuneKey::Steps, start.steps != now.steps),
        (TuneKey::Type, start.forced != now.forced),
        (TuneKey::View, start.view != now.view),
        (TuneKey::SunColour, start.light.sun_colour_share() != now.light.sun_colour_share()),
        (TuneKey::NightPull, start.light.night_pull() != now.light.night_pull()),
        (TuneKey::MoonForward, start.light.moon_forward != now.light.moon_forward),
        (TuneKey::MoonBack, start.light.moon_back != now.light.moon_back),
        (TuneKey::MoonCap, start.light.moon_cap != now.light.moon_cap),
    ]
    .into_iter()
    .chain(LightWeight::ALL.into_iter().map(|w| (TuneKey::Weight(w), start.light.weight(w) != now.light.weight(w))))
    .chain([
        (TuneKey::Wisp, start.wisp != now.wisp),
    ])
    .filter(|c| c.1)
    .map(|c| c.0.short())
    .collect();
    format!(
        "shape {:.2}, erosion {:.2}, density x{:.2}, quality {}, type {}, view {:?}, light{} ({}); the medium's history dropped once; {}",
        now.shape,
        now.erosion,
        now.density,
        now.steps,
        now.forced.map_or_else(|| "the weather's mix".to_string(), |t| format!("{t:?} everywhere")),
        now.view,
        if now.light.text().is_empty() { " as designed".to_string() } else { format!(" with {}", now.light.text()) },
        if changed.is_empty() { "all at their start values".to_string() } else { format!("changed from the start: {}", changed.join(", ")) },
        crate::volumetric_clouds_wisp::describe(now.wisp)
    )
}

/// Where the tune file is read from (the poll's I/O, so the tests can stand in for the disk).
pub trait TuneSource {
    /// The file's modification time (if the file system keeps one) and length.
    fn stamp(&self, path: &Path) -> std::io::Result<(Option<SystemTime>, u64)>;
    fn read(&self, path: &Path) -> std::io::Result<String>;
}

/// The disk.
pub struct DiskFiles;

impl TuneSource for DiskFiles {
    fn stamp(&self, path: &Path) -> std::io::Result<(Option<SystemTime>, u64)> {
        let m = std::fs::metadata(path)?;
        Ok((m.modified().ok(), m.len()))
    }

    fn read(&self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
}

/// The tune file's path from the variable's value (unset or empty: none).
pub fn tune_path(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The live tuning's state: the start configuration, the file, the last good overrides, the
/// effective values and how many times they changed.
#[derive(Resource, Debug)]
pub struct CloudTune {
    start: CloudAids,
    path: Option<PathBuf>,
    next_poll: Option<Duration>,
    stamp: Option<(Option<SystemTime>, u64)>,
    good: TuneOverrides,
    effective: CloudAids,
    told_bad: Vec<String>,
    told_unreadable: bool,
    told_on: bool,
    edits: u32,
}

impl Default for CloudTune {
    /// The statics as read at the start and the variable's file.
    fn default() -> Self {
        CloudTune::new(crate::volumetric_clouds::start_aids(), tune_path(crate::dev_aids::var_os(TUNE_VAR)))
    }
}

impl CloudTune {
    pub fn new(start: CloudAids, path: Option<PathBuf>) -> Self {
        CloudTune { start, path, next_poll: None, stamp: None, good: TuneOverrides::default(), effective: start, told_bad: Vec::new(), told_unreadable: false, told_on: false, edits: 0 }
    }

    /// The values the volume follows now.
    pub fn effective(&self) -> CloudAids {
        self.effective
    }

    /// How many times the effective values changed (each drops the medium's history once).
    pub fn edits(&self) -> u32 {
        self.edits
    }

    /// One look at the file at real time `now` (at most every [`POLL_EVERY`]); returns the log
    /// lines to write.
    pub fn poll(&mut self, now: Duration, files: &dyn TuneSource) -> Vec<String> {
        let mut out = Vec::new();
        let Some(path) = self.path.clone() else { return out };
        if !self.told_on {
            self.told_on = true;
            out.push(format!(
                "debug switch {TUNE_VAR}: {} looked at every {:.1} s of real time while the row is on; live: {}; start-only (read once at the start): HEIGHT, HORIZON",
                path.display(),
                POLL_EVERY.as_secs_f32(),
                live_names()
            ));
        }
        if self.next_poll.is_some_and(|t| now < t) {
            return out;
        }
        self.next_poll = Some(now + POLL_EVERY);
        let read = files.stamp(&path).and_then(|st| if self.stamp == Some(st) { Ok(None) } else { files.read(&path).map(|text| Some((st, text))) });
        let (stamp, text) = match read {
            Ok(None) => return out,
            Ok(Some(x)) => x,
            Err(e) => {
                if !self.told_unreadable {
                    self.told_unreadable = true;
                    out.push(format!("volumetric clouds: tune file {} cannot be read ({e}): the last good values kept", path.display()));
                }
                return out;
            }
        };
        self.told_unreadable = false;
        self.stamp = Some(stamp);
        let parsed = parse_tune(&text);
        for (line, why) in &parsed.bad {
            if !self.told_bad.contains(line) {
                out.push(format!("volumetric clouds: tune file {}: `{line}` ignored ({why})", path.display()));
            }
        }
        self.told_bad = parsed.bad.into_iter().map(|(line, _)| line).collect();
        self.good = parsed.overrides;
        let next = self.start.with(&self.good);
        if next != self.effective {
            out.push(format!("volumetric clouds: tune file {}: {}", path.display(), describe(&self.start, &next)));
            self.effective = next;
            self.edits = self.edits.wrapping_add(1);
        }
        out
    }
}

/// Each frame the row is on, before the volume's own systems (volumetric_clouds.rs
/// `count_sky_jumps`, `follow_volume`): one look at the tune file when one is named and its time
/// has come; nothing at all without the variable.
pub fn poll_tune(graphics: Option<Res<crate::graphics::Graphics>>, real: Option<Res<Time<Real>>>, mut tune: ResMut<CloudTune>) {
    if tune.path.is_none() || !graphics.is_some_and(|g| g.volumetric_clouds) {
        return;
    }
    let Some(real) = real else { return };
    for line in tune.poll(real.elapsed(), &DiskFiles) {
        info!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volumetric_clouds::{debug_density, debug_steps, debug_strength, VolumeInputs};
    use crate::volumetric_clouds_light::LightAids;
    use std::cell::{Cell, RefCell};

    /// A start at the designed values.
    fn start() -> CloudAids {
        CloudAids { density: 1.0, steps: 28, view: CloudView::None, forced: None, shape: 1.0, erosion: 1.0, light: LightAids::default(), wisp: 1.0, height: 40_000.0, horizon: 400_000.0 }
    }

    /// A file in memory: its text (`None`: missing), its stamp, and how often it was looked at.
    #[derive(Default)]
    struct Fake {
        text: RefCell<Option<String>>,
        stamp: Cell<u64>,
        stats: Cell<u32>,
        reads: Cell<u32>,
    }

    impl Fake {
        fn write(&self, text: &str) {
            *self.text.borrow_mut() = Some(text.to_string());
            self.stamp.set(self.stamp.get() + 1);
        }
    }

    impl TuneSource for Fake {
        fn stamp(&self, _: &Path) -> std::io::Result<(Option<SystemTime>, u64)> {
            self.stats.set(self.stats.get() + 1);
            match self.text.borrow().as_ref() {
                Some(t) => Ok((Some(SystemTime::UNIX_EPOCH + Duration::from_secs(self.stamp.get())), t.len() as u64)),
                None => Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing")),
            }
        }

        fn read(&self, _: &Path) -> std::io::Result<String> {
            self.reads.set(self.reads.get() + 1);
            self.text.borrow().clone().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// The fallible parsers agree with the start parsers on every valid value and refuse the rest
    /// (the start parsers give their defaults there).
    #[test]
    fn the_file_parsers_are_the_start_parsers_without_defaults() {
        for v in ["0", " 2.5 ", "9", "1"] {
            assert_eq!(try_density(v), Some(debug_density(Some(v))), "{v}");
        }
        for v in ["-1", "NaN", "inf", "lots", ""] {
            assert_eq!(try_density(v), None, "{v}");
        }
        for v in ["1", "16", "1000", "28"] {
            assert_eq!(try_steps(v), Some(debug_steps(Some(v))), "{v}");
        }
        assert_eq!((try_steps("-3"), try_steps("many")), (None, None));
        for v in ["0", "0.5", "3", "-1", "4", "9"] {
            assert_eq!(try_strength(v), Some(debug_strength(Some(v))), "{v}");
        }
        assert_eq!((try_strength("3"), try_strength("9"), debug_strength(Some("2.5")), debug_strength(Some("7"))), (Some(3.0), Some(4.0), 2.5, 4.0), "0..4 since L2's fix round");
        assert_eq!((try_strength("NaN"), try_strength("x")), (None, None));
        for (v, view) in [("none", CloudView::None), ("Depth", CloudView::Depth), ("sun", CloudView::Sun), ("ambient", CloudView::Ambient), ("cover", CloudView::Cover), ("vigor", CloudView::Vigour), ("count", CloudView::Count), ("noise", CloudView::Noise)] {
            assert_eq!(try_view(v), Some(view), "{v}");
        }
        assert_eq!(try_view("map"), None, "code 5 is not built on this branch");
        assert_eq!([try_view("opacity"), try_view("body-height"), try_view("detail"), try_view("overhead"), try_view("face")], [Some(CloudView::Opacity), Some(CloudView::BodyHeight), None, None, None]);
        assert_eq!((try_type("none"), try_type(" Tower "), try_type("anvil")), (Some(None), Some(Some(CloudType::Tower)), None));
        assert_eq!(TuneKey::named("shape"), Some(TuneKey::Shape));
        assert_eq!(TuneKey::named("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS"), Some(TuneKey::Density));
        assert_eq!(TuneKey::named("nvf_play_debug_volumetric_clouds_view"), Some(TuneKey::View));
        for gone in ["light", "model", "detail", "crisp", "rough", "tower", "sky", "shade", "silver", "face", "overhead"] {
            assert_eq!(TuneKey::named(gone), None, "{gone}");
        }
        assert_eq!(TuneKey::named("backscatter"), Some(TuneKey::Weight(LightWeight::Backscatter)));
        assert_eq!(TuneKey::named("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_GROUND"), Some(TuneKey::Weight(LightWeight::Ground)));
        assert_eq!(TuneKey::named("anvil"), None, "an unknown name");
        assert_eq!(TuneKey::named("powder"), Some(TuneKey::Weight(LightWeight::Powder)), "a light weight since the form and contrast step");
        assert_eq!(TuneKey::ALL.into_iter().filter(|k| !k.live()).collect::<Vec<_>>(), [TuneKey::Height, TuneKey::Horizon]);
    }

    /// The file's lines: comments, blanks, a later line winning, every kind of bad line with why;
    /// start-only keys refused.
    #[test]
    fn a_tune_file_parses_and_names_its_bad_lines() {
        let p = parse_tune("# a tune file\nSHAPE=0.5\n\n erosion = 0.25 # comment\nVIEW=depth\nshape=0.75\nTYPE=none\nNVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS=2\nSTEPS=56\nHEIGHT=24000\nMARCH=refined\nnonsense\nMODEL=1\nDENSITY=-1\n");
        let o = p.overrides;
        assert_eq!((o.shape, o.erosion, o.view, o.forced, o.density, o.steps), (Some(0.75), Some(0.25), Some(CloudView::Depth), Some(None), Some(2.0), Some(56)));
        let bad: Vec<&str> = p.bad.iter().map(|b| b.0.as_str()).collect();
        assert_eq!(bad, ["HEIGHT=24000", "MARCH=refined", "nonsense", "MODEL=1", "DENSITY=-1"]);
        assert!(p.bad[0].1.contains("start-only") && p.bad[1].1.contains("an unknown name") && p.bad[2].1.contains("NAME=value") && p.bad[3].1.contains("an unknown name") && p.bad[4].1.contains("DENSITY wants"));
        assert_eq!(parse_tune(""), ParsedTune::default());
    }

    /// Precedence (the file over the start values), removal (a key gone goes back to its start
    /// value), the history dropped once per effective change and never for an unchanged reread.
    #[test]
    fn the_file_overrides_the_start_and_a_removed_key_reverts() {
        let mut start = start();
        start.density = 1.5;
        start.view = CloudView::Count;
        let files = Fake::default();
        files.write("SHAPE=0.5\nVIEW=sun\n");
        let mut tune = CloudTune::new(start, Some(PathBuf::from("tune.txt")));
        let lines = tune.poll(ms(0), &files);
        assert!(lines[0].starts_with("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_TUNE"), "{lines:?}");
        let e = tune.effective();
        assert_eq!((e.shape, e.view, e.density, e.erosion), (0.5, CloudView::Sun, 1.5, 1.0), "the file wins where it speaks, the start elsewhere");
        assert_eq!(tune.edits(), 1);
        let applied = e.apply(VolumeInputs { domain: 1.0, ..Default::default() });
        assert_eq!((applied.shape_aid, applied.view, applied.density, applied.steps, applied.domain), (Some(0.5), CloudView::Sun, 1.5, 28, 1.0), "the start-only inputs untouched");
        // Removed: back to the start value.
        files.write("VIEW=sun\n");
        tune.poll(ms(600), &files);
        assert_eq!((tune.effective().shape, tune.effective().view), (1.0, CloudView::Sun));
        assert_eq!(tune.edits(), 2);
        // Rewritten with the same values: read, nothing changed, the history kept.
        files.write("VIEW = sun # again\n");
        assert!(tune.poll(ms(1_200), &files).is_empty());
        assert_eq!(tune.edits(), 2);
        // Emptied: all the start values.
        files.write("");
        tune.poll(ms(1_800), &files);
        assert_eq!(tune.effective(), start);
        assert_eq!(tune.edits(), 3);
    }

    /// A file missing at the start, then made: the start values, one line saying it cannot be read,
    /// then its values. A read that fails later keeps the last good values. No variable: nothing.
    #[test]
    fn a_missing_file_then_created_and_a_failed_read_keeps_the_last_good() {
        let files = Fake::default();
        let mut tune = CloudTune::new(start(), Some(PathBuf::from("later.txt")));
        let lines = tune.poll(ms(0), &files);
        assert!(lines.iter().any(|l| l.contains("cannot be read")) && tune.effective() == start() && tune.edits() == 0, "{lines:?}");
        assert!(tune.poll(ms(500), &files).is_empty(), "said once");
        files.write("EROSION=0\n");
        tune.poll(ms(1_000), &files);
        assert_eq!(tune.effective().erosion, 0.0);
        // Gone again (deleted while being saved): the last good values kept, said once.
        *files.text.borrow_mut() = None;
        let lines = tune.poll(ms(1_500), &files);
        assert!(lines.iter().any(|l| l.contains("the last good values kept")) && tune.effective().erosion == 0.0);
        let mut none = CloudTune::new(start(), tune_path(Some("".into())));
        assert!(none.poll(ms(0), &files).is_empty() && none.effective() == start());
        assert_eq!(tune_path(None), None);
    }

    /// At most one look every POLL_EVERY of real time; an unchanged stamp is not read again.
    #[test]
    fn the_file_is_looked_at_twice_a_second_at_most() {
        let files = Fake::default();
        files.write("STEPS=56\n");
        let mut tune = CloudTune::new(start(), Some(PathBuf::from("t.txt")));
        for t in [0u64, 1, 100, 250, 499] {
            tune.poll(ms(t), &files);
        }
        assert_eq!((files.stats.get(), files.reads.get()), (1, 1));
        tune.poll(ms(500), &files);
        assert_eq!((files.stats.get(), files.reads.get()), (2, 1), "looked at, not read: the stamp is the same");
        files.write("STEPS=16\n");
        tune.poll(ms(700), &files);
        assert_eq!(tune.effective().steps, 56, "not yet: 200 ms after the last look");
        tune.poll(ms(1_000), &files);
        assert_eq!((files.stats.get(), files.reads.get(), tune.effective().steps), (3, 2, 16));
    }

    /// The light's keys (#3032 stage 2e, L2): the weights, live, held to 0..4; they reach the uniform's
    /// inputs, a removed one goes back to its start value, and each change drops the history once.
    #[test]
    fn the_light_keys_are_live() {
        let p = parse_tune("OCTAVE=0\nbackscatter=2.5\nUPPER=9\nLOWER=-1\nGROUND=0.5\nNVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_FORWARD=3\nSUN=0.25\nLOWER=lots\n");
        assert_eq!(p.overrides.weights, [Some(0.0), Some(2.5), Some(4.0), Some(0.0), Some(0.5), Some(3.0), Some(0.25), None, None, None]);
        let bad: Vec<&str> = p.bad.iter().map(|b| b.0.as_str()).collect();
        assert_eq!(bad, ["LOWER=lots"]);
        assert!(p.bad[0].1.contains("LOWER wants a number"));
        assert!(TuneKey::ALL.into_iter().filter(|k| matches!(k, TuneKey::Weight(_))).all(TuneKey::live));
        for w in LightWeight::ALL {
            assert_eq!(TuneKey::named(w.short()), Some(TuneKey::Weight(w)));
            assert_eq!(TuneKey::Weight(w).var(), w.var());
        }
        let files = Fake::default();
        files.write("SUN=3\nUPPER=2\n");
        let mut tune = CloudTune::new(start(), Some(PathBuf::from("t.txt")));
        let lines = tune.poll(ms(0), &files);
        let e = tune.effective();
        assert!(e.light.weight(LightWeight::Sun) == 3.0 && e.light.weight(LightWeight::Upper) == 2.0 && e.light.weight(LightWeight::Lower) == 1.0);
        assert!(lines.iter().any(|l| l.contains("light with UPPER x2.00, SUN x3.00") && l.contains("changed from the start: UPPER, SUN")), "{lines:?}");
        assert_eq!(e.apply(VolumeInputs::default()).light_aids, e.light, "the light's aids reach the uniform's inputs");
        assert_eq!(tune.edits(), 1);
        files.write("UPPER=2\nFORWARD=0\n");
        tune.poll(ms(500), &files);
        let e = tune.effective();
        assert!(e.light.weight(LightWeight::Sun) == 1.0 && e.light.weight(LightWeight::Forward) == 0.0 && e.light.weight(LightWeight::Upper) == 2.0, "SUN removed: back to the start's");
        assert_eq!(tune.edits(), 2);
        // SUNCOLOUR (alias SUNCOLOR): the sunlight's share in the direct light's colour, 0..1, live.
        let p = parse_tune("SUNCOLOUR=0.2\nsuncolor=1.5\nSUNCOLOUR=warm\n");
        assert_eq!((p.overrides.sun_colour, p.bad.len()), (Some(1.0), 1), "the alias, held to 0..1; the later line wins");
        assert!(p.bad[0].1.contains("SUNCOLOUR wants a number (held to 0..1)"));
        for name in ["SUNCOLOUR", "suncolor", "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SUNCOLOUR", "nvf_play_debug_volumetric_clouds_suncolor"] {
            assert_eq!(TuneKey::named(name), Some(TuneKey::SunColour), "{name}");
        }
        assert!(TuneKey::SunColour.live() && TuneKey::SunColour.var() == SUN_COLOUR_VAR);
        files.write("SUNCOLOR=0\n");
        let lines = tune.poll(ms(1_000), &files);
        let e = tune.effective();
        assert!(e.light.sun_colour == 0.0 && tune.edits() == 3, "{e:?}");
        assert!(lines.iter().any(|l| l.contains("SUNCOLOUR 0.00") && l.contains("changed from the start: SUNCOLOUR")), "{lines:?}");
        assert_eq!(e.apply(VolumeInputs::default()).light_aids.sun_colour, 0.0, "it reaches the uniform's inputs");
        files.write("");
        tune.poll(ms(1_500), &files);
        assert_eq!(tune.effective().light.sun_colour, crate::volumetric_clouds_light::SUN_COLOUR_SHARE, "removed: back to the start's");
    }

    /// NIGHTPULL (0..2), MOONFORWARD and MOONBACK (0..4): live, their start variables' names, a change named and
    /// dropped once, the uniform's inputs, a removed key back at its start value.
    #[test]
    fn the_night_keys_are_live() {
        use crate::volumetric_clouds_light::{MAX_NIGHT_PULL, MOON_BACK, MOON_FORWARD, NIGHT_PULL};
        let p = parse_tune("NIGHTPULL=9\nnightpull=0.8\nNVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_NIGHTPULL=dark\nMOONFORWARD=9\nMoonBack=0.5\nMOONBACK=none\n");
        assert_eq!((p.overrides.night_pull, p.overrides.moon_forward, p.overrides.moon_back, p.bad.len()), (Some(0.8), Some(4.0), Some(0.5), 2));
        assert!(p.bad[0].1.contains("NIGHTPULL wants a number (held to 0..2)") && p.bad[1].1.contains("MOONBACK wants a number (held to 0..4)"), "{:?}", p.bad);
        assert_eq!(parse_tune("NIGHTPULL=9").overrides.night_pull, Some(MAX_NIGHT_PULL));
        for (key, name, var) in [(TuneKey::NightPull, "nightpull", NIGHT_PULL_VAR), (TuneKey::MoonForward, "moonforward", MOON_FORWARD_VAR), (TuneKey::MoonBack, "moonback", MOON_BACK_VAR)] {
            assert_eq!((TuneKey::named(name), TuneKey::named(var), key.var(), key.live()), (Some(key), Some(key), var, true));
            assert_eq!(var, format!("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_{}", key.short()));
        }
        let files = Fake::default();
        files.write("NIGHTPULL=0.8\nMOONFORWARD=2\nMOONBACK=0.5\n");
        let mut tune = CloudTune::new(start(), Some(PathBuf::from("t.txt")));
        let lines = tune.poll(ms(0), &files);
        let e = tune.effective();
        assert_eq!((e.light.night_pull, e.light.moon_forward, e.light.moon_back, tune.edits()), (0.8, Some(2.0), Some(0.5), 1));
        assert!(lines.iter().any(|l| l.contains("NIGHTPULL 0.80, MOONFORWARD x2.00, MOONBACK x0.50") && l.contains("changed from the start: NIGHTPULL, MOONFORWARD, MOONBACK")), "{lines:?}");
        assert_eq!(e.apply(VolumeInputs::default()).light_aids, e.light);
        files.write("");
        tune.poll(ms(600), &files);
        let e = tune.effective();
        assert_eq!((e, e.light.night_pull, e.light.moon_forward, e.light.moon_back), (start(), NIGHT_PULL, MOON_FORWARD, MOON_BACK));
    }

    /// WISP (W2): live, 0..4, its start variable's name, a change named and dropped once, the uniform's inputs.
    #[test]
    fn the_wisp_key_is_live() {
        let p = parse_tune("WISP=9\nwisp=2.5\nNVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_WISP=lots\n");
        assert_eq!((p.overrides.wisp, p.bad.len()), (Some(2.5), 1));
        assert!(p.bad[0].1.contains("WISP wants a number (held to 0..4)"), "{:?}", p.bad);
        assert_eq!((TuneKey::named("wisp"), TuneKey::Wisp.var(), TuneKey::Wisp.live()), (Some(TuneKey::Wisp), "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_WISP", true));
        let files = Fake::default();
        files.write("WISP=0\n");
        let mut tune = CloudTune::new(start(), Some(PathBuf::from("t.txt")));
        let lines = tune.poll(ms(0), &files);
        assert_eq!((tune.effective().wisp, tune.edits()), (0.0, 1));
        assert!(lines.iter().any(|l| l.contains("WISP 0.00 (no wisps)") && l.contains("changed from the start: WISP")), "{lines:?}");
        assert_eq!(tune.effective().apply(VolumeInputs::default()).wisp_aid, Some(0.0));
        files.write("");
        tune.poll(ms(600), &files);
        assert_eq!(tune.effective(), start(), "an empty file: the start values again");
    }

    /// One log line per changed bad line: a bad line that stays is not logged again; changed, it is.
    #[test]
    fn a_bad_line_is_logged_once_per_change() {
        let files = Fake::default();
        files.write("SHAPE=lots\n");
        let mut tune = CloudTune::new(start(), Some(PathBuf::from("t.txt")));
        let bad = |lines: &[String]| lines.iter().filter(|l| l.contains("ignored")).count();
        assert_eq!(bad(&tune.poll(ms(0), &files)), 1);
        files.write("SHAPE=lots\nVIEW=depth\n");
        assert_eq!(bad(&tune.poll(ms(500), &files)), 0, "the same bad line, not again");
        files.write("SHAPE=more\nVIEW=depth\n");
        assert_eq!(bad(&tune.poll(ms(1_000), &files)), 1, "changed: once more");
        assert_eq!((tune.effective().shape, tune.effective().view), (1.0, CloudView::Depth), "a bad line changes nothing");
    }
}
