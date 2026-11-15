//! WorldStreamingPlugin: load the cells around the camera and spawn their terrain and
//! references; unload cells that fall out of range.

use crate::fade::CellFade;
use crate::terrain::{prepare_terrain, spawn_terrain, TerrainCtx, TerrainMaterials, TerrainPrep};
use crate::water::{water_material, WaterCtx};
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
use std::path::Path;

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

/// Open the plugins `names` (load order) for the game: from the mods folder `mods` when it
/// has the file, else from `data_dir` (`fnv_install::plugins::plugin_path`). A plugin that cannot
/// be read or parsed (a truncated or broken mod) is left out, and then every plugin the
/// load order would refuse: a missing master, a master that loads after it, a second
/// copy, past the 255th (`fnv_esm::loadorder::drop_unloadable`). The game goes on without
/// them; a line for each is printed now (the log is not up yet) and kept for the log.
pub fn open_plugins(data_dir: &Path, mods: Option<&Path>, names: &[String]) -> OpenedPlugins {
    let mut dropped = Vec::new();
    let mut plugins = Vec::with_capacity(names.len());
    for n in names {
        // The file whatever its case on disk (a case-sensitive copy may hold
        // `deadmoney.esm`; #995); the plugin keeps the name the load order gives it.
        let path = fnv_install::plugins::plugin_path(data_dir, mods, n);
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

impl WorldData {
    pub fn open(schema_dir: &Path, opened: OpenedPlugins, world: &str) -> Result<WorldData, String> {
        let schema: &'static EsmSchema =
            Box::leak(Box::new(EsmSchema::load_dir(schema_dir).map_err(|e| format!("{}: {e}", schema_dir.display()))?));
        let OpenedPlugins { plugins, dropped: dropped_plugins } = opened;
        let plugins: &'static [EsmPlugin] = Box::leak(plugins.into_boxed_slice());
        let lo: &'static LoadOrder<'static> = Box::leak(Box::new(LoadOrder::new(plugins).map_err(|e| e.to_string())?));
        let index: &'static WorldIndex = Box::leak(Box::new(WorldIndex::build(lo, schema)));
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

    /// The LAND of the grid squares around `g` ([`crate::terrain::NEIGHBOURS`]): a cell's
    /// texture layers are stitched to theirs along its edges.
    pub fn neighbour_lands(&self, g: (i32, i32)) -> [Option<u32>; 4] {
        crate::terrain::NEIGHBOURS.map(|(dx, dy)| self.grid_cell((g.0 + dx, g.1 + dy)).and_then(|c| c.land))
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
    /// Seamless doors (step 3): the place the player left through a door, its cells kept
    /// spawned and hidden ([`travel`]); going back draws them again at once.
    pub parked: Option<Parked>,
    /// Seamless doors: an interior to spawn ahead, hidden, as the place parked, so that its
    /// door opens onto it the first time too (`door_ahead.rs` asks for it outdoors near
    /// its door, once its models are prepared).
    pub prespawn_want: Option<u32>,
    /// That interior being spawned, a few references a frame.
    pub prespawning: Option<(u32, CellSpawn)>,
    /// Seamless doors, indoors: the worldspace's squares around a door's arrival to spawn
    /// ahead, hidden, as the place parked (a new game starts in Doc Mitchell's house with
    /// nothing outdoors kept). `door_ahead.rs` asks once they are read ahead.
    pub prespawn_outdoors: Option<Vec<(i32, i32)>>,
    /// Those squares being spawned: done, the one under way, those still to start.
    pub outdoors_spawning: Option<OutdoorsSpawn>,
}

/// The outdoors spawned ahead from indoors ([`Streamer::prespawn_outdoors`]).
pub struct OutdoorsSpawn {
    pub want: Vec<(i32, i32)>,
    done: HashMap<(i32, i32), Entity>,
    now: Option<((i32, i32), CellSpawn)>,
    ms: f64,
    frames: u32,
}

/// On the body of an actor drawn in the place parked (`residents.rs`): a door showing
/// that place draws it too (`door_portal.rs`).
#[derive(Component)]
pub struct ParkedActor;

/// The cells of a place left through a door, kept spawned (seamless doors step 3,
/// `nvf_play::seamless`). Hidden; still `RenderCell`s, so what nvf-play draws into them
/// (dropped items, script-drawn references) stays in step with the simulation.
pub struct Parked {
    /// The interior, or `None` for the worldspace's exterior.
    pub interior: Option<u32>,
    pub world: u32,
    /// The cells as they were in `Streamer::loaded`.
    pub cells: HashMap<(i32, i32), Entity>,
}

impl Parked {
    /// Whether this is the place `interior` (or the exterior of `world`).
    pub fn is(&self, interior: Option<u32>, world: u32) -> bool {
        self.interior == interior && (interior.is_some() || self.world == world)
    }
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

pub fn setup_root(mut commands: Commands) {
    commands.spawn((Name::new("World"), WorldRoot, game_to_bevy(), Visibility::default()));
}

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
    (actor_world, sim, mut ahead): (Option<Res<crate::actors::ActorWorld>>, Option<Res<nvf_play::sim::SimState>>, ResMut<crate::readahead::ReadAhead>),
    mut actor_parts: ResMut<ActorParts>,
    loading: Option<Res<nvf_play::loading::Loading>>,
    children: Query<&Children>,
) {
    let (Ok(root), Ok(cam)) = (root.single(), camera.single()) else { return };
    streamer.stats.frames += 1;
    let before = (streamer.stats.actors.equipment_spawns, streamer.stats.actors.record_spawns);
    let frame_start = std::time::Instant::now();
    let behind_screen = loading.is_some_and(|l| l.unseen_frames());
    let mut actors = actor_world.as_deref().map(|w| (w, &mut *actor_parts, sim.as_deref().map(|s| &s.world), &mut *ahead));
    despawn_dying(&mut commands, &mut streamer.dying, &children);
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
        if !behind_screen {
            let tile = terrain.settings.tile;
            let s = &mut *streamer;
            spawn_outdoors_ahead(&mut commands, &world, s, root, tile, &mut models, &mut terrain, &mut water, &mut trees, &mut grass, &mut emittance, &mut actors);
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
    read_ahead(&world, &mut streamer, centre, r + 1, tile, &mut models);

    // Before the start square is in (or behind a loading screen) cells are drawn whole in
    // the frame they load, as before; after it, over frames within a time budget, hidden
    // until all their references are in, then faded in (module docs of `CellSpawn`).
    let budgeted = streamer.settled && !behind_screen;
    let deadline = frame_start + if budgeted { SPAWN_BUDGET } else { LOADING_SCREEN_FRAME };
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
        while sp.next < sp.refs.len() && !(budgeted && std::time::Instant::now() >= deadline) {
            let r = sp.refs[sp.next];
            sp.next += 1;
            spawn_ref(&mut commands, &world, sp.entity, sp.origin, r, &mut models, &mut water, &mut trees, &mut emittance, &mut actors, s);
        }
        sp.ms += t0.elapsed().as_secs_f64() * 1000.0;
        sp.frames += 1;
        if sp.next < sp.refs.len() {
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
        let forced = !ready && (!budgeted || now - since >= MAX_WAIT_SECS);
        if !ready && !forced {
            // Nearer cells first: a farther one does not jump the queue.
            break;
        }
        streamer.ahead.waiting.remove(&c);
        streamer.stats.cells_waited += usize::from(now > since);
        streamer.stats.cells_forced += usize::from(forced && budgeted);
        let t0 = std::time::Instant::now();
        let prep = streamer.ahead.listed.remove(&c).and_then(|a| a.terrain);
        let s = &mut *streamer;
        let (e, refs, origin) = begin_cell(&mut commands, &world, CellKey::Exterior(c), prep, tile, &mut models, &mut terrain, &mut water, &mut grass, budgeted, s);
        commands.entity(root).add_child(e);
        let mut sp = CellSpawn { entity: e, refs, next: 0, origin, ms: 0.0, frames: 1 };
        while sp.next < sp.refs.len() && !(budgeted && std::time::Instant::now() >= deadline) {
            let r = sp.refs[sp.next];
            sp.next += 1;
            spawn_ref(&mut commands, &world, e, origin, r, &mut models, &mut water, &mut trees, &mut emittance, &mut actors, s);
        }
        sp.ms = t0.elapsed().as_secs_f64() * 1000.0;
        if sp.next < sp.refs.len() {
            streamer.spawning.insert(c, sp);
        } else {
            finish_cell(&mut commands, &mut streamer, c, sp, fading);
        }
    }
    // Seamless doors: the interior asked for ahead, spawned hidden within what is left of
    // the frame's budget, becomes the place parked once all its references are in.
    if budgeted {
        let s = &mut *streamer;
        let want_id = s.prespawn_want.filter(|id| !s.parked.as_ref().is_some_and(|p| p.interior == Some(*id)));
        if s.prespawning.as_ref().is_some_and(|(id, _)| Some(*id) != want_id) {
            if let Some((_, sp)) = s.prespawning.take() {
                retire(&mut commands, &mut s.dying, sp.entity);
            }
        }
        if let (Some(id), None) = (want_id, s.prespawning.as_ref()) {
            let (e, refs, origin) = begin_cell(&mut commands, &world, CellKey::Interior(id), None, tile, &mut models, &mut terrain, &mut water, &mut grass, true, s);
            commands.entity(root).add_child(e);
            s.prespawning = Some((id, CellSpawn { entity: e, refs, next: 0, origin, ms: 0.0, frames: 0 }));
            info!("seamless doors: interior {id:08X} spawned ahead, hidden");
        }
        let prespawn_deadline = std::time::Instant::now() + PRESPAWN_BUDGET;
        if let Some((id, mut sp)) = s.prespawning.take() {
            let t0 = std::time::Instant::now();
            while sp.next < sp.refs.len() && std::time::Instant::now() < prespawn_deadline {
                let r = sp.refs[sp.next];
                sp.next += 1;
                spawn_ref(&mut commands, &world, sp.entity, sp.origin, r, &mut models, &mut water, &mut trees, &mut emittance, &mut actors, s);
            }
            sp.ms += t0.elapsed().as_secs_f64() * 1000.0;
            sp.frames += 1;
            if sp.next < sp.refs.len() {
                s.prespawning = Some((id, sp));
            } else {
                if let Some(old) = s.parked.take() {
                    old.cells.into_values().for_each(|e| retire(&mut commands, &mut s.dying, e));
                }
                info!("seamless doors: interior {id:08X} is in, hidden ({} references over {} frames, {:.1} ms): its door opens onto it", sp.refs.len(), sp.frames, sp.ms);
                s.parked = Some(Parked { interior: Some(id), world: world.world, cells: HashMap::from([((i32::MIN, i32::MIN), sp.entity)]) });
            }
        }
    }
    if !streamer.settled && want.iter().all(|c| streamer.loaded.contains_key(c)) {
        streamer.settled = true;
        streamer.arrival = None;
    }
    crate::profile::actor_spawns(streamer.stats.actors.equipment_spawns - before.0, streamer.stats.actors.record_spawns - before.1);
}

/// Seamless doors, indoors: spawn the squares asked for ahead ([`Streamer::prespawn_outdoors`]),
/// hidden, within [`PRESPAWN_BUDGET`] a frame; all in, they are the place parked.
#[allow(clippy::too_many_arguments)]
fn spawn_outdoors_ahead(
    commands: &mut Commands,
    world: &WorldData,
    s: &mut Streamer,
    root: Entity,
    tile: f32,
    models: &mut ModelCtx,
    terrain: &mut TerrainMaterials,
    water: &mut WaterCtx,
    trees: &mut crate::tree::TreeCtx,
    grass: &mut crate::grass::GrassCtx,
    emittance: &mut crate::emittance::EmittanceState,
    actors: &mut Option<ActorDraw<'_, '_>>,
) {
    let want = s.prespawn_outdoors.clone().filter(|_| !s.parked.as_ref().is_some_and(|p| p.interior.is_none()));
    let stale = match (&s.outdoors_spawning, &want) {
        (Some(o), Some(w)) => &o.want != w,
        (Some(_), None) => true,
        _ => false,
    };
    if stale {
        if let Some(o) = s.outdoors_spawning.take() {
            for e in o.done.into_values().chain(o.now.map(|(_, sp)| sp.entity)) {
                retire(commands, &mut s.dying, e);
            }
        }
    }
    let Some(want) = want else { return };
    if s.outdoors_spawning.is_none() {
        info!("seamless doors: the outdoors around a door ({} squares) spawned ahead, hidden", want.len());
        s.outdoors_spawning = Some(OutdoorsSpawn { want: want.clone(), done: HashMap::new(), now: None, ms: 0.0, frames: 0 });
    }
    let deadline = std::time::Instant::now() + PRESPAWN_BUDGET;
    let t0 = std::time::Instant::now();
    let mut o = s.outdoors_spawning.take().expect("outdoors");
    o.frames += 1;
    while std::time::Instant::now() < deadline {
        if o.now.is_none() {
            let Some(&c) = want.iter().find(|c| !o.done.contains_key(c)) else { break };
            let prep = s.ahead.listed.get_mut(&c).and_then(|a| a.terrain.take());
            let (e, refs, origin) = begin_cell(commands, world, CellKey::Exterior(c), prep, tile, models, terrain, water, grass, true, s);
            commands.entity(root).add_child(e);
            o.now = Some((c, CellSpawn { entity: e, refs, next: 0, origin, ms: 0.0, frames: 0 }));
        }
        let Some((c, mut sp)) = o.now.take() else { break };
        while sp.next < sp.refs.len() && std::time::Instant::now() < deadline {
            let r = sp.refs[sp.next];
            sp.next += 1;
            spawn_ref(commands, world, sp.entity, sp.origin, r, models, water, trees, emittance, actors, s);
        }
        if sp.next < sp.refs.len() {
            o.now = Some((c, sp));
            break;
        }
        o.done.insert(c, sp.entity);
    }
    o.ms += t0.elapsed().as_secs_f64() * 1000.0;
    if o.done.len() == want.len() {
        info!("seamless doors: the outdoors around a door is in, hidden ({} squares over {} frames, {:.1} ms): its door opens onto it", want.len(), o.frames, o.ms);
        if let Some(old) = s.parked.take() {
            old.cells.into_values().for_each(|e| retire(commands, &mut s.dying, e));
        }
        s.parked = Some(Parked { interior: None, world: world.world, cells: o.done });
        s.prespawn_outdoors = None;
    } else {
        s.outdoors_spawning = Some(o);
    }
}

/// Main-thread time a frame spends spawning an interior ahead for seamless doors
/// [engineering choice: beside [`SPAWN_BUDGET`], a saloon's few hundred references take a
/// second or so].
const PRESPAWN_BUDGET: std::time::Duration = std::time::Duration::from_millis(2);

/// Main-thread time a frame spends spawning cells once the start square is in
/// [engineering choice: about a fifth of a 60 fps frame; a town cell's few thousand
/// references take a few frames, which the cell fade and the read-ahead ring cover].
const SPAWN_BUDGET: std::time::Duration = std::time::Duration::from_millis(3);

/// Seconds a cell waits for its read-ahead before it is drawn anyway, preparing what is
/// missing on the main thread [engineering choice].
const MAX_WAIT_SECS: f64 = 1.5;

/// Direct children of retired cells despawned per frame (each with everything below it)
/// [engineering choice: a town cell's references go in a few frames].
const DESPAWN_PER_FRAME: usize = 400;

/// A cell whose references are being spawned over frames (after the start square is in:
/// [`stream`]). Its entity is hidden until the last reference is in; then it joins
/// `Streamer::loaded` and fades in as a cell loaded in one frame did, so nothing reads it
/// half drawn (the LOD stands in for it meanwhile).
#[derive(Debug)]
pub struct CellSpawn {
    entity: Entity,
    refs: Vec<u32>,
    /// Index of the next reference to spawn.
    next: usize,
    /// The cell's origin (game space).
    origin: Vec3,
    /// Main-thread time spent on it so far, and the frames it took.
    ms: f64,
    frames: u32,
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
fn despawn_dying(commands: &mut Commands, dying: &mut Vec<Entity>, children: &Query<&Children>) {
    let mut left = DESPAWN_PER_FRAME;
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
    /// Cells read ahead for a load door's destination outdoors (seamless doors,
    /// `door_ahead.rs`): kept whatever the ring around the camera ([`pin_cells`]).
    pinned: HashSet<(i32, i32)>,
}

impl ReadAhead {
    /// Cells pinned for doors, and how many of them are read with their models prepared.
    pub fn pinned_ready(&self) -> (usize, usize) {
        (self.pinned.len(), self.pinned.iter().filter(|c| self.listed.get(c).is_some_and(|a| a.ready)).count())
    }
}

/// Start reading cell `c` ahead on a worker thread, unless it is drawn, being read or read.
fn start_cell(world: &WorldData, streamer: &mut Streamer, c: (i32, i32), tile: f32) {
    let Streamer { ahead, loaded, spawning, hide, .. } = streamer;
    if loaded.contains_key(&c) || spawning.contains_key(&c) || ahead.listing.contains_key(&c) || ahead.listed.contains_key(&c) {
        return;
    }
    let Some(cell) = world.grid_cell(c) else { return };
    let mut refs = cell.refs.clone();
    refs.extend(world.persistent_by_grid.get(&c).into_iter().flatten());
    let land = cell.land;
    let around = world.neighbour_lands(c);
    let (lo, schema, hide) = (world.lo, world.schema, hide.clone());
    let task = nvf_assets::read_pool::read_pool().spawn(async move {
        CellAhead {
            models: cell_models(lo, schema, &refs, &hide),
            terrain: land.and_then(|l| prepare_terrain(lo, schema, l, c, tile, around)),
            ready: false,
        }
    });
    ahead.listing.insert(c, task);
}

/// Seamless doors (step 1): read ahead `cells`, the squares a door's travel arrives
/// among, and keep them read until other cells are pinned (`door_ahead.rs`). Runs
/// indoors too, where [`stream`] reads nothing ahead. Their models are prepared as soon
/// as they are listed, and asked again each call so they keep their place in the cache.
pub fn pin_cells(world: &WorldData, streamer: &mut Streamer, cells: &[(i32, i32)], tile: f32, models: &mut ModelCtx) {
    if streamer.ahead.world != world.world {
        streamer.ahead = ReadAhead { world: world.world, ..default() };
    }
    streamer.ahead.pinned = cells.iter().copied().collect();
    for &c in cells {
        start_cell(world, streamer, c, tile);
    }
    take_listed(&mut streamer.ahead, models);
    for c in cells {
        if let Some(a) = streamer.ahead.listed.get_mut(c).filter(|a| !a.ready) {
            a.ready = models.models_ready(a.models.iter().map(|(p, s)| (p.as_str(), s.as_slice())));
        }
    }
}

/// Start reading ahead the cells within `ring` steps of `centre` that are not drawn or
/// read yet; take in the readings done and send their models to be prepared; forget
/// cells far beyond the ring.
fn read_ahead(world: &WorldData, streamer: &mut Streamer, centre: (i32, i32), ring: i32, tile: f32, models: &mut ModelCtx) {
    if streamer.ahead.world != world.world {
        streamer.ahead = ReadAhead { world: world.world, ..default() };
    }
    {
        let ahead = &mut streamer.ahead;
        let steps = |c: &(i32, i32)| (c.0 - centre.0).abs().max((c.1 - centre.1).abs());
        let pinned = &ahead.pinned;
        ahead.listing.retain(|c, _| steps(c) <= ring + 1 || pinned.contains(c));
        ahead.listed.retain(|c, _| steps(c) <= ring + 1 || pinned.contains(c));
        ahead.waiting.retain(|c, _| steps(c) <= ring);
    }
    for dx in -ring..=ring {
        for dy in -ring..=ring {
            start_cell(world, streamer, (centre.0 + dx, centre.1 + dy), tile);
        }
    }
    take_listed(&mut streamer.ahead, models);
}

/// Take in the cells read on worker threads and send their models to be prepared.
fn take_listed(ahead: &mut ReadAhead, models: &mut ModelCtx) {
    let mut done = Vec::new();
    ahead.listing.retain(|c, task| match check_ready(task) {
        Some(a) => {
            done.push((*c, a));
            false
        }
        None => true,
    });
    for (c, mut a) in done {
        a.ready = models.models_ready(a.models.iter().map(|(p, s)| (p.as_str(), s.as_slice())));
        ahead.listed.insert(c, a);
    }
}

/// Whether the cell `c`'s models are all prepared, so it can be drawn without reading a
/// model file on this thread. A grid square without a cell has nothing to read.
fn cell_ready(world: &WorldData, ahead: &mut ReadAhead, c: (i32, i32), models: &mut ModelCtx) -> bool {
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
/// models (`spawn_model`): what a cell's read-ahead prepares. Runs on a worker thread.
pub(crate) fn cell_models(lo: &LoadOrder, schema: &EsmSchema, refs: &[u32], hide: &[String]) -> Vec<(String, Vec<AltTexture>)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for &r in refs {
        let Some(p) = placement(lo, schema, r) else { continue };
        if p.imposter || !p.draws_base_model() || !enabled(lo, schema, &p, 0) {
            continue;
        }
        let Some(base) = p.base.and_then(|b| base_object(lo, schema, b)) else { continue };
        if !DRAWN.contains(&&base.sig) || base.is_speedtree() {
            continue;
        }
        let Some(path) = base.world_model_path() else { continue };
        if script_marker(base.form_id).unwrap_or_else(|| is_marker(&path)) || hide.iter().any(|h| path.contains(h.as_str())) {
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
) {
    let free = !streamer.settled || loading.is_some_and(|l| l.unseen_frames());
    models.take_fetched(time.elapsed_secs_f64(), (!free).then_some(TAKE_IN_BUDGET));
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
pub fn report(mut exit: MessageReader<AppExit>, streamer: Res<Streamer>, models: Res<nvf_assets::ModelCache>, late: Res<LateMeshes>) {
    if exit.read().last().is_none() {
        return;
    }
    let s = &streamer.stats;
    info!(
        "streaming summary: {} cells loaded ({:.0} ms on the main thread), spread over {} frames; {} waited for their read-ahead, {} drawn before it was done; {} models cached, {} in flight; {} late-mesh marks",
        s.cells_loaded, s.load_ms, s.spawn_frames, s.cells_waited, s.cells_forced, models.held(), models.in_flight(), late.marked
    );
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
    portal: Query<&GlobalTransform, (With<crate::door_portal::PortalCamera>, Without<TreeBillboard>)>,
) {
    let Ok(cam) = camera.single() else { return };
    let eye = bevy_to_game(cam.translation());
    // Seamless doors: a billboard of the place a door shows faces the door's camera.
    let door_eye = portal.iter().next().map(|g| bevy_to_game(g.translation()));
    for (mut t, g) in &mut trees {
        let at = bevy_to_game(g.translation());
        let eye = match door_eye {
            Some(d) if d.distance_squared(at) < eye.distance_squared(at) => d,
            _ => eye,
        };
        let d = eye - at;
        if d.x * d.x + d.y * d.y > 1e-6 {
            t.rotation = Quat::from_rotation_z(d.y.atan2(d.x));
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
    keeps: Option<Res<nvf_play::seamless::TravelKeeps>>,
) {
    let Some(t) = msgs.read().last().copied() else { return };
    // `worldspace::switch` has run before: `world.world` is the destination's.
    let keep = keeps.is_some_and(|k| k.keeps(&t));
    if let Some((_, sp)) = streamer.prespawning.take() {
        // Not finished before the travel: spawned again if still wanted.
        let s = &mut *streamer;
        retire(&mut commands, &mut s.dying, sp.entity);
    }
    streamer.prespawn_want = None;
    streamer.prespawn_outdoors = None;
    if let Some(o) = streamer.outdoors_spawning.take() {
        let s = &mut *streamer;
        for e in o.done.into_values().chain(o.now.map(|(_, sp)| sp.entity)) {
            retire(&mut commands, &mut s.dying, e);
        }
    }
    let back = streamer.parked.take_if(|p| p.is(t.interior, world.world));
    for (_, sp) in streamer.spawning.drain() {
        commands.entity(sp.entity).despawn();
    }
    let left: HashMap<(i32, i32), Entity> = streamer.loaded.drain().collect();
    if keep {
        // Seamless doors (step 3): the place left stays, hidden; a place parked before
        // that is not the destination goes.
        if let Some(old) = streamer.parked.take() {
            old.cells.into_values().for_each(|e| retire(&mut commands, &mut streamer.dying, e));
        }
        for &e in left.values() {
            commands.entity(e).insert(Visibility::Hidden);
        }
        info!("world: the place left stays loaded and hidden ({} cells)", left.len());
        streamer.parked = Some(Parked { interior: streamer.interior, world: world.world, cells: left });
    } else {
        left.into_values().for_each(|e| {
            commands.entity(e).despawn();
        });
        if let Some(old) = streamer.parked.take() {
            old.cells.into_values().for_each(|e| retire(&mut commands, &mut streamer.dying, e));
        }
    }
    despawn_all_dying(&mut commands, &mut streamer.dying);
    // The destination appears at once, as after a loading screen.
    streamer.fades.clear();
    streamer.settled = false;
    streamer.interior = t.interior;
    streamer.arrival = t.interior.is_none().then(|| cell_of(t.feet[0], t.feet[1]));
    let back_out = back.as_ref().is_some_and(|p| p.interior.is_none());
    if let Some(p) = back {
        // Going back to the place parked: drawn again as it was.
        info!("world: back to the place left: {} cells drawn again at once", p.cells.len());
        for (&c, &e) in &p.cells {
            commands.entity(e).insert(Visibility::Inherited);
            streamer.loaded.insert(c, e);
        }
        if t.interior.is_some() {
            streamer.settled = true;
        }
    }
    if keep {
        // Seamless doors: the eye's adaptation eases from the place left's (`SkyState::cinematic`).
        sky.grade_ease = Some((sky.cinematic(), 0.0));
    }
    sky.set_interior(t.interior.and_then(|id| fnv_world::lighting::cell_lighting(world.lo, world.schema, id)));
    sky.interior_grade = t.interior.and_then(|id| crate::sky::SkyState::cell_grade(world.lo, world.schema, id));
    match (keep, t.interior.is_some(), lod.parked.is_some()) {
        // Seamless doors (step 3): into an interior keeping the outdoors, the distant
        // land is hidden, not dropped; back out, it is drawn again as it was.
        (true, true, false) if streamer.parked.as_ref().is_some_and(|p| p.interior.is_none()) => lod.park(&mut commands, true),
        (_, false, true) if back_out => lod.park(&mut commands, false),
        _ => lod.set_enabled(&mut commands, t.interior.is_none() && !args.no_lod),
    }
}

/// SetCellImageSpace (`fnv_game::game::FormChanges::cell_image_spaces`): an interior
/// whose image space a script changed is graded with the new one, from when the cell
/// loads after the call (entered again, or a save made in it loaded): the original
/// shows a change to the cell the player is in only then [documented: GECK wiki
/// SetCellImageSpace, notes]. Looked at on every travel (which graded the cell with
/// its XCIM) and when the script runtime (with a loaded save's changes) becomes ready.
/// Runs after [`travel`].
pub fn cell_image_space(
    mut travels: MessageReader<nvf_play::Travel>,
    streamer: Res<Streamer>,
    world: Res<WorldData>,
    script: Option<Res<nvf_play::script::ScriptState>>,
    mut sky: ResMut<crate::sky::SkyState>,
    mut seen: Local<Option<(Option<u32>, bool)>>,
) {
    let rt = script.as_deref().and_then(|s| s.rt.as_ref());
    let key = (streamer.interior, rt.is_some());
    let travelled = travels.read().count() > 0;
    if *seen == Some(key) && !travelled {
        return;
    }
    *seen = Some(key);
    let (Some(cell), Some(rt)) = (streamer.interior, rt) else { return };
    let Some(&space) = rt.game.forms.cell_image_spaces.get(&fnv_sim::FormId(cell)) else { return };
    match fnv_world::imagespace::image_space(world.lo, world.schema, space.0) {
        Some(grade) => {
            info!("cell image space: {cell:#010x} graded with {:#010x} (SetCellImageSpace)", space.0);
            sky.interior_grade = Some(grade);
        }
        None => warn!("cell image space: {:#010x} for {cell:#010x} is not an image space the grade reads", space.0),
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
    let (cell_e, refs, origin) = begin_cell(commands, world, key, None, tile, models, terrain, water, grass, false, streamer);
    for r in refs {
        spawn_ref(commands, world, cell_e, origin, r, models, water, trees, emittance, actors, streamer);
    }
    cell_e
}

/// Spawn a cell's entity with its landscape (from `prep`, read ahead, or read here),
/// grass and water; returns it with the references to spawn into it and its origin (game
/// space). `hidden`: spawned hidden, to be shown when its references are all in.
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
    grass: &mut crate::grass::GrassCtx,
    hidden: bool,
    streamer: &mut Streamer,
) -> (Entity, Vec<u32>, Vec3) {
    let stats = &mut streamer.stats;
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
    if let CellKey::Exterior(g) = key {
        if let Some((c, land_id)) = cell.and_then(|c| Some((c, c.land?))) {
            let prep = prep.or_else(|| prepare_terrain(world.lo, world.schema, land_id, g, tile, world.neighbour_lands(g)));
            if let Some(prep) = prep {
                let land = spawn_terrain(commands, cell_e, world, prep, &mut TerrainCtx { tm: terrain, assets: models });
                // Grass grows on the landscape's textures (grass.rs); the water surface
                // is the one drawn below (`spawn_water`).
                let ws = world.index.worldspaces.get(&world.world);
                let water = c.has_water().then(|| c.water_surface(ws.and_then(|w| w.default_water_height))).flatten();
                grass.spawn(commands, cell_e, world, land_id, &land, g, water, models);
            }
        }
        if let Some(p) = world.persistent_by_grid.get(&g) {
            refs.extend(p);
        }
    }
    if let Some(c) = cell {
        spawn_water(commands, cell_e, world, c, models, water, stats);
    }
    (cell_e, refs, origin)
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
    let waits = streamer.settled;
    let override_model = streamer.model_overrides.get(&r).cloned();
    let (hide, light_scale, managed, stats, waiting) = (&streamer.hide, streamer.light_scale, &streamer.managed, &mut streamer.stats, &mut streamer.waiting);
    let manage_actors = streamer.manage_actors;
    let to_cell = Affine3A::from_translation(-origin);
    if managed.contains(&r) {
        return;
    }
    let Some(p) = placement(world.lo, world.schema, r) else { return };
    // Imposters are drawn by the regions the camera is in, not by their cell
    // (imposters.rs).
    if p.imposter {
        stats.refs_imposters += 1;
        return;
    }
    // A primitive (a trigger volume, room bounds, a portal) is seen only in the editor,
    // even where its activator was copied from one with a model (level-design.md §4.1).
    if !p.draws_base_model() {
        stats.refs_skipped_type += 1;
        *stats.skipped_types.entry("primitive".into()).or_default() += 1;
        return;
    }
    if manage_actors && p.base.and_then(|b| base_object(world.lo, world.schema, b)).is_some_and(|b| matches!(&b.sig, b"NPC_" | b"CREA")) {
        return;
    }
    if !world.enabled(&p, 0) {
        // Actors, creatures and light sources are spawned hidden, so a script's
        // Enable can show them (actors.rs `SpawnedDisabled`); other models are
        // nvf-play's (`script_draw.rs`).
        if !p.deleted() {
            spawn_dormant(commands, cell_e, world, &p, r, to_cell, models, actors, light_scale, emittance, stats, waits.then_some(&mut *waiting));
        }
        stats.refs_skipped_disabled += 1;
        return;
    }
    let base: Option<BaseObject> = p.base.and_then(|b| base_object(world.lo, world.schema, b));
    // Placed NPCs and creatures (actors.rs `spawn`, `spawn_creature`, actors.md §10).
    if let (Some((aw, parts, sim, ahead)), Some(b)) = (actors.as_mut(), base.as_ref().filter(|b| matches!(&b.sig, b"NPC_" | b"CREA"))) {
        let w = WaitingActor { cell: cell_e, rt: to_cell * ref_affine(&p), at: Vec3::from_array(p.position), reference: r, base: b.form_id, creature: &b.sig == b"CREA", hidden: false };
        if spawn_or_wait(commands, w, aw, *sim, ahead, &mut ActorCtx { cache: parts, models }, waits.then_some(&mut *waiting), &mut stats.actors) {
            return;
        }
    }
    let Some(base) = base.filter(|b| DRAWN.contains(&&b.sig)) else {
        stats.refs_skipped_type += 1;
        let t = base_type_name(p.base.and_then(|b| base_object(world.lo, world.schema, b)).as_ref());
        *stats.skipped_types.entry(t).or_default() += 1;
        return;
    };
    if &base.sig == b"LIGH"
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
        )
    {
        stats.lights += 1;
    }
    if base.is_speedtree() {
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
    if &base.sig == b"PWAT" && spawn_placed_water(commands, cell_e, world, &base, r, to_cell * ref_affine(&p), models, water, stats) {
        return;
    }
    // A destroyed reference: its destruction stage's replacement model (destroyed.rs).
    let base = match override_model {
        Some(m) => base_object_with_model(base, &m),
        None => base,
    };
    // The reference transform replaces the model root's own (world.md §4).
    spawn_model(commands, cell_e, world, &p, r, &base, to_cell * ref_affine(&p), models, emittance, hide, stats);
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
    rt: Affine3A,
    models: &mut ModelCtx,
    water: &mut WaterCtx,
    stats: &mut StreamStats,
) -> bool {
    let Some(wid) = fnv_world::sky::placeable_water_type(world.lo, world.schema, base.form_id) else { return false };
    let Some(path) = base.world_model_path() else { return false };
    let Some(model) = models.model(&path) else { return false };
    let reflects = fnv_world::sky::placeable_water_reflects(world.lo, world.schema, base.form_id);
    let Some(mat) = water_material(world, wid, reflects, true, models, water) else { return false };
    if !reflects {
        info!("placed water {r:08X}: its PWAT {:08X} does not set Reflects, drawn without reflection", base.form_id);
    }
    for s in &model.shapes {
        commands.entity(cell_e).with_child((
            Mesh3d(s.mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_matrix(Mat4::from(rt * s.transform)),
            bevy::camera::visibility::RenderLayers::layer(crate::reflection::WATER_LAYER),
            RefVisual(r),
        ));
    }
    stats.water_planes += 1;
    true
}

/// Draw a reference's model as children of `parent`, `rt` being the reference's
/// transform in the parent's space: an exterior or interior cell, or the world root for
/// imposters (`imposters.rs`).
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
) {
    // Armour only ever worn (Non-Playable, no world model): its biped model would hang in
    // the air at the reference (the Fort's slave backpacks, #2439; `fnv_world::placed::worn_only`).
    if base.worn_only {
        stats.refs_no_model += 1;
        info!("world: 0x{r:08X} {}: armour only ever worn (Non-Playable, no world model), not drawn on its own", base.edid.as_deref().unwrap_or("?"));
        return;
    }
    let Some(path) = base.world_model_path() else {
        stats.refs_no_model += 1;
        return;
    };
    if hide.iter().any(|h| path.contains(h.as_str())) {
        return;
    }
    if script_marker(base.form_id).unwrap_or_else(|| is_marker(&path)) {
        stats.refs_skipped_type += 1;
        *stats.skipped_types.entry("marker".into()).or_default() += 1;
        return;
    }
    // With the base's alternate textures on the shapes they name (texture-swaps.md §4).
    let Some(model) = models.model_swapped(&path, false, &base.world_alt_textures) else {
        stats.refs_no_model += 1;
        if spawn_error_marker(commands, parent, r, rt, models) {
            stats.refs_error_marker += 1;
            // Once per model: a broken mod can place hundreds of one missing model.
            static TOLD: std::sync::Mutex<Option<HashSet<String>>> = std::sync::Mutex::new(None);
            if TOLD.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashSet::new).insert(path.clone()) {
                info!("world: 0x{r:08X} {path}: its model did not load; drawn as the error marker (each reference of it)");
            }
        }
        return;
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
    if let Some(rig) = model.rig.as_ref().filter(|rig| (rig.idle.is_some() && idle.is_some()) || door.is_some() || activator.is_some() || scripted || jointed || wreck) {
        // Animated (Idle sequence): its own node hierarchy per reference.
        // "Random Anim Start" bases start their idle at a phase spread by reference
        // (fnv_actor::anims::random_anim_start; animation.md §8).
        let phase = if random_anim_start(world, base.form_id) { crate::actors::clip_phase(r) } else { 0.0 };
        stats.refs_random_start += usize::from(phase > 0.0 && rig.idle.is_some());
        let (root, drawn, nodes) = rig.spawn_nodes(commands, parent, Transform::from_matrix(Mat4::from(rt)), &path, phase);
        commands.entity(root).insert(RefVisual(r));
        for e in &drawn {
            commands.entity(*e).insert(RefVisual(r));
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
        stats.shapes += drawn.len();
        stats.refs_drawn += 1;
        stats.refs_animated += 1;
        return;
    }
    // A jointed object left posed by its designer (`XRGD`, `nvf_play::clutter_pose`).
    // Whether it poses is decided by `clutter_pose::drawn` (the matching rule and every
    // body on a named node below the model's root), the same condition the collision
    // uses and the same function `nvf_play::script_draw` draws references enabled later
    // with (issue #2339).
    let posed = nvf_play::clutter_pose::drawn(world.lo, &base.sig, r, &path, &model.nodes, || nvf_play::clutter_pose::body_nodes(&models.data, &path));
    if let Some(p) = &posed {
        info!("clutter pose: 0x{r:08X} {path} drawn in its stored pose ({} bodies)", p.bodies);
    }
    stats.refs_posed += usize::from(posed.is_some());
    for s in &model.shapes {
        let shape = nvf_play::clutter_pose::shape_transform(posed.as_ref(), s);
        let m = Mat4::from(rt * shape);
        // External emittance: the reference's XEMI source drives the colour.
        let material = p
            .emittance
            .filter(|_| s.external_emittance)
            .and_then(|x| emittance.material(world, &mut models.materials, &s.material, x))
            .unwrap_or_else(|| s.material.clone());
        // A shape below a damage-stage node: under a holder of its own, shown at its stages
        // only (destroyed.rs); the holder's RefVisual follows the reference's presence.
        let stages = s.node.and_then(|n| nvf_assets::model::damage_stages_of(&model.nodes, n));
        let e = match stages {
            Some(st) => {
                let holder = commands.spawn((Transform::IDENTITY, Visibility::Inherited, RefVisual(r), ChildOf(parent))).id();
                commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(material), Transform::from_matrix(m), Visibility::Hidden, crate::destroyed::StagedShape::new(r, st), ChildOf(holder))).id()
            }
            None => commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(material), Transform::from_matrix(m), RefVisual(r), ChildOf(parent))).id(),
        };
        // A mine's light parts: shown and hidden by its state (`nvf_play::mines`).
        if let Some(l) = nvf_play::mines::MineLight::of(s.name.as_deref(), &s.ancestors) {
            commands.entity(e).insert(l);
        }
        stats.shapes += 1;
    }
    spawn_emitters(commands, parent, r, &model, &path, rt);
    stats.refs_drawn += 1;
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
            if crate::lights::spawn_light(commands, holder, world, base.form_id, base.model.as_deref(), models, rt, light_scale, p, emittance) {
                crate::actors::spawn_hidden(commands, holder, r);
                stats.lights_hidden += 1;
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
    let Some(mat) = water_material(world, wid, true, false, models, water) else { return };
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
fn billboard(world: &WorldData, tree: u32, models: &mut ModelCtx) -> Option<(Handle<Mesh>, Handle<NifMaterial>)> {
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
            info!("render: {} base form(s) made markers or drawn by a script (SetInternalMarker)", m.len());
        }
    }
}

/// Editor-only marker meshes [hypothesis: the game hides them; models named marker*].
fn is_marker(path: &str) -> bool {
    let file = path.rsplit('\\').next().unwrap_or(path);
    file.starts_with("marker") || path.contains("\\markers\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces() {
        let t = game_to_bevy();
        let g = Vec3::new(1.0, 2.0, 3.0);
        let b = t.transform_point(g);
        assert!((bevy_to_game(b) - g).length() < 1e-5);
        // Game up (z) is Bevy up (y).
        assert!((t.transform_point(Vec3::Z) - Vec3::Y).length() < 1e-5);
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
}
