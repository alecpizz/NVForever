//! Graph, path and walker tests on synthetic navmeshes (no game data).

use crate::graph::{EdgeKind, NavGraph, Space, NONE};
use crate::path::{PathError, PathParams, Place};
use crate::record::{link_kind, tri_flags, DoorLink, EdgeLink, NavMeshRecord, Triangle};
use crate::follow::goal_height_fits;
use crate::{Follower, Steer, WalkEvent, Walker};
use std::collections::HashSet;

const W: Space = Space::World(0x3C);

/// A navmesh over a grid of `nx` × `ny` squares of side `size` from `origin`, each
/// square two counter-clockwise triangles (south-east and north-west of its diagonal),
/// except the squares in `holes`. Heights follow `z(x, y)`.
fn grid(form_id: u32, origin: [f32; 2], nx: usize, ny: usize, size: f32, holes: &[(usize, usize)], z: impl Fn(f32, f32) -> f32) -> NavMeshRecord {
    let vid = |i: usize, j: usize| (j * (nx + 1) + i) as u16;
    let mut vertices = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            let (x, y) = (origin[0] + i as f32 * size, origin[1] + j as f32 * size);
            vertices.push([x, y, z(x, y)]);
        }
    }
    let holes: HashSet<(usize, usize)> = holes.iter().copied().collect();
    // Triangle index of (square, half): half 0 = (a, b, c), half 1 = (a, c, d).
    let mut index = std::collections::HashMap::new();
    for j in 0..ny {
        for i in 0..nx {
            if !holes.contains(&(i, j)) {
                for h in 0..2 {
                    let n = index.len() as i16;
                    index.insert((i, j, h), n);
                }
            }
        }
    }
    let at = |i: isize, j: isize, h: usize| -> i16 {
        if i < 0 || j < 0 {
            return -1;
        }
        index.get(&(i as usize, j as usize, h)).copied().unwrap_or(-1)
    };
    let mut triangles = vec![Triangle { v: [0; 3], edges: [-1; 3], flags: 0, cover: 0 }; index.len()];
    for (&(i, j, h), &n) in &index {
        let (a, b, c, d) = (vid(i, j), vid(i + 1, j), vid(i + 1, j + 1), vid(i, j + 1));
        let (ii, jj) = (i as isize, j as isize);
        triangles[n as usize] = if h == 0 {
            Triangle { v: [a, b, c], edges: [at(ii, jj - 1, 1), at(ii + 1, jj, 1), at(ii, jj, 1)], flags: 0, cover: 0 }
        } else {
            Triangle { v: [a, c, d], edges: [at(ii, jj, 0), at(ii, jj + 1, 0), at(ii - 1, jj, 0)], flags: 0, cover: 0 }
        };
    }
    NavMeshRecord { form_id, vertices, triangles, ..Default::default() }
}

/// Link `a`'s east boundary edges to `b`'s west boundary edges (and back) where they
/// overlap, as NVEX portals.
fn seam(a: &mut NavMeshRecord, b: &mut NavMeshRecord, kind: u32) {
    let ax = a.vertices.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
    let east: Vec<(usize, usize, [f32; 3], [f32; 3])> = edges_at(a, ax);
    let west = edges_at(b, ax);
    for &(at, ae, p0, p1) in &east {
        let (lo, hi) = (p0[1].min(p1[1]), p0[1].max(p1[1]));
        let mid = (lo + hi) / 2.0;
        if let Some(&(bt, be, ..)) = west.iter().find(|w| w.2[1].min(w.3[1]) <= mid && mid <= w.2[1].max(w.3[1])) {
            link(a, at, ae, b.form_id, bt, kind);
            link(b, bt, be, a.form_id, at, kind);
        }
    }
}

/// Boundary edges of `m` lying on the line x = `x`: (triangle, edge, ends).
fn edges_at(m: &NavMeshRecord, x: f32) -> Vec<(usize, usize, [f32; 3], [f32; 3])> {
    let mut out = Vec::new();
    for (ti, t) in m.triangles.iter().enumerate() {
        for e in 0..3 {
            let (p, q) = (m.vertices[t.v[e] as usize], m.vertices[t.v[(e + 1) % 3] as usize]);
            if t.edges[e] == -1 && (p[0] - x).abs() < 1e-3 && (q[0] - x).abs() < 1e-3 {
                out.push((ti, e, p, q));
            }
        }
    }
    out
}

fn link(m: &mut NavMeshRecord, t: usize, e: usize, to: u32, tt: usize, kind: u32) {
    m.triangles[t].flags |= 1 << e;
    m.triangles[t].edges[e] = m.edge_links.len() as i16;
    m.edge_links.push(EdgeLink { kind, navmesh: Some(to), triangle: tt as u16 });
}

fn graph(meshes: &[(NavMeshRecord, Space)]) -> NavGraph {
    let input: Vec<_> = meshes.iter().map(|(m, s)| (m, Some(0), Some(*s))).collect();
    NavGraph::assemble(&input)
}

fn flat(_: f32, _: f32) -> f32 {
    0.0
}

/// Every point along the polyline (every `step` units) has navmesh under it.
fn on_mesh(g: &NavGraph, space: Space, points: &[[f32; 3]], step: f32) -> bool {
    points.windows(2).all(|w| {
        let n = ((w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]) / step).ceil().max(1.0) as usize;
        (0..=n).all(|k| {
            let s = k as f32 / n as f32;
            let p = [w[0][0] + (w[1][0] - w[0][0]) * s, w[0][1] + (w[1][1] - w[0][1]) * s, w[0][2] + (w[1][2] - w[0][2]) * s];
            g.locate(space, p, 1000.0).is_some()
        })
    })
}

#[test]
fn internal_edges_resolve_and_outline_is_marked() {
    let g = graph(&[(grid(1, [0.0, 0.0], 3, 2, 100.0, &[], flat), W)]);
    assert_eq!(g.tris.len(), 12);
    assert_eq!(g.stats.internal_edges, 12 * 3 - 10); // 36 triangle edges, 10 on the outline
    assert_eq!(g.stats.boundary_edges, 10);
    // Every internal neighbour points back.
    for (i, t) in g.tris.iter().enumerate() {
        for e in 0..3 {
            if t.kind[e] == EdgeKind::Internal {
                assert!(g.tris[t.next[e] as usize].next.contains(&(i as u32)));
            }
        }
    }
    // The centre vertex (1, 1) of the grid is inside; the corner (0, 0) on the outline.
    assert!(g.boundary_vertex[0]);
    assert!(!g.boundary_vertex[4 + 1]);
    let t = g.locate(W, [150.0, 50.0, 10.0], 100.0).expect("triangle under the point");
    assert!(g.contains_xy(t, 150.0, 50.0));
    assert_eq!(g.locate(W, [350.0, 50.0, 0.0], 100.0), None);
}

#[test]
fn straight_path_in_the_open() {
    let g = graph(&[(grid(1, [0.0, 0.0], 10, 10, 100.0, &[], flat), W)]);
    let p = g.find_path(Place { space: W, pos: [150.0, 150.0, 0.0] }, Place { space: W, pos: [850.0, 750.0, 0.0] }, &PathParams::default()).unwrap();
    assert_eq!(p.legs.len(), 1);
    assert_eq!(p.legs[0].points.len(), 2, "{:?}", p.legs[0].points);
    assert!((p.length - (700.0f32).hypot(600.0)).abs() < 1.0);
}

#[test]
fn path_goes_round_a_wall_with_clearance() {
    // A wall of holes across x = 400..500 from y = 0 to 800; the way round is at the north.
    let holes: Vec<(usize, usize)> = (0..8).map(|j| (4, j)).collect();
    let g = graph(&[(grid(1, [0.0, 0.0], 10, 10, 100.0, &holes, flat), W)]);
    let params = PathParams { radius: 24.0, ..Default::default() };
    let p = g.find_path(Place { space: W, pos: [150.0, 150.0, 0.0] }, Place { space: W, pos: [850.0, 150.0, 0.0] }, &params).unwrap();
    let pts = &p.legs[0].points;
    assert!(pts.len() >= 4, "{pts:?}");
    assert!(on_mesh(&g, W, pts, 5.0), "{pts:?}");
    // Every corner keeps the clearance from the wall (x 400-500, y 0-800).
    for c in &pts[1..pts.len() - 1] {
        let dx = (400.0 - c[0]).max(c[0] - 500.0).max(0.0);
        let dy = (c[1] - 800.0).max(0.0);
        assert!(dx.hypot(dy) >= 23.9, "corner too close to the wall: {c:?} in {pts:?}");
    }
    // Rounding the wall's end, the path never turns back west.
    assert!(pts.windows(2).all(|w| w[1][0] >= w[0][0] - 1e-3), "{pts:?}");
    assert!(p.length > 1400.0 && p.length < 1700.0, "{}", p.length);
}

#[test]
fn path_crosses_a_seam_between_navmeshes_whose_grids_differ() {
    let mut a = grid(1, [0.0, 0.0], 4, 4, 100.0, &[], flat);
    let mut b = grid(2, [400.0, -50.0], 3, 3, 150.0, &[], flat);
    seam(&mut a, &mut b, link_kind::PORTAL);
    let g = graph(&[(a, W), (b, W)]);
    assert!(g.stats.portal_edges > 0);
    assert_eq!(g.stats.unresolved_links, 0);
    let p = g.find_path(Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: W, pos: [800.0, 300.0, 0.0] }, &PathParams::default()).unwrap();
    assert!(on_mesh(&g, W, &p.legs[0].points, 5.0));
    assert!((p.length - (750.0f32).hypot(250.0)).abs() < 2.0, "{} {:?}", p.length, p.legs[0].points);
    assert_eq!(p.legs[0].points.len(), 2, "{:?}", p.legs[0].points);
    let meshes: HashSet<u32> = p.legs[0].corridor.iter().map(|&t| g.tris[t as usize].mesh).collect();
    assert_eq!(meshes.len(), 2);
}

#[test]
fn unlinked_navmeshes_are_unreachable_and_ledges_are_opt_in() {
    let a = grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let b = grid(2, [200.0, 0.0], 2, 2, 100.0, &[], |_, _| -300.0);
    let g = graph(&[(a.clone(), W), (b.clone(), W)]);
    let from = Place { space: W, pos: [50.0, 50.0, 0.0] };
    let to = Place { space: W, pos: [350.0, 50.0, -300.0] };
    assert!(matches!(g.find_path(from, to, &PathParams::default()), Err(PathError::Unreachable { .. })));
    assert!(g.find_path(from, to, &PathParams { safe_drop_max: Some(300.0), ..Default::default() }).is_err());
    // The same two joined by a ledge down (and up, back).
    let (mut a, mut b) = (a, b);
    let (ea, eb) = (edges_at(&a, 200.0), edges_at(&b, 200.0));
    for &(at, ae, ..) in &ea {
        link(&mut a, at, ae, 2, eb[0].0, link_kind::LEDGE_DOWN);
    }
    for &(bt, be, ..) in &eb {
        link(&mut b, bt, be, 1, ea[0].0, link_kind::LEDGE_UP);
    }
    let g = graph(&[(a, W), (b, W)]);
    assert!(g.stats.ledge_edges > 0);
    assert!(g.find_path(from, to, &PathParams::default()).is_err());
    let safe = PathParams { safe_drop_max: Some(300.0), ..Default::default() };
    let drop = g.find_path(from, to, &safe).unwrap();
    assert_eq!(drop.legs[0].hops.len(), 1);
    assert!(g.find_path(from, to, &PathParams { safe_drop_max: Some(299.0), ..safe.clone() }).is_err());
    assert!(g.find_path(to, from, &safe).is_err());
    let mut walker = Walker::new(W, from.pos, 0.0, 100.0);
    walker.go_to(&g, to, &safe).unwrap();
    let mut airborne = false;
    for _ in 0..200 {
        walker.update(&g, 0.05);
        airborne |= walker.air.is_some();
    }
    assert!(airborne);
    assert!((walker.feet[2] + 300.0).abs() < 1.0);
    assert!((walker.feet[0] - to.pos[0]).abs() < 2.0);
    let ledges = PathParams { ledges: true, ..Default::default() };
    let p = g.find_path(from, to, &ledges).unwrap();
    assert_eq!(p.end().map(|e| e[2]), Some(-300.0));
    // The drop is a hop: off the top edge at x 200, onto the lower navmesh beyond it.
    let leg = &p.legs[0];
    assert_eq!(leg.hops.len(), 1, "{:?}", leg.points);
    let h = leg.hops[0];
    assert!(!h.up);
    let (off, land) = (leg.points[h.point], leg.points[h.point + 1]);
    assert!((off[0] - 200.0).abs() < 1e-3 && off[2] == 0.0, "{off:?}");
    assert!(land[0] > 210.0 && land[0] < 300.0 && land[2] == -300.0, "{land:?}");
    // Back up is 300: out of a jump's reach, so no way back.
    assert!(g.find_path(to, from, &ledges).is_err());
    assert!(g.find_path(to, from, &PathParams { ledge_rise_max: 400.0, ..ledges.clone() }).is_ok());
}

#[test]
fn a_step_between_unlinked_areas_is_found_where_they_lie_close() {
    // SSHQ01's shape (#3473): a floor and, 24 units beyond its east edge and 31 below, a
    // stair top on a navmesh of its own, no link between them.
    let a = grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let b = grid(2, [224.0, 0.0], 2, 2, 100.0, &[], |_, _| -31.0);
    let g = graph(&[(a, W), (b, W)]);
    let from = Place { space: W, pos: [50.0, 50.0, 0.0] };
    let to = Place { space: W, pos: [350.0, 50.0, -31.0] };
    assert!(matches!(g.find_path(from, to, &PathParams::default()), Err(PathError::Unreachable { .. })));
    let area = |p: Place| g.area[g.locate(p.space, p.pos, 64.0).unwrap() as usize];
    let (fa, ta) = (area(from), area(to));
    assert_ne!(fa, ta);
    let (off, on) = g.step_between(W, fa, ta, 48.0, 48.0).expect("a step");
    // Off the floor's east edge, onto the lower navmesh, a little way into it.
    assert!((off[0] - 200.0).abs() < 1e-3 && off[2] == 0.0, "{off:?}");
    assert!(on[0] > 224.0 && on[0] <= 224.0 + crate::graph::STEP_IN + 1.0 && on[2] == -31.0, "{on:?}");
    assert_eq!(area(Place { space: W, pos: on }), ta);
    // From there the way goes on; and both ways are found.
    assert!(g.find_path(Place { space: W, pos: on }, to, &PathParams::default()).is_ok());
    assert!(g.step_between(W, ta, fa, 48.0, 48.0).is_some());
    // Too far apart, too high, or the same area: none.
    assert!(g.step_between(W, fa, ta, 20.0, 48.0).is_none());
    assert!(g.step_between(W, fa, ta, 48.0, 30.0).is_none());
    assert!(g.step_between(W, fa, fa, 48.0, 48.0).is_none());
    assert!(g.step_between(Space::Interior(9), fa, ta, 48.0, 48.0).is_none());
}

