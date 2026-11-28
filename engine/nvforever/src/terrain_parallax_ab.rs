//! Test aid `NVF_PLAY_TEST_TERRAIN_PARALLAX_AB=<seconds>[:<word>]…[,…]` (#3322, terrain
//! parallax; terrain.rs "Parallax", docs/spec-notes/pbr-materials.md "Terrain layers:
//! parallax"): a readback of the world view that tells whether the terrain parallax moves the
//! ground's texture under the camera, for a smoke scenario over generated ground sets
//! (nvf_assets `NVF_PLAY_TEST_PBR_GROUND_PACK`). `--play` only; it makes the run unattended.
//!
//! At each time (on the world aids' clock, nvf-play test_aid.rs "Which clock") three
//! screenshots of the primary window are taken, A, B and C. The words after the time:
//! - `flip` (the default) or `hold`: with the Terrain parallax switch off, on, off (the aid
//!   sets `Graphics::terrain_parallax` as the settings row does, settings.rs, and puts the
//!   run's own value back after C), or with the switch left alone;
//! - `view` (the default), `grazing` or `head-on`: the view as the other aids posed it
//!   (`NVF_PLAY_TEST_LOOK`), or the aid's own pose on a spot of ground the march moves
//!   ([`find_spot`]): `grazing` stands the player [`GRAZING_PITCH_DEG`] degrees above the
//!   spot's horizon line, about three eye heights back, and aims the view at the spot;
//!   `head-on` stands the player on it looking straight down.
//!
//! - `base` (#3846; the first sequence that asks for a pose decides): the spot is on ground of
//!   a quadrant without painted layers whose base has a height map.
//!
//! **The spot** (once, at the first sequence that asks for a pose): among the loaded cells
//! around the player (3 x 3), the nearest landscape vertex where the march moves almost all of
//! what shows ([`SPOT_SHARE`] of it, at the vertex and its eight neighbours: terrain.rs
//! `CellCover`, the folded layers with height maps less the layers drawn over them, and the
//! physics' landscape, not a placed object, under it) and from which a grazing line of sight
//! reaches it over the landscape, from a stand on the physics' landscape inside the same
//! loaded cells. None found: a `FAIL` line and the sequence is skipped.
//!
//! **What is under the centre**, logged for every sequence before shot A (and again before shot
//! B, the switch on, when the first found nothing to move: heights from diffuse alphas are held
//! only while it is on): the physics ray
//! along the view meets the landscape of a cell or a placed object; for the landscape the
//! ground cover there (terrain.rs `ground_cover`, an estimate: the meshes' interpolation of
//! the painted opacities). Where the march would move
//! less than [`BARE_SHARE`] of it the verdict is `bare`: the readback says nothing about the
//! march. With the aid's own pose a cover that cannot be told counts as none (`bare`).
//!
//! Each shot waits until [`SETTLE_SECS`] have passed since its switch state was set
//! ([`POSE_SETTLE_SECS`] after a pose) and the pipeline gate counts no pipeline still
//! compiling (pipeline_gate.rs), at most [`CAP_SECS`] (then the verdict is `unsettled`). A shot
//! is requested only after the previous one was received, and the next state is set only then.
//! The pictures are saved as `terrain_parallax_ab_<seconds>s_<a|b|c>.png` in
//! `NVF_PLAY_TEST_SHOT_DIR` (the scenarios give each its own folder).
//!
//! The measure ([`measure`]), on each picture's brightness (the mean of the three colour
//! channels, 0..255) in a small box at the centre of the view ([`centre_box`]), the
//! crosshair's square left out of every sum ([`CROSS_PX`]): A-B, A-C and B-C, the mean absolute
//! difference (A-C, two pictures with the same switch state, is what changes anyway: grass,
//! clouds, light); the whole-pixel translation that best matches B to A ([`best_shift`]:
//! B(x, y) ≈ A(x - dx, y - dy), searched within [`search_radius`]) and the difference left at
//! it; each picture's contrast; and the correlation of each pair ([`correlation`]: normalised
//! cross-correlation, mean brightness and contrast taken out, so a uniform brightening or a
//! contrast change of an unchanged texture leaves it at 1). The verdict ([`judge`]), in this
//! order:
//! - `bare`: the centre is not ground the march moves (above);
//! - `flat`: a picture without texture to measure;
//! - `unsettled`: A-C past its own limits ([`STABLE_LEVELS`], [`STABLE_SHARE`] of the
//!   contrast, or a correlation under [`STABLE_CORRELATION`]): the scene changed by itself, so
//!   neither a shift nor its absence can be told;
//! - `moved`: B is A translated by at least [`MOVED_PX`], the translation explains most of the
//!   difference, and both A-B and B-C stand well above A-C (kept for a march that slides the
//!   texture as one; relief over the generated ridges warps it instead, so this is rare);
//! - `changed`: the switch made a real, stable change at the centre without one translation
//!   explaining it: the A-B and B-C correlations at most [`CHANGED_CORRELATION`] (the texture
//!   decorrelated, not merely brightened or reshaded);
//! - `still`: one rule: no fitted translation at all and A-B at most [`STILL_SHARE`] of A's
//!   contrast (A-C within its limits, as above);
//! - `unclear` otherwise.
//!
//! [design] **What is claimed.** At the grazing pose: `changed` or `moved`, a change the switch
//! alone makes (A and C, both off, agree) that decorrelates the texture at the centre. The
//! direction is not claimed: relief over ridges warps and stretches the texture (the
//! best-fit translation is logged as information only). Head-on: `still`, meaning no common
//! translation of the texture at the view's centre and a change of at most [`STILL_SHARE`] of
//! its spread; not that the picture is unchanged. The measured box reaches about 2 degrees
//! off the vertical, where the march at depth 0.08 still moves a sunk texel by a few pixels
//! (about depth x tan(2 degrees) = 0.003 texture units, near a pixel per world unit at 125
//! units), radially, and the marched relief changes what shows at ridge edges.
//!
//! [observed, the first run on the aid's spot, cc1ee98ea, (-71552, 3200) in cell (-18, 0),
//! shifted share 0.88 to 0.92 under the centre] Grazing (pitch -20.7): A-B 12.29, A-C 0.10,
//! B-C 12.29 levels, contrast 14.1, translation (+0, +0), the ridge grid in strong relief in
//! B only. Head-on: A-B 5.02, A-C 1.02, B-C 4.87, contrast 32.5, (+0, +0). The switch held
//! off at the same poses: 0.06 / 0.10 / 0.05 (contrast 14.1) and 1.65 / 1.02 / 1.52
//! (contrast 32.5). The run on 555ace098, its saved shots' correlations (A-B / A-C / B-C,
//! worked out from the PNGs with this box and mask): grazing 0.231 / 1.000 / 0.231, head-on
//! 0.945 / 0.998 / 0.949, held off 1.000 / 1.000 / 1.000 and 0.991 / 0.998 / 0.994. The
//! thresholds below are set from these with margins (each constant says which).
//!
//! Thresholds [engineering policy]: not facts about the original game; the main session
//! confirms them against later runs.

