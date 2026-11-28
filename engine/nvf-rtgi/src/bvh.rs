//! Rays against triangles: the watertight test of Woop, Benthin and Wald (2013), in single
//! precision with the double-precision edge fallback (no ray slips between two triangles that
//! share an edge), and a bounding volume tree (binned SAH, the halves built in parallel).

use crate::failed::Cancel;
use crate::vec3::V3;
use std::sync::atomic::{AtomicBool, Ordering};

pub type Corners = [V3; 3];

/// From a place in a direction: distances along the ray are in units of its length, which may be anything
/// from about 1e-20 to 1e20 in a scene of the game's size.
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub(crate) o: V3,
    pub(crate) d: V3,
    pub(crate) inverse: V3,
    /// The axes it does not move along.
    pub(crate) still: [bool; 3],
    pub(crate) axes: [usize; 3],
    pub(crate) shear: V3,
}

impl Ray {
    /// Whether it starts and goes somewhere that is a number.
    fn sound(&self) -> bool {
        self.o.finite() && self.d.finite()
    }

    pub fn direction(&self) -> V3 {
        self.d
    }

    pub fn new(o: V3, d: V3) -> Ray {
        let a = d.0.map(f32::abs);
        let z = if a[0] > a[1] && a[0] > a[2] {
            0
        } else if a[1] > a[2] {
            1
        } else {
            2
        };
        let (mut x, mut y) = ((z + 1) % 3, (z + 2) % 3);
        if d[z] < 0.0 {
            std::mem::swap(&mut x, &mut y);
        }
        // Against the largest, so that a ray's length makes no difference.
        // One whose reciprocal overflows is none either.
        let still = a.map(|f| f < 1e-20 * a[z] || (1.0 / f).is_infinite());
        // For the triangles too: they and the boxes mean the same ray.
        let d = V3(std::array::from_fn(|k| if still[k] { 0.0 } else { d[k] }));
        // Never infinite.
        let inverse = V3(std::array::from_fn(|k| if still[k] { 0.0 } else { 1.0 / d[k] }));
        Ray { o, d, inverse, still, axes: [x, y, z], shear: V3::new(d[x] / d[z], d[y] / d[z], 1.0 / d[z]) }
    }
}

/// Distance and corner weights where the ray's line meets the triangle.
pub(crate) fn meet(r: &Ray, tri: &Corners) -> Option<(f32, [f32; 3])> {
    let [x, y, z] = r.axes;
    let p = tri.map(|c| {
        let v = c - r.o;
        V3::new(v[x] - r.shear[0] * v[z], v[y] - r.shear[1] * v[z], r.shear[2] * v[z])
    });
    let edge = |a: V3, b: V3| a[0] * b[1] - a[1] * b[0];
    let mut w = [edge(p[2], p[1]), edge(p[0], p[2]), edge(p[1], p[0])];
    if w.contains(&0.0) {
        let fine = |a: V3, b: V3| (f64::from(a[0]) * f64::from(b[1]) - f64::from(a[1]) * f64::from(b[0])) as f32;
        w = [fine(p[2], p[1]), fine(p[0], p[2]), fine(p[1], p[0])];
    }
    if w.iter().any(|f| *f < 0.0) && w.iter().any(|f| *f > 0.0) {
        return None;
    }
    let det = w[0] + w[1] + w[2];
    let t = (w[0] * p[0][2] + w[1] * p[1][2] + w[2] * p[2][2]) / det;
    (det != 0.0 && t.is_finite()).then(|| (t, w.map(|f| f / det)))
}

type D = [f64; 3];

fn sub(x: D, y: D) -> D {
    std::array::from_fn(|k| x[k] - y[k])
}

fn dot(x: D, y: D) -> f64 {
    x[0] * y[0] + x[1] * y[1] + x[2] * y[2]
}

fn cross(x: D, y: D) -> D {
    [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]]
}

fn length(x: D) -> f64 {
    dot(x, x).sqrt()
}

/// Many times what one rounding of a double can do: the room every bound below leaves for its own arithmetic.
const ROUNDS: f64 = 64.0 * f64::EPSILON;

/// How far `p` is from the box `min`..`max`, in double precision.
fn off_box(p: D, min: D, max: D) -> f64 {
    length(std::array::from_fn(|k| (min[k] - p[k]).max(p[k] - max[k]).max(0.0)))
}

