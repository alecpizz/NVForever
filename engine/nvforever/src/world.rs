//! WorldStreamingPlugin: load the cells around the camera and spawn their terrain and
//! references; unload cells that fall out of range.

use crate::fade::CellFade;
use crate::terrain::{prepare_terrain, TerrainCtx, TerrainMaterials, TerrainPrep};
use crate::water::{cell_water_center, water_material, WaterCtx};
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, Task};
use fnv_esm::{LoadOrder, Plugin as EsmPlugin, Schema as EsmSchema};
use fnv_world::imposter::ImposterRegions;
use fnv_world::placed::{base_object, placement, BaseObject, Placement};
use fnv_world::transform::ref_rotation;
use fnv_world::alt_textures::AltTexture;
use fnv_world::{cell_of, WorldIndex, CELL_SIZE};
use nvf_assets::actor::{ActorCtx, ActorParts};
use nvf_assets::model::{ModelCtx, PreparedModel};
use nvf_assets::{nif_material, NifMaterial};
use nvf_play::{RefVisual, RenderCell};
use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;
use std::path::{Path, PathBuf};

/// Base object types drawn as static models. NPCs and creatures are assembled from parts
/// (`actors.rs`); sounds and idle markers have no model. PROJ: placed mines and charges
/// (PGRE), drawn with their projectile's model (`nvf_play::mines`).
const DRAWN: [&[u8; 4]; 25] = [
    b"STAT", b"SCOL", b"MSTT", b"TREE", b"FURN", b"DOOR", b"CONT", b"ACTI", b"TACT", b"TERM", b"LIGH", b"MISC",
    b"WEAP", b"AMMO", b"ARMO", b"ALCH", b"BOOK", b"NOTE", b"KEYM", b"PWAT", b"IMOD", b"CHIP", b"CCRD", b"CMNY", b"PROJ",
];

#[derive(Resource)]
pub struct WorldData {
    /// Leaked (they live for the whole run) so `nvf-play` can share them.
    pub lo: &'static LoadOrder<'static>,
    pub schema: &'static EsmSchema,
    pub index: &'static WorldIndex,
    pub world: u32,
    /// The worldspace's persistent references by the grid square they stand in.
    pub persistent_by_grid: HashMap<(i32, i32), Vec<u32>>,
    /// Regions with imposter lists (imposters.rs).
    pub imposters: &'static ImposterRegions,
    /// Regions with weather lists (sky.rs `weather_region`).
    pub weather_regions: &'static [fnv_world::sky::RegionWeather],
    /// Plugins left out of the load order, one line each ([`open_plugins`]); logged once
    /// the log is up.
    pub dropped_plugins: Vec<String>,
}

/// The plugins of a load order, opened, and a line for each one left out.
pub struct OpenedPlugins {
    pub plugins: Vec<EsmPlugin>,
    pub dropped: Vec<String>,
}

/// Open the plugins `names` (load order) for the game, each from the file `file_of` names
/// (main: the enabled mod layers then Data, or `--mo2`'s folders then Data). A plugin that cannot
/// be read or parsed (a truncated or broken mod) is left out, and then every plugin the
/// load order would refuse: a missing master, a master that loads after it, a second
/// copy, past the 255th (`fnv_esm::loadorder::drop_unloadable`). The game goes on without
/// them; a line for each is printed now (the log is not up yet) and kept for the log.
pub fn open_plugins(names: &[String], file_of: impl Fn(&str) -> PathBuf) -> OpenedPlugins {
    let mut dropped = Vec::new();
    let mut plugins = Vec::with_capacity(names.len());
    for n in names {
        // The file whatever its case on disk (a case-sensitive copy may hold
        // `deadmoney.esm`; #995); the plugin keeps the name the load order gives it.
        let path = file_of(n);
        match EsmPlugin::open(path) {
            Ok(mut p) => {
                p.name = n.clone();
                plugins.push(p)
            }
            Err(e) => dropped.push(format!("plugin {n} left out: cannot be read: {e}")),
        }
    }
    let (plugins, refused) = fnv_esm::loadorder::drop_unloadable(plugins);
    dropped.extend(refused.into_iter().map(|d| format!("plugin {} left out: {}", d.plugin, d.reason)));
    for d in &dropped {
        eprintln!("warning: {d}");
    }
    OpenedPlugins { plugins, dropped }
}

/// Per-session random seed for Base Object Swapper `chanceR(...)` rolls [design].
/// Combines OS-seeded `RandomState` entropy with wall-clock nanoseconds and process ID so
/// each run starts with an independent `chanceR` seed while `build_bos_table` /
/// `BosTable::with_random_seed` preserve deterministic control in tests.
fn bos_session_seed() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0u128, |d| d.as_nanos());
    h.write_u128(nanos);
    h.write_u32(std::process::id());
    h.finish()
}

fn build_bos_table(lo: &LoadOrder, bos_sources: &[(String, String)], random_seed: u64) -> fnv_world::BosTable {
    fnv_world::BosTable::load_from_sources(
        lo,
        bos_sources.iter().map(|(name, text)| (name.as_str(), text.as_str())),
    )
    .with_random_seed(random_seed)
}

impl WorldData {
    pub fn open(
        schema_dir: &Path,
        opened: OpenedPlugins,
        world: &str,
        bos_sources: Vec<(String, String)>,
        merge_region_lists: bool,
    ) -> Result<WorldData, String> {
        let schema: &'static EsmSchema =
            Box::leak(Box::new(EsmSchema::load_dir(schema_dir).map_err(|e| format!("{}: {e}", schema_dir.display()))?));
        let OpenedPlugins { plugins, dropped: dropped_plugins } = opened;
        let plugins: &'static [EsmPlugin] = Box::leak(plugins.into_boxed_slice());
        let lo: &'static LoadOrder<'static> = Box::leak(Box::new(LoadOrder::new(plugins).map_err(|e| e.to_string())?));
        let mut built = WorldIndex::build(lo, schema);
        if merge_region_lists {
            built.set_merge_region_lists(true);
            info!("hNVSE: bMergeRegionLists=1 enabled (merging exterior CELL XCLR region lists across overrides)");
        }
        if !bos_sources.is_empty() {
            let bos = build_bos_table(lo, &bos_sources, bos_session_seed());
            let rep = bos.report();
            if rep.files_loaded > 0 {
                info!(
                    "base object swapper: {} file(s), {} form rule(s), {} reference rule(s), {} skipped",
                    rep.files_loaded,
                    rep.form_rules,
                    rep.reference_rules,
                    rep.total_skipped(),
                );
            }
            built.apply_bos(bos);
        }
        let index: &'static WorldIndex = Box::leak(Box::new(built));
        let w = index.world_by_name(world).ok_or_else(|| format!("no worldspace {world}"))?;
        let imposters: &'static ImposterRegions = Box::leak(Box::new(ImposterRegions::build(lo, schema)));
        let weather_regions: &'static [fnv_world::sky::RegionWeather] = Box::leak(fnv_world::sky::region_weathers(lo, schema).into_boxed_slice());
        let mut data = WorldData { lo, schema, index, world: w.form_id, persistent_by_grid: HashMap::new(), imposters, weather_regions, dropped_plugins };
        data.set_world(w.form_id);
        Ok(data)
    }

    /// Draw worldspace `world` (FormID): index its persistent references by grid square.
    pub fn set_world(&mut self, world: u32) {
        self.world = world;
        self.persistent_by_grid.clear();
        let pc = self.index.worldspaces.get(&world).and_then(|w| w.persistent_cell).and_then(|c| self.index.cells.get(&c));
        for &r in pc.map_or(&[][..], |c| &c.refs) {
            if let Some(p) = placement(self.lo, self.schema, r) {
                self.persistent_by_grid.entry(cell_of(p.position[0], p.position[1])).or_default().push(r);
            }
        }
    }

    pub fn grid_cell(&self, g: (i32, i32)) -> Option<&fnv_world::Cell> {
        let w = self.index.worldspaces.get(&self.world)?;
        self.index.cells.get(w.grid.get(&g)?)
    }

    /// The LAND of grid square `g` and of the eight around it
    /// (`fnv_world::land_surface::LandIds`): a cell's texture layers are stitched to its
    /// side neighbours' ([`crate::terrain::NEIGHBOURS`]) and its smoothed surface reads
    /// their heights along its edges (#2149).
    pub fn land_ids(&self, g: (i32, i32)) -> fnv_world::land_surface::LandIds {
        fnv_world::land_surface::land_ids(g, |n| self.grid_cell(n).and_then(|c| c.land))
    }

    /// The landscape surface of grid square `g` as this run draws and collides it (smoothed
    /// or as stored, `nvf_play::smooth_terrain::spec`); `None` without landscape.
    pub fn land_surface(&self, g: (i32, i32)) -> Option<fnv_world::land_surface::Surface> {
        fnv_world::land_surface::load_surface(self.lo, self.schema, &self.land_ids(g), nvf_play::smooth_terrain::spec())
    }

    /// Whether a reference is enabled at the start of a game: not deleted, and its enable
    /// parent in the required state, or with no parent not initially disabled [documented:
    /// XESP; the parent chain is followed, cycles count as disabled]. Under a parent its
    /// own flag does not count, as `fnv_sim::World::is_enabled` has it (simulation.md §1).
    pub fn enabled(&self, p: &Placement, depth: usize) -> bool {
        enabled(self.lo, self.schema, p, depth)
    }
}

/// [`WorldData::enabled`] from the load order (for worker threads).
pub fn enabled(lo: &LoadOrder, schema: &EsmSchema, p: &Placement, depth: usize) -> bool {
    if p.deleted() {
        return false;
    }
    match p.enable_parent {
        None => !p.initially_disabled(),
        Some(_) if depth > 16 => false,
        Some((parent, opposite)) => match placement(lo, schema, parent) {
            Some(pp) => enabled(lo, schema, &pp, depth + 1) != opposite,
            None => !opposite && !p.initially_disabled(),
        },
    }
}

/// Marks the root that maps game space (Z up) to Bevy space (Y up).
#[derive(Component)]
pub struct WorldRoot;

#[derive(Component)]
pub struct CellEntity;

/// A SpeedTree reference drawn as a billboard; `face_billboards` keeps it facing the camera.
#[derive(Component)]
pub struct TreeBillboard;

#[derive(Resource)]
pub struct Streamer {
    /// Cells within this many grid steps of the camera's cell are loaded.
    pub radius: i32,
    /// Cells drawn, including exterior cells still fading out.
    pub loaded: HashMap<(i32, i32), Entity>,
    /// Exterior cells fading in or out (fade.rs); loaded cells not in here are fully drawn.
    pub fades: HashMap<(i32, i32), CellFade>,
    /// Seconds a cell takes to fade in or out; 0 switches cells at once.
    pub fade_secs: f32,
    /// The cells around the start (or a door's destination) are in: cells loaded from now
    /// on fade in (the first ones appear at once, as behind a loading screen).
    pub settled: bool,
    /// The grid square a door's travel arrives in, outdoors: the cells are loaded around
    /// it, not around the camera, until they are all in (`settled`). The frame of the
    /// travel still has the camera's transform of the place left: around it, cells
    /// of empty land far away loaded at once and counted as the destination's, so the
    /// loading screen came down with the arrival's own cells still to load, and they
    /// faded in in front of the player (playtest 2026-10-12, items 9 and 10).
    pub arrival: Option<(i32, i32)>,
    /// Cells started per frame (spawned whole before the start square is in, over frames
    /// after it: [`CellSpawn`]).
    pub per_frame: usize,
    /// Cells whose references are being spawned, hidden until all are in.
    pub spawning: HashMap<(i32, i32), CellSpawn>,
    /// Cells gone, hidden, despawned a slice a frame.
    pub dying: Vec<Entity>,
    /// The cells around the loaded ones read ahead on worker threads.
    pub ahead: ReadAhead,
    /// Model path fragments not to draw (diagnostics).
    pub hide: Vec<String>,
    /// Interior mode: this one cell instead of the exterior grid.
    pub interior: Option<u32>,
    /// Multiplier on every LIGH light's brightness (1 = as stored).
    pub light_scale: f32,
    /// References another system draws (NPCs travelling on the navmesh, nav.rs): cells
    /// do not spawn them.
    pub managed: std::collections::HashSet<u32>,
    /// Every placed actor and creature is another system's (`residents.rs`, with
    /// `--npc-packages all`): cells spawn none.
    pub manage_actors: bool,
    pub stats: StreamStats,
    /// The cells drawn most recently, for the AI's process levels (`cell_buffer.rs`).
    pub buffer: crate::cell_buffer::CellBuffer,
    /// The exterior cell the loaded grid is around (the camera's, or a travel's arrival).
    pub centre: Option<(i32, i32)>,
    /// Placed actors of loaded cells whose parts are still being read on a worker thread
    /// (`readahead::ready_to_draw`), drawn by [`draw_waiting_actors`] once they are in.
    pub waiting: Vec<WaitingActor>,
    /// Destroyed references drawn with their destruction stage's replacement model
    /// (`destroyed.rs`): kept here so a cell drawn again draws them so.
    pub model_overrides: HashMap<u32, String>,
    /// Co-op replication, the host's: its guests' surroundings, away from the host's own
    /// (`coop::replication`). Not drawn here; their residents run in the high process as
    /// those drawn do (`residents::simulated`).
    pub coop_areas: Vec<fnv_ai::LoadedArea>,
    /// Door travels so far ([`travel`]): what drew the place left must go (the full-detail
    /// ring, full_detail.rs, clears itself when it changes).
    pub travels: u64,
}

/// `base` drawn with the model `path` (a DMDL replacement model) instead of its own.
pub fn base_object_with_model(base: BaseObject, path: &str) -> BaseObject {
    BaseObject { world_model: Some(path.to_string()), ..base }
}

/// A placed actor or creature of a loaded cell, drawn once its parts are read.
pub struct WaitingActor {
    cell: Entity,
    rt: Affine3A,
    at: Vec3,
    reference: u32,
    base: u32,
    creature: bool,
    hidden: bool,
}

impl Streamer {
    /// How much of an exterior cell is drawn: 0 not loaded … 1 fully drawn.
    pub fn cell_level(&self, c: (i32, i32)) -> f32 {
        match self.fades.get(&c) {
            Some(f) => f.level,
            None if self.loaded.contains_key(&c) => 1.0,
            None => 0.0,
        }
    }
}

/// Which cell to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellKey {
    Exterior((i32, i32)),
    Interior(u32),
}

#[derive(Debug, Default, Clone)]
pub struct StreamStats {
    /// Cumulative cell construction and reference spawn attempts, including work that
    /// produces no visual [design, #2248: begin_cell, spawn_ref; quality_probe.rs].
    pub cell_starts: u64,
    pub reference_attempts: u64,
    pub cells_loaded: usize,
    pub refs_drawn: usize,
    pub refs_skipped_disabled: usize,
    pub refs_skipped_type: usize,
    pub refs_no_model: usize,
    /// References whose model failed to load, drawn as the game's error marker.
    pub refs_error_marker: usize,
    /// TREE references (SpeedTree, generated or as billboards).
    pub refs_speedtree: usize,
    /// References drawn with their own node hierarchy, playing an Idle sequence.
    pub refs_animated: usize,
    /// Of those, references whose base has "Random Anim Start" (a spread start phase).
    pub refs_random_start: usize,
    /// Doors that open in place, drawn with their node hierarchy (`crate::doors`).
    pub refs_doors: usize,
    /// Drawn references whose model has named sequences scripts can play (object_anims.rs).
    pub refs_group_rigs: usize,
    /// References drawn in the pose their `XRGD` stores (clutter skeletons, `nvf_play::clutter_pose`).
    pub refs_posed: usize,
    /// `NiBillboardNode`s of placed models drawn turning to the camera (`face_camera`,
    /// #2549), on both paths, and the references they belong to.
    pub billboards: usize,
    pub refs_billboards: usize,
    /// Imposters met in loaded cells, left to `crate::imposters`.
    pub refs_imposters: usize,
    pub water_planes: usize,
    pub lights: usize,
    /// Light sources of references not enabled at the start, spawned hidden.
    pub lights_hidden: usize,
    pub actors: crate::actors::ActorStats,
    pub shapes: usize,
    pub load_ms: f64,
    /// Frames the cells loaded took to spawn, all told.
    pub spawn_frames: usize,
    /// Cells that waited for their read-ahead, and cells drawn before it was done.
    pub cells_waited: usize,
    pub cells_forced: usize,
    pub skipped_types: HashMap<String, usize>,
    /// Frames [`stream`] found the main view's camera and the world root and went on
    /// ([`note_other_cameras`] reads it).
    pub frames: u64,
}

/// Game space → Bevy space.
pub fn game_to_bevy() -> Transform {
    Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2))
}

/// Bevy-space position → game-space position.
pub fn bevy_to_game(p: Vec3) -> Vec3 {
    Vec3::new(p.x, -p.z, p.y)
}

/// `World`, with a parent above it (issue #1387). Bevy assigns the `GlobalTransform` of a root
/// whose tree is dirty, which marks it changed even with the same value, and then has to
/// recompute every direct child; a child's goes through `set_if_neq`, so with this parent
/// `World`'s stays unchanged when only something under it moves, and its clean direct
/// children stop at their test [design: bevy_transform 0.20.1 `src/systems.rs` :529, :707-718].
///
/// The parent has only `World` under it and stays identity: particles.rs and shot_fx.rs read
/// `World`'s local matrix as the game-to-Bevy conversion. What is directly under `World` stays
/// so (nav.rs `show_actor` finds a driven body by its parent being `World`).
pub fn setup_root(mut commands: Commands) {
    let parent = commands.spawn((Name::new("World parent"), Transform::IDENTITY, Visibility::default())).id();
    commands.spawn((Name::new("World"), WorldRoot, game_to_bevy(), Visibility::default(), ChildOf(parent)));
}

/// [`stream`]'s actor resources and the frame's pace, as one parameter (a system takes at
/// most 16).
type StreamActors<'w> = (Option<Res<'w, crate::actors::ActorWorld>>, Option<Res<'w, nvf_play::sim::SimState>>, ResMut<'w, crate::readahead::ReadAhead>, Res<'w, crate::stream_pace::StreamPace>);