#[test]
fn walkers_take_off_from_the_ledge_links_point() {
    let a = grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let b = grid(2, [200.0, 0.0], 2, 2, 100.0, &[], |_, _| -100.0);
    let (mut a, b) = (a, b);
    let (ea, eb) = (edges_at(&a, 200.0), edges_at(&b, 200.0));
    for &(at, ae, ..) in &ea {
        link(&mut a, at, ae, 2, eb[0].0, link_kind::LEDGE_DOWN);
    }
    let g = graph(&[(a, W), (b, W)]);
    let ledges = PathParams { ledges: true, ..Default::default() };
    let to = Place { space: W, pos: [350.0, 50.0, -100.0] };
    let p = g.find_path(Place { space: W, pos: [50.0, 50.0, 0.0] }, to, &ledges).unwrap();
    let off = p.legs[0].points[p.legs[0].hops[0].point];
    // At any walking speed and frame step the flight leaves from the same point.
    for (speed, dt) in [(85.3, 1.0 / 60.0), (86.8, 1.0 / 60.0), (85.3, 1.0 / 37.0), (344.0, 1.0 / 30.0)] {
        let mut w = Walker::new(W, [50.0, 50.0, 0.0], 0.0, speed);
        w.go_to(&g, to, &ledges).unwrap();
        let mut from = None;
        for _ in 0..2000 {
            w.update(&g, dt);
            if let Some(a) = w.air {
                from = Some(a.flight.from);
                break;
            }
        }
        assert_eq!(from, Some(off), "speed {speed}, step {dt}");
    }
}

#[test]
fn safe_drop_limit_uses_the_sloping_landing_not_the_triangle_centre() {
    let mut a = grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let b = grid(2, [200.0, 0.0], 2, 2, 100.0, &[], |x, _| -300.0 - (x - 200.0) * 0.5);
    let (ea, eb) = (edges_at(&a, 200.0), edges_at(&b, 200.0));
    for &(at, ae, ..) in &ea {
        link(&mut a, at, ae, 2, eb[0].0, link_kind::LEDGE_DOWN);
    }
    let g = graph(&[(a, W), (b, W)]);
    let from = Place { space: W, pos: [50.0, 50.0, 0.0] };
    let to = Place { space: W, pos: [350.0, 50.0, -375.0] };
    let p = g.find_path(from, to, &PathParams { ledges: true, ..Default::default() }).unwrap();
    let leg = &p.legs[0];
    let h = leg.hops[0];
    let height = leg.points[h.point][2] - leg.points[h.point + 1][2];
    assert!(height > 300.0 && height < 320.0, "{height}");
    let safe = PathParams { safe_drop_max: Some(height), ..Default::default() };
    assert!(g.find_path(from, to, &safe).is_ok());
    assert!(g.find_path(from, to, &PathParams { safe_drop_max: Some(299.0), ..safe }).is_err());
}

#[test]
fn low_ledges_are_jumped_up() {
    let a = grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let b = grid(2, [200.0, 0.0], 2, 2, 100.0, &[], |_, _| 40.0);
    let (mut a, mut b) = (a, b);
    let (ea, eb) = (edges_at(&a, 200.0), edges_at(&b, 200.0));
    for &(at, ae, ..) in &ea {
        link(&mut a, at, ae, 2, eb[0].0, link_kind::LEDGE_UP);
    }
    for &(bt, be, ..) in &eb {
        link(&mut b, bt, be, 1, ea[0].0, link_kind::LEDGE_DOWN);
    }
    let g = graph(&[(a, W), (b, W)]);
    let ledges = PathParams { ledges: true, ..Default::default() };
    let p = g.find_path(Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: W, pos: [350.0, 50.0, 40.0] }, &ledges).unwrap();
    assert_eq!(p.legs[0].hops.iter().map(|h| h.up).collect::<Vec<_>>(), vec![true]);
    assert_eq!(p.end().map(|e| e[2]), Some(40.0));
}

#[test]
fn doors_join_spaces_into_legs() {
    let inside = Space::Interior(0x77);
    let mut a = grid(1, [0.0, 0.0], 4, 4, 100.0, &[], flat);
    let mut b = grid(2, [5000.0, 5000.0], 3, 3, 100.0, &[], |_, _| 50.0);
    // Door 0xD1 next to a's triangle at (350, 350); door 0xD2 next to b's at (5050, 5050).
    let ta = (0..a.triangles.len()).find(|&t| {
        let v = a.triangles[t].v.map(|i| a.vertices[i as usize]);
        v.iter().all(|p| p[0] >= 300.0 && p[1] >= 300.0)
    });
    a.door_links.push(DoorLink { door: 0xD1, triangle: ta.unwrap() as u16 });
    a.triangles[ta.unwrap()].flags |= tri_flags::DOOR;
    b.door_links.push(DoorLink { door: 0xD2, triangle: 0 });
    let meshes = [(a, W), (b, inside)];
    let input: Vec<_> = meshes.iter().map(|(m, s)| (m, Some(0), Some(*s))).collect();
    let mut g = NavGraph::assemble(&input);
    g.join_doors(&input, |door| match door {
        0xD1 => Some((0xD2, [5050.0, 5030.0, 50.0], 0.5, inside)),
        0xD2 => Some((0xD1, [340.0, 340.0, 0.0], 3.0, W)),
        _ => None,
    });
    assert_eq!(g.stats.doors, 2);
    let p = g.find_path(Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: inside, pos: [5250.0, 5250.0, 50.0] }, &PathParams::default()).unwrap();
    assert_eq!(p.legs.len(), 2);
    assert_eq!(p.legs[0].space, W);
    assert_eq!(p.legs[0].door.map(|d| d.door), Some(0xD1));
    assert_eq!(p.legs[1].space, inside);
    assert_eq!(p.legs[1].points[0], [5050.0, 5030.0, 50.0]);
    // Without doors there is no way in.
    assert!(g.find_path(Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: inside, pos: [5250.0, 5250.0, 50.0] }, &PathParams { doors: false, ..Default::default() }).is_err());
    // Nor for an actor that opens no doors ("Can't Open Doors").
    assert!(g.find_path(Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: inside, pos: [5250.0, 5250.0, 50.0] }, &PathParams { opens_doors: false, ..Default::default() }).is_err());

    // A walker goes all the way through the door.
    let mut w = Walker::new(W, [50.0, 50.0, 0.0], 0.0, 100.0);
    w.go_to(&g, Place { space: inside, pos: [5250.0, 5250.0, 50.0] }, &PathParams::default()).unwrap();
    let mut events = Vec::new();
    for _ in 0..2000 {
        match w.update(&g, 0.05) {
            WalkEvent::Moved | WalkEvent::Turned => {}
            e => events.push(e),
        }
        if w.follower.is_none() {
            break;
        }
    }
    assert!(matches!(events[0], WalkEvent::UsedDoor(_)), "{events:?}");
    assert_eq!(events.last(), Some(&WalkEvent::Arrived));
    assert_eq!(w.space, inside);
    assert!((w.feet[0] - 5250.0).abs() < 1.0 && (w.feet[1] - 5250.0).abs() < 1.0);
}

#[test]
fn a_walker_stops_and_faces_a_load_door_before_going_through() {
    use crate::follow::{heading_delta, heading_of};
    let inside = Space::Interior(0x77);
    let mut a = grid(1, [0.0, 0.0], 4, 4, 100.0, &[], flat);
    let b = grid(2, [5000.0, 5000.0], 3, 3, 100.0, &[], |_, _| 50.0);
    let ta = (0..a.triangles.len())
        .find(|&t| a.triangles[t].v.map(|i| a.vertices[i as usize]).iter().all(|p| p[0] >= 300.0 && p[1] >= 300.0))
        .unwrap();
    a.door_links.push(DoorLink { door: 0xD1, triangle: ta as u16 });
    a.triangles[ta].flags |= tri_flags::DOOR;
    let meshes = [(a, W), (b, inside)];
    let input: Vec<_> = meshes.iter().map(|(m, s)| (m, Some(0), Some(*s))).collect();
    let mut g = NavGraph::assemble(&input);
    g.join_doors(&input, |door| (door == 0xD1).then_some((0xD2, [5050.0, 5030.0, 50.0], 0.5, inside)));
    // The door stands due north of where the path ends; the walk comes up from the
    // south-west, at about 45 degrees to it.
    let door = [360.0, 700.0, 0.0];
    g.place_doors(|d| (d == 0xD1).then_some(door));

    let mut w = Walker::new(W, [50.0, 50.0, 0.0], 0.0, 100.0);
    w.go_to(&g, Place { space: inside, pos: [5250.0, 5250.0, 50.0] }, &PathParams::default()).unwrap();
    let (mut turns_at_door, mut stood_at) = (0, None);
    for _ in 0..2000 {
        let (feet, heading) = (w.feet, w.heading);
        match w.update(&g, 0.05) {
            WalkEvent::Moved => turns_at_door = 0,
            WalkEvent::Turned => {
                // Turning on the spot: the feet stay put.
                assert_eq!(w.feet, feet);
                turns_at_door += 1;
                stood_at = Some(feet);
            }
            WalkEvent::UsedDoor(d) => {
                assert_eq!(d.door_at, Some(door));
                assert!(turns_at_door > 0, "went through without stopping to turn");
                assert_eq!(stood_at, Some(feet));
                let want = heading_of([door[0] - feet[0], door[1] - feet[1]]);
                assert!(heading_delta(heading, want).abs() < 1e-3, "faced {heading}, door at {want}");
                assert!(w.door_off.is_some_and(|o| o < 1e-3));
                assert_eq!(w.space, inside);
                return;
            }
            e => panic!("unexpected {e:?}"),
        }
    }
    panic!("never reached the door");
}

/// A door on a one-triangle navmesh whose portal lands on the floor's edge, where the
/// floor's own link leads elsewhere (Lonesome Road's silo lift, 0x0100A556): the floor's
/// edge is linked back, and a path from the floor goes through the door.
#[test]
fn a_one_way_portal_is_linked_back_from_the_edge_it_lands_on() {
    let inside = Space::Interior(0x77);
    let mut a = grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let mut b = grid(2, [5000.0, 5000.0], 3, 3, 100.0, &[], |_, _| 50.0);
    // The patch east of a: one triangle on a's east edge at y 0..100.
    let (at, ae, p0, p1) = edges_at(&a, 200.0).into_iter().find(|e| e.2[1].max(e.3[1]) <= 100.0).unwrap();
    let mut patch = NavMeshRecord { form_id: 3, vertices: vec![p1, p0, [260.0, 50.0, 0.0]], triangles: vec![Triangle { v: [0, 1, 2], edges: [-1; 3], flags: 0, cover: 0 }], ..Default::default() };
    link(&mut patch, 0, 0, 1, at, 0);
    // a's edge there links, stale, to a triangle of b far off.
    link(&mut a, at, ae, 2, 0, 0);
    patch.door_links.push(DoorLink { door: 0xD1, triangle: 0 });
    b.door_links.push(DoorLink { door: 0xD2, triangle: 0 });
    let meshes = [(a, W), (patch, W), (b, inside)];
    let input: Vec<_> = meshes.iter().map(|(m, s)| (m, Some(0), Some(*s))).collect();
    let mut g = NavGraph::assemble(&input);
    assert_eq!(g.stats.portals_joined_back, 1);
    g.join_doors(&input, |door| match door {
        0xD1 => Some((0xD2, [5050.0, 5030.0, 50.0], 0.5, inside)),
        0xD2 => Some((0xD1, [240.0, 50.0, 0.0], 3.0, W)),
        _ => None,
    });
    let p = g.find_path(Place { space: W, pos: [20.0, 180.0, 0.0] }, Place { space: inside, pos: [5250.0, 5250.0, 50.0] }, &PathParams::default()).unwrap();
    assert_eq!(p.legs[0].door.map(|d| d.door), Some(0xD1));
}

#[test]
fn routes_chain_interiors_and_worldspaces_and_respect_locks() {
    use crate::graph::DoorLock;
    // Worldspace W -> interior A -> interior B -> worldspace V, each its own navmesh far
    // apart; a door near each mesh's north-east corner leads on to the next one's
    // south-west corner. B's door on to V is locked with key 0x4B.
    let (a, b, v) = (Space::Interior(0xA), Space::Interior(0xB), Space::World(0x56));
    let spaces = [W, a, b, v];
    let mut meshes: Vec<NavMeshRecord> = (0..4).map(|i| grid(10 + i as u32, [i as f32 * 10_000.0, 0.0], 4, 4, 100.0, &[], flat)).collect();
    let corner = |m: &NavMeshRecord| {
        let x0 = m.vertices[0][0];
        (0..m.triangles.len()).find(|&t| m.triangles[t].v.iter().all(|&i| m.vertices[i as usize][0] >= x0 + 300.0 && m.vertices[i as usize][1] >= 300.0)).unwrap()
    };
    for (i, m) in meshes.iter_mut().enumerate().take(3) {
        let t = corner(m);
        m.door_links.push(DoorLink { door: 0xD0 + i as u32, triangle: t as u16 });
    }
    let input: Vec<_> = meshes.iter().zip(spaces).map(|(m, s)| (m, Some(0), Some(s))).collect();
    let mut g = NavGraph::assemble(&input);
    g.join_doors(&input, |door| {
        let i = (door - 0xD0) as usize;
        let next = (i + 1) as f32 * 10_000.0;
        Some((0xE0 + i as u32, [next + 50.0, 50.0, 0.0], 0.0, spaces[i + 1]))
    });
    g.set_door_lock(0xD2, Some(DoorLock { level: 255, key: Some(0x4B) }));
    assert_eq!(g.stats.doors, 3);
    let from = Place { space: W, pos: [50.0, 50.0, 0.0] };
    let to = Place { space: v, pos: [30_350.0, 350.0, 0.0] };
    let open = PathParams::default();
    // Locked without the key: no way through.
    assert!(!g.can_reach(from, to, &open));
    assert!(matches!(g.find_path(from, to, &open), Err(PathError::Unreachable { .. })));
    // An interior short of the lock is still reachable, through one door.
    let inside_a = Place { space: a, pos: [10_200.0, 200.0, 0.0] };
    assert_eq!(g.door_route(from, inside_a, &open).unwrap().len(), 1);
    // With the key: three doors, four legs, ending in the other worldspace.
    let keyed = PathParams { keys: vec![0x4B], ..Default::default() };
    let route = g.door_route(from, to, &keyed).unwrap();
    assert_eq!(route.iter().map(|d| d.door).collect::<Vec<_>>(), vec![0xD0, 0xD1, 0xD2]);
    let p = g.find_path(from, to, &keyed).unwrap();
    assert_eq!(p.legs.iter().map(|l| l.space).collect::<Vec<_>>(), spaces.to_vec());
    assert_eq!(p.end(), Some([30_350.0, 350.0, 0.0]));
    // Unlocking the door at run time opens the way for anyone.
    g.set_door_lock(0xD2, None);
    assert!(g.can_reach(from, to, &open));
    // The game's lock states brought in: 0xD1 locked by a script; a door the graph
    // does not know is passed over; 0xD0 and 0xD2, not named, keep theirs.
    assert_eq!(g.door_refs(), vec![0xD0, 0xD1, 0xD2]);
    let lock = DoorLock { level: 50, key: None };
    let changes = g.sync_door_locks([(0xD1, Some(lock)), (0xFF, Some(lock))]);
    assert_eq!(changes, crate::graph::DoorLockChanges { unlocked: vec![], locked: vec![0xD1] });
    assert!(!g.door_locks.contains_key(&0xFF));
    assert!(!g.can_reach(from, to, &open));
    assert!(g.sync_door_locks([(0xD1, Some(lock))]).is_empty(), "nothing new");
    // Unlocked again (a script's Unlock): the way is open and the door is reported.
    let changes = g.sync_door_locks([(0xD0, None), (0xD1, None)]);
    assert_eq!(changes.unlocked, vec![0xD1]);
    assert!(g.can_reach(from, to, &open));
}

