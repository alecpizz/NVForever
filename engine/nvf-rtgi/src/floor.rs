//! The floor of a cell that can be walked on, and the places above it.

use crate::failed::{go_on, guarded, unavailable, Cancel, Failed, Reason};
use crate::vec3::V3;

/// How far above the floor the eye is.
pub const EYE: f32 = 112.0;

/// A room of a cell, as a box of its own axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub center: V3,
    /// Unit.
    pub axes: [V3; 3],
    /// Along them.
    pub half: [f32; 3],
}

/// What a floor may be, looked at before any of it is ordered. The defaults are a budget for play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloorLimits {
    pub triangles: usize,
    pub rooms: usize,
}

impl Default for FloorLimits {
    fn default() -> FloorLimits {
        FloorLimits { triangles: 1 << 16, rooms: 1 << 10 }
    }
}

/// Triangles that are held against every room between two looks at `cancel`.
const PLACED: usize = 1 << 10;

/// The smallest room that holds `p`. One whose size is no finite number holds nothing; one that is flat holds what is
/// exactly in its plane, and is the smallest there.
fn room(rooms: &[Room], p: V3) -> Option<usize> {
    let holds = |r: &Room| (0..3).all(|k| (p - r.center).dot(r.axes[k]).abs() <= r.half[k]);
    let size = |i: &usize| rooms[*i].half.iter().product::<f32>();
    (0..rooms.len()).filter(|i| size(i).is_finite() && holds(&rooms[*i])).min_by(|a, b| size(a).total_cmp(&size(b)))
}

fn middle(t: &[V3; 3]) -> V3 {
    (t[0] + t[1] + t[2]) * (1.0 / 3.0)
}

fn area(t: &[V3; 3]) -> f32 {
    0.5 * (t[1] - t[0]).cross(t[2] - t[0]).length()
}

/// The triangles that can be walked on, in the order of room, storey (256 units of height), strip (512
/// units) and place: what is near by area is near by place.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Floor {
    triangles: Vec<[V3; 3]>,
    /// The area up to and with each.
    until: Vec<f64>,
}

impl Floor {
    /// Of a navmesh's triangles, those whose area is more than 0 and whose middle is finite, as both are in `f32`.
    /// `cancel` is asked every 1,024 triangles, not while they are put in order.
    pub fn new(triangles: &[[V3; 3]], rooms: &[Room], limits: &FloorLimits, cancel: Cancel) -> Result<Floor, Failed> {
        guarded(|| Floor::of(triangles, rooms, limits, cancel))
    }

