//! The bounce light around a point in the air, and its two lowest bands.
//!
//! What a ray brings is what the lamps in sight of the surface it meets add there to what the surface shows:
//! the display sum with them, less the display sum of ambient and directional alone, each as displayed. A
//! single-sided surface's back is solid and black; a double-sided one is lit from the side met; below
//! the alpha cutoff the ray goes on; an unlit surface brings a share of its own colour; a miss brings nothing.
//! Any surface is in a lamp's way, so one in a closed shade lights nothing outside of it.

use crate::bvh::Ray;
use crate::light::{lit, Lights};
use crate::texture::Lookup;
use crate::trace::{Hit, PreparedScene};
use crate::vec3::V3;

/// What an unlit surface shows: its colour, clipped; linear.
pub fn own(hit: &Hit) -> V3 {
    let a = hit.albedo(Lookup::Color);
    // One that is no number (an overflow times 0) is black.
    V3([a[0], a[1], a[2]].map(|c| if c > 0.0 { c.min(1.0) } else { 0.0 }))
}

/// What lamps of the display sum `seen` add to what a surface of albedo `a` shows under `base`. A lamp never
/// darkens, the bound is for the rounding.
fn added(a: [f32; 3], base: V3, seen: V3) -> V3 {
    (V3(lit(a, base + seen)) - V3(lit(a, base))).max(V3::ZERO)
}

/// What one ray did for the figures.
#[derive(Default)]
struct Asked {
    lamps: u32,
    shadow_rays: u32,
}

/// What a lit surface sends back of the lamps' light.
fn leaving(world: &PreparedScene, lights: &Lights, hit: &Hit, asked: &mut Asked) -> V3 {
    let (n, g) = (hit.shading(), hit.facing());
    let mut seen = V3::ZERO;
    for l in lights.near(hit.p) {
        asked.lamps += 1;
        let add = l.light(hit.p, n);
        if add != V3::ZERO {
            asked.shadow_rays += 1;
            if world.sees(hit.p, g, l.position) {
                seen = seen + add;
            }
        }
    }
    if seen == V3::ZERO {
        // Both sums are the same.
        return V3::ZERO;
    }
    let a = hit.albedo(Lookup::Color);
    added([a[0], a[1], a[2]], lights.base(n), seen)
}

/// Direction `i` of `count` over the sphere (a Fibonacci spiral).
pub fn spiral(i: usize, count: usize) -> V3 {
    let y = 1.0 - (2 * i + 1) as f32 / count as f32;
    let (s, c) = (i as f32 * 2.399_96).sin_cos();
    let r = (1.0 - y * y).sqrt();
    V3::new(r * c, y, r * s)
}

/// The directions of a point's rays, in their order: a Fibonacci spiral over the sphere. A solve with `count` rays
/// a point has these.
pub fn directions(count: usize) -> Vec<V3> {
    (0..count).map(|i| spiral(i, count)).collect()
}

/// The two lowest bands of the light around a point, for the light on a surface there: with `w` towards
/// the source, C0 = mean(L) and Cj = 2 mean(L wj), and the light on one that faces `n` is C0 + C.n.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Moments {
    /// C0, Cx, Cy, Cz.
    pub c: [V3; 4],
}

impl Moments {
    /// With the light `l` from `w`, not yet divided by how many there are.
    fn add(&mut self, w: V3, l: V3) {
        self.c[0] = self.c[0] + l;
        for k in 0..3 {
            self.c[k + 1] = self.c[k + 1] + l * (2.0 * w[k]);
        }
    }

    fn over(self, samples: usize) -> Moments {
        let share = 1.0 / samples.max(1) as f32;
        Moments { c: self.c.map(|c| c * share) }
    }
}

/// What a point in the air gets from all around, and what its rays met.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Probe {
    pub moments: Moments,
    pub hits: u32,
    /// Of `hits`, the hidden sides.
    pub backs: u32,
    /// The lamps that were looked at, all those of each lit hit's cell of the lamps' grid, and the shadow rays to those
    /// that add something.
    pub lamps: u32,
    pub shadow_rays: u32,
}

