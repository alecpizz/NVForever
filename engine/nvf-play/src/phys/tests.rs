//! Streaming through the loader, over cells made up in memory (no game data): the world
//! must come out the same whatever order the loader finishes cells in, the same as
//! loading every cell on the main thread, and the same on every run.

use super::*;
use fnv_collide::{CollisionObject, Iso, Motion, Shape};
use fnv_phys::{CharacterInput, PreparedRef, PreparedTerrain, RefPose};
use fnv_sim::{FormStore, RefTable, SimConfig, World};
use fnv_world::land::SIDE;
use fnv_world::WorldIndex;
use prepare::{Entry, RefShapes};

fn sim() -> SimState {
    let forms: &'static FormStore = Box::leak(Box::default());
    let refs: &'static RefTable = Box::leak(Box::default());
    SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) }
}

fn play_data() -> PlayData {
    let lo: &'static fnv_esm::LoadOrder<'static> = Box::leak(Box::new(fnv_esm::LoadOrder::new(&[]).expect("empty load order")));
    let schema: &'static fnv_esm::Schema = Box::leak(Box::default());
    let index: &'static WorldIndex = Box::leak(Box::default());
    PlayData { lo, schema, index, world: 0x3C, interior: None, eye: [0.0; 3], heading_deg: 0.0, pitch_deg: 0.0, auto_walk: 0.0, start_at_cell_spot: false, packages: false }
}

fn object(half: f32, motion: Motion) -> CollisionObject {
    CollisionObject {
        node: None,
        node_block: 0,
        node_transform: Iso::IDENTITY,
        body_transform: Iso::IDENTITY,
        motion,
        layer: fnv_phys::layers::STATIC,
        mass: 20.0,
        friction: 0.5,
        restitution: 0.0,
        shape: Shape::Box { half_extents: [half; 3] }.into(),
        shell: 0.0,
        material: None,
        wind: false,
    }
}

/// A made-up exterior cell: gently sloping terrain, a row of static boxes and a dynamic
/// crate dropped from above (so that stepping depends on when and in what order bodies
/// arrived), with FormIDs derived from the grid square.
fn fake_cell(job: &Job) -> Prepared {
    let PhysCell::Exterior(gx, gy) = job.cell else { panic!("exterior cells only") };
    let params = PhysParams::default();
    let (x0, y0) = (gx as f32 * CELL_SIZE, gy as f32 * CELL_SIZE);
    // A plane rising to the north-east, continuous across cells.
    let heights: Vec<f32> =
        (0..SIDE * SIDE).map(|i| 0.01 * (x0 + (i % SIDE) as f32 * 128.0) + 0.005 * (y0 + (i / SIDE) as f32 * 128.0)).collect();
    let base = 0x0100_0000 + (((gx + 100) as u32) << 12) + (((gy + 100) as u32) << 4);
    let mut refs = Vec::new();
    for i in 0..6u32 {
        let dynamic = i == 5;
        let position = [x0 + 300.0 + i as f32 * 600.0, y0 + 1500.0 + (i % 2) as f32 * 900.0, if dynamic { 400.0 } else { 60.0 }];
        let pl = Placement {
            form_id: base + i,
            sig: *b"REFR",
            flags: 0,
            base: None,
            position,
            rotation: [0.0, 0.0, i as f32 * 0.4],
            scale: 1.0,
            enable_parent: None,
            open_by_default: false,
            teleport: false,
            emittance: None,
            imposter: false,
            primitive: false,
        };
        // As `prepare::cell` does: at the simulation's pose when it holds another.
        let pose = job.pose_of(pl.form_id);
        let pl = prepare::at_pose(pl, pose);
        let obj = object(if dynamic { 15.0 } else { 50.0 }, if dynamic { Motion::Dynamic } else { Motion::Static });
        let prepared = PreparedRef::new(&params, &RefPose::from_placement(&pl), &[obj]);
        let dynamic_local = dynamic.then_some(Iso::IDENTITY);
        // As `prepare::reference` does: built from its destruction model when it has one.
        let model = job.model_of(pl.form_id).map(str::to_string);
        refs.push(Entry { form_id: pl.form_id, placement: Some(pl), shapes: Some(RefShapes { tallies: vec!["fake"], water: Vec::new(), prepared: Some(prepared), dynamic_local, door: None, animated: None, drawn: None, model }) });
    }
    // A pond: a reference with only a water volume (placed water, physics.md §6.6).
    let pond = Placement {
        form_id: base + 6,
        sig: *b"REFR",
        flags: 0,
        base: None,
        position: [x0 + 2000.0, y0 + 3500.0, 0.0],
        rotation: [0.0; 3],
        scale: 1.0,
        enable_parent: None,
        open_by_default: false,
        teleport: false,
        emittance: None,
        imposter: false,
        primitive: false,
    };
    let water = vec![CollisionObject { layer: fnv_phys::layers::WATER, ..object(100.0, Motion::Static) }];
    refs.push(Entry { form_id: pond.form_id, placement: Some(pond), shapes: Some(RefShapes { tallies: vec!["water_volume"], water, prepared: None, dynamic_local: None, door: None, animated: None, drawn: None, model: None }) });
    let terrain = Some(PreparedTerrain::new(&params, (gx, gy), &heights));
    Prepared { cell: job.cell, surface: Some(fnv_world::land_surface::Surface::stored(&heights)), terrain, refs, times: LoadTimes::default() }
}

/// [`fake_cell`] with its second box keyframed (as a swing trap's held bodies).
fn fake_cell_keyframed(job: &Job) -> Prepared {
    let mut c = fake_cell(job);
    let e = &mut c.refs[1];
    let pl = e.placement.as_ref().expect("placed");
    let prepared = PreparedRef::new(&PhysParams::default(), &RefPose::from_placement(pl), &[object(50.0, Motion::Keyframed)]);
    e.shapes.as_mut().expect("shapes").prepared = Some(prepared);
    c
}

fn state(manual: bool) -> PhysState {
    let loader = if manual { loader::Loader::manual() } else { loader::Loader::default() };
    PhysState { fake: Some(fake_cell), loader, ..Default::default() }
}

fn centre(gx: i32, gy: i32) -> [f32; 3] {
    [(gx as f32 + 0.5) * CELL_SIZE, (gy as f32 + 0.5) * CELL_SIZE, 50.0]
}

/// Bit-identical worlds: every body's state in handle order, and what is loaded.
fn fingerprint(p: &PhysState) -> (u64, Vec<PhysCell>, fnv_phys::Stats) {
    (p.world.state_digest(), p.cells.keys().copied().collect(), p.world.stats())
}

/// FormID of made-up reference `i` of cell (`gx`, `gy`) ([`fake_cell`]).
fn fake_ref(gx: i32, gy: i32, i: u32) -> u32 {
    0x0100_0000 + (((gx + 100) as u32) << 12) + (((gy + 100) as u32) << 4) + i
}

const ENABLE_PARENT: u32 = 0x0200_0000;

/// How [`parented_cell`] sets up made-up reference `i`: (header flags, its enable parent
/// and opposite flag). Reference 4 is flagged too, but the simulation's table leaves it out.
fn parented(i: u32) -> (u32, Option<(u32, bool)>) {
    use fnv_world::placed::ref_flags::{DELETED, INITIALLY_DISABLED};
    match i {
        0 => (INITIALLY_DISABLED, Some((ENABLE_PARENT, false))),
        1 => (INITIALLY_DISABLED, Some((ENABLE_PARENT, true))),
        2 | 4 => (INITIALLY_DISABLED, None),
        3 => (0, Some((ENABLE_PARENT, false))),
        _ => (DELETED, Some((ENABLE_PARENT, false))),
    }
}

/// [`fake_cell`] with its first six references as [`parented`] says.
fn parented_cell(job: &Job) -> Prepared {
    let mut c = fake_cell(job);
    for (i, e) in c.refs.iter_mut().take(6).enumerate() {
        let pl = e.placement.as_mut().expect("placed");
        (pl.flags, pl.enable_parent) = parented(i as u32);
    }
    c
}

/// The simulation's side of [`parented_cell`]: the parent and the references it holds.
fn parented_sim() -> SimState {
    use fnv_sim::refs::{EnableParent, PlacedRef};
    let child = |i: u32| {
        let (flags, parent) = parented(i);
        let enable_parent = parent.map(|(p, opposite)| EnableParent { reference: FormId(p), opposite, pop_in: false });
        PlacedRef { flags, enable_parent, ..crate::sim::placed_ref(fake_ref(0, 0, i)) }
    };
    SimState::over_placed([crate::sim::placed_ref(ENABLE_PARENT), child(0), child(1), child(2), child(3), child(5)])
}