#[allow(clippy::too_many_arguments)]
pub fn stream(
    mut commands: Commands,
    world: Res<WorldData>,
    mut streamer: ResMut<Streamer>,
    root: Query<Entity, With<WorldRoot>>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    mut models: ModelCtx,
    mut terrain: TerrainMaterials,
    mut water: WaterCtx,
    mut trees: crate::tree::TreeCtx,
    mut grass: crate::grass::GrassCtx,
    mut emittance: ResMut<crate::emittance::EmittanceState>,
    time: Res<Time>,
    (actor_world, sim, mut ahead, pace): StreamActors,
    mut actor_parts: ResMut<ActorParts>,
    loading: Option<Res<nvf_play::loading::Loading>>,
    children: Query<&Children>,
) {
    let (Ok(root), Ok(cam)) = (root.single(), camera.single()) else { return };
    streamer.stats.frames += 1;
    let before = (streamer.stats.actors.equipment_spawns, streamer.stats.actors.record_spawns);
    let frame_start = std::time::Instant::now();
    // Under the start's screen the cells wait for their read-ahead on the read pool
    // ([`start_pool_prep`]), unless a travel has gone out under it ([`start_pool_cells`]).
    let pool_prep = start_pool_prep();
    let pool_start = loading.as_deref().is_some_and(|l| start_pool_cells(pool_prep, l.starting(), l.travelling()));
    let behind_screen = loading.is_some_and(|l| l.unseen_frames());
    let mut actors = actor_world.as_deref().map(|w| (w, &mut *actor_parts, sim.as_deref().map(|s| &s.world), &mut *ahead));
    // Before the start square is in, and behind a loading screen, the budgets are whole.
    let paced = streamer.settled && !behind_screen;
    despawn_dying_up_to(&mut commands, &mut streamer.dying, &children, if paced { pace.count(DESPAWN_PER_FRAME) } else { DESPAWN_PER_FRAME });
    if let Some(id) = streamer.interior {
        if streamer.loaded.is_empty() {
            let t0 = std::time::Instant::now();
            let s = &mut *streamer;
            let e = load_cell(&mut commands, &world, CellKey::Interior(id), &mut models, &mut terrain, &mut water, &mut trees, &mut grass, &mut emittance, &mut actors, s);
            commands.entity(root).add_child(e);
            streamer.loaded.insert((i32::MIN, i32::MIN), e);
            streamer.stats.cells_loaded += 1;
            streamer.stats.load_ms += t0.elapsed().as_secs_f64() * 1000.0;
            // The interior is its one cell: in once it is loaded (travellers in it wait
            // for this, nav.rs).
            streamer.settled = true;
        }
        crate::profile::actor_spawns(streamer.stats.actors.equipment_spawns - before.0, streamer.stats.actors.record_spawns - before.1);
        return;
    }
    let g = bevy_to_game(cam.translation());
    let centre = streamer.arrival.unwrap_or_else(|| cell_of(g.x, g.y));
    streamer.centre = Some(centre);
    let r = streamer.radius;
    let want: HashSet<(i32, i32)> =
        (-r..=r).flat_map(|dx| (-r..=r).map(move |dy| (centre.0 + dx, centre.1 + dy))).collect();
    let steps = |c: &(i32, i32)| (c.0 - centre.0).abs().max((c.1 - centre.1).abs());

    // Unload with one cell of hysteresis, fading out first (fade.rs). A cell still being
    // spawned is dropped at once: it has not been shown.
    let far: Vec<(i32, i32)> = streamer.loaded.keys().filter(|c| steps(c) > r + 1).copied().collect();
    let fading = streamer.fade_secs > 0.0;
    for c in far {
        if fading {
            streamer.fades.entry(c).or_insert(CellFade::new(1.0, true)).leaving = true;
        } else if let Some(e) = streamer.loaded.remove(&c) {
            retire(&mut commands, &mut streamer.dying, e);
        }
    }
    let gone: Vec<(i32, i32)> = streamer.spawning.keys().filter(|c| steps(c) > r + 1).copied().collect();
    for c in gone {
        if let Some(sp) = streamer.spawning.remove(&c) {
            retire(&mut commands, &mut streamer.dying, sp.entity);
        }
    }
    // Cells wanted again while fading out fade back in.
    for c in &want {
        if let Some(f) = streamer.fades.get_mut(c) {
            f.leaving = false;
        }
    }
    let rate = if fading { time.delta_secs() / streamer.fade_secs } else { 1.0 };
    let mut gone = Vec::new();
    for (c, f) in &mut streamer.fades {
        f.level = (f.level + if f.leaving { -rate } else { rate }).clamp(0.0, 1.0);
        if f.leaving && f.level <= 0.0 {
            gone.push(*c);
        }
    }
    for c in gone {
        streamer.fades.remove(&c);
        if let Some(e) = streamer.loaded.remove(&c) {
            retire(&mut commands, &mut streamer.dying, e);
        }
    }

    // Read ahead the cells up to one step beyond the loaded ones (worker threads).
    let now = time.elapsed_secs_f64();
    let tile = terrain.settings.tile;
    {
        let _span = info_span!("world::cell_read_ahead").entered();
        read_ahead(&world, &mut streamer, centre, r + 1, tile, &mut models);
    }

    // Before the start square is in (or behind a loading screen) cells are drawn whole in
    // the frame they load, as before; after it, over frames within a time budget, hidden
    // until all their references are in, then faded in (module docs of `CellSpawn`).
    let budgeted = streamer.settled && !behind_screen;
    let deadline = frame_start + if budgeted { pace.time(SPAWN_BUDGET) } else { LOADING_SCREEN_FRAME };
    // Cells under way first, nearest first.
    let mut order: Vec<(i32, i32)> = streamer.spawning.keys().copied().collect();
    order.sort_by_key(|c| steps(c) * 1000 + (c.0 - centre.0).abs() + (c.1 - centre.1).abs());
    for c in order {
        if budgeted && std::time::Instant::now() >= deadline {
            break;
        }
        let Some(mut sp) = streamer.spawning.remove(&c) else { continue };
        let t0 = std::time::Instant::now();
        let s = &mut *streamer;
        // What is left of its start first (terrain quadrants, grass), then its references.
        let started = sp.start(&mut commands, &world, &mut models, &mut terrain, &mut grass, budgeted, || budgeted && std::time::Instant::now() >= deadline);
        if started {
            let _span = info_span!("world::cell_refs").entered();
            let (e, origin) = (sp.entity, sp.origin);
            spawn_until(&sp.refs, &mut sp.next, budgeted.then_some(deadline), |r| {
                spawn_ref(&mut commands, &world, e, origin, r, &mut models, &mut water, &mut trees, &mut emittance, &mut actors, s)
            });
        }
        // All in but its grass, still on a read thread: the cell waits for it.
        let done = started && sp.next >= sp.refs.len() && sp.grown(&mut commands, &mut grass, &mut models);
        sp.ms += t0.elapsed().as_secs_f64() * 1000.0;
        sp.frames += 1;
        if !done {
            streamer.spawning.insert(c, sp);
        } else {
            finish_cell(&mut commands, &mut streamer, c, sp, fading);
        }
    }

    let mut missing: Vec<(i32, i32)> =
        want.iter().filter(|c| !streamer.loaded.contains_key(c) && !streamer.spawning.contains_key(c)).copied().collect();
    missing.sort_by_key(|c| steps(c) * 1000 + (c.0 - centre.0).abs() + (c.1 - centre.1).abs());
    for (started, c) in missing.into_iter().enumerate() {
        // Behind a loading screen: as many cells as fit in the frame's loading time.
        if !budgeted && started >= streamer.per_frame && !(behind_screen && frame_start.elapsed() < LOADING_SCREEN_FRAME) {
            break;
        }
        if budgeted && (started >= streamer.per_frame || std::time::Instant::now() >= deadline) {
            break;
        }
        // Drawn once its models are prepared (read ahead), or after waiting long enough.
        let ready = cell_ready(&world, &mut streamer.ahead, c, &mut models);
        let since = *streamer.ahead.waiting.entry(c).or_insert(now);
        let pool_wait = (pool_start && !budgeted).then(|| streamer.ahead.start.waited(frame_start));
        let forced = !ready && wait_over(budgeted, now - since, pool_wait);
        if !ready && !forced {
            // Nearer cells first: a farther one does not jump the queue.
            break;
        }
        if pool_wait.is_some() {
            streamer.ahead.start.drawn(ready);
        }
        streamer.ahead.waiting.remove(&c);
        streamer.stats.cells_waited += usize::from(now > since);
        streamer.stats.cells_forced += usize::from(forced && budgeted);
        let t0 = std::time::Instant::now();
        let prep = streamer.ahead.listed.remove(&c).and_then(|a| a.terrain);
        let s = &mut *streamer;
        let (e, refs, origin, start) = {
            let _span = info_span!("world::cell_start").entered();
            begin_cell(&mut commands, &world, CellKey::Exterior(c), prep, tile, &mut models, &mut terrain, &mut water, budgeted, s)
        };
        commands.entity(root).add_child(e);
        let mut sp = CellSpawn { entity: e, refs, next: 0, origin, ms: 0.0, frames: 1, start, grass: None };
        let started = sp.start(&mut commands, &world, &mut models, &mut terrain, &mut grass, budgeted, || budgeted && std::time::Instant::now() >= deadline);
        if started {
            let _span = info_span!("world::cell_refs").entered();
            spawn_until(&sp.refs, &mut sp.next, budgeted.then_some(deadline), |r| {
                spawn_ref(&mut commands, &world, e, origin, r, &mut models, &mut water, &mut trees, &mut emittance, &mut actors, s)
            });
        }
        let done = started && sp.next >= sp.refs.len() && sp.grown(&mut commands, &mut grass, &mut models);
        sp.ms = t0.elapsed().as_secs_f64() * 1000.0;
        if !done {
            streamer.spawning.insert(c, sp);
        } else {
            finish_cell(&mut commands, &mut streamer, c, sp, fading);
        }
    }
    if !streamer.settled && want.iter().all(|c| streamer.loaded.contains_key(c)) {
        streamer.settled = true;
        streamer.arrival = None;
        if let Some(line) = streamer.ahead.start.settled_line() {
            info!("{line}");
        }
    }
    if budgeted {
        pace.spent(frame_start.elapsed());
    }
    crate::profile::actor_spawns(streamer.stats.actors.equipment_spawns - before.0, streamer.stats.actors.record_spawns - before.1);
}

/// Spawn the references `refs[*next..]` one at a time with `spawn`, until all are in or,
/// with a `deadline`, the deadline has passed (checked before each one).
pub(crate) fn spawn_until(refs: &[u32], next: &mut usize, deadline: Option<std::time::Instant>, mut spawn: impl FnMut(u32)) {
    while *next < refs.len() && !deadline.is_some_and(|d| std::time::Instant::now() >= d) {
        let r = refs[*next];
        *next += 1;
        spawn(r);
    }
}

/// Main-thread time a frame spends spawning cells once the start square is in
/// [engineering choice: about a fifth of a 60 fps frame; a town cell's few thousand
/// references take a few frames, which the cell fade and the read-ahead ring cover].
/// Above 60 fps a frame gets the same share of itself (stream_pace.rs), as it does of
/// [`DESPAWN_PER_FRAME`] and [`TAKE_IN_BUDGET`].
const SPAWN_BUDGET: std::time::Duration = std::time::Duration::from_millis(3);

/// Seconds a cell waits for its read-ahead before it is drawn anyway, preparing what is
/// missing on the main thread [engineering choice].
const MAX_WAIT_SECS: f64 = 1.5;

/// Whether a cell whose models are not all prepared is drawn now, the rest prepared on the
/// main thread: in play once it has waited [`MAX_WAIT_SECS`] (`waited`, game seconds);
/// before the start square is in or behind a screen at once, except under the start's
/// screen ([`start_pool_prep`]: `pool_wait`, the real seconds the start's cells have
/// waited for the read pool so far), where the cells wait for the pool up to
/// [`MAX_WAIT_SECS`] too. The start's screen stays up until they are all drawn either way.
fn wait_over(budgeted: bool, waited: f64, pool_wait: Option<f64>) -> bool {
    if budgeted {
        waited >= MAX_WAIT_SECS
    } else {
        pool_wait.is_none_or(|w| w >= MAX_WAIT_SECS)
    }
}

/// Whether the cells asked for this frame are the start's, which wait for the read pool
/// ([`wait_over`]) and are counted in the start's line ([`StartPrep`]): with the pool
/// preparation `on` ([`start_pool_prep`]), under the start's screen, and not for a travel
/// that went out under it, whose cells are drawn at once as behind any travel's screen and
/// would otherwise share the start's `StartPrep::since` and counts.
fn start_pool_cells(on: bool, starting: bool, travelling: bool) -> bool {
    on && starting && !travelling
}

/// Direct children of retired cells despawned per frame (each with everything below it)
/// [engineering choice: a town cell's references go in a few frames].
const DESPAWN_PER_FRAME: usize = 400;

/// A cell whose references are being spawned over frames (after the start square is in:
/// [`stream`]). Its entity is hidden until the last reference is in; then it joins
/// `Streamer::loaded` and fades in as a cell loaded in one frame did, so nothing reads it
/// half drawn (the LOD stands in for it meanwhile).
#[derive(Debug)]
pub struct CellSpawn {
    /// The cell's entity (also read by terrain_lod.rs: a cell being spawned is alive).
    pub entity: Entity,
    refs: Vec<u32>,
    /// Index of the next reference to spawn.
    next: usize,
    /// The cell's origin (game space).
    origin: Vec3,
    /// Main-thread time spent on it so far, and the frames it took.
    ms: f64,
    frames: u32,
    /// What is left of its start (terrain, grass) before its references.
    start: Option<CellStart>,
    /// Its grass being grown on a read thread (grass.rs `GrassCtx::begin`); the cell is
    /// shown once it is in ([`CellSpawn::grown`]), so the grass appears with it.
    grass: Option<crate::grass::GrassGrowth>,
}

/// [design] What is left of a cell's start before its references are spawned
/// (docs/design/streaming-hitches.md §3.4). A cell's terrain and grass used to be spawned in
/// one piece when the cell was begun, whatever was left of the frame's spawn budget (up to
/// 4.6 ms against a budget of 1.5 ms at 120 fps [observed 2026-11-20]). They are steps now:
/// each of the terrain's four quadrants, then the grass, and [`stream`] stops between two of
/// them when its budget is spent. The cell is hidden until its last reference is in, so a
/// cell part-way through its start is not seen; a cell loaded whole (the start's square,
/// behind a loading screen) takes every step at once, as before.
enum CellStart {
    Terrain { spawn: Box<crate::terrain::TerrainSpawn>, land_id: u32, water: Option<f32> },
    Grass { land: Box<fnv_world::land::Land>, surface: Box<fnv_world::land_surface::Surface>, land_id: u32, grid: (i32, i32), water: Option<f32> },
}

impl std::fmt::Debug for CellStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Terrain { .. } => "CellStart::Terrain",
            Self::Grass { .. } => "CellStart::Grass",
        })
    }
}

impl CellStart {
    /// Take the next step; `None` when the start is done. With `growth` the grass is grown
    /// on a read thread and left there to be taken in ([`CellSpawn::grown`]); without, it
    /// is grown here.
    #[allow(clippy::too_many_arguments)]
    fn step(self, commands: &mut Commands, cell: Entity, world: &WorldData, models: &mut ModelCtx, terrain: &mut TerrainMaterials, grass: &mut crate::grass::GrassCtx, growth: Option<&mut Option<crate::grass::GrassGrowth>>) -> Option<Self> {
        match self {
            Self::Terrain { mut spawn, land_id, water } => {
                let _span = info_span!("world::cell_terrain").entered();
                let tm = &mut TerrainCtx { tm: terrain, assets: models };
                if !spawn.step(commands, cell, world, tm) {
                    return Some(Self::Terrain { spawn, land_id, water });
                }
                let grid = spawn.grid();
                let (land, surface) = spawn.finish(commands, cell, tm);
                Some(Self::Grass { land: Box::new(land), surface: Box::new(surface), land_id, grid, water })
            }
            Self::Grass { land, surface, land_id, grid, water } => {
                let _span = info_span!("world::cell_grass").entered();
                match growth {
                    Some(growth) => *growth = grass.begin(commands, cell, world, land_id, *land, *surface, grid, water, models),
                    None => grass.spawn(commands, cell, world, land_id, &land, &surface, grid, water, models),
                }
                None
            }
        }
    }
}

/// Whether a cell's start is taken in steps (`NVF_CELL_START_STEPS=0`: whole, in the frame
/// the cell is begun, as before; to compare).
fn start_in_steps() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("NVF_CELL_START_STEPS").is_ok_and(|v| v.trim() == "0"))
}

impl CellSpawn {
    /// The cell's start, as far as `stop` allows (asked before each step); `true` when it
    /// is done and the references can follow. `over_frames`: the cell is spawned over
    /// frames (hidden meanwhile), so its grass can be grown on a read thread; a cell
    /// loaded whole grows it here.
    #[allow(clippy::too_many_arguments)]
    fn start(&mut self, commands: &mut Commands, world: &WorldData, models: &mut ModelCtx, terrain: &mut TerrainMaterials, grass: &mut crate::grass::GrassCtx, over_frames: bool, stop: impl Fn() -> bool) -> bool {
        let steps = start_in_steps();
        let apart = over_frames && crate::stream_pace::on_read_threads();
        while let Some(start) = self.start.take() {
            if steps && stop() {
                self.start = Some(start);
                return false;
            }
            self.start = start.step(commands, self.entity, world, models, terrain, grass, apart.then_some(&mut self.grass));
        }
        true
    }

    /// Whether the cell's grass is in: taken in now if the read thread is done with it
    /// (`true` also when none is being grown).
    fn grown(&mut self, commands: &mut Commands, grass: &mut crate::grass::GrassCtx, models: &mut ModelCtx) -> bool {
        let Some(mut growth) = self.grass.take() else { return true };
        match growth.ready() {
            Some(grown) => {
                let _span = info_span!("world::cell_grass").entered();
                grass.finish(commands, self.entity, growth.grid, grown, models);
                true
            }
            None => {
                self.grass = Some(growth);
                false
            }
        }
    }
}

/// The cell `c` is all in: shown, loaded, faded in.
fn finish_cell(commands: &mut Commands, streamer: &mut Streamer, c: (i32, i32), sp: CellSpawn, fading: bool) {
    commands.entity(sp.entity).insert(Visibility::Inherited);
    streamer.loaded.insert(c, sp.entity);
    if fading {
        // Before the start square is in, cells appear at once; their meshes still need
        // their (fully drawn) ranges (fade.rs).
        let level = if streamer.settled { 0.0 } else { 1.0 };
        streamer.fades.insert(c, CellFade::new(level, false));
    }
    let s = &mut streamer.stats;
    s.cells_loaded += 1;
    s.load_ms += sp.ms;
    s.spawn_frames += sp.frames as usize;
}

/// Hide a cell that has gone and queue it for despawning in slices ([`despawn_dying`]). It
/// is no `RenderCell` from now on, so nvf-play no longer draws into it.
fn retire(commands: &mut Commands, dying: &mut Vec<Entity>, e: Entity) {
    commands.entity(e).insert(Visibility::Hidden).remove::<RenderCell>();
    dying.push(e);
}

/// Despawn up to [`DESPAWN_PER_FRAME`] children of retired cells, and the cells left
/// empty.
pub(crate) fn despawn_dying(commands: &mut Commands, dying: &mut Vec<Entity>, children: &Query<&Children>) {
    despawn_dying_up_to(commands, dying, children, DESPAWN_PER_FRAME);
}

