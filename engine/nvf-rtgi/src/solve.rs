//! From a prepared scene, its light and the seeds to a volume.

use crate::failed::{guarded, unavailable, Cancel, Failed, Reason};
use crate::floor::Floor;
use crate::gather::Moments;
use crate::grid::{tree, Coverage, Grid, Place};
use crate::light::LightState;
use crate::reach::{GatherConfig, EMISSIONS, SPACINGS};
use crate::trace::PreparedScene;
use crate::vec3::V3;
use crate::volume::{allowed, packed, Reader, Refused};
use std::collections::{BTreeMap, BTreeSet};

/// From where a cell is reached.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Seeds {
    /// Places in the air. Their order does not matter.
    pub points: Vec<V3>,
    /// What a fit looks from, where the points are a floor's. `None`: they are places of their own, such as
    /// that of an arrival, and a fit looks from the points of the grid. A solve does not look at it.
    pub floor: Option<Floor>,
}

impl Seeds {
    /// The eye above the middle of each triangle of a floor.
    pub fn of(floor: Floor) -> Seeds {
        Seeds { points: floor.eyes(), floor: Some(floor) }
    }

    /// Without a floor.
    pub fn at(points: Vec<V3>) -> Seeds {
        Seeds { points, floor: None }
    }
}

/// What is asked of a solve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolveConfig {
    pub gather: GatherConfig,
    /// Up a surface's geometric normal, where a reader looks; the points themselves do not depend on it.
    pub offset: f32,
    /// The texture may take no more, which is looked at before any of its box is.
    pub atlas_bytes: u64,
    /// Whether the points of the box that the flood did not ask for are gathered too, for [`Counts`] and
    /// [`Stats`] alone: the texture is the same without.
    pub census: bool,
}

impl Default for SolveConfig {
    fn default() -> SolveConfig {
        SolveConfig { gather: GatherConfig::default(), offset: 24.0, atlas_bytes: 64 << 20, census: false }
    }
}

/// How many points of the box about the reached ones there are, by what they are.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Counts {
    pub points: usize,
    pub reached: usize,
    /// In the air but not reached.
    pub unreached: usize,
    pub inside: usize,
    pub no_hit: usize,
    /// Not looked at (none with [`SolveConfig::census`]).
    pub unknown: usize,
}

/// What it took.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    /// Points gathered, and their rays.
    pub points: usize,
    pub rays: u64,
    /// The lamps that were looked at and the shadow rays ([`crate::Probe`]), summed over the points.
    pub lamps: u64,
    pub shadow_rays: u64,
    /// About what the solve held at most beside the scene, the texture included; by its parts' sizes, not measured.
    pub peak_bytes: u64,
}

/// A cell's bounce light.
#[derive(Clone, Debug, PartialEq)]
pub struct SolvedVolume {
    /// The first point as multiples of the spacing, and its place.
    pub low: Place,
    pub origin: [f32; 3],
    pub spacing: f32,
    pub offset: f32,
    /// What it was solved with.
    pub emission: f32,
    /// The grid's, without the shell; the texture's.
    pub dimensions: [u32; 3],
    pub atlas: [u32; 3],
    /// The texture: 3D, rgba16float, little-endian, x fastest, then y, then z, without row padding. Four slabs one
    /// after another along z, each of the grid's dimensions with a texel of zeros all around:
    /// `C0, m`, `Cx, m v`, `Cy, 0`, `Cz, 0`. m is 1 at a point that is reached and else 0, where all else is 0 too;
    /// C0 and Cj are [`crate::Moments`], v the luminance of C0. [`crate::Reader`] has the read.
    pub rgba16f_le: Vec<u8>,
    pub counts: Counts,
    pub coverage: Coverage,
    pub stats: Stats,
}

/// The box about the points that are reached.
struct Extent {
    low: Place,
    dimensions: [u32; 3],
    atlas: [u32; 3],
    counts: Counts,
}

