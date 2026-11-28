//! The player's miscellaneous statistics (GetPCMiscStat, ModPCMiscStat, the Pip-Boy's
//! General list): one counter per code, kept in `PlayerState::misc_stats` and saved with
//! it. ModPCMiscStat adds to the same counter the rules count into. docs/spec-notes/misc-stats.md.
//!
//! The codes are the order of [`NAMES`] [hypothesis, misc-stats.md §1: the five codes the
//! shipped scripts compile (1, 10, 20, 22, 24) fall at their places, and the original's
//! saves hold 43 counters whose values fit these names, Total Things Killed (35) the sum of
//! People Killed (2) and Creatures Killed (3)].

use crate::aid::{Applied, ChangeKind, LIMB_CONDITIONS, RAD_LEVEL, DEHYDRATION};
use crate::data::IngestibleDef;
use crate::game::Game;
use fnv_sim::FormId;

/// The 43 statistics by code.
pub const NAMES: [&str; 43] = [
    "Quests Completed",
    "Locations Discovered",
    "People Killed",
    "Creatures Killed",
    "Locks Picked",
    "Computers Hacked",
    "Stimpaks Taken",
    "Rad-X Taken",
    "RadAway Taken",
    "Chems Taken",
    "Times Addicted",
    "Mines Disarmed",
    "Speech Successes",
    "Pockets Picked",
    "Pants Exploded",
    "Books Read",
    "Health From Stimpaks",
    "Weapons Created",
    "Health From Food",
    "Water Consumed",
    "Sandman Kills",
    "Paralyzing Punches",
    "Robots Disabled",
    "Times Slept",
    "Corpses Eaten",
    "Mysterious Stranger Visits",
    "Doctor Bags Used",
    "Challenges Completed",
    "Miss Fortunate Occurrences",
    "Disintegrations",
    "Have Limbs Crippled",
    "Speech Failures",
    "Items Crafted",
    "Weapon Modifications",
    "Items Repaired",
    "Total Things Killed",
    "Dismembered Limbs",
    "Caravan Games Won",
    "Caravan Games Lost",
    "Barter Amount Traded",
    "Roulette Games Played",
    "Blackjack Games Played",
    "Slots Games Played",
];

pub const QUESTS_COMPLETED: u16 = 0;
pub const LOCATIONS_DISCOVERED: u16 = 1;
pub const PEOPLE_KILLED: u16 = 2;
pub const CREATURES_KILLED: u16 = 3;
pub const LOCKS_PICKED: u16 = 4;
pub const COMPUTERS_HACKED: u16 = 5;
pub const STIMPAKS_TAKEN: u16 = 6;
pub const RAD_X_TAKEN: u16 = 7;
pub const RADAWAY_TAKEN: u16 = 8;
pub const CHEMS_TAKEN: u16 = 9;
pub const TIMES_ADDICTED: u16 = 10;
pub const MINES_DISARMED: u16 = 11;
pub const SPEECH_SUCCESSES: u16 = 12;
pub const POCKETS_PICKED: u16 = 13;
pub const PANTS_EXPLODED: u16 = 14;
pub const BOOKS_READ: u16 = 15;
pub const HEALTH_FROM_STIMPAKS: u16 = 16;
pub const HEALTH_FROM_FOOD: u16 = 18;
pub const WATER_CONSUMED: u16 = 19;
pub const TIMES_SLEPT: u16 = 23;
pub const DOCTOR_BAGS_USED: u16 = 26;
pub const CHALLENGES_COMPLETED: u16 = 27;
pub const DISINTEGRATIONS: u16 = 29;
pub const HAVE_LIMBS_CRIPPLED: u16 = 30;
pub const SPEECH_FAILURES: u16 = 31;
pub const ITEMS_CRAFTED: u16 = 32;
pub const ITEMS_REPAIRED: u16 = 34;
pub const TOTAL_THINGS_KILLED: u16 = 35;
pub const DISMEMBERED_LIMBS: u16 = 36;
pub const BARTER_AMOUNT_TRADED: u16 = 39;
pub const ROULETTE_GAMES_PLAYED: u16 = 40;
pub const CARAVAN_GAMES_WON: u16 = 37;
pub const CARAVAN_GAMES_LOST: u16 = 38;
pub const BLACKJACK_GAMES_PLAYED: u16 = 41;
pub const SLOTS_GAMES_PLAYED: u16 = 42;