/// [`despawn_dying`] with `per_frame` children at most (the loaded cells' count follows
/// the frame rate, stream_pace.rs).
fn despawn_dying_up_to(commands: &mut Commands, dying: &mut Vec<Entity>, children: &Query<&Children>, per_frame: usize) {
    let mut left = per_frame;
    dying.retain(|&cell| {
        if left == 0 {
            return true;
        }
        let kids = children.get(cell).map(|c| &c[..]).unwrap_or(&[]);
        let n = kids.len().min(left);
        for &k in &kids[..n] {
            commands.entity(k).try_despawn();
        }
        left -= n;
        if n == kids.len() {
            commands.entity(cell).try_despawn();
            return false;
        }
        true
    });
}

/// Despawn retired cells at once (behind a loading screen).
fn despawn_all_dying(commands: &mut Commands, dying: &mut Vec<Entity>) {
    for e in dying.drain(..) {
        commands.entity(e).try_despawn();
    }
}

/// What is read ahead for one exterior cell on a worker thread: the models its
/// references draw, and its landscape.
pub struct CellAhead {
    /// (model path, alternate textures) of the references drawn as static models.
    models: Vec<(String, Vec<AltTexture>)>,
    terrain: Option<TerrainPrep>,
    /// Its models were found all prepared.
    ready: bool,
}

/// The read-ahead of the cells around the loaded ones (docs/design/streaming-hitches.md).
#[derive(Default)]
pub struct ReadAhead {
    /// The worldspace the cells are of.
    world: u32,
    listing: HashMap<(i32, i32), Task<CellAhead>>,
    listed: HashMap<(i32, i32), CellAhead>,
    /// When each cell wanted for drawing was first found not ready (seconds).
    waiting: HashMap<(i32, i32), f64>,
    /// The start's cells read ahead on the read pool ([`start_pool_prep`]).
    start: StartPrep,
}

/// How the start's cells came in ([`start_pool_prep`]), for the line logged when they are
/// all drawn.
#[derive(Default)]
struct StartPrep {
    /// Cells read ahead under the picture hold ([`read_ahead_under_hold`]).
    held: usize,
    /// When the start's cells were first asked for, after the hold (real time).
    since: Option<std::time::Instant>,
    /// Cells drawn with every model prepared, and drawn before (the rest prepared on the
    /// main thread).
    ready: usize,
    forced: usize,
    /// The real seconds from `since` to the last cell drawn.
    waited: f64,
    /// The line has been logged.
    logged: bool,
}

impl StartPrep {
    /// The real seconds the start's cells have waited for the read pool by `now`.
    fn waited(&mut self, now: std::time::Instant) -> f64 {
        now.saturating_duration_since(*self.since.get_or_insert(now)).as_secs_f64()
    }

    /// One of the start's cells is drawn, its models all `ready` or not.
    fn drawn(&mut self, ready: bool) {
        if ready {
            self.ready += 1;
        } else {
            self.forced += 1;
        }
        if let Some(since) = self.since {
            self.waited = since.elapsed().as_secs_f64();
        }
    }

    /// The line logged once the start's cells are all drawn, the first time only.
    fn settled_line(&mut self) -> Option<String> {
        if self.logged || self.since.is_none() {
            return None;
        }
        self.logged = true;
        Some(format!(
            "world: the start's cells drawn: {} with their models prepared on the read pool, {} before (the rest prepared on the main thread); {} cells read ahead under the picture hold, {:.2} s from the first cell to the last",
            self.ready, self.forced, self.held, self.waited
        ))
    }
}

impl ReadAhead {
    /// Cells still being read on worker threads.
    pub(crate) fn busy(&self) -> bool {
        !self.listing.is_empty()
    }

    /// The landscape read ahead for cell `c`, taken (the cell's reading is done with).
    pub(crate) fn take_terrain(&mut self, c: (i32, i32)) -> Option<TerrainPrep> {
        self.waiting.remove(&c);
        self.listed.remove(&c).and_then(|a| a.terrain)
    }
}

/// Which references' models a cell's read-ahead prepares: the load order, the placement
/// and its base (enabled, not an imposter or primitive, [`cell_models`]).
pub(crate) type ModelFilter = fn(&LoadOrder, &Placement, &BaseObject) -> bool;

/// The models the loaded cells draw as static models ([`spawn_model`]).
fn loaded_models(_: &LoadOrder, _: &Placement, base: &BaseObject) -> bool {
    DRAWN.contains(&&base.sig) && !base.is_speedtree()
}

/// Start reading ahead the cells within `ring` steps of `centre` that are not drawn or
/// read yet; take in the readings done and send their models to be prepared; forget
/// cells far beyond the ring.
fn read_ahead(world: &WorldData, streamer: &mut Streamer, centre: (i32, i32), ring: i32, tile: f32, models: &mut ModelCtx) {
    let Streamer { ahead, loaded, spawning, hide, .. } = streamer;
    let area = AheadArea { centre, inner: 0, ring, start: true, deadline: None };
    read_ahead_cells(world, ahead, area, |c| loaded.contains_key(c) || spawning.contains_key(c), hide, loaded_models, tile, models);
}

/// The cells a read-ahead covers: those `inner` to `ring` steps from `centre`; `start`:
/// new readings may begin (otherwise only finished ones are taken in); `deadline`: no new
/// reading begins and no finished one is looked at past it (the rest wait for a later
/// frame).
#[derive(Clone, Copy, Debug)]
pub(crate) struct AheadArea {
    pub centre: (i32, i32),
    pub inner: i32,
    pub ring: i32,
    pub start: bool,
    pub deadline: Option<std::time::Instant>,
}

/// [`read_ahead`] over any [`ReadAhead`]: start reading the cells of `area` that `skip`
/// does not leave out and that are not read or being read, their references' models as
/// `filter` chooses them; take in the readings done and send their models to be
/// prepared; forget cells far beyond the ring.
#[allow(clippy::too_many_arguments)]
pub(crate) fn read_ahead_cells(world: &WorldData, ahead: &mut ReadAhead, area: AheadArea, skip: impl Fn(&(i32, i32)) -> bool, hide: &[String], filter: ModelFilter, tile: f32, models: &mut ModelCtx) {
    let AheadArea { centre, inner, ring, start, deadline } = area;
    let late = || deadline.is_some_and(|d| std::time::Instant::now() >= d);
    if ahead.world != world.world {
        *ahead = ReadAhead { world: world.world, ..default() };
    }
    let steps = |c: &(i32, i32)| (c.0 - centre.0).abs().max((c.1 - centre.1).abs());
    ahead.listing.retain(|c, _| steps(c) <= ring + 1);
    // [design] #3082: what a cell left behind was read ahead for (its landscape, the
    // smoothed surface and every quadrant's meshes, all CPU data, no asset handles) is
    // freed on a worker thread. A crossing leaves a row of the outer ring's cells, mostly
    // never loaded; freeing them here took 9-16 ms of the frame [observed 2026-11-19,
    // `profile hitch` spans, stress tour Goodsprings-Primm at --step 2048].
    let far: Vec<(i32, i32)> = ahead.listed.keys().filter(|c| steps(c) > ring + 1).copied().collect();
    let left: Vec<CellAhead> = far.iter().filter_map(|c| ahead.listed.remove(c)).collect();
    if !left.is_empty() {
        nvf_assets::read_pool::read_pool().spawn(async move { drop(left) }).detach();
    }
    ahead.waiting.retain(|c, _| steps(c) <= ring);
    for dx in (-ring..=ring).filter(|_| start) {
        for dy in -ring..=ring {
            let c = (centre.0 + dx, centre.1 + dy);
            if steps(&c) < inner || skip(&c) || ahead.listing.contains_key(&c) || ahead.listed.contains_key(&c) || late() {
                continue;
            }
            let Some(cell) = world.grid_cell(c) else { continue };
            let mut refs = cell.refs.clone();
            refs.extend(world.persistent_by_grid.get(&c).into_iter().flatten());
            let land = cell.land;
            let around = world.land_ids(c);
            let (spec, draw) = (nvf_play::smooth_terrain::spec(), crate::terrain_lod::first_draw());
            let (lo, schema, index, hide) = (world.lo, world.schema, world.index, hide.to_vec());
            let task = nvf_assets::read_pool::read_pool().spawn(async move {
                CellAhead {
                    models: cell_models(lo, schema, index, &refs, &hide, filter),
                    terrain: land.and_then(|l| prepare_terrain(lo, schema, l, c, tile, around, spec, draw)),
                    ready: false,
                }
            });
            ahead.listing.insert(c, task);
        }
    }
    let mut done = Vec::new();
    ahead.listing.retain(|c, task| match (!late()).then(|| check_ready(task)).flatten() {
        Some(a) => {
            done.push((*c, a));
            false
        }
        None => true,
    });
    for (c, mut a) in done {
        // Past the deadline its models are asked for when the cell is (`cell_ready`).
        if !late() {
            a.ready = models.models_ready(a.models.iter().map(|(p, s)| (p.as_str(), s.as_slice())));
        }
        ahead.listed.insert(c, a);
    }
}

/// Whether the cell `c`'s models are all prepared, so it can be drawn without reading a
/// model file on this thread. A grid square without a cell has nothing to read.
pub(crate) fn cell_ready(world: &WorldData, ahead: &mut ReadAhead, c: (i32, i32), models: &mut ModelCtx) -> bool {
    match ahead.listed.get_mut(&c) {
        Some(a) => {
            // Asked again: a model read ahead long ago may have been released since.
            a.ready = models.models_ready(a.models.iter().map(|(p, s)| (p.as_str(), s.as_slice())));
            a.ready
        }
        None => !ahead.listing.contains_key(&c) && world.grid_cell(c).is_none(),
    }
}

/// The distinct models (path, alternate textures) the references `refs` draw as static
/// models (`spawn_model`) as `filter` chooses them: what a cell's read-ahead prepares.
/// Runs on a worker thread.
fn cell_models(lo: &LoadOrder, schema: &EsmSchema, index: &WorldIndex, refs: &[u32], hide: &[String], filter: ModelFilter) -> Vec<(String, Vec<AltTexture>)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for &r in refs {
        let Some(p) = index.placement(lo, schema, r) else { continue };
        if !drawn_with_cell(&p) || !enabled(lo, schema, &p, 0) {
            continue;
        }
        let Some(base) = p.base.and_then(|b| base_object(lo, schema, b)) else { continue };
        if !filter(lo, &p, &base) {
            continue;
        }
        let Some(path) = base.world_model_path() else { continue };
        if is_drawn_marker(base.form_id, &path) || hide.iter().any(|h| path.contains(h.as_str())) {
            continue;
        }
        if seen.insert((path.clone(), base.world_alt_textures.clone())) {
            out.push((path, base.world_alt_textures));
        }
    }
    out
}

/// Main-thread time a frame spends putting models prepared on worker threads into the
/// asset stores, once the start square is in [engineering choice].
const TAKE_IN_BUDGET: std::time::Duration = std::time::Duration::from_millis(2);

/// Bytes of meshes and textures sent to the GPU a frame while playing (Bevy's
/// `RenderAssetBytesPerFrame`; behind a loading screen, no limit) [engineering choice: a
/// cell's new textures and meshes reach the GPU over a few frames instead of one; Bevy
/// sends whole assets and stops after the first one over the limit].
const UPLOAD_BYTES_PER_FRAME: usize = 24 << 20;

/// The upload limit while playing: [`UPLOAD_BYTES_PER_FRAME`], or the test aid
/// `NVF_PLAY_TEST_UPLOAD_BYTES=<bytes>` (a small limit makes every model wait for its
/// meshes as large mod textures do, [`respecialize_late_meshes`]).
fn upload_bytes_per_frame() -> usize {
    static BYTES: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *BYTES.get_or_init(|| match std::env::var("NVF_PLAY_TEST_UPLOAD_BYTES").map(|v| v.trim().parse::<usize>()) {
        Ok(Ok(n)) => {
            info!("test aid NVF_PLAY_TEST_UPLOAD_BYTES: {n} bytes sent to the GPU a frame while playing");
            n
        }
        Ok(Err(e)) => {
            warn!("NVF_PLAY_TEST_UPLOAD_BYTES: {e}");
            UPLOAD_BYTES_PER_FRAME
        }
        Err(_) => UPLOAD_BYTES_PER_FRAME,
    })
}

/// Take in what worker threads prepared for the cells read ahead
/// (`ModelCtx::take_fetched`), and set the GPU upload limit: both without limit behind a
/// loading screen and before the start square is in.
pub fn take_cell_work(
    mut models: ModelCtx,
    time: Res<Time<Real>>,
    streamer: Res<Streamer>,
    loading: Option<Res<nvf_play::loading::Loading>>,
    bytes: Option<ResMut<bevy::render::render_asset::RenderAssetBytesPerFrame>>,
    pace: Res<crate::stream_pace::StreamPace>,
) {
    let free = !streamer.settled || loading.is_some_and(|l| l.unseen_frames());
    let started = std::time::Instant::now();
    models.take_fetched(time.elapsed_secs_f64(), (!free).then(|| pace.time(TAKE_IN_BUDGET)));
    if !free {
        pace.spent(started.elapsed());
    }
    let limit = (!free).then(upload_bytes_per_frame);
    if let Some(mut b) = bytes.filter(|b| b.max_bytes != limit) {
        b.max_bytes = limit;
    }
}

/// Frames after an entity is given its mesh under the upload limit at which its `Mesh3d`
/// is marked changed again ([`respecialize_late_meshes`]): about 2 s at 60 frames a
/// second [engineering choice: the limit holds back at most a few frames' worth of
/// assets, and each mark costs the entity one more pipeline lookup].
const LATE_MESH_FRAMES: [u32; 7] = [2, 4, 8, 16, 32, 64, 128];

/// Entities given a mesh while the upload limit was on, with the frame they got it.
#[derive(Resource, Default)]
pub struct LateMeshes {
    frame: u32,
    waiting: Vec<(Entity, u32)>,
    /// Marks made, for the exit summary.
    marked: u64,
}

/// Mark the meshes of new entities changed again over the next frames while the upload
/// limit is on. Bevy 0.19 never draws an entity whose mesh was not on the GPU yet when it
/// first chose the entity's pipeline [design: bevy_pbr 0.19.1 `specialize_material_meshes`
/// skips such an entity without putting it in its pending queue, which it does for a
/// material not ready; only a change to `Mesh3d` or `MeshMaterial3d` or their assets
/// tries it again]. Under the limit a mesh can wait behind large textures: with 4096x4096
/// mod textures (VRP 4X) the Vigor Tester's menu model lost all 275 meshes that way and
/// showed only its glows (owner report 2026-10-26). A mark makes Bevy try again.
#[allow(clippy::type_complexity)]
pub fn respecialize_late_meshes(
    mut late: ResMut<LateMeshes>,
    bytes: Option<Res<bevy::render::render_asset::RenderAssetBytesPerFrame>>,
    mut meshes: ParamSet<(Query<Entity, Added<Mesh3d>>, Query<&mut Mesh3d>)>,
) {
    let late = &mut *late;
    late.frame = late.frame.wrapping_add(1);
    if bytes.is_some_and(|b| b.max_bytes.is_some()) {
        let frame = late.frame;
        late.waiting.extend(meshes.p0().iter().map(|e| (e, frame)));
    }
    let mut meshes = meshes.p1();
    let now = late.frame;
    let mut marked = 0;
    late.waiting.retain(|&(e, at)| {
        let age = now.wrapping_sub(at);
        if LATE_MESH_FRAMES.contains(&age) {
            let Ok(mut m) = meshes.get_mut(e) else { return false };
            m.set_changed();
            marked += 1;
        }
        age < LATE_MESH_FRAMES[LATE_MESH_FRAMES.len() - 1]
    });
    late.marked += marked;
}

/// Log what streaming did at the exit (stress runs read it).
pub fn report(mut exit: MessageReader<AppExit>, streamer: Res<Streamer>, models: Res<nvf_assets::ModelCache>, late: Res<LateMeshes>, terrain: Option<Res<crate::terrain::TerrainCache>>) {
    if exit.read().last().is_none() {
        return;
    }
    let s = &streamer.stats;
    info!(
        "streaming summary: {} cells loaded ({:.0} ms on the main thread), spread over {} frames; {} waited for their read-ahead, {} drawn before it was done; {} models cached, {} in flight; {} late-mesh marks",
        s.cells_loaded, s.load_ms, s.spawn_frames, s.cells_waited, s.cells_forced, models.held(), models.in_flight(), late.marked
    );
    info!("{}", nvf_assets::tangents::summary());
    // Shapes that asked to be blended and are drawn opaque (#3230).
    info!("{}", nvf_assets::nif_spawn::blend_opaque_summary());
    // Unlit shapes without a NiAlphaProperty that are blended all the same (#3443).
    info!("{}", nvf_assets::nif_spawn::vertex_alpha_blended_summary());
    // What the two readers of each model file cost (#3149).
    info!("{}", nvf_assets::load_times::summary());
    // The smoke scenario `terrain-smoothing` reads it (#2149).
    if let Some(t) = terrain {
        // Cell draws counted over the run (a cell drawn again after it unloaded counts
        // again), not the cells loaded now.
        // Under 4 x 4 near, after the recipe: its redraws (terrain_lod.rs, #2676).
        let levels = t.lod.as_ref().map_or(String::new(), |l| format!("; {}", l.summary()));
        info!("terrain summary: {} cells drawn smoothed, {} on the stored triangles ({}){levels}", t.smoothed_cells, t.stored_cells, nvf_play::smooth_terrain::spec().describe());
    }
    if s.billboards > 0 {
        info!("world: {} placed billboard nodes turn to the camera ({} references)", s.billboards, s.refs_billboards);
    }
}

/// Behind a loading screen (`nvf_play::loading`) cells are loaded one after another for
/// this long each frame instead of `Streamer::per_frame` at a time [engineering choice:
/// the window still answers, and fewer frames are drawn while the screen is up].
const LOADING_SCREEN_FRAME: std::time::Duration = std::time::Duration::from_millis(200);

/// Whether cells and actors may start loading: not yet under the start's screen until it
/// has been drawn (`nvf_play::loading::Loading::world_held`). Before its first frame the
/// screen is only asked for.
pub fn world_not_held(loading: Option<Res<nvf_play::loading::Loading>>, cover: Option<Res<nvf_play::loading::StartCover>>) -> bool {
    cover.is_none() && !loading.is_some_and(|l| l.world_held())
}

/// Whether the start's models are prepared on the read pool (docs/design/loading-screen.md
/// §1a): the cells around the camera are read ahead under the picture hold
/// ([`read_ahead_under_hold`]) and, under the start's screen, wait for that read-ahead
/// ([`wait_over`]) instead of having their models prepared on the main thread in the first
/// frame after the hold. Test aid `NVF_START_POOL_PREP=0`: as before, nothing read under
/// the hold and the start's cells drawn at once.
pub fn start_pool_prep() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var("NVF_START_POOL_PREP").ok().as_deref().map(str::trim) != Some("0");
        if !on {
            info!("test aid NVF_START_POOL_PREP=0: the start's cells are read ahead after the picture hold and drawn at once, their models prepared on the main thread");
        }
        on
    })
}

