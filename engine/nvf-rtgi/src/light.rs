//! The formula of the engine's light sum in display values (nvforever lights/display_shader.rs; the attenuation is
//! lights.rs `GAME_ATTENUATION`, the cone Bevy's), with a grid over the lamps:
//!
//! ```text
//! S    = ambient + D max(0, N.L) + sum_i display_i att_i cone_i max(0, N.L_i)
//! att  = saturate(1 - d^2 / range^2)
//! cone = saturate((cos - cos outer) / max(cos inner - cos outer, 1e-4))^2
//! out  = min(albedo x max(S, 0)^2.2, 1)
//! ```
//!
//! Shadows are the caller's. The engine's ambient occlusion is not in it. The engine adds its terms in another
//! order, so the last bits can differ.

use crate::failed::{go_on, guarded, unavailable, Cancel, Failed, Reason};
use crate::vec3::V3;

/// The display gamma of the sum.
pub const POWER: f32 = 2.2;

/// A cell's light as it is now, in display values: those that are on, a flickering one at its mean.
#[derive(Clone, Debug, Default)]
pub struct LightState {
    pub ambient: V3,
    pub directionals: Vec<Directional>,
    /// Their order is the order they are summed in.
    pub lamps: Vec<Lamp>,
}

#[derive(Clone, Copy, Debug)]
pub struct Directional {
    pub display: V3,
    /// Unit, towards the source.
    pub to_source: V3,
}

#[derive(Clone, Copy, Debug)]
pub struct Lamp {
    pub position: V3,
    /// Its fade is in it.
    pub display: V3,
    /// Where it ends.
    pub range: f32,
    /// Of a spot.
    pub cone: Option<Cone>,
}

/// A spot's cone: where it shines (unit), and the scale and offset of the cosine.
#[derive(Clone, Copy, Debug)]
pub struct Cone {
    direction: V3,
    scale: f32,
    offset: f32,
}

impl Cone {
    /// From the inner and outer half angles (radians). A direction that has no unit vector, or an angle that is no
    /// number, stays one that is none: a light state with it is refused.
    pub fn new(direction: V3, inner: f32, outer: f32) -> Cone {
        let width = inner.cos() - outer.cos();
        let scale = 1.0 / if width.is_nan() { width } else { width.max(1e-4) };
        Cone { direction: direction.unit().unwrap_or(V3([f32::NAN; 3])), scale, offset: -outer.cos() * scale }
    }

    /// What it is held as: where it shines, then the scale and the offset that make the cosine from there its
    /// factor, clamped to 0..1 and squared.
    pub fn numbers(&self) -> [f32; 5] {
        let [x, y, z] = self.direction.0;
        [x, y, z, self.scale, self.offset]
    }

    /// The factor at a point `toward` from the lamp (unit).
    fn factor(&self, toward: V3) -> f32 {
        let c = (self.direction.dot(toward) * self.scale + self.offset).clamp(0.0, 1.0);
        c * c
    }
}

impl Lamp {
    /// The display-value light at a surface at `p` facing `n` (unit), nothing in the way.
    pub(crate) fn light(&self, p: V3, n: V3) -> V3 {
        let to = self.position - p;
        let d2 = to.dot(to);
        let reach = if self.range > 0.0 { (1.0 - d2 / (self.range * self.range)).clamp(0.0, 1.0) } else { 0.0 };
        if reach == 0.0 {
            return V3::ZERO;
        }
        // The cone's direction is unit near the lamp too, where the other is shorter; none right at it.
        let cone = self.cone.map_or(1.0, |c| c.factor(-to.unit().unwrap_or(V3::ZERO)));
        let to = to * (1.0 / d2.max(1e-4).sqrt());
        let facing = n.dot(to).clamp(0.0, 1.0);
        self.display * (reach * facing * cone)
    }

    fn finite(&self) -> bool {
        let cone = self.cone.is_none_or(|c| c.direction.finite() && c.scale.is_finite() && c.offset.is_finite());
        let near = self.position.max_abs() <= FURTHEST && self.range.abs() <= FURTHEST;
        near && self.position.finite() && self.display.finite() && cone
    }
}