/// Whether two places are joined by doors at all counts locked and disabled doors; a
/// navmesh no door leads to is joined to nothing (Gomez's side room in the NCR safehouse,
/// issue #2405), and a place off every navmesh is not known.
#[test]
fn joined_by_doors_counts_locked_and_disabled_doors_and_not_islands() {
    use crate::graph::DoorLock;
    let (a, b, island) = (Space::Interior(0xA), Space::Interior(0xB), Space::Interior(0xC));
    let spaces = [W, a, b, island];
    let mut meshes: Vec<NavMeshRecord> = (0..4).map(|i| grid(10 + i as u32, [i as f32 * 10_000.0, 0.0], 4, 4, 100.0, &[], flat)).collect();
    let corner = |m: &NavMeshRecord| {
        let x0 = m.vertices[0][0];
        (0..m.triangles.len()).find(|&t| m.triangles[t].v.iter().all(|&i| m.vertices[i as usize][0] >= x0 + 300.0 && m.vertices[i as usize][1] >= 300.0)).unwrap()
    };
    for (i, m) in meshes.iter_mut().enumerate().take(2) {
        let t = corner(m);
        m.door_links.push(DoorLink { door: 0xD0 + i as u32, triangle: t as u16 });
    }
    let input: Vec<_> = meshes.iter().zip(spaces).map(|(m, s)| (m, Some(0), Some(s))).collect();
    let mut g = NavGraph::assemble(&input);
    g.join_doors(&input, |door| {
        let i = (door - 0xD0) as usize;
        let next = (i + 1) as f32 * 10_000.0;
        Some((0xE0 + i as u32, [next + 50.0, 50.0, 0.0], 0.0, spaces[i + 1]))
    });
    let from = Place { space: W, pos: [50.0, 50.0, 0.0] };
    let to = Place { space: b, pos: [20_350.0, 350.0, 0.0] };
    let open = PathParams::default();
    assert_eq!(g.joined_by_doors(from, to), Some(true));
    // Locked without a key, then disabled: no way for anyone, but joined still.
    g.set_door_lock(0xD1, Some(DoorLock { level: 25, key: Some(0x4B) }));
    assert!(!g.can_reach(from, to, &open));
    assert_eq!(g.joined_by_doors(from, to), Some(true), "a locked door joins");
    g.sync_door_presence([(0xD1, false)]);
    assert!(!g.can_reach(from, to, &PathParams { open_locked: true, ..Default::default() }));
    assert_eq!(g.joined_by_doors(from, to), Some(true), "a disabled door joins");
    // The fourth navmesh has no door: joined to nothing; far off every navmesh: not known.
    assert_eq!(g.joined_by_doors(from, Place { space: island, pos: [30_350.0, 350.0, 0.0] }), Some(false));
    assert_eq!(g.joined_by_doors(from, Place { space: island, pos: [90_000.0, 350.0, 0.0] }), None);
}

/// A load door not in the world (disabled by a script or by its enable parent) is no way
/// through, and the game's presence states brought in report what changed (issue #2566:
/// Loyal put in the Nellis hangar behind a disabled door).
#[test]
fn disabled_load_doors_take_no_part_in_routes() {
    let (a, b) = (Space::Interior(0xA), Space::Interior(0xB));
    let spaces = [W, a, b];
    let mut meshes: Vec<NavMeshRecord> = (0..3).map(|i| grid(10 + i as u32, [i as f32 * 10_000.0, 0.0], 4, 4, 100.0, &[], flat)).collect();
    let corner = |m: &NavMeshRecord| {
        let x0 = m.vertices[0][0];
        (0..m.triangles.len()).find(|&t| m.triangles[t].v.iter().all(|&i| m.vertices[i as usize][0] >= x0 + 300.0 && m.vertices[i as usize][1] >= 300.0)).unwrap()
    };
    for (i, m) in meshes.iter_mut().enumerate().take(2) {
        let t = corner(m);
        m.door_links.push(DoorLink { door: 0xD0 + i as u32, triangle: t as u16 });
    }
    let input: Vec<_> = meshes.iter().zip(spaces).map(|(m, s)| (m, Some(0), Some(s))).collect();
    let mut g = NavGraph::assemble(&input);
    g.join_doors(&input, |door| {
        let i = (door - 0xD0) as usize;
        let next = (i + 1) as f32 * 10_000.0;
        Some((0xE0 + i as u32, [next + 50.0, 50.0, 0.0], 0.0, spaces[i + 1]))
    });
    let from = Place { space: W, pos: [50.0, 50.0, 0.0] };
    let to = Place { space: b, pos: [20_350.0, 350.0, 0.0] };
    let p = PathParams::default();
    assert_eq!(g.door_route(from, to, &p).unwrap().len(), 2);
    // A's door on to B disabled: B is out of reach, A is not; a door the graph has no load
    // door for is passed over.
    let changes = g.sync_door_presence([(0xD0, true), (0xD1, false), (0xFF, false)]);
    assert_eq!(changes, crate::graph::DoorPresenceChanges { gone: vec![0xD1], back: vec![] });
    assert!(!g.disabled_doors.contains(&0xFF));
    assert!(!g.can_reach(from, to, &p));
    assert!(matches!(g.find_path(from, to, &p), Err(PathError::Unreachable { .. })));
    assert!(g.can_reach(from, Place { space: a, pos: [10_200.0, 200.0, 0.0] }, &p));
    // Not even an actor that opens every lock goes through it.
    assert!(!g.can_reach(from, to, &PathParams { open_locked: true, ..Default::default() }));
    assert!(g.sync_door_presence([(0xD1, false)]).is_empty(), "nothing new");
    // Enabled again: the way is open and the door is reported.
    assert_eq!(g.sync_door_presence([(0xD1, true)]).back, vec![0xD1]);
    assert!(g.can_reach(from, to, &p));
}

#[test]
fn walker_follows_the_surface_on_a_slope() {
    let slope = |x: f32, _y: f32| x * 0.25;
    let g = graph(&[(grid(1, [0.0, 0.0], 8, 8, 100.0, &[(3, 3), (3, 4), (4, 3), (4, 4)], slope), W)]);
    let mut w = Walker::new(W, [50.0, 50.0, 12.5], 0.0, 150.0);
    w.go_to(&g, Place { space: W, pos: [750.0, 750.0, 187.5] }, &PathParams::default()).unwrap();
    let mut steps = 0;
    while w.follower.is_some() && steps < 10_000 {
        w.update(&g, 1.0 / 60.0);
        steps += 1;
        assert!((w.feet[2] - slope(w.feet[0], w.feet[1])).abs() < 0.5, "off the surface at {:?}", w.feet);
        assert!(w.tri == NONE || g.contains_xy(w.tri, w.feet[0], w.feet[1]));
    }
    assert!(w.follower.is_none(), "did not arrive");
    assert!((w.feet[0] - 750.0).abs() < 1.0 && (w.feet[1] - 750.0).abs() < 1.0, "{:?}", w.feet);
    // It went round the hole in the middle, so further than the straight line.
    assert!(w.travelled > 700.0 * std::f32::consts::SQRT_2);
}

#[test]
fn disabled_navmesh_blocks_paths() {
    let mut a = grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let mut b = grid(2, [200.0, 0.0], 2, 2, 100.0, &[], flat);
    seam(&mut a, &mut b, link_kind::PORTAL);
    let mut g = graph(&[(a, W), (b, W)]);
    let (from, to) = (Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: W, pos: [350.0, 150.0, 0.0] });
    assert_eq!(g.find_path(from, to, &PathParams::default()).unwrap().goal_snap, 0.0);
    assert!(g.set_enabled(2, false));
    // The goal is no longer on a usable triangle: it snaps 150 units onto the first
    // navmesh's edge, and the path says so.
    assert_eq!(g.locate(W, to.pos, 10.0), None);
    let p = g.find_path(from, to, &PathParams::default()).unwrap();
    assert!((p.goal_snap - 150.0).abs() < 0.5, "{}", p.goal_snap);
    assert!(!g.set_enabled(9, false));
}

/// The scripts' switches (`EnableNavMesh` / `DisableNavmesh`) applied to the graph: a
/// path planned over a navmesh disabled since is blocked, and the way across is gone
/// until it is enabled again; a navmesh the load order has disabled is enabled by its
/// switch and falls back to disabled without one.
#[test]
fn navmesh_switches_block_and_reopen_the_way() {
    let mut a = grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let mut b = grid(2, [200.0, 0.0], 2, 2, 100.0, &[], flat);
    let mut c = grid(3, [400.0, 0.0], 2, 2, 100.0, &[], flat);
    seam(&mut a, &mut b, link_kind::PORTAL);
    seam(&mut b, &mut c, link_kind::PORTAL);
    let mut g = graph(&[(a, W), (b, W), (c, W)]);
    let (from, to) = (Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: W, pos: [550.0, 150.0, 0.0] });
    let mut w = Walker::new(W, from.pos, 0.0, 100.0);
    w.go_to(&g, to, &PathParams::default()).unwrap();
    assert!(!w.follower.as_ref().unwrap().blocked(&g));
    let mut switches = std::collections::BTreeMap::new();
    switches.insert(2, false);
    assert!(g.apply_switches(&switches));
    assert!(!g.apply_switches(&switches), "applied once");
    assert!(w.follower.as_ref().unwrap().blocked(&g));
    assert!(!g.can_reach(from, to, &PathParams::default()));
    switches.insert(2, true);
    assert!(g.apply_switches(&switches));
    assert!(!w.follower.as_ref().unwrap().blocked(&g));
    assert!(g.can_reach(from, to, &PathParams::default()));
    // Initially disabled (as the record's flag would make it): enabled only by a switch.
    // (The goal then lies on no usable triangle: a path would snap it onto the middle
    // navmesh's edge.)
    g.meshes[2].initially_enabled = false;
    assert!(g.apply_switches(&std::collections::BTreeMap::new()));
    assert_eq!(g.locate(W, to.pos, 10.0), None);
    switches.insert(3, true);
    assert!(g.apply_switches(&switches));
    assert!(g.locate(W, to.pos, 10.0).is_some());
}

