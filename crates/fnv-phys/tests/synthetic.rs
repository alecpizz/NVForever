//! fnv-phys on hand-built worlds: terrain from synthetic LAND heights and references
//! with hand-built `fnv_collide` shapes (no game data).

use fnv_collide::{CollisionObject, Iso, Motion, Shape};
use fnv_phys::terrain::height_at;
use fnv_phys::*;
use fnv_phys::layers;
use fnv_world::land::SIDE;
use fnv_world::CELL_SIZE;
use proptest::prelude::*;

const ID: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn rz(a: f32) -> [f32; 9] {
    let (s, c) = a.sin_cos();
    [c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0]
}

fn flat(z: f32) -> Vec<f32> {
    vec![z; SIDE * SIDE]
}

fn object(shape: Shape, motion: Motion) -> CollisionObject {
    CollisionObject {
        node: None,
        node_block: 0,
        node_transform: Iso::IDENTITY,
        body_transform: Iso::IDENTITY,
        motion,
        layer: 1,
        mass: 10.0,
        friction: 0.5,
        restitution: 0.0,
        shape: shape.into(),
        shell: 0.0,
        material: None,
        wind: false,
    }
}

fn at(form_id: u32, position: [f32; 3]) -> RefPose {
    RefPose { form_id, position, rotation: ID, scale: 1.0 }
}

fn boxed(h: [f32; 3]) -> Shape {
    Shape::Box { half_extents: h }
}

/// Flat ground at z = 0 over the four cells around the origin.
fn ground() -> PhysWorld {
    let mut w = PhysWorld::new(PhysParams::default());
    for x in -1..=0 {
        for y in -1..=0 {
            w.add_terrain((x, y), &flat(0.0));
        }
    }
    w
}

/// Rays start close to the geometry: f32 ray–sphere tests from thousands of units away
/// lose about a unit of precision (physics.md §5).
fn down(w: &mut PhysWorld, x: f32, y: f32) -> Option<RayHit> {
    w.queries().ray([x, y, 1_000.0], [0.0, 0.0, -1.0], 2_000.0, &Filter::default())
}

#[test]
fn terrain_seams_and_owners() {
    let mut w = PhysWorld::new(PhysParams::default());
    let slope: Vec<f32> = (0..SIDE * SIDE).map(|k| (k % SIDE) as f32 * 4.0).collect();
    w.add_terrain((0, 0), &slope);
    w.add_terrain((1, 0), &slope.iter().map(|h| h + 128.0).collect::<Vec<_>>());
    // Exactly on the shared edge, and just either side of it.
    for (x, want_cell) in [(CELL_SIZE - 0.5, 0), (CELL_SIZE + 0.5, 1)] {
        let hit = down(&mut w, x, 1000.0).expect("hit");
        assert_eq!(hit.owner, Owner::Terrain { x: want_cell, y: 0 });
    }
    let edge = down(&mut w, CELL_SIZE, 1000.0).expect("rays on a cell edge hit");
    assert!((edge.point[2] - 128.0).abs() < 0.01);
    let local = height_at(&slope, 333.0, 1000.0).unwrap();
    let hit = down(&mut w, 333.0, 1000.0).unwrap();
    assert!((hit.point[2] - local).abs() < 0.01);
    assert!(hit.normal[2] > 0.99);
    assert!(w.remove_terrain((0, 0)));
    assert!(down(&mut w, 333.0, 1000.0).is_none());
    assert_eq!(w.stats().terrain_cells, 1);
}

#[test]
fn reference_transform_rotation_and_scale() {
    let mut w = ground();
    // A 10 × 2 × 2 box offset 20 along the node's x, the reference turned 90° about z
    // and scaled ×2: the box ends up along world y, centred 40 north of the reference.
    let mut o = object(boxed([10.0, 2.0, 2.0]), Motion::Static);
    o.node_transform = Iso { translation: [20.0, 0.0, 0.0], rotation: ID };
    let pose = RefPose { form_id: 0x1234, position: [100.0, 100.0, 50.0], rotation: rz(std::f32::consts::FRAC_PI_2), scale: 2.0 };
    let r = w.add_ref(&pose, &[o]);
    assert_eq!((r.bodies, r.colliders, r.errors.len()), (1, 1, 0));
    let hit = down(&mut w, 100.0, 140.0 + 19.0).expect("hits the box");
    assert_eq!(hit.owner, Owner::Ref { form_id: 0x1234, object: 0, layer: 1 });
    assert!((hit.point[2] - 54.0).abs() < 1e-3, "{hit:?}");
    // Width along world x is 2 × 2 × 2 = 8: 3 units off-axis hits, 5 misses.
    assert!(matches!(down(&mut w, 103.0, 140.0).unwrap().owner, Owner::Ref { .. }));
    assert!(matches!(down(&mut w, 105.0, 140.0).unwrap().owner, Owner::Terrain { .. }));
    assert!(w.remove_ref(0x1234));
    assert!(matches!(down(&mut w, 100.0, 140.0).unwrap().owner, Owner::Terrain { .. }));
}

#[test]
fn body_transform_composes_after_node() {
    let mut w = ground();
    let mut o = object(Shape::Sphere { radius: 5.0 }, Motion::Static);
    o.node_transform = Iso { translation: [0.0, 0.0, 30.0], rotation: rz(std::f32::consts::FRAC_PI_2) };
    o.body_transform = Iso { translation: [10.0, 0.0, 0.0], rotation: ID };
    w.add_ref(&at(7, [0.0, 0.0, 0.0]), &[o]);
    // node ∘ body: the body offset is turned by the node rotation → (0, 10, 30).
    let hit = w.queries().ray([0.0, 10.0, 100.0], [0.0, 0.0, -1.0], 200.0, &Filter::default()).unwrap();
    assert!((hit.point[2] - 35.0).abs() < 1e-2, "{hit:?}");
}

#[test]
fn bad_inputs_are_reported_not_dropped() {
    let mut w = ground();
    let r = w.add_ref(&RefPose { rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0], ..at(1, [0.0; 3]) }, &[object(boxed([1.0; 3]), Motion::Static)]);
    assert_eq!((r.bodies, r.errors.len()), (0, 1));
    let shape = Shape::Compound {
        children: vec![(Iso::IDENTITY, boxed([1.0; 3])), (Iso::IDENTITY, Shape::Sphere { radius: -1.0 })],
    };
    let r = w.add_ref(&at(2, [0.0; 3]), &[object(shape, Motion::Static)]);
    assert_eq!((r.bodies, r.colliders, r.errors), (1, 1, vec![ConvertError::BadDimension]));
}

#[test]
fn non_finite_positions_are_refused() {
    let mut w = ground();
    // A NaN in the reference's position, the node's translation or a hull point: no body
    // reaches the world, and stepping stays finite.
    let r = w.add_ref(&at(1, [f32::NAN, 0.0, 0.0]), &[object(boxed([1.0; 3]), Motion::Dynamic)]);
    assert_eq!((r.bodies, r.errors.len()), (0, 1));
    let mut o = object(boxed([1.0; 3]), Motion::Dynamic);
    o.node_transform.translation = [0.0, f32::INFINITY, 0.0];
    let r = w.add_ref(&at(2, [0.0, 0.0, 50.0]), &[o]);
    assert_eq!((r.bodies, r.errors), (0, vec![ConvertError::BadDimension]));
    let shape = Shape::Compound {
        children: vec![
            (Iso { translation: [f32::NAN; 3], rotation: ID }, Shape::Sphere { radius: 1.0 }),
            (Iso::IDENTITY, Shape::ConvexHull { points: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, f32::NAN]] }),
            (Iso::IDENTITY, boxed([1.0; 3])),
        ],
    };
    let r = w.add_ref(&at(3, [0.0, 0.0, 50.0]), &[object(shape, Motion::Dynamic)]);
    assert_eq!((r.bodies, r.colliders, r.errors.len()), (1, 1, 2));
    for _ in 0..30 {
        w.step();
    }
    assert!(down(&mut w, 0.0, 0.0).is_some_and(|h| h.point.iter().all(|v| v.is_finite())));
}

#[test]
fn trimesh_material_on_ray_hit() {
    let mut w = PhysWorld::new(PhysParams::default());
    let mesh = Shape::TriMesh {
        vertices: vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 100.0, 0.0], [0.0, 100.0, 0.0]],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        materials: vec![11, 22],
    };
    w.add_ref(&at(9, [0.0; 3]), &[object(mesh, Motion::Static)]);
    assert_eq!(down(&mut w, 80.0, 20.0).unwrap().material, Some(11));
    assert_eq!(down(&mut w, 20.0, 80.0).unwrap().material, Some(22));
    // From below (back face).
    let up = w.queries().ray([20.0, 80.0, -50.0], [0.0, 0.0, 1.0], 100.0, &Filter::default()).unwrap();
    assert_eq!(up.material, Some(22));
}

#[test]
fn convex_material_on_ray_hit() {
    // A bottle's convex hull carries one material (24 BOTTLE); a box without one has none.
    let mut w = PhysWorld::new(PhysParams::default());
    let hull = Shape::ConvexHull { points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0], [10.0, 10.0, 10.0]] };
    w.add_ref(&at(9, [0.0; 3]), &[CollisionObject { material: Some(24), ..object(hull, Motion::Static) }]);
    w.add_ref(&at(10, [50.0, 50.0, 0.0]), &[object(boxed([5.0; 3]), Motion::Static)]);
    assert_eq!(down(&mut w, 5.0, 5.0).unwrap().material, Some(24));
    assert_eq!(down(&mut w, 50.0, 50.0).unwrap().material, None);
}

#[test]
fn ray_all_sweep_overlap_and_filters() {
    let mut w = ground();
    w.add_ref(&at(1, [0.0, 0.0, 10.0]), &[object(boxed([5.0; 3]), Motion::Static)]);
    w.add_ref(&at(2, [0.0, 0.0, 40.0]), &[object(boxed([5.0; 3]), Motion::Static)]);
    let q = w.queries();
    // Off the four-cell corner at the origin, so one terrain cell is below.
    let all = q.ray_all([1.0, 1.0, 100.0], [0.0, 0.0, -1.0], 200.0, &Filter::default());
    let keys: Vec<Key> = all.iter().map(|h| h.owner.key()).collect();
    assert_eq!(keys, vec![Key::Ref(2), Key::Ref(1), Key::Terrain(0, 0)]);
    let skip2 = q.ray([0.0, 0.0, 100.0], [0.0, 0.0, -1.0], 200.0, &Filter::excluding(Key::Ref(2))).unwrap();
    assert_eq!(skip2.owner.key(), Key::Ref(1));
    let no_terrain = Filter { no_terrain: true, ..Default::default() };
    assert!(q.ray([50.0, 0.0, 100.0], [0.0, 0.0, -1.0], 200.0, &no_terrain).is_none());
    // A sphere of radius 2 swept down from z = 100 touches the top box (z = 45) after 53.
    let ball = Shape::Sphere { radius: 2.0 };
    let from = Iso { translation: [0.0, 0.0, 100.0], rotation: ID };
    let hit = q.sweep(&ball, &from, [0.0, 0.0, -100.0], &Filter::default()).unwrap().unwrap();
    assert_eq!(hit.owner.key(), Key::Ref(2));
    assert!((hit.fraction - 0.53).abs() < 1e-3, "{hit:?}");
    assert!((hit.point[2] - 45.0).abs() < 1e-2 && hit.normal[2] > 0.99, "{hit:?}");
    let big = Shape::Sphere { radius: 22.0 };
    let mid = Iso { translation: [0.0, 0.0, 17.0], rotation: ID };
    let over: Vec<Key> = q.overlap(&big, &mid, &Filter::default()).unwrap().iter().map(|o| o.key()).collect();
    assert_eq!(over, vec![Key::Terrain(-1, -1), Key::Terrain(-1, 0), Key::Terrain(0, -1), Key::Terrain(0, 0), Key::Ref(1), Key::Ref(2)]);
}

#[test]
fn dynamic_clutter_settles_on_terrain() {
    let mut w = ground();
    let o = object(boxed([4.0, 4.0, 4.0]), Motion::Dynamic);
    w.add_ref(&RefPose { rotation: rz(0.3), ..at(5, [10.0, 10.0, 200.0]) }, &[o]);
    for _ in 0..600 {
        w.step();
    }
    let b = w.ref_bodies(5)[0];
    // On the terrain's contact shell: 3.5 above the bare surface (physics.md §4.3).
    assert!((b.translation[2] - (4.0 + fnv_phys::TERRAIN_SHELL)).abs() < 0.5, "{b:?}");
    assert!((b.translation[0] - 10.0).abs() < 5.0 && (b.translation[1] - 10.0).abs() < 5.0, "{b:?}");
    assert!(b.sleeping || b.linvel.iter().all(|v| v.abs() < 1.0), "{b:?}");
    let s = w.stats();
    assert_eq!((s.bodies_dynamic, s.steps), (1, 600));
}

/// Clutter placed resting on the terrain falls asleep and stays put, a stack included.
/// With the terrain's contact skin larger than rapier's prediction distance, such bodies
/// crept along the ground for ever, never asleep [observed: Goodsprings' motorcycles and
/// crates outside the saloon, physics.md §4.3].
#[test]
fn clutter_on_the_terrain_shell_falls_asleep() {
    let mut w = ground();
    let shell = fnv_phys::TERRAIN_SHELL;
    // Placed as a level designer leaves them: a little above where they rest.
    let mut bike = object(boxed([60.0, 20.0, 30.0]), Motion::Dynamic);
    (bike.mass, bike.restitution) = (600.0, 0.4);
    w.add_ref(&RefPose { rotation: rz(0.4), ..at(1, [100.0, 100.0, 30.0 + shell + 0.5]) }, &[bike]);
    let mut crate_ = object(boxed([40.0, 35.0, 18.0]), Motion::Dynamic);
    (crate_.mass, crate_.restitution) = (20.0, 0.4);
    w.add_ref(&at(2, [-200.0, -150.0, 18.0 + shell + 0.5]), std::slice::from_ref(&crate_));
    w.add_ref(&RefPose { rotation: rz(0.1), ..at(3, [-198.0, -152.0, 54.0 + shell + 1.0]) }, &[crate_]);
    for _ in 0..180 {
        w.step();
    }
    let rested: Vec<BodyState> = (1..=3).map(|id| w.ref_bodies(id)[0]).collect();
    for _ in 0..600 {
        w.step();
    }
    for (id, before) in (1..=3).zip(rested) {
        let b = w.ref_bodies(id)[0];
        let moved = (0..3).map(|i| (b.translation[i] - before.translation[i]).powi(2)).sum::<f32>().sqrt();
        assert!(b.sleeping && moved < 0.05, "ref {id}: moved {moved} in 10 s after settling, {b:?}");
    }
    let bike = w.ref_bodies(1)[0];
    assert!((bike.translation[2] - (30.0 + shell)).abs() < 0.5, "{bike:?}");
}

/// A body that responds to the wind starts awake even when placed asleep, and the wind
/// rolls it downwind; one that does not is not blown (docs/spec-notes/wind.md).
#[test]
fn the_wind_blows_the_bodies_that_respond_to_it() {
    let mut w = ground();
    let shell = fnv_phys::TERRAIN_SHELL;
    let ball = |wind| CollisionObject { mass: 2.0, wind, ..object(Shape::Sphere { radius: 30.0 }, Motion::Dynamic) };
    let params = w.params;
    w.insert_ref(PreparedRef::new(&params, &at(1, [0.0, 0.0, 30.0 + shell]), &[ball(true)]).asleep());
    w.insert_ref(PreparedRef::new(&params, &at(2, [-400.0, 0.0, 30.0 + shell]), &[ball(false)]).asleep());
    assert_eq!(w.wind_refs().iter().map(|r| r.0).collect::<Vec<_>>(), vec![1]);
    assert!(!w.ref_bodies(1)[0].sleeping && w.ref_bodies(2)[0].sleeping);
    let dt = params.dt;
    for _ in 0..120 {
        assert_eq!(w.blow(2, [200.0, 0.0], 300.0, dt), 0);
        w.blow(1, [200.0, 0.0], 300.0, dt);
        w.step();
    }
    let b = w.ref_bodies(1)[0];
    assert!(b.translation[0] > 100.0 && b.translation[1].abs() < 20.0, "{b:?}");
    assert!(b.linvel[0] > 100.0 && b.linvel[0] < 230.0, "{b:?}");
    assert_eq!(w.ref_bodies(2)[0].translation[0], -400.0);
}

#[test]
fn fixed_bounds_cover_static_and_keyframed_collision_only() {
    let mut w = PhysWorld::new(PhysParams::default());
    assert_eq!(w.fixed_bounds(), None);
    w.add_ref(&at(1, [100.0, 0.0, 500.0]), &[object(boxed([50.0, 20.0, 10.0]), Motion::Static)]);
    w.add_ref(&at(2, [0.0, 0.0, -900.0]), &[object(boxed([5.0, 5.0, 5.0]), Motion::Dynamic)]);
    w.add_character(9, CharacterParams::default(), [0.0, 0.0, -2000.0]);
    let (lo, hi) = w.fixed_bounds().expect("one fixed box");
    let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.1);
    assert!(close(lo, [50.0, -20.0, 490.0]) && close(hi, [150.0, 20.0, 510.0]), "{lo:?} {hi:?}");
}

#[test]
fn falling_body_follows_gravity() {
    // Free fall for 0.5 s: z = z0 - g t² / 2 (semi-implicit Euler is within g·dt·t).
    let params = PhysParams::default();
    let mut w = PhysWorld::new(params);
    w.add_ref(&at(1, [0.0, 0.0, 10_000.0]), &[object(Shape::Sphere { radius: 1.0 }, Motion::Dynamic)]);
    for _ in 0..30 {
        w.step();
    }
    let t = 30.0 * params.dt;
    let z = w.ref_bodies(1)[0].translation[2];
    let want = 10_000.0 - params.gravity * t * t / 2.0;
    assert!((z - want).abs() <= params.gravity * params.dt * t + 1e-2, "{z} vs {want}");
}

fn scene() -> PhysWorld {
    let mut w = ground();
    for i in 0..12u32 {
        let o = object(boxed([3.0, 3.0, 3.0]), Motion::Dynamic);
        w.add_ref(&RefPose { rotation: rz(i as f32), ..at(100 + i, [(i % 4) as f32 * 5.0, (i / 4) as f32 * 5.0, 20.0 + i as f32 * 7.0]) }, &[o]);
    }
    w
}

#[test]
fn stepping_is_deterministic() {
    let (mut a, mut b) = (scene(), scene());
    let mut digests = Vec::new();
    for _ in 0..240 {
        a.step();
        b.step();
        assert_eq!(a.state_digest(), b.state_digest());
        digests.push(a.state_digest());
    }
    // Something actually moved.
    assert!(digests.windows(2).any(|d| d[0] != d[1]));
}

#[test]
fn stepper_runs_whole_fixed_steps() {
    let p = PhysParams { dt: 0.01, max_substeps: 4, ..Default::default() };
    let mut s = Stepper::default();
    assert_eq!(s.advance(&p, 0.025), 2);
    assert_eq!(s.advance(&p, 0.006), 1);
    assert!((s.accumulated - 0.001).abs() < 1e-6);
    assert!((s.alpha(p.dt) - 0.1).abs() < 1e-4);
    assert_eq!(s.advance(&p, 1.0), 4);
    assert!(s.accumulated <= f64::from(p.dt));
    assert_eq!(s.advance(&p, -1.0), 1);
}

#[test]
fn stepper_does_not_drift_over_an_hour_of_frames() {
    // An hour at 144 frames a second, 60 steps a second: every step of the hour, none lost
    // or gained to the remainder's rounding.
    let p = PhysParams { dt: 1.0 / 60.0, max_substeps: 15, ..Default::default() };
    let mut s = Stepper::default();
    let frames = 144 * 3600;
    let steps: u64 = (0..frames).map(|_| u64::from(s.advance(&p, 1.0 / 144.0))).sum();
    // The step is 1/60 in f32 (a hair over), the frame 1/144 in f32: at most one step off.
    let exact = (f64::from(1.0f32 / 144.0) * f64::from(frames) / f64::from(p.dt)).floor() as u64;
    assert!(steps.abs_diff(exact) <= 1, "{steps} steps, {exact} expected");
}

#[test]
fn stepper_keeps_the_blend_when_the_step_changes() {
    let p60 = PhysParams { dt: 1.0 / 60.0, max_substeps: 15, ..Default::default() };
    let mut s = Stepper::default();
    s.advance(&p60, 0.01);
    let before = s.alpha(p60.dt);
    s.rescale(p60.dt, 1.0 / 152.0);
    assert!((s.alpha(1.0 / 152.0) - before).abs() < 1e-5, "the drawn pose does not jump");
    s.rescale(1.0 / 152.0, 1.0 / 240.0);
    assert!((s.alpha(1.0 / 240.0) - before).abs() < 1e-5);
    // Nothing to rescale from or to.
    s.rescale(0.0, 1.0 / 60.0);
    s.rescale(1.0 / 60.0, f32::NAN);
    assert!((s.alpha(1.0 / 240.0) - before).abs() < 1e-5);
    assert_eq!(Stepper::default().alpha(0.0), 0.0);
}

// --- character controller ---

fn settle(w: &mut PhysWorld, c: &mut Character, steps: usize) -> CharacterStep {
    let mut last = None;
    for _ in 0..steps {
        last = Some(w.move_character(c, CharacterInput::default()));
        w.step();
    }
    last.unwrap()
}

fn walk(w: &mut PhysWorld, c: &mut Character, velocity: [f32; 2], steps: usize) -> Vec<CharacterStep> {
    (0..steps)
        .map(|_| {
            let s = w.move_character(c, CharacterInput { velocity, jump: false, ..Default::default() });
            w.step();
            s
        })
        .collect()
}

#[test]
fn character_falls_lands_and_walks() {
    let mut w = ground();
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 300.0]);
    let mut landed = None;
    for _ in 0..120 {
        let s = w.move_character(&mut c, CharacterInput::default());
        w.step();
        if let Some(v) = s.landed_speed {
            landed = Some(v);
        }
    }
    assert!(c.grounded);
    // Fell ~300 units: touchdown speed ≈ sqrt(2 g h).
    let v = landed.expect("landed once");
    let want = (2.0 * w.params.gravity * 300.0f32).sqrt();
    assert!((v - want).abs() < 0.05 * want, "{v} vs {want}");
    // On the terrain's contact shell (physics.md §6.2).
    assert!((c.feet[2] - TERRAIN_SHELL).abs() < 0.02, "{:?}", c.feet);
    // fMoveBaseSpeed is 77 in FalloutNV.esm [observed]; whether it is a walk speed in
    // units/s is a hypothesis, used here only as a plausible speed.
    let steps = walk(&mut w, &mut c, [77.0, 0.0], 60);
    assert!(steps.iter().all(|s| s.grounded));
    assert!((c.feet[0] - 77.0).abs() < 1.0, "{:?}", c.feet);
    assert!(c.feet[1].abs() < 0.1, "{:?}", c.feet);
}

#[test]
fn character_climbs_steps_but_not_walls() {
    let p = CharacterParams::default();
    for (height, climbs) in [(p.step_height * 0.6, true), (p.step_height * 2.0, false)] {
        let mut w = ground();
        // A wide block whose west face is at x = 100.
        w.add_ref(&at(1, [300.0, 0.0, height / 2.0]), &[object(boxed([200.0, 200.0, height / 2.0]), Motion::Static)]);
        let mut c = w.add_character(1, p, [0.0, 0.0, 1.0]);
        settle(&mut w, &mut c, 30);
        walk(&mut w, &mut c, [150.0, 0.0], 90);
        if climbs {
            assert!(c.feet[0] > 150.0 && (c.feet[2] - height).abs() < 2.0, "height {height}: {:?}", c.feet);
        } else {
            assert!(c.feet[0] < 100.0 - p.radius + 1.0 && c.feet[2] < TERRAIN_SHELL + 1.0, "height {height}: {:?}", c.feet);
        }
    }
}

#[test]
fn character_climbs_shallow_steps_without_jumping() {
    // Steps like the camper's door at the Powder Ganger camp (playtest list 5 item 17): a
    // 10-unit step with a 24-unit top, then a 20-unit one. The cone bottom meets each edge
    // with its own steep face, which rapier's autostep refused (`step_up_move`).
    let mut w = ground();
    w.add_ref(&at(1, [112.0, 0.0, 5.0]), &[object(boxed([12.0, 200.0, 5.0]), Motion::Static)]);
    w.add_ref(&at(2, [324.0, 0.0, 15.0]), &[object(boxed([200.0, 200.0, 15.0]), Motion::Static)]);
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    let steps = walk(&mut w, &mut c, [344.0, 0.0], 60);
    assert!(c.feet[0] > 200.0 && (c.feet[2] - 30.0).abs() < 2.0, "{:?}", c.feet);
    assert!(c.stepped > 0, "climbed by the step-up");
    assert!(steps.iter().all(|s| s.landed_speed.is_none_or(|v| v < 100.0)), "no fall onto the steps");
}

/// A caravan's doorway like `MobileHomeASNV`'s (issue #2494): a floor 50 above the ground
/// ending at x = 100, where a zero-thickness lip `lip` units high rises from its edge,
/// door jambs either side (90 apart), and an open gap 15 wide down to the ground beyond;
/// with `ramp`, a plank ramp whose upper end hangs over the gap's far side 18 above the
/// floor and slopes down outwards at 24 degrees.
fn caravan_door(lip: f32, ramp: bool) -> PhysWorld {
    let mut w = ground();
    let floor = 50.0;
    w.add_ref(&at(1, [-100.0, 0.0, floor / 2.0]), &[object(boxed([200.0, 200.0, floor / 2.0]), Motion::Static)]);
    let vertices = vec![[100.0, -200.0, floor], [100.0, 200.0, floor], [100.0, 200.0, floor + lip], [100.0, -200.0, floor + lip]];
    w.add_ref(&at(2, [0.0, 0.0, 0.0]), &[object(Shape::TriMesh { vertices, triangles: vec![[0, 1, 2], [0, 2, 3]], materials: vec![0; 2] }, Motion::Static)]);
    for (id, y) in [(3, 55.0), (4, -55.0)] {
        w.add_ref(&at(id, [100.0, y, floor + 75.0]), &[object(boxed([20.0, 10.0, 75.0]), Motion::Static)]);
    }
    if ramp {
        let (s, c) = 24f32.to_radians().sin_cos();
        // Rotation about y tilting +x downwards; the top face's west end at (115, 0, 68).
        let rot = [c, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, c];
        let half = [100.0, 40.0, 3.0];
        let top_west = [-c * half[0] + s * half[2], s * half[0] + c * half[2]];
        let pos = [115.0 - top_west[0], 0.0, floor + 18.0 - top_west[1]];
        w.add_ref(&RefPose { form_id: 5, position: pos, rotation: rot, scale: 1.0 }, &[object(boxed(half), Motion::Static)]);
    }
    w
}

