//! Encounter zones and the level leveled lists are read at for one placed reference
//! (docs/spec-notes/encounter-zones.md).
//!
//! - A zone (ECZN, [`crate::forms::ZoneDef`]) gets its level the first time the player
//!   enters it: the player's level, raised to the zone's minimum level, except that a
//!   zone with "Match PC Below Minimum Level" takes a lower player's own level
//!   [documented: GECK wiki Bethsoft Tutorial NPC population, "if a second-level player
//!   enters an Encounter Zone with a min. level of 10, leveled lists in the Zone will
//!   spawn actors as though the player were level 10"; GetZoneFlag names the flag;
//!   hypothesis: what the flag does]. The level is then kept for good ("When the player
//!   enters an encounter zone for the first time, the level of that zone is set
//!   permanently", same page).
//! - A placed reference reads leveled lists at its zone's level, or at the player's level
//!   where it has no zone [documented: GECK wiki Reference, Leveled Actor: "If there is no
//!   encounter zone, which is common in the wasteland, the player's level is used"],
//!   changed by its level modifier (XLCM, [`modified`]).
//! - That level is fixed for the reference when its cell first comes into the loaded area
//!   ([`World::fix_level`]) [hypothesis: the game makes a leveled actor when its cell
//!   loads; fixing it keeps the actor the same one across later level-ups until its cell
//!   is reset].

use serde::{Deserialize, Serialize};

use crate::form::FormId;
use crate::forms::ZoneDef;
use crate::world::World;

/// The level a reference reads leveled lists at, and whether every entry at or under it
/// is eligible (the level modifier Easy) instead of the list's own "Calculate from all
/// levels" flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelAt {
    pub level: u16,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_levels: bool,
}

impl LevelAt {
    pub const fn plain(level: u16) -> LevelAt {
        LevelAt { level, all_levels: false }
    }

    /// Whether a list with "Calculate from all levels" `list_all` picks from every
    /// eligible entry.
    pub fn all(&self, list_all: bool) -> bool {
        list_all || self.all_levels
    }
}

/// The level a zone takes when first entered by a player of `player_level` (module notes).
pub fn level_on_entry(zone: &ZoneDef, player_level: u16) -> u16 {
    let min = zone.min_level.max(0) as u16;
    if player_level >= min || zone.match_pc_below_min {
        player_level.max(1)
    } else {
        min
    }
}

/// `level` changed by a level modifier (XLCM) [documented: GECK wiki Reference, Leveled
/// Actor: Easy "50% of the calculated level. All entries in the leveled list at or below
/// that level are available"; Medium 75 %, Hard 125 %, Boss 200 %, "Only the closest
/// entry ... will be selected"; None "A random entry equal to or below the calculated
/// level"]. [observed: XLCM holds 0–3 on 297 placed actors of FalloutNV.esm, read as
/// Easy, Medium, Hard, Very Hard after xEdit's enum.] [hypothesis: the percentage is
/// rounded down and never below 1; Medium, Hard and Boss keep the list's own flag, as
/// None does, rather than forcing "closest only".]
pub fn modified(level: u16, modifier: Option<i32>) -> LevelAt {
    modified_by(level, modifier, |_| None)
}

/// The game settings holding each level modifier's share, Easy to Boss, and the shares
/// they hold in FalloutNV.esm [observed: `fLeveledActorMultEasy` 0.5, `…Medium` 0.75,
/// `…Hard` 1.25, `…Boss` 2.0, the percentages the wiki gives; hypothesis: these are what
/// the engine reads, by their names].
pub const LEVEL_MULT_SETTINGS: [(&str, f32); 4] = [("fLeveledActorMultEasy", 0.5), ("fLeveledActorMultMedium", 0.75), ("fLeveledActorMultHard", 1.25), ("fLeveledActorMultBoss", 2.0)];

