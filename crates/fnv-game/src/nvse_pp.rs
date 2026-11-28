//! The weapon-instance plugin's functions (ppNVSE, catalogue origin PP): AX variables,
//! form instances, the extended item mods' slots, CallLoop, the sighting node and a few
//! weapon and inventory helpers [documented: each page in the wiki export;
//! docs/spec-notes/ext-pp.md] and the probe set `pp` run in the game with ppNVSE installed
//! [observed: pp probes, the row named]; what neither settles is marked [hypothesis]. Its file functions
//! (pReadFile, pWriteFile, pWriteFileAlt) are in `crate::mod_files`.
//!
//! Devkit, the plugin's text files that give forms traits, item-mod slots and akimbo sets,
//! is not read by NVForever [decision: a separate loader, ext-pp.md §6], so the traits and
//! akimbo functions find none and no extended slot is defined, so none can be filled.

use crate::game::{Game, RunCtx};
use crate::trace::EventKind;
use fnv_script::vm::{ArrayKind, Value, XKey, XVal};
use fnv_sim::FormId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// The node the iron sights line up on unless SetSightingNode changed it [documented:
/// GetCurrentSightingNode].
pub const SIGHTING_NODE: &str = "##SightingNode";

/// A form instance: the base form it was made from, its key and its editor ID
/// [documented: CreateFormInstance, "ParentsEditorID + "Inst" + InstanceCount"].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub parent: FormId,
    pub key: String,
    pub edid: String,
}

/// A SetOnInstanceDeconstruct / SetOnInstanceReconstruct registration.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceHandler {
    pub reconstruct: bool,
    pub priority: i32,
    pub func: XVal,
    /// The instance key it is for (lower case); `None` for every instance.
    pub filter: Option<String>,
}

/// An OnAttachItemMod / OnDetachItemMod registration: filter 1 the item mod, filter 2 the
/// object [documented: OnAttachItemMod].
#[derive(Debug, Clone, PartialEq)]
pub struct ModHandler {
    pub attach: bool,
    pub priority: i32,
    pub func: XVal,
    pub on_rebuild: bool,
    pub item_mod: Option<FormId>,
    pub object: Option<FormId>,
}

/// A CallLoop registration.
#[derive(Debug, Clone, PartialEq)]
pub struct CallLoop {
    pub func: XVal,
    pub on: FormId,
    pub args: Vec<XVal>,
    /// Seconds between calls.
    pub delay: f64,
    /// The game time of the next call.
    pub next: f64,
}

/// What lasts across a load (`Game::take_session`): the instance and item-mod handlers of
/// function scripts, and the instances the old game had (they are deconstructed when
/// the new one starts).
#[derive(Debug, Clone, Default)]
pub struct PpSession {
    handlers: Vec<InstanceHandler>,
    mod_handlers: Vec<ModHandler>,
    gone: Vec<(FormId, String)>,
}

/// An AX key: the form it is on (`None` global) and the key, lower case.
pub type AxKey = (Option<FormId>, String);
/// An AX key's entries: the key as first given and the entries (`None` a nulled one).
pub type AxList = (String, Vec<Option<XVal>>);

/// This plugin's state.
#[derive(Debug, Default)]
pub struct PpState {
    /// AX variables: (form, key lower case) -> (the key as first given, the entries;
    /// `None` a nulled entry). Session only [documented: "all AX values remain valid for
    /// the session"].
    pub ax: BTreeMap<AxKey, AxList>,
    /// The form instances, saved with the game (`GameSnapshot::form_instances`)
    /// [documented: CreateFormInstance, "tied to the users save game"].
    pub instances: BTreeMap<FormId, Instance>,
    pub handlers: Vec<InstanceHandler>,
    pub mod_handlers: Vec<ModHandler>,
    /// The extended slots' mods by object (a base form, an instance or a reference):
    /// slot lower case -> (slot as given, item mod). Session only [hypothesis].
    pub attached: HashMap<FormId, BTreeMap<String, (String, FormId)>>,
    /// The extended slots Devkit defines (lower case); AttachMod attaches only in these.
    /// Empty: NVForever reads no Devkit (ext-pp.md §6).
    pub slots: std::collections::BTreeSet<String>,
    pub loops: Vec<CallLoop>,
    /// SetSightingNode's node and the weapon the player had then.
    pub sighting: Option<(Option<FormId>, String)>,
    /// EvaluateString's texts and what each compiled to (`None`: it does not compile);
    /// at most [`EVALUATE_MAX`] (`Game::evaluate_string`).
    pub evaluated: HashMap<String, Option<u32>>,
    /// The count in the last instance's editor ID ("Inst" and it); it only rises, so no
    /// two instances share one (restored from the saved editor IDs on a load).
    pub inst_seq: u32,
    /// SetWeaponDegradationMult's multipliers by weapon base.
    pub degradation: HashMap<FormId, f64>,
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize) -> Option<f64> {
    match args.get(k)? {
        XVal::Num(x) => Some(*x),
        _ => None,
    }
}

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        Some(XVal::Num(x)) => crate::nvse_pp::number(*x),
        _ => String::new(),
    }
}

