//! JIP LN NVSE, batch 6 (#1006): destruction data, creature data and messages
//! [documented: each page in the wiki export; docs/spec-notes/jip-records-6.md]. Setters
//! change the form for the session, read by the getters and by the game.

use crate::data::{DestructionDef, DestructionStage};
use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use serde_json::Value as Json;
use std::collections::{HashMap, HashSet};

/// What this batch keeps for the session (none of it is saved: AddDestructionStage's
/// "the changes will not be saved with the game" [documented]; the rest [hypothesis]).
#[derive(Debug, Default)]
pub struct Jip6State {
    /// Destruction data a setter changed, whole, by base form (`Game::destruction`).
    /// Carried across same-run loads via `Game::take_session` / `put_session`, not saved.
    pub destruction: HashMap<FormId, DestructionDef>,
    /// A creature's model list with each model on or off (ToggleCreatureModel), by base.
    pub creature_models: HashMap<FormId, Vec<(String, bool)>>,
    /// Messages SetMessageDisabled turned off.
    pub disabled_messages: HashSet<FormId>,
    /// ToggleCraftingMessages 0.
    pub crafting_messages_off: bool,
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

impl Game<'_> {
    /// A base form's destruction data: a setter's session copy first, else the record's.
    pub fn destruction(&self, base: FormId) -> Option<&DestructionDef> {
        self.other.jip6.destruction.get(&base).or_else(|| self.data.destructibles.get(&base))
    }

    /// The session copy of a base form's destruction data to change; `None` for a form
    /// without any.
    pub(crate) fn destruction_mut(&mut self, base: FormId) -> Option<&mut DestructionDef> {
        if !self.other.jip6.destruction.contains_key(&base) {
            let d = self.data.destructibles.get(&base)?.clone();
            self.other.jip6.destruction.insert(base, d);
        }
        self.other.jip6.destruction.get_mut(&base)
    }

    /// A message's flags (DNAM: 1 Message Box, 2 Auto-display), SetMessageFlags' change
    /// first [documented: GetMessageFlags].
    pub fn message_flags(&self, m: FormId) -> u32 {
        match crate::nvse::record_value(self, m, "DNAM", "") {
            Some(XVal::Num(x)) => x as u32,
            _ => self.data.messages.get(&m).map_or(0, |d| u32::from(d.message_box)),
        }
    }

    /// The seconds a message that is not a box stays on screen (TNAM),
    /// SetMessageDisplayTime's change first.
    pub fn message_seconds(&self, m: FormId) -> Option<u32> {
        match self.field_overrides.get(&(m, "TNAM/".to_string())) {
            Some(XVal::Num(x)) => Some(*x as u32),
            _ => self.data.messages.get(&m).and_then(|d| d.display_seconds),
        }
    }

    /// Whether SetMessageDisabled turned a message off: "a disabled message will never be
    /// displayed by the game" [documented: SetMessageDisabled].
    pub fn message_disabled(&self, m: FormId) -> bool {
        self.other.jip6.disabled_messages.contains(&m)
    }

    /// Whether the crafting menu's "added" lines show (ToggleCraftingMessages)
    /// [documented: ToggleCraftingMessages, "the corner messages that display when
    /// crafting items"].
    pub fn crafting_messages_shown(&self) -> bool {
        !self.other.jip6.crafting_messages_off
    }

