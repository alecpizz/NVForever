//! The 1111 NVSE plugin's functions (One111): the condition lists of records, the
//! damage-calculation event values, the weapon's additional stat vectors, armour totals,
//! ammo effects and a few weapon and ammo setters [documented: each page in the wiki
//! export; docs/spec-notes/ext-one111.md]. What the probe set `one111` showed in the game
//! (OneOneOneOne.dll installed, 2026-11-01) is marked [observed: one111]; what a page
//! leaves open and no probe checked is marked [hypothesis]. Every change lasts for the
//! session.

use crate::cond::{Cmp, Condition, Operand, RunOn};
use crate::game::Game;
use fnv_script::vm::{ArgValue, ArrayKind, Value, XKey, XVal};
use fnv_sim::FormId;
use std::collections::HashMap;

/// "INT_MIN aka -2147483648", the getters' "something went wrong" [documented:
/// GetConditionFormOneInt; GetOnDamageCalcNumb].
pub const INT_MIN: f64 = -2_147_483_648.0;

/// GetVampire's opcode: AddConditionForm… makes a condition "with function GetVampire ==
/// 1" [documented: AddConditionFormOne; SetConditionFormOneInt's example, 4136 is
/// GetVampire].
pub const GET_VAMPIRE: u16 = 4136;

/// A condition list, by the kind of entity [documented: GetConditionFormOneCount,
/// GetConditionFormTwoCount, GetConditionFormThreeCount]: FormOne a recipe, idle,
/// package or topic info; FormTwo an actor effect's or ingestible's effect, a message's
/// button or a terminal's menu item (`first`); FormThree a quest's (`first` −1) or an
/// objective's target (`first` the objective, `second` the target), a perk's (`first` −1)
/// or a perk entry's condition page (`first` the entry, `second` the page).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListKey {
    One(FormId),
    Two(FormId, i32),
    Three(FormId, i32, i32),
}

/// The values of the hit being calculated, as One111's damage events give them
/// [documented: GetOnDamageCalcNumb, GetOnDamageCalcFlag, GetOnDamageCalcRef,
/// GetOnDamageCalcAdditionalNumb]: `numbers` by GetOnDamageCalcNumb's IDs 0–9 (`None` not
/// set), `additional` by GetOnDamageCalcAdditionalNumb's 0–5, `flags` the HitData flags,
/// `refs` source, target, weapon and projectile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DamageCalc {
    pub numbers: [Option<f64>; 10],
    pub additional: [Option<f64>; 6],
    pub flags: u32,
    pub refs: [Option<FormId>; 4],
}

/// This plugin's session state.
#[derive(Debug, Default)]
pub struct One111State {
    /// Condition lists a script changed: the whole changed list, read instead of the
    /// record's.
    pub lists: HashMap<ListKey, Vec<Condition>>,
    /// The weapons' additional stat vectors: (weapon base, vector type 0–6) -> the pairs
    /// (health 0–1, value), sorted by health [documented: GetWeapAdditionalVectorValue].
    pub vectors: HashMap<(FormId, u8), Vec<(f64, f64)>>,
    /// The hit whose damage event is running (`Game::one111_damage_calc_begin`); `None`
    /// outside one.
    pub damage_calc: Option<DamageCalc>,
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize) -> Option<f64> {
    match args.get(k)? {
        XVal::Num(x) => Some(*x),
        _ => None,
    }
}

