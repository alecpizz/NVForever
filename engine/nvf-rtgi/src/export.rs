//! What another tracer of the same rays needs, such as the GPU solve of `nvf-rtgi-live`: the prepared scene plainly,
//! the triangles near a place, how the triangle test sees a direction, the counts that decide a point's validity, and
//! the points of a volume. All of it is read off what a solve uses; none of it changes what a solve computes.

use crate::bvh::{self, Corners, Ray};
use crate::failed::{guarded, unavailable, Cancel, Failed, Reason};
use crate::gather::{counted, probe};
use crate::grid::{point, Place};
use crate::light::{LightState, Lights};
use crate::parallel::cancellable;
use crate::reach::EMISSIONS;
use crate::scene::Material;
use crate::solve::SolvedVolume;
use crate::texture::{Sampler, Texels};
use crate::trace::{lift, Face, PreparedScene, MOST_THREADS};
use crate::vec3::V3;
use crate::volume::{Reader, Refused};

/// A prepared scene, with what was replaced in it already replaced.
#[derive(Clone, Debug)]
pub struct Export<'a> {
    /// Of each triangle by its number, counter-clockwise seen from its front; finite. Of two at one distance along a
    /// ray the lower number is met.
    pub corners: Vec<Corners>,
    pub faces: &'a [Face],
    pub materials: &'a [Material],
    /// `None` is white, as is a material's number past the end.
    pub textures: Vec<Option<Texels<'a>>>,
}

impl PreparedScene {
    pub fn export(&self) -> Export<'_> {
        let (bvh, faces, materials, textures) = self.parts();
        Export { corners: bvh.in_order(), faces, materials, textures: textures.iter().map(|t| t.as_ref().map(Sampler::texels)).collect() }
    }
}

/// A triangle, and the box of its leaf of the tree, which a ray has to enter for it to be tested. Where a ray starts in
/// a surface's plane the box decides as much as the test.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Near {
    pub tri: u32,
    pub min: V3,
    pub max: V3,
}

impl PreparedScene {
    /// The triangles no further from a place than `far`, in order. Conservative: it never omits one within the radius,
    /// and may name one beyond it by no more than its width across its longest side and a small allowance for
    /// rounding. None for a `far` below 0.
    pub fn near(&self, at: V3, far: f32) -> Vec<Near> {
        self.parts().0.within(at, far).into_iter().map(|(tri, min, max)| Near { tri, min, max }).collect()
    }
}

/// How the solver's rays see a direction. An axis is still where the direction's part along it is below 1e-20 of the
/// largest, or its reciprocal overflows; that part is 0 for everything a ray does then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sheared {
    /// The direction as the rays go: its still parts 0. The tree, the triangle test and which side is met use it;
    /// the moments of a point use the direction as given.
    pub along: [f32; 3],
    /// The axes the triangle test shears along, the last one the longest, and by how much.
    pub axes: [u32; 3],
    pub shear: [f32; 3],
    /// For boxes: one over the direction, 0 on a still axis.
    pub inverse: [f32; 3],
    pub still: [bool; 3],
}

impl Sheared {
    pub fn of(along: V3) -> Sheared {
        let r = Ray::new(V3::ZERO, along);
        Sheared { along: r.d.0, axes: r.axes.map(|a| a as u32), shear: r.shear.0, inverse: r.inverse.0, still: r.still }
    }
}

/// The solver's triangle test: the distance, which may be below 0, and the corners' weights, where the ray's line meets
/// the triangle. A ray meets the nearest distance above 0, the lower triangle's of equal ones.
pub fn meet(from: V3, along: V3, corners: &Corners) -> Option<(f32, [f32; 3])> {
    bvh::meet(&Ray::new(from, along), corners)
}

/// Of each of the points `at`, its rays along `directions`, the hits and the hidden sides of its [`crate::Probe`]:
/// a classifier for [`crate::reach_by`] on the CPU, which then gives the reach of [`crate::reach()`].
pub fn classified(scene: &PreparedScene, at: &[V3], directions: &[V3], threads: usize, cancel: Cancel) -> Result<Vec<[u32; 2]>, Failed> {
    guarded(|| {
        if !(1..=MOST_THREADS).contains(&threads) {
            return unavailable(Reason::Input("threads"));
        }
        cancellable(at.len(), threads, cancel, |i| counted(scene, at[i], directions))
    })
}

/// What one ray meets and brings, as a point's [`crate::Probe`] counts it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounce {
    pub tri: u32,
    /// Of the corners.
    pub weights: [f32; 3],
    pub p: V3,
    pub front: bool,
    /// A side that is not drawn: it brings nothing.
    pub hidden: bool,
    /// Where its shadow rays start.
    pub start: V3,
    pub light: V3,
    /// The lamps of its cell of the lamps' table, and those of them that had a shadow ray.
    pub lamps: u32,
    pub shadow_rays: u32,
}

