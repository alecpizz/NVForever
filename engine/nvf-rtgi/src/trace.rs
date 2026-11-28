//! The ray scene: the triangles of all instances in world space, and what stops a ray.
//!
//! - Every surface stops a ray, on either side; a hit on a side that is not drawn is flagged
//!   ([`Hit::hidden`]).
//! - Alpha test on every ray: below the cutoff the ray goes on.
//! - Of hits at exactly the same distance the first in the scene's order is met. Of different triangles in one
//!   plane either may be nearer by a rounding.

use crate::bvh::{Bvh, Corners, Meet, Ray, NODE_BYTES};
use crate::failed::{go_on, unavailable, Cancel, Failed, Reason};
use crate::scene::{Cull, Material, Mesh, SceneInput};
use crate::texture::{Lookup, Sampler};
use crate::vec3::{blend, srgb_to_linear, Affine, V3};

impl Material {
    /// Whether this side is not drawn.
    fn hides(&self, front: bool) -> bool {
        match self.cull {
            Cull::None => false,
            Cull::Back => !front,
            Cull::Front => front,
        }
    }
}

/// What a triangle has beside its corners, as it is after [`crate::prepare`].
#[derive(Clone, Copy, Debug)]
pub struct Face {
    pub material: u32,
    /// The geometric normal, unit, towards the front side.
    pub normal: V3,
    /// Of the corners, unit.
    pub normals: [[f32; 3]; 3],
    /// Those that are not finite are kept, as given.
    pub uv: [[f32; 2]; 3],
    pub colors: [[f32; 4]; 3],
}

/// The most triangles that can be counted at all, and the most threads.
const MOST_TRIANGLES: usize = 1 << 26;
pub const MOST_THREADS: usize = 64;
/// Triangles that are put in place, or instances that are counted, between two looks at `cancel`.
const PLACED: usize = 1 << 16;
/// About what a triangle takes while a scene is prepared: placed, with its box, its number, its place in the
/// tree's order and two nodes.
const TRIANGLE_BYTES: usize = 2 * size_of::<Corners>() + size_of::<Face>() + size_of::<(V3, V3)>() + size_of::<u32>() + 2 * NODE_BYTES;

/// What a scene may be, looked at before any of it is made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneLimits {
    /// Of all the instances together. The default is a budget for play.
    pub triangles: usize,
}

impl Default for SceneLimits {
    fn default() -> SceneLimits {
        SceneLimits { triangles: 1 << 21 }
    }
}

impl SceneLimits {
    /// About what the triangles may take while they are prepared. The input and its textures are not in it.
    pub fn bytes(&self) -> u64 {
        self.triangles.min(MOST_TRIANGLES) as u64 * TRIANGLE_BYTES as u64
    }
}

/// What a scene is made of, and what is wrong with it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneStats {
    pub triangles: usize,
    /// Of the bounding volume tree.
    pub nodes: usize,
    /// Triangles, tree, materials and textures, as they are held.
    pub bytes: usize,
    /// Instances whose transform mirrors.
    pub mirrored: usize,
    /// Triangles without area; those with a corner that is not finite; those of an index that is not there.
    pub degenerate: usize,
    pub not_finite: usize,
    pub broken: usize,
    /// Corners whose vertex colour is not finite: white; whose normal is none: the triangle's; whose texture
    /// coordinates are not finite: 0, 0 where they are read.
    pub colors: usize,
    pub normals: usize,
    pub uvs: usize,
    /// Meshes with an array of another length than the positions', or a part of a triangle at the end.
    pub meshes: usize,
    /// Instances of a mesh or a material that is not there: left out.
    pub instances: usize,
    /// Materials with a number that is not finite: without a cutoff, white or with the plain texture
    /// coordinates, by which of them it is.
    pub materials: usize,
    /// Materials of a texture that is not there, textures that are missing, and those that are not whole: white.
    pub unknown_textures: usize,
    pub missing_textures: usize,
    pub broken_textures: usize,
}

impl SceneStats {
    /// Whether nothing is left out or replaced but triangles without area.
    pub fn sound(&self) -> bool {
        let faults = [self.not_finite, self.broken, self.colors, self.normals, self.uvs, self.meshes, self.instances, self.materials, self.unknown_textures, self.missing_textures, self.broken_textures];
        faults == [0; 11]
    }
}

