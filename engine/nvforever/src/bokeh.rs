//! Photo mode's lens blur (bokeh depth of field), the rules the passes in
//! `imagespace_blur.wesl` (`bokeh`, `bokeh_fill`) follow, kept here to be tested
//! (docs/design/photo-mode.md, "Depth of field") [design: a
//! real lens blur in place of the Gaussian depth of field photo mode used].
//!
//! - **The blur's size** is a circle of confusion from a thin lens [documented: the thin
//!   lens equation, any optics text]: a point at distance `z` with the lens focused at `f`
//!   is drawn as a disc whose diameter is proportional to `|z - f| / z`, taken here as the
//!   disc's radius in pixels, `max * aperture * |z - f| / z`, held to `max * aperture`
//!   (the reach of the main search) behind the focus and to [`NEAR_SCALE`] times that in
//!   front of it. What lies behind reaches the reach at the horizon; what lies in front
//!   grows faster, as through a lens, so a wall by the lens spreads far wider than the
//!   distance is blurred. The Blur slider is the aperture (0 to 1), the Focus slider
//!   (or Auto) is `f`.
//! - **Round discs**: each point gathers the points round it on a disc (a golden-angle
//!   spiral, even over the disc), each taken in where its own disc reaches the point, so a
//!   bright point spreads as a round disc of even brightness. The picture is already
//!   tonemapped (the grade runs after tonemapping), so near-white points are weighted up
//!   ([`highlight`]): they make the bright discs a real lens shows in the dark.
//! - **No bleeding behind**: a point behind the one gathering (farther from the eye) is
//!   seen in it only as far as the gathering point's own disc ([`tap_weight`]): a sharp
//!   subject is not covered by the blurred background behind it, and the edge of a blurred
//!   foreground object lets what is behind it show through, as through a lens.
//! - **A blurred foreground spreads soft**: a point in front that is more blurred than the
//!   one gathering goes to a near layer ([`near_share`]). Each such tap stands for its
//!   share of its own disc's area ([`coverage`]); their sum is how much of the gathering
//!   point the foreground covers, and the near layer is laid over the rest by it. So just
//!   past a blurred near object's edge it covers half, fading to nothing a disc away,
//!   instead of a few taps outweighing the point and moving a hard edge outwards. Discs
//!   past the main reach are found by a second, wider spiral ([`NEAR_TAPS`] taps over
//!   [`NEAR_SCALE`] times the reach), which also gathers the rest of a large disc past the
//!   reach, each of its taps weighing the area it stands for.
//! - Before the gather (`bokeh_prefilter`) and after it (`bokeh_fill`) what is blurred is
//!   spread over the spacing of the taps that will find its disc ([`spread_px`]): the main
//!   spiral's for a disc within the reach, the wide spiral's (about 3.6 times as far
//!   apart) for a large near disc. So a small bright point in a blurred foreground is met
//!   by the wide spiral's taps as an even disc, not as a pattern of the taps, and the fill
//!   smooths the grain from neighbours at least half as blurred.
//!
//! The pass runs only while photo mode is open with its Blur above 0
//! (`ImageSpaceGrade::bokeh.x`); gameplay's depth of field (VATS, conversations, the
//! modifiers) is the Gaussian one as before.
//!
//! The shader runs these rules. Rust uses [`MAX_COC`]; the rest is kept as a reference
//! model in the tests, which also check the shader's constants against it.

/// The largest disc, a part of the screen height (radius), at Blur 100% [design: about 19
/// pixels at 1080 lines, a wide-open portrait lens's look on a full-screen picture].
pub const MAX_COC: f32 = 0.018;
#[cfg(test)]
mod tests {
    // ---- the shader's rules (imagespace_blur.wesl), as a reference model ----