/// [design/code refs: `nvf-play::phys::npc_params`] The character parameter sets that
/// walk through doors: the player's, an actor's (with the shared actor slope limit)
/// and two small creatures' (`npc_params` for actors drawn 30 and 60 tall: as tall as drawn, radius
/// 0.17 of the height but at least 10, `step_width` at most the radius).
fn door_walkers() -> Vec<(&'static str, CharacterParams)> {
    let player = CharacterParams::default();
    let npc = CharacterParams { max_slope_deg: fnv_phys::character::ACTOR_MAX_SLOPE_DEG, ..player };
    let small = |height: f32| {
        let radius = (0.17 * height).clamp(10.0, npc.radius);
        CharacterParams { radius, height, step_width: npc.step_width.min(radius), ..npc }
    };
    vec![("player", player), ("npc", npc), ("creature 30", small(30.0)), ("creature 60", small(60.0))]
}

#[test]
fn character_walks_out_over_a_threshold_lip() {
    // Issue #2494: walking out of a caravan the cone bottom rests on the lip's edge, which
    // has no top, and the step-up found no walkable top to climb onto (the floor under the
    // axis more than 4 below the perched feet, the gap or the ramp ahead), so the player
    // stood on the threshold until a jump. Actors and creatures walk with the same
    // controller.
    for (who, p) in door_walkers() {
        for ramp in [false, true] {
            for speed in [115.5f32, 344.0] {
                // Straight out, and 10 degrees off into the door jamb's corner with the lip.
                for (y, deg) in [(0.0f32, 0.0f32), (10.0, 10.0), (-10.0, -10.0)] {
                    let mut w = caravan_door(10.3, ramp);
                    let mut c = w.add_character(1, p, [0.0, y, 51.0]);
                    settle(&mut w, &mut c, 30);
                    let (s, co) = deg.to_radians().sin_cos();
                    let steps = walk(&mut w, &mut c, [speed * co, speed * s], 360);
                    let peak = steps.iter().map(|s| s.feet[2]).fold(f32::MIN, f32::max);
                    let case = format!("{who}, ramp {ramp}, speed {speed}, {deg} degrees");
                    assert!(c.feet[0] > 200.0 && c.feet[2] < 10.0, "{case}: {:?}, peak {peak}", c.feet);
                    // Over the lip (or onto the ramp's upper end, 18 above the floor), not
                    // lifted above it.
                    let top = if ramp { 18.0 } else { 10.3 };
                    assert!(peak < 50.0 + top + 2.0, "{case}: peak {peak}");
                }
            }
        }
    }
}

#[test]
fn thin_walls_are_stepped_over_up_to_the_step_height() {
    // [design, #2494, #2511] The crest rule climbs an edge with no top up to
    // `step_height` (24) above the walkable surface the character stood on: a
    // zero-thickness wall 24 high is stepped over; 25, 26 and 28 stay walls, also when a
    // slow approach first perches the cone low on the wall's edge (the ground beyond then
    // counts as a walkable top 19 ahead) and later steps start from those raised feet.
    for (who, p) in door_walkers() {
        // [design, #2511] Terrain and a static floor exercise both support clearances.
        for floor in [0.0f32, 50.0] {
            for height in [24.0f32, 25.0, 26.0, 28.0] {
                for speed in [30.0f32, 115.5, 344.0] {
                    let mut w = ground();
                    if floor > 0.0 {
                        w.add_ref(&at(2, [0.0, 0.0, floor / 2.0]), &[object(boxed([400.0, 400.0, floor / 2.0]), Motion::Static)]);
                    }
                    let vertices = vec![[100.0, -200.0, floor], [100.0, 200.0, floor], [100.0, 200.0, floor + height], [100.0, -200.0, floor + height]];
                    w.add_ref(&at(1, [0.0, 0.0, 0.0]), &[object(Shape::TriMesh { vertices, triangles: vec![[0, 1, 2], [0, 2, 3]], materials: vec![0; 2] }, Motion::Static)]);
                    let mut c = w.add_character(1, p, [40.0, 0.0, floor + 1.0]);
                    settle(&mut w, &mut c, 30);
                    let steps = walk(&mut w, &mut c, [speed, 0.0], 360);
                    let peak = steps.iter().map(|s| s.feet[2] - floor).fold(f32::MIN, f32::max);
                    let case = format!("{who}, floor {floor}, wall {height}, speed {speed}");
                    if height <= p.step_height {
                        assert!(c.feet[0] > 130.0, "{case}: stepped over, {:?}, peak {peak}", c.feet);
                    } else {
                        // [design, #2511] A low perch is allowed; climbing the wall is not.
                        assert!(c.feet[0] < 100.0 && peak < TERRAIN_SHELL + 8.0, "{case}: {:?}, peak {peak}", c.feet);
                    }
                }
            }
        }
    }
}

/// A static floor with its top at z = 50 and a zero-thickness wall `height` high rising
/// from it at x = 100.
fn floor_and_thin_wall(height: f32) -> PhysWorld {
    let mut w = ground();
    w.add_ref(&at(1, [0.0, 0.0, 25.0]), &[object(boxed([400.0, 400.0, 25.0]), Motion::Static)]);
    let vertices = vec![[100.0, -400.0, 50.0], [100.0, 400.0, 50.0], [100.0, 400.0, 50.0 + height], [100.0, -400.0, 50.0 + height]];
    w.add_ref(&at(2, [0.0, 0.0, 0.0]), &[object(Shape::TriMesh { vertices, triangles: vec![[0, 1, 2], [0, 2, 3]], materials: vec![0; 2] }, Motion::Static)]);
    w
}

#[test]
fn a_thin_wall_too_high_blocks_after_a_placement_or_a_drop_onto_its_edge() {
    // Issue #2494, second review: with no run of step-ups behind it (placed there, or
    // dropped from a higher support), a cone resting low on a 26-unit thin wall's edge
    // must count from the floor under it, not from its raised feet, or retries from ever
    // higher feet climb the wall.
    let p = CharacterParams::default();
    // Where the cone rests on the edge (76): axis 17.8 short of it, feet 2.9 above the floor.
    let (x, perch) = (100.0 - 17.8, 50.0 + 26.0 - p.bottom_slope * (17.8 - p.bottom_flat));
    for speed in [30.0f32, 115.5, 344.0] {
        // Placed just above the perch.
        let mut w = floor_and_thin_wall(26.0);
        let mut c = w.add_character(1, p, [x, 0.0, perch + 0.5]);
        walk(&mut w, &mut c, [0.0, 0.0], 5);
        assert!(c.feet[2] > 50.0 + 1.5, "speed {speed}: perched after the placement, {:?}", c.feet);
        let steps = walk(&mut w, &mut c, [speed, 0.0], 360);
        let peak = steps.iter().map(|s| s.feet[2]).fold(f32::MIN, f32::max);
        assert!(c.feet[0] < 100.0 && peak < 50.0 + 8.0, "placed, speed {speed}: {:?}, peak {peak}", c.feet);

        // Walked off a platform 30 above the floor (y > 30) and dropped onto the perch.
        let mut w = floor_and_thin_wall(26.0);
        w.add_ref(&at(3, [40.0, 115.0, 65.0]), &[object(boxed([55.0, 85.0, 15.0]), Motion::Static)]);
        let mut c = w.add_character(1, p, [x, 50.0, 81.0]);
        settle(&mut w, &mut c, 30);
        walk(&mut w, &mut c, [0.0, -115.5], 40);
        walk(&mut w, &mut c, [0.0, 0.0], 30);
        assert!(c.feet[1] < 30.0 - p.radius && c.feet[2] > 50.0 + 1.5 && c.feet[2] < 50.0 + 8.0, "speed {speed}: perched after the drop, {:?}", c.feet);
        let steps = walk(&mut w, &mut c, [speed, 0.0], 360);
        let peak = steps.iter().map(|s| s.feet[2]).fold(f32::MIN, f32::max);
        assert!(c.feet[0] < 100.0 && peak < 50.0 + 8.0, "dropped, speed {speed}: {:?}, peak {peak}", c.feet);
    }
}

#[test]
fn a_jump_onto_a_high_edge_steps_on_over_it() {
    // Issue #2494, second review: a character that jumped from the ground onto a
    // threshold's lip 60 above it (the caravan door from outside), resting on the lip's
    // edge with its cone 4-16 below the top, walks on over it into the caravan: a landing
    // ends any run of step-up perches, and the limit then counts from where it rests (no
    // ground within a step under it), not from the ground it jumped from.
    let p = CharacterParams::default();
    for ((x0, vx, hold), speed) in [(125.0f32, 60.0f32, 15usize), (130.0, 60.0, 20), (140.0, 100.0, 15), (125.0, 60.0, 20)].into_iter().flat_map(|j| [(j, 115.5f32), (j, 344.0)]) {
        let mut w = caravan_door(10.3, false);
        let mut c = w.add_character(1, p, [x0, 0.0, 4.0]);
        settle(&mut w, &mut c, 30);
        w.move_character(&mut c, CharacterInput { velocity: [-vx, 0.0], jump: true, ..Default::default() });
        w.step();
        walk(&mut w, &mut c, [-vx, 0.0], hold);
        walk(&mut w, &mut c, [0.0, 0.0], 60);
        let case = format!("jump from {x0} at {vx} for {hold}, speed {speed}");
        assert!(c.feet[0] > 100.0 && c.feet[2] > 40.0 && c.feet[2] < 60.3 && c.grounded, "{case}: resting on the lip's edge after the jump, {:?}", c.feet);
        // About 170 units on: inside, on the floor (which ends 400 in).
        walk(&mut w, &mut c, [-speed, 0.0], (170.0 / speed * 60.0) as usize);
        assert!(c.feet[0] < 60.0 && (c.feet[2] - 50.7).abs() < 1.0, "{case}: over the lip onto the floor, {:?}", c.feet);
    }
}

#[test]
fn character_climbs_steep_stairs_without_jumping() {
    // Catwalk stairs like the REPCONN Facility's flights up to the intercom (RocketLabMid,
    // owner's playtest 2026-10-23): risers and treads of 24. Walking or running up them
    // from any start the cone bottom comes to rest on some step's edge, where the step-up
    // had found less than its minimum rise and the player stuck until a jump.
    for speed in [115.5f32, 150.0, 344.0] {
        for start in (0..8).map(|i| i as f32 * 3.0) {
            let mut w = ground();
            for i in 0..8 {
                // Step i from x = 100 + 24 i to the far end of the top floor (x = 1200).
                let (top, west) = (24.0 * (i + 1) as f32, 100.0 + 24.0 * i as f32);
                w.add_ref(&at(10 + i, [(west + 1200.0) / 2.0, 0.0, top / 2.0]), &[object(boxed([(1200.0 - west) / 2.0, 200.0, top / 2.0]), Motion::Static)]);
            }
            let mut c = w.add_character(1, CharacterParams::default(), [start, 0.0, 1.0]);
            settle(&mut w, &mut c, 30);
            let steps = walk(&mut w, &mut c, [speed, 0.0], 240);
            // On the top floor (x > 292, z 192) once past the stairs.
            let past = steps.iter().find(|s| s.feet[0] > 300.0).map(|s| s.feet);
            assert!(past.is_some_and(|f| (f[2] - 192.0).abs() < 2.0), "speed {speed}, start {start}: {past:?}, end {:?}", c.feet);
            assert!(steps.iter().all(|s| s.landed_speed.is_none_or(|v| v < 100.0)), "speed {speed}, start {start}: no fall onto the steps");
        }
    }
}

/// A ramp rising eastwards at `deg` degrees from x = 100, as a rotated box.
fn ramp(w: &mut PhysWorld, deg: f32) {
    let a = deg.to_radians();
    let (s, c) = a.sin_cos();
    // Rotation about y by -a tilts +x upwards.
    let rot = [c, 0.0, -s, 0.0, 1.0, 0.0, s, 0.0, c];
    let half = [400.0, 200.0, 10.0];
    // Place the top face's west edge at (100, 0, 0).
    let top_west = [-half[0], 0.0, half[2]];
    let rotated = [rot[0] * top_west[0] + rot[2] * top_west[2], 0.0, rot[6] * top_west[0] + rot[8] * top_west[2]];
    let pos = [100.0 - rotated[0], 0.0, -rotated[2]];
    w.add_ref(&RefPose { form_id: 50, position: pos, rotation: rot, scale: 1.0 }, &[object(boxed(half), Motion::Static)]);
}

#[test]
fn character_respects_slope_limit() {
    for (deg, climbs) in [(25.0f32, true), (60.0, false)] {
        let mut w = ground();
        ramp(&mut w, deg);
        let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
        settle(&mut w, &mut c, 30);
        walk(&mut w, &mut c, [150.0, 0.0], 120);
        let up = c.feet[2];
        if climbs {
            assert!(up > 50.0, "{deg}°: {:?}", c.feet);
        } else {
            assert!(up < c.params.step_height + 2.0, "{deg}°: {:?}", c.feet);
        }
    }
}

#[test]
fn actors_walk_navmesh_grade_slopes_below_the_cone_face_angle() {
    // [design, #2511] Synthetic ramps cover the 46–49-degree range cited in
    // docs/design/navmesh.md §5, with the same parameters as the thin-wall tests.
    for (who, p) in door_walkers().into_iter().filter(|(who, _)| *who != "player") {
        let margin = p.bottom_slope.atan().to_degrees() - p.max_slope_deg;
        assert!(margin > 1.0 && margin < 3.0, "{who}: cone-face margin {margin}");
        for deg in [46.0f32, 47.0, 48.0, 49.0] {
            for speed in [115.5f32, 344.0] {
                let mut w = ground();
                ramp(&mut w, deg);
                let mut c = w.add_character(1, p, [0.0, 0.0, 1.0]);
                settle(&mut w, &mut c, 30);
                walk(&mut w, &mut c, [speed, 0.0], 120);
                assert!(c.feet[0] > 150.0 && c.feet[2] > 50.0,
                    "{who}, {deg} degrees, speed {speed}: {:?}", c.feet);
            }
        }
    }
}

#[test]
fn character_jump_apex() {
    let mut w = ground();
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    let z0 = c.feet[2];
    let mut apex = z0;
    let mut grounded_again = false;
    for i in 0..120 {
        let s = w.move_character(&mut c, CharacterInput { velocity: [0.0, 0.0], jump: i == 0, ..Default::default() });
        w.step();
        apex = apex.max(s.feet[2]);
        if i > 5 && s.grounded {
            grounded_again = true;
            break;
        }
    }
    assert!(grounded_again);
    // Gravity is exact over a step (`character::gravity_step`): the highest step's end is
    // within half a step's drop of the analytic apex, g·dt²/8 (0.024 at 60 Hz).
    let g = w.params.gravity;
    let dt = w.params.dt;
    let rise = apex - z0;
    let h = c.params.jump_height;
    assert!(rise <= h + 0.01 && h - rise <= g * dt * dt / 8.0 + 0.01, "apex {rise}");
}

/// The physics rates the game offers (`nvf_play::phys_rate`, at most 160), and 152 (the
/// owner's variable-refresh cap at 160 Hz). Nothing above 160 is offered or tested.
const RATES: [f32; 5] = [60.0, 120.0, 144.0, 152.0, 160.0];

/// Steps of `hz` a second in `secs` seconds.
fn steps_in(secs: f32, hz: f32) -> usize {
    (secs * hz).round() as usize
}

/// The character's rules at each physics rate, in seconds: what changes with the step's
/// length, and by how much (docs/spec-notes/physics.md §5, "The physics rate"). Run with
/// `--nocapture` for the table.
#[test]
fn character_rules_hold_at_every_physics_rate() {
    let p = CharacterParams::default();
    let mut rows = Vec::new();
    for hz in RATES {
        let dt = 1.0 / hz;
        let rate = |w: &mut PhysWorld| w.set_step(dt);
        // Rest height on the terrain and on a static box (physics.md §6.2).
        let mut w = ground();
        rate(&mut w);
        w.add_ref(&at(1, [300.0, 0.0, 10.0]), &[object(boxed([50.0, 50.0, 10.0]), Motion::Static)]);
        let mut c = w.add_character(1, p, [0.0, 0.0, 30.0]);
        settle(&mut w, &mut c, steps_in(1.0, hz));
        let rest_terrain = c.feet[2];
        let mut b = w.add_character(2, p, [300.0, 0.0, 40.0]);
        settle(&mut w, &mut b, steps_in(1.0, hz));
        let rest_box = b.feet[2] - 20.0;
        // Jump apex on flat ground.
        let z0 = c.feet[2];
        let mut apex = z0;
        for i in 0..steps_in(2.0, hz) {
            let s = w.move_character(&mut c, CharacterInput { jump: i == 0, ..Default::default() });
            w.step();
            apex = apex.max(s.feet[2]);
            if i > 5 && s.grounded {
                break;
            }
        }
        let apex = apex - z0;
        // A running jump on flat ground: the time in the air and the touchdown speed.
        let mut w = ground();
        rate(&mut w);
        let mut r = w.add_character(1, p, [0.0, 0.0, 30.0]);
        settle(&mut w, &mut r, steps_in(1.0, hz));
        walk(&mut w, &mut r, [344.0, 0.0], steps_in(0.5, hz));
        let (mut run_landed, mut run_air) = (0.0, 0.0);
        for i in 0..steps_in(2.0, hz) {
            let s = w.move_character(&mut r, CharacterInput { velocity: [344.0, 0.0], jump: i == 0, ..Default::default() });
            w.step();
            if let Some(v) = s.landed_speed {
                (run_landed, run_air) = (v, (i + 1) as f32 * dt);
                break;
            }
        }
        // A fall of 300 units: the landing speed and the time it took.
        let mut w = ground();
        rate(&mut w);
        let mut f = w.add_character(1, p, [0.0, 0.0, 300.0 + TERRAIN_SHELL]);
        let (mut landed, mut fall_s) = (0.0, 0.0);
        for i in 0..steps_in(2.0, hz) {
            let s = w.move_character(&mut f, CharacterInput::default());
            w.step();
            if let Some(v) = s.landed_speed {
                (landed, fall_s) = (v, (i + 1) as f32 * dt);
                break;
            }
        }
        // Up a step of 0.6 step heights, and up the REPCONN stairs (risers of 24) running.
        let mut w = ground();
        rate(&mut w);
        let height = p.step_height * 0.6;
        w.add_ref(&at(1, [300.0, 0.0, height / 2.0]), &[object(boxed([200.0, 200.0, height / 2.0]), Motion::Static)]);
        let mut s = w.add_character(1, p, [0.0, 0.0, 1.0]);
        settle(&mut w, &mut s, steps_in(0.5, hz));
        walk(&mut w, &mut s, [150.0, 0.0], steps_in(1.5, hz));
        let step_up = s.feet[0] > 150.0 && (s.feet[2] - height).abs() < 2.0;
        let mut w = ground();
        rate(&mut w);
        for i in 0..8 {
            let (top, west) = (24.0 * (i + 1) as f32, 100.0 + 24.0 * i as f32);
            w.add_ref(&at(10 + i, [(west + 1200.0) / 2.0, 0.0, top / 2.0]), &[object(boxed([(1200.0 - west) / 2.0, 200.0, top / 2.0]), Motion::Static)]);
        }
        let mut s = w.add_character(1, p, [0.0, 0.0, 1.0]);
        settle(&mut w, &mut s, steps_in(0.5, hz));
        let climbed = walk(&mut w, &mut s, [344.0, 0.0], steps_in(4.0, hz)).iter().find(|s| s.feet[0] > 300.0).is_some_and(|s| (s.feet[2] - 192.0).abs() < 2.0);
        // A walkable ramp climbed, a steep one not; how far a 45° one lets a walker slide.
        let mut slopes = Vec::new();
        for deg in [25.0f32, 45.0, 60.0] {
            let mut w = ground();
            rate(&mut w);
            ramp(&mut w, deg);
            let mut s = w.add_character(1, p, [0.0, 0.0, 1.0]);
            settle(&mut w, &mut s, steps_in(0.5, hz));
            walk(&mut w, &mut s, [150.0, 0.0], steps_in(2.0, hz));
            slopes.push(s.feet[2]);
        }
        // A 12-unit box pushed for a second at 120 units/s.
        let mut w = ground();
        rate(&mut w);
        w.add_ref(&at(1, [60.0, 0.0, 6.0]), &[object(boxed([6.0, 6.0, 6.0]), Motion::Dynamic)]);
        let mut s = w.add_character(1, p, [0.0, 0.0, 1.0]);
        settle(&mut w, &mut s, steps_in(0.5, hz));
        walk(&mut w, &mut s, [120.0, 0.0], steps_in(1.0, hz));
        let pushed = w.ref_bodies(1)[0].translation[0] - 60.0;
        // A walk of a second along flat ground: no step lost (physics.md §6.4).
        let mut w = ground();
        rate(&mut w);
        let mut s = w.add_character(1, p, [0.0, 0.0, 1.0]);
        settle(&mut w, &mut s, steps_in(0.5, hz));
        let x0 = s.feet[0];
        walk(&mut w, &mut s, [344.0, 0.0], steps_in(1.0, hz));
        let walked = s.feet[0] - x0;
        rows.push((hz, rest_terrain, rest_box, apex, run_air, run_landed, landed, fall_s, step_up, climbed, slopes.clone(), pushed, walked));
        // What must hold at every rate.
        assert!((rest_terrain - TERRAIN_SHELL).abs() < 0.05, "{hz} Hz: rest on the terrain {rest_terrain}");
        assert!(rest_box.abs() < 1.5, "{hz} Hz: rest on a box {rest_box} above it");
        // Gravity is exact over a step: the apex within half a step's drop of the jump
        // height (g·dt²/8), a landing at the take-off speed after 2·v0/g, a 300-unit fall's
        // touchdown at sqrt(2·g·300) after sqrt(2·300/g), each landing seen on the step
        // that reaches the ground.
        let g = w.params.gravity;
        let v0 = (2.0 * g * p.jump_height).sqrt();
        assert!(apex <= p.jump_height + 0.01 && p.jump_height - apex <= g * dt * dt / 8.0 + 0.01, "{hz} Hz: apex {apex}");
        assert!((run_landed - v0).abs() < 0.5, "{hz} Hz: running jump landed at {run_landed}");
        assert!(run_air >= 2.0 * v0 / g - 0.01 && run_air < 2.0 * v0 / g + dt, "{hz} Hz: running jump {run_air} s in the air");
        let want = (2.0 * g * 300.0f32).sqrt();
        assert!((landed - want).abs() < 0.5, "{hz} Hz: landed at {landed}");
        let want_s = (2.0 * 300.0 / g).sqrt();
        assert!(fall_s >= want_s - 0.01 && fall_s < want_s + dt, "{hz} Hz: fell 300 in {fall_s} s");
        assert!(step_up && climbed, "{hz} Hz: step up {step_up}, stairs {climbed}");
        assert!(slopes[0] > 50.0 && slopes[2] < p.step_height + 2.0, "{hz} Hz: slopes {slopes:?}");
        assert!(pushed > 2.0, "{hz} Hz: pushed {pushed}");
        assert!((walked - 344.0).abs() < 4.0, "{hz} Hz: walked {walked}");
    }
    eprintln!("hz | rest terrain | rest box | apex | run air s | run landed | landed | fall s | step up | stairs | ramp 25/45/60 z | pushed | walked 1 s");
    for (hz, rt, rb, apex, run_air, run_landed, landed, fall, up, stairs, slopes, pushed, walked) in rows {
        eprintln!("{hz} | {rt:.3} | {rb:.3} | {apex:.2} | {run_air:.4} | {run_landed:.1} | {landed:.1} | {fall:.4} | {up} | {stairs} | {:.1}/{:.1}/{:.1} | {pushed:.2} | {walked:.2}", slopes[0], slopes[1], slopes[2]);
    }
}

#[test]
fn a_reference_made_in_the_terrain_shell_is_lifted_onto_it_and_rests() {
    // A box made with its bottom 1.8 units over the bare terrain, inside the terrain's
    // 3.5-unit shell, as an ash pile made at a standing actor's feet was.
    let mut w = ground();
    w.add_ref(&at(7, [100.0, 100.0, 6.8]), &[object(boxed([5.0, 5.0, 5.0]), Motion::Dynamic)]);
    let low = w.ref_lowest_z(7).expect("colliders");
    assert!((low - 1.8).abs() < 0.05, "lowest {low}");
    assert_eq!(w.lift_ref(7, TERRAIN_SHELL - low), 1);
    let lifted = w.ref_lowest_z(7).unwrap();
    assert!((lifted - TERRAIN_SHELL).abs() < 0.05, "lowest after the lift {lifted}");
    // On the shell it falls asleep where it is.
    for _ in 0..180 {
        w.step();
    }
    let b = w.ref_bodies(7)[0];
    assert!(b.sleeping && (b.translation[2] - (TERRAIN_SHELL + 5.0)).abs() < 0.5, "{b:?}");
    assert_eq!(w.lift_ref(0x77, 1.0), 0, "no such reference");
}

/// A body moving at `speed` units/s along x from `x0` (game units), stepped for 0.25 s of
/// game time in steps of `dt`: the share of its way it went.
fn share_moved(x0: f32, speed: f32, dt: f32) -> f32 {
    let mut w = PhysWorld::new(PhysParams::default());
    w.set_step(dt);
    w.add_ref(&at(0x99, [x0, 4800.0, 9000.0]), &[object(boxed([3.0, 3.0, 3.0]), Motion::Dynamic)]);
    w.step();
    w.set_ref_velocity(0x99, 0, speed);
    let b0 = w.ref_bodies(0x99)[0];
    let n = (0.25 / dt).round() as usize;
    for _ in 0..n {
        w.step();
    }
    (w.ref_bodies(0x99)[0].translation[0] - b0.translation[0]) / (speed * n as f32 * dt)
}

/// rapier keeps positions in f32 metres from the worldspace's origin: a step too short
/// moves a slow body by less than its position can show far from it, and the body stands
/// still. The game's shortest step (`nvf_play::phys::SHORTEST_STEP`, 1/240 s, also under
/// a slowed clock) still moves slow bodies at Goodsprings; steps of 1/3000 s (a 2 % kill
/// camera at 60 Hz before it) did not move them at all [observed: this test].
#[test]
fn slow_bodies_far_from_the_origin_move_at_the_shortest_step() {
    for speed in [5.0f32, 10.0, 20.0, 50.0] {
        let share = share_moved(-68000.0, speed, 1.0 / 240.0);
        assert!((0.7..1.3).contains(&share), "{speed} units/s at 1/240 s: {share}");
    }
    assert_eq!(share_moved(-68000.0, 10.0, 1.0 / 3000.0), 0.0, "1/3000 s steps leave a body at 10 units/s where it is");
}