    fn of(triangles: &[[V3; 3]], rooms: &[Room], limits: &FloorLimits, cancel: Cancel) -> Result<Floor, Failed> {
        if triangles.len() > limits.triangles {
            return unavailable(Reason::TooMany { what: "triangles of the floor", limit: limits.triangles });
        }
        if rooms.len() > limits.rooms {
            return unavailable(Reason::TooMany { what: "rooms", limit: limits.rooms });
        }
        let mut placed = Vec::with_capacity(triangles.len());
        for (i, t) in triangles.iter().enumerate() {
            if i.is_multiple_of(PLACED) {
                go_on(cancel)?;
            }
            if area(t).is_finite() && area(t) > 0.0 && middle(t).finite() {
                // Rooms are asked half the eye's height up: one may begin a little above the floor.
                placed.push((*t, room(rooms, middle(t) + V3::new(0.0, EYE / 2.0, 0.0))));
            }
        }
        let place = |t: &([V3; 3], Option<usize>)| {
            let m = middle(&t.0);
            (t.1.unwrap_or(usize::MAX), (m[1] / 256.0).floor() as i64, (m[0] / 512.0).floor() as i64, m[2])
        };
        placed.sort_by(|a, b| {
            let (a, b) = (place(a), place(b));
            (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)).then(a.3.total_cmp(&b.3))
        });
        let triangles: Vec<[V3; 3]> = placed.into_iter().map(|t| t.0).collect();
        let mut sum = 0.0;
        let until = triangles.iter().map(|t| {
            sum += f64::from(area(t));
            sum
        });
        Ok(Floor { until: until.collect(), triangles })
    }

    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    /// The eye above every triangle's middle.
    pub fn eyes(&self) -> Vec<V3> {
        self.triangles.iter().map(|t| middle(t) + V3::new(0.0, EYE, 0.0)).collect()
    }

    /// The eye above one point in each of `count` strata of the same area: its middle's triangle, and there
    /// a place by the stratum's number alone. The caller bounds how many.
    pub(crate) fn anchors(&self, count: usize) -> Vec<V3> {
        if self.is_empty() {
            return Vec::new();
        }
        let even = |i: usize, step: f64| ((0.5 + (i + 1) as f64 * step) % 1.0) as f32;
        let one = |i: usize| self.point((i as f64 + 0.5) / count as f64, even(i, 0.754_877_666), even(i, 0.569_840_291));
        (0..count).map(|i| one(i) + V3::new(0.0, EYE, 0.0)).collect()
    }

    /// The point `share` (0 to 1) of the area in, at (u, v) of its triangle.
    fn point(&self, share: f64, u: f32, v: f32) -> V3 {
        let all = self.until.last().copied().unwrap_or(0.0);
        let i = self.until.partition_point(|a| *a <= share * all).min(self.triangles.len() - 1);
        let t = &self.triangles[i];
        let u = u.sqrt();
        t[0] * (1.0 - u) + t[1] * (u * (1.0 - v)) + t[2] * (u * v)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn made(triangles: &[[V3; 3]], rooms: &[Room]) -> Floor {
        Floor::new(triangles, rooms, &FloorLimits::default(), &|| false).unwrap()
    }

    /// The floor of `gather::tests::room`; the eyes are in its middle's height.
    pub fn floor() -> Floor {
        let c = |x: f32, z: f32| V3::new(80.0 * x, -100.0, 80.0 * z);
        made(&[[c(-1.0, -1.0), c(1.0, 1.0), c(1.0, -1.0)], [c(-1.0, -1.0), c(-1.0, 1.0), c(1.0, 1.0)]], &[])
    }

    #[test]
    fn eyes_and_anchors() {
        let t = |x: f32| [V3::new(x, 0.0, 0.0), V3::new(x + 3.0, 0.0, 0.0), V3::new(x, 0.0, 3.0)];
        let flat = [V3::ZERO, V3::new(1.0, 0.0, 0.0), V3::new(2.0, 0.0, 0.0)];
        let f = made(&[t(0.0), flat, [V3([f32::NAN; 3]); 3], t(9.0)], &[]);
        assert_eq!(f.eyes(), [V3::new(1.0, EYE, 1.0), V3::new(10.0, EYE, 1.0)]);
        // Equal areas: half of the strata each, every point over its triangle.
        let a = f.anchors(64);
        assert!(a.len() == 64 && (0..64).all(|i| (a[i][0] > 8.0) == (i >= 32)));
        let over = |p: &V3, x: f32| p[1] == EYE && p[0] - x > -1e-5 && p[2] > -1e-5 && p[0] - x + p[2] < 3.0 + 1e-5;
        assert!(a.iter().enumerate().all(|(i, p)| over(p, if i < 32 { 0.0 } else { 9.0 })), "{a:?}");
        // Three times the area has three times the strata, whichever comes first.
        let wide = [V3::new(9.0, 0.0, 0.0), V3::new(18.0, 0.0, 0.0), V3::new(9.0, 0.0, 3.0)];
        let long = [V3::ZERO, V3::new(9.0, 0.0, 0.0), V3::new(0.0, 0.0, 3.0)];
        for (triangles, first) in [([t(0.0), wide], 16), ([long, t(9.0)], 48)] {
            let a = made(&triangles, &[]).anchors(64);
            assert_eq!((0..64).filter(|i| a[*i][0] < 9.0).collect::<Vec<_>>(), (0..first).collect::<Vec<_>>());
        }
        let floor = floor();
        assert!(floor.anchors(4).iter().all(|p| (p[1] - 12.0).abs() < 1e-4) && floor.anchors(9) == floor.anchors(9));
        assert!(Floor::default().is_empty() && Floor::default().anchors(4).is_empty() && Floor::default().eyes().is_empty() && floor.anchors(0).is_empty());
    }

    /// By the rooms first, the smallest that holds one; those of none come last; then by storey, strip and place.
    #[test]
    fn the_order() {
        let t = |x: f32, y: f32, z: f32| [V3::new(x, y, z), V3::new(x + 3.0, y, z), V3::new(x, y, z + 3.0)];
        let axes = [V3::new(1.0, 0.0, 0.0), V3::new(0.0, 1.0, 0.0), V3::new(0.0, 0.0, 1.0)];
        let room = |x: f32, half: f32| Room { center: V3::new(x, 50.0, 0.0), axes, half: [half, 300.0, 300.0] };
        let rooms = [room(0.0, 900.0), room(0.0, 50.0)];
        let given = [t(5000.0, 0.0, 0.0), t(10.0, 0.0, 0.0), t(200.0, 0.0, 9.0), t(200.0, 0.0, -9.0), t(-200.0, 0.0, 0.0), t(200.0, 290.0, -20.0)];
        let f = made(&given, &rooms);
        assert_eq!(f.triangles, [given[4], given[3], given[2], given[5], given[1], given[0]].to_vec());
        assert_eq!(made(&given, &[]).triangles.len(), 6);
        // Rooms that are none: without a number, an end, or a size that is one.
        for half in [f32::NAN, f32::INFINITY, f32::MAX] {
            let broken = [Room { half: [9000.0, half, half], ..rooms[0] }, Room { center: V3([f32::NAN; 3]), ..rooms[1] }, rooms[0], rooms[1]];
            assert!(made(&given, &broken) == f);
        }
        // A middle that is too far off to be a number: its sides are short.
        let off = |x: f32, z: f32| V3::new(1.5e38, x, z);
        assert!(made(&[[off(0.0, 0.0), off(1.0, 0.0), off(0.0, 1.0)]], &rooms).is_empty());
        assert!(made(&[[V3::ZERO, V3::new(f32::MAX, 0.0, 0.0), V3::new(0.0, 0.0, f32::MAX)]], &rooms).is_empty());
        // Too many are refused before they are looked at.
        let of = |triangles, n, cancel: Cancel| Floor::new(&given, &rooms, &FloorLimits { triangles, rooms: n }, cancel).map(|f| f.triangles.len());
        assert_eq!(of(6, 2, &|| false), Ok(6));
        assert_eq!(of(5, 2, &|| true), Err(Failed::Unavailable(Reason::TooMany { what: "triangles of the floor", limit: 5 })));
        assert_eq!(of(6, 1, &|| true), Err(Failed::Unavailable(Reason::TooMany { what: "rooms", limit: 1 })));
        assert_eq!(of(6, 2, &|| true), Err(Failed::Cancelled));
        assert_eq!(of(6, 2, &|| panic!("expected here")), Err(Failed::Internal));
    }
}
