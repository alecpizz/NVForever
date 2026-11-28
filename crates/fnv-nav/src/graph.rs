//! The nav graph of a load order: every navmesh's triangles in one array, each edge
//! resolved to the triangle beyond it (in the same navmesh, or across a cell seam through
//! an NVEX edge link), doors joining spaces, and a bucket index per space for finding the
//! triangle under a point.
//!
//! A *space* is a worldspace (its exterior cells and persistent cell) or one interior
//! cell. Coordinates are only comparable within a space.

use crate::record::{link_kind, tri_flags, NavMeshRecord};
use crate::NavRecords;
use fnv_esm::{LoadOrder, Schema};
use fnv_world::fields::{vec3, Fields};
use fnv_world::index::global_of;
use fnv_world::WorldIndex;
use std::collections::HashMap;

/// No triangle.
pub const NONE: u32 = u32::MAX;
/// Side of the square buckets of the point index, game units [engine choice].
pub const BUCKET: f32 = 512.0;
/// Farthest a navmesh vertex may lie from the origin on any axis: far past any worldspace
/// (cells are 4,096 units square, docs/spec-notes/world.md, so this is over 2,000
/// cells out), so a vertex past it is broken data [engine choice].
pub const MAX_COORD: f32 = 1.0e7;
/// Widest a triangle may be on any axis: 16 exterior cells (docs/spec-notes/world.md);
/// a navmesh belongs to one cell, so a triangle this wide is broken data [engine
/// choice]. Without the limit its point-index buckets could number in the billions.
pub const MAX_TRI_SPAN: f32 = 65_536.0;
/// Largest radius a point-index query searches ([`NavGraph::triangles_near`],
/// [`NavGraph::nearest`]); larger ones (a huge scale on an obstacle, a mod's game
/// setting) are cut to this [engine choice].
pub const MAX_QUERY_RADIUS: f32 = 65_536.0;

/// [`NavGraph::step_between`]: how far apart the points along an outline edge are looked at.
pub const STEP_SAMPLE: f32 = 8.0;
/// [`NavGraph::step_between`]: how far into its triangle the point stepped onto is moved.
pub const STEP_IN: f32 = 16.0;

/// A worldspace or an interior cell (load-order FormIDs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub enum Space {
    World(u32),
    Interior(u32),
}

/// How an edge leads on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum EdgeKind {
    /// No triangle beyond (a boundary), or a link whose target is not loaded.
    Boundary,
    /// A triangle of the same navmesh.
    Internal,
    /// A triangle of another navmesh across a portal link (NVEX type 0 or 3).
    Portal,
    /// A ledge link (NVEX type 1, up, or 2, down): the triangle beyond is far above or
    /// below (docs/spec-notes/navmesh.md §3).
    LedgeUp,
    LedgeDown,
}

/// One triangle of the graph.
#[derive(Debug, Clone, Copy)]
pub struct NavTri {
    /// Index into [`NavGraph::meshes`].
    pub mesh: u32,
    /// Global vertex indices (into [`NavGraph::vertices`]).
    pub v: [u32; 3],
    /// The triangle beyond each edge (0: v0-v1, 1: v1-v2, 2: v2-v0), or [`NONE`].
    pub next: [u32; 3],
    pub kind: [EdgeKind; 3],
    pub flags: u16,
    /// Listed in its navmesh's NVCA (cover triangles); every listed triangle has a
    /// nonzero NVTR cover field [observed: FalloutNV.esm, `fnvtool nav survey`, 15,791 of
    /// 15,791]. What the field's bits mean is not known (docs/spec-notes/navmesh.md §2).
    pub cover: bool,
}

/// An edge of a cover triangle on the outline of the navmesh ([`NavGraph::cover_edges_near`]):
/// what stands beyond it (a rock, a wall, a car) is cover [hypothesis: the GECK's "Find
/// Cover Edges" marks the outside edges by the height of what stands there; the edges
/// of a listed triangle that lead nowhere are those].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverEdge {
    pub tri: u32,
    pub a: [f32; 3],
    pub b: [f32; 3],
    /// The edge's middle.
    pub mid: [f32; 3],
    /// Unit vector across the edge, out of the navmesh (towards what stands there),
    /// horizontal.
    pub out: [f32; 2],
}

impl CoverEdge {
    /// The edge's length, horizontally.
    pub fn length(&self) -> f32 {
        (self.b[0] - self.a[0]).hypot(self.b[1] - self.a[1])
    }

    /// Whether `from` (a threat) is on the far side of the edge's line: the cover stands
    /// between it and someone at the edge [design].
    pub fn faces(&self, from: [f32; 3]) -> bool {
        (from[0] - self.mid[0]) * self.out[0] + (from[1] - self.mid[1]) * self.out[1] > 0.0
    }

    /// A place to stand behind the edge, `back` units in from its middle (on the
    /// navmesh side), at the edge's height.
    pub fn behind(&self, back: f32) -> [f32; 3] {
        [self.mid[0] - self.out[0] * back, self.mid[1] - self.out[1] * back, self.mid[2]]
    }
}

impl NavTri {
    pub fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }
}

/// One navmesh of the graph.
#[derive(Debug, Clone)]
pub struct NavMesh {
    pub form_id: u32,
    pub cell: Option<u32>,
    pub space: Option<Space>,
    /// Its triangles are `first_tri .. first_tri + tri_count` of the graph.
    pub first_tri: u32,
    pub tri_count: u32,
    pub first_vertex: u32,
    /// Disabled navmeshes take no part in paths (NAVM "Initially Disabled";
    /// `DisableNavmesh` / `EnableNavMesh` change it at run time).
    pub enabled: bool,
    /// Its state as the load order has it (not "Initially Disabled"), before any switch
    /// ([`NavGraph::apply_switches`]).
    pub initially_enabled: bool,
}

