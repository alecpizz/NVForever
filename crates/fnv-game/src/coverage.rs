//! Script coverage of headless runs (docs/handoff/mq-walkthrough-2026-10-27.md §5): what
//! a run reached of the load order's quests, scripts and dialogue, and the report of many
//! runs against the masters.
//!
//! A run with [`Game::coverage`] set records the quest stages set, the script blocks run
//! (unit and block), the dialogue lines (INFO) said and the result scripts run (quest
//! stage, INFO, terminal, package and the rest). [`Coverage::log`] writes them as a
//! `fnv.script.coverage-log/1` document whose FormIDs name their plugin
//! (`FalloutNV.esm:10E908`), so that runs over different load orders merge; [`report`]
//! merges any number of logs and compares them with a load order's records.

use crate::data::GameData;
use crate::game::Game;
use fnv_script::ir::{FuncId, XExpr};
use fnv_script::unit::RefSlot;
use fnv_script::vm::{ArgValue, Value};
use fnv_script::{Arg, Call, Expr, RefIndex, Stmt, Token, UnitOwner};
use fnv_sim::FormId;
use serde_json::{json, Map, Value as Json};
use std::collections::{BTreeMap, BTreeSet};

pub const LOG_FORMAT: &str = "fnv.script.coverage-log/1";
pub const REPORT_FORMAT: &str = "fnv.script.coverage/1";

/// What one run reached, in the run's own FormIDs and unit keys.
#[derive(Debug, Default, Clone)]
pub struct Coverage {
    pub stages: BTreeSet<(FormId, i32)>,
    /// Unit key and the block's place among the unit's blocks.
    pub blocks: BTreeSet<(String, usize)>,
    pub infos: BTreeSet<FormId>,
    /// Unit keys of the result scripts run.
    pub results: BTreeSet<String>,
    /// Quests the run marked completed: by a Complete Quest stage or by the script
    /// function CompleteQuest (not by a chain's given state, not by a fail stage).
    pub completed: BTreeSet<FormId>,
}

impl Game<'_> {
    pub(crate) fn cover_block(&mut self, key: impl FnOnce() -> String, index: usize) {
        if let Some(c) = self.coverage.as_mut() {
            c.blocks.insert((key(), index));
        }
    }

    pub(crate) fn cover_result(&mut self, key: impl FnOnce() -> String) {
        if let Some(c) = self.coverage.as_mut() {
            c.results.insert(key());
        }
    }

    pub(crate) fn cover_stage(&mut self, quest: FormId, stage: i32) {
        if let Some(c) = self.coverage.as_mut() {
            c.stages.insert((quest, stage));
        }
    }

    pub(crate) fn cover_completed(&mut self, quest: FormId) {
        if let Some(c) = self.coverage.as_mut() {
            c.completed.insert(quest);
        }
    }

    pub(crate) fn cover_info(&mut self, info: FormId) {
        if let Some(c) = self.coverage.as_mut() {
            c.infos.insert(info);
        }
    }
}

/// A FormID by its plugin: `Plugin.esm:XXXXXX` (the plugin of the load-order index in
/// the top byte, the object ID below it).
pub fn form_name(load_order: &[String], id: u32) -> String {
    let i = (id >> 24) as usize;
    let plugin = load_order.get(i).map(String::as_str).unwrap_or("?");
    format!("{plugin}:{:06X}", id & 0x00FF_FFFF)
}

/// A unit key without its plugin and with every FormID by its plugin: the winning unit
/// of an owner is the same unit whichever plugin last changed it, so runs over load
/// orders with and without an add-on that overrides it name it alike.
pub fn unit_name(load_order: &[String], key: &str) -> String {
    let owner = key.split_once('|').map(|(_, o)| o).unwrap_or(key);
    let mut out = String::with_capacity(owner.len() + 16);
    let mut rest = owner;
    while let Some(at) = rest.find("0x") {
        let hex = &rest[at + 2..];
        let n = hex.bytes().take_while(u8::is_ascii_hexdigit).count();
        out.push_str(&rest[..at]);
        match (n == 8).then(|| u32::from_str_radix(&hex[..8], 16).ok()).flatten() {
            Some(id) => out.push_str(&form_name(load_order, id)),
            None => out.push_str(&rest[at..at + 2 + n]),
        }
        rest = &hex[n..];
    }
    out.push_str(rest);
    out
}

