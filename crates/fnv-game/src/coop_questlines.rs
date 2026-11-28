//! Questlines a co-op guest can sit out, and its quest progression sync choice
//! (docs/design/coop-quest-sync.md) [design].
//!
//! - **Lines.** [`LINES`] names each line by its roots, its split points (editor IDs), or
//!   a DLC master. [`compute`] fills in the rest from the data: families of a root's
//!   editor ID (its timers, support and fail quests), a DLC master's quests, and the quests
//!   a member's scripts start, to a fixed point (design §2).
//! - **The guest's choice.** [`QuestSync`] (`PlayerState::quest_sync`, carried with the
//!   character into its own save): sync on or off, and the lines sat out by key.
//! - **The warning.** [`in_progress`]: the shared quests part way through, which turning
//!   sync off (or sitting their line out) leaves at their current stage (design §4).

use crate::coop_joint::ClaimLedger;
use crate::data::GameData;
use fnv_util::words::{tr, trf};
use crate::quest::QuestState;
use fnv_sim::FormId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A set of lines, one bit per row of [`LINES`] (the wire's form: every player runs the
/// same build, so the rows agree).
pub type Mask = u64;

/// What kind of line a row is: its heading in the Questlines menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Main,
    Dlc,
    Faction,
    Companion,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Main, Kind::Dlc, Kind::Faction, Kind::Companion];

    /// The menu heading [design: our own words].
    pub fn label(self) -> &'static str {
        match self {
            Kind::Main => tr("Main story"),
            Kind::Dlc => tr("Add-ons"),
            Kind::Faction => tr("Factions"),
            Kind::Companion => tr("Companions"),
        }
    }
}

/// One line: its saved key, its shown name, and where it splits off (module notes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineDef {
    /// Saved with the character (`QuestSync::sat_out`): never change a key once shipped.
    pub key: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    /// Editor IDs: the line's first quests (and those it shares with other lines).
    pub roots: &'static [&'static str],
    /// A DLC master: all of its quests are the line's.
    pub plugin: Option<&'static str>,
}

/// The Battle of Hoover Dam and the ending, shared by all four branches: a quest of
/// several lines is sat out when any is (design §2) [observed: FalloutNV.esm QUST EDIDs
/// and FULL names: Eureka!, Viva Las Vegas!, the end states, the Legate's fight].
macro_rules! finale {
    ($($r:expr),*) => {
        &[$($r,)* "vHDBattleController", "VMQ06", "VMQEndStates", "VLegateShowdown", "VEnding", "VEndingBinks"]
    };
}