/// A door that joins two triangles (usually in two spaces).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct DoorEdge {
    /// The door reference next to the triangle the edge starts from.
    pub door: u32,
    /// The door reference it leads to (XTEL).
    pub to_door: u32,
    /// Where an actor using the door arrives (XTEL position) and its heading (XTEL z
    /// rotation, radians).
    pub arrival: [f32; 3],
    pub arrival_heading: f32,
    /// The triangle of the arrival.
    pub to: u32,
    /// Where the door reference itself stands, when known ([`NavGraph::place_doors`]):
    /// an actor turns to it before going through (`Walker::face_door`).
    pub door_at: Option<[f32; 3]>,
}

/// How near two edges' ends are to count as the same edge, game units [engine choice].
pub const EDGE_MATCH: f32 = 1.0;

/// Counts of what the graph could and could not resolve while building.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BuildStats {
    pub navmeshes: usize,
    pub triangles: usize,
    pub vertices: usize,
    pub internal_edges: usize,
    pub portal_edges: usize,
    pub ledge_edges: usize,
    pub boundary_edges: usize,
    /// Linked edges whose NVEX entry, navmesh or triangle is missing.
    pub unresolved_links: usize,
    /// One-way portals linked back from the edge they land on ([`NavGraph::join_one_way_portals`]).
    pub portals_joined_back: usize,
    /// Triangles left out because their corners are not finite, lie past [`MAX_COORD`]
    /// or spread wider than [`MAX_TRI_SPAN`].
    pub bad_triangles: usize,
    pub navmeshes_without_space: usize,
    pub doors: usize,
    /// Door links whose door has no teleport, or whose destination has no triangle.
    pub doors_unresolved: usize,
    /// Doors that open in place standing on the navmesh (`crate::doors`).
    pub in_place_doors: usize,
}

/// The nav graph.
#[derive(Debug, Default)]
pub struct NavGraph {
    pub meshes: Vec<NavMesh>,
    pub tris: Vec<NavTri>,
    pub vertices: Vec<[f32; 3]>,
    /// Vertices on the walkable area's outline: on an edge that leads nowhere an actor
    /// can walk ([`EdgeKind::Boundary`] or a ledge). Paths keep their clearance from these.
    pub boundary_vertex: Vec<bool>,
    /// Navmesh FormID -> index into `meshes`.
    pub by_form: HashMap<u32, u32>,
    /// Doors leaving each triangle.
    pub doors: HashMap<u32, Vec<DoorEdge>>,
    /// Lock data of door references (authored XLOC; the game updates it as doors are
    /// unlocked, [`NavGraph::set_door_lock`]).
    pub door_locks: HashMap<u32, DoorLock>,
    /// Doors that are not in the world now (disabled, or deleted). No path goes through
    /// such a load door ([`NavGraph::door_open`]); such a door that opens in place bars
    /// nothing and gets no stop, as if it were not placed (#2405: Point Lookout's
    /// mansion has a second, initially disabled and locked door in the doorway of the
    /// one Desmond's quest opens, `DLC04SideDoubleDoor02REF`, and it closed his way to
    /// the player). Built from the
    /// load order's enable state ([`NavGraph::build`]); the game brings it in line as
    /// scripts enable and disable references ([`NavGraph::sync_door_presence`]). Nellis's
    /// hangar has two pairs of doors on the same spot, one into each version of the
    /// hangar, and only one pair is enabled at a time (issue #2566).
    pub disabled_doors: std::collections::HashSet<u32>,
    /// The up component of each triangle's unit normal (1 level, 0 vertical): the cosine
    /// of its slope.
    pub up: Vec<f32>,
    /// The walkable area of each triangle ([`NONE`] for unusable ones): triangles joined
    /// by internal and portal edges, within one space. Doors join areas.
    pub area: Vec<u32>,
    /// The space of each area.
    pub area_space: Vec<Space>,
    /// Doors leaving each area: (triangle the door is next to, index into `doors` of it).
    pub area_doors: HashMap<u32, Vec<(u32, usize)>>,
    /// Doors that open in place (no XTEL), by reference ([`crate::doors`]).
    pub in_place: HashMap<u32, crate::doors::InPlaceDoor>,
    /// The doors that open in place standing on each triangle.
    pub door_tris: HashMap<u32, Vec<u32>>,
    /// Obstacles that can come and go, by reference ([`crate::obstacles`]).
    pub obstacles: HashMap<u32, crate::obstacles::ObstacleCut>,
    /// A number no other obstacle state of this process had, taken anew at every change
    /// of `obstacles` ([`NavGraph::obstacles_stamp`]); 0 while none was learnt.
    pub(crate) obstacles_stamp: u64,
    /// How many enabled obstacles cut each triangle; empty when none does. A cut
    /// triangle takes no part in paths ([`NavGraph::usable`]).
    pub cut: Vec<u16>,
    /// Triangle lists of each space's buckets ([`BUCKET`]-unit squares).
    buckets: HashMap<Space, HashMap<(i32, i32), Vec<u32>>>,
    pub stats: BuildStats,
}

/// A door's lock (XLOC) [documented: xEdit REFR XLOC]. A door with lock data starts
/// locked [hypothesis, as `fnv_sim::world` assumes].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct DoorLock {
    pub level: u8,
    /// The key that opens it.
    pub key: Option<u32>,
}

/// What [`NavGraph::sync_door_locks`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoorLockChanges {
    /// Doors no longer locked.
    pub unlocked: Vec<u32>,
    /// Doors locked now, or locked differently (level, key).
    pub locked: Vec<u32>,
}

