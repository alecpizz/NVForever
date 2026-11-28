//! NVForever's tree generator (docs/spec-notes/speedtree.md §5). This is not the
//! original library's algorithm (undocumented); it builds a tree of the same shape from
//! the parameters: tube branches level by level and leaf cards on the last wood level.
//! Space: Z up, tree size 1, trunk base at the origin.

use crate::{Level, Tree};

/// A leaf card: drawn as a camera-facing quad of `size` attached at `pivot`.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaf {
    pub position: [f32; 3],
    /// Index into `Tree::leaf_maps` / `Tree::leaf_uvs`.
    pub map: usize,
    /// Width, height.
    pub size: [f32; 2],
    /// Attachment point as a fraction of width and height from the bottom left.
    pub pivot: [f32; 2],
    /// Lighting normal: horizontally away from the middle of the crown, tilted up 45°
    /// (a rule chosen by eye; speedtree.md §6).
    pub normal: [f32; 3],
}

/// Branch triangles (Z up) and leaf cards.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// u around the branch, v along it, in bark texture repeats.
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub leaves: Vec<Leaf>,
    /// Lowest and highest leaf attachment point (z): the crown, for dimming.
    pub crown: [f32; 2],
}

impl Geometry {
    /// Highest point of the tree: branch vertices and the tops of the leaf cards.
    pub fn top(&self) -> f32 {
        let wood = self.positions.iter().map(|p| p[2]);
        let leaves = self.leaves.iter().map(|l| l.position[2] + l.size[1] * (1.0 - l.pivot[1]));
        wood.chain(leaves).fold(0.0, f32::max)
    }

