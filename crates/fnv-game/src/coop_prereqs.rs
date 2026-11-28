//! Quest prerequisites read from the data, for joint progression's first claim of a
//! quest (coop-shared-progress.md §3.3; `crate::coop_joint`).
//!
//! [design] The reading: a quest's ways in are the dialogue lines whose result script
//! starts it (`StartQuest Q`, or `SetStage Q <its first stage>`). What each line's
//! conditions ask of *other* quests is what that way in asks: `GetQuestCompleted X == 1`,
//! `GetStageDone X s == 1`, `GetStage X >= s` (and `> s`). Conditions joined with OR form
//! one group that any member satisfies; a group with anything else in it (an actor value,
//! a global, a test of Q itself, a negative test) can be met some other way, so it asks
//! nothing. The guest's own game meets a quest's prerequisites when any way in has all its
//! groups met. A quest with no such line (a script starts it) asks nothing beyond the
//! continuity rules of [`crate::coop_joint::check`].
//!
//! `fnvtool coop prereqs` prints the table for review.

use crate::cond::{Cmp, Condition, Operand};
use crate::data::GameData;
use crate::quest::QuestState;
use fnv_script::ir::{Arg, Expr, Stmt, Token};
use fnv_script::unit::RefSlot;
use fnv_script::vm::{ArgValue, Value};
use fnv_script::ScriptUnit;
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet};

/// One thing a way in asks of another quest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Need {
    StageDone { quest: FormId, stage: i32 },
    /// `GetStage X >= stage`: a stage at or past it done (or the quest ended).
    StageAtLeast { quest: FormId, stage: i32 },
    Completed { quest: FormId },
}

impl Need {
    pub fn holds(&self, own: &BTreeMap<FormId, QuestState>) -> bool {
        match self {
            Need::StageDone { quest, stage } => own.get(quest).is_some_and(|s| s.stages_done.contains(stage)),
            Need::StageAtLeast { quest, stage } => own.get(quest).is_some_and(|s| s.completed || s.stages_done.iter().any(|d| d >= stage)),
            Need::Completed { quest } => own.get(quest).is_some_and(|s| s.completed),
        }
    }
}

/// A way in: groups that must all hold, each met by any of its needs.
pub type Way = Vec<Vec<Need>>;

/// Every quest's ways in that ask something (a quest absent asks nothing).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prereqs {
    pub ways: BTreeMap<FormId, Vec<Way>>,
    /// Quests with a starting line but whose lines ask nothing: met by anyone.
    pub open: BTreeSet<FormId>,
}

impl Prereqs {
    /// Whether the guest's own game `own` meets `q`'s prerequisites.
    pub fn met(&self, q: FormId, own: &BTreeMap<FormId, QuestState>) -> bool {
        if self.open.contains(&q) {
            return true;
        }
        match self.ways.get(&q) {
            None => true,
            Some(ways) => ways.iter().any(|w| w.iter().all(|g| g.iter().any(|n| n.holds(own)))),
        }
    }
}

