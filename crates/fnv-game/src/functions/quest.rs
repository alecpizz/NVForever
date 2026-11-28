//! Quest functions. Behaviour: docs/spec-notes/scripts.md §5 (captures named there).

use super::{none, yes, Args, Ctx};
use crate::game::{Game, InstKey};
use fnv_script::vm::Value;
use fnv_sim::FormId;

fn quest(g: &Game, a: &Args, i: usize) -> Result<FormId, String> {
    let q = a.form(i)?;
    if g.data.is_quest(q) {
        Ok(q)
    } else {
        Err(format!("{} is not a quest", g.data.name(q)))
    }
}

/// A co-op guest's world reads the host's story: quest `q` as the host has it, for a
/// placed reference's script (`crate::coop::HostStory`), outside conditions.
fn host<'g>(g: &'g Game, ctx: &Ctx, q: FormId) -> Option<&'g crate::coop::HostQuestState> {
    if ctx.in_condition {
        return None;
    }
    g.host_story(ctx.inst)?.quests.get(&q)
}

/// The quest a function names, or the running quest when the argument is absent
/// (`GetQuestCompleted` documents an optional quest).
fn quest_or_own(g: &Game, ctx: &Ctx, a: &Args) -> Result<FormId, String> {
    match a.opt_form(0) {
        Some(_) => quest(g, a, 0),
        None => ctx.quest.ok_or_else(|| "no quest".to_string()),
    }
}

pub fn set_stage(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    let stage = a.int(1)?;
    g.set_stage(q, stage);
    none()
}

pub fn get_stage(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    if let Some(h) = host(g, ctx, q) {
        return Ok(Value::Num(h.stage as f64));
    }
    Ok(Value::Num(g.quests.get(&q).map(|s| s.stage()).unwrap_or(0) as f64))
}

pub fn get_stage_done(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    let stage = a.int(1)?;
    if let Some(h) = host(g, ctx, q) {
        return yes(h.stage_done(stage));
    }
    yes(g.quests.get(&q).is_some_and(|s| s.stages_done.contains(&stage)))
}

pub fn start_quest(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    g.start_quest(q);
    none()
}

/// The quest stops; a running script finishes its current run [documented: GECK wiki
/// StopQuest].
pub fn stop_quest(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    g.stop_quest(q);
    none()
}

pub fn get_quest_running(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    yes(g.quests.get(&q).is_some_and(|s| s.running))
}

/// 1 for a completed or failed quest [documented: catalogue summary; observed: a Fail
/// Quest stage makes it 1, `L5.quest.stage_order`].
pub fn get_quest_completed(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest_or_own(g, ctx, a)?;
    if let Some(h) = host(g, ctx, q) {
        return yes(h.ended);
    }
    yes(g.quests.get(&q).is_some_and(|s| s.completed || s.failed))
}

pub fn set_objective_displayed(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    let (i, on) = (a.int(1)?, a.num_or(2, 1.0) != 0.0);
    g.set_objective(q, i, Some(on), None);
    none()
}

pub fn set_objective_completed(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    let (i, on) = (a.int(1)?, a.num_or(2, 1.0) != 0.0);
    g.set_objective(q, i, None, Some(on));
    none()
}

pub fn get_objective_displayed(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    let i = a.int(1)?;
    yes(g.quests.get(&q).and_then(|s| s.objectives.get(&i)).is_some_and(|o| o.displayed))
}

pub fn get_objective_completed(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    let i = a.int(1)?;
    yes(g.quests.get(&q).and_then(|s| s.objectives.get(&i)).is_some_and(|o| o.completed))
}

/// Conditions only: quest and variable index (xEdit `ptVariableName`). A null quest
/// (FormID 0, as on Papa Khan's goodbyes INFO 0x0014B595-7) reads 0 [observed
/// 2026-10-30, capture o4_00_probe: a placed copy of Papa Khan told to SayTo GOODBYE chose
/// 0x0014B595, whose `GetQuestVariable <null> 1 == 0` passed; observer-checks.md check 35].
pub fn get_quest_variable(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    if ctx.in_condition && a.is_null_form(0) {
        return Ok(Value::ZERO);
    }
    let q = quest(g, a, 0)?;
    let i = a.int(1)? as u16;
    Ok(g.var(InstKey::Quest { quest: q }, i))
}