impl Extent {
    /// Of at least one point.
    fn of(grid: &mut Grid, reached: &BTreeSet<Place>, config: &SolveConfig) -> Result<Extent, Failed> {
        let (mut low, mut high) = ([i32::MAX; 3], [i32::MIN; 3]);
        for at in reached {
            low = std::array::from_fn(|k| low[k].min(at[k]));
            high = std::array::from_fn(|k| high[k].max(at[k]));
        }
        let dimensions: [u32; 3] = std::array::from_fn(|k| high[k].abs_diff(low[k]) + 1);
        let (atlas, _) = allowed(dimensions, config.atlas_bytes)?;
        // Fewer than the texture's bytes.
        let points: usize = dimensions.iter().map(|d| *d as usize).product();
        if config.census {
            let limit = config.gather.limits.points;
            if points > limit {
                return unavailable(Reason::TooManyPoints { limit });
            }
            grid.make((low[2]..=high[2]).flat_map(|z| (low[1]..=high[1]).flat_map(move |y| (low[0]..=high[0]).map(move |x| [x, y, z]))))?;
        }
        let mut counts = Counts { points, reached: reached.len(), ..Counts::default() };
        for (at, s) in grid.made().filter(|(at, _)| (0..3).all(|k| (low[k]..=high[k]).contains(&at[k]))) {
            if s.valid() {
                counts.unreached += usize::from(!reached.contains(at));
            } else if s.hits == 0 {
                counts.no_hit += 1;
            } else {
                counts.inside += 1;
            }
        }
        counts.unknown = points - counts.reached - counts.unreached - counts.no_hit - counts.inside;
        Ok(Extent { low, dimensions, atlas, counts })
    }
}

impl SolvedVolume {
    pub(crate) fn of(scene: &PreparedScene, light: &LightState, seeds: &[V3], config: &SolveConfig, cancel: Cancel) -> Result<SolvedVolume, Failed> {
        let GatherConfig { spacing, rays, emission, .. } = config.gather;
        if !config.offset.is_finite() {
            return unavailable(Reason::Input("offset"));
        }
        let lights = config.gather.lights(light, seeds, cancel)?;
        let (mut grid, reached, coverage) = config.gather.flood(scene, &lights, seeds, cancel)?;
        let Extent { low, dimensions, atlas, counts } = Extent::of(&mut grid, &reached, config)?;
        let lit = grid.made().filter(|(at, _)| reached.contains(*at)).map(|(at, s)| (at, &s.moments));
        let rgba16f_le = packed(low, dimensions, lit, config.atlas_bytes, cancel)?;
        let mut stats = Stats::default();
        for (_, s) in grid.made() {
            stats.points += 1;
            stats.rays += rays as u64;
            stats.lamps += u64::from(s.lamps);
            stats.shadow_rays += u64::from(s.shadow_rays);
        }
        let points = tree(reached.len(), size_of::<Place>());
        stats.peak_bytes = (lights.bytes() + grid.bytes() + points + grid.flood_bytes().max(rgba16f_le.capacity())) as u64;
        let origin = low.map(|c| c as f32 * spacing);
        Ok(SolvedVolume { low, origin, spacing, offset: config.offset, emission, dimensions, atlas, rgba16f_le, counts, coverage, stats })
    }
}

