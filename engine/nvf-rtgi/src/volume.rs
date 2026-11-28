//! The bounce light of the points that count, as a texture, and the read a shader makes of it.
//!
//! The texture is 3D, rgba16float, x fastest, then y, then z, without row padding. It holds four slabs one
//! after another along z, each of the grid's dimensions with a texel of zeros all around:
//!
//! ```text
//! slab 0: C0, m    slab 1: Cx, m v    slab 2: Cy, 0    slab 3: Cz, 0
//! ```
//!
//! m is 1 at a point that is reached and in the air, else 0, and all else is 0 there (premultiplied). C0 = mean(L),
//! Cj = 2 mean(L wj) over the sphere (L linear RGB, w towards the source), v the luminance of C0. [`Reader`] has
//! the read.

use crate::failed::{go_on, unavailable, Cancel, Failed, Reason};
use crate::gather::Moments;
use crate::grid::{Place, STEPS};
use crate::reach::SPACINGS;
use crate::solve::SolvedVolume;
use crate::vec3::{luminance, V3};

/// The texture's slabs, along z.
pub const SLABS: usize = 4;
/// The bytes of a texel.
const TEXEL: usize = 8;
/// The support that is taken as all there is.
pub const THIN: f32 = 0.25;
/// A side of the texture may not be larger (wgpu's default for a 3D one).
const MOST_SIDE: u32 = 2048;
/// The largest half float.
const MOST_HALF: f32 = 65_504.0;

/// The nearest half float, ties to even; `None` outside its range.
pub fn to_half(x: f32) -> Option<u16> {
    let v = x.abs();
    if !x.is_finite() || v > MOST_HALF {
        return None;
    }
    let sign = ((x.to_bits() >> 16) & 0x8000) as u16;
    if v < f32::from_bits(0x3880_0000) {
        // Below 2^-14: steps of 2^-24. 1024 of them are the first normal one.
        return Some(sign | (v * 16_777_216.0).round_ties_even() as u16);
    }
    let bits = v.to_bits();
    let (kept, rest) = ((bits >> 13) - (112 << 10), bits & 0x1FFF);
    let up = rest > 0x1000 || (rest == 0x1000 && kept & 1 == 1);
    Some(sign | (kept + u32::from(up)) as u16)
}

pub fn from_half(h: u16) -> f32 {
    let sign = u32::from(h & 0x8000) << 16;
    let (e, m) = (u32::from((h >> 10) & 0x1F), u32::from(h & 0x3FF));
    let size = match e {
        0 => (m as f32 / 16_777_216.0).to_bits(),
        31 => 0x7F80_0000 | (m << 13),
        _ => ((e + 112) << 23) | (m << 13),
    };
    f32::from_bits(sign | size)
}

/// The texture's size for a grid's; `None` of one that is too large.
pub fn atlas_of(dimensions: [u32; 3]) -> Option<[u32; 3]> {
    let padded = dimensions.map(|d| d.checked_add(2).filter(|p| *p > 2 && *p <= MOST_SIDE));
    let out = [padded[0]?, padded[1]?, padded[2]?.checked_mul(SLABS as u32)?];
    (out[2] <= MOST_SIDE).then_some(out)
}

/// Its bytes.
pub fn bytes_of(atlas: [u32; 3]) -> u64 {
    atlas.iter().map(|s| u64::from(*s)).product::<u64>() * TEXEL as u64
}

/// The texture's size and bytes, where both may be.
pub fn allowed(dimensions: [u32; 3], most_bytes: u64) -> Result<([u32; 3], usize), Failed> {
    let atlas = atlas_of(dimensions).ok_or(Failed::Unavailable(Reason::TooLarge { dimensions }))?;
    let bytes = bytes_of(atlas);
    // What a `Vec` may hold, which is half of what an address may be.
    match isize::try_from(bytes) {
        Ok(size) if bytes <= most_bytes => Ok((atlas, size as usize)),
        _ => unavailable(Reason::TooManyBytes { bytes, limit: most_bytes }),
    }
}

/// The texture of the points in a box from `low`, little-endian; its bytes are looked at before any are asked for.
pub fn packed<'a>(low: Place, dimensions: [u32; 3], points: impl Iterator<Item = (&'a Place, &'a Moments)>, most_bytes: u64, cancel: Cancel) -> Result<Vec<u8>, Failed> {
    let (atlas, bytes) = allowed(dimensions, most_bytes)?;
    let atlas = atlas.map(|s| s as usize);
    go_on(cancel)?;
    let mut out = vec![0u8; bytes];
    for (i, (at, m)) in points.enumerate() {
        if i.is_multiple_of(STEPS) {
            go_on(cancel)?;
        }
        let v = luminance(m.c[0].0);
        for (slab, (c, a)) in m.c.iter().zip([1.0, v, 0.0, 0.0]).enumerate() {
            let [x, y, z]: [usize; 3] = std::array::from_fn(|k| (at[k] - low[k]) as usize + 1);
            let z = z + slab * (dimensions[2] as usize + 2);
            for (k, from) in [c[0], c[1], c[2], a].into_iter().enumerate() {
                let half = to_half(from).ok_or(Failed::Unavailable(Reason::OutOfRange { at: *at }))?;
                let to = ((z * atlas[1] + y) * atlas[0] + x) * TEXEL + 2 * k;
                out[to..to + 2].copy_from_slice(&half.to_le_bytes());
            }
        }
    }
    Ok(out)
}