#[test]
#[ignore]
fn probe_slow_steps_far_from_the_origin() {
    for (x0, label) in [(-68000.0f32, "goodsprings"), (-150000.0, "far")] {
        for dt in [1.0 / 60.0f32, 1.0 / 240.0, 1.0 / 500.0, 1.0 / 1000.0, 1.0 / 3000.0] {
            let mut line = format!("{label} dt 1/{:.0}:", 1.0 / dt);
            for speed in [5.0f32, 10.0, 20.0, 50.0, 100.0, 200.0] {
                let mut w = PhysWorld::new(PhysParams::default());
                w.set_step(dt);
                w.add_ref(&at(0x99, [x0, 4800.0, 9000.0]), &[object(boxed([3.0, 3.0, 3.0]), Motion::Dynamic)]);
                w.step();
                w.set_ref_velocity(0x99, 0, speed);
                let b0 = w.ref_bodies(0x99)[0];
                let n = (0.25 / dt).round() as usize;
                for _ in 0..n {
                    w.step();
                }
                let b = w.ref_bodies(0x99)[0];
                line += &format!(" {speed}u/s {:.0}%", (b.translation[0] - b0.translation[0]) / (speed * n as f32 * dt) * 100.0);
            }
            eprintln!("{line}");
        }
    }
}

#[test]
fn character_ceiling_stops_jump() {
    let mut w = ground();
    // A slab 20 units above the head.
    let top = CharacterParams::default().height + 20.0;
    w.add_ref(&at(1, [0.0, 0.0, top + 5.0]), &[object(boxed([200.0, 200.0, 5.0]), Motion::Static)]);
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 10);
    let mut apex: f32 = 0.0;
    for i in 0..60 {
        let s = w.move_character(&mut c, CharacterInput { velocity: [0.0, 0.0], jump: i == 0, ..Default::default() });
        w.step();
        apex = apex.max(s.feet[2]);
    }
    assert!(apex < 21.0, "head went through the ceiling: {apex}");
    assert!(c.grounded);
}

#[test]
fn character_slides_along_walls() {
    let mut w = ground();
    // A wall along y = 50 (south face at 50), long in x.
    w.add_ref(&at(1, [0.0, 60.0, 100.0]), &[object(boxed([500.0, 10.0, 100.0]), Motion::Static)]);
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    walk(&mut w, &mut c, [100.0, 100.0], 120);
    assert!(c.feet[1] <= 50.0 - c.params.radius + 0.01, "{:?}", c.feet);
    assert!(c.feet[0] > 100.0, "kept moving along the wall: {:?}", c.feet);
}

#[test]
fn character_pushes_dynamic_clutter() {
    let mut w = ground();
    w.add_ref(&at(1, [60.0, 0.0, 6.0]), &[object(boxed([6.0, 6.0, 6.0]), Motion::Dynamic)]);
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    let steps = walk(&mut w, &mut c, [120.0, 0.0], 60);
    assert!(steps.iter().any(|s| s.touched.iter().any(|o| o.key() == Key::Ref(1))));
    assert!(w.ref_bodies(1)[0].translation[0] > 62.0, "{:?}", w.ref_bodies(1));
}

/// A car-sized body of a car's Havok mass does not give way to a character running
/// into it (playtest list 7 item 37, physics.md §6.10); the character stops at it.
#[test]
fn character_does_not_push_heavy_bodies() {
    let mut w = ground();
    let car = CollisionObject { mass: 600.0, ..object(boxed([60.0, 120.0, 30.0]), Motion::Dynamic) };
    w.add_ref(&at(1, [100.0, 0.0, 30.5]), &[car]);
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    let before = w.ref_bodies(1)[0].translation;
    walk(&mut w, &mut c, [400.0, 0.0], 120);
    let after = w.ref_bodies(1)[0].translation;
    assert!((after[0] - before[0]).abs() < 0.5 && (after[1] - before[1]).abs() < 0.5, "{before:?} -> {after:?}");
    assert!(c.feet[0] < 40.0 - c.params.radius + 1.0, "stopped at the body: {:?}", c.feet);
}

/// Asleep and resting a little into the ground (as placed cars are), a heavy body is not
/// woken by a character running into it (it would rise out of the ground and slide).
#[test]
fn character_does_not_wake_a_heavy_body() {
    let mut w = ground();
    let car = CollisionObject { mass: 600.0, ..object(boxed([60.0, 120.0, 30.0]), Motion::Dynamic) };
    w.insert_ref(fnv_phys::PreparedRef::new(&PhysParams::default(), &at(1, [100.0, 0.0, 28.0]), &[car]).asleep());
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    walk(&mut w, &mut c, [400.0, 0.0], 120);
    let b = w.ref_bodies(1)[0];
    assert!(b.sleeping && (b.translation[2] - 28.0).abs() < 0.01, "{b:?}");
}

#[test]
fn activation_ray_skips_the_caster() {
    let mut w = ground();
    w.add_ref(&at(77, [100.0, 0.0, 100.0]), &[object(boxed([10.0; 3]), Motion::Static)]);
    let c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 0.0]);
    let eye = [0.0, 0.0, 100.0];
    let q = w.queries();
    let any = q.ray(eye, [1.0, 0.0, 0.0], 150.0, &Filter::default()).unwrap();
    assert_eq!(any.owner, Owner::Character { id: 1 }, "the eye is inside the caster's capsule");
    let hit = q.ray(eye, [1.0, 0.0, 0.0], 150.0, &Filter::excluding(c.key())).unwrap();
    assert_eq!(hit.owner.key(), Key::Ref(77));
    assert!((hit.distance - 90.0).abs() < 1e-3);
    assert!(q.ray(eye, [1.0, 0.0, 0.0], 80.0, &Filter::excluding(c.key())).is_none());
}

// --- actor hit volumes (combat C3) ---

fn person(feet: [f32; 3], dead: bool) -> HitVolume {
    HitVolume { feet, radius: 20.0, height: 128.0, dead }
}

#[test]
fn actor_volumes_take_shots() {
    let mut w = ground();
    w.set_actor_volume(0x10, person([200.0, 0.0, 0.0], false));
    w.update_queries();
    let shot = w.queries().ray([0.0, 0.0, 100.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default()).unwrap();
    assert_eq!(shot.owner, Owner::Actor { form_id: 0x10, layer: layers::BIPED, part: fnv_phys::actors::WHOLE });
    assert!((shot.distance - 180.0).abs() < 0.1, "{}", shot.distance);
    // Above the head: a miss.
    assert!(w.queries().ray([0.0, 0.0, 140.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default()).is_none());
    // Moved: the shot follows the volume.
    w.set_actor_volume(0x10, person([300.0, 0.0, 0.0], false));
    w.update_queries();
    let shot = w.queries().ray([0.0, 0.0, 100.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default()).unwrap();
    assert!((shot.distance - 280.0).abs() < 0.1);
    // Dead: DEAD_BIP, which a filter can skip.
    w.set_actor_volume(0x10, person([300.0, 0.0, 0.0], true));
    w.update_queries();
    let shot = w.queries().ray([0.0, 0.0, 100.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default()).unwrap();
    assert_eq!(shot.owner, Owner::Actor { form_id: 0x10, layer: layers::DEAD_BIP, part: fnv_phys::actors::WHOLE });
    let skip = Filter { exclude_layers: LayerSet::from_layers(&[layers::DEAD_BIP]), ..Default::default() };
    assert!(w.queries().ray([0.0, 0.0, 100.0], [1.0, 0.0, 0.0], 1000.0, &skip).is_none());
    assert_eq!(w.actor_volumes().collect::<Vec<_>>(), vec![0x10]);
    assert!(w.remove_actor_volume(0x10));
    w.update_queries();
    assert!(w.queries().ray([0.0, 0.0, 100.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default()).is_none());
}

#[test]
fn non_finite_hit_volumes_and_parts_are_left_out() {
    let mut w = ground();
    w.set_actor_volume(0x10, HitVolume { radius: f32::NAN, ..person([200.0, 0.0, 0.0], false) });
    assert_eq!(w.actor_volumes().count(), 0);
    let good = PartCapsule { part: 1, a: [200.0, 0.0, 40.0], b: [200.0, 0.0, 100.0], radius: 15.0 };
    let bad = PartCapsule { part: 2, a: [f32::NAN, 0.0, 40.0], ..good };
    w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &[good, bad]);
    assert_eq!(w.actor_part_count(0x10), 1);
    w.update_queries();
    let shot = w.queries().ray([0.0, 0.0, 70.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default()).unwrap();
    assert_eq!(shot.owner, Owner::Actor { form_id: 0x10, layer: layers::BIPED, part: 1 });
}

#[test]
fn a_filter_can_skip_every_actor() {
    // Shell casings see only the world: no hit volume, no character's capsule, yet the
    // ground under them.
    let mut w = ground();
    w.set_actor_volume(0x10, person([200.0, 0.0, 0.0], false));
    let c = w.add_character(1, CharacterParams::default(), [400.0, 0.0, 3.5]);
    w.update_queries();
    let along = |w: &mut PhysWorld, f: &Filter| w.queries().ray([0.0, 0.0, 40.0], [1.0, 0.0, 0.0], 1000.0, f).map(|h| h.owner.key());
    assert_eq!(along(&mut w, &Filter::default()), Some(Key::Actor(0x10)));
    assert_eq!(along(&mut w, &Filter { exclude_layers: layers::actor_volumes(), ..Default::default() }), Some(c.key()));
    let world_only = Filter { no_actors: true, ..Default::default() };
    assert_eq!(along(&mut w, &world_only), None);
    let down = w.queries().ray([200.0, 0.0, 100.0], [0.0, 0.0, -1.0], 200.0, &world_only).unwrap();
    assert!(matches!(down.owner, Owner::Terrain { .. }), "{:?}", down.owner);
}

#[test]
fn characters_walk_through_actor_volumes_and_clutter_ignores_them() {
    let mut w = ground();
    w.set_actor_volume(0x10, person([60.0, 0.0, 0.0], false));
    w.add_ref(&at(1, [60.0, 40.0, 6.0]), &[object(boxed([6.0, 6.0, 6.0]), Motion::Dynamic)]);
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    let steps = walk(&mut w, &mut c, [120.0, 0.0], 120);
    assert!(c.feet[0] > 200.0, "{:?}", c.feet);
    assert!(steps.iter().all(|s| s.touched.iter().all(|o| o.key() != Key::Actor(0x10))));
    // Moving the volume through the box does not push it.
    let before = w.ref_bodies(1)[0].translation;
    for i in 0..30 {
        w.set_actor_volume(0x10, person([60.0, i as f32 * 2.0, 0.0], false));
        w.step();
    }
    let after = w.ref_bodies(1)[0].translation;
    assert!((after[0] - before[0]).abs() < 0.01 && (after[1] - before[1]).abs() < 0.01, "{before:?} {after:?}");
}

#[test]
fn character_runs_are_deterministic() {
    let run = || {
        let mut w = scene();
        let mut c = w.add_character(1, CharacterParams::default(), [-50.0, -50.0, 5.0]);
        let mut trace = Vec::new();
        for i in 0..300u32 {
            let a = i as f32 * 0.05;
            let s = w.move_character(&mut c, CharacterInput { velocity: [120.0 * a.cos(), 120.0 * a.sin()], jump: i.is_multiple_of(97), ..Default::default() });
            w.step();
            trace.push((s.feet.map(f32::to_bits), w.state_digest()));
        }
        trace
    };
    assert_eq!(run(), run());
}

proptest! {
    #[test]
    fn owner_roundtrip(x in any::<i32>(), y in any::<i32>(), f in any::<u32>(), o in any::<u16>(), l in any::<u8>()) {
        for owner in [Owner::Terrain { x, y }, Owner::Ref { form_id: f, object: o, layer: l }, Owner::Character { id: f }, Owner::Actor { form_id: f, layer: l, part: o as u8 }] {
            prop_assert_eq!(Owner::decode(owner.encode()), Some(owner));
        }
    }

    #[test]
    fn terrain_ray_matches_analytic(seed in proptest::collection::vec(-40i32..40, SIDE * SIDE), px in 0.0f32..4096.0, py in 0.0f32..4096.0) {
        let heights: Vec<f32> = seed.iter().map(|&v| v as f32 * 8.0).collect();
        let mut w = PhysWorld::new(PhysParams::default());
        w.add_terrain((2, -5), &heights);
        let (x0, y0) = (2.0 * CELL_SIZE, -5.0 * CELL_SIZE);
        let hit = down(&mut w, x0 + px, y0 + py).expect("hit");
        let want = height_at(&heights, px, py).unwrap();
        prop_assert!((hit.point[2] - want).abs() < 0.05, "{} vs {}", hit.point[2], want);
    }
}

#[test]
fn bodies_added_then_queried_still_simulate() {
    // Adding a dynamic body and querying before the first step syncs the broad phase
    // through rapier's collision-only pipeline; the body must still fall afterwards.
    let mut w = ground();
    w.add_ref(&at(0x99, [100.0, 100.0, 500.0]), &[object(boxed([10.0, 10.0, 10.0]), Motion::Dynamic)]);
    let _ = down(&mut w, 0.0, 0.0);
    for _ in 0..30 {
        w.step();
    }
    let z = w.ref_bodies(0x99)[0].translation[2];
    assert!(z < 490.0, "the box fell: z = {z}");
}

#[test]
fn a_struck_dynamic_body_moves_and_a_fixed_one_does_not() {
    let mut w = ground();
    w.add_ref(&at(0x99, [100.0, 100.0, 10.0]), &[CollisionObject { mass: 2.0, ..object(boxed([5.0, 5.0, 10.0]), Motion::Dynamic) }]);
    w.add_ref(&at(0x9A, [300.0, 100.0, 10.0]), &[object(boxed([5.0, 5.0, 10.0]), Motion::Static)]);
    for _ in 0..60 {
        w.step();
    }
    let (before, v0) = (w.ref_bodies(0x99)[0].translation, w.ref_bodies(0x99)[0].linvel);
    // 400 of impulse on a mass of 2: 200 units per second along +x, struck on the line
    // through the body's centre.
    let gained = w.push_ref(0x99, [before[0] - 5.0, before[1], before[2]], [400.0, 0.0, 0.0], 1000.0).expect("a dynamic body");
    assert!((gained - 200.0).abs() < 1.0, "speed gained {gained}");
    let v = w.ref_bodies(0x99)[0].linvel;
    assert!((v[0] - v0[0] - 200.0).abs() < 1.0 && (v[1] - v0[1]).abs() < 1.0, "velocity {v0:?} -> {v:?}");
    for _ in 0..30 {
        w.step();
    }
    let after = w.ref_bodies(0x99)[0].translation;
    assert!(after[0] > before[0] + 20.0, "pushed along +x: {before:?} -> {after:?}");
    // The speed gained is bounded.
    assert_eq!(w.push_ref(0x99, after, [1.0e6, 0.0, 0.0], 300.0).map(f32::round), Some(300.0));
    // No dynamic body, no reference: nothing.
    assert_eq!(w.push_ref(0x9A, [300.0, 100.0, 10.0], [400.0, 0.0, 0.0], 1000.0), None);
    assert_eq!(w.push_ref(0x77, [0.0; 3], [400.0, 0.0, 0.0], 1000.0), None);
}

#[test]
fn set_velocity_sets_one_axis_of_a_dynamic_body() {
    let mut w = ground();
    w.add_ref(&at(0x99, [100.0, 100.0, 10.0]), &[object(boxed([5.0, 5.0, 10.0]), Motion::Dynamic)]);
    w.add_ref(&at(0x9A, [300.0, 100.0, 10.0]), &[object(boxed([5.0, 5.0, 10.0]), Motion::Static)]);
    for _ in 0..60 {
        w.step();
    }
    // The console's SetVel z 300: up at 300 units per second, x and y kept.
    let before = w.ref_bodies(0x99)[0];
    assert_eq!(w.set_ref_velocity(0x99, 2, 300.0), 1);
    let v = w.ref_bodies(0x99)[0].linvel;
    assert!((v[2] - 300.0).abs() < 0.5 && (v[0] - before.linvel[0]).abs() < 0.5, "velocity {v:?}");
    let _ = down(&mut w, 0.0, 0.0);
    for _ in 0..10 {
        w.step();
    }
    let z = w.ref_bodies(0x99)[0].translation[2];
    assert!(z > before.translation[2] + 20.0, "thrown up: {} -> {z}", before.translation[2]);
    // No dynamic body, no reference: nothing set.
    assert_eq!(w.set_ref_velocity(0x9A, 0, 100.0), 0);
    assert_eq!(w.set_ref_velocity(0x77, 0, 100.0), 0);
}

#[test]
fn a_sleeping_body_struck_then_queried_still_moves() {
    // The engine queries between a shot and the next step, with the broad phase out of
    // date (an actor's hit volume moved, a cell streamed in): the strike must survive it.
    // A bottle-sized box asleep on a shelf.
    let mut w = ground();
    w.add_ref(&at(0x9C, [100.0, 100.0, 10.0]), &[object(boxed([50.0, 50.0, 10.0]), Motion::Static)]);
    w.add_ref(&at(0x99, [100.0, 100.0, 34.5]), &[CollisionObject { mass: 1.0, ..object(boxed([3.0, 3.0, 14.0]), Motion::Dynamic) }]);
    for _ in 0..600 {
        w.step();
    }
    let before = w.ref_bodies(0x99)[0];
    assert!(before.sleeping, "the box came to rest: {before:?}");
    assert!(w.push_ref(0x99, before.translation, [200.0, 0.0, 0.0], 1000.0).is_some());
    w.add_ref(&at(0x9B, [600.0, 600.0, 10.0]), &[object(boxed([5.0, 5.0, 10.0]), Motion::Static)]);
    let _ = down(&mut w, 0.0, 0.0);
    for _ in 0..30 {
        w.step();
    }
    let after = w.ref_bodies(0x99)[0].translation;
    assert!(after[0] > before.translation[0] + 20.0, "pushed along +x: {:?} -> {after:?}", before.translation);
}

#[test]
fn snapshot_continues_identically() {
    let mut a = scene();
    let mut ca = a.add_character(1, CharacterParams::default(), [-50.0, -50.0, 5.0]);
    let input = |i: u32| CharacterInput { velocity: [100.0, 60.0 - i as f32 * 0.4], jump: i.is_multiple_of(53), ..Default::default() };
    for i in 0..90u32 {
        a.move_character(&mut ca, input(i));
        a.step();
    }
    // The clutter was simulated (the top box started at z = 97).
    assert!(a.ref_bodies(111)[0].translation[2] < 60.0, "{:?}", a.ref_bodies(111));
    let mut b = a.snapshot();
    let mut cb = ca.clone();
    for i in 90..300u32 {
        let sa = a.move_character(&mut ca, input(i));
        let sb = b.move_character(&mut cb, input(i));
        a.step();
        b.step();
        assert_eq!(sa, sb);
        assert_eq!(a.state_digest(), b.state_digest(), "step {i}");
    }
}

fn on_layer(shape: Shape, layer: u8) -> CollisionObject {
    CollisionObject { layer, ..object(shape, Motion::Static) }
}

#[test]
fn queries_skip_excluded_layers() {
    let mut w = ground();
    w.add_ref(&at(1, [0.0, 0.0, 50.0]), &[on_layer(boxed([20.0, 20.0, 20.0]), layers::TRIGGER)]);
    let hit = down(&mut w, 0.0, 0.0).unwrap();
    assert_eq!(hit.owner, Owner::Ref { form_id: 1, object: 0, layer: layers::TRIGGER });
    let f = Filter { exclude_layers: LayerSet::from_layers(&[layers::TRIGGER]), ..Default::default() };
    let hit = w.queries().ray([0.0, 0.0, 1_000.0], [0.0, 0.0, -1.0], 2_000.0, &f).unwrap();
    assert_eq!(hit.owner, Owner::Terrain { x: 0, y: 0 });
}

#[test]
fn character_passes_through_non_solid_layers() {
    for (layer, blocks) in [(layers::TRIGGER, false), (layers::STATIC, true), (layers::INVISIBLE_WALL, true)] {
        let mut w = ground();
        // A wall across the path, west face at x = 100.
        w.add_ref(&at(1, [110.0, 0.0, 100.0]), &[on_layer(boxed([10.0, 300.0, 100.0]), layer)]);
        let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
        settle(&mut w, &mut c, 30);
        walk(&mut w, &mut c, [150.0, 0.0], 90);
        let through = c.feet[0] > 200.0;
        assert_eq!(through, !blocks, "layer {layer} ({:?}): {:?}", layers::name(layer as u32), c.feet);
    }
}

#[test]
fn shots_pass_a_transparent_sheet_that_stops_blows_and_characters() {
    // A thin sheet (a chain-link panel) at x = 100 in front of a static wall at x = 300.
    for (layer, shot_passes) in [(layers::TRANSPARENT, true), (layers::TRANSPARENT_SMALL, false), (layers::TRANSPARENT_SMALL_ANIM, false), (layers::STATIC, false)] {
        let mut w = ground();
        w.add_ref(&at(1, [100.0, 0.0, 100.0]), &[on_layer(boxed([2.0, 300.0, 100.0]), layer)]);
        w.add_ref(&at(2, [310.0, 0.0, 100.0]), &[on_layer(boxed([10.0, 300.0, 100.0]), layers::STATIC)]);
        let shot = Filter { exclude_layers: layers::shot_ignores(), ..Default::default() };
        let hit = w.queries().ray([0.0, 0.0, 100.0], [1.0, 0.0, 0.0], 1_000.0, &shot).unwrap();
        let want = if shot_passes { Key::Ref(2) } else { Key::Ref(1) };
        assert_eq!(hit.owner.key(), want, "layer {layer} ({:?})", layers::name(layer as u32));
        // A melee swing's ray stops on the sheet whatever its layer: the Transparent
        // exception is for projectiles only.
        let blow = Filter { exclude_layers: layers::melee_ignores(), ..Default::default() };
        let hit = w.queries().ray([0.0, 0.0, 100.0], [1.0, 0.0, 0.0], 1_000.0, &blow).unwrap();
        assert_eq!(hit.owner.key(), Key::Ref(1), "melee, layer {layer} ({:?})", layers::name(layer as u32));
        // A character walking at it stops on the sheet whatever its layer.
        let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
        settle(&mut w, &mut c, 30);
        walk(&mut w, &mut c, [150.0, 0.0], 90);
        assert!(c.feet[0] < 100.0, "layer {layer}: the character got through to {:?}", c.feet);
    }
}

#[test]
fn collision_marker_primitives_become_boxes() {
    use fnv_phys::primitive::{primitive_object, BoundsMeaning, Primitive, PrimitiveKind};
    let p = Primitive { bounds: [100.0, 10.0, 200.0], kind: PrimitiveKind::PortalBox, layer: None };
    let o = primitive_object(&p, BoundsMeaning::HalfExtents).unwrap();
    let mut w = ground();
    // A reference turned 90° about z: the long x bound lies along world y.
    let r = w.add_ref(&RefPose { rotation: rz(std::f32::consts::FRAC_PI_2), ..at(0x21_0001, [500.0, 500.0, 150.0]) }, &[o]);
    assert_eq!((r.bodies, r.colliders, r.errors.len()), (1, 1, 0));
    assert!(down(&mut w, 500.0, 595.0).is_some_and(|h| h.owner.key() == Key::Ref(0x21_0001)));
    assert!(down(&mut w, 515.0, 500.0).is_some_and(|h| h.owner.key() == Key::Terrain(0, 0)));
}

#[test]
fn arrival_rests_on_the_support_shell() {
    let mut w = ground();
    // A static slab (top at z = 40) carrying a 0.7-unit shell, at x = 500.
    let mut slab = object(boxed([50.0, 50.0, 20.0]), Motion::Static);
    slab.shell = 0.7;
    w.add_ref(&at(0x10, [500.0, 0.0, 20.0]), &[slab]);
    let p = CharacterParams::default();
    let f = Filter { exclude_layers: layers::character_ignores(), ..Default::default() };
    // Below the terrain, on it, or above it: the feet end on the terrain's shell.
    for z in [-8.0, 0.0, 3.0, 60.0] {
        let a = w.arrival([100.0, 100.0, z], &p, &f);
        assert!((a.feet[2] - TERRAIN_SHELL).abs() < 1e-3, "from {z}: {:?}", a.feet);
        let s = a.support.expect("terrain under the point");
        assert!(matches!(s.owner, Owner::Terrain { .. }) && s.point[2].abs() < 1e-3 && s.shell == TERRAIN_SHELL);
        assert_eq!((a.feet[0], a.feet[1]), (100.0, 100.0));
    }
    // On the slab: its shell, from just under its top as from above.
    for z in [38.0, 40.0, 90.0] {
        let a = w.arrival([500.0, 0.0, z], &p, &f);
        assert!((a.feet[2] - 40.7).abs() < 1e-3, "from {z}: {:?}", a.feet);
        assert_eq!(a.support.unwrap().owner, Owner::Ref { form_id: 0x10, object: 0, layer: 1 });
    }
    // The shell scales with the reference.
    let mut slab = object(boxed([50.0, 50.0, 20.0]), Motion::Static);
    slab.shell = 0.7;
    w.add_ref(&RefPose { scale: 2.0, ..at(0x11, [-500.0, 0.0, 40.0]) }, &[slab]);
    let a = w.arrival([-500.0, 0.0, 80.0], &p, &f);
    assert!((a.feet[2] - 81.4).abs() < 1e-3, "{:?}", a.feet);
    // Nothing in range: the actor stays where it was put.
    let a = w.arrival([100.0, 100.0, 5_000.0], &p, &f);
    assert_eq!((a.feet, a.support), ([100.0, 100.0, 5_000.0], None));
}

/// Two static walls forming a notch that opens to the west, apex at (200, 0), faces at
/// `half_angle` degrees either side of the x axis.
fn notch(w: &mut PhysWorld, half_angle: f32) {
    let a = half_angle.to_radians();
    let (s, c) = a.sin_cos();
    for (id, sign) in [(60u32, 1.0f32), (61, -1.0)] {
        // Wall along the line from the apex towards (-c, sign·s); the notch's inside is
        // on the side of normal (-s, -sign·c).
        let along = [-c, sign * s];
        let out = [s, sign * c];
        let centre = [200.0 + along[0] * 100.0 + out[0] * 5.0, along[1] * 100.0 + out[1] * 5.0, 100.0];
        w.add_ref(&RefPose { rotation: rz(-sign * a), ..at(id, centre) }, &[object(boxed([100.0, 5.0, 100.0]), Motion::Static)]);
    }
}

#[test]
fn character_wedged_in_a_corner_stays_put() {
    // Regression: rapier's controller slid along one face of a concave corner at a time
    // and went back and forth (about 2 units per step against rock 0x000EDE82,
    // physics.md §6.1).
    for half_angle in [20.0f32, 30.0, 45.0] {
        let mut w = ground();
        notch(&mut w, half_angle);
        let mut c = w.add_character(1, CharacterParams::default(), [0.0, 3.0, 1.0]);
        settle(&mut w, &mut c, 30);
        let steps = walk(&mut w, &mut c, [300.0, 20.0], 180);
        let tail = &steps[120..];
        let path: f32 = tail.windows(2).map(|p| (p[1].feet[0] - p[0].feet[0]).hypot(p[1].feet[1] - p[0].feet[1])).sum();
        let first = tail[0].feet;
        let last = tail[tail.len() - 1].feet;
        assert!(last[0] > 100.0, "{half_angle}°: reached the notch: {last:?}");
        let net = (last[0] - first[0]).hypot(last[1] - first[1]);
        // Pressed into the corner it may still settle deeper, but never back and forth.
        assert!(path < 1.0 && path - net < 0.1, "{half_angle}°: path {path}, net {net} over the last 60 steps");
        assert!((last[2] - first[2]).abs() < 0.01, "{half_angle}°: crept from {} to {}", first[2], last[2]);
    }
}

#[test]
fn character_rests_on_contact_shells() {
    // Terrain (shell 3.5) and a static slab with a 0.7 shell whose top is at z = 40.
    let mut w = ground();
    let mut slab = object(boxed([60.0, 60.0, 10.0]), Motion::Static);
    slab.shell = 0.7;
    w.add_ref(&at(0x10, [300.0, 0.0, 30.0]), &[slab]);
    let p = CharacterParams::default();
    let f = Filter { exclude_layers: layers::character_ignores(), ..Default::default() };
    // Dropped from above, the controller ends where the arrival rule puts the feet.
    let mut c = w.add_character(1, p, [0.0, 0.0, 60.0]);
    let s = settle(&mut w, &mut c, 90);
    let a = w.arrival([0.0, 0.0, 60.0], &p, &Filter { exclude: vec![c.key()], ..f.clone() });
    assert!(s.grounded && (s.feet[2] - TERRAIN_SHELL).abs() < 0.02, "{:?}", s.feet);
    assert!((s.feet[2] - a.feet[2]).abs() < 0.02 && s.gap == TERRAIN_SHELL);
    // Standing still, it stays there.
    let s2 = settle(&mut w, &mut c, 60);
    assert!((0..3).all(|i| (s2.feet[i] - s.feet[i]).abs() < 0.01), "{:?} -> {:?}", s.feet, s2.feet);
    // Walking onto the slab (the edge is 40 - 3.5 high from the terrain's shell: too high
    // to step, so start on it) rests 0.7 above it; walking off onto the terrain, 3.5.
    w.place_character(&mut c, [300.0, 0.0, 41.0]);
    let s = settle(&mut w, &mut c, 30);
    assert!((s.feet[2] - 40.7).abs() < 0.02 && (s.gap - 0.7).abs() < 1e-6, "{:?}", s.feet);
    let steps = walk(&mut w, &mut c, [-200.0, 0.0], 120);
    let s = steps.last().unwrap();
    assert!(s.feet[0] < 200.0 && s.grounded, "{:?}", s.feet);
    assert!((s.feet[2] - TERRAIN_SHELL).abs() < 0.02 && s.gap == TERRAIN_SHELL, "{:?}", s.feet);
    // A low static floor flush with the terrain: from terrain onto it the feet sink to its
    // shell, and back on the terrain they are lifted again.
    let mut mat = object(boxed([60.0, 60.0, 0.5]), Motion::Static);
    mat.shell = 0.7;
    w.add_ref(&at(0x11, [-300.0, 0.0, 0.0]), &[mat]);
    let steps = walk(&mut w, &mut c, [-200.0, 0.0], 60);
    let on_mat = steps.iter().rfind(|s| s.feet[0] < -260.0 && s.feet[0] > -340.0).expect("crossed the mat");
    assert!((on_mat.feet[2] - 1.2).abs() < 0.05, "{:?}", on_mat.feet);
    let steps = walk(&mut w, &mut c, [-200.0, 0.0], 60);
    let s = steps.last().unwrap();
    assert!((s.feet[2] - TERRAIN_SHELL).abs() < 0.05, "{:?}", s.feet);
}

#[test]
fn walking_on_level_ground_loses_no_step() {
    // Regression: rapier dropped a step's horizontal motion now and then on exactly level
    // surfaces (degenerate slope tangent), here once in 60 steps along a cell seam.
    let mut w = ground();
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 300.0]);
    settle(&mut w, &mut c, 120);
    let mut prev = c.feet;
    for (i, s) in walk(&mut w, &mut c, [77.0, 0.0], 240).into_iter().enumerate() {
        let dx = s.feet[0] - prev[0];
        assert!((dx - 77.0 * w.params.dt).abs() < 1e-2, "step {i}: moved {dx}");
        prev = s.feet;
    }
}

// --- swimming (physics.md §6.6) ---

/// The flat ground of [`ground`] under water whose surface is 500 units above it.
fn lake() -> PhysWorld {
    let mut w = ground();
    for x in -1..=0 {
        for y in -1..=0 {
            w.set_water(WaterArea::Exterior(x, y), Some(500.0));
        }
    }
    w
}

fn swim(w: &mut PhysWorld, c: &mut Character, velocity: [f32; 2], vertical: f32, jump: bool, steps: usize) -> CharacterStep {
    let mut last = None;
    for i in 0..steps {
        last = Some(w.move_character(c, CharacterInput { velocity, jump: jump && i == 0, vertical, view: None }));
        w.step();
    }
    last.unwrap()
}

#[test]
fn a_fall_into_water_stops_and_floats() {
    let mut w = lake();
    let p = CharacterParams::default();
    let mut c = w.add_character(1, p, [0.0, 0.0, 500.0]);
    let mut landed = false;
    let mut entered_at = None;
    for i in 0..240 {
        let s = w.move_character(&mut c, CharacterInput::default());
        w.step();
        landed |= s.landed_speed.is_some();
        if s.swimming && entered_at.is_none() {
            entered_at = Some((i, s.feet[2]));
        }
    }
    let (_, z) = entered_at.expect("swims");
    // The fall stops in the step that takes the feet past the swimming depth.
    assert!(z <= 500.0 - p.swim_depth && z > 500.0 - p.swim_depth - 7.0, "{z}");
    assert!(!landed);
    assert!(c.swimming && !c.grounded);
    // Above the floating height it sinks to it and stays.
    assert!((c.feet[2] - (500.0 - p.float_depth)).abs() < 0.01 || c.feet[2] < 500.0 - p.float_depth, "{:?}", c.feet);
}

/// Flat ground at 0 under water at `surface` (the four cells round the origin).
fn pool(surface: f32) -> PhysWorld {
    let mut w = ground();
    for x in -1..=0 {
        for y in -1..=0 {
            w.set_water(WaterArea::Exterior(x, y), Some(surface));
        }
    }
    w
}

#[test]
fn walking_on_a_deep_bottom_swims_a_shallow_one_does_not() {
    // Water shallower than the wading depth: the character walks on the bottom.
    let p = CharacterParams::default();
    let mut w = pool(p.wade_depth - 10.0);
    let mut c = w.add_character(1, p, [0.0, 0.0, TERRAIN_SHELL]);
    let steps = walk(&mut w, &mut c, [86.8, 0.0], 120);
    assert!(steps.iter().all(|s| s.grounded && !s.swimming));
    assert!((c.feet[0] - 173.6).abs() < 1.0, "{:?}", c.feet);
    // Deeper: it swims from the first step, where it is (no buoyancy lifts it).
    let mut w = lake();
    let mut c = w.add_character(1, p, [0.0, 0.0, TERRAIN_SHELL]);
    let steps = walk(&mut w, &mut c, [86.8, 0.0], 60);
    assert!(steps.iter().all(|s| s.swimming), "{:?}", steps.first());
    assert!(c.swimming && !c.grounded);
    assert!((c.feet[2] - TERRAIN_SHELL).abs() < 0.05, "{:?}", c.feet);
}

#[test]
fn swimming_follows_the_view_and_has_no_buoyancy() {
    let mut w = lake();
    let p = CharacterParams::default();
    let mut c = w.add_character(1, p, [0.0, 0.0, 405.0]);
    swim(&mut w, &mut c, [0.0, 0.0], 0.0, false, 120);
    assert!(c.swimming);
    let float = 500.0 - p.float_depth;
    assert!((c.feet[2] - float).abs() < 0.01, "{:?}", c.feet);
    // Horizontal swimming keeps the floating height.
    swim(&mut w, &mut c, [348.0, 0.0], 0.0, false, 60);
    assert!((c.feet[0] - 348.0).abs() < 1.0 && (c.feet[2] - float).abs() < 0.01, "{:?}", c.feet);
    // Diving at 30 degrees down for half a second.
    let (s, k) = 30f32.to_radians().sin_cos();
    swim(&mut w, &mut c, [348.0 * k, 0.0], -348.0 * s, false, 30);
    assert!((c.feet[2] - (float - 87.0)).abs() < 1.0, "{:?}", c.feet);
    // Released, it stays at its depth.
    let z = c.feet[2];
    swim(&mut w, &mut c, [0.0, 0.0], 0.0, false, 120);
    assert!((c.feet[2] - z).abs() < 0.01 && c.swimming, "{:?}", c.feet);
    // Swimming up stops within one step above the floating height and sinks back.
    let mut top = f32::MIN;
    for _ in 0..90 {
        let st = swim(&mut w, &mut c, [0.0, 0.0], 246.0, false, 1);
        top = top.max(st.feet[2]);
    }
    assert!(top > float && top <= float + 246.0 / 60.0 + 0.01, "{top}");
    swim(&mut w, &mut c, [0.0, 0.0], 0.0, false, 60);
    assert!((c.feet[2] - float).abs() < 0.01, "{:?}", c.feet);
}

#[test]
fn jumping_works_only_above_the_floating_height() {
    let mut w = lake();
    let p = CharacterParams::default();
    let mut c = w.add_character(1, p, [0.0, 0.0, 405.0]);
    swim(&mut w, &mut c, [0.0, 0.0], 0.0, false, 120);
    let float = c.feet[2];
    swim(&mut w, &mut c, [0.0, 0.0], 0.0, true, 30);
    assert!((c.feet[2] - float).abs() < 0.01 && c.swimming, "{:?}", c.feet);
    // Placed above the floating height (the swimming state is kept), it jumps out at twice
    // the land height and falls back into the water.
    w.place_character(&mut c, [0.0, 0.0, 440.0]);
    assert!(c.swimming);
    let mut top = f32::MIN;
    for i in 0..120 {
        let st = swim(&mut w, &mut c, [0.0, 0.0], 0.0, i == 0, 1);
        top = top.max(st.feet[2]);
    }
    let rise = top - 440.0;
    assert!((rise - 2.0 * p.jump_height).abs() < 6.0, "{rise}");
    assert!(c.swimming);
}

#[test]
fn swimming_down_to_the_bottom_ends_swimming_where_it_is_shallow() {
    // A bottom shallower than the wading depth: the swimmer walks on it.
    let p = CharacterParams::default();
    let surface = p.wade_depth - 5.0;
    let mut w = pool(surface);
    let mut c = w.add_character(1, p, [0.0, 0.0, surface - p.swim_depth - 2.0]);
    swim(&mut w, &mut c, [0.0, 0.0], 0.0, false, 30);
    assert!(c.swimming, "{:?}", c.feet);
    swim(&mut w, &mut c, [0.0, 0.0], -348.0, false, 60);
    assert!(!c.swimming && c.grounded, "{:?}", c.feet);
    assert!((c.feet[2] - TERRAIN_SHELL).abs() < 0.05, "{:?}", c.feet);
    // A deep bottom: the swimmer stays on it, swimming.
    let mut w = lake();
    let mut c = w.add_character(1, p, [0.0, 0.0, 405.0]);
    swim(&mut w, &mut c, [0.0, 0.0], 0.0, false, 60);
    swim(&mut w, &mut c, [0.0, 0.0], -348.0, false, 120);
    assert!(c.swimming, "{:?}", c.feet);
    // Against the bed (the swept capsule's skin, not the walking contact shell).
    assert!(c.feet[2] > 0.0 && c.feet[2] < TERRAIN_SHELL + 0.05, "{:?}", c.feet);
}

#[test]
fn swim_vertical_rules() {
    let p = CharacterParams::default();
    let dt = 1.0 / 60.0;
    let float = 500.0 - p.float_depth;
    // Diving always applies; rising only from the floating height or below it.
    assert_eq!(fnv_phys::character::swim_vertical(&p, float + 30.0, 500.0, -100.0, dt), -100.0);
    assert_eq!(fnv_phys::character::swim_vertical(&p, float - 30.0, 500.0, 100.0, dt), 100.0);
    assert_eq!(fnv_phys::character::swim_vertical(&p, float, 500.0, 100.0, dt), 100.0);
    assert_eq!(fnv_phys::character::swim_vertical(&p, float + 30.0, 500.0, 100.0, dt), -p.swim_sink_speed);
    // Sinking stops at the floating height; below it there is no buoyancy.
    let v = fnv_phys::character::swim_vertical(&p, float + 0.1, 500.0, 0.0, dt);
    assert!((v * dt + 0.1).abs() < 1e-4, "{v}");
    assert_eq!(fnv_phys::character::swim_vertical(&p, float - 50.0, 500.0, 0.0, dt), 0.0);
}

#[test]
fn swimming_survives_above_the_surface() {
    // Placed 60 above the surface while swimming, the character keeps swimming and sinks
    // at the sinking speed (the game's player did 101 above placed water, physics.md §6.6).
    let mut w = lake();
    let p = CharacterParams::default();
    let mut c = w.add_character(1, p, [0.0, 0.0, 405.0]);
    swim(&mut w, &mut c, [0.0, 0.0], 0.0, false, 60);
    assert!(c.swimming);
    w.place_character(&mut c, [0.0, 0.0, 560.0]);
    swim(&mut w, &mut c, [0.0, 0.0], 0.0, false, 60);
    assert!(c.swimming && !c.grounded);
    assert!((c.feet[2] - (560.0 - p.swim_sink_speed)).abs() < 0.05, "{:?}", c.feet);
}

/// A placeable water model's volume: a box under the surface (half extents of the
/// 1×2 model, its top at the model's origin), as the Water-layer phantoms hold it.
fn water_box() -> CollisionObject {
    let mut o = on_layer(boxed([256.0, 512.0, 512.0]), layers::WATER);
    o.body_transform.translation = [0.0, 0.0, -512.0];
    o
}

#[test]
fn placed_water_is_the_box_of_its_reference() {
    let mut w = ground();
    // Scale 2, turned a quarter: 1024 wide along x, 2048 along y before the turn.
    let pose = RefPose { form_id: 7, position: [1000.0, 0.0, 300.0], rotation: rz(std::f32::consts::FRAC_PI_2), scale: 2.0 };
    assert_eq!(w.add_placed_water(&pose, &[water_box()]), 1);
    let v = *w.placed_water().next().unwrap();
    assert!((v.surface - 300.0).abs() < 1e-3, "{v:?}");
    // After the turn the long side runs along x.
    assert_eq!(w.water_surface_at(1000.0 + 1000.0, 0.0), Some(300.0));
    assert_eq!(w.water_surface_at(1000.0, 1000.0), None);
    assert_eq!(w.water_surface_at(1000.0, 500.0), Some(300.0));
    // Down to the bottom of the box only, for the character's lookup.
    assert_eq!(w.water_surface_for([1000.0, 0.0, 300.0 - 2047.0]), Some(300.0));
    assert_eq!(w.water_surface_for([1000.0, 0.0, 300.0 - 2049.0]), None);
    // A fall into it stops past the swimming depth; outside the box it lands.
    let p = CharacterParams::default();
    let mut c = w.add_character(1, p, [1000.0, 0.0, 305.0]);
    swim(&mut w, &mut c, [0.0, 0.0], 0.0, false, 240);
    assert!(c.swimming && (c.feet[2] - (300.0 - p.float_depth)).abs() < 8.0, "{:?}", c.feet);
    let mut d = w.add_character(2, p, [1000.0, 1000.0, 305.0]);
    swim(&mut w, &mut d, [0.0, 0.0], 0.0, false, 240);
    assert!(!d.swimming && d.grounded, "{:?}", d.feet);
    // Removed with its reference.
    assert!(w.remove_ref(7));
    assert_eq!(w.water_surface_at(1000.0, 0.0), None);
}

// --- air control, landings, edges (physics.md §6.4.1) ---

/// Per-step horizontal displacements of a character told `velocity` with Jump pressed in
/// the first step, from standing on the ground.
fn jump_steps(w: &mut PhysWorld, c: &mut Character, velocity: [f32; 2], steps: usize) -> Vec<(f32, CharacterStep)> {
    let mut prev = c.feet;
    (0..steps)
        .map(|i| {
            let s = w.move_character(c, CharacterInput { velocity, jump: i == 0, ..Default::default() });
            w.step();
            let d = (s.feet[0] - prev[0]).hypot(s.feet[1] - prev[1]);
            prev = s.feet;
            (d, s)
        })
        .collect()
}

#[test]
fn a_jump_with_the_run_starts_slow_in_the_air() {
    let mut w = ground();
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 10.0]);
    settle(&mut w, &mut c, 60);
    let dt = w.params.dt;
    let steps = jump_steps(&mut w, &mut c, [0.0, 339.5], 90);
    // In the air the speed closes 0.3 of the gap to the keys' speed per 1/60 s.
    let mut v = 0.0;
    for (i, (d, s)) in steps.iter().take(30).enumerate() {
        assert!(!s.grounded, "step {i} still in the air");
        v += fnv_phys::air::AIR_GAIN * (339.5 - v);
        assert!((d / dt - v).abs() < 0.5, "step {i}: {} against {v}", d / dt);
    }
    // After the landing the landing profile, then the keys' speed again.
    let land = steps.iter().position(|(_, s)| s.landed_speed.is_some()).expect("lands");
    for (j, &want) in fnv_phys::air::LANDING_SPEEDS.iter().enumerate() {
        let (d, _) = steps[land + 1 + j];
        assert!((d / dt - want).abs() < 0.5, "landing step {j}: {} against {want}", d / dt);
    }
    let (d, _) = steps[land + 1 + fnv_phys::air::LANDING_SPEEDS.len()];
    assert!((d / dt - 339.5).abs() < 0.5);
}

#[test]
fn a_key_pressed_only_in_the_air_gives_air_control() {
    let mut w = ground();
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 10.0]);
    settle(&mut w, &mut c, 60);
    let dt = w.params.dt;
    jump_steps(&mut w, &mut c, [0.0, 0.0], 5);
    let steps = jump_steps(&mut w, &mut c, [0.0, 344.0], 40);
    let (d, s) = &steps[39];
    assert!(!s.grounded && (d / dt - fnv_phys::air::AIR_CONTROL_SPEED).abs() < 0.2, "{}", d / dt);
}