impl Coverage {
    /// The run's coverage log (`fnv.script.coverage-log/1`); `title` names the run.
    pub fn log(&self, data: &GameData, title: &str) -> Json {
        let lo = &data.load_order;
        let keys: BTreeSet<&str> = self.blocks.iter().map(|(k, _)| k.as_str()).collect();
        let units: BTreeMap<String, &crate::data::Compiled> =
            data.units.iter().map(|u| (u.key(), u)).filter(|(k, _)| keys.contains(k.as_str())).collect();
        let block_name = |key: &str, i: usize| {
            units
                .get(key)
                .and_then(|u| u.program.blocks.get(i))
                .and_then(|b| data.table.block_name(b.event))
                .unwrap_or("?")
                .to_string()
        };
        json!({
            "format": LOG_FORMAT,
            "title": title,
            "load_order": lo,
            "stages": self.stages.iter().map(|(q, s)| json!([form_name(lo, q.0), s])).collect::<Vec<_>>(),
            "blocks": self.blocks.iter().map(|(k, i)| json!([unit_name(lo, k), i, block_name(k, *i)])).collect::<Vec<_>>(),
            "infos": self.infos.iter().map(|i| form_name(lo, i.0)).collect::<Vec<_>>(),
            "results": self.results.iter().map(|k| unit_name(lo, k)).collect::<Vec<_>>(),
            "completed": self.completed.iter().map(|q| form_name(lo, q.0)).collect::<Vec<_>>(),
        })
    }
}

/// Logs merged: everything any of them reached, in plugin-named form.
#[derive(Debug, Default)]
struct Reached {
    stages: BTreeSet<(String, i64)>,
    blocks: BTreeSet<(String, u64)>,
    infos: BTreeSet<String>,
    results: BTreeSet<String>,
    completed: BTreeSet<String>,
}

impl Reached {
    fn add(&mut self, log: &Json) -> Result<(), String> {
        if log["format"] != LOG_FORMAT {
            return Err(format!("not a {LOG_FORMAT} document"));
        }
        let arr = |k: &str| log[k].as_array().cloned().unwrap_or_default();
        for s in arr("stages") {
            self.stages.insert((s[0].as_str().unwrap_or_default().to_string(), s[1].as_i64().unwrap_or_default()));
        }
        for b in arr("blocks") {
            self.blocks.insert((b[0].as_str().unwrap_or_default().to_string(), b[1].as_u64().unwrap_or_default()));
        }
        self.infos.extend(arr("infos").iter().filter_map(|i| i.as_str().map(str::to_string)));
        self.results.extend(arr("results").iter().filter_map(|i| i.as_str().map(str::to_string)));
        // Older logs have no "completed": their quests count by the stage flags alone.
        self.completed.extend(arr("completed").iter().filter_map(|i| i.as_str().map(str::to_string)));
        Ok(())
    }
}

/// The main quest's Editor ID prefixes in FalloutNV.esm [observed: the quests the
/// main-quest walkthroughs run: VCG (Goodsprings start), VMQ, VHD (Hoover Dam), VEnding].
pub(crate) const MAIN_PREFIXES: [&str; 4] = ["vmq", "vhd", "vending", "vcg0"];

/// The four story add-ons' plugins.
const STORY_ADDONS: [&str; 4] = ["deadmoney.esm", "honesthearts.esm", "oldworldblues.esm", "lonesomeroad.esm"];