/// A door that opens in place across a corridor: 100 wide (its model's x), 8 thick
/// (y), turned so it spans the corridor (game y) at `x`, from y = `y0` to `y0 + 100`.
fn hinged(door: u32, x: f32, y0: f32) -> crate::doors::InPlaceDoor {
    crate::doors::InPlaceDoor {
        door,
        space: W,
        position: [x, y0 + 50.0, 0.0],
        // Model x along game y, model y along game -x.
        rotation: [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        scale: 1.0,
        bounds: ([-50.0, -4.0, 0.0], [50.0, 4.0, 200.0]),
    }
}

/// A corridor 1,000 long and 100 wide with a door across it at x = 500.
fn corridor_with_door(lock: Option<crate::graph::DoorLock>) -> NavGraph {
    let mut g = graph(&[(grid(1, [0.0, 0.0], 10, 1, 100.0, &[], flat), W)]);
    g.set_in_place_doors(vec![hinged(0xD00, 500.0, 0.0)], |_| lock);
    g
}

#[test]
fn a_path_through_a_door_that_opens_in_place_stops_in_front_of_it() {
    let g = corridor_with_door(None);
    assert_eq!(g.stats.in_place_doors, 1);
    assert!(!g.door_tris.is_empty() && g.door_tris.values().all(|d| d == &vec![0xD00]));
    let from = Place { space: W, pos: [50.0, 50.0, 0.0] };
    let to = Place { space: W, pos: [950.0, 50.0, 0.0] };
    let p = g.find_path(from, to, &PathParams::default()).unwrap();
    assert_eq!(p.legs.len(), 1);
    let leg = &p.legs[0];
    assert_eq!(leg.door_stops.len(), 1);
    let s = leg.door_stops[0];
    assert_eq!(s.door, 0xD00);
    // The clearance (24) and the opening's width (the navmesh along the door's line,
    // 96-100 at the 4-unit sampling step: as far as a leaf can swing) before the door at
    // x = 496, in line with the middle of the opening.
    let at = leg.points[s.point];
    assert!((at[0] - (496.0 - 122.0)).abs() <= 2.5 && (at[1] - 50.0).abs() < 0.5, "{at:?}");
    // The polyline is otherwise unchanged: straight along the corridor.
    assert!(leg.points.iter().all(|q| (q[1] - 50.0).abs() < 0.5));

    // The follower walks to the stop, then holds there until the door has opened.
    let mut f = crate::Follower::new(p.clone());
    match f.steer(from.pos) {
        crate::Steer::Walk { target, .. } => assert_eq!(target, at),
        s => panic!("{s:?}"),
    }
    let near = [at[0] - 10.0, 50.0, 0.0];
    assert_eq!(f.steer(near), crate::Steer::OpenDoor { door: 0xD00, at });
    // Still held a moment later, even pushed away from the stop by the opening door.
    assert_eq!(f.steer(near), crate::Steer::OpenDoor { door: 0xD00, at });
    assert_eq!(f.steer([at[0] - 60.0, 70.0, 0.0]), crate::Steer::OpenDoor { door: 0xD00, at });
    f.door_opened();
    // Straight through the middle of the opening to a clearance (24) and the margin (24)
    // beyond the door's far face (x = 504), then on.
    match f.steer(near) {
        crate::Steer::Walk { target, .. } => assert_eq!(target, [552.0, 50.0, 0.0]),
        s => panic!("{s:?}"),
    }
    match f.steer([552.0, 50.0, 0.0]) {
        crate::Steer::Walk { target, .. } => assert_eq!(target, [950.0, 50.0, 0.0]),
        s => panic!("{s:?}"),
    }
    assert_eq!(f.steer([950.0, 50.0, 0.0]), crate::Steer::Arrived);

    // An actor that overshoots the stop is held as well.
    let mut f = crate::Follower::new(p);
    assert!(matches!(f.steer(from.pos), crate::Steer::Walk { .. }));
    assert!(matches!(f.steer([at[0] + 20.0, 50.0, 0.0]), crate::Steer::OpenDoor { door: 0xD00, .. }));
}

#[test]
fn walker_waits_for_the_door_to_open() {
    let g = corridor_with_door(None);
    let mut w = Walker::new(W, [50.0, 50.0, 0.0], 0.0, 100.0);
    w.go_to(&g, Place { space: W, pos: [950.0, 50.0, 0.0] }, &PathParams::default()).unwrap();
    // A door that takes 1.67 s to open, started by the first request.
    let mut opened_at: Option<f32> = None;
    let mut time = 0.0f32;
    let dt = 0.1;
    let mut waiting = Vec::new();
    let mut asked = Vec::new();
    for _ in 0..400 {
        time += dt;
        let ev = w.update_with(&g, dt, &mut |d| {
            asked.push(d);
            let t0 = *opened_at.get_or_insert(time);
            time - t0 >= 1.67
        });
        if let WalkEvent::WaitingForDoor(d) = ev {
            waiting.push((time, w.feet));
            assert_eq!(d, 0xD00);
        }
        if ev == WalkEvent::Arrived {
            break;
        }
    }
    assert!(w.follower.is_none(), "arrived");
    assert!((w.feet[0] - 950.0).abs() < 1.0);
    assert!(asked.iter().all(|&d| d == 0xD00));
    // It stood still in front of the door while it opened, then walked on.
    let (first, last) = (waiting.first().unwrap(), waiting.last().unwrap());
    assert!(last.0 - first.0 >= 1.5, "waited {:.2} s", last.0 - first.0);
    assert_eq!(first.1, last.1);
    assert!((first.1[0] - 372.0).abs() < 17.0, "{:?}", first.1);
    // A door already open is not waited for.
    let mut w = Walker::new(W, [50.0, 50.0, 0.0], 0.0, 100.0);
    w.go_to(&g, Place { space: W, pos: [950.0, 50.0, 0.0] }, &PathParams::default()).unwrap();
    let mut events = Vec::new();
    for _ in 0..200 {
        let ev = w.update_with(&g, dt, &mut |_| true);
        events.push(ev);
        if ev == WalkEvent::Arrived {
            break;
        }
    }
    assert!(events.contains(&WalkEvent::Arrived));
    assert!(!events.iter().any(|e| matches!(e, WalkEvent::WaitingForDoor(_))));
}

#[test]
fn locked_doors_that_open_in_place_close_the_way_without_the_key() {
    use crate::graph::DoorLock;
    let mut g = corridor_with_door(Some(DoorLock { level: 50, key: Some(0x4B) }));
    let from = Place { space: W, pos: [50.0, 50.0, 0.0] };
    let to = Place { space: W, pos: [950.0, 50.0, 0.0] };
    assert!(matches!(g.find_path(from, to, &PathParams::default()), Err(PathError::Unreachable { .. })));
    // Short of the door is still reachable.
    assert!(g.find_path(from, Place { space: W, pos: [350.0, 50.0, 0.0] }, &PathParams::default()).is_ok());
    // Its owner may use it without the key (Fallout 3's playroom door, Dad's).
    for p in [PathParams { keys: vec![0x4B], ..Default::default() }, PathParams { open_locked: true, ..Default::default() }, PathParams { owned_doors: vec![0xD00], ..Default::default() }] {
        let path = g.find_path(from, to, &p).unwrap();
        assert_eq!(path.legs[0].door_stops.len(), 1);
    }
    // Unlocked at run time.
    g.set_door_lock(0xD00, None);
    assert!(g.find_path(from, to, &PathParams::default()).is_ok());
}

#[test]
fn a_door_that_opens_in_place_and_is_not_in_the_world_bars_nothing() {
    // #2405, Point Lookout's mansion: two doors in one doorway, the quest's spawn door
    // (opened by its stage) and a locked one placed initially disabled for later. The
    // disabled one stood in Desmond's way; a door not in the world bars nothing.
    use crate::graph::DoorLock;
    let mut g = graph(&[(grid(1, [0.0, 0.0], 10, 1, 100.0, &[], flat), W)]);
    let locked = DoorLock { level: 255, key: None };
    g.set_in_place_doors(vec![hinged(0xD00, 500.0, 0.0), hinged(0xD01, 500.0, 0.0)], |_| Some(locked));
    let from = Place { space: W, pos: [50.0, 50.0, 0.0] };
    let to = Place { space: W, pos: [950.0, 50.0, 0.0] };
    let spawn_open = PathParams { open_doors: vec![0xD00], ..Default::default() };
    // Both in the world: the locked one closes the way, and the search says how much it
    // searched before giving up (the failed leg's triangles were not counted: "0
    // triangles searched" read as no door at all).
    match g.find_path(from, to, &spawn_open) {
        Err(PathError::Unreachable { expanded }) => assert!(expanded > 0, "{expanded}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(g.doors_closing(from, to, &spawn_open), Some(vec![0xD01]), "the door to blame");
    // Disabled (as placed, or by a script): no barrier, and no stop in front of it.
    let changes = g.sync_door_presence([(0xD01, false)]);
    assert_eq!(changes.gone, vec![0xD01]);
    let path = g.find_path(from, to, &spawn_open).unwrap();
    assert_eq!(path.legs[0].door_stops.iter().map(|s| s.door).collect::<Vec<_>>(), vec![0xD00]);
    assert_eq!(g.doors_closing(from, to, &spawn_open), Some(vec![]));
    // Enabled again: closed again.
    assert_eq!(g.sync_door_presence([(0xD01, true)]).back, vec![0xD01]);
    assert!(g.find_path(from, to, &spawn_open).is_err());
    // A way no door would open: nothing to blame.
    let island = graph(&[(grid(1, [0.0, 0.0], 2, 1, 100.0, &[], flat), W), (grid(2, [1000.0, 0.0], 2, 1, 100.0, &[], flat), W)]);
    assert_eq!(island.doors_closing(from, Place { space: W, pos: [1050.0, 50.0, 0.0] }, &PathParams::default()), None);
}

#[test]
fn locked_door_overlap_allows_only_endpoints_outside_its_footprint() {
    // [design, #2716] Synthetic rotated door: model y is game -x. The goal's
    // triangle reaches x=500, but the goal at x=450 is outside the footprint.
    let g = corridor_with_door(Some(crate::graph::DoorLock { level: 255, key: None }));
    let from = Place { space: W, pos: [350.0, 75.0, 0.0] };
    let beside = Place { space: W, pos: [450.0, 75.0, 0.0] };
    let t = g.locate(W, beside.pos, 1.0).unwrap();
    let p = PathParams::default();
    assert!(!g.door_tri_open(t, &p));
    for (a, b) in [(from, beside), (beside, from)] {
        let path = g.find_path(a, b, &p).unwrap();
        assert!(path.legs[0].door_stops.is_empty());
        assert!(path.legs[0].points.iter().all(|q| q[0] < 496.0));
    }
    // [design, #2716] Neither an endpoint inside/on the bounds nor a passing
    // triangle gets the exception. A same-triangle goal inside is refused too.
    for x in [496.0, 500.0, 504.0] {
        let inside = Place { space: W, pos: [x, 75.0, 0.0] };
        assert!(g.find_path(from, inside, &p).is_err());
        assert!(g.find_path(inside, from, &p).is_err());
        assert!(g.find_path(inside, inside, &p).is_err());
    }
    let across = Place { space: W, pos: [650.0, 75.0, 0.0] };
    assert!(g.find_path(from, across, &p).is_err());
    assert!(g.find_path(beside, across, &p).is_err());
    assert!(g.walkable_line(W, g.locate(W, from.pos, 1.0).unwrap(), from.pos, across.pos, 0.0, &p).is_none());
}

#[test]
fn endpoint_exemptions_cannot_cross_a_refusing_door_footprint() {
    // [design, #2716] Adjacent triangles on opposite sides are both exempt, but
    // their generated segment crosses the synthetic rotated door's footprint.
    let mut g = corridor_with_door(Some(crate::graph::DoorLock { level: 50, key: Some(0x4B) }));
    for (door_x, from, to, same_triangle) in [
        (500.0, [450.0, 25.0, 0.0], [550.0, 75.0, 0.0], false),
        (450.0, [430.0, 10.0, 0.0], [475.0, 10.0, 0.0], true),
    ] {
        g.set_in_place_doors(vec![hinged(0xD00, door_x, 0.0)], |_| Some(crate::graph::DoorLock { level: 50, key: Some(0x4B) }));
        let (s, t) = (g.locate(W, from, 1.0).unwrap(), g.locate(W, to, 1.0).unwrap());
        assert_eq!(s == t, same_triangle);
        let closed = PathParams::default();
        assert!(g.door_endpoint_open(s, from, &closed));
        assert!(g.door_endpoint_open(t, to, &closed));
        for (a, b) in [(from, to), (to, from)] {
            let (a, b) = (Place { space: W, pos: a }, Place { space: W, pos: b });
            assert!(matches!(g.find_path(a, b, &closed), Err(PathError::Unreachable { .. })));
            let refused = PathParams { keys: vec![0x4B], refused_doors: vec![0xD00], ..Default::default() };
            assert!(matches!(g.find_path(a, b, &refused), Err(PathError::Unreachable { .. })));
            assert!(g.find_path(a, b, &PathParams { keys: vec![0x4B], ..Default::default() }).is_ok());
            assert!(g.find_path(a, b, &PathParams { open_doors: vec![0xD00], ..refused }).is_ok());
        }
    }
}

#[test]
fn endpoint_must_be_outside_every_refusing_door_footprint() {
    // [design, #2716] The point clears the first rotated door but lies in the second.
    let mut g = corridor_with_door(None);
    g.set_in_place_doors(vec![hinged(0xD00, 500.0, 0.0), hinged(0xD01, 450.0, 0.0)], |_| Some(crate::graph::DoorLock { level: 255, key: None }));
    let from = Place { space: W, pos: [350.0, 75.0, 0.0] };
    let to = Place { space: W, pos: [450.0, 75.0, 0.0] };
    let t = g.locate(W, to.pos, 1.0).unwrap();
    assert_eq!(g.door_tris[&t].len(), 2);
    assert!(g.find_path(from, to, &PathParams::default()).is_err());
    assert!(g.find_path(from, to, &PathParams { open_doors: vec![0xD01], ..Default::default() }).is_ok());
}

#[test]
fn open_locked_in_place_door_does_not_refuse_paths_or_raycasts() {
    // [hypothesis, #2716] An open locked door is no barrier (observer check 206).
    let g = corridor_with_door(Some(crate::graph::DoorLock { level: 255, key: None }));
    let from = Place { space: W, pos: [350.0, 50.0, 0.0] };
    let to = Place { space: W, pos: [650.0, 50.0, 0.0] };
    let p = PathParams { open_doors: vec![0xD00], refused_doors: vec![0xD00], opens_doors: false, ..Default::default() };
    assert!(g.find_path(from, to, &p).is_ok());
    let t = g.locate(W, from.pos, 1.0).unwrap();
    assert!(g.walkable_line(W, t, from.pos, to.pos, 0.0, &p).is_some());
    let closed = PathParams { open_doors: Vec::new(), ..p };
    assert!(g.find_path(from, to, &closed).is_err());
}

#[test]
fn a_path_beside_a_door_does_not_stop() {
    // Three lanes; the door closes only the southern one, at x = 500.
    let mut g = graph(&[(grid(1, [0.0, 0.0], 10, 3, 100.0, &[], flat), W)]);
    g.set_in_place_doors(vec![hinged(0xD00, 500.0, 0.0)], |_| None);
    let north = |x: f32| Place { space: W, pos: [x, 250.0, 0.0] };
    let south = |x: f32| Place { space: W, pos: [x, 50.0, 0.0] };
    let p = g.find_path(north(50.0), north(950.0), &PathParams::default()).unwrap();
    assert!(p.legs[0].door_stops.is_empty());
    // Through it, it stops.
    let p = g.find_path(south(50.0), south(950.0), &PathParams::default()).unwrap();
    assert_eq!(p.legs[0].door_stops.len(), 1);
    // Locked, the path goes round it through the open lanes.
    g.set_door_lock(0xD00, Some(crate::graph::DoorLock { level: 1, key: None }));
    let p = g.find_path(south(50.0), south(950.0), &PathParams::default()).unwrap();
    assert!(p.legs[0].door_stops.is_empty());
    assert!(p.legs[0].points.iter().any(|q| q[1] > 100.0), "went round: {:?}", p.legs[0].points);
}

#[test]
fn an_actor_starting_at_the_door_backs_off_to_open_it() {
    let g = corridor_with_door(None);
    // Starting 20 units in front of the door: it steps back to the stop, clear of the
    // leaf, before opening it.
    let from = Place { space: W, pos: [476.0, 50.0, 0.0] };
    let p = g.find_path(from, Place { space: W, pos: [950.0, 50.0, 0.0] }, &PathParams::default()).unwrap();
    let leg = &p.legs[0];
    let stop = leg.points[leg.door_stops[0].point];
    assert!((stop[0] - 374.0).abs() <= 2.5, "{stop:?}");
    let mut f = crate::Follower::new(p);
    match f.steer(from.pos) {
        crate::Steer::Walk { target, .. } => assert_eq!(target, stop),
        s => panic!("{s:?}"),
    }
    assert!(matches!(f.steer([380.0, 50.0, 0.0]), crate::Steer::OpenDoor { door: 0xD00, .. }));
    f.door_opened();
    assert!(matches!(f.steer([380.0, 50.0, 0.0]), crate::Steer::Walk { .. }));
}

#[test]
fn a_path_planned_again_within_the_approach_to_an_open_door_goes_straight_on() {
    let g = corridor_with_door(None);
    // 76 units before the door, past the stop at 374 (a follower whose path is planned
    // again on its way through).
    let from = Place { space: W, pos: [420.0, 50.0, 0.0] };
    let to = Place { space: W, pos: [950.0, 50.0, 0.0] };
    // Closed (or not known to be open): back to the stop, clear of the leaf.
    let p = g.find_path(from, to, &PathParams::default()).unwrap();
    let leg = &p.legs[0];
    assert!((leg.points[leg.door_stops[0].point][0] - 374.0).abs() <= 2.5);
    // Open: the stop is where it is, and on through the opening.
    let p = g.find_path(from, to, &PathParams { open_doors: vec![0xD00], ..PathParams::default() }).unwrap();
    let leg = &p.legs[0];
    assert_eq!(leg.door_stops[0].point, 0);
    assert!(leg.points.iter().all(|q| q[0] >= 420.0 - 0.5), "{:?}", leg.points);
    let mut f = crate::Follower::new(p);
    assert!(matches!(f.steer(from.pos), crate::Steer::OpenDoor { door: 0xD00, .. }));
    f.door_opened();
    match f.steer(from.pos) {
        crate::Steer::Walk { target, .. } => assert_eq!(target, [552.0, 50.0, 0.0]),
        s => panic!("{s:?}"),
    }
}

#[test]
fn a_diagonal_approach_is_squared_up_to_the_middle_of_the_door() {
    // A room 500 x 400 south of a wall at y = 400 with a doorway 100 wide in it
    // (x 200..300), a room north of it; the door (100 wide, hinged at an end) closes
    // the doorway. The path comes from the room's far corner, diagonally.
    let holes: Vec<(usize, usize)> = (0..10).filter(|&i| !(4..6).contains(&i)).map(|i| (i, 8)).collect();
    let mut g = graph(&[(grid(1, [0.0, 0.0], 10, 17, 50.0, &holes, flat), W)]);
    let door = crate::doors::InPlaceDoor {
        door: 0xD01,
        space: W,
        position: [200.0, 425.0, 0.0],
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        scale: 1.0,
        bounds: ([0.0, -4.0, 0.0], [100.0, 4.0, 200.0]),
    };
    g.set_in_place_doors(vec![door], |_| None);
    let p = g.find_path(Place { space: W, pos: [480.0, 20.0, 0.0] }, Place { space: W, pos: [20.0, 830.0, 0.0] }, &PathParams::default()).unwrap();
    let leg = &p.legs[0];
    assert_eq!(leg.door_stops.len(), 1);
    let i = leg.door_stops[0].point;
    let (stop, through) = (leg.points[i], leg.points[i + 1]);
    // In line with the middle of the doorway (x = 250), in front of it and beyond it.
    assert!((stop[0] - 250.0).abs() < 1.0 && stop[1] < 425.0 - 100.0, "{stop:?}");
    assert!((through[0] - 250.0).abs() < 1.0 && through[1] > 429.0, "{through:?}");
    assert!(on_mesh(&g, W, &leg.points, 5.0));
}

#[test]
fn cover_edges_are_the_outline_of_listed_triangles_facing_out() {
    // A 4 × 4 grid of 100-unit squares with a hole (a rock) at square (2, 2); the four
    // triangles round the hole are listed as cover.
    let mut m = grid(0x10, [0.0, 0.0], 4, 4, 100.0, &[(2, 2)], flat);
    let listed: Vec<u16> = (0..m.triangles.len() as u16)
        .filter(|&t| {
            let c = m.triangles[t as usize].v.map(|v| m.vertices[v as usize]);
            let (x, y) = ((c[0][0] + c[1][0] + c[2][0]) / 3.0, (c[0][1] + c[1][1] + c[2][1]) / 3.0);
            (150.0..=350.0).contains(&x) && (150.0..=350.0).contains(&y) && !((200.0..300.0).contains(&x) && (200.0..300.0).contains(&y))
        })
        .collect();
    m.cover_triangles = listed;
    let g = graph(&[(m, W)]);
    let edges = g.cover_edges_near(W, [250.0, 100.0, 0.0], 400.0);
    // Only the hole's four sides lie inside listed triangles (the grid's outer edges are
    // not listed).
    assert_eq!(edges.len(), 4, "{edges:?}");
    // The south side of the rock: its middle at (250, 200), out of the navmesh is north.
    let south = edges[0];
    assert!((south.mid[0] - 250.0).abs() < 1e-3 && (south.mid[1] - 200.0).abs() < 1e-3);
    assert!((south.out[1] - 1.0).abs() < 1e-3 && south.out[0].abs() < 1e-3);
    // A threat north of the rock is covered from there; one south of it is not.
    assert!(south.faces([250.0, 380.0, 0.0]) && !south.faces([250.0, 20.0, 0.0]));
    assert_eq!(south.behind(40.0), [250.0, 160.0, 0.0]);
    assert!((south.length() - 100.0).abs() < 1e-3);
}

/// A floor, a ramp up from its east side to a landing, and a deck turning back west
/// from the landing over the floor, 200 units above it (a catwalk over a factory floor:
/// RocketLabMid's flights and catwalks, docs/spec-notes/navmesh.md §4b).
fn floor_with_deck_above() -> NavGraph {
    let mut floor = grid(1, [0.0, 0.0], 4, 4, 100.0, &[], flat);
    let mut ramp = grid(2, [400.0, 0.0], 4, 1, 100.0, &[], |x, _| (x - 400.0) * 0.5);
    let mut landing = grid(3, [800.0, 0.0], 1, 4, 100.0, &[], |_, _| 200.0);
    let mut deck = grid(4, [0.0, 300.0], 8, 1, 100.0, &[], |_, _| 200.0);
    seam(&mut floor, &mut ramp, link_kind::PORTAL);
    seam(&mut ramp, &mut landing, link_kind::PORTAL);
    seam(&mut deck, &mut landing, link_kind::PORTAL);
    graph(&[(floor, W), (ramp, W), (landing, W), (deck, W)])
}

// [design, #2532] Synthetic ramp route with a shorter authored drop at the deck's west end.
fn ramp_with_drop() -> NavGraph {
    let mut floor = grid(1, [0.0, 0.0], 4, 4, 100.0, &[], flat);
    let mut ramp = grid(2, [400.0, 0.0], 4, 1, 100.0, &[], |x, _| (x - 400.0) * 0.5);
    let mut landing = grid(3, [800.0, 0.0], 1, 4, 100.0, &[], |_, _| 200.0);
    let mut deck = grid(4, [0.0, 300.0], 8, 1, 100.0, &[], |_, _| 200.0);
    seam(&mut floor, &mut ramp, link_kind::PORTAL);
    seam(&mut ramp, &mut landing, link_kind::PORTAL);
    seam(&mut deck, &mut landing, link_kind::PORTAL);
    let (dt, de, ..) = edges_at(&deck, 0.0)[0];
    let ft = edges_at(&floor, 0.0).into_iter().find(|(_, _, a, b)| a[1].min(b[1]) == 300.0).unwrap().0;
    link(&mut deck, dt, de, floor.form_id, ft, link_kind::LEDGE_DOWN);
    graph(&[(floor, W), (ramp, W), (landing, W), (deck, W)])
}

#[test]
fn safe_drops_preserve_the_ordinary_ramp_and_its_smoothed_path() {
    let g = ramp_with_drop();
    let from = Place { space: W, pos: [50.0, 350.0, 200.0] };
    let to = Place { space: W, pos: [50.0, 350.0, 0.0] };
    let ordinary = g.find_path(from, to, &PathParams::default()).unwrap();
    assert!(ordinary.legs[0].points.iter().any(|p| p[0] > 700.0));
    assert!(ordinary.legs[0].hops.is_empty());
    let safe = PathParams { safe_drop_max: Some(200.0), ..Default::default() };
    assert_eq!(g.find_path(from, to, &safe).unwrap(), ordinary);
    let shortcut = g.find_path(from, to, &PathParams { ledges: true, ..safe.clone() }).unwrap();
    assert_eq!(shortcut.legs[0].hops.len(), 1);
    assert!(!shortcut.legs[0].hops[0].up);
    assert!(shortcut.length < ordinary.length);
    let limited = PathParams { max_expanded: 1, ..safe };
    assert!(matches!(g.find_path(from, to, &limited), Err(PathError::TooFar { .. })));
}

#[test]
fn safe_drops_retry_when_the_ordinary_same_area_route_is_blocked() {
    let mut g = ramp_with_drop();
    // [design, #2532] Area labels still connect both levels, but this actor cannot use the ramp.
    let ramp = g.by_form[&2];
    for t in g.tris.iter_mut().filter(|t| t.mesh == ramp) {
        t.flags |= tri_flags::WATER;
    }
    let from = Place { space: W, pos: [50.0, 350.0, 200.0] };
    let to = Place { space: W, pos: [50.0, 350.0, 0.0] };
    let ordinary = PathParams { water: false, ..Default::default() };
    assert!(g.door_route(from, to, &ordinary).unwrap().is_empty());
    assert!(matches!(g.find_path(from, to, &ordinary), Err(PathError::Unreachable { .. })));
    let safe = PathParams { safe_drop_max: Some(200.0), ..ordinary };
    let path = g.find_path(from, to, &safe).unwrap();
    assert_eq!(path.legs[0].hops.len(), 1);
    assert!(!path.legs[0].hops[0].up);
}

#[test]
fn a_straight_line_does_not_reach_a_level_stacked_over_the_floor() {
    let g = floor_with_deck_above();
    let p = PathParams::default();
    let (low, high) = ([150.0, 350.0, 0.0], [150.0, 350.0, 200.0]);
    let floor = g.locate(W, low, 10.0).expect("floor triangle");
    let deck = g.locate(W, high, 10.0).expect("deck triangle");
    assert_ne!(floor, deck);
    // Seen from above the deck's spot is on the floor triangle; it is not walkable from
    // there all the same.
    assert_eq!(g.walkable_line(W, floor, low, high, 18.0, &p), None);
    assert_eq!(g.walkable_line(W, floor, [50.0, 50.0, 0.0], high, 18.0, &p), None);
    // On one level the line is walkable.
    assert_eq!(g.walkable_line(W, floor, [50.0, 50.0, 0.0], low, 18.0, &p), Some(floor));
    assert_eq!(g.walkable_line(W, deck, [650.0, 350.0, 200.0], high, 18.0, &p), Some(deck));
}

#[test]
fn a_path_up_to_a_level_stacked_over_the_floor_takes_the_ramp() {
    // RocketLabMid (owner report 2026-10-23): the path from the floor to the top
    // catwalk was pulled straight from the floor under the catwalk up to it.
    let g = floor_with_deck_above();
    for (from, to) in [([150.0, 350.0, 0.0], [150.0, 350.0, 200.0]), ([150.0, 350.0, 200.0], [150.0, 350.0, 0.0]), ([50.0, 50.0, 0.0], [100.0, 350.0, 200.0])] {
        let path = g.find_path(Place { space: W, pos: from }, Place { space: W, pos: to }, &PathParams::default()).unwrap();
        let pts = &path.legs[0].points;
        // Up the ramp (x 400-800) and back along the deck: well over 1000 units.
        assert!(path.length > 1000.0, "{pts:?}");
        assert!(pts.iter().any(|q| q[0] > 700.0), "{pts:?}");
        // No segment rises or falls more steeply than the ramp (1 in 2).
        for w in pts.windows(2) {
            let h = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            assert!((w[1][2] - w[0][2]).abs() <= 0.5 * h + 1.0, "{w:?} in {pts:?}");
        }
    }
}

/// A broken mod's navmesh with triangles but no vertices (last in the graph, so its
/// triangles' fallback index is past the end) is assembled with them left out.
#[test]
fn a_navmesh_with_triangles_but_no_vertices_is_skipped() {
    let good = grid(0x10, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let bad = NavMeshRecord {
        form_id: 0x11,
        triangles: vec![Triangle { v: [0, 1, 2], edges: [0, -1, 5], flags: 0, cover: 0 }; 2],
        ..Default::default()
    };
    let g = graph(&[(good, W), (bad, W)]);
    let first_bad = g.meshes[1].first_tri;
    assert!(g.tris[first_bad as usize].has(tri_flags::DELETED));
    assert!(!g.usable(first_bad));
    assert_eq!(g.corners(first_bad), [[0.0; 3]; 3]);
}

/// Sandbags across the corridor at x = 500 (as [`hinged`]'s door: 100 wide, 8 thick),
/// `height` tall from z = `base`, initially disabled.
fn sandbags(base: f32, height: f32) -> crate::obstacles::Obstacle {
    let d = hinged(0x5B0, 500.0, 0.0);
    crate::obstacles::Obstacle { reference: 0x5B0, space: W, position: [d.position[0], d.position[1], base], rotation: d.rotation, scale: 1.0, bounds: ([-50.0, -4.0, 0.0], [50.0, 4.0, height]), initially_enabled: false }
}

/// An obstacle that comes and goes (ESM full audit gap 22, `crate::obstacles`): while
/// enabled it cuts the triangles under it and the corridor is shut; a path planned
/// across it before is blocked; disabled again, the way is open.
#[test]
fn an_enabled_obstacle_cuts_the_navmesh_under_it() {
    let mut g = graph(&[(grid(1, [0.0, 0.0], 10, 1, 100.0, &[], flat), W)]);
    g.set_obstacles(vec![sandbags(0.0, 90.0)], |o| o.initially_enabled);
    assert_eq!(g.obstacles.len(), 1);
    assert_eq!(g.cut_triangles(), 0, "disabled: nothing cut");
    let (from, to) = (Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: W, pos: [950.0, 50.0, 0.0] });
    assert!(g.can_reach(from, to, &PathParams::default()));
    let mut w = Walker::new(W, from.pos, 0.0, 100.0);
    w.go_to(&g, to, &PathParams::default()).unwrap();
    assert_eq!(g.sync_obstacles([(0x5B0, true), (0x999, true)]), vec![0x5B0]);
    assert!(g.sync_obstacles([(0x5B0, true)]).is_empty(), "applied once");
    // The triangles touching x = 496..504: the squares on both sides of the line.
    assert!(g.cut_triangles() >= 2);
    assert!(w.follower.as_ref().unwrap().blocked(&g));
    assert!(!g.can_reach(from, to, &PathParams::default()));
    assert_eq!(g.sync_obstacles([(0x5B0, false)]), vec![0x5B0]);
    assert_eq!(g.cut_triangles(), 0);
    assert!(g.can_reach(from, to, &PathParams::default()));
}

/// The obstacle stamp changes with every change of the obstacles or their states and
/// only then; no two graphs share one once they learnt obstacles (`fnv_ai`'s obstacle
/// poll is keyed on it).
#[test]
fn the_obstacle_stamp_changes_with_the_obstacles_only() {
    let mut g = graph(&[(grid(1, [0.0, 0.0], 10, 1, 100.0, &[], flat), W)]);
    assert_eq!(g.obstacles_stamp(), 0, "none learnt");
    g.set_obstacles(vec![sandbags(0.0, 90.0)], |o| o.initially_enabled);
    let learnt = g.obstacles_stamp();
    assert_ne!(learnt, 0);
    assert!(g.sync_obstacles([(0x5B0, false), (0x999, true)]).is_empty());
    assert_eq!(g.obstacles_stamp(), learnt, "nothing changed");
    assert_eq!(g.sync_obstacles([(0x5B0, true)]), vec![0x5B0]);
    let switched = g.obstacles_stamp();
    assert_ne!(switched, learnt);
    assert_eq!(g.sync_obstacles([(0x5B0, false)]), vec![0x5B0]);
    assert!(g.obstacles_stamp() != switched && g.obstacles_stamp() != learnt, "back to the first states, still a new stamp");
    let mut other = graph(&[(grid(1, [0.0, 0.0], 10, 1, 100.0, &[], flat), W)]);
    other.set_obstacles(vec![sandbags(0.0, 90.0)], |o| o.initially_enabled);
    assert_ne!(other.obstacles_stamp(), g.obstacles_stamp());
    g.set_obstacles(vec![sandbags(0.0, 90.0)], |o| o.initially_enabled);
    assert_ne!(g.obstacles_stamp(), other.obstacles_stamp(), "learnt again");
}

/// An obstacle placed enabled (with an enable parent, not Initially Disabled) stood there
/// when the navmesh was made: it never cuts, enabled or not (the Hoover Dam sandbag on
/// Colonel Moore's way to Kimball's podium, `crate::obstacles`).
#[test]
fn an_obstacle_placed_enabled_does_not_cut() {
    let mut g = graph(&[(grid(1, [0.0, 0.0], 10, 1, 100.0, &[], flat), W)]);
    let placed = crate::obstacles::Obstacle { initially_enabled: true, ..sandbags(0.0, 90.0) };
    g.set_obstacles(vec![placed], |o| o.initially_enabled);
    assert_eq!(g.obstacles.len(), 1, "still learnt");
    assert_eq!(g.cut_triangles(), 0);
    let (from, to) = (Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: W, pos: [950.0, 50.0, 0.0] });
    assert!(g.can_reach(from, to, &PathParams::default()));
    assert!(g.sync_obstacles([(0x5B0, false)]).len() == 1 && g.cut_triangles() == 0);
    assert!(g.sync_obstacles([(0x5B0, true)]).len() == 1 && g.cut_triangles() == 0);
    assert!(g.can_reach(from, to, &PathParams::default()));
}

/// An obstacle wider than an exterior cell (the bounds of MQ05's landing vertibirds hold
/// their whole flight) cuts nothing when enabled; one just under the limit still does
/// (issue #2846, `crate::obstacles::MAX_SIDE`).
#[test]
fn an_obstacle_wider_than_a_cell_does_not_cut() {
    let side = crate::obstacles::MAX_SIDE;
    let wide = crate::obstacles::Obstacle { bounds: ([-side / 2.0 - 1.0, -4.0, 0.0], [side / 2.0 + 1.0, 4.0, 90.0]), ..sandbags(0.0, 90.0) };
    assert!(wide.too_wide());
    let scaled = crate::obstacles::Obstacle { bounds: ([-side * 0.499, -4.0, 0.0], [side * 0.499, 4.0, 90.0]), scale: 1.01, ..sandbags(0.0, 90.0) };
    assert!(scaled.too_wide(), "the placed scale counts");
    let under = crate::obstacles::Obstacle { bounds: ([-side / 2.0 + 1.0, -4.0, 0.0], [side / 2.0 - 1.0, 4.0, 90.0]), ..sandbags(0.0, 90.0) };
    assert!(!under.too_wide());
    let (from, to) = (Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: W, pos: [950.0, 50.0, 0.0] });
    let mut g = graph(&[(grid(1, [0.0, 0.0], 10, 1, 100.0, &[], flat), W)]);
    g.set_obstacles(vec![wide], |_| true);
    assert_eq!(g.cut_triangles(), 0);
    assert!(g.can_reach(from, to, &PathParams::default()));
    g.set_obstacles(vec![under], |_| true);
    assert!(g.cut_triangles() > 0);
    assert!(!g.can_reach(from, to, &PathParams::default()));
}

/// The floor near an obstacle's top is on it, not under it, and a floor far below its
/// base is another level: neither is cut.
#[test]
fn an_obstacle_cuts_only_floor_within_its_height() {
    let floor = [[490.0, 0.0, 0.0], [510.0, 0.0, 0.0], [500.0, 100.0, 0.0]];
    assert!(sandbags(0.0, 90.0).cuts(floor));
    // Sunk until its top is 10 above the floor.
    assert!(!sandbags(-80.0, 90.0).cuts(floor));
    // Standing on a deck 200 above the floor.
    assert!(!sandbags(200.0, 90.0).cuts(floor));
    // Beside the floor, seen from above.
    assert!(!sandbags(0.0, 90.0).cuts([[600.0, 0.0, 0.0], [700.0, 0.0, 0.0], [650.0, 100.0, 0.0]]));
}

#[test]
fn places_to_avoid_cost_more_and_are_gone_round() {
    let g = graph(&[(grid(1, [0.0, 0.0], 10, 10, 100.0, &[], flat), W)]);
    let (from, to) = (Place { space: W, pos: [150.0, 150.0, 0.0] }, Place { space: W, pos: [850.0, 150.0, 0.0] });
    // Radiated ground round (500, 150), radius 220: straight through without the cost,
    // round its north edge with it.
    let straight = g.find_path(from, to, &PathParams::default()).unwrap();
    assert!(straight.length < 720.0, "{}", straight.length);
    let p = PathParams { avoid: vec![([500.0, 150.0, 0.0], 220.0)], avoid_cost: 8.0, ..Default::default() };
    let round = g.find_path(from, to, &p).unwrap();
    assert!(round.length > 760.0, "{}", round.length);
    assert!(round.legs[0].points.iter().any(|q| q[1] > 300.0), "{:?}", round.legs[0].points);
}

#[test]
fn water_triangles_close_to_actors_that_do_not_swim() {
    // A strip of water across x = 400..500 from y = 0 to 800; land round it at the north.
    let mut m = grid(1, [0.0, 0.0], 10, 10, 100.0, &[], flat);
    for i in 0..m.triangles.len() {
        let t = &m.triangles[i];
        let c = t.v.iter().fold([0.0f32; 2], |c, &v| [c[0] + m.vertices[v as usize][0] / 3.0, c[1] + m.vertices[v as usize][1] / 3.0]);
        if (400.0..500.0).contains(&c[0]) && c[1] < 800.0 {
            m.triangles[i].flags |= tri_flags::WATER;
        }
    }
    let g = graph(&[(m, W)]);
    let (from, to) = (Place { space: W, pos: [150.0, 150.0, 0.0] }, Place { space: W, pos: [850.0, 150.0, 0.0] });
    let wet = |p: &crate::path::Path| p.legs[0].corridor.iter().filter(|&&t| g.tris[t as usize].has(tri_flags::WATER)).count();
    // A swimmer crosses the strip (three times the cost of land, still shorter).
    let swim = g.find_path(from, to, &PathParams::default()).unwrap();
    assert!(wet(&swim) > 0 && swim.length < 900.0, "{} {:?}", swim.length, swim.legs[0].points);
    // One that does not swim goes round it by land.
    let dry = g.find_path(from, to, &PathParams { water: false, ..Default::default() }).unwrap();
    assert_eq!(wet(&dry), 0, "{:?}", dry.legs[0].points);
    assert!(dry.length > 1400.0, "{}", dry.length);
    // A goal in the water is out of its reach at once; standing in it, it walks out.
    let pond = Place { space: W, pos: [450.0, 150.0, 0.0] };
    let no_swim = PathParams { water: false, ..Default::default() };
    assert!(matches!(g.find_path(from, pond, &no_swim), Err(PathError::Unreachable { expanded: 0 })));
    assert!(g.find_path(pond, to, &no_swim).is_ok());
}

// Broken data (a damaged or badly made plugin): the graph builds, queries answer, and
// nothing panics or runs on for ever.

/// `grid` plus triangles whose corners are not finite, lie far out, or spread over many
/// cells (each its own three vertices, no neighbours).
fn with_bad_corners() -> NavMeshRecord {
    let mut m = grid(1, [0.0, 0.0], 4, 4, 100.0, &[], flat);
    let bad: [[[f32; 3]; 3]; 4] = [
        [[0.0, 0.0, 0.0], [f32::INFINITY, 0.0, 0.0], [0.0, 100.0, 0.0]],
        [[0.0, 0.0, 0.0], [100.0, 0.0, f32::NAN], [0.0, 100.0, 0.0]],
        [[1.0e30, 0.0, 0.0], [1.0e30, 100.0, 0.0], [-1.0e30, 0.0, 0.0]],
        [[0.0, 0.0, 0.0], [1.0e6, 0.0, 0.0], [0.0, 1.0e6, 0.0]],
    ];
    for c in bad {
        let first = m.vertices.len() as u16;
        m.vertices.extend(c);
        m.triangles.push(Triangle { v: [first, first + 1, first + 2], edges: [-1; 3], flags: 0, cover: 0 });
    }
    m
}

#[test]
fn triangles_with_broken_corners_are_left_out() {
    let g = graph(&[(with_bad_corners(), W)]);
    assert_eq!(g.stats.bad_triangles, 4);
    for t in 32..36 {
        assert!(g.tris[t].has(tri_flags::DELETED) && !g.usable(t as u32));
    }
    // The good part still works.
    let p = g.find_path(Place { space: W, pos: [50.0, 50.0, 0.0] }, Place { space: W, pos: [350.0, 350.0, 0.0] }, &PathParams::default()).unwrap();
    assert!(on_mesh(&g, W, &p.legs[0].points, 10.0));
}

#[test]
fn indices_past_the_end_are_ignored() {
    let mut m = grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let n = m.triangles.len();
    m.triangles[0].v[0] = 999; // vertex past the end
    m.triangles[1].edges[0] = 999; // neighbour past the end
    m.triangles[2].edges[1] = -7; // negative neighbour
    // Linked edges: NVEX index past the list, a navmesh not loaded, a triangle past the end.
    m.triangles[3].flags |= 1 << 1;
    m.triangles[3].edges[1] = 50;
    link(&mut m, 4, 1, 0xDEAD, 0, link_kind::PORTAL);
    link(&mut m, 5, 1, 1, 60_000, link_kind::LEDGE_UP);
    m.cover_triangles.extend([0, 65_535]);
    m.door_links.push(DoorLink { door: 0xD1, triangle: 65_535 });
    // A navmesh with triangles and no vertices at all.
    let mut empty = NavMeshRecord { form_id: 2, ..Default::default() };
    empty.triangles.push(Triangle { v: [0, 1, 2], edges: [0, 1, 2], flags: 0b111, cover: 0 });
    let meshes = [(m, W), (empty, W)];
    let input: Vec<_> = meshes.iter().map(|(m, s)| (m, Some(0), Some(*s))).collect();
    let mut g = NavGraph::assemble(&input);
    g.join_doors(&input, |_| Some((0xD2, [f32::NAN, 0.0, 0.0], 0.0, W)));
    assert_eq!(g.tris.len(), n + 1);
    assert!(g.stats.unresolved_links >= 3);
    assert_eq!(g.stats.doors, 0);
    assert!(!g.usable(0) && !g.usable(n as u32));
    for t in 0..g.tris.len() as u32 {
        assert!(g.tris[t as usize].next.iter().all(|&x| x == NONE || (x as usize) < g.tris.len()));
    }
    // Most of the grid is broken: a path or a clean error, either is an answer.
    for (a, b) in [([150.0, 150.0, 0.0], [50.0, 150.0, 0.0]), ([50.0, 50.0, 0.0], [150.0, 150.0, 0.0])] {
        let _ = g.find_path(Place { space: W, pos: a }, Place { space: W, pos: b }, &PathParams { ledges: true, ..Default::default() });
        let _ = g.find_path(Place { space: W, pos: a }, Place { space: W, pos: b }, &PathParams::default());
    }
}

#[test]
fn queries_with_broken_numbers_answer_at_once() {
    let g = graph(&[(with_bad_corners(), W)]);
    let ok = [100.0, 100.0, 0.0];
    for p in [[f32::NAN, 0.0, 0.0], [f32::INFINITY, 0.0, 0.0], [0.0, f32::NEG_INFINITY, 0.0], [0.0, 0.0, f32::NAN]] {
        assert_eq!(g.nearest(W, p, 512.0), None, "{p:?}");
        assert!(g.triangles_near(W, p[0], p[1], 512.0).is_empty() || p[0].is_finite() && p[1].is_finite());
        let e = g.find_path(Place { space: W, pos: ok }, Place { space: W, pos: p }, &PathParams::default());
        assert!(matches!(e, Err(PathError::NoTriangle { .. })), "{p:?}: {e:?}");
    }
    // Huge, infinite and NaN radii.
    for r in [f32::MAX, f32::INFINITY, f32::NAN, -5.0] {
        let _ = g.nearest(W, [5000.0, 5000.0, 0.0], r);
        let _ = g.triangles_near(W, 100.0, 100.0, r);
        let _ = g.cover_edges_near(W, ok, r);
    }
    assert!(g.nearest(W, [5000.0, 5000.0, 0.0], f32::MAX).is_some());
    // Far-out but finite points (bucket indices at the ends of i32).
    for p in [[3.0e38, 3.0e38, 0.0], [-3.0e38, 0.0, 0.0]] {
        assert_eq!(g.nearest(W, p, f32::MAX), None);
        assert!(g.triangles_near(W, p[0], p[1], f32::MAX).is_empty());
    }
}

#[test]
fn obstacles_and_doors_with_broken_placement_do_not_hang() {
    let mut g = graph(&[(grid(1, [0.0, 0.0], 4, 4, 100.0, &[], flat), W)]);
    let o = |scale: f32, position: [f32; 3]| crate::obstacles::Obstacle {
        reference: 0x5B0,
        space: W,
        position,
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        scale,
        bounds: ([-50.0, -4.0, 0.0], [50.0, 4.0, 90.0]),
        initially_enabled: true,
    };
    for (s, p) in [(f32::INFINITY, [200.0, 200.0, 0.0]), (1.0e30, [200.0, 200.0, 0.0]), (f32::NAN, [200.0, 200.0, 0.0]), (1.0, [f32::NAN, 0.0, 0.0]), (1.0, [1.0e30, 1.0e30, 0.0])] {
        g.set_obstacles(vec![o(s, p)], |o| o.initially_enabled);
    }
}

// ---- a goal's height (#1500; docs/design/navmesh.md §4.1) --------------------------------

/// One leg through `points`, ending at the path's goal.
fn one_leg(points: Vec<[f32; 3]>) -> crate::Path {
    crate::Path { legs: vec![crate::Leg { space: W, points, corridor: vec![], door: None, door_stops: vec![], hops: vec![] }], length: 0.0, expanded: 0, start_snap: 0.0, goal_snap: 0.0 }
}

const DECK_GOAL: [f32; 3] = [150.0, 350.0, 200.0];

/// A walker on the deck of [`floor_with_deck_above`], on its way to [`DECK_GOAL`].
fn walker_on_the_deck(g: &NavGraph) -> Walker {
    let mut w = Walker::new(W, [650.0, 350.0, 200.0], -std::f32::consts::FRAC_PI_2, 200.0);
    w.go_to(g, Place { space: W, pos: DECK_GOAL }, &PathParams::default()).unwrap();
    w
}

#[test]
fn a_follower_under_or_over_the_end_of_its_path_has_not_arrived() {
    let path = one_leg(vec![[0.0, 0.0, 100.0], [100.0, 0.0, 100.0]]);
    assert_eq!(Follower::new(path.clone()).steer([100.0, 0.0, 100.0]), Steer::Arrived);
    assert_eq!(Follower::new(path.clone()).steer([100.0, 0.0, 0.0]), Steer::OffPath);
    assert_eq!(Follower::new(path).steer([100.0, 0.0, 200.0]), Steer::OffPath);
}

#[test]
fn a_real_path_with_the_feet_under_its_goal_has_not_arrived() {
    let g = floor_with_deck_above();
    let w = walker_on_the_deck(&g);
    let mut f = w.follower.clone().unwrap();
    // The path's own end is the place on the navmesh, on the deck.
    let end = f.destination().unwrap().pos;
    assert!((end[2] - 200.0).abs() < 1e-3, "{end:?}");
    assert!(g.locate(W, end, 1.0).is_some());
    assert_eq!(f.steer([DECK_GOAL[0], DECK_GOAL[1], 0.0]), Steer::OffPath);
    // Still not there when it says so again; there at the height.
    assert_eq!(f.steer([DECK_GOAL[0], DECK_GOAL[1], 0.0]), Steer::OffPath);
    assert_eq!(f.steer(DECK_GOAL), Steer::Arrived);
}

#[test]
fn the_height_a_goal_allows_grows_with_the_distance_from_it() {
    let goal = [0.0, 0.0, 0.0];
    assert!(goal_height_fits([0.0, 0.0, 40.0], goal));
    assert!(goal_height_fits([0.0, 0.0, -40.0], goal));
    assert!(!goal_height_fits([0.0, 0.0, 40.01], goal));
    assert!(!goal_height_fits([0.0, 0.0, -40.01], goal));
    // On a 60 degree slope at any distance, with the margin to spare.
    for d in [1.0f32, 16.0, 64.0] {
        let rise = d * 60f32.to_radians().tan();
        assert!(goal_height_fits([d, 0.0, rise + 39.9], goal), "{d}");
        assert!(!goal_height_fits([d, 0.0, rise + 40.1], goal), "{d}");
    }
    // Through a follower, at its radius of one unit.
    let path = one_leg(vec![[100.0, 0.0, 0.0], [0.0, 0.0, 0.0]]);
    assert_eq!(Follower::new(path.clone()).steer([0.5, 0.0, 40.8]), Steer::Arrived);
    assert_eq!(Follower::new(path).steer([0.5, 0.0, 40.9]), Steer::OffPath);
}

#[test]
fn a_walker_up_a_sixty_degree_slope_arrives_without_a_new_path() {
    let tan = 60f32.to_radians().tan();
    let g = graph(&[(grid(1, [0.0, 0.0], 6, 3, 100.0, &[], move |x, _| x * tan), W)]);
    let goal = [550.0, 150.0, 550.0 * tan];
    let mut w = Walker::new(W, [50.0, 150.0, 50.0 * tan], std::f32::consts::FRAC_PI_2, 300.0);
    w.go_to(&g, Place { space: W, pos: goal }, &PathParams::default()).unwrap();
    let mut arrived = false;
    for _ in 0..200 {
        let ev = w.update(&g, 0.1);
        arrived = ev == WalkEvent::Arrived;
        if arrived {
            break;
        }
    }
    assert!(arrived);
    assert_eq!((w.height_searches, w.height_waivers), (0, 0));
}

/// Corners, door stops, take-offs, load doors and asking a door stay horizontal: feet 500
/// under them do what feet at their height do.
#[test]
fn only_the_goal_asks_for_a_height() {
    let under = -500.0;
    // A corner reached, and one passed.
    let mut f = Follower::new(one_leg(vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 100.0, 0.0], [200.0, 100.0, 0.0]]));
    assert!(matches!(f.steer([95.0, 0.0, under]), Steer::Walk { target, .. } if target == [100.0, 100.0, 0.0]));
    assert!(matches!(f.steer([100.0, 130.0, under]), Steer::Walk { target, .. } if target == [200.0, 100.0, 0.0]));
    // A door stop, also held, and the door asked for from there.
    let mut stops = one_leg(vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [200.0, 0.0, 0.0]]);
    stops.legs[0].door_stops = vec![crate::DoorStop { door: 7, point: 1 }];
    let mut f = Follower::new(stops);
    assert_eq!(f.door_ahead([90.0, 0.0, under], 48.0), Some(7));
    assert_eq!(f.steer([95.0, 0.0, under]), Steer::OpenDoor { door: 7, at: [100.0, 0.0, 0.0] });
    assert_eq!(f.steer([50.0, 0.0, under]), Steer::OpenDoor { door: 7, at: [100.0, 0.0, 0.0] });
    // The take-off of a ledge link.
    let mut hop = one_leg(vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [150.0, 0.0, 100.0], [300.0, 0.0, 100.0]]);
    hop.legs[0].hops = vec![crate::LedgeHop { point: 1, up: true }];
    let mut f = Follower::new(hop);
    assert!(matches!(f.steer([99.0, 0.0, under]), Steer::Walk { target, .. } if target == [150.0, 0.0, 100.0]));
    assert!(f.hop().is_some());
    // A load door at the end of a leg, used from another height and asked for too.
    let door = crate::graph::DoorEdge { door: 9, to_door: 10, arrival: [0.0; 3], arrival_heading: 0.0, to: 0, door_at: None };
    let mut load = one_leg(vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]]);
    load.legs[0].door = Some(door);
    let mut f = Follower::new(load);
    assert_eq!(f.door_ahead([80.0, 0.0, under], 48.0), Some(9));
    assert_eq!(f.steer([80.0, 0.0, under]), Steer::Door(door));
}