/// Issue #1620: a reference with an enable parent follows the parent, whatever its own
/// Initially Disabled flag, and no state is made for it to find that out (the reflector
/// rows at HELIOS One were drawn but had no collision until a console read made state).
#[test]
fn an_enable_parents_children_collide_by_the_parents_state() {
    let d = play_data();
    let mut sim = parented_sim();
    let mut p = PhysState { fake: Some(parented_cell), loader: loader::Loader::manual(), ..Default::default() };
    let cell = PhysCell::Exterior(0, 0);
    let (child, opposite, alone, plain, not_simulated, deleted) = (fake_ref(0, 0, 0), fake_ref(0, 0, 1), fake_ref(0, 0, 2), fake_ref(0, 0, 3), fake_ref(0, 0, 4), fake_ref(0, 0, 5));
    let untouched = |sim: &SimState| [child, opposite, alone, plain, not_simulated, deleted].iter().all(|&r| sim.world.peek(FormId(r)).is_none());

    // The parent is enabled.
    p.load(&LoadCtx { d: &d, sim: &sim, data: None }, cell);
    assert!(p.has_collision(child), "flagged: follows its parent");
    assert!(!p.has_collision(opposite), "flagged, opposite of its parent");
    assert!(!p.has_collision(alone), "flagged, no parent: its own flag");
    assert!(p.has_collision(plain));
    assert!(!p.has_collision(not_simulated), "flagged, not in the simulation's table: its own flag");
    assert!(!p.has_collision(deleted));
    assert!(untouched(&sim), "asking makes no state");

    // Disabled while the cell is away: the other way round when it comes back.
    p.unload(cell);
    sim.world.set_enabled(FormId(ENABLE_PARENT), false).unwrap();
    p.load(&LoadCtx { d: &d, sim: &sim, data: None }, cell);
    assert!(!p.has_collision(child));
    assert!(p.has_collision(opposite));
    assert!(!p.has_collision(alone));
    assert!(!p.has_collision(plain));
    assert!(!p.has_collision(not_simulated));
    assert!(!p.has_collision(deleted));
    assert!(untouched(&sim));

    // Enabled again while it is away.
    p.unload(cell);
    sim.world.set_enabled(FormId(ENABLE_PARENT), true).unwrap();
    p.load(&LoadCtx { d: &d, sim: &sim, data: None }, cell);
    assert!(p.has_collision(child));
    assert!(!p.has_collision(opposite));
    assert!(untouched(&sim));
}

#[test]
fn a_destroyed_reference_collides_as_its_destruction_model_and_back() {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = state(true);
    p.load(&ctx, PhysCell::Exterior(0, 0));
    let (car, other) = (fake_ref(0, 0, 1), fake_ref(0, 0, 2));
    let hulk = "Vehicles\\CarHulk01.NIF".to_string();
    assert_eq!(p.built_model(car), None);
    // The stage that names a model: built again from it, the others left alone.
    let built = p.refs_built;
    p.set_destruction_models(&ctx, HashMap::from([(car, hulk.clone())]));
    assert_eq!(p.built_model(car), Some(hulk.as_str()));
    assert!(p.has_collision(car));
    assert_eq!(p.built_model(other), None);
    assert_eq!(p.refs_built, built + 1, "only the destroyed reference is built again");
    // Nothing changed: nothing built.
    p.set_destruction_models(&ctx, HashMap::from([(car, hulk.clone())]));
    assert_eq!(p.refs_built, built + 1);
    // ClearDestruction: the base model's collision again.
    p.set_destruction_models(&ctx, HashMap::new());
    assert_eq!(p.built_model(car), None);
    assert!(p.has_collision(car));
    assert_eq!(p.refs_built, built + 2);
    // A reference whose cell is not loaded gets nothing now, and its model when the
    // cell loads.
    let far = fake_ref(5, 5, 1);
    p.set_destruction_models(&ctx, HashMap::from([(far, hulk.clone())]));
    assert_eq!(p.built_model(far), None);
    assert!(!p.has_collision(far));
    p.load(&ctx, PhysCell::Exterior(5, 5));
    assert_eq!(p.built_model(far), Some(hulk.as_str()));
    // Its cell unloaded: forgotten with it.
    p.unload(PhysCell::Exterior(5, 5));
    assert_eq!(p.built_model(far), None);
}

#[test]
fn a_job_knows_its_references_destruction_models() {
    let job = Job { cell: PhysCell::Exterior(0, 0), refs: vec![1, 5, 9], land: None, lands: Default::default(), surface: fnv_world::land_surface::SurfaceSpec::Stored, poses: Vec::new(), scales: Vec::new(), models: vec![(5, "a.nif".into()), (9, "b.nif".into())] };
    assert_eq!(job.model_of(5), Some("a.nif"));
    assert_eq!(job.model_of(9), Some("b.nif"));
    assert_eq!(job.model_of(1), None);
}

#[test]
fn cells_go_in_in_request_order_whatever_order_they_finish() {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let run = |reverse: bool| {
        let mut p = state(true);
        p.stream(&ctx, centre(0, 0), CELLS_PER_FRAME);
        // The cell underfoot went in at once; its eight neighbours wait for the loader.
        assert_eq!(p.cells.keys().copied().collect::<Vec<_>>(), vec![PhysCell::Exterior(0, 0)]);
        assert_eq!(p.loader.pending().count(), 8);
        p.loader.run_manual(reverse);
        for _ in 0..8 {
            p.stream(&ctx, centre(0, 0), CELLS_PER_FRAME);
        }
        assert_eq!(p.cells.len(), 9);
        for _ in 0..30 {
            p.world.step();
        }
        fingerprint(&p)
    };
    let forward = run(false);
    assert_eq!(run(true), forward);
    // The same as loading every cell on the main thread, nearest first.
    let mut sync = state(true);
    for (x, y) in cells::wanted((0, 0)) {
        sync.load(&ctx, PhysCell::Exterior(x, y));
    }
    for _ in 0..30 {
        sync.world.step();
    }
    assert_eq!(fingerprint(&sync), forward);
    // The check can fail: in another order the same cells give another world.
    let mut other = state(true);
    for (x, y) in cells::wanted((0, 0)).into_iter().rev() {
        other.load(&ctx, PhysCell::Exterior(x, y));
    }
    for _ in 0..30 {
        other.world.step();
    }
    assert_ne!(fingerprint(&other), forward);
}

/// Walk east across three cells, streaming as the game does and waiting for the loader
/// thread each frame.
fn walk() -> ((u64, Vec<PhysCell>, fnv_phys::Stats), [f32; 3]) {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = state(false);
    let start = centre(0, 0);
    p.stream(&ctx, start, CELLS_PER_FRAME);
    p.finish_loading(&ctx);
    p.place_player(start, 200.0, 1000.0).expect("ground under the start");
    for _ in 0..900 {
        let feet = p.player.as_ref().unwrap().feet;
        p.stream(&ctx, feet, CELLS_PER_FRAME);
        p.finish_loading(&ctx);
        assert!(p.ready_at(feet));
        let c = p.player.as_mut().unwrap();
        p.world.move_character(c, CharacterInput { velocity: [700.0, 0.0], ..Default::default() });
        p.world.step();
    }
    let feet = p.player.as_ref().unwrap().feet;
    assert!(feet[0] > 2.0 * CELL_SIZE, "walked into the third cell: {feet:?}");
    assert_eq!(p.stats.loaded_inline, 0);
    (fingerprint(&p), feet)
}

#[test]
fn two_runs_are_identical() {
    let a = walk();
    let b = walk();
    assert_eq!(a.0, b.0);
    assert_eq!(a.1.map(f32::to_bits), b.1.map(f32::to_bits));
}

#[test]
fn the_cell_underfoot_never_waits_for_the_loader() {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    // A loader that never finishes anything.
    let mut p = state(true);
    p.stream(&ctx, centre(0, 0), CELLS_PER_FRAME);
    p.place_player(centre(0, 0), 200.0, 1000.0).expect("ground");
    assert_eq!(p.stats.loaded_inline, 0, "the start is not a hitch");
    // The player steps into the next cell before the loader delivered it.
    let next = centre(1, 0);
    assert!(!p.ready_at(next));
    p.stream(&ctx, next, CELLS_PER_FRAME);
    assert!(p.ready_at(next));
    assert_eq!(p.stats.loaded_inline, 1);
    // Its request was dropped: the loader's late result is not inserted twice.
    assert!(!p.loader.pending().any(|c| c == PhysCell::Exterior(1, 0)));
    p.loader.run_manual(false);
    let bodies = p.world.stats().refs;
    for _ in 0..20 {
        p.stream(&ctx, next, CELLS_PER_FRAME);
    }
    assert_eq!(p.cells.len(), 12);
    assert_eq!(p.world.stats().refs, bodies + 10 * 6, "ten more cells of six references");
}

#[test]
fn placed_water_comes_and_goes_with_its_cell() {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    // Only the cell underfoot goes in: the neighbours wait for a loader that never runs.
    let mut p = state(true);
    let pond = [2000.0, 3500.0, 50.0];
    assert_eq!(p.world.water_surface_for(pond), None);
    p.stream(&ctx, centre(0, 0), CELLS_PER_FRAME);
    assert_eq!(p.world.water_surface_for(pond), Some(100.0), "the box's top");
    assert_eq!(p.stats.counts.get("water_volume_refs"), Some(&1));
    p.unload(PhysCell::Exterior(0, 0));
    assert_eq!(p.world.water_surface_for(pond), None);
}

#[test]
fn cells_left_behind_are_cancelled_and_reset_forgets_requests() {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = state(true);
    p.stream(&ctx, centre(0, 0), CELLS_PER_FRAME);
    // Far away before anything arrived: the old requests go, new ones replace them.
    p.stream(&ctx, centre(10, 0), CELLS_PER_FRAME);
    assert!(p.loader.pending().all(|c| matches!(c, PhysCell::Exterior(x, _) if x >= 9)));
    assert_eq!(p.cells.keys().copied().collect::<Vec<_>>(), vec![PhysCell::Exterior(10, 0)]);
    p.loader.run_manual(false);
    for _ in 0..20 {
        p.stream(&ctx, centre(10, 0), CELLS_PER_FRAME);
    }
    assert_eq!(p.cells.len(), 9);
    assert!(p.cells.keys().all(|c| matches!(c, PhysCell::Exterior(x, _) if (9..=11).contains(x))));
    // Door travel: everything requested is forgotten with the cells.
    p.stream(&ctx, centre(20, 0), CELLS_PER_FRAME);
    assert!(p.loader.pending().count() > 0);
    p.reset(Space::Interior(7));
    assert_eq!(p.loader.pending().count(), 0);
    assert!(p.cells.is_empty());
}

