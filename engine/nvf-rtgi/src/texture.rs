//! Textures as bounce and shadow rays read them: one level each for the alpha test and for the colour,
//! bilinear, in linear light.

use crate::scene::{Level, Texture, Wrap};
use crate::vec3::srgb_to_linear;

impl Wrap {
    fn texel(self, i: i64, n: i64) -> usize {
        (match self {
            Wrap::Repeat => i.rem_euclid(n),
            Wrap::Clamp => i.clamp(0, n - 1),
            Wrap::Mirror => {
                let m = i.rem_euclid(2 * n);
                m.min(2 * n - 1 - m)
            }
        }) as usize
    }
}

/// What a lookup is for, which decides the level it reads: the largest of at most 256 texels a side for the
/// alpha and of at most 64 for the colour.
#[derive(Clone, Copy, Debug)]
pub enum Lookup {
    Alpha,
    Color,
}

impl Lookup {
    /// The longest side of a level it reads, when there is one as small.
    fn side(self) -> usize {
        match self {
            Lookup::Alpha => 256,
            Lookup::Color => 64,
        }
    }

    /// The level it reads of a texture whose levels have these sides, the largest first: the first within
    /// [`Lookup::side`], else the last.
    fn level(self, sides: &[[usize; 2]]) -> Option<usize> {
        sides.iter().position(|[w, h]| *w.max(h) <= self.side()).or(sides.len().checked_sub(1))
    }
}

/// The levels that rays read of a texture whose levels have these sides (width and height, the largest first), in
/// the same order: the colour's, and the alpha's where any material with the texture has a cutoff (`tested`). With
/// the others left out of a [`Texture`] rays read the same.
pub fn levels_read(sides: &[[usize; 2]], tested: bool) -> Vec<usize> {
    let alpha = if tested { Lookup::Alpha.level(sides) } else { None };
    let mut read: Vec<usize> = alpha.into_iter().chain(Lookup::Color.level(sides)).collect();
    read.dedup();
    read
}

pub struct Sampler {
    /// From [`Lookup::Alpha`]'s level to [`Lookup::Color`]'s, which is the last.
    levels: Vec<Level>,
    to_linear: Box<[f32; 256]>,
    wrap: [Wrap; 2],
    srgb: bool,
}

/// A texture as rays read it.
#[derive(Clone, Copy, Debug)]
pub struct Texels<'a> {
    /// The levels that [`Lookup::Alpha`] and [`Lookup::Color`] read; they may be the same one.
    pub alpha: &'a Level,
    pub color: &'a Level,
    pub srgb: bool,
    /// What a colour's byte stands for.
    pub to_linear: &'a [f32; 256],
    pub wrap: [Wrap; 2],
}

impl Sampler {
    /// `None` of one without levels or with one that is not whole.
    pub fn new(t: Texture) -> Option<Sampler> {
        let whole = |l: &Level| l.width > 0 && l.height > 0 && l.width.checked_mul(l.height) == Some(l.texels.len());
        if !t.levels.iter().all(whole) {
            return None;
        }
        let sides: Vec<[usize; 2]> = t.levels.iter().map(|l| [l.width, l.height]).collect();
        let (alpha, color) = (Lookup::Alpha.level(&sides)?, Lookup::Color.level(&sides)?);
        let to_linear = Box::new(std::array::from_fn(|i| {
            let c = i as f32 / 255.0;
            if t.srgb {
                srgb_to_linear(c)
            } else {
                c
            }
        }));
        let levels = t.levels.into_iter().take(color + 1).skip(alpha).collect();
        Some(Sampler { levels, to_linear, wrap: t.wrap, srgb: t.srgb })
    }

