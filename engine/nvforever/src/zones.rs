//! Encounter zones on screen (`fnv_game::zones`, docs/spec-notes/encounter-zones.md §5):
//! the drawn actors read their leveled lists at the level the game gives each reference,
//! so the actor drawn is the one the game's rules (factions, packages, carried items)
//! speak of.
//!
//! The game's level of every placed actor is copied into `ActorWorld::levels` whenever it
//! can have changed: a zone was entered, references were fixed at a level (a cell
//! loaded), or the player's level changed. A reference whose level is not fixed yet gets
//! the level its cell's load will fix it at, so an actor drawn just before the game hears
//! of its cell is drawn as it will stay.

use crate::actors::ActorWorld;
use bevy::prelude::*;
use nvf_play::script::ScriptState;
use std::collections::HashMap;

pub struct ZonesPlugin;

impl Plugin for ZonesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (sync_levels, sync_actor_lists).after(nvf_play::PlaySet::Activation));
    }
}

/// ActorForms is the renderer's independent record view. Copy only changed list
/// definitions, including removal on load; unchanged frames allocate nothing.
#[derive(Default)]
struct ActorListSync {
    saved: std::collections::BTreeMap<fnv_sim::FormId, Vec<(u16, fnv_sim::FormId)>>,
    session: HashMap<fnv_sim::FormId, fnv_game::nvse_leveled::LevList>,
}

impl ActorListSync {
    fn accept(&mut self, g: &fnv_game::Game<'_>, actors: &mut fnv_actor::records::ActorForms) {
        if self.saved == g.forms.actor_list_additions && self.session == g.other.lev_actors {
            return;
        }
        let keys: std::collections::BTreeSet<_> = self.saved.keys().chain(self.session.keys()).chain(g.forms.actor_list_additions.keys()).chain(g.other.lev_actors.keys()).copied().collect();
        for id in keys {
            let Some(list) = g.lev_list(id) else { continue };
            let list = fnv_actor::records::LeveledNpc {
                chance_none: list.chance_none,
                all_levels: list.flags & 1 != 0,
                entries: list.entries.iter().map(|e| (e.0, e.1)).collect(),
            };
            match g.data.sig(id).as_ref() {
                Some(b"LVLN") => { actors.leveled.insert(id, list); }
                Some(b"LVLC") => { actors.leveled_creatures.insert(id, list); }
                _ => {}
            }
        }
        self.saved.clone_from(&g.forms.actor_list_additions);
        self.session.clone_from(&g.other.lev_actors);
    }
}

fn sync_actor_lists(script: Option<Res<ScriptState>>, actor_world: Option<ResMut<ActorWorld>>, mut last: Local<ActorListSync>) {
    let Some(g) = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game) else { return };
    let Some(mut aw) = actor_world else { return };
    last.accept(g, &mut aw.actors);
}

/// What the copy was made from: the zones entered, the player's level and the fixed
/// references.
#[derive(Default)]
struct LevelSync {
    key: Option<(usize, u16, bool)>,
    fixed: std::collections::BTreeMap<fnv_sim::FormId, fnv_sim::zones::LevelAt>,
}

/// The references whose fixed level was added, changed or removed between `old` and `new`.
fn fixed_changes<'a>(old: &'a std::collections::BTreeMap<fnv_sim::FormId, fnv_sim::zones::LevelAt>, new: &'a std::collections::BTreeMap<fnv_sim::FormId, fnv_sim::zones::LevelAt>) -> impl Iterator<Item = fnv_sim::FormId> + 'a {
    new.iter().filter(|(r, l)| old.get(r) != Some(l)).map(|(r, _)| *r)
        .chain(old.keys().filter(|r| !new.contains_key(r)).copied())
}

fn sync_levels(script: Option<Res<ScriptState>>, sim: Option<Res<nvf_play::sim::SimState>>, actor_world: Option<Res<ActorWorld>>, mut last: Local<LevelSync>) {
    let Some(script) = script else { return };
    let Some(rt) = script.rt.as_ref() else { return };
    let (Some(aw), Some(sim)) = (actor_world, sim) else { return };
    let g = &rt.game;
    // The world played is the simulation's: outside a game turn `g.world` is an empty
    // stand-in (`nvf_play::script::with_world`) with no fixed levels.
    let w = &sim.world;
    let player = g.player.level.map_or(w.config().player_level, |l| l.clamp(1, i32::from(u16::MAX)) as u16);
    let key = (w.zone_levels().len(), player, g.tweaks.no_lock_encounter_zone_levels);
    let display = fnv_sim::zones::LevelAt::plain(fnv_actor::resolve::DISPLAY_LEVEL);
    let is_actor = |r: fnv_sim::FormId| w.placed.get(r).is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE"));
    if last.key == Some(key) {
        // [design] #2956 follow-up: a reference not fixed reads its zone's level or the
        // player's (`World::level_at`), neither changed, so only the references whose fixed
        // level changed can read another level. Every placed actor of the game again at
        // each cell load was 9-10 ms of the frame [observed 2026-11-19, `profile hitch`].
        if w.fixed_levels() == &last.fixed { return; }
        let changed: Vec<_> = fixed_changes(&last.fixed, w.fixed_levels()).filter(|r| is_actor(*r)).collect();
        if let Ok(mut l) = aw.levels.lock() {
            for r in changed {
                let at = sim.leveled_at(g, r);
                if at == display { l.remove(&r.0); } else { l.insert(r.0, at); }
            }
        }
        last.fixed.clone_from(w.fixed_levels());
        return;
    }
    last.key = Some(key);
    last.fixed.clone_from(w.fixed_levels());
    let levels: HashMap<u32, fnv_sim::zones::LevelAt> = w
        .placed
        .refs
        .iter()
        .filter(|(_, p)| matches!(&p.sig, b"ACHR" | b"ACRE"))
        .map(|(&r, _)| (r.0, sim.leveled_at(g, r)))
        .filter(|(_, l)| *l != display)
        .collect();
    debug!("zones: {} actor reference(s) read leveled lists above the display level", levels.len());
    if let Ok(mut l) = aw.levels.lock() {
        *l = levels;
    };
}

#[cfg(test)]
mod actor_list_tests;