/// What each ray (from, along) meets and brings, for another tracer's single rays: of a point's rays the lights are
/// what its [`crate::Probe`] sums, and the counts add up to its counts. `emission` as [`crate::GatherConfig`]'s, and
/// refused as there.
pub fn bounces(scene: &PreparedScene, light: &LightState, rays: &[(V3, V3)], emission: f32, threads: usize, cancel: Cancel) -> Result<Vec<Option<Bounce>>, Failed> {
    guarded(|| {
        if !(1..=MOST_THREADS).contains(&threads) {
            return unavailable(Reason::Input("threads"));
        }
        if !EMISSIONS.contains(&emission) {
            return unavailable(Reason::Input("emission"));
        }
        let lights = Lights::new(light, cancel)?;
        cancellable(rays.len(), threads, cancel, |i| {
            let (at, d) = rays[i];
            let h = scene.transport(&Ray::new(at, d), f32::INFINITY)?;
            let one = probe(scene, &lights, at, &[d], emission);
            let start = h.p + h.facing() * lift(h.p);
            Some(Bounce { tri: h.tri(), weights: h.weights(), p: h.p, front: h.front, hidden: h.hidden, start, light: one.moments.c[0], lamps: one.lamps, shadow_rays: one.shadow_rays })
        })
    })
}

/// A point of a volume that counts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumePoint {
    /// As multiples of the spacing, and in the world.
    pub place: Place,
    pub position: V3,
    /// Its texel in the first slab of the texture; those of the others are [`SolvedVolume::slab_step`] further
    /// along z each.
    pub texel: [u32; 3],
}

impl SolvedVolume {
    /// The points that count, by their texels: x fastest, then y, then z. One that a [`Reader`] refuses has none.
    pub fn points(&self) -> Result<Vec<VolumePoint>, Failed> {
        let mut places = Vec::new();
        match Reader::scanned(self, &|| false, |at| places.push(at)) {
            Ok(_) => {}
            Err(Refused::Cancelled) => return Err(Failed::Cancelled),
            Err(Refused::Because(_)) => return unavailable(Reason::Input("volume")),
        }
        let of = |place: Place| VolumePoint { place, position: point(place, self.spacing), texel: std::array::from_fn(|k| place[k].abs_diff(self.low[k]) + 1) };
        Ok(places.into_iter().map(of).collect())
    }

