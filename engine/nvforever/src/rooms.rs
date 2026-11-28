//! Room and portal culling in interiors (docs/spec-notes/level-design.md §3.1;
//! docs/design/frame-rate-audit.md §3): with the camera in a room, only the rooms seen
//! through its portals, and through theirs, are drawn.
//!
//! The rule [documented: level-design.md §3.1, the editor's Optimization and Occlusion
//! pages]: room markers are boxes; portals join two rooms; the space outside every room
//! (the unbounded space) acts as one more room; a portal in view makes the room behind it
//! visible, seen through a narrower frustum (the portal's projection), and the step repeats
//! from there. What is done here [engineering choices, each a simplification of that rule]:
//! - **Data**: room markers are references of base `0x1F` with a box `XPRM` (half
//!   extents); portals are references of base `0x20` with an `XPRM` one unit thick and
//!   `XPOD` naming the two rooms, a null entry the unbounded space [observed:
//!   level-design.md §3.1 "Data"].
//! - **Membership**: room membership is not stored per object (level-design.md §3.1), so
//!   a placed object's mesh belongs to every room whose box, grown by [`MARGIN`], its
//!   bounds touch, and to the unbounded space when no one of those boxes holds it whole
//!   (it touches none, or reaches out of them: #4485). A mesh is drawn when
//!   one of its rooms is. Walls and floors on a room's edge belong to both sides. Only the
//!   meshes of placed objects and items are culled: actors, the player's body, dropped
//!   items, lights, particles and effects are always drawn (actors walk between rooms;
//!   the editor's pages say objects moved by Havok are drawn while they move).
//! - **Linked rooms** (`XLRM` on a room marker, with its count in `XRMR`) [documented:
//!   xEdit's record definitions name them "Linked Rooms Count" and "Linked Room";
//!   observed: Fallout 3's masters as Tale of Two Wastelands loads them have them (320 in
//!   Fallout3.esm), the New Vegas masters none; hypothesis for the rule]: two linked
//!   rooms are seen together with no portal between them, as if they were one room, so a
//!   room in view makes its linked rooms visible through the same rectangle. Links are
//!   taken both ways (the data names them both ways). Vault 101 needs them: the room
//!   just outside the G.O.A.T. classroom (0x060ABBF5) has no portal but the classroom
//!   door and is linked to the corridor (0x060ABBF6); without links the corridor was
//!   culled from there (owner report 2026-11-08).
//! - **The narrower frustum** is a screen rectangle: a portal's corners projected, their
//!   bounding rectangle cut by the rectangle the room was seen through. A portal with a
//!   corner behind the camera passes the whole rectangle on (the camera is in or near it).
//! - **No room**: with the camera in no room (outside the boxes, in a cell without rooms,
//!   or with no player camera), everything is drawn: never the void.
//! - **Occlusion planes** (§3.2, `occlusion.rs`), in interiors (outdoors only with
//!   `--occlusion-planes-outdoors`, see `occlusion.rs`): a mesh
//!   wholly in the volume a plane hides from the eye is hidden too, and a portal wholly
//!   in it is not followed. Multibounds are not used: each mesh is tested by its own
//!   bounds (which references are multibounds in New Vegas is open, §13).
//!
//! A culled mesh is moved to render layer [`CULL_LAYER`], which no camera or light sees;
//! the meshes this module hides are marked [`RoomHidden`] and given back the default
//! layer when their room is in view again (placed objects' meshes have no layers of
//! their own). `--no-room-culling` turns rooms and portals off, `--no-occlusion-planes`
//! the planes; the test aids
//! `NVF_PLAY_TEST_ROOM_CULLING_CYCLE=<seconds>` turns it off and on again every that many
//! seconds of real time (`rooms: culling off|on (test aid)`), and
//! `NVF_PLAY_TEST_OCCLUSION_CYCLE=<seconds>` the planes likewise (`occlusion: planes
//! off|on (test aid)`), to compare frame times in one run.
//!
//! Logged: `rooms: <cell>: <n> rooms, <p> portals` when an interior with rooms loads, and
//! `rooms: <cell>: camera in room <id>, <v> of <n> rooms in view, <h> meshes hidden`
//! when that changes (at most a line a second), and at the exit
//! `rooms summary: …` with the frames culled and the most meshes hidden. For the planes:
//! `occlusion: <cell>: <n> planes` when an interior with planes loads, `occlusion: <n>
//! planes in the loaded cells` outdoors when that changes, `occlusion: <n> planes in
//! front, <h> of <m> meshes hidden by planes, <q> portals not followed` when that changes (at
//! most a line a second), and at the exit `occlusion summary: …`.

use crate::draw_distance::DrawClass;
use crate::occlusion::{Plane, Shadow};
use crate::world::{bevy_to_game, Streamer, WorldData};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use fnv_world::fields::{formid_of, global, num, winner, Fields};
use fnv_world::placed::placement;
use fnv_world::transform::ref_rotation;
use nvf_play::RefVisual;
use std::collections::{HashMap, HashSet, VecDeque};