/// The lines (design §2). Editor IDs and names [observed: the masters' QUST records,
/// `fnvtool esm inventory`]; which quest is whose [hypothesis: from the Fallout Wiki's
/// quest pages, to check with `fnvtool coop questlines`]. At most 64 rows ([`Mask`]).
pub const LINES: &[LineDef] = &[
    // The trunk every branch shares: Goodsprings to Benny and the Platinum Chip.
    LineDef { key: "main", name: "Main story: to Benny and the Platinum Chip", kind: Kind::Main, roots: &["VCG02", "VCG03", "VMQ01", "VMQTops"], plugin: None },
    LineDef { key: "house", name: "Mr. House", kind: Kind::Main, roots: finale!("VMQHouse", "vfreeformlucky38", "VHDHouseBattle"), plugin: None },
    LineDef { key: "ncr", name: "NCR", kind: Kind::Main, roots: finale!("VMQ03a", "VMQ05", "VMQNCRFail", "VHDKimballSpeech", "VHDAssassinationDialog"), plugin: None },
    LineDef { key: "legion", name: "Caesar's Legion", kind: Kind::Main, roots: finale!("VMQ02", "VMQ03b", "VMS35", "VMQCaesarFail", "VHDLegionBattle"), plugin: None },
    LineDef {
        key: "yes-man",
        name: "Yes Man (independent)",
        kind: Kind::Main,
        roots: finale!("VMQYesMan01", "VMQYesMan02", "VMQYesMan03", "VMQYesManFailsafe", "VMQYesManSupport", "VHDIndependentBattle"),
        plugin: None,
    },
    LineDef { key: "dead-money", name: "Dead Money", kind: Kind::Dlc, roots: &[], plugin: Some("DeadMoney.esm") },
    LineDef { key: "honest-hearts", name: "Honest Hearts", kind: Kind::Dlc, roots: &[], plugin: Some("HonestHearts.esm") },
    LineDef { key: "old-world-blues", name: "Old World Blues", kind: Kind::Dlc, roots: &[], plugin: Some("OldWorldBlues.esm") },
    LineDef { key: "lonesome-road", name: "Lonesome Road", kind: Kind::Dlc, roots: &[], plugin: Some("LonesomeRoad.esm") },
    LineDef { key: "gun-runners-arsenal", name: "Gun Runners' Arsenal", kind: Kind::Dlc, roots: &[], plugin: Some("GunRunnersArsenal.esm") },
    LineDef { key: "bos", name: "Brotherhood of Steel", kind: Kind::Faction, roots: &["VMS55"], plugin: None },
    LineDef { key: "boomers", name: "Boomers", kind: Kind::Faction, roots: &["VMS23", "VMS15", "VNellisJack", "VNellisInfestation", "NellisSunshineBoogie"], plugin: None },
    LineDef { key: "khans", name: "Great Khans", kind: Kind::Faction, roots: &["VMS45", "VMS46"], plugin: None },
    LineDef { key: "kings", name: "The Kings", kind: Kind::Faction, roots: &["VMS19", "VMS29"], plugin: None },
    LineDef { key: "omertas", name: "Omertas", kind: Kind::Faction, roots: &["VMS21"], plugin: None },
    LineDef { key: "white-glove", name: "White Glove Society", kind: Kind::Faction, roots: &["VMS18"], plugin: None },
    LineDef { key: "powder-gangers", name: "Powder Gangers", kind: Kind::Faction, roots: &["VMS02"], plugin: None },
    LineDef { key: "goodsprings", name: "Goodsprings", kind: Kind::Faction, roots: &["VMS16"], plugin: None },
    LineDef { key: "boone", name: "Boone", kind: Kind::Companion, roots: &["VMS53"], plugin: None },
    LineDef { key: "cass", name: "Cass", kind: Kind::Companion, roots: &["VCassCompanion", "VMS29a"], plugin: None },
    LineDef { key: "arcade", name: "Arcade Gannon", kind: Kind::Companion, roots: &["VMS54"], plugin: None },
    LineDef { key: "veronica", name: "Veronica", kind: Kind::Companion, roots: &["VMS49"], plugin: None },
    LineDef { key: "rex", name: "Rex", kind: Kind::Companion, roots: &["VMS24"], plugin: None },
    LineDef { key: "ede", name: "ED-E", kind: Kind::Companion, roots: &["vDialogueEDE"], plugin: None },
];

/// The bit of row `i` of [`LINES`].
pub fn bit(i: usize) -> Mask {
    1u64.checked_shl(i as u32).unwrap_or(0)
}

/// The row of `key`.
pub fn index(key: &str) -> Option<usize> {
    LINES.iter().position(|l| l.key == key)
}

/// The shown names of the lines in `m`, in table order.
pub fn names(m: Mask) -> Vec<&'static str> {
    LINES.iter().enumerate().filter(|(i, _)| m & bit(*i) != 0).map(|(_, l)| l.name).collect()
}

/// Whether `edid` is of `root`'s family: the root itself, or the root and a suffix (its
/// timers, support and fail quests), where a root ending in a digit takes no suffix that
/// starts with one (`VMS2` is not `VMS20`'s) [design].
pub fn family(root: &str, edid: &str) -> bool {
    if edid.len() < root.len() || !edid.is_char_boundary(root.len()) || !edid[..root.len()].eq_ignore_ascii_case(root) {
        return false;
    }
    let rest = &edid[root.len()..];
    match (root.chars().last(), rest.chars().next()) {
        (_, None) => true,
        (Some(r), Some(s)) if r.is_ascii_digit() && s.is_ascii_digit() => false,
        _ => true,
    }
}

/// What [`group`] needs of one quest.
#[derive(Debug, Clone)]
pub struct QuestFacts<'a> {
    pub form: FormId,
    pub edid: &'a str,
    /// Its master's file name (the load order's entry for its index).
    pub plugin: &'a str,
    /// Start-game enabled: runs from the game's start, so no line takes it by the start
    /// graph.
    pub start_enabled: bool,
}

/// Each quest's lines ([`Mask`]); a quest in none is absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Questlines {
    pub of: BTreeMap<FormId, Mask>,
}

impl Questlines {
    pub fn lines_of(&self, q: FormId) -> Mask {
        self.of.get(&q).copied().unwrap_or(0)
    }