/// A number as a slot name ("1" for 1).
pub(crate) fn number(x: f64) -> String {
    if x.fract() == 0.0 { format!("{}", x as i64) } else { format!("{x}") }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

/// The index and form among the trailing arguments from `k`: the first number is the
/// index, the first form the form (the catalogue's optional index comes before a form).
fn index_and_form(args: &[XVal], k: usize) -> (Option<i64>, Option<FormId>) {
    let mut index = None;
    let mut f = None;
    for a in args.iter().skip(k) {
        match a {
            XVal::Num(x) if index.is_none() => index = Some(*x as i64),
            XVal::Ref(r) if f.is_none() => f = (*r != 0).then_some(FormId(*r)),
            _ => {}
        }
    }
    (index, f)
}

/// The most distinct texts EvaluateString compiles in one game (`Game::evaluate_string`).
pub const EVALUATE_MAX: usize = 1_024;

/// The most entries one AX key holds: AX_Set refuses an index at or past it
/// [decision: PR #1291 review, so a script cannot ask for gigabytes; the pages give no
/// limit].
pub const AX_MAX_ENTRIES: usize = 65_536;

/// AX_GetType's codes [documented: AX GetType: −2 no such aux, −1 null, 0 number, 1
/// form, 2 string, 3 array].
fn type_code(v: Option<&Option<XVal>>) -> f64 {
    match v {
        None => -2.0,
        Some(None) => -1.0,
        Some(Some(XVal::Num(_))) => 0.0,
        Some(Some(XVal::Ref(_))) => 1.0,
        Some(Some(XVal::Str(_))) => 2.0,
        Some(Some(XVal::Array(_))) => 3.0,
    }
}

impl Game<'_> {
    /// An object's base form, through references and inventory references.
    fn pp_base(&self, f: FormId) -> FormId {
        crate::functions::base_of(self, f).unwrap_or(f)
    }

    /// A new instance of `parent`, with the editor ID "Inst" and a count [observed: pp
    /// probe inst, "Inst1" for the first; the page puts the base's editor ID first]. The
    /// count rises with every instance made in the game and is never reused, so no two
    /// share an editor ID (see docs/spec-notes/ext-pp.md §2). `None`
    /// when the clone IDs are used up (they end where the compiled scripts' begin).
    fn pp_new_instance(&mut self, parent: FormId, key: String) -> Option<FormId> {
        if crate::nvse_data::CLONE_BASE + self.clone_seq + 1 >= crate::game::COMPILED_BASE {
            return None;
        }
        self.clone_seq += 1;
        let id = FormId(crate::nvse_data::CLONE_BASE + self.clone_seq);
        self.pp.inst_seq += 1;
        let edid = format!("Inst{}", self.pp.inst_seq);
        self.clones.insert(id, parent);
        self.other.edids.insert(id, edid.clone());
        self.pp.instances.insert(id, Instance { parent, key, edid });
        Some(id)
    }

    /// The saved instances back after a load (`Game::restore`): each is a clone of its base
    /// form again, with its editor ID.
    pub(crate) fn pp_restore_instances(&mut self, saved: &[(FormId, Instance)]) {
        for (id, i) in saved {
            self.clones.insert(*id, i.parent);
            self.other.edids.insert(*id, i.edid.clone());
            self.pp.instances.insert(*id, i.clone());
            self.clone_seq = self.clone_seq.max(id.0.saturating_sub(crate::nvse_data::CLONE_BASE));
            let n = i.edid.strip_prefix("Inst").and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
            self.pp.inst_seq = self.pp.inst_seq.max(n);
        }
    }

    /// What lasts across a load ([`PpSession`]): handlers of function scripts (a lambda
    /// belongs to the old game [hypothesis, as the event handlers]).
    pub(crate) fn pp_take_session(&mut self) -> PpSession {
        let script = |f: &XVal| self.session_function(f);
        PpSession {
            handlers: self.pp.handlers.iter().filter(|h| script(&h.func)).cloned().collect(),
            mod_handlers: self.pp.mod_handlers.iter().filter(|h| script(&h.func)).cloned().collect(),
            gone: self.pp.instances.iter().map(|(id, i)| (*id, i.key.clone())).collect(),
        }
    }

    /// Take over the handlers after a load; the old game's instances are deconstructed,
    /// then each loaded one reconstructed, skipped (dropped) when a handler sets the
    /// function value 1 [documented: SetOnInstanceDeconstruct, "every time the player
    /// reloads a save"; SetOnInstanceReconstruct, SetFunctionValue skips it].
    pub(crate) fn pp_put_session(&mut self, s: PpSession) {
        self.pp.handlers = s.handlers;
        self.pp.mod_handlers = s.mod_handlers;
        if self.pp.handlers.is_empty() {
            return;
        }
        for (id, key) in s.gone {
            self.pp_instance_event(false, id, &key);
        }
        let loaded: Vec<(FormId, String)> = self.pp.instances.iter().map(|(id, i)| (*id, i.key.clone())).collect();
        for (id, key) in loaded {
            if self.pp_instance_event(true, id, &key) {
                self.pp.instances.remove(&id);
                self.clones.remove(&id);
                self.other.edids.remove(&id);
            }
        }
    }

    /// Run the deconstruct or reconstruct handlers for an instance, highest priority first
    /// [hypothesis: as event handler priorities]; true when one set a true function value.
    fn pp_instance_event(&mut self, reconstruct: bool, id: FormId, key: &str) -> bool {
        let mut hs: Vec<InstanceHandler> = self.pp.handlers.iter().filter(|h| h.reconstruct == reconstruct && h.filter.as_ref().is_none_or(|f| f.eq_ignore_ascii_case(key))).cloned().collect();
        hs.sort_by_key(|a| std::cmp::Reverse(a.priority));
        let mut skip = false;
        for h in hs {
            match self.run_function(&h.func, None, vec![XVal::Ref(id.0)], None, RunCtx::default()) {
                Ok(v) => skip |= self.udf_value_set && v.truthy(),
                Err(error) => self.emit(EventKind::ScriptError { unit: "instance handler".into(), error }),
            }
        }
        skip
    }

    /// Run the attach or detach handlers whose filters match, highest priority first
    /// [documented: OnAttachItemMod, the script gets the item mod and the object].
    fn pp_mod_event(&mut self, attach: bool, item_mod: FormId, object: FormId) {
        let mut hs: Vec<ModHandler> = self
            .pp
            .mod_handlers
            .iter()
            .filter(|h| h.attach == attach && h.item_mod.is_none_or(|m| m == item_mod) && h.object.is_none_or(|o| o == object || self.unclone(object) == o))
            .cloned()
            .collect();
        hs.sort_by_key(|a| std::cmp::Reverse(a.priority));
        for h in hs {
            if let Err(error) = self.run_function(&h.func, None, vec![XVal::Ref(item_mod.0), XVal::Ref(object.0)], None, RunCtx::default()) {
                self.emit(EventKind::ScriptError { unit: "item mod handler".into(), error });
            }
        }
    }

    /// Each frame (`Game::tick`): the CallLoop functions due. A function that sets its
    /// value sets the next delay, −1 ends the loop [documented: CallLoop].
    pub(crate) fn run_pp_loops(&mut self) {
        if self.pp.loops.is_empty() {
            return;
        }
        let now = self.time;
        let due: Vec<CallLoop> = self.pp.loops.iter().filter(|l| l.next <= now).cloned().collect();
        for l in due {
            let r = self.run_function(&l.func, Some(Value::Ref(l.on.0)), l.args.clone(), None, RunCtx { self_ref: Some(l.on), ..RunCtx::default() });
            let set = self.udf_value_set;
            let Some(i) = self.pp.loops.iter().position(|x| x.func == l.func && x.on == l.on) else { continue };
            match r {
                Ok(XVal::Num(d)) if set && d < 0.0 => {
                    let gone = self.pp.loops.remove(i);
                    for a in &gone.args {
                        self.ext.release(a);
                    }
                }
                Ok(v) => {
                    let x = &mut self.pp.loops[i];
                    if let (true, XVal::Num(d)) = (set, v) {
                        x.delay = d;
                    }
                    x.next = now + x.delay.max(0.0);
                }
                Err(error) => {
                    self.pp.loops[i].next = now + self.pp.loops[i].delay.max(0.0);
                    self.emit(EventKind::ScriptError { unit: "CallLoop".into(), error });
                }
            }
        }
        // A loop function's array result was handed over as a temporary (`run_function`);
        // the loops run between statements, so free it now (PR #1291 review). A string
        // result is an owned value, dropped with `r`.
        self.ext.sweep();
    }

    /// The vanilla mod flags of an object: an inventory item's own; a base form or an
    /// instance has none (AttachMod turns none on there, see "attachmod").
    fn pp_vanilla_flags(&mut self, object: FormId) -> u8 {
        let Some((c, item)) = self.temp_item(object) else { return 0 };
        self.world.inventory_mut(c).ok().and_then(|inv| inv.groups().find(|(i, _, _)| *i == item).map(|(_, e, _)| e.mods)).unwrap_or(0)
    }

    /// The item mod of a vanilla slot (1–3): the weapon's WMI1–3.
    fn pp_vanilla_mod(&self, object: FormId, slot: u8) -> Option<FormId> {
        let w = self.pp_base(object);
        match crate::nvse::record_value(self, w, &format!("WMI{slot}"), "") {
            Some(XVal::Ref(r)) if r != 0 => Some(FormId(r)),
            _ => None,
        }
    }

    /// The slots and mods attached to an object: the vanilla slots it has on, then the
    /// extended ones.
    fn pp_attached(&mut self, object: FormId) -> Vec<(String, FormId)> {
        let flags = self.pp_vanilla_flags(object);
        let mut out: Vec<(String, FormId)> = (1..=3u8).filter(|s| flags & (1 << (s - 1)) != 0).filter_map(|s| self.pp_vanilla_mod(object, s).map(|m| (s.to_string(), m))).collect();
        if let Some(m) = self.pp.attached.get(&object) {
            out.extend(m.values().cloned());
        }
        out
    }

    /// The degradation multiplier of a weapon: SetWeaponDegradationMult's, else the
    /// record's override when its flag is on, else fDamageToWeaponValue (0.2) [documented:
    /// SetWeaponDegradationMult, it "overrides the fDamageToWeaponValue"; the record's
    /// override as `fnv_combat::condition::weapon_wear` reads it; hypothesis: what the
    /// getter reads without a script's value].
    pub fn pp_degradation_mult(&self, weapon: FormId) -> f64 {
        if let Some(m) = self.pp.degradation.get(&weapon) {
            return *m;
        }
        let flags = match crate::nvse::record_value(self, weapon, "DNAM", "flags2") {
            Some(XVal::Num(x)) => x as u64,
            _ => 0,
        };
        // Bit 7, "Override - Damage to Weapon Mult" [documented: the Layer 1 schema].
        if flags & (1 << 7) != 0 {
            if let Some(XVal::Num(x)) = crate::nvse::record_value(self, weapon, "DNAM", "override_damage_to_weapon_mult") {
                return x;
            }
        }
        self.world.setting_f32("fDamageToWeaponValue").map_or(0.2, f64::from)
    }

    /// The sighting node the player's weapon lines up on: SetSightingNode's until the
    /// player's weapon changes [documented: SetSightingNode, "resets … once the player
    /// unequips or switches weapons"]. Follow-up: the engine's iron sights
    /// (`nvforever::iron_sights`) still look for the default node.
    pub fn pp_sighting_node(&mut self) -> String {
        let Some((w, node)) = self.pp.sighting.clone() else { return SIGHTING_NODE.into() };
        if crate::functions::items::equipped_weapon(self, FormId::PLAYER_REF) == w {
            node
        } else {
            self.pp.sighting = None;
            SIGHTING_NODE.into()
        }
    }

    // ---- AX variables ----

    fn ax_entries(&self, key: &str, f: Option<FormId>) -> Option<&Vec<Option<XVal>>> {
        self.pp.ax.get(&(f, key.to_string())).map(|(_, v)| v)
    }

    fn ax_drop(&mut self, v: Option<XVal>) {
        if let Some(v) = v {
            self.ext.release(&v);
        }
    }
}

