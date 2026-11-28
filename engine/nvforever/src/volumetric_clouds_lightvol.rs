//! Volumetric clouds' light volume (#3032 stage 2g, checkpoint A; docs/design/graphics-enhanced.md
//! §3.23): the CPU mirror of a slowly updated 3D texture of the optical depth toward the sun around
//! the camera, which a lit view sample reads once (trilinear) after marching only the nearest of
//! the eight light intervals itself, its schedule and its GPU passes. An engineering choice for the enhanced
//! look, not a fact about the original game.
//!
//! Sources [documented: cited publications]: Bauer 2019, "Creating the Atmospheric World of Red
//! Dead Redemption 2", slides 33 and 35 (one density lookup toward the sun on a cloud sample, the
//! rest of the sun's visibility from a cloud shadow map updated a part at a time); Kim and Neumann
//! 2001, "Opacity Shadow Maps" (opacity toward the light accumulated on slices and interpolated);
//! Schneider 2015, "The Real-time Volumetric Cloudscapes of Horizon Zero Dawn" (the cone of light
//! taps this replaces). The forms below are our own.
//!
//! The split [design]: the reader marches the first `k` of the eight
//! intervals (the partition computed with denominator eight) to the handoff point and reads the volume there:
//! the depth from that node's height to the bent top.
//!
//! Addressing [design]: the density translates rigidly with
//! the published unwrapped drift in field coordinates `X = world xz + repeat x drift` and bent
//! height above the base (the test `the_field_moves_rigidly_with_the_drift` holds it against the
//! uniform), so the lattice is addressed in signed, floor-snapped field nodes and bent height.
//! The directional integral at a node depends on the camera only through the bend's tilt
//! (`s ΔC · L_xz / R` of height over `s`): a bake keeps its frozen camera.

use crate::volumetric_clouds::{bent_y, light_intervals, light_reach, roots, CloudVolumeUniform, LIGHT_STEPS};
use bevy::math::{DVec2, IVec3, Vec2, Vec3, Vec4};

/// The near level's lattice [design]: 128 x 128 horizontal nodes 500 apart
/// (±31,750 about the window's centre); the tests' sweep has its layers [`LV_UP`] apart.
#[cfg_attr(not(test), allow(dead_code))]
pub const LV_ACROSS: f32 = 500.0;
#[cfg_attr(not(test), allow(dead_code))]
pub const LV_UP: f32 = 500.0;
#[cfg_attr(not(test), allow(dead_code))]
pub const LV_NODES: u32 = 128;
/// The volume's one declared footprint (width and placement width, units) [design:
/// an overhead half-buffer sample's].
#[cfg_attr(not(test), allow(dead_code))]
pub const LV_FOOTPRINT: f32 = 64.0;
/// The sweep producer's longest substep along the sun between two layers (units)
/// [design].
#[cfg_attr(not(test), allow(dead_code))]
pub const LV_SWEEP_STEP: f32 = 250.0;
/// The largest depth a half float holds.
pub const LV_HALF_MOST: f32 = 65_504.0;
/// The two nested levels of checkpoint B1 [design]: the near level 128 x 128
/// nodes 500 apart, the far level 128 x 128 nodes 4,000 apart (it covers 254,000 units about the
/// camera, where 94 to 97 % of the lit samples are [hypothesis: counted on the CPU mirror of the body of
/// 2026-11-20 by a trial test removed with that body]), both a bent-height layer every 1,000 units over the domain (25 at
/// the run's 24,000, 41 at the tests' 40,000).
pub const LV_FAR_ACROSS: f32 = 4_000.0;
pub const LV_LEVEL_UP: f32 = 1_000.0;
/// The far level's footprint (width = placement width, units) [design]: its cell, so the field it
/// samples is filtered to what a 4,000-unit lattice can hold (a narrower footprint aliases the
/// sub-cell detail into the depth between nodes; the near level keeps the overhead sample's 64).
pub const LV_FAR_FOOTPRINT: f32 = LV_FAR_ACROSS;
/// The guard band (cells of the inner level) over which a level hands over to the next one out
/// (near to far, far to the legacy taps): a convex mix, never a seam [design].
pub const LV_GUARD: f32 = 4.0;

/// A lattice: nodes `across` units apart in field x and z, `nodes` of them each way, `layers`
/// bent-height nodes `up` units apart from the base (the first at the base, the last at the top).
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LvGrid {
    pub across: f32,
    pub up: f32,
    pub nodes: u32,
    pub layers: u32,
}

#[cfg_attr(not(test), allow(dead_code))]
impl LvGrid {
    /// A lattice over a domain `domain` units tall: layers `up` apart (at least two layers, the
    /// spacing shortened to fit the domain exactly).
    pub fn new(across: f32, up: f32, nodes: u32, domain: f32) -> LvGrid {
        let layers = ((domain / up.max(1.0)).ceil() as u32 + 1).max(2);
        LvGrid { across: across.max(1.0), up: domain.max(1.0) / (layers - 1) as f32, nodes: nodes.max(2), layers }
    }
    pub fn texels(&self) -> u64 {
        u64::from(self.nodes) * u64::from(self.nodes) * u64::from(self.layers)
    }
    /// One copy in R16Float.
    pub fn bytes(&self) -> u64 {
        2 * self.texels()
    }
}

/// Where a window sits: its lowest node in signed field nodes (x, z) and the camera's field
/// position in nodes from that corner.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LvWindow {
    pub lo: [i64; 2],
    pub cam: Vec2,
}

/// The field position (units) of a world position `world` under the published unwrapped drift
/// `drift` (texture repeats) at `repeat` units a repeat: `world + repeat x drift`, in f64.
#[cfg_attr(not(test), allow(dead_code))]
pub fn lv_field(world: DVec2, drift: [f64; 2], repeat: f64) -> DVec2 {
    let d = |v: f64| if v.is_finite() { v * repeat } else { 0.0 };
    world + DVec2::new(d(drift[0]), d(drift[1]))
}

/// The window of `g` about the camera's field position `cam_field`: floor-snapped to whole nodes,
/// the camera between the two middle nodes (so it is covered `g.covered()` each way).
#[cfg_attr(not(test), allow(dead_code))]
pub fn lv_window(cam_field: DVec2, g: &LvGrid) -> LvWindow {
    let across = f64::from(g.across);
    let half = i64::from(g.nodes / 2 - 1);
    let snap = |v: f64| if v.is_finite() { (v / across).floor() as i64 - half } else { -half };
    let lo = [snap(cam_field.x), snap(cam_field.y)];
    LvWindow { lo, cam: lv_node_pos(cam_field, lo, g) }
}

/// The field position `field` in nodes of `g` from a window's lowest node `lo` (f64 until the
/// difference is taken, so a far drift keeps its precision; a position that is not finite stands
/// between the middle nodes).
pub fn lv_node_pos(field: DVec2, lo: [i64; 2], g: &LvGrid) -> Vec2 {
    let across = f64::from(g.across);
    let half = (g.nodes / 2 - 1) as f32;
    let pos = |v: f64, l: i64| if v.is_finite() { (v / across - l as f64) as f32 } else { half };
    Vec2::new(pos(field.x, lo[0]), pos(field.y, lo[1]))
}

/// The camera-relative height of the point at horizontal `(x, z)` from the camera whose bent
/// height is `h` above the base (the inverse of [`bent_y`]).
fn unbend(x: f32, z: f32, h: f32, cam_y: f32, u: &CloudVolumeUniform) -> f32 {
    u.slab.x + h - cam_y - (x * x + z * z) / (2.0 * u.reach.y.max(1.0))
}

/// The trilinear read at lattice coordinate `c` over `layers` layers, the layer clamped to the
/// lattice (above the top the top's value, 0 by construction), the nodes from `fetch`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn lv_trilinear(c: Vec3, layers: u32, fetch: &dyn Fn(IVec3) -> f32) -> f32 {
    let top = (layers.max(2) - 1) as f32;
    let y = c.y.clamp(0.0, top);
    let (x0, z0) = (c.x.floor(), c.z.floor());
    let y0 = y.floor().min(top - 1.0);
    let f = Vec3::new(c.x - x0, y - y0, c.z - z0);
    let b = IVec3::new(x0 as i32, y0 as i32, z0 as i32);
    let mut v = 0.0;
    for (dx, dy, dz) in [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 0, 1), (1, 0, 1), (0, 1, 1), (1, 1, 1)] {
        let wx = if dx == 1 { f.x } else { 1.0 - f.x };
        let wy = if dy == 1 { f.y } else { 1.0 - f.y };
        let wz = if dz == 1 { f.z } else { 1.0 - f.z };
        let weight = wx * wy * wz;
        if weight > 0.0 {
            v += weight * fetch(b + IVec3::new(dx, dy, dz));
        }
    }
    v
}

/// A depth as the R16Float texel holds it: clamped to 0..[`LV_HALF_MOST`], rounded to half.
#[cfg_attr(not(test), allow(dead_code))]
pub fn lv_store(v: f32) -> f32 {
    let v = if v.is_finite() { v.clamp(0.0, LV_HALF_MOST) } else { 0.0 };
    half::f16::from_f32(v).to_f32()
}

/// The reader's near part: the depth over the first `k` of today's eight intervals from `rel`
/// (camera height `cam_y`) along `ld`, exactly as the march takes them, and the handoff
/// distance `a R` where the volume is read.
#[cfg_attr(not(test), allow(dead_code))]
pub fn lv_near(sigma: &dyn Fn(Vec3) -> f32, (rel, cam_y): (Vec3, f32), ld: Vec3, k: u32, u: &CloudVolumeUniform) -> (f32, f32) {
    let reach = light_reach(rel, cam_y, ld, u);
    let (mut s, mut tau) = (0.0f32, 0.0f32);
    for ds in light_intervals(reach, LIGHT_STEPS).into_iter().take(k as usize) {
        tau += sigma(rel + ld * (s + 0.5 * ds)) * ds;
        s += ds;
    }
    (tau, s)
}

/// The top-down sweep's lattice: `nx` x `nz` nodes `across` apart
/// on each of `layers` bent-height layers `up` apart, layer 0's node (0, 0) at `origin` (x, z
/// from the frozen camera at height `cam_y`), each layer's nodes moved along the light's azimuth by
/// the run of the bent light ray `lean` from the camera's column at the base up to the layer's
/// bent height ([`LvSweep::run`]; `lean` zero: world-aligned, no run; the light: light-aligned, the
/// lattice's columns are the sun's rays through the bend near the camera).
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LvSweep {
    pub across: f32,
    pub up: f32,
    pub nx: u32,
    pub nz: u32,
    pub layers: u32,
    pub origin: Vec2,
    pub lean: Vec3,
    pub cam_y: f32,
}

#[cfg_attr(not(test), allow(dead_code))]
impl LvSweep {
    pub fn texels(&self) -> u64 {
        u64::from(self.nx) * u64::from(self.nz) * u64::from(self.layers)
    }
    pub fn index(&self, n: IVec3) -> usize {
        (n.z as usize * self.layers as usize + n.y as usize) * self.nx as usize + n.x as usize
    }
    /// The horizontal run of the straight ray along `lean` from the camera's column at the base
    /// to bent height `h` above it: `s(h) L_xz` with `s(h)` the root of `s L_y + s^2 |L_xz|^2 / 2R
    /// = h` (`L_y` held at least 0.05 up); none for a zero `lean`.
    pub fn run(&self, h: f32, u: &CloudVolumeUniform) -> Vec2 {
        let l = Vec2::new(self.lean.x, self.lean.z);
        let a = l.length_squared() / (2.0 * u.reach.y.max(1.0));
        let b = self.lean.y.max(0.05);
        let h = h.max(0.0);
        let s = if a <= 1e-12 { h / b } else { (-b + (b * b + 4.0 * a * h).sqrt()) / (2.0 * a) };
        l * s
    }
    /// Layer `y`'s (fractional) lattice origin.
    pub fn layer_origin(&self, y: f32, u: &CloudVolumeUniform) -> Vec2 {
        self.origin + self.run(y * self.up, u)
    }
    pub fn node_rel(&self, n: IVec3, u: &CloudVolumeUniform) -> Vec3 {
        let xz = self.layer_origin(n.y as f32, u) + Vec2::new(n.x as f32, n.z as f32) * self.across;
        Vec3::new(xz.x, unbend(xz.x, xz.y, n.y as f32 * self.up, self.cam_y, u), xz.y)
    }
    /// The lattice coordinate of `rel` (the run follows the continuous layer, so a read between
    /// two layers interpolates along the lattice's own columns).
    pub fn coord(&self, rel: Vec3, u: &CloudVolumeUniform) -> Vec3 {
        let y = (bent_y(rel, self.cam_y, u.reach.y) - u.slab.x) / self.up;
        let xz = (Vec2::new(rel.x, rel.z) - self.layer_origin(y, u)) / self.across;
        Vec3::new(xz.x, y, xz.y)
    }
    pub fn covers(&self, c: Vec3) -> bool {
        c.x >= 0.0 && c.x <= (self.nx - 1) as f32 && c.z >= 0.0 && c.z <= (self.nz - 1) as f32 && c.y >= 0.0
    }
}

