//! Cell resets: dead actors flagged Respawn come back (docs/spec-notes/dead-on-spawn.md §4;
//! owner's playtest 2026-10-12, far-world leftovers).
//!
//! - **When** [documented: GECK wiki SetZoneRespawns, "Respawn rate on zones is controlled
//!   by a gamesetting: iHoursToRespawnCell (default setting is 72 hours)"; IsCellExpired, a
//!   cell "has expired and going to be reset on the next load (happens 3 days after the
//!   last visit)"; ResetInterior, the cell "has to be loaded in order to get cleaned up"]:
//!   a cell the player's loaded area left starts its clock then (GameDaysPassed,
//!   `fnv_sim::World::detach_cell`); when the loaded area takes it in again after at least
//!   `iHoursToRespawnCell` game hours (`fnv_sim::World::respawn_hours`: 72 when no plugin
//!   sets it), the cell is reset before anything in it runs.
//! - **Who comes back** [documented: GECK wiki Category:NPC: the flag Respawn,
//!   "References of this NPC in the world will be recreated after three days"; the note on
//!   Base Health 0, "the 'Respawns' flag must not be ticked or the actor will respawn alive
//!   after 3 days"; ResetInterior, "Unlike a time-based cell reset, this scripted reset does
//!   NOT resurrect Actors"]: every dead actor reference placed in that cell (its authored
//!   cell) whose base has Respawn — the template's flag with the template flag "Base Data"
//!   ([`crate::data::ActorDef::template_base`]). Corpses laid out by a new game (Base Health
//!   0, `Game::lay_out_corpses`) are dead actors like any other.
//! - **How** [hypothesis: "recreated" read as the reference starting again as authored]:
//!   alive, at its authored place and facing, with its authored inventory rolled again
//!   (`fnv_sim::World::reset_inventory`), and none of its run-time state (actor values and
//!   damage, killer, factions, packages: `RefExtra` dropped). The wiki's "with 0 HP" is not
//!   copied: it comes back with its full health [design: an actor alive at 0 health has no
//!   meaning here; the owner checks].
//! - The cell's scripted references run their OnReset blocks (`Game::cell_reset_blocks`).
//! - Respawning containers in the cell go back to their authored contents
//!   (`fnv_sim::World::attach_cell`, docs/spec-notes/simulation.md §3).
//! - A cell whose encounter zone has "Never Resets" is not reset: no actor comes back and
//!   no container refills (docs/spec-notes/encounter-zones.md §4).
//! - A recreated actor's leveled level is fixed again (`crate::zones`).
//! - **ResetInterior cell** (Fallout 3's; The Pitt and Fallout 3 call it) marks an
//!   interior cell expired; it is reset at its next load, whenever the player left it,
//!   and that reset does not bring dead actors back [documented: GECK wiki ResetInterior,
//!   catalogue notes "only flags the cell as expired", "the cell has to be loaded in
//!   order to get cleaned up", "does NOT resurrect Actors"]: its respawning containers go
//!   back to their authored contents and its scripted references run OnReset
//!   [hypothesis: the rest of a reset, as a timed one; an encounter zone's "Never
//!   Resets" does not stop it]. Kept in `FormChanges::expired_cells` (saved) until then.
//! - Not done: removing the bodies of killed actors that do not respawn
//!   ("Quest Item: Dead body isn't removed when the cell regenerates", Category:NPC).

use crate::functions::base_of;
use crate::game::Game;
use fnv_sim::FormId;
use serde::Serialize;

/// What a cell reset did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CellReset {
    pub cell: FormId,
    /// Game hours the cell was left for.
    pub hours_away: f64,
    /// Actors brought back alive.
    pub respawned: Vec<FormId>,
    /// Containers back to their authored contents.
    pub containers: Vec<FormId>,
}