/// What a [`Reader`] finds at a surface: the numbers a shader has before it lights it. Outside the volume all are 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Read {
    /// The bounce light on the surface, linear, a channel of which may be below 0: two bands undershoot.
    pub raw: V3,
    /// The luminance of the light around, summed over the points that count by their trilinear parts.
    pub faded: f32,
    /// The sum of those parts: 1 in the middle of the air, 0 where no point counts.
    pub support: f32,
}

impl Read {
    /// From the four slabs, filtered.
    fn of(slabs: [[f32; 4]; SLABS], n: V3) -> Read {
        let [c0, cx, cy, cz] = slabs;
        let (support, faded) = (c0[3], cx[3]);
        let w = 1.0 / support.max(THIN);
        let raw = V3(std::array::from_fn(|k| w * (c0[k] + cx[k] * n[0] + cy[k] * n[1] + cz[k] * n[2])));
        Read { raw, faded, support }
    }

    /// The bounce light on the surface, linear: E.
    pub fn e(&self) -> V3 {
        self.raw.max(V3::ZERO)
    }

    /// The luminance of the light around: v. Thin support fades it out.
    pub fn v(&self) -> f32 {
        self.faded / self.support.max(THIN)
    }
}

/// The read of a volume on the CPU: what a shader has to find. It reads the volume's own bytes
/// ([`SolvedVolume::rgba16f_le`]). At P with the geometric normal G (both on the side the camera sees) and the shading
/// normal n, one filtered read a slab, all at the same place:
///
/// ```text
/// q = (P + offset G - origin) / spacing          outside -1 < q < dimensions: all is 0
/// t = (q + 1.5 + (0, 0, s (dimensions.z + 2))) / atlas         of slab s, linear, clamped to the edge
/// support = m;  w = 1 / max(support, 0.25)
/// E = max(w (C0 + Cx n.x + Cy n.y + Cz n.z), 0)       v = w (m v)
/// ```
///
/// `origin` is the first point that is not of the shell, and every point is a multiple of the spacing. E is the
/// irradiance of the light around over pi, as far as two bands hold it: what a white surface sends back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reader<'a> {
    origin: V3,
    spacing: f32,
    offset: f32,
    dimensions: [u32; 3],
    atlas: [u32; 3],
    rgba16f_le: &'a [u8],
}

/// Why a volume is not read.
#[derive(Clone, Debug, PartialEq)]
pub enum Refused {
    Cancelled,
    Because(String),
}

