//! A cell's constants: how much of its flat ambient goes, and how strong the bounce light in its place is, so
//! that the picture stays as bright as it was on average.
//!
//! ```text
//! v = the light around;  k = least + (1 - least) min(pay v, 1)
//! was = max(S, 0)^2.2;  light = max(S - (1 - b) k A, 0)^2.2 + g E
//! light = light + pull (Y(light) was / Y(was) - light)   where Y(was) > 0, Y the luminance
//! out = min(albedo light, 1)
//! ```
//!
//! per channel and linear, with the display sum S, the ambient A in it, and E and v as a [`Reader`] has them. The
//! fit needs no camera: it is over what some rays meet from places above the floor, spread over its area, in all
//! directions. Every place with a surface weighs the same. What is kept is the mean luminance of the display
//! values. First the fit that is the same everywhere (k = 1), whose g tells what light the ambient that is taken is
//! worth (`pay`), then the one with k. A g is never more than the cap: less of the ambient is taken then.
//!
//! It is a formula on surfaces that are met, not a picture: no ambient occlusion and no shadow is in it. Of no support
//! the light around is 0, so k is `least` there. `pay` is by the ambient alone, without what stands beside it in S.

use crate::bvh::Ray;
use crate::failed::{go_on, unavailable, Cancel, Failed, Reason};
use crate::gather::{own, spiral};
use crate::grid::{point, Place};
use crate::light::{lit, LightState, Lights, POWER};
use crate::parallel::cancellable;
use crate::reach::{EMISSIONS, MOST_RAYS};
use crate::solve::{Seeds, SolveConfig, SolvedVolume};
use crate::texture::Lookup;
use crate::trace::{PreparedScene, MOST_THREADS};
use crate::vec3::{linear_to_srgb, luminance, V3};
use crate::volume::{Reader, Refused, THIN};
use std::cell::Cell;

/// The most places, and rays of all of them.
const MOST_GROUPS: usize = 4096;
const MOST_FIT_RAYS: usize = 1 << 17;
/// The most a bounce may be made stronger.
const MOST_CAP: f32 = 1000.0;
/// The largest display value of the ambient and of a surface's sum. With it no light overflows: raised to 2.2 it is
/// below 2e13, a read of half floats times the cap below 1e9, and a product of two such is in f32's range. Light times
/// an albedo may overflow, to an infinity and to no NaN, the albedo being finite: it shows as 1 or, below 0, as 0. So
/// every mean is a number, b and g are by their bounds, and pay is by what f32 tells of 1 - b.
const LARGEST: f32 = 1e6;
/// An ambient of no channel above this display value (half a step of 255) has nothing to be taken.
const NO_AMBIENT: f32 = 0.5 / 255.0;

/// What is asked of a fit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitConfig {
    /// The share of the ambient to be replaced.
    pub share: f32,
    /// The bounce is never made stronger than this, 0 to 1,000.
    pub cap: f32,
    /// Of the unlit surfaces' own light: what the volume was solved with, or it is refused.
    pub emission: f32,
    /// The share of what is taken of the ambient that still is where nothing is bounced.
    pub least: f32,
    /// How far the colour goes back to what it was.
    pub pull: f32,
    /// Places, 1 to 4,096, and rays of each, 1 to [`MOST_RAYS`]; 131,072 rays in all.
    pub groups: usize,
    pub rays: usize,
    /// Of a read: what the volume is made for, or it is refused.
    pub offset: f32,
    /// 1 to [`MOST_THREADS`].
    pub threads: usize,
}

impl Default for FitConfig {
    fn default() -> FitConfig {
        FitConfig { share: 0.75, cap: 10.0, emission: 0.25, least: 0.25, pull: 0.7, groups: 64, rays: 16, offset: 24.0, threads: 1 }
    }
}

impl FitConfig {
    /// Against a volume's emission and offset.
    fn check(&self, emission: f32, offset: f32) -> Result<(), Failed> {
        let share = |s: f32| (0.0..=1.0).contains(&s);
        let fine = [
            ("share", share(self.share)),
            ("cap", (0.0..=MOST_CAP).contains(&self.cap)),
            ("least", share(self.least)),
            ("pull", share(self.pull)),
            ("groups", (1..=MOST_GROUPS).contains(&self.groups)),
            ("rays", (1..=MOST_RAYS).contains(&self.rays) && self.groups.saturating_mul(self.rays) <= MOST_FIT_RAYS),
            ("threads", (1..=MOST_THREADS).contains(&self.threads)),
            ("emission", EMISSIONS.contains(&self.emission) && self.emission == emission),
            ("offset", self.offset.is_finite() && self.offset == offset),
        ];
        match fine.iter().find(|f| !f.1) {
            Some((what, _)) => unavailable(Reason::Input(what)),
            None => Ok(()),
        }
    }
}

/// A cell's constants. A lit surface shows, per channel and linear,
///
/// ```text
/// k     = least + (1 - least) min(pay v, 1)
/// was   = max(S, 0)^2.2
/// light = max(S - (1 - b) k A, 0)^2.2 + g E
/// light = light + pull (Y(light) was / Y(was) - light)          where Y(was) > 0
/// out   = min(albedo light, 1)
/// ```
///
/// where S is the display sum on it, A the ambient in that sum, E and v are [`crate::Read::e`] and
/// [`crate::Read::v`] of the volume there, and Y is the luminance (0.2126, 0.7152, 0.0722): the light keeps its own
/// and its colour goes back to what it was. The constants are fitted with this and are good for nothing else: another
/// k needs another fit, and is another behaviour for what shows them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fitted {
    /// The share of the ambient that stays where k is 1; 1 - b is what is taken there.
    pub b: f32,
    /// How strong the bounce light is: the factor on E.
    pub g: f32,
    /// What turns the light around into the part of the taken ambient that it pays for: where pay v is 1 or more
    /// all of the share is taken, where nothing is around the share `least` of it. 0 where the fit that is the same
    /// everywhere takes nothing.
    pub pay: f32,
    /// Whether g is the cap, and less of the ambient is taken than was asked for.
    pub capped: bool,
    /// As they were asked for ([`FitConfig`]).
    pub least: f32,
    pub pull: f32,
    /// What the fit is over.
    pub met: Met,
}

/// Why a cell keeps its stock light.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Stock {
    /// No place to look from: a floor without a triangle, or no point of the grid.
    NoReach,
    /// No ray met a lit surface.
    NoReceivers(Met),
    /// Nothing is bounced to any surface met.
    NoBounce(Met),
    /// The ambient is as good as none: no channel above half a step of 255.
    NegligibleAmbient,
    /// The display sum on a surface is beyond 1,000,000, or its colour is no number: no constants are made of it.
    NotFinite(Met),
}

/// What a fit comes to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FitOutcome {
    Fitted(Fitted),
    /// No constants: nothing is done to the cell's light.
    Stock(Stock),
}

/// Where the rays of a fit start.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Population {
    /// Above the floor.
    #[default]
    Floor,
    /// The grid's own points, of seeds without a floor.
    Reached,
}

