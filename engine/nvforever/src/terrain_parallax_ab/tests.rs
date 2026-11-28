//! The terrain parallax readback's measure on generated pictures (no GPU).

use super::*;

const W: usize = 1280;
const H: usize = 720;

/// A textured picture: value noise on a 4-pixel lattice (aperiodic, deterministic), levels
/// 40..215, read at `(x, y)` through `map` (the picture's own coordinate to the texture's).
fn picture(map: impl Fn(f32, f32) -> (f32, f32)) -> Luma {
    let lattice = |i: i64, j: i64| -> f32 {
        let mut h = (i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663)) as u64;
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        (h % 1000) as f32 / 999.0
    };
    let noise = |x: f32, y: f32| -> f32 {
        let (gx, gy) = (x / 4.0, y / 4.0);
        let (i, j) = (gx.floor() as i64, gy.floor() as i64);
        let (fx, fy) = (gx - gx.floor(), gy - gy.floor());
        let top = lattice(i, j) * (1.0 - fx) + lattice(i + 1, j) * fx;
        let bottom = lattice(i, j + 1) * (1.0 - fx) + lattice(i + 1, j + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    };
    let mut v = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            let (tx, ty) = map(x as f32, y as f32);
            v.push(40.0 + 175.0 * noise(tx, ty));
        }
    }
    Luma { w: W, h: H, v }
}

fn plain() -> Luma {
    picture(|x, y| (x, y))
}