/// Whether a triangle may be no further from `p` than `far` (not below 0), in double precision and conservative:
/// it never says no of one within `far`, and says yes of one further by no more than its width across its longest
/// side (the third corner's distance from that side) and a rounding. The triangle's box has to be within reach, and
/// so does the longest side less that width: all of the triangle is within the width of that side. Then a side
/// within reach is enough, or else the plane where `p` is not clearly outside one of the sides, each with the room
/// its roundings need. A triangle whose normal is lost in the rounding (it is all but a line) is near then too.
fn near(p: V3, tri: &Corners, far: f64) -> bool {
    let (p, [a, b, c]) = (p.0.map(f64::from), tri.map(|v| v.0.map(f64::from)));
    let size = [p, a, b, c].iter().flatten().fold(0.0f64, |m, x| m.max(x.abs()));
    let reach = far + ROUNDS * (size + far);
    let (lo, hi): (D, D) = (std::array::from_fn(|k| a[k].min(b[k]).min(c[k])), std::array::from_fn(|k| a[k].max(b[k]).max(c[k])));
    if off_box(p, lo, hi) > reach {
        return false;
    }
    let sides = [(a, b), (b, c), (c, a)];
    let from = |q: D, (x, y): &(D, D)| {
        let (e, w) = (sub(*y, *x), sub(q, *x));
        let t = if dot(e, e) > 0.0 { (dot(w, e) / dot(e, e)).clamp(0.0, 1.0) } else { 0.0 };
        length(std::array::from_fn(|k| w[k] - e[k] * t))
    };
    let side = |s: &(D, D)| from(p, s);
    // Every place of the triangle is within `width` of its longest side, so p is no nearer to it than this.
    let longest = (0..3).max_by(|i, j| length(sub(sides[*i].1, sides[*i].0)).total_cmp(&length(sub(sides[*j].1, sides[*j].0)))).unwrap_or(0);
    let width = from([c, a, b][longest], &sides[longest]);
    if side(&sides[longest]) - width > reach + ROUNDS * size {
        return false;
    }
    if sides.iter().any(|s| side(s) <= reach) {
        return true;
    }
    let (e1, e2) = (sub(b, a), sub(c, a));
    let (n, long) = (cross(e1, e2), length(e1) * length(e2));
    let normal = length(n);
    if normal <= ROUNDS * long {
        return true;
    }
    let to = sub(p, a);
    let plane = dot(to, n).abs() / normal;
    if plane > reach + ROUNDS * length(to) * (1.0 + long / normal) {
        return false;
    }
    sides.iter().all(|(x, y)| {
        let (e, w) = (sub(*y, *x), sub(p, *x));
        dot(cross(e, w), n) >= -ROUNDS * length(e) * length(w) * (normal + long)
    })
}

/// An inner node's left child follows it and `a` is its right one (`count` 0); a leaf holds
/// `count` triangles from slot `a`.
struct Node {
    min: V3,
    a: u32,
    max: V3,
    count: u32,
}

impl Node {
    /// Where the ray enters the box, infinite when it misses it; widened by the rounding.
    fn enter(&self, r: &Ray, near: f32, far: f32) -> f32 {
        let (mut near, mut far) = (near, far);
        for k in 0..3 {
            if r.still[k] {
                // All of it is in the closed slab, or nothing.
                if !(self.min[k]..=self.max[k]).contains(&r.o[k]) {
                    return f32::INFINITY;
                }
                continue;
            }
            let (a, b) = ((self.min[k] - r.o[k]) * r.inverse[k], (self.max[k] - r.o[k]) * r.inverse[k]);
            near = near.max(a.min(b));
            far = far.min(a.max(b) * (1.0 + 6e-7));
        }
        if near <= far {
            near
        } else {
            f32::INFINITY
        }
    }
}

/// A candidate as the policies see it.
#[derive(Clone, Copy, Debug)]
pub struct Meet {
    pub tri: u32,
    slot: u32,
    pub t: f32,
    pub weights: [f32; 3],
}

/// Of a node.
pub const NODE_BYTES: usize = size_of::<Node>();

const BINS: usize = 16;
/// Below this depth of the tree the halves are by count: the stack below is deep enough.
const SAH_DEPTH: u32 = 48;
const STACK: usize = 96;
/// Boxes are kept this much (relative) past the nearest hit, for the rounding.
const ROUNDING: f32 = 4e-6;

pub struct Bvh {
    nodes: Vec<Node>,
    /// In the leaves' order, and which triangle each was.
    tris: Vec<Corners>,
    ids: Vec<u32>,
}

struct Builder<'a> {
    boxes: &'a [(V3, V3)],
    cancel: Cancel<'a>,
    stopped: AtomicBool,
}

/// Nodes of as many triangles ask whether to go on, and may have a child built by another thread.
const LARGE: usize = 4096;

type Bin = (V3, V3, usize);
const EMPTY: Bin = (V3([f32::INFINITY; 3]), V3([f32::NEG_INFINITY; 3]), 0);

fn join(a: Bin, b: Bin) -> Bin {
    (a.0.min(b.0), a.1.max(b.1), a.2 + b.2)
}

fn area(b: Bin) -> f32 {
    let d = b.1 - b.0;
    d[0] * d[1] + d[1] * d[2] + d[2] * d[0]
}

