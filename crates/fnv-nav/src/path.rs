//! Paths over the nav graph: A* across triangles (through edges, cell seams and doors),
//! then the funnel algorithm per space to pull the corridor of triangles tight into a
//! polyline that keeps a clearance from the walkable area's outline.
//!
//! How the original game chooses paths is not known; costs and clearances here are
//! engine choices (docs/design/navmesh.md §3), not game rules.

use crate::graph::{area2, DoorEdge, EdgeKind, NavGraph, Space, NONE};
use crate::record::tri_flags;
use serde::Serialize;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};

/// Path searches ([`NavGraph::find_path`]) since [`probe::enable`], for attributing
/// their cost (diagnostics only: never read by the searches; a reader takes differences
/// between two reads). Nothing is counted until it is enabled.
pub mod probe {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

    static ON: AtomicBool = AtomicBool::new(false);
    static SEARCHES: AtomicU64 = AtomicU64::new(0);
    static FOUND: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn searched(found: bool) {
        if !ON.load(Relaxed) {
            return;
        }
        SEARCHES.fetch_add(1, Relaxed);
        if found {
            FOUND.fetch_add(1, Relaxed);
        }
    }

    /// Count from now on (the engine's `NVF_PLAY_TEST_PROFILE_COUNTS`).
    pub fn enable() {
        ON.store(true, Relaxed);
    }

    /// (searches, searches that found a path) so far.
    pub fn searches() -> (u64, u64) {
        (SEARCHES.load(Relaxed), FOUND.load(Relaxed))
    }
}

/// What a path may use and what it costs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PathParams {
    /// Clearance kept from the walkable area's outline, game units (about the actor's
    /// radius).
    pub radius: f32,
    /// A large creature: triangles flagged "No Large Creatures" are closed.
    pub large: bool,
    /// Whether doors may be used (paths between spaces).
    pub doors: bool,
    /// Whether ledge links may be used (drops down and climbs up a ledge).
    pub ledges: bool,
    /// Additional downward ledge links allowed after ordinary routing fails, up to this
    /// takeoff-to-landing height [design, #2532]. `None` preserves the navigation default.
    pub safe_drop_max: Option<f32>,
    /// The highest ledge-up link used, game units from the middle of its edge to the
    /// landing triangle's centre: what the actor's jump reaches
    /// ([`crate::jump::LEDGE_RISE_MAX`]).
    pub ledge_rise_max: f32,
    /// Cost per unit across triangles flagged "Preferred", relative to 1 elsewhere.
    pub preferred_cost: f32,
    /// Cost per unit across water triangles.
    pub water_cost: f32,
    /// Places to keep away from (centre, radius; radiated ground for an actor that avoids
    /// radiation): a triangle whose centre lies in one costs [`PathParams::avoid_cost`]
    /// times as much.
    pub avoid: Vec<([f32; 3], f32)>,
    pub avoid_cost: f32,
    /// Whether water triangles may be used at all: a creature without the Swims flag
    /// "will avoid using water triangles" [documented: GECK wiki Navmesh Creation
    /// Workflow], so they are closed to it.
    pub water: bool,
    /// Fixed cost of using a door, in units of distance.
    pub door_cost: f32,
    /// A* gives up after expanding this many triangles (per leg).
    pub max_expanded: usize,
    /// Locked doors may be used (the actor can open them).
    pub open_locked: bool,
    /// Triangles steeper than this (degrees) are closed: for a mover that cannot climb
    /// what the navmesh calls walkable. `None`: every navmesh triangle is walkable.
    pub max_slope_deg: Option<f32>,
    /// Keys the actor carries: locked doors these open may be used.
    pub keys: Vec<u32>,
    /// Locked doors the actor owns (its base or a faction it is in): these may be used
    /// as if it carried their key.
    pub owned_doors: Vec<u32>,
    /// [hypothesis, #2716] Doors that open in place known to stand open: these do not
    /// refuse triangles, even while locked. [design / NavGraph::square_approach]
    /// A path that starts (or turns) within
    /// the approach to one goes straight on through it instead of backing off to the stop
    /// in front of it ([`NavGraph::square_approach`]).
    pub open_doors: Vec<u32>,
    /// Whether the actor opens doors. One that does not (a creature flagged "Can't Open
    /// Doors") uses no teleport door; doors that open in place are left to how it is
    /// walked (docs/spec-notes/packages.md §7.1).
    pub opens_doors: bool,
    /// Doors whose script did not let this actor through when it last tried (a door's
    /// OnActivate block that called no `Activate` for it): closed to its paths as a
    /// locked door without the key is (`fnv_game::npc_doors`).
    pub refused_doors: Vec<u32>,
    /// Load doors not in the world now ([`NavGraph::disabled_doors`]) may be used: only
    /// for asking whether two places are joined by doors at all
    /// ([`NavGraph::joined_by_doors`]), never for a walk.
    pub through_disabled: bool,
}

impl Default for PathParams {
    fn default() -> Self {
        PathParams {
            radius: 24.0,
            large: false,
            doors: true,
            ledges: false,
            safe_drop_max: None,
            ledge_rise_max: crate::jump::LEDGE_RISE_MAX,
            preferred_cost: 0.85,
            water_cost: 3.0,
            avoid: Vec::new(),
            avoid_cost: 1.0,
            water: true,
            door_cost: 256.0,
            max_expanded: 400_000,
            open_locked: false,
            max_slope_deg: None,
            keys: Vec::new(),
            owned_doors: Vec::new(),
            open_doors: Vec::new(),
            opens_doors: true,
            refused_doors: Vec::new(),
            through_disabled: false,
        }
    }
}

/// A position in a space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Place {
    pub space: Space,
    pub pos: [f32; 3],
}

/// The part of a path within one space.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Leg {
    pub space: Space,
    /// Corners of the polyline, from the leg's start to its end.
    pub points: Vec<[f32; 3]>,
    /// The triangles the leg crosses, in order.
    pub corridor: Vec<u32>,
    /// The door that ends the leg (the next leg starts at its arrival), if any.
    pub door: Option<DoorEdge>,
    /// Doors that open in place on the way, in order: where the actor stops to open each
    /// ([`NavGraph::door_stops`]).
    pub door_stops: Vec<DoorStop>,
    /// Ledge links crossed on the way, in order: the actor takes off at one point and
    /// lands at the next (`crate::jump`).
    pub hops: Vec<LedgeHop>,
}

/// A ledge link a leg crosses: from [`Leg::points`]`[point]`, on the top edge (a drop)
/// or below it (a climb), to `points[point + 1]` on the triangle the link names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LedgeHop {
    pub point: usize,
    /// A ledge-up link (a climb) rather than a ledge-down one (a drop).
    pub up: bool,
}

/// A stop in front of a door that opens in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DoorStop {
    /// The door reference.
    pub door: u32,
    /// The point of the leg where the actor stops (an index into [`Leg::points`]).
    pub point: usize,
}