/// Under the start's picture hold ([`world_not_held`], so [`stream`] does not run yet),
/// read ahead the cells around the camera as [`stream`] would in its first frame: their
/// lists, landscape and models are prepared on the read pool while the screen's picture
/// comes up, and [`take_cell_work`] takes the models in, so the cells find them ready
/// once the hold is let go [design] ([`start_pool_prep`]; not for an interior, which is
/// one cell drawn whole).
pub fn read_ahead_under_hold(
    world: Res<WorldData>,
    mut streamer: ResMut<Streamer>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    mut models: ModelCtx,
    terrain: Option<Res<crate::terrain::TerrainSettings>>,
    loading: Option<Res<nvf_play::loading::Loading>>,
) {
    let held = loading.is_some_and(|l| l.starting() && l.world_held());
    let (Some(terrain), Ok(cam)) = (terrain, camera.single()) else { return };
    if !held || streamer.interior.is_some() || !start_pool_prep() {
        return;
    }
    let g = bevy_to_game(cam.translation());
    let centre = streamer.arrival.unwrap_or_else(|| cell_of(g.x, g.y));
    let ring = streamer.radius + 1;
    read_ahead(&world, &mut streamer, centre, ring, terrain.tile, &mut models);
    let ahead = &mut streamer.ahead;
    let cells = ahead.listing.len() + ahead.listed.len();
    if ahead.start.held == 0 && cells > 0 {
        info!("world: the start's read-ahead begins under the loading screen's picture hold: {cells} cells around ({}, {}) read on the read pool", centre.0, centre.1);
    }
    ahead.start.held = ahead.start.held.max(cells);
}

/// Tell the loading screen whether cells around the camera are still to be drawn (runs
/// after [`stream`]; `travel` has run before it, so a travel sent this frame counts).
pub fn loading_gate(
    streamer: Res<Streamer>,
    mut loading: ResMut<nvf_play::loading::Loading>,
    gate: Res<crate::pipeline_gate::PipelineGate>,
) {
    let busy = !streamer.settled;
    if loading.render_busy != busy {
        loading.render_busy = busy;
    }
    // Pipelines not ready: what they would draw is missing from the view, which keeps the
    // clear colour (`pipeline_gate`). Before the first render frame nothing has asked yet.
    // The start's screen waits for all of them, a travel under it included.
    let busy_now = if loading.travelling() && !loading.starting() { gate.busy_for_travel() } else { gate.busy() };
    let pipelines = gate.frames() == 0 || busy_now > 0;
    if loading.pipelines_busy != pipelines {
        loading.pipelines_busy = pipelines;
    }
    if std::mem::take(&mut loading.pipeline_cap_note) {
        gate.list_busy();
    }
    loading.release_picture_hold(gate.frames() >= nvf_play::loading::PICTURE_FRAMES && gate.screen_busy() == 0, std::time::Instant::now());
    // Actors around the player not drawn yet: a loading screen waits for them too, so
    // they are not seen appearing after it comes down (`readahead::actors_waiting`).
    let actors = crate::readahead::actors_waiting() > 0;
    if loading.actors_busy != actors {
        loading.actors_busy = actors;
    }
}

/// Turn every tree billboard about its vertical (game Z) axis so that its face (local
/// +X) points at the camera. Billboards are children of cells, which only translate, and
/// the cells of the world root, so a yaw about game Z in the billboard's own transform
/// is a yaw about the world's up axis.
pub fn face_billboards(
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    mut trees: Query<(&mut Transform, &GlobalTransform), With<TreeBillboard>>,
) {
    let Ok(cam) = camera.single() else { return };
    let eye = bevy_to_game(cam.translation());
    for (t, g) in &mut trees {
        let d = eye - bevy_to_game(g.translation());
        if d.x * d.x + d.y * d.y > 1e-6 {
            // Written only when it differs: a write marks the transform changed, which
            // dirties the billboard's cell for propagation and sends it over the cell's
            // children [design: bevy_transform's `mark_dirty_trees` and
            // `propagate_parent_transforms`]. An exact compare: the same eye and place give
            // the same bits, any turn differs.
            t.map_unchanged(|t| &mut t.rotation).set_if_neq(Quat::from_rotation_z(d.y.atan2(d.x)));
        }
    }
}

/// Turn every `NiBillboardNode` drawn as an entity ([`Billboard`]: placed models' glow
/// cards, whether their node hierarchy is spawned or their shapes baked, and the effect
/// models of `shot_fx`) to the camera, each frame before transforms propagate
/// (docs/spec-notes/billboard-nodes.md): about the node's own y axis for the "rotate
/// about up" modes (1, 9: about the node's y; 5, BSROTATE_ABOUT_UP: about its z, +y
/// shown), its +z straight at the camera for the others [documented: nif.xml
/// BillboardMode names; hypothesis: the axes, docs/spec-notes/billboard-nodes.md §1].
pub fn face_camera(
    camera: Query<Entity, crate::fly::WorldCamera>,
    boards: Query<(Entity, &nvf_assets::model::Billboard, &ChildOf)>,
    parents: Query<&ChildOf>,
    mut transforms: Query<&mut Transform>,
) {
    let Ok(cam) = camera.single() else { return };
    // An entity's place in the world from this frame's transforms: the propagated ones
    // are a frame old (the camera has moved since) and missing for what was spawned
    // this frame.
    let world = |transforms: &Query<&mut Transform>, e: Entity| {
        let mut m = Affine3A::IDENTITY;
        let mut at = Some(e);
        while let Some(p) = at {
            if let Ok(t) = transforms.get(p) {
                m = t.compute_affine() * m;
            }
            at = parents.get(p).ok().map(ChildOf::parent);
        }
        m
    };
    let eye = Vec3::from(world(&transforms, cam).translation);
    for (e, b, parent) in &boards {
        let m = world(&transforms, parent.parent());
        let Ok(mut t) = transforms.get_mut(e) else { continue };
        // The way to the camera in the node's own frame as the file has it.
        let d = b.bind.inverse() * (m.inverse().transform_point3(eye) - t.translation);
        let turn = match b.mode {
            1 | 9 => Quat::from_rotation_y(d.x.atan2(d.z)),
            // About the node's z, its +y towards the camera (#2549: the torch's flame
            // stands along +z under a mode-5 node).
            5 => Quat::from_rotation_z((-d.x).atan2(d.y)),
            _ => Quat::from_rotation_arc(Vec3::Z, d.normalize_or(Vec3::Z)),
        };
        let rotation = b.bind * turn;
        if t.rotation != rotation {
            t.rotation = rotation;
        }
    }
}

/// What [`note_other_cameras`] remembers between frames.
#[derive(Default)]
pub struct OtherCameras {
    /// `StreamStats::frames` the frame before.
    last: u64,
    /// The other camera's name while one is active, once streaming has run beside it.
    open: Option<String>,
    /// Frames streaming ran while that camera was active.
    frames: u64,
}

impl OtherCameras {
    /// One frame: `frames` the streaming counter now, `other` the name of an active 3D
    /// camera beside the main view's (a menu's or a scope's), if any. Returns the line to
    /// log: when streaming first runs beside such a camera, and when the camera is gone.
    pub fn step(&mut self, frames: u64, other: Option<&str>) -> Option<String> {
        let ran = frames != std::mem::replace(&mut self.last, frames);
        match other {
            // Another camera took over with no frame between: close the first one's count.
            Some(name) if ran && self.open.as_deref().is_some_and(|o| o != name) => {
                let line = self.open.replace(name.to_string()).map(|prev| format!("world: streaming kept running behind another 3D camera ({prev}): {} frames", self.frames));
                self.frames = 1;
                line
            }
            Some(name) if ran => {
                self.frames += 1;
                if self.open.is_none() {
                    self.open = Some(name.to_string());
                    return Some(format!("world: streaming runs with another 3D camera active ({name}), the main view's camera found by its marker"));
                }
                None
            }
            Some(_) => None,
            None => self.open.take().map(|name| format!("world: streaming kept running behind another 3D camera ({name}): {} frames", std::mem::take(&mut self.frames))),
        }
    }
}

/// The 3D cameras beside the main view's (a menu's, a scope's), the water reflection's
/// left out, with their names.
type OtherCameraQuery<'w, 's> =
    Query<'w, 's, (&'static Camera, Option<&'static Name>), (With<Camera3d>, Without<crate::fly::FlyCam>, Without<crate::reflection::ReflectionCamera>)>;

/// Log that streaming keeps running while a menu's or a scope's own 3D camera is active
/// beside the main view's: once when it first runs beside one, and for how many frames
/// when the camera is gone. Before, every system that took "the one `Camera3d`" stopped
/// while such a camera existed [design: issue #1753, `fly::WorldCamera`]. The water
/// reflection's camera does not count.
pub fn note_other_cameras(
    streamer: Res<Streamer>,
    others: OtherCameraQuery,
    mut state: Local<OtherCameras>,
) {
    let other = others.iter().find(|(c, _)| c.is_active).map(|(_, n)| n.map_or("unnamed", Name::as_str));
    if let Some(line) = state.step(streamer.stats.frames, other) {
        info!("{line}");
    }
}

/// `nvf_play::Travel` (door teleports): drop the loaded cells, switch between the
/// worldspace and an interior; streaming reloads around the camera, which the player
/// systems have moved to the destination. With `--play` this happens behind a loading
/// screen, which stays until `Streamer::settled` ([`loading_gate`]).
#[allow(clippy::too_many_arguments)]
pub fn travel(
    mut commands: Commands,
    mut msgs: MessageReader<nvf_play::Travel>,
    world: Res<WorldData>,
    mut streamer: ResMut<Streamer>,
    mut sky: ResMut<crate::sky::SkyState>,
    mut lod: ResMut<crate::lod::LodState>,
    args: Res<crate::Args>,
    script: Option<Res<nvf_play::script::ScriptState>>,
) {
    let Some(t) = msgs.read().last().copied() else { return };
    streamer.travels += 1;
    for (_, e) in streamer.loaded.drain() {
        commands.entity(e).despawn();
    }
    for (_, sp) in streamer.spawning.drain() {
        commands.entity(sp.entity).despawn();
    }
    despawn_all_dying(&mut commands, &mut streamer.dying);
    // The destination appears at once, as after a loading screen.
    streamer.fades.clear();
    streamer.settled = false;
    streamer.interior = t.interior;
    streamer.arrival = t.interior.is_none().then(|| cell_of(t.feet[0], t.feet[1]));
    let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    sky.set_interior(t.interior.and_then(|id| effective_cell_lighting(&world, game, id)));
    sky.interior_grade = t.interior.and_then(|id| crate::sky::SkyState::cell_grade(world.lo, world.schema, id));
    lod.set_enabled(&mut commands, t.interior.is_none() && !args.no_lod);
}

/// The actual cell-lighting reader shared by startup/travel and authored
/// headless camera controls. No ScriptState testing constructor is needed.
pub(crate) fn effective_cell_lighting(world: &WorldData, game: Option<&fnv_game::Game<'_>>, cell: u32) -> Option<fnv_world::lighting::CellLighting> {
    match game {
        Some(game) => game.session_cell_lighting(world.lo, world.schema, fnv_sim::FormId(cell)),
        None => fnv_world::lighting::cell_lighting(world.lo, world.schema, cell),
    }
}

type CellLightingKey = (Option<u32>, Option<(u64, u64)>);

/// Reapply interior cell lighting on travel, runtime availability/replacement changes,
/// or live session lighting edits (`Game::lighting_token` bumped by
/// `SetCellLightingTemplate`, `SetCellFlag` 8..=16, `SetLightingTemplateTraitNumeric`,
/// and `SetInteriorLightingTraitNumeric`). Unchanged frames neither resolve fields nor
/// allocate; edits to unrelated cells/templates leave `SkyState` untouched.
pub fn cell_lighting(
    mut travels: MessageReader<nvf_play::Travel>,
    streamer: Res<Streamer>,
    world: Res<WorldData>,
    script: Option<Res<nvf_play::script::ScriptState>>,
    mut sky: ResMut<crate::sky::SkyState>,
    mut seen: Local<Option<CellLightingKey>>,
) {
    let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    let key = (streamer.interior, game.map(fnv_game::Game::lighting_token));
    let travelled = travels.read().count() > 0;
    if *seen == Some(key) && !travelled {
        return;
    }
    *seen = Some(key);
    let light = streamer.interior.and_then(|cell| effective_cell_lighting(&world, game, cell));
    if sky.interior != light {
        sky.set_interior(light);
    }
}

/// Resolve the active exterior worldspace's base image-space FormID (`WRLD INAM`):
/// live session `SetWorldspaceImagespace` override (following `WNAM` + `PNAM` Use Image
/// Space Data inheritance in `fnv_game`) first, else authored `WRLD INAM` / parent's.
pub(crate) fn effective_worldspace_image_space_id(
    world: &WorldData,
    game: Option<&fnv_game::Game<'_>>,
) -> Option<u32> {
    game.and_then(|g| g.worldspace_image_space_override(fnv_sim::FormId(world.world)))
        .map(|f| f.0)
        .or_else(|| crate::sky::worldspace_image_space_id(world))
}

/// Resolve an IMGS record with any live `SetImageSpaceTrait` session overrides from `game`.
pub(crate) fn effective_image_space(
    world: &WorldData,
    game: Option<&fnv_game::Game<'_>>,
    space: u32,
) -> Option<fnv_world::imagespace::Cinematic> {
    match game {
        Some(g) => fnv_world::imagespace::image_space_with_traits(
            world.lo,
            world.schema,
            space,
            g.image_space_trait_overrides(fnv_sim::FormId(space)),
        ),
        None => fnv_world::imagespace::image_space(world.lo, world.schema, space),
    }
}

/// Cell-load identity for [`CellImageSpaceState`]: `(interior_cell, exterior_cell,
/// worldspace, game_instance_id)`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CellImageSpaceLoadKey {
    interior: Option<u32>,
    exterior: Option<u32>,
    world: u32,
    instance_id: Option<u64>,
}

/// Cached cell-load and live trait-revision state for [`cell_image_space`] so unchanged
/// frames do zero work and a pending `SetCellImageSpace` waits until the next cell load.
#[derive(Default)]
pub struct CellImageSpaceState {
    /// Last cell/worldspace/game-instance identity at which the active cell's image space
    /// FormID was bound.
    load_key: Option<CellImageSpaceLoadKey>,
    /// The `IMGS` FormID bound when the current interior cell loaded (from
    /// `SetCellImageSpace` at load time, else authored `CELL XCIM`). Held fixed until the
    /// cell next loads so a `SetCellImageSpace` call on the active cell does not switch
    /// early when `SetImageSpaceTrait` refreshes traits.
    active_interior_space: Option<u32>,
    /// Explicit `SetCellImageSpace` FormID bound when the current exterior cell loaded
    /// (`None` when no script set one, leaving vanilla exterior cells on worldspace `INAM`).
    active_exterior_cell_space: Option<u32>,
    /// Last `SetImageSpaceTrait` / `SetWorldspaceImagespace` revision applied to the active
    /// image space.
    trait_rev: Option<u64>,
}

/// SetCellImageSpace (`fnv_game::game::FormChanges::cell_image_spaces`), live
/// `SetWorldspaceImagespace` (`WRLD INAM/`), and live `SetImageSpaceTrait`
/// (`fnv_game::nvse_traits::TraitState`):
/// - A cell's `SetCellImageSpace` assignment is bound when the cell loads (on travel,
///   cell entry, or when the script runtime with a loaded save's changes becomes ready):
///   the original shows a `SetCellImageSpace` change to the cell the player is already
///   in only when the cell next loads [documented: GECK wiki SetCellImageSpace, notes].
///   Outdoors, only explicit `SetCellImageSpace` assignments override the worldspace
///   image space, leaving unmodded exterior cells unchanged.
/// - Session `SetWorldspaceImagespace` edits (with `WNAM` + `PNAM` inheritance) and
///   `SetImageSpaceTrait` edits (`0..=32`) on the active image space refresh immediately
///   when `Game::image_space_sync_token()` advances [hypothesis: docs/spec-notes/world.md §6].
/// - Does zero work on unchanged frames and resets cleanly on game-instance switches.
///
/// Runs after [`travel`].
pub fn cell_image_space(
    mut travels: MessageReader<nvf_play::Travel>,
    streamer: Res<Streamer>,
    world: Res<WorldData>,
    script: Option<Res<nvf_play::script::ScriptState>>,
    mut sky: ResMut<crate::sky::SkyState>,
    mut state: Local<CellImageSpaceState>,
) {
    let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    let token = game.map(|g| g.image_space_sync_token());
    let instance_id = token.map(|(id, _)| id);
    let trait_rev = token.map(|(_, rev)| rev);
    let ext_cell = if streamer.interior.is_none() {
        streamer
            .centre
            .and_then(|g| world.grid_cell(g))
            .map(|c| c.form_id)
            .or_else(|| {
                game.and_then(|g| {
                    g.cell_of(fnv_sim::FormId::PLAYER_REF)
                        .filter(|&c| g.world.placed.world_of(c) == Some(fnv_sim::FormId(world.world)))
                        .map(|c| c.0)
                })
            })
    } else {
        None
    };
    let load_key = CellImageSpaceLoadKey {
        interior: streamer.interior,
        exterior: ext_cell,
        world: world.world,
        instance_id,
    };
    let travelled = travels.read().count() > 0;
    let rebind = state.load_key != Some(load_key) || travelled;
    if !rebind && state.trait_rev == trait_rev {
        return;
    }
    state.load_key = Some(load_key);
    state.trait_rev = trait_rev;
    if let Some(cell) = streamer.interior {
        state.active_exterior_cell_space = None;
        let mut scripted = false;
        let space = if rebind {
            let assigned = game.and_then(|g| g.forms.cell_image_spaces.get(&fnv_sim::FormId(cell)).map(|f| f.0));
            scripted = assigned.is_some();
            let bound = assigned.or_else(|| fnv_world::lighting::cell_image_space(world.lo, world.schema, cell));
            state.active_interior_space = bound;
            bound
        } else {
            state.active_interior_space
        };
        match space {
            Some(id) => match effective_image_space(&world, game, id) {
                Some(grade) => {
                    if sky.interior_grade != Some(grade) {
                        if scripted {
                            info!("cell image space: {cell:#010x} graded with {id:#010x} (SetCellImageSpace)");
                        }
                        sky.interior_grade = Some(grade);
                    }
                }
                None => warn!("cell image space: {id:#010x} for {cell:#010x} is not an image space the grade reads"),
            },
            None => sky.interior_grade = None,
        }
        return;
    }
    state.active_interior_space = None;
    if rebind {
        state.active_exterior_cell_space = ext_cell.and_then(|cell| {
            game.and_then(|g| g.forms.cell_image_spaces.get(&fnv_sim::FormId(cell)).map(|f| f.0))
        });
    }
    let world_space = state
        .active_exterior_cell_space
        .or_else(|| effective_worldspace_image_space_id(&world, game));
    let id_changed = sky.world_image_space_id != world_space;
    sky.world_image_space_id = world_space;
    let next = world_space.and_then(|id| effective_image_space(&world, game, id));
    if id_changed || sky.image_space != next {
        sky.image_space = next;
        sky.refresh();
    }
}