fn int(args: &[XVal], k: usize, dflt: i64) -> i64 {
    num(args, k).map_or(dflt, |x| x as i64)
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

/// How a parameter type reads in GetConditionForm…Int's value IDs 1 and 2 [documented:
/// GetConditionFormOneInt: −1 no such parameter, 0 int, 1 float, 2 reference, 3 base
/// form; GetConditionFormOneFloat, "what parameters count as int": integers and the
/// enumerations]. The xEdit types naming a placed thing are references, the other record
/// types base forms (see docs/spec-notes/ext-one111.md §1); no condition function takes a float
/// parameter in the catalogue, so 1 never comes back.
fn param_kind(ty: &str) -> i64 {
    const REFS: &[&str] = &["ptReference", "ptActor", "ptObjectReference", "ptMapMarker", "ptContainer", "ptFurniture"];
    const NUMBERS: &[&str] = &[
        "ptInteger",
        "ptQuestStage",
        "ptQuestObjective",
        "ptVariableName",
        "ptVATSValueParam",
        "ptActorValue",
        "ptSex",
        "ptAxis",
        "ptMiscStat",
        "ptCriticalStage",
        "ptMenuMode",
        "ptFormType",
        "ptAlignment",
        "ptEquipType",
        "ptBodyLocation",
        "ptPlayerAction",
        "ptCreatureType",
        "ptVATSValueFunction",
        "ptCrimeType",
    ];
    if NUMBERS.contains(&ty) {
        0
    } else if REFS.contains(&ty) {
        2
    } else {
        3
    }
}

/// A condition as AddConditionForm… makes it: run on the subject, GetVampire == 1
/// [documented: AddConditionFormOne, "What actually happens"].
fn new_condition() -> Condition {
    Condition { cmp: Cmp::Eq, or: false, run_on: RunOn::Subject, function: GET_VAMPIRE - 0x1000, params: Vec::new(), rhs: Operand::Value(1.0) }
}

const CMPS: [Cmp; 6] = [Cmp::Eq, Cmp::Ne, Cmp::Gt, Cmp::Ge, Cmp::Lt, Cmp::Le];

impl Game<'_> {
    /// A condition list: the script's changed copy, else the record's; `None` for an
    /// entity or index that has none ("something is wrong").
    pub fn one111_list(&self, key: ListKey) -> Option<Vec<Condition>> {
        if let Some(l) = self.one111.lists.get(&key) {
            return Some(l.clone());
        }
        let d = self.data;
        let at = |i: i32| usize::try_from(i).ok();
        match key {
            ListKey::One(f) => {
                let f = self.unclone(f);
                if let Some(r) = d.recipes.get(&f) {
                    return Some(r.conditions.clone());
                }
                if let Some(i) = d.idles.get(&f) {
                    return i.conditions.clone();
                }
                if let Some(i) = d.infos.get(&f) {
                    return Some(i.conditions.clone());
                }
                self.package_def(&f).map(|p| p.conditions.clone())
            }
            ListKey::Two(f, i) => {
                let (f, i) = (self.unclone(f), at(i)?);
                if let Some(l) = d.effect_items.get(&f) {
                    return l.get(i).map(|e| e.conditions.clone());
                }
                if let Some(m) = d.messages.get(&f) {
                    return m.button_conditions.get(i).cloned();
                }
                d.terminals.get(&f).and_then(|t| t.items.get(i)).map(|t| t.conditions.clone())
            }
            ListKey::Three(f, first, second) => {
                let f = self.unclone(f);
                if let Some(q) = d.quests.get(&f) {
                    // The objective by its place among the quest's objectives, not the
                    // number the record gives it [observed: one111, Run Goodsprings Run's
                    // objective 0 has a target, its objectives being numbered 10 and up;
                    // hypothesis: the places in objective-number order (the record lists
                    // 70 before 60, so its own order may differ past the first ones; not
                    // probed)].
                    if first == -1 {
                        return Some(q.conditions.clone());
                    }
                    return q.objectives.values().nth(at(first)?).and_then(|o| o.targets.get(at(second)?)).map(|t| t.conditions.clone());
                }
                let p = d.perks.get(&f)?;
                if first == -1 {
                    return p.conditions.clone();
                }
                // The entry in record order, its conditions on page `second` (0–2).
                let e = p.entries.get(at(first)?)?;
                if !(0..=2).contains(&second) {
                    return None;
                }
                match &e.effect {
                    crate::data::PerkEffect::EntryPoint(ep) => ep.conditions.as_ref().map(|cs| cs.iter().filter(|(tab, _)| i32::from(*tab) == second).map(|(_, c)| c.clone()).collect()),
                    // An ability or quest entry has no condition pages: −1 [observed:
                    // one111, Nerd Rage!'s entry 0, an ability, pages 0 and 1].
                    _ => None,
                }
            }
        }
    }

    /// The list to change: the changed copy, made from the record's on the first change.
    fn one111_list_mut(&mut self, key: ListKey) -> Option<&mut Vec<Condition>> {
        if !self.one111.lists.contains_key(&key) {
            let l = self.one111_list(key)?;
            self.one111.lists.insert(key, l);
        }
        self.one111.lists.get_mut(&key)
    }

    /// The xEdit parameter types of a condition function.
    fn one111_param_types(&self, function: u16) -> Vec<String> {
        self.data.param_types.0.get(&function).cloned().unwrap_or_default()
    }

    /// GetConditionForm…Int's parameter type for parameter `k` (0 or 1).
    fn one111_param_kind(&self, c: &Condition, k: usize) -> i64 {
        let types = self.one111_param_types(c.function);
        if let Some(t) = types.get(k) {
            return param_kind(t);
        }
        if !types.is_empty() || self.data.param_types.0.contains_key(&c.function) {
            return -1;
        }
        // A function the catalogue gives no types: by what the record held.
        match c.params.get(k) {
            Some(ArgValue::Value(Value::Ref(_))) => 3,
            Some(_) => 0,
            None => -1,
        }
    }

    /// The engine starts One111's damage events for a hit: the values the handlers read
    /// and may change (GetOnDamageCalc…, SetOnDamageCalcNumb). Follow-up: the engine's
    /// damage calculation does not call this yet (ext-one111.md §2).
    pub fn one111_damage_calc_begin(&mut self, d: DamageCalc) {
        self.one111.damage_calc = Some(d);
    }

    /// The events are over: the values as the handlers left them.
    pub fn one111_damage_calc_end(&mut self) -> Option<DamageCalc> {
        self.one111.damage_calc.take()
    }

    /// A weapon's additional stat at a health (0–1), as GetWeapAdditionalVectorValueScale
    /// gives it; `None` when the vector is empty (for the engine's damage, value and spread
    /// to apply: a follow-up, ext-one111.md §3).
    pub fn one111_vector_at(&self, weapon: FormId, kind: u8, health: f64) -> Option<f64> {
        let v = self.one111.vectors.get(&(weapon, kind)).filter(|v| !v.is_empty())?;
        let (lo, hi) = (v[0], v[v.len() - 1]);
        if health <= lo.0 {
            return Some(lo.1);
        }
        if health >= hi.0 {
            return Some(hi.1);
        }
        let i = v.iter().position(|p| p.0 >= health)?;
        let (a, b) = (v[i - 1], v[i]);
        if (b.0 - a.0).abs() < f64::EPSILON {
            return Some(b.1);
        }
        Some(a.1 + (health - a.0) / (b.0 - a.0) * (b.1 - a.1))
    }

    /// The DR or DT of the armour an actor wears: each piece's record value times its
    /// condition's share (`Game::armor_condition_mult`, honoring SetArmorConditionPenalty
    /// and Stewie's `Tweaks::armor_condition_penalty`) [documented: GetActorTotalArmorDR,
    /// "calculated DR of all armor"; hypothesis: the condition is part of "calculated"].
    fn one111_armor_total(&mut self, actor: FormId, dt: bool) -> f64 {
        let Ok(inv) = self.world.inventory_mut(actor) else { return 0.0 };
        let worn: Vec<(FormId, f32)> = inv.equipped().into_iter().map(|i| (i, inv.condition_in_use(i).unwrap_or(1.0))).collect();
        let mut total = 0.0;
        for (item, condition) in worn {
            if self.data.sig(self.unclone(item)) != Some(*b"ARMO") {
                continue;
            }
            let v = match crate::nvse::record_value(self, item, "DNAM", if dt { "dt" } else { "dr" }) {
                Some(XVal::Num(x)) => x,
                _ => 0.0,
            };
            total += v * self.armor_condition_mult(condition);
        }
        total
    }

    /// An actor's spread with its equipped weapon: the wobble (the actor's part) and the
    /// weapon's [documented: GetCalculatedSpreadAlt; the actor's part by the GECK wiki's
    /// Gun Spread Formula (New Vegas), the same terms as `fnv_combat::spread::actor_spread`:
    /// unaimed, moving, standing, the Strength and skill shortfalls, times
    /// 1 − fWobbleToSkillConversion × skill/100, at least fMinGunSpreadValue; hypothesis:
    /// crippled arms and perks left out, the full spread the sum of the two parts]. The
    /// weapon's part is its Min Spread, or the Min Spread stat vector at its condition
    /// [documented: GetWeapAdditionalVectorValue, 2 "replaces the weapon form value"].
    fn one111_spread(&mut self, actor: FormId) -> (f64, f64) {
        let Some(w) = crate::functions::items::equipped_weapon(self, actor) else { return (0.0, 0.0) };
        let base = crate::functions::base_of(self, w).unwrap_or(w);
        let field = |g: &Game, path: &str| match crate::nvse::record_value(g, base, "DNAM", path) {
            Some(XVal::Num(x)) => x,
            _ => 0.0,
        };
        let condition = self.world.inventory_mut(actor).ok().and_then(|inv| inv.condition_in_use(base)).map_or(1.0, f64::from);
        let weapon = self.one111_vector_at(self.unclone(base), 2, condition).unwrap_or_else(|| field(self, "min_spread"));
        let setting = |g: &Game, name: &str, d: f64| g.world.setting_f32(name).map_or(d, f64::from);
        let m = self.motion(actor);
        let aiming = actor == FormId::PLAYER_REF && self.jip2.told.aiming;
        let unaimed = if aiming { 0.0 } else { setting(self, "fUnaimedSpreadPenalty", 0.2) };
        let moving = if !m.moving {
            0.0
        } else if m.running {
            setting(self, "fRunningSpreadPenalty", 0.2)
        } else {
            setting(self, "fWalkingSpreadPenalty", 0.1)
        };
        let standing = if m.sneaking { 0.0 } else { setting(self, "fStandingSpreadPenalty", 0.1) };
        let skill_code = field(self, "skill") as u16;
        let skill = crate::functions::actor::actor_value(self, actor, skill_code).unwrap_or(0.0);
        let strength = crate::functions::actor::actor_value(self, actor, 5).unwrap_or(0.0);
        let str_req = (field(self, "strength_req") - strength.min(10.0)).max(0.0) * setting(self, "fWeapStrengthReqPenalty", 0.025);
        let skill_req = (field(self, "skill_req") - skill.min(100.0)).max(0.0) * setting(self, "fWeapSkillReqPenalty", 0.01);
        let mult = 1.0 - setting(self, "fWobbleToSkillConversion", 0.5) * skill.clamp(0.0, 100.0) / 100.0;
        let wobble = (mult * (unaimed + moving + standing + str_req + skill_req)).max(setting(self, "fMinGunSpreadValue", 0.01));
        (wobble, weapon)
    }
}