/// What the rays of a fit met: how far the constants can be trusted. A group is the rays of one place.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Met {
    /// Where the places are.
    pub population: Population,
    /// The places, those of them whose rays met a surface, and those whose rays met a lit one.
    pub groups: usize,
    pub used: usize,
    pub with_lit: usize,
    /// Rays that met a lit surface, an unlit one, nothing, and a side that is not drawn.
    pub lit: usize,
    pub unlit: usize,
    pub missed: usize,
    pub backs: usize,
    /// Of the lit ones, those where the volume has as good as nothing (a support of at most 1e-4, where a read can
    /// still have some light), and, these among them, those where it has less than is taken as all there is ([`THIN`]).
    pub unsupported: usize,
    pub thin: usize,
}

/// A volume and what is to be done with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub volume: SolvedVolume,
    pub fit: FitOutcome,
}

/// A lit surface that a ray met: its albedo, the display sum, and the bounce light and the light around as they
/// are read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Patch {
    a: [f32; 3],
    s: V3,
    e: V3,
    faded: f32,
    support: f32,
}

impl Patch {
    /// Whether the light on it does not overflow and its colour is a number.
    fn sound(&self) -> bool {
        self.s.finite() && self.s.max_abs() <= LARGEST && V3(self.a).finite()
    }
}

/// The rays of one place.
#[derive(Clone, Debug, Default)]
struct Group {
    lit: Vec<Patch>,
    /// The sum of [`bright`] of the unlit ones, and how many surfaces there are.
    unlit: f64,
    count: usize,
    missed: usize,
    backs: usize,
}

#[derive(Clone, Copy, Debug)]
struct Version {
    b: f32,
    g: f32,
    pay: f32,
    least: f32,
    pull: f32,
}

impl Version {
    /// The share of the ambient taken at a patch.
    fn taken(&self, p: &Patch) -> f32 {
        let v = p.faded / p.support.max(THIN);
        let k = self.least + (1.0 - self.least) * (self.pay * v).min(1.0);
        (1.0 - self.b) * k
    }

    /// What the patch shows, linear.
    fn shown(&self, p: &Patch, ambient: V3) -> [f32; 3] {
        let s = p.s - ambient * self.taken(p);
        let mut light: [f32; 3] = std::array::from_fn(|k| s[k].max(0.0).powf(POWER) + self.g * p.e[k]);
        let was = p.s.0.map(|c| c.max(0.0).powf(POWER));
        let (y, before) = (luminance(light), luminance(was));
        if before > 0.0 {
            light = std::array::from_fn(|k| light[k] + self.pull * (y * was[k] / before - light[k]));
        }
        std::array::from_fn(|k| (p.a[k] * light[k]).min(1.0))
    }
}

/// The luminance of a linear colour's display values, 0 to 255.
fn bright(c: [f32; 3]) -> f64 {
    f64::from(luminance(c.map(|c| 255.0 * linear_to_srgb(c.clamp(0.0, 1.0)))))
}

/// The mean over the groups with a surface, each with the same weight, of what its surfaces show.
fn mean(groups: &[Group], shown: impl Fn(&Patch) -> [f32; 3]) -> f64 {
    let (mut out, mut some) = (0.0, 0);
    for g in groups.iter().filter(|g| g.count > 0) {
        let sum = g.lit.iter().fold(g.unlit, |s, p| s + bright(shown(p)));
        out += sum / g.count as f64;
        some += 1;
    }
    out / f64::from(some.max(1))
}

/// Halve `low`..`high` 24 times, to 2^-24 of it, keeping `low` where `below`.
fn bisect(mut low: f32, mut high: f32, below: impl Fn(f32) -> bool) -> (f32, f32) {
    for _ in 0..24 {
        let m = 0.5 * (low + high);
        *if below(m) { &mut low } else { &mut high } = m;
    }
    (low, high)
}

/// [`bisect`] from 0 to the cap, and on, at most 160 times (to the least number there is), until what is left is 2^-12 of what is found. A gain that
/// is not below 2^-12 of the cap has that from the 24 alone, and nothing is done to it: a strong field's small one is
/// as exact as a weak field's. Without a root above 0 it ends at 0.
fn gain(cap: f32, below: impl Fn(f32) -> bool) -> f32 {
    let (mut low, mut high) = bisect(0.0, cap, &below);
    for _ in 0..160 {
        let m = 0.5 * (low + high);
        if high - low <= low / 4096.0 || m <= low || m >= high {
            break;
        }
        *if below(m) { &mut low } else { &mut high } = m;
    }
    low
}

/// What g makes of a bounce, to the light of the ambient that is taken.
fn pay(ambient: V3, b: f32, g: f32) -> f32 {
    let light = |share: f32| luminance(ambient.0.map(|c| (share * c).max(0.0).powf(POWER)));
    let taken = light(1.0) - light(b);
    if taken > 0.0 {
        g / taken
    } else {
        0.0
    }
}

/// The version that keeps the mean, and whether it is capped.
fn kept(groups: &[Group], ambient: V3, c: &FitConfig, least: f32, pay: f32, cancel: Cancel) -> Result<(Version, bool), Failed> {
    let first = Version { b: 1.0 - c.share, g: 0.0, pay, least, pull: c.pull };
    let plain = mean(groups, |p| lit(p.a, p.s));
    let stopped = Cell::new(false);
    let with = |b: f32, g: f32| {
        if stopped.get() || cancel() {
            stopped.set(true);
            return 0.0;
        }
        let v = Version { b, g, ..first };
        mean(groups, |p| v.shown(p, ambient))
    };
    let lowest = first.b;
    let out = if with(lowest, c.cap) < plain {
        (Version { b: bisect(lowest, 1.0, |b| with(b, c.cap) <= plain).0, g: c.cap, ..first }, true)
    } else {
        (Version { g: gain(c.cap, |g| with(lowest, g) <= plain), ..first }, false)
    };
    if stopped.get() {
        return Err(Failed::Cancelled);
    }
    Ok(out)
}

/// `some` of the places, spread: in the order of their coordinates' interleaved bits, from the lowest, the
/// middle one of each of as many even runs.
fn spread(places: &[Place], some: usize) -> Vec<Place> {
    let low: Place = std::array::from_fn(|k| places.iter().map(|p| p[k]).min().unwrap_or(0));
    let code = |p: &Place| (0..63).fold(0u64, |c, i| c | ((((p[i % 3] - low[i % 3]) as u64 >> (i / 3)) & 1) << i));
    let mut sorted = places.to_vec();
    sorted.sort_unstable_by_key(|p| (code(p), *p));
    let n = some.min(sorted.len());
    (0..n).map(|i| sorted[((2 * i + 1) as u64 * sorted.len() as u64 / (2 * n) as u64) as usize]).collect()
}

/// `group`'s bits turned round, as a number in 0..`of`: the first groups' are spread over it.
fn turned(group: usize, of: usize) -> usize {
    ((u64::from((group as u32).reverse_bits()) * of as u64) >> 32) as usize
}

/// Ray `i` of `rays` around a place: the spiral of that many, turned about the vertical by the group.
fn around(group: usize, i: usize, rays: usize) -> V3 {
    const PARTS: usize = 1 << 20;
    let d = spiral(i, rays);
    let (s, c) = (std::f32::consts::TAU * turned(group, PARTS) as f32 / PARTS as f32).sin_cos();
    V3::new(d[0] * c - d[2] * s, d[1], d[0] * s + d[2] * c)
}

