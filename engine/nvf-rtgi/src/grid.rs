//! The points of the air that can be reached.
//!
//! The grid's points are the multiples of its spacing in the cell's own coordinates. A seed starts the valid
//! points beside it that it sees; from there every valid neighbour along an axis, inside the box about the
//! geometry, with nothing on the way.

use crate::failed::{go_on, unavailable, Cancel, Failed, Reason};
use crate::gather::{directions, probe, Probe};
use crate::light::Lights;
use crate::parallel::cancellable;
use crate::trace::PreparedScene;
use crate::vec3::V3;
use std::collections::{BTreeMap, BTreeSet};

/// A point of the grid, as multiples of the spacing.
pub type Place = [i32; 3];

/// How a flood went.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Coverage {
    pub seeds: usize,
    /// Those that see no valid point of the 64 around them.
    pub failed: usize,
    /// Those that see none of the eight around, but some of the 64.
    pub far: usize,
    /// Points.
    pub reached: usize,
    /// The parts of what was reached without an open way between them, and the largest's points.
    pub parts: usize,
    pub largest: usize,
    /// The grid's points inside the box about the geometry.
    pub in_bounds: usize,
}

/// The steps of small work between two looks at `cancel`.
pub const STEPS: usize = 4096;
/// Seeds whose candidates are held at a time.
const SEEDS: usize = 1024;
/// Seeds as far off as this (in spacings) are in no cell.
const FAR: f32 = 1e9;

pub fn point(at: Place, space: f32) -> V3 {
    V3(at.map(|c| c as f32 * space))
}

/// What the rays of each of some candidates met, in their order: its hits and, of them, the hidden sides
/// ([`Probe::hits`], [`Probe::backs`]). It is given their places and the points there.
pub type Classify<'c> = dyn FnMut(&[Place], &[V3]) -> Result<Vec<[u32; 2]>, Failed> + 'c;

/// The places `from`..=`to` beside the lower corner of `p`'s box, of a grid `space` apart.
fn block(p: V3, space: f32, from: i32, to: i32) -> Vec<Place> {
    if !(0..3).all(|k| (p[k] / space).abs() < FAR) {
        return Vec::new();
    }
    let low: Place = std::array::from_fn(|k| (p[k] / space).floor() as i32);
    let line = move || from..=to;
    line().flat_map(|x| line().flat_map(move |y| line().map(move |z| [low[0] + x, low[1] + y, low[2] + z]))).collect()
}

fn neighbours(at: Place) -> [Place; 6] {
    std::array::from_fn(|i| {
        let mut to = at;
        to[i / 2] += if i % 2 == 0 { -1 } else { 1 };
        to
    })
}

/// The sizes of the parts of `places` that `links` hold together, the largest first.
fn parts(places: &BTreeSet<Place>, links: &BTreeSet<[Place; 2]>, cancel: Cancel) -> Result<Vec<usize>, Failed> {
    let mut beside: BTreeMap<Place, Vec<Place>> = BTreeMap::new();
    for (i, [a, b]) in links.iter().enumerate() {
        if i.is_multiple_of(STEPS) {
            go_on(cancel)?;
        }
        beside.entry(*a).or_default().push(*b);
        beside.entry(*b).or_default().push(*a);
    }
    let (mut met, mut out) = (BTreeSet::new(), Vec::new());
    for start in places {
        let (mut stack, mut size) = (vec![*start], 0);
        while let Some(at) = stack.pop() {
            if met.insert(at) {
                if met.len().is_multiple_of(STEPS) {
                    go_on(cancel)?;
                }
                size += 1;
                stack.extend(beside.get(&at).into_iter().flatten());
            }
        }
        if size > 0 {
            out.push(size);
        }
    }
    out.sort_unstable_by(|a, b| b.cmp(a));
    Ok(out)
}

/// About what a tree of `n` entries of `each` bytes holds: its nodes are two thirds full.
pub fn tree(n: usize, each: usize) -> usize {
    n.saturating_mul(each).saturating_mul(3) / 2
}

/// What the points of a grid are gathered with.
pub struct Work<'a> {
    pub world: &'a PreparedScene,
    pub lights: &'a Lights<'a>,
    pub emission: f32,
    pub rays: usize,
    pub space: f32,
    pub threads: usize,
    /// No more points than this.
    pub limit: usize,
    pub cancel: Cancel<'a>,
}