/// [`fake_cell`], with square (1, 0) also holding a walkway: a slab 20 units thick whose
/// top is at z 420, reaching 500 units west over the border into square (0, 0).
fn bridge_cell(job: &Job) -> Prepared {
    let mut p = fake_cell(job);
    if job.cell == PhysCell::Exterior(1, 0) {
        let pl = Placement {
            form_id: 0x0200_0001,
            sig: *b"REFR",
            flags: 0,
            base: None,
            position: [CELL_SIZE + 100.0, 0.5 * CELL_SIZE, 400.0],
            rotation: [0.0; 3],
            scale: 1.0,
            enable_parent: None,
            open_by_default: false,
            teleport: false,
            emittance: None,
            imposter: false,
            primitive: false,
        };
        let obj = CollisionObject { shape: Shape::Box { half_extents: [600.0, 600.0, 20.0] }.into(), ..object(0.0, Motion::Static) };
        let prepared = PreparedRef::new(&PhysParams::default(), &RefPose::from_placement(&pl), &[obj]);
        p.refs.push(Entry { form_id: pl.form_id, placement: Some(pl), shapes: Some(RefShapes { tallies: vec!["fake"], water: Vec::new(), prepared: Some(prepared), dynamic_local: None, door: None, animated: None, drawn: None, model: None }) });
    }
    p
}

#[test]
fn a_start_over_a_neighbours_walkway_waits_for_it() {
    // #1103: a save made on the Bison Steve rollercoaster loaded the player on the ground
    // under it. The start's own square goes in at once; the track's, next door, later.
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = PhysState { fake: Some(bridge_cell), loader: loader::Loader::manual(), ..Default::default() };
    // Saved standing on the walkway, in square (0, 0) 300 units from the border.
    let saved = [CELL_SIZE - 300.0, 0.5 * CELL_SIZE, 420.0];
    p.stream(&ctx, saved, CELLS_PER_FRAME);
    assert!(p.ready_at(saved), "the square underfoot is in");
    assert!(!p.ready_around(saved), "the walkway's square is not");
    // Placed now, the start would find only the terrain (about z 51).
    let early = p.place_player(saved, 120.0, 100_000.0).expect("terrain");
    assert!(early.point[2] < 100.0, "{early:?}");
    p.loader.run_manual(false);
    for _ in 0..8 {
        p.stream(&ctx, saved, CELLS_PER_FRAME);
    }
    assert!(p.ready_around(saved));
    let hit = p.place_player(saved, 120.0, 100_000.0).expect("the walkway");
    assert!((hit.point[2] - 420.0).abs() < 1.0, "{hit:?}");
    let feet = p.player.as_ref().unwrap().feet;
    assert!((feet[2] - 420.0).abs() < 5.0, "back at the saved height: {feet:?}");
}

/// A made-up slab placed in square (2, 0) whose bounds reach west over (1, 0) into (0, 0)
/// (#2795: HELIOS One's plaza, placed two squares from its south end).
const SLAB: u32 = 0x0200_0002;

/// [`fake_cell`], with the slab wherever the job lists it: 40 units thick, top at z 420,
/// from x 3240 (square 0) to x 17240 (square 4), 600 units either side of y 2048.
fn reach_cell(job: &Job) -> Prepared {
    let mut p = fake_cell(job);
    if job.refs.contains(&SLAB) {
        let pl = Placement {
            form_id: SLAB,
            sig: *b"REFR",
            flags: 0,
            base: None,
            position: [2.5 * CELL_SIZE, 0.5 * CELL_SIZE, 400.0],
            rotation: [0.0; 3],
            scale: 1.0,
            enable_parent: None,
            open_by_default: false,
            teleport: false,
            emittance: None,
            imposter: false,
            primitive: false,
        };
        let obj = CollisionObject { shape: Shape::Box { half_extents: [7000.0, 600.0, 20.0] }.into(), ..object(0.0, Motion::Static) };
        let prepared = PreparedRef::new(&PhysParams::default(), &RefPose::from_placement(&pl), &[obj]);
        p.refs.push(Entry { form_id: pl.form_id, placement: Some(pl), shapes: Some(RefShapes { tallies: vec!["fake"], water: Vec::new(), prepared: Some(prepared), dynamic_local: None, door: None, animated: None, drawn: None, model: None }) });
    }
    p
}

#[test]
fn a_reference_reaching_into_loaded_squares_is_added_once_and_removed_with_the_last() {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = PhysState { fake: Some(reach_cell), loader: loader::Loader::manual(), ..Default::default() };
    // The index as `fnv_world::reach` builds it: placed in (2, 0), reaching (0, 0)..(4, 0).
    p.reach = Arc::new(fnv_world::reach::ReachIndex::from_entries([(SLAB, (2, 0), ((0, 0), (4, 0)))]));
    // A square its bounds do not reach does not load it.
    p.load(&ctx, PhysCell::Exterior(0, 1));
    assert!(!p.has_collision(SLAB));
    assert!(!p.cells[&PhysCell::Exterior(0, 1)].refs.contains(&SLAB));
    // The player's square, two squares from where the slab is placed: it loads it, and the
    // player stands on it rather than on the terrain (about z 46 there).
    p.load(&ctx, PhysCell::Exterior(0, 0));
    assert!(p.has_collision(SLAB));
    let hit = p.place_player([3600.0, 0.5 * CELL_SIZE, 500.0], 120.0, 100_000.0).expect("ground");
    assert!((hit.point[2] - 420.0).abs() < 1.0, "on the slab: {hit:?}");
    let bodies = p.world.ref_bodies(SLAB).len();
    assert!(bodies > 0);
    // The next square reaches it too: still one body.
    p.load(&ctx, PhysCell::Exterior(1, 0));
    assert_eq!(p.world.ref_bodies(SLAB).len(), bodies, "added once");
    assert_eq!(p.held.get(&SLAB), Some(&2));
    // The first holder goes: the other keeps it.
    p.unload(PhysCell::Exterior(0, 0));
    assert_eq!(p.world.ref_bodies(SLAB).len(), bodies, "kept by the square still loaded");
    assert_eq!(p.ref_cell.get(&SLAB), Some(&PhysCell::Exterior(1, 0)));
    // The last goes: so does the slab.
    p.unload(PhysCell::Exterior(1, 0));
    assert!(!p.has_collision(SLAB));
    assert!(!p.held.contains_key(&SLAB) && !p.ref_cell.contains_key(&SLAB));
    // Nothing is left held once every square is gone.
    p.unload(PhysCell::Exterior(0, 1));
    assert!(p.held.is_empty());
}

#[test]
fn indoors_the_one_cell_is_all_a_start_waits_for() {
    let mut p = state(true);
    p.space = Space::Interior(7);
    assert!(!p.ready_around([0.0; 3]));
    p.cells.insert(PhysCell::Interior(7), LoadedCell::default());
    assert!(p.ready_around([0.0; 3]));
}

/// The dynamic crate of cell (0, 0) as a placed reference the simulation knows.
fn sim_with_crate() -> (SimState, u32, [f32; 3]) {
    let id = fake_ref(0, 0, 5);
    let pos = [300.0 + 5.0 * 600.0, 1500.0 + 900.0, 400.0];
    (sim_with_placed(id, pos, [0.0, 0.0, 2.0]), id, pos)
}

/// A simulation that places made-up reference `id` ([`fake_cell`]) at `pos`, turned `rot`.
fn sim_with_placed(id: u32, pos: [f32; 3], rot: [f32; 3]) -> SimState {
    let mut table = RefTable::default();
    table.refs.insert(
        FormId(id),
        fnv_sim::refs::PlacedRef {
            id: FormId(id),
            sig: *b"REFR",
            edid: None,
            base: FormId(0x0500_0000),
            cell: None,
            stored_in: None,
            plugin: 0,
            flags: 0,
            in_persistent_group: false,
            pos,
            rot,
            scale: None,
            count: None,
            ammo: None,
            health: None,
            owner: None,
            lock: None,
            teleport: None,
            enable_parent: None,
            linked: None,
            patrol_idle: None,
            patrol_topic: None,
            merchant_container: None,
            marker_reputation: None,
            open_by_default: false,
            primitive: None,
            activate_parents: Vec::new(),
            parent_activate_only: false,
            prompt: None,
            radiation: None,
            radius: None,
            encounter_zone: None,
            level_modifier: None,
            map_marker: None,
            radio: None,
        },
    );
    let forms: &'static FormStore = Box::leak(Box::default());
    let refs: &'static RefTable = Box::leak(Box::new(table));
    SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) }
}

/// The ground probe of the `actor_feet` test aid sees through a dynamic body resting on
/// the spot (pete-terrain met a tumbleweed at Pete's feet, #2149); a character's own
/// ground query still meets it.
#[test]
fn the_fixed_ground_leaves_out_dynamic_bodies() {
    let d = play_data();
    let (sim, _id, crate_at) = sim_with_crate();
    let mut p = state(true);
    p.load(&LoadCtx { d: &d, sim: &sim, data: None }, PhysCell::Exterior(0, 0));
    let from = [crate_at[0], crate_at[1], crate_at[2] + 100.0];
    let terrain = 0.01 * crate_at[0] + 0.005 * crate_at[1];
    let (with_crate, _) = p.ground_drawn(from, 600.0, None).expect("the crate");
    assert!(with_crate > crate_at[2], "ground_drawn meets the crate's top: {with_crate}");
    let (fixed, _) = p.fixed_ground_drawn(from, 600.0).expect("the terrain");
    assert!((fixed - terrain).abs() < 1.0, "fixed_ground_drawn meets the terrain at {terrain}: {fixed}");
}