/// Rank of a quest in the untouched list: 0 main quest, 1 faction, 2 add-on, 3 the rest.
/// A faction quest is a base-game quest whose stage scripts change a reputation
/// [hypothesis: a working rule for ranking, not a category of the data].
fn rank(data: &GameData, q: FormId, edid: &str) -> u8 {
    let plugin = data.load_order.get((q.0 >> 24) as usize).map(|p| p.to_ascii_lowercase()).unwrap_or_default();
    let e = edid.to_ascii_lowercase();
    if plugin == "falloutnv.esm" && MAIN_PREFIXES.iter().any(|p| e.starts_with(p)) {
        return 0;
    }
    if STORY_ADDONS.contains(&plugin.as_str()) || plugin != "falloutnv.esm" {
        return 2;
    }
    let reputation = data.quests.get(&q).is_some_and(|d| {
        d.stages.values().flatten().filter_map(|e| e.unit).any(|u| {
            data.unit(u).unit.source.as_deref().is_some_and(|s| s.to_ascii_lowercase().contains("reputation"))
        })
    });
    if reputation {
        1
    } else {
        3
    }
}

const RANKS: [&str; 4] = ["main", "faction", "add-on", "other"];

/// The actor a line belongs to: its fixed speaker, else the actor of its own GetIsID
/// condition (dialogue.rs `can_say_as`).
pub(crate) fn line_actor(data: &GameData, info: &crate::data::InfoDef) -> Option<FormId> {
    info.conditions
        .iter()
        .find(|c| c.function == 0x48 && matches!(c.run_on, crate::cond::RunOn::Subject))
        .and_then(|c| match c.params.first() {
            Some(ArgValue::Value(Value::Ref(f))) => Some(FormId(*f)),
            _ => None,
        })
        .or(info.speaker)
        .filter(|f| data.sig(*f).is_some())
}

/// The kind of a result script, by its owner.
fn result_kind(o: &UnitOwner) -> Option<&'static str> {
    Some(match o {
        UnitOwner::QuestStage { .. } => "stage",
        UnitOwner::Info { .. } => "info",
        UnitOwner::Terminal { .. } => "terminal",
        UnitOwner::Package { .. } => "package",
        UnitOwner::Script { .. } | UnitOwner::Perk { .. } | UnitOwner::Patrol { .. } | UnitOwner::Other { .. } => return None,
    })
}

/// `SetStage quest stage` [documented: GECK wiki SetStage; functions/mod.rs].
const SET_STAGE: u16 = 0x1039;

/// One place in the data that sets a quest stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setter {
    /// What holds the call: `script` (an SCPT record), `stage` (a quest stage's result
    /// script), `info_begin` / `info_end`, `terminal`, `package_begin` / `package_end` /
    /// `package_change`, `perk_script`, `patrol`, `perk` (a perk's Quest + Stage effect,
    /// which the engine runs when the rank is gained: perks.rs `gain_rank`), or the
    /// lower-case signature of any other record with a unit.
    pub kind: String,
    /// The record holding it (the quest, for a stage result script).
    pub form: FormId,
    /// The stage, for a stage result script.
    pub stage: Option<i64>,
}

/// SetStage calls found in one unit: the quest's reference slot (None when the quest
/// argument is not a reference list entry) and the stage.
type StageCalls = Vec<(Option<RefIndex>, StageArg)>;

/// Which quest stages anything in the load order can set. A setter is a `SetStage` call
/// in any compiled unit whose quest is a form in the unit's reference list (a stage given
/// by a variable puts the quest in `by_variable`), or a perk's Quest + Stage effect. No
/// condition function sets a stage in the engine (conditions only read: GetStage,
/// GetStageDone), so conditions are not setters. A stage's own result script setting that
/// stage again is not a setter of it.
#[derive(Debug, Default)]
pub struct Setters {
    pub literal: BTreeMap<(FormId, i32), Vec<Setter>>,
    /// Quests set to a stage held in a variable (or an expression), with the setters.
    pub by_variable: BTreeMap<FormId, Vec<Setter>>,
    /// SetStage calls whose quest is a variable: no quest to credit.
    pub quest_by_variable: usize,
}