/// A path: one leg per space visited.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Path {
    pub legs: Vec<Leg>,
    /// Polyline length summed over the legs, game units.
    pub length: f32,
    /// Triangles A* expanded.
    pub expanded: usize,
    /// How far the start and the goal were moved to put them on the navmesh (3D
    /// distance); callers decide whether a far snap means the goal is not really reached.
    pub start_snap: f32,
    pub goal_snap: f32,
}

impl Path {
    pub fn end(&self) -> Option<[f32; 3]> {
        self.legs.last().and_then(|l| l.points.last().copied())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum PathError {
    /// No usable triangle near the start or the goal.
    NoTriangle { which: &'static str, at: [f32; 3] },
    /// The goal cannot be reached from the start (searched everything reachable).
    Unreachable { expanded: usize },
    /// The search stopped at `max_expanded`.
    TooFar { expanded: usize },
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PathError::NoTriangle { which, at } => write!(f, "no navmesh triangle near the {which} ({:.0}, {:.0}, {:.0})", at[0], at[1], at[2]),
            PathError::Unreachable { expanded } => write!(f, "the goal cannot be reached ({expanded} triangles searched)"),
            PathError::TooFar { expanded } => write!(f, "search limit reached after {expanded} triangles"),
        }
    }
}

impl std::error::Error for PathError {}

/// How far from a point a path start or goal may be snapped onto the navmesh.
pub const SNAP_RADIUS: f32 = 512.0;

/// A path search this slow, milliseconds, is said at debug level ([`NavGraph::find_path`],
/// #3785) [design: a frame's budget].
const SLOW_SEARCH_MS: f64 = 16.0;

/// How far a point may be above or below a triangle's surface and still be on it, for
/// straight-line walks ([`NavGraph::walkable_line`]): less than the gap between levels
/// stacked over each other, which is over 40 units almost everywhere in FalloutNV.esm
/// [observed] (docs/spec-notes/navmesh.md §4b) [engine choice].
pub const LEVEL_GAP: f32 = 40.0;

#[derive(Clone, Copy)]
enum Via {
    Start,
    Edge(u32, u8),
    Door(u32, usize),
}

#[derive(Clone, Copy)]
struct Node {
    g: f32,
    via: Via,
    /// Where the path enters the triangle (estimate used for costs).
    at: [f32; 3],
    closed: bool,
}

/// A float with a total order (for heaps).
#[derive(PartialEq, Clone, Copy)]
struct OrdF(f32);
impl Eq for OrdF {}
impl PartialOrd for OrdF {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for OrdF {
    fn cmp(&self, o: &Self) -> Ordering {
        self.0.total_cmp(&o.0)
    }
}

/// A door in the door search (triangle, index), or `None` for the goal.
type DoorKey = Option<(u32, usize)>;

#[derive(PartialEq)]
struct Open(f32, u32);
impl Eq for Open {}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        // Min-heap on f, ties by triangle for determinism.
        o.0.total_cmp(&self.0).then_with(|| o.1.cmp(&self.1))
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, a[2] + (b[2] - a[2]) * s]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// A leg being assembled from its pieces ([`NavGraph::legs_of`]).
#[derive(Default)]
struct LegBuild {
    points: Vec<[f32; 3]>,
    corridor: Vec<u32>,
    stops: Vec<DoorStop>,
    hops: Vec<LedgeHop>,
}

/// Where a piece of a leg starts: the point, its triangle, and the index in the leg's
/// corridor of the piece's first triangle.
struct Piece {
    start: [f32; 3],
    start_tri: u32,
    first: usize,
}

impl NavGraph {
    /// Whether a path may step from triangle `t` over edge `e`.
    fn passable(&self, t: u32, e: usize, p: &PathParams) -> Option<u32> {
        self.passable_for_path(t, e, p, &[])
    }

    // [design, #2716] Only the original path endpoints can bypass coarse door tagging;
    // all other passability checks, including raycasts, retain the ordinary refusal.
    fn passable_for_path(&self, t: u32, e: usize, p: &PathParams, door_endpoints: &[u32]) -> Option<u32> {
        let tri = &self.tris[t as usize];
        let n = tri.next[e];
        if n == NONE || !self.usable(n) {
            return None;
        }
        match tri.kind[e] {
            EdgeKind::Internal | EdgeKind::Portal => {}
            EdgeKind::LedgeDown if p.ledges || p.safe_drop_max.is_some_and(|max| {
                let (off, land) = self.ledge_ends(t, e, p.radius);
                off[2] - land[2] <= max
            }) => {}
            EdgeKind::LedgeUp if p.ledges && self.ledge_rise(t, e) <= p.ledge_rise_max => {}
            _ => return None,
        }
        if p.large && self.tris[n as usize].has(tri_flags::NO_LARGE_CREATURES) {
            return None;
        }
        if !p.water && self.tris[n as usize].has(tri_flags::WATER) {
            return None;
        }
        if p.max_slope_deg.is_some_and(|m| self.up[n as usize] < m.to_radians().cos() - 1e-4) {
            return None;
        }
        // [design, #2716] Passing triangles stay closed; an endpoint beside the door
        // can use its own overlapping triangle (checked by door_endpoint_open).
        if !door_endpoints.contains(&n) && !self.door_tri_open(n, p) {
            return None;
        }
        Some(n)
    }

    /// How far a ledge link (edge `e` of `t`) rises: from the middle of the edge to the
    /// centre of the triangle beyond (negative for a drop), as navmesh.md §3 measures.
    pub fn ledge_rise(&self, t: u32, e: usize) -> f32 {
        let (l, r) = self.portal(t, e);
        self.centre(self.tris[t as usize].next[e])[2] - (l[2] + r[2]) / 2.0
    }

    /// Where an actor crossing ledge link `e` of `t` takes off and lands: the middle of
    /// the edge (kept `radius` from its outline ends), and the point of the triangle
    /// beyond nearest to it seen from above, moved `radius` towards that triangle's
    /// centre [design].
    pub fn ledge_ends(&self, t: u32, e: usize, radius: f32) -> ([f32; 3], [f32; 3]) {
        let (l, r) = self.shrunk_portal(t, e, radius);
        let off = lerp(l, r, 0.5);
        let n = self.tris[t as usize].next[e];
        let c = self.centre(n);
        let near = self.nearest_on_tri_xy(n, off);
        let d = (c[0] - near[0]).hypot(c[1] - near[1]);
        let s = if d > 1e-4 { (radius / d).min(1.0) } else { 0.0 };
        let (x, y) = (near[0] + (c[0] - near[0]) * s, near[1] + (c[1] - near[1]) * s);
        (off, [x, y, self.height_on(n, x, y)])
    }