    /// The quests of row `i`, in FormID order.
    pub fn members(&self, i: usize) -> Vec<FormId> {
        self.of.iter().filter(|(_, m)| **m & bit(i) != 0).map(|(q, _)| *q).collect()
    }
}

/// Group the quests into [`LINES`] (module notes; design §2). `starts` is, for each quest,
/// the quests its scripts and dialogue lines start. Pure: the same data gives the same
/// table.
pub fn group(quests: &[QuestFacts], starts: &BTreeMap<FormId, BTreeSet<FormId>>) -> Questlines {
    let mut of: BTreeMap<FormId, Mask> = BTreeMap::new();
    // 1-3: roots, families and DLC masters: fixed members, never taken by another line's
    // start graph. A quest named as another line's root is that line's, not a family
    // member here (VMS29a, Birds of a Feather, is Cass's, not the Kings' VMS29).
    let rooted = |i: usize, edid: &str| LINES.iter().enumerate().any(|(j, l)| j != i && l.roots.iter().any(|r| r.eq_ignore_ascii_case(edid)));
    for (i, l) in LINES.iter().enumerate() {
        for q in quests {
            let named = l.roots.iter().any(|r| r.eq_ignore_ascii_case(q.edid));
            let by_root = named || (!rooted(i, q.edid) && l.roots.iter().any(|r| family(r, q.edid)));
            let by_plugin = l.plugin.is_some_and(|p| p.eq_ignore_ascii_case(q.plugin));
            if by_root || by_plugin {
                *of.entry(q.form).or_default() |= bit(i);
            }
        }
    }
    let fixed: BTreeSet<FormId> = of.keys().copied().collect();
    let enabled: BTreeSet<FormId> = quests.iter().filter(|q| q.start_enabled).map(|q| q.form).collect();
    // 4: what members start, to a fixed point.
    loop {
        let mut grew = false;
        let members: Vec<(FormId, Mask)> = of.iter().map(|(q, m)| (*q, *m)).collect();
        for (q, m) in members {
            for s in starts.get(&q).into_iter().flatten() {
                if fixed.contains(s) || enabled.contains(s) || *s == q {
                    continue;
                }
                let e = of.entry(*s).or_default();
                if (*e | m) != *e {
                    *e |= m;
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    Questlines { of }
}

/// The quests each quest's scripts start: its quest script, its stages' result scripts
/// and its dialogue lines' result scripts (`StartQuest Q`, `SetStage Q <its first
/// stage>`, as `crate::coop_prereqs` reads them).
fn start_graph(data: &GameData) -> BTreeMap<FormId, BTreeSet<FormId>> {
    let mut out: BTreeMap<FormId, BTreeSet<FormId>> = BTreeMap::new();
    let scan = |owner: FormId, unit: Option<crate::data::UnitId>, out: &mut BTreeMap<FormId, BTreeSet<FormId>>| {
        if let Some(c) = unit.and_then(|u| data.units.get(u)) {
            crate::coop_prereqs::starts(data, &c.unit, &c.program, out.entry(owner).or_default());
        }
    };
    for (q, def) in &data.quests {
        scan(*q, def.script.and_then(|s| data.scripts.get(&s).copied()), &mut out);
        for entries in def.stages.values() {
            for e in entries {
                scan(*q, e.unit, &mut out);
            }
        }
    }
    for d in data.infos.values() {
        if let Some(q) = d.quest {
            scan(q, d.begin, &mut out);
            scan(q, d.end, &mut out);
        }
    }
    out.retain(|_, s| !s.is_empty());
    out
}

/// The load order's questlines (module notes).
pub fn compute(data: &GameData) -> Questlines {
    let facts: Vec<QuestFacts> = data
        .quests
        .values()
        .map(|d| QuestFacts {
            form: d.form,
            edid: d.edid.as_deref().unwrap_or(""),
            plugin: data.load_order.get((d.form.0 >> 24) as usize).map_or("", |s| s.as_str()),
            start_enabled: d.start_enabled(),
        })
        .collect();
    group(&facts, &start_graph(data))
}

/// A guest character's quest progression sync (design §1): saved with the character.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestSync {
    /// Sync on or off; `None` until the character first chose (the join question asks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    /// The lines sat out, by [`LineDef::key`] (a key a later table lacks is kept, unused).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub sat_out: BTreeSet<String>,
}

impl QuestSync {
    pub fn is_empty(&self) -> bool {
        self.on.is_none() && self.sat_out.is_empty()
    }

    /// The lines sat out, as bits of [`LINES`].
    pub fn mask(&self) -> Mask {
        self.sat_out.iter().filter_map(|k| index(k)).fold(0, |m, i| m | bit(i))
    }

    pub fn sits_out(&self, i: usize) -> bool {
        LINES.get(i).is_some_and(|l| self.sat_out.contains(l.key))
    }

    /// Sit line `i` out, or take it again.
    pub fn set_sat_out(&mut self, i: usize, out: bool) {
        let Some(l) = LINES.get(i) else { return };
        if out {
            self.sat_out.insert(l.key.to_string());
        } else {
            self.sat_out.remove(l.key);
        }
    }

    /// Whether this character takes the host's progress in a quest of lines `lines`
    /// (`None`: progress that names no quest, the host's script XP and plain hand-outs).
    /// Off takes nothing; not chosen yet takes what it took before the toggle (rewards: yes;
    /// stages wait for the choice, `coop_progress`) (design §3).
    pub fn takes(&self, lines: Option<Mask>) -> bool {
        self.on != Some(false) && lines.is_none_or(|m| m & self.mask() == 0)
    }
}

/// The shared quests part way through (design §4): a claimed stage in the character's
/// `ledger` (waiting or replayed), not completed or failed in its own game (`own`) nor
/// ended in the host's (`host_ended`), and `within` (a line's quests, or all). In
/// FormID order.
pub fn in_progress(own: &BTreeMap<FormId, QuestState>, ledger: &ClaimLedger, host_ended: &[(FormId, bool)], within: impl Fn(FormId) -> bool) -> Vec<FormId> {
    let claimed: BTreeSet<FormId> = ledger.stages.iter().filter(|(_, s)| !s.is_empty()).map(|(q, _)| *q).chain(ledger.done.iter().filter(|(_, s)| !s.is_empty()).map(|(q, _)| *q)).collect();
    claimed
        .into_iter()
        .filter(|q| !own.get(q).is_some_and(|s| s.completed || s.failed))
        .filter(|q| !host_ended.iter().any(|(h, _)| h == q))
        .filter(|q| within(*q))
        .collect()
}

/// The confirm box's text for quests `names` part way through (design §4) [design: our
/// own words]: turning sync off (`line` `None`) or sitting `line` out.
pub fn warning(names: &[String], line: Option<&str>) -> String {
    let list = names.join(", ");
    let what = match line {
        None => tr("If you stop syncing quest progress now").to_string(),
        Some(l) => trf("If you sit out {line} now", &[("line", &l)]),
    };
    let one = names.len() == 1;
    let these = if one { tr("This shared quest is part way through") } else { tr("These shared quests are part way through") };
    let stay = if one { tr("it stays at its current stage in your own save and stops following the host") } else { tr("they stay at their current stage in your own save and stop following the host") };
    trf("{these}: {list}. {what}, {stay}. Syncing again later picks a quest up only if you missed none of its steps.", &[("these", &these), ("list", &list), ("what", &what), ("stay", &stay)])
}

impl crate::game::Game<'_> {
    /// This load order's questlines, read on first use.
    pub fn coop_questlines(&self) -> &Questlines {
        self.coop_questlines_cache.get_or_init(|| compute(self.data))
    }

    /// Whether this character takes the host's progress in `q` (`None`: progress that
    /// names no quest) by its [`QuestSync`].
    pub fn coop_takes_quest(&self, q: Option<FormId>) -> bool {
        let sync = &self.player.quest_sync;
        if sync.on == Some(false) {
            return false;
        }
        // The table is read only when a line is sat out.
        let lines = q.map(|q| if sync.sat_out.is_empty() { 0 } else { self.coop_questlines().lines_of(q) });
        sync.takes(lines)
    }

    /// The shown names of this character's shared quests part way through, in its lines
    /// `within` (0: every quest) (design §4). `own` is its own game's quests (at join).
    pub fn coop_sync_in_progress(&self, own: &BTreeMap<FormId, QuestState>, within: Mask) -> Vec<String> {
        let lines = (within != 0).then(|| self.coop_questlines());
        in_progress(own, &self.player.claims, &self.coop_host_ended, |q| lines.is_none_or(|l| l.lines_of(q) & within != 0))
            .into_iter()
            .map(|q| self.data.quests.get(&q).and_then(|d| d.name.clone()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| self.data.name(q)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coop_joint::ClaimedStage;

    fn facts<'a>(list: &'a [(u32, &'a str, &'a str, bool)]) -> Vec<QuestFacts<'a>> {
        list.iter().map(|(f, e, p, s)| QuestFacts { form: FormId(*f), edid: e, plugin: p, start_enabled: *s }).collect()
    }

    fn line(key: &str) -> Mask {
        bit(index(key).unwrap())
    }

    #[test]
    fn the_table_fits_a_mask_and_its_keys_are_unique() {
        assert!(LINES.len() <= 64);
        let keys: BTreeSet<&str> = LINES.iter().map(|l| l.key).collect();
        assert_eq!(keys.len(), LINES.len());
        for k in ["main", "house", "ncr", "legion", "yes-man", "dead-money", "honest-hearts", "old-world-blues", "lonesome-road"] {
            assert!(index(k).is_some(), "{k}");
        }
    }

    #[test]
    fn families_take_suffixes_but_not_other_numbers() {
        assert!(family("VMQHouse", "VMQHouse"));
        assert!(family("VMQHouse", "VMQHouse7"));
        assert!(family("VMQHouse", "vmqhouselockdown"));
        assert!(family("VMS46", "VMS46Timer"));
        assert!(family("VMS55", "VMS55a"));
        assert!(!family("VMS2", "VMS20"));
        assert!(!family("VMS55", "VMS5"));
        assert!(!family("VMQHouse", "VMQ01"));
    }

    /// An example: Ring-a-Ding-Ding! (Benny, the chip) is the trunk; House splits
    /// off at its first quest; what House's quests start is House's; the finale is every
    /// branch's; a DLC is its master's; a start-enabled holder is no one's.
    #[test]
    fn quests_group_into_lines_from_roots_families_plugins_and_what_they_start() {
        let list = [
            (0x01, "VMQTops", "FalloutNV.esm", false),
            (0x02, "VMQHouse1", "FalloutNV.esm", false),
            (0x03, "VMQHouse2", "FalloutNV.esm", false),
            (0x04, "VStreetSS6", "FalloutNV.esm", false),
            (0x05, "VDialogueMrHouse", "FalloutNV.esm", true),
            (0x06, "vHDBattleController", "FalloutNV.esm", false),
            (0x07, "VMS99", "FalloutNV.esm", false),
            (0x0100_0001, "NVDLC01MQ01", "DeadMoney.esm", false),
            (0x08, "VMQ05", "FalloutNV.esm", false),
            (0x09, "VMS29", "FalloutNV.esm", false),
            (0x0A, "VMS29a", "FalloutNV.esm", false),
        ];
        let f = facts(&list);
        let mut starts: BTreeMap<FormId, BTreeSet<FormId>> = BTreeMap::new();
        // Ring-a-Ding-Ding! starts House's first quest: House's root, not the trunk's.
        starts.entry(FormId(0x01)).or_default().insert(FormId(0x02));
        // House I starts a scene and the always-running dialogue holder.
        starts.entry(FormId(0x02)).or_default().extend([FormId(0x04), FormId(0x05)]);
        let g = group(&f, &starts);
        assert_eq!(g.lines_of(FormId(0x01)), line("main"));
        assert_eq!(g.lines_of(FormId(0x02)), line("house"));
        assert_eq!(g.lines_of(FormId(0x03)), line("house"), "a family member");
        assert_eq!(g.lines_of(FormId(0x04)), line("house"), "started by a House quest");
        assert_eq!(g.lines_of(FormId(0x05)), 0, "start-game enabled: no one's");
        assert_eq!(g.lines_of(FormId(0x06)), line("house") | line("ncr") | line("legion") | line("yes-man"), "the finale is every branch's");
        assert_eq!(g.lines_of(FormId(0x07)), 0, "in no line");
        assert_eq!(g.lines_of(FormId(0x0100_0001)), line("dead-money"));
        assert_eq!(g.lines_of(FormId(0x08)), line("ncr"));
        assert_eq!(g.lines_of(FormId(0x09)), line("kings"));
        assert_eq!(g.lines_of(FormId(0x0A)), line("cass"), "another line's root, not a Kings family member");
        assert_eq!(g.members(index("house").unwrap()), vec![FormId(0x02), FormId(0x03), FormId(0x04), FormId(0x06)]);
    }

    #[test]
    fn toggle_off_takes_nothing_and_on_takes_everything_not_sat_out() {
        let mut s = QuestSync::default();
        // Not chosen yet: rewards as before the toggle.
        assert!(s.takes(Some(line("house"))));
        assert!(s.takes(None));
        s.on = Some(false);
        assert!(!s.takes(Some(line("house"))));
        assert!(!s.takes(Some(0)), "a quest in no line");
        assert!(!s.takes(None), "script XP and plain hand-outs too");
        s.on = Some(true);
        assert!(s.takes(Some(line("house"))));
        assert!(s.takes(Some(0)));
        assert!(s.takes(None));
    }

    /// Per line: a sat-out line's stages, XP, caps and hand-outs are not taken; the trunk's
    /// and other lines' still are; a quest of several lines is sat out with any of them.
    #[test]
    fn a_sat_out_line_filters_its_quests_only() {
        let mut s = QuestSync { on: Some(true), ..QuestSync::default() };
        s.set_sat_out(index("house").unwrap(), true);
        assert_eq!(s.mask(), line("house"));
        assert!(s.sits_out(index("house").unwrap()));
        assert!(s.takes(Some(line("main"))), "Benny and the chip still sync");
        assert!(!s.takes(Some(line("house"))));
        let finale = line("house") | line("ncr") | line("legion") | line("yes-man");
        assert!(!s.takes(Some(finale)), "the finale with House sat out");
        assert!(s.takes(Some(line("cass"))));
        assert!(s.takes(None));
        s.set_sat_out(index("house").unwrap(), false);
        assert!(s.takes(Some(finale)));
        assert!(s.sat_out.is_empty());
        // A key a later table lacks is kept but means nothing.
        s.sat_out.insert("gone".into());
        assert_eq!(s.mask(), 0);
    }

    fn own(list: &[(u32, bool, bool)]) -> BTreeMap<FormId, QuestState> {
        list.iter().map(|(q, completed, failed)| (FormId(*q), QuestState { completed: *completed, failed: *failed, stages_done: [10].into(), ..QuestState::default() })).collect()
    }

    #[test]
    fn in_progress_finds_claimed_quests_not_ended_anywhere() {
        let mut ledger = ClaimLedger::default();
        for q in [0x10, 0x11, 0x12, 0x13] {
            ledger.stages.insert(FormId(q), vec![ClaimedStage { stage: 20, line: None }]);
        }
        // Replayed earlier, still running in its own game.
        ledger.done.insert(FormId(0x14), [10].into());
        // An empty entry is no claim.
        ledger.stages.insert(FormId(0x15), Vec::new());
        let mine = own(&[(0x11, true, false), (0x12, false, true)]);
        let host_ended = [(FormId(0x13), false)];
        assert_eq!(in_progress(&mine, &ledger, &host_ended, |_| true), vec![FormId(0x10), FormId(0x14)]);
        // Within a line.
        assert_eq!(in_progress(&mine, &ledger, &host_ended, |q| q == FormId(0x14)), vec![FormId(0x14)]);
        // Nothing claimed: no warning.
        assert!(in_progress(&mine, &ClaimLedger::default(), &[], |_| true).is_empty());
    }

    #[test]
    fn the_warning_names_the_quests_and_what_happens() {
        let w = warning(&["Heartache by the Number".into(), "Ring-a-Ding-Ding!".into()], None);
        assert!(w.contains("Heartache by the Number, Ring-a-Ding-Ding!"));
        assert!(w.contains("stay at their current stage"));
        assert!(w.contains("stop following the host"));
        let w = warning(&["The House Always Wins, I".into()], Some("Mr. House"));
        assert!(w.contains("If you sit out Mr. House now"));
        assert!(w.contains("stays at its current stage"));
    }

    #[test]
    fn the_choice_round_trips_through_a_save() {
        let mut s = QuestSync { on: Some(false), ..QuestSync::default() };
        s.set_sat_out(index("house").unwrap(), true);
        s.set_sat_out(index("dead-money").unwrap(), true);
        let json = serde_json::to_string(&s).unwrap();
        let back: QuestSync = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.mask(), line("house") | line("dead-money"));
        // Nothing chosen writes nothing; an older save without the field reads as default.
        assert_eq!(serde_json::to_string(&QuestSync::default()).unwrap(), "{}");
        assert_eq!(serde_json::from_str::<QuestSync>("{}").unwrap(), QuestSync::default());
    }
}