/// A level's weight at lattice coordinate `c` (`nx` x `nz` nodes): 1 inside, falling linearly to 0
/// over the last `band` cells before the window's edge, 0 outside it and under the base (the
/// reader's handoff point under the base takes the legacy taps).
#[cfg_attr(not(test), allow(dead_code))]
pub fn lv_guard(c: Vec3, nx: u32, nz: u32, band: f32) -> f32 {
    if c.y.is_nan() || c.y < 0.0 {
        return 0.0;
    }
    let edge = c.x.min((nx.max(2) - 1) as f32 - c.x).min(c.z).min((nz.max(2) - 1) as f32 - c.z);
    (edge / band.max(1e-3)).clamp(0.0, 1.0)
}

/// The nesting [design]: `reads` the levels' (weight, depth) inner first, each
/// mixed over everything outside it, `rest` beyond the last (the legacy taps' remaining intervals,
/// evaluated only when some weight is left): `Σ_i w_i d_i Π_{j<i} (1 - w_j) + Π (1 - w_j) rest`.
/// Returns the depth, the first level with any weight (`reads.len()`: the legacy taps alone) and
/// whether that level alone gave it (weight 1).
#[cfg_attr(not(test), allow(dead_code))]
pub fn lv_nest(reads: &[(f32, f32)], rest: &dyn Fn() -> f32) -> (f32, usize, bool) {
    let used = reads.iter().position(|r| r.0 > 0.0).unwrap_or(reads.len());
    let whole = reads.get(used).is_some_and(|r| r.0 >= 1.0);
    let (mut keep, mut v) = (1.0f32, 0.0f32);
    for &(w, d) in reads {
        let w = w.clamp(0.0, 1.0);
        v += keep * w * d;
        keep *= 1.0 - w;
    }
    if keep > 0.0 {
        v += keep * rest();
    }
    (v, used, whole)
}

/// The substeps the sweep takes over one layer for a light `ld_y` up: the slant over the layer
/// over [`LV_SWEEP_STEP`], at least one (the bend ignored).
#[cfg_attr(not(test), allow(dead_code))]
pub fn lv_sweep_substeps(up: f32, ld_y: f32) -> u32 {
    ((up / ld_y.max(0.05) / LV_SWEEP_STEP).ceil() as u32).max(1)
}

/// One node of the sweep: the depth from the node `rel` (its bent height `up` under the next
/// layer's) along `ld`: `sigma` integrated to where the straight ray from the frozen camera
/// crosses the next layer's bent height (substeps of at most [`LV_SWEEP_STEP`]), plus the
/// depth `above` reads there (`None` outside the lattice: zero beyond, the boundary rule under
/// test). Returns the texel's value and the density evaluations it took.
#[cfg_attr(not(test), allow(dead_code))]
pub fn lv_sweep_node(sigma: &dyn Fn(Vec3) -> f32, rel: Vec3, ld: Vec3, up: f32, above: &dyn Fn(Vec3) -> Option<f32>, u: &CloudVolumeUniform) -> (f32, u32) {
    let twice_r = 2.0 * u.reach.y.max(1.0);
    let a = (ld.x * ld.x + ld.z * ld.z) / twice_r;
    let b = ld.y + 2.0 * (rel.x * ld.x + rel.z * ld.z) / twice_r;
    let s = match roots(a, b, -up) {
        Some((_, hi)) if hi > 0.0 => hi,
        _ => return (0.0, 0),
    };
    let n = ((s / LV_SWEEP_STEP).ceil() as u32).max(1);
    let ds = s / n as f32;
    let local: f32 = (0..n).map(|i| sigma(rel + ld * ((i as f32 + 0.5) * ds)) * ds).sum();
    (lv_store(local + above(rel + ld * s).unwrap_or(0.0)), n)
}

// ---- Checkpoint B1: the debug aids, the uniform's block, and the GPU producer ----

/// The levels in force (`NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_VOLUME=off|near|far|both`, default
/// both) [design].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LvLevels {
    Off,
    Near,
    Far,
    Both,
}

impl LvLevels {
    pub fn near(self) -> bool {
        matches!(self, LvLevels::Near | LvLevels::Both)
    }
    pub fn far(self) -> bool {
        matches!(self, LvLevels::Far | LvLevels::Both)
    }
    pub fn word(self) -> &'static str {
        match self {
            LvLevels::Off => "off",
            LvLevels::Near => "near",
            LvLevels::Far => "far",
            LvLevels::Both => "near and far",
        }
    }
}

/// `..._LIGHT_VOLUME`: `off` (also `0`, `none`), `near`, `far`; anything else both.
pub fn debug_levels(v: Option<&str>) -> LvLevels {
    match v.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("off" | "0" | "none") => LvLevels::Off,
        Some("near") => LvLevels::Near,
        Some("far") => LvLevels::Far,
        _ => LvLevels::Both,
    }
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_VOLUME_CELLS=<near>,<far>`: each level's spacing across
/// (units, 50 to 100,000; a missing or bad value keeps its default, 500 and 4,000).
pub fn debug_cells(v: Option<&str>) -> [f32; 2] {
    let mut cells = [LV_ACROSS, LV_FAR_ACROSS];
    if let Some(v) = v {
        for (slot, part) in cells.iter_mut().zip(v.split(',')) {
            if let Ok(x) = part.trim().parse::<f32>() {
                if x.is_finite() && (50.0..=100_000.0).contains(&x) {
                    *slot = x;
                }
            }
        }
    }
    cells
}

/// The near taps the trace marches before it reads the volume, for `asked` of them (the config's):
/// volumetric_clouds_march.wesl `vc_near_count` under LIGHT_VOLUME, at least one (`VC_NEAR_LEAST`), at
/// most all the intervals. What the logs state.
pub fn lv_near_taps(u: &CloudVolumeUniform, asked: u32) -> u32 {
    asked.max(1).min((u.march.y.max(0.0) as u32).clamp(1, 16))
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_TAPS=0..8`: the near taps, `start` when unset or unreadable
/// (volumetric_clouds.rs `START_VALUES`); 8 (all the intervals) is the legacy path
/// whatever the volume says.
pub fn start_taps(v: Option<&str>, start: u32) -> u32 {
    v.and_then(|s| s.trim().parse::<u32>().ok()).filter(|&k| k <= LIGHT_STEPS).unwrap_or(start)
}