impl LightState {
    fn finite(&self) -> bool {
        self.ambient.finite() && self.directionals.iter().all(|d| d.display.finite() && d.to_source.finite()) && self.lamps.iter().all(Lamp::finite)
    }
}

/// The lamps that can reach each cell of a grid over their spheres: the sphere against the
/// cell's box, which is wider by what rounding can do to a point's cell.
struct Grid {
    min: V3,
    size: V3,
    cells: [usize; 3],
    /// The lamps of cell i are `lamps[start[i]..start[i + 1]]`, in order.
    start: Vec<u32>,
    lamps: Vec<u32>,
}

/// The largest coordinate and range of a lamp, 2^21: what a cell of the grid is wider by stays small beside it.
const FURTHEST: f32 = 2_097_152.0;
/// Cells along the longest side.
const CELLS: f32 = 32.0;
/// The most lamps and directionals, and entries of the table.
const MOST_LAMPS: usize = 4096;
const MOST_ENTRIES: usize = 1 << 24;

impl Grid {
    fn new(lamps: &[Lamp], most: usize, cancel: Cancel) -> Result<Grid, Failed> {
        let reach = |l: &Lamp| V3([l.range.max(0.0); 3]);
        let (mut min, mut max) = (V3([f32::INFINITY; 3]), V3([f32::NEG_INFINITY; 3]));
        for l in lamps {
            (min, max) = (min.min(l.position - reach(l)), max.max(l.position + reach(l)));
        }
        if lamps.is_empty() {
            (min, max) = (V3::ZERO, V3::ZERO);
        }
        let side = ((max - min).max_abs() / CELLS).max(1.0);
        let cells: [usize; 3] = std::array::from_fn(|k| (((max[k] - min[k]) / side).ceil() as usize).max(1));
        // A point's cell and a cell's corners are off by at most half of this.
        let wide = V3([8.0 * f32::EPSILON * min.max_abs().max(max.max_abs()); 3]);
        let mut grid = Grid { min, size: V3([side; 3]), cells, start: vec![0], lamps: Vec::new() };
        for i in 0..cells.iter().product() {
            go_on(cancel)?;
            let at = [i % cells[0], i / cells[0] % cells[1], i / cells[0] / cells[1]];
            let lo = min + V3(at.map(|c| c as f32 * side));
            for (k, l) in lamps.iter().enumerate() {
                let nearest = l.position.max(lo - wide).min(lo + grid.size + wide) - l.position;
                // A little over, for the rounding of the distance.
                if nearest.dot(nearest) <= l.range * l.range * 1.001 + 1.0 {
                    if grid.lamps.len() >= most {
                        return unavailable(Reason::TooMany { what: "entries of the lamps' table", limit: most });
                    }
                    grid.lamps.push(k as u32);
                }
            }
            grid.start.push(grid.lamps.len() as u32);
        }
        Ok(grid)
    }

    fn at(&self, p: V3) -> &[u32] {
        let mut i = 0;
        for k in (0..3).rev() {
            let c = (p[k] - self.min[k]) / self.size[k];
            // No point that a lamp reaches is outside of min and max; one near max can be rounded to the edge itself.
            if !(c >= 0.0 && c <= self.cells[k] as f32) {
                return &[];
            }
            i = i * self.cells[k] + (c as usize).min(self.cells[k] - 1);
        }
        &self.lamps[self.start[i] as usize..self.start[i + 1] as usize]
    }
}

/// The lamps that may reach a place, as the solver looks them up, for another tracer that makes the same sum. With
/// c = (p - min) / side on each axis: outside of 0..=cells nothing reaches p; else its cell is c rounded down, and
/// one at `cells` (a point rounded onto the far edge) is the last. The lamps of cell i = (z cells.y + y) cells.x + x
/// are `lamps[start[i]..start[i + 1]]`, in the order they are summed. Each cell's lamps were found against a box a
/// little wider than the cell, for what rounding does to a point's cell.
#[derive(Clone, Debug, PartialEq)]
pub struct LampTable {
    pub min: V3,
    pub side: f32,
    pub cells: [u32; 3],
    pub start: Vec<u32>,
    /// Of [`LightState::lamps`].
    pub lamps: Vec<u32>,
}