/// The codes the rules or the engine count (the others only ModPCMiscStat changes).
pub const COUNTED: [u16; 35] = [
    QUESTS_COMPLETED,
    LOCATIONS_DISCOVERED,
    PEOPLE_KILLED,
    CREATURES_KILLED,
    LOCKS_PICKED,
    COMPUTERS_HACKED,
    STIMPAKS_TAKEN,
    RAD_X_TAKEN,
    RADAWAY_TAKEN,
    CHEMS_TAKEN,
    TIMES_ADDICTED,
    MINES_DISARMED,
    SPEECH_SUCCESSES,
    POCKETS_PICKED,
    PANTS_EXPLODED,
    BOOKS_READ,
    HEALTH_FROM_STIMPAKS,
    HEALTH_FROM_FOOD,
    WATER_CONSUMED,
    TIMES_SLEPT,
    DOCTOR_BAGS_USED,
    CHALLENGES_COMPLETED,
    DISINTEGRATIONS,
    HAVE_LIMBS_CRIPPLED,
    SPEECH_FAILURES,
    ITEMS_CRAFTED,
    ITEMS_REPAIRED,
    TOTAL_THINGS_KILLED,
    DISMEMBERED_LIMBS,
    CARAVAN_GAMES_WON,
    CARAVAN_GAMES_LOST,
    BARTER_AMOUNT_TRADED,
    ROULETTE_GAMES_PLAYED,
    BLACKJACK_GAMES_PLAYED,
    SLOTS_GAMES_PLAYED,
];

/// The code of a statistic's name, ignoring case.
pub fn code(name: &str) -> Option<u16> {
    NAMES.iter().position(|n| n.eq_ignore_ascii_case(name.trim())).map(|i| i as u16)
}

/// ETYP codes (`IngestibleDef::equip_type`).
const EQUIP_CHEMS: i64 = 10;
const EQUIP_STIMPAK: i64 = 11;
/// Actor values: Health, Rad Resistance.
const HEALTH: u16 = 16;
const RAD_RESIST: u16 = 20;

/// What one use of an ingestible adds to the counters, by what it is and what its effects
/// did [hypothesis for every rule, misc-stats.md §3]: a Stimpak-type item (ETYP 11) is a
/// stimpak and counts the health it gives; a Chems-type item (ETYP 10) that raises Rad
/// Resistance is Rad-X, one that lowers rads is RadAway, any other is a chem; an item
/// whose effects only restore limb conditions (no chem, no stimpak) is a doctor's bag; a
/// food item (ENIT flag Food) counts the health it gives, and water when it lowers
/// dehydration.
pub fn ingest_counts(def: &IngestibleDef, applied: &[Applied]) -> Vec<(u16, f64)> {
    let changes: Vec<_> = applied.iter().filter_map(|a| a.change.map(|c| (c, a.duration))).collect();
    let health: f64 = changes
        .iter()
        .filter(|(c, _)| c.code == HEALTH && c.amount > 0.0)
        .map(|(c, d)| match c.kind {
            ChangeKind::Once => c.amount,
            ChangeKind::PerSecond => c.amount * d,
            ChangeKind::Modifier => 0.0,
        })
        .sum();
    let raises = |code: u16| changes.iter().any(|(c, _)| c.code == code && c.amount > 0.0);
    let mut out = Vec::new();
    if def.equip_type == EQUIP_STIMPAK {
        out.push((STIMPAKS_TAKEN, 1.0));
        if health > 0.0 {
            out.push((HEALTH_FROM_STIMPAKS, health.round()));
        }
    } else if def.equip_type == EQUIP_CHEMS {
        // "Restore" lowers rads (`crate::aid::grows`), so a positive change on rads is RadAway.
        let stat = if raises(RAD_RESIST) {
            RAD_X_TAKEN
        } else if raises(RAD_LEVEL) {
            RADAWAY_TAKEN
        } else {
            CHEMS_TAKEN
        };
        out.push((stat, 1.0));
    } else if !changes.is_empty() && changes.iter().all(|(c, _)| LIMB_CONDITIONS.contains(&c.code) && c.amount > 0.0) {
        out.push((DOCTOR_BAGS_USED, 1.0));
    }
    if def.flags & IngestibleDef::FOOD != 0 {
        if health > 0.0 {
            out.push((HEALTH_FROM_FOOD, health.round()));
        }
        if raises(DEHYDRATION) {
            out.push((WATER_CONSUMED, 1.0));
        }
    }
    out
}