impl Builder<'_> {
    fn centre(&self, i: u32) -> V3 {
        let (lo, hi) = self.boxes[i as usize];
        lo * 0.5 + hi * 0.5
    }

    /// Whether to stop, asked at work on `n` triangles when they are many.
    fn halted(&self, n: usize) -> bool {
        let halted = n >= LARGE && (self.stopped.load(Ordering::Relaxed) || (self.cancel)());
        if halted {
            self.stopped.store(true, Ordering::Relaxed);
        }
        halted
    }

    /// The bin of a centre along an axis.
    fn bin(c: f32, lo: f32, hi: f32) -> usize {
        (((c - lo) / (hi - lo) * BINS as f32) as usize).min(BINS - 1)
    }

    /// How many of `refs` (put first) make the left child; 0 for a leaf.
    fn split(&self, refs: &mut [u32], whole: Bin, depth: u32) -> usize {
        let n = refs.len();
        if n <= 2 {
            return 0;
        }
        let half = if n <= 8 { 0 } else { n / 2 };
        let (lo, hi) = refs.iter().fold((EMPTY.0, EMPTY.1), |(lo, hi), i| (lo.min(self.centre(*i)), hi.max(self.centre(*i))));
        if depth >= SAH_DEPTH {
            return half;
        }
        let mut best = (if n <= 8 { (n as f32 - 1.0) * area(whole) } else { f32::INFINITY }, 0, 0);
        for k in (0..3).filter(|k| hi[*k] > lo[*k]) {
            if self.halted(n) {
                return 0;
            }
            let mut bins = [EMPTY; BINS];
            for i in refs.iter() {
                let b = &mut bins[Self::bin(self.centre(*i)[k], lo[k], hi[k])];
                let (min, max) = self.boxes[*i as usize];
                *b = join(*b, (min, max, 1));
            }
            let mut right = [EMPTY; BINS + 1];
            for i in (0..BINS).rev() {
                right[i] = join(right[i + 1], bins[i]);
            }
            let mut left = EMPTY;
            for (i, right) in right.iter().enumerate().take(BINS).skip(1) {
                left = join(left, bins[i - 1]);
                if left.2 > 0 && right.2 > 0 {
                    let cost = area(left) * left.2 as f32 + area(*right) * right.2 as f32;
                    if cost < best.0 {
                        best = (cost, k, i);
                    }
                }
            }
        }
        let (_, k, at) = best;
        if at == 0 {
            return half;
        }
        let mut mid = 0;
        for i in 0..n {
            if Self::bin(self.centre(refs[i])[k], lo[k], hi[k]) < at {
                refs.swap(i, mid);
                mid += 1;
            }
        }
        mid
    }

    /// The tree over `refs`, whose leaves start at slot `first`, appended to `nodes`. The same
    /// nodes in the same order whatever `forks` (levels that use a thread for each child).
    fn build(&self, nodes: &mut Vec<Node>, refs: &mut [u32], first: u32, depth: u32, forks: u32) {
        if self.halted(refs.len()) {
            return;
        }
        let whole = refs.iter().fold(EMPTY, |b, i| join(b, (self.boxes[*i as usize].0, self.boxes[*i as usize].1, 1)));
        let at = nodes.len();
        nodes.push(Node { min: whole.0, max: whole.1, a: first, count: refs.len() as u32 });
        let mid = self.split(refs, whole, depth);
        if mid == 0 {
            return;
        }
        let (l, r) = refs.split_at_mut(mid);
        let second = first + mid as u32;
        if forks == 0 || l.len().min(r.len()) < LARGE {
            self.build(nodes, l, first, depth + 1, forks);
            nodes[at].a = nodes.len() as u32;
            self.build(nodes, r, second, depth + 1, forks);
        } else {
            let part = |refs: &mut [u32], first| {
                let mut out = Vec::new();
                self.build(&mut out, refs, first, depth + 1, forks - 1);
                out
            };
            let (left, right) = std::thread::scope(|s| {
                let other = std::thread::Builder::new().spawn_scoped(s, || part(l, first));
                let right = part(r, second);
                (other.ok().map(|t| t.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))), right)
            });
            // Without another thread it is this one's.
            let left = left.unwrap_or_else(|| part(l, first));
            for (i, child) in [left, right].into_iter().enumerate() {
                let base = nodes.len() as u32;
                if i == 1 {
                    nodes[at].a = base;
                }
                nodes.extend(child.into_iter().map(|n| Node { a: if n.count == 0 { n.a + base } else { n.a }, ..n }));
            }
        }
        nodes[at].count = 0;
    }
}

