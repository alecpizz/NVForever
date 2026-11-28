//! The made volume, its reads and the light on the CPU: rtgi_volume.wesl mirrored step for step,
//! whose text tests.rs `the_light_is_the_approved_one` holds.

use super::made::*;
use super::*;
use crate::mesh_blend_shader_tests::compose;
use bevy::render::gpu_readback::ReadbackComplete;
use nvf_rtgi::{Reader, V3};
use std::collections::BTreeSet;

const READBACK: &str = "embedded://nvforever/rtgi_readback.wesl";
const LOWS: [[i32; 3]; 3] = [[0, 0, 0], [-1, 3, -2], [-150, 40, 97]];

/// A filter that gives a texel's part as it is.
fn exact(part: f32) -> f32 {
    part
}

/// 8 fractional bits: rounded, cut, and up, so that a place just off a texel's centre comes out on
/// either side of it.
const COARSE: [fn(f32) -> f32; 3] = [|part| (part * 256.0).round() / 256.0, |part| (part * 256.0).floor() / 256.0, |part| (part * 256.0).ceil() / 256.0];

/// The texture at coordinates of 0 to 1, linear, clamped to the edge.
fn sample(v: &SolvedVolume, t: Vec3, filter: fn(f32) -> f32) -> Vec4 {
    let sides = UVec3::from(v.atlas);
    let x = t * sides.as_vec3() - 0.5;
    let (low, part) = (x.floor(), (x - x.floor()).map(filter));
    let mut out = Vec4::ZERO;
    for i in 0..8 {
        let up = Vec3::from_array(std::array::from_fn(|k| ((i >> k) & 1) as f32));
        let share = (up * part + (1.0 - up) * (1.0 - part)).element_product();
        let at = (low + up).max(Vec3::ZERO).as_uvec3().min(sides - 1);
        let from = 8 * ((at.z * sides.y + at.y) * sides.x + at.x) as usize;
        let channel = |k: usize| half::f16::from_le_bytes([v.rgba16f_le[from + 2 * k], v.rgba16f_le[from + 2 * k + 1]]).to_f32();
        out += share * Vec4::from_array(std::array::from_fn(channel));
    }
    out
}

/// `nvf_rtgi_read`: E, v.
fn read(v: &SolvedVolume, u: &RtgiVolumeUniform, c: &Check, filter: fn(f32) -> f32) -> (Vec3, f32) {
    let [p, g, n] = [c.p, c.g, c.n].map(Vec3::from);
    let q = (p + u.dimensions.w * g - u.origin.truncate()) * u.origin.w;
    if !(q.cmpgt(Vec3::NEG_ONE).all() && q.cmplt(u.dimensions.truncate()).all()) {
        return (Vec3::ZERO, 0.0);
    }
    let t = q + 1.5;
    let slab = Vec3::new(0.0, 0.0, u.atlas.w);
    let [c0, cx, cy, cz] = [0.0, 1.0, 2.0, 3.0].map(|s| sample(v, (t + s * slab) * u.atlas.truncate(), filter));
    let w = 1.0 / c0.w.max(u.fit.z);
    let e = (w * (c0.truncate() + cx.truncate() * n.x + cy.truncate() * n.y + cz.truncate() * n.z)).max(Vec3::ZERO);
    (e, w * cx.w)
}

fn linear(x: Vec3) -> Vec3 {
    x.max(Vec3::ZERO).powf(2.2)
}

/// `nvf_rtgi_light`, from what was read.
fn light(u: &RtgiVolumeUniform, ambient: Vec3, sum: Vec3, occlusion: Vec3, (e, v): (Vec3, f32)) -> Vec3 {
    let luminance = |x: Vec3| x.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    let k = u.fit.y + (1.0 - u.fit.y) * (u.fit.x * v).min(1.0);
    let bounce = u.light.y * e * occlusion;
    let mut light = linear(sum - (1.0 - u.light.x) * k * ambient) + bounce;
    let was = linear(sum);
    let before = luminance(was);
    if before > 0.0 {
        light += u.fit.w * (luminance(light) * was / before - light);
    }
    if u.light.z < 1.0 {
        light = was + u.light.z * (light - was);
    }
    light
}

