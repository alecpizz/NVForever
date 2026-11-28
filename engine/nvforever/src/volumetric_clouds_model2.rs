//! The volumetric clouds' lean model (#3032; docs/design/graphics-enhanced.md §3.23): the CPU
//! mirror of the march's `vc_m2_*` functions (volumetric_clouds_march.wesl), text-pinned against the
//! shader, with the boundary cases. The one CPU density
//! function the estimators and oracles take (volumetric_clouds_model2_tests.rs). An
//! engineering choice for the enhanced look, never a parity item.
//!
//! Written from the published descriptions of
//! real-time cloud systems [documented: Bauer, "Creating the Atmospheric World of Red Dead Redemption 2",
//! SIGGRAPH 2019, slides 14-18: a top-down cover map, a gradient over height per cloud type, the base
//! shape the cover thresholded against it, the detail carved in from one 3D Perlin-Worley read by a linear
//! rescale; Schneider and Vos, "The Real-time Volumetric Cloudscapes of Horizon Zero Dawn", SIGGRAPH 2015,
//! and Schneider, "Nubis: Authoring Real-Time Volumetric Cloudscapes with the Decima Engine", SIGGRAPH
//! 2017: a low-frequency Perlin-Worley base remapped by the coverage makes the billowy lobes, a
//! higher-frequency Worley fBm erodes the edge region, wispy near the base and billowy (inverted) above].
//! One flat condensation base for every column; a signed field (the cover times a profile per type
//! against the base noise, less the erosion's displacement of the surface) whose zero is the surface,
//! the density rising over a narrow transition; its own extinction per unit by type. Nested heads stretch the upper
//! profile above a common basal layer. The constants and the form are our own [design]; no code from any
//! project.
//!
//! The mirror is the tests' (the shader is the truth at run time), hence no use outside them yet.
#![cfg_attr(not(test), allow(dead_code))]

use crate::volumetric_clouds::CloudVolumeUniform;
use bevy::prelude::*;

/// The base noise's spread value's mean gradient per repeat where it is 0.35..0.65 (march `VC_BASE_GRAD`;
/// measured from the bake, `the_base_gradient_is_the_bakes`).
pub const BASE_GRAD: f32 = 30.0;
/// The light's filter cell without a volume's spacing (march `VC_LIGHT_CELL`), units.
pub const LIGHT_CELL: f32 = 500.0;

/// The march's own smoothstep (WGSL's: Hermite between two edges, `a < b`).
pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The light's filter cell (march `vc_light_cell`): the level's spacing the uniform carries in
/// `lcell.x`, else [`LIGHT_CELL`].
pub fn light_cell(u: &CloudVolumeUniform) -> f32 {
    if u.lcell.x > 0.0 { u.lcell.x } else { LIGHT_CELL }
}

/// The base noise's spread value, 0..1 (march `vc_spread_base`).
pub fn spread(b: f32, u: &CloudVolumeUniform) -> f32 {
    (0.25 + 0.5 * (b - u.nlobe.x) / (u.nlobe.y - u.nlobe.x).max(1e-3)).clamp(0.0, 1.0)
}

/// The placement excess over its threshold at which the cover is whole (march `VC_M2_COVER`; a smooth
/// step up to it).
pub const COVER: f32 = 0.12;
/// Per type (sheet, cumulus, tower; march `VC_M2_*`): the height as a share of the domain (times the
/// vigour's scale), the base rescale's relief at SHAPE 1 (sheets low: overcast stays whole), the surface's
/// least transition (units), the extinction per unit at density 1 and DENSITY 1 (optical depth 1 in 400
/// units of whole cumulus), and the erosion's strength.
pub const EXTENT: [f32; 3] = [0.11, 0.14, 0.48];
pub const RELIEF: [f32; 3] = [0.25, 0.75, 0.6];
pub const EDGE: [f32; 3] = [600.0, 240.0, 300.0];
pub const SIGMA: [f32; 3] = [1.5e-3, 2.5e-3, 3.0e-3];
pub const ERODE_TYPE: [f32; 3] = [0.3, 1.0, 0.8];
/// The erosion read's frequency over its texture's own repeat (march `VC_M2_ERODE_FREQ`): cells 2,000 / 1,000 / 500 units,
/// finer than the base noise's lobes (1,500 to 6,000); a whole number, so the drift's phase scaled by it
/// moves whole repeats where it wraps.
pub const ERODE_FREQ: f32 = 2.0;
/// Each erosion band's surface displacement at EROSION 1, as a share of its cell (march `VC_M2_BILLOW`).
pub const BILLOW: f32 = 1.15;
/// The least field change per unit (march `VC_M2_GRAD_LEAST`).
pub const GRAD_LEAST: f32 = 1.0e-4;
/// The share of the column above a sample holding cloud, for the light's vertical estimate (march
/// `VC_M2_FILL`).
pub const FILL: f32 = 0.6;
/// The wisps' gradient per WISP per unit of amplitude for the erosion's step bound (march
/// `VC_M2_WISP_GRAD`), a little over [`crate::volumetric_clouds_wisp::wisp_bounds`]'s.
pub const WISP_GRAD: f32 = 2.04e-3;
/// How far the erosion bands move the fair lobes' fine read, in its noise coordinates (march `VC_M2_FRAY`).
#[cfg_attr(not(test), allow(dead_code))]
pub const FRAY: f32 = 0.10;

/// Diagonal world-space shear of the placement field (march `vc_m2_place_pos`).
pub const SHEAR: f32 = 0.10;

/// [hypothesis] Shorter horizontal cells reduce fair bodies' angular sizes. Keep the
/// wavelength fixed through weather fades so distant formations stay anchored.
pub const PLACE_FREQUENCY: f32 = 2.2;

/// The larger lobe banks share a fixed placement scale through weather transitions.
pub fn place_frequency(puff: f32) -> f32 {
    PLACE_FREQUENCY * (1.0 - 0.36 * puff)
}

/// The fair lobe material loses weight continuously as coverage closes into a deck.
pub fn lobe_weight(types: Vec3, u: &CloudVolumeUniform) -> f32 {
    u.kind.x * (1.0 - types.x) * u.nscale.z.clamp(0.0, 1.0) * (1.0 - smoothstep(0.55, 0.72, u.scale.w)) * smoothstep(0.0, 0.10, u.scale.w)
}

/// [hypothesis] Join the bank progressively through heavy weather. At full deck the
/// entire placement range has positive cover, even at its deepest trough.
pub fn placement_threshold(amount: f32) -> f32 {
    let deck = smoothstep(0.55, 0.85, amount);
    let fair = crate::volumetric_clouds::place_threshold(amount);
    fair + (-COVER - fair) * deck
}

fn by_type(w: Vec3, k: [f32; 3]) -> f32 {
    w.dot(Vec3::from_array(k))
}

/// What the lean model reads of a place's column (march `VcColumn`): the placement's excess over its
/// threshold, the local type weights (sheet, cumulus, tower), the vigour (0..1) and how much of the
/// placement the footprint resolves.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Column {
    /// Share of fair-cloud depth sustained by a broad, humid updraft.
    pub developed_bank: f32,
    pub fair_water_excess: f32,
    /// Filtered humidity at the condensation level.
    pub base_humidity: f32,
    /// The regional base relief and the local variation in water content.
    pub deck_relief: Vec2,
    pub head: f32,
    pub over: f32,
    pub types: Vec3,
    pub vigour: f32,
    pub res: f32,
}

/// A column's lean geometry (march `vc_m2_geometry`): the cover, the height (units), the shading's top
/// share and the relief.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Geometry {
    pub crown_top: f32,
    pub crown_cover: f32,
    pub deck_relief: Vec2,
    pub base_humidity: f32,
    pub parent: f32,
    pub cover: f32,
    pub extent: f32,
    pub top: f32,
    pub relief: f32,
}

/// The cover, 0..1 (march `vc_m2_cover`): a smooth step of the excess up to [`COVER`]; above 0 exactly
/// where the field passes its threshold.
pub fn cover(over: f32) -> f32 {
    smoothstep(0.0, COVER, over)
}

/// The unstretched bank height (march `vc_m2_parent_extent`).
fn parent_extent(col: &Column, u: &CloudVolumeUniform) -> f32 {
    let depth = u.slab.y.max(1.0);
    let scale = u.scale.x + (u.scale.y - u.scale.x) * col.vigour.clamp(0.0, 1.0).powf(u.scale.z.max(1e-3));
    let bank = smoothstep(0.55, 0.85, u.scale.w) * smoothstep(0.26, 0.85, col.over);
    let core = (1.0 - smoothstep(0.55, 0.72, u.scale.w)) * smoothstep(0.055, 0.16, col.over) * u.nscale.z.clamp(0.0, 1.0);
    (by_type(col.types, EXTENT) * depth * scale * (1.0 + 0.55 * bank + 0.20 * core)).clamp(u.field.w, depth)
}

/// Depth of the unmodified bank as a fraction of the parent column.
pub const FOOT: f32 = 0.36;

/// Stretch the crown about the parent bank's foot.
pub fn extent(col: &Column, u: &CloudVolumeUniform) -> f32 {
    let parent = parent_extent(col, u);
    (parent + (1.0 - FOOT) * parent * col.head).clamp(u.field.w, u.slab.y.max(1.0))
}

/// Upper bound on the bent height used by the view and its support probes.
pub fn sampling_ceiling(u: &CloudVolumeUniform) -> f32 {
    let domain = u.slab.y.max(1.0);
    if u.mix.x == 0.0 && u.mix.z == 0.0 && lobe_weight(Vec3::Y, u) >= 1.0 {
        let parent = u.field.w.max(EXTENT[1] * domain * u.scale.x.max(u.scale.y) * 1.20);
        return u.slab.x + domain.min(parent * (1.0 + (1.0 - FOOT) * 1.55) + 1.0);
    }
    u.slab.x + domain
}

pub(super) fn stretch(y: f32, src: f32, dst: f32, foot: f32) -> f32 {
    y.min(foot) + (y - foot).max(0.0) * ((dst - foot) / (src - foot).max(1.0))
}

/// The base rescale's relief for the types `w`, times SHAPE (march `vc_m2_relief`).
pub fn relief(w: Vec3, u: &CloudVolumeUniform) -> f32 {
    by_type(w, RELIEF) * u.nscale.z.max(0.0)
}

/// The field's change per world unit at relief `r` (march `vc_m2_grad`).
pub fn grad(r: f32, u: &CloudVolumeUniform) -> f32 {
    (r * BASE_GRAD * u.nscale.x).max(GRAD_LEAST)
}

/// The erosion read's three cells (units; march `vc_m2_cells`).
pub fn cells(u: &CloudVolumeUniform) -> [f32; 3] {
    let repeat = 1.0 / (u.nscale.y * ERODE_FREQ).max(1e-9);
    [repeat / 3.0, repeat / 6.0, repeat / 12.0]
}

/// The erosion's displacement per unit of a band's form for the types `w` (march `vc_m2_billow`).
pub fn billow(w: Vec3, u: &CloudVolumeUniform) -> f32 {
    BILLOW * u.nscale.w.max(0.0) * by_type(w, ERODE_TYPE)
}