use crate::terrain::{ground_cover, CellCover, TerrainCache, TerrainMaterial, TerrainSettings};
use crate::world::WorldData;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use fnv_phys::{layers, Filter, Owner};
use nvf_play::pause::AidTime;
use nvf_play::phys::PhysState;
use nvf_play::player::{Player, MAX_PITCH};
use std::collections::VecDeque;
use std::path::PathBuf;

/// The aid's variable.
pub const ENV: &str = "NVF_PLAY_TEST_TERRAIN_PARALLAX_AB";

/// The least time between setting a switch state and its shot (seconds): the flipped
/// materials' edit reaches the GPU, any temporal filter settles.
pub const SETTLE_SECS: f32 = 2.0;

/// The least time between the aid's pose and shot A (seconds): the near landscape redrawn
/// around the new place.
pub const POSE_SETTLE_SECS: f32 = 5.0;

/// The most time a shot waits for the pipeline gate (seconds).
pub const CAP_SECS: f32 = 20.0;

/// The measured box: this share of the view's width and height, centred.
pub const BOX_SHARE: (f32, f32) = (0.06, 0.08);

/// Half the side of the square at the centre left out of the measure (the crosshair, up to
/// 26 px across at 720 lines), pixels.
pub const CROSS_PX: i32 = 14;

/// The least translation (pixels, the larger component) a `moved` verdict needs.
pub const MOVED_PX: i32 = 3;

/// A `moved` verdict needs A-B (and B-C) at least this many levels and this many times A-C.
pub const MOVED_LEVELS: f64 = 1.5;
pub const MOVED_RATIO: f64 = 2.0;

/// A `moved` verdict needs the difference left at the best translation no more than this
/// share of A-B: the translation explains the change.
pub const MOVED_FIT: f64 = 0.7;

/// A `still` verdict (no fitted translation) needs A-B no more than this share of
/// A's contrast. A picture changed altogether differs by about 1.1 times its spread (two
/// unrelated samples of one spread). Head-on the first run measured 0.15 (5.02 on 32.5), the
/// grazing view 0.87 (12.29 on 14.1): 0.3 is twice the head-on value and under half the
/// grazing one.
pub const STILL_SHARE: f64 = 0.3;

/// A `changed` verdict needs the A-B and B-C correlations at most this. An unrelated texture
/// gives about 0, an unchanged one 1. Grazing measured 0.231 (it would still pass at three
/// times that); head-on 0.945 and the held-off runs 0.991 and 1.000 stay well above it.
pub const CHANGED_CORRELATION: f64 = 0.7;
/// The least mean change (grey levels over the centre box) a `changed` verdict needs, A to B
/// and B to C: a decorrelation from a few pixels alone (one bright speck moving) stays out.
/// The grazing readback measured 12.29 [observed: smoke run of 2026-11-22]; head-on 5.05
/// is held back by its correlation (0.946), not by this floor.
pub const CHANGED_LEVELS: f64 = 4.0;