    /// The point of triangle `t` nearest to `p` seen from above (its height on the plane).
    fn nearest_on_tri_xy(&self, t: u32, p: [f32; 3]) -> [f32; 3] {
        if self.contains_xy(t, p[0], p[1]) {
            return [p[0], p[1], self.height_on(t, p[0], p[1])];
        }
        let c = self.corners(t);
        let mut best = (f32::MAX, c[0]);
        for k in 0..3 {
            let (a, b) = (c[k], c[(k + 1) % 3]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            let s = if len2 > 1e-8 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let q = lerp(a, b, s);
            let d = (q[0] - p[0]).hypot(q[1] - p[1]);
            if d < best.0 {
                best = (d, q);
            }
        }
        best.1
    }

    fn cost_factor(&self, t: u32, p: &PathParams) -> f32 {
        let tri = &self.tris[t as usize];
        let base = if tri.has(tri_flags::WATER) {
            p.water_cost
        } else if tri.has(tri_flags::PREFERRED) {
            p.preferred_cost
        } else {
            1.0
        };
        if p.avoid.is_empty() {
            return base;
        }
        let c = self.centre(t);
        let inside = p.avoid.iter().any(|(at, r)| (0..3).map(|i| (at[i] - c[i]).powi(2)).sum::<f32>() <= r * r);
        if inside {
            base * p.avoid_cost
        } else {
            base
        }
    }

    /// Whether a path may go through door `d`: the door is in the world
    /// ([`NavGraph::disabled_doors`]: a disabled door is not there to use [hypothesis: the
    /// original's actors do not walk through a disabled load door; issue #2566]), its
    /// arrival is usable and the door is not locked, or the traveller holds its key or
    /// owns it, or may open locks.
    pub fn door_open(&self, d: &DoorEdge, p: &PathParams) -> bool {
        if (self.disabled_doors.contains(&d.door) && !p.through_disabled) || !self.usable(d.to) || !p.opens_doors || p.refused_doors.contains(&d.door) {
            return false;
        }
        match self.door_locks.get(&d.door) {
            None => true,
            Some(_) if p.open_locked || p.owned_doors.contains(&d.door) => true,
            Some(l) => l.key.is_some_and(|k| p.keys.contains(&k)),
        }
    }

    /// The doors to go through, in order, to get from `from` to `to`: empty when both are
    /// in the same walkable area. A search over walkable areas joined by doors, costed by
    /// straight lines between doors; it answers "can this actor get there?" (through any
    /// number of interiors and worldspaces) without searching triangles. `Unreachable`
    /// when no chain of open doors joins the two areas.
    pub fn door_route(&self, from: Place, to: Place, p: &PathParams) -> Result<Vec<DoorEdge>, PathError> {
        let (start, s_at) = self.nearest(from.space, from.pos, SNAP_RADIUS).ok_or(PathError::NoTriangle { which: "start", at: from.pos })?;
        let (goal, g_at) = self.nearest(to.space, to.pos, SNAP_RADIUS).ok_or(PathError::NoTriangle { which: "goal", at: to.pos })?;
        let route = self.route_between(start, s_at, goal, g_at, p, &[]).ok_or(PathError::Unreachable { expanded: 0 })?;
        Ok(route.iter().map(|&(f, i)| self.doors[&f][i]).collect())
    }

    /// Whether `to` can be reached from `from` ([`NavGraph::door_route`]).
    pub fn can_reach(&self, from: Place, to: Place, p: &PathParams) -> bool {
        self.door_route(from, to, p).is_ok()
    }

    /// Whether any chain of load doors joins the walkable areas of `from` and `to`,
    /// every door counted, locked or disabled (a script's state of the moment), for an
    /// actor that opens doors: `Some(false)` when the data gives no door between them at
    /// all (two navmesh islands of one cell, a holding cell no door leads to; the AI puts
    /// its actors out of sight at such a place instead, packages.md §11), `None` when
    /// either place has no navmesh near it.
    pub fn joined_by_doors(&self, from: Place, to: Place) -> Option<bool> {
        let any = PathParams { open_locked: true, through_disabled: true, ..PathParams::default() };
        match self.door_route(from, to, &any) {
            Ok(_) => Some(true),
            Err(PathError::NoTriangle { .. }) => None,
            Err(_) => Some(false),
        }
    }

    /// For a way `p` finds none of: the doors that close it. A path is looked for with
    /// every door open (locked ones, those that open in place, those whose script refused
    /// the actor; not doors that are not in the world), and the doors on it that `p`
    /// keeps closed are returned, in the order met. `None` when there is no way even so
    /// (no navmesh, no chain of doors, ...): no door is to blame. For telling an
    /// unreachable place apart in the logs (#2405: "0 triangles searched" read as no
    /// door at all, while a locked door in the same room closed the way).
    pub fn doors_closing(&self, from: Place, to: Place, p: &PathParams) -> Option<Vec<u32>> {
        let all = PathParams {
            open_locked: true,
            opens_doors: true,
            refused_doors: Vec::new(),
            open_doors: self.in_place.keys().copied().collect(),
            ..p.clone()
        };
        let path = self.find_path(from, to, &all).ok()?;
        let mut out: Vec<u32> = Vec::new();
        for l in &path.legs {
            for s in &l.door_stops {
                if !self.in_place_door_open(s.door, p) && !out.contains(&s.door) {
                    out.push(s.door);
                }
            }
            if let Some(d) = l.door.filter(|d| !self.door_open(d, p) && !out.contains(&d.door)) {
                out.push(d.door);
            }
        }
        // None with a stop: one the corridor crosses beside the polyline (no stop) closed
        // it, refusing the triangles it stands on.
        if out.is_empty() {
            for d in path.legs.iter().flat_map(|l| &l.corridor).filter_map(|t| self.door_tris.get(t)).flatten() {
                if !self.in_place_door_open(*d, p) && !out.contains(d) {
                    out.push(*d);
                }
            }
        }
        Some(out)
    }

    /// Doors (triangle, index) from the area of `start` to the area of `goal`, leaving
    /// out `excluded`; `Some(vec![])` within one area.
    fn route_between(&self, start: u32, s_at: [f32; 3], goal: u32, g_at: [f32; 3], p: &PathParams, excluded: &[(u32, usize)]) -> Option<Vec<(u32, usize)>> {
        let (a_s, a_g) = (self.area[start as usize], self.area[goal as usize]);
        if a_s == NONE || a_g == NONE {
            return None;
        }
        if a_s == a_g {
            return Some(Vec::new());
        }
        if !p.doors {
            return None;
        }
        // Dijkstra over doors; the goal is the node `None`.
        let mut best: HashMap<DoorKey, (f32, DoorKey)> = HashMap::new();
        let mut heap: BinaryHeap<(Reverse<OrdF>, DoorKey)> = BinaryHeap::new();
        let leave = |area: u32, at: [f32; 3], g: f32, prev: DoorKey, best: &mut HashMap<DoorKey, (f32, DoorKey)>, heap: &mut BinaryHeap<(Reverse<OrdF>, DoorKey)>| {
            for &(f, i) in self.area_doors.get(&area).map_or(&[][..], |v| v.as_slice()) {
                if excluded.contains(&(f, i)) || !self.door_open(&self.doors[&f][i], p) {
                    continue;
                }
                let c = g + dist(at, self.centre(f)) + p.door_cost;
                let k = Some((f, i));
                if best.get(&k).is_none_or(|b| c < b.0) {
                    best.insert(k, (c, prev));
                    heap.push((Reverse(OrdF(c)), k));
                }
            }
        };
        leave(a_s, s_at, 0.0, None, &mut best, &mut heap);
        let mut done: std::collections::HashSet<DoorKey> = std::collections::HashSet::new();
        while let Some((Reverse(OrdF(c)), k)) = heap.pop() {
            if !done.insert(k) {
                continue;
            }
            let Some((f, i)) = k else {
                // The goal: walk back.
                let mut route = Vec::new();
                let mut at = best[&None].1;
                while let Some(d) = at {
                    route.push(d);
                    at = best[&Some(d)].1;
                }
                route.reverse();
                return Some(route);
            };
            let d = self.doors[&f][i];
            let a = self.area[d.to as usize];
            if a == a_g {
                let g = c + dist(d.arrival, g_at);
                if best.get(&None).is_none_or(|b| g < b.0) {
                    best.insert(None, (g, k));
                    heap.push((Reverse(OrdF(g)), None));
                }
            }
            leave(a, d.arrival, c, k, &mut best, &mut heap);
        }
        None
    }

    /// A path from `from` to `to` (each snapped onto the navmesh of its space within
    /// [`SNAP_RADIUS`]). Within one walkable area: A* over triangles. Across areas
    /// (other spaces, other islands): the door route first ([`NavGraph::door_route`]),
    /// then A* for each leg; a leg that cannot be walked takes its door out of the route
    /// and the route is planned again. With ledges allowed, A* runs over everything at
    /// once (ledges join areas the area labels keep apart).
    /// [design, #2532] With only `safe_drop_max`, try the ordinary route first and
    /// search using harmless downward links only when it is unreachable.
    pub fn find_path(&self, from: Place, to: Place, p: &PathParams) -> Result<Path, PathError> {
        // A span for profiling and a count for diagnostics ([`probe`]); the search is
        // the same.
        let _span = tracing::info_span!("nav::find_path").entered();
        let t0 = std::time::Instant::now();
        let found = self.search_path(from, to, p);
        probe::searched(found.is_ok());
        // A slow search (one that ran to its limit, or searched a whole area for an
        // unreachable place) is said at debug level with its ends and outcome, so a caller
        // repeating one can be found (#3785).
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        if ms >= SLOW_SEARCH_MS {
            let outcome = match &found {
                Ok(path) => format!("found, {:.0} units, {} triangles searched", path.length, path.expanded),
                Err(e) => e.to_string(),
            };
            tracing::debug!("nav: a path search from {:?} {:.0?} to {:?} {:.0?} took {ms:.1} ms: {outcome}", from.space, from.pos, to.space, to.pos);
        }
        found
    }

    /// [`NavGraph::find_path`]'s search.
    fn search_path(&self, from: Place, to: Place, p: &PathParams) -> Result<Path, PathError> {
        if !p.ledges && p.safe_drop_max.is_some() {
            // [design, #2532] Exclude safe drops from both ordinary A* and smoothing.
            // Retry only an unreachable route; retain search limits and snap errors.
            let ordinary = PathParams { safe_drop_max: None, ..p.clone() };
            match self.search_path(from, to, &ordinary) {
                Err(PathError::Unreachable { .. }) => {}
                result => return result,
            }
        }
        let (start, s_at) = self.nearest(from.space, from.pos, SNAP_RADIUS).ok_or(PathError::NoTriangle { which: "start", at: from.pos })?;
        let (goal, g_at) = self.nearest(to.space, to.pos, SNAP_RADIUS).ok_or(PathError::NoTriangle { which: "goal", at: to.pos })?;
        // [design, #2716] Test the points against every refusing oriented footprint,
        // then exempt only these two triangles throughout this path's leg searches.
        if !self.door_endpoint_open(start, s_at, p) || !self.door_endpoint_open(goal, g_at, p) {
            return Err(PathError::Unreachable { expanded: 0 });
        }
        let door_endpoints = [start, goal];
        // Water closed to it ([`PathParams::water`]): a goal in water is out of reach at
        // once, not after a search of everything round it; one that stands in water may
        // walk out through it [design].
        let wet = |t: u32| self.tris[t as usize].has(tri_flags::WATER);
        let lifted;
        let p = if p.water {
            p
        } else if wet(start) {
            lifted = PathParams { water: true, ..p.clone() };
            &lifted
        } else if wet(goal) {
            return Err(PathError::Unreachable { expanded: 0 });
        } else {
            p
        };
        let (chain, expanded) = if p.ledges || p.safe_drop_max.is_some() {
            self.search(start, s_at, goal, g_at, to.space, p, p.doors, &door_endpoints)?
        } else {
            let mut excluded: Vec<(u32, usize)> = Vec::new();
            let mut expanded = 0usize;
            loop {
                let Some(route) = self.route_between(start, s_at, goal, g_at, p, &excluded) else {
                    return Err(PathError::Unreachable { expanded });
                };
                let mut chain: Vec<(u32, Via)> = Vec::new();
                let (mut t, mut at) = (start, s_at);
                let mut entered_by: Option<(u32, usize)> = None;
                let mut failed = None;
                for &(f, i) in route.iter().chain(std::iter::once(&(NONE, 0))) {
                    let (target, target_at, space) = if f == NONE {
                        (goal, g_at, to.space)
                    } else {
                        (f, self.centre(f), self.space_of_tri(f).unwrap_or(to.space))
                    };
                    match self.search(t, at, target, target_at, space, p, false, &door_endpoints) {
                        Ok((mut c, n)) => {
                            expanded += n;
                            // This leg starts by the door that ended the last one.
                            if let Some((lf, li)) = entered_by {
                                c[0].1 = Via::Door(lf, li);
                            }
                            chain.extend(c);
                        }
                        Err(PathError::TooFar { expanded: n }) => return Err(PathError::TooFar { expanded: expanded + n }),
                        Err(e) => {
                            // A leg searched and not found counts too (#2405): the count
                            // said 0 for every way a closed door in one area shut off,
                            // as if the door search had found nothing.
                            if let PathError::Unreachable { expanded: n } = e {
                                expanded += n;
                            }
                            failed = Some((f, i));
                            break;
                        }
                    }
                    if f != NONE {
                        let d = self.doors[&f][i];
                        (t, at) = (d.to, d.arrival);
                        entered_by = Some((f, i));
                    }
                }
                match failed {
                    None => break (chain, expanded),
                    // The leg to the goal itself cannot be walked: no door helps.
                    Some((NONE, _)) => return Err(PathError::Unreachable { expanded }),
                    Some(d) if excluded.len() < 16 => excluded.push(d),
                    Some(_) => return Err(PathError::Unreachable { expanded }),
                }
            }
        };
        let legs = self.legs_of(&chain, s_at, g_at, from.space, to.space, p);
        // [design, #2716] Check the final geometry after funneling, shortcuts and door
        // approaches: exempt endpoint triangles can straddle the same closed door.
        if legs.iter().any(|l| !self.door_segments_open(&l.corridor, &l.points, p)) {
            return Err(PathError::Unreachable { expanded });
        }
        let length = legs.iter().map(|l| l.points.windows(2).map(|w| dist(w[0], w[1])).sum::<f32>()).sum();
        Ok(Path { legs, length, expanded, start_snap: dist(from.pos, s_at), goal_snap: dist(to.pos, g_at) })
    }

    /// A* from triangle `start` (entered at `s_at`) to `goal` (`g_at`, in `goal_space`):
    /// the triangles and how each was entered, and how many were expanded.
    #[allow(clippy::too_many_arguments)]
    fn search(&self, start: u32, s_at: [f32; 3], goal: u32, g_at: [f32; 3], goal_space: Space, p: &PathParams, doors: bool, door_endpoints: &[u32]) -> Result<(Vec<(u32, Via)>, usize), PathError> {
        let min_factor = p.preferred_cost.min(1.0).min(p.water_cost).max(0.01);
        let h = |t: u32, at: [f32; 3]| {
            if self.space_of_tri(t) == Some(goal_space) {
                dist(at, g_at) * min_factor
            } else {
                0.0
            }
        };
        let mut nodes: HashMap<u32, Node> = HashMap::new();
        let mut open = BinaryHeap::new();
        nodes.insert(start, Node { g: 0.0, via: Via::Start, at: s_at, closed: false });
        open.push(Open(h(start, s_at), start));
        let mut expanded = 0usize;
        let mut found = false;
        while let Some(Open(_, t)) = open.pop() {
            let node = nodes[&t];
            if node.closed {
                continue;
            }
            nodes.get_mut(&t).expect("open node").closed = true;
            if t == goal {
                found = true;
                break;
            }
            expanded += 1;
            if expanded > p.max_expanded {
                return Err(PathError::TooFar { expanded });
            }
            let factor = self.cost_factor(t, p);
            let mut relax = |n: u32, via: Via, at: [f32; 3], step: f32, nodes: &mut HashMap<u32, Node>| {
                let g = node.g + step;
                let better = nodes.get(&n).is_none_or(|o| !o.closed && g < o.g);
                if better {
                    nodes.insert(n, Node { g, via, at, closed: false });
                    open.push(Open(g + h(n, at), n));
                }
            };
            for e in 0..3 {
                let Some(n) = self.passable_for_path(t, e, p, door_endpoints) else { continue };
                let (l, r) = self.portal(t, e);
                // Too narrow for the actor: an edge between two outline vertices that is
                // narrower than the actor's clearance radius.
                let tri = &self.tris[t as usize];
                let width = dist(l, r);
                let tight = self.boundary_vertex[tri.v[e] as usize] && self.boundary_vertex[tri.v[(e + 1) % 3] as usize];
                if tight && width < p.radius && n != goal {
                    continue;
                }
                // Costs run between the middles of the portals (kept clear of outline
                // ends), so each triangle has one entry point whatever the way in and
                // the search is a plain graph search; the funnel and the shortcut pass
                // straighten the result.
                let (sl, sr) = self.shrunk_portal(t, e, p.radius);
                let at = lerp(sl, sr, 0.5);
                let step = dist(node.at, at) * factor;
                relax(n, Via::Edge(t, e as u8), at, step, &mut nodes);
            }
            if doors {
                for (i, d) in self.doors.get(&t).map_or(&[][..], |v| v.as_slice()).iter().enumerate() {
                    if !self.door_open(d, p) {
                        continue;
                    }
                    let step = dist(node.at, self.centre(t)) * factor + p.door_cost;
                    relax(d.to, Via::Door(t, i), d.arrival, step, &mut nodes);
                }
            }
        }
        if !found {
            return Err(PathError::Unreachable { expanded });
        }
        // Walk back: triangles and how each was entered.
        let mut chain: Vec<(u32, Via)> = Vec::new();
        let mut t = goal;
        loop {
            let via = nodes[&t].via;
            chain.push((t, via));
            match via {
                Via::Start => break,
                Via::Edge(prev, _) | Via::Door(prev, _) => t = prev,
            }
        }
        chain.reverse();
        Ok((chain, expanded))
    }

    /// Split a chain of triangles into legs at doors, and pull each leg tight. A ledge
    /// link splits a leg into pieces pulled tight apart: one ends where the actor takes
    /// off, the next starts where it lands ([`LedgeHop`]).
    fn legs_of(&self, chain: &[(u32, Via)], s_at: [f32; 3], g_at: [f32; 3], from_space: Space, to_space: Space, p: &PathParams) -> Vec<Leg> {
        let mut legs = Vec::new();
        let mut leg = LegBuild::default();
        let mut piece = Piece { start: s_at, start_tri: chain.first().map_or(NONE, |c| c.0), first: 0 };
        let mut portals: Vec<([f32; 3], [f32; 3])> = Vec::new();
        for (i, &(t, _)) in chain.iter().enumerate() {
            leg.corridor.push(t);
            let next = chain.get(i + 1);
            match next.map(|n| n.1) {
                Some(Via::Edge(_, e)) => {
                    let kind = self.tris[t as usize].kind[e as usize];
                    if matches!(kind, EdgeKind::LedgeUp | EdgeKind::LedgeDown) {
                        let (off, land) = self.ledge_ends(t, e as usize, p.radius);
                        let space = self.space_of_tri(t).unwrap_or(from_space);
                        self.add_piece(&mut leg, space, &piece, off, &portals, p);
                        leg.hops.push(LedgeHop { point: leg.points.len() - 1, up: kind == EdgeKind::LedgeUp });
                        portals.clear();
                        let n = self.tris[t as usize].next[e as usize];
                        piece = Piece { start: land, start_tri: n, first: leg.corridor.len() };
                    } else {
                        portals.push(self.shrunk_portal(t, e as usize, p.radius));
                    }
                }
                Some(Via::Door(_, di)) => {
                    let d = self.doors[&t][di];
                    let end = self.door_approach(t, piece.start, p.radius);
                    let space = self.space_of_tri(t).unwrap_or(from_space);
                    self.add_piece(&mut leg, space, &piece, end, &portals, p);
                    let b = std::mem::take(&mut leg);
                    legs.push(Leg { space, points: b.points, corridor: b.corridor, door: Some(d), door_stops: b.stops, hops: b.hops });
                    portals.clear();
                    piece = Piece { start: d.arrival, start_tri: next.map_or(NONE, |n| n.0), first: 0 };
                }
                Some(Via::Start) => unreachable!("only the first triangle has no predecessor"),
                None => {
                    self.add_piece(&mut leg, to_space, &piece, g_at, &portals, p);
                    let b = std::mem::take(&mut leg);
                    legs.push(Leg { space: to_space, points: b.points, corridor: b.corridor, door: None, door_stops: b.stops, hops: b.hops });
                }
            }
        }
        legs
    }

    /// Pull one piece of a leg tight (from `piece.start` to `end` through `portals`) and
    /// add it to the leg, with the door stops on it.
    fn add_piece(&self, leg: &mut LegBuild, space: Space, piece: &Piece, end: [f32; 3], portals: &[([f32; 3], [f32; 3])], p: &PathParams) {
        let start_tri = if piece.start_tri != NONE { piece.start_tri } else { leg.corridor[piece.first] };
        let points = self.shortcut(space, start_tri, funnel(piece.start, end, portals), p);
        let (points, stops) = self.door_stops(space, &leg.corridor[piece.first..], points, p);
        let base = leg.points.len();
        leg.stops.extend(stops.into_iter().map(|s| DoorStop { door: s.door, point: s.point + base }));
        leg.points.extend(points);
    }

    /// Stops in front of the doors that open in place a leg's polyline enters, in order.
    /// For each door standing on a triangle of `corridor` whose bounds the polyline
    /// crosses, the polyline is made to cross it square through the middle of the opening
    /// (the navmesh's walkable stretch along the door's line, within its bounds): from a
    /// stop in front of the door, the opening's width plus the clearance away along its
    /// normal (less where the navmesh is shallower), to a point a clearance beyond it
    /// ([`NavGraph::square_approach`]). A hinged leaf swung open lies along that line at
    /// the hinge end, half the opening's width to the side, so the way through stays clear
    /// whichever way it opens. Where that cannot be walked, the stop is put on the
    /// polyline the stand-off before the crossing instead. Doors whose bounds the polyline
    /// passes beside get no stop. A door in [`PathParams::open_doors`] that the polyline
    /// reaches from a corner already within that approach (a path planned again on the
    /// way through: Follow packages replan every half second) has its stop at that corner
    /// and no back-off (owner's playtest 2026-10-17: a pack brahmin following its
    /// merchant through the open Novac motel gate walked back to the stop at each
    /// replan and never got through). Engine choices (docs/design/navmesh.md §6.1).
    pub fn door_stops(&self, space: Space, corridor: &[u32], mut points: Vec<[f32; 3]>, p: &PathParams) -> (Vec<[f32; 3]>, Vec<DoorStop>) {
        // A door not in the world gets no stop (`NavGraph::disabled_doors`).
        let mut doors: Vec<u32> = corridor.iter().filter_map(|t| self.door_tris.get(t)).flatten().copied().filter(|d| !self.disabled_doors.contains(d)).collect();
        if doors.is_empty() {
            return (points, Vec::new());
        }
        doors.sort_unstable();
        doors.dedup();
        let hd = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[1] - b[1]);
        let mut stops: Vec<DoorStop> = Vec::new();
        let mut from = 0usize;
        loop {
            // The next door the polyline enters after the last stop: (distance along the
            // polyline, segment, door).
            let mut next: Option<(f32, usize, u32)> = None;
            for &d in &doors {
                let door = &self.in_place[&d];
                let mut s = 0.0;
                for i in 0..points.len().saturating_sub(1) {
                    let len = hd(points[i], points[i + 1]);
                    if i >= from {
                        if let Some(t) = door.entry(points[i], points[i + 1]) {
                            let at = s + t * len;
                            if next.is_none_or(|n| at < n.0 || (at == n.0 && d < n.2)) {
                                next = Some((at, i, d));
                            }
                            break;
                        }
                    }
                    s += len;
                }
            }
            let Some((enter, seg, door)) = next else { break };
            doors.retain(|&d| d != door);
            if let Some((pts, stop)) = self.square_approach(space, &points, seg, door, p) {
                points = pts;
                stops.push(DoorStop { door, point: stop });
                from = stop;
                continue;
            }
            // On the polyline, the stand-off before the crossing: an existing corner within
            // a unit of it, else a new one.
            let at = (enter - self.in_place[&door].stand_off(p.radius)).max(0.0);
            let mut s = 0.0;
            let mut index = points.len() - 1;
            for i in 0..points.len() - 1 {
                if (at - s).abs() <= 1.0 {
                    index = i;
                    break;
                }
                let len = hd(points[i], points[i + 1]);
                if at < s + len {
                    let u = (at - s) / len.max(1e-6);
                    points.insert(i + 1, lerp(points[i], points[i + 1], u));
                    index = i + 1;
                    break;
                }
                s += len;
            }
            let index = index.max(from);
            stops.push(DoorStop { door, point: index });
            from = index;
        }
        (points, stops)
    }