/// The render layer culled meshes are moved to: seen by no camera or light.
pub const CULL_LAYER: usize = 29;
/// Room boxes are grown by this much for membership, game units [engineering choice: a
/// wall's thickness, so the walls and floors on a room's edge count on both sides].
pub const MARGIN: f32 = 32.0;
/// Base forms of room markers and portals [observed: level-design.md §3.1].
pub const ROOM_MARKER: u32 = 0x1F;
pub const PORTAL: u32 = 0x20;
/// Rooms followed at most per frame (a guard against a loop of portals).
const MAX_STEPS: usize = 4096;

/// A room box: centre, rotation (game space, `v' = M v`) and half extents.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoomBox {
    pub id: u32,
    pub center: Vec3,
    pub rot: Mat3,
    pub half: Vec3,
}

impl RoomBox {
    /// Whether `p` (game space) is inside, the box grown by `grow`.
    pub fn contains(&self, p: Vec3, grow: f32) -> bool {
        let l = self.rot.transpose() * (p - self.center);
        l.x.abs() <= self.half.x + grow && l.y.abs() <= self.half.y + grow && l.z.abs() <= self.half.z + grow
    }

    /// Whether the box grown by `grow` touches the world box `min`–`max` (game space):
    /// the oriented box against the axis-aligned one, by their separating axes along
    /// the world's and the box's own axes [a conservative test: boxes it calls touching
    /// may not, never the other way].
    pub fn touches(&self, min: Vec3, max: Vec3, grow: f32) -> bool {
        let half = self.half + Vec3::splat(grow);
        let c2 = (min + max) / 2.0;
        let h2 = (max - min) / 2.0;
        let d = c2 - self.center;
        let axes = [self.rot.x_axis, self.rot.y_axis, self.rot.z_axis];
        // The world's axes.
        for i in 0..3 {
            let r1: f32 = (0..3).map(|k| half[k] * axes[k][i].abs()).sum();
            if d[i].abs() > r1 + h2[i] {
                return false;
            }
        }
        // The box's axes.
        for (k, a) in axes.iter().enumerate() {
            let r2 = h2.x * a.x.abs() + h2.y * a.y.abs() + h2.z * a.z.abs();
            if d.dot(*a).abs() > half[k] + r2 {
                return false;
            }
        }
        true
    }
}

/// A portal: its corners (game space) and the two rooms it joins (indices into the
/// rooms; `None` the unbounded space).
#[derive(Debug, Clone, PartialEq)]
pub struct Portal {
    pub id: u32,
    pub corners: Vec<Vec3>,
    pub rooms: [Option<usize>; 2],
}

/// A space a portal leads to: a room, or the unbounded space.
type Space = Option<usize>;

/// The rooms and portals of the interior drawn now.
#[derive(Resource, Default)]
pub struct Rooms {
    pub off: bool,
    /// `NVF_PLAY_TEST_ROOM_CULLING_CYCLE`: seconds on, then as many off.
    cycle: Option<f32>,
    cycled_off: bool,
    pub cell: Option<u32>,
    pub rooms: Vec<RoomBox>,
    pub portals: Vec<Portal>,
    /// Linked rooms (indices into `rooms`), each pair once (module notes).
    pub links: Vec<[usize; 2]>,
    /// The spaces in view this frame (`None` in it: the unbounded space); `None`: all.
    pub visible: Option<HashSet<Space>>,
    pub camera_room: Option<usize>,
    last_log: f32,
    logged: Option<(Option<usize>, usize, usize)>,
    pub frames_culled: u64,
    pub most_hidden: usize,
    /// `--no-occlusion-planes`.
    pub planes_off: bool,
    /// `--occlusion-planes-outdoors`.
    pub planes_outdoors: bool,
    /// `NVF_PLAY_TEST_OCCLUSION_CYCLE`: seconds on, then as many off.
    planes_cycle: Option<f32>,
    planes_cycled_off: bool,
    /// The occlusion planes of the interior, or of the loaded exterior cells.
    pub planes: Vec<Plane>,
    /// Outdoors: the planes of each exterior cell read so far, and the loaded cells the
    /// planes were gathered for.
    plane_cells: HashMap<(i32, i32), Vec<Plane>>,
    plane_grid: Vec<(i32, i32)>,
    /// Meshes this module hides now (by rooms or planes).
    hidden_now: usize,
    planes_last_log: f32,
    planes_logged: Option<(usize, usize, usize)>,
    pub frames_planes: u64,
    pub most_plane_hidden: usize,
    pub most_portals_blocked: usize,
}

/// The spaces a mesh belongs to ([`MARGIN`]): the rooms it touches, and whether it is also
/// in the unbounded space (it touches none, or it reaches out of every room it touches).
#[derive(Component, Debug, Clone, Default)]
pub struct InRooms(pub Vec<usize>, pub bool);