impl Game<'_> {
    /// Whether the actor reference `r` respawns: its base has the flag Respawn, or its
    /// template does with the template flag "Base Data" (module notes).
    pub fn respawns(&self, r: FormId) -> bool {
        base_of(self, r)
            .and_then(|b| self.template_source(b, r, |d| d.template_base))
            .is_some_and(|(id, d)| self.acbs_flag(id, d, crate::actor_values::acbs::RESPAWN, |d| d.respawn))
    }

    /// Whether `cell`'s encounter zone currently has "Never Resets", accounting for
    /// `SetCellEncounterZone` / `SetWorldspaceEncounterZone`, saved `SetZoneRespawns`,
    /// and session `SetZoneFlag` changes.
    pub fn cell_never_resets(&self, cell: FormId) -> bool {
        let Some(z) = crate::nvse_jg5::cell_zone(self, cell) else {
            return false;
        };
        self.zone(z).map_or_else(|| self.world.cell_never_resets(cell), |d| d.never_resets)
    }

    /// Whether `cell` will reset at its next load: `None` while the player is in it or
    /// before the loaded area has ever held it; else whether ResetInterior marked it or
    /// it has been left for `iHoursToRespawnCell` game hours outside a zone that never
    /// resets (module notes). IsCellExpired and the local map's tweak read it.
    pub fn cell_expired(&self, cell: FormId) -> Option<bool> {
        if self.cell_of(FormId::PLAYER_REF) == Some(cell) || !self.forms.visited_cells.contains(&cell) {
            return None;
        }
        if self.forms.expired_cells.contains(&cell) {
            return Some(true);
        }
        let now = self.world.calendar().ok().map(|k| k.days_passed);
        let away = now.zip(self.world.detached_since(cell)).map(|(n, s)| (f64::from(n) - f64::from(s)) * 24.0);
        Some(away.is_some_and(|h| h + 1e-3 >= f64::from(self.world.respawn_hours())) && !self.cell_never_resets(cell))
    }

    /// The cells `left` are no longer in the loaded area: their reset clock starts.
    pub(crate) fn cells_left(&mut self, left: &[FormId]) {
        // TogglePurgeOnUnload's references in them go (`crate::nvse_jip12`).
        self.purge_left(left);
        for &c in left {
            // Without the time globals (synthetic data) there is no clock to start.
            let _ = self.world.detach_cell(c);
        }
    }

    /// `cell` comes into the loaded area: reset it when it has been away long enough
    /// (module notes). The reset, if any, is also kept for [`Game::take_cell_resets`].
    pub(crate) fn cell_entered(&mut self, cell: FormId) -> Option<CellReset> {
        let expired = self.forms.expired_cells.remove(&cell);
        let full = self.forms.expired_cells_full.remove(&cell);
        let since = self.world.detached_since(cell);
        let now = self.world.calendar().ok().map(|c| c.days_passed);
        let never_resets = self.cell_never_resets(cell);
        let containers = self.world.attach_cell_with_never_resets(cell, never_resets).unwrap_or_default();
        let away = now.zip(since).map(|(n, s)| (n as f64 - s as f64) * 24.0);
        // The same slack as `World::attach_cell` (GameDaysPassed is an f32). An encounter
        // zone with "Never Resets" is never reset (docs/spec-notes/encounter-zones.md §4).
        let timed = away.filter(|h| h + 1e-3 >= self.world.respawn_hours() as f64 && !never_resets);
        // ResetInteriorAlt's reset brings the dead respawning actors back as a timed one
        // [documented: ResetInteriorAlt, "will also let dead actors that were set to
        // respawn be resurrected"].
        let by_time = timed.is_some();
        let timed = timed.or_else(|| full.then(|| away.unwrap_or(0.0)));
        let containers = if by_time || timed.is_none() { containers } else { self.world.reset_containers(cell) };
        let Some(hours_away) = timed else {
            if !expired {
                return None;
            }
            // ResetInterior's reset: no actor comes back (module notes). `attach_cell`
            // reset nothing, its delay not having run out.
            let containers = self.world.reset_containers(cell);
            let reset = CellReset { cell, hours_away: away.unwrap_or(0.0), respawned: Vec::new(), containers };
            self.cell_resets.push(reset.clone());
            return Some(reset);
        };
        let placed = self.world.placed;
        let dead: Vec<FormId> = placed
            .by_cell
            .get(&cell)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&r| placed.get(r).is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE")) && self.is_dead(r) && self.respawns(r))
            .collect();
        for &r in &dead {
            self.recreate(r);
        }
        let reset = CellReset { cell, hours_away, respawned: dead, containers };
        self.cell_resets.push(reset.clone());
        Some(reset)
    }

    /// The reference `r` starts again as authored, alive (module notes).
    fn recreate(&mut self, r: FormId) {
        // The script it runs now (its pick's, a SetScript change included), before the
        // pick is made again.
        let before = self.ref_script(r);
        self.extra.remove(&r);
        if let Some(p) = self.world.placed.get(r) {
            let (cell, pos, rot) = (p.cell, p.pos, p.rot);
            if let Ok(s) = self.world.get(r) {
                (s.cell, s.pos, s.rot) = (cell, pos, rot);
            }
        }
        // Made anew: at its level now, its leveled pick and carried items with it.
        self.refix_level(r);
        let _ = self.world.reset_inventory(r);
        self.maybe_apply_npc_weapon_mods(r);
        // A new pick may run another script (template flag "Script"): when the script it
        // runs changes, its instance starts fresh; a ScriptWait ends unless it is the
        // script it runs now [design, #3228].
        let key = crate::game::InstKey::Ref { reference: r };
        let after = self.ref_script(r);
        if before != after {
            self.instances.remove(&key);
        }
        if self.jip.waits.get(&key).is_some_and(|w| Some(w.script) != after) {
            self.jip.waits.remove(&key);
        }
        // The layer that moves actors starts it again from its authored place.
        self.teleported.insert(r);
    }

    /// The references of the reset `cell` with a script run its OnReset block, the block's
    /// subject the reference itself [documented: GECK wiki Reference, the cell's reset
    /// "trigger[s] OnReset blocks in scripts attached to the reference"; the event handler
    /// table's WhatsReset]. Every scripted reference placed there and present, not only
    /// those that respawned, after the reset and before their OnLoad [hypothesis: the
    /// order; the wiki ties the blocks to the cell's reset, not to respawning].
    pub(crate) fn cell_reset_blocks(&mut self, cell: FormId) {
        let mut refs = self.data.scripted_by_cell.get(&cell).cloned().unwrap_or_default();
        // Actors scripted only through their template too (#3228).
        refs.extend(self.template_scripted_in(cell));
        refs.sort();
        refs.dedup();
        for r in refs {
            if self.present(r) {
                self.ref_event(r, crate::game::event::ON_RESET, Some(fnv_script::vm::Value::Ref(r.0)), None);
            }
        }
    }

    /// The cell resets since the last call (the engine draws their actors alive again).
    pub fn take_cell_resets(&mut self) -> Vec<CellReset> {
        std::mem::take(&mut self.cell_resets)
    }
}