/// What is reached so far, the last of it, and which places an open way joins.
#[derive(Default)]
struct Flood {
    reached: BTreeSet<Place>,
    edge: Vec<Place>,
    links: BTreeSet<[Place; 2]>,
}

impl Flood {
    fn add(&mut self, at: Place) {
        if self.reached.insert(at) {
            self.edge.push(at);
        }
    }
}

/// The grid's points as they are asked for.
pub struct Grid<'a> {
    work: Work<'a>,
    directions: Vec<V3>,
    bounds: Option<(V3, V3)>,
    made: BTreeMap<Place, Probe>,
    /// Of the last flood.
    links: usize,
}

impl<'a> Grid<'a> {
    pub fn new(work: Work<'a>) -> Grid<'a> {
        Grid { directions: directions(work.rays), bounds: work.world.bounds(), made: BTreeMap::new(), links: 0, work }
    }

    fn point(&self, at: Place) -> V3 {
        point(at, self.work.space)
    }

    /// Gather those that are not there yet, reached or not.
    pub fn make(&mut self, places: impl Iterator<Item = Place>) -> Result<(), Failed> {
        self.make_by(places, None)
    }

    /// [`Grid::make`]; with `by`, what it says of their rays in place of gathering them: such a point has its hits and
    /// backs alone. A count that cannot be, or a wrong number of them, is the crate's fault.
    fn make_by(&mut self, places: impl Iterator<Item = Place>, by: Option<&mut Classify>) -> Result<(), Failed> {
        let room = self.work.limit.saturating_sub(self.made.len());
        let mut asked = BTreeSet::new();
        for at in places {
            if !self.made.contains_key(&at) && asked.insert(at) && asked.len() > room {
                return unavailable(Reason::TooManyPoints { limit: self.work.limit });
            }
        }
        let asked: Vec<Place> = asked.into_iter().collect();
        let w = &self.work;
        let made = match by {
            None => cancellable(asked.len(), w.threads, w.cancel, |i| probe(w.world, w.lights, self.point(asked[i]), &self.directions, w.emission))?,
            Some(_) if asked.is_empty() => Vec::new(),
            Some(classify) => {
                go_on(w.cancel)?;
                let points: Vec<V3> = asked.iter().map(|at| self.point(*at)).collect();
                let met = classify(&asked, &points)?;
                let sound = |[hits, backs]: &[u32; 2]| backs <= hits && *hits as usize <= w.rays;
                if met.len() != asked.len() || !met.iter().all(sound) {
                    return Err(Failed::Internal);
                }
                met.into_iter().map(|[hits, backs]| Probe { hits, backs, ..Probe::default() }).collect()
            }
        };
        self.made.extend(asked.into_iter().zip(made));
        Ok(())
    }

    /// All there are, in the order of their places.
    pub fn made(&self) -> impl Iterator<Item = (&Place, &Probe)> {
        self.made.iter()
    }

    /// About what the points and the directions hold.
    pub fn bytes(&self) -> usize {
        tree(self.made.len(), size_of::<(Place, Probe)>()) + self.directions.capacity() * size_of::<V3>()
    }

    /// About what the last flood held beside them: each link once as it is and once from either end.
    pub fn flood_bytes(&self) -> usize {
        tree(self.links, size_of::<[Place; 2]>()) + self.links.saturating_mul(size_of::<[Place; 2]>()) + tree(self.made.len(), size_of::<(Place, Vec<Place>)>() + 2 * size_of::<Place>())
    }

    fn inside(&self, at: Place) -> bool {
        let p = self.point(at);
        self.bounds.is_some_and(|(min, max)| (0..3).all(|k| (min[k]..=max[k]).contains(&p[k])))
    }

    fn valid(&self, at: &Place) -> bool {
        self.made.get(at).is_some_and(|s| s.valid())
    }

    /// Of each start, the places among the `near` of it, inside the box and in its sight.
    fn in_sight(&self, from: &[V3], near: impl Fn(usize) -> Vec<Place> + Sync) -> Result<Vec<Vec<Place>>, Failed> {
        let seen = |i: usize| {
            let mut to = near(i);
            to.retain(|at| self.inside(*at) && self.work.world.open(from[i], self.point(*at)));
            to
        };
        cancellable(from.len(), self.work.threads, self.work.cancel, seen)
    }

    /// Of each seed, the valid points among the [`block`] of it that it sees, added to what is reached; those
    /// seeds that see none.
    fn start(&mut self, seeds: &[V3], (from, to): (i32, i32), out: &mut Flood, mut by: Option<&mut Classify>) -> Result<Vec<V3>, Failed> {
        let mut lost = Vec::new();
        for some in seeds.chunks(SEEDS) {
            let seen = self.in_sight(some, |i| block(some[i], self.work.space, from, to))?;
            self.make_by(seen.iter().flatten().copied(), by.as_deref_mut())?;
            for (seed, there) in some.iter().zip(&seen) {
                let mut there = there.iter().filter(|at| self.valid(at));
                let Some(first) = there.next() else {
                    lost.push(*seed);
                    continue;
                };
                out.add(*first);
                for at in there {
                    out.links.insert([*first, *at]);
                    out.add(*at);
                }
            }
        }
        Ok(lost)
    }

    /// What the seeds reach.
    #[cfg(test)]
    pub fn flood(&mut self, seeds: &[V3]) -> Result<(BTreeSet<Place>, Coverage), Failed> {
        self.flood_by(seeds, None)
    }

    /// What the seeds reach; with `by`, its points made by what that says ([`Grid::make_by`]).
    pub fn flood_by(&mut self, seeds: &[V3], mut by: Option<&mut Classify>) -> Result<(BTreeSet<Place>, Coverage), Failed> {
        let space = self.work.space;
        let mut out = Flood::default();
        let lost = self.start(seeds, (0, 1), &mut out, by.as_deref_mut())?;
        let failed = self.start(&lost, (-1, 2), &mut out, by.as_deref_mut())?.len();
        while !out.edge.is_empty() {
            let edge = std::mem::take(&mut out.edge);
            let from: Vec<V3> = edge.iter().map(|at| self.point(*at)).collect();
            let next = self.in_sight(&from, |i| neighbours(edge[i]).to_vec())?;
            self.make_by(next.iter().flatten().copied(), by.as_deref_mut())?;
            for (at, next) in edge.iter().zip(&next) {
                for to in next.iter().filter(|to| self.valid(to)) {
                    out.links.insert([*at, *to]);
                    out.add(*to);
                }
            }
        }
        let Flood { reached, links, .. } = out;
        self.links = links.len();
        let sizes = parts(&reached, &links, self.work.cancel)?;
        let along = |min: f32, max: f32| ((max / space).floor() - (min / space).ceil() + 1.0).max(0.0) as usize;
        let in_bounds = self.bounds.map_or(0, |(min, max)| (0..3).map(|k| along(min[k], max[k])).fold(1, usize::saturating_mul));
        let coverage = Coverage { seeds: seeds.len(), failed, far: lost.len() - failed, reached: reached.len(), parts: sizes.len(), largest: sizes.first().copied().unwrap_or(0), in_bounds };
        Ok((reached, coverage))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::gather::tests::{built, lamp, plain, rectangle, sides, state, AMBIENT};
    use crate::light::tests::lights_of;
    use crate::light::LightState;
    use crate::scene::{Cull, Material, SceneInput};
    use crate::trace::tests::prepared;

    pub const SPACE: f32 = 192.0;

    /// Two closed rooms side by side, 20 units between them.
    fn rooms() -> PreparedScene {
        let room = |x: f32| sides(V3::new(x, 100.0, 100.0), V3([300.0, 290.0, 290.0]), plain(), |_, _| true);
        built([room(-310.0), room(310.0)].concat())
    }

    fn lights() -> LightState {
        state(AMBIENT, vec![lamp(V3::new(-100.0, 100.0, 100.0))])
    }

    pub fn work<'a>(world: &'a PreparedScene, lights: &'a Lights<'a>, threads: usize) -> Work<'a> {
        Work { world, lights, emission: 0.0, rays: 512, space: SPACE, threads, limit: usize::MAX, cancel: &|| false }
    }