fn setter_kind(o: &UnitOwner) -> String {
    use fnv_script::unit::PackageEvent;
    match o {
        UnitOwner::Script { .. } => "script".into(),
        UnitOwner::QuestStage { .. } => "stage".into(),
        UnitOwner::Info { end: false, .. } => "info_begin".into(),
        UnitOwner::Info { end: true, .. } => "info_end".into(),
        UnitOwner::Terminal { .. } => "terminal".into(),
        UnitOwner::Package { event, .. } => match event {
            PackageEvent::Begin => "package_begin".into(),
            PackageEvent::End => "package_end".into(),
            PackageEvent::Change => "package_change".into(),
        },
        UnitOwner::Perk { .. } => "perk_script".into(),
        UnitOwner::Patrol { .. } => "patrol".into(),
        UnitOwner::Other { sig, .. } => sig.to_ascii_lowercase(),
    }
}

/// A SetStage call's stage argument.
enum StageArg {
    Literal(i32),
    Variable,
}

fn plain_call(c: &Call, out: &mut StageCalls) {
    if c.opcode != SET_STAGE {
        return;
    }
    let q = match c.args.first() {
        Some(Arg::Ref(r)) => Some(*r),
        _ => None,
    };
    let s = match c.args.get(1) {
        Some(Arg::Int(n)) => StageArg::Literal(*n),
        Some(Arg::Float(f)) if f.fract() == 0.0 => StageArg::Literal(*f as i32),
        _ => StageArg::Variable,
    };
    out.push((q, s));
}

fn expr_calls(e: &Expr, out: &mut StageCalls) {
    for t in &e.0 {
        match t {
            Token::Call { call } => plain_call(call, out),
            Token::X { e } => x_calls(e, out),
            _ => {}
        }
    }
}

fn x_calls(e: &XExpr, out: &mut StageCalls) {
    match e {
        XExpr::Call { call } => {
            let is = match &call.func {
                FuncId::Opcode(o) => *o == SET_STAGE,
                FuncId::Name(n) => n.eq_ignore_ascii_case("setstage"),
            };
            if is {
                let q = match call.args.first() {
                    Some(XExpr::Ref { r }) => Some(*r),
                    _ => None,
                };
                let s = match call.args.get(1) {
                    Some(XExpr::Num { value, .. }) if value.fract() == 0.0 => StageArg::Literal(*value as i32),
                    _ => StageArg::Variable,
                };
                out.push((q, s));
            }
            if let Some(o) = &call.on {
                x_calls(o, out);
            }
            call.args.iter().for_each(|a| x_calls(a, out));
        }
        XExpr::Unary { e, .. } => x_calls(e, out),
        XExpr::Binary { l, r, .. } => {
            x_calls(l, out);
            x_calls(r, out);
        }
        XExpr::Member { l, .. } => x_calls(l, out),
        XExpr::Assign { target, value, .. } => {
            x_calls(target, out);
            x_calls(value, out);
        }
        // Lambda bodies are not reached by Program::walk: walk them here.
        XExpr::Lambda { body, value, .. } => {
            body.iter().for_each(|s| walk_calls(s, out));
            if let Some(v) = value {
                x_calls(v, out);
            }
        }
        _ => {}
    }
}

/// A statement's SetStage calls and those of the statements inside it.
fn walk_calls(st: &Stmt, out: &mut StageCalls) {
    stmt_calls(st, out);
    match st {
        Stmt::If { arms, otherwise } => {
            arms.iter().flat_map(|a| &a.body).for_each(|s| walk_calls(s, out));
            otherwise.iter().flatten().for_each(|s| walk_calls(s, out));
        }
        Stmt::While { body, .. } | Stmt::ForEach { body, .. } => body.iter().for_each(|s| walk_calls(s, out)),
        _ => {}
    }
}