    /// Discs smaller than this (pixels, radius) are drawn sharp [design].
    const SHARP_PX: f32 = 0.5;
    /// The taps on the spiral of the main pass (`imagespace_blur.wesl` `BOKEH_TAPS`).
    const TAPS: u32 = 96;
    /// The taps on the wide spiral (`NEAR_TAPS`).
    const NEAR_TAPS: u32 = 64;
    /// How much larger than the reach a disc in front of the focus may grow, and the wide
    /// spiral's radius [design: 3, a wall by the lens spread over about 5% of the screen
    /// height at Blur 100%].
    const NEAR_SCALE: f32 = 3.0;
    /// Where the main spiral starts handing near discs to the wide one, as a share of the
    /// reach [design: a blend wide enough that the handover shows no seam].
    const WIDE_FROM: f32 = 0.7;
    /// How much more a near-white point weighs ([`highlight`]) [design].
    const HIGHLIGHT_GAIN: f32 = 6.0;
    /// The luminance (0 to 1, tonemapped) from which a point starts to weigh more [design].
    const HIGHLIGHT_FROM: f32 = 0.7;

    /// The radius in pixels of the disc a point `z` game units away is drawn as, with the lens
    /// focused at `focus` and opened to `aperture` (0 to 1); `max_px` the largest disc. Under
    /// [`SHARP_PX`] it is 0 (sharp). The sky (`z` infinite) is the farthest point. In front
    /// of the focus the disc may grow to [`NEAR_SCALE`] times the reach.
    fn coc_px(z: f32, focus: f32, aperture: f32, max_px: f32) -> f32 {
        let focus = focus.max(1.0);
        let z = z.max(1e-3);
        let spread = if z.is_finite() { (z - focus).abs() / z } else { 1.0 };
        let reach = max_px * aperture.clamp(0.0, 1.0);
        let cap = if z < focus { reach * NEAR_SCALE } else { reach };
        let r = (reach * spread).min(cap);
        if r < SHARP_PX {
            0.0
        } else {
            r
        }
    }

    /// How much a tap `dist` pixels from the point gathering is taken in: by its own disc
    /// `tap_coc`, or, when it lies behind the point (`tap_behind`), as far as the point's
    /// own disc `centre_coc`. The edge of a disc is softened over a pixel.
    fn tap_weight(centre_coc: f32, tap_coc: f32, tap_behind: bool, dist: f32) -> f32 {
        let reach = if tap_behind { centre_coc } else { tap_coc };
        (reach - dist + 0.5).clamp(0.0, 1.0)
    }