#[test]
fn two_new_paths_then_the_height_is_waived() {
    let g = graph(&[(grid(1, [0.0, 0.0], 4, 4, 100.0, &[], flat), W)]);
    let goal = [150.0, 150.0, 0.0];
    let feet = [150.0, 150.0, 50.0];
    let mut w = Walker::new(W, feet, 0.0, 100.0);
    w.go_to(&g, Place { space: W, pos: goal }, &PathParams::default()).unwrap();
    let mut f = w.follower.take().unwrap();
    f.goal_radius = 8.0;
    assert_eq!(f.steer(feet), Steer::OffPath);
    let new = w.off_path(&g, feet, &f).expect("the first new path");
    assert_eq!(new.goal_radius, 8.0, "the caller's radius stays");
    assert!(w.off_path(&g, feet, &f).is_some(), "the second");
    assert_eq!(w.height_waivers, 0);
    assert!(w.off_path(&g, feet, &f).is_none(), "no third");
    assert_eq!((w.height_waivers, w.trip.as_ref().unwrap().waived), (1, true));
    // Told once.
    assert!(w.off_path(&g, feet, &f).is_none());
    assert_eq!(w.height_waivers, 1);
}

#[test]
fn a_new_path_starting_far_from_the_feet_or_none_at_all_waives_the_height() {
    let g = graph(&[(grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat), W), (grid(2, [1000.0, 0.0], 2, 2, 100.0, &[], flat), W)]);
    for feet in [[100.0, 100.0, 65.0], [1100.0, 100.0, 0.0]] {
        let mut w = Walker::new(W, [100.0, 100.0, 0.0], 0.0, 100.0);
        w.go_to(&g, Place { space: W, pos: [100.0, 100.0, 0.0] }, &PathParams::default()).unwrap();
        let f = w.follower.clone().unwrap();
        assert!(w.off_path(&g, feet, &f).is_none(), "{feet:?}");
        let t = w.trip.as_ref().unwrap();
        assert_eq!((t.replans, t.waived, w.height_waivers), (1, true, 1), "{feet:?}");
    }
}