#[test]
fn a_key_pressed_just_after_take_off_counts_as_held() {
    let mut w = ground();
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 10.0]);
    settle(&mut w, &mut c, 60);
    let dt = w.params.dt;
    jump_steps(&mut w, &mut c, [0.0, 0.0], 2);
    let steps = jump_steps(&mut w, &mut c, [0.0, 344.0], 30);
    let (d, s) = &steps[29];
    assert!(!s.grounded && (d / dt - 344.0).abs() < 0.5, "{}", d / dt);
}

#[test]
fn a_falling_character_rests_on_an_edge_without_sliding() {
    // A thin wall (3.5 wide, top at 70) and drops beside its centre line.
    let mut w = ground();
    w.add_ref(&at(1, [0.0, 0.0, 35.0]), &[object(boxed([1.75, 200.0, 35.0]), Motion::Static)]);
    let p = CharacterParams::default();
    for d in [8.0f32, 12.0, 16.0] {
        let mut c = w.add_character(2, p, [d, 0.0, 270.0]);
        let s = settle(&mut w, &mut c, 180);
        w.remove_character(c);
        // Past its flat tip the cone bottom touches the edge `bottom_slope` units lower per
        // unit off it, keeping the skin along its surface's normal.
        let e = d - 1.75;
        let want = 70.0 - p.bottom_slope * (e - p.bottom_flat) + p.skin * (1.0 + p.bottom_slope * p.bottom_slope).sqrt();
        assert!((s.feet[0] - d).abs() < 0.01 && s.feet[1].abs() < 0.01, "{d}: slid to {:?}", s.feet);
        assert!((s.feet[2] - want).abs() < 0.3, "{d}: {:?} against {want}", s.feet);
        assert!(s.grounded);
    }
}

#[test]
fn walls_keep_a_static_shell_gap_while_on_terrain() {
    let mut w = ground();
    // A wall whose south face is at y = 50.
    w.add_ref(&at(1, [0.0, 60.0, 100.0]), &[object(boxed([500.0, 10.0, 100.0]), Motion::Static)]);
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    let s = walk(&mut w, &mut c, [0.0, 200.0], 90).pop().unwrap();
    assert!(s.gap == TERRAIN_SHELL && (s.feet[2] - TERRAIN_SHELL).abs() < 0.02, "{:?}", s.feet);
    let stop = 50.0 - c.params.radius - 0.7;
    assert!((s.feet[1] - stop).abs() < 0.1, "{:?} against {stop}", s.feet);
}

#[test]
fn placed_water_survives_adding_the_same_refs_bodies() {
    // A reference whose model has both water volumes and solid collision: the callers add
    // the water, then the bodies (no model of the game has both, physics.md §6.6, but the
    // order must not matter).
    let mut w = ground();
    let pose = RefPose { form_id: 9, position: [1000.0, 0.0, 300.0], rotation: ID, scale: 1.0 };
    assert_eq!(w.add_placed_water(&pose, &[water_box()]), 1);
    let rep = w.add_ref(&pose, &[object(boxed([20.0, 20.0, 20.0]), Motion::Static)]);
    assert_eq!(rep.bodies, 1);
    assert_eq!(w.water_surface_at(1000.0, 0.0), Some(300.0));
    // Adding the bodies again replaces them and still keeps the water.
    w.add_ref(&pose, &[object(boxed([20.0, 20.0, 20.0]), Motion::Static)]);
    assert_eq!(w.ref_bodies(9).len(), 1);
    assert_eq!(w.water_surface_at(1000.0, 0.0), Some(300.0));
    // The loader's path (prepared off the main thread, then inserted) keeps it too.
    let prepared = PreparedRef::new(&w.params, &pose, &[object(boxed([20.0, 20.0, 20.0]), Motion::Static)]);
    w.insert_ref(prepared);
    assert_eq!(w.ref_bodies(9).len(), 1);
    assert_eq!(w.water_surface_at(1000.0, 0.0), Some(300.0));
    // The other order works too.
    let other = RefPose { form_id: 10, position: [-1000.0, 0.0, 300.0], ..pose };
    w.add_ref(&other, &[object(boxed([20.0, 20.0, 20.0]), Motion::Static)]);
    w.add_placed_water(&other, &[water_box()]);
    assert_eq!(w.water_surface_at(-1000.0, 0.0), Some(300.0));
    // Removing the reference removes both.
    assert!(w.remove_ref(9));
    assert_eq!(w.water_surface_at(1000.0, 0.0), None);
    assert!(w.ref_bodies(9).is_empty());
    // A reference with only water is removed too.
    let water_only = RefPose { form_id: 11, position: [0.0, 1000.0, 300.0], ..pose };
    w.add_placed_water(&water_only, &[water_box()]);
    assert!(w.remove_ref(11));
    assert!(!w.remove_ref(11));
}

#[test]
fn keyframed_door_blocks_until_posed_open() {
    // A door panel 100 wide (y) across the path at x = 100, hinged at y = -50, on the
    // AnimStatic layer, keyframed as door models are (docs/spec-notes/doors.md).
    let mut w = ground();
    let panel = CollisionObject {
        layer: layers::ANIM_STATIC,
        node_transform: Iso { translation: [0.0, -50.0, 0.0], rotation: ID },
        body_transform: Iso { translation: [0.0, 50.0, 100.0], rotation: ID },
        ..object(boxed([4.0, 50.0, 100.0]), Motion::Keyframed)
    };
    let pose = at(7, [100.0, 0.0, 0.0]);
    w.add_ref(&pose, std::slice::from_ref(&panel));
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 10.0]);
    settle(&mut w, &mut c, 30);
    walk(&mut w, &mut c, [80.0, 0.0], 150);
    assert!(c.feet[0] < 100.0 - 20.0, "blocked by the closed door: {:?}", c.feet);
    // A static object cannot be posed; a missing object index is refused.
    assert!(!w.pose_ref_object(&pose, 3, &panel.node_transform, &panel.body_transform));
    // Swing the hinge 90° (the panel now runs along +x from the hinge, out of the path).
    let open = Iso { translation: [0.0, -50.0, 0.0], rotation: rz(-std::f32::consts::FRAC_PI_2) };
    assert!(w.pose_ref_object(&pose, 0, &open, &panel.body_transform));
    let b = &w.ref_bodies(7)[0];
    assert!((b.translation[0] - 150.0).abs() < 0.5 && (b.translation[1] + 50.0).abs() < 0.5, "{:?}", b.translation);
    walk(&mut w, &mut c, [80.0, 0.0], 150);
    assert!(c.feet[0] > 150.0, "walked through the open doorway: {:?}", c.feet);
    // Stepping keeps the body where it was posed.
    w.step();
    let b = &w.ref_bodies(7)[0];
    assert!((b.translation[0] - 150.0).abs() < 0.5, "{:?}", b.translation);
}