/// The condition-list function's family and its list's place among the arguments: the
/// entity, then `first` (FormTwo, FormThree) and `second` (FormThree).
fn list_key(args: &[XVal], family: u8) -> Option<(ListKey, usize)> {
    let f = form(args, 0)?;
    // An index outside i32 is no list, not a wrapped one [decision].
    let index = |k: usize, dflt: i64| i32::try_from(int(args, k, dflt)).ok();
    Some(match family {
        1 => (ListKey::One(f), 1),
        2 => (ListKey::Two(f, index(1, -1)?), 2),
        _ => {
            let first = index(1, -1)?;
            // "If First is (-1) then Second is ignored" [documented].
            let second = if first == -1 { 0 } else { index(2, 0)? };
            (ListKey::Three(f, first, second), 3)
        }
    })
}

/// The family of a condition-list function name (1 FormOne, 2 FormTwo, 3 FormThree) and
/// what it does.
fn family(name: &str) -> Option<(u8, &'static str)> {
    const VERBS: [(&str, &str); 6] = [
        ("getconditionform", "get"),
        ("setconditionform", "set"),
        ("addconditionform", "addcondition"),
        ("deleteconditionform", "deletecondition"),
        ("swapconditionform", "swapcondition"),
        ("evalconditionlistform", "evalconditionlist"),
    ];
    let (rest, verb) = VERBS.iter().find_map(|(p, v)| name.strip_prefix(p).map(|r| (r, *v)))?;
    let (fam, tail) = [("one", 1u8), ("two", 2), ("three", 3)].iter().find_map(|(w, f)| rest.strip_prefix(w).map(|t| (*f, t)))?;
    let op = match (verb, tail) {
        ("get", "count") => "getcount",
        ("get", "int") => "getint",
        ("get", "float") => "getfloat",
        ("get", "form") => "getform",
        ("set", "int") => "setint",
        ("set", "float") => "setfloat",
        ("set", "form") => "setform",
        (v, "") if v != "get" && v != "set" => v,
        _ => return None,
    };
    Some((fam, op))
}