#[test]
fn a_trip_is_the_place_asked_for() {
    let g = graph(&[(grid(1, [0.0, 0.0], 10, 10, 100.0, &[], flat), W)]);
    let p = PathParams::default();
    let feet = [150.0, 150.0, 50.0];
    let mut w = Walker::new(W, feet, 0.0, 100.0);
    let go = |w: &mut Walker, to: [f32; 3]| {
        w.go_to(&g, Place { space: W, pos: to }, &p).unwrap();
    };
    // Asked for 300 above its navmesh: the snapped end is not what a later ask is held to.
    go(&mut w, [150.0, 150.0, 300.0]);
    let f = w.follower.clone().unwrap();
    assert!(w.off_path(&g, feet, &f).is_some());
    go(&mut w, [150.0, 150.0, 300.0]);
    assert_eq!(w.trip.as_ref().unwrap().replans, 1, "the count stays");
    // Used up and waived; about the same place again keeps that.
    assert!(w.off_path(&g, feet, &f).is_some());
    assert!(w.off_path(&g, feet, &f).is_none());
    go(&mut w, [160.0, 150.0, 300.0]);
    let t = w.trip.as_ref().unwrap();
    assert_eq!((t.replans, t.waived), (2, true), "a new follower does not erase it");
    assert_eq!(t.to.pos, [160.0, 150.0, 300.0], "the new paths go to the last place asked for");
    assert!(w.off_path(&g, feet, &f).is_none());
    assert_eq!(w.height_waivers, 1);
    // Further than the same trip's reach: all new.
    go(&mut w, [160.0 + crate::follow::SAME_TRIP + 1.0, 150.0, 300.0]);
    let t = w.trip.as_ref().unwrap();
    assert_eq!((t.replans, t.waived), (0, false));
    // Another space is a new trip at any distance.
    let other = crate::follow::Trip { to: Place { space: Space::Interior(1), pos: [150.0, 150.0, 0.0] }, params: p.clone(), replans: 2, waived: true };
    w.trip = Some(other);
    go(&mut w, [150.0, 150.0, 0.0]);
    let t = w.trip.as_ref().unwrap();
    assert_eq!((t.replans, t.waived), (0, false));
}

