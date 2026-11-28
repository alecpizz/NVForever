//! How the landscape is smoothed (#2149, `fnv_world::land_surface`): one choice for the
//! whole run, so the drawn ground (nvforever terrain.rs), its collision (`crate::phys`) and
//! every height query stand on the same surface.
//!
//! [design, #2149, #2676] The settings page's Smooth terrain row ([`Smoothing`]): Off, 2 × 2,
//! 4 × 4 or 4 × 4 near. 4 × 4 is the default (maintainer decision 2026-11-13: "terrain
//! smoothing should be the default have the collision follow the smoothed surface"; 4 × 4
//! the contributor's pick of 2026-11-15, for the maintainer to confirm). `--smooth-terrain <off|2x2|4x4|4x4-near>` chooses for a
//! run, `--no-smooth-terrain` is Off. A change on the page takes effect at the next start
//! (loading a save starts the game again): cells are drawn and collided as they load, so a
//! change while the game runs would leave the loaded cells' ground and collision on the old
//! surface beside new cells on the other [design]. The INI's older on/off (`1`/`0`, also
//! true/false, on/off, yes/no) reads as 4 × 4 and Off, what they meant.
//!
//! - **2 × 2 / 4 × 4**: each landscape square cut into 2 × 2 or 4 × 4 squares, drawn and
//!   collided at that level everywhere.
//! - **4 × 4 near** [design, #2676; NVForever's own rule]: the surface is the 4 × 4 one (the
//!   collision and every query use it, so actors near the player stand on what is drawn),
//!   but a loaded quadrant is drawn at 4 × 4 only near the camera and every other surface
//!   vertex (2 × 2, the same surface: `fnv_world::land_surface::QuadrantDraw`) further
//!   away ([`DistanceRule`]; nvforever terrain_lod.rs).
//!
//! How it is smoothed is fixed for the run too ([`params`]): by default detail, within 8
//! units, thresholds 2:6 (`fnv_world::land_surface::SmoothParams::DEFAULT`). [design, #2149:
//! the contributor's pick from in-game comparisons (2026-11-15), for the maintainer to confirm.] Test aids choose another recipe
//! for comparing them in the game; they are not settings rows or INI entries:
//! - `NVF_PLAY_TEST_SMOOTH_MODE=interpolate|fit|detail` (the LAND vertices kept, fitted,
//!   or fitted where they look like quantisation steps only);
//! - `NVF_PLAY_TEST_SMOOTH_DETAIL=<t0>:<t1>` (detail's thresholds, units, 0 ≤ t0 < t1 ≤ 16;
//!   2:6 by default; used with `detail` only);
//! - `NVF_PLAY_TEST_SMOOTH_SUBDIV=2|4` (new squares per grid square and side, over the
//!   row's level; 2 with 4 × 4 near draws and collides 2 × 2 everywhere);
//! - `NVF_PLAY_TEST_SMOOTH_BOUND=<units>` (0 to 16: how far from the stored triangles);
//! - `NVF_PLAY_TEST_SMOOTH_NEAR=<units>` (4 × 4 near: drawn 4 × 4 within this distance,
//!   [`FINE_NEAR`] by default; 0 to [`MAX_NEAR`]).
//!
//! A value out of range gives one warning at the start and that field's default. With
//! smoothing off they do nothing: the run draws and collides the stored triangles.

use fnv_world::land_surface::{detail_thresholds_valid, SmoothMode, SmoothParams, SurfaceSpec, DEFAULT_DETAIL, MAX_BOUND, SUBDIVS};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// The Smooth terrain row's choices [design, #2149, #2676].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Smoothing {
    /// The stored triangles.
    Off,
    /// 2 × 2 squares per landscape square, everywhere.
    Sub2,
    /// 4 × 4 everywhere (the default).
    Sub4,
    /// The 4 × 4 surface, drawn 4 × 4 near the camera and 2 × 2 further ([`DistanceRule`]).
    Near,
}