impl Bvh {
    /// `None` when `cancel` says so. There are fewer than 2^31 triangles.
    pub fn build(tris: &[Corners], threads: usize, cancel: Cancel) -> Option<Bvh> {
        let boxes: Vec<(V3, V3)> = tris.iter().map(|t| (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]))).collect();
        let mut ids: Vec<u32> = (0..tris.len() as u32).collect();
        let mut nodes = Vec::new();
        if !ids.is_empty() {
            let builder = Builder { boxes: &boxes, cancel, stopped: AtomicBool::new(false) };
            builder.build(&mut nodes, &mut ids, 0, 0, threads.max(1).ilog2());
            if builder.stopped.into_inner() {
                return None;
            }
        }
        Some(Bvh { nodes, tris: ids.iter().map(|i| tris[*i as usize]).collect(), ids })
    }

    /// The corners of a candidate's triangle.
    pub fn corners(&self, m: &Meet) -> &Corners {
        &self.tris[m.slot as usize]
    }

    /// The box about all triangles, when there are any.
    pub fn bounds(&self) -> Option<(V3, V3)> {
        let out = self.tris.iter().flatten().fold((EMPTY.0, EMPTY.1), |b, p| (b.0.min(*p), b.1.max(*p)));
        (!self.tris.is_empty()).then_some(out)
    }

    /// The corners by the triangles' numbers.
    pub fn in_order(&self) -> Vec<Corners> {
        let mut out = vec![[V3::ZERO; 3]; self.tris.len()];
        for (tri, id) in self.tris.iter().zip(&self.ids) {
            out[*id as usize] = *tri;
        }
        out
    }

    /// The triangles no further from `p` than `far`, in order, with their leaves' boxes. Conservative: it never leaves
    /// out one within `far`, and may name one beyond it by no more than its width across its longest side and a small
    /// allowance for rounding ([`near`]). None for a `far` below 0.
    pub fn within(&self, p: V3, far: f32) -> Vec<(u32, V3, V3)> {
        let (mut out, mut open) = (Vec::new(), Vec::new());
        if far.is_nan() || far < 0.0 {
            return out;
        }
        let (at, far) = (p.0.map(f64::from), f64::from(far));
        open.extend((!self.nodes.is_empty()).then_some(0usize));
        while let Some(i) = open.pop() {
            let n = &self.nodes[i];
            let (min, max) = (n.min.0.map(f64::from), n.max.0.map(f64::from));
            let size = [at, min, max].iter().flatten().fold(0.0f64, |m, x| m.max(x.abs()));
            if off_box(at, min, max) > far + ROUNDS * (size + far) {
                continue;
            }
            if n.count == 0 {
                open.extend([i + 1, n.a as usize]);
                continue;
            }
            for slot in n.a as usize..(n.a + n.count) as usize {
                if near(p, &self.tris[slot], far) {
                    out.push((self.ids[slot], n.min, n.max));
                }
            }
        }
        out.sort_unstable_by_key(|n| n.0);
        out
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Nodes, triangle corners and ids, as they are held.
    pub fn bytes(&self) -> usize {
        self.nodes.capacity() * size_of::<Node>() + self.tris.capacity() * size_of::<Corners>() + self.ids.capacity() * size_of::<u32>()
    }

    /// Every candidate of the leaves the ray crosses nearer than `limit`, the nearer boxes
    /// first; `visit` gives the new limit.
    fn walk(&self, r: &Ray, near: f32, mut limit: f32, mut visit: impl FnMut(Meet, f32) -> f32) {
        if !r.sound() {
            return;
        }
        let mut stack = [(0u32, 0f32); STACK];
        let mut top = 0;
        match self.nodes.first() {
            Some(n) if n.enter(r, near, limit) < f32::INFINITY => {}
            _ => return,
        }
        let mut at = 0;
        loop {
            let n = &self.nodes[at as usize];
            if n.count > 0 {
                for slot in n.a..n.a + n.count {
                    if let Some((t, weights)) = meet(r, &self.tris[slot as usize]) {
                        if t > near && t <= limit {
                            limit = visit(Meet { tri: self.ids[slot as usize], slot, t, weights }, limit);
                        }
                    }
                }
            } else {
                let mut a = (at + 1, self.nodes[at as usize + 1].enter(r, near, limit));
                let mut b = (n.a, self.nodes[n.a as usize].enter(r, near, limit));
                if b.1 < a.1 {
                    std::mem::swap(&mut a, &mut b);
                }
                if b.1 < f32::INFINITY {
                    stack[top] = b;
                    top += 1;
                }
                if a.1 < f32::INFINITY {
                    at = a.0;
                    continue;
                }
            }
            loop {
                if top == 0 {
                    return;
                }
                top -= 1;
                if stack[top].1 <= limit {
                    at = stack[top].0;
                    break;
                }
            }
        }
    }

    /// The nearest accepted candidate in (near, far], the lower triangle's of equal ones; the ray
    /// passes through those that `stops` does not accept.
    pub fn closest(&self, r: &Ray, near: f32, far: f32, mut stops: impl FnMut(&Meet) -> bool) -> Option<Meet> {
        let mut best: Option<Meet> = None;
        self.walk(r, near, far, |m, limit| {
            if stops(&m) && best.is_none_or(|held| m.t < held.t || (m.t == held.t && m.tri < held.tri)) {
                best = Some(m);
                limit.min(m.t * (1.0 + ROUNDING))
            } else {
                limit
            }
        });
        best
    }

    /// Whether an accepted candidate lies in (near, far).
    pub fn any(&self, r: &Ray, near: f32, far: f32, mut stops: impl FnMut(&Meet) -> bool) -> bool {
        let mut found = false;
        // A limit below every distance ends the walk.
        self.walk(r, near, far, |m, limit| {
            found = found || (m.t < far && stops(&m));
            if found {
                f32::NEG_INFINITY
            } else {
                limit
            }
        });
        found
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::vec3::tests::Rng;

    fn built(tris: &[Corners], threads: usize) -> Bvh {
        Bvh::build(tris, threads, &|| false).unwrap()
    }

    /// A thin triangle right below a point is near it: in single precision its regions are rounded away.
    #[test]
    fn thin_triangles_are_near() {
        let one = |tri: Corners, p: V3, far: f32| built(&[tri], 1).within(p, far).len() == 1;
        let tri = [V3::ZERO, V3::new(65536.0, 0.0, 0.0), V3::new(65536.0, 2.0, 0.0)];
        let p = V3::new(49152.0, 1.0, 0.01);
        assert!(one(tri, p, 0.5) && one(tri, p, 0.0101) && !one(tri, p, 0.005) && !one(tri, V3::new(49152.0, 1.6, 0.0), 0.05));
        // Beside a side, past a corner, and of a triangle that is a line.
        assert!(one(tri, V3::new(1.0, -1.0, 0.0), 1.0) && !one(tri, V3::new(1.0, -1.0, 0.0), 0.99));
        assert!(one(tri, V3::new(-3.0, -4.0, 0.0), 5.0) && !one(tri, V3::new(-3.0, -4.0, 0.0), 4.99));
        let line = [V3::ZERO, V3::new(2.0, 0.0, 0.0), V3::new(4.0, 0.0, 0.0)];
        assert!(one(line, V3::new(3.0, 0.0, 2.0), 2.0) && !one(line, V3::new(3.0, 0.0, 2.0), 1.99));
        // Exactly at the radius, from a side and from the plane of two long sides that are all but parallel, in
        // either order of the corners.
        let tri = [V3::ZERO, V3::new(3.0, 4.0, 0.0), V3::new(0.0, 4.0, 0.0)];
        assert!(one(tri, V3::new(2.5, 2.5, 0.0), 0.5) && !one(tri, V3::new(2.5, 2.5, 0.0), 0.4999));
        let [a, b, c] = [V3::new(65535.0, -49150.625, 65535.0), V3::new(0.0, 0.625, 0.0), V3::new(0.0, 0.625, -1.0 + 2f32.powi(-20))];
        for tri in [[a, b, c], [b, c, a], [c, a, b]] {
            assert!(one(tri, V3::ZERO, 0.5) && !one(tri, V3::ZERO, 0.49), "{tri:?}");
        }
        // Where a box's squared distance is past f32's end; no radius below 0.
        let wall = [V3::new(0.0, -1.0, -1.0), V3::new(0.0, 1.0, -1.0), V3::new(0.0, 0.0, 1.0)];
        let far = 2f32.powi(64);
        assert!(one(wall, V3::new(far, 0.0, 0.0), far) && !one(wall, V3::new(far, 0.0, 0.0), far / 2.0));
        let aside = V3::new(1e5, 0.0, 0.0);
        let (there, on) = (wall.map(|c| c + aside), V3::new(0.0, 0.0, 0.5) + aside);
        for far in [-0.00001, -1.0, f32::NAN] {
            assert!(!one(there, on, far), "{far}");
        }
        assert!(one(there, on, 0.0), "on it at a radius of 0");
        // A sliver far off, all but in line with its longest side: neither its lost normal nor its plane's room for
        // the rounding names it.
        for h in [2f32.powi(-35), 2f32.powi(-33)] {
            let sliver = [V3::ZERO, V3::new(10000.0, 10000.0, 0.0), V3::new(5000.0, 5000.0, h)];
            for far in [0.02, 0.5] {
                assert!(!one(sliver, V3::new(0.0, 10000.0, 0.0), far), "{h} {far}");
            }
            assert!(one(sliver, V3::new(5000.0, 5000.0, 0.01), 0.02), "{h}: beside it");
        }
    }

    /// A random unit vector.
    pub fn direction(rng: &mut Rng) -> V3 {
        loop {
            let d = V3(std::array::from_fn(|_| rng.draw() * 2.0 - 1.0));
            if let Some(d) = d.unit().filter(|_| d.length() < 1.0) {
                return d;
            }
        }
    }

    /// `n` triangles in a box of 200 about (5000, -3000, 900), some of them large, some flat.
    fn random(n: usize, rng: &mut Rng) -> Vec<Corners> {
        let middle = V3::new(5000.0, -3000.0, 900.0);
        (0..n)
            .map(|i| {
                let c = middle + V3(std::array::from_fn(|_| rng.draw() * 200.0 - 100.0));
                let size = if i.is_multiple_of(50) { 80.0 } else { 8.0 };
                let mut t: Corners = std::array::from_fn(|_| c + direction(rng) * size);
                if i.is_multiple_of(7) {
                    t = t.map(|p| V3::new(p[0], c[1], p[2]));
                }
                t
            })
            .collect()
    }

    fn brute(tris: &[Corners], r: &Ray, far: f32) -> Option<(f32, u32)> {
        let hits = tris.iter().enumerate().filter_map(|(i, t)| meet(r, t).map(|(t, _)| (t, i as u32)));
        hits.filter(|(t, _)| *t > 0.0 && *t <= far).min_by(|a, b| a.0.total_cmp(&b.0))
    }

    #[test]
    fn the_tree_finds_what_brute_force_finds() {
        let mut rng = Rng(1);
        for n in [1, 3, 40, 900] {
            let tris = random(n, &mut rng);
            let bvh = built(&tris, 4);
            let (mut hits, mut blocked) = (0, 0);
            for i in 0..4000 {
                let o = V3::new(5000.0, -3000.0, 900.0) + direction(&mut rng) * 150.0;
                // Some rays along an axis, some at a triangle.
                let aim = tris[i % n][0] * 0.3 + tris[i % n][1] * 0.3 + tris[i % n][2] * 0.4;
                let d = match i % 3 {
                    0 => V3(std::array::from_fn(|k| if k == i % 9 / 3 { 1.0 } else { 0.0 })),
                    1 => aim - o,
                    _ => direction(&mut rng),
                };
                let r = Ray::new(o, d);
                let (found, want) = (bvh.closest(&r, 0.0, f32::INFINITY, |_| true), brute(&tris, &r, f32::INFINITY));
                assert_eq!(found.is_some(), want.is_some());
                if let (Some(m), Some((t, _))) = (found, want) {
                    assert!((m.t - t).abs() <= 1e-5 * t, "{} {t}", m.t);
                    hits += 1;
                }
                // A finite segment: all but the hits just past its end.
                let far = rng.draw() * 300.0 / d.length();
                let want = brute(&tris, &r, far);
                assert_eq!(bvh.any(&r, 0.0, far, |_| true), want.is_some());
                blocked += usize::from(want.is_some());
            }
            assert!(hits > 1000 && (n < 40 || blocked > 100), "{n}: {hits} {blocked}");
        }
    }

    /// Rays along an axis that lie on the boxes' faces, by brute force: triangles and starts are whole numbers,
    /// so many rays meet an edge or a corner, where a box ends too.
    #[test]
    fn rays_on_faces() {
        let mut rng = Rng(9);
        let mut whole = |n: f32| V3(std::array::from_fn(|_| (rng.draw() * n).floor()));
        let (mut hits, mut blocked) = (0, 0);
        for n in [1, 2, 7, 60] {
            let tris: Vec<Corners> = (0..n).map(|_| std::array::from_fn(|_| whole(6.0))).collect();
            let bvh = built(&tris, 2);
            for i in 0..4000 {
                let axis = V3(std::array::from_fn(|k| if k == i % 3 { 1.0 } else { 0.0 }));
                // The zeros of the second are negative.
                let d = if i % 2 == 0 { axis } else { -axis };
                let o = whole(6.0) - d * 10.0;
                let (r, far) = (Ray::new(o, d), 1.0 + whole(20.0)[0]);
                let (found, want) = (bvh.closest(&r, 0.0, f32::INFINITY, |_| true), brute(&tris, &r, f32::INFINITY));
                assert_eq!(found.map(|m| m.t), want.map(|w| w.0), "{o:?} {d:?}");
                assert_eq!(bvh.any(&r, 0.0, far, |_| true), brute(&tris, &r, far).is_some_and(|w| w.0 < far));
                hits += usize::from(want.is_some());
                blocked += usize::from(brute(&tris, &r, far).is_some());
            }
        }
        assert!(hits > 1000 && blocked > 500, "{hits} {blocked}");
        // By hand: the edge at the least and at the greatest x of one triangle.
        let t = [V3::new(-1.0, -1.0, 0.0), V3::new(1.0, -1.0, 0.0), V3::new(1.0, 1.0, 0.0)];
        let bvh = built(&[t], 1);
        for (x, y) in [(1.0, 0.5), (1.0, 1.0), (-1.0, -1.0), (0.0, -1.0)] {
            let r = Ray::new(V3::new(x, y, 5.0), V3::new(0.0, 0.0, -1.0));
            assert!(bvh.any(&r, 0.0, 9.0, |_| true), "{x} {y}");
        }
        assert!(!bvh.any(&Ray::new(V3::new(1.001, 0.0, 5.0), V3::new(0.0, 0.0, -1.0)), 0.0, 9.0, |_| true));
    }

    /// The same hit whatever the direction's length, at a fraction of it.
    #[test]
    fn the_length_makes_no_difference() {
        let t = [V3::new(-9.0, -9.0, 0.0), V3::new(9.0, -9.0, 0.0), V3::new(0.0, 9.0, 0.0)];
        let bvh = built(&[t], 1);
        for d in [V3::new(0.0, 0.0, -1.0), V3::new(0.3, -0.2, -1.0)] {
            for scale in [1e-30f32, 1e-21, 1e-6, 1.0, 1e9, 1e30] {
                let m = bvh.closest(&Ray::new(V3::new(0.1, 0.2, 1.0), d * scale), 0.0, f32::INFINITY, |_| true);
                assert!(m.is_some_and(|m| (m.t * scale - 1.0).abs() < 1e-5), "{d:?} {scale}");
            }
        }
    }

    /// A component that counts as none is none to the box and to the triangle alike.
    #[test]
    fn still_axes_agree() {
        // In the plane z = 0 that the rays almost run in, and across them.
        let along = [V3::new(250.0, 150.0, 0.0), V3::new(300.0, 200.0, 0.0), V3::new(250.0, 200.0, 0.0)];
        let across = [V3::new(250.0, 150.0, -9.0), V3::new(250.0, 250.0, -9.0), V3::new(250.0, 200.0, 20.0)];
        for (t, hits) in [(along, 0), (across, 32)] {
            let (bvh, mut found) = (built(&[t], 1), 0);
            for y in [0.0, -3.063_896e-8] {
                for z in [0.0, 1.101_741_5e-21, 1e-22, -1e-24] {
                    for at in [-8.839_944e-20, -1e-19, 1e-30, 0.0] {
                        let r = Ray::new(V3::new(330.236_1, 200.0, at), V3::new(-1.0, y, z));
                        let m = bvh.closest(&r, 0.0, f32::INFINITY, |_| true).map(|m| m.t);
                        assert_eq!(m, meet(&r, &t).map(|m| m.0), "{y} {z} {at}");
                        found += usize::from(m.is_some());
                    }
                }
            }
            assert_eq!(found, hits);
        }
    }

    /// A component whose reciprocal overflows, beside one that is not much larger.
    #[test]
    fn reciprocals_that_overflow() {
        let t = [V3::ZERO, V3::new(1.0, 0.0, 0.0), V3::new(0.0, 1.0, 0.0)];
        let bvh = built(&[t], 1);
        for x in [1e-39, -1e-39, 1e-45, 0.0] {
            let r = Ray::new(V3::new(0.0, 0.25, 1.0), V3::new(x, 0.0, -1e-20));
            let m = bvh.closest(&r, 0.0, f32::INFINITY, |_| true);
            assert!(m.is_some_and(|m| (m.t * 1e-20 - 1.0).abs() < 1e-5), "{x}");
        }
    }

    /// A closed room far from the origin, its sides of many triangles: no ray from its inside slips out where
    /// they meet.
    #[test]
    fn no_ray_slips_out() {
        const N: usize = 8;
        let mut rng = Rng(11);
        for (middle, size) in [(V3::ZERO, 37.3), (V3::new(7000.0, 2000.0, -4000.0), 211.7), (V3::new(-61_000.0, 8000.0, 52_000.0), 400.0)] {
            let at = |k: usize, side: f32, i: usize, j: usize| {
                let mut p = [0.0; 3];
                p[k] = side;
                p[(k + 1) % 3] = i as f32 / N as f32 * 2.0 - 1.0;
                p[(k + 2) % 3] = j as f32 / N as f32 * 2.0 - 1.0;
                middle + V3(p) * size
            };
            let mut tris: Vec<Corners> = Vec::new();
            for k in 0..3 {
                for side in [-1.0, 1.0] {
                    for (i, j) in (0..N * N).map(|n| (n / N, n % N)) {
                        let c = [at(k, side, i, j), at(k, side, i + 1, j), at(k, side, i + 1, j + 1), at(k, side, i, j + 1)];
                        tris.extend([[c[0], c[1], c[2]], [c[0], c[2], c[3]]]);
                    }
                }
            }
            let bvh = built(&tris, 1);
            for n in 0..20_000 {
                let t = tris[n % tris.len()];
                let o = middle + V3(std::array::from_fn(|_| (rng.draw() * 1.8 - 0.9) * size));
                // At a corner, or at a place on an edge.
                let w = if n % 2 == 0 { 0.0 } else { rng.draw() };
                let aim = t[n % 3] * (1.0 - w) + t[(n + 1) % 3] * w;
                let d = (aim - o) * [1.0, 1e-4, 300.0][n % 5 % 3];
                let r = Ray::new(o, d);
                let (found, want) = (bvh.closest(&r, 0.0, f32::INFINITY, |_| true), brute(&tris, &r, f32::INFINITY));
                assert!(found.is_some(), "{o:?} {d:?}");
                assert_eq!(found.map(|m| m.t), want.map(|w| w.0), "{o:?} {d:?}");
            }
        }
    }

    /// The walk's stack holds the deepest tree: halves by count below the binned ones.
    #[test]
    fn the_stack_is_deep_enough() {
        // 2^26 triangles, the most there may be, halved down to the leaves.
        let halves = (1..).take_while(|k| (1usize << 26) >> k > 4).count();
        assert!(SAH_DEPTH as usize + halves < STACK, "{halves}");
        // Sizes that grow many times over: the bins split a few off at a time.
        let far = |i: usize| 2f32.powi(i as i32 % 120 - 60);
        let tris: Vec<Corners> = (0..20_000).map(|i| [V3::new(far(i), 0.0, 0.0), V3::new(far(i), far(i), 0.0), V3::new(far(i), 0.0, far(i))]).collect();
        let bvh = built(&tris, 1);
        let (mut deepest, mut open) = (0, vec![(0usize, 1usize)]);
        while let Some((at, depth)) = open.pop() {
            deepest = deepest.max(depth);
            let n = &bvh.nodes[at];
            if n.count == 0 {
                open.extend([(at + 1, depth + 1), (n.a as usize, depth + 1)]);
            }
        }
        assert!((20..STACK).contains(&deepest), "{deepest}");
    }

    /// A ray that starts or goes nowhere meets nothing.
    #[test]
    fn rays_from_nowhere() {
        let t = [V3::new(-9.0, -9.0, 0.0), V3::new(9.0, -9.0, 0.0), V3::new(0.0, 9.0, 0.0)];
        let bvh = built(&[t], 1);
        for wrong in [f32::NAN, f32::INFINITY] {
            for (o, d) in [(V3::new(wrong, 0.0, 1.0), V3::new(0.0, 0.0, -1.0)), (V3::new(0.0, 0.0, 1.0), V3::new(wrong, 0.0, -1.0))] {
                let r = Ray::new(o, d);
                assert!(!r.sound() && !bvh.any(&r, 0.0, f32::INFINITY, |_| true));
            }
        }
    }

    /// The same tree from one thread and from many.
    #[test]
    fn the_tree_does_not_depend_on_the_threads() {
        let tris = random(30_000, &mut Rng(2));
        let (a, b) = (built(&tris, 1), built(&tris, 16));
        assert_eq!(a.ids, b.ids);
        let flat = |t: &Bvh| t.nodes.iter().map(|n| (n.a, n.count, n.min, n.max)).collect::<Vec<_>>();
        assert!(flat(&a) == flat(&b));
        assert!(a.nodes.len() > 7_000 && a.bytes() > 30_000 * 40);
        // It asks at the large nodes alone, and stops there.
        let asked = std::sync::atomic::AtomicUsize::new(0);
        let late = || asked.fetch_add(1, Ordering::Relaxed) >= 3;
        for threads in [1, 16] {
            asked.store(0, Ordering::Relaxed);
            assert!(Bvh::build(&tris, threads, &late).is_none() && asked.load(Ordering::Relaxed) < 100);
        }
        assert!(Bvh::build(&tris, 4, &|| true).is_none() && Bvh::build(&tris[..LARGE - 1], 4, &|| true).is_some());
    }

    /// Many equal boxes: split by count, and the first triangle wins.
    #[test]
    fn equal_triangles() {
        let t = [V3::ZERO, V3::new(1.0, 0.0, 0.0), V3::new(0.0, 1.0, 0.0)];
        let bvh = built(&vec![t; 1000], 4);
        let r = Ray::new(V3::new(0.2, 0.2, 5.0), V3::new(0.0, 0.0, -1.0));
        let m = bvh.closest(&r, 0.0, f32::INFINITY, |_| true).unwrap();
        assert!(m.tri == 0 && (m.t - 5.0).abs() <= 1e-5 * 5.0, "{} {}", m.tri, m.t);
        let off = bvh.closest(&r, 0.0, f32::INFINITY, |m| m.tri > 700).unwrap();
        assert_eq!(off.tri, 701);
        assert!(built(&[], 4).closest(&r, 0.0, 9.0, |_| true).is_none());
    }

    #[test]
    fn segments_end() {
        let t = [V3::new(-9.0, -9.0, 0.0), V3::new(9.0, -9.0, 0.0), V3::new(0.0, 9.0, 0.0)];
        let bvh = built(&[t], 1);
        let r = Ray::new(V3::new(0.0, 0.0, 5.0), V3::new(0.0, 0.0, -2.0));
        assert!(bvh.any(&r, 0.0, 2.6, |_| true) && !bvh.any(&r, 0.0, 2.4, |_| true));
        assert!(!bvh.any(&r, 2.6, 9.0, |_| true) && !bvh.any(&r, 0.0, 2.6, |_| false));
        assert!(!bvh.any(&Ray::new(V3::new(0.0, 0.0, -1.0), r.d), 0.0, 9.0, |_| true), "behind it");
    }
}