impl Game<'_> {
    /// The player's statistic `code` goes up by `n`.
    pub fn count_stat(&mut self, code: u16, n: f64) {
        if n != 0.0 {
            *self.player.misc_stats.entry(code).or_default() += n;
            self.challenge_stat(code, n);
        }
    }

    /// The player's statistic `code` as GetPCMiscStat reads it: its counter, and for
    /// Locations Discovered also the map markers the player found (`PlayerState::found_markers`)
    /// [hypothesis: one data point, the save nvftest with only Goodsprings found read 1].
    pub fn misc_stat(&self, code: u16) -> f64 {
        let own = if code == LOCATIONS_DISCOVERED { self.player.found_markers.len() as f64 } else { 0.0 };
        own + self.player.misc_stats.get(&code).copied().unwrap_or(0.0)
    }

    /// The wait menu's hours start passing (`Rest::begin`); a sleep counts in Times Slept
    /// when no sleep was passing already [hypothesis: one per sleep, counted as it starts],
    /// unless `bSleepHealingMinDuration` + `bMiscStatRequiresSleepDuration` require the sleep
    /// to meet the minimum healing duration [design: docs/design/tweaks.md].
    pub fn begin_rest(&mut self, sleep: bool, hours: u32) {
        let count_sleep = !(self.tweaks.sleep_heal_min_hours > 0
            && self.tweaks.sleep_misc_stat_requires_duration
            && hours < self.tweaks.sleep_heal_min_hours);
        if sleep && !self.rest.sleeping() && count_sleep {
            self.count_stat(TIMES_SLEPT, 1.0);
        }
        self.rest.begin(sleep, hours);
        // JohnnyGuitar's sleep/wait handlers: 1 wait, 2 sleep [documented:
        // SetJohnnyOnSleepWaitEventHandler] (`crate::nvse_jg8`).
        let mode = fnv_script::vm::XVal::Num(if sleep { 2.0 } else { 1.0 });
        self.jg8_fire(crate::nvse_jg8::Ev::SleepWait, std::slice::from_ref(&mode), vec![mode.clone()]);
    }

    /// A quest was completed (CompleteQuest, a Complete Quest stage): every quest counts,
    /// shown in the Pip-Boy or not [hypothesis: the original's Save 3, in Doc Mitchell's
    /// house before VCG01 can finish, already reads 1].
    pub(crate) fn quest_completed(&mut self, quest: FormId) {
        self.count_stat(QUESTS_COMPLETED, 1.0);
        self.cover_completed(quest);
        self.emit(crate::trace::EventKind::QuestCompleted { quest });
        self.coop_rewards.completed(self.frame, quest);
        self.jg_event(crate::nvse_jip3::JgEvent::CompleteQuest, Some(quest));
    }

    /// The player killed `victim` (called while it still lives): an NPC is a person, a
    /// creature a creature, and both a thing killed.
    pub(crate) fn count_kill(&mut self, victim: FormId) {
        let npc = crate::functions::base_of(self, victim).and_then(|b| self.data.actors.get(&b)).is_some_and(|d| &d.sig == b"NPC_");
        self.count_stat(if npc { PEOPLE_KILLED } else { CREATURES_KILLED }, 1.0);
        self.count_stat(TOTAL_THINGS_KILLED, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aid::ValueChange;

    fn applied(code: u16, kind: ChangeKind, amount: f64, duration: f64) -> Applied {
        let change = ValueChange { code, kind, amount, parts: false, limb: None, applied: 0.0, permanent_pool: false, suspended: false };
        Applied { id: 0, effect: FormId(0), change: Some(change), duration, extended: false, inactive: false }
    }

    fn item(equip_type: i64, flags: u8) -> IngestibleDef {
        IngestibleDef { equip_type, flags, ..IngestibleDef::default() }
    }

    #[test]
    fn known_codes_fall_at_their_places() {
        // The codes the shipped scripts compile [observed, encodings.json].
        assert_eq!(code("Locations Discovered"), Some(1));
        assert_eq!(code("times addicted"), Some(10));
        assert_eq!(code("Sandman Kills"), Some(20));
        assert_eq!(code("Robots Disabled"), Some(22));
        assert_eq!(code("Corpses Eaten"), Some(24));
        assert_eq!(code("Bobbleheads Found"), None);
        assert_eq!(code("Caravan Games Won"), Some(CARAVAN_GAMES_WON));
        assert_eq!(code("Caravan Games Lost"), Some(CARAVAN_GAMES_LOST));
        assert!(COUNTED.iter().all(|&c| (c as usize) < NAMES.len()));
    }

    #[test]
    fn ingestibles_count_by_kind() {
        let stim = ingest_counts(&item(11, 4), &[applied(HEALTH, ChangeKind::PerSecond, 10.0, 3.0)]);
        assert_eq!(stim, vec![(STIMPAKS_TAKEN, 1.0), (HEALTH_FROM_STIMPAKS, 30.0)]);
        assert_eq!(ingest_counts(&item(10, 4), &[applied(RAD_RESIST, ChangeKind::Modifier, 75.0, 480.0)]), vec![(RAD_X_TAKEN, 1.0)]);
        assert_eq!(ingest_counts(&item(10, 4), &[applied(RAD_LEVEL, ChangeKind::PerSecond, 5.0, 10.0)]), vec![(RADAWAY_TAKEN, 1.0)]);
        assert_eq!(ingest_counts(&item(10, 0), &[applied(5, ChangeKind::Modifier, 2.0, 240.0)]), vec![(CHEMS_TAKEN, 1.0)]);
        assert_eq!(ingest_counts(&item(-1, 4), &[applied(26, ChangeKind::Once, 100.0, 0.0), applied(27, ChangeKind::Once, 100.0, 0.0)]), vec![(DOCTOR_BAGS_USED, 1.0)]);
        let water = ingest_counts(&item(12, 2), &[applied(HEALTH, ChangeKind::Once, 20.0, 0.0), applied(DEHYDRATION, ChangeKind::Once, 20.0, 0.0)]);
        assert_eq!(water, vec![(HEALTH_FROM_FOOD, 20.0), (WATER_CONSUMED, 1.0)]);
        assert!(ingest_counts(&item(13, 0), &[applied(HEALTH, ChangeKind::Modifier, 5.0, 60.0)]).is_empty());
    }
}