impl Smoothing {
    /// The page's order.
    pub const ALL: [Smoothing; 4] = [Smoothing::Off, Smoothing::Sub2, Smoothing::Sub4, Smoothing::Near];
    /// [design, #2149: maintainer decision 2026-11-13; 4 × 4 the contributor's pick of
    /// 2026-11-15, for the maintainer to confirm].
    /// Making 4 × 4 near the default is the maintainer's call (#2676).
    pub const DEFAULT: Smoothing = Smoothing::Sub4;

    /// As the page shows it (NVForever's own words).
    pub fn name(self) -> &'static str {
        match self {
            Smoothing::Off => "Off",
            Smoothing::Sub2 => "2 x 2",
            Smoothing::Sub4 => "4 x 4",
            Smoothing::Near => "4 x 4 near",
        }
    }

    /// As the INI keeps it and `--smooth-terrain` takes it.
    pub fn stored(self) -> &'static str {
        match self {
            Smoothing::Off => "off",
            Smoothing::Sub2 => "2x2",
            Smoothing::Sub4 => "4x4",
            Smoothing::Near => "4x4-near",
        }
    }

    /// A choice by its INI text or its name (any case, `×` for `x`), or the row's older on/off
    /// (`1`, true, on, yes: 4 × 4; `0`, false, no: Off).
    pub fn parse(text: &str) -> Option<Smoothing> {
        let t = text.trim().to_ascii_lowercase().replace('×', "x");
        Smoothing::ALL.into_iter().find(|s| s.stored() == t || s.name().to_ascii_lowercase() == t).or(match t.as_str() {
            "1" | "true" | "on" | "yes" => Some(Smoothing::Sub4),
            "0" | "false" | "no" => Some(Smoothing::Off),
            _ => None,
        })
    }

    /// Whether the landscape is smoothed at all.
    pub fn smooths(self) -> bool {
        self != Smoothing::Off
    }

    fn code(self) -> u8 {
        Smoothing::ALL.iter().position(|s| *s == self).unwrap_or(2) as u8
    }

    fn of_code(c: u8) -> Smoothing {
        Smoothing::ALL.get(c as usize).copied().unwrap_or(Smoothing::DEFAULT)
    }
}

/// What the settings page shows and the INI keeps (the next start's choice).
static SETTING: AtomicU8 = AtomicU8::new(2);
/// This run's choice, fixed once the INI is read ([`fix`]).
static RUN: AtomicU8 = AtomicU8::new(2);
/// This run's recipe from the test aids, and whether the subdivision aid was taken ([`params`]).
static PARAMS: OnceLock<(SmoothParams, bool)> = OnceLock::new();
/// The near distance from its test aid ([`distance_rule`]).
static NEAR: OnceLock<f32> = OnceLock::new();

/// The INI key and settings row.
pub const KEY: &str = "smooth-terrain";

/// The test aids (module notes).
pub const MODE_AID: &str = "NVF_PLAY_TEST_SMOOTH_MODE";
pub const SUBDIV_AID: &str = "NVF_PLAY_TEST_SMOOTH_SUBDIV";
pub const BOUND_AID: &str = "NVF_PLAY_TEST_SMOOTH_BOUND";
pub const DETAIL_AID: &str = "NVF_PLAY_TEST_SMOOTH_DETAIL";
pub const NEAR_AID: &str = "NVF_PLAY_TEST_SMOOTH_NEAR";

/// The settings row's note, NVForever's own words [design].
pub const NOTE: &str = "Smooths height steps while keeping bumps and ridges: 2 x 2 or 4 x 4 per landscape square, or 4 x 4 near and 2 x 2 further. Applies at the next start.";

/// 4 × 4 near: a quadrant is drawn 4 × 4 once the camera comes within this many units of it
/// (its square, seen from above) [design, #2676: one quadrant's side, so the quadrant the
/// camera is over and the eight around it are 4 × 4 or close to it; at that distance 4 × 4
/// and 2 × 2, about a unit apart in height, are within a pixel at 1080p (a unit spans about
/// 0.7 pixels at 1,500 units with a 75° view); to be tuned from pictures].
pub const FINE_NEAR: f32 = 2048.0;