/// The volume is what its notes say: the sides (rows of 40 bytes), every point with values of its
/// own, the bands of either sign, what does not count and what is black; the CPU's reader takes it.
#[test]
fn the_made_volume_is_what_it_says() {
    let v = made(LOWS[1]);
    Reader::new(&v).unwrap();
    assert_eq!((v.dimensions, v.atlas, v.rgba16f_le.len()), ([3, 2, 4], [5, 4, 24], 40 * 4 * 24));
    let counting: Vec<_> = points().filter(|p| !UNSUPPORTED.contains(p)).collect();
    assert_eq!((points().count(), counting.len()), (24, 21));
    for slab in 0..4 {
        let colours: Vec<[f32; 3]> = counting.iter().map(|p| texels(*p)[slab]).map(|[r, g, b, _]| [r, g, b]).collect();
        assert!((0..21).all(|i| (0..i).all(|j| colours[i] != colours[j])), "slab {slab}");
        let below = colours.iter().flatten().any(|c| *c < 0.0);
        assert_eq!((below, colours.iter().filter(|c| **c == [0.0; 3]).count()), (slab > 0, 1), "slab {slab}");
    }
    // One of them counts and is black.
    assert_eq!(counting.iter().filter(|p| texels(**p) == [[0.0, 0.0, 0.0, 1.0], [0.0; 4], [0.0; 4], [0.0; 4]]).count(), 1);
}

/// Whether the stored bytes have these points' values in these slabs, worked out by hand from the
/// notes: `z * 20 + y * 5 + x` texels of 8 bytes, a point one in from the shell on each axis, a slab
/// 6 layers deep.
fn packed_right(bytes: &[u8]) -> bool {
    let by_hand: [(usize, [f32; 4]); 10] = [
        // The shell.
        (0, [0.0; 4]),
        // The point (0, 0, 0): C0 and m, then Cy.
        (20 + 5 + 1, [0.25, 0.359_375, 0.468_75, 1.0]),
        (13 * 20 + 5 + 1, [-0.4375, -0.156_25, 0.125, 0.0]),
        // (2, 0, 3): C0, Cx and m v, Cy, Cz.
        (4 * 20 + 5 + 3, [0.921_875, 1.031_25, 1.140_625, 1.0]),
        (10 * 20 + 5 + 3, [0.3125, -0.3125, -0.031_25, 1.015_89]),
        (16 * 20 + 5 + 3, [0.093_75, 0.375, -0.25, 0.0]),
        (22 * 20 + 5 + 3, [0.4375, -0.1875, 0.093_75, 0.0]),
        // (0, 1, 1): Cy.
        (14 * 20 + 2 * 5 + 1, [-0.0625, 0.218_75, -0.406_25, 0.0]),
        // (1, 0, 0) does not count, (2, 0, 1) does and is black.
        (20 + 5 + 2, [0.0; 4]),
        (2 * 20 + 5 + 3, [0.0, 0.0, 0.0, 1.0]),
    ];
    by_hand.iter().all(|(texel, want)| {
        let halves = bytes[8 * texel..8 * texel + 8].as_chunks::<2>().0;
        halves.iter().zip(want).all(|(h, want)| (half::f16::from_le_bytes(*h).to_f32() - want).abs() < 1e-3)
    })
}

/// The bytes are the fixture's, by an index of their own; two slabs, or two axes, the other way round
/// are not. The uniform has each member in its place.
#[test]
fn the_made_volume_is_packed_and_described() {
    let v = made(LOWS[1]);
    assert!(packed_right(&v.rgba16f_le));
    let slab = 6 * 160;
    for (a, b) in [(1, 2), (1, 3), (2, 3)] {
        let mut swapped = v.rgba16f_le.clone();
        let (low, high) = swapped.split_at_mut(b * slab);
        low[a * slab..(a + 1) * slab].swap_with_slice(&mut high[..slab]);
        assert!(!packed_right(&swapped), "slabs {a} and {b}");
    }
    // y before x.
    let texel = |i: usize| v.rgba16f_le[8 * i..8 * i + 8].to_vec();
    let turned: Vec<u8> = (0..24 * 20).flat_map(|i| texel(i / 20 * 20 + (i % 4) * 5 + i % 20 / 4)).collect();
    assert!(!packed_right(&turned));

    let u = RtgiVolumeUniform::new(&v, Fit { b: 0.3, g: 1.25, pay: 0.8, least: 0.2, pull: 0.4 }, 0.7);
    assert_eq!(u.origin, Vec4::new(-64.0, 192.0, -128.0, 1.0 / 64.0));
    assert_eq!(u.dimensions, Vec4::new(3.0, 2.0, 4.0, 24.0));
    assert_eq!(u.atlas, Vec4::new(1.0 / 5.0, 1.0 / 4.0, 1.0 / 24.0, 6.0));
    // pay, least, thin, pull; b, g, share.
    assert_eq!(u.fit, Vec4::new(0.8, 0.2, 0.25, 0.4));
    assert_eq!(u.light, Vec4::new(0.3, 1.25, 0.7, 0.0));
}