/// [`modified`] with each modifier's share read through `setting` (a game setting by
/// Editor ID, [`LEVEL_MULT_SETTINGS`]), so a plugin that changes them changes the levels.
pub fn modified_by(level: u16, modifier: Option<i32>, setting: impl Fn(&str) -> Option<f32>) -> LevelAt {
    let share = match modifier.and_then(|m| usize::try_from(m).ok()).and_then(|m| LEVEL_MULT_SETTINGS.get(m)) {
        Some(&(name, default)) => setting(name).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(default),
        None => 1.0,
    };
    // Whole percent, as the shares are, so 0.75 × 10 is 7 and not 7.4999 rounded up.
    let pct = (f64::from(share) * 100.0).round() as u64;
    let l = (u64::from(level) * pct / 100).clamp(1, u64::from(u16::MAX)) as u16;
    LevelAt { level: l, all_levels: modifier == Some(0) }
}

impl World<'_> {
    /// The zone's level, once the player has entered it.
    pub fn zone_level(&self, zone: FormId) -> Option<u16> {
        self.zone_levels.get(&zone).copied()
    }

    /// Every zone entered so far and its level.
    pub fn zone_levels(&self) -> &std::collections::BTreeMap<FormId, u16> {
        &self.zone_levels
    }

    /// Effective `ZoneDef` for `zone`: authored `ECZN DATA` with session
    /// `SetZoneMinLevel` and `SetZoneFlag` bit 2 ("Match PC Below Minimum Level")
    /// overrides applied. `None` when `zone` is not an `ECZN` form.
    pub fn effective_zone_def(&self, zone: FormId) -> Option<ZoneDef> {
        let mut d = *self.forms.zones.get(&zone)?;
        if let Some(&min) = self.zone_session.min_levels.get(&zone) {
            d.min_level = min;
        }
        if let Some(&m) = self.zone_session.match_levels.get(&zone) {
            d.match_pc_below_min = m;
        }
        Some(d)
    }

    /// Session `SetZoneMinLevel zone level` override (ignored for non-`ECZN` forms).
    pub fn set_zone_min_level(&mut self, zone: FormId, min_level: i8) {
        if self.forms.zones.contains_key(&zone) {
            self.zone_session.min_levels.insert(zone, min_level);
        }
    }

    /// Session `SetZoneFlag zone 2 on` ("Match PC Below Minimum Level") override (ignored
    /// for non-`ECZN` forms).
    pub fn set_zone_match_level(&mut self, zone: FormId, match_pc_below_min: bool) {
        if self.forms.zones.contains_key(&zone) {
            self.zone_session.match_levels.insert(zone, match_pc_below_min);
        }
    }

    /// Reference `r`'s own encounter zone: session `SetRefEncounterZone` override first
    /// (`None` when cleared), else authored `REFR/ACHR/ACRE XEZN`.
    pub fn ref_own_zone(&self, r: FormId) -> Option<FormId> {
        match self.zone_session.ref_zones.get(&r) {
            Some(&z) => z,
            None => self.placed.get(r).and_then(|p| p.encounter_zone),
        }
    }

    /// Session `SetRefEncounterZone [zone]` override (`None` removes the reference's own zone).
    pub fn set_ref_zone(&mut self, r: FormId, zone: Option<FormId>) {
        self.zone_session.ref_zones.insert(r, zone);
    }

    /// Effective encounter zone of worldspace `world`: session `SetWorldspaceEncounterZone`
    /// override first (`None` when cleared), else authored `WRLD XEZN`.
    pub fn world_zone_of(&self, world: FormId) -> Option<FormId> {
        match self.zone_session.world_zones.get(&world) {
            Some(&z) => z,
            None => self.placed.world_zones.get(&world).copied(),
        }
    }

    /// Session `SetWorldspaceEncounterZone world [zone]` override.
    pub fn set_world_zone(&mut self, world: FormId, zone: Option<FormId>) {
        self.zone_session.world_zones.insert(world, zone);
    }

    /// Effective encounter zone of `cell`: session `SetCellEncounterZone` first (`None`
    /// when cleared), else authored `CELL XEZN`, else its worldspace's effective zone
    /// (`SetWorldspaceEncounterZone` -> `WRLD XEZN`).
    pub fn cell_zone_of(&self, cell: FormId) -> Option<FormId> {
        if let Some(&z) = self.zone_session.cell_zones.get(&cell) {
            return z;
        }
        if let Some(&z) = self.placed.cell_zones.get(&cell) {
            return Some(z);
        }
        let w = self.placed.world_of(cell)?;
        self.world_zone_of(w)
    }

    /// Session `SetCellEncounterZone cell [zone]` override.
    pub fn set_cell_zone(&mut self, cell: FormId, zone: Option<FormId>) {
        self.zone_session.cell_zones.insert(cell, zone);
    }

    /// Effective encounter zone governing reference `r`: its own zone ([`World::ref_own_zone`]),
    /// else its placed cell's effective zone ([`World::cell_zone_of`]).
    pub fn zone_of(&self, r: FormId) -> Option<FormId> {
        if let Some(z) = self.ref_own_zone(r) {
            return Some(z);
        }
        let cell = self.placed.get(r)?.cell?;
        self.cell_zone_of(cell)
    }

    /// The player (at `player_level`) enters `zone`: it gets its level now if it has none,
    /// reading live session `SetZoneMinLevel` / `SetZoneFlag` bit 2 overrides. When
    /// [`World::no_lock_encounter_zone_levels`] is enabled (`bNoLockEncounterZoneLevels`),
    /// re-entering a visited zone recomputes and updates its level against `player_level`
    /// instead of keeping the first-visit level frozen [design: docs/design/tweaks.md].
    /// `Some(level)` when the zone's stored level was newly set or changed by this call.
    pub fn enter_zone(&mut self, zone: FormId, player_level: u16) -> Option<u16> {
        if !self.no_lock_encounter_zone_levels && self.zone_levels.contains_key(&zone) {
            return None;
        }
        let def = self.effective_zone_def(zone).unwrap_or_default();
        let level = level_on_entry(&def, player_level);
        if self.zone_levels.get(&zone).copied() == Some(level) {
            return None;
        }
        self.zone_levels.insert(zone, level);
        Some(level)
    }

    /// Record a saved encounter zone level directly (offline save import).
    pub fn set_zone_level(&mut self, zone: FormId, level: u16) {
        self.zone_levels.insert(zone, level.max(1));
    }

    /// The level `r` reads leveled lists at: the one fixed for it, else its effective
    /// zone's level (or the level the zone would take if entered now, including session
    /// overrides), else `player_level`, changed by its level modifier. When `lock_zones`
    /// is `false` (`bNoLockEncounterZoneLevels`), a previously recorded zone level is
    /// recomputed from `player_level` and `effective_zone_def` instead of staying frozen
    /// [design: docs/design/tweaks.md].
    pub fn level_at_with_zone_lock(&self, r: FormId, player_level: u16, lock_zones: bool) -> LevelAt {
        if let Some(&l) = self.ref_levels.get(&r) {
            return l;
        }
        let base = match self.zone_of(r) {
            Some(z) => self
                .zone_level(z)
                .filter(|_| lock_zones)
                .unwrap_or_else(|| level_on_entry(&self.effective_zone_def(z).unwrap_or_default(), player_level)),
            None => player_level.max(1),
        };
        modified_by(base, self.placed.get(r).and_then(|p| p.level_modifier), |name| self.setting_f32(name))
    }

    /// The level `r` reads leveled lists at: [`World::level_at_with_zone_lock`] using this
    /// world's `no_lock_encounter_zone_levels` setting.
    pub fn level_at(&self, r: FormId, player_level: u16) -> LevelAt {
        self.level_at_with_zone_lock(r, player_level, !self.no_lock_encounter_zone_levels)
    }

    /// The level fixed for `r`, if any.
    pub fn fixed_level(&self, r: FormId) -> Option<LevelAt> {
        self.ref_levels.get(&r).copied()
    }

    /// Every reference whose level is fixed.
    pub fn fixed_levels(&self) -> &std::collections::BTreeMap<FormId, LevelAt> {
        &self.ref_levels
    }

    /// Fix the level of `r` as [`World::level_at`] gives it now; `Some` when it was not
    /// fixed before.
    pub fn fix_level(&mut self, r: FormId, player_level: u16) -> Option<LevelAt> {
        if self.ref_levels.contains_key(&r) {
            return None;
        }
        let l = self.level_at(r, player_level);
        self.ref_levels.insert(r, l);
        Some(l)
    }

    /// Fix the level of `r` at `at`, fixed before or not (a co-op guest takes the level
    /// the host's game reads `r`'s leveled lists at, so both pick alike); whether it
    /// changed.
    pub fn set_level(&mut self, r: FormId, at: LevelAt) -> bool {
        self.ref_levels.insert(r, at) != Some(at)
    }

    /// Forget the level fixed for `r` (its cell was reset: it is made anew).
    pub fn unfix_level(&mut self, r: FormId) {
        self.ref_levels.remove(&r);
    }

    /// Whether `cell`'s zone has "Never Resets" (docs/spec-notes/encounter-zones.md §4),
    /// or a script turned its respawning off ([`World::set_zone_respawns`]).
    pub fn cell_never_resets(&self, cell: FormId) -> bool {
        let Some(z) = self.cell_zone_of(cell) else { return false };
        match self.zone_respawns.get(&z) {
            Some(&respawns) => !respawns,
            None => self.forms.zones.get(&z).is_some_and(|d| d.never_resets),
        }
    }

    /// Saved `SetZoneRespawns` override for `zone`, if any.
    pub fn zone_respawns_override(&self, zone: FormId) -> Option<bool> {
        self.zone_respawns.get(&zone).copied()
    }

    /// `SetZoneRespawns zone flag`: the zone's cells reset (`true`) or never reset, over
    /// its "Never Resets" flag; kept in the saved state [documented: GECK wiki
    /// SetZoneRespawns, "turns respawning on or off for an encounter zone", a savebaked
    /// function].
    pub fn set_zone_respawns(&mut self, zone: FormId, respawns: bool) {
        self.zone_respawns.insert(zone, respawns);
    }

    /// The level carried items of `r` are rolled at: [`World::level_at`] at the player's
    /// level now ([`World::roll_level`]), so the number a cell load would fix. An Easy
    /// modifier's "all levels" is not passed on, fixed or not.
    pub(crate) fn item_level(&self, r: FormId) -> u16 {
        self.level_at(r, self.roll_level()).level
    }

    /// The level a leveled lock on `r` is read at: its fixed level, else the configured
    /// one, 1 in play, whatever the player's level or its zone. So only a container fixed
    /// at a cell load hardens, and a door, which gets no fixed level, keeps its authored
    /// level [design; simulation.md §5].
    pub(crate) fn lock_read_level(&self, r: FormId) -> u16 {
        self.ref_levels.get(&r).map_or(self.config().player_level, |l| l.level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_modifiers_follow_the_settings() {
        // The masters' shares give the wiki's percentages.
        let at = |m| modified(10, m).level;
        assert_eq!([at(Some(0)), at(Some(1)), at(Some(2)), at(Some(3)), at(None)], [5, 7, 12, 20, 10]);
        assert!(modified(10, Some(0)).all_levels);
        // A plugin's own share is read.
        let hard = |name: &str| (name == "fLeveledActorMultHard").then_some(1.5);
        assert_eq!(modified_by(10, Some(2), hard).level, 15);
        assert_eq!(modified_by(10, Some(1), hard).level, 7);
        // Never below level 1.
        assert_eq!(modified_by(1, Some(0), |_| Some(0.0)).level, 1);
    }
}