impl DoorLockChanges {
    pub fn is_empty(&self) -> bool {
        self.unlocked.is_empty() && self.locked.is_empty()
    }
}

/// What [`NavGraph::sync_door_presence`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoorPresenceChanges {
    /// Doors no longer in the world (disabled, deleted).
    pub gone: Vec<u32>,
    /// Doors back in it (enabled).
    pub back: Vec<u32>,
}

impl DoorPresenceChanges {
    pub fn is_empty(&self) -> bool {
        self.gone.is_empty() && self.back.is_empty()
    }
}

/// Whether a reference is enabled as the load order places it: its own "Initially
/// Disabled" flag, or, when it has an enable parent (XESP), the state at the end of its
/// chain of parents, turned over by each "opposite" link [documented: xEdit
/// wbDefinitionsFNV.pas REFR XESP "Set Enable State to Opposite of Parent"; the same
/// rule as `fnv_sim::World::is_enabled` for a reference no script has touched]. A
/// deleted reference is not there; a chain that loops, or names a reference the load
/// order lacks, leaves the reference's own flag.
pub fn authored_enabled(lo: &LoadOrder, schema: &Schema, id: u32) -> bool {
    let Some(p) = fnv_world::placed::placement(lo, schema, id) else { return true };
    if p.deleted() {
        return false;
    }
    let own = !p.initially_disabled();
    let (mut cur, mut invert, mut seen) = (p, false, vec![id]);
    while let Some((parent, opposite)) = cur.enable_parent {
        let Some(next) = fnv_world::placed::placement(lo, schema, parent).filter(|_| !seen.contains(&parent)) else { return own };
        seen.push(parent);
        invert ^= opposite;
        cur = next;
    }
    !cur.initially_disabled() ^ invert
}

/// A door reference's lock data, if any.
pub fn door_lock(lo: &LoadOrder, schema: &Schema, door: u32) -> Option<DoorLock> {
    let (pi, r) = fnv_world::fields::winner(lo, door)?;
    let f = Fields::new(r, pi, schema).ok()?;
    let x = f.get(b"XLOC")?;
    let level = x.get("level").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
    let key = x.get("key").and_then(|k| global_of(lo, pi, k));
    Some(DoorLock { level, key })
}