/// A material's numbers made finite, and whether one was not. A wrong cutoff is none: a surface that is
/// broken stops light rather than letting it through.
fn made_finite(m: &mut Material) -> bool {
    let before = (m.cutoff, m.base, m.uv);
    if !m.cutoff.is_finite() {
        m.cutoff = 0.0;
    }
    m.base = m.base.map(|c| if c.is_finite() { c } else { 1.0 });
    if !m.uv.iter().flatten().all(|c| c.is_finite()) {
        m.uv = Material::default().uv;
    }
    before != (m.cutoff, m.base, m.uv)
}

/// The soup while it is put together.
#[derive(Default)]
struct Soup {
    tris: Vec<Corners>,
    faces: Vec<Face>,
    stats: SceneStats,
}

impl Soup {
    /// The mesh's triangles in world space; a mirroring transform turns the corners round, so
    /// that the front stays counter-clockwise.
    fn add(&mut self, mesh: &Mesh, at: &Affine, material: u32, cancel: Cancel) -> Result<(), Failed> {
        let mirrored = at.determinant() < 0.0;
        self.stats.mirrored += usize::from(mirrored);
        for (n, t) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
            if n.is_multiple_of(PLACED) {
                go_on(cancel)?;
            }
            let mut c = t.map(|i| i as usize);
            if c.iter().any(|i| *i >= mesh.positions.len()) {
                self.stats.broken += 1;
                continue;
            }
            if mirrored {
                c.swap(1, 2);
            }
            let tri = c.map(|i| at.point(V3(mesh.positions[i])));
            if !tri.iter().all(|p| p.finite()) {
                self.stats.not_finite += 1;
                continue;
            }
            let (u, v) = (tri[1] - tri[0], tri[2] - tri[0]);
            // Scaled first: the products of a sliver's sides are tiny.
            let Some(normal) = u.unit().zip(v.unit()).and_then(|(u, v)| u.cross(v).unit()) else {
                self.stats.degenerate += 1;
                continue;
            };
            let colors = c.map(|i| match mesh.colors.get(i) {
                Some(c) if !c.iter().all(|c| c.is_finite()) => {
                    self.stats.colors += 1;
                    [1.0; 4]
                }
                c => c.copied().unwrap_or([1.0; 4]),
            });
            let normals = c.map(|i| {
                let given = mesh.normals.get(i);
                let turned = given.and_then(|n| at.normal(V3(*n)));
                self.stats.normals += usize::from(given.is_some() && turned.is_none());
                turned.unwrap_or(normal).0
            });
            let uv = c.map(|i| mesh.uv0.get(i).copied().unwrap_or_default());
            self.stats.uvs += uv.iter().filter(|uv| !uv.iter().all(|c| c.is_finite())).count();
            self.faces.push(Face {
                material,
                normal,
                normals,
                uv,
                colors,
            });
            self.tris.push(tri);
        }
        Ok(())
    }
}

/// Where a ray stopped in a scene.
#[derive(Clone, Copy)]
pub struct Hit<'a> {
    scene: &'a PreparedScene,
    tri: u32,
    /// Of the triangle's corners.
    weights: [f32; 3],
    /// In units of the ray's direction.
    pub t: f32,
    pub p: V3,
    /// Whether the ray met the front side.
    pub front: bool,
    /// Whether the side met is not drawn (a single-sided surface's back).
    pub hidden: bool,
}

impl<'a> Hit<'a> {
    fn face(&self) -> &'a Face {
        &self.scene.faces[self.tri as usize]
    }

    pub fn material(&self) -> &'a Material {
        self.scene.material(self.tri)
    }

    /// The triangle's number, as in [`crate::Export`].
    pub fn tri(&self) -> u32 {
        self.tri
    }

    /// Of the triangle's corners.
    pub fn weights(&self) -> [f32; 3] {
        self.weights
    }

    /// Texture x material x vertex colour, linear, with the alpha of the same lookup.
    pub fn albedo(&self, lookup: Lookup) -> [f32; 4] {
        self.scene.albedo(self.tri, self.weights, lookup)
    }

    /// The geometric normal on the side the ray came from.
    pub fn facing(&self) -> V3 {
        let n = self.face().normal;
        if self.front {
            n
        } else {
            -n
        }
    }

    /// The interpolated normal, unit; that of the triangle where it vanishes.
    pub fn interpolated(&self) -> V3 {
        V3(blend(&self.face().normals, self.weights)).unit().unwrap_or(self.face().normal)
    }

    /// [`Hit::interpolated`], turned towards the ray on the back of a double-sided surface.
    pub fn shading(&self) -> V3 {
        let n = self.interpolated();
        if !self.front && self.material().double_sided {
            -n
        } else {
            n
        }
    }
}

