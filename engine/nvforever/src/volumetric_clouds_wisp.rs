//! The volumetric clouds' wisps (#3032 stage 2h, checkpoint W2; docs/design/graphics-enhanced.md §3.23): the CPU
//! mirror of the march's `vc_wisp` (volumetric_clouds_march.wesl), its aid and its uniform block. An engineering choice for the enhanced look, never a parity item.
//!
//! On the view's eroded path only: the light's taps and the light volume's producer read `vc_shape` with
//! `eroded` false, which runs none of this. `WISP`: the erosion's lookup displaced by a bounded analytic field
//! ([`wisp`], amplitude and gradient bounds [`wisp_bounds`]) that moves with the body, the read's footprint
//! widened by the field's local stretch. Bauer 2019, slides 17-18: the detail lookup displaced by two 2D
//! displacement samples and the wind [documented]; our field is analytic (no texture, no read), our values.

use bevy::prelude::*;

/// The most `WISP` may be.
pub const MAX_WISP: f32 = 4.0;
/// The strength a run starts with (the look judged by eye on 2026-11-23).
pub const START_WISP: f32 = 0.52;

/// The wisps' field (march `vc_wisp`; W2) [design]: each horizontal component's peak at WISP 1 (units;
/// a few hundred to a couple of thousand over WISP 1..4).
pub const WISP_AMP: f32 = 300.0;
/// The two terms' weights in each component (summing to 1, so a component's peak is the amplitude).
pub const WISP_WEIGHTS: [f32; 2] = [0.6, 0.4];
/// The terms' cycles per base-noise repeat ([`crate::cloud_noise::BASE_REPEAT`], 96,000 units: 13,714
/// and 10,667 units), whole numbers so the field does not jump where the drift's phase wraps, and not
/// dividing into the erosion's 12,000-unit repeat, so the displaced erosion stops tiling at it.
pub const WISP_CYCLES: [f32; 2] = [7.0, 9.0];
/// The terms' wavelengths over the height (units; x's first and second term, z's first and second): the
/// shear between heights that leans and tears the cells. Not commensurate with each other.
pub const WISP_RISE: [f32; 4] = [6_000.0, 3_700.0, 5_300.0, 3_300.0];
/// The terms' fixed phases (radians).
pub const WISP_PHASE: [f32; 4] = [0.0, 2.0, 4.0, 1.0];

/// `WISP`: a finite number held to 0..[`MAX_WISP`]; anything else `None`.
pub fn try_wisp(v: &str) -> Option<f32> {
    v.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(0.0, MAX_WISP))
}

/// The wisps' strength a run starts with: `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_WISP` (read by `get`), else
/// [`START_WISP`].
pub fn start_wisp(get: impl Fn(&str) -> Option<String>) -> f32 {
    get("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_WISP").as_deref().and_then(try_wisp).unwrap_or(START_WISP)
}

/// The body in words (the activation's line and the tune file's), with the wisps at a strength.
pub fn describe(wisp: f32) -> String {
    use crate::volumetric_clouds_model2 as m2;
    let (amp, grad) = wisp_bounds(wisp);
    let r = crate::cloud_noise::EROSION_REPEAT / m2::ERODE_FREQ;
    format!(
        "volumetric clouds: body: one flat base for every column; the placement's excess over its threshold as the cover times a profile per type against the base noise (relief by type, sheet / cumulus / tower {:.2} / {:.2} / {:.2} at SHAPE 1) gives a signed field whose zero is the surface, the density whole within {:.0} / {:.0} / {:.0} units of it (never under two filter widths), its own extinction per unit ({:.4} / {:.4} / {:.4} at DENSITY 1); one Worley read ({:.0} / {:.0} / {:.0}-unit bands) moves the surface by up to {:.2} x EROSION of each band's cell, wispy at the foot and billowy above; WISP {wisp:.2} ({})",
        m2::RELIEF[0], m2::RELIEF[1], m2::RELIEF[2], m2::EDGE[0], m2::EDGE[1], m2::EDGE[2], m2::SIGMA[0], m2::SIGMA[1], m2::SIGMA[2], r / 3.0, r / 6.0, r / 12.0, m2::BILLOW,
        if wisp > 0.0 { format!("that read displaced up to {amp:.0} units a component, stretched at most {:.2} times", 1.0 + grad) } else { "no wisps".to_string() },
    )
}

