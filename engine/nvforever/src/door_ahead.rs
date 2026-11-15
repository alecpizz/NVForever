//! Seamless doors, step 1 for the renderer (docs/design/seamless-doors.md §2;
//! `nvf_play::seamless`): what a load door near the player leads to is read and prepared
//! before it is opened, so its travel draws the destination without reading a file.
//!
//! For each door `nvf_play::seamless::DoorsAhead` lists:
//! - **an interior**: the distinct models its references draw (`world::cell_models`, on a
//!   worker thread) are prepared (`ModelCtx::models_ready`), and the actors in it (placed,
//!   and residents whose walkers stand there) are listed for the actor read-ahead
//!   (`readahead::ReadAhead::list_for_door`);
//! - **the worldspace being drawn**: the squares around the arrival are read ahead and kept
//!   while indoors (`world::pin_cells`: models and landscape), and their actors listed.
//!
//! Physics prepares the same destinations on its own (`nvf_play::seamless`,
//! `phys::warm`). Another worldspace's destination is not read ahead.
//!
//! At the exit `door read-ahead summary: …`.

use crate::readahead::ReadAhead as ActorAhead;
use crate::world::{Streamer, WorldData};
use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, Task};
use fnv_world::alt_textures::AltTexture;
use fnv_world::cell_of;
use fnv_world::placed::{base_object, placement};
use nvf_assets::model::ModelCtx;
use nvf_play::seamless::{DoorsAhead, Place, Seamless};
use std::collections::{HashMap, HashSet};

/// Real seconds between checks that what was prepared is still in the caches (a model
/// asked for is kept) [engineering choice: far under the caches' 300 s].
const KEEP_ASKING_SECS: f64 = 5.0;

type Models = Vec<(String, Vec<AltTexture>)>;

#[derive(Default)]
struct InteriorAhead {
    task: Option<Task<Models>>,
    models: Option<Models>,
    ready: bool,
    asked: f64,
}

#[derive(Resource, Default)]
pub struct DoorAhead {
    /// The doors the destinations were listed for, last.
    doors: Vec<u32>,
    interiors: HashMap<u32, InteriorAhead>,
    /// The worldspace squares pinned, when a door leads outdoors.
    cells: Vec<(i32, i32)>,
    cells_logged: bool,
    last: f64,
    /// For the exit line: interiors found ready ahead, actors listed.
    interiors_ready: usize,
    actors_listed: usize,
}

/// The squares around an arrival outdoors that the renderer draws (`--radius`).
fn arrival_cells(feet: [f32; 3], radius: i32) -> Vec<(i32, i32)> {
    let (x, y) = cell_of(feet[0], feet[1]);
    (-radius..=radius).flat_map(|dx| (-radius..=radius).map(move |dy| (x + dx, y + dy))).collect()
}

