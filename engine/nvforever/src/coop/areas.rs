//! Co-op replication: players in one game in different places, far apart [design: step 1
//! and the AI half of step 2 of the multi-area plan].
//!
//! The host's game simulates each guest's surroundings as it does its own while the
//! guest is away from it:
//!
//! - **Scripts, OnLoad and packages**: the cells around each guest are in
//!   `Game::loaded_cells` with the host's (`Game::set_coop_cells`).
//! - **The AI's high process**: the residents there walk, take up their packages and
//!   fight every frame (`Streamer::coop_areas`, `residents::simulated`), on the navmesh:
//!   the host's physics covers its own cells only, so there is no collision there.
//! - **Drawing**: each guest draws its own surroundings (its own streamer), with the
//!   host's actors there where the host's AI has them (`Residents::simulated_undrawn`,
//!   sent on the actor stream as the drawn ones are, `share_npcs`).
//!
//! A guest in the host's drawn cells adds nothing. Doors and fast travel move only the
//! one who uses them; a script moving the host's player takes everyone (`leash`).

use super::*;
use fnv_nav::Space;
use std::collections::BTreeSet;

/// Exterior cells simulated each way from a guest's cell (5 x 5, the script cells of
/// `Game::cells_around`).
const AROUND: i32 = 2;
/// How often the areas' cost is logged, seconds.
const LOG_EVERY: f32 = 10.0;

/// The host's record of its guests' areas.
#[derive(Default)]
pub(super) struct Areas {
    /// Each guest away from the host: its place and centre cell (exterior grid).
    last: Vec<(u8, Place, (i32, i32))>,
    last_log: f32,
    frames: u32,
    frame_secs: f32,
}

/// The host's side, each frame: the guests' surroundings join what it simulates.
#[allow(clippy::type_complexity)]
pub(super) fn coop_areas(
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    mut streamer: ResMut<Streamer>,
    (mut script, residents, world, loc): (Option<ResMut<nvf_play::script::ScriptState>>, Option<Res<Residents>>, Option<Res<WorldData>>, Option<Res<Location>>),
) {
    let coop = &mut *coop;
    let hosting = coop.link.as_ref().is_some_and(|l| l.is_host());
    let game = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| &mut rt.game);
    let (Some(game), Some(world), Some(loc), true) = (game, world, loc, hosting) else {
        if !streamer.coop_areas.is_empty() {
            streamer.coop_areas.clear();
        }
        return;
    };
    let here = place_of(&loc);
    let mut areas = Vec::new();
    let mut now_list = Vec::new();
    let mut cells = BTreeSet::new();
    for (&id, p) in coop.peers.iter().filter(|(_, p)| !p.lost) {
        let Some((_, s)) = p.samples.back() else { continue };
        let centre = fnv_world::cell_of(s.feet[0], s.feet[1]);
        match s.place {
            Place::Interior(c) => {
                if here == s.place {
                    continue;
                }
                areas.push(fnv_ai::LoadedArea { space: Space::Interior(c), cells: Default::default() });
                cells.insert(FormId(c));
            }
            Place::Exterior(w) => {
                // In the host's drawn cells: simulated there already.
                if here == s.place && w == world.world && streamer.interior.is_none() && streamer.loaded.contains_key(&centre) {
                    continue;
                }
                let around = (-AROUND..=AROUND).flat_map(|dx| (-AROUND..=AROUND).map(move |dy| (centre.0 + dx, centre.1 + dy))).collect();
                areas.push(fnv_ai::LoadedArea { space: Space::World(w), cells: around });
                if let Some(c) = game.world.placed.world_grid.get(&FormId(w)).and_then(|g| g.get(&centre)) {
                    cells.insert(*c);
                }
            }
        }
        now_list.push((id, s.place, centre));
    }
    if now_list != coop.areas.last {
        for (id, place, centre) in &now_list {
            if !coop.areas.last.iter().any(|(i, p, c)| i == id && p == place && c == centre) {
                info!("co-op areas: player {} is away from the host, in {}: its surroundings are simulated here", id + 1, place_name(&world, *place, *centre));
            }
        }
        for (id, ..) in coop.areas.last.iter().filter(|(i, ..)| !now_list.iter().any(|(j, ..)| j == i)) {
            info!("co-op areas: player {} is back in the host's area (or gone)", id + 1);
        }
        coop.areas.last = now_list;
    }
    if streamer.coop_areas != areas {
        streamer.coop_areas = areas;
    }
    game.set_coop_cells(cells);
    // The cost: frame time, the process's memory and each area's actors, now and then.
    let a = &mut coop.areas;
    a.frames += 1;
    a.frame_secs += time.delta_secs();
    let now = time.elapsed_secs();
    if now - a.last_log >= LOG_EVERY {
        let undrawn = residents.as_deref().map(|r| r.simulated_undrawn(&world, &streamer)).unwrap_or_default();
        let per: Vec<String> = streamer
            .coop_areas
            .iter()
            .zip(&a.last)
            .map(|(area, (id, place, centre))| {
                let n = undrawn.iter().filter(|x| area.holds(x.2, x.3)).count();
                format!("player {} in {}: {n} actors simulated undrawn", id + 1, place_name(&world, *place, *centre))
            })
            .collect();
        info!(
            "co-op areas: {} away from the host; host frame avg {:.2} ms over {} frames; private memory {} MB; script cells {}{}{}",
            per.len(),
            1000.0 * a.frame_secs / a.frames.max(1) as f32,
            a.frames,
            private_mb().map_or("?".into(), |m| m.to_string()),
            game.loaded_cells.len(),
            if per.is_empty() { "" } else { "; " },
            per.join("; ")
        );
        a.last_log = now;
        a.frames = 0;
        a.frame_secs = 0.0;
    }
}

fn place_name(world: &WorldData, place: Place, centre: (i32, i32)) -> String {
    match place {
        Place::Interior(c) => crate::nav::space_name(world, Space::Interior(c)),
        Place::Exterior(w) => format!("{} cell {:?}", crate::nav::space_name(world, Space::World(w)), centre),
    }
}

/// This process's private bytes, MB.
#[cfg(windows)]
pub(crate) fn private_mb() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: a zeroed counters struct of the size given, filled by the call for this
    // process's own pseudo handle.
    let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut c).cast::<PROCESS_MEMORY_COUNTERS>(), c.cb) };
    (ok != 0).then_some(c.PrivateUsage as u64 >> 20)
}

#[cfg(not(windows))]
pub(crate) fn private_mb() -> Option<u64> {
    None
}