#[test]
fn a_walker_that_fell_under_its_goal_goes_back_by_the_ramp() {
    let g = floor_with_deck_above();
    let mut w = walker_on_the_deck(&g);
    w.update(&g, 0.1);
    let under = [DECK_GOAL[0], DECK_GOAL[1], 0.0];
    w.feet = under;
    w.tri = NONE;
    let mut furthest = 0.0f32;
    let mut arrived = false;
    for _ in 0..600 {
        let ev = w.update(&g, 0.1);
        furthest = furthest.max(w.feet[0]);
        if ev == WalkEvent::Arrived {
            arrived = true;
            break;
        }
        assert_eq!(w.trip.as_ref().unwrap().replans, 1);
    }
    assert!(arrived, "at {:?}", w.feet);
    assert!(furthest > 700.0, "not by the ramp: {furthest}");
    assert!((w.feet[2] - 200.0).abs() < 1.0, "{:?}", w.feet);
    assert_eq!((w.height_searches, w.height_waivers), (1, 0));
}

/// A way back only through a door takes the waiver, and the walk ends where it stands.
fn the_waiver_is_taken_for_a_way_through_a_door(g: &NavGraph, w: &mut Walker, under: [f32; 3]) {
    w.update(g, 0.1);
    w.feet = under;
    w.tri = NONE;
    assert_eq!(w.update(g, 0.1), WalkEvent::Idle);
    assert!(w.follower.is_none(), "the walk is over");
    assert_eq!(w.feet, under, "no teleport");
    assert_eq!((w.height_searches, w.height_waivers), (1, 1));
}

#[test]
fn a_way_back_through_a_door_that_opens_in_place_takes_the_waiver() {
    let mut g = floor_with_deck_above();
    g.set_in_place_doors(vec![hinged(0xD00, 600.0, 0.0)], |_| None);
    let under = [DECK_GOAL[0], DECK_GOAL[1], 0.0];
    // There is that way, and it stops at the door.
    let way = g.find_path(Place { space: W, pos: under }, Place { space: W, pos: DECK_GOAL }, &PathParams::default()).unwrap();
    assert_eq!((way.legs.len(), way.legs[0].door_stops.len()), (1, 1));
    let mut w = walker_on_the_deck(&g);
    the_waiver_is_taken_for_a_way_through_a_door(&g, &mut w, under);
}

#[test]
fn a_way_back_through_a_load_door_takes_the_waiver() {
    // The floor and the deck are joined only through a room in another space.
    let room = Space::Interior(0x77);
    let mut floor = grid(1, [0.0, 0.0], 8, 4, 100.0, &[], flat);
    let mut deck = grid(2, [0.0, 300.0], 8, 1, 100.0, &[], |_, _| 200.0);
    let mut inside = grid(3, [5000.0, 5000.0], 3, 3, 100.0, &[], flat);
    let door_on = |m: &mut NavMeshRecord, door: u32, t: u16| {
        m.door_links.push(DoorLink { door, triangle: t });
        m.triangles[t as usize].flags |= tri_flags::DOOR;
    };
    door_on(&mut floor, 0xD1, 0);
    door_on(&mut inside, 0xD2, 0);
    door_on(&mut inside, 0xD3, 16);
    door_on(&mut deck, 0xD4, 0);
    let meshes = [(floor, W), (deck, W), (inside, room)];
    let input: Vec<_> = meshes.iter().map(|(m, s)| (m, Some(0), Some(*s))).collect();
    let mut g = NavGraph::assemble(&input);
    g.join_doors(&input, |door| match door {
        0xD1 => Some((0xD2, [5030.0, 5030.0, 0.0], 0.0, room)),
        0xD2 => Some((0xD1, [30.0, 30.0, 0.0], 0.0, W)),
        0xD3 => Some((0xD4, [30.0, 330.0, 200.0], 0.0, W)),
        0xD4 => Some((0xD3, [5270.0, 5270.0, 0.0], 0.0, room)),
        _ => None,
    });
    let under = [DECK_GOAL[0], DECK_GOAL[1], 0.0];
    // There is that way.
    let way = g.find_path(Place { space: W, pos: under }, Place { space: W, pos: DECK_GOAL }, &PathParams::default()).unwrap();
    assert!(way.legs.len() > 1, "{way:?}");
    let mut w = walker_on_the_deck(&g);
    the_waiver_is_taken_for_a_way_through_a_door(&g, &mut w, under);
}

