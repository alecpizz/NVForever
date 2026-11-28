//! Test aid `NVF_TEST_ACTOR_FEET=<seconds>[:<range>]`: once, after that many seconds,
//! log for every drawn actor within `range` game units of the camera (default 3000) where
//! its root stands, where its lowest foot bone is, the drawn landscape height under it
//! and, with `--play`, the collision under it and the drawn ground (the top of a model's
//! drawn floors over that collision, `PhysState::ground_drawn`), so a floating or sunken
//! actor shows in the log (docs/spec-notes/floating-actors.md).

use crate::limbs::ActorRig;
use crate::world::{bevy_to_game, WorldData};
use bevy::prelude::*;
use nvf_play::phys::PhysState;

/// The ground under an actor is looked for from this far above its root (above a floor
/// it is drawn sunk into) [design].
const GROUND_FROM: f32 = 30.0;

/// The collision and drawn ground under a drawn root, as log text: ", collision ground
/// z …, drawn ground z …, root … above the drawn ground", or nothing without physics.
/// Fixed collision only: a dynamic body resting at the actor's feet (a tumbleweed the
/// wind left there) is not the ground it stands on (`PhysState::fixed_ground_drawn`).
fn ground_text(phys: Option<&mut PhysState>, root: Vec3) -> String {
    let Some(phys) = phys else { return String::new() };
    match phys.fixed_ground_drawn([root.x, root.y, root.z + GROUND_FROM], 2.0 * GROUND_FROM) {
        Some((bare, drawn)) => format!(", collision ground z {bare:.1}, drawn ground z {drawn:.1}, root {:.1} above the drawn ground", root.z - drawn),
        None => ", no collision ground under it".to_string(),
    }
}

/// A foot bone this far above the actor's root is off the ground: a standing actor's
/// ankles are about 7 units up, a walking one's lift a little more [observed].
const LIFTED: f32 = 30.0;

#[derive(Resource)]
pub struct FeetProbe {
    at: f32,
    range: f32,
    done: bool,
}

/// The probe asked for by the environment, if any.
pub fn from_env() -> Option<FeetProbe> {
    let v = std::env::var("NVF_TEST_ACTOR_FEET").ok()?;
    let mut it = v.split(':');
    let at = it.next()?.trim().parse().ok()?;
    let range = it.next().and_then(|r| r.trim().parse().ok()).unwrap_or(3000.0);
    Some(FeetProbe { at, range, done: false })
}