    fn flood(world: &PreparedScene, seeds: &[V3], threads: usize) -> (BTreeSet<Place>, Coverage, Vec<(Place, Probe)>) {
        let (state, w) = (lights(), world);
        let lights = lights_of(&state);
        let mut grid = Grid::new(work(w, &lights, threads));
        let (reached, coverage) = grid.flood(seeds).unwrap();
        (reached, coverage, grid.made.into_iter().collect())
    }

    /// The flood stays in the room it starts in, and inside the box about the geometry; one that starts
    /// in the void between the rooms reaches nothing.
    #[test]
    fn walls_stop_the_flood() {
        let world = rooms();
        let here = |x: f32| V3::new(x, 100.0, 100.0);
        let seeds = [here(-300.0), here(300.0), here(0.0), here(-250.0)];
        let (left, of_left, made) = flood(&world, &seeds[..1], 2);
        assert_eq!(of_left, Coverage { seeds: 1, failed: 0, far: 0, reached: 27, parts: 1, largest: 27, in_bounds: 63 });
        assert!(left.iter().all(|at| made.iter().any(|(p, s)| p == at && s.valid() && s.moments.c[0][0] > 0.0)));
        let (both, of_both, made) = flood(&world, &seeds, 2);
        assert_eq!(of_both, Coverage { seeds: 4, failed: 1, far: 0, reached: 54, parts: 2, largest: 27, in_bounds: 63 });
        assert!(both.is_superset(&left) && made.len() >= 54);
        assert_eq!(made.iter().filter(|(p, s)| both.contains(p) && s.moments.c[0] == V3::ZERO).count(), 27, "the room without a lamp");
        assert_eq!(flood(&world, &seeds[2..3], 1).1, Coverage { seeds: 1, failed: 1, in_bounds: 63, ..Coverage::default() });
        // On any number of threads, and in any order of the seeds.
        for threads in [1, 4, 12] {
            assert!(flood(&world, &seeds, threads) == (both.clone(), of_both, made.clone()));
        }
        let turned: Vec<V3> = seeds.iter().rev().copied().collect();
        assert!(flood(&world, &turned, 2) == (both, of_both, made));
    }