impl LampTable {
    /// The table of a solve in this light; what a solve refuses is refused.
    pub fn of(light: &LightState, cancel: Cancel) -> Result<LampTable, Failed> {
        let Grid { min, size, cells, start, lamps } = guarded(|| Lights::new(light, cancel))?.grid;
        Ok(LampTable { min, side: size[0], cells: cells.map(|c| c as u32), start, lamps })
    }
}

/// A [`LightState`] as the rays ask for it.
pub struct Lights<'a> {
    state: &'a LightState,
    grid: Grid,
}

impl<'a> Lights<'a> {
    pub fn new(state: &'a LightState, cancel: Cancel) -> Result<Lights<'a>, Failed> {
        if state.lamps.len().max(state.directionals.len()) > MOST_LAMPS {
            return unavailable(Reason::TooMany { what: "lights", limit: MOST_LAMPS });
        }
        if !state.finite() {
            return unavailable(Reason::Input("light"));
        }
        Ok(Lights { grid: Grid::new(&state.lamps, MOST_ENTRIES, cancel)?, state })
    }

    /// Of the table.
    pub fn bytes(&self) -> usize {
        (self.grid.start.capacity() + self.grid.lamps.capacity()) * size_of::<u32>()
    }

    /// The lamps that may reach `p` (those of its cell), in order.
    pub fn near(&self, p: V3) -> impl Iterator<Item = &Lamp> {
        self.grid.at(p).iter().map(|i| &self.state.lamps[*i as usize])
    }

    /// The ambient and the directional part at a surface facing `n`.
    pub fn base(&self, n: V3) -> V3 {
        let sun = self.state.directionals.iter().fold(V3::ZERO, |s, d| s + d.display * n.dot(d.to_source).clamp(0.0, 1.0));
        self.state.ambient + sun
    }

    /// The whole sum, unshadowed, as drawn.
    pub fn sum(&self, p: V3, n: V3) -> V3 {
        let lamps = self.near(p).fold(V3::ZERO, |s, l| {
            let add = l.light(p, n);
            if add != V3::ZERO {
                s + add
            } else {
                s
            }
        });
        self.base(n) + lamps
    }
}

/// What the sum makes of a surface: min(albedo x max(S, 0)^2.2, 1) per channel, linear.
pub fn lit(albedo: [f32; 3], sum: V3) -> [f32; 3] {
    std::array::from_fn(|k| (albedo[k] * sum[k].max(0.0).powf(POWER)).min(1.0))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::vec3::tests::Rng;

    pub fn lights_of(state: &LightState) -> Lights<'_> {
        Lights::new(state, &|| false).unwrap()
    }

    const UP: V3 = V3([0.0, 1.0, 0.0]);

    fn lamp(position: V3, range: f32) -> Lamp {
        Lamp { position, display: V3::new(0.8, 0.4, 0.2), range, cone: None }
    }

    #[test]
    fn attenuation_and_facing() {
        let l = lamp(V3::new(0.0, 50.0, 0.0), 100.0);
        assert!((l.light(V3::ZERO, UP) - V3::new(0.6, 0.3, 0.15)).length() < 1e-6, "1 - 0.25");
        let aside = l.light(V3::new(50.0, 0.0, 0.0), UP);
        assert!((aside[0] - 0.8 * 0.5 * 0.5f32.sqrt()).abs() < 1e-6);
        assert_eq!(l.light(V3::ZERO, -UP), V3::ZERO);
        assert_eq!(l.light(V3::new(0.0, -50.0, 0.0), UP), V3::ZERO, "at the range");
        assert_eq!(lamp(V3::ZERO, 0.0).light(V3::ZERO, UP), V3::ZERO);
        assert!(l.light(l.position, UP).finite(), "at the lamp");
    }