/// A cell's triangles, ready for rays ([`crate::prepare`]).
pub struct PreparedScene {
    bvh: Bvh,
    faces: Vec<Face>,
    materials: Vec<Material>,
    textures: Vec<Option<Sampler>>,
    stats: SceneStats,
}

impl PreparedScene {
    pub(crate) fn of(scene: SceneInput, limits: &SceneLimits, threads: usize, cancel: Cancel) -> Result<PreparedScene, Failed> {
        let SceneInput { meshes, instances, mut materials, textures } = scene;
        if !(1..=MOST_THREADS).contains(&threads) {
            return unavailable(Reason::Input("threads"));
        }
        // Each instance's mesh and material, where both are there.
        let parts = instances.iter().map(|x| {
            let material = u32::try_from(x.material).ok().filter(|_| x.material < materials.len())?;
            Some((meshes.get(x.mesh)?, Affine(x.world_from_local), material))
        });
        let limit = limits.triangles.min(MOST_TRIANGLES);
        let mut count = 0usize;
        for (i, part) in parts.clone().enumerate() {
            if i.is_multiple_of(PLACED) {
                go_on(cancel)?;
            }
            count = count.saturating_add(part.map_or(0, |(mesh, _, _)| mesh.indices.len() / 3));
            if count > limit {
                return unavailable(Reason::TooMany { what: "triangles", limit });
            }
        }
        let mut soup = Soup { tris: Vec::with_capacity(count), faces: Vec::with_capacity(count), ..Soup::default() };
        for part in parts {
            go_on(cancel)?;
            match part {
                Some((mesh, at, material)) => soup.add(mesh, &at, material, cancel)?,
                None => soup.stats.instances += 1,
            }
        }
        let uneven = |m: &&Mesh| !m.indices.len().is_multiple_of(3) || [m.normals.len(), m.uv0.len(), m.colors.len()].iter().any(|n| *n != 0 && *n != m.positions.len());
        soup.stats.meshes = meshes.iter().filter(uneven).count();
        soup.stats.materials = materials.iter_mut().map(made_finite).filter(|wrong| *wrong).count();
        soup.stats.unknown_textures = materials.iter().filter(|m| m.texture.is_some_and(|t| t >= textures.len())).count();
        soup.stats.missing_textures = textures.iter().filter(|t| t.is_none()).count();
        let bvh = Bvh::build(&soup.tris, threads, cancel).ok_or(Failed::Cancelled)?;
        let mut samplers = Vec::with_capacity(textures.len());
        for t in textures {
            go_on(cancel)?;
            let sampler = t.map(Sampler::new);
            soup.stats.broken_textures += usize::from(matches!(sampler, Some(None)));
            samplers.push(sampler.flatten());
        }
        let held = samplers.capacity() * size_of::<Option<Sampler>>() + samplers.iter().flatten().map(Sampler::bytes).sum::<usize>();
        let stats = SceneStats {
            triangles: soup.tris.len(),
            nodes: bvh.node_count(),
            bytes: bvh.bytes() + soup.faces.capacity() * size_of::<Face>() + materials.capacity() * size_of::<Material>() + held,
            ..soup.stats
        };
        Ok(PreparedScene { bvh, faces: soup.faces, materials, textures: samplers, stats })
    }

    pub fn stats(&self) -> SceneStats {
        self.stats
    }

    /// The box about all triangles, when there are any.
    pub fn bounds(&self) -> Option<(V3, V3)> {
        self.bvh.bounds()
    }

    /// What an [`Export`](crate::Export) is of.
    pub(crate) fn parts(&self) -> (&Bvh, &[Face], &[Material], &[Option<Sampler>]) {
        (&self.bvh, &self.faces, &self.materials, &self.textures)
    }

    fn material(&self, tri: u32) -> &Material {
        &self.materials[self.faces[tri as usize].material as usize]
    }

    fn uv(&self, face: &Face, weights: [f32; 3]) -> [f32; 2] {
        let uv = blend(&face.uv, weights);
        self.materials[face.material as usize].uv.map(|r| r[0] * uv[0] + r[1] * uv[1] + r[2])
    }