    /// Width of the tree seen from the side: the larger of its x and y extents, leaf
    /// cards counted at their full width.
    pub fn width(&self) -> f32 {
        let mut lo = [f32::MAX; 2];
        let mut hi = [f32::MIN; 2];
        let mut grow = |p: [f32; 3], r: f32| {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k] - r);
                hi[k] = hi[k].max(p[k] + r);
            }
        };
        for &p in &self.positions {
            grow(p, 0.0);
        }
        for l in &self.leaves {
            grow(l.position, l.size[0] * l.pivot[0].max(1.0 - l.pivot[0]));
        }
        if lo[0] > hi[0] {
            return 0.0;
        }
        (hi[0] - lo[0]).max(hi[1] - lo[1])
    }

    /// Brightness at height z for a dimming value (TREE CNAM leaf or branch dimming):
    /// `dimming` at the bottom of the crown and below, rising to 1 at its top along the
    /// cube root of the height in the crown (speedtree.md §6) [hypothesis: the curve is
    /// fitted to the top-to-bottom brightness of two billboards]. A display-space factor.
    pub fn brightness(&self, dimming: f32, z: f32) -> f32 {
        let [lo, hi] = self.crown;
        let h = if hi > lo { ((z - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 1.0 };
        let d = dimming.clamp(0.0, 1.0);
        d + (1.0 - d) * h.cbrt()
    }
}

/// Generate the tree. Deterministic: the same parameters give the same geometry.
pub fn generate(tree: &Tree) -> Geometry {
    generate_with(tree, &angle_from_axis(tree))
}

/// Per level: whether its 6007 angle is measured from the parent's axis
/// ([`springs_from_stub`]) rather than from the plane across the parent.
pub fn angle_from_axis(tree: &Tree) -> Vec<bool> {
    (0..tree.levels.len()).map(|i| springs_from_stub(tree, i)).collect()
}

/// [`generate`] with the angle reading of each level given (`from_axis[i]`; missing
/// entries read from the plane): for the measuring aids of `crate::probe`, which hold the
/// readings fixed while they edit other parameters.
pub fn generate_with(tree: &Tree, from_axis: &[bool]) -> Geometry {
    let mut g = Geometry::default();
    let mut rng = Rng::new(tree.seed);
    let trunk = &tree.levels[0];
    let length = trunk.length().sample(0.0, rng.f()).max(1e-4);
    let radius = trunk.radius().sample(0.0, rng.f()).max(0.0);
    let mut budget = Budget { branches: MAX_BRANCHES };
    let at = Branch { li: 0, base: [0.0, 0.0, 0.0], dir: [0.0, 0.0, 1.0], length, radius };
    grow(tree, from_axis, at, &mut rng, &mut g, &mut budget);
    light_leaves(&mut g.leaves);
    let zs = || g.leaves.iter().map(|l| l.position[2]);
    g.crown = if g.leaves.is_empty() {
        [0.0, g.top()]
    } else {
        [zs().fold(f32::MAX, f32::min), zs().fold(f32::MIN, f32::max)]
    };
    g
}

/// Guard against runaway counts from odd parameters.
const MAX_CHILDREN: f32 = 4096.0;

/// Limits on the whole tree, so a broken file's frequencies (4096 children a level,
/// level after level) cannot grow it without end: past them the tree stops growing
/// [observed: the base game's trees (`Fallout - Meshes.bsa`, 10 files) have at most 5
/// levels, 7284 branch vertices and 6095 leaves; 3-5 levels, speedtree.md §3].
pub const MAX_LEVELS: usize = 8;
pub const MAX_VERTICES: usize = 200_000;
pub const MAX_LEAVES: usize = 20_000;
/// Branches grown (with or without wood: a branch of radius 0 adds no vertices).
pub const MAX_BRANCHES: usize = 50_000;

/// What is left of the whole tree's limits besides the geometry's own counts.
struct Budget {
    branches: usize,
}

/// Golden angle: successive children turn this far around their parent.
const GOLDEN: f32 = 2.399_963;

/// One branch to grow: its level, base point, direction, length and base radius.
struct Branch {
    li: usize,
    base: V,
    dir: V,
    length: f32,
    radius: f32,
}

fn grow(tree: &Tree, from_axis: &[bool], b: Branch, rng: &mut Rng, g: &mut Geometry, budget: &mut Budget) {
    let Branch { li, base, dir, length, radius } = b;
    if budget.branches == 0 || g.positions.len() >= MAX_VERTICES {
        return;
    }
    budget.branches -= 1;
    let lv = &tree.levels[li];
    let segs = lv.segments_along.clamp(1, 32) as usize;
    // Centre line: equal steps, each turned a little from the last by the 6000 spread.
    let bend = lv.bend().variance.to_radians() / segs as f32;
    let mut pts = vec![base];
    let mut dirs = vec![dir];
    let mut d = dir;
    for s in 0..segs {
        if s > 0 && bend > 0.0 {
            let axis = rotate(perpendicular(d), d, rng.f() * std::f32::consts::TAU);
            d = norm(rotate(d, axis, (2.0 * rng.f() - 1.0) * bend));
        }
        pts.push(add(pts[s], scale(d, length / segs as f32)));
        dirs.push(d);
    }
    if radius > 0.0 {
        tube(lv, &pts, &dirs, radius, g);
    }

    let child = li + 1;
    let levels = tree.levels.len().min(MAX_LEVELS);
    if child >= levels {
        return;
    }
    let cl = &tree.levels[child];
    let leafy = child == levels - 1;
    let axis_reading = from_axis.get(child).copied().unwrap_or(false);
    // Children per unit of length (6012), rounded up or down at random.
    let n = (lv.frequency.max(0.0) * length + rng.f()).floor().min(MAX_CHILDREN) as usize;
    let mut azimuth = rng.f() * std::f32::consts::TAU;
    for k in 0..n {
        let t = lv.first + (lv.last - lv.first) * ((k as f32 + rng.f()) / n as f32);
        let (at, along) = point_at(&pts, &dirs, t);
        if leafy {
            if tree.leaf_maps.is_empty() || g.leaves.len() >= MAX_LEAVES {
                return;
            }
            let map = ((rng.f() * tree.leaf_maps.len() as f32) as usize).min(tree.leaf_maps.len() - 1);
            let m = &tree.leaf_maps[map];
            let s = 0.85 + 0.3 * rng.f();
            g.leaves.push(Leaf { position: at, map, size: [m.size[0] * s, m.size[1] * s], pivot: m.pivot, normal: [0.0, 0.0, 1.0] });
            continue;
        }
        let clen = cl.length().sample(t, rng.f()).max(1e-4);
        let parent_r = radius * lv.taper().y(t).max(0.0);
        let crad = cl.radius().sample(t, rng.f()).clamp(0.0, parent_r.max(0.0));
        // Measured from the plane across the parent, positive towards its tip
        // [hypothesis: WhiteOak01's level-1 range −10° to 60° only makes sense so]; from
        // the parent's axis for branches springing from a stub (`springs_from_stub`).
        let angle = if axis_reading {
            // The spread taken as 1 − r: the same branches, up to rounding, as the curve
            // mirrored about 90° under the plane reading (the probe rows measured in
            // speedtree.md §7 before this reading).
            cl.angle().sample(t, 1.0 - rng.f()).to_radians()
        } else {
            std::f32::consts::FRAC_PI_2 - cl.angle().sample(t, rng.f()).to_radians()
        };
        azimuth += GOLDEN + 0.3 * (2.0 * rng.f() - 1.0);
        let axis = rotate(perpendicular(along), along, azimuth);
        let cdir = norm(rotate(along, axis, angle));
        grow(tree, from_axis, Branch { li: child, base: at, dir: cdir, length: clen, radius: crad }, rng, g, budget);
    }
}

/// Rings of `segments_around` (at least 3) vertices at each centre-line point, radius
/// tapered by the 6006 curve; quads between rings, wound counter-clockwise from outside.
/// The bark texture repeats `texture_repeat` times around and along.
fn tube(lv: &Level, pts: &[V], dirs: &[V], radius: f32, g: &mut Geometry) {
    let sides = lv.segments_around.clamp(3, 32) as usize;
    let first = g.positions.len() as u32;
    let [around, along] = lv.texture_repeat.map(|r| if r > 0.0 { r } else { 1.0 });
    let mut u = perpendicular(dirs[0]);
    let segs = pts.len() - 1;
    for (i, (&p, &d)) in pts.iter().zip(dirs).enumerate() {
        // Carry the frame along so the rings do not twist.
        u = norm(sub(u, scale(d, dot(u, d))));
        let v = cross(d, u);
        let s = i as f32 / segs as f32;
        let r = radius * lv.taper().y(s).max(0.0);
        for k in 0..=sides {
            let a = k as f32 / sides as f32 * std::f32::consts::TAU;
            let n = add(scale(u, a.cos()), scale(v, a.sin()));
            g.positions.push(add(p, scale(n, r)));
            g.normals.push(n);
            g.uvs.push([around * k as f32 / sides as f32, along * s]);
        }
    }
    let row = sides as u32 + 1;
    for i in 0..segs as u32 {
        for k in 0..sides as u32 {
            let a = first + i * row + k;
            let (b, c, d) = (a + 1, a + row + 1, a + row);
            g.indices.extend([a, b, c, a, c, d]);
        }
    }
}

/// How many times its parent's length level i's shortest branch must be at least for the
/// parent to count as a stub [design: a clear margin; EuonymusBush01's stems are 10 times
/// its trunk, WastelandShrub01's stems half its trunk, WhiteOak01's branches under a
/// tenth of its trunk (speedtree.md §7)].
pub const STUB_RATIO: f32 = 2.0;

/// Level i's shortest branch over its parent's longest (6004 ranges and spreads; infinite
/// under a parent of no length), or None for the trunk or a missing level.
pub fn length_ratio(tree: &Tree, i: usize) -> Option<f32> {
    let parent = tree.levels.get(i.checked_sub(1)?)?;
    let (p, c) = (parent.length(), tree.levels.get(i)?.length());
    let longest_parent = p.min.max(p.max) + p.variance.abs();
    let shortest = c.min.min(c.max) - c.variance.abs();
    Some(if longest_parent > 0.0 { shortest / longest_parent } else if shortest > 0.0 { f32::INFINITY } else { 0.0 })
}

/// Whether level `i`'s branches spring from a stub: its shortest branch is at least
/// [`STUB_RATIO`] times its parent's longest. Their 6007 angle is then measured from the
/// parent's axis, not from the plane across it (speedtree.md §4, §7) [hypothesis: fitted
/// to the three base-game trees' billboards; EuonymusBush01's stems (10-40°) on a 0.01
/// trunk stand up only so, while WhiteOak01's branches on its 0.5 trunk fit the plane
/// reading].
pub fn springs_from_stub(tree: &Tree, i: usize) -> bool {
    length_ratio(tree, i).is_some_and(|r| r >= STUB_RATIO)
}

/// Point and direction at fraction t of a centre line.
fn point_at(pts: &[V], dirs: &[V], t: f32) -> (V, V) {
    let segs = pts.len() - 1;
    let x = t.clamp(0.0, 1.0) * segs as f32;
    let i = (x.floor() as usize).min(segs - 1);
    let f = x - i as f32;
    (add(pts[i], scale(sub(pts[i + 1], pts[i]), f)), dirs[i + 1])
}

/// The lighting normals of NVForever's Physical lighting (#3827, part 3b) in place of
/// [`light_leaves`]'s: a dome around the crown's core (the leaves' box centre, lowered to 35 %
/// of its height): out from that core, with an upward part of three quarters of the leaf's
/// height above it plus a quarter of the crown's reach, so a crown
/// shades as a dense volume and its underside still sees the sky [design, chosen by eye on
/// #3828's branch; speedtree.md §6]. The CNAM dimming still darkens the crown towards its
/// bottom; this rule's upward normals keep the two from compounding into a black underside.
pub fn canopy_normals(leaves: &mut [Leaf]) {
    if leaves.is_empty() {
        return;
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for l in leaves.iter() {
        for axis in 0..3 {
            lo[axis] = lo[axis].min(l.position[axis]);
            hi[axis] = hi[axis].max(l.position[axis]);
        }
    }
    let core = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, lo[2] + 0.35 * (hi[2] - lo[2])];
    let max_r = leaves
        .iter()
        .map(|l| {
            let d = sub(l.position, core);
            dot(d, d).sqrt()
        })
        .fold(0.0f32, f32::max)
        .max(1e-4);
    for l in leaves.iter_mut() {
        let d = sub(l.position, core);
        let nz = d[2].max(0.0) * 0.75 + 0.25 * max_r;
        l.normal = norm([d[0], d[1], nz]);
    }
}

fn light_leaves(leaves: &mut [Leaf]) {
    if leaves.is_empty() {
        return;
    }
    let mut c = [0.0; 3];
    for l in leaves.iter() {
        c = add(c, l.position);
    }
    c = scale(c, 1.0 / leaves.len() as f32);
    // Outward around the crown only (no vertical part): the top-to-bottom darkening is
    // left to the CNAM dimming (Geometry::brightness), which would otherwise be counted
    // twice by down-facing normals under the crown.
    for l in leaves.iter_mut() {
        let out = sub(l.position, c);
        let out = [out[0], out[1], 0.0];
        let n = if dot(out, out) > 1e-12 { norm(out) } else { [0.0, 0.0, 0.0] };
        l.normal = norm(add(n, [0.0, 0.0, 1.0]));
    }
}

type V = [f32; 3];

fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V, s: f32) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V) -> V {
    let l = dot(a, a).sqrt();
    if l > 0.0 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0, 1.0]
    }
}
/// A unit vector perpendicular to `d`.
fn perpendicular(d: V) -> V {
    let other = if d[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    norm(cross(d, other))
}
/// `v` turned by `angle` radians about the unit `axis` (Rodrigues).
fn rotate(v: V, axis: V, angle: f32) -> V {
    let (s, c) = angle.sin_cos();
    add(add(scale(v, c), scale(cross(axis, v), s)), scale(axis, dot(axis, v) * (1.0 - c)))
}

/// SplitMix64: small, fast and fully determined by the seed.
struct Rng(u64);

impl Rng {
    fn new(seed: u32) -> Rng {
        Rng(seed as u64 ^ 0x9E37_79B9_7F4A_7C15)
    }
    /// Uniform in [0, 1).
    fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canopy_normals_point_up_and_out_from_the_lowered_core() {
        let leaf = |p: [f32; 3]| Leaf { position: p, map: 0, size: [1.0, 1.0], pivot: [0.5, 0.5], normal: [0.0; 3] };
        let mut leaves = vec![leaf([-10.0, 0.0, 0.0]), leaf([10.0, 0.0, 0.0]), leaf([0.0, 0.0, 20.0]), leaf([0.0, 0.0, 0.0])];
        canopy_normals(&mut leaves);
        for l in &leaves {
            let n = l.normal;
            assert!(((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() - 1.0).abs() < 1e-4, "unit: {n:?}");
            assert!(n[2] > 0.0, "always up: {n:?}");
        }
        assert!(leaves[0].normal[0] < 0.0 && leaves[1].normal[0] > 0.0, "out from the core");
        assert!(leaves[2].normal[2] > leaves[0].normal[2], "the top faces up more than the sides");
    }
}