pub fn ref_affine(p: &Placement) -> Affine3A {
    let m = Mat3::from_cols_array(&ref_rotation(p.rotation)).transpose(); // row-major
    Affine3A::from_mat3_translation(m * p.scale, Vec3::from_array(p.position))
}

/// Resources shared by a cell's actor spawns, including readiness refreshes.
type ActorDraw<'a, 'w> = (&'a crate::actors::ActorWorld, &'a mut ActorParts, Option<&'a fnv_sim::World<'w>>, &'a mut crate::readahead::ReadAhead);

/// Load a cell whole in one go: [`begin_cell`], then every reference ([`spawn_ref`]).
#[allow(clippy::too_many_arguments)]
fn load_cell(
    commands: &mut Commands,
    world: &WorldData,
    key: CellKey,
    models: &mut ModelCtx,
    terrain: &mut TerrainMaterials,
    water: &mut WaterCtx,
    trees: &mut crate::tree::TreeCtx,
    grass: &mut crate::grass::GrassCtx,
    emittance: &mut crate::emittance::EmittanceState,
    actors: &mut Option<ActorDraw<'_, '_>>,
    streamer: &mut Streamer,
) -> Entity {
    let tile = terrain.settings.tile;
    let (cell_e, refs, origin, start) = begin_cell(commands, world, key, None, tile, models, terrain, water, false, streamer);
    let mut sp = CellSpawn { entity: cell_e, refs, next: 0, origin, ms: 0.0, frames: 1, start, grass: None };
    sp.start(commands, world, models, terrain, grass, false, || false);
    for r in sp.refs {
        spawn_ref(commands, world, cell_e, origin, r, models, water, trees, emittance, actors, streamer);
    }
    cell_e
}

/// Spawn a cell's entity with its water and begin its landscape (from `prep`, read ahead,
/// or read here); returns it with the references to spawn into it, its origin (game space)
/// and what is left of its start: the terrain's quadrants and the grass ([`CellStart`]).
/// `hidden`: spawned hidden, to be shown when its references are all in.
#[allow(clippy::too_many_arguments)]
fn begin_cell(
    commands: &mut Commands,
    world: &WorldData,
    key: CellKey,
    prep: Option<TerrainPrep>,
    tile: f32,
    models: &mut ModelCtx,
    terrain: &mut TerrainMaterials,
    water: &mut WaterCtx,
    hidden: bool,
    streamer: &mut Streamer,
) -> (Entity, Vec<u32>, Vec3, Option<CellStart>) {
    let stats = &mut streamer.stats;
    stats.cell_starts += 1;
    let (origin, cell, name) = match key {
        CellKey::Exterior(g) => {
            (Vec3::new(g.0 as f32 * CELL_SIZE, g.1 as f32 * CELL_SIZE, 0.0), world.grid_cell(g), format!("Cell {},{}", g.0, g.1))
        }
        CellKey::Interior(id) => (Vec3::ZERO, world.index.cells.get(&id), format!("Interior {id:08X}")),
    };
    let visibility = if hidden { Visibility::Hidden } else { Visibility::default() };
    let cell_e = commands.spawn((Name::new(name), CellEntity, Transform::from_translation(origin), visibility)).id();
    // Hook for nvf-play: it draws the references the simulation created (dropped items)
    // in this cell as children of this entity.
    commands.entity(cell_e).insert(match key {
        CellKey::Exterior((x, y)) => RenderCell::Exterior { world: world.world, x, y },
        CellKey::Interior(id) => RenderCell::Interior(id),
    });
    let mut refs: Vec<u32> = cell.map(|c| c.refs.clone()).unwrap_or_default();
    let mut start = None;
    if let CellKey::Exterior(g) = key {
        if let Some((c, land_id)) = cell.and_then(|c| Some((c, c.land?))) {
            let prep = prep.or_else(|| {
                let _span = info_span!("world::cell_terrain_unprepared").entered();
                prepare_terrain(world.lo, world.schema, land_id, g, tile, world.land_ids(g), nvf_play::smooth_terrain::spec(), crate::terrain_lod::first_draw())
            });
            if let Some(prep) = prep {
                // Grass grows on the landscape's textures (grass.rs), once the terrain is
                // in; the water surface is the one drawn below (`spawn_water`).
                let ws = world.index.worldspaces.get(&world.world);
                let water = c.has_water().then(|| c.water_surface(ws.and_then(|w| w.default_water_height))).flatten();
                let spawn = crate::terrain::TerrainSpawn::begin(world, prep, &mut TerrainCtx { tm: terrain, assets: models });
                start = Some(CellStart::Terrain { spawn: Box::new(spawn), land_id, water });
            }
        }
        if let Some(p) = world.persistent_by_grid.get(&g) {
            refs.extend(p);
        }
    }
    if let Some(c) = cell {
        spawn_water(commands, cell_e, world, c, models, water, stats);
    }
    (cell_e, refs, origin, start)
}

/// The landscape of exterior grid square `g` (its cell `c`) spawned under `cell_e`: from
/// `prep` (read ahead) or read here; with `draw_only` drawn only, as the full-detail ring
/// draws it (`crate::terrain::spawn_terrain_draw_only`). The LAND's FormID, the landscape and
/// the surface drawn, for the grass; `None` without landscape.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_cell_terrain(
    commands: &mut Commands,
    world: &WorldData,
    cell_e: Entity,
    g: (i32, i32),
    c: &fnv_world::Cell,
    prep: Option<TerrainPrep>,
    tile: f32,
    models: &mut ModelCtx,
    terrain: &mut TerrainMaterials,
    draw_only: bool,
) -> Option<(u32, fnv_world::land::Land, fnv_world::land_surface::Surface)> {
    let land_id = c.land?;
    let prep = prep.or_else(|| prepare_terrain(world.lo, world.schema, land_id, g, tile, world.land_ids(g), nvf_play::smooth_terrain::spec(), crate::terrain_lod::first_draw()))?;
    let tm = &mut TerrainCtx { tm: terrain, assets: models };
    let (land, surface) = if draw_only { crate::terrain::spawn_terrain_draw_only(commands, cell_e, world, prep, tm) } else { crate::terrain::spawn_terrain(commands, cell_e, world, prep, tm) };
    Some((land_id, land, surface))
}

/// What a reference is, for drawing it ([`classify_ref`]): the record-type decision of
/// [`spawn_ref`], with nothing spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefKind {
    /// Drawn by the regions the camera is in (imposters.rs), not by its cell.
    Imposter,
    /// A primitive: a trigger volume, room bounds, a portal (level-design.md §4.1).
    Primitive,
    /// Not enabled at the start of a game (deleted, or its enable state).
    Disabled,
    /// A placed NPC or creature (actors.rs).
    Actor,
    /// A base type not drawn as a model (sounds, idle markers, …), or no base.
    Skipped,
    /// Drawn: with its light source (`light`, a LIGH), as `how` says.
    Drawn { light: bool, how: DrawnAs },
}

/// How a drawn reference is drawn ([`RefKind::Drawn`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawnAs {
    /// A SpeedTree (tree.rs, or its billboard).
    Tree,
    /// Placeable water, drawn as its water type's surface when it has one, else a model.
    PlacedWater,
    /// Its model ([`spawn_model`]).
    Model,
}

/// The record-type decision of [`spawn_ref`] for placement `p` with its base `base`;
/// `enabled` is asked only for references past the imposter and primitive tests. Whether
/// the reference is drawn with its cell at all is [`drawn_with_cell`]'s rule, the one
/// every drawing path shares; when it is not, the kind says why (an imposter before a
/// primitive).
pub(crate) fn classify_ref(p: &Placement, base: Option<&BaseObject>, enabled: impl FnOnce() -> bool) -> RefKind {
    if !drawn_with_cell(p) {
        return if p.imposter { RefKind::Imposter } else { RefKind::Primitive };
    }
    if !enabled() {
        return RefKind::Disabled;
    }
    let Some(base) = base else { return RefKind::Skipped };
    if is_actor(&base.sig) {
        return RefKind::Actor;
    }
    if !DRAWN.contains(&&base.sig) {
        return RefKind::Skipped;
    }
    let how = if base.is_speedtree() {
        DrawnAs::Tree
    } else if &base.sig == b"PWAT" {
        DrawnAs::PlacedWater
    } else {
        DrawnAs::Model
    };
    RefKind::Drawn { light: &base.sig == b"LIGH", how }
}

/// Whether a base record type is a placed NPC's or creature's.
fn is_actor(sig: &[u8; 4]) -> bool {
    matches!(sig, b"NPC_" | b"CREA")
}

/// Spawn reference `r` into the cell `cell_e` whose origin is `origin` (game space).
#[allow(clippy::too_many_arguments)]
fn spawn_ref(
    commands: &mut Commands,
    world: &WorldData,
    cell_e: Entity,
    origin: Vec3,
    r: u32,
    models: &mut ModelCtx,
    water: &mut WaterCtx,
    trees: &mut crate::tree::TreeCtx,
    emittance: &mut crate::emittance::EmittanceState,
    actors: &mut Option<ActorDraw<'_, '_>>,
    streamer: &mut Streamer,
) {
    // Once the start's cells are in, an actor whose parts are not read yet is drawn when
    // they are (`draw_waiting_actors`); before, behind the loading screen, at once.
    streamer.stats.reference_attempts += 1;
    let waits = streamer.settled;
    let override_model = streamer.model_overrides.get(&r).cloned();
    let (hide, light_scale, managed, stats, waiting) = (&streamer.hide, streamer.light_scale, &streamer.managed, &mut streamer.stats, &mut streamer.waiting);
    let manage_actors = streamer.manage_actors;
    let to_cell = Affine3A::from_translation(-origin);
    if managed.contains(&r) {
        return;
    }
    let Some(p) = world.index.placement(world.lo, world.schema, r) else { return };
    let base: Option<BaseObject> = p.base.and_then(|b| base_object(world.lo, world.schema, b));
    let skip_type = |stats: &mut StreamStats| {
        stats.refs_skipped_type += 1;
        *stats.skipped_types.entry(base_type_name(base.as_ref())).or_default() += 1;
    };
    let (light, how) = match classify_ref(&p, base.as_ref(), || world.enabled(&p, 0)) {
        // Imposters are drawn by the regions the camera is in, not by their cell
        // (imposters.rs).
        RefKind::Imposter => {
            stats.refs_imposters += 1;
            return;
        }
        // A primitive (a trigger volume, room bounds, a portal) is seen only in the editor,
        // even where its activator was copied from one with a model (level-design.md §4.1).
        RefKind::Primitive => {
            stats.refs_skipped_type += 1;
            *stats.skipped_types.entry("primitive".into()).or_default() += 1;
            return;
        }
        _ if manage_actors && base.as_ref().is_some_and(|b| is_actor(&b.sig)) => return,
        RefKind::Disabled => {
            // Actors, creatures and light sources are spawned hidden, so a script's
            // Enable can show them (actors.rs `SpawnedDisabled`); other models are
            // nvf-play's (`script_draw.rs`).
            if !p.deleted() {
                spawn_dormant(commands, cell_e, world, &p, r, to_cell, models, actors, light_scale, emittance, stats, waits.then_some(&mut *waiting));
            }
            stats.refs_skipped_disabled += 1;
            return;
        }
        RefKind::Actor => {
            // Placed NPCs and creatures (actors.rs `spawn`, `spawn_creature`, actors.md §10).
            if let (Some((aw, parts, sim, ahead)), Some(b)) = (actors.as_mut(), base.as_ref()) {
                let w = WaitingActor { cell: cell_e, rt: to_cell * ref_affine(&p), at: Vec3::from_array(p.position), reference: r, base: b.form_id, creature: &b.sig == b"CREA", hidden: false };
                if spawn_or_wait(commands, w, aw, *sim, ahead, &mut ActorCtx { cache: parts, models }, waits.then_some(&mut *waiting), &mut stats.actors) {
                    return;
                }
            }
            skip_type(stats);
            return;
        }
        RefKind::Skipped => {
            skip_type(stats);
            return;
        }
        RefKind::Drawn { light, how } => (light, how),
    };
    let Some(base) = base else { return };
    if light
        && crate::lights::spawn_light(
            commands,
            cell_e,
            world,
            base.form_id,
            base.model.as_deref(),
            models,
            to_cell * ref_affine(&p),
            light_scale,
            &p,
            emittance,
        ).is_some_and(|light| light.emitting)
    {
        stats.lights += 1;
    }
    if how == DrawnAs::Tree {
        // Geometry generated from the .spt parameters (tree.rs); without it, the
        // tree's billboard as one upright quad turned towards the camera. The
        // billboard drops the reference's rotation: it has no facing of its own.
        if let Some(v) = trees.visual(world, base.form_id, models) {
            let rt = Mat4::from(to_cell * ref_affine(&p));
            stats.shapes += crate::tree::spawn(commands, cell_e, &v, rt, RefVisual(r));
            stats.refs_speedtree += 1;
            return;
        }
        if let Some((mesh, mat)) = billboard(world, base.form_id, models) {
            let at = Vec3::from_array(p.position) - origin;
            let t = Transform::from_translation(at).with_scale(Vec3::splat(p.scale));
            commands.entity(cell_e).with_child((Mesh3d(mesh), MeshMaterial3d(mat), t, TreeBillboard, RefVisual(r)));
            stats.shapes += 1;
        }
        stats.refs_speedtree += 1;
        return;
    }
    // Placeable water: its model's shapes drawn as its water type's surface.
    if how == DrawnAs::PlacedWater && spawn_placed_water(commands, cell_e, world, &base, r, streamer.interior.is_none().then(|| Vec3::from_array(p.position)), to_cell * ref_affine(&p), models, water, stats) {
        return;
    }
    // A destroyed reference: its destruction stage's replacement model (destroyed.rs).
    let base = match override_model {
        Some(m) => base_object_with_model(base, &m),
        None => base,
    };
    // The reference transform replaces the model root's own (world.md §4).
    spawn_model(commands, cell_e, world, &p, r, &base, to_cell * ref_affine(&p), models, emittance, hide, stats, SpawnMode::Full);
}

/// A placeable water reference (PWAT): the shapes of its model (a flat square, e.g.
/// `water_placeable\waterp512x512.nif`, whose one shape has a `WaterShaderProperty` and
/// no texture) drawn with the water material of the PWAT's water type, as a cell's water
/// plane is (water.rs) [hypothesis: that the game draws them with the WATR's look; drawn
/// as an ordinary model they were untextured and pure white: the Goodsprings cave's
/// pools, owner's playtest list 5 item 70; world.md §6]. False when it has no water type
/// or model, to be drawn as a model.
#[allow(clippy::too_many_arguments)]
fn spawn_placed_water(
    commands: &mut Commands,
    cell_e: Entity,
    world: &WorldData,
    base: &BaseObject,
    r: u32,
    exterior_position: Option<Vec3>,
    rt: Affine3A,
    models: &mut ModelCtx,
    water: &mut WaterCtx,
    stats: &mut StreamStats,
) -> bool {
    let Some(wid) = fnv_world::sky::placeable_water_type(world.lo, world.schema, base.form_id) else { return false };
    let Some(path) = base.world_model_path() else { return false };
    let Some(model) = models.model(&path) else { return false };
    let reflects = fnv_world::sky::placeable_water_reflects(world.lo, world.schema, base.form_id);
    let Some(mat) = water_material(world, wid, reflects, true, exterior_position, models, water) else { return false };
    if !reflects {
        info!("placed water {r:08X}: its PWAT {:08X} does not set Reflects, drawn without reflection", base.form_id);
    }
    for s in &model.shapes {
        commands.entity(cell_e).with_child((
            Mesh3d(s.mesh.clone()),
            MeshMaterial3d(mat.clone()),
            bevy::mesh::MeshTag::new(r),
            Transform::from_matrix(Mat4::from(rt * s.transform)),
            bevy::camera::visibility::RenderLayers::layer(crate::reflection::WATER_LAYER),
            RefVisual(r),
        ));
    }
    stats.water_planes += 1;
    true
}

/// How [`spawn_model`] draws a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnMode {
    /// As the loaded cells draw it: rigs for what moves, stored poses, emitters, the mesh
    /// terrain blend's census, the error marker for a model that did not load.
    Full,
    /// Drawn only, for the full-detail ring (full_detail.rs, #2701): the static model's
    /// shapes baked as the file stores them (billboard nodes still turn, damage stages
    /// still follow the game), a moveable static's idle sequence playing, every entity
    /// marked `nvf_play::DrawOnly`; no other rig, pose, emitter, census, mine light or
    /// error marker, and nothing logged [design].
    Scenery,
}