impl Probe {
    /// Whether it is inside geometry: more than a quarter of the surfaces met show their back. One
    /// that meets nothing is not.
    pub fn inside(&self) -> bool {
        self.backs > self.hits / 4
    }

    /// In the air, and something is around it.
    pub fn valid(&self) -> bool {
        self.hits > 0 && !self.inside()
    }
}

/// Of a point's rays, those that met a surface and, of them, the hidden sides: the [`Probe::hits`] and [`Probe::backs`]
/// of [`probe`], which are all that decide whether it is valid. Nothing is lit.
pub fn counted(world: &PreparedScene, at: V3, directions: &[V3]) -> [u32; 2] {
    let mut out = [0; 2];
    for d in directions {
        let Some(h) = world.transport(&Ray::new(at, *d), f32::INFINITY) else { continue };
        out[0] += 1;
        out[1] += u32::from(h.hidden);
    }
    out
}

/// The [`Probe`] at a point, its rays in the order of `directions`; `emission` is the share of the unlit
/// surfaces' own light that counts.
pub fn probe(world: &PreparedScene, lights: &Lights, at: V3, directions: &[V3], emission: f32) -> Probe {
    let (mut out, mut asked) = (Probe::default(), Asked::default());
    for d in directions {
        let Some(h) = world.transport(&Ray::new(at, *d), f32::INFINITY) else { continue };
        out.hits += 1;
        if h.hidden {
            out.backs += 1;
            continue;
        }
        let light = if h.material().unlit { own(&h) * emission } else { leaving(world, lights, &h, &mut asked) };
        out.moments.add(*d, light);
    }
    Probe { moments: out.moments.over(directions.len()), lamps: asked.lamps, shadow_rays: asked.shadow_rays, ..out }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::light::tests::lights_of;
    use crate::light::{Directional, Lamp, LightState};
    use crate::scene::{Cull, Material, Mesh, SceneInput};
    use crate::trace::tests::{prepared, scene, SAME};
    use crate::vec3::tests::Rng;

    const UP: V3 = V3([0.0, 1.0, 0.0]);
    pub const ALBEDO: [f32; 3] = [0.5, 0.25, 0.8];
    pub const AMBIENT: V3 = V3([0.2, 0.3, 0.4]);

    pub type Part = (Mesh, [[f32; 4]; 3], Material);

    /// +x, -x, +y, -y, +z, -z.
    pub fn axis(i: usize) -> V3 {
        V3(std::array::from_fn(|k| if k == i / 2 { 1.0 - 2.0 * (i % 2) as f32 } else { 0.0 }))
    }

    /// A rectangle of half sides `u` and `v`, its front towards `u` x `v`.
    pub fn rectangle(centre: V3, u: V3, v: V3, material: Material) -> Part {
        let positions = [centre - u - v, centre + u - v, centre + u + v, centre - u + v].map(|p| p.0).to_vec();
        (Mesh { positions, indices: vec![0, 1, 2, 0, 2, 3], ..Mesh::default() }, SAME, material)
    }

    /// The sides of a box that `keep` (axis, whether the upper one) passes; fronts inward, outward
    /// for a negative `half`.
    pub fn sides(centre: V3, half: V3, material: Material, keep: impl Fn(usize, bool) -> bool) -> Vec<Part> {
        let axis = |k: usize| V3(std::array::from_fn(|i| if i == k % 3 { half[i] } else { 0.0 }));
        let mut out = Vec::new();
        for k in 0..3 {
            for upper in [false, true].into_iter().filter(|upper| keep(k, *upper)) {
                let (u, v) = if upper { (axis(k + 2), axis(k + 1)) } else { (axis(k + 1), axis(k + 2)) };
                out.push(rectangle(if upper { centre + axis(k) } else { centre - axis(k) }, u, v, material));
            }
        }
        out
    }

    pub fn plain() -> Material {
        Material { base: [ALBEDO[0], ALBEDO[1], ALBEDO[2], 1.0], ..Material::default() }
    }

    pub fn built(parts: Vec<Part>) -> PreparedScene {
        prepared(scene(parts, Vec::new()), 1)
    }

    /// A closed box of half side 100 about the origin.
    pub fn room(material: Material) -> PreparedScene {
        built(sides(V3::ZERO, V3([100.0; 3]), material, |_, _| true))
    }

    pub fn lamp(position: V3) -> Lamp {
        Lamp { position, display: V3([0.8; 3]), range: 300.0, cone: None }
    }

    pub fn state(ambient: V3, lamps: Vec<Lamp>) -> LightState {
        LightState { ambient, lamps, ..LightState::default() }
    }

    fn of(world: &PreparedScene, state: &LightState, at: V3, emission: f32) -> Probe {
        probe(world, &lights_of(state), at, &directions(512), emission)
    }

    /// The moments of rays traced for them alone, with both display sums at every lit surface, and whether
    /// there is something around and at most a quarter of it shows its back.
    pub fn traced_again(world: &PreparedScene, lights: &Lights, at: V3, directions: &[V3], emission: f32) -> (Moments, bool) {
        let (mut out, mut fronts, mut backs) = (Moments::default(), 0, 0);
        for d in directions {
            let light = match world.transport(&Ray::new(at, *d), f32::INFINITY) {
                None => continue,
                Some(h) if h.hidden => {
                    backs += 1;
                    continue;
                }
                Some(h) if h.material().unlit => own(&h) * emission,
                Some(h) => {
                    let a = h.albedo(Lookup::Color);
                    let (n, g, base) = (h.shading(), h.facing(), lights.base(h.shading()));
                    let add = |s: V3, l: &Lamp| s + l.light(h.p, n);
                    let seen = lights.near(h.p).filter(|l| l.light(h.p, n) != V3::ZERO && world.sees(h.p, g, l.position)).fold(V3::ZERO, add);
                    added([a[0], a[1], a[2]], base, seen)
                }
            };
            fronts += 1;
            out.add(*d, light);
        }
        (out.over(directions.len()), fronts > 0 && fronts >= 3 * backs)
    }

    fn close(a: V3, b: V3, off: f32) -> bool {
        (a - b).max_abs() <= off
    }

    /// What lamps add by hand: a grey of 1 under an ambient of 0.15 and lamps of 0.4; both clipped; lamps
    /// that add nothing; the rounding's bound.
    #[test]
    fn what_lamps_add() {
        let grey = |a: f32, ambient: f32, lamps: f32| added([a; 3], V3([ambient; 3]), V3([lamps; 3]))[0];
        assert!((grey(1.0, 0.15, 0.4) - 0.253014).abs() < 2e-6);
        assert!(grey(1.0, 1.1, 0.4) == 0.0 && grey(0.7, 0.3, 0.0) == 0.0 && grey(-1.0, 0.3, 0.4) == 0.0);
        assert!(added([f32::NAN, f32::INFINITY, f32::NEG_INFINITY], V3([0.3; 3]), V3([0.4; 3])).finite());
    }

    /// Two bands by hand: an even light is the same on every side; L = 0.5 + 0.3 wx gives 0.7 and 0.3.
    #[test]
    fn two_bands() {
        let of = |l: &dyn Fn(V3) -> V3| {
            let mut out = Moments::default();
            directions(512).into_iter().for_each(|w| out.add(w, l(w)));
            out.over(512)
        };
        let on = |m: &Moments, n: V3| m.c[0] + m.c[1] * n[0] + m.c[2] * n[1] + m.c[3] * n[2];
        let even = of(&|_| V3(ALBEDO));
        let mut rng = Rng(3);
        for _ in 0..50 {
            let n = crate::bvh::tests::direction(&mut rng);
            assert!(close(on(&even, n), V3(ALBEDO), 4e-3), "{:?}", on(&even, n));
        }
        let side = of(&|w| V3::new(0.5 + 0.3 * w[0], 0.5, 0.0));
        for (i, want) in [0.7, 0.3, 0.5, 0.5, 0.5, 0.5].into_iter().enumerate() {
            assert!(close(on(&side, axis(i)), V3::new(want, 0.5, 0.0), 4e-3), "{i}: {side:?}");
        }
        assert!(axis(0) == V3::new(1.0, 0.0, 0.0) && axis(5) == V3::new(0.0, 0.0, -1.0));
        // A light from one side alone is below 0 from the other.
        assert!(on(&of(&|w| V3([if w[0] > 0.9 { 1.0 } else { 0.0 }; 3])), axis(1))[0] < 0.0);
    }

    #[test]
    fn directions_are_even() {
        let d = directions(512);
        assert!(d.len() == 512 && d.iter().all(|d| (d.length() - 1.0).abs() < 1e-6) && directions(0).is_empty());
        assert!(d.iter().fold(V3::ZERO, |s, d| s + *d).max_abs() / 512.0 < 4e-3);
    }

    /// Nothing but lamps is bounced, whatever the ambient and the directional; emission is, in its share.
    #[test]
    fn what_is_bounced() {
        let (w, p) = (room(plain()), V3::new(5.0, 0.0, 0.0));
        let sun = LightState { directionals: vec![Directional { display: V3([0.6; 3]), to_source: UP }], ..state(AMBIENT, Vec::new()) };
        for s in [&state(AMBIENT, Vec::new()), &sun] {
            let g = of(&w, s, p, 0.25);
            assert!(g == Probe { hits: 512, ..Probe::default() } && g.valid(), "{g:?}");
        }
        let glow = of(&room(Material { unlit: true, ..plain() }), &state(AMBIENT, vec![lamp(V3::ZERO)]), p, 0.25);
        assert!(close(glow.moments.c[0], V3(ALBEDO) * 0.25, 1e-4) && (glow.lamps, glow.shadow_rays) == (0, 0), "{glow:?}");
        // Clipped as displayed.
        let bright = Material { unlit: true, base: [3.0, 0.5, -1.0, 1.0], ..plain() };
        assert!(close(of(&room(bright), &sun, p, 1.0).moments.c[0], V3([1.0, 0.5, 0.0]), 1e-4));
        let overflow = Material { unlit: true, base: [f32::MAX, 0.5, 0.5, 1.0], ..plain() };
        let mut parts = sides(V3::ZERO, V3([100.0; 3]), overflow, |_, _| true);
        parts.iter_mut().for_each(|p| p.0.colors = vec![[f32::MAX, 1.0, 1.0, 1.0]; 4]);
        parts.iter_mut().for_each(|p| p.2.texture = Some(0));
        let black = crate::texture::tests::rgba(1, 1, &[[0, 255, 255, 255]], false, crate::scene::Wrap::Repeat);
        let none = of(&prepared(scene(parts, vec![Some(black)]), 1), &sun, p, 1.0);
        assert!(close(none.moments.c[0], V3([0.0, 0.5, 0.5]), 1e-4), "{none:?}");
        // A back is black whatever it shows in front, and the point is inside.
        let back = of(&room(Material { cull: Cull::Front, ..bright }), &sun, p, 1.0);
        assert!((back.hits, back.backs, back.moments, back.inside()) == (512, 512, Moments::default(), true));
        // Without ambient: what the lamp shows; with it the same lamp shows more.
        let [dark, lit] = [V3::ZERO, AMBIENT].map(|a| of(&w, &state(a, vec![lamp(V3::ZERO)]), p, 0.0));
        assert!(dark.moments.c[0][0] > 0.01 && lit.moments.c[0][0] > 1.2 * dark.moments.c[0][0] && lit.shadow_rays == 512, "{dark:?} {lit:?}");
        // The lamp is by the wall at -x, whose light is towards -x.
        let m = of(&w, &state(AMBIENT, vec![lamp(V3::new(-50.0, 60.0, 0.0))]), V3::ZERO, 0.0).moments;
        assert!(m.c[1][2] < 0.0 && m.c[0][0] > 0.0, "{m:?}");
    }

    /// One wall of the room, seen from its middle, gets the light of a lamp by hand: the mean over the
    /// directions that meet it.
    #[test]
    fn one_wall_by_hand() {
        let w = built(sides(V3::ZERO, V3([100.0; 3]), plain(), |k, upper| k == 1 && !upper));
        let l = lamp(V3::ZERO);
        let g = of(&w, &state(V3::ZERO, vec![l]), V3::ZERO, 0.0);
        let mut want = Moments::default();
        let mut hits = 0;
        for d in directions(512) {
            let p = d * (-100.0 / d[1]);
            if d[1] < 0.0 && p[0].abs() < 100.0 && p[2].abs() < 100.0 {
                hits += 1;
                want.add(d, V3(lit(ALBEDO, l.light(p, UP))));
            }
        }
        let want = want.over(512);
        assert!(g.hits == hits && hits > 50 && (0..4).all(|k| close(g.moments.c[k], want.c[k], 1e-5)), "{g:?}, not {want:?}");
    }

    /// A wall keeps the lamp's light from the other side; a hole in it lets it through.
    #[test]
    fn walls_and_lamps() {
        let wall = |cutoff| Material { cull: Cull::None, double_sided: true, cutoff, base: [0.5, 0.5, 0.5, 0.4], ..Material::default() };
        let of = |cutoff, x: f32| {
            let mut parts = sides(V3::ZERO, V3([100.0; 3]), plain(), |_, _| true);
            parts.push(rectangle(V3::ZERO, V3::new(0.0, 100.0, 0.0), V3::new(0.0, 0.0, 100.0), wall(cutoff)));
            of(&built(parts), &state(AMBIENT, vec![lamp(V3::new(-50.0, 10.0, 0.0))]), V3::new(x, 0.0, 0.0), 0.0)
        };
        let dark = of(0.0, 60.0);
        assert!(dark.moments == Moments::default() && dark.shadow_rays > 300, "{dark:?}");
        assert!(of(0.0, -60.0).moments.c[0][0] > 0.05 && of(0.5, 60.0).moments.c[0][0] > 0.02);
    }

    /// A box turned inside out, double-sided: as bright inside as the plain one, and lit on the side met.
    #[test]
    fn double_sided_backs() {
        let two = built(sides(V3::ZERO, V3([-100.0; 3]), Material { cull: Cull::None, double_sided: true, ..plain() }, |_, _| true));
        let s = state(AMBIENT, vec![lamp(V3::new(0.0, 30.0, 0.0))]);
        let (a, b) = (of(&two, &s, UP * 9.0, 0.0), of(&room(plain()), &s, UP * 9.0, 0.0));
        assert!((0..4).all(|k| close(a.moments.c[k], b.moments.c[k], 1e-4)) && a.backs == 0 && a.moments.c[0][0] > 0.05, "{a:?} {b:?}");
    }

    /// The back of a double-sided surface is lit by the interpolated normal turned round, from the side met. One
    /// that is drawn on both sides but not double-sided keeps its front's normal: here the lamp on the side
    /// met is behind it, and the one on the other side has the wall itself in its way. A culled back is black.
    #[test]
    fn backs_by_hand() {
        let n = V3::new(0.28, 0.0, 0.96);
        let of = |cull, double_sided, z: f32| {
            let mut part = rectangle(V3::ZERO, V3::new(100.0, 0.0, 0.0), V3::new(0.0, 100.0, 0.0), Material { cull, double_sided, ..plain() });
            part.0.normals = vec![n.0; 4];
            let l = lamp(V3::new(-10.0, 20.0, z));
            let sun = Directional { display: V3([0.3; 3]), to_source: V3::new(0.0, 0.0, -1.0) };
            let s = LightState { directionals: vec![sun], ..state(AMBIENT, vec![l]) };
            (of(&built(vec![part]), &s, V3::new(0.0, 0.0, -100.0), 0.0), l, lights_of(&s).base(-n))
        };
        let (g, l, base) = of(Cull::None, true, -60.0);
        assert!(close(base, AMBIENT + V3([0.288; 3]), 1e-6), "{base:?}");
        let (mut want, mut hits) = (Moments::default(), 0);
        for d in directions(512) {
            let p = V3::new(0.0, 0.0, -100.0) + d * (100.0 / d[2]);
            if d[2] > 0.0 && p[0].abs() < 100.0 && p[1].abs() < 100.0 {
                hits += 1;
                want.add(d, added(ALBEDO, base, l.light(p, -n)));
            }
        }
        let want = want.over(512);
        assert!((g.hits, g.backs, g.shadow_rays) == (hits, 0, hits) && hits > 50 && want.c[0][0] > 0.01, "{g:?}");
        assert!((0..4).all(|k| close(g.moments.c[k], want.c[k], 1e-5)), "{g:?}, not {want:?}");
        let nothing = Probe { hits, lamps: hits, ..Probe::default() };
        assert_eq!(of(Cull::None, true, 60.0).0, nothing);
        assert_eq!(of(Cull::None, false, -60.0).0, nothing);
        assert_eq!(of(Cull::None, false, 60.0).0, Probe { shadow_rays: hits, ..nothing });
        assert_eq!(of(Cull::Back, false, -60.0).0, Probe { hits, backs: hits, ..Probe::default() });
    }

    /// A lamp in a closed shade bounces nothing outside of it, whichever sides the shade has, and of the shade
    /// only its own colour is sent, where it is unlit. A shade that is open lets light out.
    #[test]
    fn shades() {
        let of = |shade: Material, open: bool, lamps: Vec<Lamp>| {
            let mut parts = sides(V3::ZERO, V3([100.0; 3]), plain(), |_, _| true);
            // Fronts outward; where it is open, without the lower side.
            parts.extend(sides(V3::new(0.0, 60.0, 0.0), V3([-20.0; 3]), shade, |k, upper| !open || k != 1 || !upper));
            of(&built(parts), &state(AMBIENT, lamps), V3::new(5.0, -20.0, 0.0), 0.25)
        };
        let on = || vec![lamp(V3::new(0.0, 60.0, 0.0))];
        let both = Material { cull: Cull::None, double_sided: true, ..plain() };
        for shade in [plain(), both, Material { cull: Cull::Front, ..plain() }] {
            let g = of(shade, false, on());
            assert!(g.moments == Moments::default() && g.hits == 512 && g.shadow_rays > 400, "{g:?}");
        }
        assert!([plain(), both].into_iter().all(|shade| of(shade, true, on()).moments.c[0][0] > 0.01));
        // The shade's, which is above, and nothing else.
        let glow = Material { unlit: true, ..both };
        let [lit, dark] = [on(), Vec::new()].map(|lamps| of(glow, false, lamps));
        assert!(lit.moments == dark.moments && lit.shadow_rays > 400 && dark.shadow_rays == 0, "{lit:?} {dark:?}");
        let c = lit.moments.c;
        assert!(c[0][0] > 0.0 && c[2][0] > 0.0 && close(c[0], V3(ALBEDO) * (c[0][0] / ALBEDO[0]), 1e-6), "{c:?}");
    }

    /// 1 back of 4 hits is in the air, 2 are not; no hit is neither inside nor valid.
    #[test]
    fn inside_and_valid() {
        let met = |hits, backs| Probe { hits, backs, ..Probe::default() };
        assert!(met(4, 1).valid() && !met(4, 2).valid() && met(4, 2).inside() && !met(0, 0).valid() && !met(0, 0).inside());
        assert!(met(7, 1).valid() && met(7, 2).inside() && met(3, 1).inside());
        assert!(met(1 << 30, 1 << 30).inside() && met(u32::MAX, 1 << 30).inside() && met(1 << 30, 1 << 28).valid());
        let none = of(&prepared(SceneInput::default(), 1), &state(AMBIENT, vec![lamp(UP)]), V3::ZERO, 1.0);
        assert_eq!(none, Probe::default());
    }
}