/// The condition-list functions; `None` for another name.
fn condition_call(g: &mut Game, name: &str, args: &[XVal]) -> Option<XVal> {
    let (fam, op) = family(name)?;
    let fail_for = |op: &str| match op {
        "getcount" => XVal::Num(-1.0),
        "getint" | "getfloat" => XVal::Num(INT_MIN),
        "getform" => XVal::Ref(0),
        _ => XVal::Num(0.0),
    };
    let Some((key, at)) = list_key(args, fam) else { return Some(fail_for(op)) };
    let v = match op {
        // [documented: GetConditionFormOneCount, −1 when something is wrong].
        "getcount" => XVal::Num(g.one111_list(key).map_or(-1.0, |l| l.len() as f64)),
        // [documented: AddConditionFormOne: omitted or −1 adds at the end; a position
        // must hold a condition already, which moves down].
        "addcondition" => {
            let pos = int(args, at, -1);
            let Some(l) = g.one111_list_mut(key) else { return Some(XVal::Num(0.0)) };
            if pos == -1 {
                l.push(new_condition());
            } else if (0..l.len() as i64).contains(&pos) {
                l.insert(pos as usize, new_condition());
            } else {
                return Some(XVal::Num(0.0));
            }
            XVal::Num(1.0)
        }
        // [documented: DeleteConditionFormOne, "other Conditions are moved to fill the
        // gap"].
        "deletecondition" => {
            let pos = int(args, at, -1);
            let Some(l) = g.one111_list_mut(key) else { return Some(XVal::Num(0.0)) };
            if !(0..l.len() as i64).contains(&pos) {
                return Some(XVal::Num(0.0));
            }
            l.remove(pos as usize);
            XVal::Num(1.0)
        }
        "swapcondition" => {
            let (a, b) = (int(args, at, -1), int(args, at + 1, -1));
            let Some(l) = g.one111_list_mut(key) else { return Some(XVal::Num(0.0)) };
            let n = l.len() as i64;
            if !(0..n).contains(&a) || !(0..n).contains(&b) {
                return Some(XVal::Num(0.0));
            }
            l.swap(a as usize, b as usize);
            XVal::Num(1.0)
        }
        // [documented: EvalConditionListFormOne: 1 when the list passes with ActionRef as
        // the subject and TargetRef as the target; hypothesis: an empty list passes, as
        // the game's own lists do].
        "evalconditionlist" => {
            let Some(l) = g.one111_list(key) else { return Some(XVal::Num(0.0)) };
            let (subject, target) = (form(args, at), form(args, at + 1));
            yes(g.conditions_pass(&l, subject, target, "EvalConditionList"))
        }
        _ => {
            let (pos, id) = (int(args, at, -1), int(args, at + 1, -1));
            let Some(c) = g.one111_list(key).and_then(|l| usize::try_from(pos).ok().and_then(|p| l.get(p).cloned())) else { return Some(fail_for(op)) };
            match op {
                "getint" => XVal::Num(get_int(g, &c, id).map_or(INT_MIN, |v| v as f64)),
                "getfloat" => XVal::Num(get_float(g, &c, id).unwrap_or(INT_MIN)),
                "getform" => XVal::Ref(get_form(&c, id).map_or(0, |f| f.0)),
                _ => {
                    let mut c = c;
                    let ok = match op {
                        "setint" => set_int(g, &mut c, id, int(args, at + 2, 0)),
                        "setfloat" => set_float(g, &mut c, id, num(args, at + 2).unwrap_or(0.0)),
                        _ => set_form(g, &mut c, id, form(args, at + 2)),
                    };
                    if ok {
                        if let Some(l) = g.one111_list_mut(key) {
                            l[pos as usize] = c;
                        }
                    }
                    yes(ok)
                }
            }
        }
    };
    Some(v)
}