    /// Make `points` cross door `door` square through the middle of its opening, the
    /// polyline entering it on segment `seg`: (new points, index of the stop), or `None`
    /// when no such way can be walked (see [`NavGraph::door_stops`]).
    pub fn square_approach(&self, space: Space, points: &[[f32; 3]], seg: usize, door: u32, p: &PathParams) -> Option<(Vec<[f32; 3]>, usize)> {
        let d = &self.in_place[&door];
        let (c0, n0, half) = (d.centre(), d.normal(), d.half_thickness());
        let (w, reach) = d.width_axis();
        let (a, b0) = (points[seg], points[seg + 1]);
        let dir = [b0[0] - a[0], b0[1] - a[1]];
        let along = dir[0] * n0[0] + dir[1] * n0[1];
        if along.abs() < 1e-3 {
            return None;
        }
        let n = if along > 0.0 { n0 } else { [-n0[0], -n0[1]] };
        let clear = 0.75 * p.radius;
        // Where the polyline crosses the door's line, along its width; then the walkable
        // stretch of navmesh along that line around it, within the bounds: the opening.
        let t = ((c0[0] - a[0]) * n[0] + (c0[1] - a[1]) * n[1]) / (dir[0] * n[0] + dir[1] * n[1]);
        let x = lerp(a, b0, t.clamp(0.0, 1.0));
        let u0 = ((x[0] - c0[0]) * w[0] + (x[1] - c0[1]) * w[1]).clamp(-reach, reach);
        let open_at = |u: f32| {
            let q = [c0[0] + w[0] * u, c0[1] + w[1] * u, c0[2]];
            self.locate(space, q, 64.0).is_some_and(|t| self.door_tri_open(t, p))
        };
        if !open_at(u0) {
            return None;
        }
        let step = 4.0;
        let (mut lo, mut hi) = (u0, u0);
        while lo - step >= -reach && open_at(lo - step) {
            lo -= step;
        }
        while hi + step <= reach && open_at(hi + step) {
            hi += step;
        }
        let mid = 0.5 * (lo + hi);
        let c = [c0[0] + w[0] * mid, c0[1] + w[1] * mid, c0[2]];
        let opening = hi - lo;
        let ahead = |q: [f32; 3]| (q[0] - c[0]) * n[0] + (q[1] - c[1]) * n[1];
        let on_mesh = |x: f32, y: f32| self.nearest(space, [x, y, c[2]], 48.0);
        let walkable = |a: [f32; 3], b: [f32; 3]| {
            let t = self.locate(space, a, 128.0)?;
            self.walkable_line(space, t, a, b, clear, p)
        };
        let beyond = half + p.radius + crate::doors::STOP_MARGIN;
        let (_, b) = on_mesh(c[0] + n[0] * beyond, c[1] + n[1] * beyond)?;
        let full = half + p.radius + crate::doors::STOP_MARGIN.max(opening);
        let after: Vec<usize> = ((seg + 1)..points.len()).take(16).collect();
        let past = |k: &[usize]| {
            k.iter()
                .copied()
                .find(|&k| ahead(points[k]) >= beyond - 1.0 && walkable(b, points[k]).is_some())
                .or_else(|| k.iter().copied().find(|&k| walkable(b, points[k]).is_some()))
        };
        let hd = |a: [f32; 3], q: [f32; 3]| (a[0] - q[0]).hypot(a[1] - q[1]);
        // Through an open door from a corner within the approach: no back-off.
        if p.open_doors.contains(&door) {
            let near = (0..=seg).rev().take(16).find(|&h| (-full..0.0).contains(&ahead(points[h])) && walkable(points[h], b).is_some());
            if let (Some(h), Some(k)) = (near, past(&after)) {
                let mut out: Vec<[f32; 3]> = points[..=h].to_vec();
                out.push(b);
                if hd(b, points[k]) > 1.0 || k + 1 < points.len() {
                    out.extend_from_slice(&points[k..]);
                }
                return Some((out, h));
            }
        }
        for depth in [full, 0.75 * full, 0.5 * full, beyond] {
            let Some((_, s)) = on_mesh(c[0] - n[0] * depth, c[1] - n[1] * depth) else { continue };
            if walkable(s, b).is_none() {
                continue;
            }
            // Join the polyline: the nearest corner before the door that lies behind the
            // stop (else any before it, backing off to the stop), and the first corner
            // past the far point.
            let before: Vec<usize> = (0..=seg).rev().take(16).collect();
            let h = before
                .iter()
                .copied()
                .find(|&h| ahead(points[h]) <= -depth + 1.0 && walkable(points[h], s).is_some())
                .or_else(|| before.iter().copied().find(|&h| walkable(points[h], s).is_some()));
            let Some(h) = h else { continue };
            let Some(k) = past(&after) else { continue };
            let mut out: Vec<[f32; 3]> = points[..=h].to_vec();
            if hd(points[h], s) > 1.0 {
                out.push(s);
            }
            let stop = out.len() - 1;
            out.push(b);
            if hd(b, points[k]) > 1.0 || k + 1 < points.len() {
                out.extend_from_slice(&points[k..]);
            }
            return Some((out, stop));
        }
        None
    }