    /// A creature's model list (NIFZ) with each model on or off, ToggleCreatureModel's
    /// changes first.
    fn creature_models(&self, crea: FormId) -> Vec<(String, bool)> {
        if let Some(v) = self.other.jip6.creature_models.get(&crea) {
            return v.clone();
        }
        let list = match self.data.record_field(crea, "NIFZ", "") {
            Some(Json::Array(a)) => a.iter().filter_map(Json::as_str).map(str::to_string).collect(),
            Some(Json::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        };
        list.into_iter().filter(|s| !s.is_empty()).map(|s| (s, true)).collect()
    }
}

/// A destruction stage's numeric trait 0-4 [documented: GetNthDestructionStageTrait, 0
/// health percentage, 1 self damage per second, 2 model damage stage, 3 flags, 4 debris
/// count].
fn stage_trait(s: &DestructionStage, t: i64) -> Option<f64> {
    Some(match t {
        0 => f64::from(s.health_percent),
        1 => f64::from(s.self_damage_per_second),
        2 => f64::from(s.damage_stage),
        3 => f64::from(s.flags),
        4 => f64::from(s.debris_count),
        _ => return None,
    })
}

/// A value past what the trait holds is not set: a health percentage past 0-100, a
/// byte past 0-255, a number past the i32 field, NaN or infinity [hypothesis: the game
/// keeps the record's fields in range].
fn set_stage_trait(s: &mut DestructionStage, t: i64, v: f64) {
    if !v.is_finite() {
        return;
    }
    let byte = (0.0..=255.0).contains(&v).then_some(v as u8);
    let int = (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&v).then_some(v as i32);
    match t {
        // The health percentage takes any whole number: it wraps as an unsigned 32-bit
        // value and is kept modulo 101 (150 reads 49, 1000 reads 91, -1 reads 67)
        // [observed: jip6 dest.settrait.1.0.*, jip6b dest.hp.*].
        0 => {
            if let Some(i) = int {
                s.health_percent = (i as u32 % 101) as u8;
            }
        }
        1 => {
            if let Some(i) = int {
                s.self_damage_per_second = i;
            }
        }
        // The model damage stage takes any whole number the same way, modulo 9 (10 reads
        // 1, 255 reads 3, 256 reads 4, -1 reads 3) [observed: jip-records-6.md §1, #2039].
        2 => {
            if let Some(i) = int {
                s.damage_stage = (i as u32 % 9) as u8;
            }
        }
        // Flags 1 cap damage, 2 disable, 4 destroy [documented: the page's table].
        3 => {
            if let Some(b) = byte {
                s.flags = b & 7;
            }
        }
        4 => {
            if let Some(i) = int {
                s.debris_count = i;
            }
        }
        _ => {}
    }
}

/// Each stage's DSTD index is its place in the list after a stage is added or removed
/// [hypothesis: the game keeps them in order and numbers them so].
fn renumber(d: &mut DestructionDef) {
    for (i, s) in d.stages.iter_mut().enumerate() {
        s.index = i.min(255) as u8;
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let of_sig = |g: &Game, f: Option<FormId>, sig: &[u8; 4]| f.filter(|&f| g.data.sig(f) == Some(*sig));
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- destruction data: `object` is a base form (a reference reads its base)
        // [documented: the pages' `object:ref`; "only usable with object types that have
        // Destruction Data"]. A form without any reads 0 / none / "".
        "getdestructiondatatargetable" => yes(form(args, 0).and_then(|f| g.destruction(base(g, f))).is_some_and(|d| d.vats_targetable)),
        // Setters return 0 [hypothesis: as the record setters of batch 5 were observed to].
        "setdestructiondatatargetable" => {
            let on = num(args, 1, 0.0) != 0.0;
            if let Some(d) = form(args, 0).map(|f| base(g, f)).and_then(|b| g.destruction_mut(b)) {
                d.vats_targetable = on;
            }
            zero
        }
        "getnthdestructionstagedebris" | "getnthdestructionstageexplosion" | "getnthdestructionstagereplacement" | "getnthdestructionstagetrait" => {
            let i = usize::try_from(num(args, 1, -1.0) as i64).ok();
            let stage = form(args, 0).and_then(|f| g.destruction(base(g, f))).and_then(|d| d.stages.get(i?));
            match name {
                "getnthdestructionstagedebris" => XVal::Ref(stage.and_then(|s| s.debris).map_or(0, |f| f.0)),
                "getnthdestructionstageexplosion" => XVal::Ref(stage.and_then(|s| s.explosion).map_or(0, |f| f.0)),
                "getnthdestructionstagereplacement" => XVal::Str(stage.and_then(|s| s.model.clone()).unwrap_or_default()),
                _ => XVal::Num(stage.and_then(|s| stage_trait(s, num(args, 2, -1.0) as i64)).unwrap_or(0.0)),
            }
        }
        // `object index value`; the debris and explosion may be left out for none
        // [documented: SetNthDestructionStageDebris, "to None"].
        "setnthdestructionstagedebris" | "setnthdestructionstageexplosion" | "setnthdestructionstagereplacement" | "setnthdestructionstagetrait" => {
            let i = usize::try_from(num(args, 1, -1.0) as i64).ok();
            let (new_form, path, t, v) = (form(args, 2), text(args, 2), num(args, 2, -1.0) as i64, num(args, 3, 0.0));
            if let Some(s) = form(args, 0).map(|f| base(g, f)).and_then(|b| g.destruction_mut(b)).and_then(|d| d.stages.get_mut(i?)) {
                match name {
                    "setnthdestructionstagedebris" => s.debris = new_form,
                    "setnthdestructionstageexplosion" => s.explosion = new_form,
                    "setnthdestructionstagereplacement" => {
                        s.model = Some(path).filter(|p| !p.is_empty());
                        // the replacement setter alone returns 1 [observed: jip6 dest.setrepl;
                        // the other stage setters 0, jip6b dest.setexpl.result]
                        return Some(Ok(XVal::Num(1.0)));
                    }
                    _ => set_stage_trait(s, t, v),
                }
            }
            zero
        }
        // `object index healthPercent damageStage`: the new stage count, -1 when nothing
        // was added [documented: AddDestructionStage]. An index past the end adds at the
        // end [hypothesis].
        "adddestructionstage" => {
            // A NaN or infinite index adds nothing.
            let (i, health, stage) = (num(args, 1, -1.0), num(args, 2, 0.0), num(args, 3, 0.0));
            let ok = i.is_finite() && i >= 0.0 && health.is_finite() && stage.is_finite();
            let i = i as i64;
            // A form without destruction data gets it (not VATS targetable) with the new
            // stage [observed: jip6b dest.add.none]. The result is the stage's index; an
            // index past the end adds nothing and gives -1 [observed: jip6 dest.add.*,
            // jip6b dest.add.end, dest.add.0].
            let b = form(args, 0).map(|f| base(g, f));
            if let Some(b) = b.filter(|&b| ok && g.destruction(b).is_none()) {
                g.other.jip6.destruction.insert(b, DestructionDef::default());
            }
            let d = b.and_then(|b| g.destruction_mut(b));
            match d.filter(|d| ok && i as usize <= d.stages.len()) {
                Some(d) => {
                    let s = DestructionStage { health_percent: health.clamp(0.0, 100.0) as u8, damage_stage: stage.clamp(0.0, 255.0) as u8, ..DestructionStage::default() };
                    d.stages.insert(i as usize, s);
                    renumber(d);
                    XVal::Num(i as f64)
                }
                None => XVal::Num(-1.0),
            }
        }
        // `object index`: the new stage count, -1 when nothing was removed [documented].
        "removedestructionstage" => {
            let i = usize::try_from(num(args, 1, -1.0) as i64).ok();
            let d = form(args, 0).map(|f| base(g, f)).and_then(|b| g.destruction_mut(b));
            match (d, i) {
                // the removed stage's index [observed: jip6 dest.remove.1, dest.remove.last]
                (Some(d), Some(i)) if i < d.stages.len() => {
                    d.stages.remove(i);
                    renumber(d);
                    XVal::Num(i as f64)
                }
                _ => XVal::Num(-1.0),
            }
        }
        // ---- creatures: `[baseForm]`, else the calling reference's base [documented:
        // the pages' examples `RexRef.GetCreatureDamage`, `GetCreatureDamage Rex`].
        // DATA's damage, RNAM's reach, LNAM's melee weapon list [documented: the pages;
        // the fields: xEdit CREA]. Not a creature: 0.
        "getcreaturedamage" | "getcreaturereach" | "getcreatureweaponlist" => {
            let c = of_sig(g, form(args, 0).or(subject).map(|f| base(g, f)), b"CREA");
            let read = |sub: &str, path: &str| c.and_then(|c| crate::nvse::record_value(g, c, sub, path));
            match name {
                "getcreaturedamage" => XVal::Num(read("DATA", "damage").and_then(|v| v.num().ok()).unwrap_or(0.0)),
                "getcreaturereach" => XVal::Num(read("RNAM", "").and_then(|v| v.num().ok()).unwrap_or(0.0)),
                _ => XVal::Ref(match read("LNAM", "") {
                    Some(XVal::Ref(r)) => r,
                    _ => 0,
                }),
            }
        }
        // `damage [baseForm]`.
        // The field is an i16 (xEdit CREA DATA): a value past it, NaN or infinity is not
        // set [hypothesis].
        "setcreaturedamage" => {
            let v = num(args, 0, f64::NAN).trunc();
            // held to 0-65535 [observed: jip6 cr.setdamage.*, jip6b cr.dmg.*]
            if let Some(c) = of_sig(g, form(args, 1).or(subject).map(|f| base(g, f)), b"CREA").filter(|_| !v.is_nan()) {
                g.field_overrides.insert((c, "DATA/damage".into()), XVal::Num(v.clamp(0.0, 65535.0)));
            }
            zero
        }
        // `creature`: CSCR, the creature whose sounds it uses [documented:
        // GetCreatureSoundsTemplate; the field: xEdit CREA "Inherits Sounds from"].
        "getcreaturesoundstemplate" => XVal::Ref(match of_sig(g, form(args, 0).map(|f| base(g, f)), b"CREA").and_then(|c| crate::nvse::record_value(g, c, "CSCR", "")) {
            Some(XVal::Ref(r)) => r,
            _ => 0,
        }),
        // `creature [template]`: left out clears it [documented: SetCreatureSoundsTemplate].
        "setcreaturesoundstemplate" => {
            if let Some(c) = of_sig(g, form(args, 0).map(|f| base(g, f)), b"CREA") {
                let t = of_sig(g, form(args, 1), b"CREA").map_or(0, |t| t.0);
                g.field_overrides.insert((c, "CSCR/".into()), XVal::Ref(t));
            }
            zero
        }
        // The model list (NIFZ): `path [baseForm]` in it, case-insensitive [documented:
        // CreatureHasModel], on or off [hypothesis: "the model-list includes"]; the paths
        // toggled on as an array [documented: GetCreatureModels]; `path on [baseForm]`
        // turns one on or off, 1 when the list has it [documented: ToggleCreatureModel's
        // `success`; hypothesis: 0 for a path not in the list].
        "creaturehasmodel" => {
            let p = text(args, 0);
            let c = of_sig(g, form(args, 1).or(subject).map(|f| base(g, f)), b"CREA");
            // a model turned off is not had [observed: jip6 cr.toggle.off, jip6b cr.toggle.offagain]
            yes(c.is_some_and(|c| g.creature_models(c).iter().any(|(m, on)| *on && m.eq_ignore_ascii_case(&p))))
        }
        "getcreaturemodels" => {
            let c = of_sig(g, form(args, 0).or(subject).map(|f| base(g, f)), b"CREA");
            let on: Vec<String> = c.map(|c| g.creature_models(c).into_iter().filter(|m| m.1).map(|m| m.0).collect()).unwrap_or_default();
            let entries = on.into_iter().enumerate().map(|(i, m)| (XKey::Num(i as f64), XVal::Str(m))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        "togglecreaturemodel" => {
            let (p, on) = (text(args, 0), num(args, 1, 0.0) != 0.0);
            let Some(c) = of_sig(g, form(args, 2).or(subject).map(|f| base(g, f)), b"CREA") else { return Some(Ok(zero)) };
            // 1 when the model's state changed; turning on a path not in the list adds it
            // [observed: jip6 cr.toggle.*, jip6b cr.toggle.add, cr.toggle.offagain]
            let mut list = g.creature_models(c);
            let changed = match list.iter_mut().find(|(m, _)| m.eq_ignore_ascii_case(&p)) {
                Some(m) => std::mem::replace(&mut m.1, on) != on,
                None if on && !p.is_empty() => {
                    list.push((p, true));
                    true
                }
                None => false,
            };
            if changed {
                g.other.jip6.creature_models.insert(c, list);
            }
            yes(changed)
        }
        // ---- messages [documented: the pages; flags 1 Is Message Box, 2 Auto-display].
        "getmessageflags" => XVal::Num(of_sig(g, form(args, 0), b"MESG").map_or(0, |m| g.message_flags(m)) as f64),
        // Values > 3 (`7`) are ignored without mutating DNAM [observed:
        // `w6d2p3_47_jip6_messages_creatures_and_bptd`]; `< 0` (`-1`) rejection and `.trunc()`
        // are [hypothesis], and ignoring omitted/non-finite args is [design].
        "setmessageflags" => {
            let v = num(args, 1, f64::NAN).trunc();
            if let Some(m) = of_sig(g, form(args, 0), b"MESG").filter(|_| (0.0..=3.0).contains(&v)) {
                g.field_overrides.insert((m, "DNAM/".into()), XVal::Num(v));
            }
            zero
        }
        // TNAM is a u32 of seconds (xEdit MESG): a negative value, one past it, NaN or
        // infinity is not set (see docs/spec-notes/jip-records-6.md §3).
        "setmessagedisplaytime" => {
            let v = num(args, 1, f64::NAN).trunc();
            if let Some(m) = of_sig(g, form(args, 0), b"MESG").filter(|_| (0.0..=f64::from(u32::MAX)).contains(&v)) {
                g.field_overrides.insert((m, "TNAM/".into()), XVal::Num(v));
            }
            zero
        }
        "getmessagedisabled" => yes(of_sig(g, form(args, 0), b"MESG").is_some_and(|m| g.message_disabled(m))),
        // `messageOrList disable`: a form list disables or enables every message in it
        // [documented: SetMessageDisabled].
        "setmessagedisabled" => {
            let off = num(args, 1, 0.0) != 0.0;
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            let ms: Vec<FormId> = if g.data.sig(f) == Some(*b"FLST") { crate::functions::refs::list_members(g, f) } else { vec![f] };
            for m in ms.into_iter().filter(|&m| g.data.sig(m) == Some(*b"MESG")) {
                if off {
                    g.other.jip6.disabled_messages.insert(m);
                } else {
                    g.other.jip6.disabled_messages.remove(&m);
                }
            }
            zero
        }
        // the state before the call, 1 shown [observed: jip6 msg.crafting, jip6b msg.crafting.twice]
        "togglecraftingmessages" => {
            let was = g.crafting_messages_shown();
            g.other.jip6.crafting_messages_off = num(args, 0, 1.0) == 0.0;
            yes(was)
        }
        _ => return None,
    };
    Some(Ok(v))
}