/// A statement's own SetStage calls: the call it is, and calls in its expressions.
fn stmt_calls(st: &Stmt, out: &mut StageCalls) {
    match st {
        Stmt::Call { call } => plain_call(call, out),
        Stmt::Set { expr, .. } => expr_calls(expr, out),
        Stmt::If { arms, .. } => arms.iter().for_each(|a| expr_calls(&a.cond, out)),
        Stmt::Unmatched { cond: Some(c), .. } => expr_calls(c, out),
        Stmt::Let { expr } | Stmt::Eval { expr } => x_calls(expr, out),
        Stmt::While { cond, .. } => x_calls(cond, out),
        Stmt::ForEach { head, .. } => {
            x_calls(&head.var, out);
            x_calls(&head.source, out);
        }
        _ => {}
    }
}

fn add_setter(v: &mut Vec<Setter>, s: Setter) {
    if !v.contains(&s) {
        v.push(s);
    }
}

/// The stage setters of `data` ([`Setters`]).
pub fn stage_setters(data: &GameData) -> Setters {
    let mut out = Setters::default();
    for u in &data.units {
        let mut calls = StageCalls::new();
        u.program.walk(&mut |st| stmt_calls(st, &mut calls));
        if calls.is_empty() {
            continue;
        }
        let owner = &u.unit.owner;
        let setter = || Setter {
            kind: setter_kind(owner),
            form: FormId(owner.form()),
            stage: match owner {
                UnitOwner::QuestStage { stage, .. } => Some(*stage),
                _ => None,
            },
        };
        for (q, s) in calls {
            let quest = match q.and_then(|r| u.unit.slot(r)) {
                Some(RefSlot::Form(f)) if data.is_quest(FormId(f)) => FormId(f),
                _ => {
                    out.quest_by_variable += 1;
                    continue;
                }
            };
            match s {
                StageArg::Literal(n) => {
                    let own = matches!(owner, UnitOwner::QuestStage { form, stage, .. } if *form == quest.0 && *stage == i64::from(n));
                    if !own {
                        add_setter(out.literal.entry((quest, n)).or_default(), setter());
                    }
                }
                StageArg::Variable => add_setter(out.by_variable.entry(quest).or_default(), setter()),
            }
        }
    }
    let mut perks: Vec<_> = data.perks.iter().collect();
    perks.sort_by_key(|(id, _)| **id);
    for (id, p) in perks {
        for e in &p.entries {
            if let crate::data::PerkEffect::QuestStage { quest, stage } = e.effect {
                if data.is_quest(quest) {
                    let s = Setter { kind: "perk".into(), form: *id, stage: None };
                    add_setter(out.literal.entry((quest, i32::from(stage))).or_default(), s);
                }
            }
        }
    }
    out
}

fn setter_json(data: &GameData, s: &Setter) -> Json {
    let mut o = Map::new();
    o.insert("kind".into(), json!(s.kind));
    o.insert("form".into(), json!(form_name(&data.load_order, s.form.0)));
    if let Some(e) = data.edid_of.get(&s.form) {
        o.insert("edid".into(), json!(e));
    }
    if let Some(st) = s.stage {
        o.insert("stage".into(), json!(st));
    }
    Json::Object(o)
}

/// How many setters the detail lists per stage.
const SETTERS_SHOWN: usize = 3;