/// What is looked at.
struct Case<'a> {
    world: &'a PreparedScene,
    lights: &'a Lights<'a>,
    reader: &'a Reader<'a>,
    config: &'a FitConfig,
}

impl Case<'_> {
    /// What the rays from a place meet.
    fn group(&self, from: V3, number: usize) -> Group {
        let mut out = Group::default();
        for i in 0..self.config.rays {
            let Some(hit) = self.world.transport(&Ray::new(from, around(number, i, self.config.rays)), f32::INFINITY) else {
                out.missed += 1;
                continue;
            };
            if hit.hidden {
                out.backs += 1;
                continue;
            }
            out.count += 1;
            if hit.material().unlit {
                out.unlit += bright(own(&hit).0);
                continue;
            }
            let a = hit.albedo(Lookup::Color);
            let read = self.reader.read_at(hit.p + hit.facing() * self.config.offset, hit.shading());
            let s = self.lights.sum(hit.p, hit.shading());
            // Without a sign of its zeros.
            out.lit.push(Patch { a: [a[0], a[1], a[2]], s, e: read.e() + V3::ZERO, faded: read.faded, support: read.support });
        }
        out
    }
}

fn met(population: Population, groups: &[Group]) -> Met {
    let lit = || groups.iter().flat_map(|g| &g.lit);
    let count = |f: fn(&Group) -> usize| groups.iter().map(f).sum::<usize>();
    Met {
        population,
        groups: groups.len(),
        used: groups.iter().filter(|g| g.count > 0).count(),
        with_lit: groups.iter().filter(|g| !g.lit.is_empty()).count(),
        lit: lit().count(),
        unlit: count(|g| g.count - g.lit.len()),
        missed: count(|g| g.missed),
        backs: count(|g| g.backs),
        unsupported: lit().filter(|p| p.support <= 1e-4).count(),
        thin: lit().filter(|p| p.support < THIN).count(),
    }
}

/// What may not be, looked at before any work.
fn check(light: &LightState, config: &FitConfig, emission: f32, offset: f32) -> Result<(), Failed> {
    config.check(emission, offset)?;
    if !light.ambient.0.iter().all(|c| (0.0..=LARGEST).contains(c)) {
        return unavailable(Reason::Input("light"));
    }
    Ok(())
}

pub fn fitted(scene: &PreparedScene, light: &LightState, volume: &SolvedVolume, seeds: &Seeds, config: &FitConfig, cancel: Cancel) -> Result<FitOutcome, Failed> {
    check(light, config, volume.emission, volume.offset)?;
    go_on(cancel)?;
    let lights = Lights::new(light, cancel)?;
    let mut reached = Vec::new();
    let keep = |at| {
        if seeds.floor.is_none() {
            reached.push(at);
        }
    };
    let reader = match Reader::scanned(volume, cancel, keep) {
        Ok(reader) => reader,
        Err(Refused::Cancelled) => return Err(Failed::Cancelled),
        Err(Refused::Because(_)) => return unavailable(Reason::Input("volume")),
    };
    let stock = |why| Ok(FitOutcome::Stock(why));
    let ambient = light.ambient;
    if ambient.0.iter().all(|c| *c <= NO_AMBIENT) {
        return stock(Stock::NegligibleAmbient);
    }
    let (from, population) = match &seeds.floor {
        Some(floor) => (floor.anchors(config.groups), Population::Floor),
        None => (spread(&reached, config.groups).into_iter().map(|at| point(at, volume.spacing)).collect(), Population::Reached),
    };
    if from.is_empty() {
        return stock(Stock::NoReach);
    }
    let case = Case { world: scene, lights: &lights, reader: &reader, config };
    let groups = cancellable(from.len(), config.threads, cancel, |i| case.group(from[i], i))?;
    let met = met(population, &groups);
    let lit = || groups.iter().flat_map(|g| &g.lit);
    if lit().next().is_none() {
        return stock(Stock::NoReceivers(met));
    }
    if !lit().all(Patch::sound) {
        return stock(Stock::NotFinite(met));
    }
    if lit().all(|p| p.e == V3::ZERO) {
        return stock(Stock::NoBounce(met));
    }
    let (even, _) = kept(&groups, ambient, config, 1.0, 0.0, cancel)?;
    let (v, capped) = kept(&groups, ambient, config, config.least, pay(ambient, even.b, even.g), cancel)?;
    Ok(FitOutcome::Fitted(Fitted { b: v.b, g: v.g, pay: v.pay, capped, least: v.least, pull: v.pull, met }))
}