/// A keyframed body is teleported into place (never moved smoothly), so it is a fixed
/// body. As a kinematic body it broke the character controller: rapier's "carried by a
/// kinematic body" rule froze a character walking down its slope (the Prospector Saloon's
/// broken door 0x00108BD7, issue #1485; the same model also let a degenerate collision
/// mesh's centre of mass become a phantom velocity that flung a character). A character
/// walking down a keyframed slope must move normally.
#[test]
fn keyframed_slope_does_not_freeze_the_character() {
    // A slab tilted so its top descends eastward, keyframed as door parts are.
    let a = 5.0f32.to_radians();
    let (s, c) = a.sin_cos();
    let rot = [c, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, c];
    let half = [200.0, 200.0, 5.0];
    let west = [-half[0], 0.0, half[2]];
    let rw = [rot[0] * west[0] + rot[2] * west[2], 0.0, rot[6] * west[0] + rot[8] * west[2]];
    let mut panel = object(boxed(half), Motion::Keyframed);
    panel.shell = 0.7;
    let mut w = ground();
    w.add_ref(&RefPose { form_id: 1, position: [0.0 - rw[0], 0.0, 100.0 - rw[2]], rotation: rot, scale: 1.0 }, &[panel]);
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 100.0, 120.0]);
    settle(&mut w, &mut c, 60);
    let dt = w.params.dt;
    let mut min_speed = f32::INFINITY;
    let mut prev = c.feet;
    for _ in 0..120 {
        let s = w.move_character(&mut c, CharacterInput { velocity: [0.0, -344.0], ..Default::default() });
        w.step();
        let speed = ((s.feet[0] - prev[0]).powi(2) + (s.feet[1] - prev[1]).powi(2)).sqrt() / dt;
        min_speed = min_speed.min(speed);
        prev = s.feet;
    }
    assert!(min_speed > 50.0, "froze on the keyframed slope: min {min_speed:.1} units/s");
}

#[test]
fn explosions_push_dynamic_bodies_away() {
    let mut w = ground();
    w.add_ref(&at(1, [60.0, 0.0, 6.0]), &[object(boxed([6.0, 6.0, 6.0]), Motion::Dynamic)]);
    w.add_ref(&at(2, [600.0, 0.0, 6.0]), &[object(boxed([6.0, 6.0, 6.0]), Motion::Dynamic)]);
    for _ in 0..30 {
        w.step();
    }
    let (near, far) = (w.ref_bodies(1)[0].translation, w.ref_bodies(2)[0].translation);
    assert_eq!(w.push_dynamic([0.0, 0.0, 6.0], 300.0, 400.0), 1);
    for _ in 0..30 {
        w.step();
    }
    assert!(w.ref_bodies(1)[0].translation[0] > near[0] + 20.0, "{:?}", w.ref_bodies(1));
    assert!((w.ref_bodies(2)[0].translation[0] - far[0]).abs() < 0.01);
}

/// #4917: a picked White Horse Nettle's body (one 45 kg box on its end, on the Non
/// Collidable layer) stays where it was placed: a character walks through it, a blast
/// beside it and a crate dropped on it leave it be, and the crate falls through to the
/// ground. Queries still see it.
#[test]
fn a_loose_body_on_the_non_collidable_layer_stays_rooted() {
    let mut w = ground();
    let nettle = CollisionObject { layer: fnv_phys::layers::NON_COLLIDABLE, mass: 45.0, ..object(boxed([2.5, 2.6, 27.0]), Motion::Dynamic) };
    w.insert_ref(fnv_phys::PreparedRef::new(&PhysParams::default(), &at(1, [60.0, 0.0, 27.0]), &[nettle]).asleep());
    w.add_ref(&at(2, [60.0, 0.0, 80.0]), &[object(boxed([6.0, 6.0, 6.0]), Motion::Dynamic)]);
    let placed = w.ref_bodies(1)[0];
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    walk(&mut w, &mut c, [120.0, 0.0], 120);
    assert!(c.feet[0] > 100.0, "walked through it: {:?}", c.feet);
    assert_eq!(w.push_dynamic([50.0, 0.0, 6.0], 300.0, 400.0), 1, "the blast pushes the crate only");
    for _ in 0..120 {
        w.step();
    }
    let b = w.ref_bodies(1)[0];
    assert_eq!((b.translation, b.rotation), (placed.translation, placed.rotation), "{placed:?} -> {b:?}");
    let crate_ = w.ref_bodies(2)[0];
    assert!(crate_.translation[2] < 20.0, "the crate fell through it to the ground: {crate_:?}");
    assert_eq!(w.stats().bodies_dynamic, 1);
    let hit = w.queries().ray([0.0, 0.0, 30.0], [1.0, 0.0, 0.0], 200.0, &Filter::default()).unwrap();
    assert_eq!(hit.owner.key(), Key::Ref(1));
}

// --- per-part hit volumes (combat C8) ---

#[test]
fn part_capsules_say_which_part_was_hit() {
    use fnv_phys::PartCapsule;
    let mut w = ground();
    let v = person([200.0, 0.0, 0.0], false);
    let parts = [
        // Torso: a vertical segment; head above it; a leg sloping forward.
        PartCapsule { part: 0, a: [200.0, 0.0, 70.0], b: [200.0, 0.0, 110.0], radius: 15.0 },
        PartCapsule { part: 1, a: [200.0, 0.0, 120.0], b: [200.0, 0.0, 130.0], radius: 9.0 },
        PartCapsule { part: 4, a: [200.0, 0.0, 60.0], b: [220.0, 0.0, 10.0], radius: 7.0 },
    ];
    w.set_actor_parts(0x10, v, &parts);
    w.update_queries();
    assert_eq!(w.actor_part_count(0x10), 3);
    // The widest part is the torso: its centre is the aim point.
    let aim = w.actor_aim_point(0x10).unwrap();
    assert!((aim[0] - 200.0).abs() < 1e-3 && (aim[2] - 90.0).abs() < 1e-3, "{aim:?}");
    let head = w.actor_part_center(0x10, 1).unwrap();
    assert!((head[2] - 125.0).abs() < 1e-3, "{head:?}");
    assert!(w.actor_part_center(0x10, 9).is_none());
    // A side branch of the torso (a creature's leg) as wide as the torso does not take
    // the aim point: the first of the widest, the part's own chain, keeps it.
    let mut branched = parts.to_vec();
    branched.push(PartCapsule { part: 0, a: [200.0, 0.0, 70.0], b: [200.0, 0.0, 20.0], radius: 15.0 });
    w.set_actor_parts(0x11, person([400.0, 0.0, 0.0], false), &branched.iter().map(|p| PartCapsule { a: [p.a[0] + 200.0, p.a[1], p.a[2]], b: [p.b[0] + 200.0, p.b[1], p.b[2]], ..*p }).collect::<Vec<_>>());
    w.update_queries();
    let aim = w.actor_aim_point(0x11).unwrap();
    assert!((aim[2] - 90.0).abs() < 1e-3, "{aim:?}");
    let shot = |w: &mut PhysWorld, z: f32| w.queries().ray([0.0, 0.0, z], [1.0, 0.0, 0.0], 1000.0, &Filter::default()).map(|h| (h.owner, h.distance));
    let (o, d) = shot(&mut w, 90.0).unwrap();
    assert_eq!(o, Owner::Actor { form_id: 0x10, layer: layers::BIPED, part: 0 });
    assert!((d - 185.0).abs() < 0.1, "{d}");
    assert_eq!(shot(&mut w, 125.0).unwrap().0, Owner::Actor { form_id: 0x10, layer: layers::BIPED, part: 1 });
    // The leg at z 35 is centred at x 210.
    let (o, d) = shot(&mut w, 35.0).unwrap();
    assert_eq!(o, Owner::Actor { form_id: 0x10, layer: layers::BIPED, part: 4 });
    assert!(d > 195.0 && d < 210.0, "{d}");
    // Between the head and the top of the torso capsule there is a gap no single capsule had.
    assert!(shot(&mut w, 145.0).is_none());
    // Posed again (moved along x): the same colliders follow.
    let moved: Vec<PartCapsule> = parts.iter().map(|p| PartCapsule { a: [p.a[0] + 100.0, p.a[1], p.a[2]], b: [p.b[0] + 100.0, p.b[1], p.b[2]], ..*p }).collect();
    w.set_actor_parts(0x10, person([300.0, 0.0, 0.0], true), &moved);
    w.update_queries();
    let (o, d) = shot(&mut w, 90.0).unwrap();
    assert_eq!(o, Owner::Actor { form_id: 0x10, layer: layers::DEAD_BIP, part: 0 });
    assert!((d - 285.0).abs() < 0.1, "{d}");
    // Back to the single capsule.
    w.set_actor_volume(0x10, person([300.0, 0.0, 0.0], false));
    w.update_queries();
    assert_eq!(w.actor_part_count(0x10), 0);
    assert_eq!(shot(&mut w, 125.0).unwrap().0, Owner::Actor { form_id: 0x10, layer: layers::BIPED, part: fnv_phys::actors::WHOLE });
}

/// The torso, head and a leg of `part_capsules_say_which_part_was_hit`, `dx` along x.
fn three_parts(dx: f32) -> Vec<PartCapsule> {
    [
        PartCapsule { part: 0, a: [200.0, 0.0, 70.0], b: [200.0, 0.0, 110.0], radius: 15.0 },
        PartCapsule { part: 1, a: [200.0, 0.0, 120.0], b: [200.0, 0.0, 130.0], radius: 9.0 },
        PartCapsule { part: 4, a: [200.0, 0.0, 60.0], b: [220.0, 0.0, 10.0], radius: 7.0 },
    ]
    .iter()
    .map(|p| PartCapsule { a: [p.a[0] + dx, p.a[1], p.a[2]], b: [p.b[0] + dx, p.b[1], p.b[2]], ..*p })
    .collect()
}

#[test]
fn bit_identical_part_writes_are_skipped_and_dirty_nothing() {
    let mut w = ground();
    w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &three_parts(0.0));
    w.update_queries();
    let before = w.refresh_stats();
    assert_eq!(before.hitvol_written, 3);
    // The same pose again: nothing written, nothing for a refresh to do.
    w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &three_parts(0.0));
    w.update_queries();
    let same = w.refresh_stats();
    assert_eq!((same.hitvol_written, same.hitvol_skipped, same.refreshes), (3, 3, before.refreshes));
    assert_eq!(same.hitvol_colliders, 6, "every capsule handed in is still counted");
    // -0.0 is not 0.0 to the bit: written.
    let mut signed = three_parts(0.0);
    signed[1].a[1] = -0.0;
    w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &signed);
    assert_eq!(w.refresh_stats().hitvol_written, 6);
    // Dead now (same pose): written, the colliders move to DEAD_BIP.
    w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], true), &signed);
    w.update_queries();
    assert_eq!(w.refresh_stats().hitvol_written, 9);
    assert_eq!(w.refresh_stats().refreshes, before.refreshes + 1);
    let shot = w.queries().ray([0.0, 0.0, 90.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default()).unwrap();
    assert_eq!(shot.owner, Owner::Actor { form_id: 0x10, layer: layers::DEAD_BIP, part: 0 });
    // After the single capsule, the same parts are written again (its parts were dropped).
    w.set_actor_volume(0x10, person([200.0, 0.0, 0.0], true));
    w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], true), &signed);
    assert_eq!(w.actor_part_count(0x10), 3);
    assert_eq!(w.refresh_stats().hitvol_written, 13);
}

#[test]
fn standalone_hit_volumes_answer_as_attached_ones_to_the_bit() {
    // The same writes into a world with attached capsules and one with standalone ones
    // (both set explicitly, whatever the default): every query, aim point, body digest and
    // step agrees to the bit, including aim points read between a write and the next refresh.
    let mut a = ground();
    a.set_standalone_hit_volumes(false);
    let mut b = ground();
    b.set_standalone_hit_volumes(true);
    let bits = |h: Option<RayHit>| h.map(|h| (h.owner, h.distance.to_bits(), h.point.map(f32::to_bits), h.normal.map(f32::to_bits)));
    let look = |w: &mut PhysWorld| {
        let aims = (w.actor_aim_point(0x10).map(|p| p.map(f32::to_bits)), w.actor_part_centers(0x10).iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>());
        let mut shots = Vec::new();
        for z in [10.0, 35.0, 69.0, 90.0, 115.0, 125.0, 131.0, 145.0] {
            for y in [-12.0, -3.0, 0.0, 4.5, 11.0] {
                shots.push(bits(w.queries().ray([0.0, y, z], [1.0, 0.0, 0.0], 1000.0, &Filter::default())));
            }
        }
        shots.push(bits(w.queries().ray([210.0, 0.0, 300.0], [0.0, 0.0, -1.0], 1000.0, &Filter::default())));
        (aims, shots, w.state_digest())
    };
    #[allow(clippy::type_complexity)]
    let script: Vec<Box<dyn Fn(&mut PhysWorld)>> = vec![
        Box::new(|w| w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &three_parts(0.0))),
        Box::new(|w| w.set_actor_parts(0x10, person([213.5, 1.25, 0.5], false), &three_parts(13.5))),
        Box::new(|w| w.step()),
        Box::new(|w| w.set_actor_parts(0x10, person([213.5, 1.25, 0.5], true), &three_parts(13.5)[..2])),
        Box::new(|w| w.set_actor_volume(0x10, person([250.0, -3.0, 0.0], false))),
        Box::new(|w| w.step()),
        Box::new(|w| w.set_actor_parts(0x10, person([180.0, 0.0, 0.0], false), &three_parts(-20.0))),
    ];
    for (i, act) in script.iter().enumerate() {
        act(&mut a);
        act(&mut b);
        // Before any refresh (aim points read the colliders as they stand), then after.
        let early = |w: &PhysWorld| (w.actor_aim_point(0x10).map(|p| p.map(f32::to_bits)), w.actor_part_centers(0x10).iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>());
        assert_eq!(early(&a), early(&b), "write {i}, before the refresh");
        assert_eq!(look(&mut a), look(&mut b), "write {i}");
    }
    for _ in 0..5 {
        a.step();
        b.step();
    }
    assert_eq!(a.state_digest(), b.state_digest());
    assert_eq!((a.stats().bodies_kinematic, a.stats().colliders), (b.stats().bodies_kinematic, b.stats().colliders));
    assert!(a.remove_actor_volume(0x10) && b.remove_actor_volume(0x10));
    assert_eq!(a.stats().colliders, b.stats().colliders, "the standalone colliders went with the volume");
    assert_eq!(look(&mut a), look(&mut b));
    assert!(b.queries().ray([0.0, 0.0, 90.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default()).is_none());
}

#[test]
fn a_swinging_door_is_not_solid() {
    // The closed panel of `keyframed_door_blocks_until_posed_open`, made non-solid as
    // while it swings: the character walks through it and rays miss it; solid again, it
    // blocks once more. A static frame beside it stays solid throughout.
    let mut w = ground();
    let panel = CollisionObject {
        layer: layers::ANIM_STATIC,
        node_transform: Iso { translation: [0.0, -50.0, 0.0], rotation: ID },
        body_transform: Iso { translation: [0.0, 50.0, 100.0], rotation: ID },
        ..object(boxed([4.0, 50.0, 100.0]), Motion::Keyframed)
    };
    let frame = CollisionObject { node_transform: Iso { translation: [0.0, 300.0, 100.0], rotation: ID }, ..object(boxed([4.0, 50.0, 100.0]), Motion::Static) };
    let pose = at(7, [100.0, 0.0, 0.0]);
    w.add_ref(&pose, &[panel, frame]);
    let solid = Filter::default();
    let ray = |w: &mut PhysWorld, y: f32| w.queries().ray([0.0, y, 50.0], [1.0, 0.0, 0.0], 500.0, &solid).map(|h| h.owner);
    assert!(matches!(ray(&mut w, 0.0), Some(Owner::Ref { form_id: 7, object: 0, .. })));
    assert_eq!(w.set_ref_keyframed_solid(7, false), 1);
    assert_eq!(w.set_ref_keyframed_solid(7, false), 0, "already not solid");
    assert!(ray(&mut w, 0.0).is_none(), "the swinging panel is not hit");
    assert!(matches!(ray(&mut w, 300.0), Some(Owner::Ref { form_id: 7, object: 1, .. })), "the frame still is");
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 10.0]);
    settle(&mut w, &mut c, 30);
    walk(&mut w, &mut c, [80.0, 0.0], 150);
    assert!(c.feet[0] > 150.0, "walked through the swinging panel: {:?}", c.feet);
    // At rest again: solid.
    assert_eq!(w.set_ref_keyframed_solid(7, true), 1);
    assert!(matches!(ray(&mut w, 0.0), Some(Owner::Ref { form_id: 7, object: 0, .. })));
    walk(&mut w, &mut c, [-80.0, 0.0], 150);
    assert!(c.feet[0] > 100.0 + 20.0, "blocked from the far side: {:?}", c.feet);
}

#[test]
fn a_character_told_to_passes_other_characters() {
    let mut w = ground();
    let mut stander = w.add_character(1, CharacterParams::default(), [100.0, 0.0, 1.0]);
    let mut blocked = w.add_character(2, CharacterParams::default(), [0.0, 0.0, 1.0]);
    let mut passing = w.add_character(3, CharacterParams::default(), [0.0, 300.0, 1.0]);
    passing.passes_characters = true;
    for c in [&mut stander, &mut blocked, &mut passing] {
        settle(&mut w, c, 30);
    }
    // Another character is in the way: stopped touching it (two radii and the gap).
    walk(&mut w, &mut blocked, [120.0, 0.0], 120);
    assert!(blocked.feet[0] > 50.0 && blocked.feet[0] < 57.0, "{:?}", blocked.feet);
    // Put behind the two, the third walks through both and is not turned aside.
    w.place_character(&mut passing, [-60.0, 0.0, 1.0]);
    settle(&mut w, &mut passing, 30);
    let steps = walk(&mut w, &mut passing, [120.0, 0.0], 180);
    assert!(passing.feet[0] > 250.0 && passing.feet[1].abs() < 0.01, "{:?}", passing.feet);
    assert!(steps.iter().all(|s| s.touched.iter().all(|o| !matches!(o.key(), Key::Character(_)))));
    // Walking through them it took neither along.
    settle(&mut w, &mut blocked, 1);
    settle(&mut w, &mut stander, 1);
    assert!(blocked.feet[0] < 57.0 && (stander.feet[0] - 100.0).abs() < 0.01, "{:?} {:?}", blocked.feet, stander.feet);
    // The one passed still meets the one that passes it.
    let p = passing.feet;
    w.place_character(&mut stander, [p[0] + 100.0, p[1], 1.0]);
    settle(&mut w, &mut stander, 30);
    walk(&mut w, &mut stander, [-120.0, 0.0], 60);
    assert!(stander.feet[0] > p[0] + 43.0 && stander.feet[0] < p[0] + 46.0, "{:?} {p:?}", stander.feet);
}

#[test]
fn a_placed_character_takes_its_body_along() {
    // Regression: placed after it had stood long enough for its kinematic body to sleep,
    // the body stayed where it was put while the character walked on, so rays and other
    // characters met it there.
    let mut w = ground();
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    for _ in 0..600 {
        w.step();
    }
    w.place_character(&mut c, [300.0, 0.0, 1.0]);
    settle(&mut w, &mut c, 30);
    walk(&mut w, &mut c, [0.0, 120.0], 120);
    assert!(c.feet[1] > 200.0, "{:?}", c.feet);
    w.update_queries();
    // A ray across where it was put meets nothing; one across where it is meets it.
    assert!(w.queries().ray([200.0, 0.0, 60.0], [1.0, 0.0, 0.0], 200.0, &Filter::default()).is_none());
    let hit = w.queries().ray([200.0, c.feet[1], 60.0], [1.0, 0.0, 0.0], 200.0, &Filter::default()).unwrap();
    assert_eq!(hit.owner, Owner::Character { id: 1 });
    assert!((hit.distance - 78.0).abs() < 1.0, "{}", hit.distance);
}

/// A placed reference's collision added asleep (`PreparedRef::asleep`), as the engine adds
/// a cell's clutter.
fn add_asleep(w: &mut PhysWorld, form_id: u32, pos: [f32; 3], half: [f32; 3]) {
    let p = PreparedRef::new(&w.params, &at(form_id, pos), &[CollisionObject { mass: 1.0, ..object(boxed(half), Motion::Dynamic) }]).asleep();
    w.insert_ref(p);
}

#[test]
fn clutter_added_asleep_stays_put_with_nothing_under_it() {
    // A book placed in mid-air (inside a display case whose inner shelf has no
    // collision) stays where it was placed, through queries and steps.
    let mut w = ground();
    add_asleep(&mut w, 0x99, [100.0, 100.0, 300.0], [5.0, 5.0, 2.0]);
    let _ = down(&mut w, 0.0, 0.0);
    for _ in 0..120 {
        w.step();
    }
    let b = w.ref_bodies(0x99)[0];
    assert!(b.sleeping && (b.translation[2] - 300.0).abs() < 1e-3, "{b:?}");
}

#[test]
fn clutter_added_asleep_falls_once_struck() {
    let mut w = ground();
    add_asleep(&mut w, 0x99, [100.0, 100.0, 300.0], [5.0, 5.0, 2.0]);
    for _ in 0..10 {
        w.step();
    }
    let at0 = w.ref_bodies(0x99)[0].translation;
    assert!(w.push_ref(0x99, at0, [0.0, 0.0, -10.0], 1000.0).is_some());
    let _ = down(&mut w, 0.0, 0.0);
    for _ in 0..60 {
        w.step();
    }
    let z = w.ref_bodies(0x99)[0].translation[2];
    assert!(z < 250.0, "the struck box fell: z = {z}");
}

#[test]
fn clutter_added_asleep_wakes_when_a_body_falls_on_it() {
    // A box asleep in mid-air; an awake box dropped onto it wakes it, and both fall.
    let mut w = ground();
    add_asleep(&mut w, 0x99, [100.0, 100.0, 300.0], [10.0, 10.0, 5.0]);
    w.add_ref(&at(0x9A, [100.0, 100.0, 340.0]), &[CollisionObject { mass: 1.0, ..object(boxed([5.0, 5.0, 5.0]), Motion::Dynamic) }]);
    for _ in 0..120 {
        w.step();
    }
    let z = w.ref_bodies(0x99)[0].translation[2];
    assert!(z < 250.0, "the sleeping box was woken and fell: z = {z}");
}

#[test]
fn clutter_added_asleep_falls_when_its_support_is_removed() {
    // A box asleep on a shelf (a dynamic box asleep too) falls when the shelf's reference
    // is removed (picked up).
    let mut w = ground();
    add_asleep(&mut w, 0x9B, [100.0, 100.0, 290.0], [20.0, 20.0, 5.0]);
    add_asleep(&mut w, 0x99, [100.0, 100.0, 300.0], [5.0, 5.0, 5.0]);
    for _ in 0..10 {
        w.step();
    }
    assert!(w.remove_ref(0x9B));
    for _ in 0..60 {
        w.step();
    }
    let z = w.ref_bodies(0x99)[0].translation[2];
    assert!(z < 250.0, "the box fell once its support was gone: z = {z}");
}

#[test]
fn clutter_added_asleep_on_a_shelf_it_overlaps_stays_asleep() {
    // Placed 0.6 into a static shelf and touching a neighbour, as clutter is placed: no
    // push apart at load.
    let mut w = ground();
    w.add_ref(&at(0x9C, [100.0, 100.0, 290.0]), &[object(boxed([50.0, 50.0, 10.0]), Motion::Static)]);
    add_asleep(&mut w, 0x99, [100.0, 100.0, 304.4], [5.0, 5.0, 5.0]);
    add_asleep(&mut w, 0x9A, [109.0, 100.0, 304.4], [5.0, 5.0, 5.0]);
    let _ = down(&mut w, 0.0, 0.0);
    for _ in 0..120 {
        w.step();
    }
    for id in [0x99, 0x9A] {
        let b = w.ref_bodies(id)[0];
        assert!(b.sleeping && (b.translation[2] - 304.4).abs() < 1e-3, "{id:X}: {b:?}");
    }
}

#[test]
fn clutter_asleep_on_a_keyframed_lid_stays_asleep() {
    // A container's keyframed lid (a fixed body), a bottle overlapping it.
    let mut w = ground();
    let lid = PreparedRef::new(&w.params, &at(0x9C, [100.0, 100.0, 290.0]), &[object(boxed([50.0, 50.0, 10.0]), Motion::Keyframed)]);
    w.insert_ref(lid);
    add_asleep(&mut w, 0x99, [100.0, 100.0, 304.0], [5.0, 5.0, 5.0]);
    for _ in 0..120 {
        w.step();
    }
    let b = w.ref_bodies(0x99)[0];
    assert!(b.sleeping && (b.translation[2] - 304.0).abs() < 1e-3, "{b:?}");
}

#[test]
fn running_off_a_low_edge_keeps_the_ground_and_the_speed() {
    // A platform (top at `h`) ending at x = 0; the character runs off it at 344 units/s
    // (physics.md §6.4.2).
    for (h, lands_within) in [(8.0f32, 3usize), (16.0, 3), (22.0, 3)] {
        let mut w = ground();
        w.add_ref(&at(1, [-500.0, 0.0, h / 2.0]), &[object(boxed([500.0, 200.0, h / 2.0]), Motion::Static)]);
        let mut c = w.add_character(1, CharacterParams::default(), [-200.0, 0.0, h + 5.0]);
        settle(&mut w, &mut c, 60);
        let steps = walk(&mut w, &mut c, [344.0, 0.0], 90);
        let airborne = steps.iter().filter(|s| !s.grounded).count();
        assert!(airborne <= lands_within, "{h}: {airborne} steps in the air");
        assert!(steps.iter().all(|s| s.landed_speed.is_none_or(|v| v < air::LANDING_MIN_SPEED)), "{h}: a hard landing");
        assert!(c.since_landing.is_none(), "{h}: the landing slowed it");
        assert!((c.feet[0] - (-200.0 + 344.0 * 1.5)).abs() < 2.0, "{h}: {:?}", c.feet);
    }
}

#[test]
fn running_off_a_high_edge_still_falls() {
    let mut w = ground();
    w.add_ref(&at(1, [-500.0, 0.0, 30.0]), &[object(boxed([500.0, 200.0, 30.0]), Motion::Static)]);
    let mut c = w.add_character(1, CharacterParams::default(), [-200.0, 0.0, 65.0]);
    settle(&mut w, &mut c, 60);
    let steps = walk(&mut w, &mut c, [344.0, 0.0], 90);
    let airborne = steps.iter().filter(|s| !s.grounded).count();
    assert!(airborne >= 10, "{airborne} steps in the air off a 60-unit edge");
    assert!(steps.iter().any(|s| s.landed_speed.is_some_and(|v| v >= air::LANDING_MIN_SPEED)));
}

#[test]
fn the_landing_profile_needs_a_drop_above_the_step_height() {
    assert!(fnv_phys::character::landing_profile(290.0, 61.6, 24.0));
    assert!(!fnv_phys::character::landing_profile(170.0, 20.0, 24.0));
    assert!(!fnv_phys::character::landing_profile(90.0, 30.0, 24.0));
}