/// The reads are of the kinds they are said to be, some are clamped, and none is let off.
#[test]
fn the_reads_are_what_they_say() {
    let v = made(LOWS[2]);
    let reader = Reader::new(&v).unwrap();
    let all = checks(&v).unwrap();
    assert_eq!((all.len(), READS % 32), (READS, 0), "whole workgroups");
    let mut clamped = 0;
    for c in &all {
        let r = reader.read(V3(c.p), V3(c.g), V3(c.n));
        clamped += usize::from(r.raw.0.iter().any(|e| *e < 0.0));
        let fits = match c.kind {
            Kind::Exact | Kind::Centre => r.support < 1e-4 || r.support > 0.9999,
            Kind::Fraction => r.support > 0.25,
            Kind::Thin => r.support > 0.0 && r.support < 0.25,
            Kind::Shell => r.support > 0.0 && r.support < 0.75,
        };
        assert!(fits && c.bound.iter().all(|b| *b >= 1e-4 && *b < 0.15), "{c:?} {r:?}");
    }
    assert!(clamped > 0 && clamped < 20, "{clamped}");
    let of = |kind| all.iter().filter(|c| c.kind == kind).count();
    assert_eq!(KINDS.map(|k| of(k.0)), [28, 20, 24, 12, 12]);
    // Only an exact centre has the arithmetic's bound alone.
    assert!(all.iter().all(|c| (c.kind == Kind::Exact) == (0..4).all(|k| c.bound[k] == 1e-4 + c.want[k].abs() / 2048.0)));
}

/// The shader's read is the CPU's, wherever the volume is.
#[test]
fn the_mirror_reads_as_the_reader() {
    for low in LOWS {
        let v = made(low);
        let u = RtgiVolumeUniform::new(&v, FIT, 1.0);
        for c in checks(&v).unwrap() {
            let (e, of_v) = read(&v, &u, &c, exact);
            let off = (e.extend(of_v) - Vec4::from_array(c.want)).abs().max_element();
            assert!(off < 1e-4, "{c:?}: {e} {of_v}");
        }
    }
}

/// The uniform of nothing gives the light that was, exactly, whatever is read: with no support, with
/// thin and with full support, and outside (a `thin` of 0 gives no number where there is none). So
/// does a share of 0; a share between is between.
#[test]
fn the_stock_uniform_is_the_stock_light() {
    let v = made(LOWS[0]);
    let full = RtgiVolumeUniform::new(&v, FIT, 1.0);
    let empty = RtgiVolumeUniform::empty();
    assert_eq!((empty.fit.z, empty.fit.w, empty.light.to_array()), (0.25, 0.0, [1.0, 0.0, 0.0, 0.0]));
    // Its constants with a place to read.
    let stock = RtgiVolumeUniform { fit: empty.fit, light: empty.light, ..full };
    let [off, half_way] = [0.0, 0.5].map(|share| RtgiVolumeUniform::new(&v, FIT, share));
    let mut changed = 0;
    for c in checks(&v).unwrap() {
        for sum in [Vec3::ZERO, Vec3::new(0.3, 0.2, 0.7), Vec3::new(0.0, 0.9, 1.6), Vec3::splat(0.004)] {
            let of = |u: &RtgiVolumeUniform| light(u, 0.8 * sum, sum, Vec3::new(0.5, 0.7, 1.0), read(&v, u, &c, exact));
            let was = linear(sum);
            assert!(of(&stock) == was && of(&empty) == was && of(&off) == was, "{c:?} {sum}");
            let all = of(&full);
            assert!((of(&half_way) - 0.5 * (was + all)).abs().max_element() < 1e-5);
            changed += usize::from((all - was).abs().max_element() > 1e-3);
        }
    }
    assert!(changed > 100, "the full light is another one: {changed}");
}