pub fn solved(scene: &PreparedScene, light: &LightState, seeds: &Seeds, config: &SolveConfig, fit: &FitConfig, cancel: Cancel) -> Result<Solution, Failed> {
    check(light, fit, config.gather.emission, config.offset)?;
    let volume = SolvedVolume::of(scene, light, &seeds.points, config, cancel)?;
    let fit = fitted(scene, light, &volume, seeds, fit, cancel)?;
    Ok(Solution { volume, fit })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::floor::tests::{floor, made};
    use crate::floor::{Floor, EYE};
    use crate::gather::tests::{axis, built, lamp, plain, rectangle, room, sides, state, AMBIENT};
    use crate::light::tests::lights_of;
    use crate::light::{Directional, Lamp};
    use crate::reach::GatherConfig;
    use crate::scene::{Cull, Material};
    use crate::vec3::tests::Rng;
    use crate::volume::tests::{even, field, volume};
    use crate::volume::Read;
    use crate::{fit, solution};
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const NEVER: Cancel = &|| false;

    fn light() -> LightState {
        state(AMBIENT, vec![lamp(V3::new(30.0, 40.0, -20.0))])
    }

    fn seeds(floor: impl Into<Option<Floor>>) -> Seeds {
        Seeds { points: vec![V3::new(5.0, 6.0, 7.0)], floor: floor.into() }
    }

    fn config() -> FitConfig {
        FitConfig { groups: 10, threads: 2, ..FitConfig::default() }
    }

    fn of_room(material: Material, light: &LightState, floor: impl Into<Option<Floor>>, c: &FitConfig) -> Solution {
        solution(&room(material), light, &seeds(floor), &SolveConfig { gather: GatherConfig { threads: c.threads, ..GatherConfig::default() }, ..SolveConfig::default() }, c, NEVER).unwrap()
    }

    fn constants(fit: FitOutcome) -> Fitted {
        match fit {
            FitOutcome::Fitted(f) => f,
            other => panic!("{other:?}"),
        }
    }

    /// The rays of a fit, again.
    fn groups(world: &PreparedScene, light: &LightState, volume: &SolvedVolume, from: &[V3], c: &FitConfig) -> Vec<Group> {
        let (lights, reader) = (lights_of(light), Reader::new(volume).unwrap());
        let case = Case { world, lights: &lights, reader: &reader, config: c };
        from.iter().enumerate().map(|(i, p)| case.group(*p, i)).collect()
    }

    fn version(f: &Fitted) -> Version {
        Version { b: f.b, g: f.g, pay: f.pay, least: f.least, pull: f.pull }
    }

    /// The sRGB transfer, linear to display.
    fn display(c: f64) -> f64 {
        if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    }

    /// Where a rising `f` is 0.
    fn root(mut low: f64, mut high: f64, f: impl Fn(f64) -> f64) -> f64 {
        assert!(f(low) < 0.0 && f(high) > 0.0, "{} {}", f(low), f(high));
        for _ in 0..80 {
            let m = 0.5 * (low + high);
            *if f(m) < 0.0 { &mut low } else { &mut high } = m;
        }
        low
    }

    fn near(a: f32, b: f64) -> bool {
        (f64::from(a) / b - 1.0).abs() < 1e-4
    }

    /// The equation of [`Fitted`], word for word, of a read.
    fn documented(f: &Fitted, read: &Read, s: V3, a: V3, albedo: [f32; 3]) -> [f64; 3] {
        let of = |v: V3| v.0.map(f64::from);
        let lum = |c: [f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let [b, g, pay, least, pull] = [f.b, f.g, f.pay, f.least, f.pull].map(f64::from);
        let k = least + (1.0 - least) * (pay * f64::from(read.v())).min(1.0);
        let (s, a, e) = (of(s), of(a), of(read.e()));
        let was = s.map(|s| s.max(0.0).powf(2.2));
        let mut light: [f64; 3] = std::array::from_fn(|c| (s[c] - (1.0 - b) * k * a[c]).max(0.0).powf(2.2) + g * e[c]);
        if lum(was) > 0.0 {
            let y = lum(light);
            light = std::array::from_fn(|c| light[c] + pull * (y * was[c] / lum(was) - light[c]));
        }
        std::array::from_fn(|c| (f64::from(albedo[c]) * light[c]).min(1.0))
    }

    /// Spread over the box, each once, whatever their order; all when there are fewer.
    #[test]
    fn places_are_spread() {
        let places: Vec<Place> = (0..512).map(|i| [i % 8 - 5, i / 8 % 8 + 2, i / 64]).collect();
        let some = spread(&places, 64);
        let blocks: BTreeSet<_> = some.iter().map(|p| [(p[0] + 5) / 2, (p[1] - 2) / 2, p[2] / 2]).collect();
        assert!(some.len() == 64 && blocks.len() == 64, "one in each block of eight");
        let back: Vec<Place> = places.iter().rev().copied().collect();
        assert!(spread(&back, 64) == some && spread(&places[..7], 64).len() == 7 && spread(&[], 64).is_empty());
        assert_eq!(spread(&[[i32::MAX - 2046; 3], [i32::MAX; 3]], 2).len(), 2);
        assert!((0..16).all(|i| (around(3, i, 16).length() - 1.0).abs() < 1e-5) && around(0, 1, 16) != around(1, 1, 16));
    }

    /// By hand: a grey of 0.5 under a sum of 0.6, of which 0.2 is ambient. Half the support, whose v is all
    /// paid for; nothing around, and no support at all, where the least is taken.
    #[test]
    fn what_is_shown() {
        let v = Version { b: 0.25, g: 2.0, pay: 10.0, least: 0.25, pull: 0.7 };
        let p = Patch { a: [0.5; 3], s: V3([0.6; 3]), e: V3([0.05; 3]), faded: 0.1, support: 0.5 };
        assert!((v.taken(&p) - 0.75).abs() < 1e-6);
        let want = 0.5 * (0.45f32.powf(2.2) + 0.1);
        assert!(v.shown(&p, V3([0.2; 3])).iter().all(|c| (c - want).abs() < 1e-6));
        let dark = Patch { e: V3::ZERO, faded: 0.0, ..p };
        assert!((v.taken(&dark) - 0.1875).abs() < 1e-6 && (v.taken(&Patch { faded: 0.00625, support: 0.1, ..p }) - 0.75 * 0.4375).abs() < 1e-6);
        let none = v.shown(&Patch { support: 0.0, ..dark }, V3([0.2; 3]));
        assert!(none.iter().all(|c| (c - 0.5 * 0.5625f32.powf(2.2)).abs() < 1e-6), "{none:?}");
        // A coloured bounce on a grey sum: its luminance is added, and three tenths of its colour.
        let red = Patch { e: V3::new(0.5, 0.0, 0.0), ..p };
        let (plain, out) = (0.45f32.powf(2.2), v.shown(&red, V3([0.2; 3])));
        let y = plain + 2.0 * 0.2126 * 0.5;
        assert!((out[0] - 0.5 * (0.3 * (plain + 1.0) + 0.7 * y)).abs() < 1e-6 && (out[1] - 0.5 * (0.3 * plain + 0.7 * y)).abs() < 1e-6, "{out:?}");
        assert_eq!(v.shown(&Patch { a: [9.0; 3], ..p }, V3::ZERO), [1.0; 3]);
        // Where there was no light there is no colour to go back to.
        let black = Patch { s: V3([-0.1; 3]), ..red };
        assert_eq!(v.shown(&black, V3([0.2; 3])), [0.5, 0.0, 0.0]);
    }

    /// What is fitted is the equation that is documented, over reads, sums and colours at random.
    #[test]
    fn the_equation() {
        let mut rng = Rng(11);
        for _ in 0..2000 {
            let mut some = |least: f32, most: f32| V3(std::array::from_fn(|_| least + (most - least) * rng.draw()));
            let [(b, g), (pay, least)] = [some(0.0, 1.0), some(0.0, 1.0)].map(|v| (v[0], v[1]));
            let f = Fitted { b, g: 10.0 * g, pay: 40.0 * pay, capped: false, least, pull: some(0.0, 1.0)[0], met: Met::default() };
            let (raw, s, a, albedo) = (some(-0.05, 0.1), some(-0.1, 1.2), some(0.0, 0.5), some(0.0, 1.0).0);
            let read = Read { raw, faded: some(0.0, 0.1)[0], support: some(0.0, 1.0)[0] };
            let shown = version(&f).shown(&Patch { a: albedo, s, e: read.e(), faded: read.faded, support: read.support }, a);
            let want = documented(&f, &read, s, a, albedo);
            assert!((0..3).all(|k| (f64::from(shown[k]) - want[k]).abs() < 1e-5), "{shown:?}, not {want:?}: {f:?} {read:?}");
        }
    }

    #[test]
    fn what_is_paid() {
        let want = 2.0 / (0.2f32.powf(2.2) - 0.05f32.powf(2.2));
        assert!((pay(V3([0.2; 3]), 0.25, 2.0) / want - 1.0).abs() < 1e-5 && pay(V3::ZERO, 0.25, 2.0) == 0.0 && pay(V3([0.2; 3]), 1.0, 2.0) == 0.0);
        // Of a colour its luminance.
        let of = |c: f64| c.powf(2.2) - (0.5 * c).powf(2.2);
        assert!(near(pay(V3::new(0.1, 0.2, 0.4), 0.5, 3.0), 3.0 / (0.2126 * of(0.1) + 0.7152 * of(0.2) + 0.0722 * of(0.4))));
    }

    /// Below the middle of its range or above it, and its two ends.
    #[test]
    fn bisections() {
        assert!((bisect(0.0, 8.0, |x| x * x <= 2.0).0 - 2f32.sqrt()).abs() < 1e-6 && (bisect(0.25, 1.0, |x| x <= 0.9).0 - 0.9).abs() < 1e-6);
        assert!(bisect(0.0, 8.0, |_| false).0 == 0.0 && bisect(0.25, 1.0, |_| false).0 == 0.25);
        assert!((8.0 - 1e-6..8.0).contains(&bisect(0.0, 8.0, |_| true).0) && (1.0 - 1e-6..=1.0).contains(&bisect(0.25, 1.0, |_| true).0));
        assert_eq!(bisect(0.0, 0.0, |_| true).0, 0.0);
    }

    /// A gain of an ordinary size is the bisection's own, to the bit and for no more looks. A small one is as near to
    /// its own size, and one that is nowhere is 0.
    #[test]
    fn gains() {
        let looks = Cell::new(0);
        let of = |cap: f32, root: f32| {
            looks.set(0);
            gain(cap, |g| {
                looks.set(looks.get() + 1);
                g <= root
            })
        };
        for cap in [10.0, 3.7, 1000.0, 1e-3] {
            for share in [0.0003, 0.01, 0.14, 0.5, 0.999, 2.0] {
                let root = cap * share;
                assert!(of(cap, root).to_bits() == bisect(0.0, cap, |g| g <= root).0.to_bits() && looks.get() == 24, "{cap} {root}");
            }
            for small in [1e-4, 1e-7, 2.1e-9, 1e-20, 1e-34] {
                let root = cap * small;
                let g = of(cap, root);
                assert!(g <= root && g >= root * (1.0 - 1.0 / 2048.0) && (25..=184).contains(&looks.get()), "{cap} {root}: {g}");
            }
            assert!(of(cap, -1.0) == 0.0 && (130..=184).contains(&looks.get()), "{}", looks.get());
        }
        assert_eq!(of(0.0, 1.0), 0.0);
    }

    /// An even field as strong as a texture holds, which needs a gain of 2.1e-6: the white room stays at 102 of 255
    /// whatever the cap. With a gain of 0 the least share would go for nothing, down to 82.
    #[test]
    fn a_small_gain_by_hand() {
        let world = room(Material { base: [1.0; 4], ..plain() });
        let v = volume(&field([-2; 3], [5; 3], |_| Some(even([6e4; 3]))));
        let (ambient, read) = (V3([0.4; 3]), Reader::new(&v).unwrap().read_at(V3::ZERO, axis(2)));
        let shown = |f: &Fitted| 255.0 * display(documented(f, &read, ambient, ambient, [1.0; 3])[1]);
        let stock = 255.0 * display(0.4f64.powf(2.2));
        assert!((stock - 102.1).abs() < 0.1, "{stock}");
        let want = (0.4f64.powf(2.2) - 0.1f64.powf(2.2)) / 6e4;
        for cap in [MOST_CAP, 10.0, 1e-5] {
            let f = constants(fit(&world, &state(ambient, Vec::new()), &v, &seeds(floor()), &FitConfig { cap, ..config() }, NEVER).unwrap());
            assert!(!f.capped && f.b == 0.25 && (f64::from(f.g) / want - 1.0).abs() < 1e-3 && (f64::from(f.pay) * 6e4 - 1.0).abs() < 1e-3, "{f:?}, not {want}");
            assert!((shown(&f) - stock).abs() < 0.05, "{} {f:?}", shown(&f));
            assert!((shown(&Fitted { g: 0.0, pay: 0.0, ..f }) - 82.0).abs() < 0.1);
        }
    }

    /// A colour as no surface has needs a gain of 4.8e-31, or red is all there is of it: the room stays at 73.
    #[test]
    fn a_tiny_gain_by_hand() {
        let world = room(Material { base: [1e30, 1.0, 0.0, 1.0], ..plain() });
        let v = volume(&field([-2; 3], [5; 3], |_| Some(even([1.0, 0.0, 0.0]))));
        let c = FitConfig { share: 0.5, least: 1.0, pull: 0.0, cap: MOST_CAP, ..config() };
        let f = constants(fit(&world, &state(V3::new(0.0, 0.4, 0.0), Vec::new()), &v, &seeds(floor()), &c, NEVER).unwrap());
        // Green is what is left of it, and red has to make up for the rest.
        let light = |red: f64, green: f64| 255.0 * (0.2126 * display(red.min(1.0)) + 0.7152 * display(green));
        let (stock, left) = (light(0.0, 0.4f64.powf(2.2)), 0.2f64.powf(2.2));
        assert!((stock - 73.04).abs() < 0.01 && (light(0.0, left) - 33.97).abs() < 0.01, "{stock}");
        assert!(!f.capped && f.b == 0.5 && (f64::from(f.g) / 4.78e-31 - 1.0).abs() < 0.01 && (light(f64::from(f.g) * 1e30, left) - stock).abs() < 0.05, "{f:?}");
    }

    /// A grey room in an even field, by hand. Under an ambient A alone, with a field of c, all surfaces are alike, so
    /// (b A)^2.2 + g c = A^2.2, and what the taken ambient is worth is c of it: pay = 1 / c, and k is 1. With a
    /// cap that is too low it is the same equation for b.
    #[test]
    fn an_even_room_by_hand() {
        let world = room(Material { base: [0.5, 0.5, 0.5, 1.0], ..plain() });
        let c = 1.0 / 64.0;
        let v = volume(&field([-2; 3], [5; 3], |_| Some(even([c as f32; 3]))));
        let of = |ambient: f32, config: &FitConfig| constants(fit(&world, &state(V3([ambient; 3]), Vec::new()), &v, &seeds(floor()), config, NEVER).unwrap());
        let whole = 0.4f64.powf(2.2);
        let f = of(0.4, &config());
        assert!(f.b == 0.25 && !f.capped && near(f.g, (whole - 0.1f64.powf(2.2)) / c) && near(f.pay, 64.0), "{f:?}");
        // Half of the ambient is asked for: less bounce for it, worth as much.
        let f = of(0.4, &FitConfig { share: 0.5, ..config() });
        assert!(f.b == 0.5 && near(f.g, (whole - 0.2f64.powf(2.2)) / c) && near(f.pay, 64.0), "{f:?}");
        let f = of(0.4, &FitConfig { cap: 4.0, ..config() });
        assert!(f.capped && f.g == 4.0 && near(f.b, (whole - 4.0 * c).powf(1.0 / 2.2) / 0.4) && near(f.pay, 64.0), "{f:?}");
        assert_eq!((f.met, f.least, f.pull), (Met { population: Population::Floor, groups: 10, used: 10, with_lit: 10, lit: 160, ..Met::default() }, 0.25, 0.7));
        // Nothing may be added: all of the ambient stays, to the bisection's end.
        let f = of(0.4, &FitConfig { cap: 0.0, ..config() });
        assert!(f.capped && f.g == 0.0 && f.b > 1.0 - 1e-6 && f.pay == 0.0, "{f:?}");
        // Nothing may be taken.
        let f = of(0.4, &FitConfig { share: 0.0, ..config() });
        assert!(f.b == 1.0 && f.g < 1e-4 && f.pay == 0.0 && !f.capped, "{f:?}");
    }

    /// Places under a ceiling whose field is bright, over a floor whose field is an eighth of it, that the first place
    /// has and the second has not. So the ceiling's surfaces weigh (2/4 + 2/2) / 2 and the floor's (2/4 + 0) / 2, and k
    /// is another on each. Both fits by hand, and both capped.
    #[test]
    fn two_planes_by_hand() {
        let grey = Material { base: [0.5, 0.5, 0.5, 1.0], ..plain() };
        let ceiling = rectangle(V3::new(0.0, 100.0, 0.0), axis(0) * 4000.0, axis(4) * 900.0, grey);
        let world = built(vec![ceiling, rectangle(V3::new(-1100.0, -100.0, 0.0), axis(4) * 900.0, axis(0) * 900.0, grey)]);
        // Their eyes are half way between the planes.
        let t = |x: f32| [V3::new(x, -EYE, 0.0), V3::new(x + 3.0, -EYE, 0.0), V3::new(x, -EYE, 3.0)];
        let seeds = seeds(made(&[t(-1000.0), t(1000.0)], &[]));
        let v = volume(&field([-23, -2, -7], [47, 5, 15], |at| Some(even([if at[1] > 0 { 1.0 / 32.0 } else { 1.0 / 256.0 }; 3]))));
        let (a, fields) = (0.4f64, [1.0 / 32.0, 1.0 / 256.0]);
        let mean = |b: f64, g: f64, pay: f64, least: f64| {
            let of = |v: f64| {
                let k = least + (1.0 - least) * (pay * v).min(1.0);
                display(0.5 * ((a - (1.0 - b) * k * a).powf(2.2) + g * v))
            };
            0.75 * of(fields[0]) + 0.25 * of(fields[1])
        };
        let plain = display(0.5 * a.powf(2.2));
        let of = |cap: f32| constants(fit(&world, &state(V3([0.4; 3]), Vec::new()), &v, &seeds, &FitConfig { groups: 2, rays: 4, cap, ..config() }, NEVER).unwrap());
        let worth = |b: f64, g: f64| g / (a.powf(2.2) - (b * a).powf(2.2));
        let f = of(10.0);
        let pay = worth(0.25, root(0.0, 10.0, |g| mean(0.25, g, 0.0, 1.0) - plain));
        let g = root(0.0, 10.0, |g| mean(0.25, g, pay, 0.25) - plain);
        assert!(!f.capped && f.b == 0.25 && near(f.pay, pay) && near(f.g, g) && pay / 256.0 < 0.5, "{f:?}, not {g} {pay}");
        assert_eq!(f.met, Met { population: Population::Floor, groups: 2, used: 2, with_lit: 2, lit: 6, missed: 2, ..Met::default() });
        let f = of(2.0);
        let pay = worth(root(0.25, 1.0, |b| mean(b, 2.0, 0.0, 1.0) - plain), 2.0);
        let b = root(0.25, 1.0, |b| mean(b, 2.0, pay, 0.25) - plain);
        assert!(f.capped && f.g == 2.0 && near(f.pay, pay) && near(f.b, b), "{f:?}, not {b} {pay}");
        // Only the fit that is the same everywhere is capped: the whole share is taken in the end.
        let f = of(5.5);
        let (even, pay) = (root(0.25, 1.0, |b| mean(b, 5.5, 0.0, 1.0) - plain), f64::from(f.pay));
        let g = root(0.0, 10.0, |g| mean(0.25, g, pay, 0.25) - plain);
        assert!(!f.capped && f.b == 0.25 && near(f.pay, worth(even, 5.5)) && near(f.g, g) && even > 0.26 && g < 5.5, "{f:?}, not {g} {even}");
    }

    /// The picture stays as bright: over the same surfaces the mean is that without bounce, which a little more
    /// or less of it is not, by the documented equation of rays traced and read again. All of the share is taken,
    /// `pay` is the even fit's, and the mean rises with g and with b.
    #[test]
    fn a_fit_keeps_the_brightness() {
        let (c, light, world) = (config(), light(), room(plain()));
        let s = of_room(plain(), &light, floor(), &c);
        let f = constants(s.fit);
        assert!(f.b == 0.25 && f.g > 0.0 && f.g < 10.0 && f.pay > 0.0 && !f.capped && (f.least, f.pull) == (0.25, 0.7), "{f:?}");
        assert_eq!(f.met, Met { population: Population::Floor, groups: 10, used: 10, with_lit: 10, lit: 160, ..Met::default() });
        let places = floor().anchors(10);
        let (lights, reader) = (lights_of(&light), Reader::new(&s.volume).unwrap());
        let traced = |f: Option<Fitted>| {
            let mut sum = 0.0;
            for (i, p) in places.iter().enumerate() {
                for ray in 0..16 {
                    let hit = world.transport(&Ray::new(*p, around(i, ray, 16)), f32::INFINITY).unwrap();
                    let (a, s) = (hit.albedo(Lookup::Color), lights.sum(hit.p, hit.shading()));
                    let plain = std::array::from_fn(|k| (f64::from(a[k]) * f64::from(s[k]).powf(2.2)).min(1.0));
                    let out = f.map_or(plain, |f| documented(&f, &reader.read(hit.p, hit.facing(), hit.shading()), s, AMBIENT, [a[0], a[1], a[2]]));
                    sum += 0.2126 * display(out[0]) + 0.7152 * display(out[1]) + 0.0722 * display(out[2]);
                }
            }
            sum / 160.0
        };
        let off = traced(None);
        let with = |g: f32| traced(Some(Fitted { g, ..f }));
        assert!((with(f.g) / off - 1.0).abs() < 1e-5 && with(f.g * 1.01) > off && with(f.g * 0.99) < off);
        let g = groups(&world, &light, &s.volume, &places, &c);
        let even = kept(&g, AMBIENT, &c, 1.0, 0.0, NEVER).unwrap().0;
        assert!(f.pay == pay(AMBIENT, even.b, even.g) && even.g >= f.g);
        let shown = |b: f32, g_: f32| mean(&g, |p| Version { b, g: g_, ..version(&f) }.shown(p, AMBIENT));
        assert!((0..40).all(|i| shown(0.25, 0.25 * i as f32) < shown(0.25, 0.25 * (i + 1) as f32)));
        assert!((0..40).all(|i| shown(i as f32 / 40.0, 1.0) < shown((i + 1) as f32 / 40.0, 1.0)));
        // Capped: as strong as it may be, and less is taken.
        let low = FitConfig { cap: f.g * 0.25, ..c };
        let capped = constants(of_room(plain(), &light, floor(), &low).fit);
        assert!(capped.capped && capped.g == low.cap && capped.b > 0.25 && capped.b < 1.0, "{capped:?}");
        // The same on any number of threads.
        for threads in [1, 4, 12] {
            assert!(of_room(plain(), &light, floor(), &FitConfig { threads, ..c }) == s, "{threads}");
        }
    }

    /// On the back of a double-sided surface the sum and the bounce are by the normal that is turned round: a lamp on
    /// the side that is met lights it as it lights a front, one on the other side does not.
    #[test]
    fn double_sided_backs() {
        let two = built(sides(V3::ZERO, V3([-100.0; 3]), Material { cull: Cull::None, double_sided: true, ..plain() }, |_, _| true));
        let mut up = even([0.5; 3]);
        up[2] = V3([0.25; 3]);
        let v = volume(&field([-2; 3], [5; 3], |_| Some(up)));
        let of = |world: &PreparedScene, lamps: Vec<Lamp>| groups(world, &state(AMBIENT, lamps), &v, &[V3::ZERO], &config()).remove(0).lit;
        let inside = || vec![lamp(V3::new(5.0, 10.0, 0.0))];
        let (backs, fronts) = (of(&two, inside()), of(&room(plain()), inside()));
        assert!(backs.len() == 16 && backs.iter().zip(&fronts).all(|(b, f)| (b.s - f.s).max_abs() < 1e-5 && b.s[0] > AMBIENT[0] + 0.1), "{backs:?} {fronts:?}");
        // The floor faces up, from the side met. A lamp under it, with nothing in its way, is behind it.
        let on_floor = |p: &&Patch| (p.e[0] - 0.75).abs() < 1e-3;
        assert!(backs.iter().all(|p| p.e[0] > 0.24) && backs.iter().any(|p| on_floor(&p)));
        let under = of(&two, vec![lamp(V3::new(5.0, -160.0, 0.0))]);
        assert!(under.iter().filter(on_floor).all(|p| p.s == AMBIENT) && under.iter().any(|p| p.s != AMBIENT));
        let down = Ray::new(V3::ZERO, axis(3));
        let hit = two.transport(&down, f32::INFINITY).unwrap();
        assert!((hit.interpolated() - axis(3)).max_abs() < 1e-6 && (hit.shading() - axis(2)).max_abs() < 1e-6);
    }

    /// A volume of the lower part of a room: what it does not hold is counted, and loses the least share alone.
    #[test]
    fn without_support() {
        let v = volume(&field([-2; 3], [5, 2, 5], |_| Some(even([1.0 / 64.0; 3]))));
        let ambient = V3([0.4; 3]);
        let f = constants(fit(&room(plain()), &state(ambient, Vec::new()), &v, &seeds(floor()), &config(), NEVER).unwrap());
        assert!(f.met.lit == 160 && f.met.unsupported > 40 && f.met.thin > f.met.unsupported && f.met.thin < 140, "{f:?}");
        let g = groups(&room(plain()), &state(ambient, Vec::new()), &v, &floor().anchors(10), &config());
        for p in g.iter().flat_map(|g| &g.lit).filter(|p| p.support == 0.0) {
            let want = documented(&f, &Read::default(), ambient, ambient, p.a);
            let light = (0.4 * (1.0 - (1.0 - f64::from(f.b)) * 0.25)).powf(2.2);
            assert!((0..3).all(|k| (want[k] - f64::from(p.a[k]) * light).abs() < 1e-6 && (f64::from(version(&f).shown(p, ambient)[k]) - want[k]).abs() < 1e-5));
        }
    }

    /// Without a floor the places are points that were reached.
    #[test]
    fn places_without_a_floor() {
        let s = of_room(plain(), &light(), None, &config());
        let f = constants(s.fit);
        assert!((f.met.population, f.met.groups, f.met.lit) == (Population::Reached, 10, 160) && f.g > 0.0, "{f:?}");
        let mut places: Vec<Place> = Vec::new();
        Reader::scanned(&s.volume, NEVER, |at| places.push(at)).unwrap();
        assert!(places.len() == s.volume.counts.reached && places.len() == 27 && places.contains(&[-1, 0, 1]) && !places.contains(&[2, 0, 0]));
        let of = Seeds::of(floor());
        assert!(of.points == floor().eyes() && of.floor == Some(floor()) && Seeds::at(of.points).floor.is_none());
    }

    #[test]
    fn what_stays_as_it_is() {
        let why = |material: Material, light: &LightState| match of_room(material, light, floor(), &config()).fit {
            FitOutcome::Stock(why) => why,
            other => panic!("{other:?}"),
        };
        assert!(matches!(why(plain(), &state(AMBIENT, Vec::new())), Stock::NoBounce(m) if m.lit == 160));
        assert!(matches!(why(Material { unlit: true, ..plain() }, &light()), Stock::NoReceivers(m) if m.unlit == 160 && m.lit == 0 && m.used == 10));
        assert_eq!(why(plain(), &state(V3([0.5 / 255.0; 3]), light().lamps)), Stock::NegligibleAmbient);
        // A floor that is not in the room: its rays meet backs alone.
        let out = made(&[[V3::new(900.0, 0.0, 0.0), V3::new(900.0, 0.0, 9.0), V3::new(909.0, 0.0, 0.0)]], &[]);
        let away = of_room(plain(), &light(), out, &config()).fit;
        assert!(matches!(away, FitOutcome::Stock(Stock::NoReceivers(m)) if m.lit == 0 && m.backs + m.missed == 160), "{away:?}");
        // A floor that has nothing is no reason to look from elsewhere. As seeds it is none.
        assert_eq!(of_room(plain(), &light(), Floor::default(), &config()).fit, FitOutcome::Stock(Stock::NoReach));
        let none = solution(&room(plain()), &light(), &Seeds::of(Floor::default()), &SolveConfig::default(), &config(), NEVER);
        assert_eq!(none, Err(Failed::Unavailable(Reason::NoSeeds)));
        // A volume that has no point left.
        let mut none = of_room(plain(), &light(), floor(), &config()).volume;
        none.rgba16f_le.fill(0);
        none.counts = crate::solve::Counts { points: none.counts.points, unknown: none.counts.points, ..Default::default() };
        let of = |volume: &SolvedVolume, c: &FitConfig| fit(&room(plain()), &light(), volume, &seeds(None), c, NEVER);
        assert_eq!(of(&none, &config()), Ok(FitOutcome::Stock(Stock::NoReach)));
        none.rgba16f_le[1] = 0x3C;
        assert_eq!(of(&none, &config()), Err(Failed::Unavailable(Reason::Input("volume"))));
    }

    /// Numbers as large as they may be, and larger.
    #[test]
    fn large_numbers() {
        let full = [V3([6e4; 3]), V3([6e4; 3]), V3([-6e4; 3]), V3([6e4; 3])];
        let v = volume(&field([-2; 3], [5; 3], |_| Some(full)));
        let of = |base: f32, ambient: f32, sun: f32, c: &FitConfig| {
            let world = room(Material { base: [base, base, 0.5, 1.0], ..plain() });
            let suns = [axis(0), axis(1), axis(2), axis(3), axis(4), axis(5)].map(|to_source| Directional { display: V3([sun; 3]), to_source });
            fit(&world, &LightState { directionals: suns.to_vec(), ..state(V3([ambient; 3]), Vec::new()) }, &v, &seeds(floor()), c, NEVER)
        };
        let c = FitConfig { cap: MOST_CAP, ..config() };
        for base in [1e38, -1e38, 0.0, 1e-30, 1.0] {
            for ambient in [2e-3, 1.0, 1e6] {
                for (sun, share) in [(0.0, 1.0), (-1e6, 0.75), (1e6 - ambient, 6e-8), (0.5f32.min(1e6 - ambient), 1e-30)] {
                    let f = constants(of(base, ambient, sun, &FitConfig { share, ..c }).unwrap());
                    assert!((1.0 - share..=1.0).contains(&f.b) && (0.0..=MOST_CAP).contains(&f.g) && f.pay.is_finite() && f.pay >= 0.0, "{base} {ambient} {sun} {share}: {f:?}");
                }
            }
        }
        let stock = |fit: Result<FitOutcome, Failed>| assert!(matches!(fit, Ok(FitOutcome::Stock(Stock::NotFinite(m))) if m.lit == 160), "{fit:?}");
        stock(of(1.0, 0.5, 1.1e6, &c));
        stock(of(1.0, 0.5, f32::MAX, &c));
        stock(of(1.0, 0.5, f32::MIN, &c));
        // A colour that overflows.
        let mut parts = sides(V3::ZERO, V3([100.0; 3]), Material { base: [f32::MAX; 4], ..plain() }, |_, _| true);
        parts.iter_mut().for_each(|p| p.0.colors = vec![[f32::MAX, 1.0, 1.0, 1.0]; 4]);
        stock(fit(&built(parts), &light(), &v, &seeds(floor()), &c, NEVER));
        let input = Err(Failed::Unavailable(Reason::Input("light")));
        for ambient in [1.1e6, f32::MAX, f32::INFINITY, f32::NAN, -1e-3] {
            assert_eq!(of(1.0, ambient, 0.0, &c), input, "{ambient}");
        }
        assert_eq!(of(1.0, 0.5, f32::NAN, &c), input);
        // Places that are far off, and are none.
        for far in [1e9, 1.1e38, f32::INFINITY] {
            let corner = V3([far; 3]);
            let away = fit(&room(plain()), &light(), &v, &seeds(made(&[[corner, corner + axis(0) * (far * 1e-3), corner + axis(4) * (far * 1e-3)]], &[])), &c, NEVER);
            assert!(matches!(away, Ok(FitOutcome::Stock(Stock::NoReceivers(_) | Stock::NoReach))), "{away:?}");
        }
    }

    /// Before any work, and a solution's before its solve.
    #[test]
    fn what_is_refused() {
        let s = of_room(plain(), &light(), floor(), &config());
        let world = room(plain());
        let why = |c: FitConfig| match fit(&world, &light(), &s.volume, &seeds(floor()), &c, &|| true) {
            Err(Failed::Unavailable(Reason::Input(what))) => {
                assert_eq!(solution(&world, &light(), &seeds(floor()), &SolveConfig::default(), &c, &|| true), Err(Failed::Unavailable(Reason::Input(what))));
                what
            }
            other => panic!("{other:?}"),
        };
        let c = config();
        assert_eq!([FitConfig { share: 1.5, ..c }, FitConfig { cap: f32::NAN, ..c }, FitConfig { least: -0.1, ..c }, FitConfig { pull: f32::NAN, ..c }].map(why), ["share", "cap", "least", "pull"]);
        assert_eq!([FitConfig { groups: 0, ..c }, FitConfig { groups: 4096, rays: 4096, ..c }, FitConfig { rays: 0, ..c }, FitConfig { threads: 0, ..c }].map(why), ["groups", "rays", "rays", "threads"]);
        assert_eq!([FitConfig { groups: usize::MAX, ..c }, FitConfig { rays: usize::MAX, ..c }, FitConfig { threads: usize::MAX, ..c }, FitConfig { rays: 4097, groups: 1, ..c }].map(why), ["groups", "rays", "threads", "rays"]);
        assert_eq!([1000.1, -1.0, f32::MAX, f32::INFINITY].map(|cap| why(FitConfig { cap, ..c })), ["cap"; 4]);
        assert_eq!([0.5, -0.25, f32::NAN].map(|emission| why(FitConfig { emission, ..c })), ["emission"; 3]);
        assert_eq!([4.0, f32::NAN, f32::INFINITY].map(|offset| why(FitConfig { offset, ..c })), ["offset"; 3]);
        // A volume of numbers that no solve has, which the config has too.
        let of = |v: &SolvedVolume, c: &FitConfig| fit(&world, &light(), v, &seeds(floor()), c, NEVER);
        let input = |what| Err(Failed::Unavailable(Reason::Input(what)));
        assert_eq!(of(&SolvedVolume { emission: 1.5, ..s.volume.clone() }, &FitConfig { emission: 1.5, ..c }), input("emission"));
        assert_eq!(of(&SolvedVolume { offset: f32::INFINITY, ..s.volume.clone() }, &FitConfig { offset: f32::INFINITY, ..c }), input("offset"));
        for spacing in [0.0, 9.9e-4, 1.1e6, f32::NAN] {
            assert_eq!(of(&SolvedVolume { spacing, ..s.volume.clone() }, &c), input("volume"));
        }
        // A volume that is none is refused where the cell would stay as it is for another reason, after the ambient.
        let cut = SolvedVolume { rgba16f_le: vec![0; 8], ..s.volume.clone() };
        let stays = |ambient: f32, floor: Floor| fit(&world, &state(V3([ambient; 3]), Vec::new()), &cut, &seeds(floor), &c, NEVER);
        assert_eq!([stays(0.0, floor()), stays(0.5, floor()), stays(0.5, Floor::default()), stays(-1.0, floor())], [input("volume"), input("volume"), input("volume"), input("light")]);
        let far = SolvedVolume { offset: f32::MAX, ..s.volume.clone() };
        assert!(matches!(of(&far, &FitConfig { offset: f32::MAX, ..c }), Ok(FitOutcome::Stock(Stock::NoBounce(m))) if m.unsupported == 160));
        let wrong = state(AMBIENT, vec![Lamp { range: f32::NAN, ..lamp(V3::ZERO) }]);
        assert_eq!(fit(&world, &wrong, &s.volume, &seeds(floor()), &c, NEVER), input("light"));
        assert_eq!(solution(&world, &state(V3([-1.0; 3]), Vec::new()), &seeds(floor()), &SolveConfig::default(), &c, &|| true), Err(Failed::Unavailable(Reason::Input("light"))));
    }

    /// Before any work, between the pieces of it, in the bisections and not after their end; a panic of the caller's
    /// check is a failure wherever it comes.
    #[test]
    fn cancelling() {
        let s = of_room(plain(), &light(), floor(), &config());
        let world = room(plain());
        let asked = AtomicUsize::new(0);
        let until = |n: usize, panics: bool| {
            asked.store(0, Ordering::Relaxed);
            let cancel = || {
                let stop = asked.fetch_add(1, Ordering::Relaxed) >= n;
                assert!(!(stop && panics), "expected here");
                stop
            };
            fit(&world, &light(), &s.volume, &seeds(floor()), &config(), &cancel)
        };
        assert_eq!(until(usize::MAX, false), Ok(s.fit));
        let times = asked.load(Ordering::Relaxed);
        for n in [0, 1, times / 2, times - 61, times - 55, times - 50, times - 20, times - 1] {
            assert_eq!(until(n, false), Err(Failed::Cancelled), "{n} of {times}");
            assert_eq!(until(n, true), Err(Failed::Internal), "{n} of {times}");
        }
        assert_eq!(until(times, true), Ok(s.fit));
        let solve = |cancel: Cancel| solution(&world, &light(), &seeds(floor()), &SolveConfig::default(), &config(), cancel);
        assert_eq!(solve(&|| true), Err(Failed::Cancelled));
        assert_eq!(solve(&|| panic!("expected here")), Err(Failed::Internal));
    }
}