/// The shading's top estimate (march `vc_m2_top`): the height share where the envelope at cover `c` meets
/// `tau`, times `min(c / tau, 1)`; continuous in the cover, 0 with none.
pub fn top(c: f32, tau: f32, w: Vec3, u: &CloudVolumeUniform) -> f32 {
    if tau <= 0.0 {
        return 1.0;
    }
    let a = w.x * u.knot_s0 + w.y * u.knot_c0 + w.z * u.knot_t0;
    let b = w.x * u.knot_s1 + w.y * u.knot_c1 + w.z * u.knot_t1;
    let need = (tau / c.max(1e-6)).min(a.x);
    let k = [a.x, a.y, a.z, a.w, b.x, b.y, b.z, b.w];
    let mut t = 1.0;
    for i in 1..8 {
        if k[i] < need {
            let f = ((k[i - 1] - need) / (k[i - 1] - k[i]).max(1e-6)).clamp(0.0, 1.0);
            t = ((i - 1) as f32 + f) / 7.0;
            break;
        }
    }
    t * (c / tau).min(1.0)
}

/// The threshold the top estimate takes (march `vc_m2_geometry`): half the relief at the noise median.
/// The local tears are confined to the resolved margin and omitted from this estimate.
pub fn top_threshold(w: Vec3, u: &CloudVolumeUniform) -> f32 {
    0.5 * relief(w, u)
}

/// The column's geometry (march `vc_m2_geometry`).
pub fn geometry(col: &Column, u: &CloudVolumeUniform) -> Geometry {
    let c = cover(col.over);
    let parent = parent_extent(col, u);
    let extent = extent(col, u);
    let tau = top_threshold(col.types, u);
    let bank_top = top(c, tau, col.types, u);
    let saddle = smoothstep(0.50, 1.0, c) * smoothstep(0.0, 0.28, -col.head);
    let attachment_width = 0.045 * mixed_sheet(u);
    let crown_head = if attachment_width > 0.0 { col.head * smoothstep(0.0, attachment_width, col.over) } else { col.head };
    let crown_cover = cover(col.over + 0.18 * crown_head) * (1.0 - 0.44 * saddle);
    let peak = if crown_cover == c { bank_top } else { top(crown_cover, tau, col.types, u) };
    let crown_top = stretch(parent * peak, parent, extent, FOOT * parent);
    Geometry { parent, cover: c, crown_cover, crown_top, deck_relief: col.deck_relief, base_humidity: col.base_humidity, extent, top: bank_top, relief: relief(col.types, u) }
}

/// How much of each band a filter width `ew` keeps (march `vc_m2_keeps`).
pub fn keeps(ew: f32, u: &CloudVolumeUniform) -> [f32; 3] {
    cells(u).map(|c| crate::volumetric_clouds::band_keep(c, ew))
}

/// The erosion read's coordinates (march `vc_m2_erode_coord`).
pub fn erode_coord(pos: Vec3, yb: f32, u: &CloudVolumeUniform) -> Vec3 {
    ERODE_FREQ * Vec3::new(pos.x * u.nscale.y + u.nphase.z, (yb - u.slab.x) * u.nscale.y, pos.z * u.nscale.y + u.nphase.w)
}

/// Where a lean sample is: its height over the flat base, the cast width, the view's step filter `fw`
/// (`vc_detail_filter`), the view's (`eroded`) or the light's, the wisps on (`detail`) and their stretch
/// there (`wisp_g`, march `vc_wisp(..).g`; read only where the wisps act).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub above: f32,
    pub width: f32,
    pub fw: f32,
    pub eroded: bool,
    pub detail: bool,
    pub wisp_g: f32,
}

/// The base read takes its world-space filter width, frequency and offset in repeats.
/// The erosion read takes its filter width at [`erode_coord`] displaced by the wisps.
pub struct Reads<'a> {
    pub base: &'a mut dyn FnMut(f32, f32, Vec3) -> f32,
    pub erode: &'a mut dyn FnMut(f32) -> [f32; 3],
}

fn light_height(g: &Geometry, above: f32) -> f32 {
    let base_top = g.parent * g.top;
    base_top + (g.crown_top - base_top) * smoothstep(0.52, 0.88, above / g.parent.max(1.0))
}

/// The lean model's density (0..1) and the shading's height share (march `vc_m2_density`), read for read.
pub fn density(g: &Geometry, w: Vec3, s: &Sample, u: &CloudVolumeUniform, reads: &mut Reads) -> (f32, f32) {
    let physical_h = s.above / g.extent.max(1.0);
    let hs = (s.above / light_height(g, s.above).max(1.0)).clamp(0.0, 1.0);
    if g.cover <= 0.0 || physical_h >= 1.0 {
        return (0.0, hs);
    }
    let h = stretch(s.above, g.extent, g.parent, FOOT * g.parent) / g.parent;
    let storm = smoothstep(0.55, 0.72, u.scale.w) * smoothstep(0.0, 0.08, w.x);
    let upper = smoothstep(FOOT, 0.56, h);
    let lift = (g.extent / g.parent - 1.0).max(0.0);
    let join = (s.above / g.parent - (0.88 + 0.24 * lift)).abs();
    let neck = 0.46 * smoothstep(0.45, 1.45, lift) * (1.0 - smoothstep(0.0, 0.33, join));
    let e = (g.cover + (g.crown_cover - g.cover) * upper) * crate::volumetric_clouds::profile_at(h, w).clamp(0.0, 1.0) * (1.0 - neck);
    if e <= 0.0 {
        return (0.0, hs);
    }
    let lw = if s.eroded {
        let step_width = s.width.max(s.fw);
        let sample_area = ((s.width * u.dshape.w).max(1.0) * step_width).sqrt();
        step_width + (sample_area - step_width) * mixed_sheet(u)
    } else {
        2.0 * s.width.max(light_cell(u))
    };
    let core = (1.0 - smoothstep(0.55, 0.72, u.scale.w)) * smoothstep(0.50, 0.90, g.cover) * (1.0 - upper);
    let r = g.relief * (1.0 - 0.60 * core);
    let gr = grad(r, u);
    // Mip filtering already averages the noise. Keep the coarse light transition centred on
    // the near surface and bound its width in field units, so a dense core stays opaque.
    let intrinsic = gr * by_type(w, EDGE);
    let ramp = if s.eroded { intrinsic.max(2.0 * gr * lw) } else { intrinsic.max((2.0 * gr * lw).min(0.42)) };
    let offset = if s.eroded { 0.0 } else { (0.5 * (ramp - intrinsic)).min(0.5 * e) };
    let c = cells(u);
    let fair = (1.0 - smoothstep(0.55, 0.72, u.scale.w)) * u.nscale.z.clamp(0.0, 1.0);
    let side = fair * (1.0 - smoothstep(0.16, 0.46, g.cover));
    let amp = (1.0 - storm) * billow(w, u) * upper.max(side) * (0.12 + 2.0 * (1.0 - e)).min(1.0);
    let most = gr * amp * (0.5 * (c[0] + c[1] + c[2]) + c[1]);
    let widest = ramp + 0.5 * gr * amp * c[1];
    let reach = 0.58 * gr * amp * c[0] * (1.0 - smoothstep(0.55, 0.72, u.scale.w));
    let mixed = mixed_sheet(u);
    let dry_base = 1.0 - smoothstep(0.25, 0.75, g.base_humidity);
    let bed = 0.48 * g.parent * (-g.deck_relief.x).max(0.0) + mixed * dry_base * (0.16 * g.parent).min(600.0);
    let soft = (0.6 * u.profile.x).max(1.0);
    let fair_skin = fair * (0.32 + 0.68 * side) * (0.34 * g.parent).min(1150.0);
    let storm_skin = storm * (1.0 - w.x) * (0.25 * g.parent).min(1350.0);
    let skin = fair_skin + storm_skin;
    let edge_lift = 0.12 * storm * storm_skin;
    if e - r - most + offset >= widest && s.above >= bed + edge_lift + 0.65 * skin + soft.max(1.5 * skin) {
        return (1.0, hs);
    }
    let basal_width = 1600.0 * (1.0 - upper.max(side));
    let base_width = lw.max(basal_width + (240.0 - basal_width) * storm);
    let mut n = spread((reads.base)(base_width, 1.0, Vec3::ZERO), u);
    let deck_foot = storm * (1.0 - w.x) * (1.0 - smoothstep(0.30, 0.65, h));
    if deck_foot > 0.0 {
        let bend = Vec3::new(g.deck_relief.y, n - 0.5, g.deck_relief.x);
        let offset = Vec3::new(0.41, 0.37, 0.18) + 0.10 * bend;
        let local = spread((reads.base)(base_width, 3.0, offset), u);
        n = (n + deck_foot * 0.90 * (local - 0.5)).clamp(0.0, 1.0);
    }
    let lower_edge = (bed + edge_lift + skin * (0.65 - n)).max(0.0);
    let height_in_bank = s.above - lower_edge;
    let pocket = (storm * (1.0 - w.x)).max(0.45 * mixed) * (1.0 - smoothstep(0.20, 0.65, n));
    let water = 1.0 - 0.76 * pocket * (1.0 - smoothstep(0.0, (1.5 * skin).max(1.0), height_in_bank));
    let bottom = water * smoothstep(0.0, soft, height_in_bank);
    let cut = r * (1.0 - (n + (0.5 - n) * core));
    let surface = |delta: f32| e - (cut + delta).max(0.08 * r) + offset;
    if surface(-most) + reach <= 0.0 {
        return (0.0, hs);
    }
    if surface(most) >= widest || amp <= 0.0 {
        return (bottom * smoothstep(0.0, ramp, surface(0.0)), hs);
    }
    let ew = if s.eroded && s.detail && u.dshape.z > 0.0 { lw * (1.0 + s.wisp_g) } else { lw };
    let keep = keeps(ew, u);
    let mut ep = [0.5f32; 3];
    if keep[0] + keep[1] + keep[2] > 0.0 {
        let ev = (reads.erode)(ew);
        ep = ev.map(|x| (0.25 + 0.5 * (x - u.nerode.x) / (u.nerode.y - u.nerode.x).max(1e-3)).clamp(0.0, 1.0));
    }
    let x: [f32; 3] = std::array::from_fn(|k| 0.5 + keep[k] * (ep[k] - 0.5));
    let mut displacement = 0.0;
    for k in 0..3 {
        displacement += (0.5 - x[k]) * c[k];
    }
    let cross_band = 2.5 * (x[1] - x[0]).abs() + 1.4 * (x[2] - x[1]).abs();
    let tear = keep[0] * cross_band.min(1.0) * (0.35 + 0.65 * x[2]);
    let fringe = gr * amp * tear * c[1];
    let f = surface(gr * amp * (displacement + tear * c[1]));
    let dense = smoothstep(0.0, ramp + 0.5 * fringe, f);
    let veil = 0.16 * tear * smoothstep(0.0, ramp, f + 0.70 * fringe);
    let tuft = keep[0] * smoothstep(0.52, 0.78, ep[0]) * smoothstep(0.12, 0.42, x[1]) * (1.0 - smoothstep(0.60, 0.82, x[1]));
    let tip = 0.26 * tuft * (smoothstep(0.0, ramp, f + reach * tuft) - smoothstep(0.0, ramp, f));
    (bottom * dense.max(veil.max(tip)), hs)
}

/// The lean model's extinction per unit at density 1 for the types `w`, times DENSITY (march
/// `vc_m2_sigma`).
pub fn sigma(w: Vec3, u: &CloudVolumeUniform) -> f32 {
    by_type(w, SIGMA) * u.march.z
}