    /// The portal of edge `e` of `t`, with each end that is an outline vertex moved in
    /// by `radius` (at most to 45% of the width), so the path keeps clear of walls.
    pub fn shrunk_portal(&self, t: u32, e: usize, radius: f32) -> ([f32; 3], [f32; 3]) {
        let (l, r) = self.portal(t, e);
        let tri = &self.tris[t as usize];
        let len = dist(l, r);
        if len < 1e-3 || radius <= 0.0 {
            return (l, r);
        }
        let s = (radius / len).min(0.45);
        let towards = |a: [f32; 3], b: [f32; 3]| [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, a[2] + (b[2] - a[2]) * s];
        // Ends of a portal that is not this triangle's own whole edge (a seam portal)
        // lie on the edge's interior and are not outline vertices; keep the test on the
        // edge's vertices, which is conservative.
        let right_out = self.boundary_vertex[tri.v[e] as usize];
        let left_out = self.boundary_vertex[tri.v[(e + 1) % 3] as usize];
        let nl = if left_out { towards(l, r) } else { l };
        let nr = if right_out { towards(r, l) } else { r };
        (nl, nr)
    }

    /// Where a leg ending at a door triangle stops: the triangle's point nearest to the
    /// door side, approximated by its centre (the door reference stands at the
    /// triangle's outer edge).
    fn door_approach(&self, t: u32, _from: [f32; 3], _radius: f32) -> [f32; 3] {
        self.centre(t)
    }