#[allow(clippy::type_complexity)]
pub fn probe(
    time: Res<Time>,
    mut p: ResMut<FeetProbe>,
    // Not the water's reflection camera, which follows the view mirrored below the
    // water (and came first in this query: no actor was in range of it).
    cams: Query<&GlobalTransform, crate::fly::WorldCamera>,
    roots: Query<(&Name, &GlobalTransform, &InheritedVisibility, &ActorRig)>,
    nodes: Query<&GlobalTransform>,
    bodies: Query<(&Name, &GlobalTransform), Without<ActorRig>>,
    (world, mut phys): (Option<Res<WorldData>>, Option<ResMut<PhysState>>),
) {
    if p.done || time.elapsed_secs() < p.at {
        return;
    }
    p.done = true;
    let Some(cam) = cams.iter().next().map(|c| bevy_to_game(c.translation())) else { return };
    let (mut n, mut lifted) = (0, 0);
    for (name, gt, vis, rig) in &roots {
        if !name.as_str().starts_with("Actor ") || !vis.get() {
            continue;
        }
        let root = bevy_to_game(gt.translation());
        if root.truncate().distance(cam.truncate()) > p.range {
            continue;
        }
        let foot = (rig.skel.names.iter().zip(&rig.nodes))
            .filter(|(n, _)| n.as_deref().is_some_and(|n| n.contains("Foot")))
            .filter_map(|(_, e)| nodes.get(*e).ok())
            .map(|t| bevy_to_game(t.translation()).z)
            .reduce(f32::min);
        let ground = world.as_deref().and_then(|w| land_height(w, root.x, root.y));
        let fmt = |v: Option<f32>| v.map_or("-".to_string(), |v| format!("{v:.0}"));
        let fmt1 = |v: Option<f32>| v.map_or("-".to_string(), |v| format!("{v:.1}"));
        info!(
            "actor feet: {} at ({:.0}, {:.0}, {:.1}): lowest foot bone z {}, drawn landscape z {}, root {} above it{}",
            name.as_str().trim_start_matches("Actor "),
            root.x,
            root.y,
            root.z,
            fmt(foot),
            fmt(ground),
            fmt1(ground.map(|g| root.z - g)),
            ground_text(phys.as_deref_mut(), root),
        );
        n += 1;
        lifted += usize::from(foot.is_some_and(|f| f - root.z > LIFTED));
    }
    // The player's own body (third-person rig), where it is drawn over the landscape.
    for (name, gt) in &bodies {
        if name.as_str() == "Player body (Third person)" {
            let root = bevy_to_game(gt.translation());
            let ground = world.as_deref().and_then(|w| land_height(w, root.x, root.y));
            let above = ground.map_or("-".to_string(), |g| format!("{:.1}", root.z - g));
            let more = ground_text(phys.as_deref_mut(), root);
            info!("actor feet: player body at ({:.0}, {:.0}, {:.1}): drawn landscape z {}, root {above} above it{more}", root.x, root.y, root.z, ground.map_or("-".to_string(), |g| format!("{g:.1}")));
        }
    }
    info!("actor feet: {n} actors within {:.0} units of the camera, {lifted} with a foot bone over {LIFTED} units above the root", p.range);
}

/// The landscape height at game (x, y) in worldspace `world` (FormID), whichever one is
/// drawn here (a co-op host simulates actors in its guests' worldspaces too).
pub(crate) fn land_height_in(data: &WorldData, world: u32, x: f32, y: f32) -> Option<f32> {
    let g = fnv_world::cell_of(x, y);
    let cell = data.index.worldspaces.get(&world).and_then(|w| w.grid.get(&g)).and_then(|c| data.index.cells.get(c))?;
    let land = fnv_world::land::load_land(data.lo, data.schema, cell.land?, fnv_world::land::LandParts::HEIGHTS)?;
    let (ox, oy) = (g.0 as f32 * fnv_world::CELL_SIZE, g.1 as f32 * fnv_world::CELL_SIZE);
    Some(fnv_world::grass::height_at(land.heights.as_ref()?, x - ox, y - oy))
}

/// How far below the land an undrawn actor's feet may be before they are put on it, game
/// units [design: a little more than the land mesh's own error].
pub const BELOW_LAND: f32 = 32.0;

/// An actor's feet, put on the land when its AI has them under it ([`BELOW_LAND`]). The
/// host's AI moves actors it does not draw on the navmesh alone, with no collision; where
/// it has them under the land a guest drew them there, and they fought from there
/// [observed: buckallobill, in game: lakelurks under the ground killed the guest].
pub fn on_the_land(feet: [f32; 3], land: Option<f32>) -> [f32; 3] {
    match land {
        Some(g) if feet[2] < g - BELOW_LAND => [feet[0], feet[1], g],
        _ => feet,
    }
}

/// The full-detail landscape height at game (x, y), as the terrain mesh draws it (on the
/// smoothed or stored surface, `WorldData::land_surface`).
fn land_height(world: &WorldData, x: f32, y: f32) -> Option<f32> {
    let g = fnv_world::cell_of(x, y);
    let surface = world.land_surface(g)?;
    let (ox, oy) = (g.0 as f32 * fnv_world::CELL_SIZE, g.1 as f32 * fnv_world::CELL_SIZE);
    Some(surface.height_at_clamped(x - ox, y - oy))
}