/// The uniform's `dshape` for an aid (`None`: 1, as designed): z WISP, w 1 (the trace sets its tile's); every
/// value finite.
pub fn wisp_block(wisp: Option<f32>) -> Vec4 {
    Vec4::new(0.0, 0.0, wisp.filter(|w| w.is_finite()).map_or(1.0, |w| w.clamp(0.0, MAX_WISP)), 1.0)
}

/// The wisps' displacement at one point (march `VcWisp`): world (x, z) units, and the Frobenius norm of
/// its derivative by world (x, y, z), `g` (the lookup's stretch is at most `1 + g`).
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wisp {
    pub d: Vec2,
    pub g: f32,
}

/// The wisps' field (march `vc_wisp`) at the body coordinates `uw` (world x and z in base-noise repeats
/// plus the drift's phase, march `pos.x * nscale.x + nphase.x`, `pos.z * nscale.x + nphase.y`), the
/// height over the domain's base `y` (units), one over the base repeat `inv_repeat` (`nscale.x`) and the
/// strength `wisp` (`dshape.z`); the same f32 arithmetic in the same order.
#[cfg_attr(not(test), allow(dead_code))]
pub fn wisp(uw: Vec2, y: f32, inv_repeat: f32, wisp: f32) -> Wisp {
    use std::f32::consts::TAU;
    let a = WISP_AMP * wisp;
    let [a0, a1] = WISP_WEIGHTS;
    let [n0, n1] = WISP_CYCLES;
    let (u, w) = (uw.x, uw.y);
    let ky = Vec4::splat(TAU) / Vec4::from_array(WISP_RISE);
    let t = TAU * Vec4::new(n0 * w, n1 * u, n0 * u, n1 * w) + y * ky + Vec4::from_array(WISP_PHASE);
    let s = Vec4::new(t.x.sin(), t.y.sin(), t.z.sin(), t.w.sin());
    let c = Vec4::new(t.x.cos(), t.y.cos(), t.z.cos(), t.w.cos());
    let k0 = TAU * n0 * inv_repeat;
    let k1 = TAU * n1 * inv_repeat;
    let dxx = a1 * k1 * c.y;
    let dxy = a0 * ky.x * c.x + a1 * ky.y * c.y;
    let dxz = a0 * k0 * c.x;
    let dzx = a0 * k0 * c.z;
    let dzy = a0 * ky.z * c.z + a1 * ky.w * c.w;
    let dzz = a1 * k1 * c.w;
    let g = a * (dxx * dxx + dxy * dxy + dxz * dxz + dzx * dzx + dzy * dzy + dzz * dzz).sqrt();
    Wisp { d: a * Vec2::new(a0 * s.x + a1 * s.y, a0 * s.z + a1 * s.w), g }
}

/// The field's bounds at a strength, for the base repeat as built: (the most any one
/// component reaches, units; the most its gradient's Frobenius norm reaches, each entry at its cosine's
/// peak at once). `1 + gradient` bounds the lookup's stretch.
pub fn wisp_bounds(wisp: f32) -> (f32, f32) {
    use std::f32::consts::TAU;
    let a = WISP_AMP * wisp.max(0.0);
    let [a0, a1] = WISP_WEIGHTS;
    let k = WISP_CYCLES.map(|n| TAU * n / crate::cloud_noise::BASE_REPEAT);
    let ky = WISP_RISE.map(|l| TAU / l);
    let entries = [a1 * k[1], a0 * ky[0] + a1 * ky[1], a0 * k[0], a0 * k[0], a0 * ky[2] + a1 * ky[3], a1 * k[1]];
    (a * (a0 + a1), a * entries.iter().map(|e| e * e).sum::<f32>().sqrt())
}

