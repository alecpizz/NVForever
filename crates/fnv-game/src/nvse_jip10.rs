//! JIP LN NVSE, batch 10 (#1006): quest objectives and their targets, the quest lists,
//! SetStageAlt, perks and factions, script variables added at run time, scripts and their
//! event blocks switched off, FakeScriptEvent, linked and enable children, the references'
//! extra data, script map markers, and the text-file functions [documented: each page in
//! the wiki export; docs/spec-notes/jip-scripts-10.md].

use crate::game::{Game, InstKey, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The first variable index AddScriptVariable gives: past any script's own (the indices
/// are a u16 and scripts declare far fewer) [hypothesis: the original's numbering is its
/// own; scripts reach the variables only by name].
pub const ADDED_VAR_BASE: u16 = 0x8000;

/// The most a file read gives, in characters [documented: ReadStringFromFile, "Maximum
/// length of a file is 16384 characters"].
const READ_LIMIT: usize = 16384;

/// What this batch keeps. `added_vars` and `extra` are saved with the game
/// [documented: AddScriptVariable, "saved with the game"; Extra Data Functions (JIP), "saved
/// in the game and persists with the reference"]; the rest lasts the session
/// [documented: AddRefMapMarker's markers are removed on load; hypothesis for the others].
#[derive(Debug, Default)]
pub struct Jip10State {
    /// SetObjectiveText's texts, by (quest, objective).
    pub objective_texts: BTreeMap<(FormId, i32), String>,
    /// The objectives' target lists Add/Remove/SetObjectiveNthTarget changed.
    pub objective_targets: BTreeMap<(FormId, i32), Vec<FormId>>,
    /// AddScriptVariable's names, by script, in the order added (index
    /// `ADDED_VAR_BASE + place`); a removed one leaves an empty name so the others keep
    /// their indices.
    pub added_vars: BTreeMap<FormId, Vec<String>>,
    /// SetScriptDisabled's scripts.
    pub disabled_scripts: BTreeSet<FormId>,
    /// SetScriptEventDisabled's blocks, by reference or base form: the block codes.
    pub disabled_events: BTreeMap<FormId, BTreeSet<u16>>,
    /// FakeScriptEvent is running a block: the disabled blocks run [documented: "not
    /// affected by SetScriptEventDisabled"].
    pub faking: bool,
    /// SetRefExtraData's values: reference -> owning mod index -> slot 0-19 -> value.
    pub extra: BTreeMap<FormId, BTreeMap<u8, BTreeMap<u8, XVal>>>,
    /// AddRefMapMarker's references, for the map.
    pub map_markers: BTreeSet<FormId>,
}

/// The part of [`Jip10State`] a save keeps (`GameSnapshot::jip10`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Jip10Saved {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub added_vars: BTreeMap<FormId, Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<FormId, BTreeMap<u8, BTreeMap<u8, XVal>>>,
}

impl Jip10Saved {
    pub fn is_empty(&self) -> bool {
        self.added_vars.is_empty() && self.extra.is_empty()
    }
}