/// Draw a reference's model as children of `parent`, `rt` being the reference's
/// transform in the parent's space: an exterior or interior cell, or the world root for
/// imposters (`imposters.rs`); `mode`: [`SpawnMode`]. Whether a shape of the model was
/// spawned (false: no model, hidden, a marker, a model that did not load or has no shape;
/// the full-detail ring's Scenery mode then spawns nothing at all).
#[allow(clippy::too_many_arguments)]
pub fn spawn_model(
    commands: &mut Commands,
    parent: Entity,
    world: &WorldData,
    p: &Placement,
    r: u32,
    base: &BaseObject,
    rt: Affine3A,
    models: &mut ModelCtx,
    emittance: &mut crate::emittance::EmittanceState,
    hide: &[String],
    stats: &mut StreamStats,
    mode: SpawnMode,
) -> bool {
    let scenery = mode == SpawnMode::Scenery;
    // Armour only ever worn (Non-Playable, no world model): its biped model would hang in
    // the air at the reference (the Fort's slave backpacks, #2439; `fnv_world::placed::worn_only`).
    if base.worn_only {
        stats.refs_no_model += 1;
        if scenery {
            return false;
        }
        info!("world: 0x{r:08X} {}: armour only ever worn (Non-Playable, no world model), not drawn on its own", base.edid.as_deref().unwrap_or("?"));
        return false;
    }
    let Some(path) = base.world_model_path() else {
        stats.refs_no_model += 1;
        return false;
    };
    if hide.iter().any(|h| path.contains(h.as_str())) {
        return false;
    }
    if is_drawn_marker(base.form_id, &path) {
        stats.refs_skipped_type += 1;
        *stats.skipped_types.entry("marker".into()).or_default() += 1;
        return false;
    }
    // With the base's alternate textures on the shapes they name (texture-swaps.md §4).
    let Some(model) = models.model_swapped(&path, false, &base.world_alt_textures) else {
        stats.refs_no_model += 1;
        if !scenery && spawn_error_marker(commands, parent, r, rt, models) {
            stats.refs_error_marker += 1;
            // Once per model: a broken mod can place hundreds of one missing model.
            static TOLD: std::sync::Mutex<Option<HashSet<String>>> = std::sync::Mutex::new(None);
            if TOLD.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashSet::new).insert(path.clone()) {
                info!("world: 0x{r:08X} {path}: its model did not load; drawn as the error marker (each reference of it)");
            }
        }
        return false;
    };
    // A door that opens in place (no XTEL) and has Open/Close sequences: posed from
    // its state in `nvf_play::doors` (doors.rs). Teleport doors stay static. A container
    // whose model has them (a locker, a safe) is drawn and timed the same way
    // (`nvf_play::container_anim`), and so is a terminal whose keyboard and screen come
    // out (`nvf_play::terminal`, #1532).
    let door = model.rig.as_ref().filter(|rig| rig.door.is_some() && ((&base.sig == b"DOOR" && !p.teleport) || &base.sig == b"CONT" || &base.sig == b"TERM"));
    // An activator with Open/Close or an idle (a radio, a jukebox): its on and off
    // looks follow its state (activators.rs).
    let activator = model.rig.as_ref().filter(|rig| (rig.door.is_some() || rig.idle.is_some()) && &base.sig == b"ACTI");
    // A sequence only scripts play (PlayGroup Forward, Left, …): its own node hierarchy
    // too, so a script can play it (object_anims.rs).
    let scripted = model.rig.as_ref().is_some_and(|rig| rig.has_script_groups());
    // An item lying in the world (a gun, a hat) keeps its stored pose: the controllers of
    // its own that move parts (a slide, a magazine) are there for the clips of the actor
    // holding it [design; hypothesis: the game leaves them still on the ground,
    // effects-animation-audit.md §3].
    let idle = model.rig.as_ref().and_then(|rig| rig.idle.as_ref()).filter(|c| !(c.only_free_motion() && is_item(&base.sig)));
    // An activator whose model joins its bodies (a swinging trap): its nodes, for its
    // bodies to move once it is let go (swing_traps.rs).
    let jointed = model.jointed && &base.sig == b"ACTI";
    // A destroyed reference drawn with its destruction model (destroyed.rs: not its base
    // object's model) whose bodies are loose parts, a car's hulk: its nodes, for the
    // bodies to move once they fall (wrecks.rs). Any other model in place of the base's
    // passes this test too; wrecks.rs lets the parts fall only when the game has the
    // reference destroyed to that model and physics built its collision from it.
    let wreck = model.loose && p.base.and_then(|b| base_object(world.lo, world.schema, b)).and_then(|b| b.world_model_path()).is_some_and(|m| m != path);
    // A lamp's housing, shade or bulb (or a light's own model): casts no lamp shadows
    // (light_shadows.rs `LightFixture`).
    let fixture = &base.sig == b"LIGH" || crate::light_shadows::is_fixture_model(&path);
    // Drawn only (the full-detail ring): a moveable static's idle alone, nothing else rigged.
    let rigged = |rig: &&nvf_assets::model::Rig| match scenery {
        true => &base.sig == b"MSTT" && rig.idle.is_some() && idle.is_some() && !rig.shapes.is_empty(),
        false => (rig.idle.is_some() && idle.is_some()) || door.is_some() || activator.is_some() || scripted || jointed || wreck,
    };
    if let Some(rig) = model.rig.as_ref().filter(rigged) {
        // Animated (Idle sequence): its own node hierarchy per reference.
        // "Random Anim Start" bases start their idle at a phase spread by reference
        // (fnv_actor::anims::random_anim_start; animation.md §8).
        let phase = if random_anim_start(world, base.form_id) { crate::actors::clip_phase(r) } else { 0.0 };
        stats.refs_random_start += usize::from(phase > 0.0 && rig.idle.is_some());
        let (root, drawn, nodes) = rig.spawn_nodes(commands, parent, Transform::from_matrix(Mat4::from(rt)), &path, phase);
        commands.entity(root).insert(RefVisual(r));
        for e in &drawn {
            commands.entity(*e).insert(RefVisual(r));
            if fixture {
                commands.entity(*e).insert(crate::light_shadows::LightFixture);
            }
        }
        // Drawn only: its idle plays, and that is all (no door, group, emitter, light or
        // node map; `object_anims::publish_poses` leaves it out).
        if scenery {
            for e in std::iter::once(root).chain(drawn.iter().copied()) {
                commands.entity(e).insert(nvf_play::DrawOnly);
            }
            // Still while its ring cell is hidden (full_detail.rs `fade_ring` wakes it).
            commands.entity(root).insert(crate::full_detail::RingDormant);
            stats.shapes += drawn.len();
            stats.refs_drawn += 1;
            stats.refs_animated += 1;
            return !drawn.is_empty();
        }
        // [design] Census while the blend is on (#2661): a rigged STAT's or SCOL's shapes
        // are tracked, never converted.
        if let Some(census) = blend_census(world, p, r, base, &path, &model, false) {
            for (index, e) in drawn.iter().enumerate() {
                commands.entity(*e).insert(crate::mesh_terrain_blend::BlendCensus { shape: index, ..census.clone() });
            }
        }
        if door.is_some() {
            commands.entity(root).insert(crate::doors::DoorRig::new(r, model.clone(), &path, nodes.clone(), p.open_by_default));
            stats.refs_doors += 1;
        } else if activator.is_some() {
            commands.entity(root).insert(crate::activators::ActivatorRig::new(r, base.form_id, model.clone(), &path, nodes.clone()));
        }
        // Its particle systems and add-on nodes emit from its own nodes.
        if model.particles {
            let start = rig.idle.as_ref().map_or(0.0, |c| phase * (c.stop - c.start));
            commands.entity(root).insert(crate::particles::ModelEmitter::placed_from(path.clone(), r, start));
        }
        for (i, n) in model.nodes.iter().enumerate() {
            if let Some((index, &e)) = n.addon.zip(n.name.as_ref().and_then(|k| nodes.get(k))) {
                crate::particles::place_addon(commands, e, r, &model.nodes, i, index);
            }
        }
        // A light whose model's sequences drive it: the reference's light hangs from the
        // node holding it and takes its keys (lights.rs, docs/spec-notes/model-lights.md).
        if let Some(l) = model.light.as_ref().filter(|_| &base.sig == b"LIGH") {
            let anchor = l.parent.as_ref().and_then(|n| nodes.get(n)).copied().unwrap_or(root);
            commands.entity(root).insert(crate::lights::LightRig { reference: r, anchor });
        }
        if jointed {
            commands.entity(root).insert(crate::swing_traps::TrapRig { form_id: r, path: path.clone(), nodes: nodes.clone() });
        }
        if wreck {
            commands.entity(root).insert(crate::wrecks::WreckRig { form_id: r, path: path.clone(), nodes: nodes.clone() });
        }
        if !rig.groups.is_empty() {
            commands.entity(root).insert(crate::object_anims::GroupRig::new(r, model.clone(), &path, nodes.clone()));
            stats.refs_group_rigs += 1;
        }
        // Its nodes by name, for what follows one (`nvf_play::RefNodes`, ui_attach.rs): the
        // map spawn_nodes made, kept rather than dropped here (copied only when a GroupRig
        // took it above, as before).
        commands.entity(root).insert(nvf_play::RefNodes { reference: r, nodes });
        let boards = rig.nodes.iter().skip(1).filter(|n| n.billboard.is_some()).count();
        stats.billboards += boards;
        stats.refs_billboards += usize::from(boards > 0);
        stats.shapes += drawn.len();
        stats.refs_drawn += 1;
        stats.refs_animated += 1;
        return true;
    }
    // A jointed object left posed by its designer (`XRGD`, `nvf_play::clutter_pose`).
    // Whether it poses is decided by `clutter_pose::drawn` (the matching rule and every
    // body on a named node below the model's root), the same condition the collision
    // uses and the same function `nvf_play::script_draw` draws references enabled later
    // with (issue #2339).
    let posed = if scenery { None } else { nvf_play::clutter_pose::drawn(world.lo, &base.sig, r, &path, &model.nodes, || nvf_play::clutter_pose::body_nodes(&models.data, &path)) };
    if let Some(p) = &posed {
        info!("clutter pose: 0x{r:08X} {path} drawn in its stored pose ({} bodies)", p.bodies);
    }
    stats.refs_posed += usize::from(posed.is_some());
    // [design] Census of every STAT and SCOL shape while the blend is on, before any
    // selection (#2661).
    let census = if scenery { None } else { blend_census(world, p, r, base, &path, &model, posed.is_some()) };
    // Shapes below a billboard node go under an entity for it that turns to the camera
    // (#2549: a torch's glow card, else a flat sheet); a posed model stays as posed. Its
    // holder carries the reference's RefVisual, so moving and redrawing it take them along.
    let mut boards = nvf_assets::model::BakedBillboards::default();
    for (index, s) in model.shapes.iter().enumerate() {
        let shape = nvf_play::clutter_pose::shape_transform(posed.as_ref(), s);
        let board = match posed.is_none().then_some(scenery) {
            None => None,
            Some(false) => boards.place(commands, parent, &model, rt, s, RefVisual(r)),
            Some(true) => boards.place(commands, parent, &model, rt, s, (RefVisual(r), nvf_play::DrawOnly)),
        };
        let (parent, m, billboard) = match board {
            Some((board, below)) => (board, Mat4::from(below), true),
            None => (parent, Mat4::from(rt * shape), false),
        };
        // External emittance: the reference's XEMI source drives the colour.
        let material = p
            .emittance
            .filter(|_| s.external_emittance)
            .and_then(|x| emittance.material(world, &mut models.materials, &s.material, x))
            .unwrap_or_else(|| s.material.clone());
        // A shape below a damage-stage node: under a holder of its own, shown at its stages
        // only (destroyed.rs); the holder's RefVisual follows the reference's presence.
        let stages = s.node.and_then(|n| nvf_assets::model::damage_stages_of(&model.nodes, n));
        // [design] Morphing meshes are registered independently of rigs (nvf-assets/src/morph.rs).
        // Only shapes without reference or shape reasons may be converted (BlendShape).
        // [design] A shape turned to the camera under a billboard holder (#2549) is counted
        // but never blends, as the backfill (`blend_backfill`) leaves it out.
        let blend = census.as_ref().map(|c| {
            let mut tracked = shape_census(c, index, s, models);
            tracked.reasons |= crate::mesh_terrain_blend::billboard_reason(billboard);
            (tracked, material.clone())
        });
        let e = match stages {
            Some(st) => {
                let holder = commands.spawn((Transform::IDENTITY, Visibility::Inherited, RefVisual(r), ChildOf(parent))).id();
                if scenery {
                    commands.entity(holder).insert(nvf_play::DrawOnly);
                }
                commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(material), Transform::from_matrix(m), Visibility::Hidden, crate::destroyed::StagedShape::new(r, st), ChildOf(holder))).id()
            }
            None => commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(material), Transform::from_matrix(m), RefVisual(r), ChildOf(parent))).id(),
        };
        // In the same command as the spawn: no system sees the entity without its mark.
        if scenery {
            commands.entity(e).insert(nvf_play::DrawOnly);
        }
        #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
        if s.external_emittance && !scenery && !billboard && posed.is_none()
            && crate::room_beam_census::enabled()
            && !models.cache.morphs.ids().any(|id| id == s.mesh.id())
        {
            commands.entity(e).insert(crate::room_beam_census::RaySource::new(r, p.emittance, &path, s.block, rt * shape));
        }
        if let Some((tracked, original)) = blend {
            if tracked.reasons == 0 {
                commands.entity(e).insert(crate::mesh_terrain_blend::BlendShape::new(original));
            }
            commands.entity(e).insert(tracked);
        }
        // A mine's light parts: shown and hidden by its state (`nvf_play::mines`).
        if let Some(l) = nvf_play::mines::MineLight::of(s.name.as_deref(), &s.ancestors).filter(|_| !scenery) {
            commands.entity(e).insert(l);
        }
        if fixture {
            commands.entity(e).insert(crate::light_shadows::LightFixture);
        }
        stats.shapes += 1;
    }
    stats.billboards += boards.len();
    stats.refs_billboards += usize::from(!boards.is_empty());
    if !scenery {
        spawn_emitters(commands, parent, r, &model, &path, rt);
    }
    // A model without shapes draws nothing (counted as drawn by the loaded cells, as
    // before; not by the full-detail ring).
    stats.refs_drawn += usize::from(!scenery || !model.shapes.is_empty());
    !model.shapes.is_empty()
}

/// [design] The mesh terrain blend's census of a STAT or SCOL reference (#2661;
/// mesh_terrain_blend.rs) while the blend is on: its reference reasons from the actual
/// spawn path and prepared model, before any selection; `None` while the blend is off or
/// for another record type. Record presence follows fnv-game/src/data.rs (DEST); no
/// asset-name selection. The shape index and shape reasons are the caller's.
fn blend_census(world: &WorldData, p: &Placement, r: u32, base: &BaseObject, path: &str, model: &PreparedModel, posed: bool) -> Option<crate::mesh_terrain_blend::BlendCensus> {
    if !crate::mesh_terrain_blend::recording() {
        return None;
    }
    census_of(world, p, r, base, path, model, posed)
}

/// [`blend_census`] whether or not the blend records spawns now (the backfill's).
fn census_of(world: &WorldData, p: &Placement, r: u32, base: &BaseObject, path: &str, model: &PreparedModel, posed: bool) -> Option<crate::mesh_terrain_blend::BlendCensus> {
    let base_type = crate::mesh_terrain_blend::BaseType::of(&base.sig)?;
    let exterior = world.index.refs.get(&r).and_then(|r| world.index.cells.get(&r.cell))
        .is_some_and(|c| !c.is_interior() && c.world == Some(world.world));
    let destructible = fnv_world::fields::winner(world.lo, base.form_id)
        .and_then(|(pi, record)| fnv_world::fields::Fields::new(record, pi, world.schema).ok())
        .is_some_and(|f| f.raw(b"DEST").is_some());
    let facts = crate::mesh_terrain_blend::ReferenceFacts {
        exterior,
        rigged: model.rig.is_some() || model.skinned != 0,
        particles: model.particles,
        posed: model.jointed || model.loose || posed,
        damage_stages: model.nodes.iter().any(|n| n.damage_stages.is_some()),
        destructible,
        distant: p.imposter,
    };
    Some(crate::mesh_terrain_blend::BlendCensus {
        reference: r,
        base: base.form_id,
        base_type,
        model: path.into(),
        shape: 0,
        reasons: crate::mesh_terrain_blend::reference_reasons(facts),
    })
}

/// [design] Shape `index` of a reference's census: its shape reasons (external emittance,
/// a registered morph mesh, nvf-assets/src/morph.rs) added to the reference's.
fn shape_census(c: &crate::mesh_terrain_blend::BlendCensus, index: usize, s: &nvf_assets::model::PreparedShape, models: &ModelCtx) -> crate::mesh_terrain_blend::BlendCensus {
    let morph = models.cache.morphs.ids().any(|id| id == s.mesh.id());
    let reasons = c.reasons | crate::mesh_terrain_blend::shape_reasons(s.external_emittance, morph);
    crate::mesh_terrain_blend::BlendCensus { shape: index, reasons, ..c.clone() }
}

/// The shapes drawn while the blend was off: no census yet, the ordinary material on.
type Untracked<'w, 's> = Query<'w, 's, (Entity, &'static RefVisual, &'static Mesh3d, &'static MeshMaterial3d<NifMaterial>, &'static ChildOf), Without<crate::mesh_terrain_blend::BlendCensus>>;

/// [design] The blend turned on while cells were drawn (#2661, the settings row): the
/// census and selection of every STAT and SCOL shape already drawn directly under a cell
/// as `spawn_model` draws a static one, once per switch (`BlendState::shapes_pending`).
/// The same rules as at spawn: the reference's placement, base (a destroyed reference's
/// replacement model, destroyed.rs), prepared model and stored pose decide its reasons
/// ([`census_of`]); a shape is matched to its model's shape by its mesh, which gives its
/// index and shape reasons ([`shape_census`]); the material it draws with now is its
/// source. Shapes under a rig's nodes (animated models), under a damage stage's holder,
/// under a billboard holder (#2549) or drawn as an imposter have no cell for a parent and
/// are left out; those can never blend anyway.
pub(crate) fn blend_backfill(
    mut commands: Commands,
    world: Res<WorldData>,
    streamer: Res<Streamer>,
    mut state: ResMut<crate::mesh_terrain_blend::BlendState>,
    mut models: ModelCtx,
    shapes: Untracked,
    cells: Query<(), With<CellEntity>>,
) {
    if !std::mem::take(&mut state.shapes_pending) {
        return;
    }
    let mut by_ref = HashMap::<u32, Vec<_>>::new();
    for (e, r, mesh, material, parent) in &shapes {
        if cells.contains(parent.parent()) {
            by_ref.entry(r.0).or_default().push((e, &mesh.0, &material.0));
        }
    }
    let (mut tracked, mut convertible) = (0, 0);
    for (r, drawn) in by_ref {
        let Some(p) = world.index.placement(world.lo, world.schema, r) else { continue };
        let Some(base) = p.base.and_then(|b| base_object(world.lo, world.schema, b)) else { continue };
        if crate::mesh_terrain_blend::BaseType::of(&base.sig).is_none() {
            continue;
        }
        let base = match streamer.model_overrides.get(&r) {
            Some(m) => base_object_with_model(base, m),
            None => base,
        };
        let Some(path) = base.world_model_path() else { continue };
        let Some(model) = models.model_swapped(&path, false, &base.world_alt_textures) else { continue };
        let posed = nvf_play::clutter_pose::drawn(world.lo, &base.sig, r, &path, &model.nodes, || nvf_play::clutter_pose::body_nodes(&models.data, &path)).is_some();
        let Some(census) = census_of(&world, &p, r, &base, &path, &model, posed) else { continue };
        for (e, mesh, material) in drawn {
            let Some((index, s)) = model.shapes.iter().enumerate().find(|(_, s)| &s.mesh == mesh) else { continue };
            let shape = shape_census(&census, index, s, &models);
            if shape.reasons == 0 {
                commands.entity(e).try_insert(crate::mesh_terrain_blend::BlendShape::new(material.clone()));
                convertible += 1;
            }
            commands.entity(e).try_insert(shape);
            tracked += 1;
        }
    }
    info!("mesh terrain blend: backfill tracked-shapes={tracked} convertible-shapes={convertible}");
}

/// A placed model's own particle systems (a fire's flames: Gomorrah's roof,
/// `nvfiregomorrah01.nif`) and its add-on nodes (`BSValueNode`, the ADDN fire beside
/// them): emitted from a holder at the reference with its nodes below it, which a
/// static model's baked shapes do not have. Under `RefVisual(r)`, so a disabled
/// reference does not burn (`particles::emit`).
pub(crate) fn spawn_emitters(commands: &mut Commands, parent: Entity, r: u32, model: &PreparedModel, path: &str, rt: Affine3A) {
    let addons = model.nodes.iter().any(|n| n.addon.is_some());
    if !model.particles && !addons {
        return;
    }
    let holder = commands.spawn((Transform::from_matrix(Mat4::from(rt)), Visibility::Inherited, RefVisual(r), ChildOf(parent))).id();
    if model.particles {
        commands.entity(holder).insert(crate::particles::ModelEmitter::placed(path, r));
    }
    if !addons {
        return;
    }
    let nodes: Vec<Entity> = model.nodes.iter().map(|n| commands.spawn((Transform::from_matrix(Mat4::from(n.local)), Visibility::Inherited)).id()).collect();
    for (i, (n, &e)) in model.nodes.iter().zip(&nodes).enumerate() {
        commands.entity(n.parent.map_or(holder, |p| nodes[p])).add_child(e);
        if let Some(index) = n.addon {
            crate::particles::place_addon(commands, e, r, &model.nodes, i, index);
        }
    }
}