impl SolvedVolume {
    /// A volume of points whose light was gathered elsewhere, such as at the places of a [`crate::reach_by`], packed
    /// as a solve packs the points it reaches: the same places and light are the same box, texture and bytes. What its
    /// [`crate::Reader`] would refuse is refused. The
    /// config's spacing, emission, offset and bytes are the volume's. Its counts are of the reached points alone, the
    /// rest of the box unknown; its stats are none.
    pub fn of_points(points: &BTreeMap<Place, Moments>, coverage: Coverage, config: &SolveConfig) -> Result<SolvedVolume, Failed> {
        guarded(|| {
            let GatherConfig { spacing, emission, .. } = config.gather;
            if !SPACINGS.contains(&spacing) || !EMISSIONS.contains(&emission) || !config.offset.is_finite() {
                return unavailable(Reason::Input("volume"));
            }
            let Some(first) = points.keys().next() else {
                return unavailable(Reason::NothingReached { seeds: coverage.seeds });
            };
            let (mut low, mut high) = (*first, *first);
            for at in points.keys() {
                low = std::array::from_fn(|k| low[k].min(at[k]));
                high = std::array::from_fn(|k| high[k].max(at[k]));
            }
            // A box of every i32 has no u32 side: saturated, it is one too large.
            let dimensions: [u32; 3] = std::array::from_fn(|k| high[k].abs_diff(low[k]).saturating_add(1));
            let (atlas, _) = allowed(dimensions, config.atlas_bytes)?;
            let rgba16f_le = packed(low, dimensions, points.iter(), config.atlas_bytes, &|| false)?;
            let all: usize = dimensions.iter().map(|d| *d as usize).product();
            let counts = Counts { points: all, reached: points.len(), unknown: all - points.len(), ..Counts::default() };
            let origin = low.map(|c| c as f32 * spacing);
            let out = SolvedVolume { low, origin, spacing, offset: config.offset, emission, dimensions, atlas, rgba16f_le, counts, coverage, stats: Stats::default() };
            // Light that no solve has, such as a luminance below 0, is what its reader refuses.
            match Reader::scanned(&out, &|| false, |_| ()) {
                Ok(_) => Ok(out),
                Err(Refused::Cancelled) => Err(Failed::Cancelled),
                Err(Refused::Because(_)) => unavailable(Reason::Input("moments")),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gather::tests::{axis, lamp, plain, rectangle, sides, state, AMBIENT};
    use crate::light::Lamp;
    use crate::reach::tests::{furnished, left, light, right, walled};
    use crate::reach::{GatherLimits, MOST_RAYS};
    use crate::scene::{Material, SceneInput};
    use crate::trace::tests::{prepared, scene};
    use crate::volume::tests::volume;
    use crate::volume::{from_half, Reader};
    use crate::{reach, solve};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const NEVER: Cancel = &|| false;

    fn gather() -> GatherConfig {
        GatherConfig { emission: 0.0, threads: 2, ..GatherConfig::default() }
    }

    fn config() -> SolveConfig {
        SolveConfig { gather: gather(), census: true, ..SolveConfig::default() }
    }

    fn run(world: &PreparedScene, light: &LightState, seeds: &[V3], config: &SolveConfig) -> Result<SolvedVolume, Failed> {
        solve(world, light, &Seeds::at(seeds.to_vec()), config, NEVER)
    }

    /// A flood stays in its room, and its light is there.
    #[test]
    fn walls() {
        let world = prepared(scene(walled(), Vec::new()), 1);
        let one = run(&world, &light(), &[left()], &config()).unwrap();
        assert!(one.low == [-6, 0, 0] && one.dimensions == [7, 4, 4] && one.counts.reached == 112 && one.counts.points == 112 && one.coverage.parts == 1, "{:?}", one.counts);
        assert!((one.origin, one.spacing, one.offset, one.emission) == ([-384.0, 0.0, 0.0], 64.0, 24.0, 0.0) && one.atlas == [9, 6, 24] && one.rgba16f_le.len() == 9 * 6 * 24 * 8);
        let both = run(&world, &light(), &[left(), right()], &config()).unwrap();
        assert!(both.coverage.parts == 2 && both.coverage.seeds == 2 && both.counts.reached == both.coverage.reached);
        let reader = Reader::new(&both).unwrap();
        let mut lit = 0;
        for i in 0..both.counts.points as u32 {
            let [w, h, _] = both.dimensions;
            let at = [i % w, i / w % h, i / (w * h)];
            let t = reader.slabs(at.map(|c| c as usize + 1));
            let x = both.low[0] + at[0] as i32;
            if t[0][3] == 1.0 {
                // The dark room's points are valid and black.
                assert!(if x > 0 { t[0][0] == 0.0 && t[1][3] == 0.0 } else { t[0][0] > 0.0 && t[1][3] > 0.0 }, "{at:?} {t:?}");
                lit += usize::from(x <= 0);
            }
        }
        assert_eq!(lit, one.counts.reached);
        // The read leaks through the thin wall, as it has to by its definition, and stays black further off.
        assert!(reader.read_at(V3::new(44.0, 100.0, 100.0), axis(0)).e()[0] > 0.0);
        let dark = reader.read(V3::new(40.0, 100.0, 100.0), axis(0), axis(0));
        assert!(dark.v() < 1e-5 && dark.support > 0.99, "{dark:?}");
        let there = reader.read(V3::new(-128.0, 40.0, 64.0), axis(2), axis(2));
        assert!(there.e()[0] > 0.01 && (there.support - 1.0).abs() < 1e-3, "{there:?}");
        // Another spacing: the same room, other points.
        let wide = run(&world, &light(), &[left()], &SolveConfig { gather: GatherConfig { spacing: 96.0, ..gather() }, ..config() }).unwrap();
        assert!(wide.low[0] == -4 && wide.dimensions[0] == 5 && wide.spacing == 96.0);
    }

    /// The texels are what `reach` gathers at the places they stand for, to a half float; the same volume, counts
    /// and figures on any number of threads, from a scene prepared on any.
    #[test]
    fn what_is_reached() {
        let (input, light) = furnished();
        let seeds = [left(), right(), V3::new(-300.0, 60.0, 60.0)];
        let of = |threads: usize| run(&prepared(input.clone(), threads), &light, &seeds, &SolveConfig { gather: GatherConfig { threads, emission: 0.25, ..gather() }, ..config() }).unwrap();
        let v = of(1);
        for threads in [4, 12] {
            assert!(of(threads) == v, "{threads}");
        }
        assert!(v.stats.shadow_rays > 1000 && v.stats.rays == 512 * v.stats.points as u64 && v.stats.peak_bytes > v.rgba16f_le.len() as u64, "{:?}", v.stats);
        assert_eq!((v.low, v.dimensions, v.counts), ([-6, 0, 0], [14, 4, 4], Counts { points: 224, reached: 220, inside: 4, ..Counts::default() }));
        let reached = reach(&prepared(input, 2), &light, &seeds, &GatherConfig { emission: 0.25, ..gather() }, NEVER).unwrap();
        assert!(reached.points.len() == 220 && reached.coverage == v.coverage);
        let reader = Reader::new(&v).unwrap();
        for (at, p) in &reached.points {
            let t = reader.slabs([(at[0] + 7) as usize, (at[1] + 1) as usize, (at[2] + 1) as usize]);
            for (slab, c) in p.moments.c.iter().enumerate() {
                assert!((0..3).all(|k| (t[slab][k] - c[k]).abs() <= c[k].abs() / 2048.0 + 3e-8), "{at:?} {slab}: {t:?}, not {c:?}");
            }
            let c = p.moments.c[0];
            let luminance = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            assert!(t[0][3] == 1.0 && (t[1][3] - luminance).abs() <= luminance / 2048.0 + 1e-7, "{at:?}");
        }
    }

    /// Two triangles in one place: the first one's colour is sent on, whichever it is.
    #[test]
    fn coplanar_triangles_by_their_order() {
        let cover = |a: usize, b: usize| {
            let colours = [[0.9, 0.1, 0.1, 1.0], [0.1, 0.1, 0.9, 1.0]];
            let floor = |i: usize| rectangle(V3::new(-190.0, -50.0, 100.0), V3::new(0.0, 0.0, 150.0), V3::new(210.0, 0.0, 0.0), Material { base: colours[i], ..plain() });
            let mut parts = vec![floor(a), floor(b)];
            parts.extend(sides(V3::new(-190.0, 100.0, 100.0), V3::new(210.0, 150.0, 150.0), plain(), |k, upper| k != 1 || upper));
            run(&prepared(scene(parts, Vec::new()), 2), &light(), &[left()], &config()).unwrap()
        };
        let (red, blue) = (cover(0, 1), cover(1, 0));
        // Of C0, the first slab.
        let mean = |v: &SolvedVolume| {
            let texels = v.rgba16f_le.as_chunks::<8>().0;
            [0, 1, 2].map(|k| texels[..texels.len() / 4].iter().map(|t| from_half(u16::from_le_bytes([t[2 * k], t[2 * k + 1]]))).sum::<f32>())
        };
        let (r, b) = (mean(&red), mean(&blue));
        assert!(r[0] > 1.1 * b[0] && b[2] > 1.1 * r[2], "{r:?} {b:?}");
        assert!(red == cover(0, 1));
    }

    /// Emission is in the light at its share, lamps or not, and the ambient alone gives a black, valid field. Light
    /// that is the same from all around is itself on a surface, whichever way that faces: its irradiance over pi.
    #[test]
    fn black_and_emission() {
        let world = prepared(scene(walled(), Vec::new()), 1);
        let black = run(&world, &state(AMBIENT, Vec::new()), &[left()], &config()).unwrap();
        let r = Reader::new(&black).unwrap().read_at(left(), axis(0));
        assert!((r.e(), r.v()) == (V3::ZERO, 0.0) && (r.support - 1.0).abs() < 1e-5 && black.stats.shadow_rays == 0);
        assert!(black.rgba16f_le.as_chunks::<8>().0.iter().all(|t| t[..6] == [0; 6]), "no sign of a zero either");
        let glow = prepared(scene(sides(V3::new(-190.0, 100.0, 100.0), V3::new(210.0, 150.0, 150.0), Material { unlit: true, ..plain() }, |_, _| true), Vec::new()), 1);
        let v = run(&glow, &state(AMBIENT, Vec::new()), &[left()], &SolveConfig { gather: GatherConfig { emission: 0.25, ..gather() }, ..config() }).unwrap();
        for i in 0..6 {
            let r = Reader::new(&v).unwrap().read_at(left(), axis(i));
            let want = V3::new(0.125, 0.0625, 0.2);
            assert!((r.e() - want).max_abs() < 2e-3 && (r.v() - 0.085715).abs() < 1e-3, "{r:?}");
        }
    }

    /// Half a sky: white above the middle of a room, black below. By hand the irradiance over pi of a surface that is
    /// turned by the angle a from the zenith is (1 + cos a) / 2.
    #[test]
    fn half_a_sky() {
        let glow = Material { unlit: true, base: [1.0; 4], ..plain() };
        let half = V3::new(100.0, 50.0, 100.0);
        let mut parts = sides(V3::new(0.0, 50.0, 0.0), half, glow, |k, upper| k != 1 || upper);
        parts.extend(sides(V3::new(0.0, -50.0, 0.0), half, plain(), |k, upper| k != 1 || !upper));
        let v = run(&prepared(scene(parts, Vec::new()), 1), &state(AMBIENT, Vec::new()), &[V3::ZERO], &SolveConfig { gather: GatherConfig { emission: 1.0, ..gather() }, ..config() }).unwrap();
        let reader = Reader::new(&v).unwrap();
        for (n, want) in [(axis(2), 1.0), (axis(3), 0.0), (axis(0), 0.5), (axis(5), 0.5), (V3::new(0.0, 0.5, 0.75f32.sqrt()), 0.75), (V3::new(-0.8, -0.6, 0.0), 0.2)] {
            let r = reader.read_at(V3::ZERO, n);
            assert!((r.e() - V3([want; 3])).max_abs() < 5e-3 && (r.v() - 0.5).abs() < 2e-3, "{n:?} {r:?}");
        }
        // 24 down the normal of a surface above it, it is the same place.
        let read = reader.read(V3::new(0.0, 24.0, 0.0), axis(3), axis(2));
        assert!((read.e() - V3([1.0; 3])).max_abs() < 5e-3, "{read:?}");
    }

    /// The census counts the rest of the box, which is in what it took too, and changes nothing else.
    #[test]
    fn the_census() {
        // A solid block on the floor of the room: its fronts look outward.
        let mut parts = walled();
        parts.extend(sides(V3::new(-190.0, 30.0, 100.0), V3([-70.0; 3]), plain(), |_, _| true));
        let world = prepared(scene(parts, Vec::new()), 1);
        let of = |census| run(&world, &light(), &[left(), V3::new(-190.0, 180.0, 100.0)], &SolveConfig { census, ..config() }).unwrap();
        let (whole, quick) = (of(true), of(false));
        assert!(!SolveConfig::default().census);
        assert!(whole.counts.unknown == 0 && whole.counts.inside > 0 && whole.counts.reached < whole.counts.points, "{:?}", whole.counts);
        assert!(whole.rgba16f_le == quick.rgba16f_le && whole.coverage == quick.coverage && whole.low == quick.low && whole.counts.points == quick.counts.points);
        let n = quick.counts;
        assert!(n.unknown > 0 && n.reached + n.unreached + n.inside + n.no_hit + n.unknown == n.points && quick.stats.points < whole.stats.points, "{n:?}");
    }

    /// `reach` and `solve` refuse the same input for the same reason.
    #[test]
    fn one_set_of_rules() {
        let world = prepared(scene(walled(), Vec::new()), 1);
        let empty = prepared(SceneInput::default(), 4);
        let wrong = state(AMBIENT, vec![lamp(V3([f32::NAN; 3]))]);
        let limits = |points, seeds| GatherConfig { limits: GatherLimits { points, seeds }, ..gather() };
        let nowhere = V3([f32::NAN; 3]);
        for (world, light, seeds, gather, reason) in [
            (&world, &light(), vec![left()], GatherConfig { spacing: 0.0, ..gather() }, Reason::Input("spacing")),
            (&world, &light(), vec![left()], GatherConfig { emission: 1.001, ..gather() }, Reason::Input("emission")),
            (&world, &light(), vec![left()], GatherConfig { rays: MOST_RAYS + 1, ..gather() }, Reason::Input("rays")),
            (&world, &light(), vec![left()], GatherConfig { threads: 0, ..gather() }, Reason::Input("threads")),
            (&world, &light(), vec![], gather(), Reason::NoSeeds),
            (&world, &light(), vec![left(); 3], limits(1000, 2), Reason::TooMany { what: "seeds", limit: 2 }),
            (&world, &wrong, vec![left()], limits(0, 1), Reason::TooManyPoints { limit: 0 }),
            (&world, &light(), vec![left()], limits(20, 1), Reason::TooManyPoints { limit: 20 }),
            (&world, &wrong, vec![left()], gather(), Reason::Input("light")),
            (&world, &light(), vec![nowhere], gather(), Reason::NothingReached { seeds: 1 }),
            (&empty, &light(), vec![left()], gather(), Reason::NothingReached { seeds: 1 }),
        ] {
            let want = Failed::Unavailable(reason);
            assert_eq!(reach(world, light, &seeds, &gather, NEVER), Err(want));
            assert_eq!(run(world, light, &seeds, &SolveConfig { gather, ..config() }), Err(want));
        }
        assert_eq!(run(&world, &light(), &[left(), nowhere], &config()).unwrap().coverage.failed, 1);
    }

    #[test]
    fn what_is_unavailable() {
        let world = prepared(scene(walled(), Vec::new()), 1);
        let why = |config: SolveConfig| run(&world, &light(), &[left()], &config).unwrap_err();
        for offset in [f32::INFINITY, f32::NAN] {
            assert_eq!(why(SolveConfig { offset, ..config() }), Failed::Unavailable(Reason::Input("offset")));
        }
        let bytes = 9 * 6 * 24 * 8;
        assert_eq!(why(SolveConfig { atlas_bytes: bytes - 1, ..config() }), Failed::Unavailable(Reason::TooManyBytes { bytes, limit: bytes - 1 }));
        assert!(run(&world, &light(), &[left()], &SolveConfig { atlas_bytes: bytes, ..config() }).is_ok());
        // Light that is taken away from a colour below 0 adds more than a half float holds.
        let dark = prepared(scene(sides(V3::ZERO, V3([100.0; 3]), Material { base: [-1e8, 0.5, 0.5, 1.0], ..plain() }, |_, _| true), Vec::new()), 1);
        let below = state(V3([1.0; 3]), vec![Lamp { display: V3([-0.1; 3]), ..lamp(V3::new(5.0, 10.0, 0.0)) }]);
        let out = run(&dark, &below, &[V3::ZERO], &config()).unwrap_err();
        assert!(matches!(out, Failed::Unavailable(Reason::OutOfRange { .. })) && out.to_string().contains("half float"), "{out}");
    }

    /// Two small rooms far apart span a large box: it is refused before it is made or walked, and where it may
    /// be its points that nobody asked for are counted without a look at them.
    #[test]
    fn far_apart() {
        let room = |x: f32, y: f32| sides(V3::new(x, y, 100.0), V3([150.0; 3]), plain(), |_, _| true);
        let of = |x: f32, y: f32, config: SolveConfig| {
            let world = prepared(scene([room(100.0, 100.0), room(x, y)].concat(), Vec::new()), 2);
            run(&world, &light(), &[V3([100.0; 3]), V3::new(x, y, 100.0)], &config)
        };
        let game = SolveConfig { census: false, ..config() };
        let bytes = (942 * 942 * 24 * 8) as u64;
        assert_eq!(of(60_000.0, 60_000.0, game).unwrap_err(), Failed::Unavailable(Reason::TooManyBytes { bytes, limit: 64 << 20 }));
        assert!(matches!(of(135_000.0, 100.0, game).unwrap_err(), Failed::Unavailable(Reason::TooLarge { .. })));
        let wide = of(60_000.0, 100.0, game).unwrap();
        let n = wide.counts;
        assert!(wide.dimensions[0] > 900 && n.reached < 200 && n.unknown > 14_000 && n.unknown + n.reached == n.points, "{n:?}");
        Reader::new(&wide).unwrap();
        let few = GatherConfig { limits: GatherLimits { points: 1000, seeds: 2 }, ..gather() };
        assert_eq!(of(60_000.0, 100.0, SolveConfig { gather: few, census: true, ..game }).unwrap_err(), Failed::Unavailable(Reason::TooManyPoints { limit: 1000 }));
    }

    /// The light of `reach()`'s points packed again is `solve()`'s texture, byte for byte, in the same box; what may not
    /// be is refused.
    #[test]
    fn points_packed_again() {
        let (input, light) = furnished();
        let world = prepared(input, 2);
        let seeds = [left(), right(), V3::new(-300.0, 60.0, 60.0)];
        let config = SolveConfig { gather: GatherConfig { rays: 64, emission: 0.25, ..gather() }, offset: 20.0, ..SolveConfig::default() };
        let v = run(&world, &light, &seeds, &config).unwrap();
        let reached = reach(&world, &light, &seeds, &config.gather, NEVER).unwrap();
        let moments: BTreeMap<Place, Moments> = reached.points.iter().map(|(at, p)| (*at, p.moments)).collect();
        let again = SolvedVolume::of_points(&moments, reached.coverage, &config).unwrap();
        let shape = |v: &SolvedVolume| (v.low, v.origin, v.spacing, v.offset, v.emission, v.dimensions, v.atlas, v.coverage);
        assert!(shape(&again) == shape(&v) && again.rgba16f_le == v.rgba16f_le);
        let n = v.counts;
        assert_eq!((again.counts, again.stats), (Counts { points: n.points, reached: n.reached, unknown: n.points - n.reached, ..Counts::default() }, Stats::default()));
        assert!(Reader::new(&again).is_ok() && again.points().unwrap() == v.points().unwrap());
        let refused = |points: &BTreeMap<Place, Moments>, config: SolveConfig| SolvedVolume::of_points(points, reached.coverage, &config).unwrap_err();
        assert_eq!(refused(&BTreeMap::new(), config), Failed::Unavailable(Reason::NothingReached { seeds: 3 }));
        let wrong = [GatherConfig { spacing: 0.0, ..config.gather }, GatherConfig { spacing: f32::NAN, ..config.gather }, GatherConfig { emission: 1.5, ..config.gather }];
        for gather in wrong {
            assert_eq!(refused(&moments, SolveConfig { gather, ..config }), Failed::Unavailable(Reason::Input("volume")));
        }
        assert_eq!(refused(&moments, SolveConfig { offset: f32::INFINITY, ..config }), Failed::Unavailable(Reason::Input("volume")));
        assert!(matches!(refused(&moments, SolveConfig { atlas_bytes: 4096, ..config }), Failed::Unavailable(Reason::TooManyBytes { limit: 4096, .. })));
        let mut bright = moments.clone();
        bright.insert([0, 1, 1], Moments { c: [V3([1e6; 3]); 4] });
        assert_eq!(refused(&bright, config), Failed::Unavailable(Reason::OutOfRange { at: [0, 1, 1] }));
        // A luminance below 0, which the reader refuses; a box wider than a u32.
        let dark = BTreeMap::from([([0, 0, 0], Moments { c: [V3([-1.0; 3]), V3::ZERO, V3::ZERO, V3::ZERO] })]);
        assert!(Reader::new(&volume(&([0; 3], [1; 3], dark.clone()))).is_err());
        assert_eq!(refused(&dark, config), Failed::Unavailable(Reason::Input("moments")));
        let one = Moments { c: [V3([0.5; 3]), V3::ZERO, V3::ZERO, V3::ZERO] };
        let wide = BTreeMap::from([([i32::MIN, 0, 0], one), ([i32::MAX, 0, 0], one)]);
        assert_eq!(refused(&wide, config), Failed::Unavailable(Reason::TooLarge { dimensions: [u32::MAX, 1, 1] }));
    }

    /// Before any work, between the pieces of it, and not after its end; a panic of the caller's check is a
    /// failure, not the caller's end.
    #[test]
    fn cancelling() {
        let world = prepared(scene(walled(), Vec::new()), 1);
        let seeds = Seeds::at(vec![left()]);
        let asked = AtomicUsize::new(0);
        let until = |n: usize| {
            asked.store(0, Ordering::Relaxed);
            let cancel = || asked.fetch_add(1, Ordering::Relaxed) >= n;
            solve(&world, &light(), &seeds, &config(), &cancel)
        };
        let whole = until(usize::MAX).unwrap();
        let times = asked.load(Ordering::Relaxed);
        assert!(times > 4, "{times}");
        // The last look is the packing's.
        for n in [0, 1, times / 2, times - 2, times - 1] {
            assert_eq!(until(n), Err(Failed::Cancelled));
        }
        assert!(until(times).unwrap() == whole);
        let check = || -> bool { panic!("expected here") };
        assert_eq!(solve(&world, &light(), &seeds, &config(), &check), Err(Failed::Internal));
    }
}