    /// How much of a tap goes to the near layer: a tap in front that is more blurred than
    /// the point gathering, all of it from 2 pixels more.
    fn near_share(centre_coc: f32, tap_coc: f32, tap_in_front: bool) -> f32 {
        if tap_in_front {
            ((tap_coc - centre_coc) * 0.5).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The share of a near point's disc (radius `tap_coc`) one of `n` taps over a search
    /// disc of `radius` stands for: the tap's area over the disc's. Discs under the taps'
    /// spacing count as that spacing.
    fn coverage(tap_coc: f32, radius: f32, n: u32) -> f32 {
        let c = tap_coc.max(radius * 1.8 / (n as f32).sqrt());
        radius * radius / (c * c * n as f32)
    }

    /// How much of a near disc (radius `tap_coc`) the wide spiral takes; the main spiral
    /// takes the rest (`wide_share`). Handed over from [`WIDE_FROM`] times the reach to the
    /// reach, so a disc growing past the reach leaves no seam.
    fn wide_share(tap_coc: f32, reach: f32) -> f32 {
        ((tap_coc - reach * WIDE_FROM) / (reach * (1.0 - WIDE_FROM))).clamp(0.0, 1.0)
    }

    /// The spacing of the taps that find a disc of radius `c` (pixels): the main spiral's,
    /// handed over to the wide spiral's as [`wide_share`] hands the disc over. The prefilter
    /// spreads a blurred point that far, the fill smooths over up to that.
    fn spread_px(c: f32, reach: f32) -> f32 {
        let main = reach * 1.8 / (TAPS as f32).sqrt();
        let wide = reach * NEAR_SCALE * 1.8 / (NEAR_TAPS as f32).sqrt();
        main + (wide - main) * wide_share(c, reach)
    }

    /// A point of a test scene: its grey and its distance.
    type Scene = dyn Fn(f32, f32) -> (f32, f32);

    /// The main pass on a scene at pixel `p`: the gather over both spirals, the near layer
    /// and their mix (`imagespace_blur.wesl` `bokeh`). Greys stay under [`HIGHLIGHT_FROM`], so the
    /// highlight weight is left out.
    fn shade(scene: &Scene, p: [f32; 2], focus: f32, aperture: f32) -> f32 {
        let reach = MAX * aperture;
        let (grey, z0) = scene(p[0], p[1]);
        let c0 = coc_px(z0, focus, aperture, MAX);
        let (mut acc, mut total) = (grey, 1.0);
        let (mut near_acc, mut near_total, mut cover) = (0.0, 0.0, 0.0);
        for i in 0..TAPS {
            let s = spiral(i, TAPS);
            let off = [s[0] * reach, s[1] * reach];
            let (g, z) = scene(p[0] + off[0], p[1] + off[1]);
            let tc = coc_px(z, focus, aperture, MAX);
            let w = tap_weight(c0, tc, z > z0, off[0].hypot(off[1]));
            if w <= 0.0 {
                continue;
            }
            let f = near_share(c0, tc, z < z0);
            acc += g * w * (1.0 - f);
            total += w * (1.0 - f);
            // Discs nearing the reach are handed to the wide spiral.
            let m = f * (1.0 - wide_share(tc, reach));
            near_acc += g * w * m;
            near_total += w * m;
            cover += w * m * coverage(tc, reach, TAPS);
        }
        let wide = reach * NEAR_SCALE;
        let k = wide * wide * TAPS as f32 / (reach * reach * NEAR_TAPS as f32);
        for i in 0..NEAR_TAPS {
            let s = spiral(i, NEAR_TAPS);
            let off = [s[0] * wide, s[1] * wide];
            let (g, z) = scene(p[0] + off[0], p[1] + off[1]);
            let tc = coc_px(z, focus, aperture, MAX);
            let d = off[0].hypot(off[1]);
            let w = tap_weight(c0, tc, z > z0, d);
            let t = wide_share(tc, reach);
            if w <= 0.0 || (d <= reach && t <= 0.0) {
                continue;
            }
            let f = near_share(c0, tc, z < z0);
            if d > reach {
                acc += g * w * (1.0 - f) * k;
                total += w * (1.0 - f) * k;
            }
            near_acc += g * w * f * t;
            near_total += w * f * t;
            cover += w * f * t * coverage(tc, wide, NEAR_TAPS);
        }
        let far = acc / total;
        if near_total <= 0.0 {
            return far;
        }
        far + (near_acc / near_total - far) * cover.min(1.0)
    }

    /// The weight of a point by its luminance (tonemapped, 0 to 1): 1, rising to
    /// 1 + [`HIGHLIGHT_GAIN`] at white from [`HIGHLIGHT_FROM`].
    fn highlight(luma: f32) -> f32 {
        let t = ((luma - HIGHLIGHT_FROM) / (1.0 - HIGHLIGHT_FROM)).clamp(0.0, 1.0);
        let s = t * t * (3.0 - 2.0 * t);
        1.0 + HIGHLIGHT_GAIN * s * s
    }

    /// Tap `i` of `n` on the golden-angle spiral, on the unit disc (`imagespace_blur.wesl`
    /// `spiral`): even over the disc's area.
    fn spiral(i: u32, n: u32) -> [f32; 2] {
        const GOLDEN_ANGLE: f32 = 2.399_963;
        let r = ((i as f32 + 0.5) / n as f32).sqrt();
        let a = i as f32 * GOLDEN_ANGLE;
        [r * a.cos(), r * a.sin()]
    }

    // ---- the tests ----

    const MAX: f32 = 20.0;

    #[test]
    fn in_focus_is_sharp_and_the_blur_grows_away_from_it() {
        assert_eq!(coc_px(500.0, 500.0, 1.0, MAX), 0.0);
        // Just off the focus: still under half a pixel.
        assert_eq!(coc_px(505.0, 500.0, 1.0, MAX), 0.0);
        let behind = [600.0, 1000.0, 4000.0, 100_000.0].map(|z| coc_px(z, 500.0, 1.0, MAX));
        assert!(behind.windows(2).all(|w| w[0] < w[1]), "{behind:?}");
        assert!(behind[3] <= MAX);
        // The horizon and the sky: the whole aperture.
        assert!((coc_px(f32::INFINITY, 500.0, 1.0, MAX) - MAX).abs() < 1e-4);
        assert!((coc_px(f32::INFINITY, 500.0, 0.5, MAX) - MAX * 0.5).abs() < 1e-4);
    }

    #[test]
    fn in_front_of_the_focus_the_blur_grows_faster_than_behind() {
        // The same distance off the focus.
        let front = coc_px(400.0, 500.0, 0.5, MAX);
        let back = coc_px(600.0, 500.0, 0.5, MAX);
        assert!(front > back, "{front} {back}");
        // Very near: held to the largest near disc; behind, to the reach.
        assert_eq!(coc_px(10.0, 500.0, 1.0, MAX), MAX * NEAR_SCALE);
        assert_eq!(coc_px(10.0, 500.0, 0.5, MAX), MAX * 0.5 * NEAR_SCALE);
        assert!(coc_px(1e6, 500.0, 1.0, MAX) <= MAX);
    }

    #[test]
    fn the_aperture_scales_the_blur_and_zero_is_off() {
        assert_eq!(coc_px(5000.0, 500.0, 0.0, MAX), 0.0);
        let half = coc_px(5000.0, 500.0, 0.5, MAX);
        let full = coc_px(5000.0, 500.0, 1.0, MAX);
        assert!((full - 2.0 * half).abs() < 1e-4);
    }

    #[test]
    fn a_blurred_background_does_not_cover_a_sharp_subject() {
        // A sharp point (disc 0) and a background point 5 px away with a 15 px disc.
        assert_eq!(tap_weight(0.0, 15.0, true, 5.0), 0.0);
        // The same point in front of it does cover it: a blurred foreground spreads.
        assert_eq!(tap_weight(0.0, 15.0, false, 5.0), 1.0);
        // Behind a blurred point, what lies behind is seen as far as that point's disc.
        assert_eq!(tap_weight(15.0, 15.0, true, 5.0), 1.0);
        assert_eq!(tap_weight(15.0, 0.0, true, 5.0), 1.0);
        assert_eq!(tap_weight(3.0, 15.0, true, 5.0), 0.0);
    }

    /// A wall (grey 0.1) near the eye, left of x = 0, over a field (grey 0.6) in focus.
    fn wall_over_field(x: f32, _y: f32) -> (f32, f32) {
        if x < 0.0 {
            (0.1, 100.0)
        } else {
            (0.6, 2000.0)
        }
    }

    /// Across a blurred near object's edge the picture goes soft both ways over its disc
    /// (here the largest near disc, 60 pixels), with no step: no hard edge moved out over
    /// what lies behind.
    #[test]
    fn a_blurred_foreground_spreads_soft_over_what_is_behind() {
        let row: Vec<f32> = (-80..=80).map(|x| shade(&wall_over_field, [x as f32, 0.0], 2000.0, 1.0)).collect();
        let at = |x: i32| row[(x + 80) as usize];
        // A disc away from the edge: the wall and the field as they are.
        assert!((at(-65) - 0.1).abs() < 0.01, "{row:?}");
        assert!((at(65) - 0.6).abs() < 0.01, "{row:?}");
        // At the edge, about half of each; half a disc away on either side, a part of the
        // other: the spread is as wide as the near disc, both ways.
        assert!((at(0) - 0.35).abs() < 0.06, "{row:?}");
        assert!(at(30) > 0.42 && at(30) < 0.56, "{row:?}");
        assert!(at(-30) > 0.15 && at(-30) < 0.28, "{row:?}");
        // Brighter all the way across, and no step bigger than 8% of the change (a pixel
        // of a 120-pixel ramp is under 1%, the spirals' grain adds some).
        for w in row.windows(2) {
            assert!(w[1] >= w[0] - 1e-3, "{row:?}");
            assert!(w[1] - w[0] < 0.08 * 0.5, "a step of {} in {row:?}", w[1] - w[0]);
        }
    }

    /// A near object whose disc grows past the reach (the main spiral hands it to the wide
    /// one) changes the picture smoothly: no seam where its disc crosses the reach.
    #[test]
    fn a_near_disc_crossing_the_reach_leaves_no_seam() {
        // A near post 10 pixels left of the point, over a far field; the post's distance is
        // swept so its disc goes from half the reach to twice it.
        let at = |zp: f32| {
            let scene = move |x: f32, _y: f32| if x < -10.0 { (0.1, zp) } else { (0.6, 2000.0) };
            shade(&scene, [0.0, 0.0], 2000.0, 1.0)
        };
        let reach = MAX;
        let zs: Vec<f32> = (0..=200)
            .map(|i| {
                let c = reach * (0.5 + 1.5 * i as f32 / 200.0);
                // coc = MAX * (focus - z) / z  =>  z = focus * MAX / (c + MAX)
                2000.0 * MAX / (c + MAX)
            })
            .collect();
        let vals: Vec<f32> = zs.iter().map(|&z| at(z)).collect();
        for w in vals.windows(2) {
            assert!((w[1] - w[0]).abs() < 0.01, "a jump of {} in {vals:?}", w[1] - w[0]);
        }
    }

    /// A large near disc is spread over the wide spiral's spacing, so every wide tap that
    /// lands in a small bright point's disc meets it: no point falls between the taps.
    #[test]
    fn a_large_near_disc_is_spread_as_wide_as_the_wide_taps_lie_apart() {
        let reach = MAX;
        let wide = reach * NEAR_SCALE;
        // The largest gap between a wide tap and its nearest neighbour.
        let pts: Vec<[f32; 2]> = (0..NEAR_TAPS).map(|i| spiral(i, NEAR_TAPS)).map(|s| [s[0] * wide, s[1] * wide]).collect();
        let gap = pts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                pts.iter().enumerate().filter(|&(j, _)| j != i).map(|(_, b)| (a[0] - b[0]).hypot(a[1] - b[1])).fold(f32::MAX, f32::min)
            })
            .fold(0.0, f32::max);
        // Spread over its radius each side, a point reaches the next tap.
        assert!(2.0 * spread_px(wide, reach) >= gap, "{} {gap}", spread_px(wide, reach));
        // Within the reach the main spiral's spacing, as before; handed over smoothly.
        assert_eq!(spread_px(reach * 0.5, reach), reach * 1.8 / (TAPS as f32).sqrt());
        let mut last = spread_px(0.0, reach);
        for i in 0..=100 {
            let s = spread_px(reach * 1.5 * i as f32 / 100.0, reach);
            assert!(s >= last && s - last < 0.1 * reach, "{s} {last}");
            last = s;
        }
    }