    fn albedo(&self, tri: u32, weights: [f32; 3], lookup: Lookup) -> [f32; 4] {
        let face = &self.faces[tri as usize];
        let m = &self.materials[face.material as usize];
        let mut c = blend(&face.colors, weights);
        if m.display_colors {
            c = [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), c[3]];
        }
        let texture = m.texture.and_then(|t| self.textures.get(t)?.as_ref());
        let texel = texture.map_or([1.0; 4], |t| t.sample(self.uv(face, weights), lookup));
        std::array::from_fn(|k| c[k] * m.base[k] * texel[k])
    }

    /// Whether the surface is there at the candidate (the alpha test).
    fn solid(&self, m: &Meet) -> bool {
        let cutoff = self.material(m.tri).cutoff;
        if cutoff <= 0.0 {
            return true;
        }
        let alpha = self.albedo(m.tri, m.weights, Lookup::Alpha)[3];
        // One that is no number (an overflow times 0) stops.
        alpha >= cutoff || alpha.is_nan()
    }

    /// What a bounce ray meets before `far` or there, in units of the ray's direction: every surface, either side.
    pub fn transport(&self, r: &Ray, far: f32) -> Option<Hit<'_>> {
        let m = self.bvh.closest(r, 0.0, far, |m| self.solid(m))?;
        let c = self.bvh.corners(&m);
        let front = self.faces[m.tri as usize].normal.dot(r.direction()) < 0.0;
        let p = V3(blend(&[c[0].0, c[1].0, c[2].0], m.weights));
        Some(Hit { scene: self, tri: m.tri, weights: m.weights, t: m.t, p, front, hidden: self.material(m.tri).hides(front) })
    }

    /// Whether the ray is stopped before `far`.
    pub fn blocked(&self, r: &Ray, far: f32) -> bool {
        self.bvh.any(r, 0.0, far, |m| self.solid(m))
    }

    /// Whether the open segment from a hit (leaving on the side of `n`) to `to` is free; both
    /// ends are drawn in by [`lift`].
    pub fn sees(&self, p: V3, n: V3, to: V3) -> bool {
        let from = p + n * lift(p);
        let d = to - from;
        !self.blocked(&Ray::new(from, d), 1.0 - lift(to) / d.length().max(1e-6))
    }

    /// Whether nothing is on the segment, its ends included: both are drawn out by [`lift`]. Two equal ends
    /// have nothing between them, on a surface too.
    pub fn open(&self, a: V3, b: V3) -> bool {
        let Some(d) = (b - a).unit() else { return true };
        let (from, to) = (a - d * lift(a), b + d * lift(b));
        !self.blocked(&Ray::new(from, to - from), 1.0)
    }
}