#[test]
fn a_co_op_guests_copy_of_a_host_moved_object_is_not_written_back() {
    let d = play_data();
    let (mut sim, id, _) = sim_with_crate();
    let mut p = state(true);
    p.load(&LoadCtx { d: &d, sim: &sim, data: None }, PhysCell::Exterior(0, 0));
    p.follow_only.insert(id);
    for _ in 0..240 {
        p.world.step();
        assert!(p.settle_placed(&mut sim.world).is_empty(), "the host owns its pose");
    }
    assert!(sim.moved_pose(id).is_none());
}

#[test]
fn placed_clutter_moved_by_physics_is_written_back_and_reloads_there() {
    let d = play_data();
    let (mut sim, id, placed) = sim_with_crate();
    let mut p = state(true);
    p.load(&LoadCtx { d: &d, sim: &sim, data: None }, PhysCell::Exterior(0, 0));
    assert!(p.placed_dynamic.get(&id).is_some_and(|c| !c.moved), "tracked, not moved yet");
    assert!(sim.moved_pose(id).is_none());
    let mut written = 0;
    for _ in 0..240 {
        p.world.step();
        written += p.settle_placed(&mut sim.world).len();
    }
    assert!(p.placed_dynamic[&id].moved && written > 0);
    let (pos, rot) = sim.moved_pose(id).expect("written back");
    assert!(pos[2] < placed[2] - 200.0, "fell onto the terrain: {pos:?}");
    // At rest: nothing more to write.
    assert!(p.settle_placed(&mut sim.world).is_empty());
    // Unloaded and loaded again: built at the simulation's pose, not the placement's.
    p.unload(PhysCell::Exterior(0, 0));
    assert!(!p.placed_dynamic.contains_key(&id));
    // (The made-up cells have no records, so the job names the crate itself.)
    let cell = PhysCell::Exterior(0, 0);
    let job = with_poses(Job { cell, refs: vec![id], land: None, lands: Default::default(), surface: fnv_world::land_surface::SurfaceSpec::Stored, poses: Vec::new(), scales: Vec::new(), models: Vec::new() }, &sim);
    p.insert(&LoadCtx { d: &d, sim: &sim, data: None }, fake_cell(&job));
    let b = p.world.ref_bodies(id)[0];
    for i in 0..3 {
        assert!((b.translation[i] - pos[i]).abs() < 1e-2, "{b:?} against {pos:?}");
    }
    assert!(fnv_world::transform::max_diff(&b.rotation, &fnv_world::transform::ref_rotation(rot)) < 1e-4);
    assert!(!p.placed_dynamic[&id].moved, "a fresh start from where it rests");
}

/// Placed clutter falling under physics is drawn between its last two steps
/// (`PhysState::drawn_ref_pose`, `fnv_phys::drawn`): at the share of a step the stepper
/// holds, from the pose before the step to the one after it, while the simulation holds
/// the stepped pose; it is listed for drawing while it moves and for one frame after.
#[test]
fn falling_clutter_is_drawn_between_its_last_two_steps() {
    let d = play_data();
    let (mut sim, id, _) = sim_with_crate();
    let mut p = state(true);
    p.load(&LoadCtx { d: &d, sim: &sim, data: None }, PhysCell::Exterior(0, 0));
    let dt = p.world.params.dt;
    let frame = |p: &mut PhysState, sim: &mut SimState| {
        p.world.step();
        p.settle_placed(&mut sim.world);
        p.update_between();
    };
    for _ in 0..20 {
        frame(&mut p, &mut sim);
    }
    assert!(p.placed_dynamic[&id].moved, "falling");
    let pd = &p.placed_dynamic[&id];
    let (local, scale) = (pd.local, pd.scale);
    let before = dropped::ref_pose_from_body(&p.world.ref_bodies(id)[0], &local, scale);
    frame(&mut p, &mut sim);
    let after = dropped::ref_pose_from_body(&p.world.ref_bodies(id)[0], &local, scale);
    assert!(after.0[2] < before.0[2] - 1.0, "{before:?} {after:?}");
    assert_eq!((p.between.now.clone(), p.between.redraw.clone()), (vec![id], vec![id]));
    // The one place the share of a step is worked out.
    p.stepper.accumulated = 0.0;
    assert_eq!(p.draw_alpha(), 0.0);
    assert_eq!(p.drawn_ref_pose(id), Some(before));
    p.stepper.accumulated = f64::from(dt);
    assert_eq!(p.draw_alpha(), 1.0);
    assert_eq!(p.drawn_ref_pose(id), Some(after));
    p.stepper.accumulated = f64::from(dt) * 0.25;
    let quarter = p.drawn_ref_pose(id).unwrap().0[2];
    assert!((quarter - (before.0[2] * 0.75 + after.0[2] * 0.25)).abs() < 1e-2, "{quarter}");
    // The simulation holds the stepped pose.
    let (pos, _) = sim.moved_pose(id).expect("written back");
    assert!((pos[2] - after.0[2]).abs() < 0.02, "{pos:?} {after:?}");
    // At rest: listed once more, then no longer.
    let mut frames = 0;
    while !p.world.ref_bodies(id)[0].sleeping {
        frame(&mut p, &mut sim);
        frames += 1;
        assert!(frames < 1200, "comes to rest");
    }
    frame(&mut p, &mut sim);
    assert!(p.between.now.is_empty());
    frame(&mut p, &mut sim);
    assert!(p.between.redraw.is_empty());
    assert!(p.between.most >= 1 && p.between.frames > 20, "{:?}", p.between);
    // A placed reference physics has not moved is drawn at its placement.
    assert!(p.drawn_ref_pose(fake_ref(0, 0, 1)).is_none());
}

/// A script's move of a reference whose collision physics never moves (a fixed box, as
/// a STAT's) builds it again at the simulation's pose, not left at the old one (issue
/// #1466); a repose where it already stands builds nothing.
#[test]
fn a_script_moved_fixed_reference_collides_at_its_new_pose() {
    let d = play_data();
    let id = fake_ref(0, 0, 1);
    let placed = [300.0 + 600.0, 1500.0 + 900.0, 60.0];
    let mut sim = sim_with_placed(id, placed, [0.0, 0.0, 0.4]);
    let mut p = state(true);
    p.load(&LoadCtx { d: &d, sim: &sim, data: None }, PhysCell::Exterior(0, 0));
    assert!(p.has_collision(id) && !p.placed_dynamic.contains_key(&id), "fixed collision, not one dynamic body");
    let built = p.refs_built;
    // Told of a move it has not made: nothing built.
    p.repose_ref(&LoadCtx { d: &d, sim: &sim, data: None }, id, 0.0);
    assert_eq!(p.refs_built, built);
    // SetPos and SetAngle.
    let (to, angles) = ([placed[0] + 400.0, placed[1] - 200.0, placed[2] + 30.0], [0.0, 0.0, 1.2]);
    {
        let s = sim.world.get(FormId(id)).expect("placed");
        s.pos = to;
        s.rot = angles;
    }
    p.repose_ref(&LoadCtx { d: &d, sim: &sim, data: None }, id, 0.0);
    assert_eq!(p.refs_built, built + 1, "built again");
    let bodies = p.world.ref_bodies(id);
    assert_eq!(bodies.len(), 1);
    for i in 0..3 {
        assert!((bodies[0].translation[i] - to[i]).abs() < 1e-2, "{:?} against {to:?}", bodies[0]);
    }
    assert!(fnv_world::transform::max_diff(&bodies[0].rotation, &fnv_world::transform::ref_rotation(angles)) < 1e-4);
    // Where it was built now: nothing more.
    p.repose_ref(&LoadCtx { d: &d, sim: &sim, data: None }, id, 0.0);
    assert_eq!(p.refs_built, built + 1);
}

/// A script that turns a fixed reference a little every frame (Rotate, 10 degrees a
/// second at 60 frames a second) does not build its collision again every frame: only
/// when it strays [`super::REPOSE_NOW_ANGLE`] from its build, and once when it rests.
#[test]
fn a_reference_turned_every_frame_is_built_again_a_bounded_number_of_times() {
    let d = play_data();
    let id = fake_ref(0, 0, 1);
    let placed = [300.0 + 600.0, 1500.0 + 900.0, 60.0];
    let mut sim = sim_with_placed(id, placed, [0.0, 0.0, 0.4]);
    let mut p = state(true);
    p.load(&LoadCtx { d: &d, sim: &sim, data: None }, PhysCell::Exterior(0, 0));
    assert!(p.has_collision(id) && !p.placed_dynamic.contains_key(&id));
    let (dt, frames) = (1.0 / 60.0, 180);
    let step = 10f32.to_radians() * dt as f32;
    for f in 1..=frames {
        sim.world.get(FormId(id)).expect("placed").rot[2] += step;
        let now = f as f64 * dt;
        let ctx = LoadCtx { d: &d, sim: &sim, data: None };
        p.repose_ref(&ctx, id, now);
        p.flush_reposes(&ctx, now);
    }
    // 30 degrees in 3 s: twice past 15 degrees from the last build, at most.
    assert!(p.reposes <= 2, "built again {} times in {frames} frames", p.reposes);
    assert!(p.repose_waiting(id), "the last turn waits for it to rest");
    // It rests: built once more, where the script left it.
    let rest = frames as f64 * dt + super::REPOSE_SETTLE;
    let before = p.reposes;
    p.flush_reposes(&LoadCtx { d: &d, sim: &sim, data: None }, rest);
    assert_eq!(p.reposes, before + 1);
    assert!(!p.repose_waiting(id));
    let rot = fnv_world::transform::ref_rotation([0.0, 0.0, 0.4 + step * frames as f32]);
    assert!(fnv_world::transform::max_diff(&p.world.ref_bodies(id)[0].rotation, &rot) < 1e-3);
    p.flush_reposes(&LoadCtx { d: &d, sim: &sim, data: None }, rest + 1.0);
    assert_eq!(p.reposes, before + 1, "nothing more once built where it rests");
}