impl InRooms {
    /// The spaces of a mesh with game-space bounds `min`–`max` among `rooms`. A mesh is in
    /// the unbounded space too when no one room's grown box holds it whole [design, #4485:
    /// past a doorway into the unbounded space (HELIOS One's tower, 0017B450; REPCONN's
    /// basement), corridor pieces that also reach a corner of a neighbouring room's box
    /// were that room's only, and were hidden while seen through the doorway, the fog
    /// showing where the corridor should be].
    pub fn of(rooms: &[RoomBox], min: Vec3, max: Vec3) -> InRooms {
        let touched: Vec<usize> = rooms.iter().enumerate().filter(|(_, r)| r.touches(min, max, MARGIN)).map(|(i, _)| i).collect();
        let corners = (0..8).map(|i| Vec3::new(if i & 1 == 0 { min.x } else { max.x }, if i & 2 == 0 { min.y } else { max.y }, if i & 4 == 0 { min.z } else { max.z }));
        let held = touched.iter().any(|&i| corners.clone().all(|c| rooms[i].contains(c, MARGIN)));
        InRooms(touched, !held)
    }

    /// Whether the mesh is in one of the spaces `visible` holds (`None`: the unbounded).
    pub fn seen_in(&self, visible: &HashSet<Option<usize>>) -> bool {
        (self.1 && visible.contains(&None)) || self.0.iter().any(|i| visible.contains(&Some(*i)))
    }
}

/// A mesh's bounds in game space (axis-aligned), kept up as it moves.
#[derive(Component, Debug, Clone, Copy)]
pub struct GameBounds(pub Vec3, pub Vec3);

/// A placed reference's mesh that is never culled (an actor's, a dropped item's).
#[derive(Component)]
pub struct Unroomed;

/// A mesh this module moved to [`CULL_LAYER`].
#[derive(Component)]
pub struct RoomHidden;

pub struct RoomsPlugin {
    pub off: bool,
    /// `--no-occlusion-planes`.
    pub planes_off: bool,
    /// `--occlusion-planes-outdoors`: the planes of exterior cells are used too.
    pub planes_outdoors: bool,
}

impl Plugin for RoomsPlugin {
    fn build(&self, app: &mut App) {
        if self.off {
            info!("rooms: --no-room-culling: every room of an interior is drawn");
        }
        let cycle = std::env::var("NVF_PLAY_TEST_ROOM_CULLING_CYCLE").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|s| *s > 0.0);
        if let Some(c) = cycle {
            info!("test aid NVF_PLAY_TEST_ROOM_CULLING_CYCLE: room culling off and on every {c} s");
        }
        if self.planes_off {
            info!("occlusion: --no-occlusion-planes: occlusion planes hide nothing");
        }
        let planes_cycle = std::env::var("NVF_PLAY_TEST_OCCLUSION_CYCLE").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|s| *s > 0.0);
        if let Some(c) = planes_cycle {
            info!("test aid NVF_PLAY_TEST_OCCLUSION_CYCLE: occlusion planes off and on every {c} s");
        }
        if self.planes_outdoors && !self.planes_off {
            info!("occlusion: --occlusion-planes-outdoors: the planes of exterior cells hide meshes too");
        }
        app.insert_resource(Rooms { off: self.off, cycle, planes_off: self.planes_off, planes_outdoors: self.planes_outdoors, planes_cycle, ..default() }).add_systems(Update, (load, assign, cull).chain().after(crate::draw_distance::apply_ranges)).add_systems(Last, report);
    }
}

/// A portal's corners from its centre, rotation and half extents: the box's corners on
/// its two widest axes, at the middle of the thin one.
pub fn portal_corners(center: Vec3, rot: Mat3, half: Vec3) -> Vec<Vec3> {
    let mut axes = [(half.x, rot.x_axis), (half.y, rot.y_axis), (half.z, rot.z_axis)];
    axes.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (a, b) = (axes[0].1 * axes[0].0, axes[1].1 * axes[1].0);
    vec![center + a + b, center + a - b, center - a - b, center - a + b]
}