/// Whether a base record type is an item a reference can pick up.
fn is_item(sig: &[u8; 4]) -> bool {
    matches!(sig, b"WEAP" | b"ARMO" | b"AMMO" | b"MISC" | b"ALCH" | b"BOOK" | b"KEYM" | b"NOTE" | b"IMOD" | b"CHIP" | b"CMNY" | b"CCRD")
}

/// Whether base form `fid` has the "Random Anim Start" record flag (fnv_actor::anims).
fn random_anim_start(world: &WorldData, fid: u32) -> bool {
    let Some(d) = world.lo.index.get(&fid).and_then(|d| d.last()) else { return false };
    let h = &world.lo.records[d.plugin as usize][d.ordinal as usize].header;
    fnv_actor::anims::random_anim_start(&h.sig, h.flags)
}

/// A reference not enabled at the start of a game: an NPC or creature is spawned
/// hidden, and a light's light source hidden under a `RefVisual` holder, so that
/// `RefVisual` sync shows them when a script enables the reference (actors.rs
/// `SpawnedDisabled`; docs/design/layer5-play.md §4). The light's model is drawn by
/// nvf-play's `script_draw.rs` when enabled.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_dormant(
    commands: &mut Commands,
    cell_e: Entity,
    world: &WorldData,
    p: &Placement,
    r: u32,
    to_cell: Affine3A,
    models: &mut ModelCtx,
    actors: &mut Option<ActorDraw<'_, '_>>,
    light_scale: f32,
    emittance: &mut crate::emittance::EmittanceState,
    stats: &mut StreamStats,
    waiting: Option<&mut Vec<WaitingActor>>,
) {
    let Some(base) = p.base.and_then(|b| base_object(world.lo, world.schema, b)) else { return };
    let rt = to_cell * ref_affine(p);
    match (&base.sig, actors.as_mut()) {
        (b"NPC_" | b"CREA", Some((aw, parts, sim, ahead))) => {
            let w = WaitingActor { cell: cell_e, rt, at: Vec3::from_array(p.position), reference: r, base: base.form_id, creature: &base.sig == b"CREA", hidden: true };
            spawn_or_wait(commands, w, aw, *sim, ahead, &mut ActorCtx { cache: parts, models }, waiting, &mut stats.actors);
        }
        (b"LIGH", _) => {
            let holder = commands.spawn((Transform::IDENTITY, Visibility::Inherited, RefVisual(r), ChildOf(cell_e))).id();
            if let Some(light) = crate::lights::spawn_light(commands, holder, world, base.form_id, base.model.as_deref(), models, rt, light_scale, p, emittance) {
                crate::actors::spawn_hidden(commands, holder, r);
                if light.emitting { stats.lights_hidden += 1; }
            } else {
                commands.entity(holder).despawn();
            }
        }
        _ => {}
    }
}

/// Draw a placed actor or creature of a loaded cell (`actors::spawn`, `spawn_creature`),
/// or with `waiting` put it there when its parts are still to be read on a worker thread
/// (`readahead::ready_to_draw`) [engineering choice: it appears a few frames after its cell,
/// at the edge of the loaded cells, instead of stalling the frame]. Whether it was drawn
/// or put to wait.
#[allow(clippy::too_many_arguments)]
fn spawn_or_wait(
    commands: &mut Commands,
    w: WaitingActor,
    aw: &crate::actors::ActorWorld,
    sim: Option<&fnv_sim::World<'_>>,
    ahead: &mut crate::readahead::ReadAhead,
    ctx: &mut ActorCtx,
    waiting: Option<&mut Vec<WaitingActor>>,
    stats: &mut crate::actors::ActorStats,
) -> bool {
    if let Some(waiting) = waiting {
        if !crate::readahead::ready_to_draw(aw, sim, ctx, ahead, w.reference, w.base, w.creature) {
            waiting.push(w);
            return true;
        }
    }
    crate::readahead::clocked(|| {
        if w.creature {
            crate::actors::spawn_creature(commands, w.cell, w.rt, w.reference, w.base, aw, ctx, stats, w.hidden)
        } else {
            crate::actors::spawn(commands, w.cell, w.rt, w.at, w.reference, w.base, aw, sim, ctx, stats, w.hidden)
        }
    })
}

/// Draw the placed actors waiting for their parts ([`Streamer::waiting`]) whose parts
/// are in; those of cells gone since are dropped.
pub fn draw_waiting_actors(
    mut commands: Commands,
    mut streamer: ResMut<Streamer>,
    actor_world: Option<Res<crate::actors::ActorWorld>>,
    mut ahead: ResMut<crate::readahead::ReadAhead>,
    sim: Option<Res<nvf_play::sim::SimState>>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
) {
    if streamer.waiting.is_empty() {
        return;
    }
    let Some(aw) = actor_world.as_deref() else { return };
    let s = &mut *streamer;
    let before = (s.stats.actors.equipment_spawns, s.stats.actors.record_spawns);
    let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
    for w in std::mem::take(&mut s.waiting) {
        if !s.loaded.values().any(|&e| e == w.cell) {
            continue;
        }
        if !crate::readahead::ready_to_draw(aw, sim.as_deref().map(|s| &s.world), &mut ctx, &mut ahead, w.reference, w.base, w.creature) {
            s.waiting.push(w);
            continue;
        }
        spawn_or_wait(&mut commands, w, aw, sim.as_deref().map(|s| &s.world), &mut ahead, &mut ctx, None, &mut s.stats.actors);
    }
    crate::profile::actor_spawns(s.stats.actors.equipment_spawns - before.0, s.stats.actors.record_spawns - before.1);
}

/// Water surface of a cell: cells flagged "has water" get a plane at XCLW, or at the
/// worldspace's default water height when XCLW is absent or the 3.4e38 sentinel
/// [hypothesis, world.md §1]; the water type is CELL XCWT, else the worldspace's NAM2.
fn spawn_water(
    commands: &mut Commands,
    cell_e: Entity,
    world: &WorldData,
    c: &fnv_world::Cell,
    models: &mut ModelCtx,
    water: &mut WaterCtx,
    stats: &mut StreamStats,
) {
    if !c.has_water() {
        return;
    }
    let ws = &world.index.worldspaces[&world.world];
    // The same surface the physics swims in (`Cell::water_surface`, physics.md §6.6).
    let Some(height) = c.water_surface(ws.default_water_height) else { return };
    let Some(wid) = c.water_type.or(ws.water) else { return };
    let Some(mat) = water_material(world, wid, true, false, c.grid.map(|grid| cell_water_center(grid, height)), models, water) else { return };
    let s = CELL_SIZE;
    let mesh = water.plane(&mut models.meshes);
    commands.entity(cell_e).with_child((
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        Transform::from_xyz(s / 2.0, s / 2.0, height),
        bevy::camera::visibility::RenderLayers::layer(crate::reflection::WATER_LAYER),
    ));
    stats.water_planes += 1;
}

/// Billboard quad mesh and material for a TREE base (cached per base).
pub(crate) fn billboard(world: &WorldData, tree: u32, models: &mut ModelCtx) -> Option<(Handle<Mesh>, Handle<NifMaterial>)> {
    if let Some(b) = models.cache.billboard(tree) {
        return b;
    }
    let made = (|| {
        let ([w, h], tex) = fnv_world::placed::tree_billboard(world.lo, world.schema, tree)?;
        let a = &mut *models;
        let image = a.textures.get(&tex, true, &a.data, &mut a.images, a.formats.as_deref())?;
        let hw = w / 2.0;
        // One vertical quad in the local YZ plane facing +X, bottom at z = 0 (game space);
        // `face_billboards` turns +X towards the camera. Normals point up, so the
        // pre-shaded billboard picture is lit like the ground under it rather than
        // darkening when seen against the sun [hypothesis: chosen by eye].
        // Counter-clockwise seen from +X (local +Y is to the viewer's right), so the front
        // face is the one shown: double-sided materials flip the normal on back faces.
        let pos = vec![[0.0, -hw, 0.0], [0.0, hw, 0.0], [0.0, hw, h], [0.0, -hw, h]];
        let nor = vec![[0.0, 0.0, 1.0]; 4];
        let uv = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::RENDER_WORLD);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        mesh.insert_indices(bevy::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]));
        let mat = a.materials.add(nif_material(StandardMaterial {
            base_color_texture: Some(image),
            alpha_mode: AlphaMode::Mask(0.5),
            double_sided: true,
            cull_mode: None,
            perceptual_roughness: 0.9,
            ..default()
        }));
        Some((a.meshes.add(mesh), mat))
    })();
    models.cache.set_billboard(tree, made.clone());
    made
}

/// The game's own marker for a model it could not load: a big red sign with an
/// exclamation mark [observed: owner's screenshot of the original with a mod's model
/// missing; observed: `meshes\marker_error.nif` in Fallout - Meshes.bsa, one unlit
/// shape; hypothesis: the original draws this file in the missing model's place].
pub const ERROR_MARKER: &str = "meshes\\marker_error.nif";

/// Draw [`ERROR_MARKER`] where reference `r`'s model would be (its transform `rt` in
/// `parent`'s space), so a broken mod shows where it is broken; whether it was drawn
/// (the marker itself can be missing from a stripped install).
fn spawn_error_marker(commands: &mut Commands, parent: Entity, r: u32, rt: Affine3A, models: &mut ModelCtx) -> bool {
    let Some(marker) = models.model(ERROR_MARKER) else { return false };
    for s in &marker.shapes {
        let m = Mat4::from(rt * s.transform);
        commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(s.material.clone()), Transform::from_matrix(m), RefVisual(r), ChildOf(parent)));
    }
    true
}

fn base_type_name(b: Option<&BaseObject>) -> String {
    b.map_or("none".into(), |b| String::from_utf8_lossy(&b.sig).into_owned())
}

/// The base forms SetInternalMarker changed (`fnv_game::nvse_jip12`): drawn (false) or
/// not (true), for the references drawn from then on. Kept here, filled by
/// [`script_markers`], because every drawing path reads [`spawn_model`]'s marker test.
static SCRIPT_MARKERS: std::sync::RwLock<std::collections::BTreeMap<u32, bool>> = std::sync::RwLock::new(std::collections::BTreeMap::new());

/// Moved on each time [`SCRIPT_MARKERS`] changes, for what keeps lists classified by
/// [`is_drawn_marker`] (distant_lights.rs) to list them again.
static SCRIPT_MARKERS_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many times the scripts' marker changes changed (0 at the start).
pub fn script_markers_generation() -> u64 {
    SCRIPT_MARKERS_GENERATION.load(std::sync::atomic::Ordering::Acquire)
}

/// Whether the base `base` with model `path` is an editor marker, not drawn (a script's
/// SetInternalMarker first): [`spawn_model`]'s test, for callers that decide before it.
pub fn is_drawn_marker(base: u32, path: &str) -> bool {
    script_marker(base).unwrap_or_else(|| is_marker(path))
}

fn script_marker(base: u32) -> Option<bool> {
    SCRIPT_MARKERS.read().ok()?.get(&base).copied()
}

/// Take the scripts' marker changes from the game when they change.
pub fn script_markers(script: Option<Res<nvf_play::script::ScriptState>>) {
    let Some(g) = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game) else { return };
    let changes = g.internal_marker_changes();
    let same = SCRIPT_MARKERS.read().is_ok_and(|m| m.len() == changes.len() && m.iter().all(|(k, v)| changes.get(&fnv_sim::FormId(*k)) == Some(v)));
    if !same {
        if let Ok(mut m) = SCRIPT_MARKERS.write() {
            *m = changes.iter().map(|(k, v)| (k.0, *v)).collect();
            // Moved on while the write lock is held: a reader that sees the new generation
            // reads the new markers.
            SCRIPT_MARKERS_GENERATION.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            info!("render: {} base form(s) made markers or drawn by a script (SetInternalMarker)", m.len());
        }
    }
}

/// Whether a placed reference is drawn with its cell: not an imposter (drawn by the regions
/// the camera is in, imposters.rs) and not a primitive (a trigger volume, room bounds, a
/// portal: seen only in the editor, even where its activator was copied from one with a
/// model, level-design.md §4.1). Every drawing path asks this.
pub fn drawn_with_cell(p: &Placement) -> bool {
    !p.imposter && p.draws_base_model()
}