/// Keyframed collision switched off (a swing trap's held bodies after let-go) stays off
/// when a script's move builds it again.
#[test]
fn a_switched_off_collision_stays_off_when_built_again() {
    let d = play_data();
    let id = fake_ref(0, 0, 1);
    let placed = [300.0 + 600.0, 1500.0 + 900.0, 60.0];
    let mut sim = sim_with_placed(id, placed, [0.0, 0.0, 0.4]);
    let mut p = PhysState { fake: Some(fake_cell_keyframed), ..state(true) };
    p.load(&LoadCtx { d: &d, sim: &sim, data: None }, PhysCell::Exterior(0, 0));
    assert_eq!(p.world.set_ref_keyframed_solid(id, false), 1, "its keyframed box switched off");
    sim.world.get(FormId(id)).expect("placed").pos[0] += 400.0;
    p.repose_ref(&LoadCtx { d: &d, sim: &sim, data: None }, id, 0.0);
    assert_eq!(p.reposes, 1);
    assert_eq!(p.world.set_ref_keyframed_solid(id, false), 0, "still off after the build");
    assert_eq!(p.world.set_ref_keyframed_solid(id, true), 1, "and there to switch on again");
}

/// A STAT's dynamic bodies are fixed (the saloon's motorcycles, playtest list 5 item 59);
/// a moveable static's stay dynamic.
#[test]
fn statics_never_move_whatever_their_model_says() {
    let model = || vec![object(10.0, Motion::Dynamic), object(5.0, Motion::Keyframed)];
    let mut tallies = Vec::new();
    let mut stat = model();
    prepare::fix_static_base(Some(*b"STAT"), &mut stat, &mut tallies);
    assert_eq!(stat.iter().map(|o| o.motion).collect::<Vec<_>>(), [Motion::Static, Motion::Keyframed]);
    assert_eq!(tallies, ["static_base_fixed"]);
    let mut mstt = model();
    prepare::fix_static_base(Some(*b"MSTT"), &mut mstt, &mut tallies);
    assert_eq!(mstt[0].motion, Motion::Dynamic);
    assert_eq!(tallies.len(), 1);
}

/// A clutter skeleton's collision lies in the pose its `XRGD` stores, as its drawing does
/// (playtest list 5 item 71): a torso standing 69 up with a skull 45 above it, laid on its
/// back 10 above the floor, puts the skull 45 along the floor, not over the torso.
#[test]
fn stored_pose_lays_the_collision_down() {
    use fnv_collide::nif::{NodeTree, Similarity};
    use fnv_world::xrgd::Bone;
    let at = |t: [f32; 3]| Similarity { iso: Iso { translation: t, rotation: Iso::IDENTITY.rotation }, scale: 1.0 };
    // Root 0 → torso 1 → neck 2 (no body) → skull 3.
    let tree = NodeTree::from_nodes([(0, None, Similarity::IDENTITY), (1, Some(0), at([0.0, 0.0, 69.0])), (2, Some(1), at([0.0, 0.0, 40.0])), (3, Some(2), at([0.0, 0.0, 5.0]))]);
    let body = |block: usize, z: f32| CollisionObject { node: Some(format!("n{block}")), node_block: block, node_transform: Iso { translation: [0.0, 0.0, z], rotation: Iso::IDENTITY.rotation }, ..object(4.0, Motion::Dynamic) };
    let standing = vec![body(1, 69.0), body(3, 114.0)];
    // The torso turned a quarter about y (z onto x), the skull unturned under the neck.
    let bones = [Bone { position: [0.0, 0.0, 10.0], angles: [0.0, -std::f32::consts::FRAC_PI_2, 0.0] }, Bone { position: [0.0, 0.0, 5.0], angles: [0.0; 3] }];
    let mut objects = standing.clone();
    assert!(prepare::pose_stored(&tree, &bones, &mut objects));
    let p = |o: &CollisionObject| o.node_transform.translation;
    let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3);
    assert!(near(p(&objects[0]), [0.0, 0.0, 10.0]), "{:?}", p(&objects[0]));
    assert!(near(p(&objects[1]), [45.0, 0.0, 10.0]), "{:?}", p(&objects[1]));
    assert!(near(objects[1].node_transform.apply([0.0, 0.0, 1.0]), [46.0, 0.0, 10.0]), "the skull lies turned with the torso");
    // Entries that do not fit leave the file's pose.
    let mut objects = standing.clone();
    assert!(!prepare::pose_stored(&tree, &bones[..1], &mut objects));
    assert_eq!(objects, standing);
}

/// A skeleton part copied with the whole skeleton's `XRGD` poses its bodies from the
/// first entries, the extra entries ignored (issue #1400).
#[test]
fn a_skeleton_part_poses_its_collision_from_the_first_entries() {
    use fnv_collide::nif::{NodeTree, Similarity};
    use fnv_world::xrgd::Bone;
    let at = |t: [f32; 3]| Similarity { iso: Iso { translation: t, rotation: Iso::IDENTITY.rotation }, scale: 1.0 };
    let tree = NodeTree::from_nodes([(0, None, Similarity::IDENTITY), (1, Some(0), at([40.0, 0.0, 0.0])), (2, Some(1), at([23.0, 0.0, 0.0]))]);
    let body = |block: usize| CollisionObject { node: Some(format!("n{block}")), node_block: block, node_transform: Iso::IDENTITY, ..object(4.0, Motion::Dynamic) };
    let standing = vec![body(1), body(2)];
    // Three entries (a whole skeleton's), the first two for the two bodies, the third spare.
    let bones = [Bone { position: [5.0, 0.0, 0.0], angles: [0.0; 3] }, Bone { position: [10.0, 0.0, 0.0], angles: [0.0; 3] }, Bone { position: [999.0; 3], angles: [0.0; 3] }];
    let mut objects = standing.clone();
    assert!(prepare::pose_stored(&tree, &bones, &mut objects));
    assert_eq!(objects[0].node_transform.translation, [5.0, 0.0, 0.0], "entry 0");
    assert_eq!(objects[1].node_transform.translation, [15.0, 0.0, 0.0], "entry 0 + entry 1");
    // A single body with several entries is refused (which one is meant is ambiguous).
    let mut one = vec![standing[0].clone()];
    assert!(!prepare::pose_stored(&tree, &bones, &mut one));
}

/// A small creature's character is its own size, a person's the player's (playtest list
/// 7 item 35: the mantis nymphs, drawn about 30 units tall, had the player's capsule).
#[test]
fn small_actors_get_characters_of_their_size() {
    let person = super::npc_params(Some(127.0));
    assert_eq!((person.radius, person.height), (22.0, fnv_phys::CharacterParams::default().height));
    assert_eq!(super::npc_params(None).radius, 22.0);
    let nymph = super::npc_params(Some(30.0));
    assert_eq!((nymph.radius, nymph.height), (crate::blocking::MIN_RADIUS, 30.0));
    let dog = super::npc_params(Some(80.0));
    assert!((dog.radius - 13.6).abs() < 1e-3 && dog.height == 80.0, "{dog:?}");
    assert_eq!(super::npc_params(Some(3.0)).height, super::NPC_MIN_HEIGHT);
    for params in [person, super::npc_params(None), nymph, dog] {
        assert_eq!(params.max_slope_deg, fnv_phys::character::ACTOR_MAX_SLOPE_DEG);
        assert!(params.max_slope_deg < params.bottom_slope.atan().to_degrees());
        assert_eq!(params.step_height, 24.0);
    }
}

/// The gate of [`gate_cell`]: placed at x 1000 in square (0, 0); its "Forward" (played
/// once, 1 s) slides its keyframed panel 300 units along x.
const GATE: u32 = 0x0200_0002;
const GATE_X: f32 = 1000.0;

fn gate_anim() -> fnv_collide::anim::ModelAnim {
    use fnv_collide::nif::Similarity;
    use fnv_nif::anim::{Channel, Key, KeyKind, Keys, TransformTrack};
    let mut anim = fnv_collide::anim::ModelAnim::from_nodes([(0, Some("Root".to_string()), None, Similarity::IDENTITY), (1, Some("Panel".to_string()), Some(0), Similarity::IDENTITY)]);
    let key = |time: f32, x: f32| Key { time, value: [x, 0.0, 0.0], forward: [0.0; 3], backward: [0.0; 3], tbc: [0.0; 3] };
    let slide = TransformTrack {
        translation: Channel::Keys(Keys { kind: KeyKind::Linear, keys: vec![key(0.0, 0.0), key(1.0, 300.0)] }),
        rotation: Channel::Static(None),
        euler: None,
        scale: Channel::Static(None),
        per_cycle: None,
    };
    anim.add_sequence("Forward", 1.0, 2, vec![(1, slide)]);
    anim
}

/// [`fake_cell`], with square (0, 0) also holding [`GATE`]: an activator (as
/// `prepare::reference` finds it) whose one collision object, a keyframed 100-unit wall,
/// hangs from the node its sequence moves.
fn gate_cell(job: &Job) -> Prepared {
    let mut p = fake_cell(job);
    if job.cell == PhysCell::Exterior(0, 0) {
        let pl = Placement {
            form_id: GATE,
            sig: *b"REFR",
            flags: 0,
            base: None,
            position: [GATE_X, 3000.0, 100.0],
            rotation: [0.0; 3],
            scale: 1.0,
            enable_parent: None,
            open_by_default: false,
            teleport: false,
            emittance: None,
            imposter: false,
            primitive: false,
        };
        let obj = CollisionObject { node: Some("Panel".into()), node_block: 1, shape: Shape::Box { half_extents: [50.0, 10.0, 100.0] }.into(), ..object(0.0, Motion::Keyframed) };
        let prepared = PreparedRef::new(&PhysParams::default(), &RefPose::from_placement(&pl), &[obj]);
        let animated = Some(prepare::AnimatedBodies { anim: Arc::new(gate_anim()), objects: vec![(0, 1, Iso::IDENTITY)] });
        p.refs.push(Entry { form_id: pl.form_id, placement: Some(pl), shapes: Some(RefShapes { tallies: vec!["fake"], water: Vec::new(), prepared: Some(prepared), dynamic_local: None, door: None, animated, drawn: None, model: None }) });
    }
    p
}