/// `fnvtool script cover` / `coverage-report`: `logs` merged and compared with `data`'s
/// quests, scripts and lines. The summary holds the totals and the untouched quests,
/// ranked (main quest, factions, add-ons, the rest) and cut to `untouched_max`; the
/// detail holds every quest, script and actor.
pub fn report(data: &GameData, logs: &[Json], untouched_max: usize) -> Result<Json, String> {
    let mut r = Reached::default();
    for (i, l) in logs.iter().enumerate() {
        r.add(l).map_err(|e| format!("log {i}: {e}"))?;
    }
    let lo = &data.load_order;
    let edid = |f: FormId| data.edid_of.get(&f).cloned().unwrap_or_else(|| form_name(lo, f.0));

    // Quests: stages reached; completed when a stage that completes the quest was reached
    // or the run marked the quest completed (CompleteQuest in a script, which many quests
    // end by instead of a flagged stage).
    let mut quests = Vec::new();
    let mut untouched: Vec<(u8, String, usize)> = Vec::new();
    let (mut stages_total, mut stages_reached, mut quests_touched, mut completed) = (0usize, 0usize, 0usize, 0usize);
    let setters = stage_setters(data);
    let (mut settable, mut by_variable, mut missing_settable, mut missing_by_variable, mut reached_unsettable) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for (q, def) in &data.quests {
        if def.stages.is_empty() {
            continue;
        }
        let name = form_name(lo, q.0);
        let reached: Vec<i32> = def.stages.keys().copied().filter(|s| r.stages.contains(&(name.clone(), *s as i64))).collect();
        let completing: Vec<i32> =
            def.stages.iter().filter(|(_, es)| es.iter().any(|e| e.flags & 1 != 0)).map(|(s, _)| *s).collect();
        let complete = completing.iter().any(|s| reached.contains(s)) || r.completed.contains(&name);
        let e = def.edid.clone().unwrap_or_else(|| name.clone());
        stages_total += def.stages.len();
        stages_reached += reached.len();
        // A quest marked completed with no stage reached (one whose only stage is its
        // fail stage, ended by CompleteQuest) was touched too.
        let touched = !reached.is_empty() || complete;
        quests_touched += usize::from(touched);
        completed += usize::from(complete);
        let rk = rank(data, *q, &e);
        if !touched {
            untouched.push((rk, e.clone(), def.stages.len()));
        }
        // Missing stages by whether anything in the data can set them.
        let var_setters = setters.by_variable.get(q);
        let (mut m_set, mut m_var, mut m_none) = (Vec::new(), Vec::new(), Vec::new());
        let mut shown = Map::new();
        for s in def.stages.keys() {
            let hit = reached.contains(s);
            match (setters.literal.get(&(*q, *s)), var_setters) {
                (Some(l), _) => {
                    settable += 1;
                    if !hit {
                        m_set.push(*s);
                        let l: Vec<Json> = l.iter().take(SETTERS_SHOWN).map(|x| setter_json(data, x)).collect();
                        shown.insert(s.to_string(), json!(l));
                    }
                }
                (None, Some(_)) => {
                    by_variable += 1;
                    if !hit {
                        m_var.push(*s);
                    }
                }
                (None, None) if hit => reached_unsettable += 1,
                (None, None) => m_none.push(*s),
            }
        }
        missing_settable += m_set.len();
        missing_by_variable += m_var.len();
        let mut row = json!({
            "quest": e, "form": name, "rank": RANKS[rk as usize],
            "stages": def.stages.len(), "reached": reached,
            "completing_stages": completing, "completed": complete,
            "missing_settable": m_set, "missing_unsettable": m_none,
            "setters": shown,
        });
        if let Some(v) = var_setters {
            row["missing_by_variable"] = json!(m_var);
            let v: Vec<Json> = v.iter().take(SETTERS_SHOWN).map(|x| setter_json(data, x)).collect();
            row["by_variable_setters"] = json!(v);
        }
        quests.push(row);
    }
    untouched.sort();
    let mut untouched_by_rank = Map::new();
    for (i, n) in RANKS.iter().enumerate() {
        untouched_by_rank.insert(n.to_string(), json!(untouched.iter().filter(|u| u.0 as usize == i).count()));
    }

    // Scripts (SCPT units with blocks): blocks run out of all.
    let mut scripts = Vec::new();
    let (mut blocks_total, mut blocks_run, mut scripts_touched) = (0usize, 0usize, 0usize);
    let mut results_total: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for u in &data.units {
        let key = unit_name(lo, &u.key());
        if let Some(kind) = result_kind(&u.unit.owner) {
            // A result script field with no statement (most INFOs' and packages') is
            // nothing to run.
            if u.program.top.is_empty() {
                continue;
            }
            let e = results_total.entry(kind).or_default();
            e.0 += 1;
            e.1 += usize::from(r.results.contains(&key));
            continue;
        }
        if !matches!(u.unit.owner, UnitOwner::Script { .. }) || u.program.blocks.is_empty() {
            continue;
        }
        let n = u.program.blocks.len();
        let run = (0..n).filter(|i| r.blocks.contains(&(key.clone(), *i as u64))).count();
        blocks_total += n;
        blocks_run += run;
        scripts_touched += usize::from(run > 0);
        scripts.push(json!({"script": edid(FormId(u.unit.owner.form())), "unit": key, "blocks": n, "run": run}));
    }

    // Lines per actor.
    let mut actors: BTreeMap<FormId, (usize, usize)> = BTreeMap::new();
    let (mut lines_total, mut lines_said) = (0usize, 0usize);
    for (id, info) in &data.infos {
        let said = r.infos.contains(&form_name(lo, id.0));
        lines_total += 1;
        lines_said += usize::from(said);
        if let Some(a) = line_actor(data, info) {
            let e = actors.entry(a).or_default();
            e.0 += 1;
            e.1 += usize::from(said);
        }
    }
    let actors_json: Vec<Json> = actors
        .iter()
        .map(|(a, (n, s))| json!({"actor": edid(*a), "form": form_name(lo, a.0), "lines": n, "said": s}))
        .collect();
    let actors_touched = actors.values().filter(|(_, s)| *s > 0).count();

    let results: Map<String, Json> =
        results_total.iter().map(|(k, (n, run))| (k.to_string(), json!({"total": n, "run": run}))).collect();
    Ok(json!({
        "format": REPORT_FORMAT,
        "load_order": lo,
        "logs": logs.len(),
        "summary": {
            "quests": {"total": quests.len(), "touched": quests_touched, "completed": completed},
            "stages": {
                "total": stages_total, "reached": stages_reached,
                "settable": settable, "by_variable": by_variable,
                "missing_settable": missing_settable, "missing_by_variable": missing_by_variable,
                "reached_unsettable": reached_unsettable,
                "set_calls_quest_by_variable": setters.quest_by_variable,
            },
            "scripts": {"total": scripts.len(), "touched": scripts_touched},
            "blocks": {"total": blocks_total, "run": blocks_run},
            "lines": {"total": lines_total, "said": lines_said},
            "actors": {"with_lines": actors.len(), "touched": actors_touched},
            "results": results,
            "untouched_quests": untouched_by_rank,
        },
        "untouched": untouched
            .iter()
            .take(untouched_max)
            .map(|(rk, e, n)| json!({"quest": e, "rank": RANKS[*rk as usize], "stages": n}))
            .collect::<Vec<_>>(),
        "detail": {"quests": quests, "scripts": scripts, "actors": actors_json},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_carry_the_plugin() {
        let lo = vec!["FalloutNV.esm".to_string(), "HonestHearts.esm".to_string()];
        assert_eq!(form_name(&lo, 0x0010E908), "FalloutNV.esm:10E908");
        assert_eq!(form_name(&lo, 0x01000ADE), "HonestHearts.esm:000ADE");
        assert_eq!(unit_name(&lo, "HonestHearts.esm|QUST 0x0010E908 stage 100 entry 1"), "QUST FalloutNV.esm:10E908 stage 100 entry 1");
        assert_eq!(unit_name(&lo, "FalloutNV.esm|INFO 0x01000ADE end"), "INFO HonestHearts.esm:000ADE end");
        // Not a FormID: kept as it is.
        assert_eq!(unit_name(&lo, "x|Other 0x12 unit 0"), "Other 0x12 unit 0");
    }
}