/// Editor-only marker meshes [hypothesis: the game hides them; models named marker*].
fn is_marker(path: &str) -> bool {
    let file = path.rsplit('\\').next().unwrap_or(path);
    file.starts_with("marker") || path.contains("\\markers\\")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A synthetic placed reference with the two drawing flags; no game data.
    pub(crate) fn flagged_placement(imposter: bool, primitive: bool) -> Placement {
        Placement {
            form_id: 0x100, sig: *b"REFR", flags: 0, base: Some(0x200), position: [0.0; 3], rotation: [0.0; 3], scale: 1.0,
            enable_parent: None, open_by_default: false, teleport: false, emittance: None, imposter, primitive,
        }
    }

    /// The ring's `classify_ref` takes "drawn with its cell" from `drawn_with_cell`, the
    /// rule every drawing path shares: a reference that rule turns away is an imposter or
    /// a primitive (an imposter first, `enabled` never asked), one it keeps goes on to the
    /// enabled and record-type tests.
    #[test]
    fn classify_ref_shares_drawn_with_cell() {
        let stat = BaseObject {
            form_id: 0x200, sig: *b"STAT", edid: None, model: Some("synthetic.nif".into()), world_model: None, world_alt_textures: vec![], worn_only: false, flags: 0, bounds: None,
        };
        for (imposter, primitive, want) in [
            (true, false, RefKind::Imposter),
            (true, true, RefKind::Imposter),
            (false, true, RefKind::Primitive),
            (false, false, RefKind::Drawn { light: false, how: DrawnAs::Model }),
        ] {
            let p = flagged_placement(imposter, primitive);
            let mut asked = false;
            let kind = classify_ref(&p, Some(&stat), || {
                asked = true;
                true
            });
            assert_eq!(kind, want, "imposter {imposter}, primitive {primitive}");
            assert_eq!(asked, drawn_with_cell(&p), "enabled is asked exactly for the references drawn with their cell");
            assert_eq!(drawn_with_cell(&p), !matches!(kind, RefKind::Imposter | RefKind::Primitive));
        }
    }

    #[test]
    fn spaces() {
        let t = game_to_bevy();
        let g = Vec3::new(1.0, 2.0, 3.0);
        let b = t.transform_point(g);
        assert!((bevy_to_game(b) - g).length() < 1e-5);
        // Game up (z) is Bevy up (y).
        assert!((t.transform_point(Vec3::Z) - Vec3::Y).length() < 1e-5);
    }

    /// A cell not ready: in play it waits `MAX_WAIT_SECS`; before the start square is in it
    /// is drawn at once, unless under the start's screen with the pool preparation on
    /// (`start_pool_prep`), where it waits for the read pool up to `MAX_WAIT_SECS` of real
    /// time.
    #[test]
    fn cells_wait_for_the_read_pool_under_the_start_screen_only() {
        assert!(!wait_over(true, 0.5, None));
        assert!(wait_over(true, MAX_WAIT_SECS, None));
        // In play the pool wait plays no part.
        assert!(!wait_over(true, 0.5, Some(9.0)));
        // Behind a travel's screen, or with NVF_START_POOL_PREP=0: at once.
        assert!(wait_over(false, 0.0, None));
        // Under the start's screen: waits for the pool, then is drawn anyway.
        assert!(!wait_over(false, 0.0, Some(0.0)));
        assert!(!wait_over(false, 0.0, Some(MAX_WAIT_SECS - 0.01)));
        assert!(wait_over(false, 0.0, Some(MAX_WAIT_SECS)));
    }

    /// Only the start's own cells wait for the read pool and are counted: not with the aid
    /// off, not behind a travel's screen, and not for a travel sent under the start's.
    #[test]
    fn a_travel_under_the_start_screen_is_not_the_starts() {
        assert!(start_pool_cells(true, true, false));
        assert!(!start_pool_cells(false, true, false), "NVF_START_POOL_PREP=0");
        assert!(!start_pool_cells(true, false, true), "a travel's screen");
        assert!(!start_pool_cells(true, true, true), "a travel under the start's screen");
        assert!(!start_pool_cells(true, false, false), "in play");
    }

    /// The start's line: once, after its cells were counted, with how they came in.
    #[test]
    fn the_start_line_counts_its_cells_once() {
        let mut s = StartPrep { held: 34, ..default() };
        assert_eq!(s.settled_line(), None, "no start cell counted (a travel's screen, or the aid off)");
        let t0 = std::time::Instant::now();
        assert_eq!(s.waited(t0), 0.0);
        assert!(s.waited(t0 + std::time::Duration::from_millis(250)) >= 0.25);
        s.drawn(true);
        s.drawn(true);
        s.drawn(false);
        let line = s.settled_line().unwrap();
        assert!(line.starts_with("world: the start's cells drawn: 2 with their models prepared on the read pool, 1 before"), "{line}");
        assert!(line.contains("34 cells read ahead under the picture hold"), "{line}");
        assert_eq!(s.settled_line(), None, "once");
    }

    /// Issue #1753: with a menu's own `Camera3d` beside the main view's (and the water
    /// reflection's), a world system follows the main view's camera (`fly::WorldCamera`)
    /// instead of stopping or taking the menu's.
    #[test]
    fn billboards_face_the_main_view_beside_a_menu_camera() {
        let mut app = App::new();
        app.add_systems(Update, face_billboards);
        let at = |p: Vec3| GlobalTransform::from(Transform::from_translation(p));
        // The menu's camera, spawned first, 1000 units along game +X (Bevy +X).
        app.world_mut().spawn((Name::new("Lockpicking camera"), Camera3d::default(), at(Vec3::new(1000.0, 0.0, 0.0))));
        // The main view, 1000 units along game +Y (Bevy -Z).
        let main = app.world_mut().spawn((Camera3d::default(), at(Vec3::new(0.0, 0.0, -1000.0)), crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 })).id();
        // The reflection, 1000 units along game -X.
        app.world_mut().spawn((Camera3d::default(), at(Vec3::new(-1000.0, 0.0, 0.0)), crate::reflection::ReflectionCamera));
        let tree = app.world_mut().spawn((TreeBillboard, Transform::from_rotation(Quat::from_rotation_z(-1.0)), GlobalTransform::IDENTITY)).id();
        app.update();
        let r = app.world().get::<Transform>(tree).unwrap().rotation;
        assert!(r.abs_diff_eq(Quat::from_rotation_z(FRAC_PI_2), 1e-5), "faces the main view (game +Y), not the menu's camera: {r:?}");
        // The main view moves; the tree follows it.
        app.world_mut().entity_mut(main).insert(at(Vec3::new(0.0, 0.0, 1000.0)));
        app.update();
        let r = app.world().get::<Transform>(tree).unwrap().rotation;
        assert!(r.abs_diff_eq(Quat::from_rotation_z(-FRAC_PI_2), 1e-5), "follows the main view to game -Y: {r:?}");
    }

    /// A tree billboard already facing a camera that did not move keeps its transform
    /// unchanged, so it does not dirty its cell for transform propagation.
    #[test]
    fn a_still_camera_leaves_tree_billboards_unchanged() {
        #[derive(Resource, Default)]
        struct Turned(usize);
        let mut app = App::new();
        app.init_resource::<Turned>().add_systems(Update, (
            face_billboards,
            |changed: Query<(), (With<TreeBillboard>, Changed<Transform>)>, mut turned: ResMut<Turned>| turned.0 = changed.iter().count(),
        ).chain());
        let at = |p: Vec3| GlobalTransform::from(Transform::from_translation(p));
        let main = app.world_mut().spawn((Camera3d::default(), at(Vec3::new(0.0, 0.0, -1000.0)), crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 })).id();
        let tree = app.world_mut().spawn((TreeBillboard, Transform::IDENTITY, GlobalTransform::IDENTITY)).id();
        app.update();
        app.update();
        assert!(app.world().get::<Transform>(tree).unwrap().rotation.abs_diff_eq(Quat::from_rotation_z(FRAC_PI_2), 1e-5));
        for _ in 0..2 {
            app.update();
            assert_eq!(app.world().resource::<Turned>().0, 0);
        }
        app.world_mut().entity_mut(main).insert(at(Vec3::new(0.0, 0.0, 1000.0)));
        app.update();
        assert_eq!(app.world().resource::<Turned>().0, 1);
    }

    /// The log of `note_other_cameras`: once when streaming first runs beside another
    /// camera, nothing while it stays or while streaming does not run, the count at the end.
    #[test]
    fn other_camera_lines() {
        let mut s = OtherCameras::default();
        assert_eq!(s.step(1, None), None);
        assert_eq!(s.step(2, None), None);
        let opened = s.step(3, Some("Lockpicking camera")).expect("the first frame beside it");
        assert!(opened.starts_with("world: streaming runs with another 3D camera active (Lockpicking camera)"), "{opened}");
        assert_eq!(s.step(4, Some("Lockpicking camera")), None);
        // Streaming held (a loading screen): not counted.
        assert_eq!(s.step(4, Some("Lockpicking camera")), None);
        assert_eq!(s.step(5, Some("Lockpicking camera")), None);
        assert_eq!(s.step(6, None).as_deref(), Some("world: streaming kept running behind another 3D camera (Lockpicking camera): 3 frames"));
        assert_eq!(s.step(7, None), None);
        // A camera beside the main view while streaming never runs logs nothing.
        assert_eq!(s.step(7, Some("Scope camera")), None);
        assert_eq!(s.step(7, None), None);
        // One camera taking over from another with no frame between: each its own count.
        assert!(s.step(8, Some("Terminal camera")).is_some());
        assert_eq!(s.step(9, Some("Terminal camera")), None);
        assert_eq!(s.step(10, Some("Lockpicking camera")).as_deref(), Some("world: streaming kept running behind another 3D camera (Terminal camera): 2 frames"));
        assert_eq!(s.step(11, None).as_deref(), Some("world: streaming kept running behind another 3D camera (Lockpicking camera): 1 frames"));
    }

    #[test]
    fn markers() {
        assert!(is_marker("meshes\\marker_x.nif"));
        assert!(!is_marker("meshes\\clutter\\junk\\can.nif"));
    }

    /// Issue #1620, over a synthetic plugin (`nvf_play::script_draw`'s copy of the rule is
    /// tested the same way): under an enable parent a reference's own Initially Disabled
    /// flag does not count.
    #[test]
    fn under_an_enable_parent_a_references_own_flag_does_not_count() {
        use fnv_esm::raw::{write_subrecords, Subrecord};
        const FLAGGED: u32 = 0x800;
        const DELETED: u32 = 0x20;
        const ON: u32 = 0x301;
        const OFF: u32 = 0x302;
        const CELL: u32 = 0x200;
        let sub = |sig: &[u8; 4], data: Vec<u8>| Subrecord { sig: *sig, data, extended: false };
        let record = |sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]| {
            let payload = write_subrecords(subs);
            [&sig[..], &(payload.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
        };
        let group = |label: [u8; 4], kind: i32, body: &[u8]| [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &kind.to_le_bytes(), &[0; 8], body].concat();
        let refr = |fid: u32, flags: u32, parent: Option<(u32, bool)>| {
            let mut subs = vec![sub(b"NAME", 0x100u32.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])];
            if let Some((p, opposite)) = parent {
                subs.push(sub(b"XESP", [&p.to_le_bytes()[..], &[opposite as u8, 0, 0, 0]].concat()));
            }
            record(b"REFR", flags, fid, &subs)
        };
        // (reference, its flags, its parent, enabled at the start)
        let cases = [
            (ON, 0, None, true),
            (OFF, FLAGGED, None, false),
            (0x310, FLAGGED, Some((ON, false)), true),
            (0x311, FLAGGED, Some((ON, true)), false),
            (0x312, FLAGGED, Some((OFF, false)), false),
            (0x313, FLAGGED, Some((OFF, true)), true),
            (0x314, 0, Some((ON, false)), true),
            (0x315, 0, Some((OFF, false)), false),
            (0x316, 0, Some((OFF, true)), true),
            // Two deep, flagged all the way down.
            (0x317, FLAGGED, Some((0x310, false)), true),
            // Deleted stays out; a parent that does not exist leaves the flag as it was.
            (0x318, DELETED | FLAGGED, Some((ON, false)), false),
            (0x319, FLAGGED, Some((0x999, false)), false),
            (0x31A, 0, Some((0x999, false)), true),
            (0x31B, 0, Some((0x999, true)), false),
        ];
        let refs: Vec<u8> = cases.iter().flat_map(|&(id, flags, parent, _)| refr(id, flags, parent)).collect();
        let label = CELL.to_le_bytes();
        let children = group(label, 6, &group(label, 9, &refs));
        let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", b"TestCell\0".to_vec())]);
        let cells = group(*b"CELL", 0, &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat())));
        let hedr = [&1.34f32.to_le_bytes()[..], &0i32.to_le_bytes(), &0x800u32.to_le_bytes()].concat();
        let bytes = [record(b"TES4", 1, 0, &[sub(b"HEDR", hedr)]), cells].concat();
        let plugins = vec![EsmPlugin::parse(&bytes, "Test.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let schema = EsmSchema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
        for (id, _, _, want) in cases {
            let p = placement(&lo, &schema, id).unwrap();
            assert_eq!(enabled(&lo, &schema, &p, 0), want, "0x{id:X}");
        }
    }

    #[test]
    fn bos_engine_session_seed_varies_chance_r_while_keeping_chance_s_identical() {
        use fnv_esm::raw::{write_subrecords, Subrecord};
        let sub = |sig: &[u8; 4], data: Vec<u8>| Subrecord { sig: *sig, data, extended: false };
        let z = |s: &str| {
            let mut v = s.as_bytes().to_vec();
            v.push(0);
            v
        };
        let record = |sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]| {
            let payload = write_subrecords(subs);
            [&sig[..], &(payload.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
        };
        let group = |label: [u8; 4], kind: i32, body: &[u8]| [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &kind.to_le_bytes(), &[0; 8], body].concat();
        let hedr = [&1.34f32.to_le_bytes()[..], &0i32.to_le_bytes(), &0x800u32.to_le_bytes()].concat();
        let stat_body = [
            record(b"STAT", 0, 0x1001, &[sub(b"EDID", z("SynOrigA"))]),
            record(b"STAT", 0, 0x1002, &[sub(b"EDID", z("SynSwapA"))]),
            record(b"STAT", 0, 0x1003, &[sub(b"EDID", z("SynOrigB"))]),
            record(b"STAT", 0, 0x1004, &[sub(b"EDID", z("SynSwapB"))]),
        ]
        .concat();
        let bytes = [record(b"TES4", 1, 0, &[sub(b"HEDR", hedr)]), group(*b"STAT", 0, &stat_body)].concat();
        let plugins = vec![EsmPlugin::parse(&bytes, "TestBos.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let sources = vec![(
            "baseobjectswapper\\session_test.ini".to_string(),
            "[Forms]\nSynOrigA|SynSwapA|NONE|chanceS(50)\nSynOrigB|SynSwapB|NONE|chanceR(50)\n".to_string(),
        )];

        // Two engine sessions with distinct seeds vary chanceR while keeping chanceS identical.
        let s1 = build_bos_table(&lo, &sources, 0xA1B2_C3D4_E5F6_0708);
        let s2 = build_bos_table(&lo, &sources, 0x0102_0304_0506_0708);
        for rid in 0x0001_0000u32..0x0001_0100 {
            assert_eq!(s1.resolve(rid, 0x1001), s2.resolve(rid, 0x1001), "chanceS must stay identical");
        }
        let diffs = (0x0001_0000u32..0x0001_0100)
            .filter(|&rid| s1.resolve(rid, 0x1003) != s2.resolve(rid, 0x1003))
            .count();
        assert!(diffs > 20, "chanceR should vary across session seeds, got {diffs} diffs");

        // Fresh OS-backed session seeds should not collapse to a fixed constant.
        let seeds: HashSet<u64> = (0..4).map(|_| bos_session_seed()).collect();
        assert!(seeds.len() > 1, "expected varying session seeds, got {seeds:?}");
    }

    /// Issue #2549: a placed torch's glow card (`oasistorch01.nif`'s `GlowMesh`, a
    /// mode-4 `NiBillboardNode` whose one quad lies in its XY plane, +z its face) faces a
    /// camera at its side after one update, whether its shapes are baked (the static
    /// path, `BakedBillboards`) or its node hierarchy spawned (`Rig::spawn_playing`, the
    /// animated path); drawn unturned it is a flat sheet seen edge-on.
    #[test]
    fn placed_billboard_nodes_face_a_camera_at_their_side() {
        use nvf_assets::model::{BakedBillboards, Billboard, ModelNode, PreparedModel, PreparedShape, Rig, RigNode};
        let glow_at = Vec3::new(0.0, 0.0, 106.3);
        let node = |name: &str, parent, local, billboard| ModelNode { name: Some(name.into()), parent, local, billboard, addon: None, damage_stages: None };
        let nodes = vec![node("Torch", None, Affine3A::IDENTITY, None), node("GlowMesh", Some(0), Affine3A::from_translation(glow_at), Some(4))];
        let own = Affine3A::from_translation(Vec3::new(0.0, 0.0, 9.2));
        let shape = |node: Option<usize>, transform| PreparedShape {
            mesh: Handle::default(),
            material: Handle::default(),
            transform,
            name: None,
            ancestors: Vec::new(),
            node,
            own,
            cpu: None,
            segments: Vec::new(),
            external_emittance: false,
            look: nvf_assets::material_anim::Look::stored([0.0; 3], 1.0, 1.0),
            missing_diffuse: None,
            block: 0,
        };
        let glow = shape(Some(1), Affine3A::from_translation(glow_at) * own);
        let stick = shape(Some(0), Affine3A::IDENTITY);
        let model = PreparedModel { nodes, shapes: vec![glow, stick], ..Default::default() };
        let rt = Affine3A::from_translation(Vec3::new(500.0, -300.0, 20.0));
        let world_of = |w: &World, e: Entity| {
            let mut m = Affine3A::IDENTITY;
            let mut at = Some(e);
            while let Some(p) = at {
                m = w.get::<Transform>(p).unwrap().compute_affine() * m;
                at = w.get::<ChildOf>(p).map(ChildOf::parent);
            }
            m
        };
        // A camera level with the glow, off to the reference's side and a little behind.
        let eye = Vec3::from(rt.translation) + Vec3::new(800.0, -200.0, 115.5);
        let faces = |w: &World, e: Entity| {
            let m = world_of(w, e);
            let to_eye = (eye - Vec3::from(m.translation)).normalize();
            m.transform_vector3(Vec3::Z).normalize().dot(to_eye)
        };

        for animated in [false, true] {
            let mut app = App::new();
            app.add_systems(Update, face_camera);
            app.world_mut().spawn((Camera3d::default(), Transform::from_translation(eye), crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }));
            let cell = app.world_mut().spawn(Transform::IDENTITY).id();
            let card = if animated {
                let rig_node = |name: &str, parent, local: Vec3, billboard| RigNode { name: Some(name.into()), parent, local: Transform::from_translation(local), hidden: false, billboard };
                let rig = Rig { nodes: vec![rig_node("Torch", None, Vec3::ZERO, None), rig_node("GlowMesh", Some(0), glow_at, Some(4))], shapes: Vec::new(), idle: None, door: None, groups: Vec::new(), effect: None };
                let (_, _, by_name) = rig.spawn_nodes(&mut app.world_mut().commands(), cell, Transform::from_matrix(Mat4::from(rt)), "test", 0.0);
                app.world_mut().flush();
                by_name["GlowMesh"]
            } else {
                let mut boards = BakedBillboards::default();
                let mut commands = app.world_mut().commands();
                let (board, below) = boards.place(&mut commands, cell, &model, rt, &model.shapes[0], RefVisual(7)).expect("the glow is under a billboard node");
                assert!(boards.place(&mut commands, cell, &model, rt, &model.shapes[1], ()).is_none(), "the torch's stick stays baked");
                let card = commands.spawn((Transform::from_matrix(Mat4::from(below)), ChildOf(board))).id();
                app.world_mut().flush();
                // Where the file puts it, before any turn.
                let at = Vec3::from(world_of(app.world(), card).translation);
                // The holder, a child of the cell, is the reference's: moved.rs and
                // destroyed.rs move and redraw it with the rest of the reference.
                let holder = app.world().get::<ChildOf>(board).unwrap().parent();
                assert_eq!((app.world().get::<RefVisual>(holder).map(|v| v.0), app.world().get::<ChildOf>(holder).map(|c| c.parent())), (Some(7), Some(cell)));
                assert!((at - Vec3::from((rt * model.shapes[0].transform).translation)).length() < 1e-3, "{at}");
                card
            };
            let node = if animated { card } else { app.world().get::<ChildOf>(card).unwrap().parent() };
            assert_eq!(app.world().get::<Billboard>(node).map(|b| b.mode), Some(4), "animated {animated}");
            assert!(faces(app.world(), card) < 0.2, "unturned, the card lies flat (seen edge-on): {}", faces(app.world(), card));
            app.update();
            assert!(faces(app.world(), card) > 0.999, "animated {animated}: the card faces the camera: {}", faces(app.world(), card));
        }
    }

    /// Issue #2549: the torch's flame stands along +z under a mode-5 node; turned, it
    /// stays upright (about its z) with its +y towards the camera.
    #[test]
    fn a_mode_5_billboard_turns_about_its_z() {
        let mut app = App::new();
        app.add_systems(Update, face_camera);
        let eye = Vec3::new(300.0, -300.0, 500.0);
        app.world_mut().spawn((Camera3d::default(), Transform::from_translation(eye), crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }));
        let holder = app.world_mut().spawn(Transform::IDENTITY).id();
        let flame = app.world_mut().spawn((Transform::from_xyz(0.0, 0.0, 100.0), nvf_assets::model::Billboard { mode: 5, bind: Quat::IDENTITY }, ChildOf(holder))).id();
        app.update();
        let r = app.world().get::<Transform>(flame).unwrap().rotation;
        assert!((r * Vec3::Z).abs_diff_eq(Vec3::Z, 1e-5), "upright: {}", r * Vec3::Z);
        let level = Vec3::new(eye.x, eye.y, 0.0).normalize();
        assert!((r * Vec3::Y).dot(level) > 0.9999, "+y to the camera: {}", r * Vec3::Y);
    }
}

#[cfg(test)]
mod blend_census_tests {
    use super::*;
    use crate::mesh_terrain_blend::{reason, BaseType};

    /// #2661 round 7: a SCOL reference is taken into the census as a STAT is, under the
    /// same reference rules and labelled by its base type; other record types are not.
    /// Synthetic index and records; no game data.
    #[test]
    fn scol_references_get_the_census_of_a_stat_and_other_records_none() {
        let mut world = crate::outfits::tests::empty_world_data();
        let mut index = WorldIndex::default();
        index.cells.insert(0x10, fnv_world::Cell { form_id: 0x10, world: Some(0), grid: Some((-16, -2)), ..default() });
        index.cells.insert(0x11, fnv_world::Cell { form_id: 0x11, data_flags: 1, ..default() });
        index.refs.insert(0x100, fnv_world::index::RefEntry { cell: 0x10, sig: *b"REFR", persistent: false });
        index.refs.insert(0x101, fnv_world::index::RefEntry { cell: 0x11, sig: *b"REFR", persistent: false });
        world.index = Box::leak(Box::new(index));
        let placement = |imposter: bool| Placement {
            form_id: 0x100, sig: *b"REFR", flags: 0, base: Some(0x200), position: [0.0; 3], rotation: [0.0; 3], scale: 1.0,
            enable_parent: None, open_by_default: false, teleport: false, emittance: None, imposter, primitive: false,
        };
        let base = |sig: [u8; 4]| BaseObject {
            form_id: 0x200, sig, edid: None, model: Some("scol/synthetic_road.nif".into()), world_model: None, world_alt_textures: vec![], worn_only: false, flags: 0, bounds: None,
        };
        let model = PreparedModel::default();
        let path = "scol/synthetic_road.nif";
        let scol = census_of(&world, &placement(false), 0x100, &base(*b"SCOL"), path, &model, false).expect("a SCOL is tracked");
        assert_eq!((scol.base_type, scol.reasons, scol.reference, scol.base), (BaseType::Scol, 0, 0x100, 0x200));
        assert_eq!(&*scol.model, path);
        let stat = census_of(&world, &placement(false), 0x100, &base(*b"STAT"), path, &model, false).expect("a STAT is tracked");
        assert_eq!((stat.base_type, stat.reasons), (BaseType::Stat, 0));
        // The same reference rules: an imposter, an interior, a stored pose.
        assert_eq!(census_of(&world, &placement(true), 0x100, &base(*b"SCOL"), path, &model, false).unwrap().reasons, reason::DISTANT);
        assert_eq!(census_of(&world, &placement(false), 0x101, &base(*b"SCOL"), path, &model, false).unwrap().reasons, reason::WORLD);
        assert_eq!(census_of(&world, &placement(false), 0x100, &base(*b"SCOL"), path, &model, true).unwrap().reasons, reason::POSED);
        for sig in [*b"MSTT", *b"DOOR", *b"ACTI", *b"TREE", *b"MISC"] {
            assert!(census_of(&world, &placement(false), 0x100, &base(sig), path, &model, false).is_none(), "{}", String::from_utf8_lossy(&sig));
        }
    }
}