/// Read the rooms and portals of interior `cell` (module notes).
fn read(world: &WorldData, cell: u32) -> (Vec<RoomBox>, Vec<Portal>, Vec<[usize; 2]>) {
    let Some(c) = world.index.cells.get(&cell) else { return (Vec::new(), Vec::new(), Vec::new()) };
    let mut rooms = Vec::new();
    let mut raw_portals = Vec::new();
    let mut raw_links: Vec<(u32, u32)> = Vec::new();
    for &r in &c.refs {
        let Some(p) = placement(world.lo, world.schema, r) else { continue };
        let base = p.base.map(|b| b & 0x00FF_FFFF);
        if base != Some(ROOM_MARKER) && base != Some(PORTAL) {
            continue;
        }
        let Some((pi, rec)) = winner(world.lo, r) else { continue };
        let Ok(f) = Fields::new(rec, pi, world.schema) else { continue };
        let Some(bounds) = f.get(b"XPRM").and_then(|v| v.get("bounds").cloned()) else { continue };
        let Some(half) = (|| Some(Vec3::new(num(&bounds, "x")? as f32, num(&bounds, "y")? as f32, num(&bounds, "z")? as f32).abs()))() else { continue };
        let center = Vec3::from_array(p.position);
        let rot = Mat3::from_cols_array(&ref_rotation(p.rotation)).transpose();
        if base == Some(ROOM_MARKER) {
            rooms.push(RoomBox { id: r, center, rot, half });
            for link in f.subs.iter().filter(|s| &s.sig == b"XLRM").filter_map(|s| f.decode(s)) {
                if let Some(to) = formid_of(&link).and_then(|l| global(world.lo, pi, l)) {
                    raw_links.push((r, to));
                }
            }
        } else {
            let pair: Vec<Option<u32>> = f
                .get(b"XPOD")
                .and_then(|v| v.as_array().cloned())
                .map(|a| a.iter().map(|x| formid_of(x).and_then(|l| global(world.lo, pi, l))).collect())
                .unwrap_or_default();
            raw_portals.push((r, portal_corners(center, rot, half), pair));
        }
    }
    let index: HashMap<u32, usize> = rooms.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let portals = raw_portals
        .into_iter()
        .filter_map(|(id, corners, pair)| {
            if pair.len() != 2 {
                return None;
            }
            // A null entry is the unbounded space; a room not among the cell's markers
            // cannot be followed (the portal is dropped).
            let to = |x: &Option<u32>| -> Option<Space> {
                match x {
                    None => Some(None),
                    Some(r) => index.get(r).map(|i| Some(*i)),
                }
            };
            Some(Portal { id, corners, rooms: [to(&pair[0])?, to(&pair[1])?] })
        })
        .collect();
    let links = linked_pairs(&rooms, &raw_links);
    (rooms, portals, links)
}