/// A-C's correlation may be no lower than this (measured 0.998 to 1.000: ten times the
/// largest loss seen).
pub const STABLE_CORRELATION: f64 = 0.98;

/// A-C, two pictures with the same switch state, may differ by at most the larger of this
/// many levels and this share of A's contrast; past it the verdict is `unsettled`.
pub const STABLE_LEVELS: f64 = 3.0;
pub const STABLE_SHARE: f64 = 0.15;

/// A picture whose box has a brightness spread (standard deviation) under this many levels
/// has nothing to measure a translation by.
pub const FLAT_LEVELS: f64 = 2.0;

/// Under this shifted share at the centre ([`crate::terrain::GroundCover::shifted`]) the
/// verdict is `bare`.
pub const BARE_SHARE: f32 = 0.5;

/// The shifted share a spot, and each of its eight neighbouring vertices, needs.
pub const SPOT_SHARE: f32 = 0.9;

/// The grazing pose's angle below the horizon, degrees.
pub const GRAZING_PITCH_DEG: f32 = 20.0;

/// The most spots tried for a line of sight, nearest first.
const SPOT_TRIES: usize = 300;

/// How the view is set for a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    /// As the other aids set it.
    View,
    Grazing,
    HeadOn,
}

/// One timed sequence: when, whether the switch is left alone, and the pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    pub at: f32,
    pub hold: bool,
    pub pose: Pose,
    /// The pose's spot on a base alone (#3846).
    pub base: bool,
}

/// `<seconds>[:<word>]…[,…]`, in time order (module notes).
pub fn parse(value: &str) -> Result<Vec<Spec>, String> {
    let mut specs = Vec::new();
    for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let mut words = item.split(':').map(str::trim);
        let time = words.next().unwrap_or_default();
        let at: f32 = time.parse().map_err(|_| format!("{item:?}: a time in seconds first"))?;
        let mut spec = Spec { at, hold: false, pose: Pose::View, base: false };
        for w in words {
            match w {
                "flip" => spec.hold = false,
                "hold" => spec.hold = true,
                "view" => spec.pose = Pose::View,
                "grazing" => spec.pose = Pose::Grazing,
                "head-on" => spec.pose = Pose::HeadOn,
                "base" => spec.base = true,
                _ => return Err(format!("{item:?}: {w:?} is none of flip, hold, view, grazing, head-on, base")),
            }
        }
        specs.push(spec);
    }
    if specs.is_empty() {
        return Err("no time given".into());
    }
    specs.sort_by(|a, b| a.at.total_cmp(&b.at));
    Ok(specs)
}

/// One picture's brightness, row by row.
#[derive(Debug, Clone, Default)]
pub struct Luma {
    pub w: usize,
    pub h: usize,
    pub v: Vec<f32>,
}

impl Luma {
    /// From four bytes a pixel, row by row (RGBA or BGRA alike: the three colour channels'
    /// mean). `None` for any other size.
    pub fn from_pixels(bytes: &[u8], w: usize, h: usize) -> Option<Luma> {
        if w == 0 || h == 0 || bytes.len() != w * h * 4 {
            return None;
        }
        let v = bytes.as_chunks::<4>().0.iter().map(|p| (f32::from(p[0]) + f32::from(p[1]) + f32::from(p[2])) / 3.0).collect();
        Some(Luma { w, h, v })
    }

    fn at(&self, x: i32, y: i32) -> f32 {
        self.v[y as usize * self.w + x as usize]
    }
}

/// A box of the picture: left, top, width, height (pixels).
pub type Rect = (i32, i32, i32, i32);

/// The measured box for a `w` x `h` view ([`BOX_SHARE`], centred, at least 8 pixels a side).
pub fn centre_box(w: usize, h: usize) -> Rect {
    let bw = ((w as f32 * BOX_SHARE.0) as i32).max(8);
    let bh = ((h as f32 * BOX_SHARE.1) as i32).max(8);
    ((w as i32 - bw) / 2, (h as i32 - bh) / 2, bw, bh)
}

/// The translation searched, at most, for a `w` x `h` view: a 40th of the width sideways, a
/// little more than an eighth of the height up and down (the grazing view's shift runs along
/// the screen's vertical), both held inside the picture around [`centre_box`].
pub fn search_radius(w: usize, h: usize) -> (i32, i32) {
    let (x0, y0, bw, bh) = centre_box(w, h);
    let rx = (w as i32 / 40).min(x0).min(w as i32 - x0 - bw);
    let ry = (h as i32 * 2 / 15).min(y0).min(h as i32 - y0 - bh);
    (rx.max(0), ry.max(0))
}

/// Whether a pixel lies in the crosshair's square, left out of the measure.
fn crosshair(l: &Luma, x: i32, y: i32) -> bool {
    (x - l.w as i32 / 2).abs() < CROSS_PX && (y - l.h as i32 / 2).abs() < CROSS_PX
}