/// The AX functions; `None` for another name.
fn ax_call(g: &mut Game, name: &str, args: &[XVal]) -> Option<XVal> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // `value key [index] [form]`: the value at the index (0 when omitted), the list
        // grown with nulls to reach it; an array is held as it is, not copied
        // [documented: AX Set, "Arrays are not flattened"]. Returns the value
        // (see docs/spec-notes/ext-pp.md §1).
        "ax_set" => {
            let value = args.first().cloned().unwrap_or(XVal::Num(0.0));
            let key = text(args, 1);
            let (index, f) = index_and_form(args, 2);
            // A negative index or one past the cap sets nothing and gives 0 [decision: PR
            // #1291 review; a list may not grow without bound].
            let Some(i) = usize::try_from(index.unwrap_or(0)).ok().filter(|&i| i < AX_MAX_ENTRIES) else { return Some(zero) };
            g.ext.retain(&value);
            let (_, list) = g.pp.ax.entry((f, key.clone())).or_insert_with(|| (key, Vec::new()));
            if list.len() <= i {
                list.resize(i + 1, None);
            }
            let old = list[i].replace(value.clone());
            g.ax_drop(old);
            value
        }
        // `key [index] form`: the value; 0 for none (see docs/spec-notes/ext-pp.md §1).
        "ax_get" => {
            let key = text(args, 0);
            let (index, f) = index_and_form(args, 1);
            let i = usize::try_from(index.unwrap_or(0)).ok();
            g.ax_entries(&key, f).and_then(|l| i.and_then(|i| l.get(i)).cloned().flatten()).unwrap_or(zero)
        }
        "ax_gettype" => {
            let key = text(args, 0);
            let (index, f) = index_and_form(args, 1);
            let i = usize::try_from(index.unwrap_or(0)).ok();
            XVal::Num(type_code(g.ax_entries(&key, f).and_then(|l| i.and_then(|i| l.get(i)))))
        }
        // [documented: AX Size, "the size"]; 0 for no such aux (see docs/spec-notes/ext-pp.md §1).
        "ax_size" => {
            let (_, f) = index_and_form(args, 1);
            XVal::Num(g.ax_entries(&text(args, 0), f).map_or(0.0, |l| l.len() as f64))
        }
        "ax_exists" => {
            let (_, f) = index_and_form(args, 1);
            yes(g.ax_entries(&text(args, 0), f).is_some())
        }
        // `toFind key [deepSearch] [form]`: the index of the first entry equal to it, −1
        // for none; a deep search also looks inside arrays held, giving the index of the
        // entry holding it [hypothesis: the page shows no result for either].
        "ax_find" => {
            let needle = args.first().cloned().unwrap_or(XVal::Num(0.0));
            let key = text(args, 1);
            // The deep flag is read and does not look inside arrays: 5 held in an array
            // entry was not found with it [observed: pp probe ax.find].
            let (_deep, f) = index_and_form(args, 2);
            let Some(list) = g.ax_entries(&key, f).cloned() else { return Some(XVal::Num(-1.0)) };
            let found = list.iter().position(|e| e.as_ref().is_some_and(|v| v == &needle));
            XVal::Num(found.map_or(-1.0, |i| i as f64))
        }
        // `key [index] form`: index omitted or −1 deletes the whole key [documented: AX
        // Erase; AX Null]; AX_Erase shifts the later entries down, AX_Null leaves a null.
        "ax_erase" | "ax_null" => {
            let key = text(args, 0);
            let (index, f) = index_and_form(args, 1);
            let k = (f, key.clone());
            match index.unwrap_or(-1) {
                -1 => {
                    if let Some((_, list)) = g.pp.ax.remove(&k) {
                        for v in list {
                            g.ax_drop(v);
                        }
                    }
                    zero
                }
                i => {
                    let Some((_, list)) = g.pp.ax.get_mut(&k) else { return Some(zero) };
                    let Some(i) = usize::try_from(i).ok().filter(|&i| i < list.len()) else { return Some(zero) };
                    let old = if name == "ax_erase" { list.remove(i) } else { list[i].take() };
                    g.ax_drop(old);
                    zero
                }
            }
        }
        // `key [form]`: the entries to the console [documented: AX Dump]; 1 when the key
        // exists (see docs/spec-notes/ext-pp.md §1).
        "ax_dump" => {
            let key = text(args, 0);
            let (_, f) = index_and_form(args, 1);
            let Some((shown, list)) = g.pp.ax.get(&(f, key.clone())).cloned() else { return Some(zero) };
            let on = f.map_or_else(|| "global".to_string(), |f| format!("{:08X}", f.0));
            g.console_out.push(format!("AX \"{shown}\" ({on}), {} entries", list.len()));
            for (i, e) in list.iter().enumerate() {
                let t = match e {
                    None => "null".to_string(),
                    Some(XVal::Num(x)) => number(*x),
                    Some(XVal::Ref(r)) => format!("{r:08X}"),
                    Some(XVal::Str(s)) => format!("\"{s}\""),
                    Some(XVal::Array(id)) => format!("array #{id}"),
                };
                g.console_out.push(format!("  [{i}] {t}"));
            }
            XVal::Num(1.0)
        }
        _ => return None,
    };
    Some(v)
}