/// Linked rooms as pairs of room indices, each pair once whichever way the data names it;
/// a link to a room not among the cell's markers is dropped.
fn linked_pairs(rooms: &[RoomBox], links: &[(u32, u32)]) -> Vec<[usize; 2]> {
    let index: HashMap<u32, usize> = rooms.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let mut out: Vec<[usize; 2]> = links
        .iter()
        .filter_map(|(a, b)| {
            let (a, b) = (*index.get(a)?, *index.get(b)?);
            (a != b).then_some([a.min(b), a.max(b)])
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn load(mut rooms: ResMut<Rooms>, streamer: Option<Res<Streamer>>, world: Option<Res<WorldData>>) {
    let interior = streamer.as_ref().and_then(|s| s.interior);
    if rooms.cell != interior {
        rooms.cell = interior;
        rooms.rooms.clear();
        rooms.portals.clear();
        rooms.links.clear();
        rooms.planes.clear();
        rooms.plane_cells.clear();
        rooms.plane_grid.clear();
        rooms.visible = None;
        rooms.logged = None;
        rooms.planes_logged = None;
        if let (Some(cell), Some(world)) = (interior, world.as_ref()) {
            let (r, p, l) = read(world, cell);
            if !r.is_empty() {
                info!("rooms: {cell:08X}: {} rooms, {} portals, {} linked pairs", r.len(), p.len(), l.len());
            }
            rooms.rooms = r;
            rooms.portals = p;
            rooms.links = l;
            let planes = world.index.cells.get(&cell).map(|c| crate::occlusion::read(world, c.refs.iter().copied())).unwrap_or_default();
            if !planes.is_empty() {
                info!("occlusion: {cell:08X}: {} planes", planes.len());
            }
            rooms.planes = planes;
        }
        return;
    }
    // Outdoors, with `--occlusion-planes-outdoors`: the planes of the loaded cells (and
    // of the persistent references standing in them), read once per cell.
    let (Some(s), Some(world), None, true) = (streamer, world, interior, rooms.planes_outdoors) else { return };
    let mut grid: Vec<(i32, i32)> = s.loaded.keys().copied().collect();
    grid.sort_unstable();
    if grid == rooms.plane_grid {
        return;
    }
    let rooms = &mut *rooms;
    for g in &grid {
        rooms.plane_cells.entry(*g).or_insert_with(|| {
            let refs = world.grid_cell(*g).map(|c| c.refs.as_slice()).unwrap_or_default().iter().chain(world.persistent_by_grid.get(g).into_iter().flatten());
            crate::occlusion::read(&world, refs.copied())
        });
    }
    let planes: Vec<Plane> = grid.iter().filter_map(|g| rooms.plane_cells.get(g)).flatten().cloned().collect();
    if planes.len() != rooms.planes.len() {
        let ids: Vec<String> = planes.iter().map(|p| format!("{:08X}", p.id)).collect();
        info!("occlusion: {} planes in the loaded cells{}{}", planes.len(), if ids.is_empty() { "" } else { ": " }, ids.join(", "));
    }
    rooms.planes = planes;
    rooms.plane_grid = grid;
}

/// Give new meshes of placed objects and items their rooms.
#[allow(clippy::type_complexity)]
fn assign(
    mut commands: Commands,
    rooms: Res<Rooms>,
    new: Query<(Entity, &DrawClass, &GlobalTransform, &Aabb), (With<RefVisual>, Without<InRooms>, Without<Unroomed>)>,
) {
    if rooms.rooms.is_empty() && rooms.planes.is_empty() {
        return;
    }
    for (e, class, gt, aabb) in &new {
        if !matches!(class, DrawClass::Object | DrawClass::Item) {
            commands.entity(e).insert(Unroomed);
            continue;
        }
        let (min, max) = game_bounds(gt, aabb);
        commands.entity(e).insert((InRooms::of(&rooms.rooms, min, max), GameBounds(min, max)));
    }
}

/// A mesh's bounds in game space (axis-aligned).
fn game_bounds(gt: &GlobalTransform, aabb: &Aabb) -> (Vec3, Vec3) {
    let c = Vec3::from(aabb.center);
    let h = Vec3::from(aabb.half_extents);
    let m = gt.affine();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for i in 0..8 {
        let s = Vec3::new(if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { -1.0 } else { 1.0 });
        let p = bevy_to_game(m.transform_point3(c + h * s));
        min = min.min(p);
        max = max.max(p);
    }
    (min, max)
}

/// A screen rectangle in normalised device coordinates: (min x, min y, max x, max y).
type Rect2 = [f32; 4];
const FULL: Rect2 = [-1.0, -1.0, 1.0, 1.0];

fn cut(a: Rect2, b: Rect2) -> Option<Rect2> {
    let r = [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    (r[0] < r[2] && r[1] < r[3]).then_some(r)
}

fn covers(a: Rect2, b: Rect2) -> bool {
    a[0] <= b[0] && a[1] <= b[1] && a[2] >= b[2] && a[3] >= b[3]
}

fn union(a: Rect2, b: Rect2) -> Rect2 {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

/// The screen rectangle of a portal's corners (game space) through `clip_from_game`; the
/// whole screen when a corner is behind the camera; `None` when all are.
fn portal_rect(corners: &[Vec3], clip_from_game: &Mat4) -> Option<Rect2> {
    let mut r = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
    let mut behind = 0;
    for c in corners {
        let p = *clip_from_game * c.extend(1.0);
        if p.w <= 1e-3 {
            behind += 1;
            continue;
        }
        let (x, y) = (p.x / p.w, p.y / p.w);
        r = [r[0].min(x), r[1].min(y), r[2].max(x), r[3].max(y)];
    }
    match behind {
        0 => Some(r),
        n if n == corners.len() => None,
        _ => Some(FULL),
    }
}

/// The spaces seen from `start` through the portals and linked rooms (module notes); a
/// portal wholly in one of the planes' `shadows` is not followed (counted in `blocked`).
pub fn visible_spaces(portals: &[Portal], links: &[[usize; 2]], start: Space, clip_from_game: &Mat4, shadows: &[Shadow], blocked: &mut usize) -> HashSet<Space> {
    let hidden: Vec<bool> = portals.iter().map(|p| shadows.iter().any(|s| s.hides_points(&p.corners))).collect();
    *blocked = hidden.iter().filter(|h| **h).count();
    let mut seen: HashMap<Space, Rect2> = HashMap::from([(start, FULL)]);
    let mut queue: VecDeque<(Space, Rect2)> = VecDeque::from([(start, FULL)]);
    let mut steps = 0;
    while let Some((space, rect)) = queue.pop_front() {
        steps += 1;
        if steps > MAX_STEPS {
            break;
        }
        // A linked room is seen through the same rectangle as this one.
        if let Some(here) = space {
            for l in links.iter().filter(|l| l.contains(&here)) {
                let other = Some(if l[0] == here { l[1] } else { l[0] });
                match seen.get_mut(&other) {
                    Some(had) if covers(*had, rect) => {}
                    Some(had) => {
                        *had = union(*had, rect);
                        queue.push_back((other, rect));
                    }
                    None => {
                        seen.insert(other, rect);
                        queue.push_back((other, rect));
                    }
                }
            }
        }
        for (p, _) in portals.iter().zip(&hidden).filter(|(_, h)| !**h) {
            let other = if p.rooms[0] == space {
                p.rooms[1]
            } else if p.rooms[1] == space {
                p.rooms[0]
            } else {
                continue;
            };
            let Some(pr) = portal_rect(&p.corners, clip_from_game) else { continue };
            let Some(through) = cut(pr, rect) else { continue };
            match seen.get_mut(&other) {
                Some(had) if covers(*had, through) => {}
                Some(had) => {
                    *had = union(*had, through);
                    queue.push_back((other, through));
                }
                None => {
                    seen.insert(other, through);
                    queue.push_back((other, through));
                }
            }
        }
    }
    seen.into_keys().collect()
}

#[allow(clippy::type_complexity)]
fn cull(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut rooms: ResMut<Rooms>,
    camera: Query<(&Camera, &GlobalTransform), With<nvf_play::PlayerCamera>>,
    mut meshes: Query<(Entity, &InRooms, &mut GameBounds, Ref<GlobalTransform>, &Aabb, Has<RoomHidden>, Option<&RenderLayers>)>,
) {
    let rooms = &mut *rooms;
    if rooms.rooms.is_empty() && rooms.planes.is_empty() && rooms.hidden_now == 0 {
        return;
    }
    let now = time.elapsed_secs();
    if let Some(c) = rooms.cycle {
        let off = (now / c) as u64 % 2 == 1;
        if off != rooms.cycled_off {
            rooms.cycled_off = off;
            info!("rooms: culling {} (test aid)", if off { "off" } else { "on" });
        }
    }
    if let Some(c) = rooms.planes_cycle {
        let off = (now / c) as u64 % 2 == 1;
        if off != rooms.planes_cycled_off {
            rooms.planes_cycled_off = off;
            info!("occlusion: planes {} (test aid)", if off { "off" } else { "on" });
        }
    }
    let cam = camera.iter().find(|(c, _)| c.is_active);
    let eye = cam.map(|(_, gt)| bevy_to_game(gt.translation()));
    // The volumes the planes hide from the eye.
    let shadows: Vec<Shadow> = match eye {
        Some(eye) if !rooms.planes_off && !rooms.planes_cycled_off => rooms.planes.iter().filter_map(|p| p.shadow_from(eye)).collect(),
        _ => Vec::new(),
    };
    let start = eye.and_then(|p| rooms.rooms.iter().position(|r| r.contains(p, 0.0)));
    let mut blocked = 0;
    let visible = match (rooms.off || rooms.cycled_off, cam, start) {
        (false, Some((c, gt)), Some(start)) => {
            // Game space → Bevy world space is the world root's rotation (world.rs
            // `game_to_bevy`; an interior's cell entity sits at the origin).
            let bevy_from_game = crate::world::game_to_bevy().to_matrix();
            let clip_from_game = c.clip_from_view() * gt.to_matrix().inverse() * bevy_from_game;
            Some(visible_spaces(&rooms.portals, &rooms.links, Some(start), &clip_from_game, &shadows, &mut blocked))
        }
        _ => None,
    };
    let mut hidden = 0usize;
    let mut by_planes = 0usize;
    let mut total = 0usize;
    for (e, in_rooms, mut bounds, gt, aabb, was_hidden, layers) in &mut meshes {
        total += 1;
        if gt.is_changed() {
            let (min, max) = game_bounds(&gt, aabb);
            *bounds = GameBounds(min, max);
        }
        let in_view = visible.as_ref().is_none_or(|v| in_rooms.seen_in(v));
        let behind_plane = in_view && shadows.iter().any(|s| s.hides_box(bounds.0, bounds.1));
        let show = in_view && !behind_plane;
        if !show {
            hidden += 1;
        }
        if behind_plane {
            by_planes += 1;
        }
        match (show, was_hidden) {
            (false, false) if layers.is_none() => {
                commands.entity(e).insert((RenderLayers::layer(CULL_LAYER), RoomHidden));
            }
            (true, true) => {
                commands.entity(e).remove::<(RenderLayers, RoomHidden)>();
            }
            _ => {}
        }
    }
    rooms.hidden_now = hidden;
    rooms.camera_room = start;
    if visible.is_some() {
        rooms.frames_culled += 1;
        rooms.most_hidden = rooms.most_hidden.max(hidden);
    }
    if !shadows.is_empty() {
        rooms.frames_planes += 1;
        rooms.most_plane_hidden = rooms.most_plane_hidden.max(by_planes);
        rooms.most_portals_blocked = rooms.most_portals_blocked.max(blocked);
    }
    let plane_state = (shadows.len(), by_planes, blocked);
    if !rooms.planes.is_empty() && rooms.planes_logged != Some(plane_state) && now - rooms.planes_last_log >= 1.0 {
        rooms.planes_logged = Some(plane_state);
        rooms.planes_last_log = now;
        info!("occlusion: {} planes in front, {by_planes} of {} meshes hidden by planes, {blocked} portals not followed", shadows.len(), total);
    }
    if rooms.rooms.is_empty() {
        rooms.visible = visible;
        return;
    }
    let state = (start, visible.as_ref().map_or(rooms.rooms.len() + 1, |v| v.len()), hidden);
    if rooms.logged != Some(state) && now - rooms.last_log >= 1.0 {
        rooms.logged = Some(state);
        rooms.last_log = now;
        let cell = rooms.cell.unwrap_or(0);
        match (visible.as_ref(), start) {
            (Some(v), Some(s)) => info!(
                "rooms: {cell:08X}: camera in room {:08X}, {} of {} rooms in view{}, {hidden} meshes hidden",
                rooms.rooms[s].id,
                v.iter().filter(|x| x.is_some()).count(),
                rooms.rooms.len(),
                if v.contains(&None) { " and the unbounded space" } else { "" }
            ),
            _ => info!("rooms: {cell:08X}: camera in no room, every room drawn"),
        }
    }
    rooms.visible = visible;
}

fn report(mut exit: MessageReader<AppExit>, rooms: Res<Rooms>) {
    if exit.read().last().is_none() {
        return;
    }
    if rooms.frames_culled > 0 {
        info!("rooms summary: {} frames culled by rooms and portals, at most {} meshes hidden", rooms.frames_culled, rooms.most_hidden);
    }
    if rooms.frames_planes > 0 {
        info!(
            "occlusion summary: {} frames with occlusion planes in front, at most {} meshes hidden by planes, at most {} portals not followed",
            rooms.frames_planes, rooms.most_plane_hidden, rooms.most_portals_blocked
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(id: u32, center: [f32; 3], half: [f32; 3]) -> RoomBox {
        RoomBox { id, center: Vec3::from_array(center), rot: Mat3::IDENTITY, half: Vec3::from_array(half) }
    }

    #[test]
    fn rooms_contain_and_touch() {
        let r = room(1, [0.0, 0.0, 0.0], [100.0, 50.0, 20.0]);
        assert!(r.contains(Vec3::new(99.0, -49.0, 0.0), 0.0));
        assert!(!r.contains(Vec3::new(101.0, 0.0, 0.0), 0.0));
        assert!(r.contains(Vec3::new(101.0, 0.0, 0.0), 2.0));
        assert!(r.touches(Vec3::new(90.0, 0.0, 0.0), Vec3::new(130.0, 10.0, 10.0), 0.0));
        assert!(!r.touches(Vec3::new(140.0, 0.0, 0.0), Vec3::new(150.0, 10.0, 10.0), 32.0));
        assert!(r.touches(Vec3::new(120.0, 0.0, 0.0), Vec3::new(150.0, 10.0, 10.0), 32.0), "within the margin");
        // A box turned a quarter: its long side along y.
        let t = RoomBox { rot: Mat3::from_rotation_z(std::f32::consts::FRAC_PI_2), ..r };
        assert!(t.contains(Vec3::new(0.0, 99.0, 0.0), 0.0) && !t.contains(Vec3::new(99.0, 0.0, 0.0), 0.0));
        assert!(!t.touches(Vec3::new(60.0, 0.0, 0.0), Vec3::new(70.0, 1.0, 1.0), 0.0));
    }

    #[test]
    fn portals_span_their_two_widest_axes() {
        let c = portal_corners(Vec3::ZERO, Mat3::IDENTITY, Vec3::new(64.0, 0.5, 96.0));
        assert_eq!(c.len(), 4);
        assert!(c.iter().all(|p| p.y == 0.0 && p.x.abs() == 64.0 && p.z.abs() == 96.0));
    }

    /// A camera at the origin looking along +x (game space), 90° wide.
    fn camera() -> Mat4 {
        let proj = bevy::math::proj::perspective(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 10_000.0);
        // View: game +x forward, +z up.
        let view = Mat4::from_quat(Transform::IDENTITY.looking_at(Vec3::X, Vec3::Z).rotation).inverse();
        proj * view
    }

    #[test]
    fn rooms_are_seen_through_portals_in_view() {
        // Room 0 around the camera; room 1 beyond a portal ahead; room 2 beyond a portal
        // behind the camera's back, far off; room 3 behind room 1's far portal.
        let p = |id, x: f32, a, b| Portal { id, corners: portal_corners(Vec3::new(x, 0.0, 0.0), Mat3::IDENTITY, Vec3::new(0.5, 50.0, 50.0)), rooms: [a, b] };
        let portals = vec![p(10, 200.0, Some(0), Some(1)), p(11, -300.0, Some(0), Some(2)), p(12, 600.0, Some(1), Some(3)), p(13, 900.0, Some(3), None)];
        let v = visible_spaces(&portals, &[], Some(0), &camera(), &[], &mut 0);
        assert!(v.contains(&Some(0)) && v.contains(&Some(1)) && v.contains(&Some(3)) && v.contains(&None));
        assert!(!v.contains(&Some(2)), "the room behind the camera is not seen");
        // A portal off to the side, outside the first portal's rectangle, is not followed.
        let side = vec![p(10, 200.0, Some(0), Some(1)), Portal { id: 14, corners: portal_corners(Vec3::new(600.0, 2000.0, 0.0), Mat3::IDENTITY, Vec3::new(0.5, 50.0, 50.0)), rooms: [Some(1), Some(4)] }];
        let v = visible_spaces(&side, &[], Some(0), &camera(), &[], &mut 0);
        assert!(v.contains(&Some(1)) && !v.contains(&Some(4)));
    }

    /// Vault 101 outside the G.O.A.T. classroom: the camera's room (0) has a portal only
    /// behind it, but is linked to the corridor (1), which has a portal ahead to room 2
    /// and a link to room 3. The linked rooms are seen without a portal, and what lies
    /// through the linked room's portals too.
    #[test]
    fn linked_rooms_are_seen_together() {
        let p = |id, x: f32, a, b| Portal { id, corners: portal_corners(Vec3::new(x, 0.0, 0.0), Mat3::IDENTITY, Vec3::new(0.5, 50.0, 50.0)), rooms: [a, b] };
        let portals = vec![p(10, -300.0, Some(0), Some(4)), p(11, 600.0, Some(1), Some(2))];
        let v = visible_spaces(&portals, &[[0, 1], [1, 3]], Some(0), &camera(), &[], &mut 0);
        assert!(v.contains(&Some(1)) && v.contains(&Some(3)), "linked rooms: {v:?}");
        assert!(v.contains(&Some(2)), "through the linked room's portal: {v:?}");
        assert!(!v.contains(&Some(4)), "the portal behind the camera");
        // Without links only the camera's room.
        let v = visible_spaces(&portals, &[], Some(0), &camera(), &[], &mut 0);
        assert_eq!(v, HashSet::from([Some(0)]));
    }

    #[test]
    fn links_are_pairs_of_rooms_taken_once() {
        let rooms = vec![room(5, [0.0; 3], [1.0; 3]), room(6, [0.0; 3], [1.0; 3]), room(7, [0.0; 3], [1.0; 3])];
        // Named both ways, a link to a room of another cell, and one to itself.
        let links = [(5, 6), (6, 5), (6, 7), (7, 99), (5, 5)];
        assert_eq!(linked_pairs(&rooms, &links), vec![[0, 1], [1, 2]]);
    }

    #[test]
    fn a_portal_behind_an_occlusion_plane_is_not_followed() {
        let p = |id, x: f32, a, b| Portal { id, corners: portal_corners(Vec3::new(x, 0.0, 0.0), Mat3::IDENTITY, Vec3::new(0.5, 50.0, 50.0)), rooms: [a, b] };
        let portals = vec![p(10, 200.0, Some(0), Some(1)), p(12, 600.0, Some(1), Some(3))];
        // A wall plane between the two portals, wider than the far one's shadow.
        let wall = Plane::new(1, Vec3::new(400.0, 0.0, 0.0), Mat3::IDENTITY, Vec3::new(0.5, 200.0, 200.0));
        let shadows: Vec<Shadow> = wall.shadow_from(Vec3::ZERO).into_iter().collect();
        let mut blocked = 0;
        let v = visible_spaces(&portals, &[], Some(0), &camera(), &shadows, &mut blocked);
        assert!(v.contains(&Some(1)) && !v.contains(&Some(3)));
        assert_eq!(blocked, 1);
    }

    #[test]
    fn a_portal_the_camera_stands_in_passes_the_whole_view() {
        let corners = vec![Vec3::new(-10.0, -50.0, -50.0), Vec3::new(10.0, 50.0, -50.0), Vec3::new(10.0, 50.0, 50.0), Vec3::new(-10.0, -50.0, 50.0)];
        assert_eq!(portal_rect(&corners, &camera()), Some(FULL));
        let behind: Vec<Vec3> = corners.iter().map(|c| Vec3::new(-100.0 - c.x.abs(), c.y, c.z)).collect();
        assert_eq!(portal_rect(&behind, &camera()), None);
    }
    #[test]
    fn a_mesh_reaching_out_of_its_rooms_is_in_the_unbounded_space_too() {
        let room = RoomBox { id: 1, center: Vec3::ZERO, rot: Mat3::IDENTITY, half: Vec3::splat(100.0) };
        // Inside the room: the room's only.
        let inside = InRooms::of(&[room], Vec3::splat(-50.0), Vec3::splat(50.0));
        assert_eq!((inside.0.as_slice(), inside.1), (&[0usize][..], false));
        // Touching the room's corner, most of it out past a doorway (#4485): both.
        let out = InRooms::of(&[room], Vec3::new(110.0, -50.0, -50.0), Vec3::new(600.0, 50.0, 50.0));
        assert_eq!((out.0.as_slice(), out.1), (&[0usize][..], true));
        let mut v = HashSet::new();
        v.insert(None);
        assert!(out.seen_in(&v) && !inside.seen_in(&v), "seen through a portal to the unbounded space");
        // Touching no room: the unbounded space's only.
        let far = InRooms::of(&[room], Vec3::splat(500.0), Vec3::splat(600.0));
        assert!(far.0.is_empty() && far.1);
    }
}