/// Conditions only: a reference's script variable by index.
pub fn get_script_variable(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = a.opt_form(0).map(Ok).unwrap_or_else(|| ctx.subject())?;
    let i = a.int(1)? as u16;
    let (key, _) = g.instance_of(r).ok_or_else(|| format!("{} runs no script", g.data.name(r)))?;
    Ok(g.var(key, i))
}

pub fn set_quest_delay(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    let d = a.num(1)?;
    // A delay of 0 is the INI default, 5 s, not the quest's own DATA delay [documented:
    // GECK wiki SetQuestDelay, "Setting delay to 0 tells the quest to process at the INI
    // setting default (5 seconds)"].
    g.quest(q).delay = Some(if d > 0.0 { d } else { crate::quest::DEFAULT_QUEST_DELAY });
    none()
}

/// Every objective the quest defines is marked completed (not displayed)
/// [documented: GECK wiki CompleteAllObjectives; the display rule of
/// SetObjectiveCompleted, scripts.md §5].
pub fn complete_all_objectives(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    // Iterate in QOBJ record order rather than ascending index order
    // [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE Plugin 184,
    // 2026-11-23, GSDocMitchellHouse (0x00103DF9), 10 official base masters, probe
    // `w1_3656_objectives_unholster_imod` in #3679 (`StartQuest 0x000E282D`,
    // `SetObjectiveDisplayed 50 1`, `SetObjectiveDisplayed 5 1`, `CompleteAllObjectives`
    // -> fires `ShowOff:OnCompleteObjective` only for displayed objectives in QOBJ record
    // order: 50 then 5, not 5 then 50)].
    let def = &g.data.quests[&q];
    let ids: Vec<i32> = if def.objective_order.is_empty() {
        def.objectives.keys().copied().collect()
    } else {
        let mut seen = std::collections::HashSet::new();
        def.objective_order.iter().copied().filter(|i| seen.insert(*i)).collect()
    };
    for i in ids {
        // Also on a completed or failed quest (`Game::set_objective_whatever`).
        g.set_objective_whatever(q, i, None, Some(true));
    }
    none()
}

/// The quest's stages, objectives, completed/failed state and script variables go back
/// to their start [documented: catalogue summary of ResetQuest; observed: o41, the
/// variables read 0 after it]. A SetQuestDelay override is kept [observed: o41,
/// VMQ03aTimer at SetQuestDelay 2 ran every 2.0 s after the reset, not at its DATA 0.5 s;
/// observer check 41, issue #1109]. Called from another script the quest stops
/// [observed: o41, GetQuestRunning 0 and no GameMode run until StartQuest, from a job
/// script; layer5-captures.md §5.7, issue #1232]. A reset from the quest's own script or
/// stage result leaves it running [hypothesis, not observed: the Emergency Radio resets
/// itself in its MenuMode and goes on to the supply drop].
pub fn reset_quest(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    let (running, delay) = g.quests.get(&q).map_or((false, None), |s| (s.running, s.delay));
    let running = running && ctx.quest == Some(q);
    g.quests.insert(q, crate::quest::QuestState { running, delay, ..Default::default() });
    g.instances.remove(&InstKey::Quest { quest: q });
    none()
}

/// Marks the quest completed (the Pip-Boy part is presentation) [documented: catalogue
/// summary]. It does not stop the quest [hypothesis, as the Complete Quest stage flag,
/// layer5-captures.md §5.3].
pub fn complete_quest(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = quest(g, a, 0)?;
    let s = g.quest(q);
    if !s.completed {
        s.completed = true;
        g.quest_completed(q);
    }
    none()
}