/// The mean absolute difference between `b` over `rect` and `a` translated by `(dx, dy)`
/// (`b(x, y)` against `a(x - dx, y - dy)`), every `step`th pixel, the crosshair left out on
/// both sides. `None` when no pixel was compared or the translation leaves `a`.
pub fn difference(a: &Luma, b: &Luma, rect: Rect, (dx, dy): (i32, i32), step: usize) -> Option<f64> {
    let (x0, y0, bw, bh) = rect;
    let (mut sum, mut n) = (0.0f64, 0u32);
    for y in (y0..y0 + bh).step_by(step) {
        for x in (x0..x0 + bw).step_by(step) {
            let (ax, ay) = (x - dx, y - dy);
            if ax < 0 || ay < 0 || ax >= a.w as i32 || ay >= a.h as i32 || x >= b.w as i32 || y >= b.h as i32 {
                return None;
            }
            if crosshair(b, x, y) || crosshair(a, ax, ay) {
                continue;
            }
            sum += f64::from((b.at(x, y) - a.at(ax, ay)).abs());
            n += 1;
        }
    }
    (n > 0).then(|| sum / f64::from(n))
}

/// The whole-pixel translation within `radius` that best matches `b` to `a` over `rect`
/// (searched on every second pixel, the nearest to none winning a tie) and the difference left
/// at it over every pixel.
pub fn best_shift(a: &Luma, b: &Luma, rect: Rect, radius: (i32, i32)) -> Option<((i32, i32), f64)> {
    let mut best: Option<((i32, i32), f64)> = None;
    for dy in -radius.1..=radius.1 {
        for dx in -radius.0..=radius.0 {
            let Some(d) = difference(a, b, rect, (dx, dy), 2) else { continue };
            let better = match best {
                None => true,
                Some(((bx, by), bd)) => d < bd || (d == bd && dx.abs() + dy.abs() < bx.abs() + by.abs()),
            };
            if better {
                best = Some(((dx, dy), d));
            }
        }
    }
    let (shift, _) = best?;
    Some((shift, difference(a, b, rect, shift, 1)?))
}

/// The brightness spread (standard deviation, levels) over `rect`, the crosshair's square left
/// out as in [`difference`].
pub fn contrast(l: &Luma, rect: Rect) -> f64 {
    let (x0, y0, bw, bh) = rect;
    let values: Vec<f64> = (y0..y0 + bh)
        .flat_map(|y| (x0..x0 + bw).map(move |x| (x, y)))
        .filter(|&(x, y)| x >= 0 && y >= 0 && (x as usize) < l.w && (y as usize) < l.h && !crosshair(l, x, y))
        .map(|(x, y)| f64::from(l.at(x, y)))
        .collect();
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
}

/// What [`measure`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    pub rect: Rect,
    /// A-B, A-C and B-C, mean absolute difference (levels).
    pub ab: f64,
    pub ac: f64,
    pub bc: f64,
    /// B = A translated by this (pixels; dy > 0 down the screen).
    pub shift: (i32, i32),
    /// A-B left at that translation.
    pub fit: f64,
    /// Each picture's contrast over the box.
    pub contrast: [f64; 3],
    /// The correlations A-B, A-C and B-C ([`correlation`]).
    pub correlation: [f64; 3],
}

/// The normalised cross-correlation of `a` and `b` over `rect`, the crosshair left out as in
/// [`difference`]: each picture's mean taken out and the products divided by both spreads, so
/// 1 for the same texture whatever its brightness or contrast, about 0 for unrelated ones; 0
/// when either is uniform (the `flat` verdict comes first).
pub fn correlation(a: &Luma, b: &Luma, rect: Rect) -> f64 {
    let (x0, y0, bw, bh) = rect;
    let points: Vec<(f64, f64)> = (y0..y0 + bh)
        .flat_map(|y| (x0..x0 + bw).map(move |x| (x, y)))
        .filter(|&(x, y)| x >= 0 && y >= 0 && (x as usize) < a.w.min(b.w) && (y as usize) < a.h.min(b.h) && !crosshair(a, x, y))
        .map(|(x, y)| (f64::from(a.at(x, y)), f64::from(b.at(x, y))))
        .collect();
    if points.is_empty() {
        return 0.0;
    }
    let n = points.len() as f64;
    let (ma, mb) = (points.iter().map(|p| p.0).sum::<f64>() / n, points.iter().map(|p| p.1).sum::<f64>() / n);
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in &points {
        ab += (x - ma) * (y - mb);
        aa += (x - ma).powi(2);
        bb += (y - mb).powi(2);
    }
    if aa <= 0.0 || bb <= 0.0 { 0.0 } else { ab / (aa * bb).sqrt() }
}