/// The lean model's sample (march `vc_m2_shape`): (the shape including the base's fade, the extinction per
/// unit at shape 1, the shading's height share), blending the unresolved thin cover (the weather table's
/// extinction over a sheet) where `res` is under 1. `d` is the density result at `above`.
/// `water_content` scales the extinction of the resolved material.
pub fn shape(col: &Column, above: f32, d: (f32, f32), water_content: f32, u: &CloudVolumeUniform) -> (f32, f32, f32) {
    let fade = smoothstep(0.0, u.profile.x.max(1.0), above);
    let amount = u.scale.w.clamp(0.0, 1.0);
    let deck = smoothstep(0.55, 0.75, amount);
    let bank_mass = 0.72 * (col.over + u.field.y) + 0.28 * col.vigour + 0.65 * col.deck_relief.y;
    let local_water = 0.23 + 2.12 * smoothstep(0.28, 0.74, bank_mass);
    let front_mass = 0.48 + 0.53 * (col.over + u.field.y - 0.5) + 0.20 * (col.vigour - 0.5) + 0.68 * col.deck_relief.x;
    let front_water = 0.48 + 1.88 * smoothstep(0.25, 0.73, front_mass);
    let front_cover = 0.24 + 0.76 * smoothstep(0.30, 0.52, col.over + u.field.y);
    let water = (0.88 * local_water).max(front_water * front_cover);
    let fair = 1.0 + col.fair_water_excess;
    let sg = sigma(col.types, u) * (fair + (water - fair) * deck) * water_content;
    let (shape, h) = if col.over > 0.0 { d } else { (0.0, 0.0) };
    if col.res >= 1.0 {
        if shape <= 0.0 {
            return (0.0, sg, 0.0);
        }
        return (shape * fade, sg, h);
    }
    let sheet_h = u.field.w.max(1.0);
    let sheet = if above < sheet_h { u.profile.y.clamp(0.0, 1.0) * crate::volumetric_clouds::profile_at(above / sheet_h, col.types) } else { 0.0 };
    let ks = if amount > 0.0 { crate::volumetric_clouds::table_depth_of(&table(u), amount) / sheet_h * u.march.z } else { 0.0 };
    let cr = col.res * shape * sg;
    let cs = (1.0 - col.res) * sheet * ks;
    let k = ks + (sg - ks) * col.res;
    if cr + cs <= 0.0 || k <= 0.0 {
        return (0.0, k, 0.0);
    }
    ((cr + cs) / k * fade, k, (cr * h + cs * (above / sheet_h)) / (cr + cs))
}

/// The uniform's extinction table (march `vc_table`'s `cal_k0`, `cal_k1`).
fn table(u: &CloudVolumeUniform) -> [f32; 8] {
    [u.cal_k0.x, u.cal_k0.y, u.cal_k0.z, u.cal_k0.w, u.cal_k1.x, u.cal_k1.y, u.cal_k1.z, u.cal_k1.w]
}

/// The vertical dimensions used by both the fair lobes and their optical path.
pub struct FairProfile {
    pub bank: f32,
    pub crown: f32,
    pub height: f32,
    pub depth_scale: f32,
    pub plume_scale: f32,
}

pub fn plume_height(height: f32, parent: f32, scale: f32) -> f32 {
    height + (height - 0.28 * parent).max(0.0) * (scale - 1.0)
}

pub fn open_bank_weight(u: &CloudVolumeUniform) -> f32 {
    1.0 - smoothstep(0.35, 0.55, u.scale.w)
}

pub fn mixed_sheet(u: &CloudVolumeUniform) -> f32 {
    4.0 * u.mix.x * (1.0 - u.mix.x) * open_bank_weight(u)
}

/// The sheet's share retained under the local lobes (march `vc_m2_material`).
pub fn bank_water(cover: f32, puff_density: f32, mixed: f32) -> f32 {
    let condensed = smoothstep(0.025, 0.48, puff_density);
    let dry_water = 0.015 + 0.40 * cover;
    let shallow_water = dry_water + (1.0 - dry_water) * condensed;
    1.0 + (shallow_water - 1.0) * mixed
}

/// Density and extinction-weighted height for a mixed bank (march `vc_m2_material`).
pub fn mixed_material(bank: (f32, f32), puff: (f32, f32), cover: f32, blend: f32, s: &Sample, pwidth: f32, u: &CloudVolumeUniform) -> (f32, f32) {
    let mixed = mixed_sheet(u);
    let attached = smoothstep(0.0, 0.25, blend);
    let mut lobe_share = blend;
    if s.eroded {
        let across = (s.width * u.dshape.w).max(1.0);
        let shoulder = 1.0 / (32.0 * u.nscale.x).max(1e-8);
        let placement_across = (s.width * (1.0 + (u.dshape.w - 1.0) * mixed)).max(1.0);
        let aspect = pwidth / placement_across;
        let extra_span = across * (aspect * aspect - 1.0).max(0.0);
        let clustered = 0.90 * mixed * attached * smoothstep(0.09 * shoulder, 0.31 * shoulder, extra_span);
        lobe_share = blend + (1.0 - blend) * clustered;
    }
    let bank_density = (1.0 - lobe_share) * bank_water(cover, puff.0, mixed * attached) * bank.0;
    let puff_density = lobe_share * puff.0;
    let total = bank_density + puff_density;
    (total, (bank_density * bank.1 + puff_density * puff.1) / total.max(1e-8))
}

pub fn fair_profile(col: &Column, g: &Geometry, u: &CloudVolumeUniform) -> FairProfile {
    let bank = smoothstep(0.012, 0.20, col.over);
    let growth = 0.58 * bank * smoothstep(0.32, 1.30, col.fair_water_excess);
    let crown = g.parent + (g.extent - g.parent) * growth;
    let radius = 1.24 * col.over.max(0.0) / (u.field.x * u.map.x).max(1e-9);
    let permitted = 320.0 + 0.43 * radius;
    let depth_scale = 1.0 + open_bank_weight(u) * (1.0 - col.developed_bank) * ((permitted / crown.max(1.0)).min(1.0) - 1.0);
    let water = smoothstep(0.80, 1.38, col.fair_water_excess);
    let plume_scale = 1.0 + open_bank_weight(u) * col.developed_bank * (0.42 + 0.58 * water - 1.0);
    FairProfile { bank, crown, height: g.parent * (0.53 + 0.47 * bank), depth_scale, plume_scale }
}

pub fn base_onset(base_humidity: f32, turbulence: f32, u: &CloudVolumeUniform) -> f32 {
    let filament = smoothstep(-0.22, 0.20, turbulence);
    let humidity = 0.52 * base_humidity + 0.48 * filament;
    let dry = 1.0 - smoothstep(0.25, 0.76, humidity);
    (0.025 + 0.305 * dry) * open_bank_weight(u)
}

/// The light's vertical estimate per unit height share (march `vc_incident`): the
/// sample's extinction per unit `k` over the column's estimated height, [`FILL`] of it holding cloud.
pub fn shading_column(k: f32, g: &Geometry, col: &Column, above: f32, h: f32, lobe_weight: f32, u: &CloudVolumeUniform) -> f32 {
    let res = col.res;
    let length = if res < 1.0 && h > 0.0 { above / h } else { light_height(g, above) };
    let length = length.max(1.0);
    let shelter = if h > 0.0 { length + lobe_weight * res * (above / h.max(0.08) - length) } else { length };
    let fair_path = lobe_weight * res * open_bank_weight(u) * smoothstep(0.0, 0.02, g.cover);
    let occupied = if fair_path > 0.0 {
        let p = fair_profile(col, g, u);
        let onset = base_onset(col.base_humidity, 0.0, u) * p.height;
        let height = light_height(g, above / p.depth_scale);
        p.depth_scale * (plume_height(height, g.parent, p.plume_scale) - onset)
    } else { shelter };
    k * FILL * (shelter + fair_path * (occupied.max(1.0) - shelter))
}

/// Whether a point `above` units over the flat base may hold cloud (march `vc_m2_support`).
pub fn support(col: &Column, above: f32, u: &CloudVolumeUniform) -> bool {
    if above <= 0.0 || above >= u.slab.y.max(1.0) {
        return false;
    }
    let sheet = col.res < 1.0 && above < u.field.w.max(1.0) && u.profile.y > 0.0;
    (col.over > 0.0 && above < extent(col, u)) || sheet
}

/// Half the finest erosion band a cast width `width` keeps, over the wisps' most stretch (march
/// `vc_m2_erosion_bound`).
pub fn erosion_bound(width: f32, u: &CloudVolumeUniform) -> f32 {
    if u.nscale.w <= 0.0 {
        return 1.0e9;
    }
    let stretch = 1.0 + crate::volumetric_clouds_wisp::WISP_AMP * WISP_GRAD * u.dshape.z.max(0.0);
    cells(u).iter().filter(|c| width < 0.5 * **c).fold(1.0e9, |b, c| b.min(0.5 * c / stretch))
}