/// Issue #1396: an animated object's collision follows the group its drawn model plays
/// (the TTW Megaton gate stayed solid where it stood closed), stays at the end of a group
/// played once, and is posed there from the last group asked when nothing draws it (after
/// a load).
#[test]
fn an_animated_objects_collision_follows_its_group() {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = PhysState { fake: Some(gate_cell), loader: loader::Loader::manual(), ..Default::default() };
    p.load(&ctx, PhysCell::Exterior(0, 0));
    assert!(p.animated.contains_key(&GATE));
    let x = |p: &PhysState| p.world.ref_bodies(GATE)[0].translation[0];
    // A ray along y, 20 units off the ground, through where the panel stands closed.
    let blocks = |p: &mut PhysState, at: f32| p.world.queries().ray([at, 2900.0, 120.0], [0.0, 1.0, 0.0], 200.0, &Filter { no_terrain: true, ..Default::default() }).is_some_and(|h| matches!(h.owner, Owner::Ref { form_id: GATE, .. }));
    assert!((x(&p) - GATE_X).abs() < 1e-3);
    assert!(blocks(&mut p, GATE_X));
    // Nothing plays: the bind pose stays.
    p.pose_animated(None, |_| None);
    assert!((x(&p) - GATE_X).abs() < 1e-3);
    // Halfway through, as drawn.
    let mut poses = ObjectPoses::default();
    poses.playing.insert(GATE, ("Forward".into(), 0.5));
    p.pose_animated(Some(&poses), |_| None);
    assert!((x(&p) - (GATE_X + 150.0)).abs() < 1e-2, "{}", x(&p));
    // At its end: out of the way, solid where it is drawn open.
    poses.playing.insert(GATE, ("Forward".into(), 1.0));
    p.pose_animated(Some(&poses), |_| None);
    assert!((x(&p) - (GATE_X + 300.0)).abs() < 1e-2, "{}", x(&p));
    assert!(!blocks(&mut p, GATE_X), "the way through is open");
    assert!(blocks(&mut p, GATE_X + 300.0), "the open panel is solid");
    // Reloaded (the cell and its collision built again, nothing drawn yet): posed at the
    // end of the last group asked, which the saved game keeps.
    p.unload(PhysCell::Exterior(0, 0));
    assert!(!p.animated.contains_key(&GATE));
    p.load(&ctx, PhysCell::Exterior(0, 0));
    assert!((x(&p) - GATE_X).abs() < 1e-3, "built closed");
    p.pose_animated(None, |r| (r == GATE).then_some("forward"));
    assert!((x(&p) - (GATE_X + 300.0)).abs() < 1e-2, "{}", x(&p));
    assert!(!blocks(&mut p, GATE_X));
    // A sequence the model does not have, or none: the bind pose again.
    p.pose_animated(None, |_| Some("Backward"));
    assert!((x(&p) - GATE_X).abs() < 1e-3);
}

#[test]
fn the_pose_of_a_group_not_drawn_is_its_end_or_its_loops_start() {
    let mut anim = gate_anim();
    anim.add_sequence("Idle", 2.0, 0, Vec::new());
    assert_eq!(super::group_pose(&anim, None, Some("Forward")), (Some(0), 1.0));
    assert_eq!(super::group_pose(&anim, None, Some("idle")), (Some(1), 0.0));
    assert_eq!(super::group_pose(&anim, Some(&("Idle".into(), 0.7)), Some("Forward")), (Some(1), 0.7));
    // What is drawn but not in the collision's model: the last group asked.
    assert_eq!(super::group_pose(&anim, Some(&("Other".into(), 0.7)), Some("Forward")), (Some(0), 1.0));
    assert_eq!(super::group_pose(&anim, None, None), (None, 0.0));
}

#[test]
fn a_place_near_loaded_collision_only_is_refused_and_a_character_goes_exactly_where_told() {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = PhysState { fake: Some(fake_cell), loader: loader::Loader::manual(), ..Default::default() };
    let start = centre(0, 0);
    p.stream(&ctx, start, CELLS_PER_FRAME);
    // The loader has not delivered: only the cell underfoot is in.
    assert!(p.ready_at(start));
    assert!(p.ready_near(start, 256.0), "the middle of a square is far from its neighbours");
    assert!(!p.ready_near([CELL_SIZE - 10.0, start[1], 50.0], 256.0), "the square next door is not in");
    assert!(p.add_npc(7, [start[0] + 200.0, start[1] + 200.0, 200.0]));
    assert_eq!(p.npc_spot(7, [CELL_SIZE - 10.0, start[1], 50.0]), Err(NpcSpotRefused::NotLoaded));
    assert_eq!(p.npc_spot(8, start), Err(NpcSpotRefused::NotLoaded), "no character");
    // Everything in: a spot on the sloping terrain, and the character put on it to the unit.
    p.loader.run_manual(false);
    for _ in 0..8 {
        p.stream(&ctx, start, CELLS_PER_FRAME);
    }
    assert!(p.ready_near([CELL_SIZE - 10.0, start[1], 50.0], 256.0));
    let ground = p.terrain_z(start[0] + 200.0, start[1] - 400.0).expect("terrain");
    let at = [start[0] + 200.0, start[1] - 400.0, ground];
    let feet = p.npc_spot(7, at).expect("open ground");
    assert!((feet[2] - ground).abs() < 6.0, "{feet:?} on {ground}");
    p.npcs.get_mut(&7).unwrap().velocity = [100.0, 0.0];
    assert!(p.place_npc(7, feet));
    let b = &p.npcs[&7];
    assert_eq!((b.ch.feet, b.prev_feet, b.velocity), (feet, feet, [0.0, 0.0]));
    assert!(!p.place_npc(8, feet));
    // Too far over the ground for the level asked for.
    assert_eq!(p.npc_spot(7, [at[0], at[1], ground + 1000.0]), Err(NpcSpotRefused::NoSupport));
    // The player takes up room.
    p.place_player(feet, 48.0, 400.0);
    assert_eq!(p.npc_spot(7, [at[0] + 5.0, at[1], at[2]]), Err(NpcSpotRefused::NoRoom));
}

/// #3428: a placed item of several dynamic bodies (the teddy bear) becomes one dynamic
/// body, which the grab key can hold; a fixture of several (a hanging lamp, base STAT or
/// ACTI) is left for `hold_jointed`; an item of one body or with a fixed part is left too.
#[test]
fn a_jointed_item_is_joined_and_a_jointed_fixture_is_not() {
    let part = |x: f32, motion: Motion| CollisionObject { node_transform: Iso { translation: [x, 0.0, 0.0], rotation: Iso::IDENTITY.rotation }, mass: 1.0, ..object(2.0, motion) };
    let bear = || vec![part(0.0, Motion::Dynamic), part(5.0, Motion::Dynamic), part(-5.0, Motion::Dynamic)];
    let mut tallies = Vec::new();
    let mut item = bear();
    prepare::join_jointed_item(Some(*b"MISC"), &mut item, &mut tallies);
    assert_eq!((item.len(), item[0].motion, item[0].mass), (1, Motion::Dynamic, 3.0));
    assert_eq!(tallies, vec!["jointed_item_joined"]);
    for sig in [*b"STAT", *b"ACTI", *b"MSTT", *b"LIGH"] {
        let mut fixture = bear();
        prepare::join_jointed_item(Some(sig), &mut fixture, &mut tallies);
        assert_eq!(fixture.len(), 3, "{}", String::from_utf8_lossy(&sig));
    }
    let mut no_base = bear();
    prepare::join_jointed_item(None, &mut no_base, &mut tallies);
    assert_eq!(no_base.len(), 3);
    let mut with_fixed = vec![part(0.0, Motion::Dynamic), part(5.0, Motion::Static)];
    prepare::join_jointed_item(Some(*b"MISC"), &mut with_fixed, &mut tallies);
    assert_eq!(with_fixed.len(), 2);
    assert_eq!(tallies.len(), 1);
}

#[test]
fn several_dynamic_bodies_join_into_one_with_their_shapes_in_place() {
    let at = |x: f32| Iso { translation: [x, 0.0, 0.0], rotation: Iso::IDENTITY.rotation };
    let body = |x: f32, motion: Motion| CollisionObject {
        node: Some(format!("Bone{x}")),
        node_block: 0,
        node_transform: at(x),
        body_transform: at(1.0),
        motion,
        layer: 4,
        mass: 2.0,
        friction: 0.3,
        restitution: 0.8,
        shape: Arc::new(Shape::Sphere { radius: 3.0 }),
        shell: x / 10.0,
        material: None,
        wind: false,
    };
    let j = one_dynamic_body(&[body(0.0, Motion::Dynamic), body(10.0, Motion::Dynamic)]).unwrap();
    assert_eq!(j.node_transform.translation, [1.0, 0.0, 0.0]);
    assert_eq!(j.body_transform, Iso::IDENTITY);
    assert_eq!((j.mass, j.shell, j.layer), (4.0, 1.0, 4));
    let Shape::Compound { children } = &*j.shape else { panic!("{:?}", j.shape) };
    // The second body's shape sits 10 units along from the first's, as in the file.
    assert_eq!(children.iter().map(|(p, _)| p.translation[0]).collect::<Vec<_>>(), vec![0.0, 10.0]);
    // One body, or a static or keyframed one among them: left as they are.
    assert!(one_dynamic_body(&[body(0.0, Motion::Dynamic)]).is_none());
    assert!(one_dynamic_body(&[body(0.0, Motion::Dynamic), body(10.0, Motion::Keyframed)]).is_none());
    assert!(one_dynamic_body(&[body(0.0, Motion::Static), body(10.0, Motion::Dynamic)]).is_none());
}