    /// Drop corners of a funnelled leg wherever the straight line from a corner to a
    /// later one is walkable ([`NavGraph::walkable_line`], keeping 3/4 of the clearance):
    /// the corridor A* found need not be the best one, and this straightens what the
    /// corridor bent. Looks at most 32 corners ahead.
    fn shortcut(&self, space: Space, start_tri: u32, pts: Vec<[f32; 3]>, p: &PathParams) -> Vec<[f32; 3]> {
        if pts.len() <= 2 {
            return pts;
        }
        let clear = p.radius * 0.75;
        let mut out = vec![pts[0]];
        let (mut i, mut ti) = (0usize, start_tri);
        while i + 1 < pts.len() {
            let mut next = (i + 1, NONE);
            for j in ((i + 2)..pts.len().min(i + 33)).rev() {
                if cuts_into_avoided(&pts[i..=j], p) {
                    continue;
                }
                if let Some(t) = self.walkable_line(space, ti, pts[i], pts[j], clear, p) {
                    next = (j, t);
                    break;
                }
            }
            i = next.0;
            out.push(pts[i]);
            ti = if next.1 != NONE { next.1 } else { self.locate(space, pts[i], 128.0).unwrap_or(ti) };
        }
        out
    }

    /// Whether an actor can walk the straight line from `a` (on triangle `t`) to `b`: it
    /// crosses only edges a path may cross, and keeps `clear` units (seen from above)
    /// from the edges it may not cross, of the triangles it passes and their neighbours.
    /// Returns the triangle under `b`.
    pub fn walkable_line(&self, space: Space, t: u32, a: [f32; 3], b: [f32; 3], clear: f32, p: &PathParams) -> Option<u32> {
        let cur = self.trace_line(space, t, a, b, clear, p)?;
        // The line walks one surface: where it reaches b's spot seen from above, b must
        // be on that surface. A b far above or below is on another level stacked over
        // this one (a catwalk over a floor): not walkable straight.
        ((self.height_on(cur, b[0], b[1]) - b[2]).abs() <= LEVEL_GAP).then_some(cur)
    }