    /// From a slab of the texture to the next, along z.
    pub fn slab_step(&self) -> u32 {
        self.dimensions[2] + 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gather::directions;
    use crate::gather::tests::{plain, rectangle};
    use crate::reach::tests::{left, light, walled};
    use crate::reach::GatherConfig;
    use crate::scene::{Level, SceneInput, Texture, Wrap};
    use crate::solve::{Seeds, SolveConfig};
    use crate::trace::tests::{prepared, scene, SAME};
    use crate::vec3::blend;

    const NEVER: Cancel = &|| false;

    /// The corners are the triangles' by their numbers, whatever the tree's order.
    #[test]
    fn a_scene_plainly() {
        let parts = (0..40).map(|i| rectangle(V3([10.0 * i as f32; 3]), V3::new(0.0, 4.0, 0.0), V3::new(0.0, 0.0, 4.0), plain()).0);
        let w = prepared(scene(parts.map(|mesh| (mesh, SAME, plain())).collect(), Vec::new()), 2);
        let e = w.export();
        assert_eq!((e.corners.len(), e.faces.len(), e.materials.len()), (80, 80, 40));
        for i in 0..40 {
            let (at, along) = (10.0 * i as f32, V3::new(-1.0, 0.0, 0.0));
            let from = V3::new(at + 1.0, at - 1.0, at + 0.5);
            let h = w.transport(&Ray::new(from, along), 9.0).unwrap();
            let corners = &e.corners[h.tri() as usize];
            assert_eq!(e.faces[h.tri() as usize].material, i);
            assert_eq!(V3(blend(&corners.map(|c| c.0), h.weights())), h.p);
            assert_eq!(meet(from, along, corners), Some((1.0, h.weights())));
        }
        // Near a corner of the third square, one of its sides, and nothing.
        let near = |p: V3, far: f32| w.near(p, far).into_iter().map(|n| n.tri).collect::<Vec<_>>();
        assert!(w.near(V3([20.0; 3]), 1e3).iter().all(|n| e.corners[n.tri as usize].iter().all(|c| c.max(n.min) == *c && c.min(n.max) == *c)));
        assert_eq!((near(V3::new(20.3, 16.0, 16.0), 0.5), near(V3::new(19.8, 24.0, 21.0), 0.3), near(V3::new(25.0, 20.0, 20.0), 4.9)), (vec![4, 5], vec![4], Vec::new()));
        assert_eq!(near(V3([20.0; 3]), 1e3).len(), 80);
        // A direction as the rays see it; a still axis is 0 along it, and of no reciprocal.
        let sheared = Sheared::of(V3::new(0.25, -0.5, 0.125));
        assert_eq!(sheared, Sheared { along: [0.25, -0.5, 0.125], axes: [0, 2, 1], shear: [-0.5, -0.25, -2.0], inverse: [4.0, -2.0, 8.0], still: [false; 3] });
        let grazing = Sheared::of(V3::new(1.0, 1e-30, -0.5));
        assert!(grazing.along == [1.0, 0.0, -0.5] && grazing.still == [false, true, false] && grazing.inverse == [1.0, 0.0, -2.0]);
        let level = |side: usize, byte: u8| Level { width: side, height: side, texels: vec![[byte; 4]; side * side] };
        let t = Texture { levels: vec![level(512, 1), level(256, 2), level(64, 3), level(32, 4)], srgb: true, wrap: [Wrap::Mirror, Wrap::Clamp] };
        let w = prepared(SceneInput { textures: vec![None, Some(t)], ..SceneInput::default() }, 1);
        let e = w.export();
        let t = e.textures[1].unwrap();
        assert!(e.textures[0].is_none() && e.corners.is_empty() && t.srgb && t.wrap == [Wrap::Mirror, Wrap::Clamp] && (t.to_linear[255] - 1.0).abs() < 1e-6);
        assert_eq!([t.alpha, t.color].map(|l| (l.width, l.texels[0][0])), [(256, 2), (64, 3)]);
    }

    /// The points of a solve are its reached ones, where the texture says so; a volume that is not sound has none.
    #[test]
    fn a_volume_plainly() {
        let world = prepared(scene(walled(), Vec::new()), 1);
        let config = SolveConfig { gather: GatherConfig { rays: 64, threads: 2, ..GatherConfig::default() }, ..SolveConfig::default() };
        let v = crate::solve(&world, &light(), &Seeds::at(vec![left()]), &config, NEVER).unwrap();
        let points = v.points().unwrap();
        assert!(points.len() == v.counts.reached && points.len() > 50);
        assert!(points.windows(2).all(|w| (w[0].texel[2], w[0].texel[1], w[0].texel[0]) < (w[1].texel[2], w[1].texel[1], w[1].texel[0])));
        for p in &points {
            let [x, y, z] = p.texel.map(|c| c as usize);
            let at = |slab: usize| (((z + slab * v.slab_step() as usize) * v.atlas[1] as usize + y) * v.atlas[0] as usize + x) * 8 + 6;
            assert_eq!(v.rgba16f_le[at(0)..at(0) + 2], [0, 0x3C], "m is 1");
            assert!(v.rgba16f_le[at(2)..at(2) + 2] == [0; 2] && p.position == V3(p.place.map(|c| c as f32 * 64.0)));
        }
        let mut wrong = v.clone();
        wrong.counts.reached -= 1;
        assert_eq!(wrong.points(), Err(Failed::Unavailable(Reason::Input("volume"))));
        // The counts of a point, as its probe has them.
        let at: Vec<V3> = points.iter().map(|p| p.position).collect();
        let counts = classified(&world, &at, &directions(64), 2, NEVER).unwrap();
        assert!(counts.iter().all(|[hits, backs]| *hits == 64 && *backs <= hits / 4));
        assert_eq!(classified(&world, &at, &[], 0, NEVER), Err(Failed::Unavailable(Reason::Input("threads"))));
        assert_eq!(classified(&world, &at, &directions(8), 1, &|| true), Err(Failed::Cancelled));
    }

    /// A point's rays one by one are its probe: the same lights, summed in the same order, and the same counts.
    #[test]
    fn rays_one_by_one() {
        let world = prepared(scene(walled(), Vec::new()), 1);
        let config = GatherConfig { rays: 64, threads: 2, ..GatherConfig::default() };
        let reached = crate::reach(&world, &light(), &[left()], &config, NEVER).unwrap();
        let (place, probe) = reached.points.iter().find(|(_, p)| p.shadow_rays > 0).unwrap();
        let from = reached.position(*place);
        let rays: Vec<(V3, V3)> = directions(64).into_iter().map(|d| (from, d)).collect();
        let b = bounces(&world, &light(), &rays, config.emission, 2, NEVER).unwrap();
        let met = b.iter().flatten();
        let sum = met.clone().fold(V3::ZERO, |s, b| if b.hidden { s } else { s + b.light });
        let counts = met.fold([0; 4], |c, b| [c[0] + 1, c[1] + u32::from(b.hidden), c[2] + b.lamps, c[3] + b.shadow_rays]);
        assert_eq!((sum * (1.0 / 64.0), counts), (probe.moments.c[0], [probe.hits, probe.backs, probe.lamps, probe.shadow_rays]));
        assert!(b.iter().flatten().all(|b| (b.start - b.p).length() > 0.0099 && b.weights.iter().all(|w| (0.0..=1.0).contains(w))));
        assert_eq!(bounces(&world, &light(), &rays, 0.25, 0, NEVER), Err(Failed::Unavailable(Reason::Input("threads"))));
        for emission in [-0.1, 1.001, f32::NAN] {
            assert_eq!(bounces(&world, &light(), &rays, emission, 1, NEVER), Err(Failed::Unavailable(Reason::Input("emission"))));
        }
    }
}