/// Places in a smoothed cell (cell-local x, y) where the smoothed surface lies more than
/// 5 units off the stored triangles, from the south-west on.
fn lifted_spots(s: &fnv_world::land_surface::Surface) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for k in 0..4000 {
        let (x, y) = (300.0 + (k as f32 * 97.31) % 3500.0, 300.0 + (k as f32 * 61.17) % 3500.0);
        if s.lift_at(x, y).is_some_and(|l| l.abs() > 5.0) && out.iter().all(|&(a, b): &(f32, f32)| (a - x).hypot(b - y) > 400.0) {
            out.push((x, y));
        }
    }
    out
}

/// The top of the static box [`smooth_cell_with`] stands at (`x`, `y`): above the stored
/// triangles by the surface's bound and 4 units more, so above the smoothed surface too
/// whatever the bound (12 units at the default's 8).
fn box_top(s: &fnv_world::land_surface::Surface, x: f32, y: f32) -> f32 {
    s.stored_height_at(x, y).unwrap() + s.bound() + 4.0
}

/// The default smoothing's made-up cell ([`smooth_cell_with`]).
fn smooth_cell(job: &Job) -> Prepared {
    smooth_cell_with(job, fnv_world::land_surface::SmoothParams::DEFAULT)
}

/// The previous default, kept explicitly so the interpolation mode stays covered.
fn interpolated_cell(job: &Job) -> Prepared {
    use fnv_world::land_surface::{SmoothMode, SmoothParams};
    smooth_cell_with(job, SmoothParams { mode: SmoothMode::Interpolate, subdiv: 2, max_deviation: 8.0 })
}

/// The same cell fitted, 4 × 4, within 16 units: its LAND vertices move ([`smooth_cell_with`]).
fn fitted_cell(job: &Job) -> Prepared {
    use fnv_world::land_surface::{SmoothMode, SmoothParams};
    smooth_cell_with(job, SmoothParams { mode: SmoothMode::Fit, subdiv: 4, max_deviation: 16.0 })
}

/// The made-up smoothed cells the landscape tests run on, the default first.
const SMOOTH_CELLS: [fn(&Job) -> Prepared; 3] = [smooth_cell, interpolated_cell, fitted_cell];

/// A made-up exterior cell on a smoothed landscape (#2149): hills in steps of 8, and a
/// static box standing on the ground at the second of [`lifted_spots`], its top at
/// [`box_top`].
fn smooth_cell_with(job: &Job, params: fnv_world::land_surface::SmoothParams) -> Prepared {
    let PhysCell::Exterior(gx, gy) = job.cell else { panic!("exterior cells only") };
    let heights: Vec<f32> = (0..SIDE * SIDE)
        .map(|i| {
            let (r, c) = ((i / SIDE) as f32, (i % SIDE) as f32);
            ((140.0 * (c * 0.7).sin() * (r * 0.5).cos() + 60.0 * ((c - r) * 0.9).sin()) / 8.0).round() * 8.0
        })
        .collect();
    let mut block: fnv_world::land_surface::Block = [[None; 3]; 3];
    block[1][1] = Some(&heights);
    let surface = fnv_world::land_surface::Surface::smoothed(&block, params).expect("heights");
    let params = PhysParams::default();
    let (x0, y0) = (gx as f32 * CELL_SIZE, gy as f32 * CELL_SIZE);
    let (bx, by) = lifted_spots(&surface)[1];
    let top = box_top(&surface, bx, by);
    let pl = Placement {
        form_id: 0x0200_0000,
        sig: *b"REFR",
        flags: 0,
        base: None,
        position: [x0 + bx, y0 + by, top - 50.0],
        rotation: [0.0; 3],
        scale: 1.0,
        enable_parent: None,
        open_by_default: false,
        teleport: false,
        emittance: None,
        imposter: false,
        primitive: false,
    };
    let prepared = PreparedRef::new(&params, &RefPose::from_placement(&pl), &[object(50.0, Motion::Static)]);
    let refs = vec![Entry { form_id: pl.form_id, placement: Some(pl), shapes: Some(RefShapes { tallies: vec!["fake"], water: Vec::new(), prepared: Some(prepared), dynamic_local: None, door: None, animated: None, drawn: None, model: None }) }];
    let terrain = Some(PreparedTerrain::from_surface(&params, (gx, gy), &surface));
    Prepared { cell: job.cell, surface: Some(surface), terrain, refs, times: LoadTimes::default() }
}

/// An actor standing without a character at an authored or navmesh height over the bare
/// landscape is drawn on the smoothed surface as it stood on the stored triangles; at
/// furniture, on a static or well above the ground it stays where its feet are (#2149).
#[test]
fn an_actor_standing_on_the_landscape_follows_the_smoothed_surface() {
    for make in SMOOTH_CELLS {
        an_actor_standing_on_the_landscape_follows(make);
    }
}

fn an_actor_standing_on_the_landscape_follows(make: fn(&Job) -> Prepared) {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = PhysState { fake: Some(make), loader: loader::Loader::manual(), ..Default::default() };
    p.stream(&ctx, centre(0, 0), CELLS_PER_FRAME);
    let s = p.cells[&PhysCell::Exterior(0, 0)].surface.clone().expect("terrain");
    let spots = lifted_spots(&s);
    let (x, y) = spots[0];
    let (stored, smooth) = (s.stored_height_at(x, y).unwrap(), s.height_at(x, y).unwrap());
    assert!((smooth - stored).abs() > 5.0);
    assert_eq!(p.terrain_z(x, y), Some(smooth), "the collision is the smoothed surface");
    // On the stored ground (an authored height): drawn on the smoothed ground.
    let drawn = p.standing_drawn_feet(1, [x, y, stored], true);
    assert!((drawn[2] - smooth).abs() < 1e-3, "{drawn:?}, ground {smooth}");
    assert!((p.drawn_sink_of(1) - (stored - smooth)).abs() < 1e-3);
    // A navmesh height 6 units over the stored ground: 6 over the smoothed.
    let drawn = p.standing_drawn_feet(1, [x, y, stored + 6.0], true);
    assert!((drawn[2] - (smooth + 6.0)).abs() < 1e-3, "{drawn:?}");
    // At furniture: where its feet are.
    assert_eq!(p.standing_drawn_feet(1, [x, y, stored], false), [x, y, stored]);
    // Far above the ground (on something the collision does not know): where its feet are.
    assert_eq!(p.standing_drawn_feet(2, [x, y, stored + 200.0], true), [x, y, stored + 200.0]);
    // Resting on the collision as a character would: on the drawn ground, as before.
    let rest = p.standing_drawn_feet(3, [x, y, smooth + fnv_phys::TERRAIN_SHELL], true);
    assert!((rest[2] - smooth).abs() < 0.1, "{rest:?}");
    // On the static box standing on the ground: where its feet are.
    let (bx, by) = spots[1];
    let top = box_top(&s, bx, by);
    // (On the box's top as the ray finds it: within a float's rounding of `top`.)
    let on_box = p.standing_drawn_feet(4, [bx, by, top], true);
    assert!(on_box[..2] == [bx, by] && (on_box[2] - top).abs() < 1e-3, "{on_box:?}, box top {top}");
    assert_eq!(p.landscape_lift([bx, by, top]), None);
    // At a LAND vertex the fit moved: the collision and the lift are the moved height's.
    if s.spec().knot_shift() > 0.0 {
        let moved = (2..31).flat_map(|r| (2..31).map(move |c| (r, c))).find(|&(r, c)| (s.heights()[s.land_vertex(r, c)] - s.land_height(r, c)).abs() > 1.0);
        let (r, c) = moved.expect("the fit moves some LAND vertex");
        let (kx, ky) = (c as f32 * fnv_world::land::SPACING, r as f32 * fnv_world::land::SPACING);
        let knot = s.heights()[s.land_vertex(r, c)];
        assert_eq!(p.terrain_z(kx, ky), Some(knot));
        assert!((s.lift_at(kx, ky).unwrap() - (knot - s.land_height(r, c))).abs() < 1e-3);
        let drawn = p.standing_drawn_feet(6, [kx, ky, s.land_height(r, c)], true);
        assert!((drawn[2] - knot).abs() < 1e-3, "{drawn:?}, moved knot {knot}");
    }
    // Indoors nothing follows the landscape.
    p.space = Space::Interior(7);
    assert_eq!(p.landscape_lift([x, y, stored]), None);
}

/// An actor asked about before the collision under it loads is looked at again once it
/// has: the lift is not kept at none (#2149, review round 2).
#[test]
fn a_standing_actor_is_looked_at_again_when_its_collision_loads() {
    for make in SMOOTH_CELLS {
        a_standing_actor_is_looked_at_again_when_its_collision_loads_on(make);
    }
}