/// The placed actors of `refs` enabled at the start: (reference, base, creature).
fn placed_actors(world: &WorldData, refs: &[u32]) -> Vec<(u32, u32, bool)> {
    refs.iter()
        .filter_map(|&r| {
            let p = placement(world.lo, world.schema, r).filter(|p| !p.deleted() && world.enabled(p, 0))?;
            let b = p.base.and_then(|b| base_object(world.lo, world.schema, b)).filter(|b| matches!(&b.sig, b"NPC_" | b"CREA"))?;
            Some((r, b.form_id, &b.sig == b"CREA"))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn door_ahead(
    seamless: Option<Res<Seamless>>,
    doors: Option<Res<DoorsAhead>>,
    location: Option<Res<nvf_play::Location>>,
    world: Res<WorldData>,
    mut streamer: ResMut<Streamer>,
    mut models: ModelCtx,
    terrain: Res<crate::terrain::TerrainSettings>,
    mut actors: ResMut<ActorAhead>,
    residents: Option<Res<crate::residents::Residents>>,
    mut state: ResMut<DoorAhead>,
) {
    let (Some(s), Some(doors), Some(location)) = (seamless, doors, location) else { return };
    if !s.ahead {
        return;
    }
    let now = nvf_play::seamless::now_secs();
    let ids: Vec<u32> = doors.doors.iter().map(|d| d.door).collect();
    let here = doors.place == Some(Place::of(*location));
    let mut want_interiors = HashSet::new();
    let mut want_cells = Vec::new();
    if here {
        for d in &doors.doors {
            match Place::arrival(&d.travel, *location) {
                Place::Interior(c) => {
                    want_interiors.insert(c);
                }
                Place::Exterior(w) if w == world.world => want_cells.extend(arrival_cells(d.travel.feet, streamer.radius)),
                Place::Exterior(_) => {}
            }
        }
    }
    want_cells.sort_unstable();
    want_cells.dedup();
    if ids != state.doors {
        // The doors near the player changed: list their destinations' actors again.
        state.doors = ids;
        actors.forget_door_actors();
        let mut list = Vec::new();
        for &c in &want_interiors {
            let refs = world.index.cells.get(&c).map(|c| c.refs.clone()).unwrap_or_default();
            list.extend(placed_actors(&world, &refs));
            list.extend(residents.as_deref().map(|r| r.undrawn_inside(c)).unwrap_or_default());
        }
        if !want_cells.is_empty() {
            let set: HashSet<(i32, i32)> = want_cells.iter().copied().collect();
            for &g in &want_cells {
                let mut refs: Vec<u32> = world.grid_cell(g).map(|c| c.refs.clone()).unwrap_or_default();
                refs.extend(world.persistent_by_grid.get(&g).into_iter().flatten());
                list.extend(placed_actors(&world, &refs));
            }
            list.extend(residents.as_deref().map(|r| r.undrawn_in(world.world, &set)).unwrap_or_default());
        }
        let n = actors.list_for_door(list);
        state.actors_listed += n;
        if n > 0 {
            info!("door read-ahead: {n} actor(s) of the doors' destinations listed");
        }
        state.interiors.retain(|c, _| want_interiors.contains(c));
        state.cells_logged = false;
    }
    // Interiors: their models listed on a worker thread, then prepared.
    for &c in &want_interiors {
        let a = state.interiors.entry(c).or_default();
        if a.models.is_none() && a.task.is_none() {
            let refs = world.index.cells.get(&c).map(|c| c.refs.clone()).unwrap_or_default();
            let (lo, schema, hide) = (world.lo, world.schema, streamer.hide.clone());
            a.task = Some(nvf_assets::read_pool::read_pool().spawn(async move { crate::world::cell_models(lo, schema, &refs, &hide) }));
        }
        if let Some(m) = a.task.as_mut().and_then(check_ready) {
            a.task = None;
            a.models = Some(m);
        }
        let Some(m) = a.models.as_ref() else { continue };
        if !a.ready || now - a.asked >= KEEP_ASKING_SECS {
            let was = a.ready;
            a.ready = models.models_ready(m.iter().map(|(p, s)| (p.as_str(), s.as_slice())));
            a.asked = now;
            if a.ready && !was {
                info!("door read-ahead: interior {c:08X} ready ahead: its {} models prepared", m.len());
                state.interiors_ready += 1;
            }
        }
    }
    // Seamless doors' keep and portal: outdoors, the interior behind the nearest door is
    // spawned ahead, hidden, once its models are prepared, so its door opens onto it.
    let nearest = doors.doors.first().filter(|_| here && s.keep && s.portal && location.interior.is_none());
    let want = nearest.and_then(|d| match Place::arrival(&d.travel, *location) {
        Place::Interior(c) if state.interiors.get(&c).is_some_and(|a| a.ready) => Some(c),
        _ => None,
    });
    if streamer.prespawn_want != want {
        streamer.prespawn_want = want;
    }
    // Indoors with nothing outdoors kept: the squares around the nearest door's arrival
    // outdoors, spawned ahead once read (`world::spawn_outdoors_ahead`).
    let outdoors = doors.doors.first().filter(|_| here && s.keep && s.portal && location.interior.is_some() && !streamer.parked.as_ref().is_some_and(|p| p.interior.is_none()));
    let want_out = outdoors.and_then(|d| match Place::arrival(&d.travel, *location) {
        Place::Exterior(w) if w == world.world => {
            let cells = arrival_cells(d.travel.feet, streamer.radius);
            let (n, ready) = streamer.ahead.pinned_ready();
            (n > 0 && ready == n).then_some(cells)
        }
        _ => None,
    });
    if streamer.prespawn_outdoors != want_out && (want_out.is_some() || streamer.outdoors_spawning.is_none()) {
        streamer.prespawn_outdoors = want_out;
    }
    // Outdoors: the squares around the arrival, pinned in the cell read-ahead.
    if (now - state.last >= nvf_play::seamless::AHEAD_EVERY_SECS) || want_cells != state.cells {
        state.last = now;
        if !want_cells.is_empty() || !state.cells.is_empty() {
            crate::world::pin_cells(&world, &mut streamer, &want_cells, terrain.tile, &mut models);
        }
        state.cells = want_cells;
        let (n, ready) = streamer.ahead.pinned_ready();
        if n > 0 && ready == n && !state.cells_logged {
            state.cells_logged = true;
            info!("door read-ahead: the {n} squares around a door's arrival outdoors are read, their models prepared");
        }
    }
}

pub fn report(mut exit: MessageReader<AppExit>, seamless: Option<Res<Seamless>>, state: Res<DoorAhead>) {
    if exit.read().next().is_none() || !seamless.is_some_and(|s| s.ahead) {
        return;
    }
    info!("door read-ahead summary: {} interior(s) prepared ahead, {} actor(s) listed", state.interiors_ready, state.actors_listed);
}