/// Whether an array holds the value, at any depth (a cycle stops at 32 levels).
/// A string map of slot names to item mods.
fn slot_map(g: &mut Game, slots: Vec<(String, FormId)>) -> XVal {
    let entries = slots.into_iter().map(|(s, m)| (XKey::Str(s), XVal::Ref(m.0))).collect();
    XVal::Array(g.ext.new_array(ArrayKind::StringMap, entries))
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    if let Some(v) = ax_call(g, name, args) {
        return Some(Ok(v));
    }
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- form instances ----
        // `form [key]`: a new form standing for the base form, for this save; not of an
        // instance [documented: CreateFormInstance, "You can not create instances of
        // instances"]. NewWeaponInstance, the older name, takes a plugin name instead of a
        // key [documented: NewWeaponInstance; hypothesis: the plugin is not used, its
        // return value is the instance].
        "createforminstance" | "newweaponinstance" => {
            let Some(f) = form(args, 0) else { return Some(Ok(XVal::Ref(0))) };
            let base = g.pp_base(f);
            if g.pp.instances.contains_key(&base) || g.data.sig(g.unclone(base)).is_none() {
                return Some(Ok(XVal::Ref(0)));
            }
            let key = if name == "createforminstance" { text(args, 1) } else { String::new() };
            XVal::Ref(g.pp_new_instance(base, key).map_or(0, |f| f.0))
        }
        // [documented: GetStaticForm; GetWeaponBase (its older name)]: the base form an
        // instance (or a reference's) was made from; a form that is no instance itself.
        "getstaticform" | "getweaponbase" => match form(args, 0).or(subject) {
            Some(f) => {
                let b = g.pp_base(f);
                XVal::Ref(g.pp.instances.get(&b).map_or(b, |i| i.parent).0)
            }
            None => XVal::Ref(0),
        },
        // [documented: IsFormInstance; IsWeaponInstance].
        "isforminstance" | "isweaponinstance" => yes(form(args, 0).or(subject).is_some_and(|f| {
            let b = g.pp_base(f);
            g.pp.instances.contains_key(&b)
        })),
        // `priority script [key]` [documented: SetOnInstanceDeconstruct;
        // SetOnInstanceReconstruct]; the same script and key again replaces the
        // registration [hypothesis].
        "setoninstancedeconstruct" | "setoninstancereconstruct" => {
            let Some(func) = args.get(1).cloned() else { return Some(Ok(zero)) };
            let reconstruct = name == "setoninstancereconstruct";
            let filter = Some(text(args, 2)).filter(|s| !s.is_empty()).map(|s| s.to_lowercase());
            g.pp.handlers.retain(|h| !(h.reconstruct == reconstruct && h.func == func && h.filter == filter));
            g.pp.handlers.push(InstanceHandler { reconstruct, priority: num(args, 0).unwrap_or(0.0) as i32, func, filter });
            zero
        }
        // ---- item mods ----
        // `object slot [itemMod]`: slots "1"–"3" are the vanilla ones, on (an inventory
        // item's own flags, else the base form's or instance's) [documented: AttachMod's
        // first example "will enable slot 2"; an item mod given must be the slot's WMI];
        // any other slot name is an extended slot: the mod attached, or with none the one
        // there removed [documented: AttachMod's notes], firing the detach and attach
        // handlers [documented: OnAttachItemMod]. Returns 1 when something was done
        // (see docs/spec-notes/ext-pp.md §3).
        "attachmod" => {
            let Some(object) = form(args, 0) else { return Some(Ok(zero)) };
            let slot = text(args, 1);
            let item_mod = form(args, 2);
            if let Ok(s @ 1..=3) = slot.trim().parse::<u8>() {
                if item_mod.is_some_and(|m| g.pp_vanilla_mod(object, s) != Some(m)) || g.pp_vanilla_mod(object, s).is_none() {
                    return Some(Ok(zero));
                }
                let bit = 1u8 << (s - 1);
                // On a form instance (and so a base form [hypothesis]) nothing is turned on
                // and 0 returned [observed: pp probe attach.vanilla]: an inventory item only.
                let Some((c, item)) = g.temp_item(object) else { return Some(Ok(zero)) };
                let ok = g.world.inventory_mut(c).is_ok_and(|inv| {
                    let Some((_, e, _)) = inv.groups().find(|(i, _, _)| *i == item) else { return false };
                    inv.set_mods_one(item, e, e.mods | bit)
                });
                return Some(Ok(yes(ok)));
            }
            let lower = slot.trim().to_lowercase();
            // A slot name Devkit does not define attaches nothing and gives 0 [observed: pp
            // probe attach, "nvfslot"]; NVForever reads no Devkit (§6), so `slots` stays
            // empty outside tests until a loader fills it.
            if !g.pp.slots.contains(&lower) {
                return Some(Ok(zero));
            }
            let old = g.pp.attached.get(&object).and_then(|m| m.get(&lower)).map(|(_, m)| *m);
            match item_mod {
                Some(m) => {
                    g.pp.attached.entry(object).or_default().insert(lower, (slot, m));
                }
                None => {
                    if let Some(map) = g.pp.attached.get_mut(&object) {
                        map.remove(&lower);
                    }
                }
            }
            if let Some(o) = old.filter(|o| Some(*o) != item_mod) {
                g.pp_mod_event(false, o, object);
            }
            if let Some(m) = item_mod.filter(|m| Some(*m) != old) {
                g.pp_mod_event(true, m, object);
            }
            yes(old.is_some() || item_mod.is_some())
        }
        // [documented: GetAllAttachedMods, a string map of slot to item mod, vanilla and
        // extended].
        "getallattachedmods" => {
            let Some(object) = form(args, 0) else { return Some(Ok(slot_map(g, Vec::new()))) };
            let slots = g.pp_attached(object);
            slot_map(g, slots)
        }
        // `weapon "slot"` (the page GetAttachedMod, whose template the catalogue names
        // GetAllAttachedWeaponMods): the item mod in the slot [documented: its examples];
        // no slot, none (see docs/spec-notes/ext-pp.md §3).
        "getallattachedweaponmods" | "getattachedmod" => {
            let Some(object) = form(args, 0) else { return Some(Ok(XVal::Ref(0))) };
            let slot = text(args, 1).to_lowercase();
            let slots = g.pp_attached(object);
            XVal::Ref(slots.into_iter().find(|(s, _)| s.to_lowercase() == slot).map_or(0, |(_, m)| m.0))
        }
        // `object [slot]` (the page GetAllSlotMods): the mods that can go in the slot, as
        // a string map of slot to item mod; without a slot every slot's [documented].
        // Without Devkit's slot lists a vanilla slot can take its WMI and an extended slot
        // the mod attached there (see docs/spec-notes/ext-pp.md §3).
        "getallmodsinslot" | "getallslotmods" => {
            let Some(object) = form(args, 0) else { return Some(Ok(slot_map(g, Vec::new()))) };
            let want = text(args, 1).to_lowercase();
            let mut slots: Vec<(String, FormId)> = (1..=3u8).filter_map(|s| g.pp_vanilla_mod(object, s).map(|m| (s.to_string(), m))).collect();
            if let Some(m) = g.pp.attached.get(&object) {
                slots.extend(m.values().cloned());
            }
            slots.retain(|(s, _)| want.is_empty() || s.to_lowercase() == want);
            slot_map(g, slots)
        }
        // `object [checkVanillaMods]`: the object has a mod in an extended slot
        // [documented: HasExtendedMods; hypothesis: "uses the extended mod system" read as
        // having an extended mod], or, with the flag, vanilla mods: the 10mm pistol's base
        // form, with no extended mod and its three WMI slots filled, gives 0 without the
        // flag and 1 with it [observed: pp probes hasext.pistol, hasext.pistol.vanilla].
        "hasextendedmods" => {
            let Some(object) = form(args, 0) else { return Some(Ok(zero)) };
            let ext = g.pp.attached.get(&object).is_some_and(|m| !m.is_empty());
            let vanilla = num(args, 1).unwrap_or(0.0) != 0.0 && (1..=3u8).any(|s| g.pp_vanilla_mod(object, s).is_some());
            yes(ext || vanilla)
        }
        // `priority script runOn… filters…`, or `script 0` to remove [documented:
        // OnAttachItemMod; OnDetachItemMod: filter "1" the item mod, "2" the object].
        "onattachitemmod" | "ondetachitemmod" => {
            let attach = name == "onattachitemmod";
            if let Some(XVal::Ref(_)) = args.first() {
                let func = args[0].clone();
                if num(args, 1).unwrap_or(1.0) == 0.0 {
                    g.pp.mod_handlers.retain(|h| !(h.attach == attach && h.func == func));
                }
                return Some(Ok(zero));
            }
            let Some(func) = args.get(1).cloned() else { return Some(Ok(zero)) };
            let mut h = ModHandler { attach, priority: num(args, 0).unwrap_or(0.0) as i32, func, on_rebuild: num(args, 2).unwrap_or(0.0) != 0.0, item_mod: None, object: None };
            let mut k = 3;
            while k + 1 < args.len() {
                match text(args, k).trim() {
                    "1" => h.item_mod = form(args, k + 1),
                    "2" => h.object = form(args, k + 1),
                    _ => {}
                }
                k += 2;
            }
            g.pp.mod_handlers.retain(|x| !(x.attach == h.attach && x.func == h.func && x.item_mod == h.item_mod && x.object == h.object));
            g.pp.mod_handlers.push(h);
            zero
        }
        // Akimbo sets come from Devkit files [decision: not read, ext-pp.md §6]: no form
        // is one.
        "isakimboform" | "hasakimboset" => zero,
        // `akimbo left right` (the last two written) [documented: GetAkimboWeaponsAlt]:
        // none.
        "getakimboweaponsalt" => {
            let entries = (0..3).map(|i| (XKey::Num(f64::from(i)), if i == 0 { XVal::Num(0.0) } else { XVal::Ref(0) })).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        // Form traits come from Devkit files too [decision: ext-pp.md §6]: none exists, so
        // its type is −1 [documented: GetFormTraitType, "Trait does not exist"], its size
        // 0 and its value 0 [hypothesis].
        "getformtrait" | "getformtraitsize" | "hasformtrait" => zero,
        "getformtraittype" => XVal::Num(-1.0),
        // ---- the player and the weapons ----
        // [documented: IsThirdPerson] the camera the engine tells (`Game::set_first_person`).
        "isthirdperson" => yes(!g.senses.first_person),
        // `x y z` written, in degrees: the player's own angles, which the first-person
        // camera follows [documented: GetPlayerCameraRotation, "pitch, roll and yaw";
        // hypothesis: the engine does not tell the camera's, so a third-person orbit is
        // not seen].
        "getplayercamerarotation" => {
            let rot = g.ref_pos_rot(FormId::PLAYER_REF).map_or([0.0; 3], |(_, r)| r);
            let mut entries = vec![(XKey::Num(0.0), XVal::Num(0.0))];
            entries.extend(rot.iter().enumerate().map(|(i, a)| (XKey::Num(i as f64 + 1.0), XVal::Num(f64::from(*a).to_degrees()))));
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        // `weapon [ammo]` on an actor: the first ammo of the weapon's list the actor carries;
        // given an ammo, the next one after it that it carries, going round to it last
        // [documented: FindWeaponAmmo's example]; none carried: no form. An ammo not in the
        // list starts from the top (see docs/spec-notes/ext-pp.md §4).
        "findweaponammo" => {
            let (Some(actor), Some(w)) = (subject, form(args, 0)) else { return Some(Ok(XVal::Ref(0))) };
            let list = g.weapon_ammo(g.pp_base(w));
            let start = form(args, 1).and_then(|a| list.iter().position(|x| *x == a)).map_or(0, |i| i + 1);
            let n = list.len();
            let carried = |g: &mut Game, a: FormId| g.world.inventory_mut(actor).is_ok_and(|inv| inv.count(a) > 0);
            let mut found = None;
            for k in 0..n {
                let a = list[(start + k) % n];
                if carried(g, a) {
                    found = Some(a);
                    break;
                }
            }
            XVal::Ref(found.map_or(0, |a| a.0))
        }
        // On an inventory reference: the item replaced by another base form, keeping
        // whether it is worn [documented: ReplaceItemInInventory, "works on equipped
        // items"]; with `copy` its condition and mods kept too, `count` how many (all of
        // them when omitted; see docs/spec-notes/ext-pp.md §4).
        "replaceitemininventory" => {
            let (Some(r), Some(new)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let Some((c, item)) = g.temp_item(r) else { return Some(Ok(zero)) };
            let copy = num(args, 1).unwrap_or(0.0) != 0.0;
            let count = num(args, 2).map_or(i32::MAX, |n| n.max(0.0) as i32);
            let Ok(inv) = g.world.inventory_mut(c) else { return Some(Ok(zero)) };
            let taken = inv.take(item, count);
            for (e, n) in &taken {
                let extra = if copy { *e } else { fnv_sim::inventory::Extra { equipped: e.equipped, ..Default::default() } };
                inv.add_extra(new, *n, extra);
            }
            yes(!taken.is_empty())
        }
        // `node`: the player's weapon lines up on that node until it changes [documented:
        // SetSightingNode]; GetCurrentSightingNode reads it [documented].
        // Once a node is set, further sets do nothing while the player keeps that weapon:
        // after "Bip01 Head", setting "##SightingNode" or another name left it reading
        // "Bip01 Head" [observed: pp probes sight.set, sight.set2; hypothesis: until the
        // weapon changes, as GetCurrentSightingNode's page says of the set node].
        "setsightingnode" => {
            if g.pp.sighting.is_none() || g.pp_sighting_node() == SIGHTING_NODE {
                let w = crate::functions::items::equipped_weapon(g, FormId::PLAYER_REF);
                g.pp.sighting = Some((w, text(args, 0)));
            }
            zero
        }
        "getcurrentsightingnode" => XVal::Str(g.pp_sighting_node()),
        // `multiplier [weapon]` on a weapon reference, or `weapon multiplier` as the
        // example passes them [documented: SetWeaponDegradationMult]: the weapon's base.
        // GetWeaponDegradationMult (catalogued under the setter's name) reads it.
        // Follow-up: the engine's weapon wear does not read it yet (ext-pp.md §5).
        "setweapondegradationmult" | "getweapondegradationmult" => {
            let w = args.iter().find_map(|a| if let XVal::Ref(r) = a { (*r != 0).then_some(FormId(*r)) } else { None }).or(subject).map(|f| g.unclone(g.pp_base(f)));
            let m = args.iter().find_map(|a| if let XVal::Num(x) = a { Some(*x) } else { None });
            let Some(w) = w else { return Some(Ok(zero)) };
            match m.filter(|_| name == "setweapondegradationmult") {
                Some(m) => {
                    g.pp.degradation.insert(w, m);
                    zero
                }
                None => XVal::Num(g.pp_degradation_mult(w)),
            }
        }
        // The name (FULL) of the info's topic, not the info's own prompt (RNAM): for one of
        // Doc Mitchell's lines with a 27-character prompt the game gave the 26 characters
        // of its topic's name, and for a line with no prompt the 19 of its topic's
        // [observed: pp probes prompt, prompt.none; the page says "prompt"]. "" without a
        // topic or a name (see docs/spec-notes/ext-pp.md §4).
        "gettopicprompt" => XVal::Str(form(args, 0).and_then(|i| g.data.infos.get(&i)).and_then(|i| i.topic).and_then(|t| g.data.full_names.get(&t).cloned()).unwrap_or_default()),
        // `delay udf args…` on a reference (the player when none): the function called
        // every `delay` seconds, −1 removing it [documented: CallLoop]. The first call
        // after one delay, and the same function on the same reference registered again
        // replaces it (see docs/spec-notes/ext-pp.md §4). Not kept across a load [documented: the page's
        // example, "auto-removed when loading a game"].
        "callloop" => {
            let Some(func) = args.get(1).cloned() else { return Some(Ok(zero)) };
            let delay = num(args, 0).unwrap_or(0.0);
            let on = subject.unwrap_or(FormId::PLAYER_REF);
            if let Some(i) = g.pp.loops.iter().position(|l| l.func == func && l.on == on) {
                let gone = g.pp.loops.remove(i);
                for a in &gone.args {
                    g.ext.release(a);
                }
            }
            if delay >= 0.0 {
                let rest: Vec<XVal> = args.iter().skip(2).cloned().collect();
                for a in &rest {
                    g.ext.retain(a);
                }
                g.pp.loops.push(CallLoop { func, on, args: rest, delay, next: g.time + delay });
            }
            zero
        }
        // `"condition"`: 1 when the text holds as an `if` condition [documented:
        // EvaluateString]; a text that does not compile is false (see docs/spec-notes/ext-pp.md §4).
        "evaluatestring" => {
            let t = text(args, 0);
            match g.evaluate_string(&t) {
                Ok(b) => yes(b),
                Err(error) => {
                    g.emit(EventKind::ScriptError { unit: "EvaluateString".into(), error });
                    zero
                }
            }
        }
        _ => return None,
    };
    Some(Ok(v))
}