    pub fn texels(&self) -> Texels<'_> {
        Texels { alpha: &self.levels[0], color: &self.levels[self.levels.len() - 1], srgb: self.srgb, to_linear: &self.to_linear, wrap: self.wrap }
    }

    /// Texels, levels and the table, as they are held.
    pub fn bytes(&self) -> usize {
        let texels: usize = self.levels.iter().map(|l| size_of::<[u8; 4]>() * l.texels.capacity()).sum();
        texels + self.levels.capacity() * size_of::<Level>() + size_of::<[f32; 256]>()
    }

    fn linear(&self, c: [u8; 4]) -> [f32; 4] {
        let f = |k: usize| self.to_linear[usize::from(c[k])];
        [f(0), f(1), f(2), f32::from(c[3]) / 255.0]
    }

    /// Linear RGB and alpha at `uv`.
    pub fn sample(&self, uv: [f32; 2], lookup: Lookup) -> [f32; 4] {
        let l = match lookup {
            Lookup::Alpha => &self.levels[0],
            Lookup::Color => &self.levels[self.levels.len() - 1],
        };
        // Coordinates can be finite and their texels not.
        let texel = |c: f32, n: usize| Some(c * n as f32 - 0.5).filter(|x| x.is_finite()).unwrap_or(-0.5);
        let (x, y) = (texel(uv[0], l.width), texel(uv[1], l.height));
        let (fx, fy) = (x - x.floor(), y - y.floor());
        let (x, y) = (x.floor() as i64, y.floor() as i64);
        let mut out = [0.0; 4];
        for (dx, dy, w) in [(0, 0, (1.0 - fx) * (1.0 - fy)), (1, 0, fx * (1.0 - fy)), (0, 1, (1.0 - fx) * fy), (1, 1, fx * fy)] {
            let at = self.wrap[1].texel(y.saturating_add(dy), l.height as i64) * l.width + self.wrap[0].texel(x.saturating_add(dx), l.width as i64);
            let c = self.linear(l.texels[at]);
            out = std::array::from_fn(|k| out[k] + w * c[k]);
        }
        out
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A texture of one level, row by row.
    pub fn rgba(width: usize, height: usize, texels: &[[u8; 4]], srgb: bool, wrap: Wrap) -> Texture {
        Texture { levels: vec![Level { width, height, texels: texels.to_vec() }], srgb, wrap: [wrap; 2] }
    }

    #[test]
    fn bilinear_and_wrap() {
        let texels = [[0, 0, 0, 255], [255; 4]];
        let at = |wrap: Wrap, u: f32| Sampler::new(rgba(2, 1, &texels, false, wrap)).unwrap().sample([u, 0.5], Lookup::Color)[0];
        let r = [0.25, 0.75, 0.5, 0.0, 1.0, -0.25, 7.25, f32::NAN].map(|u| at(Wrap::Repeat, u));
        assert_eq!(r, [0.0, 1.0, 0.5, 0.5, 0.5, 1.0, 0.0, 0.5]);
        assert_eq!([0.0, 1.0, 9.0].map(|u| at(Wrap::Clamp, u)), [0.0, 1.0, 1.0]);
        assert_eq!([0.0, 1.25, -0.25].map(|u| at(Wrap::Mirror, u)), [0.0, 1.0, 0.0]);
        assert_eq!([f32::MAX, f32::MIN, f32::INFINITY].map(|u| at(Wrap::Repeat, u)), [0.5; 3], "as those that are none");
        assert!(at(Wrap::Clamp, 1e30) == 1.0 && at(Wrap::Mirror, -1e30).is_finite());
    }

    /// The transfer is undone before the blend.
    #[test]
    fn srgb_is_decoded_first() {
        let t = Sampler::new(rgba(2, 1, &[[188, 0, 0, 255], [0, 0, 0, 0]], true, Wrap::Repeat)).unwrap();
        let full = srgb_to_linear(188.0 / 255.0);
        assert!((t.sample([0.25, 0.5], Lookup::Color)[0] - full).abs() < 1e-6);
        let half = t.sample([0.5, 0.5], Lookup::Color);
        assert!((half[0] - full / 2.0).abs() < 1e-6 && half[3] == 0.5);
    }

    /// The largest level within the side, else the smallest there is; the rest is let go.
    #[test]
    fn levels_by_the_side() {
        let chain = |sides: &[usize]| {
            let level = |(i, s): (usize, &usize)| Level { width: *s, height: 1, texels: vec![[i as u8 * 51, 0, 0, 255]; *s] };
            Texture { levels: sides.iter().enumerate().map(level).collect(), srgb: false, wrap: [Wrap::Repeat; 2] }
        };
        let read = |sides: &[usize]| {
            let t = Sampler::new(chain(sides)).unwrap();
            let n = |lookup| (t.sample([0.3, 0.6], lookup)[0] * 5.0).round() as usize;
            (n(Lookup::Alpha), n(Lookup::Color), t.levels.len())
        };
        assert_eq!(read(&[1024, 512, 256, 128, 64, 32]), (2, 4, 3));
        assert_eq!(read(&[1024, 512]), (1, 1, 1));
        assert_eq!(read(&[32, 16]), (0, 0, 1));
        assert_eq!(read(&[512, 100]), (1, 1, 1));
        assert!(Sampler::new(chain(&[])).is_none() && Sampler::new(rgba(2, 2, &[[0; 4]; 3], false, Wrap::Clamp)).is_none());
        assert!(Sampler::new(rgba(0, 1, &[], false, Wrap::Clamp)).is_none());
    }

    /// A texture of the levels read alone gives every lookup that is made what the whole one gives, and none of
    /// them is let go; whatever the sides and however short the chain.
    #[test]
    fn the_levels_read_are_enough() {
        assert_eq!(levels_read(&[], true), [0; 0]);
        let square = |sides: &[usize], tested| levels_read(&sides.iter().map(|s| [*s; 2]).collect::<Vec<_>>(), tested);
        assert_eq!((square(&[1024, 512, 256, 128, 64, 32], true), square(&[1024, 512, 256, 128, 64, 32], false)), (vec![2, 4], vec![4]));
        assert_eq!((square(&[512, 300], true), square(&[256, 128], true), square(&[256, 128], false)), (vec![1], vec![0, 1], vec![1]));
        let mut chains = 0;
        for (w, h) in [(1, 1), (3, 2), (64, 64), (65, 64), (64, 65), (100, 30), (130, 1), (256, 256), (257, 1), (1, 300), (512, 128), (600, 600), (1030, 4)] {
            let sides: Vec<[usize; 2]> = (0..11).map(|m| [(w >> m).max(1), (h >> m).max(1)]).take_while(|s| *s != [1, 1]).chain([[1, 1]]).collect();
            for mips in 1..=sides.len() {
                let sides = &sides[..mips];
                let of = |read: &[usize]| {
                    let level = |l: usize| Level { width: sides[l][0], height: sides[l][1], texels: vec![[l as u8 * 20; 4]; sides[l][0] * sides[l][1]] };
                    let levels = read.iter().map(|l| level(*l)).collect();
                    Sampler::new(Texture { levels, srgb: false, wrap: [Wrap::Clamp; 2] }).unwrap()
                };
                let whole = of(&(0..mips).collect::<Vec<_>>());
                for tested in [false, true] {
                    let read = levels_read(sides, tested);
                    let part = of(&read);
                    let same = |lookup| part.sample([0.5; 2], lookup) == whole.sample([0.5; 2], lookup);
                    assert!(same(Lookup::Color) && (!tested || same(Lookup::Alpha)), "{sides:?} {tested}: {read:?}");
                    assert_eq!(part.levels.len(), read.len(), "{sides:?} {tested}");
                }
                chains += 1;
            }
        }
        assert!(chains > 60);
    }
}