/// Half the narrowest surface transition at a cast width (march `vc_m2_edge_bound`).
pub fn edge_bound(width: f32) -> f32 {
    0.5 * EDGE[0].min(EDGE[1]).min(EDGE[2]).max(2.0 * width.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volumetric_clouds::{uniform, VolumeInputs, REFERENCE_HEIGHT, SHEET};

    fn march() -> String {
        include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n")
    }

    /// The uniform at amount `a` and the given wisps (the relief and the erosion at 1).
    fn lean_uniform(a: f32, wisp_aid: Option<f32>) -> CloudVolumeUniform {
        uniform(&VolumeInputs { domain: REFERENCE_HEIGHT, horizon: 400_000.0, steps: 28, density: 1.0, amount: a, wisp_aid, ..Default::default() })
    }

    fn cumulus(over: f32) -> Column {
        Column { developed_bank: 0.0, base_humidity: 0.5, fair_water_excess: 0.0, deck_relief: Vec2::ZERO, head: 0.0, over, types: Vec3::Y, vigour: 0.5, res: 1.0 }
    }

    #[test]
    fn a_drier_fair_base_shortens_the_occupied_light_path() {
        let mut u = lean_uniform(0.32, None);
        let wet = Column { base_humidity: 1.0, ..cumulus(0.15) };
        let dry = Column { base_humidity: 0.0, ..wet };
        let g = geometry(&wet, &u);
        let p = fair_profile(&wet, &g, &u);
        let above = 0.4 * p.depth_scale * p.height;
        let optical = |col: &Column, weather: &CloudVolumeUniform| shading_column(SIGMA[1], &g, col, above, 0.4, 1.0, weather);
        assert_eq!(base_onset(1.0, 0.0, &u), 0.025);
        let difference = base_onset(0.0, 0.0, &u) - base_onset(1.0, 0.0, &u);
        assert!(difference > 0.30 && difference < 0.306);
        let removed = difference * p.height * p.depth_scale;
        assert!((optical(&wet, &u) - optical(&dry, &u) - SIGMA[1] * FILL * removed).abs() < 1e-5);
        u.scale.w = 0.46;
        let partly_dry = optical(&wet, &u) - optical(&dry, &u);
        assert!(partly_dry > 0.0 && partly_dry < SIGMA[1] * FILL * removed);
        u.scale.w = 0.55;
        assert_eq!(base_onset(0.0, 0.0, &u), 0.0);
        assert_eq!(optical(&dry, &u), optical(&wet, &u));
    }

    #[test]
    fn a_developed_fair_bank_has_a_shared_base_and_water_limited_crowns() {
        let mut u = lean_uniform(0.32, None);
        let weak = Column { developed_bank: 1.0, fair_water_excess: 0.70, ..cumulus(0.20) };
        let fed = Column { fair_water_excess: 1.40, ..weak };
        let g = geometry(&weak, &u);
        let scale = fair_profile(&weak, &g, &u).plume_scale;
        assert!((scale - 0.42).abs() < 1e-6);
        assert_eq!(fair_profile(&fed, &g, &u).plume_scale, 1.0);
        assert_eq!(fair_profile(&Column { developed_bank: 0.0, ..weak }, &g, &u).plume_scale, 1.0);
        for share in [0.0, 0.18, 0.28, 0.55, 1.0, 1.2] {
            let original = g.parent * share;
            let occupied = plume_height(original, g.parent, scale);
            if share <= 0.28 { assert_eq!(occupied, original); } else { assert!(occupied < original); }
            assert!((plume_height(occupied, g.parent, 1.0 / scale) - original).abs() < 1e-6 * g.parent);
        }
        let optical = |col: &Column, weather: &CloudVolumeUniform| shading_column(SIGMA[1], &g, col, 0.4 * g.parent, 0.4, 1.0, weather);
        assert!(optical(&weak, &u) < optical(&fed, &u));
        u.scale.w = 0.47;
        let transition = fair_profile(&weak, &g, &u).plume_scale;
        assert!(transition > scale && transition < 1.0);
        u.scale.w = 0.55;
        assert_eq!(fair_profile(&weak, &g, &u).plume_scale, 1.0);
        assert_eq!(optical(&weak, &u), optical(&fed, &u));
    }

    /// The base texture's raw value whose spread value is `n`.
    fn raw_base(n: f32, u: &CloudVolumeUniform) -> f32 {
        u.nlobe.x + (n - 0.25) / 0.5 * (u.nlobe.y - u.nlobe.x)
    }

    /// The erosion texture's raw value whose spread value is `ep`.
    fn raw_erode(ep: f32, u: &CloudVolumeUniform) -> f32 {
        u.nerode.x + (ep - 0.25) / 0.5 * (u.nerode.y - u.nerode.x)
    }

    /// [`density`] with fixed spread values (the base `n`, every erosion band `ep`), and the reads made
    /// (base, erosion).
    fn run(g: &Geometry, w: Vec3, s: &Sample, u: &CloudVolumeUniform, n: f32, ep: f32) -> ((f32, f32), [u32; 2]) {
        let (b, e) = (raw_base(n, u), raw_erode(ep, u));
        let made = std::cell::Cell::new([0u32; 2]);
        let mut base = |_, _, _| {
            let mut m = made.get();
            m[0] += 1;
            made.set(m);
            b
        };
        let mut erode = |_w: f32| {
            let mut m = made.get();
            m[1] += 1;
            made.set(m);
            [e; 3]
        };
        let got = density(g, w, s, u, &mut Reads { base: &mut base, erode: &mut erode });
        (got, made.get())
    }

    fn view(above: f32, width: f32) -> Sample {
        Sample { above, width, fw: width, eroded: true, detail: true, wisp_g: 0.0 }
    }

    #[test]
    fn the_mixed_bank_filters_view_texels_across_the_step_and_light_at_its_cell() {
        let mut u = lean_uniform(0.07, None);
        u.mix = Vec3::new(0.5, 0.5, 0.0).extend(0.0);
        u.lcell.x = 800.0;
        let types = u.mix.truncate();
        let g = geometry(&Column { types, ..cumulus(0.07) }, &u);
        let read_width = |weather: &CloudVolumeUniform, fw: f32, eroded: bool| {
            let sample = Sample { fw, eroded, ..view(0.60 * g.parent, 600.0) };
            let mut used = None;
            let mut base = |width, frequency, _| {
                if frequency == 1.0 { used = Some(width); }
                raw_base(0.5, weather)
            };
            density(&g, types, &sample, weather, &mut Reads { base: &mut base, erode: &mut |_| [raw_erode(0.5, weather); 3] });
            used.expect("the bank margin samples the base texture")
        };
        for side in [1.0, 2.0, 3.0] {
            u.dshape.w = 1.0 / side;
            for step in [600.0_f32, 1200.0] {
                let expected = (600.0 / side * step).sqrt();
                assert!((read_width(&u, step, true) - expected).abs() < 0.001);
                assert_eq!(read_width(&u, step, false), 1600.0);
            }
        }
        for (sheet, amount) in [(0.0, 0.29), (1.0, 0.07), (0.5, 0.73)] {
            u.scale.w = amount;
            u.mix = Vec3::new(sheet, 1.0 - sheet, 0.0).extend(0.0);
            assert_eq!(read_width(&u, 1200.0, true), 1200.0);
        }
    }

    /// [`BASE_GRAD`] is the bake's: the base noise's spread value's mean gradient per repeat where it is
    /// 0.35..0.65 (central differences on level 0 of the CPU copy), within 10 %.
    #[test]
    fn the_base_gradient_is_the_bakes() {
        use crate::volumetric_clouds::NOISE_LOBE;
        let noise = crate::cloud_noise::production();
        let n = noise.base.size as usize;
        let at = |x: usize, y: usize, z: usize| {
            let b = f32::from(noise.base.bytes[((z % n) * n + (y % n)) * n + (x % n)]) / 255.0;
            (0.25 + 0.5 * (b - NOISE_LOBE[0]) / (NOISE_LOBE[1] - NOISE_LOBE[0])).clamp(0.0, 1.0)
        };
        let (mut sum, mut count) = (0.0f64, 0usize);
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    if !(0.35..=0.65).contains(&at(x, y, z)) {
                        continue;
                    }
                    let half = n as f32 / 2.0;
                    let g = Vec3::new(at(x + 1, y, z) - at(x + n - 1, y, z), at(x, y + 1, z) - at(x, y + n - 1, z), at(x, y, z + 1) - at(x, y, z + n - 1)) * half;
                    sum += f64::from(g.length());
                    count += 1;
                }
            }
        }
        let grad = (sum / count as f64) as f32;
        assert!((grad - BASE_GRAD).abs() <= 0.1 * BASE_GRAD, "measured {grad} against {BASE_GRAD}");
    }

    /// The march's lean model is this mirror's (text pins).
    #[test]
    fn the_lean_mirror_matches_the_march() {
        let src = march();
        for line in [
            "const VC_M2_COVER: f32 = 0.12;",
            "const VC_M2_EXTENT: vec3<f32> = vec3<f32>(0.11, 0.14, 0.48);",
            "const VC_M2_RELIEF: vec3<f32> = vec3<f32>(0.25, 0.75, 0.6);",
            "const VC_M2_EDGE: vec3<f32> = vec3<f32>(600.0, 240.0, 300.0);",
            "const VC_M2_SIGMA: vec3<f32> = vec3<f32>(1.5e-3, 2.5e-3, 3.0e-3);",
            "const VC_M2_ERODE_TYPE: vec3<f32> = vec3<f32>(0.3, 1.0, 0.8);",
            "const VC_M2_ERODE_FREQ: f32 = 2.0;",
            "const VC_M2_BILLOW: f32 = 1.15;",
            "const VC_M2_GRAD_LEAST: f32 = 1.0e-4;",
            "const VC_M2_FOOT: f32 = 0.36;",
            "const VC_M2_FILL: f32 = 0.6;",
            "const VC_M2_WISP_GRAD: f32 = 2.04e-3;",
            "const VC_M2_FRAY: f32 = 0.10;",
            "return smoothstep(0.0, VC_M2_COVER, col.over);",
            "let scale = v.scale.x + (v.scale.y - v.scale.x) * pow(clamp(col.vigour, 0.0, 1.0), max(v.scale.z, 1e-3));",
            "return clamp(dot(col.types, VC_M2_EXTENT) * depth * scale * (1.0 + 0.55 * bank + 0.20 * core), v.field.w, depth);",
            "return dot(w, VC_M2_RELIEF) * max(v.nscale.z, 0.0);",
            "return max(r * VC_BASE_GRAD * v.nscale.x, VC_M2_GRAD_LEAST);",
            "const VC_BASE_GRAD: f32 = 30.0;",
            "return clamp(0.25 + 0.5 * (b - v.nlobe.x) / max(v.nlobe.y - v.nlobe.x, 1e-3), 0.0, 1.0);",
            "return VC_M2_BILLOW * max(v.nscale.w, 0.0) * dot(w, VC_M2_ERODE_TYPE);",
            "let need = min(tau / max(c, 1e-6), a.x);",
            "return top * min(c / tau, 1.0);",
            "let tau = 0.5 * r;",
            "return VC_M2_ERODE_FREQ * vc_erosion_coord(pos, yb, v);",
            "let e = mix(c, column.crown_cover, upper) * clamp(vc_profile(h, w, v), 0.0, 1.0) * (1.0 - neck);",
            "let texel = mix(1.0, v.dshape.w, vc_m2_mixed_sheet(v));",
            "return mix(footprint * texel, projected, deck);",
            "let step_width = max(width, fw);",
            "let sample_area = sqrt(max(1.0, width * v.dshape.w) * step_width);",
            "lw = mix(step_width, sample_area, vc_m2_mixed_sheet(v));",
            "lw = 2.0 * max(width, vc_light_cell(v));",
            "let ramp = max(intrinsic, select(min(footprint, 0.42), footprint, eroded));",
            "let amp = (1.0 - storm) * vc_m2_billow(w, v) * max(upper, side) * min(1.0, 0.12 + 2.0 * (1.0 - e));",
            "let storm_skin = storm * (1.0 - w.x) * min(0.25 * parent, 1350.0);",
            "let skin = fair_skin + storm_skin;",
            "let edge_lift = 0.12 * storm * storm_skin;",
            "if e - r - most + offset >= widest && above >= bed + edge_lift + 0.65 * skin + max(soft, 1.5 * skin) {",
            "let lower_edge = max(0.0, bed + edge_lift + skin * (0.65 - n));",
            "let bottom = water * smoothstep(0.0, soft, height_in_bank);",
            "bank += max(basin * gate * smoothstep(0.015, 0.07, pit), min(basin, deficit) * lip);",
            "let r = column.m2.w * (1.0 - 0.60 * core);",
            "crown_cover *= 1.0 - 0.44 * smoothstep(0.50, 1.0, m2.x) * smoothstep(0.0, 0.28, -col.head);",
            "let form = vec3<f32>(0.5) - x;",
            "let f = e - max(cut + g * amp * (dot(form, cells) + tear * cells.y), least) + offset;",
            "return vec2<f32>(bottom * max(dense, max(veil, tip)), hs);",
            "const VC_M2_SHEAR: f32 = 0.10;",
            "return pos + VC_M2_SHEAR * vec3<f32>(pos.z, 0.0, pos.x);",
            "let pw = width * select(1.0 + VC_M2_SHEAR, 1.04, fair);",
            "let weight = select(1.0 - cross, cross, fair);",
            "bank += weight * vc_m2_bank(placed, max(pw, min_width), storm, fair, v);",
            "col.over = bank.x - v.field.y;",
            "col.developed_bank = bank.y;",
            "let growth = 0.58 * bank * smoothstep(0.32, 1.30, column.fair_water_excess);",
            "column.base_humidity = moisture;",
            "let h = raw_height - max(0.52 - raw_height, 0.0) * onset / (0.52 - onset);",
            "ls.column.y = mix(ls.column.y, d.y * VC_M2_FILL * max(occupied, 1.0), fair_path);",
            "bank = mix(bank, max(small_bank, v.field.y + mature_excess), weight);",
            "developed = weight * smoothstep(-0.015, 0.13, mature_excess);",
            "vc_m2_open_bank_weight(v) * (1.0 - column.col.developed_bank)",
            "let shoulder = vc_m2_caps(uv, 16u, 0.37, width, v);",
            "let head = vc_m2_caps(uv, 32u, 0.0, width, v);",
            "let bud = vc_m2_caps(tuft_uv, 64u, 0.37, 1.6181 * width, v);",
            "return 4.0 * sheet * (1.0 - sheet) * vc_m2_open_bank_weight(v);",
            "let attachment_width = 0.045 * vc_m2_mixed_sheet(v);",
            "crown_head *= smoothstep(0.0, attachment_width, col.over);",
            "var crown_cover = smoothstep(0.0, VC_M2_COVER, col.over + 0.18 * crown_head);",
            "+ mixed_sheet * dry_base * min(0.16 * parent, 600.0);",
            "let pocket = max(storm * (1.0 - w.x), 0.45 * mixed_sheet) * (1.0 - smoothstep(0.20, 0.65, n));",
            "let rim_width = mix(0.012, 0.045, vc_m2_mixed_sheet(v));",
            "let condensed = smoothstep(0.025, 0.48, puff.x);",
            "let shallow_water = mix(0.015 + 0.40 * column.m2.x, 1.0, condensed);",
            "let attached = smoothstep(0.0, 0.25, blend);",
            "let bank_water = mix(1.0, shallow_water, mixed_sheet * attached);",
            "var lobe_share = blend;\n    if eroded {",
            "let shoulder = 1.0 / max(32.0 * v.nscale.x, 1e-8);",
            "let placement_across = max(width * mix(1.0, v.dshape.w, mixed_sheet), 1.0);",
            "let aspect = pwidth / placement_across;",
            "let extra_span = across * max(aspect * aspect - 1.0, 0.0);",
            "let clustered = 0.90 * mixed_sheet * attached * smoothstep(0.09 * shoulder, 0.31 * shoulder, extra_span);",
            "lobe_share = mix(blend, 1.0, clustered);",
            "let shares = vec2<f32>((1.0 - lobe_share) * bank_water * bank.x, lobe_share * puff.x);",
            "col.head = fair * attachment * (max(nested, 0.0) + 0.40 * min(nested, 0.0));",
            "let h = vc_m2_stretch(above, column.m2.y, parent, VC_M2_FOOT * parent) / parent;",
            "let base_width = max(lw, mix(1600.0 * (1.0 - max(upper, side)), 240.0, storm));",
            "let deck_foot = storm * (1.0 - w.x) * (1.0 - smoothstep(0.30, 0.65, h));",
            "let bend = vec3<f32>(column.deck_relief.y, n - 0.5, column.deck_relief.x);",
            "let detail_uv = 3.0 * uv + vec3<f32>(0.41, 0.37, 0.18) + 0.10 * bend;",
            "vc_level(base_width, v.nlevel.x, 3.0 * v.nscale.x, v.nlevel.y)).r, v);",
            "n = clamp(n + deck_foot * 0.90 * (local - 0.5), 0.0, 1.0);",
            "let water = max(0.88 * local_water, front_water * front_cover);",
            "var sigma = vc_m2_sigma(col.types, v) * mix(1.0 + column.fair_water_excess, water, deck);",
            "sigma *= exp2(lower_bank * (0.45 * broad + 0.72 * local));",
            "return dot(w, VC_M2_SIGMA) * v.march.z;",
            "return vec4<f32>((cr + cs) / k * fade, k, amount, (cr * h + cs * (above / sheet_h)) / (cr + cs));",
            "ls.column.y = d.y * VC_M2_FILL * max(vc_m2_light_height(column, above), 1.0);",
            "ls.column.y = d.y * VC_M2_FILL * max(above / d.w, 1.0);",
            // The dispatch: the flat base before the rejection, the lean field and support, the bounds.
            "let above = yb - v.slab.x;\n    if above <= 0.0 || (col.edge <= 0.0 && col.res >= 1.0) {",
            "return vc_m2_shape(pos, yb, above, column, width, pwidth, nb, ns, ne, eroded, detail, fw, v);",
            "let m2 = vc_m2_material(column, above, pos, yb, width, pwidth, fw, eroded, detail, nb, ns, ne, v);",
            "let m2 = vc_m2_geometry(col, v);",
            "return (col.edge > 0.0 && above < vc_m2_extent(col, v)) || sheet2;",
            "let inside = in_domain && vc_m2_support(col, yb - v.slab.x, v);",
            "vc_m2_erosion_bound(spread * t0, v)), vc_m2_edge_bound(spread * t0));",
            "vc_m2_erosion_bound(w, v)), vc_m2_edge_bound(w));",
            "return min(max(requested, VC_NEAR_LEAST), n);",
            // The previews show the body's own quantities.
            "g = 0.5 + 0.5 * vc_m2_cover(col);",
            "g = select(0.0, vc_m2_extent(col, v) / max(v.slab.y, 1.0), col.edge > 0.0);",
            "return vec3<f32>(0.9 * clamp(d.w, 0.0, 1.0));",
        ] {
            assert!(src.contains(line), "the march no longer has `{line}`: the mirror must follow");
        }
        assert_eq!((COVER, EXTENT, RELIEF, EDGE, SIGMA, ERODE_TYPE), (0.12, [0.11, 0.14, 0.48], [0.25, 0.75, 0.6], [600.0, 240.0, 300.0], [1.5e-3, 2.5e-3, 3.0e-3], [0.3, 1.0, 0.8]));
        assert_eq!((ERODE_FREQ, BILLOW, GRAD_LEAST, FOOT, FILL, FRAY), (2.0, 1.15, 1.0e-4, 0.36, 0.6, 0.10));
        // The column material has a base read, lower-deck detail read and erosion read.
        // The fair lobe material uses five filtered base reads and one erosion read.
        let lobes = &src[src.find("fn vc_m2_lobes(").unwrap()..];
        let lobes = &lobes[..lobes.find("\n}\n").unwrap()];
        assert_eq!(lobes.matches("textureSampleLevel(").count(), 6);
        assert!(!lobes.contains("v.dshape"), "WISP moves nothing in the fair lobes");
        assert_eq!(lobes.matches("vc_m2_lobe_field(").count(), 1);
        for term in ["let large = vc_m2_lobe_field(uv, h, 8u, 0.70", "let profile = vc_m2_fair_profile(column, v);",
            "let cap = min(h - max(0.30, 0.68 * shoulder), small_top - h);", "let fine_uv = 6.0 * uvw + VC_M2_FRAY * erosion.yzx;",
            "dot(erosion, vec3<f32>(0.46, 0.21, 0.10)) + 0.20 * rim + 0.12 * finest;", "foot * density * boundary"] {
            assert!(lobes.contains(term), "fair lobe sampling or support changed: {term}");
        }
        let lean = &src[src.find("fn vc_m2_density(").unwrap()..];
        let lean = &lean[..lean.find("\n}\n").unwrap()];
        assert_eq!(lean.matches("textureSampleLevel(").count(), 3);
        for part in ["vc_m1_turret", "vc_m1_body", "vc_tuft", "vc_m1_aspect", "vc_puff_density", "vc_m1_cauli", "families", "fair_shoulder", "vc_column(", "vc_vigour("] {
            assert!(!lean.contains(part), "the bank's density reads no `{part}`");
        }
        // The wisps act on the view's eroded path only.
        assert!(lean.contains("if eroded && detail && v.dshape.z > 0.0 {") && lean.matches("vc_wisp(").count() == 1);
    }

    /// The step rule: the narrowest transition, the erosion read's bands over the wisps' stretch; no PUFF
    /// unless asked for.
    #[test]
    fn the_step_rule_follows_the_surface_and_the_bands() {
        let u = lean_uniform(0.4, None);
        assert_eq!(u.kind.x, 0.0, "no PUFF unless asked for");
        assert_eq!(edge_bound(0.0), 0.5 * EDGE[1]);
        assert!((edge_bound(400.0) - 400.0).abs() < 1e-3);
        let c = cells(&u);
        assert!((c[0] - 2_000.0).abs() < 1e-2 && (c[2] - 500.0).abs() < 1e-2, "{c:?}");
        let stretch = 1.0 + crate::volumetric_clouds_wisp::WISP_AMP * WISP_GRAD * u.dshape.z;
        assert!((erosion_bound(0.0, &u) - 250.0 / stretch).abs() < 1e-3);
        assert!(erosion_bound(3_000.0, &u) > 1e8, "every band gone");
        assert!(WISP_GRAD * crate::volumetric_clouds_wisp::WISP_AMP >= crate::volumetric_clouds_wisp::wisp_bounds(1.0).1, "the bound's stretch covers the wisps'");
    }

    /// Zero stays zero and the reads: no cover, a point over the column's height or the profile's zero gives
    /// no density and no read; deep inside, whole with no read; where the field is below zero at the base
    /// read, nothing after one read; a sample the erosion cannot change, one read; near the surface, two;
    /// the light reads the erosion only where its footprint keeps a band; bounded and finite throughout.
    #[test]
    fn zero_stays_zero_and_the_reads_are_counted() {
        let u = lean_uniform(0.4, None);
        let g = geometry(&cumulus(0.3), &u);
        assert_eq!(g.cover, 1.0);
        for (over, above) in [(0.0f32, 1_000.0f32), (-0.1, 1_000.0), (0.3, g.extent), (0.3, g.extent + 10.0)] {
            let gg = geometry(&cumulus(over), &u);
            let (d, reads) = run(&gg, Vec3::Y, &view(above, 64.0), &u, 0.5, 0.5);
            assert_eq!((d.0, reads), (0.0, [0, 0]), "over {over} above {above}");
        }
        // Cover 1 on the profile's whole foot at SHAPE 0.25 (relief 0.19): whole whatever the noise.
        let mut low = u;
        low.nscale.z = 0.25;
        let gl = geometry(&cumulus(0.3), &low);
        let (d, reads) = run(&gl, Vec3::Y, &view(0.1 * gl.extent, 64.0), &low, 0.0, 0.0);
        assert_eq!((d.0, reads), (1.0, [0, 0]));
        // At SHAPE 1 a saturated column's crown reads the base: its lobes reach the surface.
        let (_, reads) = run(&g, Vec3::Y, &view(0.8 * g.extent, 64.0), &u, 0.0, 0.0);
        assert_eq!(reads[0], 1);
        // Half cover, mid height: low noise, nothing after one read; high noise, near the surface, two.
        let half = geometry(&cumulus(0.06), &u);
        let mut no_erode = u;
        no_erode.nscale.w = 0.0;
        let (d, reads) = run(&half, Vec3::Y, &view(0.56 * half.extent, 64.0), &no_erode, 0.0, 0.5);
        assert_eq!((d.0, reads), (0.0, [1, 0]));
        // With signed relief the same sample can be reached by an outward head.
        let at_rim = view(0.56 * half.extent, 64.0);
        assert_eq!(run(&half, Vec3::Y, &at_rim, &u, 0.0, 0.5).1, [1, 1]);
        assert!(run(&half, Vec3::Y, &at_rim, &u, 0.0, 1.0).0 .0 > 0.9);
        let mut seen_two = false;
        for k in 0..=50 {
            let (d, reads) = run(&half, Vec3::Y, &view(0.56 * half.extent, 64.0), &u, k as f32 / 50.0, 0.5);
            assert!((0.0..=1.0).contains(&d.0));
            seen_two |= reads == [1, 1] && d.0 > 0.0 && d.0 < 1.0;
        }
        assert!(seen_two, "the surface's samples take the erosion read");
        // The light at a footprint wider than every band (filter 6,000): the mean, never the read.
        let far = Sample { eroded: false, ..view(0.3 * half.extent, 3_000.0) };
        for k in 0..=20 {
            let (dl, reads) = run(&half, Vec3::Y, &far, &u, k as f32 / 20.0, 0.0);
            let (dl2, _) = run(&half, Vec3::Y, &far, &u, k as f32 / 20.0, 1.0);
            assert_eq!((reads[1], dl), (0, dl2), "the light's erosion is its bands' mean");
        }
        // At the far level a broad footprint still sees an optically thick core.
        let core = Sample { eroded: false, ..view(0.30 * g.extent, 4_000.0) };
        assert!(run(&g, Vec3::Y, &core, &u, 0.5, 0.5).0 .0 > 0.95);
        // Never above 1, never negative, finite, whatever the noise and the keys.
        for relief_aid in [0.25f32, 1.0, 2.0, 4.0] {
            for erosion in [0.0f32, 1.0, 4.0] {
                let mut v = u;
                v.nscale.z = relief_aid;
                v.nscale.w = erosion;
                for types in [Vec3::X, Vec3::Y, Vec3::Z] {
                    for over in [0.001f32, 0.03, 0.06, 0.2] {
                        let g = geometry(&Column { developed_bank: 0.0, base_humidity: 0.5, fair_water_excess: 0.0, deck_relief: Vec2::ZERO, head: 0.0, over, types, vigour: 0.5, res: 1.0 }, &v);
                        for k in 0..40 {
                            let above = (k as f32 + 0.5) / 40.0 * g.extent;
                            for (n, ep) in [(0.0f32, 0.0f32), (0.3, 0.9), (0.7, 0.2), (1.0, 1.0)] {
                                let (d, _) = run(&g, types, &view(above, 64.0), &v, n, ep);
                                assert!(d.0.is_finite() && (0.0..=1.0).contains(&d.0) && (0.0..=1.0).contains(&d.1), "{d:?}");
                            }
                        }
                    }
                }
            }
        }
    }

    /// The surface is crisp: across the base noise the density goes from nothing to whole over the type's
    /// transition (240 units for cumulus near the camera, two filter widths far off: never narrower), so
    /// the inside is dense and the silhouette sharp; and the extinction per unit is the type's own, so
    /// 400 units of whole cumulus at DENSITY 1 are one optical depth, whatever the column's height.
    #[test]
    fn the_surface_is_crisp_and_dense() {
        let u = lean_uniform(0.4, Some(0.0));
        let mut no_erosion = u;
        no_erosion.nscale.w = 0.0;
        let half = geometry(&cumulus(0.06), &no_erosion);
        let above = 0.3 * half.extent;
        let e = half.cover * crate::volumetric_clouds::profile_at(above / half.extent, Vec3::Y);
        let r = half.relief;
        let gr = grad(r, &no_erosion);
        for width in [32.0f32, 64.0, 300.0] {
            // The noise where the field crosses zero, and how far the noise must move to make it whole.
            let n0 = 1.0 - e / r;
            let ramp = gr * EDGE[1].max(2.0 * width);
            let dn = ramp / r;
            let at = |n: f32| run(&half, Vec3::Y, &view(above, width), &no_erosion, n, 0.5).0 .0;
            assert_eq!(at(n0 - 1e-3), 0.0);
            assert!(at(n0 + dn + 1e-3) >= 0.999, "whole past the transition at width {width}");
            assert!(at(n0 + 0.5 * dn) > 0.3 && at(n0 + 0.5 * dn) < 0.7);
            // In world units: the noise's measured gradient makes the transition its width.
            let world = dn / (BASE_GRAD * u.nscale.x);
            assert!((world - EDGE[1].max(2.0 * width)).abs() < 1e-2 * world, "{world}");
        }
        for types in [Vec3::X, Vec3::Y, Vec3::Z] {
            assert!((sigma(types, &u) - by_type(types, SIGMA)).abs() < 1e-9, "DENSITY 1");
        }
        assert!((sigma(Vec3::Y, &u) * 400.0 - 1.0).abs() < 1e-6);
        let mut dense = u;
        dense.march.z = 2.0;
        assert!((sigma(Vec3::Y, &dense) - 2.0 * SIGMA[1]).abs() < 1e-9, "DENSITY multiplies it");
        // The light's vertical estimate (march `vc_incident`'s `lean`): the sample's extinction over the
        // column's estimated height, FILL of it cloud; a whole cumulus column is many optical depths deep.
        let wet_column = cumulus(0.3);
        let gw = geometry(&wet_column, &u);
        assert!((shading_column(SIGMA[1], &gw, &wet_column, 600.0, 0.1, 0.0, &u) - SIGMA[1] * FILL * gw.top * gw.extent).abs() < 1e-6);
        assert!(shading_column(SIGMA[1], &gw, &wet_column, 600.0, 0.1, 0.0, &u) > 5.0);
        let mut closing = u;
        closing.scale.w = 0.55;
        assert!((shading_column(SIGMA[1], &gw, &wet_column, 600.0, 0.1, 1.0, &closing) - SIGMA[1] * FILL * 6000.0).abs() < 1e-5);
        let empty = Column { res: 0.0, ..cumulus(-0.02) };
        let (_, k_sheet, h_sheet) = shape(&empty, 600.0, (0.0, 0.0), 1.0, &u);
        let column = shading_column(k_sheet, &geometry(&empty, &u), &empty, 600.0, h_sheet, 0.0, &u);
        assert!(k_sheet > 0.0 && (column - k_sheet * FILL * u.field.w).abs() < 1e-5);
        // The resolved sample's extinction is the type's, not the table over the column.
        let col = cumulus(0.06);
        let (s, k, _) = shape(&col, 3_000.0, (0.8, 0.4), 1.0, &u);
        assert!((k - SIGMA[1]).abs() < 1e-9 && (s - 0.8).abs() < 1e-6);
        // Unresolved: res 1 the resolved sample exactly, res 0 the sheet's, continuous between.
        let mut last = shape(&Column { res: 1.0, ..col }, 600.0, (0.8, 0.4), 1.0, &u);
        for i in (0..100).rev() {
            let res = i as f32 / 100.0;
            let got = shape(&Column { res, ..col }, 600.0, (0.8, 0.4), 1.0, &u);
            assert!((got.0 * got.1 - last.0 * last.1).abs() < 0.05 * last.0 * last.1 + 1e-7, "res {res}: {got:?} after {last:?}");
            last = got;
        }
        // Between, the two optical densities are weighed by the resolution: the resolved sample's (0.8 of
        // whole cumulus) and the sheet's (the share asked for times the profile at half a sheet, at the
        // weather table's depth over a sheet), and the height by what each holds.
        use crate::volumetric_clouds::{profile_at, sky_share, table_depth, SHEET};
        let resolved = 0.8 * SIGMA[1];
        let sheet_k = table_depth(0.4) / SHEET;
        let sheet = sky_share(0.4) * profile_at(0.5, Vec3::Y) * sheet_k;
        assert!(sheet > 1e-5 && (sheet / resolved - 1.0).abs() > 0.2, "two densities to tell apart: {sheet} {resolved}");
        for res in [0.0f32, 0.1, 0.25, 0.5, 0.75, 0.9] {
            let (s, k, h) = shape(&Column { res, ..col }, 0.5 * SHEET, (0.8, 0.4), 1.0, &u);
            let (cr, cs) = (res * resolved, (1.0 - res) * sheet);
            assert!((s * k / (cr + cs) - 1.0).abs() < 1e-4, "res {res}: {} against {}", s * k, cr + cs);
            assert!((k / (res * SIGMA[1] + (1.0 - res) * sheet_k) - 1.0).abs() < 1e-4, "res {res}: {k}");
            assert!((h - (cr * 0.4 + cs * 0.5) / (cr + cs)).abs() < 1e-4, "res {res}: {h}");
        }
        let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        let body = &src[src.find("fn vc_m2_shape(").unwrap()..];
        let body = &body[..body.find("\n}\n").unwrap()];
        for line in [
            "let ks = vc_extinction(amount, sheet_h, v);",
            "let cr = col.res * shape * sigma;",
            "let cs = (1.0 - col.res) * sheet * ks;",
            "let k = mix(ks, sigma, col.res);",
            "return vec4<f32>((cr + cs) / k * fade, k, amount, (cr * h + cs * (above / sheet_h)) / (cr + cs));",
        ] {
            assert_eq!(body.matches(line).count(), 1, "{line}");
        }
    }

    /// Relief by type: at SHAPE 1 the cumulus's base noise shapes its surface while a whole overcast sheet
    /// stays whole wherever its profile is high (the sheet's relief 0.25: no holes in an overcast deck
    /// under SHAPE up to 2), and the sheet's erosion is weaker.
    #[test]
    fn overcast_stays_whole_while_cumulus_is_shaped() {
        let u = lean_uniform(0.9, None);
        let deck = Column { developed_bank: 0.0, base_humidity: 0.5, fair_water_excess: 0.0, deck_relief: Vec2::ZERO, head: 0.0, over: 0.4, types: Vec3::X, vigour: 0.5, res: 1.0 };
        // At the same supported point a weak bank transmits more than a thick
        // bank; both still contain water. The shared cover remains continuous.
        let transmission = |field| {
            let col = Column { over: field - u.field.y, ..deck };
            let above = 600.0;
            assert!(support(&col, above, &u));
            let (density, k, _) = shape(&col, above, (1.0, 0.25), 1.0, &u);
            (-density * k * 1500.0).exp()
        };
        let (thin, thick) = (transmission(0.30), transmission(0.70));
        assert!(thick > 0.0 && thin < 1.0 && thin - thick > 0.35);
        // At equal placement, mesoscale relief changes the underside and optical
        // depth while both columns still carry dense cloud at mid-height.
        let patch = |relief| {
            let col = Column { over: 0.48 - u.field.y, deck_relief: relief, ..deck };
            let g = geometry(&col, &u);
            let at = |h| run(&g, Vec3::X, &view(h * g.parent, 200.0), &u, 0.5, 0.5).0.0;
            let (_, k, _) = shape(&col, 0.35 * g.parent, (at(0.35), 0.35), 1.0, &u);
            (at(0.08), at(0.35), k)
        };
        let (thin_base, thin_middle, thin_k) = patch(Vec2::splat(-0.35));
        let (thick_base, thick_middle, thick_k) = patch(Vec2::splat(0.35));
        assert!(thin_base < 0.01 && thick_base > 0.99);
        assert!(thin_middle > 0.99 && thick_middle > 0.99);
        assert!(thin_k > 0.0 && thick_k > 2.0 * thin_k);
        // A local pocket can shade a thin front without lowering its base.
        // A thick front still shades the column when local water is low.
        let (pocket_base, pocket_middle, pocket_k) = patch(Vec2::new(-0.35, 0.35));
        let (_, _, front_k) = patch(Vec2::new(0.35, -0.35));
        assert_eq!((pocket_base, pocket_middle), (thin_base, thin_middle));
        assert!(pocket_k > 2.0 * thin_k && front_k > 2.0 * thin_k);
        let nominal = sigma(deck.types, &u);
        assert!(thin_k < nominal && thick_k > nominal);
        let g = geometry(&deck, &u);
        for shape_aid in [1.0f32, 2.0] {
            let mut v = u;
            v.nscale.z = shape_aid;
            let gv = geometry(&deck, &v);
            for h in [0.05f32, 0.15, 0.3] {
                for n in [0.0f32, 0.5, 1.0] {
                    for ep in [0.0f32, 1.0] {
                        let (d, _) = run(&gv, Vec3::X, &view(h * gv.extent, 64.0), &v, n, ep);
                        assert!(d.0 > 0.9, "SHAPE {shape_aid}: the deck at {h} with noise {n} erosion {ep}: {d:?}");
                    }
                }
            }
        }
        // The cumulus at the same cover and height: the noise decides.
        let cu = geometry(&Column { types: Vec3::Y, ..deck }, &u);
        let lo = run(&cu, Vec3::Y, &view(0.6 * cu.extent, 64.0), &u, 0.0, 0.5).0 .0;
        let hi = run(&cu, Vec3::Y, &view(0.6 * cu.extent, 64.0), &u, 1.0, 0.5).0 .0;
        assert!(lo < 0.1 && hi > 0.9, "{lo} {hi}");
        assert!(billow(Vec3::X, &u) < billow(Vec3::Y, &u) && relief(Vec3::X, &u) < relief(Vec3::Y, &u));
        assert!(g.extent >= SHEET);
        let far = Sample { eroded: false, ..view(g.extent * 0.30, 4_000.0) };
        assert!(run(&g, Vec3::X, &far, &u, 0.5, 0.5).0 .0 > 0.95);
    }

    /// The erosion carves the surface without holing the body: at a surface sample the billowy form moves
    /// the surface by up to BILLOW of each kept band's cell (the largest cell spans 2,000 units,
    /// the smallest spans 500); the centred displacement fades to zero with the footprint (no shrink
    /// with distance); the inside stays whole for any erosion strength up to the key's most.
    #[test]
    fn the_erosion_carves_the_surface_not_the_body() {
        let u = lean_uniform(0.4, Some(0.0));
        let c = cells(&u);
        assert!((c[0] - 2_000.0).abs() < 0.01, "resolved head scale");
        let g = geometry(&cumulus(0.06), &u);
        let above = 0.6 * g.extent;
        // Somewhere along the noise the erosion's two extremes differ (a surface sample).
        let mut spread = 0.0f32;
        for k in 0..=100 {
            let n = k as f32 / 100.0;
            let lo = run(&g, Vec3::Y, &view(above, 32.0), &u, n, 0.0).0 .0;
            let hi = run(&g, Vec3::Y, &view(above, 32.0), &u, n, 1.0).0 .0;
            spread = spread.max((lo - hi).abs());
        }
        assert!(spread > 0.9, "the erosion moves the surface across the whole transition: {spread}");
        // The mean kept: far off (all bands faded) the field is the near field at the bands' mean.
        // Find the half-density isosurface without displacement. A cellular head must
        // add density outside it, and a recess must remove density inside it.
        let profile = g.cover * crate::volumetric_clouds::profile_at(above / g.extent, Vec3::Y);
        let mid = 1.0 - (profile - 0.5 * grad(g.relief, &u) * EDGE[1]) / g.relief;
        let at = |n, ep| run(&g, Vec3::Y, &view(above, 32.0), &u, n, ep).0 .0;
        assert!(at(mid - 0.08, 0.5) < 0.01 && at(mid - 0.08, 1.0) > 0.99);
        assert!(at(mid + 0.08, 0.5) > 0.99 && at(mid + 0.08, 0.0) < 0.01);
        let far = run(&g, Vec3::Y, &Sample { fw: 3_000.0, ..view(above, 32.0) }, &u, 0.7, 0.0).0 .0;
        let near_mean = run(&g, Vec3::Y, &Sample { fw: 3_000.0, ..view(above, 32.0) }, &u, 0.7, 1.0).0 .0;
        assert_eq!(far, near_mean, "every band faded: the mean, whatever the read");
        // The inside: whole at the worst noise and the deepest cut, EROSION up to 4.
        for erosion in [1.0f32, 4.0] {
            let mut v = u;
            v.nscale.z = 0.5;
            v.nscale.w = erosion;
            let gv = geometry(&cumulus(0.4), &v);
            let (d, _) = run(&gv, Vec3::Y, &view(0.1 * gv.extent, 32.0), &v, 0.0, 1.0);
            assert!(d.0 >= 0.999, "EROSION {erosion}: {d:?}");
        }
    }

    /// Both noise triples have the same signed billow displacement. Only the
    /// disagreement between scales cuts the fair silhouette. A joined deck at
    /// the measured overcast amount filters the cells out of its density.
    #[test]
    fn crossing_erosion_scales_cut_a_fair_rim_and_spare_a_storm_bank() {
        let neutral = [0.5; 3];
        let crossing = [0.68, 0.14, 0.5];
        for (amount, types) in [(0.29, Vec3::Y), (0.73, Vec3::new(0.11, 0.68, 0.21))] {
            let u = lean_uniform(amount, None);
            let g = geometry(&Column { types, ..cumulus(0.06) }, &u);
            let smp = view(0.6 * g.extent, 100.0);
            let at = |n, bands: [f32; 3]| {
                let mut base = |_, _, _| raw_base(n, &u);
                let mut erode = |_| bands.map(|band| raw_erode(band, &u));
                density(&g, types, &smp, &u, &mut Reads { base: &mut base, erode: &mut erode }).0
            };
            let largest_cut = (0..=100).map(|i| {
                let n = i as f32 / 100.0;
                let change = at(n, neutral) - at(n, crossing);
                if amount < 0.5 { change } else { change.abs() }
            }).fold(0.0_f32, f32::max);
            if amount < 0.5 {
                assert!(largest_cut > 0.75, "fair margin lost its channels: {largest_cut}");
            } else {
                assert_eq!(largest_cut, 0.0);
            }
        }
    }

    /// A saturated bank must retain valleys between heads across its interior.
    #[test]
    fn the_crown_has_saddles_inside_a_full_bank() {
        let u = lean_uniform(0.29, None);
        let parent = cumulus(0.35);
        let high = geometry(&Column { head: 0.6, ..parent }, &u);
        let low = geometry(&Column { head: -0.30, ..parent }, &u);
        assert_eq!((high.cover, low.cover), (1.0, 1.0));
        assert!(high.crown_cover - low.crown_cover > 0.4);
        let at = |g: &Geometry, height| run(g, Vec3::Y, &view(height * g.parent, 300.0), &u, 0.5, 0.5).0.0;
        assert_eq!(at(&low, 0.25), at(&high, 0.25));
        assert!(at(&high, 0.78) > at(&low, 0.78) + 0.75);
    }

    #[test]
    fn a_mixed_crown_loses_density_at_the_placement_boundary() {
        let mut u = lean_uniform(0.07, None);
        u.kind.x = 1.0;
        let types = u.mix.truncate();
        let head = 1.55 * lobe_weight(types, &u);
        let at = |over| {
            let g = geometry(&Column { head, types, ..cumulus(over) }, &u);
            let sample = view(0.68 * g.parent, 60.0);
            (g.crown_cover, run(&g, types, &sample, &u, 1.0, 0.5).0.0)
        };
        // Probe just inside the cutoff with a bright base texel. A cut-off bank
        // would still have positive crown cover and optical density here.
        assert!(head > 0.0 && mixed_sheet(&u) > 0.9);
        assert_eq!(at(0.0), (0.0, 0.0));
        let edge = at(0.0001);
        assert!(edge.0 < 0.001 && edge.1 == 0.0, "{edge:?}");
        let interior = at(0.055);
        assert!(interior.0 > 0.8 && interior.1 > 0.5, "{interior:?}");
    }

    #[test]
    fn a_shallow_sheet_retains_water_under_lobes_and_thins_at_open_edges() {
        let mut u = lean_uniform(0.07, None);
        u.mix = Vec3::new(0.5, 0.5, 0.0).extend(0.0);
        let mixed = mixed_sheet(&u);
        let dry_edge = bank_water(0.05, 0.0, mixed);
        let dry_interior = bank_water(0.95, 0.0, mixed);
        let damp_edge = bank_water(0.05, 0.25, mixed);
        assert!(dry_edge > 0.0 && dry_edge < 0.05);
        assert!(dry_interior > 0.35 && dry_interior < damp_edge && damp_edge < 0.95);
        assert_eq!(bank_water(0.05, 0.6, mixed), 1.0);

        // Pure cumulus, pure sheet, and a closed mixed deck keep their existing material.
        for (sheet, amount) in [(0.0, 0.29), (1.0, 0.07), (0.5, 0.73)] {
            u.mix = Vec3::new(sheet, 1.0 - sheet, 0.0).extend(0.0);
            u.scale.w = amount;
            assert_eq!(bank_water(0.05, 0.0, mixed_sheet(&u)), 1.0);
        }
    }

    #[test]
    fn a_grazing_mixed_bank_shifts_weight_to_lobes_but_retains_its_sheet() {
        let mut u = lean_uniform(0.07, None);
        u.mix = Vec3::new(0.5, 0.5, 0.0).extend(0.0);
        let shoulder = 1.0 / (32.0 * u.nscale.x);
        let across = 300.0;
        let projected = |extra: f32| (across * (across + extra * shoulder)).sqrt();
        let blend = 0.35;
        // At equal density the total stays one and the mixed height shows the lobe weight.
        let at = |width: f32, pwidth: f32, weather: &CloudVolumeUniform| {
            let (density, height) = mixed_material((1.0, 0.0), (1.0, 1.0), 0.5, blend, &view(0.0, width), pwidth, weather);
            assert!((density - 1.0).abs() < 1e-6);
            height
        };
        assert_eq!(at(across, across, &u), blend, "square view footprint");
        u.dshape.w = 1.0 / 3.0;
        let width = 3.0 * across;
        assert_eq!(at(width, across, &u), blend, "face-on reconstructed texel");
        let mid = at(width, projected(0.20), &u);
        let far = at(width, projected(0.40), &u);
        assert!(blend < mid && mid < far);
        assert!((1.0 - far - 0.10 * (1.0 - blend)).abs() < 1e-6);
        let (density, height) = mixed_material((0.25, 0.0), (1.0, 1.0), 0.5, blend, &view(0.0, width), projected(0.40), &u);
        assert!((density - (0.25 * (1.0 - far) + far)).abs() < 1e-6);
        assert!((density * height - far).abs() < 1e-6);
        for (sheet, amount) in [(0.0, 0.29), (1.0, 0.07), (0.5, 0.73)] {
            u.scale.w = amount;
            u.mix = Vec3::new(sheet, 1.0 - sheet, 0.0).extend(0.0);
            assert_eq!(at(width, projected(0.40), &u), blend);
        }
    }

    #[test]
    fn humidity_raises_the_mixed_bank_base_but_spares_pure_cumulus_and_decks() {
        let mut clear = lean_uniform(0.29, None);
        clear.mix = Vec3::Y.extend(0.0);
        for u in [lean_uniform(0.07, None), clear, lean_uniform(0.73, None)] {
            let types = u.mix.truncate();
            let at = |base_humidity| {
                let g = geometry(&Column { types, base_humidity, ..cumulus(0.30) }, &u);
                run(&g, types, &view(200.0, 40.0), &u, 1.0, 0.5).0.0
            };
            let (wet, dry) = (at(1.0), at(0.0));
            if u.scale.w < 0.1 {
                assert!(wet > 0.8 && dry == 0.0, "{wet}, {dry}");
            } else {
                assert_eq!(mixed_sheet(&u), 0.0);
                assert_eq!(wet, dry);
            }
        }
    }

    /// The sampled storm underside has distinct lower edges within a single
    /// broad band. The cloud above the pocket remains connected.
    #[test]
    fn mixed_overcast_has_a_resolved_lower_edge() {
        let u = lean_uniform(0.73, None);
        let types = Vec3::new(0.11, 0.68, 0.21);
        let g = geometry(&Column { types, ..cumulus(0.4) }, &u);
        let skin = (1.0 - types.x) * (0.25 * g.parent).min(1350.0);
        let edge_lift = 0.12 * skin;
        let dry_edge = 0.65 * skin + edge_lift;
        let at = |height, n| run(&g, types, &view(height, 300.0), &u, n, 0.5).0.0;
        assert!(at(0.46 * skin, 0.95) > 0.9 && at(0.46 * skin, 0.05) < 0.01);
        assert_eq!(at(dry_edge - 0.5 * edge_lift, 0.0), 0.0, "a dry pocket is still open here");
        assert!(at(skin, 0.05) > 0.0 && at(skin, 0.05) < at(skin, 0.95));
        // A shortcut that forgets the lift would fill the first sample.
        let mut full = u;
        full.nscale.z = 0.1;
        let inside = geometry(&Column { types, ..cumulus(0.4) }, &full);
        let full_height = dry_edge + (1.5 * skin).max((0.6 * full.profile.x).max(1.0));
        let dry = |height| run(&inside, types, &view(height, 300.0), &full, 0.0, 0.5);
        let (density, reads) = dry(full_height - 0.5 * edge_lift);
        assert!(density.0 < 0.999 && reads[0] > 0);
        let (density, reads) = dry(full_height + 0.5 * edge_lift);
        assert_eq!((density.0, reads), (1.0, [0, 0]));
    }

    #[test]
    fn a_pure_sheet_in_transitional_weather_keeps_its_original_lower_ramp() {
        let amount = 0.635;
        let u = lean_uniform(amount, None);
        let g = geometry(&Column { types: Vec3::X, ..cumulus(0.4) }, &u);
        let fair = 1.0 - smoothstep(0.55, 0.72, amount);
        let skin = fair * 0.32 * (0.34 * g.parent).min(1150.0);
        let above = 120.0;
        for noise in [0.18, 0.62] {
            let original_edge = (skin * (0.65 - noise)).max(0.0);
            let expected = smoothstep(0.0, 0.6 * u.profile.x, above - original_edge);
            assert!(expected > 0.0 && expected < 1.0);
            let density = run(&g, Vec3::X, &view(above, 64.0), &u, noise, 0.5).0.0;
            assert!((density - expected).abs() < 1e-5, "sheet noise {noise}: {density} vs {expected}");
        }
    }

    #[test]
    fn only_the_mixed_deck_foot_takes_the_second_filtered_base_read() {
        let types = Vec3::new(0.11, 0.68, 0.21);
        let sample = |amount, types: Vec3, height, eroded, detail_noise| {
            let mut u = lean_uniform(amount, None);
            u.lcell.x = 600.0;
            let col = Column { types, deck_relief: Vec2::new(0.12, 0.30), ..cumulus(0.4) };
            let g = geometry(&col, &u);
            let s = Sample { eroded, fw: 640.0, ..view(height * g.parent, 300.0) };
            let mut second = None;
            let mut base = |width, frequency, offset| {
                if frequency == 1.0 { return raw_base(0.5, &u); }
                assert_eq!(frequency, 3.0);
                second = Some((width, offset));
                raw_base(detail_noise, &u)
            };
            let d = density(&g, types, &s, &u, &mut Reads { base: &mut base, erode: &mut |_| [raw_erode(0.5, &u); 3] });
            (d.0, second)
        };
        for (eroded, expected_width) in [(true, 640.0), (false, 1200.0)] {
            let (dry, _) = sample(0.73, types, 0.05, eroded, 0.0);
            let (wet, read) = sample(0.73, types, 0.05, eroded, 1.0);
            let (width, offset) = read.expect("detail in the mixed deck foot");
            assert_eq!(width, expected_width);
            assert!((offset - Vec3::new(0.44, 0.37, 0.192)).length() < 1e-5);
            assert!(wet > dry + 0.05, "the second band changes the lower surface");
            assert!(sample(0.64, types, 0.05, eroded, 1.0).1.is_some(), "weather transition");
            for (amount, mix, h) in [(0.29, types, 0.05), (0.55, types, 0.05), (0.73, Vec3::X, 0.05), (0.73, types, 0.75)] {
                assert!(sample(amount, mix, h, eroded, 1.0).1.is_none());
            }
        }
    }

    /// A crown can change the roof without changing the support or optical density
    /// of the basal layer. Compare samples right on the eroded margin of a bank.
    #[test]
    fn heads_stretch_only_above_the_common_foot() {
        let u = lean_uniform(0.29, None);
        let col = cumulus(0.06);
        let shoulder = geometry(&Column { head: 0.55, ..col }, &u);
        let saddle = geometry(&Column { head: -0.30, ..col }, &u);
        assert!(shoulder.extent > saddle.extent + 1000.0);
        // At the same parent height a wide head must actually fill the upper side.
        let side = view(0.68 * shoulder.parent, 240.0);
        let tall = run(&shoulder, Vec3::Y, &side, &u, 0.50, 0.50).0.0;
        let short = run(&saddle, Vec3::Y, &side, &u, 0.50, 0.50).0.0;
        assert!(tall > 0.9 && short < 0.1, "upper side {tall} vs {short}");
        for above in [80.0, 250.0, 500.0, 600.0, FOOT * shoulder.parent] {
            let sample = view(above, 180.0);
            for n in [0.15, 0.4, 0.7, 1.0] {
                let tall = run(&shoulder, Vec3::Y, &sample, &u, n, 0.3).0;
                let short = run(&saddle, Vec3::Y, &sample, &u, n, 0.3).0;
                assert_eq!(tall, short, "basal density or shading height changed at {above}");
                assert_eq!(light_height(&shoulder, above), light_height(&saddle, above));
            }
        }
    }

    /// The flat base and the sheet's limits: every column stands on the domain's base; the erosion takes
    /// nothing through the protected foot and acts on the upper side; the height is at least a sheet and at most the
    /// domain; the unresolved sheet holds only under a sheet's height and only where unresolved.
    #[test]
    fn the_flat_base_and_the_sheet_limits() {
        let u = lean_uniform(0.4, None);
        for types in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(0.2, 0.5, 0.3)] {
            for vigour in [0.0f32, 0.5, 1.0] {
                for res in [1.0f32, 0.5, 0.0] {
                    let col = Column { developed_bank: 0.0, base_humidity: 0.5, fair_water_excess: 0.0, deck_relief: Vec2::ZERO, head: 0.0, over: 0.1, types, vigour, res };
                    let e = extent(&col, &u);
                    assert!((SHEET..=u.slab.y).contains(&e), "{types} {vigour}: {e}");
                    assert!(!support(&col, 0.0, &u) && !support(&col, -5.0, &u), "nothing at or under the base");
                    assert!(support(&col, 1.0, &u), "the base itself is in the support");
                    assert!(!support(&col, e, &u));
                    let outside = Column { over: -0.05, ..col };
                    assert_eq!(support(&outside, 0.5 * SHEET, &u), res < 1.0, "the sheet only where unresolved");
                    assert!(!support(&outside, SHEET + 1.0, &u), "the sheet's top");
                }
            }
        }
        let g = geometry(&cumulus(0.06), &u);
        let max_diff = |above: f32| (0..=100).map(|k| (run(&g, Vec3::Y, &view(above, 64.0), &u, k as f32 / 100.0, 0.0).0 .0 - run(&g, Vec3::Y, &view(above, 64.0), &u, k as f32 / 100.0, 1.0).0 .0).abs()).fold(0.0f32, f32::max);
        assert!(max_diff(1.0) < 0.05, "the base stays flat: {}", max_diff(1.0));
        assert_eq!(max_diff(FOOT * g.parent), 0.0, "no cellular cuts through the foot");
        assert!(max_diff(0.56 * g.parent) > 0.5, "the erosion acts on the upper side");
    }

    /// Where the density is above 0 the support holds (mirror level).
    #[test]
    fn the_lean_support_holds_every_density() {
        for a in [0.05f32, 0.4, 0.9] {
            let mut u = lean_uniform(a, None);
            for relief_aid in [0.5f32, 1.0, 3.0] {
                u.nscale.z = relief_aid;
                for over in [-0.05f32, 0.0, 0.004, 0.05, 0.3] {
                    for res in [1.0f32, 0.6, 0.0] {
                        for types in [Vec3::X, Vec3::Y, Vec3::Z] {
                            let col = Column { developed_bank: 0.0, base_humidity: 0.5, fair_water_excess: 0.0, deck_relief: Vec2::ZERO, head: 0.0, over, types, vigour: 0.7, res };
                            let g = geometry(&col, &u);
                            for k in 0..200 {
                                let above = (k as f32 + 0.5) * 200.0;
                                for (n, ep) in [(1.0f32, 0.0f32), (0.6, 0.6), (0.2, 1.0)] {
                                    let (d, _) = run(&g, types, &view(above, 64.0), &u, n, ep);
                                    let (s, _, _) = shape(&col, above, d, 1.0, &u);
                                    if s > 0.0 {
                                        assert!(support(&col, above, &u), "amount {a} SHAPE {relief_aid} over {over} res {res} {types}: density {s} at {above} outside the support");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// The shading height is continuous in the cover (the round-1 estimate jumped from
    /// 1/14 to 1/7 of the column where the cover passed the threshold): the top estimate and the height
    /// share at fixed heights move by little for a small step of the cover anywhere, including where the
    /// cover passes the threshold and at SHAPE 4 (where the threshold passes 1); 0 with no cover; monotone.
    #[test]
    fn the_shading_height_is_continuous() {
        let mixed = (1..100).map(|n| crate::volumetric_clouds::local_types(Vec4::new(0.21, 0.55, 0.24, 0.0), 0.16, n as f32 / 100.0))
            .find(|w| w.x + w.y + w.z < 1.0).expect("a mixed plateau below 1 after f32 normalization");
        for relief_aid in [1.0f32, 2.0, 4.0] {
            let mut u = lean_uniform(0.4, None);
            u.nscale.z = relief_aid;
            for types in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(0.3, 0.6, 0.1), mixed] {
                let tau = top_threshold(types, &u);
                assert_eq!(top(0.0, tau, types, &u), 0.0);
                let mut last = 0.0f32;
                for i in 1..=2_000 {
                    let c = i as f32 / 2_000.0;
                    let t = top(c, tau, types, &u);
                    assert!(t >= last - 1e-6, "SHAPE {relief_aid} {types}: monotone at {c}");
                    assert!((t - last).abs() < 0.02, "SHAPE {relief_aid} {types}: a step of {} at cover {c} (threshold {tau})", t - last);
                    last = t;
                }
                // Both sides of the threshold, the height share a light sample is given at h 0.1.
                if tau < 1.0 {
                    let side = |c: f32| (0.1 / top(c, tau, types, &u).max(1e-3)).clamp(0.0, 1.0);
                    assert!((side(tau - 1e-4) - side(tau + 1e-4)).abs() < 0.01, "the light's height share across the threshold");
                }
            }
        }
        // The cumulus at whole cover: the profile 0.55 at h = 5/7 and 0.32 at 6/7.
        let u = lean_uniform(0.4, None);
        let t = top(1.0, 0.435, Vec3::Y, &u);
        assert!((t - 5.5 / 7.0).abs() < 1e-4, "{t}");
    }

    /// The erosion read moves whole repeats where the drift's phase wraps (its frequency a whole number).
    #[test]
    fn the_erosion_read_wraps() {
        assert_eq!(ERODE_FREQ.fract(), 0.0);
        let mut u = lean_uniform(0.4, None);
        let p = Vec3::new(12_345.0, 0.0, -67_890.0);
        u.nphase.z = 0.999_9;
        let a = erode_coord(p, u.slab.x + 3_000.0, &u);
        u.nphase.z = -0.000_1;
        let b = erode_coord(p, u.slab.x + 3_000.0, &u);
        let d = a - b;
        assert!((d.x - d.x.round()).abs() < 1e-3 && d.x.round() == ERODE_FREQ && d.y.abs() < 1e-6, "{d}");
    }
}