#[test]
fn a_held_box_follows_the_grip_target_and_falls_when_let_go() {
    let mut w = ground();
    w.add_ref(&at(5, [60.0, 0.0, 6.0]), &[object(boxed([6.0, 6.0, 6.0]), Motion::Dynamic)]);
    for _ in 0..60 {
        w.step();
    }
    let rest = w.ref_bodies(5)[0].translation;
    // Nothing to hold on a reference without collision.
    assert!(w.grab(GripOn::Ref(6), rest, HoldParams::default()).is_none());
    let mass = w.grip_mass(GripOn::Ref(5), rest).unwrap();
    assert_eq!(w.grab(GripOn::Ref(5), rest, HoldParams::default()), Some(mass));
    // Lifted 60 units and carried 40 to the side: the grip point is there within a second.
    let target = [rest[0], rest[1] + 40.0, rest[2] + 60.0];
    w.set_grip_target(target);
    for _ in 0..60 {
        w.step();
    }
    let p = w.grip_point().unwrap();
    let gap = (0..3).map(|i| (p[i] - target[i]).powi(2)).sum::<f32>().sqrt();
    assert!(gap < 3.0, "grip point {p:?}, target {target:?}");
    // Held still in the air: it does not sag away from the target.
    for _ in 0..60 {
        w.step();
    }
    let p = w.grip_point().unwrap();
    assert!((p[2] - target[2]).abs() < 2.0, "{p:?}");
    assert_eq!(w.holding().map(|g| g.on), Some(GripOn::Ref(5)));
    // Let go, it falls back to the ground.
    assert!(w.release_grip(None).is_some());
    assert!(w.holding().is_none() && w.release_grip(None).is_none());
    for _ in 0..120 {
        w.step();
    }
    let end = w.ref_bodies(5)[0].translation;
    assert!((end[2] - rest[2]).abs() < 2.0 && (end[1] - rest[1] - 40.0).abs() < 15.0, "{end:?} from {rest:?}");
}

#[test]
fn a_thrown_box_flies_off() {
    let mut w = ground();
    w.add_ref(&at(5, [60.0, 0.0, 6.0]), &[object(boxed([6.0, 6.0, 6.0]), Motion::Dynamic)]);
    let rest = w.ref_bodies(5)[0].translation;
    w.grab(GripOn::Ref(5), rest, HoldParams::default()).unwrap();
    w.set_grip_target([rest[0], rest[1], rest[2] + 50.0]);
    for _ in 0..30 {
        w.step();
    }
    w.release_grip(Some([400.0, 0.0, 0.0]));
    for _ in 0..30 {
        w.step();
    }
    assert!(w.ref_bodies(5)[0].translation[0] > rest[0] + 100.0);
}

#[test]
fn a_static_reference_cannot_be_held() {
    let mut w = ground();
    w.add_ref(&at(7, [60.0, 0.0, 6.0]), &[object(boxed([6.0, 6.0, 6.0]), Motion::Static)]);
    assert!(w.grip_mass(GripOn::Ref(7), [60.0, 0.0, 6.0]).is_none());
    assert!(w.grab(GripOn::Ref(7), [60.0, 0.0, 6.0], HoldParams::default()).is_none());
    assert!(w.holding().is_none());
}

/// Loose items in water float or sink by their Havok material (docs/spec-notes/ragdoll.md
/// §8, captures fl_items_lake and fl_heavy_lake): a wooden one under the surface rises
/// and floats there, a metal one sinks to the bed at a pace slowed by the water, and an
/// item out of water falls as before.
#[test]
fn loose_items_float_or_sink_by_material() {
    let mut w = ground();
    w.set_water(WaterArea::Exterior(0, 0), Some(400.0));
    let item = |material: u32| CollisionObject { material: Some(material), ..object(boxed([4.0, 4.0, 2.0]), Motion::Dynamic) };
    w.add_ref(&at(1, [100.0, 100.0, 250.0]), &[item(9)]); // wood
    w.add_ref(&at(2, [200.0, 100.0, 250.0]), &[item(5)]); // metal
    w.add_ref(&at(3, [300.0, 100.0, 250.0]), &[item(24)]); // bottle
    w.add_ref(&at(4, [-200.0, -200.0, 250.0]), &[item(9)]); // wood, out of the water (cell -1, -1)
    let z = |w: &PhysWorld, id: u32| w.ref_bodies(id)[0].translation[2];
    let mut metal_low_at = None;
    for i in 0..(20.0 * 60.0) as usize {
        w.step();
        if metal_low_at.is_none() && z(&w, 2) < 20.0 {
            metal_low_at = Some(i as f32 / 60.0);
        }
    }
    for id in [1, 3] {
        assert!((z(&w, id) - 400.0).abs() < 6.0, "item {id} floats at the surface: z {}", z(&w, id));
    }
    // 250 units down at about 170 a second, not the free fall's 0.85 s.
    let t = metal_low_at.expect("the metal item reaches the bed");
    assert!(t > 1.2 && t < 3.0, "the metal item reached the bed after {t} s");
    assert!(z(&w, 4) < 20.0, "out of water the wooden item lies on the ground: z {}", z(&w, 4));
}

// --- attached vs standalone hit volumes: differential tests ---
//
// Each scenario drives a world with attached part capsules and one with standalone ones
// (`PhysWorld::set_standalone_hit_volumes`, both set explicitly) through the same script and
// compares every answer. Exact ties are built on purpose: before #1459 first-hit `ray`/`sweep`
// kept the first of equal costs in tree-traversal order [design: parry3d 0.31.1
// `partitioning/bvh/bvh_traverse.rs:393`] and `ray_all` broke equal distances only by owner;
// several segments can share a part id [design: engine/nvforever/src/limbs.rs:334]. Now all
// three break ties by owner, segment and hit geometry (`first_hit`, physics.md §5.1).
//
// Every scenario runs twice: `hitvol_diff_*` compares to the bit (owner, part, normal,
// point, distance, hit order); `hitvol_ties_*` compares what a tie cannot change (a ray's
// distance and point, the set of `ray_all` hits, a sweep's fraction, overlaps, aim points,
// body states, the character), and every answer it lets through that differs to the bit
// must be an exact tie shown in both worlds (`tied`). The comments above six `hitvol_diff_*`
// keep the divergences observed before #1459, when they were `#[ignore]`d.

/// One query answer: what was asked, the answer as printed, every float in it to the bit,
/// the part of it no exact tie can change (`loose`), and what shows that a difference
/// outside `loose` is an exact tie (`tie`, [`tied`]).
#[derive(Debug)]
struct Ans {
    q: String,
    shown: String,
    bits: Vec<u32>,
    loose: String,
    tie: Tie,
}

/// The evidence [`tied`] reads, asked of the answer's own world.
#[derive(Debug, Default)]
enum Tie {
    /// An answer with no tie in it (loose and exact are the same).
    #[default]
    None,
    /// A first-hit ray, and every hit of the same ray (`ray_all`).
    Ray(Option<RayHit>, Vec<RayHit>),
    /// `ray_all`'s hits.
    RayAll(Vec<RayHit>),
    /// A sweep, and the owners the swept shape touches (within [`TIE_TOUCH`]) where it
    /// stopped.
    Sweep(Option<SweepHit>, Vec<Owner>),
}

/// How far (game units) a sweep's shape is grown to find what it touches at its stop.
const TIE_TOUCH: f32 = 0.05;

/// `s` grown by `by` on every side (the probe shapes: spheres, capsules and boxes).
fn grown(s: &Shape, by: f32) -> Shape {
    match s {
        Shape::Sphere { radius } => Shape::Sphere { radius: radius + by },
        Shape::Capsule { a, b, radius } => Shape::Capsule { a: *a, b: *b, radius: radius + by },
        Shape::Box { half_extents } => Shape::Box { half_extents: half_extents.map(|h| h + by) },
        other => panic!("no probe shape: {other:?}"),
    }
}

/// Whether a difference between the attached world's answer `x` and the standalone one's
/// `y` that `loose` lets through is an exact tie, shown in both worlds:
/// - `ray`: each world's winner is among the other world's `ray_all` hits, with the same
///   owner, distance and normal to the bit;
/// - `ray_all`: the hits come in the same order of distances, so only hits at one
///   distance changed places;
/// - `sweep`: each world's winner is touched by the swept shape where the other world's
///   sweep stopped (the same fraction, by `loose`).
fn tied(x: &Ans, y: &Ans) -> Result<(), String> {
    let same = |a: &RayHit, b: &RayHit| a.owner == b.owner && a.distance.to_bits() == b.distance.to_bits() && a.normal.map(f32::to_bits) == b.normal.map(f32::to_bits);
    match (&x.tie, &y.tie) {
        (Tie::Ray(Some(hx), all_x), Tie::Ray(Some(hy), all_y)) => {
            if !all_x.iter().any(|h| same(h, hy)) {
                return Err(format!("the standalone winner is no attached hit at its distance: {hy:?}"));
            }
            if !all_y.iter().any(|h| same(h, hx)) {
                return Err(format!("the attached winner is no standalone hit at its distance: {hx:?}"));
            }
            Ok(())
        }
        (Tie::RayAll(ax), Tie::RayAll(ay)) => {
            let d = |hs: &[RayHit]| hs.iter().map(|h| h.distance.to_bits()).collect::<Vec<_>>();
            if d(ax) == d(ay) {
                Ok(())
            } else {
                Err("hits changed places across distances".into())
            }
        }
        (Tie::Sweep(Some(hx), touch_x), Tie::Sweep(Some(hy), touch_y)) => {
            if !touch_x.contains(&hy.owner) {
                return Err(format!("the standalone winner {:?} is not touched at the attached stop {touch_x:?}", hy.owner));
            }
            if !touch_y.contains(&hx.owner) {
                return Err(format!("the attached winner {:?} is not touched at the standalone stop {touch_y:?}", hx.owner));
            }
            Ok(())
        }
        _ => Err("differs where no tie can be".into()),
    }
}

fn ray_bits(h: &RayHit, out: &mut Vec<u32>) {
    out.push(h.distance.to_bits());
    out.extend(h.point.map(f32::to_bits));
    out.extend(h.normal.map(f32::to_bits));
    out.push(h.shell.to_bits());
}

fn ray_shown(h: &RayHit) -> String {
    format!("{:?} d={:?} n={:?} p={:?}", h.owner, h.distance, h.normal, h.point)
}

/// A first-hit ray, with `all`, the same ray's `ray_all` (for [`tied`]).
fn ans_ray(q: String, h: Option<RayHit>, all: Vec<RayHit>) -> Ans {
    let mut bits = Vec::new();
    if let Some(h) = &h {
        ray_bits(h, &mut bits);
    }
    // Equal distances give the same point; owner and normal follow the tie's winner.
    let loose = format!("{:x?}", h.map(|h| (h.distance.to_bits(), h.point.map(f32::to_bits))));
    Ans { q, shown: h.as_ref().map_or("miss".into(), ray_shown), bits, loose, tie: Tie::Ray(h, all) }
}

fn ans_ray_all(q: String, hs: Vec<RayHit>) -> Ans {
    let mut bits = Vec::new();
    hs.iter().for_each(|h| ray_bits(h, &mut bits));
    // The same hits in any order.
    let mut each: Vec<String> = hs
        .iter()
        .map(|h| {
            let mut b = Vec::new();
            ray_bits(h, &mut b);
            format!("{:?} {b:x?}", h.owner)
        })
        .collect();
    each.sort();
    Ans { q, shown: format!("[{}]", hs.iter().map(ray_shown).collect::<Vec<_>>().join("; ")), bits, loose: each.join("; "), tie: Tie::RayAll(hs) }
}

/// A sweep, with `touch`, what its shape touches where it stopped (for [`tied`]).
fn ans_sweep(q: String, h: Option<SweepHit>, touch: Vec<Owner>) -> Ans {
    let mut bits = Vec::new();
    let shown = match &h {
        Some(h) => {
            bits.push(h.fraction.to_bits());
            bits.extend(h.point.map(f32::to_bits));
            bits.extend(h.normal.map(f32::to_bits));
            format!("{:?} f={:?} n={:?} p={:?}", h.owner, h.fraction, h.normal, h.point)
        }
        None => "miss".into(),
    };
    // The witness point and normal belong to the tie's winner; the fraction does not.
    let loose = format!("{:x?}", h.map(|h| h.fraction.to_bits()));
    Ans { q, shown, bits, loose, tie: Tie::Sweep(h, touch) }
}

/// An answer with no tie in it: loose and exact are the same.
fn ans_plain(q: String, shown: String, bits: Vec<u32>) -> Ans {
    let loose = format!("{shown} {bits:x?}");
    Ans { q, shown, bits, loose, tie: Tie::None }
}

fn ans_points(q: String, ps: &[[f32; 3]]) -> Ans {
    ans_plain(q, format!("{ps:?}"), ps.iter().flat_map(|p| p.map(f32::to_bits)).collect())
}

fn ans_bodies(q: String, bs: &[BodyState]) -> Ans {
    let bits = bs.iter().flat_map(|b| b.translation.iter().chain(&b.rotation).chain(&b.linvel).map(|v| v.to_bits()).chain([b.sleeping as u32])).collect();
    ans_plain(q, format!("{bs:?}"), bits)
}

fn ans_character(s: &CharacterStep, c: &Character) -> Ans {
    let mut bits: Vec<u32> = s.feet.map(f32::to_bits).to_vec();
    bits.push(s.gap.to_bits());
    bits.extend(s.landed_speed.map(f32::to_bits));
    bits.extend(c.feet.map(f32::to_bits));
    bits.push(c.vertical_speed.to_bits());
    ans_plain("character".into(), format!("{s:?}"), bits)
}

/// The questions put to both worlds.
#[derive(Default)]
struct Probe {
    /// (origin, direction): each asked as `ray` and `ray_all`, default filter, 1000 units.
    rays: Vec<([f32; 3], [f32; 3])>,
    /// (shape, start, motion, filter) for `sweep`.
    sweeps: Vec<(Shape, Iso, [f32; 3], Filter)>,
    /// (shape, place) for `overlap`, default filter.
    overlaps: Vec<(Shape, Iso)>,
    /// Actors whose aim point and part centres are read (before the queries refresh).
    actors: Vec<u32>,
}

fn place(t: [f32; 3]) -> Iso {
    Iso { translation: t, rotation: ID }
}

/// A controller-sized capsule (18-unit radius, 128 tall) standing on its start point.
fn controller_capsule() -> Shape {
    Shape::Capsule { a: [0.0, 0.0, 18.0], b: [0.0, 0.0, 110.0], radius: 18.0 }
}

fn ask(w: &mut PhysWorld, p: &Probe) -> Vec<Ans> {
    let mut out = Vec::new();
    for &id in &p.actors {
        let aim = w.actor_aim_point(id);
        out.push(ans_points(format!("aim {id:#x}"), aim.as_slice()));
        out.push(ans_points(format!("part centres {id:#x}"), &w.actor_part_centers(id)));
    }
    let f = Filter::default();
    let q = w.queries();
    for (i, &(o, d)) in p.rays.iter().enumerate() {
        let all = q.ray_all(o, d, 1000.0, &f);
        out.push(ans_ray(format!("ray {i} {o:?}->{d:?}"), q.ray(o, d, 1000.0, &f), all.clone()));
        out.push(ans_ray_all(format!("ray_all {i} {o:?}->{d:?}"), all));
    }
    for (i, (s, from, m, filter)) in p.sweeps.iter().enumerate() {
        let hit = q.sweep(s, from, *m, filter).unwrap();
        // What the shape touches where it stopped: a tie's other candidate is there too.
        let touch = hit.map_or(Vec::new(), |h| {
            let at = Iso { translation: std::array::from_fn(|k| from.translation[k] + m[k] * h.fraction), rotation: from.rotation };
            q.overlap(&grown(s, TIE_TOUCH), &at, filter).unwrap()
        });
        out.push(ans_sweep(format!("sweep {i} from {:?} by {m:?} no_actors={}", from.translation, filter.no_actors), hit, touch));
    }
    for (i, (s, at)) in p.overlaps.iter().enumerate() {
        let o = q.overlap(s, at, &f).unwrap();
        out.push(ans_plain(format!("overlap {i} at {:?}", at.translation), format!("{o:?}"), Vec::new()));
    }
    out
}

/// Whether two answers differ: to the bit, or (`exact` false) where no tie reaches.
fn differ(x: &Ans, y: &Ans, exact: bool) -> bool {
    assert_eq!(x.q, y.q);
    if exact {
        (&x.shown, &x.bits) != (&y.shown, &y.bits)
    } else {
        x.loose != y.loose
    }
}

/// The kind of a question (its first word), for the summary.
fn kind(q: &str) -> &str {
    q.split(' ').next().unwrap_or(q)
}

fn summary(diffs: &[(String, String)], asked: usize) -> String {
    let mut kinds = std::collections::BTreeMap::<&str, usize>::new();
    for (k, _) in diffs {
        *kinds.entry(k.as_str()).or_default() += 1;
    }
    format!("{} of {asked} answers differ, by kind {kinds:?}:\n{}", diffs.len(), diffs.iter().take(40).map(|d| d.1.clone()).collect::<Vec<_>>().join("\n"))
}

#[test]
fn hitvol_tie_check_rejects_what_is_no_tie() {
    let hit = |form_id: u32, part: u8, distance: f32, normal: [f32; 3]| RayHit { owner: Owner::Actor { form_id, layer: 8, part }, distance, point: [distance, 0.0, 0.0], normal, material: None, shell: 0.0 };
    let ray = |h: RayHit, all: Vec<RayHit>| ans_ray("ray".into(), Some(h), all);
    let (a, b) = (hit(0x10, 0, 5.0, [-1.0, 0.0, 0.0]), hit(0x11, 0, 5.0, [-1.0, 0.0, 0.0]));
    // Both candidates in both worlds at one distance: a tie.
    assert!(tied(&ray(a, vec![a, b]), &ray(b, vec![b, a])).is_ok());
    // The other world's winner is not a hit here, or not at that distance: no tie.
    assert!(tied(&ray(a, vec![a]), &ray(b, vec![b, a])).is_err());
    assert!(tied(&ray(a, vec![a, hit(0x11, 0, 5.5, [-1.0, 0.0, 0.0])]), &ray(b, vec![b, a])).is_err());
    // Same owner, other normal, and only one such hit: no tie.
    let c = hit(0x10, 0, 5.0, [0.0, 1.0, 0.0]);
    assert!(tied(&ray(a, vec![a]), &ray(c, vec![c])).is_err());
    // `ray_all`: places swapped at one distance is a tie, across distances is not.
    let far = hit(0x12, 0, 9.0, [-1.0, 0.0, 0.0]);
    assert!(tied(&ans_ray_all("ray_all".into(), vec![a, b, far]), &ans_ray_all("ray_all".into(), vec![b, a, far])).is_ok());
    assert!(tied(&ans_ray_all("ray_all".into(), vec![a, far]), &ans_ray_all("ray_all".into(), vec![far, a])).is_err());
    // A sweep whose other winner is not touched where it stopped: no tie.
    let sw = |owner: Owner, touch: Vec<Owner>| ans_sweep("sweep".into(), Some(SweepHit { owner, fraction: 0.5, point: [0.0; 3], normal: [1.0, 0.0, 0.0] }), touch);
    assert!(tied(&sw(a.owner, vec![a.owner, b.owner]), &sw(b.owner, vec![a.owner, b.owner])).is_ok());
    assert!(tied(&sw(a.owner, vec![a.owner]), &sw(b.owner, vec![a.owner, b.owner])).is_err());
    // An answer with no tie in it never passes as one.
    assert!(tied(&ans_plain("aim".into(), "x".into(), vec![1]), &ans_plain("aim".into(), "y".into(), vec![2])).is_err());
}

/// An attached-capsule world and a standalone-capsule one built alike, and the
/// differences between their answers so far.
struct Pair {
    a: PhysWorld,
    s: PhysWorld,
    exact: bool,
    diffs: Vec<(String, String)>,
    asked: usize,
    /// Relaxed: answers that differed to the bit, each shown to be an exact tie ([`tied`]).
    ties: usize,
}

impl Pair {
    fn new(make: impl Fn() -> PhysWorld, exact: bool) -> Pair {
        let mut a = make();
        a.set_standalone_hit_volumes(false);
        let mut s = make();
        s.set_standalone_hit_volumes(true);
        Pair { a, s, exact, diffs: Vec::new(), asked: 0, ties: 0 }
    }

    fn act(&mut self, f: impl Fn(&mut PhysWorld)) {
        f(&mut self.a);
        f(&mut self.s);
    }

    fn compare(&mut self, ctx: &str, a: Vec<Ans>, s: Vec<Ans>) {
        assert_eq!(a.len(), s.len());
        for (x, y) in a.iter().zip(&s) {
            self.asked += 1;
            let why = if differ(x, y, self.exact) {
                Some("differs".to_string())
            } else if !self.exact && differ(x, y, true) {
                // Relaxed: a difference `loose` lets through must be an exact tie.
                self.ties += 1;
                tied(x, y).err().map(|e| format!("not an exact tie: {e}"))
            } else {
                None
            };
            if let Some(why) = why {
                self.diffs.push((kind(&x.q).to_string(), format!("{ctx}: {} ({why})\n    attached:   {}\n    standalone: {}", x.q, x.shown, y.shown)));
            }
        }
    }

    fn check(&mut self, ctx: &str, p: &Probe) {
        let (a, s) = (ask(&mut self.a, p), ask(&mut self.s, p));
        self.compare(ctx, a, s);
    }

    fn check_boxes(&mut self, ctx: &str) {
        let (a, s) = (ans_bodies("boxes".into(), &box_states(&self.a)), ans_bodies("boxes".into(), &box_states(&self.s)));
        let (da, ds) = (self.a.state_digest(), self.s.state_digest());
        let digest = |d: u64| ans_plain("digest".into(), format!("{d:#x}"), Vec::new());
        self.compare(ctx, vec![a, digest(da)], vec![s, digest(ds)]);
    }

    /// Panics listing every answer that differed.
    fn finish(self) {
        assert!(self.asked > 0);
        assert!(self.diffs.is_empty(), "{}", summary(&self.diffs, self.asked));
        println!("{} answers, all equal{}", self.asked, if self.exact { " to the bit".to_string() } else { format!(" modulo {} exact ties, each shown", self.ties) });
    }
}

/// Tie geometry on an actor standing at x = 200 + dx: part 0 holds part 7 (a ray starting
/// inside both hits each at distance 0); two segments of part 2 at y = +-3 and a part 3 /
/// part 4 pair at y = +-3 are mirror images, so a ray along y = 0 meets each pair at the
/// same distance with mirrored normals.
fn tie_parts(dx: f32) -> Vec<PartCapsule> {
    let p = |part: u8, a: [f32; 3], b: [f32; 3], radius: f32| PartCapsule { part, a: [a[0] + dx, a[1], a[2]], b: [b[0] + dx, b[1], b[2]], radius };
    vec![
        p(0, [200.0, 0.0, 60.0], [200.0, 0.0, 110.0], 15.0),
        p(7, [200.0, 0.0, 70.0], [200.0, 0.0, 100.0], 12.0),
        p(2, [260.0, 3.0, 40.0], [260.0, 3.0, 80.0], 10.0),
        p(2, [260.0, -3.0, 40.0], [260.0, -3.0, 80.0], 10.0),
        p(3, [320.0, 3.0, 40.0], [320.0, 3.0, 80.0], 10.0),
        p(4, [320.0, -3.0, 40.0], [320.0, -3.0, 80.0], 10.0),
    ]
}

/// Rays, sweeps and overlaps aimed at the ties of `tie_parts(dx)`.
fn tie_probe(dx: f32, actors: &[u32]) -> Probe {
    let x = |v: f32| v + dx;
    let ball = Shape::Sphere { radius: 4.0 };
    Probe {
        rays: vec![
            // Inside parts 0 and 7: both at distance 0.
            ([x(200.0), 0.0, 85.0], [1.0, 0.0, 0.0]),
            ([x(200.0), 0.0, 85.0], [-1.0, 0.0, 0.0]),
            ([x(200.0), 0.0, 85.0], [0.0, 1.0, 0.0]),
            ([x(200.0), 0.0, 85.0], [0.0, 0.0, 1.0]),
            // Grazing the two part-2 segments at equal distance (same owner).
            ([x(230.0), 0.0, 60.0], [1.0, 0.0, 0.0]),
            ([x(290.0), 0.0, 60.0], [-1.0, 0.0, 0.0]),
            // The part 3 / part 4 pair at equal distance (owners differ by part).
            ([x(290.0), 0.0, 60.0], [1.0, 0.0, 0.0]),
            ([x(350.0), 0.0, 60.0], [-1.0, 0.0, 0.0]),
            // Down through the middle of each pair.
            ([x(260.0), 0.0, 200.0], [0.0, 0.0, -1.0]),
            ([x(320.0), 0.0, 200.0], [0.0, 0.0, -1.0]),
            // Along the whole actor.
            ([0.0, 0.0, 60.0], [1.0, 0.0, 0.0]),
            ([0.0, 0.0, 90.0], [1.0, 0.0, 0.0]),
        ],
        sweeps: vec![
            (ball.clone(), place([x(230.0), 0.0, 60.0]), [100.0, 0.0, 0.0], Filter::default()),
            (ball.clone(), place([x(290.0), 0.0, 60.0]), [100.0, 0.0, 0.0], Filter::default()),
            (ball.clone(), place([x(200.0), 0.0, 85.0]), [50.0, 0.0, 0.0], Filter::default()),
            (controller_capsule(), place([x(100.0), 0.0, 0.5]), [300.0, 0.0, 0.0], Filter::default()),
            (controller_capsule(), place([x(100.0), 0.0, 0.5]), [300.0, 0.0, 0.0], Filter { no_actors: true, ..Default::default() }),
            (controller_capsule(), place([x(100.0), 0.0, 40.0]), [0.0, 0.0, -100.0], Filter::default()),
        ],
        overlaps: vec![
            (ball.clone(), place([x(200.0), 0.0, 85.0])),
            (ball, place([x(260.0), 0.0, 60.0])),
            (Shape::Box { half_extents: [80.0, 30.0, 60.0] }, place([x(260.0), 0.0, 60.0])),
        ],
        actors: actors.to_vec(),
    }
}