/// How far a ray that leaves `p` starts off the surface: 0.01 units, more where the largest coordinate
/// is above 2,500 (4e-6 of it).
pub fn lift(p: V3) -> f32 {
    0.01f32.max(4e-6 * p.max_abs())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::bvh::tests::direction;
    use crate::scene::{Instance, Texture, Wrap};
    use crate::texture::tests::rgba;
    use crate::vec3::tests::Rng;

    pub const SAME: [[f32; 4]; 3] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
    const UP: V3 = V3([0.0, 0.0, 1.0]);

    fn near(a: V3, b: V3) -> bool {
        (a - b).length() < 1e-5
    }

    /// A quad in the plane z = `z` over -1..1, facing +z, its texture once across it.
    pub fn quad(z: f32) -> Mesh {
        Mesh {
            positions: vec![[-1.0, -1.0, z], [1.0, -1.0, z], [1.0, 1.0, z], [-1.0, 1.0, z]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            uv0: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            colors: Vec::new(),
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    /// Each mesh in its place, of its own material.
    pub fn scene(parts: Vec<(Mesh, [[f32; 4]; 3], Material)>, textures: Vec<Option<Texture>>) -> SceneInput {
        let mut out = SceneInput { textures, ..SceneInput::default() };
        for (i, (mesh, world_from_local, material)) in parts.into_iter().enumerate() {
            out.instances.push(Instance { mesh: i, material: i, world_from_local });
            out.meshes.push(mesh);
            out.materials.push(material);
        }
        out
    }

    pub fn prepared(scene: SceneInput, threads: usize) -> PreparedScene {
        PreparedScene::of(scene, &SceneLimits::default(), threads, &|| false).unwrap()
    }

    fn one(mesh: Mesh, material: Material, textures: Vec<Option<Texture>>) -> PreparedScene {
        prepared(scene(vec![(mesh, SAME, material)], textures), 1)
    }

    fn world(quads: &[(f32, Material)], textures: Vec<Option<Texture>>) -> PreparedScene {
        prepared(scene(quads.iter().map(|(z, m)| (quad(*z), SAME, *m)).collect(), textures), 1)
    }

    fn down(x: f32) -> Ray {
        Ray::new(V3::new(x, 0.1, 9.0), -UP)
    }

    fn material(w: &PreparedScene, h: Option<Hit<'_>>) -> Option<u32> {
        h.map(|h| w.faces[h.tri as usize].material)
    }

    /// A ball of 2,048 triangles far from the origin.
    fn ball(centre: V3, radius: f32) -> Mesh {
        let (rings, around) = (32usize, 32usize);
        let mut m = Mesh::default();
        for i in 0..=rings {
            for k in 0..=around {
                let (a, b) = (std::f32::consts::PI * i as f32 / rings as f32, std::f32::consts::TAU * k as f32 / around as f32);
                let n = V3::new(a.sin() * b.cos(), a.cos(), a.sin() * b.sin());
                m.positions.push((centre + n * radius).0);
                m.normals.push(n.0);
            }
        }
        for i in 0..rings * (around + 1) {
            let j = i + around + 1;
            if i % (around + 1) < around {
                m.indices.extend([i, i + 1, j, i + 1, j + 1, j].map(|i| i as u32));
            }
        }
        m
    }

    /// Rays that leave a ball's outside see nothing of it; from its inside they all meet it,
    /// on the hidden side (no ray slips out between two triangles).
    #[test]
    fn no_surface_shadows_itself() {
        let (centre, mut rng) = (V3::new(7000.0, 2000.0, -4000.0), Rng(3));
        let w = one(ball(centre, 50.0), Material::default(), Vec::new());
        assert_eq!(w.stats().degenerate + w.stats().triangles, 2048);
        for _ in 0..5000 {
            let d = direction(&mut rng);
            let h = w.transport(&Ray::new(centre + d * 90.0, -d), f32::INFINITY).unwrap();
            assert!(h.front && !h.hidden && (h.p - centre).length() < 50.1);
            let n = h.facing();
            let out = Ray::new(h.p + n * lift(h.p), rng.cosine(n));
            assert!(w.transport(&out, f32::INFINITY).is_none() && !w.blocked(&out, 1e4));
            assert!(w.sees(h.p, n, h.p + n * 500.0));
            assert!(!w.sees(h.p, n, centre + (centre - h.p) * 2.0), "the far side");
            let inside = w.transport(&Ray::new(centre + direction(&mut rng) * 40.0, direction(&mut rng)), f32::INFINITY).unwrap();
            assert!(!inside.front && inside.hidden);
        }
    }

    /// A segment that ends on a surface is not stopped by that surface; an open one is.
    #[test]
    fn both_ends_are_drawn_in() {
        let w = world(&[(0.0, Material::default()), (3.0, Material::default())], Vec::new());
        assert!(w.sees(V3::new(0.2, 0.1, 0.0), UP, V3::new(-0.3, 0.2, 3.0)));
        assert!(!w.sees(V3::new(0.2, 0.1, 0.0), UP, V3::new(-0.3, 0.2, 3.2)));
        let at = |z: f32| V3::new(0.2, 0.1, z);
        assert!(w.open(at(0.5), at(2.5)) && !w.open(at(0.5), at(3.0)) && !w.open(at(0.0), at(2.5)) && w.open(at(0.0), at(0.0)));
    }

    #[test]
    fn sides() {
        let two = Material { cull: Cull::None, double_sided: true, ..Material::default() };
        let w = world(&[(0.0, Material::default()), (2.0, two), (4.0, Material { cull: Cull::Front, ..Material::default() })], Vec::new());
        // From above the front of each, from below the back.
        let up = Ray::new(V3::new(0.2, 0.1, -9.0), UP);
        let h = w.transport(&down(0.2), f32::INFINITY).unwrap();
        assert_eq!((material(&w, Some(h)), h.front, h.hidden), (Some(2), true, true));
        let h = w.transport(&up, f32::INFINITY).unwrap();
        assert_eq!((material(&w, Some(h)), h.front, h.hidden), (Some(0), false, true));
        let h = w.transport(&Ray::new(V3::new(0.2, 0.1, 1.0), UP), f32::INFINITY).unwrap();
        assert_eq!((h.front, h.hidden), (false, false));
        assert!(near(h.facing(), -UP) && near(h.shading(), -UP));
        let h = w.transport(&up, f32::INFINITY).unwrap();
        assert!(near(h.shading(), UP), "not turned on a single-sided one");
        assert!(w.blocked(&down(0.2), 5.1) && !w.blocked(&down(0.2), 4.9));
    }

    /// Several cut-out layers: left half of the texture gone; the vertex and the material alpha
    /// count; the cut holds at its cutoff.
    #[test]
    fn rays_go_on_below_the_cutoff() {
        let cut = |cutoff, alpha, shift| Material { cutoff, base: [1.0, 1.0, 1.0, alpha], texture: Some(0), uv: [[1.0, 0.0, shift], [0.0, 1.0, 0.0]], ..Material::default() };
        let texels: Vec<[u8; 4]> = (0..8).map(|i| [255, 255, 255, if i < 4 { 0 } else { 255 }]).collect();
        let layers = [(0.0, Material::default()), (1.0, cut(0.5, 1.0, 0.0)), (2.0, cut(0.5, 1.0, 0.5)), (3.0, cut(0.5, 0.4, 0.5)), (4.0, cut(0.0, 0.0, 0.0))];
        for (n, want) in [(2, [0, 1]), (3, [2, 1]), (4, [2, 1]), (5, [4, 4])] {
            let w = world(&layers[..n], vec![Some(rgba(8, 1, &texels, false, Wrap::Repeat))]);
            for (x, want) in [-0.5, 0.5].into_iter().zip(want) {
                assert_eq!(material(&w, w.transport(&down(x), 99.0)), Some(want), "{n} layers at {x}");
                assert!(w.blocked(&down(x), 9.01 - want as f32) && !w.blocked(&down(x), 8.99 - want as f32));
            }
        }
        let mut m = quad(1.0);
        m.colors = vec![[1.0, 1.0, 1.0, 0.0], [1.0; 4], [1.0; 4], [1.0, 1.0, 1.0, 0.0]];
        let w = one(m, Material { cutoff: 0.5, ..Material::default() }, Vec::new());
        assert!(w.transport(&down(-0.1), 99.0).is_none() && w.transport(&down(0.1), 99.0).is_some());
    }

    /// The albedo's factors, display vertex colours and the missing texture.
    #[test]
    fn albedo_factors() {
        let mut m = quad(0.0);
        m.colors = vec![[0.5, 1.0, 1.0, 1.0]; 4];
        let lit = Material { base: [1.0, 0.5, 1.0, 1.0], texture: Some(0), ..Material::default() };
        let unlit = Material { display_colors: true, texture: Some(1), ..lit };
        let nowhere = Material { texture: Some(7), ..lit };
        let parts = [lit, unlit, nowhere].map(|s| (m.clone(), SAME, s)).to_vec();
        let w = prepared(scene(parts, vec![Some(rgba(1, 1, &[[255, 255, 51, 255]], false, Wrap::Repeat)), None]), 1);
        let c = w.albedo(0, [0.2, 0.3, 0.5], Lookup::Color);
        assert!((c[0] - 0.5).abs() < 1e-6 && (c[1] - 0.5).abs() < 1e-6 && (c[2] - 0.2).abs() < 1e-6, "{c:?}");
        let c = w.albedo(2, [0.2, 0.3, 0.5], Lookup::Color);
        assert!((c[0] - 0.214_041).abs() < 1e-4 && (c[2] - 1.0).abs() < 1e-5, "{c:?}");
        assert!((w.albedo(4, [0.2, 0.3, 0.5], Lookup::Color)[2] - 1.0).abs() < 1e-5, "a texture that is not there is white");
    }

    /// Of triangles that are the same the first in the scene's order is met, whatever the tree; one that is
    /// cut away there is not.
    #[test]
    fn coplanar_triangles() {
        let gone = Material { cutoff: 0.5, base: [1.0, 1.0, 1.0, 0.2], ..Material::default() };
        let plain = Material::default();
        for (n, first) in [(2, 0), (40, 0), (40, 7)] {
            let quads: Vec<(f32, Material)> = (0..n).map(|i| (0.0, if i < first { gone } else { plain })).collect();
            let w = world(&quads, Vec::new());
            for x in [-0.9, -0.2, 0.3, 0.85] {
                assert_eq!(material(&w, w.transport(&down(x), 99.0)), Some(first as u32), "{n} {first} {x}");
            }
        }
        // A little in front is in front.
        let w = world(&[(0.0, plain), (0.05, plain)], Vec::new());
        assert_eq!(material(&w, w.transport(&down(0.3), 99.0)), Some(1));
    }

    #[test]
    fn broken_triangles_are_dropped() {
        let mut m = quad(0.0);
        m.positions.extend([[f32::NAN, 0.0, 0.0], [f32::INFINITY, 0.0, 0.0], [0.0, 0.0, 0.0], [0.5, 0.5, 0.0]]);
        m.normals = vec![[f32::NAN; 3]; 4];
        m.uv0 = vec![[f32::NAN; 2]; 4];
        // The last two: of a vertex that is not there, and no whole triangle.
        m.indices.extend([0, 1, 4, 5, 1, 2, 0, 0, 1, 0, 6, 2, 6, 7, 2, 0, 1, 8, 0]);
        let mut s = scene(vec![(m, SAME, Material { texture: Some(0), ..Material::default() }), (quad(1.0), [[0.0; 4]; 3], Material::default())], vec![Some(rgba(1, 1, &[[255; 4]], false, Wrap::Repeat)), Some(rgba(1, 2, &[[255; 4]], false, Wrap::Repeat)), None]);
        s.instances.extend([Instance { mesh: 9, material: 0, world_from_local: SAME }, Instance { mesh: 0, material: 9, world_from_local: SAME }]);
        let w = prepared(s, 1);
        let n = w.stats();
        assert_eq!((n.triangles, n.not_finite, n.degenerate, n.broken), (2, 2, 5, 1), "two of a transform without volume");
        assert_eq!((n.instances, n.missing_textures, n.broken_textures, n.meshes, n.sound()), (2, 1, 1, 1, false));
        // Of the two triangles that are kept.
        assert_eq!((n.normals, n.uvs), (6, 6));
        let h = w.transport(&down(0.3), 99.0).unwrap();
        assert!(near(h.shading(), UP), "no usable normal: the triangle's");
        assert!(h.albedo(Lookup::Color).iter().all(|c| (c - 1.0).abs() < 1e-5));
        for r in [Ray::new(V3::new(f32::NAN, 0.0, 0.0), UP), Ray::new(V3::ZERO, V3::ZERO)] {
            assert!(w.transport(&r, 99.0).is_none() && !w.blocked(&r, 99.0));
        }
        let none = prepared(SceneInput::default(), 4);
        assert!(none.bounds().is_none() && none.transport(&down(0.0), 99.0).is_none() && none.stats() == SceneStats::default());
        assert!(one(quad(0.0), Material::default(), Vec::new()).stats().sound());
    }

    /// A surface with a number that is none stops rays, is counted, and gives numbers.
    #[test]
    fn broken_materials_are_opaque() {
        let wrong = |f: &dyn Fn(&mut Material)| {
            let mut m = Material { cutoff: 0.5, texture: Some(0), ..Material::default() };
            f(&mut m);
            let w = one(quad(0.0), m, vec![Some(rgba(1, 1, &[[255; 4]], false, Wrap::Repeat))]);
            let h = w.transport(&down(0.3), 99.0).unwrap();
            assert!(w.blocked(&down(0.3), 99.0) && h.albedo(Lookup::Color).iter().all(|c| c.is_finite()));
            (w.stats().materials, w.stats().sound())
        };
        assert_eq!(wrong(&|m| m.cutoff = f32::NAN), (1, false));
        assert_eq!(wrong(&|m| m.cutoff = f32::INFINITY), (1, false));
        assert_eq!(wrong(&|m| m.base[3] = f32::NAN), (1, false));
        assert_eq!(wrong(&|m| m.uv[0][2] = f32::NAN), (1, false));
        assert_eq!(wrong(&|_| ()), (0, true));
        let mut m = quad(0.0);
        m.colors = vec![[1.0, 1.0, 1.0, f32::NAN]; 4];
        let w = one(m, Material { cutoff: 0.5, texture: Some(3), ..Material::default() }, Vec::new());
        assert!(w.blocked(&down(0.3), 99.0) && (w.stats().colors, w.stats().unknown_textures) == (6, 1));
        // A normal that is none, texture coordinates that are none: counted, and not where there are none at all.
        let count = |f: &dyn Fn(&mut Mesh)| {
            let mut m = quad(0.0);
            f(&mut m);
            let n = one(m, Material::default(), Vec::new()).stats();
            (n.normals, n.uvs, n.sound())
        };
        assert_eq!(count(&|m| m.normals = vec![[0.0; 3]; 4]), (6, 0, false));
        assert_eq!(count(&|m| m.uv0[1][0] = f32::INFINITY), (0, 1, false));
        assert_eq!(count(&|m| (m.normals, m.uv0) = (Vec::new(), Vec::new())), (0, 0, true));
    }

    #[test]
    fn what_is_not_prepared() {
        let of = |input: SceneInput, threads| PreparedScene::of(input, &SceneLimits::default(), threads, &|| false).err();
        for threads in [0, MOST_THREADS + 1, usize::MAX] {
            assert_eq!(of(SceneInput::default(), threads), Some(Failed::Unavailable(Reason::Input("threads"))));
        }
        // Few triangles of many instances: refused before any is made.
        let mut many = scene(vec![(Mesh { indices: vec![0; 3 << 12], ..quad(0.0) }, SAME, Material::default())], Vec::new());
        let limit = SceneLimits::default().triangles;
        many.instances = vec![many.instances[0]; (limit >> 12) + 1];
        assert_eq!(of(many.clone(), 1), Some(Failed::Unavailable(Reason::TooMany { what: "triangles", limit })));
        // The same may be where the caller says so; lifted altogether there is still a last one.
        many.instances.truncate(4);
        let with = |triangles| PreparedScene::of(many.clone(), &SceneLimits { triangles }, 1, &|| false).map(|w| w.stats().degenerate);
        assert_eq!([4 << 12, (4 << 12) - 1].map(with), [Ok(4 << 12), Err(Failed::Unavailable(Reason::TooMany { what: "triangles", limit: (4 << 12) - 1 }))]);
        many.instances = vec![many.instances[0]; (MOST_TRIANGLES >> 12) + 1];
        let lifted = PreparedScene::of(many.clone(), &SceneLimits { triangles: usize::MAX }, 1, &|| false).err();
        assert_eq!(lifted, Some(Failed::Unavailable(Reason::TooMany { what: "triangles", limit: MOST_TRIANGLES })));
        assert!(SceneLimits::default().bytes() <= 640 << 20 && SceneLimits { triangles: usize::MAX }.bytes() < 1 << 40);
        let stop = PreparedScene::of(scene(vec![(quad(0.0), SAME, Material::default())], Vec::new()), &SceneLimits::default(), 1, &|| true);
        assert_eq!(stop.err(), Some(Failed::Cancelled));
        // While the instances are counted, before anything else is asked.
        many.instances.truncate(PLACED + 1);
        many.meshes[0].indices.clear();
        let asked = std::sync::atomic::AtomicUsize::new(0);
        let late = || asked.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 1;
        many.instances = vec![many.instances[0]; PLACED + 1];
        let stop = PreparedScene::of(many, &SceneLimits::default(), 1, &late);
        assert!(stop.err() == Some(Failed::Cancelled) && asked.into_inner() == 2);
        // In the middle of one large mesh.
        let large = Mesh { indices: vec![0; 3 * (PLACED + 1)], ..quad(0.0) };
        let asked = std::sync::atomic::AtomicUsize::new(0);
        let late = || asked.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 3;
        let stop = PreparedScene::of(scene(vec![(large, SAME, Material::default())], Vec::new()), &SceneLimits::default(), 1, &late);
        assert!(stop.err() == Some(Failed::Cancelled) && asked.into_inner() == 4, "the count, the part, its start, and after {PLACED}");
    }

    /// A mirrored copy keeps its front, its shading normal and its texture.
    #[test]
    fn mirrored_instances_keep_their_front() {
        let texels = [[255, 0, 0, 255], [0, 255, 0, 255]];
        let turned = [[-2.0, 0.0, 0.0, 10.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 1.0]];
        let w = prepared(scene(vec![(quad(0.0), turned, Material { texture: Some(0), ..Material::default() })], vec![Some(rgba(2, 1, &texels, false, Wrap::Clamp))]), 1);
        assert_eq!(w.stats().mirrored, 1);
        let h = w.transport(&Ray::new(V3::new(11.4, 0.1, 9.0), -UP), 99.0).unwrap();
        assert!(h.front && !h.hidden && near(h.p, V3::new(11.4, 0.1, 1.0)));
        assert!(near(h.facing(), UP) && near(h.shading(), UP));
        assert!((h.albedo(Lookup::Color)[0] - 1.0).abs() < 1e-5, "local x = -0.7, the texture's left half");
        assert!(w.transport(&Ray::new(V3::new(11.4, 0.1, -9.0), UP), 99.0).unwrap().hidden);
    }
}