fn num(args: &[XVal], k: usize) -> Option<f64> {
    match args.get(k) {
        Some(XVal::Num(x)) if x.is_finite() => Some(*x),
        _ => None,
    }
}

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k) {
        Some(XVal::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

/// A list getter's array; none at all when there is nothing to give [observed: the jip10
/// probes, 2026-11-08: GetAllPerks, GetLinkedChildren, GetEnableChildren and
/// GetQuestObjectives with nothing to give returned no array, `ar_Size` -1].
fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    if items.is_empty() {
        return XVal::Array(0);
    }
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

fn refs(g: &mut Game, items: impl IntoIterator<Item = FormId>) -> XVal {
    let v = items.into_iter().map(|f| XVal::Ref(f.0)).collect();
    list(g, v)
}

impl Game<'_> {
    /// An objective's display text: SetObjectiveText's, else the record's (NNAM).
    pub fn objective_text(&self, quest: FormId, objective: i32) -> Option<String> {
        if let Some(t) = self.other.jip10.objective_texts.get(&(quest, objective)) {
            return Some(t.clone());
        }
        self.data.quests.get(&quest)?.objectives.get(&objective)?.text.clone()
    }

    /// An objective's target references: the scripts' list, else the record's (QSTA)
    /// in record order.
    pub fn objective_targets(&self, quest: FormId, objective: i32) -> Vec<FormId> {
        if let Some(t) = self.other.jip10.objective_targets.get(&(quest, objective)) {
            return t.clone();
        }
        self.data.quests.get(&quest).and_then(|q| q.objectives.get(&objective)).map(|o| o.targets.iter().filter_map(|t| t.target).collect()).unwrap_or_default()
    }

    /// Whether the scripts changed an objective's target list.
    pub fn objective_targets_changed(&self, quest: FormId, objective: i32) -> bool {
        self.other.jip10.objective_targets.contains_key(&(quest, objective))
    }

    /// The index AddScriptVariable gave `name` in `script`, if it did.
    pub fn added_var_index(&self, script: FormId, name: &str) -> Option<u16> {
        let names = self.other.jip10.added_vars.get(&script)?;
        names.iter().position(|n| !n.is_empty() && n.eq_ignore_ascii_case(name)).map(|i| ADDED_VAR_BASE + i as u16)
    }

    /// Whether a script's blocks are not to run: SetScriptDisabled's script, or the
    /// event's block switched off for the instance's reference or its base
    /// (SetScriptEventDisabled), unless FakeScriptEvent runs it.
    pub(crate) fn script_blocked(&self, key: InstKey, script: FormId, event: u16) -> bool {
        let j = &self.other.jip10;
        if j.disabled_scripts.contains(&script) {
            return true;
        }
        if j.faking || j.disabled_events.is_empty() {
            return false;
        }
        let forms: Vec<FormId> = match key {
            InstKey::Ref { reference } => vec![reference, crate::functions::base_of(self, reference).unwrap_or(reference)],
            InstKey::Item { item, .. } => vec![item],
            _ => Vec::new(),
        };
        forms.iter().any(|f| j.disabled_events.get(f).is_some_and(|e| e.contains(&event)))
    }

    /// The save's part of this batch.
    pub(crate) fn jip10_saved(&self) -> Jip10Saved {
        let j = &self.other.jip10;
        Jip10Saved { added_vars: j.added_vars.clone(), extra: j.extra.clone() }
    }

    /// A save's part of this batch back.
    pub(crate) fn restore_jip10(&mut self, s: &Jip10Saved) {
        let j = &mut self.other.jip10;
        j.added_vars = s.added_vars.clone();
        j.extra = s.extra.clone();
        j.map_markers.clear();
    }
}

/// A block code by its name (`OnHit`), without regard to case, from the catalogue.
fn event_code(g: &Game, name: &str) -> Option<u16> {
    g.data.table.blocks.iter().find(|(_, n)| n.eq_ignore_ascii_case(name.trim())).map(|(c, _)| *c)
}

/// GetObjectiveTargets gives an objective's targets: shown and not done, or changed by a
/// script [observed: one done objective gave none; hypothesis: the rule].
fn targets_kept(g: &Game, q: FormId, i: i32) -> bool {
    g.objective_targets_changed(q, i) || g.quests.get(&q).and_then(|s| s.objectives.get(&i)).is_some_and(|o| o.displayed && !o.completed)
}

/// A quest is shown in the Pip-Boy: it has a name and an objective shown or done
/// [hypothesis: as the Pip-Boy's quest list, `nvf_play::pipboy::data`].
fn listed(g: &Game, q: FormId) -> bool {
    let named = g.data.quests.get(&q).and_then(|d| d.name.as_deref()).is_some_and(|n| !n.trim().is_empty());
    named && g.quests.get(&q).is_some_and(|s| s.objectives.values().any(|o| o.displayed || o.completed))
}

/// The mod index that owns a script's extra data: the calling script's file (its FormID's
/// first byte) [documented: "the parent mod of the calling script"]; 0xFF for a script of
/// no file (the console, a runtime script) [hypothesis].
fn calling_mod(g: &Game) -> u8 {
    g.other.script.map_or(NO_MOD, |s| (s.0 >> 24) as u8)
}

/// [`calling_mod`] of a script of no file.
const NO_MOD: u8 = 0xFF;

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let quest_at = |g: &Game, k: usize| form(args, k).filter(|q| g.data.quests.contains_key(q));
    let displayed = |g: &Game, q: FormId, i: i32| g.quests.get(&q).and_then(|s| s.objectives.get(&i)).is_some_and(|o| o.displayed);
    let v = match name {
        // ---- objectives: `quest index [target]`; an objective already displayed for
        // the changes [documented: "these Objectives must be already Displayed"].
        "addobjectivetarget" | "removeobjectivetarget" => {
            let (Some(q), Some(i), Some(t)) = (quest_at(g, 0), num(args, 1), form(args, 2)) else { return Some(Ok(zero)) };
            let i = i as i32;
            if displayed(g, q, i) {
                let mut list = g.objective_targets(q, i);
                if name == "addobjectivetarget" {
                    if !list.contains(&t) {
                        list.push(t);
                    }
                } else {
                    list.retain(|&x| x != t);
                }
                g.other.jip10.objective_targets.insert((q, i), list);
            }
            zero
        }
        // `quest index n ref`: the n-th target becomes the reference; past the end
        // nothing (see docs/spec-notes/jip-scripts-10.md §1).
        "setobjectiventhtarget" => {
            let (Some(q), Some(i), Some(n), Some(t)) = (quest_at(g, 0), num(args, 1), num(args, 2), form(args, 3)) else { return Some(Ok(zero)) };
            let i = i as i32;
            let mut list = g.objective_targets(q, i);
            if let Some(slot) = usize::try_from(n as i64).ok().and_then(|n| list.get_mut(n)) {
                *slot = t;
                g.other.jip10.objective_targets.insert((q, i), list);
            }
            zero
        }
        // An objective shown and not done, or one whose list a script changed [observed:
        // VMS01's objective 10, done, gave no array though its record names a target;
        // hypothesis: the targets of a done or unshown objective are not kept].
        "getobjectivetargets" => {
            let (Some(q), Some(i)) = (quest_at(g, 0), num(args, 1)) else { return Some(Ok(XVal::Array(0))) };
            let t = if targets_kept(g, q, i as i32) { g.objective_targets(q, i as i32) } else { Vec::new() };
            refs(g, t)
        }
        "getobjectivehastarget" => {
            let (Some(q), Some(i), Some(t)) = (quest_at(g, 0), num(args, 1), form(args, 2)) else { return Some(Ok(zero)) };
            yes(targets_kept(g, q, i as i32) && g.objective_targets(q, i as i32).contains(&t))
        }
        // `quest index "text"` (formatted by the VM) [documented]: the Pip-Boy and the
        // corner messages show it.
        "setobjectivetext" => {
            let (Some(q), Some(i), Some(t)) = (quest_at(g, 0), num(args, 1), text(args, 2)) else { return Some(Ok(zero)) };
            if g.data.quests.get(&q).is_some_and(|d| d.objectives.contains_key(&(i as i32))) {
                g.other.jip10.objective_texts.insert((q, i as i32), t);
            }
            zero
        }
        // The active quest's objectives shown and not done, by index (see
        // docs/spec-notes/jip-scripts-10.md §1).
        "getactiveobjectives" => {
            let open: Vec<XVal> = g.ui.active_quest.and_then(|q| g.quests.get(&q)).map(|s| s.objectives.iter().filter(|(_, o)| o.displayed && !o.completed).map(|(&i, _)| XVal::Num(f64::from(i))).collect()).unwrap_or_default();
            list(g, open)
        }
        // `quest [completed]`: the objectives shown and not done, or (completed) the done
        // ones shown, by index [documented: the examples; hypothesis: the indices].
        "getquestobjectives" => {
            let Some(q) = quest_at(g, 0) else { return Some(Ok(XVal::Array(0))) };
            let done = num(args, 1).is_some_and(|x| x != 0.0);
            let out: Vec<XVal> = g.quests.get(&q).map(|s| s.objectives.iter().filter(|(_, o)| (o.displayed || o.completed) && o.completed == done).map(|(&i, _)| XVal::Num(f64::from(i))).collect()).unwrap_or_default();
            list(g, out)
        }
        // `[completed]`: the quests in the Pip-Boy's list that are running and not done,
        // or (completed) those done [documented; hypothesis: the Pip-Boy's list].
        "getquests" => {
            let done = num(args, 0).is_some_and(|x| x != 0.0);
            let qs: Vec<FormId> = g.quests.iter().filter(|(_, s)| if done { s.completed } else { s.running && !s.completed }).map(|(&q, _)| q).filter(|&q| listed(g, q)).collect();
            refs(g, qs)
        }
        "clearactivequest" => {
            if g.ui.active_quest.take().is_some() {
                g.ui.active_quest_count += 1;
            }
            zero
        }
        // `quest stage`: as SetStage, then GetStage is the stage even below the highest
        // done, without clearing GetStageDone for higher completed stages [documented:
        // "set the specified quest's highest completed stage ... even if it is lower";
        // observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, probe
        // jip_29_setstagealt_clears_higher: SetStage 20 then SetStageAlt 10 -> GetStage=10,
        // GetStageDone 20 == 1].
        "setstagealt" => {
            let (Some(q), Some(s)) = (quest_at(g, 0), num(args, 1)) else { return Some(Ok(zero)) };
            let s = s as i32;
            g.set_stage(q, s);
            if let Some(st) = g.quests.get_mut(&q) {
                st.stages_done.insert(s);
                st.current_stage = Some(s);
            }
            zero
        }
        // ---- perks: `[forTeammates] [includeHidden]` [documented]. NVForever's other
        // actors hold perks as with uNPCPerks on [hypothesis: NVForever gives them].
        "getallperks" => {
            let Some(r) = subject else { return Some(Ok(XVal::Array(0))) };
            let mates = num(args, 0).is_some_and(|x| x != 0.0);
            let hidden = num(args, 1).is_some_and(|x| x != 0.0);
            let mut held: Vec<FormId> = if r == FormId::PLAYER_REF {
                if mates { g.player.teammate_perks.iter().copied().collect() } else { g.player.perks.keys().copied().collect() }
            } else {
                let own: Vec<FormId> = g.extra.get(&r).map(|x| x.perks.keys().copied().collect()).unwrap_or_default();
                let mut v = own;
                if mates && g.is_teammate(r) {
                    v.extend(g.player.teammate_perks.iter().copied().filter(|p| !g.extra.get(&r).is_some_and(|x| x.perks.contains_key(p))));
                }
                v
            };
            held.retain(|&p| hidden || !g.perk_def(p).is_some_and(|d| d.hidden));
            refs(g, held)
        }
        // `[forTeammates]` [documented].
        "removeallperks" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            if r == FormId::PLAYER_REF {
                if num(args, 0).is_some_and(|x| x != 0.0) {
                    g.player.teammate_perks.clear();
                } else {
                    let all: Vec<FormId> = g.player.perks.keys().copied().collect();
                    for p in all {
                        if g.player.perks.remove(&p).is_some() {
                            g.perk_event(p, false, 0, 0);
                        }
                    }
                }
            } else if g.extra.get_mut(&r).is_some_and(|x| !std::mem::take(&mut x.perks).is_empty()) {
                g.sync_perk_abilities_of(r);
            }
            zero
        }
        // ---- factions: `ref.GetFactions` the factions now; `GetFactions base` the base's
        // own (or its template's when it takes its factions from one) [documented].
        "getfactions" => {
            let f: Vec<FormId> = match form(args, 0) {
                Some(b) => {
                    let src = g.data.base_template_source(b, |d| d.template_factions);
                    g.data.actors.get(&src).map(|d| d.factions.iter().map(|&(f, _)| f).collect()).unwrap_or_default()
                }
                // The base record's order, then factions added since [observed: Doc
                // Mitchell's first is Goodsprings Militia, as in his record; FormID order
                // put another first].
                None => match subject {
                    Some(r) => {
                        let now = crate::functions::actor::factions_of(g, r);
                        let base = crate::functions::base_of(g, r).map(|b| g.data.base_template_source(b, |d| d.template_factions));
                        let mut order: Vec<FormId> = base.and_then(|b| g.data.actors.get(&b)).map(|d| d.factions.iter().map(|&(f, _)| f).filter(|f| now.contains(f)).collect()).unwrap_or_default();
                        let rest: Vec<FormId> = now.into_iter().filter(|f| !order.contains(f)).collect();
                        order.extend(rest);
                        order
                    }
                    None => Vec::new(),
                },
            };
            refs(g, f)
        }
        // ---- script variables added at run time: `ref.F "name"` or `F "name" quest`
        // [documented]; GetVariable and the rest reach them (`crate::nvse`).
        "addscriptvariable" | "removescriptvariable" | "hasvariableadded" => {
            let Some(var) = text(args, 0).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) else { return Some(Ok(zero)) };
            let parent = form(args, 1).or(subject);
            let Some((key, script)) = parent.and_then(|p| g.instance_of(p)) else { return Some(Ok(zero)) };
            let own = g.data.script(script).and_then(|u| u.var_index(&var)).is_some();
            let added = g.added_var_index(script, &var);
            match name {
                "hasvariableadded" => yes(added.is_some()),
                // 1 when added, and 1 again for a name added before [observed: the jip10
                // probe]; 0 for a name the script declares [hypothesis].
                "addscriptvariable" => {
                    if own {
                        return Some(Ok(zero));
                    }
                    if added.is_some() {
                        return Some(Ok(XVal::Num(1.0)));
                    }
                    g.other.jip10.added_vars.entry(script).or_default().push(var);
                    XVal::Num(1.0)
                }
                _ => {
                    let Some(i) = added else { return Some(Ok(zero)) };
                    if let Some(n) = g.other.jip10.added_vars.get_mut(&script).and_then(|v| v.get_mut(usize::from(i - ADDED_VAR_BASE))) {
                        n.clear();
                    }
                    if let Some(inst) = g.instances.get_mut(&key) {
                        inst.vars.remove(&i);
                    }
                    XVal::Num(1.0)
                }
            }
        }
        // `ref.F` or `F quest` [documented].
        "removealladdedvariables" => {
            let parent = form(args, 0).or(subject);
            if let Some((key, script)) = parent.and_then(|p| g.instance_of(p)) {
                if let Some(names) = g.other.jip10.added_vars.remove(&script) {
                    if let Some(inst) = g.instances.get_mut(&key) {
                        for i in 0..names.len() {
                            inst.vars.remove(&(ADDED_VAR_BASE + i as u16));
                        }
                    }
                }
            }
            zero
        }
        // `scriptVars linkedRefs auxVars refMaps` [documented]. NVForever does not keep
        // which mod added a variable or a link: every one goes (see docs/spec-notes/jip-scripts-10.md §3); the aux
        // variables and RefMaps by the calling mod (1) or the public ones (2) [documented].
        "clearjipsaveddata" => {
            if num(args, 0).is_some_and(|x| x != 0.0) {
                let all = std::mem::take(&mut g.other.jip10.added_vars);
                for (script, names) in all {
                    for inst in g.instances.values_mut().filter(|i| i.script == Some(script)) {
                        for i in 0..names.len() {
                            inst.vars.remove(&(ADDED_VAR_BASE + i as u16));
                        }
                    }
                }
            }
            if num(args, 1).is_some_and(|x| x != 0.0) {
                g.jip2.links.clear();
            }
            let plugin = g.other.plugin.clone();
            let private = |k: &str| k.split_once('\u{1}').is_some_and(|(_, p)| p == plugin);
            let public = |k: &str| !k.contains('\u{1}');
            let which = |k: usize| num(args, k).map_or(0, |x| x as i64);
            let (aux, maps) = (which(2), which(3));
            let keep = |mode: i64, k: &str| !((mode == 1 && private(k)) || (mode == 2 && public(k)));
            for vars in g.ext.jip.aux.values_mut() {
                vars.retain(|k, _| keep(aux, k));
            }
            g.ext.jip.aux.retain(|_, v| !v.is_empty());
            g.ext.jip.refmaps.retain(|k, _| keep(maps, k));
            zero
        }
        // ---- scripts and their event blocks switched off: `scriptOrList disable`,
        // `script`; `formOrList "events" disable`, `form "event"` [documented].
        "setscriptdisabled" => {
            let (Some(f), Some(on)) = (form(args, 0), num(args, 1)) else { return Some(Ok(zero)) };
            let mut scripts = crate::functions::refs::list_members(g, f);
            if scripts.is_empty() {
                scripts.push(f);
            }
            for s in scripts.into_iter().filter(|s| g.data.scripts.contains_key(s)) {
                if on != 0.0 {
                    g.other.jip10.disabled_scripts.insert(s);
                } else {
                    g.other.jip10.disabled_scripts.remove(&s);
                }
            }
            zero
        }
        "getscriptdisabled" => yes(form(args, 0).is_some_and(|s| g.other.jip10.disabled_scripts.contains(&s))),
        "setscripteventdisabled" => {
            let (Some(f), Some(names), Some(on)) = (form(args, 0), text(args, 1), num(args, 2)) else { return Some(Ok(zero)) };
            let codes: Vec<u16> = names.split_whitespace().filter_map(|n| event_code(g, n)).collect();
            let mut forms = crate::functions::refs::list_members(g, f);
            if forms.is_empty() {
                forms.push(f);
            }
            for x in forms {
                let set = g.other.jip10.disabled_events.entry(x).or_default();
                for &c in &codes {
                    if on != 0.0 {
                        set.insert(c);
                    } else {
                        set.remove(&c);
                    }
                }
                if set.is_empty() {
                    g.other.jip10.disabled_events.remove(&x);
                }
            }
            zero
        }
        "getscripteventdisabled" => {
            let (Some(f), Some(n)) = (form(args, 0), text(args, 1)) else { return Some(Ok(zero)) };
            yes(event_code(g, &n).is_some_and(|c| g.other.jip10.disabled_events.get(&f).is_some_and(|e| e.contains(&c))))
        }
        // `ref.FakeScriptEvent "event" [filter]`: the reference's block of that event
        // runs, as if the event came, with the filter as its object; 1 when a block ran.
        // Not OnActivate, not a quest's [documented].
        "fakescriptevent" => {
            let (Some(r), Some(n)) = (subject, text(args, 0)) else { return Some(Ok(zero)) };
            let Some(code) = event_code(g, &n).filter(|&c| c != crate::game::event::ON_ACTIVATE) else { return Some(Ok(zero)) };
            let filter = form(args, 1).map(|f| fnv_script::vm::Value::Ref(f.0));
            let (key, script) = match g.temp_item(r) {
                Some((holder, item)) => (InstKey::Item { holder, item }, g.base_script(item)),
                None => (InstKey::Ref { reference: r }, g.ref_script(r)),
            };
            let Some(script) = script else { return Some(Ok(zero)) };
            g.other.jip10.faking = true;
            let ran = g.run_event(key, script, code, filter, RunCtx { self_ref: Some(r), ..Default::default() });
            g.other.jip10.faking = false;
            yes(ran)
        }
        // ---- the references linked to this one, and those it is the enable parent of
        // [documented], in FormID order.
        "getlinkedchildren" => {
            let Some(r) = subject else { return Some(Ok(XVal::Array(0))) };
            let ids: Vec<FormId> = g.world.placed.refs.keys().copied().chain(g.jip2.links.keys().copied()).collect::<BTreeSet<_>>().into_iter().filter(|&c| c != r && g.linked_ref(c) == Some(r)).collect();
            refs(g, ids)
        }
        "getenablechildren" => {
            let Some(r) = subject else { return Some(Ok(XVal::Array(0))) };
            let ids: Vec<FormId> = g.world.placed.refs.iter().filter(|(_, p)| p.enable_parent.is_some_and(|e| e.reference == r)).map(|(&id, _)| id).collect();
            refs(g, ids)
        }
        // ---- extra data: slots 0-17 a number or a form, 18-19 a string [documented],
        // owned by the calling script's mod, read by any mod's index [documented].
        "setrefextradata" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let owner = calling_mod(g);
            // A script of no plugin keeps nothing [observed: the jip10 probe, run from a
            // script of no file, read nothing back on the player; hypothesis: the reason].
            if owner == NO_MOD {
                return Some(Ok(zero));
            }
            let Some(slot) = num(args, 0).map(|s| s as i64) else {
                // Both left out: all of this mod's data on the reference goes [documented].
                if let Some(m) = g.other.jip10.extra.get_mut(&r) {
                    m.remove(&owner);
                }
                g.other.jip10.extra.retain(|_, m| !m.is_empty());
                return Some(Ok(zero));
            };
            let Ok(slot) = u8::try_from(slot) else { return Some(Ok(zero)) };
            let v = match (slot, args.get(1)) {
                (0..=17, Some(v @ (XVal::Num(_) | XVal::Ref(_)))) => Some(v.clone()),
                (18 | 19, Some(v @ XVal::Str(_))) => Some(v.clone()),
                // No value clears the slot (see docs/spec-notes/jip-scripts-10.md §4).
                (0..=19, None) => None,
                _ => return Some(Ok(zero)),
            };
            let slots = g.other.jip10.extra.entry(r).or_default().entry(owner).or_default();
            match v {
                Some(v) => {
                    slots.insert(slot, v);
                }
                None => {
                    slots.remove(&slot);
                }
            }
            g.other.jip10.extra.retain(|_, m| {
                m.retain(|_, s| !s.is_empty());
                !m.is_empty()
            });
            zero
        }
        // `slot [modIndex]`: 0, a form or "" when unset by the slot's type [hypothesis].
        "getrefextradata" => {
            let (Some(r), Some(slot)) = (subject, num(args, 0).map(|s| s as i64)) else { return Some(Ok(zero)) };
            let owner = num(args, 1).map_or_else(|| calling_mod(g), |m| m as u8);
            let v = u8::try_from(slot).ok().and_then(|s| g.other.jip10.extra.get(&r)?.get(&owner)?.get(&s).cloned());
            v.unwrap_or(if (18..=19).contains(&slot) { XVal::Str(String::new()) } else { zero })
        }
        "refhasextradata" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let owner = num(args, 0).map_or_else(|| calling_mod(g), |m| m as u8);
            yes(g.other.jip10.extra.get(&r).is_some_and(|m| m.get(&owner).is_some_and(|s| !s.is_empty())))
        }
        // ---- map markers of references: not an inventory item [documented]; removed
        // on load [documented].
        "addrefmapmarker" | "removerefmapmarker" | "refhasmapmarker" => {
            let Some(r) = subject.filter(|&r| g.temp_item(r).is_none()) else { return Some(Ok(zero)) };
            match name {
                "addrefmapmarker" => {
                    g.other.jip10.map_markers.insert(r);
                    zero
                }
                "removerefmapmarker" => {
                    g.other.jip10.map_markers.remove(&r);
                    zero
                }
                _ => yes(g.other.jip10.map_markers.contains(&r)),
            }
        }
        // ---- text files under the game's folder (NVForever's layer: `crate::mod_files`).
        // `path [startLine] [count]`: from line 1, all lines; at most 16384 characters
        // [documented]; a missing file "" [hypothesis].
        "readstringfromfile" => {
            let Some(path) = text(args, 0) else { return Some(Ok(XVal::Str(String::new()))) };
            let Some(t) = g.mod_files.read_root_text(&path) else { return Some(Ok(XVal::Str(String::new()))) };
            let t: String = t.chars().take(READ_LIMIT).collect();
            let start = num(args, 1).map_or(1, |s| (s as i64).max(1)) as usize;
            // The lines joined by line breaks, no break after the last [observed: lines 2-3
            // of `one two three` read as `two\nthree`].
            let lines: Vec<&str> = t.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).skip(start - 1).collect();
            let lines = match num(args, 2).filter(|c| *c > 0.0) {
                Some(c) => &lines[..(c as usize).min(lines.len())],
                None => &lines[..],
            };
            XVal::Str(lines.join("\n"))
        }
        // `path append "text"` (formatted by the VM): 1 when written [documented].
        "writestringtofile" => {
            let (Some(path), Some(t)) = (text(args, 0), text(args, 2)) else { return Some(Ok(zero)) };
            let append = num(args, 1).is_some_and(|x| x != 0.0);
            // An append starts on a line of its own [observed: `one%rtwo%rthree` then
            // `%rfour` appended left `one\ntwo\nthree\n\nfour`, line breaks LF only].
            let body = match g.mod_files.read_root_text(&path).filter(|o| append && !o.is_empty()) {
                Some(old) => old + "\n" + &t,
                None => t,
            };
            yes(g.mod_files.write_root_text(&path, body))
        }
        // `indent "text"` (formatted by the VM): a line in `Mod Logs\<mod>.log` under the
        // game's folder, a tab per indent level [documented: the folder and name];
        // a script of no mod writes nothing (see docs/spec-notes/jip-scripts-10.md §5).
        "modlogprint" => {
            let plugin = g.other.plugin.clone();
            let Some(t) = text(args, 1).filter(|_| !plugin.is_empty()) else { return Some(Ok(zero)) };
            let indent = num(args, 0).map_or(0, |i| (i as i64).clamp(0, 32) as usize);
            let path = format!("Mod Logs\\{plugin}.log");
            let old = g.mod_files.read_root_text(&path).unwrap_or_default();
            let line = format!("{}{t}\n", "\t".repeat(indent));
            g.mod_files.write_root_text(&path, old + &line);
            zero
        }
        // `[ref.]RunBatchScript path`: the file's lines run at once on the reference, or
        // the player; 1 when read and run without an error [documented].
        "runbatchscript" => {
            let Some(src) = text(args, 0).and_then(|p| g.mod_files.read_root_text(&p)) else { return Some(Ok(zero)) };
            let on = subject.unwrap_or(FormId::PLAYER_REF);
            yes(g.run_source(&src, RunCtx { self_ref: Some(on), ..Default::default() }).is_ok())
        }
        _ => return None,
    };
    Some(Ok(v))
}