    /// Full inside the inner angle, a quarter half way between the cosines, none outside.
    #[test]
    fn cones() {
        let c = Cone::new(V3::new(0.0, -3.0, 0.0), 0.3, 0.6);
        let at = |cos: f32| c.factor(V3::new((1.0 - cos * cos).sqrt(), -cos, 0.0));
        let half = (0.3f32.cos() + 0.6f32.cos()) / 2.0;
        assert_eq!((at(1.0), at(0.5)), (1.0, 0.0));
        assert!((at(half) - 0.25).abs() < 1e-4 && at(0.6f32.cos()) < 1e-6 && (at(0.3f32.cos()) - 1.0).abs() < 1e-4);
        let spot = Lamp { cone: Some(c), ..lamp(V3::new(0.0, 50.0, 0.0), 100.0) };
        assert!((spot.light(V3::ZERO, UP)[0] - 0.6).abs() < 1e-6);
        assert_eq!(spot.light(V3::new(50.0, 0.0, 0.0), UP), V3::ZERO);
        // Right at the lamp the angle still counts, and what faces it is lit by its distance over 0.01.
        let near = Lamp { cone: Some(Cone::new(-UP, 0.3, 0.5)), ..lamp(V3::new(0.0, 0.005, 0.0), 100.0) };
        assert!((near.light(V3::ZERO, UP) - near.display * 0.5).length() < 1e-6, "{:?}", near.light(V3::ZERO, UP));
        assert_eq!(near.light(V3::new(0.005, 0.0, 0.0), UP), V3::ZERO, "45 degrees off");
        assert_eq!(near.light(near.position, UP), V3::ZERO);
    }

    #[test]
    fn lit_texels() {
        let out = lit([0.5, 1.0, 0.25], V3::new(0.5, 1.5, -1.0));
        assert!((out[0] - 0.5 * 0.5f32.powf(2.2)).abs() < 1e-6);
        assert_eq!((out[1], out[2]), (1.0, 0.0));
    }

    /// The grid leaves out no lamp that adds light, and keeps their order.
    #[test]
    fn the_grid_finds_every_lamp() {
        let mut rng = Rng(5);
        let mut place = |s: f32| V3(std::array::from_fn(|_| rng.draw() * s - 100.0));
        let lamps: Vec<Lamp> = (0..300).map(|i| lamp(place(6000.0), 20.0 + (i * 7 % 400) as f32)).collect();
        let state = LightState { lamps, ..LightState::default() };
        let lights = lights_of(&state);
        let sum = |p: V3| lights.near(p).fold(V3::ZERO, |s, l| s + l.light(p, UP));
        let (mut some, mut asked) = (0, 0);
        for _ in 0..3000 {
            let p = place(6200.0);
            let all = state.lamps.iter().fold(V3::ZERO, |s, l| s + l.light(p, UP));
            assert_eq!(sum(p), all);
            assert_eq!(lights.sum(p, UP), all);
            some += usize::from(all != V3::ZERO);
            asked += lights.near(p).count();
        }
        assert!(some > 100 && asked < 3000 * 30, "{some} {asked}");
        assert_eq!(sum(V3([f32::NAN; 3])), V3::ZERO);
        let sun = LightState { ambient: UP, directionals: vec![Directional { display: UP, to_source: UP }], lamps: Vec::new() };
        let lights = lights_of(&sun);
        assert!(lights.base(UP) == UP * 2.0 && lights.sum(V3::ZERO, UP) == UP * 2.0 && lights.base(-UP) == UP && lights.near(V3::ZERO).count() == 0);
    }