#[test]
fn a_walk_that_ended_leaves_the_next_go_to_a_new_trip() {
    let g = graph(&[(grid(1, [0.0, 0.0], 4, 4, 100.0, &[], flat), W)]);
    let (p, goal) = (PathParams::default(), [150.0, 150.0, 0.0]);
    let feet = [150.0, 150.0, 65.0];
    let mut w = Walker::new(W, [50.0, 50.0, 0.0], 0.0, 100.0);
    w.go_to(&g, Place { space: W, pos: goal }, &p).unwrap();
    let f = w.follower.clone().unwrap();
    assert!(w.off_path(&g, feet, &f).is_none());
    assert!(w.trip.as_ref().unwrap().waived);
    // Cancelled (a Follow package caught up, say), then the same place again.
    w.follower = None;
    w.go_to(&g, Place { space: W, pos: goal }, &p).unwrap();
    let t = w.trip.as_ref().unwrap();
    assert_eq!((t.replans, t.waived), (0, false));
    // Arrived, then the same place again.
    assert!(w.off_path(&g, feet, &f).is_none());
    for _ in 0..200 {
        if w.update(&g, 0.1) == WalkEvent::Arrived {
            break;
        }
    }
    assert!(w.follower.is_none());
    w.go_to(&g, Place { space: W, pos: goal }, &p).unwrap();
    let t = w.trip.as_ref().unwrap();
    assert_eq!((t.replans, t.waived), (0, false));
}

#[test]
fn a_walker_with_no_way_back_ends_its_walk_where_it_stands() {
    // No ramp: the floor and the deck are not joined.
    let g = graph(&[(grid(1, [0.0, 0.0], 8, 4, 100.0, &[], flat), W), (grid(2, [0.0, 300.0], 8, 1, 100.0, &[], |_, _| 200.0), W)]);
    let mut w = walker_on_the_deck(&g);
    w.update(&g, 0.1);
    let under = [DECK_GOAL[0], DECK_GOAL[1], 0.0];
    w.feet = under;
    w.tri = NONE;
    assert_eq!(w.update(&g, 0.1), WalkEvent::Idle);
    assert!(w.follower.is_none(), "the walk is over");
    assert_eq!(w.feet, under, "no teleport");
    assert_eq!((w.height_searches, w.height_waivers), (1, 1));
}

/// A narrow L of squares 100 wide round the path (0, 0) to (100, 0) to (100, 300), its
/// surface rising northwards: squares (0, 0) and (1, 0) along the bottom, (1, 1) to (1, 3)
/// up the east side, the rest left out as `holes` plus any more given.
fn narrow_l(more_holes: &[(usize, usize)]) -> NavGraph {
    let mut holes = vec![(0, 1), (0, 2), (0, 3)];
    holes.extend_from_slice(more_holes);
    graph(&[(grid(1, [-50.0, -50.0], 2, 4, 100.0, &holes, |_, y| 0.1 * y), W)])
}

/// A leg over the points given, as a path planned without its corridor (a shortcut's way
/// is not in it), for a follower to walk.
fn leg_of(points: Vec<[f32; 3]>) -> crate::Leg {
    crate::Leg { space: W, points, corridor: vec![], door: None, door_stops: vec![], hops: vec![] }
}

fn follower_of(leg: crate::Leg) -> crate::Follower {
    crate::Follower::new(crate::Path { legs: vec![leg], length: 0.0, expanded: 0, start_snap: 0.0, goal_snap: 0.0 })
}

const LADDER: [f32; 5] = [160.0, 120.0, 80.0, 40.0, 0.0];

/// #1501: feet off the first stretch of a narrow L, in the corridor's bend. The point it
/// used to give, straight from the feet to the second corner and on, is not asked for.
#[test]
fn a_skip_round_a_narrow_bend_has_the_surface_under_it() {
    let g = narrow_l(&[]);
    let leg = leg_of(vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 300.0, 30.0]]);
    let f = follower_of(leg.clone());
    let c = f.skip_candidates([60.0, 30.0, 3.0], &LADDER)[0];
    assert_eq!(c.next, 2);
    assert!((c.pos[0] - 100.0).abs() < 1e-3 && (c.pos[1] - 120.0).abs() < 1e-3, "{c:?}");
    let on = g.skip_surface(&leg, &c).expect("on the navmesh");
    // The surface's height there, not the path's guess at it.
    assert!((on[2] - 12.0).abs() < 1e-3, "{on:?}");
    assert!(g.locate(W, on, 1.0).is_some());
    // What was not on the path is not on the mesh here either: nothing is made up for it.
    let off = crate::follow::SkipCandidate { pos: [20.0, 120.0, 12.0], ..c };
    assert_eq!(g.skip_surface(&leg, &off), None);
}

#[test]
fn a_skip_over_a_gap_in_the_navmesh_falls_back_to_a_nearer_place() {
    // No navmesh from y 150 to 250 (one square), where the far places fall.
    let g = narrow_l(&[(1, 2)]);
    let leg = leg_of(vec![[100.0, 0.0, 0.0], [100.0, 300.0, 30.0]]);
    let f = follower_of(leg.clone());
    let cs = f.skip_candidates([100.0, 40.0, 4.0], &LADDER);
    assert_eq!(cs.iter().map(|c| c.pos[1]).collect::<Vec<_>>(), vec![200.0, 160.0, 120.0, 80.0]);
    let surfaces: Vec<_> = cs.iter().map(|c| g.skip_surface(&leg, c).is_some()).collect();
    assert_eq!(surfaces, vec![false, false, true, true]);
    assert_eq!(f.next, 1, "nothing committed");
}

#[test]
fn a_skip_past_a_gap_is_found_by_the_corners_past_it() {
    let g = narrow_l(&[(1, 2)]);
    // The far side of the gap as a corner of the path: its own triangle.
    let leg = leg_of(vec![[100.0, 0.0, 0.0], [100.0, 280.0, 28.0], [100.0, 300.0, 30.0]]);
    let f = follower_of(leg.clone());
    let c = f.skip_candidate([100.0, 200.0, 20.0], 80.0).unwrap();
    assert_eq!(c.pos[1], 280.0);
    assert!((g.skip_surface(&leg, &c).unwrap()[2] - 28.0).abs() < 1e-3);
    // Between two corners, the walk from the one before it cut off: from the one after
    // it, whatever the height the path gives there.
    let leg = leg_of(vec![[100.0, 0.0, 0.0], [100.0, 340.0, 34.0]]);
    let f = follower_of(leg.clone());
    let c = f.skip_candidate([100.0, 200.0, 20.0], 80.0).unwrap();
    assert!((g.skip_surface(&leg, &c).unwrap()[2] - 28.0).abs() < 1e-3);
    let off = crate::follow::SkipCandidate { pos: [c.pos[0], c.pos[1], c.pos[2] + 90.0], ..c };
    assert!((g.skip_surface(&leg, &off).unwrap()[2] - 28.0).abs() < 1e-3);
}

/// #1501, review: what the actor is stuck at can be what cuts the navmesh behind the place.
/// An obstacle came up on the straight stretch since it was planned: the triangles under it
/// are gone, the place past them is on a triangle the corridor does not list, and it is
/// the one to go to.
#[test]
fn a_skip_past_an_obstacle_that_cut_the_navmesh_since_the_plan_has_a_surface() {
    let mut g = graph(&[(grid(1, [0.0, 0.0], 10, 1, 100.0, &[], flat), W)]);
    let (start, end) = ([50.0, 50.0, 0.0], [950.0, 50.0, 0.0]);
    let mut leg = leg_of(vec![start, end]);
    leg.corridor = vec![g.locate(W, start, 1.0).unwrap()];
    let f = follower_of(leg.clone());
    let cs = f.skip_candidates([440.0, 50.0, 0.0], &[170.0, 60.0]);
    assert_eq!(cs.iter().map(|c| c.pos[0]).collect::<Vec<_>>(), vec![610.0, 500.0]);
    let past = g.locate(W, cs[0].pos, 1.0).unwrap();
    assert!(!leg.corridor.contains(&past));
    assert!(g.skip_surface(&leg, &cs[0]).is_some());
    g.set_obstacles(vec![sandbags(0.0, 90.0)], |o| o.initially_enabled);
    g.sync_obstacles([(0x5B0, true)]);
    assert!(g.cut_triangles() >= 2);
    // No way along the path from the corner before it, past the obstacle.
    let from = g.locate(W, start, 1.0).unwrap();
    assert_eq!(g.walkable_line(W, from, start, cs[0].pos, 0.0, &PathParams::default()), None);
    assert_eq!(g.skip_surface(&leg, &cs[0]), Some([cs[0].pos[0], cs[0].pos[1], 0.0]));
    // Under the obstacle itself there is no floor.
    assert_eq!(g.skip_surface(&leg, &cs[1]), None);
}

/// With both walks cut off: a triangle under the place, on the path's level.
#[test]
fn a_skip_onto_an_island_between_two_gaps_keeps_to_the_paths_height() {
    let z = |_: f32, y: f32| 0.1 * y;
    let g = graph(&[(grid(1, [50.0, 0.0], 1, 7, 100.0, &[(0, 1), (0, 5)], z), W)]);
    let leg = leg_of(vec![[100.0, 50.0, 5.0], [100.0, 650.0, 65.0]]);
    let f = follower_of(leg.clone());
    let c = f.skip_candidate([100.0, 140.0, 14.0], 160.0).unwrap();
    assert_eq!(c.pos[1], 300.0);
    assert!((g.skip_surface(&leg, &c).unwrap()[2] - 30.0).abs() < 1e-3);
    for off in [90.0, -90.0] {
        let far = crate::follow::SkipCandidate { pos: [c.pos[0], c.pos[1], c.pos[2] + off], ..c };
        assert_eq!(g.skip_surface(&leg, &far), None, "not within the level gap");
    }
}

/// With no corner to walk from, of two floors stacked and not linked: the nearer in height,
/// and within the limit.
#[test]
fn a_skip_with_nothing_to_walk_from_keeps_to_the_nearer_of_stacked_floors() {
    let lower = grid(1, [0.0, 0.0], 2, 1, 100.0, &[], flat);
    let upper = grid(2, [0.0, 0.0], 2, 1, 100.0, &[], |_, _| 100.0);
    let g = graph(&[(lower, W), (upper, W)]);
    let leg = leg_of(vec![[-500.0, 50.0, 0.0], [700.0, 50.0, 0.0]]);
    let at = |z: f32| crate::follow::SkipCandidate { pos: [100.0, 50.0, z], next: 1, along: 0.0, at_limit: false };
    assert_eq!(g.skip_surface(&leg, &at(90.0)).map(|p| p[2]), Some(100.0));
    assert_eq!(g.skip_surface(&leg, &at(10.0)).map(|p| p[2]), Some(0.0));
    assert_eq!(g.skip_surface(&leg, &at(50.0)), None, "neither within the level gap");
}

#[test]
fn a_skip_keeps_to_the_level_of_its_route() {
    let g = floor_with_deck_above();
    // On the floor under the deck, between corners and at a corner: not the deck.
    let leg = leg_of(vec![[50.0, 350.0, 0.0], [350.0, 350.0, 0.0]]);
    let f = follower_of(leg.clone());
    let c = f.skip_candidate([50.0, 350.0, 0.0], 160.0).unwrap();
    assert_eq!(g.skip_surface(&leg, &c).map(|p| p[2]), Some(0.0));
    // On the deck over it: not the floor.
    let leg = leg_of(vec![[50.0, 350.0, 200.0], [350.0, 350.0, 200.0]]);
    let f = follower_of(leg.clone());
    let c = f.skip_candidate([50.0, 350.0, 200.0], 160.0).unwrap();
    assert_eq!(g.skip_surface(&leg, &c).map(|p| p[2]), Some(200.0));
    let end = f.skip_candidate([50.0, 350.0, 200.0], 1000.0).unwrap();
    assert_eq!(g.skip_surface(&leg, &end), Some([350.0, 350.0, 200.0]));
}

#[test]
fn a_skips_height_follows_the_ground_over_a_rise_the_path_cuts_through() {
    // A hump 200 high between two ends at height 0: the straight path's height is far
    // under the surface half way.
    let hump = |x: f32, _: f32| 200.0 - (x - 400.0).abs() * 0.5;
    let g = graph(&[(grid(1, [0.0, -100.0], 8, 2, 100.0, &[], hump), W)]);
    let leg = leg_of(vec![[0.0, 0.0, 0.0], [800.0, 0.0, 0.0]]);
    let f = follower_of(leg.clone());
    let c = f.skip_candidate([240.0, 0.0, 0.0], 160.0).unwrap();
    assert_eq!(c.pos, [400.0, 0.0, 0.0]);
    let on = g.skip_surface(&leg, &c).expect("the surface is walked to");
    assert!((on[2] - 200.0).abs() < 1e-3, "{on:?}");
}

#[test]
fn a_ledge_links_span_has_nothing_to_stand_on_but_its_ends_have() {
    let mut a = grid(1, [0.0, 0.0], 2, 2, 100.0, &[], flat);
    let b = grid(2, [200.0, 0.0], 2, 2, 100.0, &[], |_, _| -100.0);
    let (ea, eb) = (edges_at(&a, 200.0), edges_at(&b, 200.0));
    for &(at, ae, ..) in &ea {
        link(&mut a, at, ae, 2, eb[0].0, link_kind::LEDGE_DOWN);
    }
    let g = graph(&[(a, W), (b, W)]);
    let mut leg = leg_of(vec![[50.0, 50.0, 0.0], [190.0, 50.0, 0.0], [260.0, 50.0, -100.0], [350.0, 50.0, -100.0]]);
    leg.hops = vec![crate::LedgeHop { point: 1, up: false }];
    let f = follower_of(leg.clone());
    let cs = f.skip_candidates([150.0, 50.0, 0.0], &[120.0, 80.0, 40.0]);
    assert_eq!(cs.iter().map(|c| c.pos[0]).collect::<Vec<_>>(), vec![270.0, 230.0, 190.0]);
    // Past the landing, on the lower floor.
    assert!((g.skip_surface(&leg, &cs[0]).unwrap()[2] + 100.0).abs() < 1e-3);
    // In the air over the drop, although a triangle lies under it.
    assert!(g.locate(W, cs[1].pos, 1000.0).is_some());
    assert_eq!(g.skip_surface(&leg, &cs[1]), None);
    // The take-off, which is on the upper floor.
    assert!(g.skip_surface(&leg, &cs[2]).unwrap()[2].abs() < 1e-3);
    // The landing itself, with a door stop there as the limit.
    let mut stopped = leg.clone();
    stopped.door_stops = vec![crate::DoorStop { door: 9, point: 2 }];
    let f = follower_of(stopped.clone());
    let c = f.skip_candidate([150.0, 50.0, 0.0], 160.0).unwrap();
    assert!(c.at_limit);
    assert!((g.skip_surface(&stopped, &c).unwrap()[2] + 100.0).abs() < 1e-3);
}

#[test]
fn a_skip_onto_a_navmesh_since_disabled_finds_no_surface() {
    let mut g = narrow_l(&[]);
    let leg = leg_of(vec![[100.0, 0.0, 0.0], [100.0, 300.0, 30.0]]);
    let f = follower_of(leg.clone());
    let c = f.skip_candidate([100.0, 0.0, 0.0], 160.0).unwrap();
    assert!(g.skip_surface(&leg, &c).is_some());
    assert!(g.set_enabled(1, false));
    assert_eq!(g.skip_surface(&leg, &c), None);
}