impl<'a> Reader<'a> {
    /// Refuses one whose texels are not as many as they have to be, not finite, not zero where they are
    /// to be, not valid as often as it says, or whose counts are not those of its box.
    pub fn new(v: &'a SolvedVolume) -> Result<Reader<'a>, String> {
        match Reader::scanned(v, &|| false, |_| ()) {
            Ok(reader) => Ok(reader),
            Err(Refused::Because(why)) => Err(why),
            Err(Refused::Cancelled) => Err("cancelled".into()),
        }
    }

    /// [`Reader::new`], which gives `each` valid place on the way: none of them counts for anything if it is refused.
    pub(crate) fn scanned(v: &'a SolvedVolume, cancel: Cancel, each: impl FnMut(Place)) -> Result<Reader<'a>, Refused> {
        let no = |why: &str| Err(Refused::Because(why.into()));
        let origin = v.low.map(|c| c as f32 * v.spacing);
        if !(SPACINGS.contains(&v.spacing) && v.offset.is_finite() && V3(origin).finite() && v.origin == origin) {
            return no("the spacing, the offset or the origin");
        }
        if atlas_of(v.dimensions) != Some(v.atlas) || v.rgba16f_le.len() as u64 != bytes_of(v.atlas) {
            return no("the dimensions, the atlas or the bytes");
        }
        if (0..3).any(|k| v.low[k].checked_add((v.dimensions[k] - 1) as i32).is_none()) {
            return no("the box's last place");
        }
        let n = &v.counts;
        let all = [n.reached, n.unreached, n.inside, n.no_hit, n.unknown].iter().try_fold(0usize, |s, n| s.checked_add(*n));
        if n.points as u64 != v.dimensions.iter().map(|d| u64::from(*d)).product::<u64>() || all != Some(n.points) {
            return no("the counts");
        }
        let out = Reader { origin: V3(v.origin), spacing: v.spacing, offset: v.offset, dimensions: v.dimensions, atlas: v.atlas, rgba16f_le: &v.rgba16f_le };
        let valid = out.check(v.low, cancel, each)?;
        if valid != n.reached {
            return no(&format!("{valid} valid texels, not {}", n.reached));
        }
        Ok(out)
    }

    /// How many texels are valid, each one's place by the first one's, `low`.
    fn check(&self, low: Place, cancel: Cancel, mut each: impl FnMut(Place)) -> Result<usize, Refused> {
        let mut valid = 0;
        for i in 0..self.rgba16f_le.len() / TEXEL / SLABS {
            if i.is_multiple_of(STEPS) && cancel() {
                return Err(Refused::Cancelled);
            }
            let (w, h) = (self.atlas[0] as usize, self.atlas[1] as usize);
            let at = [i % w, i / w % h, i / (w * h)];
            let t = self.slabs(at);
            if let Some(t) = t.iter().flatten().find(|t| !t.is_finite()) {
                return Err(Refused::Because(format!("{t} in the texture")));
            }
            let shell = (0..3).any(|k| at[k] == 0 || at[k] == self.dimensions[k] as usize + 1);
            let m = t[0][3];
            let black = || t.iter().flatten().all(|v| *v == 0.0);
            let fine = if m == 1.0 { !shell && t[1][3] >= 0.0 && t[2][3] == 0.0 && t[3][3] == 0.0 } else { m == 0.0 && black() };
            if !fine {
                return Err(Refused::Because(format!("the texel {at:?} (of the shell: {shell}): {t:?}")));
            }
            if m == 1.0 {
                valid += 1;
                each(std::array::from_fn(|k| low[k] + (at[k] as i32 - 1)));
            }
        }
        Ok(valid)
    }

    /// One of those there are.
    fn texel(&self, i: usize) -> [f32; 4] {
        let b = &self.rgba16f_le[i * TEXEL..(i + 1) * TEXEL];
        std::array::from_fn(|k| from_half(u16::from_le_bytes([b[2 * k], b[2 * k + 1]])))
    }

    /// The four texels of a place in a padded slab, which has to be in it.
    pub(crate) fn slabs(&self, at: [usize; 3]) -> [[f32; 4]; SLABS] {
        let [w, h, _] = self.atlas.map(|s| s as usize);
        let deep = self.dimensions[2] as usize + 2;
        std::array::from_fn(|s| self.texel(((at[2] + s * deep) * h + at[1]) * w + at[0]))
    }

    /// Where a point is on the grid, the first one (not of the shell) at 0.
    fn place(&self, p: V3) -> V3 {
        (p - self.origin) * (1.0 / self.spacing)
    }

    /// The texture coordinates (0 to 1) of a slab.
    fn coordinates(&self, q: V3, slab: usize) -> V3 {
        let deep = (self.dimensions[2] + 2) as f32;
        let t = q + V3([1.5; 3]) + V3::new(0.0, 0.0, slab as f32 * deep);
        V3(std::array::from_fn(|k| t[k] / self.atlas[k] as f32))
    }

    /// The texture there as hardware filters it, but with whole weights; clamped to its edge.
    fn sample(&self, t: V3) -> [f32; 4] {
        let side = self.atlas.map(|s| s as usize);
        let x: [f32; 3] = std::array::from_fn(|k| t[k] * side[k] as f32 - 0.5);
        let low = x.map(|x| x.floor());
        let mut out = [0.0; 4];
        for i in 0..8 {
            let (mut part, mut at) = (1.0, [0usize; 3]);
            for k in 0..3 {
                let up = (i >> k) & 1;
                let share = x[k] - low[k];
                part *= if up == 1 { share } else { 1.0 - share };
                at[k] = (low[k] + up as f32).clamp(0.0, (side[k] - 1) as f32) as usize;
            }
            let texel = self.texel((at[2] * side[1] + at[1]) * side[0] + at[0]);
            out = std::array::from_fn(|c| out[c] + part * texel[c]);
        }
        out
    }

    /// At a surface: the place on it, its geometric normal on the side the camera sees, and its shading normal.
    pub fn read(&self, p: V3, g: V3, n: V3) -> Read {
        self.read_at(p + g * self.offset, n)
    }

    /// At a place that is offset already.
    pub fn read_at(&self, p: V3, n: V3) -> Read {
        let q = self.place(p);
        if (0..3).any(|k| !(q[k] > -1.0 && q[k] < self.dimensions[k] as f32)) {
            return Read::default();
        }
        Read::of(std::array::from_fn(|s| self.sample(self.coordinates(q, s))), n)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::bvh::tests::direction;
    use crate::gather::tests::axis;
    use crate::solve::Counts;
    use crate::vec3::tests::Rng;
    use std::collections::BTreeMap;

    const UP: V3 = V3([0.0, 1.0, 0.0]);

    /// Points from `low` in a box, `f` of each.
    pub fn field(low: Place, dimensions: [u32; 3], f: impl Fn(Place) -> Option<[V3; 4]>) -> (Place, [u32; 3], BTreeMap<Place, Moments>) {
        let mut points = BTreeMap::new();
        for i in 0..dimensions.iter().product::<u32>() {
            let at = [i % dimensions[0], i / dimensions[0] % dimensions[1], i / (dimensions[0] * dimensions[1])];
            let at: Place = std::array::from_fn(|k| low[k] + at[k] as i32);
            if let Some(c) = f(at) {
                points.insert(at, Moments { c });
            }
        }
        (low, dimensions, points)
    }

    fn bytes(low: Place, dimensions: [u32; 3], points: &BTreeMap<Place, Moments>) -> Result<Vec<u8>, Failed> {
        packed(low, dimensions, points.iter(), 1 << 20, &|| false)
    }

    pub fn volume((low, dimensions, points): &(Place, [u32; 3], BTreeMap<Place, Moments>)) -> SolvedVolume {
        let all = dimensions.iter().product::<u32>() as usize;
        let counts = Counts { points: all, reached: points.len(), unknown: all - points.len(), ..Counts::default() };
        SolvedVolume {
            low: *low,
            origin: low.map(|c| c as f32 * 64.0),
            spacing: 64.0,
            offset: 24.0,
            emission: 0.25,
            dimensions: *dimensions,
            atlas: atlas_of(*dimensions).unwrap(),
            rgba16f_le: bytes(*low, *dimensions, points).unwrap(),
            counts,
            coverage: Default::default(),
            stats: Default::default(),
        }
    }

    fn reader(low: Place, dimensions: [u32; 3], f: impl Fn(Place) -> Option<[V3; 4]>) -> Reader<'static> {
        Reader::new(Box::leak(Box::new(volume(&field(low, dimensions, f))))).unwrap()
    }

    pub fn even(c: [f32; 3]) -> [V3; 4] {
        [V3(c), V3::ZERO, V3::ZERO, V3::ZERO]
    }

    fn close(a: V3, b: V3) -> bool {
        (a - b).max_abs() < 1e-3
    }

    #[test]
    fn halves() {
        for h in 0..=u16::MAX {
            let f = from_half(h);
            if f.is_finite() {
                assert_eq!(to_half(f), Some(h), "{f}");
            }
        }
        assert!(from_half(0x7C00).is_infinite() && from_half(0x7C01).is_nan() && from_half(0xFBFF) == -65_504.0);
        assert_eq!([1.0, -2.0, 0.5, 65_504.0, 0.0, -0.0, -1e-12].map(|f| to_half(f).unwrap()), [0x3C00, 0xC000, 0x3800, 0x7BFF, 0, 0x8000, 0x8000]);
        // Ties go to the even one, the rest to the nearest.
        let step = 1.0 / 1024.0;
        assert_eq!([1.0 + 0.5 * step, 1.0 + 1.5 * step, 1.0 + 0.51 * step].map(to_half), [Some(0x3C00), Some(0x3C02), Some(0x3C01)]);
        assert_eq!([2.0f32.powi(-25), 1.5 * 2.0f32.powi(-24), 1e-12, 2.0f32.powi(-14) * 0.99999].map(to_half), [Some(0), Some(2), Some(0), Some(0x400)]);
        assert!([65_505.0, -70_000.0, f32::INFINITY, f32::NAN].iter().all(|f| to_half(*f).is_none()));
        let mut rng = Rng(8);
        for _ in 0..1000 {
            let x = (rng.draw() - 0.5) * 8.0;
            assert!((from_half(to_half(x).unwrap()) - x).abs() <= x.abs() / 2048.0 + 3e-8, "{x}");
        }
    }

    /// An even field is the same anywhere inside and on every side; nothing is outside, and half the
    /// support is there half way through the shell, where a quarter is still taken as all there is.
    #[test]
    fn even_fields() {
        let c = [0.5, 0.25, 0.75];
        let v = reader([-3, 1, -2], [4, 3, 5], |_| Some(even(c)));
        let mut rng = Rng(2);
        for _ in 0..200 {
            let p = V3(std::array::from_fn(|k| v.origin[k] + 64.0 * rng.draw() * (v.dimensions[k] - 1) as f32));
            let r = v.read_at(p, direction(&mut rng));
            assert!(close(r.e(), V3(c)) && (r.support - 1.0).abs() < 1e-4 && (r.v() - luminance(c)).abs() < 1e-3, "{p:?} {r:?}");
        }
        let at = |x: f32| v.read_at(V3::new(-192.0 + x, 64.0, -128.0), UP);
        let half = at(-32.0);
        assert!((half.support - 0.5).abs() < 1e-4 && close(half.e(), V3(c)), "{half:?}");
        let thin = at(-56.0);
        assert!((thin.support - 0.125).abs() < 1e-4 && close(thin.e(), V3(c) * 0.5), "{thin:?}");
        assert!([-64.0, -65.0, 256.0, 1e9].iter().all(|x| at(*x) == Read::default()));
        assert!(v.read_at(V3::new(0.0, -1e9, f32::NAN), UP) == Read::default());
    }

    /// L = 0.5 + 0.3 wx: 0.7 and 0.3, through the texture; below 0 is clamped after the filter alone.
    #[test]
    fn sides_of_a_field() {
        let mut c = even([0.5, 0.5, 0.0]);
        c[1] = V3::new(0.2, 0.0, 0.0);
        let v = reader([0; 3], [2, 2, 2], |_| Some(c));
        let at = |n: V3| v.read_at(V3([20.0, 32.0, 40.0]), n).e();
        assert!(close(at(axis(0)), V3::new(0.7, 0.5, 0.0)) && close(at(axis(1)), V3::new(0.3, 0.5, 0.0)) && close(at(UP), V3::new(0.5, 0.5, 0.0)));
        // -0.5 and 1.5 for a normal of +x: their mean is 0.5, with each clamped it would be 0.75.
        let v = reader([0; 3], [2, 1, 1], |at| Some([V3([0.5; 3]), V3([if at[0] == 0 { -1.0 } else { 1.0 }; 3]), V3::ZERO, V3::ZERO]));
        assert!(close(v.read_at(V3::new(32.0, 0.0, 0.0), axis(0)).e(), V3([0.5; 3])));
        let low = v.read_at(V3::ZERO, axis(0));
        assert!(low.raw[0] < -0.49 && low.e() == V3::ZERO, "{low:?}");
    }

    /// One corner is valid: what is there is divided by its support, by no less than a quarter, once.
    #[test]
    fn thin_support() {
        let v = reader([0; 3], [2, 2, 2], |at| (at == [1, 1, 1]).then(|| even([0.8; 3])));
        for (x, support) in [(0.0, 0.0), (0.125, 0.125), (0.25, 0.25), (0.5, 0.5), (1.0, 1.0)] {
            let r = v.read_at(V3::new(64.0 * x, 64.0, 64.0), UP);
            let want = 0.8 * f32::min(support / 0.25, 1.0);
            assert!((r.support - support).abs() < 1e-5 && close(r.e(), V3([want; 3])) && (r.v() - want).abs() < 1e-3, "{r:?}");
        }
    }

    /// The read is made the offset up the geometric normal, whatever the shading normal.
    #[test]
    fn offsets() {
        let v = reader([0; 3], [3, 1, 1], |at| Some(even([at[0] as f32 + 1.0; 3])));
        let p = V3::new(40.0, 0.0, 0.0);
        for (g, x) in [(axis(0), 64.0), (axis(1), 16.0), (axis(2), 40.0)] {
            assert!((v.read(p, g, UP).raw[0] - v.read_at(V3::new(x, 24.0 * g[1], 0.0), UP).raw[0]).abs() < 1e-6);
        }
        assert!((v.read(p, axis(0), UP).raw[0] - 2.0).abs() < 2e-3 && v.read(V3::new(-80.0, 0.0, 0.0), axis(1), UP) == Read::default());
    }

    /// A point's own value at it and its parts between, left of 0 too; nothing of another slab.
    #[test]
    fn corners_and_seams() {
        let value = |at: Place| (at[0] + 10) as f32 + 0.5 * at[1] as f32 + 0.25 * at[2] as f32;
        let f = field([-2, -1, -3], [3, 2, 3], |at| Some([V3([value(at); 3]), V3([0.25; 3]), V3([-0.5; 3]), V3([-1.0; 3])]));
        let whole = volume(&f);
        let v = Reader::new(&whole).unwrap();
        for at in f.2.keys() {
            let r = v.read_at(V3(at.map(|c| c as f32 * 64.0)), V3::ZERO);
            assert!((r.raw[0] - value(*at)).abs() < 0.02 && (r.support - 1.0).abs() < 1e-4, "{at:?} {r:?}");
        }
        // At -80 the corners are -2 and -1, by 0.25 and 0.75: the value is linear.
        let r = v.read_at(V3::new(-80.0, -32.0, -144.0), V3::ZERO);
        assert!((r.raw[0] - (10.0 - 1.25 - 0.25 - 0.5625)).abs() < 0.02, "{r:?}");
        // By the normals each slab's own, at the ends of z as in its middle.
        for z in [-192.0, -160.0, -64.0] {
            let at = |n: V3| v.read_at(V3::new(-64.0, 0.0, z), n).raw[0] - v.read_at(V3::new(-64.0, 0.0, z), V3::ZERO).raw[0];
            assert!((at(axis(0)) - 0.25).abs() < 0.01 && (at(axis(2)) + 0.5).abs() < 0.01 && (at(axis(4)) + 1.0).abs() < 0.01, "{z}");
        }
        // Half way into the shell the support is a half for every slab: the next one's is not read.
        for z in [-224.0, -32.0] {
            let (r, plain) = (v.read_at(V3::new(-64.0, 0.0, z), axis(4)), v.read_at(V3::new(-64.0, 0.0, z), V3::ZERO));
            assert!((r.support - 0.5).abs() < 1e-4 && (r.raw[0] - plain.raw[0] + 1.0).abs() < 0.01 && plain.raw[0] > 8.0, "{z} {r:?}");
        }
    }

    /// The read is the trilinear sum of the decoded texels themselves, taken by hand at random places.
    #[test]
    fn the_reader_against_the_texels() {
        let low: Place = [-1, 0, 2];
        let v = reader(low, [3, 2, 2], |at| (at != [0, 1, 2]).then(|| [V3([0.1 * (at[0] + 2) as f32; 3]), V3([0.02; 3]), V3([0.01 * at[2] as f32; 3]), V3([-0.03; 3])]));
        let mut rng = Rng(5);
        for _ in 0..200 {
            let p = V3(std::array::from_fn(|k| v.origin[k] - 64.0 + 64.0 * rng.draw() * (v.dimensions[k] + 1) as f32));
            let n = direction(&mut rng);
            let mut sum = [[0.0f32; 4]; SLABS];
            for i in 0..8 {
                let mut part = 1.0;
                let at = std::array::from_fn(|k| {
                    let q = p[k] / 64.0 - (low[k] - 1) as f32;
                    let up = (i >> k) & 1;
                    part *= if up == 1 { q.fract() } else { 1.0 - q.fract() };
                    q as usize + up
                });
                if part > 0.0 {
                    let t = v.slabs(at);
                    sum = std::array::from_fn(|s| std::array::from_fn(|c| sum[s][c] + part * t[s][c]));
                }
            }
            let (a, b) = (v.read_at(p, n), Read::of(sum, n));
            assert!(close(a.raw, b.raw) && (a.support - b.support).abs() < 1e-4 && (a.faded - b.faded).abs() < 1e-4, "{p:?} {a:?} {b:?}");
        }
    }

    /// The whole way by hand: two points at (64, 0, -64) and (128, 0, -64), their half floats put where the layout
    /// says, then what is read. The texture is 4 x 3 x 12, slab s begins at z = 3 s, so the first point is texel
    /// ((1 + 3 s) 3 + 1) 4 + 1 and the second the one after it.
    #[test]
    fn by_hand() {
        let a = [[0x3800, 0x3400, 0x3000, 0x3C00], [0x3400, 0, 0xB800, 0x34B5], [0, 0x3800, 0, 0], [0xB800, 0, 0, 0]];
        let b = [[0x3C00, 0x3C00, 0x3C00, 0x3C00], [0, 0, 0, 0x3C00], [0; 4], [0; 4]];
        let mut texture = vec![0u8; 4 * 3 * 12 * 8];
        for (texel, halves) in [(17, a[0]), (53, a[1]), (89, a[2]), (125, a[3]), (18, b[0]), (54, b[1]), (90, b[2]), (126, b[3])] {
            for (k, half) in halves.into_iter().enumerate() {
                texture[8 * texel + 2 * k..8 * texel + 2 * k + 2].copy_from_slice(&u16::to_le_bytes(half));
            }
        }
        // Those numbers: the luminance of the first is 0.294125, the nearest half 1205 / 4096.
        let first = [V3::new(0.5, 0.25, 0.125), V3::new(0.25, 0.0, -0.5), V3::new(0.0, 0.5, 0.0), V3::new(-0.5, 0.0, 0.0)];
        let solved = volume(&field([1, 0, -1], [2, 1, 1], |at| Some(if at[0] == 1 { first } else { even([1.0; 3]) })));
        assert_eq!((solved.atlas, solved.origin), ([4, 3, 12], [64.0, 0.0, -64.0]));
        assert!(solved.rgba16f_le == texture);

        let v = Reader::new(&solved).unwrap();
        let is = |r: Read, raw: [f32; 3], around: f32, support: f32| {
            let off = (r.raw - V3(raw)).max_abs().max((r.v() - around).abs()).max((r.support - support).abs());
            assert!(off < 1e-5 && r.e() == r.raw.max(V3::ZERO), "{r:?}");
        };
        let y = 1205.0 / 4096.0;
        let z = axis(4);
        let at = V3::new(64.0, 0.0, -64.0);
        // At the first: C0 + 0.6 Cx + 0.8 Cy; its blue is below 0 and is no light.
        is(v.read_at(at, V3::new(0.6, 0.8, 0.0)), [0.65, 0.65, -0.175], y, 1.0);
        assert_eq!(v.read_at(at, V3::new(0.6, 0.8, 0.0)).e()[2], 0.0);
        // A half and a quarter of the way to the second, where C0 is 1 and nothing else.
        is(v.read_at(at + V3::new(32.0, 0.0, 0.0), axis(0)), [0.875, 0.625, 0.3125], 0.5 * y + 0.5, 1.0);
        is(v.read_at(at + V3::new(16.0, 0.0, 0.0), z), [0.25, 0.4375, 0.34375], 0.75 * y + 0.25, 1.0);
        // Half way into the shell along one axis, along two, and along all three, where an eighth is left: of that
        // no more than a quarter is made up for.
        for (off, support, share) in [(V3::new(0.0, -32.0, 0.0), 0.5, 1.0), (V3::new(0.0, 32.0, 32.0), 0.25, 1.0), (V3::new(-32.0, 32.0, -32.0), 0.125, 0.5), (V3::new(-56.0, 0.0, 0.0), 0.125, 0.5)] {
            is(v.read_at(at + off, z), [0.0, 0.25 * share, 0.125 * share], y * share, support);
        }
        // The shell's far edge and beyond is nothing, in every direction.
        for k in 0..6 {
            let end = if k == 0 { 128.0 } else { 64.0 };
            assert_eq!(v.read_at(at + axis(k) * end, z).support, 0.0, "{k}");
            assert_eq!(v.read_at(at + axis(k) * 1e6, z), Read::default(), "{k}");
        }
        // At a surface: 24 up its normal.
        is(v.read(V3::new(64.0, -24.0, -64.0), UP, z), [0.0, 0.25, 0.125], y, 1.0);
        is(v.read(V3::new(152.0, 0.0, -64.0), axis(1), z), [1.0; 3], 1.0, 1.0);
    }

    /// The texture's own filter: texel centres at half texels, whole weights, the edge's texel past the edge.
    #[test]
    fn the_filter_at_the_edge() {
        let mut solved = volume(&field([0; 3], [1; 3], |_| None));
        let v = |s: &SolvedVolume, t: [f32; 3]| Reader { origin: V3::ZERO, spacing: 64.0, offset: 0.0, dimensions: s.dimensions, atlas: s.atlas, rgba16f_le: &s.rgba16f_le }.sample(V3(t))[0];
        // Red 1 in the first texel and 0.5 in the last, of 3 x 3 x 12, whatever they mean in a volume.
        let last = solved.rgba16f_le.len() - 8;
        solved.rgba16f_le[..2].copy_from_slice(&[0, 0x3C]);
        solved.rgba16f_le[last..last + 2].copy_from_slice(&[0, 0x38]);
        assert_eq!([v(&solved, [0.0; 3]), v(&solved, [-9.0; 3]), v(&solved, [1.0; 3]), v(&solved, [1.5; 3])], [1.0, 1.0, 0.5, 0.5]);
        let centre = [0.5 / 3.0, 0.5 / 3.0, 0.5 / 12.0];
        assert!((v(&solved, centre) - 1.0).abs() < 1e-6 && (v(&solved, [1.0 / 3.0, 0.0, 0.0]) - 0.5).abs() < 1e-6);
        assert!((v(&solved, [1.0 / 3.0, 1.0 / 3.0, 1.0 / 12.0]) - 0.125).abs() < 1e-6 && v(&solved, [0.5; 3]) == 0.0);
    }

    /// Nothing valid, and valid but black: no bounce both, but the black one has its support.
    #[test]
    fn dark_and_missing() {
        let none = reader([0; 3], [2, 2, 2], |_| None).read_at(V3([30.0; 3]), UP);
        assert_eq!(none, Read::default());
        let black = reader([0; 3], [2, 2, 2], |_| Some(even([0.0; 3]))).read_at(V3([30.0; 3]), UP);
        assert!(black.e() == V3::ZERO && black.v() == 0.0 && (black.support - 1.0).abs() < 1e-5);
        // A dark one counts: half of a lit one's beside it is left.
        let v = reader([0; 3], [2, 1, 1], |at| Some(even([at[0] as f32; 3])));
        assert!((v.read_at(V3::new(32.0, 0.0, 0.0), UP).v() - 0.5).abs() < 1e-3);
        // An invalid one does not.
        let v = reader([0; 3], [2, 1, 1], |at| (at[0] == 1).then(|| even([1.0; 3])));
        assert!((v.read_at(V3::new(32.0, 0.0, 0.0), UP).v() - 1.0).abs() < 1e-3);
    }

    /// Every texel of the four slabs is where it has to be, and nothing else is there.
    #[test]
    fn the_texture() {
        let f = field([-1, 2, 3], [2, 3, 1], |at| (at[0] < 0).then(|| [V3([1.0 / 3.0, 0.2, 1e-5]), V3([-0.1; 3]), V3::ZERO, V3([0.3; 3])]));
        let s = volume(&f);
        assert_eq!((s.atlas, s.rgba16f_le.len()), ([4, 5, 12], 4 * 5 * 12 * 8));
        let v = Reader::new(&s).unwrap();
        let there = v.slabs([1, 2, 1]);
        assert!((there[0][0] - 1.0 / 3.0).abs() < 2e-4 && there[0][3] == 1.0 && (there[1][3] - luminance([1.0 / 3.0, 0.2, 1e-5])).abs() < 2e-4, "{there:?}");
        assert!((there[1][0] + 0.1).abs() < 1e-4 && there[2] == [0.0; 4] && there[3][3] == 0.0);
        // The last of the points, in the last slab, from its bytes: x 1, y 3, z 1 + 3 x 3.
        let at = (((1 + 9) * 5 + 3) * 4 + 1) * 8;
        assert_eq!(s.rgba16f_le[at..at + 2], to_half(0.3).unwrap().to_le_bytes());
        assert_eq!((0..4 * 5 * 12).filter(|i| v.texel(*i)[3] == 1.0).count(), 3);
        // The signs of zeros and of small numbers stay.
        let (low, dimensions, points) = field([0; 3], [1; 3], |_| Some([V3::ZERO, V3([-1e-9; 3]), V3([-0.0; 3]), V3::ZERO]));
        let small = bytes(low, dimensions, &points).unwrap();
        let word = |slab: usize| [small[((1 + slab * 3) * 9 + 4) * 8], small[((1 + slab * 3) * 9 + 4) * 8 + 1]];
        assert_eq!([0, 1, 2, 3].map(word), [[0, 0], [0, 0x80], [0, 0x80], [0, 0]]);

        let of = |c: [f32; 3]| {
            let (low, dimensions, points) = field([5, 0, 0], [1; 3], |_| Some(even(c)));
            bytes(low, dimensions, &points)
        };
        assert_eq!(of([7e4; 3]), Err(Failed::Unavailable(Reason::OutOfRange { at: [5, 0, 0] })));
        assert!(of([f32::NAN; 3]).is_err() && of([6e4, 0.0, 0.0]).is_ok());
        let one = BTreeMap::from([([0; 3], Moments::default())]);
        let of = |dimensions, most, cancel: Cancel| packed([0; 3], dimensions, one.iter(), most, cancel).map(|b| b.len());
        assert_eq!([863, 864].map(|most| of([1; 3], most, &|| false)), [Err(Failed::Unavailable(Reason::TooManyBytes { bytes: 864, limit: 863 })), Ok(864)]);
        // 2^31, which no 32-bit `Vec` may be.
        assert_eq!(bytes_of(atlas_of([2046, 126, 254]).unwrap()), 1 << 31);
        // What may not be is not asked for: 64 GiB.
        assert_eq!(of([2046, 2046, 510], 1 << 26, &|| false), Err(Failed::Unavailable(Reason::TooManyBytes { bytes: 1 << 36, limit: 1 << 26 })));
        assert!(of([2047, 1, 1], u64::MAX, &|| false) == Err(Failed::Unavailable(Reason::TooLarge { dimensions: [2047, 1, 1] })) && of([1; 3], 864, &|| true) == Err(Failed::Cancelled));
        assert!(atlas_of([2046, 1, 1]).is_some() && atlas_of([2047, 1, 1]).is_none() && atlas_of([1, 1, 511]).is_none() && atlas_of([0, 1, 1]).is_none() && atlas_of([u32::MAX; 3]).is_none());
    }

    #[test]
    fn what_a_reader_refuses() {
        let good = volume(&field([0; 3], [2, 2, 2], |_| Some(even([0.5; 3]))));
        let why = |f: &dyn Fn(&mut SolvedVolume)| {
            let mut v = good.clone();
            f(&mut v);
            Reader::new(&v).unwrap_err()
        };
        let inner = ((4 + 1) * 4 + 1) * 8;
        assert!(why(&|v| v.rgba16f_le.truncate(8)).contains("bytes") && why(&|v| v.dimensions = [2, 2, 3]).contains("dimensions"));
        assert!(why(&|v| v.origin[0] = 1.0).contains("origin") && why(&|v| v.spacing = 0.0).contains("spacing"));
        assert!(why(&|v| v.spacing = f32::from_bits(1)).contains("spacing"));
        assert!(why(&|v| (v.counts.reached, v.counts.unknown) = (7, 1)).contains("8 valid texels"));
        assert!(why(&|v| v.counts.reached = 7).contains("counts") && why(&|v| v.counts.points = 9).contains("counts") && why(&|v| v.counts.inside = usize::MAX).contains("counts"));
        assert!(why(&|v| (v.low, v.spacing, v.origin) = ([i32::MAX, 0, 0], 1e30, [f32::INFINITY, 0.0, 0.0])).contains("origin"));
        let end = |low: i32| why(&|v| (v.low, v.origin) = ([0, low, 0], [0.0, low as f32 * 64.0, 0.0]));
        assert!(end(i32::MAX).contains("last place"));
        let mut far = good.clone();
        (far.low, far.origin) = ([0, i32::MAX - 1, 0], [0.0, (i32::MAX - 1) as f32 * 64.0, 0.0]);
        let mut valid = Vec::new();
        Reader::scanned(&far, &|| false, |at| valid.push(at)).unwrap();
        assert!(valid.len() == 8 && valid.contains(&[1, i32::MAX, 1]));
        assert_eq!(Reader::scanned(&good, &|| true, |_| ()).unwrap_err(), Refused::Cancelled);
        assert!(why(&|v| v.rgba16f_le[inner + 1] = 0x7C).contains("in the texture"));
        assert!(why(&|v| v.rgba16f_le[1] = 0x3C).contains("of the shell: true"), "a lit shell");
        assert!(why(&|v| v.rgba16f_le[inner + 7] = 0x38).contains("the texel"), "half valid");
    }
}