/// The quests a unit's calls start (`StartQuest Q`, `SetStage Q <first stage>`).
pub(crate) fn starts(data: &GameData, unit: &ScriptUnit, program: &fnv_script::Program, out: &mut BTreeSet<FormId>) {
    fn call(data: &GameData, unit: &ScriptUnit, c: &fnv_script::ir::Call, out: &mut BTreeSet<FormId>) {
        let Some(name) = data.table.get(c.opcode).map(|f| f.name.as_str()) else { return };
        let set_stage = name.eq_ignore_ascii_case("SetStage");
        if !set_stage && !name.eq_ignore_ascii_case("StartQuest") {
            return;
        }
        let Some(Arg::Ref(r)) = c.args.first() else { return };
        let Some(RefSlot::Form(f)) = unit.slot(*r) else { return };
        let q = FormId(f);
        let Some(def) = data.quests.get(&q) else { return };
        if set_stage {
            let first = def.stages.keys().next().copied();
            let stage = match c.args.get(1) {
                Some(Arg::Int(s)) => Some(*s),
                Some(Arg::Float(s)) => Some(*s as i32),
                _ => None,
            };
            if stage.is_none() || stage != first {
                return;
            }
        }
        out.insert(q);
    }
    fn expr(data: &GameData, unit: &ScriptUnit, e: &Expr, out: &mut BTreeSet<FormId>) {
        for t in &e.0 {
            if let Token::Call { call: c } = t {
                call(data, unit, c, out);
            }
        }
    }
    fn stmts(data: &GameData, unit: &ScriptUnit, body: &[Stmt], out: &mut BTreeSet<FormId>) {
        for s in body {
            match s {
                Stmt::Call { call: c } => call(data, unit, c, out),
                Stmt::Set { expr: e, .. } => expr(data, unit, e, out),
                Stmt::If { arms, otherwise } => {
                    for a in arms {
                        expr(data, unit, &a.cond, out);
                        stmts(data, unit, &a.body, out);
                    }
                    if let Some(o) = otherwise {
                        stmts(data, unit, o, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::ForEach { body, .. } => stmts(data, unit, body, out),
                _ => {}
            }
        }
    }
    stmts(data, unit, &program.top, out);
    for b in &program.blocks {
        stmts(data, unit, &b.body, out);
    }
}

/// What one condition asks of another quest; `None` when it is not such a need.
fn need(data: &GameData, c: &Condition, started: &BTreeSet<FormId>) -> Option<Need> {
    let name = data.table.get(c.opcode())?.name.as_str();
    let Operand::Value(v) = c.rhs else { return None };
    let quest = match c.params.first()? {
        ArgValue::Value(Value::Ref(q)) if *q != 0 => FormId(*q),
        _ => return None,
    };
    if started.contains(&quest) {
        return None;
    }
    let stage = || match c.params.get(1) {
        Some(ArgValue::Value(Value::Num(s))) => Some(*s as i32),
        _ => None,
    };
    // "Is 1" in the forms the masters write it.
    let true_test = matches!((c.cmp, v), (Cmp::Eq, v) if v == 1.0) || matches!((c.cmp, v), (Cmp::Ne, v) if v == 0.0) || matches!((c.cmp, v), (Cmp::Ge, v) if v == 1.0) || matches!((c.cmp, v), (Cmp::Gt, v) if v == 0.0);
    if name.eq_ignore_ascii_case("GetQuestCompleted") && true_test {
        Some(Need::Completed { quest })
    } else if name.eq_ignore_ascii_case("GetStageDone") && true_test {
        Some(Need::StageDone { quest, stage: stage()? })
    } else if name.eq_ignore_ascii_case("GetStage") {
        match c.cmp {
            Cmp::Ge if v > 0.0 => Some(Need::StageAtLeast { quest, stage: v as i32 }),
            Cmp::Gt => Some(Need::StageAtLeast { quest, stage: v as i32 + 1 }),
            Cmp::Eq if v > 0.0 => Some(Need::StageAtLeast { quest, stage: v as i32 }),
            _ => None,
        }
    } else {
        None
    }
}

/// A line's conditions as a way in (module notes).
fn way(data: &GameData, conditions: &[Condition], started: &BTreeSet<FormId>) -> Way {
    let mut out = Vec::new();
    let mut group: Vec<Option<Need>> = Vec::new();
    for c in conditions {
        group.push(need(data, c, started));
        if !c.or {
            if !group.is_empty() && group.iter().all(Option::is_some) {
                out.push(group.drain(..).flatten().collect());
            }
            group.clear();
        }
    }
    if !group.is_empty() && group.iter().all(Option::is_some) {
        out.push(group.into_iter().flatten().collect());
    }
    out
}

/// The load order's prerequisites (module notes). Pure: the same data gives the same
/// table.
pub fn compute(data: &GameData) -> Prereqs {
    let mut p = Prereqs::default();
    let mut started = BTreeSet::new();
    for d in data.infos.values() {
        started.clear();
        for u in [d.begin, d.end].into_iter().flatten() {
            if let Some(c) = data.units.get(u) {
                starts(data, &c.unit, &c.program, &mut started);
            }
        }
        if started.is_empty() {
            continue;
        }
        let w = way(data, &d.conditions, &started);
        for q in &started {
            if w.is_empty() {
                p.open.insert(*q);
            } else {
                p.ways.entry(*q).or_default().push(w.clone());
            }
        }
    }
    for q in &p.open {
        p.ways.remove(q);
    }
    for ways in p.ways.values_mut() {
        ways.sort();
        ways.dedup();
    }
    p
}

impl crate::game::Game<'_> {
    /// This load order's quest prerequisites, read on first use.
    pub fn coop_prereqs(&self) -> &Prereqs {
        self.coop_prereqs_cache.get_or_init(|| compute(self.data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own(stages: &[(u32, &[i32], bool)]) -> BTreeMap<FormId, QuestState> {
        stages
            .iter()
            .map(|(q, s, done)| (FormId(*q), QuestState { stages_done: s.iter().copied().collect(), completed: *done, ..QuestState::default() }))
            .collect()
    }

    #[test]
    fn a_way_in_is_met_when_each_group_has_one_need_met() {
        const Q: FormId = FormId(0x100);
        let mut p = Prereqs::default();
        // One way: (X completed) and (Y stage 20 done or Z at stage 30+); another: W completed.
        p.ways.insert(
            Q,
            vec![
                vec![vec![Need::Completed { quest: FormId(0x10) }], vec![Need::StageDone { quest: FormId(0x20), stage: 20 }, Need::StageAtLeast { quest: FormId(0x30), stage: 30 }]],
                vec![vec![Need::Completed { quest: FormId(0x40) }]],
            ],
        );
        assert!(!p.met(Q, &own(&[])));
        assert!(!p.met(Q, &own(&[(0x10, &[], true)])), "the second group is not met");
        assert!(p.met(Q, &own(&[(0x10, &[], true), (0x30, &[10, 40], false)])));
        assert!(p.met(Q, &own(&[(0x10, &[], true), (0x20, &[20], false)])));
        assert!(p.met(Q, &own(&[(0x40, &[], true)])), "any way in will do");
        // A quest with no way that asks anything, or an open line: met by anyone.
        assert!(p.met(FormId(0x999), &own(&[])));
        p.open.insert(Q);
        assert!(p.met(Q, &own(&[])));
    }
}