    /// Nor one at the grid's upper edge or at a cell's, where a point's cell is rounded: one of ordinary sizes, and
    /// of the largest coordinates there may be.
    #[test]
    fn the_grid_at_its_edges() {
        let far = -FURTHEST;
        // The middle one's point is a sixteenth below the corner of cell 20, and rounded up to it; the last one's is
        // rounded to the grid's end.
        let [of_rooms, of_worlds] = [vec![(-2900.3, 100.0), (-100.0, 100.0)], vec![(far, 1.0), (-786_491.0 - 0.8125, 60.01), (0.0, 1.0)]]
            .map(|l| LightState { lamps: l.into_iter().map(|(x, range)| lamp(V3::new(x, 0.0, 0.0), range)).collect(), ..LightState::default() });
        let aside = V3::new(-1.0, 0.0, 0.0);
        for (state, p, n) in [
            (&of_rooms, V3::new(-1e-5, 0.0, 0.0), aside),
            (&of_rooms, V3::new(-100.0, -99.9, 0.0), UP),
            (&of_worlds, V3::new(0.99, 0.0, 0.0), aside),
            (&of_worlds, V3::new(-786_431.0 - 0.8125, 0.0, 0.0), aside),
            (&of_worlds, V3::new(far, 0.5, 0.0), -UP),
        ] {
            let all = state.lamps.iter().fold(V3::ZERO, |s, l| s + l.light(p, n));
            assert_eq!(lights_of(state).near(p).fold(V3::ZERO, |s, l| s + l.light(p, n)), all, "{p:?}");
            assert!(all != V3::ZERO, "{p:?}");
        }
        let x = |s: &LightState, x: f32| lights_of(s).near(V3::new(x, 0.0, 0.0)).count();
        assert!(x(&of_rooms, 200.0) == 0 && x(&of_rooms, -3200.0) == 0 && x(&of_rooms, f32::NAN) == 0);
    }

    /// Small lamps in a small grid as far off as they may be: the cells are not so much wider that all hold all.
    #[test]
    fn the_table_far_off() {
        let at = |i: usize| V3([FURTHEST - if i.is_multiple_of(2) { 31.0 } else { 1.0 }; 3]);
        let state = LightState { lamps: (0..MOST_LAMPS).map(|i| lamp(at(i), 1.0)).collect(), ..LightState::default() };
        let lights = Lights::new(&state, &|| false).unwrap();
        assert_eq!(lights.grid.cells, [32; 3]);
        assert_eq!(lights.near(at(1)).count(), MOST_LAMPS / 2);
    }

    /// The table as it is handed out names the lamps that the solver's lookup names, by the rule of its doc, at the
    /// grid's edges too; what a solve refuses is refused.
    #[test]
    fn the_table_handed_out() {
        let named = |t: &LampTable, p: V3| -> Vec<u32> {
            let c: [f32; 3] = std::array::from_fn(|k| (p[k] - t.min[k]) / t.side);
            if !(0..3).all(|k| (0.0..=t.cells[k] as f32).contains(&c[k])) {
                return Vec::new();
            }
            let [x, y, z]: [usize; 3] = std::array::from_fn(|k| (c[k] as usize).min(t.cells[k] as usize - 1));
            let i = (z * t.cells[1] as usize + y) * t.cells[0] as usize + x;
            t.lamps[t.start[i] as usize..t.start[i + 1] as usize].to_vec()
        };
        let mut rng = Rng(5);
        let mut place = |s: f32| V3(std::array::from_fn(|_| rng.draw() * s - 100.0));
        let state = |l: Vec<(f32, f32)>| LightState { lamps: l.into_iter().map(|(x, range)| lamp(V3::new(x, 0.0, 0.0), range)).collect(), ..LightState::default() };
        let many = LightState { lamps: (0..300).map(|i| lamp(place(6000.0), 20.0 + (i * 7 % 400) as f32)).collect(), ..LightState::default() };
        let (rooms, worlds) = (state(vec![(-2900.3, 100.0), (-100.0, 100.0)]), state(vec![(-FURTHEST, 1.0), (-786_491.0 - 0.8125, 60.01), (0.0, 1.0)]));
        let edges = [(-1e-5, 0.0), (-100.0, -99.9), (0.99, 0.0), (-786_431.0 - 0.8125, 0.0), (-FURTHEST, 0.5), (200.0, 0.0), (-3200.0, 0.0), (f32::NAN, 0.0)];
        for state in [&many, &rooms, &worlds] {
            let (t, lights) = (LampTable::of(state, &|| false).unwrap(), lights_of(state));
            let mut points: Vec<V3> = (0..3000).map(|_| place(6200.0)).chain(edges.map(|(x, y)| V3::new(x, y, 0.0))).collect();
            points.extend([t.min, t.min + V3(t.cells.map(|c| c as f32 * t.side))]);
            for p in points {
                let index = |l: &Lamp| state.lamps.iter().position(|m| std::ptr::eq(m, l)).unwrap() as u32;
                assert_eq!(lights.near(p).map(index).collect::<Vec<_>>(), named(&t, p), "{p:?}");
            }
        }
        let refused = |l: Lamp| LampTable::of(&LightState { lamps: vec![l], ..LightState::default() }, &|| false).err();
        let above = f32::from_bits(FURTHEST.to_bits() + 1);
        assert_eq!(refused(lamp(V3([FURTHEST; 3]), FURTHEST)), None);
        for l in [lamp(V3::new(0.0, above, 0.0), 9.0), lamp(UP, above), Lamp { cone: Some(Cone::new(V3::ZERO, 0.3, 0.6)), ..lamp(UP, 9.0) }] {
            assert_eq!(refused(l), Some(Failed::Unavailable(Reason::Input("light"))), "{l:?}");
        }
        assert_eq!(LampTable::of(&many, &|| true), Err(Failed::Cancelled));
        let c = Cone::new(V3::new(0.0, -2.0, 0.0), 0.3, 0.6).numbers();
        let scale = 1.0 / (0.3f32.cos() - 0.6f32.cos());
        assert_eq!(c, [0.0, -1.0, 0.0, scale, -0.6f32.cos() * scale]);
    }