/// The measure of three pictures of one size; `Err` with why not.
pub fn measure(a: &Luma, b: &Luma, c: &Luma) -> Result<Measure, String> {
    if a.w != b.w || a.h != b.h || a.w != c.w || a.h != c.h {
        return Err(format!("the pictures differ in size ({}x{}, {}x{}, {}x{})", a.w, a.h, b.w, b.h, c.w, c.h));
    }
    let rect = centre_box(a.w, a.h);
    let ab = difference(a, b, rect, (0, 0), 1).ok_or("no pixel to compare")?;
    let ac = difference(a, c, rect, (0, 0), 1).ok_or("no pixel to compare")?;
    let bc = difference(c, b, rect, (0, 0), 1).ok_or("no pixel to compare")?;
    let (shift, fit) = best_shift(a, b, rect, search_radius(a.w, a.h)).ok_or("no translation to compare")?;
    let correlation = [correlation(a, b, rect), correlation(a, c, rect), correlation(b, c, rect)];
    Ok(Measure { rect, ab, ac, bc, shift, fit, contrast: [contrast(a, rect), contrast(b, rect), contrast(c, rect)], correlation })
}

/// The verdict on a [`Measure`] (module notes); `shifted` the share of the centre the march
/// moves (`None`: not known, not judged).
pub fn judge(m: &Measure, shifted: Option<f32>) -> &'static str {
    if shifted.is_some_and(|s| s < BARE_SHARE) {
        return "bare";
    }
    if m.contrast.iter().any(|&c| c < FLAT_LEVELS) {
        return "flat";
    }
    if m.ac > STABLE_LEVELS.max(STABLE_SHARE * m.contrast[0]) || m.correlation[1] < STABLE_CORRELATION {
        return "unsettled";
    }
    let len = m.shift.0.abs().max(m.shift.1.abs());
    if len >= MOVED_PX && m.ab >= MOVED_LEVELS && m.ab >= MOVED_RATIO * m.ac && m.bc >= MOVED_RATIO * m.ac && m.fit <= MOVED_FIT * m.ab {
        "moved"
    } else if m.correlation[0] <= CHANGED_CORRELATION && m.correlation[2] <= CHANGED_CORRELATION && m.ab >= CHANGED_LEVELS && m.bc >= CHANGED_LEVELS {
        "changed"
    } else if len == 0 && m.ab <= STILL_SHARE * m.contrast[0] {
        "still"
    } else {
        "unclear"
    }
}

/// The result line's words after the time and mode.
pub fn describe(m: &Measure) -> String {
    format!(
        "A-B {:.2}, A-C {:.2}, B-C {:.2} levels over {}x{} px at the centre; B = A moved by ({:+}, {:+}) px (A-B {:.2} there); contrast {:.1}/{:.1}/{:.1}; correlation {:.3}/{:.3}/{:.3}",
        m.ab, m.ac, m.bc, m.rect.2, m.rect.3, m.shift.0, m.shift.1, m.fit, m.contrast[0], m.contrast[1], m.contrast[2], m.correlation[0], m.correlation[1], m.correlation[2]
    )
}

/// A spot of ground the march moves and where to stand to see it at a grazing angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    /// The ground point (game space).
    pub target: [f32; 3],
    /// The grazing pose's standing point (its ground) and the heading towards the target.
    pub stand: [f32; 3],
    pub heading_deg: f32,
}

/// The landscape vertices of the loaded cells around `feet` whose shifted share, and that of
/// each of their eight neighbours, reaches [`SPOT_SHARE`], nearest first: (point, the cover's
/// description).
///
/// Of the ground the march moves, the layers' part alone counts (not the base's, #3846), so the
/// spot is the same whether the base has a height map or not; with `base` instead, only
/// quadrants without painted layers, whose base has one.
fn spot_candidates(world: &WorldData, cache: &TerrainCache, materials: &Assets<TerrainMaterial>, tile: f32, feet: [f32; 3], base: bool) -> Vec<([f32; 2], String)> {
    let moved = |c: &crate::terrain::GroundCover| if base { if c.folded == 0 && c.over == 0 { c.shifted() } else { 0.0 } } else { c.layers_shifted() };
    let size = fnv_world::CELL_SIZE;
    let spacing = fnv_world::land::SPACING;
    let half = size / 2.0;
    let g0 = ((feet[0] / size).floor() as i32, (feet[1] / size).floor() as i32);
    let mut found: Vec<(f32, [f32; 2], String)> = Vec::new();
    for gy in g0.1 - 1..=g0.1 + 1 {
        for gx in g0.0 - 1..=g0.0 + 1 {
            let Some(cell) = CellCover::new(world, cache, materials, tile, (gx, gy)) else { continue };
            let side = (half / spacing).round() as i32;
            for (qx, qy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                for r in 1..side {
                    for c in 1..side {
                        let (lx, ly) = (qx as f32 * half + c as f32 * spacing, qy as f32 * half + r as f32 * spacing);
                        let here = cell.at(lx, ly);
                        if moved(&here) < SPOT_SHARE {
                            continue;
                        }
                        let around = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy))).all(|(dx, dy)| moved(&cell.at(lx + dx as f32 * spacing, ly + dy as f32 * spacing)) >= SPOT_SHARE);
                        if !around {
                            continue;
                        }
                        let p = [gx as f32 * size + lx, gy as f32 * size + ly];
                        let d2 = (p[0] - feet[0]).powi(2) + (p[1] - feet[1]).powi(2);
                        found.push((d2, p, here.describe()));
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, p, d)| (p, d)).collect()
}