/// Scenario 1: Overlapping parts of one actor (a ray starting inside two capsules) and equidistant
/// ones (same part id, and different part ids).
fn one_actor_ties(exact: bool) {
    let mut p = Pair::new(ground, exact);
    p.act(|w| w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &tie_parts(0.0)));
    p.check("written", &tie_probe(0.0, &[0x10]));
    // The ties are exact in the geometry itself, so a divergence can only be their order.
    let both = p.a.queries().ray_all([230.0, 0.0, 60.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default());
    assert!(both.len() >= 2 && both[0].distance.to_bits() == both[1].distance.to_bits() && both[0].owner == both[1].owner && both[0].normal != both[1].normal, "{both:?}");
    let pair34 = p.a.queries().ray_all([290.0, 0.0, 60.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default());
    assert!(pair34.len() >= 2 && pair34[0].distance.to_bits() == pair34[1].distance.to_bits() && pair34[0].owner != pair34[1].owner, "{pair34:?}");
    let inside = p.a.queries().ray_all([200.0, 0.0, 85.0], [1.0, 0.0, 0.0], 1000.0, &Filter::default());
    assert_eq!(inside.iter().filter(|h| h.distance == 0.0).count(), 2, "{inside:?}");
    // Moved (every capsule written again; attached ones are re-inserted into the tree).
    p.act(|w| w.set_actor_parts(0x10, person([213.5, 1.25, 0.5], false), &tie_parts(13.5)));
    p.check("moved", &tie_probe(13.5, &[0x10]));
    p.act(|w| w.step());
    p.check("stepped", &tie_probe(13.5, &[0x10]));
    p.finish();
}

// Observed 2026-11-03 (before #1459): 29 of 105 answers differ (ray 16, ray_all 12, sweep 1), none on the
// first write, all from the first move on (and after the step). Inside parts 0 and 7
// (distance 0) the attached world answers part 0, the standalone one part 7 (after the move;
// after the step these agree); the sweep from there returns
// part 0 (witness z 85.9849) vs part 7 (85.729836). On the two part-2 segments (d 20.460611)
// the attached world gives normal y +0.3 first, the standalone one -0.3, in `ray` and in
// `ray_all`'s order; on the part 3 / part 4 pair (d 20.460594) attached part 4, standalone
// part 3. Distances, points, hit sets and fractions agree (`hitvol_ties_*` passes).
// Tie: an exact-TOI tie (coincident or mirrored capsules); kept `#[ignore]`d until #1459, whose
// canonical tie-break (owner, segment, hit geometry; physics.md §5.1) makes both worlds agree.
#[test]
fn hitvol_diff_overlapping_and_equidistant_parts_of_one_actor() {
    one_actor_ties(true);
}

#[test]
fn hitvol_ties_overlapping_and_equidistant_parts_of_one_actor() {
    one_actor_ties(false);
}

/// The canonical tie-break (#1459, physics.md §5.1): coincident actors written in either
/// order answer the same, the lower form id, then the lower part, then the earlier segment.
#[test]
fn first_hit_ties_break_by_owner_then_segment_whatever_the_order() {
    let make = |order: &[u32]| {
        let mut w = ground();
        for &id in order {
            let mut parts = tie_parts(0.0);
            if id == 0x11 {
                // Part 7 listed first, so the tree and the handles differ between the worlds.
                parts.reverse();
            }
            w.set_actor_parts(id, person([200.0, 0.0, 0.0], false), &parts);
        }
        w
    };
    let (mut w1, mut w2) = (make(&[0x10, 0x11]), make(&[0x11, 0x10]));
    let f = Filter::default();
    let ball = Shape::Sphere { radius: 4.0 };
    for w in [&mut w1, &mut w2] {
        let q = w.queries();
        // Inside parts 0 and 7 of both actors: four candidates at distance 0.
        let h = q.ray([200.0, 0.0, 85.0], [1.0, 0.0, 0.0], 1000.0, &f).unwrap();
        assert_eq!((h.owner, h.distance), (Owner::Actor { form_id: 0x10, layer: layers::BIPED, part: 0 }, 0.0));
        // The two part-2 segments of 0x10 at one distance: the first listed (y = +3, so the
        // ray at y = 0 meets it on its -y side).
        let h = q.ray([230.0, 0.0, 60.0], [1.0, 0.0, 0.0], 1000.0, &f).unwrap();
        assert_eq!(h.owner, Owner::Actor { form_id: 0x10, layer: layers::BIPED, part: 2 });
        assert!(h.normal[1] < 0.0, "{h:?}");
        let s = q.sweep(&ball, &place([200.0, 0.0, 85.0]), [50.0, 0.0, 0.0], &f).unwrap().unwrap();
        assert_eq!(s.owner, Owner::Actor { form_id: 0x10, layer: layers::BIPED, part: 0 });
    }
    let q1 = w1.queries();
    let q2 = w2.queries();
    for (o, d) in tie_probe(0.0, &[]).rays {
        let (a, b) = (q1.ray(o, d, 1000.0, &f), q2.ray(o, d, 1000.0, &f));
        assert_eq!(format!("{a:?}"), format!("{b:?}"), "ray {o:?} {d:?}");
        assert_eq!(format!("{:?}", q1.ray_all(o, d, 1000.0, &f)), format!("{:?}", q2.ray_all(o, d, 1000.0, &f)), "ray_all {o:?} {d:?}");
    }
    for (s, from, m, filter) in tie_probe(0.0, &[]).sweeps {
        assert_eq!(format!("{:?}", q1.sweep(&s, &from, m, &filter).unwrap()), format!("{:?}", q2.sweep(&s, &from, m, &filter).unwrap()), "sweep {from:?}");
    }
}

/// Scenario 2: Two actors whose capsules coincide exactly.
fn two_actors_coincide(exact: bool) {
    let mut p = Pair::new(ground, exact);
    p.act(|w| {
        w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &tie_parts(0.0));
        w.set_actor_parts(0x11, person([200.0, 0.0, 0.0], false), &tie_parts(0.0));
    });
    let probe = tie_probe(0.0, &[0x10, 0x11]);
    p.check("written", &probe);
    // The second actor written again (bit-different, same pose: -0.0 for 0.0), then both moved.
    p.act(|w| {
        let mut again = tie_parts(0.0);
        again[0].a[1] = -0.0;
        w.set_actor_parts(0x11, person([200.0, 0.0, 0.0], false), &again);
    });
    p.check("one rewritten", &probe);
    p.act(|w| {
        w.set_actor_parts(0x11, person([213.5, 1.25, 0.5], false), &tie_parts(13.5));
        w.set_actor_parts(0x10, person([213.5, 1.25, 0.5], false), &tie_parts(13.5));
    });
    p.check("moved", &tie_probe(13.5, &[0x10, 0x11]));
    p.act(|w| w.step());
    p.check("stepped", &tie_probe(13.5, &[0x10, 0x11]));
    p.finish();
}

// Observed 2026-11-03 (before #1459): 29 of 148 answers differ (ray 16, ray_all 12, sweep 1), none after the
// first write or the -0.0 rewrite, all from the move on. Inside parts 0 and 7 (distance 0):
// attached 0x11 part 0, standalone 0x10 part 7. On the part-2 segments (d 20.460611):
// attached 0x11 normal y +0.3, standalone 0x10 normal y -0.3; `ray_all` lists 0x10's two
// part-2 hits (same owner, same distance) in opposite orders. On the part 3 / 4 pair
// (d 20.460594): attached 0x11 part 4, standalone 0x10 part 3. The sweep from inside part 0/7
// picks a different capsule. Overlaps agree; `hitvol_ties_*` passes.
// Tie: an exact-TOI tie (coincident or mirrored capsules); kept `#[ignore]`d until #1459, whose
// canonical tie-break (owner, segment, hit geometry; physics.md §5.1) makes both worlds agree.
#[test]
fn hitvol_diff_two_actors_coincide() {
    two_actors_coincide(true);
}

#[test]
fn hitvol_ties_two_actors_coincide() {
    two_actors_coincide(false);
}

/// `ground()` with six dynamic boxes falling near the actors.
fn ground_with_boxes() -> PhysWorld {
    let mut w = ground();
    for i in 0..6u32 {
        let o = object(boxed([4.0, 4.0, 4.0]), Motion::Dynamic);
        w.add_ref(&RefPose { rotation: rz(i as f32 * 0.7), ..at(500 + i, [150.0 + (i % 3) as f32 * 40.0, -30.0 + (i / 3) as f32 * 60.0, 30.0 + i as f32 * 9.0]) }, &[o]);
    }
    w
}

fn box_states(w: &PhysWorld) -> Vec<BodyState> {
    (500..506).flat_map(|id| w.ref_bodies(id)).collect()
}

/// Scenario 3: A pending pose flushed straight into `step` (no query between the write and the
/// step), and two or three writes to one actor before one step.
fn pending_into_step(exact: bool) {
    let mut p = Pair::new(ground_with_boxes, exact);
    p.act(|w| {
        w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &tie_parts(0.0));
        w.step();
    });
    p.check_boxes("first write stepped");
    p.check("first write stepped", &tie_probe(0.0, &[0x10]));
    p.act(|w| {
        w.set_actor_parts(0x10, person([213.5, 1.25, 0.5], false), &tie_parts(13.5));
        w.step();
    });
    p.check_boxes("move stepped");
    p.check("move stepped", &tie_probe(13.5, &[0x10]));
    // Two writes to the same actor before one step; the second is where it ends up.
    p.act(|w| {
        w.set_actor_parts(0x10, person([180.0, 0.0, 0.0], false), &tie_parts(-20.0));
        w.set_actor_parts(0x10, person([226.0, -2.0, 0.0], false), &tie_parts(26.0));
        w.step();
    });
    p.check_boxes("two writes stepped");
    p.check("two writes stepped", &tie_probe(26.0, &[0x10]));
    // Three writes, back to an earlier pose, then several steps before the query.
    p.act(|w| {
        w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &tie_parts(0.0));
        w.set_actor_parts(0x10, person([226.0, -2.0, 0.0], false), &tie_parts(26.0));
        w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &tie_parts(0.0));
        for _ in 0..3 {
            w.step();
        }
    });
    p.check_boxes("three writes, three steps");
    p.check("three writes, three steps", &tie_probe(0.0, &[0x10]));
    p.finish();
}

// Observed 2026-11-03 (before #1459): 10 of 148 answers differ, none on the first write stepped and none
// after three writes and three steps; after "move stepped" and "two writes stepped" the ray
// starting inside parts 0 and 7 (4 directions, distance 0) answers part 0 attached vs part 7
// standalone, and the sweep from there part 0 (witness z 85.9849) vs part 7 (85.729836).
// Boxes and state digest agree on every step.
// Tie: an exact-TOI tie (coincident or mirrored capsules); kept `#[ignore]`d until #1459, whose
// canonical tie-break (owner, segment, hit geometry; physics.md §5.1) makes both worlds agree.
#[test]
fn hitvol_diff_pending_pose_flushed_straight_into_step() {
    pending_into_step(true);
}

#[test]
fn hitvol_ties_pending_pose_flushed_straight_into_step() {
    pending_into_step(false);
}

/// Scenario 4: A snapshot taken while poses are pending, restored and queried.
fn snapshot_pending(exact: bool) {
    let mut p = Pair::new(ground_with_boxes, exact);
    p.act(|w| w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &tie_parts(0.0)));
    p.check("written", &tie_probe(0.0, &[0x10]));
    // Pending (no refresh yet) when the copies are taken.
    p.act(|w| {
        w.set_actor_parts(0x10, person([213.5, 1.25, 0.5], false), &tie_parts(13.5));
        w.set_actor_parts(0x11, person([213.5, 1.25, 0.5], false), &tie_parts(13.5));
    });
    let (snap_a, snap_s) = (p.a.snapshot(), p.s.snapshot());
    // The originals go on; their copies are not touched by it.
    p.act(|w| {
        w.set_actor_parts(0x10, person([180.0, 0.0, 0.0], false), &tie_parts(-20.0));
        w.step();
    });
    p.check("originals went on", &tie_probe(-20.0, &[0x10, 0x11]));
    // Restored: the copies answer as their originals would have at the snapshot.
    (p.a, p.s) = (snap_a, snap_s);
    p.check("restored", &tie_probe(13.5, &[0x10, 0x11]));
    p.act(|w| w.step());
    p.check_boxes("restored and stepped");
    p.check("restored and stepped", &tie_probe(13.5, &[0x10, 0x11]));
    // A copy taken with poses pending and stepped before any query.
    p.act(|w| w.set_actor_parts(0x10, person([226.0, -2.0, 0.0], false), &tie_parts(26.0)));
    let (mut c_a, mut c_s) = (p.a.snapshot(), p.s.snapshot());
    c_a.step();
    c_s.step();
    (p.a, p.s) = (c_a, c_s);
    p.check_boxes("copy stepped while pending");
    p.check("copy stepped while pending", &tie_probe(26.0, &[0x10, 0x11]));
    p.finish();
}

// Observed 2026-11-03 (before #1459): 66 of 187 answers differ (ray 34, ray_all 24, sweep 8), none on the
// first write or in the copy stepped while pending; in the originals after the snapshot, in
// the restored copies and after their step. Restored, the ray from (213.5, 0, 85) along +x:
// attached 0x10 part 7, standalone 0x10 part 0 (d 0); from (0, 0, 60) along +x: attached
// 0x11 part 0, standalone 0x10 part 0 (d 198.5); the controller-capsule sweep from
// (113.5, 0, 0.5): attached 0x11, standalone 0x10 (f 0.22333331). Boxes and digest agree;
// `hitvol_ties_*` passes.
// Tie: an exact-TOI tie (coincident or mirrored capsules); kept `#[ignore]`d until #1459, whose
// canonical tie-break (owner, segment, hit geometry; physics.md §5.1) makes both worlds agree.
#[test]
fn hitvol_diff_snapshot_taken_with_poses_pending() {
    snapshot_pending(true);
}

#[test]
fn hitvol_ties_snapshot_taken_with_poses_pending() {
    snapshot_pending(false);
}

/// Scenario 5: An actor removed and added again (handles freed and reused) with the same poses, and
/// part-count changes.
fn removed_readded(exact: bool) {
    let mut p = Pair::new(ground, exact);
    p.act(|w| {
        w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &tie_parts(0.0));
        w.set_actor_parts(0x11, person([200.0, 0.0, 0.0], false), &tie_parts(0.0));
    });
    let probe = tie_probe(0.0, &[0x10, 0x11]);
    p.check("written", &probe);
    // Removed and added again with the same poses, a query between.
    p.act(|w| assert!(w.remove_actor_volume(0x10)));
    p.check("removed", &probe);
    p.act(|w| w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &tie_parts(0.0)));
    p.check("added again", &probe);
    // Removed and added again with no query between (handles freed and reused at once).
    p.act(|w| {
        assert!(w.remove_actor_volume(0x11));
        w.set_actor_parts(0x11, person([200.0, 0.0, 0.0], false), &tie_parts(0.0));
    });
    p.check("removed and added again unqueried", &probe);
    // One part more for 0x10 (every collider replaced), then one fewer than at first.
    p.act(|w| {
        let mut more = tie_parts(0.0);
        more.push(PartCapsule { part: 2, a: [260.0, 0.0, 85.0], b: [260.0, 0.0, 95.0], radius: 6.0 });
        w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &more);
    });
    assert_eq!((p.a.actor_part_count(0x10), p.s.actor_part_count(0x10)), (7, 7));
    p.check("a part more", &probe);
    p.act(|w| w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &tie_parts(0.0)[1..]));
    p.check("a part fewer", &probe);
    // To the single capsule and back to parts.
    p.act(|w| w.set_actor_volume(0x10, person([200.0, 0.0, 0.0], false)));
    p.check("single capsule", &probe);
    p.act(|w| w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), &tie_parts(0.0)));
    p.check("parts again", &probe);
    p.act(|w| w.step());
    p.check("stepped", &probe);
    assert_eq!(p.a.stats().colliders, p.s.stats().colliders);
    p.finish();
}

#[test]
fn hitvol_diff_removed_and_readded_actor_and_part_count_change() {
    removed_readded(true);
}

#[test]
fn hitvol_ties_removed_and_readded_actor_and_part_count_change() {
    removed_readded(false);
}

/// A seeded generator (64-bit LCG, Knuth's MMIX constants) for the crowd's poses.
struct Lcg(u64);