/// The frames one sweep of a level takes by default (near, far) [design, B2]: a level's back set
/// is swept top down over this many frames (1 = the whole level every frame, B1's behaviour).
pub const LV_SPREAD: [u32; 2] = [8, 32];
/// The longest spread the aid takes (frames).
pub const LV_SPREAD_MOST: u32 = 256;

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_VOLUME_SPREAD=<near>,<far>`: the frames a sweep takes
/// (1 to [`LV_SPREAD_MOST`]; a missing or bad value keeps its default, [`LV_SPREAD`]).
pub fn debug_spread(v: Option<&str>) -> [u32; 2] {
    let mut spread = LV_SPREAD;
    if let Some(v) = v {
        for (slot, part) in spread.iter_mut().zip(v.split(',')) {
            if let Ok(x) = part.trim().parse::<u32>() {
                if (1..=LV_SPREAD_MOST).contains(&x) {
                    *slot = x;
                }
            }
        }
    }
    spread
}

/// The run's light-volume settings (read once at the start).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LvConfig {
    pub levels: LvLevels,
    pub cells: [f32; 2],
    pub taps: u32,
    /// The frames a sweep takes (near, far).
    pub spread: [u32; 2],
}

/// The settings from start variables read by `get` (by name).
pub fn lv_config_from(get: impl Fn(&str) -> Option<String>) -> LvConfig {
    LvConfig {
        levels: debug_levels(get("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_VOLUME").as_deref()),
        cells: debug_cells(get("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_VOLUME_CELLS").as_deref()),
        taps: start_taps(get("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_TAPS").as_deref(), crate::volumetric_clouds::START_VALUES.taps),
        spread: debug_spread(get("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_VOLUME_SPREAD").as_deref()),
    }
}

static LV_CONFIG: std::sync::LazyLock<LvConfig> = std::sync::LazyLock::new(|| lv_config_from(crate::dev_aids::var));

pub fn lv_config() -> LvConfig {
    *LV_CONFIG
}

/// Whether the light volume is drawn this frame [design]: a level asked for,
/// fewer near taps than the light's intervals (all of them is the legacy path), and a light with
/// some strength (the march takes no light taps otherwise). Off: nothing held, nothing drawn.
pub fn lv_active(u: &CloudVolumeUniform, cfg: &LvConfig) -> bool {
    cfg.levels != LvLevels::Off && cfg.taps < u.march.y.max(1.0) as u32 && u.light_dir.w > 0.0 && u.light_dir.is_finite()
}

/// The field displacement (units) the uniform's placement offsets carry: `repeat x drift`
/// (the test `the_field_moves_rigidly_with_the_drift` holds it to
/// 0.05 units), from the first placement octave's whole cells and fraction.
pub fn lv_drift_units(u: &CloudVolumeUniform) -> DVec2 {
    // Both placement lattices transform the physical wind. Move the light cache with the
    // untransformed wind carried by the vigour octave.
    let cells = f64::from(u.height.x).max(1e-6);
    let axis = |whole: u32, frac: f32, scale: f32| {
        (f64::from(whole as i32) + f64::from(frac)) / (cells * f64::from(scale).max(1e-12))
    };
    DVec2::new(axis(u.cells_v.x, u.vigour.x, u.map.x), axis(u.cells_v.y, u.vigour.y, u.map.y))
}

/// One level's grid for a spacing `across` over the uniform's domain.
pub fn lv_grid(across: f32, u: &CloudVolumeUniform) -> LvGrid {
    LvGrid::new(across, LV_LEVEL_UP, LV_NODES, u.slab.y)
}

/// The uniform's light-volume block for the frame's camera at world `cam` [design]: the windows
/// placed about the camera's field position (`cam` x, z plus [`lv_drift_units`], in f64, snapped to
/// whole nodes), the camera's position in nodes from each window's corner, the spacings, the sizes.
pub fn lv_fill(u: &mut CloudVolumeUniform, cam: Vec3, cfg: &LvConfig) {
    let field = DVec2::new(f64::from(cam.x), f64::from(cam.z)) + lv_drift_units(u);
    let level = |across: f32| {
        let g = lv_grid(across, u);
        let w = lv_window(field, &g);
        (Vec4::new(w.cam.x, w.cam.y, g.across, g.up), g)
    };
    let ((near, gn), (far, gf)) = (level(cfg.cells[0]), level(cfg.cells[1]));
    let on = |b: bool| if b { 1.0 } else { 0.0 };
    u.lv = Vec4::new(cfg.taps as f32, on(cfg.levels.near()), on(cfg.levels.far()), LV_GUARD);
    u.lv_near = near;
    u.lv_far = far;
    u.lv_size = Vec4::new(gn.nodes as f32, gf.nodes as f32, gn.layers as f32, gf.layers as f32);
    u.lv_cam = cam.extend(0.0);
}

/// Whether the light-volume block changed (volumetric_clouds.rs `needs_write`).
pub fn lv_changed(old: &CloudVolumeUniform, new: &CloudVolumeUniform) -> bool {
    old.lv != new.lv || old.lv_near != new.lv_near || old.lv_far != new.lv_far || old.lv_size != new.lv_size || old.lv_cam != new.lv_cam
}

/// The density evaluations one level's sweep takes a frame for a light `ld_y` up (the bend
/// ignored): every node under the top times the sub-steps over one layer (at most 64, the
/// producer's bound).
pub fn lv_sweep_evaluations(g: &LvGrid, ld_y: f32) -> u64 {
    u64::from(g.nodes) * u64::from(g.nodes) * u64::from(g.layers - 1) * u64::from(lv_sweep_substeps(g.up, ld_y).min(64))
}

// ---- Checkpoint B2: the amortised refresh (a front set read, a back set swept) ----
//
// [design, B2] Each level has two sets of layers (two textures):
// the FRONT, which the trace reads, and the BACK, swept top down over `spread` frames from ONE
// snapshot of the producer's uniform taken when its sweep starts (the camera, the window, the
// drift, the sun and the weather's inputs of that frame). When the back's last layer is written
// the two swap and the trace is given the new front's own window with it. In field coordinates a
// level's contents go stale only through the sun's direction, the weather's inputs during a fade,
// the window sliding and jumps; a jump restarts the back sweep, and where the front no longer
// fits at all (the first frames, a teleport, a weather jump, the sun to the moon) the level is not
// read (the next level out, or the legacy taps, take its place).

/// The light moved more than this in one frame (degrees; the sun moves about 0.0025 a frame at
/// the default time scale): the back sweeps restart from a new
/// snapshot (the fronts stay while they are within [`LV_DROP_DEG`]).
pub const LV_RESTART_DEG: f32 = 0.5;
/// A front made for a light more than this from today's (degrees) is no longer read (a clock jump,
/// the sun handing over to the moon).
pub const LV_DROP_DEG: f32 = 3.0;
/// A weather or tuning input of the density that changed by more than this share (of its value
/// or of one, whichever is larger) in one frame: a jump (a forced weather, a tuning file); a fade
/// moves them by far less a frame (a one-second fade about 0.017 of a 0..1 input).
pub const LV_JUMP_SHARE: f32 = 0.05;

/// One bake (a set's frame of reference): which of the level's two textures, its window (lowest
/// node, the bake camera's position in nodes from it), its lattice, the light it was made for,
/// the far set the near level's sweep read beyond its window (none: no nesting), and the frozen
/// uniform every layer of its sweep reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LvBake {
    pub set: usize,
    pub lo: [i64; 2],
    pub cam: Vec2,
    pub grid: LvGrid,
    pub light: Vec3,
    pub outer: Option<usize>,
    pub snap: CloudVolumeUniform,
}

/// A back sweep under way: its bake, the frames it has run and the layers written (from the top).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LvRun {
    pub bake: LvBake,
    pub frame: u32,
    pub done: u32,
}

/// One level's sets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LvLevelState {
    pub front: Option<LvBake>,
    pub back: Option<LvRun>,
}

/// One level's work this frame: the set written, its layers, the layers written from the top
/// (`from..to` counted from the top layer down), the snapshot to upload first (the frame its sweep
/// starts) and the far set its passes read beyond the window (the near level's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LvLevelPlan {
    pub set: usize,
    pub layers: u32,
    pub from: u32,
    pub to: u32,
    pub upload: Option<CloudVolumeUniform>,
    pub outer: Option<usize>,
}

impl LvLevelPlan {
    /// The layer indices written this frame, top first (the top layer is cleared to zero).
    pub fn written(&self) -> impl Iterator<Item = u32> + '_ {
        (self.from..self.to.min(self.layers)).map(|c| self.layers - 1 - c)
    }
}

/// A frame's plan: per level (near, far) its work, the sets the trace reads (none: the level is
/// not read), the trace's uniform (its light-volume block from the fronts) and, when fronts were
/// dropped, why (for the log; never per frame: a dropped front is rebuilt before it can drop again).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LvPlan {
    pub levels: [Option<LvLevelPlan>; 2],
    pub fronts: [Option<usize>; 2],
    pub trace: CloudVolumeUniform,
    pub event: Option<&'static str>,
}

/// The angle between two directions (degrees; a zero one is 90 from anything).
fn lv_angle(a: Vec3, b: Vec3) -> f32 {
    a.normalize_or_zero().dot(b.normalize_or_zero()).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Whether the density's weather or tuning inputs jumped between two frames' uniforms (any part
/// moved by more than [`LV_JUMP_SHARE`] of its value or of one): the drift's offsets, the light,
/// the colours and the light volume's block are not among them.
pub fn lv_weather_jumped(old: &CloudVolumeUniform, new: &CloudVolumeUniform) -> bool {
    let pick = |u: &CloudVolumeUniform| {
        [
            Vec4::new(u.slab.x, u.slab.y, u.march.z, u.profile.y),
            Vec4::new(u.nscale.z, u.nscale.w, u.profile.x, 0.0),
            u.field,
            u.scale,
            u.mix,
            u.tower,
            u.height,
            u.kind,
            u.knot_s0,
            u.knot_s1,
            u.knot_c0,
            u.knot_c1,
            u.knot_t0,
            u.knot_t1,
            u.cal_c0,
            u.cal_c1,
            u.cal_k0,
            u.cal_k1,
            u.nlobe.truncate().extend(0.0),
        ]
    };
    pick(old).iter().zip(pick(new).iter()).any(|(&a, &b)| {
        let d = (a - b).abs();
        !d.is_finite() || d.cmpgt(Vec4::splat(LV_JUMP_SHARE) * a.abs().max(b.abs()).max(Vec4::ONE)).any()
    })
}

/// A level's vector of the uniform's block for a camera at field position `field` reading the set
/// `b`: the camera's position in nodes from `b`'s own lowest node, `b`'s spacings.
fn lv_level_vec(field: DVec2, b: &LvBake) -> Vec4 {
    let pos = lv_node_pos(field, b.lo, &b.grid);
    Vec4::new(pos.x, pos.y, b.grid.across, b.grid.up)
}

/// Whether a live edit of the tune file moved an input the light field reads that
/// [`lv_weather_jumped`] may not see (an edit below [`LV_JUMP_SHARE`]): `DENSITY` (`march.z`), `SHAPE` and
/// `EROSION` (`nscale.zw`), none of which the weather moves. The sweeps under way restart from the new
/// values (their snapshots hold the old ones); the fronts stay until their replacements are whole.
pub fn lv_tune_changed(old: &CloudVolumeUniform, new: &CloudVolumeUniform) -> bool {
    old.march.z != new.march.z || old.nscale.z != new.nscale.z || old.nscale.w != new.nscale.w
}

/// The light's filter cell a level's producer and the trace's taps read (march
/// `vc_light_cell`, the uniform's `lcell.x`; stage 2i M1): the level's own spacing, so the cells
/// aid moves the filter with the grid.
pub fn lv_light_cell(u: &mut CloudVolumeUniform, across: f32) {
    u.lcell.x = across;
}

/// The scheduler of the two levels' sets [design, B2], pure so the swap bookkeeping is tested on
/// the CPU: [`LvSchedule::plan`] once a frame, [`LvSchedule::encoded`] once its work went on the
/// GPU (a plan that never did resets everything: no front may claim layers that were not written).
#[derive(Clone, Debug)]
pub struct LvSchedule {
    pub cfg: LvConfig,
    pub levels: [LvLevelState; 2],
    last: Option<CloudVolumeUniform>,
    pending: bool,
    /// The fronts as they stood when the last plan's work was encoded: every layer of each was
    /// written by an earlier frame's commands (the shared reader's candidates, stage 2i M2).
    settled: [Option<LvBake>; 2],
    /// The fronts that survived this frame's drop checks, before this frame's promotions.
    kept: [Option<LvBake>; 2],
}

/// Whether two bakes are the same one (set, window, lattice): what the shared reader compares
/// (the frozen uniform is left out, so a value that is not a number cannot make a bake unequal
/// to itself).
fn lv_same_bake(a: &LvBake, b: &LvBake) -> bool {
    a.set == b.set && a.lo == b.lo && a.grid == b.grid && a.cam == b.cam
}

impl LvSchedule {
    pub fn new(cfg: LvConfig) -> LvSchedule {
        LvSchedule { cfg, levels: [LvLevelState::default(); 2], last: None, pending: false, settled: [None; 2], kept: [None; 2] }
    }

    /// The last plan's work was encoded: its fronts are whole from the next frame on.
    pub fn encoded(&mut self) {
        self.pending = false;
        self.settled = [self.levels[0].front, self.levels[1].front];
    }

    /// The sets the trace reads now (near, far).
    pub fn fronts(&self) -> [Option<usize>; 2] {
        [self.levels[0].front.map(|b| b.set), self.levels[1].front.map(|b| b.set)]
    }

    /// The bakes the shared reader may read during this frame (stage 2i M2; `LightVolumeReader`,
    /// light_volume_reader.wesl): per level the front that was whole before this frame's work (all
    /// its layers encoded by an earlier frame) and still stands after this frame's drop checks.
    /// No texel of it is written during this frame: the back sweep writes the other set; a set
    /// promoted this frame is not read until the next one (its last layers are drawn this frame,
    /// by the trace pass, which views drawn earlier, such as the water reflection, would not see);
    /// a dropped front, a reset, or a plan that was not encoded gives none. A front promoted this
    /// frame takes over one frame later than in the trace.
    pub fn reader_bakes(&self) -> [Option<LvBake>; 2] {
        [0, 1].map(|i| self.settled[i].filter(|s| self.kept[i].is_some_and(|k| lv_same_bake(s, &k))))
    }

    /// The frame's plan for the uniform `u` (as the march uploads it, its block zero) and the
    /// camera at world `cam` (the trace's origin).
    pub fn plan(&mut self, u: &CloudVolumeUniform, cam: Vec3) -> LvPlan {
        let cfg = self.cfg;
        let field = DVec2::new(f64::from(cam.x), f64::from(cam.z)) + lv_drift_units(u);
        let light = u.light_dir.truncate();
        let mut event = None;
        let had_front = |s: &LvSchedule| s.levels.iter().any(|l| l.front.is_some());
        // A plan that never reached the GPU: nothing written can be trusted.
        if std::mem::replace(&mut self.pending, true) {
            if had_front(self) {
                event = Some("a frame's sweep was not drawn");
            }
            self.levels = [LvLevelState::default(); 2];
            self.settled = [None; 2];
        }
        if let Some(last) = self.last {
            if lv_weather_jumped(&last, u) {
                if had_front(self) {
                    event = Some("the weather's inputs jumped");
                }
                self.levels = [LvLevelState::default(); 2];
            } else if lv_angle(last.light_dir.truncate(), light) > LV_RESTART_DEG || (crate::dev_aids::ON && lv_tune_changed(&last, u)) {
                for level in &mut self.levels {
                    level.back = None;
                }
            }
        }
        for (i, level) in self.levels.iter_mut().enumerate() {
            let g = lv_grid(cfg.cells[i], u);
            let moved = |b: &LvBake| (lv_node_pos(field, b.lo, &b.grid) - b.cam).abs().max_element();
            let fits = |b: &LvBake| b.grid.layers == g.layers && b.grid.across == g.across && moved(b) <= LV_GUARD;
            if let Some(b) = level.front {
                let turned = lv_angle(b.light, light) > LV_DROP_DEG;
                if turned || !fits(&b) {
                    event = Some(if turned { "the light jumped" } else { "the window jumped" });
                    level.front = None;
                }
            }
            if level.back.is_some_and(|r| !fits(&r.bake)) {
                level.back = None;
            }
        }
        // The fronts the drop checks left, before any promotion (the shared reader's rule).
        self.kept = [self.levels[0].front, self.levels[1].front];
        // A near sweep reads one far set beyond its window for all its layers: a far front gone
        // (dropped) under it restarts it.
        let far_front = self.levels[1].front.map(|b| b.set);
        if self.levels[0].back.is_some_and(|r| r.bake.outer.is_some() && r.bake.outer != far_front) {
            self.levels[0].back = None;
        }
        let far = if cfg.levels.far() { self.step(1, u, (cam, field)) } else { None };
        // The far level's finished set is promoted on a frame a near sweep begins (or with no near
        // level), so a near sweep never sees its far set change or be swept over.
        if !cfg.levels.near() || self.levels[0].back.is_none() {
            let far_level = &mut self.levels[1];
            if let Some(run) = far_level.back.filter(|r| r.done >= r.bake.grid.layers) {
                far_level.front = Some(run.bake);
                far_level.back = None;
            }
        }
        let near = if cfg.levels.near() { self.step(0, u, (cam, field)) } else { None };
        self.last = Some(*u);
        LvPlan { levels: [near, far], fronts: self.fronts(), trace: self.trace_block(u, cam, field), event }
    }

    /// One level's work this frame: a sweep started from this frame's snapshot when none is under
    /// way (into the set the trace does not read), then its next layers; the near level's set
    /// promoted the frame its last layer is written (the far one waits, [`LvSchedule::plan`]).
    fn step(&mut self, i: usize, u: &CloudVolumeUniform, (cam, field): (Vec3, DVec2)) -> Option<LvLevelPlan> {
        let cfg = self.cfg;
        let outer = if i == 0 && cfg.levels.far() { self.levels[1].front } else { None };
        let level = &mut self.levels[i];
        let mut upload = None;
        if level.back.is_none() {
            let set = level.front.map_or(0, |b| 1 - b.set);
            let bake = lv_start(i, set, u, (cam, field), outer.as_ref(), &cfg);
            upload = Some(bake.snap);
            level.back = Some(LvRun { bake, frame: 0, done: 0 });
        }
        let run = level.back.as_mut()?;
        let layers = run.bake.grid.layers;
        if run.done >= layers {
            return None;
        }
        let spread = u64::from(cfg.spread[i].max(1));
        let to = ((u64::from(layers) * u64::from(run.frame + 1)) / spread).min(u64::from(layers)) as u32;
        let plan = LvLevelPlan { set: run.bake.set, layers, from: run.done, to, upload, outer: run.bake.outer };
        run.frame += 1;
        run.done = to;
        if to >= layers && (i == 0 || !cfg.levels.near()) {
            level.front = Some(run.bake);
            level.back = None;
        }
        Some(plan)
    }

    /// The trace's uniform: `u` with its light-volume block from the fronts (a level with no front
    /// is off), each level's camera position measured from its front's own lowest node.
    fn trace_block(&self, u: &CloudVolumeUniform, cam: Vec3, field: DVec2) -> CloudVolumeUniform {
        let mut t = *u;
        let (gn, gf) = (lv_grid(self.cfg.cells[0], u), lv_grid(self.cfg.cells[1], u));
        let [near, far] = [self.levels[0].front, self.levels[1].front];
        let on = |b: &Option<LvBake>| if b.is_some() { 1.0 } else { 0.0 };
        t.lv = Vec4::new(self.cfg.taps as f32, on(&near), on(&far), LV_GUARD);
        t.lv_near = near.map_or(Vec4::ZERO, |b| lv_level_vec(field, &b));
        t.lv_far = far.map_or(Vec4::ZERO, |b| lv_level_vec(field, &b));
        t.lv_size = Vec4::new(gn.nodes as f32, gf.nodes as f32, near.map_or(gn.layers, |b| b.grid.layers) as f32, far.map_or(gf.layers, |b| b.grid.layers) as f32);
        t.lv_cam = cam.extend(0.0);
        // The trace's light taps filter the field to the near level's cell (stage 2i M1).
        lv_light_cell(&mut t, gn.across);
        t
    }
}

/// A bake of level `i` (0 near, 1 far) into `set` from this frame: the window about the camera's
/// field position, the producer's frozen uniform (`lv_fill`'s block for this camera; for the near
/// level the far block is the far set `outer` it will read, as this camera sees it, and its flag
/// says whether there is one).
fn lv_start(i: usize, set: usize, u: &CloudVolumeUniform, (cam, field): (Vec3, DVec2), outer: Option<&LvBake>, cfg: &LvConfig) -> LvBake {
    let grid = lv_grid(cfg.cells[i], u);
    let w = lv_window(field, &grid);
    let mut snap = *u;
    lv_fill(&mut snap, cam, cfg);
    lv_light_cell(&mut snap, grid.across);
    if i == 0 {
        snap.lv.z = if outer.is_some() { 1.0 } else { 0.0 };
        if let Some(o) = outer {
            snap.lv_far = lv_level_vec(field, o);
        }
    }
    LvBake { set, lo: w.lo, cam: w.cam, grid, light: u.light_dir.truncate(), outer: outer.map(|o| o.set), snap }
}

// ---- Stage 2i M2: the shared reader of the light volume (light_volume_reader.wesl) ----
//
// [design] One reader of the completed fronts for every user outside
// the trace (the ground cloud shadows and the light in the air now, reflections later): the render
// world's `gpu::LightVolumeReader` (the fronts' views and windows, made by
// `gpu::publish_light_volume_reader` each frame the light volume ran), the shader struct
// [`LightVolumeWindow`] a user packs from it, and the WESL module `nvforever::light_volume_reader`
// whose header is the contract. The functions below are its CPU mirror.

/// The real seconds over which a level that becomes readable fades in for a reader that mixes it
/// over another source ([`lvr_presence`]) [engineering choice: a front's arrival changes the
/// picture from the fallback's pattern to the volume's, which must not show as a pop. Seconds, not
/// frames: the first fronts arrive while cells are still loading, at a few frames a second, when a
/// fade counted in frames took many seconds (judged by eye in stills of the development build
/// 0d2c21e79, about 9 s after the near level and 2.6 s after the far one arrived: the fallback almost
/// whole)].
pub const LVR_ARRIVAL_SECS: f32 = 1.0;

/// light_volume_reader.wesl `LightVolumeWindow`, field for field.
#[derive(Clone, Copy, Debug, Default, PartialEq, bevy::render::render_resource::ShaderType)]
pub struct LightVolumeWindow {
    /// Per level: xy the camera's position in nodes from the front's lowest node, z the spacing
    /// across, w the spacing up (units).
    pub near: Vec4,
    pub far: Vec4,
    /// x, y: nodes across (near, far); z, w: layers.
    pub size: Vec4,
    /// xyz: the camera (world); w: the bend's radius.
    pub cam: Vec4,
    /// x: the slab floor (Bevy y), y: the guard band (cells), z, w: the levels' presence (0..1).
    pub floor: Vec4,
}

/// One level as the reader sees it this frame: the set to bind, its window vector (the trace's
/// `lv_near`/`lv_far` for that front: the camera's position in nodes from its corner, across, up),
/// its nodes across and layers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LvReaderLevel {
    pub set: usize,
    pub window: Vec4,
    pub nodes: u32,
    pub layers: u32,
}

/// The shader struct for the reader's `levels` (absent: zero window, two nodes and layers, presence
/// 0, never sampled), the camera `cam`, the slab floor `floor`, the bend's radius `bend`, the guard
/// band `guard` (cells) and each level's `presence`.
pub fn lvr_window(levels: [Option<LvReaderLevel>; 2], (cam, floor, bend, guard): (Vec3, f32, f32, f32), presence: [f32; 2]) -> LightVolumeWindow {
    let window = |l: Option<LvReaderLevel>| l.map_or(Vec4::ZERO, |l| l.window);
    let nodes = |l: Option<LvReaderLevel>| l.map_or(2.0, |l| l.nodes as f32);
    let layers = |l: Option<LvReaderLevel>| l.map_or(2.0, |l| l.layers as f32);
    let p = |i: usize| if levels[i].is_some() && presence[i].is_finite() { presence[i].clamp(0.0, 1.0) } else { 0.0 };
    LightVolumeWindow {
        near: window(levels[0]),
        far: window(levels[1]),
        size: Vec4::new(nodes(levels[0]), nodes(levels[1]), layers(levels[0]), layers(levels[1])),
        cam: cam.extend(bend),
        floor: Vec4::new(floor, guard, p(0), p(1)),
    }
}

/// The reader's levels for the camera at field position `field` (world x, z plus
/// [`lv_drift_units`] of this frame's uniform): [`LvSchedule::reader_bakes`] with each one's window
/// measured as the trace measures its front's ([`lv_level_vec`]).
pub fn lv_reader_levels(schedule: &LvSchedule, field: DVec2) -> [Option<LvReaderLevel>; 2] {
    schedule.reader_bakes().map(|b| b.map(|b| LvReaderLevel { set: b.set, window: lv_level_vec(field, &b), nodes: b.grid.nodes, layers: b.grid.layers }))
}

/// A level's presence for a reader that fades it in: 0 while it is not readable (`readable` none),
/// then a smoothstep from 0 to 1 over `arrival` real seconds of its being readable without a break.
pub fn lvr_presence(readable: Option<f32>, arrival: f32) -> f32 {
    let Some(secs) = readable.filter(|s| s.is_finite()) else { return 0.0 };
    let t = (secs / arrival.max(1e-3)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The hand-over weight of a level at `edge` cells inside its window (negative outside) with the
/// guard band `guard` and presence `presence`: 0 outside, rising linearly to 1 a guard band inside,
/// times the presence (light_volume_reader.wesl `clamp(r.x / guard, 0.0, 1.0) * presence`; the
/// trace's [`lv_guard`] for a presence of 1).
#[cfg_attr(not(test), allow(dead_code))] // the CPU mirror the tests hold the shader to
pub fn lvr_weight(edge: f32, guard: f32, presence: f32) -> f32 {
    if edge.is_nan() || edge < 0.0 {
        return 0.0;
    }
    (edge / guard.max(1e-3)).clamp(0.0, 1.0) * presence.clamp(0.0, 1.0)
}

/// light_volume_reader.wesl `lvr_rise`: the distance along a ray whose bent height above the floor
/// is `bh < 0` at its start and `bh + b s + a s^2` along it to the floor; -1 when it never gets
/// there.
#[cfg_attr(not(test), allow(dead_code))] // the CPU mirror the tests hold the shader to
pub fn lvr_rise(a: f32, b: f32, bh: f32) -> f32 {
    if a <= 1e-12 {
        if b <= 1e-6 {
            return -1.0;
        }
        return -bh / b;
    }
    let root = (b * b - 4.0 * a * bh).sqrt();
    if b >= 0.0 {
        -2.0 * bh / (b + root)
    } else {
        (root - b) / (2.0 * a)
    }
}

/// light_volume_reader.wesl `lvr_level`: x how far inside the window the point at node coordinate
/// `c` and bent height `bh` is (cells, -1 outside or under the floor), y the depth there from
/// `fetch` (node x, layer, node z), bilinear across and linear in height.
#[cfg_attr(not(test), allow(dead_code))] // the CPU mirror the tests hold the shader to
pub fn lvr_level(fetch: &dyn Fn(IVec3) -> f32, c: Vec2, bh: f32, up: f32, size: Vec2) -> Vec2 {
    let nodes = size.x.max(2.0);
    let layers = size.y.max(2.0);
    let edge = c.x.min(nodes - 1.0 - c.x).min(c.y.min(nodes - 1.0 - c.y));
    if bh < 0.0 || edge < 0.0 || edge.is_nan() {
        return Vec2::new(-1.0, 0.0);
    }
    let y = (bh / up.max(1.0)).min(layers - 1.0);
    Vec2::new(edge, lv_trilinear(Vec3::new(c.x, y, c.y), layers as u32, fetch))
}

/// light_volume_reader.wesl `lvr_optical_depth`, line for line: (the depth over the covered share,
/// the covered share) at world `p` for the light from `l`, the levels' nodes from `near` and `far`.
#[cfg_attr(not(test), allow(dead_code))] // the CPU mirror the tests hold the shader to
pub fn lvr_optical_depth(w: &LightVolumeWindow, near: &dyn Fn(IVec3) -> f32, far: &dyn Fn(IVec3) -> f32, p: Vec3, l: Vec3) -> Vec2 {
    let (pn, pf) = (w.floor.z, w.floor.w);
    if pn.max(pf) <= 0.0 {
        return Vec2::ZERO;
    }
    let cam = w.cam.truncate();
    let twice_r = 2.0 * w.cam.w.max(1.0);
    let mut rel = p - cam;
    let mut bh = cam.y + rel.y + (rel.x * rel.x + rel.z * rel.z) / twice_r - w.floor.x;
    if bh < 0.0 {
        let a = (l.x * l.x + l.z * l.z) / twice_r;
        let b = l.y + 2.0 * (rel.x * l.x + rel.z * l.z) / twice_r;
        let t = lvr_rise(a, b, bh);
        if t < 0.0 {
            return Vec2::ZERO;
        }
        rel += l * t;
        bh = 0.0;
    }
    let guard = w.floor.y.max(1e-3);
    let (mut optical, mut keep) = (0.0f32, 1.0f32);
    let xz = Vec2::new(rel.x, rel.z);
    if pn > 0.0 {
        let r = lvr_level(near, w.near.truncate().truncate() + xz / w.near.z.max(1.0), bh, w.near.w, Vec2::new(w.size.x, w.size.z));
        let wn = (r.x / guard).clamp(0.0, 1.0) * pn;
        optical += wn * r.y;
        keep = 1.0 - wn;
    }
    if pf > 0.0 && keep > 0.0 {
        let r = lvr_level(far, w.far.truncate().truncate() + xz / w.far.z.max(1.0), bh, w.far.w, Vec2::new(w.size.y, w.size.w));
        let wf = (r.x / guard).clamp(0.0, 1.0) * pf;
        optical += keep * wf * r.y;
        keep *= 1.0 - wf;
    }
    let covered = 1.0 - keep;
    if covered <= 0.0 {
        return Vec2::ZERO;
    }
    Vec2::new(optical / covered, covered)
}

/// The march module's WESL conditions resolved for one variant of the trace (`light_volume`: the
/// shader def LIGHT_VOLUME; no CLOUD_VIEWS), for the plain-WGSL test assemblers
/// ([`lv_resolve_variant`]).
#[cfg(test)]
pub fn lv_resolve(src: &str, light_volume: bool) -> String {
    lv_resolve_variant(src, light_volume, false)
}

/// [`lv_resolve`] with the shader def CLOUD_VIEWS too (`views`): a line `@if(LIGHT_VOLUME) ...` or
/// `@if(!LIGHT_VOLUME) ...` (CLOUD_VIEWS the same) is kept without its condition when the condition
/// holds and dropped otherwise; a dropped `fn` takes every line to its closing `}` at the line's
/// start with it. Any other condition panics (a condition this does not resolve).
#[cfg(test)]
pub fn lv_resolve_variant(src: &str, light_volume: bool, views: bool) -> String {
    let conditions = [("@if(LIGHT_VOLUME) ", light_volume), ("@if(!LIGHT_VOLUME) ", !light_volume), ("@if(CLOUD_VIEWS) ", views), ("@if(!CLOUD_VIEWS) ", !views)];
    let mut out = String::new();
    let mut skipping = false;
    for line in src.lines() {
        if skipping {
            skipping = line != "}";
            continue;
        }
        let (rest, holds) = conditions.iter().find_map(|(c, holds)| Some((line.strip_prefix(c)?, *holds))).unwrap_or_else(|| {
            assert!(!line.contains("@if"), "a WESL condition this does not resolve: {line}");
            (line, true)
        });
        if holds {
            out.push_str(rest);
            out.push('\n');
        } else if rest.starts_with("fn ") {
            skipping = true;
        }
    }
    out
}

/// The GPU side (render world) [design,
/// B2]. Write route (a): each level is a 2D array texture of R16Float layers (128 x 128, 41
/// layers by default), `RENDER_ATTACHMENT | TEXTURE_BINDING`, two of them a level (the front set
/// the trace reads, the back set being swept, [`LvSchedule`]). A slice pass attaches a one-layer
/// view of layer `j` and samples a one-layer array view of layer `j + 1`: two subresources
/// (wgpu-core 30.0.1 tracks a texture's use per mip and per layer), so no copy and no 3D texture
/// (a 3D texture is one subresource per mip and cannot be attached and sampled in one pass).
/// R16Float is filterable and renderable without a device feature (wgpu-types 30.0.1
/// `guaranteed_format_features`). The reader samples two layers bilinearly and interpolates the
/// height itself. Each level's sweep reads its own frozen snapshot uniform, written once when the
/// sweep starts.
pub(crate) mod gpu {
    use super::{
        lv_active, lv_config, lv_drift_units, lv_grid, lv_near_taps, lv_reader_levels, lv_sweep_evaluations, lvr_presence, lvr_window, LightVolumeWindow, LvConfig, LvGrid, LvLevels, LvPlan, LvReaderLevel, LvSchedule, LV_FAR_FOOTPRINT,
        LV_FOOTPRINT, LV_GUARD, LV_NODES,
    };
    use bevy::math::DVec2;
    use crate::medium::MediumTraceUniform;
    use crate::volumetric_clouds::{variant_suffix, view_variant, CloudVolumeUniform};
    use bevy::core_pipeline::FullscreenShader;
    use bevy::prelude::*;
    use bevy::render::diagnostic::{DiagnosticsRecorder, RecordDiagnostics};
    use bevy::render::render_resource::binding_types::{sampler, texture_2d_array, texture_3d, uniform_buffer};
    use bevy::render::render_resource::{
        BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferId, CachedRenderPipelineId, ColorTargetState, ColorWrites, DynamicUniformBuffer, Extent3d, FragmentState,
        LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerId, ShaderStages, ShaderType,
        StoreOp, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor, TextureViewDimension, TextureViewId, UniformBuffer,
    };
    use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue};
    use bevy::shader::Shader;

    /// The levels' texel format.
    pub const LV_FORMAT: TextureFormat = TextureFormat::R16Float;
    /// The producer's shader.
    pub const LV_SHADER: &str = "embedded://nvforever/volumetric_clouds_lightvol.wesl";
    /// The trace's shader def for its light-volume variant.
    pub const LV_DEF: &str = "LIGHT_VOLUME";

    /// volumetric_clouds_lightvol.wesl `LvSlice`: the same field.
    #[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
    pub struct LvSliceUniform {
        /// x: the level (0 near, 1 far), y: the layer written, z: the footprint (units), w: 1 to
        /// take the far level beyond the window (when the sweep's snapshot has a far set, `lv.z`).
        pub slot: Vec4,
    }

    /// One set of a level: its texture and views: one attachment view and one sampled one-layer
    /// array view per layer, and the whole array for the readers.
    struct LvLevelGpu {
        layers: u32,
        write: Vec<TextureView>,
        read: Vec<TextureView>,
        whole: TextureView,
        _texture: Texture,
    }

    impl LvLevelGpu {
        fn new(device: &RenderDevice, label: &'static str, nodes: u32, layers: u32) -> Self {
            let texture = device.create_texture(&TextureDescriptor {
                label: Some(label),
                size: Extent3d { width: nodes, height: nodes, depth_or_array_layers: layers },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: LV_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let layer = |j: u32, dimension: TextureViewDimension| texture.create_view(&TextureViewDescriptor { dimension: Some(dimension), base_array_layer: j, array_layer_count: Some(1), ..default() });
            let write = (0..layers).map(|j| layer(j, TextureViewDimension::D2)).collect();
            let read = (0..layers).map(|j| layer(j, TextureViewDimension::D2Array)).collect();
            let whole = texture.create_view(&TextureViewDescriptor { dimension: Some(TextureViewDimension::D2Array), ..default() });
            LvLevelGpu { layers, write, read, whole, _texture: texture }
        }
    }

    /// What a state was made for: a change makes it again.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct LvKey {
        levels: LvLevels,
        layers: [u32; 2],
        entry: &'static str,
    }

    /// The bind groups' key: the trace's and the march's uniforms, the slices' buffer, the two
    /// snapshot buffers, the noise views and their sampler (the levels' views live as long as the
    /// state).
    type LvGroupKey = (BufferId, BufferId, BufferId, [BufferId; 2], TextureViewId, SamplerId, TextureViewId);

    /// The slice groups per level, per set written, per far set read beyond the window (the near
    /// level with the far level on: two; otherwise one), per layer under the top; the trace's
    /// groups per near set, per far set.
    struct LvGroups {
        key: LvGroupKey,
        slices: [[Vec<Vec<BindGroup>>; 2]; 2],
        trace: [[BindGroup; 2]; 2],
    }

    /// The light volume's render-world state while it is on (none while off: nothing held).
    struct LvState {
        key: LvKey,
        producer_layout: BindGroupLayoutDescriptor,
        trace_layout: BindGroupLayoutDescriptor,
        /// Whether this frame's uniform takes the trace's view variant.
        views: bool,
        slices: DynamicUniformBuffer<LvSliceUniform>,
        /// Per level (near, far), per layer under the top: the slice's dynamic offset.
        offsets: [Vec<u32>; 2],
        /// Per level, its two sets.
        levels: [Option<[LvLevelGpu; 2]>; 2],
        /// Per level, the frozen uniform its sweep reads (written the frame a sweep starts).
        snaps: [UniformBuffer<CloudVolumeUniform>; 2],
        schedule: LvSchedule,
        /// This frame's plan, from prepare to the encode.
        plan: Option<LvPlan>,
        groups: Option<LvGroups>,
        /// Where this frame's plan measured the shared reader's windows from (stage 2i M2).
        reader_at: Option<LvReaderAt>,
        /// The near taps last named in the log ([`lv_near_taps`]).
        told_taps: u32,
    }

    /// The pipelines asked for so far in this run: kept when the state goes, for the pipeline cache
    /// keeps every pipeline it is given and gives a second request of the same one a new entry.
    #[derive(Default)]
    pub(crate) struct LvPipelines {
        producer: Option<CachedRenderPipelineId>,
        /// The trace per entry, without a view and its view variant (`view_variant`), each queued the
        /// first time it is the one to draw with.
        trace: Vec<(&'static str, bool, CachedRenderPipelineId)>,
    }

    impl LvPipelines {
        /// The producer's pipeline, asked for (`queue`) once in a run.
        pub(crate) fn want_producer(&mut self, queue: impl FnOnce() -> CachedRenderPipelineId) {
            self.producer.get_or_insert_with(queue);
        }

        /// The trace's pipeline of `entry` and variant, asked for (`queue`) once in a run.
        pub(crate) fn want_trace(&mut self, entry: &'static str, views: bool, queue: impl FnOnce() -> CachedRenderPipelineId) {
            if self.trace(entry, views).is_none() {
                self.trace.push((entry, views, queue()));
            }
        }

        fn trace(&self, entry: &'static str, views: bool) -> Option<CachedRenderPipelineId> {
            self.trace.iter().find(|(e, v, _)| (*e, *v) == (entry, views)).map(|t| t.2)
        }
    }

    /// The frame's camera (world), its field position, the slab floor and the bend's radius, as
    /// the plan saw them: what [`publish_light_volume_reader`] measures the windows from.
    #[derive(Clone, Copy, Debug)]
    struct LvReaderAt {
        cam: Vec3,
        field: DVec2,
        floor: f32,
        bend: f32,
    }

    /// The state while on, whether [`lv_prepare`] made this frame's plan (the shared reader takes
    /// nothing from a state the medium did not prepare this frame), and the pipelines, which
    /// outlive the state.
    #[derive(Resource, Default)]
    pub(crate) struct LvGpu(Option<LvState>, bool, pub(crate) LvPipelines);

    impl LvGpu {
        /// Whether nothing is held.
        #[cfg(test)]
        pub(crate) fn is_empty(&self) -> bool {
            self.0.is_none()
        }

        /// Whether the trace's group of this frame's [`lv_encode`] holds a completed set (near, far).
        pub(crate) fn fronts(&self) -> [bool; 2] {
            self.0.as_ref().map_or([false; 2], |s| s.schedule.fronts().map(|f| f.is_some()))
        }

        /// Drops everything but the pipelines (the volume off, no light, the row off, nothing traced).
        pub(crate) fn clear(&mut self) {
            self.1 = false;
            if self.0.take().is_some() {
                info!("volumetric clouds: light volume off (textures released)");
            }
        }
    }

    /// One level of the shared reader: the front set's whole array (R16Float, filterable) and its
    /// window.
    #[derive(Clone)]
    pub(crate) struct LightVolumeLevelView {
        pub view: TextureView,
        pub level: LvReaderLevel,
    }

    /// The light volume's shared reader (stage 2i M2; the contract is light_volume_reader.wesl's
    /// header): per level the completed front a reader outside the trace may sample during this
    /// frame ([`super::LvSchedule::reader_bakes`]: no texel of it is written this frame, whatever
    /// order the views draw in), its window measured from this frame's camera and drift, and the
    /// frames it has been readable without a break (a swap of fronts is no break; a drop is). Empty
    /// while the light volume is off, has no completed front, or made no plan for more than
    /// [`LVR_KEEP_FRAMES`] frames. Made in
    /// `RenderSystems::PrepareResourcesFlush` (after the medium's prepare step); a user packs
    /// [`LightVolumeReader::window`] into its own uniform and binds the views before
    /// `RenderSystems::PrepareBindGroups`.
    #[derive(Resource, Default, Clone)]
    pub(crate) struct LightVolumeReader {
        pub levels: [Option<LightVolumeLevelView>; 2],
        pub cam: Vec3,
        pub floor: f32,
        pub bend: f32,
        pub guard: f32,
        /// When each level became readable without a break since (a swap of fronts is no break).
        pub ready_since: [Option<std::time::Instant>; 2],
        /// Frames in a row the light volume made no plan while its state was held.
        unplanned: u32,
    }

    impl LightVolumeReader {
        /// Whether a level is readable this frame (near, far).
        pub fn ready(&self) -> [bool; 2] {
            [self.levels[0].is_some(), self.levels[1].is_some()]
        }

        /// Each level's presence for a reader that fades a level in over `arrival` real seconds.
        pub fn presence(&self, arrival: f32) -> [f32; 2] {
            [0, 1].map(|i| if self.levels[i].is_some() { lvr_presence(self.ready_since[i].map(|t| t.elapsed().as_secs_f32()), arrival) } else { 0.0 })
        }

        /// The shader struct with these presences (light_volume_reader.wesl `LightVolumeWindow`).
        pub fn window(&self, presence: [f32; 2]) -> LightVolumeWindow {
            lvr_window([0, 1].map(|i| self.levels[i].as_ref().map(|l| l.level)), (self.cam, self.floor, self.bend, self.guard), presence)
        }
    }

    /// Frames the shared reader keeps its last levels while the light volume's state is held but
    /// the medium's prepare step made no plan (its view's image not ready that frame, say): nothing
    /// was swept, so the sets are as they were and a reader falling back for one frame would show a
    /// pop. Longer (the medium stopped running) empties it. For those frames the camera, the drift
    /// (the windows) and the presences are the last planned frame's and the drop checks are not
    /// rerun (light_volume_reader.wesl's header states the exception).
    pub const LVR_KEEP_FRAMES: u32 = 4;

    /// Whether [`publish_light_volume_reader`] has anything to do: the light volume is held, or the
    /// reader still holds something of it.
    fn reader_in_use(gpu: Option<Res<LvGpu>>, reader: Res<LightVolumeReader>) -> bool {
        gpu.is_some_and(|g| g.0.is_some()) || reader.levels.iter().any(Option::is_some) || reader.ready_since.iter().any(Option::is_some)
    }

    /// Makes the shared reader for this frame from the light volume's state: its levels when the
    /// medium's prepare step planned the light volume this frame, the last ones for up to
    /// [`LVR_KEEP_FRAMES`] frames without a plan while the state is held, else none.
    pub(crate) fn publish_light_volume_reader(gpu: Option<ResMut<LvGpu>>, mut reader: ResMut<LightVolumeReader>) {
        let (held, fresh) = match gpu {
            Some(mut g) => {
                let planned = std::mem::replace(&mut g.1, false);
                let fresh = g.0.as_ref().filter(|_| planned).and_then(|state| {
                    let at = state.reader_at?;
                    let levels = lv_reader_levels(&state.schedule, at.field);
                    let views: [Option<LightVolumeLevelView>; 2] =
                        [0, 1].map(|i| levels[i].and_then(|level| state.levels[i].as_ref().map(|sets| LightVolumeLevelView { view: sets[level.set & 1].whole.clone(), level })));
                    Some((at, views))
                });
                (g.0.is_some(), fresh)
            }
            None => (false, None),
        };
        let reader = &mut *reader;
        match fresh {
            Some((at, views)) => {
                for (i, v) in views.iter().enumerate() {
                    reader.ready_since[i] = if v.is_some() { Some(reader.ready_since[i].unwrap_or_else(std::time::Instant::now)) } else { None };
                }
                reader.levels = views;
                (reader.cam, reader.floor, reader.bend, reader.guard) = (at.cam, at.floor, at.bend, LV_GUARD);
                reader.unplanned = 0;
            }
            None if held && reader.unplanned < LVR_KEEP_FRAMES => reader.unplanned += 1,
            None => {
                if reader.levels.iter().any(Option::is_some) || reader.ready_since.iter().any(Option::is_some) {
                    *reader = LightVolumeReader::default();
                }
            }
        }
    }

    /// The shared reader's resource and its system in the render world (added once, by its first
    /// user).
    pub(crate) struct LightVolumeReaderPlugin;

    impl Plugin for LightVolumeReaderPlugin {
        fn build(&self, app: &mut App) {
            let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) else { return };
            render_app
                .init_resource::<LightVolumeReader>()
                .add_systems(bevy::render::Render, publish_light_volume_reader.in_set(bevy::render::RenderSystems::PrepareResourcesFlush).run_if(reader_in_use));
        }
    }

    /// What [`lv_prepare`] needs of the medium's prepare step.
    pub(crate) struct LvPrepare<'a> {
        pub device: &'a RenderDevice,
        pub queue: &'a RenderQueue,
        pub cache: &'a PipelineCache,
        pub assets: &'a AssetServer,
        pub fullscreen: &'a FullscreenShader,
        /// The trace pipeline's label and entry (medium.rs `trace_entry`).
        pub entry: (&'static str, &'static str),
        /// The frame's camera (the trace's origin, world).
        pub cam: Vec3,
    }

    /// The producer's bindings 0-6 (volumetric_clouds_lightvol.wesl): the slice (dynamic offset),
    /// the sweep's frozen uniform, the base noise, its sampler, the erosion noise, the layer above,
    /// the outer level.
    pub(crate) fn producer_layout() -> BindGroupLayoutDescriptor {
        BindGroupLayoutDescriptor::new(
            "nvf_cloud_light_producer_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<LvSliceUniform>(true),
                    uniform_buffer::<CloudVolumeUniform>(false),
                    texture_3d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_3d(TextureSampleType::Float { filterable: true }),
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                ),
            ),
        )
    }

    /// The trace's light-volume layout: medium.rs `make_pipelines`' bindings 0-4 and the two levels
    /// (volumetric_clouds_march.wesl `vc_lv_near`, `vc_lv_far`, bindings 5 and 6).
    pub(crate) fn trace_layout() -> BindGroupLayoutDescriptor {
        BindGroupLayoutDescriptor::new(
            "medium_trace_light_volume_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<MediumTraceUniform>(false),
                    uniform_buffer::<CloudVolumeUniform>(false),
                    texture_3d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_3d(TextureSampleType::Float { filterable: true }),
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                ),
            ),
        )
    }

    fn producer_pipeline(layout: &BindGroupLayoutDescriptor, shader: Handle<Shader>, fullscreen: &FullscreenShader) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("nvf_cloud_light_producer_pipeline".into()),
            layout: vec![layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader,
                entry_point: Some("lv_produce".into()),
                targets: vec![Some(ColorTargetState { format: LV_FORMAT, blend: None, write_mask: ColorWrites::ALL })],
                ..default()
            }),
            ..default()
        }
    }

    /// A snapshot buffer, written once at once so its GPU buffer exists for the bind groups.
    fn snapshot_buffer(label: &str, u: &CloudVolumeUniform, device: &RenderDevice, queue: &RenderQueue) -> UniformBuffer<CloudVolumeUniform> {
        let mut b = UniformBuffer::from(*u);
        b.set_label(Some(label));
        b.write_buffer(device, queue);
        b
    }

    /// The frame's light volume, from the medium's prepare step: off (or no light) drops
    /// everything and returns the uniform as it came (the trace's legacy variant, today's
    /// picture); on, the state made or kept, the pipelines asked for, the frame's plan made (the
    /// snapshot of a sweep that starts uploaded) and the trace's uniform with its light-volume
    /// block from the fronts.
    pub(crate) fn lv_prepare(gpu: &mut LvGpu, clouds: &CloudVolumeUniform, cx: &LvPrepare) -> CloudVolumeUniform {
        let cfg: LvConfig = lv_config();
        if !lv_active(clouds, &cfg) {
            gpu.clear();
            return *clouds;
        }
        let (gn, gf) = (lv_grid(cfg.cells[0], clouds), lv_grid(cfg.cells[1], clouds));
        let key = LvKey { levels: cfg.levels, layers: [gn.layers, gf.layers], entry: cx.entry.1 };
        if gpu.0.as_ref().is_none_or(|s| s.key != key) {
            let producer_layout = producer_layout();
            let trace_layout = trace_layout();
            let pair = |label: &'static str, nodes: u32, layers: u32| [LvLevelGpu::new(cx.device, label, nodes, layers), LvLevelGpu::new(cx.device, label, nodes, layers)];
            let levels = [cfg.levels.near().then(|| pair("nvf_cloud_light_near", gn.nodes, gn.layers)), cfg.levels.far().then(|| pair("nvf_cloud_light_far", gf.nodes, gf.layers))];
            // The slices' uniforms, written once: per level, per layer under the top.
            let mut slices = DynamicUniformBuffer::<LvSliceUniform>::default();
            slices.set_label(Some("nvf_cloud_light_slices"));
            let mut offsets: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
            for (i, level) in levels.iter().enumerate() {
                let Some([level, _]) = level else { continue };
                let footprint = if i == 0 { LV_FOOTPRINT } else { LV_FAR_FOOTPRINT };
                let outer = if i == 0 && cfg.levels.far() { 1.0 } else { 0.0 };
                for j in 0..level.layers - 1 {
                    offsets[i].push(slices.push(&LvSliceUniform { slot: Vec4::new(i as f32, j as f32, footprint, outer) }));
                }
            }
            slices.write_buffer(cx.device, cx.queue);
            let snaps = [snapshot_buffer("nvf_cloud_light_near_snapshot", clouds, cx.device, cx.queue), snapshot_buffer("nvf_cloud_light_far_snapshot", clouds, cx.device, cx.queue)];
            let bytes: u64 = levels.iter().flatten().flatten().map(|l| u64::from(l.layers) * u64::from(LV_NODES) * u64::from(LV_NODES) * 2).sum();
            let sun = clouds.light_dir.y.max(0.05);
            let per_frame = |on: bool, g: &LvGrid, spread: u32| if on { lv_sweep_evaluations(g, sun) as f64 / 1e6 / f64::from(spread.max(1)) } else { 0.0 };
            info!(
                "volumetric clouds: light volume on ({}; near {}x{}x{} at {:.0} across, far {}x{}x{} at {:.0} across, {:.0} up; {:.2} MB in R16Float (two sets a level); write route: 2D array layers, one render pass a layer, top down; sweeps over {} and {} frames; {} near taps; at this sun {:.2}M + {:.2}M density evaluations a frame)",
                cfg.levels.word(),
                gn.nodes,
                gn.nodes,
                gn.layers,
                gn.across,
                gf.nodes,
                gf.nodes,
                gf.layers,
                gf.across,
                gn.up,
                bytes as f64 / 1_048_576.0,
                cfg.spread[0],
                cfg.spread[1],
                lv_near_taps(clouds, cfg.taps),
                per_frame(cfg.levels.near(), &gn, cfg.spread[0]),
                per_frame(cfg.levels.far(), &gf, cfg.spread[1])
            );
            gpu.0 = Some(LvState { key, producer_layout, trace_layout, views: false, slices, offsets, levels, snaps, schedule: LvSchedule::new(cfg), plan: None, groups: None, reader_at: None, told_taps: lv_near_taps(clouds, cfg.taps) });
        }
        let LvGpu(state, planned, pipelines) = gpu;
        let Some(state) = state.as_mut() else { return *clouds };
        // Only a variant to draw with is asked for (a cold build takes long); both ids are kept.
        let views = view_variant(clouds);
        state.views = views;
        pipelines.want_trace(cx.entry.1, views, || {
            let mut desc = crate::medium::trace_pipeline(&state.trace_layout, cx.assets.load("embedded://nvforever/medium_trace.wesl"), cx.entry.0, cx.entry.1, cx.fullscreen, views);
            desc.label = Some(format!("{}_light_volume{}", cx.entry.0, variant_suffix(views)).into());
            if let Some(f) = desc.fragment.as_mut() {
                f.shader_defs.push(LV_DEF.into());
            }
            cx.cache.queue_render_pipeline(desc)
        });
        let taps = lv_near_taps(clouds, cfg.taps);
        if state.told_taps != taps {
            state.told_taps = taps;
            info!("volumetric clouds: light volume: {taps} near taps now (LIGHT_TAPS {})", cfg.taps);
        }
        let plan = state.schedule.plan(clouds, cx.cam);
        // The shared reader's frame of reference (stage 2i M2): the trace's own (`LvSchedule::plan`'s
        // field position for this camera and drift).
        let field = DVec2::new(f64::from(cx.cam.x), f64::from(cx.cam.z)) + lv_drift_units(clouds);
        state.reader_at = Some(LvReaderAt { cam: cx.cam, field, floor: clouds.slab.x, bend: clouds.reach.y });
        for (snap, level) in state.snaps.iter_mut().zip(&plan.levels) {
            if let Some(u) = level.as_ref().and_then(|p| p.upload) {
                snap.set(u);
                snap.write_buffer(cx.device, cx.queue);
            }
            if level.is_some() {
                pipelines.want_producer(|| cx.cache.queue_render_pipeline(producer_pipeline(&state.producer_layout, cx.assets.load(LV_SHADER), cx.fullscreen)));
            }
        }
        if let Some(why) = plan.event {
            info!("volumetric clouds: light volume sets dropped ({why}); the legacy light where no level is ready until the sweeps finish");
        }
        let trace = plan.trace;
        state.plan = Some(plan);
        *planned = true;
        trace
    }

    /// The trace's inputs the light volume binds too: its uniform, the march's uniform, the base
    /// noise, its sampler, the erosion noise.
    pub(crate) type LvInputs<'a> = (&'a UniformBuffer<MediumTraceUniform>, &'a UniformBuffer<CloudVolumeUniform>, &'a TextureView, &'a Sampler, &'a TextureView);

    /// One slice pass: `target` cleared to zero and, with `draw`, the producer drawn over it at the
    /// slice's dynamic offset.
    fn slice_pass(ctx: &mut RenderContext, target: &TextureView, draw: Option<(&RenderPipeline, &BindGroup, u32)>) {
        let attachment = [Some(RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: Operations { load: LoadOp::Clear(wgpu::Color::TRANSPARENT), store: StoreOp::Store } })];
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some("nvf_cloud_light_slice"),
            color_attachments: &attachment,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some((pipeline, group, offset)) = draw {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[offset]);
            pass.draw(0..3, 0..1);
        }
    }

    /// The bind groups for this state and these inputs.
    fn make_groups(state: &LvState, (cache, device): (&PipelineCache, &RenderDevice), inputs: LvInputs, key: LvGroupKey) -> Option<LvGroups> {
        let (t, c, nb, ns, ne) = inputs;
        let pl = cache.get_bind_group_layout(&state.producer_layout);
        let tl = cache.get_bind_group_layout(&state.trace_layout);
        let slice_binding = state.slices.binding()?;
        let mut slices: [[Vec<Vec<BindGroup>>; 2]; 2] = Default::default();
        for (i, level) in state.levels.iter().enumerate() {
            let Some(sets) = level else { continue };
            let snap = state.snaps[i].binding()?;
            // The near level's far sets (with the far level on); otherwise its own layer above
            // again, never read.
            let outers: Vec<Option<&TextureView>> = match (i, state.levels[1].as_ref()) {
                (0, Some(far)) => vec![Some(&far[0].whole), Some(&far[1].whole)],
                _ => vec![None],
            };
            for (set, lanes) in sets.iter().zip(slices[i].iter_mut()) {
                for outer in &outers {
                    let lane = set.read[1..]
                        .iter()
                        .map(|above| device.create_bind_group("nvf_cloud_light_slice_group", &pl, &BindGroupEntries::sequential((slice_binding.clone(), snap.clone(), nb, ns, ne, above, outer.unwrap_or(above)))))
                        .collect();
                    lanes.push(lane);
                }
            }
        }
        let (tb, cb) = (t.binding()?, c.binding()?);
        let any = state.levels[0].as_ref().or(state.levels[1].as_ref())?;
        let near = |n: usize| state.levels[0].as_ref().map_or(&any[n].whole, |s| &s[n].whole);
        let far = |f: usize| state.levels[1].as_ref().map_or(&any[f].whole, |s| &s[f].whole);
        let group = |n: usize, f: usize| device.create_bind_group("medium_trace_light_volume_bind_group", &tl, &BindGroupEntries::sequential((tb.clone(), cb.clone(), nb, ns, ne, near(n), far(f))));
        Some(LvGroups { key, slices, trace: [[group(0, 0), group(0, 1)], [group(1, 0), group(1, 1)]] })
    }

    /// Before the trace, on its encoder [design, B2]: this frame's planned
    /// layers of each level's back set (the far level first, top down, then the near level), each
    /// level under its GPU span (`nvf_cloud_light_far`, `nvf_cloud_light_near`); returns the
    /// trace's light-volume pipeline and the bind group of the fronts, or `None` while a pipeline
    /// compiles, the volume is off or no level has a finished set yet, so the trace draws its
    /// legacy variant meanwhile (a plan that could not be encoded resets the sets next frame).
    pub(crate) fn lv_encode(gpu: &mut LvGpu, ctx: &mut RenderContext, (cache, device): (&PipelineCache, &RenderDevice), inputs: LvInputs, diagnostics: Option<&DiagnosticsRecorder>) -> Option<(RenderPipeline, BindGroup)> {
        let LvGpu(state, _, pipelines) = gpu;
        let state = state.as_mut()?;
        // The producer for each level with layers this frame, the trace by this frame's uniform.
        let mut producers: [Option<RenderPipeline>; 2] = [None, None];
        for (i, producer) in producers.iter_mut().enumerate() {
            if state.plan.as_ref().is_some_and(|plan| plan.levels[i].is_some()) {
                *producer = Some(cache.get_render_pipeline(pipelines.producer?)?.clone());
            }
        }
        let trace = cache.get_render_pipeline(pipelines.trace(state.key.entry, state.views)?)?.clone();
        let (t, c, nb, ns, ne) = inputs;
        let key: LvGroupKey = (t.buffer()?.id(), c.buffer()?.id(), state.slices.buffer()?.id(), [state.snaps[0].buffer()?.id(), state.snaps[1].buffer()?.id()], nb.id(), ns.id(), ne.id());
        if state.groups.as_ref().is_none_or(|g| g.key != key) {
            state.groups = Some(make_groups(state, (cache, device), inputs, key)?);
        }
        let groups = state.groups.as_ref()?;
        if let Some(plan) = state.plan.take() {
            for (i, span_name) in [(1usize, "nvf_cloud_light_far"), (0, "nvf_cloud_light_near")] {
                let Some(sets) = state.levels[i].as_ref() else { continue };
                let span = diagnostics.time_span(ctx.command_encoder(), span_name);
                if let (Some(p), Some(producer)) = (plan.levels[i].as_ref(), producers[i].as_ref()) {
                    let set = &sets[p.set & 1];
                    let lanes = &groups.slices[i][p.set & 1];
                    let lane = &lanes[p.outer.unwrap_or(0).min(lanes.len().saturating_sub(1))];
                    for j in p.written() {
                        let j = j as usize;
                        match (lane.get(j), state.offsets[i].get(j)) {
                            (Some(group), Some(offset)) if j + 1 < set.layers as usize => slice_pass(ctx, &set.write[j], Some((producer, group, *offset))),
                            _ => slice_pass(ctx, &set.write[j], None),
                        }
                    }
                }
                span.end(ctx.command_encoder());
            }
            state.schedule.encoded();
        }
        let [near, far] = state.schedule.fronts();
        if near.is_none() && far.is_none() {
            return None;
        }
        Some((trace, groups.trace[near.unwrap_or(0) & 1][far.unwrap_or(0) & 1].clone()))
    }
}

#[cfg(test)]
mod near_tap_tests {
    use super::*;
    use crate::volumetric_clouds::{uniform, VolumeInputs};

    /// The count the logs state is the one the shader marches: the asked count held
    /// to at least `VC_NEAR_LEAST` and at most the intervals.
    #[test]
    fn the_stated_near_taps_are_the_marched_ones() {
        let u = uniform(&VolumeInputs::default());
        assert_eq!([0, 1, 2, 9].map(|k| lv_near_taps(&u, k)), [1, 1, 2, 8]);
        let march = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        for text in [
            "@if(LIGHT_VOLUME) fn vc_near_count(n: u32, v: CloudVolume) -> u32 {\n    let requested = u32(max(v.lv.x, 0.0) + 0.5);\n    return min(max(requested, VC_NEAR_LEAST), n);\n}",
            "@if(!LIGHT_VOLUME) fn vc_near_count(n: u32, v: CloudVolume) -> u32 {\n    return n;\n}",
            "    let near = vc_near_count(taps, v);",
            "const VC_NEAR_LEAST: u32 = 1u;",
        ] {
            assert!(march.contains(text), "{text}");
        }
        // The two log lines state it.
        let here = include_str!("volumetric_clouds_lightvol.rs").replace("\r\n", "\n");
        assert!(here.contains("                lv_near_taps(clouds, cfg.taps),\n                per_frame("));
        assert!(include_str!("volumetric_clouds.rs").contains("lv_near_taps(&uniform(&drawn_inputs(START.inputs(VolumeInputs::default()))), "));
    }
}

/// The shared reader (stage 2i M2): its bookkeeping, its window arithmetic against the trace's,
/// the projection to the floor, the hand-over weights and the fallback, on the CPU mirror; and the
/// module's text against the march's reader.
#[cfg(test)]
mod reader_tests {
    use super::*;
    use crate::volumetric_clouds::LIGHT_REACH;

    /// A uniform with the tests' slab (the plane 21,024 up, 40,000 tall), the default bend,
    /// a light `elevation` degrees up at azimuth 30, and the drift `drift` (texture repeats of
    /// 60,000 units, through the vigour's first octave's whole cells and fraction).
    #[allow(clippy::field_reassign_with_default)]
    fn frame_uniform(elevation: f32, drift: [f64; 2]) -> CloudVolumeUniform {
        let mut u = CloudVolumeUniform::default();
        u.slab = Vec4::new(21_024.0, 40_000.0, 400_000.0, 300_000.0);
        u.reach = Vec4::new(LIGHT_REACH, crate::volumetric_clouds::shell_radius(f32::NAN, f32::NAN), 0.01, 196.0);
        u.march = Vec4::new(0.0, LIGHT_STEPS as f32, 0.0, 0.0);
        let (e, a) = (elevation.to_radians(), 30f32.to_radians());
        u.light_dir = Vec3::new(e.cos() * a.cos(), e.sin(), e.cos() * a.sin()).extend(1.0);
        u.nscale.z = 1.0;
        (u.map.x, u.map.y) = (1.0 / 60_000.0, 1.0 / 60_000.0);
        u.height.x = 4.0;
        let cells = |d: f64| d * 4.0;
        let (x, z) = (cells(drift[0]), cells(drift[1]));
        u.cells_v = bevy::math::UVec4::new(x.floor() as i64 as i32 as u32, z.floor() as i64 as i32 as u32, 0, 0);
        u.vigour = Vec4::new((x - x.floor()) as f32, (z - z.floor()) as f32, 0.0, 0.0);
        u
    }

    fn cfg() -> LvConfig {
        LvConfig { levels: LvLevels::Both, cells: [LV_ACROSS, LV_FAR_ACROSS], taps: 0, spread: LV_SPREAD }
    }

    /// A schedule after 120 drawn frames of `u` at `cam`, then the plan of the frame being drawn:
    /// the reader is asked between a frame's plan and its encode, as `lv_prepare` and
    /// `publish_light_volume_reader` ask it (asked after an encode, on a frame that promoted a
    /// front, the settled set is the new one and the kept set the old one, so it gives none).
    /// Both levels must be readable then.
    fn drawn_schedule(u: &CloudVolumeUniform, cam: Vec3) -> LvSchedule {
        let mut s = LvSchedule::new(cfg());
        for _ in 0..120 {
            let _ = s.plan(u, cam);
            s.encoded();
        }
        let _ = s.plan(u, cam);
        assert!(s.reader_bakes().iter().all(Option::is_some), "both levels readable on the frame being drawn");
        s
    }

    /// The reader names a front only from the frame after its last layer was encoded, keeps the
    /// previous front on the frame a new one is promoted, and never names a set the frame's plan
    /// writes; a plan that was not encoded, a dropped front (a teleport) or a reset gives none.
    #[test]
    fn the_reader_reads_only_fronts_whole_before_the_frame() {
        let mut s = LvSchedule::new(cfg());
        let cam = Vec3::new(-20_000.0, 900.0, 5_000.0);
        let mut last_fronts = [None; 2];
        let mut promoted_at: [Option<u32>; 2] = [None; 2];
        let at = |frame: u32| frame_uniform(45.0 + 0.0025 * frame as f32, [0.0001 * f64::from(frame), 0.0]);
        for frame in 0..200u32 {
            let u = at(frame);
            let plan = s.plan(&u, cam);
            let reader = s.reader_bakes();
            for (i, (written, read)) in plan.levels.iter().zip(reader).enumerate() {
                // A set the plan writes this frame is never the reader's.
                if let (Some(p), Some(b)) = (written.as_ref(), read.as_ref()) {
                    if p.to > p.from {
                        assert_ne!(p.set, b.set, "frame {frame} level {i}: the reader's set is written this frame");
                    }
                }
                // A front promoted this frame is not read until the next one; the old one is.
                if plan.fronts[i] != last_fronts[i] {
                    promoted_at[i] = Some(frame);
                    assert_eq!(read.map(|b| b.set), last_fronts[i], "frame {frame} level {i}: on the promotion frame the reader keeps the old front");
                } else if let Some(f) = plan.fronts[i] {
                    assert_eq!(read.map(|b| b.set), Some(f), "frame {frame} level {i}: the reader reads the front once it is whole");
                }
            }
            last_fronts = plan.fronts;
            s.encoded();
        }
        assert!(promoted_at.iter().all(Option::is_some), "both levels were promoted: {promoted_at:?}");
        // A plan that was never encoded: the next frame has no reader (everything restarts).
        let u = at(199);
        let _ = s.plan(&u, cam);
        assert!(s.reader_bakes().iter().all(Option::is_some), "the fronts stand before the missed encode");
        let _ = s.plan(&u, cam);
        assert!(s.reader_bakes().iter().all(Option::is_none), "a plan that was not drawn drops the reader with the fronts");
        s.encoded();
        // A teleport drops the fronts: no reader that frame.
        let mut s = LvSchedule::new(cfg());
        for frame in 0..120u32 {
            let _ = s.plan(&frame_uniform(45.0, [0.0; 2]), cam);
            assert!(frame < 40 || s.reader_bakes().iter().all(Option::is_some), "frame {frame}");
            s.encoded();
        }
        let _ = s.plan(&frame_uniform(45.0, [0.0; 2]), cam + Vec3::new(300_000.0, 0.0, 0.0));
        assert!(s.reader_bakes().iter().all(Option::is_none), "a dropped front is not read");
    }

    /// The reader's windows are the trace's: for every level a world point's node coordinate
    /// through the window (`window.xy + (P - cam).xz / across`) is the field point's own position
    /// in nodes from the front's lowest node (`lv_node_pos` of `world + repeat x drift`, the
    /// addressing the producer writes), whatever the drift since the bake; the trace's block says
    /// the same for the camera; the bent height is `bent_y`'s.
    #[test]
    fn the_reader_addresses_the_field_as_the_trace_does() {
        let mut s = LvSchedule::new(cfg());
        let cam0 = Vec3::new(12_345.0, 700.0, -54_321.0);
        let mut last = None;
        for frame in 0..120u32 {
            let drift = [0.0002 * f64::from(frame), -0.0001 * f64::from(frame)];
            let cam = cam0 + Vec3::new(3.0 * frame as f32, 0.0, 2.0 * frame as f32);
            let u = frame_uniform(40.0, drift);
            let plan = s.plan(&u, cam);
            let field = |p: Vec3| DVec2::new(f64::from(p.x), f64::from(p.z)) + lv_drift_units(&u);
            let levels = lv_reader_levels(&s, field(cam));
            for (i, l) in levels.iter().enumerate() {
                let Some(l) = l else { continue };
                let bake = s.reader_bakes()[i].expect("a reader level has its bake");
                for p in [cam, cam + Vec3::new(9_000.0, 0.0, -14_000.0), cam + Vec3::new(-25_000.0, 0.0, 30_000.0)] {
                    let rel = p - cam;
                    let via_window = Vec2::new(l.window.x + rel.x / l.window.z, l.window.y + rel.z / l.window.z);
                    let own = lv_node_pos(field(p), bake.lo, &bake.grid);
                    assert!((via_window - own).abs().max_element() < 2e-3, "frame {frame} level {i}: {via_window} against {own}");
                }
                if plan.fronts[i] == Some(l.set) {
                    let t = if i == 0 { plan.trace.lv_near } else { plan.trace.lv_far };
                    assert_eq!(t, l.window, "frame {frame} level {i}: the trace's own vector for the same front");
                }
                assert_eq!((l.window.z, l.window.w, l.nodes, l.layers), (bake.grid.across, bake.grid.up, bake.grid.nodes, bake.grid.layers));
            }
            last = Some((levels, u, cam));
            s.encoded();
        }
        // The CPU mirror reads a point above the floor at its own bent height (a layer coordinate
        // of `bent_y`'s height over the spacing): a field linear in the layer gives it back.
        let (levels, u, cam) = last.expect("frames ran");
        assert!(levels.iter().all(Option::is_some));
        let w = lvr_window(levels, (cam, u.slab.x, u.reach.y, LV_GUARD), [1.0, 1.0]);
        let layer = |n: IVec3| n.y as f32;
        let p = cam + Vec3::new(4_000.0, 25_000.0, -3_000.0);
        let r = lvr_optical_depth(&w, &layer, &layer, p, u.light_dir.truncate());
        let want = (bent_y(p - cam, cam.y, u.reach.y) - u.slab.x) / w.near.w;
        assert_eq!(r.y, 1.0, "inside the near window, past its guard band");
        assert!((r.x - want).abs() < 1e-3, "{} against {want}", r.x);
    }

    /// A receiver under the bent floor is moved along the light to it (its bent height zero there,
    /// the distance positive), so it reads what the floor point reads; a light under the horizon
    /// with no curvature to bring it up reports no data.
    #[test]
    fn a_receiver_under_the_floor_reads_where_its_light_meets_the_floor() {
        let u = frame_uniform(45.0, [0.0; 2]);
        let r = u.reach.y;
        let cam = Vec3::new(1_000.0, 2_000.0, -500.0);
        for elevation in [6.0f32, 15.0, 34.0, 60.0, 89.0] {
            for azimuth in [0.0f32, 70.0, 200.0, 310.0] {
                let (e, a) = (elevation.to_radians(), azimuth.to_radians());
                let l = Vec3::new(e.cos() * a.cos(), e.sin(), e.cos() * a.sin());
                for p in [cam, cam + Vec3::new(20_000.0, -1_500.0, 7_000.0), cam + Vec3::new(-60_000.0, 3_000.0, -90_000.0)] {
                    let rel = p - cam;
                    let bh = bent_y(rel, cam.y, r) - u.slab.x;
                    assert!(bh < 0.0);
                    let a2 = (l.x * l.x + l.z * l.z) / (2.0 * r);
                    let b = l.y + 2.0 * (rel.x * l.x + rel.z * l.z) / (2.0 * r);
                    let t = lvr_rise(a2, b, bh);
                    assert!(t > 0.0, "{elevation} {azimuth}: {t}");
                    let on = bent_y(rel + l * t, cam.y, r) - u.slab.x;
                    assert!(on.abs() < 2.0, "{elevation} {azimuth}: bent height {on} where the floor should be");
                }
            }
        }
        assert_eq!(lvr_rise(0.0, -0.1, -100.0), -1.0, "never reaches the floor");
        assert_eq!(lvr_rise(0.0, 0.5, -100.0), 200.0, "the straight case");
        // The floor point and the receiver under it read the same depth.
        let s = drawn_schedule(&u, cam);
        let field = DVec2::new(f64::from(cam.x), f64::from(cam.z)) + lv_drift_units(&u);
        let w = lvr_window(lv_reader_levels(&s, field), (cam, u.slab.x, u.reach.y, LV_GUARD), [1.0, 1.0]);
        let smooth = |n: IVec3| 0.01 * n.x as f32 + 0.02 * n.z as f32 + 3.0 * n.y as f32;
        let l = u.light_dir.truncate();
        let p = cam + Vec3::new(3_000.0, -1_000.0, 2_000.0);
        let rel = p - cam;
        let t = lvr_rise((l.x * l.x + l.z * l.z) / (2.0 * r), l.y + 2.0 * (rel.x * l.x + rel.z * l.z) / (2.0 * r), bent_y(rel, cam.y, r) - u.slab.x);
        let below = lvr_optical_depth(&w, &smooth, &smooth, p, l);
        let floor = lvr_optical_depth(&w, &smooth, &smooth, p + l * (t - 0.01), l);
        assert!(below.y > 0.0 && (below - floor).abs().max_element() < 1e-2, "{below} against {floor}");
    }

    /// A ground receiver under a known column reads that column: the column is put on one node of
    /// the near front, addressed the way the producer writes it (the field position of the node,
    /// `(lo + n) x across - repeat x drift`, on the bent floor), the receiver set back from that
    /// floor point along the light to a ground height well under the floor, at slanted suns of an
    /// azimuth on neither axis, with x and z distinct (a swapped axis, a wrong sign of the light,
    /// the wrong end of the layers or a camera-relative slip each miss the column and read 0).
    #[test]
    fn a_ground_receiver_reads_the_column_above_it_along_the_light() {
        for (elevation, drift) in [(50.0f32, [0.0, 0.0]), (25.0, [0.137, -0.291]), (70.0, [-3.25, 1.5])] {
            let u = frame_uniform(elevation, drift);
            let cam = Vec3::new(-73_728.0, 12_500.0, -12_500.0);
            let s = drawn_schedule(&u, cam);
            let bake = s.reader_bakes()[0].expect("a near front");
            let field = DVec2::new(f64::from(cam.x), f64::from(cam.z)) + lv_drift_units(&u);
            let w = lvr_window(lv_reader_levels(&s, field), (cam, u.slab.x, u.reach.y, LV_GUARD), [1.0, 0.0]);
            let (n0x, n0z) = (bake.lo[0] + 70, bake.lo[1] + 52);
            let across = f64::from(bake.grid.across);
            let d = lv_drift_units(&u);
            let q = Vec2::new((n0x as f64 * across - d.x) as f32, (n0z as f64 * across - d.y) as f32);
            let qr = q - Vec2::new(cam.x, cam.z);
            let qy = u.slab.x - qr.length_squared() / (2.0 * u.reach.y);
            let l = u.light_dir.truncate();
            let ground = cam.y - 800.0;
            let p = Vec3::new(q.x, qy, q.y) - l * ((qy - ground) / l.y);
            let column = |n: IVec3| if (n.x, n.z) == (70, 52) { 6.0 } else { 0.0 };
            let none = |_: IVec3| -> f32 { panic!("the far level has presence 0 here") };
            let r = lvr_optical_depth(&w, &column, &none, p, l);
            assert_eq!(r.y, 1.0, "elevation {elevation}: inside the near window");
            assert!((r.x - 6.0).abs() < 0.05, "elevation {elevation}, drift {drift:?}: the receiver under the column reads {} (receiver {p}, floor point {q})", r.x);
            // One cell to the side along x, then along z: nothing.
            for side in [Vec3::new(500.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 500.0)] {
                assert!(lvr_optical_depth(&w, &column, &none, p + side, l).x < 0.05, "elevation {elevation}: the column leaks a cell away");
            }
        }
    }

    /// The hand-over weight is 0 outside a window and at its edge, rises linearly over the guard
    /// band to 1 and stays there, times the presence; the presence rises smoothly from 0 to 1 over
    /// the arrival frames. Moving a receiver across the near window's edge and then the far
    /// window's changes the reader's result by no more than the field's own change (no seam), the
    /// covered share going from 1 to 0 at the far edge.
    #[test]
    fn the_hand_over_is_continuous() {
        let g = LV_GUARD;
        assert_eq!(lvr_weight(-0.01, g, 1.0), 0.0);
        assert_eq!(lvr_weight(0.0, g, 1.0), 0.0);
        assert_eq!(lvr_weight(g, g, 1.0), 1.0);
        assert_eq!(lvr_weight(10.0 * g, g, 1.0), 1.0);
        assert_eq!(lvr_weight(0.5 * g, g, 0.5), 0.25);
        assert_eq!(lvr_weight(f32::NAN, g, 1.0), 0.0);
        let mut prev = 0.0;
        for k in 0..=1_000 {
            let e = -1.0 + k as f32 * 0.01;
            let w = lvr_weight(e, g, 1.0);
            assert!((w - prev).abs() <= 0.01 / g + 1e-6, "a jump at {e}");
            assert_eq!(w, lv_guard(Vec3::new(e, 0.0, 1.0e3), u32::MAX, u32::MAX, g), "the trace's guard for presence 1 at {e}");
            prev = w;
        }
        assert_eq!(lvr_presence(None, LVR_ARRIVAL_SECS), 0.0);
        assert_eq!(lvr_presence(Some(0.0), LVR_ARRIVAL_SECS), 0.0);
        assert_eq!(lvr_presence(Some(LVR_ARRIVAL_SECS), LVR_ARRIVAL_SECS), 1.0);
        assert_eq!(lvr_presence(Some(10.0 * LVR_ARRIVAL_SECS), LVR_ARRIVAL_SECS), 1.0);
        assert_eq!(lvr_presence(Some(f32::NAN), LVR_ARRIVAL_SECS), 0.0);
        let steps: Vec<f32> = (0..=60).map(|f| lvr_presence(Some(f as f32 / 60.0 * LVR_ARRIVAL_SECS), LVR_ARRIVAL_SECS)).collect();
        assert!(steps.windows(2).all(|p| p[1] >= p[0] && p[1] - p[0] <= 1.6 / 60.0), "{steps:?}");
        // Across the windows: a vertical sun, so the receiver reads the floor straight above it.
        let u = frame_uniform(89.9, [0.0; 2]);
        let cam = Vec3::new(0.0, 500.0, 0.0);
        let s = drawn_schedule(&u, cam);
        let field = DVec2::new(0.0, 0.0) + lv_drift_units(&u);
        let levels = lv_reader_levels(&s, field);
        let w = lvr_window(levels, (cam, u.slab.x, u.reach.y, LV_GUARD), [1.0, 1.0]);
        // One smooth field over world x, as each level's nodes sample it.
        let world_x = |l: Vec4, n: IVec3| (n.x as f32 - l.x) * l.z;
        let near = |n: IVec3| 2.0 + (world_x(w.near, n) / 50_000.0).sin();
        let far = |n: IVec3| 2.0 + (world_x(w.far, n) / 50_000.0).sin();
        let l = u.light_dir.truncate();
        let (mut last, mut seen_far_edge) = (None::<Vec2>, false);
        let mut x = 0.0f32;
        while x < 300_000.0 {
            let r = lvr_optical_depth(&w, &near, &far, Vec3::new(x, 0.0, 0.0), l);
            if let Some(p) = last {
                let field_step = 250.0 / 50_000.0 * 1.01 + 1e-3;
                assert!((r.y - p.y).abs() <= 250.0 / (LV_GUARD * LV_FAR_ACROSS) + 1e-3, "covered jumps at x {x}: {p} to {r}");
                if r.y > 0.0 && p.y > 0.0 {
                    assert!((r.x - p.x).abs() <= field_step + 0.02, "depth jumps at x {x}: {p} to {r}");
                }
            }
            seen_far_edge |= r.y == 0.0;
            last = Some(r);
            x += 250.0;
        }
        assert!(seen_far_edge, "the far window ends inside the walk");
    }

    /// A level with no front, or with presence 0, is never read: none ready gives no data, the
    /// near level alone gives none outside its window, a level fading in covers only its presence.
    #[test]
    fn a_missing_front_gives_no_data() {
        let u = frame_uniform(60.0, [0.0; 2]);
        let cam = Vec3::new(0.0, 500.0, 0.0);
        let l = u.light_dir.truncate();
        let panic_fetch = |_: IVec3| -> f32 { panic!("a level without a front was read") };
        let none = lvr_window([None, None], (cam, u.slab.x, u.reach.y, LV_GUARD), [1.0, 1.0]);
        assert_eq!((none.floor.z, none.floor.w), (0.0, 0.0), "no level, no presence whatever was asked");
        assert_eq!(lvr_optical_depth(&none, &panic_fetch, &panic_fetch, cam, l), Vec2::ZERO);
        let s = drawn_schedule(&u, cam);
        let field = DVec2::new(0.0, 0.0) + lv_drift_units(&u);
        let [near, far] = lv_reader_levels(&s, field);
        let one = |_: IVec3| 1.0;
        let near_only = lvr_window([near, None], (cam, u.slab.x, u.reach.y, LV_GUARD), [1.0, 1.0]);
        assert_eq!(lvr_optical_depth(&near_only, &one, &panic_fetch, Vec3::new(1_000.0, 0.0, 0.0), l), Vec2::new(1.0, 1.0));
        assert_eq!(lvr_optical_depth(&near_only, &one, &panic_fetch, Vec3::new(1_000_000.0, 0.0, 0.0), l), Vec2::ZERO, "outside the near window, no far level");
        let fading = lvr_window([near, far], (cam, u.slab.x, u.reach.y, LV_GUARD), [0.25, 0.0]);
        let r = lvr_optical_depth(&fading, &one, &panic_fetch, Vec3::new(1_000.0, 0.0, 0.0), l);
        assert!((r - Vec2::new(1.0, 0.25)).abs().max_element() < 1e-6, "{r}");
        // A light under the horizon meets the bent floor only far outside every window: no data.
        assert_eq!(lvr_optical_depth(&near_only, &one, &panic_fetch, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, -0.2, 0.98).normalize()), Vec2::ZERO);
    }

    /// The reader module repeats the march reader's addressing and nesting word for word, and is
    /// valid WGSL by itself; its struct is the Rust one's, field for field.
    #[test]
    fn the_reader_module_matches_the_trace_and_is_valid_wgsl() {
        let reader = include_str!("light_volume_reader.wesl").replace("\r\n", "\n");
        let march = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        for line in [
            "    let edge = min(min(c.x, nodes - 1.0 - c.x), min(c.y, nodes - 1.0 - c.y));",
            "    let y = min(bh / max(up, 1.0), layers - 1.0);",
            "    let y0 = min(floor(y), layers - 2.0);",
            "    let uv = (c + 0.5) / nodes;",
            "    let nodes = max(size.x, 2.0);",
            "    let layers = max(size.y, 2.0);",
        ] {
            assert!(reader.contains(line) && march.contains(line), "{line}");
        }
        assert!(march.contains("let r = vc_lv_fetch(vc_lv_near, ns, v.lv_near.xy + p.xz / max(v.lv_near.z, 1.0), bh, v.lv_near.w, v.lv_size.xz);"));
        assert!(reader.contains("let r = lvr_level(tn, s, w.near.xy + rel.xz / max(w.near.z, 1.0), bh, w.near.w, w.size.xz);"));
        assert!(march.contains("return cam.y + rel.y + dot(rel.xz, rel.xz) / (2.0 * max(v.reach.y, 1.0));"), "the march's bend");
        assert!(reader.contains("var bh = cam.y + rel.y + dot(rel.xz, rel.xz) / twice_r - w.floor.x;"));
        for nest in ["optical += keep * wf * r.y;", "keep *= 1.0 - wf;", "keep = 1.0 - wn;"] {
            assert!(reader.contains(nest), "{nest}");
        }
        assert!(march.contains("optical += keep * w * r.y;") && march.contains("keep *= 1.0 - w;"));
        let fields: Vec<usize> = ["    near: vec4<f32>,", "    far: vec4<f32>,", "    size: vec4<f32>,", "    cam: vec4<f32>,", "    floor: vec4<f32>,"].iter().map(|f| reader.find(f).unwrap_or_else(|| panic!("{f}"))).collect();
        assert!(fields.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(<LightVolumeWindow as bevy::render::render_resource::ShaderType>::min_size().get(), 80, "five vec4s");
        assert!(!reader.contains("@group(") && !reader.contains("@if("), "no bindings and no conditions: the user binds");
        let module = wgpu::naga::front::wgsl::parse_str(&reader).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&reader)));
        wgpu::naga::valid::Validator::new(wgpu::naga::valid::ValidationFlags::all(), wgpu::naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    }
}