/// The nearest spot (module notes): `Ok((spot, the cover's description))`, or why none.
pub fn find_spot(world: &WorldData, cache: &TerrainCache, materials: &Assets<TerrainMaterial>, tile: f32, phys: &mut PhysState, player: &Player, base: bool) -> Result<(Spot, String), String> {
    let candidates = spot_candidates(world, cache, materials, tile, player.feet, base);
    if candidates.is_empty() {
        return Err(format!("no landscape vertex in the 3 x 3 cells around ({:.0}, {:.0}) where the march moves {SPOT_SHARE} of what shows", player.feet[0], player.feet[1]));
    }
    let eye_height = if player.eye_height > 0.0 { player.eye_height } else { 120.0 };
    let back = eye_height / GRAZING_PITCH_DEG.to_radians().tan();
    let filter = || {
        let mut f = Filter::default();
        f.exclude_layers.insert(layers::TRIGGER);
        f
    };
    let terrain = |owner: Owner| matches!(owner, Owner::Terrain { .. });
    // The 3 x 3 cells the candidates come from, loaded around the player: the stand must lie
    // in them too, on their landscape (not a roof or an overhang).
    let size = fnv_world::CELL_SIZE;
    let home = ((player.feet[0] / size).floor() as i32, (player.feet[1] / size).floor() as i32);
    let loaded = |x: f32, y: f32| ((x / size).floor() as i32 - home.0).abs() <= 1 && ((y / size).floor() as i32 - home.1).abs() <= 1;
    for (p, described) in candidates.iter().take(SPOT_TRIES) {
        let Some(down) = phys.ray([p[0], p[1], player.feet[2] + 4000.0], [0.0, 0.0, -1.0], 12_000.0, filter()) else { continue };
        if !terrain(down.owner) {
            continue;
        }
        let target = down.point;
        for heading_deg in [0.0f32, 90.0, 180.0, 270.0, 45.0, 135.0, 225.0, 315.0] {
            let (s, c) = heading_deg.to_radians().sin_cos();
            let (sx, sy) = (target[0] - back * s, target[1] - back * c);
            if !loaded(sx, sy) {
                continue;
            }
            let Some(ground) = phys.ray([sx, sy, target[2] + 2000.0], [0.0, 0.0, -1.0], 6000.0, filter()) else { continue };
            if !terrain(ground.owner) {
                continue;
            }
            let eye = [sx, sy, ground.point[2] + eye_height];
            let to = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
            let length = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
            let Some(seen) = phys.ray(eye, to, length + 50.0, filter()) else { continue };
            let off = ((seen.point[0] - target[0]).powi(2) + (seen.point[1] - target[1]).powi(2) + (seen.point[2] - target[2]).powi(2)).sqrt();
            if terrain(seen.owner) && off < 40.0 {
                return Ok((Spot { target, stand: ground.point, heading_deg }, described.clone()));
            }
        }
    }
    Err(format!("{} landscape vertices around ({:.0}, {:.0}) qualify, but none of the nearest {SPOT_TRIES} has the physics' landscape under it and a grazing line of sight to it", candidates.len(), player.feet[0], player.feet[1]))
}

#[derive(Resource)]
struct Ab {
    queue: VecDeque<Spec>,
    run: Option<Run>,
    captured: [Option<Luma>; 3],
    dir: PathBuf,
    /// The spot, once looked for.
    spot: Option<Result<Spot, String>>,
}

struct Run {
    spec: Spec,
    phase: usize,
    since: f32,
    waiting: bool,
    unsettled: bool,
    /// The run's own switch value, put back after C (flip only).
    restore: bool,
    /// The shifted share under the centre (`None`: not known), measured before shot A.
    shifted: Option<Option<f32>>,
}

const NAMES: [&str; 3] = ["a", "b", "c"];

/// The switch state of each shot in a flipped sequence.
const FLIP: [bool; 3] = [false, true, false];