/// A door reference's teleport: destination door, position, z rotation.
pub fn door_teleport(lo: &LoadOrder, schema: &Schema, door: u32) -> Option<(u32, [f32; 3], f32)> {
    let (pi, r) = fnv_world::fields::winner(lo, door)?;
    let f = Fields::new(r, pi, schema).ok()?;
    let x = f.get(b"XTEL")?;
    let to = global_of(lo, pi, x.get("door")?)?;
    let pr = x.get("pos_rot")?;
    let pos = vec3(pr.get("position")?)?;
    let rot = vec3(pr.get("rotation_radians")?)?;
    Some((to, pos, rot[2]))
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Twice the signed area of (a, b, c) seen from above: positive when counter-clockwise.
pub fn area2(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let (u, v) = (sub(b, a), sub(c, a));
    u[0] * v[1] - v[0] * u[1]
}

impl NavGraph {
    /// Build the graph of every navmesh in `records`, placing each in the space of its
    /// cell (`ix`), and join doors through their teleports (read with `schema`).
    pub fn build(lo: &LoadOrder, schema: &Schema, ix: &WorldIndex, records: &NavRecords) -> NavGraph {
        let space_of = |cell: Option<u32>| -> Option<Space> {
            let c = ix.cells.get(&cell?)?;
            Some(match c.world {
                Some(w) => Space::World(w),
                None => Space::Interior(c.form_id),
            })
        };
        let input: Vec<(&NavMeshRecord, Option<u32>, Option<Space>)> = records
            .navmeshes
            .values()
            .map(|m| {
                let cell = m.cell.or_else(|| records.stored_in.get(&m.form_id).copied());
                (m, cell, space_of(cell))
            })
            .collect();
        let mut g = NavGraph::assemble(&input);
        g.join_doors(&input, |door| {
            let (to_door, arrival, heading) = door_teleport(lo, schema, door)?;
            let space = ix.refs.get(&to_door).and_then(|r| space_of(Some(r.cell)))?;
            Some((to_door, arrival, heading, space))
        });
        g.place_doors(|d| fnv_world::placed::placement(lo, schema, d).map(|p| p.position));
        let doors: Vec<u32> = g.doors.values().flatten().map(|d| d.door).collect();
        for d in doors {
            if let Some(l) = door_lock(lo, schema, d) {
                g.door_locks.insert(d, l);
            }
        }
        g.learn_in_place_doors(lo, schema, ix);
        // Load doors and doors that open in place alike (#2405): one disabled as placed
        // is not there.
        let doors = g.door_refs();
        g.disabled_doors = doors.into_iter().filter(|&d| !authored_enabled(lo, schema, d)).collect();
        g.learn_obstacles(lo, schema, ix);
        g
    }

    /// The graph of these navmeshes (each with its cell and space), without doors.
    pub fn assemble(input: &[(&NavMeshRecord, Option<u32>, Option<Space>)]) -> NavGraph {
        let mut g = NavGraph::default();
        let recs: Vec<&NavMeshRecord> = input.iter().map(|i| i.0).collect();
        // Meshes, vertices and triangles (edges resolved below).
        for &(m, cell, space) in input {
            g.stats.navmeshes_without_space += space.is_none() as usize;
            let mi = g.meshes.len() as u32;
            g.by_form.insert(m.form_id, mi);
            let first_vertex = g.vertices.len() as u32;
            let nv = m.vertices.len() as u32;
            g.vertices.extend_from_slice(&m.vertices);
            let first_tri = g.tris.len() as u32;
            for t in &m.triangles {
                let ok = t.v.iter().all(|&v| (v as u32) < nv);
                // Corners not finite, far out or spread over many cells: broken data.
                let sane = ok && sane_corners(t.v.map(|v| m.vertices[v as usize]));
                g.stats.bad_triangles += (ok && !sane) as usize;
                let ok = ok && sane;
                g.tris.push(NavTri {
                    mesh: mi,
                    v: if ok { t.v.map(|v| first_vertex + v as u32) } else { [first_vertex; 3] },
                    next: [NONE; 3],
                    kind: [EdgeKind::Boundary; 3],
                    // A triangle with a bad vertex index or broken corners is left out
                    // of paths (and of the point index).
                    flags: if ok { t.flags } else { t.flags | tri_flags::DELETED },
                    cover: false,
                });
            }
            for &c in &m.cover_triangles {
                if let Some(t) = g.tris.get_mut((first_tri + c as u32) as usize).filter(|_| (c as usize) < m.triangles.len()) {
                    t.cover = true;
                }
            }
            g.meshes.push(NavMesh {
                form_id: m.form_id,
                cell,
                space,
                first_tri,
                tri_count: m.triangles.len() as u32,
                first_vertex,
                enabled: !m.initially_disabled(),
                initially_enabled: !m.initially_disabled(),
            });
        }
        // Edges.
        for (mi, m) in recs.iter().enumerate() {
            let first = g.meshes[mi].first_tri;
            for (ti, t) in m.triangles.iter().enumerate() {
                let gi = (first + ti as u32) as usize;
                for e in 0..3 {
                    let v = t.edges[e];
                    let (next, kind) = if t.edge_is_link(e) {
                        let target = (v >= 0)
                            .then(|| m.edge_links.get(v as usize))
                            .flatten()
                            .and_then(|l| {
                                let om = *g.by_form.get(&l.navmesh?)?;
                                let o = &g.meshes[om as usize];
                                ((l.triangle as u32) < o.tri_count).then_some((o.first_tri + l.triangle as u32, l.kind))
                            });
                        match target {
                            None => {
                                g.stats.unresolved_links += 1;
                                (NONE, EdgeKind::Boundary)
                            }
                            Some((n, link_kind::LEDGE_UP)) => (n, EdgeKind::LedgeUp),
                            Some((n, link_kind::LEDGE_DOWN)) => (n, EdgeKind::LedgeDown),
                            Some((n, _)) => (n, EdgeKind::Portal),
                        }
                    } else if v >= 0 && (v as usize) < m.triangles.len() {
                        (first + v as u32, EdgeKind::Internal)
                    } else {
                        (NONE, EdgeKind::Boundary)
                    };
                    match kind {
                        EdgeKind::Boundary => g.stats.boundary_edges += 1,
                        EdgeKind::Internal => g.stats.internal_edges += 1,
                        EdgeKind::Portal => g.stats.portal_edges += 1,
                        EdgeKind::LedgeUp | EdgeKind::LedgeDown => g.stats.ledge_edges += 1,
                    }
                    g.tris[gi].next[e] = next;
                    g.tris[gi].kind[e] = kind;
                }
            }
        }
        g.join_one_way_portals();
        // Outline vertices.
        g.boundary_vertex = vec![false; g.vertices.len()];
        for t in &g.tris {
            // A triangle with a bad vertex index (deleted above) has no vertices of its own:
            // in a navmesh with triangles but no vertices its index is past the end.
            if t.has(tri_flags::DELETED) {
                continue;
            }
            for e in 0..3 {
                if !matches!(t.kind[e], EdgeKind::Internal | EdgeKind::Portal) {
                    g.boundary_vertex[t.v[e] as usize] = true;
                    g.boundary_vertex[t.v[(e + 1) % 3] as usize] = true;
                }
            }
        }
        // Buckets.
        for (ti, t) in g.tris.iter().enumerate() {
            if t.has(tri_flags::DELETED) {
                continue;
            }
            let Some(space) = g.meshes[t.mesh as usize].space else { continue };
            let p = t.v.map(|v| g.vertices[v as usize]);
            let lo_x = p.iter().map(|q| q[0]).fold(f32::INFINITY, f32::min);
            let hi_x = p.iter().map(|q| q[0]).fold(f32::NEG_INFINITY, f32::max);
            let lo_y = p.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min);
            let hi_y = p.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max);
            let b = g.buckets.entry(space).or_default();
            for by in bucket(lo_y)..=bucket(hi_y) {
                for bx in bucket(lo_x)..=bucket(hi_x) {
                    b.entry((bx, by)).or_default().push(ti as u32);
                }
            }
        }
        g.stats.navmeshes = g.meshes.len();
        g.stats.triangles = g.tris.len();
        g.stats.vertices = g.vertices.len();
        g.up = (0..g.tris.len() as u32).map(|t| slope_cos(g.corners(t))).collect();
        g.compute_areas();
        g
    }

    /// A portal from one triangle onto an edge of another that does not lead back: the
    /// edge it lands on, when it is the same edge (both ends within [`EDGE_MATCH`]) and
    /// is open (no neighbour) or links to a triangle that has no such edge (a stale
    /// link), is linked back [engine choice]. Lonesome Road's silo lift: the far door's
    /// one-triangle navmesh 0x0100A556 links onto the lift floor (0x0100A555 triangles 12
    /// and 15), whose edges there link to triangle 16 of 0x0100A554, far off; the door
    /// was an island no path reached (observed 2026-11-15, `fnvtool nav near`).
    fn join_one_way_portals(&mut self) {
        let ends = |g: &NavGraph, t: u32, e: usize| {
            let v = g.tris[t as usize].v;
            (g.vertices[v[e] as usize], g.vertices[v[(e + 1) % 3] as usize])
        };
        let same = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).abs() <= EDGE_MATCH && (a[1] - b[1]).abs() <= EDGE_MATCH && (a[2] - b[2]).abs() <= EDGE_MATCH;
        let meets = |g: &NavGraph, (p, q): ([f32; 3], [f32; 3]), t: u32, e: usize| {
            let (a, b) = ends(g, t, e);
            (same(p, a) && same(q, b)) || (same(p, b) && same(q, a))
        };
        let mut fixes: Vec<(u32, usize, u32)> = Vec::new();
        for t in 0..self.tris.len() as u32 {
            let tri = self.tris[t as usize];
            if tri.has(tri_flags::DELETED) {
                continue;
            }
            for e in 0..3 {
                let n = tri.next[e];
                if tri.kind[e] != EdgeKind::Portal || n == NONE || self.tris[n as usize].next.contains(&t) || self.tris[n as usize].has(tri_flags::DELETED) {
                    continue;
                }
                let edge = ends(self, t, e);
                let Some(ne) = (0..3).find(|&ne| meets(self, edge, n, ne)) else { continue };
                let nt = self.tris[n as usize];
                let stale = match nt.kind[ne] {
                    EdgeKind::Boundary => true,
                    EdgeKind::Portal => nt.next[ne] == NONE || !(0..3).any(|me| meets(self, edge, nt.next[ne], me)),
                    _ => false,
                };
                if stale {
                    fixes.push((n, ne, t));
                }
            }
        }
        for (n, ne, t) in fixes {
            self.tris[n as usize].next[ne] = t;
            self.tris[n as usize].kind[ne] = EdgeKind::Portal;
            self.stats.portals_joined_back += 1;
        }
    }

    /// Label walkable areas (flood fill over internal and portal edges between usable
    /// triangles) and index the doors leaving each. Run again after enabling or disabling
    /// navmeshes or joining doors.
    pub fn compute_areas(&mut self) {
        self.area = vec![NONE; self.tris.len()];
        self.area_space.clear();
        let mut stack = Vec::new();
        for s in 0..self.tris.len() as u32 {
            if self.area[s as usize] != NONE || !self.usable(s) {
                continue;
            }
            let Some(space) = self.space_of_tri(s) else { continue };
            let id = self.area_space.len() as u32;
            self.area_space.push(space);
            self.area[s as usize] = id;
            stack.push(s);
            while let Some(t) = stack.pop() {
                let tri = self.tris[t as usize];
                for e in 0..3 {
                    let n = tri.next[e];
                    if matches!(tri.kind[e], EdgeKind::Internal | EdgeKind::Portal) && n != NONE && self.area[n as usize] == NONE && self.usable(n) {
                        self.area[n as usize] = id;
                        stack.push(n);
                    }
                }
            }
        }
        self.area_doors.clear();
        for (&from, list) in &self.doors {
            let a = self.area[from as usize];
            if a == NONE {
                continue;
            }
            for i in 0..list.len() {
                self.area_doors.entry(a).or_default().push((from, i));
            }
        }
        for v in self.area_doors.values_mut() {
            v.sort_unstable();
        }
    }

    /// Join doors: from each NVDP triangle to the triangle where the door's teleport
    /// arrives (the destination door's own NVDP triangle when it has one in the
    /// destination space, else the triangle under the arrival point). `teleport` gives a
    /// door reference's destination door, arrival point, arrival heading and the
    /// destination's space.
    pub fn join_doors(
        &mut self,
        input: &[(&NavMeshRecord, Option<u32>, Option<Space>)],
        teleport: impl Fn(u32) -> Option<(u32, [f32; 3], f32, Space)>,
    ) {
        let mut door_tri: HashMap<u32, u32> = HashMap::new();
        for &(m, _, _) in input {
            let Some(&mi) = self.by_form.get(&m.form_id) else { continue };
            let mesh = &self.meshes[mi as usize];
            for d in &m.door_links {
                if (d.triangle as u32) < mesh.tri_count {
                    door_tri.entry(d.door).or_insert(mesh.first_tri + d.triangle as u32);
                }
            }
        }
        let mut doors: Vec<(u32, DoorEdge)> = Vec::new();
        for (&door, &from) in &door_tri {
            let edge = teleport(door).and_then(|(to_door, arrival, heading, space)| {
                let to = door_tri
                    .get(&to_door)
                    .copied()
                    .filter(|&t| self.space_of_tri(t) == Some(space))
                    .or_else(|| self.locate(space, arrival, 256.0))?;
                Some(DoorEdge { door, to_door, arrival, arrival_heading: heading, to, door_at: None })
            });
            match edge {
                Some(e) => doors.push((from, e)),
                None => self.stats.doors_unresolved += 1,
            }
        }
        doors.sort_by_key(|(from, e)| (*from, e.door));
        self.stats.doors = doors.len();
        self.doors.clear();
        for (from, e) in doors {
            self.doors.entry(from).or_default().push(e);
        }
        self.compute_areas();
    }

    /// Record where each joined door's reference stands (`position` of a door reference),
    /// for actors to face it before they use it.
    pub fn place_doors(&mut self, position: impl Fn(u32) -> Option<[f32; 3]>) {
        for e in self.doors.values_mut().flatten() {
            e.door_at = position(e.door);
        }
    }

    /// Set (or clear) a door's lock: the game calls this when a door is unlocked or
    /// locked at run time.
    pub fn set_door_lock(&mut self, door: u32, lock: Option<DoorLock>) {
        match lock {
            Some(l) => self.door_locks.insert(door, l),
            None => self.door_locks.remove(&door),
        };
    }

    /// Every door reference the graph knows: the load doors leaving its triangles and
    /// the doors that open in place, in order.
    pub fn door_refs(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.doors.values().flatten().map(|d| d.door).chain(self.in_place.keys().copied()).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Bring door locks in line with the game's: `states` gives doors whose state the
    /// game holds (each `Some` while locked: a script's Lock or Unlock, a key used, a
    /// lock picked); doors it does not name keep the lock they have. Doors the graph does
    /// not know ([`NavGraph::door_refs`]) are passed over. Returns the doors that are no
    /// longer locked and those that became locked or changed their lock.
    pub fn sync_door_locks(&mut self, states: impl IntoIterator<Item = (u32, Option<DoorLock>)>) -> DoorLockChanges {
        let known = self.door_refs();
        let mut out = DoorLockChanges::default();
        for (d, now) in states {
            if known.binary_search(&d).is_err() || self.door_locks.get(&d) == now.as_ref() {
                continue;
            }
            match now {
                Some(l) => {
                    self.door_locks.insert(d, l);
                    out.locked.push(d);
                }
                None => {
                    self.door_locks.remove(&d);
                    out.unlocked.push(d);
                }
            }
        }
        out
    }

    /// The load doors of the graph (those with a teleport that leave a triangle), in
    /// order, each once.
    pub fn door_refs_through(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.doors.values().flatten().map(|d| d.door).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Bring the doors' presence (load doors and doors that open in place) in line with
    /// the game's: `states` gives each door and whether it is in the world now (enabled,
    /// with its enable parents, and not deleted). Doors the graph does not know
    /// ([`NavGraph::door_refs`]) are passed over. Returns the doors gone from the world
    /// and those back in it.
    pub fn sync_door_presence(&mut self, states: impl IntoIterator<Item = (u32, bool)>) -> DoorPresenceChanges {
        let mut out = DoorPresenceChanges::default();
        let known = self.door_refs();
        for (d, present) in states {
            if known.binary_search(&d).is_err() {
                continue;
            }
            if present && self.disabled_doors.remove(&d) {
                out.back.push(d);
            } else if !present && self.disabled_doors.insert(d) {
                out.gone.push(d);
            }
        }
        out
    }

    /// The three corners of a triangle.
    pub fn corners(&self, t: u32) -> [[f32; 3]; 3] {
        self.tris[t as usize].v.map(|v| self.vertex(v))
    }

    /// A vertex by index; the origin for an index past the end (only a deleted triangle
    /// of a navmesh without vertices has one).
    fn vertex(&self, v: u32) -> [f32; 3] {
        self.vertices.get(v as usize).copied().unwrap_or_default()
    }

    pub fn centre(&self, t: u32) -> [f32; 3] {
        let c = self.corners(t);
        [0, 1, 2].map(|k| (c[0][k] + c[1][k] + c[2][k]) / 3.0)
    }

    pub fn space_of_tri(&self, t: u32) -> Option<Space> {
        self.meshes[self.tris[t as usize].mesh as usize].space
    }

    pub fn mesh_of(&self, t: u32) -> &NavMesh {
        &self.meshes[self.tris[t as usize].mesh as usize]
    }

    /// Whether paths may cross triangle `t`: not deleted, its navmesh enabled, and no
    /// enabled obstacle cuts it ([`crate::obstacles`]).
    pub fn usable(&self, t: u32) -> bool {
        let tri = &self.tris[t as usize];
        !tri.has(tri_flags::DELETED) && self.meshes[tri.mesh as usize].enabled && self.cut.get(t as usize).is_none_or(|&c| c == 0)
    }

    /// Enable or disable a navmesh (`EnableNavMesh` / `DisableNavmesh`). Returns false
    /// when it is not in the graph.
    pub fn set_enabled(&mut self, navmesh: u32, enabled: bool) -> bool {
        match self.by_form.get(&navmesh) {
            Some(&m) => {
                if self.meshes[m as usize].enabled != enabled {
                    self.meshes[m as usize].enabled = enabled;
                    self.compute_areas();
                }
                true
            }
            None => false,
        }
    }

    /// Set every navmesh as `switches` has it (navmesh FormID → enabled; the scripts'
    /// `EnableNavMesh` / `DisableNavmesh`), the others as the load order has them.
    /// Returns whether any navmesh changed.
    pub fn apply_switches(&mut self, switches: &std::collections::BTreeMap<u32, bool>) -> bool {
        let mut changed = false;
        for m in &mut self.meshes {
            let want = switches.get(&m.form_id).copied().unwrap_or(m.initially_enabled);
            if m.enabled != want {
                m.enabled = want;
                changed = true;
            }
        }
        if changed {
            self.compute_areas();
        }
        changed
    }

    /// Height of triangle `t`'s plane at (x, y).
    pub fn height_on(&self, t: u32, x: f32, y: f32) -> f32 {
        let [a, b, c] = self.corners(t);
        let d = area2(a, b, c);
        if d.abs() < 1e-6 {
            return (a[2] + b[2] + c[2]) / 3.0;
        }
        let p = [x, y, 0.0];
        let wa = area2(p, b, c) / d;
        let wb = area2(a, p, c) / d;
        let wc = 1.0 - wa - wb;
        wa * a[2] + wb * b[2] + wc * c[2]
    }

    /// Whether (x, y) is inside triangle `t` seen from above (edges included, with a
    /// small tolerance).
    pub fn contains_xy(&self, t: u32, x: f32, y: f32) -> bool {
        let [a, b, c] = self.corners(t);
        let p = [x, y, 0.0];
        let s = if area2(a, b, c) >= 0.0 { 1.0 } else { -1.0 };
        let eps = -1e-3;
        s * area2(a, b, p) >= eps && s * area2(b, c, p) >= eps && s * area2(c, a, p) >= eps
    }

    /// Triangles of `space` whose bounds may cover (x, y).
    pub fn candidates(&self, space: Space, x: f32, y: f32) -> &[u32] {
        self.buckets.get(&space).and_then(|b| b.get(&(bucket(x), bucket(y)))).map_or(&[], |v| v.as_slice())
    }

    /// Triangles of `space` whose bounds may come within `radius` (horizontally) of
    /// (x, y), each once, in index order.
    /// A radius past [`MAX_QUERY_RADIUS`] is cut to it; a point or radius that is not
    /// finite finds nothing.
    pub fn triangles_near(&self, space: Space, x: f32, y: f32, radius: f32) -> Vec<u32> {
        let Some(b) = self.buckets.get(&space) else { return Vec::new() };
        if !(x.is_finite() && y.is_finite() && radius.is_finite()) {
            return Vec::new();
        }
        let radius = radius.clamp(0.0, MAX_QUERY_RADIUS);
        let mut out = Vec::new();
        for by in bucket(y - radius)..=bucket(y + radius) {
            for bx in bucket(x - radius)..=bucket(x + radius) {
                out.extend(b.get(&(bx, by)).map_or(&[][..], |v| v.as_slice()));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The cover edges ([`CoverEdge`]) of usable cover triangles of `space` whose middle
    /// is within `radius` of `at` horizontally, nearest first.
    pub fn cover_edges_near(&self, space: Space, at: [f32; 3], radius: f32) -> Vec<CoverEdge> {
        let mut out = Vec::new();
        for t in self.triangles_near(space, at[0], at[1], radius) {
            let tri = &self.tris[t as usize];
            if !tri.cover || !self.usable(t) {
                continue;
            }
            let c = self.corners(t);
            for e in 0..3 {
                if tri.kind[e] != EdgeKind::Boundary {
                    continue;
                }
                let (a, b, other) = (c[e], c[(e + 1) % 3], c[(e + 2) % 3]);
                let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
                if (mid[0] - at[0]).hypot(mid[1] - at[1]) > radius {
                    continue;
                }
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = dx.hypot(dy);
                if len < 1.0 {
                    continue;
                }
                let mut n = [dy / len, -dx / len];
                if (other[0] - mid[0]) * n[0] + (other[1] - mid[1]) * n[1] > 0.0 {
                    n = [-n[0], -n[1]];
                }
                out.push(CoverEdge { tri: t, a, b, mid, out: n });
            }
        }
        out.sort_by(|x, y| (x.mid[0] - at[0]).hypot(x.mid[1] - at[1]).total_cmp(&(y.mid[0] - at[0]).hypot(y.mid[1] - at[1])));
        out
    }

    /// The usable triangle of `space` under or over `p` whose surface is closest to `p`
    /// vertically, within `max_dz`.
    pub fn locate(&self, space: Space, p: [f32; 3], max_dz: f32) -> Option<u32> {
        let mut best: Option<(f32, u32)> = None;
        for &t in self.candidates(space, p[0], p[1]) {
            if !self.usable(t) || !self.contains_xy(t, p[0], p[1]) {
                continue;
            }
            let dz = (self.height_on(t, p[0], p[1]) - p[2]).abs();
            if dz <= max_dz && best.is_none_or(|(d, _)| dz < d) {
                best = Some((dz, t));
            }
        }
        best.map(|(_, t)| t)
    }

    /// The usable triangle of `space` nearest to `p` within `radius` (horizontal distance
    /// to the triangle, with vertical distance counted double so that a floor above or
    /// below loses to one at the right height), and the nearest point on it. A radius
    /// past [`MAX_QUERY_RADIUS`] is cut to it; a point that is not finite finds nothing.
    pub fn nearest(&self, space: Space, p: [f32; 3], radius: f32) -> Option<(u32, [f32; 3])> {
        if !p.iter().all(|c| c.is_finite()) || radius.is_nan() {
            return None;
        }
        if let Some(t) = self.locate(space, p, 64.0) {
            return Some((t, [p[0], p[1], self.height_on(t, p[0], p[1])]));
        }
        let radius = radius.clamp(0.0, MAX_QUERY_RADIUS);
        let mut best: Option<(f32, u32, [f32; 3])> = None;
        let r = (radius / BUCKET).ceil() as i32;
        let (bx, by) = (bucket(p[0]), bucket(p[1]));
        let b = self.buckets.get(&space)?;
        let mut seen = std::collections::HashSet::new();
        for y in by.saturating_sub(r)..=by.saturating_add(r) {
            for x in bx.saturating_sub(r)..=bx.saturating_add(r) {
                for &t in b.get(&(x, y)).map_or(&[][..], |v| v.as_slice()) {
                    if !seen.insert(t) || !self.usable(t) {
                        continue;
                    }
                    let q = closest_on_triangle(p, self.corners(t));
                    let h = (q[0] - p[0]).hypot(q[1] - p[1]);
                    let score = h + 2.0 * (q[2] - p[2]).abs();
                    if h <= radius && best.is_none_or(|(s, _, _)| score < s) {
                        best = Some((score, t, q));
                    }
                }
            }
        }
        best.map(|(_, t, q)| (t, q))
    }

    /// Where a walker can step from walkable area `from` onto area `to` of the same
    /// `space` though no link joins them: the nearest pair of points, one on the outline
    /// of `from` (an edge that leads nowhere), one on a usable triangle of `to`, at most
    /// `across` units apart on the ground and `rise` units in height. The point on `to`
    /// is moved up to [`STEP_IN`] units into its triangle, so that a walker who reaches
    /// it stands on `to`. SSHQ01's prize room navmesh 0x0010489C has no link to the
    /// cell's main one, their stair tops 17 to 31 units apart [observed, #3473]; the
    /// original's followers do not cross there either (observer check 279), a player
    /// walks the stairs. Every triangle of `space` is looked at: meant for an interior.
    pub fn step_between(&self, space: Space, from: u32, to: u32, across: f32, rise: f32) -> Option<([f32; 3], [f32; 3])> {
        if from == to || from == NONE || to == NONE {
            return None;
        }
        let mut best: Option<(f32, [f32; 3], [f32; 3])> = None;
        for t in 0..self.tris.len() as u32 {
            if self.area.get(t as usize) != Some(&from) || self.space_of_tri(t) != Some(space) {
                continue;
            }
            let tri = self.tris[t as usize];
            for e in 0..3 {
                if tri.kind[e] != EdgeKind::Boundary {
                    continue;
                }
                let (a, b) = (self.vertex(tri.v[e]), self.vertex(tri.v[(e + 1) % 3]));
                let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                let n = ((len / STEP_SAMPLE).ceil() as usize).max(1);
                for k in 0..=n {
                    let s = k as f32 / n as f32;
                    let p = [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, a[2] + (b[2] - a[2]) * s];
                    for u in self.triangles_near(space, p[0], p[1], across) {
                        if self.area.get(u as usize) != Some(&to) || !self.usable(u) {
                            continue;
                        }
                        let q = closest_on_triangle(p, self.corners(u));
                        let (h, dz) = ((q[0] - p[0]).hypot(q[1] - p[1]), (q[2] - p[2]).abs());
                        if h > across || dz > rise {
                            continue;
                        }
                        let score = h * h + dz * dz;
                        if best.is_none_or(|(s, ..)| score < s) {
                            let c = self.centre(u);
                            let to_c = [c[0] - q[0], c[1] - q[1], c[2] - q[2]];
                            let d = to_c[0].hypot(to_c[1]);
                            let f = if d > 1e-3 { STEP_IN.min(d) / d } else { 0.0 };
                            let q = [q[0] + to_c[0] * f, q[1] + to_c[1] * f, q[2] + to_c[2] * f];
                            best = Some((score, p, q));
                        }
                    }
                }
            }
        }
        best.map(|(_, p, q)| (p, q))
    }

    /// The segment through which a path leaves triangle `t` by edge `e`, as (left, right)
    /// seen from inside `t` looking out. Triangles wind counter-clockwise seen from above
    /// (docs/spec-notes/navmesh.md §3), so the edge's first vertex is on the right. Across
    /// a seam between navmeshes the two triangles' edges need not match: the portal is
    /// the part of this edge facing the other triangle's nearest edge.
    pub fn portal(&self, t: u32, e: usize) -> ([f32; 3], [f32; 3]) {
        let tri = &self.tris[t as usize];
        let right = self.vertex(tri.v[e]);
        let left = self.vertex(tri.v[(e + 1) % 3]);
        if tri.kind[e] != EdgeKind::Portal {
            return (left, right);
        }
        let o = self.corners(tri.next[e]);
        // The other triangle's edge closest to this one (sum of distances of its ends to
        // this edge's line, seen from above).
        let d = sub(left, right);
        let len2 = d[0] * d[0] + d[1] * d[1];
        if len2 < 1e-6 {
            return (left, right);
        }
        let param = |q: [f32; 3]| ((q[0] - right[0]) * d[0] + (q[1] - right[1]) * d[1]) / len2;
        let off = |q: [f32; 3]| ((q[0] - right[0]) * d[1] - (q[1] - right[1]) * d[0]).abs() / len2.sqrt();
        let best = (0..3).min_by(|&a, &b| {
            let s = |k: usize| off(o[k]) + off(o[(k + 1) % 3]);
            s(a).total_cmp(&s(b))
        });
        let Some(k) = best else { return (left, right) };
        let (t0, t1) = (param(o[k]), param(o[(k + 1) % 3]));
        let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
        if (hi - lo) * len2.sqrt() < 1.0 {
            return (left, right);
        }
        let at = |s: f32| [right[0] + d[0] * s, right[1] + d[1] * s, right[2] + d[2] * s];
        (at(hi), at(lo))
    }
}

/// The cosine of a triangle's slope (the up component of its unit normal; 1 for a
/// degenerate triangle).
pub fn slope_cos([a, b, c]: [[f32; 3]; 3]) -> f32 {
    let (u, v) = (sub(b, a), sub(c, a));
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l < 1e-9 {
        1.0
    } else {
        (n[2] / l).abs()
    }
}

/// Whether a triangle's corners are usable data: finite, within [`MAX_COORD`] of the
/// origin and within [`MAX_TRI_SPAN`] of each other on every axis.
fn sane_corners(c: [[f32; 3]; 3]) -> bool {
    (0..3).all(|k| {
        let (lo, hi) = (c.iter().map(|q| q[k]).fold(f32::INFINITY, f32::min), c.iter().map(|q| q[k]).fold(f32::NEG_INFINITY, f32::max));
        c.iter().all(|q| q[k].is_finite() && q[k].abs() <= MAX_COORD) && hi - lo <= MAX_TRI_SPAN
    })
}

fn bucket(v: f32) -> i32 {
    (v / BUCKET).floor() as i32
}

/// The point of triangle `t` nearest to `p` (in 3D).
pub fn closest_on_triangle(p: [f32; 3], t: [[f32; 3]; 3]) -> [f32; 3] {
    // Ericson, Real-Time Collision Detection §5.1.5 (the standard region test).
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let [a, b, c] = t;
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let lerp = |o: [f32; 3], d: [f32; 3], s: f32| [o[0] + d[0] * s, o[1] + d[1] * s, o[2] + d[2] * s];
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return lerp(a, ab, d1 / (d1 - d3));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return lerp(a, ac, d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return lerp(b, sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]
}