    /// A wall through the grid's own points is one: none of them is reached, nor what is behind.
    #[test]
    fn walls_on_the_grid() {
        let mut soup = sides(V3::new(0.0, 100.0, 100.0), V3::new(SPACE, 290.0, 290.0), plain(), |_, _| true);
        soup.extend(sides(V3::new(2000.0, 100.0, 100.0), V3([50.0; 3]), plain(), |_, _| true));
        let (reached, coverage, _) = flood(&built(soup), &[V3::new(10.0, 100.0, 100.0)], 1);
        assert!(reached.len() == 9 && reached.iter().all(|at| at[0] == 0) && coverage.parts == 1, "{reached:?}");
        // Without geometry there is no grid.
        let none = prepared(SceneInput::default(), 1);
        assert_eq!(flood(&none, &[V3::ZERO], 1).1, Coverage { seeds: 1, failed: 1, ..Coverage::default() });
        let wild = [f32::NAN, f32::INFINITY, 1e30, -1e30].map(|x| V3::new(0.0, x, 0.0));
        assert_eq!(flood(&rooms(), &wild, 1).1, Coverage { seeds: 4, failed: 4, in_bounds: 63, ..Coverage::default() });
    }

    /// A wall without thickness between the points parts them, where it is cut away it does not.
    #[test]
    fn thin_walls() {
        let of = |cutoff| {
            let wall = Material { cull: Cull::None, double_sided: true, cutoff, base: [0.5, 0.5, 0.5, 0.4], ..Material::default() };
            let mut soup = sides(V3::new(0.0, 100.0, 100.0), V3::new(400.0, 290.0, 290.0), plain(), |_, _| true);
            soup.push(rectangle(V3::new(-90.0, 100.0, 100.0), V3::new(0.0, 290.0, 0.0), V3::new(0.0, 0.0, 290.0), wall));
            flood(&built(soup), &[V3::new(50.0, 100.0, 100.0)], 2)
        };
        let (reached, coverage, _) = of(0.0);
        assert!(reached.iter().all(|at| at[0] >= 0) && coverage.reached == 27 && coverage.in_bounds == 45, "{coverage:?}");
        assert_eq!(of(0.5).1.reached, 45);
    }

    /// A wall that ends on the grid's own lines: a segment along its edge lies on the face of its box, and is
    /// stopped there.
    #[test]
    fn walls_that_end_on_the_grid() {
        let wall = Material { cull: Cull::None, double_sided: true, ..plain() };
        // From y = -190 to 384, its greatest place on the grid.
        let part = rectangle(V3::new(-90.0, 97.0, 100.0), V3::new(0.0, 287.0, 0.0), V3::new(0.0, 0.0, 290.0), wall);
        let alone = built(vec![part.clone()]);
        for y in [-190.0, 0.0, 192.0, 384.0] {
            let (a, b) = (V3::new(-192.0, y, 192.0), V3::new(0.0, y, 192.0));
            assert!(!alone.open(a, b) && !alone.open(b, a), "{y}");
        }
        assert!(alone.open(V3::new(-192.0, 384.5, 192.0), V3::new(0.0, 384.5, 192.0)));
        let mut soup = sides(V3::new(0.0, 100.0, 100.0), V3::new(400.0, 290.0, 290.0), plain(), |_, _| true);
        soup.push(part);
        let (reached, coverage, _) = flood(&built(soup), &[V3::new(50.0, 100.0, 100.0)], 2);
        assert!(reached.iter().all(|at| at[0] >= 0) && coverage.reached == 27 && coverage.in_bounds == 45, "{coverage:?}");
    }