impl Lcg {
    fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

/// The 19 segments of a crowd member as (part, from, to, radius) about its feet, several
/// sharing a part id as limbs do.
const SKELETON: [(u8, [f32; 3], [f32; 3], f32); 19] = [
    (0, [0.0, 0.0, 70.0], [0.0, 0.0, 95.0], 14.0),
    (0, [0.0, 0.0, 95.0], [0.0, 0.0, 110.0], 13.0),
    (1, [0.0, 0.0, 115.0], [0.0, 0.0, 125.0], 8.0),
    (2, [0.0, 10.0, 105.0], [0.0, 22.0, 85.0], 5.0),
    (2, [0.0, 22.0, 85.0], [0.0, 24.0, 65.0], 4.5),
    (2, [0.0, 24.0, 65.0], [0.0, 24.0, 58.0], 3.0),
    (3, [0.0, -10.0, 105.0], [0.0, -22.0, 85.0], 5.0),
    (3, [0.0, -22.0, 85.0], [0.0, -24.0, 65.0], 4.5),
    (3, [0.0, -24.0, 65.0], [0.0, -24.0, 58.0], 3.0),
    (4, [0.0, 7.0, 65.0], [0.0, 8.0, 35.0], 7.0),
    (4, [0.0, 8.0, 35.0], [0.0, 8.0, 6.0], 5.5),
    (4, [0.0, 8.0, 6.0], [6.0, 8.0, 2.0], 3.0),
    (5, [0.0, -7.0, 65.0], [0.0, -8.0, 35.0], 7.0),
    (5, [0.0, -8.0, 35.0], [0.0, -8.0, 6.0], 5.5),
    (5, [0.0, -8.0, 6.0], [6.0, -8.0, 2.0], 3.0),
    (6, [0.0, 0.0, 110.0], [0.0, 0.0, 115.0], 6.0),
    (7, [-6.0, 0.0, 75.0], [-6.0, 0.0, 100.0], 9.0),
    (8, [3.0, 0.0, 122.0], [6.0, 0.0, 124.0], 2.0),
    (9, [-4.0, 0.0, 60.0], [-4.0, 0.0, 70.0], 10.0),
];

/// Actor `i`'s volume and parts for one frame: a wander about a grid spot and every joint
/// jittered, from `rng`.
fn crowd_pose(i: usize, rng: &mut Lcg) -> (HitVolume, Vec<PartCapsule>) {
    let home = [120.0 + (i % 5) as f32 * 70.0, -140.0 + (i / 5) as f32 * 70.0];
    let feet = [home[0] + rng.range(-25.0, 25.0), home[1] + rng.range(-25.0, 25.0), rng.range(-0.5, 0.5)];
    let mut parts = Vec::with_capacity(SKELETON.len());
    for &(part, a, b, radius) in &SKELETON {
        let mut j = || rng.range(-4.0, 4.0);
        let a = [feet[0] + a[0] + j(), feet[1] + a[1] + j(), feet[2] + a[2] + j()];
        let b = [feet[0] + b[0] + j(), feet[1] + b[1] + j(), feet[2] + b[2] + j()];
        parts.push(PartCapsule { part, a, b, radius });
    }
    (person(feet, i % 7 == 6), parts)
}

fn crowd_probe() -> Probe {
    let mut rays = Vec::new();
    for z in [8.0, 30.0, 60.0, 80.0, 100.0, 118.0] {
        for y in [-150.0, -100.0, -45.0, 0.0, 30.0, 95.0, 140.0] {
            rays.push(([0.0, y, z], [1.0, 0.0, 0.0]));
        }
    }
    for x in [130.0, 190.0, 265.0, 330.0, 405.0] {
        rays.push(([x, -250.0, 70.0], [0.0, 1.0, 0.0]));
        rays.push(([x, 20.0, 300.0], [0.0, 0.0, -1.0]));
        rays.push(([x, -120.0, 300.0], [0.1, 0.3, -1.0]));
    }
    let mut sweeps = Vec::new();
    for y in [-140.0, -70.0, 0.0, 70.0] {
        let from = place([20.0, y, 0.5]);
        sweeps.push((controller_capsule(), from, [450.0, 0.0, 0.0], Filter::default()));
        sweeps.push((controller_capsule(), from, [450.0, 0.0, 0.0], Filter { no_actors: true, ..Default::default() }));
        sweeps.push((Shape::Sphere { radius: 3.0 }, place([20.0, y, 90.0]), [450.0, 10.0, -20.0], Filter::default()));
    }
    let mut overlaps = Vec::new();
    for (x, y) in [(120.0, -140.0), (260.0, -70.0), (330.0, 0.0), (190.0, 70.0), (400.0, 70.0)] {
        overlaps.push((Shape::Sphere { radius: 40.0 }, place([x, y, 70.0])));
        overlaps.push((controller_capsule(), place([x + 20.0, y, 0.5])));
    }
    Probe { rays, sweeps, overlaps, actors: vec![0x100, 0x105, 0x10c, 0x113] }
}

/// Scenario 6: 20 actors of 19 parts each, posed from a seeded generator over 50 frames (every actor
/// written each frame before any query), with falling boxes and a walking character; every
/// third frame the writes go straight into the step. With `twins`, the last two actors copy
/// the first two exactly (coincident actors in a full tree).
fn crowd(twins: bool, exact: bool) {
    let mut p = Pair::new(ground_with_boxes, exact);
    let params = CharacterParams::default();
    let mut ca = p.a.add_character(9, params, [30.0, -100.0, 0.0]);
    let mut cs = p.s.add_character(9, params, [30.0, -100.0, 0.0]);
    let probe = crowd_probe();
    let mut rng = Lcg(0x5eed_2026_1103);
    for frame in 0..50 {
        let mut poses: Vec<(HitVolume, Vec<PartCapsule>)> = (0..20).map(|i| crowd_pose(i, &mut rng)).collect();
        if twins {
            poses[18] = poses[0].clone();
            poses[19] = poses[1].clone();
        }
        p.act(|w| {
            for (i, (v, parts)) in poses.iter().enumerate() {
                w.set_actor_parts(0x100 + i as u32, *v, parts);
            }
        });
        let input = CharacterInput { velocity: [120.0, 25.0 * ((frame % 9) as f32 - 4.0)], ..Default::default() };
        let ctx = format!("frame {frame}");
        let straight = frame % 3 == 0;
        if straight {
            // Straight into the step, then the character and the queries.
            p.act(|w| w.step());
        } else {
            p.act(|w| w.update_queries());
        }
        let (sa, ss) = (p.a.move_character(&mut ca, input), p.s.move_character(&mut cs, input));
        let (xa, xs) = (ans_character(&sa, &ca), ans_character(&ss, &cs));
        p.compare(&ctx, vec![xa], vec![xs]);
        p.check(&ctx, &probe);
        if !straight {
            p.act(|w| w.step());
        }
        p.check_boxes(&ctx);
    }
    assert_eq!(p.a.stats().steps, 50);
    assert_eq!((p.a.actor_volumes().count(), p.a.actor_part_count(0x100)), (20, 19));
    p.finish();
}

#[test]
fn hitvol_diff_crowd_of_twenty_over_fifty_frames() {
    crowd(false, true);
}

#[test]
fn hitvol_ties_crowd_of_twenty_over_fifty_frames() {
    crowd(false, false);
}

// Observed 2026-11-03 (before #1459): 205 of 7350 answers differ (ray 173, sweep 32), all first hits and winners
// that pick the other twin at the same distance (e.g. frame 1, ray from (0, -150, 30) along
// +x: attached 0x100 part 5, standalone 0x112 part 5, both d 127.61765); boxes, digest,
// character, overlaps and ray_all agree.
// Tie: an exact-TOI tie (coincident or mirrored capsules); kept `#[ignore]`d until #1459, whose
// canonical tie-break (owner, segment, hit geometry; physics.md §5.1) makes both worlds agree.
#[test]
fn hitvol_diff_crowd_with_coincident_twins() {
    crowd(true, true);
}

#[test]
fn hitvol_ties_crowd_with_coincident_twins() {
    crowd(true, false);
}

/// Scenario 7: A's skip: the same pose written twice leaves the world alone. The public API can force
/// that write: -0.0 for 0.0 in a segment end defeats the bit comparison
/// [design: crates/fnv-phys/src/actors.rs:108-111] yet hands rapier the very same pose,
/// since the mid-point (-0.0 + 0.0 = +0.0) and the direction (0.0 - -0.0 = +0.0) are
/// unchanged [design: crates/fnv-phys/src/actors.rs:251-253]. Four worlds: attached and
/// standalone, each skipped and forced; all must answer as the attached skipped one. A forced
/// write is what every write did before the skip existed.
fn skipped_vs_forced(exact: bool) {
    let pose = |w: &mut PhysWorld, parts: &[PartCapsule]| {
        w.set_actor_parts(0x10, person([200.0, 0.0, 0.0], false), parts);
        w.set_actor_parts(0x11, person([200.0, 0.0, 0.0], false), &tie_parts(0.0));
    };
    let forced: Vec<PartCapsule> = tie_parts(0.0)
        .into_iter()
        .map(|mut c| {
            if c.a[1] == 0.0 {
                c.a[1] = -0.0;
            }
            c
        })
        .collect();
    assert_eq!(forced.iter().filter(|c| c.a[1].to_bits() == (-0.0f32).to_bits()).count(), 2);
    let names = ["attached skipped", "standalone skipped", "attached forced", "standalone forced"];
    let mut worlds: Vec<PhysWorld> = (0..4)
        .map(|i| {
            let mut w = ground_with_boxes();
            w.set_standalone_hit_volumes(i % 2 == 1);
            w
        })
        .collect();
    let probe = tie_probe(0.0, &[0x10, 0x11]);
    let mut diffs = Vec::new();
    let (mut asked, mut ties) = (0, 0);
    for frame in 0..5 {
        for (i, w) in worlds.iter_mut().enumerate() {
            // Frames 1 and 3: the skipped worlds write the same pose again, the forced ones
            // force it.
            let force = i >= 2 && frame % 2 == 1;
            let plain = tie_parts(0.0);
            pose(w, if force { &forced } else { &plain });
            if frame == 2 {
                w.step();
            }
        }
        let answers: Vec<Vec<Ans>> = worlds
            .iter_mut()
            .map(|w| {
                let mut a = ask(w, &probe);
                a.push(ans_bodies("boxes".into(), &box_states(w)));
                a.push(ans_plain("digest".into(), format!("{:#x}", w.state_digest()), Vec::new()));
                a
            })
            .collect();
        for (i, other) in answers.iter().enumerate().skip(1) {
            for (x, y) in answers[0].iter().zip(other) {
                asked += 1;
                let why = if differ(x, y, exact) {
                    Some("differs".to_string())
                } else if !exact && differ(x, y, true) {
                    // Relaxed: a difference `loose` lets through must be an exact tie.
                    ties += 1;
                    tied(x, y).err().map(|e| format!("not an exact tie: {e}"))
                } else {
                    None
                };
                if let Some(why) = why {
                    diffs.push((format!("{} {}", names[i], kind(&x.q)), format!("frame {frame}: {} ({why})\n    attached skipped: {}\n    {}: {}", x.q, x.shown, names[i], y.shown)));
                }
            }
        }
    }
    // The skipped worlds wrote once (frame 0); the forced ones wrote 0x10 again on every later
    // frame (frames 1 and 3 forced, 2 and 4 back from the forced bits).
    let written: Vec<u64> = worlds.iter().map(|w| w.refresh_stats().hitvol_written).collect();
    assert_eq!((written[0], written[1], written[2], written[3]), (12, 12, 36, 36), "{written:?}");
    assert!(diffs.is_empty(), "{}", summary(&diffs, asked));
    println!("{asked} answers, all equal{}", if exact { " to the bit".to_string() } else { format!(" modulo {ties} exact ties, each shown") });
}

// Observed 2026-11-03 (before #1459): 61 of 585 answers differ (ray 46, sweep 15), all of them "attached
// forced" against "attached skipped" (standalone skipped and standalone forced both answer as
// attached skipped, to the bit): from frame 1
// on, the first hits on every tie pick the other coincident actor, e.g. the ray from
// (200, 0, 85) along +x: skipped 0x10 part 7, forced 0x11 part 7, both d 0; the ray from
// (0, 0, 60) along +x: skipped 0x11 part 0, forced 0x10 part 0, both d 185.00002; sweeps
// likewise. So the skip (A) changes which of two exactly tied hits wins compared with the
// old always-write behaviour: not a zero change on exact ties.
// Tie: an exact-TOI tie (coincident or mirrored capsules); kept `#[ignore]`d until #1459, whose
// canonical tie-break (owner, segment, hit geometry; physics.md §5.1) makes both worlds agree.
#[test]
fn hitvol_diff_skipped_write_answers_as_a_forced_one() {
    skipped_vs_forced(true);
}

#[test]
fn hitvol_ties_skipped_write_answers_as_a_forced_one() {
    skipped_vs_forced(false);
}

/// An actor's character standing at the origin of [`ground`], for where it can be put.
fn spot_character(w: &mut PhysWorld) -> Character {
    w.add_character(1, CharacterParams { max_slope_deg: 60.0, ..CharacterParams::default() }, [0.0, 0.0, 10.0])
}

const SPOT: [f32; 3] = [500.0, -500.0, 0.0];

#[test]
fn a_spot_on_open_ground_is_where_a_character_rests_and_stays() {
    let mut w = ground();
    let mut c = spot_character(&mut w);
    let feet = w.character_spot(&c, SPOT).unwrap();
    assert!((feet[0], feet[1]) == (SPOT[0], SPOT[1]));
    assert!((feet[2] - TERRAIN_SHELL).abs() < 1e-3, "{feet:?}");
    w.place_character(&mut c, feet);
    settle(&mut w, &mut c, 30);
    assert!((c.feet[2] - feet[2]).abs() < 0.5, "{:?} from {feet:?}", c.feet);
}

#[test]
fn a_prop_in_the_way_leaves_no_room_for_the_body_and_a_way_past_is_not_asked_for() {
    let mut w = ground();
    let c = spot_character(&mut w);
    // A wall between the character and the place does not count.
    w.add_ref(&at(60, [250.0, -250.0, 80.0]), &[object(boxed([20.0, 200.0, 80.0]), Motion::Static)]);
    assert!(w.character_spot(&c, SPOT).is_ok());
    // A pillar beside the place, closer than the body's radius: no room.
    w.add_ref(&at(61, [SPOT[0] + 30.0, SPOT[1], 80.0]), &[object(boxed([20.0, 20.0, 80.0]), Motion::Static)]);
    assert_eq!(w.character_spot(&c, SPOT), Err(SpotRefused::NoRoom));
    // Further along: room again.
    assert!(w.character_spot(&c, [SPOT[0] - 40.0, SPOT[1], 0.0]).is_ok());
}

#[test]
fn a_ceiling_lower_than_the_body_leaves_no_room() {
    let mut w = ground();
    let c = spot_character(&mut w);
    // The underside 100 up, above the ray's start (48): the ground is found, the body
    // does not fit under it.
    w.add_ref(&at(60, [SPOT[0], SPOT[1], 110.0]), &[object(boxed([100.0, 100.0, 10.0]), Motion::Static)]);
    assert_eq!(w.character_spot(&c, SPOT), Err(SpotRefused::NoRoom));
    // A lower one, the ray's start above it: the ground is still the level asked for,
    // and the body does not stand on top of what is over it instead.
    let mut w = ground();
    let c = spot_character(&mut w);
    w.add_ref(&at(61, [SPOT[0], SPOT[1], 25.0]), &[object(boxed([100.0, 100.0, 5.0]), Motion::Static)]);
    assert_eq!(w.character_spot(&c, SPOT), Err(SpotRefused::NoRoom));
    // Higher than the body: room under it.
    let mut w = ground();
    let c = spot_character(&mut w);
    w.add_ref(&at(62, [SPOT[0], SPOT[1], 150.0]), &[object(boxed([100.0, 100.0, 10.0]), Motion::Static)]);
    assert!(w.character_spot(&c, SPOT).is_ok());
}

#[test]
fn a_skin_laid_on_the_landscape_takes_the_body_a_slab_over_it_does_not() {
    // A road: one sheet of triangles 20 over the ground (#4826). Asked at the ground's
    // height, as a coarse navmesh under the road has it: the body overlaps the sheet
    // there, so it is put on the sheet.
    let mut w = ground();
    let c = spot_character(&mut w);
    let road = Shape::TriMesh {
        vertices: vec![[-150.0, -150.0, 0.0], [150.0, -150.0, 0.0], [150.0, 150.0, 0.0], [-150.0, 150.0, 0.0]],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        materials: vec![0; 2],
    };
    w.add_ref(&at(80, [SPOT[0], SPOT[1], 20.0]), &[object(road, Motion::Static)]);
    let on = w.character_spot(&c, SPOT);
    // On the sheet, at most a step over it (the place is tried a share of a step up).
    assert!(matches!(on, Ok(f) if f[2] > 20.0 && f[2] < 20.0 + CharacterParams::default().step_height + 2.0), "{on:?}");
    // A slab with room under its top (its underside 20 up, its top 30): still none.
    let mut w = ground();
    let c = spot_character(&mut w);
    w.add_ref(&at(81, [SPOT[0], SPOT[1], 25.0]), &[object(boxed([100.0, 100.0, 5.0]), Motion::Static)]);
    assert_eq!(w.character_spot(&c, SPOT), Err(SpotRefused::NoRoom));
}

#[test]
fn a_spot_is_on_the_level_asked_for_of_those_stacked() {
    let mut w = ground();
    let c = spot_character(&mut w);
    // A floor 200 above the ground: each height asked for gets its own.
    w.add_ref(&at(60, [SPOT[0], SPOT[1], 195.0]), &[object(boxed([60.0, 60.0, 5.0]), Motion::Static)]);
    let upper = w.character_spot(&c, [SPOT[0], SPOT[1], 200.0]).unwrap();
    assert!((upper[2] - 200.0).abs() < 2.0, "{upper:?}");
    let lower = w.character_spot(&c, [SPOT[0], SPOT[1], 5.0]).unwrap();
    assert!((lower[2] - TERRAIN_SHELL).abs() < 1e-3, "{lower:?}");
    // Two floors of one mesh, 30 apart inside the window: one ray meets only the upper.
    let mut w = ground();
    let c = spot_character(&mut w);
    let plate = |z: f32, base: u32| ([[-80.0, -80.0, z], [80.0, -80.0, z], [80.0, 80.0, z], [-80.0, 80.0, z]], [[base, base + 1, base + 2], [base, base + 2, base + 3]]);
    let (upper, ut) = plate(0.0, 0);
    let (under, lt) = plate(-30.0, 4);
    let vertices = upper.iter().chain(&under).copied().collect();
    let shape = Shape::TriMesh { vertices, triangles: vec![ut[0], ut[1], lt[0], lt[1]], materials: vec![0; 4] };
    w.add_ref(&at(70, [SPOT[0], SPOT[1], 300.0]), &[object(shape, Motion::Static)]);
    let on = w.character_spot(&c, [SPOT[0], SPOT[1], 302.0]);
    assert!(matches!(on, Ok(f) if (f[2] - 300.0).abs() < 2.0), "{on:?}");
    assert_eq!(w.character_spot(&c, [SPOT[0], SPOT[1], 272.0]), Err(SpotRefused::NoRoom), "under the upper floor");
    // With two surfaces inside the window, the one nearer the height asked for.
    let mut w = ground();
    let c = spot_character(&mut w);
    w.add_ref(&at(61, [SPOT[0], SPOT[1], 195.0]), &[object(boxed([60.0, 60.0, 5.0]), Motion::Static)]);
    w.add_ref(&at(62, [SPOT[0], SPOT[1], 135.0]), &[object(boxed([60.0, 60.0, 5.0]), Motion::Static)]);
    let lower = w.character_spot(&c, [SPOT[0], SPOT[1], 145.0]);
    assert_eq!(lower, Err(SpotRefused::NoRoom), "on the lower slab, 60 under the upper");
    let upper = w.character_spot(&c, [SPOT[0], SPOT[1], 198.0]).unwrap();
    assert!((upper[2] - 200.0).abs() < 2.0, "{upper:?}");
}

#[test]
fn nothing_to_stand_on_near_the_height_is_refused() {
    let mut w = ground();
    let c = spot_character(&mut w);
    // The terrain is 500 below: too far to be the level asked for.
    assert_eq!(w.character_spot(&c, [SPOT[0], SPOT[1], 500.0]), Err(SpotRefused::NoSupport));
    assert_eq!(w.character_spot(&c, [f32::NAN, 0.0, 0.0]), Err(SpotRefused::NoSupport));
    // A wall's steep face is no support either.
    let mut w = PhysWorld::new(PhysParams::default());
    let c = spot_character(&mut w);
    let (s, co) = 80f32.to_radians().sin_cos();
    let rot = [co, 0.0, -s, 0.0, 1.0, 0.0, s, 0.0, co];
    w.add_ref(&RefPose { form_id: 60, position: [0.0, 0.0, 0.0], rotation: rot, scale: 1.0 }, &[object(boxed([100.0, 100.0, 5.0]), Motion::Static)]);
    assert_eq!(w.character_spot(&c, [0.0, 0.0, 0.0]), Err(SpotRefused::NoSupport));
}

#[test]
fn other_characters_block_a_spot_and_the_body_itself_does_not() {
    let mut w = ground();
    let c = spot_character(&mut w);
    // Its own place: it does not stand in its own way.
    assert!(w.character_spot(&c, [0.0, 0.0, 0.0]).is_ok());
    let other = w.add_character(2, CharacterParams::default(), [SPOT[0] + 10.0, SPOT[1], TERRAIN_SHELL]);
    assert_eq!(w.character_spot(&c, SPOT), Err(SpotRefused::NoRoom));
    assert_eq!(w.queries().character_overlap(&c, [SPOT[0], SPOT[1], TERRAIN_SHELL]), vec![Owner::Character { id: 2 }]);
    w.remove_character(other);
    assert!(w.character_spot(&c, SPOT).is_ok());
}

#[test]
fn only_what_a_character_cannot_push_blocks_a_spot() {
    let mut w = ground();
    let c = spot_character(&mut w);
    // A crate it would push aside.
    let mut light = object(boxed([10.0, 10.0, 10.0]), Motion::Dynamic);
    light.mass = 20.0;
    w.add_ref(&at(60, [SPOT[0], SPOT[1], 10.5]), &[light]);
    let feet = w.character_spot(&c, SPOT).unwrap();
    assert!((feet[2] - TERRAIN_SHELL).abs() < 1e-3, "not on top of it: {feet:?}");
    // A car it stops at.
    let mut w = ground();
    let c = spot_character(&mut w);
    let mut heavy = object(boxed([60.0, 30.0, 30.0]), Motion::Dynamic);
    heavy.mass = 800.0;
    w.add_ref(&at(61, [SPOT[0], SPOT[1], 30.5]), &[heavy]);
    assert_eq!(w.character_spot(&c, SPOT), Err(SpotRefused::NoRoom));
}

#[test]
fn a_slope_it_can_walk_has_room_for_the_controllers_own_shape() {
    // A capsule's round bottom at the gap kept would sink into a slope this steep; the
    // controller's cone does not.
    let mut w = PhysWorld::new(PhysParams::default());
    let c = spot_character(&mut w);
    ramp(&mut w, 45.0);
    let (x, z) = (300.0, 200.0);
    let feet = w.character_spot(&c, [x, 0.0, z]).unwrap();
    assert!((feet[2] - z).abs() < 26.0, "{feet:?}");
    let capsule = Shape::Capsule { a: [0.0, 0.0, 22.0], b: [0.0, 0.0, 106.9], radius: 22.0 };
    let pose = Iso { translation: [x, 0.0, z + 0.5], ..Iso::IDENTITY };
    assert!(!w.queries().overlap(&capsule, &pose, &Filter::excluding(c.key())).unwrap().is_empty());
}

#[test]
fn a_stair_is_stood_on_at_the_height_its_next_step_lifts_the_body_to() {
    let mut w = ground();
    let mut c = spot_character(&mut w);
    // Steps 16 high and 16 deep, rising to the east from x = 0: the ray finds the tread
    // under the feet, the cone meets the next step's edge at that height.
    for i in 0..10u32 {
        let top = 16.0 * (i + 1) as f32;
        let x = 16.0 * i as f32 + 8.0;
        w.add_ref(&at(70 + i, [SPOT[0] + x, SPOT[1], top / 2.0]), &[object(boxed([8.0, 100.0, top / 2.0]), Motion::Static)]);
    }
    let at_step = [SPOT[0] + 40.0, SPOT[1], 48.0];
    let feet = w.character_spot(&c, at_step).unwrap();
    assert!(feet[2] > 48.0, "{feet:?}");
    assert!(feet[2] <= 48.5 + c.params.step_height, "{feet:?}");
    // It stays about there.
    w.place_character(&mut c, feet);
    settle(&mut w, &mut c, 30);
    assert!((c.feet[0] - feet[0]).abs() < 30.0 && (c.feet[2] - feet[2]).abs() < 40.0, "{:?} from {feet:?}", c.feet);
}

#[test]
fn a_steep_hill_it_can_walk_has_ground_and_room_and_a_body_in_the_hill_has_none() {
    let mut w = PhysWorld::new(PhysParams::default());
    let heights: Vec<f32> = (0..SIDE * SIDE).map(|i| ((i % SIDE) as f32 * 128.0).min(1200.0)).collect();
    w.add_terrain((0, 0), &heights);
    let c = spot_character(&mut w);
    let (x, y) = (1000.0, 2000.0);
    let z = height_at(&heights, x, y).unwrap();
    let got = w.character_spot(&c, [x, y, z]);
    // Slope 45 degrees (rise of one per unit), within this character's 60.
    assert!(got.is_ok(), "{got:?}");
    let feet = got.unwrap();
    assert!((feet[2] - z).abs() <= c.params.step_height + TERRAIN_SHELL + 1.0, "{feet:?} on {z}");
    assert_eq!(w.queries().character_overlap(&c, [x, y, z - 40.0]), vec![Owner::Terrain { x: 0, y: 0 }]);
}

/// Put down over a light crate with no room to get out of the way (against a wall): the
/// body is accepted, as it is over any clutter it would push aside, so this has to come
/// out sanely. Nothing flies off or goes through the wall, and it walks away.
#[test]
fn a_character_put_over_light_clutter_pinned_against_a_wall_walks_on() {
    let mut w = ground();
    let mut c = spot_character(&mut w);
    let wall_face = SPOT[0] + 26.0;
    w.add_ref(&at(60, [wall_face + 10.0, SPOT[1], 100.0]), &[object(boxed([10.0, 200.0, 100.0]), Motion::Static)]);
    let mut crate_ = object(boxed([10.0, 10.0, 10.0]), Motion::Dynamic);
    crate_.mass = 20.0;
    w.add_ref(&at(61, [SPOT[0] + 15.0, SPOT[1], 10.0 + TERRAIN_SHELL]), &[crate_]);
    for _ in 0..120 {
        w.step();
    }
    let rest = w.ref_bodies(61)[0];
    assert!(rest.translation[0] + 10.0 <= wall_face + 0.5, "{rest:?}");
    let feet = w.character_spot(&c, SPOT).unwrap();
    assert!((feet[2] - TERRAIN_SHELL).abs() < 1e-3, "{feet:?}");
    // It does overlap the crate.
    let capsule = Shape::Capsule { a: [0.0, 0.0, 22.0], b: [0.0, 0.0, 106.9], radius: 22.0 };
    let pose = Iso { translation: feet, ..Iso::IDENTITY };
    assert!(w.queries().overlap(&capsule, &pose, &Filter::excluding(c.key())).unwrap().iter().any(|o| matches!(o, Owner::Ref { form_id: 61, .. })));
    w.place_character(&mut c, feet);
    for _ in 0..240 {
        w.move_character(&mut c, CharacterInput::default());
        w.step();
        let b = w.ref_bodies(61)[0];
        assert!(b.translation.iter().chain(&b.linvel).chain(&c.feet).all(|v| v.is_finite()), "{b:?} {:?}", c.feet);
        assert!(b.linvel.iter().all(|v| v.abs() < 2000.0), "flung: {b:?}");
        assert!(b.translation[0] + 10.0 < wall_face + 5.0 && b.translation[2] > 0.0, "through something: {b:?}");
        assert!((c.feet[0] - feet[0]).abs() < 30.0 && (c.feet[1] - feet[1]).abs() < 30.0 && (c.feet[2] - feet[2]).abs() < 40.0, "{:?} from {feet:?}", c.feet);
    }
    // Along the wall, the way a path past it would go.
    let from = c.feet;
    walk(&mut w, &mut c, [0.0, 150.0], 120);
    assert!(c.feet[1] - from[1] > 50.0, "{:?} from {from:?}", c.feet);
    assert!((c.feet[2] - from[2]).abs() < 40.0, "{:?} from {from:?}", c.feet);
}

/// A groove of two planes rising at `deg` from level either side of the line x = 0,
/// z = 100, 800 long in y: walls steeper than walkable meeting at its bottom, the crease a
/// character stuck in for the controller's every sweep (issue #1797).
fn groove(deg: f32) -> PhysWorld {
    let mut w = ground();
    let (h, d) = (300.0, 300.0 / deg.to_radians().tan());
    let vertices = vec![[-d, -400.0, 100.0 + h], [-d, 400.0, 100.0 + h], [0.0, -400.0, 100.0], [0.0, 400.0, 100.0], [d, -400.0, 100.0 + h], [d, 400.0, 100.0 + h]];
    let triangles = vec![[0, 2, 1], [1, 2, 3], [2, 4, 3], [3, 4, 5]];
    w.add_ref(&at(1, [0.0, 0.0, 0.0]), &[object(Shape::TriMesh { vertices, triangles, materials: vec![0; 4] }, Motion::Static)]);
    w
}

#[test]
fn a_stuck_character_is_held_with_the_steps_it_would_have_had() {
    // Dropped into the groove and walked: along it a character gets some way and sticks,
    // into a wall it sticks at once [observed: this test, rapier 0.36.0]. Held, it makes the
    // same steps without sweeping again. Since gravity is exact over a step (2026-11-11) a
    // standing character presses into the crease half as far each step (½·g·dt²), and in the
    // 70° and 80° grooves a walk into a wall rides a couple of units up it and creeps
    // (0.01 units a step) instead of sticking [observed: this test, rapier 0.36.1]; the 52°
    // groove keeps the walks that stick.
    let mut held = 0;
    for deg in [50.0f32, 52.0, 60.0, 70.0, 80.0] {
        for v in [[0.0, 0.0], [150.0, 0.0], [150.0, 150.0], [0.0, 150.0], [-150.0, 30.0]] {
            let run = |hold: bool| {
                let mut w = groove(deg);
                let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 400.0]);
                c.hold_when_stuck = hold;
                settle(&mut w, &mut c, 120);
                let steps: Vec<_> = walk(&mut w, &mut c, v, 120).into_iter().map(|s| (s.feet, s.grounded, s.landed_speed, s.swimming)).collect();
                (steps, c.held_steps)
            };
            let ((free, none), (kept, n)) = (run(false), run(true));
            assert_eq!(none, 0);
            assert_eq!(free, kept, "{deg} degrees, {v:?}");
            held += n;
        }
    }
    assert!(held > 1000, "held {held} steps");
}

#[test]
fn a_held_character_moves_on_when_the_way_is_free() {
    // A walk into the 50° groove's wall sticks at once (the 70° one's no longer does with
    // exact gravity, `a_stuck_character_is_held_with_the_steps_it_would_have_had`).
    let mut w = groove(50.0);
    let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 400.0]);
    c.hold_when_stuck = true;
    settle(&mut w, &mut c, 120);
    walk(&mut w, &mut c, [150.0, 0.0], 30);
    assert!(c.held_steps > 0);
    let z = c.feet[2];
    // The groove gone, it falls to the ground within a retry's steps.
    w.remove_ref(1);
    let retry = (STUCK_RETRY_SECS * 60.0).round() as usize;
    let steps = walk(&mut w, &mut c, [150.0, 0.0], retry + 60);
    assert!(steps[..retry].iter().any(|s| s.feet[2] < z - 1.0), "{:?}", c.feet);
    assert!(c.grounded && c.feet[2] < TERRAIN_SHELL + 1.0 && c.feet[0] > 50.0, "{:?}", c.feet);
}

#[test]
fn a_held_character_waits_the_same_seconds_at_any_step_rate() {
    // The retry is a third of a second: 20 steps at 60 a second, 40 at 120, 80 at 240.
    // Whether a walk in the groove sticks at all depends on the step's length (a shorter
    // move ran out of the controller's sweeps in fewer of these walks [observed: this
    // test, rapier 0.36.0]), so each rate takes the first walk that sticks. With exact
    // gravity a standing character presses into the crease by ½·g·dt² a step, 0.006 units
    // at 240 Hz, and there only a walk along the 50° crease still sticks, after 1.9 s
    // [observed: this test, rapier 0.36.1].
    for hz in [60.0f32, 120.0, 152.0, 240.0] {
        let stuck = [50.0f32, 60.0, 70.0, 80.0].into_iter().flat_map(|deg| [[150.0, 0.0], [150.0, 150.0], [-150.0, 30.0], [300.0, 0.0], [0.0, 150.0]].map(|v| (deg, v))).find_map(|(deg, v)| {
            let mut w = groove(deg);
            w.set_step(1.0 / hz);
            let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 400.0]);
            c.hold_when_stuck = true;
            settle(&mut w, &mut c, (2.0 * hz) as usize);
            let mut steps = 0;
            while c.held_steps == 0 && steps < (2.0 * hz) as usize {
                walk(&mut w, &mut c, v, 1);
                steps += 1;
            }
            (c.held_steps > 0).then_some((w, c, v))
        });
        let Some((mut w, mut c, v)) = stuck else { panic!("{hz} Hz: no walk in the groove stuck") };
        // Held, then the groove goes: the steps are held until the full move a retry after
        // the move that stuck, R steps at this rate (the R - 1 between are held).
        assert_eq!(c.held_steps, 1, "{hz} Hz");
        w.remove_ref(1);
        let z = c.feet[2];
        let mut n = 0;
        while c.held_steps as usize == n + 1 && n < hz as usize {
            walk(&mut w, &mut c, v, 1);
            n += 1;
        }
        let r = (STUCK_RETRY_SECS * hz).round() as u32;
        assert_eq!(c.held_steps, r - 1, "{hz} Hz: held {} steps", c.held_steps);
        // And the full move went on: down to the ground, where the groove was.
        walk(&mut w, &mut c, v, hz as usize);
        assert!(c.feet[2] < z - 1.0, "{hz} Hz: {:?}", c.feet);
    }
}

#[test]
fn a_duration_summed_from_steps_is_reached_on_the_step_that_makes_it() {
    for hz in [60.0f32, 120.0, 144.0, 152.0, 160.0, 240.0] {
        let dt = 1.0 / hz;
        let want = (STUCK_RETRY_SECS * hz).round() as usize;
        let mut t = 0.0f32;
        let n = (1..1000).find(|_| {
            t += dt;
            secs_reached(t, STUCK_RETRY_SECS, dt)
        });
        assert_eq!(n, Some(want), "{hz} Hz");
    }
}

#[test]
fn holding_changes_nothing_for_characters_that_are_not_stuck() {
    // Steps, a wall to slide along, a ramp too steep and a corner: the hold never comes.
    type Scene = (fn(&mut PhysWorld), [f32; 2]);
    let scenes: Vec<Scene> = vec![
        (|w| { w.add_ref(&at(1, [300.0, 0.0, 7.0]), &[object(boxed([200.0, 200.0, 7.0]), Motion::Static)]); }, [150.0, 0.0]),
        (|w| { w.add_ref(&at(1, [300.0, 0.0, 50.0]), &[object(boxed([200.0, 200.0, 50.0]), Motion::Static)]); }, [150.0, 60.0]),
        (|w| ramp(w, 60.0), [150.0, 10.0]),
        (|w| {
            w.add_ref(&at(1, [150.0, 0.0, 50.0]), &[object(boxed([10.0, 200.0, 50.0]), Motion::Static)]);
            w.add_ref(&at(2, [0.0, 150.0, 50.0]), &[object(boxed([200.0, 10.0, 50.0]), Motion::Static)]);
        }, [150.0, 150.0]),
    ];
    for (i, (build, v)) in scenes.into_iter().enumerate() {
        let run = |hold: bool| {
            let mut w = ground();
            build(&mut w);
            let mut c = w.add_character(1, CharacterParams::default(), [0.0, 0.0, 1.0]);
            c.hold_when_stuck = hold;
            settle(&mut w, &mut c, 30);
            let steps: Vec<_> = walk(&mut w, &mut c, v, 120).into_iter().map(|s| (s.feet, s.grounded)).collect();
            (steps, c.held_steps)
        };
        let ((free, _), (kept, n)) = (run(false), run(true));
        assert_eq!(free, kept, "scene {i}");
        assert_eq!(n, 0, "scene {i}");
    }
}