fn a_standing_actor_is_looked_at_again_when_its_collision_loads_on(make: fn(&Job) -> Prepared) {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = PhysState { fake: Some(make), loader: loader::Loader::manual(), ..Default::default() };
    p.stream(&ctx, centre(0, 0), CELLS_PER_FRAME);
    assert!(!p.cells.contains_key(&PhysCell::Exterior(1, 0)), "only the cell underfoot is in");
    // The made-up cells all have the same landscape: (0, 0)'s spots are (1, 0)'s too.
    let s = p.cells[&PhysCell::Exterior(0, 0)].surface.clone().expect("terrain");
    let (x, y) = lifted_spots(&s)[0];
    let (stored, smooth) = (s.stored_height_at(x, y).unwrap(), s.height_at(x, y).unwrap());
    let feet = [CELL_SIZE + x, y, stored];
    assert_eq!(p.standing_drawn_feet(5, feet, true), feet, "no collision there yet: where its feet are");
    p.loader.run_manual(false);
    for _ in 0..8 {
        p.stream(&ctx, centre(0, 0), CELLS_PER_FRAME);
    }
    assert!(p.cells.contains_key(&PhysCell::Exterior(1, 0)));
    let drawn = p.standing_drawn_feet(5, feet, true);
    assert!((drawn[2] - smooth).abs() < 1e-3, "the same feet, now on the smoothed ground: {drawn:?}, ground {smooth}");
    // And back when the collision goes again.
    p.unload(PhysCell::Exterior(1, 0));
    assert_eq!(p.standing_drawn_feet(5, feet, true), feet);
}

/// A character that walks and stops on a sloping, smoothed landscape and is then taken
/// away is drawn where it was drawn: its feet came from physics and are not lifted onto
/// the landscape a second time (#2149, review round 2).
#[test]
fn feet_a_character_left_are_drawn_as_the_character_was() {
    for make in SMOOTH_CELLS {
        feet_a_character_left_are_drawn_as_the_character_was_on(make);
    }
}

fn feet_a_character_left_are_drawn_as_the_character_was_on(make: fn(&Job) -> Prepared) {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = PhysState { fake: Some(make), loader: loader::Loader::manual(), ..Default::default() };
    p.stream(&ctx, centre(0, 0), CELLS_PER_FRAME);
    let s = p.cells[&PhysCell::Exterior(0, 0)].surface.clone().expect("terrain");
    let spots = lifted_spots(&s);
    let (x, y) = spots[spots.len() - 1];
    assert!(p.add_npc(7, [x, y, s.height_at(x, y).unwrap() + 20.0]));
    for i in 0..90 {
        let go = if i < 45 { [120.0, 60.0] } else { [0.0, 0.0] };
        let b = p.npcs.get_mut(&7).unwrap();
        p.world.move_character(&mut b.ch, CharacterInput { velocity: go, ..Default::default() });
        p.world.step();
    }
    p.update_drawn_sinks();
    let b = &p.npcs[&7];
    let (feet, sink) = (b.ch.feet, b.sink);
    assert!(b.ch.grounded, "it came to rest on the landscape: {feet:?}");
    assert!((feet[0] - x).hypot(feet[1] - y) > 1.0, "it walked: {feet:?} from ({x}, {y})");
    p.remove_npc(7);
    let drawn = p.standing_drawn_feet(7, feet, true);
    assert_eq!(drawn, [feet[0], feet[1], feet[2] - sink], "drawn where its character was drawn");
    // Feet moved since (a walk on the navmesh): no longer the character's.
    let moved = [feet[0] + 50.0, feet[1], feet[2]];
    let _ = p.standing_drawn_feet(7, moved, true);
    assert!(!p.left.contains_key(&7));
}

/// A made-up exterior cell with no references and steep terrain: rising 0.4 units a unit
/// eastwards (about 22 degrees), for the lifts out of the terrain (#2753).
fn steep_cell(job: &Job) -> Prepared {
    let PhysCell::Exterior(gx, gy) = job.cell else { panic!("exterior cells only") };
    let x0 = gx as f32 * CELL_SIZE;
    let heights: Vec<f32> = (0..SIDE * SIDE).map(|i| 0.4 * (x0 + (i % SIDE) as f32 * 128.0)).collect();
    let terrain = Some(PreparedTerrain::new(&PhysParams::default(), (gx, gy), &heights));
    Prepared { cell: job.cell, surface: Some(fnv_world::land_surface::Surface::stored(&heights)), terrain, refs: Vec::new(), times: LoadTimes::default() }
}

/// [`steep_cell`] (0, 0) loaded, and the terrain height at (2000, 2000) in it.
fn steep() -> (PhysState, f32) {
    let (d, sim) = (play_data(), sim());
    let ctx = LoadCtx { d: &d, sim: &sim, data: None };
    let mut p = PhysState { fake: Some(steep_cell), loader: loader::Loader::manual(), ..Default::default() };
    p.load(&ctx, PhysCell::Exterior(0, 0));
    let t = p.terrain_z(2000.0, 2000.0).expect("terrain");
    assert!((t - 800.0).abs() < 1e-2, "{t}");
    (p, t)
}

fn box_at(p: &mut PhysState, form_id: u32, at: [f32; 3], rotation: [f32; 9], o: CollisionObject) {
    p.world.add_ref(&RefPose { form_id, position: at, rotation, scale: 1.0 }, &[o]);
}

const LEVEL: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

#[test]
fn a_drop_started_under_the_terrain_is_lifted_onto_it_and_rests_there() {
    let (mut p, t) = steep();
    let id = 0x0900_0001;
    box_at(&mut p, id, [2000.0, 2000.0, t - 30.0], LEVEL, object(4.0, Motion::Dynamic));
    assert!(p.lift_out_of_terrain(id), "its centre is 30 under the terrain");
    let c = p.world.ref_center(id).expect("bodies");
    assert!(c[2] > t, "lifted over the terrain: {c:?} against {t}");
    for _ in 0..300 {
        p.world.step();
    }
    let c = p.world.ref_center(id).expect("bodies");
    let ground = p.terrain_z(c[0], c[1]).expect("terrain");
    assert!(c[2] > ground, "it stays on the ground: {c:?} against {ground}");
}

#[test]
fn a_long_drop_resting_on_a_slope_is_not_lifted_when_added_again() {
    // A rifle-sized box lying down the slope (as one at rest is added again by a cell load):
    // its downhill end reaches far under the terrain under its centre, its centre does not.
    let (mut p, t) = steep();
    let id = 0x0900_0002;
    let (s, c) = (0.4f32.atan().sin(), 0.4f32.atan().cos());
    let long = CollisionObject { shape: Shape::Box { half_extents: [50.0, 4.0, 4.0] }.into(), ..object(4.0, Motion::Dynamic) };
    let at = [2000.0, 2000.0, t + 4.0 / c + fnv_phys::TERRAIN_SHELL];
    box_at(&mut p, id, at, [c, 0.0, -s, 0.0, 1.0, 0.0, s, 0.0, c], long);
    let low = p.world.ref_lowest_z(id).expect("bodies");
    assert!(t + fnv_phys::TERRAIN_SHELL - low > LIFT_ONTO_SUPPORT_MOST, "measured under its centre it reaches into the terrain ({low} against {t})");
    let before = p.world.ref_center(id).expect("bodies");
    assert!(!p.lift_out_of_terrain(id));
    assert_eq!(p.world.ref_center(id), Some(before));
}

#[test]
fn a_drop_over_a_floor_below_the_terrain_is_not_lifted() {
    let (mut p, t) = steep();
    // A floor (a static box) whose top is 250 under the terrain, and a drop just over it.
    box_at(&mut p, 0x0900_0010, [2000.0, 2000.0, t - 300.0], LEVEL, object(50.0, Motion::Static));
    let id = 0x0900_0003;
    box_at(&mut p, id, [2000.0, 2000.0, t - 240.0], LEVEL, object(4.0, Motion::Dynamic));
    assert!(!p.lift_out_of_terrain(id));
}

#[test]
fn a_ragdoll_started_under_the_terrain_rises_onto_what_stands_there() {
    use super::sinking::{SANK_NOTE, START_CLEARANCE};
    let (mut p, t) = steep();
    let t2 = p.terrain_z(2030.0, 2000.0).expect("terrain");
    // Two bodies 3 under the terrain each (a placed-dead corpse a little in the ground).
    let pose = [(0, [2000.0, 2000.0, t - 3.0]), (1, [2030.0, 2000.0, t2 - 3.0])];
    let lift = p.lift_from_under(&pose, 0.0).expect("under the terrain");
    assert!((lift.up - (3.0 + START_CLEARANCE)).abs() < 0.1, "{lift:?}");
    assert!(lift.onto.starts_with("terrain"), "{lift:?}");
    // A saved pose so little under is left as it is.
    assert_eq!(p.lift_from_under(&pose, SANK_NOTE), None);
    // Over the surface: nothing to do.
    assert_eq!(p.lift_from_under(&[(0, [2000.0, 2000.0, t + 10.0])], 0.0), None);
    // A rock on the terrain there, its top 80 over it: lifted onto the rock, not into it.
    box_at(&mut p, 0x0900_0011, [2000.0, 2000.0, t + 30.0], LEVEL, object(50.0, Motion::Static));
    let lift = p.lift_from_under(&[(0, [2000.0, 2000.0, t - 30.0])], 0.0).expect("under the terrain");
    assert!((lift.up - (110.0 + START_CLEARANCE)).abs() < 0.5, "{lift:?}");
    assert!(lift.onto.starts_with("ref 09000011"), "{lift:?}");
}

#[test]
fn a_ragdoll_standing_on_a_floor_below_the_terrain_is_not_lifted() {
    let (mut p, t) = steep();
    // A floor whose top is 350 under the terrain; the feet 5 over it. The head lies uphill,
    // about 440 under the terrain there with no floor under it: the body that needs the most
    // lift, and the one a check under the neediest body alone would have lifted from.
    box_at(&mut p, 0x0900_0012, [2000.0, 2000.0, t - 400.0], LEVEL, object(50.0, Motion::Static));
    let pose = [(0, [2600.0, 2000.0, t - 200.0]), (1, [2000.0, 2010.0, t - 345.0])];
    assert_eq!(p.lift_from_under(&pose, 0.0), None);
}