/// [documented: GetConditionFormOneInt's value IDs: 0 the function's opcode, 1 and 2 the
/// parameters' types, 3 the flags (bit 0 OR, bit 2 Use Global), 4 the comparison (0 ==
/// … 5 <=), 5 the run-on type (0 Subject … 4 Linked Reference)].
fn get_int(g: &Game, c: &Condition, id: i64) -> Option<i64> {
    Some(match id {
        0 => i64::from(c.opcode()),
        1 | 2 => g.one111_param_kind(c, (id - 1) as usize),
        3 => i64::from(u8::from(c.or)) | if matches!(c.rhs, Operand::Global(_)) { 4 } else { 0 },
        4 => CMPS.iter().position(|x| *x == c.cmp)? as i64,
        5 => match c.run_on {
            RunOn::Subject => 0,
            RunOn::Target => 1,
            RunOn::Reference(_) => 2,
            RunOn::CombatTarget => 3,
            RunOn::LinkedReference => 4,
            RunOn::Other(o) => i64::from(o),
        },
        _ => return None,
    })
}

/// [documented: GetConditionFormOneFloat's value IDs: 0 the value compared with, 1 and 2
/// the parameters when they are numbers]. A comparison with a global has no number
/// (INT_MIN; see docs/spec-notes/ext-one111.md §1).
fn get_float(g: &Game, c: &Condition, id: i64) -> Option<f64> {
    match id {
        0 => match c.rhs {
            Operand::Value(x) => Some(x),
            Operand::Global(_) => None,
        },
        1 | 2 => {
            let k = (id - 1) as usize;
            if g.one111_param_kind(c, k) > 1 {
                return None;
            }
            match c.params.get(k)? {
                ArgValue::Value(Value::Num(x)) => Some(*x),
                ArgValue::Enum(e) => Some(f64::from(*e)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// [documented: GetConditionFormOneForm's value IDs: 0 the global (Use Global on), 1 and 2
/// the parameters when they are forms, 3 the reference it runs on (run-on type 2)].
fn get_form(c: &Condition, id: i64) -> Option<FormId> {
    match id {
        0 => match c.rhs {
            Operand::Global(g) => g.map(FormId),
            Operand::Value(_) => None,
        },
        1 | 2 => match c.params.get((id - 1) as usize)? {
            ArgValue::Value(Value::Ref(r)) if *r != 0 => Some(FormId(*r)),
            _ => None,
        },
        3 => match c.run_on {
            RunOn::Reference(r) => r.map(FormId),
            _ => None,
        },
        _ => None,
    }
}

/// [documented: SetConditionFormOneInt: the parameters' types cannot be set, "they are
/// connected to the function itself"]. A new function keeps a parameter of the same kind
/// and clears the others; an opcode no condition function has is refused. Turning Use
/// Global on or off clears the value compared with [documented: "firstly make the value
/// 0 … or the game might crash"] (see docs/spec-notes/ext-one111.md §1).
fn set_int(g: &Game, c: &mut Condition, id: i64, v: i64) -> bool {
    match id {
        0 => {
            let Ok(op) = u16::try_from(v) else { return false };
            if !(0x1000..0x2000).contains(&op) || op != GET_VAMPIRE && !g.data.param_types.0.contains_key(&(op - 0x1000)) {
                return false;
            }
            let types = g.one111_param_types(op - 0x1000);
            let old = std::mem::take(&mut c.params);
            c.function = op - 0x1000;
            c.params = types.iter().take(2).enumerate().map(|(k, t)| old.get(k).filter(|p| reading(p) == type_reading(t)).cloned().unwrap_or_else(|| blank(t))).collect();
            true
        }
        3 => {
            c.or = v & 1 != 0;
            let global = v & 4 != 0;
            match (global, c.rhs) {
                (true, Operand::Value(_)) => c.rhs = Operand::Global(None),
                (false, Operand::Global(_)) => c.rhs = Operand::Value(0.0),
                _ => {}
            }
            true
        }
        4 => match usize::try_from(v).ok().and_then(|i| CMPS.get(i)) {
            Some(cmp) => {
                c.cmp = *cmp;
                true
            }
            None => false,
        },
        5 => {
            c.run_on = match v {
                0 => RunOn::Subject,
                1 => RunOn::Target,
                // Keeps the reference it had [hypothesis].
                2 => match c.run_on {
                    RunOn::Reference(r) => RunOn::Reference(r),
                    _ => RunOn::Reference(None),
                },
                3 => RunOn::CombatTarget,
                4 => RunOn::LinkedReference,
                _ => return false,
            };
            true
        }
        // The parameters' types cannot be set [documented].
        _ => false,
    }
}

/// How a parameter is held: 'i' a number, 'e' an enumeration code, 'f' a form.
fn reading(p: &ArgValue) -> char {
    match p {
        ArgValue::Value(Value::Ref(_)) => 'f',
        ArgValue::Enum(_) => 'e',
        _ => 'i',
    }
}

/// How `crate::cond` holds a parameter of an xEdit type.
fn type_reading(t: &str) -> char {
    match param_kind(t) {
        0 if INTS.contains(&t) => 'i',
        0 => 'e',
        _ => 'f',
    }
}

/// A cleared parameter of an xEdit type.
fn blank(t: &str) -> ArgValue {
    match type_reading(t) {
        'i' => ArgValue::Value(Value::Num(0.0)),
        'e' => ArgValue::Enum(0),
        _ => ArgValue::Value(Value::NONE),
    }
}

/// The xEdit parameter types read as plain integers (the rest of the numbers are
/// enumeration codes), as `crate::cond` reads them.
const INTS: &[&str] = &["ptInteger", "ptQuestStage", "ptQuestObjective", "ptVariableName", "ptVATSValueParam"];

/// [documented: SetConditionFormOneFloat: the value only with Use Global off, except 0;
/// the parameters when they are numbers, or 0 whatever their type].
fn set_float(g: &Game, c: &mut Condition, id: i64, v: f64) -> bool {
    match id {
        0 => match c.rhs {
            Operand::Value(_) => {
                c.rhs = Operand::Value(v);
                true
            }
            // 0 clears the global [hypothesis: what "setting it to 0" does with the flag
            // on].
            Operand::Global(_) if v == 0.0 => {
                c.rhs = Operand::Global(None);
                true
            }
            Operand::Global(_) => false,
        },
        1 | 2 => {
            let k = (id - 1) as usize;
            let kind = g.one111_param_kind(c, k);
            if kind == -1 {
                return false;
            }
            let p = if kind <= 1 {
                match g.one111_param_types(c.function).get(k) {
                    Some(t) if type_reading(t) == 'e' => ArgValue::Enum(v.clamp(0.0, f64::from(u16::MAX)) as u16),
                    _ => ArgValue::Value(Value::Num(v)),
                }
            } else if v == 0.0 {
                ArgValue::Value(Value::NONE)
            } else {
                return false;
            };
            while c.params.len() <= k {
                c.params.push(ArgValue::Value(Value::Num(0.0)));
            }
            c.params[k] = p;
            true
        }
        _ => false,
    }
}

/// [documented: SetConditionFormOneForm: the global only with Use Global on; the
/// parameters when they are forms, or any parameter of GetVampire (opcode 4136); the
/// reference it runs on]. A global must be a GLOB and the run-on reference needs run-on
/// type 2 (see docs/spec-notes/ext-one111.md §1).
fn set_form(g: &Game, c: &mut Condition, id: i64, v: Option<FormId>) -> bool {
    match id {
        0 => match (c.rhs, v) {
            (Operand::Global(_), Some(f)) if g.data.sig(f) == Some(*b"GLOB") => {
                c.rhs = Operand::Global(Some(f.0));
                true
            }
            _ => false,
        },
        1 | 2 => {
            let k = (id - 1) as usize;
            let any = c.opcode() == GET_VAMPIRE;
            if !any && !matches!(g.one111_param_kind(c, k), 2 | 3) {
                return false;
            }
            while c.params.len() <= k {
                c.params.push(ArgValue::Value(Value::Num(0.0)));
            }
            c.params[k] = ArgValue::Value(Value::Ref(v.map_or(0, |f| f.0)));
            true
        }
        // Accepted whatever the run-on type [observed: one111, 1 on a condition run on
        // the subject]; NVForever keeps the reference only with run-on type 2, where it
        // has a place for it [hypothesis: the game keeps it for a later change to 2].
        3 => {
            if let RunOn::Reference(_) = c.run_on {
                c.run_on = RunOn::Reference(v.map(|f| f.0));
            }
            true
        }
        _ => false,
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    if let Some(v) = condition_call(g, name, args) {
        return Some(Ok(v));
    }
    let zero = XVal::Num(0.0);
    // A weapon argument: its base form (a clone stands for its original).
    let weapon = |g: &Game, k: usize| form(args, k).map(|f| g.unclone(crate::functions::base_of(g, f).unwrap_or(f)));
    let v = match name {
        // ---- the damage calculation ----
        // [documented: GetOnDamageCalcNumb: IDs 0–9; INT_MIN for a value not set].
        "getondamagecalcnumb" => {
            let i = int(args, 0, -1);
            XVal::Num(g.one111.damage_calc.as_ref().and_then(|d| usize::try_from(i).ok().and_then(|i| d.numbers.get(i).copied().flatten())).unwrap_or(INT_MIN))
        }
        // [documented: GetOnDamageCalcAdditionalNumb: IDs 0–5]; 0 outside a damage
        // event [observed: one111]; in one, INT_MIN for one not calculated [hypothesis:
        // as GetOnDamageCalcNumb].
        "getondamagecalcadditionalnumb" => {
            let i = int(args, 0, -1);
            XVal::Num(g.one111.damage_calc.as_ref().map_or(0.0, |d| usize::try_from(i).ok().and_then(|i| d.additional.get(i).copied().flatten()).unwrap_or(INT_MIN)))
        }
        // [documented: GetOnDamageCalcFlag, the HitData flags as one number]; −1 outside
        // a damage event [observed: one111].
        "getondamagecalcflag" => XVal::Num(g.one111.damage_calc.as_ref().map_or(-1.0, |d| f64::from(d.flags))),
        // [documented: GetOnDamageCalcRef: 0 source, 1 target, 2 weapon, 3 projectile].
        "getondamagecalcref" => {
            let i = int(args, 0, -1);
            XVal::Ref(g.one111.damage_calc.as_ref().and_then(|d| usize::try_from(i).ok().and_then(|i| d.refs.get(i).copied().flatten())).map_or(0, |f| f.0))
        }
        // [documented: SetOnDamageCalcNumb, the values GetOnDamageCalcNumb gives;
        // hypothesis: 1 when set, 0 outside an event or for another ID].
        "setondamagecalcnumb" => {
            let (i, x) = (int(args, 0, -1), num(args, 1).unwrap_or(0.0));
            match g.one111.damage_calc.as_mut().and_then(|d| usize::try_from(i).ok().and_then(|i| d.numbers.get_mut(i))) {
                Some(slot) => {
                    *slot = Some(x);
                    XVal::Num(1.0)
                }
                None => zero,
            }
        }
        // ---- the weapons' additional stats ----
        // `weapon type health value`: the pair, replacing one with the same health; 1 when
        // set [documented: SetWeapAdditionalVectorValue; GetWeapAdditionalVectorValue, the
        // types 0–6, health 0–1, "the second value can only be a positive number or
        // zero"; hypothesis: a health outside 0–1 or a form that is not a weapon refused;
        // decision: a value that is not a finite number refused, so no NaN reaches combat].
        "setweapadditionalvectorvalue" => {
            let (Some(w), Some(t), Some(h), Some(x)) = (weapon(g, 0), num(args, 1), num(args, 2), num(args, 3)) else { return Some(Ok(zero)) };
            if g.data.sig(w) != Some(*b"WEAP") || !(0.0..=6.0).contains(&t) || t.fract() != 0.0 || !(0.0..=1.0).contains(&h) || !x.is_finite() || x < 0.0 {
                return Some(Ok(zero));
            }
            let v = g.one111.vectors.entry((w, t as u8)).or_default();
            v.retain(|p| (p.0 - h).abs() > 1e-6);
            v.push((h, x));
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            XVal::Num(1.0)
        }
        // [documented: GetWeapAdditionalVectorValue, the second value of the pair with
        // that health, −1 when there is none].
        "getweapadditionalvectorvalue" => {
            let (w, t, h) = (weapon(g, 0), int(args, 1, -1), num(args, 2).unwrap_or(-1.0));
            let v = w.and_then(|w| u8::try_from(t).ok().and_then(|t| g.one111.vectors.get(&(w, t))));
            XVal::Num(v.and_then(|v| v.iter().find(|p| (p.0 - h).abs() <= 1e-6)).map_or(-1.0, |p| p.1))
        }
        // [documented: GetWeapAdditionalVectorValueScale: between two pairs the linear
        // proportion, beyond them the nearest pair's, −1 for an empty vector].
        "getweapadditionalvectorvaluescale" => {
            let (w, t, h) = (weapon(g, 0), int(args, 1, -1), num(args, 2).unwrap_or(0.0));
            XVal::Num(w.zip(u8::try_from(t).ok()).and_then(|(w, t)| g.one111_vector_at(w, t, h)).unwrap_or(-1.0))
        }
        // [documented: GetWeapAdditionalVectorArray, the first values, ordered].
        "getweapadditionalvectorarray" => {
            let (w, t) = (weapon(g, 0), int(args, 1, -1));
            let firsts: Vec<f64> = w.and_then(|w| u8::try_from(t).ok().and_then(|t| g.one111.vectors.get(&(w, t)))).map(|v| v.iter().map(|p| p.0).collect()).unwrap_or_default();
            // An empty vector gives no array [observed: one111, ar_Size −1 for the 10mm
            // pistol's].
            if firsts.is_empty() {
                return Some(Ok(XVal::Array(0)));
            }
            let entries = firsts.into_iter().enumerate().map(|(i, x)| (XKey::Num(i as f64), XVal::Num(x))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        // [documented: DeleteWeapAdditionalVectorValue, 1 when the pair was there].
        "deleteweapadditionalvectorvalue" => {
            let (w, t, h) = (weapon(g, 0), int(args, 1, -1), num(args, 2).unwrap_or(-1.0));
            let Some(v) = w.and_then(|w| u8::try_from(t).ok().and_then(|t| g.one111.vectors.get_mut(&(w, t)))) else { return Some(Ok(zero)) };
            let before = v.len();
            v.retain(|p| (p.0 - h).abs() > 1e-6);
            yes(v.len() != before)
        }
        // "There is not flags for now" [documented: GetWeapAdditionalFlag]: −1
        // [observed: one111, the 10mm pistol].
        "getweapadditionalflag" => XVal::Num(-1.0),
        // `[type]`: 0 the full spread, 1 the wobble, 2 the weapon's [documented:
        // GetCalculatedSpreadAlt].
        "getcalculatedspreadalt" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let (wobble, weapon) = g.one111_spread(a);
            XVal::Num(match int(args, 0, 0) {
                1 => wobble,
                2 => weapon,
                _ => wobble + weapon,
            })
        }
        // [documented: GetActorTotalArmorDR; GetActorTotalArmorDT].
        "getactortotalarmordr" | "getactortotalarmordt" => match subject {
            Some(a) => XVal::Num(g.one111_armor_total(a, name.ends_with("dt"))),
            None => zero,
        },
        // ---- ammunition ----
        // `ammo effectType value`: the value through each of the ammo's effects of that
        // type, in record order: 0 add, 1 multiply, 2 subtract [documented:
        // ApplyAmmoEffects, "Ammo10mmHollowPoint 0 2" gives 3.5 with its 1.75 damage
        // multiplier; SetAmmoEffectTraitNumericAlt, the types and operations].
        "applyammoeffects" => {
            let (Some(ammo), Some(t)) = (form(args, 0), num(args, 1)) else { return Some(Ok(XVal::Num(num(args, 2).unwrap_or(0.0)))) };
            let x = num(args, 2).unwrap_or(0.0);
            if !t.is_finite() || t.fract() != 0.0 || !(0.0..=255.0).contains(&t) {
                return Some(Ok(XVal::Num(x)));
            }
            // AddAmmoEffect's, RemoveAmmoEffect's, and live AMEF trait changes first
            // (`crate::nvse_jip7`), keeping TempCloneForm AMMO/AMEF clones isolated.
            XVal::Num(g.apply_ammo_effects_value(ammo, t as u8, x))
        }
        // `effect trait value`: 0 the type (0–9, One111 adds 6–9), 1 the operation (0–2),
        // 2 the value; 1 when set [documented: SetAmmoEffectTraitNumericAlt]. Read back
        // by GetAmmoEffectTraitNumeric; the engine's ammo effects do not read it yet
        // (ext-one111.md §4).
        "setammoeffecttraitnumericalt" => {
            let (Some(e), Some(t), Some(x)) = (form(args, 0), num(args, 1), num(args, 2)) else { return Some(Ok(zero)) };
            if g.data.sig(g.unclone(e)) != Some(*b"AMEF") {
                return Some(Ok(zero));
            }
            let (key, ok) = match t as i64 {
                0 => ("type", (0.0..=9.0).contains(&x) && x.fract() == 0.0),
                1 => ("operation", (0.0..=2.0).contains(&x) && x.fract() == 0.0),
                // [decision: a value that is not a finite number refused].
                2 => ("value", x.is_finite()),
                _ => ("", false),
            };
            if !ok {
                return Some(Ok(zero));
            }
            g.field_overrides.insert((e, format!("DATA/{key}")), XVal::Num(x));
            XVal::Num(1.0)
        }
        // [documented: GetProjectileRefAmmo, the ammo the projectile was made with].
        "getprojectilerefammo" => XVal::Ref(subject.and_then(|r| g.gaps.projs.shots.get(&r)).and_then(|s| s.ammo).map_or(0, |a| a.0)),
        // ---- weapon fields ----
        // `weapon slot effect`: slot 1–3, the effect 0–26 (One111's list) [documented:
        // SetWeaponItemModEffectAlt]; read back by GetWeaponItemModEffect.
        "setweaponitemmodeffectalt" | "setweaponitemmodvalue2" => {
            let (Some(w), Some(slot), Some(x)) = (weapon(g, 0), num(args, 1), num(args, 2)) else { return Some(Ok(zero)) };
            if g.data.sig(w) != Some(*b"WEAP") || !(1.0..=3.0).contains(&slot) || slot.fract() != 0.0 {
                return Some(Ok(zero));
            }
            let path = if name == "setweaponitemmodeffectalt" {
                if !(0.0..=26.0).contains(&x) || x.fract() != 0.0 {
                    return Some(Ok(zero));
                }
                format!("DNAM/effect_mod{slot}")
            } else {
                // [documented: SetWeaponItemModValue2, "mod effect value two"].
                format!("DNAM/value_b_mod{slot}")
            };
            g.field_overrides.insert((w, path), XVal::Num(x));
            XVal::Num(1.0)
        }
        // [documented: GetWeaponScopeEffectShader; SetWeaponScopeEffectShader]: WEAP
        // EFSD "Scope Effect" [documented: the Layer 1 schema]; the setter takes an effect
        // shader (EFSH; see docs/spec-notes/ext-one111.md §4).
        "getweaponscopeeffectshader" => match weapon(g, 0) {
            Some(w) => match crate::nvse::record_value(g, w, "EFSD", "") {
                Some(XVal::Ref(r)) => XVal::Ref(r),
                _ => XVal::Ref(0),
            },
            None => XVal::Ref(0),
        },
        "setweaponscopeeffectshader" => {
            let (Some(w), Some(s)) = (weapon(g, 0), form(args, 1)) else { return Some(Ok(zero)) };
            if g.data.sig(w) != Some(*b"WEAP") || g.data.sig(g.unclone(s)) != Some(*b"EFSH") {
                return Some(Ok(zero));
            }
            g.field_overrides.insert((w, "EFSD/".to_string()), XVal::Ref(s.0));
            XVal::Num(1.0)
        }
        _ => return None,
    };
    Some(Ok(v))
}

#[cfg(test)]
mod tests {
    use super::family;

    #[test]
    fn names_split_into_family_and_operation() {
        assert_eq!(family("getconditionformonecount"), Some((1, "getcount")));
        assert_eq!(family("setconditionformthreefloat"), Some((3, "setfloat")));
        assert_eq!(family("addconditionformtwo"), Some((2, "addcondition")));
        assert_eq!(family("evalconditionlistformthree"), Some((3, "evalconditionlist")));
        assert_eq!(family("getweaponscopeeffectshader"), None);
    }
}