/// Installed when the variable is set (`--play` only: the aids' clock).
pub fn install(app: &mut App) {
    let Ok(value) = std::env::var(ENV) else { return };
    match parse(&value) {
        Ok(specs) => {
            let listed: Vec<String> = specs
                .iter()
                .map(|s| {
                    let pose = match s.pose {
                        Pose::View => "",
                        Pose::Grazing => ", grazing",
                        Pose::HeadOn => ", head-on",
                    };
                    format!("{} s ({}{pose})", s.at, if s.hold { "hold" } else { "flip" })
                })
                .collect();
            info!("test aid {ENV}: {} sequences of three shots: {}", specs.len(), listed.join(", "));
            let dir = std::env::var_os("NVF_PLAY_TEST_SHOT_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
            app.insert_resource(Ab { queue: specs.into(), run: None, captured: [None, None, None], dir, spot: None }).add_systems(Update, tick);
        }
        Err(e) => error!("test aid {ENV}: FAIL {e}"),
    }
}

fn set_switch(graphics: Option<&mut crate::graphics::Graphics>, on: bool, at: f32) {
    match graphics {
        Some(g) if g.terrain_parallax != on => {
            g.terrain_parallax = on;
            info!("terrain parallax ab at {at} s: switch {}", if on { "on" } else { "off" });
        }
        Some(_) => {}
        None => warn!("terrain parallax ab at {at} s: no graphics settings to switch"),
    }
}

/// The landscape's state the aid reads.
type Land<'w> = (Option<Res<'w, WorldData>>, Option<Res<'w, TerrainCache>>, Option<Res<'w, Assets<TerrainMaterial>>>, Option<Res<'w, TerrainSettings>>);

/// Put the player in `pose` at the spot; `false` when there is none (logged once).
fn pose_player(ab: &mut Ab, pose: Pose, base: bool, at: f32, land: &Land, phys: &mut PhysState, player: &mut Player) -> bool {
    if ab.spot.is_none() {
        let found = match land {
            (Some(world), Some(cache), Some(materials), Some(settings)) => find_spot(world, cache, materials, settings.tile, phys, player, base),
            _ => Err("no landscape state to look in".into()),
        };
        match &found {
            Ok((spot, described)) => info!(
                "terrain parallax ab: spot ({:.0}, {:.0}, {:.0}) for the readback, {described}; the grazing pose stands at ({:.0}, {:.0}) heading {}",
                spot.target[0], spot.target[1], spot.target[2], spot.stand[0], spot.stand[1], spot.heading_deg
            ),
            Err(e) => warn!("terrain parallax ab: FAIL no spot: {e}"),
        }
        ab.spot = Some(found.map(|(s, _)| s));
    }
    let Some(Ok(spot)) = ab.spot else { return false };
    let stand = match pose {
        Pose::Grazing => spot.stand,
        _ => spot.target,
    };
    phys.place_player([stand[0], stand[1], stand[2] + 5.0], 60.0, 1000.0);
    let feet = phys.player.as_ref().map_or(stand, |c| c.feet);
    player.feet = feet;
    match pose {
        Pose::Grazing => {
            let eye = [feet[0], feet[1], feet[2] + player.eye_height];
            let (heading, pitch) = nvf_play::control::Crosshair::aim_at(None, eye, spot.target);
            (player.heading, player.pitch) = (heading, pitch.clamp(-MAX_PITCH, MAX_PITCH));
        }
        _ => (player.heading, player.pitch) = (spot.heading_deg.to_radians(), -MAX_PITCH),
    }
    info!(
        "terrain parallax ab at {at} s: pose {}: feet ({:.0}, {:.0}, {:.0}), heading {:.1}, pitch {:.1}",
        if pose == Pose::Grazing { "grazing" } else { "head-on" },
        feet[0], feet[1], feet[2], player.heading.to_degrees(), player.pitch.to_degrees()
    );
    true
}

/// What the view's centre meets, logged; the shifted share there for the landscape, 0 for
/// anything else, `None` when it cannot be told.
fn under_centre(at: f32, land: &Land, phys: Option<&mut PhysState>, player: Option<&Player>) -> Option<f32> {
    let (Some(phys), Some(player)) = (phys, player) else {
        info!("terrain parallax ab at {at} s: under the centre: not known (no physics or player)");
        return None;
    };
    let mut filter = Filter::default();
    filter.exclude_layers.insert(layers::TRIGGER);
    let Some(hit) = phys.ray(player.eye(), player.look_dir(), 8000.0, filter) else {
        info!("terrain parallax ab at {at} s: under the centre: nothing within 8000 units");
        return Some(0.0);
    };
    let p = hit.point;
    match hit.owner {
        Owner::Terrain { .. } => {
            let cover = match land {
                (Some(world), Some(cache), Some(materials), Some(settings)) => ground_cover(world, cache, materials, settings.tile, p[0], p[1]),
                _ => None,
            };
            match cover {
                Some(c) => {
                    info!("terrain parallax ab at {at} s: under the centre: the landscape at ({:.0}, {:.0}, {:.0}), {:.0} units away, {}", p[0], p[1], p[2], hit.distance, c.describe());
                    Some(c.shifted())
                }
                None => {
                    info!("terrain parallax ab at {at} s: under the centre: the landscape at ({:.0}, {:.0}, {:.0}), its layers not known", p[0], p[1], p[2]);
                    None
                }
            }
        }
        other => {
            info!("terrain parallax ab at {at} s: under the centre: {other:?} at ({:.0}, {:.0}, {:.0}), {:.0} units away (not the landscape)", p[0], p[1], p[2], hit.distance);
            Some(0.0)
        }
    }
}

fn tick(
    time: AidTime,
    mut ab: ResMut<Ab>,
    (mut graphics, gate): (Option<ResMut<crate::graphics::Graphics>>, Option<Res<crate::pipeline_gate::PipelineGate>>),
    (mut phys, mut player): (Option<ResMut<PhysState>>, Option<ResMut<Player>>),
    land: Land,
    mut commands: Commands,
) {
    let Some(now) = time.world_secs() else { return };
    let ab = &mut *ab;
    if ab.run.is_none() {
        let Some(&spec) = ab.queue.front().filter(|s| s.at <= now) else { return };
        ab.queue.pop_front();
        if spec.pose != Pose::View {
            let placed = match (phys.as_deref_mut(), player.as_deref_mut()) {
                (Some(phys), Some(player)) => pose_player(ab, spec.pose, spec.base, spec.at, &land, phys, player),
                _ => {
                    warn!("terrain parallax ab at {} s: FAIL no physics or player to pose", spec.at);
                    false
                }
            };
            if !placed {
                warn!("terrain parallax ab at {} s: FAIL sequence skipped (no pose)", spec.at);
                return;
            }
        }
        let restore = graphics.as_deref().is_some_and(|g| g.terrain_parallax);
        if !spec.hold {
            set_switch(graphics.as_deref_mut(), FLIP[0], spec.at);
        }
        ab.captured = [None, None, None];
        ab.run = Some(Run { spec, phase: 0, since: now, waiting: false, unsettled: false, restore, shifted: None });
        return;
    }
    let run = ab.run.as_mut().expect("a sequence runs");
    let at = run.spec.at;
    if run.waiting {
        if ab.captured[run.phase].is_none() {
            return;
        }
        run.waiting = false;
        run.phase += 1;
        run.since = now;
        if run.phase < 3 {
            if !run.spec.hold {
                set_switch(graphics.as_deref_mut(), FLIP[run.phase], at);
            }
            return;
        }
        // All three received.
        // With the aid's own pose an unknown cover counts as none: the pose is there to look at
        // ground the march moves, so not knowing that is no pass.
        let shifted = match (run.spec.pose, run.shifted.flatten()) {
            (Pose::View, s) => s,
            (_, s) => Some(s.unwrap_or(0.0)),
        };
        let (hold, unsettled, restore) = (run.spec.hold, run.unsettled, run.restore);
        ab.run = None;
        if !hold {
            set_switch(graphics.as_deref_mut(), restore, at);
        }
        let mode = if hold { "hold" } else { "flip" };
        let [Some(a), Some(b), Some(c)] = std::mem::take(&mut ab.captured) else { unreachable!("every shot received") };
        match measure(&a, &b, &c) {
            Ok(m) => {
                let verdict = match judge(&m, shifted) {
                    "bare" => "bare",
                    _ if unsettled => "unsettled",
                    v => v,
                };
                info!("terrain parallax ab at {at} s ({mode}): {}; {verdict}", describe(&m));
            }
            Err(e) => warn!("terrain parallax ab at {at} s ({mode}): FAIL {e}"),
        }
        return;
    }
    let waited = now - run.since;
    let settle = if run.phase == 0 && run.spec.pose != Pose::View { POSE_SETTLE_SECS } else { SETTLE_SECS };
    let busy = gate.as_deref().map_or(0, |g| g.busy());
    if waited < settle || (busy > 0 && waited < CAP_SECS) {
        return;
    }
    // Told once, before shot A; again with the switch on when nothing there was to be moved
    // with it off: heights from diffuse alphas are held only while it is on (terrain.rs).
    let bare = |s: Option<f32>| s.unwrap_or(0.0) < BARE_SHARE;
    if run.shifted.is_none() || (run.phase == 1 && !run.spec.hold && run.shifted.is_some_and(bare)) {
        run.shifted = Some(under_centre(at, &land, phys.as_deref_mut(), player.as_deref()));
    }
    run.unsettled |= busy > 0;
    run.waiting = true;
    let phase = run.phase;
    let state = match (run.spec.hold, graphics.as_deref()) {
        (false, _) => {
            if FLIP[phase] {
                "on"
            } else {
                "off"
            }
        }
        (true, Some(g)) if g.terrain_parallax => "on (held)",
        (true, _) => "off (held)",
    };
    let path = ab.dir.join(format!("terrain_parallax_ab_{at}s_{}.png", NAMES[phase]));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(dir);
    }
    info!("terrain parallax ab at {at} s: shot {} with the switch {state} (after {waited:.1} s, {busy} pipelines busy): {}", NAMES[phase], path.display());
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path)).observe(move |ev: On<ScreenshotCaptured>, mut ab: ResMut<Ab>| {
        let size = ev.image.texture_descriptor.size;
        let luma = ev.image.data.as_deref().and_then(|d| Luma::from_pixels(d, size.width as usize, size.height as usize));
        if luma.is_none() {
            warn!("terrain parallax ab: shot {} not four bytes a pixel ({:?}); measured as empty", NAMES[phase], ev.image.texture_descriptor.format);
        }
        ab.captured[phase] = Some(luma.unwrap_or_default());
    });
}

#[cfg(test)]
mod tests;
