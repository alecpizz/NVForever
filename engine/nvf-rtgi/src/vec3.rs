//! Three-component vectors, the sRGB transfer and the luminance.

use std::ops::{Add, Index, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3(pub [f32; 3]);

impl V3 {
    pub const ZERO: V3 = V3([0.0; 3]);

    pub fn new(x: f32, y: f32, z: f32) -> V3 {
        V3([x, y, z])
    }

    pub fn dot(self, b: V3) -> f32 {
        self[0] * b[0] + self[1] * b[1] + self[2] * b[2]
    }

    pub fn cross(self, b: V3) -> V3 {
        V3([self[1] * b[2] - self[2] * b[1], self[2] * b[0] - self[0] * b[2], self[0] * b[1] - self[1] * b[0]])
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// The unit vector; `None` of a zero or not finite one.
    pub fn unit(self) -> Option<V3> {
        let l = self.length();
        (l.is_finite() && l > 0.0).then(|| self * (1.0 / l))
    }

    pub fn finite(self) -> bool {
        self.0.iter().all(|f| f.is_finite())
    }

    /// The largest absolute component.
    pub fn max_abs(self) -> f32 {
        self.0.iter().fold(0.0, |m, f| m.max(f.abs()))
    }

    pub fn min(self, b: V3) -> V3 {
        V3(std::array::from_fn(|k| self[k].min(b[k])))
    }

    pub fn max(self, b: V3) -> V3 {
        V3(std::array::from_fn(|k| self[k].max(b[k])))
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, b: V3) -> V3 {
        V3(std::array::from_fn(|k| self[k] + b[k]))
    }
}

impl Sub for V3 {
    type Output = V3;
    fn sub(self, b: V3) -> V3 {
        V3(std::array::from_fn(|k| self[k] - b[k]))
    }
}

impl Mul<f32> for V3 {
    type Output = V3;
    fn mul(self, s: f32) -> V3 {
        V3(self.0.map(|f| f * s))
    }
}

impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        self * -1.0
    }
}

impl Index<usize> for V3 {
    type Output = f32;
    fn index(&self, k: usize) -> &f32 {
        &self.0[k]
    }
}

/// The weighted sum of three values.
pub fn blend<const N: usize>(v: &[[f32; N]; 3], w: [f32; 3]) -> [f32; N] {
    std::array::from_fn(|k| v[0][k] * w[0] + v[1][k] * w[1] + v[2][k] * w[2])
}

/// The affine map of three rows of four: `M * (p, 1)`.
#[derive(Clone, Copy, Debug)]
pub struct Affine(pub [[f32; 4]; 3]);

impl Affine {
    pub fn column(&self, k: usize) -> V3 {
        V3(std::array::from_fn(|r| self.0[r][k]))
    }

    pub fn point(&self, p: V3) -> V3 {
        self.column(0) * p[0] + self.column(1) * p[1] + self.column(2) * p[2] + self.column(3)
    }

    pub fn determinant(&self) -> f32 {
        self.column(0).dot(self.column(1).cross(self.column(2)))
    }

    /// A normal by the inverse transpose, unit; `None` of one that vanishes. The cofactors are the
    /// inverse transpose times the determinant: its sign is taken out, its size does not matter.
    pub fn normal(&self, n: V3) -> Option<V3> {
        let (x, y, z) = (self.column(0), self.column(1), self.column(2));
        let out = y.cross(z) * n[0] + z.cross(x) * n[1] + x.cross(y) * n[2];
        (out * self.determinant().signum()).unit()
    }
}

/// The sRGB transfer, display value to linear.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The other way round.
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// The Rec. 709 weighted sum.
pub fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A random sequence (splitmix64), the same on every machine.
    pub struct Rng(pub u64);

    impl Rng {
        /// The next number in [0, 1).
        pub fn draw(&mut self) -> f32 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) >> 40) as f32 / 16_777_216.0
        }

        /// A direction over the hemisphere of `n` (unit), cosine-weighted.
        pub fn cosine(&mut self, n: V3) -> V3 {
            let (u, v) = (self.draw(), self.draw());
            cosine(n, u, v)
        }
    }

    /// The direction over the hemisphere of `n` (unit) that `u` and `v` in [0, 1) stand for:
    /// cosine-weighted when they are evenly spread.
    pub fn cosine(n: V3, u: f32, v: f32) -> V3 {
        let phi = std::f32::consts::TAU * v;
        let helper = if n[0].abs() > 0.7 { V3::new(0.0, 1.0, 0.0) } else { V3::new(1.0, 0.0, 0.0) };
        let x = helper.cross(n).unit().unwrap_or(helper);
        let y = n.cross(x);
        x * (u.sqrt() * phi.cos()) + y * (u.sqrt() * phi.sin()) + n * (1.0 - u).sqrt()
    }

    /// A normal stays at right angles to its surface under a skewed and a mirrored map.
    #[test]
    fn normals_follow_the_surface() {
        for m in [[[2.0, 1.0, 0.0, 5.0], [0.0, 1.0, 0.5, 0.0], [0.0, 0.0, 3.0, 1.0]], [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.3, 0.0], [0.0, 0.0, 1.0, 0.0]]] {
            let a = Affine(m);
            let (u, v) = (V3::new(1.0, 2.0, 0.5), V3::new(-1.0, 0.3, 2.0));
            let n = a.normal(u.cross(v)).unwrap();
            let map = |d: V3| a.point(d) - a.point(V3::ZERO);
            assert!(n.dot(map(u)).abs() < 1e-5 && n.dot(map(v)).abs() < 1e-5);
            assert!(n.dot(map(u.cross(v))) > 0.0, "on the same side");
        }
        assert_eq!(Affine([[0.0; 4]; 3]).normal(V3::new(0.0, 0.0, 1.0)), None);
    }

    #[test]
    fn cosine_directions_have_the_cosine_mean() {
        let (mut rng, n) = (Rng(7), V3::new(0.6, 0.0, -0.8));
        let mean = (0..20_000).map(|_| rng.cosine(n).dot(n)).sum::<f32>() / 20_000.0;
        assert!((mean - 2.0 / 3.0).abs() < 0.01, "{mean}");
    }
}