    /// Far from any edge a blurred near wall is itself, though its disc reaches past the
    /// main spiral.
    #[test]
    fn a_large_disc_of_one_colour_stays_that_colour() {
        let wall = |_x: f32, _y: f32| (0.1, 100.0);
        assert!((shade(&wall, [0.0, 0.0], 2000.0, 1.0) - 0.1).abs() < 1e-4);
    }

    /// A subject in focus (grey 0.1, left of x = 0) in front of a blurred background
    /// (grey 0.6): the subject keeps its edge, the background does not spill over it.
    #[test]
    fn a_sharp_subject_keeps_its_edge_over_a_blurred_background() {
        let scene = |x: f32, _y: f32| if x < 0.0 { (0.1, 500.0) } else { (0.6, 50_000.0) };
        for x in [-1.0, -3.0, -10.0] {
            assert!((shade(&scene, [x, 0.0], 500.0, 1.0) - 0.1).abs() < 1e-4, "{x}");
        }
        assert!((shade(&scene, [1.0, 0.0], 500.0, 1.0) - 0.6).abs() < 1e-4);
    }

    #[test]
    fn only_a_more_blurred_point_in_front_goes_to_the_near_layer() {
        assert_eq!(near_share(0.0, 15.0, true), 1.0);
        assert_eq!(near_share(10.0, 11.0, true), 0.5);
        assert_eq!(near_share(15.0, 10.0, true), 0.0);
        assert_eq!(near_share(0.0, 15.0, false), 0.0);
        // The taps inside a near disc stand for all of it, on either spiral.
        for (n, radius, disc) in [(TAPS, 20.0, 10.0), (NEAR_TAPS, 60.0, 40.0)] {
            let r = |i: u32| spiral(i, n)[0].hypot(spiral(i, n)[1]) * radius;
            let inside = (0..n).filter(|&i| r(i) <= disc).count();
            let sum = inside as f32 * coverage(disc, radius, n);
            assert!((sum - 1.0).abs() < 0.05, "{n} {sum}");
        }
    }