    /// The surface walk of [`NavGraph::walkable_line`]: the triangle under `b`'s spot
    /// seen from above reached along the line from `a` on triangle `t`, whatever height
    /// `b` gives.
    pub(crate) fn trace_line(&self, space: Space, t: u32, a: [f32; 3], b: [f32; 3], clear: f32, p: &PathParams) -> Option<u32> {
        let (mut cur, mut prev) = (t, NONE);
        let mut checked = std::collections::HashSet::new();
        for _ in 0..4096 {
            // Clearance from the outline near this triangle.
            let tri = self.tris[cur as usize];
            for n in std::iter::once(cur).chain(tri.next.iter().copied().filter(|&n| n != NONE)) {
                if !checked.insert(n) {
                    continue;
                }
                let c = self.corners(n);
                for e in 0..3 {
                    if self.passable(n, e, p).is_none() && seg_seg_dist2d(a, b, c[e], c[(e + 1) % 3]) < clear {
                        return None;
                    }
                }
            }
            if self.contains_xy(cur, b[0], b[1]) {
                return Some(cur);
            }
            // Leave by the edge the line crosses furthest along (not back the way in).
            let c = self.corners(cur);
            let mut exit: Option<(f32, usize)> = None;
            for e in 0..3 {
                if prev != NONE && tri.next[e] == prev {
                    continue;
                }
                if let Some(s) = crossing(a, b, c[e], c[(e + 1) % 3]) {
                    if exit.is_none_or(|(best, _)| s > best) {
                        exit = Some((s, e));
                    }
                }
            }
            let (s, e) = exit?;
            let n = self.passable(cur, e, p)?;
            // Across a seam the linked triangle need not be the one facing the crossing:
            // continue on the triangle under the point just past it.
            // Its height is the surface's there, not the line's (which runs straight from
            // a to b, through the air if b is on another level).
            let ahead = (s + 1e-3).min(1.0);
            let mut x = lerp(a, b, ahead);
            x[2] = self.height_on(cur, x[0], x[1]);
            prev = cur;
            cur = if self.contains_xy(n, x[0], x[1]) { n } else { self.locate(space, x, 96.0).filter(|&t| t != prev)? };
        }
        None
    }
}

/// Where along segment a-b (0..1) it crosses segment c-d, seen from above.
fn crossing(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> Option<f32> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let q = [d[0] - c[0], d[1] - c[1]];
    let cross = |u: [f32; 2], v: [f32; 2]| u[0] * v[1] - u[1] * v[0];
    let den = cross(r, q);
    if den.abs() < 1e-9 {
        return None;
    }
    let w = [c[0] - a[0], c[1] - a[1]];
    let s = cross(w, q) / den;
    let u = cross(w, r) / den;
    let eps = 1e-4;
    ((-eps..=1.0 + eps).contains(&s) && (-eps..=1.0 + eps).contains(&u)).then_some(s)
}

/// Distance between segments a-b and c-d, seen from above.
fn seg_seg_dist2d(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> f32 {
    if crossing(a, b, c, d).is_some() {
        return 0.0;
    }
    let pt_seg = |p: [f32; 3], s0: [f32; 3], s1: [f32; 3]| {
        let (dx, dy) = (s1[0] - s0[0], s1[1] - s0[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 < 1e-12 { 0.0 } else { (((p[0] - s0[0]) * dx + (p[1] - s0[1]) * dy) / l2).clamp(0.0, 1.0) };
        (p[0] - s0[0] - t * dx).hypot(p[1] - s0[1] - t * dy)
    };
    pt_seg(a, c, d).min(pt_seg(b, c, d)).min(pt_seg(c, a, b)).min(pt_seg(d, a, b))
}

/// The funnel ("string pulling") algorithm over portals given as (left, right) seen
/// along the path, in the horizontal plane; corner heights come from the portal ends.
/// After Mikko Mononen's "Simple Stupid Funnel Algorithm" (2010, public description).
pub fn funnel(start: [f32; 3], end: [f32; 3], mut portals: &[([f32; 3], [f32; 3])]) -> Vec<[f32; 3]> {
    // A start (or end) lying on a portal would open the funnel to 180 degrees, which the
    // algorithm cannot represent: such a portal is already passed (or reached).
    let on = |p: [f32; 3], (l, r): ([f32; 3], [f32; 3])| {
        let len = (l[0] - r[0]).hypot(l[1] - r[1]);
        len < 1e-4 || area2(l, r, p).abs() <= 1e-3 * len
    };
    while let Some((&first, rest)) = portals.split_first() {
        if !on(start, first) {
            break;
        }
        portals = rest;
    }
    while let Some((&last, rest)) = portals.split_last() {
        if !on(end, last) {
            break;
        }
        portals = rest;
    }
    let mut pts = vec![start];
    let mut all: Vec<([f32; 3], [f32; 3])> = Vec::with_capacity(portals.len() + 2);
    all.push((start, start));
    all.extend_from_slice(portals);
    all.push((end, end));
    let same = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
    // area2(apex, a, b) > 0 when b is to the left of apex→a (counter-clockwise).
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut left_i, mut right_i) = (0usize, 0usize);
    let mut i = 1;
    while i < all.len() {
        let (l, r) = all[i];
        // Right side: r must not cross to the right of the current right edge.
        if area2(apex, right, r) >= 0.0 {
            if same(apex, right) || area2(apex, left, r) < 0.0 {
                right = r;
                right_i = i;
            } else {
                // r crosses over the left edge: left becomes a corner.
                pts.push(left);
                apex = left;
                right = apex;
                right_i = left_i;
                i = left_i + 1;
                continue;
            }
        }
        if area2(apex, left, l) <= 0.0 {
            if same(apex, left) || area2(apex, right, l) > 0.0 {
                left = l;
                left_i = i;
            } else {
                pts.push(right);
                apex = right;
                left = apex;
                left_i = right_i;
                i = right_i + 1;
                continue;
            }
        }
        i += 1;
    }
    if !same(*pts.last().expect("start"), end) || pts.len() == 1 {
        pts.push(end);
    }
    pts
}

/// Whether the straight line from the first of `way` to its last enters a place to avoid
/// ([`PathParams::avoid`], seen from above) that the way itself keeps out of: the
/// shortcut pass may not undo the search's way round it.
fn cuts_into_avoided(way: &[[f32; 3]], p: &PathParams) -> bool {
    let (a, b) = (way[0], way[way.len() - 1]);
    p.avoid.iter().any(|(c, r)| {
        let near = |q: [f32; 3]| (q[0] - c[0]).hypot(q[1] - c[1]) <= *r;
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        let s = if len2 > 1e-8 { (((c[0] - a[0]) * dx + (c[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        near([a[0] + dx * s, a[1] + dy * s, 0.0]) && !way.iter().any(|&q| near(q))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_corridor_is_one_segment() {
        // Portals across the x axis at x = 1, 2, 3 (left is +y when walking along +x).
        let portals: Vec<_> = (1..4).map(|x| ([x as f32, 1.0, 0.0], [x as f32, -1.0, 0.0])).collect();
        let p = funnel([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], &portals);
        assert_eq!(p, vec![[0.0, 0.0, 0.0], [4.0, 0.0, 0.0]]);
    }

    #[test]
    fn corner_is_pulled_to_the_inside_vertex() {
        // Walk east, then north round a corner at (2, 1): portals east up to x = 2, then
        // portals north from y = 1.
        let portals = vec![
            ([1.0, 1.0, 0.0], [1.0, -1.0, 0.0]),
            ([2.0, 1.0, 5.0], [2.0, -1.0, 0.0]),
            ([2.0, 1.0, 5.0], [4.0, 1.0, 0.0]),
            ([2.0, 2.0, 0.0], [4.0, 2.0, 0.0]),
        ];
        let p = funnel([0.0, 0.0, 0.0], [3.0, 4.0, 0.0], &portals);
        assert_eq!(p.len(), 3, "{p:?}");
        assert_eq!(p[1], [2.0, 1.0, 5.0]);
    }

    #[test]
    fn empty_corridor() {
        assert_eq!(funnel([0.0; 3], [1.0, 1.0, 1.0], &[]), vec![[0.0; 3], [1.0, 1.0, 1.0]]);
        assert_eq!(funnel([1.0; 3], [1.0; 3], &[]), vec![[1.0; 3], [1.0; 3]]);
    }
}