/// The light at full share is the frozen formula, from numbers worked out by hand.
#[test]
fn the_light_is_the_formula() {
    let close = |a: Vec3, b: Vec3| assert!((a - b).abs().max_element() < 1e-5, "{a} {b}");
    let u = |pull| RtgiVolumeUniform { fit: Vec4::new(2.0, 0.25, 0.25, pull), light: Vec4::new(0.25, 0.5, 1.0, 0.0), ..default() };
    let of = |pull| light(&u(pull), Vec3::splat(0.4), Vec3::splat(0.6), Vec3::splat(0.5), (Vec3::new(0.2, 0.4, 0.0), 0.2));
    // k = 0.25 + 0.75 x 0.4 = 0.55; the sum less 0.75 x 0.55 x 0.4 of the ambient is 0.435.
    let want = Vec3::splat(0.435f32.powf(2.2)) + Vec3::new(0.05, 0.1, 0.0);
    close(of(0.0), want);
    // With all of the pull the colour is the one before, at the new luminance.
    close(of(1.0), Vec3::splat(want.dot(Vec3::new(0.2126, 0.7152, 0.0722))));
    // Without support: 81.25 % of a sum that is all ambient.
    close(light(&u(0.0), Vec3::splat(0.6), Vec3::splat(0.6), Vec3::ONE, (Vec3::ZERO, 0.0)), Vec3::splat((0.8125f32 * 0.6).powf(2.2)));
}