    #[test]
    fn numbers_that_are_not() {
        let state = |l: Lamp| LightState { lamps: vec![l], ..LightState::default() };
        assert!(LightState::default().finite() && state(lamp(UP, 9.0)).finite());
        assert!(!state(lamp(UP, f32::INFINITY)).finite() && !state(lamp(V3([f32::NAN; 3]), 1.0)).finite());
        let spot = |direction, inner, outer| state(Lamp { cone: Some(Cone::new(direction, inner, outer)), ..lamp(UP, 9.0) });
        assert!(spot(UP, 0.3, 0.3).finite(), "no wider than 1e-4");
        let wrong = V3::new(0.0, f32::NAN, 1.0);
        for s in [spot(UP, f32::NAN, 0.3), spot(UP, 0.3, f32::NAN), spot(UP, f32::INFINITY, 0.3), spot(wrong, 0.3, 0.6), spot(V3([f32::INFINITY; 3]), 0.3, 0.6), spot(V3::ZERO, 0.3, 0.6)] {
            assert!(!s.finite(), "{s:?}");
        }
        assert!(!LightState { ambient: V3([f32::INFINITY; 3]), ..LightState::default() }.finite());
        assert!(state(lamp(V3([FURTHEST; 3]), FURTHEST)).finite());
        assert!(!state(lamp(V3([FURTHEST * 1.1; 3]), 9.0)).finite() && !state(lamp(UP, FURTHEST * 1.1)).finite());
    }

    #[test]
    fn what_is_refused() {
        let state = |n| LightState { lamps: vec![lamp(UP, 0.0); n], ..LightState::default() };
        let why = |s: &LightState, cancel: Cancel| Lights::new(s, cancel).err();
        assert!(Lights::new(&state(MOST_LAMPS), &|| false).is_ok_and(|l| l.bytes() > 4 * MOST_LAMPS));
        assert_eq!(why(&state(MOST_LAMPS + 1), &|| false), Some(Failed::Unavailable(Reason::TooMany { what: "lights", limit: MOST_LAMPS })));
        assert_eq!(why(&state(1), &|| true), Some(Failed::Cancelled));
        assert_eq!(why(&LightState { ambient: V3([f32::NAN; 3]), ..state(1) }, &|| false), Some(Failed::Unavailable(Reason::Input("light"))));
        // Counted before they are looked at.
        let wrong = LightState { lamps: vec![lamp(V3([f32::NAN; 3]), 0.0); MOST_LAMPS + 1], ..LightState::default() };
        assert!(matches!(why(&wrong, &|| true), Some(Failed::Unavailable(Reason::TooMany { .. }))));
        let table = |most| Grid::new(&[lamp(UP, 50.0); 2], most, &|| false).map(|g| g.lamps.len());
        let whole = table(usize::MAX).unwrap();
        assert!(whole > 100 && table(whole) == Ok(whole));
        for limit in [0, 100, whole - 1] {
            assert!(matches!(table(limit), Err(Failed::Unavailable(Reason::TooMany { limit: l, .. })) if l == limit));
        }
    }
}