#[test]
fn the_times_modes_and_poses_are_read() {
    let spec = |at: f32, hold: bool, pose: Pose| Spec { at, hold, pose, base: false };
    assert_eq!(parse("20:hold, 8").unwrap(), [spec(8.0, false, Pose::View), spec(20.0, true, Pose::View)], "in time order");
    assert_eq!(parse("8:flip").unwrap(), [spec(8.0, false, Pose::View)]);
    assert_eq!(parse("9:grazing,42:head-on:hold").unwrap(), [spec(9.0, false, Pose::Grazing), spec(42.0, true, Pose::HeadOn)]);
    assert_eq!(parse("5:hold:view").unwrap(), [spec(5.0, true, Pose::View)]);
    assert_eq!(parse("9:grazing:base").unwrap(), [Spec { base: true, ..spec(9.0, false, Pose::Grazing) }]);
    for bad in ["", "x", "8:sideways", " , ", "8:grazing:up"] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_box_and_the_search_sit_at_the_centre_inside_the_picture() {
    assert_eq!(centre_box(W, H), (602, 331, 76, 57));
    assert_eq!(search_radius(W, H), (32, 96));
    // A small view: the search never leaves the picture.
    let (x0, y0, bw, bh) = centre_box(160, 90);
    let (rx, ry) = search_radius(160, 90);
    assert!(x0 - rx >= 0 && y0 - ry >= 0 && x0 + bw + rx <= 160 && y0 + bh + ry <= 90);
}

#[test]
fn pixels_of_four_bytes_are_read_as_the_channels_mean() {
    let l = Luma::from_pixels(&[30, 60, 90, 255, 0, 0, 0, 0], 2, 1).unwrap();
    assert_eq!(l.v, [60.0, 0.0]);
    assert!(Luma::from_pixels(&[0; 7], 2, 1).is_none(), "not four bytes a pixel");
    assert!(Luma::from_pixels(&[], 0, 0).is_none());
}

#[test]
fn a_translated_picture_is_measured_as_moved_by_its_translation() {
    let a = plain();
    // The texture moved 17 pixels down the screen: B(x, y) = A(x, y - 17), C back as A.
    let b = picture(|x, y| (x, y - 17.0));
    let m = measure(&a, &b, &a).unwrap();
    assert_eq!(m.shift, (0, 17));
    assert!(m.fit < 1e-3, "the translation explains the whole change: {m:?}");
    assert!(m.ab > MOVED_LEVELS && m.ac == 0.0 && m.bc == m.ab);
    assert_eq!(judge(&m, None), "moved");
    assert_eq!(judge(&m, Some(0.95)), "moved", "over ground the march moves");
    assert!(describe(&m).contains("B = A moved by (+0, +17) px"), "{}", describe(&m));
    // Up the screen and a little sideways: the sign is kept.
    let up = picture(|x, y| (x + 3.0, y + 9.0));
    let m = measure(&a, &up, &a).unwrap();
    assert_eq!((m.shift, judge(&m, None)), ((-3, -9), "moved"));
}

#[test]
fn ground_the_march_does_not_move_is_bare_whatever_the_pictures_show() {
    let a = plain();
    let b = picture(|x, y| (x, y - 17.0));
    let moved = measure(&a, &b, &a).unwrap();
    let still = measure(&a, &a, &a).unwrap();
    for m in [&moved, &still] {
        assert_eq!(judge(m, Some(BARE_SHARE - 0.01)), "bare");
        assert_eq!(judge(m, Some(0.0)), "bare", "a placed object or the base alone under the centre");
    }
    assert_eq!(judge(&still, Some(1.0)), "still");
}

#[test]
fn an_unchanged_or_radially_changed_picture_is_still() {
    let a = plain();
    let m = measure(&a, &a, &a).unwrap();
    assert_eq!((m.ab, m.shift, judge(&m, None)), (0.0, (0, 0), "still"));
    // Head-on the shift grows from nothing at the centre outwards: no common translation.
    let (cx, cy) = (W as f32 / 2.0, H as f32 / 2.0);
    let radial = picture(|x, y| (cx + (x - cx) * 0.985, cy + (y - cy) * 0.985));
    let m = measure(&a, &radial, &a).unwrap();
    assert!(m.ab > 0.0 && m.ab < STILL_SHARE * m.contrast[0], "the picture did change, by a fraction of its grain: {m:?}");
    assert_eq!((m.shift, judge(&m, None)), ((0, 0), "still"), "{m:?}");
    // A change no larger than what changes anyway (A-C) is still.
    let noisy = |seed: f32| picture(move |x, y| (x + 0.15 * (y * 0.37 + seed).sin(), y));
    let (b, c) = (noisy(1.0), noisy(2.0));
    let m = measure(&a, &b, &c).unwrap();
    assert!(m.ac <= STABLE_LEVELS, "{m:?}");
    assert_eq!(judge(&m, None), "still", "{m:?}");
}

#[test]
fn a_scene_that_changed_by_itself_is_unsettled_not_still() {
    // B and C alike, both unlike A: A-B equals A-C, which `still` alone would pass; A-C past
    // its own limit makes the sequence unjudgeable instead.
    let a = plain();
    let changed = picture(|x, y| (x * 1.7 + 300.0, y * 1.3 + 200.0));
    let m = measure(&a, &changed, &changed).unwrap();
    assert!((m.ab - m.ac).abs() < 1e-9 && m.bc == 0.0, "{m:?}");
    assert!(m.ac > STABLE_LEVELS.max(STABLE_SHARE * m.contrast[0]), "{m:?}");
    assert_eq!(judge(&m, None), "unsettled");
    // A translated B with A-C past the limit is not certified moved either.
    let moved = picture(|x, y| (x, y - 17.0));
    let m = measure(&a, &moved, &changed).unwrap();
    assert_eq!(judge(&m, None), "unsettled");
}

#[test]
fn a_picture_without_texture_is_flat_and_a_wrong_size_fails() {
    let a = plain();
    let flat = Luma { w: W, h: H, v: vec![128.0; W * H] };
    assert_eq!(judge(&measure(&a, &flat, &a).unwrap(), None), "flat");
    assert!(measure(&a, &Luma::default(), &a).is_err(), "an unreadable shot");
}

#[test]
fn a_stable_change_no_translation_explains_is_changed_and_a_middling_one_unclear() {
    let a = plain();
    // A different texture altogether in B alone (C back as A): the texture decorrelated, A and
    // C agree, no translation fits: what relief over ridges does at a grazing view.
    let other = picture(|x, y| (x * 1.7 + 300.0, y * 1.3 + 200.0));
    let m = measure(&a, &other, &a).unwrap();
    assert!(m.fit > MOVED_FIT * m.ab && m.correlation[0] < 0.3 && m.correlation[1] == 1.0, "{m:?}");
    assert_eq!(judge(&m, None), "changed", "{m:?}");
    // Three tenths of the other texture mixed in: still well correlated (about 0.9), yet more
    // than a still view's change.
    let mixed = Luma { v: a.v.iter().zip(&other.v).map(|(x, o)| 0.69 * x + 0.31 * o).collect(), ..a.clone() };
    let m = measure(&a, &mixed, &a).unwrap();
    assert!(m.correlation[0] > CHANGED_CORRELATION && m.ab > STILL_SHARE * m.contrast[0], "the fixture sits between the two rules: {m:?}");
    assert_eq!(judge(&m, None), "unclear", "{m:?}");
}

/// Controls: an unchanged texture brightened everywhere, or with its contrast scaled, is no
/// `changed` however many levels it differs by (its correlation stays 1).
#[test]
fn a_brightness_offset_or_a_contrast_scale_is_not_changed() {
    let a = plain();
    let offset = Luma { v: a.v.iter().map(|x| x + 6.0).collect(), ..a.clone() };
    let m = measure(&a, &offset, &a).unwrap();
    assert!((m.ab - 6.0).abs() < 1e-3 && (m.correlation[0] - 1.0).abs() < 1e-9, "{m:?}");
    assert_ne!(judge(&m, None), "changed");
    let darker = Luma { v: a.v.iter().map(|x| x - 30.0).collect(), ..a.clone() };
    let m = measure(&a, &darker, &a).unwrap();
    assert!(m.ab > 25.0 && (m.correlation[0] - 1.0).abs() < 1e-9);
    assert_ne!(judge(&m, None), "changed", "30 levels darker is still the same texture");
    let mean = a.v.iter().sum::<f32>() / a.v.len() as f32;
    let scaled = Luma { v: a.v.iter().map(|x| mean + 1.6 * (x - mean)).collect(), ..a.clone() };
    let m = measure(&a, &scaled, &a).unwrap();
    assert!((m.correlation[0] - 1.0).abs() < 1e-6, "{m:?}");
    assert_ne!(judge(&m, None), "changed");
}

/// One rule for `still`: a B that differs from A by half the contrast is not still, even when
/// C differs from A as much (B == C != A on a low-contrast texture, A-C inside its limit).
#[test]
fn still_needs_no_translation_and_a_small_change_in_every_case() {
    let a = Luma { v: plain().v.iter().map(|x| 128.0 + (x - 128.0) * 0.17).collect(), ..plain() };
    let m0 = measure(&a, &a, &a).unwrap();
    assert!(m0.contrast[0] > FLAT_LEVELS && m0.contrast[0] < 8.0, "a low-contrast fixture: {m0:?}");
    let up = Luma { v: a.v.iter().map(|x| x + 2.5).collect(), ..a.clone() };
    let m = measure(&a, &up, &up).unwrap();
    assert!(m.ac <= STABLE_LEVELS && m.ab > STILL_SHARE * m.contrast[0], "{m:?}");
    assert_ne!(judge(&m, None), "still");
    assert_eq!(judge(&m, None), "unclear");
    // A small translation is never still, however small the difference.
    let nudged = Measure { shift: (0, 1), ..m0.clone() };
    assert_ne!(judge(&nudged, None), "still");
}

/// The real readbacks (the aid's spot in cell (-18, 0)), by their logged numbers and the
/// correlations of the saved shots of the run on 555ace098: the verdicts the scenarios want,
/// with the margins the constants' notes give.
#[test]
fn the_runs_numbers_give_the_verdicts_the_scenarios_want() {
    let m = |ab: f64, ac: f64, bc: f64, contrast: [f64; 3], correlation: [f64; 3]| Measure { rect: centre_box(W, H), ab, ac, bc, shift: (0, 0), fit: ab, contrast, correlation };
    let grazing = m(12.29, 0.11, 12.29, [14.1, 13.5, 14.1], [0.231, 1.000, 0.231]);
    let head_on = m(5.07, 1.07, 4.64, [32.5, 31.6, 32.6], [0.945, 0.998, 0.949]);
    let held_grazing = m(0.05, 0.11, 0.06, [14.1, 14.1, 14.1], [1.000, 1.000, 1.000]);
    let held_head_on = m(1.67, 1.02, 1.53, [32.5, 32.5, 32.6], [0.991, 0.998, 0.994]);
    assert_eq!(judge(&grazing, Some(0.88)), "changed");
    assert_eq!(judge(&head_on, Some(0.91)), "still");
    assert_eq!(judge(&held_grazing, Some(0.88)), "still");
    assert_eq!(judge(&held_head_on, Some(0.91)), "still");
    // Margins: the grazing change would still pass at three times its correlation; head-on
    // would still be still at about twice its change.
    let weaker = Measure { correlation: [0.69, 1.0, 0.69], ..grazing.clone() };
    assert_eq!(judge(&weaker, None), "changed");
    let doubled = m(9.5, 1.07, 9.4, [32.5, 31.6, 32.6], [0.9, 0.998, 0.9]);
    assert_eq!(judge(&doubled, None), "still");
    // A-C losing its correlation (the scene moved by itself) makes any of them unsettled.
    let drifted = Measure { correlation: [0.231, 0.95, 0.231], ..grazing.clone() };
    assert_eq!(judge(&drifted, None), "unsettled");
}

#[test]
fn the_crosshair_is_left_out_of_every_sum() {
    let a = plain();
    // A reticle 26 px across drawn on B alone changes nothing measured, its contrast included.
    let mut b = a.clone();
    for y in H / 2 - 13..H / 2 + 13 {
        for x in W / 2 - 13..W / 2 + 13 {
            b.v[y * W + x] = 255.0;
        }
    }
    let m = measure(&a, &b, &a).unwrap();
    assert_eq!((m.ab, m.bc, m.shift), (0.0, 0.0, (0, 0)));
    assert_eq!(m.contrast[0], m.contrast[1], "the reticle is not counted as texture");
}

/// One bright speck that moves only in B decorrelates the box but changes its mean by a
/// fraction of a level: never `changed` (the [`CHANGED_LEVELS`] floor).
#[test]
fn a_moving_speck_is_not_changed() {
    let m = Measure {
        rect: (0, 0, 76, 57),
        ab: 0.071,
        ac: 0.0,
        bc: 0.071,
        shift: (0, 0),
        fit: 0.071,
        contrast: [2.13, 2.13, 2.13],
        correlation: [0.02, 1.0, 0.02],
    };
    assert_ne!(judge(&m, Some(0.9)), "changed");
}