fn bytes(floats: &[f32]) -> Vec<u8> {
    floats.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// What the readback lets through and what it does not: the same numbers and 8 fractional bits,
/// rounded, cut or up, pass; a wrong slab, place or support and no filter do not.
#[test]
fn the_readback_tells_a_wrong_read() {
    let v = made(LOWS[1]);
    let all = checks(&v).unwrap();
    let right = RtgiVolumeUniform::new(&v, FIT, 1.0);
    let judged = |u: &RtgiVolumeUniform, filter: fn(f32) -> f32| {
        let found: Vec<f32> = all.iter().map(|c| read(&v, u, c, filter)).flat_map(|(e, of_v)| e.extend(of_v).to_array()).collect();
        judge(&bytes(&found), &all).unwrap()
    };
    assert!(passes(&judged(&right, exact)));
    for filter in COARSE {
        let w = judged(&right, filter);
        assert!(passes(&w) && w[0] < 0.1 && w[2] > 0.01, "8 bits: {w:?}");
    }
    assert!(passes(&[1.0; 5]) && !passes(&[0.0, 1.0004, 0.0]) && !passes(&[f32::NAN]), "not as it is shown");
    type Change = fn(&mut RtgiVolumeUniform);
    let changes: [(&str, Change); 4] =
        [("slab", |u| u.atlas.w = 5.0), ("offset", |u| u.dimensions.w = 0.0), ("origin", |u| u.origin.y += 1.0), ("spacing", |u| u.origin.w = 1.0 / 65.0)];
    for (what, change) in changes {
        let mut u = right;
        change(&mut u);
        assert!(judged(&u, exact)[0] > 1.0, "{what}");
    }
    let mut thinner = right;
    thinner.fit.z = 0.2;
    assert!(judged(&thinner, exact)[3] > 1.0, "the support");
    let nearest = judged(&right, f32::round);
    assert!(passes(&nearest[..2]) && nearest[2..].iter().all(|w| *w > 1.0), "{nearest:?}");
    // Not a number, nothing written, and not as many as there are to be.
    assert_eq!(judge(&bytes(&[f32::NAN; 4 * READS]), &all), Some([f32::INFINITY; 5]));
    assert!(!passes(&judge(&bytes(&[0.0; 4 * READS]), &all).unwrap()));
    for n in [0, 4 * READS - 1, 4 * READS + 4] {
        assert_eq!(judge(&bytes(&vec![0.0; n]), &all), None);
    }
}

/// The readback is of one bound key: made when that is bound, read when its pass is on its way, gone
/// with the request, the binding or the pipeline.
#[test]
fn the_readback_goes_with_its_key() {
    let place = Place { cell: 5, travels: 1, game: None };
    let key = plan::Key { place, revision: 1, solved: true };
    let other = plan::Key { place: Place { travels: 2, ..place }, ..key };
    let said = |bound, dispatched, failed| Said { bound, dispatched, ended: None, failed };
    let bound = said(Some(key), None, false);
    let ended = |place: Place| Said { ended: Some(place), ..default() };
    for (made, wanted, said, want) in [
        ((false, false), Some(key), said(None, None, false), Step::Wait),
        ((false, false), Some(key), said(Some(other), Some(other), false), Step::Wait),
        ((false, false), Some(key), bound, Step::Make),
        ((true, false), Some(key), bound, Step::Wait),
        ((true, false), Some(key), said(Some(key), Some(other), false), Step::Wait),
        ((true, false), Some(key), said(Some(key), Some(key), false), Step::Read),
        ((true, true), Some(key), said(Some(key), Some(key), false), Step::Wait),
        ((true, true), Some(key), said(None, Some(key), false), Step::Retire),
        ((true, false), Some(key), said(Some(other), None, false), Step::Retire),
        ((true, false), Some(key), said(Some(key), None, true), Step::Retire),
        // Refused, late or failed before it was made; another visit's end is none of it.
        ((false, false), Some(key), ended(place), Step::Retire),
        ((false, false), Some(key), ended(other.place), Step::Wait),
        ((false, false), Some(other), bound, Step::Retire),
        ((true, true), None, bound, Step::Retire),
    ] {
        assert_eq!(step(key, made, wanted, &said), want, "{made:?} {wanted:?} {said:?}");
    }
}

/// One answer ends a job, good or malformed, while it is the request's and its own reader's; after a
/// travel it says nothing.
#[test]
fn an_answer_counts_for_its_own_request() {
    let place = Place { cell: 5, travels: 1, game: None };
    let (request, next) = (RtgiRequest::empty(place, 1), RtgiRequest::empty(Place { travels: 2, ..place }, 1));
    let all = checks(&made(LOWS[0])).unwrap();
    for (now, own, floats, done) in [
        (Some(&request), true, 4 * READS, true),
        (Some(&request), true, 3, true),
        (Some(&next), true, 4 * READS, false),
        (None, true, 4 * READS, false),
        (Some(&request), false, 4 * READS, false),
    ] {
        let mut world = World::new();
        let reader = world.spawn_empty().observe(complete).id();
        let of = if own { reader } else { world.spawn_empty().id() };
        world.insert_resource(Aid { done: false, job: Some(Job { key: request.key(), checks: all.clone(), buffers: None, reader: Some(of) }) });
        if let Some(now) = now {
            world.insert_resource(now.clone());
        }
        world.trigger(ReadbackComplete { entity: reader, data: bytes(&vec![0.0; floats]) });
        world.flush();
        let aid = world.resource::<Aid>();
        assert_eq!((aid.done, aid.job.is_none(), world.get_entity(reader).is_err()), (done, done, true));
    }
}

/// The pass's shader composes and validates, and its layouts have what it declares: the view's three
/// bindings by their numbers, and the buffers.
#[test]
fn the_readback_shader_composes() {
    let declared = tests::bindings(&compose(&tests::modules(true), READBACK, &[]));
    assert_eq!(declared, [(0, 47), (0, 48), (0, 49), (1, 0), (1, 1)].into());
    let layouts = layouts();
    let laid: BTreeSet<_> = layouts.iter().enumerate().flat_map(|(g, l)| l.entries.iter().map(move |e| (g as u32, e.binding))).collect();
    assert_eq!(laid, declared);
    assert!(layouts.iter().flat_map(|l| &l.entries).all(|e| e.visibility == wgpu::ShaderStages::COMPUTE));
    assert_eq!(RtgiVolumeUniform::min_size().get(), 80);
}