/// How much further than its near distance a 4 × 4 quadrant must be before it is drawn 2 × 2
/// again, so one at the boundary does not switch back and forth [design, #2676].
pub const FINE_HYSTERESIS: f32 = 512.0;

/// The largest near distance the test aid takes: beyond the loaded cells of the largest
/// grid (units) [design].
pub const MAX_NEAR: f32 = 65536.0;

/// 4 × 4 near's rule (module notes): drawn 4 × 4 within `near` units, back to 2 × 2 beyond
/// `far` (`near` + [`FINE_HYSTERESIS`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceRule {
    pub near: f32,
    pub far: f32,
}

impl DistanceRule {
    pub fn with_near(near: f32) -> DistanceRule {
        DistanceRule { near, far: near + FINE_HYSTERESIS }
    }

    /// Whether a quadrant `distance` units from the camera is drawn 4 × 4, `fine` whether it
    /// is now: it becomes 4 × 4 inside `near` and 2 × 2 beyond `far`, and keeps its level in
    /// between.
    pub fn fine(&self, distance: f32, fine: bool) -> bool {
        if fine {
            distance <= self.far
        } else {
            distance < self.near
        }
    }
}

/// The choice the settings page shows (what the next start uses).
pub fn setting() -> Smoothing {
    Smoothing::of_code(SETTING.load(Ordering::Relaxed))
}

pub fn set_setting(s: Smoothing) {
    SETTING.store(s.code(), Ordering::Relaxed);
}

/// This run's choice.
pub fn run() -> Smoothing {
    Smoothing::of_code(RUN.load(Ordering::Relaxed))
}

/// Whether this run's landscape is smoothed.
pub fn on() -> bool {
    run().smooths()
}