/// The least determinant the horizontal part of the lookup map `(x, z) -> (x, z) + D` can have at a
/// strength: `(1 - |dDx/dx|max)(1 - |dDz/dz|max) - |dDx/dz|max |dDz/dx|max`; positive: the map never
/// folds (a cell is stretched or leaned, never turned inside out). The height's entries do not enter it
/// (D has no vertical component).
#[cfg_attr(not(test), allow(dead_code))]
pub fn wisp_least_det(wisp: f32) -> f32 {
    use std::f32::consts::TAU;
    let a = WISP_AMP * wisp.max(0.0);
    let [a0, a1] = WISP_WEIGHTS;
    let k = WISP_CYCLES.map(|n| TAU * n / crate::cloud_noise::BASE_REPEAT);
    let (diag, cross) = (a * a1 * k[1], a * a0 * k[0]);
    (1.0 - diag) * (1.0 - diag) - cross * cross
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volumetric_clouds::{CloudVolumeUniform, VolumeInputs, REFERENCE_HEIGHT};

    fn uniform_with(wisp_aid: Option<f32>) -> CloudVolumeUniform {
        crate::volumetric_clouds::uniform(&VolumeInputs { domain: REFERENCE_HEIGHT, horizon: 400_000.0, steps: 28, density: 1.0, wisp_aid, ..Default::default() })
    }

    /// The key's range, the start value, the block and the words.
    #[test]
    fn the_wisp_key_parses() {
        assert_eq!((try_wisp("2.5"), try_wisp("9"), try_wisp("-1"), try_wisp("x"), try_wisp("NaN")), (Some(2.5), Some(4.0), Some(0.0), None, None));
        let var = |v: &'static str| move |n: &str| (n == "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_WISP").then(|| v.to_string());
        assert_eq!((start_wisp(|_| None), start_wisp(var("2")), start_wisp(var("lots"))), (START_WISP, 2.0, START_WISP));
        assert_eq!(wisp_block(Some(f32::NAN)), Vec4::new(0.0, 0.0, 1.0, 1.0), "WISP 1, untiled until prepared for a view");
        assert_eq!((wisp_block(None).z, wisp_block(Some(9.0)).z), (1.0, MAX_WISP));
        assert!(describe(0.0).ends_with("; WISP 0.00 (no wisps)") && describe(START_WISP).contains("; WISP 0.52 (that read displaced up to 156 units a component, stretched at most 1.32 times)"), "{}", describe(START_WISP));
    }

    /// W2: the mirror is the march's arithmetic. The constants are the march's literals, the
    /// field reads the base noise's own body coordinates (`vc_base_coord`'s `pos.x * v.nscale.x +
    /// v.nphase.x`: the producer's unwrapped drift, no motion of its own against the body), and the
    /// branch is the line the mirror follows.
    #[test]
    fn the_wisp_mirror_is_the_marchs_arithmetic() {
        assert_eq!((WISP_AMP, WISP_WEIGHTS, WISP_CYCLES), (300.0, [0.6, 0.4], [7.0, 9.0]));
        assert_eq!((WISP_RISE, WISP_PHASE), ([6_000.0, 3_700.0, 5_300.0, 3_300.0], [0.0, 2.0, 4.0, 1.0]));
        assert_eq!(WISP_WEIGHTS[0] + WISP_WEIGHTS[1], 1.0, "a component's peak is the amplitude");
        let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        for line in [
            "const VC_WISP_AMP: f32 = 300.0;",
            "const VC_WISP_A0: f32 = 0.6;",
            "const VC_WISP_A1: f32 = 0.4;",
            "const VC_WISP_N0: f32 = 7.0;",
            "const VC_WISP_N1: f32 = 9.0;",
            "const VC_WISP_LY: vec4<f32> = vec4<f32>(6000.0, 3700.0, 5300.0, 3300.0);",
            "const VC_WISP_PHI: vec4<f32> = vec4<f32>(0.0, 2.0, 4.0, 1.0);",
            // The field.
            "let a = VC_WISP_AMP * v.dshape.z;",
            "let u = pos.x * v.nscale.x + v.nphase.x;",
            "let w = pos.z * v.nscale.x + v.nphase.y;",
            "let ky = vec4<f32>(6.2831855) / VC_WISP_LY;",
            "let t = 6.2831855 * vec4<f32>(VC_WISP_N0 * w, VC_WISP_N1 * u, VC_WISP_N0 * u, VC_WISP_N1 * w) + (yb - v.slab.x) * ky + VC_WISP_PHI;",
            "let k0 = 6.2831855 * VC_WISP_N0 * v.nscale.x;",
            "let k1 = 6.2831855 * VC_WISP_N1 * v.nscale.x;",
            "let dxx = VC_WISP_A1 * k1 * c.y;",
            "let dxy = VC_WISP_A0 * ky.x * c.x + VC_WISP_A1 * ky.y * c.y;",
            "let dxz = VC_WISP_A0 * k0 * c.x;",
            "let dzx = VC_WISP_A0 * k0 * c.z;",
            "let dzy = VC_WISP_A0 * ky.z * c.z + VC_WISP_A1 * ky.w * c.w;",
            "let dzz = VC_WISP_A1 * k1 * c.w;",
            "let g = a * sqrt(dxx * dxx + dxy * dxy + dxz * dxz + dzx * dzx + dzy * dzy + dzz * dzz);",
            "return VcWisp(a * vec2<f32>(VC_WISP_A0 * s.x + VC_WISP_A1 * s.y, VC_WISP_A0 * s.z + VC_WISP_A1 * s.w), g);",
            // The branch in `vc_m2_density`.
            "if eroded && detail && v.dshape.z > 0.0 {",
        ] {
            assert!(src.contains(line), "the march no longer has `{line}`: the mirror must follow");
        }
        assert!(src.contains("return vec3<f32>(pos.x * v.nscale.x + v.nphase.x, (yb - v.slab.x) * v.nscale.x, pos.z * v.nscale.x + v.nphase.y);"), "the field's body coordinates are the base noise's");
        assert_eq!(std::f32::consts::TAU, 6.283_185_5);
        // The field reads nothing: no texture in its function.
        let body = &src[src.find("fn vc_wisp(").unwrap()..];
        assert!(!body[..body.find("\n}\n").unwrap()].contains("texture"), "the wisps read a texture");
    }

    /// The field's amplitude and gradient bounds hold at every strength, the local
    /// stretch the march computes is the field's true derivative (central differences), the horizontal
    /// lookup never folds up to WISP 4, and the field is whole across a wrap of the drift's phase.
    #[test]
    fn the_wisp_field_is_bounded_and_never_folds() {
        let inv = 1.0 / crate::cloud_noise::BASE_REPEAT;
        let (amp1, grad1) = wisp_bounds(1.0);
        assert_eq!(amp1, 300.0);
        assert!((grad1 - 0.6105).abs() < 1e-3, "{grad1}");
        let (amp4, grad4) = wisp_bounds(4.0);
        assert_eq!(amp4, 1_200.0);
        assert!((grad4 - 2.442).abs() < 4e-3, "{grad4}");
        assert_eq!(wisp_bounds(0.0), (0.0, 0.0));
        assert!(wisp_least_det(4.0) > 0.4 && wisp_least_det(1.0) > 0.85, "{} {}", wisp_least_det(4.0), wisp_least_det(1.0));
        let at = |x: f32, y: f32, z: f32, s: f32| wisp(Vec2::new(x * inv, z * inv), y, inv, s);
        let mut most = (0.0f32, 0.0f32);
        for s in [0.5f32, 1.0, 4.0] {
            let (amp, grad) = wisp_bounds(s);
            for i in 0..23 {
                for j in 0..19 {
                    for k in 0..17 {
                        let (x, z, y) = (i as f32 * 4_217.0 - 40_000.0, j as f32 * 5_113.0 - 30_000.0, k as f32 * 2_389.0);
                        let wp = at(x, y, z, s);
                        assert!(wp.d.x.abs() <= amp * 1.0001 && wp.d.y.abs() <= amp * 1.0001, "amplitude at {x} {y} {z}: {:?}", wp.d);
                        assert!(wp.g <= grad * 1.0001, "gradient at {x} {y} {z}: {} over {grad}", wp.g);
                        if s == 1.0 {
                            most = (most.0.max(wp.d.x.abs().max(wp.d.y.abs())), most.1.max(wp.g));
                        }
                        // The true derivative by central differences (16 units).
                        let e = 16.0;
                        let dx = (at(x + e, y, z, s).d - at(x - e, y, z, s).d) / (2.0 * e);
                        let dy = (at(x, y + e, z, s).d - at(x, y - e, z, s).d) / (2.0 * e);
                        let dz = (at(x, y, z + e, s).d - at(x, y, z - e, s).d) / (2.0 * e);
                        let fd = (dx.length_squared() + dy.length_squared() + dz.length_squared()).sqrt();
                        assert!((fd - wp.g).abs() <= 3e-3 + 1e-2 * wp.g, "the stretch at {x} {y} {z}: {} against {fd}", wp.g);
                        // The horizontal map never folds.
                        let det = (1.0 + dx.x) * (1.0 + dz.y) - dz.x * dx.y;
                        assert!(det >= wisp_least_det(s) - 5e-3 && det > 0.0, "a fold at {x} {z} (WISP {s}): {det}");
                    }
                }
            }
        }
        assert!(most.0 > 0.8 * amp1 && most.1 > 0.5 * grad1, "the bounds are near what the field reaches: {most:?}");
        // Whole across a wrap: one base repeat further in either axis is the same field.
        for (u, w) in [(0.999_99f32, 0.3f32), (0.4, 0.999_99), (0.0, 0.0)] {
            let a = wisp(Vec2::new(u, w), 1_234.0, inv, 1.0);
            let b = wisp(Vec2::new(u - 1.0, w), 1_234.0, inv, 1.0);
            let c = wisp(Vec2::new(u, w - 1.0), 1_234.0, inv, 1.0);
            assert!((a.d - b.d).length() < 0.05 && (a.d - c.d).length() < 0.05 && (a.g - b.g).abs() < 1e-4, "{a:?} {b:?} {c:?}");
        }
    }

    /// The uniform carries WISP, 0 for none (the march then skips the branch).
    #[test]
    fn the_uniform_carries_the_wisps_strength() {
        assert_eq!(uniform_with(Some(0.0)).dshape.z, 0.0);
        assert_eq!(uniform_with(None).dshape.z, 1.0, "WISP 1 as built");
        assert_eq!(uniform_with(Some(f32::NAN)).dshape.z, 1.0);
    }

    /// `eroded` false (the light's taps, the light volume's producer) never reaches the
    /// wisps, and `vc_shape`'s signature is as it was.
    #[test]
    fn the_uneroded_shape_never_reads_the_wisps() {
        let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        assert!(src.contains("fn vc_shape(rel: vec3<f32>, cam: vec3<f32>, width: f32, pwidth: f32, nb: texture_3d<f32>, ns: sampler, ne: texture_3d<f32>, eroded: bool, v: CloudVolume) -> vec4<f32> {"));
        assert_eq!(src.matches("let wp = vc_wisp(pos, yb, v);").count(), 1);
        assert!(src.contains("    if eroded && detail && v.dshape.z > 0.0 {\n        let wp = vc_wisp(pos, yb, v);\n"), "outside the eroded, detail and WISP branch");
    }

    /// The displaced read's footprint is widened by the stretch there (march `vc_m2_density`'s
    /// `ew`), so a band's keep falls as its cells shrink under the lookup's stretch; never narrower.
    #[test]
    fn the_wisps_widen_the_footprint_by_the_stretch() {
        let u = uniform_with(None);
        let (_, grad) = wisp_bounds(1.0);
        let mut widened = 0;
        for i in 0..200 {
            let xz = Vec2::new(i as f32 * 1_733.0 - 150_000.0, (i as f32 * 0.37).sin() * 80_000.0);
            let wp = wisp(Vec2::new(xz.x * u.nscale.x + u.nphase.x, xz.y * u.nscale.x + u.nphase.y), 300.0 + i as f32 * 97.0, u.nscale.x, u.dshape.z);
            assert!(wp.g >= 0.0 && wp.g <= grad * 1.0001, "{wp:?}");
            assert!(wp.d.length() <= 300.0 * std::f32::consts::SQRT_2 * 1.0001, "{wp:?}");
            let keeps = |w: f32| crate::volumetric_clouds_model2::keeps(w, &u);
            let (wide, narrow) = (keeps(150.0 * (1.0 + wp.g)), keeps(150.0));
            for k in 0..3 {
                assert!(wide[k] <= narrow[k], "{wp:?}: {wide:?} over {narrow:?}");
            }
            // At 150 units the finest band (500-unit cells, gone at 250) is the one in its fade.
            assert!(wide[0] == 1.0 && narrow[2] > 0.8 && narrow[2] < 1.0, "{wide:?} {narrow:?}");
            if wp.g > 0.2 {
                widened += 1;
                assert!(wide[2] < narrow[2] - 0.2, "{wp:?}: the stretch takes the finest band's keep: {wide:?} against {narrow:?}");
            }
        }
        assert!(widened > 0, "the field stretches somewhere");
        let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        assert!(src.contains("        epos = pos + vec3<f32>(wp.d.x, 0.0, wp.d.y);\n        ew = lw * (1.0 + wp.g);\n"), "the march's shift and stretch");
        for line in ["    let keep = vc_m2_keeps(ew, v);\n", "vc_m2_erode_coord(epos, yb, v), vc_level(ew, v.nlevel.z, q, v.nlevel.w)).rgb;"] {
            assert_eq!(src.matches(line).count(), 1, "the widened width is what the bands and the level take: {line}");
        }
    }
}
