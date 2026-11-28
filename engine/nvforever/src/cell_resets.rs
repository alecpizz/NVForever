//! Cell resets on screen (`fnv_game::respawn`, docs/spec-notes/dead-on-spawn.md §4): when
//! the game resets a cell the player's loaded area takes in again after
//! `iHoursToRespawnCell`, its respawning actors are alive again. Their corpse (the death
//! pose, `deaths.rs`, and the ragdoll, `ragdoll.rs`) is taken away, and where the corpse
//! lay is forgotten; the residents (`residents.rs`) take them up from their authored
//! place (`Game::teleported`). One log line per reset.

use crate::locomotion::Dying;
use crate::ragdoll::{Corpse, Ragdolls};
use bevy::prelude::*;
use nvf_play::phys::PhysState;
use nvf_play::script::ScriptState;
use nvf_play::RefVisual;
use std::collections::HashSet;

pub struct CellResetsPlugin;

impl Plugin for CellResetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show_resets.after(nvf_play::PlaySet::Activation).before(crate::residents::drive));
    }
}

/// A drawn actor: its entity and reference.
type Shown = (Entity, &'static RefVisual);
/// Drawn dead: in its death pose or a ragdoll.
type ShownDead = Or<(With<Dying>, With<Corpse>)>;

fn show_resets(
    mut commands: Commands,
    script: Option<ResMut<ScriptState>>,
    mut ragdolls: ResMut<Ragdolls>,
    mut phys: Option<ResMut<PhysState>>,
    mut residents: Option<ResMut<crate::residents::Residents>>,
    corpses: Query<Shown, ShownDead>,
) {
    let Some(mut script) = script else { return };
    let Some(rt) = script.rt.as_mut() else { return };
    let resets = rt.game.take_cell_resets();
    if resets.is_empty() {
        return;
    }
    let mut back = HashSet::new();
    for r in &resets {
        let names: Vec<String> = r.respawned.iter().map(|&a| format!("{a} ({})", rt.game.data.name(rt.game.world.placed.get(a).map_or(a, |p| p.base)))).collect();
        let line = format!(
            "cell reset: {} after {:.1} h away: {} actors back alive{}{}, {} containers back to their contents",
            rt.game.data.name(r.cell),
            r.hours_away,
            r.respawned.len(),
            if names.is_empty() { "" } else { ": " },
            names.join(", "),
            r.containers.len()
        );
        // A cell where nothing came back is not news.
        if r.respawned.is_empty() && r.containers.is_empty() {
            debug!("{line}");
        } else {
            info!("{line}");
        }
        back.extend(r.respawned.iter().map(|a| a.0));
    }
    let mut redrawn = HashSet::new();
    for id in &back {
        ragdolls.rest.remove(id);
        if let Some(p) = phys.as_deref_mut() {
            p.world.remove_ragdoll(*id);
        }
        if residents.as_deref_mut().is_some_and(|r| r.redraw(&mut commands, *id)) {
            redrawn.insert(*id);
        }
    }
    for (e, v) in &corpses {
        if back.contains(&v.0) && !redrawn.contains(&v.0) {
            commands.entity(e).remove::<(Dying, Corpse)>();
        }
    }
}