/// Fix the run's choice from the setting: before the first cell loads (once at start from
/// the command line, again at `PostStartup` after the INI is applied).
pub fn fix() {
    RUN.store(SETTING.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// The recipe from the test aids, read once; whether the subdivision aid was taken.
fn aids() -> (SmoothParams, bool) {
    *PARAMS.get_or_init(|| {
        let var = |k: &str| std::env::var(k).ok();
        let subdiv = var(SUBDIV_AID);
        let (p, problems) = parse_params(var(MODE_AID).as_deref(), subdiv.as_deref(), var(BOUND_AID).as_deref(), var(DETAIL_AID).as_deref());
        let taken = subdiv.is_some() && !problems.iter().any(|e| e.starts_with(SUBDIV_AID));
        for e in problems {
            bevy::log::warn!("{e}");
        }
        (p, taken)
    })
}

/// This run's smoothing recipe from the test aids (module notes), read once; a bad value
/// warns and takes that field's default. Its subdivision is the aid's, else the default's
/// (the run's own: [`spec`]).
pub fn params() -> SmoothParams {
    aids().0
}

/// The surface this run draws, collides with and measures on: what every job that builds
/// one carries (it never reads the settings itself).
pub fn spec() -> SurfaceSpec {
    let (p, aid) = aids();
    spec_of(run(), p, aid)
}

/// The surface of choice `s` with the aids' recipe `p` (`aid`: its subdivision was given).
fn spec_of(s: Smoothing, p: SmoothParams, aid: bool) -> SurfaceSpec {
    let subdiv = match (s, aid) {
        (Smoothing::Off, _) => return SurfaceSpec::Stored,
        (_, true) => p.subdiv,
        (Smoothing::Sub2, false) => 2,
        (_, false) => 4,
    };
    SurfaceSpec::Smoothed(SmoothParams { subdiv, ..p })
}

/// 4 × 4 near's rule for this run: `None` unless the run's choice is 4 × 4 near on a 4 × 4
/// surface.
pub fn distance_rule() -> Option<DistanceRule> {
    rule_of(run(), spec(), near_distance())
}

fn rule_of(s: Smoothing, spec: SurfaceSpec, near: f32) -> Option<DistanceRule> {
    (s == Smoothing::Near && spec.subdiv() == 4).then(|| DistanceRule::with_near(near))
}

/// The near distance: the test aid's, else [`FINE_NEAR`].
fn near_distance() -> f32 {
    *NEAR.get_or_init(|| {
        let (near, problem) = parse_near(std::env::var(NEAR_AID).ok().as_deref());
        if let Some(e) = problem {
            bevy::log::warn!("{e}");
        }
        near
    })
}

/// The near distance the aid's value gives, and a line when it is not taken.
pub fn parse_near(v: Option<&str>) -> (f32, Option<String>) {
    let Some(v) = v.map(str::trim) else { return (FINE_NEAR, None) };
    match v.parse::<f32>() {
        Ok(n) if n.is_finite() && (0.0..=MAX_NEAR).contains(&n) => (n, None),
        _ => (FINE_NEAR, Some(format!("{NEAR_AID}={v:?}: expected units from 0 to {MAX_NEAR}; using {FINE_NEAR}"))),
    }
}

/// The recipe the test aids' values give (`None`: not set), and a line for each value
/// that was not taken (that field keeps its default).
pub fn parse_params(mode: Option<&str>, subdiv: Option<&str>, bound: Option<&str>, detail: Option<&str>) -> (SmoothParams, Vec<String>) {
    let mut p = SmoothParams::DEFAULT;
    let mut problems = Vec::new();
    if let Some(v) = mode.map(str::trim) {
        match v.to_ascii_lowercase().as_str() {
            "interpolate" => p.mode = SmoothMode::Interpolate,
            "fit" => p.mode = SmoothMode::Fit,
            "detail" => p.mode = SmoothMode::DETAIL,
            _ => problems.push(format!("{MODE_AID}={v:?}: expected interpolate, fit or detail; using {}", p.mode.name())),
        }
    }
    if let Some(v) = detail.map(str::trim) {
        let parsed = v.split_once(':').and_then(|(a, b)| Some((a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?)));
        match (parsed, p.mode) {
            (Some((t0, t1)), SmoothMode::Detail { .. }) if detail_thresholds_valid(t0, t1) => p.mode = SmoothMode::Detail { t0, t1 },
            (Some((t0, t1)), _) if detail_thresholds_valid(t0, t1) => problems.push(format!("{DETAIL_AID}={v:?} is used with {MODE_AID}=detail only; not used")),
            _ => problems.push(format!("{DETAIL_AID}={v:?}: expected <t0>:<t1>, units, 0 <= t0 < t1 <= {MAX_BOUND}; using {}:{}", DEFAULT_DETAIL.0, DEFAULT_DETAIL.1)),
        }
    }
    if let Some(v) = subdiv.map(str::trim) {
        match v.parse::<usize>() {
            Ok(n) if SUBDIVS.contains(&n) => p.subdiv = n,
            _ => problems.push(format!("{SUBDIV_AID}={v:?}: expected one of {SUBDIVS:?}; using {}", p.subdiv)),
        }
    }
    if let Some(v) = bound.map(str::trim) {
        match v.parse::<f32>() {
            Ok(b) if b.is_finite() && (0.0..=MAX_BOUND).contains(&b) => p.max_deviation = b,
            _ => problems.push(format!("{BOUND_AID}={v:?}: expected units from 0 to {MAX_BOUND}; using {}", p.max_deviation)),
        }
    }
    debug_assert!(p.is_valid());
    (p, problems)
}

/// The start's one log line for the surface (the smoke scenarios `terrain-smoothing*`
/// read it).
pub fn describe() -> String {
    describe_run(run(), spec(), params(), distance_rule())
}

/// `terrain: <spec>; drawing, collision and queries use this surface`, then what is off or
/// how 4 × 4 near draws it.
fn describe_run(s: Smoothing, spec: SurfaceSpec, params: SmoothParams, rule: Option<DistanceRule>) -> String {
    let mut line = format!("terrain: {}; drawing, collision and queries use this surface", spec.describe());
    if !spec.is_smoothed() {
        line.push_str(" (smoothing off: --no-smooth-terrain or the Smooth terrain setting");
        if params != SmoothParams::DEFAULT {
            line.push_str("; the NVF_PLAY_TEST_SMOOTH_* recipe is not used");
        }
        line.push(')');
    } else if let Some(r) = rule {
        line.push_str(&format!("; drawn 4 x 4 within {} units of the camera, 2 x 2 beyond {} (4 x 4 near)", r.near, r.far));
    } else if s == Smoothing::Near {
        line.push_str(&format!("; 4 x 4 near is not used at subdiv={} ({SUBDIV_AID})", spec.subdiv()));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn describe_spec(spec: SurfaceSpec, params: SmoothParams) -> String {
        let s = if spec.is_smoothed() { Smoothing::Sub4 } else { Smoothing::Off };
        describe_run(s, spec, params, None)
    }

    #[test]
    fn the_aids_choose_the_recipe() {
        assert_eq!(parse_params(None, None, None, None), (SmoothParams::DEFAULT, Vec::new()));
        let (p, e) = parse_params(Some("fit"), Some("4"), Some("12"), None);
        assert_eq!((p, e.len()), (SmoothParams { mode: SmoothMode::Fit, subdiv: 4, max_deviation: 12.0 }, 0));
        let (p, e) = parse_params(Some(" Interpolate "), Some("2"), Some("0"), None);
        assert_eq!((p, e.len()), (SmoothParams { mode: SmoothMode::Interpolate, subdiv: 2, max_deviation: 0.0 }, 0));
        assert_eq!(parse_params(None, None, Some("2.5"), None).0.max_deviation, 2.5);
    }

    /// Detail and its thresholds: the default 2:6, another pair, a bad pair (warned,
    /// the default), a pair without detail (warned, not used).
    #[test]
    fn detail_takes_its_thresholds() {
        let (p, e) = parse_params(Some("detail"), Some("4"), Some("8"), None);
        assert_eq!((p, e.len()), (SmoothParams { mode: SmoothMode::DETAIL, subdiv: 4, max_deviation: 8.0 }, 0));
        assert_eq!(p.mode, SmoothMode::Detail { t0: 2.0, t1: 6.0 });
        let (p, e) = parse_params(Some("detail"), None, None, Some(" 1.5 : 9 "));
        assert_eq!((p.mode, e.len()), (SmoothMode::Detail { t0: 1.5, t1: 9.0 }, 0));
        // Detail is also the default when the mode aid is absent.
        assert_eq!(parse_params(None, None, None, Some(" 1.5 : 9 ")), (p, Vec::new()));
        for bad in ["6:2", "3:3", "-1:4", "2:17", "2", "a:b", "NaN:4", "2:inf"] {
            let (p, e) = parse_params(Some("detail"), None, None, Some(bad));
            assert_eq!((p.mode, e.len()), (SmoothMode::DETAIL, 1), "{bad}");
            assert!(e[0].starts_with(DETAIL_AID) && e[0].contains("expected"), "{bad}: {e:?}");
        }
        let (p, e) = parse_params(Some("fit"), None, None, Some("1:5"));
        assert_eq!((p.mode, e.len()), (SmoothMode::Fit, 1));
        assert!(e[0].contains("detail only"));
        let detail = SmoothParams { mode: SmoothMode::Detail { t0: 1.5, t1: 9.0 }, subdiv: 4, max_deviation: 8.0 };
        assert_eq!(describe_spec(SurfaceSpec::Smoothed(detail), detail), "terrain: mode=detail(1.5:9) subdiv=4 bound=8 land-shift=4; drawing, collision and queries use this surface");
    }

    /// Each bad field warns once and keeps its default; the good ones are still taken.
    #[test]
    fn bad_values_fall_back_field_by_field() {
        for (mode, subdiv, bound) in [("bicubic", "4", "12"), ("fit", "3", "12"), ("fit", "4", "-1"), ("fit", "4", "NaN"), ("fit", "4", "inf"), ("fit", "4", "16.5"), ("fit", "x", "12")] {
            let (p, e) = parse_params(Some(mode), Some(subdiv), Some(bound), None);
            assert_eq!(e.len(), 1, "{mode} {subdiv} {bound}: {e:?}");
            assert!(p.is_valid());
            let want = SmoothParams {
                mode: if mode == "fit" { SmoothMode::Fit } else { SmoothParams::DEFAULT.mode },
                subdiv: if subdiv == "4" { 4 } else { SmoothParams::DEFAULT.subdiv },
                max_deviation: if bound == "12" { 12.0 } else { 8.0 },
            };
            assert_eq!(p, want, "{mode} {subdiv} {bound}");
        }
        let (_, e) = parse_params(Some("x"), Some("x"), Some("x"), None);
        assert_eq!(e.len(), 3);
        assert!(e[0].starts_with(MODE_AID) && e[1].starts_with(SUBDIV_AID) && e[2].starts_with(BOUND_AID));
    }

    #[test]
    fn the_start_line_names_the_surface() {
        assert_eq!(describe_spec(SurfaceSpec::DEFAULT, SmoothParams::DEFAULT), "terrain: mode=detail(2:6) subdiv=4 bound=8 land-shift=4; drawing, collision and queries use this surface");
        let fit = SmoothParams { mode: SmoothMode::Fit, subdiv: 4, max_deviation: 16.0 };
        assert_eq!(describe_spec(SurfaceSpec::Smoothed(fit), fit), "terrain: mode=fit subdiv=4 bound=16 land-shift=4; drawing, collision and queries use this surface");
        assert_eq!(
            describe_spec(SurfaceSpec::Stored, SmoothParams::DEFAULT),
            "terrain: mode=stored subdiv=1 bound=0 land-shift=0; drawing, collision and queries use this surface (smoothing off: --no-smooth-terrain or the Smooth terrain setting)"
        );
        assert!(describe_spec(SurfaceSpec::Stored, fit).ends_with("; the NVF_PLAY_TEST_SMOOTH_* recipe is not used)"));
    }

    /// The row's choices by INI text and by name, and the older on/off: on (as #2149 kept
    /// it, `1`) is 4 × 4, off is Off, the behaviour they stood for.
    #[test]
    fn the_choices_and_the_older_switch_parse() {
        for s in Smoothing::ALL {
            assert_eq!(Smoothing::parse(s.stored()), Some(s));
            assert_eq!(Smoothing::parse(s.name()), Some(s));
            assert_eq!(Smoothing::parse(&format!(" {} ", s.name().to_uppercase())), Some(s));
        }
        assert_eq!(Smoothing::parse("4×4"), Some(Smoothing::Sub4));
        for on in ["1", "true", "On", "yes"] {
            assert_eq!(Smoothing::parse(on), Some(Smoothing::Sub4), "{on}");
        }
        for off in ["0", "false", "off", "NO"] {
            assert_eq!(Smoothing::parse(off), Some(Smoothing::Off), "{off}");
        }
        for bad in ["", "3x3", "near", "2", "4"] {
            assert_eq!(Smoothing::parse(bad), None, "{bad}");
        }
        assert_eq!(Smoothing::DEFAULT, Smoothing::Sub4);
        assert!(Smoothing::ALL.iter().all(|s| Smoothing::of_code(s.code()) == *s));
    }

    /// Each choice's surface, and the old on/off's: on is exactly #2149's default surface
    /// (detail, 4 × 4, bound 8), off the stored triangles.
    #[test]
    fn each_choice_gives_its_surface() {
        let d = SmoothParams::DEFAULT;
        let on = Smoothing::parse("1").unwrap();
        assert_eq!(spec_of(on, d, false), SurfaceSpec::DEFAULT, "the older `on` keeps #2149's default");
        assert_eq!(spec_of(Smoothing::parse("0").unwrap(), d, false), SurfaceSpec::Stored);
        assert_eq!(spec_of(Smoothing::Sub2, d, false), SurfaceSpec::Smoothed(SmoothParams { subdiv: 2, ..d }));
        assert_eq!(spec_of(Smoothing::Sub4, d, false), SurfaceSpec::DEFAULT);
        assert_eq!(spec_of(Smoothing::Near, d, false), SurfaceSpec::DEFAULT, "4 x 4 near collides on the 4 x 4 surface");
        // The subdivision aid over the row's level; off stays off.
        let two = SmoothParams { subdiv: 2, ..d };
        assert_eq!(spec_of(Smoothing::Sub4, two, true), SurfaceSpec::Smoothed(two));
        assert_eq!(spec_of(Smoothing::Sub2, d, true), SurfaceSpec::DEFAULT);
        assert_eq!(spec_of(Smoothing::Off, two, true), SurfaceSpec::Stored);
        // The rule: 4 x 4 near on a 4 x 4 surface only.
        assert_eq!(rule_of(Smoothing::Near, SurfaceSpec::DEFAULT, FINE_NEAR), Some(DistanceRule { near: FINE_NEAR, far: FINE_NEAR + FINE_HYSTERESIS }));
        assert_eq!(rule_of(Smoothing::Near, SurfaceSpec::Smoothed(two), FINE_NEAR), None);
        assert_eq!(rule_of(Smoothing::Sub4, SurfaceSpec::DEFAULT, FINE_NEAR), None);
    }

    #[test]
    fn the_start_line_names_the_level_and_the_rule() {
        let d = SmoothParams::DEFAULT;
        let two = SurfaceSpec::Smoothed(SmoothParams { subdiv: 2, ..d });
        assert_eq!(describe_run(Smoothing::Sub2, two, d, None), "terrain: mode=detail(2:6) subdiv=2 bound=8 land-shift=4; drawing, collision and queries use this surface");
        assert_eq!(
            describe_run(Smoothing::Near, SurfaceSpec::DEFAULT, d, Some(DistanceRule::with_near(2048.0))),
            "terrain: mode=detail(2:6) subdiv=4 bound=8 land-shift=4; drawing, collision and queries use this surface; drawn 4 x 4 within 2048 units of the camera, 2 x 2 beyond 2560 (4 x 4 near)"
        );
        assert!(describe_run(Smoothing::Near, two, d, None).ends_with("; 4 x 4 near is not used at subdiv=2 (NVF_PLAY_TEST_SMOOTH_SUBDIV)"));
    }

    /// 4 × 4 inside the near distance, 2 × 2 beyond the far one, the level kept in between
    /// (no flicker at the boundary).
    #[test]
    fn the_rule_has_hysteresis() {
        let r = DistanceRule::with_near(2048.0);
        assert!(r.fine(0.0, false) && r.fine(2047.0, false) && !r.fine(2048.0, false));
        assert!(!r.fine(2300.0, false), "between the two a 2 x 2 quadrant stays 2 x 2");
        assert!(r.fine(2300.0, true) && r.fine(2560.0, true), "and a 4 x 4 one stays 4 x 4");
        assert!(!r.fine(2560.5, true));
        // Walking out and back across the band switches once each way.
        let mut fine = false;
        let mut switches = 0;
        for d in (0..=4000).chain((0..=4000).rev()).map(|d| d as f32).chain([2100.0, 2500.0, 2100.0, 2500.0]) {
            let next = r.fine(d, fine);
            switches += usize::from(next != fine);
            fine = next;
        }
        assert_eq!(switches, 3, "in at the start, out, in again; the wobble in the band switches nothing");
        assert_eq!(parse_near(None), (FINE_NEAR, None));
        assert_eq!(parse_near(Some(" 4096 ")), (4096.0, None));
        assert_eq!(parse_near(Some("0")).0, 0.0);
        for bad in ["-1", "x", "NaN", "70000"] {
            let (n, e) = parse_near(Some(bad));
            assert_eq!(n, FINE_NEAR);
            assert!(e.unwrap().starts_with(NEAR_AID), "{bad}");
        }
    }
}