    /// A seed in a tube sees none of the eight points around it, but one further off, at the open end.
    #[test]
    fn far_seeds() {
        let tube = Material { cull: Cull::None, double_sided: true, ..plain() };
        let mut soup = sides(V3::new(0.0, 100.0, 100.0), V3::new(400.0, 290.0, 290.0), plain(), |_, _| true);
        soup.extend(sides(V3::new(160.0, 21.0, 21.0), V3::new(140.0, 19.0, 19.0), tube, |k, upper| k != 0 || !upper));
        let coverage = flood(&built(soup), &[V3::new(60.0, 10.0, 10.0)], 2).1;
        assert_eq!(coverage, Coverage { seeds: 1, failed: 0, far: 1, reached: 45, parts: 1, largest: 45, in_bounds: 45 });
    }

    /// More seeds than are held at a time, the same one among them again and again: what one reaches. Past the
    /// limit it stops at the first point too many, before any is gathered.
    #[test]
    fn many_seeds() {
        let (world, state) = (rooms(), lights());
        let lights = lights_of(&state);
        let of = |n: usize| flood(&world, &vec![V3::new(-300.0, 100.0, 100.0); n], 2);
        let (one, many) = (of(1), of(2 * SEEDS + 3));
        assert!(one.0 == many.0 && one.2 == many.2 && many.1 == Coverage { seeds: 2 * SEEDS + 3, ..one.1 });
        let mut grid = Grid::new(Work { limit: 3, ..work(&world, &lights, 1) });
        let mut taken = 0;
        let places = (0..1000).map(|x| [x / 2, 0, 0]).inspect(|_| taken += 1);
        assert_eq!(grid.make(places), Err(Failed::Unavailable(Reason::TooManyPoints { limit: 3 })));
        assert!(taken == 7 && grid.made.is_empty(), "{taken}");
    }

    #[test]
    fn limits_and_cancelling() {
        let (world, state) = (rooms(), lights());
        let lights = lights_of(&state);
        let seeds = [V3::new(-300.0, 100.0, 100.0)];
        let run = |limit, cancel: Cancel| {
            let mut grid = Grid::new(Work { limit, cancel, ..work(&world, &lights, 2) });
            let out = grid.flood(&seeds).map(|f| f.0.len());
            (out, grid.made.len())
        };
        let (out, made) = run(usize::MAX, &|| false);
        assert_eq!(out, Ok(27));
        assert_eq!(run(made, &|| false).0, Ok(27));
        let (out, held) = run(made - 1, &|| false);
        assert!(out == Err(Failed::Unavailable(Reason::TooManyPoints { limit: made - 1 })) && held < made);
        let asked = std::sync::atomic::AtomicUsize::new(0);
        // The seed, its eight points, and some of their neighbours' segments.
        let later = || asked.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 12;
        let (out, held) = run(usize::MAX, &later);
        assert!(out == Err(Failed::Cancelled) && held > 0 && held < made, "{held} of {made}");
    }

    #[test]
    fn parts_by_their_links() {
        let place = |x: i32| [x, 0, 0];
        let all: BTreeSet<Place> = (0..6).map(place).collect();
        let links = BTreeSet::from([[place(0), place(1)], [place(4), place(1)], [place(2), place(3)]]);
        assert_eq!(parts(&all, &links, &|| false), Ok(vec![3, 2, 1]));
        assert_eq!(parts(&all, &links, &|| true), Err(Failed::Cancelled));
        assert_eq!(block(V3::new(-10.0, 0.0, 200.0), SPACE, 0, 1).len(), 8);
        assert!(block(V3::new(-10.0, 0.0, 200.0), SPACE, -1, 2).contains(&[-2, 2, 0]) && block(V3::ZERO, SPACE, 0, 1)[0] == [0; 3]);
        assert!(block(V3::new(f32::NAN, 0.0, 0.0), SPACE, 0, 1).is_empty());
    }
}