    #[test]
    fn a_disc_ends_over_a_pixel() {
        assert_eq!(tap_weight(10.0, 10.0, false, 9.0), 1.0);
        assert_eq!(tap_weight(10.0, 10.0, false, 10.0), 0.5);
        assert_eq!(tap_weight(10.0, 10.0, false, 11.0), 0.0);
    }

    #[test]
    fn near_white_points_weigh_more_and_others_do_not() {
        assert_eq!(highlight(0.0), 1.0);
        assert_eq!(highlight(HIGHLIGHT_FROM), 1.0);
        assert!(highlight(0.85) > 1.0);
        assert!((highlight(1.0) - (1.0 + HIGHLIGHT_GAIN)).abs() < 1e-5);
        assert!(highlight(0.8) < highlight(0.9));
    }

    /// The spiral covers the disc evenly: inside it, the inner half of the area holds
    /// half the taps, and it is centred.
    #[test]
    fn the_spiral_fills_the_disc_evenly() {
        let taps: Vec<[f32; 2]> = (0..TAPS).map(|i| spiral(i, TAPS)).collect();
        assert!(taps.iter().all(|t| t[0].hypot(t[1]) <= 1.0));
        let inner = taps.iter().filter(|t| t[0].hypot(t[1]) < std::f32::consts::FRAC_1_SQRT_2).count();
        assert_eq!(inner, TAPS as usize / 2);
        let mean = taps.iter().fold([0.0f32; 2], |m, t| [m[0] + t[0] / TAPS as f32, m[1] + t[1] / TAPS as f32]);
        assert!(mean[0].hypot(mean[1]) < 0.05, "{mean:?}");
    }

    /// The shader follows this model: its constants are these.
    #[test]
    fn the_shader_has_the_same_constants() {
        let shader = include_str!("imagespace_blur.wesl");
        for line in [
            format!("const BOKEH_TAPS: i32 = {TAPS};"),
            format!("const NEAR_TAPS: i32 = {NEAR_TAPS};"),
            format!("const NEAR_SCALE: f32 = {NEAR_SCALE:?};"),
            format!("const WIDE_FROM: f32 = {WIDE_FROM:?};"),
            format!("const SHARP_PX: f32 = {SHARP_PX:?};"),
            format!("const HIGHLIGHT_GAIN: f32 = {HIGHLIGHT_GAIN:?};"),
            format!("const HIGHLIGHT_FROM: f32 = {HIGHLIGHT_FROM:?};"),
            "const GOLDEN_ANGLE: f32 = 2.399963;".to_string(),
        ] {
            assert!(shader.contains(&line), "imagespace_blur.wesl has no `{line}`");
        }
    }
}

